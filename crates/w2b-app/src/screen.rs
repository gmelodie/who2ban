//! Reading the draft off the screen, for the minutes before the game writes anything.
//!
//! The battlelobby only lands when the loading screen does, which is after the bans are
//! spent. The names are on the screen throughout the draft, so this reads them there,
//! and when the battlelobby finally arrives it is used to mark the reader's homework:
//! the shapes it saw are filed under the letters they turned out to be.

use std::path::{Path, PathBuf};

use w2b_glyph::{Atlas, geometry, name};
use w2b_parse::{Lobby, LobbyPlayer};

/// The title the client gives its window.
const GAME_WINDOW: &str = "Heroes of the Storm";

/// Shapes the client has not learned for itself yet, cut from one draft by hand.
const SEED: &str = include_str!("../assets/glyph-seed.json");

/// Fewer seats than this and there is no draft on the screen, only scenery that happens
/// to hold some bright pixels.
const LEAST_SEATS: usize = 3;

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
}

impl Reader {
    /// The atlas this machine has built, or the one shipped with the program if it has
    /// not built one yet.
    pub fn open(dir: &Path) -> Reader {
        let path = dir.join("glyphs.json");
        let atlas = Atlas::load(&path)
            .ok()
            .or_else(|| serde_json::from_str(SEED).ok())
            .unwrap_or_default();
        Reader {
            atlas,
            path,
            seen: Vec::new(),
            unsaved: false,
            screen: w2b_shot::screens().ok().and_then(|s| s.into_iter().next()),
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

    /// Grab the game's window and read whatever banners are on it. `None` when there is
    /// no window, no draft, or nothing legible.
    pub fn look(&mut self) -> Option<Vec<(geometry::Seat, String)>> {
        let screen = self.screen.as_ref()?;
        let rect = w2b_shot::find_window(GAME_WINDOW).ok().flatten()?;
        let frame = screen.grab_region(rect.x, rect.y, rect.w, rect.h).ok()?;
        // A grab of one flat colour is a dropped frame, and reading it would report an
        // empty draft rather than no draft.
        if !frame.looks_drawn() {
            return None;
        }

        let mut shots = Vec::new();
        let mut reads = Vec::new();
        for (seat, (x, y, w, h)) in geometry::banners(frame.w, frame.h) {
            let Some(cut) = frame.crop(x, y, w, h) else {
                continue;
            };
            let Some(reading) = w2b_glyph::read(&cut.rgb, cut.w, cut.h, &self.atlas) else {
                continue;
            };
            if reading.is_empty() {
                continue;
            }
            reads.push((seat, reading.text));
            shots.push(Shot {
                rgb: cut.rgb,
                w: cut.w,
                h: cut.h,
            });
        }

        if reads.len() < LEAST_SEATS {
            return None;
        }
        self.seen = shots;
        Some(reads)
    }

    /// The lobby those reads describe, holding only the seats that named somebody this
    /// database already knows. A seat that cannot be placed is left out rather than
    /// filled with a guess, so a half-read draft shows half a draft.
    pub fn lobby(reads: &[(geometry::Seat, String)], pool: &[String]) -> Option<Lobby> {
        let candidates: Vec<(String, String)> = pool
            .iter()
            .map(|tag| {
                let name = tag.split_once('#').map_or(tag.as_str(), |(n, _)| n);
                (name.to_string(), tag.clone())
            })
            .collect();

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

        (players.len() >= LEAST_SEATS).then_some(Lobby {
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
        let candidates: Vec<(String, String)> = truth
            .iter()
            .map(|tag| {
                let name = tag.split_once('#').map_or(tag.as_str(), |(n, _)| n);
                (name.to_string(), tag.clone())
            })
            .collect();

        let mut taken: Vec<String> = Vec::new();
        let mut learned = 0;
        // Taken in one pass and never revisited: the banners are gone after this.
        let seen = std::mem::take(&mut self.seen);
        for shot in &seen {
            let Some(reading) = w2b_glyph::read(&shot.rgb, shot.w, shot.h, &self.atlas) else {
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

    #[test]
    fn a_screen_with_almost_nothing_on_it_is_not_a_draft() {
        let reads = vec![(seat(false, 0), "geemelodie".to_string())];
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
