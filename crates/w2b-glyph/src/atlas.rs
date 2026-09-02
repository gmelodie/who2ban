//! The learned shapes of letters, and the business of recognising one.
//!
//! Nothing here knows what font the game uses. The shapes are cut from screenshots the
//! client took itself, which is why they carry the right anti-aliasing, the right slant
//! and the right banner behind them without anyone having to find a font file.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::segment::Blob;

/// Every letter is squashed into this square before it is compared with another, so a
/// wide `m` and a narrow `l` are judged on shape rather than on size.
pub const CELL: usize = 20;

/// One letter, deskewed and normalised.
///
/// Squashing every letter into the same square is what lets a wide `m` be compared with
/// a narrow `l`, but it also throws away the one thing that separates `S` from `s` and
/// `I` from `l`, which is how tall they were. `height` puts that back: it is the
/// letter's height as a share of the median letter on its own banner, so it means the
/// same thing whatever resolution the screen was.
#[derive(Clone)]
pub struct Glyph {
    pub cells: Vec<u8>,
    pub height: f32,
}

/// How much a difference in height counts against a difference in shape. Enough to part
/// a capital from its lowercase, not so much that one clipped letter is unrecognisable.
const HEIGHT_WEIGHT: f32 = 0.9;

impl Glyph {
    /// Nought for the same letter drawn the same size, one for nothing alike.
    pub fn distance(&self, other: &Glyph) -> f32 {
        let shape = distance(&self.cells, &other.cells);
        let size = (self.height - other.height).abs().min(1.0);
        (shape + HEIGHT_WEIGHT * size) / (1.0 + HEIGHT_WEIGHT)
    }
}

fn distance(a: &[u8], b: &[u8]) -> f32 {
    let total: u32 = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b) as u32).sum();
    total as f32 / (a.len() as f32 * 255.0)
}

/// Undo the thirty degree tilt and squash what is left into `CELL` by `CELL`. The slant
/// of the italic survives on purpose: the learned shapes are slanted too.
pub fn render(blob: &Blob, angle_degrees: f32) -> Option<Glyph> {
    let (cells, height) = render_raw(blob, angle_degrees)?;
    Some(Glyph { cells, height })
}

