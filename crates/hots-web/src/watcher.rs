use std::sync::Arc;

use hots_core::watch::{self, WatchEvent};
use hots_core::{ingest, parse, paths};
use tokio::runtime::Handle;

use crate::routes;
use crate::state::App;

/// Starts only when the game folders sit on this machine, which is what a browser without the File System Access API needs.
pub fn start(app: Arc<App>) -> bool {
    let cfg = app.config();
    let dirs = paths::replay_dirs(&cfg);
    if dirs.is_empty() && !paths::temp_root(&cfg).exists() {
        return false;
    }

    let handle = Handle::current();
    let ingesting = app.clone();
    std::thread::spawn(move || run_backfill(&ingesting));
    std::thread::spawn(move || run_watch(app, handle));
    true
}

fn run_backfill(app: &App) {
    let dirs = paths::replay_dirs(&app.config());
    if let Err(e) = ingest::backfill(&app.db, &dirs, |p| app.emit("ingest", p)) {
        tracing::warn!("backfill: {e}");
    }
}

fn run_watch(app: Arc<App>, handle: Handle) {
    let (tx, rx) = std::sync::mpsc::channel();
    let _watchers = match watch::start(&app.config(), tx) {
        Ok(watchers) => watchers,
        Err(e) => return tracing::error!("watch: {e}"),
    };

    while let Ok(event) = rx.recv() {
        let _entered = handle.enter();
        match event {
            WatchEvent::Replay(path) => match ingest::ingest_file(&app.db, &path) {
                Ok(Some(_)) => app.emit("ingested", &path.to_string_lossy()),
                Ok(None) => {}
                Err(e) => tracing::warn!("ingest {}: {e}", path.display()),
            },
            WatchEvent::Lobby(bytes) => match parse::battlelobby(&bytes) {
                Ok(lobby) => {
                    if let Err(e) = routes::accept_lobby(&app, lobby) {
                        app.emit("lobby-error", &e.to_string());
                    }
                }
                Err(e) => app.emit("lobby-error", &e.to_string()),
            },
        }
    }
}
