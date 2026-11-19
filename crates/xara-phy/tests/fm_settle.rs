//! A handheld that has just stopped transmitting hears the start of the next frame through a receiver that is
//! still settling: its audio starts quiet and rises (AGC and squelch opening) under a decaying low-frequency
//! thump (the audio coupling recovering). VARA FM 4.0 lists equalisation for such radios (gap G8); these tests
//! hold xara's FM receiver to decoding through it.
mod common;
use common::*;
use xara_phy::call::call_hash;
use xara_phy::fm::frame::{Trailer, build_data};
use xara_phy::fm::level::{FmBandwidth, Record};
use xara_phy::fm::rx::receiver::{Event, Receiver};
use xara_phy::fm::tx::{Modulator, TxOptions};

const FS: f64 = 48_000.0;

/// A slow receiver: the gain starts at `start_db` and rises with time constant `tau_s`; a thump of `thump`
/// (relative to the audio's RMS) decays with time constant `tau_s / 2`.
#[derive(Clone, Copy, Debug)]
struct Settle {
    start_db: f64,
    tau_s: f64,
    thump: f64,
}

fn settle(x: &mut [f64], s: Settle) {
    let rms = (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
    let g0 = 10f64.powf(s.start_db / 20.0);
    for (n, v) in x.iter_mut().enumerate() {
        let t = n as f64 / FS;
        let g = 1.0 - (1.0 - g0) * (-t / s.tau_s).exp();
        *v = *v * g + s.thump * rms * (-t / (s.tau_s / 2.0)).exp();
    }
}

fn lost(bw: FmBandwidth, s: Option<Settle>, lead: usize, sigma: f64) -> Vec<usize> {
    lost_at(bw, &(1..=bw.num_levels()).collect::<Vec<_>>(), s, lead, sigma, 21)
}

/// Frames of `levels`, one transmission each, through `s` and noise `sigma`; returns the levels that did not decode.
fn lost_at(bw: FmBandwidth, levels: &[usize], s: Option<Settle>, lead: usize, sigma: f64, seed: u64) -> Vec<usize> {
    let m = Modulator::new(bw);
    let mut rng = Rng::new(seed);
    let key = call_hash(b"N1CALL");
    let mut x = vec![0.0; 12_000];
    let mut sent = Vec::new();
    let mut spans = Vec::new();
    for &r in levels {
        let rec = Record::from_index(r).unwrap();
        let len = bw.level(rec).frame_bytes();
        let data = rng.bytes(len - 10);
        let frame = build_data(rec, len, &data, false, 3, false, Trailer { lead_report: 5, level: 0, chain_snr: None });
        let mut rand = |n: u32| rng.below(n);
        let opt = TxOptions { drive_db: -5.0, pre_signal: false };
        let burst = m.transmission(rec, key, &[frame.as_slice()], lead, opt, &mut rand);
        spans.push((x.len(), x.len() + burst.len()));
        x.extend(burst);
        x.extend(std::iter::repeat_n(0.0, 9000));
        sent.push((r, frame));
    }
    let mut noise = Rng::new(seed * 7 + 5);
    for v in &mut x {
        *v += sigma * noise.gauss();
    }
    // The receiver settles on what it hears, signal and noise alike, from the moment the frame arrives.
    if let Some(s) = s {
        for (a, b) in spans {
            settle(&mut x[a..b], s);
        }
    }
    let mut rx = Receiver::new(bw, key, &levels.iter().map(|&r| Record::from_index(r).unwrap()).collect::<Vec<_>>(), 1);
    let mut ev: Vec<Event> = x.chunks(480).flat_map(|b| rx.feed(b)).collect();
    ev.extend(rx.flush());
    let got: Vec<Vec<u8>> =
        ev.iter().filter_map(|e| if let Event::Frame(f) = e { f.ok.then(|| f.bytes.clone()) } else { None }).collect();
    sent.into_iter().filter(|(_, f)| !got.contains(f)).map(|(r, _)| r).collect()
}

#[test]
#[ignore = "survey: cargo test -p xara-phy --test fm_settle -- --ignored --nocapture"]
fn survey() {
    for bw in [FmBandwidth::Narrow, FmBandwidth::Wide] {
        for sigma in [0.02, 0.03] {
            eprintln!("{bw:?} sigma {sigma} clean: lost {:?}", lost(bw, None, 2, sigma));
            for (tau_s, start_db, thump) in
                [(0.6, -20.0, 1.0), (0.6, -20.0, 0.0), (0.6, 0.0, 1.0), (1.2, -20.0, 0.0), (0.3, -30.0, 0.0)]
            {
                let s = Settle { start_db, tau_s, thump };
                eprintln!("{bw:?} sigma {sigma} {s:?}: lost {:?}", lost(bw, Some(s), 2, sigma));
            }
        }
    }
}

#[test]
fn clean_baseline_decodes_every_level() {
    for bw in [FmBandwidth::Narrow, FmBandwidth::Wide] {
        assert_eq!(lost(bw, None, 2, 0.002), Vec::<usize>::new(), "{bw:?}");
    }
}

/// The top wide levels, near their noise limit, behind a receiver that starts 20 dB down and settles over 0.6 s.
/// Weighting every symbol by one noise floor for the whole frame lost about one frame in three here.
#[test]
fn settling_receiver_costs_no_top_level() {
    let s = Settle { start_db: -20.0, tau_s: 0.6, thump: 1.0 };
    let (clean, settling): (usize, usize) = (1..=3)
        .map(|seed| {
            let n = |s| lost_at(FmBandwidth::Wide, &[12, 13], s, 2, 0.02, seed).len();
            (n(None), n(Some(s)))
        })
        .fold((0, 0), |(a, b), (c, d)| (a + c, b + d));
    assert_eq!(clean, 0);
    assert!(settling <= 1, "{settling} of 6 frames lost behind a settling receiver");
}
