use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use hots_core::draft::{self, needs_refresh};
use hots_core::heroesprofile::HpClient;
use hots_core::model::FetchState;
use hots_core::watch::{self, WatchEvent};
use hots_core::{Config, Db, DraftPlayer, ingest, paths};

#[derive(Parser)]
#[command(name = "hots", about = "Draft helper core, without the window")]
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
    Player {
        battletag: String,
        #[arg(long, default_value_t = 1)]
        region: u8,
        #[arg(long)]
        refresh: bool,
    },
    /// Read a battlelobby file and print the draft
    Lobby { path: Option<PathBuf> },
    /// Ingest new replays and print each lobby as it forms
    Watch,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hots=info,hots_core=info".into()),
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
                id.map_or("already stored".into(), |i| format!("match {i}"))
            );
        }
        Cmd::Player {
            battletag,
            region,
            refresh,
        } => player(&db, &cfg, &battletag, region, refresh).await?,
        Cmd::Lobby { path } => lobby(&db, &cfg, path).await?,
        Cmd::Watch => run_watch(&db, &cfg).await?,
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
    println!(
        "api key     {}",
        if cfg.hp_api_key.is_some() {
            "set"
        } else {
            "(unset)"
        }
    );
    println!("game type   {}", cfg.hp_game_type);
    println!("ttl         {} days", cfg.hp_ttl_days);
    println!("temp root   {}", paths::temp_root(cfg).display());
    for dir in paths::replay_dirs(cfg) {
        println!("replays     {}", dir.display());
    }
    println!("matches     {}", db.match_count()?);
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

async fn player(db: &Db, cfg: &Config, battletag: &str, region: u8, refresh: bool) -> Result<()> {
    if refresh {
        let hp = HpClient::new(cfg)?;
        draft::refresh(db, &hp, cfg, battletag, region).await?;
    }
    let row = draft::player_row(db, cfg, battletag, region, 0, 0, false)?;
    print_player(&row, cfg);
    Ok(())
}

async fn lobby(db: &Db, cfg: &Config, path: Option<PathBuf>) -> Result<()> {
    let path = path.unwrap_or_else(|| {
        paths::temp_root(cfg)
            .join("TempWriteReplayP1")
            .join(paths::BATTLELOBBY_NAME)
    });
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let json = path.extension().is_some_and(|e| e == "json");
    show_lobby(db, cfg, &bytes, json).await
}

/// A `.json` lobby feeds the pipeline while the battlelobby parser is a stub.
async fn show_lobby(db: &Db, cfg: &Config, bytes: &[u8], json: bool) -> Result<()> {
    let parsed = if json {
        serde_json::from_slice(bytes)?
    } else {
        hots_core::parse::battlelobby(bytes)?
    };
    let view = draft::build(db, cfg, &parsed)?;
    let show_all = view.my_team.is_none();
    println!("region {} | my team {:?}", view.region, view.my_team);
    for row in view.players.iter().filter(|p| p.enemy || show_all) {
        print_player(row, cfg);
    }

    let Ok(hp) = HpClient::new(cfg) else {
        println!("(no api key, local rows only)");
        return Ok(());
    };
    for row in &view.players {
        if !needs_refresh(row.hp_state) || !(row.enemy || show_all) {
            continue;
        }
        if let Err(e) = draft::refresh(db, &hp, cfg, &row.battletag, row.region).await {
            println!("{}: {e}", row.battletag);
            continue;
        }
        let fresh = draft::player_row(
            db,
            cfg,
            &row.battletag,
            row.region,
            row.slot,
            row.team,
            row.enemy,
        )?;
        print_player(&fresh, cfg);
    }
    Ok(())
}

async fn run_watch(db: &Db, cfg: &Config) -> Result<()> {
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
                if let Err(e) = show_lobby(db, cfg, &bytes, false).await {
                    println!("lobby: {e}");
                }
            }
            Err(_) => return Ok(()),
        }
    }
}

fn print_player(p: &DraftPlayer, cfg: &Config) {
    let mmr = p
        .mmr
        .map(|m| format!("{m:.0}"))
        .unwrap_or_else(|| "-".into());
    println!(
        "\n{} [region {} slot {} team {}] mmr {mmr} local {} games, hp {}",
        p.battletag,
        p.region,
        p.slot,
        p.team,
        p.local_games,
        state_label(p.hp_state)
    );
    for h in &p.heroes {
        let rate = match h.winrate() {
            Some(r) if h.games >= cfg.min_games_for_winrate => format!("{:.0}%", r * 100.0),
            _ => "-".into(),
        };
        println!(
            "  {:<20} {:>4} games {:>5} {:?}",
            h.hero, h.games, rate, h.source
        );
    }
}

fn state_label(state: FetchState) -> &'static str {
    match state {
        FetchState::Fresh => "fresh",
        FetchState::Stale => "stale",
        FetchState::Pending => "pending",
        FetchState::Missing => "none",
        FetchState::Failed => "failed",
    }
}
