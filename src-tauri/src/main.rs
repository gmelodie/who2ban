#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pipeline;
mod state;

use hots_core::heroesprofile::HpClient;
use hots_core::{Config, Db, Draft, DraftPlayer, IngestProgress, ingest, paths};
use state::{App, Status};
use tauri::{AppHandle, Emitter, Manager, State};

type Reply<T> = Result<T, String>;

fn fail(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn get_config(app: State<'_, App>) -> Config {
    app.config()
}

#[tauri::command]
fn set_config(app: State<'_, App>, cfg: Config) -> Reply<()> {
    cfg.save().map_err(fail)?;
    app.set_config(cfg);
    Ok(())
}

#[tauri::command]
fn status(app: State<'_, App>) -> Reply<Status> {
    let cfg = app.config();
    Ok(Status {
        matches: app.db.match_count().map_err(fail)?,
        failed: app.db.error_count().map_err(fail)?,
        battletag: cfg.battletag.clone(),
        has_api_key: cfg.hp_api_key.is_some(),
        replay_dirs: paths::replay_dirs(&cfg)
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        temp_root: paths::temp_root(&cfg).display().to_string(),
    })
}

#[tauri::command]
fn current_draft(app: AppHandle) -> Option<Draft> {
    pipeline::draft_of(&app)
}

#[tauri::command]
async fn refresh_player(app: AppHandle, battletag: String, region: u8) -> Reply<DraftPlayer> {
    let hp = HpClient::new(&app.state::<App>().config()).map_err(fail)?;
    Ok(pipeline::fetch_one(&app, &hp, &battletag, region).await)
}

#[tauri::command]
fn rescan(app: AppHandle) -> Reply<IngestProgress> {
    let state = app.state::<App>();
    let cfg = state.config();
    ingest::backfill(&state.db, &paths::replay_dirs(&cfg), |p| {
        let _ = app.emit("ingest", p);
    })
    .map_err(fail)
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hots_draft=info,hots_core=info".into()),
        )
        .init();

    let cfg = Config::load().unwrap_or_default();
    let db = Db::open(&paths::db_path()).expect("open database");

    tauri::Builder::default()
        .manage(App::new(db, cfg))
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            status,
            current_draft,
            refresh_player,
            rescan
        ])
        .setup(|app| {
            pipeline::start(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run app");
}
