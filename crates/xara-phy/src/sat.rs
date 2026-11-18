//! VARA SAT's connect request, worked out from our own lab recordings of VARA SAT 4.4.5 (2026-11-18).
//!
//! It is single-tone MFSK: one tone per 1024-sample symbol at 48 kHz (21.33 ms) on a 46.875 Hz grid. A call is
//! a 31-symbol codeword picked only by the target call (the source call and its SSID do not change it; the
//! target's SSID does). The codeword is the HF codeword recipe with a single comb of three teeth: seed Rnd with
//! `call_hash(target) + 50`, then each symbol is tooth `rand_int(2)` and step `2 * rand_int(6)` from tone 11, plus
//! one on even positions. The first call starts with 28 symbols of tone 32 (1500 Hz, the HF tuner tone);
//! retries start with a 10-symbol preamble, the first ten `rand_int(34) + 16` of a fresh Rnd.
use crate::call::call_hash;
use crate::dsp::sine;
use crate::rnd::VbRnd;

pub const SYMBOL_SAMPLES: usize = 1024;
pub const REQUEST_SYMBOLS: usize = 31;
pub const TUNE_TONE: u32 = 32;
pub const TUNE_SYMBOLS: usize = 28;
pub const PREAMBLE_SYMBOLS: usize = 10;
const SEED_OFF: u32 = 50;

/// The codeword a station calling `target` sends (tone indices; Hz = index * 46.875).
pub fn connect_request(target: &str) -> Vec<u32> {
    let mut r = VbRnd::new();
    r.seed_rnd(call_hash(target.to_ascii_uppercase().as_bytes()) + SEED_OFF);
    (0..REQUEST_SYMBOLS)
        .map(|i| {
            let tooth = r.rand_int(2);
            11 + 14 * tooth + 2 * r.rand_int(6) + u32::from(i % 2 == 0)
        })
        .collect()
}

pub fn preamble() -> Vec<u32> {
    let mut r = VbRnd::new();
    (0..PREAMBLE_SYMBOLS).map(|_| r.rand_int(34) + 16).collect()
}

/// A whole call as tones: the tuner tone before the first call, the preamble before each retry.
pub fn request_tones(target: &str, first: bool) -> Vec<u32> {
    let head = if first { vec![TUNE_TONE; TUNE_SYMBOLS] } else { preamble() };
    head.into_iter().chain(connect_request(target)).collect()
}

/// Unit-amplitude audio at 48 kHz. Every tone fits a whole number of cycles in a symbol, so the phase runs on.
pub fn synth(tones: &[u32]) -> Vec<f64> {
    tones.iter().flat_map(|&k| sine(k, SYMBOL_SAMPLES)[..SYMBOL_SAMPLES].iter().copied()).collect()
}
