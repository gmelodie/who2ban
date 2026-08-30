use std::path::{Path, PathBuf};

use crate::config::Config;

pub const BATTLELOBBY_NAME: &str = "replay.server.battlelobby";
pub const TEMP_SUBDIR: &str = "Heroes of the Storm";

/// Wine prefixes to try when the game runs through Lutris or Bottles, best first.
const PREFIXES: [&str; 7] = [
    "Games/heroes-of-the-storm",
    "Games/battlenet",
    "Games/battle-net",
    "Games/blizzard",
    ".wine",
    ".local/share/bottles/bottles",
    ".var/app/com.usebottles.bottles/data/bottles/bottles",
];

const DOC_DIRS: [&str; 2] = ["Documents", "My Documents"];

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

    let native = std::env::temp_dir().join(TEMP_SUBDIR);
    if native.is_dir() {
        return native;
    }
    temps_of(named_prefix_users())
        .into_iter()
        .chain(wine_temp_roots(dirs::home_dir().as_deref()))
        .next()
        .unwrap_or(native)
}

/// Every `Accounts/<id>/<hero-id>/Replays/Multiplayer` the machine holds.
pub fn replay_dirs(cfg: &Config) -> Vec<PathBuf> {
    if let Some(dir) = &cfg.replay_dir {
        return vec![dir.clone()];
    }
    if let Ok(dir) = std::env::var("HOTS_REPLAY_DIR") {
        return vec![PathBuf::from(dir)];
    }

    let native = dirs::document_dir()
        .map(|docs| multiplayer_dirs(&docs.join(TEMP_SUBDIR).join("Accounts")))
        .unwrap_or_default();
    if !native.is_empty() {
        return native;
    }
    let named = replays_of(named_prefix_users());
    if !named.is_empty() {
        return named;
    }
    wine_replay_dirs(dirs::home_dir().as_deref())
}

pub fn is_battlelobby(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()) == Some(BATTLELOBBY_NAME)
}

pub fn is_replay(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("StormReplay"))
}

pub fn wine_temp_roots(home: Option<&Path>) -> Vec<PathBuf> {
    temps_of(wine_users(home))
}

pub fn wine_replay_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    replays_of(wine_users(home))
}

fn temps_of(users: Vec<PathBuf>) -> Vec<PathBuf> {
    users
        .into_iter()
        .map(|user| user.join("Temp").join(TEMP_SUBDIR))
        .filter(|dir| dir.is_dir())
        .collect()
}

fn replays_of(users: Vec<PathBuf>) -> Vec<PathBuf> {
    users
        .iter()
        .flat_map(|user| DOC_DIRS.map(|docs| user.join(docs)))
        .flat_map(|docs| multiplayer_dirs(&docs.join(TEMP_SUBDIR).join("Accounts")))
        .collect()
}

fn named_prefix_users() -> Vec<PathBuf> {
    users_in(
        std::env::var("WINEPREFIX")
            .into_iter()
            .map(PathBuf::from)
            .collect(),
    )
}

fn wine_users(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    users_in(PREFIXES.map(|dir| home.join(dir)).to_vec())
}

/// `drive_c/users/<name>` of every prefix, including the one a bottle nests one level down.
fn users_in(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let nested: Vec<PathBuf> = roots.iter().flat_map(|root| subdirs(root)).collect();
    roots
        .into_iter()
        .chain(nested)
        .flat_map(|prefix| subdirs(&prefix.join("drive_c").join("users")))
        .filter(|user| user.file_name().is_some_and(|name| name != "Public"))
        .collect()
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    out.sort();
    out
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
