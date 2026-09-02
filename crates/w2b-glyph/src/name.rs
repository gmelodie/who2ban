//! Turning a read that is nearly right into the battletag it meant, or into nothing.
//!
//! A read is never trusted on its own. It is only ever used to pick out a player the
//! database already knows, which is the only kind of player there is anything to show
//! about, and it is thrown away unless one candidate wins clearly.

/// Above this share of the name misread, the read is too poor to place at all, however
/// lonely its best candidate. Deliberately generous: a thin atlas misreads plenty and
/// still says which player it was, and it is the margin below that decides.
pub const MAX_SCORE: f32 = 0.45;

/// How far clear of the next candidate the best one has to be.
///
/// This, not the score, is what makes an answer safe. A read that half matches one
/// player and nobody else names that player; a read that half matches four names them
/// all equally badly and so names none of them. Judging on the score alone throws away
/// good answers from a thin atlas and keeps bad ones from a crowded pool.
pub const MIN_MARGIN: f32 = 0.15;

/// What an unread letter costs against a candidate. A letter the atlas could not place
/// is not evidence against a name the way a letter read as something else is: it is the
/// absence of evidence. Charging it nothing is what let a read of nothing but holes
/// match whoever was shortest, so it costs something, but less than being wrong.
pub const UNREAD_COST: f32 = 0.5;

/// A read mostly made of holes is not a poor read of somebody, it is not a read at all.
/// This is checked before any candidate is scored, so no amount of cheap holes can add
/// up to a match.
pub const MAX_UNREAD_SHARE: f32 = 0.60;

/// The letter a reader writes for one it could not place.
pub const UNREAD: char = '?';

/// How many letters a read may differ from a name by and still be taken for it.
///
/// The segmenter either finds a letter or does not; it does not quietly drop a third of
/// them. So a read of three shapes is not a poor read of a five letter name, it is a
/// read of something else, however well those three happen to line up. Without this,
/// short fragments match short names for reasons that have nothing to do with the
/// screen.
fn length_allowance(name_len: usize) -> usize {
    (name_len / 5).max(1)
}

/// Whether a read is even the right shape to be this name.
fn comparable(reading_len: usize, name_len: usize) -> bool {
    reading_len.abs_diff(name_len) <= length_allowance(name_len)
}

/// Edit distance, case-insensitive, counted in whole letters.
pub fn edits(a: &str, b: &str) -> usize {
    cost(a, b, 1.0).round() as usize
}

/// Edit distance where an unread letter is charged `unread` rather than a full swap.
fn cost(a: &str, b: &str, unread: f32) -> f32 {
    let a: Vec<char> = a.chars().flat_map(char::to_lowercase).collect();
    let b: Vec<char> = b.chars().flat_map(char::to_lowercase).collect();
    let mut prev: Vec<f32> = (0..=b.len()).map(|i| i as f32).collect();
    let mut cur = vec![0f32; b.len() + 1];

    for i in 1..=a.len() {
        cur[0] = i as f32;
        for j in 1..=b.len() {
            let swap = if a[i - 1] == b[j - 1] {
                0.0
            } else if a[i - 1] == UNREAD {
                unread
            } else {
                1.0
            };
            cur[j] = (prev[j] + 1.0).min(cur[j - 1] + 1.0).min(prev[j - 1] + swap);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Whether a read holds enough placed letters to be worth scoring at all.
pub fn legible(reading: &str) -> bool {
    let total = reading.chars().count();
    if total == 0 {
        return false;
    }
    let holes = reading.chars().filter(|c| *c == UNREAD).count();
    (holes as f32 / total as f32) <= MAX_UNREAD_SHARE
}

/// A player the read might have been.
#[derive(Debug, Clone)]
pub struct Found {
    pub battletag: String,
    /// Share of the name that had to be changed, so nought is a perfect read.
    pub score: f32,
    /// How much worse the next candidate was.
    pub margin: f32,
}

/// The one player this read can only have meant. `None` when the read is too poor to
/// place, or when two players fit it equally well, both of which are worth saying
/// nothing about rather than showing somebody the wrong pool mid-draft.
pub fn identify(reading: &str, pool: &[(String, String)]) -> Option<Found> {
    if !legible(reading) {
        return None;
    }
    let scored = rank(reading, pool)?;
    let found = scored.0;
    (found.score <= MAX_SCORE && found.margin >= MIN_MARGIN).then_some(found)
}

/// The best candidate and the gap to the next, whatever the quality. Kept apart from
/// `identify` so a caller can report a near miss without being handed one to act on.
pub fn rank(reading: &str, pool: &[(String, String)]) -> Option<(Found, String)> {
    if reading.is_empty() || pool.is_empty() {
        return None;
    }
    let read_len = reading.chars().count();
    // Candidates of a wholly different length are not near misses, and letting them
    // crowd the ranking would also shrink the margin that decides the answer.
    let mut scored: Vec<(f32, &String, &String)> = pool
        .iter()
        .filter(|(name, _)| comparable(read_len, name.chars().count()))
        .map(|(name, tag)| {
            let d = cost(reading, name, UNREAD_COST) / name.chars().count().max(1) as f32;
            (d, name, tag)
        })
        .collect();
    if scored.is_empty() {
        return None;
    }
    scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let (score, _, tag) = scored[0];
    let next = scored.get(1).map_or(f32::MAX, |s| s.0);
    Some((
        Found {
            battletag: tag.clone(),
            score,
            margin: next - score,
        },
        scored.get(1).map_or(String::new(), |s| s.1.clone()),
    ))
}
