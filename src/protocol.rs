//! Pure report builders and parsers. Nothing in this module touches the
//! device, so all of it is unit-tested without hardware.

use crate::error::Error;
use crate::ids::*;

/// Key-image output reports carry an 8-byte header.
const KEY_IMAGE_HEADER_LEN: usize = 8;
const KEY_IMAGE_CHUNK: usize = OUTPUT_REPORT_LEN - KEY_IMAGE_HEADER_LEN; // 1016
/// LCD-region output reports carry a 16-byte header.
const LCD_IMAGE_HEADER_LEN: usize = 16;
const LCD_IMAGE_CHUNK: usize = OUTPUT_REPORT_LEN - LCD_IMAGE_HEADER_LEN; // 1008

/// `03 27`: keepalive / stay-in-software-mode. Send every 500 ms.
pub fn keepalive_report() -> [u8; FEATURE_REPORT_LEN] {
    let mut report = [0u8; FEATURE_REPORT_LEN];
    report[0] = 0x03;
    report[1] = 0x27;
    report
}

/// `03 08 <pct>`: panel brightness, 0-100.
pub fn brightness_report(percent: u8) -> Result<[u8; FEATURE_REPORT_LEN], Error> {
    if percent > 100 {
        return Err(Error::InvalidArgument(format!(
            "brightness must be 0-100, got {percent}"
        )));
    }
    let mut report = [0u8; FEATURE_REPORT_LEN];
    report[0] = 0x03;
    report[1] = 0x08;
    report[2] = percent;
    Ok(report)
}

/// `03 02`: reset the module to its logo screen.
pub fn reset_report() -> [u8; FEATURE_REPORT_LEN] {
    let mut report = [0u8; FEATURE_REPORT_LEN];
    report[0] = 0x03;
    report[1] = 0x02;
    report
}

/// `03 06 <key> <r> <g> <b>`: fill one key with a solid color without
/// uploading an image.
pub fn key_color_report(key: u8, r: u8, g: u8, b: u8) -> Result<[u8; FEATURE_REPORT_LEN], Error> {
    if key >= KEY_COUNT {
        return Err(Error::InvalidArgument(format!(
            "key must be 0-{}, got {key}",
            KEY_COUNT - 1
        )));
    }
    let mut report = [0u8; FEATURE_REPORT_LEN];
    report[0] = 0x03;
    report[1] = 0x06;
    report[2] = key;
    report[3] = r;
    report[4] = g;
    report[5] = b;
    Ok(report)
}

/// `03 24 <led> <r> <g> <b>`: set one encoder-ring LED pixel by its raw
/// hardware index. Encoder 0 owns pixels 4-7, encoder 1 owns pixels 0-3.
pub fn encoder_led_report(
    led_index: u8,
    r: u8,
    g: u8,
    b: u8,
) -> Result<[u8; FEATURE_REPORT_LEN], Error> {
    if led_index >= ENCODER_COUNT * ENCODER_RING_LEDS {
        return Err(Error::InvalidArgument(format!(
            "led index must be 0-{}, got {led_index}",
            ENCODER_COUNT * ENCODER_RING_LEDS - 1
        )));
    }
    let mut report = [0u8; FEATURE_REPORT_LEN];
    report[0] = 0x03;
    report[1] = 0x24;
    report[2] = led_index;
    report[3] = r;
    report[4] = g;
    report[5] = b;
    Ok(report)
}

/// Raw hardware LED index for each visual ring position, per encoder.
/// Row = encoder (0 left, 1 right); column = visual position 0-3 starting
/// at the TOP of the ring and proceeding CLOCKWISE. Validated visually on
/// physical hardware (firmware 3.05.003, 2026-09-03): the hardware index
/// order runs counter-clockwise around each ring, with a different start
/// offset per ring.
const RING_LED_CLOCKWISE_FROM_TOP: [[u8; ENCODER_RING_LEDS as usize]; ENCODER_COUNT as usize] =
    [[5, 4, 7, 6], [3, 2, 1, 0]];

