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

/// What the people sharing this database have said about a player. The server is guarded
/// by one credential, so there is nobody to attribute a note to: it belongs to the group.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerNote {
    #[serde(default)]
    pub note: String,
    /// 1 thumbs up, -1 thumbs down, 0 neither.
    #[serde(default)]
    pub verdict: i8,
}

impl PlayerNote {
    pub fn is_empty(&self) -> bool {
        self.note.trim().is_empty() && self.verdict == 0
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
    /// Absent from a server too old to send one, which is not the same as an empty note.
    #[serde(default)]
    pub note: PlayerNote,
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

    /// The other side of the same lobby. A teammate today is an opponent later, so their
    /// pool is worth the same look as an enemy's. Everyone is an ally when `my_team` is
    /// `None`, which is why the caller shows one flat group in that case.
    pub fn allies(&self) -> impl Iterator<Item = &DraftPlayer> {
        self.players.iter().filter(|p| !p.enemy)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestProgress {
    pub done: u32,
    pub total: u32,
    pub failed: u32,
}
