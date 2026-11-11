//! Secure channel of Wi-Fi connections (protocol §13, `docs/research/secure-channel.typ`).
//!
//! TLS 1.3, or 1.2 with ECDHE + AEAD suites only, with *pinned self-signed certificates* on both
//! sides: no certificate authority. The computer is the TLS server; each side recognises the other
//! by the SHA-256 of its certificate ([`fingerprint`]). Which phones are allowed is decided after
//! the handshake, from the phone's `HELLO` (see `pairing.rs`).
//!
//! The session code reads and writes from two threads, but a `rustls` connection is one object:
//! [`accept`] hands the session one end of a local socket pair and a *pump* thread moves bytes
//! between the other end and the TLS connection.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rustls::client::danger::HandshakeSignatureValid;
use rustls::crypto::{ring as provider, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    DigitallySignedStruct, DistinguishedName, ServerConfig, ServerConnection, SignatureScheme,
};

/// Time allowed for the TLS handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Decrypted bytes waiting for the session before the pump stops reading the network
/// (the phone then drops frames: latency never piles up).
const BACKLOG: usize = 1024 * 1024;

/// The computer's certificate and private key (DER), created once.
pub struct Identity {
    pub cert: Vec<u8>,
    key: Vec<u8>,
}

impl Identity {
    /// Loads `tls-cert.der` and `tls-key.der` from `dir`, or creates them (ECDSA P-256).
    pub fn load_or_create(dir: &Path) -> Result<Self, String> {
        let (cert_path, key_path) = (dir.join("tls-cert.der"), dir.join("tls-key.der"));
        if let (Ok(cert), Ok(key)) = (fs::read(&cert_path), fs::read(&key_path)) {
            if !cert.is_empty() && !key.is_empty() {
                return Ok(Self { cert, key });
            }
        }
        let id = Self::generate()?;
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        write_private(&key_path, &id.key)?;
        fs::write(&cert_path, &id.cert)
            .map_err(|e| format!("cannot write {}: {e}", cert_path.display()))?;
        Ok(id)
    }

    /// A new self-signed ECDSA P-256 certificate.
    pub fn generate() -> Result<Self, String> {
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)
            .map_err(|e| format!("cannot create the TLS key: {e}"))?;
        let mut params = rcgen::CertificateParams::new(vec!["oculibre-host".to_string()])
            .map_err(|e| e.to_string())?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Oculibre computer");
        let cert = params
            .self_signed(&key)
            .map_err(|e| format!("cannot create the TLS certificate: {e}"))?;
        Ok(Self {
            cert: cert.der().to_vec(),
            key: key.serialize_der(),
        })
    }

    pub fn fingerprint(&self) -> String {
        fingerprint(&self.cert)
    }
}

/// Writes a private key readable by the owner only.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// SHA-256 of a DER certificate, lowercase hex: what each side pins.
pub fn fingerprint(cert_der: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, cert_der)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `a1b2 c3d4 e5f6 a7b8`: the start of a fingerprint, for people to compare.
pub fn short_fingerprint(fp: &str) -> String {
    fp.as_bytes()
        .chunks(4)
        .take(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn crypto() -> Arc<CryptoProvider> {
    Arc::new(provider::default_provider())
}

/// TLS 1.3 and 1.2 (rustls only has ECDHE + AEAD suites), client certificate required.
pub fn server_config(identity: &Identity) -> Result<Arc<ServerConfig>, String> {
    let crypto = crypto();
    let config = ServerConfig::builder_with_provider(crypto.clone())
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|e| e.to_string())?
        .with_client_cert_verifier(Arc::new(AnyClientCert(crypto)))
        .with_single_cert(
            vec![CertificateDer::from(identity.cert.clone())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key.clone())),
        )
        .map_err(|e| format!("bad TLS identity: {e}"))?;
    Ok(Arc::new(config))
}

/// Requires a client certificate and checks that the client holds its key (the handshake
/// signature), but trusts no authority: whether this phone is allowed is decided afterwards from
/// its fingerprint.
#[derive(Debug)]
struct AnyClientCert(Arc<CryptoProvider>);

