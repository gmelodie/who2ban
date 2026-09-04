//! Reading the draft off the screen, for the minutes before the game writes anything.
//!
//! The battlelobby only lands when the loading screen does, which is after the bans are
//! spent. The names are on the screen throughout the draft, so this reads them there,
//! and when the battlelobby finally arrives it is used to mark the reader's homework:
//! the shapes it saw are filed under the letters they turned out to be.

use std::path::{Path, PathBuf};

use w2b_glyph::{Atlas, Reading, geometry, name};
use w2b_parse::{Lobby, LobbyPlayer};

/// The title the client gives its window.
const GAME_WINDOW: &str = "Heroes of the Storm";

/// Shapes the client has not learned for itself yet, cut from one draft by hand.
const SEED: &str = include_str!("../assets/glyph-seed.json");

/// Fewer banners than this came back legible and there is no draft on the screen, only
/// scenery that happens to hold some bright pixels. This is a question about the screen,
/// so it is asked of what was read, never of how many of those names are on record.
const LEAST_SEATS: usize = 3;

/// A screen-read draft is worth showing as soon as it names one player, because a name
/// nobody has a record of has nothing to show anyway. Requiring three *placed* seats
/// meant a lobby of strangers with two acquaintances in it was thrown away whole, which
/// is the case the reader exists for.
const LEAST_PLACED: usize = 1;

/// A banner as it was on the screen, kept until the battlelobby can name it.
struct Shot {
    rgb: Vec<u8>,
    w: usize,
    h: usize,
}

pub struct Reader {
    atlas: Atlas,
    path: PathBuf,
    /// The last draft seen, waiting to be told what it said.
    seen: Vec<Shot>,
    unsaved: bool,
    /// Held open across looks: the display is asked every couple of seconds for as long
    /// as the program runs.
    screen: Option<w2b_shot::Screen>,
    /// Consecutive looks that found the window and copied nothing off it.
    blank_grabs: u32,
    /// Banners the last harvest knew the name of and still could not file.
    unfiled: Vec<(Vec<u8>, String)>,
    /// What the reader last reported, so a steady state is said once rather than every
    /// couple of seconds.
    said: Option<&'static str>,
}

/// Where a banner sits on the desktop, or `None` if that is off the edge of it.
fn on_screen(rect: &w2b_shot::Rect, x: usize, y: usize) -> Option<(i16, i16)> {
    let sx = i32::from(rect.x) + i32::try_from(x).ok()?;
    let sy = i32::from(rect.y) + i32::try_from(y).ok()?;
    let fits = |v: i32| (i32::from(i16::MIN)..=i32::from(i16::MAX)).contains(&v);
    (fits(sx) && fits(sy)).then_some((sx as i16, sy as i16))
}

/// Grumbling on the first blank grab would fire once for every alt-tab.
const BLANK_GRABS_BEFORE_COMPLAINT: u32 = 5;

/// A banner as a PNG, which is how it travels to a server. `None` when it will not
/// encode, which costs one banner and nothing else.
fn as_png(shot: &Shot) -> Option<Vec<u8>> {
    let buffer: image::RgbImage =
        image::ImageBuffer::from_raw(shot.w as u32, shot.h as u32, shot.rgb.clone())?;
    let mut png = std::io::Cursor::new(Vec::new());
    buffer
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()
        .map(|()| png.into_inner())
}

/// Players by the name on their banner, which is the battletag without its number.
fn candidates(tags: &[String]) -> Vec<(String, String)> {
    tags.iter()
        .map(|tag| {
            let name = tag.split_once('#').map_or(tag.as_str(), |(n, _)| n);
            (name.to_string(), tag.clone())
        })
        .collect()
}

