//! A passive FM traffic monitor.
//!
//! The HF [`Monitor`](super::Monitor) can recover a link's key blind and then
//! follow the whole session. VARA FM has no blind key path, so this monitor
//! works the way an FM station bootstraps reception: it listens with key 0 for
//! the two *keyless* records every FM station emits in the clear —
//! [`Record::Connect`] (the start of every session, carrying both callsigns)
//! and [`Record::Broadcast`] (CQ and other beacons). From those it reports who
//! is calling whom, who is calling CQ, and keeps a heard list — without needing
//! any link's key.
//!
//! Following a session's *data* frames needs that link's key (derived from the
//! two calls); that, and the Unix web UI, are deliberate follow-ups — see
//! issue #2. This module changes nothing on the proven HF path.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use xara_arq::fm::broadcast::{self, Received};
use xara_phy::fm::frame::{Kind, Record0};
use xara_phy::fm::level::{FmBandwidth, Record};
use xara_phy::fm::rx::receiver::{Event, Receiver, Transmission};

/// The audio sample rate this monitor consumes (same as the FM modem).
pub const FS: f64 = xara_phy::fm::rx::FS;

/// The receiver's internal (decimated) sample rate, used to turn frame
/// timestamps into seconds.
const FS6: f64 = xara_phy::fm::rx::FS6;

/// What a reported line is about, for JSON output and filtering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kinds {
    Connect,
    Cq,
    Broadcast,
    Note,
}

impl Kinds {
    pub fn tag(self) -> &'static str {
        match self {
            Kinds::Connect => "connect",
            Kinds::Cq => "cq",
            Kinds::Broadcast => "broadcast",
            Kinds::Note => "note",
        }
    }
}

/// One line of monitor output.
#[derive(Clone, Debug)]
pub struct Line {
    /// Seconds from the start of the input stream.
    pub t: f64,
    pub kind: Kinds,
    pub text: String,
}

impl Line {
    /// A JSON object for `--json`, mirroring the shape of the HF monitor's
    /// records: a type tag, a time, an optional wall-clock unix time, and text.
    pub fn json(&self, unix: Option<f64>) -> String {
        let mut o = xara_ui::json::Obj::typed(self.kind.tag()).num2("ts", self.t);
        if let Some(u) = unix {
            o = o.num2("unix", u);
        }
        o.str("text", &self.text).end()
    }
}

/// A station heard on the air.
#[derive(Clone, Debug, Default)]
pub struct Heard {
    /// Seconds (stream time) the station was last heard.
    pub last: f64,
    /// How it was heard, e.g. "caller", "answerer", "CQ".
    pub roles: BTreeSet<&'static str>,
    /// Number of frames attributed to it.
    pub count: usize,
    /// Whether the call is in the configured known-call list.
    pub known: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub transmissions: usize,
    pub frames: usize,
    pub frames_ok: usize,
    pub connects: usize,
    pub cq: usize,
}

/// Options for [`FmMonitor`].
#[derive(Clone, Debug, Default)]
pub struct Opts {
    /// Calls to flag as known in the heard list (from `--call` / `--calls`).
    pub known: Vec<String>,
}

/// A passive FM monitor. Feed it FM-band audio at [`FS`]; read lines with
/// [`FmMonitor::pop`].
pub struct FmMonitor {
    pub bw: FmBandwidth,
    rx: Receiver,
    bcast: broadcast::Reassembler,
    heard: BTreeMap<String, Heard>,
    known: BTreeSet<String>,
    out: VecDeque<Line>,
    stats: Stats,
}

fn secs(t_end: i64) -> f64 {
    t_end as f64 / FS6
}

impl FmMonitor {
    pub fn new(bw: FmBandwidth, o: &Opts) -> Self {
        Self {
            bw,
            // Key 0 decodes the keyless Connect and Broadcast records; neither
            // depends on a link key (see `fm::code::record_key`).
            rx: Receiver::new(bw, 0, &[Record::Connect, Record::Broadcast], 1),
            bcast: broadcast::Reassembler::default(),
            heard: BTreeMap::new(),
            known: o.known.iter().map(|c| c.to_ascii_uppercase()).collect(),
            out: VecDeque::new(),
            stats: Stats::default(),
        }
    }

    pub fn feed(&mut self, x: &[f64]) {
        let evs = self.rx.feed(x);
        for ev in evs {
            self.on(ev);
        }
    }

    /// Flush any transmission still being assembled at end of input.
    pub fn finish(&mut self) {
        let evs = self.rx.flush();
        for ev in evs {
            self.on(ev);
        }
    }

