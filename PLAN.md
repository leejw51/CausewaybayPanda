# Causewaybay Panda — plan

A restaurant-local food ordering box. One process per cafe. Guests order from
a phone or tablet in the browser. The owner runs the same binary in the shop.
There is no cloud bill: SQLite on disk, WebSocket on the LAN, optional Grok
only when the owner sets a key.

Borrowed, not copied:

| From | What we take |
| --- | --- |
| [CausewaybayZkp](../CausewaybayZkp) | Causeway Bay night, tram, neon CAFE, 16-bit candy palette (sky / cream / brick / brass). Street art reused as the Three.js window. |
| [PocketSkynet](../PocketSkynetHome/PocketSkynet) | Axum + SQLite + JSON WebSocket. Chat as the command surface. Browser holds the wallet; the server never sees a key. |
| [CausewaybayWallet](../CausewaybayWallet) | ERC-20 `transfer(address,uint256)` encoding, USDC 6 decimals, Cronos as the EVM home. |

## Product

Virtual test cafe: **Causewaybay Coffee** (銅鑼灣咖啡).

Two doors, both one tap:

- **Guest** — pick food, talk to the panda, pay in USDC.
- **Owner** — write the menu, watch payments land.

Every action exists twice: a **large button** and a **chat line**. A kid, a
grandparent, and a regular who types "two lattes" all reach the same intent.

## Stack

```
browser  --JSON/WebSocket-->  panda (Rust axum)
                                  |
                                  +- SQLite  ~/.causewaybaypanda/panda.sqlite
                                  +- optional Grok (chat NLU, asset paint)
```

| Layer | Choice |
| --- | --- |
| Server | Rust, axum 0.8, rusqlite bundled, one JSON WebSocket at `/ws` |
| Client | One vanilla page. It speaks to a server over a WebSocket, or to the same cafe compiled to WebAssembly inside the tab when no server answers. |
| JS only | Tailwind (theme + utilities), Three.js (the cafe room) |
| Pay | Demo USDC ledger (always on, $0). Optional injected wallet, same calldata as CausewaybayWallet. |
| Art | Grok `grok-imagine-image` at `make assets`. ZKP street plates for the window. |

No REST for app traffic. HTTP is the page, the WASM bundle, the pictures, and
`GET /health`. Everything a person does is a WebSocket JSON frame.

## Wire protocol

Every frame is UTF-8 JSON with a `"type"` string. Unknown types are ignored.

Client → server:

```json
{ "type": "login", "role": "guest", "name": "Mei" }
{ "type": "login", "role": "owner", "pin": "panda" }
{ "type": "chat", "text": "two lattes and an egg tart" }
{ "type": "action", "name": "add", "item_id": "latte", "qty": 1 }
{ "type": "action", "name": "pay", "method": "usdc" }
{ "type": "action", "name": "menu_upsert", "item": { "...": "..." } }
{ "type": "ping" }
```

Server → client: `welcome`, `menu`, `cart`, `assistant` (text + buttons),
`orders`, `payments`, `paid`, `error`, `pong`. Menu edits broadcast to every
open guest. A payment broadcasts to every open owner.

## Chat

A pure intent parser (no network) always runs. It understands menu names,
quantities, pay / cart / help, and owner lines like `add mango pudding 3.20`.
When `XAI_API_KEY` or `GROK_API_KEY` is set, the same utterance is also sent
to Grok and mapped onto that intent schema; on any failure we keep the local
parse. Buttons emit the `action` frames directly. Chat and buttons never
diverge: both hit `Intent` in `crates/protocol`.

## Money

Prices live in **micro-USDC** (6 decimals), same as the wallet's USDC row.

Guests in the virtual cafe start with **50 USDC** play money. Pay debitsthe
ledger, writes a `payments` row, and prints a kitchen ticket. Labelled as
demo so nobody thinks the chain moved.

Optional real path: the client asks `window.ethereum` to send USDC to the
cafe treasury with `transfer(address,uint256)` bytes from
`causewaybay_wallet::erc20`. Cronos testnet (338) so a live trial still
costs nothing. The server records the hash; it does not hold keys.

## Who runs what

Each owner starts `panda` in the restaurant. One SQLite file is that cafe.
Default owner PIN is `panda` (override `PANDA_OWNER_PIN`). Bind
`0.0.0.0:8787` so a phone on the shop Wi-Fi opens the same page.

## Design

Not a SaaS card grid. A cha chaan teng that looked out on Causeway Bay and
kept the neon.

| Token | Hex | Job |
| --- | --- | --- |
| Espresso | `#1A0F0A` | night wood, page ground |
| Steamed milk | `#F3E6D4` | type and foam |
| Brass | `#C4A35A` | rails, prices, paid |
| Jade tile | `#2F6B5A` | HK mosaic, confirm |
| Neon | `#E23C8A` | one sign, one accent |
| Harbour cyan | `#5CE1E6` | tram light, links |

Type: **Noto Serif HK** on the cafe name and dishes (bilingual), **Sora** on
the giant buttons. Buttons are palm-sized. Chat docked at the bottom. Desktop
splits the Three.js window (left) from the ticket (right). Phone stacks the
room as a short header.

Three.js: a booth, a window onto the ZKP street, warm hanging lights, the
panda barista as a billed sprite. Moods `idle` / `ordering` / `paid` — one
light change per action, no decoration animation.

## Layout

```
crates/protocol   JSON types, intent parser, ERC-20 transfer bytes
crates/protocol   money, denominations, menu, wire JSON, chat intents, ERC-20 bytes
crates/core       the cafe itself: Store trait, cart, till, kitchen, demo driver — no I/O
crates/server     axum + SQLite implementing Store, websocket hub, AI providers, chain receipts
crates/web        the core over an in-memory Store, behind wasm-bindgen, for a tab with no server
static/           Three.js scene, vendor JS, generated art
tools/            Grok image painter
```
