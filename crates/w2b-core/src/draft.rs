use w2b_parse::Lobby;

use crate::config::Config;
use crate::db::{Db, LocalHero};
use crate::error::Result;
use crate::model::{Draft, DraftPlayer, HeroRow};

/// `me` comes from the browser that asked, so one server serves several people.
pub fn build(db: &Db, cfg: &Config, lobby: &Lobby, me: Option<&str>) -> Result<Draft> {
    let me = match me.map(str::to_string).or_else(|| cfg.battletag.clone()) {
        Some(tag) => Some(tag),
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

/// The stored name may carry no discriminator, so a name on its own still finds a seat.
/// Two passes, because names repeat: an exact battletag anywhere in the lobby outranks
/// somebody else who merely shares your name and happens to sit in an earlier slot.
fn team_of(lobby: &Lobby, me: &str) -> Option<u8> {
    let name = me.split_once('#').map_or(me, |(n, _)| n);
    let exact = lobby
        .players
        .iter()
        .find(|p| p.battletag.eq_ignore_ascii_case(me));
    exact
        .or_else(|| {
            lobby.players.iter().find(|p| {
                p.battletag
                    .split_once('#')
                    .is_some_and(|(n, _)| n.eq_ignore_ascii_case(name))
            })
        })
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
        note: db.note(battletag)?,
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
