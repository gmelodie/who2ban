//! Hero portraits, bundled so the recap needs neither the network nor an installed
//! game to put a face next to a battletag. Generated from the heroes-talents data set.

/// Unit id as a replay spells it, the English name, the portrait file stem, and the
/// image itself. The unit id is what `MatchPlayer::hero_id` carries: `HeroCrusader`
/// whatever language the client that saved the replay was set to.
#[allow(clippy::type_complexity)]
static PORTRAITS: &[(&str, &str, &str, &[u8])] = &[
    ("HeroAbathur", "Abathur", "abathur", include_bytes!("../assets/heroes/abathur.png")),
    ("HeroAlarak", "Alarak", "alarak", include_bytes!("../assets/heroes/alarak.png")),
    ("HeroAlexstrasza", "Alexstrasza", "alexstrasza", include_bytes!("../assets/heroes/alexstrasza.png")),
    ("HeroAmazon", "Cassia", "cassia", include_bytes!("../assets/heroes/cassia.png")),
    ("HeroAna", "Ana", "ana", include_bytes!("../assets/heroes/ana.png")),
    ("HeroAnduin", "Anduin", "anduin", include_bytes!("../assets/heroes/anduin.png")),
    ("HeroAnubarak", "Anub'arak", "anubarak", include_bytes!("../assets/heroes/anubarak.png")),
    ("HeroArtanis", "Artanis", "artanis", include_bytes!("../assets/heroes/artanis.png")),
    ("HeroArthas", "Arthas", "arthas", include_bytes!("../assets/heroes/arthas.png")),
    ("HeroAuriel", "Auriel", "auriel", include_bytes!("../assets/heroes/auriel.png")),
    ("HeroAzmodan", "Azmodan", "azmodan", include_bytes!("../assets/heroes/azmodan.png")),
    ("HeroBarbarian", "Sonya", "sonya", include_bytes!("../assets/heroes/sonya.png")),
    ("HeroButcher", "The Butcher", "thebutcher", include_bytes!("../assets/heroes/thebutcher.png")),
    ("HeroChen", "Chen", "chen", include_bytes!("../assets/heroes/chen.png")),
    ("HeroCho", "Cho", "chogall", include_bytes!("../assets/heroes/chogall.png")),
    ("HeroChromie", "Chromie", "chromie", include_bytes!("../assets/heroes/chromie.png")),
    ("HeroCrusader", "Johanna", "johanna", include_bytes!("../assets/heroes/johanna.png")),
    ("HeroDVaMech", "D.Va", "dva", include_bytes!("../assets/heroes/dva.png")),
    ("HeroDeathwing", "Deathwing", "deathwing", include_bytes!("../assets/heroes/deathwing.png")),
    ("HeroDeckard", "Deckard", "deckard", include_bytes!("../assets/heroes/deckard.png")),
    ("HeroDehaka", "Dehaka", "dehaka", include_bytes!("../assets/heroes/dehaka.png")),
    ("HeroDemonHunter", "Valla", "valla", include_bytes!("../assets/heroes/valla.png")),
    ("HeroDiablo", "Diablo", "diablo", include_bytes!("../assets/heroes/diablo.png")),
    ("HeroDryad", "Lunara", "lunara", include_bytes!("../assets/heroes/lunara.png")),
    ("HeroFaerieDragon", "Brightwing", "brightwing", include_bytes!("../assets/heroes/brightwing.png")),
    ("HeroFalstad", "Falstad", "falstad", include_bytes!("../assets/heroes/falstad.png")),
    ("HeroFenix", "Fenix", "fenix", include_bytes!("../assets/heroes/fenix.png")),
    ("HeroFirebat", "Blaze", "blaze", include_bytes!("../assets/heroes/blaze.png")),
    ("HeroGall", "Gall", "gall", include_bytes!("../assets/heroes/gall.png")),
    ("HeroGarrosh", "Garrosh", "garrosh", include_bytes!("../assets/heroes/garrosh.png")),
    ("HeroGenji", "Genji", "genji", include_bytes!("../assets/heroes/genji.png")),
    ("HeroGreymane", "Greymane", "greymane", include_bytes!("../assets/heroes/greymane.png")),
    ("HeroGuldan", "Gul'dan", "guldan", include_bytes!("../assets/heroes/guldan.png")),
    ("HeroHanzo", "Hanzo", "hanzo", include_bytes!("../assets/heroes/hanzo.png")),
    ("HeroHogger", "Hogger", "hogger", include_bytes!("../assets/heroes/hogger.png")),
    ("HeroIllidan", "Illidan", "illidan", include_bytes!("../assets/heroes/illidan.png")),
    ("HeroImperius", "Imperius", "imperius", include_bytes!("../assets/heroes/imperius.png")),
    ("HeroJaina", "Jaina", "jaina", include_bytes!("../assets/heroes/jaina.png")),
    ("HeroJunkrat", "Junkrat", "junkrat", include_bytes!("../assets/heroes/junkrat.png")),
    ("HeroKaelthas", "Kael'thas", "kaelthas", include_bytes!("../assets/heroes/kaelthas.png")),
    ("HeroKelThuzad", "Kel'Thuzad", "kelthuzad", include_bytes!("../assets/heroes/kelthuzad.png")),
    ("HeroKerrigan", "Kerrigan", "kerrigan", include_bytes!("../assets/heroes/kerrigan.png")),
    ("HeroL90ETC", "E.T.C.", "etc", include_bytes!("../assets/heroes/etc.png")),
    ("HeroLeoric", "Leoric", "leoric", include_bytes!("../assets/heroes/leoric.png")),
    ("HeroLiLi", "Li Li", "lili", include_bytes!("../assets/heroes/lili.png")),
    ("HeroLostVikingsController", "The Lost Vikings", "lostvikings", include_bytes!("../assets/heroes/lostvikings.png")),
    ("HeroLucio", "Lúcio", "lucio", include_bytes!("../assets/heroes/lucio.png")),
    ("HeroMaiev", "Maiev", "maiev", include_bytes!("../assets/heroes/maiev.png")),
    ("HeroMalGanis", "Mal'Ganis", "malganis", include_bytes!("../assets/heroes/malganis.png")),
    ("HeroMalfurion", "Malfurion", "malfurion", include_bytes!("../assets/heroes/malfurion.png")),
    ("HeroMalthael", "Malthael", "malthael", include_bytes!("../assets/heroes/malthael.png")),
    ("HeroMedic", "Lt. Morales", "ltmorales", include_bytes!("../assets/heroes/ltmorales.png")),
    ("HeroMedivh", "Medivh", "medivh", include_bytes!("../assets/heroes/medivh.png")),
    ("HeroMeiOW", "Mei", "mei", include_bytes!("../assets/heroes/mei.png")),
    ("HeroMephisto", "Mephisto", "mephisto", include_bytes!("../assets/heroes/mephisto.png")),
    ("HeroMonk", "Kharazim", "kharazim", include_bytes!("../assets/heroes/kharazim.png")),
    ("HeroMuradin", "Muradin", "muradin", include_bytes!("../assets/heroes/muradin.png")),
    ("HeroMurky", "Murky", "murky", include_bytes!("../assets/heroes/murky.png")),
    ("HeroNecromancer", "Xul", "xul", include_bytes!("../assets/heroes/xul.png")),
    ("HeroNexusHunter", "Qhira", "qhira", include_bytes!("../assets/heroes/qhira.png")),
    ("HeroNova", "Nova", "nova", include_bytes!("../assets/heroes/nova.png")),
    ("HeroOrphea", "Orphea", "orphea", include_bytes!("../assets/heroes/orphea.png")),
    ("HeroProbius", "Probius", "probius", include_bytes!("../assets/heroes/probius.png")),
    ("HeroRagnaros", "Ragnaros", "ragnaros", include_bytes!("../assets/heroes/ragnaros.png")),
    ("HeroRaynor", "Raynor", "raynor", include_bytes!("../assets/heroes/raynor.png")),
    ("HeroRehgar", "Rehgar", "rehgar", include_bytes!("../assets/heroes/rehgar.png")),
    ("HeroRexxar", "Rexxar", "rexxar", include_bytes!("../assets/heroes/rexxar.png")),
    ("HeroSamuro", "Samuro", "samuro", include_bytes!("../assets/heroes/samuro.png")),
    ("HeroSgtHammer", "Sgt. Hammer", "sgthammer", include_bytes!("../assets/heroes/sgthammer.png")),
    ("HeroStitches", "Stitches", "stitches", include_bytes!("../assets/heroes/stitches.png")),
    ("HeroStukov", "Stukov", "stukov", include_bytes!("../assets/heroes/stukov.png")),
    ("HeroSylvanas", "Sylvanas", "sylvanas", include_bytes!("../assets/heroes/sylvanas.png")),
    ("HeroTassadar", "Tassadar", "tassadar", include_bytes!("../assets/heroes/tassadar.png")),
    ("HeroThrall", "Thrall", "thrall", include_bytes!("../assets/heroes/thrall.png")),
    ("HeroTinker", "Gazlowe", "gazlowe", include_bytes!("../assets/heroes/gazlowe.png")),
    ("HeroTracer", "Tracer", "tracer", include_bytes!("../assets/heroes/tracer.png")),
    ("HeroTychus", "Tychus", "tychus", include_bytes!("../assets/heroes/tychus.png")),
    ("HeroTyrael", "Tyrael", "tyrael", include_bytes!("../assets/heroes/tyrael.png")),
    ("HeroTyrande", "Tyrande", "tyrande", include_bytes!("../assets/heroes/tyrande.png")),
    ("HeroUther", "Uther", "uther", include_bytes!("../assets/heroes/uther.png")),
    ("HeroValeera", "Valeera", "valeera", include_bytes!("../assets/heroes/valeera.png")),
    ("HeroVarian", "Varian", "varian", include_bytes!("../assets/heroes/varian.png")),
    ("HeroWhitemane", "Whitemane", "whitemane", include_bytes!("../assets/heroes/whitemane.png")),
    ("HeroWitchDoctor", "Nazeebo", "nazeebo", include_bytes!("../assets/heroes/nazeebo.png")),
    ("HeroWizard", "Li-Ming", "liming", include_bytes!("../assets/heroes/liming.png")),
    ("HeroYrel", "Yrel", "yrel", include_bytes!("../assets/heroes/yrel.png")),
    ("HeroZagara", "Zagara", "zagara", include_bytes!("../assets/heroes/zagara.png")),
    ("HeroZarya", "Zarya", "zarya", include_bytes!("../assets/heroes/zarya.png")),
    ("HeroZeratul", "Zeratul", "zeratul", include_bytes!("../assets/heroes/zeratul.png")),
    ("HeroZuljin", "Zul'jin", "zuljin", include_bytes!("../assets/heroes/zuljin.png")),
];

