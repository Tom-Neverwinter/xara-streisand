---
title: XARA-MONITOR
section: 1
header: User Commands
date: 2026-10-31
---

# NAME

xara-monitor - passive VARA HF traffic monitor

# SYNOPSIS

**xara-monitor** (**--wav** *FILE* | **--stdin** *RATE* | [**--audio** *BACKEND*]
**--capture** *SOURCE*) [*options*]

# DESCRIPTION

**xara-monitor** decodes VARA HF sessions between other stations and prints
timestamped details about the observed packets.

The first data frame after a connect reply is the session header. A header
whose byte 6 is not zero, whose valid flag is clear or which sets unknown flag
bits is still shown and followed, with an **[UNUSUAL: ...]** note and its raw
bytes. If that frame is not a header at all, a **NO HEADER** line gives its
raw bytes.

# OPTIONS

**--wav** *FILE*

:   PCM 8/16/24/32-bit or float WAV at any rate

**--stdin** *RATE*

:   s16le mono at *RATE* Hz (1000-1000000) on stdin

**--audio** pulse|alsa|coreaudio

:   Backend for **--capture** (default coreaudio on macOS, else pulse, or alsa
    when pulse is not present)

**--capture** *SOURCE*

:   Pulse source, ALSA PCM or CoreAudio device, opened capture-only.

**--hf** 500|2300|2750

:   HF bandwidth (default: 2300)

**--call** *CALL*

:   Known call sign (repeatable)

**--calls** *FILE*

:   List of known calls, one per line.

**--no-blind**

:   Only attempt to resolve known calls

**--key-idle** *MIN*

:   Max target idle time (default: 30, 0 disables)

**--log** *FILE*
:   Append UTC-timestamped output to *FILE*.

**--dump**

:   Dump entire payloads

**--verbose**

:   Report unconfirmed blind detections

**--diag**

:   After each DATA line, print one frame line per received frame, decoded
    or not: the receiver's SN estimate, raw SN, AFC residual and decoder
    iterations. Shows whether a failed frame was weak or arrived strong
    and still failed to decode. Also appends the raw bytes of every
    session header to its HEADER line.

**--json**

:   Print newline-delimited JSON records.

**--ui-port** *N*

:   Serve the web UI (read-only) on port *N*, with bonus content at **/stream**.
    Disabled by default.

**--ui-addr** *ADDR*

:   Listen address for the web UI (default 127.0.0.1).

**--ui-host** *NAME*

:   Also accept *NAME* as HTTP Host for UI (repeatable)

**-h**, **--help**, **-V**, **--version**

:   Print the usage or the version and exit.

# EXIT STATUS

0 at the end of the input, on the first SIGINT or SIGTERM, or when standard output is
closed; 1 on an error; 2 on a bad command line; 130 on a second signal.

# SEE ALSO

**xara**(1), **sox**(1)

README.md in the xara source, https://github.com/x0rro/xara
