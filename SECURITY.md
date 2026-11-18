# Security Policy

XARA is a VARA-compatible HF/FM modem. It parses untrusted data off the air and
exposes local network services (the KISS/TCP TNC and the read-only web UI), so
we take security reports seriously and welcome them.

## Reporting a vulnerability

**Please report privately — do not open a public issue for a security bug.**

Use GitHub's private vulnerability reporting:

1. Go to the repository's **Security** tab → **Report a vulnerability**
   (or: <https://github.com/Tom-Neverwinter/xara-streisand/security/advisories/new>).
2. GitHub opens a private advisory visible only to you and the maintainers.

This keeps the report confidential while a fix is prepared, and no email address
is needed.

Please include, as far as you can:

- the affected component (modem/PHY, ARQ, the KISS/TCP TNC, the web UI, a build
  target) and version or commit,
- what an attacker can do, and the impact,
- steps or a proof of concept to reproduce (a capture/WAV, a frame, a request),
- any suggested fix.

You will normally get an acknowledgement within a few days. We aim to confirm or
dismiss a report and agree a disclosure timeline with you; please allow time for
a fix before any public disclosure (coordinated disclosure).

## Scope

In scope — the code in this repository:

- the receiver and demodulator parsing untrusted RF frames (`xara-phy`,
  `xara-arq`): memory safety, panics/DoS, decoder mistakes that mis-handle
  crafted input;
- the **KISS/TCP TNC** server and host interface (`xara-host`): anything a
  client on the TNC socket can abuse;
- the **web UI** server (`xara-ui`): the HTTP/WebSocket/NDJSON endpoints, access
  control, and how untrusted input is handled;
- the command-line tools and their handling of untrusted files (WAV, IQ).

Out of scope here:

- the proprietary VARA software (not ours — report to its author);
- vulnerabilities in third-party dependencies (report upstream; we will bump the
  dependency once there is a fix — see Dependabot below);
- operating a radio unlawfully, or interference — that is an operator and
  regulatory matter, not a software vulnerability.

## Supported versions

Security fixes land on the default branch (`main`) and the latest release. Older
releases are not maintained; please reproduce against current `main` where you
can.

## Dependencies

Dependency security updates are tracked automatically with
[Dependabot](.github/dependabot.yml) (the Cargo and GitHub Actions ecosystems).
Alerts and update PRs appear in the repository; you do not need to report a known
advisory in a dependency through the private channel above.
