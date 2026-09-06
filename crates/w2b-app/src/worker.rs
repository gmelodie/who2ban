use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use w2b_core::watch::{self, WatchEvent};
use w2b_core::{Draft, ingest, paths};
use w2b_parse::{Lobby, LobbyPlayer};

use crate::screen;
use crate::settings::Settings;
use crate::store::Store;

/// One seat of a finished match: who sat in it, what they played, and the id that names
/// that hero the same way whatever language the replay was saved in.
pub struct PlayedHero {
    pub battletag: String,
    pub hero: String,
    pub hero_id: Option<String>,
    /// The game's own verdict on who played best, which is worth a mark on the card: it
    /// is the one line of a scoreboard that anybody remembers an hour later.
    pub mvp: bool,
}

pub enum Report {
    Store(String),
    Folders {
        temp: Option<String>,
        replays: usize,
    },
    Backfill {
        done: u32,
        total: u32,
        failed: u32,
    },
    Matches(u32),
    Lobby(Box<Draft>),
    /// A match just parsed from a replay file, which is how the app learns that the lobby
    /// it is showing has finished playing itself out, and how it ended.
    Played {
        battletags: Vec<String>,
        winners: Vec<String>,
        /// Who played what, so the recap can say which card was who rather than leaving
        /// it to be remembered.
        heroes: Vec<PlayedHero>,
        map: String,
    },
    Failed(String),
}

/// What the window asks the worker to do. Saving a note is a request to a server across
/// the internet, and the frame that asked must not wait on it.
pub enum Command {
    SaveNote {
        battletag: String,
        note: w2b_core::PlayerNote,
    },
    /// A seat the reader got wrong, or never placed, named by the person looking at it.
    Correct {
        slot: u8,
        battletag: String,
    },
}

pub struct Worker {
    pub reports: Receiver<Report>,
    orders: Sender<Command>,
    stop: Arc<AtomicBool>,
}

impl Worker {
    pub fn send(&self, command: Command) {
        let _ = self.orders.send(command);
    }
}

/// A replaced worker that keeps running races the new one over the same replays.
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Worker {
    pub fn start(settings: Settings) -> Worker {
        let (tx, reports) = channel();
        let (orders, taking) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let mine = stop.clone();
        std::thread::spawn(move || run(settings, tx, taking, mine));
        Worker {
            reports,
            orders,
            stop,
        }
    }
}

