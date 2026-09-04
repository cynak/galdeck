# Changelog

Notable changes to galdeck. Versions follow [semver](https://semver.org);
until 1.0 the framework API may change between minor versions.

## Unreleased

First public release, developed and validated against a physical Corsair
Galleon 100 SD on firmware 3.05.003.

### Framework (`galdeck`)

- Per-control handles: `Buttons`/`Button` (12 keys by index or grid
  position), `Lcd` (720x384 screen, full or partial region draws), and
  `Encoders`/`Encoder`/`Ring` (knobs and LED rings addressed clockwise from
  the top, with a `set_level` readout).
- `Canvas` drawing surface: lines with thickness, rectangles, circles,
  blitting, scaling, image loading, JPEG encoding.
- `Rgb` colors (hex and HSV construction, scaling, blending) and
  `Font`/`TextStyle` text rendering with system font discovery,
  measurement, and shrink-to-fit.
- Automatic keepalive: every drawing call and `poll` holds the module in
  software mode; `take_mode_reentry` reports when the firmware reset its
  own state and content needs redrawing.
- Public `protocol` module of pure report builders and parsers, for
  porting to other languages or transports.

### Not in this crate

The daemon, CLI, and their config live in
[galdeck-daemon](https://github.com/cynak/galdeck-daemon): this repository
is the hardware framework, and user experience belongs to consumers of it.

### Firmware quirks handled

Discovered on 3.05.003 and documented in `docs/protocol.md`:

- Entering software mode makes the firmware assert its own state (ring
  LEDs turn white), wiping anything drawn during the transition.
- Bursts of feature reports are garbled; consecutive ones are spaced.
- Ring LED hardware indices run counter-clockwise, with a different start
  offset per ring.

### Known limitations

- The keyboard half (`1b1c:2b0c`) is out of scope; it types fine via the
  kernel's generic HID driver.
- Firmware newer than 3.06.005 reportedly changes the keepalive; nothing
  public implements that yet.
