// The TCP server and its listener pause are built on Unix sockets (libc
// poll, SO_LINGER, raw fds).  Off Unix (Windows) the crate offers only the
// protocol pieces -- parsing, responses, KISS, the writer -- which is what
// xara-monitor needs; the TNC itself is Unix-only for now.
#[cfg(all(unix, any(not(target_os = "linux"), test)))]
mod bouncer;
pub mod command;
pub mod kiss;
pub mod response;
#[cfg(unix)]
pub mod server;
pub mod writer;

use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;

/// Pause after a failed accept(); shared by the TNC and KISS listeners.
pub(crate) const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

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
