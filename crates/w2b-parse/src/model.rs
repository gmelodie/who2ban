use serde::{Deserialize, Serialize};

/// Blizzard account handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toon {
    pub region: u8,
    pub realm: u8,
    pub id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameMode {
    QuickMatch,
    UnrankedDraft,
    HeroLeague,
    TeamLeague,
    StormLeague,
    Aram,
    Custom,
    Brawl,
    CoOp,
    Practice,
    Unknown,
}

impl GameMode {
    pub fn as_str(self) -> &'static str {
        match self {
            GameMode::QuickMatch => "QuickMatch",
            GameMode::UnrankedDraft => "UnrankedDraft",
            GameMode::HeroLeague => "HeroLeague",
            GameMode::TeamLeague => "TeamLeague",
            GameMode::StormLeague => "StormLeague",
            GameMode::Aram => "Aram",
            GameMode::Custom => "Custom",
            GameMode::Brawl => "Brawl",
            GameMode::CoOp => "CoOp",
            GameMode::Practice => "Practice",
            GameMode::Unknown => "Unknown",
        }
    }

    /// `m_ammId` of `replay.initData`, the matchmaking queue the game came from.
    pub fn from_amm_id(id: i64) -> GameMode {
        match id {
            50001 => GameMode::QuickMatch,
            50021 => GameMode::CoOp,
            50041 => GameMode::Practice,
            50051 => GameMode::Brawl,
            50061 => GameMode::UnrankedDraft,
            50071 => GameMode::HeroLeague,
            50081 => GameMode::TeamLeague,
            50091 => GameMode::StormLeague,
            50101 => GameMode::Aram,
            _ => GameMode::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyPlayer {
    pub battletag: String,
    /// 0 or 1. The first half of the slots is team 0.
    pub team: u8,
    pub slot: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lobby {
    pub players: Vec<LobbyPlayer>,
    /// 1 NA, 2 EU, 3 KR, 5 CN.
    pub region: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchPlayer {
    /// Always present. The battlelobby carries the discriminator, details does not.
    pub name: String,
    pub battletag: Option<String>,
    /// The name as the saving client spelled it, which is that client's language.
    pub hero: String,
    /// Attribute 4002: the same four characters in every language. `None` from a client
    /// too old to send one, which then falls back to matching on the spelling.
    #[serde(default)]
    pub hero_id: Option<String>,
    pub toon: Toon,
    pub team: u8,
    pub won: bool,
}

impl MatchPlayer {
    pub fn handle(&self) -> String {
        format!(
            "{}-Hero-{}-{}",
            self.toon.region, self.toon.realm, self.toon.id
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchRecord {
    pub players: Vec<MatchPlayer>,
    pub map: String,
    pub mode: GameMode,
    pub played_at: i64,
    pub build: u32,
    /// Language-independent battleground id, for the same reason as `hero_id`.
    #[serde(default)]
    pub map_id: Option<u64>,
    /// `m_randomValue` of `replay.initData`: the seed the server hands every client, so
    /// the same match carries it in all ten replays. A record from a client too old to
    /// send one falls back to the roster.
    #[serde(default)]
    pub game_id: Option<u64>,
}

impl MatchRecord {
    /// Who played what, on which side. Two replays of one match agree on all three even
    /// when they disagree on the clock, the file name, and who the winner was.
    pub fn roster(&self) -> String {
        let mut seats: Vec<String> = self
            .players
            .iter()
            .map(|p| format!("{}:{}:{}", p.handle(), p.hero, p.team))
            .collect();
        seats.sort();
        seats.join(",")
    }

    /// A replay cut short by a disconnect records nobody as the winner, and its player
    /// rows must not overwrite the rows of a replay that saw the end.
    pub fn decided(&self) -> bool {
        self.players.iter().any(|p| p.won) && !self.players.iter().all(|p| p.won)
    }
}
