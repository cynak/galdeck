//! Calibrated panel geometry: where the zones actually are on this unit.
//!
//! The module has one physical display with the info screen on top and the
//! keys below. Nothing in the protocol says where the key area starts or how
//! big a key is — the `02 07` key-image path carries no geometry at all, and
//! the `02 0c` region path takes whatever rectangle it is given. The numbers
//! therefore come from measuring a physical keyboard, and they can differ
//! between units depending on how the panel sits behind the bezel.
//!
//! A [`Layout`] is that measurement: an outer [`Grid`] boundary divided into
//! rows and columns, with a *bleed* inset marking the strip around each zone
//! that the keycap plastic covers. Zones derive from those few numbers, and
//! any individual zone can be overridden when one key sits slightly off.

use crate::canvas::Canvas;
use crate::ids::{PANEL_HEIGHT, PANEL_WIDTH, REGION_MCU};
use crate::Rgb;

/// Revision of the on-disk format, so a future reader can refuse a file it
/// does not understand instead of misreading it.
/// Version 2 added the signed `bleed`; version 3 added measured per-row
/// bands. A version 2 file still loads — its rows are derived from the
/// boundary, which is exactly what it meant — but version 1 is refused,
/// because `bleed` there was an unsigned *inset* and reading it under the
/// current rule would silently move every zone.
pub const LAYOUT_FORMAT_VERSION: u32 = 3;

/// Oldest format this build can read.
pub const LAYOUT_FORMAT_MINIMUM: u32 = 2;

/// A rectangle in absolute panel pixels, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// One past the last pixel column. Widened, so a rect near `u16::MAX`
    /// reports honestly instead of wrapping.
    pub const fn right(&self) -> u32 {
        self.x as u32 + self.width as u32
    }

    /// One past the last pixel row.
    pub const fn bottom(&self) -> u32 {
        self.y as u32 + self.height as u32
    }

    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn contains(&self, x: u16, y: u16) -> bool {
        (x as u32) >= self.x as u32
            && (x as u32) < self.right()
            && (y as u32) >= self.y as u32
            && (y as u32) < self.bottom()
    }

    pub fn overlaps(&self, other: &Rect) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && (self.x as u32) < other.right()
            && (other.x as u32) < self.right()
            && (self.y as u32) < other.bottom()
            && (other.y as u32) < self.bottom()
    }

    /// Shrink by `dx` on each side and `dy` on top and bottom. Returns an
    /// empty rect rather than underflowing when the inset eats the whole
    /// rectangle.
    pub fn inset(&self, dx: u16, dy: u16) -> Rect {
        if self.width <= dx.saturating_mul(2) || self.height <= dy.saturating_mul(2) {
            return Rect::new(self.x, self.y, 0, 0);
        }
        Rect::new(
            self.x + dx,
            self.y + dy,
            self.width - dx * 2,
            self.height - dy * 2,
        )
    }

    /// Grow by `dx` on each side and `dy` on top and bottom. Negative
    /// values shrink. The result is kept on the panel and never inverts.
    pub fn grow(&self, dx: i32, dy: i32) -> Rect {
        // Each edge is clamped where it lands. Clamping the size and then
        // sliding the rect back on-panel would silently turn a grow into a
        // move, which is wrong for a zone pinned against the panel edge.
        let left = (self.x as i32 - dx).clamp(0, PANEL_WIDTH as i32);
        let top = (self.y as i32 - dy).clamp(0, PANEL_HEIGHT as i32);
        let right = (self.right() as i32 + dx).clamp(left, PANEL_WIDTH as i32);
        let bottom = (self.bottom() as i32 + dy).clamp(top, PANEL_HEIGHT as i32);
        Rect::new(
            left as u16,
            top as u16,
            (right - left) as u16,
            (bottom - top) as u16,
        )
    }

    /// Can this rect go on the wire as-is: still non-empty once snapped to
    /// whole region blocks, and wholly inside the panel?
    pub fn is_drawable(&self) -> bool {
        !self.is_empty()
            && self.right() <= PANEL_WIDTH as u32
            && self.bottom() <= PANEL_HEIGHT as u32
    }

    /// Slide this rect back onto the panel, keeping its size. Size is
    /// clamped first, so a rect larger than the panel shrinks rather than
    /// being shoved off the far edge.
    pub fn clamped_to_panel(&self) -> Rect {
        let width = self.width.min(PANEL_WIDTH);
        let height = self.height.min(PANEL_HEIGHT);
        Rect::new(
            self.x.min(PANEL_WIDTH - width),
            self.y.min(PANEL_HEIGHT - height),
            width,
            height,
        )
    }

    /// The largest rect at or inside this one whose width and height are
    /// whole region blocks.
    ///
    /// Truncating like this leaves up to 7px of the intended area unpainted
    /// on each axis, which shows as a sliver of bleed that no setting can
    /// remove — prefer [`Rect::to_mcu_covering`] for anything being drawn.
    pub const fn to_mcu(&self) -> Rect {
        let mcu = REGION_MCU as u16;
        Rect::new(
            self.x,
            self.y,
            self.width - self.width % mcu,
            self.height - self.height % mcu,
        )
    }

    /// The smallest rect covering this one whose width and height are whole
    /// JPEG blocks.
    ///
    /// Rounding *up* is what keeps a zone fully painted: the firmware only
    /// accepts multiples of 8, and truncating downward strands a sliver of
    /// panel at the far edge. When growing would leave the panel the origin
    /// shifts back instead, so a zone flush against the bottom edge keeps
    /// its bottom and eats a few pixels into its neighbour — which the
    /// keycap plastic covers anyway.
    pub fn to_mcu_covering(&self) -> Rect {
        if self.is_empty() {
            return *self;
        }
        let (x, width) = fit_mcu(self.x as u32, self.width as u32, PANEL_WIDTH as u32);
        let (y, height) = fit_mcu(self.y as u32, self.height as u32, PANEL_HEIGHT as u32);
        Rect::new(x as u16, y as u16, width as u16, height as u16)
    }
}

/// Fit one axis to whole region blocks without moving an edge the caller chose.
///
/// Growing to the next block is preferred, since truncating strands unpainted
/// panel. When growing would run past `limit` there are two cases, and telling
/// them apart is what stops a horizontal adjustment from dragging the opposite
/// edge around:
///
/// * the span already ends exactly at `limit` — it is flush against the panel
///   edge, so the origin shifts back and that edge is preserved;
/// * otherwise the caller placed this edge deliberately, so the span shrinks
///   to the block below and the origin stays put.
fn fit_mcu(origin: u32, size: u32, limit: u32) -> (u32, u32) {
    let mcu = REGION_MCU;
    let grown = size.div_ceil(mcu) * mcu;
    if origin + grown <= limit {
        return (origin, grown);
    }
    if origin + size == limit {
        let grown = grown.min(limit / mcu * mcu);
        return (limit - grown, grown);
    }
    (origin, size / mcu * mcu)
}

/// Round an offset to the nearest whole JPEG block, never past `span`.
fn snap_mcu(offset: u32, span: u32) -> u32 {
    let mcu = REGION_MCU;
    ((offset + mcu / 2) / mcu * mcu).min(span / mcu * mcu)
}

/// The measured extent of one row or column, along its own axis.
///
/// For a row that is top and bottom; for a column, left and right. The
/// physical keys are not evenly spaced, so dividing a single boundary by
/// the row or column count cannot fit them — a band records where one
/// actually starts and ends on the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    pub start: u16,
    pub end: u16,
}

impl Band {
    pub const fn new(start: u16, end: u16) -> Band {
        Band { start, end }
    }

