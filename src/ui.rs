// Duce's window: choose which controllers change, pick a color, RGB themes, and app settings.
// A color is applied to the selected controllers shortly after it stops changing.
//
// Layout and look are all in this file:
//   Look      -> the PALETTES / ACCENTS tables and `apply_style`
//   Color    -> `color_tab` (picker, devices, theme dropdown)     Settings -> `settings_tab`
//   Widgets   -> `segmented`, `bubble`, `swatch`, `toggle_row`, `theme_row`, `app_theme_card`
use crate::hw::{self, wide};
use crate::settings::{self, dim, RgbTheme, Settings, MAX_CUSTOM_THEMES};
use crate::{board, gpu, log::log, scan};
use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, FontId, Mesh, Pos2, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, Ui,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const WIDTH: f32 = 300.0;
const HEIGHT: f32 = 660.0;
const PAD: f32 = 16.0;
const INNER: f32 = WIDTH - PAD * 2.0;
// Wide (side-by-side) layout, used when the window is at least WIDE_MIN wide.
const WIDE_MIN: f32 = 560.0;
const MAX_WIDTH: f32 = 900.0; // content stops growing past this and stays centered
const RIGHT_W: f32 = 280.0; // controls column
const GAP_W: f32 = 28.0; // space between columns
const CONTROLS_H: f32 = 310.0; // rough height of the controls column
const HUE_H: f32 = 16.0; // hue bar
const HEX_ROW_H: f32 = 32.0; // # code row
const TABS_W: f32 = 280.0; // two tabs

/// How the window is laid out this frame, from its size.
#[derive(Clone, Copy)]
struct Layout {
    wide: bool,
    width: f32,  // content width
    picker: f32, // height of the color picker block (square, hue bar, # row)
    height: f32, // height the content is centered with
}

fn layout_for(area: Rect) -> Layout {
    if area.width() < WIDE_MIN {
        return Layout { wide: false, width: INNER, picker: 232.0, height: HEIGHT };
    }
    let width = (area.width() - 2.0 * PAD).min(MAX_WIDTH);
    let picker = (area.height() - 2.0 * PAD - 48.0).clamp(260.0, 520.0);
    Layout { wide: true, width, picker, height: 2.0 * PAD + 48.0 + picker.max(CONTROLS_H) }
}
const DEBOUNCE: Duration = Duration::from_millis(250);

// ---------------------------------------------------------------- look
struct Palette {
    id: &'static str,
    name: &'static str,
    dark: bool,
    bg: u32,
    surface: u32,
    field: u32,
    border: u32,
    text: u32,
    muted: u32,
    good: u32,
    bad: u32,
}

#[rustfmt::skip]
const PALETTES: [Palette; 7] = [
    Palette { id: "light",    name: "Light",    dark: false, bg: 0xF5F5F7, surface: 0xFFFFFF, field: 0xFFFFFF, border: 0xC9C9CF, text: 0x1C1C1E, muted: 0x62626A, good: 0x147A42, bad: 0xC62828 },
    Palette { id: "dark",     name: "Dark",     dark: true,  bg: 0x1C1C1E, surface: 0x2A2A2D, field: 0x333337, border: 0x48484D, text: 0xF2F2F4, muted: 0xA8A8AF, good: 0x6EDC9A, bad: 0xFF8A80 },
    Palette { id: "black",    name: "Black",    dark: true,  bg: 0x000000, surface: 0x141416, field: 0x1C1C1F, border: 0x3A3A3F, text: 0xF5F5F5, muted: 0xA0A0A8, good: 0x6EDC9A, bad: 0xFF8A80 },
    Palette { id: "midnight", name: "Midnight", dark: true,  bg: 0x0F172A, surface: 0x1E293B, field: 0x273449, border: 0x475569, text: 0xF1F5F9, muted: 0xA3B1C6, good: 0x6EE7A8, bad: 0xFCA5A5 },
    Palette { id: "forest",   name: "Forest",   dark: true,  bg: 0x14201A, surface: 0x1E2E26, field: 0x27392F, border: 0x47604F, text: 0xEAF3EC, muted: 0xA5BBAB, good: 0x7FE0A0, bad: 0xFF9C8F },
    Palette { id: "plum",     name: "Plum",     dark: true,  bg: 0x1F1626, surface: 0x2D2136, field: 0x382A43, border: 0x5A4868, text: 0xF3ECF7, muted: 0xB8A9C2, good: 0x7FE0A0, bad: 0xFF9C8F },
    Palette { id: "sand",     name: "Sand",     dark: false, bg: 0xF6F1E7, surface: 0xFFFBF2, field: 0xFFFBF2, border: 0xD3C8B2, text: 0x2B2418, muted: 0x6B5F4B, good: 0x1F6B3A, bad: 0xB3261E },
];

const ACCENTS: [(u32, &str); 5] =
    [(0x3B82F6, "Blue"), (0x8B5CF6, "Purple"), (0x22C55E, "Green"), (0xF97316, "Orange"), (0xEC4899, "Pink")];

const SWATCHES: [u32; 8] = [0x000000, 0xFF0000, 0xFFFF00, 0x00FF00, 0x00FFFF, 0x0000FF, 0xFF00FF, 0xFFFFFF];

fn built_in_themes() -> [RgbTheme; 6] {
    let t = |name: &str, fans, gpu| RgbTheme { name: name.into(), fans, gpu };
    [
        t("Ember", 0xFF2D00, 0xFF8A00),
        t("Ocean", 0x0066FF, 0x00D5FF),
        t("Aurora", 0x00FF88, 0x8B00FF),
        t("Synthwave", 0xFF00C8, 0x00E5FF),
        t("Ice", 0xFFFFFF, 0x88CCFF),
        t("Lights off", 0x000000, 0x000000),
    ]
}

fn st(width: f32, color: Color32) -> Stroke {
    Stroke::new(width, color)
}

fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// The colors in use right now, resolved from the chosen palette and accent.
#[derive(Clone, Copy)]
struct Th {
    dark: bool,
    bg: Color32,
    surface: Color32,
    field: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    good: Color32,
    bad: Color32,
    accent: Color32,
}

fn system_is_dark() -> bool {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut value = 1u32;
    let mut size = 4u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize").as_ptr(),
            wide("AppsUseLightTheme").as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    } == 0;
    ok && value == 0
}

fn palette_for(mode: &str) -> &'static Palette {
    let id = if mode == "system" { if system_is_dark() { "dark" } else { "light" } } else { mode };
    PALETTES.iter().find(|p| p.id == id).unwrap_or(&PALETTES[1])
}

fn resolve(mode: &str, accent: usize) -> Th {
    let p = palette_for(mode);
    Th {
        dark: p.dark,
        bg: rgb(p.bg),
        surface: rgb(p.surface),
        field: rgb(p.field),
        border: rgb(p.border),
        text: rgb(p.text),
        muted: rgb(p.muted),
        good: rgb(p.good),
        bad: rgb(p.bad),
        accent: rgb(ACCENTS[accent.min(ACCENTS.len() - 1)].0),
    }
}

fn apply_style(ctx: &egui::Context, th: &Th) {
    let mut v = if th.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = th.bg;
    v.window_fill = th.surface;
    v.extreme_bg_color = th.field;
    v.faint_bg_color = th.surface;
    v.override_text_color = Some(th.text);
    v.selection.bg_fill = th.accent;
    v.selection.stroke = st(1.0, Color32::WHITE);
    v.slider_trailing_fill = true;
    v.text_cursor.stroke = st(1.5, th.accent);
    let r = CornerRadius::same(7);
    for (w, fill, stroke) in [
        (&mut v.widgets.noninteractive, th.bg, th.border),
        (&mut v.widgets.inactive, th.field, th.border),
        (&mut v.widgets.hovered, th.field, th.accent),
        (&mut v.widgets.active, th.field, th.accent),
        (&mut v.widgets.open, th.field, th.accent),
    ] {
        w.corner_radius = r;
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = st(1.0, stroke);
        w.fg_stroke = st(1.0, th.text);
        w.expansion = 0.0;
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = vec2(8.0, 8.0);
        s.spacing.interact_size.y = 28.0;
        s.spacing.button_padding = vec2(10.0, 5.0);
        use egui::TextStyle::*;
        s.text_styles.insert(Body, FontId::proportional(14.0));
        s.text_styles.insert(Button, FontId::proportional(14.0));
        s.text_styles.insert(Small, FontId::proportional(12.0));
        s.text_styles.insert(Heading, FontId::proportional(15.0));
    });
}

fn load_fonts(ctx: &egui::Context) {
    // Segoe UI, Windows' own font, when it's there; egui's built-in fonts stay as a fallback.
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "segoe".into());
    }
    ctx.set_fonts(fonts);
}

