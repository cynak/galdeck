//! The 12 LCD keys.

use crate::canvas::Canvas;
use crate::device::Galleon;
use crate::error::Error;
use crate::ids::{KEY_COLUMNS, KEY_COUNT, KEY_PIXELS, KEY_ROWS};
use crate::Rgb;

/// Handle to the whole key block. Obtained from [`Galleon::buttons`].
pub struct Buttons<'a> {
    deck: &'a mut Galleon,
}

impl<'a> Buttons<'a> {
    /// Keys on the module.
    pub const COUNT: u8 = KEY_COUNT;
    /// Columns in the key grid.
    pub const COLUMNS: u8 = KEY_COLUMNS;
    /// Rows in the key grid.
    pub const ROWS: u8 = KEY_ROWS;
    /// Width and height of one key's display, in pixels.
    pub const PIXEL_SIZE: u32 = KEY_PIXELS;

    pub(crate) fn new(deck: &'a mut Galleon) -> Self {
        Buttons { deck }
    }

    /// Every key index, row-major from the top-left.
    pub fn indices() -> impl Iterator<Item = u8> {
        0..KEY_COUNT
    }

    /// One key by index (0-11, row-major from the top-left).
    pub fn get(&mut self, index: u8) -> Result<Button<'_>, Error> {
        Button::new(self.deck, index)
    }

    /// One key by grid position.
    pub fn at(&mut self, column: u8, row: u8) -> Result<Button<'_>, Error> {
        if column >= KEY_COLUMNS || row >= KEY_ROWS {
            return Err(Error::InvalidArgument(format!(
                "position ({column}, {row}) is outside the {KEY_COLUMNS}x{KEY_ROWS} key grid"
            )));
        }
        self.get(row * KEY_COLUMNS + column)
    }

    /// Whether a key is currently held, from the last polled report.
    pub fn is_pressed(&self, index: u8) -> bool {
        self.deck.key_state(index)
    }

    /// Paint every key one solid color.
    pub fn set_all(&mut self, color: Rgb) -> Result<(), Error> {
        for index in Self::indices() {
            self.deck.send_key_color(index, color)?;
        }
        Ok(())
    }

    /// Blank every key.
    pub fn clear(&mut self) -> Result<(), Error> {
        self.set_all(Rgb::BLACK)
    }
}

/// Handle to one key. Obtained from [`Buttons::get`] or [`Galleon::button`].
pub struct Button<'a> {
    deck: &'a mut Galleon,
    index: u8,
}

impl<'a> Button<'a> {
    pub(crate) fn new(deck: &'a mut Galleon, index: u8) -> Result<Self, Error> {
        if index >= KEY_COUNT {
            return Err(Error::InvalidArgument(format!(
                "key index must be 0-{}, got {index}",
                KEY_COUNT - 1
            )));
        }
        Ok(Button { deck, index })
    }

    pub fn index(&self) -> u8 {
        self.index
    }

    /// Grid position as `(column, row)`, from the top-left.
    pub fn position(&self) -> (u8, u8) {
        (self.index % KEY_COLUMNS, self.index / KEY_COLUMNS)
    }

    /// Display size of a key in pixels.
    pub const fn size() -> (u32, u32) {
        (KEY_PIXELS, KEY_PIXELS)
    }

    /// A blank canvas the exact size of this key, ready to draw into.
    pub fn canvas(&self) -> Canvas {
        Canvas::new(KEY_PIXELS, KEY_PIXELS)
    }

    /// Whether the key is currently held, from the last polled report.
    pub fn is_pressed(&self) -> bool {
        self.deck.key_state(self.index)
    }

    /// Light the key one solid color. Cheaper than an image: one feature
    /// report instead of a JPEG upload.
    pub fn set_color(&mut self, color: Rgb) -> Result<(), Error> {
        self.deck.send_key_color(self.index, color)
    }

    /// Blank the key.
    pub fn clear(&mut self) -> Result<(), Error> {
        self.set_color(Rgb::BLACK)
    }

    /// Show a canvas on the key. It must be exactly 160x160; use
    /// [`Canvas::scaled`] first if yours is not.
    #[cfg(feature = "encode")]
    pub fn draw(&mut self, canvas: &Canvas) -> Result<(), Error> {
        if canvas.width() != KEY_PIXELS || canvas.height() != KEY_PIXELS {
            return Err(Error::InvalidArgument(format!(
                "key image must be {KEY_PIXELS}x{KEY_PIXELS}, got {}x{}",
                canvas.width(),
                canvas.height()
            )));
        }
        let jpeg = canvas.to_jpeg(crate::ids::DEFAULT_JPEG_QUALITY)?;
        self.set_jpeg(&jpeg)
    }

    /// Show an already-encoded JPEG on the key (160x160, baseline).
    pub fn set_jpeg(&mut self, jpeg: &[u8]) -> Result<(), Error> {
        self.deck.send_key_jpeg(self.index, jpeg)
    }
}
