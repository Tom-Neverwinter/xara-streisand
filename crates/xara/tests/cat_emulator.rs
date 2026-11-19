//! Built-in CAT against emulated rigs: each protocol drives a rig model on the far side of a pseudo-terminal,
//! through the same serial setup a USB CAT port gets. The rigs follow the makers' CAT manuals: Icom CI-V (with
//! the USB echo on), Kenwood/Elecraft ASCII, newer Yaesu ASCII, and the FT-817's 5-byte commands.
#![cfg(unix)]
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::sync::{Arc, Mutex};
use xara::cat::{Cat, CatSpec};

#[derive(Default, Debug)]
struct Rig {
    hz: u64,
    ptt: bool,
}

type Shared = Arc<Mutex<Rig>>;

/// The controlling side of a pseudo-terminal and the path of its serial end.
fn pty() -> (File, String) {
    // SAFETY: plain libc pty calls; the name is copied before anything else can reuse ptsname's buffer.
    unsafe {
        let m = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(m >= 0, "posix_openpt");
        assert_eq!(libc::grantpt(m), 0);
        assert_eq!(libc::unlockpt(m), 0);
        let name = std::ffi::CStr::from_ptr(libc::ptsname(m)).to_string_lossy().into_owned();
        (File::from_raw_fd(m), name)
    }
}

fn bcd(b: u8) -> u64 {
    u64::from(b >> 4) * 10 + u64::from(b & 15)
}

/// What the rig sends back for the bytes it has received so far; `buf` keeps an incomplete command.
fn icom(buf: &mut Vec<u8>, rig: &mut Rig, addr: u8, refuse: bool) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(end) = buf.iter().position(|&b| b == 0xfd) {
        let f: Vec<u8> = buf.drain(..=end).collect();
        out.extend_from_slice(&f); // CI-V USB echo
        let ok = match f.as_slice() {
            [0xfe, 0xfe, to, 0xe0, 0x05, d @ .., 0xfd] if *to == addr && d.len() == 5 => {
                rig.hz = d.iter().rev().fold(0, |acc, &b| acc * 100 + bcd(b));
                true
            }
            [0xfe, 0xfe, to, 0xe0, 0x1c, 0x00, on, 0xfd] if *to == addr => {
                rig.ptt = *on == 1;
                true
            }
            [0xfe, 0xfe, to, ..] if *to == addr => false,
            _ => continue, // not for this rig: no answer
        };
        out.extend_from_slice(&[0xfe, 0xfe, 0xe0, addr, if ok && !refuse { 0xfb } else { 0xfa }, 0xfd]);
    }
    out
}

fn ascii(buf: &mut Vec<u8>, rig: &mut Rig, digits: usize, tx: (&str, &str)) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(end) = buf.iter().position(|&b| b == b';') {
        let c = String::from_utf8(buf.drain(..=end).collect()).unwrap();
        let c = c.trim_end_matches(';');
        if c == "FA" {
            out.extend(format!("FA{:0digits$};", rig.hz).bytes());
        } else if let Some(f) = c.strip_prefix("FA").filter(|f| f.len() == digits) {
            rig.hz = f.parse().unwrap();
        } else if c == tx.0 {
            rig.ptt = true;
        } else if c == tx.1 {
            rig.ptt = false;
        } else {
            out.extend(b"?;");
        }
    }
    out
}

fn yaesu5(buf: &mut Vec<u8>, rig: &mut Rig) -> Vec<u8> {
    let mut out = Vec::new();
    while buf.len() >= 5 {
        let c: Vec<u8> = buf.drain(..5).collect();
        match c[4] {
            0x01 => rig.hz = c[..4].iter().fold(0, |acc, &b| acc * 100 + bcd(b)) * 10,
            0x08 => {
                out.push(if rig.ptt { 0xf0 } else { 0x00 });
                rig.ptt = true;
            }
            0x88 => {
                out.push(if rig.ptt { 0x00 } else { 0xf0 });
                rig.ptt = false;
            }
            _ => {}
        }
    }
    out
}

/// Opens `kind` CAT on a fresh pty with a rig model behind it.
fn rig(kind: &str, extra: &str, refuse: bool) -> (Cat, Shared) {
    let (mut m, path) = pty();
    let spec = CatSpec::parse(&format!("{kind}:{path}{extra}")).unwrap();
    let addr = spec.addr;
    let cat = spec.open().unwrap_or_else(|e| panic!("open {path}: {e}"));
    let state: Shared = Arc::default();
    let s = state.clone();
    let kind = kind.to_string();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut b = [0u8; 256];
        // ends when the CAT side closes (EIO on the controlling side)
        while let Ok(n @ 1..) = m.read(&mut b) {
            buf.extend_from_slice(&b[..n]);
            let mut rig = s.lock().unwrap();
            let out = match kind.as_str() {
                "icom" => icom(&mut buf, &mut rig, addr, refuse),
                "kenwood" => ascii(&mut buf, &mut rig, 11, ("TX", "RX")),
                "yaesu" => ascii(&mut buf, &mut rig, 9, ("TX1", "TX0")),
                _ => yaesu5(&mut buf, &mut rig),
            };
            drop(rig);
            if !out.is_empty() && m.write_all(&out).is_err() {
                return;
            }
        }
    });
    (cat, state)
}

#[test]
fn every_protocol_tunes_and_keys_its_rig() {
    for (kind, extra) in [("icom", ""), ("icom", ":19200:a4"), ("kenwood", ""), ("yaesu", ""), ("yaesu5", "")] {
        let (mut cat, rig) = rig(kind, extra, false);
        for hz in [14_105_000, 7_078_000, 145_050_000] {
            cat.set_frequency(hz).unwrap_or_else(|e| panic!("{kind}{extra}: {e}"));
            // the 5-byte command has no reply; give the rig model a moment to act on it
            std::thread::sleep(std::time::Duration::from_millis(50));
            assert_eq!(rig.lock().unwrap().hz, hz, "{kind}{extra}");
        }
        for on in [true, false, true, false] {
            cat.set_ptt(on).unwrap_or_else(|e| panic!("{kind}{extra} ptt {on}: {e}"));
            std::thread::sleep(std::time::Duration::from_millis(50));
            assert_eq!(rig.lock().unwrap().ptt, on, "{kind}{extra}");
        }
    }
}

#[test]
fn an_icom_that_refuses_is_an_error() {
    let (mut cat, _) = rig("icom", "", true);
    let e = cat.set_ptt(true).unwrap_err().to_string();
    assert!(e.contains("refused"), "{e}");
}

#[test]
fn a_rig_on_another_ci_v_address_is_no_reply() {
    // xara addresses an IC-705 (a4) while the rig model answers only to the default 94
    let (mut m, path) = pty();
    let mut cat = CatSpec::parse(&format!("icom:{path}:19200:a4")).unwrap().open().unwrap();
    std::thread::spawn(move || {
        let (mut buf, mut rig, mut b) = (Vec::new(), Rig::default(), [0u8; 64]);
        while let Ok(n @ 1..) = m.read(&mut b) {
            buf.extend_from_slice(&b[..n]);
            let out = icom(&mut buf, &mut rig, 0x94, false);
            if !out.is_empty() && m.write_all(&out).is_err() {
                return;
            }
        }
    });
    let e = cat.set_frequency(14_105_000).unwrap_err().to_string();
    assert!(e.contains("no reply"), "{e}");
}
