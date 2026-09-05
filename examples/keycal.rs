//! Key-geometry calibration for the Galleon 100 SD Stream Deck module.
//!
//! Answers two questions the protocol docs cannot: how large the visible
//! key panel really is, and where the firmware anchors an uploaded image
//! inside it.
//!
//! ```sh
//! cargo run --example keycal              # guided A/B diagnostic
//! cargo run --example keycal -- --size 168   # one card at a chosen size
//! cargo run --example keycal -- --sweep      # turn the LEFT knob to resize
//! ```
//!
//! The cards are drawn on top of a solid red `03 06` key fill, which the
//! firmware paints across the whole panel. Any red left showing is panel
//! the uploaded image did not cover.

use std::time::{Duration, Instant};

use galdeck::ids::KEY_PIXELS;
use galdeck::{Align, Canvas, Event, Font, Galleon, Rgb, TextStyle};

/// Width of one ruler band, in image pixels. Count the bands still visible
/// on an edge to read off how much of the image is being clipped.
const BAND: u32 = 5;
const BANDS: u32 = 4;

const UNCOVERED: Rgb = Rgb::RED;
const BAND_LIGHT: Rgb = Rgb::WHITE;
const BAND_DARK: Rgb = Rgb::new(70, 70, 70);
const TOP_LEFT_MARK: Rgb = Rgb::GREEN;
const BOTTOM_RIGHT_MARK: Rgb = Rgb::new(60, 140, 255);

fn hold(deck: &mut Galleon, duration: Duration) -> Result<(), galdeck::Error> {
    let start = Instant::now();
    while start.elapsed() < duration {
        deck.poll(Duration::from_millis(200))?;
    }
    Ok(())
}

/// A square ruler card: `BANDS` alternating `BAND`-pixel rings, a green
/// square in the image's top-left corner, a blue one in its bottom-right,
/// and the size in the middle.
fn ruler_card(width: u32, height: u32, font: Option<&Font>) -> Canvas {
    let mut canvas = Canvas::filled(width, height, Rgb::BLACK);
    for band in 0..BANDS {
        let inset = band * BAND;
        let color = if band % 2 == 0 { BAND_LIGHT } else { BAND_DARK };
        for ring in 0..BAND {
            let offset = (inset + ring) as i32;
            canvas.draw_rect(
                offset,
                offset,
                width - 2 * (inset + ring),
                height - 2 * (inset + ring),
                color,
            );
        }
    }

    let mark = BANDS * BAND;
    canvas.fill_rect(mark as i32, mark as i32, 14, 14, TOP_LEFT_MARK);
    canvas.fill_rect(
        (width - mark - 14) as i32,
        (height - mark - 14) as i32,
        14,
        14,
        BOTTOM_RIGHT_MARK,
    );

    if let Some(font) = font {
        canvas.draw_text(
            &format!("{width}x{height}"),
            width as i32 / 2,
            height as i32 / 2 + 12,
            &TextStyle::new(font, 36.0).align(Align::Center),
        );
    }
    canvas
}

/// Paint every key solid red, then upload the card at `size` over it.
/// Bypasses `Button::draw` on purpose: that enforces `KEY_PIXELS`, and the
/// whole point here is to try sizes it would reject.
fn show(
    deck: &mut Galleon,
    size: u32,
    font: Option<&Font>,
) -> Result<(), Box<dyn std::error::Error>> {
    let jpeg = ruler_card(size, size, font).to_jpeg(90)?;
    for index in 0..galdeck::Buttons::COUNT {
        deck.button(index)?.set_color(UNCOVERED)?;
    }
    for index in 0..galdeck::Buttons::COUNT {
        deck.button(index)?.set_jpeg(&jpeg)?;
    }
    Ok(())
}

