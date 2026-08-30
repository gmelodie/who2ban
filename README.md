# HotS Draft Helper

Shows the hero pool of the five enemies while the Storm League lobby forms, so you can ban on evidence. It reads the files the game writes: no screenshots, no overlay, no memory reads.

## Running

```sh
cargo test --workspace          # 21 tests
cargo run -p hots-cli -- config # resolved paths and counts
cargo run -p hots-cli -- backfill
cargo run -p hots-cli -- lobby sample-lobby.json
cargo run -p hots-cli -- watch
cargo tauri dev                 # needs a webview toolchain
```

`hots lobby` reads a `.json` lobby as well as the binary file, so the whole pipeline runs with no game installed.

## Layout

| Path | What it holds |
|---|---|
| `crates/hots-core` | Parsing, database, replay ingest, watchers, Heroes Profile client, draft assembly. |
| `crates/hots-cli` | `hots`, a headless driver for the core. |
| `src-tauri` | Desktop shell: commands, events, watcher supervision. |
| `ui` | Static frontend, no bundler. |

`src-tauri` is its own cargo workspace, so `cargo test` at the root never needs the GTK and WebKit system libraries.

[rs-heroprotocol](https://github.com/gmelodie/rs-heroprotocol) reads the MPQ archive and decodes the protocol streams. `hots-core::parse` keeps what this tool needs, which is the players, the heroes, the result, the map and the game mode, and adds the battlelobby scan the crate does not cover.

## Data flow

1. The client writes `%TEMP%\Heroes of the Storm\TempWriteReplayP1\replay.server.battlelobby` when the lobby forms. A stat loop notices it inside 400 ms. It stats rather than watches because the client deletes that folder on exit, which kills a watch.
2. `draft::build` splits the ten battletags into the two teams and answers from SQLite alone. The frontend paints the enemy rows before the first ban.
3. Every enemy whose Heroes Profile rows are missing or older than the TTL gets a request of its own. Each answer replaces one card as it lands.
4. When the match ends the client writes a `.StormReplay`. `notify` sees it, the file is parsed once it stops growing, and the local aggregate grows.

## Decisions

- **Which team is mine**: `battletag` from the config. With none set, the most frequent battletag of the stored replays. With neither, the screen shows all ten players and marks nobody an enemy.
- **Ranking**: games says what they pick, winrate says what they are good at, so the card carries both and the header sorts by either. A hero under `min_games_for_winrate` shows `-` instead of a meaningless rate.
- **Local scope**: every mode by default. Quick Match still reveals a hero pool. Set `local_all_modes = false` for the ranked queues alone, which counts Hero League and Team League too, since Storm League absorbed both.
- **Merge rule**: per hero, the side with more games wins the headline number. Both counts stay on the row, and the `src` column says which sides answered.

## Config

`~/.local/share/hots-draft/config.toml` on Linux, `%APPDATA%\hots-draft\config.toml` on Windows. `HOTS_DATA_DIR` overrides the folder.

```toml
battletag = "Name#1234"
hp_api_key = "..."
hp_game_type = "Storm League"
hp_ttl_days = 7
max_heroes = 8
local_all_modes = true
# replay_dir = "..."   # skips the Documents search
# temp_dir = "..."     # skips %TEMP%
```

`HOTS_REPLAY_DIR` and `HOTS_TEMP_DIR` override the two folders for a test run.

## The battlelobby scan

`replay.server.battlelobby` is bit packed and undocumented. The same stream sits inside every `.StormReplay`, so one scanner serves both the live lobby and the finished match. `replay.details` carries the heroes, the teams and the result, but it holds short names rather than battletags, so the two sources join by slot order and the join is checked name by name.

The scan looks for a length that agrees with the `name#1234` string behind it. A test covers the one trap in real data: the character `!` is 0x21, which reads as a length of 16 and steals the byte in front of a sixteen-character tag. A count other than ten is noise, and the parser rejects it, because half of noise is a wrong enemy team.

Point `HOTS_TEST_REPLAY` at a `.StormReplay` and `cargo test` checks the parser against that file. No replay is committed here, because each one carries the battletags of nine other people.

## Known gaps

- Nothing checks the Heroes Profile response shape against the live API. The reader is tolerant on purpose. It walks the JSON, takes any object with win counts as a hero, and accepts numbers written as strings. Change `heroes_from_json` if the real response disagrees.
- `src-tauri` compiles nowhere yet. The machine it was written on has no WebKitGTK, so it needs a `cargo check` on a real desktop.
- The game mode comes from `m_ammId`. A real file confirms the Quick Match id. The published tables supply the other nine, and an unknown id stores as `Unknown`.
- The api key sits in plain text in `config.toml`.
- The icons under `src-tauri/icons` are flat blue placeholders. Run `cargo tauri icon path/to/art.png` to replace the set.
