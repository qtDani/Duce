// Motherboard / fan headers: Gigabyte's ITE RGB controllers over USB HID
// (feature reports, report ID 0xCC). The IT5711, IT8297 and IT5702 share one protocol;
// they differ in which lighting zones exist, so the zone list is chosen from the chip's
// own identity string. Nothing is written until the chip has identified itself.
use crate::hw::wide;
use crate::log::log;
use std::time::Duration;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};

const VENDOR_ITE: u16 = 0x048D;
pub const SUPPORTED_PIDS: [u16; 3] = [0x5711, 0x8297, 0x5702];
const PACKET_SIZE: usize = 64;
const LEDS_PER_PACKET: usize = 19;
const LEDS_PER_HEADER: usize = 120;
const REPORT_ID: u8 = 0xCC;

/// What one chip generation has: 12V zones taking blue-green-red, 12V zones taking
/// red-green-blue, and addressable headers taking green-red-blue.
struct Chip {
    name: &'static str,
    zones_bgr: &'static [u8],
    zones_rgb: &'static [u8],
    argb: &'static [u8],
    extended_reset: bool, // the IT5711 also resets zones 0x90..0x92
}

const CHIPS: [Chip; 3] = [
    // The IT5711 layout was taken from this exact board; 0x24 is its 12V RGB header.
    Chip { name: "IT5711", zones_bgr: &[0x20, 0x22, 0x23, 0x25, 0x26, 0x27, 0x91, 0x92], zones_rgb: &[0x24], argb: &[0x58, 0x59, 0x62], extended_reset: true },
    // Older RGB Fusion 2 chips: eight 12V zones and two addressable headers.
    Chip { name: "IT8297", zones_bgr: &[0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27], zones_rgb: &[], argb: &[0x58, 0x59], extended_reset: false },
    Chip { name: "IT5702", zones_bgr: &[0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27], zones_rgb: &[], argb: &[0x58, 0x59], extended_reset: false },
];

/// The chip the controller claims to be, from its identity string.
fn chip_for(id: &str) -> Option<&'static Chip> {
    CHIPS.iter().find(|c| id.contains(c.name))
}

struct Device {
    h: HANDLE,
    len: usize,
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.h) };
    }
}

pub fn identify() -> Result<String, String> {
    let d = open_or_fail(false)?;
    let id = read_id(&d)?;
    if chip_for(&id).is_none() {
        return Err(format!("Unsupported Gigabyte controller: {id}"));
    }
    Ok(id)
}

pub fn apply(r: u8, g: u8, b: u8, wait: bool) -> Result<(), String> {
    let d = open_or_fail(wait)?;
    let id = read_id(&d)?;
    let Some(chip) = chip_for(&id) else {
        return Err(format!("Unexpected controller: {id}"));
    };
    for z in 0x20..=0x27u8 { send(&d, &[z])?; }
    if chip.extended_reset {
        for z in 0x90..=0x92u8 { send(&d, &[z])?; }
    }
    commit(&d)?;

    send(&d, &[0x32, 0x0B])?;       // ARGB headers: direct mode
    send(&d, &[0x34, 0x22, 0x02])?; // up to 256 LEDs on each header

    for &z in chip.zones_bgr {
        set_zone(&d, z, b, g, r)?; // these zones take blue, green, red
        commit(&d)?;
    }
    for &z in chip.zones_rgb {
        set_zone(&d, z, r, g, b)?;
        commit(&d)?;
    }
    for &ch in chip.argb {
        set_header(&d, ch, g, r, b)?; // addressable strips take green, red, blue
    }
    log(&format!("Fans: applied #{r:02X}{g:02X}{b:02X} on {id}"));
    Ok(())
}

