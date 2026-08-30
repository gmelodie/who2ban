use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use hots_parse::MatchRecord;
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::Result;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS players(
    battletag     TEXT PRIMARY KEY,
    region        INTEGER NOT NULL DEFAULT 0,
    hp_fetched_at INTEGER,
    hp_mmr        REAL
);

CREATE TABLE IF NOT EXISTS matches(
    id          INTEGER PRIMARY KEY,
    replay_path TEXT NOT NULL UNIQUE,
    played_at   INTEGER NOT NULL,
    map         TEXT NOT NULL,
    mode        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS match_players(
    match_id  INTEGER NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    battletag TEXT NOT NULL,
    hero      TEXT NOT NULL,
    team      INTEGER NOT NULL,
    won       INTEGER NOT NULL,
    PRIMARY KEY(match_id, battletag)
);

CREATE INDEX IF NOT EXISTS match_players_battletag ON match_players(battletag);

CREATE TABLE IF NOT EXISTS hp_hero_stats(
    battletag  TEXT NOT NULL,
    hero       TEXT NOT NULL,
    game_type  TEXT NOT NULL,
    games      INTEGER NOT NULL,
    wins       INTEGER NOT NULL,
    mmr        REAL,
    fetched_at INTEGER NOT NULL,
    PRIMARY KEY(battletag, hero, game_type)
);

CREATE TABLE IF NOT EXISTS replay_errors(
    replay_path TEXT PRIMARY KEY,
    error       TEXT NOT NULL,
    at          INTEGER NOT NULL
);
"#;

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

#[derive(Debug, Clone)]
pub struct HpHero {
    pub hero: String,
    pub games: u32,
    pub wins: u32,
    pub mmr: Option<f64>,
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Db::from_conn(Connection::open(path)?)
    }

    pub fn open_memory() -> Result<Db> {
        Db::from_conn(Connection::open_in_memory()?)
    }

    fn from_conn(conn: Connection) -> Result<Db> {
        conn.execute_batch(SCHEMA)?;
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn upsert_player(&self, battletag: &str, region: u8) -> Result<()> {
        self.lock().execute(
            "INSERT INTO players(battletag, region) VALUES(?1, ?2)
             ON CONFLICT(battletag) DO UPDATE SET region = coalesce(nullif(excluded.region, 0), region)",
            params![battletag, region],
        )?;
        Ok(())
    }

    pub fn known_replays(&self) -> Result<HashSet<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare("SELECT replay_path FROM matches")?;
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

    /// Insert a parsed replay. Returns `None` when the path is already stored.
    pub fn record_replay(&self, path: &str, replay: &MatchRecord) -> Result<Option<i64>> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO matches(replay_path, played_at, map, mode)
             VALUES(?1, ?2, ?3, ?4)",
            params![path, replay.played_at, replay.map, replay.mode.as_str()],
        )?;
        if inserted == 0 {
            return Ok(None);
        }
        let id = tx.last_insert_rowid();
        for p in &replay.players {
            tx.execute(
                "INSERT OR REPLACE INTO match_players(match_id, battletag, hero, team, won)
                 VALUES(?1, ?2, ?3, ?4, ?5)",
                params![id, p.battletag, p.hero, p.team, p.won as i64],
            )?;
            tx.execute(
                "INSERT INTO players(battletag, region) VALUES(?1, ?2)
                 ON CONFLICT(battletag) DO UPDATE SET region = coalesce(nullif(excluded.region, 0), region)",
                params![p.battletag, p.toon.region],
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

    /// Storm League absorbed Hero League and Team League, so the old queues count as ranked.
    pub fn local_heroes(&self, battletag: &str, all_modes: bool) -> Result<Vec<LocalHero>> {
        let conn = self.lock();
        let sql = "SELECT mp.hero, count(*), sum(mp.won)
                   FROM match_players mp JOIN matches m ON m.id = mp.match_id
                   WHERE mp.battletag = ?1
                     AND (?2 OR m.mode IN ('StormLeague', 'HeroLeague', 'TeamLeague'))
                   GROUP BY mp.hero ORDER BY count(*) DESC";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![battletag, all_modes], |r| {
            Ok(LocalHero {
                hero: r.get(0)?,
                games: r.get::<_, i64>(1)? as u32,
                wins: r.get::<_, i64>(2)? as u32,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn hp_heroes(&self, battletag: &str, game_type: &str) -> Result<Vec<HpHero>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT hero, games, wins, mmr FROM hp_hero_stats
             WHERE battletag = ?1 AND game_type = ?2 ORDER BY games DESC",
        )?;
        let rows = stmt.query_map(params![battletag, game_type], |r| {
            Ok(HpHero {
                hero: r.get(0)?,
                games: r.get::<_, i64>(1)? as u32,
                wins: r.get::<_, i64>(2)? as u32,
                mmr: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn hp_fetched_at(&self, battletag: &str) -> Result<Option<i64>> {
        let conn = self.lock();
        let at = conn
            .query_row(
                "SELECT hp_fetched_at FROM players WHERE battletag = ?1",
                [battletag],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?;
        Ok(at.flatten())
    }

    pub fn hp_mmr(&self, battletag: &str) -> Result<Option<f64>> {
        let conn = self.lock();
        let mmr = conn
            .query_row(
                "SELECT hp_mmr FROM players WHERE battletag = ?1",
                [battletag],
                |r| r.get::<_, Option<f64>>(0),
            )
            .optional()?;
        Ok(mmr.flatten())
    }

    pub fn replace_hp_heroes(
        &self,
        battletag: &str,
        game_type: &str,
        heroes: &[HpHero],
        mmr: Option<f64>,
    ) -> Result<()> {
        let stamp = now();
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM hp_hero_stats WHERE battletag = ?1 AND game_type = ?2",
            params![battletag, game_type],
        )?;
        for h in heroes {
            tx.execute(
                "INSERT INTO hp_hero_stats(battletag, hero, game_type, games, wins, mmr, fetched_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![battletag, h.hero, game_type, h.games, h.wins, h.mmr, stamp],
            )?;
        }
        tx.execute(
            "INSERT INTO players(battletag, region, hp_fetched_at, hp_mmr) VALUES(?1, 0, ?2, ?3)
             ON CONFLICT(battletag) DO UPDATE SET hp_fetched_at = ?2, hp_mmr = coalesce(?3, hp_mmr)",
            params![battletag, stamp, mmr],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The battletag seen in the most stored matches, used when config has none.
    pub fn likely_self(&self) -> Result<Option<String>> {
        let conn = self.lock();
        let tag = conn
            .query_row(
                "SELECT battletag FROM match_players
                 GROUP BY battletag ORDER BY count(*) DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(tag)
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
