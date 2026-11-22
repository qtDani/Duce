// A look around the PC for RGB gear Duce cannot drive, so the window can say what it saw
// instead of showing nothing. This only reads USB device descriptions; it never opens a
// device for writing and never sends anything.
use crate::board;
use crate::hw::wide;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};

/// USB vendors known for RGB hardware. Their devices show up as "found, not supported".
const RGB_VENDORS: [(u16, &str); 11] = [
    (0x0B05, "ASUS"),
    (0x048D, "Gigabyte (ITE)"),
    (0x1B1C, "Corsair"),
    (0x1E71, "NZXT"),
    (0x2516, "Cooler Master"),
    (0x1462, "MSI"),
    (0x264A, "Thermaltake"),
    (0x0CF2, "ENE / Lian Li"),
    (0x1532, "Razer"),
    (0x1038, "SteelSeries"),
    (0x3842, "EVGA"),
];

/// Every RGB device Duce does not control, one line each: devices OpenRGB knows
/// (so support exists and could be ported into Duce), plus anything from a known
/// RGB vendor even when OpenRGB doesn't know it either.
pub fn unsupported_rgb_devices() -> Vec<String> {
    let mut seen = Vec::new();
    let mut lines = Vec::new();
    for (vid, pid, product) in hid_devices() {
        if vid == 0x048D && board::SUPPORTED_PIDS.contains(&pid) {
            continue; // that one is ours
        }
        let known = crate::openrgb_ids::knows(vid, pid);
        let vendor = RGB_VENDORS.iter().find(|(v, _)| *v == vid).map(|(_, n)| *n);
        if !known && vendor.is_none() {
            continue; // an ordinary USB device
        }
        if seen.contains(&(vid, pid)) {
            continue;
        }
        seen.push((vid, pid));
        let what = match (product.is_empty(), vendor) {
            (false, _) => product,
            (true, Some(v)) => format!("{v} device {pid:04X}"),
            (true, None) => format!("USB device {vid:04X}:{pid:04X}"),
        };
        let tag = if known { "OpenRGB: yes" } else { "OpenRGB: no" };
        lines.push(format!("{what} ({tag})"));
        if lines.len() >= 10 {
            break;
        }
    }
    lines.sort();
    lines
}

/// All HID interfaces on the PC as (vendor, product, description), reading metadata only.
fn hid_devices() -> Vec<(u16, u16, String)> {
    let mut found = Vec::new();
    unsafe {
        let mut guid = std::mem::zeroed();
        HidD_GetHidGuid(&mut guid);
        let set = SetupDiGetClassDevsW(&guid, std::ptr::null(), std::ptr::null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set == INVALID_HANDLE_VALUE as HDEVINFO {
            return found;
        }
        let mut data: SP_DEVICE_INTERFACE_DATA = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
        let mut i = 0;
        while SetupDiEnumDeviceInterfaces(set, std::ptr::null(), &guid, i, &mut data) != 0 {
            i += 1;
            let mut needed = 0u32;
            SetupDiGetDeviceInterfaceDetailW(set, &data, std::ptr::null_mut(), 0, &mut needed, std::ptr::null_mut());
            if needed < 8 {
                continue;
            }
            let mut mem = vec![0u32; (needed as usize + 3) / 4];
            mem[0] = 8; // cbSize of the fixed part on 64-bit
            let detail = mem.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            if SetupDiGetDeviceInterfaceDetailW(set, &data, detail, needed, &mut needed, std::ptr::null_mut()) == 0 {
                continue;
            }
            let chars = std::slice::from_raw_parts((mem.as_ptr() as *const u16).add(2), (needed as usize - 4) / 2);
            let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
            let path = String::from_utf16_lossy(&chars[..end]);
            // Access 0: enough to ask who the device is, with no way to read or write data.
            let h = CreateFileW(wide(&path).as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
            if h == INVALID_HANDLE_VALUE {
                continue;
            }
            let mut a: HIDD_ATTRIBUTES = std::mem::zeroed();
            a.Size = std::mem::size_of::<HIDD_ATTRIBUTES>() as u32;
            if HidD_GetAttributes(h, &mut a) != 0 {
                let mut name = [0u16; 64];
                let product = if HidD_GetProductString(h, name.as_mut_ptr().cast(), (name.len() * 2) as u32) != 0 {
                    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                    String::from_utf16_lossy(&name[..end]).trim().to_string()
                } else {
                    String::new()
                };
                found.push((a.VendorID, a.ProductID, product));
            }
            CloseHandle(h);
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    found
}
