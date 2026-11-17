use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::net::{ToSocketAddrs, UdpSocket};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static NAME: OnceLock<&'static str> = OnceLock::new();
static QUIET: AtomicBool = AtomicBool::new(false);
static LOGGING: AtomicBool = AtomicBool::new(false);
static FILE: Mutex<Option<File>> = Mutex::new(None);
static SYSLOG: OnceLock<Syslog> = OnceLock::new();

/// Log lines also go to a syslog server over UDP, as VARA's "Enable SysLog" does: RFC 5424, facility user,
/// severity info, the station name as HOSTNAME.
struct Syslog {
    sock: UdpSocket,
    station: String,
}

pub const SYSLOG_PORT: u16 = 514;

/// `addr` is HOST or HOST:PORT (default port 514); `station` names this station in each message.
pub fn syslog(addr: &str, station: &str) -> io::Result<()> {
    let with_port = if addr.rsplit_once(':').is_some_and(|(_, p)| p.parse::<u16>().is_ok()) && !addr.ends_with(']') {
        addr.to_string()
    } else {
        format!("{addr}:{SYSLOG_PORT}")
    };
    let to = with_port.to_socket_addrs()?.next().ok_or_else(|| io::Error::other(format!("{addr}: no address")))?;
    let sock = UdpSocket::bind(if to.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" })?;
    sock.connect(to)?;
    // RFC 5424 HOSTNAME: printable ASCII, no spaces, at most 255
    let station: String = station.chars().filter(|c| c.is_ascii_graphic()).take(255).collect();
    let station = if station.is_empty() { "-".into() } else { station };
    SYSLOG.set(Syslog { sock, station }).map_err(|_| io::Error::other("syslog already set"))
}

fn syslog_line(at: SystemTime, args: &fmt::Arguments) {
    if let Some(s) = SYSLOG.get() {
        // <14> = facility user (1) * 8 + severity info (6); best effort, a lost datagram is fine
        let _ = s.sock.send(format!("<14>1 {} {} {} - - - {args}", stamp(at), s.station, name()).as_bytes());
    }
}

pub fn init(name: &'static str, quiet: bool) {
    let _ = NAME.set(name);
    QUIET.store(quiet, Ordering::Relaxed);
    xara_host::set_log(say);
}

pub fn open(path: &Path) -> io::Result<()> {
    let f = OpenOptions::new().create(true).append(true).open(path)?;
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    LOGGING.store(true, Ordering::Relaxed);
    Ok(())
}

pub fn to_file() -> bool {
    LOGGING.load(Ordering::Relaxed)
}

pub fn line(console: bool, args: fmt::Arguments) {
    if console {
        let _ = writeln!(io::stderr().lock(), "{args}");
    }
    syslog_line(SystemTime::now(), &args);
    if !to_file() {
        return;
    }
    if let Some(f) = FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = f.write_all(format!("{} {args}\n", stamp(SystemTime::now())).as_bytes());
    }
}

fn name() -> &'static str {
    NAME.get().copied().unwrap_or("xara")
}

pub fn write(info: bool, args: fmt::Arguments) {
    let console = !(info && QUIET.load(Ordering::Relaxed));
    line(console, format_args!("{}: {args}", name()));
}

pub fn note(args: fmt::Arguments) {
    if to_file() || SYSLOG.get().is_some() {
        line(false, format_args!("{}: {args}", name()));
    }
}

pub fn console(args: fmt::Arguments) {
    let _ = writeln!(io::stderr().lock(), "{}: {args}", name());
}

fn say(args: fmt::Arguments) {
    write(false, args);
}

fn stamp(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let (days, s) = (d.as_secs() / 86400, d.as_secs() % 86400);
    let (y, m, day) = civil(days);
    format!("{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", s / 3600, s / 60 % 60, s % 60, d.subsec_millis())
}

fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let (era, doe) = (z / 146_097, z % 146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (era * 400 + yoe + u64::from(m <= 2), m, doy - (153 * mp + 2) / 5 + 1)
}

#[macro_export]
macro_rules! say {
    ($($a:tt)*) => {
        $crate::log::write(false, format_args!($($a)*))
    };
}

#[macro_export]
macro_rules! info {
    ($($a:tt)*) => {
        $crate::log::write(true, format_args!($($a)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64, ms: u64) -> String {
        stamp(UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_millis(ms))
    }

    #[test]
    fn syslog_sends_rfc5424_lines() {
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        syslog(&rx.local_addr().unwrap().to_string(), "N0 CALL").unwrap();
        line(false, format_args!("xara: hello"));
        let mut b = [0u8; 256];
        let n = rx.recv(&mut b).unwrap();
        let s = String::from_utf8_lossy(&b[..n]);
        assert!(s.starts_with("<14>1 ") && s.ends_with(" N0CALL xara - - - xara: hello"), "{s}");
    }

    #[test]
    fn stamps_are_utc_iso_with_milliseconds() {
        assert_eq!(at(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(at(1_709_210_096, 789), "2024-02-29T12:34:56.789Z");
        assert_eq!(at(4_107_542_400, 5), "2100-03-01T00:00:00.005Z");
        assert_eq!(at(4_107_542_399, 999), "2100-02-28T23:59:59.999Z");
    }
}
