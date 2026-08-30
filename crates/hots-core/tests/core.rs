use hots_core::db::{Db, HpHero, LocalHero};
use hots_core::heroesprofile::{heroes_from_json, mmr_from_json};
use hots_core::{Config, Source, draft};
use hots_core::{GameMode, Lobby, LobbyPlayer, MatchPlayer, MatchRecord, Toon};

fn toon(region: u8, id: u64) -> Toon {
    Toon {
        region,
        realm: 1,
        id,
    }
}

fn replay(mode: GameMode, picks: &[(&str, &str, u8, bool)]) -> MatchRecord {
    MatchRecord {
        players: picks
            .iter()
            .enumerate()
            .map(|(i, (tag, hero, team, won))| MatchPlayer {
                battletag: tag.to_string(),
                hero: hero.to_string(),
                toon: toon(1, i as u64),
                team: *team,
                won: *won,
            })
            .collect(),
        map: "Cursed Hollow".into(),
        mode,
        played_at: 1_700_000_000,
        build: 90_000,
    }
}

#[test]
fn records_a_replay_once() {
    let db = Db::open_memory().unwrap();
    let r = replay(GameMode::StormLeague, &[("Me#1", "Raynor", 0, true)]);

    assert!(db.record_replay("a.StormReplay", &r).unwrap().is_some());
    assert!(db.record_replay("a.StormReplay", &r).unwrap().is_none());
    assert_eq!(db.match_count().unwrap(), 1);
}

#[test]
fn aggregates_local_heroes_and_filters_by_mode() {
    let db = Db::open_memory().unwrap();
    db.record_replay(
        "1.StormReplay",
        &replay(GameMode::StormLeague, &[("Foe#1", "Raynor", 1, true)]),
    )
    .unwrap();
    db.record_replay(
        "2.StormReplay",
        &replay(GameMode::StormLeague, &[("Foe#1", "Raynor", 1, false)]),
    )
    .unwrap();
    db.record_replay(
        "3.StormReplay",
        &replay(GameMode::QuickMatch, &[("Foe#1", "Muradin", 1, true)]),
    )
    .unwrap();

    let all = db.local_heroes("Foe#1", true).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].hero, "Raynor");
    assert_eq!((all[0].games, all[0].wins), (2, 1));

    let ranked = db.local_heroes("Foe#1", false).unwrap();
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].hero, "Raynor");
}

#[test]
fn merges_local_and_hp_rows() {
    let local = vec![LocalHero {
        hero: "Raynor".into(),
        games: 2,
        wins: 1,
    }];
    let hp = vec![
        HpHero {
            hero: "Raynor".into(),
            games: 40,
            wins: 22,
            mmr: Some(2400.0),
        },
        HpHero {
            hero: "Jaina".into(),
            games: 10,
            wins: 5,
            mmr: None,
        },
    ];

    let rows = draft::merge_heroes(&local, &hp, 8);
    assert_eq!(rows[0].hero, "Raynor");
    assert_eq!(rows[0].source, Source::Both);
    assert_eq!((rows[0].games, rows[0].wins), (40, 22));
    assert_eq!(rows[0].local_games, 2);
    assert_eq!(rows[1].source, Source::Hp);
    assert_eq!(draft::merge_heroes(&local, &hp, 1).len(), 1);
}

#[test]
fn keeps_local_rows_when_hp_is_thinner() {
    let local = vec![LocalHero {
        hero: "Raynor".into(),
        games: 9,
        wins: 5,
    }];
    let hp = vec![HpHero {
        hero: "Raynor".into(),
        games: 2,
        wins: 0,
        mmr: None,
    }];

    let rows = draft::merge_heroes(&local, &hp, 8);
    assert_eq!((rows[0].games, rows[0].wins), (9, 5));
    assert_eq!(rows[0].winrate(), Some(5.0 / 9.0));
}

fn lobby() -> Lobby {
    Lobby {
        region: 2,
        players: (0..10)
            .map(|i| LobbyPlayer {
                battletag: format!("P{i}#100{i}"),
                team: if i < 5 { 0 } else { 1 },
                slot: i,
            })
            .collect(),
    }
}

#[test]
fn splits_the_lobby_into_teams() {
    let db = Db::open_memory().unwrap();
    let cfg = Config {
        battletag: Some("P7#1007".into()),
        ..Config::default()
    };

    let view = draft::build(&db, &cfg, &lobby()).unwrap();
    assert_eq!(view.my_team, Some(1));
    assert_eq!(view.enemies().count(), 5);
    assert!(view.enemies().all(|p| p.team == 0));
}

#[test]
fn falls_back_to_the_most_seen_battletag() {
    let db = Db::open_memory().unwrap();
    db.record_replay(
        "1.StormReplay",
        &replay(
            GameMode::StormLeague,
            &[
                ("P2#1002", "Raynor", 0, true),
                ("Other#1", "Jaina", 1, false),
            ],
        ),
    )
    .unwrap();
    db.record_replay(
        "2.StormReplay",
        &replay(GameMode::StormLeague, &[("P2#1002", "Jaina", 0, true)]),
    )
    .unwrap();

    let view = draft::build(&db, &Config::default(), &lobby()).unwrap();
    assert_eq!(view.my_team, Some(0));
    assert!(view.enemies().all(|p| p.team == 1));
}

