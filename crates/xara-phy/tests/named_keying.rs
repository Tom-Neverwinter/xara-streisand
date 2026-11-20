//! A named HF keying (xara-keygen) loads like VARA's 2027 files and is a working waveform of its own: frames round-trip
//! at every level, and its control codes are the legacy generator at keys + its own offset.  Process-wide switch, so its
//! own test binary.
mod common;

use common::{Rng, random_frame};
use xara_phy::call::call_hash;
use xara_phy::dat::hf_tables;
use xara_phy::hf::ctrl::{Codebook, Fmt, Role};
use xara_phy::hf::level::Bandwidth;
use xara_phy::hf::rx::receiver::{Event, Receiver};
use xara_phy::hf::tx::Modulator;
use xara_phy::keying;

#[test]
fn named_keying_links_and_shifts_its_codes() {
    let dir = std::env::temp_dir().join(format!("xara-named-keying-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let g = keying::hf("NET-7");
    keying::write(&dir, "NET-7", &g).unwrap();
    let n1 = call_hash(b"N1CALL");
    let s = g.info.seed_offset;
    let want = Codebook::new(n1 + s, [n1 + s, s, s, s, s], Role::Answerer, Bandwidth::Hz2300);
    let legacy = hf_tables(Bandwidth::Hz2300);

    xara_phy::hf::use_2027_tables(&dir).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(xara_phy::hf::key_shift(), s);
    assert_ne!(xara_phy::hf::tables(Bandwidth::Hz2300)[0].chan, legacy[0].chan);
    let got = Codebook::new(n1, [n1, 0, 0, 0, 0], Role::Answerer, Bandwidth::Hz2300);
    for c in 1..=50 {
        assert_eq!(got.rx(Fmt::F0, c), want.rx(Fmt::F0, c), "code {c}");
    }

    let mut rng = Rng::new(7);
    let mut fails = Vec::new();
    for bw in Bandwidth::ALL {
        let m = Modulator::new(bw);
        for rec in bw.levels() {
            let frame = random_frame(&mut rng, bw.level(rec).frame_bytes() as usize);
            let tx = m.transmission(rec, &[&frame], &mut |n| rng.below(n));
            let rms = (tx.iter().map(|v| v * v).sum::<f64>() / tx.len() as f64).sqrt();
            let mut x = vec![0.0; 24000];
            x.extend(tx.iter().map(|v| v / (rms + 1e-12) * 0.12));
            x.extend(vec![0.0; 24000]);
            let x: Vec<f64> = x.iter().map(|v| v + 0.002 * rng.gauss()).collect();
            let mut rx = Receiver::new(bw, &[rec], 1, None);
            let mut ev: Vec<Event> = x.chunks(480).flat_map(|b| rx.feed(b)).collect();
            ev.extend(rx.flush());
            let ok = ev.iter().any(|e| matches!(e, Event::Frame(f) if f.frame.ok && f.frame.bytes == frame));
            if !ok {
                fails.push((bw, rec));
            }
        }
    }
    assert!(fails.is_empty(), "{fails:?}");
}
