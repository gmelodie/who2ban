//! Turning a picture of a name banner into the shapes of its letters.
//!
//! The draft screen writes each player's name in light glyphs on a banner that is dark
//! behind them and tilted thirty degrees. Everything here works on that one idea: the
//! letters are the bright pixels, and the ones that share a straight line are the name.

/// Bright pixels, which on both the blue banner and the red one are the name.
pub struct Mask {
    pub w: usize,
    pub h: usize,
    on: Vec<bool>,
}

impl Mask {
    /// Brightness is the largest of the three channels, not their weighted average. The
    /// enemy banner writes in pink, which is dark by luminance and bright by this, and a
    /// luminance threshold that keeps it also keeps half the banner.
    pub fn from_rgb(rgb: &[u8], w: usize, h: usize, threshold: f32) -> Mask {
        let cut = (threshold * 255.0) as u8;
        let on = (0..w * h)
            .map(|i| {
                let p = &rgb[i * 3..i * 3 + 3];
                p[0].max(p[1]).max(p[2]) >= cut
            })
            .collect();
        Mask { w, h, on }
    }

    pub fn at(&self, y: usize, x: usize) -> bool {
        self.on[y * self.w + x]
    }
}

/// One run of touching bright pixels: a letter, part of one, or a piece of scenery.
#[derive(Clone)]
pub struct Blob {
    pub pixels: Vec<(usize, usize)>,
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
    pub cx: f32,
    pub cy: f32,
    /// How far along the baseline this sits, which is the order it is read in.
    pub t: f32,
}

impl Blob {
    pub fn width(&self) -> usize {
        self.x1 - self.x0 + 1
    }

    pub fn height(&self) -> usize {
        self.y1 - self.y0 + 1
    }
}

/// Eight-connected, so a diagonal stroke is one letter and not a dotted line of them.
pub fn blobs(mask: &Mask, min_pixels: usize) -> Vec<Blob> {
    let mut seen = vec![false; mask.w * mask.h];
    let mut out = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();

    for sy in 0..mask.h {
        for sx in 0..mask.w {
            if !mask.at(sy, sx) || seen[sy * mask.w + sx] {
                continue;
            }
            seen[sy * mask.w + sx] = true;
            stack.push((sy, sx));
            let mut pixels = Vec::new();

            while let Some((y, x)) = stack.pop() {
                pixels.push((y, x));
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let ny = y as i32 + dy;
                        let nx = x as i32 + dx;
                        if ny < 0 || nx < 0 || ny >= mask.h as i32 || nx >= mask.w as i32 {
                            continue;
                        }
                        let (ny, nx) = (ny as usize, nx as usize);
                        if mask.at(ny, nx) && !seen[ny * mask.w + nx] {
                            seen[ny * mask.w + nx] = true;
                            stack.push((ny, nx));
                        }
                    }
                }
            }

            if pixels.len() < min_pixels {
                continue;
            }
            let (mut sy0, mut sx0, mut sy1, mut sx1) = (usize::MAX, usize::MAX, 0, 0);
            let (mut ty, mut tx) = (0f32, 0f32);
            for &(y, x) in &pixels {
                sy0 = sy0.min(y);
                sx0 = sx0.min(x);
                sy1 = sy1.max(y);
                sx1 = sx1.max(x);
                ty += y as f32;
                tx += x as f32;
            }
            let n = pixels.len() as f32;
            out.push(Blob {
                pixels,
                x0: sx0,
                y0: sy0,
                x1: sx1,
                y1: sy1,
                cx: tx / n,
                cy: ty / n,
                t: 0.0,
            });
        }
    }
    out
}

/// A letter is neither the whole banner nor a speck of anti-aliasing.
pub fn letter_sized(found: Vec<Blob>, area: usize) -> Vec<Blob> {
    found
        .into_iter()
        .filter(|b| {
            b.pixels.len() <= area / 50
                && (5..=45).contains(&b.height())
                && (2..=45).contains(&b.width())
        })
        .collect()
}

/// Blobs to try pairing off when looking for the baseline. A battletag is twelve
/// letters; anything past this is scenery, and pairing all of it is cubic work.
const MOST_CANDIDATES: usize = 48;

/// The line a name is written along.
#[derive(Clone, Copy, Debug)]
pub struct Baseline {
    pub slope: f32,
    pub intercept: f32,
}

impl Baseline {
    pub fn y_at(&self, x: f32) -> f32 {
        self.slope * x + self.intercept
    }

