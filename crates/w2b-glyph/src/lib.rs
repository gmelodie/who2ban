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

/// Bright enough to be a letter on a banner the client has lit.
pub const BRIGHTNESS: f32 = 0.75;

/// A banner is not lit for the whole draft: the client dims a seat once its pick is
/// locked, and a dimmed name sits far below the cutoff a lit one clears. On one 4K
/// draft the lit side put nine per cent of its pixels over `BRIGHTNESS` and the dimmed
/// side seven tenths of one per cent, which is not a poor read but no read at all.
///
/// No single cutoff serves both: dropping it far enough for a dimmed banner drags
/// scenery into a lit one. So every rung is read and the caller keeps the best answer,
/// which means the draft never has to be caught in the right state.
pub const BRIGHTNESS_LADDER: [f32; 3] = [BRIGHTNESS, 0.60, 0.45];

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
    /// The cutoff this was read at, which says how lit the banner was.
    pub threshold: f32,
}

impl Reading {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// The letters of one banner, in reading order, with the angle they were written at.
pub fn letters(rgb: &[u8], w: usize, h: usize) -> Option<(Vec<Blob>, f32)> {
    letters_at(rgb, w, h, BRIGHTNESS)
}

/// The same, at a stated cutoff, for a caller walking the ladder.
pub fn letters_at(rgb: &[u8], w: usize, h: usize, threshold: f32) -> Option<(Vec<Blob>, f32)> {
    let mask = Mask::from_rgb(rgb, w, h, threshold);
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
    read_at(rgb, w, h, atlas, BRIGHTNESS)
}

/// Read one banner at a stated cutoff.
pub fn read_at(rgb: &[u8], w: usize, h: usize, atlas: &Atlas, threshold: f32) -> Option<Reading> {
    let (letters, angle) = letters_at(rgb, w, h, threshold)?;
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
    Some(Reading { text, unread, angle, threshold })
}

/// Every reading the ladder yields, brightest rung first, skipping the rungs that saw
/// nothing. Choosing between them wants a way to tell a good answer from a bad one, so
/// that is left to the caller, who has the players the name could belong to.
pub fn read_ladder(rgb: &[u8], w: usize, h: usize, atlas: &Atlas) -> Vec<Reading> {
    BRIGHTNESS_LADDER
        .iter()
        .filter_map(|&t| read_at(rgb, w, h, atlas, t))
        .filter(|r| !r.is_empty())
        .collect()
}

/// File the shapes of a banner under the letters it is known to have said.
///
/// Returns whether anything was learned. A banner whose letters do not come to the same
/// count as the name is skipped entirely rather than lined up as best it can: one
/// mislabelled shape is worse than a hundred missing ones, because it is never
/// unlearned and it drags every later read towards the wrong letter.
pub fn learn(rgb: &[u8], w: usize, h: usize, truth: &str, atlas: &mut Atlas) -> bool {
    // A rung that cuts the banner into the wrong number of letters files nothing, so the
    // next one is tried rather than the banner being given up on. Reading a dimmed seat
    // and then never learning from it would starve the atlas of half of every draft.
    BRIGHTNESS_LADDER.iter().any(|&t| learn_at(rgb, w, h, truth, atlas, t))
}

/// File a banner's shapes at a stated cutoff.
pub fn learn_at(rgb: &[u8], w: usize, h: usize, truth: &str, atlas: &mut Atlas, threshold: f32) -> bool {
    let Some((letters, angle)) = letters_at(rgb, w, h, threshold) else {
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
