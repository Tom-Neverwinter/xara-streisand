# XARA on Android

Goal: run XARA on a phone so **Pat** (the Winlink client) can do Winlink over
HF/FM from a handheld — no Windows, no laptop. VARA itself is Windows/x86 only,
so XARA is what makes this possible on ARM.

This is milestone 1: a **headless modem** — XARA built without a local audio
backend, exposing its **KISS/TCP TNC** (and the read-only web UI) on localhost,
with Pat connecting to it on the device. Audio to the radio is handled
off-device for now (see *Audio*, below).

## Status

The headless build **cross-compiles for every Android ABI** —
`aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android` —
and CI guards it (the `android` job runs `cargo check` for all three; `check`
does not link, so it needs no NDK). Portability fixes that got it there:

- `Ioctl` request type is `c_int` on bionic (not `c_ulong`).
- the async-signal-safe errno accessor is bionic's `__errno`.
- building with no audio backend is allowed on Android (as on Windows), so
  `open_capture()` just reports "no backend" at runtime.

Not yet done: a full linked build (needs the NDK linker), on-device audio, and
the Pat packaging. Those are the remaining #10 subtasks.

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

On the phone (e.g. under Termux, or as a service):

1. Start XARA's TNC with the KISS server on localhost:
   `xara --kiss-port 8100 …` (plus your call and modem options).
2. Point **Pat** at it as a KISS TNC over TCP (`localhost:8100`).
3. Pat drives connects/sessions; XARA does the modem work.

The web UI (`xara-monitor --ui-port …` for the passive monitor, or the TNC's UI)
is reachable from the phone's browser at `http://localhost:<port>/`.

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
it. Written, not yet run on a device.
