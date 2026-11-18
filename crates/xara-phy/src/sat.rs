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

/// Finds `target`'s connect request in 48 kHz audio: the sample where its 31-symbol body starts, with the share of
/// grid energy (tones 8..56) that sat on the expected tone, averaged over the body. One tone of a clean request
/// scores near 1; noise and other calls score around 1/48. Scans in eighth-symbol steps.
pub fn find_request(x: &[f64], target: &str, min_score: f64) -> Option<(usize, f64)> {
    const STEP: usize = SYMBOL_SAMPLES / 8;
    let code = connect_request(target);
    let starts = (x.len().checked_sub(SYMBOL_SAMPLES)? / STEP) + 1;
    // Power in each grid tone of the symbol-long window at every step.
    let spectra: Vec<[f64; 48]> = (0..starts)
        .map(|n| {
            let w = &x[n * STEP..n * STEP + SYMBOL_SAMPLES];
            std::array::from_fn(|b| {
                let (s, c) = (sine(b as u32 + 8, SYMBOL_SAMPLES), cos_of(b as u32 + 8));
                let (re, im) = w.iter().enumerate().fold((0.0, 0.0), |(re, im), (i, v)| (re + v * c[i], im + v * s[i]));
                re * re + im * im
            })
        })
        .collect();
    let span = (REQUEST_SYMBOLS - 1) * 8;
    (0..starts.checked_sub(span)?)
        .map(|n| {
            let score = code
                .iter()
                .enumerate()
                .map(|(i, &k)| {
                    let p = &spectra[n + 8 * i];
                    p[k as usize - 8] / p.iter().sum::<f64>().max(1e-12)
                })
                .sum::<f64>()
                / REQUEST_SYMBOLS as f64;
            (n * STEP, score)
        })
        .filter(|&(_, s)| s >= min_score)
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

fn cos_of(k: u32) -> &'static [f64] {
    use std::sync::OnceLock;
    static T: [OnceLock<Vec<f64>>; 64] = [const { OnceLock::new() }; 64];
    T[k as usize].get_or_init(|| {
        (0..SYMBOL_SAMPLES)
            .map(|n| (2.0 * std::f64::consts::PI * f64::from(k) * n as f64 / SYMBOL_SAMPLES as f64).cos())
            .collect()
    })
}
