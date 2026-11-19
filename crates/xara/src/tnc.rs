use crate::station::{HostConfig, Modem, Report, TX_RING_BYTES, secs};
use xara_arq::compression::Compression;
use xara_arq::fm::session::AUTOTUNE_LEVELS_DB;
use xara_arq::host::{HostEvent, LinkBw};
use xara_arq::kiss;
use xara_arq::time::{SECOND, Time};
use xara_host::command::{Command, Comp, DRIVE_MIN_DB, Drive, Invalid};
use xara_host::kiss::KissServer;
use xara_host::response::{LinkWidth, Response};
use xara_host::server::{Host, Input};
use xara_phy::fm::level::FmBandwidth;
use xara_phy::hf::level::Bandwidth;
use xara_ui::telemetry::{Bitrate, Control, Event, Probe, Telemetry, UI_TUNE_LIMIT};
const REGISTERED_AFTER: Time = 5 * SECOND;
pub const HOST_TUNE_LIMIT: Time = 60 * SECOND;
pub const RESTART_DELAY: Time = 3 * SECOND / 10;
const ALIVE_EVERY: Time = 60 * SECOND;
const HOST_INPUTS_PER_CALL: usize = 32;
const KISS_FRAMES_PER_CALL: usize = 16;

#[derive(Clone, Copy, PartialEq)]
enum Tuner {
    Host,
    Ui(u64),
}

impl Tuner {
    fn limit(self) -> Time {
        match self {
            Tuner::Host => HOST_TUNE_LIMIT,
            Tuner::Ui(_) => UI_TUNE_LIMIT,
        }
    }
}

pub struct Tnc<M: Modem> {
    station: M,
    host: Host,
    kiss: Option<KissServer>,
    echo: bool,
    keyed: bool,
    forced_off: bool,
    alive_at: Time,
    registered_at: Option<Time>,
    deferred_mycall: Option<Result<Vec<String>, Invalid>>,
    saved_retries: u32,
    busy: bool,
    telemetry: Option<Telemetry>,
    tune: Option<(Tuner, Time)>,
    restart: bool,
    restart_at: Option<Time>,
}

impl<M: Modem> Tnc<M> {
    pub fn new(station: M, host: Host) -> Self {
        Self {
            alive_at: station.now() + ALIVE_EVERY,
            station,
            host,
            kiss: None,
            echo: false,
            keyed: false,
            forced_off: false,
            registered_at: None,
            deferred_mycall: None,
            saved_retries: 0,
            busy: false,
            telemetry: None,
            tune: None,
            restart: false,
            restart_at: None,
        }
    }

    pub fn station(&self) -> &M {
        &self.station
    }

    pub fn host(&self) -> &Host {
        &self.host
    }

    pub fn keyed(&self) -> bool {
        self.keyed
    }

    pub fn set_tx_latency(&mut self, t: Time, lag: Time) {
        self.station.set_tx_latency(t, lag);
    }

    pub fn set_echo(&mut self, on: bool) {
        self.echo = on;
    }

    pub fn restart_requested(&self) -> bool {
        self.restart
    }

    pub fn set_telemetry(&mut self, t: Telemetry) {
        self.telemetry = Some(t);
    }

    pub fn force_ptt_off(&mut self, off: bool) {
        if off != self.forced_off {
            self.forced_off = off;
            if self.keyed {
                self.say(Response::Ptt(!off));
            }
        }
    }

    pub fn kiss(&self) -> Option<&KissServer> {
        self.kiss.as_ref()
    }

    pub fn set_kiss(&mut self, k: KissServer) {
        self.kiss = Some(k);
        if !M::KISS_RX_FOLLOWS_CLIENTS {
            self.station.set_kiss_rx(true);
        }
    }

    pub fn service(&mut self) {
        for (client, c) in self.telemetry.as_ref().map(Telemetry::controls).unwrap_or_default() {
            self.control(client, c);
        }
        if let Some(k) = &self.kiss {
            if M::KISS_RX_FOLLOWS_CLIENTS {
                self.station.set_kiss_rx(k.has_clients());
            }
            for (cmd, data) in k.input.try_iter().take(KISS_FRAMES_PER_CALL) {
                let Some(cmd) = kiss::Command::from_byte(cmd) else { continue };
                if !self.station.kiss(cmd, data) {
                    self.trace("Exceeded KISS buffer");
                }
            }
        }
        self.hold_data();
        for _ in 0..HOST_INPUTS_PER_CALL {
            let Ok(i) = self.host.input.try_recv() else { break };
            match i {
                Input::Command(c) => {
                    self.echo(&format!("> {c:?}"));
                    self.command(c);
                }
                Input::Wrong(l) => {
                    self.echo(&format!("> {l:?}"));
                    self.say(Response::Wrong);
                }
                Input::Data(d) => {
                    self.stat(|s| s.tx_bytes += d.len() as u64);
                    self.station.send(&d);
                    self.hold_data();
                }
                Input::Connected => self.toggle_busy(),
                Input::Closed => self.host_dropped(),
            }
        }
        self.flush();
    }

