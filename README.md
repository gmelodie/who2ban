# HotS Draft Helper

Shows the hero pool of the five enemies while the Storm League lobby forms, so you can ban on evidence. The page reads the game's own files: no screenshots, no overlay, no memory reads.

Only battletags, hero names and counts reach the server. The parsing happens in the browser, in wasm.

## Run it

```sh
cp .env.example .env    # domain, email, a login
make                    # https on 443, certificate and all
```

Open the site in Chrome or Edge and point it at two folders:

| Button | Folder |
|---|---|
| temp folder | `%TEMP%\Heroes of the Storm` |
| replays | `Documents\Heroes of the Storm\Accounts\<id>\<id>\Replays\Multiplayer` |

The browser remembers both and asks for one click on a later visit. After that the page polls the temp folder every 400 ms and parses any replay the server has not stored.

TLS is not decoration: a page served over plain http gets no folder access at all. `make serve` skips docker and runs on `http://localhost:8731`, which the browser also treats as secure.

Settings live in the panel behind the `settings` button: your battletag, the Heroes Profile api key, and the cache TTL.

## How it works

1. The client writes `replay.server.battlelobby` into `%TEMP%` when the lobby forms. The page reads it and parses it in wasm.
2. The page posts the ten battletags. The server answers from SQLite alone, so the enemy rows paint before the first ban.
3. Enemies whose Heroes Profile rows are missing or stale get one request each. Answers arrive over SSE and replace one card at a time.
4. When the match ends the page parses the new `.StormReplay` and posts the result, never the file.

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

- Chrome and Edge only. Firefox and Safari have no File System Access API, so the page loads but reads nothing. The CLI covers those machines.
- The Heroes Profile response shape is unverified against the live API. The reader is tolerant on purpose. Change `heroes_from_json` if it disagrees.
- The game mode comes from `m_ammId`. A real file confirms the Quick Match id; the published tables supply the other nine.
- The api key sits in plain text in the config, inside the `data` volume.