fn open_or_fail(wait: bool) -> Result<Device, String> {
    // At login the USB device may not be ready yet.
    for attempt in 0.. {
        for pid in SUPPORTED_PIDS {
            if let Some(d) = open(VENDOR_ITE, pid, 0xFF89, 0x00CC) {
                if d.len < PACKET_SIZE {
                    return Err(format!("Unexpected report length {}", d.len));
                }
                return Ok(d);
            }
        }
        if !wait || attempt >= 30 {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Err("Motherboard RGB controller not found".into())
}

fn read_id(d: &Device) -> Result<String, String> {
    send(d, &[0x60])?;
    let mut buf = vec![0u8; d.len];
    buf[0] = REPORT_ID;
    if unsafe { HidD_GetFeature(d.h, buf.as_mut_ptr().cast(), d.len as u32) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let raw = &buf[12..40];
    Ok(String::from_utf8_lossy(raw).trim_end_matches(['\0', ' ']).to_string())
}

fn set_zone(d: &Device, zone: u8, c0: u8, c1: u8, c2: u8) -> Result<(), String> {
    let mut p = vec![0u8; d.len];
    p[0] = REPORT_ID;
    p[1] = zone;
    match zone {
        0x91 => p[3] = 0x02,
        0x92 => p[3] = 0x04,
        _ => p[2] = 1 << (zone - 0x20),
    }
    p[0x0B] = 1;    // static mode
    p[0x0C] = 0x5A; // full brightness; dimness comes from the color itself
    p[0x0E] = c0;
    p[0x0F] = c1;
    p[0x10] = c2;
    write(d, &mut p)
}

fn set_header(d: &Device, channel: u8, c0: u8, c1: u8, c2: u8) -> Result<(), String> {
    let mut sent = 0;
    while sent < LEDS_PER_HEADER {
        let n = LEDS_PER_PACKET.min(LEDS_PER_HEADER - sent);
        let start = sent * 3;
        let mut p = vec![0u8; d.len];
        p[0] = REPORT_ID;
        p[1] = channel;
        p[2] = start as u8;
        p[3] = (start >> 8) as u8;
        p[4] = (n * 3) as u8;
        for i in 0..n {
            p[5 + i * 3] = c0;
            p[6 + i * 3] = c1;
            p[7 + i * 3] = c2;
        }
        write(d, &mut p)?;
        sent += LEDS_PER_PACKET;
    }
    Ok(())
}

fn commit(d: &Device) -> Result<(), String> {
    send(d, &[0x28, 0xFF, 0x07])
}

fn send(d: &Device, body: &[u8]) -> Result<(), String> {
    let mut p = vec![0u8; d.len];
    p[0] = REPORT_ID;
    p[1..1 + body.len()].copy_from_slice(body);
    write(d, &mut p)
}

fn write(d: &Device, p: &mut [u8]) -> Result<(), String> {
    if unsafe { HidD_SetFeature(d.h, p.as_mut_ptr().cast(), p.len() as u32) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    std::thread::sleep(Duration::from_millis(2));
    Ok(())
}

/// Finds the HID interface with this vendor/product and usage page/usage.
fn open(vid: u16, pid: u16, usage_page: u16, usage: u16) -> Option<Device> {
    let want = format!("vid_{vid:04x}&pid_{pid:04x}");
    unsafe {
        let mut guid = std::mem::zeroed();
        HidD_GetHidGuid(&mut guid);
        let set = SetupDiGetClassDevsW(&guid, std::ptr::null(), std::ptr::null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set == INVALID_HANDLE_VALUE as HDEVINFO {
            return None;
        }
        let mut found = None;
        let mut data: SP_DEVICE_INTERFACE_DATA = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
        let mut i = 0;
        while found.is_none() && SetupDiEnumDeviceInterfaces(set, std::ptr::null(), &guid, i, &mut data) != 0 {
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
            if !path.to_lowercase().contains(&want) {
                continue;
            }
            let h = CreateFileW(
                wide(&path).as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            );
            if h == INVALID_HANDLE_VALUE {
                continue;
            }
            let dev = Device { h, len: 0 };
            let mut a: HIDD_ATTRIBUTES = std::mem::zeroed();
            a.Size = std::mem::size_of::<HIDD_ATTRIBUTES>() as u32;
            let mut pre: PHIDP_PREPARSED_DATA = 0;
            if HidD_GetAttributes(h, &mut a) != 0 && a.VendorID == vid && a.ProductID == pid && HidD_GetPreparsedData(h, &mut pre) != 0 {
                let mut caps: HIDP_CAPS = std::mem::zeroed();
                let status = HidP_GetCaps(pre, &mut caps);
                HidD_FreePreparsedData(pre);
                if status == HIDP_STATUS_SUCCESS && caps.UsagePage == usage_page && caps.Usage == usage {
                    found = Some(Device { h: dev.h, len: caps.FeatureReportByteLength as usize });
                    std::mem::forget(dev); // ownership moved into `found`
                }
            }
        }
        SetupDiDestroyDeviceInfoList(set);
        found
    }
}