/// The reading to believe out of the ones the ladder produced.
///
/// A rung that names somebody wins, and the closest such naming wins outright: the score
/// is what decides a seat later anyway, so choosing by it here cannot seat anyone that
/// `identify` would have turned away. When no rung names anybody the fullest read is
/// kept, so a banner that a stranger is standing at still counts towards there being a
/// draft on the screen at all.
fn best_read(ladder: Vec<Reading>, candidates: &[(String, String)]) -> Option<Reading> {
    let mut best: Option<(f32, Reading)> = None;
    let mut fallback: Option<Reading> = None;

    for reading in ladder {
        match name::rank(&reading.text, candidates).map(|(found, _)| found.score) {
            Some(score) if best.as_ref().is_none_or(|(had, _)| score < *had) => {
                best = Some((score, reading));
            }
            Some(_) => {}
            None => {
                if fallback.as_ref().is_none_or(|had| reading.unread < had.unread) {
                    fallback = Some(reading);
                }
            }
        }
    }

    // Which rung answered, and how well, is the whole story when a draft is on the screen
    // and no seat is being placed: too dim to read, or read and nobody it could be.
    if let Some((score, reading)) = best {
        tracing::debug!(text = %reading.text, threshold = reading.threshold, score, "banner");
        return Some(reading);
    }
    fallback
}

impl Reader {
    /// The atlas this machine has built, or the one shipped with the program if it has
    /// not built one yet.
    pub fn open(dir: &Path) -> Reader {
        let path = dir.join("glyphs.json");
        // The shipped shapes are folded in on every launch rather than used only when
        // this machine has learned nothing. As a fallback they reached exactly the users
        // who needed them least: anyone whose reader had ever worked kept their own file
        // and never saw a seed that somebody else's learning had improved.
        let mut atlas = Atlas::load(&path).unwrap_or_default();
        if let Ok(seed) = serde_json::from_str::<Atlas>(SEED) {
            atlas.absorb(&seed);
        }
        Reader {
            atlas,
            path,
            seen: Vec::new(),
            unsaved: false,
            screen: w2b_shot::screens().ok().and_then(|s| s.into_iter().next()),
            blank_grabs: 0,
            unfiled: Vec::new(),
            said: None,
        }
    }

    /// Say what the reader is doing, but only when that changes. Every one of these was
    /// silent before, which is how a reader that never read a thing looked exactly like
    /// a reader that was working and had simply found no draft.
    pub fn say(&mut self, what: &'static str) {
        if self.said != Some(what) {
            self.said = Some(what);
            tracing::info!(state = what, "draft reader");
        }
    }

    /// Whether there is anything to read with. A machine under Wayland, or with no
    /// display at all, has not, and the client works from the battlelobby alone.
    pub fn can_look(&self) -> bool {
        self.screen.is_some()
    }

