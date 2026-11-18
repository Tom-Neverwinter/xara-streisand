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
use xara_arq::textcomp;
use xara_phy::call::call_hash;
use xara_phy::fm::ctrl::code::{BREAK, GO_AHEAD, QRT};
use xara_phy::fm::ctrl::{Codebook, Side};
use xara_phy::fm::frame::{Kind, Record0, parse_data};
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
    Data,
    Note,
}

impl Kinds {
    pub fn tag(self) -> &'static str {
        match self {
            Kinds::Connect => "connect",
            Kinds::Cq => "cq",
            Kinds::Broadcast => "broadcast",
            Kinds::Data => "data",
            Kinds::Note => "note",
        }
    }
}

/// A session being followed after its connect was heard. VARA FM data frames
/// all use one key (the answerer's call hash), so with that key the receiver
/// decodes both directions; which party is talking is tracked from the turn
/// handover control codes (`GO_AHEAD` yields, `BREAK` seizes), seeded by the
/// fact that the caller talks first.
struct Follow {
    caller: String,
    answerer: String,
    vias: bool,
    /// Stream time (seconds) the session's connect was heard.
    start: f64,
    /// 0 = caller is talking, 1 = answerer.
    talker: usize,
    /// The link's compression, from the connect: 1 = text, 2 = files, else off.
    comp: u8,
    /// Last payload chunk appended per side, to drop ARQ retransmissions.
    last: [Vec<u8>; 2],
}

impl Follow {
    fn call(&self, side: usize) -> &str {
        if side == 0 { &self.caller } else { &self.answerer }
    }
}

