// SPDX-License-Identifier: GPL-2.0-or-later
// Credit: the OpenRGB project, https://gitlab.com/CalcProgrammer1/OpenRGB (GPL-2.0-or-later).
// ASUS NVIDIA cards (ROG Astral, Strix, TUF, ...): ENE lighting chip on the card's I2C bus,
// reached through NVIDIA's driver (NVAPI). The chip sits at address 0x67 on a port that varies
// per model, so known ports are scanned; the chip is accepted only if it states an ENE "AUMA"
// firmware name and a sane LED count.
// Safety: the card and chip are identified before any write, and every write passes through
// guarded_write, which only allows the direct-mode/apply flags and the verified color registers.
// The chip's save-to-flash command and its I2C address register can never be written.
use crate::log::log;
use std::ffi::c_void;
use std::time::Duration;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

const ASUS_VENDOR: u32 = 0x1043;
const CHIP_PREFIX: &str = "AUMA"; // ENE GPU firmware names ("AUMA0-E6K5-1113" on the Astral RTX 5080)
const ADDRESS: u8 = 0x67;
const PORTS: [u8; 4] = [1, 2, 3, 0]; // port 1 first: that's where the Astral has it
const REG_DIRECT: u16 = 0x8020;
const REG_APPLY: u16 = 0x80A0;
const REG_COLORS: u16 = 0x8100;
const REG_NAME: u16 = 0x1000;
const REG_CONFIG: u16 = 0x1C00;
const MAX_LEDS: usize = 64;

type QueryInterface = unsafe extern "C" fn(u32) -> *mut c_void;
type InitFn = unsafe extern "C" fn() -> i32;
type EnumFn = unsafe extern "C" fn(*mut *mut c_void, *mut u32) -> i32;
type PciFn = unsafe extern "C" fn(*mut c_void, *mut u32, *mut u32, *mut u32, *mut u32) -> i32;
type I2cFn = unsafe extern "C" fn(*mut c_void, *mut u8, *mut u32) -> i32;

fn query() -> Result<QueryInterface, String> {
    unsafe {
        let lib = LoadLibraryA(b"nvapi64.dll\0".as_ptr());
        if lib.is_null() {
            return Err("NVIDIA driver (nvapi64.dll) not found".into());
        }
        match GetProcAddress(lib, b"nvapi_QueryInterface\0".as_ptr()) {
            Some(f) => Ok(std::mem::transmute::<_, QueryInterface>(f)),
            None => Err("nvapi_QueryInterface missing".into()),
        }
    }
}

fn function<T: Copy>(qi: QueryInterface, id: u32) -> Result<T, String> {
    let p = unsafe { qi(id) };
    if p.is_null() {
        return Err(format!("NVAPI function missing: 0x{id:08X}"));
    }
    Ok(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&p) })
}

fn check(status: i32, what: &str) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("Could not {what} (NVAPI status {status})")) }
}

struct Bus {
    gpu: *mut c_void,
    write: I2cFn,
    read: I2cFn,
    info: Box<[u64; 8]>, // NV_I2C_INFO_V3, 64 bytes
    reg: Box<[u8; 8]>,
    data: Box<[u8; 64]>,
    port: u8,
    led_count: usize, // the only color registers writes may reach; set once the chip is verified
}

impl Bus {
    /// Finds an ASUS NVIDIA card and the ENE chip on it, trying each known I2C port.
    /// Reads only; nothing is written until the chip has identified itself.
    fn new() -> Result<Bus, String> {
        let qi = query()?;
        unsafe {
            check(function::<InitFn>(qi, 0x0150_E828)?(), "initialize NVAPI")?;
            let write = function::<I2cFn>(qi, 0x283A_C65A)?;
            let read = function::<I2cFn>(qi, 0x4D7B_0709)?;
            let mut handles = [std::ptr::null_mut::<c_void>(); 64];
            let mut count = 0u32;
            check(function::<EnumFn>(qi, 0xE5AC_921F)?(handles.as_mut_ptr(), &mut count), "list GPUs")?;
            let pci = function::<PciFn>(qi, 0x2DDF_B66E)?;
            let mut cards = Vec::new();
            for &h in handles.iter().take(count.min(64) as usize) {
                let (mut device, mut subsystem, mut revision, mut ext) = (0, 0, 0, 0);
                if pci(h, &mut device, &mut subsystem, &mut revision, &mut ext) == 0
                    && subsystem & 0xFFFF == ASUS_VENDOR
                {
                    cards.push(h);
                }
            }
            if cards.is_empty() {
                return Err("No ASUS NVIDIA graphics card found".into());
            }
            let mut last = "no answer on any I2C port".to_string();
            for gpu in cards {
                for port in PORTS {
                    let mut bus = Bus { gpu, write, read, info: Box::new([0; 8]), reg: Box::new([0; 8]), data: Box::new([0; 64]), port, led_count: 0 };
                    match bus.chip_name() {
                        Ok(chip) if chip.starts_with(CHIP_PREFIX) => return Ok(bus),
                        Ok(chip) if !chip.is_empty() => last = format!("unexpected chip '{chip}' on port {port}"),
                        _ => {}
                    }
                }
            }
            Err(format!("The card's lighting chip was not found ({last})"))
        }
    }

