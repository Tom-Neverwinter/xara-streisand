// The TNC (this binary) uses Unix APIs throughout -- PTT over ioctl, signal
// trapping, Unix sockets -- so it is built only on Unix, from tnc_main.rs,
// unchanged.  Elsewhere (Windows) it says so; xara-monitor works there.
#[cfg(unix)]
include!("tnc_main.rs");

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!("xara: the TNC is Unix-only for now; on this platform use xara-monitor --wav FILE");
    std::process::ExitCode::FAILURE
}