    pub fn letters_known(&self) -> usize {
        self.atlas.letters()
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// Fold in shapes from elsewhere - the pool on a shared server, most likely. What is
    /// gained is written out, so the next launch keeps it even offline.
    pub fn absorb(&mut self, other: &Atlas) -> usize {
        let before = self.atlas.examples();
        self.atlas.absorb(other);
        let gained = self.atlas.examples() - before;
        if gained > 0 {
            self.unsaved = true;
        }
        gained
    }

    /// Banners the last harvest could not file, as PNGs, with what they turned out to
    /// say. A banner is only unfiled because this atlas could not cut it into the right
    /// number of letters; a pool with more letters in it may well manage.
    pub fn take_unfiled(&mut self) -> Vec<(Vec<u8>, String)> {
        std::mem::take(&mut self.unfiled)
    }

    /// Grab the game's window and read whatever banners are on it. `None` when there is
    /// no window, no draft, or nothing legible.
    /// `pool` is every player on record. A banner is read at each rung of the brightness
    /// ladder and the rung that names somebody most convincingly is kept: which rung that
    /// is depends on whether the client has the seat lit, and that changes seat by seat
    /// as picks lock in.
    pub fn look(&mut self, pool: &[String]) -> Option<Vec<(geometry::Seat, String)>> {
        if self.screen.is_none() {
            self.say("no screen to read");
            return None;
        }
        let candidates = candidates(pool);
        let Some(rect) = w2b_shot::find_window(GAME_WINDOW).ok().flatten() else {
            self.say("no game window");
            return None;
        };
        let screen = self.screen.as_ref()?;

        let mut shots = Vec::new();
        let mut reads = Vec::new();
        let mut drawn = false;
        // Only the ten banners are copied, not the screen they sit on. Taking the whole
        // window and cropping it afterwards moved twenty-four megabytes to keep two, and
        // did it every couple of seconds for as long as the program was open.
        for (seat, (x, y, w, h)) in geometry::banners(usize::from(rect.w), usize::from(rect.h)) {
            let Some((sx, sy)) = on_screen(&rect, x, y) else {
                continue;
            };
            let Ok(cut) = screen.grab_region(sx, sy, w as u16, h as u16) else {
                continue;
            };
            // A grab of one flat colour is a dropped frame. Reading it would report an
            // empty draft rather than no draft.
            if !cut.looks_drawn() {
                continue;
            }
            drawn = true;
            let ladder = w2b_glyph::read_ladder(&cut.rgb, cut.w, cut.h, &self.atlas);
            let Some(reading) = best_read(ladder, &candidates) else {
                continue;
            };
            reads.push((seat, reading.text));
            shots.push(Shot {
                rgb: cut.rgb,
                w: cut.w,
                h: cut.h,
            });
        }

        if !drawn {
            // The window is there and every grab came back blank, which is worth saying
            // once: it is what an exclusive fullscreen client looks like from out here.
            self.blank_grabs += 1;
            if self.blank_grabs == BLANK_GRABS_BEFORE_COMPLAINT {
                tracing::warn!(
                    "the game window will not copy; if it is running exclusive \
                     fullscreen, borderless windowed can be read and it cannot"
                );
            }
            self.say("the game window will not copy");
            return None;
        }
        self.blank_grabs = 0;

        if reads.len() < LEAST_SEATS {
            self.say("no draft on the screen");
            return None;
        }
        self.say("reading a draft");
        self.seen = shots;
        Some(reads)
    }

    /// The lobby those reads describe, holding only the seats that named somebody this
    /// database already knows. A seat that cannot be placed is left out rather than
    /// filled with a guess, so a half-read draft shows half a draft: an invented
    /// battletag would take notes and verdicts against a player who does not exist.
    pub fn lobby(reads: &[(geometry::Seat, String)], pool: &[String]) -> Option<Lobby> {
        let candidates = candidates(pool);

        let mut players = Vec::new();
        for (seat, text) in reads {
            let Some(found) = name::identify(text, &candidates) else {
                continue;
            };
            // The same player cannot hold two seats; a repeat means one read is wrong.
            if players
                .iter()
                .any(|p: &LobbyPlayer| p.battletag == found.battletag)
            {
                continue;
            }
            let team = u8::from(seat.right_hand);
            players.push(LobbyPlayer {
                battletag: found.battletag,
                team,
                slot: seat.row + team * 5,
            });
        }

        (players.len() >= LEAST_PLACED).then_some(Lobby {
            players,
            // The screen does not say, and nothing downstream of a local draft asks.
            region: 0,
        })
    }

    /// Mark the reader's homework. The battlelobby names the ten seats, so every banner
    /// still in hand can be filed under what it actually said.
    ///
    /// Each banner is read again against the ten names alone, which is a small enough
    /// field to be nearly certain about, and only a confident and unrepeated answer is
    /// learned from. A shape filed under the wrong letter is never unlearned.
    pub fn harvest(&mut self, truth: &[String]) -> usize {
        if self.seen.is_empty() || truth.is_empty() {
            return 0;
        }
        let candidates = candidates(truth);

        let mut taken: Vec<String> = Vec::new();
        let mut learned = 0;
        // Taken in one pass and never revisited: the banners are gone after this.
        let seen = std::mem::take(&mut self.seen);
        for shot in &seen {
            let ladder = w2b_glyph::read_ladder(&shot.rgb, shot.w, shot.h, &self.atlas);
            let Some(reading) = best_read(ladder, &candidates) else {
                continue;
            };
            let Some(found) = name::identify(&reading.text, &candidates) else {
                continue;
            };
            if taken.contains(&found.battletag) {
                continue;
            }
            let name = found
                .battletag
                .split_once('#')
                .map_or(found.battletag.as_str(), |(n, _)| n);
            if w2b_glyph::learn(&shot.rgb, shot.w, shot.h, name, &mut self.atlas) {
                taken.push(found.battletag.clone());
                learned += 1;
            } else if let Some(png) = as_png(shot) {
                // Known to be this player, and still unfiled: the shapes did not come to
                // the same count as the name. Kept as a picture so a fuller atlas than
                // this one can try.
                taken.push(found.battletag.clone());
                self.unfiled.push((png, name.to_string()));
            }
        }
        if learned > 0 {
            self.unsaved = true;
        }
        learned
    }

    /// Written only when there is something new in it.
    pub fn save(&mut self) -> std::io::Result<()> {
        if !self.unsaved {
            return Ok(());
        }
        self.atlas.save(&self.path)?;
        self.unsaved = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seat(right_hand: bool, row: u8) -> geometry::Seat {
        geometry::Seat { right_hand, row }
    }

    fn pool() -> Vec<String> {
        ["geemelodie#1711", "SageLion#115872", "eumesmo#1338", "Caive#1258"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn a_read_that_is_nearly_right_seats_the_player_it_meant() {
        let reads = vec![
            (seat(false, 0), "geemelodle".to_string()),
            (seat(true, 1), "SageLion".to_string()),
            (seat(true, 2), "eumesmo".to_string()),
        ];
        let lobby = Reader::lobby(&reads, &pool()).expect("three seats is a draft");
        assert_eq!(lobby.players.len(), 3);
        let mine = &lobby.players[0];
        assert_eq!(mine.battletag, "geemelodie#1711");
        assert_eq!(mine.team, 0);
        // The two panels are the two teams, and the slots run on from one to the other.
        let theirs: Vec<u8> = lobby.players[1..].iter().map(|p| p.slot).collect();
        assert_eq!(theirs, vec![6, 7]);
        assert!(lobby.players[1..].iter().all(|p| p.team == 1));
    }

    #[test]
    fn seats_that_cannot_be_placed_are_left_empty() {
        let reads = vec![
            (seat(false, 0), "geemelodie".to_string()),
            (seat(false, 1), "SageLion".to_string()),
            (seat(false, 2), "eumesmo".to_string()),
            (seat(true, 0), "?????".to_string()),
            (seat(true, 1), "xqzvw".to_string()),
        ];
        let lobby = Reader::lobby(&reads, &pool()).expect("three good seats stand");
        assert_eq!(lobby.players.len(), 3, "a guess was seated");
    }

    /// Whether there is a draft on the screen at all is settled in `look`, which wants
    /// `LEAST_SEATS` legible banners before it asks this. What is left here is only who
    /// could be placed, and one acquaintance among nine strangers is the case the reader
    /// exists for: requiring three placed seats threw that whole lobby away.
    #[test]
    fn one_player_on_a_screen_is_still_worth_showing() {
        let reads = vec![(seat(false, 0), "geemelodie".to_string())];
        let lobby = Reader::lobby(&reads, &pool()).expect("a named seat is worth showing");
        assert_eq!(lobby.players.len(), 1);
        assert_eq!(lobby.players[0].battletag, "geemelodie#1711");
    }

    #[test]
    fn a_screen_that_names_nobody_is_not_a_draft() {
        let reads = vec![
            (seat(false, 0), "?????".to_string()),
            (seat(true, 0), "xqzvw".to_string()),
            (seat(true, 1), "qqzzqq".to_string()),
        ];
        assert!(Reader::lobby(&reads, &pool()).is_none());
    }

    #[test]
    fn one_player_cannot_hold_two_seats() {
        // Two banners that read alike must not seat the same person twice.
        let reads = vec![
            (seat(false, 0), "geemelodie".to_string()),
            (seat(false, 1), "geemelodie".to_string()),
            (seat(true, 0), "SageLion".to_string()),
            (seat(true, 1), "eumesmo".to_string()),
        ];
        let lobby = Reader::lobby(&reads, &pool()).unwrap();
        assert_eq!(lobby.players.len(), 3);
    }
}
