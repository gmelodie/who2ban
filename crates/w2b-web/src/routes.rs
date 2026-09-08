use std::sync::Arc;

use crate::state::{App, Status};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use w2b_core::draft;
use w2b_core::{Config, Draft, Lobby, MatchRecord};
use w2b_glyph::Atlas;

pub struct Failed(String);

impl IntoResponse for Failed {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0).into_response()
    }
}

impl<E: std::fmt::Display> From<E> for Failed {
    fn from(e: E) -> Failed {
        Failed(e.to_string())
    }
}

type Reply<T> = Result<Json<T>, Failed>;

pub async fn get_config(State(app): State<Arc<App>>) -> Json<Config> {
    Json(app.config())
}

/// The folders belong to the machine the server runs on, not to whoever opened the page.
pub async fn put_config(State(app): State<Arc<App>>, Json(cfg): Json<Config>) -> Reply<()> {
    let here = app.config();
    let cfg = Config {
        replay_dir: here.replay_dir,
        temp_dir: here.temp_dir,
        ..cfg
    };
    cfg.save()?;
    app.set_config(cfg);
    Ok(Json(()))
}

pub async fn status(State(app): State<Arc<App>>) -> Reply<Status> {
    Ok(Json(app.status()?))
}

pub async fn get_draft(State(app): State<Arc<App>>) -> Json<Option<Draft>> {
    Json(app.draft())
}

#[derive(Deserialize)]
pub struct NoteBody {
    pub battletag: String,
    #[serde(flatten)]
    pub note: w2b_core::PlayerNote,
}

/// Everyone behind the one login shares these, so the last word is the group's word.
pub async fn put_note(State(app): State<Arc<App>>, Json(body): Json<NoteBody>) -> Reply<()> {
    tracing::info!(
        battletag = body.battletag,
        verdict = body.note.verdict,
        note = body.note.note.len(),
        "note"
    );
    app.db.set_note(&body.battletag, &body.note)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
pub struct HeroVerdictBody {
    pub battletag: String,
    pub hero: String,
    /// 1 thumbs up, -1 thumbs down.
    pub verdict: i8,
    /// Take one back rather than give one.
    #[serde(default)]
    pub take_back: bool,
}

/// A thumb for a player on one hero. These add up, where a note's verdict is replaced.
pub async fn put_hero_verdict(
    State(app): State<Arc<App>>,
    Json(body): Json<HeroVerdictBody>,
) -> Reply<()> {
    tracing::info!(
        battletag = body.battletag,
        hero = body.hero,
        verdict = body.verdict,
        take_back = body.take_back,
        "hero verdict"
    );
    app.db
        .rate_hero(&body.battletag, &body.hero, body.verdict, body.take_back)?;
    Ok(Json(()))
}

pub async fn get_note(
    State(app): State<Arc<App>>,
    axum::extract::Path(battletag): axum::extract::Path<String>,
) -> Reply<w2b_core::PlayerNote> {
    Ok(Json(app.db.note(&battletag)?))
}

#[derive(Deserialize)]
pub struct LobbyBody {
    pub lobby: Lobby,
    /// Who is asking. Every browser answers for itself.
    pub battletag: Option<String>,
}

/// The page parses the battlelobby itself, so only the ten battletags arrive here.
pub async fn post_draft(State(app): State<Arc<App>>, Json(body): Json<LobbyBody>) -> Reply<Draft> {
    Ok(Json(accept_lobby(
        &app,
        body.lobby,
        body.battletag.as_deref(),
    )?))
}

pub fn accept_lobby(app: &Arc<App>, lobby: Lobby, me: Option<&str>) -> w2b_core::Result<Draft> {
    let cfg = app.config();
    tracing::info!(
        region = lobby.region,
        players = lobby.players.len(),
        me = me.unwrap_or("unset"),
        "lobby"
    );

    let view = draft::build(&app.db, &cfg, &lobby, me)?;
    tracing::info!(
        my_team = ?view.my_team,
        enemies = view.enemies().count(),
        known = view.enemies().filter(|p| p.games > 0).count(),
        "draft"
    );

    app.set_draft(view.clone());
    Ok(view)
}

#[derive(Deserialize)]
pub struct MatchBody {
    pub key: String,
    pub record: MatchRecord,
}

#[derive(Serialize)]
pub struct Stored {
    pub stored: bool,
    pub matches: u32,
}

pub async fn post_match(State(app): State<Arc<App>>, Json(body): Json<MatchBody>) -> Reply<Stored> {
    let stored = app.db.record_replay(&body.key, &body.record)?.is_some();
    let matches = app.db.match_count()?;
    if stored {
        tracing::info!(
            key = %body.key,
            map = %body.record.map,
            mode = body.record.mode.as_str(),
            build = body.record.build,
            "match stored"
        );
    } else {
        tracing::info!(key = %body.key, "already stored, another replay of the same match");
    }
    Ok(Json(Stored { stored, matches }))
}

pub async fn known_matches(State(app): State<Arc<App>>) -> Reply<Vec<String>> {
    Ok(Json(app.db.known_replays()?.into_iter().collect()))
}

/// Every battletag on record. The desktop client reads the draft off its own screen and
/// needs to know who it is allowed to conclude a name is; a client backed by this server
/// has no local roster to ask.
pub async fn battletags(State(app): State<Arc<App>>) -> Reply<Vec<String>> {
    Ok(Json(app.db.battletags()?))
}

pub async fn recent_matches(State(app): State<Arc<App>>) -> Reply<Vec<w2b_core::MatchSummary>> {
    Ok(Json(app.db.recent_matches(20)?))
}

pub async fn player(
    State(app): State<Arc<App>>,
    axum::extract::Path(battletag): axum::extract::Path<String>,
) -> Reply<w2b_core::DraftPlayer> {
    let cfg = app.config();
    Ok(Json(draft::player_row(
        &app.db, &cfg, &battletag, 0, 0, false,
    )?))
}

/// What a client got, or gave, when it swapped shapes with the pool.
#[derive(Debug, Clone, Serialize)]
pub struct Learned {
    /// Examples the pool did not already hold.
    pub gained: usize,
    pub letters: usize,
    pub examples: usize,
}

/// The widest and tallest a banner may be before it is not a banner.
///
/// One name cropped off the draft screen is a few hundred pixels each way at 4K and
/// smaller on every other screen, so this is generous by a factor of several. It exists
/// because decoding is the one thing a client can ask this server to do that costs real
/// memory: a PNG of a few kilobytes can declare enormous dimensions, and without a limit
/// the decoder allocates for what was declared.
const MOST_BANNER_PIXELS: u32 = 2000;

/// And how much a decode may allocate whatever it declares. The crate's own default is
/// half a gigabyte, which is fine for a program opening a file its user chose and far
/// too much for one opening whatever arrives over the internet, several at a time.
const MOST_BANNER_ALLOC: u64 = 32 * 1024 * 1024;

/// The largest a banner's PNG may be. Checked before anything is decoded or written, so
/// an oversized one costs a length comparison.
const MOST_BANNER_BYTES: usize = 2 * 1024 * 1024;

/// Decode one banner, refusing anything that is not the size and shape of a banner.
fn decode_banner(png: &[u8]) -> Result<image::RgbImage, String> {
    if png.len() > MOST_BANNER_BYTES {
        return Err(format!("{} bytes is not a banner", png.len()));
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MOST_BANNER_PIXELS);
    limits.max_image_height = Some(MOST_BANNER_PIXELS);
    limits.max_alloc = Some(MOST_BANNER_ALLOC);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(png));
    reader.set_format(image::ImageFormat::Png);
    reader.limits(limits);
    reader
        .decode()
        .map(|image| image.to_rgb8())
        .map_err(|e| e.to_string())
}

