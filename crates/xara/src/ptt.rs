use std::cell::Cell;
#[cfg(unix)]
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::io::IsTerminal;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, Once, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(unix)]
use crate::Ioctl;
#[cfg(target_os = "macos")]
use crate::hid::cm108_usb;
#[cfg(unix)]
pub use crate::hidraw::is_hidraw;
#[cfg(windows)]
pub use win::Serial;

/// The serial modem line a `Serial` PTT drives.
#[cfg(unix)]
pub const RTS: libc::c_int = libc::TIOCM_RTS;
#[cfg(unix)]
pub const DTR: libc::c_int = libc::TIOCM_DTR;
#[cfg(windows)]
pub use win::{DTR, RTS};

pub trait Ptt: Send {
    fn set(&mut self, on: bool) -> io::Result<()>;
    fn release(&mut self) -> io::Result<()> {
        self.set(false)
    }
    fn describe(&self) -> String;
}

#[derive(Clone, Debug, PartialEq)]
pub enum PttSpec {
    Vox,
    Rts(String),
    Dtr(String),
    Cm108 {
        path: String,
        gpio: u8,
    },
    Cm108Usb {
        vendor: u16,
        product: u16,
        gpio: u8,
    },
    Rigctld(String),
    Trusdx,
    /// Built-in CAT; `None` until `--cat` fills it in (`--ptt cat` with a separate `--cat`).
    Cat(Option<crate::cat::CatSpec>),
}

pub const RIGCTLD_DEFAULT: &str = "127.0.0.1:4532";

impl PttSpec {
    pub fn parse(s: &str) -> Result<Self, String> {
        let (kind, rest) = s.split_once(':').unwrap_or((s, ""));
        let path = |what: &str| {
            if rest.starts_with('/') || (cfg!(windows) && win_port(rest)) {
                Ok(rest.to_string())
            } else if cfg!(windows) {
                Err(format!("--ptt {what}: needs a serial port, e.g. {what}:COM3"))
            } else {
                Err(format!("--ptt {what}: needs a device path, e.g. {what}:/dev/ttyACM0"))
            }
        };
        match kind {
            "vox" if rest.is_empty() => Ok(Self::Vox),
            "rts" => path("rts").map(Self::Rts),
            "dtr" => path("dtr").map(Self::Dtr),
            "cm108" if cfg!(windows) => {
                Err("--ptt cm108: not supported on Windows yet; use rts/dtr, rigctld or vox".into())
            }
            "cm108" if cfg!(target_os = "macos") && rest.starts_with('/') => {
                Err("--ptt cm108: macOS has no hidraw; use cm108:VID:PID[:GPIO] (4-digit hex USB ids)".into())
            }
            "cm108" => Self::cm108(rest).ok_or_else(|| {
                let dev = if cfg!(target_os = "macos") { "" } else { "/dev/hidrawN[:GPIO] or " };
                format!("--ptt cm108: expected {dev}VID:PID[:GPIO] (VID and PID as 4-digit hex, GPIO 1-8)")
            }),
            "trusdx" if !cfg!(feature = "trusdx") => {
                Err("--ptt trusdx: xara was built without the trusdx feature".into())
            }
            "trusdx" if rest.is_empty() => Ok(Self::Trusdx),
            "cat" if rest.is_empty() => Ok(Self::Cat(None)),
            "cat" => crate::cat::CatSpec::parse(rest)
                .map(|c| Self::Cat(Some(c)))
                .map_err(|e| e.replace("--cat", "--ptt cat")),
            "rigctld" => Ok(Self::Rigctld(if rest.is_empty() { RIGCTLD_DEFAULT.into() } else { rest.into() })),
            _ => {
                let cm108 = if cfg!(target_os = "macos") { "cm108:VID:PID[:GPIO]" } else { "cm108:DEV|VID:PID[:GPIO]" };
                let mut all =
                    vec!["vox", "rts:DEV", "dtr:DEV", cm108, "rigctld[:HOST:PORT]", "cat[:KIND:PORT[:BAUD[:ADDR]]]"];
                if cfg!(feature = "trusdx") {
                    all.push("trusdx");
                }
                Err(format!("--ptt {s:?}: expected {}", crate::cli::either(&all)))
            }
        }
    }

