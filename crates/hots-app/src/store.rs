use hots_core::{Config, Db, Draft, Lobby, MatchRecord, draft, paths};

use crate::settings::Settings;

/// A file beside the app when it stands alone, one shared database when it is pointed at a server.
pub enum Store {
    Local(Box<Db>),
    Server(String),
}

pub struct Stored {
    pub stored: bool,
    pub matches: u32,
}

impl Store {
    pub fn open(settings: &Settings) -> hots_core::Result<Store> {
        match settings.server.as_deref().filter(|url| !url.is_empty()) {
            Some(url) => Ok(Store::Server(url.trim_end_matches('/').to_string())),
            None => Ok(Store::Local(Box::new(Db::open(&paths::db_path())?))),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Store::Local(_) => paths::db_path().display().to_string(),
            Store::Server(url) => url.clone(),
        }
    }

    pub fn known(&self) -> Result<Vec<String>, String> {
        match self {
            Store::Local(db) => db
                .known_replays()
                .map(|set| set.into_iter().collect())
                .map_err(|e| e.to_string()),
            Store::Server(url) => get(&format!("{url}/api/matches/known")),
        }
    }

    pub fn submit(&self, key: &str, record: &MatchRecord) -> Result<Stored, String> {
        match self {
            Store::Local(db) => {
                let stored = db.record_replay(key, record).map_err(|e| e.to_string())?;
                Ok(Stored {
                    stored: stored.is_some(),
                    matches: db.match_count().map_err(|e| e.to_string())?,
                })
            }
            Store::Server(url) => {
                let body = serde_json::json!({ "key": key, "record": record });
                let reply: ServerStored = post(&format!("{url}/api/matches"), &body)?;
                Ok(Stored {
                    stored: reply.stored,
                    matches: reply.matches,
                })
            }
        }
    }

    pub fn draft(&self, cfg: &Config, lobby: &Lobby, me: Option<&str>) -> Result<Draft, String> {
        match self {
            Store::Local(db) => draft::build(db, cfg, lobby, me).map_err(|e| e.to_string()),
            Store::Server(url) => {
                let body = serde_json::json!({ "lobby": lobby, "battletag": me });
                post(&format!("{url}/api/draft"), &body)
            }
        }
    }

    pub fn count(&self) -> Result<u32, String> {
        match self {
            Store::Local(db) => db.match_count().map_err(|e| e.to_string()),
            Store::Server(url) => {
                let status: ServerStatus = get(&format!("{url}/api/status"))?;
                Ok(status.matches)
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct ServerStored {
    stored: bool,
    matches: u32,
}

#[derive(serde::Deserialize)]
struct ServerStatus {
    matches: u32,
}

fn get<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    ureq::get(url)
        .call()
        .map_err(|e| format!("{url}: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("{url}: {e}"))
}

fn post<T: serde::de::DeserializeOwned>(url: &str, body: &serde_json::Value) -> Result<T, String> {
    ureq::post(url)
        .send_json(body)
        .map_err(|e| format!("{url}: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("{url}: {e}"))
}
