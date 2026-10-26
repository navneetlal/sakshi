//! Redraws the mark in the brand's PNG icons from the geometry in
//! `assets/brand/README.md`, keeping each file's tile (square or squircle)
//! pixel for pixel. Squares of 24 pt and up take the stacked cut; smaller
//! ones and favicons take the micro cut.
//!
//! Each icon draws its cut's field centred in the tile, 58% of the tile's
//! height tall. Before writing, the command checks that this placement
//! matches each file's existing glyph, so a different layout is caught
//! rather than overwritten.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use image::{Rgba, RgbaImage};

use crate::util::workspace_root;

const INK: [f32; 3] = [18.0, 21.0, 28.0];
const TALLY: [f32; 3] = [232.0, 146.0, 30.0];
const PAPER: [f32; 3] = [247.0, 247.0, 245.0];

/// The glyph's height as a share of the tile's.
const GLYPH_SHARE: f32 = 0.58;

/// One shape of a cut, in its field's units.
#[derive(Debug, Clone, Copy)]
enum Shape {
    Disc {
        cx: f32,
        cy: f32,
        r: f32,
    },
    Bar {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rx: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cut {
    Stacked,
    Micro,
}

impl Cut {
    fn field(self) -> (f32, f32) {
        match self {
            Cut::Stacked => (44.0, 64.0),
            Cut::Micro => (44.0, 46.0),
        }
    }

    /// README: "Stacked | 44 × 64 | disc cx 16 cy 16 r 16; bars x 0,
    /// y 40 / 49 / 58, w 44 / 34 / 22, h 6, rx 3" and "Micro | 44 × 46 |
    /// disc cx 15 cy 15 r 15; bar x 0 y 38 w 44 h 8 rx 4".
    fn shapes(self) -> Vec<Shape> {
        match self {
            Cut::Stacked => vec![
                Shape::Disc {
                    cx: 16.0,
                    cy: 16.0,
                    r: 16.0,
                },
                Shape::Bar {
                    x: 0.0,
                    y: 40.0,
                    w: 44.0,
                    h: 6.0,
                    rx: 3.0,
                },
                Shape::Bar {
                    x: 0.0,
                    y: 49.0,
                    w: 34.0,
                    h: 6.0,
                    rx: 3.0,
                },
                Shape::Bar {
                    x: 0.0,
                    y: 58.0,
                    w: 22.0,
                    h: 6.0,
                    rx: 3.0,
                },
            ],
            Cut::Micro => vec![
                Shape::Disc {
                    cx: 15.0,
                    cy: 15.0,
                    r: 15.0,
                },
                Shape::Bar {
                    x: 0.0,
                    y: 38.0,
                    w: 44.0,
                    h: 8.0,
                    rx: 4.0,
                },
            ],
        }
    }

    /// The stacked cut as the earlier assets drew it: smaller disc,
    /// centred bars. Used only to check the placement against old files.
    fn old_stacked() -> Vec<Shape> {
        vec![
            Shape::Disc {
                cx: 22.0,
                cy: 14.0,
                r: 14.0,
            },
            Shape::Bar {
                x: 0.0,
                y: 36.0,
                w: 44.0,
                h: 6.0,
                rx: 3.0,
            },
            Shape::Bar {
                x: 5.0,
                y: 45.0,
                w: 34.0,
                h: 6.0,
                rx: 3.0,
            },
            Shape::Bar {
                x: 11.0,
                y: 54.0,
                w: 22.0,
                h: 6.0,
                rx: 3.0,
            },
        ]
    }
}

/// Where a cut's field lands in an image: pixel = origin + field × scale.
#[derive(Debug, Clone, Copy)]
struct Placement {
    x: f32,
    y: f32,
    scale: f32,
}

#[derive(Debug, Clone, Copy)]
struct Tile {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Tile {
    fn place(self, cut: Cut) -> Placement {
        let (fw, fh) = cut.field();
        let scale = GLYPH_SHARE * self.h / fh;
        Placement {
            x: self.x + (self.w - fw * scale) / 2.0,
            y: self.y + (self.h - fh * scale) / 2.0,
            scale,
        }
    }
}

struct Icon {
    path: PathBuf,
    /// The size the icon is seen at, in points (an @2x file counts half).
    points: u32,
}

impl Icon {
    fn cut(&self) -> Cut {
        let favicon = self
            .path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("favicon"));
        if favicon || self.points < 24 {
            Cut::Micro
        } else {
            Cut::Stacked
        }
    }
}

pub fn run(write: bool) -> anyhow::Result<()> {
    let brand = workspace_root().join("assets/brand");
    let icons = list_icons(&brand)?;
    let mut changed = 0;
    for icon in &icons {
        let mut img = image::open(&icon.path)
            .with_context(|| format!("reading {}", icon.path.display()))?
            .to_rgba8();
        let tile = tile_of(&img).with_context(|| format!("no tile in {}", icon.path.display()))?;
        let target = icon.cut();
        let name = icon
            .path
            .strip_prefix(&brand)
            .unwrap_or(&icon.path)
            .display();

        // What's there now, and a check that it sits where the 58%
        // placement puts it before anything is replaced.
        let (found, error) = identify(&img, tile);
        println!(
            "{name:40} {}x{} {:>4} pt  has {found:?}, wants {target:?}, placement error {error:.1}",
            img.width(),
            img.height(),
            icon.points,
        );
        if error > 12.0 {
            bail!(
                "{name}: the existing glyph isn't where the 58% placement puts it; not touching it"
            );
        }
        let done = matches!(
            (found, target),
            (Found::Stacked, Cut::Stacked) | (Found::Micro, Cut::Micro)
        );
        if done {
            continue;
        }

        erase_glyph(&mut img, tile);
        draw(&mut img, tile.place(target), &target.shapes(), PAPER);
        changed += 1;
        if write {
            img.save(&icon.path)
                .with_context(|| format!("writing {}", icon.path.display()))?;
        }
    }
    if write {
        println!("redrew {changed} icons");
    } else {
        println!("{changed} icons would change; pass --write to save them");
    }
    Ok(())
}

fn list_icons(brand: &Path) -> anyhow::Result<Vec<Icon>> {
    let mut icons = Vec::new();
    for dir in ["icons", "icon-macos.iconset", "tauri-icons"] {
        for entry in std::fs::read_dir(brand.join(dir))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "png") {
                let points = points_of(&path)?;
                icons.push(Icon { path, points });
            }
        }
    }
    for name in ["favicon-16.png", "favicon-32.png"] {
        let path = brand.join(name);
        let points = points_of(&path)?;
        icons.push(Icon { path, points });
    }
    icons.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(icons)
}

