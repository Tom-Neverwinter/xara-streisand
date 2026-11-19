"use strict";

const $ = (id) => document.getElementById(id);
const FLOOR = -60;
let meterFloor = -Infinity;

for (const el of document.querySelectorAll(".scale")) {
  el.replaceChildren(...[0, 1, 2, 3, 4].map((i) => {
    const s = document.createElement("span");
    const db = FLOOR * (4 - i) / 4;
    s.textContent = db ? `\u2212${-db}` : "0 dBFS";
    return s;
  }));
}

const pref = {
  get(k, d) {
    try {
      const v = localStorage.getItem(k);
      return v === null ? d : v;
    } catch (e) {
      return d;
    }
  },
  set(k, v) {
    try { localStorage.setItem(k, v); } catch (e) {}
  },
};

// Display themes besides "system" (follow the OS). A tiny inline script in <head>
// applies the saved one before paint; this fills the picker and keeps it in sync.
// The third field names the display hardware the readouts are redrawn as (see "Display hardware" below).
const THEMES = [
  ["airport", "Airport departures board", "flap"], ["nixie", "Nixie tubes", "nixie"], ["vfd", "Car dash VFD", "vfd"],
  ["sodium", "Sodium lamp"], ["deco", "Art deco"], ["phosphor", "Green phosphor"], ["radar", "Radar scope"],
  ["synthwave", "Synthwave"], ["paper", "Paper log"], ["contrast", "High contrast"],
];
function applyTheme(v) {
  const root = document.documentElement;
  if (v === "system") delete root.dataset.theme;
  else root.dataset.theme = v;
  setDisplay(THEMES.find((t) => t[0] === v)?.[2] ?? null);
}
function initTheme() {
  // ?theme=nixie in the address picks (and keeps) a theme, for a wall display or a kiosk.
  const asked = new URLSearchParams(location.search).get("theme");
  if (asked && (asked === "system" || THEMES.some((t) => t[0] === asked))) pref.set("xara.theme", asked);
  const saved = pref.get("xara.theme", "system");
  applyTheme(THEMES.some((t) => t[0] === saved) ? saved : "system");
  const sel = $("theme");
  if (!sel) return;
  for (const [v, name] of THEMES) sel.add(new Option(name, v));
  sel.value = document.documentElement.dataset.theme ?? "system";
  sel.addEventListener("change", () => {
    applyTheme(sel.value);
    pref.set("xara.theme", sel.value);
    window.dispatchEvent(new Event("themechange"));
  });
}

function setConn(text, cls) {
  const c = $("conn");
  c.textContent = text;
  c.className = "conn " + cls;
}

function connection(on) {
  let ws = null, backoff = 1000, lastStatus = 0;
  const conn = {
    send(cmd) {
      if (ws && ws.readyState === WebSocket.OPEN) ws.send(cmd);
    },
    get open() {
      return !!ws && ws.readyState === WebSocket.OPEN;
    },
    alive() {
      lastStatus = performance.now();
      setConn("live", "up");
    },
  };
  function connect() {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    ws = new WebSocket(`${proto}//${location.host}/ws`);
    ws.binaryType = "arraybuffer";
    ws.onopen = () => {
      backoff = 1000;
      on.open();
    };
    ws.onmessage = (m) => {
      if (typeof m.data !== "string") {
        on.binary(new Uint8Array(m.data));
        return;
      }
      let msg;
      try { msg = JSON.parse(m.data); } catch (e) { return; }
      on.message(msg);
    };
    ws.onclose = () => {
      setConn("offline", "");
      setTimeout(connect, backoff);
      backoff = Math.min(backoff * 2, 10000);
    };
  }
  setInterval(() => {
    if (conn.open && performance.now() - lastStatus > 2000) setConn("stale", "stale");
  }, 1000);
  connect();
  return conn;
}

function waterfallPage(key, message) {
  let wf = null;
  const conn = connection({
    open: () => wf.subscribe(),
    binary: (b) => wf.row(b),
    message,
  });
  wf = new Waterfall(conn, key);
  return { conn, wf };
}

