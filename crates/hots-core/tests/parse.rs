use hots_core::parse;

fn lobby_bytes(region: &[u8; 2], tags: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"s2mh\x00\x00");
    out.extend_from_slice(region);
    out.extend_from_slice(&[0xff; 16]);
    for tag in tags {
        out.extend_from_slice(&[0x00, 0x21]);
        out.push((tag.len() as u8) << 1 | 1);
        out.extend_from_slice(tag.as_bytes());
        out.extend_from_slice(&[0x00, 0x00, 0x01, 0x13]);
    }
    out
}

const TEN: [&str; 10] = [
    "Ninlarr#1744",
    "GhostKnight#1319",
    "Kadajto#1386",
    "ZeekeraTron#1789",
    "slayer#1787",
    "clevername#11855",
    "sebaneitor#1407",
    "ultrapaladin#1786",
    "erevlydeux#1388",
    "Balls0fsteel#1239",
];

#[test]
fn reads_ten_battletags_in_slot_order() {
    let lobby = parse::battlelobby(&lobby_bytes(b"EU", &TEN)).unwrap();

    assert_eq!(lobby.region, 2);
    assert_eq!(lobby.players.len(), 10);
    assert_eq!(lobby.players[0].battletag, "Ninlarr#1744");
    assert_eq!(lobby.players[9].battletag, "Balls0fsteel#1239");
    assert!(lobby.players[..5].iter().all(|p| p.team == 0));
    assert!(lobby.players[5..].iter().all(|p| p.team == 1));
    assert_eq!(lobby.players[7].slot, 7);
}

#[test]
fn maps_every_gateway_to_its_api_region() {
    for (gateway, region) in [(b"US", 1), (b"EU", 2), (b"KR", 3), (b"CN", 5)] {
        let lobby = parse::battlelobby(&lobby_bytes(gateway, &TEN)).unwrap();
        assert_eq!(lobby.region, region);
    }
    assert_eq!(
        parse::battlelobby(&lobby_bytes(b"XX", &TEN))
            .unwrap()
            .region,
        0
    );
}

/// A literal `!` is 0x21, which reads as a length of 16 and steals the byte behind it.
#[test]
fn a_stray_length_byte_does_not_shift_a_battletag() {
    let tags: Vec<String> = parse::battlelobby(&lobby_bytes(b"US", &TEN))
        .unwrap()
        .players
        .into_iter()
        .map(|p| p.battletag)
        .collect();
    assert_eq!(tags.len(), 10);
    assert_eq!(tags[3], "ZeekeraTron#1789");
    assert!(tags.iter().all(|tag| !tag.starts_with('!')));
}

#[test]
fn rejects_a_lobby_it_cannot_split_in_two() {
    assert!(parse::battlelobby(&lobby_bytes(b"US", &TEN[..5])).is_err());
    assert!(parse::battlelobby(&lobby_bytes(b"US", &[TEN.as_slice(), &TEN].concat())).is_err());
    assert!(parse::battlelobby(b"nothing here").is_err());
}

#[test]
fn ignores_strings_that_only_look_like_battletags() {
    let mut noise = lobby_bytes(b"US", &TEN);
    for junk in [
        "T:52495772#804",
        "blizzmaps#1",
        "a#12",
        "toolongdiscriminator#123456789",
    ] {
        noise.push((junk.len() as u8) << 1 | 1);
        noise.extend_from_slice(junk.as_bytes());
        noise.push(0);
    }

    let lobby = parse::battlelobby(&noise).unwrap();
    assert_eq!(lobby.players.len(), 10);
}

/// Point `HOTS_TEST_REPLAY` at a `.StormReplay` to check the parser against a real file.
#[test]
fn reads_a_real_replay() {
    let Ok(path) = std::env::var("HOTS_TEST_REPLAY") else {
        return;
    };
    let record = parse::replay(std::path::Path::new(&path)).unwrap();

    assert_eq!(record.players.len(), 10);
    assert!(record.build > 0);
    assert!(!record.map.is_empty());
    assert!(record.played_at > 1_400_000_000);
    assert!(record.players.iter().all(|p| p.battletag.contains('#')));
    assert!(record.players.iter().all(|p| !p.hero.is_empty()));
    assert_eq!(record.players.iter().filter(|p| p.won).count(), 5);
    assert_eq!(record.players.iter().filter(|p| p.team == 0).count(), 5);
}
