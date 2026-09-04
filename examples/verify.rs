//! Hardware verification harness for the Galleon 100 SD Stream Deck module.
//!
//! Run with the keyboard plugged in and the udev rule installed:
//!
//! ```sh
//! cargo run --example verify              # full checkout, ~1 min
//! cargo run --example verify -- --soak    # + 6-minute keepalive soak
//! cargo run --example verify -- --rings   # just the ring orientation card
//! cargo run --example verify -- --ring-probe   # raw LED index probe
//! cargo run --example verify -- --ring-hold    # keepalive/LED interaction
//! ```
//!
//! Exercises every control through the public framework API, then drops
//! into an interactive event loop. If it passes on your firmware, please
//! report the firmware version and result in the project issues.

use std::time::{Duration, Instant};

use galdeck::ids::VALIDATED_FIRMWARES;
use galdeck::{Align, Button, Buttons, Canvas, Event, Font, Galleon, Rgb, TextStyle};

fn checkpoint(name: &str) {
    println!("==> {name}");
}

/// Wait while keeping the module in software mode (a bare sleep would let
/// the keepalive lapse).
fn hold(deck: &mut Galleon, duration: Duration) -> Result<(), galdeck::Error> {
    let start = Instant::now();
    while start.elapsed() < duration {
        deck.poll(Duration::from_millis(200))?;
    }
    Ok(())
}

/// Audio cue so the observer can watch the device instead of the terminal.
fn beep(count: u32) {
    std::thread::spawn(move || {
        for _ in 0..count {
            let _ = std::process::Command::new("pw-play")
                .arg("/usr/share/sounds/freedesktop/stereo/message.oga")
                .status();
        }
    });
}

fn countdown() {
    for i in (1..=3).rev() {
        println!("    starting in {i}...");
        let _ = std::process::Command::new("pw-play")
            .arg("/usr/share/sounds/freedesktop/stereo/message.oga")
            .status();
        std::thread::sleep(Duration::from_millis(900));
    }
}

/// Per-key test card: hue by index, a diagonal, and dots encoding the
/// key's grid position (dots across = column + 1, dot row = key row).
fn key_test_card(index: u8, font: Option<&Font>) -> Canvas {
    let (width, height) = Button::size();
    let mut canvas = Canvas::filled(
        width,
        height,
        Rgb::from_hsv(index as f32 / Buttons::COUNT as f32, 0.9, 0.85),
    );
    canvas.draw_line_thick((0, 0), (width as i32, height as i32), Rgb::BLACK, 5);

    let (column, row) = (index % Buttons::COLUMNS, index / Buttons::COLUMNS);
    for dot in 0..=column {
        canvas.fill_rect(8 + dot as i32 * 20, 8 + row as i32 * 20, 14, 14, Rgb::WHITE);
    }
    if let Some(font) = font {
        canvas.draw_text(
            &index.to_string(),
            width as i32 / 2,
            height as i32 - 30,
            &TextStyle::new(font, 34.0).align(Align::Center),
        );
    }
    canvas
}

/// `--rings`: hold the ring orientation card so it can be inspected.
fn rings_only(deck: &mut Galleon) -> Result<(), Box<dyn std::error::Error>> {
    checkpoint("get ready — watch the ENCODER RINGS");
    countdown();
    checkpoint("ring orientation — per ring, clockwise from TOP: Red Green Blue White");
    let card = [Rgb::RED, Rgb::GREEN, Rgb::BLUE, Rgb::WHITE];
    for index in 0..2u8 {
        deck.encoder(index)?.ring().set_segments(card)?;
    }
    println!("    holding 30s — expect top=Red, then Green, Blue, White clockwise");
    hold(deck, Duration::from_secs(30))?;
    deck.encoders().clear_rings()?;
    Ok(())
}

