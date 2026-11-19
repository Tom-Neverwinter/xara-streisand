# XARA on Android

Goal: run XARA on a phone so **Pat** (the Winlink client) can do Winlink over
HF/FM from a handheld: no Windows, no laptop. VARA itself is Windows/x86 only,
so XARA is what makes this possible on ARM.

## The app

`xara.apk` (Android 8.0+, arm64 / armv7 / x86_64) holds the modem (`xara`,
`xara-monitor`) and **Pat**, and runs them for you:

- **Role**: *Monitor* listens only and never transmits (the default);
  *TNC* is the modem for Pat or any VARA client, and starts only after
  "I am licensed: transmit" is ticked (asked again each time the app opens).
- **Mode**: HF 2300 / 500 / 2750, VARA SAT, FM narrow / wide.
- **Audio from / to the radio**: Android's audio devices by name and id. A
  USB sound card on OTG (SignaLink, Digirig, a CM108 card) shows as
  `USB: ...`; the lists update when one is plugged in, and plugging one in
  offers to open the app. PTT is VOX (the interface's VOX, or the radio's).
- **Pat**: ticked, the app writes Pat's config (both VARA transports pointed
  at the TNC on `127.0.0.1:8300`) and runs `pat http`; **Pat** opens its web
  UI. **Modem UI** opens xara's (the TNC's with its control token).
- It runs under a foreground service (type microphone) with a wake lock, so
  it keeps listening with the screen off; Stop is in the app and in the
  notification. The live log is at the bottom of the app.

Build it with `python android/build_apk.py --pat` (below); CI builds it on
every run as the `xara-android-apk` artifact (debug-signed).

## Status

Tested on Android 14 (x86_64 emulator image, 2026-11-19), with the app:

- installs; Android extracts the modem and Pat as executables;
- *Monitor*: xara-monitor listens through AAudio, its web UI shows live
  input and the waterfall in the app; still running with the app in the
  background and the screen off (foreground service, wake lock held); Stop
  ends it;
- *TNC + Pat*: xara (TNC, KISS and web UI ports) and Pat's web UI ("Ready")
  both run; Pat from the app connects through the TNC: xara calls with VOX
  PTT, gives up with no station answering, sends its CW ID and reports
  DISCONNECTED;
- linked builds for all three ABIs with the NDK (27.2, API 26); without the
  app, `xara` and `xara-monitor` run from `adb shell` as well, and
  `xara-monitor` decodes a recording of real VARA SAT on the device (every
  frame CRC-good, every control code).

Not yet done: a real phone with a USB-OTG sound card and a radio
(receive-only first). The emulator's audio is the PC's, so nothing here
reached a radio. Serial PTT/CAT through OTG needs Android's USB host API
(apps can't open `/dev/tty*`), so the app keys by VOX for now.

### Building the app

```sh
export ANDROID_HOME=...            # SDK with build-tools and platform 35
export ANDROID_NDK_HOME=...        # or the newest under $ANDROID_HOME/ndk
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
python android/build_apk.py --pat  # -> target/android/xara.apk
```

It needs a JDK 17+ and, for `--pat`, Go 1.26 (Pat is built for
`GOOS=android` with the NDK's clang, so it resolves names like any app).
`--abis arm64-v8a` builds one ABI. Signing: a debug key unless
`XARA_KEYSTORE`, `XARA_KEYSTORE_PASS` and `XARA_KEY_ALIAS` name yours. The
script first runs the app's command-line rules on the JVM
(`android/test/CommandsTest.java`).

## The binaries alone (adb or Termux)

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

### Run, with Pat

Over `adb` (or the same commands in Termux; anything under
`/data/local/tmp` works from `adb shell`):

```sh
adb push xara xara-monitor pat /data/local/tmp/
adb shell
cd /data/local/tmp && chmod 755 xara xara-monitor pat
./xara --call YOURCALL --capture default --playback default --radio --ptt vox &
```

Pat is Go; a static Linux build runs on Android unchanged (without DNS, as
Android has no `/etc/resolv.conf`; the app's Pat is built for `GOOS=android`
instead):

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

Linking needs the NDK since `libaaudio.so` comes from it. Run on an Android 14
emulator (see Status); not yet on a phone. The app lists each device's id by
name, so a USB sound card is picked from a list there.

CI builds and links an arm64 binary the same way (NDK clang for API 26 as the
linker) and keeps it as the `xara-aarch64-linux-android` artifact of each run.
Without cargo-ndk:

```sh
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android26-clang
cargo build --release --no-default-features -p xara --target aarch64-linux-android
```
