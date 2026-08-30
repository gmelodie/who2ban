# HotS Draft Helper

Shows the hero pool of the five enemies while the Storm League lobby forms, so you can ban on evidence. It reads the game's own files: no screenshots, no overlay, no memory reads.

Two ways to run it. The server reads the folders when it sits on the gaming machine, and otherwise the page reads them and parses in wasm, so only battletags, hero names and counts cross the network.

## Run it

On the machine that runs the game, `make serve` and open `http://localhost:8731`. The server finds the game folders itself, watches them, and the page only draws. Any browser works.

On Linux it looks inside the wine prefix, Lutris first (`~/Games/heroes-of-the-storm/drive_c/users/<you>/`), then `~/.wine` and the Bottles folders. `WINEPREFIX` names one directly, and `replay_dir` and `temp_dir` in the config settle it for good.

Anywhere else, the server has no folders to read, so the browser does the reading:

```sh
cp .env.example .env    # domain, email, a login
make                    # https on 443, certificate and all
```

Open the site in Chrome or Edge and point it at two folders:

| Button | Folder |
|---|---|
| temp folder | `%TEMP%\Heroes of the Storm` |
| replays | `Documents\Heroes of the Storm\Accounts\<id>\<id>\Replays\Multiplayer` |

The dialog cannot be aimed at a wine prefix, since the picker takes a well-known folder name and nothing else, so the page prints the path to paste. The browser remembers both folders and asks for one click on a later visit. After that the page polls the temp folder every 400 ms and parses any replay the server has not stored.

TLS is not decoration in that mode: the browser gives no folder access to a page served over plain http, and `showDirectoryPicker` exists in Chrome and Edge alone. Firefox and Safari need the first mode, where the server does the reading.

Settings live in the panel behind the `settings` button: your battletag, the Heroes Profile api key, and the cache TTL.

## How it works

1. The client writes `replay.server.battlelobby` into `%TEMP%` when the lobby forms. Whichever side holds the folders reads it within 400 ms and parses it.
2. The ten battletags reach the server. It answers from SQLite alone, so the enemy rows paint before the first ban.
3. Enemies whose Heroes Profile rows are missing or stale get one request each. Answers arrive over SSE and replace one card at a time.
4. When the match ends the new `.StormReplay` is parsed the same way, and only the result of the match is stored.

Games says what they pick, winrate says what they are good at, so each card carries both and the header sorts by either.

| Path | What it holds |
|---|---|
| `crates/hots-parse` | Replay and battlelobby parsing. Builds for the host and for `wasm32`. |
| `crates/hots-core` | Database, Heroes Profile client, draft assembly. |
| `crates/hots-cli` | `hots`, for backfilling years of replays without a browser tab. |
| `crates/hots-web` | The server: sqlite, Heroes Profile, SSE, frontend baked in. |
| `ui` | Three static files, no bundler. |

[rs-heroprotocol](https://github.com/gmelodie/rs-heroprotocol) reads the MPQ archive and decodes the protocol streams. `hots-parse` adds the battlelobby scan it does not cover, and ships to the browser as a 590 KB module with a raw ABI, so there is no wasm-bindgen and no npm.

`make test` runs 21 tests, `make check` runs fmt and clippy for both targets, `make dist` builds the two files a bare host needs.

## The battlelobby scan

`replay.server.battlelobby` is bit packed and undocumented, and the same stream sits inside every `.StormReplay`, so one scanner serves the live lobby and the finished match. `replay.details` holds the heroes and the result under short names, so the two join by slot order, checked name by name.

The scan looks for a length that agrees with the `name#1234` behind it. A test covers the one trap in real data: `!` is 0x21, which reads as a length of 16 and steals the byte in front of a sixteen-character tag. A count other than ten is noise and gets rejected, because half of noise is a wrong enemy team.

Point `HOTS_TEST_REPLAY` at a `.StormReplay` and `cargo test` checks the parser against it. No replay is committed here: each one carries the battletags of nine other people.

## Known gaps

- Firefox and Safari have no File System Access API, so they cannot do the browser-side reading. Run the server on the gaming machine instead and they work like any other browser.
- The Heroes Profile response shape is unverified against the live API. The reader is tolerant on purpose. Change `heroes_from_json` if it disagrees.
- The game mode comes from `m_ammId`. A real file confirms the Quick Match id; the published tables supply the other nine.
- The api key sits in plain text in the config, inside the `data` volume.