/// Pixel size, halved for Apple's `@2x` files.
fn points_of(path: &Path) -> anyhow::Result<u32> {
    let (w, _) =
        image::image_dimensions(path).with_context(|| format!("reading {}", path.display()))?;
    let retina = path.to_string_lossy().contains("@2x");
    Ok(if retina { w / 2 } else { w })
}

/// The bounding box of the opaque tile.
fn tile_of(img: &RgbaImage) -> Option<Tile> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p[3] > 127 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    (x0 <= x1).then(|| Tile {
        x: x0 as f32,
        y: y0 as f32,
        w: (x1 - x0 + 1) as f32,
        h: (y1 - y0 + 1) as f32,
    })
}

/// How far a pixel is from ink, 0 to 1.
fn inkless(p: &Rgba<u8>) -> f32 {
    let d: f32 = (0..3).map(|i| (p[i] as f32 - INK[i]).abs()).sum();
    (d / 600.0).min(1.0)
}

/// A glyph an icon can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    /// The stacked cut with centred bars, as the assets first drew it.
    OldStacked,
    Stacked,
    Micro,
}

/// Which glyph the icon carries: the candidate that, drawn at the 58%
/// placement, differs least from the pixels. Returns that difference too.
fn identify(img: &RgbaImage, tile: Tile) -> (Found, f32) {
    [
        (Found::OldStacked, Cut::Stacked, Cut::old_stacked()),
        (Found::Stacked, Cut::Stacked, Cut::Stacked.shapes()),
        (Found::Micro, Cut::Micro, Cut::Micro.shapes()),
    ]
    .into_iter()
    .map(|(found, cut, shapes)| (found, placement_error(img, tile, cut, &shapes)))
    .min_by(|a, b| a.1.total_cmp(&b.1))
    .expect("three candidates")
}

/// Mean difference, in 0–255 units, between the image's glyph area and
/// `shapes` drawn at the 58% placement on plain ink.
fn placement_error(img: &RgbaImage, tile: Tile, cut: Cut, shapes: &[Shape]) -> f32 {
    let mut expected = img.clone();
    erase_glyph(&mut expected, tile);
    draw(&mut expected, tile.place(cut), shapes, PAPER);
    let (x0, y0, x1, y1) = inner_box(tile);
    let (mut sum, mut n) = (0.0f32, 0u32);
    for y in y0..y1 {
        for x in x0..x1 {
            let (a, b) = (img.get_pixel(x, y), expected.get_pixel(x, y));
            sum += (0..3)
                .map(|i| (a[i] as f32 - b[i] as f32).abs())
                .sum::<f32>()
                / 3.0;
            n += 1;
        }
    }
    sum / n.max(1) as f32
}

/// The central part of the tile, clear of its rounded corners and edges.
fn inner_box(tile: Tile) -> (u32, u32, u32, u32) {
    let inset = 0.12;
    (
        (tile.x + tile.w * inset).floor() as u32,
        (tile.y + tile.h * inset).floor() as u32,
        (tile.x + tile.w * (1.0 - inset)).ceil() as u32,
        (tile.y + tile.h * (1.0 - inset)).ceil() as u32,
    )
}

/// Paints the glyph area back to the tile's ink.
fn erase_glyph(img: &mut RgbaImage, tile: Tile) {
    let (x0, y0, x1, y1) = inner_box(tile);
    for y in y0..y1.min(img.height()) {
        for x in x0..x1.min(img.width()) {
            let p = img.get_pixel_mut(x, y);
            if inkless(p) > 0.0 {
                *p = Rgba([INK[0] as u8, INK[1] as u8, INK[2] as u8, p[3]]);
            }
        }
    }
}

