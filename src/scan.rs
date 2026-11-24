// A read-only look around the PC for RGB gear of every kind, naming the lighting protocol each
// piece uses where that can be told. Four places are checked:
//   USB/HID devices  - vendor/product IDs, matched against OpenRGB's device list
//   graphics cards   - PCI IDs: the card maker decides which RGB chip is on the card
//   RAM sticks       - SMBIOS part numbers (RAM lighting sits on the SMBus, which Windows
//                      only opens to a kernel driver, so the part number is all we can see)
//   motherboard      - SMBIOS maker and model
// Nothing here opens a device for writing or sends anything to one.
use crate::board;
use crate::hw::wide;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows_sys::Win32::System::SystemInformation::{GetSystemFirmwareTable, RSMB};

/// USB vendors known for RGB hardware. Their devices are listed even when OpenRGB doesn't know them.
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

/// Graphics card makers (PCI subsystem vendor) and the RGB protocol their cards use.
const GPU_MAKERS: [(u16, &str, &str); 14] = [
    (0x1043, "ASUS", "ASUS Aura GPU (ENE chip over I2C)"),
    (0x1462, "MSI", "MSI Mystic Light GPU (I2C)"),
    (0x1458, "Gigabyte", "Gigabyte RGB Fusion GPU (I2C)"),
    (0x3842, "EVGA", "EVGA GPU (I2C)"),
    (0x19DA, "Zotac", "Zotac Spectra (I2C)"),
    (0x1B4C, "Galax / KFA2", "Galax GPU (I2C)"),
    (0x1569, "Palit", "Palit GPU (I2C)"),
    (0x10B0, "Gainward", "Palit GPU (I2C)"),
    (0x196E, "PNY", "PNY GPU (I2C)"),
    (0x7377, "Colorful", "Colorful GPU (I2C)"),
    (0x1DA2, "Sapphire", "Sapphire Nitro Glow (I2C)"),
    (0x148C, "PowerColor", "PowerColor Red Devil (I2C)"),
    (0x1682, "XFX", "XFX GPU (I2C)"),
    (0x1849, "ASRock", "ASRock GPU (I2C)"),
];

/// Motherboard makers and their onboard lighting systems.
const BOARD_MAKERS: [(&str, &str); 6] = [
    ("ASUS", "ASUS Aura (USB controller or ENE on SMBus)"),
    ("GIGABYTE", "Gigabyte RGB Fusion 2 (ITE USB controller or SMBus)"),
    ("MICRO-STAR", "MSI Mystic Light (USB controller)"),
    ("MSI", "MSI Mystic Light (USB controller)"),
    ("ASROCK", "ASRock Polychrome (USB controller or SMBus)"),
    ("NZXT", "NZXT (USB controller)"),
];

pub struct Found {
    pub kind: &'static str, // "USB", "GPU", "RAM" or "Board"
    pub name: String,
    pub protocol: String,
    pub duce: bool,            // Duce can drive it
    pub openrgb: Option<bool>, // whether OpenRGB lists it; None when that can't be told
}

impl Found {
    pub fn line(&self) -> String {
        let tag = match self.openrgb {
            Some(true) => " (OpenRGB: yes)",
            Some(false) => " (OpenRGB: no)",
            None => "",
        };
        format!("{} - {}{tag}", self.name, self.protocol)
    }
}

/// Every RGB device found, in the order USB, GPU, RAM, board.
pub fn all() -> Vec<Found> {
    let mut out = usb();
    out.extend(gpus());
    let smbios = smbios();
    out.extend(ram(&smbios));
    let board_driven = out.iter().any(|f| f.kind == "USB" && f.duce);
    out.extend(board_info(&smbios, board_driven));
    out
}

/// What the window's Settings tab shows: up to ten devices Duce can't drive.
pub fn unsupported_rgb_devices() -> Vec<String> {
    let mut lines: Vec<String> = all().iter().filter(|f| !f.duce).map(|f| f.line()).take(10).collect();
    lines.sort();
    lines
}

