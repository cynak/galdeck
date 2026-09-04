//! A plain RGB drawing surface with the primitives a panel needs.
//!
//! Everything the module displays — a key image, a region of the info
//! screen — is a [`Canvas`] that gets JPEG-encoded on the way out. The
//! framework provides geometry, blitting, and (with the `text` feature)
//! glyph rendering; layout, widgets, and theming belong to the layer
//! above.

use crate::error::Error;

/// An RGB8 pixel buffer, row-major, no padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    /// A black canvas.
    pub fn new(width: u32, height: u32) -> Self {
        Canvas {
            width,
            height,
            pixels: vec![0; (width as usize) * (height as usize) * 3],
        }
    }

    /// A canvas filled with one color.
    pub fn filled(width: u32, height: u32, color: crate::Rgb) -> Self {
        let mut canvas = Canvas::new(width, height);
        canvas.fill(color);
        canvas
    }

    /// Wrap an existing RGB8 buffer; it must be exactly `width * height * 3`.
    pub fn from_rgb(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, Error> {
        let expected = (width as usize) * (height as usize) * 3;
        if pixels.len() != expected {
            return Err(Error::InvalidArgument(format!(
                "expected {expected} bytes for a {width}x{height} rgb canvas, got {}",
                pixels.len()
            )));
        }
        Ok(Canvas {
            width,
            height,
            pixels,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The raw RGB8 buffer.
    pub fn as_rgb(&self) -> &[u8] {
        &self.pixels
    }

    pub fn into_rgb(self) -> Vec<u8> {
        self.pixels
    }

    fn offset(&self, x: u32, y: u32) -> usize {
        ((y as usize) * (self.width as usize) + (x as usize)) * 3
    }

    fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height
    }

    /// Paint every pixel one color.
    pub fn fill(&mut self, color: crate::Rgb) {
        let rgb = color.to_array();
        for pixel in self.pixels.as_chunks_mut::<3>().0 {
            *pixel = rgb;
        }
    }

    /// Set one pixel; out-of-bounds coordinates are ignored, so callers can
    /// draw shapes that run off the edge without pre-clipping.
    pub fn set_pixel(&mut self, x: i32, y: i32, color: crate::Rgb) {
        if !self.in_bounds(x, y) {
            return;
        }
        let offset = self.offset(x as u32, y as u32);
        self.pixels[offset..offset + 3].copy_from_slice(&color.to_array());
    }

    /// Blend one pixel with `coverage` 0.0-1.0 (used by glyph rendering
    /// and available for antialiased drawing).
    pub fn blend_pixel(&mut self, x: i32, y: i32, color: crate::Rgb, coverage: f32) {
        if !self.in_bounds(x, y) || coverage <= 0.0 {
            return;
        }
        let coverage = coverage.min(1.0);
        let offset = self.offset(x as u32, y as u32);
        for (channel, target) in self.pixels[offset..offset + 3]
            .iter_mut()
            .zip(color.to_array())
        {
            *channel = (*channel as f32 + (target as f32 - *channel as f32) * coverage) as u8;
        }
    }

    pub fn pixel(&self, x: i32, y: i32) -> Option<crate::Rgb> {
        if !self.in_bounds(x, y) {
            return None;
        }
        let offset = self.offset(x as u32, y as u32);
        Some(crate::Rgb::new(
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
        ))
    }

    /// A straight line between two points (Bresenham).
    pub fn draw_line(&mut self, from: (i32, i32), to: (i32, i32), color: crate::Rgb) {
        self.draw_line_thick(from, to, color, 1);
    }

    /// A straight line drawn with a square brush `thickness` pixels wide.
    pub fn draw_line_thick(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        color: crate::Rgb,
        thickness: u32,
    ) {
        let (mut x, mut y) = from;
        let (x1, y1) = to;
        let dx = (x1 - x).abs();
        let dy = -(y1 - y).abs();
        let step_x = if x < x1 { 1 } else { -1 };
        let step_y = if y < y1 { 1 } else { -1 };
        let mut error = dx + dy;

        loop {
            self.stamp(x, y, color, thickness);
            if x == x1 && y == y1 {
                break;
            }
            let double = 2 * error;
            if double >= dy {
                error += dy;
                x += step_x;
            }
            if double <= dx {
                error += dx;
                y += step_y;
            }
        }
    }

    /// Square brush centred on a point.
    fn stamp(&mut self, x: i32, y: i32, color: crate::Rgb, thickness: u32) {
        if thickness <= 1 {
            self.set_pixel(x, y, color);
            return;
        }
        let half = (thickness / 2) as i32;
        for oy in -half..=half {
            for ox in -half..=half {
                self.set_pixel(x + ox, y + oy, color);
            }
        }
    }

    /// Horizontal line, `width` pixels long.
    pub fn draw_hline(&mut self, x: i32, y: i32, width: u32, color: crate::Rgb) {
        for offset in 0..width as i32 {
            self.set_pixel(x + offset, y, color);
        }
    }

    /// Vertical line, `height` pixels long.
    pub fn draw_vline(&mut self, x: i32, y: i32, height: u32, color: crate::Rgb) {
        for offset in 0..height as i32 {
            self.set_pixel(x, y + offset, color);
        }
    }

    /// Rectangle outline.
    pub fn draw_rect(&mut self, x: i32, y: i32, width: u32, height: u32, color: crate::Rgb) {
        if width == 0 || height == 0 {
            return;
        }
        self.draw_hline(x, y, width, color);
        self.draw_hline(x, y + height as i32 - 1, width, color);
        self.draw_vline(x, y, height, color);
        self.draw_vline(x + width as i32 - 1, y, height, color);
    }

    /// Solid rectangle.
    pub fn fill_rect(&mut self, x: i32, y: i32, width: u32, height: u32, color: crate::Rgb) {
        for row in 0..height as i32 {
            self.draw_hline(x, y + row, width, color);
        }
    }

    /// Circle outline (midpoint algorithm).
    pub fn draw_circle(&mut self, center: (i32, i32), radius: u32, color: crate::Rgb) {
        let (cx, cy) = center;
        let mut x = radius as i32;
        let mut y = 0;
        let mut error = 1 - x;
        while x >= y {
            for (px, py) in [
                (cx + x, cy + y),
                (cx + y, cy + x),
                (cx - y, cy + x),
                (cx - x, cy + y),
                (cx - x, cy - y),
                (cx - y, cy - x),
                (cx + y, cy - x),
                (cx + x, cy - y),
            ] {
                self.set_pixel(px, py, color);
            }
            y += 1;
            if error < 0 {
                error += 2 * y + 1;
            } else {
                x -= 1;
                error += 2 * (y - x) + 1;
            }
        }
    }

    /// Solid circle.
    pub fn fill_circle(&mut self, center: (i32, i32), radius: u32, color: crate::Rgb) {
        let (cx, cy) = center;
        let radius = radius as i32;
        for dy in -radius..=radius {
            let span = ((radius * radius - dy * dy) as f32).sqrt() as i32;
            self.draw_hline(cx - span, cy + dy, (span * 2 + 1) as u32, color);
        }
    }

    /// Copy another canvas onto this one at `(x, y)`, clipped to bounds.
    pub fn blit(&mut self, source: &Canvas, x: i32, y: i32) {
        for row in 0..source.height {
            let ty = y + row as i32;
            if ty < 0 || ty as u32 >= self.height {
                continue;
            }
            for column in 0..source.width {
                let tx = x + column as i32;
                if tx < 0 || tx as u32 >= self.width {
                    continue;
                }
                let from = source.offset(column, row);
                let to = self.offset(tx as u32, ty as u32);
                self.pixels[to..to + 3].copy_from_slice(&source.pixels[from..from + 3]);
            }
        }
    }

    /// A copy scaled to exactly `width` x `height` (nearest neighbour, so
    /// it stays dependency-free; use [`Canvas::load_scaled`] for photos).
    pub fn scaled(&self, width: u32, height: u32) -> Canvas {
        let mut out = Canvas::new(width, height);
        if width == 0 || height == 0 || self.width == 0 || self.height == 0 {
            return out;
        }
        for row in 0..height {
            let source_row = row * self.height / height;
            for column in 0..width {
                let source_column = column * self.width / width;
                let from = self.offset(source_column, source_row);
                let to = out.offset(column, row);
                out.pixels[to..to + 3].copy_from_slice(&self.pixels[from..from + 3]);
            }
        }
        out
    }

    /// Encode as a baseline JPEG, the format the module accepts.
    #[cfg(feature = "encode")]
    pub fn to_jpeg(&self, quality: u8) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
        image::ImageEncoder::write_image(
            encoder,
            &self.pixels,
            self.width,
            self.height,
            image::ExtendedColorType::Rgb8,
        )?;
        Ok(out)
    }

    /// Load an image file (PNG, JPEG, …) as a canvas.
    #[cfg(feature = "encode")]
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Canvas, Error> {
        let image = image::open(path)?.to_rgb8();
        let (width, height) = image.dimensions();
        Canvas::from_rgb(width, height, image.into_raw())
    }

    /// Load an image and scale it to fit inside `width` x `height` while
    /// keeping its aspect ratio (never enlarging), returning the fitted
    /// canvas — the usual way to put an icon on a key.
    #[cfg(feature = "encode")]
    pub fn load_scaled(
        path: impl AsRef<std::path::Path>,
        width: u32,
        height: u32,
    ) -> Result<Canvas, Error> {
        let image = image::open(path)?.to_rgb8();
        let (source_width, source_height) = image.dimensions();
        if source_width == 0 || source_height == 0 {
            return Err(Error::InvalidArgument("image has zero size".into()));
        }
        let scale = (width as f32 / source_width as f32)
            .min(height as f32 / source_height as f32)
            .min(1.0);
        let target_width = ((source_width as f32 * scale) as u32).max(1);
        let target_height = ((source_height as f32 * scale) as u32).max(1);
        let resized = image::imageops::resize(
            &image,
            target_width,
            target_height,
            image::imageops::FilterType::Triangle,
        );
        Canvas::from_rgb(target_width, target_height, resized.into_raw())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rgb;

    #[test]
    fn new_canvas_is_black_and_correctly_sized() {
        let canvas = Canvas::new(4, 3);
        assert_eq!(canvas.width(), 4);
        assert_eq!(canvas.height(), 3);
        assert_eq!(canvas.as_rgb().len(), 4 * 3 * 3);
        assert!(canvas.as_rgb().iter().all(|b| *b == 0));
        assert_eq!(canvas.pixel(0, 0), Some(Rgb::BLACK));
    }

    #[test]
    fn from_rgb_checks_length() {
        assert!(Canvas::from_rgb(2, 2, vec![0; 12]).is_ok());
        assert!(Canvas::from_rgb(2, 2, vec![0; 11]).is_err());
    }

    #[test]
    fn set_pixel_clips_instead_of_panicking() {
        let mut canvas = Canvas::new(2, 2);
        canvas.set_pixel(-1, 0, Rgb::WHITE);
        canvas.set_pixel(0, -1, Rgb::WHITE);
        canvas.set_pixel(2, 0, Rgb::WHITE);
        canvas.set_pixel(0, 2, Rgb::WHITE);
        assert!(canvas.as_rgb().iter().all(|b| *b == 0));
        assert_eq!(canvas.pixel(-1, 0), None);
        assert_eq!(canvas.pixel(2, 2), None);
    }

    #[test]
    fn draws_lines_including_diagonals_and_off_canvas() {
        let mut canvas = Canvas::new(5, 5);
        canvas.draw_line((0, 0), (4, 4), Rgb::WHITE);
        for i in 0..5 {
            assert_eq!(canvas.pixel(i, i), Some(Rgb::WHITE));
        }
        assert_eq!(canvas.pixel(0, 4), Some(Rgb::BLACK));

        // A line running off the edge clips rather than panics.
        canvas.draw_line((-10, 2), (10, 2), Rgb::RED);
        assert_eq!(canvas.pixel(0, 2), Some(Rgb::RED));
        assert_eq!(canvas.pixel(4, 2), Some(Rgb::RED));
    }

    #[test]
    fn rect_outline_touches_only_the_border() {
        let mut canvas = Canvas::new(5, 5);
        canvas.draw_rect(1, 1, 3, 3, Rgb::WHITE);
        assert_eq!(canvas.pixel(1, 1), Some(Rgb::WHITE));
        assert_eq!(canvas.pixel(3, 3), Some(Rgb::WHITE));
        assert_eq!(canvas.pixel(2, 2), Some(Rgb::BLACK)); // interior
        assert_eq!(canvas.pixel(0, 0), Some(Rgb::BLACK)); // outside
    }

    #[test]
    fn fill_rect_covers_its_area() {
        let mut canvas = Canvas::new(4, 4);
        canvas.fill_rect(1, 1, 2, 2, Rgb::GREEN);
        assert_eq!(canvas.pixel(1, 1), Some(Rgb::GREEN));
        assert_eq!(canvas.pixel(2, 2), Some(Rgb::GREEN));
        assert_eq!(canvas.pixel(3, 3), Some(Rgb::BLACK));
    }

    #[test]
    fn circles_are_centred() {
        let mut canvas = Canvas::new(9, 9);
        canvas.fill_circle((4, 4), 3, Rgb::BLUE);
        assert_eq!(canvas.pixel(4, 4), Some(Rgb::BLUE));
        assert_eq!(canvas.pixel(4, 1), Some(Rgb::BLUE)); // top of circle
        assert_eq!(canvas.pixel(0, 0), Some(Rgb::BLACK)); // corner is outside
    }

    #[test]
    fn blit_clips_at_the_edges() {
        let mut target = Canvas::new(4, 4);
        let source = Canvas::filled(2, 2, Rgb::WHITE);
        target.blit(&source, 3, 3); // only one pixel lands
        assert_eq!(target.pixel(3, 3), Some(Rgb::WHITE));
        target.blit(&source, -1, -1); // only one pixel lands
        assert_eq!(target.pixel(0, 0), Some(Rgb::WHITE));
        assert_eq!(target.pixel(1, 1), Some(Rgb::BLACK));
    }

    #[test]
    fn scaling_preserves_content() {
        let mut source = Canvas::new(2, 2);
        source.set_pixel(0, 0, Rgb::RED);
        source.set_pixel(1, 1, Rgb::BLUE);
        let scaled = source.scaled(4, 4);
        assert_eq!(scaled.width(), 4);
        assert_eq!(scaled.pixel(0, 0), Some(Rgb::RED));
        assert_eq!(scaled.pixel(3, 3), Some(Rgb::BLUE));
        // Degenerate targets return an empty canvas rather than panicking.
        assert_eq!(source.scaled(0, 4).as_rgb().len(), 0);
    }

    #[test]
    fn blend_pixel_respects_coverage() {
        let mut canvas = Canvas::new(1, 1);
        canvas.blend_pixel(0, 0, Rgb::WHITE, 0.0);
        assert_eq!(canvas.pixel(0, 0), Some(Rgb::BLACK));
        canvas.blend_pixel(0, 0, Rgb::WHITE, 1.0);
        assert_eq!(canvas.pixel(0, 0), Some(Rgb::WHITE));
    }

    #[cfg(feature = "encode")]
    #[test]
    fn encodes_jpeg_that_decodes_back_to_the_same_size() {
        let canvas = Canvas::filled(160, 160, Rgb::new(200, 30, 40));
        let jpeg = canvas.to_jpeg(90).unwrap();
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]); // JPEG SOI marker
        let decoded = image::load_from_memory(&jpeg).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (160, 160));
    }
}
