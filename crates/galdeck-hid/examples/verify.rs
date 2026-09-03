//! Hardware verification harness for the Galleon 100 SD Stream Deck module.
//!
//! Run with the keyboard plugged in and the udev rule installed:
//!
//! ```sh
//! cargo run -p galdeck-hid --example verify            # full checkout, ~1 min
//! cargo run -p galdeck-hid --example verify -- --soak  # + 6-minute keepalive soak
//! ```
//!
//! Exercises every protocol surface in order, printing a checkpoint per
//! step, then drops into an interactive event loop. This run doubles as an
//! independent verification of the community protocol documentation — if
//! it passes on your firmware, please report firmware version + result in
//! the project issues.

use std::time::{Duration, Instant};

use galdeck_hid::ids::{KEY_COUNT, KEY_PIXELS, LCD_HEIGHT, LCD_WIDTH, VALIDATED_FIRMWARES};
use galdeck_hid::{encode_jpeg_rgb, Event, Galleon};

fn checkpoint(name: &str) {
    println!("==> {name}");
}

/// Wait while keeping the module in software mode (a bare sleep would let
/// the keepalive lapse).
fn hold(deck: &mut Galleon, duration: Duration) -> Result<(), galdeck_hid::Error> {
    let start = Instant::now();
    while start.elapsed() < duration {
        deck.poll(Duration::from_millis(200))?;
    }
    Ok(())
}

/// A flat RGB buffer filled with one color.
fn solid_rgb(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    rgb.iter()
        .copied()
        .cycle()
        .take((w * h * 3) as usize)
        .collect()
}

