mod common;

use common::Seen;
use common::air::{self, BLOCK};
use xara::monitor::{Monitor, MonitorOpts};
use xara::station::hf::Station;
use xara::station::{Modem, Options, Report};
use xara_arq::compression::Compression;
use xara_arq::hf::session::Config;
use xara_arq::host::HostEvent;
use xara_phy::call::call_hash;
use xara_phy::hf::ctrl::Role;
use xara_phy::hf::frame::{Control, build_frame, header_payload, term_marker};
use xara_phy::hf::level::{Bandwidth, Record};
use xara_phy::hf::tx::Modulator;

const CALLER: &str = "N0CALL";
const ANSWERER: &str = "N1CALL";

struct Air {
    link: air::Air<Station, 2>,
    mon: Monitor,
    lines: Vec<String>,
    json: Vec<String>,
    seen: Seen,
    /// When set, what each station put on the air, kept apart, for tests that edit and replay it.
    audio: Option<[Vec<f64>; 2]>,
}

impl Air {
    fn new(bw: Bandwidth, comp: Compression, noise: f64) -> Self {
        let cfg = |call: &str, seed| {
            let mut c = Config { seed, ..Config::new(bw, call) };
            (c.host.registered, c.host.compression) = (true, comp);
            c
        };
        let st = [Station::new(cfg(CALLER, 3), Options::default()), Station::new(cfg(ANSWERER, 5), Options::default())];
        Self {
            link: air::Air::pair(st, [2400; 2], noise, |r| !matches!(r, Report::Host(HostEvent::Connected { .. }))),
            mon: Monitor::new(bw, &MonitorOpts::default()),
            lines: Vec::new(),
            json: Vec::new(),
            seen: Seen::default(),
            audio: None,
        }
    }

    fn linked(&self) -> bool {
        self.link.events.iter().all(|e| e.iter().any(|(_, r)| matches!(r, Report::Host(HostEvent::Connected { .. }))))
    }

    fn step(&mut self) {
        self.link.step();
        let (w, rng, noise) = (&self.link.wire, &mut self.link.rng, self.link.noise);
        let tail = |i: usize| w[i].range(w[i].len() - BLOCK..);
        if let Some(a) = &mut self.audio {
            a[0].extend(tail(0));
            a[1].extend(tail(1));
        }
        let heard: Vec<f64> = tail(0).zip(tail(1)).map(|(a, b)| a + b + noise * rng.gauss()).collect();
        self.mon.feed(&heard);
        self.drain();
    }

    fn drain(&mut self) {
        while let Some(l) = self.mon.pop() {
            self.lines.push(format!("{:9.3} {}", l.t, l.text));
            self.json.push(l.rec.json(l.t, None, &l.text));
            self.seen.add(&l.rec);
        }
    }

    fn run_until(&mut self, secs: f64, mut done: impl FnMut(&Self) -> bool) -> bool {
        let limit = self.link.secs() + secs;
        while self.link.secs() < limit {
            if done(self) {
                return true;
            }
            self.step();
        }
        done(self)
    }

    fn log(&self) -> String {
        self.lines.join("\n")
    }
}

fn text(n: usize, who: &str) -> Vec<u8> {
    let line =
        format!("Subject: net report\r\nThe weather station at {who} reports clear skies; check-in tonight 73\r\n");
    line.bytes().cycle().take(n).collect()
}

fn session(bw: Bandwidth, comp: Compression, noise: f64, pa: Vec<u8>, pb: Vec<u8>) -> Air {
    run_session(Air::new(bw, comp, noise), &pa, &pb, 15.0)
}

fn run_session(mut air: Air, pa: &[u8], pb: &[u8], tail: f64) -> Air {
    air.link.st[0].connect(CALLER, ANSWERER, &[]);
    assert!(air.run_until(40.0, |a| a.linked()), "no link:\n{}", air.log());
    air.link.st[0].send(pa);
    air.link.st[1].send(pb);
    let secs = 30.0 + (pa.len() + pb.len()) as f64 / 8.0;
    let done = air.run_until(secs, |a| a.link.got[1].len() >= pa.len() && a.link.got[0].len() >= pb.len());
    assert!(done, "transfer stalled:\n{}", air.log());
    air.link.st[0].disconnect();
    assert!(air.run_until(40.0, |a| a.link.st.iter().all(Station::idle)), "no disconnect:\n{}", air.log());
    air.run_until(tail, |_| false);
    air.mon.finish();
    air.drain();
    air
}