// ---------------------------------------------------------------- color math
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> u32 {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    to(r) << 16 | to(g) << 8 | to(b)
}

/// Keeps the previous hue for grays, so the picker's marker doesn't jump.
fn rgb_to_hsv(c: u32, hue: f32) -> (f32, f32, f32) {
    let r = (c >> 16 & 255) as f32 / 255.0;
    let g = (c >> 8 & 255) as f32 / 255.0;
    let b = (c & 255) as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let mut h = hue;
    if d > 0.0 {
        h = if max == r { 60.0 * (((g - b) / d) % 6.0) } else if max == g { 60.0 * ((b - r) / d + 2.0) } else { 60.0 * ((r - g) / d + 4.0) };
        if h < 0.0 {
            h += 360.0;
        }
    }
    (h, if max > 0.0 { d / max } else { 0.0 }, max)
}

// ---------------------------------------------------------------- state
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Muted,
    Good,
    Bad,
}

struct Outcome {
    fans: i32,
    gpu: i32,
    level: i32,
    fans_ok: bool,
    gpu_ok: bool,
    errors: String,
}

pub struct Gui {
    s: Settings,
    th: Th,
    theme_checked: Instant,
    tab: usize,
    fans_found: Option<bool>, // None while still checking
    gpu_found: Option<bool>,
    detect: Option<JoinHandle<(bool, bool, Vec<String>)>>,
    // NVIDIA's driver sometimes refuses I2C for a while (e.g. right after boot), so a GPU
    // that didn't answer is re-checked quietly in the background for about a minute.
    gpu_recheck: Option<JoinHandle<bool>>,
    gpu_recheck_at: Option<Instant>,
    gpu_rechecks: u32,
    others: Vec<String>,    // RGB devices that were seen but aren't supported
    confirm_uninstall: bool, // the Remove button was clicked once
    current: u32,
    h: f32,
    sat: f32,
    val: f32,
    hex_text: String,
    want_fans: i32, // colors waiting to be sent; -1 = nothing to send
    want_gpu: i32,
    sent_fans: i32, // what the lights were last sent, after dimming
    sent_gpu: i32,
    dirty_at: Option<Instant>,
    worker: Option<JoinHandle<Outcome>>,
    status: (String, Kind),
    theme_name: String,
    saving_theme: bool, // the theme row shows a name field instead of the dropdown
    saving_focus: bool, // give that field the keyboard once, when it appears
    theme_dd_rect: Rect,         // the closed theme dropdown, as last drawn
    theme_save_rect: Rect,       // the Save… button next to it
    theme_item_rects: Vec<Rect>, // rows inside the open dropdown, built-ins first
    sv_tex: Option<(f32, egui::TextureHandle)>, // color square for this hue
    hue_tex: Option<egui::TextureHandle>,
    picker_rects: [Rect; 2], // color square and hue bar, as last drawn
    bubble_rects: Vec<Rect>, // where the device bubbles were last drawn: All first
    layout: Layout,
    controls_rect: Rect, // where the color controls were last drawn
}

impl Gui {
    fn new(ctx: egui::Context) -> Self {
        load_fonts(&ctx);
        let mut gui = Gui::with_settings(settings::load(), &ctx);

        // Ask each controller to identify itself, so hardware this PC doesn't have is shown as
        // unavailable, and look around for RGB gear Duce can't drive. Autostart and the Start menu
        // shortcut are only set up once something supported was actually found (see poll),
        // so Duce leaves no trace on a PC it can't help.
        let detect = std::thread::spawn(|| {
            let probe = |r: Result<String, String>| r.map_err(|e| log(&format!("Not available: {e}"))).is_ok();
            let fans = probe(hw::identify("Fans", board::identify));
            let gpu = probe(hw::identify("GPU", gpu::identify));
            (fans, gpu, scan::unsupported_rgb_devices())
        });
        gui.detect = Some(detect);
        gui
    }

    /// Called once something supported was found: hook Duce into login.
    fn install_on_this_pc(&self) {
        register_startup();
        ensure_start_menu_shortcut();
        // Versions before 1.1 kept a background helper for lights-off at shutdown;
        // the BIOS does that now, so end a leftover one.
        stop_old_helper();
    }

    /// The window's state with no hardware checks or helper start, so tests can drive it.
    fn with_settings(s: Settings, ctx: &egui::Context) -> Self {
        let th = resolve(&s.theme_mode, s.accent);
        apply_style(ctx, &th);
        let start = if s.select_fans && s.fans_rgb >= 0 { s.fans_rgb }
            else if s.select_gpu && s.gpu_rgb >= 0 { s.gpu_rgb }
            else if s.fans_rgb >= 0 { s.fans_rgb }
            else if s.gpu_rgb >= 0 { s.gpu_rgb }
            else { 0xFF0000 };
        let mut gui = Gui {
            sent_fans: dim(s.fans_rgb, s.brightness),
            sent_gpu: dim(s.gpu_rgb, s.brightness),
            s,
            th,
            theme_checked: Instant::now(),
            // For screenshots while restyling: DUCE_TAB=0/1 opens on that tab.
            tab: std::env::var("DUCE_TAB").ok().and_then(|t| t.parse().ok()).unwrap_or(0).min(1),
            fans_found: None,
            gpu_found: None,
            detect: None,
            gpu_recheck: None,
            gpu_recheck_at: None,
            gpu_rechecks: 0,
            others: Vec::new(),
            confirm_uninstall: false,
            current: 0,
            h: 0.0,
            sat: 0.0,
            val: 0.0,
            hex_text: String::new(),
            want_fans: -1,
            want_gpu: -1,
            dirty_at: None,
            worker: None,
            status: (String::new(), Kind::Muted),
            theme_name: String::new(),
            saving_theme: false,
            saving_focus: false,
            theme_dd_rect: Rect::NOTHING,
            theme_save_rect: Rect::NOTHING,
            theme_item_rects: Vec::new(),
            sv_tex: None,
            hue_tex: None,
            picker_rects: [Rect::NOTHING; 2],
            bubble_rects: Vec::new(),
            layout: layout_for(Rect::from_min_size(Pos2::ZERO, vec2(WIDTH, HEIGHT))),
            controls_rect: Rect::NOTHING,
        };
        gui.load_color(start as u32);
        gui
    }

    fn set_status(&mut self, text: impl Into<String>, kind: Kind) {
        self.status = (text.into(), kind);
    }

    fn save(&mut self) {
        if let Err(e) = settings::save(&self.s) {
            log(&format!("Could not save settings: {e}"));
        }
    }

    fn restyle(&mut self, ctx: &egui::Context) {
        self.th = resolve(&self.s.theme_mode, self.s.accent);
        apply_style(ctx, &self.th);
    }

    // ---------- color state
    fn load_color(&mut self, c: u32) {
        self.current = c;
        let (h, s, v) = rgb_to_hsv(c, self.h);
        self.h = h;
        self.sat = s;
        self.val = v;
        self.hex_text = format!("{c:06X}");
    }

    fn from_rgb(&mut self, c: u32) {
        self.load_color(c);
        self.color_changed();
    }

    fn from_hsv(&mut self) {
        self.current = hsv_to_rgb(self.h, self.sat, self.val);
        self.hex_text = format!("{:06X}", self.current);
        self.color_changed();
    }

    fn color_changed(&mut self) {
        if !self.fans_on() && !self.gpu_on() {
            self.set_status("Pick a device above to change it.", Kind::Muted);
        }
        if self.fans_on() {
            self.want_fans = self.current as i32;
        }
        if self.gpu_on() {
            self.want_gpu = self.current as i32;
        }
        self.dirty_at = Some(Instant::now());
    }

    // ---------- which controllers change
    fn fans_on(&self) -> bool {
        self.s.select_fans && self.fans_found != Some(false)
    }
    fn gpu_on(&self) -> bool {
        self.s.select_gpu && self.gpu_found != Some(false)
    }

    fn selection_changed(&mut self) {
        self.save();
        // A color still waiting to be sent goes to the controllers it was picked for.
        if self.dirty_at.take().is_some() {
            self.start_apply();
        }
        // With one controller selected, show the color it has now. Nothing changes until a color is picked.
        let only = if self.fans_on() && !self.gpu_on() { self.s.fans_rgb }
            else if self.gpu_on() && !self.fans_on() { self.s.gpu_rgb }
            else { -1 };
        if only >= 0 {
            self.load_color(only as u32);
        }
        if !self.fans_on() && !self.gpu_on() {
            self.set_status("Pick a device above to change it.", Kind::Muted);
        } else if self.fans_on() && self.gpu_on() && self.s.fans_rgb != self.s.gpu_rgb {
            self.set_status("They have different colors. Pick one to match them.", Kind::Muted);
        } else {
            self.set_status("", Kind::Muted);
        }
    }

