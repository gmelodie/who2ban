use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use w2b_core::{Config, Db, Draft};
use w2b_glyph::Atlas;

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
    /// The shapes everyone on this server has learned between them. One client only ever
    /// sees its own drafts, so alone it learns the alphabet slowly; pooled here, a letter
    /// any one of them has met is a letter all of them can read.
    atlas: Mutex<Atlas>,
    atlas_path: PathBuf,
}

impl App {
    pub fn new(db: Db, cfg: Config, atlas_path: PathBuf) -> App {
        let atlas = Atlas::load(&atlas_path).unwrap_or_default();
        App {
            db,
            cfg: Mutex::new(cfg),
            draft: Mutex::new(None),
            atlas: Mutex::new(atlas),
            atlas_path,
        }
    }

    pub fn atlas(&self) -> Atlas {
        self.atlas.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn atlas_size(&self) -> (usize, usize) {
        let atlas = self.atlas.lock().unwrap_or_else(|e| e.into_inner());
        (atlas.letters(), atlas.examples())
    }

    /// Fold a client's shapes in and write the result. Returns how many examples the
    /// pool actually gained, which is nought when a client sends what it was given.
    pub fn absorb(&self, other: &Atlas) -> std::io::Result<usize> {
        let mut atlas = self.atlas.lock().unwrap_or_else(|e| e.into_inner());
        let before = atlas.examples();
        atlas.absorb(other);
        let gained = atlas.examples() - before;
        if gained > 0 {
            atlas.save(&self.atlas_path)?;
        }
        Ok(gained)
    }

    /// Digest a labelled banner: the picture rather than the shapes cut from it, so the
    /// pool keeps growing even from a client too old, or too thin an atlas, to cut it up
    /// correctly itself.
    pub fn digest(&self, rgb: &[u8], w: usize, h: usize, name: &str) -> std::io::Result<usize> {
        let mut atlas = self.atlas.lock().unwrap_or_else(|e| e.into_inner());
        let before = atlas.examples();
        w2b_glyph::learn(rgb, w, h, name, &mut atlas);
        let gained = atlas.examples() - before;
        if gained > 0 {
            atlas.save(&self.atlas_path)?;
        }
        Ok(gained)
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
