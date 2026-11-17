use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};
use xara::audio::{self, Backend};
use xara::cli::{self, UiArgs, Usage, num};
use xara::monitor::fm::{self, FmMonitor};
use xara::monitor::ui::{Heard, Shared};
use xara::monitor::{Monitor, MonitorOpts};
use xara::resample::{Resampler, check_rate};
use xara::say;
#[cfg(unix)]
use xara::signals;
use xara_phy::fm::level::FmBandwidth;
use xara_phy::hf::level::Bandwidth;
use xara_phy::hf::rx::FS;
use xara_ui::bus;
use xara_ui::messages::{Info, meter};
use xara_ui::server::{MONITOR, TICK, UiServer};
use xara_ui::telemetry::{EVENT_QUEUE, Meter};

const QUEUE: usize = 1200;
const CAPTURE_BATCH: usize = 4800;

const USAGE: Usage = Usage {
    name: "xara-monitor",
    synopsis: "usage: xara-monitor (--wav FILE | --stdin RATE | --capture SOURCE) [options]",
    tagline: "a passive VARA HF traffic monitor",
    sections: &["xara-monitor"],
};

enum Input {
    Wav(String),
    Stdin(f64),
    Capture(String),
}

struct Args {
    input: Input,
    audio: Option<Backend>,
    bw: Bandwidth,
    fm: Option<FmBandwidth>,
    mon: MonitorOpts,
    json: bool,
    ui: UiArgs,
    log: Option<PathBuf>,
    syslog: Option<(String, String)>,
}

fn parse() -> Result<Option<Args>, String> {
    let (mut input, mut bw, mut calls, mut names) = (None, Bandwidth::Hz2300, Vec::new(), Vec::new());
    let (mut fm, mut hf_given) = (None, false);
    let mut audio = None;
    let (mut blind, mut dump, mut verbose, mut key_idle) = (true, false, false, xara::monitor::KEY_IDLE_S);
    let (mut json, mut log) = (false, None);
    let (mut syslog, mut syslog_name) = (None, String::from("xara-monitor"));
    let mut ui = UiArgs::default();
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || it.next().ok_or(format!("{k} needs a value"));
        if cli::meta(&k, &USAGE) {
            return Ok(None);
        }
        if ui.take(&k, &mut v)? {
            continue;
        }
        match k.as_str() {
            "--wav" => input = Some(Input::Wav(v()?)),
            "--stdin" => {
                let rate = num(&k, &v()?)?;
                input = Some(Input::Stdin(check_rate(rate).map_err(|e| format!("--stdin: {e}"))?));
            }
            "--audio" => audio = Some(v()?.parse()?),
            "--capture" => input = Some(Input::Capture(v()?)),
            "--hf" => {
                bw = v()?.parse()?;
                hf_given = true;
            }
            "--fm" => {
                fm = Some(match v()?.to_ascii_lowercase().as_str() {
                    "narrow" => FmBandwidth::Narrow,
                    "wide" => FmBandwidth::Wide,
                    m => return Err(format!("--fm: expected narrow or wide, got {m}")),
                });
            }
            "--call" => calls.push(cli::call(&v()?)?),
            "--calls" => {
                let path = v()?;
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
                names.extend(
                    text.lines()
                        .filter_map(|l| l.split(['#', ',', ' ', '\t']).next())
                        .filter(|c| !c.is_empty())
                        .map(str::to_ascii_uppercase),
                );
            }
            "--no-blind" => blind = false,
            "--key-idle" => {
                let m: f64 = num(&k, &v()?)?;
                if !(m.is_finite() && m >= 0.0) {
                    return Err("--key-idle: minutes must be >= 0".into());
                }
                key_idle = m * 60.0;
            }
            "--dump" => dump = true,
            "--verbose" => verbose = true,
            "--json" => json = true,
            "--log" => log = Some(PathBuf::from(v()?)),
            "--syslog" => syslog = Some(v()?),
            "--syslog-name" => syslog_name = v()?,
            _ => return Err(format!("unknown option {k}")),
        }
    }
    let input = input.ok_or("one of --wav, --stdin, or --capture is required")?;
    if audio.is_some() && !matches!(input, Input::Capture(_)) {
        return Err("--audio is for --capture".into());
    }
    if ui.port.is_none() && (ui.addr.is_some() || !ui.hosts.is_empty()) {
        return Err("--ui-addr and --ui-host need --ui-port".into());
    }
    if fm.is_some() && hf_given {
        return Err("--hf and --fm cannot both be given".into());
    }
    let mon = MonitorOpts { calls, names, blind, key_idle, dump, verbose };
    let syslog = syslog.map(|s| (s, syslog_name));
    Ok(Some(Args { input, audio, bw, fm, mon, json, ui, log, syslog }))
}

