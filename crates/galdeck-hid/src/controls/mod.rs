//! The module's controls, as separate handles you borrow from the device:
//! [`Buttons`] (the 12 keys), [`Lcd`] (the info screen), and [`Encoders`]
//! (the two knobs and their LED rings).

mod buttons;
mod encoders;
mod lcd;

pub use buttons::{Button, Buttons};
pub use encoders::{Encoder, Encoders, Ring};
pub use lcd::Lcd;
