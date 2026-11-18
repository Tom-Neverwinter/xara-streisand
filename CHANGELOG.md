# Changelog

## Unreleased (xara-streisand)

maintained fork of x0rro/xara after upstream was withdrawn.

- Windows: the TNC runs natively (WinMM audio; serial RTS/DTR, rigctld and VOX PTT; console Ctrl-C), and `xara-monitor` plus its web UI build and run there
- `xara-monitor --fm narrow|wide`: passive VARA FM decode (connects, CQ, session text), also in the web UI
- host ports close connections that open with an HTTP request line, so a web page can't drive the TNC
- CI adds a Windows job
- full speed by default; `--unregistered` caps the link like unregistered VARA
- HF stations set to 2300 / 2750 now answer 500 Hz callers, at 500 Hz
- `CWID ON/OFF` takes effect (US calls always ID, as before)
- `MISSING SOUNDCARD` is sent to the client when the audio device fails
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

