//! Telling the client's own menus apart from a draft.
//!
//! The ten banner boxes are read wherever the game window is, and on a menu screen they
//! hold nothing but scenery: a planet, the stars behind it, the grid on the floor. Three
//! boxes of that were enough to clear `LEAST_SEATS` and call it a draft, and the fuzzy
//! match then seated real battletags read out of starfield. So the screen is asked what
//! it is before it is asked who is on it.
//!
//! The tell is the Nexus button in the top-left corner. The client draws it on every
//! screen outside a game and on no part of the draft, so a strong match for it settles
//! the question without reading a single name.

use std::sync::OnceLock;

use w2b_glyph::geometry::{self, Share};

/// Where the button sits, as a share of the game's window. Measured off a 3840 by 2160
/// capture of the client's Play screen, with room left around the hexagon so the whole
/// of it is still inside the box on a window of another size.
pub const BADGE: Share = Share {
    x0: 0.0086,
    y0: 0.0056,
    x1: 0.0367,
    y1: 0.0505,
};

/// The button on a menu, already reduced to the grid that gets compared.
const REFERENCE: &[u8] = include_bytes!("../assets/menu-badge.png");

/// The side of that grid. Coarse on purpose: the glow around the hexagon breathes, and
/// the button lights up under the pointer, and at this size neither moves a cell far.
const SIDE: usize = 32;

/// Below this the corner is holding something else. Measured at 1.00 against the capture
/// the reference was cut from and 0.07 against a Storm League draft, so the line sits far
/// from both readings. It is drawn nearer the draft than the menu deliberately: calling a
/// menu a draft costs a phantom lobby, calling a draft a menu costs the whole feature.
pub const ALIKE: f32 = 0.6;

/// Where to grab, for a window of this size.
pub fn badge_box(w: usize, h: usize) -> Option<geometry::Box> {
    geometry::region(&BADGE, w, h)
}

/// The reference grid, decoded once. `None` if the asset will not decode or is not the
/// size this compares at, in which case nothing here can answer and the reader carries
/// on as it did before.
fn reference() -> Option<&'static [u8]> {
    static CELLS: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    CELLS
        .get_or_init(|| {
            let img =
                image::load_from_memory_with_format(REFERENCE, image::ImageFormat::Png).ok()?;
            let grey = img.to_luma8();
            (grey.width() as usize == SIDE && grey.height() as usize == SIDE)
                .then(|| grey.into_raw())
        })
        .as_deref()
}

/// A plain box average down to the comparison grid. The reference was reduced by the
/// same arithmetic, so the two sides are alike where the pictures are and nowhere else.
fn shrink(rgb: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    if w < SIDE || h < SIDE || rgb.len() < w * h * 3 {
        return None;
    }
    let mut cells = Vec::with_capacity(SIDE * SIDE);
    for gy in 0..SIDE {
        let y0 = gy * h / SIDE;
        let y1 = ((gy + 1) * h / SIDE).max(y0 + 1);
        for gx in 0..SIDE {
            let x0 = gx * w / SIDE;
            let x1 = ((gx + 1) * w / SIDE).max(x0 + 1);
            let mut total = 0u32;
            let mut seen = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * w + x) * 3;
                    let r = u32::from(rgb[i]);
                    let g = u32::from(rgb[i + 1]);
                    let b = u32::from(rgb[i + 2]);
                    total += (r * 299 + g * 587 + b * 114) / 1000;
                    seen += 1;
                }
            }
            cells.push((total / seen) as u8);
        }
    }
    Some(cells)
}

/// Correlation, so a corner that is the button but dimmer, or brighter, still answers
/// yes. A flat grab has no shape to correlate and answers no.
fn correlation(a: &[u8], b: &[u8]) -> f32 {
    let n = a.len() as f32;
    let mean = |v: &[u8]| v.iter().map(|&c| f32::from(c)).sum::<f32>() / n;
    let (ma, mb) = (mean(a), mean(b));
    let (mut both, mut da, mut db) = (0.0f32, 0.0f32, 0.0f32);
    for (&x, &y) in a.iter().zip(b) {
        let (x, y) = (f32::from(x) - ma, f32::from(y) - mb);
        both += x * y;
        da += x * x;
        db += y * y;
    }
    if da <= 0.0 || db <= 0.0 {
        return 0.0;
    }
    both / (da.sqrt() * db.sqrt())
}

/// How much this grab of the corner looks like the Nexus button, from -1 to 1. `None`
/// when there is nothing to compare with, which is not an answer either way.
pub fn badge_likeness(rgb: &[u8], w: usize, h: usize) -> Option<f32> {
    let seen = shrink(rgb, w, h)?;
    Some(correlation(reference()?, &seen))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (Vec<u8>, usize, usize) {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let img = image::open(path).unwrap().to_rgb8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        (img.into_raw(), w, h)
    }

    /// The corner the reference was cut from, which is the easy half.
    #[test]
    fn a_menu_corner_is_the_button() {
        let (rgb, w, h) = fixture("menu-top-left.png");
        let alike = badge_likeness(&rgb, w, h).unwrap();
        assert!(alike >= ALIKE, "{alike}");
    }

    /// The half that matters: a draft must never be mistaken for a menu, or the reader
    /// stays shut for the one screen it exists to read.
    #[test]
    fn a_draft_corner_is_not() {
        let (rgb, w, h) = fixture("draft-top-left.png");
        let alike = badge_likeness(&rgb, w, h).unwrap();
        assert!(alike < ALIKE, "{alike}");
    }

    /// And the two readings are far enough apart that the threshold is not a knife edge.
    #[test]
    fn the_two_are_not_close() {
        let (menu, mw, mh) = fixture("menu-top-left.png");
        let (draft, dw, dh) = fixture("draft-top-left.png");
        let menu = badge_likeness(&menu, mw, mh).unwrap();
        let draft = badge_likeness(&draft, dw, dh).unwrap();
        assert!(menu - draft > 0.5, "menu {menu}, draft {draft}");
    }

    /// A grab of one flat colour correlates with nothing.
    #[test]
    fn a_blank_grab_is_not_a_menu() {
        let rgb = vec![17u8; 64 * 64 * 3];
        assert_eq!(badge_likeness(&rgb, 64, 64), Some(0.0));
    }

    /// A window too small to hold the grid is no answer rather than a wrong one.
    #[test]
    fn too_small_to_compare_says_nothing() {
        let rgb = vec![0u8; 8 * 8 * 3];
        assert_eq!(badge_likeness(&rgb, 8, 8), None);
    }
}
