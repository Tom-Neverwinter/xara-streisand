//! A VARA SAT 4.4.5 data frame as recorded in the lab (2026-11-18): the tone heard in each 512-sample symbol
//! (93.75 Hz bins), lead-in included. SAT sends HF 2750 level-4-style FSK with its own tables (seed 99) and its
//! own whitening (seed 414); hard-decision demapping of this recording must decode with a good CRC.
use xara_phy::decode::FrameDecoder;
use xara_phy::demap::energy_llrs;
use xara_phy::hf::fsk::GRAY;
use xara_phy::hf::level::{Bandwidth, FSK_PILOTS, Waveform};

const TONES: &str = "17 9 9 19 16 14 21 6 17 6 13 25 20 11 25 25 25 21 7 22 18 10 9 19 21 19 18 18 24 7 14 9 22 24 17 14 12 6 20 15 11 12 19 20 17 17 13 20 19 23 23 15 9 7 13 11 12 20 8 13 16 8 17 17 10 9 25 10 16 6 22 12 18 9 9 19 18 15 10 13 17 10 23 18 6 11 8 10 21 23 24 19 20 19 17 21 10 7 25 11 13 9 6 14 8 11 19 24 17 11 20 13 18 19 15 21 24 18 18 24 23 25 15 17 13 23 6 12 23 12 16 24 23 22 21 13 14 7 24 22 14 11 7 23 13 8 17 16 9 25 23 11 7 20 19 7 23 19 25 17 11 23 12 11 9 20 12 7 11 15 14 24 17 16 11 12 25 23 15 12 19 15 21 25 22 21 10 10 17 16 6 25 24 8 7 16 24 24 20 24 21 8 17 18 21 12 12 10 6 15 23 15 22 14 9 16 15 11 15 18 18 6 19 9 8 17 17 16 16 6 6 13 9 16 15 20 15 24 13 12 11 14 11 25 14 6 17 17 8 15 8 20 9 18 21 18 7 23 9 16 10 23 9 19 21 6 20 20 19 16 7 23 8 11 15 12 22 15 16 25 16 24 11 13 9 19 20 18 6 25 10 16 21 13 17 11 13 22 25 24 17 15 8 7 22 17 11 22 13 24 15 23 21 24 23 9 11 20 7 20 21 23 6 25 22 23 9 8 8 22 7 16 17 20 22 12 14 23 16 23 22 18 18 18 12 20 12 9 18 15 8 8 21 9 13 25 18 14 12 11 23 22 18 22 15 12 19 23 11 9 19 16 13 23 14 10 13 18 7 20 7 8 13 15 21 19 11 9 25 16 18 18 17 23 24 16 9 6 12 20 24 25 11 14 19 11 20 13 19 8 7 21 13 11";
const LEAD_IN: usize = 19;

#[test]
fn recorded_sat_fsk_frame_decodes() {
    let tones: Vec<i64> = TONES.split_whitespace().map(|t| t.parse().unwrap()).collect();
    let b = Bandwidth::Sat;
    let rec = b.levels().find(|&r| matches!(b.level(r).waveform, Waveform::Fsk { symbols: 396, .. })).unwrap();
    let Waveform::Fsk { symbols, first, last, .. } = b.level(rec).waveform else { unreachable!() };
    let (n, nb) = (symbols as usize - 1, i64::from(last - first + 1));
    let o = &xara_phy::hf::tables(b)[rec.index()].other;
    let pilots = &o[n + FSK_PILOTS as usize..];
    let mut llr = Vec::new();
    for row in (0..n).filter(|&r| pilots[2 * r] == 0) {
        let tone =
            |v: usize| i64::from(first) + (i64::from(o[row]) - i64::from(first) + i64::from(GRAY[v])).rem_euclid(nb);
        let m: Vec<f64> = (0..16).map(|v| if tones[row + LEAD_IN] == tone(v) { 8.0 } else { 0.0 }).collect();
        energy_llrs(&m, 4, &mut llr);
    }
    let out = FrameDecoder::new(b).decode(rec, &llr);
    assert!(out.ok, "CRC");
    assert!(out.bytes.starts_with(b"HELLO FROM THE LAB "));
}
