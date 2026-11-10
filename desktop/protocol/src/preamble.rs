//! Preamble, §4: 8 bytes sent by each side right after the connection is established.

use crate::{Error, MAJOR, MINOR};

pub const PREAMBLE_LEN: usize = 8;
pub const MAGIC: [u8; 4] = *b"OCUL";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Camera = 1,
    Host = 2,
}

impl Role {
    pub fn from_u8(v: u8) -> Result<Self, Error> {
        match v {
            1 => Ok(Role::Camera),
            2 => Ok(Role::Host),
            other => Err(Error::BadRole(other)),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Role::Camera => "camera",
            Role::Host => "host",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preamble {
    pub major: u8,
    pub minor: u8,
    pub role: Role,
}

impl Preamble {
    /// Our own preamble, with the version implemented by this crate.
    pub fn new(role: Role) -> Self {
        Self {
            major: MAJOR,
            minor: MINOR,
            role,
        }
    }

    pub fn encode(&self) -> [u8; PREAMBLE_LEN] {
        let mut b = [0u8; PREAMBLE_LEN];
        b[..4].copy_from_slice(&MAGIC);
        b[4] = self.major;
        b[5] = self.minor;
        b[6] = self.role as u8;
        b[7] = 0;
        b
    }

    /// Parses a peer preamble. The reserved byte is ignored.
    pub fn decode(b: &[u8; PREAMBLE_LEN]) -> Result<Self, Error> {
        let magic = [b[0], b[1], b[2], b[3]];
        if magic != MAGIC {
            return Err(Error::BadMagic(magic));
        }
        Ok(Self {
            major: b[4],
            minor: b[5],
            role: Role::from_u8(b[6])?,
        })
    }

    /// Checks that `peer` can talk to us (§4): different roles, same major version.
    /// A different minor is fine; the session then relies on features.
    pub fn check_peer(&self, peer: &Preamble) -> Result<(), Error> {
        if peer.role == self.role {
            return Err(Error::SameRole);
        }
        if peer.major != self.major {
            return Err(Error::IncompatibleMajor {
                local: self.major,
                peer: peer.major,
                peer_minor: peer.minor,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let p = Preamble::new(Role::Camera);
        assert_eq!(p.encode(), [0x4F, 0x43, 0x55, 0x4C, MAJOR, MINOR, 1, 0]);
        assert_eq!(Preamble::decode(&p.encode()).unwrap(), p);
    }

    #[test]
    fn rejects_bad_magic_and_role() {
        assert_eq!(
            Preamble::decode(b"HTTP/1.1").unwrap_err().kind(),
            "bad_magic"
        );
        assert_eq!(
            Preamble::decode(&[b'O', b'C', b'U', b'L', 0, 1, 9, 0])
                .unwrap_err()
                .kind(),
            "bad_role"
        );
    }

    #[test]
    fn peer_checks() {
        let host = Preamble::new(Role::Host);
        assert!(host.check_peer(&Preamble::new(Role::Camera)).is_ok());
        assert_eq!(
            host.check_peer(&Preamble::new(Role::Host))
                .unwrap_err()
                .kind(),
            "same_role"
        );
        let newer = Preamble {
            major: MAJOR + 1,
            minor: 0,
            role: Role::Camera,
        };
        assert_eq!(
            host.check_peer(&newer).unwrap_err().kind(),
            "incompatible_major"
        );
        let newer_minor = Preamble {
            major: MAJOR,
            minor: MINOR + 3,
            role: Role::Camera,
        };
        assert!(host.check_peer(&newer_minor).is_ok());
    }
}
