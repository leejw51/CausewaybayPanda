# Causewaybay Panda — one binary per restaurant.
#
#   make                 this list
#   make start           background cafe on :$(PORT)
#   make stop            stop it
#   make status          pid, health, URLs
#   make test-all        Rust + Playwright
#
# Every target a shop owner or a developer needs lives here.

.DEFAULT_GOAL := help

PORT        ?= 8787
PW_PORT     ?= 8799
PIDFILE     := /tmp/causewaybay-panda.pid
LOGFILE     := /tmp/causewaybay-panda.log
BROWSER     := tests/browser
PANDA_HOME  ?= $(HOME)/.causewaybaypanda
PANDA_ROOT  ?= $(CURDIR)
PKG         := causewaybay-panda-server

# Cargo may stage the binary under CARGO_TARGET_DIR (CI, sandboxes).
TARGET_DIR  := $(shell cargo metadata --format-version 1 --no-deps --offline 2>/dev/null | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])" 2>/dev/null)
ifeq ($(TARGET_DIR),)
TARGET_DIR  := $(CURDIR)/target
endif
DEBUG_BIN   := $(TARGET_DIR)/debug/panda
RELEASE_BIN := $(TARGET_DIR)/release/panda

export PANDA_PORT ?= $(PORT)
export PANDA_HOME
export PANDA_ROOT

.PHONY: help version build release start stop restart run status logs health \
	urls open test test-browser test-browser-headed test-all install-browser \
	assets assets-force fmt check wait clean distclean

help: ## Show every target
	@echo
	@echo "  CAUSEWAYBAY PANDA  ·  Causewaybay Coffee"
	@echo "  port $(PORT)  home $(PANDA_HOME)"
	@echo
	@grep -hE '^[a-zA-Z0-9_-]+:.*?## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-22s\033[0m %s\n", $$1, $$2}'
	@echo
	@echo "  Guest and owner share one page. Chat or a large button."
	@echo "  Override port with  make start PORT=9000"
	@echo
	@echo "  MODE        simulation by default: Causewaybay Coin, a faucet, no chain."
	@echo "    PANDA_MODE=live              real USDC; needs a treasury and a token"
	@echo "    PANDA_CHAIN=cronos_mainnet   cronos_testnet (default), or 25 / 338"
	@echo "    PANDA_TREASURY=0x...         the shop's wallet — required for live"
	@echo "    PANDA_USDC_ADDRESS=0x...     required on testnet, optional on mainnet"
	@echo "    PANDA_USDC_DECIMALS=6        only if the token is not 6-decimal"
	@echo "    PANDA_RPC_URL=https://...    your own node; receipts are verified here"
	@echo "    PANDA_RECEIPT_WAIT_SECS=90   how long to wait for a payment to land"
	@echo
	@echo "  BOARD       every amount settles in USDC and reads in your currency."
	@echo "    PANDA_DENOM=HKD              KRW JPY CNY TWD SGD EUR GBP USD USDC"
	@echo "    PANDA_DENOM_RATE=7.8         units per USDC, if the built-in is stale"
	@echo "    PANDA_DENOM_SYMBOL=HK\$$       PANDA_DENOM_DECIMALS=2"
	@echo
	@echo "  CHAT        the local parser always runs; a model only sees what it"
	@echo "              could not read. First key found wins."
	@echo "    PANDA_AI_PROVIDER=grok       openai anthropic ollama openrouter off"
	@echo "    PANDA_AI_MODEL=...           PANDA_AI_BASE_URL=... (proxy or Ollama)"
	@echo "    keys: XAI_API_KEY GROK_API_KEY OPENAI_API_KEY ANTHROPIC_API_KEY"
	@echo "          OPENROUTER_API_KEY OLLAMA_HOST"
	@echo
	@echo "    make start PANDA_MODE=live PANDA_CHAIN=cronos_mainnet PANDA_TREASURY=0x..."
	@echo "    make start PANDA_DENOM=KRW"
	@echo "  make chain  prints what the running shop settles in."
	@echo
	@echo "  NO SERVER   make web compiles the cafe to WebAssembly; static/ then runs"
	@echo "              the whole shop inside the browser tab (GitHub Pages, a file)."
	@echo "  A MAC       make mac builds a double-clickable app; make mac-install"
	@echo "              starts it at login. Keys and settings go in"
	@echo "              ~/.causewaybaypanda/env, one KEY=value per line."
	@echo

version: ## Print the crate version
	@sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1

build: ## Compile the debug server
	cargo build -p $(PKG)
	@test -x "$(DEBUG_BIN)" || { echo "missing $(DEBUG_BIN)" >&2; exit 1; }

release: ## Compile the release server
	cargo build -p $(PKG) --release

web: ## Compile the cafe engine to WebAssembly (static/pkg) — the page then needs no server
	tools/build_web.sh

web-serve: web ## Serve static/ alone, no panda behind it, to try the tab-only cafe
	@echo "open http://127.0.0.1:8790/?local   (the ?local is only needed while a panda is also running)"
	cd static && python3 -m http.server 8790

APP := $(CURDIR)/dist/Causewaybay Panda.app
AGENT := $(HOME)/Library/LaunchAgents/com.causewaybay.panda.plist

mac: release ## Build "Causewaybay Panda.app" — double-click to open the shop
	chmod +x tools/mac_app.sh
	PANDA_RELEASE_BIN="$(RELEASE_BIN)" tools/mac_app.sh

mac-install: mac ## Start the shop whenever this Mac logs in
	@mkdir -p "$(HOME)/Library/LaunchAgents" "$(HOME)/.causewaybaypanda"
	@sed -e 's|__APP__|$(APP)|g' -e 's|__HOME__|$(HOME)|g' tools/com.causewaybay.panda.plist > "$(AGENT)"
	@launchctl unload "$(AGENT)" 2>/dev/null || true
	@launchctl load "$(AGENT)"
	@echo "installed $(AGENT)"
	@echo "the shop now starts with this Mac; make mac-uninstall to stop that"

