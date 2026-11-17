// Off Unix (Windows) PTT, the TNC and signal trapping are not built: they
// use Unix APIs (ioctl, termios, sigaction, Unix sockets), so a Windows build
// cannot key a radio.  What is left -- the station modem, the monitor, WAV
// and FLAC I/O -- is what `xara-monitor --wav` and the tests need.
#[macro_use]
pub mod log;
pub mod audio;
#[cfg(target_os = "macos")]
mod cf;
pub mod cli;
pub mod flac;
#[cfg(target_os = "macos")]
mod hid;
#[cfg(unix)]
mod hidraw;
pub mod monitor;
#[cfg(unix)]
pub mod ptt;
pub mod record;
pub mod resample;
#[cfg(unix)]
pub mod signals;
pub mod station;
#[cfg(unix)]
pub mod tnc;
pub mod wav;

pub const DEFAULT_ADDR: &str = "127.0.0.1";
pub const HF_SPAN_HZ: u32 = 3000;
pub const FM_SPAN_HZ: u32 = 6000;

#[cfg(target_os = "linux")]
pub(crate) type Ioctl = libc::Ioctl;
// Bionic's ioctl() takes an int request, and its TIOCM* constants are c_int.
#[cfg(target_os = "android")]
pub(crate) type Ioctl = libc::c_int;
#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
pub(crate) type Ioctl = libc::c_ulong;

pub(crate) fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