/// Raw hardware LED index for a visual ring segment of an encoder.
/// `visual_segment` 0 is the top of the ring, proceeding clockwise.
pub fn encoder_ring_led_index(encoder: u8, visual_segment: u8) -> Result<u8, Error> {
    if encoder >= ENCODER_COUNT {
        return Err(Error::InvalidArgument(format!(
            "encoder must be 0-{}, got {encoder}",
            ENCODER_COUNT - 1
        )));
    }
    if visual_segment >= ENCODER_RING_LEDS {
        return Err(Error::InvalidArgument(format!(
            "ring segment must be 0-{}, got {visual_segment}",
            ENCODER_RING_LEDS - 1
        )));
    }
    Ok(RING_LED_CLOCKWISE_FROM_TOP[encoder as usize][visual_segment as usize])
}

/// Output reports for uploading a JPEG to one key. The JPEG must be
/// 160x160, baseline, no rotation or mirroring. Each report is exactly
/// 1024 bytes: `02 07 <key> <last> <len:u16le> <part:u16le>` + payload.
pub fn key_image_reports(key: u8, jpeg: &[u8]) -> Result<Vec<Vec<u8>>, Error> {
    if key >= KEY_COUNT {
        return Err(Error::InvalidArgument(format!(
            "key must be 0-{}, got {key}",
            KEY_COUNT - 1
        )));
    }
    if jpeg.is_empty() {
        return Err(Error::InvalidArgument("empty jpeg payload".into()));
    }

    let mut reports = Vec::with_capacity(jpeg.len().div_ceil(KEY_IMAGE_CHUNK));
    for (part, chunk) in jpeg.chunks(KEY_IMAGE_CHUNK).enumerate() {
        let is_last = (part + 1) * KEY_IMAGE_CHUNK >= jpeg.len();
        let mut report = vec![0u8; OUTPUT_REPORT_LEN];
        report[0] = 0x02;
        report[1] = 0x07;
        report[2] = key;
        report[3] = is_last as u8;
        report[4..6].copy_from_slice(&(chunk.len() as u16).to_le_bytes());
        report[6..8].copy_from_slice(&(part as u16).to_le_bytes());
        report[KEY_IMAGE_HEADER_LEN..KEY_IMAGE_HEADER_LEN + chunk.len()].copy_from_slice(chunk);
        reports.push(report);
    }
    Ok(reports)
}

/// Output reports for drawing a JPEG into a rectangle of the info-screen
/// segment. Rejects anything outside 720x384 — [`Lcd`](crate::controls::Lcd)
/// contracts for that region, and silently widening it would turn a caller
/// drawing at y=400 into one painting on the keys.
///
/// Use [`panel_region_reports`] to address the whole display.
pub fn lcd_region_reports(
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    jpeg: &[u8],
) -> Result<Vec<Vec<u8>>, Error> {
    if w == 0
        || h == 0
        || u32::from(x) + u32::from(w) > u32::from(LCD_WIDTH)
        || u32::from(y) + u32::from(h) > u32::from(LCD_HEIGHT)
    {
        return Err(Error::InvalidArgument(format!(
            "region {w}x{h}+{x}+{y} does not fit the {LCD_WIDTH}x{LCD_HEIGHT} lcd segment"
        )));
    }
    panel_region_reports(x, y, w, h, jpeg)
}

