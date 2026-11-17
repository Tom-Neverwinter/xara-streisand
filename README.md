# xara-streisand

An open, VARA-compatible HF/FM modem for **Linux, macOS, FreeBSD and Windows**, written in Rust.

xara-streisand is a maintained fork of [XARA](https://github.com/x0rro/xara) (MIT, by x0rro), which was withdrawn upstream in November 2026. It keeps stations that run VARA HF 4.9.0 and VARA FM 4.4.0 reachable with open software, so Winlink, Pat, VarAC and similar programs can keep using a modem nobody can switch off. The name is a nod to the [Streisand effect](https://en.wikipedia.org/wiki/Streisand_effect).

- **`xara`**: the modem (TNC). It presents VARA's TCP interface: command port 8300, data port 8301, plus a KISS port, so existing VARA client software drives it unchanged.
- **`xara-monitor`**: a receive-only monitor that decodes VARA HF and FM traffic between other stations. It never transmits.

Tested against VARA HF 4.9.0 and VARA FM 4.4.0. Help is welcome, see [CONTRIBUTING.md](CONTRIBUTING.md).

## Goal: the whole VARA suite, one free program

The aim is full compatibility with the entire VARA family, in one program that anyone can set up for free on any common OS. Wherever VARA can talk to VARA, xara should talk to either one.

| VARA product | xara today |
|---|---|
| VARA HF (500 / 2300 / 2750) | Working. Decodes real on-air VARA HF, including full Winlink sessions |
| VARA FM (narrow / wide) | Working xara-to-xara. Interop with real VARA FM still unproven |
| VARA SAT | Not yet (the HF-2300 waveform plus Doppler shift) |
| VARA Chat / VARA Terminal | xara speaks their TCP interface. End-to-end tests against the real apps are still pending |
| Winlink Express, Pat, VarAC, other VARA clients | Point them at `127.0.0.1:8300` |

Open gaps are tracked in the issues. Pick one up, see [CONTRIBUTING.md](CONTRIBUTING.md).

## Who transmits

**This project does not transmit, and neither do its developers.** Everything here is built and tested without a radio: recorded test vectors, simulated links, and audio routed between programs over virtual cables (xara ↔ xara, xara ↔ VARA on the same machine). That gets very close to on-air behaviour without anyone keying up.

Putting a signal on the air is the job of a **licensed amateur radio operator**, using their own station under their own country's rules. `xara` refuses to drive a real audio device unless you pass `--radio`, so a test setup can't transmit by accident. If you are licensed and test on air, please report what you find in an issue (see [CONTRIBUTING.md](CONTRIBUTING.md)).

## Make your own copy (please don't fork)

Please **don't press GitHub's Fork button**. Clone the repo and push it to a new repository of your own with a funny name instead. Copies outside one fork network are harder to lose in one go, and that is the point of the name. `xara-streisand` is taken, but these are all yours for the taking:

- `maga-hertz`: it's a radio unit, and it was right there
- `signalgate-but-its-ham-radio`: at least our group chat is supposed to be public
- `epstein-files-release-date-tbd`: the only source in Washington you can read unredacted
- `pardon-my-modem` (Roger Stone not included)
- `matt-gaetz-ag-for-8-days`: this modem has already lasted longer
- `sharpie-forecast-modem`: NOAA got cut, draw your own hurricane cone
- `fema-404-not-found`: emergency comms that work when the agency doesn't
- `cpb-cant-rescind-this`: you can't claw back an MIT license
- `brendan-carr-cant-revoke-this`: the FCC can pull a license, not a `git clone`
- `doge-this`: already costs $0, efficiency achieved
- `big-beautiful-bandwidth`, `liberation-day-for-your-radio`, `covfefe-error-correction`, `no-kings-just-qsos`

    git clone https://github.com/Tom-Neverwinter/xara-streisand my-funny-name
    cd my-funny-name
    git remote set-url origin https://github.com/YOU/my-funny-name.git
    git push -u origin main

Keep `LICENSE` and the copyright notice (MIT requires it). To send a change back, open an issue here that links the branch in your copy, and it gets pulled from there. (GitHub only opens pull requests between forks.)

## Platform support

| OS | Audio (`--audio`) | PTT (`--ptt`) | Status |
|---|---|---|---|
| Linux (x86_64, arm64, armv7, Raspberry Pi) | `pulse` (default; also PipeWire), `alsa` | rts/dtr, cm108, rigctld, vox | Primary platform. CI on every push |
| macOS (Apple Silicon, Intel) | `coreaudio` | rts/dtr, cm108 (by USB id), rigctld, vox | CI-tested |
| FreeBSD | `pulse`, `alsa` | rts/dtr, cm108 (via `hidraw`), rigctld, vox | CI type-checked |
| Windows 10/11 | `winmm` | rts/dtr (`COM3`), rigctld, vox. No cm108 yet | New in this fork. Tests pass locally. The web UI cannot save settings yet. **Needs on-air testers** |
| (tr)uSDX | `trusdx` (audio over the rig's USB serial) | built in | Experimental |

Platform-specific dependencies and setup are in [BUILDING.md](BUILDING.md).

## Quick start

1. Install Rust 1.85 or newer from <https://rustup.rs>, plus the system packages for your OS listed in [BUILDING.md](BUILDING.md).
2. Build and install:

       cargo install --locked --git https://github.com/Tom-Neverwinter/xara-streisand xara

3. Find your sound card's names:

       xara --list-devices

4. Run, receive-only first (no `--radio`: nothing is keyed):

       # Linux
       xara --call YOURCALL --hf 2300 --audio alsa             --capture plughw:CARD=Device,DEV=0 --playback plughw:CARD=Device,DEV=0
       # macOS
       xara --call YOURCALL --hf 2300 --capture "USB Audio CODEC" --playback "USB Audio CODEC"
       # Windows (PowerShell / cmd)
       xara --call YOURCALL --hf 2300 --capture "Microphone (USB Audio CODEC)" --playback "Speakers (USB Audio CODEC)"

5. To transmit, add `--radio` and a `--ptt` method (see [PTT settings](#ptt-settings)). Then point your client software (Pat, Winlink Express, VarAC…) at `127.0.0.1:8300` as if it were VARA.

Every option can also go in a config file (`--config FILE`, `key = value` lines). See [`dist/xara.conf.example`](dist/xara.conf.example).

## Features

- VARA HF 500 / 2300 / 2750 and VARA FM narrow / wide, ARQ sessions and broadcasts
- web UI for configuration and management (`--ui-port`)
- the usual TCP ports: VARA command/data and KISS
- FM digipeating (`--digi N1CALL`)
- recording of RX, TX and PTT to FLAC/WAV (`--record`)
- `xara-monitor`: passive VARA HF and FM monitor, with a web UI

## Building from a checkout

    git clone https://github.com/Tom-Neverwinter/xara-streisand
    cd xara-streisand
    cargo build --release        # binaries land in target/release/
    cargo test --workspace

The systemd, udev and sysusers files and `dist/install.sh` are Linux-only. The macOS launchd agent is in `dist/launchd/`.

## Repository layout

| Path | What |
|---|---|
| `crates/xara-phy` | Signal processing: coding, modulation, receivers, turbo decoder (HF and FM) |
| `crates/xara-arq` | Sans-IO ARQ state machines, compression, AX.25/KISS and broadcast codecs |
| `crates/xara-host` | VARA command/data and KISS TCP servers |
| `crates/xara-ui` | Telemetry bus and the web UI server |
| `crates/xara` | The `xara` and `xara-monitor` binaries: audio backends, PTT, CLI, station glue |
| `crates/xara-testkit` | Test-only helpers |
| `vectors/` | Recorded test vectors the test suite replays |
| `dist/` | Service files, man page, example config |
| `scripts/` | Reproducible release builds |

## Installing with EmComm Tools

If you're using a version of EmComm Tools that includes XARA support (R7 at the time of writing), you don't need to build or install XARA by hand. Download [the 0.1.3 source release](https://github.com/anelson/xara/releases/download/v0.1.3/xara-0.1.3.tar.gz), save it as `xara-0.1.3.tar.gz`, and follow the [EmComm Tools xara source package requirements](https://github.com/thetechprepper/emcomm-tools-os-community/blob/release/r7/scripts/xara.md). In most cases it is enough to place the archive in `emcomm-tools-os-community/scripts/` and proceed as normal.

### Reproducing a release

Release tarballs are reproducible. Rebuild one in an `ubuntu:22.04` container on x86_64:

    # uncomment one of these:
    TARGET=x86_64-unknown-linux-gnu
    # TARGET=aarch64-unknown-linux-gnu
    # TARGET=armv7-unknown-linux-gnueabihf

    apt-get update && apt-get install -y --no-install-recommends ca-certificates git
    git clone -b vVERSION https://github.com/Tom-Neverwinter/xara-streisand && cd xara-streisand
    scripts/release_deps.sh $TARGET
    . ~/.cargo/env
    scripts/release_bins.sh $TARGET
    scripts/release_tarball.sh HEAD
    sha256sum target/dist/*.tar.xz

## options

    --config FILE           read key = value options from FILE

    --call CALL             your callsign (required)

    --audio BACKEND         pulse, alsa, coreaudio, winmm or trusdx; default pulse on linux/bsd,
                            coreaudio on macOS, winmm on Windows
    --list-devices          list capture and playback devices for --audio
    --capture SOURCE        audio source
    --playback SINK         audio sink
    --tx-buffer-ms N        TX audio queue, raise on underruns (30-500, default 40)

    --hf 500|2300|2750      HF mode, bandwidth setting
    --fm narrow|wide        FM mode, narrow vs wide

    --radio                 enable radio mode
    --drive-db D            TX drive (<= 0 dB, default -5)
    --ptt [see section]     PTT has a lot of options

    --ui-port N             serve web UI on port N (default off)
    --ui-addr A             webui listen address (default 127.0.0.1)
    --ui-host NAME          accept as HTTP Host for UI (repeatable)
    --ui-token T            UI token (default: random)
    --ui-control            DANGER: remote token-holders can key radio
    --state-dir DIR         web UI settings storage (default: set by systemd)

    --addr A                KISS/command address (default: 127.0.0.1)
    --port N                command port (< 65535, default: 8300)
    --kiss-port N|off       KISS TCP port (default: 8100)
    --persistence P         KISS p-persistence

    --unregistered          cap the link at unregistered VARA speeds (default: full speed)

    --digi CALL             FM digipeat for CALL (default: off)
    --record PATH           record rx, tx and PTT to FLAC (or WAV if .wav)
    --log FILE              append UTC-timestamped output to FILE
    --quiet                 only output warnings and errors
    --version and --help    guess

## PTT settings

we support the following PTT methods:

- rts and dtr on a serial port
- cm108/cm119 HID GPIO (Linux, macOS, FreeBSD; not yet on Windows)
- hamlib rigctld CAT
- vox (not recommended)

you can adjust the following PTT settings as well:

- PTT lead time
- PTT tail time
- max PTT

summary of settings:

    --ptt vox                   use VOX (just don't bother)
    --ptt rts:DEV               key serial port on rts, hold dtr low (DEV: /dev/ttyUSB0, COM3)
    --ptt dtr:DEV               key serial port on dtr, hold rts low
    --ptt cm108:DEV[:GPIO]      cm108/cm119 HID GPIO (default GPIO: 3)
    --ptt cm108:VID:PID[:GPIO]  cm108/cm119 by hex USB id (stable across boots)
    --ptt rigctld[:HOST:PORT]   hamlib rigctld CAT on HOST:PORT

    --ptt-lead-ms N             PTT held for N ms before xmit (default: 0)
    --ptt-tail-ms N             PTT held for N ms after xmit (default: 100)
    --ptt-max-s N               release PTT after N sec max (1-120, default: 30)

## xara-monitor

`xara-monitor` listens to VARA HF and FM traffic between other stations and never
transmits. it is considerably more demanding than `xara` and probably won't run
on a pi.

    xara-monitor --audio alsa --capture plughw:CARD=Device,DEV=0

options:

    --wav FILE              load from a wav file
    --stdin RATE            s16le mono at RATE Hz

    --audio BACKEND         pulse, alsa, coreaudio or winmm; default coreaudio on macOS,
                            winmm on Windows, else pulse
    --capture SOURCE        audio source

    --hf 500|2300|2750      HF bandwidth (default 2300)
    --fm narrow|wide        monitor VARA FM: connects, CQ, and session text

    --call CALL             known call (repeatable)
    --calls FILE            list of known calls, one per line
    --no-blind              only attempt to resolve known calls

    --ui-port N             read-only web UI on port N (default: off)
    --ui-addr ADDR          web UI listen addr (default: 127.0.0.1)
    --ui-host NAME          accept as HTTP Host for UI (repeatable)

    --key-idle MIN          max target idle time (default: 30, 0 disables)
    --dump                  dump entire payloads
    --verbose               report unconfirmed blind detections
    --json                  print newline-delimited JSON records
    --log FILE              append UTC-timestamped output to FILE
    --version and --help    guess

## full speed

xara runs at full speed out of the box: every level of the bandwidth, plus
compression, with no license key and no special build. `--unregistered`
caps a link the way an unregistered VARA would (HF level 4, FM level 2, no
compression), e.g. to match a test setup. `--registered` is still accepted
and does nothing.

## copyright

copyright (c) 2026 x0rro and the xara-streisand contributors

released under the MIT license, see [LICENSE](LICENSE).

VARA is a product of EA5HVK. This project is not affiliated with or endorsed by its author.
