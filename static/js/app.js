/* Causewaybay Coffee — JSON WebSocket client. Buttons and chat share one send path. */
(() => {
  const $ = (id) => document.getElementById(id);
  const state = {
    ws: null,
    role: null,
    menu: [],
    cart: [],
    total: "0",
    balance: "0",
    payments: [],
    settlement: null,
    paying: false,
  };

  function show(el, on) {
    if (!el) return;
    el.hidden = !on;
    el.classList.toggle("hidden", !on);
  }

  function send(obj) {
    if (state.ws && state.ws.readyState === WebSocket.OPEN) {
      state.ws.send(JSON.stringify(obj));
    }
  }

  function action(name, extra) {
    send(
      Object.assign({ type: "action", name, item_id: "", qty: 1, tx_hash: "" }, extra || {})
    );
  }

  function connect() {
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(`${proto}://${location.host}/ws`);
    state.ws = ws;
    ws.onmessage = (ev) => {
      let msg;
      try {
        msg = JSON.parse(ev.data);
      } catch {
        return;
      }
      onMsg(msg);
    };
    ws.onclose = () => {
      setTimeout(connect, 800);
    };
  }

  function onMsg(msg) {
    switch (msg.type) {
      case "welcome":
        state.role = msg.role;
        state.balance = msg.balance_usdc;
        state.settlement = msg.settlement || null;
        $("role-label").textContent = msg.role;
        $("balance").textContent = msg.balance_usdc;
        show($("stage-door"), false);
        show($("stage-app"), true);
        document.body.classList.toggle("as-owner", msg.role === "owner");
        show($("owner-tools"), msg.role === "owner");
        show($("pay-usdc"), msg.role === "guest");
        renderSettlement();
        if (window.PandaCafe) window.PandaCafe.mood("idle");
        break;
      case "menu":
        state.menu = msg.items || [];
        renderMenu();
        break;
      case "cart":
        state.cart = msg.lines || [];
        state.total = msg.total_usdc;
        state.balance = msg.balance_usdc;
        $("balance").textContent = msg.balance_usdc;
        $("cart-total").textContent = msg.total_usdc;
        renderCart();
        if (window.PandaCafe) window.PandaCafe.mood("ordering");
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
        if (window.PandaCafe) window.PandaCafe.mood("paid");
        addLine(`Paid ${msg.amount_usdc} USDC.`);
        break;
      case "pay_request":
        settleWithWallet(msg);
        break;
      case "error":
        walletBusy(false);
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

  /** Show the wallet door only when the shop is really wired for it. */
  function renderSettlement() {
    const btn = $("pay-wallet");
    const note = $("chain-note");
    const s = state.settlement;
    const guest = state.role === "guest";
    if (!btn || !note) return;
    if (!s || !guest) {
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
      show(note, false);
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
    head.textContent = `Paid ${msg.amount_usdc} USDC · `;
    box.append(head);
    if (msg.explorer_url) {
      const a = document.createElement("a");
      a.href = msg.explorer_url;
      a.target = "_blank";
      a.rel = "noopener noreferrer";
      a.textContent = short(msg.tx_hash);
      a.setAttribute("data-testid", "paid-link");
      box.append(a);
    } else {
      const code = document.createElement("span");
      code.textContent = msg.tx_hash;
      box.append(code);
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
      price.textContent = `${item.price_usdc} USDC`;
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
        if (state.role === "guest") action("add", { item_id: item.id, qty: 1 });
        else action(item.available ? "menu_hide" : "menu_show", { item_id: item.id });
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
      left.textContent = `${line.qty}× ${line.name}`;
      const right = document.createElement("span");
      right.textContent = line.line_usdc;
      li.append(left, right);
      ul.appendChild(li);
    }
  }

  function renderPayments() {
    const box = $("payments-list");
    if (!box) return;
    box.innerHTML = "";
    const h = document.createElement("h3");
    h.textContent = "Payments";
    box.appendChild(h);
    for (const p of state.payments) {
      const d = document.createElement("div");
      d.className = "pay-row";
      d.setAttribute("data-testid", "payment-row");
      d.textContent = `${p.guest} · ${p.amount_usdc} USDC · ${p.method}`;
      box.appendChild(d);
    }
  }

  function renderOrders(orders) {
    const box = $("orders-list");
    box.innerHTML = "";
    for (const o of orders) {
      const d = document.createElement("div");
      d.className = "order-row";
      d.setAttribute("data-testid", "order-row");
      d.textContent = `${o.guest} · ${o.total_usdc} USDC · ${o.status}`;
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

  $("login-guest").addEventListener("click", () => {
    send({ type: "login", role: "guest", name: $("guest-name").value || "guest" });
  });
  $("login-owner").addEventListener("click", () => {
    send({ type: "login", role: "owner", name: "owner", pin: $("owner-pin").value || "" });
  });
  $("pay-usdc").addEventListener("click", () => action("pay", { method: "usdc" }));
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
  });

  knockoutMascot();
  connect();
  if (window.PandaCafe) window.PandaCafe.start($("cafe-3d"));
})();
