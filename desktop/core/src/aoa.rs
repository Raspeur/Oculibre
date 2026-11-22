//! USB without developer mode (#21): Android Open Accessory (AOA).
//!
//! The computer, as USB host, asks a plugged phone to switch to accessory mode with three vendor
//! control requests. The phone comes back as Google's accessory device (18d1:2d00, or 2d01 when
//! USB debugging is on), Android offers to open Oculibre, and the app reads and writes two bulk
//! endpoints. Like `adb reverse`, [`watch`] relays those bytes to the host's own port on
//! 127.0.0.1: the session sees a USB phone on loopback, in plaintext (the cable is trusted, §13).
//!
//! Linux only, through usbfs (`/dev/bus/usb`) with raw ioctls: no libusb. The device nodes are
//! root's; udev gives them to the logged-in user (`uaccess`): systemd does it for phones in file
//! transfer mode (MTP), the `android-udev-rules` package for most phone makers. Oculibre's own
//! rule (packaging, #18) is still to write.
//!
//! The phone always speaks first (its preamble): the relay connects to the host only then, so a
//! phone in accessory mode whose app is not open yet does not start a session that would time out.
//! What comes before a preamble is dropped: the rest of a session whose computer went away (the
//! phone goes on sending until its own 5 s of silence end it).

use std::collections::HashSet;
use std::ffi::{c_int, c_uint, c_void};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpStream};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use oculibre_protocol::{Role, MAGIC, PREAMBLE_LEN};

use crate::events::Accessory;

/// What the app's accessory filter matches (android/app/src/main/res/xml/accessory_filter.xml).
pub const MANUFACTURER: &str = "Oculibre";
pub const MODEL: &str = "Oculibre";
/// Sent to the phone when switching: manufacturer, model, description, version, URI (Android
/// offers it when no app matches: where to get Oculibre), serial.
const STRINGS: [&str; 6] = [
    MANUFACTURER,
    MODEL,
    "Oculibre webcam receiver",
    "1",
    "https://oculibre.org",
    "0",
];

const GOOGLE: u16 = 0x18d1;
/// Accessory, accessory + adb, then the same with audio (AOA 2).
const ACCESSORY_PRODUCTS: std::ops::RangeInclusive<u16> = 0x2d00..=0x2d05;
const HUB_CLASS: u8 = 9;

const GET_PROTOCOL: u8 = 51;
const SEND_STRING: u8 = 52;
const START: u8 = 53;

/// Android's accessory driver moves at most 16 KiB per request.
const BUFFER: usize = 16 * 1024;
/// Reads queued at once, so the phone never waits for us to submit the next one.
const READS: usize = 4;
const SCAN_EVERY: Duration = Duration::from_millis(500);
const SYSFS: &str = "/sys/bus/usb/devices";

// ---------------------------------------------------------------- usbfs (linux/usbdevice_fs.h)

#[repr(C)]
struct CtrlTransfer {
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
    timeout: u32,
    data: *mut c_void,
}

#[repr(C)]
struct Urb {
    kind: u8,
    endpoint: u8,
    status: c_int,
    flags: c_uint,
    buffer: *mut c_void,
    buffer_length: c_int,
    actual_length: c_int,
    start_frame: c_int,
    number_of_packets: c_int,
    error_count: c_int,
    signr: c_uint,
    usercontext: *mut c_void,
}

const URB_TYPE_BULK: u8 = 3;
/// End a transfer whose length is a multiple of the packet size with a zero-length packet: the
/// phone's read would otherwise wait for more.
const URB_ZERO_PACKET: c_uint = 0x40;

const fn ioc(dir: u64, nr: u64, size: usize) -> u64 {
    (dir << 30) | ((size as u64) << 16) | ((b'U' as u64) << 8) | nr
}
const IOC_NONE: u64 = 0;
const IOC_WRITE: u64 = 1;
const IOC_READ: u64 = 2;
const USBDEVFS_CONTROL: u64 = ioc(IOC_READ | IOC_WRITE, 0, size_of::<CtrlTransfer>());
const USBDEVFS_SUBMITURB: u64 = ioc(IOC_READ, 10, size_of::<Urb>());
const USBDEVFS_DISCARDURB: u64 = ioc(IOC_NONE, 11, 0);
const USBDEVFS_REAPURBNDELAY: u64 = ioc(IOC_WRITE, 13, size_of::<*mut c_void>());
const USBDEVFS_CLAIMINTERFACE: u64 = ioc(IOC_READ, 15, size_of::<c_uint>());
const USBDEVFS_RELEASEINTERFACE: u64 = ioc(IOC_READ, 16, size_of::<c_uint>());

