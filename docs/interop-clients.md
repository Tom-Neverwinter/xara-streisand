# Interop checklist: real VARA client apps against xara (#4)

The goal is to drive xara with the real client apps (VARA Chat, VARA Terminal, VarAC, Pat) and confirm clean sessions. The command lists below come from static analysis of VARA Chat 1.4.3 and VARA Terminal 1.2.2. Every command and reply they use is one xara's TNC already handles, so what's left is the live test.

## Setup

- **The xara TNC is Unix-only** (Linux, FreeBSD, macOS); the VARA apps are Windows programs.
  - Run `xara` on a Linux box, Pi or VM, and point the app's TCP host at it. Each app has separate host/port settings for VARA HF, FM and SAT.
  - Bind xara where the app can reach it: `--addr 0.0.0.0`, on a trusted network only.
- **Ports:**
  - command port: 8300 (`--port`);
  - data port: the command port + 1;
  - KISS: 8100 (`--kiss-port`), which VARA Chat uses for broadcasts.
- Two stations are needed, e.g. two xara instances on an audio loop, or one xara and one real VARA.

## What each app sends and expects

| | VARA Chat 1.4.3 | VARA Terminal 1.2.2 | xara |
|---|---|---|---|
| `MYCALL <call>` | ✓ | ✓ | handled |
| `CHAT ON` | ✓ | | handled (= `LISTEN CQ`) |
| `LISTEN ON` | | ✓ | handled |
| `COMPRESSION FILES` | | ✓ | handled |
| `BW500` / `BW2300` / `BW2750` | ✓ | ✓ | handled |
| `CONNECT <src> <dst> [VIA <digi>]` | ✓ | ✓ | handled (up to 2 digis) |
| `DISCONNECT`, `ABORT` | ✓ | ✓ | handled |
| replies parsed: `CONNECTED`, `DISCONNECTED`, `BUFFER n`, `PTT ON/OFF`, `WRONG` | ✓ | ✓ | sent |
| replies parsed: `NARROW` / `WIDE` (FM) | ✓ | ✓ | check in session |

**VARA Chat's own data formats ride on the data port, so xara only has to pass bytes through unchanged:**
- **File transfer:** a header `<len+1>` then `#` (raw Deflate, RFC 1951, level 5, for files ≤ 600,000 bytes) or `$` (raw bytes, up to 2,048,000), then the data.
- **CQ / `ALL:` broadcasts:** AX.25 UI frames sent over VARA's KISS port.

## Checklist

Record pass or fail, the bandwidth, and the xara commit for each item.

**VARA Chat → xara**
- [ ] The app connects to the command port; xara logs `MYCALL` and `CHAT ON`.
- [ ] `BW2300` from the app's menu is accepted (repeat for 500 and 2750).
- [ ] Connect to the other station: the app shows connected, and `BUFFER` counts move.
- [ ] Keyboard chat both ways, including text over one frame long.
- [ ] Send a small file (≤ 600 KB, which goes `#`/Deflate): the receiver saves an identical file.
- [ ] Send a large file (> 600 KB, which goes `$`/raw).
- [ ] CQ and an `ALL:` broadcast over KISS reach the other station.
- [ ] Disconnect, then abort mid-transfer: the app returns to idle.

**VARA Terminal → xara**
- [ ] `MYCALL`, `LISTEN ON` and `COMPRESSION FILES` are accepted.
- [ ] Connect and disconnect, both directions (as caller and as listener).
- [ ] Text exchange.

**VarAC → xara**
- [ ] Connect and a QSO. (Not statically analysed: watch xara's log for any `WRONG` reply.)

**Pat (Winlink) → xara**
- [ ] A full Winlink session against a gateway or a second station.

## If something fails

- Run xara with its log (`--log FILE`): every host command and reply is echoed there.
- A `WRONG` reply names a command xara doesn't parse. Note the exact line.
- File an issue with the app, version, bandwidth, the log excerpt, and whether a real VARA modem behaves differently.
