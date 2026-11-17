//! Built-in CAT rig control: PTT and dial frequency over the rig's serial (or USB-serial) CAT port, without
//! hamlib.  Three protocol families cover most HF rigs, each from its maker's published CAT manual:
//!
//! - `icom`: CI-V binary frames (`FE FE <rig> E0 <cmd> .. FD`), e.g. IC-7300, IC-705, IC-7610, IC-9700;
//! - `kenwood`: ASCII `FA00014105000;` / `TX;` / `RX;`, e.g. Kenwood, Elecraft, QRP Labs QMX, (tr)uSDX;
//! - `yaesu`: the newer Yaesu ASCII dialect (`FA014105000;`, `TX1;` / `TX0;`), e.g. FT-991A, FT-710, FTDX10;
//! - `yaesu5`: the classic 5-byte binary commands, e.g. FT-817, FT-818, FT-857, FT-897.
//!
//! `--cat KIND:PORT[:BAUD[:ADDR]]` names the rig; `--ptt cat` keys through it and `--rig-freq` tunes it.
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use crate::ptt::Ptt;
use crate::serial::{self, Port};

const REPLY: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Icom,
    Kenwood,
    Yaesu,
    Yaesu5,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatSpec {
    pub kind: Kind,
    pub port: String,
    pub baud: u32,
    /// CI-V address of the rig (Icom only).
    pub addr: u8,
}

impl Kind {
    fn default_baud(self) -> u32 {
        match self {
            Self::Icom => 19200,
            Self::Kenwood | Self::Yaesu => 38400,
            Self::Yaesu5 => 9600,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Icom => "icom",
            Self::Kenwood => "kenwood",
            Self::Yaesu => "yaesu",
            Self::Yaesu5 => "yaesu5",
        }
    }
}

/// CI-V address of the IC-7300; others: IC-705 a4, IC-7610 98, IC-9700 a2, IC-7100 88, IC-7851 8e.
pub const ICOM_DEFAULT_ADDR: u8 = 0x94;

impl CatSpec {
    pub fn parse(s: &str) -> Result<Self, String> {
        let usage = "--cat: expected icom|kenwood|yaesu|yaesu5:PORT[:BAUD[:CIV-ADDR]], e.g. icom:COM4:19200:94";
        let (kind, rest) = s.split_once(':').ok_or(usage)?;
        let kind = match kind {
            "icom" => Kind::Icom,
            "kenwood" | "elecraft" => Kind::Kenwood,
            "yaesu" => Kind::Yaesu,
            "yaesu5" => Kind::Yaesu5,
            _ => return Err(usage.into()),
        };
        // A Windows port can be \\.\COM12, which has no ':'; a Unix one is a path.  Split options off the end.
        let mut parts: Vec<&str> = rest.split(':').collect();
        let addr = if kind == Kind::Icom && parts.len() == 3 {
            let a = parts.pop().unwrap();
            u8::from_str_radix(a.trim_start_matches("0x"), 16)
                .map_err(|_| format!("--cat: CI-V address {a:?} is not hex"))?
        } else {
            ICOM_DEFAULT_ADDR
        };
        let baud = match parts.len() {
            2 => parts.pop().unwrap().parse().map_err(|_| format!("--cat: bad baud rate in {s:?}"))?,
            1 => kind.default_baud(),
            _ => return Err(usage.into()),
        };
        let port = parts[0].to_string();
        if port.is_empty() {
            return Err(usage.into());
        }
        if !serial::BAUDS.contains(&baud) {
            return Err(format!("--cat: baud {baud} not supported; use one of {:?}", serial::BAUDS));
        }
        Ok(Self { kind, port, baud, addr })
    }

    pub fn open(&self) -> io::Result<Cat> {
        let stop_bits = if self.kind == Kind::Yaesu5 { 2 } else { 1 };
        let port = serial::open(&self.port, self.baud, stop_bits)?;
        Ok(Cat { spec: self.clone(), port })
    }
}

pub struct Cat {
    spec: CatSpec,
    port: Port,
}

