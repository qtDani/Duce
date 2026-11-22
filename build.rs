// Builds Duce's .ico from src/icon.rs and embeds it in the exe, so Explorer and the taskbar show it.
// Uses windres from w64devkit (C:\Users\dani\Tools\w64devkit), the same toolkit that provides dlltool.
use std::path::PathBuf;

include!("src/icon.rs");

/// An .ico with one 32-bit BMP image per size (largest first is not required; Windows picks the best).
fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| bmp_entry(s)).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&s, img) in sizes.iter().zip(&images) {
        let dim = if s >= 256 { 0 } else { s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend_from_slice(&img);
    }
    out
}

fn bmp_entry(size: u32) -> Vec<u8> {
    let rgba = render(size);
    let mut out = Vec::new();
    // BITMAPINFOHEADER; height is doubled because the AND mask follows the colors.
    for v in [40u32, size, size * 2] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    for _ in 0..6 {
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    // Colors: bottom-up rows, BGRA.
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    // AND mask: all zero (alpha does the work), rows padded to 4 bytes.
    let row = ((size + 31) / 32 * 4) as usize;
    out.resize(out.len() + row * size as usize, 0);
    out
}

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico_path = out.join("duce.ico");
    std::fs::write(&ico_path, ico(&[16, 20, 24, 32, 40, 48, 64, 128, 256])).unwrap();
    // Also keep a copy in the project, handy for shortcuts.
    let _ = std::fs::copy(&ico_path, "duce.ico");

    let rc = out.join("duce.rc");
    std::fs::write(&rc, format!("1 ICON \"{}\"\n", ico_path.display().to_string().replace('\\', "/"))).unwrap();
    let obj = out.join("duce_res.o");
    let tools = PathBuf::from(r"C:\Users\dani\Tools\w64devkit\bin");
    let status = std::process::Command::new(tools.join("windres.exe"))
        .arg(format!("--preprocessor={}", tools.join("gcc.exe").display()))
        .args(["--preprocessor-arg=-E", "--preprocessor-arg=-xc-header", "--preprocessor-arg=-DRC_INVOKED"])
        .arg("-i")
        .arg(&rc)
        .arg("-o")
        .arg(&obj)
        .status()
        .expect("windres (w64devkit) is needed to embed the icon");
    assert!(status.success(), "windres failed");
    println!("cargo:rustc-link-arg-bins={}", obj.display());
}
