//! A date shim for one 32-bit Windows process: loaded into it, it moves the wall-clock date it sees by a whole
//! number of days (CLOCKSHIM_DATE=YYYY-MM-DD picks the date; the time of day is kept), leaving every timer and
//! tick count alone. It patches kernelbase's GetSystemTimeAsFileTime, GetSystemTimePreciseAsFileTime,
//! GetSystemTime and GetLocalTime with jumps to replacements that read the real time from ntdll and add the shift.
//! The machine's clock is never touched.
#![allow(non_snake_case, clippy::missing_safety_doc)]
use core::ffi::c_void;

type Bool = i32;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;
const DAY: i64 = 864_000_000_000; // 100 ns units

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FileTime {
    lo: u32,
    hi: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SystemTime {
    year: u16,
    month: u16,
    dow: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    ms: u16,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(m: *mut c_void, name: *const u8) -> *mut c_void;
    fn VirtualProtect(p: *mut c_void, n: usize, new: u32, old: *mut u32) -> Bool;
    fn FlushInstructionCache(h: *mut c_void, p: *const c_void, n: usize) -> Bool;
    fn GetCurrentProcess() -> *mut c_void;
    fn FileTimeToSystemTime(ft: *const FileTime, st: *mut SystemTime) -> Bool;
    fn SystemTimeToFileTime(st: *const SystemTime, ft: *mut FileTime) -> Bool;
    fn FileTimeToLocalFileTime(ft: *const FileTime, local: *mut FileTime) -> Bool;
    fn GetEnvironmentVariableA(name: *const u8, buf: *mut u8, n: u32) -> u32;
    fn OutputDebugStringA(s: *const u8);
}

#[link(name = "ntdll")]
extern "system" {
    fn NtQuerySystemTime(t: *mut i64) -> i32;
}

static mut SHIFT: i64 = 0;

fn real() -> i64 {
    let mut t = 0i64;
    unsafe { NtQuerySystemTime(&mut t) };
    t
}

fn now() -> FileTime {
    let t = real() + unsafe { SHIFT };
    FileTime { lo: t as u32, hi: (t >> 32) as u32 }
}

unsafe extern "system" fn get_system_time_as_file_time(out: *mut FileTime) {
    *out = now();
}

unsafe extern "system" fn get_system_time(out: *mut SystemTime) {
    let ft = now();
    FileTimeToSystemTime(&ft, out);
}

unsafe extern "system" fn get_local_time(out: *mut SystemTime) {
    let ft = now();
    let mut local = FileTime::default();
    FileTimeToLocalFileTime(&ft, &mut local);
    FileTimeToSystemTime(&local, out);
}

/// Days from today (UTC) to CLOCKSHIM_DATE, in 100 ns units; 0 if unset or unreadable.
unsafe fn shift_from_env() -> i64 {
    let mut b = [0u8; 32];
    let n = GetEnvironmentVariableA(b"CLOCKSHIM_DATE\0".as_ptr(), b.as_mut_ptr(), b.len() as u32) as usize;
    let s = core::str::from_utf8(&b[..n.min(b.len())]).unwrap_or("");
    let mut it = s.split('-').map(|p| p.parse::<u16>().ok());
    let (Some(Some(y)), Some(Some(m)), Some(Some(d))) = (it.next(), it.next(), it.next()) else {
        return 0;
    };
    let mut today = SystemTime::default();
    let ft = {
        let t = real();
        FileTime { lo: t as u32, hi: (t >> 32) as u32 }
    };
    FileTimeToSystemTime(&ft, &mut today);
    let day_of = |st: &SystemTime| {
        let mut f = FileTime::default();
        let mid = SystemTime { year: st.year, month: st.month, day: st.day, ..SystemTime::default() };
        SystemTimeToFileTime(&mid, &mut f);
        (i64::from(f.hi) << 32 | i64::from(f.lo)) / DAY
    };
    (day_of(&SystemTime { year: y, month: m, day: d, ..SystemTime::default() }) - day_of(&today)) * DAY
}

/// Overwrite the start of `module!name` with `jmp to`.
unsafe fn hook(module: &[u8], name: &[u8], to: usize) -> bool {
    let m = GetModuleHandleA(module.as_ptr());
    if m.is_null() {
        return false;
    }
    let f = GetProcAddress(m, name.as_ptr()) as *mut u8;
    if f.is_null() {
        return false;
    }
    let mut old = 0u32;
    if VirtualProtect(f.cast(), 5, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
        return false;
    }
    let rel = (to as isize - (f as isize + 5)) as i32;
    f.write(0xe9);
    (f.add(1) as *mut i32).write_unaligned(rel);
    VirtualProtect(f.cast(), 5, old, &mut old);
    FlushInstructionCache(GetCurrentProcess(), f.cast(), 5);
    true
}

#[no_mangle]
pub unsafe extern "system" fn DllMain(_h: *mut c_void, reason: u32, _r: *mut c_void) -> Bool {
    if reason == 1 {
        SHIFT = shift_from_env();
        if SHIFT != 0 {
            let mut ok = true;
            for (module, name, to) in [
                (&b"kernelbase.dll\0"[..], &b"GetSystemTimeAsFileTime\0"[..], get_system_time_as_file_time as usize),
                (b"kernelbase.dll\0", b"GetSystemTimePreciseAsFileTime\0", get_system_time_as_file_time as usize),
                (b"kernelbase.dll\0", b"GetSystemTime\0", get_system_time as usize),
                (b"kernelbase.dll\0", b"GetLocalTime\0", get_local_time as usize),
            ] {
                ok &= hook(module, name, to);
            }
            OutputDebugStringA(if ok { b"clockshim: date shifted\0".as_ptr() } else { b"clockshim: a hook failed\0".as_ptr() });
        }
    }
    1
}
