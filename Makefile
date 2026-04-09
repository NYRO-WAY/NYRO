.PHONY: dev build server check clean webui smoke smoke-storage release-check help

# Development — start Tauri desktop app with hot reload
dev: webui-build
	cargo tauri build --config src-tauri/tauri.dev.conf.json

# Build desktop app (release)
build: webui-build
	cargo tauri build

# Build server binary only (release)
server:
	cargo build -p nyro-server --release

# Run server binary locally (debug)
server-dev:
	cargo run -p nyro-server -- --proxy-port 19530 --admin-port 19531

# Build webui
webui-build:
	cd webui && pnpm install && pnpm build

# Type check & lint everything
check:
	cargo check --workspace
	cd webui && pnpm build

# End-to-end smoke (local mock upstream + nyro-server)
smoke:
	python3 scripts/smoke/server_smoke.py

# Storage backend smoke (default: sqlite + postgres)
smoke-storage:
	python3 scripts/smoke/storage_backends_smoke.py

# Pre-release verification gate
release-check: check smoke

# Clean all build artifacts
clean:
	cargo clean
	rm -rf webui/dist webui/node_modules/.vite

help:
	@echo "Nyro AI Gateway"
	@echo ""
	@echo "  make dev          Start Tauri desktop app (dev mode)"
	@echo "  make build        Build desktop app (release)"
	@echo "  make server       Build server binary (release)"
	@echo "  make server-dev   Run server binary (debug)"
	@echo "  make webui-build  Build frontend only"
	@echo "  make check        Type check Rust + TypeScript"
	@echo "  make smoke        Run local server smoke tests"
	@echo "  make smoke-storage Run storage smoke tests (default sqlite + postgres)"
	@echo "  make release-check Run check + smoke before release"
	@echo "  make clean        Remove build artifacts"
