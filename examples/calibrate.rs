//! Staged panel calibration for the Galleon 100 SD Stream Deck module.
//!
//! ```sh
//! cargo run --example calibrate                  # walk the stages, save at the end
//! cargo run --example calibrate -- --show        # draw the saved layout and hold
//! cargo run --example calibrate -- --print       # print the saved matrix, no device
//! cargo run --example calibrate -- --json        # print the saved layout as JSON
//! cargo run --example calibrate -- --json-out P  # also write JSON to P on save
//! ```
//!
//! Completing the wizard writes the editable text layout and prints JSON to
//! stdout, so a calibration run can be piped straight into another tool.
//!
//! The module is one physical panel: an info screen on top, the keys below.
//! Nothing in the protocol says where the keys are, and it varies with how
//! the display sits behind the bezel — so it is measured per unit.
//!
//! Each stage asks one question with one visible success condition: **no red
//! showing**. Red is panel the keycap plastic is supposed to cover, so any
//! red you can see is geometry that is still wrong.
//!
//! One binding to remember: the **left knob moves the first value, the right
//! knob moves the second**, and the HUD always names which two those are.

use std::time::{Duration, Instant};

use galdeck::ids::{PANEL_HEIGHT, PANEL_WIDTH, REGION_MCU};
use galdeck::layout::{Band, Layout, Rect, Source};
use galdeck::{Align, Canvas, Event, Font, Galleon, Rgb, TextStyle};

/// Height of the HUD strip. A multiple of 8, like everything drawn. The red ground never encroaches on it: a bound
/// driven under the bezel is invisible, and these numbers are the way back.
const HUD_HEIGHT: u16 = 152;

/// How long the left knob must be held to cycle the step size. Long enough
/// not to fire on a tap that means "previous stage", short enough not to
/// feel stuck.
const HOLD: Duration = Duration::from_millis(600);

/// Minimum spacing between repaints. A stage change repaints the ground and
/// every zone; without a floor here a fast spin queues more uploads than the
/// module wants to take.
const PAINT_INTERVAL: Duration = Duration::from_millis(120);

/// Bleed inside the boundary: the strip the keycap plastic should hide.
const BLEED: Rgb = Rgb::new(200, 0, 0);
/// Panel outside the boundary. A different shade on purpose — one of these
/// reds is removable with the bounds knobs and the other is not, and painting
/// them identically made a two-minute diagnosis take an hour.
const OUTSIDE: Rgb = Rgb::new(70, 0, 30);
const GREEN: Rgb = Rgb::new(0, 230, 90);
/// Rule and zone-border thickness, one whole JPEG block.
const EDGE: u16 = 8;

/// Thickness of the bright line marking a zone's exact outer edge.
///
/// The green border is drawn *inside* the zone, so the green-to-dark
/// boundary sits [`EDGE`] pixels in from the real one. Where zones tile with
/// no bleed their borders touch and no red separates them, leaving nothing
/// to mark the true edge — this line is it.
const EDGE_MARK: i32 = 2;

/// Width of the bound rules: one whole region block. Anything narrower
/// renders about half its height — see [`REGION_MCU`].
const RULE: u16 = REGION_MCU as u16;

/// Tallest region drawn in one go. Nothing taller than this has ever been
/// confirmed to render on hardware — every successful draw this session has
/// been 224 rows or fewer — so tall fills go out as a stack of bands rather
/// than betting on a single large region.
const MAX_BAND: u16 = 224;

/// Round a span down to whole region blocks, for a dimension that must not
/// shear. Positions are unconstrained; only sizes are.
trait McuFloor {
    fn to_mcu_floor(self) -> u16;
}

impl McuFloor for u16 {
    fn to_mcu_floor(self) -> u16 {
        self - self % REGION_MCU as u16
    }
}



