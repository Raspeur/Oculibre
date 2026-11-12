//! The computer side as a whole: wait for phones (USB and, with Wi-Fi on, the network through
//! TLS), run one session at a time into the virtual camera, offer pairings, stop on request.
//!
//! A front-end creates a [`Host`] with [`Host::start`], then either calls [`Host::run`] (the
//! console: until Ctrl+C) or runs it on a thread and drives it from elsewhere (the desktop app):
//! [`Host::remote`] for the phone being served (camera, quality), [`Host::set_view`] for rotation
//! and mirror, [`Host::stop`]. What happens comes back as [`Event`]s.

use std::error::Error;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oculibre_protocol::Hello;
use rustls::ServerConfig;

use crate::discovery::{self, Announcer};
use crate::events::{Event, Events, Refusal, Via, WifiInfo};
use crate::pairing::{self, Pairings};
use crate::session::{self, Driver, End, Link, Remote, Setup, Sink, View};
use crate::vcam::VirtualCamera;
use crate::{adb, tls, v4l2};

/// What the user chose (the options of `oculibre`, the settings of the app).
#[derive(Debug, Clone)]
pub struct Config {
    /// TCP port for phones (0: any free port, for tests).
    pub port: u16,
    /// The v4l2loopback device; `None`: the one named "Oculibre", else the first loopback device.
    pub device: Option<PathBuf>,
    /// Run `adb reverse` so that USB phones can connect.
    pub adb: bool,
    /// Also accept paired phones over the network (TLS).
    pub wifi: bool,
    /// Testing: offer TLS 1.2 only, as with Android 5–9 phones.
    pub tls12_only: bool,
    pub view: View,
    /// Portrait virtual camera (landscape by default, like every webcam).
    pub portrait: bool,
    /// Tests: pictures are dropped instead of being written to a v4l2 device.
    #[cfg(test)]
    pub discard_pictures: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: oculibre_protocol::HOST_PORT,
            device: None,
            adb: true,
            wifi: false,
            tls12_only: false,
            view: View::default(),
            portrait: false,
            #[cfg(test)]
            discard_pictures: false,
        }
    }
}

/// A phone that connected, ready for [`Host::session`].
pub struct Incoming {
    link: Box<dyn Link>,
    pub peer: SocketAddr,
    pub via: Via,
    /// Wi-Fi: SHA-256 of the phone's TLS certificate.
    fingerprint: Option<String>,
}

/// A one-time pairing offer ([`Host::pairing_offer`]): show `uri` as a QR code.
#[derive(Debug, Clone)]
pub struct Offer {
    /// `oculibre://pair?…`
    pub uri: String,
    /// Valid this long, for one phone.
    pub ttl: Duration,
    /// Also opened on the phone plugged in over USB (it asks the user to confirm).
    pub sent_over_usb: bool,
}

/// TLS identity and paired phones, when Wi-Fi is on.
struct Wifi {
    config: Arc<ServerConfig>,
    pairings: Pairings,
    fingerprint: String,
    addresses: Vec<IpAddr>,
    /// mDNS announcement, as long as the host lives.
    _announcer: Option<Announcer>,
}

/// The receiver: see the module documentation.
pub struct Host {
    config: Config,
    device: PathBuf,
    listener: TcpListener,
    /// Where phones connect (the real port when `config.port` was 0).
    address: SocketAddr,
    wifi: Option<Wifi>,
    hello: Hello,
    events: Events,
    stopping: AtomicBool,
    /// The running session's connection, to end it from [`Host::stop`].
    current: Mutex<Option<Box<dyn Link>>>,
    /// The running session, once the phone is in.
    remote: Mutex<Option<Remote>>,
    /// Rotation and mirror: for the running session and the next ones.
    view: Mutex<View>,
}

