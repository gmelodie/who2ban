//! Everything here runs against banners cut from one real draft: a 3840 by 2160 capture
//! of a Storm League draft, five names on the blue side and five on the red.

use std::path::PathBuf;

use w2b_glyph::{Atlas, name};

/// The ten seats of that draft, as the battlelobby spelled them at load.
const BANNERS: [(&str, &str); 10] = [
    ("sagelion", "SageLion"),
    ("nickstar28", "NickStar28"),
    ("noheroe", "noheroe"),
    ("geemelodie", "geemelodie"),
    ("eumesmo", "eumesmo"),
    ("judicante", "Judicante"),
    ("ajiwajim2", "AJIWAJIM2"),
    ("trollmllaman", "Trollmllaman"),
    ("matheusdasilva", "Matheus da silva"),
    ("gabrielvargas", "Gabriel Vargas"),
];

fn banner(stem: &str) -> (Vec<u8>, usize, usize) {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures"]
        .iter()
        .collect::<PathBuf>()
        .join(format!("{stem}.png"));
    let img = image::open(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .to_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    (img.into_raw(), w, h)
}

fn letters_without_spaces(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace()).count()
}

#[test]
fn a_banner_is_cut_into_its_letters() {
    let mut exact = 0;
    for (stem, truth) in BANNERS {
        let (rgb, w, h) = banner(stem);
        let (found, angle) = w2b_glyph::letters(&rgb, w, h).expect(stem);
        // Both banners lean the same amount, the two teams in opposite directions.
        assert!(
            (25.0..35.0).contains(&angle.abs()),
            "{stem}: banner angle {angle}"
        );
        if found.len() == letters_without_spaces(truth) {
            exact += 1;
        }
    }
    assert!(exact >= 9, "only {exact} of 10 banners cut cleanly");
}

/// The real test of the shapes: learn nine names, then read the tenth, which shares no
/// single drawn letter with what was learned.
#[test]
fn a_name_is_read_from_the_shapes_of_the_others() {
    let mut correct = 0;
    let mut total = 0;

    for (held, truth) in BANNERS {
        let mut atlas = Atlas::new();
        for (stem, other) in BANNERS {
            if stem == held {
                continue;
            }
            let (rgb, w, h) = banner(stem);
            w2b_glyph::learn(&rgb, w, h, other, &mut atlas);
        }
        let (rgb, w, h) = banner(held);
        let Some(reading) = w2b_glyph::read(&rgb, w, h, &atlas) else {
            continue;
        };
        let wanted: String = truth.chars().filter(|c| !c.is_whitespace()).collect();
        if reading.text.chars().count() != wanted.chars().count() {
            continue;
        }
        for (got, want) in reading.text.chars().zip(wanted.chars()) {
            if got == w2b_glyph::UNREAD {
                continue; // a letter the atlas has never been shown
            }
            total += 1;
            correct += usize::from(got == want);
        }
    }
    assert!(total > 20, "only {total} letters were read at all");
    let share = correct as f32 / total as f32;
    assert!(share >= 0.88, "letters read correctly: {correct}/{total}");
}

#[test]
fn a_read_that_is_nearly_right_still_finds_the_player() {
    let pool = vec![
        ("geemelodie".to_string(), "geemelodie#1711".to_string()),
        ("eumesmo".to_string(), "eumesmo#1338".to_string()),
        ("Quaresma".to_string(), "Quaresma#2211".to_string()),
        ("gameleque".to_string(), "gameleque#1102".to_string()),
    ];
    // One letter misread out of ten, which is what a thin atlas does.
    let found = name::identify("geemelodle", &pool).expect("a near miss is still the player");
    assert_eq!(found.battletag, "geemelodie#1711");
    assert!(found.score <= 0.2, "score {}", found.score);
}

#[test]
fn a_read_too_poor_to_place_names_nobody() {
    let pool = vec![
        ("ATARI".to_string(), "ATARI#1234".to_string()),
        ("Arthas".to_string(), "Arthas#4321".to_string()),
        ("Chami".to_string(), "Chami#1000".to_string()),
    ];
    // Rubbish out of the segmenter, and a read of nothing but unplaced letters. Neither
    // may be rounded to whoever happens to be closest.
    for junk in ["th", "N", "Cmi", "xqzvw", "?????"] {
        assert!(
            name::identify(junk, &pool).is_none(),
            "{junk:?} was matched to a player"
        );
    }
}

#[test]
fn two_players_who_fit_equally_well_name_neither() {
    let pool = vec![
        ("Marco".to_string(), "Marco#1".to_string()),
        ("Marca".to_string(), "Marca#2".to_string()),
    ];
    assert!(name::identify("Marcx", &pool).is_none());
}

#[test]
fn a_banner_whose_letters_do_not_add_up_teaches_nothing() {
    let mut atlas = Atlas::new();
    let (rgb, w, h) = banner("geemelodie");
    // The banner says ten letters; claiming it says three must not file nine wrong.
    assert!(!w2b_glyph::learn(&rgb, w, h, "abc", &mut atlas));
    assert_eq!(atlas.examples(), 0);
}

#[test]
fn a_poor_read_that_can_only_be_one_player_still_names_them() {
    // A thin atlas leaves holes, so the score is bad and the answer is still certain:
    // nobody else in the pool is anywhere near. Judging this on the score alone threw
    // away answers that were both correct and unambiguous.
    let pool = vec![
        ("Matheusdasilva".to_string(), "Matheusdasilva#1".to_string()),
        ("HeroesdelaLU".to_string(), "HeroesdelaLU#2".to_string()),
        ("Elvendaval01".to_string(), "Elvendaval01#3".to_string()),
    ];
    let found = name::identify("??t?e?sd?s?l??", &pool).expect("only one player fits");
    assert_eq!(found.battletag, "Matheusdasilva#1");
    assert!(found.score > name::MIN_MARGIN, "the score is meant to be poor here");
    assert!(found.margin >= name::MIN_MARGIN, "margin {}", found.margin);
}

#[test]
fn a_read_of_the_wrong_length_is_not_a_near_miss() {
    // Three shapes where a name has five is not a poor read of that name: the segmenter
    // does not drop two letters in five. Without this, short fragments match short names.
    let pool = vec![
        ("Chami".to_string(), "Chami#1".to_string()),
        ("Zzzzzzzzzz".to_string(), "Zzzzzzzzzz#2".to_string()),
    ];
    assert!(name::identify("Cmi", &pool).is_none());
    // The same letters at the right length are fine.
    assert!(name::identify("Chami", &pool).is_some());
}

#[test]
fn a_read_that_is_mostly_holes_names_nobody() {
    let pool = vec![
        ("ATARI".to_string(), "ATARI#1".to_string()),
        ("Bravo".to_string(), "Bravo#2".to_string()),
    ];
    assert!(!name::legible("?????"));
    assert!(name::identify("?????", &pool).is_none());
}
