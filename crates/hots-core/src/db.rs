use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use hots_parse::MatchRecord;
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Error, Result};

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS matches(
    id          INTEGER PRIMARY KEY,
    fingerprint TEXT NOT NULL UNIQUE,
    played_at   INTEGER NOT NULL,
    map         TEXT NOT NULL,
    mode        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS replay_files(
    path     TEXT PRIMARY KEY,
    match_id INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS match_players(
    match_id  INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    handle    TEXT NOT NULL,
    name      TEXT NOT NULL,
    battletag TEXT,
    hero      TEXT NOT NULL,
    team      INTEGER NOT NULL,
    won       INTEGER NOT NULL,
    PRIMARY KEY(match_id, handle)
);

CREATE INDEX IF NOT EXISTS match_players_battletag ON match_players(battletag);
CREATE INDEX IF NOT EXISTS match_players_name ON match_players(name);

DROP TABLE IF EXISTS hp_hero_stats;
DROP TABLE IF EXISTS players;

CREATE TABLE IF NOT EXISTS replay_errors(
    replay_path TEXT PRIMARY KEY,
    error       TEXT NOT NULL,
    at          INTEGER NOT NULL
);
"#;

/// A database outlives every version of this program, so a step that cannot keep the
/// rows copies the file aside before it touches anything.
const SCHEMA_VERSION: i64 = 3;

/// The shape before 3 held no fingerprint and no player handle, neither of which can be
/// worked out from what it stored.
const REBUILT_BELOW: i64 = 3;

fn migrate(conn: &Connection, path: Option<&Path>) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if version > SCHEMA_VERSION {
        return Err(Error::Other(format!(
            "this database is version {version}, which is newer than this build understands"
        )));
    }

    let fresh: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'matches'",
        [],
        |r| r.get(0),
    )?;
    if fresh == 0 {
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        return Ok(());
    }

    if version < REBUILT_BELOW {
        let kept = keep_a_copy(path)?;
        tracing::warn!(
            version,
            backup = kept.as_deref().unwrap_or("none"),
            "rebuilding a database older than this build, the copy stays"
        );
        conn.execute_batch(
            "DROP TABLE IF EXISTS match_players;
             DROP TABLE IF EXISTS replay_files;
             DROP TABLE IF EXISTS matches;",
        )?;
    }

    conn.execute_batch(SCHEMA)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

/// Nothing here removes a database. A step that has to reshape one leaves the old file behind.
fn keep_a_copy(path: Option<&Path>) -> Result<Option<String>> {
    let Some(path) = path.filter(|path| path.exists()) else {
        return Ok(None);
    };
    let backup = path.with_extension(format!("before-v{SCHEMA_VERSION}.db"));
    std::fs::copy(path, &backup)?;
    Ok(Some(backup.display().to_string()))
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct LocalHero {
    pub hero: String,
    pub games: u32,
    pub wins: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MatchSummary {
    pub played_at: i64,
    pub map: String,
    pub mode: String,
    pub players: u32,
    pub files: u32,
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Db::from_conn(Connection::open(path)?, Some(path))
    }

    pub fn open_memory() -> Result<Db> {
        Db::from_conn(Connection::open_in_memory()?, None)
    }

    fn from_conn(conn: Connection, path: Option<&Path>) -> Result<Db> {
        migrate(&conn, path)?;
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn known_replays(&self) -> Result<HashSet<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT path FROM replay_files")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut set = HashSet::new();
        for row in rows {
            set.insert(row?);
        }
        let mut stmt = conn.prepare("SELECT replay_path FROM replay_errors")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for row in rows {
            set.insert(row?);
        }
        Ok(set)
    }

    /// Store a parsed replay. `None` when this match already came from another file,
    /// which is the normal case when two people in one game both send theirs.
    pub fn record_replay(&self, path: &str, replay: &MatchRecord) -> Result<Option<i64>> {
        let fingerprint = replay.fingerprint();
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO matches(fingerprint, played_at, map, mode)
             VALUES(?1, ?2, ?3, ?4)",
            params![
                fingerprint,
                replay.played_at,
                replay.map,
                replay.mode.as_str()
            ],
        )?;
        let id: i64 = tx.query_row(
            "SELECT id FROM matches WHERE fingerprint = ?1",
            [&fingerprint],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO replay_files(path, match_id) VALUES(?1, ?2)",
            params![path, id],
        )?;
        if inserted == 0 {
            tx.execute("DELETE FROM replay_errors WHERE replay_path = ?1", [path])?;
            tx.commit()?;
            return Ok(None);
        }
        for p in &replay.players {
            tx.execute(
                "INSERT OR REPLACE INTO match_players(match_id, handle, name, battletag, hero, team, won)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, p.handle(), p.name, p.battletag, p.hero, p.team, p.won as i64],
            )?;
        }
        tx.execute("DELETE FROM replay_errors WHERE replay_path = ?1", [path])?;
        tx.commit()?;
        Ok(Some(id))
    }

    pub fn record_replay_error(&self, path: &str, error: &str) -> Result<()> {
        self.lock().execute(
            "INSERT OR REPLACE INTO replay_errors(replay_path, error, at) VALUES(?1, ?2, ?3)",
            params![path, error, now()],
        )?;
        Ok(())
    }

    /// A replay whose battlelobby would not scan has the short name and nothing else,
    /// so the name stands in. Storm League absorbed Hero League and Team League.
    pub fn local_heroes(&self, battletag: &str, all_modes: bool) -> Result<Vec<LocalHero>> {
        let name = battletag.split_once('#').map_or(battletag, |(n, _)| n);
        let conn = self.lock();
        let sql = "SELECT mp.hero, count(*), sum(mp.won)
                   FROM match_players mp JOIN matches m ON m.id = mp.match_id
                   WHERE (mp.battletag = ?1 OR (mp.battletag IS NULL AND mp.name = ?3))
                     AND (?2 OR m.mode IN ('StormLeague', 'HeroLeague', 'TeamLeague'))
                   GROUP BY mp.hero ORDER BY count(*) DESC";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![battletag, all_modes, name], |r| {
            Ok(LocalHero {
                hero: r.get(0)?,
                games: r.get::<_, i64>(1)? as u32,
                wins: r.get::<_, i64>(2)? as u32,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    /// The player seen in the most stored matches, used when nobody says who they are.
    pub fn likely_self(&self) -> Result<Option<String>> {
        let conn = self.lock();
        let tag = conn
            .query_row(
                "SELECT coalesce(battletag, name) FROM match_players
                 GROUP BY handle ORDER BY count(*) DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(tag)
    }

    pub fn recent_matches(&self, limit: u32) -> Result<Vec<MatchSummary>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT m.played_at, m.map, m.mode,
                    (SELECT count(*) FROM match_players p WHERE p.match_id = m.id),
                    (SELECT count(*) FROM replay_files f WHERE f.match_id = m.id)
             FROM matches m ORDER BY m.played_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            Ok(MatchSummary {
                played_at: r.get(0)?,
                map: r.get(1)?,
                mode: r.get(2)?,
                players: r.get::<_, i64>(3)? as u32,
                files: r.get::<_, i64>(4)? as u32,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn file_count(&self) -> Result<u32> {
        let conn = self.lock();
        let n = conn.query_row("SELECT count(*) FROM replay_files", [], |r| {
            r.get::<_, i64>(0)
        })?;
        Ok(n as u32)
    }

    pub fn match_count(&self) -> Result<u32> {
        let conn = self.lock();
        let n = conn.query_row("SELECT count(*) FROM matches", [], |r| r.get::<_, i64>(0))?;
        Ok(n as u32)
    }

    pub fn error_count(&self) -> Result<u32> {
        let conn = self.lock();
        let n = conn.query_row("SELECT count(*) FROM replay_errors", [], |r| {
            r.get::<_, i64>(0)
        })?;
        Ok(n as u32)
    }
}
