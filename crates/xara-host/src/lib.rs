// Linux pauses a listener with shutdown(); other platforms (macOS, BSD, Windows) use the bouncer.
#[cfg(any(windows, all(unix, any(not(target_os = "linux"), test))))]
mod bouncer;
pub mod command;
pub mod kiss;
pub mod response;
pub mod server;
pub mod writer;

use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;

/// Pause after a failed accept(); shared by the TNC and KISS listeners.
pub(crate) const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

const HTTP_METHODS: [&str; 9] = ["GET", "POST", "PUT", "HEAD", "OPTIONS", "DELETE", "PATCH", "CONNECT", "TRACE"];

/// Does `b` start with an HTTP request line, such as `POST / HTTP/1.1`?
///
/// Any web page the operator visits can make the browser send an HTTP
/// request to a port on 127.0.0.1 (`fetch(.., {mode: "no-cors"})`).  The
/// command, data and KISS ports read whatever arrives: the header lines are
/// answered WRONG or dropped, and a body such as `\rCQFRAME ..\r` or a KISS
/// frame then runs -- the page makes the station transmit.  No TNC, KISS or
/// Winlink client starts with a request line, so a connection that does is
/// closed before anything it sent is acted on.
pub fn http_request_line(b: &[u8]) -> bool {
    let end = b.iter().position(|&c| c == b'\r' || c == b'\n').unwrap_or(b.len());
    let Ok(line) = std::str::from_utf8(&b[..end]) else { return false };
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) = (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    HTTP_METHODS.contains(&method) && !target.is_empty() && version.starts_with("HTTP/")
}

fn refuse_http(port: &str) {
    say(format_args!("{port} port: closed a connection that began with an HTTP request (a web page?)"));
}

static LOG: OnceLock<fn(fmt::Arguments)> = OnceLock::new();

pub fn set_log(f: fn(fmt::Arguments)) {
    let _ = LOG.set(f);
}

fn say(args: fmt::Arguments) {
    match LOG.get() {
        Some(f) => f(args),
        None => eprintln!("xara: {args}"),
    }
}