function meter(id) {
  const el = $(id);
  const hold = { db: FLOOR, at: 0 };
  const pct = (db) => Math.max(0, Math.min(100, (db - FLOOR) / -FLOOR * 100));
  return (m) => {
    el.querySelector(".fill").style.clipPath = `inset(0 ${100 - pct(m.rms)}% 0 0)`;
    const now = performance.now();
    if (m.peak >= hold.db || now - hold.at > 1500) {
      hold.db = m.peak;
      hold.at = now;
    }
    el.querySelector(".hold").style.left = `calc(${pct(hold.db)}% - 2px)`;
    el.querySelector(".val").textContent = m.rms <= meterFloor ? "-" : `${m.rms.toFixed(1)} dB`;
  };
}

function palette(stops) {
  const lut = new Uint8ClampedArray(256 * 3);
  for (let i = 0; i < 256; i++) {
    const x = i / 255;
    let k = 1;
    while (stops[k][0] < x) k++;
    const [x0, ...a] = stops[k - 1], [x1, ...b] = stops[k];
    const f = (x - x0) / (x1 - x0);
    for (let c = 0; c < 3; c++) lut[i * 3 + c] = a[c] + (b[c] - a[c]) * f;
  }
  return lut;
}

const PALETTE = palette([[0, 0, 0, 0], [0.22, 0, 0, 120], [0.45, 0, 120, 220], [0.62, 0, 220, 200],
  [0.75, 240, 230, 0], [0.88, 255, 70, 0], [1, 255, 255, 255]]);

class Waterfall {
  constructor(conn, key) {
    this.conn = conn;
    this.key = key;
    this.span = null;
    this.rowType = null;
    this.step = 0.5;
    this.noise = null;
    this.passband = null;
    this.ptt = false;
    this.img = null;
    this.clock = 0;
    this.stamps = [];
    this.on = pref.get(key, "on") !== "off";
    this.canvas = $("wf");
    $("wf-on").checked = this.on;
    $("wf-on").addEventListener("change", (e) => {
      this.on = e.target.checked;
      pref.set(key, this.on ? "on" : "off");
      this.subscribe();
    });
    document.addEventListener("visibilitychange", () => this.subscribe());
    $("wf-wrap").addEventListener("mousemove", (e) => {
      if (!this.span) return;
      const r = e.currentTarget.getBoundingClientRect();
      $("wf-cursor").textContent = `${Math.round((e.clientX - r.left) / r.width * this.span)} Hz`;
    });
    $("wf-wrap").addEventListener("mouseleave", () => { $("wf-cursor").textContent = ""; });
    window.addEventListener("resize", () => this.drawAxis());
  }

  subscribe() {
    document.querySelector(".wf").classList.toggle("off", !this.on);
    this.conn.send(this.on && !document.hidden ? "WATERFALL ON" : "WATERFALL OFF");
  }

  reset(span) {
    this.noise = null;
    this.stamps = [];
    this.span = span;
    this.drawAxis();
  }

  config(m) {
    this.rowType = m.row;
    this.span = m.span;
    this.step = m.step;
    this.canvas.width = m.bins;
    this.img = null;
    this.drawAxis();
  }

  setPassband(lo, hi) {
    const p = this.passband;
    if (lo === null ? p === null : p && p[0] === lo && p[1] === hi) return;
    this.passband = lo === null ? null : [lo, hi];
    this.drawAxis();
  }

  row(b) {
    if (b[0] !== this.rowType || !this.on) return;
    const n = b[2] | (b[3] << 8);
    const v = b.subarray(4, 4 + n);
    const p20 = Uint8Array.from(v).sort()[Math.floor(n * 0.2)];
    this.noise = this.noise === null ? p20 : this.noise * 0.95 + p20 * 0.05;
    const lo = this.noise - 6 / this.step, range = 50 / this.step;
    const c = this.canvas;
    const g = c.getContext("2d");
    if (c.width !== n) c.width = n;
    if (!this.img || this.img.width !== n) this.img = g.createImageData(n, 1);
    g.drawImage(c, 0, 0, n, c.height - 1, 0, 1, n, c.height - 1);
    const d = this.img.data;
    for (let i = 0; i < n; i++) {
      const k = Math.max(0, Math.min(255, Math.round((v[i] - lo) / range * 255))) * 3;
      d[i * 4] = PALETTE[k];
      d[i * 4 + 1] = PALETTE[k + 1];
      d[i * 4 + 2] = PALETTE[k + 2];
      d[i * 4 + 3] = 255;
    }
    if (this.ptt) {
      for (let i = 0; i < 4; i++) d.set([255, 40, 40, 255], i * 4);
    }
    g.putImageData(this.img, 0, 0);
    this.stamps.unshift(this.clock);
    if (this.stamps.length > c.height) this.stamps.length = c.height;
  }

