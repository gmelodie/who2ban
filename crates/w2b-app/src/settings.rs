use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The shared database. Clearing it in settings keeps everything on this machine.
pub const DEFAULT_SERVER: &str = "https://w2b.gmelodie.com";

/// Who this machine is and where its replays go, which belongs to the machine
/// rather than to the shared database.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub battletag: String,
    pub server: Option<String>,
    pub username: String,
    pub password: String,
    pub replay_dir: Option<PathBuf>,
    pub temp_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            battletag: String::new(),
            server: Some(DEFAULT_SERVER.to_string()),
            username: String::new(),
            password: String::new(),
            replay_dir: None,
            temp_dir: None,
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        w2b_core::paths::data_dir().join("app.toml")
    }

    /// A server that nobody has logged into yet answers every request with 401.
    pub fn needs_login(&self) -> bool {
        self.shared_server().is_some() && (self.username.is_empty() || self.password.is_empty())
    }

    pub fn shared_server(&self) -> Option<&str> {
        self.server.as_deref().filter(|url| !url.is_empty())
    }

    pub fn load() -> Settings {
        std::fs::read_to_string(Settings::path())
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Settings::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }

    /// `config.toml` holds what an answer looks like, this file holds where to look.
    pub fn folders(&self) -> w2b_core::Config {
        w2b_core::Config {
            battletag: Some(self.battletag.clone()).filter(|tag| !tag.is_empty()),
            replay_dir: self.replay_dir.clone(),
            temp_dir: self.temp_dir.clone(),
            ..w2b_core::Config::load().unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_install_points_at_the_shared_database() {
        assert_eq!(Settings::default().server.as_deref(), Some(DEFAULT_SERVER));
    }

    /// A file that names no server is a file that wants the default, except when it
    /// says so with an empty string.
    #[test]
    fn an_empty_server_stays_empty() {
        let kept: Settings = toml::from_str("server = \"\"\nbattletag = \"Me#1\"\n").unwrap();
        assert_eq!(kept.server.as_deref(), Some(""));
        assert_eq!(kept.battletag, "Me#1");

        let silent: Settings = toml::from_str("battletag = \"Me#1\"\n").unwrap();
        assert_eq!(silent.server.as_deref(), Some(DEFAULT_SERVER));
    }
}
