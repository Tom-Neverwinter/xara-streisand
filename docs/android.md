# XARA on Android

Goal: run XARA on a phone so **Pat** (the Winlink client) can do Winlink over
HF/FM from a handheld — no Windows, no laptop. VARA itself is Windows/x86 only,
so XARA is what makes this possible on ARM.

This is milestone 1: a **headless modem** — XARA built without a local audio
backend, exposing its **KISS/TCP TNC** (and the read-only web UI) on localhost,
with Pat connecting to it on the device. Audio to the radio is handled
off-device for now (see *Audio*, below).

## Status

Tested on Android 14 (x86_64 emulator image, 2026-11-19):

- linked builds for `aarch64-linux-android`, `armv7-linux-androideabi` and
  `x86_64-linux-android` with the NDK (27.2, API 26); CI links arm64 on every
  run and keeps it as an artifact;
- `xara` runs as the TNC with **AAudio** capture and playback
  (`--capture default --playback default --radio --ptt vox`): both streams
  open, the command, data and KISS ports answer;
- **Pat** (v1.0.1, a static `linux/amd64` build) on the same device drives it
  over `varahf` on localhost: it initialises the modem and connects, and xara
  calls over AAudio, keying VOX PTT each call, then gives up as it should
  with no station answering;
- `xara-monitor` on the device decodes a recording of real VARA SAT (every
  frame CRC-good, every control code).

Not yet done: a real phone with a USB-OTG sound card and a radio, and an APK.
The emulator's audio is the PC's, so nothing here reached a radio.

## Build

`cargo check` needs nothing but the target:

```sh
rustup target add aarch64-linux-android
cargo check --no-default-features -p xara -p xara-ui --target aarch64-linux-android
```

A full (linked) build needs the Android **NDK** for the linker. The easy path
is [`cargo-ndk`](https://github.com/bbqsrc/cargo-ndk):

```sh
rustup target add aarch64-linux-android
cargo install cargo-ndk
cargo ndk -t arm64-v8a build --release --no-default-features -p xara
# -> target/aarch64-linux-android/release/{xara, xara-monitor}
```

`--no-default-features` is required: it drops the `pulse`/`alsa` audio backends,
which do not exist on Android.

## Run, with Pat

Over `adb` (or the same commands in Termux; anything under
`/data/local/tmp` works from `adb shell`):

```sh
adb push xara xara-monitor pat /data/local/tmp/
adb shell
cd /data/local/tmp && chmod 755 xara xara-monitor pat
./xara --call YOURCALL --capture default --playback default --radio --ptt vox &
```

Pat is Go; a static Linux build runs on Android unchanged:

```sh
GOOS=linux GOARCH=arm64 CGO_ENABLED=0 go install github.com/la5nta/pat@latest
```

and its config (`pat --config pat.json`, with `HOME` set to a writable
directory) points the VARA HF transport at xara:

```json
{"mycall": "YOURCALL", "secure_login_password": "...",
 "varahf": {"addr": "127.0.0.1:8300", "bandwidth": 2300, "rig": "", "ptt_ctrl": false}}
```

Then `pat connect varahf:///CALL` (or `pat http` and the browser). Pat checks
your Winlink account online before each connect; with no network it asks, so
run `pat init` once while online. Use `--ptt vox` with a VOX interface, or
`--ptt cat` / a serial PTT line when the OTG interface has one.

The web UI (`--ui-port`, or `xara-monitor --ui-port …` for the passive
monitor) is reachable from the phone's browser at `http://localhost:<port>/`.

## Audio

XARA has no Android audio backend yet (it has ALSA/PulseAudio/CoreAudio). Until
one exists, get audio to the radio **off the modem process**:

- a **USB-OTG sound card** (digirig/SignaLink-style) to the rig, bridged to
  XARA, or
- a future native **AAudio/OpenSL** backend (the clean long-term answer).

A headless XARA reached over TCP is useful on its own (decode piped audio,
monitor, bridge to Pat); a native mic/speaker path is milestone 2.

## iOS

Harder — no Termux, background-audio limits, and a different toolchain. Scope it
separately; the archived NR5P fork has an iOS branch to reference.

## Audio (AAudio)

`--audio aaudio` (the default on Android) uses the NDK's AAudio API, Android 8.0
(API 26) or newer: 48 kHz mono 16-bit, low-latency shared streams. Native code
gets no device names from Android, so `--capture` and `--playback` are
`default` or a numeric AAudio device id, the `AudioDeviceInfo.getId()` value a
Java/Kotlin app (or `dumpsys media.audio_policy`) shows for, e.g., a USB sound
card on OTG. Every Android output is a real device, so playback needs
`--radio`; use the headless KISS mode when the audio lives elsewhere.

Linking needs the NDK (`cargo ndk ... build`) since `libaaudio.so` comes from
it. Run on an Android 14 emulator (see Status); not yet on a phone.

CI builds and links an arm64 binary the same way (NDK clang for API 26 as the
linker) and keeps it as the `xara-aarch64-linux-android` artifact of each run.
Without cargo-ndk:

```sh
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android26-clang
cargo build --release --no-default-features -p xara --target aarch64-linux-android
```
