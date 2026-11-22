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