    // ---------- applying
    /// Takes what's waiting and clears it, so a color that fails isn't retried on its own.
    fn take_wanted(&mut self) -> (i32, i32) {
        let level = self.s.brightness;
        let pick = |want: i32, found: Option<bool>, saved: i32, sent: i32| {
            if found == Some(true) && want >= 0 && (want != saved || dim(want, level) != sent) { want } else { -1 }
        };
        let fans = pick(self.want_fans, self.fans_found, self.s.fans_rgb, self.sent_fans);
        let gpu = pick(self.want_gpu, self.gpu_found, self.s.gpu_rgb, self.sent_gpu);
        self.want_fans = -1;
        self.want_gpu = -1;
        (fans, gpu)
    }

    fn start_apply(&mut self) {
        if self.worker.is_some() || self.detect.is_some() {
            // Busy; try again shortly.
            self.dirty_at = Some(Instant::now());
            return;
        }
        let (fans, gpu) = self.take_wanted();
        if fans < 0 && gpu < 0 {
            return;
        }
        self.set_status("Applying...", Kind::Muted);
        let level = self.s.brightness;
        self.worker = Some(std::thread::spawn(move || apply_now(fans, gpu, level)));
    }

    fn finish(&mut self, o: Outcome) {
        if o.fans_ok {
            self.s.fans_rgb = o.fans;
            self.sent_fans = dim(o.fans, o.level);
        }
        if o.gpu_ok {
            self.s.gpu_rgb = o.gpu;
            self.sent_gpu = dim(o.gpu, o.level);
        }
        self.save();
        if !o.errors.is_empty() {
            self.set_status(o.errors.trim_end().to_string(), Kind::Bad);
            return;
        }
        let at = if o.level < 100 { format!(" at {}%.", o.level) } else { ".".into() };
        let msg = match (o.fans >= 0, o.gpu >= 0) {
            (true, true) if o.fans != o.gpu => format!("Applied #{:06X} to fans and #{:06X} to graphics card{at}", o.fans, o.gpu),
            (true, true) => format!("Applied #{:06X} to fans and graphics card{at}", o.fans),
            (true, false) => format!("Applied #{:06X} to fans{at}", o.fans),
            _ => format!("Applied #{:06X} to graphics card{at}", o.gpu),
        };
        self.set_status(msg, Kind::Good);
    }

