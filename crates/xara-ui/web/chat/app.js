"use strict";
// xara terminal: keyboard-to-keyboard over the TNC's own host ports, bridged by /chat/ws (see
// xara-ui/src/chat.rs).  Plain text: each line goes out on the data port ending in CR, and whatever the
// peer sends is shown as it arrives.  Commands are VARA's published TCP host commands.
const CH_DATA = 0;
const MAX_LOG = 2000;
const UTF8 = new TextEncoder();

const st = { ws: null, up: false, peer: "", buffer: 0, rx: "" };
const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };
const stamp = () => new Date().toISOString().slice(11, 19);
const myCall = () => $("mycall").value.trim().toUpperCase();
const enc = () => $("charset").value;

function log(cls, text) {
  const box = $("log");
  const line = el("div", "m " + cls);
  line.append(el("span", "t", stamp()), document.createTextNode(text));
  box.append(line);
  while (box.childElementCount > MAX_LOG) box.firstElementChild.remove();
  box.scrollTop = box.scrollHeight;
}

// ---- link to the bridge
function open() {
  const ws = new WebSocket(`${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/chat/ws`);
  ws.binaryType = "arraybuffer";
  st.ws = ws;
  setConn("connecting", "stale");
  ws.onopen = () => setConn("bridge up", "up");
  ws.onclose = () => {
    setConn("bridge down", "");
    setState(false, "");
    setTimeout(open, 3000);
  };
  ws.onmessage = (m) => {
    if (typeof m.data === "string") return reply(m.data);
    const b = new Uint8Array(m.data);
    if (b[0] === CH_DATA) onData(b.subarray(1));
  };
}
const cmd = (line) => st.ws && st.ws.readyState === 1 && st.ws.send("C " + line);
const data = (bytes) => {
  if (!st.ws || st.ws.readyState !== 1) return false;
  const f = new Uint8Array(bytes.length + 1);
  f[0] = CH_DATA;
  f.set(bytes, 1);
  st.ws.send(f);
  return true;
};

function reply(t) {
  if (t.startsWith("E ")) return log(/cannot|not allowed|failed/.test(t) ? "bad" : "sys", t.slice(2));
  const r = t.slice(2), w = r.split(" ");
  switch (w[0]) {
    case "CONNECTED": setState(true, w[1] === myCall() ? w[2] : w[1]); log("sys", r); break;
    case "DISCONNECTED": setState(false, ""); log("sys", r); flushRx(); break;
    case "BUFFER": st.buffer = Number(w[1]) || 0; $("buffer").textContent = st.buffer; break;
    case "PTT": $("ptt").classList.toggle("on", w[1] === "ON"); break;
    case "IAMALIVE": break;
    default: log("sys", r);
  }
}

function setState(up, peer) {
  st.up = up;
  st.peer = peer || "";
  const s = $("state");
  s.textContent = up ? "connected" : "disconnected";
  s.className = "state" + (up ? " up" : "");
  $("peer").textContent = st.peer;
}

// ---- text both ways
// shortcut: "latin1" covers what most keyboard-to-keyboard programs send; add charsets when someone needs one.
function encode(t) {
  if (enc() === "utf-8") return UTF8.encode(t);
  return Uint8Array.from(t, (c) => (c.charCodeAt(0) < 256 ? c.charCodeAt(0) : 63));
}

function sendText(t) {
  if (!t) return;
  if (!data(encode(t.replace(/\r?\n/g, "\r") + "\r"))) return log("bad", "bridge down");
  log("me", `${myCall() || "me"}> ${t}`);
}

// Show the peer's text a line at a time; a partial line waits for its CR/LF, or for a pause.
let idle = null;
function onData(b) {
  st.rx += new TextDecoder(enc() === "utf-8" ? "utf-8" : "latin1").decode(b);
  const lines = st.rx.split(/\r\n|\r|\n/);
  st.rx = lines.pop();
  for (const l of lines) log("peer-line", `${st.peer || "peer"}> ${l}`);
  clearTimeout(idle);
  if (st.rx) idle = setTimeout(flushRx, 1500);
}
function flushRx() {
  if (st.rx) log("peer-line", `${st.peer || "peer"}> ${st.rx}`);
  st.rx = "";
}

// ---- controls
function apply() {
  const call = myCall();
  if (!call) return log("bad", "set My call first");
  pref.set("xara.term.mycall", call); pref.set("xara.term.bw", $("bw").value); pref.set("xara.term.charset", enc());
  pref.set("xara.term.listen", $("listen").checked ? "1" : "0");
  $("me").textContent = call;
  cmd(`MYCALL ${call}`);
  if ($("bw").value) cmd(`BW${$("bw").value}`);
  cmd($("listen").checked ? "LISTEN ON" : "LISTEN OFF");
}
$("mycall").value = pref.get("xara.term.mycall", "");
$("bw").value = pref.get("xara.term.bw", "2300");
$("charset").value = pref.get("xara.term.charset", "latin1");
$("listen").checked = pref.get("xara.term.listen", "1") === "1";
$("me").textContent = myCall();
$("apply").onclick = apply;
$("connect").onclick = () => {
  const t = $("target").value.trim().toUpperCase(), via = $("via").value.trim().toUpperCase();
  if (!myCall() || !t) return log("bad", "set My call and the call to connect to");
  cmd(`CONNECT ${myCall()} ${t}${via ? ` VIA ${via}` : ""}`);
  $("state").textContent = "calling";
  $("state").className = "state calling";
};
$("disconnect").onclick = () => cmd("DISCONNECT");
$("abort").onclick = () => cmd("ABORT");
$("compose").onsubmit = (e) => {
  e.preventDefault();
  if (!st.up) return log("bad", "not connected");
  sendText($("text").value);
  $("text").value = "";
};
$("text").addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); $("compose").requestSubmit(); }
});
open();
