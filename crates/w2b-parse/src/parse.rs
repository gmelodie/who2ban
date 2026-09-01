use std::path::Path;

use heroprotocol::{Protocol, Value};

use crate::model::{GameMode, Lobby, LobbyPlayer, MatchPlayer, MatchRecord, Toon};
use crate::{Error, Result};

const LOBBY_STREAM: &str = "replay.server.battlelobby";

pub fn replay(path: &Path) -> Result<MatchRecord> {
    replay_bytes(std::fs::read(path)?)
}

/// The battlelobby of a finished match, which is the same stream a live lobby writes.
pub fn lobby_stream(bytes: Vec<u8>) -> Result<Vec<u8>> {
    let archive = heroprotocol::mpq::Archive::new(bytes)?;
    stream(&archive, LOBBY_STREAM)
}

pub fn replay_bytes(bytes: Vec<u8>) -> Result<MatchRecord> {
    let archive = heroprotocol::mpq::Archive::new(bytes)?;
    let base = base_build(&archive)?;
    let protocol = protocol_for(base);

    let details = protocol.decode_replay_details(&stream(&archive, "replay.details")?)?;
    let entries = details
        .get("m_playerList")
        .and_then(array)
        .ok_or_else(|| malformed("details has no m_playerList"))?;

    let tags = archive
        .read_file(LOBBY_STREAM)?
        .map(|bytes| battletags(&bytes))
        .unwrap_or_default();
    let names: Vec<String> = entries.iter().filter_map(|e| text(e, "m_name")).collect();
    let tags = join_battletags(&names, tags);

    // The names in `details` are in the language of whoever saved the file, so the same
    // hero arrives as Deathwing from one client and Asa da Morte from the next. These
    // codes are game data, not text, and every client writes the same ones.
    let units = hero_units(protocol, &archive);
    let players = entries
        .iter()
        .zip(tags)
        .enumerate()
        .map(|(i, (entry, battletag))| player(entry, battletag, units.get(i).cloned().flatten()))
        .collect::<Result<Vec<_>>>()?;

    let init = init_data(protocol, &archive);

    Ok(MatchRecord {
        players,
        map: text(&details, "m_title").unwrap_or_default(),
        mode: init.as_ref().map_or(GameMode::Unknown, mode_of),
        played_at: unix_time(int(&details, "m_timeUTC").unwrap_or(0)),
        build: base,
        game_id: init.as_ref().and_then(game_id),
        map_id: init.as_ref().and_then(map_id),
    })
}

/// A build past the newest table decodes with the nearest older one, because the two
/// streams this reads are self-describing.
pub fn protocol_for(base: u32) -> &'static Protocol {
    heroprotocol::versions::build_or_older(base)
        .map(|(_, protocol)| protocol)
        .unwrap_or_else(heroprotocol::latest)
}

pub fn is_exact_build(base: u32) -> bool {
    heroprotocol::build(base).is_some()
}

fn base_build(archive: &heroprotocol::mpq::Archive) -> Result<u32> {
    let user_data = archive
        .user_data()
        .ok_or_else(|| malformed("replay has no user data header"))?;
    let header = heroprotocol::latest().decode_replay_header(&user_data.content)?;
    header
        .get("m_version")
        .and_then(|v| v.get("m_baseBuild"))
        .and_then(Value::as_i64)
        .map(|build| build as u32)
        .ok_or_else(|| malformed("header has no m_baseBuild"))
}

fn stream(archive: &heroprotocol::mpq::Archive, name: &str) -> Result<Vec<u8>> {
    archive
        .read_file(name)?
        .ok_or_else(|| malformed(&format!("replay has no {name}")))
}

/// `m_mapFileSyncChecksum`: the same battleground checksums the same for every client of
/// one build, which is enough to know that two spellings are one place.
fn map_id(init: &Value) -> Option<u64> {
    init.get("m_syncLobbyState")
        .and_then(|s| s.get("m_gameDescription"))
        .and_then(|d| d.get("m_mapFileSyncChecksum"))
        .and_then(Value::as_i64)
        .map(|sum| sum as u64)
}

/// The hero each player started the match as, named the way the game data names it:
/// `HeroWitchDoctor` whatever language the client that saved the file was set to. Taken
/// from the tracker stream because the lobby attribute that also carries it is filled in
/// for the local player alone in some builds, and one hero out of ten is worse than none.
/// Keyed by `m_controlPlayerId`, which is the `m_playerList` slot plus one.
fn hero_units(
    protocol: &'static Protocol,
    archive: &heroprotocol::mpq::Archive,
) -> Vec<Option<String>> {
    let Ok(data) = stream(archive, "replay.tracker.events") else {
        return Vec::new();
    };

    let mut born: Vec<Option<String>> = vec![None; LOBBY_SIZE];
    let mut left = LOBBY_SIZE;
    for event in protocol.decode_replay_tracker_events(&data) {
        let Ok(event) = event else { continue };
        if event_name(&event).as_deref() != Some(UNIT_BORN) {
            continue;
        }
        let Some(unit) = text(&event, "m_unitTypeName") else {
            continue;
        };
        // A hero and nothing else: minions and structures are born in this stream too.
        if !unit.starts_with("Hero") || unit.len() <= 4 {
            continue;
        }
        let Some(slot) = int(&event, "m_controlPlayerId")
            .filter(|owner| (1..=LOBBY_SIZE as i64).contains(owner))
            .map(|owner| owner as usize - 1)
        else {
            continue;
        };
        // The first body a player is born in is the one they drafted, which is the
        // difference that matters to the heroes that change shape mid-fight.
        if born[slot].is_none() {
            born[slot] = Some(unit);
            left -= 1;
            if left == 0 {
                break;
            }
        }
    }
    born
}