fn top_level_keys(j: &str) -> usize {
    let (mut depth, mut in_str, mut esc, mut keys) = (0, false, false, 0);
    for c in j.chars() {
        match (in_str, esc, c) {
            (true, true, _) => esc = false,
            (true, false, '\\') => esc = true,
            (true, false, '"') => in_str = false,
            (true, ..) => {}
            (false, _, '"') => in_str = true,
            (false, _, '{' | '[') => depth += 1,
            (false, _, '}' | ']') => depth -= 1,
            (false, _, ':') if depth == 1 => keys += 1,
            _ => {}
        }
    }
    keys
}

fn check_json(air: &Air, pa: &[u8], pb: &[u8]) {
    let mut streams = [Vec::new(), Vec::new()];
    assert_eq!(air.json.len(), air.lines.len());
    for (j, line) in air.json.iter().zip(&air.lines) {
        let v: xara_testkit::json::Value = xara_testkit::json::from_str(j).unwrap_or_else(|e| panic!("{e}: {j}"));
        let o = v.as_object().unwrap();
        assert_eq!(o.len(), top_level_keys(j), "duplicate key in {j}");
        assert_eq!(o["text"].as_str(), Some(&line[10..]), "{j}");
        if o["t"] == "payload" && o["status"] == "data" {
            let i = match o["sender"].as_str() {
                Some("caller") => 0,
                Some("answerer") => 1,
                _ => continue,
            };
            streams[i].extend(o["data"].as_str().unwrap().chars().map(|c| u8::try_from(u32::from(c)).unwrap()));
        }
    }
    assert_eq!(streams, [pa, pb], "payload records do not reassemble the streams");
}

fn check(air: &Air, pa: &[u8], pb: &[u8]) {
    check_json(air, pa, pb);
    let log = air.log();
    let s = &air.seen;
    let answerer = format!("#{:06X}", call_hash(ANSWERER.as_bytes()));
    assert!(log.contains(&format!("learned answerer key {answerer}")), "{log}");
    assert_eq!(s.callers, [CALLER], "{log}");
    let has = |sender: Role, codes: &[u32]| s.has(Some(sender), codes);
    assert!(has(Role::Caller, &[26, 31, 36]), "no connect request:\n{log}");
    assert!(has(Role::Answerer, &[15, 16, 17, 18, 22, 23, 24, 25, 41, 42, 43, 44]), "no connect reply:\n{log}");
    assert!(has(Role::Caller, &[3, 46]) || has(Role::Answerer, &[3, 46]), "no QRT:\n{log}");
    assert_eq!(s.delivered[0], pa, "caller → answerer stream differs:\n{log}");
    assert_eq!(s.delivered[1], pb, "answerer → caller stream differs:\n{log}");
    assert!(s.delivered[2].is_empty(), "unattributed data:\n{log}");
}

#[test]
fn monitor_follows_unknown_text_session() {
    let (pa, pb) = (text(6000, CALLER), text(1500, ANSWERER));
    let air = session(Bandwidth::Hz2300, Compression::Text, 0.002, pa.clone(), pb.clone());
    check(&air, &pa, &pb);
    assert!(air.mon.stats.levels.keys().any(|&l| l > 4), "link never left FSK:\n{}", air.log());
}

#[test]
fn monitor_follows_unknown_files_session_500() {
    let (pa, pb) = (text(2500, CALLER), text(600, ANSWERER));
    let air = session(Bandwidth::Hz500, Compression::Files, 0.002, pa.clone(), pb.clone());
    check(&air, &pa, &pb);
}

#[test]
fn monitor_drops_idle_keys_after_the_link() {
    let (pa, pb) = (text(1500, CALLER), text(300, ANSWERER));
    let mut air = Air::new(Bandwidth::Hz2300, Compression::Text, 0.002);
    air.mon = Monitor::new(Bandwidth::Hz2300, &MonitorOpts { key_idle: 20.0, ..Default::default() });
    let air = run_session(air, &pa, &pb, 45.0);
    check(&air, &pa, &pb);
    let log = air.log();
    let ended = log.find("ended (QRT)").unwrap_or_else(|| panic!("no QRT link end:\n{log}"));
    for note in [
        format!("dropped answerer key #{:06X}", call_hash(ANSWERER.as_bytes())),
        format!("dropped caller key {CALLER}"),
    ] {
        let i = log.find(&note).unwrap_or_else(|| panic!("no \"{note}\":\n{log}"));
        assert!(i > ended, "{note} before the link ended:\n{log}");
    }
}

