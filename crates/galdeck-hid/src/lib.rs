//! Userspace driver for the Elgato Stream Deck module built into the
//! Corsair Galleon 100 SD keyboard (`1b1c:2b18`, HID interface 0).
//!
//! The module speaks the Elgato Gen2 Main Protocol with three Corsair
//! deltas: a `03 27` feature-report keepalive every 500 ms (without which
//! the module falls back to hardware mode and stops reporting controls),
//! a mandatory interface-0 selection, and encoder ring LEDs addressed via
//! feature report `03 24`.
//!
//! Protocol facts were written up independently from the MIT-licensed
//! reference implementation in Julusian/node-elgato-stream-deck
//! (`packages/core/src/models/galleon-k100.ts`), the official Elgato Gen2
//! HID documentation, and community protocol notes. See `docs/protocol.md`
//! in the repository. Everything is validated only on firmware 3.06.005 so
//! far; newer firmware may change the keepalive (see the README).

pub mod device;
pub mod error;
pub mod ids;
pub mod protocol;

pub use device::{Event, Galleon};
pub use error::Error;

#[cfg(feature = "encode")]
pub use device::encode_jpeg_rgb;
