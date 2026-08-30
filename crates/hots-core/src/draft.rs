use hots_parse::Lobby;

use crate::config::Config;
use crate::db::{Db, LocalHero};
use crate::error::Result;
use crate::model::{Draft, DraftPlayer, HeroRow};

pub fn build(db: &Db, cfg: &Config, lobby: &Lobby) -> Result<Draft> {
    let me = match &cfg.battletag {
        Some(tag) => Some(tag.clone()),
        None => db.likely_self()?,
    };
    let my_team = me.as_deref().and_then(|me| team_of(lobby, me));

    let mut players = Vec::with_capacity(lobby.players.len());
    for p in &lobby.players {
        let enemy = my_team.is_some_and(|t| t != p.team);
        players.push(player_row(db, cfg, &p.battletag, p.slot, p.team, enemy)?);
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
    slot: u8,
    team: u8,
    enemy: bool,
) -> Result<DraftPlayer> {
    let local = db.local_heroes(battletag, cfg.local_all_modes)?;

    Ok(DraftPlayer {
        battletag: battletag.to_string(),
        slot,
        team,
        enemy,
        games: local.iter().map(|h| h.games).sum(),
        heroes: hero_rows(local, cfg.max_heroes),
    })
}

/// Most played first, since that is what they will pick.
pub fn hero_rows(local: Vec<LocalHero>, max: usize) -> Vec<HeroRow> {
    let mut out: Vec<HeroRow> = local
        .into_iter()
        .map(|h| HeroRow {
            hero: h.hero,
            games: h.games,
            wins: h.wins,
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
