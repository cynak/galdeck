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
/// Edge length the `02 07` key-image path accepts. The firmware places and
/// clips these images itself — the report carries no geometry — so this is
/// the size that path blits, not the size of the physical key.
///
/// Must stay a multiple of [`JPEG_MCU`]: observed on firmware 3.05.005,
/// sending 164 or 172 shears the image progressively down the rows rather
/// than rejecting it. Sizes below this render smaller; sizes above it add
/// no coverage.
pub const KEY_PIXELS: u32 = 160;

/// JPEG minimum coded unit. The encoder emits 4:4:4, so blocks are 8x8 and
/// any image dimension that is not a multiple of 8 risks shear. This is the
/// bound for the `02 07` key-image path.
pub const JPEG_MCU: u32 = 8;

/// Dimension granularity for `02 0c` region updates — stricter than
/// [`JPEG_MCU`].
///
/// Observed on firmware 3.05.005: a region 8 pixels wide renders only about
/// half its requested height, and a region whose height is a multiple of 8
/// but not 16 shears into diagonal streaks. Both are consistent with the
/// device rounding a region's stride up to 16, consuming two rows of image
/// data for every row it paints. Region width and height are rounded to
/// this, not to [`JPEG_MCU`].
pub const REGION_MCU: u32 = 16;

pub const ENCODER_COUNT: u8 = 2;
/// Individually addressable RGB LEDs around each encoder.
pub const ENCODER_RING_LEDS: u8 = 4;

/// The info-screen segment: the region [`Lcd`](crate::controls::Lcd) draws
/// to. This is a *contract*, not the limit of what the hardware accepts —
/// see [`PANEL_WIDTH`] / [`PANEL_HEIGHT`] for the addressable extent.
pub const LCD_WIDTH: u16 = 720;
pub const LCD_HEIGHT: u16 = 384;

/// Full addressable extent of the `02 0c` region path.
///
/// The module has one physical portrait display: the info screen on top and
/// the 12 keys below it. Observed on firmware 3.05.005 — a region update at
/// y=384, past the [`LCD_HEIGHT`] the library used to enforce, rendered on
/// the key area, and a ruler spanning y=256..448 ran continuously from the
/// info screen onto the top key row.
pub const PANEL_WIDTH: u16 = 720;
/// NOT MEASURED: reported panel height, not yet confirmed by drawing at the
/// bottom of the panel. Nothing below y=448 has been written successfully.
pub const PANEL_HEIGHT: u16 = 1280;

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

/// JPEG quality used when the framework encodes a canvas for the device.
pub const DEFAULT_JPEG_QUALITY: u8 = 90;

/// Firmware versions this protocol implementation is validated against:
/// 3.06.005 by the upstream reference implementations, 3.05.003 and
/// 3.05.005 by this project on physical hardware (2026-09-03, 2026-09-05).
pub const VALIDATED_FIRMWARES: &[&str] = &["3.05.003", "3.05.005", "3.06.005"];
