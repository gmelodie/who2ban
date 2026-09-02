//! Reading the ten player names off the draft screen.
//!
//! The game writes no file while a draft is running: the battlelobby only appears once
//! the loading screen does, by which time the bans are spent. The names are on the
//! screen throughout, though, so this reads them off it.
//!
//! Nothing here needs the game's font. The client screenshots its own draft, and when
//! the battlelobby turns up at load it says what those names were; the shapes are cut
//! out and filed against the letters they turned out to be. What the client learns is
//! drawn by the same machine at the same size, which is the only rendering it will ever
//! have to read.

pub mod atlas;
pub mod geometry;
pub mod name;
pub mod segment;

pub use atlas::{Atlas, CELL, Glyph, Verdict};
pub use segment::{Baseline, Blob, Mask};

/// Bright enough to be a letter on either team's banner.
pub const BRIGHTNESS: f32 = 0.75;

/// How far off the line a letter may sit, in pixels, before it is scenery.
const OFF_LINE: f32 = 9.0;

/// A shape further than this from everything on file is not being recognised, it is
/// being rounded to the nearest thing that happens to be there.
const MAX_GLYPH_DISTANCE: f32 = 0.30;

/// And a shape that fits two letters about equally is not recognised either.
const MIN_GLYPH_MARGIN: f32 = 0.02;

/// The letter that stands for one the atlas could not place.
pub use name::UNREAD;

/// What a banner was read as.
#[derive(Debug, Clone)]
pub struct Reading {
    pub text: String,
    /// Letters that came out as `UNREAD`.
    pub unread: usize,
    /// The tilt of the banner, which is about thirty degrees either way.
    pub angle: f32,
}

impl Reading {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// The letters of one banner, in reading order, with the angle they were written at.
pub fn letters(rgb: &[u8], w: usize, h: usize) -> Option<(Vec<Blob>, f32)> {
    let mask = Mask::from_rgb(rgb, w, h, BRIGHTNESS);
    let found = segment::letter_sized(segment::blobs(&mask, 18), w * h);
    let line = segment::fit_baseline(&found, OFF_LINE)?;
    let letters = segment::letters_along(found, &line, OFF_LINE);
    (!letters.is_empty()).then(|| (letters, line.angle_degrees()))
}

/// The shapes of a banner's letters, each sized against the median letter beside it.
/// Relative rather than absolute, so the same atlas reads a 1080p screen and a 4K one.
fn shapes(letters: &[Blob], angle: f32) -> Vec<Option<Glyph>> {
    let raw: Vec<Option<(Vec<u8>, f32)>> = letters
        .iter()
        .map(|b| atlas::render_raw(b, angle))
        .collect();

    let mut heights: Vec<f32> = raw.iter().filter_map(|r| r.as_ref().map(|(_, h)| *h)).collect();
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = heights.get(heights.len() / 2).copied().unwrap_or(1.0).max(1.0);

    raw.into_iter()
        .map(|r| {
            r.map(|(cells, h)| Glyph {
                cells,
                height: h / median,
            })
        })
        .collect()
}

/// Read one banner. Letters the atlas cannot place come back as `UNREAD` rather than as
/// the closest thing on file, so a thin atlas produces a poor read and not a wrong one.
pub fn read(rgb: &[u8], w: usize, h: usize, atlas: &Atlas) -> Option<Reading> {
    let (letters, angle) = letters(rgb, w, h)?;
    let mut text = String::new();
    let mut unread = 0;

    for shape in shapes(&letters, angle) {
        let placed = shape.and_then(|g| atlas.classify(&g)).filter(|v| {
            v.distance <= MAX_GLYPH_DISTANCE && (v.runner_up - v.distance) >= MIN_GLYPH_MARGIN
        });
        match placed {
            Some(v) => text.push(v.letter),
            None => {
                text.push(UNREAD);
                unread += 1;
            }
        }
    }
    Some(Reading { text, unread, angle })
}

/// File the shapes of a banner under the letters it is known to have said.
///
/// Returns whether anything was learned. A banner whose letters do not come to the same
/// count as the name is skipped entirely rather than lined up as best it can: one
/// mislabelled shape is worse than a hundred missing ones, because it is never
/// unlearned and it drags every later read towards the wrong letter.
pub fn learn(rgb: &[u8], w: usize, h: usize, truth: &str, atlas: &mut Atlas) -> bool {
    let Some((letters, angle)) = letters(rgb, w, h) else {
        return false;
    };
    // Spaces are gaps, not shapes, so a name is filed under what was actually drawn.
    let wanted: Vec<char> = truth.chars().filter(|c| !c.is_whitespace()).collect();
    if letters.len() != wanted.len() {
        return false;
    }
    for (shape, letter) in shapes(&letters, angle).into_iter().zip(wanted) {
        if let Some(glyph) = shape {
            atlas.learn(letter, &glyph);
        }
    }
    true
}