impl Cat {
    fn send(&mut self, b: &[u8]) -> io::Result<()> {
        self.port.flush_input();
        self.port.write_all(b)
    }

    /// Read until `done(buf)` or the reply timeout.
    fn read_until(&mut self, done: impl Fn(&[u8]) -> bool) -> io::Result<Vec<u8>> {
        let end = Instant::now() + REPLY;
        let mut buf = Vec::new();
        let mut b = [0u8; 64];
        while !done(&buf) && Instant::now() < end {
            match self.port.read(&mut b) {
                Ok(0) => {}
                Ok(n) => buf.extend_from_slice(&b[..n]),
                Err(e) if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
        }
        Ok(buf)
    }

    fn err(&self, what: &str) -> io::Error {
        io::Error::other(format!("{} CAT on {}: {what}", self.spec.kind.name(), self.spec.port))
    }

    /// A CI-V command; waits for the rig's FB (ok) or FA (refused), skipping the echo of our own frame.
    fn civ(&mut self, cmd: &[u8]) -> io::Result<()> {
        let frame = civ_frame(self.spec.addr, cmd);
        self.send(&frame)?;
        let rig = self.spec.addr;
        let reply = self.read_until(|b| civ_reply(b, rig).is_some())?;
        match civ_reply(&reply, rig) {
            Some(true) => Ok(()),
            Some(false) => Err(self.err(&format!("rig refused {cmd:02x?}"))),
            None => Err(self.err("no reply (check baud, CI-V address and CI-V USB echo/transceive settings)")),
        }
    }

    pub fn set_frequency(&mut self, hz: u64) -> io::Result<()> {
        match self.spec.kind {
            Kind::Icom => self.civ(&[&[0x05][..], &icom_bcd(hz)].concat()),
            Kind::Kenwood | Kind::Yaesu => {
                let digits = if self.spec.kind == Kind::Kenwood { 11 } else { 9 };
                self.send(format!("FA{hz:0digits$};").as_bytes())?;
                self.send(b"FA;")?;
                let r = self.read_until(|b| b.ends_with(b";"))?;
                match ascii_freq(&r) {
                    Some(got) if got == hz => Ok(()),
                    Some(got) => Err(self.err(&format!("asked for {hz} Hz, rig is on {got} Hz"))),
                    None => Err(self.err(&format!("no FA reply ({:?})", String::from_utf8_lossy(&r)))),
                }
            }
            Kind::Yaesu5 => self.send(&yaesu5_freq(hz)),
        }
    }

    pub fn set_ptt(&mut self, on: bool) -> io::Result<()> {
        match self.spec.kind {
            Kind::Icom => self.civ(&[0x1c, 0x00, u8::from(on)]),
            Kind::Kenwood => self.send(if on { b"TX;" } else { b"RX;" }),
            Kind::Yaesu => self.send(if on { b"TX1;" } else { b"TX0;" }),
            Kind::Yaesu5 => {
                self.send(&[0, 0, 0, 0, if on { 0x08 } else { 0x88 }])?;
                // the rig answers one status byte; it carries nothing we need
                self.read_until(|b| !b.is_empty()).map(drop)
            }
        }
    }

    pub fn describe(&self) -> String {
        let addr = if self.spec.kind == Kind::Icom { format!(" CI-V {:02x}", self.spec.addr) } else { String::new() };
        format!("{} CAT on {} at {} baud{addr}", self.spec.kind.name(), self.spec.port, self.spec.baud)
    }
}

impl Ptt for Cat {
    fn set(&mut self, on: bool) -> io::Result<()> {
        self.set_ptt(on)
    }

