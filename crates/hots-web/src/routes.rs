use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use hots_core::draft;
use hots_core::{Config, Draft, Lobby, MatchRecord};
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
    tracing::info!(
        region = lobby.region,
        players = lobby.players.len(),
        "lobby"
    );

    let view = draft::build(&app.db, &cfg, &lobby)?;
    tracing::info!(
        my_team = ?view.my_team,
        enemies = view.enemies().count(),
        known = view.enemies().filter(|p| p.games > 0).count(),
        "draft"
    );

    app.set_draft(view.clone());
    app.emit("lobby", &view);
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
        app.emit("ingested", &body.key);
    } else {
        tracing::debug!(key = %body.key, "match already stored");
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
