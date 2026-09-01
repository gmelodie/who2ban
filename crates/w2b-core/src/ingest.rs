use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::db::Db;
use crate::error::Result;
use crate::model::IngestProgress;
use crate::paths;

pub fn scan_dirs(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = dirs
        .iter()
        .flat_map(|d| walkdir::WalkDir::new(d).into_iter().filter_map(|e| e.ok()))
        .filter(|e| e.file_type().is_file() && paths::is_replay(e.path()))
        .map(|e| e.into_path())
        .collect();
    out.sort();
    out
}

/// The file name, since two machines hold the same replay at different paths.
pub fn replay_key(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .to_string()
}

pub fn ingest_file(db: &Db, path: &Path) -> Result<Option<i64>> {
    let key = replay_key(path);
    match w2b_parse::replay(path) {
        Ok(replay) => db.record_replay(&key, &replay),
        Err(e) => {
            db.record_replay_error(&key, &e.to_string())?;
            Err(e.into())
        }
    }
}

pub fn backfill(
    db: &Db,
    dirs: &[PathBuf],
    mut on_progress: impl FnMut(&IngestProgress),
) -> Result<IngestProgress> {
    let known = db.known_replays()?;
    let files: Vec<PathBuf> = scan_dirs(dirs)
        .into_iter()
        .filter(|p| !known.contains(&replay_key(p)))
        .collect();

    let mut progress = IngestProgress {
        done: 0,
        total: files.len() as u32,
        failed: 0,
    };
    on_progress(&progress);

    for file in files {
        if ingest_file(db, &file).is_err() {
            progress.failed += 1;
        }
        progress.done += 1;
        on_progress(&progress);
    }
    Ok(progress)
}

/// Block until the file stops growing, so a half-written replay is never parsed.
pub fn wait_until_stable(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let mut last = None;
    while Instant::now() < deadline {
        let stamp = std::fs::metadata(path)
            .ok()
            .and_then(|m| Some((m.len(), m.modified().ok()?)));
        if stamp.is_some() && stamp == last {
            return true;
        }
        last = stamp;
        std::thread::sleep(Duration::from_millis(120));
    }
    false
}
