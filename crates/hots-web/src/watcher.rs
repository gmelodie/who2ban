use std::sync::Arc;

use std::path::Path;

use hots_core::watch::{self, WatchEvent};
use hots_core::{ingest, parse, paths};
use tokio::runtime::Handle;

use crate::routes;
use crate::state::App;

/// Starts only when the game folders sit on this machine, which is what a browser without the File System Access API needs.
pub fn start(app: Arc<App>) -> bool {
    let cfg = app.config();
    let replays = paths::replay_dirs(&cfg).iter().any(|dir| dir.is_dir());
    let temp = paths::temp_root(&cfg);
    if !replays && !temp.exists() && !temp.parent().is_some_and(Path::exists) {
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
    match ingest::backfill(&app.db, &dirs, |p| app.emit("ingest", p)) {
        Ok(done) => tracing::info!(
            parsed = done.done - done.failed,
            failed = done.failed,
            "backfill"
        ),
        Err(e) => tracing::warn!("backfill: {e}"),
    }
}

fn run_watch(app: Arc<App>, handle: Handle) {
    let (tx, rx) = std::sync::mpsc::channel();
    let _watchers = match watch::start(&app.config(), tx) {
        Ok(watchers) => watchers,
        Err(e) => {
            app.set_watch_error(e.to_string());
            return tracing::error!("watch: {e}");
        }
    };

    while let Ok(event) = rx.recv() {
        let _entered = handle.enter();
        match event {
            WatchEvent::Replay(path) => match ingest::ingest_file(&app.db, &path) {
                Ok(Some(id)) => {
                    tracing::info!(id, path = %path.display(), "replay ingested");
                    app.emit("ingested", &path.to_string_lossy())
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("ingest {}: {e}", path.display()),
            },
            WatchEvent::Lobby(bytes) => match parse::battlelobby(&bytes) {
                Ok(lobby) => {
                    if let Err(e) = routes::accept_lobby(&app, lobby, None) {
                        app.emit("lobby-error", &e.to_string());
                    }
                }
                Err(e) => app.emit("lobby-error", &e.to_string()),
            },
        }
    }
}
