//! Robustness: the over-the-air frame parsers take attacker-controlled bytes
//! (demodulated frames) and must fail gracefully, never panic. A decode panic
//! is a remote denial of service. These feed pseudo-random and
//! structured-random bytes at every parser entry point and only require that it
//! returns rather than panicking.

use xara_phy::fm::frame::{Record0, parse_data};
use xara_phy::fm::level::Record;

// A tiny deterministic xorshift PRNG — no external crates, reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

fn records() -> Vec<Record> {
    (0..64).filter_map(Record::from_index).collect()
}

#[test]
fn parse_data_never_panics() {
    let mut rng = Rng(0x1234_5678_9abc_def0);
    let recs = records();
    for _ in 0..40_000 {
        // lengths from 0 up past the smallest valid frame, to hit every branch
        let len = (rng.next() % 64) as usize;
        let frame = rng.bytes(len);
        for &rec in &recs {
            for vias in [false, true] {
                let _ = parse_data(rec, &frame, vias);
            }
        }
    }
}

#[test]
fn record0_parse_never_panics() {
    let mut rng = Rng(0x0fed_cba9_8765_4321);
    for _ in 0..200_000 {
        let len = (rng.next() % 96) as usize;
        let frame = rng.bytes(len);
        let _ = Record0::parse(&frame);
    }
}
