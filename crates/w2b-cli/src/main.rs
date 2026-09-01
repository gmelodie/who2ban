use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use w2b_core::draft;
use w2b_core::watch::{self, WatchEvent};
use w2b_core::{Config, Db, DraftPlayer, ingest, paths};

#[derive(Parser)]
#[command(name = "w2b-cli", about = "The who2ban core, without the window")]
struct Cli {
    #[arg(long)]
    db: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print the resolved config and the folders it points at
    Config,
    /// Parse every replay not stored yet
    Backfill,
    /// Parse one replay
    Ingest { path: PathBuf },
    /// Show the stored rows of one player
    Player { battletag: String },
    /// Read a battlelobby file and print the draft
    Lobby { path: Option<PathBuf> },
    /// Report what the battletag scan sees in a replay or a battlelobby
    Scan { path: PathBuf },
    /// Ingest new replays and print each lobby as it forms
    Watch,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "w2b=info,w2b_core=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let cfg = Config::load().context("load config")?;
    let db = Db::open(&cli.db.unwrap_or_else(paths::db_path))?;

    match cli.cmd {
        Cmd::Config => show_config(&cfg, &db)?,
        Cmd::Backfill => backfill(&db, &cfg)?,
        Cmd::Ingest { path } => {
            let id = ingest::ingest_file(&db, &path)?;
            println!(
                "{}",
                id.map_or_else(
                    || "already stored, another replay of the same match".to_string(),
                    |i| format!("match {i}")
                )
            );
        }
        Cmd::Player { battletag } => player(&db, &cfg, &battletag)?,
        Cmd::Lobby { path } => lobby(&db, &cfg, path)?,
        Cmd::Scan { path } => scan(&path)?,
        Cmd::Watch => run_watch(&db, &cfg)?,
    }
    Ok(())
}

fn show_config(cfg: &Config, db: &Db) -> Result<()> {
    println!("config      {}", Config::path().display());
    println!("database    {}", paths::db_path().display());
    println!(
        "battletag   {}",
        cfg.battletag.as_deref().unwrap_or("(unset)")
    );
    println!("all modes   {}", cfg.local_all_modes);
    println!("temp root   {}", paths::temp_root(cfg).display());
    for dir in paths::replay_dirs(cfg) {
        println!("replays     {}", dir.display());
    }
    println!("matches     {}", db.match_count()?);
    println!("files       {}", db.file_count()?);
    println!("failed      {}", db.error_count()?);
    Ok(())
}

fn backfill(db: &Db, cfg: &Config) -> Result<()> {
    let dirs = paths::replay_dirs(cfg);
    if dirs.is_empty() {
        anyhow::bail!(
            "no replay folder found, set replay_dir in {}",
            Config::path().display()
        );
    }
    let progress = ingest::backfill(db, &dirs, |p| {
        if p.done % 25 == 0 || p.done == p.total {
            println!("{}/{} ({} failed)", p.done, p.total, p.failed);
        }
    })?;
    println!(
        "done: {} parsed, {} failed",
        progress.done - progress.failed,
        progress.failed
    );
    Ok(())
}

/// Prints the framing around each discriminator, which is what a new build changes.
fn scan(path: &std::path::Path) -> Result<()> {
    let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let bytes = match w2b_core::parse::lobby_stream(raw.clone()) {
        Ok(stream) => {
            println!("battlelobby stream: {} bytes", stream.len());
            stream
        }
        Err(e) => {
            println!("not a replay archive ({e}), reading the file as a battlelobby");
            raw
        }
    };

    match w2b_core::parse::battlelobby(&bytes) {
        Ok(lobby) => println!(
            "region {} | {} battletags",
            lobby.region,
            lobby.players.len()
        ),
        Err(e) => println!("scan: {e}"),
    }

    let marks: Vec<usize> = bytes
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'#')
        .map(|(i, _)| i)
        .collect();
    println!("{} '#' bytes in the stream, last ten:", marks.len());
    for at in marks.iter().rev().take(10).rev() {
        let from = at.saturating_sub(20);
        let to = (at + 12).min(bytes.len());
        println!("  {at:#08x} {}", dump(&bytes[from..to]));
    }
    Ok(())
}

fn dump(window: &[u8]) -> String {
    let hex: Vec<String> = window.iter().map(|b| format!("{b:02x}")).collect();
    let text: String = window
        .iter()
        .map(|b| {
            if b.is_ascii_graphic() {
                *b as char
            } else {
                '.'
            }
        })
        .collect();
    format!("{}  |{text}|", hex.join(" "))
}

fn player(db: &Db, cfg: &Config, battletag: &str) -> Result<()> {
    print_player(&draft::player_row(db, cfg, battletag, 0, 0, false)?, cfg);
    Ok(())
}

fn lobby(db: &Db, cfg: &Config, path: Option<PathBuf>) -> Result<()> {
    let path = path.unwrap_or_else(|| {
        paths::temp_root(cfg)
            .join("TempWriteReplayP1")
            .join(paths::BATTLELOBBY_NAME)
    });
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let json = path.extension().is_some_and(|e| e == "json");
    show_lobby(db, cfg, &bytes, json)
}

/// A `.json` lobby feeds the pipeline without a game installed.
fn show_lobby(db: &Db, cfg: &Config, bytes: &[u8], json: bool) -> Result<()> {
    let parsed = if json {
        serde_json::from_slice(bytes)?
    } else {
        w2b_core::parse::battlelobby(bytes)?
    };
    let view = draft::build(db, cfg, &parsed, None)?;
    let show_all = view.my_team.is_none();
    println!("region {} | my team {:?}", view.region, view.my_team);
    for row in view.players.iter().filter(|p| p.enemy || show_all) {
        print_player(row, cfg);
    }
    Ok(())
}

fn run_watch(db: &Db, cfg: &Config) -> Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _watchers = watch::start(cfg, tx)?;
    println!("watching, ctrl-c to stop");
    loop {
        match rx.recv() {
            Ok(WatchEvent::Replay(path)) => match ingest::ingest_file(db, &path) {
                Ok(_) => println!("ingested {}", path.display()),
                Err(e) => println!("failed {}: {e}", path.display()),
            },
            Ok(WatchEvent::Lobby(bytes)) => {
                if let Err(e) = show_lobby(db, cfg, &bytes, false) {
                    println!("lobby: {e}");
                }
            }
            Err(_) => return Ok(()),
        }
    }
}

fn print_player(p: &DraftPlayer, cfg: &Config) {
    println!(
        "\n{} [slot {} team {}] {} games on record",
        p.battletag, p.slot, p.team, p.games
    );
    for h in &p.heroes {
        let rate = match h.winrate() {
            Some(r) if h.games >= cfg.min_games_for_winrate => format!("{:.0}%", r * 100.0),
            _ => "-".into(),
        };
        println!("  {:<20} {:>4} games {:>5}", h.hero, h.games, rate);
    }
}
