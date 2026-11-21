//! Secure channel of Wi-Fi connections (protocol §13, `docs/research/secure-channel.typ`).
//!
//! TLS 1.3, or 1.2 with ECDHE + AEAD suites only, with *pinned self-signed certificates* on both
//! sides: no certificate authority. The computer is the TLS server; each side recognises the other
//! by the SHA-256 of its certificate ([`fingerprint`]). Which phones are allowed is decided after
//! the handshake, from the phone's `HELLO` (see `pairing.rs`).
//!
//! The session code reads and writes from two threads, but a `rustls` connection is one object:
//! [`accept`] hands the session a [`TlsLink`], which shares the connection behind a lock. The
//! reading thread waits for the network *without* the lock (`peek`), then takes it to decrypt; a
//! write takes it to encrypt and send. No extra thread, no socket pair, nothing specific to an
//! operating system.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
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
    crate::store::create_private(path)
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
/// `tls12_only`: for testing the path of Android 5–9 phones on a recent phone.
pub fn server_config(identity: &Identity, tls12_only: bool) -> Result<Arc<ServerConfig>, String> {
    let crypto = crypto();
    let versions: &[&rustls::SupportedProtocolVersion] = if tls12_only {
        &[&rustls::version::TLS12]
    } else {
        &[&rustls::version::TLS13, &rustls::version::TLS12]
    };
    let config = ServerConfig::builder_with_provider(crypto.clone())
        .with_protocol_versions(versions)
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
    /// The session's byte stream: plaintext in and out, TLS on the network.
    pub link: TlsLink,
    /// SHA-256 of the phone's certificate.
    pub fingerprint: String,
    /// `TLSv1_3` or `TLSv1_2`.
    pub version: String,
}

/// Runs the TLS handshake on a new connection.
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
    // From now on the session decides (its own read timeout; writes wait for the phone).
    tcp.set_read_timeout(None)
        .and_then(|_| tcp.set_write_timeout(None))
        .map_err(|e| e.to_string())?;
    Ok(Accepted {
        link: TlsLink::new(conn, tcp),
        fingerprint,
        version,
    })
}

/// A TLS connection as the session's [`Link`](crate::session::Link): reading decrypts, writing
/// encrypts. Clones share the connection (one reading thread, any number of writers).
///
/// Two locks, so that nobody waits for the network while holding the connection: encrypting and
/// decrypting happen under `shared` (quick), and encrypted bytes wait in `outbox` until whoever
/// holds `sending` writes them to the socket. Otherwise a writer blocked on a full socket would
/// keep the reader from reading, while the phone waits for us to read before it reads: deadlock.
pub struct TlsLink {
    shared: Arc<Mutex<Shared>>,
    sending: Arc<Mutex<()>>,
    tcp: TcpStream,
}

struct Shared {
    conn: ServerConnection,
    /// Encrypted bytes not sent yet, in order.
    outbox: Vec<u8>,
}

impl Shared {
    /// Moves what rustls has to send (records, alerts, session tickets) to the outbox.
    fn encrypt_pending(&mut self) {
        while self.conn.wants_write() {
            if self.conn.write_tls(&mut self.outbox).is_err() {
                break; // a Vec never fails
            }
        }
    }
}

impl TlsLink {
    fn new(conn: ServerConnection, tcp: TcpStream) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                conn,
                outbox: Vec::new(),
            })),
            sending: Arc::new(Mutex::new(())),
            tcp,
        }
    }

    fn lock(&self) -> io::Result<std::sync::MutexGuard<'_, Shared>> {
        self.shared
            .lock()
            .map_err(|_| io::Error::other("TLS connection poisoned"))
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            shared: self.shared.clone(),
            sending: self.sending.clone(),
            tcp: self.tcp.try_clone()?,
        })
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.tcp.set_read_timeout(timeout)
    }

    /// Writes the outbox to the socket. `wait`: take the turn to send even if someone else is
    /// sending (writers); otherwise leave it to them (the reader must never block here).
    fn send(&self, wait: bool) -> io::Result<()> {
        loop {
            let turn = if wait {
                self.sending
                    .lock()
                    .map_err(|_| io::Error::other("TLS sending poisoned"))?
            } else {
                match self.sending.try_lock() {
                    Ok(turn) => turn,
                    Err(_) => return Ok(()), // the one sending will take our bytes too
                }
            };
            loop {
                let bytes = std::mem::take(&mut self.lock()?.outbox);
                if bytes.is_empty() {
                    break;
                }
                (&self.tcp).write_all(&bytes)?;
            }
            drop(turn);
            // Bytes added between our last look and the end of our turn: send them too.
            if self.lock()?.outbox.is_empty() {
                return Ok(());
            }
        }
    }

    /// Closes properly (TLS `close_notify`), then the socket: the other side's reads end.
    pub fn shutdown(&self) -> io::Result<()> {
        if let Ok(mut shared) = self.lock() {
            shared.conn.send_close_notify();
            shared.encrypt_pending();
        }
        let _ = self.tcp.set_write_timeout(Some(Duration::from_secs(1)));
        let _ = self.send(true);
        let result = self.tcp.shutdown(Shutdown::Both);
        crate::session::wake_readers(&self.tcp);
        result
    }
}

