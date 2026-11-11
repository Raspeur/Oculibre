//! mDNS announcement (protocol §2 "Discovery"): with `--wifi`, the computer announces
//! `_oculibre-host._tcp` so that a paired phone finds it again when its address changes.
//!
//! The TXT record carries the start of the computer's certificate fingerprint, enough for a phone
//! to recognise the computer it is paired with among several. It is not a secret (the phone still
//! checks the whole fingerprint in the TLS handshake) and reveals nothing the QR code did not.

use std::net::IpAddr;

use mdns_sd::{ServiceDaemon, ServiceInfo};

/// DNS-SD service type of a host listening for cameras (§2).
pub const SERVICE: &str = "_oculibre-host._tcp.local.";
/// Version of the TXT record format.
const TXT_VERSION: &str = "1";
/// Hex characters of the fingerprint published in TXT `k` (64 bits).
const KEY_PREFIX: usize = 16;

/// Keeps announcing while alive.
pub struct Announcer {
    daemon: ServiceDaemon,
}

impl Drop for Announcer {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}

/// TXT properties: `v` (format version) and `k` (fingerprint start).
pub fn txt(fingerprint: &str) -> Vec<(&'static str, String)> {
    vec![
        ("v", TXT_VERSION.to_string()),
        ("k", fingerprint.chars().take(KEY_PREFIX).collect()),
    ]
}

/// A valid DNS label from the computer's name (`raspeur-ubuntu-studio`).
pub fn dns_label(name: &str) -> String {
    let mut label: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    label = label.trim_matches('-').chars().take(63).collect();
    if label.is_empty() {
        "oculibre".into()
    } else {
        label
    }
}

/// The addresses worth announcing: mDNS only reaches the local network, so home and office
/// addresses (10/8, 172.16/12, 192.168/16) only, when there are some. A VPN address (Tailscale's
/// 100.64/10…) in the announcement could be the one an older phone picks: Android before 14 keeps a
/// single address per service.
pub fn announced_addresses(addresses: &[IpAddr]) -> Vec<IpAddr> {
    let local: Vec<IpAddr> = addresses
        .iter()
        .copied()
        .filter(|ip| matches!(ip, IpAddr::V4(v4) if v4.is_private()))
        .collect();
    if local.is_empty() {
        addresses.to_vec()
    } else {
        local
    }
}

/// Announces this computer on its local network addresses (see [`announced_addresses`]).
pub fn announce(
    name: &str,
    fingerprint: &str,
    addresses: &[IpAddr],
    port: u16,
) -> Result<Announcer, String> {
    if addresses.is_empty() {
        return Err("no network address".into());
    }
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let host = format!("{}.local.", dns_label(name));
    let addresses = announced_addresses(addresses);
    let info = ServiceInfo::new(
        SERVICE,
        name,
        &host,
        &addresses[..],
        port,
        &txt(fingerprint)[..],
    )
    .map_err(|e| e.to_string())?;
    daemon.register(info).map_err(|e| e.to_string())?;
    Ok(Announcer { daemon })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_has_the_fingerprint_start() {
        let fp = "46b5ee534ffeee792fae09cfde631495702feb5a2e623fd01f1c258959457e5b";
        assert_eq!(
            txt(fp),
            vec![
                ("v", "1".to_string()),
                ("k", "46b5ee534ffeee79".to_string())
            ]
        );
    }

    /// Needs a network interface with multicast: `cargo test -- --ignored announced_and_found`.
    #[test]
    #[ignore]
    fn announced_and_found() {
        use std::time::{Duration, Instant};
        let fp = "0123456789abcdef".repeat(4);
        let addresses = crate::pairing::lan_addresses();
        let _a = announce("oculibre-test", &fp, &addresses, 47199).unwrap();
        let browser = ServiceDaemon::new().unwrap();
        let events = browser.browse(SERVICE).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            if let Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) =
                events.recv_timeout(Duration::from_millis(200))
            {
                if info.get_port() == 47199 {
                    assert_eq!(info.get_property_val_str("k"), Some("0123456789abcdef"));
                    let announced = info.get_addresses();
                    assert!(!announced.is_empty());
                    assert!(
                        announced.iter().all(|ip| match ip.to_ip_addr() {
                            IpAddr::V4(v4) => v4.is_private(),
                            IpAddr::V6(_) => false,
                        }),
                        "{announced:?}"
                    );
                    let _ = browser.shutdown();
                    return;
                }
            }
        }
        panic!("not found");
    }

    #[test]
    fn only_local_network_addresses_are_announced() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        assert_eq!(
            announced_addresses(&[ip("192.168.1.50"), ip("100.112.121.63")]),
            vec![ip("192.168.1.50")]
        );
        assert_eq!(
            announced_addresses(&[ip("100.112.121.63"), ip("10.0.0.2"), ip("172.20.1.1")]),
            vec![ip("10.0.0.2"), ip("172.20.1.1")]
        );
        // Nothing local: announce what there is.
        assert_eq!(
            announced_addresses(&[ip("100.1.2.3")]),
            vec![ip("100.1.2.3")]
        );
    }

    #[test]
    fn dns_labels() {
        assert_eq!(dns_label("raspeur-ubuntu-studio"), "raspeur-ubuntu-studio");
        assert_eq!(dns_label("Clément's PC"), "cl-ment-s-pc");
        assert_eq!(dns_label("--"), "oculibre");
        assert_eq!(dns_label(&"a".repeat(80)).len(), 63);
    }
}
