use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime};

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
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        let Ok(event) = res else { return };
        if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
            return;
        }
        for path in event.paths.into_iter().filter(|p| paths::is_replay(p)) {
            if ingest::wait_until_stable(&path, Duration::from_secs(20)) {
                let _ = tx.send(WatchEvent::Replay(path));
            }
        }
    })?;
    for dir in dirs {
        watcher.watch(&dir, RecursiveMode::NonRecursive)?;
    }
    Ok(Some(watcher))
}

fn start_lobby_poll(cfg: &Config, tx: Sender<WatchEvent>, stop: Arc<AtomicBool>) {
    let path = paths::temp_root(cfg)
        .join("TempWriteReplayP1")
        .join(paths::BATTLELOBBY_NAME);

    std::thread::spawn(move || {
        let mut last: Option<(u64, SystemTime)> = None;
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(400));
            let Ok(meta) = std::fs::metadata(&path) else {
                last = None;
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            let stamp = (meta.len(), modified);
            if last == Some(stamp) || meta.len() == 0 {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            last = Some(stamp);
            if tx.send(WatchEvent::Lobby(bytes)).is_err() {
                return;
            }
        }
    });
}
