//! Pairing (protocol §13): which phones may connect over Wi-Fi.
//!
//! `oculibre --pair` opens a *pairing offer*: a one-time token, shown with this computer's
//! certificate fingerprint and addresses as a QR code (`oculibre://pair?…`). The phone pins the
//! fingerprint and sends the token in its `HELLO`, inside TLS; the computer then remembers the
//! phone's certificate fingerprint. Later connections need no token.

use std::fs;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use oculibre_protocol::Hello;
use serde::{Deserialize, Serialize};

/// How long a pairing offer stays valid.
pub const OFFER_TTL: Duration = Duration::from_secs(10 * 60);

/// A paired phone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Phone {
    /// SHA-256 of the phone's TLS certificate.
    pub fingerprint: String,
    /// Name from its `HELLO` when it was paired.
    pub name: String,
    /// Install id from its `HELLO`.
    pub id: String,
    pub paired_on: String,
}

struct Offer {
    token: String,
    until: Instant,
}

/// The paired phones (`paired-phones.json`) and the current pairing offer.
pub struct Pairings {
    path: PathBuf,
    phones: Mutex<Vec<Phone>>,
    offer: Mutex<Option<Offer>>,
}

impl Pairings {
    pub fn load(path: PathBuf) -> Self {
        let phones = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self {
            path,
            phones: Mutex::new(phones),
            offer: Mutex::new(None),
        }
    }

    pub fn phones(&self) -> Vec<Phone> {
        self.phones.lock().unwrap().clone()
    }

    /// Starts a pairing offer and returns its token (128 random bits, hex).
    pub fn open_offer(&self, ttl: Duration) -> Result<String, String> {
        let mut bytes = [0u8; 16];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
            .map_err(|_| "no random numbers available")?;
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        *self.offer.lock().unwrap() = Some(Offer {
            token: token.clone(),
            until: Instant::now() + ttl,
        });
        Ok(token)
    }

    /// May this phone stream? `Ok(Some(name))`: it has just been paired with the offer's token.
    pub fn authorize(&self, fingerprint: &str, hello: &Hello) -> Result<Option<String>, String> {
        if self
            .phones
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.fingerprint == fingerprint)
        {
            return Ok(None);
        }
        let Some(token) = hello.pairing_token.as_deref() else {
            return Err(
                "this phone is not paired with this computer: run `oculibre --pair` \
                        and scan the QR code with the phone"
                    .into(),
            );
        };
        let mut offer = self.offer.lock().unwrap();
        let valid = offer.as_ref().is_some_and(|o| {
            Instant::now() < o.until && same(o.token.as_bytes(), token.as_bytes())
        });
        if !valid {
            return Err(
                "this pairing code is not valid any more: run `oculibre --pair` again \
                        and scan the new QR code"
                    .into(),
            );
        }
        *offer = None; // one time only
        let name = if hello.name.is_empty() {
            "phone".to_string()
        } else {
            hello.name.clone()
        };
        let phone = Phone {
            fingerprint: fingerprint.into(),
            name: name.clone(),
            id: hello.id.clone(),
            paired_on: crate::bench::today(),
        };
        let mut phones = self.phones.lock().unwrap();
        phones.retain(|p| p.id != phone.id || p.id.is_empty()); // re-pairing replaces
        phones.push(phone);
        save(&self.path, &phones)?;
        Ok(Some(name))
    }

    /// Forgets the phones whose name or fingerprint start with `which` (case-insensitive), or all
    /// of them for `--all`. Returns the forgotten ones.
    pub fn forget(&self, which: &str) -> Result<Vec<Phone>, String> {
        let which = which.to_lowercase();
        let mut phones = self.phones.lock().unwrap();
        let (gone, kept): (Vec<Phone>, Vec<Phone>) = phones.drain(..).partition(|p| {
            which == "--all"
                || p.name.to_lowercase().starts_with(&which)
                || p.fingerprint.starts_with(&which)
        });
        *phones = kept;
        save(&self.path, &phones)?;
        Ok(gone)
    }
}

/// Constant-time comparison (the token must not leak through timing).
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn save(path: &Path, phones: &[Phone]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let json = serde_json::to_vec_pretty(phones).expect("serialisable");
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| f.write_all(&json))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// `oculibre://pair?v=1&n=<name>&k=<fingerprint>&t=<token>&a=<ip>,<ip>&p=<port>` (§13).
pub fn uri(name: &str, fingerprint: &str, token: &str, addresses: &[IpAddr], port: u16) -> String {
    let a: Vec<String> = addresses.iter().map(IpAddr::to_string).collect();
    format!(
        "oculibre://pair?v=1&n={}&k={fingerprint}&t={token}&a={}&p={port}",
        percent(name),
        a.join(",")
    )
}

/// Percent-encodes everything but unreserved characters (RFC 3986).
fn percent(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The QR code of `text` as a grid, for a front-end that draws it itself (the desktop app):
/// `width` modules per row, row after row, `true` for dark. No quiet zone: add 4 modules around.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QrGrid {
    pub width: usize,
    pub dark: Vec<bool>,
}

