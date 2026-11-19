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
| VARA FM 4.4.0 (narrow / wide) | ✅ | Proven against real VARA FM 4.4.1 in a lab loopback on 2026-11-18 (§6). Digipeating is xara↔xara only so far. |
| VARA SAT 4.4.5 (QO-100) | 🟡 | Receive: `xara-monitor --hf sat` decodes real VARA SAT data frames (a recorded link between two VARA SAT stations, every frame CRC-good, 2026-11-19). SAT has its own 17-level table (HF 2300's engine shifted down one level, with its own FSK levels), tables (seed 99), whitening (seed 414) and control codebook; the codebook reproduces every control codeword heard on that link. SAT's control codes (one tone per 1024-sample symbol, 46.875 Hz grid) are detected too: every one on that link, all 31 symbols. `xara --hf sat` links over SAT: SAT replaces HF's start-level codes with GO AHEAD and keeps BREAK/IDLE as per-link codes, as on the recorded link. TX/RX shift and Doppler schedules are in (`--tx-shift`, `--rx-shift`, `--doppler`). Tested xara to xara over simulated audio; not yet on air against real VARA SAT |
| VARA Chat 1.4.3 | ✅ | xara is the modem Chat drives (ports 8300/8301). Run end-to-end both ways between two xara in the lab (2026-11-18) |
| VARA Terminal 1.2.2 | ✅ | Same as Chat: run end-to-end both ways in the lab (2026-11-18) |
| VARA HF 4.9.1 / FM 4.4.1 after 2027-01-01 | ❌ | Those versions change their on-air format on 2027-01-01 (shown publicly by K3JSJ and KT7RUN). xara speaks the 4.9.0 / 4.4.0 format. |
| Pat 1.0.1 | ✅ | Pat's `varahf` transport drives xara (ports 8300/8301). Lab, 2026-11-18: two Pat over two xara HF 2300 ran a full P2P B2F session, one message each way, a 4053-byte attachment arriving byte-identical |
| VarAC 16.1.8 | ✅ | xara is the VARA HF and VARA FM modem VarAC drives. Lab, 2026-11-18, two VarAC over two xara, HF (VarAC links at 500 Hz) and FM wide: connect, S/N exchange, chat both ways, CQ with slot (listed on the other side), beacon (listed), QSY, VMail delivered and acknowledged, and a VMail attachment (file transfer) arriving byte-identical on HF (≈1080 bps) and FM (≈840 bps). FM CQ and beacons too: xara FM now decodes record 16 for CHAT ON clients, not only KISS ones, and re-arms it once its own TX has drained. Through a digipeater: the `CONNECT … VIA` link VarAC uses runs end to end over a `--digi` xara (driven from the TNC port; VarAC's own window not yet). VarAC closes any QSO after 240 s on its calling frequency and won't send attachments there |
| Winlink Express, BPQ32, vARIM | 🟡 | They talk the TNC protocol in §2, which xara implements. The real apps are untested. |

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
| Accept a 500 Hz caller while set to 2300 / 2750 | ✅ | Answered and run at 500 Hz (`audio_link.rs::wide_station_answers_narrow_callers`). Always on; in VARA it is the "Accept 500 Hz connections" option, off by default |
| "Tuner enhancement" (gateway lets a caller's first 2 requests pass so its ATU can tune) | ✅ | `--tuner-enhancement` (`audio_link.rs::tuner_enhancement_answers_the_third_request`) |

## 6. VARA FM extras

| Feature | xara | Notes |
|---|---|---|
| Narrow / wide, all levels | ✅ | Real VARA FM 4.4.1 and xara on one virtual audio loop, 2026-11-18. Connect, data both ways, byte-checked, and disconnect worked in all four cases (narrow/wide × either side calling). Results in the table below this one |
| Digipeater, up to 2 hops (`--digi`) | ✅ | `fm_digi_link.rs` |
| Ping (route test) | ✅ / 🟡 | xara answers a Ping like VARA FM does, direct or through digipeaters: a Reply record with the S/N it heard, and `PENDING` … `CANCELPENDING` to the client. Real VARA FM 4.4.1's Ping dialog shows xara's result (lab, 2026-11-18); `fm_frames.rs::vara_ping_and_reply` holds xara's Reply byte-for-byte to VARA's. Through a digipeater (lab, 2026-11-18) xara answers back along the path, and as a digipeater (`--digi`) relays the Ping and Reply between two real VARA FMs with the same frames and host lines as VARA's own digipeater (`digi.rs::relays_a_ping_and_its_reply_like_vara`, `fm_session.rs::a_digipeated_ping_gets_a_reply_back_through_the_digi`). Not yet: sending Pings (in VARA that is a GUI button, not a TCP command) |
| Autotune | ✅ | VARA FM sends 10 AutoTune records, each louder (step 1-10 in byte 33, -41 to -11 dBFS on the lab loop). The target replies with a step in byte 34 as step << 2, and the caller sets its drive to that step's ramp level counted from step 10 = 0 dB (1 = -30, 2 = -25, 4 = -18, 5 = -15, 6 = -12, 8 = -6 dB; 0 or above 10 shows NO SIGNAL). Real VARA picked steps 3 to 7, mostly 4, over a clean loop whatever the level into it (0, -10 and -20 dB tried); xara names three steps above the first it heard, so 4 on a clean path (lab, 2026-11-18). `fm_frames.rs::vara_autotune` holds the reply byte-for-byte to VARA's |
| Variable TX delay for handhelds | 🟡 | Use `--ptt-lead-ms`; there is no FM-specific setting |
| Audio equalisation for slow handheld receivers | ✅ | VARA FM 4.0 lists this as built in, with no setting. xara's QAM equaliser follows the noise floor row by row instead of assuming one for the whole frame, so a receiver still settling after transmitting (starting 20-30 dB down, settling over 0.15-1.2 s, with a low-frequency thump) decodes like a steady one. Tested on simulated receivers in `fm_settle.rs`, not against a real handheld |
| Monitor log of all stations on frequency | ✅ | `xara-monitor --fm` |

Lab runs against real VARA FM 4.4.1 (unregistered), nothing transmitted:

| Mode | Caller | Data each way | Highest level the other side decoded |
|---|---|---|---|
| narrow | VARA FM | 2000 / 1000 bytes, all correct | 2 (an unregistered caller caps the whole link) |
| narrow | xara | 2000 / 1000 bytes, all correct | 7, 7087 bps |
| wide | VARA FM | 6000 / 3000 bytes, all correct | 2 (same cap) |
| wide | xara | 20000 / 10000 bytes, all correct | 12, 22102 bps |

VARA FM's `FM Mode` setting is 1 for narrow and 2 for wide.

## 7. Station, radio and setup

| Feature | VARA | xara |
|---|---|---|
| Operating systems | Windows only | ✅ Linux, macOS, FreeBSD, Windows; Android (headless, AAudio; `docs/android.md`) |
| Audio | Windows sound cards | ✅ PulseAudio/PipeWire, ALSA, CoreAudio, WinMM, (tr)uSDX |
| Left / right / both sound-card channel | ✅ | ✅ `--channel left\|right\|both` on WinMM, PulseAudio, ALSA and CoreAudio (not (tr)uSDX) |
| PTT: serial RTS/DTR, CM108 GPIO, VOX | ✅ | ✅ (CM108 by USB id on macOS and Windows) |
| PTT / tuning via CAT | ✅ Chat/Terminal rig lists | ✅ via hamlib `rigctld`; built-in CAT (`--cat`, `--ptt cat`) |
| Remote Syslog client | ✅ (registered only) | ✅ `--syslog` |
| Setup screen | ✅ GUI | Web UI ✅; first-run wizard ✅; Windows settings save ✅ |
| Several instances on separate ports | ✅ one folder per instance | ✅ `--port N` (data on N+1), `--kiss-port`, `--config` |
| Recording RX/TX/PTT | ❌ | ✅ `--record` |
| Passive monitor of other stations | Monitor log (FM) | ✅ `xara-monitor`, HF and FM, with web UI |
| Price | Shareware; full speed needs a licence | Free, MIT |

## 8. Gaps, in priority order

Every gap here can be built and tested without transmitting: two xara instances, or xara and VARA, on virtual audio cables. On-air confirmation is for licensed operators.

| # | Gap | Why it matters | Where to start |
|---|---|---|---|
| G10 | **VARA SAT link layer** | QO-100 users | Done: data frames (all levels, decoded from real VARA SAT audio), the control codebook (matches a recorded link), the connect request and reply (`xara-phy/src/sat.rs`), control-code detection (every code on the recorded link), `xara-monitor --hf sat`, and SAT links from the TNC (`xara --hf sat`, xara to xara). Left: a link against real VARA SAT on air. The meaning of codes 45 and 47 is inferred. Issue #5 |
| G12 | **The 2027 VARA format** | Talking to upgraded stations after 2027-01-01 | not started in this repo |

Closed: G1 (VARA FM interop, §6), G2 (500 Hz callers), G3 (`CLEANTXBUFFER` already matches 4.9.0), G4 (`CWID`), G5 (`MISSING SOUNDCARD`), G6 (`--tuner-enhancement`), G7 (FM Autotune reply, §6), G9 (`--channel`), G11 (end-to-end runs with VARA Chat, Terminal, Pat and VarAC, §1), 2026-11-18; G8 (FM audio equalisation for slow handheld receivers, §6), 2026-11-19. Numbers are kept so old issue references still point somewhere.

Pick one, open an issue saying you're on it, and see [CONTRIBUTING.md](../CONTRIBUTING.md). Clean sources only: published documents, our own recordings and lab loopback, never VARA's binaries.