    fn cm108(s: &str) -> Option<Self> {
        let gpio = |g: Option<&str>| g.map_or(Some(3), |g| g.parse().ok().filter(|g| (1..=8).contains(g)));
        if s.starts_with('/') {
            let (path, g) = s.rsplit_once(':').map_or((s, None), |(p, g)| (p, Some(g)));
            return Some(Self::Cm108 { path: path.into(), gpio: gpio(g)? });
        }
        let id = |v: &str| {
            u16::from_str_radix(v, 16).ok().filter(|_| v.len() == 4 && v.bytes().all(|b| b.is_ascii_hexdigit()))
        };
        let mut f = s.split(':');
        let (vendor, product, g) = (id(f.next()?)?, id(f.next()?)?, f.next());
        f.next().is_none().then_some(Self::Cm108Usb { vendor, product, gpio: gpio(g)? })
    }

    pub fn open(&self) -> io::Result<Box<dyn Ptt>> {
        Ok(match self {
            Self::Vox => Box::new(Vox),
            Self::Rts(p) => Box::new(Serial::open(p, RTS)?),
            Self::Dtr(p) => Box::new(Serial::open(p, DTR)?),
            #[cfg(unix)]
            Self::Cm108 { path, gpio } => Box::new(Cm108::open(path, *gpio)?),
            #[cfg(unix)]
            Self::Cm108Usb { vendor, product, gpio } => cm108_usb(*vendor, *product, *gpio)?,
            #[cfg(windows)]
            Self::Cm108 { .. } | Self::Cm108Usb { .. } => {
                return Err(io::Error::other("--ptt cm108 is not supported on Windows"));
            }
            Self::Rigctld(a) => Box::new(Rigctld::open(a)?),
            Self::Cat(Some(c)) => Box::new(c.open()?),
            Self::Cat(None) => return Err(io::Error::other("--ptt cat needs --cat KIND:PORT[:BAUD[:ADDR]]")),
            Self::Trusdx => return Err(io::Error::other("--ptt trusdx comes with --audio trusdx")),
        })
    }
}

pub struct Vox;

impl Ptt for Vox {
    fn set(&mut self, _: bool) -> io::Result<()> {
        Ok(())
    }

    fn describe(&self) -> String {
        "VOX (no PTT line)".into()
    }
}

#[cfg(unix)]
pub struct Serial {
    f: File,
    path: String,
    line: libc::c_int,
}

#[cfg(unix)]
pub fn open_tty(path: &str) -> io::Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;
    if !f.is_terminal() {
        return Err(io::Error::other(format!("{path} is not a serial port")));
    }
    Ok(f)
}