impl Read for TlsLink {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            match self.lock()?.conn.reader().read(buf) {
                Ok(n) => return Ok(n), // 0: the phone closed properly (close_notify)
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e), // closed without close_notify
            }
            // Wait for the network without any lock (writers go on meanwhile). Honours the read
            // timeout the session set.
            self.tcp.peek(&mut [0u8; 1])?;
            let failed = {
                let mut shared = self.lock()?;
                let mut tcp = &self.tcp;
                if shared.conn.read_tls(&mut tcp)? == 0 {
                    return Ok(0); // the socket closed
                }
                let result = shared.conn.process_new_packets().err();
                shared.encrypt_pending(); // answers, or the alert telling the phone why
                result
            };
            self.send(false)?;
            if let Some(e) = failed {
                return Err(io::Error::new(io::ErrorKind::InvalidData, e));
            }
        }
    }
}

impl Write for TlsLink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        {
            let mut shared = self.lock()?;
            let mut rest = buf;
            while !rest.is_empty() {
                // rustls takes what fits in its buffer; encrypting empties it.
                let n = shared.conn.writer().write(rest)?;
                shared.encrypt_pending();
                if n == 0 {
                    return Err(io::ErrorKind::WriteZero.into());
                }
                rest = &rest[n..];
            }
        }
        self.send(true)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.send(true)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rustls::client::danger::{ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::ServerName;
    use rustls::{ClientConfig, ClientConnection, StreamOwned};
    use std::net::TcpListener;
    use std::thread;

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
    fn link_carries_both_ways_and_closes() {
        for version in [&rustls::version::TLS13, &rustls::version::TLS12] {
            let host = Identity::generate().unwrap();
            let phone = Identity::generate().unwrap();
            let config = server_config(&host, false).unwrap();
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
            link.shutdown().unwrap();
            let (down, phone_fp) = phone_side.join().unwrap();
            assert_eq!(down.len(), 2_000_000);
            assert!(down.iter().enumerate().all(|(i, &b)| b == (i % 241) as u8));
            assert_eq!(accepted.fingerprint, phone_fp);
        }
    }

    #[test]
    fn tls12_only_server_makes_a_recent_phone_use_tls12() {
        let host = Identity::generate().unwrap();
        let phone = Identity::generate().unwrap();
        let config = server_config(&host, true).unwrap();
        let (c, s) = pair_of_sockets();
        let host_fp = host.fingerprint();
        let phone_side = thread::spawn(move || {
            let crypto = crypto();
            let config = ClientConfig::builder_with_provider(crypto.clone())
                .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(Pinned(host_fp, crypto)))
                .with_client_auth_cert(
                    vec![CertificateDer::from(phone.cert.clone())],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(phone.key.clone())),
                )
                .unwrap();
            let conn = ClientConnection::new(Arc::new(config), ServerName::try_from("x").unwrap())
                .unwrap();
            let mut tls = StreamOwned::new(conn, c);
            tls.write_all(b"hi").unwrap();
            tls.flush().unwrap();
        });
        let accepted = accept(s, config).unwrap();
        assert_eq!(accepted.version, "TLSv1_2");
        phone_side.join().unwrap();
    }

    #[test]
    fn a_phone_pinning_another_computer_fails() {
        let host = Identity::generate().unwrap();
        let other = Identity::generate().unwrap();
        let phone = Identity::generate().unwrap();
        let config = server_config(&host, false).unwrap();
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join("tls-key.der"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(short_fingerprint(&a.fingerprint()).len(), 19);
    }
}