    fn poll(&mut self) {
        if self.detect.as_ref().is_some_and(|d| d.is_finished()) {
            let (fans, gpu, others) = self.detect.take().unwrap().join().unwrap_or((false, false, Vec::new()));
            self.fans_found = Some(fans);
            self.gpu_found = Some(gpu);
            self.others = others;
            if fans || gpu {
                self.install_on_this_pc();
            } else {
                self.set_status("No supported controllers were found on this PC.", Kind::Bad);
            }
            if !gpu {
                self.gpu_recheck_at = Some(Instant::now() + Duration::from_secs(2));
            }
        }
        if self.gpu_recheck_at.is_some_and(|t| Instant::now() >= t) && self.gpu_recheck.is_none() {
            self.gpu_recheck_at = None;
            self.gpu_rechecks += 1;
            self.gpu_recheck = Some(std::thread::spawn(|| hw::identify_once(gpu::identify).is_ok()));
        }
        if self.gpu_recheck.as_ref().is_some_and(|r| r.is_finished()) {
            let found = self.gpu_recheck.take().unwrap().join().unwrap_or(false);
            if found {
                log("GPU found on a later check");
                self.gpu_found = Some(true);
                if self.fans_found != Some(true) {
                    self.install_on_this_pc();
                }
                // It probably missed its color too; send the saved one.
                if self.s.gpu_rgb >= 0 && self.want_gpu < 0 {
                    self.want_gpu = self.s.gpu_rgb;
                    self.dirty_at = Some(Instant::now());
                }
            } else if self.gpu_rechecks < 30 {
                self.gpu_recheck_at = Some(Instant::now() + Duration::from_secs(2));
            } else {
                log("GPU still not answering after a minute of re-checks");
            }
        }
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            if let Ok(o) = self.worker.take().unwrap().join() {
                self.finish(o);
            }
            // Something else was picked while that was running.
            if (self.want_fans >= 0 || self.want_gpu >= 0) && self.dirty_at.is_none() {
                self.start_apply();
            }
        }
        if self.dirty_at.is_some_and(|t| t.elapsed() >= DEBOUNCE) {
            self.dirty_at = None;
            self.save();
            self.start_apply();
        }
    }

    fn brightness_changed(&mut self) {
        // Re-send each controller's saved color at the new level, unless a new color is already waiting.
        if self.want_fans < 0 && self.s.fans_rgb >= 0 {
            self.want_fans = self.s.fans_rgb;
        }
        if self.want_gpu < 0 && self.s.gpu_rgb >= 0 {
            self.want_gpu = self.s.gpu_rgb;
        }
        self.dirty_at = Some(Instant::now());
    }

    fn apply_rgb_theme(&mut self, t: &RgbTheme) {
        // Themes set every controller that was found, whichever bubbles are selected.
        self.want_fans = t.fans as i32;
        self.want_gpu = t.gpu as i32;
        self.load_color(if self.s.select_fans || !self.s.select_gpu { t.fans } else { t.gpu });
        self.dirty_at = Some(Instant::now());
    }

    fn save_rgb_theme(&mut self) {
        let name = self.theme_name.trim().to_string();
        let taken = built_in_themes().iter().chain(self.s.custom_themes.iter()).any(|t| t.name.eq_ignore_ascii_case(&name));
        if name.is_empty() {
            self.set_status("Type a name for the theme first.", Kind::Bad);
        } else if name.contains('|') || name.contains('=') {
            self.set_status("Names can't contain | or =.", Kind::Bad);
        } else if taken {
            self.set_status(format!("There is already a theme called {name}."), Kind::Bad);
        } else if self.s.custom_themes.len() >= MAX_CUSTOM_THEMES {
            self.set_status(format!("You can save up to {MAX_CUSTOM_THEMES} themes. Delete one first."), Kind::Bad);
        } else {
            let pick = |c: i32, fallback: u32| if c >= 0 { c as u32 } else { fallback };
            let theme = RgbTheme { name: name.clone(), fans: pick(self.s.fans_rgb, self.current), gpu: pick(self.s.gpu_rgb, self.current) };
            self.s.custom_themes.push(theme);
            self.save();
            self.theme_name.clear();
            self.set_status(format!("Saved {name}."), Kind::Good);
        }
    }

    // ---------------------------------------------------------------- tabs
    /// Narrow window: everything stacked. Wide window: picker on the left, controls on the right.
    fn color_tab(&mut self, ui: &mut Ui) {
        let l = self.layout;
        if !l.wide {
            self.device_bubbles(ui);
            ui.add_space(4.0);
            self.picker_block(ui, l.picker);
            ui.add_space(4.0);
            self.color_controls(ui);
            return;
        }
        let left = l.width - RIGHT_W - GAP_W;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(vec2(left, l.picker), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(left);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                self.picker_block(ui, l.picker);
            });
            ui.add_space(GAP_W);
            ui.allocate_ui_with_layout(vec2(RIGHT_W, l.picker.max(CONTROLS_H)), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(RIGHT_W);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                if l.picker > CONTROLS_H {
                    ui.add_space((l.picker - CONTROLS_H) / 2.0); // line the controls up with the picker's middle
                }
                self.device_bubbles(ui);
                ui.add_space(4.0);
                self.color_controls(ui);
            });
        });
    }

    /// Color square, hue bar and the # code row, filling `height` in total.
    fn picker_block(&mut self, ui: &mut Ui, height: f32) {
        let square = (height - HUE_H - HEX_ROW_H - 2.0 * 8.0).max(80.0);
        if self.color_picker(ui, square) {
            self.from_hsv();
        }
        self.hex_row(ui);
    }

    /// Swatches, hex, RGB/HSV fields, brightness and status, sized to the column they're in.
    fn color_controls(&mut self, ui: &mut Ui) {
        let top = ui.cursor().min;
        let w = ui.available_width();
        // Swatches
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let size = (w - 4.0 * 7.0) / 8.0;
            for c in SWATCHES {
                if swatch(ui, &self.th, size, c, c == self.current).clicked() {
                    self.from_rgb(c);
                }
            }
        });

        // RGB and HSV fields
        let field_w = (w - 16.0) / 3.0;
        let mut c = [(self.current >> 16 & 255) as i32, (self.current >> 8 & 255) as i32, (self.current & 255) as i32];
        let mut rgb_changed = false;
        ui.horizontal(|ui| {
            for (v, p) in c.iter_mut().zip(["R  ", "G  ", "B  "]) {
                rgb_changed |= ui.add_sized([field_w, 28.0], egui::DragValue::new(v).range(0..=255).prefix(p)).changed();
            }
        });
        if rgb_changed {
            self.from_rgb((c[0] << 16 | c[1] << 8 | c[2]) as u32);
        }
        let mut hsv = [(self.h.round() as i32).min(359), (self.sat * 100.0).round() as i32, (self.val * 100.0).round() as i32];
        let mut hsv_changed = false;
        ui.horizontal(|ui| {
            hsv_changed |= ui.add_sized([field_w, 28.0], egui::DragValue::new(&mut hsv[0]).range(0..=359).prefix("H  ").suffix("°")).changed();
            hsv_changed |= ui.add_sized([field_w, 28.0], egui::DragValue::new(&mut hsv[1]).range(0..=100).prefix("S  ").suffix("%")).changed();
            hsv_changed |= ui.add_sized([field_w, 28.0], egui::DragValue::new(&mut hsv[2]).range(0..=100).prefix("V  ").suffix("%")).changed();
        });
        if hsv_changed {
            (self.h, self.sat, self.val) = (hsv[0] as f32, hsv[1] as f32 / 100.0, hsv[2] as f32 / 100.0);
            self.from_hsv();
        }

        // Brightness
        ui.horizontal(|ui| {
            ui.label("Brightness");
            ui.spacing_mut().slider_width = w - 140.0;
            let r = ui.add(egui::Slider::new(&mut self.s.brightness, 0..=100).step_by(1.0).suffix("%"));
            if r.changed() {
                self.brightness_changed();
            }
        });

        self.theme_controls(ui, w);
        self.status_line(ui);
        self.controls_rect = Rect::from_min_max(top, ui.min_rect().max);
    }

    /// One row: a dropdown with every built-in and saved theme (fans + GPU colors as two dots),
    /// and a Save… button that swaps the row for a name field.
    fn theme_controls(&mut self, ui: &mut Ui, w: f32) {
        ui.add_space(2.0);
        ui.label(egui::RichText::new("Theme").color(self.th.muted).small());
        let dd_w = w - 76.0 - 8.0;

        if self.saving_theme {
            ui.horizontal(|ui| {
                let edit = ui.add_sized(
                    [dd_w, 28.0],
                    egui::TextEdit::singleline(&mut self.theme_name).hint_text("Name").char_limit(24),
                );
                if self.saving_focus {
                    edit.request_focus();
                    self.saving_focus = false;
                }
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if accent_button(ui, &self.th, "Save", 76.0).clicked() || enter {
                    self.save_rgb_theme();
                    if self.theme_name.is_empty() {
                        self.saving_theme = false; // saved; errors keep the field open
                    }
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.saving_theme = false;
                    self.theme_name.clear();
                }
            });
            return;
        }

        let mut themes: Vec<(RgbTheme, bool)> = built_in_themes().into_iter().map(|t| (t, false)).collect();
        themes.extend(self.s.custom_themes.iter().cloned().map(|t| (t, true)));
        let active = themes.iter().position(|(t, _)| self.s.fans_rgb == t.fans as i32 && self.s.gpu_rgb == t.gpu as i32);

        let mut apply = None;
        let mut delete = None;
        self.theme_item_rects.clear();
        let th = self.th;
        ui.horizontal(|ui| {
            let combo = egui::ComboBox::from_id_salt("rgb_theme").width(dd_w).selected_text("").show_ui(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (i, (t, custom)) in themes.iter().enumerate() {
                    let (row, what) = theme_row(ui, &th, t, *custom, active == Some(i));
                    self.theme_item_rects.push(row);
                    match what {
                        CardClick::Apply => apply = Some(t.clone()),
                        CardClick::Delete => delete = Some(t.name.clone()),
                        CardClick::None => {}
                    }
                }
            });
            // The closed button shows the active theme's colors and name.
            let rect = combo.response.rect;
            self.theme_dd_rect = rect;
            let p = ui.painter();
            match active {
                Some(i) => {
                    let t = &themes[i].0;
                    draw_theme_dots(p, pos2(rect.left() + 16.0, rect.center().y), t.fans, t.gpu, th.border);
                    p.text(pos2(rect.left() + 36.0, rect.center().y), Align2::LEFT_CENTER, &t.name, FontId::proportional(14.0), th.text);
                }
                None => {
                    p.text(pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, "Custom", FontId::proportional(14.0), th.muted);
                }
            }
            let save = ui
                .add_sized([76.0, rect.height()], egui::Button::new("Save…"))
                .on_hover_text("Save the current colors as a theme");
            self.theme_save_rect = save.rect;
            if save.clicked() {
                self.saving_theme = true;
                self.saving_focus = true;
            }
        });

        if let Some(t) = apply {
            self.apply_rgb_theme(&t);
        }
        if let Some(name) = delete {
            self.s.custom_themes.retain(|t| t.name != name);
            self.save();
            self.set_status(format!("Deleted {name}."), Kind::Muted);
        }
    }

    // ---------- devices
    /// Every controller Duce knows about. To support another one, add a row here and its
    /// found/selected/color fields; the bubbles, "All" and scrolling follow automatically.
    fn devices(&self) -> Vec<Device> {
        vec![
            Device { name: "Fans & board", short: "Fans", found: self.fans_found, selected: self.s.select_fans, color: self.s.fans_rgb },
            Device { name: "Graphics card", short: "GPU", found: self.gpu_found, selected: self.s.select_gpu, color: self.s.gpu_rgb },
        ]
    }

    fn set_selected(&mut self, index: usize, on: bool) {
        match index {
            0 => self.s.select_fans = on,
            _ => self.s.select_gpu = on,
        }
    }

    /// "All" first, then one bubble per device, filled with its current color; a ring means it will change.
    /// Devices that weren't found are hidden; while still checking they show faded.
    fn device_bubbles(&mut self, ui: &mut Ui) {
        const SLOT: f32 = 44.0;
        const GAP: f32 = 6.0;
        let devices = self.devices();
        let shown: Vec<(usize, &Device)> = devices.iter().enumerate().filter(|(_, d)| d.found != Some(false)).collect();
        let usable: Vec<&Device> = shown.iter().map(|(_, d)| *d).filter(|d| d.found == Some(true)).collect();
        let all_on = !usable.is_empty() && usable.iter().all(|d| d.selected);
        let n = shown.len() + 1;
        let total = n as f32 * SLOT + (n - 1) as f32 * GAP;

        let mut clicked: Option<Option<usize>> = None; // Some(None) = All, Some(Some(i)) = device i
        let mut rects = Vec::new();
        egui::ScrollArea::horizontal().id_salt("devices").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                let w = ui.available_width();
                if total < w {
                    ui.add_space((w - total) / 2.0);
                }
                let fills: Vec<i32> = usable.iter().map(|d| d.color).collect();
                let tip = if all_on { "All devices will change" } else { "Change all devices" };
                let r = bubble(ui, &self.th, "All", &fills, all_on, !usable.is_empty(), tip);
                rects.push(r.rect);
                if r.clicked() {
                    clicked = Some(None);
                }
                for (i, d) in &shown {
                    let ready = d.found == Some(true);
                    let on = d.selected && ready;
                    let tip = if !ready { format!("{}: checking...", d.name) }
                        else if on { format!("{}: will change", d.name) }
                        else { format!("{}: won't change", d.name) };
                    let fill = if ready { d.color } else { -1 };
                    let r = bubble(ui, &self.th, d.short, &[fill], on, ready, &tip);
                    rects.push(r.rect);
                    if r.clicked() {
                        clicked = Some(Some(*i));
                    }
                }
            });
        });
        self.bubble_rects = rects;

        match clicked {
            Some(None) => {
                for (i, d) in devices.iter().enumerate() {
                    if d.found == Some(true) {
                        self.set_selected(i, true);
                    }
                }
                self.selection_changed();
            }
            Some(Some(i)) => {
                self.set_selected(i, !devices[i].selected);
                self.selection_changed();
            }
            None => {}
        }
    }

    /// Color square (left to right = more vivid, top to bottom = darker) with a hue bar under it.
    /// While dragging, a tag next to the pointer shows the # code.
    fn color_picker(&mut self, ui: &mut Ui, square_h: f32) -> bool {
        self.ensure_picker_textures(ui.ctx());
        let w = ui.available_width();
        let mut changed = false;

        // Square
        let (sq, sq_resp) = ui.allocate_exact_size(vec2(w, square_h), Sense::click_and_drag());
        if sq_resp.dragged() || sq_resp.clicked() {
            if let Some(p) = sq_resp.interact_pointer_pos() {
                // The last couple of pixels snap to the edge, so pure colors, white and black are easy to hit.
                let snap = |f: f32| if f < 0.01 { 0.0 } else if f > 0.99 { 1.0 } else { f };
                let s = snap(((p.x - sq.left()) / sq.width()).clamp(0.0, 1.0));
                let v = 1.0 - snap(((p.y - sq.top()) / sq.height()).clamp(0.0, 1.0));
                if s != self.sat || v != self.val {
                    (self.sat, self.val) = (s, v);
                    changed = true;
                }
            }
        }
        if let Some((_, tex)) = &self.sv_tex {
            egui::Image::from_texture(egui::load::SizedTexture::from_handle(tex)).corner_radius(8).paint_at(ui, sq);
        }
        ui.painter().rect_stroke(sq, 8.0, st(1.0, self.th.border), StrokeKind::Inside);
        let color = rgb(hsv_to_rgb(self.h, self.sat, self.val));
        let knob = pos2(sq.left() + self.sat * sq.width(), sq.top() + (1.0 - self.val) * sq.height());
        draw_knob(ui.painter(), knob, color);

        // Hue bar
        // Inset by the knob's radius on both sides, so the knob stays inside the square's width.
        let (row, bar_resp) = ui.allocate_exact_size(vec2(w, HUE_H), Sense::click_and_drag());
        let bar = row.shrink2(vec2(10.0, 0.0));
        if bar_resp.dragged() || bar_resp.clicked() {
            if let Some(p) = bar_resp.interact_pointer_pos() {
                let h = ((p.x - bar.left()) / bar.width()).clamp(0.0, 0.9999) * 360.0;
                if h != self.h {
                    self.h = h;
                    changed = true;
                }
            }
        }
        if let Some(tex) = &self.hue_tex {
            egui::Image::from_texture(egui::load::SizedTexture::from_handle(tex)).corner_radius(HUE_H / 2.0).paint_at(ui, bar);
        }
        self.picker_rects = [sq, bar];
        let hue_knob = pos2(bar.left() + self.h / 360.0 * bar.width(), bar.center().y);
        draw_knob(ui.painter(), hue_knob, rgb(hsv_to_rgb(self.h, 1.0, 1.0)));

        // Live # code next to whatever is being dragged.
        let dragging = if sq_resp.dragged() { Some(knob) } else if bar_resp.dragged() { Some(hue_knob) } else { None };
        if let Some(at) = dragging {
            let code = format!("#{:06X}", hsv_to_rgb(self.h, self.sat, self.val));
            let font = FontId::monospace(14.0);
            let size = ui.painter().layout_no_wrap(code.clone(), font.clone(), self.th.text).size() + vec2(14.0, 8.0);
            let above = at.y - 16.0 - size.y > sq.top();
            let center = pos2(at.x.clamp(sq.left() + size.x / 2.0, sq.right() - size.x / 2.0), if above { at.y - 16.0 - size.y / 2.0 } else { at.y + 16.0 + size.y / 2.0 });
            let tag = Rect::from_center_size(center, size);
            let p = ui.painter();
            p.rect(tag, 6.0, self.th.surface, st(1.0, self.th.border), StrokeKind::Inside);
            p.text(tag.center(), Align2::CENTER_CENTER, code, font, self.th.text);
        }
        changed
    }

    /// The square's gradient depends on the hue, so it's redrawn when the hue moves.
    fn ensure_picker_textures(&mut self, ctx: &egui::Context) {
        if self.hue_tex.is_none() {
            let n = 360;
            let mut px = Vec::with_capacity(n * 4);
            for i in 0..n {
                let c = hsv_to_rgb(i as f32 * 360.0 / n as f32, 1.0, 1.0);
                px.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]);
            }
            let img = egui::ColorImage::from_rgba_unmultiplied([n, 1], &px);
            self.hue_tex = Some(ctx.load_texture("hue", img, egui::TextureOptions::LINEAR));
        }
        if self.sv_tex.as_ref().is_none_or(|(h, _)| (h - self.h).abs() > 0.1) {
            let n = 128;
            let mut px = Vec::with_capacity(n * n * 4);
            for y in 0..n {
                for x in 0..n {
                    let c = hsv_to_rgb(self.h, x as f32 / (n - 1) as f32, 1.0 - y as f32 / (n - 1) as f32);
                    px.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]);
                }
            }
            let img = egui::ColorImage::from_rgba_unmultiplied([n, n], &px);
            self.sv_tex = Some((self.h, ctx.load_texture("sv", img, egui::TextureOptions::LINEAR)));
        }
    }

    /// Preview, the editable # code, and a Copy button.
    fn hex_row(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(44.0, HEX_ROW_H), Sense::hover());
            ui.painter().rect(rect, 7.0, rgb(self.current), st(1.0, self.th.border), StrokeKind::Inside);
            ui.label(egui::RichText::new("#").font(FontId::monospace(14.0)).color(self.th.muted));
            // The field takes all the room left, so Copy ends flush with the square's right edge.
            let field_w = (ui.available_width() - 72.0 - ui.spacing().item_spacing.x).max(60.0);
            let edit = ui.add_sized(
                [field_w, HEX_ROW_H],
                egui::TextEdit::singleline(&mut self.hex_text).font(FontId::monospace(14.0)).char_limit(7).vertical_align(egui::Align::Center).margin(egui::Margin::symmetric(12, 0)),
            );
            if edit.changed() {
                let t = self.hex_text.trim().trim_start_matches('#').to_string();
                if t.len() == 6 {
                    if let Ok(c) = u32::from_str_radix(&t, 16) {
                        self.current = c;
                        let (h, s, v) = rgb_to_hsv(c, self.h);
                        (self.h, self.sat, self.val) = (h, s, v);
                        self.color_changed();
                    }
                }
            }
            if edit.lost_focus() {
                self.hex_text = format!("{:06X}", self.current);
            }
            if ui.add_sized([72.0, HEX_ROW_H], egui::Button::new("Copy")).on_hover_text("Copy the # code").clicked() {
                let code = format!("#{:06X}", self.current);
                ui.ctx().copy_text(code.clone());
                self.set_status(format!("Copied {code}."), Kind::Good);
            }
        });
    }
    /// Narrow window: one column. Wide window: app themes on the left, the rest on the right.
    fn settings_tab(&mut self, ui: &mut Ui) {
        if !self.layout.wide {
            self.app_theme_settings(ui);
            ui.add_space(6.0);
            self.other_settings(ui);
            return;
        }
        let half = (ui.available_width() - GAP_W) / 2.0;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(vec2(half, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(half);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                self.app_theme_settings(ui);
            });
            ui.add_space(GAP_W);
            ui.allocate_ui_with_layout(vec2(half, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(half);
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                self.other_settings(ui);
            });
        });
    }

    fn app_theme_settings(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.label(egui::RichText::new("App theme").strong());
        let w = (ui.available_width() - 8.0) / 2.0;
        let mut ids: Vec<(&str, &str)> = vec![("system", "System")];
        ids.extend(PALETTES.iter().map(|p| (p.id, p.name)));
        let mut picked = None;
        for pair in ids.chunks(2) {
            ui.horizontal(|ui| {
                for (id, name) in pair {
                    if app_theme_card(ui, &self.th, w, id, name, self.s.theme_mode == *id).clicked() {
                        picked = Some(id.to_string());
                    }
                }
            });
        }
        if let Some(id) = picked {
            self.s.theme_mode = id;
            self.save();
            self.restyle(&ctx);
        }
    }

    fn other_settings(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.label(egui::RichText::new("Accent color").strong());
        ui.horizontal(|ui| {
            for (i, (c, _name)) in ACCENTS.iter().enumerate() {
                if swatch(ui, &self.th, 38.0, *c, i == self.s.accent).clicked() {
                    self.s.accent = i;
                    self.save();
                    self.restyle(&ctx);
                }
            }
        });

        ui.add_space(6.0);
        ui.label(egui::RichText::new("Startup").strong());
        if toggle_row(ui, &self.th, "Apply colors at login", "Restores them after a restart", &mut self.s.start_at_login) {
            self.save();
        }
        ui.label(egui::RichText::new("Duce always starts with Windows, so it can turn the lights off at shutdown.").color(self.th.muted).small());

        if !self.others.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Other RGB devices on this PC").strong());
            ui.label(egui::RichText::new("Duce can't control these yet. Each shows the lighting protocol it uses; \"OpenRGB: yes\" means the free OpenRGB app can drive it, and support could be added to Duce on request.").color(self.th.muted).small());
            for line in &self.others {
                ui.label(egui::RichText::new(format!("•  {line}")).color(self.th.muted).small());
            }
        }

        ui.add_space(10.0);
        ui.label(egui::RichText::new("Remove").strong());
        let label = if self.confirm_uninstall { "Click again to confirm" } else { "Remove Duce from this PC" };
        let resp = ui.add(egui::Button::new(egui::RichText::new(label).color(self.th.bad)));
        if resp.clicked() {
            if !self.confirm_uninstall {
                self.confirm_uninstall = true;
                self.set_status("This removes autostart and Duce's files. Click again if you are sure.", Kind::Muted);
            } else if let Ok(exe) = std::env::current_exe() {
                // A separate copy does the removal, so this window can get out of its way.
                let _ = std::process::Command::new(exe).arg("--uninstall").spawn();
                std::process::exit(0);
            }
        } else if self.confirm_uninstall && ui.input(|i| i.pointer.any_click()) {
            self.confirm_uninstall = false; // clicked somewhere else: stand down
        }
        self.status_line(ui);
    }

    fn status_line(&self, ui: &mut Ui) {
        let color = match self.status.1 {
            Kind::Muted => self.th.muted,
            Kind::Good => self.th.good,
            Kind::Bad => self.th.bad,
        };
        ui.add(egui::Label::new(egui::RichText::new(&self.status.0).color(color).small()).wrap());
    }
}

