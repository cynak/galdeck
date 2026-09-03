//! The device handle: opening, keepalive, IO, and event decoding.

use std::time::{Duration, Instant};

use hidapi::{DeviceInfo, HidApi, HidDevice};

use crate::error::Error;
use crate::ids::*;
use crate::protocol::{self, InputReport};

/// Granularity of the poll loop; keepalives are refreshed at least this
/// often while polling.
const POLL_SLICE: Duration = Duration::from_millis(100);

/// A decoded, state-diffed input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    KeyDown(u8),
    KeyUp(u8),
    EncoderDown(u8),
    EncoderUp(u8),
    /// Positive delta = clockwise.
    EncoderRotate(u8, i8),
    LcdShortPress {
        x: u16,
        y: u16,
    },
    LcdLongPress {
        x: u16,
        y: u16,
    },
    LcdSwipe {
        from: (u16, u16),
        to: (u16, u16),
    },
}

/// Handle to the Stream Deck module. Single-threaded by design: keep the
/// handle on one thread and drive [`Galleon::poll`] regularly (it refreshes
/// the keepalive that holds the module in software mode).
pub struct Galleon {
    device: HidDevice,
    last_keepalive: Instant,
    /// False for passive handles ([`Galleon::open_passive`]): no keepalive
    /// is ever sent, so the module is left in whatever mode it was in.
    keepalive_enabled: bool,
    /// Set when a keepalive followed a gap long enough that the module had
    /// dropped out of software mode; see [`Galleon::take_mode_reentry`].
    mode_reentered: bool,
    key_states: [bool; KEY_COUNT as usize],
    encoder_states: [bool; ENCODER_COUNT as usize],
}

fn to_cpath(path: &str) -> Result<std::ffi::CString, Error> {
    std::ffi::CString::new(path)
        .map_err(|_| Error::InvalidArgument(format!("bad device path {path:?}")))
}

fn is_galleon_control_interface(info: &DeviceInfo) -> bool {
    info.vendor_id() == VENDOR_ID
        && info.product_id() == PRODUCT_ID
        && info.interface_number() == CONTROL_INTERFACE
}

impl Galleon {
    /// Device paths of all connected Galleon Stream Deck modules (control
    /// interface only).
    pub fn list(api: &HidApi) -> Vec<String> {
        api.device_list()
            .filter(|info| is_galleon_control_interface(info))
            .map(|info| info.path().to_string_lossy().into_owned())
            .collect()
    }

    /// Open the first connected module and switch it into software mode.
    pub fn open(api: &HidApi) -> Result<Self, Error> {
        let info = api
            .device_list()
            .find(|info| is_galleon_control_interface(info))
            .ok_or(Error::DeviceNotFound)?;
        Self::open_device(info.open_device(api)?, false)
    }

    /// Open a specific module by hidraw path (as returned by [`Galleon::list`]).
    pub fn open_path(api: &HidApi, path: &str) -> Result<Self, Error> {
        Self::open_device(api.open_path(&to_cpath(path)?)?, false)
    }

    /// Open a module without sending any keepalive: the module stays in
    /// whatever mode it is in (normally hardware mode). Identity getters
    /// ([`Galleon::firmware_version`], [`Galleon::serial_number`]) work on
    /// a passive handle; drawing commands are pointless outside software
    /// mode.
    pub fn open_passive(api: &HidApi, path: &str) -> Result<Self, Error> {
        Self::open_device(api.open_path(&to_cpath(path)?)?, true)
    }

    fn open_device(device: HidDevice, passive: bool) -> Result<Self, Error> {
        // The device needs a moment after open before it accepts traffic.
        std::thread::sleep(SETTLE_DELAY);

        let mut galleon = Galleon {
            device,
            last_keepalive: Instant::now(),
            keepalive_enabled: !passive,
            mode_reentered: false,
            key_states: [false; KEY_COUNT as usize],
            encoder_states: [false; ENCODER_COUNT as usize],
        };
        if !passive {
            // The first keepalive switches the module into software mode;
            // give the mode transition time to finish, or the firmware's
            // own entry state (e.g. white ring LEDs) wipes what we draw.
            galleon.send_keepalive()?;
            std::thread::sleep(SOFTWARE_MODE_ENTRY_SETTLE);
        }
        Ok(galleon)
    }

    /// All feature reports go through here: the device mishandles bursts of
    /// feature reports (observed on firmware 3.05.003: rapid 03 24 ring-LED
    /// writes all end up showing the last color), so consecutive sends are
    /// spaced by a moment — the reference implementation does the same.
    fn send_feature(&mut self, report: &[u8]) -> Result<(), Error> {
        self.device.send_feature_report(report)?;
        std::thread::sleep(FEATURE_REPORT_SPACING);
        Ok(())
    }