impl Host {
    /// Checks the virtual camera, runs `adb reverse`, listens, and with Wi-Fi on loads the TLS
    /// identity and the paired phones and announces the computer (mDNS). Nothing is accepted
    /// before [`Host::run`] or [`Host::accept`].
    pub fn start(config: Config, events: Events) -> Result<Host, String> {
        let device = match config.device.clone() {
            Some(d) => d,
            None => v4l2::find_loopback().map_err(|e| e.to_string())?,
        };
        v4l2::check_access(&device).map_err(|e| e.to_string())?;
        events(Event::VirtualCamera {
            path: device.clone(),
        });

        if config.adb {
            events(Event::Usb(adb::reverse(config.port)));
        }

        // §13: plaintext only on loopback (USB through adb reverse); the network needs Wi-Fi (TLS).
        let bind_ip = if config.wifi {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        };
        let listener = TcpListener::bind((bind_ip, config.port)).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                format!(
                    "port {} is already used, probably by another `oculibre` still running: \
                     stop it first (or use --port)",
                    config.port
                )
            } else {
                format!("cannot listen on port {}: {e}", config.port)
            }
        })?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let address = SocketAddr::new(bind_ip, port);

        let wifi = if config.wifi {
            let identity = tls::Identity::load_or_create(&config_dir())?;
            let fingerprint = identity.fingerprint();
            let addresses = pairing::lan_addresses();
            let announcer = match discovery::announce(&host_name(), &fingerprint, &addresses, port)
            {
                Ok(a) => Some(a),
                Err(e) => {
                    events(Event::AnnounceFailed(e));
                    None
                }
            };
            let wifi = Wifi {
                config: tls::server_config(&identity, config.tls12_only)?,
                pairings: Pairings::load(config_dir().join("paired-phones.json")),
                fingerprint,
                addresses,
                _announcer: announcer,
            };
            events(Event::Wifi(WifiInfo {
                addresses: wifi.addresses.clone(),
                paired: wifi.pairings.phones().len(),
                key: tls::short_fingerprint(&wifi.fingerprint),
                announced: wifi._announcer.is_some(),
            }));
            Some(wifi)
        } else {
            None
        };

        let view = config.view;
        Ok(Host {
            hello: session::host_hello(host_id(), host_name(), os_version()),
            config,
            device,
            listener,
            address,
            wifi,
            events,
            stopping: AtomicBool::new(false),
            current: Mutex::new(None),
            remote: Mutex::new(None),
            view: Mutex::new(view),
        })
    }

    /// Where phones connect.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// The phone being served, to send it commands (`set_mode`…); `None` between sessions.
    pub fn remote(&self) -> Option<Remote> {
        self.remote.lock().unwrap().clone()
    }

    /// Rotation and mirror in use, or for the next phone.
    pub fn view(&self) -> View {
        *self.view.lock().unwrap()
    }

    /// Changes rotation and mirror: at once for the phone being served, and for the next ones.
    pub fn set_view(&self, view: View) {
        *self.view.lock().unwrap() = view;
        if let Some(remote) = self.remote() {
            remote.set_view(view);
        }
    }

    /// A one-time pairing offer (Wi-Fi only): show its URI as a QR code. Over USB, it is also
    /// opened on the phone plugged in.
    pub fn pairing_offer(&self) -> Result<Offer, String> {
        let wifi = self.wifi.as_ref().ok_or("pairing needs Wi-Fi to be on")?;
        if wifi.addresses.is_empty() {
            return Err(
                "no network address found: connect this computer to the Wi-Fi first".into(),
            );
        }
        let token = wifi.pairings.open_offer(pairing::OFFER_TTL)?;
        let uri = pairing::uri(
            &host_name(),
            &wifi.fingerprint,
            &token,
            &wifi.addresses,
            self.address.port(),
        );
        let sent_over_usb = self.config.adb && adb::open_uri(&uri);
        Ok(Offer {
            uri,
            ttl: pairing::OFFER_TTL,
            sent_over_usb,
        })
    }

    /// Serves phones one after the other until [`Host::stop`].
    pub fn run(&self) {
        while let Some(phone) = self.accept() {
            let end = self.session(phone, self.events.clone(), None);
            (self.events)(Event::SessionEnded(end.map_err(|e| e.to_string())));
        }
    }

    /// Waits for a phone: USB (loopback) as is, the network through TLS when Wi-Fi is on.
    /// `None` once [`Host::stop`] was called.
    pub fn accept(&self) -> Option<Incoming> {
        loop {
            let accepted = self.listener.accept();
            if self.stopping.load(Ordering::Relaxed) {
                return None;
            }
            let (stream, peer) = match accepted {
                Ok(c) => c,
                Err(e) => {
                    (self.events)(Event::AcceptFailed(e.to_string()));
                    continue;
                }
            };
            if peer.ip().is_loopback() {
                let _ = stream.set_nodelay(true);
                (self.events)(Event::PhoneConnected {
                    peer,
                    via: Via::Usb,
                });
                return Some(Incoming {
                    link: Box::new(stream),
                    peer,
                    via: Via::Usb,
                    fingerprint: None,
                });
            }
            // §13: never plaintext on a network.
            let Some(wifi) = &self.wifi else {
                (self.events)(Event::PhoneRefused {
                    peer,
                    reason: Refusal::WifiOff,
                });
                continue;
            };
            match tls::accept(stream, wifi.config.clone()) {
                Ok(a) => {
                    let via = Via::Wifi { tls: a.version };
                    (self.events)(Event::PhoneConnected {
                        peer,
                        via: via.clone(),
                    });
                    return Some(Incoming {
                        link: Box::new(a.link),
                        peer,
                        via,
                        fingerprint: Some(a.fingerprint),
                    });
                }
                Err(e) => (self.events)(Event::PhoneRefused {
                    peer,
                    reason: Refusal::Tls(e),
                }),
            }
        }
    }

    /// Runs one phone's session into the virtual camera. `events`: where this session reports
    /// (the bench passes its own); `driver`: see [`session::run_driven`].
    pub fn session(
        &self,
        phone: Incoming,
        events: Events,
        driver: Option<Driver<'_>>,
    ) -> Result<End, Box<dyn Error>> {
        let Incoming {
            link, fingerprint, ..
        } = phone;
        // Wi-Fi phones must be paired (or bring a valid pairing token); USB phones need nothing.
        let authorize = match (&self.wifi, &fingerprint) {
            (Some(wifi), Some(fingerprint)) => Some(move |hello: &Hello| {
                wifi.pairings.authorize(fingerprint, hello).map(|new| {
                    if let Some(name) = new {
                        (self.events)(Event::Paired { name });
                    }
                })
            }),
            _ => None,
        };
        if let Ok(clone) = link.try_clone_link() {
            *self.current.lock().unwrap() = Some(clone);
        }
        if self.stopping.load(Ordering::Relaxed) {
            return Ok(End::Finished);
        }
        let mut sink: Box<dyn Sink + Send> = Box::new(VirtualCamera::new(
            self.device.clone(),
            self.config.portrait,
            events.clone(),
        ));
        #[cfg(test)]
        if self.config.discard_pictures {
            sink = Box::new(Discard);
        }
        let setup = Setup {
            view: self.view(),
            driver,
            authorize: authorize.as_ref().map(|a| a as session::Authorize<'_>),
            on_start: Some(Box::new(|remote| {
                *self.remote.lock().unwrap() = Some(remote)
            })),
            ..Setup::new(events)
        };
        let end = session::run_driven(link, &self.hello, &mut *sink, setup);
        *self.remote.lock().unwrap() = None;
        *self.current.lock().unwrap() = None;
        end
    }

    /// Ends the running session (if any) and makes [`Host::run`] / [`Host::accept`] return.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::Relaxed);
        if let Some(link) = self.current.lock().unwrap().as_ref() {
            let _ = link.shutdown_link();
        }
        // accept() blocks: wake it up with a connection of our own.
        let wake = if self.address.ip().is_unspecified() {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.address.port())
        } else {
            self.address
        };
        let _ = TcpStream::connect_timeout(&wake, Duration::from_secs(1));
    }
}

