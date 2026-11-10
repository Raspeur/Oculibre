//! Message header, §5: 16 bytes, big-endian.

use crate::{Error, MAX_MESSAGE_LEN};

pub const HEADER_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Header {
    pub msg_type: u8,
    pub flags: u8,
    pub stream: u8,
    pub length: u32,
    /// Microseconds; camera clock for media, sender clock otherwise; 0 = none.
    pub timestamp: u64,
}

impl Header {
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[0] = self.msg_type;
        b[1] = self.flags;
        b[2] = self.stream;
        b[3] = 0; // reserved
        b[4..8].copy_from_slice(&self.length.to_be_bytes());
        b[8..16].copy_from_slice(&self.timestamp.to_be_bytes());
        b
    }

    /// Parses a header and enforces the maximum message length. The reserved byte is ignored.
    pub fn decode(b: &[u8; HEADER_LEN]) -> Result<Self, Error> {
        let length = u32::from_be_bytes([b[4], b[5], b[6], b[7]]);
        if length > MAX_MESSAGE_LEN {
            return Err(Error::TooLarge {
                length,
                max: MAX_MESSAGE_LEN,
            });
        }
        Ok(Self {
            msg_type: b[0],
            flags: b[1],
            stream: b[2],
            length,
            timestamp: u64::from_be_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_layout() {
        let h = Header {
            msg_type: 0x21,
            flags: 1,
            stream: 1,
            length: 0x0001_0203,
            timestamp: 0x1122_3344_5566_7788,
        };
        let b = h.encode();
        assert_eq!(
            b,
            [0x21, 1, 1, 0, 0, 1, 2, 3, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]
        );
        assert_eq!(Header::decode(&b).unwrap(), h);
    }

    #[test]
    fn reserved_byte_ignored_and_limit_enforced() {
        let mut b = Header {
            msg_type: 3,
            length: 16,
            ..Default::default()
        }
        .encode();
        b[3] = 0xAB;
        assert_eq!(Header::decode(&b).unwrap().length, 16);
        b[4..8].copy_from_slice(&(MAX_MESSAGE_LEN + 1).to_be_bytes());
        assert_eq!(Header::decode(&b).unwrap_err().kind(), "too_large");
    }
}