fn check(ret: c_int) -> io::Result<c_int> {
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

/// A control transfer on endpoint 0; returns the bytes moved.
fn control(
    file: &File,
    request_type: u8,
    request: u8,
    index: u16,
    data: &mut [u8],
) -> io::Result<usize> {
    let mut t = CtrlTransfer {
        request_type,
        request,
        value: 0,
        index,
        length: data.len() as u16,
        timeout: 1000,
        data: data.as_mut_ptr().cast(),
    };
    // SAFETY: `t` and the buffer it points to live through the (synchronous) call.
    check(unsafe { libc::ioctl(file.as_raw_fd(), USBDEVFS_CONTROL as _, &mut t) })
        .map(|n| n as usize)
}

// ---------------------------------------------------------------- devices (sysfs)

/// A USB device as sysfs lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct UsbDevice {
    /// Its sysfs folder, and name there ("1-4.2").
    sys: PathBuf,
    name: String,
    bus: u32,
    dev: u32,
    vendor: u16,
    product: u16,
    class: u8,
    /// What it calls itself ("Redmi Note 9 Pro"), when it says.
    label: String,
}

impl UsbDevice {
    fn key(&self) -> (u32, u32) {
        (self.bus, self.dev)
    }

    fn node(&self) -> PathBuf {
        PathBuf::from(format!("/dev/bus/usb/{:03}/{:03}", self.bus, self.dev))
    }

    fn is_accessory(&self) -> bool {
        self.vendor == GOOGLE && ACCESSORY_PRODUCTS.contains(&self.product)
    }
}

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

fn read_hex(path: &Path) -> Option<u16> {
    u16::from_str_radix(&read_text(path)?, 16).ok()
}

/// The USB devices plugged in (interfaces and hubs' ports left out).
fn devices(root: &Path) -> Vec<UsbDevice> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found: Vec<UsbDevice> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.contains(':') {
                return None; // an interface ("1-4:1.0")
            }
            let sys = e.path();
            let label = [
                read_text(&sys.join("manufacturer")),
                read_text(&sys.join("product")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
            Some(UsbDevice {
                bus: read_text(&sys.join("busnum"))?.parse().ok()?,
                dev: read_text(&sys.join("devnum"))?.parse().ok()?,
                vendor: read_hex(&sys.join("idVendor"))?,
                product: read_hex(&sys.join("idProduct"))?,
                class: read_hex(&sys.join("bDeviceClass")).unwrap_or(0) as u8,
                label,
                name,
                sys,
            })
        })
        .collect();
    found.sort_by_key(|d| d.key());
    found
}

/// The accessory interface (vendor class, subclass 0xff: not adb's 0x42) and its two bulk
/// endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Endpoints {
    interface: u32,
    input: u8,
    output: u8,
}

fn accessory_endpoints(dev: &UsbDevice) -> Option<Endpoints> {
    let prefix = format!("{}:", dev.name);
    let entries = std::fs::read_dir(&dev.sys).ok()?;
    for e in entries.flatten() {
        if !e.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let dir = e.path();
        let hex = |name: &str| read_hex(&dir.join(name));
        if hex("bInterfaceClass") != Some(0xff) || hex("bInterfaceSubClass") != Some(0xff) {
            continue;
        }
        let (mut input, mut output) = (None, None);
        for ep in std::fs::read_dir(&dir).ok()?.flatten() {
            if !ep.file_name().to_string_lossy().starts_with("ep_") {
                continue;
            }
            let ep = ep.path();
            if read_text(&ep.join("type")).as_deref() != Some("Bulk") {
                continue;
            }
            let Some(address) = read_hex(&ep.join("bEndpointAddress")) else {
                continue;
            };
            if address & 0x80 != 0 {
                input = Some(address as u8);
            } else {
                output = Some(address as u8);
            }
        }
        return Some(Endpoints {
            interface: hex("bInterfaceNumber")? as u32,
            input: input?,
            output: output?,
        });
    }
    None
}