#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Top and bottom of the zone area.
    VerticalBounds,
    /// Left and right of the zone area.
    HorizontalBounds,
    /// The upper and lower edge of each row, measured one row at a time.
    /// The 4x3 shape is a property of the hardware, so it is not asked for
    /// — but the physical keys are not evenly spaced, and dividing the
    /// boundary cannot fit them.
    Rows,
    /// The left and right edge of each column, same idea.
    Columns,
    /// How much of each cell the plastic covers.
    Bleed,
    /// Move one zone, for a key that sits slightly off centre.
    NudgeMove,
    /// Resize one zone, for a key whose window is a different size.
    NudgeSize,
    /// The finished result, drawn as a whole.
    Review,
}

impl Stage {
    const ALL: [Stage; 8] = [
        Stage::VerticalBounds,
        Stage::HorizontalBounds,
        Stage::Rows,
        Stage::Columns,
        Stage::Bleed,
        Stage::NudgeMove,
        Stage::NudgeSize,
        Stage::Review,
    ];

    fn title(&self) -> &'static str {
        match self {
            Stage::VerticalBounds => "1/8  VERTICAL BOUNDS",
            Stage::HorizontalBounds => "2/8  HORIZONTAL BOUNDS",
            Stage::Rows => "3/8  ROW EDGES",
            Stage::Columns => "4/8  COLUMN EDGES",
            Stage::Bleed => "5/8  BLEED",
            Stage::NudgeMove => "6/8  MOVE ONE ZONE",
            Stage::NudgeSize => "7/8  RESIZE ONE ZONE",
            Stage::Review => "8/8  REVIEW",
        }
    }

    /// What the two knobs move, in order: left then right.
    fn knobs(&self) -> (&'static str, &'static str) {
        match self {
            Stage::VerticalBounds => ("top", "bottom"),
            Stage::HorizontalBounds => ("left", "right"),
            Stage::Rows => ("row top", "row bottom"),
            Stage::Columns => ("col left", "col right"),
            Stage::Bleed => ("bleed x", "bleed y"),
            Stage::NudgeMove => ("move x", "move y"),
            Stage::NudgeSize => ("width", "height"),
            Stage::Review => ("", ""),
        }
    }

    fn hint(&self) -> &'static str {
        match self {
            Stage::VerticalBounds => "drop the top to the first key row, the bottom to the last",
            Stage::HorizontalBounds => "pull the sides out until they meet the outer keys",
            Stage::Rows => "press a key to pick its row, then set that row's edges",
            Stage::Columns => "press a key to pick its column, then set its edges",
            Stage::Bleed => "0 = flush green; negative exposes red, positive covers it",
            Stage::NudgeMove => "optional: press a key, then centre it in its switch",
            Stage::NudgeSize => "optional: sizes move a whole 16px block at a time",
            Stage::Review => "press the right knob to save",
        }
    }
}

/// Everything the wizard is editing.
struct Session {
    layout: Layout,
    stage: Stage,
    /// Which zone the nudge stage is moving.
    selected: u8,
    step: u16,
    /// Why the last adjustment did not take. A knob that stops moving with
    /// no explanation is indistinguishable from a hardware limit.
    refused: Option<String>,
    /// When the left knob went down, and whether the hold already fired.
    /// A tap moves back a stage; a hold cycles the step size, on any stage.
    left_down: Option<Instant>,
    left_held: bool,
}

impl Session {
    /// Which row the selected key belongs to.
    fn selected_row(&self) -> u8 {
        self.selected / self.layout.columns().max(1)
    }

    /// Which column the selected key belongs to.
    fn selected_column(&self) -> u8 {
        self.selected % self.layout.columns().max(1)
    }