    pub const fn len(&self) -> u16 {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// One calibrated zone: a key cell in the matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone {
    /// Row in the matrix, from the top.
    pub row: u8,
    /// Column in the matrix, from the left.
    pub column: u8,
    /// Row-major index, matching the key index the device reports.
    pub index: u8,
    /// Where the zone sits on the panel, bleed already removed.
    pub bounds: Rect,
    /// Whether this zone's bounds came from an override rather than from
    /// the grid.
    pub overridden: bool,
}

/// The zone grid as a template: an outer boundary divided into rows and
/// columns, inset by the bleed.
///
/// Use [`Grid::aligned`] to build one whose boundary sits on whole JPEG
/// blocks — that keeps every derived track block-aligned, which is what
/// stops one edge from dragging the opposite edge around when the firmware's
/// multiple-of-8 rule is applied at draw time.
///
/// The bleed is the strip the keycap plastic covers — panel that is lit but
/// not visible. Calibration drives it until none of it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    /// Outer boundary of the whole zone area.
    pub bounds: Rect,
    pub rows: u8,
    pub columns: u8,
    /// Horizontal edge offset applied to every cell. **Zero means the zone
    /// exactly fills its track** — all green, no bleed showing. Negative
    /// pulls the zone in and exposes bleed; positive pushes it out over the
    /// bleed, overlapping the neighbouring track under the keycap plastic.
    pub bleed_x: i16,
    /// Vertical edge offset. Same convention as [`Grid::bleed_x`].
    pub bleed_y: i16,
}

impl Grid {
    /// This grid with its outer boundary snapped to whole region blocks.
    ///
    /// Rarely needed: [`Grid::track`] already snaps its interior divisions
    /// so every track is a whole number of region blocks wherever the boundary
    /// starts. Reach for this only when the *boundary itself* must be
    /// block-aligned — and note that it quantises the origin, so a caller
    /// adjusting the boundary a pixel at a time will find its steps
    /// absorbed.
    pub fn aligned(&self) -> Grid {
        let mcu = REGION_MCU as u16;
        let left = self.bounds.x / mcu * mcu;
        let top = self.bounds.y / mcu * mcu;
        let right = ((self.bounds.right() as u16).div_ceil(mcu) * mcu).min(PANEL_WIDTH);
        let bottom = ((self.bounds.bottom() as u16).div_ceil(mcu) * mcu).min(PANEL_HEIGHT);
        Grid {
            bounds: Rect::new(left, top, right - left, bottom - top),
            ..*self
        }
    }

    /// The `i`th vertical division line, absolute. `i` runs `0..=columns`,
    /// so `column_edge(0)` is the left boundary and `column_edge(columns)`
    /// the right.
    ///
    /// Multiplying before dividing spreads the remainder across the columns
    /// rather than stranding it at the far edge — a leftover strip there
    /// would show as red that no bleed setting could ever hide.
    pub fn column_edge(&self, i: u8) -> u16 {
        let columns = self.columns.max(1) as u32;
        let i = i.min(self.columns) as u32;
        let offset = self.bounds.width as u32 * i / columns;
        // Interior lines snap so each track's WIDTH is a whole number of
        // blocks. Snapping the offset rather than the absolute coordinate
        // is what leaves the boundary itself free to move one pixel at a
        // time — the firmware constrains sizes, not positions.
        if i == 0 || i == columns {
            self.bounds.x + offset as u16
        } else {
            self.bounds.x + snap_mcu(offset, self.bounds.width as u32) as u16
        }
    }

    /// The `i`th horizontal division line, absolute.
    pub fn row_edge(&self, i: u8) -> u16 {
        let rows = self.rows.max(1) as u32;
        let i = i.min(self.rows) as u32;
        let offset = self.bounds.height as u32 * i / rows;
        if i == 0 || i == rows {
            self.bounds.y + offset as u16
        } else {
            self.bounds.y + snap_mcu(offset, self.bounds.height as u32) as u16
        }
    }

    /// The undivided track at a position, bleed included: the panel strip
    /// the keycap window sits inside.
    pub fn track(&self, row: u8, column: u8) -> Rect {
        let (x0, x1) = (self.column_edge(column), self.column_edge(column + 1));
        let (y0, y1) = (self.row_edge(row), self.row_edge(row + 1));
        Rect::new(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
    }

    /// Nominal track size, for a caller sizing a control.
    pub fn cell_size(&self) -> (u16, u16) {
        if self.rows == 0 || self.columns == 0 {
            return (0, 0);
        }
        let track = self.track(0, 0);
        (track.width, track.height)
    }

    /// The drawable zone at a position: the track adjusted by the bleed.
    ///
    /// At bleed zero this *is* the track, so the zones tile the boundary
    /// exactly and no red can show. Negative exposes bleed; positive covers
    /// it by expanding past the track.
    pub fn cell(&self, row: u8, column: u8) -> Rect {
        self.track(row, column)
            .grow(self.bleed_x as i32, self.bleed_y as i32)
    }

    pub fn zone_count(&self) -> u16 {
        self.rows as u16 * self.columns as u16
    }
}

/// Why a layout is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LayoutProblem {
    #[error("grid must have at least one row and one column")]
    EmptyGrid,
    #[error("grid boundary {0:?} falls outside the {PANEL_WIDTH}x{PANEL_HEIGHT} panel")]
    BoundsOutsidePanel(Rect),
    #[error("zone {index} ({row},{column}) is empty — bleed {bleed} exceeds the cell")]
    ZoneCollapsed {
        index: u8,
        row: u8,
        column: u8,
        bleed: i16,
    },
    #[error("zone {index} at {bounds:?} falls outside the panel")]
    ZoneOutsidePanel { index: u8, bounds: Rect },
    #[error("override for zone {0} is outside the {1}-zone matrix")]
    OverrideOutOfRange(u8, u16),
    #[error("{axis} {at} has no extent ({start}..{end})")]
    BandCollapsed {
        axis: &'static str,
        at: u8,
        start: u16,
        end: u16,
    },
    #[error("{axis} {at} starts before the one ahead of it")]
    BandsOutOfOrder { axis: &'static str, at: u8 },
    #[error("{axis} {at} band {start}..{end} falls outside the panel")]
    BandOutsidePanel {
        axis: &'static str,
        at: u8,
        start: u16,
        end: u16,
    },
}

/// A parse or IO failure reading a layout file.
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("layout file is version {0}, this build understands {LAYOUT_FORMAT_VERSION}")]
    UnsupportedVersion(u32),
    #[error("line {line}: unknown field {name:?}")]
    UnknownField { line: usize, name: String },
    #[error("line {line}: {field} expects {expected} numbers, got {got}")]
    BadValue {
        line: usize,
        field: String,
        expected: usize,
        got: usize,
    },
    #[error("calibrated layout is not usable: {0}")]
    Invalid(#[from] LayoutProblem),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Where a layout's numbers came from — worth knowing before trusting them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Arithmetic defaults. Not measured on any hardware.
    Template,
    /// Read from a file on disk.
    File,
    /// Produced by a calibration session on this unit.
    Calibrated,
}

/// The calibrated geometry of one physical unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Visible area of the info screen, above the zones.
    pub screen: Rect,
    /// The zone grid.
    pub grid: Grid,
    /// Measured extent of each row. Empty means the rows are derived by
    /// dividing [`Grid::bounds`]; when present there is one per row and it
    /// takes precedence over that division.
    row_bands: Vec<Band>,
    /// Measured extent of each column, same rule.
    column_bands: Vec<Band>,
    /// Absolute per-zone overrides, keyed by row-major index. Sparse: a
    /// zone with no entry derives from the grid or its band.
    overrides: Vec<(u8, Rect)>,
    pub source: Source,
}

impl Layout {
    /// Starting point for calibration, not a specification.
    ///
    /// The top of the zone area is 432, observed as a good starting position
    /// on physical hardware (firmware 3.05.005). Everything else is
    /// arithmetic: the remaining height divided 4 rows by 3 columns. Panel
    /// alignment varies between units, so treat a layout still carrying
    /// [`Source::Template`] as uncalibrated whatever its numbers look like.
    pub const TEMPLATE: Layout = Layout {
        screen: Rect::new(0, 0, 720, 384),
        grid: Grid {
            bounds: Rect::new(0, 432, 720, 848),
            rows: 4,
            columns: 3,
            bleed_x: 0,
            bleed_y: 0,
        },
        row_bands: Vec::new(),
        column_bands: Vec::new(),
        overrides: Vec::new(),
        source: Source::Template,
    };