mac-uninstall: ## Stop starting the shop at login
	@launchctl unload "$(AGENT)" 2>/dev/null || true
	@rm -f "$(AGENT)"
	@echo "removed $(AGENT)"

wait: ## Block until /health answers
	@i=0; until curl -sf http://127.0.0.1:$(PORT)/health >/dev/null; do \
		i=$$((i+1)); \
		if [ $$i -gt 80 ]; then echo "server did not come up on :$(PORT)" >&2; exit 1; fi; \
		sleep 0.1; \
	done

start: build ## Run the cafe in the background
	@mkdir -p "$(PANDA_HOME)"
	@if [ -f $(PIDFILE) ] && kill -0 $$(cat $(PIDFILE)) 2>/dev/null; then \
		echo "already running pid $$(cat $(PIDFILE))"; \
		$(MAKE) --no-print-directory urls; \
	else \
		PANDA_PORT=$(PORT) PANDA_HOME="$(PANDA_HOME)" PANDA_ROOT="$(PANDA_ROOT)" \
		  "$(DEBUG_BIN)" > $(LOGFILE) 2>&1 & echo $$! > $(PIDFILE); \
		$(MAKE) --no-print-directory wait PORT=$(PORT); \
		echo "started pid $$(cat $(PIDFILE))"; \
		$(MAKE) --no-print-directory urls; \
	fi

stop: ## Stop the background cafe
	@if [ -f $(PIDFILE) ]; then \
		kill $$(cat $(PIDFILE)) 2>/dev/null || true; \
		rm -f $(PIDFILE); \
		echo stopped; \
	else \
		echo "not running"; \
	fi

restart: stop start ## Stop, then start

run: build ## Run the cafe in the foreground
	@mkdir -p "$(PANDA_HOME)"
	PANDA_PORT=$(PORT) PANDA_HOME="$(PANDA_HOME)" PANDA_ROOT="$(PANDA_ROOT)" "$(DEBUG_BIN)"

status: ## Pid, health, listening URLs
	@if [ -f $(PIDFILE) ] && kill -0 $$(cat $(PIDFILE)) 2>/dev/null; then \
		echo "running  pid $$(cat $(PIDFILE))"; \
	else \
		echo "stopped"; \
	fi
	@curl -sf http://127.0.0.1:$(PORT)/health && echo || echo "health   down"
	@$(MAKE) --no-print-directory urls

logs: ## Tail the background server log
	@if [ -f $(LOGFILE) ]; then tail -n 80 $(LOGFILE); else echo "no log yet — make start"; fi

health: ## GET /health
	@curl -sf http://127.0.0.1:$(PORT)/health && echo || (echo "down" >&2; exit 1)

chain: ## What the running shop settles in
	@curl -sf http://127.0.0.1:$(PORT)/health \
		| python3 -c "import json,sys; d=json.load(sys.stdin); \
print('mode       %s' % d['mode']); \
print('board      %s' % d['denom']); \
print('chat       %s' % d['ai']); \
print('chain      %s' % d['chain']); \
print('settlement %s' % ('real USDC on chain' if d['onchain'] else 'Causewaybay Coin (test money)'))" \
		|| echo "down"

urls: ## Print local and LAN addresses
	@echo "local    http://127.0.0.1:$(PORT)"
	@python3 -c "import socket;\
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);\
s.connect(('8.8.8.8',80));\
print('lan      http://%s:$(PORT)' % s.getsockname()[0]);\
s.close()" 2>/dev/null || true

open: ## Open the cafe in the default browser
	@open http://127.0.0.1:$(PORT) 2>/dev/null || xdg-open http://127.0.0.1:$(PORT)

test: ## Rust protocol, SQLite, WebSocket tests
	cargo test --workspace

install-browser: ## npm install + Playwright Chromium (once)
	cd $(BROWSER) && npm install
	cd $(BROWSER) && npx playwright install chromium

test-browser: build ## Playwright guest/owner flows (own throwaway server)
	@test -d $(BROWSER)/node_modules || $(MAKE) --no-print-directory install-browser
	cd $(BROWSER) && PW_PORT=$(PW_PORT) PANDA_ROOT="$(PANDA_ROOT)" npx playwright test

test-browser-headed: build ## Playwright with a visible window
	@test -d $(BROWSER)/node_modules || $(MAKE) --no-print-directory install-browser
	cd $(BROWSER) && PW_PORT=$(PW_PORT) PANDA_ROOT="$(PANDA_ROOT)" npx playwright test --headed

test-all: test test-browser ## Rust tests, then Playwright

assets: ## Paint cafe art with Grok (needs GROK_API_KEY or XAI_API_KEY)
	chmod +x tools/grok_image.sh tools/gen_assets.sh
	tools/gen_assets.sh

assets-force: ## Repaint every Grok plate
	chmod +x tools/grok_image.sh tools/gen_assets.sh
	tools/gen_assets.sh --force

fmt: ## cargo fmt
	cargo fmt --all

check: ## cargo fmt --check + clippy + tests
	cargo fmt --all -- --check
	cargo clippy -p $(PKG) -- -D warnings
	$(MAKE) --no-print-directory test

clean: ## Remove build artifacts and Playwright reports
	cargo clean
	rm -rf $(BROWSER)/test-results $(BROWSER)/playwright-report $(BROWSER)/blob-report
	rm -f $(PIDFILE)

distclean: clean ## Also drop node_modules
	rm -rf $(BROWSER)/node_modules