    /// Send the keepalive immediately, regardless of the interval. If the
    /// gap since the previous keepalive was long enough for the module to
    /// have dropped out of software mode, this waits out the re-entry
    /// transition and records it (see [`Galleon::take_mode_reentry`]).
    pub fn send_keepalive(&mut self) -> Result<(), Error> {
        let reentry = self.last_keepalive.elapsed() >= SOFTWARE_MODE_REENTRY_GAP;
        self.send_feature(&protocol::keepalive_report())?;
        self.last_keepalive = Instant::now();
        if reentry {
            std::thread::sleep(SOFTWARE_MODE_ENTRY_SETTLE);
            self.mode_reentered = true;
        }
        Ok(())
    }

    /// Send the keepalive if the interval has elapsed. Called internally by
    /// every drawing command and by [`Galleon::poll`], so the 500 ms
    /// cadence is maintained even during long upload sequences; call it
    /// yourself only around long stretches of your own non-device work.
    /// No-op on passive handles.
    pub fn tick_keepalive(&mut self) -> Result<(), Error> {
        if self.keepalive_enabled && self.last_keepalive.elapsed() >= KEEPALIVE_INTERVAL {
            self.send_keepalive()?;
        }
        Ok(())
    }

    /// True once, after a keepalive followed a gap long enough that the
    /// module re-entered software mode. On re-entry the firmware asserts
    /// its own state (observed: ring LEDs turn white), so redraw
    /// everything when this fires.
    pub fn take_mode_reentry(&mut self) -> bool {
        std::mem::take(&mut self.mode_reentered)
    }

    /// Wait up to `timeout` for input events, refreshing the keepalive
    /// while waiting. Returns as soon as at least one event is decoded;
    /// returns an empty vec on timeout. Safe to call with long timeouts.
    pub fn poll(&mut self, timeout: Duration) -> Result<Vec<Event>, Error> {
        let started = Instant::now();
        let mut buf = [0u8; INPUT_REPORT_LEN];
        loop {
            self.tick_keepalive()?;

            let remaining = timeout.saturating_sub(started.elapsed());
            let slice = remaining.min(POLL_SLICE);
            let len = self
                .device
                .read_timeout(&mut buf, slice.as_millis() as i32)?;
            if len > 0 {
                let events = self.decode(&buf[..len]);
                if !events.is_empty() {
                    return Ok(events);
                }
            }
            if started.elapsed() >= timeout {
                return Ok(Vec::new());
            }
        }
    }

    fn decode(&mut self, raw: &[u8]) -> Vec<Event> {
        let mut events = Vec::new();
        match protocol::parse_input(raw) {
            Some(InputReport::ButtonStates(states)) => {
                for (i, pressed) in states.iter().enumerate() {
                    if *pressed != self.key_states[i] {
                        self.key_states[i] = *pressed;
                        events.push(if *pressed {
                            Event::KeyDown(i as u8)
                        } else {
                            Event::KeyUp(i as u8)
                        });
                    }
                }
            }
            Some(InputReport::EncoderStates(states)) => {
                for (i, pressed) in states.iter().enumerate() {
                    if *pressed != self.encoder_states[i] {
                        self.encoder_states[i] = *pressed;
                        events.push(if *pressed {
                            Event::EncoderDown(i as u8)
                        } else {
                            Event::EncoderUp(i as u8)
                        });
                    }
                }
            }
            Some(InputReport::EncoderRotation(deltas)) => {
                for (i, delta) in deltas.iter().enumerate() {
                    if *delta != 0 {
                        events.push(Event::EncoderRotate(i as u8, *delta));
                    }
                }
            }
            Some(InputReport::LcdShortPress { x, y }) => events.push(Event::LcdShortPress { x, y }),
            Some(InputReport::LcdLongPress { x, y }) => events.push(Event::LcdLongPress { x, y }),
            Some(InputReport::LcdSwipe { from, to }) => events.push(Event::LcdSwipe { from, to }),
            None => {}
        }
        events
    }

    /// Panel brightness, 0-100.
    pub fn set_brightness(&mut self, percent: u8) -> Result<(), Error> {
        self.tick_keepalive()?;
        self.send_feature(&protocol::brightness_report(percent)?)?;
        Ok(())
    }

    /// Fill one key (0-11, row-major) with a solid color.
    pub fn fill_key_color(&mut self, key: u8, r: u8, g: u8, b: u8) -> Result<(), Error> {
        self.tick_keepalive()?;
        self.send_feature(&protocol::key_color_report(key, r, g, b)?)?;
        Ok(())
    }

    /// Upload a 160x160 JPEG to one key.
    pub fn set_key_jpeg(&mut self, key: u8, jpeg: &[u8]) -> Result<(), Error> {
        for report in protocol::key_image_reports(key, jpeg)? {
            self.tick_keepalive()?;
            self.device.write(&report)?;
        }
        Ok(())
    }