/// Distinct test card per key: hue by index, black diagonal, index encoded
/// as white corner dots (count = key % 3 + 1, row of dots = key / 3).
fn key_test_card(key: u8) -> Vec<u8> {
    let hue = key as f32 / KEY_COUNT as f32;
    let (r, g, b) = hsv_to_rgb(hue, 0.9, 0.9);
    let size = KEY_PIXELS as usize;
    let mut buf = solid_rgb(KEY_PIXELS, KEY_PIXELS, [r, g, b]);
    for i in 0..size {
        for t in 0..6 {
            let x = (i + t).min(size - 1);
            let off = (i * size + x) * 3;
            buf[off..off + 3].copy_from_slice(&[0, 0, 0]);
        }
    }
    let row = (key / 3) as usize;
    let dots = (key % 3 + 1) as usize;
    for d in 0..dots {
        for y in 0..14 {
            for x in 0..14 {
                let px = 8 + d * 20 + x;
                let py = 8 + row * 20 + y;
                let off = (py * size + px) * 3;
                buf[off..off + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
    }
    buf
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match (i as i32) % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// Blocking countdown with one audio cue per second, so the observer has
/// time to look at the device before a visual test starts.
fn countdown() {
    for i in (1..=3).rev() {
        println!("    starting in {i}...");
        let _ = std::process::Command::new("pw-play")
            .arg("/usr/share/sounds/freedesktop/stereo/message.oga")
            .status();
        std::thread::sleep(Duration::from_millis(900));
    }
}

/// `--rings`: hold the ring-rotation test card for 30s and nothing else.
fn rings_only(deck: &mut Galleon) -> Result<(), Box<dyn std::error::Error>> {
    checkpoint("get ready — watch the ENCODER RINGS");
    countdown();
    checkpoint("ring rotation check — per ring, clockwise from TOP: Red Green Blue White");
    let segment_colors: [[u8; 3]; 4] = [[200, 0, 0], [0, 200, 0], [0, 0, 200], [200, 200, 200]];
    for encoder in 0..2u8 {
        for (segment, c) in segment_colors.iter().enumerate() {
            deck.set_encoder_ring_segment(encoder, segment as u8, c[0], c[1], c[2])?;
        }
    }
    println!("    holding for 30s — check BOTH rings: top=Red, then Green, Blue, White clockwise");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        deck.poll(Duration::from_secs(1))?;
    }
    deck.set_encoder_ring(0, 0, 0, 0)?;
    deck.set_encoder_ring(1, 0, 0, 0)?;
    Ok(())
}

/// `--ring-probe`: characterize the 03 24 LED addressing — light raw
/// hardware led indices 0..7 one at a time (red, 3 s each, cleared in
/// between) so an observer can note what each index actually controls.
fn ring_probe(deck: &mut Galleon) -> Result<(), Box<dyn std::error::Error>> {
    use galdeck_hid::protocol;
    checkpoint("ring LED probe — raw indices 0..7, red, 3s each");
    println!("    note per index: which ring lights, one segment or the whole ring, and where");
    for index in 0..8u8 {
        // Clear everything via raw writes (avoid the visual-rotation mapping
        // under test).
        for clear in 0..8u8 {
            deck.send_feature_report_raw(&protocol::encoder_led_report(clear, 0, 0, 0)?)?;
        }
        std::thread::sleep(Duration::from_millis(300));
        println!("    >>> raw led index {index} = RED");
        deck.send_feature_report_raw(&protocol::encoder_led_report(index, 200, 0, 0)?)?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            deck.poll(Duration::from_millis(500))?;
        }
    }
    for clear in 0..8u8 {
        deck.send_feature_report_raw(&protocol::encoder_led_report(clear, 0, 0, 0)?)?;
    }
    println!("    probe done — all cleared");
    Ok(())
}

/// Fire-and-forget audio cue so the observer can keep their eyes on the
/// device; `count` short beeps identify the phase.
fn beep(count: u32) {
    std::thread::spawn(move || {
        for _ in 0..count {
            let _ = std::process::Command::new("pw-play")
                .arg("/usr/share/sounds/freedesktop/stereo/message.oga")
                .status();
        }
    });
}

/// `--ring-hold`: isolate what clears the ring pattern — time, or our own
/// keepalive traffic. Phases are announced by beep count (1/2/3), end by a
/// long alarm sound; the observer only watches the rings.
fn ring_hold(deck: &mut Galleon) -> Result<(), Box<dyn std::error::Error>> {
    const PHASE: Duration = Duration::from_secs(12);
    let segment_colors: [[u8; 3]; 4] = [[200, 0, 0], [0, 200, 0], [0, 0, 200], [200, 200, 200]];
    let set_pattern = |deck: &mut Galleon| -> Result<(), galdeck_hid::Error> {
        for encoder in 0..2u8 {
            for (segment, c) in segment_colors.iter().enumerate() {
                deck.set_encoder_ring_segment(encoder, segment as u8, c[0], c[1], c[2])?;
            }
        }
        Ok(())
    };

    checkpoint("get ready — watching the ENCODER RINGS; starting after the countdown beeps");
    countdown();

    checkpoint("1 beep = phase A (12s): pattern set, then TOTAL SILENCE (no keepalives)");
    beep(1);
    set_pattern(deck)?;
    std::thread::sleep(PHASE);

    checkpoint("2 beeps = phase B (12s): keepalives RESUME, nothing else sent");
    beep(2);
    deck.send_keepalive()?;
    let start = Instant::now();
    while start.elapsed() < PHASE {
        deck.poll(Duration::from_millis(500))?;
    }

    checkpoint("3 beeps = phase C (12s): pattern RE-SET, keepalives keep running");
    beep(3);
    set_pattern(deck)?;
    let start = Instant::now();
    while start.elapsed() < PHASE {
        deck.poll(Duration::from_millis(500))?;
    }

    let _ = std::process::Command::new("pw-play")
        .arg("/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga")
        .status();
    deck.set_encoder_ring(0, 0, 0, 0)?;
    deck.set_encoder_ring(1, 0, 0, 0)?;
    println!("    done — rings cleared");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let soak = std::env::args().any(|a| a == "--soak");
    let rings = std::env::args().any(|a| a == "--rings");
    let probe = std::env::args().any(|a| a == "--ring-probe");
    let hold_diag = std::env::args().any(|a| a == "--ring-hold");

    checkpoint("enumerate & open (1b1c:2b18 interface 0)");
    let api = hidapi::HidApi::new()?;
    let paths = Galleon::list(&api);
    println!("    found {} module(s): {paths:?}", paths.len());
    let mut deck = Galleon::open(&api)?;

    if rings {
        return rings_only(&mut deck);
    }
    if probe {
        return ring_probe(&mut deck);
    }
    if hold_diag {
        return ring_hold(&mut deck);
    }

    checkpoint("firmware & serial (feature reports 5/6)");
    let firmware = deck.firmware_version()?;
    let serial = deck.serial_number()?;
    println!("    firmware: {firmware}   serial: {serial}");
    if !VALIDATED_FIRMWARES.contains(&firmware.as_str()) {
        println!(
            "    !! firmware differs from the validated {VALIDATED_FIRMWARES:?} — if later steps"
        );
        println!("    !! fail (especially input timing out), the keepalive may have changed;");
        println!("    !! please open an issue with this firmware version either way.");
    }

    checkpoint("brightness 60% (03 08)");
    deck.set_brightness(60)?;

    checkpoint("solid key colors (03 06) — expect a 3x4 rainbow");
    for key in 0..KEY_COUNT {
        let (r, g, b) = hsv_to_rgb(key as f32 / KEY_COUNT as f32, 0.9, 0.9);
        deck.fill_key_color(key, r, g, b)?;
    }
    hold(&mut deck, Duration::from_secs(2))?;

    checkpoint("JPEG key upload (02 07) — expect test cards with corner dots");
    println!("    dot count = column + 1, dot row = key row");
    for key in 0..KEY_COUNT {
        deck.set_key_rgb(key, &key_test_card(key))?;
    }
    hold(&mut deck, Duration::from_secs(2))?;

    checkpoint("LCD full fill (02 0c, 720x384) — expect a horizontal gradient");
    let mut lcd = Vec::with_capacity(LCD_WIDTH as usize * LCD_HEIGHT as usize * 3);
    for y in 0..LCD_HEIGHT as u32 {
        for x in 0..LCD_WIDTH as u32 {
            let (r, g, b) = hsv_to_rgb(x as f32 / LCD_WIDTH as f32, 0.8, 0.9);
            let dim = 0.3 + 0.7 * (y as f32 / LCD_HEIGHT as f32);
            lcd.extend_from_slice(&[
                (r as f32 * dim) as u8,
                (g as f32 * dim) as u8,
                (b as f32 * dim) as u8,
            ]);
        }
    }
    let jpeg = encode_jpeg_rgb(LCD_WIDTH as u32, LCD_HEIGHT as u32, &lcd)?;
    println!(
        "    ({} byte jpeg, {} reports)",
        jpeg.len(),
        jpeg.len().div_ceil(1008)
    );
    deck.set_lcd_region_jpeg(0, 0, LCD_WIDTH, LCD_HEIGHT, &jpeg)?;
    hold(&mut deck, Duration::from_secs(1))?;

    checkpoint("LCD region update — expect a white 100x100 square at (20,20)");
    deck.set_lcd_region_rgb(20, 20, 100, 100, &solid_rgb(100, 100, [255, 255, 255]))?;

    checkpoint("encoder rings solid (03 24) — left red, right blue");
    deck.set_encoder_ring(0, 200, 0, 0)?;
    deck.set_encoder_ring(1, 0, 0, 200)?;
    hold(&mut deck, Duration::from_secs(2))?;

    checkpoint("encoder ring rotation check — per ring, clockwise from top: R G B W");
    println!("    (if the order is rotated, the visual-rotation constants need adjusting)");
    let segment_colors: [[u8; 3]; 4] = [[200, 0, 0], [0, 200, 0], [0, 0, 200], [200, 200, 200]];
    for encoder in 0..2u8 {
        for (segment, c) in segment_colors.iter().enumerate() {
            deck.set_encoder_ring_segment(encoder, segment as u8, c[0], c[1], c[2])?;
        }
    }

    if soak {
        checkpoint("keepalive soak: 6 minutes in software mode");
        let start = Instant::now();
        let mut minutes_reported = 0;
        while start.elapsed() < Duration::from_secs(360) {
            let events = deck.poll(Duration::from_secs(5))?;
            for e in &events {
                println!("    [{}s] {e:?}", start.elapsed().as_secs());
            }
            let minutes = start.elapsed().as_secs() / 60;
            if minutes > minutes_reported {
                minutes_reported = minutes;
                println!("    {minutes} min — still in software mode");
            }
        }
        println!("    soak passed: module held software mode for 6 minutes");
    }

    checkpoint("interactive: 30s event loop");
    println!("    press keys / encoders, rotate, touch the LCD — events print below;");
    println!("    pressing a key flashes it white");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        for event in deck.poll(Duration::from_secs(1))? {
            println!("    {event:?}");
            if let Event::KeyDown(key) = event {
                deck.fill_key_color(key, 255, 255, 255)?;
            }
            if let Event::KeyUp(key) = event {
                deck.set_key_rgb(key, &key_test_card(key))?;
            }
        }
    }

    checkpoint("cleanup: clear panel");
    deck.clear_all()?;

    println!("\nall protocol surfaces exercised — firmware {firmware}. please report this result!");
    Ok(())
}
