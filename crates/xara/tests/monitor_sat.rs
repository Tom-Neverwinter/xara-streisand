// xara-monitor --hf sat: SAT data frames at every level are decoded and reported.
use xara::monitor::record::Record;
use xara::monitor::{Monitor, MonitorOpts};
use xara_phy::hf::level::Bandwidth;
use xara_phy::hf::tx::Modulator;

#[test]
fn monitor_decodes_every_sat_level() {
    let bw = Bandwidth::Sat;
    let m = Modulator::new(bw);
    let mut s = 99u32;
    let mut rand = |n: u32| {
        s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (s >> 8) % n
    };
    let mut x = vec![0.0; 48000];
    for rec in bw.levels() {
        let len = bw.level(rec).frame_bytes() as usize;
        let mut frame: Vec<u8> = (0..len - 2).map(|i| (i * 31 + rec.get()) as u8).collect();
        frame.extend(xara_phy::crc::crc16_genibus(&frame).to_be_bytes());
        let tx = m.transmission(rec, &[&frame], &mut rand);
        let rms = (tx.iter().map(|v| v * v).sum::<f64>() / tx.len() as f64).sqrt();
        x.extend(tx.iter().map(|v| v / (rms + 1e-12) * 0.12));
        x.extend(vec![0.0; 3 * 48000]);
    }
    let mut mon = Monitor::new(bw, &MonitorOpts::default());
    for c in x.chunks(480) {
        mon.feed(c);
    }
    mon.finish();
    let mut seen: Vec<usize> = std::iter::from_fn(|| mon.pop())
        .filter_map(|l| match l.rec {
            Record::Data { level, frames, .. } if frames == "+" => Some(level),
            _ => None,
        })
        .collect();
    seen.sort_unstable();
    seen.dedup();
    let want: Vec<usize> = bw.levels().map(|r| r.get()).collect();
    assert_eq!(seen, want);
}