    pub fn rows(&self) -> u8 {
        self.grid.rows
    }

    pub fn columns(&self) -> u8 {
        self.grid.columns
    }

    pub fn zone_count(&self) -> u16 {
        self.grid.zone_count()
    }

    fn index_of(&self, row: u8, column: u8) -> Option<u8> {
        if row >= self.grid.rows || column >= self.grid.columns {
            return None;
        }
        u8::try_from(row as u16 * self.grid.columns as u16 + column as u16).ok()
    }

    /// One zone by matrix position.
    ///
    /// Precedence: a per-zone override wins outright; otherwise the column
    /// division supplies x and width, and the row's [`Band`] supplies y and
    /// height when one has been measured.
    pub fn zone(&self, row: u8, column: u8) -> Option<Zone> {
        let index = self.index_of(row, column)?;
        if let Some(bounds) = self.override_for(index) {
            return Some(Zone {
                row,
                column,
                index,
                bounds,
                overridden: true,
            });
        }
        let mut bounds = self.grid.track(row, column);
        if let Some(band) = self.row_band(row) {
            bounds.y = band.start;
            bounds.height = band.len();
        }
        if let Some(band) = self.column_band(column) {
            bounds.x = band.start;
            bounds.width = band.len();
        }
        Some(Zone {
            row,
            column,
            index,
            bounds: bounds.grow(self.grid.bleed_x as i32, self.grid.bleed_y as i32),
            overridden: false,
        })
    }

    /// The measured extent of one row, if it has been calibrated.
    pub fn row_band(&self, row: u8) -> Option<Band> {
        (self.row_bands.len() == self.grid.rows as usize)
            .then(|| self.row_bands.get(row as usize).copied())
            .flatten()
    }

    /// The measured extent of one column, if it has been calibrated.
    pub fn column_band(&self, column: u8) -> Option<Band> {
        (self.column_bands.len() == self.grid.columns as usize)
            .then(|| self.column_bands.get(column as usize).copied())
            .flatten()
    }

    /// Every row's extent, measured where calibrated and derived otherwise.
    pub fn row_bands(&self) -> Vec<Band> {
        (0..self.grid.rows)
            .map(|row| {
                self.row_band(row).unwrap_or_else(|| {
                    let track = self.grid.track(row, 0);
                    Band::new(track.y, track.bottom() as u16)
                })
            })
            .collect()
    }

    /// Every column's extent, measured where calibrated and derived otherwise.
    pub fn column_bands(&self) -> Vec<Band> {
        (0..self.grid.columns)
            .map(|column| {
                self.column_band(column).unwrap_or_else(|| {
                    let track = self.grid.track(0, column);
                    Band::new(track.x, track.right() as u16)
                })
            })
            .collect()
    }

    /// Measure one row. Seeds every band from the current division on first
    /// use, so calibrating one row does not collapse the others.
    pub fn set_row(&mut self, row: u8, band: Band) -> Result<(), LayoutProblem> {
        if row >= self.grid.rows {
            return Err(LayoutProblem::OverrideOutOfRange(row, self.zone_count()));
        }
        if self.row_bands.len() != self.grid.rows as usize {
            self.row_bands = self.row_bands();
        }
        self.row_bands[row as usize] = band;
        Ok(())
    }

    /// Measure one column.
    pub fn set_column(&mut self, column: u8, band: Band) -> Result<(), LayoutProblem> {
        if column >= self.grid.columns {
            return Err(LayoutProblem::OverrideOutOfRange(column, self.zone_count()));
        }
        if self.column_bands.len() != self.grid.columns as usize {
            self.column_bands = self.column_bands();
        }
        self.column_bands[column as usize] = band;
        Ok(())
    }

    /// Discard every measured row and column, going back to dividing the
    /// boundary.
    pub fn clear_bands(&mut self) {
        self.row_bands.clear();
        self.column_bands.clear();
    }

    /// One zone by the row-major index the device uses for keys.
    pub fn zone_at(&self, index: u8) -> Option<Zone> {
        if self.grid.columns == 0 {
            return None;
        }
        self.zone(index / self.grid.columns, index % self.grid.columns)
    }

    /// Every zone, row-major. This is the matrix a consumer iterates.
    pub fn zones(&self) -> Vec<Zone> {
        (0..self.grid.rows)
            .flat_map(|row| (0..self.grid.columns).map(move |column| (row, column)))
            .filter_map(|(row, column)| self.zone(row, column))
            .collect()
    }

    /// The matrix as rows of zones, for a consumer that wants the shape as
    /// well as the contents.
    pub fn matrix(&self) -> Vec<Vec<Zone>> {
        (0..self.grid.rows)
            .map(|row| {
                (0..self.grid.columns)
                    .filter_map(|column| self.zone(row, column))
                    .collect()
            })
            .collect()
    }

    fn override_for(&self, index: u8) -> Option<Rect> {
        self.overrides
            .iter()
            .find(|(at, _)| *at == index)
            .map(|(_, rect)| *rect)
    }

    /// Pin one zone to an absolute rect, overriding the grid.
    ///
    /// Overrides are stored separately from the grid on purpose: re-running
    /// the bounds or rows/columns stages replaces the grid without silently
    /// discarding corrections made to individual zones.
    pub fn set_zone(&mut self, index: u8, bounds: Rect) -> Result<(), LayoutProblem> {
        if u16::from(index) >= self.zone_count() {
            return Err(LayoutProblem::OverrideOutOfRange(index, self.zone_count()));
        }
        match self.overrides.iter_mut().find(|(at, _)| *at == index) {
            Some(entry) => entry.1 = bounds,
            None => self.overrides.push((index, bounds)),
        }
        self.overrides.sort_by_key(|(at, _)| *at);
        Ok(())
    }

    /// Shift and resize one zone relative to where it sits now.
    ///
    /// The result is kept on the panel here rather than left to the caller.
    /// Clamping x and width independently let a nudge walk a zone past the
    /// right edge: the layout still validated, but `panel_region_reports`
    /// rejected it, and the error surfaced out of the caller's draw pass.
    pub fn nudge_zone(
        &mut self,
        index: u8,
        dx: i32,
        dy: i32,
        dwidth: i32,
        dheight: i32,
    ) -> Result<Rect, LayoutProblem> {
        let zone = self
            .zone_at(index)
            .ok_or(LayoutProblem::OverrideOutOfRange(index, self.zone_count()))?;
        let mcu = REGION_MCU as i32;
        let moved = Rect::new(
            (zone.bounds.x as i32 + dx).clamp(0, PANEL_WIDTH as i32) as u16,
            (zone.bounds.y as i32 + dy).clamp(0, PANEL_HEIGHT as i32) as u16,
            (zone.bounds.width as i32 + dwidth).clamp(mcu, PANEL_WIDTH as i32) as u16,
            (zone.bounds.height as i32 + dheight).clamp(mcu, PANEL_HEIGHT as i32) as u16,
        )
        .clamped_to_panel();
        self.set_zone(index, moved)?;
        Ok(moved)
    }

    /// Drop one zone's override, returning it to the grid.
    pub fn clear_zone(&mut self, index: u8) {
        self.overrides.retain(|(at, _)| *at != index);
    }

    /// Drop every override.
    pub fn clear_overrides(&mut self) {
        self.overrides.clear();
    }

    /// The overrides, row-major.
    pub fn overrides(&self) -> &[(u8, Rect)] {
        &self.overrides
    }

    /// Replace the grid, keeping per-zone overrides that still address a
    /// zone that exists.
    pub fn set_grid(&mut self, grid: Grid) {
        let count = grid.zone_count();
        let rows = grid.rows as usize;
        self.grid = grid;
        self.overrides.retain(|(at, _)| u16::from(*at) < count);
        // Bands are per-row and per-column measurements; a count change
        // invalidates them wholesale rather than leaving a partial set.
        if self.row_bands.len() != rows {
            self.row_bands.clear();
        }
        if self.column_bands.len() != grid.columns as usize {
            self.column_bands.clear();
        }
    }

