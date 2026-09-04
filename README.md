# galdeck

Linux support for the **Elgato Stream Deck module built into the Corsair
Galleon 100 SD keyboard** — a hardware framework you can build on, plus a
config-driven daemon and CLI that use it. No kernel module, no root
daemon, no official software required.

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

## Layers

galdeck is a **hardware framework** with a reference consumer on top. The
split is deliberate: the framework owns the device and nothing about your
workflow, so anyone can build their own experience on it.

```
   your project  ─ mappings, widgets, themes, animations
        │  uses as a library
   galdeck-hid   ─ Buttons · Lcd · Encoders · Canvas · events   ← the framework
        │  hidraw
   Galleon 100 SD Stream Deck module (1b1c:2b18)
```

| Piece | What it does |
|---|---|
| [`crates/galdeck-hid`](crates/galdeck-hid) | **The framework.** Component handles for each control, a drawing canvas, colors, fonts, an event stream, and the keepalive that keeps the module awake. Plus `examples/verify.rs`, the hardware checkout harness. |
| [`crates/galdeck-daemon`](crates/galdeck-daemon) | A reference consumer: TOML profiles with pages, key labels/icons/colors, shell actions, encoder bindings with ring turn feedback; auto-reconnect; control socket. |
| [`crates/galdeck-cli`](crates/galdeck-cli) | `galdeck` command: `detect`, `status`, `brightness`, `page`, `reload`, `ping`. |
| [`docs/protocol.md`](docs/protocol.md) | Independent protocol documentation (CC-BY 4.0). |
| [`udev/`](udev), [`systemd/`](systemd) | Scoped udev rule (uaccess, not world-writable) and a user service unit. |

### Using the framework

Each control is its own handle, borrowed from the open device:

| Control | Handle | Hardware |
|---|---|---|
| Keys | `Buttons` / `Button` | 12 keys, 3x4, each a 160x160 display |
| Info screen | `Lcd` | one 720x384 drawable region |
| Knobs | `Encoders` / `Encoder` / `Ring` | 2 push-click encoders, 4 addressable RGB LEDs each |

```rust
use galdeck_hid::{Align, Event, Galleon, Rgb, TextStyle};
use std::time::Duration;

let api = hidapi::HidApi::new()?;
let mut deck = Galleon::open(&api)?;
deck.set_brightness(70)?;

// A solid key is one cheap feature report; an image is a canvas upload.
deck.button(0)?.set_color(Rgb::from_hex("#1d3b53").unwrap())?;

let mut canvas = deck.button(1)?.canvas();      // blank 160x160
canvas.fill(Rgb::new(20, 20, 28));
canvas.draw_line((10, 150), (150, 10), Rgb::GREEN);
canvas.fill_circle((80, 60), 24, Rgb::RED);
if let Some(font) = galdeck_hid::Font::system() {
    canvas.draw_text("Ready", 80, 130, &TextStyle::new(&font, 28.0).align(Align::Center));
}
deck.button(1)?.draw(&canvas)?;

// Rings address segments clockwise from the top, whatever the hardware order.
deck.encoder(0)?.ring().set_level(0.5, Rgb::GREEN, Rgb::BLACK)?;

// Partial screen updates are much cheaper than full redraws.
deck.lcd().draw_at(20, 20, &canvas)?;

for event in deck.poll(Duration::from_secs(5))? {
    match event {
        Event::KeyDown(key) => println!("key {key} pressed"),
        Event::EncoderRotate(knob, delta) => println!("knob {knob} moved {delta}"),
        _ => {}
    }
}
```

The module only accepts drawing while in *software mode*, which it leaves
without a keepalive roughly twice a second. Every drawing call and `poll`
refreshes that for you; after a long gap, `take_mode_reentry()` tells you
the firmware reset its own state and your content needs redrawing. Run
`cargo doc -p galdeck-hid --open` for the full API.

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
that misbehaves are gold.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full guide,
[docs/device-recon.md](docs/device-recon.md) for capture recipes, and
[CHANGELOG.md](CHANGELOG.md) for what has changed.

## License

Code: [MIT](LICENSE). Protocol documentation ([docs/protocol.md](docs/protocol.md)): CC-BY 4.0.
