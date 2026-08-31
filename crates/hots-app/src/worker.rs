use std::sync::mpsc::{Receiver, Sender, channel};

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
}

impl Worker {
    pub fn start(settings: Settings) -> Worker {
        let (tx, reports) = channel();
        std::thread::spawn(move || run(settings, tx));
        Worker { reports }
    }
}

fn run(settings: Settings, tx: Sender<Report>) {
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

    backfill(&store, &cfg, &tx);

    let (events, rx) = channel();
    let _watchers = match watch::start(&cfg, events) {
        Ok(watchers) => watchers,
        Err(e) => return drop(tx.send(Report::Failed(format!("watch: {e}")))),
    };

    let me = Some(settings.battletag.clone()).filter(|tag| !tag.is_empty());
    while let Ok(event) = rx.recv() {
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

fn backfill(store: &Store, cfg: &hots_core::Config, tx: &Sender<Report>) {
    let known = match store.known() {
        Ok(known) => known,
        Err(e) => return drop(tx.send(Report::Failed(e))),
    };
    let known: std::collections::HashSet<String> = known.into_iter().collect();

    let files: Vec<_> = ingest::scan_dirs(&paths::replay_dirs(cfg))
        .into_iter()
        .filter(|path| !known.contains(&key_of(path)))
        .collect();

    let total = files.len() as u32;
    let mut failed = 0;
    for (done, path) in files.iter().enumerate() {
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

/// The file name is the key, since two machines hold the same replay at different paths.
fn key_of(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn submit(store: &Store, path: &std::path::Path, tx: &Sender<Report>) -> bool {
    let record = match hots_parse::replay(path) {
        Ok(record) => record,
        Err(e) => {
            let _ = tx.send(Report::Failed(format!("{}: {e}", key_of(path))));
            return false;
        }
    };
    match store.submit(&key_of(path), &record) {
        Ok(reply) => {
            if !reply.stored {
                tracing::debug!(file = key_of(path), "another replay of a stored match");
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