/// Runs on a worker thread. Each color is -1 if that controller shouldn't change.
fn apply_now(fans: i32, gpu: i32, level: i32) -> Outcome {
    let mut o = Outcome { fans, gpu, level, fans_ok: false, gpu_ok: false, errors: String::new() };
    if fans >= 0 {
        let e = hw::apply(dim(fans, level) as u32, true, false, false);
        o.fans_ok = e.is_empty();
        o.errors += &e;
    }
    if gpu >= 0 {
        let e = hw::apply(dim(gpu, level) as u32, false, true, false);
        o.gpu_ok = e.is_empty();
        o.errors += &e;
    }
    o
}

// ---------------------------------------------------------------- startup entry
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Registers Duce to apply the saved colors at login (a one-second run, nothing stays resident),
/// every time the window opens, so it keeps working if the exe moves.
/// Also removes the old MiniRGB entry.
fn register_startup() {
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    let data = wide(&format!("\"{exe}\" --agent"));
    let r = unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide("Duce").as_ptr(), REG_SZ,
            data.as_ptr().cast(), (data.len() * 2) as u32)
    };
    if r != 0 {
        log(&format!("Could not register Duce to start with Windows (error {r})"));
    }
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide("MiniRGB").as_ptr()) };
}

/// Ends the resident helper that Duce versions before 1.1 kept running for lights-off at
/// shutdown (the BIOS's "LEDs in Sleep, Hibernation, and Soft Off States = Off" covers that now).
fn stop_old_helper() {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};
    unsafe {
        let hwnd = FindWindowW(wide("DuceHelper").as_ptr(), std::ptr::null());
        if hwnd.is_null() {
            return;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 || pid == std::process::id() {
            return;
        }
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !h.is_null() {
            log("Stopping the pre-1.1 background helper; the BIOS handles shutdown lights now");
            TerminateProcess(h, 0);
            CloseHandle(h);
        }
    }
}