/// Every key at a different size, so one look compares them all. Key `i`
/// gets `start + i * step`; the label on each card is its own size.
///
/// Two readings come out of this at once. If the red margin shrinks as the
/// sizes grow while the ruler bands keep their thickness, the firmware
/// blits 1:1 and the panel is simply bigger than the image. If instead
/// every key shows the same red margin and the bands get *thinner* on the
/// larger cards, the firmware is scaling whatever it is given into a fixed
/// area, and no image size will ever reach the edge.
fn grid(
    deck: &mut Galleon,
    start: u32,
    step: u32,
    font: Option<&Font>,
) -> Result<(), Box<dyn std::error::Error>> {
    for index in 0..galdeck::Buttons::COUNT {
        deck.button(index)?.set_color(UNCOVERED)?;
    }
    for index in 0..galdeck::Buttons::COUNT {
        let size = start + index as u32 * step;
        let jpeg = ruler_card(size, size, font).to_jpeg(90)?;
        deck.button(index)?.set_jpeg(&jpeg)?;
        let (column, row) = (
            index % galdeck::Buttons::COLUMNS,
            index / galdeck::Buttons::COLUMNS,
        );
        println!("    key {index} (col {column}, row {row}) = {size}");
    }
    Ok(())
}

/// Does pre-filling the key hide the uncovered margin?
///
/// The firmware clamps uploaded key images to 160x160 but paints the whole
/// panel for an `03 06` solid fill, and that fill survives underneath the
/// image. So the margin the image cannot reach can be made to match the
/// artwork instead of showing whatever was there before.
///
/// Top two rows (keys 0-5) pre-fill RED, so the margin stands out. Bottom
/// two rows (keys 6-11) pre-fill with the card's own background, so the
/// margin should disappear and the key should read as full-bleed.
fn prefill(
    deck: &mut Galleon,
    size: u32,
    font: Option<&Font>,
) -> Result<(), Box<dyn std::error::Error>> {
    // A plain card: one background, a thick contrasting inner frame, so an
    // uncovered edge is obvious but a matched one is invisible.
    let background = Rgb::new(20, 110, 200);
    let mut canvas = Canvas::filled(size, size, background);
    canvas.fill_rect(30, 30, size - 60, size - 60, Rgb::WHITE);
    if let Some(font) = font {
        canvas.draw_text(
            "FULL",
            size as i32 / 2,
            size as i32 / 2 + 10,
            &TextStyle::new(font, 30.0)
                .align(Align::Center)
                .color(Rgb::BLACK),
        );
    }
    let jpeg = canvas.to_jpeg(90)?;

    for index in 0..galdeck::Buttons::COUNT {
        let under = if index < 6 { UNCOVERED } else { background };
        deck.button(index)?.set_color(under)?;
    }
    for index in 0..galdeck::Buttons::COUNT {
        deck.button(index)?.set_jpeg(&jpeg)?;
    }
    println!("    keys 0-5  (top two rows):    red underneath — margin visible");
    println!("    keys 6-11 (bottom two rows): background underneath — margin hidden");
    Ok(())
}

/// Three sizes side by side, one per column, repeated down all four rows.
///
/// Keeps every upload inside the range the firmware is known to survive —
/// oversized cards have been observed to knock the module off the USB bus
/// and take the whole keyboard down with it.
fn columns(
    deck: &mut Galleon,
    sizes: [u32; 3],
    font: Option<&Font>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cards: Vec<Vec<u8>> = sizes
        .iter()
        .map(|size| ruler_card(*size, *size, font).to_jpeg(90))
        .collect::<Result<_, _>>()?;
    for index in 0..galdeck::Buttons::COUNT {
        let column = (index % galdeck::Buttons::COLUMNS) as usize;
        deck.button(index)?.set_jpeg(&cards[column])?;
    }
    println!(
        "    column 0 = {}, column 1 = {}, column 2 = {}",
        sizes[0], sizes[1], sizes[2]
    );
    Ok(())
}

/// Rectangular cards, one shape per column. The key cells of the 720x1280
/// panel are 240x224, so the panel a key image lands on need not be square
/// even though every implementation so far has assumed it is.
fn rect_columns(
    deck: &mut Galleon,
    shapes: [(u32, u32); 3],
    font: Option<&Font>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cards: Vec<Vec<u8>> = shapes
        .iter()
        .map(|(w, h)| ruler_card(*w, *h, font).to_jpeg(90))
        .collect::<Result<_, _>>()?;
    for index in 0..galdeck::Buttons::COUNT {
        let column = (index % galdeck::Buttons::COLUMNS) as usize;
        deck.button(index)?.set_jpeg(&cards[column])?;
    }
    for (column, (w, h)) in shapes.iter().enumerate() {
        println!("    column {column} = {w}x{h}");
    }
    Ok(())
}