impl ClientCertVerifier for AnyClientCert {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// A phone after a successful TLS handshake.
pub struct Accepted {
    /// Plaintext side for the session (the pump thread does the TLS).
    pub link: UnixStream,
    /// SHA-256 of the phone's certificate.
    pub fingerprint: String,
    /// `TLSv1_3` or `TLSv1_2`.
    pub version: String,
}

/// Runs the TLS handshake on a new connection and starts the pump.
pub fn accept(tcp: TcpStream, config: Arc<ServerConfig>) -> Result<Accepted, String> {
    let mut conn = ServerConnection::new(config).map_err(|e| e.to_string())?;
    let mut tcp = tcp;
    let _ = tcp.set_nodelay(true);
    tcp.set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .and_then(|_| tcp.set_write_timeout(Some(HANDSHAKE_TIMEOUT)))
        .map_err(|e| e.to_string())?;
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp).map_err(|e| match e.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => {
                "no TLS handshake within 5 s (an old app, or not an Oculibre phone?)".to_string()
            }
            _ => format!("TLS handshake failed: {e}"),
        })?;
    }
    let fingerprint = conn
        .peer_certificates()
        .and_then(|c| c.first())
        .map(|c| fingerprint(c))
        .ok_or("the phone sent no certificate")?;
    let version = conn
        .protocol_version()
        .map(|v| format!("{v:?}"))
        .unwrap_or_default();

    let (session_end, pump_end) = UnixStream::pair().map_err(|e| e.to_string())?;
    thread::Builder::new()
        .name("oculibre-tls".into())
        .spawn(move || pump(conn, tcp, pump_end))
        .map_err(|e| e.to_string())?;
    Ok(Accepted {
        link: session_end,
        fingerprint,
        version,
    })
}

