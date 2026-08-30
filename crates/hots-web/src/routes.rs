use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use hots_core::draft::{self, needs_refresh};
use hots_core::heroesprofile::HpClient;
use hots_core::model::FetchState;
use hots_core::{Config, Draft, DraftPlayer, Lobby, MatchRecord};
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

use crate::state::{App, Status};

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

pub async fn put_config(State(app): State<Arc<App>>, Json(cfg): Json<Config>) -> Reply<()> {
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

/// The page parses the battlelobby itself, so only the ten battletags arrive here.
pub async fn post_draft(State(app): State<Arc<App>>, Json(lobby): Json<Lobby>) -> Reply<Draft> {
    Ok(Json(accept_lobby(&app, lobby)?))
}

pub fn accept_lobby(app: &Arc<App>, lobby: Lobby) -> hots_core::Result<Draft> {
    let cfg = app.config();
    let mut view = draft::build(&app.db, &cfg, &lobby)?;

    let hp = match HpClient::new(&cfg) {
        Ok(hp) => Some(Arc::new(hp)),
        Err(e) => {
            app.emit("hp-error", &e.to_string());
            None
        }
    };

    let show_all = view.my_team.is_none();
    let wanted: Vec<(String, u8)> = match &hp {
        None => Vec::new(),
        Some(_) => view
            .players
            .iter_mut()
            .filter(|p| p.enemy || show_all)
            .filter(|p| needs_refresh(p.hp_state))
            .map(|p| {
                p.hp_state = FetchState::Pending;
                (p.battletag.clone(), p.region)
            })
            .collect(),
    };

    app.set_draft(view.clone());
    app.emit("lobby", &view);
    if let Some(hp) = hp {
        fetch_all(app, hp, wanted);
    }
    Ok(view)
}

fn fetch_all(app: &Arc<App>, hp: Arc<HpClient>, wanted: Vec<(String, u8)>) {
    for (battletag, region) in wanted {
        let app = app.clone();
        let hp = hp.clone();
        tokio::spawn(async move {
            let row = refresh_one(&app, &hp, &battletag, region).await;
            app.emit("player", &row);
        });
    }
}

/// The slot, the team and the enemy flag come from the stored draft, never from the caller.
async fn refresh_one(app: &App, hp: &HpClient, battletag: &str, region: u8) -> DraftPlayer {
    let cfg = app.config();
    let known = app
        .draft()
        .and_then(|d| d.players.iter().find(|p| p.battletag == battletag).cloned());
    let (slot, team, enemy) = known
        .as_ref()
        .map(|p| (p.slot, p.team, p.enemy))
        .unwrap_or((0, 0, true));

    let fetched = draft::refresh(&app.db, hp, &cfg, battletag, region)
        .await
        .and_then(|()| draft::player_row(&app.db, &cfg, battletag, region, slot, team, enemy));

    match fetched {
        Ok(row) => {
            app.replace_player(&row);
            row
        }
        Err(e) => {
            let mut row = known.unwrap_or_else(|| blank(battletag, region, slot, team, enemy));
            row.hp_state = FetchState::Failed;
            row.error = Some(e.to_string());
            row
        }
    }
}

fn blank(battletag: &str, region: u8, slot: u8, team: u8, enemy: bool) -> DraftPlayer {
    DraftPlayer {
        battletag: battletag.to_string(),
        region,
        slot,
        team,
        enemy,
        mmr: None,
        heroes: Vec::new(),
        local_games: 0,
        hp_state: FetchState::Failed,
        error: None,
    }
}

#[derive(Deserialize)]
pub struct RefreshBody {
    pub battletag: String,
    pub region: u8,
}

pub async fn refresh_player(
    State(app): State<Arc<App>>,
    Json(body): Json<RefreshBody>,
) -> Reply<DraftPlayer> {
    let hp = HpClient::new(&app.config())?;
    Ok(Json(
        refresh_one(&app, &hp, &body.battletag, body.region).await,
    ))
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
        app.emit("ingested", &body.key);
    }
    Ok(Json(Stored { stored, matches }))
}

pub async fn known_matches(State(app): State<Arc<App>>) -> Reply<Vec<String>> {
    Ok(Json(app.db.known_replays()?.into_iter().collect()))
}

pub async fn events(
    State(app): State<Arc<App>>,
) -> Sse<impl Stream<Item = Result<SseEvent, std::convert::Infallible>>> {
    let stream = BroadcastStream::new(app.subscribe()).filter_map(|event| {
        let event = event.ok()?;
        Some(Ok(SseEvent::default().event(event.kind).data(event.data)))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
