#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("parser: {0}")]
    Parser(#[from] heroprotocol::Error),
    #[error("malformed file: {0}")]
    Malformed(String),
    #[error("watcher: {0}")]
    Watch(#[from] notify::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("config: {0}")]
    Config(String),
    #[error("heroes profile: {0}")]
    HeroesProfile(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
