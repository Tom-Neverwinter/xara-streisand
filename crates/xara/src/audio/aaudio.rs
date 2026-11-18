//! Android audio through AAudio (NDK, Android 8.0 / API 26+): 48 kHz mono 16-bit, blocking reads and writes.
//! Android names no devices to native code, so a device is `default` or a numeric AAudio device id (as
//! Java's `AudioDeviceInfo.getId()` reports it; e.g. a USB sound card plugged in over OTG).  Every Android
//! output is a real device, so playback needs --radio.
use std::ffi::c_void;
use std::io;

use super::ratematch::{Queue, TxBuffer};
use super::{Audio, DeviceList, IO_TIMEOUT, Path, RATE, Report, capture_only};

type Builder = *mut c_void;
type Stream = *mut c_void;

const DIRECTION_OUTPUT: i32 = 0;
const DIRECTION_INPUT: i32 = 1;
const FORMAT_PCM_I16: i32 = 1;
const SHARING_SHARED: i32 = 1;
const PERFORMANCE_LOW_LATENCY: i32 = 12;
const UNSPECIFIED: i32 = 0;

#[link(name = "aaudio")]
unsafe extern "C" {
    fn AAudio_createStreamBuilder(b: *mut Builder) -> i32;
    fn AAudio_convertResultToText(r: i32) -> *const std::ffi::c_char;
    fn AAudioStreamBuilder_setDirection(b: Builder, d: i32);
    fn AAudioStreamBuilder_setDeviceId(b: Builder, id: i32);
    fn AAudioStreamBuilder_setSampleRate(b: Builder, rate: i32);
    fn AAudioStreamBuilder_setChannelCount(b: Builder, n: i32);
    fn AAudioStreamBuilder_setFormat(b: Builder, f: i32);
    fn AAudioStreamBuilder_setSharingMode(b: Builder, m: i32);
    fn AAudioStreamBuilder_setPerformanceMode(b: Builder, m: i32);
    fn AAudioStreamBuilder_openStream(b: Builder, s: *mut Stream) -> i32;
    fn AAudioStreamBuilder_delete(b: Builder) -> i32;
    fn AAudioStream_requestStart(s: Stream) -> i32;
    fn AAudioStream_read(s: Stream, buf: *mut c_void, frames: i32, timeout_ns: i64) -> i32;
    fn AAudioStream_write(s: Stream, buf: *const c_void, frames: i32, timeout_ns: i64) -> i32;
    fn AAudioStream_getFramesWritten(s: Stream) -> i64;
    fn AAudioStream_getFramesRead(s: Stream) -> i64;
    fn AAudioStream_getXRunCount(s: Stream) -> i32;
    fn AAudioStream_getSampleRate(s: Stream) -> i32;
    fn AAudioStream_setBufferSizeInFrames(s: Stream, frames: i32) -> i32;
    fn AAudioStream_close(s: Stream) -> i32;
}

fn check(what: &str, r: i32) -> io::Result<i32> {
    if r >= 0 {
        return Ok(r);
    }
    // SAFETY: AAudio returns a static NUL-terminated string for any result code.
    let text = unsafe { std::ffi::CStr::from_ptr(AAudio_convertResultToText(r)) }.to_string_lossy().into_owned();
    Err(io::Error::other(format!("aaudio {what}: {text} ({r})")))
}

fn device_id(name: &str) -> io::Result<i32> {
    match name {
        "" | "default" => Ok(UNSPECIFIED),
        n => n
            .parse::<i32>()
            .ok()
            .filter(|&id| id > 0)
            .ok_or_else(|| io::Error::other(format!("{n:?}: an AAudio device is `default` or a numeric device id"))),
    }
}

struct Owned(Stream);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: self.0 is a stream openStream returned, closed only here.
        unsafe { AAudioStream_close(self.0) };
    }
}

fn open(direction: i32, device: &str) -> io::Result<Owned> {
    let id = device_id(device)?;
    let mut b: Builder = std::ptr::null_mut();
    // SAFETY: b is a valid out-pointer.
    check("builder", unsafe { AAudio_createStreamBuilder(&mut b) })?;
    let mut s: Stream = std::ptr::null_mut();
    // SAFETY: b is a live builder until deleted below; s is a valid out-pointer.
    let r = unsafe {
        AAudioStreamBuilder_setDirection(b, direction);
        AAudioStreamBuilder_setDeviceId(b, id);
        AAudioStreamBuilder_setSampleRate(b, RATE as i32);
        AAudioStreamBuilder_setChannelCount(b, 1);
        AAudioStreamBuilder_setFormat(b, FORMAT_PCM_I16);
        AAudioStreamBuilder_setSharingMode(b, SHARING_SHARED);
        AAudioStreamBuilder_setPerformanceMode(b, PERFORMANCE_LOW_LATENCY);
        let r = AAudioStreamBuilder_openStream(b, &mut s);
        AAudioStreamBuilder_delete(b);
        r
    };
    let what = if direction == DIRECTION_INPUT { "capture" } else { "playback" };
    check(&format!("opening {what} {device:?}"), r)?;
    let s = Owned(s);
    // SAFETY: s.0 is an open stream.
    let rate = unsafe { AAudioStream_getSampleRate(s.0) };
    if rate != RATE as i32 {
        return Err(io::Error::other(format!("aaudio {what}: device runs at {rate} Hz, not {RATE}")));
    }
    // SAFETY: as above.
    check(&format!("starting {what}"), unsafe { AAudioStream_requestStart(s.0) })?;
    Ok(s)
}

