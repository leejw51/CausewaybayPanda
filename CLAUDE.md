# Causewaybay Panda

## Goal

This project is a usable prototype for **AI cafe management and customer
handling**. An owner runs the counter; guests order and pay. Everything a
person does goes through one JSON frame protocol, whether it reaches a server
over a WebSocket or the same engine compiled to WebAssembly inside the tab.

## Shape

    crates/protocol   money, denominations, menu, wire JSON, chat intents, ERC-20 bytes
    crates/core       the cafe itself behind a Store trait: cart, till, kitchen, demo driver. No I/O.
    crates/ai         one Ai interface; native reqwest or the browser's fetch. Grok, OpenAI, Anthropic, OpenRouter, Ollama.
    crates/server     axum + SQLite implementing Store; websocket hub; chain receipts; the AI at run time.
    crates/web        the core over an in-memory Store, behind wasm-bindgen, for a tab with no server.
    static/           one vanilla page. Socket when a server answers /health, engine when none does.

Interfaces, applied web or native: `Store` (SQLite natively, `MemStore` in a
tab) and `Ai` (reqwest natively, fetch in a tab). The domain never sees which.

## Money

Everything settles in micro-USDC. What a price *reads* as is a separate layer
(`PANDA_DENOM`, HKD by default; hardcoded table, `PANDA_DENOM_RATE` corrects).
Simulation is the default and hands out Causewaybay Coin with a faucet; a
simulation needs no owner pin. `PANDA_MODE=live` takes real USDC on Cronos, verifies
the receipt against the chain before booking, and keeps the pin. The owner
can set all of this from the counter (`core::setup`); what they keep is a
shop setting and wins over the environment.

## Working here

- `make test-all` — Rust tests, then every Playwright project. Run it before
  saying anything is done. Nothing in the suite touches a network: the chain,
  the wallet and the model are all stand-ins (`tests/browser/harness.mjs`).
- **Write testing code and verify. Check all flows with Playwright.** Every
  flow a person can take — guest, owner, tab-only, live, model — has a spec
  under `tests/browser/`; a new flow gets one. Tests share one shop per run:
  use `payAndNumber` rather than assuming order numbers, and restore anything
  you toggle.
- `make web` rebuilds the wasm engine into `static/pkg` (commit it); `make mac`
  builds the double-clickable app; `make start` runs the dev shop on :8787.
- Prices in tests are HKD strings (`HK$38.00`); a latte is HK$38, an egg tart HK$10.
- The local parser must handle every real order on its own. A model only reads
  what the parser could not.
