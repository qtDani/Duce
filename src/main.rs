// Duce - sets one color on the motherboard/fans and the ASUS RTX 5080.
//   Duce.exe                        open the color picker
//   Duce.exe --agent                the autostart entry: apply saved colors (if enabled) and exit.
//                                      Lights-off at shutdown/sleep is the BIOS's job
//                                      (LEDs in Sleep, Hibernation, and Soft Off States = Off).
//   Duce.exe --apply                apply each controller's saved color and exit
//   Duce.exe --off                  turn the lights off and exit
//   Duce.exe --check                identify both controllers; changes no lighting
//   Duce.exe --uninstall            remove Duce from this PC (autostart, helper, shortcut, files)
//   Duce.exe --uninstall-cleanup    the same minus the files; the MSI runs this while uninstalling
//   Duce.exe RRGGBB [--no-fans] [--no-gpu]   apply a color and exit
// Results go to Duce.log next to the exe. Exit code 0 = everything requested succeeded.
#![windows_subsystem = "windows"]

mod board;
mod gpu;
mod hw;
mod icon;
mod log;
mod openrgb_ids;
mod scan;
mod settings;
mod ui;
mod uninstall;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn ok(errors: String) -> i32 {
    if errors.is_empty() { 0 } else { 1 }
}

fn run(args: &[String]) -> i32 {
    let Some(first) = args.first() else {
        ui::run();
        return 0;
    };
    match first.as_str() {
        "--check" => {
            let mut errors = String::new();
            match board::identify() {
                Ok(id) => log::log(&format!("Fans check: {id}")),
                Err(e) => { log::log(&format!("Fans check ERROR: {e}")); errors.push('x'); }
            }
            match gpu::identify() {
                Ok(id) => log::log(&format!("GPU check: {id}")),
                Err(e) => { log::log(&format!("GPU check ERROR: {e}")); errors.push('x'); }
            }
            ok(errors)
        }
        "--apply" => ok(hw::apply_saved(true)),
        "--off" => ok(hw::lights_off(true)),
        // The autostart entry. Until v1.1 this stayed resident as a background helper for
        // lights-off at shutdown; the BIOS handles that now, so it applies and exits.
        // Kept under the old name so autostart entries from earlier versions still work.
        "--agent" => {
            if args.iter().any(|a| a == "--no-apply") || !settings::load().start_at_login {
                0
            } else {
                ok(hw::apply_saved(true))
            }
        }
        "--uninstall" => uninstall::uninstall(),
        "--uninstall-cleanup" => { uninstall::cleanup(); 0 }
        "--stop" => { uninstall::stop_other_copies(); 0 } // the MSI stops the old version while upgrading
        hex => {
            let hex = hex.trim_start_matches('#');
            match u32::from_str_radix(hex, 16) {
                Ok(rgb) if hex.len() == 6 => {
                    let fans = !args.iter().any(|a| a == "--no-fans");
                    let gpu = !args.iter().any(|a| a == "--no-gpu");
                    ok(hw::apply(rgb, fans, gpu, false))
                }
                _ => { log::log("ERROR: Color must be RRGGBB"); 1 }
            }
        }
    }
}