/// Live size tuning on the knobs.
///
/// Rows 1 and 3 (keys 3-5 and 9-11) follow the encoders; rows 0 and 2 hold
/// a fixed 160x160 reference so there is always a known-good card in view
/// to compare against. The left knob moves width, the right moves height,
/// 8 pixels per detent — the firmware only renders key images whose
/// dimensions are a multiple of 8, so 8 is the smallest step that is not
/// simply broken.
fn tune(deck: &mut Galleon, font: Option<&Font>) -> Result<(), Box<dyn std::error::Error>> {
    /// Clamped well below the sizes that were seen to knock the module off
    /// the USB bus.
    const MIN: u32 = 96;
    const MAX: u32 = 256;
    const REFERENCE: u32 = 160;

    let tuned_rows = [1u8, 3];
    let (mut width, mut height) = (240u32, 224u32);

    let reference = ruler_card(REFERENCE, REFERENCE, font).to_jpeg(90)?;
    for index in 0..galdeck::Buttons::COUNT {
        if !tuned_rows.contains(&(index / galdeck::Buttons::COLUMNS)) {
            deck.button(index)?.set_jpeg(&reference)?;
        }
    }

    let draw = |deck: &mut Galleon, w: u32, h: u32| -> Result<(), Box<dyn std::error::Error>> {
        let jpeg = ruler_card(w, h, font).to_jpeg(90)?;
        for index in 0..galdeck::Buttons::COUNT {
            if tuned_rows.contains(&(index / galdeck::Buttons::COLUMNS)) {
                deck.button(index)?.set_jpeg(&jpeg)?;
            }
        }
        println!("    {w}x{h}");
        Ok(())
    };
    draw(deck, width, height)?;

    loop {
        // Coalesce a whole poll batch into one redraw: a fast spin would
        // otherwise queue up more uploads than the module wants to take.
        let (mut dw, mut dh) = (0i32, 0i32);
        for event in deck.poll(Duration::from_millis(100))? {
            match event {
                Event::EncoderRotate(0, delta) => dw += delta as i32,
                Event::EncoderRotate(1, delta) => dh += delta as i32,
                Event::KeyDown(0) => {
                    println!("==> stopped at {width}x{height}");
                    return Ok(());
                }
                _ => {}
            }
        }
        if dw != 0 || dh != 0 {
            width = (width as i32 + dw * 8).clamp(MIN as i32, MAX as i32) as u32;
            height = (height as i32 + dh * 8).clamp(MIN as i32, MAX as i32) as u32;
            draw(deck, width, height)?;
        }
    }
}

/// Region reports built without galdeck's bounds check.
///
/// `Lcd::draw_jpeg_at` refuses anything outside 720x384 because that is
/// the segment the protocol notes describe. The check is ours, not the
/// hardware's — this probe exists to find out whether the firmware
/// actually accepts more.
fn region_reports_unchecked(x: u16, y: u16, w: u16, h: u16, jpeg: &[u8]) -> Vec<Vec<u8>> {
    const HEADER: usize = 16;
    const CHUNK: usize = 1024 - HEADER;
    jpeg.chunks(CHUNK)
        .enumerate()
        .map(|(part, chunk)| {
            let mut report = vec![0u8; 1024];
            report[0] = 0x02;
            report[1] = 0x0c;
            report[2..4].copy_from_slice(&x.to_le_bytes());
            report[4..6].copy_from_slice(&y.to_le_bytes());
            report[6..8].copy_from_slice(&w.to_le_bytes());
            report[8..10].copy_from_slice(&h.to_le_bytes());
            report[10] = ((part + 1) * CHUNK >= jpeg.len()) as u8;
            report[11..13].copy_from_slice(&(part as u16).to_le_bytes());
            report[13..15].copy_from_slice(&(chunk.len() as u16).to_le_bytes());
            report[HEADER..HEADER + chunk.len()].copy_from_slice(chunk);
            report
        })
        .collect()
}

