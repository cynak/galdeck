//! A hardware framework for the Elgato Stream Deck module built into the
//! Corsair Galleon 100 SD keyboard (`1b1c:2b18`, HID interface 0).
//!
//! This crate owns the hardware layer and nothing above it: talking to the
//! device, holding it in software mode, drawing pixels and lighting LEDs,
//! and turning HID reports into events. Deciding *what* to draw — key
//! mappings, widgets, themes, animations — belongs to a consumer built on
//! top; [`galdeck-daemon`] in this repository is one such consumer.
//!
//! # The controls
//!
//! Open the device, then borrow the control you want:
//!
//! | Control | Handle | Hardware |
//! |---|---|---|
//! | Keys | [`Buttons`] / [`Button`] | 12 keys, 3x4, each a 160x160 display |
//! | Info screen | [`Lcd`] | one 720x384 drawable region |
//! | Knobs | [`Encoders`] / [`Encoder`] / [`Ring`] | 2 push-click encoders, 4 RGB LEDs each |
//!
//! ```no_run
//! use galdeck_hid::{Align, Event, Galleon, Rgb, TextStyle};
//! use std::time::Duration;
//!
//! let api = hidapi::HidApi::new()?;
//! let mut deck = Galleon::open(&api)?;
//! deck.set_brightness(70)?;
//!
//! // Light a key, and draw on another.
//! deck.button(0)?.set_color(Rgb::from_hex("#1d3b53").unwrap())?;
//!
//! let mut canvas = deck.button(1)?.canvas();
//! canvas.fill(Rgb::new(20, 20, 28));
//! canvas.draw_line((10, 150), (150, 10), Rgb::GREEN);
//! if let Some(font) = galdeck_hid::Font::system() {
//!     let style = TextStyle::new(&font, 28.0).align(Align::Center);
//!     canvas.draw_text("Ready", 80, 130, &style);
//! }
//! deck.button(1)?.draw(&canvas)?;
//!
//! // Show a level on the left knob's ring.
//! deck.encoder(0)?.ring().set_level(0.5, Rgb::GREEN, Rgb::BLACK)?;
//!
//! // React to input.
//! for event in deck.poll(Duration::from_secs(5))? {
//!     match event {
//!         Event::KeyDown(key) => println!("key {key} pressed"),
//!         Event::EncoderRotate(knob, delta) => println!("knob {knob} moved {delta}"),
//!         _ => {}
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Keeping the module awake
//!
//! The module only accepts drawing while in *software mode*, which it
//! leaves unless it receives a keepalive roughly twice a second. Every
//! drawing call and [`Galleon::poll`] refreshes it for you; the one thing
//! to avoid is going quiet for long stretches while holding the device.
//! After a gap, [`Galleon::take_mode_reentry`] reports that the firmware
//! reset its own state and your content needs redrawing.
//!
//! # Protocol
//!
//! The wire format is documented in `docs/protocol.md` and implemented in
//! [`protocol`], which is pure functions over byte buffers — useful if you
//! are porting to another language or transport. Hardware-validated on
//! firmware 3.05.003; see [`ids::VALIDATED_FIRMWARES`].
//!
//! [`galdeck-daemon`]: https://github.com/cynak/galdeck

pub mod canvas;
pub mod color;
pub mod controls;
pub mod device;
pub mod error;
pub mod ids;
pub mod protocol;
pub mod text;

pub use canvas::Canvas;
pub use color::Rgb;
pub use controls::{Button, Buttons, Encoder, Encoders, Lcd, Ring};
pub use device::{Event, Galleon};
pub use error::Error;
pub use text::{Align, Font, TextStyle};
