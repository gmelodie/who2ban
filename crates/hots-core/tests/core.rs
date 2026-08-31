use hots_core::db::{Db, LocalHero};
use hots_core::{
    Config, GameMode, Lobby, LobbyPlayer, MatchPlayer, MatchRecord, PlayerNote, Toon, draft,
};

fn toon(region: u8, id: u64) -> Toon {
    Toon {
        region,
        realm: 1,
        id,
    }
}

/// Three games of one player differ by their rosters, which is what dedup keys on.
fn at(mut record: MatchRecord, minutes: i64) -> MatchRecord {
    record.played_at += minutes * 60;
    record
}

fn replay(mode: GameMode, picks: &[(&str, &str, u8, bool)]) -> MatchRecord {
    MatchRecord {
        players: picks
            .iter()
            .enumerate()
            .map(|(i, (tag, hero, team, won))| MatchPlayer {
                name: tag.split('#').next().unwrap().to_string(),
                battletag: Some(tag.to_string()),
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
        game_id: None,
    }
}

fn seeded(mut record: MatchRecord, id: u64) -> MatchRecord {
    record.game_id = Some(id);
    record
}

/// Two replays of one match, saved by two clients whose clocks disagree.
#[test]
fn one_seed_is_one_game_however_far_the_clocks_drift() {
    let db = Db::open_memory().unwrap();
    let game = replay(GameMode::StormLeague, &[("Me#1", "Raynor", 0, true)]);
    let mine = seeded(game.clone(), 1042313809);
    let theirs = seeded(at(game, 400), 1042313809);

    assert!(db.record_replay("mine.StormReplay", &mine).unwrap().is_some());
    assert!(
        db.record_replay("theirs.StormReplay", &theirs)
            .unwrap()
            .is_none(),
        "the seed the server picked outranks either clock"
    );
    assert_eq!(db.match_count().unwrap(), 1);
    assert_eq!(db.local_heroes("Me#1", true).unwrap()[0].games, 1);
}

/// The same ten people can draft the same ten heroes twice. Two seeds say they did.
#[test]
fn two_seeds_are_two_games_however_alike() {
    let db = Db::open_memory().unwrap();
    let game = replay(GameMode::StormLeague, &[("Me#1", "Raynor", 0, true)]);

    db.record_replay("a.StormReplay", &seeded(game.clone(), 1))
        .unwrap();
    assert!(
        db.record_replay("b.StormReplay", &seeded(game, 2))
            .unwrap()
            .is_some()
    );
    assert_eq!(db.match_count().unwrap(), 2);
}

/// A client too old to send a seed still lands on the row a seeded one opened.
#[test]
fn a_record_without_a_seed_joins_the_match_it_matches() {
    let db = Db::open_memory().unwrap();
    let game = replay(GameMode::StormLeague, &[("Me#1", "Raynor", 0, true)]);

    db.record_replay("seeded.StormReplay", &seeded(game.clone(), 7))
        .unwrap();
    assert!(
        db.record_replay("plain.StormReplay", &at(game, 3))
            .unwrap()
            .is_none()
    );
    assert_eq!(db.match_count().unwrap(), 1);
    assert_eq!(db.file_count().unwrap(), 2);
}

/// A replay cut short by a disconnect records ten losers. The one that saw the end wins.
#[test]
fn a_finished_replay_replaces_a_severed_one() {
    let db = Db::open_memory().unwrap();
    let severed = seeded(
        replay(
            GameMode::StormLeague,
            &[("Me#1", "Raynor", 0, false), ("Foe#2", "Jaina", 1, false)],
        ),
        99,
    );
    let whole = seeded(
        replay(
            GameMode::StormLeague,
            &[("Me#1", "Raynor", 0, true), ("Foe#2", "Jaina", 1, false)],
        ),
        99,
    );

    db.record_replay("cut.StormReplay", &severed).unwrap();
    assert_eq!(db.local_heroes("Me#1", true).unwrap()[0].wins, 0);

    assert!(db.record_replay("whole.StormReplay", &whole).unwrap().is_none());
    assert_eq!(db.match_count().unwrap(), 1);
    assert_eq!(
        db.local_heroes("Me#1", true).unwrap()[0].wins,
        1,
        "the replay that saw the end owns the result"
    );
}

#[test]
fn records_a_replay_once() {
    let db = Db::open_memory().unwrap();
    let r = replay(GameMode::StormLeague, &[("Me#1", "Raynor", 0, true)]);

    assert!(db.record_replay("a.StormReplay", &r).unwrap().is_some());
    assert!(db.record_replay("a.StormReplay", &r).unwrap().is_none());
    assert_eq!(db.match_count().unwrap(), 1);
}

/// Two people in one game each send their own file, named by their own clock.
#[test]
fn one_game_counts_once_however_many_people_send_it() {
    let db = Db::open_memory().unwrap();
    let game = replay(
        GameMode::StormLeague,
        &[("Me#1", "Raynor", 0, true), ("Friend#2", "Jaina", 0, true)],
    );

    assert!(
        db.record_replay("2026-08-30 17.05.46 Alterac.StormReplay", &game)
            .unwrap()
            .is_some()
    );
    assert!(
        db.record_replay("2026-08-30 22.05.46 Alterac.StormReplay", &game)
            .unwrap()
            .is_none(),
        "the same game under another clock is not a second game"
    );

    assert_eq!(db.match_count().unwrap(), 1);
    assert_eq!(db.file_count().unwrap(), 2, "both files are remembered");
    assert_eq!(
        db.known_replays().unwrap().len(),
        2,
        "so neither is parsed again"
    );

    let heroes = db.local_heroes("Me#1", true).unwrap();
    assert_eq!(heroes[0].games, 1, "one game, not two");
}

#[test]
fn two_different_games_stay_apart() {
    let db = Db::open_memory().unwrap();
    let one = replay(GameMode::StormLeague, &[("Me#1", "Raynor", 0, true)]);
    let mut two = one.clone();
    two.played_at += 1800;

    db.record_replay("a.StormReplay", &one).unwrap();
    assert!(db.record_replay("b.StormReplay", &two).unwrap().is_some());
    assert_eq!(db.match_count().unwrap(), 2);
    assert_eq!(db.local_heroes("Me#1", true).unwrap()[0].games, 2);
}

#[test]
fn aggregates_local_heroes_and_filters_by_mode() {
    let db = Db::open_memory().unwrap();
    let sl = GameMode::StormLeague;
    db.record_replay(
        "1.StormReplay",
        &replay(sl, &[("Foe#1", "Raynor", 1, true)]),
    )
    .unwrap();
    db.record_replay(
        "2.StormReplay",
        &at(replay(sl, &[("Foe#1", "Raynor", 1, false)]), 30),
    )
    .unwrap();
    db.record_replay(
        "3.StormReplay",
        &at(
            replay(GameMode::QuickMatch, &[("Foe#1", "Muradin", 1, true)]),
            60,
        ),
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

/// Finding a seat in the lobby ignores case, so counting the games must too.
#[test]
fn a_battletag_matches_whatever_its_case() {
    let db = Db::open_memory().unwrap();
    db.record_replay(
        "1.StormReplay",
        &replay(GameMode::StormLeague, &[("Foe#1", "Raynor", 1, true)]),
    )
    .unwrap();

    assert_eq!(db.local_heroes("foe#1", true).unwrap().len(), 1);
    assert_eq!(db.local_heroes("FOE#1", true).unwrap().len(), 1);
}

/// The desktop app and the cli both write this key into `replay_files`.
#[test]
fn the_same_replay_keys_the_same_from_either_folder() {
    let a = hots_core::ingest::replay_key(std::path::Path::new("/home/me/Replays/g.StormReplay"));
    let b = hots_core::ingest::replay_key(std::path::Path::new("/mnt/backup/g.StormReplay"));
    assert_eq!(a, "g.StormReplay");
    assert_eq!(a, b);
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

    let view = draft::build(&db, &cfg, &lobby(), None).unwrap();
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

    let view = draft::build(&db, &Config::default(), &lobby(), None).unwrap();
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

    let view = draft::build(&db, &cfg, &lobby(), None).unwrap();
    assert_eq!(view.my_team, None);
    assert_eq!(view.enemies().count(), 0);
    assert_eq!(view.players.len(), 10);
}

#[test]
fn round_trips_the_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let cfg = Config {
        battletag: Some("Me#1234".into()),
        max_heroes: 4,
        local_all_modes: false,
        ..Config::default()
    };

    cfg.save_to(&path).unwrap();
    let back = Config::load_from(&path).unwrap();
    assert_eq!(back.battletag.as_deref(), Some("Me#1234"));
    assert_eq!(back.max_heroes, 4);
    assert!(!back.local_all_modes);
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

fn lutris_prefix(home: &std::path::Path, user: &str) -> std::path::PathBuf {
    let prefix = home
        .join("Games/heroes-of-the-storm/drive_c/users")
        .join(user);
    std::fs::create_dir_all(prefix.join("Temp/Heroes of the Storm")).unwrap();
    std::fs::create_dir_all(prefix.join(
        "Documents/Heroes of the Storm/Accounts/1234567/1-Hero-1-1234567/Replays/Multiplayer",
    ))
    .unwrap();
    prefix
}

#[test]
fn finds_the_folders_of_a_lutris_prefix() {
    use hots_core::paths::{wine_replay_dirs, wine_temp_roots};

    let home = tempfile::tempdir().unwrap();
    lutris_prefix(home.path(), "gabe");
    std::fs::create_dir_all(
        home.path()
            .join("Games/heroes-of-the-storm/drive_c/users/Public"),
    )
    .unwrap();

    let temps = wine_temp_roots(Some(home.path()));
    assert_eq!(temps.len(), 1);
    assert!(temps[0].ends_with("users/gabe/Temp/Heroes of the Storm"));

    let replays = wine_replay_dirs(Some(home.path()));
    assert_eq!(replays.len(), 1);
    assert!(replays[0].ends_with("1-Hero-1-1234567/Replays/Multiplayer"));
}

/// The layout of a real box: a Lutris prefix under ~/Games plus a bare one named by hand.
#[test]
fn finds_both_prefixes_of_a_split_install() {
    use hots_core::paths::{wine_replay_dirs, wine_temp_roots};

    let home = tempfile::tempdir().unwrap();
    let lutris = home.path().join("Games/battlenet/drive_c/users/steamuser");
    let bare = home.path().join("wine32/drive_c/users/steamuser");
    std::fs::create_dir_all(
        lutris.join("AppData/Local/Temp/Heroes of the Storm/TempWriteReplayP1"),
    )
    .unwrap();
    for account in ["2-Hero-1-11944033", "98-Hero-1-687994"] {
        std::fs::create_dir_all(lutris.join(format!(
            "Documents/Heroes of the Storm/Accounts/77009925/{account}/Replays/Multiplayer"
        )))
        .unwrap();
    }
    std::fs::create_dir_all(bare.join(
        "Documents/Heroes of the Storm/Accounts/77009925/1-Hero-1-168611/Replays/Multiplayer",
    ))
    .unwrap();

    let temps = wine_temp_roots(Some(home.path()));
    assert_eq!(temps.len(), 1);
    assert!(temps[0].ends_with("AppData/Local/Temp/Heroes of the Storm"));

    let replays = wine_replay_dirs(Some(home.path()));
    assert_eq!(replays.len(), 3, "every prefix contributes its history");
    assert!(replays.iter().any(|p| p.starts_with(&lutris)));
    assert!(replays.iter().any(|p| p.starts_with(&bare)));
}

#[test]
fn finds_the_folders_of_a_bottle() {
    use hots_core::paths::wine_temp_roots;

    let home = tempfile::tempdir().unwrap();
    let bottle = home.path().join(".local/share/bottles/bottles/hots");
    std::fs::create_dir_all(bottle.join("drive_c/users/steamuser/Temp/Heroes of the Storm"))
        .unwrap();

    let temps = wine_temp_roots(Some(home.path()));
    assert_eq!(temps.len(), 1);
    assert!(temps[0].ends_with("users/steamuser/Temp/Heroes of the Storm"));
}

/// Windows: Documents lands in OneDrive on many boxes, and the game may have used both.
#[test]
fn finds_documents_on_either_side_of_a_onedrive_move() {
    use hots_core::paths::{home_replay_dirs, home_temp_roots};

    let home = tempfile::tempdir().unwrap();
    for docs in ["Documents", "OneDrive/Documents"] {
        std::fs::create_dir_all(home.path().join(format!(
            "{docs}/Heroes of the Storm/Accounts/77009925/1-Hero-1-168611/Replays/Multiplayer"
        )))
        .unwrap();
    }
    std::fs::create_dir_all(home.path().join("AppData/Local/Temp/Heroes of the Storm")).unwrap();

    let replays = home_replay_dirs(Some(home.path()));
    assert_eq!(replays.len(), 2);
    assert!(
        replays
            .iter()
            .any(|p| p.starts_with(home.path().join("OneDrive")))
    );

    let temps = home_temp_roots(Some(home.path()));
    assert_eq!(temps.len(), 1);
    assert!(temps[0].ends_with("AppData/Local/Temp/Heroes of the Storm"));
}

#[test]
fn finds_nothing_without_a_prefix() {
    let home = tempfile::tempdir().unwrap();
    assert!(hots_core::paths::wine_temp_roots(Some(home.path())).is_empty());
    assert!(hots_core::paths::wine_replay_dirs(Some(home.path())).is_empty());
    assert!(hots_core::paths::home_replay_dirs(Some(home.path())).is_empty());
    assert!(hots_core::paths::home_temp_roots(Some(home.path())).is_empty());
    assert!(hots_core::paths::wine_temp_roots(None).is_empty());
}

#[test]
fn ranks_the_most_played_hero_first() {
    let local = vec![
        LocalHero {
            hero: "Jaina".into(),
            games: 3,
            wins: 3,
        },
        LocalHero {
            hero: "Raynor".into(),
            games: 9,
            wins: 4,
        },
    ];

    let rows = draft::hero_rows(local.clone(), 8);
    assert_eq!(rows[0].hero, "Raynor");
    assert_eq!((rows[0].games, rows[0].wins), (9, 4));
    assert_eq!(rows[0].winrate(), Some(4.0 / 9.0));
    assert_eq!(draft::hero_rows(local, 1).len(), 1);
}

#[test]
fn counts_every_stored_game_of_a_player() {
    let db = Db::open_memory().unwrap();
    let cfg = Config::default();
    db.record_replay(
        "1.StormReplay",
        &replay(GameMode::StormLeague, &[("Foe#1", "Raynor", 1, true)]),
    )
    .unwrap();

    let row = draft::player_row(&db, &cfg, "Foe#1", 5, 1, true).unwrap();
    assert_eq!(row.games, 1);
    assert_eq!(row.heroes.len(), 1);
    assert!(row.enemy);

    let unknown = draft::player_row(&db, &cfg, "Nobody#9", 0, 0, false).unwrap();
    assert_eq!(unknown.games, 0);
    assert!(unknown.heroes.is_empty());
}

/// The same database serves several people, so who is asking travels with the request.
#[test]
fn each_browser_says_who_it_is() {
    let db = Db::open_memory().unwrap();
    let cfg = Config::default();

    let mine = draft::build(&db, &cfg, &lobby(), Some("P7#1007")).unwrap();
    assert_eq!(mine.my_team, Some(1));

    let theirs = draft::build(&db, &cfg, &lobby(), Some("P2#1002")).unwrap();
    assert_eq!(theirs.my_team, Some(0));
    assert!(theirs.enemies().all(|p| p.team == 1));
}

/// A replay whose battlelobby would not scan still counts, under the short name.
#[test]
fn falls_back_to_the_short_name() {
    let db = Db::open_memory().unwrap();
    let mut record = replay(GameMode::StormLeague, &[("Foe#1234", "Raynor", 1, true)]);
    record.players[0].battletag = None;
    db.record_replay("1.StormReplay", &record).unwrap();

    let by_tag = db.local_heroes("Foe#1234", true).unwrap();
    assert_eq!(by_tag.len(), 1, "a lobby battletag finds the nameless row");
    assert_eq!(by_tag[0].hero, "Raynor");

    assert!(db.local_heroes("Other#1", true).unwrap().is_empty());
}

#[test]
fn an_exact_battletag_wins_over_the_name() {
    let db = Db::open_memory().unwrap();
    db.record_replay(
        "1.StormReplay",
        &replay(GameMode::StormLeague, &[("Foe#1111", "Jaina", 1, true)]),
    )
    .unwrap();

    assert_eq!(db.local_heroes("Foe#1111", true).unwrap().len(), 1);
    assert!(
        db.local_heroes("Foe#2222", true).unwrap().is_empty(),
        "a stored battletag is not matched by name"
    );
}

/// With the game closed the temp folder is gone, so `Temp` itself has to be enough.
#[test]
fn finds_the_temp_folder_of_a_closed_game() {
    use hots_core::paths::{temp_beside, wine_temp_roots};

    let home = tempfile::tempdir().unwrap();
    let user = home.path().join("Games/battlenet/drive_c/users/steamuser");
    std::fs::create_dir_all(user.join("AppData/Local/Temp")).unwrap();
    let replays =
        user.join("Documents/Heroes of the Storm/Accounts/77009925/1-Hero-1-1/Replays/Multiplayer");
    std::fs::create_dir_all(&replays).unwrap();

    let beside = temp_beside(&replays).unwrap();
    assert_eq!(beside, user.join("AppData/Local/Temp/Heroes of the Storm"));
    assert!(!beside.exists(), "the game has not created it yet");

    let found = wine_temp_roots(Some(home.path()));
    assert_eq!(found.first(), Some(&beside));
}

#[test]
fn prefers_a_temp_folder_that_is_already_there() {
    use hots_core::paths::wine_temp_roots;

    let home = tempfile::tempdir().unwrap();
    let user = home.path().join("Games/battlenet/drive_c/users/steamuser");
    std::fs::create_dir_all(user.join("AppData/Local/Temp")).unwrap();
    std::fs::create_dir_all(user.join("Temp/Heroes of the Storm")).unwrap();

    let found = wine_temp_roots(Some(home.path()));
    assert_eq!(found.first(), Some(&user.join("Temp/Heroes of the Storm")));
    assert_eq!(found.len(), 2, "the empty one stays as a second chance");
}

#[test]
fn ignores_a_replay_folder_outside_a_prefix() {
    use hots_core::paths::temp_beside;
    assert!(
        temp_beside(std::path::Path::new(
            "/home/me/Documents/Heroes of the Storm"
        ))
        .is_none()
    );
}

/// An update must not cost anyone their history.
#[test]
fn a_database_survives_being_reopened() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hots.db");

    let db = Db::open(&path).unwrap();
    db.record_replay(
        "1.StormReplay",
        &replay(GameMode::StormLeague, &[("Foe#1", "Raynor", 1, true)]),
    )
    .unwrap();
    drop(db);

    for _ in 0..3 {
        let db = Db::open(&path).unwrap();
        assert_eq!(db.match_count().unwrap(), 1, "the rows are still there");
        assert_eq!(db.local_heroes("Foe#1", true).unwrap().len(), 1);
    }
    assert!(
        !path.with_extension("before-v3.db").exists(),
        "nothing to back up"
    );
}

/// Names repeat. Your own battletag decides your side, not the first seat that shares a name.
#[test]
fn an_exact_battletag_outranks_an_earlier_namesake() {
    let db = Db::open_memory().unwrap();
    let lobby = Lobby {
        region: 1,
        players: (0..10)
            .map(|i| LobbyPlayer {
                battletag: match i {
                    0 => "Nova#1111".to_string(),
                    7 => "Nova#2222".to_string(),
                    n => format!("Seat{n}#{n}"),
                },
                team: (i >= 5) as u8,
                slot: i as u8,
            })
            .collect(),
    };

    let draft = draft::build(&db, &Config::default(), &lobby, Some("Nova#2222")).unwrap();
    assert_eq!(draft.my_team, Some(1), "slot 7 is mine, slot 0 is a stranger");
    assert!(
        draft
            .players
            .iter()
            .any(|p| p.battletag == "Nova#1111" && p.enemy)
    );
}

/// One credential guards the server, so a note is the group's note and the last word wins.
#[test]
fn a_note_and_a_verdict_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hots.db");

    {
        let db = Db::open(&path).unwrap();
        assert_eq!(db.note("Foe#1").unwrap(), PlayerNote::default());
        db.set_note(
            "Foe#1",
            &PlayerNote {
                note: "throws when behind".to_string(),
                verdict: -1,
            },
        )
        .unwrap();
    }

    let db = Db::open(&path).unwrap();
    let kept = db.note("Foe#1").unwrap();
    assert_eq!(kept.note, "throws when behind");
    assert_eq!(kept.verdict, -1);
    assert_eq!(
        db.note("foe#1").unwrap().verdict,
        -1,
        "a battletag is a battletag whatever its case"
    );
}

/// Taking back both the words and the verdict forgets the player entirely.
#[test]
fn clearing_a_note_removes_it() {
    let db = Db::open_memory().unwrap();
    db.set_note(
        "Foe#1",
        &PlayerNote {
            note: "rude".to_string(),
            verdict: 1,
        },
    )
    .unwrap();
    db.set_note("Foe#1", &PlayerNote::default()).unwrap();
    assert_eq!(db.note("Foe#1").unwrap(), PlayerNote::default());
}

/// A draft carries what is known about each seat, so the window asks the server once.
#[test]
fn a_draft_carries_the_notes() {
    let db = Db::open_memory().unwrap();
    db.set_note(
        "Foe#1",
        &PlayerNote {
            note: "picks Abathur every time".to_string(),
            verdict: -1,
        },
    )
    .unwrap();

    let lobby = Lobby {
        region: 1,
        players: (0..10)
            .map(|i| LobbyPlayer {
                battletag: match i {
                    0 => "Foe#1".to_string(),
                    n => format!("Seat{n}#{n}"),
                },
                team: (i >= 5) as u8,
                slot: i as u8,
            })
            .collect(),
    };
    let draft = draft::build(&db, &Config::default(), &lobby, Some("Seat5#5")).unwrap();
    let seat = draft
        .players
        .iter()
        .find(|p| p.battletag == "Foe#1")
        .unwrap();
    assert_eq!(seat.note.note, "picks Abathur every time");
    assert_eq!(seat.note.verdict, -1);
    assert!(seat.enemy, "and it is the enemy who is worth a note");
}

/// A shape this build cannot read is copied aside, never deleted in place.
#[test]
fn an_older_database_leaves_a_copy_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hots.db");
    Db::open(&path).unwrap();

    let old = rusqlite::Connection::open(&path).unwrap();
    old.pragma_update(None, "user_version", 1).unwrap();
    drop(old);

    Db::open(&path).unwrap();
    let backup = path.with_extension("before-v5.db");
    assert!(backup.exists(), "the old file is kept");
    assert!(std::fs::metadata(&backup).unwrap().len() > 0);
}

#[test]
fn a_newer_database_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hots.db");
    Db::open(&path).unwrap();

    let ahead = rusqlite::Connection::open(&path).unwrap();
    ahead.pragma_update(None, "user_version", 99).unwrap();
    drop(ahead);

    assert!(Db::open(&path).is_err(), "a newer database is not touched");
}
