# galdeck-hid

A Linux hardware framework for the Elgato Stream Deck module built into the
**Corsair Galleon 100 SD** keyboard (USB `1b1c:2b18`).

This crate owns the hardware layer and nothing above it: talking to the
device, holding it in software mode, drawing pixels, lighting LEDs, and
turning HID reports into events. What to draw — key mappings, widgets,
themes, animations — is left to you.

Userspace only: no kernel module, no root, no vendor software. It talks to
`hidraw` and needs a udev rule granting your session access to the device.

## Controls

| Control | Handle | Hardware |
|---|---|---|
| Keys | `Buttons` / `Button` | 12 keys, 3x4, each a 160x160 display |
| Info screen | `Lcd` | one 720x384 drawable region |
| Knobs | `Encoders` / `Encoder` / `Ring` | 2 push-click encoders, 4 addressable RGB LEDs each |

```rust
use galdeck_hid::{Align, Event, Galleon, Rgb, TextStyle};
use std::time::Duration;

let api = galdeck_hid::hidapi::HidApi::new()?;
let mut deck = Galleon::open(&api)?;
deck.set_brightness(70)?;

// A solid key costs one feature report; an image is a canvas upload.
deck.button(0)?.set_color(Rgb::from_hex("#1d3b53").unwrap())?;

let mut canvas = deck.button(1)?.canvas();   // blank 160x160
canvas.fill(Rgb::new(20, 20, 28));
canvas.draw_line((10, 150), (150, 10), Rgb::GREEN);
canvas.fill_circle((80, 60), 24, Rgb::RED);
if let Some(font) = galdeck_hid::Font::system() {
    canvas.draw_text("Ready", 80, 130, &TextStyle::new(&font, 28.0).align(Align::Center));
}
deck.button(1)?.draw(&canvas)?;

// Ring segments are numbered clockwise from the top, whatever the
// hardware's internal order.
deck.encoder(0)?.ring().set_level(0.5, Rgb::GREEN, Rgb::BLACK)?;

for event in deck.poll(Duration::from_secs(5))? {
    match event {
        Event::KeyDown(key) => println!("key {key} pressed"),
        Event::EncoderRotate(knob, delta) => println!("knob {knob} moved {delta}"),
        _ => {}
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Things worth knowing

- **Software mode.** The module ignores drawing unless it receives a
  keepalive roughly twice a second; that is why it looks dead without a
  driver. Every drawing call and `poll` refreshes it for you. After a long
  gap the firmware resets its own state — `take_mode_reentry()` tells you
  to redraw.
- **Partial screen updates.** `Lcd::draw_at` uploads only the rectangle you
  changed and is far cheaper than a full redraw.
- **Feature-report pacing.** The firmware garbles bursts of feature
  reports, so the framework spaces them; a solid key or LED write is still
  much cheaper than an image.
- **`encode` feature** (default): canvas JPEG encoding and image loading.
  Without it the framework still drives LEDs and uploads pre-encoded JPEGs.

## Device access

```sh
sudo cp udev/70-galdeck.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

## Protocol

The wire format is documented in [`docs/protocol.md`](../../docs/protocol.md)
(CC-BY 4.0) and implemented in the public `protocol` module as pure
functions over byte buffers — useful if you are porting to another language
or transport. Hardware-validated on firmware 3.05.003; see
`ids::VALIDATED_FIRMWARES`.

Part of the [galdeck](https://github.com/cynak/galdeck) project. MIT licensed.
