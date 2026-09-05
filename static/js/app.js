/* Causewaybay Coffee — JSON WebSocket client. Buttons and chat share one send path. */
(() => {
  const $ = (id) => document.getElementById(id);
  const KEY = "causewaybay.session";
  // What this browser remembers: enough to walk back in as the same person.
  function remembered() {
    try {
      return JSON.parse(localStorage.getItem(KEY) || "null");
    } catch {
      return null;
    }
  }
  function remember(role, name, session_id) {
    try {
      localStorage.setItem(KEY, JSON.stringify({ role, name, session_id }));
    } catch {
      /* private mode; a reload is a new guest, which is fine */
    }
  }
  function forget() {
    try {
      localStorage.removeItem(KEY);
    } catch {
      /* nothing to forget */
    }
  }
  const state = {
    transport: null,
    role: null,
    menu: [],
    cart: [],
    total: "0",
    balance: "0",
    payments: [],
    settlement: null,
    paying: false,
    orders: new Map(),
    canFaucet: false,
    auto: false,
    ai: null,
  };

  const STATUS_LINE = {
    placed: "Order received",
    preparing: "Being made",
    ready: "Ready — come and get it",
    collected: "Collected",
    cancelled: "Cancelled",
  };
  const NEXT_STEP = {
    placed: { label: "Start making" },
    preparing: { label: "Mark ready" },
    ready: { label: "Handed over" },
  };

  function show(el, on) {
    if (!el) return;
    el.hidden = !on;
    el.classList.toggle("hidden", !on);
  }

  function send(obj) {
    if (state.transport) state.transport.send(obj);
  }

  function action(name, extra) {
    send(
      Object.assign(
        { type: "action", name, item_id: "", qty: 1, tx_hash: "", order_id: "", status: "", on: false },
        extra || {}
      )
    );
  }

  /** A server if there is one, the cafe engine in this tab if not. */
  async function connect() {
    const t = await window.PandaTransport.open(onMsg);
    state.transport = t;
    document.body.classList.toggle("local", Boolean(t.local));
    // A simulation has no till to lock, so the door says the counter is open.
    const note = $("local-note");
    if (note) {
      if (t.local) {
        note.textContent = "This tab is the whole cafe. It is a simulation: any pin opens the counter.";
        show(note, true);
      } else if (t.mode === "simulation") {
        note.textContent = "This shop is a simulation: any pin opens the counter.";
        show(note, true);
      }
    }
    t.onOpen = () => {
      // A reload, or the wifi dropping for a moment: pick the same session
      // up rather than starting a stranger at the door. Only a guest walks
      // back in on their own; the owner is asked for the pin again.
      const held = remembered();
      if (held && held.role === "guest" && held.session_id) {
        send({ type: "login", role: "guest", name: held.name || "guest", pin: "", session: held.session_id });
      }
    };
    if (t.local) t.onOpen();
  }

  function onMsg(msg) {
    switch (msg.type) {
      case "welcome":
        state.role = msg.role;
        state.balance = msg.balance_usdc;
        state.settlement = msg.settlement || null;
        state.orders = new Map((msg.orders || []).map((o) => [o.id, o]));
        remember(msg.role, msg.name, msg.session_id);
        if (msg.role === "guest" && msg.name) $("guest-name").value = msg.name;
        $("role-label").textContent = msg.role;
        $("balance").textContent = msg.balance_display || msg.balance_usdc;
        show($("stage-door"), false);
        show($("stage-app"), true);
        document.body.classList.toggle("as-owner", msg.role === "owner");
        show($("owner-tools"), msg.role === "owner");
        show($("pay-usdc"), msg.role === "guest");
        renderSettlement();
        renderMyOrders();
        renderQueue();
        renderAuto();
        break;
      case "menu":
        state.menu = msg.items || [];
        renderMenu();
        break;
      case "cart":
        state.cart = msg.lines || [];
        state.total = msg.total_usdc;
        state.balance = msg.balance_usdc;
        state.canFaucet = Boolean(msg.can_faucet);
        $("balance").textContent = msg.balance_display || msg.balance_usdc;
        $("cart-total").textContent = msg.total_display || msg.total_usdc;
        renderCart();
        renderFaucet();
        break;
      case "order_update": {
        const was = state.orders.get(msg.order.id);
        state.orders.set(msg.order.id, msg.order);
        renderMyOrders();
        renderQueue();
        // The sign coming on is the one moment worth interrupting for.
        if (state.role === "guest" && msg.order.status === "ready" && (!was || was.status !== "ready")) {
          closeSheet();
          showMyOrders();
        }
        break;
      }
      case "takings":
        renderTakings(msg);
        break;
      case "auto":
        state.auto = Boolean(msg.on);
        renderAuto();
        break;
      case "ai_status":
        state.ai = msg;
        renderAiSetup();
        break;
      case "assistant":
        addLine(msg.text);
        renderQuick(msg.buttons || []);
        break;
      case "payments":
        state.payments = msg.payments || [];
        renderPayments();
        break;
      case "orders":
        renderOrders(msg.orders || []);
        break;
      case "paid":
        renderPaid(msg);
        // On a phone the sheet was covering the board; the thing to look at
        // now is the order card, so put it in front of them.
        closeSheet();
        showMyOrders();
        break;
      case "pay_request":
        settleWithWallet(msg);
        break;
      case "error":
        walletBusy(false);
        if (!state.role) forget();
        addLine(msg.message);
        const door = $("door-error");
        if (!$("stage-app") || $("stage-app").hidden) {
          door.textContent = msg.message;
          show(door, true);
        }
        break;
      default:
        break;
    }
  }

  /** Say plainly which shop this is: test money, or real USDC. */
  function renderSettlement() {
    const s = state.settlement;
    const guest = state.role === "guest";
    const badge = $("mode-badge");
    const btn = $("pay-wallet");
    const note = $("chain-note");
    if (!s) return;

    if (badge) {
      const sim = s.mode === "simulation";
      badge.textContent = sim
        ? `Test money: ${s.coin_name}. Prices in ${s.denom.code}.`
        : `Real USDC on ${s.chain_name}. Prices in ${s.denom.code}.`;
      badge.className = sim ? "mode-line" : "mode-line live";
      show(badge, true);
    }
    // A purse only means something to a guest spending the shop's test money.
    // In a live shop the money is in their own wallet, which we cannot read.
    const purseBox = document.querySelector(".purse");
    if (purseBox) purseBox.hidden = !guest || s.mode !== "simulation";
    const purse = $("purse-label");
    if (purse) purse.textContent = s.mode === "simulation" ? s.coin_name : "Wallet";
    const priceLabel = $("price-label");
    if (priceLabel) priceLabel.textContent = `Price in ${s.denom.code}`;
    const priceBox = $("new-price");
    if (priceBox) priceBox.placeholder = s.denom.symbol ? `${s.denom.symbol}38` : "38";

    if (!btn || !note) return;
    if (!guest) {
      show(btn, false);
      show(note, false);
      return;
    }
    const wallet = window.PandaWallet && window.PandaWallet.available();
    show(btn, Boolean(s.onchain && wallet));
    if (s.onchain && wallet) {
      btn.textContent = `Pay with wallet · ${s.chain_name}`;
      note.textContent = `USDC ${short(s.usdc_address)} on ${s.chain_name}.`;
      show(note, true);
    } else if (s.onchain) {
      note.textContent = `This shop takes USDC on ${s.chain_name}. Open in a wallet browser to pay on chain.`;
      show(note, true);
    } else {
      note.textContent = `Paying in ${s.coin_name}. It is test money — nothing real is spent.`;
      show(note, true);
    }
    renderFaucet();
  }

  /** The faucet is a simulation affordance and nothing else. */
  function renderFaucet() {
    const btn = $("faucet");
    if (!btn) return;
    const s = state.settlement;
    const on = Boolean(s && s.mode === "simulation" && state.role === "guest");
    show(btn, on);
    btn.disabled = on && !state.canFaucet;
    btn.title = state.canFaucet
      ? `Add ${s ? s.faucet_display : ""}`
      : `You are at the ${s ? s.faucet_cap_display : ""} ceiling`;
  }

  /** The card a guest watches while the kitchen works. */
  function renderMyOrders() {
    const box = $("my-orders");
    if (!box) return;
    const mine = [...state.orders.values()]
      .filter((o) => o.status !== "collected" && o.status !== "cancelled")
      .sort((a, b) => a.order_no - b.order_no);
    if (state.role !== "guest" || !mine.length) {
      box.innerHTML = "";
      show(box, false);
      return;
    }
    box.innerHTML = "";
    for (const o of mine) {
      const card = document.createElement("div");
      card.className = `order-card ${o.status}`;
      card.setAttribute("data-testid", `my-order-${o.order_no}`);
      const no = document.createElement("strong");
      no.textContent = `#${o.order_no}`;
      const status = document.createElement("span");
      status.className = "order-status";
      status.setAttribute("data-testid", "my-order-status");
      status.textContent = STATUS_LINE[o.status] || o.status;
      const what = document.createElement("span");
      what.className = "order-what";
      what.textContent = o.lines.map((l) => `${l.qty}× ${l.name}`).join(", ");
      card.append(no, status, what);
      box.appendChild(card);
    }
    show(box, true);
  }

  /** The owner's choice of who listens to the chat. */
  function renderAiSetup() {
    const a = state.ai;
    const sel = $("ai-provider");
    if (!a || !sel) return;
    const status = $("ai-status");
    status.textContent = a.ready ? `${labelFor(a.provider)} · ${a.model}` : "Local parser only";
    status.classList.toggle("on", a.ready);
    // Fill the choices once; keep the owner's current pick.
    if (!sel.options.length) {
      for (const p of a.providers) {
        const o = document.createElement("option");
        o.value = p.key;
        o.textContent = p.label;
        sel.appendChild(o);
      }
      sel.addEventListener("change", () => {
        // A new provider starts from its own default model; the old model's
        // name would be meaningless to it.
        $("ai-model").value = "";
        renderAiHint();
      });
    }
    if (a.ready) sel.value = a.provider;
    $("ai-model").value = a.ready ? a.model : "";
    renderAiHint();
  }

  function labelFor(key) {
    const p = state.ai && state.ai.providers.find((x) => x.key === key);
    return p ? p.label : key;
  }

  function renderAiHint() {
    const a = state.ai;
    const p = a && a.providers.find((x) => x.key === $("ai-provider").value);
    if (!p) return;
    $("ai-hint").textContent = p.needs_key
      ? `Get a key at ${p.hint}. Default model: ${p.default_model}.`
      : `${p.hint}. Default model: ${p.default_model}.`;
    $("ai-key").disabled = !p.needs_key;
    $("ai-key").placeholder = p.needs_key
      ? a.ready && a.provider === p.key
        ? "a key is held — leave empty to keep it"
        : "paste a key"
      : "no key needed";
    $("ai-model").placeholder = p.default_model;
  }

  /** The switch that lets the cafe run itself. The owner's, always; a guest's
      too when this tab is the whole cafe and there is nobody at the counter. */
  function renderAuto() {
    for (const id of ["auto-owner", "auto-guest"]) {
      const btn = $(id);
      if (!btn) continue;
      const local = Boolean(state.transport && state.transport.local);
      const mine = id === "auto-owner" ? state.role === "owner" : state.role === "guest" && local;
      show(btn, mine);
      btn.textContent = state.auto ? "Stop the cafe" : "Run the cafe on its own";
      btn.classList.toggle("running", state.auto);
      btn.setAttribute("aria-pressed", String(state.auto));
    }
    const dot = $("auto-dot");
    if (dot) show(dot, state.auto);
  }

  /** Today so far, at the top of the counter. */
  function renderTakings(t) {
    const total = $("takings-total");
    if (!total) return;
    total.textContent = t.total_display;
    $("takings-count").textContent =
      t.orders === 1 ? "from 1 order" : `from ${t.orders} orders`;
    const split = $("takings-split");
    const s = state.settlement;
    if (!s) {
      split.textContent = "";
    } else if (s.mode === "simulation") {
      split.textContent = `All in ${s.coin_name}, which is test money.`;
    } else {
      split.textContent = `${t.wallet_display} in USDC on ${s.chain_name}.`;
    }
  }

  /** The counter's queue: oldest first, one button to move each ticket on. */
  function renderQueue() {
    const box = $("queue");
    if (!box || state.role !== "owner") return;
    const open = [...state.orders.values()]
      .filter((o) => o.status !== "collected" && o.status !== "cancelled")
      .sort((a, b) => a.order_no - b.order_no);
    box.innerHTML = "";
    if (!open.length) {
      const p = document.createElement("p");
      p.className = "quiet";
      p.setAttribute("data-testid", "queue-empty");
      p.textContent = "No orders waiting.";
      box.appendChild(p);
      return;
    }
    for (const o of open) {
      const row = document.createElement("div");
      row.className = `ticket-row ${o.status}`;
      row.setAttribute("data-testid", `ticket-${o.order_no}`);

      const head = document.createElement("div");
      head.className = "ticket-head";
      const no = document.createElement("strong");
      no.textContent = `#${o.order_no}`;
      const who = document.createElement("span");
      who.textContent = o.guest;
      const total = document.createElement("span");
      total.className = "price";
      total.textContent = o.total_display;
      head.append(no, who, total);

      const what = document.createElement("p");
      what.className = "ticket-what";
      what.textContent = o.lines.map((l) => `${l.qty}× ${l.name}`).join(", ");

      const acts = document.createElement("div");
      acts.className = "ticket-acts";
      const next = NEXT_STEP[o.status];
      if (next) {
        const go = document.createElement("button");
        go.type = "button";
        go.className = "btn small jade";
        go.setAttribute("data-testid", `ticket-next-${o.order_no}`);
        go.textContent = next.label;
        go.addEventListener("click", () => action("order_advance", { order_id: o.id }));
        acts.appendChild(go);
      }
      const off = document.createElement("button");
      off.type = "button";
      off.className = "btn small ghost";
      off.setAttribute("data-testid", `ticket-cancel-${o.order_no}`);
      off.textContent = "Cancel";
      off.addEventListener("click", () => action("order_cancel", { order_id: o.id }));
      acts.appendChild(off);

      row.append(head, what, acts);
      box.appendChild(row);
    }
  }

  function short(addr) {
    return addr && addr.length > 12 ? `${addr.slice(0, 6)}…${addr.slice(-4)}` : addr;
  }

  function walletBusy(on) {
    state.paying = on;
    const btn = $("pay-wallet");
    if (!btn) return;
    btn.disabled = on;
    btn.textContent = on
      ? "Check your wallet…"
      : `Pay with wallet · ${state.settlement ? state.settlement.chain_name : ""}`;
  }

  /** The server priced the cart and handed over a transfer. Sign it, then give
      the hash back so the order is recorded against a real transaction. */
  async function settleWithWallet(req) {
    if (!window.PandaWallet || !state.settlement) return;
    walletBusy(true);
    try {
      const hash = await window.PandaWallet.pay(state.settlement, req);
      addLine(`Sent ${req.amount_usdc} USDC. Waiting for the panda to see it…`);
      action("pay", { method: "wallet", tx_hash: hash });
    } catch (err) {
      addLine(`Wallet: ${window.PandaWallet.reason(err)}`);
    } finally {
      walletBusy(false);
    }
  }

  function renderPaid(msg) {
    const box = $("paid-banner");
    box.textContent = "";
    const head = document.createElement("span");
    const onchain = Boolean(msg.explorer_url);
    head.textContent = `Paid ${msg.amount_display || msg.amount_usdc} for order #${msg.order_no}${
      onchain ? ". " : "."
    }`;
    box.append(head);
    if (onchain) {
      const a = document.createElement("a");
      a.href = msg.explorer_url;
      a.target = "_blank";
      a.rel = "noopener noreferrer";
      a.textContent = short(msg.tx_hash);
      a.setAttribute("data-testid", "paid-link");
      box.append(a);
    }
    show(box, true);
  }

  function addLine(text) {
    const box = $("transcript");
    const p = document.createElement("p");
    p.textContent = text;
    box.appendChild(p);
    box.scrollTop = box.scrollHeight;
  }

  function plate(item) {
    const el = document.createElement("span");
    el.className = "plate";
    el.textContent = (item.name || "?").trim().charAt(0).toUpperCase();
    return el;
  }

  function renderMenu() {
    const grid = $("menu-grid");
    grid.innerHTML = "";
    const items = state.role === "guest" ? state.menu.filter((i) => i.available) : state.menu;
    for (const item of items) {
      const b = document.createElement("button");
      b.type = "button";
      b.className = item.available ? "dish" : "dish off";
      b.dataset.testid = "dish-" + item.id;
      b.setAttribute("data-testid", "dish-" + item.id);
      // A dish the owner just wrote has no plate yet; an empty src draws a
      // broken-image glyph, so stand a lettered tile in its place.
      let img;
      if (item.image) {
        img = document.createElement("img");
        img.alt = "";
        img.src = item.image;
        img.addEventListener("error", () => img.replaceWith(plate(item)));
      } else {
        img = plate(item);
      }
      const body = document.createElement("span");
      body.className = "body";
      const name = document.createElement("span");
      name.className = "name";
      name.textContent = item.name;
      const zh = document.createElement("span");
      zh.className = "zh";
      zh.textContent = item.name_zh;
      const price = document.createElement("span");
      price.className = "price";
      price.textContent = item.price_display || `${item.price_usdc} USDC`;
      body.append(name, zh, price);
      if (!item.available) {
        // Real text, so it reaches a screen reader as well as the eye.
        const badge = document.createElement("span");
        badge.className = "badge";
        badge.textContent = "off the board";
        body.append(badge);
      }
      b.append(img, body);
      b.addEventListener("click", () => {
        if (state.role === "guest") {
          action("add", { item_id: item.id, qty: 1 });
          if (matchMedia("(max-width: 800px)").matches) {
            $("ticket").classList.add("open");
            $("sheet-toggle").setAttribute("aria-expanded", "true");
          }
        } else {
          action(item.available ? "menu_hide" : "menu_show", { item_id: item.id });
        }
      });
      grid.appendChild(b);
    }
  }

  function renderCart() {
    const ul = $("cart-lines");
    ul.innerHTML = "";
    for (const line of state.cart) {
      const li = document.createElement("li");
      li.setAttribute("data-testid", "cart-" + line.item_id);

      const left = document.createElement("span");
      left.className = "cart-name";
      left.textContent = `${line.qty}× ${line.name}`;

      const steps = document.createElement("span");
      steps.className = "steps";
      const less = document.createElement("button");
      less.type = "button";
      less.className = "step";
      less.setAttribute("aria-label", `One fewer ${line.name}`);
      less.setAttribute("data-testid", "less-" + line.item_id);
      less.textContent = "−";
      less.addEventListener("click", () =>
        action("set_qty", { item_id: line.item_id, qty: line.qty - 1 })
      );
      const more = document.createElement("button");
      more.type = "button";
      more.className = "step";
      more.setAttribute("aria-label", `One more ${line.name}`);
      more.setAttribute("data-testid", "more-" + line.item_id);
      more.textContent = "+";
      more.addEventListener("click", () =>
        action("set_qty", { item_id: line.item_id, qty: line.qty + 1 })
      );
      steps.append(less, more);

      const right = document.createElement("span");
      right.className = "price";
      right.textContent = line.line_display || line.line_usdc;

      li.append(left, steps, right);
      ul.appendChild(li);
    }
    show($("cart-empty"), state.cart.length === 0);
    renderSheetSummary();
    const pay = $("pay-usdc");
    if (pay) pay.disabled = state.cart.length === 0;
    const wallet = $("pay-wallet");
    if (wallet) wallet.disabled = state.cart.length === 0 || state.paying;
  }

  function openBooks() {
    const d = document.querySelector(".books");
    if (d) d.open = true;
  }

  function closeSheet() {
    const t = $("ticket");
    if (!t) return;
    t.classList.remove("open");
    $("sheet-toggle").setAttribute("aria-expanded", "false");
  }

  function showMyOrders() {
    const box = $("my-orders");
    if (box && !box.hidden) box.scrollIntoView({ block: "start", behavior: "smooth" });
  }

  /** The one line a phone shows when the sheet is closed. */
  function renderSheetSummary() {
    const el = $("sheet-summary");
    if (!el) return;
    const n = state.cart.reduce((a, l) => a + l.qty, 0);
    const total = $("cart-total").textContent;
    el.textContent =
      n === 0 ? "Your order is empty" : `${n} ${n === 1 ? "item" : "items"}, ${total}`;
  }

  function renderPayments() {
    const box = $("payments-list");
    if (!box) return;
    openBooks();
    box.innerHTML = "";
    const h = document.createElement("h3");
    h.textContent = "Payments";
    box.appendChild(h);
    for (const p of state.payments) {
      const d = document.createElement("div");
      d.className = "pay-row";
      d.setAttribute("data-testid", "payment-row");
      d.textContent = `${p.guest} · ${p.amount_display || p.amount_usdc} · ${p.method}`;
      box.appendChild(d);
    }
  }

  function renderOrders(orders) {
    // The owner's full book. The live queue above is the working view.
    for (const o of orders) state.orders.set(o.id, o);
    renderQueue();
    openBooks();
    const box = $("orders-list");
    box.innerHTML = "";
    for (const o of orders) {
      const d = document.createElement("div");
      d.className = "order-row";
      d.setAttribute("data-testid", "order-row");
      d.textContent = `#${o.order_no} · ${o.guest} · ${o.total_display} · ${o.status}`;
      box.appendChild(d);
    }
  }

  function renderQuick(buttons) {
    const box = $("quick-btns");
    box.innerHTML = "";
    for (const b of buttons) {
      const el = document.createElement("button");
      el.type = "button";
      el.textContent = b.label;
      el.setAttribute("data-testid", "quick-" + b.action);
      el.addEventListener("click", () =>
        action(b.action, { item_id: b.item_id || "", qty: b.qty || 1 })
      );
      box.appendChild(el);
    }
  }

  // The mascot plate ships on a flat pink card. Sample that colour from a
  // corner and flood it in from the edges, so pink inside the panda stays.
  function knockoutMascot() {
    const canvas = $("mascot");
    if (!canvas) return;
    const img = new Image();
    img.onload = () => {
      const w = canvas.width;
      const h = canvas.height;
      const ctx = canvas.getContext("2d", { willReadFrequently: true });
      ctx.clearRect(0, 0, w, h);
      ctx.drawImage(img, 0, 0, w, h);
      const data = ctx.getImageData(0, 0, w, h);
      const px = data.data;
      const plate = [px[0], px[1], px[2]];
      const dist2 = (i) => {
        const dr = px[i] - plate[0];
        const dg = px[i + 1] - plate[1];
        const db = px[i + 2] - plate[2];
        return dr * dr + dg * dg + db * db;
      };
      const cut = 70 * 70;
      const feather = 130 * 130;

      const seen = new Uint8Array(w * h);
      const stack = [];
      for (let x = 0; x < w; x++) stack.push(x, x + (h - 1) * w);
      for (let y = 0; y < h; y++) stack.push(y * w, w - 1 + y * w);
      while (stack.length) {
        const at = stack.pop();
        if (seen[at]) continue;
        seen[at] = 1;
        const i = at * 4;
        const d = dist2(i);
        if (d > feather) continue;
        // Flat field goes fully clear; the antialiased rim gets a soft ramp.
        px[i + 3] = d <= cut ? 0 : Math.round(255 * ((d - cut) / (feather - cut)));
        if (px[i + 3] !== 0) continue;
        const x = at % w;
        const y = (at / w) | 0;
        if (x > 0) stack.push(at - 1);
        if (x < w - 1) stack.push(at + 1);
        if (y > 0) stack.push(at - w);
        if (y < h - 1) stack.push(at + w);
      }
      ctx.putImageData(data, 0, 0);
    };
    img.src = "/assets/panda.png";
  }

  function login(role, name, pin) {
    // A guest walks back into their own session. The owner may too, but only
    // alongside the pin — the server never takes a remembered id as a key.
    const held = remembered();
    const session = held && held.role === role ? held.session_id : "";
    send({ type: "login", role, name, pin: pin || "", session });
  }
  $("login-guest").addEventListener("click", () => {
    login("guest", $("guest-name").value || "guest", "");
  });
  $("login-owner").addEventListener("click", () => {
    login("owner", "owner", $("owner-pin").value || "");
  });
  // The plain Pay button names no method: the shop takes whatever it takes.
  $("pay-usdc").addEventListener("click", () => action("pay", {}));
  $("faucet").addEventListener("click", () => action("faucet", {}));
  // Back to the door as nobody: the remembered session is dropped, so the
  // next person in — or the same person at the other door — starts clean.
  $("leave").addEventListener("click", () => {
    forget();
    location.reload();
  });
  for (const id of ["auto-owner", "auto-guest"]) {
    $(id).addEventListener("click", () => action("auto", { on: !state.auto }));
  }
  $("ai-save").addEventListener("click", () => {
    send({
      type: "ai_setup",
      provider: $("ai-provider").value,
      key: $("ai-key").value.trim(),
      model: $("ai-model").value.trim(),
    });
    $("ai-key").value = "";
  });
  $("ai-off").addEventListener("click", () => send({ type: "ai_setup", provider: "off" }));
  $("sheet-toggle").addEventListener("click", () => {
    const t = $("ticket");
    const open = t.classList.toggle("open");
    $("sheet-toggle").setAttribute("aria-expanded", String(open));
  });
  $("pay-wallet").addEventListener("click", () => {
    if (state.paying) return;
    // The server answers with a pay_request for the wallet to sign.
    action("pay", { method: "wallet" });
  });
  $("chat-form").addEventListener("submit", (e) => {
    e.preventDefault();
    const text = $("chat-input").value.trim();
    if (!text) return;
    send({ type: "chat", text });
    $("chat-input").value = "";
  });
  $("new-add").addEventListener("click", () => {
    const name = $("new-name").value.trim();
    const price = $("new-price").value.trim();
    const category = $("new-cat").value.trim() || "other";
    if (!name || !price) return;
    action("menu_upsert", {
      item: { name, price, category, id: "", name_zh: "", description: "", image: "" },
    });
    $("new-name").value = "";
    $("new-price").value = "";
    $("new-cat").value = "";
  });

  // The order sheet rests on the dock, whose height changes with the
  // transcript and quick buttons. Measure it rather than guess.
  const dock = document.querySelector(".chat-dock");
  if (dock && "ResizeObserver" in window) {
    new ResizeObserver(() => {
      document.documentElement.style.setProperty("--dock-h", `${dock.offsetHeight}px`);
    }).observe(dock);
  }

  knockoutMascot();
  connect();
})();
