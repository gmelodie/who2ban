use hots_core::{Config, Db, Draft, Lobby, MatchRecord, draft, paths};

use crate::settings::Settings;

/// A file beside the app when it stands alone, one shared database when it is pointed at a server.
pub enum Store {
    Local(Box<Db>),
    Server(Server),
}

pub struct Server {
    url: String,
    auth: Option<String>,
}

pub struct Stored {
    pub stored: bool,
    pub matches: u32,
}

impl Store {
    pub fn open(settings: &Settings) -> hots_core::Result<Store> {
        match settings.shared_server() {
            Some(url) => Ok(Store::Server(Server::new(url, settings))),
            None => Ok(Store::Local(Box::new(Db::open(&paths::db_path())?))),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Store::Local(_) => paths::db_path().display().to_string(),
            Store::Server(server) => server.url.clone(),
        }
    }

    pub fn known(&self) -> Result<Vec<String>, String> {
        match self {
            Store::Local(db) => db
                .known_replays()
                .map(|set| set.into_iter().collect())
                .map_err(|e| e.to_string()),
            Store::Server(server) => server.get("/api/matches/known"),
        }
    }

    pub fn submit(&self, key: &str, record: &MatchRecord) -> Result<Stored, String> {
        match self {
            Store::Local(db) => {
                let stored = db.record_replay(key, record).map_err(|e| e.to_string())?;
                Ok(Stored {
                    stored: stored.is_some(),
                    matches: db.match_count().map_err(|e| e.to_string())?,
                })
            }
            Store::Server(server) => {
                let body = serde_json::json!({ "key": key, "record": record });
                let reply: ServerStored = server.post("/api/matches", &body)?;
                Ok(Stored {
                    stored: reply.stored,
                    matches: reply.matches,
                })
            }
        }
    }

    pub fn draft(&self, cfg: &Config, lobby: &Lobby, me: Option<&str>) -> Result<Draft, String> {
        match self {
            Store::Local(db) => draft::build(db, cfg, lobby, me).map_err(|e| e.to_string()),
            Store::Server(server) => {
                let body = serde_json::json!({ "lobby": lobby, "battletag": me });
                server.post("/api/draft", &body)
            }
        }
    }

    pub fn set_note(&self, battletag: &str, note: &hots_core::PlayerNote) -> Result<(), String> {
        match self {
            Store::Local(db) => db.set_note(battletag, note).map_err(|e| e.to_string()),
            Store::Server(server) => {
                let body = serde_json::json!({
                    "battletag": battletag,
                    "note": note.note,
                    "verdict": note.verdict,
                });
                server.put("/api/note", &body)
            }
        }
    }

    pub fn count(&self) -> Result<u32, String> {
        match self {
            Store::Local(db) => db.match_count().map_err(|e| e.to_string()),
            Store::Server(server) => {
                let status: ServerStatus = server.get("/api/status")?;
                Ok(status.matches)
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct ServerStored {
    stored: bool,
    matches: u32,
}

#[derive(serde::Deserialize)]
struct ServerStatus {
    matches: u32,
}

impl Server {
    fn new(url: &str, settings: &Settings) -> Server {
        let login = format!("{}:{}", settings.username, settings.password);
        Server {
            url: url.trim_end_matches('/').to_string(),
            auth: (!settings.username.is_empty())
                .then(|| format!("Basic {}", base64(login.as_bytes()))),
        }
    }

    fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let url = format!("{}{path}", self.url);
        let mut request = ureq::get(&url);
        if let Some(auth) = &self.auth {
            request = request.header("authorization", auth);
        }
        read(&url, request.call())
    }

    fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, String> {
        let url = format!("{}{path}", self.url);
        let mut request = ureq::post(&url);
        if let Some(auth) = &self.auth {
            request = request.header("authorization", auth);
        }
        read(&url, request.send_json(body))
    }

    /// Nothing here reads the reply, so nothing here fails on a reply it did not expect.
    fn put(&self, path: &str, body: &serde_json::Value) -> Result<(), String> {
        let url = format!("{}{path}", self.url);
        let mut request = ureq::put(&url);
        if let Some(auth) = &self.auth {
            request = request.header("authorization", auth);
        }
        request.send_json(body).map(drop).map_err(|e| failed(&url, e))
    }
}

fn read<T: serde::de::DeserializeOwned>(
    url: &str,
    reply: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<T, String> {
    reply
        .map_err(|e| failed(url, e))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("{url}: {e}"))
}

/// What a 401 says, and what sends the app back to its login screen.
pub const REJECTED: &str = "the server rejected this login";

fn failed(url: &str, e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(401) => format!("{url}: {REJECTED}"),
        e => format!("{url}: {e}"),
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let word = chunk
            .iter()
            .chain([0, 0].iter())
            .take(3)
            .fold(0u32, |word, byte| word << 8 | u32::from(*byte));
        for i in 0..4 {
            match i <= chunk.len() {
                true => out.push(ALPHABET[(word >> (18 - 6 * i)) as usize & 63] as char),
                false => out.push('='),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_becomes_a_basic_auth_header() {
        let settings = Settings {
            username: "me".to_string(),
            password: "secret".to_string(),
            ..Settings::default()
        };
        let server = Server::new("https://hots.example.com/", &settings);
        assert_eq!(server.url, "https://hots.example.com");
        assert_eq!(server.auth.as_deref(), Some("Basic bWU6c2VjcmV0"));

        let open = Server::new("https://hots.example.com", &Settings::default());
        assert_eq!(open.auth, None);
    }

    #[test]
    fn base64_pads_every_tail() {
        assert_eq!(base64(b"a:b"), "YTpi");
        assert_eq!(base64(b"a:bc"), "YTpiYw==");
        assert_eq!(base64(b"a:bcd"), "YTpiY2Q=");
    }
}