    fn describe(&self) -> String {
        Cat::describe(self)
    }
}

/// FE FE <to> E0 <cmd..> FD, E0 being the controller's conventional address.
fn civ_frame(rig: u8, cmd: &[u8]) -> Vec<u8> {
    [&[0xfe, 0xfe, rig, 0xe0][..], cmd, &[0xfd]].concat()
}

/// Some(true) for an FB (ok) frame from `rig` to us, Some(false) for FA (no good), None if neither yet.
fn civ_reply(b: &[u8], rig: u8) -> Option<bool> {
    b.split(|&c| c == 0xfd).find_map(|f| {
        let f = &f[f.iter().position(|&c| c != 0xfe)?..];
        match f {
            [0xe0, from, 0xfb] if *from == rig => Some(true),
            [0xe0, from, 0xfa] if *from == rig => Some(false),
            _ => None,
        }
    })
}

/// Icom frequency: 10 BCD digits of Hz, least significant byte first.
fn icom_bcd(hz: u64) -> [u8; 5] {
    let mut out = [0u8; 5];
    let mut v = hz;
    for b in &mut out {
        *b = (v % 10) as u8 | (((v / 10) % 10) as u8) << 4;
        v /= 100;
    }
    out
}

/// Yaesu 5-byte "set frequency": 8 BCD digits of 10 Hz units, most significant first, then opcode 01.
fn yaesu5_freq(hz: u64) -> [u8; 5] {
    let mut v = hz / 10;
    let mut d = [0u8; 4];
    for b in d.iter_mut().rev() {
        *b = (v % 10) as u8 | (((v / 10) % 10) as u8) << 4;
        v /= 100;
    }
    [d[0], d[1], d[2], d[3], 0x01]
}

/// The Hz in the last `FA<digits>;` of a reply.
fn ascii_freq(r: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(r).ok()?;
    let f = s.rsplit("FA").next()?.strip_suffix(';')?;
    (!f.is_empty() && f.bytes().all(|b| b.is_ascii_digit())).then(|| f.parse().ok())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_parse() {
        let p = |s| CatSpec::parse(s);
        assert_eq!(p("icom:COM4"), Ok(CatSpec { kind: Kind::Icom, port: "COM4".into(), baud: 19200, addr: 0x94 }));
        assert_eq!(p("icom:/dev/ttyUSB0:115200:a4").unwrap().addr, 0xa4);
        assert_eq!(p("kenwood:/dev/ttyUSB0").unwrap().baud, 38400);
        assert_eq!(p("yaesu5:COM3:4800").unwrap().baud, 4800);
        assert_eq!(p("elecraft:COM7:9600").unwrap().kind, Kind::Kenwood);
        for bad in ["icom", "icom:", "icom:COM4:fast", "icom:COM4:19200:zz", "kenwood:COM1:1234", "ft8:COM1"] {
            assert!(p(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn icom_frames() {
        assert_eq!(icom_bcd(14_105_000), [0x00, 0x50, 0x10, 0x14, 0x00]);
        assert_eq!(icom_bcd(145_050_000), [0x00, 0x00, 0x05, 0x45, 0x01]);
        assert_eq!(civ_frame(0x94, &[0x1c, 0x00, 0x01]), [0xfe, 0xfe, 0x94, 0xe0, 0x1c, 0x00, 0x01, 0xfd]);
        let echo = civ_frame(0x94, &[0x1c, 0x00, 0x01]);
        let ok = [0xfe, 0xfe, 0xe0, 0x94, 0xfb, 0xfd];
        assert_eq!(civ_reply(&echo, 0x94), None);
        assert_eq!(civ_reply(&[echo.as_slice(), &ok].concat(), 0x94), Some(true));
        assert_eq!(civ_reply(&[0xfe, 0xfe, 0xe0, 0x94, 0xfa, 0xfd], 0x94), Some(false));
        assert_eq!(civ_reply(&ok, 0xa4), None); // another rig on the bus
    }

    #[test]
    fn yaesu_and_ascii() {
        assert_eq!(yaesu5_freq(14_105_000), [0x01, 0x41, 0x05, 0x00, 0x01]);
        assert_eq!(yaesu5_freq(7_101_700), [0x00, 0x71, 0x01, 0x70, 0x01]);
        assert_eq!(ascii_freq(b"FA00014105000;"), Some(14_105_000));
        assert_eq!(ascii_freq(b"?;FA014105000;"), Some(14_105_000));
        assert_eq!((ascii_freq(b"?;"), ascii_freq(b"FA;")), (None, None));
    }
}
