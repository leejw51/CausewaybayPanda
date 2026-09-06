# Causewaybay Coffee · 銅鑼灣咖啡

A small food-ordering system for a cafe: guests order and pay from their
phones, the owner runs the counter, and an AI reads the free-form chat. One
Rust core runs it three ways — on a Mac in the shop, in a browser tab with no
server at all, and (later) on a real backend — because the domain sits behind
interfaces and never sees which.

```
make start            # the shop on :8787; phones on the wifi open the LAN address
make web              # compile the engine to WebAssembly; static/ is then a complete cafe with no server
make mac              # a double-clickable "Causewaybay Panda.app"
make test-all         # 135 Rust + 106 Playwright tests; nothing touches a network
```

## What a guest does

Tap a dish or say it — "two lattes and an egg tart" — pay, and get a number.
The card on their phone changes as the kitchen works: received, being made,
ready. Above the board, their own tally: orders here, spent, their usual dish,
and what is still coming. In a simulation they pay with **Causewaybay Coin**, test money with a
faucet. In a live shop they pay **real USDC on Cronos** from their own wallet,
and the shop verifies the receipt against the chain before booking anything.

## What the owner does

Sees the day on one card — takings, what is in the kitchen, guests served,
the average order, what is selling — works the queue one button at a time,
adds or hides dishes by form or by chat, and chooses who listens to the chat — Grok,
OpenAI, Anthropic, OpenRouter or Ollama — from the counter. A switch lets the
cafe run itself for a demonstration: regulars arrive, order and pay, the
kitchen works the tickets, all through the same code a real tap goes through.

## Money

Everything settles in micro-USDC; what a price *reads* as is a separate layer.
`PANDA_DENOM=HKD` (default) or KRW, JPY, CNY, TWD, SGD, EUR, GBP, USD, USDC — a
hardcoded table, `PANDA_DENOM_RATE` corrects it.

## Shape

    crates/protocol   money, denominations, menu, wire JSON, chat intents, ERC-20 bytes
    crates/core       the cafe behind a Store trait: cart, till, kitchen, demo driver. No I/O.
    crates/ai         one Ai interface, five providers; reqwest natively, fetch in a tab
    crates/server     axum + SQLite implementing Store; websocket hub; chain receipts
    crates/web        the core over an in-memory Store, behind wasm-bindgen
    static/           one vanilla page: socket when a server answers /health, engine when none does

## Running it for real

`PANDA_MODE=live PANDA_CHAIN=cronos_mainnet PANDA_TREASURY=0x…` — see
`make help`. A live shop keeps its owner pin; a simulation needs none. Keys
are kept in the shop's own SQLite (natively) or the tab's localStorage (in a
browser) — on your machine, in plain text, like any local application.

MIT. Fonts are SIL OFL 1.1 (`static/fonts/`).
