// VARA FM 4.4.1 switches tables on 2027-01-01: Binary3/4.dat (legacy, = XARA's generated
// tables) become Binary1/2.dat, which store the records in reverse order.  These tests need
// XARA_VARA_FM_DIR pointing at a folder with VARA FM's .dat files; without it they pass
// vacuously and say so.  The 2027 switch is process-wide, so it lives in this test binary.
mod common;
use common::Rng;
use std::path::PathBuf;
use xara_phy::dat::{fm_tables, parse_fm_dat};
use xara_phy::fm::frame::{Trailer, build_data};
use xara_phy::fm::level::{FmBandwidth, Record};
use xara_phy::fm::rx::receiver::{Event, Receiver};
use xara_phy::fm::tx::{Modulator, TxOptions};

const FILES: [(FmBandwidth, &str, &str); 2] =
    [(FmBandwidth::Narrow, "Binary1.dat", "Binary3.dat"), (FmBandwidth::Wide, "Binary2.dat", "Binary4.dat")];

fn dir() -> Option<PathBuf> {
    let d = std::env::var_os("XARA_VARA_FM_DIR").map(PathBuf::from);
    if d.is_none() {
        eprintln!("XARA_VARA_FM_DIR not set: skipping the VARA FM table-file checks");
    }
    d
}

#[test]
fn legacy_files_match_generated_tables() {
    let Some(d) = dir() else { return };
    for (bw, _, legacy) in FILES {
        assert_eq!(
            parse_fm_dat(bw, &std::fs::read(d.join(legacy)).unwrap(), false).unwrap(),
            fm_tables(bw),
            "{legacy}"
        );
    }
}

#[test]
fn files_2027_parse_and_differ() {
    let Some(d) = dir() else { return };
    for (bw, new, _) in FILES {
        let t = parse_fm_dat(bw, &std::fs::read(d.join(new)).unwrap(), true).unwrap();
        assert!(t.iter().zip(fm_tables(bw)).all(|(a, b)| a.chan != b.chan), "{new}");
    }
}

#[test]
fn loopback_every_record_2027() {
    let Some(d) = dir() else { return };
    xara_phy::fm::code::use_2027_tables(&d).unwrap();
    let mut rng = Rng::new(2027);
    let mut fails = Vec::new();
    for bw in [FmBandwidth::Narrow, FmBandwidth::Wide] {
        let m = Modulator::new(bw);
        let key = 0x1234;
        for rec in bw.levels().map(Record::from).chain([Record::Short, Record::Start]) {
            let len = bw.level(rec).frame_bytes();
            let data = rng.bytes(len.min(200) / 2);
            let frame =
                build_data(rec, len, &data, false, 3, false, Trailer { lead_report: 1, level: 0, chain_snr: None });
            let opt = TxOptions { drive_db: -5.0, pre_signal: false };
            let mut x = vec![0.0; 12000];
            x.extend(m.transmission(rec, key, &[&frame], 25, opt, &mut |n| rng.below(n)));
            x.extend(vec![0.0; 24000]);
            let mut rx = Receiver::new(bw, key, &[rec], 1);
            let mut ev: Vec<Event> = x.chunks(480).flat_map(|b| rx.feed(b)).collect();
            ev.extend(rx.flush());
            if !ev.iter().any(|e| matches!(e, Event::Frame(f) if f.ok && f.bytes == frame)) {
                fails.push((bw, rec.index()));
            }
        }
    }
    assert!(fails.is_empty(), "{fails:?}");
}
