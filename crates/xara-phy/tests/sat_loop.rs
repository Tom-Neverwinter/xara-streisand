// Every VARA SAT level round-trips through XARA's modulator and receiver with SAT's tables.
mod common;

use common::{Rng, random_frame};
use xara_phy::hf::level::Bandwidth;
use xara_phy::hf::rx::receiver::{Event, Receiver};
use xara_phy::hf::tx::Modulator;

#[test]
fn every_sat_level_loops_back() {
    let bw = Bandwidth::Sat;
    let m = Modulator::new(bw);
    let mut rng = Rng::new(99);
    let mut fails = Vec::new();
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
            fails.push(rec);
        }
    }
    assert!(fails.is_empty(), "SAT levels failing loopback: {fails:?}");
    assert_eq!("sat".parse::<Bandwidth>(), Ok(Bandwidth::Sat));
    assert_eq!(Bandwidth::Sat.name(), "sat");
}
