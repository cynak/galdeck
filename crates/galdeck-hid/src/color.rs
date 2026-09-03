//! Colors as the hardware sees them: 8-bit RGB.

/// An 8-bit RGB color. Both the LED commands and the image surfaces speak
/// this, so it is the single color type across the framework.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb::new(0, 0, 0);
    pub const WHITE: Rgb = Rgb::new(255, 255, 255);
    pub const RED: Rgb = Rgb::new(255, 0, 0);
    pub const GREEN: Rgb = Rgb::new(0, 255, 0);
    pub const BLUE: Rgb = Rgb::new(0, 0, 255);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    /// Parse `#rrggbb` or `rrggbb`.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        Some(Rgb::new(
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ))
    }

    /// Build from hue (0-1, wrapping), saturation and value (0-1). Handy
    /// for indicator rings and generated palettes.
    pub fn from_hsv(hue: f32, saturation: f32, value: f32) -> Self {
        let hue = hue.rem_euclid(1.0) * 6.0;
        let saturation = saturation.clamp(0.0, 1.0);
        let value = value.clamp(0.0, 1.0);
        let sector = hue.floor();
        let offset = hue - sector;
        let p = value * (1.0 - saturation);
        let q = value * (1.0 - offset * saturation);
        let t = value * (1.0 - (1.0 - offset) * saturation);
        let (r, g, b) = match sector as u32 % 6 {
            0 => (value, t, p),
            1 => (q, value, p),
            2 => (p, value, t),
            3 => (p, q, value),
            4 => (t, p, value),
            _ => (value, p, q),
        };
        let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
        Rgb::new(byte(r), byte(g), byte(b))
    }

    /// Scale all channels by `factor` (clamped), for dimming.
    pub fn scaled(self, factor: f32) -> Self {
        let scale = |c: u8| ((c as f32) * factor.max(0.0)).round().clamp(0.0, 255.0) as u8;
        Rgb::new(scale(self.r), scale(self.g), scale(self.b))
    }

    /// Linear blend towards `other`; `t` 0.0 keeps self, 1.0 gives other.
    pub fn lerp(self, other: Rgb, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb::new(
            mix(self.r, other.r),
            mix(self.g, other.g),
            mix(self.b, other.b),
        )
    }

    pub const fn to_array(self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }
}

impl From<(u8, u8, u8)> for Rgb {
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Rgb::new(r, g, b)
    }
}

impl From<[u8; 3]> for Rgb {
    fn from([r, g, b]: [u8; 3]) -> Self {
        Rgb::new(r, g, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex() {
        assert_eq!(Rgb::from_hex("#ff8000"), Some(Rgb::new(255, 128, 0)));
        assert_eq!(Rgb::from_hex("010203"), Some(Rgb::new(1, 2, 3)));
        assert_eq!(Rgb::from_hex("#f80"), None);
        assert_eq!(Rgb::from_hex("#gggggg"), None);
    }

    #[test]
    fn hsv_hits_primary_hues() {
        assert_eq!(Rgb::from_hsv(0.0, 1.0, 1.0), Rgb::RED);
        assert_eq!(Rgb::from_hsv(1.0 / 3.0, 1.0, 1.0), Rgb::GREEN);
        assert_eq!(Rgb::from_hsv(2.0 / 3.0, 1.0, 1.0), Rgb::BLUE);
        // Hue wraps rather than clamping.
        assert_eq!(Rgb::from_hsv(1.0, 1.0, 1.0), Rgb::RED);
        // Zero saturation is greyscale.
        assert_eq!(Rgb::from_hsv(0.4, 0.0, 1.0), Rgb::WHITE);
    }

    #[test]
    fn scales_and_blends() {
        assert_eq!(Rgb::WHITE.scaled(0.5), Rgb::new(128, 128, 128));
        assert_eq!(Rgb::WHITE.scaled(5.0), Rgb::WHITE);
        assert_eq!(Rgb::BLACK.lerp(Rgb::WHITE, 0.5), Rgb::new(128, 128, 128));
        assert_eq!(Rgb::BLACK.lerp(Rgb::WHITE, 2.0), Rgb::WHITE);
    }
}
