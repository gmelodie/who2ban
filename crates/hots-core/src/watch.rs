use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant, SystemTime};

use notify::{Event, EventKind, RecursiveMode, Watcher};

use crate::config::Config;
use crate::error::Result;
use crate::ingest;
use crate::paths;

#[derive(Debug, Clone)]
pub enum WatchEvent {
    Replay(PathBuf),
    Lobby(Vec<u8>),
}

pub struct Watchers {
    _replays: Option<notify::RecommendedWatcher>,
    stop: Arc<AtomicBool>,
}

impl Drop for Watchers {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The temp folder appears when the game launches, so a path resolved before that is a guess.
const RESOLVE_EVERY: Duration = Duration::from_secs(10);

/// The subfolder the client writes the live lobby into.
const LOBBY_DIR: &str = "TempWriteReplayP1";

fn lobby_paths(cfg: &Config) -> Vec<PathBuf> {
    paths::temp_roots(cfg)
        .into_iter()
        .map(|root| root.join(LOBBY_DIR).join(paths::BATTLELOBBY_NAME))
        .collect()
}

/// The lobby of whichever prefix wrote one last. An empty file is one the client has
/// created but not filled yet, which parses as noise.
fn newest_lobby(paths: &[PathBuf]) -> Option<(PathBuf, u64, SystemTime)> {
    paths
        .iter()
        .filter_map(|path| {
            let meta = std::fs::metadata(path).ok()?;
            Some((path.clone(), meta.len(), meta.modified().ok()?))
        })
        .filter(|(_, len, _)| *len > 0)
        .max_by_key(|(_, _, modified)| *modified)
}

/// The lobby uses a stat loop: the client deletes its temp folder on exit, which kills a watch.
pub fn start(cfg: &Config, tx: Sender<WatchEvent>) -> Result<Watchers> {
    let stop = Arc::new(AtomicBool::new(false));
    let replays = start_replay_watch(cfg, tx.clone())?;
    start_lobby_poll(cfg, tx, stop.clone());
    Ok(Watchers {
        _replays: replays,
        stop,
    })
}

fn start_replay_watch(
    cfg: &Config,
    tx: Sender<WatchEvent>,
) -> Result<Option<notify::RecommendedWatcher>> {
    let dirs: Vec<_> = paths::replay_dirs(cfg)
        .into_iter()
        .filter(|dir| dir.is_dir())
        .collect();
    if dirs.is_empty() {
        return Ok(None);
    }
    let found = start_settling(tx);
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        let Ok(event) = res else { return };
        if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
            return;
        }
        for path in event.paths.into_iter().filter(|p| paths::is_replay(p)) {
            let _ = found.send(path);
        }
    })?;
    for dir in dirs {
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;
    }
    Ok(Some(watcher))
}

/// notify delivers on one thread, and every other event queues behind this wait.
fn start_settling(tx: Sender<WatchEvent>) -> Sender<PathBuf> {
    let (found, rx) = channel::<PathBuf>();
    std::thread::spawn(move || {
        while let Ok(path) = rx.recv() {
            if ingest::wait_until_stable(&path, Duration::from_secs(20))
                && tx.send(WatchEvent::Replay(path)).is_err()
            {
                return;
            }
        }
    });
    found
}

fn start_lobby_poll(cfg: &Config, tx: Sender<WatchEvent>, stop: Arc<AtomicBool>) {
    let cfg = cfg.clone();

    std::thread::spawn(move || {
        let mut paths = lobby_paths(&cfg);
        let mut looked = Instant::now();
        let mut last: Option<(PathBuf, u64, SystemTime)> = None;
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(400));
            // The game creates its temp folder at launch, so the folders that exist
            // now are not the folders that existed when this thread started.
            if looked.elapsed() >= RESOLVE_EVERY {
                looked = Instant::now();
                paths = lobby_paths(&cfg);
            }
            let Some(found) = newest_lobby(&paths) else {
                last = None;
                continue;
            };
            if last.as_ref() == Some(&found) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&found.0) else {
                continue;
            };
            last = Some(found);
            if tx.send(WatchEvent::Lobby(bytes)).is_err() {
                return;
            }
        }
    });
}
