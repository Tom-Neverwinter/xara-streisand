//! Windows audio through WinMM (waveIn/waveOut), the API VARA itself uses: 48 kHz mono 16-bit, event callbacks.
//! Devices are named as Windows lists them (Sound settings / `xara --list-audio`); WinMM truncates names to 31
//! characters, so a longer name matches its first 31.  Without --radio, playback is refused unless the device
//! looks like a virtual cable, as the Unix backends refuse anything but a null sink.
use std::collections::VecDeque;
use std::io;
use std::time::Instant;

use super::ratematch::{Queue, TxBuffer};
use super::{Audio, BLOCK, DeviceList, IO_TIMEOUT, Path, RATE, Report, capture_only, remaining};

#[allow(non_snake_case, clippy::upper_case_acronyms)]
mod ffi {
    #[repr(C, packed(1))]
    pub struct WAVEFORMATEX {
        pub wFormatTag: u16,
        pub nChannels: u16,
        pub nSamplesPerSec: u32,
        pub nAvgBytesPerSec: u32,
        pub nBlockAlign: u16,
        pub wBitsPerSample: u16,
        pub cbSize: u16,
    }

    #[repr(C)]
    pub struct WAVEHDR {
        pub lpData: *mut u8,
        pub dwBufferLength: u32,
        pub dwBytesRecorded: u32,
        pub dwUser: usize,
        pub dwFlags: u32,
        pub dwLoops: u32,
        pub lpNext: *mut WAVEHDR,
        pub reserved: usize,
    }

    #[repr(C)]
    pub struct WAVEINCAPSW {
        pub wMid: u16,
        pub wPid: u16,
        pub vDriverVersion: u32,
        pub szPname: [u16; 32],
        pub dwFormats: u32,
        pub wChannels: u16,
        pub wReserved1: u16,
    }

    #[repr(C)]
    pub struct WAVEOUTCAPSW {
        pub wMid: u16,
        pub wPid: u16,
        pub vDriverVersion: u32,
        pub szPname: [u16; 32],
        pub dwFormats: u32,
        pub wChannels: u16,
        pub wReserved1: u16,
        pub dwSupport: u32,
    }

    #[repr(C, packed(1))]
    pub struct MMTIME {
        pub wType: u32,
        pub u: u32,
        pub pad: u32,
    }

    pub const WAVE_FORMAT_PCM: u16 = 1;
    pub const CALLBACK_EVENT: u32 = 0x0005_0000;
    pub const WAVE_MAPPER: u32 = u32::MAX;
    pub const WHDR_DONE: u32 = 1;
    pub const TIME_SAMPLES: u32 = 2;

    #[link(name = "winmm")]
    unsafe extern "system" {
        pub fn waveInGetNumDevs() -> u32;
        pub fn waveInGetDevCapsW(id: usize, caps: *mut WAVEINCAPSW, size: u32) -> u32;
        pub fn waveInOpen(h: *mut isize, id: u32, fmt: *const WAVEFORMATEX, cb: usize, inst: usize, flags: u32) -> u32;
        pub fn waveInPrepareHeader(h: isize, hdr: *mut WAVEHDR, size: u32) -> u32;
        pub fn waveInUnprepareHeader(h: isize, hdr: *mut WAVEHDR, size: u32) -> u32;
        pub fn waveInAddBuffer(h: isize, hdr: *mut WAVEHDR, size: u32) -> u32;
        pub fn waveInStart(h: isize) -> u32;
        pub fn waveInReset(h: isize) -> u32;
        pub fn waveInClose(h: isize) -> u32;
        pub fn waveOutGetNumDevs() -> u32;
        pub fn waveOutGetDevCapsW(id: usize, caps: *mut WAVEOUTCAPSW, size: u32) -> u32;
        pub fn waveOutOpen(h: *mut isize, id: u32, fmt: *const WAVEFORMATEX, cb: usize, inst: usize, flags: u32) -> u32;
        pub fn waveOutPrepareHeader(h: isize, hdr: *mut WAVEHDR, size: u32) -> u32;
        pub fn waveOutUnprepareHeader(h: isize, hdr: *mut WAVEHDR, size: u32) -> u32;
        pub fn waveOutWrite(h: isize, hdr: *mut WAVEHDR, size: u32) -> u32;
        pub fn waveOutGetPosition(h: isize, t: *mut MMTIME, size: u32) -> u32;
        pub fn waveOutReset(h: isize) -> u32;
        pub fn waveOutClose(h: isize) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn CreateEventW(attr: *const u8, manual: i32, initial: i32, name: *const u16) -> isize;
        pub fn WaitForSingleObject(h: isize, ms: u32) -> u32;
        pub fn CloseHandle(h: isize) -> i32;
    }
}
use ffi::*;