/// Draws the shapes over the image with anti-aliased edges: the disc in
/// Tally, bars in `bars`.
fn draw(img: &mut RgbaImage, at: Placement, shapes: &[Shape], bars: [f32; 3]) {
    let samples = if at.scale < 2.0 { 8 } else { 4 };
    for shape in shapes {
        let (color, (fx0, fy0, fx1, fy1)) = match *shape {
            Shape::Disc { cx, cy, r } => (TALLY, (cx - r, cy - r, cx + r, cy + r)),
            Shape::Bar { x, y, w, h, .. } => (bars, (x, y, x + w, y + h)),
        };
        let px0 = (at.x + fx0 * at.scale).floor().max(0.0) as u32;
        let py0 = (at.y + fy0 * at.scale).floor().max(0.0) as u32;
        let px1 = ((at.x + fx1 * at.scale).ceil() as u32).min(img.width());
        let py1 = ((at.y + fy1 * at.scale).ceil() as u32).min(img.height());
        for py in py0..py1 {
            for px in px0..px1 {
                let mut hits = 0;
                for sy in 0..samples {
                    for sx in 0..samples {
                        let x = (px as f32 + (sx as f32 + 0.5) / samples as f32 - at.x) / at.scale;
                        let y = (py as f32 + (sy as f32 + 0.5) / samples as f32 - at.y) / at.scale;
                        if inside(*shape, x, y) {
                            hits += 1;
                        }
                    }
                }
                if hits == 0 {
                    continue;
                }
                let cover = hits as f32 / (samples * samples) as f32;
                let p = img.get_pixel_mut(px, py);
                for i in 0..3 {
                    p[i] = (color[i] * cover + p[i] as f32 * (1.0 - cover)).round() as u8;
                }
            }
        }
    }
}

fn inside(shape: Shape, x: f32, y: f32) -> bool {
    match shape {
        Shape::Disc { cx, cy, r } => (x - cx).powi(2) + (y - cy).powi(2) <= r * r,
        Shape::Bar {
            x: bx,
            y: by,
            w,
            h,
            rx,
        } => {
            if x < bx || x > bx + w || y < by || y > by + h {
                return false;
            }
            // Distance into a corner's rounding square, if in one.
            let cx = (bx + rx - x).max(x - (bx + w - rx)).max(0.0);
            let cy = (by + rx - y).max(y - (by + h - rx)).max(0.0);
            cx * cx + cy * cy <= rx * rx
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(size: f32) -> Tile {
        Tile {
            x: 0.0,
            y: 0.0,
            w: size,
            h: size,
        }
    }

    #[test]
    fn places_the_field_centred_at_58_percent() {
        // favicon.svg: translate(19.240 13.440) scale(0.58) in a 64 tile.
        let p = tile(64.0).place(Cut::Stacked);
        assert!((p.scale - 0.58).abs() < 1e-5);
        assert!((p.x - 19.24).abs() < 1e-3 && (p.y - 13.44).abs() < 1e-3);
    }

    #[test]
    fn small_icons_and_favicons_take_the_micro_cut() {
        let icon = |path: &str, points| Icon {
            path: PathBuf::from(path),
            points,
        };
        assert_eq!(icon("icons/icon-16.png", 16).cut(), Cut::Micro);
        assert_eq!(icon("icons/icon-24.png", 24).cut(), Cut::Stacked);
        assert_eq!(icon("favicon-32.png", 32).cut(), Cut::Micro);
        assert_eq!(icon("icon_16x16@2x.png", 16).cut(), Cut::Micro);
    }

    #[test]
    fn draws_a_glyph_that_reads_back_as_its_cut() {
        for cut in [Cut::Stacked, Cut::Micro] {
            let mut img = RgbaImage::from_pixel(256, 256, Rgba([18, 21, 28, 255]));
            let t = tile(256.0);
            draw(&mut img, t.place(cut), &cut.shapes(), PAPER);
            let (found, error) = identify(&img, t);
            let expected = match cut {
                Cut::Stacked => Found::Stacked,
                Cut::Micro => Found::Micro,
            };
            assert_eq!(found, expected);
            assert!(error < 0.5);
        }
        let mut old = RgbaImage::from_pixel(64, 64, Rgba([18, 21, 28, 255]));
        draw(
            &mut old,
            tile(64.0).place(Cut::Stacked),
            &Cut::old_stacked(),
            PAPER,
        );
        assert_eq!(identify(&old, tile(64.0)).0, Found::OldStacked);
    }

    #[test]
    fn bars_have_round_ends() {
        let bar = Shape::Bar {
            x: 0.0,
            y: 0.0,
            w: 44.0,
            h: 6.0,
            rx: 3.0,
        };
        assert!(inside(bar, 22.0, 3.0));
        assert!(inside(bar, 3.0, 3.0));
        assert!(!inside(bar, 0.2, 0.2));
        assert!(!inside(bar, 43.8, 5.8));
    }
}
