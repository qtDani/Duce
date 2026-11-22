use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// Folder the exe is in; settings and the log live next to it.
pub fn dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

static LOCK: Mutex<()> = Mutex::new(());

pub fn log(s: &str) {
    let _held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = dir().join("Duce.log");
    if std::fs::metadata(&path).map(|m| m.len() > 64 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    let t = unsafe {
        let mut t = std::mem::zeroed();
        windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t);
        t
    };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}\r",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, s
        );
    }
}