/// A Start menu shortcut, so Windows search finds Duce. Recreated if missing.
fn ensure_start_menu_shortcut() {
    let Some(appdata) = std::env::var_os("APPDATA") else { return };
    let link = std::path::Path::new(&appdata).join(r"Microsoft\Windows\Start Menu\Programs\Duce.lnk");
    if link.exists() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath='{}'; $s.IconLocation='{},0'; $s.Description='Duce RGB'; $s.Save()",
        link.display().to_string().replace('\'', "''"),
        exe.display().to_string().replace('\'', "''"),
        exe.display().to_string().replace('\'', "''"),
    );
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let made = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    if let Err(e) = made {
        log(&format!("Could not create Start menu shortcut: {e}"));
    }
}

// ---------------------------------------------------------------- widgets
fn draw_knob(p: &egui::Painter, at: Pos2, color: Color32) {
    p.circle_filled(at, 8.0, color);
    p.circle_stroke(at, 8.0, st(2.5, Color32::WHITE));
    p.circle_stroke(at, 9.5, st(1.0, Color32::from_black_alpha(140)));
}
fn segmented(ui: &mut Ui, th: &Th, width: f32, items: &[&str], selected: &mut usize) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
    ui.painter().rect(rect, 9.0, th.surface, st(1.0, th.border), StrokeKind::Inside);
    let w = rect.width() / items.len() as f32;
    for (i, name) in items.iter().enumerate() {
        let r = Rect::from_min_size(pos2(rect.left() + w * i as f32, rect.top()), vec2(w, rect.height())).shrink(3.0);
        let resp = ui.interact(r, ui.id().with(("tab", i)), Sense::click());
        if resp.clicked() {
            *selected = i;
        }
        let on = *selected == i;
        if on {
            ui.painter().rect_filled(r, 7.0, th.accent);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 7.0, th.field);
        }
        let color = if on { Color32::WHITE } else { th.text };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, *name, FontId::proportional(14.0), color);
    }
}

/// One controller as the device bubbles see it.
struct Device {
    name: &'static str,
    short: &'static str,
    found: Option<bool>, // None while still checking
    selected: bool,
    color: i32, // -1 = never set
}

/// A round bubble filled with one color, or split into slices for several (used by "All").
fn bubble(ui: &mut Ui, th: &Th, label: &str, fills: &[i32], ringed: bool, enabled: bool, tip: &str) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(44.0, 52.0), sense);
    let p = ui.painter();
    let c = pos2(rect.center().x, rect.top() + 19.0);
    let r = 14.0;
    let fade = |col: Color32| if enabled { col } else { col.gamma_multiply(0.35) };
    let color = |v: i32| fade(if v >= 0 { rgb(v as u32) } else { th.field });

    if fills.len() <= 1 {
        p.circle_filled(c, r, color(fills.first().copied().unwrap_or(-1)));
    } else {
        // Pie slices, starting at the top, one per device.
        let steps = 48 / fills.len() + 2;
        let mut mesh = Mesh::default();
        for (i, &f) in fills.iter().enumerate() {
            let col = color(f);
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(c, col);
            let a0 = std::f32::consts::TAU * i as f32 / fills.len() as f32 - std::f32::consts::FRAC_PI_2;
            let a1 = std::f32::consts::TAU * (i + 1) as f32 / fills.len() as f32 - std::f32::consts::FRAC_PI_2;
            for s in 0..=steps {
                let a = a0 + (a1 - a0) * s as f32 / steps as f32;
                mesh.colored_vertex(c + vec2(a.cos(), a.sin()) * r, col);
            }
            for s in 0..steps as u32 {
                mesh.add_triangle(base, base + 1 + s, base + 2 + s);
            }
        }
        p.add(Shape::mesh(mesh));
    }
    p.circle_stroke(c, r, st(1.0, th.border));
    if ringed {
        p.circle_stroke(c, r + 3.5, st(2.0, th.accent));
    } else if resp.hovered() && enabled {
        p.circle_stroke(c, r + 3.5, st(1.0, th.muted));
    }
    let text = if ringed { th.text } else { th.muted };
    p.text(pos2(c.x, rect.bottom() - 8.0), Align2::CENTER_CENTER, label, FontId::proportional(11.5), fade(text));
    let resp = resp.on_hover_text(tip);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

