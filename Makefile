.PHONY: dev build server check clean webui help

# Development — start Tauri desktop app with hot reload
dev:
	cd webui && pnpm install
	cargo tauri dev

# Build desktop app (release)
build: webui-build
	cargo tauri build

# Build server binary only (release)
server:
	cargo build -p nyro-server --release

# Run server binary locally (debug)
server-dev:
	cargo run -p nyro-server -- --proxy-port 18080 --admin-port 18081

# Build webui
webui-build:
	cd webui && pnpm install && pnpm build

# Type check & lint everything
check:
	cargo check --workspace
	cd webui && pnpm build

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
	@echo "  make clean        Remove build artifacts"
