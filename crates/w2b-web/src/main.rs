mod routes;
mod state;

use std::sync::Arc;

use axum::Router;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use state::App;
use w2b_core::{Config, Db, paths};

const INDEX: &str = include_str!("../../../ui/index.html");
const APP_JS: &str = include_str!("../../../ui/app.js");
const STYLE: &str = include_str!("../../../ui/style.css");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "w2b_web=info,w2b_core=info".into()),
        )
        .init();

    let cfg = Config::load()?;
    tracing::info!(config = %Config::path().display(), db = %paths::db_path().display(), "starting");
    tracing::info!(
        battletag = cfg.battletag.as_deref().unwrap_or("unset"),
        all_modes = cfg.local_all_modes,
        heroes_shown = cfg.max_heroes,
        "config"
    );
    let app = Arc::new(App::new(Db::open(&paths::db_path())?, cfg));
    tracing::info!(
        matches = app.db.match_count().unwrap_or(0),
        failed = app.db.error_count().unwrap_or(0),
        "database"
    );

    let router = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style))
        .route(
            "/api/config",
            get(routes::get_config).put(routes::put_config),
        )
        .route("/api/status", get(routes::status))
        .route(
            "/api/draft",
            get(routes::get_draft).post(routes::post_draft),
        )
        .route("/api/matches", post(routes::post_match))
        .route("/api/matches/known", get(routes::known_matches))
        .route("/api/matches/recent", get(routes::recent_matches))
        .route("/api/players/battletags", get(routes::battletags))
        .route("/api/player/{battletag}", get(routes::player))
        .route("/api/note/{battletag}", get(routes::get_note))
        .route("/api/note", axum::routing::put(routes::put_note))
        .with_state(app);

    let addr = std::env::var("W2B_ADDR").unwrap_or_else(|_| "127.0.0.1:8731".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

async fn index() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], INDEX)
}

async fn app_js() -> impl IntoResponse {
    script(APP_JS)
}

fn script(body: &'static str) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        body,
    )
}

async fn style() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], STYLE)
}