/// Feeds recorded audio to a fresh monitor and returns its lines and the number of control records.
fn replay(bw: Bandwidth, opts: &MonitorOpts, audio: &[f64]) -> (Vec<String>, usize) {
    let mut mon = Monitor::new(bw, opts);
    let (mut lines, mut ctrl) = (Vec::new(), 0);
    let mut take = |mon: &mut Monitor| {
        while let Some(l) = mon.pop() {
            ctrl += usize::from(matches!(l.rec, xara::monitor::record::Record::Ctrl { .. }));
            lines.push(format!("{:9.3} {}", l.t, l.text));
        }
    };
    for b in audio.chunks(BLOCK) {
        mon.feed(b);
        take(&mut mon);
    }
    mon.finish();
    take(&mut mon);
    (lines, ctrl)
}

fn kiss_count(lines: &[String]) -> usize {
    lines.iter().filter(|l| l.contains("KISS cmd")).count()
}

/// Real VARA leaves about 28 ms more between a broadcast's announcement and its data frame than
/// XARA's transmitter does (measured on air: 5.016 s end to end against 4.988 s). With that gap
/// the data frame no longer covers the announcement, and the blind detector used to read the
/// level 2 announcement as a connect request: its codeword seed (0xCD80A4) is the seed of a
/// 500 Hz connect to callsign hash 0xCD809D.
#[test]
fn monitor_does_not_read_kiss_broadcasts_as_connects() {
    let mut air = Air::new(Bandwidth::Hz500, Compression::Off, 0.002);
    air.audio = Some(Default::default());
    let sent = 4;
    for i in 0..sent {
        let text = format!("level two broadcast number {i:02}").into_bytes();
        assert!(air.link.st[0].kiss(xara_arq::kiss::Command::Generic, text));
    }
    assert!(air.run_until(90.0, |a| kiss_count(&a.lines) == sent), "{}", air.log());
    let ends: Vec<f64> = air
        .lines
        .iter()
        .filter(|l| l.contains("UNPROTO") && l.contains("c47"))
        .map(|l| l.split_whitespace().next().unwrap().parse().unwrap())
        .collect();
    assert_eq!(ends.len(), sent, "{}", air.log());
    let [mut audio, _] = air.audio.take().unwrap();
    for t in ends.iter().rev() {
        let at = ((t + 0.012) * 48000.0) as usize;
        audio.splice(at..at, std::iter::repeat_n(0.0, 1440));
    }
    audio.extend(std::iter::repeat_n(0.0, 10 * 48000));

    let (lines, ctrl) = replay(Bandwidth::Hz500, &MonitorOpts::default(), &audio);
    let log = lines.join("\n");
    assert_eq!(kiss_count(&lines), sent, "{log}");
    assert_eq!(ctrl, 0, "a broadcast was reported as session control:\n{log}");
    assert!(!log.contains("learned") && !log.contains("link "), "phantom key or link:\n{log}");
    assert!(!log.contains("answerer") && !log.contains("caller"), "broadcast data given a session role:\n{log}");
}

/// Runs a short 500 Hz session, then replays it with the caller's session header replaced by
/// `frame`, modulated at the same level in the same place. An empty frame leaves silence there.
fn session_with_header(frame: impl Fn(usize) -> Vec<u8>, opts: &MonitorOpts) -> String {
    let bw = Bandwidth::Hz500;
    let mut air = Air::new(bw, Compression::Text, 0.002);
    air.audio = Some(Default::default());
    let air = run_session(air, &text(60, CALLER), &text(300, ANSWERER), 5.0);
    let log = air.log();
    let at = air.lines.iter().position(|l| l.contains("HEADER")).unwrap_or_else(|| panic!("no header:\n{log}"));
    let data = air.lines[..at].iter().rfind(|l| l.contains(" DATA ")).unwrap();
    let mut words = data.split_whitespace();
    let t_end: f64 = words.next().unwrap().parse().unwrap();
    let level: usize = words.nth(1).unwrap()[1..].parse().unwrap();
    let rec = Record::new(level).unwrap();
    let l = bw.level(rec);

    let [mut caller, answerer] = air.audio.clone().unwrap();
    let mid = ((t_end - l.airtime() / 2.0) * 48000.0) as usize;
    assert!(caller[mid] != 0.0, "the caller is not transmitting where the header should be");
    // The transmission runs between the nearest 10 ms stretches of silence on either side.
    let quiet = |w: &[f64]| w.iter().all(|&x| x == 0.0);
    let start = (0..mid).rev().find(|&i| i >= 480 && quiet(&caller[i - 480..i])).unwrap();
    let end = (mid..caller.len() - 480).find(|&i| quiet(&caller[i..i + 480])).unwrap();
    let rms = |x: &[f64]| (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
    let level_rms = rms(&caller[start..end]);

    let mut seed = 7u32;
    let mut rand = |n: u32| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) % n.max(1)
    };
    let frame = frame(l.frame_bytes() as usize);
    caller[start..end].fill(0.0);
    if !frame.is_empty() {
        let tx = Modulator::new(bw).transmission(rec, &[&frame], &mut rand);
        let gain = level_rms / rms(&tx);
        assert!(tx.len().abs_diff(end - start) < 4800, "replacement {} samples, original {}", tx.len(), end - start);
        for (o, v) in caller[start..].iter_mut().zip(&tx) {
            *o = v * gain;
        }
    }
    let mix: Vec<f64> = caller.iter().zip(&answerer).map(|(a, b)| a + b).collect();
    replay(bw, opts, &mix).0.join("\n")
}

