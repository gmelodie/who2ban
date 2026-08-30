use std::collections::HashMap;

use crate::config::Config;
use crate::db::{Db, HpHero, LocalHero, now};
use crate::error::Result;
use crate::heroesprofile::HpClient;
use hots_parse::Lobby;

use crate::model::{Draft, DraftPlayer, FetchState, HeroRow, Source};

pub fn build(db: &Db, cfg: &Config, lobby: &Lobby) -> Result<Draft> {
    let me = match &cfg.battletag {
        Some(tag) => Some(tag.clone()),
        None => db.likely_self()?,
    };
    let my_team = me.as_deref().and_then(|me| team_of(lobby, me));

    let mut players = Vec::with_capacity(lobby.players.len());
    for p in &lobby.players {
        let enemy = my_team.is_some_and(|t| t != p.team);
        players.push(player_row(
            db,
            cfg,
            &p.battletag,
            lobby.region,
            p.slot,
            p.team,
            enemy,
        )?);
    }

    Ok(Draft {
        region: lobby.region,
        my_team,
        players,
    })
}

fn team_of(lobby: &Lobby, battletag: &str) -> Option<u8> {
    lobby
        .players
        .iter()
        .find(|p| p.battletag.eq_ignore_ascii_case(battletag))
        .map(|p| p.team)
}

pub fn player_row(
    db: &Db,
    cfg: &Config,
    battletag: &str,
    region: u8,
    slot: u8,
    team: u8,
    enemy: bool,
) -> Result<DraftPlayer> {
    let local = db.local_heroes(battletag, cfg.local_all_modes)?;
    let hp = db.hp_heroes(battletag, &cfg.hp_game_type)?;
    let local_games = local.iter().map(|h| h.games).sum();

    Ok(DraftPlayer {
        battletag: battletag.to_string(),
        region,
        slot,
        team,
        enemy,
        mmr: db.hp_mmr(battletag)?,
        heroes: merge_heroes(&local, &hp, cfg.max_heroes),
        local_games,
        hp_state: fetch_state(db.hp_fetched_at(battletag)?, cfg.ttl_secs()),
        error: None,
    })
}

fn fetch_state(fetched_at: Option<i64>, ttl: i64) -> FetchState {
    match fetched_at {
        None => FetchState::Missing,
        Some(at) if now() - at < ttl => FetchState::Fresh,
        Some(_) => FetchState::Stale,
    }
}

pub fn needs_refresh(state: FetchState) -> bool {
    matches!(state, FetchState::Missing | FetchState::Stale)
}

pub async fn refresh(
    db: &Db,
    hp: &HpClient,
    cfg: &Config,
    battletag: &str,
    region: u8,
) -> Result<()> {
    let stats = hp.player_stats(battletag, region).await?;
    db.upsert_player(battletag, region)?;
    db.replace_hp_heroes(battletag, &cfg.hp_game_type, &stats.heroes, stats.mmr)?;
    Ok(())
}

pub fn merge_heroes(local: &[LocalHero], hp: &[HpHero], max: usize) -> Vec<HeroRow> {
    let mut rows: HashMap<&str, HeroRow> = HashMap::new();

    for h in local {
        let row = rows.entry(&h.hero).or_insert_with(|| empty_row(&h.hero));
        row.local_games = h.games;
        row.local_wins = h.wins;
        row.source = row.source.merge(Source::Local);
    }
    for h in hp {
        let row = rows.entry(&h.hero).or_insert_with(|| empty_row(&h.hero));
        row.hp_games = h.games;
        row.hp_wins = h.wins;
        row.source = row.source.merge(Source::Hp);
    }

    let mut out: Vec<HeroRow> = rows
        .into_values()
        .map(|mut row| {
            let use_hp = row.hp_games >= row.local_games;
            row.games = if use_hp {
                row.hp_games
            } else {
                row.local_games
            };
            row.wins = if use_hp { row.hp_wins } else { row.local_wins };
            row
        })
        .collect();

    out.sort_by(|a, b| {
        b.games
            .cmp(&a.games)
            .then_with(|| b.wins.cmp(&a.wins))
            .then_with(|| a.hero.cmp(&b.hero))
    });
    out.truncate(max);
    out
}

fn empty_row(hero: &str) -> HeroRow {
    HeroRow {
        hero: hero.to_string(),
        games: 0,
        wins: 0,
        local_games: 0,
        local_wins: 0,
        hp_games: 0,
        hp_wins: 0,
        source: Source::None,
    }
}