  mark(ts, loHz, hiHz, color) {
    const y = this.stamps.findIndex((s) => s <= ts);
    if (y < 0 || !this.on) return false;
    const c = this.canvas, g = c.getContext("2d");
    const x0 = Math.max(0, Math.round(loHz / this.span * c.width));
    const x1 = Math.min(c.width, Math.round(hiHz / this.span * c.width));
    g.fillStyle = color;
    g.fillRect(x0, y, Math.max(1, x1 - x0), 2);
    return true;
  }

  drawAxis() {
    if (!this.span) return;
    const c = $("wf-axis");
    const dpr = window.devicePixelRatio || 1;
    const w = c.clientWidth, h = c.clientHeight;
    c.width = Math.round(w * dpr);
    c.height = Math.round(h * dpr);
    const g = c.getContext("2d");
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    const css = getComputedStyle(document.documentElement);
    const band = $("wf-band");
    if (this.passband) {
      const [lo, hi] = this.passband;
      band.style.display = "block";
      band.style.left = `${lo / this.span * 100}%`;
      band.style.width = `${(hi - lo) / this.span * 100}%`;
      g.fillStyle = css.getPropertyValue("--band");
      g.globalAlpha = 0.35;
      g.fillRect(lo / this.span * w, 0, (hi - lo) / this.span * w, 4);
      g.globalAlpha = 1;
    } else {
      band.style.display = "none";
    }
    const step = this.span > 4000 ? 1000 : 500;
    g.font = "11px system-ui, sans-serif";
    g.fillStyle = css.getPropertyValue("--dim");
    g.strokeStyle = css.getPropertyValue("--dim");
    g.textBaseline = "top";
    for (let f = 0; f <= this.span; f += step / 2) {
      const x = Math.round(f / this.span * (w - 1)) + 0.5;
      const major = f % step === 0;
      g.beginPath();
      g.moveTo(x, 0);
      g.lineTo(x, major ? 7 : 4);
      g.stroke();
      if (major) {
        g.textAlign = f === 0 ? "left" : f >= this.span ? "right" : "center";
        g.fillText(String(f), x, 9);
      }
    }
  }
}

class LogView {
  constructor(el, keep, show) {
    this.el = el;
    this.keep = keep;
    this.show = show;
    this.entries = [];
    this.offset = null;
    this.filter = () => true;
  }

  clear() {
    this.entries = [];
    this.render();
  }

  reset() {
    this.offset = null;
    this.clear();
  }

  setOffset(offset) {
    const first = this.offset === null;
    this.offset = offset;
    if (first) this.render();
  }

  clock(ts) {
    if (this.offset === null) return "";
    return new Date((ts + this.offset) * 1000).toLocaleTimeString([], { hour12: false });
  }

  row(e) {
    const li = document.createElement("li");
    li.className = e.cls;
    const t = document.createElement("time");
    t.textContent = this.clock(e.ts);
    const s = document.createElement("span");
    s.textContent = e.text;
    li.append(t, s);
    return li;
  }

  add(e) {
    this.entries.push(e);
    if (this.entries.length > this.keep) this.entries.splice(0, this.entries.length - this.keep);
    if (this.offset !== null && this.filter(e)) {
      const log = this.el;
      const stick = log.scrollTop + log.clientHeight >= log.scrollHeight - 4;
      log.appendChild(this.row(e));
      while (log.childElementCount > this.show) log.firstElementChild.remove();
      if (stick) log.scrollTop = log.scrollHeight;
    }
  }

  addOrCollapse(e) {
    const last = this.entries[this.entries.length - 1];
    if (!e.key || !last || last.key !== e.key) {
      this.add(e);
      return;
    }
    last.base ??= last.text;
    last.count = (last.count ?? 1) + 1;
    last.ts = e.ts;
    last.text = `${last.base}  ×${last.count}`;
    if (this.offset !== null && this.filter(last) && this.el.lastElementChild) {
      this.el.lastElementChild.replaceWith(this.row(last));
    }
  }

  render() {
    const log = this.el;
    log.replaceChildren();
    if (this.offset === null) return;
    const shown = this.entries.filter(this.filter).slice(-this.show);
    log.append(...shown.map((e) => this.row(e)));
    log.scrollTop = log.scrollHeight;
  }
}