    fn adjust(&mut self, left: i32, right: i32) {
        let grid = self.layout.grid;
        let bounds = grid.bounds;
        let step = self.step as i32;

        let mut next = grid;
        match self.stage {
            Stage::VerticalBounds => {
                // Top and bottom are edges, not an origin and a size, so
                // moving the top must not drag the bottom with it.
                // Never above the HUD: driving zones over it would hide
                // the numbers that are the way back from a bad bound.
                let top = (bounds.y as i32 + left * step)
                    .clamp(HUD_HEIGHT as i32, PANEL_HEIGHT as i32);
                let bottom =
                    (bounds.bottom() as i32 + right * step).clamp(0, PANEL_HEIGHT as i32);
                if bottom > top {
                    next.bounds.y = top as u16;
                    next.bounds.height = (bottom - top) as u16;
                }
            }
            Stage::HorizontalBounds => {
                let leftmost = (bounds.x as i32 + left * step).clamp(0, PANEL_WIDTH as i32);
                let rightmost = (bounds.right() as i32 + right * step).clamp(0, PANEL_WIDTH as i32);
                if rightmost > leftmost {
                    next.bounds.x = leftmost as u16;
                    next.bounds.width = (rightmost - leftmost) as u16;
                }
            }
            Stage::Rows | Stage::Columns => {
                // Each edge moves independently: pulling one edge in must
                // not drag the opposite edge with it.
                let vertical = self.stage == Stage::Rows;
                let (bands, at, limit) = if vertical {
                    (self.layout.row_bands(), self.selected_row(), PANEL_HEIGHT)
                } else {
                    (
                        self.layout.column_bands(),
                        self.selected_column(),
                        PANEL_WIDTH,
                    )
                };
                let Some(band) = bands.get(at as usize) else {
                    return;
                };
                let start = (band.start as i32 + left * step).clamp(0, limit as i32);
                let end = (band.end as i32 + right * step).clamp(0, limit as i32);
                if end <= start {
                    return;
                }
                let band = Band::new(start as u16, end as u16);
                let mut candidate = self.layout.clone();
                let applied = if vertical {
                    candidate.set_row(at, band)
                } else {
                    candidate.set_column(at, band)
                };
                match applied.and_then(|()| candidate.validate()) {
                    Ok(()) => {
                        self.layout = candidate;
                        self.refused = None;
                    }
                    Err(problem) => self.refused = Some(problem.to_string()),
                }
                return;
            }
            Stage::Bleed => {
                let (track_w, track_h) = grid.cell_size();
                // Zero is flush. Negative pulls the zone in and exposes
                // bleed, bounded where the zone would collapse; positive
                // pushes it out over the bleed.
                // Bounded by the track on both sides: pulled in far enough
                // to collapse the zone, or pushed out far enough to swallow
                // the neighbouring key, are both nonsense.
                let limit_x = (track_w as i32 - 8) / 2;
                let limit_y = (track_h as i32 - 8) / 2;
                next.bleed_x = (grid.bleed_x as i32 + left * step).clamp(-limit_x, limit_x) as i16;
                next.bleed_y = (grid.bleed_y as i32 + right * step).clamp(-limit_y, limit_y) as i16;
            }
            Stage::NudgeMove => {
                if let Err(problem) =
                    self.layout
                        .nudge_zone(self.selected, left * step, right * step, 0, 0)
                {
                    self.refused = Some(problem.to_string());
                }
                return;
            }
            Stage::NudgeSize => {
                // A whole region block per detent whatever the step setting:
                // the firmware only renders multiples of 16, so a finer step
                // would move the number without moving the pixels.
                let block = REGION_MCU as i32;
                if let Err(problem) =
                    self.layout
                        .nudge_zone(self.selected, 0, 0, left * block, right * block)
                {
                    self.refused = Some(problem.to_string());
                }
                return;
            }
            Stage::Review => return,
        }

        if next != grid {
            let mut candidate = self.layout.clone();
            candidate.set_grid(next);
            match candidate.validate() {
                Ok(()) => {
                    self.layout = candidate;
                    self.refused = None;
                }
                Err(problem) => self.refused = Some(problem.to_string()),
            }
        }
    }

