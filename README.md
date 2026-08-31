# HotS Draft Helper

Shows the hero pool of the five enemies while the Storm League lobby forms, so you can ban on evidence. Every number comes from replays you already have. No external service, no api key, no overlay, no memory reads.

## Run it

Download `hots-app` from the latest build, run it on the machine with the game, and type your battletag into settings. It finds the replay folder and the temp folder, parses what it finds, and paints the enemy hero pools when a lobby forms.

It talks to `https://hots.gmelodie.com` by default, so replays pool with everyone else's. Clear the server field in settings to keep the database on your machine instead.

```sh
make app      # the desktop app from source
make serve    # the admin console on http://localhost:8731
make          # the console behind nginx and a certificate
make test     # 29 tests
```

## How it works

The client writes `replay.server.battlelobby` into `%TEMP%` when a lobby forms. The app reads it within 400 ms, parses the ten battletags, and answers from SQLite before the first ban. When the match ends it parses the new `.StormReplay` and stores the result, never the file.

The battlelobby is bit packed and undocumented, and the same stream sits inside every replay, so one scanner serves both. It anchors on the `#` and keeps a name whose length the byte in front agrees with. A replay that will not scan still counts, under the short name from `replay.details`.

A match is keyed by its start time and roster, so two people in one game who both send their replay leave one match behind.

The database is never removed on an update. A shape this build cannot read is copied to `hots.before-vN.db` first, and a database newer than the build stops it rather than being touched.

| Path | What it holds |
|---|---|
| `crates/hots-parse` | Replay and battlelobby parsing. |
| `crates/hots-core` | Database, folder discovery, draft assembly. |
| `crates/hots-app` | The desktop app. egui, one static binary. |
| `crates/hots-cli` | The same work from a terminal, plus `scan` for a stubborn replay. |
| `crates/hots-web` | Server: sqlite, http, admin console. |

Parsing comes from [rs-heroprotocol](https://github.com/gmelodie/rs-heroprotocol). A build past its newest protocol table decodes with the nearest older one. Point `HOTS_TEST_REPLAY` at a `.StormReplay` and `cargo test` checks the parser against it; none is committed here, since each carries nine other people's battletags.

The app has to run on the machine with the game, and it only knows the players it has replays for.
