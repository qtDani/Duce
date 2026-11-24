# Duce

A small Windows app that sets the RGB color of Gigabyte motherboards (and the fans on their
headers) and ASUS NVIDIA graphics cards.

## Supported hardware

- **Motherboard:** Gigabyte boards with an ITE IT5711, IT8297 or IT5702 controller
- **Graphics card:** ASUS NVIDIA cards with an ENE "AUMA" lighting chip

Only an IT5711 board and a ROG Astral RTX 5080 are known to have been used with it. Duce
identifies each controller before writing to it, but it does write to hardware:
use it at your own risk.

## Using it

Open `Duce.exe` to pick colors, or use the command line:

| Command | What it does |
| --- | --- |
| `Duce.exe RRGGBB [--no-fans] [--no-gpu]` | Set a color |
| `Duce.exe --apply` | Apply the saved colors |
| `Duce.exe --off` | Turn the lights off |
| `Duce.exe --check` | Identify the controllers without changing the lighting |
| `Duce.exe --uninstall` | Remove Duce from the PC |

Results are written to `Duce.log` next to the exe.

### duce-lite (terminal only)

`duce-lite.exe` is the same controller code without the window (about 0.8 MB instead of 4.5 MB).
It prints its results and exits when done: nothing stays running, because the controllers
hold the color themselves. It shares `settings.txt` with `Duce.exe` when it sits
in the same folder.

| Command | What it does |
| --- | --- |
| `duce-lite scan` | List every RGB device on the PC and the lighting protocol it uses |
| `duce-lite check` | Identify the controllers Duce drives, changing nothing |
| `duce-lite set RRGGBB [--fans\|--gpu] [--brightness N] [--no-save]` | Set (and save) a color; names like `red` work too |
| `duce-lite apply` / `off` / `status` | Apply saved colors / lights off / show saved colors |

## Detection

Duce looks for RGB hardware of every kind, read-only, and names the protocol where it can:
USB devices by their IDs (matched against OpenRGB's list of 1195 devices), graphics cards by
their card maker's PCI ID, RAM by its part number and the motherboard by its model. RAM
lighting sits on the SMBus, which Windows only opens to a kernel driver, so RAM is reported
but never controlled. `tools/gen_openrgb_ids.py` regenerates the device list from a newer
OpenRGB `60-openrgb.rules`.

## Building

Needs Rust with the `x86_64-pc-windows-gnu` toolchain, plus `windres`, `gcc` and `dlltool` on
`PATH` (for example from [w64devkit](https://github.com/skeeto/w64devkit)).
`W64DEVKIT` can point at a w64devkit folder for `windres` and `gcc`.

```
cargo build --release      # target/release/Duce.exe
```

## Credits and license

- [OpenRGB](https://gitlab.com/CalcProgrammer1/OpenRGB) (GPL-2.0-or-later): the list of known
  RGB devices is generated from it, and it is credited for the controller protocols.
- `vendor/egui_software_backend` is a copy of egui_software_backend 0.0.3 (MIT or Apache-2.0,
  license files included), modified to fix immediate redraws when a delayed one was requested.

Duce is licensed under [GPL-2.0-or-later](LICENSE) and comes with no warranty. Some dependencies
are Apache-2.0, so compiled builds are distributed under GPL-3.0 terms, as "or later" allows.
It is not affiliated with Gigabyte, ASUS or OpenRGB.