    pub fn process(&mut self, input: &[f64], output: &mut [f64]) {
        self.service();
        self.station.process(input, output);
        let now = self.station.now();
        if now >= self.alive_at {
            self.alive_at += ALIVE_EVERY;
            self.say(Response::IAmAlive);
        }
        if self.registered_at.is_some_and(|t| now >= t) {
            self.registered_at = None;
            self.say(registered(self.station.config()));
            self.say(Response::EncryptionDisabled);
        }
        self.flush();
        if self.station.busy() != self.busy {
            self.report_busy(self.station.busy());
        }
        self.limit_tune(now);
        if self.restart_at.is_some_and(|t| now >= t) {
            self.restart_at = None;
            if self.restartable() {
                self.restart = true;
            } else {
                self.controlled("restart".to_string(), false);
            }
        }
        self.publish(input, output);
    }

    fn limit_tune(&mut self, now: Time) {
        let Some((by, until)) = self.tune else { return };
        if !self.station.tuning() {
            self.tune = None;
        } else if now >= until {
            self.stop_tune();
            let what = format!("tune off after {} s", by.limit() / SECOND);
            self.echo(&format!("  {what}"));
            self.emit(|| Event::Control { what, ok: true });
        }
    }

    fn start_tune(&mut self, by: Tuner) -> bool {
        let ok = self.station.tune(true);
        if ok {
            self.tune = Some((by, self.station.now() + by.limit()));
        }
        ok
    }

    fn tuned_by(&self, by: Tuner) -> bool {
        self.tune.is_some_and(|(b, _)| b == by)
    }

    fn stop_tune(&mut self) -> bool {
        self.tune = None;
        self.station.tune(false)
    }

    fn set_drive(&mut self, db: f64) -> f64 {
        let db = db.clamp(DRIVE_MIN_DB, 0.0).round_ties_even();
        self.station.set_drive(db);
        db
    }

    fn host_tune(&mut self, drive: Drive) {
        match drive {
            Drive::OutOfRange => return,
            Drive::Db(db) => {
                self.set_drive(f64::from(db));
            }
            Drive::NotNumeric => {}
        }
        if !self.station.tuning() {
            self.start_tune(Tuner::Host);
        }
    }

    fn control(&mut self, client: u64, c: Control) {
        let (what, ok) = match c {
            Control::Left if self.tuned_by(Tuner::Ui(client)) && self.station.tuning() => {
                self.stop_tune();
                ("tune off: the page that started it is gone".to_string(), true)
            }
            Control::Left => return,
            Control::Tune(true) => ("tune on".to_string(), self.start_tune(Tuner::Ui(client))),
            Control::Tune(false) => ("tune off".to_string(), self.stop_tune()),
            Control::Drive(db) => (format!("drive {} dB", self.set_drive(db)), true),
            Control::Abort => {
                self.stop_tune();
                self.station.abort();
                ("abort".to_string(), true)
            }
            Control::Restart => {
                let ok = self.restartable();
                if ok {
                    self.restart_at = Some(self.station.now() + RESTART_DELAY);
                }
                ("restart".to_string(), ok)
            }
            // The page acts on the station directly, so a client on the command port sees only the link events.
            Control::Connect { remote, vias } => {
                let mycall = self.station.config().host().mycalls.first().cloned().unwrap_or_default();
                let ok = !self.station.ptt()
                    && (vias.is_empty() || M::VIAS)
                    && !vias.contains(&mycall)
                    && self.station.connect(&mycall, &remote, &vias);
                (format!("connect {remote}"), ok)
            }
            Control::Disconnect => {
                self.station.disconnect();
                ("disconnect".to_string(), true)
            }
            Control::Listen(on) => {
                self.station.set_listen(on);
                (format!("listen {}", if on { "on" } else { "off" }), true)
            }
            Control::Cq => {
                let mycall = self.station.config().host().mycalls.first().cloned().unwrap_or_default();
                ("cq".to_string(), self.idle() && self.station.cq_frame(&mycall, None))
            }
            Control::Probe { target, vias, tune } => {
                let what = format!("{} {target}", if tune { "autotune" } else { "ping" });
                (what, self.idle() && !self.station.ptt() && self.station.probe(&target, &vias, tune))
            }
        };
        self.controlled(what, ok);
        self.flush();
    }

