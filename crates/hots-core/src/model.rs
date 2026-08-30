use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeroRow {
    pub hero: String,
    pub games: u32,
    pub wins: u32,
}

impl HeroRow {
    pub fn winrate(&self) -> Option<f64> {
        if self.games == 0 {
            return None;
        }
        Some(self.wins as f64 / self.games as f64)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftPlayer {
    pub battletag: String,
    pub slot: u8,
    pub team: u8,
    pub enemy: bool,
    pub heroes: Vec<HeroRow>,
    /// Games on record, which the shown heroes may not cover.
    pub games: u32,
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
