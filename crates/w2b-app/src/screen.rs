//! Reading the draft off the screen, for the minutes before the game writes anything.
//!
//! The battlelobby only lands when the loading screen does, which is after the bans are
//! spent. The names are on the screen throughout the draft, so this reads them there,
//! and when the battlelobby finally arrives it is used to mark the reader's homework:
//! the shapes it saw are filed under the letters they turned out to be.

use std::path::{Path, PathBuf};

use w2b_glyph::{Atlas, Reading, geometry, name};

use crate::chrome;
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
    /// The last draft seen, waiting to be told what it said, each banner under the slot
    /// it was cut from so a correction aimed at one card reaches the right picture.
    seen: Vec<(u8, Shot)>,
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
    /// Whether the last look found the client sitting on one of its own menus. A look
    /// gives nothing for several reasons and only this one means the match is over.
    menu: bool,
}

/// The slot a seat holds in a lobby, which is how everything downstream of the screen
/// names it. The left panel is slots 0 to 4 and the right panel 5 to 9.
fn slot_of(seat: &geometry::Seat) -> u8 {
    seat.row + u8::from(seat.right_hand) * 5
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

/// How the banners that could be read line up with the slots the battlelobby gave them:
/// how many named the player the file seats there, and how many named somebody else.
///
/// The second number is the one that matters. A single clash says the file's slot order
/// and the screen's are not the same order, and every name filed by slot after that would
/// go on the wrong picture - and then on to the shared pool, where nobody can take it
/// back. Banners that read as nothing count as neither: they are the ones with nothing to
/// say about the ordering, and the ones the filing is for.
fn agreement(slots: &[u8], read: &[Option<String>], truth: &[String]) -> (usize, usize) {
    let mut agreed = 0;
    let mut clashed = 0;
    for (slot, read) in slots.iter().zip(read) {
        let (Some(read), Some(seat)) = (read, truth.get(usize::from(*slot))) else {
            continue;
        };
        if read == seat {
            agreed += 1;
        } else {
            clashed += 1;
        }
    }
    (agreed, clashed)
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
            menu: false,
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

    /// Whether the last look found a menu, which is the one way of finding no draft that
    /// means the match in front of this program has finished.
    pub fn saw_menu(&self) -> bool {
        self.menu
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

    /// File the banner at this slot under a name the user has vouched for.
    ///
    /// The same filing `harvest` does, with the person at the keyboard standing in for
    /// the battlelobby. It is the better teacher of the two: it arrives while the draft
    /// is still on the screen, it needs no read to have half worked first, and it can
    /// name the letters this atlas has never seen, which are exactly the ones stopping it
    /// from reading that banner in the first place.
    ///
    /// `false` when there is no banner for that slot, or when the shapes on it do not
    /// come to the same count as the name, which teaches nothing rather than teaching a
    /// letter the wrong shape.
    pub fn teach(&mut self, slot: u8, name: &str) -> bool {
        let Some((_, shot)) = self.seen.iter().find(|(at, _)| *at == slot) else {
            return false;
        };
        let rgb = shot.rgb.clone();
        let (w, h) = (shot.w, shot.h);
        let filed = w2b_glyph::learn(&rgb, w, h, name, &mut self.atlas);
        if filed {
            self.unsaved = true;
        }
        filed
    }

    /// Drop the banners in hand without filing them. What was read while the battlelobby
    /// stands is the HUD, not a draft, and a shape learned from it would be filed under a
    /// letter it never was.
    pub fn forget(&mut self) {
        self.seen.clear();
    }

    /// Banners the last harvest could not file, as PNGs, with what they turned out to
    /// say. A banner is only unfiled because this atlas could not cut it into the right
    /// number of letters; a pool with more letters in it may well manage.
    pub fn take_unfiled(&mut self) -> Vec<(Vec<u8>, String)> {
        std::mem::take(&mut self.unfiled)
    }

    /// Whether the client is showing one of its own menus rather than a draft, which it
    /// says by drawing the Nexus button in its top-left corner.
    ///
    /// Every way of failing to look answers no. A corner that cannot be grabbed is not
    /// evidence of a menu, and the reader should be no worse off than it was before this
    /// was asked at all.
    fn on_a_menu(&self, rect: &w2b_shot::Rect) -> bool {
        let Some(screen) = self.screen.as_ref() else {
            return false;
        };
        let Some((x, y, w, h)) = chrome::badge_box(usize::from(rect.w), usize::from(rect.h))
        else {
            return false;
        };
        let Some((sx, sy)) = on_screen(rect, x, y) else {
            return false;
        };
        let Ok(cut) = screen.grab_region(sx, sy, w as u16, h as u16) else {
            return false;
        };
        let Some(alike) = chrome::badge_likeness(&cut.rgb, cut.w, cut.h) else {
            return false;
        };
        tracing::debug!(alike, threshold = chrome::ALIKE, "menu badge");
        alike >= chrome::ALIKE
    }

    /// Grab the game's window and read whatever banners are on it. `None` when there is
    /// no window, no draft, or nothing legible.
    /// `pool` is every player on record. A banner is read at each rung of the brightness
    /// ladder and the rung that names somebody most convincingly is kept: which rung that
    /// is depends on whether the client has the seat lit, and that changes seat by seat
    /// as picks lock in.
    pub fn look(&mut self, pool: &[String]) -> Option<Vec<(geometry::Seat, String)>> {
        self.menu = false;
        if self.screen.is_none() {
            self.say("no screen to read");
            return None;
        }
        let candidates = candidates(pool);
        let Some(rect) = w2b_shot::find_window(GAME_WINDOW).ok().flatten() else {
            self.say("no game window");
            return None;
        };
        // One small grab, before the ten, to find out whether there is a draft to read at
        // all. Without it a menu's scenery reads as three seats and gets published as a
        // lobby the user is nowhere near.
        self.menu = self.on_a_menu(&rect);
        if self.menu {
            self.say("the client is on a menu, not in a draft");
            return None;
        }
        let screen = self.screen.as_ref()?;

        let mut shots: Vec<(u8, Shot)> = Vec::new();
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
            let read = best_read(ladder, &candidates);
            // Kept whether or not it read. A banner this atlas cannot make a name of is
            // the one banner worth keeping: its letters are the ones the atlas is
            // missing, and the battlelobby is about to say what they are. Dropping it
            // here is what held the reader to the letters it already had, because every
            // banner it ever learned from was one it could already read.
            shots.push((
                slot_of(&seat),
                Shot {
                    rgb: cut.rgb,
                    w: cut.w,
                    h: cut.h,
                },
            ));
            if let Some(reading) = read {
                reads.push((seat, reading.text));
            }
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
            players.push(LobbyPlayer {
                battletag: found.battletag,
                team: u8::from(seat.right_hand),
                slot: slot_of(seat),
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
    /// The names come from the file by slot, not from reading the banner. This used to
    /// read each banner again and file it under whoever that named, which meant a banner
    /// only ever taught the atlas letters it could already spell out - and the ones it
    /// could not were the entire point. A reader that cannot read `Trollmllaman` learns
    /// nothing from `Trollmllaman` until something else tells it that is what the banner
    /// says, and at load the file does exactly that.
    ///
    /// A shape filed under the wrong letter is never unlearned, and what is learned here
    /// goes on to the shared pool, so `by_slot` below refuses the whole lobby rather than
    /// file one banner on an assumption a read has contradicted.
    pub fn harvest(&mut self, truth: &[String]) -> usize {
        if self.seen.is_empty() || truth.is_empty() {
            return 0;
        }
        let candidates = candidates(truth);
        // Taken in one pass and never revisited: the banners are gone after this.
        let seen = std::mem::take(&mut self.seen);

        // The banners are still read, but no longer to find out who they are - the file
        // says that. They are read to check the one thing this rests on: that the file
        // seats its ten in the same slots the screen does.
        let read: Vec<Option<String>> = seen
            .iter()
            .map(|(_, shot)| self.read_name(shot, &candidates))
            .collect();
        let slots: Vec<u8> = seen.iter().map(|(slot, _)| *slot).collect();
        let (agreed, clashed) = agreement(&slots, &read, truth);
        let by_slot = clashed == 0;
        if by_slot {
            tracing::debug!(agreed, banners = seen.len(), "banners named by the lobby");
        } else {
            tracing::warn!(
                agreed,
                clashed,
                "the battlelobby seats its players in a different order than the screen \
                 does, so the banners are filed only under what they read as"
            );
        }

        let mut taken: Vec<String> = Vec::new();
        let mut learned = 0;
        for ((slot, shot), read) in seen.iter().zip(read) {
            // The file when its slots can be trusted, and otherwise what the banner made
            // of itself. A banner that read as nothing has no second answer.
            let named = match by_slot {
                true => truth.get(usize::from(*slot)).cloned().or(read),
                false => read,
            };
            let Some(battletag) = named else {
                continue;
            };
            if taken.contains(&battletag) {
                continue;
            }
            taken.push(battletag.clone());
            let name = battletag
                .split_once('#')
                .map_or(battletag.as_str(), |(n, _)| n);
            if w2b_glyph::learn(&shot.rgb, shot.w, shot.h, name, &mut self.atlas) {
                learned += 1;
            } else if let Some(png) = as_png(shot) {
                // Known to be this player, and still unfiled: the shapes did not come to
                // the same count as the name. Kept as a picture so a fuller atlas than
                // this one can try.
                self.unfiled.push((png, name.to_string()));
            }
        }
        if learned > 0 {
            self.unsaved = true;
        }
        learned
    }

    /// Who a banner reads as, or `None` when this atlas can make no name of it. Only
    /// `harvest` asks, and only to check the file's slots against the screen's.
    fn read_name(&self, shot: &Shot, candidates: &[(String, String)]) -> Option<String> {
        let ladder = w2b_glyph::read_ladder(&shot.rgb, shot.w, shot.h, &self.atlas);
        let reading = best_read(ladder, candidates)?;
        name::identify(&reading.text, candidates).map(|found| found.battletag)
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

    fn truth() -> Vec<String> {
        pool().into_iter().take(3).collect()
    }

    fn reads(names: [Option<&str>; 3]) -> Vec<Option<String>> {
        names.iter().map(|n| n.map(str::to_string)).collect()
    }

    /// Every banner that could be read names the player the file seats in that slot, so
    /// the two orders are the same order and the rest may be filed by slot.
    #[test]
    fn reads_that_match_their_slots_leave_the_filing_alone() {
        let (agreed, clashed) = agreement(
            &[0, 1, 2],
            &reads([Some("geemelodie#1711"), None, Some("eumesmo#1338")]),
            &truth(),
        );
        assert_eq!(clashed, 0);
        assert_eq!(agreed, 2);
    }

    /// Nothing could be read at all, which is the state a fresh atlas is in. There is no
    /// evidence against the slots, so the file gets to name all ten - that is the whole
    /// point of harvesting at load.
    #[test]
    fn banners_that_read_as_nothing_do_not_veto_the_slots() {
        let (agreed, clashed) = agreement(&[0, 1, 2], &reads([None, None, None]), &truth());
        assert_eq!((agreed, clashed), (0, 0));
    }

    /// One banner names somebody the file seats elsewhere. That is enough: filing by slot
    /// would teach the wrong shapes and then hand them to the shared pool.
    #[test]
    fn one_read_in_the_wrong_seat_vetoes_the_lot() {
        let (_, clashed) = agreement(
            &[0, 1, 2],
            &reads([Some("SageLion#115872"), None, Some("eumesmo#1338")]),
            &truth(),
        );
        assert!(clashed > 0);
    }

    /// A screen slot the file has no seat for says nothing either way, rather than
    /// counting as a clash and throwing the lobby away.
    #[test]
    fn a_slot_the_file_never_named_is_not_a_clash() {
        let (agreed, clashed) = agreement(&[9], &reads([Some("Caive#1258"), None, None]), &truth());
        assert_eq!((agreed, clashed), (0, 0));
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
