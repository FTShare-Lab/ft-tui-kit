//! Rendering infrastructure.
//!
//! Chart containers compute plot layouts, series generate geometry, and
//! rendering backends quantize it to a glyph family.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Style};

use crate::scale::{TimeScale, ValueScale};

mod block;
mod box_drawing;
mod braille;
mod line;
mod quadrant;
mod wick;

pub(crate) use block::Block;
pub(crate) use block::draw_bar;
pub(crate) use box_drawing::BoxDrawing;
pub(crate) use braille::Braille;
pub(crate) use line::LineRasterizer;
pub(crate) use line::{draw_value_line, line_value_bounds};
pub(crate) use quadrant::Quadrant;

/// The laid-out plot area together with the scales mapping data onto it.
///
/// A container computes this once for the drawn area and shares it with
/// downstream components such as series, labels, and overlays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlotLayout {
    /// The rectangle the data is drawn into, excluding any axis gutters.
    pub(crate) plot: Rect,
    /// Maps values onto the rows of `plot`.
    pub(crate) value: ValueScale,
    /// Maps candle indices onto the columns of `plot`.
    pub(crate) time: TimeScale,
    /// The color the plot was filled with, which partial cells blend against.
    pub(crate) bg: Color,
}

/// Whether a candle body is drawn solid or as an outline.
///
/// - `Filled` paints the whole body.
/// - `Hollow` traces the body's border and leaves the interior empty.
///
/// A body too small to enclose an interior is drawn filled, depending on the
/// marker that is used to render the chart.
///
/// Set per direction on a [`CandleSeries`](crate::CandleSeries) with
/// [`bull_fill`](crate::CandleSeries::bull_fill) and
/// [`bear_fill`](crate::CandleSeries::bear_fill).
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Hash)]
pub enum BodyFill {
    /// A solid body.
    #[default]
    Filled,
    /// An outlined body with an empty interior.
    Hollow,
}

/// One candle's geometry and colors.
///
/// Geometry is defined in terms of plot area, not a specific glyph family. Both
/// axes are continuous and measured from the plot's top-left corner: the body
/// spans the fractional columns `[body_left, body_right)` and the four row
/// fields are fractional rows (smaller is higher on screen). A backend quantizes
/// each axis to its own grid, so the same geometry draws through any glyph
/// family. The wick runs along the body's horizontal [`center`](Self::center).
pub(crate) struct CandleGeometry {
    /// The fractional column of the left edge of the body.
    pub body_left: f64,
    /// The fractional column of the right edge of the body.
    pub body_right: f64,
    /// The fractional row of the top of the body.
    pub body_top_row: f64,
    /// The fractional row of the bottom of the body.
    pub body_bottom_row: f64,
    /// The fractional row of the high wick.
    pub high_row: f64,
    /// The fractional row of the low wick.
    pub low_row: f64,
    /// The color of the body.
    pub body: Color,
    /// The color of the wick.
    pub wick: Color,
    /// The color the empty portion of a partially filled cell is painted.
    pub bg: Color,
    /// Whether the body is solid or an outline.
    pub fill: BodyFill,
}

impl CandleGeometry {
    /// The fractional column at the horizontal center of the body, where the
    /// wick is drawn. A backend quantizes this to the nearest sub-cell.
    pub(crate) fn center(&self) -> f64 {
        (self.body_left + self.body_right) / 2.0
    }
}

/// One bar's geometry and colors.
pub(crate) struct BarGeometry {
    /// The fractional column of the left edge of the bar.
    pub left: f64,
    /// The fractional column of the right edge of the bar.
    pub right: f64,
    /// The fractional row of the top of the bar.
    pub value_row: f64,
    /// The color of the bar.
    pub color: Color,
    /// The color the empty portion of a partially filled cell is painted.
    pub bg: Color,
}

