use std::path::{Path, PathBuf};
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
    /// Where the draft banners clients send are kept. Beside the pool they feed, because
    /// they are the same evidence: a banner is only ever sent here because no client
    /// could cut it into its letters, and the picture is the only thing that says why.
    banners_dir: PathBuf,
}

/// Banners kept on the server. Ten a draft, a few tens of kilobytes each, so this is a
/// couple of hundred megabytes at the very worst and far less in practice: a banner is
/// only sent when a client fails to file it, and that stops as the pool learns.
const MOST_KEPT: usize = 2000;

/// One banner on the server's disk.
#[derive(Debug, Clone, Serialize)]
pub struct Kept {
    /// The file, which is also how it is asked for again. Never a path.
    pub file: String,
    /// The name the battlelobby said the banner carried.
    pub name: String,
    /// Seconds since the epoch, from the filename rather than the filesystem.
    pub at: u64,
    pub bytes: u64,
}

impl App {
    pub fn new(db: Db, cfg: Config, atlas_path: PathBuf) -> App {
        let atlas = Atlas::load(&atlas_path).unwrap_or_default();
        let banners_dir = atlas_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("banners");
        App {
            db,
            cfg: Mutex::new(cfg),
            draft: Mutex::new(None),
            atlas: Mutex::new(atlas),
            atlas_path,
            banners_dir,
        }
    }

    pub fn banners_dir(&self) -> &Path {
        &self.banners_dir
    }

    /// Keep one banner as it arrived, under the name the battlelobby gave it.
    ///
    /// The pool digests these and throws the picture away, which is what left the last
    /// investigation with a letter count and nothing to look at. A banner nobody can cut
    /// up is not a transient: it is the same shape every draft until the segmenter is
    /// fixed, and fixing it means having one to open.
    pub fn keep_banner(&self, name: &str, png: &[u8]) -> std::io::Result<String> {
        std::fs::create_dir_all(&self.banners_dir)?;
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        // The name is whatever a stranger called themselves, so only the plainly safe
        // characters of it reach the filesystem.
        let safe: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .take(32)
            .collect();
        // A draft is ten banners in the same second, so the digest of the picture keeps
        // them apart - and makes the same banner sent twice land on one file.
        let mark = short_hash(png);
        let file = format!("{at}-{safe}-{mark}.png");
        std::fs::write(self.banners_dir.join(&file), png)?;
        prune(&self.banners_dir, MOST_KEPT);
        Ok(file)
    }

    /// What is on disk, newest first.
    pub fn kept_banners(&self) -> Vec<Kept> {
        let Ok(entries) = std::fs::read_dir(&self.banners_dir) else {
            return Vec::new();
        };
        let mut out: Vec<Kept> = entries
            .flatten()
            .filter_map(|e| {
                let file = e.file_name().to_str()?.to_string();
                let meta = e.metadata().ok()?;
                if !meta.is_file() || !file.ends_with(".png") {
                    return None;
                }
                // `<seconds>-<name>-<mark>.png`, which is what `keep_banner` wrote.
                let stem = file.strip_suffix(".png")?;
                let (at, rest) = stem.split_once('-')?;
                let name = rest.rsplit_once('-').map_or(rest, |(n, _)| n).to_string();
                Some(Kept {
                    at: at.parse().unwrap_or(0),
                    name,
                    bytes: meta.len(),
                    file,
                })
            })
            .collect();
        out.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.file.cmp(&b.file)));
        out
    }

    /// One kept banner by its file. `None` for anything that is not a plain name in that
    /// one folder, so no request can walk out of it.
    pub fn kept_banner(&self, file: &str) -> Option<Vec<u8>> {
        let plain = !file.is_empty()
            && file.ends_with(".png")
            && file
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
            && !file.contains("..");
        if !plain {
            return None;
        }
        std::fs::read(self.banners_dir.join(file)).ok()
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

/// Enough of a digest to tell two banners of the same second apart.
fn short_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Drop the oldest files until only `most` are left. Failures are ignored: tidying is
/// not a reason to refuse a banner that has already been written.
fn prune(dir: &Path, most: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let at = e.path();
            let when = e.metadata().ok()?.modified().ok()?;
            at.is_file().then_some((when, at))
        })
        .collect();
    if files.len() <= most {
        return;
    }
    files.sort_by_key(|(when, _)| *when);
    for (_, at) in &files[..files.len() - most] {
        let _ = std::fs::remove_file(at);
    }
}