function lineChart(c, points, now, span, lo, hi) {
  const dpr = window.devicePixelRatio || 1;
  const w = c.clientWidth, h = c.clientHeight;
  if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) {
    c.width = Math.round(w * dpr);
    c.height = Math.round(h * dpr);
  }
  const g = c.getContext("2d");
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, w, h);
  const css = getComputedStyle(document.documentElement);
  const dim = css.getPropertyValue("--dim"), line = css.getPropertyValue("--line"), accent = css.getPropertyValue("--accent");
  lo = Math.min(lo, ...points.map((p) => Math.floor(p.v / 10) * 10));
  hi = Math.max(hi, ...points.map((p) => Math.ceil(p.v / 10) * 10));
  const left = 30, bottom = h - 4;
  const x = (ts) => left + (w - left - 4) * (1 - (now - ts) / span);
  const y = (v) => bottom - (bottom - 6) * (v - lo) / (hi - lo);
  g.font = "11px system-ui, sans-serif";
  g.textBaseline = "middle";
  for (let v = lo; v <= hi; v += 10) {
    g.strokeStyle = line;
    g.beginPath();
    g.moveTo(left, y(v));
    g.lineTo(w, y(v));
    g.stroke();
    g.fillStyle = dim;
    g.fillText(String(v), 2, y(v));
  }
  if (!points.length) return;
  g.strokeStyle = accent;
  g.fillStyle = accent;
  g.lineWidth = 1.5;
  g.beginPath();
  points.forEach((p, i) => (i ? g.lineTo(x(p.ts), y(p.v)) : g.moveTo(x(p.ts), y(p.v))));
  g.stroke();
  for (const p of points) {
    g.beginPath();
    g.arc(x(p.ts), y(p.v), 2.5, 0, 2 * Math.PI);
    g.fill();
  }
}

// ---- Display hardware --------------------------------------------------------------------------------------
// Three themes redraw every readout as hardware: an airport split-flap board ("flap"), nixie tubes ("nixie") and
// a car-dash vacuum fluorescent display ("vfd"). The page keeps writing plain text into its readouts; each one is
// watched, visually hidden by common.css (screen readers still read it) and mirrored into a sibling the theme draws.

const READOUTS = ".tile > b, .state, .meter .val, #drive-val, #clock";
const display = { kind: null, items: [] };
const reduceMotion = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

class Readout {
  constructor(src) {
    this.src = src;
    this.out = document.createElement("span");
    this.out.className = `disp ${display.kind}`;
    this.out.setAttribute("aria-hidden", "true");
    src.after(this.out);
    this.text = null;
    this.color = null;
    this.w = 0;
    this.mo = new MutationObserver(() => this.update());
    this.mo.observe(src, { childList: true, characterData: true, subtree: true, attributes: true, attributeFilter: ["class"] });
    this.update();
  }

  remove() {
    this.mo.disconnect();
    this.out.remove();
  }

  update(force) {
    const text = (this.src.textContent.trim() || "-").replace(/−/g, "-");
    const color = getComputedStyle(this.src).color;
    if (!force && text === this.text && color === this.color) return;
    this.text = text;
    this.color = color;
    this.out.style.color = color;
    RENDER[display.kind](this, text);
  }
}

function setDisplay(kind) {
  if (kind === display.kind) return;
  for (const r of display.items) r.remove();
  display.items = [];
  flap.busy.clear();
  display.kind = kind;
  const root = document.documentElement;
  if (!kind) {
    delete root.dataset.display;
    return;
  }
  root.dataset.display = kind;
  display.items = [...document.querySelectorAll(READOUTS)].map((el) => new Readout(el));
}

const RENDER = {};

// Split-flap: every character cell is a Solari flap unit, two fixed half-cards and two leaves that fall over the
// hinge. A cell steps through the drum in order, like the real thing, so a change ripples across the board.
const flap = { drum: " ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.,:-/%+()'", busy: new Set(), timer: 0, step: 100, most: 9 };

function flapCell() {
  const el = document.createElement("span");
  el.className = "flap";
  const [t, b, lt, lb] = ["t", "b", "lt", "lb"].map((k) => {
    const h = document.createElement("span");
    h.className = `h ${k}`;
    const i = document.createElement("i");
    h.append(i);
    el.append(h);
    return i;
  });
  const c = { el, t, b, lt, lb, cur: " ", target: " ", go: false };
  t.textContent = b.textContent = lt.textContent = lb.textContent = " ";
  return c;
}

