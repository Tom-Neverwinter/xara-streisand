# Changelog

## 0.2.0

2026-11-20. xara-streisand: maintained fork of x0rro/xara after upstream was withdrawn.

- releases carry macOS binaries (Apple silicon and Intel, unsigned) and the Android APK next to the Linux tarballs and the Windows zip; CI on main uploads macOS test builds as artifacts
- SAT: data frames now lead with 0.2 s of random symbols, as VARA SAT sends (HF uses 0.08 / 0.12 s). With the shorter lead VARA SAT accepted xara's opening header only about half the time, so xara calling SAT was a coin flip; now 9/9 in the lab (old and rebuilt 4.4.5)
- named keyings: `xara-keygen --name NAME --out DIR` writes a group's own HF and FM tables, loaded with `--keying DIR/hf_keying` (or `fm_keying` with `--fm`); stations hear only others on the same keying. Generator version 1 of XaraNext 1.3 (MIT, x0rro), byte for byte, so a name means the same tables in both. Checked live: two xara on one keying link, on different keyings they don't
- VARA FM 2027 mode: the codebook skip count now uses key + 687 (the seed keeps + 933), per XaraNext's spec; the old code used + 933 for both, wrong for keys within 246 of a multiple of 32768
- VARA HF 4.9.3 / FM 4.4.3 (2026-11-20) cancel the 2027-01-01 format switch; checked in the lab (clock in 2027: links with the default legacy tables, not with 2027 ones). `--tables auto` is removed, as following the date would now break links
- the 2027 VARA format: `--tables 2027` speaks VARA HF 4.9.1's and FM 4.4.1's 2027 mode: their new scrambling/interleaver tables, read from your own VARA folder (`--vara-dir`; nothing of VARA's ships), and the shifted control-code call keys (HF +687, FM +933). Linked live against real VARA run with its clock in 2027 (`lab/`): HF 500/2300/2750 up to L13/L14/L15, FM narrow and wide (G12)
- Android app (`android/build_apk.py`, CI artifact `xara-android-apk`): the modem and Pat in one APK, monitor (receive only) or TNC (after an explicit transmit confirmation), audio devices listed by name so a USB sound card on OTG is picked from a list, a foreground service that keeps running with the screen off, and the web UIs in-app; tested on an Android 14 emulator
- Android: run on an Android 14 emulator: linked builds for arm64, armv7 and x86_64 (NDK), the TNC on AAudio, Pat on the same device driving it over `varahf`, and on-device decode of real VARA audio; `docs/android.md` has the recipe
- built-in CAT: each protocol (Icom CI-V with USB echo, Kenwood/Elecraft, newer Yaesu, FT-817 5-byte) is tested tuning and keying an emulated rig over a pseudo-terminal; CAT ports without modem lines (socat, Bluetooth) now open on Linux/macOS
- VARA SAT receive: `xara-monitor --hf sat` decodes VARA SAT 4.4.5 data frames (its own level table, tables, whitening and control codebook), checked against recordings of two real VARA SAT stations, including every control code on the link; `xara --hf sat` links with real VARA SAT 4.4.5, calling and answering (G10; checked over a virtual audio cable, unregistered so up to L4)
- FM: decodes through a handheld receiver that is still settling after transmitting (gain rising over the frame, a low-frequency thump): the QAM equaliser now follows the noise floor row by row (G8). Before, the top levels lost about one frame in three behind such a receiver near their noise limit
- Windows: the TNC runs natively (WinMM audio; serial RTS/DTR, rigctld and VOX PTT; console Ctrl-C), and `xara-monitor` plus its web UI build and run there
- `xara-monitor --fm narrow|wide`: passive VARA FM decode (connects, CQ, session text), also in the web UI
- host ports close connections that open with an HTTP request line, so a web page can't drive the TNC
- CI adds a Windows job
- full speed by default; `--unregistered` caps the link like unregistered VARA
- HF stations set to 2300 / 2750 now answer 500 Hz callers, at 500 Hz
- `CWID ON/OFF` takes effect (US calls always ID, as before)
- `MISSING SOUNDCARD` is sent to the client when the audio device fails
- web UI: link controls (connect with vias, disconnect, listen, CQ), FM Ping and Autotune (xara can now send both; Autotune sets the drive from the answer), session timer / bytes / average speed, a session history, a heard-stations list, and five more themes (green phosphor, radar, synthwave, paper, high contrast)
- web UI: display themes that redraw the readouts as hardware: an airport split-flap board (cells flip through the drum, flip clock), nixie tubes (cathode stacks, decimal points, neon colons, a once-a-minute cathode run on the clock) and a car-dash VFD (7-segment numbers, 5x7 dot-matrix text, filament wires, segmented bar graphs), art deco drum counters (gold sunburst, stepped frames, digits that roll into place); a radar scope that plots the heard stations as echoes under a rotating sweep; a synthwave outrun horizon (striped sun, scrolling neon grid, chrome type); a UTC clock in the header; `?theme=NAME` in the address
- FM: the Autotune reply names a ramp step (byte 34 = step << 2), three above the first step heard; it used to name step 4 × the first step heard, so -6 dB or NO SIGNAL when step 1 was lost; if step 10 is lost it now replies when step 10 was due instead of not at all
- FM: answers VARA FM's Ping and Autotune, as proven against real VARA FM 4.4.1; `xara-monitor --fm --dump` prints connect, ping and reply records in full
- FM: answers Pings that come through digipeaters, and `--digi` relays Pings and their Replies, matching real VARA FM 4.4.1 frame for frame
- `--tuner-enhancement`: VARA's HF gateway option; callers' first 2 connect requests go unanswered so an ATU can tune
- `--channel left|right|both`: use one side of a stereo sound card, as VARA's channel setting
- docs/COMPATIBILITY.md: feature-by-feature chart against the VARA suite, checked by `vara_doc.rs`
- docs: platform support table, Windows build notes, contributor guide

## 0.1.3

- macOS / CoreAudio support thanks to NR5P's work in x0rro/xara#19
- FreeBSD support thanks to dcasati's work in x0rro/xara#29
- experimental (tr)uSDX support, thanks to VA3SHC's work in x0rro/xara#12
- revamped TX latency model

## 0.1.2

- `--log` produces timestamped log files
- new audio resampling and buffering path
- bugfix: call signs with SSID are encoded properly

## 0.1.1

recording feature and bugfixes

- `--record` flag captures rx, tx, and PTT in FLAC
- bugfix: long squelch tails no longer trigger false receive
- bugfix: phantom frames no longer send inappropriate compression

## 0.1.0

first release

- modem is fully compatible with VARA HF 4.9.0 and VARA FM 4.4.0
- web UI for configuration and management
- all the usual TCP ports (works with your favorite software)
- pipewire, pulse, and alsa support

