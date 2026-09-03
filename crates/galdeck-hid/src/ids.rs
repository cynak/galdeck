//! Device constants for the Galleon 100 SD's Stream Deck module.

use std::time::Duration;

/// Corsair's USB vendor id. The module does NOT use Elgato's (0x0fd9).
pub const VENDOR_ID: u16 = 0x1b1c;
/// The Stream Deck module behind the keyboard's internal hub.
pub const PRODUCT_ID: u16 = 0x2b18;
/// The vendor HID data interface. Other interfaces on 2b18 belong to the
/// keyboard stack; opening them by accident can grab key input.
pub const CONTROL_INTERFACE: i32 = 0;

/// LCD keys, indexed row-major: 3 columns x 4 rows.
pub const KEY_COUNT: u8 = 12;
pub const KEY_COLUMNS: u8 = 3;
pub const KEY_ROWS: u8 = 4;
/// Native pixel size of one key; key images are JPEG at exactly this size,
/// no rotation or mirroring.
pub const KEY_PIXELS: u32 = 160;

pub const ENCODER_COUNT: u8 = 2;
/// Individually addressable RGB LEDs around each encoder.
pub const ENCODER_RING_LEDS: u8 = 4;

/// Host-addressable info-screen segment (the physical panel is larger, but
/// this is the region the protocol exposes for drawing).
pub const LCD_WIDTH: u16 = 720;
pub const LCD_HEIGHT: u16 = 384;

/// The module leaves software mode if it misses keepalives; senders use
/// this interval. (The exact device-side timeout is uncharacterized —
/// observed on firmware 3.05.003: a 12 s gap still left commands working,
/// but the module had dropped out of software mode somewhere within it.)
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_millis(500);
/// The device needs a moment after open before it accepts traffic.
pub const SETTLE_DELAY: Duration = Duration::from_millis(200);
/// Entering software mode (the first keepalive, or one after a long gap)
/// makes the firmware assert its own state — observed on 3.05.003 as all
/// encoder-ring LEDs turning white — wiping anything drawn during the
/// transition. Wait this long after a mode-entering keepalive before
/// drawing.
pub const SOFTWARE_MODE_ENTRY_SETTLE: Duration = Duration::from_millis(1000);
/// A keepalive gap longer than this is treated as having let the module
/// drop out of software mode (the true timeout is somewhere between the
/// 500 ms cadence and the 12 s observed above).
pub const SOFTWARE_MODE_REENTRY_GAP: Duration = Duration::from_millis(2000);
/// Minimum spacing between consecutive feature reports. Bursts of feature
/// reports misbehave (observed on firmware 3.05.003: rapid ring-LED writes
/// all take the final color); the reference implementation spaces them too.
pub const FEATURE_REPORT_SPACING: Duration = Duration::from_millis(2);

/// Feature reports are 32 bytes including the report id, zero padded.
pub const FEATURE_REPORT_LEN: usize = 32;
/// Output (image) reports are 1024 bytes including the report id.
pub const OUTPUT_REPORT_LEN: usize = 1024;
/// Input reports arrive on a 512-byte endpoint.
pub const INPUT_REPORT_LEN: usize = 512;

/// Firmware versions this protocol implementation is validated against:
/// 3.06.005 by the upstream reference implementations, 3.05.003 by this
/// project on physical hardware (2026-09-03).
pub const VALIDATED_FIRMWARES: &[&str] = &["3.05.003", "3.06.005"];