    /// Draw a JPEG into a rectangle of the 720x384 LCD segment.
    pub fn set_lcd_region_jpeg(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        jpeg: &[u8],
    ) -> Result<(), Error> {
        for report in protocol::lcd_region_reports(x, y, w, h, jpeg)? {
            self.tick_keepalive()?;
            self.device.write(&report)?;
        }
        Ok(())
    }

    /// Set one visual ring segment (0 = top, clockwise) of an encoder.
    pub fn set_encoder_ring_segment(
        &mut self,
        encoder: u8,
        visual_segment: u8,
        r: u8,
        g: u8,
        b: u8,
    ) -> Result<(), Error> {
        let led = protocol::encoder_ring_led_index(encoder, visual_segment)?;
        self.tick_keepalive()?;
        self.send_feature(&protocol::encoder_led_report(led, r, g, b)?)?;
        Ok(())
    }

    /// Set a whole encoder ring to one color.
    pub fn set_encoder_ring(&mut self, encoder: u8, r: u8, g: u8, b: u8) -> Result<(), Error> {
        for segment in 0..ENCODER_RING_LEDS {
            self.set_encoder_ring_segment(encoder, segment, r, g, b)?;
        }
        Ok(())
    }

    /// Blank all keys, rings, and the LCD segment.
    pub fn clear_all(&mut self) -> Result<(), Error> {
        for key in 0..KEY_COUNT {
            self.fill_key_color(key, 0, 0, 0)?;
        }
        for encoder in 0..ENCODER_COUNT {
            self.set_encoder_ring(encoder, 0, 0, 0)?;
        }
        #[cfg(feature = "encode")]
        {
            let black = vec![0u8; (LCD_WIDTH as usize) * (LCD_HEIGHT as usize) * 3];
            let jpeg = encode_jpeg_rgb(LCD_WIDTH as u32, LCD_HEIGHT as u32, &black)?;
            self.set_lcd_region_jpeg(0, 0, LCD_WIDTH, LCD_HEIGHT, &jpeg)?;
        }
        Ok(())
    }

    /// Send a raw feature report as-is. Escape hatch for protocol probing
    /// (e.g. the `verify --ring-probe` harness); not needed in normal use.
    pub fn send_feature_report_raw(&mut self, report: &[u8]) -> Result<(), Error> {
        self.send_feature(report)
    }

    /// Reset the module to its built-in logo screen.
    pub fn reset_to_logo(&mut self) -> Result<(), Error> {
        self.send_feature(&protocol::reset_report())?;
        Ok(())
    }

    /// Firmware version of the module (feature report 5).
    pub fn firmware_version(&mut self) -> Result<String, Error> {
        let mut buf = [0u8; FEATURE_REPORT_LEN];
        buf[0] = 5;
        let len = self.device.get_feature_report(&mut buf)?;
        protocol::parse_firmware_version(&buf[..len])
            .ok_or(Error::MalformedReport("firmware version"))
    }

    /// Serial number of the module (feature report 6).
    pub fn serial_number(&mut self) -> Result<String, Error> {
        let mut buf = [0u8; FEATURE_REPORT_LEN];
        buf[0] = 6;
        let len = self.device.get_feature_report(&mut buf)?;
        protocol::parse_serial_number(&buf[..len]).ok_or(Error::MalformedReport("serial number"))
    }

    /// Render an RGB8 buffer to one key (encodes to JPEG internally).
    /// `rgb` must be exactly 160*160*3 bytes.
    #[cfg(feature = "encode")]
    pub fn set_key_rgb(&mut self, key: u8, rgb: &[u8]) -> Result<(), Error> {
        let expected = (KEY_PIXELS * KEY_PIXELS * 3) as usize;
        if rgb.len() != expected {
            return Err(Error::InvalidArgument(format!(
                "key image must be {expected} bytes (160x160 rgb), got {}",
                rgb.len()
            )));
        }
        let jpeg = encode_jpeg_rgb(KEY_PIXELS, KEY_PIXELS, rgb)?;
        self.set_key_jpeg(key, &jpeg)
    }

    /// Render an RGB8 buffer into a rectangle of the LCD segment.
    #[cfg(feature = "encode")]
    pub fn set_lcd_region_rgb(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        rgb: &[u8],
    ) -> Result<(), Error> {
        let expected = w as usize * h as usize * 3;
        if rgb.len() != expected {
            return Err(Error::InvalidArgument(format!(
                "lcd image must be {expected} bytes ({w}x{h} rgb), got {}",
                rgb.len()
            )));
        }
        let jpeg = encode_jpeg_rgb(w as u32, h as u32, rgb)?;
        self.set_lcd_region_jpeg(x, y, w, h, &jpeg)
    }
}

/// Encode an RGB8 pixel buffer as a baseline JPEG (quality 90).
#[cfg(feature = "encode")]
pub fn encode_jpeg_rgb(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
    image::ImageEncoder::write_image(encoder, rgb, width, height, image::ExtendedColorType::Rgb8)?;
    Ok(out)
}
