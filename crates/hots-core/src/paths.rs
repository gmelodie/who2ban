use std::path::{Path, PathBuf};

use crate::config::Config;

pub const BATTLELOBBY_NAME: &str = "replay.server.battlelobby";
pub const TEMP_SUBDIR: &str = "Heroes of the Storm";

pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("HOTS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("hots-draft")
}

pub fn db_path() -> PathBuf {
    data_dir().join("hots.db")
}

/// Root the client recreates on every launch and deletes on exit.
pub fn temp_root(cfg: &Config) -> PathBuf {
    if let Some(dir) = &cfg.temp_dir {
        return dir.clone();
    }
    if let Ok(dir) = std::env::var("HOTS_TEMP_DIR") {
        return PathBuf::from(dir);
    }
    std::env::temp_dir().join(TEMP_SUBDIR)
}

pub fn is_battlelobby(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some(BATTLELOBBY_NAME)
}

pub fn is_replay(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("StormReplay"))
}

/// Every `Accounts/<id>/<hero-id>/Replays/Multiplayer` under Documents.
pub fn replay_dirs(cfg: &Config) -> Vec<PathBuf> {
    if let Some(dir) = &cfg.replay_dir {
        return vec![dir.clone()];
    }
    if let Ok(dir) = std::env::var("HOTS_REPLAY_DIR") {
        return vec![PathBuf::from(dir)];
    }
    let accounts = match dirs::document_dir() {
        Some(docs) => docs.join(TEMP_SUBDIR).join("Accounts"),
        None => return Vec::new(),
    };
    multiplayer_dirs(&accounts)
}

fn multiplayer_dirs(accounts: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(accounts)
        .min_depth(3)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir() && e.file_name() == "Replays")
        .map(|e| e.path().join("Multiplayer"))
        .filter(|p| p.is_dir())
        .collect()
}
