//! Shared wick rendering.
//!
//! A wick is drawn as a vertical run of line glyphs along the candle's center column,
//! from the body out to the high and low.
//!
//! Used by the block and quadrant backends.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Style;

use crate::render::{self, CandleGeometry};

/// Vertical steps per terminal row a wick tip resolves to (`│`, `╵`, `╷`).
pub(crate) const HALVES_PER_ROW: u32 = 2;

/// Draws `geometry`'s wicks, reaching from the body's top and bottom cells
/// (`row_top`, `row_bot`) out to its high and low.
///
/// The high and low resolve to half a row, so a tip that lands mid-cell uses a
/// half-height glyph rather than snapping to a whole row. The wick is painted in
/// the candle's wick color over its background.
///
/// Returns `(upper, lower)`: whether each wick actually extended past the body's
/// top and bottom cells. Backends that fuse the wick into the body can use this
/// result.
pub(crate) fn draw(
    buf: &mut Buffer,
    plot: Rect,
    geometry: &CandleGeometry,
    row_top: u32,
    row_bot: u32,
) -> (bool, bool) {
    let CandleGeometry {
        high_row,
        low_row,
        wick: fg,
        bg,
        ..
    } = *geometry;

    let style = Style::default().fg(fg).bg(bg);

    // The wick runs along the whole column nearest the body's center,
    // plot-relative.
    let center_col = (geometry.center() - 0.5).round() as u32;

    let last_half = u32::from(plot.height) * HALVES_PER_ROW - 1;
    let high_half = (high_row * HALVES_PER_ROW as f64).round() as u32;
    let low_half = ((low_row * HALVES_PER_ROW as f64).round() as u32).min(last_half);

    let upper = draw_upper_wick(buf, plot, center_col, high_half, row_top, style);
    let lower = draw_lower_wick(buf, plot, center_col, low_half, row_bot, style);
    (upper, lower)
}

/// Draws the wick above the body, from the body's top cell up to `high_half`
/// (a half-row position from the plot top). The tip cell is the lower-half glyph
/// `╷` when the high lands on a half-row boundary, otherwise a full `│`.
///
/// Returns whether the wick reached past the body's top cell at all.
fn draw_upper_wick(
    buf: &mut Buffer,
    plot: Rect,
    center_col: u32,
    high_half: u32,
    row_top: u32,
    style: Style,
) -> bool {
    let body_edge = row_top * HALVES_PER_ROW;
    if high_half >= body_edge {
        return false;
    }

    let tip = high_half / HALVES_PER_ROW;

    if !high_half.is_multiple_of(HALVES_PER_ROW) {
        // The high reaches only the lower half of the tip cell.
        render::put(buf, plot, center_col, tip, "╷", style);
        for r in (tip + 1)..row_top {
            render::put(buf, plot, center_col, r, "│", style);
        }
    } else {
        for r in tip..row_top {
            render::put(buf, plot, center_col, r, "│", style);
        }
    }

    true
}

/// Draws the wick below the body, from the body's bottom cell down to
/// `low_half`. The tip cell is the upper-half glyph `╵` when the low lands on a
/// half-row boundary, otherwise a full `│`.
///
/// Returns whether the wick reached past the body's bottom cell at all.
fn draw_lower_wick(
    buf: &mut Buffer,
    plot: Rect,
    center_col: u32,
    low_half: u32,
    row_bot: u32,
    style: Style,
) -> bool {
    let body_edge = (row_bot + 1) * HALVES_PER_ROW;
    if low_half < body_edge {
        return false;
    }

    let tip = low_half / HALVES_PER_ROW;

    for r in (row_bot + 1)..tip {
        render::put(buf, plot, center_col, r, "│", style);
    }

    if low_half.is_multiple_of(HALVES_PER_ROW) {
        // The low reaches only the upper half of the tip cell.
        render::put(buf, plot, center_col, tip, "╵", style);
    } else {
        render::put(buf, plot, center_col, tip, "│", style);
    }

    true
}