#[cfg(unix)]
pub fn modem(f: &File, req: Ioctl, bits: libc::c_int) -> io::Result<()> {
    // SAFETY: f is open and bits is the int the request reads.
    if unsafe { libc::ioctl(f.as_raw_fd(), req, &bits) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
impl Serial {
    pub fn open(path: &str, line: libc::c_int) -> io::Result<Self> {
        let f = open_tty(path)?;
        modem(&f, libc::TIOCMBIC, libc::TIOCM_RTS | libc::TIOCM_DTR)?;
        Ok(Self { f, path: path.into(), line })
    }

    fn lines(&self) -> io::Result<libc::c_int> {
        let mut bits: libc::c_int = 0;
        // SAFETY: self.f is open and bits is the int TIOCMGET writes.
        if unsafe { libc::ioctl(self.f.as_raw_fd(), libc::TIOCMGET, &mut bits) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(bits)
    }
}

#[cfg(unix)]
impl Ptt for Serial {
    fn set(&mut self, on: bool) -> io::Result<()> {
        modem(&self.f, if on { libc::TIOCMBIS } else { libc::TIOCMBIC }, self.line)?;
        if (self.lines()? & self.line != 0) != on {
            return Err(io::Error::other(format!("{}: modem line did not follow", self.path)));
        }
        Ok(())
    }

    fn describe(&self) -> String {
        format!("{} on {} (other line held low)", if self.line == RTS { "RTS" } else { "DTR" }, self.path)
    }
}

#[cfg(unix)]
pub struct Cm108 {
    f: File,
    path: String,
    gpio: u8,
}

pub fn cm108_report(gpio: u8, on: bool) -> [u8; 5] {
    let m = 1u8 << (gpio - 1);
    [0, 0, if on { m } else { 0 }, m, 0]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn cm108_usb(vendor: u16, product: u16, gpio: u8) -> io::Result<Box<dyn Ptt>> {
    Ok(Box::new(Cm108::open(&crate::hidraw::find(vendor, product)?, gpio)?))
}

#[cfg(unix)]
impl Cm108 {
    pub fn open(path: &str, gpio: u8) -> io::Result<Self> {
        let f = OpenOptions::new().write(true).open(path)?;
        if !is_hidraw(path) {
            say!("warning: {path} is not a hidraw device; CM108 PTT may not work");
        }
        let mut c = Self { f, path: path.into(), gpio };
        c.set(false)?;
        Ok(c)
    }
}

#[cfg(unix)]
impl Ptt for Cm108 {
    fn set(&mut self, on: bool) -> io::Result<()> {
        self.f.write_all(&cm108_report(self.gpio, on))
    }

    fn describe(&self) -> String {
        format!("CM108 GPIO{} on {}", self.gpio, self.path)
    }
}

pub struct Rigctld {
    link: Option<(TcpStream, BufReader<TcpStream>)>,
    addr: String,
}

impl Rigctld {
    pub fn open(addr: &str) -> io::Result<Self> {
        let mut c = Self { link: Some(Self::connect(addr)?), addr: addr.into() };
        c.set(false)?;
        Ok(c)
    }

    fn connect(addr: &str) -> io::Result<(TcpStream, BufReader<TcpStream>)> {
        let a = addr.to_socket_addrs()?.next().ok_or_else(|| io::Error::other(format!("{addr}: no address")))?;
        let w = TcpStream::connect_timeout(&a, Duration::from_secs(2))?;
        w.set_read_timeout(Some(Duration::from_secs(1)))?;
        w.set_write_timeout(Some(Duration::from_secs(1)))?;
        w.set_nodelay(true)?;
        Ok((w.try_clone()?, BufReader::new(w)))
    }

    fn exchange(&mut self, cmd: &[u8]) -> io::Result<String> {
        let (w, r) = match &mut self.link {
            Some(l) => l,
            None => self.link.insert(Self::connect(&self.addr)?),
        };
        w.write_all(cmd)?;
        let mut l = String::new();
        if r.read_line(&mut l)? == 0 || !l.ends_with('\n') {
            return Err(io::Error::other(format!("rigctld {} closed", self.addr)));
        }
        Ok(l)
    }

    /// Tune the rig's dial to `hz` (rigctld `F`), e.g. once at startup with `--rig-freq`.
    pub fn set_frequency(&mut self, hz: u64) -> io::Result<()> {
        let l = self.exchange(format!("F {hz}\n").as_bytes()).inspect_err(|_| self.link = None)?;
        match l.trim() {
            "RPRT 0" => Ok(()),
            r => Err(io::Error::other(format!("rigctld {}: F {hz}: {r}", self.addr))),
        }
    }
}

impl Ptt for Rigctld {
    fn set(&mut self, on: bool) -> io::Result<()> {
        let l = self.exchange(if on { b"T 1\n" } else { b"T 0\n" }).inspect_err(|_| self.link = None)?;
        match l.trim() {
            "RPRT 0" => Ok(()),
            r => Err(io::Error::other(format!("rigctld {}: {r}", self.addr))),
        }
    }

    fn describe(&self) -> String {
        format!("rigctld at {}", self.addr)
    }
}

pub(crate) const DEVICE_NS: &str = r"\\.\";

/// `COM3` or `\\.\COM12`.
#[cfg_attr(not(windows), allow(dead_code))]
fn win_port(s: &str) -> bool {
    let s = s.strip_prefix(DEVICE_NS).unwrap_or(s);
    s.len() > 3 && s[..3].eq_ignore_ascii_case("com") && s[3..].bytes().all(|b| b.is_ascii_digit())
}

#[cfg(windows)]
mod win {
    use super::Ptt;
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::windows::io::AsRawHandle;

    pub const RTS: i32 = 1;
    pub const DTR: i32 = 2;
    const SETRTS: u32 = 3;
    const CLRRTS: u32 = 4;
    const SETDTR: u32 = 5;
    const CLRDTR: u32 = 6;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn EscapeCommFunction(h: *mut core::ffi::c_void, func: u32) -> i32;
    }

    pub struct Serial {
        f: File,
        path: String,
        line: i32,
    }

    impl Serial {
        pub fn open(path: &str, line: i32) -> io::Result<Self> {
            let dev = if path.starts_with(super::DEVICE_NS) {
                path.to_string()
            } else {
                format!("{}{path}", super::DEVICE_NS)
            };
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&dev)
                .map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;
            let s = Self { f, path: path.into(), line };
            s.escape(CLRRTS)?;
            s.escape(CLRDTR)?;
            Ok(s)
        }

        fn escape(&self, func: u32) -> io::Result<()> {
            // SAFETY: the handle is an open serial port for as long as self.f lives.
            if unsafe { EscapeCommFunction(self.f.as_raw_handle(), func) } == 0 {
                return Err(io::Error::other(format!("{}: {}", self.path, io::Error::last_os_error())));
            }
            Ok(())
        }
    }

    impl Ptt for Serial {
        fn set(&mut self, on: bool) -> io::Result<()> {
            self.escape(match (self.line == RTS, on) {
                (true, true) => SETRTS,
                (true, false) => CLRRTS,
                (false, true) => SETDTR,
                (false, false) => CLRDTR,
            })
        }

        fn describe(&self) -> String {
            format!("{} on {} (other line held low)", if self.line == RTS { "RTS" } else { "DTR" }, self.path)
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PttTiming {
    pub lead: Duration,
    pub tail: Duration,
    pub max_on: Duration,
    pub stall: Duration,
}

impl Default for PttTiming {
    fn default() -> Self {
        Self {
            lead: Duration::ZERO,
            tail: Duration::from_millis(100),
            max_on: Duration::from_secs(30),
            stall: Duration::from_millis(500),
        }
    }
}

struct Watched {
    id: u64,
    shutdown: Arc<AtomicBool>,
    released: Arc<AtomicBool>,
}

static WATCHED: Mutex<Vec<Watched>> = Mutex::new(Vec::new());
static RELEASED: Condvar = Condvar::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(0);
static PANICKED: AtomicBool = AtomicBool::new(false);
static HOOK: Once = Once::new();

thread_local! {
    static PTT_THREAD: Cell<bool> = const { Cell::new(false) };
}

fn watched() -> std::sync::MutexGuard<'static, Vec<Watched>> {
    crate::lock(&WATCHED)
}

pub fn panicked() -> bool {
    PANICKED.load(Ordering::SeqCst)
}

pub fn wait_released(timeout: Duration) -> bool {
    let held = |w: &mut Vec<Watched>| !w.iter().all(|w| w.released.load(Ordering::SeqCst));
    !RELEASED.wait_timeout_while(watched(), timeout, held).unwrap_or_else(PoisonError::into_inner).1.timed_out()
}

fn mark_released(released: &AtomicBool, on: bool) {
    released.store(on, Ordering::SeqCst);
    if on {
        drop(watched());
        RELEASED.notify_all();
    }
}

fn guard_panics(shutdown: Arc<AtomicBool>, released: Arc<AtomicBool>) -> u64 {
    HOOK.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PANICKED.store(true, Ordering::SeqCst);
            for w in watched().iter() {
                w.shutdown.store(true, Ordering::SeqCst);
            }
            if !PTT_THREAD.get() {
                wait_released(Duration::from_millis(500));
            }
            prev(info);
        }));
    });
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    watched().push(Watched { id, shutdown, released });
    id
}

