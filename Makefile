.PHONY: wasm serve test check dist

wasm:
	cargo build -p hots-parse --release --target wasm32-unknown-unknown

serve: wasm
	cargo run -p hots-web

test:
	cargo test --workspace

check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets
	cargo clippy -p hots-parse --target wasm32-unknown-unknown

# The server reads hots_parse.wasm from its own folder.
dist: wasm
	cargo build --release -p hots-web
	install -D target/release/hots-web dist/hots-web
	install -D target/wasm32-unknown-unknown/release/hots_parse.wasm dist/hots_parse.wasm