fn swatch(ui: &mut Ui, th: &Th, size: f32, c: u32, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let p = ui.painter();
    let inner = rect.shrink(3.0);
    p.rect(inner, 7.0, rgb(c), st(1.0, th.border), StrokeKind::Inside);
    if selected {
        p.rect_stroke(rect.shrink(0.5), 9.0, st(2.0, th.accent), StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_stroke(rect.shrink(0.5), 9.0, st(1.0, th.muted), StrokeKind::Inside);
    }
    resp.on_hover_text(format!("#{c:06X}"))
}

fn accent_button(ui: &mut Ui, th: &Th, text: &str, w: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
    let fill = if resp.hovered() { th.accent.gamma_multiply(0.85) } else { th.accent };
    ui.painter().rect_filled(rect, 8.0, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(14.0), Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Returns true when the switch was flipped.
fn toggle_row(ui: &mut Ui, th: &Th, title: &str, sub: &str, on: &mut bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
    }
    let p = ui.painter();
    p.text(pos2(rect.left(), rect.top() + 13.0), Align2::LEFT_CENTER, title, FontId::proportional(14.0), th.text);
    p.text(pos2(rect.left(), rect.top() + 32.0), Align2::LEFT_CENTER, sub, FontId::proportional(12.0), th.muted);
    let t = ui.ctx().animate_bool(resp.id, *on);
    let track = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(38.0, 22.0));
    let fill = if *on { th.accent } else { th.field };
    p.rect(track, 11.0, fill, st(1.0, if *on { th.accent } else { th.border }), StrokeKind::Inside);
    let x = egui::lerp(track.left() + 11.0..=track.right() - 11.0, t);
    p.circle_filled(pos2(x, track.center().y), 8.0, Color32::WHITE);
    resp.clicked()
}

enum CardClick {
    None,
    Apply,
    Delete,
}

/// A theme's two colors as overlapping dots: fans on the left, GPU on the right.
fn draw_theme_dots(p: &egui::Painter, at: Pos2, fans: u32, gpu: u32, border: Color32) {
    p.circle(at, 7.0, rgb(fans), st(1.0, border));
    p.circle(at + vec2(9.0, 0.0), 7.0, rgb(gpu), st(1.0, border));
}

/// One line inside the theme dropdown: dots, name, and an × on saved themes.
fn theme_row(ui: &mut Ui, th: &Th, t: &RgbTheme, custom: bool, active: bool) -> (Rect, CardClick) {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, 6.0, th.accent.gamma_multiply(0.25));
    } else if resp.hovered() {
        p.rect_filled(rect, 6.0, th.field);
    }
    draw_theme_dots(p, pos2(rect.left() + 14.0, rect.center().y), t.fans, t.gpu, th.border);
    p.text(pos2(rect.left() + 34.0, rect.center().y), Align2::LEFT_CENTER, &t.name, FontId::proportional(14.0), th.text);

    if custom {
        let x = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(18.0, 18.0));
        let over = resp.hover_pos().is_some_and(|pos| x.contains(pos));
        p.text(x.center(), Align2::CENTER_CENTER, "×", FontId::proportional(16.0), if over { th.bad } else { th.muted });
        if resp.clicked() && resp.interact_pointer_pos().is_some_and(|pos| x.contains(pos)) {
            return (rect, CardClick::Delete);
        }
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    (rect, if resp.clicked() { CardClick::Apply } else { CardClick::None })
}

fn app_theme_card(ui: &mut Ui, th: &Th, w: f32, id: &str, name: &str, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 64.0), Sense::click());
    let p = ui.painter();
    let stroke = if selected { st(2.0, th.accent) } else { st(1.0, th.border) };
    p.rect(rect, 10.0, if resp.hovered() { th.field } else { th.surface }, stroke, StrokeKind::Inside);

    // Small preview of the palette (System shows light and dark side by side).
    let preview = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(rect.width() - 16.0, 28.0));
    let draw = |area: Rect, pal: &Palette, corner: CornerRadius| {
        p.rect_filled(area, corner, rgb(pal.bg));
        p.rect_filled(Rect::from_min_size(area.min + vec2(6.0, 6.0), vec2(area.width() * 0.55, 6.0)), 3.0, rgb(pal.surface));
        p.rect_filled(Rect::from_min_size(area.min + vec2(6.0, 16.0), vec2(area.width() * 0.35, 5.0)), 2.5, rgb(pal.muted));
        p.circle_filled(pos2(area.right() - 10.0, area.center().y), 5.0, th.accent);
    };
    if id == "system" {
        let half = preview.width() / 2.0;
        draw(Rect::from_min_size(preview.min, vec2(half, preview.height())), &PALETTES[0], CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 });
        draw(Rect::from_min_size(preview.min + vec2(half, 0.0), vec2(half, preview.height())), &PALETTES[1], CornerRadius { nw: 0, sw: 0, ne: 6, se: 6 });
    } else if let Some(pal) = PALETTES.iter().find(|p| p.id == id) {
        draw(preview, pal, CornerRadius::same(6));
    }
    p.rect_stroke(preview, 6.0, st(1.0, th.border), StrokeKind::Inside);
    p.text(pos2(rect.left() + 10.0, rect.bottom() - 14.0), Align2::LEFT_CENTER, name, FontId::proportional(13.0), th.text);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---------------------------------------------------------------- frame loop
impl Gui {
    /// One frame of the window.
    fn frame(&mut self, ui: &mut Ui) {
        let ctx = &ui.ctx().clone();
        self.poll();

        // Follow Windows' light/dark switch when the app theme is System.
        if self.s.theme_mode == "system" && self.theme_checked.elapsed() > Duration::from_secs(2) {
            self.theme_checked = Instant::now();
            if system_is_dark() != self.th.dark {
                self.restyle(ctx);
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.th.bg))
            .show_inside(ui, |ui| {
                // Narrow windows stack everything; wide ones go side by side (see layout_for).
                // The content stays in the middle of the window and scrolls if the window is too short.
                let area = ui.available_rect_before_wrap();
                let l = layout_for(area);
                self.layout = l;
                let outer = l.width + 2.0 * PAD;
                let x = if area.width() > outer { area.center().x - outer / 2.0 } else { area.left() };
                let y = if area.height() > l.height { area.center().y - l.height / 2.0 } else { area.top() };
                let column = Rect::from_min_max(pos2(x, y), pos2(x + outer, area.bottom()));
                ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
                    egui::ScrollArea::vertical().auto_shrink(false).scroll_source(egui::scroll_area::ScrollSource { drag: false, ..Default::default() }).show(ui, |ui| {
                        egui::Frame::new().inner_margin(PAD).show(ui, |ui| {
                            ui.set_width(l.width);
                            let th = self.th;
                            let tabs_w = if l.wide { TABS_W } else { l.width };
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.add_space((l.width - tabs_w) / 2.0);
                                segmented(ui, &th, tabs_w, &["Color", "Settings"], &mut self.tab);
                            });
                            ui.add_space(6.0);
                            match self.tab {
                                0 => self.color_tab(ui),
                                _ => self.settings_tab(ui),
                            }
                        });
                    });
                });
            });

        // Keep ticking while something is pending, so debounced colors and results show up.
        if self.dirty_at.is_some() || self.worker.is_some() || self.detect.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        } else if self.gpu_recheck.is_some() || self.gpu_recheck_at.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        } else if self.s.theme_mode == "system" {
            ctx.request_repaint_after(Duration::from_secs(2));
        }
    }
}

impl egui_software_backend::App for Gui {
    fn ui(&mut self, ui: &mut Ui, _backend: &mut egui_software_backend::SoftwareBackend) {
        self.frame(ui);
    }

    fn on_exit(&mut self, _ctx: &egui::Context) {
        // Finish anything still pending so the last color picked is the one that sticks.
        if let Some(w) = self.worker.take() {
            if let Ok(o) = w.join() {
                self.finish(o);
            }
        }
        if let Some(d) = self.detect.take() {
            let (fans, gpu, _) = d.join().unwrap_or((false, false, Vec::new()));
            self.fans_found = Some(fans);
            self.gpu_found = Some(gpu);
        }
        let (fans, gpu) = self.take_wanted();
        if fans >= 0 || gpu >= 0 {
            let o = apply_now(fans, gpu, self.s.brightness);
            self.finish(o);
        }
        self.save(); // everything is on disk whenever the window closes
    }
}

pub fn run() {
    let config = egui_software_backend::SoftwareBackendAppConfiguration::new().viewport_builder(
        egui::ViewportBuilder::default()
            .with_title("Duce")
            .with_icon(egui::IconData { rgba: crate::icon::render(64), width: 64, height: 64 })
            .with_inner_size([572.0, 360.0])
            .with_resizable(true)
            .with_min_inner_size([WIDTH, 360.0]),
    );
    if let Err(e) = egui_software_backend::run_app_with_software_backend(config, Gui::new) {
        log(&format!("Window ERROR: {e:?}"));
    }
}