/// `--ring-probe`: light raw hardware LED indices one at a time to
/// characterize the `03 24` addressing on new firmware.
fn ring_probe(deck: &mut Galleon) -> Result<(), Box<dyn std::error::Error>> {
    use galdeck::protocol;
    checkpoint("ring LED probe — raw indices 0..7, red, 3s each");
    println!("    note per index: which ring lights, one segment or the whole ring, and where");
    let clear_all = |deck: &mut Galleon| -> Result<(), galdeck::Error> {
        for index in 0..8u8 {
            deck.send_feature_report_raw(&protocol::encoder_led_report(index, 0, 0, 0)?)?;
        }
        Ok(())
    };
    for index in 0..8u8 {
        clear_all(deck)?;
        std::thread::sleep(Duration::from_millis(300));
        println!("    >>> raw led index {index} = RED");
        deck.send_feature_report_raw(&protocol::encoder_led_report(index, 200, 0, 0)?)?;
        hold(deck, Duration::from_secs(3))?;
    }
    clear_all(deck)?;
    println!("    probe done — all cleared");
    Ok(())
}

/// `--ring-hold`: isolate what clears ring state — elapsed time, or the
/// software-mode re-entry that a keepalive triggers after a gap.
fn ring_hold(deck: &mut Galleon) -> Result<(), Box<dyn std::error::Error>> {
    const PHASE: Duration = Duration::from_secs(12);
    let card = [Rgb::RED, Rgb::GREEN, Rgb::BLUE, Rgb::WHITE];
    let set_pattern = |deck: &mut Galleon| -> Result<(), galdeck::Error> {
        for index in 0..2u8 {
            deck.encoder(index)?.ring().set_segments(card)?;
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
    hold(deck, PHASE)?;

    checkpoint("3 beeps = phase C (12s): pattern RE-SET, keepalives keep running");
    beep(3);
    set_pattern(deck)?;
    hold(deck, PHASE)?;

    let _ = std::process::Command::new("pw-play")
        .arg("/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga")
        .status();
    deck.encoders().clear_rings()?;
    println!("    done — rings cleared");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = |name: &str| std::env::args().any(|a| a == name);
    let (soak, rings, probe, hold_diag) = (
        arg("--soak"),
        arg("--rings"),
        arg("--ring-probe"),
        arg("--ring-hold"),
    );

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
    println!(
        "    firmware: {firmware}   serial: {}",
        deck.serial_number()?
    );
    if !VALIDATED_FIRMWARES.contains(&firmware.as_str()) {
        println!("    !! firmware differs from the validated {VALIDATED_FIRMWARES:?} — if later");
        println!("    !! steps fail (especially input timing out), the keepalive may have");
        println!("    !! changed; please open an issue with this firmware version either way.");
    }

    let font = Font::system();
    if font.is_none() {
        println!("    (no system font found — text steps will be skipped)");
    }

    checkpoint("brightness 60%");
    deck.set_brightness(60)?;

    checkpoint("solid key colors — expect a 3x4 rainbow");
    for index in Buttons::indices() {
        let hue = index as f32 / Buttons::COUNT as f32;
        deck.button(index)?
            .set_color(Rgb::from_hsv(hue, 0.9, 0.85))?;
    }
    hold(&mut deck, Duration::from_secs(2))?;

    checkpoint("key images — expect test cards; dots across = column + 1, dot row = key row");
    for index in Buttons::indices() {
        let card = key_test_card(index, font.as_ref());
        deck.button(index)?.draw(&card)?;
    }
    hold(&mut deck, Duration::from_secs(2))?;

    checkpoint("key grid mapping — top-left key flashes white, then bottom-right");
    for (column, row) in [(0, 0), (Buttons::COLUMNS - 1, Buttons::ROWS - 1)] {
        deck.buttons().at(column, row)?.set_color(Rgb::WHITE)?;
        hold(&mut deck, Duration::from_millis(900))?;
        let index = row * Buttons::COLUMNS + column;
        deck.button(index)?
            .draw(&key_test_card(index, font.as_ref()))?;
    }

    checkpoint("info screen — full redraw: gradient, frame, crosshair, label");
    let mut screen = deck.lcd().canvas();
    let (width, height) = (screen.width(), screen.height());
    for x in 0..width {
        let color = Rgb::from_hsv(x as f32 / width as f32, 0.75, 0.9);
        for y in 0..height {
            let dim = 0.35 + 0.65 * (y as f32 / height as f32);
            screen.set_pixel(x as i32, y as i32, color.scaled(dim));
        }
    }
    screen.draw_rect(0, 0, width, height, Rgb::WHITE);
    screen.draw_line((0, 0), (width as i32, height as i32), Rgb::BLACK);
    screen.draw_line((width as i32, 0), (0, height as i32), Rgb::BLACK);
    screen.fill_circle((width as i32 / 2, height as i32 / 2), 40, Rgb::BLACK);
    if let Some(font) = font.as_ref() {
        screen.draw_text(
            "galdeck",
            width as i32 / 2,
            height as i32 / 2,
            &TextStyle::new(font, 56.0).align(Align::Center),
        );
    }
    deck.lcd().draw(&screen)?;
    hold(&mut deck, Duration::from_secs(2))?;

    checkpoint("info screen — partial region update: white square near the top-left");
    let patch = Canvas::filled(100, 100, Rgb::WHITE);
    deck.lcd().draw_at(20, 20, &patch)?;
    hold(&mut deck, Duration::from_secs(1))?;

    checkpoint("encoder rings — solid: left red, right blue");
    deck.encoder(0)?.ring().set_all(Rgb::RED)?;
    deck.encoder(1)?.ring().set_all(Rgb::BLUE)?;
    hold(&mut deck, Duration::from_secs(2))?;

    checkpoint("encoder rings — orientation: clockwise from TOP, Red Green Blue White");
    let card = [Rgb::RED, Rgb::GREEN, Rgb::BLUE, Rgb::WHITE];
    for index in 0..2u8 {
        deck.encoder(index)?.ring().set_segments(card)?;
    }
    hold(&mut deck, Duration::from_secs(3))?;

    checkpoint("encoder rings — level readout sweeping 0 to full");
    for step in 0..=4 {
        let level = step as f32 / 4.0;
        for index in 0..2u8 {
            deck.encoder(index)?
                .ring()
                .set_level(level, Rgb::GREEN, Rgb::new(20, 0, 0))?;
        }
        hold(&mut deck, Duration::from_millis(500))?;
    }

    if soak {
        checkpoint("keepalive soak: 6 minutes in software mode");
        let start = Instant::now();
        let mut minutes_reported = 0;
        while start.elapsed() < Duration::from_secs(360) {
            for event in deck.poll(Duration::from_secs(5))? {
                println!("    [{}s] {event:?}", start.elapsed().as_secs());
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
    println!("    press keys / encoders, rotate them, touch the screen — events print below;");
    println!("    a pressed key turns white, a turned knob fills its ring");
    let start = Instant::now();
    let mut level = [0.5f32; 2];
    while start.elapsed() < Duration::from_secs(30) {
        for event in deck.poll(Duration::from_secs(1))? {
            println!("    {event:?}");
            match event {
                Event::KeyDown(index) => deck.button(index)?.set_color(Rgb::WHITE)?,
                Event::KeyUp(index) => deck
                    .button(index)?
                    .draw(&key_test_card(index, font.as_ref()))?,
                Event::EncoderRotate(index, delta) => {
                    let slot = &mut level[index as usize % 2];
                    *slot = (*slot + delta as f32 * 0.1).clamp(0.0, 1.0);
                    deck.encoder(index)?
                        .ring()
                        .set_level(*slot, Rgb::GREEN, Rgb::new(20, 0, 0))?;
                }
                Event::EncoderDown(index) => deck.encoder(index)?.ring().set_all(Rgb::WHITE)?,
                Event::EncoderUp(index) => deck.encoder(index)?.ring().set_level(
                    level[index as usize % 2],
                    Rgb::GREEN,
                    Rgb::new(20, 0, 0),
                )?,
                _ => {}
            }
        }
    }

    checkpoint("cleanup: clear panel");
    deck.clear_all()?;

    println!("\nall controls exercised through the framework API — firmware {firmware}.");
    println!("please report this result!");
    Ok(())
}
