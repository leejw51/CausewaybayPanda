/* How the page reaches the cafe. Two ways, one shape.

   With a server on the other end, frames go over a WebSocket. With none —
   the page opened from a file, from GitHub Pages, from anywhere with no
   panda behind it — the same frames go to the cafe engine compiled from the
   same Rust into WebAssembly, running right here in the tab. The page cannot
   tell which, and does not need to. */
(function (global) {
  const SNAPSHOT_KEY = "causewaybay.shop";

  /** A socket to a real server. */
  class SocketTransport {
    constructor(onMessage) {
      this.local = false;
      this.onMessage = onMessage;
      this.ws = null;
      this.onOpen = () => {};
      this.connect();
    }
    connect() {
      const proto = location.protocol === "https:" ? "wss" : "ws";
      const ws = new WebSocket(`${proto}://${location.host}/ws`);
      this.ws = ws;
      ws.onopen = () => this.onOpen();
      ws.onmessage = (ev) => {
        let msg;
        try {
          msg = JSON.parse(ev.data);
        } catch {
          return;
        }
        this.onMessage(msg);
      };
      ws.onclose = () => setTimeout(() => this.connect(), 800);
    }
    send(obj) {
      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        this.ws.send(JSON.stringify(obj));
      }
    }
  }

  /** The cafe engine in this tab. */
  class LocalTransport {
    constructor(onMessage, engine, conn) {
      this.local = true;
      this.onMessage = onMessage;
      this.engine = engine;
      this.conn = conn;
      this.timer = null;
      this.onOpen = () => {};
      this.tickMs = Number(new URLSearchParams(location.search).get("tick")) || 2500;
      // The clock is the browser's; the engine has none of its own.
      this.clock();
      setInterval(() => this.clock(), 30_000);
      // A socket "opens" a moment after construction; so does this.
      setTimeout(() => this.onOpen(), 0);
    }
    clock() {
      const d = new Date();
      const today = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(
        d.getDate()
      ).padStart(2, "0")}`;
      this.engine.set_clock(Date.now(), today);
    }
    deliver(json) {
      let frames;
      try {
        frames = JSON.parse(json);
      } catch {
        return;
      }
      for (const f of frames) {
        if (f.conn === this.conn) {
          if (f.msg && f.msg.type === "welcome") this.role = f.msg.role;
          this.onMessage(f.msg);
        }
        // Either switch wants a beat; the engine says whether any is on.
        if (f.msg && (f.msg.type === "auto" || f.msg.type === "kitchen")) {
          this.autoTimer(this.engine.ticking_wanted());
        }
      }
      this.persist();
    }
    send(obj) {
      // A line the local parser cannot read goes to the model the owner chose,
      // if any — from this tab, with this tab's key. Everything else is
      // answered on the spot.
      if (obj.type === "chat" && this.mod && !this.engine.parses(obj.text)) {
        const cfg = this.engine.ai_config_json(this.conn);
        if (cfg) {
          this.askModel(obj, JSON.parse(cfg));
          return;
        }
      }
      this.deliver(this.engine.handle(this.conn, JSON.stringify(obj)));
    }
    async askModel(obj, cfg) {
      this.onMessage({ type: "assistant", text: "Asking the model…", buttons: [] });
      let intent = "";
      try {
        intent = await this.mod.ask_ai(
          cfg.provider,
          cfg.key,
          cfg.model,
          obj.text,
          JSON.stringify(cfg.board),
          this.role || "guest",
          JSON.stringify(cfg.facts || [])
        );
      } catch {
        intent = "";
      }
      if (intent) {
        this.deliver(this.engine.apply_intent(this.conn, intent));
      } else {
        // The model had nothing better: the parser's own reading stands.
        this.deliver(this.engine.handle(this.conn, JSON.stringify(obj)));
      }
    }
    /** A beat every few seconds while the cafe runs itself or the panda
        works the kitchen. */
    autoTimer(on) {
      if (on && !this.timer) {
        this.timer = setInterval(() => this.deliver(this.engine.tick()), this.tickMs);
      } else if (!on && this.timer) {
        clearInterval(this.timer);
        this.timer = null;
      }
    }
    persist() {
      try {
        localStorage.setItem(SNAPSHOT_KEY, this.engine.snapshot());
      } catch {
        /* private mode: the shop lives for this tab only */
      }
    }
  }

  /** Is there a panda behind this page? Decide within a second. Returns its
      /health when there is, so the door can say what kind of shop it is. */
  async function serverPresent() {
    if (location.protocol === "file:") return null;
    const q = new URLSearchParams(location.search);
    if (q.has("local")) return null;
    try {
      const ctl = new AbortController();
      const t = setTimeout(() => ctl.abort(), 1200);
      const r = await fetch("/health", { signal: ctl.signal, cache: "no-store" });
      clearTimeout(t);
      if (!r.ok) return null;
      const j = await r.json();
      return j && j.ok === true ? j : null;
    } catch {
      return null;
    }
  }

  /** Build the right transport and hand it back. */
  async function open(onMessage) {
    const health = await serverPresent();
    if (health) {
      const t = new SocketTransport(onMessage);
      t.mode = health.mode || "simulation";
      t.cafe = { name: health.cafe || "", name_zh: health.cafe_zh || "" };
      return t;
    }

    // Beside the page: /pkg on a server or a static host, ./pkg from a file.
    const base = location.protocol === "file:" ? "./pkg/" : "/pkg/";
    const mod = await import(`${base}causewaybay_panda_web.js`);
    await mod.default({ module_or_path: `${base}causewaybay_panda_web_bg.wasm` });
    let snapshot = null;
    try {
      snapshot = localStorage.getItem(SNAPSHOT_KEY);
    } catch {
      /* fine */
    }
    // A tab is always a simulation: there is no till to lock, so no pin.
    const q = new URLSearchParams(location.search);
    const engine = new mod.Engine(snapshot, "panda", q.get("denom") || null, Date.now() % 2 ** 32);
    const conn = engine.connect();
    const t = new LocalTransport(onMessage, engine, conn);
    t.mode = "simulation";
    t.mod = mod;
    try {
      t.cafe = JSON.parse(engine.cafe_json());
    } catch {
      t.cafe = null;
    }
    // The cafe may have been left running, or the panda at the pass.
    if (engine.ticking_wanted()) t.autoTimer(true);
    return t;
  }

  global.PandaTransport = { open };
})(window);
