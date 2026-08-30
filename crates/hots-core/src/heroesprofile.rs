use std::time::Duration;

use serde_json::Value;

use crate::config::Config;
use crate::db::HpHero;
use crate::error::{Error, Result};

pub struct HpClient {
    http: reqwest::Client,
    base: String,
    token: String,
    game_type: String,
}

#[derive(Debug, Clone, Default)]
pub struct PlayerStats {
    pub heroes: Vec<HpHero>,
    pub mmr: Option<f64>,
}

impl HpClient {
    pub fn new(cfg: &Config) -> Result<HpClient> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("hots-draft-helper/0.1")
            .build()?;
        Ok(HpClient {
            http,
            base: cfg.hp_base_url.trim_end_matches('/').to_string(),
            token: cfg.api_key()?.to_string(),
            game_type: cfg.hp_game_type.clone(),
        })
    }

    pub async fn player_stats(&self, battletag: &str, region: u8) -> Result<PlayerStats> {
        let heroes = self.hero_stats(battletag, region).await?;
        let mmr = self.mmr(battletag, region).await.unwrap_or(None);
        Ok(PlayerStats { heroes, mmr })
    }

    async fn hero_stats(&self, battletag: &str, region: u8) -> Result<Vec<HpHero>> {
        let url = format!("{}/Player/Hero/All", self.base);
        let body = self.get(&url, battletag, region).await?;
        Ok(heroes_from_json(&body))
    }

    async fn mmr(&self, battletag: &str, region: u8) -> Result<Option<f64>> {
        let url = format!("{}/Player/MMR", self.base);
        let body = self.get(&url, battletag, region).await?;
        Ok(mmr_from_json(&body))
    }

    async fn get(&self, url: &str, battletag: &str, region: u8) -> Result<Value> {
        let resp = self
            .http
            .get(url)
            .query(&[
                ("api_token", self.token.as_str()),
                ("battletag", battletag),
                ("region", &region.to_string()),
                ("game_type", self.game_type.as_str()),
            ])
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(Error::HeroesProfile(format!(
                "{status} from {url}: {}",
                text.chars().take(200).collect::<String>()
            )));
        }
        serde_json::from_str(&text)
            .map_err(|e| Error::HeroesProfile(format!("bad json from {url}: {e}")))
    }
}

pub fn heroes_from_json(body: &Value) -> Vec<HpHero> {
    let mut out = Vec::new();
    collect_heroes(body, None, &mut out);
    out.sort_by(|a, b| b.games.cmp(&a.games).then_with(|| a.hero.cmp(&b.hero)));
    out
}

pub fn mmr_from_json(body: &Value) -> Option<f64> {
    find_number(body, "mmr")
}

fn as_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().replace('%', "").parse().ok(),
        _ => None,
    }
}

fn field(map: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    map.get(key).and_then(as_num)
}

fn hero_from(name: &str, map: &serde_json::Map<String, Value>) -> Option<HpHero> {
    let wins = field(map, "wins");
    let losses = field(map, "losses");
    let played = field(map, "games_played").or_else(|| field(map, "games"));
    let rate = field(map, "win_rate").or_else(|| field(map, "winrate"));

    let games = played.or_else(|| Some(wins? + losses?))?;
    if games <= 0.0 {
        return None;
    }
    let wins = wins.or_else(|| Some(games * rate? / 100.0))?;

    Some(HpHero {
        hero: name.to_string(),
        games: games.round() as u32,
        wins: wins.round().clamp(0.0, games.round()) as u32,
        mmr: field(map, "mmr"),
    })
}

/// The hero name sits at an unspecified depth, so use the key of the object that carries the counts.
fn collect_heroes(v: &Value, key: Option<&str>, out: &mut Vec<HpHero>) {
    match v {
        Value::Object(map) => {
            if let Some(name) = key
                && let Some(hero) = hero_from(name, map)
            {
                out.push(hero);
                return;
            }
            for (k, child) in map {
                collect_heroes(child, Some(k), out);
            }
        }
        Value::Array(items) => {
            for item in items {
                if let Some(map) = item.as_object() {
                    let name = map
                        .get("hero")
                        .or_else(|| map.get("hero_name"))
                        .and_then(|n| n.as_str());
                    if let Some(hero) = name.and_then(|n| hero_from(n, map)) {
                        out.push(hero);
                        continue;
                    }
                }
                collect_heroes(item, key, out);
            }
        }
        _ => {}
    }
}

fn find_number(v: &Value, key: &str) -> Option<f64> {
    match v {
        Value::Object(map) => {
            if let Some(n) = map.get(key).and_then(as_num) {
                return Some(n);
            }
            map.values().find_map(|c| find_number(c, key))
        }
        Value::Array(items) => items.iter().find_map(|c| find_number(c, key)),
        _ => None,
    }
}
