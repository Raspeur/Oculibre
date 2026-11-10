//! Reading and writing over any byte stream (TCP, a recorded file…).

use std::io::{self, Read, Write};

use crate::{types, Error, Header, Message, Preamble, HEADER_LEN, MAX_JSON_LEN, PREAMBLE_LEN};

/// Reads a preamble then messages from `R`.
///
/// Limits are enforced from the header alone, before any allocation: a hostile peer cannot make
/// us allocate more than 16 MiB per message, nor more than 64 KiB for a JSON message. Unknown
/// types are skipped without being buffered.
pub struct MessageReader<R> {
    inner: R,
    buf: Vec<u8>,
}

impl<R: Read> MessageReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            buf: Vec::new(),
        }
    }

    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    pub fn into_inner(self) -> R {
        self.inner
    }

    pub fn read_preamble(&mut self) -> Result<Preamble, Error> {
        let mut b = [0u8; PREAMBLE_LEN];
        self.read_exact_or_closed(&mut b)?;
        Preamble::decode(&b)
    }

    /// Next message. Returns [`Error::ConnectionClosed`] when the peer closes cleanly between
    /// two messages, and an I/O error (unexpected EOF) when it closes in the middle of one.
    pub fn read_message(&mut self) -> Result<Message, Error> {
        let mut hb = [0u8; HEADER_LEN];
        self.read_exact_or_closed(&mut hb)?;
        let header = Header::decode(&hb)?;
        let len = header.length as usize;

        if !types::is_decoded(header.msg_type) {
            let skipped = io::copy(
                &mut (&mut self.inner).take(header.length as u64),
                &mut io::sink(),
            )?;
            if skipped != header.length as u64 {
                return Err(Error::Io(io::ErrorKind::UnexpectedEof.into()));
            }
            return Message::decode(&header, &[]);
        }
        if types::is_json(header.msg_type) && len > MAX_JSON_LEN {
            return Err(Error::JsonTooLarge {
                msg_type: header.msg_type,
                length: header.length,
            });
        }

        self.buf.clear();
        self.buf.resize(len, 0);
        self.inner.read_exact(&mut self.buf)?;
        Message::decode(&header, &self.buf)
    }

    fn read_exact_or_closed(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.inner.read(&mut buf[filled..]) {
                Ok(0) if filled == 0 => return Err(Error::ConnectionClosed),
                Ok(0) => return Err(Error::Io(io::ErrorKind::UnexpectedEof.into())),
                Ok(n) => filled += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(Error::Io(e)),
            }
        }
        Ok(())
    }
}

/// Writes one message in a single `write_all` (header and payload together, so that a
/// `TCP_NODELAY` socket sends them in as few packets as possible).
pub fn write_message<W: Write>(w: &mut W, message: &Message) -> io::Result<()> {
    w.write_all(&message.encode())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ping, Role};
    use std::io::Cursor;

    #[test]
    fn reads_preamble_then_messages_then_clean_close() {
        let mut stream = Preamble::new(Role::Camera).encode().to_vec();
        stream.extend(Message::Ping(Ping { seq: 1, t1: 2 }).encode());
        stream.extend(
            Header {
                msg_type: 0x7A,
                length: 5,
                ..Default::default()
            }
            .encode(),
        );
        stream.extend([9; 5]); // unknown type, skipped
        stream.extend(Message::Ping(Ping { seq: 3, t1: 4 }).encode());

        let mut r = MessageReader::new(Cursor::new(stream));
        assert_eq!(r.read_preamble().unwrap().role, Role::Camera);
        assert_eq!(
            r.read_message().unwrap(),
            Message::Ping(Ping { seq: 1, t1: 2 })
        );
        assert!(matches!(
            r.read_message().unwrap(),
            Message::Unknown { msg_type: 0x7A, .. }
        ));
        assert_eq!(
            r.read_message().unwrap(),
            Message::Ping(Ping { seq: 3, t1: 4 })
        );
        assert_eq!(r.read_message().unwrap_err().kind(), "connection_closed");
    }

    #[test]
    fn truncated_message_is_an_io_error() {
        let mut bytes = Message::Ping(Ping { seq: 1, t1: 2 }).encode();
        bytes.truncate(20);
        let mut r = MessageReader::new(Cursor::new(bytes));
        assert_eq!(r.read_message().unwrap_err().kind(), "io");
    }

    #[test]
    fn huge_json_rejected_before_reading_payload() {
        let h = Header {
            msg_type: types::HELLO,
            length: (MAX_JSON_LEN + 1) as u32,
            ..Default::default()
        };
        let mut r = MessageReader::new(Cursor::new(h.encode().to_vec())); // no payload at all
        assert_eq!(r.read_message().unwrap_err().kind(), "json_too_large");
    }
}