    /// Is every zone inside the panel and non-empty?
    pub fn validate(&self) -> Result<(), LayoutProblem> {
        if self.grid.rows == 0 || self.grid.columns == 0 {
            return Err(LayoutProblem::EmptyGrid);
        }
        if self.screen.right() > PANEL_WIDTH as u32
            || self.screen.bottom() > PANEL_HEIGHT as u32
        {
            return Err(LayoutProblem::BoundsOutsidePanel(self.screen));
        }
        if self.grid.bounds.right() > PANEL_WIDTH as u32
            || self.grid.bounds.bottom() > PANEL_HEIGHT as u32
        {
            return Err(LayoutProblem::BoundsOutsidePanel(self.grid.bounds));
        }
        for (index, _) in &self.overrides {
            if u16::from(*index) >= self.zone_count() {
                return Err(LayoutProblem::OverrideOutOfRange(*index, self.zone_count()));
            }
        }
        // Bands must be ordered and non-empty on both axes: a row or column
        // measured before the one ahead of it would scramble the
        // index-to-key mapping, which is a confusing bug to meet later.
        let axes: [(&str, &[Band], u16); 2] = [
            ("row", &self.row_bands, PANEL_HEIGHT),
            ("column", &self.column_bands, PANEL_WIDTH),
        ];
        for (axis, bands, limit) in axes {
            let expected = if axis == "row" {
                self.grid.rows as usize
            } else {
                self.grid.columns as usize
            };
            if bands.len() != expected {
                continue;
            }
            for (at, band) in bands.iter().enumerate() {
                let at = at as u8;
                if band.is_empty() {
                    return Err(LayoutProblem::BandCollapsed {
                        axis,
                        at,
                        start: band.start,
                        end: band.end,
                    });
                }
                if band.end as u32 > limit as u32 {
                    return Err(LayoutProblem::BandOutsidePanel {
                        axis,
                        at,
                        start: band.start,
                        end: band.end,
                    });
                }
                if at > 0 && band.start < bands[at as usize - 1].start {
                    return Err(LayoutProblem::BandsOutOfOrder { axis, at });
                }
            }
        }
        for zone in self.zones() {
            if !zone.bounds.is_drawable() {
                return Err(LayoutProblem::ZoneCollapsed {
                    index: zone.index,
                    row: zone.row,
                    column: zone.column,
                    bleed: self.grid.bleed_x.min(self.grid.bleed_y),
                });
            }
            if zone.bounds.right() > PANEL_WIDTH as u32
                || zone.bounds.bottom() > PANEL_HEIGHT as u32
            {
                return Err(LayoutProblem::ZoneOutsidePanel {
                    index: zone.index,
                    bounds: zone.bounds,
                });
            }
        }
        Ok(())
    }
}

// ---- text format ----
//
// Deliberately plain: the whole point of calibration is that a user can open
// the file and nudge a number, and a separate process (the daemon UI) can
// read it without agreeing on a serialisation library first.

