use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("hid error: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("no Galleon 100 SD stream deck module found (usb 1b1c:2b18 interface 0) — is the keyboard plugged in and the udev rule installed?")]
    DeviceNotFound,
    #[error("{0}")]
    InvalidArgument(String),
    #[error("device returned a malformed {0} report")]
    MalformedReport(&'static str),
    #[cfg(feature = "encode")]
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
}