// ---------------------------------------------------------------- tests
// Clicks are fed straight into egui, so these need no window, mouse or hardware.
#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, Pos2};

    fn gui(ctx: &egui::Context) -> Gui {
        let s = Settings { fans_rgb: 0x0066FF, gpu_rgb: 0x00D5FF, theme_mode: "dark".into(), ..Settings::default() };
        let mut g = Gui::with_settings(s, ctx);
        g.fans_found = Some(true);
        g.gpu_found = Some(true);
        g
    }

    fn frame(ctx: &egui::Context, g: &mut Gui, events: Vec<Event>) {
        frame_sized(ctx, g, events, vec2(WIDTH, HEIGHT));
    }

    fn frame_sized(ctx: &egui::Context, g: &mut Gui, events: Vec<Event>, size: egui::Vec2) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| g.frame(ui));
    }

    fn click(ctx: &egui::Context, g: &mut Gui, at: Pos2) {
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(ctx, g, vec![Event::PointerMoved(at)]);
        frame(ctx, g, vec![button(true)]);
        frame(ctx, g, vec![button(false)]);
        frame(ctx, g, vec![]);
    }

    #[test]
    fn bubbles_choose_devices() {
        let ctx = egui::Context::default();
        let mut g = gui(&ctx);
        frame(&ctx, &mut g, vec![]);
        assert_eq!(g.bubble_rects.len(), 3, "All, Fans, GPU");
        let (all, fans, gpu) = (g.bubble_rects[0].center(), g.bubble_rects[1].center(), g.bubble_rects[2].center());
        assert!(all.x < fans.x && fans.x < gpu.x, "in order, left to right");

        click(&ctx, &mut g, gpu);
        assert!(g.s.select_fans && !g.s.select_gpu, "GPU bubble turns the GPU off");
        assert_eq!(g.current, 0x0066FF, "with only fans selected, the picker shows the fans' color");

        click(&ctx, &mut g, fans);
        assert!(!g.s.select_fans && !g.s.select_gpu, "fans bubble turns the fans off too");
        assert_eq!(g.status.0, "Pick a device above to change it.");

        click(&ctx, &mut g, all);
        assert!(g.s.select_fans && g.s.select_gpu, "All selects every device again");

        click(&ctx, &mut g, fans);
        click(&ctx, &mut g, fans);
        assert!(g.s.select_fans && g.s.select_gpu, "clicking a bubble twice puts it back");
    }

    #[test]
    fn missing_device_has_no_bubble() {
        let ctx = egui::Context::default();
        let mut g = gui(&ctx);
        g.gpu_found = Some(false);
        frame(&ctx, &mut g, vec![]);
        assert_eq!(g.bubble_rects.len(), 2, "only All and Fans");
    }

    #[test]
    fn tabs_switch() {
        let ctx = egui::Context::default();
        let mut g = gui(&ctx);
        frame(&ctx, &mut g, vec![]);
        let tab_y = PAD + 17.0;
        click(&ctx, &mut g, pos2(PAD + INNER / 4.0 * 3.0, tab_y));
        assert_eq!(g.tab, 1, "Settings");
        click(&ctx, &mut g, pos2(PAD + INNER / 4.0, tab_y));
        assert_eq!(g.tab, 0, "back to Color");
    }

    fn click_sized(ctx: &egui::Context, g: &mut Gui, at: Pos2, size: egui::Vec2) {
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        for events in [vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)], vec![]] {
            frame_sized(ctx, g, events, size);
        }
    }

    #[test]
    fn narrow_window_stacks_and_centers() {
        let ctx = egui::Context::default();
        let mut g = gui(&ctx);
        let size = vec2(420.0, 900.0);
        frame_sized(&ctx, &mut g, vec![], size);
        assert!(!g.layout.wide);
        let fans = g.bubble_rects[1].center();
        assert!((fans.x - 210.0).abs() < 1.0, "middle bubble at x={} should be in the middle", fans.x);
        assert!(g.bubble_rects[0].top() > (900.0 - HEIGHT) / 2.0, "centered vertically");
    }

    #[test]
    fn wide_window_goes_side_by_side() {
        let ctx = egui::Context::default();
        let mut g = gui(&ctx);
        let size = vec2(900.0, 700.0);
        frame_sized(&ctx, &mut g, vec![], size);
        let l = g.layout;
        assert!(l.wide);
        assert!(l.picker > 232.0, "the picker gets bigger (got {})", l.picker);
        let fans = g.bubble_rects[1].center();
        assert!(fans.x > 450.0, "device bubbles are in the right-hand column (x={})", fans.x);
        let c = g.controls_rect;
        assert!(c.width() <= RIGHT_W + 1.0 && c.left() > 450.0, "controls stay in the right column: {c:?}");
        assert!(c.height() > 120.0, "controls are stacked top to bottom, not in a row: {c:?}");
        assert!(g.bubble_rects[0].bottom() <= c.top() + 1.0, "bubbles sit above the controls");

        // Tabs are centered, and clicks still land.
        let tab_y = (700.0 - l.height) / 2.0 + PAD + 17.0;
        click_sized(&ctx, &mut g, pos2(450.0 + TABS_W / 4.0, tab_y), size);
        assert_eq!(g.tab, 1, "Settings tab");
        click_sized(&ctx, &mut g, pos2(450.0 - TABS_W / 4.0, tab_y), size);
        assert_eq!(g.tab, 0, "back to Color");
        let gpu = g.bubble_rects[2].center();
        click_sized(&ctx, &mut g, gpu, size);
        assert!(!g.s.select_gpu, "GPU bubble still clickable");
    }

    #[test]
    fn every_tab_draws_at_any_size() {
        let ctx = egui::Context::default();
        let mut g = gui(&ctx);
        for size in [vec2(300.0, 360.0), vec2(300.0, 640.0), vec2(560.0, 400.0), vec2(760.0, 560.0), vec2(1920.0, 1080.0)] {
            for tab in 0..2 {
                g.tab = tab;
                frame_sized(&ctx, &mut g, vec![], size);
            }
        }
    }

    #[test]
    fn square_and_hue_bar_pick_colors() {
        for size in [vec2(WIDTH, HEIGHT), vec2(900.0, 700.0)] {
            let ctx = egui::Context::default();
            let mut g = gui(&ctx);
            frame_sized(&ctx, &mut g, vec![], size);
            let [sq, bar] = g.picker_rects;
            assert!(sq.width() > 100.0 && sq.height() > 80.0, "square is drawn: {sq:?}");
            assert!(bar.top() > sq.bottom(), "hue bar sits under the square");

            // Top-right of the square = full color at full brightness; hue bar at 1/3 = green.
            click_sized(&ctx, &mut g, pos2(bar.left() + bar.width() / 3.0, bar.center().y), size);
            assert!((g.h - 120.0).abs() < 2.0, "hue ~120 (got {})", g.h);
            click_sized(&ctx, &mut g, pos2(sq.right() - 0.5, sq.top() + 0.5), size);
            assert!(g.sat > 0.99 && g.val > 0.99, "sat {} val {}", g.sat, g.val);
            let (r, gr, b) = (g.current >> 16 & 255, g.current >> 8 & 255, g.current & 255);
            assert!(gr == 255 && r < 10 && b < 10, "bright green, got {:06X}", g.current);
            assert_eq!(g.hex_text, format!("{:06X}", g.current), "the # code follows the picker");
            assert_eq!(g.want_fans, g.current as i32, "the new color is queued for the lights");

            // Bottom-left = black.
            click_sized(&ctx, &mut g, pos2(sq.left() + 0.5, sq.bottom() - 0.5), size);
            assert_eq!(g.current, 0, "black at the bottom");
        }
    }

    #[test]
    fn theme_dropdown_applies_saves_and_deletes() {
        let ctx = egui::Context::default();
        let mut g = gui(&ctx);
        g.s.custom_themes.push(RgbTheme { name: "Mine".into(), fans: 0x112233, gpu: 0x445566 });
        frame(&ctx, &mut g, vec![]);
        assert!(g.theme_dd_rect.width() > 50.0, "the dropdown is drawn: {:?}", g.theme_dd_rect);

        // Open it and pick the first built-in theme (Ember).
        let dd = g.theme_dd_rect.center();
        click(&ctx, &mut g, dd);
        assert_eq!(g.theme_item_rects.len(), 7, "6 built-in + 1 saved");
        let first = g.theme_item_rects[0].center() - vec2(20.0, 0.0);
        click(&ctx, &mut g, first);
        assert_eq!(g.want_fans, 0xFF2D00, "Ember's fans color is queued");
        assert_eq!(g.want_gpu, 0xFF8A00, "Ember's GPU color is queued");

        // Open it again and delete the saved theme with its ×.
        click(&ctx, &mut g, dd);
        let row = *g.theme_item_rects.last().unwrap();
        click(&ctx, &mut g, pos2(row.right() - 14.0, row.center().y));
        assert!(g.s.custom_themes.is_empty(), "the saved theme is gone");

        // Save…: type a name into the field and press Enter.
        let save = g.theme_save_rect.center();
        click(&ctx, &mut g, save);
        assert!(g.saving_theme, "Save… swaps the row for a name field");
        frame(&ctx, &mut g, vec![Event::Text("Neon".into())]);
        assert_eq!(g.theme_name, "Neon", "the field has the keyboard right away");
        let enter = Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() };
        frame(&ctx, &mut g, vec![enter]);
        frame(&ctx, &mut g, vec![]);
        assert!(g.s.custom_themes.iter().any(|t| t.name == "Neon"), "the theme is saved");
        assert!(!g.saving_theme, "the field closes after saving");
    }
}