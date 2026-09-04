//! Read-only device identification: is the module here, and what firmware
//! is it running?
//!
//! ```sh
//! cargo run --example detect
//! ```
//!
//! Opens the module passively — no keepalive is sent, so it stays in
//! whatever mode it was in. Safe to run at any time, and the first thing
//! to try when setting up.

use galdeck::ids::VALIDATED_FIRMWARES;
use galdeck::Galleon;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api = galdeck::hidapi::HidApi::new()?;
    let paths = Galleon::list(&api);

    if paths.is_empty() {
        println!("no Galleon 100 SD Stream Deck module found (usb 1b1c:2b18)");
        println!();
        println!("  - is the keyboard plugged in?");
        println!("  - is the udev rule installed? (udev/70-galdeck.rules)");
        println!("  - after installing it: sudo udevadm control --reload && sudo udevadm trigger");
        return Ok(());
    }

    for path in paths {
        println!("module at {path}");
        match Galleon::open_passive(&api, &path) {
            Ok(mut deck) => {
                let firmware = deck.firmware_version()?;
                println!("  firmware: {firmware}");
                println!("  serial:   {}", deck.serial_number()?);
                if !VALIDATED_FIRMWARES.contains(&firmware.as_str()) {
                    println!("  note: validated firmwares are {VALIDATED_FIRMWARES:?} — please");
                    println!("        report how `cargo run --example verify` goes on yours");
                }
            }
            Err(e) => {
                println!("  open failed: {e}");
                println!("  (a permission error usually means the udev rule is missing)");
            }
        }
    }
    Ok(())
}
