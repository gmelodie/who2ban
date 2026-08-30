.DEFAULT_GOAL := up
.PHONY: up down logs app-logs serve wasm test check dist

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

# Local run without docker or tls, reachable at http://localhost:8731 only.
serve: wasm
	cargo run -p hots-web

wasm:
	cargo build -p hots-parse --release --target wasm32-unknown-unknown

test:
	cargo test --workspace

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets
	cargo clippy -p hots-parse --target wasm32-unknown-unknown

dist: wasm
	cargo build --release -p hots-web
	install -D target/release/hots-web dist/hots-web
	install -D target/wasm32-unknown-unknown/release/hots_parse.wasm dist/hots_parse.wasm
