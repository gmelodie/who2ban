use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub battletag: Option<String>,
    pub hp_api_key: Option<String>,
    pub hp_base_url: String,
    pub hp_game_type: String,
    pub hp_ttl_days: u32,
    pub hp_history_days: u32,
    pub min_games_for_winrate: u32,
    pub max_heroes: usize,
    /// Count every game mode in the local aggregate, not Storm League alone.
    pub local_all_modes: bool,
    pub replay_dir: Option<PathBuf>,
    pub temp_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            battletag: None,
            hp_api_key: None,
            hp_base_url: "https://api.heroesprofile.com/api".to_string(),
            hp_game_type: "Storm League".to_string(),
            hp_ttl_days: 7,
            hp_history_days: 180,
            min_games_for_winrate: 3,
            max_heroes: 8,
            local_all_modes: true,
            replay_dir: None,
            temp_dir: None,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        paths::data_dir().join("config.toml")
    }

    pub fn load() -> Result<Config> {
        Config::load_from(&Config::path())
    }

    pub fn load_from(path: &std::path::Path) -> Result<Config> {
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(path)?;
        toml::from_str(&text).map_err(|e| Error::Config(e.to_string()))
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Config::path())
    }

    pub fn save_to(&self, path: &std::path::Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn ttl_secs(&self) -> i64 {
        self.hp_ttl_days as i64 * 86_400
    }

    pub fn api_key(&self) -> Result<&str> {
        self.hp_api_key
            .as_deref()
            .filter(|k| !k.is_empty())
            .ok_or_else(|| Error::Config("no Heroes Profile api key".into()))
    }
}