/// Every record a followed link can carry, so the receiver assembles them.
fn link_records(bw: FmBandwidth) -> Vec<Record> {
    let mut v = vec![Record::Connect, Record::Broadcast, Record::Short, Record::Start];
    v.extend((1..=bw.num_levels()).filter_map(Record::from_index));
    v
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
    /// `--dump`: also print each connect-type record in full (raw bytes and fields).
    pub dump: bool,
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
    follow: Option<Follow>,
    dump: bool,
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
            follow: None,
            dump: o.dump,
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

    /// The web UI's `decoder` object for FM, in the same shape the monitor page
    /// reads for HF: a heard list and a stats block. FM has no blind key search
    /// or per-level control tally, so `ctrl`/`ctrl_blind`/`searches` are 0 and
    /// the link/keys panels stay empty; the event log and heard list carry the
    /// FM session (issue #2).
    pub fn decoder_json(&self) -> String {
        use xara_ui::json::Obj;
        let now = self.heard.values().map(|h| h.last).fold(0.0_f64, f64::max);
        let mut items: Vec<(&String, &Heard)> = self.heard.iter().collect();
        items.sort_by(|a, b| b.1.last.total_cmp(&a.1.last));
        let heard: Vec<String> = items
            .iter()
            .map(|(name, h)| {
                let kinds: Vec<String> = h.roles.iter().map(|r| (*r).to_string()).collect();
                Obj::new()
                    .str("name", name)
                    .strs("kinds", &kinds)
                    .num2("first", h.last)
                    .num2("last", h.last)
                    .int("count", h.count as u64)
                    .end()
            })
            .collect();
        let s = &self.stats;
        let stats = Obj::new()
            .int("frames_ok", s.frames_ok as u64)
            .int("frames", s.frames as u64)
            .int("ctrl", 0)
            .int("ctrl_blind", 0)
            .int("searches", 0)
            .int("transmissions", s.transmissions as u64)
            .int("connects", s.connects as u64)
            .int("cq", s.cq as u64);
        let link = match &self.follow {
            Some(f) => Obj::new()
                .str("caller", &f.caller)
                .str("answerer", &f.answerer)
                .num2("start", f.start)
                .nums2("bytes", &[0.0, 0.0])
                .end(),
            None => "null".to_string(),
        };
        Obj::new()
            .num2("decoded", now)
            .raw("link", &link)
            .raw("keys", "[]")
            .raw("heard", &format!("[{}]", heard.join(",")))
            .obj("stats", stats)
            .end()
    }

    fn on(&mut self, ev: Event) {
        match ev {
            Event::Tx(t) => self.on_tx(t),
            // Control frames are only decodable once we are following a link
            // (its codebook is installed); they carry the turn handover.
            Event::Ctrl(c) => self.on_ctrl(secs(c.det.t_end), c.det.code),
            // Per-frame events are rolled up into the Tx event.
            Event::Frame(_) => {}
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
            // Any other record is session data; readable only while following
            // a link (otherwise the wrong key makes it undecodable anyway).
            _ => self.on_data(at, &t),
        }
    }

    // Turn handover. This mirrors the protocol's own turn semantics, so it is
    // not just a guess: the ACK codes (ACK_STAY, ACK_UP*, ACK_REQ*, ...) keep
    // the turn with the current talker, GO_AHEAD hands it to the other party,
    // and BREAK is the listener seizing it; either of the latter two flips the
    // talker. QRT ends the session. The one thing that can desync attribution
    // is a GO_AHEAD/BREAK lost to noise (data frames carry no sender id, so a
    // missed handover cannot be recovered passively); a fresh connect resyncs.
    fn on_ctrl(&mut self, at: f64, code: u32) {
        let Some(f) = &mut self.follow else { return };
        match code {
            GO_AHEAD | BREAK => f.talker ^= 1,
            QRT => {
                let (c, a) = (f.caller.clone(), f.answerer.clone());
                self.follow = None;
                self.emit(at, Kinds::Note, format!("link ended {} <-> {}", disp(&c), disp(&a)));
            }
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
        if self.dump {
            let hex: String = frame.bytes.iter().map(|b| format!("{b:02x}")).collect();
            self.emit(at, Kinds::Note, format!("record {hex} {r:?}"));
        }
        let verb = match r.kind {
            Kind::Connect => "connect",
            Kind::Reply => "reply",
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
        // Only an actual connect sets up a session to follow (not a ping etc.),
        // and only when both calls are known.
        if r.kind == Kind::Connect && !r.source.is_empty() && !r.target.is_empty() {
            self.start_follow(at, &r.source, &r.target, r.vias() > 0, r.compression);
        }
    }

    // Re-key the receiver to a link's data key and install its control
    // codebook, so its data frames and turn handovers decode. The keyless
    // Connect/Broadcast records still decode too, so a later connect simply
    // re-points the follower.
    fn start_follow(&mut self, at: f64, caller: &str, answerer: &str, vias: bool, comp: u8) {
        let (c, a) = (caller.to_ascii_uppercase(), answerer.to_ascii_uppercase());
        // Both parties key data with the answerer's call hash.
        self.rx.set_key(call_hash(a.as_bytes()));
        self.rx.expect(&link_records(self.bw), Some(1));
        // Codebook::for_link(upstream, downstream) == new(hash(down), hash(up)).
        let cb = Codebook::for_link(c.as_bytes(), a.as_bytes());
        let codes: Vec<u32> = (1..=24).collect();
        self.rx.set_ctrl(Side::Source, Some(cb), &codes);
        self.follow =
            Some(Follow { caller: c, answerer: a, vias, start: at, talker: 0, comp, last: [Vec::new(), Vec::new()] });
    }

    fn on_data(&mut self, at: f64, t: &Transmission) {
        // Borrow the two fields disjointly: the follow state and the output
        // queue (going through self.emit would reborrow all of self).
        let (Some(f), out) = (self.follow.as_mut(), &mut self.out) else { return };
        for (i, frame) in t.frames.iter().enumerate() {
            if !frame.ok {
                continue;
            }
            let Some(df) = parse_data(t.record, &frame.bytes, f.vias) else { continue };
            if df.data.is_empty() || df.data == f.last[f.talker] {
                continue; // empty fill, or an ARQ retransmission of the last chunk
            }
            f.last[f.talker] = df.data.clone();
            let body = decode_payload(f.comp, df.control.compressed, i, &df.data);
            out.push_back(Line { t: at, kind: Kinds::Data, text: format!("{}> {}", disp(f.call(f.talker)), body) });
        }
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

/// Turn a data frame's payload into display text, mirroring the receiver's
/// `inflate`: a text-compressed first frame is decompressed; a files link is
/// shown as a byte count (binary transfer, not readable); everything else is
/// shown raw. `comp` is the link's compression (1 = text, 2 = files, else off),
/// `frame_ix` the frame's position in the transmission (frame 1 is never
/// decompressed, matching the protocol).
fn decode_payload(comp: u8, compressed: bool, frame_ix: usize, data: &[u8]) -> String {
    const TEXT: u8 = 1;
    const FILES: u8 = 2;
    if comp == FILES {
        format!("<{} file bytes>", data.len())
    } else if comp == TEXT && compressed && frame_ix != 1 {
        textcomp::decompress(data).map_or_else(|| as_text(data), |(d, _)| as_text(&d))
    } else {
        as_text(data)
    }
}

/// Render payload bytes for display: keep printable ASCII and common
/// whitespace, replace the rest with a dot (as the HF monitor's payload pane
/// does).
fn as_text(b: &[u8]) -> String {
    b.iter().map(|&c| if c == b'\n' || c == b'\t' || (0x20..=0x7e).contains(&c) { c as char } else { '·' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xara_phy::fm::ctrl::{Codebook as TxCodebook, frame_audio};
    use xara_phy::fm::frame::{Trailer, build_data};
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

        let mut mon = FmMonitor::new(bw, &Opts { known: vec!["N1CALL".into()], dump: false });
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
    fn follows_a_session_and_attributes_payloads() {
        let bw = FmBandwidth::Narrow;
        let m = Modulator::new(bw);
        let mut rand = rng();
        let opt = TxOptions { drive_db: -5.0, pre_signal: false };
        // Data key is the answerer's (N1) hash; control uses for_link(caller, answerer).
        let key = call_hash(b"N1CALL");
        let cb = TxCodebook::for_link(b"N0CALL", b"N1CALL");
        let rec = Record::from_index(2).unwrap();
        let flen = bw.level(rec).frame_bytes();
        let data = |msg: &[u8]| {
            build_data(rec, flen, msg, false, 3, false, Trailer { lead_report: 1, level: 0, chain_snr: None })
        };

        let mut x = vec![0.0; 24000];
        let r0 = Record0::connect("N0CALL", "N1CALL", "", "", false, false).to_bytes().unwrap();
        x.extend(m.transmission(Record::Connect, 0, &[&r0[..]], 21, opt, &mut rand));
        x.extend(std::iter::repeat_n(0.0, 24000));
        // Caller N0 talks first.
        let d0 = data(b"HELLO FROM N0");
        x.extend(m.transmission(rec, key, &[&d0[..]], 2, opt, &mut rand));
        x.extend(std::iter::repeat_n(0.0, 12000));
        // Turn handover, then answerer N1 talks.
        x.extend(frame_audio(&cb, GO_AHEAD, 2, -5.0));
        x.extend(std::iter::repeat_n(0.0, 12000));
        let d1 = data(b"HI N0 THIS IS N1");
        x.extend(m.transmission(rec, key, &[&d1[..]], 2, opt, &mut rand));
        x.extend(std::iter::repeat_n(0.0, 12000));
        x.extend(frame_audio(&cb, QRT, 2, -5.0));
        x.extend(std::iter::repeat_n(0.0, 24000));

        let mut mon = FmMonitor::new(bw, &Opts::default());
        let lines = feed_all(&mut mon, &x);
        let data_lines: Vec<&str> = lines.iter().filter(|l| l.kind == Kinds::Data).map(|l| l.text.as_str()).collect();
        assert!(
            data_lines.iter().any(|t| t == &"N0CALL> HELLO FROM N0"),
            "caller payload missing/mis-attributed: {data_lines:?}"
        );
        assert!(
            data_lines.iter().any(|t| t == &"N1CALL> HI N0 THIS IS N1"),
            "answerer payload missing/mis-attributed: {data_lines:?}"
        );
        assert!(lines.iter().any(|l| l.kind == Kinds::Note && l.text.contains("link ended")), "no link-end note");
    }

    #[test]
    fn decompresses_a_text_compressed_payload() {
        let bw = FmBandwidth::Narrow;
        let m = Modulator::new(bw);
        let mut rand = rng();
        let opt = TxOptions { drive_db: -5.0, pre_signal: false };
        let key = call_hash(b"N1CALL");
        let rec = Record::from_index(2).unwrap();
        let flen = bw.level(rec).frame_bytes();

        let msg = b"CQ CQ de N0CALL, pleased to meet you on FM";
        let enc = textcomp::compress(msg, 200, textcomp::START_STATE);
        assert_eq!(enc.consumed, msg.len(), "whole message should fit the frame");

        let mut x = vec![0.0; 24000];
        // A registered connect advertising text compression (code 1).
        let mut r0v = Record0::connect("N0CALL", "N1CALL", "", "", true, false);
        r0v.compression = 1;
        let r0 = r0v.to_bytes().unwrap();
        x.extend(m.transmission(Record::Connect, 0, &[&r0[..]], 21, opt, &mut rand));
        x.extend(std::iter::repeat_n(0.0, 24000));
        // The caller's first frame, text-compressed.
        let d0 =
            build_data(rec, flen, &enc.bytes, true, 3, false, Trailer { lead_report: 1, level: 0, chain_snr: None });
        x.extend(m.transmission(rec, key, &[&d0[..]], 2, opt, &mut rand));
        x.extend(std::iter::repeat_n(0.0, 24000));

        let mut mon = FmMonitor::new(bw, &Opts::default());
        let lines = feed_all(&mut mon, &x);
        let want = format!("N0CALL> {}", std::str::from_utf8(msg).unwrap());
        assert!(
            lines.iter().any(|l| l.kind == Kinds::Data && l.text == want),
            "decompressed text not found: {:?}",
            lines.iter().filter(|l| l.kind == Kinds::Data).map(|l| &l.text).collect::<Vec<_>>()
        );
    }

    #[test]
    fn json_is_well_formed() {
        let l = Line { t: 1.5, kind: Kinds::Cq, text: "CQ de N2CALL".into() };
        let j = l.json(Some(1000.0));
        assert!(j.contains("\"t\":\"cq\""), "{j}");
        assert!(j.contains("\"ts\":1.5"), "{j}");
        assert!(j.contains("N2CALL"), "{j}");
    }

    #[test]
    fn decoder_json_shape_for_the_ui() {
        // Idle monitor: no session, empty heard, zeroed stats, valid JSON.
        let mon = FmMonitor::new(FmBandwidth::Narrow, &Opts::default());
        let j = mon.decoder_json();
        assert!(j.contains("\"link\":null"), "{j}");
        assert!(j.contains("\"keys\":[]"), "{j}");
        assert!(j.contains("\"heard\":[]"), "{j}");
        assert!(j.contains("\"frames\":0") && j.contains("\"searches\":0"), "{j}");
    }
}