impl Layout {
    /// Default path, honouring `XDG_CONFIG_HOME`.
    pub fn default_path() -> std::path::PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()))
                    .join(".config")
            });
        base.join("galdeck").join("layout.conf")
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("# galdeck panel layout\n");
        out.push_str("# Measured on one physical unit. Panel geometry depends on how the\n");
        out.push_str("# display sits behind the bezel, so these values are not a spec.\n");
        out.push_str("# Edit a zone line to correct one key; delete it to fall back to\n");
        out.push_str("# the grid.\n");
        out.push_str("#\n");
        out.push_str("# bleed: 0 means each zone exactly fills its track. Negative pulls\n");
        out.push_str("# the zone in and exposes bleed; positive pushes it out over the\n");
        out.push_str("# bleed, under the keycap plastic.\n\n");
        out.push_str(&format!("version = {LAYOUT_FORMAT_VERSION}\n"));
        out.push_str(&format!("panel = {PANEL_WIDTH},{PANEL_HEIGHT}\n"));
        out.push_str(&format!(
            "screen = {},{},{},{}\n",
            self.screen.x, self.screen.y, self.screen.width, self.screen.height
        ));
        out.push_str(&format!(
            "bounds = {},{},{},{}\n",
            self.grid.bounds.x, self.grid.bounds.y, self.grid.bounds.width, self.grid.bounds.height
        ));
        out.push_str(&format!("matrix = {},{}\n", self.grid.rows, self.grid.columns));
        if self.row_bands.len() == self.grid.rows as usize {
            out.push_str("\n# measured row extents: row<n> = top,bottom\n");
            for (row, band) in self.row_bands.iter().enumerate() {
                out.push_str(&format!("row{row} = {},{}\n", band.start, band.end));
            }
        }
        if self.column_bands.len() == self.grid.columns as usize {
            out.push_str("\n# measured column extents: col<n> = left,right\n");
            for (column, band) in self.column_bands.iter().enumerate() {
                out.push_str(&format!("col{column} = {},{}\n", band.start, band.end));
            }
        }
        out.push_str(&format!(
            "bleed = {},{}\n",
            self.grid.bleed_x, self.grid.bleed_y
        ));
        if !self.overrides.is_empty() {
            out.push_str("\n# per-zone overrides: zone<index> = x,y,width,height\n");
            for (index, rect) in &self.overrides {
                out.push_str(&format!(
                    "zone{index} = {},{},{},{}\n",
                    rect.x, rect.y, rect.width, rect.height
                ));
            }
        }
        out.push_str("\n# derived zones, for reference — regenerated on every write\n");
        for zone in self.zones() {
            out.push_str(&format!(
                "# [{},{}] index {:>2} = {},{},{},{}\n",
                zone.row,
                zone.column,
                zone.index,
                zone.bounds.x,
                zone.bounds.y,
                zone.bounds.width,
                zone.bounds.height
            ));
        }
        out
    }

    pub fn from_text(text: &str) -> Result<Layout, LayoutError> {
        let mut layout = Layout {
            source: Source::File,
            ..Layout::TEMPLATE
        };
        let mut overrides = Vec::new();
        let mut rows: Vec<(u8, Band)> = Vec::new();
        let mut columns: Vec<(u8, Band)> = Vec::new();

        for (number, raw) in text.lines().enumerate() {
            let line = number + 1;
            let content = raw.split('#').next().unwrap_or("").trim();
            if content.is_empty() {
                continue;
            }
            let Some((name, value)) = content.split_once('=') else {
                continue;
            };
            let (name, value) = (name.trim(), value.trim());
            let numbers: Vec<i32> = value
                .split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect();
            let expect = |want: usize| -> Result<(), LayoutError> {
                if numbers.len() == want {
                    Ok(())
                } else {
                    Err(LayoutError::BadValue {
                        line,
                        field: name.to_string(),
                        expected: want,
                        got: numbers.len(),
                    })
                }
            };
            let rect = |n: &[i32]| {
                Rect::new(
                    n[0].max(0) as u16,
                    n[1].max(0) as u16,
                    n[2].max(0) as u16,
                    n[3].max(0) as u16,
                )
            };

            match name {
                // Version is checked first so a future file reports its
                // version rather than a confusing unknown-field error.
                "version" => {
                    expect(1)?;
                    let version = numbers[0].max(0) as u32;
                    if !(LAYOUT_FORMAT_MINIMUM..=LAYOUT_FORMAT_VERSION).contains(&version) {
                        return Err(LayoutError::UnsupportedVersion(version));
                    }
                }
                // Informational: the panel extent is a build constant, not
                // something a layout file gets to redefine.
                "panel" => {}
                "screen" => {
                    expect(4)?;
                    layout.screen = rect(&numbers);
                }
                "bounds" => {
                    expect(4)?;
                    layout.grid.bounds = rect(&numbers);
                }
                "matrix" => {
                    expect(2)?;
                    layout.grid.rows = numbers[0].clamp(0, 255) as u8;
                    layout.grid.columns = numbers[1].clamp(0, 255) as u8;
                }
                "bleed" => {
                    expect(2)?;
                    layout.grid.bleed_x = numbers[0] as i16;
                    layout.grid.bleed_y = numbers[1] as i16;
                }
                row if row.starts_with("row") => {
                    let index: u8 = row[3..].parse().map_err(|_| LayoutError::UnknownField {
                        line,
                        name: row.to_string(),
                    })?;
                    expect(2)?;
                    rows.push((
                        index,
                        Band::new(numbers[0].max(0) as u16, numbers[1].max(0) as u16),
                    ));
                }
                column if column.starts_with("col") => {
                    let index: u8 = column[3..].parse().map_err(|_| LayoutError::UnknownField {
                        line,
                        name: column.to_string(),
                    })?;
                    expect(2)?;
                    columns.push((
                        index,
                        Band::new(numbers[0].max(0) as u16, numbers[1].max(0) as u16),
                    ));
                }
                zone if zone.starts_with("zone") => {
                    let index: u8 = zone[4..].parse().map_err(|_| LayoutError::UnknownField {
                        line,
                        name: zone.to_string(),
                    })?;
                    expect(4)?;
                    overrides.push((index, rect(&numbers)));
                }
                other => {
                    return Err(LayoutError::UnknownField {
                        line,
                        name: other.to_string(),
                    })
                }
            }
        }

        rows.sort_by_key(|(row, _)| *row);
        for (row, band) in rows {
            layout.set_row(row, band)?;
        }
        columns.sort_by_key(|(column, _)| *column);
        for (column, band) in columns {
            layout.set_column(column, band)?;
        }
        for (index, bounds) in overrides {
            layout.set_zone(index, bounds)?;
        }
        layout.validate()?;
        Ok(layout)
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Layout, LayoutError> {
        Layout::from_text(&std::fs::read_to_string(path)?)
    }

    /// Write atomically: a half-written layout read by the daemon mid-save
    /// would be worse than no layout at all.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> Result<(), LayoutError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("conf.tmp");
        std::fs::write(&temporary, self.to_text())?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    }

    /// The calibrated layout as JSON, for another process to ingest.
    ///
    /// The text format is the editable one a person keeps; this is the
    /// machine-readable view, and it is deliberately *derived* — every zone
    /// is emitted fully resolved, with the grid division, row and column
    /// bands and per-zone overrides already applied, so a consumer never has
    /// to reimplement the precedence rules to know where a key is.
    ///
    /// Each zone also carries `drawable`: the rect actually sent to the
    /// device, rounded out to whole region blocks. Draw to that and the
    /// firmware's multiple-of-16 rule is already satisfied.
    ///
    /// Hand-rolled rather than pulling in a serialiser: every value is a
    /// number or a fixed keyword, so there is nothing to escape.
    pub fn to_json(&self) -> String {
        fn rect(r: &Rect) -> String {
            format!(
                r#"{{"x":{},"y":{},"width":{},"height":{}}}"#,
                r.x, r.y, r.width, r.height
            )
        }
        fn bands(bands: &[Band]) -> String {
            let items: Vec<String> = bands
                .iter()
                .map(|b| format!(r#"{{"start":{},"end":{}}}"#, b.start, b.end))
                .collect();
            format!("[{}]", items.join(","))
        }

        let zones: Vec<String> = self
            .zones()
            .iter()
            .map(|zone| {
                format!(
                    r#"    {{"index":{},"row":{},"column":{},"overridden":{},"bounds":{},"drawable":{}}}"#,
                    zone.index,
                    zone.row,
                    zone.column,
                    zone.overridden,
                    rect(&zone.bounds),
                    rect(&zone.bounds.to_mcu_covering())
                )
            })
            .collect();

        format!(
            concat!(
                "{{\n",
                r#"  "version":{},"#,
                "\n",
                r#"  "source":"{}","#,
                "\n",
                r#"  "panel":{{"width":{},"height":{}}},"#,
                "\n",
                r#"  "regionBlock":{},"#,
                "\n",
                r#"  "screen":{},"#,
                "\n",
                r#"  "grid":{{"rows":{},"columns":{},"bounds":{},"bleed":{{"x":{},"y":{}}}}},"#,
                "\n",
                r#"  "rows":{},"#,
                "\n",
                r#"  "columns":{},"#,
                "\n",
                r#"  "zones":["#,
                "\n{}\n  ]\n}}\n"
            ),
            LAYOUT_FORMAT_VERSION,
            match self.source {
                Source::Template => "template",
                Source::File => "file",
                Source::Calibrated => "calibrated",
            },
            PANEL_WIDTH,
            PANEL_HEIGHT,
            REGION_MCU,
            rect(&self.screen),
            self.grid.rows,
            self.grid.columns,
            rect(&self.grid.bounds),
            self.grid.bleed_x,
            self.grid.bleed_y,
            bands(&self.row_bands()),
            bands(&self.column_bands()),
            zones.join(",\n"),
        )
    }

    /// The matrix as a table, for a terminal.
    pub fn to_ascii(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "{}x{} matrix in {},{} {}x{}  bleed {},{}  [{:?}]\n",
            self.grid.rows,
            self.grid.columns,
            self.grid.bounds.x,
            self.grid.bounds.y,
            self.grid.bounds.width,
            self.grid.bounds.height,
            self.grid.bleed_x,
            self.grid.bleed_y,
            self.source,
        ));
        for row in self.matrix() {
            for zone in &row {
                out.push_str(&format!(
                    "  {:>2}:{:>4},{:<4} {:>3}x{:<3}{}",
                    zone.index,
                    zone.bounds.x,
                    zone.bounds.y,
                    zone.bounds.width,
                    zone.bounds.height,
                    if zone.overridden { "*" } else { " " },
                ));
            }
            out.push('\n');
        }
        if !self.overrides.is_empty() {
            out.push_str("  * = per-zone override\n");
        }
        out
    }
}


// ---- graphical representation ----

impl Layout {
    /// A scale-to-fit schematic of the calibrated panel: the info screen,
    /// the zone boundary, every zone, and the bleed between them.
    ///
    /// Drawn at the end of a calibration session so the result can be seen
    /// as a whole rather than inferred from six numbers — and available to
    /// any consumer that wants to show the same picture in its own UI.
    ///
    /// Colours match the calibration pattern: green is a zone, red is bleed
    /// the keycaps are meant to cover.
    pub fn schematic(&self, width: u32, height: u32, font: Option<&crate::Font>) -> Canvas {
        const MARGIN: u32 = 10;
        let mut canvas = Canvas::filled(width, height, Rgb::new(12, 12, 16));

        let usable_w = width.saturating_sub(MARGIN * 2);
        let usable_h = height.saturating_sub(MARGIN * 2);
        if usable_w == 0 || usable_h == 0 {
            return canvas;
        }
        // Uniform scale, so the schematic keeps the panel's real proportions
        // — a squashed picture would misrepresent the thing being measured.
        let scale = (usable_w as f32 / PANEL_WIDTH as f32)
            .min(usable_h as f32 / PANEL_HEIGHT as f32);
        let offset_x = MARGIN as i32 + ((usable_w as f32 - PANEL_WIDTH as f32 * scale) / 2.0) as i32;
        let offset_y =
            MARGIN as i32 + ((usable_h as f32 - PANEL_HEIGHT as f32 * scale) / 2.0) as i32;
        let map = |value: u16| (value as f32 * scale) as i32;
        let span = |value: u16| ((value as f32 * scale) as u32).max(1);

        // The panel outline.
        canvas.draw_rect(
            offset_x,
            offset_y,
            span(PANEL_WIDTH),
            span(PANEL_HEIGHT),
            Rgb::new(70, 70, 80),
        );

        // The info screen.
        canvas.fill_rect(
            offset_x + map(self.screen.x),
            offset_y + map(self.screen.y),
            span(self.screen.width),
            span(self.screen.height),
            Rgb::new(26, 34, 52),
        );

        // The zone boundary, and the bleed inside it.
        canvas.fill_rect(
            offset_x + map(self.grid.bounds.x),
            offset_y + map(self.grid.bounds.y),
            span(self.grid.bounds.width),
            span(self.grid.bounds.height),
            Rgb::new(150, 20, 20),
        );

        for zone in self.zones() {
            let (x, y) = (
                offset_x + map(zone.bounds.x),
                offset_y + map(zone.bounds.y),
            );
            let (w, h) = (span(zone.bounds.width), span(zone.bounds.height));
            canvas.fill_rect(x, y, w, h, Rgb::new(10, 40, 22));
            canvas.draw_rect(
                x,
                y,
                w,
                h,
                if zone.overridden {
                    Rgb::new(255, 190, 0)
                } else {
                    Rgb::new(0, 230, 90)
                },
            );
            if let Some(font) = font {
                canvas.draw_text(
                    &zone.index.to_string(),
                    x + w as i32 / 2,
                    y + h as i32 / 2 + 6,
                    &crate::TextStyle::new(font, 16.0)
                        .align(crate::Align::Center)
                        .color(Rgb::new(190, 220, 200)),
                );
            }
        }
        canvas
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: u8, columns: u8, bleed: i16) -> Layout {
        Layout {
            grid: Grid {
                bounds: Rect::new(0, 384, 720, 896),
                rows,
                columns,
                bleed_x: bleed,
                bleed_y: bleed,
            },
            ..Layout::TEMPLATE
        }
    }

