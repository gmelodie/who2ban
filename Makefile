.DEFAULT_GOAL := up
.PHONY: up down logs app-logs serve app test check dist

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
