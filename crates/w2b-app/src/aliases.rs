//! The names the draft screen writes instead of battletags.
//!
//! The client draws a Real ID friend's real name on their banner where everyone else
//! gets their battletag, so the screen says `Gabriel Vargas` and the battlelobby says
//! `Varguitos`. Those are the seats worth reading most: the people played with are the
//! people on record. Without a mapping between the two the reader can never place them,
//! and `harvest` can never file their banner, because the letters it is holding are not
//! the letters of the name it is told the seat holds.
//!
//! Kept on this machine and never sent anywhere. Everything else the client learns goes
//! to the shared pool, because a letter's shape belongs to the game's font and to nobody
//! in particular. A real name against a battletag is the opposite of that: it is a fact
//! about a person, learned from one player's friend list, and it stays there.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What the screen calls somebody, against who they are.
#[derive(Default)]
pub struct Aliases {
    /// Drawn name to battletag. Keyed by the drawn name because that is what a read
    /// produces and what a lookup has to start from.
    by_drawn: BTreeMap<String, String>,
    path: PathBuf,
    unsaved: bool,
}

impl Aliases {
    pub fn load(path: &Path) -> Aliases {
        let by_drawn = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Aliases {
            by_drawn,
            path: path.to_path_buf(),
            unsaved: false,
        }
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.by_drawn.len()
    }

    /// Every mapping, as `candidates` wants them: the name on the banner, and the
    /// battletag a read of that name means.
    pub fn pairs(&self) -> impl Iterator<Item = (String, String)> + '_ {
        self.by_drawn
            .iter()
            .map(|(drawn, tag)| (drawn.clone(), tag.clone()))
    }

    /// Note that this banner is this player. `true` when that was news.
    ///
    /// A person has one real name, so any other drawn name already pointing at this
    /// battletag goes: a friend who changes what Blizzard shows should not leave the old
    /// spelling behind to be matched against forever.
    pub fn record(&mut self, drawn: &str, battletag: &str) -> bool {
        if self.by_drawn.get(drawn).is_some_and(|had| had == battletag) {
            return false;
        }
        self.by_drawn
            .retain(|had, tag| tag != battletag || had == drawn);
        self.by_drawn
            .insert(drawn.to_string(), battletag.to_string());
        self.unsaved = true;
        true
    }

    /// Written only when there is something new in it.
    pub fn save(&mut self) -> std::io::Result<()> {
        if !self.unsaved {
            return Ok(());
        }
        let json = serde_json::to_vec_pretty(&self.by_drawn)?;
        std::fs::write(&self.path, json)?;
        self.unsaved = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases() -> Aliases {
        Aliases::load(Path::new("/nonexistent/aliases.json"))
    }

    #[test]
    fn a_drawn_name_finds_the_player_it_belongs_to() {
        let mut a = aliases();
        assert!(a.record("GabrielVargas", "Varguitos#11833"));
        let found: Vec<(String, String)> = a.pairs().collect();
        assert_eq!(
            found,
            vec![("GabrielVargas".to_string(), "Varguitos#11833".to_string())]
        );
    }

    #[test]
    fn the_same_mapping_twice_is_not_news() {
        let mut a = aliases();
        assert!(a.record("GabrielVargas", "Varguitos#11833"));
        assert!(!a.record("GabrielVargas", "Varguitos#11833"));
    }

    /// One person, one real name. The spelling Blizzard used to show must not stay
    /// behind competing with the one it shows now.
    #[test]
    fn a_player_keeps_only_their_newest_drawn_name() {
        let mut a = aliases();
        a.record("GabrielVargas", "Varguitos#11833");
        a.record("GabrielV", "Varguitos#11833");
        let found: Vec<String> = a.pairs().map(|(drawn, _)| drawn).collect();
        assert_eq!(found, vec!["GabrielV".to_string()]);
    }

    /// Two friends are two mappings, and recording one must not disturb the other.
    #[test]
    fn one_friend_does_not_displace_another() {
        let mut a = aliases();
        a.record("GabrielVargas", "Varguitos#11833");
        a.record("AnaSilva", "anasilva#2201");
        assert_eq!(a.len(), 2);
    }
}
