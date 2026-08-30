# HotS Draft Helper

Shows the hero pool of the five enemies while the Storm League lobby forms, so you can ban on evidence. A web app: the page reads the game's own files, so there are no screenshots, no overlay and no memory reads.

The page does the reading and the parsing. Only the ten battletags reach the server.

## Running

```sh
make serve                      # builds the wasm parser, then serves on 127.0.0.1:8731
```

Open `http://localhost:8731` in Chrome or Edge, then point it at two folders:

| Button | Folder |
|---|---|
| temp folder | `%TEMP%\Heroes of the Storm` |
| replays | `Documents\Heroes of the Storm\Accounts\<id>\<id>\Replays\Multiplayer` |

The browser remembers both, and asks for one click to hand the permission back on a later visit. Everything else runs by itself: the page polls the temp folder every 400 ms, and it parses any replay the server has not stored yet.

```sh
make test                       # 21 tests
make check                      # fmt, clippy, clippy for wasm32
make dist                       # dist/hots-web and dist/hots_parse.wasm
```

The two files of `dist` go anywhere together, and `HOTS_ADDR` moves the port.

## Layout

| Path | What it holds |
|---|---|
| `crates/hots-parse` | Replay and battlelobby parsing. Builds for the host and for `wasm32`. |
| `crates/hots-core` | Database, Heroes Profile client, draft assembly, folder watchers for the CLI. |
| `crates/hots-cli` | `hots`, a headless driver that reads the folders from the machine it runs on. |
| `crates/hots-web` | The server: sqlite, Heroes Profile, an SSE stream, and the frontend baked in. |
| `ui` | Three static files and no bundler. |

[rs-heroprotocol](https://github.com/gmelodie/rs-heroprotocol) reads the MPQ archive and decodes the protocol streams. `hots-parse` keeps what this tool needs, which is the players, the heroes, the result, the map and the game mode, and adds the battlelobby scan the crate does not cover. It reaches the browser as a 590 KB `wasm32` module with a raw ABI, so there is no wasm-bindgen and no npm anywhere in the build.

## Data flow

1. The client writes `%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby` when the lobby forms. The page reads it inside 400 ms and parses it in wasm.
2. The page posts the ten battletags to `POST /api/draft`. The server answers from SQLite alone, and the enemy rows paint before the first ban.
3. Every enemy whose Heroes Profile rows are missing or older than the TTL gets a request of its own. Each answer arrives over SSE and replaces one card.
4. When the match ends the client writes a `.StormReplay`. The page parses it in wasm and posts the result of the match, never the file.

The page polls rather than watches because the client deletes its temp folder on exit, which kills a directory watch.

## Decisions

- **Which team is mine**: `battletag` from the config. With none set, the most frequent battletag of the stored replays. With neither, the screen shows all ten players and marks nobody an enemy.
- **Ranking**: games says what they pick, winrate says what they are good at, so the card carries both and the header sorts by either. A hero under `min_games_for_winrate` shows `-` instead of a meaningless rate.
- **Local scope**: every mode by default. Quick Match still reveals a hero pool. Set `local_all_modes = false` for the ranked queues alone, which counts Hero League and Team League too, since Storm League absorbed both.
- **Merge rule**: per hero, the side with more games wins the headline number. Both counts stay on the row, and the `src` column says which sides answered.
- **What leaves the machine**: battletags, hero names and counts. No file, and no path.

## Config

`~/.local/share/hots-draft/config.toml` on Linux, `%APPDATA%\hots-draft\config.toml` on Windows. `HOTS_DATA_DIR` overrides the folder, and the settings panel writes the same file.

```toml
battletag = "Name#1234"
hp_api_key = "..."
hp_game_type = "Storm League"
hp_ttl_days = 7
max_heroes = 8
local_all_modes = true
```

## The command line

`hots` reads the folders from the machine it runs on, which is the way to backfill years of replays without a browser tab.

```sh
cargo run -p hots-cli -- config
cargo run -p hots-cli -- backfill
cargo run -p hots-cli -- lobby sample-lobby.json
cargo run -p hots-cli -- watch
```

It shares the database and the config with the server. `HOTS_REPLAY_DIR` and `HOTS_TEMP_DIR` override the two folders.

## The battlelobby scan

`replay.server.battlelobby` is bit packed and undocumented. The same stream sits inside every `.StormReplay`, so one scanner serves both the live lobby and the finished match. `replay.details` carries the heroes, the teams and the result, but it holds short names rather than battletags, so the two sources join by slot order and the join is checked name by name.

The scan looks for a length that agrees with the `name#1234` string behind it. A test covers the one trap in real data: the character `!` is 0x21, which reads as a length of 16 and steals the byte in front of a sixteen-character tag. A count other than ten is noise, and the parser rejects it, because half of noise is a wrong enemy team.

Point `HOTS_TEST_REPLAY` at a `.StormReplay` and `cargo test` checks the parser against that file. No replay is committed here, because each one carries the battletags of nine other people.

## Known gaps

- The File System Access API is Chrome and Edge only. Firefox and Safari can show the page, but they cannot read the folders, so the draft screen stays empty there. The CLI covers those machines.
- The server has no authentication. It binds `127.0.0.1` for that reason. Put a reverse proxy and a password in front before `HOTS_ADDR` points anywhere else.
- Nothing checks the Heroes Profile response shape against the live API. The reader is tolerant on purpose. It walks the JSON, takes any object with win counts as a hero, and accepts numbers written as strings. Change `heroes_from_json` if the real response disagrees.
- The game mode comes from `m_ammId`. A real file confirms the Quick Match id. The published tables supply the other nine, and an unknown id stores as `Unknown`.
- The api key sits in plain text in `config.toml`.