    fn controlled(&self, what: String, ok: bool) {
        self.echo(&format!("ui {what}{}", if ok { "" } else { " refused" }));
        self.emit(|| Event::Control { what, ok });
    }

    fn publish(&self, input: &[f64], output: &[f64]) {
        let Some(t) = &self.telemetry else { return };
        let link = self.station.link();
        let (now, ptt, busy) = (self.station.now(), self.keyed && !self.forced_off, self.busy);
        let (tuning, drive_db) = (self.station.tuning(), self.station.options().drive_db);
        let tune_left = self.tune.map_or(0.0, |(_, u)| secs(u.saturating_sub(now)));
        let cfg = self.station.config();
        let (retries, buffered) = (cfg.host().retries, self.station.buffered());
        t.audio(input);
        let listen = self.station.listen_on();
        t.update(|s| {
            s.now = now;
            s.listen = listen;
            s.rx.feed(input);
            s.tx.feed(output);
            (s.ptt, s.busy, s.link, s.retries, s.buffered) = (ptt, busy, link, retries, buffered);
            (s.tuning, s.tune_left, s.drive_db) = (tuning, tune_left, drive_db);
            if s.calls != cfg.host().mycalls {
                s.calls = cfg.host().mycalls.clone();
            }
        });
    }

    /// Our FM Ping or Autotune was answered (or not). An Autotune answer sets the drive, as VARA FM does.
    fn probed(&mut self, call: String, sn: Option<f64>, step: Option<u8>) {
        let drive = step.filter(|s| (1..=10).contains(s)).map(|s| AUTOTUNE_LEVELS_DB[usize::from(s - 1)]);
        if let Some(db) = drive {
            self.set_drive(db);
        }
        let what = match (sn, step, drive) {
            (None, ..) => format!("{call}: no answer"),
            (Some(sn), None, _) => format!("{call} heard us at {sn:.1} dB S/N"),
            (Some(_), Some(s), Some(db)) => format!("{call}: Autotune step {s}, drive set to {db} dB"),
            (Some(_), Some(_), None) => format!("{call}: Autotune, no signal"),
        };
        self.trace(&what);
        if let Some(t) = &self.telemetry {
            t.update(|s| {
                s.probe = Some(Probe { call, at: s.now, sn, step });
            });
        }
    }

    /// Update the UI's counters for the link in progress, if there is one.
    fn stat(&self, f: impl FnOnce(&mut xara_ui::telemetry::Session)) {
        if let Some(t) = &self.telemetry {
            t.update(|s| {
                if let Some(cur) = s.session.as_mut() {
                    f(cur)
                }
            });
        }
    }

    fn echo(&self, s: &str) {
        if self.echo || crate::log::wants_lines() {
            crate::log::line(self.echo, format_args!("{:10.3} {s}", secs(self.station.now())));
        }
    }

    fn trace(&self, s: &str) {
        self.echo(&format!("  {s}"));
        self.emit(|| Event::Trace(s.to_string()));
    }

    fn emit(&self, e: impl FnOnce() -> Event) {
        if let Some(t) = &self.telemetry {
            t.event(self.station.now(), e());
        }
    }

    /// The audio device failed: tell the client, as VARA does, so gateway software can restart us.
    pub fn missing_soundcard(&self) {
        self.say(Response::MissingSoundcard);
    }

    fn say(&self, r: Response) {
        let s = r.to_string();
        self.echo(&format!("< {s}"));
        self.host.line(&s);
        let Some(t) = &self.telemetry else { return };
        match &r {
            Response::Sn(sn) => t.update(|s| s.sn = Some(*sn)),
            Response::Bitrate { level, bps, tx } => {
                t.update(|s| s.bitrate = Some(Bitrate { level: *level, bps: *bps, tx: *tx }))
            }
            Response::Connected { caller, answerer, .. } => {
                let me = &self.station.config().host().mycalls;
                let peer = if me.iter().any(|c| c.eq_ignore_ascii_case(caller)) { answerer } else { caller };
                t.update(|s| {
                    s.link_up(peer);
                    let sn = s.sn;
                    s.heard(peer, "linked", sn);
                });
            }
            Response::Disconnected => t.update(|s| {
                (s.sn, s.bitrate) = (None, None);
                s.link_down();
            }),
            _ => {}
        }
        t.event(self.station.now(), Event::Host(r));
    }