    #[test]
    fn template_derives_a_full_matrix() {
        let layout = Layout::TEMPLATE;
        layout.validate().unwrap();
        assert_eq!(layout.zone_count(), 12);
        assert_eq!(layout.zones().len(), 12);

        // Row-major indexing must match the key index the device reports.
        let zone = layout.zone(1, 2).unwrap();
        assert_eq!(zone.index, 5);
        assert_eq!(layout.zone_at(5).unwrap(), zone);

        // Track divisions snap to whole region blocks, so the rows are not
        // all the same height when the boundary does not divide evenly.
        assert_eq!(zone.bounds, Rect::new(480, 640, 240, 224));
        for zone in layout.zones() {
            assert_eq!(zone.bounds.width % 16, 0);
            assert_eq!(zone.bounds.height % 16, 0);
        }
    }

    #[test]
    fn zero_bleed_means_zones_exactly_fill_their_tracks() {
        // The user-facing contract: green at zero, no red anywhere.
        let layout = grid(4, 3, 0);
        for zone in layout.zones() {
            assert_eq!(zone.bounds, layout.grid.track(zone.row, zone.column));
        }
        // And the tracks together cover the whole boundary.
        let first = layout.zone(0, 0).unwrap().bounds;
        let last = layout.zone(3, 2).unwrap().bounds;
        assert_eq!((first.x, first.y), (0, 384));
        assert_eq!((last.right(), last.bottom()), (720, 1280));
    }

    #[test]
    fn negative_bleed_exposes_red_and_positive_covers_it() {
        let pulled_in = grid(4, 3, -8).zone(0, 0).unwrap().bounds;
        assert_eq!(pulled_in, Rect::new(8, 392, 224, 208));

        // Positive pushes the zone out over the bleed, overlapping the
        // neighbouring track — hidden by the keycap plastic.
        let pushed_out = grid(4, 3, 8).zone(1, 1).unwrap().bounds;
        let track = grid(4, 3, 8).grid.track(1, 1);
        assert_eq!(pushed_out.x, track.x - 8);
        assert_eq!(pushed_out.width, track.width + 16);
    }

    #[test]
    fn bleed_that_eats_the_cell_is_a_validation_error() {
        let layout = grid(4, 3, -200);
        assert!(matches!(
            layout.validate(),
            Err(LayoutProblem::ZoneCollapsed { .. })
        ));
    }

