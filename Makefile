UNAME            ?= $(shell uname)
REMOVE           ?= rm -rf
NYRO_DIR       ?= /usr/local/nyro

# Rockspec file
ROCKSPEC         := rockspec/nyro-master-0.rockspec

# Engine (Rust workspace)
ENGINE_DIR       := engine
ifeq ($(UNAME), Darwin)
    ENGINE_LIB_NAME := libnyro.dylib
else
    ENGINE_LIB_NAME := libnyro.so
endif
ENGINE_LIB       := $(ENGINE_DIR)/target/release/$(ENGINE_LIB_NAME)
WEBUI_DIR        := webui

# Detect OpenResty LuaJIT path
ifeq ($(UNAME), Darwin)
	# macOS with Homebrew OpenResty
	LUAJIT_DIR       ?= $(shell test -d /opt/homebrew/opt/openresty/luajit && echo /opt/homebrew/opt/openresty/luajit || echo /usr/local/opt/openresty/luajit)
	LUAROCKS_FLAGS   := --lua-dir=$(LUAJIT_DIR)
else
	# Linux
	LUAJIT_DIR       ?= /usr/local/openresty/luajit
	ifneq ("$(wildcard $(LUAJIT_DIR))","")
		LUAROCKS_FLAGS := --lua-dir=$(LUAJIT_DIR)
	else
		LUAROCKS_FLAGS :=
	endif
endif

.PHONY: engine
engine:
	@echo "Building Nyro engine (Rust)..."
	cd $(ENGINE_DIR) && cargo build --release
	@echo "Engine built: $(ENGINE_LIB)"

.PHONY: install
install: engine
	@echo "Installing engine library to system..."
	@install -m 755 $(ENGINE_LIB) /usr/local/lib/$(ENGINE_LIB_NAME)
	@echo "Installing NYRO using luarocks..."
	@echo "Using Lua/LuaJIT at: $(LUAJIT_DIR)"
	luarocks make $(ROCKSPEC) --tree=$(NYRO_DIR) $(LUAROCKS_FLAGS)
	@echo "Installing binary and configuration files..."
	@mkdir -p $(NYRO_DIR)/bin
	@mkdir -p $(NYRO_DIR)/lib
	@install -m 755 bin/nyro $(NYRO_DIR)/bin/nyro
	@install -m 644 conf/nyro.yaml $(NYRO_DIR)/conf/nyro.yaml
	@install -m 644 conf/config.yaml $(NYRO_DIR)/conf/config.yaml
	@cp $(ENGINE_LIB) $(NYRO_DIR)/lib/
	@echo ""
	@echo "Installation complete!"
	@echo ""
	@echo "NYRO modules installed to: $(NYRO_DIR)/share/lua/5.1/"
	@echo "NYRO dependencies installed to: $(NYRO_DIR)/lib/lua/5.1/"
	@echo "Engine library installed to: $(NYRO_DIR)/lib/$(ENGINE_LIB_NAME)"
	@echo "Binary installed to: $(NYRO_DIR)/bin/nyro"
	@echo "Configuration files installed to: $(NYRO_DIR)/conf/"
	@echo ""
	@echo "To use NYRO, add to your PATH:"
	@echo "  export PATH=$(NYRO_DIR)/bin:\$$PATH"
	@echo ""
	@echo "Then you can run:"
	@echo "  nyro version"
	@echo "  nyro start"

.PHONY: webui-build
webui-build:
	@echo "Building WebUI..."
	cd $(WEBUI_DIR) && npm install && npm run build
	@echo "WebUI built: $(WEBUI_DIR)/dist"

.PHONY: dev
dev: engine webui-build
	@echo "Installing engine library for development..."
	@mkdir -p ./lua_modules/lib
	@cp $(ENGINE_LIB) ./lua_modules/lib/
	@if [ ! -f conf/config.yaml ]; then \
		echo "conf/config.yaml not found, copying from example..."; \
		cp conf/config.yaml.example conf/config.yaml; \
	fi
	@echo "Installing NYRO for development..."
	@echo "Using Lua/LuaJIT at: $(LUAJIT_DIR)"
	luarocks make $(ROCKSPEC) --tree=./lua_modules $(LUAROCKS_FLAGS)
	@echo ""
	@echo "Development installation complete!"
	@echo ""
	@echo "You can now run NYRO directly:"
	@echo "  ./bin/nyro version"
	@echo "  ./bin/nyro start"
	@echo ""
	@echo "Note: The CLI automatically detects ./lua_modules/ and adds it to the search path."
	@echo "      Engine library is at: ./lua_modules/lib/$(ENGINE_LIB_NAME)"

.PHONY: uninstall
uninstall:
	@echo "Uninstalling NYRO..."
	luarocks remove nyro --tree=$(NYRO_DIR) 2>/dev/null || true
	@echo "Removing binary and configuration files..."
	@$(REMOVE) $(NYRO_DIR)/bin/nyro
	@$(REMOVE) $(NYRO_DIR)/conf
	@$(REMOVE) $(NYRO_DIR)/lib/$(ENGINE_LIB_NAME)
	@echo "NYRO has been uninstalled."

.PHONY: clean
clean:
	@echo "Cleaning engine build files..."
	cd $(ENGINE_DIR) && cargo clean
	@echo "Cleaning local development installation..."
	$(REMOVE) ./lua_modules
	@echo "Clean complete!"

.PHONY: help
help:
	@echo "NYRO Makefile targets:"
	@echo ""
	@echo "  make engine      - Build Rust engine library only"
	@echo "  make install     - Build engine and install NYRO to $(NYRO_DIR)"
	@echo "  make webui-build - Install deps and build WebUI"
	@echo "  make dev         - Build engine, WebUI and install for local development"
	@echo "  make uninstall   - Uninstall NYRO from $(NYRO_DIR)"
	@echo "  make clean       - Clean all build files and local installation"
	@echo "  make help        - Show this help message"
	@echo ""
	@echo "Variables:"
	@echo "  NYRO_DIR       - Installation directory (default: /usr/local/nyro)"
	@echo ""
	@echo "Examples:"
	@echo "  make install"
	@echo "  make dev"
	@echo "  make install NYRO_DIR=/opt/nyro"
