//! Splits a raw H.264 Annex B byte stream (as received from TCP, in arbitrary chunks)
//! into complete NAL units, each returned with its start code.
//!
//! A NAL is known to be complete only when the next start code arrives, so the last NAL
//! of a frame waits for the next frame (~1 frame of extra latency). The Oculibre framing
//! (length-prefixed messages, see docs/protocol.md) will remove that delay.

#[derive(Default)]
pub struct Splitter {
    buf: Vec<u8>,
}

/// Returns `(begin, payload)`: `begin` includes the start code (3 or 4 bytes),
/// `payload` is the first byte after it.
fn find_start_code(buf: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i + 3 <= buf.len() {
        if buf[i + 2] > 1 {
            // Fast skip: no start code can end at i + 2.
            i += 3;
            continue;
        }
        if buf[i] == 0 && buf[i + 1] == 0 && buf[i + 2] == 1 {
            let begin = if i > from && buf[i - 1] == 0 {
                i - 1
            } else {
                i
            };
            return Some((begin, i + 3));
        }
        i += 1;
    }
    None
}

impl Splitter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Next complete NAL unit (start code included), if any.
    pub fn next_nal(&mut self) -> Option<Vec<u8>> {
        let (begin, payload) = find_start_code(&self.buf, 0)?;
        let (next, _) = find_start_code(&self.buf, payload)?;
        let nal = self.buf[begin..next].to_vec();
        self.buf.drain(..next);
        Some(nal)
    }

    /// Bytes waiting for the next start code.
    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut s = Splitter::new();
        let mut out = Vec::new();
        for c in chunks {
            s.push(c);
            while let Some(n) = s.next_nal() {
                out.push(n);
            }
        }
        out
    }

    #[test]
    fn splits_three_and_four_byte_start_codes() {
        let stream: &[u8] = &[
            0, 0, 0, 1, 0x67, 0xAA, 0, 0, 1, 0x68, 0xBB, 0, 0, 0, 1, 0x65,
        ];
        let nals = collect(&[stream]);
        assert_eq!(
            nals,
            vec![vec![0, 0, 0, 1, 0x67, 0xAA], vec![0, 0, 1, 0x68, 0xBB]]
        );
    }

    #[test]
    fn handles_start_code_split_across_chunks() {
        let nals = collect(&[
            &[0, 0, 0, 1, 0x67, 0x11, 0],
            &[0],
            &[0, 1, 0x68],
            &[0, 0, 1],
        ]);
        assert_eq!(
            nals,
            vec![vec![0, 0, 0, 1, 0x67, 0x11], vec![0, 0, 0, 1, 0x68]]
        );
    }

    #[test]
    fn keeps_last_nal_until_next_start_code() {
        let mut s = Splitter::new();
        s.push(&[0, 0, 1, 0x65, 1, 2, 3]);
        assert!(s.next_nal().is_none());
        assert_eq!(s.pending(), 7);
    }
}
