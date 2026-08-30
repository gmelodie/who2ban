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
    pub battletag: String,
    pub hero: String,
    pub toon: Toon,
    pub team: u8,
    pub won: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchRecord {
    pub players: Vec<MatchPlayer>,
    pub map: String,
    pub mode: GameMode,
    pub played_at: i64,
    pub build: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    None,
    Local,
    Hp,
    Both,
}

impl Source {
    pub fn merge(self, other: Source) -> Source {
        match (self, other) {
            (Source::None, x) | (x, Source::None) => x,
            (a, b) if a == b => a,
            _ => Source::Both,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeroRow {
    pub hero: String,
    pub games: u32,
    pub wins: u32,
    pub local_games: u32,
    pub local_wins: u32,
    pub hp_games: u32,
    pub hp_wins: u32,
    pub source: Source,
}

impl HeroRow {
    pub fn winrate(&self) -> Option<f64> {
        if self.games == 0 {
            return None;
        }
        Some(self.wins as f64 / self.games as f64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FetchState {
    Fresh,
    Stale,
    Pending,
    Missing,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftPlayer {
    pub battletag: String,
    pub region: u8,
    pub slot: u8,
    pub team: u8,
    pub enemy: bool,
    pub mmr: Option<f64>,
    pub heroes: Vec<HeroRow>,
    pub local_games: u32,
    pub hp_state: FetchState,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub region: u8,
    /// Team of the configured battletag, `None` when it is not in the lobby.
    pub my_team: Option<u8>,
    pub players: Vec<DraftPlayer>,
}

impl Draft {
    pub fn enemies(&self) -> impl Iterator<Item = &DraftPlayer> {
        self.players.iter().filter(|p| p.enemy)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestProgress {
    pub done: u32,
    pub total: u32,
    pub failed: u32,
}