    fn report_busy(&mut self, on: bool) {
        self.busy = on;
        self.say(Response::Busy(on));
    }

    fn toggle_busy(&mut self) {
        self.report_busy(!self.busy);
    }

    fn hold_data(&self) {
        self.host.hold_data(!self.idle() && self.station.buffered() >= TX_RING_BYTES);
    }

    fn idle(&self) -> bool {
        self.station.idle()
    }

    fn restartable(&self) -> bool {
        self.idle() && !self.station.ptt() && !self.station.tuning()
    }

    fn reconfigure(&mut self, f: impl Fn(&mut M::Config)) -> bool {
        let mut cfg = self.station.config().clone();
        f(&mut cfg);
        self.station.reconfigure(cfg)
    }

    fn set_mycalls(&mut self, calls: Vec<String>) {
        if self.reconfigure(|c| c.host_mut().mycalls = calls.clone()) {
            self.registered_at = Some(self.station.now() + REGISTERED_AFTER);
        }
    }

    fn set_chat(&mut self, on: bool) {
        if !self.reconfigure(|c| c.host_mut().chat = on) {
            self.station.update(|c| c.host_mut().chat = on);
        }
    }

    fn host_dropped(&mut self) {
        if self.tuned_by(Tuner::Host) {
            self.stop_tune();
        }
        self.station.set_listen(false);
        self.station.drop_link();
        self.set_chat(false);
    }

    fn command(&mut self, c: Command) {
        let idle = self.idle();
        let ok = match c {
            Command::Connect { mycall, remote } => !self.station.ptt() && self.station.connect(&mycall, &remote, &[]),
            Command::ConnectVia { mycall, remote, vias } => {
                M::VIAS
                    && !self.station.ptt()
                    && !vias.contains(&mycall)
                    && self.station.connect(&mycall, &remote, &vias)
            }
            Command::Mycall(calls) if !idle => {
                self.deferred_mycall = Some(calls);
                false
            }
            Command::Mycall(Ok(calls)) => {
                self.deferred_mycall = None;
                self.set_mycalls(calls);
                true
            }
            Command::Mycall(Err(Invalid)) => {
                self.say(Response::Wrong);
                true
            }
            Command::CqFrame { call, bw } => idle && self.station.cq_frame(&call, bw.as_deref()),
            Command::Disconnect => {
                self.station.disconnect();
                true
            }
            Command::Abort => {
                self.station.abort();
                true
            }
            Command::Chat(on) if idle => {
                if on {
                    self.toggle_busy();
                }
                self.set_chat(on && self.host.has_cmd_client() && self.host.has_data_client());
                if on {
                    self.station.set_listen(true);
                }
                true
            }
            Command::Chat(_) => false,
            Command::WinlinkSession | Command::P2pSession => {
                self.station.set_p2p(c == Command::P2pSession);
                true
            }
            Command::Retries(v) => {
                let cur = self.station.config().host().retries;
                let new = match v {
                    Ok(0) if self.saved_retries > 0 => Some(self.saved_retries),
                    Ok(0) | Err(Invalid) => None,
                    Ok(n) => {
                        self.saved_retries = cur;
                        Some(u32::from(n))
                    }
                };
                if let Some(n) = new {
                    self.station.update(|c| c.host_mut().retries = n);
                }
                true
            }
            Command::Listen(on) => {
                let linked = !idle;
                self.station.set_listen(on);
                self.say(Response::Ok);
                if linked {
                    self.station.drop_link();
                }
                return;
            }
            Command::Compression(m) => {
                let m = match m {
                    Comp::Off => Compression::Off,
                    Comp::Text => Compression::Text,
                    Comp::Files => Compression::Files,
                };
                self.station.update(|c| c.host_mut().compression = m);
                true
            }
            Command::Bw(b) => idle && self.station.set_bw(b),
            Command::Gateway => M::CWID_GATEWAY,
            // US calls always ID (Session::cwid); this turns it on for everyone else.
            Command::Cwid(on) if M::CWID_GATEWAY => {
                self.station.update(|c| c.set_cwid(on));
                true
            }
            Command::Cwid(_) => false,
            Command::Public(_) => true,
            Command::TuneOff => {
                self.stop_tune();
                true
            }
            Command::Tune(d) => {
                self.say(Response::Ok);
                self.host_tune(d);
                return;
            }
            Command::DriveLevel(d) => {
                if let Drive::Db(db) = d {
                    self.set_drive(f64::from(db));
                }
                true
            }
            Command::IgnoreKissDcd(on) => {
                self.station.set_ignore_dcd(on);
                true
            }
            Command::TuneQuery => {
                self.say(Response::Tune(self.station.options().drive_db.round()));
                return;
            }
            Command::CleanTxBuffer => {
                self.say(Response::CleanTxBuffer { empty: self.station.buffered() == 0 });
                return;
            }
            Command::Version => {
                self.say(Response::Version(M::VERSION));
                return;
            }
        };
        self.say(if ok { Response::Ok } else { Response::Wrong });
    }

