//! Windows counterpart of signals.rs: Ctrl-C, Ctrl-Break and closing the console window ask for a clean stop
//! (PTT released, sockets closed); a second one exits at once, as a second Unix signal does.
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

struct Trap {
    stop: Arc<AtomicBool>,
    signaled: Arc<AtomicBool>,
    before_exit: fn(),
}

static TRAP: OnceLock<Trap> = OnceLock::new();

#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
}

unsafe extern "system" fn handler(_: u32) -> i32 {
    // Windows runs this on its own thread, so ordinary code is fine here.
    let Some(t) = TRAP.get() else { return 0 };
    if t.signaled.swap(true, Ordering::SeqCst) {
        (t.before_exit)();
        std::process::exit(130);
    }
    t.stop.store(true, Ordering::SeqCst);
    1
}

/// `sigs` is ignored: every console control event is trapped.
pub fn trap(_sigs: &[i32], stop: Arc<AtomicBool>, before_exit: fn()) -> io::Result<Arc<AtomicBool>> {
    let signaled = Arc::new(AtomicBool::new(false));
    TRAP.set(Trap { stop, signaled: signaled.clone(), before_exit })
        .map_err(|_| io::Error::other("signals are already trapped"))?;
    // SAFETY: handler matches PHANDLER_ROUTINE and only reads TRAP, which is set above and never changes.
    if unsafe { SetConsoleCtrlHandler(Some(handler), 1) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(signaled)
}