RENDER.flap = (r, text) => {
  text = text.toUpperCase();
  r.w = Math.max(r.w, text.length);
  r.cells ??= [];
  while (r.cells.length < r.w) {
    const c = flapCell();
    r.cells.push(c);
    r.out.append(c.el);
  }
  const padded = text.padEnd(r.cells.length);
  r.cells.forEach((c, i) => {
    c.target = padded[i];
    if (reduceMotion()) {
      c.cur = c.t.textContent = c.b.textContent = c.target;
    } else if (c.cur !== c.target) {
      flap.busy.add(c);
    }
  });
  if (flap.busy.size && !flap.timer) flap.timer = setInterval(flapTick, flap.step);
};

function flapTick() {
  const d = flap.drum, n = d.length;
  for (const c of flap.busy) {
    if (c.go) {
      c.b.textContent = c.cur;
      c.el.classList.remove("go");
      c.go = false;
    }
    if (c.cur === c.target) {
      flap.busy.delete(c);
      continue;
    }
    let i = d.indexOf(c.cur);
    const j = d.indexOf(c.target);
    let next = c.target;
    if (i >= 0 && j >= 0) {
      // A long way round the drum is skipped to the last few cards, so a readout never lags by seconds.
      if ((j - i + n) % n > flap.most) i = (j - flap.most + n) % n;
      next = d[(i + 1) % n];
    }
    c.lt.textContent = c.b.textContent = c.cur;
    c.t.textContent = c.lb.textContent = next;
    void c.el.offsetWidth;
    c.el.classList.add("go");
    c.go = true;
    c.cur = next;
  }
  if (!flap.busy.size) {
    clearInterval(flap.timer);
    flap.timer = 0;
  }
}

// Nixie: digits are IN-14 style tubes with the whole cathode stack visible behind the lit one; letters get a lit
// glyph of their own, "." lights the previous tube's decimal point and ":" is a pair of INS-1 neon dots.
const NIXIE_STACK = "3894057261"; // back to front, as the cathodes sit in the tube

function nixieUnit(u) {
  const el = document.createElement("span");
  if (u.k === "neon") {
    el.className = "neon";
    el.append(document.createElement("i"), document.createElement("i"));
    return el;
  }
  el.className = u.k === "alpha" ? "tube alpha" : "tube";
  for (const ch of u.k === "alpha" ? [u.ch] : NIXIE_STACK) {
    const b = document.createElement("b");
    b.textContent = ch;
    el.append(b);
  }
  const dp = document.createElement("i");
  dp.className = "dp";
  el.append(dp);
  return el;
}

RENDER.nixie = (r, text) => {
  const units = [];
  for (const ch of text) {
    const last = units[units.length - 1];
    if (ch === "." || ch === ",") {
      if (last && last.k !== "neon" && !last.dp) last.dp = true;
      else units.push({ k: "tube", ch: " ", dp: true });
    } else if (ch === ":") {
      units.push({ k: "neon" });
    } else {
      units.push({ k: /[0-9 ]/.test(ch) ? "tube" : "alpha", ch: ch.toUpperCase(), dp: false });
    }
  }
  // Numbers sit right-aligned in a fixed row of tubes; unused tubes stay dark, as on a real counter.
  r.w = Math.max(r.w, units.length);
  while (units.length < r.w) units.unshift({ k: "tube", ch: " ", dp: false });
  const key = units.map((u) => (u.k === "alpha" ? u.ch : u.k)).join();
  if (key !== r.key) {
    r.key = key;
    r.out.replaceChildren(...units.map(nixieUnit));
  }
  units.forEach((u, i) => {
    const el = r.out.children[i];
    if (u.k === "neon") return;
    for (const b of el.querySelectorAll("b")) b.classList.toggle("on", u.k === "alpha" || b.textContent === u.ch);
    el.querySelector(".dp").classList.toggle("on", u.dp);
  });
};