/// Asks a device to switch to accessory mode. `Ok(false)`: it does not speak AOA (not an Android
/// device). Devices this user may not open fail with `PermissionDenied`.
fn switch(dev: &UsbDevice) -> io::Result<bool> {
    let file = OpenOptions::new().read(true).write(true).open(dev.node())?;
    let mut version = [0u8; 2];
    match control(&file, 0xc0, GET_PROTOCOL, 0, &mut version) {
        Ok(2) => {}
        _ => return Ok(false),
    }
    if u16::from_le_bytes(version) == 0 {
        return Ok(false);
    }
    for (index, text) in STRINGS.iter().enumerate() {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        control(&file, 0x40, SEND_STRING, index as u16, &mut bytes)?;
    }
    control(&file, 0x40, START, 0, &mut [])?;
    Ok(true)
}

// ---------------------------------------------------------------- the bulk endpoints

/// One transfer: boxed, so the kernel's pointers to it stay valid while it is queued.
struct Transfer {
    urb: Urb,
    buf: Vec<u8>,
    queued: bool,
}

const WRITE_SLOT: usize = READS;

/// The accessory interface, claimed, with its transfers. Dropping it cancels what is queued and
/// closes the device before the buffers are freed.
struct Accessor {
    file: Option<File>,
    endpoints: Endpoints,
    /// `READS` reads, then the write. Boxed on purpose: the kernel keeps their addresses while
    /// they are queued, whatever happens to the vector.
    #[allow(clippy::vec_box)]
    slots: Vec<Box<Transfer>>,
}

/// What came back from the phone.
enum Done {
    Read(usize),
    Written,
}

impl Accessor {
    fn open(dev: &UsbDevice, endpoints: Endpoints) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(dev.node())?;
        let mut interface = endpoints.interface as c_uint;
        // SAFETY: a pointer to an unsigned int, for the call.
        check(unsafe {
            libc::ioctl(
                file.as_raw_fd(),
                USBDEVFS_CLAIMINTERFACE as _,
                &mut interface,
            )
        })?;
        let slots = (0..=READS)
            .map(|i| {
                let mut t = Box::new(Transfer {
                    urb: Urb {
                        kind: URB_TYPE_BULK,
                        endpoint: if i < READS {
                            endpoints.input
                        } else {
                            endpoints.output
                        },
                        status: 0,
                        flags: 0,
                        buffer: std::ptr::null_mut(),
                        buffer_length: 0,
                        actual_length: 0,
                        start_frame: 0,
                        number_of_packets: 0,
                        error_count: 0,
                        signr: 0,
                        usercontext: i as *mut c_void,
                    },
                    buf: vec![0; BUFFER],
                    queued: false,
                });
                t.urb.buffer = t.buf.as_mut_ptr().cast();
                t
            })
            .collect();
        Ok(Self {
            file: Some(file),
            endpoints,
            slots,
        })
    }

    fn fd(&self) -> c_int {
        self.file.as_ref().map_or(-1, |f| f.as_raw_fd())
    }

    fn submit(&mut self, slot: usize, len: usize, flags: c_uint) -> io::Result<()> {
        let fd = self.fd();
        let t = &mut self.slots[slot];
        t.urb.buffer_length = len as c_int;
        t.urb.actual_length = 0;
        t.urb.status = 0;
        t.urb.flags = flags;
        // SAFETY: the URB and its buffer are boxed and outlive the transfer: Drop cancels and
        // reaps every queued URB, then closes the device, before freeing them.
        check(unsafe { libc::ioctl(fd, USBDEVFS_SUBMITURB as _, &mut t.urb) })?;
        t.queued = true;
        Ok(())
    }

    fn submit_reads(&mut self) -> io::Result<()> {
        for slot in 0..READS {
            self.submit(slot, BUFFER, 0)?;
        }
        Ok(())
    }

    fn writing(&self) -> bool {
        self.slots[WRITE_SLOT].queued
    }

    /// Sends `bytes` (at most [`BUFFER`]); only when the previous write is done.
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.slots[WRITE_SLOT].buf[..bytes.len()].copy_from_slice(bytes);
        self.submit(WRITE_SLOT, bytes.len(), URB_ZERO_PACKET)
    }

    /// A transfer that completed, if any (without waiting). An error once the phone is gone.
    fn reap(&mut self) -> io::Result<Option<(usize, Done)>> {
        let mut urb: *mut Urb = std::ptr::null_mut();
        // SAFETY: the kernel writes the address of one of our URBs into `urb`.
        let ret = unsafe { libc::ioctl(self.fd(), USBDEVFS_REAPURBNDELAY as _, &mut urb) };
        if ret < 0 {
            let e = io::Error::last_os_error();
            return if e.raw_os_error() == Some(libc::EAGAIN) {
                Ok(None)
            } else {
                Err(e)
            };
        }
        // SAFETY: `urb` is one of the URBs we submitted, still boxed in `slots`.
        let (slot, status, actual) = unsafe {
            (
                (*urb).usercontext as usize,
                (*urb).status,
                (*urb).actual_length as usize,
            )
        };
        let t = &mut self.slots[slot];
        t.queued = false;
        if status != 0 {
            return Err(io::Error::from_raw_os_error(-status));
        }
        Ok(Some(if slot < READS {
            (slot, Done::Read(actual))
        } else {
            (slot, Done::Written)
        }))
    }

    fn data(&self, slot: usize, len: usize) -> &[u8] {
        &self.slots[slot].buf[..len]
    }
}

