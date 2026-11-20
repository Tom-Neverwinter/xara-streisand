//! runat SHIM.dll DIR PROGRAM [ARGS...]: start PROGRAM (32-bit) suspended in DIR, load SHIM.dll into it before
//! any of its own code runs, then let it go. With CLOCKSHIM_DATE=YYYY-MM-DD in the environment the program sees
//! that date. Prints the child's PID and exits; stop the child by that PID.
#![allow(non_snake_case, clippy::upper_case_acronyms)]
use core::ffi::c_void;
use std::os::windows::ffi::OsStrExt;

type Handle = *mut c_void;
const CREATE_SUSPENDED: u32 = 4;
const MEM_COMMIT_RESERVE: u32 = 0x3000;
const PAGE_READWRITE: u32 = 4;
const INFINITE: u32 = u32::MAX;

#[repr(C)]
struct StartupInfo {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    xs: u32,
    ys: u32,
    xc: u32,
    yc: u32,
    fill: u32,
    flags: u32,
    show: u16,
    cb2: u16,
    r2: *mut u8,
    i: Handle,
    o: Handle,
    e: Handle,
}

#[repr(C)]
struct ProcessInfo {
    process: Handle,
    thread: Handle,
    pid: u32,
    tid: u32,
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateProcessW(app: *const u16, cmd: *mut u16, pa: *mut c_void, ta: *mut c_void, inherit: i32, flags: u32,
        env: *mut c_void, dir: *const u16, si: *mut StartupInfo, pi: *mut ProcessInfo) -> i32;
    fn VirtualAllocEx(p: Handle, a: *mut c_void, n: usize, t: u32, prot: u32) -> *mut c_void;
    fn WriteProcessMemory(p: Handle, a: *mut c_void, b: *const c_void, n: usize, w: *mut usize) -> i32;
    fn GetModuleHandleA(n: *const u8) -> Handle;
    fn GetProcAddress(m: Handle, n: *const u8) -> *mut c_void;
    fn CreateRemoteThread(p: Handle, a: *mut c_void, st: usize, f: *mut c_void, arg: *mut c_void, fl: u32,
        tid: *mut u32) -> Handle;
    fn WaitForSingleObject(h: Handle, ms: u32) -> u32;
    fn GetExitCodeThread(h: Handle, code: *mut u32) -> i32;
    fn ResumeThread(h: Handle) -> u32;
    fn TerminateProcess(h: Handle, code: u32) -> i32;
    fn CloseHandle(h: Handle) -> i32;
    fn GetLastError() -> u32;
}

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain([0]).collect()
}

fn quote(a: &str) -> String {
    if a.contains([' ', '\t', '"']) { format!("\"{}\"", a.replace('"', "\\\"")) } else { a.to_string() }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        eprintln!("usage: runat SHIM.dll DIR PROGRAM [ARGS...]");
        std::process::exit(2);
    }
    let dll = std::fs::canonicalize(&a[0]).expect("shim dll").to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let cmdline = a[2..].iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ");
    unsafe {
        let mut si: StartupInfo = std::mem::zeroed();
        si.cb = std::mem::size_of::<StartupInfo>() as u32;
        let mut pi: ProcessInfo = std::mem::zeroed();
        let mut cmd = wide(&cmdline);
        let dir = wide(&a[1]);
        if CreateProcessW(std::ptr::null(), cmd.as_mut_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), 0,
            CREATE_SUSPENDED, std::ptr::null_mut(), dir.as_ptr(), &mut si, &mut pi) == 0 {
            eprintln!("runat: CreateProcess failed ({})", GetLastError());
            std::process::exit(1);
        }
        let path = wide(&dll);
        let n = path.len() * 2;
        let mem = VirtualAllocEx(pi.process, std::ptr::null_mut(), n, MEM_COMMIT_RESERVE, PAGE_READWRITE);
        let mut w = 0usize;
        let load = GetProcAddress(GetModuleHandleA(b"kernel32.dll\0".as_ptr()), b"LoadLibraryW\0".as_ptr());
        let ok = !mem.is_null()
            && WriteProcessMemory(pi.process, mem, path.as_ptr().cast(), n, &mut w) != 0
            && !load.is_null();
        let t = if ok { CreateRemoteThread(pi.process, std::ptr::null_mut(), 0, load, mem, 0, std::ptr::null_mut()) }
            else { std::ptr::null_mut() };
        let mut module = 0u32;
        if !t.is_null() {
            WaitForSingleObject(t, INFINITE);
            GetExitCodeThread(t, &mut module);
            CloseHandle(t);
        }
        if module == 0 {
            eprintln!("runat: loading the shim into the child failed ({})", GetLastError());
            TerminateProcess(pi.process, 1);
            std::process::exit(1);
        }
        ResumeThread(pi.thread);
        println!("{}", pi.pid);
        CloseHandle(pi.thread);
        CloseHandle(pi.process);
    }
}