    pub fn angle_degrees(&self) -> f32 {
        self.slope.atan().to_degrees()
    }
}

/// The largest set of letters sharing one straight line is the name; whatever else the
/// crop caught is scenery. Every pair is tried because a name is short enough that it
/// costs nothing, and because seeding from a stray would cost the whole read.
pub fn fit_baseline(found: &[Blob], tolerance: f32) -> Option<Baseline> {
    if found.len() < 2 {
        return None;
    }
    // Every pair, then every blob against each: fine for the dozen letters of a name,
    // cubic on a crop that caught a busy corner of the screen. A name is never the
    // smallest specks in its own box, so only the largest are worth pairing off.
    let mut order: Vec<usize> = (0..found.len()).collect();
    if order.len() > MOST_CANDIDATES {
        order.sort_by_key(|&i| std::cmp::Reverse(found[i].pixels.len()));
        order.truncate(MOST_CANDIDATES);
    }

    let mut best = (0usize, Baseline { slope: 0.0, intercept: 0.0 });

    for (n, &i) in order.iter().enumerate() {
        for &j in &order[n + 1..] {
            let (a, b) = (&found[i], &found[j]);
            if (b.cx - a.cx).abs() < 1e-6 {
                continue;
            }
            let slope = (b.cy - a.cy) / (b.cx - a.cx);
            // A name is written across the banner, never up it.
            if slope.abs() > 1.2 {
                continue;
            }
            let line = Baseline {
                slope,
                intercept: a.cy - slope * a.cx,
            };
            let hits = found
                .iter()
                .filter(|c| (c.cy - line.y_at(c.cx)).abs() <= tolerance)
                .count();
            if hits > best.0 {
                best = (hits, line);
            }
        }
    }
    if best.0 < 2 {
        return None;
    }

    // Refit on everything that agreed, so two seed points do not decide the angle.
    let inliers: Vec<&Blob> = found
        .iter()
        .filter(|c| (c.cy - best.1.y_at(c.cx)).abs() <= tolerance)
        .collect();
    Some(least_squares(&inliers).unwrap_or(best.1))
}

fn least_squares(points: &[&Blob]) -> Option<Baseline> {
    let n = points.len() as f32;
    if n < 2.0 {
        return None;
    }
    let sx: f32 = points.iter().map(|p| p.cx).sum();
    let sy: f32 = points.iter().map(|p| p.cy).sum();
    let sxx: f32 = points.iter().map(|p| p.cx * p.cx).sum();
    let sxy: f32 = points.iter().map(|p| p.cx * p.cy).sum();
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-6 {
        return None;
    }
    let slope = (n * sxy - sx * sy) / denom;
    Some(Baseline {
        slope,
        intercept: (sy - slope * sx) / n,
    })
}

/// The letters of the name, left to right along the banner rather than across the
/// picture, with the dot of an `i` put back onto its stem.
pub fn letters_along(found: Vec<Blob>, line: &Baseline, tolerance: f32) -> Vec<Blob> {
    let angle = line.slope.atan();
    let (ca, sa) = (angle.cos(), angle.sin());

    let mut kept: Vec<Blob> = found
        .into_iter()
        .filter(|b| (b.cy - line.y_at(b.cx)).abs() <= tolerance)
        .map(|mut b| {
            b.t = b.cx * ca + b.cy * sa;
            b
        })
        .collect();
    kept.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));

    let mut heights: Vec<usize> = kept.iter().map(|b| b.height()).collect();
    heights.sort_unstable();
    let median = heights.get(heights.len() / 2).copied().unwrap_or(10) as f32;

    merge_marks(kept, median * 0.30)
}

/// An `i` is a stem and a dot standing in the same place along the line, and so is a `j`.
/// The gap is deliberately mean: hold it too wide and a narrow capital swallows the
/// letter beside it.
fn merge_marks(letters: Vec<Blob>, gap: f32) -> Vec<Blob> {
    let mut out: Vec<Blob> = Vec::new();
    for b in letters {
        match out.last_mut() {
            Some(prev) if (b.t - prev.t).abs() < gap => {
                prev.x0 = prev.x0.min(b.x0);
                prev.y0 = prev.y0.min(b.y0);
                prev.x1 = prev.x1.max(b.x1);
                prev.y1 = prev.y1.max(b.y1);
                prev.t = (prev.t + b.t) / 2.0;
                prev.pixels.extend(b.pixels);
            }
            _ => out.push(b),
        }
    }
    out
}