pub enum Event {
    On,
    Off,
    Watchdog,
    Stalled,
}

pub struct PttCtl {
    tx: Option<Sender<bool>>,
    thread: Option<JoinHandle<io::Result<()>>>,
    shutdown: Arc<AtomicBool>,
    forced: Arc<AtomicBool>,
    watch: Option<u64>,
}

struct Keyer {
    dev: Box<dyn Ptt>,
    keyed: bool,
    released: Arc<AtomicBool>,
}

impl Keyer {
    fn set(&mut self, on: bool) -> io::Result<()> {
        self.released.store(false, Ordering::SeqCst);
        let r = self.dev.set(on);
        self.keyed = on || r.is_err();
        mark_released(&self.released, !self.keyed);
        r
    }

    fn release(&mut self) -> bool {
        for _ in 0..3 {
            if self.dev.release().is_ok() {
                self.keyed = false;
                mark_released(&self.released, true);
                return true;
            }
        }
        false
    }
}

impl Drop for Keyer {
    fn drop(&mut self) {
        if self.keyed {
            self.release();
        }
    }
}

impl PttCtl {
    pub fn spawn(dev: Box<dyn Ptt>, timing: PttTiming, shutdown: Arc<AtomicBool>) -> Self {
        let released = Arc::new(AtomicBool::new(true));
        let watch = guard_panics(shutdown.clone(), released.clone());
        let (ltx, lrx) = mpsc::channel::<String>();
        spawn("ptt-log", move || {
            for m in lrx {
                say!("{m}");
            }
        });
        let mut c = Self::run(dev, timing, shutdown, released, move |e| {
            let m = match e {
                Event::Watchdog => format!("PTT watchdog: keyed longer than {:?}, released", timing.max_on),
                Event::Stalled => "PTT released: audio loop stalled".into(),
                _ => return,
            };
            let _ = ltx.send(m);
        });
        c.watch = Some(watch);
        c
    }

