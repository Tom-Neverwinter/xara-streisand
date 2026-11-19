//! Robustness: the over-the-air decoders take attacker-controlled bytes and
//! must fail gracefully, never panic (a decode panic is a remote DoS). Feed
//! pseudo-random bytes at each entry point and require it returns.

use xara_arq::fm::broadcast::Reassembler;
use xara_arq::textcomp;

// Longest dictionary phrase (textcomp::MAX_PHRASE, which is crate-private).
const MAX_PHRASE: usize = 39;

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

#[test]
fn decompress_never_panics_and_output_is_bounded() {
    let mut rng = Rng(0xa5a5_5a5a_1234_9876);
    for _ in 0..200_000 {
        let len = (rng.next() % 300) as usize;
        let payload = rng.bytes(len);
        if let Some((out, state)) = textcomp::decompress(&payload) {
            // state must stay a valid table index (1..=8), and output amplification
            // is bounded by the bitstream length times the longest phrase.
            assert!((1..=8).contains(&state), "decompress returned invalid state {state}");
            let bound = payload.len().saturating_mul(8).saturating_mul(MAX_PHRASE) + 64;
            assert!(out.len() <= bound, "decompress output {} exceeds bound {bound}", out.len());
        }
    }
}

#[test]
fn broadcast_push_never_panics() {
    let mut rng = Rng(0x0102_0304_dead_c0de);
    let mut re = Reassembler::default();
    for _ in 0..200_000 {
        let len = (rng.next() % 300) as usize;
        let frame = rng.bytes(len);
        let _ = re.push(&frame);
    }
}