#[test]
fn marks_every_player_when_the_lobby_has_no_self() {
    let db = Db::open_memory().unwrap();
    let cfg = Config {
        battletag: Some("Nobody#1".into()),
        ..Config::default()
    };

    let view = draft::build(&db, &cfg, &lobby()).unwrap();
    assert_eq!(view.my_team, None);
    assert_eq!(view.enemies().count(), 0);
    assert_eq!(view.players.len(), 10);
}

#[test]
fn caches_hp_rows_and_reports_freshness() {
    let db = Db::open_memory().unwrap();
    let cfg = Config::default();

    let before = draft::player_row(&db, &cfg, "Foe#1", 1, 0, 0, true).unwrap();
    assert_eq!(before.hp_state, hots_core::FetchState::Missing);
    assert!(draft::needs_refresh(before.hp_state));

    let heroes = vec![HpHero {
        hero: "Jaina".into(),
        games: 30,
        wins: 18,
        mmr: Some(2600.0),
    }];
    db.replace_hp_heroes("Foe#1", &cfg.hp_game_type, &heroes, Some(2555.0))
        .unwrap();

    let after = draft::player_row(&db, &cfg, "Foe#1", 1, 0, 0, true).unwrap();
    assert_eq!(after.hp_state, hots_core::FetchState::Fresh);
    assert_eq!(after.mmr, Some(2555.0));
    assert_eq!(after.heroes[0].hp_games, 30);
    assert!(!draft::needs_refresh(after.hp_state));
}

#[test]
fn replacing_hp_rows_drops_the_old_ones() {
    let db = Db::open_memory().unwrap();
    let one = vec![HpHero {
        hero: "Jaina".into(),
        games: 30,
        wins: 18,
        mmr: None,
    }];
    let two = vec![HpHero {
        hero: "Raynor".into(),
        games: 5,
        wins: 1,
        mmr: None,
    }];

    db.replace_hp_heroes("Foe#1", "Storm League", &one, None)
        .unwrap();
    db.replace_hp_heroes("Foe#1", "Storm League", &two, None)
        .unwrap();

    let rows = db.hp_heroes("Foe#1", "Storm League").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].hero, "Raynor");
}

#[test]
fn reads_hero_rows_out_of_a_nested_response() {
    let body = serde_json::json!({
        "Foe#1234": {
            "Storm League": {
                "Jaina":  {"wins": 10, "losses": 5, "games_played": 15, "win_rate": 66.7, "mmr": 2600},
                "Raynor": {"wins": "3", "losses": "1", "win_rate": "75"}
            }
        }
    });

    let rows = heroes_from_json(&body);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].hero, "Jaina");
    assert_eq!((rows[0].games, rows[0].wins), (15, 10));
    assert_eq!(rows[0].mmr, Some(2600.0));
    assert_eq!(
        (rows[1].hero.as_str(), rows[1].games, rows[1].wins),
        ("Raynor", 4, 3)
    );
}

#[test]
fn reads_hero_rows_out_of_an_array_response() {
    let body = serde_json::json!([
        {"hero": "Muradin", "games_played": 8, "win_rate": 50},
        {"hero_name": "Li Li", "wins": 2, "losses": 2}
    ]);

    let rows = heroes_from_json(&body);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].hero.as_str(), rows[0].games, rows[0].wins),
        ("Muradin", 8, 4)
    );
    assert_eq!(rows[1].hero, "Li Li");
}

#[test]
fn skips_rows_with_no_games() {
    let body = serde_json::json!({"Jaina": {"wins": 0, "losses": 0}, "note": "none"});
    assert!(heroes_from_json(&body).is_empty());
}

#[test]
fn finds_the_mmr_at_any_depth() {
    let body = serde_json::json!({"Foe#1": {"Storm League": {"mmr": "2450.5"}}});
    assert_eq!(mmr_from_json(&body), Some(2450.5));
    assert_eq!(mmr_from_json(&serde_json::json!({})), None);
}

#[test]
fn round_trips_the_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let cfg = Config {
        battletag: Some("Me#1234".into()),
        hp_api_key: Some("key".into()),
        hp_ttl_days: 3,
        ..Config::default()
    };

    cfg.save_to(&path).unwrap();
    let back = Config::load_from(&path).unwrap();
    assert_eq!(back.battletag.as_deref(), Some("Me#1234"));
    assert_eq!(back.ttl_secs(), 3 * 86_400);
    assert_eq!(back.api_key().unwrap(), "key");
    assert!(
        Config::load_from(&dir.path().join("missing.toml"))
            .unwrap()
            .battletag
            .is_none()
    );
}

#[test]
fn recognises_the_game_files() {
    use hots_core::paths::{is_battlelobby, is_replay};
    use std::path::Path;

    assert!(is_replay(Path::new("/x/2024.StormReplay")));
    assert!(is_replay(Path::new("/x/2024.stormreplay")));
    assert!(!is_replay(Path::new("/x/2024.txt")));
    assert!(is_battlelobby(Path::new("/x/replay.server.battlelobby")));
    assert!(!is_battlelobby(Path::new("/x/replay.details")));
}
