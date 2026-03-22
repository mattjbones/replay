.PHONY: dev build check clean release test test-rust test-ui lint

# Development with hot reload (Tauri watches both Rust + frontend)
dev:
	cargo tauri dev

# Type-check without building
check:
	cargo check

# Debug build
build:
	cargo tauri build --debug

# Release build (.app + .dmg)
release:
	cargo tauri build

# Clean build artifacts
clean:
	cargo clean

# ---------- Testing ----------

# Run all tests (Rust + UI)
test: test-rust test-ui

# Run Rust integration and unit tests
test-rust:
	cargo test

# Run Playwright UI tests (installs deps if needed)
test-ui:
	cd tests/ui && npm install && npx playwright install chromium && npx playwright test

# ---------- Linting ----------

# Run cargo check + clippy
lint:
	cargo check --all-targets
	cargo clippy --all-targets -- -D warnings
