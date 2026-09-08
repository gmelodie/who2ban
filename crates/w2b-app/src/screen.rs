//! Reading the draft off the screen, for the minutes before the game writes anything.
//!
//! The battlelobby only lands when the loading screen does, which is after the bans are
//! spent. The names are on the screen throughout the draft, so this reads them there,
//! and when the battlelobby finally arrives it is used to mark the reader's homework:
//! the shapes it saw are filed under the letters they turned out to be.

use std::path::{Path, PathBuf};

use w2b_glyph::{Atlas, Reading, geometry, name};

use crate::aliases::Aliases;
use crate::chrome;
use w2b_parse::{Lobby, LobbyPlayer};

/// The title the client gives its window.
const GAME_WINDOW: &str = "Heroes of the Storm";

/// Shapes the client has not learned for itself yet, cut from one draft by hand.
const SEED: &str = include_str!("../assets/glyph-seed.json");

/// Fewer banners than this have been read off the screen and there is no draft on it,
/// only scenery that happens to hold some bright pixels. This is a question about the
/// screen, so it is asked of what was read, never of how many of those names are on
/// record.
///
/// Asked of the banners in hand rather than of the ones this one look managed. Per look
/// it was a quorum three banners had to reach simultaneously, and banners do not become
/// legible together: they light as their seats fill, so a draft spent its first half
/// minute one banner short and showed nothing whatever, including the seats it had been
/// reading cleanly the whole time. The menu badge is what keeps scenery out - it is
/// checked every look, and finding a menu drops the banners in hand.
const LEAST_SEATS: usize = 3;

/// A screen-read draft is worth showing as soon as it names one player, because a name
/// nobody has a record of has nothing to show anyway. Requiring three *placed* seats
/// meant a lobby of strangers with two acquaintances in it was thrown away whole, which
/// is the case the reader exists for.
const LEAST_PLACED: usize = 1;

/// How legible a banner was in the frame it was cut from: letters the atlas could place,
/// and failing that the number of shapes it cut into at all.
///
/// Only ever compared between frames of the same banner, to keep the better one. The
/// letters come first because a banner that reads is plainly clearer than one that does
/// not; the shape count breaks the tie for the banners whose letters this atlas has never
/// seen, which place nothing in any frame and are the ones worth learning from.
type Legibility = (usize, usize);

/// A banner as it was on the screen, kept until the battlelobby can name it.
struct Shot {
    rgb: Vec<u8>,
    w: usize,
    h: usize,
    legibility: Legibility,
}

pub struct Reader {
    atlas: Atlas,
    path: PathBuf,
    /// The names the screen writes instead of battletags, for the Real ID friends whose
    /// banners say one thing and whose battlelobby entry says another.
    aliases: Aliases,
    /// The last draft seen, waiting to be told what it said, each banner under the slot
    /// it was cut from so a correction aimed at one card reaches the right picture.
    seen: Vec<(u8, Shot)>,
    /// What each slot's banner last said, under the slot it was cut from, with how
    /// legible the frame it was read from was.
    ///
    /// Held across looks for the same reason `seen` is: a banner that read once has
    /// named its seat, and the next look finding it dimmed, covered by a portrait or
    /// simply missed is not that seat emptying. Kept per slot, the first cards appear as
    /// their banners become legible; dropped every look, nothing appeared at all until
    /// three banners happened to be legible within the same couple of seconds, which on
    /// a draft of mostly unknown letters is most of a minute after the screen came up.
    held: Vec<(u8, geometry::Seat, String, Legibility)>,
    unsaved: bool,
    /// Held open across looks: the display is asked every couple of seconds for as long
    /// as the program runs.
    screen: Option<w2b_shot::Screen>,
    /// Consecutive looks that found the window and copied nothing off it.
    blank_grabs: u32,
    /// Full frames still owed to `FRAMES_DIR`, counted down a look at a time.
    wanted_frames: u32,
    /// Banners the last harvest knew the name of and still could not file.
    unfiled: Vec<(Vec<u8>, String)>,
    /// What the reader last reported, so a steady state is said once rather than every
    /// couple of seconds.
    said: Option<&'static str>,
    /// Names read cleanly off a banner that several players in the pool answer to, said
    /// once each rather than every couple of seconds for the whole draft.
    shared_said: std::collections::HashSet<String>,
    /// Whether the last look found the client sitting on one of its own menus. A look
    /// gives nothing for several reasons and only this one means the match is over.
    menu: bool,
}

