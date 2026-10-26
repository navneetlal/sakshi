//! Draws the app icon: an amber lens on a dark rounded square. The PNG is
//! the source for `pnpm tauri icon`, which makes every platform's sizes.

use std::path::Path;

use anyhow::Context;
use image::{Rgba, RgbaImage};

const SIZE: u32 = 1024;
/// Samples per pixel along each axis, for anti-aliasing.
const SUPERSAMPLE: u32 = 4;

/// The macOS icon grid: an 824-point square with rounded corners.
const TILE_INSET: f32 = 100.0;
const TILE_RADIUS: f32 = 185.0;
const TOP: [f32; 3] = [0x26 as f32, 0x34 as f32, 0x52 as f32];
const BOTTOM: [f32; 3] = [0x11 as f32, 0x18 as f32, 0x28 as f32];
const AMBER: [f32; 3] = [0xF4 as f32, 0xB6 as f32, 0x3F as f32];
const RING_RADIUS: f32 = 250.0;
const RING_WIDTH: f32 = 56.0;
const PUPIL_RADIUS: f32 = 112.0;

pub fn generate(out: &Path) -> anyhow::Result<()> {
    let image = RgbaImage::from_fn(SIZE, SIZE, pixel);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    image
        .save(out)
        .with_context(|| format!("writing {}", out.display()))?;
    println!("wrote {}", out.display());
    Ok(())
}

fn pixel(x: u32, y: u32) -> Rgba<u8> {
    let mut sum = [0.0f32; 4];
    for sy in 0..SUPERSAMPLE {
        for sx in 0..SUPERSAMPLE {
            let px = x as f32 + (sx as f32 + 0.5) / SUPERSAMPLE as f32;
            let py = y as f32 + (sy as f32 + 0.5) / SUPERSAMPLE as f32;
            let [r, g, b, a] = sample(px, py);
            // Premultiplied, so transparent samples don't darken edges.
            sum[0] += r * a;
            sum[1] += g * a;
            sum[2] += b * a;
            sum[3] += a;
        }
    }
    let n = (SUPERSAMPLE * SUPERSAMPLE) as f32;
    let alpha = sum[3] / n;
    if alpha == 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let channel = |v: f32| (v / sum[3]).round().clamp(0.0, 255.0) as u8;
    Rgba([
        channel(sum[0]),
        channel(sum[1]),
        channel(sum[2]),
        (alpha * 255.0).round() as u8,
    ])
}

/// The color at a point, as `[r, g, b, alpha]` with alpha 0 or 1.
fn sample(x: f32, y: f32) -> [f32; 4] {
    if !in_tile(x, y) {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let c = SIZE as f32 / 2.0;
    let d = ((x - c).powi(2) + (y - c).powi(2)).sqrt();
    if d <= PUPIL_RADIUS || (d - RING_RADIUS).abs() <= RING_WIDTH / 2.0 {
        return [AMBER[0], AMBER[1], AMBER[2], 1.0];
    }
    let t = (y - TILE_INSET) / (SIZE as f32 - 2.0 * TILE_INSET);
    let mix = |i: usize| TOP[i] + (BOTTOM[i] - TOP[i]) * t;
    [mix(0), mix(1), mix(2), 1.0]
}

fn in_tile(x: f32, y: f32) -> bool {
    let (lo, hi) = (TILE_INSET, SIZE as f32 - TILE_INSET);
    if x < lo || x > hi || y < lo || y > hi {
        return false;
    }
    // Distance into a corner's rounding square, if the point is in one.
    let corner = |v: f32| {
        if v < lo + TILE_RADIUS {
            lo + TILE_RADIUS - v
        } else if v > hi - TILE_RADIUS {
            v - (hi - TILE_RADIUS)
        } else {
            0.0
        }
    };
    let (dx, dy) = (corner(x), corner(y));
    dx == 0.0 || dy == 0.0 || dx * dx + dy * dy <= TILE_RADIUS * TILE_RADIUS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_transparent_and_the_center_is_amber() {
        assert_eq!(pixel(0, 0)[3], 0);
        assert_eq!(pixel(TILE_INSET as u32 + 2, TILE_INSET as u32 + 2)[3], 0);
        assert_eq!(pixel(SIZE / 2, SIZE / 2), Rgba([0xF4, 0xB6, 0x3F, 255]));
        assert_eq!(pixel(SIZE / 2, TILE_INSET as u32 + 40)[3], 255);
    }
}