fn run(settings: Settings, tx: Sender<Report>, orders: Receiver<Command>, stop: Arc<AtomicBool>) {
    let cfg = settings.folders();
    let store = match Store::open(&settings) {
        Ok(store) => store,
        Err(e) => return drop(tx.send(Report::Failed(e.to_string()))),
    };

    let _ = tx.send(Report::Store(store.describe()));
    let _ = tx.send(Report::Folders {
        temp: paths::found_temp_root(&cfg).map(|dir| dir.display().to_string()),
        replays: paths::replay_dirs(&cfg).len(),
    });

    // The draft is what anyone opens this program for, so the watch starts before the
    // upload does. A backfill of a thousand replays must not blind it through a draft.
    let (events, rx) = channel();
    let _watchers = match watch::start(&cfg, events) {
        Ok(watchers) => watchers,
        Err(e) => return drop(tx.send(Report::Failed(format!("watch: {e}")))),
    };

    let me = Some(settings.battletag.clone()).filter(|tag| !tag.is_empty());
    let mut reader = screen::Reader::open(&paths::data_dir());
    // Before the first draft, not after it: the shapes are only worth having in advance.
    if let Some(pooled) = store.glyphs() {
        let gained = reader.absorb(&pooled);
        if gained > 0 {
            let _ = reader.save();
        }
        tracing::info!(gained, letters = reader.letters_known(), "glyphs from the server");
    }
    // Who the battlelobby last named, and what was last read off the screen. Declared
    // before the backfill because a lobby can form while it runs.
    let mut from_file: Vec<String> = Vec::new();
    let mut on_screen: Vec<LobbyPlayer> = Vec::new();
    // The pool is every player on record, read once: a name the reader cannot find here
    // is one there would be nothing to show about anyway. Read before the backfill rather
    // than after it, because the backfill takes minutes and answers corrections while it
    // runs, and a correction with no pool behind it cannot fill in a discriminator.
    let mut pool = store.battletags();
    backfill(
        &store,
        &cfg,
        &tx,
        &stop,
        &rx,
        &orders,
        me.as_deref(),
        &mut reader,
        &mut from_file,
        &mut on_screen,
        &pool,
    );

    // Wayland and a headless machine both refuse the screen, and a store kept on a
    // server has no roster to match a read against. Either way the client goes on
    // working from the battlelobby, which is what it did before it could read at all.
    let watching = reader.can_look();
    tracing::info!(
        screen = watching,
        letters = reader.letters_known(),
        "draft reader"
    );

    let mut looked = std::time::Instant::now();

    while !stop.load(Ordering::Relaxed) {
        obey(
            &store,
            &cfg,
            &tx,
            &orders,
            &mut reader,
            me.as_deref(),
            &mut on_screen,
            &pool,
        );

        if watching && !pool.is_empty() && looked.elapsed() >= LOOK_EVERY {
            looked = std::time::Instant::now();
            look(
                &store,
                &cfg,
                &tx,
                me.as_deref(),
                &mut reader,
                &pool,
                &mut on_screen,
                &mut from_file,
            );
        }

        let event = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        // A finished match adds players, and the next draft may hold one of them.
        if matches!(event, WatchEvent::Replay(_)) {
            pool = store.battletags();
        }
        handle(
            &store,
            &cfg,
            &tx,
            me.as_deref(),
            &mut reader,
            event,
            &mut from_file,
            &mut on_screen,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn obey(
    store: &Store,
    cfg: &w2b_core::Config,
    tx: &Sender<Report>,
    orders: &Receiver<Command>,
    reader: &mut screen::Reader,
    me: Option<&str>,
    seated: &mut Vec<LobbyPlayer>,
    pool: &[String],
) {
    for order in orders.try_iter() {
        match order {
            Command::SaveNote { battletag, note } => {
                if let Err(e) = store.set_note(&battletag, &note) {
                    let _ = tx.send(Report::Failed(format!("note on {battletag}: {e}")));
                }
            }
            Command::Correct { slot, battletag } => {
                correct(store, cfg, tx, reader, me, seated, pool, slot, battletag);
            }
        }
    }
}

/// A name the reader got wrong, put right by the person looking at the screen.
///
/// Worth more than the correction of one card. The banner is still in hand, so the shapes
/// on it can be filed under the letters they actually are, and the letters a reader gets
/// wrong are by definition the ones it does not know. A user who corrects `Hogger` has
/// just taught it `H`, which no draft has managed to teach it yet.
#[allow(clippy::too_many_arguments)]
fn correct(
    store: &Store,
    cfg: &w2b_core::Config,
    tx: &Sender<Report>,
    reader: &mut screen::Reader,
    me: Option<&str>,
    seated: &mut Vec<LobbyPlayer>,
    pool: &[String],
    slot: u8,
    typed: String,
) {
    let battletag = resolve(&typed, pool);
    let name = battletag
        .split_once('#')
        .map_or(battletag.as_str(), |(n, _)| n)
        .to_string();

    let filed = reader.teach(slot, &name);
    if filed {
        if let Err(e) = reader.save() {
            let _ = tx.send(Report::Failed(format!("atlas: {e}")));
        }
        if let Some(gained) = store.push_glyphs(reader.atlas()) {
            tracing::info!(gained, "glyphs given to the server");
        }
    }
    tracing::info!(
        slot,
        %battletag,
        filed,
        letters = reader.letters_known(),
        "corrected"
    );

    // The card changes whether or not the shapes could be filed: the person said who is
    // in that seat, and that is true regardless of what the atlas could make of it.
    seated.retain(|had| had.slot != slot && had.battletag != battletag);
    seated.push(LobbyPlayer {
        team: u8::from(slot >= 5),
        slot,
        battletag,
    });
    seated.sort_by_key(|had| had.slot);

    let whole = Lobby {
        players: seated.clone(),
        region: 0,
    };
    match store.draft(cfg, &whole, me) {
        Ok(draft) => drop(tx.send(Report::Lobby(Box::new(draft)))),
        Err(e) => drop(tx.send(Report::Failed(e))),
    }
}

/// The battletag the user meant. They see a name without its discriminator on the screen,
/// so that is what they type; the pool knows the rest of it.
fn resolve(typed: &str, pool: &[String]) -> String {
    let typed = typed.trim();
    if typed.contains('#') {
        return typed.to_string();
    }
    pool.iter()
        .find(|tag| {
            tag.split_once('#')
                .map_or(tag.as_str(), |(name, _)| name)
                .eq_ignore_ascii_case(typed)
        })
        .cloned()
        .unwrap_or_else(|| typed.to_string())
}

#[allow(clippy::too_many_arguments)]
fn handle(
    store: &Store,
    cfg: &w2b_core::Config,
    tx: &Sender<Report>,
    me: Option<&str>,
    reader: &mut screen::Reader,
    event: WatchEvent,
    from_file: &mut Vec<String>,
    on_screen: &mut Vec<LobbyPlayer>,
) {
    match event {
        WatchEvent::Replay(path) => {
            // A replay is written when a match finishes, so the lobby it describes has
            // nothing left to say about what is on the screen now.
            from_file.clear();
            on_screen.clear();
            submit(store, &path, tx);
        }
        WatchEvent::Lobby(bytes) => match w2b_parse::battlelobby(&bytes) {
            Ok(lobby) => {
                // The file is the truth, so it both replaces whatever was read off the
                // screen and says what those shapes were. This is the only moment the
                // reader is ever told it was right.
                let names: Vec<String> =
                    lobby.players.iter().map(|p| p.battletag.clone()).collect();
                *from_file = names.clone();
                // The next draft is a different lobby, so what was read for this one must
                // not be mistaken for it.
                on_screen.clear();
                let learned = reader.harvest(&names);
                if learned > 0 {
                    if let Err(e) = reader.save() {
                        let _ = tx.send(Report::Failed(format!("atlas: {e}")));
                    }
                    tracing::info!(banners = learned, letters = reader.letters_known(), "learned");
                    if let Some(gained) = store.push_glyphs(reader.atlas()) {
                        tracing::info!(gained, "glyphs given to the server");
                    }
                }
                // Banners this atlas could not cut up. The pool may have the letters
                // this client is missing, so it is sent the picture instead.
                let unfiled = reader.take_unfiled();
                if let Some(gained) = store.push_banners(&unfiled) {
                    tracing::info!(banners = unfiled.len(), gained, "banners given to the server");
                }
                match store.draft(cfg, &lobby, me) {
                    Ok(draft) => drop(tx.send(Report::Lobby(Box::new(draft)))),
                    Err(e) => drop(tx.send(Report::Failed(e))),
                }
            }
            Err(e) => drop(tx.send(Report::Failed(format!("lobby: {e}")))),
        },
    }
}

/// The screen is looked at this often while no draft has been found on it. A draft runs
/// for minutes, so nothing is missed by not looking harder, and a grab of a 4K screen is
/// not free.
const LOOK_EVERY: Duration = Duration::from_secs(2);

/// Read the draft off the screen and report it, unless it says the same as last time.
/// `seated` is what the screen has placed for this draft so far, which only ever grows
/// until the draft is over.
#[allow(clippy::too_many_arguments)]
fn look(
    store: &Store,
    cfg: &w2b_core::Config,
    tx: &Sender<Report>,
    me: Option<&str>,
    reader: &mut screen::Reader,
    pool: &[String],
    seated: &mut Vec<LobbyPlayer>,
    from_file: &mut Vec<String>,
) {
    let Some(reads) = reader.look(pool) else {
        // What the battlelobby named describes a match that is over, so it is dropped
        // rather than held against the next draft: without this, a group that re-queues
        // together suppresses its own next draft, because the only seats the reader can
        // place are the same friends the last battlelobby already named.
        //
        // But only a menu says the match is over. A look gives nothing for all sorts of
        // passing reasons - a camera over dark ground, a fight covering the HUD, a
        // dropped frame - and treating each of those as the end of the match dropped the
        // battlelobby mid-game, whereupon the next look read the HUD and seated whatever
        // it made of it. Three letters of noise that happen to spell a short name is all
        // it takes, and the cards went haywire for the rest of the game.
        if reader.saw_menu() {
            from_file.clear();
            seated.clear();
        }
        return;
    };
    // From the loading screen onwards the battlelobby has named all ten seats, and no
    // read of the screen can improve on that. Everything the screen shows for the rest of
    // the match is the HUD, the scoreboard, the terrain: shapes that are not names. So
    // the file stands, and goes on standing until the screen shows nothing draft-shaped
    // at all, which is the branch above clearing it when the match is over.
    //
    // This used to ask instead whether every name read was one the file already knew, and
    // a single misread defeated that: nine names off the battlelobby and one invented out
    // of the terrain is not "the same match", so the match in progress was republished as
    // a fresh draft with a stranger in it.
    if !from_file.is_empty() {
        reader.forget();
        reader.say("the battlelobby has named this match, so the file stands");
        return;
    }
    let Some(lobby) = screen::Reader::lobby(&reads, pool) else {
        // The banners were read and named nobody on record. Worth saying: it is the one
        // failure that looks identical to a working reader with no draft in front of it.
        reader.say("read the draft, seated nobody on record");
        return;
    };

    // Seats accumulate across reads rather than each read replacing the draft.
    //
    // The ten names are on the screen for the whole of the draft, so a banner that reads
    // this time and not the next did not empty: it went dim, or a portrait locked in over
    // it, or the ladder picked a different rung. Publishing each read whole made the cards
    // flicker, players arriving and leaving as the reads disagreed, and a player who took
    // several attempts to place would vanish again on the next.
    let mut gained = 0;
    for player in lobby.players {
        if seated.iter().any(|had| had.battletag == player.battletag) {
            continue;
        }
        // Two names for one seat means one of the two reads is wrong, and a later read is
        // no more trustworthy than an earlier one. The first answer stands.
        if seated.iter().any(|had| had.slot == player.slot) {
            continue;
        }
        seated.push(player);
        gained += 1;
    }
    if gained == 0 {
        return;
    }

    let whole = Lobby {
        players: seated.clone(),
        // The screen does not say, and nothing downstream of a local draft asks.
        region: 0,
    };
    match store.draft(cfg, &whole, me) {
        Ok(mut draft) => {
            own_side(&mut draft);
            reader.say("seating a draft read from the screen");
            tracing::info!(gained, seats = seated.len(), "draft read from the screen");
            let _ = tx.send(Report::Lobby(Box::new(draft)));
        }
        Err(e) => drop(tx.send(Report::Failed(e))),
    }
}

/// Which side is yours, for a draft that came off the screen.
///
/// The draft screen puts your own team in the left panel and the enemy in the right one,
/// every time, and the reader has already filed the two panels as teams 0 and 1. So the
/// answer is on the screen before a single name is legible.
///
/// Whoever built the draft answered this by looking for the configured battletag among
/// the seats, which is the only thing a battlelobby can do but is strictly worse here. It
/// waits for your own banner to be one of the ones read, so the first seats arrive with
/// both sides shown as allies; and if your banner is misread - or your battletag is
/// spelt differently from the name on it - it never arrives at all, and a draft whose
/// sides were never in doubt is shown as ten allies with the apology above them.
fn own_side(draft: &mut Draft) {
    draft.my_team = Some(LEFT_PANEL);
    for player in &mut draft.players {
        player.enemy = player.team != LEFT_PANEL;
    }
}

/// The panel `screen::Reader` files your own side under. Its seats are slots 0 to 4.
const LEFT_PANEL: u8 = 0;

/// Every replay the store has not seen. A lobby that forms while this runs is answered
/// between files rather than after the last one.
#[allow(clippy::too_many_arguments)]
fn backfill(
    store: &Store,
    cfg: &w2b_core::Config,
    tx: &Sender<Report>,
    stop: &AtomicBool,
    events: &Receiver<WatchEvent>,
    orders: &Receiver<Command>,
    me: Option<&str>,
    reader: &mut screen::Reader,
    from_file: &mut Vec<String>,
    on_screen: &mut Vec<LobbyPlayer>,
    pool: &[String],
) {
    let known = match store.known() {
        Ok(known) => known,
        Err(e) => return drop(tx.send(Report::Failed(e))),
    };
    let known: std::collections::HashSet<String> = known.into_iter().collect();

    let files: Vec<_> = ingest::scan_dirs(&paths::replay_dirs(cfg))
        .into_iter()
        .filter(|path| !known.contains(&ingest::replay_key(path)))
        .collect();

    let total = files.len() as u32;
    let mut failed = 0;
    for (done, path) in files.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        for event in events.try_iter() {
            handle(store, cfg, tx, me, reader, event, from_file, on_screen);
        }
        obey(store, cfg, tx, orders, reader, me, on_screen, pool);
        if !submit(store, path, tx) {
            failed += 1;
        }
        let _ = tx.send(Report::Backfill {
            done: done as u32 + 1,
            total,
            failed,
        });
    }
    if let Ok(count) = store.count() {
        let _ = tx.send(Report::Matches(count));
    }
}

fn submit(store: &Store, path: &std::path::Path, tx: &Sender<Report>) -> bool {
    let record = match w2b_parse::replay(path) {
        Ok(record) => record,
        Err(e) => {
            let _ = tx.send(Report::Failed(format!("{}: {e}", ingest::replay_key(path))));
            return false;
        }
    };
    match store.submit(&ingest::replay_key(path), &record) {
        Ok(reply) => {
            if !reply.stored {
                tracing::debug!(
                    file = ingest::replay_key(path),
                    "another replay of a stored match"
                );
            }
            let _ = tx.send(Report::Matches(reply.matches));
            let tag =
                |p: &w2b_core::MatchPlayer| p.battletag.clone().unwrap_or_else(|| p.name.clone());
            let _ = tx.send(Report::Played {
                battletags: record.players.iter().map(tag).collect(),
                winners: record.players.iter().filter(|p| p.won).map(tag).collect(),
                heroes: record
                    .players
                    .iter()
                    .map(|p| PlayedHero {
                        battletag: tag(p),
                        hero: p.hero.clone(),
                        hero_id: p.hero_id.clone(),
                        mvp: p.mvp,
                    })
                    .collect(),
                map: record.map.clone(),
            });
            true
        }
        Err(e) => {
            let _ = tx.send(Report::Failed(e));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool() -> Vec<String> {
        vec![
            "AngeLo#12639".to_string(),
            "Hogger#15962".to_string(),
            "anarquista#11551".to_string(),
        ]
    }

    /// The banner shows a name without its discriminator, so that is what gets typed.
    #[test]
    fn a_name_alone_finds_its_whole_battletag() {
        assert_eq!(resolve("Hogger", &pool()), "Hogger#15962");
    }

    /// Nobody matches the case of a name they are reading off a screen.
    #[test]
    fn the_case_typed_does_not_matter() {
        assert_eq!(resolve("angelo", &pool()), "AngeLo#12639");
        assert_eq!(resolve("  ANARQUISTA  ", &pool()), "anarquista#11551");
    }

    /// Someone who types the discriminator has said everything there is to say.
    #[test]
    fn a_whole_battletag_is_taken_as_given() {
        assert_eq!(resolve("Someone#4242", &pool()), "Someone#4242");
    }

    /// A player nobody has on record still names the seat. There is nothing to show about
    /// them, but the correction is still true, and the atlas still learns the letters.
    #[test]
    fn a_stranger_is_kept_as_typed() {
        assert_eq!(resolve("Kroowar", &pool()), "Kroowar");
    }

    fn seat(slot: u8, team: u8) -> w2b_core::DraftPlayer {
        w2b_core::DraftPlayer {
            battletag: format!("Someone#{slot}"),
            slot,
            team,
            // What the builder made of it, which is what `own_side` is here to overrule.
            enemy: false,
            heroes: Vec::new(),
            games: 0,
            note: Default::default(),
        }
    }

    /// The reader could not find the configured battletag on the screen, so the draft
    /// arrived with nobody's side known. The panels still say who is who.
    #[test]
    fn the_right_panel_is_the_other_side() {
        let mut draft = Draft {
            region: 0,
            my_team: None,
            players: vec![seat(0, 0), seat(5, 1), seat(6, 1)],
        };
        own_side(&mut draft);

        assert_eq!(draft.my_team, Some(0));
        assert_eq!(draft.enemies().count(), 2);
        assert!(draft.enemies().all(|p| p.slot >= 5));
        assert_eq!(draft.allies().count(), 1);
    }

    /// The whole point: one seat read on the left is enough to know which side is yours.
    /// This is the state the old answer could say nothing about, because the one name it
    /// looked for was not among the seats read yet.
    #[test]
    fn a_single_seat_settles_it() {
        let mut draft = Draft {
            region: 0,
            my_team: None,
            players: vec![seat(7, 1)],
        };
        own_side(&mut draft);

        assert_eq!(draft.my_team, Some(0));
        assert_eq!(draft.enemies().count(), 1);
    }
}
