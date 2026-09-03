//! Key and info-screen image rendering.

use std::path::Path;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use anyhow::{Context, Result};
use galdeck_hid::ids::{KEY_PIXELS, LCD_HEIGHT, LCD_WIDTH};
use image::{imageops, Rgb, RgbImage};

/// Common system locations of a usable sans-serif TTF.
const FONT_SEARCH_PATHS: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
];

pub fn load_font(configured: Option<&Path>) -> Result<Option<FontVec>> {
    if let Some(path) = configured {
        let data =
            std::fs::read(path).with_context(|| format!("reading font {}", path.display()))?;
        return Ok(Some(
            FontVec::try_from_vec(data)
                .with_context(|| format!("parsing font {}", path.display()))?,
        ));
    }
    for candidate in FONT_SEARCH_PATHS {
        if let Ok(data) = std::fs::read(candidate) {
            if let Ok(font) = FontVec::try_from_vec(data) {
                log::debug!("using font {candidate}");
                return Ok(Some(font));
            }
        }
    }
    log::warn!("no usable font found — key labels and info-screen text will be skipped; set `font` in the config");
    Ok(None)
}

/// Draw `text` centered horizontally at `center_y` (vertical center of the
/// text block), blended onto the image.
fn draw_text_centered(
    img: &mut RgbImage,
    font: &FontVec,
    px: f32,
    text: &str,
    center_x: f32,
    center_y: f32,
    color: [u8; 3],
) {
    let scale = PxScale::from(px);
    let scaled = font.as_scaled(scale);

    let mut width = 0.0f32;
    let mut last: Option<ab_glyph::GlyphId> = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(prev) = last {
            width += scaled.kern(prev, id);
        }
        width += scaled.h_advance(id);
        last = Some(id);
    }

    let mut x = center_x - width / 2.0;
    let baseline = center_y + (scaled.ascent() + scaled.descent()) / 2.0 - scaled.descent();
    let mut last: Option<ab_glyph::GlyphId> = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(prev) = last {
            x += scaled.kern(prev, id);
        }
        let glyph = id.with_scale_and_position(scale, ab_glyph::point(x, baseline));
        x += scaled.h_advance(id);
        last = Some(id);

        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px < 0 || py < 0 || px >= img.width() as i32 || py >= img.height() as i32 {
                    return;
                }
                let pixel = img.get_pixel_mut(px as u32, py as u32);
                for (channel, target) in pixel.0.iter_mut().zip(color) {
                    *channel =
                        (*channel as f32 + (target as f32 - *channel as f32) * coverage) as u8;
                }
            });
        }
    }
}

/// Elide a label so it plausibly fits a key.
fn fit_label(label: &str, max_chars: usize) -> String {
    if label.chars().count() <= max_chars {
        label.to_string()
    } else {
        let mut s: String = label.chars().take(max_chars.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

/// Render one 160x160 key image: background color, optional icon, optional
/// label along the bottom.
pub fn render_key(
    background: [u8; 3],
    icon: Option<&Path>,
    label: Option<&str>,
    font: Option<&FontVec>,
) -> Result<Vec<u8>> {
    let size = KEY_PIXELS;
    let mut img = RgbImage::from_pixel(size, size, Rgb(background));

    let label_strip = if label.is_some() { 36 } else { 0 };

    if let Some(path) = icon {
        let icon_img = image::open(path)
            .with_context(|| format!("loading icon {}", path.display()))?
            .to_rgb8();
        let max_h = size - label_strip - 8;
        let max_w = size - 8;
        let scale = (max_w as f32 / icon_img.width() as f32)
            .min(max_h as f32 / icon_img.height() as f32)
            .min(1.0);
        let (w, h) = (
            (icon_img.width() as f32 * scale) as u32,
            (icon_img.height() as f32 * scale) as u32,
        );
        let resized = imageops::resize(
            &icon_img,
            w.max(1),
            h.max(1),
            imageops::FilterType::Triangle,
        );
        let x = (size - resized.width()) / 2;
        let y = (size - label_strip - resized.height()) / 2;
        imageops::overlay(&mut img, &resized, x as i64, y as i64);
    }

    if let (Some(label), Some(font)) = (label, font) {
        let text = fit_label(label, 12);
        let center_y = if icon.is_some() {
            size as f32 - label_strip as f32 / 2.0 - 4.0
        } else {
            size as f32 / 2.0
        };
        draw_text_centered(
            &mut img,
            font,
            26.0,
            &text,
            size as f32 / 2.0,
            center_y,
            [255, 255, 255],
        );
    }

    Ok(img.into_raw())
}

/// Render the 720x384 info-screen image: dark background, centered text.
pub fn render_lcd(text: &str, font: Option<&FontVec>) -> Vec<u8> {
    let mut img = RgbImage::from_pixel(LCD_WIDTH as u32, LCD_HEIGHT as u32, Rgb([16, 18, 24]));
    if let Some(font) = font {
        draw_text_centered(
            &mut img,
            font,
            56.0,
            &fit_label(text, 24),
            LCD_WIDTH as f32 / 2.0,
            LCD_HEIGHT as f32 / 2.0,
            [220, 224, 232],
        );
    }
    img.into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_key_without_font_or_icon() {
        let rgb = render_key([10, 20, 30], None, Some("ignored"), None).unwrap();
        assert_eq!(rgb.len(), 160 * 160 * 3);
        assert_eq!(&rgb[..3], &[10, 20, 30]);
    }

    #[test]
    fn renders_lcd_dimensions() {
        let rgb = render_lcd("hello", None);
        assert_eq!(rgb.len(), 720 * 384 * 3);
    }

    #[test]
    fn elides_long_labels() {
        assert_eq!(fit_label("short", 12), "short");
        let elided = fit_label("a very long label indeed", 12);
        assert_eq!(elided.chars().count(), 12);
        assert!(elided.ends_with('…'));
    }
}
