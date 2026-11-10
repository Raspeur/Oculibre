//! Hostile input must never panic, hang or allocate beyond the limits (§14).
//! The `fuzz/` crate does the same with coverage guidance (needs nightly); this test runs
//! everywhere, with a fixed seed.

use std::io::Cursor;

use oculibre_protocol::{vectors, Header, Message, MessageReader, HEADER_LEN};

struct XorShift(u64);
impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn drain(bytes: Vec<u8>) {
    let mut r = MessageReader::new(Cursor::new(bytes));
    let _ = r.read_preamble();
    for _ in 0..64 {
        if r.read_message().is_err() {
            break;
        }
    }
}

#[test]
fn random_bytes() {
    let mut rng = XorShift(0x0C01_1B2E);
    for _ in 0..20_000 {
        let len = (rng.next() % 200) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        drain(bytes);
    }
}

#[test]
fn mutated_valid_messages() {
    let mut rng = XorShift(0xDEC0_DE42);
    let seeds: Vec<Vec<u8>> = vectors::all().into_iter().map(|v| v.bytes).collect();
    for _ in 0..50_000 {
        let mut b = seeds[(rng.next() % seeds.len() as u64) as usize].clone();
        if b.is_empty() {
            continue;
        }
        for _ in 0..1 + rng.next() % 4 {
            let i = (rng.next() % b.len() as u64) as usize;
            b[i] = rng.next() as u8;
        }
        if b.len() >= HEADER_LEN {
            let mut hb = [0u8; HEADER_LEN];
            hb.copy_from_slice(&b[..HEADER_LEN]);
            if let Ok(h) = Header::decode(&hb) {
                let _ = Message::decode(&h, &b[HEADER_LEN..]);
            }
        }
        drain(b);
    }
}
