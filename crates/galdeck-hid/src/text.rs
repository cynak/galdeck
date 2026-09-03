//! Glyph rendering onto a [`Canvas`].
//!
//! Drawing a label is a hardware-level need — every consumer would
//! otherwise reimplement it — so the framework provides placement and
//! measurement. Choosing fonts, sizes, and layout is the caller's job.

use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont};

use crate::canvas::Canvas;
use crate::error::Error;
use crate::Rgb;

/// Where text sits relative to the anchor point given to [`Canvas::draw_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Anchor is the left edge of the text.
    #[default]
    Left,
    /// Anchor is the horizontal centre of the text.
    Center,
    /// Anchor is the right edge of the text.
    Right,
}

/// A loaded font face.
pub struct Font {
    inner: FontVec,
}

/// Common system locations of a sans-serif face, tried by [`Font::system`].
const SYSTEM_FONT_PATHS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
];

impl Font {
    /// Load a TTF/OTF file.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Font, Error> {
        let path = path.as_ref();
        let data = std::fs::read(path)
            .map_err(|e| Error::InvalidArgument(format!("reading font {}: {e}", path.display())))?;
        Font::from_bytes(data)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Font, Error> {
        let inner = FontVec::try_from_vec(data)
            .map_err(|e| Error::InvalidArgument(format!("parsing font: {e}")))?;
        Ok(Font { inner })
    }

    /// First usable sans-serif face found in the usual system locations,
    /// or `None` if the system ships none where we look.
    pub fn system() -> Option<Font> {
        for candidate in SYSTEM_FONT_PATHS {
            if let Ok(font) = Font::load(candidate) {
                return Some(font);
            }
        }
        None
    }

    /// Width in pixels the string would occupy at `size_px`.
    pub fn measure(&self, text: &str, size_px: f32) -> f32 {
        let scaled = self.inner.as_scaled(PxScale::from(size_px));
        let mut width = 0.0;
        let mut previous = None;
        for character in text.chars() {
            let id = scaled.glyph_id(character);
            if let Some(previous) = previous {
                width += scaled.kern(previous, id);
            }
            width += scaled.h_advance(id);
            previous = Some(id);
        }
        width
    }

    /// Distance from the top of a line to its baseline at `size_px`.
    pub fn ascent(&self, size_px: f32) -> f32 {
        self.inner.as_scaled(PxScale::from(size_px)).ascent()
    }

    /// Full line height at `size_px`.
    pub fn line_height(&self, size_px: f32) -> f32 {
        let scaled = self.inner.as_scaled(PxScale::from(size_px));
        scaled.ascent() - scaled.descent() + scaled.line_gap()
    }

    /// Largest size (at most `max_size_px`) at which `text` fits `max_width`.
    pub fn fitting_size(&self, text: &str, max_width: f32, max_size_px: f32) -> f32 {
        let width = self.measure(text, max_size_px);
        if width <= max_width || width <= f32::EPSILON {
            max_size_px
        } else {
            max_size_px * (max_width / width)
        }
    }
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font").finish_non_exhaustive()
    }
}

/// How a run of text should be drawn: face, size, color, alignment, and
/// an optional width it must shrink to fit.
#[derive(Debug, Clone, Copy)]
pub struct TextStyle<'f> {
    font: &'f Font,
    size_px: f32,
    color: Rgb,
    align: Align,
    max_width: Option<u32>,
}

impl<'f> TextStyle<'f> {
    /// White, left-aligned text at `size_px`.
    pub fn new(font: &'f Font, size_px: f32) -> Self {
        TextStyle {
            font,
            size_px,
            color: Rgb::WHITE,
            align: Align::Left,
            max_width: None,
        }
    }

