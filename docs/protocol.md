# Corsair Galleon 100 SD — Stream Deck module protocol

Independently written protocol documentation for the Elgato Stream Deck
module built into the Corsair Galleon 100 SD keyboard.

License: **CC-BY 4.0** — reuse freely in any project (GPL, MIT, proprietary)
with attribution to "the galdeck project".

Provenance: written from the behavior of the MIT-licensed reference
implementation in [Julusian/node-elgato-stream-deck] (`galleon-k100` model,
merged January 2026), the official [Elgato Stream Deck HID docs] (Gen2 Main
Protocol), community protocol notes from the opendeck-galleon project, and
the rust-elgato-streamdeck PR #63 test suite. No text was copied from
GPL/MPL sources; byte layouts are facts.

[Julusian/node-elgato-stream-deck]: https://github.com/Julusian/node-elgato-stream-deck
[Elgato Stream Deck HID docs]: https://docs.elgato.com/streamdeck/hid/

## Firmware validation matrix

| Firmware | Status |
|---|---|
| 3.06.005 | Everything below validated (multiple implementations) |
| ≥ 3.06.006 | **Unverified** report that the keepalive changes to a `02 25`-prefixed report; nothing public implements it. If you have this firmware, please report your findings. |

**Recommendation: do not update the module firmware until support for your
current version is confirmed working.**

## USB topology

The keyboard contains an internal USB 2.0 hub; the "one keyboard" is three
USB devices:

| Device | VID:PID | Notes |
|---|---|---|
| Internal hub | `1b1c:2b19` | Product string reuses a K100 MAX name |
| Keyboard | `1b1c:2b0c` | Standard HID keyboard + Corsair vendor config channel; out of scope here |
| **Stream Deck module** | **`1b1c:2b18`** | Product string "K100 MK2"; 4 HID interfaces |

Note the module uses **Corsair's vendor id 0x1b1c**, not Elgato's 0x0fd9 —
stock Stream Deck software and libraries do not detect it for that reason
alone. The protocol on interface 0 is otherwise the Elgato **Gen2 Main
Protocol** plus the deltas documented below.

**Open interface 0 only.** Interrupt IN endpoint 0x81 and OUT endpoint 0x09,
512-byte max packet size (transparent at the HID report layer). The other
interfaces carry keyboard/consumer-control/lighting functions; matching by
VID/PID alone risks grabbing keyboard input.

## Module geometry

- **12 LCD keys**, 3 columns × 4 rows, indexed row-major 0–11 from top-left.
  Key images are **160×160 JPEG**, no rotation, no mirroring.
- **2 push-click rotary encoders** (0 = left, 1 = right), each with
  **4 individually addressable RGB ring LEDs**.
- **One host-addressable LCD segment, 720×384**, drawn via rectangular JPEG
  region updates. (The physical panel is a larger 720×1280 portrait display;
  only this segment is exposed for drawing.) The protocol defines touch
  events for it (see Input), though the shipped panel is not touch-operated.

## Report inventory

| Direction | Report id | Size (incl. id) | Use |
|---|---|---|---|
| IN (interrupt) | `0x01` | up to 512 | input events |
| OUT (interrupt) | `0x02` | 1024 | image data |
| FEATURE (set) | `0x03` | 32, zero-padded | commands |
| FEATURE (get) | `0x05` | 32 | firmware version |
| FEATURE (get) | `0x06` | 32 | serial number |

## Software mode and the keepalive (Corsair delta 1)

Without host traffic the module sits in *hardware mode* (inert numpad
imagery, no input reports). Sending feature report

```
03 27 00 ... 00        (32 bytes)
```

switches it into *software mode* and must be **repeated every 500 ms**; if
keepalives stop, the module drops back to hardware mode. 500 ms is the
empirically safe interval used by all implementations — the device-side
timeout has not been characterized.

After opening the device, wait ~200 ms before the first traffic.

## Feature commands (report id 0x03, 32 bytes, zero-padded)

