//! CM108/CM119 GPIO PTT on Windows: find the HID interface by USB vendor/product id through SetupAPI and send
//! the same 5-byte output report the hidraw (Linux/BSD) and IOKit (macOS) paths send.
use std::ffi::c_void;
use std::io;
use std::ptr::{null, null_mut};

use crate::ptt::{Ptt, cm108_report};

type Handle = *mut c_void;
const INVALID: Handle = -1isize as Handle;

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[repr(C)]
struct InterfaceData {
    size: u32,
    class: Guid,
    flags: u32,
    reserved: usize,
}

#[repr(C)]
struct Attributes {
    size: u32,
    vendor: u16,
    product: u16,
    version: u16,
}

const DIGCF_PRESENT: u32 = 0x02;
const DIGCF_DEVICEINTERFACE: u32 = 0x10;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_SHARE_READ_WRITE: u32 = 0x3;
const OPEN_EXISTING: u32 = 3;

#[link(name = "hid")]
unsafe extern "system" {
    fn HidD_GetHidGuid(g: *mut Guid);
    fn HidD_GetAttributes(h: Handle, a: *mut Attributes) -> u8;
    fn HidD_SetOutputReport(h: Handle, buf: *const u8, len: u32) -> u8;
}

#[link(name = "setupapi")]
unsafe extern "system" {
    fn SetupDiGetClassDevsW(g: *const Guid, enumerator: *const u16, parent: Handle, flags: u32) -> Handle;
    fn SetupDiEnumDeviceInterfaces(
        set: Handle,
        info: *const c_void,
        g: *const Guid,
        i: u32,
        d: *mut InterfaceData,
    ) -> i32;
    fn SetupDiGetDeviceInterfaceDetailW(
        set: Handle,
        d: *const InterfaceData,
        detail: *mut u8,
        size: u32,
        need: *mut u32,
        info: *mut c_void,
    ) -> i32;
    fn SetupDiDestroyDeviceInfoList(set: Handle) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        sec: *const c_void,
        disp: u32,
        flags: u32,
        tmpl: Handle,
    ) -> Handle;
    fn WriteFile(h: Handle, buf: *const u8, len: u32, wrote: *mut u32, ov: *mut c_void) -> i32;
    fn CloseHandle(h: isize) -> i32; // isize, as audio/winmm.rs declares it
}

struct File(Handle);

impl Drop for File {
    fn drop(&mut self) {
        // SAFETY: self.0 is a handle CreateFileW returned and nothing else closes it.
        unsafe { CloseHandle(self.0 as isize) };
    }
}

// SAFETY: a file handle may be used from any thread; whichever owns the value is its only user.
unsafe impl Send for File {}

fn open(path: &[u16], access: u32) -> Option<File> {
    // SAFETY: path is NUL-terminated UTF-16.
    let h = unsafe { CreateFileW(path.as_ptr(), access, FILE_SHARE_READ_WRITE, null(), OPEN_EXISTING, 0, null_mut()) };
    (h != INVALID).then_some(File(h))
}

/// Device interface paths (NUL-terminated UTF-16) of every present HID device.
fn hid_paths() -> io::Result<Vec<Vec<u16>>> {
    let mut g = Guid { data1: 0, data2: 0, data3: 0, data4: [0; 8] };
    // SAFETY: g is a valid out-pointer.
    unsafe { HidD_GetHidGuid(&mut g) };
    // SAFETY: g is valid; the other arguments may be null.
    let set = unsafe { SetupDiGetClassDevsW(&g, null(), null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE) };
    if set == INVALID {
        return Err(io::Error::last_os_error());
    }
    let mut out = Vec::new();
    for i in 0.. {
        let mut d = InterfaceData { size: size_of::<InterfaceData>() as u32, class: g, flags: 0, reserved: 0 };
        // SAFETY: set is a live device-info set and d is sized as the API requires.
        if unsafe { SetupDiEnumDeviceInterfaces(set, null(), &g, i, &mut d) } == 0 {
            break;
        }
        let mut need = 0u32;
        // SAFETY: a null buffer with size 0 only asks for the size.
        unsafe { SetupDiGetDeviceInterfaceDetailW(set, &d, null_mut(), 0, &mut need, null_mut()) };
        if need < 8 {
            continue;
        }
        // SP_DEVICE_INTERFACE_DETAIL_DATA_W: a u32 cbSize (8 on 64-bit, 6 on 32-bit) then the path.
        let mut buf = vec![0u8; need as usize + 2];
        let cb: u32 = if cfg!(target_pointer_width = "64") { 8 } else { 6 };
        buf[..4].copy_from_slice(&cb.to_ne_bytes());
        // SAFETY: buf holds `need` bytes and starts with the cbSize the API checks.
        if unsafe { SetupDiGetDeviceInterfaceDetailW(set, &d, buf.as_mut_ptr(), need, null_mut(), null_mut()) } == 0 {
            continue;
        }
        let path: Vec<u16> = buf[4..].chunks_exact(2).map(|c| u16::from_ne_bytes([c[0], c[1]])).collect();
        let end = path.iter().position(|&c| c == 0).unwrap_or(path.len());
        out.push(path[..end].iter().copied().chain([0]).collect());
    }
    // SAFETY: set came from SetupDiGetClassDevsW.
    unsafe { SetupDiDestroyDeviceInfoList(set) };
    Ok(out)
}

fn ids(f: &File) -> Option<(u16, u16)> {
    let mut a = Attributes { size: size_of::<Attributes>() as u32, vendor: 0, product: 0, version: 0 };
    // SAFETY: f is an open HID handle and a is sized as the API requires.
    (unsafe { HidD_GetAttributes(f.0, &mut a) } != 0).then_some((a.vendor, a.product))
}

struct Cm108Win {
    f: File,
    id: (u16, u16),
    gpio: u8,
}

impl Ptt for Cm108Win {
    fn set(&mut self, on: bool) -> io::Result<()> {
        let r = cm108_report(self.gpio, on);
        let mut n = 0;
        // SAFETY: f is open for writing and r outlives the call.
        if unsafe { WriteFile(self.f.0, r.as_ptr(), r.len() as u32, &mut n, null_mut()) } != 0 && n as usize == r.len()
        {
            return Ok(());
        }
        // Some drivers only take output reports over the control pipe.
        // SAFETY: as above.
        if unsafe { HidD_SetOutputReport(self.f.0, r.as_ptr(), r.len() as u32) } != 0 {
            return Ok(());
        }
        Err(io::Error::other(format!("CM108 {:04x}:{:04x}: {}", self.id.0, self.id.1, io::Error::last_os_error())))
    }

    fn describe(&self) -> String {
        format!("CM108 GPIO{} on USB {:04x}:{:04x}", self.gpio, self.id.0, self.id.1)
    }
}

/// The first present HID interface with this USB id that opens for writing (a CM108 shows one per interface;
/// only the HID one accepts the report).
pub fn cm108_usb(vendor: u16, product: u16, gpio: u8) -> io::Result<Box<dyn Ptt>> {
    for path in hid_paths()? {
        let Some(probe) = open(&path, 0) else { continue };
        if ids(&probe) != Some((vendor, product)) {
            continue;
        }
        let Some(f) = open(&path, GENERIC_WRITE) else { continue };
        let mut c = Cm108Win { f, id: (vendor, product), gpio };
        if c.set(false).is_ok() {
            return Ok(Box::new(c));
        }
    }
    Err(io::Error::other(format!("no writable CM108 HID device {vendor:04x}:{product:04x} (is it plugged in?)")))
}
