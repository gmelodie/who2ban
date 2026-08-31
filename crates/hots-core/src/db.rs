use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use hots_parse::MatchRecord;
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Error, Result};
use crate::model::PlayerNote;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS matches(
    id        INTEGER PRIMARY KEY,
    game_id   TEXT UNIQUE,
    roster    TEXT NOT NULL,
    played_at INTEGER NOT NULL,
    map       TEXT NOT NULL,
    mode      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS matches_roster ON matches(roster);

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

CREATE TABLE IF NOT EXISTS replay_errors(
    replay_path TEXT PRIMARY KEY,
    error       TEXT NOT NULL,
    at          INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS player_notes(
    battletag  TEXT PRIMARY KEY COLLATE NOCASE,
    note       TEXT NOT NULL DEFAULT '',
    verdict    INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL
);
"#;

/// A database outlives every version of this program, so a step that cannot keep the
/// rows copies the file aside before it touches anything.
const SCHEMA_VERSION: i64 = 5;

/// The shape before 3 held no fingerprint and no player handle, neither of which can be
/// worked out from what it stored. 4 reshapes in place: it needs only the player rows,
/// which 3 already has, and a server holds the only copy of what its clients sent.
const REBUILT_BELOW: i64 = 3;

/// Two replays of one match are one row. The seed the server picked says so outright.
/// A record from a client that sends none is matched on its roster instead, close enough
/// in time that the same ten people cannot have drafted the same ten heroes twice. The
/// closest genuine repeat on record sat 36 minutes out, the furthest true duplicate 11.
const SAME_MATCH_WITHIN: i64 = 20 * 60;

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
             DROP TABLE IF EXISTS matches;
             DROP TABLE IF EXISTS hp_hero_stats;
             DROP TABLE IF EXISTS players;",
        )?;
    }

    if (REBUILT_BELOW..4).contains(&version) {
        // This one folds rows together, and a server holds the only copy of what its
        // clients sent it, so the file it started from stays on disk.
        let kept = keep_a_copy(path)?;
        tracing::info!(
            version,
            backup = kept.as_deref().unwrap_or("none"),
            "merging matches that the old shape stored more than once"
        );
        to_v4(conn)?;
    }

    conn.execute_batch(SCHEMA)?;
    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

/// Version 3 keyed a match on its save time, which is the clock of whoever saved it, so
/// the same match arrived once per player. This reshapes those rows rather than dropping
/// them, then folds the duplicates it can now see together.
fn to_v4(conn: &Connection) -> Result<()> {
    // Rebuilding a table two others point at, by the recipe the SQLite manual gives.
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let result = rebuild_matches(conn);
    conn.pragma_update(None, "foreign_keys", "ON")?;
    result
}

fn rebuild_matches(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE matches_new(
            id        INTEGER PRIMARY KEY,
            game_id   TEXT UNIQUE,
            roster    TEXT NOT NULL,
            played_at INTEGER NOT NULL,
            map       TEXT NOT NULL,
            mode      TEXT NOT NULL
         );
         INSERT INTO matches_new(id, game_id, roster, played_at, map, mode)
            SELECT id, NULL, '', played_at, map, mode FROM matches;
         DROP TABLE matches;
         ALTER TABLE matches_new RENAME TO matches;",
    )?;

    let rosters = stored_rosters(conn)?;
    for (id, roster, _) in &rosters {
        conn.execute(
            "UPDATE matches SET roster = ?1 WHERE id = ?2",
            params![roster, id],
        )?;
    }
    fold_duplicates(conn, &rosters)?;
    Ok(())
}

