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

Verified by `make test-all`: 155 Rust tests, 129 Playwright tests, nothing
touching a network.

| Layer | What is there |
| --- | --- |
| `crates/protocol` | micro-USDC money, denomination table (HKD default), menu seed, wire JSON (`ClientMsg` / `ServerMsg`), the intent parser, ERC-20 `transfer` bytes |
| `crates/core` | the cafe behind `trait Store`: cart, till, kitchen queue, settlement rules, the self-running demo driver. No I/O. `MemStore` for the tab. |
| `crates/ai` | one `Ai::interpret(text, board, role) -> Option<Intent>`. Grok (`api.x.ai/v1`, `grok-4-fast`), OpenAI, Anthropic, OpenRouter, Ollama. reqwest natively, fetch in wasm. |
| `crates/server` | axum, SQLite `Store`, WebSocket hub, same-origin check on `/ws`, Cronos receipt verification for `PANDA_MODE=live` |
| `crates/web` | the core over `MemStore` behind wasm-bindgen; the page's `LocalTransport` drives it when no server answers `/health` |
| `static/` | one vanilla page: door → guest or owner; board, ticket (a sheet on a phone), chat dock; owner tools (dashboard with the treasury, kitchen switch, AI setup, new dish, queue, auto switch); the guest's own card; the wallet as the purse |

### The AI at the counter

- **Reads.** Every chat line hits the local parser first; only what it
  cannot read goes to the model, with the board and the asker's own facts
  (`cafe::facts_for`): the day's figures for the owner, their orders and
  cart for a guest. Nothing a guest could not read off their own page.
- **Answers.** `Intent::Say { text, suggest }` — the model speaks as the
  panda; dishes it names that are on the board become order buttons for a
  guest, never for the owner. "What's good?", "what sold today?", "where's
  my order?" all work against a stand-in model in the suite.
- **Works the kitchen.** `core::kitchen::Kitchen` — switched on by the owner
  ("kitchen on", or the button), a placed ticket is picked up on the next
  beat and called ready three beats later, through the same `apply` as a
  tap. Handing over stays a person's. The switch is a shop setting, so a
  restart resumes it. Server beat `PANDA_KITCHEN_TICK_MS` (20 s); the tab
  beats on its own timer. Allowed in a live shop: it spends nothing.

### The shop set from the counter

`core::setup` — nine settings (`cafe.name`, `cafe.name_zh`, `shop.mode`,
`shop.denom`, `shop.denom_rate`, `chain.key`, `chain.treasury`,
`chain.usdc`, `chain.rpc_url`) laid over the environment's `shop::Config`
and `settlement::Config`; `resolve_shop` at boot, `apply` on a `setup`
frame (checked first — mode, rate, chain, addresses, URL; live refused with
the reason until the settlement is real). The `Shop` sits behind a lock in
`AppState` and both stores take a new denomination at run time; after a
change every open page gets a `shop` frame, the board in the new money,
and its own cart or figures. The pin changes in the same frame
(`Store::set_pin`). A tab applies the same code but never goes live.

### The chain in the page

- A live guest's purse is their wallet: `eth_accounts` on the way in (no
  prompt), a **Connect wallet** button otherwise, then `balanceOf` through
  the wallet's own node, shown in USDC and in the board's money. Re-read
  after every payment.
- The owner's card carries the treasury's on-chain USDC, read server-side
  (`verify::balance_of`) on login and after every confirmed payment, linked
  to the explorer. A simulation sends no `treasury` frame at all.
- Chain payments in the till link to their transaction.

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

### Done since the first plan

- The phone flow: a `phone` Playwright project (Pixel 7); the sheet opens
  for the cart's first line however it got there and then leaves the guest's
  choice alone; the owner's queue buttons are a thumb's size.
- Dashboards for both doors; the AI answering and working the kitchen; the
  wallet as the purse and the treasury on the card; the shop set up from
  the counter (above).

## Open now

Nothing blocking. `door-desktop.png` at the root is untracked: a README
screenshot or scratch, the owner's call.

## Next, in order

1. **The model acts on the books.** It answers from them now; let it also
   change them in bulk on the owner's word — "hide everything under ten
   dollars", "put the buns up ten percent" — as a list of the same menu
   intents, shown for a confirming tap before they run.
2. **Menu images.** New dishes the owner adds have no plate. Either an
   owner upload (bytes into SQLite, served under `/assets/menu/`) or a
   one-off `make assets` with Grok's image model for the shop's own dishes.
   Upload first; it works in a tab with no key.
3. **Receipts a guest can keep.** A paid order is a banner and a card;
   nothing survives the tab. Print-friendly `/receipt/<order>` page from the
   server, and the same view rendered from the tab's snapshot in local mode.
4. **A second table of guests.** Sessions are one name per socket. A guest
   who reloads is resumed; two phones sharing one bill are not. Model
   "table" as a session group, one cart, any phone pays.
5. **Real backend later.** `Store` is the seam. A Postgres or hosted
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
- The environment is a default. Anything an owner would reasonably change
  is changeable from the counter and kept by the shop.
