# Causewaybay Panda — plan

A cafe management and ordering system with an AI at the counter. One Rust
core runs three ways: a server in the shop (axum + SQLite), the same engine
compiled to WebAssembly inside a browser tab, and a double-clickable Mac app.
Guests order and pay from their phones; the owner runs the till, the kitchen
and the menu; a model — Grok by default — reads whatever the local parser
could not.

This file is the state of the work and what comes next. `README.md` says
what the product is; `CLAUDE.md` says how to work here.

## Where it stands (2026-09-06)

Verified by `make test-all`: 135 Rust tests, 106 Playwright tests, nothing
touching a network.

| Layer | What is there |
| --- | --- |
| `crates/protocol` | micro-USDC money, denomination table (HKD default), menu seed, wire JSON (`ClientMsg` / `ServerMsg`), the intent parser, ERC-20 `transfer` bytes |
| `crates/core` | the cafe behind `trait Store`: cart, till, kitchen queue, settlement rules, the self-running demo driver. No I/O. `MemStore` for the tab. |
| `crates/ai` | one `Ai::interpret(text, board, role) -> Option<Intent>`. Grok (`api.x.ai/v1`, `grok-4-fast`), OpenAI, Anthropic, OpenRouter, Ollama. reqwest natively, fetch in wasm. |
| `crates/server` | axum, SQLite `Store`, WebSocket hub, same-origin check on `/ws`, Cronos receipt verification for `PANDA_MODE=live` |
| `crates/web` | the core over `MemStore` behind wasm-bindgen; the page's `LocalTransport` drives it when no server answers `/health` |
| `static/` | one vanilla page: door → guest or owner; board, ticket, chat dock; owner tools (dashboard, AI setup, new dish, queue, auto switch); the guest's own card; wallet bridge |

### Dashboards

Two `ServerMsg` frames, both built in `core::cafe` from the same rows the
books show, pushed without being asked and refreshed by every payment and
every ticket that moves; "today" / "how are we doing" / "my visits" in the
chat asks for the one that fits the speaker (`Intent::Dashboard`).

- `dashboard` (owner): takings, orders, average, distinct guests, the
  kitchen split (waiting / making / ready), collected, cancelled, top five
  dishes with quantity and take.
- `guest_dashboard` (the guest it belongs to): orders here, spent, the dish
  they have had most, open orders, the status of the latest.

### How the AI is wired

1. Every chat line hits the local parser first (`cafe::intents_for_chat`).
   Dish names, quantities, pay / cart / help, owner lines — no round trip.
2. Only an `Intent::Unknown` reaches the model (`ws.rs::resolve_intents`),
   with the whole board (id, name, 中文名, price, on/off) and the role.
3. The model returns the same `Intent` schema; on any failure the parser's
   shrug stands. The Playwright `ai` project proves this end to end against
   an OpenAI-shaped stand-in.
4. Key resolution: the owner's choice at the counter (kept in SQLite
   `settings`, or the tab's localStorage) wins; else `PANDA_AI_PROVIDER`;
   else the first key found — `XAI_API_KEY` / `GROK_API_KEY` first, so a
   shop with only a Grok key needs nothing else set.

### What PLAN.md used to promise and the code does not do

Dropped, deliberately, and not coming back unless asked:

- Three.js room and Tailwind — the page is plain CSS, no vendor JS.
- Grok `grok-imagine-image` at build time — `tools/gen_assets.sh` still
  exists, the plates are committed under `static/assets`; nothing runs it.
- "Guests start with 50 USDC" — true (HK$390 on the default board) but only
  in simulation; live hands out nothing.

## Open now

**The phone flow.** README leads with a guest ordering from a phone, and it
is the one flow without a green test.

- `tests/browser/phone.spec.js` is written but untracked, and there is no
  Playwright project that runs it at a phone viewport; in the `cafe`
  project (1280×720) the sheet handle is `display:none`, so two of its five
  tests cannot pass.
- At a phone viewport (an earlier run under a since-removed `phone`
  project) the chat **Send** button is intercepted by `#transcript` and
  `#quick-btns`: the dock's fixed `--dock-h: 7.5rem` is shorter than its
  contents once a transcript line and quick buttons are both present.
- Work: fix the dock so the composer is always on top and reachable;
  add a `phone` project (e.g. Pixel 7 / iPhone 13 device descriptor) to
  `playwright.config.mjs` running `phone.spec.js` against the `cafe`
  server; add it to the `cafe` project's `testIgnore`; commit the spec.
- Housekeeping in the same change: decide whether `door-desktop.png` is a
  README screenshot or scratch. (`phone.spec.js` is parked in the `cafe`
  project's `testIgnore` until the `phone` project exists.)

## Next, in order

1. **Kitchen on the owner's phone.** The queue is tested on desktop only.
   Same `phone` project, one spec: advance a ticket, see the guest's card
   move.
2. **The model does more than order.** Today the model only maps a sentence
   onto the intent schema. Owner asks worth answering from the books:
   "what sold today", "which dish is slow", "hide everything under ten
   dollars". Add `Intent::Report` / bulk menu intents to `protocol`, teach
   the parser the plain forms, let the model fill the free ones. Stand-in
   model in the harness answers them; no network.
3. **Menu images.** New dishes the owner adds have no plate. Either an
   owner upload (bytes into SQLite, served under `/assets/menu/`) or a
   one-off `make assets` with Grok's image model for the shop's own dishes.
   Upload first; it works in a tab with no key.
4. **Receipts a guest can keep.** A paid order is a banner and a card;
   nothing survives the tab. Print-friendly `/receipt/<order>` page from the
   server, and the same view rendered from the tab's snapshot in local mode.
5. **A second table of guests.** Sessions are one name per socket. A guest
   who reloads is resumed; two phones sharing one bill are not. Model
   "table" as a session group, one cart, any phone pays.
6. **Real backend later.** `Store` is the seam. A Postgres or hosted
   implementation of the same trait would let one panda serve several
   cafes; nothing in `core` changes. Not before the shop-in-a-box is used
   by one real counter.

## Rules that stay

- Every flow a person can take has a Playwright spec; a new flow gets one.
- The local parser must handle every real order alone. A model only reads
  what the parser could not.
- Money settles in micro-USDC; what a price reads as is a separate layer.
- No REST for app traffic. HTTP is the page, the wasm bundle, the pictures,
  `/health`. Everything a person does is one WebSocket JSON frame, and the
  same frame drives the engine in a tab.
- Keys only from the environment or the owner's form; never in a tracked
  file.