/// Every stored match as `(id, roster, decided)`, read back from the player rows.
fn stored_rosters(conn: &Connection) -> Result<Vec<(i64, String, bool)>> {
    let mut stmt = conn.prepare(
        "SELECT m.id, group_concat(seat), max(seats.won), min(seats.won)
         FROM matches m
         JOIN (SELECT match_id, handle || ':' || hero || ':' || team AS seat, won
               FROM match_players ORDER BY match_id, seat) seats ON seats.match_id = m.id
         GROUP BY m.id",
    )?;
    let rows = stmt.query_map([], |r| {
        let won_any: i64 = r.get(2)?;
        let won_all: i64 = r.get(3)?;
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            won_any == 1 && won_all == 0,
        ))
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// One match per roster per twenty minutes. The row that saw a winner outranks one cut
/// short by a disconnect, and its replay files move over rather than being forgotten.
fn fold_duplicates(conn: &Connection, rosters: &[(i64, String, bool)]) -> Result<()> {
    let mut by_roster: std::collections::HashMap<&str, Vec<(i64, bool, i64)>> =
        std::collections::HashMap::new();
    for (id, roster, decided) in rosters {
        let played_at: i64 =
            conn.query_row("SELECT played_at FROM matches WHERE id = ?1", [id], |r| {
                r.get(0)
            })?;
        by_roster
            .entry(roster.as_str())
            .or_default()
            .push((*id, *decided, played_at));
    }

    for group in by_roster.values_mut() {
        if group.len() < 2 {
            continue;
        }
        group.sort_by_key(|(id, _, played_at)| (*played_at, *id));
        let mut runs: Vec<Vec<(i64, bool, i64)>> = Vec::new();
        for row in group.iter() {
            match runs.last_mut() {
                Some(run) if row.2 - run[0].2 <= SAME_MATCH_WITHIN => run.push(*row),
                _ => runs.push(vec![*row]),
            }
        }
        for run in runs.iter().filter(|run| run.len() > 1) {
            let keeper = run
                .iter()
                .max_by_key(|(id, decided, _)| (*decided, -*id))
                .expect("a run holds at least two rows")
                .0;
            for (id, _, _) in run.iter().filter(|(id, _, _)| *id != keeper) {
                conn.execute(
                    "UPDATE OR IGNORE replay_files SET match_id = ?1 WHERE match_id = ?2",
                    params![keeper, id],
                )?;
                conn.execute("DELETE FROM matches WHERE id = ?1", [id])?;
                conn.execute("DELETE FROM match_players WHERE match_id = ?1", [id])?;
                conn.execute("DELETE FROM replay_files WHERE match_id = ?1", [id])?;
            }
        }
    }
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

    /// `foreign_keys` belongs to the connection, not to the file, so it is set on every open.
    fn from_conn(conn: Connection, path: Option<&Path>) -> Result<Db> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
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
    /// `None` when this replay is another view of a match already stored, which is the
    /// usual answer once a whole team submits the game they just played together.
    pub fn record_replay(&self, path: &str, replay: &MatchRecord) -> Result<Option<i64>> {
        let roster = replay.roster();
        let game_id = replay.game_id.map(|id| id.to_string());
        let mut conn = self.lock();
        let tx = conn.transaction()?;

        let existing = match &game_id {
            Some(id) => tx
                .query_row("SELECT id FROM matches WHERE game_id = ?1", [id], |r| {
                    r.get::<_, i64>(0)
                })
                .optional()?,
            None => None,
        };
        // Without a seed, the same ten people in the same seats on the same heroes, at
        // about the same time, is the same game. A row that already carries a different
        // seed is a different game and says so.
        let existing = match existing {
            Some(id) => Some(id),
            None => tx
                .query_row(
                    "SELECT id FROM matches
                     WHERE roster = ?1 AND abs(played_at - ?2) <= ?3
                       AND (?4 IS NULL OR game_id IS NULL OR game_id = ?4)
                     ORDER BY abs(played_at - ?2) LIMIT 1",
                    params![roster, replay.played_at, SAME_MATCH_WITHIN, game_id],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?,
        };

        let id = match existing {
            Some(id) => {
                // The row learns the seed from the first record that carries one, and a
                // replay that saw the end replaces the rows of one cut short.
                if game_id.is_some() {
                    tx.execute(
                        "UPDATE matches SET game_id = ?1 WHERE id = ?2 AND game_id IS NULL",
                        params![game_id, id],
                    )?;
                }
                if replay.decided() && !decided(&tx, id)? {
                    write_players(&tx, id, replay)?;
                }
                id
            }
            None => {
                tx.execute(
                    "INSERT INTO matches(game_id, roster, played_at, map, mode)
                     VALUES(?1, ?2, ?3, ?4, ?5)",
                    params![
                        game_id,
                        roster,
                        replay.played_at,
                        replay.map,
                        replay.mode.as_str()
                    ],
                )?;
                let id = tx.last_insert_rowid();
                write_players(&tx, id, replay)?;
                id
            }
        };

        tx.execute(
            "INSERT OR IGNORE INTO replay_files(path, match_id) VALUES(?1, ?2)",
            params![path, id],
        )?;
        tx.execute("DELETE FROM replay_errors WHERE replay_path = ?1", [path])?;
        tx.commit()?;
        Ok(existing.is_none().then_some(id))
    }

    /// What this database remembers about a player, which is nothing until someone says
    /// something. One credential guards the whole server, so a note is the group's note.
    pub fn note(&self, battletag: &str) -> Result<PlayerNote> {
        let conn = self.lock();
        let found = conn
            .query_row(
                "SELECT note, verdict FROM player_notes WHERE battletag = ?1",
                [battletag],
                |r| {
                    Ok(PlayerNote {
                        note: r.get(0)?,
                        verdict: r.get::<_, i64>(1)? as i8,
                    })
                },
            )
            .optional()?;
        Ok(found.unwrap_or_default())
    }

    /// A note that says nothing and judges nobody is not stored, so clearing one removes it.
    pub fn set_note(&self, battletag: &str, note: &PlayerNote) -> Result<()> {
        let conn = self.lock();
        if note.is_empty() {
            conn.execute("DELETE FROM player_notes WHERE battletag = ?1", [battletag])?;
            return Ok(());
        }
        conn.execute(
            "INSERT INTO player_notes(battletag, note, verdict, updated_at)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(battletag) DO UPDATE SET
                note = excluded.note,
                verdict = excluded.verdict,
                updated_at = excluded.updated_at",
            params![battletag, note.note, note.verdict as i64, now()],
        )?;
        Ok(())
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
                   WHERE (mp.battletag = ?1 COLLATE NOCASE
                          OR (mp.battletag IS NULL AND mp.name = ?3 COLLATE NOCASE))
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
                "SELECT coalesce(max(battletag), max(name)) FROM match_players
                 GROUP BY handle ORDER BY count(*) DESC, handle LIMIT 1",
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

/// A stored match with a winning side. A replay that ended in a disconnect stores ten
/// losers, and its rows must not stand in for a replay that saw who won.
fn decided(tx: &rusqlite::Transaction<'_>, id: i64) -> Result<bool> {
    let (won, total): (i64, i64) = tx.query_row(
        "SELECT coalesce(sum(won), 0), count(*) FROM match_players WHERE match_id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(won > 0 && won < total)
}

fn write_players(tx: &rusqlite::Transaction<'_>, id: i64, replay: &MatchRecord) -> Result<()> {
    for p in &replay.players {
        tx.execute(
            "INSERT OR REPLACE INTO match_players(match_id, handle, name, battletag, hero, team, won)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, p.handle(), p.name, p.battletag, p.hero, p.team, p.won as i64],
        )?;
    }
    Ok(())
}