/// One banner as it was on a client's screen, with the name the battlelobby says it
/// carried. PNG rather than raw pixels: a draft is ten of these and the raw form is
/// twenty-four megabytes of mostly background.
#[derive(Debug, Clone, Deserialize)]
pub struct BannerBody {
    /// PNG bytes. Serde reads a JSON array of numbers, which `Vec<u8>` is.
    pub png: Vec<u8>,
    /// The name the banner turned out to say, without its discriminator.
    pub name: String,
}

pub async fn get_glyphs(State(app): State<Arc<App>>) -> Json<Atlas> {
    Json(app.atlas())
}

/// A client folds what it has learned into the pool and is told what was new.
pub async fn post_glyphs(State(app): State<Arc<App>>, Json(atlas): Json<Atlas>) -> Reply<Learned> {
    let gained = app.absorb(&atlas)?;
    let (letters, examples) = app.atlas_size();
    if gained > 0 {
        tracing::info!(gained, letters, examples, "glyphs pooled");
    }
    Ok(Json(Learned {
        gained,
        letters,
        examples,
    }))
}

/// The pictures rather than the shapes. A client that could not cut a banner into the
/// right number of letters still has the banner, and the pool can try again with an
/// alphabet that client did not have.
pub async fn post_banners(
    State(app): State<Arc<App>>,
    Json(banners): Json<Vec<BannerBody>>,
) -> Reply<Learned> {
    let mut gained = 0;
    let mut kept = 0;
    for banner in &banners {
        let image = match decode_banner(&banner.png) {
            Ok(image) => image,
            // One unreadable picture is not a reason to drop the other nine.
            Err(e) => {
                tracing::warn!(name = %banner.name, error = %e, "banner would not decode");
                continue;
            }
        };
        // Kept before it is digested, and kept whether or not the digesting works. The
        // pool being unable to cut it up is precisely why the picture is worth having.
        match app.keep_banner(&banner.name, &banner.png) {
            Ok(file) => {
                kept += 1;
                tracing::debug!(name = %banner.name, %file, "banner kept");
            }
            Err(e) => tracing::warn!(name = %banner.name, error = %e, "banner would not be kept"),
        }
        let (w, h) = (image.width() as usize, image.height() as usize);
        gained += app.digest(&image.into_raw(), w, h, &banner.name)?;
    }
    let (letters, examples) = app.atlas_size();
    tracing::info!(
        gained,
        kept,
        letters,
        examples,
        banners = banners.len(),
        "banners digested"
    );
    Ok(Json(Learned {
        gained,
        letters,
        examples,
    }))
}

/// The banners this server has kept, newest first, so they can be fetched and looked at
/// from wherever the reader is being worked on rather than only from the box they landed
/// on. The pool runs on one machine and the segmenter is fixed on another.
pub async fn list_banners(State(app): State<Arc<App>>) -> Json<Vec<crate::state::Kept>> {
    Json(app.kept_banners())
}

/// One kept banner, as the PNG that arrived.
pub async fn get_banner(
    State(app): State<Arc<App>>,
    axum::extract::Path(file): axum::extract::Path<String>,
) -> Response {
    match app.kept_banner(&file) {
        Some(png) => ([(axum::http::header::CONTENT_TYPE, "image/png")], png).into_response(),
        None => (StatusCode::NOT_FOUND, "no such banner").into_response(),
    }
}
