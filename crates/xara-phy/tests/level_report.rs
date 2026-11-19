//! Reports, not checks: run on demand to look at the level tables and the
//! decoder's own sensitivity.
//!
//!   cargo test --release -p xara-phy --test level_report -- --ignored --nocapture
//!
//! `level_tables` prints every HF level (modulation, FEC, rates, threshold) as
//! Markdown, for comparison with VARA's published "VARA HF v4.0 Levels" table.
//! `snr_sweep_500` modulates 500 Hz frames, adds white noise and decodes them,
//! printing the decode rate against SNR (3 kHz noise bandwidth) with the mean
//! SN the receiver reported for the frames it decoded. Those reported values
//! are in the same units xara-monitor prints for on-air frames.
mod common;

use common::{Rng, random_frame};
use xara_phy::hf::level::{Bandwidth, Record, Waveform};
use xara_phy::hf::rx::receiver::{Event, Receiver};
use xara_phy::hf::tx::Modulator;

/// Code rate for a `Level::rate` index (puncturing of the rate-1/3 turbo code; see `fec::puncture_period`).
fn code_rate(index: u32) -> &'static str {
    match index {
        2 => "1/3",
        3 => "1/2",
        4 => "2/3",
        5 => "3/4",
        6 => "4/5",
        7 => "5/6",
        _ => "?",
    }
}

#[test]
#[ignore = "report; run with --ignored --nocapture"]
fn level_tables() {
    for bw in Bandwidth::ALL {
        println!("## {} Hz ({} levels)\n", bw.hz(), bw.num_levels());
        println!(
            "| L | modulation | carriers | bits/sym | FEC | sym/s | frame B | airtime s | net bit/s | thr dB | extra |"
        );
        println!("|---|---|---|---|---|---|---|---|---|---|---|");
        for (i, l) in bw.table().iter().enumerate() {
            let (m, nc) = match l.waveform {
                Waveform::Fsk { .. } => (format!("{}-FSK", 1u32 << l.bps), l.nc().to_string()),
                Waveform::Psk { carriers_hz, .. } => (
                    match l.bps {
                        1 => "BPSK".to_string(),
                        2 => "QPSK".to_string(),
                        b => format!("{}-PSK", 1u32 << b),
                    },
                    format!("{} ({:?} Hz)", carriers_hz.len(), carriers_hz),
                ),
                Waveform::Ofdm { nc, .. } => (
                    match l.bps {
                        2 => "OFDM QPSK".to_string(),
                        3 => "OFDM 8PSK".to_string(),
                        b => format!("OFDM {}-QAM", 1u32 << b),
                    },
                    nc.to_string(),
                ),
            };
            println!(
                "| {} | {} | {} | {} | {} | {:.2} | {} | {:.2} | {:.0} | {:+.1} | {} |",
                i + 1,
                m,
                nc,
                l.bps,
                code_rate(l.rate),
                1.0 / l.symbol_period(),
                l.frame_bytes(),
                l.airtime(),
                l.net_bps(),
                l.snr_thr_db,
                if l.extra { "yes" } else { "" }
            );
        }
        println!();
    }
}

#[test]
#[ignore = "report; run with --ignored --nocapture (about 3 min)"]
fn snr_sweep_500() {
    const FS: f64 = 48000.0;
    const SIG_RMS: f64 = 0.12;
    const TRIALS: usize = 12;
    let bw = Bandwidth::Hz500;
    let m = Modulator::new(bw);
    let all: Vec<Record> = bw.levels().collect();
    println!("500 Hz, AWGN, SNR in 3 kHz; decoded/{TRIALS} (mean reported SN of decoded frames)");
    for lv in [1usize, 2, 3, 7] {
        let rec = Record::new(lv).unwrap();
        let mut line = format!("L{lv:<2} thr {:+5.1}:", bw.level(rec).snr_thr_db);
        for snr in (-20i64..=4).step_by(2) {
            let sigma = (SIG_RMS * SIG_RMS * (FS / 2.0 / 3000.0) / 10f64.powf(snr as f64 / 10.0)).sqrt();
            let mut rng = Rng::new(1000 + lv as u64 * 100 + (snr + 50) as u64);
            let (mut ok, mut est) = (0, Vec::new());
            for _ in 0..TRIALS {
                let frame = random_frame(&mut rng, bw.level(rec).frame_bytes() as usize);
                let t = m.transmission(rec, &[&frame], &mut |n| rng.below(n));
                let rms = (t.iter().map(|v| v * v).sum::<f64>() / t.len() as f64).sqrt();
                let mut x = vec![0.0; 24000];
                x.extend(t.iter().map(|v| v / (rms + 1e-12) * SIG_RMS));
                x.extend(vec![0.0; 24000]);
                let x: Vec<f64> = x.iter().map(|v| v + sigma * rng.gauss()).collect();
                // Listen on every level, as xara-monitor does.
                let mut rx = Receiver::new(bw, &all, 1, None);
                let mut ev: Vec<Event> = x.chunks(480).flat_map(|b| rx.feed(b)).collect();
                ev.extend(rx.flush());
                for e in &ev {
                    let Event::Frame(f) = e else { continue };
                    if f.frame.ok && f.frame.bytes == frame {
                        ok += 1;
                        est.push(f.frame.snr_db);
                    }
                }
            }
            let mean = if est.is_empty() { f64::NAN } else { est.iter().sum::<f64>() / est.len() as f64 };
            line += &format!(" {snr:+}:{ok:>2}({mean:+.0})");
        }
        println!("{line}");
    }
}
