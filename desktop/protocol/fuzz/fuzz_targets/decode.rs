//! Coverage-guided fuzzing of the protocol parser (§14, §16).
//!
//!     cargo install cargo-fuzz
//!     cd desktop/protocol && cargo +nightly fuzz run decode
//!
//! Seed corpus: the shared test vectors (`tests/protocol/vectors/*.hex`, converted with
//! `xxd -r -p`). Any crash, hang or out-of-memory found here is a security bug.
#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use oculibre_protocol::{Header, Message, MessageReader, HEADER_LEN};

fuzz_target!(|data: &[u8]| {
    // As a stream: preamble, then as many messages as the bytes allow.
    let mut reader = MessageReader::new(Cursor::new(data));
    let _ = reader.read_preamble();
    for _ in 0..256 {
        if reader.read_message().is_err() {
            break;
        }
    }

    // As one message: header + whatever follows.
    if data.len() >= HEADER_LEN {
        let mut h = [0u8; HEADER_LEN];
        h.copy_from_slice(&data[..HEADER_LEN]);
        if let Ok(header) = Header::decode(&h) {
            if let Ok(m) = Message::decode(&header, &data[HEADER_LEN..]) {
                let _ = m.encode();
            }
        }
    }
});