pub fn qr_grid(text: &str) -> Result<QrGrid, String> {
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(QrGrid {
        width: code.width(),
        dark: code
            .to_colors()
            .into_iter()
            .map(|c| c == qrcode::Color::Dark)
            .collect(),
    })
}

/// The QR code of `text` for a terminal with a dark background (Unicode half blocks).
pub fn qr(text: &str) -> Result<String, String> {
    use qrcode::render::unicode::Dense1x2;
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(code
        .render::<Dense1x2>()
        .dark_color(Dense1x2::Light)
        .light_color(Dense1x2::Dark)
        .quiet_zone(true)
        .build())
}

/// IPv4 addresses of this computer a phone on the same network may reach (no loopback, no
/// container bridges).
pub fn lan_addresses() -> Vec<IpAddr> {
    let mut out = Vec::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills `list`, freed below with freeifaddrs; entries are only read.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return out;
        }
        let mut cur = list;
        while !cur.is_null() {
            let ifa = &*cur;
            cur = ifa.ifa_next;
            if ifa.ifa_addr.is_null() || i32::from((*ifa.ifa_addr).sa_family) != libc::AF_INET {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy();
            if ["lo", "docker", "br-", "veth", "virbr", "lxc", "podman"]
                .iter()
                .any(|p| name.starts_with(p))
            {
                continue;
            }
            let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
            let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            if !ip.is_loopback() && !ip.is_unspecified() && !out.contains(&IpAddr::V4(ip)) {
                out.push(IpAddr::V4(ip));
            }
        }
        libc::freeifaddrs(list);
    }
    // Home and office networks first, VPNs and the rest after.
    out.sort_by_key(|ip| match ip {
        IpAddr::V4(v4) => !v4.is_private(),
        IpAddr::V6(_) => true,
    });
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn qr_grid_is_square_with_finder_patterns() {
        let g = super::qr_grid("oculibre://pair?v=1&k=abc").unwrap();
        assert_eq!(g.dark.len(), g.width * g.width);
        // Top-left finder pattern: a dark 7×7 ring.
        assert!((0..7).all(|i| g.dark[i] && g.dark[i * g.width]));
        assert!(!g.dark[g.width + 1]);
    }

    use super::*;

    fn hello(name: &str, token: Option<&str>) -> Hello {
        Hello {
            name: name.into(),
            id: format!("id-{name}"),
            pairing_token: token.map(String::from),
            ..Default::default()
        }
    }

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "oculibre-pairing-{name}-{}.json",
            std::process::id()
        ))
    }

    #[test]
    fn pairing_needs_the_offer_token_once() {
        let path = temp("once");
        let p = Pairings::load(path.clone());
        assert!(p
            .authorize("fp1", &hello("Redmi", None))
            .unwrap_err()
            .contains("not paired"));
        assert!(p
            .authorize("fp1", &hello("Redmi", Some("guess")))
            .unwrap_err()
            .contains("not valid"));

        let token = p.open_offer(OFFER_TTL).unwrap();
        assert_eq!(token.len(), 32);
        assert!(p.authorize("fp1", &hello("Redmi", Some("0000"))).is_err());
        assert_eq!(
            p.authorize("fp1", &hello("Redmi", Some(&token))).unwrap(),
            Some("Redmi".into())
        );
        // Paired: no token needed any more; the token cannot pair a second phone.
        assert_eq!(p.authorize("fp1", &hello("Redmi", None)).unwrap(), None);
        assert!(p.authorize("fp2", &hello("Other", Some(&token))).is_err());

        // Saved, readable by the owner only.
        let again = Pairings::load(path.clone());
        assert_eq!(again.phones().len(), 1);
        assert_eq!(again.phones()[0].fingerprint, "fp1");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        assert_eq!(again.forget("red").unwrap().len(), 1);
        assert!(Pairings::load(path.clone()).phones().is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn expired_offer_is_refused() {
        let path = temp("expired");
        let p = Pairings::load(path.clone());
        let token = p.open_offer(Duration::ZERO).unwrap();
        assert!(p.authorize("fp", &hello("A", Some(&token))).is_err());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn re_pairing_a_phone_replaces_its_old_key() {
        let path = temp("repair");
        let p = Pairings::load(path.clone());
        let t = p.open_offer(OFFER_TTL).unwrap();
        p.authorize("old", &hello("A", Some(&t))).unwrap();
        let t = p.open_offer(OFFER_TTL).unwrap();
        p.authorize("new", &hello("A", Some(&t))).unwrap();
        let phones = p.phones();
        assert_eq!(phones.len(), 1);
        assert_eq!(phones[0].fingerprint, "new");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn uri_and_qr() {
        let u = uri(
            "raspeur PC",
            "ab12",
            "cd34",
            &[
                "192.168.1.50".parse().unwrap(),
                "100.1.2.3".parse().unwrap(),
            ],
            47100,
        );
        assert_eq!(
            u,
            "oculibre://pair?v=1&n=raspeur%20PC&k=ab12&t=cd34&a=192.168.1.50,100.1.2.3&p=47100"
        );
        let q = qr(&u).unwrap();
        assert!(q.lines().count() > 20);
        assert!(same(b"abc", b"abc") && !same(b"abc", b"abd") && !same(b"abc", b"ab"));
    }
}