    fn flush(&mut self) {
        while let Some(r) = self.station.pop() {
            match r {
                Report::Trace(t) => self.trace(&t),
                Report::Ptt(on) => {
                    self.keyed = on;
                    if !self.forced_off {
                        self.say(Response::Ptt(on))
                    }
                }
                Report::Busy(on) => {
                    if on != self.busy {
                        self.report_busy(on);
                    }
                }
                Report::UnencryptedLink => self.say(Response::UnencryptedLink),
                Report::Line(l) => self.say(Response::Line(l)),
                Report::Kiss(cmd, data) => {
                    if let Some(k) = &self.kiss {
                        k.send(cmd.byte(), &data);
                    }
                }
                Report::Bitrate { level, bps, tx } => self.say(Response::Bitrate { level, bps, tx }),
                Report::Host(HostEvent::Data(d)) => {
                    self.stat(|s| s.rx_bytes += d.len() as u64);
                    self.host.data(&d)
                }
                Report::Heard { call, what, sn } => {
                    if let Some(t) = &self.telemetry {
                        t.update(|s| {
                            let sn = Some(sn).filter(|v| v.is_finite()).or(s.sn);
                            s.heard(&call, what, sn)
                        });
                    }
                }
                Report::Host(HostEvent::Disconnected) => {
                    let deferred = self.deferred_mycall.take();
                    if let Some(Err(Invalid)) = deferred {
                        self.say(Response::Wrong);
                    }
                    self.say(Response::Disconnected);
                    if let Some(Ok(calls)) = deferred {
                        self.set_mycalls(calls);
                    }
                }
                Report::Host(HostEvent::Probe { call, sn, step }) => self.probed(call, sn, step),
                Report::Host(HostEvent::UnableToConnect) => self.emit(|| Event::UnableToConnect),
                Report::Host(HostEvent::TimedOut) => self.emit(|| Event::TimedOut),
                Report::Host(h) => {
                    if let Some(r) = response(h) {
                        self.say(r);
                    }
                }
            }
        }
    }
}

fn registered(cfg: &impl HostConfig) -> Response {
    Response::Registered(cfg.host().registered.then(|| cfg.host().mycalls.clone()))
}

pub fn response(h: HostEvent) -> Option<Response> {
    Some(match h {
        HostEvent::Pending => Response::Pending,
        HostEvent::CancelPending => Response::CancelPending,
        HostEvent::Connected { caller, answerer, vias, bw } => {
            let width = match bw {
                LinkBw::Hf(Bandwidth::Sat) => LinkWidth::Sat,
                LinkBw::Hf(b) => LinkWidth::Hz(b.hz()),
                LinkBw::Fm(FmBandwidth::Narrow) => LinkWidth::Narrow,
                LinkBw::Fm(FmBandwidth::Wide) => LinkWidth::Wide,
            };
            Response::Connected { caller, answerer, vias, width }
        }
        HostEvent::Disconnected => Response::Disconnected,
        HostEvent::Registered(r) => Response::LinkRegistered(r),
        HostEvent::Buffer(n) => Response::Buffer(n),
        HostEvent::Sn(s) => Response::Sn(s),
        HostEvent::UnableToConnect
        | HostEvent::TimedOut
        | HostEvent::Data(_)
        | HostEvent::Unencrypted
        | HostEvent::RxFrame(_)
        | HostEvent::Log(_)
        | HostEvent::Probe { .. } => return None,
    })
}
