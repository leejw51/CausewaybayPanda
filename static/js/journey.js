/* The guest's journey: four plates for the four stages of an order, a bar
   that creeps along between the kitchen's real updates, and a burst of
   particles when the sign comes on and when the tray lands.

   The kitchen only ever says "placed", "preparing", "ready", "collected".
   Between those words the bar moves on its own, at the pace a real bar
   works — a simulation of progress, labelled as an estimate, so a guest
   watching their phone sees something happen while they wait. The moment a
   real update arrives the bar snaps to it. */
(function (global) {
  const STAGES = [
    { key: "placed", label: "Received", plate: "received", estimate_ms: 20_000 },
    { key: "preparing", label: "Being made", plate: "making", estimate_ms: 60_000 },
    { key: "ready", label: "On its way", plate: "ready", estimate_ms: 0 },
    { key: "collected", label: "Served", plate: "delivered", estimate_ms: 0 },
  ];
  const BASE = location.protocol === "file:" ? "./assets/journey/" : "/assets/journey/";

  /** Do not animate for a person who asked not to be animated at. */
  function stillness() {
    return matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  /* Plates arrive on flat magenta. Knock it out once per plate, the way the
     mascot on the door is, and hand out the result as a data URL so every
     card shares one decode. */
  const plates = new Map();
  function plate(name) {
    if (plates.has(name)) return plates.get(name);
    const p = new Promise((resolve) => {
      const img = new Image();
      img.onload = () => {
        try {
          resolve(knockout(img, 160));
        } catch {
          resolve(img.src);
        }
      };
      img.onerror = () => resolve("");
      img.src = `${BASE}${name}.png`;
    });
    plates.set(name, p);
    return p;
  }

  function knockout(img, size) {
    const canvas = document.createElement("canvas");
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    ctx.drawImage(img, 0, 0, size, size);
    const data = ctx.getImageData(0, 0, size, size);
    const px = data.data;
    const w = size;
    const h = size;
    const bg = [px[0], px[1], px[2]];
    const dist2 = (i) => {
      const dr = px[i] - bg[0];
      const dg = px[i + 1] - bg[1];
      const db = px[i + 2] - bg[2];
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
    return canvas.toDataURL("image/png");
  }

  /** Index of a status along the journey; -1 for one that left it. */
  function stageIndex(status) {
    return STAGES.findIndex((s) => s.key === status);
  }

  /**
   * Build the strip for one order: four plates, the one it is at lit, and a
   * bar underneath. `since_ms` is when the order reached its current stage,
   * so a reload part-way through a wait does not start the creep again from
   * nothing.
   */
  function strip(status, since_ms) {
    const at = stageIndex(status);
    const box = document.createElement("div");
    box.className = "journey";
    box.setAttribute("data-testid", "journey");
    box.setAttribute("data-stage", status);

    const steps = document.createElement("ol");
    steps.className = "journey-steps";
    STAGES.forEach((s, i) => {
      const li = document.createElement("li");
      li.className = "journey-step" + (i < at ? " done" : i === at ? " active" : "");
      li.setAttribute("data-stage", s.key);
      const img = document.createElement("img");
      img.alt = "";
      img.className = "journey-plate";
      plate(s.plate).then((src) => {
        if (src) img.src = src;
      });
      const label = document.createElement("span");
      label.textContent = s.label;
      li.append(img, label);
      steps.appendChild(li);
    });

    const track = document.createElement("div");
    track.className = "journey-track";
    const fill = document.createElement("div");
    fill.className = "journey-fill";
    fill.setAttribute("data-testid", "journey-fill");
    // Each stage owns a quarter of the bar. Done stages are full; the
    // active one creeps from its start towards its end at the estimated
    // pace, never quite arriving — only the kitchen can say it is done.
    const from = Math.max(0, at) / STAGES.length;
    const to = (Math.max(0, at) + 0.92) / STAGES.length;
    const est = at >= 0 ? STAGES[at].estimate_ms : 0;
    if (at >= 2) {
      // Ready and delivered: the bar is where it is, no guessing left.
      fill.style.width = `${((at + 1) / STAGES.length) * 100}%`;
    } else if (est > 0 && !stillness()) {
      const elapsed = Math.max(0, Date.now() - (since_ms || Date.now()));
      const delay = -Math.min(elapsed, est);
      fill.style.setProperty("--from", `${from * 100}%`);
      fill.style.setProperty("--to", `${to * 100}%`);
      fill.style.animation = `journey-creep ${est}ms cubic-bezier(0.2, 0.7, 0.4, 1) ${delay}ms 1 forwards`;
      fill.classList.add("creeping");
    } else {
      fill.style.width = `${from * 100}%`;
    }
    track.appendChild(fill);

    const note = document.createElement("small");
    note.className = "journey-note";
    note.setAttribute("data-testid", "journey-note");
    note.textContent =
      at === 0
        ? "Waiting for the kitchen to pick it up · estimate"
        : at === 1
          ? "On the bar now · estimate"
          : at === 2
            ? "The panda is bringing it over"
            : at === 3
              ? "Enjoy"
              : "";

    box.append(steps, track, note);
    return box;
  }

  /* Particles. Brass coins, cream hearts, cyan sparkles, magenta stars — the
     shop's own palette, drawn rather than loaded, over the card that earned
     them. One canvas per burst, gone when the last piece has fallen. */
  const COLOURS = {
    coin: "#c4a35a",
    heart: "#f3e6d4",
    spark: "#5ce1e6",
    star: "#ff3d9a",
  };

  function burst(host, count, big) {
    if (!host || stillness()) return null;
    const rect = host.getBoundingClientRect();
    if (!rect.width || !rect.height) return null;
    const canvas = document.createElement("canvas");
    canvas.className = "burst";
    canvas.setAttribute("data-testid", "burst");
    const dpr = Math.min(2, global.devicePixelRatio || 1);
    const pad = 40;
    canvas.width = (rect.width + pad * 2) * dpr;
    canvas.height = (rect.height + pad * 2) * dpr;
    canvas.style.width = `${rect.width + pad * 2}px`;
    canvas.style.height = `${rect.height + pad * 2}px`;
    canvas.style.left = `${-pad}px`;
    canvas.style.top = `${-pad}px`;
    host.appendChild(canvas);
    const ctx = canvas.getContext("2d");
    ctx.scale(dpr, dpr);

    const kinds = Object.keys(COLOURS);
    const cx = rect.width / 2 + pad;
    const cy = rect.height / 2 + pad;
    const bits = [];
    for (let i = 0; i < count; i++) {
      const a = Math.random() * Math.PI * 2;
      const speed = (big ? 5 : 3.2) + Math.random() * (big ? 6 : 3.5);
      bits.push({
        kind: kinds[i % kinds.length],
        x: cx + (Math.random() - 0.5) * rect.width * 0.6,
        y: cy + (Math.random() - 0.5) * rect.height * 0.4,
        vx: Math.cos(a) * speed,
        vy: Math.sin(a) * speed - (big ? 4 : 2.5),
        r: (big ? 4 : 3) + Math.random() * 3,
        spin: (Math.random() - 0.5) * 0.3,
        rot: Math.random() * Math.PI * 2,
        life: 1,
      });
    }
    const gravity = 0.22;
    const drag = 0.985;
    const started = performance.now();
    const span = big ? 1800 : 1200;

    function draw(bit) {
      ctx.save();
      ctx.translate(bit.x, bit.y);
      ctx.rotate(bit.rot);
      ctx.globalAlpha = Math.max(0, Math.min(1, bit.life));
      ctx.fillStyle = COLOURS[bit.kind];
      const r = bit.r;
      switch (bit.kind) {
        case "coin":
          ctx.beginPath();
          ctx.ellipse(0, 0, r, r * 0.65, 0, 0, Math.PI * 2);
          ctx.fill();
          break;
        case "heart":
          ctx.beginPath();
          ctx.moveTo(0, r);
          ctx.bezierCurveTo(-r * 1.4, -r * 0.2, -r * 0.6, -r * 1.2, 0, -r * 0.4);
          ctx.bezierCurveTo(r * 0.6, -r * 1.2, r * 1.4, -r * 0.2, 0, r);
          ctx.fill();
          break;
        case "spark":
          ctx.beginPath();
          for (let k = 0; k < 4; k++) {
            const t = (k / 4) * Math.PI * 2;
            ctx.lineTo(Math.cos(t) * r * 1.4, Math.sin(t) * r * 1.4);
            ctx.lineTo(Math.cos(t + Math.PI / 4) * r * 0.4, Math.sin(t + Math.PI / 4) * r * 0.4);
          }
          ctx.closePath();
          ctx.fill();
          break;
        default:
          ctx.beginPath();
          for (let k = 0; k < 5; k++) {
            const t = (k / 5) * Math.PI * 2 - Math.PI / 2;
            ctx.lineTo(Math.cos(t) * r * 1.3, Math.sin(t) * r * 1.3);
            const u = t + Math.PI / 5;
            ctx.lineTo(Math.cos(u) * r * 0.55, Math.sin(u) * r * 0.55);
          }
          ctx.closePath();
          ctx.fill();
      }
      ctx.restore();
    }

    function frame(now) {
      const t = (now - started) / span;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      for (const b of bits) {
        b.vy += gravity;
        b.vx *= drag;
        b.vy *= drag;
        b.x += b.vx;
        b.y += b.vy;
        b.rot += b.spin;
        b.life = 1 - Math.max(0, t - 0.55) / 0.45;
        draw(b);
      }
      if (t < 1 && canvas.isConnected) {
        requestAnimationFrame(frame);
      } else {
        canvas.remove();
      }
    }
    requestAnimationFrame(frame);
    return canvas;
  }

  global.PandaJourney = { STAGES, stageIndex, strip, burst, plate, stillness };
})(window);