// VFD: numbers on seven-segment digits, everything else on a 5x7 dot matrix; unlit segments and dots stay
// faintly visible, and CSS draws the filament wires across the glass. Font: 7 rows of 5 bits, top row first.
const MATRIX = {
  "0": "0e11131519110e", "1": "040c040404040e", "2": "0e11010204081f", "3": "1f02040201110e",
  "4": "02060a121f0202", "5": "1f101e0101110e", "6": "0608101e11110e", "7": "1f010204080808",
  "8": "0e11110e11110e", "9": "0e11110f01020c", "A": "0e1111111f1111", "B": "1e11111e11111e",
  "C": "0e11101010110e", "D": "1c12111111121c", "E": "1f10101e10101f", "F": "1f10101e101010",
  "G": "0e11101711110f", "H": "1111111f111111", "I": "0e04040404040e", "J": "0702020202120c",
  "K": "11121418141211", "L": "1010101010101f", "M": "111b1515111111", "N": "11111915131111",
  "O": "0e11111111110e", "P": "1e11111e101010", "Q": "0e11111115120d", "R": "1e11111e141211",
  "S": "0f10100e01011e", "T": "1f040404040404", "U": "1111111111110e", "V": "11111111110a04",
  "W": "1111111515150a", "X": "11110a040a1111", "Y": "1111110a040404", "Z": "1f01020408101f",
  " ": "00000000000000", ".": "00000000000c0c", ",": "000000000c0408", ":": "000c0c000c0c00",
  "-": "0000001f000000", "/": "00010204081000", "%": "18190204081303", "+": "0004041f040400",
  "(": "02040808080402", ")": "08040202020408", ">": "08040201020408", "<": "02040810080402",
  "!": "04040404000004", "?": "0e110102040004", "#": "0a0a1f0a1f0a0a", "=": "00001f001f0000",
  "_": "0000000000001f", "'": "0c040800000000", "×": "00110a040a1100",
};
const glyph = (ch) => {
  const hex = MATRIX[ch] ?? MATRIX["?"];
  return [0, 1, 2, 3, 4, 5, 6].map((k) => parseInt(hex.slice(k * 2, k * 2 + 2), 16));
};
// Seven segments: bits a b c d e f g, from the top clockwise, g across the middle.
const SEGMENTS = { "0": 0x3f, "1": 0x06, "2": 0x5b, "3": 0x4f, "4": 0x66, "5": 0x6d, "6": 0x7d, "7": 0x07,
  "8": 0x7f, "9": 0x6f, "-": 0x40, " ": 0 };

function vfdCanvas(r, w, h) {
  if (!r.cv) {
    r.cv = document.createElement("canvas");
    r.out.append(r.cv);
  }
  const dpr = window.devicePixelRatio || 1;
  r.cv.width = Math.ceil(w * dpr);
  r.cv.height = Math.ceil(h * dpr);
  r.cv.style.width = `${w}px`;
  r.cv.style.height = `${h}px`;
  const g = r.cv.getContext("2d");
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, w, h);
  return g;
}

// Unlit elements first, faint, then the lit ones with their glow.
function vfdPaint(g, color, shapes, blur) {
  g.fillStyle = color;
  for (const [lit, alpha] of [[false, 0.08], [true, 1]]) {
    g.globalAlpha = alpha;
    g.shadowColor = lit ? color : "transparent";
    g.shadowBlur = lit ? blur : 0;
    for (const s of shapes) if (s.lit === lit) s.draw(g);
  }
}

function vfdMatrix(r, text, color) {
  const p = parseFloat(getComputedStyle(r.out).getPropertyValue("--dot")) || 3;
  r.w = Math.max(r.w, text.length);
  text = text.toUpperCase().padEnd(r.w);
  const g = vfdCanvas(r, (text.length * 6 - 1) * p + p, 8 * p);
  const shapes = [];
  [...text].forEach((ch, k) => {
    const rows = glyph(ch);
    for (let y = 0; y < 7; y++) {
      for (let x = 0; x < 5; x++) {
        const lit = !!(rows[y] & (16 >> x));
        const px = p / 2 + (k * 6 + x) * p, py = p / 2 + y * p, s = p * 0.78;
        shapes.push({ lit, draw: (c) => c.fillRect(px, py, s, s) });
      }
    }
  });
  vfdPaint(g, color, shapes, p * 1.6);
}

