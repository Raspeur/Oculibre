//! USB without developer mode (#21): Android Open Accessory (AOA).
//!
//! The computer, as USB host, asks a plugged phone to switch to accessory mode with three vendor
//! control requests. The phone comes back as Google's accessory device (18d1:2d00, or 2d01 when
//! USB debugging is on), Android offers to open Oculibre, and the app reads and writes two bulk
//! endpoints. Like `adb reverse`, [`watch`] relays those bytes to the host's own port on
//! 127.0.0.1: the session sees a USB phone on loopback, in plaintext (the cable is trusted, §13).
//!
//! Which phones are switched: those switched before on this computer (remembered by a hash of their
//! USB serial number), the ones the user asks for ([`Watch::switch`], after
//! [`Accessory::Detected`]), or every one when [`Watch::set_switch_all`] says so. Switching a phone
//! nobody asked for would show Android's "Open Oculibre" on any phone plugged in to charge, and
//! take its file transfer away.
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

use std::collections::{HashMap, HashSet};
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

use crate::events::{Accessory, UsbPhone};

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
    /// Its USB serial number, when it has one: only ever kept as a hash ([`Known`]).
    serial: Option<String>,
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
            let serial = read_text(&sys.join("serial")).filter(|s| !s.is_empty());
            Some(UsbDevice {
                bus: read_text(&sys.join("busnum"))?.parse().ok()?,
                dev: read_text(&sys.join("devnum"))?.parse().ok()?,
                vendor: read_hex(&sys.join("idVendor"))?,
                product: read_hex(&sys.join("idProduct"))?,
                class: read_hex(&sys.join("bDeviceClass")).unwrap_or(0) as u8,
                label,
                serial,
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

/// What a USB device answered when asked whether it speaks AOA.
enum Probe {
    /// An Android device, opened.
    Android(File),
    /// Not one (or it went away).
    Not,
    /// This user may not open it, at least not yet: udev gives access a moment after the device
    /// appears (systemd's uaccess, after libmtp's probe).
    NoAccess,
}

/// Asks a device whether it speaks AOA (an Android device). Asking does not switch anything: no
/// prompt on the phone.
fn android(dev: &UsbDevice) -> Probe {
    let file = match OpenOptions::new().read(true).write(true).open(dev.node()) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Probe::NoAccess,
        Err(_) => return Probe::Not,
    };
    let mut version = [0u8; 2];
    match control(&file, 0xc0, GET_PROTOCOL, 0, &mut version) {
        Ok(2) if u16::from_le_bytes(version) > 0 => Probe::Android(file),
        _ => Probe::Not,
    }
}

/// How long a device that cannot be opened yet is looked at again (udev's access comes late).
const ACCESS_GRACE: Duration = Duration::from_secs(10);

/// Switches an Android device ([`android`]) to accessory mode: it leaves, and comes back as the
/// accessory a second later.
fn switch(file: &File) -> io::Result<()> {
    for (index, text) in STRINGS.iter().enumerate() {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        control(file, 0x40, SEND_STRING, index as u16, &mut bytes)?;
    }
    control(file, 0x40, START, 0, &mut [])?;
    Ok(())
}

// ---------------------------------------------------------------- phones switched before

/// The phones switched before on this computer, switched again without asking. Kept as hashes of
/// their USB serial number salted with this computer's id: the file never holds the number (a
/// hardware identifier, §7.1), and the same phone gets another hash on another computer. Once a
/// phone streamed over the cable, its hash is tied to its install id (`HELLO.id`), so that
/// forgetting the phone in the app forgets its cable too.
struct Known {
    file: Option<PathBuf>,
    salt: String,
    /// Hash → the phone's install id, once it streamed.
    phones: HashMap<String, Option<String>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct KnownPhone {
    usb: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct KnownFile {
    phones: Vec<KnownPhone>,
}

impl Known {
    /// `file`: where they are kept (`None`: in memory only).
    fn load(file: Option<PathBuf>, salt: String) -> Self {
        let phones = file
            .as_ref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|text| serde_json::from_str::<KnownFile>(&text).ok())
            .map(|k| k.phones.into_iter().map(|p| (p.usb, p.id)).collect())
            .unwrap_or_default();
        Self { file, salt, phones }
    }

    fn hash(&self, serial: &str) -> String {
        let input = format!("{}\n{serial}", self.salt);
        let digest = ring::digest::digest(&ring::digest::SHA256, input.as_bytes());
        digest.as_ref()[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// A phone without a serial number is never known: it is asked about at each plug.
    fn contains(&self, serial: Option<&str>) -> bool {
        serial.is_some_and(|s| self.phones.contains_key(&self.hash(s)))
    }

    fn remember(&mut self, serial: Option<&str>) {
        let Some(serial) = serial else { return };
        let hash = self.hash(serial);
        if self.phones.contains_key(&hash) {
            return;
        }
        self.phones.insert(hash, None);
        self.save();
    }

    /// The phone with this hash streamed: it is this install id.
    fn tie(&mut self, hash: &str, id: &str) {
        if self.phones.get(hash).and_then(|i| i.as_deref()) == Some(id) {
            return;
        }
        self.phones.insert(hash.to_string(), Some(id.to_string()));
        self.save();
    }

    /// Forgets the cables of this install id: it will be asked about again.
    fn forget(&mut self, id: &str) {
        let before = self.phones.len();
        self.phones.retain(|_, i| i.as_deref() != Some(id));
        if self.phones.len() != before {
            self.save();
        }
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        let mut phones: Vec<_> = self
            .phones
            .iter()
            .map(|(usb, id)| KnownPhone {
                usb: usb.clone(),
                id: id.clone(),
            })
            .collect();
        phones.sort_by(|a, b| a.usb.cmp(&b.usb));
        if let Ok(text) = serde_json::to_string_pretty(&KnownFile { phones }) {
            let _ = crate::store::save_private(file, text.as_bytes());
        }
    }
}

/// Forgets the cables of a phone in the file of the phones switched before, when no watch runs.
pub fn forget(file: &Path, id: &str) {
    Known::load(Some(file.to_path_buf()), String::new()).forget(id);
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
/// Which relay connection carries which phone: the local port of the relay's connection to the
/// host → the hash of the phone's serial number ([`Watch::tie`]).
type Links = Arc<Mutex<HashMap<u16, String>>>;

fn relay(
    dev: &UsbDevice,
    endpoints: Endpoints,
    port: u16,
    stop: &AtomicBool,
    link: Option<(&Links, &str)>,
) -> Result<(), String> {
    let result = relay_bytes(dev, endpoints, port, stop, link);
    if let Some((links, hash)) = link {
        links.lock().unwrap().retain(|_, h| h != hash);
    }
    result
}

fn relay_bytes(
    dev: &UsbDevice,
    endpoints: Endpoints,
    port: u16,
    stop: &AtomicBool,
    link: Option<(&Links, &str)>,
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
                            if let (Some((links, hash)), Ok(local)) = (link, t.local_addr()) {
                                links.lock().unwrap().insert(local.port(), hash.to_string());
                            }
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

/// A device as [`Accessory::Detected`] names it: bus and device number ("1:7"), new at each plug.
fn device_id((bus, dev): (u32, u32)) -> String {
    format!("{bus}:{dev}")
}

fn parse_device_id(id: &str) -> Option<(u32, u32)> {
    let (bus, dev) = id.split_once(':')?;
    Some((bus.parse().ok()?, dev.parse().ok()?))
}

/// Follows the phones plugged in; stops when dropped.
pub struct Watch {
    stop: Arc<AtomicBool>,
    all: Arc<AtomicBool>,
    /// Phones the user asked to switch ([`Watch::switch`]).
    wanted: Arc<Mutex<HashSet<(u32, u32)>>>,
    known: Arc<Mutex<Known>>,
    links: Links,
    /// Phones announced ([`Accessory::Detected`]) and waiting for an answer: device → its label.
    announced: Arc<Mutex<HashMap<(u32, u32), String>>>,
}

impl Watch {
    /// Switch every Android phone plugged in, known or not (the user's choice, off by default).
    pub fn set_switch_all(&self, all: bool) {
        self.all.store(all, Ordering::Relaxed);
    }

    /// The phone that came from `peer` (the relay's connection) said it is `id` (its HELLO): its
    /// cable is tied to it. Nothing for other connections.
    pub fn tie(&self, peer: std::net::SocketAddr, id: &str) {
        if !peer.ip().is_loopback() || id.is_empty() {
            return;
        }
        let hash = self.links.lock().unwrap().get(&peer.port()).cloned();
        if let Some(hash) = hash {
            self.known.lock().unwrap().tie(&hash, id);
        }
    }

    /// The phones announced and still waiting for an answer, for a window that opens after the
    /// announcement (the engine starts before the window listens).
    pub fn announced(&self) -> Vec<UsbPhone> {
        let mut phones: Vec<_> = self
            .announced
            .lock()
            .unwrap()
            .iter()
            .map(|(key, phone)| UsbPhone {
                id: device_id(*key),
                phone: phone.clone(),
            })
            .collect();
        phones.sort_by(|a, b| a.id.cmp(&b.id));
        phones
    }

    /// Forgets the cables of the phone `id`: it will be asked about again.
    pub fn forget(&self, id: &str) {
        self.known.lock().unwrap().forget(id);
    }

    /// Switches a phone [`Accessory::Detected`] announced (`id`), and remembers it.
    pub fn switch(&self, id: &str) -> Result<(), String> {
        let key = parse_device_id(id).ok_or_else(|| format!("not a USB device: {id}"))?;
        self.wanted.lock().unwrap().insert(key);
        Ok(())
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Follows the USB devices: switches the phones it should (see the module documentation) and
/// announces the others, then relays each phone in accessory mode to the host on
/// 127.0.0.1:`port`. `all`: switch every phone. `known`: the file of the phones switched before.
/// `report` hears what happens.
pub fn watch(
    port: u16,
    all: bool,
    known: Option<PathBuf>,
    report: impl Fn(Accessory) + Send + Sync + 'static,
) -> Watch {
    let stop = Arc::new(AtomicBool::new(false));
    let switch_all = Arc::new(AtomicBool::new(all));
    let wanted: Arc<Mutex<HashSet<(u32, u32)>>> = Arc::default();
    let known = Arc::new(Mutex::new(Known::load(known, crate::host::host_id())));
    let links: Links = Arc::default();
    let announced: Arc<Mutex<HashMap<(u32, u32), String>>> = Arc::default();
    let watch = Watch {
        announced: announced.clone(),
        stop: stop.clone(),
        all: switch_all.clone(),
        wanted: wanted.clone(),
        known: known.clone(),
        links: links.clone(),
    };
    let report = Arc::new(report);
    let _ = thread::Builder::new()
        .name("oculibre-aoa".into())
        .spawn(move || {
            // Each plug is looked at once (a device number changes at each plug); phones
            // announced wait for the user; accessories that cannot be opened are left alone.
            let mut seen: HashSet<(u32, u32)> = HashSet::new();
            // When each device was first seen, for the ones not accessible yet.
            let mut appeared: HashMap<(u32, u32), Instant> = HashMap::new();
            let failed: Arc<Mutex<HashSet<(u32, u32)>>> = Arc::default();
            let relaying: Arc<Mutex<HashSet<(u32, u32)>>> = Arc::default();
            while !stop.load(Ordering::Relaxed) {
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
                        if seen.insert(key) {
                            report(Accessory::Waiting);
                        }
                        let (dev, stop, links) = (dev.clone(), stop.clone(), links.clone());
                        let hash = dev.serial.as_deref().map(|s| known.lock().unwrap().hash(s));
                        let (relaying, failed, report) =
                            (relaying.clone(), failed.clone(), report.clone());
                        let _ = thread::Builder::new()
                            .name("oculibre-aoa-relay".into())
                            .spawn(move || {
                                let link = hash.as_deref().map(|h| (&links, h));
                                if let Err(e) = relay(&dev, endpoints, port, &stop, link) {
                                    failed.lock().unwrap().insert(dev.key());
                                    report(Accessory::Failed(e));
                                }
                                relaying.lock().unwrap().remove(&dev.key());
                            });
                        continue;
                    }
                    if dev.class == HUB_CLASS {
                        continue;
                    }
                    let first = seen.insert(key);
                    if !first && !announced.lock().unwrap().contains_key(&key) {
                        continue; // not a phone, or switched already
                    }
                    let asked = wanted.lock().unwrap().remove(&key);
                    let all = switch_all.load(Ordering::Relaxed);
                    let serial = dev.serial.as_deref();
                    let now = asked || all || first && known.lock().unwrap().contains(serial);
                    if !now && !first {
                        continue; // announced, waiting for the user
                    }
                    let file = match android(dev) {
                        Probe::Android(file) => file,
                        Probe::NoAccess
                            if appeared.entry(key).or_insert_with(Instant::now).elapsed()
                                < ACCESS_GRACE =>
                        {
                            seen.remove(&key); // look again at the next scan
                            continue;
                        }
                        _ => continue, // not a phone, or never ours
                    };
                    if !now {
                        announced.lock().unwrap().insert(key, dev.label.clone());
                        report(Accessory::Detected {
                            id: device_id(key),
                            phone: dev.label.clone(),
                        });
                        continue;
                    }
                    if announced.lock().unwrap().remove(&key).is_some() {
                        report(Accessory::Gone { id: device_id(key) }); // answered: no more question
                    }
                    match switch(&file) {
                        Ok(()) => {
                            known.lock().unwrap().remember(serial);
                            report(Accessory::Switched {
                                phone: dev.label.clone(),
                            });
                        }
                        Err(e) => report(Accessory::Failed(format!(
                            "{} could not switch to accessory mode ({e})",
                            dev.label
                        ))),
                    }
                }
                let present: HashSet<_> = plugged.iter().map(UsbDevice::key).collect();
                let mut waiting = announced.lock().unwrap();
                for key in waiting.keys().filter(|k| !present.contains(k)) {
                    report(Accessory::Gone {
                        id: device_id(*key),
                    });
                }
                waiting.retain(|k, _| present.contains(k));
                seen.retain(|k| present.contains(k));
                appeared.retain(|k, _| present.contains(k));
                failed.lock().unwrap().retain(|k| present.contains(k));
                wanted.lock().unwrap().retain(|k| waiting.contains_key(k));
                drop(waiting);
                thread::sleep(SCAN_EVERY);
            }
        });
    watch
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
    fn known_phones_are_hashes_salted_by_the_computer() {
        let file = std::env::temp_dir().join(format!("oculibre-usb-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let mut known = Known::load(Some(file.clone()), "computer-a".into());
        assert!(!known.contains(Some("ABC123")));
        known.remember(Some("ABC123"));
        known.remember(None); // no serial number: nothing to remember
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(!text.contains("ABC123"), "never the number itself: {text}");
        let again = Known::load(Some(file.clone()), "computer-a".into());
        assert!(again.contains(Some("ABC123")));
        assert!(!again.contains(Some("XYZ")) && !again.contains(None));
        let elsewhere = Known::load(Some(file.clone()), "computer-b".into());
        assert!(
            !elsewhere.contains(Some("ABC123")),
            "another computer, another hash"
        );

        // It streamed: tied to its install id; forgetting that id forgets the cable.
        let mut known = Known::load(Some(file.clone()), "computer-a".into());
        let hash = known.hash("ABC123");
        known.tie(&hash, "phone-1");
        known.remember(Some("OTHER"));
        forget(&file, "phone-1");
        let after = Known::load(Some(file.clone()), "computer-a".into());
        assert!(!after.contains(Some("ABC123")));
        assert!(after.contains(Some("OTHER")), "the other phone stays");
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn device_ids_go_both_ways() {
        assert_eq!(device_id((1, 7)), "1:7");
        assert_eq!(parse_device_id("1:7"), Some((1, 7)));
        assert_eq!(parse_device_id("1-4"), None);
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
        write(&phone, "serial", "abc123");
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
        assert_eq!(phone.serial.as_deref(), Some("abc123"));
        assert_eq!(found[0].serial, None);
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
