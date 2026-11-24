// duce-lite: Duce for the terminal. No window, no egui, nothing left running - each command does
// its job and exits. Shares settings.txt and Duce.log with Duce.exe when they sit in the same folder.
#![allow(dead_code)] // the shared modules carry window-only helpers this binary doesn't call

#[path = "board.rs"]
mod board;
#[path = "gpu.rs"]
mod gpu;
#[path = "hw.rs"]
mod hw;
#[path = "log.rs"]
mod log;
#[path = "openrgb_ids.rs"]
mod openrgb_ids;
#[path = "scan.rs"]
mod scan;
#[path = "settings.rs"]
mod settings;

const HELP: &str = "duce-lite - set RGB colors from the terminal

  duce-lite scan                  list every RGB device on this PC and its lighting protocol
  duce-lite check                 identify the controllers Duce drives (changes nothing)
  duce-lite set RRGGBB [options]  set a color and save it (restored at login by Duce)
      --fans | --gpu              only that controller (default: both)
      --brightness N              0-100, saved for later colors too
      --no-save                   apply only; the saved colors stay as they were
  duce-lite apply                 apply the saved colors
  duce-lite off                   turn the lights off (saved colors are kept)
  duce-lite status                show the saved colors and brightness

Colors can also be named: red green blue white cyan magenta yellow orange purple pink off";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn run(args: &[String]) -> i32 {
    let cmd = args.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    match cmd.as_str() {
        "" | "help" | "-h" | "--help" | "/?" => { println!("{HELP}"); 0 }
        "scan" => scan(),
        "check" => check(),
        "set" => match args.get(1) {
            Some(c) => set(c, &args[2..]),
            None => fail("set needs a color, for example: duce-lite set FF0000"),
        },
        "apply" => report(hw::apply_saved(false), "Saved colors applied"),
        "off" => report(hw::lights_off(true), "Lights off"),
        "status" => status(),
        // Shorthand: duce-lite FF0000
        other if parse_color(other).is_some() => set(other, &args[1..]),
        other => fail(&format!("Unknown command '{other}'. Run duce-lite help")),
    }
}

fn fail(msg: &str) -> i32 {
    eprintln!("{msg}");
    1
}

/// Prints errors from hw (one per line, already logged) or the success message.
fn report(errors: String, done: &str) -> i32 {
    if errors.is_empty() {
        println!("{done}");
        0
    } else {
        eprint!("{errors}");
        1
    }
}

fn parse_color(s: &str) -> Option<u32> {
    let named = match s.to_ascii_lowercase().as_str() {
        "red" => 0xFF0000, "green" => 0x00FF00, "blue" => 0x0000FF, "white" => 0xFFFFFF,
        "cyan" => 0x00FFFF, "magenta" => 0xFF00FF, "yellow" => 0xFFFF00, "orange" => 0xFF6000,
        "purple" => 0x8000FF, "pink" => 0xFF2080, "off" | "black" => 0x000000,
        _ => {
            let hex = s.trim_start_matches('#');
            return if hex.len() == 6 { u32::from_str_radix(hex, 16).ok() } else { None };
        }
    };
    Some(named)
}

fn set(color: &str, opts: &[String]) -> i32 {
    let Some(rgb) = parse_color(color) else {
        return fail(&format!("'{color}' is not a color: use RRGGBB (like FF8000) or a name"));
    };
    let has = |o: &str| opts.iter().any(|a| a == o);
    let (only_fans, only_gpu) = (has("--fans"), has("--gpu"));
    let (fans, gpu) = if only_fans || only_gpu { (only_fans, only_gpu) } else { (true, true) };

    let mut s = settings::load();
    if let Some(i) = opts.iter().position(|a| a == "--brightness") {
        match opts.get(i + 1).and_then(|v| v.parse::<i32>().ok()) {
            Some(b) if (0..=100).contains(&b) => s.brightness = b,
            _ => return fail("--brightness needs a number from 0 to 100"),
        }
    }

    let sent = settings::dim(rgb as i32, s.brightness) as u32;
    let errors = hw::apply(sent, fans, gpu, false);
    if !has("--no-save") {
        if fans { s.fans_rgb = rgb as i32; }
        if gpu { s.gpu_rgb = rgb as i32; }
        s.select_fans = fans;
        s.select_gpu = gpu;
        if let Err(e) = settings::save(&s) {
            eprintln!("Could not save settings: {e}");
        }
    }
    let target = match (fans, gpu) { (true, true) => "fans and GPU", (true, false) => "fans", _ => "GPU" };
    report(errors, &format!("#{rgb:06X} at {}% on {target}", s.brightness))
}

fn check() -> i32 {
    let mut code = 0;
    for (what, f) in [("Fans", board::identify as fn() -> Result<String, String>), ("GPU", gpu::identify)] {
        match hw::identify(what, f) {
            Ok(id) => println!("{what:<5} ok   {id}"),
            Err(e) => { println!("{what:<5} --   {e}"); code = 1; }
        }
    }
    code
}

fn scan() -> i32 {
    let found = scan::all();
    if found.is_empty() {
        println!("No RGB devices found.");
        return 0;
    }
    println!("{:<6} {:<6} {:<8} {}", "Type", "Duce", "OpenRGB", "Device - protocol");
    for f in &found {
        let duce = if f.duce { "yes" } else { "no" };
        let openrgb = match f.openrgb { Some(true) => "yes", Some(false) => "no", None => "-" };
        println!("{:<6} {:<6} {:<8} {} - {}", f.kind, duce, openrgb, f.name, f.protocol);
    }
    if found.iter().any(|f| f.kind == "RAM") {
        println!("\nRAM lighting is on the SMBus, which needs a kernel driver; Duce does not install one.");
    }
    0
}

fn status() -> i32 {
    let s = settings::load();
    let c = |v: i32| if v < 0 { "not set".to_string() } else { format!("#{v:06X}") };
    println!("Fans        {}", c(s.fans_rgb));
    println!("GPU         {}", c(s.gpu_rgb));
    println!("Brightness  {}%", s.brightness);
    println!("At login    {}", if s.start_at_login { "saved colors restored" } else { "off" });
    0
}