/// The slot a seat holds in a lobby, which is how everything downstream of the screen
/// names it. The left panel is slots 0 to 4 and the right panel 5 to 9.
fn slot_of(seat: &geometry::Seat) -> u8 {
    seat.row + u8::from(seat.right_hand) * 5
}

/// How much of a banner a reading got hold of, for choosing between frames of it.
fn legibility_of(reading: &Reading) -> Legibility {
    let shapes = reading.text.chars().count();
    (shapes - reading.unread.min(shapes), shapes)
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

/// Whether an environment variable spelled a switch means off. Shared by the two things
/// here that write pictures to disk, so they are turned off the same way.
fn off(v: &str) -> bool {
    matches!(
        v.to_ascii_lowercase().as_str(),
        "" | "0" | "no" | "off" | "false"
    )
}

/// Where full frames of the loading screen are kept, and how many.
///
/// The picks are shown together on the loading screen and nowhere else this program can
/// reach: the battlelobby names the ten players and says nothing about what any of them
/// chose. Whole frames rather than crops, because what these are for is measuring where
/// on the screen the portraits sit, and a crop taken by boxes that have not been worked
/// out yet cannot show that. They are large, so few are kept.
const FRAMES_DIR: &str = "frames";
const MOST_FRAMES: usize = 6;

/// Frames to keep when a battlelobby arrives. The file is written as the loading screen
/// comes up, and one grab at that instant can still catch the draft sliding off; spread
/// over a few looks, one of them lands on the screen wanted.
const FRAMES_PER_LOBBY: u32 = 3;

/// Unfiled banners kept on disk, which is a few drafts' worth. They are only worth
/// having while the reader is being fixed, and the newest are the ones that say what it
/// is doing now, so the oldest go when the folder gets past this.
const MOST_KEPT: usize = 40;

/// Drop the oldest files until only `most` are left. Every failure is ignored: this is
/// housekeeping for a debugging aid, and a folder that will not tidy itself is not a
/// reason to stop reading the draft.
fn prune(dir: &Path, most: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let at = e.path();
            let when = e.metadata().ok()?.modified().ok()?;
            at.is_file().then_some((when, at))
        })
        .collect();
    if files.len() <= most {
        return;
    }
    files.sort_by_key(|(when, _)| *when);
    for (_, at) in &files[..files.len() - most] {
        let _ = std::fs::remove_file(at);
    }
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

