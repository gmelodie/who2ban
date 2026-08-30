use std::sync::Mutex;

use hots_core::{Config, Db, Draft};
use serde::Serialize;

pub struct App {
    pub db: Db,
    pub cfg: Mutex<Config>,
    pub draft: Mutex<Option<Draft>>,
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

impl App {
    pub fn new(db: Db, cfg: Config) -> App {
        App {
            db,
            cfg: Mutex::new(cfg),
            draft: Mutex::new(None),
        }
    }

    pub fn config(&self) -> Config {
        self.cfg.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_config(&self, cfg: Config) {
        *self.cfg.lock().unwrap_or_else(|e| e.into_inner()) = cfg;
    }

    pub fn set_draft(&self, draft: Draft) {
        *self.draft.lock().unwrap_or_else(|e| e.into_inner()) = Some(draft);
    }

    pub fn draft(&self) -> Option<Draft> {
        self.draft.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