    fn prepare(&mut self, register: u8, size: u32) {
        *self.info = [0; 8];
        self.reg[0] = register;
        let reg = self.reg.as_mut_ptr();
        let data = self.data.as_mut_ptr();
        let p = self.info.as_mut_ptr() as *mut u8;
        unsafe {
            (p as *mut u32).write_unaligned((3 << 16) | 64); // version 3, 64-byte struct
            *p.add(9) = ADDRESS << 1;
            (p.add(16) as *mut *mut u8).write_unaligned(reg);
            (p.add(24) as *mut u32).write_unaligned(1); // one register-address byte
            (p.add(32) as *mut *mut u8).write_unaligned(data);
            (p.add(40) as *mut u32).write_unaligned(size);
            (p.add(44) as *mut u32).write_unaligned(0xFFFF); // speed: use default
            *p.add(52) = self.port;
            (p.add(56) as *mut u32).write_unaligned(1); // port is set
        }
    }

    fn send(&mut self, register: u8, bytes: &[u8]) -> Result<(), String> {
        self.prepare(register, bytes.len() as u32);
        self.data[..bytes.len()].copy_from_slice(bytes);
        let mut unknown = 0u32;
        check(unsafe { (self.write)(self.gpu, self.info.as_mut_ptr() as *mut u8, &mut unknown) }, "I2C write")?;
        std::thread::sleep(Duration::from_millis(1));
        Ok(())
    }

    fn point(&mut self, register: u16) -> Result<(), String> {
        self.send(0x00, &[(register >> 8) as u8, register as u8])
    }

    fn read_reg(&mut self, register: u16) -> Result<u8, String> {
        self.point(register)?;
        self.prepare(0x81, 1);
        let mut unknown = 0u32;
        check(unsafe { (self.read)(self.gpu, self.info.as_mut_ptr() as *mut u8, &mut unknown) }, "I2C read")?;
        Ok(self.data[0])
    }

    /// Every lighting write passes through here. Anything else is refused.
    fn guarded_write(&mut self, register: u16, value: u8) -> Result<(), String> {
        let control = (register == REG_DIRECT || register == REG_APPLY) && value == 1;
        let color = self.led_count > 0
            && register >= REG_COLORS
            && (register as usize) < REG_COLORS as usize + self.led_count * 3;
        if !control && !color {
            return Err(format!("Blocked GPU write to 0x{register:04X}"));
        }
        self.point(register)?;
        self.send(0x01, &[value])
    }

    fn chip_name(&mut self) -> Result<String, String> {
        let mut name = [0u8; 16];
        for (i, c) in name.iter_mut().enumerate() {
            *c = self.read_reg(REG_NAME + i as u16)?;
        }
        Ok(String::from_utf8_lossy(&name).trim_end_matches(['\0', ' ']).to_string())
    }

    fn verify(&mut self) -> Result<String, String> {
        let chip = self.chip_name()?;
        if !chip.starts_with(CHIP_PREFIX) {
            return Err(format!("Unexpected GPU RGB chip: '{chip}'"));
        }
        let leds = self.read_reg(REG_CONFIG + 3)? as usize;
        if !(1..=MAX_LEDS).contains(&leds) {
            return Err(format!("Unexpected GPU LED count: {leds}"));
        }
        self.led_count = leds;
        Ok(format!("{chip}, {leds} LEDs, port {}", self.port))
    }

    fn set_all(&mut self, r: u8, g: u8, b: u8) -> Result<usize, String> {
        self.guarded_write(REG_DIRECT, 1)?;
        self.guarded_write(REG_APPLY, 1)?;
        let order = [r, b, g]; // this chip takes red, blue, green
        let n = self.led_count * 3;
        for i in 0..n {
            self.guarded_write(REG_COLORS + i as u16, order[i % 3])?;
        }
        let mut wrong = 0;
        for i in 0..n {
            if self.read_reg(REG_COLORS + i as u16)? != order[i % 3] {
                wrong += 1;
            }
        }
        Ok(wrong)
    }
}

pub fn identify() -> Result<String, String> {
    Bus::new()?.verify()
}

pub fn apply(r: u8, g: u8, b: u8, wait: bool) -> Result<(), String> {
    // At login the NVIDIA driver may need a moment.
    let mut attempt = 0;
    loop {
        let result = Bus::new().and_then(|mut bus| {
            let id = bus.verify()?;
            let wrong = bus.set_all(r, g, b)?;
            log(&format!("GPU: applied #{r:02X}{g:02X}{b:02X} on {id}; readback mismatches: {wrong}"));
            if wrong != 0 {
                return Err(format!("{wrong} color bytes did not read back correctly"));
            }
            Ok(())
        });
        match result {
            Err(e) if e.starts_with("Blocked") => return Err(e),
            Err(_) if wait && attempt < 10 => {
                attempt += 1;
                std::thread::sleep(Duration::from_secs(3));
            }
            other => return other,
        }
    }
}
