# HotS Draft Helper

Shows the hero pool of the five enemies while the Storm League lobby forms, so you can ban on evidence. It reads the game's own files: no screenshots, no overlay, no memory reads.

Every number comes from the replays on your own disk. There is no external service and no api key: what the database has parsed is what the screen shows, so a player nobody has met is simply blank.

The home screen watches for a lobby and answers with the enemy hero pools. Behind it sits one other page, `submit your replays`, which is what teaches the database who anyone is. The home screen names the folder it watches and says when it last read it, so a folder that stopped working says so instead of going quiet. Your battletag lives in your own browser, so one server and one shared database serve several people on several machines.

Two ways to run it. The server reads the folders when it sits on the gaming machine, and otherwise the page reads them and parses in wasm, so only battletags, hero names and counts cross the network.

## Run it

On the machine that runs the game, `make serve` and open `http://localhost:8731`. The server finds the game folders itself, watches them, and the page only draws. Any browser works.

On Linux it looks inside the wine prefix: anything under `~/Games`, any `drive_c` in your home, `~/.wine`, and the Bottles folders. The replay folder is the anchor, since it is always on disk, and the temp folder is taken from the same prefix rather than from the system `/tmp`. The client deletes that folder when it quits, so a `Temp` that exists is enough and the path resolves with the game closed. `WINEPREFIX` names a prefix directly, and `replay_dir` and `temp_dir` in `~/.local/share/hots-draft/config.toml` settle it for good. Every prefix contributes its replays, since one machine often holds several.

On Windows that is the only mode that gives live lobbies, because the client writes the battlelobby into `%TEMP%`, which sits under `AppData`, and the browser refuses every folder in there: it answers that the folder contains system files. The server has no such limit.

Anywhere else, the server has no folders to read, so the browser does the reading:

```sh
cp .env.example .env    # domain, email, a login
make                    # https on 443, certificate and all
```

Open the site in Chrome or Edge and point it at two folders:

| Button | Folder to pick |
|---|---|
| replays | `%USERPROFILE%\Documents\Heroes of the Storm` |
| temp folder | the `Temp` folder of the wine prefix, on Linux only |

Documents is a folder that exists with the game closed, and the page walks down from there, so nobody has to find `Replays\Multiplayer`. The temp button is disabled on Windows, where `AppData` is off limits to the browser: the page says so, and takes a battlelobby through a plain file input instead, which the block does not cover. The picker takes a well-known folder name and nothing else, so the page prints each path with a copy button: paste it into the File name box of the dialog and press enter. The browser remembers both folders and asks for one click on a later visit. After that the page polls the temp folder every 400 ms and parses any replay the server has not stored.

TLS is not decoration in that mode: the browser gives no folder access to a page served over plain http, and `showDirectoryPicker` exists in Chrome and Edge alone. Firefox and Safari need the first mode, where the server does the reading.

Type your battletag into the header box. It stays in that browser and travels with each lobby, which is how one server tells one player's team from another's. The `settings` panel holds how many heroes a card shows and how many games a hero needs before its winrate means anything.

## How it works

1. The client writes `replay.server.battlelobby` into `%TEMP%` when the lobby forms. Whichever side holds the folders reads it within 400 ms and parses it.
2. The ten battletags and your own reach the server. It answers from SQLite alone, so the enemy rows paint before the first ban.
3. When the match ends the new `.StormReplay` is parsed the same way, and only the result is stored. The next lobby with those players is that much sharper.

Games says what they pick, winrate says what they are good at, so each card carries both and the header sorts by either.

| Path | What it holds |
|---|---|
| `crates/hots-parse` | Replay and battlelobby parsing. Builds for the host and for `wasm32`. |
| `crates/hots-core` | Database, folder discovery, draft assembly. |
| `crates/hots-cli` | `hots`, for backfilling years of replays without a browser tab. |
| `crates/hots-web` | The server: sqlite, SSE, frontend baked in. |
| `ui` | Three static files, no bundler. |

[rs-heroprotocol](https://github.com/gmelodie/rs-heroprotocol) reads the MPQ archive and decodes the protocol streams. `hots-parse` adds the battlelobby scan it does not cover, and ships to the browser as a 590 KB module with a raw ABI, so there is no wasm-bindgen and no npm.

`make logs` follows the whole stack and `make app-logs` follows the server alone. `RUST_LOG=hots_web=debug,hots_core=debug` in `.env` says a great deal more. `make test` runs 21 tests, `make check` runs fmt and clippy for both targets, `make dist` builds the two files a bare host needs.

## The battlelobby scan

`replay.server.battlelobby` is bit packed and undocumented, and the same stream sits inside every `.StormReplay`, so one scanner serves the live lobby and the finished match. `replay.details` holds the heroes and the result under short names, so the two join by slot order, checked name by name.

The scan anchors on the `#`, walks out to the digits and the name, and keeps the pair only when the byte in front encodes that length, in any of the three encodings seen across builds. A test covers the one trap in real data: `!` is 0x21, which reads as a length of 16 and steals the byte in front of a sixteen-character tag. A count other than ten is noise and gets rejected, because half of noise is a wrong enemy team.

Point `HOTS_TEST_REPLAY` at a `.StormReplay` and `cargo test` checks the parser against it. No replay is committed here: each one carries the battletags of nine other people.

## Known gaps

- Firefox and Safari have no File System Access API, so they cannot do the browser-side reading. Run the server on the gaming machine instead and they work like any other browser.
- Windows blocks `AppData` in every browser folder picker, so a remote server cannot watch a Windows lobby. Replays still work, since Documents is allowed, and the server run locally has no such limit.
- Coverage is whatever the database has parsed. An enemy nobody has played with shows nothing, and there is no service to ask.
- A replay whose lobby stream will not scan is still stored, under the short name from `replay.details`, and a lobby battletag finds it by the part before the `#`. Two players sharing a name merge until one of their replays scans.
- The game mode comes from `m_ammId`. A real file confirms the Quick Match id; the published tables supply the other nine.
- A build past the newest protocol table decodes with the nearest older one rather than failing, since the two streams this reads are self-describing. A patch that moves a field would need `rs-heroprotocol` regenerated.