impl Drop for Accessor {
    fn drop(&mut self) {
        let fd = self.fd();
        for t in self.slots.iter_mut().filter(|t| t.queued) {
            // SAFETY: cancelling one of our queued URBs.
            unsafe { libc::ioctl(fd, USBDEVFS_DISCARDURB as _, &mut t.urb) };
        }
        // Cancelled URBs complete at once; give up after a second (phone gone: nothing comes).
        let deadline = Instant::now() + Duration::from_secs(1);
        while self.slots.iter().any(|t| t.queued) && Instant::now() < deadline {
            match self.reap() {
                Ok(Some(_)) => {}
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(e) if e.raw_os_error() == Some(libc::ENODEV) => break,
                Err(_) => {} // a cancelled one, reaped with its error
            }
        }
        let mut interface = self.endpoints.interface as c_uint;
        // SAFETY: as when claiming.
        unsafe { libc::ioctl(fd, USBDEVFS_RELEASEINTERFACE as _, &mut interface) };
        // Closed before the buffers are freed: the kernel forgets what was still queued.
        drop(self.file.take());
    }
}

/// Waits until the phone or the host has something, at most `timeout`. Returns (usb, tcp) ready.
fn wait(
    usb: c_int,
    tcp: Option<c_int>,
    timeout: Duration,
) -> io::Result<(libc::c_short, libc::c_short)> {
    let mut fds = [
        libc::pollfd {
            fd: usb,
            events: libc::POLLOUT,
            revents: 0,
        },
        libc::pollfd {
            fd: tcp.unwrap_or(-1),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    // SAFETY: two pollfd structures we own (a negative fd is ignored by poll).
    let ret = unsafe { libc::poll(fds.as_mut_ptr(), 2, timeout.as_millis() as c_int) };
    if ret < 0 {
        let e = io::Error::last_os_error();
        return if e.kind() == io::ErrorKind::Interrupted {
            Ok((0, 0))
        } else {
            Err(e)
        };
    }
    Ok((fds[0].revents, fds[1].revents))
}

/// Relays one session between the accessory and the host's port. `Ok` when the session is over,
/// the phone left or the watch stopped; `Err` when the accessory cannot be opened.
fn relay(
    dev: &UsbDevice,
    endpoints: Endpoints,
    port: u16,
    stop: &AtomicBool,
) -> Result<(), String> {
    let mut usb = Accessor::open(dev, endpoints)
        .map_err(|e| format!("cannot open the phone in accessory mode ({e})"))?;
    if usb.submit_reads().is_err() {
        return Ok(());
    }
    let mut tcp: Option<TcpStream> = None;
    let mut chunk = vec![0u8; BUFFER];
    let unplugged = |tcp: &Option<TcpStream>| {
        if let Some(t) = tcp {
            let _ = t.shutdown(Shutdown::Both);
        }
        Ok(())
    };
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        // While the host has bytes in the previous write, leave its socket alone.
        let host = tcp
            .as_ref()
            .filter(|_| !usb.writing())
            .map(|t| t.as_raw_fd());
        let Ok((usb_ready, tcp_ready)) = wait(usb.fd(), host, SCAN_EVERY) else {
            return unplugged(&tcp);
        };
        if usb_ready & (libc::POLLERR | libc::POLLHUP) != 0 {
            return unplugged(&tcp);
        }
        loop {
            match usb.reap() {
                Ok(None) => break,
                Ok(Some((slot, Done::Read(n)))) => {
                    if n > 0 && (tcp.is_some() || starts_a_session(usb.data(slot, n))) {
                        if tcp.is_none() {
                            // The phone spoke: now the host gets a USB phone.
                            let t = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
                                .map_err(|e| format!("cannot reach the receiver ({e})"))?;
                            let _ = t.set_nodelay(true);
                            tcp = Some(t);
                        }
                        let t = tcp.as_mut().expect("connected above");
                        if t.write_all(usb.data(slot, n)).is_err() {
                            return Ok(()); // the host ended the session
                        }
                    }
                    if usb.submit(slot, BUFFER, 0).is_err() {
                        return unplugged(&tcp);
                    }
                }
                Ok(Some((_, Done::Written))) => {}
                Err(_) => return unplugged(&tcp),
            }
        }
        if tcp_ready & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 && !usb.writing() {
            let t = tcp.as_mut().expect("polled only when connected");
            match t.read(&mut chunk) {
                Ok(0) | Err(_) => {
                    // Session over. A zero-length packet ends the phone's pending read, so
                    // the app can close the accessory (Android only lets it go then).
                    if usb.write(&[]).is_ok() {
                        let deadline = Instant::now() + Duration::from_millis(300);
                        while usb.writing() && Instant::now() < deadline {
                            let _ = wait(usb.fd(), None, Duration::from_millis(50));
                            while let Ok(Some(_)) = usb.reap() {}
                        }
                    }
                    break;
                }
                Ok(n) => {
                    if usb.write(&chunk[..n]).is_err() {
                        return unplugged(&tcp);
                    }
                }
            }
        }
    }
    if let Some(t) = &tcp {
        let _ = t.shutdown(Shutdown::Both);
    }
    Ok(())
}

/// A phone's preamble (§4): what a new session starts with. The version is left to the host,
/// which tells an old app why it refuses it.
fn starts_a_session(bytes: &[u8]) -> bool {
    bytes.len() >= PREAMBLE_LEN && bytes[..4] == MAGIC && bytes[6] == Role::Camera as u8
}

// ---------------------------------------------------------------- the watch

/// Follows the phones plugged in; stops when dropped.
pub struct Watch {
    stop: Arc<AtomicBool>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Switches every phone plugged in (now and later) to accessory mode, and relays each phone in
/// accessory mode to the host on 127.0.0.1:`port`. `report` hears what happens.
pub fn watch(port: u16, report: impl Fn(Accessory) + Send + Sync + 'static) -> Watch {
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let report = Arc::new(report);
    let _ = thread::Builder::new()
        .name("oculibre-aoa".into())
        .spawn(move || {
            // Devices asked to switch, and accessories that cannot be opened: once per plug
            // (a device number changes at each plug).
            let mut tried: HashSet<(u32, u32)> = HashSet::new();
            let failed: Arc<Mutex<HashSet<(u32, u32)>>> = Arc::default();
            let relaying: Arc<Mutex<HashSet<(u32, u32)>>> = Arc::default();
            while !stopped.load(Ordering::Relaxed) {
                let plugged = devices(Path::new(SYSFS));
                for dev in &plugged {
                    let key = dev.key();
                    if dev.is_accessory() {
                        if failed.lock().unwrap().contains(&key)
                            || !relaying.lock().unwrap().insert(key)
                        {
                            continue; // cannot be opened, or already relayed
                        }
                        let Some(endpoints) = accessory_endpoints(dev) else {
                            relaying.lock().unwrap().remove(&key);
                            continue; // sysfs not complete yet
                        };
                        if tried.insert(key) {
                            report(Accessory::Waiting);
                        }
                        let (dev, stop) = (dev.clone(), stopped.clone());
                        let (relaying, failed, report) =
                            (relaying.clone(), failed.clone(), report.clone());
                        let _ = thread::Builder::new()
                            .name("oculibre-aoa-relay".into())
                            .spawn(move || {
                                if let Err(e) = relay(&dev, endpoints, port, &stop) {
                                    failed.lock().unwrap().insert(dev.key());
                                    report(Accessory::Failed(e));
                                }
                                relaying.lock().unwrap().remove(&dev.key());
                            });
                    } else if dev.class != HUB_CLASS && tried.insert(key) {
                        if let Ok(true) = switch(dev) {
                            report(Accessory::Switched {
                                phone: dev.label.clone(),
                            });
                        }
                    }
                }
                let present: HashSet<_> = plugged.iter().map(UsbDevice::key).collect();
                tried.retain(|k| present.contains(k));
                failed.lock().unwrap().retain(|k| present.contains(k));
                thread::sleep(SCAN_EVERY);
            }
        });
    Watch { stop }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oculibre_protocol::Preamble;

    #[test]
    fn only_a_phone_preamble_starts_a_session() {
        let phone = Preamble::new(Role::Camera).encode();
        assert!(starts_a_session(&phone));
        let mut old = phone;
        old[4] = 0; // another major version: the host explains
        assert!(starts_a_session(&old));
        assert!(!starts_a_session(&Preamble::new(Role::Host).encode()));
        assert!(!starts_a_session(&[0x21, 0x00, 0x01, 0x00, 1, 2, 3, 4])); // video left over
        assert!(!starts_a_session(b"OCUL"));
    }

    #[test]
    fn structures_match_the_kernel() {
        // linux/usbdevice_fs.h on 64-bit: the ioctl numbers carry these sizes.
        if size_of::<*mut c_void>() == 8 {
            assert_eq!(size_of::<CtrlTransfer>(), 24);
            assert_eq!(size_of::<Urb>(), 56);
            assert_eq!(USBDEVFS_SUBMITURB, 0x8038_550a);
            assert_eq!(USBDEVFS_REAPURBNDELAY, 0x4008_550d);
            assert_eq!(USBDEVFS_CONTROL, 0xc018_5500);
        }
        assert_eq!(USBDEVFS_CLAIMINTERFACE, 0x8004_550f);
        assert_eq!(USBDEVFS_DISCARDURB, 0x550b);
    }

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), format!("{text}\n")).unwrap();
    }

    #[test]
    fn finds_phones_and_the_accessory_endpoints() {
        let root = std::env::temp_dir().join(format!("oculibre-aoa-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let device =
            |name: &str, bus: &str, dev: &str, vendor: &str, product: &str, class: &str| {
                let d = root.join(name);
                for (file, text) in [
                    ("busnum", bus),
                    ("devnum", dev),
                    ("idVendor", vendor),
                    ("idProduct", product),
                    ("bDeviceClass", class),
                ] {
                    write(&d, file, text);
                }
                d
            };
        device("usb1", "1", "1", "1d6b", "0002", "09");
        let phone = device("1-4", "1", "7", "18d1", "2d01", "00");
        write(&phone, "manufacturer", "Xiaomi");
        write(&phone, "product", "Redmi Note 9 Pro");
        std::fs::create_dir_all(root.join("1-4:1.0")).unwrap(); // an interface: left out
        let accessory = phone.join("1-4:1.0");
        write(&accessory, "bInterfaceNumber", "00");
        write(&accessory, "bInterfaceClass", "ff");
        write(&accessory, "bInterfaceSubClass", "ff");
        write(&accessory.join("ep_81"), "type", "Bulk");
        write(&accessory.join("ep_81"), "bEndpointAddress", "81");
        write(&accessory.join("ep_02"), "type", "Bulk");
        write(&accessory.join("ep_02"), "bEndpointAddress", "02");
        let adb = phone.join("1-4:1.1");
        write(&adb, "bInterfaceNumber", "01");
        write(&adb, "bInterfaceClass", "ff");
        write(&adb, "bInterfaceSubClass", "42");

        let found = devices(&root);
        assert_eq!(found.len(), 2);
        let phone = &found[1];
        assert!(phone.is_accessory() && !found[0].is_accessory());
        assert_eq!(phone.label, "Xiaomi Redmi Note 9 Pro");
        assert_eq!(phone.node(), PathBuf::from("/dev/bus/usb/001/007"));
        assert_eq!(found[0].class, HUB_CLASS);
        assert_eq!(
            accessory_endpoints(phone),
            Some(Endpoints {
                interface: 0,
                input: 0x81,
                output: 0x02
            })
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
