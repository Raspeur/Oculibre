//! Clock synchronisation from `PING`/`PONG` (§11).

use std::collections::VecDeque;

/// One `PING`/`PONG` exchange, all times in µs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    /// Responder clock minus sender clock.
    pub offset_us: i64,
    /// Round trip, excluding the responder's processing time.
    pub rtt_us: i64,
}

/// `t1`: PING sent (sender clock), `t2`: PING received and `t3`: PONG sent (responder clock),
/// `t4`: PONG received (sender clock).
pub fn sample(t1: u64, t2: u64, t3: u64, t4: u64) -> Sample {
    let (t1, t2, t3, t4) = (t1 as i64, t2 as i64, t3 as i64, t4 as i64);
    Sample {
        offset_us: ((t2 - t1) + (t3 - t4)) / 2,
        rtt_us: (t4 - t1) - (t3 - t2),
    }
}

/// Keeps the last `window` samples and trusts the one with the lowest round trip:
/// the less time in flight, the less error on the offset.
#[derive(Debug, Clone)]
pub struct ClockSync {
    window: usize,
    samples: VecDeque<Sample>,
}

impl ClockSync {
    pub fn new(window: usize) -> Self {
        Self {
            window: window.max(1),
            samples: VecDeque::new(),
        }
    }

    pub fn add(&mut self, s: Sample) {
        if s.rtt_us < 0 {
            return; // clocks went backwards or bogus peer: ignore
        }
        if self.samples.len() == self.window {
            self.samples.pop_front();
        }
        self.samples.push_back(s);
    }

    pub fn best(&self) -> Option<Sample> {
        self.samples.iter().copied().min_by_key(|s| s.rtt_us)
    }
}

impl Default for ClockSync {
    fn default() -> Self {
        Self::new(16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetric_path() {
        // Responder clock is 1000 µs ahead, 10 µs each way, 5 µs processing.
        let s = sample(100, 1110, 1115, 125);
        assert_eq!(
            s,
            Sample {
                offset_us: 1000,
                rtt_us: 20
            }
        );
    }

    #[test]
    fn keeps_lowest_rtt() {
        let mut c = ClockSync::new(3);
        c.add(Sample {
            offset_us: 5,
            rtt_us: 50,
        });
        c.add(Sample {
            offset_us: 7,
            rtt_us: 10,
        });
        c.add(Sample {
            offset_us: 9,
            rtt_us: 40,
        });
        assert_eq!(c.best().unwrap().offset_us, 7);
        c.add(Sample {
            offset_us: 1,
            rtt_us: 30,
        });
        c.add(Sample {
            offset_us: 2,
            rtt_us: 35,
        }); // the rtt=10 sample left the window
        assert_eq!(c.best().unwrap().offset_us, 1);
    }
}
