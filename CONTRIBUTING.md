# Contributing to galdeck

## The most useful contribution: hardware reports

This project is only as good as the hardware it has been tested on, and
that is currently **one unit on firmware 3.05.003**. If you own a Galleon
100 SD, running the verification harness and reporting the result is
genuinely valuable — especially on a different firmware.

```sh
cargo run -p galdeck-cli -- detect          # read-only: firmware + serial
cargo run -p galdeck-hid --example verify   # full checkout, ~1 minute
```

Open an issue with your firmware version, distro, kernel, and what did or
did not happen. Two firmware quirks are already known and handled (see the
firmware matrix in [docs/protocol.md](docs/protocol.md)); newer firmware
reportedly changes the keepalive, and nobody has characterized that yet.

**Do not update your module firmware** to test — there is no known
downgrade path, and an untested firmware may leave you unable to use the
module at all.

If something misbehaves, a USB capture is gold: see
[docs/device-recon.md](docs/device-recon.md) for the usbmon and
Windows-VM-passthrough recipes, and commit traces under `captures/`.

## Code

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three must pass; CI runs them plus an MSRV check.

Guidelines:

- **Keep the layers apart.** `galdeck-hid` is a hardware framework: it
  owns the device, drawing primitives, and events. Key mappings, widgets,
  themes, and animations belong to consumers — `galdeck-daemon` is one, and
  it should not need framework changes to add a feature that is really
  about user experience.
- **Protocol claims need evidence.** A capture, a reference
  implementation, or behavior you observed on hardware — say which, and
  say which firmware. Facts that only hold on one firmware belong in the
  matrix, not as unconditional code.
- **Test what can be tested without hardware.** Report builders, parsers,
  canvas geometry, and config validation are all pure; hardware-dependent
  behavior goes in the verify harness instead.
- **Mind the firmware's quirks** documented in `docs/protocol.md` —
  especially feature-report pacing and the software-mode entry transition.
  Both were found the hard way.

## Protocol documentation

[docs/protocol.md](docs/protocol.md) is CC-BY 4.0 rather than MIT so any
project can absorb it, including GPL ones. It was written independently
from reference implementations and their documentation; please keep it
that way and do not paste in text from GPL-licensed sources.

## Licensing

Code is MIT. By contributing you agree your contribution is licensed the
same way.