fn header_frame(len: usize, edit: impl Fn(&mut [u8; 8])) -> Vec<u8> {
    let mut p = header_payload(CALLER.as_bytes(), 0x80 | 1 << 1 | 1);
    edit(&mut p);
    let ctrl = Control { reply: 2, compressed: false, hint: 0 };
    build_frame(len, &p, ctrl, Some(6), term_marker(CALLER.as_bytes()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The fields a later VARA could stamp with a version or date are exactly the ones XARA's session
/// layer refuses. The monitor must show such a header, say what is unusual and keep following.
#[test]
fn monitor_shows_a_header_the_session_layer_would_refuse() {
    let log = session_with_header(|len| header_frame(len, |p| p[6] = 0x2a), &MonitorOpts::default());
    let header = log.lines().find(|l| l.contains("HEADER")).unwrap_or_else(|| panic!("no header line:\n{log}"));
    assert!(header.contains("HEADER N0CALL"), "{header}");
    assert!(header.contains("[UNUSUAL: byte 6 = 2a]"), "{header}");
    let raw = hex(&header_frame(46, |p| p[6] = 0x2a)[..8]);
    assert!(header.ends_with(&format!("raw {raw}")), "{header}");
    assert!(!log.contains("NO HEADER"), "{log}");
    assert!(log.contains("learned caller key N0CALL"), "the caller was not followed:\n{log}");
    assert!(log.lines().any(|l| l.contains("CALLER") && l.ends_with("N0CALL")), "no caller control frames:\n{log}");
}

#[test]
fn monitor_shows_raw_bytes_of_an_ordinary_header_with_diag() {
    let plain = session_with_header(|len| header_frame(len, |_| {}), &MonitorOpts::default());
    let header = plain.lines().find(|l| l.contains("HEADER")).unwrap_or_else(|| panic!("no header line:\n{plain}"));
    assert!(!header.contains("UNUSUAL") && !header.contains("raw"), "{header}");
    let opts = MonitorOpts { diag: true, ..MonitorOpts::default() };
    let diag = session_with_header(|len| header_frame(len, |_| {}), &opts);
    let header = diag.lines().find(|l| l.contains("HEADER")).unwrap();
    let raw = hex(&header_frame(46, |_| {})[..8]);
    assert!(header.ends_with(&format!("raw {raw}")) && !header.contains("UNUSUAL"), "{header}");
}

#[test]
fn monitor_says_when_the_frame_after_a_connect_reply_is_no_header() {
    let body = b"this is not a session header";
    let ctrl = Control { reply: 2, compressed: false, hint: 0 };
    let log = session_with_header(|len| build_frame(len, body, ctrl, Some(1), 0), &MonitorOpts::default());
    let line = log.lines().find(|l| l.contains("NO HEADER")).unwrap_or_else(|| panic!("not reported:\n{log}"));
    assert!(line.contains(&format!("raw {}", hex(body))), "{line}");
    assert!(!log.contains("HEADER N0CALL"), "{log}");
    assert_eq!(log.matches("NO HEADER").count(), 1, "only the frame in header position is flagged:\n{log}");
}

/// A listener often misses the header itself. The frames that follow are ordinary session data
/// and must not be reported as a failed header.
#[test]
fn monitor_does_not_flag_later_frames_when_the_header_was_not_heard() {
    let log = session_with_header(|_| Vec::new(), &MonitorOpts::default());
    assert!(!log.contains("HEADER"), "{log}");
    assert!(
        log.lines().filter(|l| l.contains(" ctl ")).count() >= 2,
        "the session's data was not shown:
{log}"
    );
}