/// Whether a read is at least as much a hero's name as it is the player it placed.
///
/// Once a seat has a hero on it the card is retitled: the hero's name goes on top in
/// capitals, the battletag drops to a small line beneath, and the banner box holds both.
/// The big line is the one that reads, and a pool of thousands holds players called
/// Alarak, Cassia and Murky, so a seat on Alarak was handed `Alarak#11471`'s card. A tie
/// goes to the hero: a player who really is called that cannot be told from the title by
/// its letters, and a blank seat is better than a stranger's card.
fn names_a_hero(reading: &str, score: f32) -> bool {
    let heroes: Vec<(String, String)> = crate::heroes::names()
        .map(|hero| (hero.to_string(), hero.to_string()))
        .collect();
    name::rank(reading, &heroes).is_some_and(|(hero, _)| hero.score <= score)
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
                if fallback
                    .as_ref()
                    .is_none_or(|had| reading.unread < had.unread)
                {
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
            aliases: Aliases::load(&dir.join("aliases.json")),
            path,
            seen: Vec::new(),
            held: Vec::new(),
            unsaved: false,
            screen: w2b_shot::screens().ok().and_then(|s| s.into_iter().next()),
            blank_grabs: 0,
            wanted_frames: 0,
            unfiled: Vec::new(),
            said: None,
            shared_said: std::collections::HashSet::new(),
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

    /// Why a seat that read perfectly well is still empty.
    ///
    /// A blank card usually means the banner could not be read, or that it named nobody
    /// on record, and neither is worth a word. This is the third case, and the only one
    /// the person watching can do anything about: the letters were certain and several
    /// players answer to them, so the reader cannot say which and they can. Said once per
    /// name per draft, because the look comes round every couple of seconds.
    fn blame(&mut self, reading: &str, candidates: &[(String, String)]) {
        let Some((found, _)) = name::rank(reading, candidates) else {
            return;
        };
        if found.shared < 2
            || !name::legible(reading)
            || found.score > name::MAX_SCORE
            || names_a_hero(reading, found.score)
        {
            return;
        }
        let name = found
            .battletag
            .split_once('#')
            .map_or(found.battletag.as_str(), |(n, _)| n)
            .to_string();
        if self.shared_said.insert(name.clone()) {
            tracing::info!(
                %name,
                players = found.shared,
                "read this banner, but several players go by that name: name the seat to settle it"
            );
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

    pub fn aliases_known(&self) -> usize {
        self.aliases.len()
    }

    /// Who a read is allowed to conclude a banner is: everyone in the pool by the name
    /// on their banner, and then the Real ID friends whose banner says something else
    /// entirely. Only aliases for players who are actually here, so a friend sitting out
    /// this draft cannot take a seat with a name that half resembles theirs.
    fn candidates(&self, tags: &[String]) -> Vec<(String, String)> {
        let mut out = candidates(tags);
        out.extend(
            self.aliases
                .pairs()
                .filter(|(_, tag)| tags.iter().any(|had| had == tag)),
        );
        out
    }

    /// What the atlas makes of a banner when it can place every single letter. `None`
    /// when any came out a hole, because a name with a guess in it is not good enough to
    /// be recorded as what the screen calls somebody.
    fn clean_read(&self, shot: &Shot) -> Option<String> {
        w2b_glyph::read_ladder(&shot.rgb, shot.w, shot.h, &self.atlas)
            .into_iter()
            .filter(|r| r.unread == 0 && name::legible(&r.text))
            .max_by_key(|r| r.text.chars().count())
            .map(|r| r.text)
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
        // Held reads outlive a look on purpose, but not the draft they were read from:
        // kept into the next one they would seat last game's players off no evidence at
        // all, which is the failure the whole of `look` is otherwise built to avoid.
        self.held.clear();
        // The next draft is a different ten players, and a name that was crowded in this
        // one is worth saying again when it turns up in that one.
        self.shared_said.clear();
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
        let Some((x, y, w, h)) = chrome::badge_box(usize::from(rect.w), usize::from(rect.h)) else {
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
        let candidates = self.candidates(pool);
        let Some(rect) = w2b_shot::find_window(GAME_WINDOW).ok().flatten() else {
            self.say("no game window");
            return None;
        };
        // Before the menu check, not after it: a loading screen is not a draft and may
        // well answer yes to the badge, and the loading screen is the one thing these
        // frames are wanted for.
        self.keep_frame(&rect);
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
            let legibility = read.as_ref().map_or((0, 0), legibility_of);
            shots.push((
                slot_of(&seat),
                Shot {
                    rgb: cut.rgb,
                    w: cut.w,
                    h: cut.h,
                    legibility,
                },
            ));
            if let Some(reading) = read {
                reads.push((seat, reading.text, legibility));
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

        self.keep_read(reads);
        if self.held.len() < LEAST_SEATS {
            self.say("no draft on the screen");
            return None;
        }
        self.say("reading a draft");
        self.keep_clearest(shots);
        Some(
            self.held
                .iter()
                .map(|(_, seat, text, _)| (*seat, text.clone()))
                .collect(),
        )
    }

    /// Fold this look's readings into the ones in hand, keeping each slot's clearest.
    ///
    /// The counterpart of `keep_clearest`, which does this for the pictures. A seat only
    /// changes its mind for a frame that read it better than the one it is standing on,
    /// so a banner going murky as its pick locks in cannot talk the seat out of the name
    /// it already gave, and a misread off a dimmer frame cannot overwrite a clean one.
    fn keep_read(&mut self, reads: Vec<(geometry::Seat, String, Legibility)>) {
        for (seat, text, legibility) in reads {
            let slot = slot_of(&seat);
            match self.held.iter_mut().find(|(at, ..)| *at == slot) {
                Some((_, _, _, held)) if *held >= legibility => {}
                Some(held) => *held = (slot, seat, text, legibility),
                None => self.held.push((slot, seat, text, legibility)),
            }
        }
        self.held.sort_by_key(|(slot, ..)| *slot);
    }

    /// Fold this look's banners into the ones in hand, keeping each slot's clearest frame.
    ///
    /// The battlelobby arrives with the loading screen, and by then the banners are the
    /// worst they have been all draft: seats dim as their picks lock in, portraits slide
    /// over them, and the draft is on its way off the screen entirely. Keeping whichever
    /// frame happened to be last handed the one moment that can name a banner the least
    /// legible picture of it, and `learn` wants the count of shapes to come out exactly
    /// right, so a draft with all ten names known at load taught the atlas nothing.
    ///
    /// Held per slot rather than per look for the same reason the seats are: a banner
    /// that read this time and not the next did not empty, it dimmed.
    fn keep_clearest(&mut self, shots: Vec<(u8, Shot)>) {
        for (slot, shot) in shots {
            match self.seen.iter_mut().find(|(at, _)| *at == slot) {
                Some((_, held)) if held.legibility >= shot.legibility => {}
                Some((_, held)) => *held = shot,
                None => self.seen.push((slot, shot)),
            }
        }
        self.seen.sort_by_key(|(slot, _)| *slot);
    }

    /// The lobby those reads describe, holding only the seats that named somebody this
    /// database already knows. A seat that cannot be placed is left out rather than
    /// filled with a guess, so a half-read draft shows half a draft: an invented
    /// battletag would take notes and verdicts against a player who does not exist.
    pub fn lobby(&mut self, reads: &[(geometry::Seat, String)], pool: &[String]) -> Option<Lobby> {
        let candidates = self.candidates(pool);

        let mut players = Vec::new();
        for (seat, text) in reads {
            let Some(found) = name::identify(text, &candidates) else {
                self.blame(text, &candidates);
                continue;
            };
            if names_a_hero(text, found.score) {
                continue;
            }
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
        let candidates = self.candidates(truth);
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
            let from_file = by_slot && truth.get(usize::from(*slot)).is_some();
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
                continue;
            }
            // The screen writes a Real ID friend's real name where their battletag would
            // go, so the letters on this banner are not the letters of the name the file
            // gives this seat, and nothing can be filed under a name that is not what is
            // drawn. But if the atlas can already read what *is* drawn, every letter of
            // it placed, then both names are known at once and the pair is worth keeping.
            // That is the only moment the two are ever seen together: the battlelobby
            // knows the battletag and never the real name, the screen the reverse.
            //
            // Only when the file named this seat. A banner filed under what it read as
            // has nothing to say about anybody's real name.
            if from_file
                && let Some(drawn) = self
                    .clean_read(shot)
                    .filter(|d| !d.eq_ignore_ascii_case(name))
            {
                if self.aliases.record(&drawn, &battletag) {
                    tracing::info!(
                        slot,
                        %drawn,
                        %battletag,
                        "the draft screen calls this player by another name"
                    );
                    if let Err(e) = self.aliases.save() {
                        tracing::warn!(error = %e, "the name would not be saved");
                    }
                }
                // Filed under the letters actually drawn, which is the spelling the
                // shapes are of. Now that the seat has a name the reader can reach,
                // it will be read off the screen next draft rather than sat empty.
                if w2b_glyph::learn(&shot.rgb, shot.w, shot.h, &drawn, &mut self.atlas) {
                    learned += 1;
                    continue;
                }
            }
            // Known to be this player and still unfiled, which is the case that keeps the
            // atlas at the alphabet it already has: the banners it cannot cut are the
            // banners carrying the letters it cannot read. Say what the rungs made of it,
            // because the wanted count against the counts on offer is the whole diagnosis
            // and nothing else in the program ever gets to see it.
            let cuts: Vec<String> = w2b_glyph::shape_counts(&shot.rgb, shot.w, shot.h)
                .iter()
                .map(|(t, n)| format!("{t:.2}:{n}"))
                .collect();
            tracing::info!(
                slot,
                name,
                wanted = name.chars().filter(|c| !c.is_whitespace()).count(),
                cuts = %cuts.join(" "),
                "banner would not cut into its letters"
            );
            self.keep_picture(*slot, name, shot);
            if let Some(png) = as_png(shot) {
                // Kept as a picture so a fuller atlas than this one can try.
                self.unfiled.push((png, name.to_string()));
            }
        }
        if learned > 0 {
            self.unsaved = true;
        }
        learned
    }

    /// Write a banner that could not be filed beside the atlas, under the name it is
    /// known to carry.
    ///
    /// On by default, because these are the one thing missing whenever this goes wrong.
    /// A banner that will not cut up is the only evidence of why, the atlas cannot learn
    /// the letters on it until that is understood, and the program already builds this
    /// exact PNG to hand to a server that can do no more with it than the client just
    /// did. Keeping it costs a write; not keeping it costs the next investigation, which
    /// is a whole draft's wait for a picture nobody thought to save.
    ///
    /// `cargo run -p w2b-glyph --example banners` takes exactly this picture and that
    /// name and says, rung by rung, what the segmenter made of it.
    ///
    /// `W2B_KEEP_BANNERS=0` turns it off. Nothing is unbounded: only banners that could
    /// not be filed are kept, so the folder shrinks as the reader gets better at its job
    /// and empties altogether when it is right, and `MOST_KEPT` holds the broken case.
    fn keep_picture(&self, slot: u8, name: &str, shot: &Shot) {
        if std::env::var("W2B_KEEP_BANNERS").is_ok_and(|v| off(&v)) {
            return;
        }
        let Some(dir) = self.path.parent().map(|d| d.join("unfiled")) else {
            return;
        };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let Some(png) = as_png(shot) else { return };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        // The name is a battletag's first half, so it is whatever a stranger chose to
        // call themselves; anything that is not plainly a filename is spelled out of it.
        let safe: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let at = dir.join(format!("{stamp}-{slot}-{safe}.png"));
        match std::fs::write(&at, png) {
            Ok(()) => tracing::info!(path = %at.display(), "banner kept"),
            Err(e) => return tracing::warn!(error = %e, "banner would not be kept"),
        }
        prune(&dir, MOST_KEPT);
    }

    /// Ask for a run of full frames, one per look, starting with the next one.
    pub fn want_frames(&mut self) {
        self.wanted_frames = self.wanted_frames.max(FRAMES_PER_LOBBY);
    }

    /// Keep one whole frame of the game window, if any are still owed.
    ///
    /// Nobody is going to catch a loading screen with a screenshot key: it is up for half
    /// a minute in the middle of a match, and the person it would be asked of is the one
    /// playing it. So the program collects its own, the way `keep` collects banners, and
    /// for the same reason: the boxes this file is full of were measured off a capture,
    /// and the next set has to be measured off one too.
    ///
    /// `W2B_KEEP_FRAMES=0` turns it off. A frame is several megabytes where a banner is a
    /// few dozen kilobytes, so `MOST_FRAMES` is small and the folder is tidied after each.
    fn keep_frame(&mut self, rect: &w2b_shot::Rect) {
        if self.wanted_frames == 0 || std::env::var("W2B_KEEP_FRAMES").is_ok_and(|v| off(&v)) {
            return;
        }
        // Counted down whether or not this one works out. A window that will not copy
        // will not copy on the next look either, and a run that never ends is a frame
        // written every couple of seconds for the rest of the match.
        self.wanted_frames -= 1;
        let Some(screen) = self.screen.as_ref() else {
            return;
        };
        let Some((sx, sy)) = on_screen(rect, 0, 0) else {
            return;
        };
        let Ok(cut) = screen.grab_region(sx, sy, rect.w, rect.h) else {
            return;
        };
        // The same dropped-frame guard the banners get. A flat grab measures nothing.
        if !cut.looks_drawn() {
            return;
        }
        let Some(dir) = self.path.parent().map(|d| d.join(FRAMES_DIR)) else {
            return;
        };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let shot = Shot {
            rgb: cut.rgb,
            w: cut.w,
            h: cut.h,
            legibility: (0, 0),
        };
        let Some(png) = as_png(&shot) else { return };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        // The size is in the name because it is the first thing anyone measuring off one
        // of these needs to know, and it is what tells two monitors' frames apart.
        let at = dir.join(format!("{stamp}-{}x{}.png", shot.w, shot.h));
        match std::fs::write(&at, png) {
            Ok(()) => tracing::info!(path = %at.display(), "frame kept"),
            Err(e) => return tracing::warn!(error = %e, "frame would not be kept"),
        }
        prune(&dir, MOST_FRAMES);
    }

    /// Who a banner reads as, or `None` when this atlas can make no name of it. Only
    /// `harvest` asks, and only to check the file's slots against the screen's.
    fn read_name(&self, shot: &Shot, candidates: &[(String, String)]) -> Option<String> {
        let ladder = w2b_glyph::read_ladder(&shot.rgb, shot.w, shot.h, &self.atlas);
        let reading = best_read(ladder, candidates)?;
        // A hero's name here would count as a clash with the file's slots, and one clash
        // files the whole lobby under what the banners read as.
        name::identify(&reading.text, candidates)
            .filter(|found| !names_a_hero(&reading.text, found.score))
            .map(|found| found.battletag)
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
        [
            "geemelodie#1711",
            "SageLion#115872",
            "eumesmo#1338",
            "Caive#1258",
        ]
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

    fn shot(legibility: Legibility) -> Shot {
        Shot {
            rgb: vec![0; 3],
            w: 1,
            h: 1,
            legibility,
        }
    }

    /// A reader with nothing on disk and no screen, which is all `keep_clearest` needs.
    fn reader() -> Reader {
        let mut r = Reader::open(Path::new("/nonexistent"));
        r.seen.clear();
        r
    }

    /// Both switches are read the same way, and the way to get one wrong is to have it
    /// mean off when it was set to turn something on.
    #[test]
    fn only_a_switch_spelled_off_means_off() {
        for spelling in ["0", "no", "off", "false", "OFF", "False", ""] {
            assert!(off(spelling), "{spelling:?}");
        }
        for spelling in ["1", "yes", "on", "true", "please"] {
            assert!(!off(spelling), "{spelling:?}");
        }
    }

    /// The seats a draft is shown by arrive one at a time, because banners light as their
    /// seats fill. A seat read on the look that could only reach two banners has to still
    /// be there on the look that reaches three, or the draft shows nothing until three
    /// banners happen to be legible at once.
    #[test]
    fn a_seat_read_once_survives_a_look_that_misses_it() {
        let mut r = reader();
        r.keep_read(vec![(seat(false, 0), "geemelodie".into(), (10, 10))]);
        r.keep_read(vec![(seat(false, 1), "SageLion".into(), (8, 8))]);
        r.keep_read(vec![(seat(false, 2), "Caive".into(), (5, 5))]);
        assert_eq!(r.held.len(), 3);
        // The one read three looks ago is still standing, under the slot it was cut from.
        assert_eq!(r.held[0].0, 0);
        assert_eq!(r.held[0].2, "geemelodie");
    }

    /// A banner goes murky as its pick locks in, and a murky frame is where a misread
    /// comes from. The seat keeps the name it was given by the frame that read it best.
    #[test]
    fn a_dimmer_reading_does_not_displace_a_clearer_one() {
        let mut r = reader();
        r.keep_read(vec![(seat(true, 2), "SageLion".into(), (8, 8))]);
        r.keep_read(vec![(seat(true, 2), "Sagel_on".into(), (2, 8))]);
        assert_eq!(r.held.len(), 1);
        assert_eq!(r.held[0].2, "SageLion");
    }

    /// And a clearer reading does displace a dimmer one, so a seat misread while it was
    /// dark is corrected by the frame that finally shows it lit.
    #[test]
    fn a_clearer_reading_displaces_a_dimmer_one() {
        let mut r = reader();
        r.keep_read(vec![(seat(true, 2), "Sagel_on".into(), (2, 8))]);
        r.keep_read(vec![(seat(true, 2), "SageLion".into(), (8, 8))]);
        assert_eq!(r.held[0].2, "SageLion");
    }

    /// The reads belong to the draft they were read from. Carried into the next one they
    /// would seat last game's players off no evidence at all.
    #[test]
    fn forgetting_a_draft_drops_its_readings() {
        let mut r = reader();
        r.keep_read(vec![(seat(false, 0), "geemelodie".into(), (10, 10))]);
        r.forget();
        assert!(r.held.is_empty());
    }

    /// The banners a draft is named by are the ones the battlelobby gets, and the
    /// battlelobby arrives when the draft is at its dimmest. The clear frame from the
    /// middle of it has to survive the murky one at the end of it.
    #[test]
    fn a_dimmer_frame_does_not_displace_a_clearer_one() {
        let mut r = reader();
        r.keep_clearest(vec![(3, shot((6, 6)))]);
        r.keep_clearest(vec![(3, shot((0, 2)))]);
        assert_eq!(r.seen.len(), 1);
        assert_eq!(r.seen[0].1.legibility, (6, 6));
    }

    /// And a clearer one does displace a murky one, so a banner that only comes good
    /// halfway through the draft is the one that gets filed.
    #[test]
    fn a_clearer_frame_displaces_a_dimmer_one() {
        let mut r = reader();
        r.keep_clearest(vec![(3, shot((0, 2)))]);
        r.keep_clearest(vec![(3, shot((6, 6)))]);
        assert_eq!(r.seen[0].1.legibility, (6, 6));
    }

    /// A banner whose letters this atlas has never seen places nothing in any frame, so
    /// the cutting is all there is to go on: the frame that found six shapes is a better
    /// picture of a six letter name than the one that found two.
    #[test]
    fn unreadable_banners_are_judged_on_what_they_cut_into() {
        let mut r = reader();
        r.keep_clearest(vec![(3, shot((0, 2)))]);
        r.keep_clearest(vec![(3, shot((0, 6)))]);
        assert_eq!(r.seen[0].1.legibility, (0, 6));
    }

    /// A look that drops a seat has not emptied it. Every slot the draft ever showed is
    /// still in hand when the battlelobby turns up to name them.
    #[test]
    fn a_seat_missing_from_one_look_is_still_held() {
        let mut r = reader();
        r.keep_clearest(vec![(0, shot((4, 4))), (7, shot((5, 5)))]);
        r.keep_clearest(vec![(0, shot((1, 4)))]);
        let slots: Vec<u8> = r.seen.iter().map(|(slot, _)| *slot).collect();
        assert_eq!(slots, vec![0, 7]);
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
        let lobby = reader()
            .lobby(&reads, &pool())
            .expect("three seats is a draft");
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
        let lobby = reader()
            .lobby(&reads, &pool())
            .expect("three good seats stand");
        assert_eq!(lobby.players.len(), 3, "a guess was seated");
    }

    /// Whether there is a draft on the screen at all is settled in `look`, which wants
    /// `LEAST_SEATS` legible banners before it asks this. What is left here is only who
    /// could be placed, and one acquaintance among nine strangers is the case the reader
    /// exists for: requiring three placed seats threw that whole lobby away.
    #[test]
    fn one_player_on_a_screen_is_still_worth_showing() {
        let reads = vec![(seat(false, 0), "geemelodie".to_string())];
        let lobby = reader()
            .lobby(&reads, &pool())
            .expect("a named seat is worth showing");
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
        assert!(reader().lobby(&reads, &pool()).is_none());
    }

    /// A Real ID friend's banner says their real name, so the spelling the pool holds
    /// for them is nowhere on the screen and the seat can never be placed. Once the
    /// battlelobby and the banner have been seen together, it reads like any other.
    #[test]
    fn a_friend_the_screen_renames_is_still_seated() {
        let mut r = reader();
        let reads = vec![(seat(false, 0), "GabrielVargas".to_string())];
        assert!(
            r.lobby(&reads, &pool()).is_none(),
            "seated before the name was known"
        );

        r.aliases.record("GabrielVargas", "Caive#1258");
        let lobby = r.lobby(&reads, &pool()).expect("the name places the seat");
        assert_eq!(lobby.players[0].battletag, "Caive#1258");
    }

    /// The name of a friend who is not in this draft must not seat them in it. Their
    /// real name is as much a name as any other and would win its seat outright.
    #[test]
    fn a_friend_sitting_this_one_out_takes_no_seat() {
        let mut r = reader();
        r.aliases.record("GabrielVargas", "Varguitos#11833");
        let reads = vec![(seat(false, 0), "GabrielVargas".to_string())];
        assert!(
            r.lobby(&reads, &pool()).is_none(),
            "seated an absent player"
        );
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
        let lobby = reader().lobby(&reads, &pool()).unwrap();
        assert_eq!(lobby.players.len(), 3);
    }

    /// A seat on Alarak has a card titled ALARAK, and the pool holds a player of that
    /// name. The draft this was written after gave that seat to him.
    #[test]
    fn a_hero_titling_a_card_takes_no_players_seat() {
        let mut tags = pool();
        tags.push("Alarak#11471".to_string());
        let reads = vec![
            (seat(false, 0), "geemelodie".to_string()),
            (seat(false, 1), "ALARAK".to_string()),
        ];
        let lobby = reader()
            .lobby(&reads, &tags)
            .expect("geemelodie still stands");
        let seated: Vec<&str> = lobby.players.iter().map(|p| p.battletag.as_str()).collect();
        assert_eq!(seated, vec!["geemelodie#1711"]);
    }
}
