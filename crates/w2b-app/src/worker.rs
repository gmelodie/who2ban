use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use w2b_core::watch::{self, WatchEvent};
use w2b_core::{Draft, ingest, paths};

use crate::screen;
use crate::settings::Settings;
use crate::store::Store;

/// One seat of a finished match: who sat in it, what they played, and the id that names
/// that hero the same way whatever language the replay was saved in.
pub struct PlayedHero {
    pub battletag: String,
    pub hero: String,
    pub hero_id: Option<String>,
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
    backfill(&store, &cfg, &tx, &stop, &rx, &orders, me.as_deref(), &mut reader);

    // Wayland and a headless machine both refuse the screen, and a store kept on a
    // server has no roster to match a read against. Either way the client goes on
    // working from the battlelobby, which is what it did before it could read at all.
    let watching = reader.can_look();
    tracing::info!(
        screen = watching,
        letters = reader.letters_known(),
        "draft reader"
    );

    // The pool is every player on record, read once: a name the reader cannot find here
    // is one there would be nothing to show about anyway.
    let mut pool = store.battletags();
    let mut looked = std::time::Instant::now();
    let mut on_screen: Vec<String> = Vec::new();

    while !stop.load(Ordering::Relaxed) {
        obey(&store, &tx, &orders);

        if watching && !pool.is_empty() && looked.elapsed() >= LOOK_EVERY {
            looked = std::time::Instant::now();
            look(&store, &cfg, &tx, me.as_deref(), &mut reader, &pool, &mut on_screen);
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
        handle(&store, &cfg, &tx, me.as_deref(), &mut reader, event);
    }
}

fn obey(store: &Store, tx: &Sender<Report>, orders: &Receiver<Command>) {
    for order in orders.try_iter() {
        match order {
            Command::SaveNote { battletag, note } => {
                if let Err(e) = store.set_note(&battletag, &note) {
                    let _ = tx.send(Report::Failed(format!("note on {battletag}: {e}")));
                }
            }
        }
    }
}

fn handle(
    store: &Store,
    cfg: &w2b_core::Config,
    tx: &Sender<Report>,
    me: Option<&str>,
    reader: &mut screen::Reader,
    event: WatchEvent,
) {
    match event {
        WatchEvent::Replay(path) => {
            submit(store, &path, tx);
        }
        WatchEvent::Lobby(bytes) => match w2b_parse::battlelobby(&bytes) {
            Ok(lobby) => {
                // The file is the truth, so it both replaces whatever was read off the
                // screen and says what those shapes were. This is the only moment the
                // reader is ever told it was right.
                let names: Vec<String> =
                    lobby.players.iter().map(|p| p.battletag.clone()).collect();
                let learned = reader.harvest(&names);
                if learned > 0 {
                    if let Err(e) = reader.save() {
                        let _ = tx.send(Report::Failed(format!("atlas: {e}")));
                    }
                    tracing::info!(banners = learned, letters = reader.letters_known(), "learned");
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
/// Returns the roster it reported, so an unchanged screen stays quiet.
fn look(
    store: &Store,
    cfg: &w2b_core::Config,
    tx: &Sender<Report>,
    me: Option<&str>,
    reader: &mut screen::Reader,
    pool: &[String],
    last: &mut Vec<String>,
) {
    let Some(reads) = reader.look() else { return };
    let Some(lobby) = screen::Reader::lobby(&reads, pool) else {
        return;
    };

    let mut roster: Vec<String> = lobby.players.iter().map(|p| p.battletag.clone()).collect();
    roster.sort();
    if roster == *last {
        return;
    }
    match store.draft(cfg, &lobby, me) {
        Ok(draft) => {
            tracing::info!(seats = roster.len(), "draft read from the screen");
            *last = roster;
            let _ = tx.send(Report::Lobby(Box::new(draft)));
        }
        Err(e) => drop(tx.send(Report::Failed(e))),
    }
}

/// Every replay the store has not seen. A lobby that forms while this runs is answered
/// between files rather than after the last one.
fn backfill(
    store: &Store,
    cfg: &w2b_core::Config,
    tx: &Sender<Report>,
    stop: &AtomicBool,
    events: &Receiver<WatchEvent>,
    orders: &Receiver<Command>,
    me: Option<&str>,
    reader: &mut screen::Reader,
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
            handle(store, cfg, tx, me, reader, event);
        }
        obey(store, tx, orders);
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