    #[test]
    fn zones_never_overlap_under_the_template() {
        let zones = Layout::TEMPLATE.zones();
        for (i, a) in zones.iter().enumerate() {
            for b in &zones[i + 1..] {
                assert!(!a.bounds.overlaps(&b.bounds), "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn overrides_replace_one_zone_and_survive_a_regrid() {
        let mut layout = Layout::TEMPLATE;
        layout.set_zone(4, Rect::new(100, 700, 200, 200)).unwrap();
        assert!(layout.zone_at(4).unwrap().overridden);
        assert_eq!(layout.zone_at(4).unwrap().bounds, Rect::new(100, 700, 200, 200));
        assert!(!layout.zone_at(5).unwrap().overridden);

        // Re-measuring the boundary must not silently discard corrections.
        layout.set_grid(Grid {
            bounds: Rect::new(0, 400, 720, 880),
            ..layout.grid
        });
        assert_eq!(layout.zone_at(4).unwrap().bounds, Rect::new(100, 700, 200, 200));

        layout.clear_zone(4);
        assert!(!layout.zone_at(4).unwrap().overridden);
    }

    #[test]
    fn shrinking_the_matrix_drops_overrides_that_no_longer_address_a_zone() {
        let mut layout = Layout::TEMPLATE;
        layout.set_zone(11, Rect::new(0, 400, 100, 100)).unwrap();
        layout.set_grid(Grid {
            rows: 2,
            columns: 3,
            ..layout.grid
        });
        assert_eq!(layout.zone_count(), 6);
        assert!(layout.overrides().is_empty());
        layout.validate().unwrap();
    }

    #[test]
    fn out_of_range_override_is_rejected_rather_than_stored() {
        let mut layout = Layout::TEMPLATE;
        assert!(matches!(
            layout.set_zone(12, Rect::new(0, 0, 8, 8)),
            Err(LayoutProblem::OverrideOutOfRange(12, 12))
        ));
    }

    #[test]
    fn nudge_moves_relative_to_the_current_bounds() {
        let mut layout = Layout::TEMPLATE;
        let before = layout.zone_at(0).unwrap().bounds;
        layout.nudge_zone(0, 4, -8, 0, 0).unwrap();
        let after = layout.zone_at(0).unwrap().bounds;
        assert_eq!(after.x, before.x + 4);
        assert_eq!(after.y, before.y - 8);
        assert_eq!(after.width, before.width);
    }

    #[test]
    fn text_round_trips_including_overrides() {
        let mut layout = grid(4, 3, 8);
        layout.set_zone(7, Rect::new(250, 900, 216, 200)).unwrap();
        let parsed = Layout::from_text(&layout.to_text()).unwrap();
        assert_eq!(parsed.grid, layout.grid);
        assert_eq!(parsed.screen, layout.screen);
        assert_eq!(parsed.overrides(), layout.overrides());
        assert_eq!(parsed.zones(), layout.zones());
    }

    #[test]
    fn a_future_version_is_named_rather_than_misread() {
        let text = format!("version = {}\n", LAYOUT_FORMAT_VERSION + 1);
        assert!(matches!(
            Layout::from_text(&text),
            Err(LayoutError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn measured_bands_override_the_derived_division() {
        let mut layout = Layout::TEMPLATE;
        let derived = layout.zone(1, 0).unwrap().bounds;

        layout.set_row(1, Band::new(700, 900)).unwrap();
        let measured = layout.zone(1, 0).unwrap().bounds;
        assert_eq!((measured.y, measured.height), (700, 200));
        assert_ne!(measured.y, derived.y);
        // The column division still supplies x and width.
        assert_eq!((measured.x, measured.width), (derived.x, derived.width));
        // Setting one row seeds the rest from the division rather than
        // collapsing them.
        assert_eq!(layout.row_bands().len(), 4);
        assert_eq!(layout.zone(0, 0).unwrap().bounds.y, Layout::TEMPLATE.zone(0, 0).unwrap().bounds.y);
        layout.validate().unwrap();

        layout.clear_bands();
        assert_eq!(layout.zone(1, 0).unwrap().bounds, derived);
    }

    #[test]
    fn bands_must_be_ordered_and_non_empty() {
        let mut layout = Layout::TEMPLATE;
        layout.set_row(2, Band::new(500, 500)).unwrap();
        assert!(matches!(
            layout.validate(),
            Err(LayoutProblem::BandCollapsed { axis: "row", at: 2, .. })
        ));

        let mut layout = Layout::TEMPLATE;
        // Row 2 measured above row 1 would scramble index-to-key mapping.
        layout.set_row(2, Band::new(440, 600)).unwrap();
        assert!(matches!(
            layout.validate(),
            Err(LayoutProblem::BandsOutOfOrder { axis: "row", at: 2 })
        ));
    }

    #[test]
    fn changing_the_row_count_discards_stale_bands() {
        let mut layout = Layout::TEMPLATE;
        layout.set_row(0, Band::new(440, 600)).unwrap();
        assert_eq!(layout.row_bands.len(), 4);
        layout.set_grid(Grid {
            rows: 3,
            ..layout.grid
        });
        assert!(layout.row_bands.is_empty(), "stale bands survived a regrid");
        layout.validate().unwrap();
    }

    #[test]
    fn bands_round_trip_through_the_text_format() {
        let mut layout = Layout::TEMPLATE;
        layout.set_row(0, Band::new(432, 640)).unwrap();
        layout.set_row(1, Band::new(648, 872)).unwrap();
        layout.set_row(2, Band::new(880, 1088)).unwrap();
        layout.set_row(3, Band::new(1096, 1280)).unwrap();
        let parsed = Layout::from_text(&layout.to_text()).unwrap();
        assert_eq!(parsed.row_bands(), layout.row_bands());
        assert_eq!(parsed.zones(), layout.zones());
    }

    #[test]
    fn measured_columns_override_the_derived_division() {
        let mut layout = Layout::TEMPLATE;
        let derived = layout.zone(0, 1).unwrap().bounds;
        layout.set_column(1, Band::new(300, 500)).unwrap();
        let measured = layout.zone(0, 1).unwrap().bounds;
        assert_eq!((measured.x, measured.width), (300, 200));
        // The row division still supplies y and height.
        assert_eq!((measured.y, measured.height), (derived.y, derived.height));
        assert_eq!(layout.column_bands().len(), 3);
        layout.validate().unwrap();
    }

    #[test]
    fn rows_and_columns_compose_into_one_zone() {
        let mut layout = Layout::TEMPLATE;
        layout.set_row(2, Band::new(900, 1100)).unwrap();
        layout.set_column(2, Band::new(480, 660)).unwrap();
        let zone = layout.zone(2, 2).unwrap();
        assert_eq!(zone.bounds, Rect::new(480, 900, 180, 200));
        assert_eq!(zone.index, 8);
        layout.validate().unwrap();
    }

    #[test]
    fn columns_round_trip_and_validate_like_rows() {
        let mut layout = Layout::TEMPLATE;
        layout.set_column(0, Band::new(52, 260)).unwrap();
        layout.set_column(1, Band::new(260, 468)).unwrap();
        layout.set_column(2, Band::new(468, 676)).unwrap();
        let parsed = Layout::from_text(&layout.to_text()).unwrap();
        assert_eq!(parsed.column_bands(), layout.column_bands());

        let mut broken = layout.clone();
        broken.set_column(2, Band::new(100, 200)).unwrap();
        assert!(matches!(
            broken.validate(),
            Err(LayoutProblem::BandsOutOfOrder { axis: "column", at: 2 })
        ));
    }

    #[test]
    fn a_version_2_file_still_loads_with_derived_rows() {
        // The previous format had no bands; its rows came from dividing the
        // boundary, so deriving them is exactly what it meant. Refusing it
        // would throw away a real calibration.
        let text = "version = 2\nbounds = 52,432,624,848\nmatrix = 4,3\nbleed = 0,0\n";
        let layout = Layout::from_text(text).unwrap();
        assert_eq!(layout.grid.bounds, Rect::new(52, 432, 624, 848));
        assert_eq!(layout.row_bands().len(), 4);
    }

    #[test]
    fn a_version_1_file_is_refused_rather_than_misread() {
        // v1 `bleed` was an unsigned inset; under the current rule the same
        // number means an outset, so a silent read would move every zone.
        let text = "version = 1\nbleed = 12,12\n";
        assert!(matches!(
            Layout::from_text(text),
            Err(LayoutError::UnsupportedVersion(1))
        ));
    }

    #[test]
    fn track_sizes_are_block_aligned_wherever_the_boundary_starts() {
        // A boundary whose rows do not divide into whole blocks: the
        // interior lines snap, so no track needs draw-time rounding.
        let layout = Layout {
            grid: Grid {
                bounds: Rect::new(0, 452, 720, 828),
                rows: 4,
                columns: 3,
                bleed_x: 0,
                bleed_y: 0,
            },
            ..Layout::TEMPLATE
        };
        // Interior tracks are whole blocks tall...
        for row in 0..3 {
            assert_eq!(
                layout.grid.track(row, 0).height % 16,
                0,
                "track row {row} is not a whole number of region blocks"
            );
        }
        // ...and the outer edges stay exactly where they were put, so the
        // boundary can still be nudged a pixel at a time.
        assert_eq!(layout.grid.row_edge(0), 452);
        assert_eq!(layout.grid.row_edge(4), 1280);
        // And the tracks still tile with no gap.
        for row in 0..3 {
            assert_eq!(
                layout.grid.track(row, 0).bottom(),
                layout.grid.track(row + 1, 0).y as u32
            );
        }
    }

    #[test]
    fn no_grid_top_leaves_the_panel_bottom_uncovered() {
        // The reported bug: with the bottom pinned at the panel edge, some
        // grid tops stranded a few rows of red that no knob could reach.
        for top in (152..640).step_by(1) {
            let layout = Layout {
                grid: Grid {
                    bounds: Rect::new(0, top, 720, PANEL_HEIGHT - top),
                    rows: 4,
                    columns: 3,
                    bleed_x: 0,
                    bleed_y: 0,
                },
                ..Layout::TEMPLATE
            };
            let last = layout.zone(3, 0).unwrap().bounds.to_mcu_covering();
            assert_eq!(
                last.bottom(),
                PANEL_HEIGHT as u32,
                "top {top} leaves {} rows of red at the panel bottom",
                PANEL_HEIGHT as u32 - last.bottom()
            );
            assert_eq!(last.height % 16, 0);
        }
    }

    #[test]
    fn a_one_pixel_boundary_nudge_actually_moves_the_zones() {
        // Single-stepping the boundary must move the geometry. Quantising
        // the origin to whole blocks silently ate steps of 1, 2 and 4.
        for top in 448..472u16 {
            let grid = Grid {
                bounds: Rect::new(0, top, 720, PANEL_HEIGHT - top),
                rows: 4,
                columns: 3,
                bleed_x: 0,
                bleed_y: 0,
            };
            assert_eq!(grid.track(0, 0).y, top, "top {top} was absorbed");
            for row in 0..3 {
                assert_eq!(grid.track(row, 0).height % 16, 0);
            }
        }
    }

    #[test]
    fn covering_never_moves_an_edge_the_caller_chose() {
        // A boundary whose right edge is not on a block and is NOT flush
        // against the panel: the width shrinks, the left edge stays put.
        // Shifting here is what made the left bound wander while the right
        // bound was being adjusted.
        let chosen = Rect::new(3, 400, 714, 200);
        let painted = chosen.to_mcu_covering();
        assert_eq!(painted.x, 3, "left edge moved");
        assert_eq!(painted.width % 16, 0);
        assert!(painted.right() <= PANEL_WIDTH as u32);

        // Sweeping the right edge one pixel at a time must never produce a
        // shearable width, and may only move the left edge in the one case
        // where the span is already flush with the panel edge.
        for width in 1..=(PANEL_WIDTH - 3) {
            let span = Rect::new(3, 400, width, 200);
            let painted = span.to_mcu_covering();
            assert_eq!(painted.width % 16, 0, "shearable width {width}");
            assert!(painted.right() <= PANEL_WIDTH as u32);
            if span.right() != PANEL_WIDTH as u32 {
                assert_eq!(painted.x, 3, "left edge moved at width {width}");
            }
        }
    }

    #[test]
    fn an_aligned_grid_needs_no_draw_time_correction() {
        // Every boundary, however awkward, becomes one whose tracks are all
        // block-aligned — so to_mcu_covering is the identity and nothing can
        // shift under the user while they adjust the opposite edge.
        for right in 600..=720u16 {
            for left in 0..8u16 {
                let grid = Grid {
                    bounds: Rect::new(left, 452, right - left, 828),
                    rows: 4,
                    columns: 3,
                    bleed_x: 0,
                    bleed_y: 0,
                }
                .aligned();
                for row in 0..4 {
                    for column in 0..3 {
                        let track = grid.track(row, column);
                        assert_eq!(
                            track.to_mcu_covering(),
                            track,
                            "track ({row},{column}) of {left}..{right} needs correction"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn covering_still_preserves_an_edge_already_flush_with_the_panel() {
        // Flush against the bottom: the origin shifts so the panel edge
        // stays covered. This is the fix for the red line at the bottom.
        let flush = Rect::new(0, PANEL_HEIGHT - 207, 240, 207);
        let painted = flush.to_mcu_covering();
        assert_eq!(painted.bottom(), PANEL_HEIGHT as u32);
        assert_eq!(painted.height, 208);

        let flush_right = Rect::new(PANEL_WIDTH - 237, 400, 237, 200);
        let painted = flush_right.to_mcu_covering();
        assert_eq!(painted.right(), PANEL_WIDTH as u32);
        assert_eq!(painted.width % 16, 0);
    }

    #[test]
    fn growing_a_zone_pinned_to_the_edge_does_not_slide_it() {
        let flush = Rect::new(0, PANEL_HEIGHT - 100, 240, 100);
        let grown = flush.grow(8, 8);
        assert_eq!(grown.bottom(), PANEL_HEIGHT as u32, "bottom edge moved");
        assert_eq!(grown.x, 0, "left edge moved");
        assert_eq!(grown.y, flush.y - 8);
    }

    #[test]
    fn a_typo_is_reported_with_its_line() {
        let text = format!("version = {LAYOUT_FORMAT_VERSION}\nbonuds = 0,384,720,896\n");
        match Layout::from_text(&text) {
            Err(LayoutError::UnknownField { line, name }) => {
                assert_eq!(line, 2);
                assert_eq!(name, "bonuds");
            }
            other => panic!("expected UnknownField, got {other:?}"),
        }
    }

    #[test]
    fn wrong_arity_is_reported_rather_than_silently_padded() {
        let text = format!("version = {LAYOUT_FORMAT_VERSION}\nbounds = 0,384\n");
        assert!(matches!(
            Layout::from_text(&text),
            Err(LayoutError::BadValue { expected: 4, got: 2, .. })
        ));
    }

    #[test]
    fn a_nudge_can_never_walk_a_zone_off_the_panel() {
        let mut layout = Layout::TEMPLATE;
        // Far further than the panel is wide or tall, in one step.
        let moved = layout.nudge_zone(2, 5000, 5000, 0, 0).unwrap();
        assert!(moved.is_drawable(), "{moved:?} would be rejected by the wire");
        assert!(moved.right() <= PANEL_WIDTH as u32);
        assert!(moved.bottom() <= PANEL_HEIGHT as u32);
        layout.validate().unwrap();

        // And shrinking cannot collapse it below one JPEG block.
        let shrunk = layout.nudge_zone(2, 0, 0, -5000, -5000).unwrap();
        assert!(shrunk.is_drawable());
    }

    #[test]
    fn an_empty_zone_fails_validation() {
        let mut layout = Layout::TEMPLATE;
        layout.overrides.push((0, Rect::new(0, 400, 0, 200)));
        assert!(matches!(
            layout.validate(),
            Err(LayoutProblem::ZoneCollapsed { index: 0, .. })
        ));
    }

    #[test]
    fn mcu_covering_paints_the_whole_zone_instead_of_stranding_a_sliver() {
        // Truncating leaves 12px unpainted; covering grows to reach it.
        let rect = Rect::new(0, 400, 220, 220);
        assert_eq!(rect.to_mcu(), Rect::new(0, 400, 208, 208));
        assert_eq!(rect.to_mcu_covering(), Rect::new(0, 400, 224, 224));

        // Already aligned: unchanged either way.
        let aligned = Rect::new(8, 400, 240, 224);
        assert_eq!(aligned.to_mcu_covering(), aligned);

        // Flush against the bottom edge: growing would leave the panel, so
        // the origin shifts back and the bottom edge is preserved.
        let flush = Rect::new(0, PANEL_HEIGHT - 220, 240, 220);
        let covered = flush.to_mcu_covering();
        assert_eq!(covered.bottom(), PANEL_HEIGHT as u32);
        assert_eq!(covered.height, 224);
        assert!(covered.is_drawable());
    }

    #[test]
    fn division_remainder_is_spread_not_stranded_at_the_edge() {
        // 719 over 3 columns: naive division loses 2px at the right edge,
        // which would show as red no bleed setting could hide.
        let layout = Layout {
            grid: Grid {
                bounds: Rect::new(0, 384, 719, 890),
                rows: 4,
                columns: 3,
                bleed_x: 0,
                bleed_y: 0,
            },
            ..Layout::TEMPLATE
        };
        let last_column = layout.zone(0, 2).unwrap();
        assert_eq!(last_column.bounds.right(), 719, "right edge must be reached");
        let last_row = layout.zone(3, 0).unwrap();
        assert_eq!(last_row.bounds.bottom(), 384 + 890);

        // And the tracks still tile without gaps or overlaps.
        for row in 0..4 {
            for column in 0..2 {
                let a = layout.grid.track(row, column);
                let b = layout.grid.track(row, column + 1);
                assert_eq!(a.right(), b.x as u32);
            }
        }
    }

    #[test]
    fn json_resolves_every_zone_for_a_consumer() {
        let mut layout = Layout::TEMPLATE;
        layout.set_row(0, Band::new(432, 640)).unwrap();
        layout.set_column(0, Band::new(52, 260)).unwrap();
        layout.set_zone(4, Rect::new(260, 700, 208, 208)).unwrap();
        layout.source = Source::Calibrated;

        let json = layout.to_json();
        assert!(json.contains(r#""source":"calibrated""#));
        assert!(json.contains(r#""regionBlock":16"#));
        assert!(json.contains(r#""rows":[{"start":432,"end":640}"#));
        assert!(json.contains(r#""columns":[{"start":52,"end":260}"#));
        // Zone 0 must already have the row and column bands applied, so a
        // consumer never reimplements the precedence rules.
        assert!(json.contains(r#""index":0,"row":0,"column":0,"overridden":false,"bounds":{"x":52,"y":432,"width":208,"height":208}"#));
        assert!(json.contains(r#""index":4,"row":1,"column":1,"overridden":true"#));
        // One entry per zone.
        assert_eq!(json.matches(r#""index":"#).count(), 12);
    }

    #[test]
    fn schematic_fits_inside_the_canvas_it_is_given() {
        let canvas = Layout::TEMPLATE.schematic(360, 200, None);
        assert_eq!((canvas.width(), canvas.height()), (360, 200));
        // A degenerate size must not panic or overflow.
        Layout::TEMPLATE.schematic(4, 4, None);
    }

    #[test]
    fn mcu_rounding_never_grows_a_rect() {
        let rect = Rect::new(3, 5, 231, 209).to_mcu();
        assert_eq!(rect, Rect::new(3, 5, 224, 208));
        assert_eq!(rect.width % 16, 0);
        assert_eq!(rect.height % 16, 0);
    }
}