fn utc(t: f64) -> String {
    let d = (t.rem_euclid(86400.0) * 10.0).round() as u64 % 864_000;
    format!("{:02}:{:02}:{:02}.{}", d / 36_000, d / 600 % 60, d / 10 % 60, d % 10)
}

#[derive(Clone, Copy)]
struct Out {
    epoch: Option<f64>,
    json: bool,
}

fn drain(m: &mut Monitor, out: Out, ui: &mut Option<(Arc<Shared>, Heard)>) -> std::io::Result<()> {
    let mut o = std::io::stdout().lock();
    while let Some(l) = m.pop() {
        if let Some((s, h)) = ui {
            s.line(&l, h);
        }
        if xara::log::to_file() {
            match out.epoch {
                Some(e) => xara::log::line(false, format_args!("{} {}", utc(e + l.t), l.text)),
                None => xara::log::line(false, format_args!("{:9.3} {}", l.t, l.text)),
            }
        }
        match (out.json, out.epoch) {
            (true, e) => writeln!(o, "{}", l.rec.json(l.t, e.map(|e| e + l.t), &l.text))?,
            (false, Some(e)) => writeln!(o, "{} {}", utc(e + l.t), l.text)?,
            (false, None) => writeln!(o, "{:9.3} {}", l.t, l.text)?,
        }
    }
    if let Some((s, h)) = ui {
        s.decoded(m, h);
    }
    o.flush()
}

struct Decoder {
    tx: Option<SyncSender<Vec<f64>>>,
    worker: Option<JoinHandle<io::Result<()>>>,
    ui: Option<Arc<Shared>>,
}

impl Decoder {
    fn spawn(m: Monitor, out: Out, ui: Option<Arc<Shared>>) -> io::Result<Self> {
        let (tx, rx) = sync_channel(QUEUE);
        let shared = ui.clone();
        let worker = thread::Builder::new().name("decode".into()).spawn(move || decode(m, rx, out, shared))?;
        Ok(Self { tx: Some(tx), worker: Some(worker), ui })
    }

    fn send(&self, x: Vec<f64>) -> bool {
        if let Some(s) = &self.ui {
            s.input(&x);
        }
        self.tx.as_ref().is_some_and(|t| t.send(x).is_ok())
    }

    fn finish(mut self, input: io::Result<()>) -> io::Result<()> {
        drop(self.tx.take());
        let done = self
            .worker
            .take()
            .map_or(Ok(()), |w| w.join().unwrap_or_else(|_| Err(io::Error::other("decoder panicked"))));
        done.and(input)
    }
}

fn decode(mut m: Monitor, rx: Receiver<Vec<f64>>, out: Out, ui: Option<Arc<Shared>>) -> io::Result<()> {
    let mut ui = ui.map(|s| (s, Heard::default()));
    for x in rx {
        for c in x.chunks(audio::BLOCK) {
            m.feed(c);
        }
        drain(&mut m, out, &mut ui)?;
    }
    m.finish();
    drain(&mut m, out, &mut ui)
}

// Off Unix there is no web UI (see xara-ui), so a Ui is never made there.
#[cfg_attr(not(unix), allow(dead_code))]
struct Ui {
    shared: Arc<Shared>,
    status: JoinHandle<()>,
}

