//! A serial port set up for CAT: raw 8 data bits, no parity, 1 or 2 stop bits, no flow control, RTS and DTR
//! held low (many interfaces key PTT on them), reads that time out after a short wait.
use std::fs::File;
use std::io::{self, Read, Write};

pub const BAUDS: [u32; 7] = [4800, 9600, 19200, 38400, 57600, 115_200, 1200];

pub struct Port {
    f: File,
}

impl Read for Port {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.f.read(b)
    }
}

impl Write for Port {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.f.write(b)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.f.flush()
    }
}

#[cfg(unix)]
pub fn open(path: &str, baud: u32, stop_bits: u8) -> io::Result<Port> {
    use std::os::fd::AsRawFd;
    let f = crate::ptt::open_tty(path)?;
    let fd = f.as_raw_fd();
    let speed = match baud {
        1200 => libc::B1200,
        4800 => libc::B4800,
        9600 => libc::B9600,
        19200 => libc::B19200,
        38400 => libc::B38400,
        57600 => libc::B57600,
        115_200 => libc::B115200,
        _ => return Err(io::Error::other(format!("{path}: baud {baud} not supported"))),
    };
    let fail = |what: &str| io::Error::other(format!("{path}: {what}: {}", io::Error::last_os_error()));
    // SAFETY: fd is open; t is a plain struct tcgetattr fills before anything reads it.
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut t) != 0 {
            return Err(fail("tcgetattr"));
        }
        libc::cfmakeraw(&mut t);
        libc::cfsetispeed(&mut t, speed);
        libc::cfsetospeed(&mut t, speed);
        t.c_cflag |= libc::CLOCAL | libc::CREAD;
        t.c_cflag &= !(libc::CRTSCTS | libc::HUPCL);
        if stop_bits == 2 {
            t.c_cflag |= libc::CSTOPB;
        } else {
            t.c_cflag &= !libc::CSTOPB;
        }
        t.c_cc[libc::VMIN] = 0;
        t.c_cc[libc::VTIME] = 1; // reads return after 0.1 s of silence
        if libc::tcsetattr(fd, libc::TCSANOW, &t) != 0 {
            return Err(fail("tcsetattr"));
        }
        // open_tty opened it non-blocking; VTIME does the waiting now
        let fl = libc::fcntl(fd, libc::F_GETFL);
        if fl < 0 || libc::fcntl(fd, libc::F_SETFL, fl & !libc::O_NONBLOCK) != 0 {
            return Err(fail("fcntl"));
        }
    }
    crate::ptt::modem(&f, libc::TIOCMBIC, libc::TIOCM_RTS | libc::TIOCM_DTR)?;
    Ok(Port { f })
}

#[cfg(unix)]
impl Port {
    /// Drop stale bytes (an unsolicited status line, an earlier echo) before a command.
    pub fn flush_input(&mut self) {
        use std::os::fd::AsRawFd;
        // SAFETY: the descriptor is open.
        unsafe { libc::tcflush(self.f.as_raw_fd(), libc::TCIFLUSH) };
    }
}

#[cfg(windows)]
#[allow(non_snake_case)]
mod win {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Default)]
    pub struct DCB {
        pub DCBlength: u32,
        pub BaudRate: u32,
        pub flags: u32,
        pub wReserved: u16,
        pub XonLim: u16,
        pub XoffLim: u16,
        pub ByteSize: u8,
        pub Parity: u8,
        pub StopBits: u8,
        pub XonChar: i8,
        pub XoffChar: i8,
        pub ErrorChar: i8,
        pub EofChar: i8,
        pub EvtChar: i8,
        pub wReserved1: u16,
    }

    #[repr(C)]
    pub struct COMMTIMEOUTS {
        pub ReadIntervalTimeout: u32,
        pub ReadTotalTimeoutMultiplier: u32,
        pub ReadTotalTimeoutConstant: u32,
        pub WriteTotalTimeoutMultiplier: u32,
        pub WriteTotalTimeoutConstant: u32,
    }

    pub const ONE_STOP: u8 = 0;
    pub const TWO_STOPS: u8 = 2;
    pub const PURGE_RXCLEAR: u32 = 0x0008;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GetCommState(h: *mut c_void, d: *mut DCB) -> i32;
        pub fn SetCommState(h: *mut c_void, d: *const DCB) -> i32;
        pub fn SetCommTimeouts(h: *mut c_void, t: *const COMMTIMEOUTS) -> i32;
        pub fn PurgeComm(h: *mut c_void, flags: u32) -> i32;
    }
}

#[cfg(windows)]
pub fn open(path: &str, baud: u32, stop_bits: u8) -> io::Result<Port> {
    use std::os::windows::io::AsRawHandle;
    use win::*;
    let dev = if path.starts_with(crate::ptt::DEVICE_NS) {
        path.to_string()
    } else {
        format!("{}{path}", crate::ptt::DEVICE_NS)
    };
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&dev)
        .map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;
    let h = f.as_raw_handle();
    let fail = |what: &str| io::Error::other(format!("{path}: {what}: {}", io::Error::last_os_error()));
    let mut d = DCB { DCBlength: size_of::<DCB>() as u32, ..DCB::default() };
    // SAFETY: h is an open comm handle and d is sized as the API requires.
    if unsafe { GetCommState(h, &mut d) } == 0 {
        return Err(fail("GetCommState"));
    }
    d.BaudRate = baud;
    // fBinary only: no parity check, no CTS/DSR/XON flow control, DTR and RTS control disabled (held low)
    d.flags = 1;
    d.ByteSize = 8;
    d.Parity = 0;
    d.StopBits = if stop_bits == 2 { TWO_STOPS } else { ONE_STOP };
    // SAFETY: as above.
    if unsafe { SetCommState(h, &d) } == 0 {
        return Err(fail("SetCommState"));
    }
    let t = COMMTIMEOUTS {
        ReadIntervalTimeout: 50,
        ReadTotalTimeoutMultiplier: 0,
        ReadTotalTimeoutConstant: 100,
        WriteTotalTimeoutMultiplier: 0,
        WriteTotalTimeoutConstant: 1000,
    };
    // SAFETY: as above.
    if unsafe { SetCommTimeouts(h, &t) } == 0 {
        return Err(fail("SetCommTimeouts"));
    }
    Ok(Port { f })
}

#[cfg(windows)]
impl Port {
    /// Drop stale bytes (an unsolicited status line, an earlier echo) before a command.
    pub fn flush_input(&mut self) {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: the handle is open.
        unsafe { win::PurgeComm(self.f.as_raw_handle(), win::PURGE_RXCLEAR) };
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn dcb_has_the_sdk_size() {
        assert_eq!(size_of::<super::win::DCB>(), 28);
    }
}
