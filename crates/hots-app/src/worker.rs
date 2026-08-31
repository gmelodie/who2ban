use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use hots_core::watch::{self, WatchEvent};
use hots_core::{Draft, ingest, paths};

use crate::settings::Settings;
use crate::store::Store;

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
    Failed(String),
}

pub struct Worker {
    pub reports: Receiver<Report>,
    stop: Arc<AtomicBool>,
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
        let stop = Arc::new(AtomicBool::new(false));
        let mine = stop.clone();
        std::thread::spawn(move || run(settings, tx, mine));
        Worker { reports, stop }
    }
}

fn run(settings: Settings, tx: Sender<Report>, stop: Arc<AtomicBool>) {
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

    backfill(&store, &cfg, &tx, &stop);

    let (events, rx) = channel();
    let _watchers = match watch::start(&cfg, events) {
        Ok(watchers) => watchers,
        Err(e) => return drop(tx.send(Report::Failed(format!("watch: {e}")))),
    };

    let me = Some(settings.battletag.clone()).filter(|tag| !tag.is_empty());
    while !stop.load(Ordering::Relaxed) {
        let event = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        match event {
            WatchEvent::Replay(path) => {
                submit(&store, &path, &tx);
            }
            WatchEvent::Lobby(bytes) => match hots_parse::battlelobby(&bytes) {
                Ok(lobby) => match store.draft(&cfg, &lobby, me.as_deref()) {
                    Ok(draft) => drop(tx.send(Report::Lobby(Box::new(draft)))),
                    Err(e) => drop(tx.send(Report::Failed(e))),
                },
                Err(e) => drop(tx.send(Report::Failed(format!("lobby: {e}")))),
            },
        }
    }
}

fn backfill(store: &Store, cfg: &hots_core::Config, tx: &Sender<Report>, stop: &AtomicBool) {
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
    let record = match hots_parse::replay(path) {
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
            true
        }
        Err(e) => {
            let _ = tx.send(Report::Failed(e));
            false
        }
    }
}