/// Is the key area part of the same panel as the info screen?
///
/// Paints a region through the `02 0c` path at coordinates below the
/// documented 720x384 segment. If the keys are the bottom 720x896 of the
/// one 720x1280 display, this is the command that can reach the pixels the
/// key-image path clips off.
fn panel_probe(
    api: &hidapi::HidApi,
    deck: &mut Galleon,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    font: Option<&Font>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = Galleon::list(api)
        .into_iter()
        .next()
        .ok_or("no galleon module found")?;
    let raw = api.open_path(&std::ffi::CString::new(path)?)?;

    let mut canvas = Canvas::filled(w as u32, h as u32, Rgb::new(255, 0, 200));
    // A grid so any structure in what lights up is readable.
    for step in (0..w as i32).step_by(40) {
        canvas.draw_vline(step, 0, h as u32, Rgb::BLACK);
    }
    for step in (0..h as i32).step_by(40) {
        canvas.draw_hline(0, step, w as u32, Rgb::BLACK);
    }
    if let Some(font) = font {
        canvas.draw_text(
            &format!("{w}x{h} @ {x},{y}"),
            w as i32 / 2,
            h as i32 / 2,
            &TextStyle::new(font, 40.0)
                .align(Align::Center)
                .color(Rgb::WHITE),
        );
    }

    let jpeg = canvas.to_jpeg(85)?;
    let reports = region_reports_unchecked(x, y, w, h, &jpeg);
    println!(
        "    {w}x{h} at ({x},{y}) — {} bytes in {} reports",
        jpeg.len(),
        reports.len()
    );
    for report in &reports {
        deck.poll(Duration::from_millis(0))?;
        raw.write(report)?;
    }
    Ok(())
}

/// Paint all 12 presumed key cells through the region path.
///
/// If the keys are the bottom 720x896 of the panel, cell (col,row) sits at
/// `(x0 + col * w, y0 + row * h)`. Each cell gets its own hue, a white
/// border hard against its edges, and its index — so a cell that is offset
/// or the wrong size shows up as a border that does not line up with the
/// physical key.
fn cell_map(
    api: &hidapi::HidApi,
    deck: &mut Galleon,
    x0: u16,
    y0: u16,
    w: u16,
    h: u16,
    font: Option<&Font>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = Galleon::list(api)
        .into_iter()
        .next()
        .ok_or("no galleon module found")?;
    let raw = api.open_path(&std::ffi::CString::new(path)?)?;

    for index in 0..galdeck::Buttons::COUNT {
        let (column, row) = (
            index % galdeck::Buttons::COLUMNS,
            index / galdeck::Buttons::COLUMNS,
        );
        let mut canvas = Canvas::filled(
            w as u32,
            h as u32,
            Rgb::from_hsv(index as f32 / galdeck::Buttons::COUNT as f32, 0.85, 0.8),
        );
        // Border hard against the cell edge: if any of it is missing, the
        // cell is bigger than the key; if it is inset, it is smaller.
        for ring in 0..4 {
            canvas.draw_rect(
                ring,
                ring,
                w as u32 - 2 * ring as u32,
                h as u32 - 2 * ring as u32,
                Rgb::WHITE,
            );
        }
        if let Some(font) = font {
            canvas.draw_text(
                &index.to_string(),
                w as i32 / 2,
                h as i32 / 2 + 16,
                &TextStyle::new(font, 48.0)
                    .align(Align::Center)
                    .color(Rgb::BLACK),
            );
        }
        let jpeg = canvas.to_jpeg(85)?;
        let x = x0 + column as u16 * w;
        let y = y0 + row as u16 * h;
        for report in region_reports_unchecked(x, y, w, h, &jpeg) {
            deck.poll(Duration::from_millis(0))?;
            raw.write(&report)?;
        }
        println!("    key {index} -> {w}x{h} at ({x},{y})");
    }
    Ok(())
}

