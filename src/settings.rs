// Same settings.txt format as the old C# MiniRGB, so the two copies understand each other's files.
use crate::log::{dir, log};

pub const MAX_CUSTOM_THEMES: usize = 6;
pub const THEME_IDS: [&str; 8] = ["system", "light", "dark", "black", "midnight", "forest", "plum", "sand"];

#[derive(Clone)]
pub struct RgbTheme {
    pub name: String,
    pub fans: u32,
    pub gpu: u32,
}

/// A color of -1 means Duce has never set that controller, so it is left alone.
#[derive(Clone)]
pub struct Settings {
    pub fans_rgb: i32,
    pub gpu_rgb: i32,
    pub accent: usize,
    pub brightness: i32,
    pub select_fans: bool,
    pub select_gpu: bool,
    pub start_at_login: bool,
    pub theme_mode: String,
    pub custom_themes: Vec<RgbTheme>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            fans_rgb: -1,
            gpu_rgb: -1,
            accent: 0,
            brightness: 100,
            select_fans: true,
            select_gpu: true,
            start_at_login: true,
            theme_mode: "system".into(),
            custom_themes: Vec::new(),
        }
    }
}

/// Saved colors stay as picked; brightness only dims what gets sent to the lights.
pub fn dim(rgb: i32, brightness: i32) -> i32 {
    if rgb < 0 {
        return -1;
    }
    let f = |c: i32| c * brightness / 100;
    f(rgb >> 16 & 255) << 16 | f(rgb >> 8 & 255) << 8 | f(rgb & 255)
}

fn path() -> std::path::PathBuf {
    dir().join("settings.txt")
}

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim(), 16).ok().map(|v| v & 0xFFFFFF)
}

fn color(s: &str) -> Option<i32> {
    if s == "none" { Some(-1) } else { hex(s).map(|v| v as i32) }
}

pub fn load() -> Settings {
    let mut s = Settings::default();
    let Ok(text) = std::fs::read_to_string(path()) else { return s };
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() >= 3 && !lines[0].contains('=') {
        // Older format: one color, then whether fans and GPU used it.
        if let Some(rgb) = hex(lines[0]) {
            s.select_fans = lines[1] == "1";
            s.select_gpu = lines[2] == "1";
            if s.select_fans { s.fans_rgb = rgb as i32; }
            if s.select_gpu { s.gpu_rgb = rgb as i32; }
        }
        return s;
    }
    for line in lines {
        let Some((key, value)) = line.split_once('=') else { continue };
        let good = match key {
            "fans" => color(value).map(|v| s.fans_rgb = v).is_some(),
            "gpu" => color(value).map(|v| s.gpu_rgb = v).is_some(),
            "selectFans" => { s.select_fans = value == "1"; true }
            "selectGpu" => { s.select_gpu = value == "1"; true }
            "theme" => { if THEME_IDS.contains(&value) { s.theme_mode = value.into(); } true }
            "accent" => value.parse::<usize>().map(|v| s.accent = v.min(4)).is_ok(),
            "brightness" => value.parse::<i32>().map(|v| s.brightness = v.clamp(0, 100)).is_ok(),
            "startAtLogin" => { s.start_at_login = value == "1"; true }
            "rgbTheme" if s.custom_themes.len() < MAX_CUSTOM_THEMES => {
                let parts: Vec<&str> = value.split('|').collect();
                match (parts.len() == 3, parts.get(1).and_then(|p| hex(p)), parts.get(2).and_then(|p| hex(p))) {
                    (true, Some(fans), Some(gpu)) if !parts[0].trim().is_empty() => {
                        s.custom_themes.push(RgbTheme { name: parts[0].trim().into(), fans, gpu });
                        true
                    }
                    _ => false,
                }
            }
            _ => true,
        };
        if !good {
            log(&format!("Ignored bad setting: {line}"));
        }
    }
    s
}

static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn save(s: &Settings) -> std::io::Result<()> {
    let c = |v: i32| if v < 0 { "none".to_string() } else { format!("{v:06X}") };
    let b = |v: bool| if v { "1" } else { "0" };
    let mut text = format!(
        "fans={}\r\ngpu={}\r\nselectFans={}\r\nselectGpu={}\r\ntheme={}\r\naccent={}\r\nbrightness={}\r\nstartAtLogin={}\r\n",
        c(s.fans_rgb), c(s.gpu_rgb), b(s.select_fans), b(s.select_gpu), s.theme_mode, s.accent, s.brightness, b(s.start_at_login)
    );
    for t in &s.custom_themes {
        text += &format!("rgbTheme={}|{:06X}|{:06X}\r\n", t.name, t.fans, t.gpu);
    }
    // Write then swap, so a power cut mid-save can't leave a damaged settings file.
    let _held = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp = path().with_extension("txt.tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path())
}