const UNIT_BORN: &str = "NNet.Replay.Tracker.SUnitBornEvent";

fn event_name(event: &Value) -> Option<String> {
    match event.get("_event")? {
        Value::Str(name) => Some(name.to_string()),
        other => text_of(other),
    }
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::Blob(bytes) => Some(String::from_utf8_lossy(bytes).trim().to_string()),
        _ => None,
    }
}

fn player(
    entry: &Value,
    battletag: Option<String>,
    hero_id: Option<String>,
) -> Result<MatchPlayer> {
    let toon = entry
        .get("m_toon")
        .ok_or_else(|| malformed("player has no m_toon"))?;

    Ok(MatchPlayer {
        name: text(entry, "m_name").unwrap_or_default(),
        battletag,
        hero: text(entry, "m_hero").unwrap_or_default(),
        hero_id,
        toon: Toon {
            region: int(toon, "m_region").unwrap_or(0) as u8,
            realm: int(toon, "m_realm").unwrap_or(0) as u8,
            id: int(toon, "m_id").unwrap_or(0) as u64,
        },
        team: int(entry, "m_teamId").unwrap_or(0) as u8,
        won: int(entry, "m_result") == Some(1),
    })
}

/// A scan that comes up short costs the discriminator, never the replay: the short
/// name and the toon of `m_playerList` identify a player well enough on their own.
fn join_battletags(names: &[String], tags: Vec<String>) -> Vec<Option<String>> {
    if tags.len() == names.len()
        && names
            .iter()
            .zip(&tags)
            .all(|(name, tag)| tags_name(tag) == name)
    {
        return tags.into_iter().map(Some).collect();
    }
    names.iter().map(|name| by_name(name, &tags)).collect()
}

fn tags_name(tag: &str) -> &str {
    tag.split_once('#').map_or(tag, |(name, _)| name)
}

fn by_name(name: &str, tags: &[String]) -> Option<String> {
    let mut hits = tags.iter().filter(|tag| tags_name(tag) == name);
    match (hits.next(), hits.next()) {
        (Some(tag), None) => Some(tag.clone()),
        _ => None,
    }
}

fn init_data(protocol: &Protocol, archive: &heroprotocol::mpq::Archive) -> Option<Value> {
    stream(archive, "replay.initData")
        .and_then(|bytes| Ok(protocol.decode_replay_initdata(&bytes)?))
        .ok()
}

/// The seed the server picks for the match and sends to every client, which makes it the
/// one field ten replays of one game agree on exactly.
fn game_id(init: &Value) -> Option<u64> {
    init.get("m_syncLobbyState")
        .and_then(|s| s.get("m_gameDescription"))
        .and_then(|d| d.get("m_randomValue"))
        .and_then(Value::as_i64)
        .map(|seed| seed as u64)
}

fn mode_of(init: &Value) -> GameMode {
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

/// Battletags in file order, which is slot order. The stream is undocumented, so the
/// scan anchors on the `#` and keeps a name whose length the byte in front agrees with.
fn battletags(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut taken = 0;
    for (hash, _) in bytes.iter().enumerate().filter(|(_, b)| **b == b'#') {
        if hash < taken {
            continue;
        }
        if let Some((start, tag)) = tag_around(bytes, hash) {
            taken = start + tag.len();
            out.push(tag);
        }
    }
    out
}

/// The length is written as a vint, whose encoding has moved between builds.
fn encodes(header: u8, len: usize) -> bool {
    let len = len as u8;
    header == (len << 1) | 1 || header == len << 1 || header == len
}

fn tag_around(bytes: &[u8], hash: usize) -> Option<(usize, String)> {
    let digits = bytes[hash + 1..]
        .iter()
        .take(9)
        .take_while(|b| b.is_ascii_digit())
        .count();
    if !(3..=8).contains(&digits) {
        return None;
    }
    let end = hash + 1 + digits;

    for name_len in 2..=24usize {
        let start = hash.checked_sub(name_len)?;
        if start == 0 {
            break;
        }
        if !encodes(bytes[start - 1], end - start) {
            continue;
        }
        let text = std::str::from_utf8(bytes.get(start..end)?).ok()?;
        let (name, _) = text.split_once('#')?;
        if name
            .chars()
            .all(|c| !c.is_control() && c != ':' && c != '#')
        {
            return Some((start, text.to_string()));
        }
    }
    None
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