/// How far down the panel does the visible window actually go?
///
/// Paints a numbered ruler through the region path across a span of rows.
/// Every 8 pixels gets a tick, every 32 gets its absolute y printed on both
/// sides. The largest number still readable on the glass is the bottom of
/// the visible window - everything past it is panel hidden behind the case.
fn ruler(
    api: &hidapi::HidApi,
    deck: &mut Galleon,
    y0: u16,
    y1: u16,
    font: Option<&Font>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = Galleon::list(api)
        .into_iter()
        .next()
        .ok_or("no galleon module found")?;
    let raw = api.open_path(&std::ffi::CString::new(path)?)?;

    let (w, h) = (720u16, y1 - y0);
    let mut canvas = Canvas::filled(w as u32, h as u32, Rgb::BLACK);
    for absolute in (y0..y1).filter(|y| y % 8 == 0) {
        let local = (absolute - y0) as i32;
        let major = absolute % 32 == 0;
        canvas.draw_hline(
            0,
            local,
            if major { w as u32 } else { 40 },
            if major {
                Rgb::WHITE
            } else {
                Rgb::new(90, 90, 90)
            },
        );
        if major {
            if let Some(font) = font {
                let style = TextStyle::new(font, 22.0).color(Rgb::new(255, 220, 0));
                canvas.draw_text(&absolute.to_string(), 48, local + 20, &style);
                canvas.draw_text(&absolute.to_string(), w as i32 - 100, local + 20, &style);
            }
        }
    }

    let jpeg = canvas.to_jpeg(90)?;
    let reports = region_reports_unchecked(0, y0, w, h, &jpeg);
    println!(
        "    ruler y={y0}..{y1} — {} bytes in {} reports",
        jpeg.len(),
        reports.len()
    );
    for report in &reports {
        deck.poll(Duration::from_millis(0))?;
        raw.write(report)?;
    }
    Ok(())
}

/// Fine ruler across the seam between the info screen and the key row.
///
/// Labels every 8 pixels, staggered horizontally so consecutive labels do
/// not collide, with the band behind each one alternating shade. Read off
/// the last number visible on the info screen and the first number visible
/// on the keys: the gap between them is the strip hidden by the case.
fn fine_ruler(
    api: &hidapi::HidApi,
    deck: &mut Galleon,
    y0: u16,
    y1: u16,
    font: Option<&Font>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = Galleon::list(api)
        .into_iter()
        .next()
        .ok_or("no galleon module found")?;
    let raw = api.open_path(&std::ffi::CString::new(path)?)?;

    const STEP: u16 = 8;
    /// Enough columns that a label never sits directly under its neighbour.
    const COLUMNS: u16 = 8;

    let (w, h) = (720u16, y1 - y0);
    let mut canvas = Canvas::filled(w as u32, h as u32, Rgb::BLACK);
    for (band, absolute) in (y0..y1).step_by(STEP as usize).enumerate() {
        let local = (absolute - y0) as i32;
        let shade = if band % 2 == 0 {
            Rgb::new(24, 24, 32)
        } else {
            Rgb::new(48, 48, 64)
        };
        canvas.fill_rect(0, local, w as u32, STEP as u32, shade);
        canvas.draw_hline(0, local, w as u32, Rgb::new(120, 120, 140));
        if let Some(font) = font {
            // Multiples of 32 in yellow so the coarse ruler's landmarks
            // stay recognisable between the two probes.
            let color = if absolute % 32 == 0 {
                Rgb::new(255, 220, 0)
            } else {
                Rgb::WHITE
            };
            canvas.draw_text(
                &absolute.to_string(),
                20 + (band as u16 % COLUMNS) as i32 * 86,
                local + 18,
                &TextStyle::new(font, 20.0).color(color),
            );
        }
    }

    let jpeg = canvas.to_jpeg(92)?;
    let reports = region_reports_unchecked(0, y0, w, h, &jpeg);
    println!(
        "    fine ruler y={y0}..{y1}, label every {STEP}px — {} reports",
        reports.len()
    );
    for report in &reports {
        deck.poll(Duration::from_millis(0))?;
        raw.write(report)?;
    }
    Ok(())
}