/// Moves bytes both ways between the TLS connection and the session, until either side closes.
fn pump(mut conn: ServerConnection, mut tcp: TcpStream, mut local: UnixStream) {
    let closed = |tcp: &TcpStream, local: &UnixStream| {
        let _ = tcp.shutdown(Shutdown::Both);
        let _ = local.shutdown(Shutdown::Both);
    };
    if tcp.set_read_timeout(None).is_err()
        || tcp.set_write_timeout(None).is_err()
        || tcp.set_nonblocking(true).is_err()
        || local.set_nonblocking(true).is_err()
    {
        return closed(&tcp, &local);
    }
    let mut to_local: Vec<u8> = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut peer_gone = false;
    loop {
        // Decrypted bytes → session.
        while to_local.len() < BACKLOG {
            match conn.reader().read(&mut buf) {
                Ok(0) => {
                    peer_gone = true; // close_notify
                    break;
                }
                Ok(n) => to_local.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    peer_gone = true; // closed without close_notify
                    break;
                }
            }
        }
        while !to_local.is_empty() {
            match local.write(&to_local) {
                Ok(n) => {
                    to_local.drain(..n);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => return closed(&tcp, &local), // the session is gone
            }
        }
        if peer_gone && to_local.is_empty() {
            return closed(&tcp, &local);
        }
        // Encrypted bytes → network.
        while conn.wants_write() {
            match conn.write_tls(&mut tcp) {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => return closed(&tcp, &local),
            }
        }

        let mut fds = [
            libc::pollfd {
                fd: tcp.as_raw_fd(),
                events: (if !peer_gone && to_local.len() < BACKLOG {
                    libc::POLLIN
                } else {
                    0
                }) | (if conn.wants_write() { libc::POLLOUT } else { 0 }),
                revents: 0,
            },
            libc::pollfd {
                fd: local.as_raw_fd(),
                events: libc::POLLIN
                    | (if to_local.is_empty() {
                        0
                    } else {
                        libc::POLLOUT
                    }),
                revents: 0,
            },
        ];
        // SAFETY: two valid pollfd entries owned by this frame.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if n < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return closed(&tcp, &local);
        }
        let ready = |r: i16| r & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0;

        if ready(fds[0].revents) {
            match conn.read_tls(&mut tcp) {
                Ok(0) => peer_gone = true,
                Ok(_) => {
                    if conn.process_new_packets().is_err() {
                        // Tell the phone why (TLS alert), then give up.
                        while conn.wants_write() && conn.write_tls(&mut tcp).is_ok() {}
                        return closed(&tcp, &local);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(_) => peer_gone = true,
            }
        }
        if ready(fds[1].revents) {
            match local.read(&mut buf) {
                Ok(0) | Err(_) => {
                    // The session ended: close the TLS connection properly.
                    conn.send_close_notify();
                    let _ = tcp.set_nonblocking(false);
                    let _ = tcp.set_write_timeout(Some(Duration::from_secs(1)));
                    while conn.wants_write() && conn.write_tls(&mut tcp).is_ok() {}
                    return closed(&tcp, &local);
                }
                Ok(n) => {
                    if conn.writer().write_all(&buf[..n]).is_err() {
                        return closed(&tcp, &local);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rustls::client::danger::{ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::ServerName;
    use rustls::{ClientConfig, ClientConnection, StreamOwned};
    use std::net::TcpListener;

    /// Pins the server's certificate fingerprint, like the phone does.
    #[derive(Debug)]
    pub struct Pinned(pub String, pub Arc<CryptoProvider>);

    impl ServerCertVerifier for Pinned {
        fn verify_server_cert(
            &self,
            end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            if fingerprint(end_entity) == self.0 {
                Ok(ServerCertVerified::assertion())
            } else {
                Err(rustls::Error::General("not the paired computer".into()))
            }
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(
                message,
                cert,
                dss,
                &self.1.signature_verification_algorithms,
            )
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(
                message,
                cert,
                dss,
                &self.1.signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.1.signature_verification_algorithms.supported_schemes()
        }
    }

    /// A phone's TLS client: its own identity, the computer's pinned fingerprint.
    pub fn client(
        phone: &Identity,
        host_fp: &str,
        version: &'static rustls::SupportedProtocolVersion,
        tcp: TcpStream,
    ) -> StreamOwned<ClientConnection, TcpStream> {
        let crypto = crypto();
        let config = ClientConfig::builder_with_provider(crypto.clone())
            .with_protocol_versions(&[version])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Pinned(host_fp.into(), crypto)))
            .with_client_auth_cert(
                vec![CertificateDer::from(phone.cert.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(phone.key.clone())),
            )
            .unwrap();
        let conn =
            ClientConnection::new(Arc::new(config), ServerName::try_from("oculibre").unwrap())
                .unwrap();
        StreamOwned::new(conn, tcp)
    }

    fn pair_of_sockets() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        (client, listener.accept().unwrap().0)
    }

    /// Both TLS versions: 2 MB each way at the same time, then a clean close from the session.
    #[test]
    fn pump_carries_both_ways_and_closes() {
        for version in [&rustls::version::TLS13, &rustls::version::TLS12] {
            let host = Identity::generate().unwrap();
            let phone = Identity::generate().unwrap();
            let config = server_config(&host).unwrap();
            let (c, s) = pair_of_sockets();
            let host_fp = host.fingerprint();
            let phone_side = thread::spawn(move || {
                let mut tls = client(&phone, &host_fp, version, c);
                let up: Vec<u8> = (0..2_000_000u32).map(|i| (i % 251) as u8).collect();
                tls.write_all(&up).unwrap();
                tls.flush().unwrap();
                let mut down = Vec::new();
                tls.read_to_end(&mut down).unwrap(); // until the host closes
                (down, phone.fingerprint())
            });
            let accepted = accept(s, config).unwrap();
            assert_eq!(
                accepted.version,
                if version == &rustls::version::TLS13 {
                    "TLSv1_3"
                } else {
                    "TLSv1_2"
                }
            );
            let mut link = accepted.link;
            let writer = {
                let mut w = link.try_clone().unwrap();
                thread::spawn(move || {
                    let down: Vec<u8> = (0..2_000_000u32).map(|i| (i % 241) as u8).collect();
                    w.write_all(&down).unwrap();
                })
            };
            let mut up = vec![0u8; 2_000_000];
            link.read_exact(&mut up).unwrap();
            assert!(up.iter().enumerate().all(|(i, &b)| b == (i % 251) as u8));
            writer.join().unwrap();
            link.shutdown(Shutdown::Both).unwrap();
            let (down, phone_fp) = phone_side.join().unwrap();
            assert_eq!(down.len(), 2_000_000);
            assert!(down.iter().enumerate().all(|(i, &b)| b == (i % 241) as u8));
            assert_eq!(accepted.fingerprint, phone_fp);
        }
    }

    #[test]
    fn a_phone_pinning_another_computer_fails() {
        let host = Identity::generate().unwrap();
        let other = Identity::generate().unwrap();
        let phone = Identity::generate().unwrap();
        let config = server_config(&host).unwrap();
        let (c, s) = pair_of_sockets();
        let other_fp = other.fingerprint();
        let phone_side = thread::spawn(move || {
            let mut tls = client(&phone, &other_fp, &rustls::version::TLS13, c);
            tls.write_all(b"HELLO").is_err() || tls.flush().is_err()
        });
        assert!(accept(s, config).is_err());
        assert!(phone_side.join().unwrap());
    }

    #[test]
    fn identity_is_created_once() {
        let dir = std::env::temp_dir().join(format!("oculibre-tls-test-{}", std::process::id()));
        let a = Identity::load_or_create(&dir).unwrap();
        let b = Identity::load_or_create(&dir).unwrap();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.fingerprint().len(), 64);
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.join("tls-key.der"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(short_fingerprint(&a.fingerprint()).len(), 19);
    }
}
