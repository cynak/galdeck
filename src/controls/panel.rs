//! The whole physical display, beneath the info-screen abstraction.

use crate::canvas::Canvas;
use crate::device::Galleon;
use crate::error::Error;
use crate::ids::{PANEL_HEIGHT, PANEL_WIDTH, REGION_MCU};
#[cfg(feature = "encode")]
use crate::Rgb;

/// Handle to the raw panel. Obtained from [`Galleon::panel`].
///
/// The module has one portrait display: the info screen on top, the 12 keys
/// below. [`Lcd`](crate::controls::Lcd) draws to the documented segment;
/// this handle addresses all of it, including the key area.
///
/// Nothing here knows where the keys are. That geometry depends on how the
/// panel sits behind the keyboard's bezel and is measured per unit — see
/// `examples/calibrate.rs`.
pub struct Panel<'a> {
    deck: &'a mut Galleon,
}

impl<'a> Panel<'a> {
    /// Addressable width in pixels.
    pub const WIDTH: u16 = PANEL_WIDTH;
    /// Addressable height in pixels.
    pub const HEIGHT: u16 = PANEL_HEIGHT;

    pub(crate) fn new(deck: &'a mut Galleon) -> Self {
        Panel { deck }
    }

    /// Addressable size as `(width, height)`.
    pub const fn size() -> (u16, u16) {
        (PANEL_WIDTH, PANEL_HEIGHT)
    }

    /// Draw an already-encoded JPEG into a rectangle. `width` and `height`
    /// must match the JPEG's real dimensions, and both should be multiples
    /// of [`REGION_MCU`] — a region 8 wide renders half its height, and an
    /// off-block height shears into diagonal streaks.
    pub fn draw_jpeg_at(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        jpeg: &[u8],
    ) -> Result<(), Error> {
        self.deck.send_panel_region(x, y, width, height, jpeg)
    }

    /// Draw a canvas with its top-left at `(x, y)`.
    #[cfg(feature = "encode")]
    pub fn draw_at(&mut self, x: u16, y: u16, canvas: &Canvas) -> Result<(), Error> {
        let jpeg = canvas.to_jpeg(crate::ids::DEFAULT_JPEG_QUALITY)?;
        self.draw_jpeg_at(x, y, canvas.width() as u16, canvas.height() as u16, &jpeg)
    }

    /// Flood a rectangle with one color.
    ///
    /// Solid colors compress to almost nothing, so this is far cheaper on
    /// the wire than its pixel count suggests — a 720x8 rule is a single
    /// output report.
    #[cfg(feature = "encode")]
    pub fn fill_rect(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        color: Rgb,
    ) -> Result<(), Error> {
        let canvas = Canvas::filled(width as u32, height as u32, color);
        self.draw_at(x, y, &canvas)
    }

    /// Round a dimension **down** to a whole [`REGION_MCU`] block: the
    /// largest size at or below `size` that the firmware will not shear.
    ///
    /// Use this to size a canvas that must fit inside a space. To *cover* a
    /// space, round up instead — truncating a drawn rect strands up to 7px
    /// of unpainted panel at its far edge, which is a visible sliver, not a
    /// rounding detail. See [`Rect::to_mcu_covering`](crate::layout::Rect::to_mcu_covering).
    pub const fn to_mcu_floor(size: u16) -> u16 {
        size - size % REGION_MCU as u16
    }
}