    /// The two live values the HUD shows for this stage.
    fn values(&self) -> (i64, i64) {
        let grid = self.layout.grid;
        match self.stage {
            Stage::VerticalBounds => (grid.bounds.y as i64, grid.bounds.bottom() as i64),
            Stage::HorizontalBounds => (grid.bounds.x as i64, grid.bounds.right() as i64),
            Stage::Rows => self
                .layout
                .row_bands()
                .get(self.selected_row() as usize)
                .map(|b| (b.start as i64, b.end as i64))
                .unwrap_or((0, 0)),
            Stage::Columns => self
                .layout
                .column_bands()
                .get(self.selected_column() as usize)
                .map(|b| (b.start as i64, b.end as i64))
                .unwrap_or((0, 0)),
            Stage::Bleed => (grid.bleed_x as i64, grid.bleed_y as i64),
            Stage::NudgeMove => self
                .layout
                .zone_at(self.selected)
                .map(|z| (z.bounds.x as i64, z.bounds.y as i64))
                .unwrap_or((0, 0)),
            Stage::NudgeSize => self
                .layout
                .zone_at(self.selected)
                .map(|z| (z.bounds.width as i64, z.bounds.height as i64))
                .unwrap_or((0, 0)),
            Stage::Review => (self.layout.zone_count() as i64, 0),
        }
    }
}

/// The calibration pattern: red ground, green zone edges.
///
/// Red is deliberately the signal rather than green. Hunting for a green
/// edge fails when the edge has slid under the bezel; red showing where it
/// should be hidden is visible from across the room.
/// Flood a rectangle, split into bands no taller than [`MAX_BAND`].
///
/// Solid colours compress to almost nothing, so the extra reports cost
/// little, and it keeps every region within a size the device is known to
/// accept.
fn fill_banded(
    deck: &mut Galleon,
    rect: Rect,
    color: Rgb,
) -> Result<(), Box<dyn std::error::Error>> {
    if rect.is_empty() {
        return Ok(());
    }
    let mut y = rect.y;
    let bottom = rect.bottom() as u16;
    while y < bottom {
        let height = MAX_BAND.min(bottom - y);
        deck.panel().fill_rect(rect.x, y, rect.width, height, color)?;
        y += height;
    }
    Ok(())
}

