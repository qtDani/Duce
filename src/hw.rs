// Applying colors to both controllers, one at a time.
use crate::{board, gpu, log::log, settings};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The window, the background helper and the old C# MiniRGB are separate processes;
/// this named lock (shared with the old C# MiniRGB) stops them writing at the same time.
struct HardwareLock(HANDLE);

impl HardwareLock {
    fn take() -> Result<Self, String> {
        unsafe {
            let h = CreateMutexW(std::ptr::null(), 0, wide(r"Local\MiniRGB.Hardware").as_ptr());
            if h.is_null() {
                return Err("Could not create the hardware lock".into());
            }
            let r = WaitForSingleObject(h, 45_000);
            // Abandoned = the other process exited mid-write; the lock is ours now.
            if r == WAIT_OBJECT_0 || r == WAIT_ABANDONED {
                Ok(HardwareLock(h))
            } else {
                CloseHandle(h);
                Err("Lights are busy".into())
            }
        }
    }
}

impl Drop for HardwareLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

/// Checks a controller under the hardware lock, retrying a few times: NVIDIA's driver
/// sometimes fails a single read (status -1) when other software is using the card.
pub fn identify(what: &str, check: fn() -> Result<String, String>) -> Result<String, String> {
    let mut last = String::new();
    for attempt in 0..4 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
        match HardwareLock::take().and_then(|_held| check()) {
            Ok(id) => return Ok(id),
            Err(e) => {
                log(&format!("{what} check failed (try {}): {e}", attempt + 1));
                last = e;
            }
        }
    }
    Err(last)
}

/// One quiet check under the hardware lock, for repeated background re-checks.
pub fn identify_once(check: fn() -> Result<String, String>) -> Result<String, String> {
    HardwareLock::take().and_then(|_held| check())
}

/// Each device is handled separately so a problem with one never blocks the other.
pub fn apply(rgb: u32, fans: bool, gpu: bool, wait: bool) -> String {
    let (r, g, b) = ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    let mut errors = String::new();
    if fans {
        if let Err(e) = HardwareLock::take().and_then(|_held| board::apply(r, g, b, wait)) {
            log(&format!("Fans ERROR: {e}"));
            errors += &format!("Fans: {e}\n");
        }
    }
    if gpu {
        if let Err(e) = HardwareLock::take().and_then(|_held| gpu::apply(r, g, b, wait)) {
            log(&format!("GPU ERROR: {e}"));
            errors += &format!("GPU: {e}\n");
        }
    }
    errors
}

/// Each controller gets its own saved color, whichever ones are selected in the window.
pub fn apply_saved(wait: bool) -> String {
    let s = settings::load();
    let mut errors = String::new();
    if s.fans_rgb >= 0 {
        errors += &apply(settings::dim(s.fans_rgb, s.brightness) as u32, true, false, wait);
    }
    if s.gpu_rgb >= 0 {
        errors += &apply(settings::dim(s.gpu_rgb, s.brightness) as u32, false, true, wait);
    }
    errors
}

/// Saved colors are untouched, so the next apply brings them back.
/// At shutdown and sleep only the fans need it: the graphics card loses power by itself,
/// and writing to it takes ~4 s, longer than Windows waits before sleeping.
pub fn lights_off(gpu_too: bool) -> String {
    let s = settings::load();
    let mut errors = String::new();
    if s.fans_rgb >= 0 {
        errors += &apply(0, true, false, false);
    }
    if gpu_too && s.gpu_rgb >= 0 {
        errors += &apply(0, false, true, false);
    }
    errors
}
