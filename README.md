# galdeck

A Linux **hardware framework** for the Elgato Stream Deck module built into
the **Corsair Galleon 100 SD** keyboard (USB `1b1c:2b18`).

On Windows and macOS the module is driven by Elgato's Stream Deck app; on
Linux it sits inert in "hardware mode", because it only accepts drawing
while a host keeps it awake. galdeck implements that host side and hands
you the controls: 12 LCD keys, a 720×384 info screen, and two push-click
encoders with RGB rings.

This crate owns the hardware layer and nothing above it. What to draw — key
mappings, widgets, themes, animations — belongs to whatever you build on
top. Userspace only: no kernel module, no root, no vendor software.

**Status: verified on real hardware.** Every control passes the
[verify harness](docs/device-recon.md) on a physical Galleon 100 SD running
firmware 3.05.003 (3.06.005 is validated upstream). Three firmware quirks
were found and are handled; see the
[firmware matrix](docs/protocol.md#firmware-validation-matrix). **Don't
update your module firmware** — newer firmware reportedly changes the
keepalive and nothing public implements it yet.

Not affiliated with or endorsed by Corsair or Elgato. "Stream Deck" and
"Galleon" are their trademarks.

## Controls

Open the device, then borrow the control you want:

| Control | Handle | Hardware |
|---|---|---|
| Keys | `Buttons` / `Button` | 12 keys, 3×4, each a 160×160 display |
| Info screen | `Lcd` | one 720×384 drawable region |
| Knobs | `Encoders` / `Encoder` / `Ring` | 2 push-click encoders, 4 addressable RGB LEDs each |

```rust
use galdeck::{Align, Event, Galleon, Rgb, TextStyle};
use std::time::Duration;

let api = galdeck::hidapi::HidApi::new()?;
let mut deck = Galleon::open(&api)?;
deck.set_brightness(70)?;

// A solid key costs one feature report; an image is a canvas upload.
deck.button(0)?.set_color(Rgb::from_hex("#1d3b53").unwrap())?;

let mut canvas = deck.button(1)?.canvas();   // blank 160x160
canvas.fill(Rgb::new(20, 20, 28));
canvas.draw_line((10, 150), (150, 10), Rgb::GREEN);
canvas.fill_circle((80, 60), 24, Rgb::RED);
if let Some(font) = galdeck::Font::system() {
    canvas.draw_text("Ready", 80, 130, &TextStyle::new(&font, 28.0).align(Align::Center));
}
deck.button(1)?.draw(&canvas)?;

// Ring segments are numbered clockwise from the top, whatever the
// hardware's internal order.
deck.encoder(0)?.ring().set_level(0.5, Rgb::GREEN, Rgb::BLACK)?;

// Partial screen updates are far cheaper than full redraws.
deck.lcd().draw_at(20, 20, &canvas)?;

for event in deck.poll(Duration::from_secs(5))? {
    match event {
        Event::KeyDown(key) => println!("key {key} pressed"),
        Event::EncoderRotate(knob, delta) => println!("knob {knob} moved {delta}"),
        _ => {}
    }
}
```

`cargo doc --open` has the full API.

## Getting started

```sh
# device access (build needs libudev headers: apt install libudev-dev)
sudo cp udev/70-galdeck.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
# replug the keyboard, then:

cargo run --example detect   # read-only: is it there, what firmware?
cargo run --example verify   # full checkout of every control, ~1 minute
```

Add it to your project with `galdeck = "0.1"`.

## Things worth knowing

- **Software mode.** The module ignores drawing unless it receives a
  keepalive roughly twice a second — that is why it looks dead without a
  driver. Every drawing call and `poll` refreshes it for you. After a long
  gap the firmware resets its own state; `take_mode_reentry()` tells you to
  redraw.
- **Partial screen updates.** `Lcd::draw_at` uploads only the rectangle you
  changed.
- **Feature-report pacing.** This firmware garbles bursts of feature
  reports, so the framework spaces them.
- **`encode` feature** (default): canvas JPEG encoding and image loading.
  Without it the framework still drives LEDs and uploads pre-encoded JPEGs.

## Built on galdeck

- **[galdeck-daemon](https://github.com/cynak/galdeck-daemon)** — the
  reference consumer: TOML profiles with pages, key labels, icons, shell
  actions, encoder bindings with ring feedback, and a control CLI. Worth a
  read if you are building your own.

Built something? Open a PR adding it here.

## Device background

The Galleon 100 SD (CES 2026) replaces the numpad with a genuine Stream
Deck. It enumerates behind an internal hub as `1b1c:2b18` — **Corsair's**
vendor id, not Elgato's — which is the sole reason stock Stream Deck
tooling ignores it. The protocol is Elgato's documented Gen2 protocol plus
three Corsair deltas (keepalive, interface selection, ring LEDs), all
written up in [docs/protocol.md](docs/protocol.md) under CC-BY 4.0 so any
project can absorb it. The `protocol` module implements it as pure
functions over byte buffers, useful if you are porting to another language
or transport.

The keyboard half (`1b1c:2b0c`) types fine out of the box via the kernel's
generic HID driver and can be configured with Corsair's browser-based Web
Hub (WebHID; see the commented-out rule in `udev/`). Native keyboard
RGB/profile support is out of scope — see ckb-next issue #1264.

## Relationship to other projects

The protocol groundwork exists thanks to
[Julusian/node-elgato-stream-deck] (MIT; first shipped Galleon support —
the reference this was ported from), Elgato's public Gen2 HID docs, and
early community work (opendeck-galleon's notes, rust-elgato-streamdeck
PR #63). galdeck adds what none of those do on Linux: a framework to build
on, independently rewritten protocol docs under CC-BY, and a hardware
verification anyone can rerun.

[Julusian/node-elgato-stream-deck]: https://github.com/Julusian/node-elgato-stream-deck

## Contributing

Hardware reports are the most valuable contribution right now — everything
rests on one unit. Run `cargo run --example verify` and open an issue with
your firmware version and result, especially on firmware newer than
3.06.005 where the keepalive may differ.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full guide,
[docs/device-recon.md](docs/device-recon.md) for USB capture recipes, and
[CHANGELOG.md](CHANGELOG.md) for what has changed.

## License

Code: [MIT](LICENSE). Protocol documentation
([docs/protocol.md](docs/protocol.md)): CC-BY 4.0.
