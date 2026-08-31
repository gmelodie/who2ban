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
    pub hero: String,
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
}

impl MatchRecord {
    /// The same game from ten different machines, under ten different file names, since
    /// the client names a replay by the local clock of whoever played it.
    pub fn fingerprint(&self) -> String {
        let mut handles: Vec<String> = self.players.iter().map(MatchPlayer::handle).collect();
        handles.sort();
        format!("{}/{}", self.played_at, handles.join(","))
    }
}