/// Interactive hunt: the left encoder resizes the card, any key press ends.
fn sweep(
    deck: &mut Galleon,
    mut size: u32,
    font: Option<&Font>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("    LEFT knob resizes the card, press any key to stop.");
    println!("    Grow it until the red border disappears — that size is the panel size.");
    show(deck, size, font)?;
    println!("    size = {size}");
    loop {
        for event in deck.poll(Duration::from_millis(100))? {
            match event {
                Event::EncoderRotate(_, delta) => {
                    let next = (size as i32 + delta as i32).clamp(120, 260) as u32;
                    if next != size {
                        size = next;
                        show(deck, size, font)?;
                        println!("    size = {size}");
                    }
                }
                Event::KeyDown(_) | Event::EncoderDown(_) => {
                    println!("==> stopped at size = {size}");
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse::<u32>().ok())
    };

    let api = hidapi::HidApi::new()?;
    let mut deck = Galleon::open(&api)?;
    println!("==> firmware {}", deck.firmware_version()?);
    deck.set_brightness(70)?;

    let font = Font::system();
    if font.is_none() {
        println!("    (no system font — cards will be drawn without the size label)");
    }

    if flag("--fine") {
        let bound = |i: usize, fallback: u16| {
            args.iter()
                .position(|a| a == "--fine")
                .and_then(|p| args.get(p + 1))
                .and_then(|v| v.split(',').nth(i))
                .and_then(|v| v.parse().ok())
                .unwrap_or(fallback)
        };
        let (y0, y1) = (bound(0, 360), bound(1, 440));
        println!("==> fine ruler across the screen/key seam");
        fine_ruler(&api, &mut deck, y0, y1, font.as_ref())?;
        println!("==> holding — press either KNOB to end");
        loop {
            if deck
                .poll(Duration::from_millis(200))?
                .iter()
                .any(|e| matches!(e, Event::EncoderDown(_)))
            {
                return Ok(());
            }
        }
    }

    if flag("--ruler") {
        let bound = |i: usize, fallback: u16| {
            args.iter()
                .position(|a| a == "--ruler")
                .and_then(|p| args.get(p + 1))
                .and_then(|v| v.split(',').nth(i))
                .and_then(|v| v.parse().ok())
                .unwrap_or(fallback)
        };
        let (y0, y1) = (bound(0, 256), bound(1, 448));
        println!("==> visible-window ruler");
        ruler(&api, &mut deck, y0, y1, font.as_ref())?;
        println!("==> holding — press either KNOB to end");
        loop {
            if deck
                .poll(Duration::from_millis(200))?
                .iter()
                .any(|e| matches!(e, Event::EncoderDown(_)))
            {
                return Ok(());
            }
        }
    }

    if flag("--cells") {
        let geom = |i: usize, fallback: u16| {
            args.iter()
                .position(|a| a == "--cells")
                .and_then(|p| args.get(p + 1))
                .and_then(|v| v.split(',').nth(i))
                .and_then(|v| v.parse().ok())
                .unwrap_or(fallback)
        };
        let (x0, y0, w, h) = (geom(0, 0), geom(1, 384), geom(2, 240), geom(3, 224));
        println!("==> key cell map: origin ({x0},{y0}), cells {w}x{h}");
        cell_map(&api, &mut deck, x0, y0, w, h, font.as_ref())?;
        println!("==> holding — press either KNOB to end");
        loop {
            if deck
                .poll(Duration::from_millis(200))?
                .iter()
                .any(|e| matches!(e, Event::EncoderDown(_)))
            {
                return Ok(());
            }
        }
    }

    if flag("--panel") {
        let geom = |i: usize, fallback: u16| {
            args.iter()
                .position(|a| a == "--panel")
                .and_then(|p| args.get(p + 1))
                .and_then(|v| v.split(',').nth(i))
                .and_then(|v| v.parse().ok())
                .unwrap_or(fallback)
        };
        let (x, y, w, h) = (geom(0, 0), geom(1, 384), geom(2, 720), geom(3, 224));
        println!("==> region probe past the documented 720x384 bound");
        panel_probe(&api, &mut deck, x, y, w, h, font.as_ref())?;
        println!("==> holding — press either KNOB to end");
        loop {
            if deck
                .poll(Duration::from_millis(200))?
                .iter()
                .any(|e| matches!(e, Event::EncoderDown(_)))
            {
                return Ok(());
            }
        }
    }

    if flag("--tune") {
        println!("==> live tuning: LEFT knob = width, RIGHT knob = height, 8px per detent");
        println!("    rows 1 and 3 follow the knobs; rows 0 and 2 stay at 160x160");
        println!("    press the TOP-LEFT key to stop");
        return tune(&mut deck, font.as_ref());
    }

    if flag("--rects") {
        let shape = |i: usize, fallback: (u32, u32)| {
            args.iter()
                .position(|a| a == "--rects")
                .and_then(|p| args.get(p + 1))
                .and_then(|v| v.split(',').nth(i))
                .and_then(|v| v.split_once('x'))
                .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                .unwrap_or(fallback)
        };
        let shapes = [
            shape(0, (224, 224)),
            shape(1, (240, 224)),
            shape(2, (240, 240)),
        ];
        println!("==> rectangular comparison by column");
        rect_columns(&mut deck, shapes, font.as_ref())?;
        println!("==> holding — press either KNOB to end");
        loop {
            if deck
                .poll(Duration::from_millis(200))?
                .iter()
                .any(|e| matches!(e, Event::EncoderDown(_)))
            {
                return Ok(());
            }
        }
    }

    if flag("--columns") {
        let parse = |i: usize, fallback: u32| {
            args.iter()
                .position(|a| a == "--columns")
                .and_then(|p| args.get(p + 1))
                .and_then(|v| v.split(',').nth(i))
                .and_then(|v| v.parse().ok())
                .unwrap_or(fallback)
        };
        let sizes = [parse(0, 160), parse(1, 168), parse(2, 176)];
        println!("==> size comparison by column");
        columns(&mut deck, sizes, font.as_ref())?;
        println!("==> holding — press either KNOB to end");
        loop {
            if deck
                .poll(Duration::from_millis(200))?
                .iter()
                .any(|e| matches!(e, Event::EncoderDown(_)))
            {
                return Ok(());
            }
        }
    }

    if flag("--prefill") {
        println!("==> pre-fill workaround check");
        let size = value("--prefill").unwrap_or(160);
        println!("    card size {size}x{size}");
        prefill(&mut deck, size, font.as_ref())?;
        println!("==> holding — press either KNOB to end");
        loop {
            if deck
                .poll(Duration::from_millis(200))?
                .iter()
                .any(|e| matches!(e, Event::EncoderDown(_)))
            {
                return Ok(());
            }
        }
    }

    if flag("--grid") {
        let start = value("--grid").unwrap_or(160);
        let step = value("--step").unwrap_or(2);
        println!("==> size grid over a red fill: key 0 = {start}, +{step} per key");
        println!("    Look for the first key whose red border is gone.");
        grid(&mut deck, start, step, font.as_ref())?;
        println!("==> holding — press either KNOB to end");
        loop {
            if deck
                .poll(Duration::from_millis(200))?
                .iter()
                .any(|e| matches!(e, Event::EncoderDown(_)))
            {
                return Ok(());
            }
        }
    }

    if flag("--sweep") {
        return sweep(
            &mut deck,
            value("--sweep").unwrap_or(KEY_PIXELS),
            font.as_ref(),
        );
    }

    if let Some(size) = value("--size") {
        show(&mut deck, size, font.as_ref())?;
        println!("==> {size}x{size} card held for 30s");
        hold(&mut deck, Duration::from_secs(30))?;
        return Ok(());
    }

    println!("==> step 1: solid red key fill (feature 03 06), 10s");
    println!("    Does red reach every edge of the visible key? If it does NOT,");
    println!("    the firmware itself under-paints and no image size will fix it.");
    for index in 0..galdeck::Buttons::COUNT {
        deck.button(index)?.set_color(UNCOVERED)?;
    }
    hold(&mut deck, Duration::from_secs(10))?;

    println!("==> step 2: {KEY_PIXELS}x{KEY_PIXELS} ruler card over the red fill, 30s");
    println!("    Red edges = panel the image missed. Bands are {BAND}px each.");
    println!("    Green square marks the image's TOP-LEFT, blue its BOTTOM-RIGHT:");
    println!("    if green is not in the key's top-left, the image is being flipped.");
    show(&mut deck, KEY_PIXELS, font.as_ref())?;
    hold(&mut deck, Duration::from_secs(30))?;

    println!("==> step 3: rerun with --sweep to find the size that covers the panel");
    Ok(())
}