    pub fn pop(&mut self) -> Option<Line> {
        self.out.pop_front()
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn heard(&self) -> &BTreeMap<String, Heard> {
        &self.heard
    }

    fn on(&mut self, ev: Event) {
        match ev {
            Event::Tx(t) => self.on_tx(t),
            // Per-frame events are rolled up into the Tx event; control
            // detections need a link key we do not have here.
            Event::Frame(_) | Event::Ctrl(_) => {}
        }
    }

    fn on_tx(&mut self, t: Transmission) {
        self.stats.transmissions += 1;
        self.stats.frames += t.frames.len();
        self.stats.frames_ok += t.frames.iter().filter(|f| f.ok).count();
        let at = secs(t.t_end);
        match t.record {
            Record::Connect => self.on_connect(at, &t),
            Record::Broadcast => self.on_broadcast(at, &t),
            // We only expect Connect/Broadcast; anything else means a keyed
            // record we cannot read without the link key.
            _ => {}
        }
    }

    fn on_connect(&mut self, at: f64, t: &Transmission) {
        let Some(frame) = t.frames.iter().find(|f| f.ok) else {
            self.emit(at, Kinds::Note, "connect frame (CRC failed)".into());
            return;
        };
        let Some(r) = Record0::parse(&frame.bytes) else {
            self.emit(at, Kinds::Note, "connect frame (unparsable)".into());
            return;
        };
        self.stats.connects += 1;
        let verb = match r.kind {
            Kind::Connect => "connect",
            Kind::Reply => "connect reply",
            Kind::Ping => "ping",
            Kind::AutoTune => "autotune",
        };
        if !r.source.is_empty() {
            self.hear(&r.source, "caller", at);
        }
        if !r.target.is_empty() {
            self.hear(&r.target, "answerer", at);
        }
        let vias: Vec<&str> = [&r.via1, &r.via2].into_iter().map(String::as_str).filter(|s| !s.is_empty()).collect();
        for v in &vias {
            self.hear(v, "digi", at);
        }
        let via = if vias.is_empty() { String::new() } else { format!(" via {}", vias.join(",")) };
        let band = if r.wide { "wide" } else { "narrow" };
        let text = format!("{verb} {} -> {}{via} ({band})", disp(&r.source), disp(&r.target));
        self.emit(at, Kinds::Connect, text);
    }

    fn on_broadcast(&mut self, at: f64, t: &Transmission) {
        for frame in t.frames.iter().filter(|f| f.ok) {
            match self.bcast.push(&frame.bytes) {
                Some(Received::Cq(cq)) => {
                    self.stats.cq += 1;
                    self.hear(&cq.call, "CQ", at);
                    self.emit(at, Kinds::Cq, format!("CQ de {}", disp(&cq.call)));
                }
                Some(Received::Kiss(cmd, data)) => {
                    self.emit(at, Kinds::Broadcast, format!("broadcast {cmd:?} ({} bytes)", data.len()));
                }
                None => {}
            }
        }
    }

    fn hear(&mut self, call: &str, role: &'static str, at: f64) {
        let call = call.to_ascii_uppercase();
        let known = self.known.contains(&call);
        let h = self.heard.entry(call).or_default();
        h.last = at;
        h.count += 1;
        h.known = known;
        h.roles.insert(role);
    }

    fn emit(&mut self, t: f64, kind: Kinds, text: String) {
        self.out.push_back(Line { t, kind, text });
    }
}

fn disp(call: &str) -> &str {
    if call.is_empty() { "?" } else { call }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xara_phy::fm::tx::{Modulator, TxOptions};

    // A tiny deterministic PRNG for the modulator's lead-in randomisation; the
    // exact values do not matter for decoding.
    fn rng() -> impl FnMut(u32) -> u32 {
        let mut s: u32 = 0x1234_5678;
        move |n| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if n == 0 { 0 } else { s % n }
        }
    }

    fn feed_all(mon: &mut FmMonitor, x: &[f64]) -> Vec<Line> {
        for c in x.chunks(480) {
            mon.feed(c);
        }
        mon.finish();
        std::iter::from_fn(|| mon.pop()).collect()
    }

    #[test]
    fn hears_connect_and_cq_without_the_link_key() {
        let bw = FmBandwidth::Narrow;
        let m = Modulator::new(bw);
        let mut rand = rng();
        let opt = TxOptions { drive_db: -5.0, pre_signal: false };

        let mut x = vec![0.0; 12000];
        // A connect N0CALL -> N1CALL, key 0 (connect is keyless).
        let r0 = Record0::connect("N0CALL", "N1CALL", "", "", false, false).to_bytes().unwrap();
        let cr: Vec<&[u8]> = vec![&r0];
        x.extend(m.transmission(Record::Connect, 0, &cr, 21, opt, &mut rand));
        x.extend(std::iter::repeat_n(0.0, 12000));
        // A CQ from N2CALL (broadcast is keyless).
        let buf = broadcast::cq_buffer("N2CALL", None).unwrap();
        let frames = broadcast::frames(bw, &buf).unwrap();
        let refs: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();
        x.extend(m.transmission(Record::Broadcast, 0, &refs, 9, opt, &mut rand));
        x.extend(std::iter::repeat_n(0.0, 12000));

        let mut mon = FmMonitor::new(bw, &Opts { known: vec!["N1CALL".into()] });
        let lines = feed_all(&mut mon, &x);

        let connect = lines.iter().find(|l| l.kind == Kinds::Connect).expect("a connect line");
        assert!(connect.text.contains("N0CALL -> N1CALL"), "{:?}", connect.text);
        let cq = lines.iter().find(|l| l.kind == Kinds::Cq).expect("a CQ line");
        assert!(cq.text.contains("N2CALL"), "{:?}", cq.text);

        let heard = mon.heard();
        assert!(heard.contains_key("N0CALL"), "heard {:?}", heard.keys().collect::<Vec<_>>());
        assert!(heard.contains_key("N1CALL"));
        assert!(heard.contains_key("N2CALL"));
        assert!(heard["N1CALL"].known, "N1CALL should be flagged known");
        assert!(heard["N2CALL"].roles.contains("CQ"));
        assert_eq!(mon.stats().connects, 1);
        assert_eq!(mon.stats().cq, 1);
    }

    #[test]
    fn json_is_well_formed() {
        let l = Line { t: 1.5, kind: Kinds::Cq, text: "CQ de N2CALL".into() };
        let j = l.json(Some(1000.0));
        assert!(j.contains("\"t\":\"cq\""), "{j}");
        assert!(j.contains("\"ts\":1.5"), "{j}");
        assert!(j.contains("N2CALL"), "{j}");
    }
}