function vfdSegments(r, text, color) {
  const h = parseFloat(getComputedStyle(r.out).getPropertyValue("--seg")) || 24;
  const t = h * 0.13, w = h * 0.52, gap = t * 0.18, slant = h * 0.09;
  // Lay out digits, decimal points (on the digit before) and colons, keeping the widest row seen.
  const cells = [];
  for (const ch of text) {
    const last = cells[cells.length - 1];
    if (ch === "." && last && last.k === "d" && !last.dp) last.dp = true;
    else if (ch === ":") cells.push({ k: "colon" });
    else cells.push({ k: "d", ch, dp: ch === "." });
  }
  r.w = Math.max(r.w, cells.length);
  while (cells.length < r.w) cells.unshift({ k: "d", ch: " ", dp: false });
  const adv = (c) => (c.k === "colon" ? t * 2.6 : w + t * 1.6);
  const width = cells.reduce((s, c) => s + adv(c), 0) + slant + t;
  const g = vfdCanvas(r, width, h + t);
  g.transform(1, 0, -slant / h, 1, slant, t / 2);
  const hseg = (x0, x1, y) => (c) => {
    c.beginPath();
    c.moveTo(x0, y);
    c.lineTo(x0 + t / 2, y - t / 2);
    c.lineTo(x1 - t / 2, y - t / 2);
    c.lineTo(x1, y);
    c.lineTo(x1 - t / 2, y + t / 2);
    c.lineTo(x0 + t / 2, y + t / 2);
    c.fill();
  };
  const vseg = (x, y0, y1) => (c) => {
    c.beginPath();
    c.moveTo(x, y0);
    c.lineTo(x + t / 2, y0 + t / 2);
    c.lineTo(x + t / 2, y1 - t / 2);
    c.lineTo(x, y1);
    c.lineTo(x - t / 2, y1 - t / 2);
    c.lineTo(x - t / 2, y0 + t / 2);
    c.fill();
  };
  const dot = (x, y, rad) => (c) => {
    c.beginPath();
    c.arc(x, y, rad, 0, 2 * Math.PI);
    c.fill();
  };
  const shapes = [];
  let x = t / 2;
  for (const c of cells) {
    if (c.k === "colon") {
      shapes.push({ lit: true, draw: dot(x + t * 0.8, h * 0.32, t * 0.5) }, { lit: true, draw: dot(x + t * 0.8, h * 0.7, t * 0.5) });
    } else {
      const bits = SEGMENTS[c.ch] ?? 0;
      const L = x, R = x + w, T = 0, M = h / 2, B = h;
      const segs = [hseg(L + gap, R - gap, T), vseg(R, T + gap, M - gap), vseg(R, M + gap, B - gap),
        hseg(L + gap, R - gap, B), vseg(L, M + gap, B - gap), vseg(L, T + gap, M - gap), hseg(L + gap, R - gap, M)];
      segs.forEach((draw, k) => shapes.push({ lit: !!(bits & (1 << k)), draw }));
      shapes.push({ lit: c.dp, draw: dot(R + t * 0.85, B, t * 0.55) });
    }
    x += adv(c);
  }
  vfdPaint(g, color, shapes, t * 1.4);
}

RENDER.vfd = (r, text) => {
  const numeric = /^[-0-9.: ]+$/.test(text);
  if (numeric !== r.numeric) {
    r.numeric = numeric;
    r.w = 0;
  }
  (numeric ? vfdSegments : vfdMatrix)(r, text, r.color);
};

// A UTC station clock in the header, ticking on the second. On nixies it runs every cathode once a minute, the
// "slot machine" real clocks use against cathode poisoning.
function initClock() {
  const head = document.querySelector("header");
  if (!head) return;
  const wrap = document.createElement("span");
  wrap.className = "clock-wrap";
  wrap.title = "UTC";
  const c = document.createElement("span");
  c.id = "clock";
  c.className = "clock";
  const z = document.createElement("small");
  z.textContent = "UTC";
  wrap.append(c, z);
  head.insertBefore(wrap, $("theme") ?? $("conn"));
  const tick = () => {
    const now = new Date().toISOString().slice(11, 19);
    if (display.kind === "nixie" && now.endsWith(":00") && !reduceMotion()) {
      let n = 0;
      const spin = setInterval(() => {
        c.textContent = now.replace(/\d/g, () => String(Math.floor(Math.random() * 10)));
        if (++n > 10) {
          clearInterval(spin);
          c.textContent = now;
        }
      }, 50);
    } else {
      c.textContent = now;
    }
    setTimeout(tick, 1000 - (Date.now() % 1000) + 5);
  };
  tick();
}
initClock();
initTheme();
// Canvas readouts follow the pixel ratio; redraw them when it changes.
window.addEventListener("resize", () => display.items.forEach((r) => r.update(true)));
