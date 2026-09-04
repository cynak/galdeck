//! The info screen: one host-drawable 720x384 region.

use crate::canvas::Canvas;
use crate::device::Galleon;
use crate::error::Error;
use crate::ids::{LCD_HEIGHT, LCD_WIDTH};
// Only the encode-gated fill/clear helpers name a color.
#[cfg(feature = "encode")]
use crate::Rgb;

/// Handle to the info screen. Obtained from [`Galleon::lcd`].
///
/// The screen accepts rectangular updates, so redrawing a small changed
/// area with [`Lcd::draw_at`] is much cheaper than a full-screen redraw.
pub struct Lcd<'a> {
    deck: &'a mut Galleon,
}

impl<'a> Lcd<'a> {
    /// Drawable width in pixels.
    pub const WIDTH: u16 = LCD_WIDTH;
    /// Drawable height in pixels.
    pub const HEIGHT: u16 = LCD_HEIGHT;

    pub(crate) fn new(deck: &'a mut Galleon) -> Self {
        Lcd { deck }
    }

    /// Drawable size as `(width, height)`.
    pub const fn size() -> (u16, u16) {
        (LCD_WIDTH, LCD_HEIGHT)
    }

    /// A blank canvas covering the whole screen, ready to draw into.
    pub fn canvas(&self) -> Canvas {
        Canvas::new(LCD_WIDTH as u32, LCD_HEIGHT as u32)
    }

    /// Replace the whole screen. The canvas must be exactly 720x384.
    #[cfg(feature = "encode")]
    pub fn draw(&mut self, canvas: &Canvas) -> Result<(), Error> {
        if canvas.width() != LCD_WIDTH as u32 || canvas.height() != LCD_HEIGHT as u32 {
            return Err(Error::InvalidArgument(format!(
                "full-screen image must be {LCD_WIDTH}x{LCD_HEIGHT}, got {}x{}",
                canvas.width(),
                canvas.height()
            )));
        }
        self.draw_at(0, 0, canvas)
    }

    /// Draw a canvas into a rectangle with its top-left at `(x, y)`. The
    /// rectangle must fit within the screen.
    #[cfg(feature = "encode")]
    pub fn draw_at(&mut self, x: u16, y: u16, canvas: &Canvas) -> Result<(), Error> {
        let jpeg = canvas.to_jpeg(crate::ids::DEFAULT_JPEG_QUALITY)?;
        self.draw_jpeg_at(x, y, canvas.width() as u16, canvas.height() as u16, &jpeg)
    }

    /// Draw an already-encoded JPEG into a rectangle. `width` and `height`
    /// must match the JPEG's real dimensions.
    pub fn draw_jpeg_at(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        jpeg: &[u8],
    ) -> Result<(), Error> {
        self.deck.send_lcd_region(x, y, width, height, jpeg)
    }

    /// Flood the whole screen with one color.
    #[cfg(feature = "encode")]
    pub fn fill(&mut self, color: Rgb) -> Result<(), Error> {
        let canvas = Canvas::filled(LCD_WIDTH as u32, LCD_HEIGHT as u32, color);
        self.draw(&canvas)
    }

    /// Blank the screen.
    #[cfg(feature = "encode")]
    pub fn clear(&mut self) -> Result<(), Error> {
        self.fill(Rgb::BLACK)
    }
}