const HDR: u32 = size_of::<WAVEHDR>() as u32;
const CAPTURE_BUFFERS: usize = 32; // x 10 ms
const WAIT_MS: u32 = 20; // re-check between waits: completions that fire before a wait are not lost
const VIRTUAL: [&str; 7] = ["cable", "virtual", "vb-audio", "voicemod", "loopback", "steam streaming", "voicemeeter"];

fn mm(what: &str, rc: u32) -> io::Result<()> {
    if rc == 0 { Ok(()) } else { Err(io::Error::other(format!("{what}: WinMM error {rc}"))) }
}

fn format() -> WAVEFORMATEX {
    WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM,
        nChannels: 1,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * 2,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    }
}

fn name(w: &[u16; 32]) -> String {
    String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(32)])
}

fn inputs() -> Vec<String> {
    (0..unsafe { waveInGetNumDevs() } as usize)
        .filter_map(|i| {
            let mut c: WAVEINCAPSW = unsafe { std::mem::zeroed() };
            (unsafe { waveInGetDevCapsW(i, &mut c, size_of::<WAVEINCAPSW>() as u32) } == 0).then(|| name(&c.szPname))
        })
        .collect()
}

fn outputs() -> Vec<String> {
    (0..unsafe { waveOutGetNumDevs() } as usize)
        .filter_map(|i| {
            let mut c: WAVEOUTCAPSW = unsafe { std::mem::zeroed() };
            (unsafe { waveOutGetDevCapsW(i, &mut c, size_of::<WAVEOUTCAPSW>() as u32) } == 0).then(|| name(&c.szPname))
        })
        .collect()
}

pub fn list() -> io::Result<(DeviceList, DeviceList)> {
    let pair = |v: Vec<String>| v.into_iter().map(|n| (n, String::new())).collect();
    Ok((pair(inputs()), pair(outputs())))
}

fn is_default(name: &str) -> bool {
    name.is_empty() || name == "default"
}

/// The device index for `want` among `names`: exact, then a name WinMM cut to 31 characters, then a unique
/// case-insensitive substring.  "default" (or empty) is the Windows default device.
fn find(names: &[String], want: &str, what: &str) -> io::Result<u32> {
    if is_default(want) {
        return Ok(WAVE_MAPPER);
    }
    let lw = want.to_lowercase();
    let hit = names.iter().position(|n| n == want).or_else(|| {
        names.iter().position(|n| n.chars().count() == 31 && want.starts_with(n.as_str()))
    });
    let hit = hit.or_else(|| {
        let m: Vec<_> = names.iter().enumerate().filter(|(_, n)| n.to_lowercase().contains(&lw)).collect();
        (m.len() == 1).then(|| m[0].0)
    });
    hit.map(|i| i as u32)
        .ok_or_else(|| io::Error::other(format!("no single {what} device matching {want:?}; have {names:?}")))
}

fn check_playback(names: &[String], want: &str, radio: bool) -> io::Result<u32> {
    if radio && is_default(want) {
        return Err(io::Error::other("--radio needs the playback device named explicitly, not a default"));
    }
    let i = find(names, want, "playback")?;
    let n = if i == WAVE_MAPPER { "" } else { names[i as usize].as_str() };
    if !radio && !VIRTUAL.iter().any(|v| n.to_lowercase().contains(v)) {
        return Err(io::Error::other(format!(
            "refusing playback to {want:?}: not a virtual audio cable (a real radio interface needs --radio)"
        )));
    }
    Ok(i)
}

pub fn check(playback: &str, radio: bool) -> io::Result<()> {
    check_playback(&outputs(), playback, radio).map(drop)
}

struct Event(isize);

impl Event {
    fn new() -> io::Result<Self> {
        let h = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        if h == 0 { Err(io::Error::last_os_error()) } else { Ok(Self(h)) }
    }

