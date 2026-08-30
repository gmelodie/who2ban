use std::sync::Arc;

use hots_core::draft::{self, needs_refresh};
use hots_core::heroesprofile::HpClient;
use hots_core::model::FetchState;
use hots_core::watch::{self, WatchEvent};
use hots_core::{Config, Draft, DraftPlayer, ingest, paths};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::App;

pub fn start(app: AppHandle) {
    let backfill = app.clone();
    std::thread::spawn(move || run_backfill(&backfill));
    std::thread::spawn(move || run_watch(&app));
}

fn run_backfill(app: &AppHandle) {
    let state = app.state::<App>();
    let cfg = state.config();
    let dirs = paths::replay_dirs(&cfg);
    let result = ingest::backfill(&state.db, &dirs, |p| {
        let _ = app.emit("ingest", p);
    });
    if let Err(e) = result {
        tracing::warn!("backfill: {e}");
    }
}

fn run_watch(app: &AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel();
    let cfg = app.state::<App>().config();
    let _watchers = match watch::start(&cfg, tx) {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("watch: {e}");
            return;
        }
    };

    while let Ok(event) = rx.recv() {
        match event {
            WatchEvent::Replay(path) => {
                let state = app.state::<App>();
                match ingest::ingest_file(&state.db, &path) {
                    Ok(_) => {
                        let _ = app.emit("ingested", path.to_string_lossy());
                    }
                    Err(e) => tracing::warn!("ingest {}: {e}", path.display()),
                }
            }
            WatchEvent::Lobby(bytes) => on_lobby(app, &bytes),
        }
    }
}

fn on_lobby(app: &AppHandle, bytes: &[u8]) {
    let state = app.state::<App>();
    let cfg = state.config();

    let lobby = match hots_core::parse::battlelobby(bytes) {
        Ok(l) => l,
        Err(e) => {
            let _ = app.emit("lobby-error", e.to_string());
            return;
        }
    };
    let mut view = match draft::build(&state.db, &cfg, &lobby) {
        Ok(v) => v,
        Err(e) => {
            let _ = app.emit("lobby-error", e.to_string());
            return;
        }
    };

    let show_all = view.my_team.is_none();
    let wanted: Vec<(String, u8)> = view
        .players
        .iter_mut()
        .filter(|p| p.enemy || show_all)
        .filter(|p| needs_refresh(p.hp_state))
        .map(|p| {
            p.hp_state = FetchState::Pending;
            (p.battletag.clone(), p.region)
        })
        .collect();

    state.set_draft(view.clone());
    let _ = app.emit("lobby", &view);

    fetch_all(app, &cfg, wanted);
}

fn fetch_all(app: &AppHandle, cfg: &Config, wanted: Vec<(String, u8)>) {
    if wanted.is_empty() {
        return;
    }
    let hp = match HpClient::new(cfg) {
        Ok(hp) => Arc::new(hp),
        Err(e) => {
            let _ = app.emit("hp-error", e.to_string());
            return;
        }
    };

    for (battletag, region) in wanted {
        let app = app.clone();
        let hp = hp.clone();
        tauri::async_runtime::spawn(async move {
            let row = fetch_one(&app, &hp, &battletag, region).await;
            let _ = app.emit("player", row);
        });
    }
}

/// The slot, the team and the enemy flag come from the stored draft, never from the caller.
pub async fn fetch_one(
    app: &AppHandle,
    hp: &HpClient,
    battletag: &str,
    region: u8,
) -> DraftPlayer {
    let state = app.state::<App>();
    let cfg = state.config();
    let known = state
        .draft()
        .and_then(|d| d.players.iter().find(|p| p.battletag == battletag).cloned());
    let (slot, team, enemy) = known
        .as_ref()
        .map(|p| (p.slot, p.team, p.enemy))
        .unwrap_or((0, 0, true));

    if let Err(e) = draft::refresh(&state.db, hp, &cfg, battletag, region).await {
        let mut row = known.unwrap_or_else(|| empty_row(battletag, region, slot, team, enemy));
        row.hp_state = FetchState::Failed;
        row.error = Some(e.to_string());
        return row;
    }

    match draft::player_row(&state.db, &cfg, battletag, region, slot, team, enemy) {
        Ok(row) => {
            update_stored(&state, &row);
            row
        }
        Err(e) => {
            let mut row = empty_row(battletag, region, slot, team, enemy);
            row.hp_state = FetchState::Failed;
            row.error = Some(e.to_string());
            row
        }
    }
}

fn update_stored(state: &App, row: &DraftPlayer) {
    let Some(mut draft) = state.draft() else { return };
    if let Some(slot) = draft
        .players
        .iter_mut()
        .find(|p| p.battletag == row.battletag)
    {
        *slot = row.clone();
    }
    state.set_draft(draft);
}

fn empty_row(battletag: &str, region: u8, slot: u8, team: u8, enemy: bool) -> DraftPlayer {
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

pub fn draft_of(app: &AppHandle) -> Option<Draft> {
    app.state::<App>().draft()
}