/// Output reports for drawing a JPEG anywhere on the physical panel.
///
/// The `02 0c` header carries x/y/w/h as plain u16 LE, so it addresses the
/// whole 720x1280 display — the info screen and the key area alike. The
/// JPEG dimensions must match `w` x `h`, and both should be multiples of
/// [`JPEG_MCU`]; the firmware shears images whose dimensions are not.
///
/// Each report is exactly 1024 bytes:
/// `02 0c <x:u16le> <y:u16le> <w:u16le> <h:u16le> <last> <part:u16le> <len:u16le> 00` + payload.
pub fn panel_region_reports(
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    jpeg: &[u8],
) -> Result<Vec<Vec<u8>>, Error> {
    if w == 0
        || h == 0
        || u32::from(x) + u32::from(w) > u32::from(PANEL_WIDTH)
        || u32::from(y) + u32::from(h) > u32::from(PANEL_HEIGHT)
    {
        return Err(Error::InvalidArgument(format!(
            "region {w}x{h}+{x}+{y} does not fit the {PANEL_WIDTH}x{PANEL_HEIGHT} panel"
        )));
    }
    if jpeg.is_empty() {
        return Err(Error::InvalidArgument("empty jpeg payload".into()));
    }

    let mut reports = Vec::with_capacity(jpeg.len().div_ceil(LCD_IMAGE_CHUNK));
    for (part, chunk) in jpeg.chunks(LCD_IMAGE_CHUNK).enumerate() {
        let is_last = (part + 1) * LCD_IMAGE_CHUNK >= jpeg.len();
        let mut report = vec![0u8; OUTPUT_REPORT_LEN];
        report[0] = 0x02;
        report[1] = 0x0c;
        report[2..4].copy_from_slice(&x.to_le_bytes());
        report[4..6].copy_from_slice(&y.to_le_bytes());
        report[6..8].copy_from_slice(&w.to_le_bytes());
        report[8..10].copy_from_slice(&h.to_le_bytes());
        report[10] = is_last as u8;
        report[11..13].copy_from_slice(&(part as u16).to_le_bytes());
        report[13..15].copy_from_slice(&(chunk.len() as u16).to_le_bytes());
        report[LCD_IMAGE_HEADER_LEN..LCD_IMAGE_HEADER_LEN + chunk.len()].copy_from_slice(chunk);
        reports.push(report);
    }
    Ok(reports)
}

/// One decoded input report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputReport {
    /// Full pressed-state snapshot of the 12 keys, row-major.
    ButtonStates([bool; KEY_COUNT as usize]),
    /// Pressed-state snapshot of the two encoders.
    EncoderStates([bool; ENCODER_COUNT as usize]),
    /// Signed rotation deltas per encoder (positive = clockwise).
    EncoderRotation([i8; ENCODER_COUNT as usize]),
    /// Short tap on the LCD segment.
    LcdShortPress { x: u16, y: u16 },
    /// Long press on the LCD segment.
    LcdLongPress { x: u16, y: u16 },
    /// Swipe across the LCD segment.
    LcdSwipe { from: (u16, u16), to: (u16, u16) },
}

fn u16_le(raw: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([raw[offset], raw[offset + 1]])
}

