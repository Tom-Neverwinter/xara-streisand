//! Telemetry feed and web UI server (HTTP, WebSocket, NDJSON) for the xara modem.
pub mod bus;
pub mod chat;
pub mod json;
pub mod messages;
// The web server is cross-platform: it binds a TcpListener and its access token
// comes from the OS CSPRNG (/dev/urandom on Unix, BCryptGenRandom on Windows),
// in an owner-only file on Unix / a per-user config dir on Windows. (settings::store
// still writes with a Unix mode, so it is gated; the settings types stay portable.)
pub mod server;
pub mod settings;
pub mod telemetry;
pub(crate) mod waterfall;
pub mod ws;
