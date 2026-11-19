"use strict";

const SN_WINDOW = 300;
const NOTABLE = new Set(["connected", "disconnected", "pending", "cancel_pending", "link_registered",
  "registered", "unencrypted_link", "line"]);
const NOTICES = { timed_out: "Link timed out", unable_to_connect: "Unable to connect" };

let control = false;
let tuning = false;
let driveHeld = false;
let mode = "hf";
let restartAfterSave = false;
let settingsDirty = false;
let latestTs = 0;
let snPoints = [];
let verbosity = Number(pref.get("xara.verbosity", "0"));
const log = new LogView($("log"), 2000, 600);
log.filter = (e) => e.level <= verbosity;
const rx = meter("rx"), tx = meter("tx");

$("verbosity").value = String(verbosity);
$("verbosity").addEventListener("change", (e) => {
  verbosity = Number(e.target.value);
  pref.set("xara.verbosity", String(verbosity));
  log.render();
});

const { conn, wf } = waterfallPage("xara.waterfall", handle);
const send = (cmd) => conn.send(cmd);

function handle(m) {
  switch (m.t) {
    case "hello":
      $("mode").textContent = m.mode.toUpperCase();
      control = m.control;
      mode = m.mode;
      $("controls").hidden = !control;
      $("link-controls").hidden = !control;
      for (const b of document.querySelectorAll(".fm-only")) b.hidden = m.mode !== "fm";
      $("locked").hidden = !m.locked;
      $("drive").min = String(m.drive_min);
      meterFloor = m.meter_floor;
      if (control) {
        send("SETTINGS ?");
        send("DEVICES ?");
      }
      snPoints = [];
      log.reset();
      wf.reset(m.span);
      break;
    case "waterfall":
      wf.config(m);
      break;
    case "status":
      status(m);
      break;
    case "host":
      if (m.kind === "sn") addSn(m.ts, m.sn);
      log.add({ ts: m.ts, cls: m.kind, text: m.line, level: NOTABLE.has(m.kind) ? 0 : 1 });
      break;
    case "notice":
      log.add({ ts: m.ts, cls: "notice", text: NOTICES[m.kind] || m.kind, level: 0 });
      break;
    case "settings":
      showSettings(m);
      break;
    case "devices":
      fillDevices("dev-capture", m.capture);
      fillDevices("dev-playback", m.playback);
      break;
    case "settings_result":
      settingsResult(m);
      break;
    case "control":
      if (m.what === "restart") {
        setMsg(m.ok ? "Restarting…" : "Restart refused: the modem is busy.", !m.ok);
      }
      log.add({
        ts: m.ts ?? latestTs,
        cls: m.ok ? "control" : "notice",
        text: `UI ${m.what}${m.ok ? "" : " refused" + (m.reason ? `: ${m.reason}` : "")}`,
        level: 0,
      });
      break;
    case "trace":
      log.add({ ts: m.ts, cls: "trace", text: m.text, level: 2 });
      break;
  }
}

function status(s) {
  conn.alive();
  latestTs = s.ts;
  wallOffset = Date.now() / 1000 - s.ts;
  log.setOffset(wallOffset);

  $("calls").textContent = s.calls.join(" ");
  $("mode").textContent = `${mode.toUpperCase()} ${/^\d+$/.test(s.bw) ? s.bw + " Hz" : s.bw}`;

  const st = $("state");
  st.textContent = s.state === "Listen" ? "Idle" : s.state;
  st.className = "state " + (s.state === "Listen" ? "idle" : s.state === "Calling" ? "calling" : "linked");
  $("peer").textContent = s.peer ? `⇄ ${s.peer}` : "";

  const linked = s.state !== "Listen";
  $("level").textContent = linked ? s.level : "-";
  $("bps").textContent = s.bitrate ? s.bitrate.bps : "-";
  $("dir").textContent = s.bitrate ? (s.bitrate.tx ? "bps TX" : "bps RX") : "";
  $("sn").textContent = s.sn === null ? "-" : s.sn.toFixed(1);
  $("afc").textContent = s.afc === null ? "-" : (s.afc > 0 ? "+" : "") + s.afc.toFixed(1);
  $("tries").textContent = `${s.attempts} / ${s.retries}`;
  $("buffer").textContent = s.buffer;

  session(s);
  heard(s);
  sessions(s);
  probe(s.probe);

  wf.ptt = s.ptt;
  wf.setPassband(s.passband[0], s.passband[1]);
  if (control) {
    controls(s);
    linkControls(s);
  }
  $("ptt").classList.toggle("on", s.ptt);
  $("busy").classList.toggle("on", s.busy);
  $("clip").classList.toggle("on", s.rx.clip > 0);
  rx(s.rx);
  tx(s.tx);
  drawSn();
}

