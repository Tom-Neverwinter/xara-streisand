# VARA compatibility chart

What xara-streisand does against each part of the VARA suite, what is missing, and what to work on next.

**Sources:** VARA's published documents only: "VARA Protocol Native TNC Commands" (EA5HVK, 2025-10-10), the VARA KISS interface note, the HF 4.7 Quick Guide, "VARA FM 4.0", and the changelogs shipped with each program. The xara side is read from `main` and its tests. Status checked 2026-11-18.

**Legend:**
- ✅ works, with the test that proves it;
- 🟡 works xara-to-xara or in parts, but has not been confirmed against real VARA;
- ❌ missing;
- 🔀 on a branch, not merged yet;
- ➖ deliberately out of scope.

`cargo test --workspace` runs every test named here. The command and reply tables are checked by [`crates/xara-host/tests/vara_doc.rs`](../crates/xara-host/tests/vara_doc.rs): it fails if xara drifts from the published command set, or if a reply listed as not sent starts being sent without updating this chart.

## 1. The suite at a glance

| VARA product | xara | Notes |
|---|---|---|
| VARA HF 4.9.0 (500 / 2300 / 2750) | ✅ | Decodes real on-air VARA HF, including full Winlink sessions. All 13 / 16 / 17 published speed levels. |
| VARA FM 4.4.0 (narrow / wide) | 🟡 | Full PHY and ARQ, digipeating; passes xara↔xara over simulated audio. Interop with real VARA FM is unproven (issue #1). |
| VARA SAT 4.4.5 (QO-100) | ❌ | Not in xara. The generic part, TX/RX frequency shift and Doppler tracking, is 🔀 on `tx-rx-shift` / `doppler` (issue #5). |
| VARA Chat 1.4.3 | 🟡 | xara is the modem Chat drives (ports 8300/8301). Not run end-to-end against the real app yet (issue #4). |
| VARA Terminal 1.2.2 | 🟡 | Same as Chat. |
| VARA HF 4.9.1 / FM 4.4.1 after 2027-01-01 | ❌ | Those versions change their on-air format on 2027-01-01 (shown publicly by K3JSJ and KT7RUN). xara speaks the 4.9.0 / 4.4.0 format. |
| Winlink Express, Pat, VarAC, BPQ32, vARIM | 🟡 | They talk the TNC protocol in §2, which xara implements. Pat-style sessions are tested (`tnc_tcp.rs`). The real apps are untested. |

## 2. TNC commands (application → modem, port 8300)

Every documented command is understood (`vara_doc.rs::every_documented_command_is_understood`).

| Command | xara | Notes / evidence |
|---|---|---|
| `CONNECT src dst` | ✅ | `tnc_tcp.rs`, `host_protocol.rs` |
| `CONNECT src dst VIA d1 d2` (FM) | ✅ | `fm_digi_link.rs` |
| `LISTEN ON` / `OFF` | ✅ | `host_protocol.rs` |
| `MYCALL c1 … c5` | ✅ | Up to 5 calls; SSIDs -1…-15, -T, -R, plus -X / -L (later VARA changelogs). `command.rs` tests |
| `DISCONNECT` / `ABORT` | ✅ | `host_protocol.rs` |
| `COMPRESSION OFF` / `TEXT` / `FILES` | ✅ | Huffman text compression, `textcomp.rs`. Off on unregistered-capped links (see `--unregistered`) |
| `BW500` / `BW2300` / `BW2750` | ✅ | |
| `CHAT ON` / `OFF`, `LISTEN CQ` | ✅ | Chat timing and CQ listening |
| `WINLINK SESSION` / `P2P SESSION` | ✅ | Retry cycle, `hf_timing.rs` |
| `TUNE -X` / `TUNE ?` / `TUNE OFF` | ✅ | |
| `CLEANTXBUFFER` | ✅ | As real VARA HF 4.9.0 does it: `BUFFEREMPTY`, or `FAILED` with the queued data kept (`vectors/host`, `host_protocol.rs`). The PDF's erase-and-`OK` is not what 4.9.0 sends |
| `VERSION` | ✅ | |
| Not in the PDF but used by apps: `RETRIES`, `DRIVELEVEL`, `IGNOREKISSDCD`, `PUBLIC`, `GATEWAY`, `CWID` | ✅ | `CWID ON/OFF` turns the disconnect CW ID on or off for non-US calls (`host_protocol.rs::cwid_on_off_sets_the_link_config`). `PUBLIC` answers `OK` like 4.9.0; VARA documents no effect for it |

## 3. Replies (modem → application)

| Reply | xara | Notes |
|---|---|---|
| `CONNECTED` (HF with BW, FM with VIA and NARROW/WIDE) | ✅ | |
| `DISCONNECTED`, `PTT ON/OFF`, `BUFFER n`, `PENDING`, `CANCELPENDING`, `BUSY ON/OFF` | ✅ | `host_protocol.rs`, `tnc_tcp.rs` |
| `REGISTERED calls`, `LINK REGISTERED` / `UNREGISTERED` | ✅ | |
| `IAMALIVE` (every 60 s) | ✅ | `host_protocol.rs::registered_and_iamalive` |
| `CQFRAME src [bw]` | ✅ | `xara-arq/src/cq.rs` |
| `SN`, `BITRATE (level) bps` | ✅ | |
| `CLEANTXBUFFER BUFFEREMPTY` / `FAILED` | ✅ | Spelled as real VARA 4.9.0 sends them (`vectors/host`); the PDF runs the words together |
| `CLEANTXBUFFER OK` | ➖ | Real VARA HF 4.9.0 never sends it (see §2) |
| `ENCRYPTION DISABLED`, `UNENCRYPTED LINK` | ✅ | |
| `ENCRYPTION READY`, `ENCRYPTED LINK` | ➖ | VARA's AES-256 is only for licensed non-amateur services; encrypted traffic is not allowed on amateur bands in most countries |
| `MISSING SOUNDCARD` | ✅ | Sent when the audio device fails, just before xara exits, so gateway software can restart it |
| `OK`, `WRONG` | ✅ | |

## 4. KISS / APRS interface

| Feature | xara | Notes |
|---|---|---|
| KISS over TCP (`--kiss-port`, default 8100) | ✅ | `kiss.rs`, `unproto_link.rs` |
| Frame types 0 (AX.25), 1 (AX.25, 7-character calls), 2 (generic data) | ✅ | `xara-arq/src/kiss.rs` |
| p-persistence 0.3, DCD wait, `--persistence` | ✅ | |
| Fragmentation of long frames, broadcasts | ✅ | `fm_broadcast.rs`, `kiss.rs` |
| Multiple KISS clients | ✅ | Up to 10 at once (`xara-host/src/kiss.rs`) |

## 5. Link behaviour

| Feature | xara | Notes |
|---|---|---|
| Adaptive speed (up and down the level ladder) | ✅ | `adapt.rs`, `link.rs`, golden runs in `xara-arq/tests/golden` |
| Full speed by default (all levels + compression) | ✅ | `--unregistered` caps it like unregistered VARA |
| Busy-channel detector | ✅ | `xara-phy/tests/busy.rs` |
| CW ID at disconnect | ✅ | Always for US calls, as in VARA; `CWID ON` for everyone else |
| Callsign hashing / multiple MYCALLs | ✅ | `crc_call.rs` |
| Files up to 600 000 bytes per session (HF) | ✅ | TX ring size |
| Accept a 500 Hz caller while set to 2300 / 2750 | ✅ | Answered and run at 500 Hz (`audio_link.rs::wide_station_answers_narrow_callers`). Always on; VARA makes it an option |
| Gateway first-reply delay for antenna tuners, "Tuner enhancement" extra retries | ❌ | Quick Guide options. Gap G6 |

## 6. VARA FM extras

| Feature | xara | Notes |
|---|---|---|
| Narrow / wide, all levels | 🟡 | `fm_audio_link.rs`, `fm_link.rs`; real-VARA interop unproven |
| Digipeater, up to 2 hops (`--digi`) | ✅ | `fm_digi_link.rs` |
| Ping (route test) | ❌ | Frames decode (`xara-monitor --fm` shows them); xara neither sends nor answers them. Gap G7 |
| Autotune | ❌ | Same as Ping. Gap G7 |
| Variable TX delay for handhelds | 🟡 | Use `--ptt-lead-ms`; there is no FM-specific setting |
| Audio equalisation for slow handheld receivers | ❌ | Gap G8 |
| Monitor log of all stations on frequency | ✅ | `xara-monitor --fm` |

## 7. Station, radio and setup

| Feature | VARA | xara |
|---|---|---|
| Operating systems | Windows only | ✅ Linux, macOS, FreeBSD, Windows; Android 🔀 (`android-headless`, `android-audio`) |
| Audio | Windows sound cards | ✅ PulseAudio/PipeWire, ALSA, CoreAudio, WinMM, (tr)uSDX |
| Left / right / both sound-card channel | ✅ | ❌ gap G9 (mono only) |
| PTT: serial RTS/DTR, CM108 GPIO, VOX | ✅ | ✅ (CM108 on Windows 🔀 `windows-cm108`) |
| PTT / tuning via CAT | ✅ Chat/Terminal rig lists | ✅ via hamlib `rigctld`; built-in CAT 🔀 `cat-control` |
| Remote Syslog client | ✅ (registered only) | 🔀 `syslog` |
| Setup screen | ✅ GUI | Web UI ✅; first-run wizard 🔀 `setup-wizard`; Windows settings save 🔀 `windows-settings` |
| Several instances on separate ports | ✅ one folder per instance | ✅ `--port N` (data on N+1), `--kiss-port`, `--config` |
| Recording RX/TX/PTT | ❌ | ✅ `--record` |
| Passive monitor of other stations | Monitor log (FM) | ✅ `xara-monitor`, HF and FM, with web UI |
| Price | Shareware; full speed needs a licence | Free, MIT |

## 8. Gaps, in priority order

Every gap here can be built and tested without transmitting: two xara instances, or xara and VARA, on virtual audio cables. On-air confirmation is for licensed operators.

| # | Gap | Why it matters | Where to start |
|---|---|---|---|
| G1 | **Prove FM against real VARA FM** | FM is the most-used emcomm mode after HF Winlink | issue #1; loopback xara↔VARA FM on virtual cables |
| G6 | **Gateway tuner delay / tuner-enhancement retries** | Gateways with auto-tuners after a band change | `hf/timing.rs` |
| G7 | **FM Ping and Autotune** | Route testing and level setting on FM nets | `xara-arq/src/fm/session.rs` (frames already parse) |
| G8 | **FM audio equalisation** | Handhelds with slow receivers | `xara-phy/src/fm` |
| G9 | **Left / right channel selection** | Stereo interfaces with two radios | `xara/src/audio` |
| G10 | **VARA SAT** | QO-100 users | merge `doppler`, then the SAT link layer |
| G11 | **End-to-end runs with VARA Chat, Terminal, VarAC, Pat** | Proves the client side | issue #4 |
| G12 | **The 2027 VARA format** | Talking to upgraded stations after 2027-01-01 | not started in this repo |

Closed: G2 (500 Hz callers), G3 (`CLEANTXBUFFER` already matches 4.9.0), G4 (`CWID`), G5 (`MISSING SOUNDCARD`), 2026-11-18. Numbers are kept so old issue references still point somewhere.

Pick one, open an issue saying you're on it, and see [CONTRIBUTING.md](../CONTRIBUTING.md). Clean sources only: published documents, our own recordings and lab loopback, never VARA's binaries.