/// The stem is the caller's cache key: egui loads an image once per URI, so two cards on
/// the same hero must name the same one.
pub fn portrait(hero_id: Option<&str>, hero: &str) -> Option<(&'static str, &'static [u8])> {
    // The spelling is the language of whichever client saved the replay, so it is only
    // worth trying once the id that means the same thing everywhere has come up empty.
    let by_id = hero_id.and_then(|id| {
        PORTRAITS
            .iter()
            .find(|(unit, _, _, _)| unit.eq_ignore_ascii_case(id))
    });
    by_id
        .or_else(|| {
            PORTRAITS
                .iter()
                .find(|(_, name, _, _)| name.eq_ignore_ascii_case(hero))
        })
        .map(|(_, _, stem, bytes)| (*stem, *bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hero_carries_an_image() {
        assert_eq!(PORTRAITS.len(), 90);
        assert!(PORTRAITS.iter().all(|(_, _, _, bytes)| !bytes.is_empty()));
    }

    #[test]
    fn the_unit_id_is_read_before_a_translated_spelling() {
        // A German client spells Johanna's hero name its own way; the id does not move.
        let (stem, _) = portrait(Some("HeroCrusader"), "Johanna der Kreuzritter").unwrap();
        assert_eq!(stem, "johanna");
    }

    #[test]
    fn a_spelling_still_answers_when_the_replay_carried_no_id() {
        let (stem, _) = portrait(None, "Johanna").unwrap();
        assert_eq!(stem, "johanna");
    }

    #[test]
    fn a_hero_this_build_has_never_heard_of_is_not_guessed_at() {
        assert!(portrait(Some("HeroWhoeverIsNext"), "Whoever Is Next").is_none());
    }
}