// Status times are seconds since xara started; wallOffset turns them into wall-clock time.
let wallOffset = 0;
const clock = (ts) =>
  new Date((ts + wallOffset) * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
function bytes(n) {
  return n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} kB` : `${(n / 1048576).toFixed(2)} MB`;
}
function span(secs) {
  const t = Math.max(0, Math.round(secs));
  const h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), x = t % 60;
  const two = (v) => String(v).padStart(2, "0");
  return h ? `${h}:${two(m)}:${two(x)}` : `${m}:${two(x)}`;
}
function ago(secs) {
  if (secs < 60) return `${Math.round(secs)} s ago`;
  return secs < 3600 ? `${Math.round(secs / 60)} min ago` : `${(secs / 3600).toFixed(1)} h ago`;
}
function cell(text, cls) {
  const td = document.createElement("td");
  td.textContent = text;
  if (cls) td.className = cls;
  return td;
}

function session(s) {
  const c = s.session;
  const secs = c ? s.ts - c.since : 0;
  $("dur").textContent = c ? span(secs) : "-";
  $("sent").textContent = c ? bytes(c.tx_bytes) : "-";
  $("recv").textContent = c ? bytes(c.rx_bytes) : "-";
  $("avg").textContent = c && secs >= 1 ? String(Math.round(((c.tx_bytes + c.rx_bytes) * 8) / secs)) : "-";
}

let heardKey = "";
function heard(s) {
  // Redraw when the list changes, and every 5 s so "last heard" stays current.
  const key = s.heard.map((h) => h.call + h.at).join() + Math.floor(s.ts / 5);
  if (key === heardKey) return;
  heardKey = key;
  $("heard-none").hidden = s.heard.length > 0;
  $("heard").replaceChildren(...s.heard.map((h) => {
    const tr = document.createElement("tr");
    const go = document.createElement("td");
    if (control) {
      const b = document.createElement("button");
      b.className = "btn small";
      b.type = "button";
      b.textContent = "Use";
      b.title = `Put ${h.call} in the callsign box`;
      b.addEventListener("click", () => { $("lc-call").value = h.call; $("lc-call").focus(); });
      go.append(b);
    }
    tr.append(cell(h.call, "mono"), cell(h.what), cell(h.sn === null ? "-" : h.sn.toFixed(1)),
      cell(ago(s.ts - h.at)), cell(String(h.count)), go);
    return tr;
  }));
}

let sessionsKey = -1;
function sessions(s) {
  if (s.sessions.length === sessionsKey) return;
  sessionsKey = s.sessions.length;
  $("sessions-none").hidden = s.sessions.length > 0;
  $("sessions").replaceChildren(...s.sessions.slice().reverse().map((p) => {
    const tr = document.createElement("tr");
    tr.append(cell(p.peer, "mono"), cell(clock(p.since)), cell(span(p.until - p.since)),
      cell(bytes(p.tx_bytes)), cell(bytes(p.rx_bytes)));
    return tr;
  }));
}

function probe(p) {
  const el = $("probe");
  el.hidden = !p;
  if (!p) return;
  if (p.sn === null) { el.textContent = `${p.call}: no answer (${clock(p.at)})`; return; }
  const sn = `heard us at ${p.sn.toFixed(1)} dB S/N`;
  const step = p.step === undefined ? "" : p.step ? `; Autotune step ${p.step}, drive set` : "; Autotune: no signal";
  el.textContent = `${p.call} ${sn}${step} (${clock(p.at)})`;
}

let listenHeld = 0;
function linkControls(s) {
  const idle = s.state === "Listen";
  for (const id of ["lc-connect", "lc-cq", "lc-ping", "lc-autotune"]) $(id).disabled = !idle || s.ptt;
  $("lc-disconnect").disabled = idle;
  // Don't fight a click that the TNC hasn't reported back yet.
  if (Date.now() > listenHeld) $("lc-listen").checked = s.listen;
}

const target = () => ["lc-call", "lc-via1", "lc-via2"].map((id) => $(id).value.trim().toUpperCase()).filter(Boolean);
$("link-controls").addEventListener("submit", (e) => {
  e.preventDefault();
  const t = target();
  if (t.length) send(`CONNECT ${t.join(" ")}`);
});
$("lc-disconnect").addEventListener("click", () => send("DISCONNECT"));
$("lc-cq").addEventListener("click", () => send("CQ"));
$("lc-ping").addEventListener("click", () => {
  const t = target();
  if (t.length) send(`PING ${t.join(" ")}`);
});
$("lc-autotune").addEventListener("click", () => {
  const t = target();
  if (t.length) send(`AUTOTUNE ${t[0]}`);
});
$("lc-listen").addEventListener("change", (e) => {
  listenHeld = Date.now() + 1500;
  send(`LISTEN ${e.target.checked ? "ON" : "OFF"}`);
});

function controls(s) {
  tuning = s.tuning;
  const t = $("tune");
  t.classList.toggle("on", s.tuning);
  t.textContent = s.tuning ? `Stop tune (${Math.ceil(s.tune_left)} s)` : "Tune";
  t.disabled = !s.tuning && (s.state !== "Listen" || s.ptt);
  $("abort").disabled = s.state === "Listen" && !s.tuning;
  if (!driveHeld) {
    $("drive").value = String(Math.round(s.drive));
    $("drive-val").textContent = `${s.drive.toFixed(0)} dB`;
  }
}

$("tune").addEventListener("click", () => send(tuning ? "TUNE OFF" : "TUNE ON"));
$("abort").addEventListener("click", () => send("ABORT"));
$("drive").addEventListener("pointerdown", () => { driveHeld = true; });
$("drive").addEventListener("input", (e) => {
  driveHeld = true;
  $("drive-val").textContent = `${e.target.value} dB`;
});
$("drive").addEventListener("change", (e) => {
  send(`DRIVE ${e.target.value}`);
  setTimeout(() => { driveHeld = false; }, 300);
});

function setMsg(text, bad) {
  const p = $("set-msg");
  p.textContent = text;
  p.classList.toggle("bad", !!bad);
}

function showSettings(m) {
  $("settings").hidden = false;
  $("set-path").textContent = m.available ? m.path : "";
  const err = $("set-error");
  err.hidden = !m.error;
  err.textContent = m.error || "";
  $("set-hf").hidden = mode !== "hf";
  $("set-fm").hidden = mode !== "fm";
  $("set-save").disabled = !m.available;
  $("set-reset").disabled = !m.available;
  if (!m.available) setMsg("Saving settings needs --state-dir, or running as a systemd service.", false);
  if (settingsDirty) return;
  for (const el of $("set-form").elements) {
    el.value = m.saved[el.name] || "";
    if (el.tagName === "INPUT") el.placeholder = m.values[el.name] || "";
    if (el.tagName === "SELECT") el.options[0].textContent = `command line (${m.values[el.name] || "default"})`;
  }
}

function fillDevices(id, list) {
  $(id).replaceChildren(...list.map((d) => {
    const o = document.createElement("option");
    o.value = d.name;
    o.label = d.desc;
    return o;
  }));
}

function settingsResult(m) {
  setMsg(m.ok ? (m.what === "save" ? "Saved." : m.text) : `${m.what}: ${m.text}`, !m.ok);
  if (m.what !== "save" && m.what !== "reset") return;
  if (m.ok) {
    settingsDirty = false;
    if (restartAfterSave) send("RESTART");
  }
  restartAfterSave = false;
}

$("set-form").addEventListener("input", () => { settingsDirty = true; });
$("set-save").addEventListener("click", () => {
  const lines = [...$("set-form").elements]
    .filter((el) => !el.closest("[hidden]"))
    .map((el) => `${el.name} ${el.value.trim()}`);
  restartAfterSave = true;
  send(["SETTINGS SAVE", ...lines].join("\n"));
});
$("set-reset").addEventListener("click", () => {
  restartAfterSave = true;
  send("SETTINGS RESET");
});
$("set-devices").addEventListener("click", () => send("DEVICES ?"));

function addSn(ts, sn) {
  snPoints.push({ ts, v: sn });
  while (snPoints.length && snPoints[0].ts < ts - SN_WINDOW) snPoints.shift();
}

function drawSn() {
  lineChart($("sn-chart"), snPoints, latestTs, SN_WINDOW, -10, 30);
}

window.addEventListener("resize", drawSn);
wf.drawAxis();
