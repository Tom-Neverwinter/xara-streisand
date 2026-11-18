# building

every platform needs Rust 1.85 or newer (<https://rustup.rs>; the CI
toolchain is pinned in `.rust-version`). the standard build command is:

    cargo install --locked --git https://github.com/Tom-Neverwinter/xara-streisand xara

or from a checkout:

    cargo build --release          # target/release/xara and xara-monitor
    cargo test --workspace         # the whole suite; no radio or sound card needed

the rest of this doc covers platform-specific deps and setup quirks.

## cargo features (crate `xara`)

| feature | what |
|---|---|
| `pulse`, `alsa` | Unix audio backends, both on by default. pick one with `--no-default-features --features alsa` |
| `trusdx` | experimental (tr)uSDX support |
| `fixed-turbo`, `float-turbo` | turbo decoder arithmetic variants, used for testing on small ARM boards |

CoreAudio (macOS) and WinMM (Windows) are always built on their platform.

## linux

on debian/ubuntu:

    sudo apt install libpulse-dev libasound2-dev

on fedora:

    sudo dnf install pulseaudio-lib-devel alsa-lib-devel

then build the usual way. the README covers all the main options for
linux.

## macOS

no additional deps. audio goes through CoreAudio. devices are named by
CoreAudio UID or exact name:

    xara --list-devices
    xara --call YOURCALL --hf 2300 \
         --capture "USB Audio CODEC" --playback "USB Audio CODEC" \
         --radio --ptt rts:/dev/cu.usbserial-0001

without `--radio`, playback only goes into a virtual device such as
[BlackHole](https://github.com/ExistentialAudio/BlackHole).

you will need to grant `xara` microphone access on the first run.

### ptt

serial ports are `/dev/cu.*`. cm108 PTT only works by USB id, e.g.,
`cm108:1209:7388` for an AIOC.

to start xara at login, copy the launchd agent, replace YOU in it with your
login name, then load it:

    cp dist/launchd/xara.plist ~/Library/LaunchAgents/
    launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/xara.plist

to stop it (a plain kill gets it restarted):

    launchctl bootout gui/$(id -u)/io.github.x0rro.xara

it will ask for microphone access on first launch and on every reinstall.

## freebsd

install the deps:

    pkg install rust pkgconf alsa-lib alsa-plugins pulseaudio

then build the usual way. pulse and alsa are both supported. to use
`--radio` with alsa, pin a PCM in `~/.asoundrc`:

    pcm.digirig {
        type plug
        slave.pcm { type oss device "/dev/dsp5" }
    }

then use `--audio alsa --capture digirig --playback digirig`.

### ptt

cm108 PTT needs `hidraw(4)`, not `uhid(4)`: put `hidraw_load="YES"` and
`hw.usb.usbhid.enable="1"` in `/boot/loader.conf`. normally root-only,
to grant the `operator` group access: put the following in
`/etc/devfs.rules` and set `devfs_system_ruleset="xara"` in
`/etc/rc.conf`.

    [xara=10]
    add path 'hidraw*' mode 0660 group operator

serial PTT uses the `/dev/cuaU*` devices.

## windows

release builds: each tagged release has a `xara-VERSION-x86_64-pc-windows-msvc.zip`
(with its `.sha256` and a build-provenance attestation, `gh attestation verify`).
unzip it anywhere; no installer. to build it yourself:

needs Windows 10 or 11 and the MSVC Rust toolchain (`rustup` installs it and
asks for the Visual Studio Build Tools, "Desktop development with C++").
there are no other dependencies: audio goes through WinMM, the same API VARA
uses.

    cargo build --release
    target\release\xara.exe --list-devices
    target\release\xara.exe --call YOURCALL --hf 2300 ^
        --capture "Microphone (USB Audio CODEC)" --playback "Speakers (USB Audio CODEC)" ^
        --radio --ptt rts:COM3

devices are named as Windows' sound settings show them. WinMM truncates
names to 31 characters, and a longer name matches on its first 31. a
unique, case-insensitive substring also works. without `--radio`, playback
only goes to a device that looks like a virtual cable (VB-Audio CABLE,
Voicemeeter, …), so a test run can't key a real radio by accident.

### ptt

- serial RTS/DTR: `--ptt rts:COM3` (ports above COM9 also work as `\.\COM12`)
- hamlib: run `rigctld` and use `--ptt rigctld[:HOST:PORT]`
- `--ptt vox`
- cm108/cm119 HID PTT (AIOC, Digirig Lite…) by USB id: `--ptt cm108:VID:PID[:GPIO]`

### known gaps

- no Windows service: xara needs your audio devices, so it runs as a
  per-user logon task instead. `dist\windows\xara-task.ps1` registers it
  (config `%APPDATA%\xara\xara.conf`, log `%APPDATA%\xara\xara.log`).
- WinMM is the only backend (no WASAPI or ASIO).

## everything else

untested but probably close: other BSDs (try the FreeBSD steps) and musl
Linux (the ARM CI jobs build musl). Android/Termux is tracked in an issue.
reports and patches are welcome, see CONTRIBUTING.md.
