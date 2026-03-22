.PHONY: dev build check clean release

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
