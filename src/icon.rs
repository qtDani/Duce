// Duce's icon: three overlapping light circles (red, green, blue) that mix where they overlap.
// Plain Rust with no dependencies, so build.rs can include it to make the exe's .ico too.
// To restyle the icon, change the colors or circle layout below.

/// Flat colors for each region: only red, green, blue, the pairs where two overlap, and all three.
const RED: [u8; 3] = [0xE8, 0x1D, 0x3C];
const GREEN: [u8; 3] = [0x7C, 0xD9, 0x5A];
const BLUE: [u8; 3] = [0x1F, 0x6F, 0xF2];
const RED_GREEN: [u8; 3] = [0xFF, 0xD3, 0x3D];
const RED_BLUE: [u8; 3] = [0xE8, 0x3E, 0x9C];
const GREEN_BLUE: [u8; 3] = [0x8E, 0xEE, 0xF2];
const ALL: [u8; 3] = [0xFF, 0xFF, 0xFF];

/// Centers and radius, as fractions of the icon's size.
const CIRCLES: [(f32, f32); 3] = [(0.50, 0.335), (0.315, 0.655), (0.685, 0.655)];
const RADIUS: f32 = 0.305;

/// Square RGBA image, `size` x `size`, anti-aliased by sampling each pixel 4x4 times.
#[allow(dead_code)]
pub fn render(size: u32) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let s = size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut r, mut g, mut b, mut covered) = (0u32, 0u32, 0u32, 0u32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / s;
                    let py = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / s;
                    let inside = CIRCLES.map(|(cx, cy)| (px - cx).powi(2) + (py - cy).powi(2) <= RADIUS * RADIUS);
                    let color = match inside {
                        [true, true, true] => ALL,
                        [true, true, false] => RED_GREEN,
                        [true, false, true] => RED_BLUE,
                        [false, true, true] => GREEN_BLUE,
                        [true, false, false] => RED,
                        [false, true, false] => GREEN,
                        [false, false, true] => BLUE,
                        [false, false, false] => continue,
                    };
                    r += color[0] as u32;
                    g += color[1] as u32;
                    b += color[2] as u32;
                    covered += 1;
                }
            }
            if covered == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let alpha = covered * 255 / (SAMPLES * SAMPLES);
                out.extend_from_slice(&[(r / covered) as u8, (g / covered) as u8, (b / covered) as u8, alpha as u8]);
            }
        }
    }
    out
}
