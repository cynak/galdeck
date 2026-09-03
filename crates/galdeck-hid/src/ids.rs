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
/// this interval. (The exact device-side timeout is uncharacterized.)
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_millis(500);
/// The device needs a moment after open before it accepts traffic.
pub const SETTLE_DELAY: Duration = Duration::from_millis(200);

/// Feature reports are 32 bytes including the report id, zero padded.
pub const FEATURE_REPORT_LEN: usize = 32;
/// Output (image) reports are 1024 bytes including the report id.
pub const OUTPUT_REPORT_LEN: usize = 1024;
/// Input reports arrive on a 512-byte endpoint.
pub const INPUT_REPORT_LEN: usize = 512;

/// Firmware version everything public has been validated against.
pub const VALIDATED_FIRMWARE: &str = "3.06.005";
