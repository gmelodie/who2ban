pub mod model;
pub mod parse;

#[cfg(target_arch = "wasm32")]
mod wasm;

pub use model::{GameMode, Lobby, LobbyPlayer, MatchPlayer, MatchRecord, Toon};
pub use parse::{battlelobby, lobby_stream, replay, replay_bytes};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("malformed file: {0}")]
    Malformed(String),
}

impl From<heroprotocol::Error> for Error {
    fn from(e: heroprotocol::Error) -> Error {
        Error::Protocol(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