| Bytes | Meaning |
|---|---|
| `03 27` | keepalive / enter software mode |
| `03 08 pp` | set panel brightness, `pp` = 0–100 |
| `03 02` | reset to logo screen (leaves software mode imagery) |
| `03 06 kk rr gg bb` | fill key `kk` (0–11) with a solid color |
| `03 24 ii rr gg bb` | set encoder ring LED pixel `ii` (0–7) — see below |

### Encoder ring LEDs (Corsair delta 3)

The 8 ring LEDs live in one index space: **encoder 0 (left) owns pixels
4–7, encoder 1 (right) owns pixels 0–3** — i.e. `index = (1 − encoder) × 4 +
hardware_segment`. Within each ring the hardware order is rotated relative
to the visual ring; hardware segment `h` appears at visual position
`(h + rotation) mod 4` with `rotation = 3` for encoder 0 and `1` for
encoder 1 (visual position 0 = top, clockwise).

## Image uploads (output report 0x02, 1024 bytes)

JPEG payloads are split into chunks carried in fixed 1024-byte reports,
zero-padded after the payload. Multi-byte fields are little-endian.

### Key image — command 0x07

8-byte header, 1016-byte max payload per report:

| Offset | Field |
|---|---|
| 0 | `0x02` report id |
| 1 | `0x07` command |
| 2 | key index 0–11 |
| 3 | 1 if this is the last chunk, else 0 |
| 4–5 | payload byte count (u16 LE) |
| 6–7 | chunk index from 0 (u16 LE) |
| 8… | JPEG chunk |

### LCD segment region — command 0x0c

16-byte header, 1008-byte max payload per report:

| Offset | Field |
|---|---|
| 0 | `0x02` report id |
| 1 | `0x0c` command |
| 2–3 | x (u16 LE) |
| 4–5 | y (u16 LE) |
| 6–7 | width (u16 LE) |
| 8–9 | height (u16 LE) |
| 10 | 1 if last chunk, else 0 |
| 11–12 | chunk index from 0 (u16 LE) |
| 13–14 | payload byte count (u16 LE) |
| 15 | padding |
| 16… | JPEG chunk |

The JPEG must decode to exactly width × height; the rectangle must fit in
720×384. Other Gen2 image commands (`08`, `09`, `0b`) exist in the Elgato
protocol family and have been observed on the wire, but are not needed to
drive this module and are not documented here.

## Input (input report 0x01)

Byte 0 is the report id `0x01`; byte 1 selects the event type.

### Type 0x00 — keys

Pressed-state snapshot: key `k` (0–11) is pressed iff byte `4 + k` is
non-zero.

### Type 0x02 — LCD touch

Byte 4 subtype: 1 = short press, 2 = long press, 3 = swipe. Coordinates
u16 LE: x at bytes 6–7, y at 8–9; for swipes the end point x at 10–11, y at
12–13. (Defined by the protocol family; the shipped panel is not
touch-operated, so these may never fire.)

### Type 0x03 — encoders

Byte 4 subtype:

- `0x00` press states: encoder `e` pressed iff byte `5 + e` non-zero.
- `0x01` rotation: signed i8 delta per encoder at byte `5 + e`; positive =
  clockwise. Fast turns coalesce into larger deltas.

### Type 0x04 — NFC

Defined by the Gen2 protocol family; the Galleon has no NFC reader.

## Getters

- **Firmware version**: get feature report `0x05` (32 bytes). Byte 1 is a
  length `n`; bytes 2–5 are a checksum; the ASCII version string occupies
  bytes 6 to `n + 2`.
- **Serial number**: get feature report `0x06` (32 bytes). Byte 1 is a
  length `n`; the ASCII serial occupies bytes 2 to `n + 2`.

## Uncharacterized

- Exact keepalive timeout (how many missed intervals before hardware mode).
- The alleged `02 25` keepalive on firmware ≥ 3.06.006.
- Behavior and framing of image commands `08`, `09`, `0b` on this module.
- Roles of interfaces 1–3 on `2b18` beyond keyboard/consumer/lighting
  descriptors.
- Whether the touchscreen event types can fire on shipped hardware.
