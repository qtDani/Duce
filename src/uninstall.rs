// Removing Duce from a PC: stop every Duce process, unhook it from login,
// remove the Start menu entry, and delete its files.
//   --uninstall-cleanup  everything except deleting the program files
//                        (the MSI runs this while uninstalling, then removes the files itself)
//   --uninstall          full removal, used by the button in Settings
use crate::hw::wide;
use crate::log::log;
use std::os::windows::process::CommandExt;
use windows_sys::Win32::System::ApplicationInstallationAndServicing::MsiEnumRelatedProductsW;
use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, HKEY_CURRENT_USER};

/// The MSI's upgrade code, the one id every Duce version shares; must match duce.wxs.
/// (Each build gets its own product code, so the installed one has to be looked up.)
const UPGRADE_CODE: &str = "{4EB5AF92-5F18-4E93-B1C6-649EE48A8B6F}";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn cleanup() {
    log("Uninstall: cleaning up");
    stop_other_copies();
    unsafe {
        RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide("Duce").as_ptr());
        RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide("MiniRGB").as_ptr());
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let link = std::path::Path::new(&appdata).join(r"Microsoft\Windows\Start Menu\Programs\Duce.lnk");
        let _ = std::fs::remove_file(link);
    }
}

pub fn uninstall() -> i32 {
    cleanup();
    if let Some(product) = installed_msi_product() {
        // Windows removes the files, the shortcut and the Apps-list entry.
        let r = std::process::Command::new("msiexec.exe").args(["/x", &product, "/passive"]).spawn();
        if let Err(e) = r {
            log(&format!("Uninstall ERROR: could not start msiexec: {e}"));
            return 1;
        }
        return 0;
    }
    // A plain copied exe: delete our own files once this process has exited.
    // Only known files are deleted, and the folder is removed only if that leaves it empty,
    // so a Duce.exe sitting on the Desktop never takes anything else with it.
    let Ok(exe) = std::env::current_exe() else { return 1 };
    let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let script = format!(
        "ping 127.0.0.1 -n 3 >nul & del /f /q \"{exe}\" \"{dir}\\settings.txt\" \"{dir}\\Duce.log\" & rd \"{dir}\"",
        exe = exe.display(),
        dir = dir.display(),
    );
    let mut cmd = std::process::Command::new("cmd.exe");
    // raw_arg: handed to cmd exactly as written; normal argument quoting would garble it.
    cmd.raw_arg(format!("/c {script}")).creation_flags(CREATE_NO_WINDOW);
    let r = cmd.spawn();
    if let Err(e) = r {
        log(&format!("Uninstall ERROR: could not schedule file removal: {e}"));
        return 1;
    }
    0
}

/// Ends every other Duce process (open windows, or the resident helper of versions
/// before 1.1), never this one.
pub fn stop_other_copies() {
    let me = std::process::id().to_string();
    let r = std::process::Command::new("taskkill.exe")
        .args(["/F", "/IM", "Duce.exe", "/FI", &format!("PID ne {me}")])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    if let Err(e) = r {
        log(&format!("Uninstall: taskkill failed: {e}"));
    }
    std::thread::sleep(std::time::Duration::from_millis(400));
}

/// The product code of whichever Duce version the MSI installed, if any.
fn installed_msi_product() -> Option<String> {
    let mut buf = [0u16; 39]; // a GUID string and its terminator
    let r = unsafe { MsiEnumRelatedProductsW(wide(UPGRADE_CODE).as_ptr(), 0, 0, buf.as_mut_ptr()) };
    if r != 0 {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}
