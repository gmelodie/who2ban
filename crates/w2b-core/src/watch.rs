use std::path::{Path, PathBuf};
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

/// A client that was killed leaves its lobby behind, and no match runs this long.
const FRESH: Duration = Duration::from_secs(2 * 60 * 60);

/// A read that lands mid-write finds fewer than ten players, which is reported as noise.
const SETTLE: Duration = Duration::from_secs(2);

/// The subfolder the client writes the live lobby into.
const LOBBY_DIR: &str = "TempWriteReplayP1";

#[derive(Debug, Clone, PartialEq)]
struct Stamp {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
}

fn lobby_paths(cfg: &Config) -> Vec<PathBuf> {
    paths::temp_roots(cfg)
        .into_iter()
        .map(|root| root.join(LOBBY_DIR).join(paths::BATTLELOBBY_NAME))
        .collect()
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Stamp {
        path: path.to_path_buf(),
        len: meta.len(),
        modified: meta.modified().ok()?,
    })
}

/// A clock that moved backwards dates the live file in the future.
fn is_fresh(found: &Stamp) -> bool {
    found.modified.elapsed().map_or(true, |age| age < FRESH)
}

/// The lobby of whichever prefix wrote one last. An empty file is a created one, not a game.
fn newest_lobby(paths: &[PathBuf]) -> Option<Stamp> {
    paths
        .iter()
        .filter_map(|path| stamp(path))
        .filter(|found| found.len > 0 && is_fresh(found))
        .max_by_key(|found| found.modified)
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
        let mut last: Option<Stamp> = None;
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(400));
            // The scan walks every wine prefix, so it stops once a lobby answers.
            if last.is_none() && looked.elapsed() >= RESOLVE_EVERY {
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
            if !ingest::wait_until_stable(&found.path, SETTLE) {
                continue;
            }
            let (Some(settled), Ok(bytes)) = (stamp(&found.path), std::fs::read(&found.path))
            else {
                continue;
            };
            last = Some(settled);
            tracing::info!(file = %found.path.display(), bytes = bytes.len(), "lobby");
            if tx.send(WatchEvent::Lobby(bytes)).is_err() {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lobby_of_a_client_that_was_killed_is_not_a_game() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(paths::BATTLELOBBY_NAME);
        std::fs::write(&path, b"a lobby").unwrap();
        assert!(newest_lobby(std::slice::from_ref(&path)).is_some());

        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() - FRESH - Duration::from_secs(60))
            .unwrap();
        assert!(newest_lobby(&[path]).is_none());
    }

    #[test]
    fn an_empty_lobby_is_a_file_the_client_has_not_written_yet() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(paths::BATTLELOBBY_NAME);
        std::fs::write(&path, b"").unwrap();
        assert!(newest_lobby(&[path]).is_none());
    }
}
