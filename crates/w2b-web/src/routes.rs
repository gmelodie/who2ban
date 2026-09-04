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
    for banner in &banners {
        let image = match image::load_from_memory_with_format(&banner.png, image::ImageFormat::Png)
        {
            Ok(image) => image.to_rgb8(),
            // One unreadable picture is not a reason to drop the other nine.
            Err(e) => {
                tracing::warn!(name = %banner.name, error = %e, "banner would not decode");
                continue;
            }
        };
        let (w, h) = (image.width() as usize, image.height() as usize);
        gained += app.digest(&image.into_raw(), w, h, &banner.name)?;
    }
    let (letters, examples) = app.atlas_size();
    if gained > 0 {
        tracing::info!(gained, letters, examples, banners = banners.len(), "banners digested");
    }
    Ok(Json(Learned {
        gained,
        letters,
        examples,
    }))
}