/// Quantizes a body edge span `[start, end)` (in fractional rows or columns) to
/// a backend's sub-cell grid.
///
/// `scale` is the number of sub-cells per cell along the axis and `max` the
/// total sub-cells the plot spans. Both ends round to the nearest sub-cell. The
/// span is kept at least one sub-cell long so a doji still shows a body, then
/// clamped to `[0, max]`. Returns the half-open sub-cell span `[lo, hi)`.
pub(crate) fn quantize_span(start: f64, end: f64, scale: u32, max: u32) -> (u32, u32) {
    let s = f64::from(scale);
    let mut lo = (start * s).round() as u32;
    let mut hi = (end * s).round() as u32;
    if hi <= lo {
        hi = lo + 1;
    }
    hi = hi.min(max);
    lo = lo.min(hi - 1);
    (lo, hi)
}

/// Whether the sub-cell `(x, y)` lies on the border of the half-open rectangle
/// `[left, right) x [top, bot)`. Used to trace a one sub-cell thick ring for a
/// hollow body.
pub(crate) fn on_border(x: u32, y: u32, left: u32, right: u32, top: u32, bot: u32) -> bool {
    x == left || x + 1 == right || y == top || y + 1 == bot
}

/// Writes `symbol` styled by `style` at the plot-relative cell `(col, row)`,
/// ignoring positions outside the plot.
pub(crate) fn put(buf: &mut Buffer, plot: Rect, col: u32, row: u32, symbol: &str, style: Style) {
    if col >= u32::from(plot.width) || row >= u32::from(plot.height) {
        return;
    }
    let x = plot.x + col as u16;
    let y = plot.y + row as u16;
    if let Some(cell) = buf.cell_mut((x, y)) {
        cell.set_symbol(symbol);
        cell.set_style(style);
    }
}

/// A backend that paints fractional-row geometry into terminal cells.
///
/// Receives raster geometry and the backend quantizes it to the vertical
/// resolution of its glyphs.
pub(crate) trait Rasterizer {
    /// Draws one candle's geometry into `plot`.
    fn draw_candle(&self, buf: &mut Buffer, plot: Rect, geometry: &CandleGeometry);
}

/// A dataset that knows how to draw itself into a laid-out plot.
///
/// A series produces fractional-row geometry and colors and paints it through a
/// [`Rasterizer`].
pub(crate) trait Series {
    /// The value span the data occupies, or `None` when there is nothing to
    /// draw. The container autoscales the value axis from this.
    fn value_bounds(&self) -> Option<(f64, f64)>;

    /// Lays out this series' columns into `plot`, choosing which entries are in
    /// view.
    fn time_scale(&self, plot: Rect) -> TimeScale;

    /// Draws the visible data into the plot area, rasterized with the series'
    /// selected glyph family.
    fn draw(&self, buf: &mut Buffer, layout: &PlotLayout);
}

#[cfg(test)]
mod tests {
    use super::{on_border, quantize_span};

    #[test]
    fn quantize_span_rounds_each_end_to_the_nearest_sub_cell() {
        // 0.1 .. 0.6 row at 8 sub-cells per row rounds to eighths 1 .. 5.
        assert_eq!(quantize_span(0.1, 0.6, 8, 8), (1, 5));
    }

    #[test]
    fn quantize_span_keeps_a_doji_at_least_one_sub_cell_tall() {
        // A zero-height body still spans one sub-cell.
        assert_eq!(quantize_span(0.5, 0.5, 4, 8), (2, 3));
    }

    #[test]
    fn quantize_span_clamps_into_the_plot_and_pulls_the_start_back() {
        // The end rounds past `max`, so it clamps and the start follows it in to
        // keep the span one sub-cell long inside the plot.
        assert_eq!(quantize_span(1.0, 1.2, 4, 4), (3, 4));
    }

    #[test]
    fn on_border_is_true_on_each_edge_and_false_inside() {
        // A 3x3 sub-cell rectangle [0, 3) x [0, 3): the center is the only
        // interior sub-cell.
        assert!(on_border(0, 1, 0, 3, 0, 3));
        assert!(on_border(2, 1, 0, 3, 0, 3));
        assert!(on_border(1, 0, 0, 3, 0, 3));
        assert!(on_border(1, 2, 0, 3, 0, 3));
        assert!(!on_border(1, 1, 0, 3, 0, 3));
    }
}