fn serve(a: &Args, source: &str, epoch: Option<f64>, stop: &Arc<AtomicBool>) -> io::Result<Option<Ui>> {
    let Some(port) = a.ui.port else { return Ok(None) };
    let (publisher, sub) = bus::channel(EVENT_QUEUE);
    let info = Info { version: env!("CARGO_PKG_VERSION"), mode: "hf", span_hz: xara::HF_SPAN_HZ };
    let addr = a.ui.addr();
    let ui = UiServer::bind_readonly(addr, port, sub, info, MONITOR, a.ui.hosts.clone())
        .map_err(|e| io::Error::other(format!("--ui-port {port}: {e}")))?;
    say!("UI http://{}/ (read-only)", ui.addr);
    let shared = Arc::new(Shared::new(publisher, source, a.bw, epoch));
    let (s, stop) = (shared.clone(), stop.clone());
    let status = thread::Builder::new().name("ui-status".into()).spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            thread::sleep(TICK);
            s.publisher.status(s.status());
        }
    })?;
    Ok(Some(Ui { shared, status }))
}

fn linger(ui: Option<Ui>) {
    let Some(u) = ui else { return };
    u.shared.end();
    say!("input ended; the page stays up until interrupted");
    let _ = u.status.join();
}

fn start(a: &Args, source: &str, epoch: Option<f64>) -> std::io::Result<()> {
    if !a.json {
        return Ok(());
    }
    let mut o = xara_ui::json::Obj::typed("start")
        .str("version", env!("CARGO_PKG_VERSION"))
        .str("source", source)
        .int("bw", u64::from(a.bw.hz()))
        .bool("blind", a.mon.blind)
        .strs("calls", &a.mon.calls);
    if let Some(e) = epoch {
        o = o.num2("unix", e);
    }
    let mut w = std::io::stdout().lock();
    writeln!(w, "{}", o.end())?;
    w.flush()
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn pipe(
    a: &Args,
    m: Monitor,
    stop: &Arc<AtomicBool>,
    kind: &str,
    source: &str,
    epoch: Option<f64>,
    feed: impl FnOnce(&Decoder) -> io::Result<()>,
) -> io::Result<Option<Ui>> {
    start(a, kind, epoch)?;
    let ui = serve(a, source, epoch, stop)?;
    let d = Decoder::spawn(m, Out { epoch, json: a.json }, ui.as_ref().map(|u| u.shared.clone()))?;
    let input = feed(&d);
    d.finish(input)?;
    Ok(ui)
}

fn drain_fm(
    m: &mut FmMonitor,
    o: &mut impl Write,
    epoch: Option<f64>,
    json: bool,
    events: Option<&bus::Publisher>,
) -> io::Result<()> {
    while let Some(l) = m.pop() {
        if let Some(p) = events {
            p.event(l.json(epoch.map(|e| e + l.t)));
        }
        if xara::log::to_file() {
            match epoch {
                Some(e) => xara::log::line(false, format_args!("{} {}", utc(e + l.t), l.text)),
                None => xara::log::line(false, format_args!("{:9.3} {}", l.t, l.text)),
            }
        }
        match (json, epoch) {
            (true, e) => writeln!(o, "{}", l.json(e.map(|e| e + l.t)))?,
            (false, Some(e)) => writeln!(o, "{} {}", utc(e + l.t), l.text)?,
            (false, None) => writeln!(o, "{:9.3} {}", l.t, l.text)?,
        }
    }
    o.flush()
}

fn start_fm(o: &mut impl Write, bw: FmBandwidth, known: &[String], epoch: Option<f64>) -> io::Result<()> {
    let mut obj = xara_ui::json::Obj::typed("start")
        .str("version", env!("CARGO_PKG_VERSION"))
        .str("mode", "fm")
        .str("bw", if bw == FmBandwidth::Wide { "wide" } else { "narrow" })
        .strs("calls", known);
    if let Some(e) = epoch {
        obj = obj.num2("unix", e);
    }
    writeln!(o, "{}", obj.end())?;
    o.flush()
}

// Feeds the read-only web UI for the FM monitor: the heard list and stats come
// from the FmMonitor, the event log from each decoded line, and the waterfall
// from the audio. HF-decoder-only panels (link, keys, per-frame search counts)
// stay empty for FM.
struct FmUi {
    publisher: bus::Publisher,
    meter: Meter,
    sent: u64,
    source: String,
    epoch: Option<f64>,
}

impl FmUi {
    fn setup(a: &Args, source: String, epoch: Option<f64>) -> io::Result<Option<FmUi>> {
        let Some(port) = a.ui.port else { return Ok(None) };
        let (publisher, sub) = bus::channel(EVENT_QUEUE);
        let info = Info { version: env!("CARGO_PKG_VERSION"), mode: "fm", span_hz: xara::FM_SPAN_HZ };
        let server = UiServer::bind_readonly(a.ui.addr(), port, sub, info, MONITOR, a.ui.hosts.clone())
            .map_err(|e| io::Error::other(format!("--ui-port {port}: {e}")))?;
        say!("UI http://{}/ (read-only)", server.addr);
        Ok(Some(FmUi { publisher, meter: Meter::default(), sent: 0, source, epoch }))
    }

    fn audio(&mut self, x: &[f64]) {
        self.meter.feed(x);
        self.publisher.audio(x);
        self.sent += x.len() as u64;
    }

    fn status(&mut self, m: &FmMonitor, ended: bool) {
        let ts = self.sent as f64 / fm::FS;
        let rx = std::mem::take(&mut self.meter);
        let span = f64::from(xara::FM_SPAN_HZ);
        let s = xara_ui::json::Obj::typed("status")
            .num2("ts", ts)
            .opt2("epoch", self.epoch)
            .str("source", &self.source)
            .str("mode", "FM")
            .int("bw", u64::from(xara::FM_SPAN_HZ))
            .nums2("passband", &[0.0, span])
            .obj("rx", meter(&rx))
            .num2("lag", 0.0)
            .bool("ended", ended)
            .int("dropped", self.publisher.dropped() as u64)
            .raw("decoder", &m.decoder_json())
            .end();
        self.publisher.status(s);
    }
}

fn fm_source(input: &Input) -> String {
    match input {
        Input::Wav(p) => p.clone(),
        Input::Stdin(_) => "stdin".into(),
        Input::Capture(s) => format!("capture {s}"),
    }
}

// A passive FM monitor: decode synchronously and print lines, and (with
// --ui-port) feed the read-only web UI. Session-payload following is still the
// terminal/JSON path (issue #2). The audio rate is the same 48 kHz the HF
// monitor uses, so the input handling mirrors it.
fn run_fm(a: &Args, bw: FmBandwidth, stop: &Arc<AtomicBool>) -> io::Result<()> {
    let known: Vec<String> = a.mon.calls.iter().chain(&a.mon.names).cloned().collect();
    let mut m = FmMonitor::new(bw, &fm::Opts { known: known.clone() });
    xara::log::line(
        true,
        format_args!(
            "xara-monitor {}: FM {}, {} known calls",
            env!("CARGO_PKG_VERSION"),
            if bw == FmBandwidth::Wide { "wide" } else { "narrow" },
            known.len(),
        ),
    );
    let mut o = std::io::stdout().lock();
    // A recorded file plays on its own time base (seconds from the start); live
    // inputs stamp against wall-clock epoch.
    let epoch = match &a.input {
        Input::Wav(_) => None,
        _ => Some(now()),
    };
    if a.json {
        start_fm(&mut o, bw, &known, epoch)?;
    }
    let mut ui = FmUi::setup(a, fm_source(&a.input), epoch)?;
    match &a.input {
        Input::Wav(path) => {
            let w =
                xara::wav::read(std::path::Path::new(path)).map_err(|e| io::Error::other(format!("{path}: {e}")))?;
            say!("{path}: {} Hz, {:.1} s", w.rate, w.samples.len() as f64 / f64::from(w.rate));
            let mut r =
                Resampler::new(f64::from(w.rate), fm::FS).map_err(|e| io::Error::other(format!("{path}: {e}")))?;
            let mut x = r.process(&w.samples);
            x.extend(r.flush());
            for c in x.chunks(CAPTURE_BATCH) {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                for b in c.chunks(audio::BLOCK) {
                    m.feed(b);
                    if let Some(u) = &mut ui {
                        u.audio(b);
                    }
                }
                drain_fm(&mut m, &mut o, epoch, a.json, ui.as_ref().map(|u| &u.publisher))?;
                if let Some(u) = &mut ui {
                    u.status(&m, false);
                }
            }
        }
        Input::Stdin(rate) => {
            let mut r = Resampler::new(*rate, fm::FS).map_err(io::Error::other)?;
            let mut stdin = io::stdin().lock();
            let (mut buf, mut carry) = (vec![0u8; 4800], Vec::new());
            while !stop.load(Ordering::SeqCst) {
                let n = stdin.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                carry.extend_from_slice(&buf[..n]);
                let whole = carry.len() & !1;
                let x: Vec<f64> = carry[..whole].chunks_exact(2).map(xara::wav::s16).collect();
                carry.drain(..whole);
                for b in r.process(&x).chunks(audio::BLOCK) {
                    m.feed(b);
                    if let Some(u) = &mut ui {
                        u.audio(b);
                    }
                }
                drain_fm(&mut m, &mut o, epoch, a.json, ui.as_ref().map(|u| &u.publisher))?;
                if let Some(u) = &mut ui {
                    u.status(&m, false);
                }
            }
        }
        Input::Capture(src) => {
            let mut dev = audio::open_capture(a.audio.unwrap_or_default(), src)?;
            say!("listening on {src}");
            let mut x = vec![0i16; audio::BLOCK];
            while !stop.load(Ordering::SeqCst) {
                dev.read(&mut x)?;
                let y: Vec<f64> = x.iter().map(|&s| audio::sample(s)).collect();
                m.feed(&y);
                if let Some(u) = &mut ui {
                    u.audio(&y);
                }
                drain_fm(&mut m, &mut o, epoch, a.json, ui.as_ref().map(|u| &u.publisher))?;
                if let Some(u) = &mut ui {
                    u.status(&m, false);
                }
            }
        }
    }
    m.finish();
    drain_fm(&mut m, &mut o, epoch, a.json, ui.as_ref().map(|u| &u.publisher))?;
    // A summary on stderr (so --json stdout stays clean) that the corpus
    // regression check can parse, mirroring the HF monitor's "frames N/M ok".
    let s = m.stats();
    say!(
        "summary: {} transmissions, frames {}/{} ok, {} connects, {} cq",
        s.transmissions,
        s.frames_ok,
        s.frames,
        s.connects,
        s.cq
    );
    if let Some(u) = &mut ui {
        u.status(&m, true);
    }
    if ui.is_some() {
        say!("input ended; the page stays up until interrupted");
        while !stop.load(Ordering::SeqCst) {
            thread::sleep(TICK);
        }
    }
    Ok(())
}

fn run(a: Args, stop: &Arc<AtomicBool>) -> std::io::Result<()> {
    if let Some(bw) = a.fm {
        return run_fm(&a, bw, stop);
    }
    let m = Monitor::new(a.bw, &a.mon);
    xara::log::line(
        true,
        format_args!(
            "xara-monitor {}: {} Hz, {} configured calls, {} names, blind {}",
            env!("CARGO_PKG_VERSION"),
            a.bw.hz(),
            a.mon.calls.len(),
            a.mon.names.len(),
            if a.mon.blind { "on" } else { "off" }
        ),
    );
    match &a.input {
        Input::Wav(path) => {
            let w =
                xara::wav::read(std::path::Path::new(&path)).map_err(|e| io::Error::other(format!("{path}: {e}")))?;
            say!("{path}: {} Hz, {:.1} s", w.rate, w.samples.len() as f64 / f64::from(w.rate));
            let mut r = Resampler::new(f64::from(w.rate), FS).map_err(|e| io::Error::other(format!("{path}: {e}")))?;
            let mut x = r.process(&w.samples);
            x.extend(r.flush());
            let ui = pipe(&a, m, stop, "wav", "wav", None, |d| {
                for c in x.chunks(CAPTURE_BATCH) {
                    if stop.load(Ordering::SeqCst) || !d.send(c.to_vec()) {
                        break;
                    }
                }
                Ok(())
            })?;
            linger(ui);
        }
        Input::Stdin(rate) => {
            let (epoch, mut r) = (now(), Resampler::new(*rate, FS).map_err(io::Error::other)?);
            let mut stdin = io::stdin().lock();
            let (mut buf, mut carry) = (vec![0u8; 4800], Vec::new());
            let ui = pipe(&a, m, stop, "stdin", "stdin", Some(epoch), |d| {
                while !stop.load(Ordering::SeqCst) {
                    let n = stdin.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    carry.extend_from_slice(&buf[..n]);
                    let whole = carry.len() & !1;
                    let x: Vec<f64> = carry[..whole].chunks_exact(2).map(xara::wav::s16).collect();
                    carry.drain(..whole);
                    if !d.send(r.process(&x)) {
                        break;
                    }
                }
                Ok(())
            })?;
            linger(ui);
        }
        Input::Capture(src) => {
            let mut io = audio::open_capture(a.audio.unwrap_or_default(), src)?;
            let epoch = now();
            say!("listening on {src}");
            let (mut x, mut batch) = (vec![0i16; audio::BLOCK], Vec::new());
            pipe(&a, m, stop, "capture", &format!("capture {src}"), Some(epoch), |d| {
                let r = (|| {
                    while !stop.load(Ordering::SeqCst) {
                        io.read(&mut x)?;
                        batch.extend(x.iter().map(|&s| audio::sample(s)));
                        if batch.len() >= CAPTURE_BATCH && !d.send(std::mem::take(&mut batch)) {
                            break;
                        }
                    }
                    Ok(())
                })();
                if !batch.is_empty() {
                    d.send(std::mem::take(&mut batch));
                }
                r
            })?;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    xara::log::init("xara-monitor", false);
    let a = match cli::args(parse(), &USAGE) {
        Ok(a) => a,
        Err(c) => return c,
    };
    if let Some(p) = &a.log {
        if let Err(e) = xara::log::open(p) {
            say!("--log {}: {e}", p.display());
            return ExitCode::FAILURE;
        }
    }
    if let Some((to, name)) = &a.syslog {
        if let Err(e) = xara::log::syslog(to, name) {
            say!("--syslog {to}: {e}");
            return ExitCode::FAILURE;
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    // Off Unix, Ctrl-C ends the process at once: fine for a monitor, which
    // never keys a radio and has nothing to release.
    #[cfg(unix)]
    let _ = signals::trap(&[libc::SIGINT, libc::SIGTERM], stop.clone(), || {});
    match run(a, &stop) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            say!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::utc;

    #[test]
    fn utc_carries_rounded_seconds() {
        assert_eq!(utc(0.0), "00:00:00.0");
        assert_eq!(utc(3600.0 + 59.94), "01:00:59.9");
        assert_eq!(utc(3600.0 + 3.0 * 60.0 + 59.96), "01:04:00.0");
        assert_eq!(utc(23.0 * 3600.0 + 59.0 * 60.0 + 59.97), "00:00:00.0");
        assert_eq!(utc(-0.04), "00:00:00.0");
        assert_eq!(utc(86400.0 * 3.0 + 45296.14), "12:34:56.1");
    }
}