fn render(
    deck: &mut Galleon,
    session: &Session,
    font: Option<&Font>,
) -> Result<(), Box<dyn std::error::Error>> {
    let layout = &session.layout;
    let bounds = layout.grid.bounds;
    let top = layout.screen.bottom().max(HUD_HEIGHT as u32) as u16;

    if session.stage == Stage::Review {
        // Everything below the info screen goes dark, so the schematic
        // above is the only thing competing for attention.
        fill_banded(
            deck,
            Rect::new(0, top, PANEL_WIDTH, PANEL_HEIGHT - top),
            Rgb::new(6, 6, 8),
        )?;
        return Ok(());
    }

    fill_banded(
        deck,
        Rect::new(0, top, PANEL_WIDTH, PANEL_HEIGHT - top),
        OUTSIDE,
    )?;

    // Clip the boundary to the area below the HUD before rounding — rounding
    // first and clamping afterwards can leave a dimension off-block, which
    // the firmware shears into diagonal streaks.
    let visible_top = bounds.y.max(top);
    let visible_bottom = (bounds.bottom() as u16).max(visible_top);
    let inside = Rect::new(
        bounds.x,
        visible_top,
        bounds.width,
        visible_bottom - visible_top,
    );

    match session.stage {
        // While two edges are being set, draw those two edges and nothing
        // else. Twelve zones would be answering a question that has not
        // been asked yet.
        Stage::VerticalBounds | Stage::HorizontalBounds => {
            if !inside.is_drawable() {
                return Ok(());
            }
            // Floor the interior fill and keep its origin: rounding UP here
            // overflows the panel whenever the far edge is already at it,
            // and the correction moves the near edge — the exact edge being
            // adjusted. That is what made the top jump 439 -> 432 and then
            // sit still from 441 to 447.
            fill_banded(deck, inside.to_mcu(), Rgb::new(8, 8, 10))?;

            // The rules are drawn at their exact coordinates. Both spans are
            // whole blocks by construction (EDGE, and the full width or
            // height), so nothing needs rounding and nothing can shift. The
            // floored interior leaves at most 7px short of the far edge, and
            // the far rule covers precisely that.
            if session.stage == Stage::VerticalBounds {
                let width = inside.width.to_mcu_floor();
                let bottom = (inside.bottom() as u16) - RULE;
                deck.panel()
                    .fill_rect(inside.x, inside.y, width, RULE, GREEN)?;
                deck.panel()
                    .fill_rect(inside.x, bottom, width, RULE, GREEN)?;
            } else {
                // One continuous region per rule, spanning the calibrated
                // upper bound to the lower bound. Drawing these in bands
                // made each band look like it stopped short, because the
                // keycap gaps between rows hide part of a continuous line
                // and the band seams landed in the visible parts.
                let height = inside.height.to_mcu_floor();
                let right = (inside.right() as u16) - RULE;
                deck.panel()
                    .fill_rect(inside.x, inside.y, RULE, height, GREEN)?;
                deck.panel()
                    .fill_rect(right, inside.y, RULE, height, GREEN)?;
            }
            return Ok(());
        }
        _ => {}
    }

    // Rows and columns are set one band at a time, so draw the bands — the
    // same lesson the bounds stages taught: twelve zones while two edges are
    // being set is noise.
    if matches!(session.stage, Stage::Rows | Stage::Columns) {
        fill_banded(deck, inside.to_mcu(), BLEED)?;
        let vertical = session.stage == Stage::Rows;
        let (bands, selected) = if vertical {
            (layout.row_bands(), session.selected_row())
        } else {
            (layout.column_bands(), session.selected_column())
        };

        for (at, band) in bands.iter().enumerate() {
            let cell = if vertical {
                Rect::new(inside.x, band.start, inside.width.to_mcu_floor(), band.len())
            } else {
                Rect::new(band.start, inside.y, band.len(), inside.height.to_mcu_floor())
            };
            if !cell.is_drawable() {
                continue;
            }
            fill_banded(deck, cell.to_mcu(), Rgb::new(8, 8, 10))?;

            // The band under the knobs is bright; the rest are dimmed, so
            // the two edges being set are unambiguous.
            let color = if at as u8 == selected {
                GREEN
            } else {
                Rgb::new(0, 90, 40)
            };
            if vertical {
                deck.panel()
                    .fill_rect(cell.x, band.start, cell.width, RULE, color)?;
                deck.panel()
                    .fill_rect(cell.x, band.end - RULE, cell.width, RULE, color)?;
            } else {
                deck.panel()
                    .fill_rect(band.start, cell.y, RULE, cell.height, color)?;
                deck.panel()
                    .fill_rect(band.end - RULE, cell.y, RULE, cell.height, color)?;
            }

            if let Some(font) = font {
                let mut label =
                    Canvas::filled(RULE as u32 * 4, RULE as u32 * 3, Rgb::new(8, 8, 10));
                label.draw_text(
                    &at.to_string(),
                    (RULE * 2) as i32,
                    (RULE * 2) as i32 + 6,
                    &TextStyle::new(font, 32.0).align(Align::Center).color(color),
                );
                deck.panel()
                    .draw_at(cell.x + RULE, cell.y + RULE, &label)?;
            }
        }
        return Ok(());
    }

    // From the bleed stage on, the zones themselves are the question.
    fill_banded(deck, inside.to_mcu(), BLEED)?;

    for zone in layout.zones() {
        if !zone.bounds.is_drawable() {
            continue;
        }
        // Round the drawn rect UP to whole blocks: truncating strands up to
        // 7px of unpainted panel at each edge, which shows as a sliver of
        // red that no setting can remove.
        let bounds = zone.bounds.to_mcu_covering();
        // The border is always green. Selection is shown by the fill, so
        // colour keeps one meaning throughout: green edge = correct edge,
        // red = bleed that should be hidden.
        let chosen = matches!(session.stage, Stage::NudgeMove | Stage::NudgeSize)
            && zone.index == session.selected;
        let mut canvas = Canvas::filled(
            bounds.width as u32,
            bounds.height as u32,
            if chosen {
                Rgb::new(0, 48, 64)
            } else {
                Rgb::new(8, 8, 10)
            },
        );
        for ring in 0..EDGE as i32 {
            // The outermost pixels are the zone's min/max, so they get a
            // brighter tint: everything from there inward is decoration.
            let color = if ring < EDGE_MARK {
                Rgb::new(190, 255, 210)
            } else {
                GREEN
            };
            canvas.draw_rect(
                ring,
                ring,
                bounds.width as u32 - 2 * ring as u32,
                bounds.height as u32 - 2 * ring as u32,
                color,
            );
        }
        if matches!(session.stage, Stage::NudgeMove | Stage::NudgeSize) {
            if let Some(font) = font {
                canvas.draw_text(
                    &zone.index.to_string(),
                    bounds.width as i32 / 2,
                    bounds.height as i32 / 2 + 14,
                    &TextStyle::new(font, 40.0).align(Align::Center).color(if chosen {
                        Rgb::WHITE
                    } else {
                        Rgb::new(70, 70, 80)
                    }),
                );
            }
        }
        deck.panel().draw_at(bounds.x, bounds.y, &canvas)?;
    }
    Ok(())
}

