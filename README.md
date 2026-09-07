# Causewaybay Coffee · 銅鑼灣咖啡

A small food-ordering system for a cafe: guests order and pay from their
phones, the owner runs the counter, and an AI reads the free-form chat. One
Rust core runs it two ways — on a Mac in the shop, and in a browser tab with
no server at all — because the domain sits behind interfaces and never sees
which.

```
make start            # the shop on :8787; phones on the wifi open the LAN address
make web              # compile the engine to WebAssembly; static/ is then a complete cafe with no server
make pages            # the same static/, ready to publish to Cloudflare Pages
make mac              # a double-clickable "Causewaybay Panda.app"
make test-all         # Rust tests, then every Playwright project; nothing touches a network
```

## On a static host, with no backend at all

`static/` is the whole shop once `make web` has run, and `static/pkg` is
committed, so a host needs no build step:

    npx wrangler pages deploy static --project-name=<your project>

From a Git repository, set the Pages build command to nothing and the output
directory to `static`. The page looks for a cafe at `/health`, finds none, and
runs the engine in the tab instead — the same Rust the server runs, over an
in-memory store, keeping the shop in that browser's `localStorage`.

What that means in practice: **every visitor gets their own cafe**, because
their storage is the database. It is the right shape for a prototype, a demo
or a menu you hand someone, and the wrong one for two devices that need to see
the same queue — that is what `make start` is for. A tab is always a
simulation: there is no server to read a receipt, so it never takes real USDC.

**No key ships with it.** The owner opens the counter with any pin, pastes
their own key under "Who listens to the chat", and the tab calls the model
directly from the browser. Grok is offered first; OpenAI, Anthropic,
OpenRouter and Ollama are there too. The key is kept in that browser and goes
nowhere else, because there is nowhere else for it to go.

## What a guest does

Tap a dish or say it — "two lattes and an egg tart" — pay, and get a number.
The card on their phone changes as the kitchen works: received, being made,
ready. Above the board, their own tally: orders here, spent, their usual dish,
and what is still coming. Ask the panda anything — "what's good here?",
"where's my order?" — and the model answers from the board and their own
orders, with a button for each dish it names.

In a simulation they pay with **Causewaybay Coin**, test money with a faucet.
In a live shop the wallet is the purse: the page reads their **USDC on
Cronos** straight off the chain, they pay from that wallet, and the shop
verifies the receipt against the chain before booking anything.

## What the owner does

Sees the day on one card — takings, what is in the kitchen, guests served,
the average order, what is selling, and in a live shop what the treasury
holds on chain — works the queue one button at a time, adds or hides dishes
by form or by chat, and chooses who listens to the chat — Grok, OpenAI,
Anthropic, OpenRouter or Ollama — from the counter.

The AI runs the back of house when asked. "Let the panda work the kitchen"
and tickets are picked up and called ready on their own; handing over stays
a tap. "What sold today?", "how is the kitchen?" are answered from the
books, not guessed. A second switch lets the cafe run itself for a
demonstration: regulars arrive, order and pay, all through the same code a
real tap goes through.

The shop itself is set from the counter too — its name, the money the
board reads in, the till (simulation or live, chain, treasury, USDC
contract, node) and the owner pin. Kept by the shop like the choice of
model, so the next start finds it as it was left; the environment is only
the starting point. Going live is refused, with the reason, until the
treasury is named.

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

Either from the counter (the owner's "The shop" panel: live, the chain,
the treasury) or from the environment — `PANDA_MODE=live
PANDA_CHAIN=cronos_mainnet PANDA_TREASURY=0x…`, see `make help`. What the
owner kept wins over the environment. A live shop keeps its owner pin; a
simulation needs none. Keys and the setup are kept in the shop's own SQLite
(natively) or the tab's localStorage (in a browser) — on your machine, in
plain text, like any local application. A tab is always a simulation: there
is no server to read receipts.

MIT. Fonts are SIL OFL 1.1 (`static/fonts/`).
