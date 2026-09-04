.DEFAULT_GOAL := up
.PHONY: up down logs app-logs serve app test check dist atlas

# https on 443, behind nginx and a Let's Encrypt certificate.
up: .env
	docker compose up -d --build
	@echo "https://$$(grep -E '^DOMAIN=' .env | cut -d= -f2)"

down:
	docker compose down

logs:
	docker compose logs -f --tail 100

app-logs:
	docker compose logs -f --tail 100 app

.env:
	@cp .env.example .env
	@echo "wrote .env from .env.example. Fill it in, then run make again."
	@false

# The admin console, without docker or tls, at http://localhost:8731.
serve:
	cargo run -p w2b-web

# The desktop app, which is what watches for a lobby.
app:
	cargo run -p w2b-app

test:
	cargo test --workspace

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets

dist:
	cargo build --release -p w2b-web -p w2b-app
	install -D target/release/w2b-web dist/w2b-web
	install -D target/release/w2b dist/w2b

# Fold learned shapes into the ones the app ships with, so a fresh install starts where
# this machine got to. A copy rather than a merge: the client absorbs the seed on every
# launch, so whatever it has on disk is always a superset of what it was shipped.
#
# Takes the shared pool when SERVER is given, which is everything every client has
# learned between them, and this machine's own file otherwise.
#   make atlas
#   make atlas SERVER=https://user:pass@draft.example.com
ATLAS := $(or $(XDG_DATA_HOME),$(HOME)/.local/share)/who2ban/glyphs.json
SEED  := crates/w2b-app/assets/glyph-seed.json

atlas:
ifdef SERVER
	curl -fsS "$(SERVER)/api/glyphs" -o "$(SEED)"
else
	@test -f "$(ATLAS)" || { \
	    echo "$(ATLAS) does not exist: nothing has been learned on this machine yet."; \
	    echo "Play a draft with the app open, or pass SERVER=... to take the shared pool."; \
	    exit 1; \
	}
	cp "$(ATLAS)" "$(SEED)"
endif
	@python3 -c "import json;d=json.load(open('$(SEED)'));print('seed now holds',len(d['shapes']),'letters and',sum(len(v) for v in d['shapes'].values()),'examples')"
	@echo "commit $(SEED) to share it."
