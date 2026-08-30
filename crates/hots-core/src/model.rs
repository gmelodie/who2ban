use serde::{Deserialize, Serialize};

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
