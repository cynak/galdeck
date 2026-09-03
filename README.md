# galdeck

Linux support for the **Elgato Stream Deck module built into the Corsair
Galleon 100 SD keyboard** — a userspace driver, a config-driven daemon, and
a CLI. No kernel module, no root daemon, no official software required.

On Windows/macOS the module is driven by Elgato's Stream Deck app; on Linux
it sits inert in "hardware mode". galdeck implements the host side of the
protocol: it holds the module in software mode (a 500 ms keepalive is
required — this is why the module appears dead without a driver), renders
your configured pages onto the 12 LCD keys and the info screen, lights the
encoder rings, and runs your commands on key presses and encoder turns.

**Status: verified on real hardware.** All protocol surfaces — key JPEG
uploads, LCD region drawing, encoder ring LEDs, brightness, input events,
and the keepalive — pass the [verify harness](docs/device-recon.md) on a
physical Galleon 100 SD running firmware 3.05.003 (2026-09-03; 3.06.005
is validated upstream). Two firmware quirks were discovered and are
handled by the driver: see the
[firmware matrix](docs/protocol.md#firmware-validation-matrix). **Don't
update your module firmware** — newer firmware allegedly changes the
keepalive and nothing public implements it yet.

Not affiliated with or endorsed by Corsair or Elgato. "Stream Deck" and
"Galleon" are their trademarks.

## What's in the box

| Piece | What it does |
|---|---|
| [`crates/galdeck-hid`](crates/galdeck-hid) | Driver library: device discovery (`1b1c:2b18` interface 0), keepalive, key/LCD JPEG uploads, encoder ring LEDs, input events. Plus `examples/verify.rs`, the hardware checkout harness. |
| [`crates/galdeck-daemon`](crates/galdeck-daemon) | User daemon: TOML profiles with pages, key labels/icons/colors, shell actions, encoder bindings; auto-reconnect; control socket. |
| [`crates/galdeck-cli`](crates/galdeck-cli) | `galdeck` command: `detect`, `status`, `brightness`, `page`, `reload`, `ping`. |
| [`docs/protocol.md`](docs/protocol.md) | Independent protocol documentation (CC-BY 4.0). |
| [`udev/`](udev), [`systemd/`](systemd) | Scoped udev rule (uaccess, not world-writable) and a user service unit. |

## Quick start

```sh
# 1. build (needs libudev headers: apt install libudev-dev / pacman -S systemd)
cargo build --release

# 2. device access
sudo cp udev/70-galdeck.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
# replug the keyboard

# 3. first contact — read-only, prints firmware + serial
cargo run -p galdeck-cli -- detect

# 4. full protocol checkout on real hardware (see docs/device-recon.md first)
cargo run -p galdeck-hid --example verify

# 5. daily use
mkdir -p ~/.config/galdeck && cp config/galdeck.example.toml ~/.config/galdeck/config.toml
cargo install --path crates/galdeck-daemon
cargo install --path crates/galdeck-cli
mkdir -p ~/.config/systemd/user && cp systemd/galdeck.service ~/.config/systemd/user/
systemctl --user daemon-reload && systemctl --user enable --now galdeck
galdeck status
```

Configuration lives in `~/.config/galdeck/config.toml` — see the commented
[example](config/galdeck.example.toml). `galdeck reload` applies edits live.

Notes on running as a service:

- Apps you launch from a key are children of the daemon, so the unit sets
  `KillMode=process`; without it, `systemctl --user restart galdeck` would
  close the windows you opened from the deck. Their memory still counts
  toward the service in `systemctl status` — cosmetic, not a leak.
- GUI actions need the systemd user manager to know your graphical
  session (`systemctl --user show-environment` should list `WAYLAND_DISPLAY`
  or `DISPLAY`). GNOME and KDE do this for you.
- Logs: `journalctl --user -u galdeck -f`. Set `RUST_LOG=debug` in the unit
  for per-event tracing.

## Device background

The Galleon 100 SD (CES 2026) replaces the numpad with a genuine Stream
Deck: 12 LCD keys (160×160), two push-click encoders with RGB rings, and a
720×384 host-drawable info screen. It enumerates behind an internal hub as
`1b1c:2b18` — **Corsair's** vendor id, not Elgato's — which is the sole
reason stock Stream Deck tooling ignores it. The protocol is Elgato's
documented Gen2 protocol plus three Corsair deltas (keepalive, interface
selection, ring LEDs); all details in [docs/protocol.md](docs/protocol.md).

The keyboard half (`1b1c:2b0c`) types fine out of the box via the kernel's
generic HID driver and can be configured with Corsair's browser-based Web
Hub (WebHID; see the commented-out rule in `udev/`). Native keyboard
RGB/profile support is out of scope for now — see ckb-next issue #1264.

## Relationship to other projects

The protocol groundwork exists thanks to
[Julusian/node-elgato-stream-deck] (MIT; first shipped Galleon support —
the reference this library was ported from), Elgato's public Gen2 HID
docs, and early community work (opendeck-galleon's notes,
rust-elgato-streamdeck PR #63). galdeck exists to provide what none of
those do on Linux: a daemon and tooling you can daily-drive, plus
independently rewritten protocol docs under CC-BY that any project — GPL,
MIT, or otherwise — can absorb. If upstream Stream Deck libraries grow
Galleon support, great: this repo's protocol docs, captures, and
verification harness still serve as the hardware-facts commons.

[Julusian/node-elgato-stream-deck]: https://github.com/Julusian/node-elgato-stream-deck

## Contributing

Hardware reports are the most valuable contribution right now: run
`cargo run -p galdeck-hid --example verify` and open an issue with your
firmware version and the result — especially on firmware newer than
3.06.005, where the keepalive may differ. Traffic captures for anything
that misbehaves are gold; see [docs/device-recon.md](docs/device-recon.md).

Code: `cargo fmt`, `cargo clippy --workspace --all-targets`, `cargo test
--workspace` must pass. Protocol claims need a source (a capture, a
reference implementation, or hardware behavior you observed).

## License

Code: [MIT](LICENSE). Protocol documentation ([docs/protocol.md](docs/protocol.md)): CC-BY 4.0.