/// The shape, and how tall it stood once the tilt was taken out of it.
pub fn render_raw(blob: &Blob, angle_degrees: f32) -> Option<(Vec<u8>, f32)> {
    let (w, h) = (blob.width(), blob.height());
    let mut tile = vec![0u8; w * h];
    for &(y, x) in &blob.pixels {
        tile[(y - blob.y0) * w + (x - blob.x0)] = 255;
    }

    let theta = angle_degrees.to_radians();
    let (ct, st) = (theta.cos(), theta.sin());

    // Where the corners land decides how big the upright picture has to be.
    let corners = [(0.0, 0.0), (w as f32, 0.0), (0.0, h as f32), (w as f32, h as f32)];
    let (mut lo_x, mut lo_y, mut hi_x, mut hi_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in corners {
        let (x, y) = (x - w as f32 / 2.0, y - h as f32 / 2.0);
        let rx = x * ct + y * st;
        let ry = -x * st + y * ct;
        lo_x = lo_x.min(rx);
        hi_x = hi_x.max(rx);
        lo_y = lo_y.min(ry);
        hi_y = hi_y.max(ry);
    }
    let (dw, dh) = ((hi_x - lo_x).ceil() as usize, (hi_y - lo_y).ceil() as usize);
    if dw == 0 || dh == 0 {
        return None;
    }

    let mut upright = vec![0u8; dw * dh];
    for dy in 0..dh {
        for dx in 0..dw {
            let px = dx as f32 + lo_x + 0.5;
            let py = dy as f32 + lo_y + 0.5;
            let sx = px * ct - py * st + w as f32 / 2.0;
            let sy = px * st + py * ct + h as f32 / 2.0;
            upright[dy * dw + dx] = sample(&tile, w, h, sx, sy);
        }
    }

    let (bx0, by0, bx1, by1) = ink_bounds(&upright, dw, dh)?;
    let (bw, bh) = (bx1 - bx0 + 1, by1 - by0 + 1);
    let mut cells = vec![0u8; CELL * CELL];
    for cy in 0..CELL {
        for cx in 0..CELL {
            let sx = bx0 as f32 + (cx as f32 + 0.5) * bw as f32 / CELL as f32;
            let sy = by0 as f32 + (cy as f32 + 0.5) * bh as f32 / CELL as f32;
            cells[cy * CELL + cx] = sample(&upright, dw, dh, sx, sy);
        }
    }
    Some((cells, bh as f32))
}

fn sample(buf: &[u8], w: usize, h: usize, x: f32, y: f32) -> u8 {
    let (x, y) = (x - 0.5, y - 0.5);
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let get = |yy: i32, xx: i32| -> f32 {
        if yy < 0 || xx < 0 || yy >= h as i32 || xx >= w as i32 {
            0.0
        } else {
            buf[yy as usize * w + xx as usize] as f32
        }
    };
    let top = get(y0, x0) * (1.0 - fx) + get(y0, x0 + 1) * fx;
    let bottom = get(y0 + 1, x0) * (1.0 - fx) + get(y0 + 1, x0 + 1) * fx;
    (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8
}

fn ink_bounds(buf: &[u8], w: usize, h: usize) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            if buf[y * w + x] > 60 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    (x0 != usize::MAX).then_some((x0, y0, x1, y1))
}

/// What the client has learned a letter looks like. Several shapes are kept per letter,
/// because the same `e` is drawn a little differently on a light banner and a dark one.
/// A filed shape: the picture, and how tall the letter stood.
#[derive(Clone, Serialize, Deserialize)]
pub struct Shape {
    cells: Vec<u8>,
    height: f32,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Atlas {
    /// Keyed by the letter, which serialises as a string because JSON has no char.
    shapes: BTreeMap<String, Vec<Shape>>,
    /// Beyond this many examples of one letter, another adds nothing but disk.
    #[serde(default = "default_per_letter")]
    per_letter: usize,
}

fn default_per_letter() -> usize {
    24
}

/// What a shape was judged to be, and how much better than the next guess.
#[derive(Debug, Clone, Copy)]
pub struct Verdict {
    pub letter: char,
    pub distance: f32,
    /// Distance to the best shape of any *other* letter. A small gap is a coin toss.
    pub runner_up: f32,
}

impl Atlas {
    pub fn new() -> Atlas {
        Atlas {
            shapes: BTreeMap::new(),
            per_letter: default_per_letter(),
        }
    }

    pub fn letters(&self) -> usize {
        self.shapes.len()
    }

    pub fn examples(&self) -> usize {
        self.shapes.values().map(Vec::len).sum()
    }

    pub fn knows(&self, letter: char) -> bool {
        self.shapes.contains_key(&letter.to_string())
    }

    /// A shape already all but identical to one on file teaches nothing.
    pub fn learn(&mut self, letter: char, glyph: &Glyph) {
        let slot = self.shapes.entry(letter.to_string()).or_default();
        if slot.len() >= self.per_letter {
            return;
        }
        let known = slot.iter().any(|s| {
            distance(&s.cells, &glyph.cells) < 0.02 && (s.height - glyph.height).abs() < 0.08
        });
        if !known {
            slot.push(Shape {
                cells: glyph.cells.clone(),
                height: glyph.height,
            });
        }
    }

    pub fn classify(&self, glyph: &Glyph) -> Option<Verdict> {
        // The closest example of each letter, then the two best letters. Keeping the
        // runner-up is the whole point: the gap between them is the confidence.
        let mut scored: Vec<(char, f32)> = self
            .shapes
            .iter()
            .filter_map(|(letter, shapes)| {
                let closest = shapes
                    .iter()
                    .map(|s| {
                        Glyph {
                            cells: s.cells.clone(),
                            height: s.height,
                        }
                        .distance(glyph)
                    })
                    .fold(f32::MAX, f32::min);
                Some((letter.chars().next()?, closest))
            })
            .collect();
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let (letter, distance) = *scored.first()?;
        Some(Verdict {
            letter,
            distance,
            runner_up: scored.get(1).map_or(f32::MAX, |s| s.1),
        })
    }

    pub fn load(path: &Path) -> std::io::Result<Atlas> {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(std::io::Error::other)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string(self).map_err(std::io::Error::other)?;
        // Written beside and moved, so a crash mid-write cannot cost what was learned.
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, text)?;
        std::fs::rename(temp, path)
    }
}