    fn wait(&self, until: Instant, what: &str) -> io::Result<()> {
        remaining(until, what)?;
        unsafe { WaitForSingleObject(self.0, WAIT_MS) };
        Ok(())
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// A header the driver holds a pointer to: boxed so it never moves, with its samples.
struct Buf {
    hdr: Box<WAVEHDR>,
    data: Vec<i16>,
}

impl Buf {
    fn new(data: Vec<i16>) -> Self {
        let mut b = Self { hdr: Box::new(unsafe { std::mem::zeroed() }), data };
        b.hdr.lpData = b.data.as_mut_ptr().cast();
        b.hdr.dwBufferLength = (2 * b.data.len()) as u32;
        b
    }

    fn done(&self) -> bool {
        (unsafe { std::ptr::read_volatile(&raw const self.hdr.dwFlags) } & WHDR_DONE) != 0
    }
}

struct Capture {
    h: isize,
    ev: Event,
    bufs: VecDeque<Buf>, // in the order the driver fills them
}

impl Capture {
    fn open(want: &str) -> io::Result<Self> {
        let id = find(&inputs(), want, "capture")?;
        let ev = Event::new()?;
        let mut h = 0;
        mm(&format!("opening capture {want:?}"), unsafe { waveInOpen(&mut h, id, &format(), ev.0 as usize, 0, CALLBACK_EVENT) })?;
        let mut c = Self { h, ev, bufs: VecDeque::new() };
        for _ in 0..CAPTURE_BUFFERS {
            let mut b = Buf::new(vec![0; BLOCK]);
            c.queue(&mut b)?;
            c.bufs.push_back(b);
        }
        mm("starting capture", unsafe { waveInStart(c.h) })?;
        Ok(c)
    }

    fn queue(&self, b: &mut Buf) -> io::Result<()> {
        b.hdr.dwFlags = 0;
        mm("capture buffer", unsafe { waveInPrepareHeader(self.h, &mut *b.hdr, HDR) })?;
        mm("capture buffer", unsafe { waveInAddBuffer(self.h, &mut *b.hdr, HDR) })
    }

    /// Move every filled buffer, in order, onto `rx` and hand it back to the driver.
    fn collect(&mut self, rx: &mut VecDeque<i16>) -> io::Result<()> {
        while self.bufs.front().is_some_and(Buf::done) {
            let mut b = self.bufs.pop_front().unwrap();
            unsafe { waveInUnprepareHeader(self.h, &mut *b.hdr, HDR) };
            rx.extend(&b.data[..b.hdr.dwBytesRecorded as usize / 2]);
            self.queue(&mut b)?;
            self.bufs.push_back(b);
        }
        Ok(())
    }

    fn ready(&self) -> usize {
        self.bufs.iter().take_while(|b| b.done()).map(|b| b.hdr.dwBytesRecorded as usize / 2).sum()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        unsafe { waveInReset(self.h) };
        for b in &mut self.bufs {
            unsafe { waveInUnprepareHeader(self.h, &mut *b.hdr, HDR) };
        }
        unsafe { waveInClose(self.h) };
    }
}

struct Play {
    h: isize,
    ev: Event,
    name: String,
    cap: usize,
    sent: VecDeque<Buf>,
    written: u32, // samples, wrapping like the driver's position counter
    report: Report,
    underruns: u64,
}

impl Play {
    fn open(want: &str, radio: bool, tx: TxBuffer) -> io::Result<Self> {
        let names = outputs();
        let id = check_playback(&names, want, radio)?;
        let ev = Event::new()?;
        let mut h = 0;
        mm(&format!("opening playback {want:?}"), unsafe { waveOutOpen(&mut h, id, &format(), ev.0 as usize, 0, CALLBACK_EVENT) })?;
        Ok(Self { h, ev, name: want.into(), cap: tx.capacity(), sent: VecDeque::new(), written: 0, report: Report::default(), underruns: 0 })
    }

    fn reap(&mut self) {
        while self.sent.front().is_some_and(Buf::done) {
            let mut b = self.sent.pop_front().unwrap();
            unsafe { waveOutUnprepareHeader(self.h, &mut *b.hdr, HDR) };
        }
    }

    fn queued(&mut self) -> usize {
        let mut t = MMTIME { wType: TIME_SAMPLES, u: 0, pad: 0 };
        let pos = unsafe { waveOutGetPosition(self.h, &mut t, size_of::<MMTIME>() as u32) };
        self.reap();
        if pos != 0 || t.wType != TIME_SAMPLES {
            return self.sent.iter().map(|b| b.data.len()).sum();
        }
        self.written.wrapping_sub(t.u) as usize
    }
}

impl Drop for Play {
    fn drop(&mut self) {
        unsafe { waveOutReset(self.h) };
        for b in &mut self.sent {
            unsafe { waveOutUnprepareHeader(self.h, &mut *b.hdr, HDR) };
        }
        unsafe { waveOutClose(self.h) };
    }
}

pub struct WinmmIo {
    rec: Capture,
    play: Option<Play>,
    rx: VecDeque<i16>,
}

// The handles and the driver-owned headers are only touched from the thread that owns this value.
unsafe impl Send for WinmmIo {}

impl WinmmIo {
    pub fn open(capture: &str, playback: &str, radio: bool, tx: TxBuffer) -> io::Result<Self> {
        let play = Some(Play::open(playback, radio, tx)?);
        Ok(Self { rec: Capture::open(capture)?, play, rx: VecDeque::new() })
    }

    pub fn open_capture(capture: &str) -> io::Result<Self> {
        Ok(Self { rec: Capture::open(capture)?, play: None, rx: VecDeque::new() })
    }
}

impl Audio for WinmmIo {
    fn read(&mut self, x: &mut [i16]) -> io::Result<()> {
        let until = Instant::now() + IO_TIMEOUT;
        loop {
            self.rec.collect(&mut self.rx)?;
            if self.rx.len() >= x.len() {
                break;
            }
            self.rec.ev.wait(until, "capture (no audio)")?;
        }
        let n = x.len();
        for (v, s) in x.iter_mut().zip(self.rx.drain(..n)) {
            *v = s;
        }
        Ok(())
    }

    fn write(&mut self, x: &[i16]) -> io::Result<()> {
        let Some(p) = &mut self.play else { return Err(capture_only()) };
        let until = Instant::now() + IO_TIMEOUT;
        while p.queued() + x.len() > p.cap {
            p.ev.wait(until, "playback (device not draining)")?;
        }
        if p.sent.is_empty() && p.written != 0 {
            p.underruns += 1; // everything we sent had played out before this block arrived
        }
        let mut b = Buf::new(x.to_vec());
        mm("playback buffer", unsafe { waveOutPrepareHeader(p.h, &mut *b.hdr, HDR) })?;
        mm("playback write", unsafe { waveOutWrite(p.h, &mut *b.hdr, HDR) })?;
        p.written = p.written.wrapping_add(x.len() as u32);
        p.sent.push_back(b);
        Ok(())
    }

    fn queued(&mut self) -> io::Result<Option<Queue>> {
        let Some(p) = &mut self.play else { return Err(capture_only()) };
        let frames = p.queued();
        if p.report.due(p.underruns) {
            say!("winmm playback {:?}: underrun ({} so far)", p.name, p.underruns);
        }
        Ok(Some(Queue { frames, floor: 0, underruns: p.underruns }))
    }

    fn backlog(&mut self) -> io::Result<Option<usize>> {
        Ok(Some(self.rx.len() + self.rec.ready()))
    }

    fn path(&mut self) -> io::Result<Path> {
        Ok(Path { capture: None, queue: true, ..Path::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_and_the_radio_guard() {
        let names: Vec<String> =
            ["Speakers (Realtek(R) Audio)", "Line (Voicemod Virtual Audio De", "CABLE Input (VB-Audio Virtual C", "USB Audio CODEC"]
                .map(String::from)
                .into();
        assert_eq!(find(&names, "USB Audio CODEC", "x").unwrap(), 3);
        assert_eq!(find(&names, "CABLE Input (VB-Audio Virtual Cable)", "x").unwrap(), 2); // WinMM's 31-char cut
        assert_eq!(find(&names, "voicemod", "x").unwrap(), 1);
        assert!(find(&names, "virtual", "x").is_err()); // two matches
        assert_eq!(find(&names, "default", "x").unwrap(), WAVE_MAPPER);
        let err = |n: &str, radio| check_playback(&names, n, radio).err().map(|e| e.to_string());
        assert!(err("USB Audio CODEC", false).is_some_and(|e| e.contains("needs --radio")));
        assert!(err("default", true).is_some_and(|e| e.contains("named explicitly")));
        assert_eq!((err("USB Audio CODEC", true), err("voicemod", false)), (None, None));
    }

    #[test]
    fn wave_structs_have_the_sdk_layout() {
        assert_eq!(size_of::<WAVEFORMATEX>(), 18);
        assert_eq!(size_of::<MMTIME>(), 12);
        assert_eq!(size_of::<WAVEINCAPSW>(), 80);
        assert_eq!(size_of::<WAVEOUTCAPSW>(), 84);
        assert_eq!(size_of::<WAVEHDR>(), if cfg!(target_pointer_width = "64") { 48 } else { 32 });
    }
}
