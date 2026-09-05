//! The module's controls, as separate handles you borrow from the device:
//! [`Buttons`] (the 12 keys), [`Lcd`] (the info screen), [`Encoders`] (the
//! two knobs and their LED rings), and [`Panel`] (the whole physical
//! display underneath the info-screen abstraction).

mod buttons;
mod encoders;
mod lcd;
mod panel;

pub use buttons::{Button, Buttons};
pub use encoders::{Encoder, Encoders, Ring};
pub use lcd::Lcd;
pub use panel::Panel;