pub fn list() -> io::Result<(DeviceList, DeviceList)> {
    let d = || vec![("default".to_string(), "or a numeric AAudio device id (AudioDeviceInfo.getId())".to_string())];
    Ok((d(), d()))
}

pub fn check_playback(playback: &str, radio: bool) -> io::Result<()> {
    device_id(playback)?;
    if !radio {
        return Err(io::Error::other("refusing playback: every Android output is a real device; add --radio"));
    }
    Ok(())
}

struct Play {
    s: Owned,
    report: Report,
}

pub struct AaudioIo {
    rec: Owned,
    play: Option<Play>,
}

// SAFETY: AAudio streams may be used from any one thread at a time; whichever owns the value is that thread.
unsafe impl Send for AaudioIo {}

impl AaudioIo {
    pub fn open(capture: &str, playback: &str, radio: bool, tx: TxBuffer) -> io::Result<Self> {
        check_playback(playback, radio)?;
        let s = open(DIRECTION_OUTPUT, playback)?;
        // writes block once this much is queued, as the other backends' TxBuffer ceiling
        // SAFETY: s.0 is an open stream.
        check("playback buffer", unsafe { AAudioStream_setBufferSizeInFrames(s.0, tx.capacity() as i32) })?;
        let play = Play { s, report: Report::default() };
        Ok(Self { rec: open(DIRECTION_INPUT, capture)?, play: Some(play) })
    }

    pub fn open_capture(capture: &str) -> io::Result<Self> {
        Ok(Self { rec: open(DIRECTION_INPUT, capture)?, play: None })
    }
}

const TIMEOUT_NS: i64 = IO_TIMEOUT.as_nanos() as i64;

impl Audio for AaudioIo {
    fn read(&mut self, x: &mut [i16]) -> io::Result<()> {
        let mut done = 0;
        while done < x.len() {
            let rest = &mut x[done..];
            // SAFETY: rest is a writable buffer of rest.len() mono i16 frames.
            let n = unsafe { AAudioStream_read(self.rec.0, rest.as_mut_ptr().cast(), rest.len() as i32, TIMEOUT_NS) };
            match check("capture", n)? {
                0 => return Err(io::Error::new(io::ErrorKind::TimedOut, "capture (no audio): timed out")),
                n => done += n as usize,
            }
        }
        Ok(())
    }

    fn write(&mut self, x: &[i16]) -> io::Result<()> {
        let Some(p) = &mut self.play else { return Err(capture_only()) };
        let mut done = 0;
        while done < x.len() {
            let rest = &x[done..];
            // SAFETY: rest is a readable buffer of rest.len() mono i16 frames.
            let n = unsafe { AAudioStream_write(p.s.0, rest.as_ptr().cast(), rest.len() as i32, TIMEOUT_NS) };
            match check("playback", n)? {
                0 => return Err(io::Error::new(io::ErrorKind::TimedOut, "playback (device not draining): timed out")),
                n => done += n as usize,
            }
        }
        Ok(())
    }

    fn queued(&mut self) -> io::Result<Option<Queue>> {
        let Some(p) = &mut self.play else { return Err(capture_only()) };
        // SAFETY: p.s.0 is an open stream.
        let (written, played, xruns) = unsafe {
            (AAudioStream_getFramesWritten(p.s.0), AAudioStream_getFramesRead(p.s.0), AAudioStream_getXRunCount(p.s.0))
        };
        let underruns = u64::try_from(xruns).unwrap_or(0);
        if p.report.due(underruns) {
            say!("aaudio playback: underrun ({underruns} so far)");
        }
        let frames = usize::try_from(written - played).unwrap_or(0);
        Ok(Some(Queue { frames, floor: 0, underruns }))
    }

    fn backlog(&mut self) -> io::Result<Option<usize>> {
        // SAFETY: self.rec.0 is an open stream.
        let (arrived, taken) =
            unsafe { (AAudioStream_getFramesWritten(self.rec.0), AAudioStream_getFramesRead(self.rec.0)) };
        Ok(usize::try_from(arrived - taken).ok())
    }

    fn path(&mut self) -> io::Result<Path> {
        Ok(Path { capture: None, queue: true, ..Path::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_ids() {
        assert_eq!(device_id("default").unwrap(), UNSPECIFIED);
        assert_eq!(device_id("17").unwrap(), 17);
        assert!(device_id("0").is_err() && device_id("usb").is_err());
        assert!(check_playback("default", false).is_err());
        assert!(check_playback("default", true).is_ok());
    }
}
