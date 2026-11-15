// Off Unix the web server below is not built, so what only it uses is dead.
#![cfg_attr(not(unix), allow(dead_code))]

pub mod bus;
pub mod json;
pub mod messages;
// The web server's access token comes from /dev/urandom and is kept in a
// 0600 file.  Porting that safely off Unix needs a CSPRNG and Windows ACLs,
// so off Unix (Windows) there is no web UI: xara-monitor decodes to the
// terminal instead.  (settings::store, which writes with a Unix mode, is
// gated the same way; the settings types stay portable.)
#[cfg(unix)]
pub mod server;
pub mod settings;
pub mod telemetry;
pub(crate) mod waterfall;
pub mod ws;