    pub fn color(mut self, color: Rgb) -> Self {
        self.color = color;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// Shrink the text if it would otherwise exceed this width.
    pub fn max_width(mut self, max_width: u32) -> Self {
        self.max_width = Some(max_width);
        self
    }

    /// The size this style resolves to for `text`, after any shrink-to-fit.
    pub fn resolved_size(&self, text: &str) -> f32 {
        match self.max_width {
            Some(max_width) => self.font.fitting_size(text, max_width as f32, self.size_px),
            None => self.size_px,
        }
    }
}

impl Canvas {
    /// Draw a single line of text. `(x, y)` is the anchor: `y` is the
    /// vertical centre of the line, `x` its left edge, centre, or right
    /// edge according to the style's alignment.
    pub fn draw_text(&mut self, text: &str, x: i32, y: i32, style: &TextStyle<'_>) {
        let font = style.font;
        let size_px = style.resolved_size(text);
        let scale = PxScale::from(size_px);
        let scaled = font.inner.as_scaled(scale);
        let width = font.measure(text, size_px);

        let mut pen_x = match style.align {
            Align::Left => x as f32,
            Align::Center => x as f32 - width / 2.0,
            Align::Right => x as f32 - width,
        };
        // Centre the ink of the line on `y`.
        let baseline = y as f32 + (scaled.ascent() + scaled.descent()) / 2.0 - scaled.descent();

        let mut previous = None;
        for character in text.chars() {
            let id = scaled.glyph_id(character);
            if let Some(previous) = previous {
                pen_x += scaled.kern(previous, id);
            }
            let glyph = id.with_scale_and_position(scale, ab_glyph::point(pen_x, baseline));
            pen_x += scaled.h_advance(id);
            previous = Some(id);

            if let Some(outlined) = font.inner.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                let origin_x = bounds.min.x as i32;
                let origin_y = bounds.min.y as i32;
                outlined.draw(|gx, gy, coverage| {
                    self.blend_pixel(
                        origin_x + gx as i32,
                        origin_y + gy as i32,
                        style.color,
                        coverage,
                    );
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_font() -> Option<Font> {
        Font::system()
    }

    #[test]
    fn measures_and_fits_text() {
        let Some(font) = test_font() else {
            eprintln!("no system font available; skipping");
            return;
        };
        let wide = font.measure("wwwwwwwwww", 20.0);
        let narrow = font.measure("i", 20.0);
        assert!(wide > narrow);
        assert!(font.measure("", 20.0) == 0.0);

        // Doubling the size roughly doubles the width.
        let single = font.measure("hello", 20.0);
        let double = font.measure("hello", 40.0);
        assert!((double / single - 2.0).abs() < 0.1);

        // A string too wide gets a reduced size, a narrow one keeps its size.
        assert!(font.fitting_size("wwwwwwwwwwww", 50.0, 40.0) < 40.0);
        assert_eq!(font.fitting_size("i", 500.0, 40.0), 40.0);
    }

    #[test]
    fn draws_text_within_bounds() {
        let Some(font) = test_font() else {
            eprintln!("no system font available; skipping");
            return;
        };
        let style = TextStyle::new(&font, 40.0).align(Align::Center);
        let mut canvas = Canvas::new(160, 160);
        canvas.draw_text("Hi", 80, 80, &style);
        let lit = canvas.as_rgb().iter().filter(|b| **b > 0).count();
        assert!(lit > 0, "text should have marked pixels");

        // Text anchored far outside must clip, not panic.
        let mut canvas = Canvas::new(32, 32);
        canvas.draw_text("overflowing", -500, -500, &TextStyle::new(&font, 40.0));
        canvas.draw_text(
            "overflowing",
            500,
            500,
            &TextStyle::new(&font, 40.0).align(Align::Right),
        );
    }

    #[test]
    fn max_width_shrinks_text_to_fit() {
        let Some(font) = test_font() else {
            eprintln!("no system font available; skipping");
            return;
        };
        let plain = TextStyle::new(&font, 40.0);
        let fitted = TextStyle::new(&font, 40.0).max_width(50);
        assert_eq!(plain.resolved_size("wwwwwwwwwwww"), 40.0);
        assert!(fitted.resolved_size("wwwwwwwwwwww") < 40.0);
        // Text that already fits keeps its requested size.
        assert_eq!(fitted.resolved_size("i"), 40.0);
    }

    #[test]
    fn alignment_shifts_the_ink() {
        let Some(font) = test_font() else {
            eprintln!("no system font available; skipping");
            return;
        };
        let ink_center_x = |align: Align| -> Option<f32> {
            let mut canvas = Canvas::new(200, 60);
            canvas.draw_text("abc", 100, 30, &TextStyle::new(&font, 24.0).align(align));
            let mut sum = 0.0;
            let mut count = 0.0;
            for x in 0..200 {
                for y in 0..60 {
                    if canvas.pixel(x, y).map(|p| p.r > 0).unwrap_or(false) {
                        sum += x as f32;
                        count += 1.0;
                    }
                }
            }
            (count > 0.0).then_some(sum / count)
        };
        let left = ink_center_x(Align::Left).unwrap();
        let center = ink_center_x(Align::Center).unwrap();
        let right = ink_center_x(Align::Right).unwrap();
        assert!(
            right < center && center < left,
            "got {right} {center} {left}"
        );
        assert!((center - 100.0).abs() < 12.0, "centered text near anchor");
    }
}