/// `~/.config/oculibre` (or `$XDG_CONFIG_HOME/oculibre`): TLS identity, paired phones, host id.
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("oculibre")
}

/// Random id of this computer, created once (§7.1: never derived from hardware identifiers).
pub fn host_id() -> String {
    let dir = config_dir();
    let file = dir.join("host-id");
    if let Ok(id) = fs::read_to_string(&file) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }
    let mut b = [0u8; 16];
    if let Ok(mut f) = fs::File::open("/dev/urandom") {
        let _ = std::io::Read::read_exact(&mut f, &mut b);
    }
    b[6] = (b[6] & 0x0f) | 0x40; // UUID version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let id = format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    );
    let _ = fs::create_dir_all(dir).and_then(|_| fs::write(file, &id));
    id
}

/// This computer's name, as phones show it.
pub fn host_name() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .or_else(|_| fs::read_to_string("/etc/hostname"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "computer".into())
}

/// The kernel version, sent in our `HELLO`.
pub fn os_version() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|s| s.trim().to_string())
}

/// Tests: a virtual camera that drops every picture.
#[cfg(test)]
struct Discard;

#[cfg(test)]
impl Sink for Discard {
    fn show(
        &mut self,
        _: &crate::yuv::Picture,
        _: crate::yuv::Transform,
    ) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}
