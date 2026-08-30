use std::path::Path;

use heroprotocol::Value;

use crate::model::{GameMode, Lobby, LobbyPlayer, MatchPlayer, MatchRecord, Toon};
use crate::{Error, Result};

const LOBBY_STREAM: &str = "replay.server.battlelobby";

pub fn replay(path: &Path) -> Result<MatchRecord> {
    replay_bytes(std::fs::read(path)?)
}

pub fn replay_bytes(bytes: Vec<u8>) -> Result<MatchRecord> {
    let raw = heroprotocol::Replay::new(heroprotocol::mpq::Archive::new(bytes)?)?;
    let details = raw.details()?;
    let entries = details
        .get("m_playerList")
        .and_then(array)
        .ok_or_else(|| malformed("details has no m_playerList"))?;

    let tags = raw
        .archive()
        .read_file(LOBBY_STREAM)?
        .map(|bytes| battletags(&bytes))
        .unwrap_or_default();
    let names: Vec<String> = entries.iter().filter_map(|e| text(e, "m_name")).collect();
    let tags = join_battletags(&names, tags)?;

    let players = entries
        .iter()
        .zip(tags)
        .map(|(entry, battletag)| player(entry, battletag))
        .collect::<Result<Vec<_>>>()?;

    Ok(MatchRecord {
        players,
        map: text(&details, "m_title").unwrap_or_default(),
        mode: mode_of(&raw),
        played_at: unix_time(int(&details, "m_timeUTC").unwrap_or(0)),
        build: raw.base_build(),
    })
}

fn player(entry: &Value, battletag: String) -> Result<MatchPlayer> {
    let toon = entry
        .get("m_toon")
        .ok_or_else(|| malformed("player has no m_toon"))?;

    Ok(MatchPlayer {
        battletag,
        hero: text(entry, "m_hero").unwrap_or_default(),
        toon: Toon {
            region: int(toon, "m_region").unwrap_or(0) as u8,
            realm: int(toon, "m_realm").unwrap_or(0) as u8,
            id: int(toon, "m_id").unwrap_or(0) as u64,
        },
        team: int(entry, "m_teamId").unwrap_or(0) as u8,
        won: int(entry, "m_result") == Some(1),
    })
}

/// The lobby lists its players in slot order, which is the order of `m_playerList`.
fn join_battletags(names: &[String], tags: Vec<String>) -> Result<Vec<String>> {
    if tags.len() != names.len() {
        return Err(malformed(&format!(
            "{} battletags for {} players",
            tags.len(),
            names.len()
        )));
    }
    if names
        .iter()
        .zip(&tags)
        .all(|(name, tag)| tag.starts_with(name.as_str()) && tag[name.len()..].starts_with('#'))
    {
        return Ok(tags);
    }
    names.iter().map(|name| by_name(name, &tags)).collect()
}

fn by_name(name: &str, tags: &[String]) -> Result<String> {
    let mut hits = tags
        .iter()
        .filter(|tag| tag.starts_with(name) && tag[name.len()..].starts_with('#'));
    match (hits.next(), hits.next()) {
        (Some(tag), None) => Ok(tag.clone()),
        _ => Err(malformed(&format!("no single battletag for {name}"))),
    }
}

fn mode_of(raw: &heroprotocol::Replay) -> GameMode {
    let Ok(init) = raw.initdata() else {
        return GameMode::Unknown;
    };
    let Some(options) = init
        .get("m_syncLobbyState")
        .and_then(|s| s.get("m_gameDescription"))
        .and_then(|d| d.get("m_gameOptions"))
    else {
        return GameMode::Unknown;
    };
    if options.get("m_amm") == Some(&Value::Bool(false)) {
        return GameMode::Custom;
    }
    int(options, "m_ammId").map_or(GameMode::Unknown, GameMode::from_amm_id)
}

pub const LOBBY_SIZE: usize = 10;

/// A wrong count means the scan found noise, and half of noise is a wrong enemy team.
pub fn battlelobby(bytes: &[u8]) -> Result<Lobby> {
    let tags = battletags(bytes);
    if tags.len() != LOBBY_SIZE {
        return Err(malformed(&format!(
            "{} battletags in the lobby, expected {LOBBY_SIZE}",
            tags.len()
        )));
    }

    let half = tags.len() / 2;
    let players = tags
        .into_iter()
        .enumerate()
        .map(|(i, battletag)| LobbyPlayer {
            battletag,
            team: (i >= half) as u8,
            slot: i as u8,
        })
        .collect();

    Ok(Lobby {
        players,
        region: region_of(bytes),
    })
}

/// The undocumented stream yields its battletags to a scan for a length that agrees with the string behind it.
fn battletags(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        match battletag_at(bytes, i) {
            Some(tag) => {
                i += 1 + tag.len();
                out.push(tag);
            }
            None => i += 1,
        }
    }
    out
}

fn battletag_at(bytes: &[u8], at: usize) -> Option<String> {
    let header = bytes[at];
    if header & 1 == 0 {
        return None;
    }
    let len = (header >> 1) as usize;
    if !(5..=48).contains(&len) {
        return None;
    }

    let end = at + 1 + len;
    let text = std::str::from_utf8(bytes.get(at + 1..end)?).ok()?;
    if bytes.get(end).is_some_and(u8::is_ascii_digit) {
        return None;
    }

    let (name, number) = text.split_once('#')?;
    let named = (2..=24).contains(&name.chars().count())
        && name
            .chars()
            .all(|c| !c.is_control() && c != ':' && c != '#');
    let numbered = (3..=8).contains(&number.len()) && number.bytes().all(|b| b.is_ascii_digit());
    (named && numbered).then(|| text.to_string())
}

/// The map dependencies name the gateway the game ran on.
fn region_of(bytes: &[u8]) -> u8 {
    for window in bytes.windows(8) {
        if !matches!(&window[..4], b"s2ma" | b"s2mh" | b"s2mv") || window[4..6] != [0, 0] {
            continue;
        }
        match &window[6..8] {
            b"US" => return 1,
            b"EU" => return 2,
            b"KR" => return 3,
            b"CN" => return 5,
            _ => continue,
        }
    }
    0
}

/// Windows file time, in units of 100 ns since 1601.
fn unix_time(filetime: i64) -> i64 {
    filetime / 10_000_000 - 11_644_473_600
}

fn malformed(what: &str) -> Error {
    Error::Malformed(what.to_string())
}

fn array(v: &Value) -> Option<&[Value]> {
    match v {
        Value::Array(items) => Some(items),
        _ => None,
    }
}

fn int(v: &Value, name: &str) -> Option<i64> {
    v.get(name).and_then(Value::as_i64)
}

fn text(v: &Value, name: &str) -> Option<String> {
    let bytes = v.get(name).and_then(Value::as_bytes)?;
    Some(String::from_utf8_lossy(bytes).into_owned())
}
