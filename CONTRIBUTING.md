# contributing

thanks for helping. xara-streisand keeps the open XARA modem alive after
upstream withdrew it. the goal is complete compatibility with the whole
VARA suite (HF, FM, SAT, and the Chat / Terminal apps' modem side) in one
free program that is easy to set up on any common OS. it is not an
everything-modem: features VARA doesn't have belong in your own repo.

## don't fork, copy

please don't use GitHub's Fork button. clone and push to your own repo
with a funny name (see the README), and keep `LICENSE`. to contribute,
open an issue here that links your branch. that issue is where review
happens. the README has the commands.

## nobody here transmits

developers test without a radio: test vectors, simulated links, and audio
routed over virtual cables (xara ↔ xara, or xara ↔ real VARA on the same
machine). on-air testing is done only by licensed operators, on their own
stations, under their own rules. never add code that keys a transmitter
without `--radio` and an explicit `--ptt`.

## what we take

- bug fixes, with a test that fails without the fix
- platform work: Windows, macOS, BSDs, ARM boards, Android/Termux, new
  audio backends, PTT methods
- interop reports: "client X against xara on OS Y: works, or fails like
  this", with logs (`--log`) and, if you can, a recording (`--record`) of
  **your own** test traffic. virtual-cable loopback reports are as welcome
  as on-air ones from licensed operators. say which kind it was.
- closing a suite gap: VARA FM interop, VARA SAT, end-to-end runs with
  VARA Chat / Terminal / VarAC / Pat (see the README's goal table)
- packaging, CI, docs, and tests

open an issue first for anything large, so nobody duplicates work.

## the one hard rule: clean sources only

**do not contribute anything derived from disassembling or decompiling
VARA**, and do not paste VARA's files (`.dat` tables, DLLs, installers) into
issues or PRs. everything here must come from:

- the MIT XARA code and its recorded test vectors,
- signals anyone may receive, or your own lab loopback,
- VARA's published documentation and its observable behaviour (TCP
  commands, audio in and out).

if you cannot say where a constant or a table came from, it does not go
in. this keeps the project safe for everyone who builds on it. PRs that
can't show clean provenance will be closed.

## getting started

    git clone https://github.com/Tom-Neverwinter/xara-streisand
    cd xara-streisand
    cargo test --workspace

no radio or sound card is needed: the test suite replays recorded vectors
(`vectors/`) and runs whole links over simulated audio. see
[BUILDING.md](BUILDING.md) for your OS's system packages.

### where things live

| crate | what | start reading at |
|---|---|---|
| `xara-phy` | DSP: FEC/turbo, OFDM/FSK/PSK modulation and receivers, HF and FM | `src/hf/`, `src/fm/` |
| `xara-arq` | sans-IO ARQ state machines, compression, AX.25/KISS, broadcasts | `src/hf/`, `src/fm/`, `src/session.rs` |
| `xara-host` | VARA command/data and KISS TCP servers | `src/server.rs`, `src/command.rs` |
| `xara-ui` | telemetry bus, web UI server, settings | `src/server.rs` |
| `xara` | the binaries: CLI, audio backends, PTT, station glue | `src/tnc_main.rs`, `src/audio/`, `src/ptt.rs` |
| `xara-testkit` | test-only helpers | |

### before you send a change

run what CI runs (Linux shown; drop the pulse/alsa lines on macOS and
Windows):

    cargo fmt --all --check
    rustfmt --edition 2024 --check crates/xara/src/tnc_main.rs   # main.rs include!s it
    cargo clippy --locked --workspace --all-targets -- -D warnings
    cargo clippy --locked -p xara --all-targets --no-default-features --features pulse -- -D warnings
    cargo clippy --locked -p xara --all-targets --no-default-features --features alsa -- -D warnings
    cargo test --locked --release --workspace

CI covers Linux (x86_64, arm64, armv7 under qemu), macOS, Windows, a
FreeBSD type-check and the MSRV build. if you can only test one OS, say so
in your issue.

### cross-platform code

- gate platform code with `#[cfg(unix)]`, `#[cfg(windows)]` or
  `#[cfg(target_os = "...")]` and keep the other platforms compiling.
  look at `crates/xara/src/audio/mod.rs` for the pattern.
- if a feature is missing on a platform, fail with a clear message that
  says what to use instead (see `--ptt cm108` on Windows). don't fail
  silently.
- no new dependencies for a few lines of code. the workspace keeps its
  dependency list short on purpose (see `Cargo.toml`).

## style

- comments say *why*, not what. one line where one line does.
- commit messages: a short summary line, then a body only if it helps.
- AI-assisted contributions are fine if a human has read, tested and
  stands behind every line. no slop: no florid docs, no novel-length
  comments or commit messages.

## credit

xara was written by x0rro and released under the MIT license. this fork
keeps that license and that credit. by contributing you agree your work
is released under the same MIT license.