    pub fn spawn_with(
        dev: Box<dyn Ptt>,
        t: PttTiming,
        shutdown: Arc<AtomicBool>,
        log: impl FnMut(Event) + Send + 'static,
    ) -> Self {
        Self::run(dev, t, shutdown, Arc::new(AtomicBool::new(true)), log)
    }

    fn run(
        dev: Box<dyn Ptt>,
        t: PttTiming,
        shutdown: Arc<AtomicBool>,
        released: Arc<AtomicBool>,
        mut log: impl FnMut(Event) + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<bool>();
        let (stop, forced) = (shutdown.clone(), Arc::new(AtomicBool::new(false)));
        let forced_off = forced.clone();
        let thread = spawn("ptt", move || {
            PTT_THREAD.set(true);
            let mut k = Keyer { dev, keyed: false, released };
            let (mut want, mut tripped, mut stalled) = (false, false, false);
            let (mut off_at, mut on_since, mut last) = (None::<Instant>, Instant::now(), Instant::now());
            let mut fault = None;
            loop {
                let msg = match k.keyed {
                    true => rx.recv_timeout(Duration::from_millis(2)),
                    false => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                };
                match msg {
                    Ok(w) => {
                        last = Instant::now();
                        stalled = false;
                        if w != want {
                            want = w;
                            off_at = (!w && k.keyed).then(|| last + t.tail);
                            tripped &= w;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let now = Instant::now();
                let stall = k.keyed && !stalled && now - last > t.stall;
                let trip = k.keyed && !tripped && now - on_since > t.max_on;
                stalled |= stall;
                tripped |= trip;
                forced_off.store(tripped || stalled, Ordering::SeqCst);
                let target = !tripped && !stalled && (want || off_at.is_some_and(|o| now < o));
                let change = target != k.keyed;
                if change {
                    if let Err(e) = k.set(target) {
                        fault = Some(e);
                        break;
                    }
                    if k.keyed {
                        on_since = now;
                    }
                }
                if stall {
                    log(Event::Stalled);
                }
                if trip {
                    log(Event::Watchdog);
                }
                if change {
                    log(if k.keyed { Event::On } else { Event::Off });
                }
            }
            drop(rx);
            let keyed = k.keyed;
            if !keyed {
                let _ = k.dev.release();
            }
            let released = !keyed || k.release();
            k.keyed = false;
            if keyed {
                log(Event::Off);
            }
            let state = if released { "released" } else { "COULD NOT RELEASE PTT" };
            match fault {
                Some(e) => Err(io::Error::new(e.kind(), format!("PTT {} failed: {e}; {state}", k.dev.describe()))),
                None if !released => Err(io::Error::other(format!("COULD NOT RELEASE PTT on {}", k.dev.describe()))),
                None => Ok(()),
            }
        });
        Self { tx: Some(tx), thread: Some(thread), shutdown, forced, watch: None }
    }

    pub fn set(&mut self, on: bool) -> io::Result<()> {
        if self.tx.as_ref().is_some_and(|tx| tx.send(on).is_ok()) {
            return Ok(());
        }
        self.finish()?;
        if self.shutdown.load(Ordering::SeqCst) { Ok(()) } else { Err(io::Error::other("PTT thread stopped")) }
    }

    pub fn forced_off(&self) -> bool {
        self.forced.load(Ordering::SeqCst)
    }

    pub fn finish(&mut self) -> io::Result<()> {
        self.tx.take();
        let r = match self.thread.take().map(JoinHandle::join) {
            Some(Ok(r)) => r,
            Some(Err(_)) => Err(io::Error::other("PTT thread panicked")),
            None => Ok(()),
        };
        if let Some(id) = self.watch.take() {
            watched().retain(|w| w.id != id);
        }
        r
    }
}

fn spawn<T: Send + 'static>(name: &str, f: impl FnOnce() -> T + Send + 'static) -> JoinHandle<T> {
    thread::Builder::new().name(name.into()).spawn(f).expect("spawn PTT thread")
}

impl Drop for PttCtl {
    fn drop(&mut self) {
        if let Err(e) = self.finish() {
            say!("{e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Stuck;

    impl Ptt for Stuck {
        fn set(&mut self, on: bool) -> io::Result<()> {
            if on { Ok(()) } else { Err(io::Error::other("stuck")) }
        }

        fn describe(&self) -> String {
            "stuck".into()
        }
    }

    #[test]
    fn the_panic_hook_is_installed_once_and_releases_every_controller() {
        let stops: Vec<Arc<AtomicBool>> = (0..4).map(|_| Arc::default()).collect();
        let mut c: Vec<PttCtl> =
            stops.iter().map(|s| PttCtl::spawn(Box::new(Stuck), PttTiming::default(), s.clone())).collect();
        for p in &mut c {
            p.set(true).unwrap();
        }
        // Wait until every PTT thread has really keyed: a fixed sleep raced on loaded machines, and a
        // controller that never keyed has nothing to release, so finish() came back Ok.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !watched().iter().all(|w| !w.released.load(Ordering::SeqCst)) {
            assert!(Instant::now() < deadline, "PTT threads never keyed");
            thread::sleep(Duration::from_millis(1));
        }
        let t0 = Instant::now();
        assert!(thread::spawn(|| panic!("test panic")).join().is_err());
        let took = t0.elapsed();
        assert!(panicked());
        assert!(stops.iter().all(|s| s.load(Ordering::SeqCst)));
        assert!(took >= Duration::from_millis(450) && took < Duration::from_millis(1500), "{took:?}");
        for mut p in c {
            assert!(p.finish().unwrap_err().to_string().contains("COULD NOT RELEASE"));
        }
        assert!(watched().is_empty());
    }
}