/// Parse a raw input report as read from hidraw (report id `0x01` at byte
/// 0). Returns `None` for reports we do not understand — unknown types are
/// expected (e.g. the protocol carries touchscreen and NFC event types the
/// hardware may never send) and must not be treated as errors.
pub fn parse_input(raw: &[u8]) -> Option<InputReport> {
    if raw.len() < 5 || raw[0] != 0x01 {
        return None;
    }
    match raw[1] {
        // Buttons: state bytes start 3 bytes after the type byte.
        0x00 => {
            if raw.len() < 4 + KEY_COUNT as usize {
                return None;
            }
            let mut states = [false; KEY_COUNT as usize];
            for (i, state) in states.iter_mut().enumerate() {
                *state = raw[4 + i] != 0;
            }
            Some(InputReport::ButtonStates(states))
        }
        // LCD touch events.
        0x02 => {
            if raw.len() < 10 {
                return None;
            }
            let x = u16_le(raw, 6);
            let y = u16_le(raw, 8);
            match raw[4] {
                1 => Some(InputReport::LcdShortPress { x, y }),
                2 => Some(InputReport::LcdLongPress { x, y }),
                3 => {
                    if raw.len() < 14 {
                        return None;
                    }
                    Some(InputReport::LcdSwipe {
                        from: (x, y),
                        to: (u16_le(raw, 10), u16_le(raw, 12)),
                    })
                }
                _ => None,
            }
        }
        // Encoders.
        0x03 => {
            if raw.len() < 5 + ENCODER_COUNT as usize {
                return None;
            }
            match raw[4] {
                0x00 => {
                    let mut states = [false; ENCODER_COUNT as usize];
                    for (i, state) in states.iter_mut().enumerate() {
                        *state = raw[5 + i] != 0;
                    }
                    Some(InputReport::EncoderStates(states))
                }
                0x01 => {
                    let mut deltas = [0i8; ENCODER_COUNT as usize];
                    for (i, delta) in deltas.iter_mut().enumerate() {
                        *delta = raw[5 + i] as i8;
                    }
                    Some(InputReport::EncoderRotation(deltas))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Parse the firmware version string out of a feature-report 5 buffer
/// (report id at byte 0, length at byte 1, 4 checksum bytes, then ASCII).
pub fn parse_firmware_version(buf: &[u8]) -> Option<String> {
    let end = (*buf.get(1)? as usize).checked_add(2)?;
    let bytes = buf.get(6..end.min(buf.len()))?;
    Some(
        String::from_utf8_lossy(bytes)
            .trim_end_matches('\0')
            .to_string(),
    )
}

/// Parse the serial number string out of a feature-report 6 buffer.
pub fn parse_serial_number(buf: &[u8]) -> Option<String> {
    let end = (*buf.get(1)? as usize).checked_add(2)?;
    let bytes = buf.get(2..end.min(buf.len()))?;
    Some(
        String::from_utf8_lossy(bytes)
            .trim_end_matches('\0')
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keepalive_matches_the_wire_format() {
        let report = keepalive_report();
        assert_eq!(report.len(), 32);
        assert_eq!(&report[..2], &[0x03, 0x27]);
        assert!(report[2..].iter().all(|b| *b == 0));
    }

    #[test]
    fn brightness_encodes_percentage_and_rejects_out_of_range() {
        let report = brightness_report(60).unwrap();
        assert_eq!(&report[..3], &[0x03, 0x08, 60]);
        assert!(brightness_report(101).is_err());
        brightness_report(0).unwrap();
        brightness_report(100).unwrap();
    }

    #[test]
    fn key_color_addresses_keys_directly() {
        let report = key_color_report(11, 1, 2, 3).unwrap();
        assert_eq!(&report[..6], &[0x03, 0x06, 11, 1, 2, 3]);
        assert!(key_color_report(12, 0, 0, 0).is_err());
    }

    #[test]
    fn ring_mapping_matches_observed_hardware() {
        // Validated visually on firmware 3.05.003: walking each visual
        // ring clockwise from the top must visit these hardware leds.
        let left: Vec<u8> = (0..4)
            .map(|v| encoder_ring_led_index(0, v).unwrap())
            .collect();
        let right: Vec<u8> = (0..4)
            .map(|v| encoder_ring_led_index(1, v).unwrap())
            .collect();
        assert_eq!(left, vec![5, 4, 7, 6]);
        assert_eq!(right, vec![3, 2, 1, 0]);

        // Together the two rings must cover all 8 leds exactly once.
        let mut all: Vec<u8> = left.iter().chain(right.iter()).copied().collect();
        all.sort_unstable();
        assert_eq!(all, (0..8).collect::<Vec<u8>>());

        assert!(encoder_ring_led_index(2, 0).is_err());
        assert!(encoder_ring_led_index(0, 4).is_err());
    }

    #[test]
    fn encoder_led_report_bounds() {
        let report = encoder_led_report(7, 9, 8, 7).unwrap();
        assert_eq!(&report[..6], &[0x03, 0x24, 7, 9, 8, 7]);
        assert!(encoder_led_report(8, 0, 0, 0).is_err());
    }

    #[test]
    fn key_image_single_chunk() {
        let jpeg = vec![0xAB; 100];
        let reports = key_image_reports(3, &jpeg).unwrap();
        assert_eq!(reports.len(), 1);
        let r = &reports[0];
        assert_eq!(r.len(), 1024);
        assert_eq!(&r[..4], &[0x02, 0x07, 3, 1]); // last flag set
        assert_eq!(u16::from_le_bytes([r[4], r[5]]), 100);
        assert_eq!(u16::from_le_bytes([r[6], r[7]]), 0);
        assert_eq!(&r[8..108], &jpeg[..]);
        assert!(r[108..].iter().all(|b| *b == 0));
    }

    #[test]
    fn key_image_chunking_boundaries() {
        // Exactly two full chunks: 2032 bytes.
        let jpeg = vec![0x11; 2 * 1016];
        let reports = key_image_reports(0, &jpeg).unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0][3], 0);
        assert_eq!(reports[1][3], 1);
        assert_eq!(u16::from_le_bytes([reports[1][4], reports[1][5]]), 1016);
        assert_eq!(u16::from_le_bytes([reports[1][6], reports[1][7]]), 1);

        // One byte over: three reports, last carries 1 byte.
        let jpeg = vec![0x11; 2 * 1016 + 1];
        let reports = key_image_reports(0, &jpeg).unwrap();
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[2][3], 1);
        assert_eq!(u16::from_le_bytes([reports[2][4], reports[2][5]]), 1);
    }

    #[test]
    fn lcd_region_header_layout() {
        let jpeg = vec![0xCD; 1500];
        let reports = lcd_region_reports(10, 20, 100, 50, &jpeg).unwrap();
        assert_eq!(reports.len(), 2);
        let r = &reports[0];
        assert_eq!(r.len(), 1024);
        assert_eq!(&r[..2], &[0x02, 0x0c]);
        assert_eq!(u16::from_le_bytes([r[2], r[3]]), 10);
        assert_eq!(u16::from_le_bytes([r[4], r[5]]), 20);
        assert_eq!(u16::from_le_bytes([r[6], r[7]]), 100);
        assert_eq!(u16::from_le_bytes([r[8], r[9]]), 50);
        assert_eq!(r[10], 0); // not last
        assert_eq!(u16::from_le_bytes([r[11], r[12]]), 0); // part
        assert_eq!(u16::from_le_bytes([r[13], r[14]]), 1008); // body length
        assert_eq!(&r[16..16 + 1008], &jpeg[..1008]);

        let last = &reports[1];
        assert_eq!(last[10], 1);
        assert_eq!(u16::from_le_bytes([last[11], last[12]]), 1);
        assert_eq!(u16::from_le_bytes([last[13], last[14]]), 1500 - 1008);
    }

    #[test]
    fn lcd_region_rejects_out_of_bounds() {
        let jpeg = vec![0u8; 10];
        assert!(lcd_region_reports(700, 0, 100, 50, &jpeg).is_err());
        assert!(lcd_region_reports(0, 380, 10, 10, &jpeg).is_err());
        assert!(lcd_region_reports(0, 0, 0, 10, &jpeg).is_err());
        // Near-u16::MAX coordinates must error, not overflow the check.
        assert!(lcd_region_reports(65500, 0, 100, 50, &jpeg).is_err());
        assert!(lcd_region_reports(0, 65500, 10, 100, &jpeg).is_err());
        lcd_region_reports(0, 0, 720, 384, &jpeg).unwrap();
    }

    #[test]
    fn panel_regions_reach_the_key_area_but_lcd_regions_do_not() {
        let jpeg = vec![0u8; 10];
        // The key area is below the info-screen segment. The panel path
        // must reach it; the Lcd path must keep refusing, or a consumer
        // drawing past 384 would silently start painting on the keys.
        let reports = panel_region_reports(0, 896, 720, 224, &jpeg).unwrap();
        assert_eq!(u16::from_le_bytes([reports[0][4], reports[0][5]]), 896);
        assert!(lcd_region_reports(0, 896, 720, 224, &jpeg).is_err());

        // Full panel accepts; one row past it does not.
        panel_region_reports(0, 0, 720, 1280, &jpeg).unwrap();
        assert!(panel_region_reports(0, 1200, 720, 200, &jpeg).is_err());
        assert!(panel_region_reports(0, 1280, 720, 8, &jpeg).is_err());
        // Width is unchanged: the panel is no wider than the segment.
        assert!(panel_region_reports(700, 0, 100, 50, &jpeg).is_err());
    }

    fn raw_report(event_type: u8, tail: &[u8]) -> Vec<u8> {
        // Reports arrive on a 512-byte endpoint; build a plausibly padded one.
        let mut raw = vec![0u8; 512];
        raw[0] = 0x01;
        raw[1] = event_type;
        raw[4..4 + tail.len()].copy_from_slice(tail);
        raw
    }

    #[test]
    fn parses_button_states() {
        let mut tail = [0u8; 12];
        tail[0] = 1;
        tail[11] = 1;
        let report = parse_input(&raw_report(0x00, &tail)).unwrap();
        let InputReport::ButtonStates(states) = report else {
            panic!("wrong variant")
        };
        assert!(states[0] && states[11]);
        assert!(!states[1..11].iter().any(|s| *s));
    }

    #[test]
    fn parses_encoder_press_and_rotation() {
        // Subtype byte sits at raw[4], encoder bytes at raw[5..].
        let report = parse_input(&raw_report(0x03, &[0x00, 1, 0])).unwrap();
        assert_eq!(report, InputReport::EncoderStates([true, false]));

        let report = parse_input(&raw_report(0x03, &[0x01, 0xFF, 2])).unwrap();
        assert_eq!(report, InputReport::EncoderRotation([-1, 2]));
    }

    #[test]
    fn parses_lcd_touch_events() {
        let mut tail = [0u8; 10];
        tail[0] = 1; // short press
        tail[2..4].copy_from_slice(&300u16.to_le_bytes());
        tail[4..6].copy_from_slice(&120u16.to_le_bytes());
        let report = parse_input(&raw_report(0x02, &tail)).unwrap();
        assert_eq!(report, InputReport::LcdShortPress { x: 300, y: 120 });

        tail[0] = 3; // swipe
        tail[6..8].copy_from_slice(&500u16.to_le_bytes());
        tail[8..10].copy_from_slice(&200u16.to_le_bytes());
        let report = parse_input(&raw_report(0x02, &tail)).unwrap();
        assert_eq!(
            report,
            InputReport::LcdSwipe {
                from: (300, 120),
                to: (500, 200)
            }
        );
    }

    #[test]
    fn ignores_unknown_and_malformed_reports() {
        assert_eq!(parse_input(&[]), None);
        assert_eq!(parse_input(&[0x01]), None);
        assert_eq!(parse_input(&raw_report(0x7F, &[])), None); // unknown type
        assert_eq!(parse_input(&raw_report(0x02, &[9, 0, 0, 0, 0, 0])), None); // unknown subtype
        let mut not_input = raw_report(0x00, &[0u8; 12]);
        not_input[0] = 0x02;
        assert_eq!(parse_input(&not_input), None); // wrong report id
    }

    #[test]
    fn parses_version_strings() {
        // Report 5: id, payload length, 4 checksum bytes, ascii version.
        let mut buf = [0u8; 32];
        buf[0] = 5;
        buf[1] = 12; // string runs to index 14
        buf[6..14].copy_from_slice(b"3.06.005");
        assert_eq!(parse_firmware_version(&buf).unwrap(), "3.06.005");

        // Report 6: id, payload length, ascii serial.
        let mut buf = [0u8; 32];
        buf[0] = 6;
        buf[1] = 10;
        buf[2..12].copy_from_slice(b"A1B2C3D4E5");
        assert_eq!(parse_serial_number(&buf).unwrap(), "A1B2C3D4E5");

        assert_eq!(parse_firmware_version(&[]), None);
    }
}