fn usb() -> Vec<Found> {
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for (vid, pid, product) in hid_devices() {
        if seen.contains(&(vid, pid)) {
            continue;
        }
        let protocol = crate::openrgb_ids::protocol(vid, pid);
        let vendor = RGB_VENDORS.iter().find(|(v, _)| *v == vid).map(|(_, n)| *n);
        if protocol.is_none() && vendor.is_none() {
            continue; // an ordinary USB device
        }
        seen.push((vid, pid));
        let duce = vid == 0x048D && board::SUPPORTED_PIDS.contains(&pid);
        let name = match (product.is_empty(), vendor) {
            (false, _) => format!("{product} [{vid:04X}:{pid:04X}]"),
            (true, Some(v)) => format!("{v} device [{vid:04X}:{pid:04X}]"),
            (true, None) => format!("USB device [{vid:04X}:{pid:04X}]"),
        };
        let protocol = match protocol {
            Some(p) => p.to_string(),
            None => format!("unknown {} protocol", vendor.unwrap_or("vendor")),
        };
        out.push(Found { kind: "USB", name, protocol, duce, openrgb: Some(crate::openrgb_ids::protocol(vid, pid).is_some()) });
    }
    out
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

/// Graphics cards from their PCI hardware IDs (PCI\VEN_xxxx&DEV_xxxx&SUBSYS_ddddvvvv).
fn gpus() -> Vec<Found> {
    let mut out = Vec::new();
    unsafe {
        let set = SetupDiGetClassDevsW(&GUID_DEVCLASS_DISPLAY, std::ptr::null(), std::ptr::null_mut(), DIGCF_PRESENT);
        if set == INVALID_HANDLE_VALUE as HDEVINFO {
            return out;
        }
        let mut info: SP_DEVINFO_DATA = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
        let mut i = 0;
        while SetupDiEnumDeviceInfo(set, i, &mut info) != 0 {
            i += 1;
            let ids = property(set, &info, SPDRP_HARDWAREID);
            let desc = property(set, &info, SPDRP_DEVICEDESC);
            let Some((chip_vendor, subvendor)) = pci_ids(&ids) else { continue };
            if integrated(chip_vendor, &desc) {
                continue; // built into the CPU: it carries the board maker's ID but has no lights
            }
            let maker = GPU_MAKERS.iter().find(|m| m.0 == subvendor);
            let protocol = match maker {
                Some(m) => m.2.to_string(),
                None => format!("no known RGB (card maker {subvendor:04X})"),
            };
            let name = match maker {
                Some(m) => format!("{desc} ({})", m.1),
                None => desc,
            };
            // Duce drives ASUS NVIDIA cards; whether this one has the ENE chip is checked on use.
            let duce = chip_vendor == 0x10DE && subvendor == 0x1043;
            out.push(Found { kind: "GPU", name, protocol, duce, openrgb: None });
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}

unsafe fn property(set: HDEVINFO, info: &SP_DEVINFO_DATA, prop: u32) -> String {
    let mut buf = [0u16; 512];
    let mut needed = 0u32;
    if SetupDiGetDeviceRegistryPropertyW(set, info, prop, std::ptr::null_mut(), buf.as_mut_ptr().cast(), (buf.len() * 2) as u32, &mut needed) == 0 {
        return String::new();
    }
    // A multi-string for hardware IDs: the first entry is the most specific one.
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// CPU graphics: Intel apart from Arc cards, AMD apart from RX / Pro cards.
fn integrated(chip_vendor: u16, desc: &str) -> bool {
    let d = desc.to_ascii_uppercase();
    match chip_vendor {
        0x8086 => !d.contains("ARC"),
        0x1002 => !(d.contains(" RX") || d.contains(" PRO ") || d.contains("FURY") || d.contains("VII")),
        _ => false,
    }
}

/// (chip vendor, card maker) from "PCI\VEN_10DE&DEV_2C02&SUBSYS_89F51043&REV_A1".
fn pci_ids(id: &str) -> Option<(u16, u16)> {
    let up = id.to_ascii_uppercase();
    let field = |key: &str, len: usize| {
        let start = up.find(key)? + key.len();
        up.get(start..start + len).and_then(|s| u32::from_str_radix(s, 16).ok())
    };
    let ven = field("VEN_", 4)? as u16;
    let subsys = field("SUBSYS_", 8)?;
    Some((ven, subsys as u16))
}

/// SMBIOS structures as (type, formatted bytes, strings).
type Smbios = Vec<(u8, Vec<u8>, Vec<String>)>;

fn smbios() -> Smbios {
    let mut out = Vec::new();
    let raw = unsafe {
        let size = GetSystemFirmwareTable(RSMB, 0, std::ptr::null_mut(), 0);
        if size == 0 {
            return out;
        }
        let mut buf = vec![0u8; size as usize];
        if GetSystemFirmwareTable(RSMB, 0, buf.as_mut_ptr(), size) != size {
            return out;
        }
        buf
    };
    // 8-byte header (calling method, version, length), then the structures.
    let mut p = 8;
    while p + 4 <= raw.len() {
        let (kind, len) = (raw[p], raw[p + 1] as usize);
        if len < 4 || p + len > raw.len() {
            break;
        }
        let formatted = raw[p..p + len].to_vec();
        let mut q = p + len;
        let mut strings = Vec::new();
        // Strings end with an empty one; a structure with none still has two zero bytes.
        loop {
            let end = raw[q..].iter().position(|&b| b == 0).map(|e| q + e);
            let Some(end) = end else { return out };
            if end == q {
                q += 1;
                break;
            }
            strings.push(String::from_utf8_lossy(&raw[q..end]).trim().to_string());
            q = end + 1;
        }
        if strings.is_empty() {
            q += 1;
        }
        out.push((kind, formatted, strings));
        if kind == 127 {
            break;
        }
        p = q;
    }
    out
}

fn smbios_str(f: &[u8], s: &[String], offset: usize) -> String {
    match f.get(offset) {
        Some(&i) if i > 0 => s.get(i as usize - 1).cloned().unwrap_or_default(),
        _ => String::new(),
    }
}

/// RAM sticks whose part number marks them as an RGB model.
fn ram(smbios: &Smbios) -> Vec<Found> {
    let mut out = Vec::new();
    for (kind, f, s) in smbios {
        if *kind != 17 || f.len() < 0x1B {
            continue;
        }
        let size = u16::from_le_bytes([f[0x0C], f[0x0D]]);
        if size == 0 {
            continue; // empty slot
        }
        let slot = smbios_str(f, s, 0x10);
        let maker = smbios_str(f, s, 0x17);
        let part = smbios_str(f, s, 0x1A);
        let Some(protocol) = ram_protocol(&maker, &part) else { continue };
        let name = format!("{} {part} in {slot}", ram_maker_name(&maker));
        out.push(Found { kind: "RAM", name, protocol: protocol.into(), duce: false, openrgb: None });
    }
    out
}

/// SMBIOS often gives a JEDEC code instead of the maker's name.
fn ram_maker_name(maker: &str) -> &str {
    let m = maker.to_ascii_uppercase();
    match () {
        _ if m.contains("CORSAIR") || m.contains("029E") => "Corsair",
        _ if m.contains("G.SKILL") || m.contains("G SKILL") || m.contains("04CD") => "G.Skill",
        _ if m.contains("KINGSTON") || m.contains("0198") => "Kingston",
        _ if m.contains("TEAM") || m.contains("04EF") => "TeamGroup",
        _ if m.contains("ADATA") || m.contains("04CB") => "ADATA",
        _ if m.contains("PATRIOT") || m.contains("8502") => "Patriot",
        _ if m.contains("CRUCIAL") || m.contains("MICRON") || m.contains("859B") || m.contains("802C") => "Crucial",
        _ => maker,
    }
}

/// The lighting protocol of an RGB RAM model, judged from its part number; None = no RGB.
fn ram_protocol(maker: &str, part: &str) -> Option<&'static str> {
    let p = part.to_ascii_uppercase();
    let has = |keys: &[&str]| keys.iter().any(|k| p.contains(k));
    match ram_maker_name(maker) {
        // Vengeance RGB / RGB Pro / RGB RS / RGB RT, Dominator Platinum and Titanium RGB.
        "Corsair" if has(&["CMW", "CMH", "CMR", "CMT", "CMG", "CMN", "CMP"]) => Some("Corsair Vengeance/Dominator RGB (SMBus)"),
        // Trident Z RGB / Royal / Neo, Trident Z5 RGB / Neo RGB / CK.
        "G.Skill" if has(&["TZR", "TZ5R", "TZ5NR", "TZ5CR", "TZN", "GTRS", "GTRG", "TRS", "TZ5RK"]) => Some("ENE RAM RGB (SMBus)"),
        // Fury Beast / Renegade RGB end in BBA, RSA, BWA, RWA.
        "Kingston" if has(&["BBA", "RSA", "BWA", "RWA"]) => Some("Kingston Fury RGB (SMBus)"),
        "Patriot" if has(&["PVR", "PVSR", "PV5R"]) => Some("Patriot Viper RGB (SMBus)"),
        "TeamGroup" | "ADATA" | "Crucial" if has(&["RGB", "ARGB", "TF3D", "TF4D", "FF3D", "FF4D", "TF10D", "ARB"]) => Some("ENE RAM RGB (SMBus)"),
        _ if p.contains("RGB") => Some("RAM RGB, protocol unknown (SMBus)"),
        _ => None,
    }
}

/// The motherboard and the lighting system its maker uses.
fn board_info(smbios: &Smbios, ite_driven: bool) -> Vec<Found> {
    for (kind, f, s) in smbios {
        if *kind != 2 || f.len() < 6 {
            continue;
        }
        let maker = smbios_str(f, s, 0x04);
        let model = smbios_str(f, s, 0x05);
        let up = maker.to_ascii_uppercase();
        let protocol = BOARD_MAKERS
            .iter()
            .find(|(m, _)| up.contains(m))
            .map(|(_, p)| p.to_string())
            .unwrap_or_else(|| "no known onboard RGB".into());
        // A Gigabyte board's lighting is its ITE USB controller, so Duce drives it when it drives that.
        let duce = ite_driven && up.contains("GIGABYTE");
        return vec![Found { kind: "Board", name: format!("{maker} {model}"), protocol, duce, openrgb: None }];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pci_ids() {
        assert_eq!(pci_ids(r"PCI\VEN_10DE&DEV_2C02&SUBSYS_89F51043&REV_A1"), Some((0x10DE, 0x1043)));
        assert_eq!(pci_ids(r"ROOT\BasicDisplay"), None);
    }

    #[test]
    fn skips_cpu_graphics() {
        assert!(integrated(0x1002, "AMD Radeon(TM) Graphics"));
        assert!(!integrated(0x1002, "AMD Radeon RX 7900 XTX"));
        assert!(integrated(0x8086, "Intel(R) UHD Graphics 770"));
        assert!(!integrated(0x10DE, "NVIDIA GeForce RTX 5080"));
    }

    #[test]
    fn tells_rgb_ram() {
        assert!(ram_protocol("Corsair", "CMH32GX5M2B6000C30").is_some());
        assert!(ram_protocol("Corsair", "CMK32GX5M2B5600C36").is_none());
        assert!(ram_protocol("G Skill Intl", "F5-6000J3038F16GX2-TZ5RK").is_some());
        assert!(ram_protocol("Kingston", "KF552C40-16").is_none());
        assert!(ram_protocol("Kingston", "KF552C40BBA-16").is_some());
    }
}
