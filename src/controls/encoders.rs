//! The two push-click rotary encoders and their RGB LED rings.

use crate::device::Galleon;
use crate::error::Error;
use crate::ids::{ENCODER_COUNT, ENCODER_RING_LEDS};
use crate::Rgb;

/// Handle to both encoders. Obtained from [`Galleon::encoders`].
pub struct Encoders<'a> {
    deck: &'a mut Galleon,
}

impl<'a> Encoders<'a> {
    /// Encoders on the module.
    pub const COUNT: u8 = ENCODER_COUNT;

    pub(crate) fn new(deck: &'a mut Galleon) -> Self {
        Encoders { deck }
    }

    /// Every encoder index (0 = left, 1 = right).
    pub fn indices() -> impl Iterator<Item = u8> {
        0..ENCODER_COUNT
    }

    /// One encoder by index (0 = left, 1 = right).
    pub fn get(&mut self, index: u8) -> Result<Encoder<'_>, Error> {
        Encoder::new(self.deck, index)
    }

    /// Whether an encoder is currently held, from the last polled report.
    pub fn is_pressed(&self, index: u8) -> bool {
        self.deck.encoder_state(index)
    }

    /// Turn every ring LED off.
    pub fn clear_rings(&mut self) -> Result<(), Error> {
        for index in Self::indices() {
            self.deck.send_ring_color(index, Rgb::BLACK)?;
        }
        Ok(())
    }
}

/// Handle to one encoder. Obtained from [`Encoders::get`] or
/// [`Galleon::encoder`].
pub struct Encoder<'a> {
    deck: &'a mut Galleon,
    index: u8,
}

impl<'a> Encoder<'a> {
    pub(crate) fn new(deck: &'a mut Galleon, index: u8) -> Result<Self, Error> {
        if index >= ENCODER_COUNT {
            return Err(Error::InvalidArgument(format!(
                "encoder index must be 0-{}, got {index}",
                ENCODER_COUNT - 1
            )));
        }
        Ok(Encoder { deck, index })
    }

    pub fn index(&self) -> u8 {
        self.index
    }

    /// Whether the encoder is currently held, from the last polled report.
    pub fn is_pressed(&self) -> bool {
        self.deck.encoder_state(self.index)
    }

    /// The LED ring around this encoder.
    pub fn ring(&mut self) -> Ring<'_> {
        Ring {
            deck: self.deck,
            encoder: self.index,
        }
    }
}

/// The ring of individually addressable RGB LEDs around an encoder.
///
/// Segments are numbered clockwise from the top, so segment 0 is always
/// the top LED regardless of the hardware's internal ordering.
pub struct Ring<'a> {
    deck: &'a mut Galleon,
    encoder: u8,
}

impl Ring<'_> {
    /// Addressable segments in the ring.
    pub const SEGMENTS: u8 = ENCODER_RING_LEDS;

    /// Every segment index, clockwise from the top.
    pub fn segments() -> impl Iterator<Item = u8> {
        0..ENCODER_RING_LEDS
    }

    /// Light the whole ring one color.
    pub fn set_all(&mut self, color: Rgb) -> Result<(), Error> {
        self.deck.send_ring_color(self.encoder, color)
    }

    /// Light one segment, numbered clockwise from the top.
    pub fn set_segment(&mut self, segment: u8, color: Rgb) -> Result<(), Error> {
        self.deck.send_ring_segment(self.encoder, segment, color)
    }

    /// Light each segment individually, clockwise from the top.
    pub fn set_segments(&mut self, colors: [Rgb; ENCODER_RING_LEDS as usize]) -> Result<(), Error> {
        for (segment, color) in colors.into_iter().enumerate() {
            self.set_segment(segment as u8, color)?;
        }
        Ok(())
    }

    /// Show a 0.0-1.0 level as filled segments, clockwise from the top —
    /// the ring's natural readout for things like volume. Richer
    /// visualisations belong in the layer above.
    pub fn set_level(&mut self, level: f32, on: Rgb, off: Rgb) -> Result<(), Error> {
        let filled = (level.clamp(0.0, 1.0) * ENCODER_RING_LEDS as f32).round() as u8;
        for segment in Self::segments() {
            let color = if segment < filled { on } else { off };
            self.set_segment(segment, color)?;
        }
        Ok(())
    }

    /// Turn the ring off.
    pub fn clear(&mut self) -> Result<(), Error> {
        self.set_all(Rgb::BLACK)
    }
}
