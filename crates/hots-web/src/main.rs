mod routes;
mod state;
mod watcher;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use hots_core::{Config, Db, paths};
use state::App;

const INDEX: &str = include_str!("../../../ui/index.html");
const APP_JS: &str = include_str!("../../../ui/app.js");
const FS_JS: &str = include_str!("../../../ui/fs.js");
const WASM_JS: &str = include_str!("../../../ui/wasm.js");
const STYLE: &str = include_str!("../../../ui/style.css");

const WASM_NAME: &str = "hots_parse.wasm";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hots_web=info,hots_core=info".into()),
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

    let module = wasm_path();
    tracing::info!(path = %module.display(), found = module.exists(), "parser module");

    let cfg = app.config();
    tracing::info!(
        temp_root = %paths::temp_root(&cfg).display(),
        replay_dirs = paths::replay_dirs(&cfg).len(),
        "game folders"
    );
    for dir in paths::replay_dirs(&cfg) {
        tracing::info!(path = %dir.display(), "replay folder");
    }
    if watcher::start(app.clone()) {
        app.set_watching(true);
        tracing::info!("reading the game folders from this machine");
    } else {
        tracing::info!("no game folders here, the page has to read them");
    }

    let router = Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/fs.js", get(fs_js))
        .route("/wasm.js", get(wasm_js))
        .route("/style.css", get(style))
        .route("/hots_parse.wasm", get(wasm))
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
        .route("/api/events", get(routes::events))
        .with_state(app);

    let addr = std::env::var("HOTS_ADDR").unwrap_or_else(|_| "127.0.0.1:8731".to_string());
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

async fn fs_js() -> impl IntoResponse {
    script(FS_JS)
}

async fn wasm_js() -> impl IntoResponse {
    script(WASM_JS)
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

/// A second file next to the binary, so a parser rebuild needs no server rebuild.
async fn wasm() -> impl IntoResponse {
    match std::fs::read(wasm_path()) {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "application/wasm")],
            bytes.into_response(),
        )
            .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            format!("{WASM_NAME} not found: {e}. Build it with `make wasm`."),
        )
            .into_response(),
    }
}

fn wasm_path() -> PathBuf {
    if let Ok(path) = std::env::var("HOTS_WASM") {
        return PathBuf::from(path);
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(WASM_NAME)));
    match beside_exe {
        Some(path) if path.exists() => path,
        _ => PathBuf::from("target/wasm32-unknown-unknown/release").join(WASM_NAME),
    }
}
