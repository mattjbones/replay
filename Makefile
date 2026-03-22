.PHONY: dev build check clean release test test-rust test-ui lint

# Development with hot reload (Tauri watches both Rust + frontend)
dev:
	cargo tauri dev --config crates/recap-app/tauri.conf.json

# Type-check without building
check:
	cargo check --workspace

# Debug build
build:
	cargo tauri build --debug --config crates/recap-app/tauri.conf.json

# Release build (.app + .dmg)
release:
	cargo tauri build --config crates/recap-app/tauri.conf.json

# Clean build artifacts
clean:
	cargo clean

# ---------- Testing ----------

# Run all tests (Rust + UI)
test: test-rust test-ui

# Run Rust integration and unit tests
test-rust:
	cargo test --workspace

# Run Playwright UI tests (installs deps if needed)
test-ui:
	cd tests/ui && npm install && npx playwright install chromium && npx playwright test

# ---------- Linting ----------

# Run cargo check + clippy
lint:
	cargo check --workspace --all-targets
	cargo clippy --workspace --all-targets -- -D warnings
