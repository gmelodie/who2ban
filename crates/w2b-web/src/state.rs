use std::sync::Mutex;

use serde::Serialize;
use w2b_core::{Config, Db, Draft};

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub matches: u32,
    pub files: u32,
    pub failed: u32,
}

pub struct App {
    pub db: Db,
    cfg: Mutex<Config>,
    draft: Mutex<Option<Draft>>,
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

    /// The last lobby any client sent, so a reload shows it again.
    pub fn draft(&self) -> Option<Draft> {
        self.draft.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_draft(&self, draft: Draft) {
        *self.draft.lock().unwrap_or_else(|e| e.into_inner()) = Some(draft);
    }

    pub fn status(&self) -> w2b_core::Result<Status> {
        Ok(Status {
            matches: self.db.match_count()?,
            files: self.db.file_count()?,
            failed: self.db.error_count()?,
        })
    }
}
