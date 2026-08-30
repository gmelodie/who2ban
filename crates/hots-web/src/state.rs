use std::sync::Mutex;

use hots_core::{Config, Db, Draft, paths};
use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct Event {
    pub kind: &'static str,
    pub data: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub matches: u32,
    pub failed: u32,
    pub battletag: Option<String>,
    pub has_api_key: bool,
    pub replay_dirs: Vec<String>,
    pub temp_root: String,
}

pub struct App {
    pub db: Db,
    cfg: Mutex<Config>,
    draft: Mutex<Option<Draft>>,
    events: broadcast::Sender<Event>,
}

impl App {
    pub fn new(db: Db, cfg: Config) -> App {
        App {
            db,
            cfg: Mutex::new(cfg),
            draft: Mutex::new(None),
            events: broadcast::channel(64).0,
        }
    }

    pub fn config(&self) -> Config {
        self.cfg.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_config(&self, cfg: Config) {
        *self.cfg.lock().unwrap_or_else(|e| e.into_inner()) = cfg;
    }

    pub fn draft(&self) -> Option<Draft> {
        self.draft.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_draft(&self, draft: Draft) {
        *self.draft.lock().unwrap_or_else(|e| e.into_inner()) = Some(draft);
    }

    pub fn replace_player(&self, row: &hots_core::DraftPlayer) {
        let mut held = self.draft.lock().unwrap_or_else(|e| e.into_inner());
        let Some(draft) = held.as_mut() else { return };
        if let Some(slot) = draft
            .players
            .iter_mut()
            .find(|p| p.battletag == row.battletag)
        {
            *slot = row.clone();
        }
    }

    /// A send with no listener is the normal case: nobody has the page open.
    pub fn emit(&self, kind: &'static str, data: &impl Serialize) {
        let Ok(text) = serde_json::to_string(data) else {
            return;
        };
        let _ = self.events.send(Event { kind, data: text });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub fn status(&self) -> hots_core::Result<Status> {
        let cfg = self.config();
        Ok(Status {
            matches: self.db.match_count()?,
            failed: self.db.error_count()?,
            battletag: cfg.battletag.clone(),
            has_api_key: cfg.hp_api_key.is_some(),
            replay_dirs: paths::replay_dirs(&cfg)
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            temp_root: paths::temp_root(&cfg).display().to_string(),
        })
    }
}
