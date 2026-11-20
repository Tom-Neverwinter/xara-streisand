// VARA HF 4.9.1 switches to a new table set on 2027-01-01 (Mod1.dat, Mod2.dat, Binary1-3.dat).
// Those tables are read from a VARA install, so these tests need XARA_VARA_DIR pointing at a
// folder holding VARA's .dat files; without it they pass vacuously and say so.
// The 2027 switch is process-wide, so the loopback lives in this separate test binary.
mod common;

use common::{Rng, random_frame};
use std::path::PathBuf;
use xara_phy::dat::{hf_tables, parse_hf_dat};
use xara_phy::hf::level::Bandwidth;
use xara_phy::hf::rx::receiver::{Event, Receiver};
use xara_phy::hf::tx::Modulator;

const FILES: [(Bandwidth, &str, &str); 3] = [
    (Bandwidth::Hz500, "Binary1.dat", "Binary4.dat"),
    (Bandwidth::Hz2300, "Binary2.dat", "Binary5.dat"),
    (Bandwidth::Hz2750, "Binary3.dat", "Binary6.dat"),
];

fn vara_dir() -> Option<PathBuf> {
    let d = std::env::var_os("XARA_VARA_DIR").map(PathBuf::from);
    if d.is_none() {
        eprintln!("XARA_VARA_DIR not set: skipping the VARA table-file checks");
    }
    d
}

#[test]
fn legacy_files_match_generated_tables() {
    let Some(dir) = vara_dir() else { return };
    for (bw, _, legacy) in FILES {
        let raw = std::fs::read(dir.join(legacy)).unwrap();
        assert_eq!(parse_hf_dat(bw, &raw, false).unwrap(), hf_tables(bw), "{legacy}");
    }
}

#[test]
fn files_2027_parse_and_differ() {
    let Some(dir) = vara_dir() else { return };
    for (bw, new, _) in FILES {
        let t = parse_hf_dat(bw, &std::fs::read(dir.join(new)).unwrap(), true).unwrap();
        let old = hf_tables(bw);
        assert!(t.iter().zip(&old).all(|(a, b)| a.chan != b.chan && a.turbo != b.turbo), "{new}");
    }
}

fn wav(path: &std::path::Path, x: &[f64]) {
    let mut b = Vec::with_capacity(44 + 2 * x.len());
    let n = (2 * x.len()) as u32;
    b.extend(b"RIFF");
    b.extend((36 + n).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(
        [16u32.to_le_bytes(), 0x0001_0001u32.to_le_bytes(), 48000u32.to_le_bytes(), 96000u32.to_le_bytes()].concat(),
    );
    b.extend([2u8, 0, 16, 0]);
    b.extend(b"data");
    b.extend(n.to_le_bytes());
    b.extend(x.iter().flat_map(|v| ((v * 32767.0).clamp(-32768.0, 32767.0) as i16).to_le_bytes()));
    std::fs::write(path, b).unwrap();
}

/// Every level of every bandwidth round-trips through XARA's own modulator and receiver with
/// the 2027 set.  With XARA_2027_WAV_DIR set it also writes the audio plus a decode-style log
/// (frame end times) per bandwidth, for checking against an independent implementation.
#[test]
fn loopback_every_level_2027() {
    let Some(dir) = vara_dir() else { return };
    xara_phy::hf::use_2027_tables(&dir).unwrap();
    assert!(xara_phy::hf::uses_2027_tables());
    let out = std::env::var_os("XARA_2027_WAV_DIR").map(PathBuf::from);
    let mut rng = Rng::new(2027);
    let mut fails = Vec::new();
    for bw in Bandwidth::ALL {
        let m = Modulator::new(bw);
        let (mut all, mut log) = (Vec::new(), String::new());
        for rec in bw.levels() {
            let frame = random_frame(&mut rng, bw.level(rec).frame_bytes() as usize);
            let tx = m.transmission(rec, &[&frame], &mut |n| rng.below(n));
            let rms = (tx.iter().map(|v| v * v).sum::<f64>() / tx.len() as f64).sqrt();
            let mut x = vec![0.0; 24000];
            x.extend(tx.iter().map(|v| v / (rms + 1e-12) * 0.12));
            x.extend(vec![0.0; 24000]);
            let mut x: Vec<f64> = x.iter().map(|v| v + 0.002 * rng.gauss()).collect();
            let mut rx = Receiver::new(bw, &[rec], 1, None);
            let mut ev: Vec<Event> = x.chunks(480).flat_map(|b| rx.feed(b)).collect();
            ev.extend(rx.flush());
            let got: Vec<&Vec<u8>> = ev
                .iter()
                .filter_map(|e| match e {
                    Event::Frame(f) if f.frame.ok => Some(&f.frame.bytes),
                    _ => None,
                })
                .collect();
            if got != [&frame] {
                fails.push((bw, rec));
            }
            let end = (all.len() + x.len() - 24000) as f64 / 48000.0;
            log += &format!("{end:9.3} DATA     L{rec}  x1 [+] loopback\n");
            all.append(&mut x);
        }
        if let Some(o) = &out {
            wav(&o.join(format!("loop2027-{}.wav", bw.hz())), &all);
            std::fs::write(o.join(format!("loop2027-{}.txt", bw.hz())), log).unwrap();
        }
    }
    assert!(fails.is_empty(), "{fails:?}");
}