fn hud(
    deck: &mut Galleon,
    session: &Session,
    font: Option<&Font>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Review shows the schematic instead: the point of the last stage is to
    // see the result whole rather than as two numbers.
    if session.stage == Stage::Review {
        let canvas = session.layout.schematic(
            PANEL_WIDTH as u32,
            session.layout.screen.height as u32,
            font,
        );
        deck.panel().draw_at(0, 0, &canvas)?;
        return Ok(());
    }

    let mut canvas = Canvas::filled(PANEL_WIDTH as u32, HUD_HEIGHT as u32, Rgb::new(10, 10, 16));
    if let Some(font) = font {
        canvas.draw_text(
            session.stage.title(),
            14,
            30,
            &TextStyle::new(font, 24.0).color(Rgb::new(120, 200, 255)),
        );
        if matches!(session.stage, Stage::NudgeMove | Stage::NudgeSize) {
            canvas.draw_text(
                &format!("zone {}", session.selected),
                286,
                30,
                &TextStyle::new(font, 22.0).color(Rgb::new(120, 220, 240)),
            );
        }
        canvas.draw_text(
            &format!("step {}", session.step),
            PANEL_WIDTH as i32 - 14,
            30,
            &TextStyle::new(font, 20.0)
                .align(Align::Right)
                .color(Rgb::new(110, 110, 120)),
        );

        let (left, right) = session.stage.knobs();
        let (left_value, right_value) = session.values();
        canvas.draw_text(
            &format!("L  {left}: {left_value}"),
            14,
            76,
            &TextStyle::new(font, 26.0).color(Rgb::WHITE),
        );
        canvas.draw_text(
            &format!("R  {right}: {right_value}"),
            370,
            76,
            &TextStyle::new(font, 26.0).color(Rgb::WHITE),
        );
        let (message, color) = match &session.refused {
            Some(problem) => (problem.as_str(), Rgb::new(255, 120, 100)),
            None => (session.stage.hint(), Rgb::new(170, 170, 120)),
        };
        canvas.draw_text(
            message,
            14,
            110,
            &TextStyle::new(font, 19.0)
                .max_width(PANEL_WIDTH as u32 - 28)
                .color(color),
        );
        canvas.draw_text(
            "L tap: back    L hold: step    R press: next",
            PANEL_WIDTH as i32 / 2,
            140,
            &TextStyle::new(font, 17.0)
                .align(Align::Center)
                .color(Rgb::new(100, 100, 110)),
        );
    }
    deck.panel().draw_at(0, 0, &canvas)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let path = Layout::default_path();

    // Progress goes to stderr throughout, so --json and the JSON written at
    // the end of a run leave stdout clean enough to pipe.
    let saved = Layout::load(&path);
    let starting = match &saved {
        Ok(layout) => {
            eprintln!("==> loaded {}", path.display());
            layout.clone()
        }
        Err(error) => {
            eprintln!("==> no usable saved layout ({error}); starting from the template");
            eprintln!("    NOTE: template values are arithmetic, not measured");
            Layout::TEMPLATE
        }
    };

    if flag("--print") {
        print!("{}", starting.to_ascii());
        return Ok(());
    }

    // Both of these work with no device attached, so a consumer can read a
    // calibration without touching the hardware.
    if flag("--json") {
        print!("{}", starting.to_json());
        return Ok(());
    }

    let json_out = args
        .iter()
        .position(|a| a == "--json-out")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let api = hidapi::HidApi::new()?;
    // One handle for everything — keepalive, input and drawing. Two open
    // handles to the same hidraw node is the leading suspect for the module
    // dropping off the USB bus.
    let mut deck = Galleon::open(&api)?;
    deck.set_brightness(80)?;
    let font = Font::system();

    let mut session = Session {
        layout: starting,
        stage: Stage::VerticalBounds,
        selected: 0,
        step: 8,
        refused: None,
        left_down: None,
        left_held: false,
    };

    if flag("--show") {
        session.stage = Stage::Review;
        render(&mut deck, &session, font.as_ref())?;
        hud(&mut deck, &session, font.as_ref())?;
        print!("{}", session.layout.to_ascii());
        eprintln!("==> holding — press either knob to end");
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

    eprintln!("==> LEFT knob moves the first value, RIGHT knob the second");
    eprintln!("    LEFT tap: previous stage    LEFT hold: cycle step (1/2/4/8)");
    eprintln!("    RIGHT press: next stage / save");
    eprintln!("    goal: no red showing, only green");

    render(&mut deck, &session, font.as_ref())?;
    hud(&mut deck, &session, font.as_ref())?;
    let mut last_paint = Instant::now();
    let mut dirty = false;

    loop {
        let (mut left, mut right) = (0i32, 0i32);
        for event in deck.poll(Duration::from_millis(100))? {
            match event {
                Event::EncoderRotate(0, delta) => left += delta as i32,
                Event::EncoderRotate(1, delta) => right += delta as i32,
                Event::EncoderDown(0) => {
                    session.left_down = Some(Instant::now());
                    session.left_held = false;
                }
                Event::EncoderUp(0) => {
                    // A hold already did its work; only a tap goes back.
                    if !session.left_held {
                        let at = Stage::ALL.iter().position(|s| *s == session.stage).unwrap();
                        if at > 0 {
                            session.stage = Stage::ALL[at - 1];
                            dirty = true;
                        }
                    }
                    session.left_down = None;
                    session.left_held = false;
                }
                Event::EncoderDown(1) => {
                    let at = Stage::ALL.iter().position(|s| *s == session.stage).unwrap();
                    if session.stage == Stage::Review {
                        session.layout.source = Source::Calibrated;
                        session.layout.save(&path)?;
                        eprintln!("==> saved {}", path.display());
                        eprint!("{}", session.layout.to_ascii());
                        if let Some(out) = &json_out {
                            std::fs::write(out, session.layout.to_json())?;
                            eprintln!("==> wrote {out}");
                        }
                        // JSON on stdout, progress on stderr: the run can be
                        // piped into another tool without stripping chatter.
                        print!("{}", session.layout.to_json());
                        return Ok(());
                    }
                    session.stage = Stage::ALL[at + 1];
                    dirty = true;
                }
                Event::KeyDown(index)
                    if matches!(
                        session.stage,
                        Stage::Rows | Stage::Columns | Stage::NudgeMove | Stage::NudgeSize
                    )
                        && u16::from(index) < session.layout.zone_count() =>
                {
                    session.selected = index;
                    dirty = true;
                }
                _ => {}
            }
        }

        // Fire the step change while the knob is still held rather than on
        // release, so the HUD confirms it before the user lets go.
        if let Some(since) = session.left_down {
            if !session.left_held && since.elapsed() >= HOLD {
                session.step = if session.step >= 8 { 1 } else { session.step * 2 };
                session.left_held = true;
                dirty = true;
            }
        }

        if left != 0 || right != 0 {
            session.adjust(left, right);
            dirty = true;
        }

        if dirty && last_paint.elapsed() >= PAINT_INTERVAL {
            render(&mut deck, &session, font.as_ref())?;
            hud(&mut deck, &session, font.as_ref())?;
            last_paint = Instant::now();
            dirty = false;
        }
    }
}
