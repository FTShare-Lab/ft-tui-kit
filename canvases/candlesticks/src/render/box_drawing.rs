//! Box-drawing rasterizer.
//!
//! Box-drawing line glyphs (`─`, `│`, `┌`, ...) place their line through the
//! center of a cell, so a candle outline's edges land at cell-center positions:
//! one position per cell, offset half a row from the cell grid. This is coarser
//! than the other backends, but allows a closed outline rendering. If the body
//! is wide enough, the wick can also be fused into the body's edges with tee
//! glyphs.
//!
//! Solid bodies, and hollow bodies wide enough but too short to enclose a closed
//! outline, are filled with quadrant blocks inset to the same cell-center bounds
//! the outline traces, so a filled and a hollow body of the same geometry occupy
//! exactly the same space.
//!
//! A body so short it collapses to a single row (a doji) cannot form an outline
//! and is drawn as a flat horizontal line, fused into a tee or a cross where its
//! wicks meet it, regardless of fill.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Style;

use crate::render::quadrant::{self, SubRect};
use crate::render::{self, BodyFill, CandleGeometry, Rasterizer, wick};

/// Quadrant sub-cells per cell along each axis.
const SUB: u32 = 2;

/// A body shorter than this many rows renders as a flat doji line instead of a
/// quadrant block. It is the midpoint between zero height and the half row a
/// single quadrant sub-cell spans, so a short body quantizes to whichever is
/// nearer its true height.
const FLAT_MAX_HEIGHT: f64 = 0.25;

/// Box-drawing rasterizer backend.
///
/// Draws closeable hollow bodies as box-drawing outlines and fills solid bodies
/// (and outlines too small to close) with quadrant blocks aligned to the same
/// footprint.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BoxDrawing;

impl Rasterizer for BoxDrawing {
    fn draw_candle(&self, buf: &mut Buffer, plot: Rect, geometry: &CandleGeometry) {
        draw_candle(buf, plot, geometry);
    }
}

/// A candle body reduced to the whole cells it occupies within the plot.
///
/// Box-drawing edges sit at cell centers, so the body is quantized to whole
/// cells: the outline runs along the centers of these boundary cells, inclusive
/// of both ends. Indices are plot-relative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Footprint {
    row_top: u32,
    row_bot: u32,
    col_left: u32,
    col_right: u32,
}

impl Footprint {
    /// Whether the footprint has distinct top/bottom and left/right edge cells,
    /// so a rectangle outline can be closed. A body only one cell wide or one
    /// cell tall cannot, and is filled solid instead.
    fn closeable(&self) -> bool {
        self.row_bot > self.row_top && self.col_right > self.col_left
    }
}

/// Reduces a candle's fractional geometry to the whole cells its body occupies.
///
/// Each edge rounds to the nearest cell, like the other backends, then clamps
/// into the plot. The right edge rounds to an exclusive cell boundary, so the
/// last occupied column is one less.
fn footprint(plot: Rect, geometry: &CandleGeometry) -> Footprint {
    let last_row = u32::from(plot.height).saturating_sub(1);
    let last_col = u32::from(plot.width).saturating_sub(1);

    let row_top = (geometry.body_top_row.max(0.0).round() as u32).min(last_row);
    let row_bot = (geometry.body_bottom_row.max(0.0).round() as u32).min(last_row);

    let col_left = (geometry.body_left.max(0.0).round() as u32).min(last_col);
    let col_right = (geometry.body_right.max(0.0).round() as u32)
        .saturating_sub(1)
        .min(last_col);

    Footprint {
        row_top,
        row_bot,
        col_left,
        col_right,
    }
}

/// Draws one candle into `plot`.
pub(crate) fn draw_candle(buf: &mut Buffer, plot: Rect, geometry: &CandleGeometry) {
    if plot.width == 0 || plot.height == 0 {
        return;
    }

    let footprint = footprint(plot, geometry);

    let height = geometry.body_bottom_row - geometry.body_top_row;
    if height < FLAT_MAX_HEIGHT {
        draw_flat(buf, plot, geometry, footprint);
    } else if geometry.fill == BodyFill::Hollow && footprint.closeable() {
        draw_hollow(buf, plot, geometry, footprint);
    } else {
        fill_solid(buf, plot, geometry, footprint);
    }
}

/// Draws a closeable hollow body: its wick, its rectangle outline, and the tee
/// glyphs that fuse the two together where they meet.
fn draw_hollow(buf: &mut Buffer, plot: Rect, geometry: &CandleGeometry, footprint: Footprint) {
    let Footprint {
        row_top,
        row_bot,
        col_left,
        col_right,
    } = footprint;

    // The wick only paints cells outside the body's rows, so it never collides
    // with the outline drawn next.
    let (up, down) = wick::draw(buf, plot, geometry, row_top, row_bot);
    draw_outline(buf, plot, geometry, footprint);

    // Fuse the wick into the body's edges with tee glyphs, but only when the
    // body's center column lands on a horizontal edge strictly between the
    // corners.
    let center_col = geometry.center().floor() as u32;
    if center_col <= col_left || center_col >= col_right {
        return;
    }

    let style = Style::default().fg(geometry.body).bg(geometry.bg);
    if up {
        render::put(buf, plot, center_col, row_top, "┴", style);
    }
    if down {
        render::put(buf, plot, center_col, row_bot, "┬", style);
    }
}

/// Draws a body too short to enclose an outline as a flat horizontal line, the
/// box-drawing form of a doji. The line spans the body's columns, and where the
/// wicks meet it at the center column they fuse it into a tee or a cross.
fn draw_flat(buf: &mut Buffer, plot: Rect, geometry: &CandleGeometry, footprint: Footprint) {
    let Footprint {
        col_left,
        col_right,
        ..
    } = footprint;
    let style = Style::default().fg(geometry.body).bg(geometry.bg);

    // Place the line at the cell whose center is nearest the body's midpoint.
    let mid = (geometry.body_top_row + geometry.body_bottom_row) / 2.0;
    let last_row = u32::from(plot.height).saturating_sub(1);
    let row = (mid.floor() as u32).min(last_row);

    // The wick reaches above and below the single body row.
    let (up, down) = wick::draw(buf, plot, geometry, row, row);

    for col in col_left..=col_right {
        render::put(buf, plot, col, row, "─", style);
    }

    // Fuse the meeting wicks into the body line at its center column: a cross
    // when both reach, a tee for one, the plain line for neither.
    let symbol = match (up, down) {
        (true, true) => "┼",
        (true, false) => "┴",
        (false, true) => "┬",
        (false, false) => "─",
    };
    let center_col = geometry.center().floor() as u32;
    render::put(buf, plot, center_col, row, symbol, style);
}

/// Fills `footprint` solid with quadrant blocks, inset to the same cell-center
/// bounds the outline traces, so a filled body covers exactly the region a
/// hollow body of the same geometry would. Draws the wick along the way.
fn fill_solid(buf: &mut Buffer, plot: Rect, geometry: &CandleGeometry, footprint: Footprint) {
    let max_sub_y = u32::from(plot.height) * SUB;
    let max_sub_x = u32::from(plot.width) * SUB;

    // Inset by one sub-cell (half a cell) on every side so the solid edge lands
    // where the outline's lines would, at the boundary cells' centers. Guard a
    // minimum of one sub-cell so a doji still shows a body.
    let top = footprint.row_top * SUB + 1;
    let mut bot = footprint.row_bot * SUB + 1;
    if bot <= top {
        bot = top + 1;
    }
    let bot = bot.min(max_sub_y);
    let top = top.min(bot - 1);

    let left = footprint.col_left * SUB + 1;
    let mut right = footprint.col_right * SUB + 1;
    if right <= left {
        right = left + 1;
    }
    let right = right.min(max_sub_x);

    // The wick runs from the body's top and bottom cells out to the high and low.
    wick::draw(buf, plot, geometry, top / SUB, (bot - 1) / SUB);

    let rect = SubRect {
        left,
        right,
        top,
        bot,
    };
    quadrant::fill_subcells(buf, plot, rect, false, geometry.body, geometry.bg);
}

/// Traces the rectangle outline of `footprint` in `geometry`'s body color over
/// its background, leaving the interior untouched.
fn draw_outline(buf: &mut Buffer, plot: Rect, geometry: &CandleGeometry, footprint: Footprint) {
    let Footprint {
        row_top,
        row_bot,
        col_left,
        col_right,
    } = footprint;
    let style = Style::default().fg(geometry.body).bg(geometry.bg);

    render::put(buf, plot, col_left, row_top, "┌", style);
    render::put(buf, plot, col_right, row_top, "┐", style);
    render::put(buf, plot, col_left, row_bot, "└", style);
    render::put(buf, plot, col_right, row_bot, "┘", style);

    for col in (col_left + 1)..col_right {
        render::put(buf, plot, col, row_top, "─", style);
        render::put(buf, plot, col, row_bot, "─", style);
    }
    for row in (row_top + 1)..row_bot {
        render::put(buf, plot, col_left, row, "│", style);
        render::put(buf, plot, col_right, row, "│", style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::style::Color;

    const BODY: Color = Color::Rgb(0, 200, 120);
    const WICK: Color = Color::Rgb(110, 116, 130);
    const BG: Color = Color::Rgb(10, 10, 12);

    fn buffer(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    /// A candle spanning columns `[x0, x1)` and the given rows, with the chosen
    /// fill. The wick reaches to the body edges, so it adds nothing.
    fn candle(x0: f64, x1: f64, top: f64, bottom: f64, fill: BodyFill) -> CandleGeometry {
        CandleGeometry {
            body_left: x0,
            body_right: x1,
            body_top_row: top,
            body_bottom_row: bottom,
            high_row: top,
            low_row: bottom,
            body: BODY,
            wick: WICK,
            bg: BG,
            fill,
        }
    }

    fn hollow(x0: f64, x1: f64, top: f64, bottom: f64) -> CandleGeometry {
        candle(x0, x1, top, bottom, BodyFill::Hollow)
    }

    fn filled(x0: f64, x1: f64, top: f64, bottom: f64) -> CandleGeometry {
        candle(x0, x1, top, bottom, BodyFill::Filled)
    }

    fn grid(buf: &Buffer, w: u16, h: u16) -> Vec<String> {
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    /// The bounding box (inclusive) of the non-empty cells, or `None` if empty.
    fn bounds(buf: &Buffer, w: u16, h: u16) -> Option<(u16, u16, u16, u16)> {
        let mut b: Option<(u16, u16, u16, u16)> = None;
        for y in 0..h {
            for x in 0..w {
                if buf[(x, y)].symbol() != " " {
                    b = Some(match b {
                        None => (x, x, y, y),
                        Some((x0, x1, y0, y1)) => (x0.min(x), x1.max(x), y0.min(y), y1.max(y)),
                    });
                }
            }
        }
        b
    }

    #[test]
    fn footprint_reduces_a_body_to_inclusive_cell_indices() {
        let plot = Rect::new(0, 0, 3, 3);
        let fp = footprint(plot, &hollow(0.0, 3.0, 0.0, 2.0));
        assert_eq!(
            fp,
            Footprint {
                row_top: 0,
                row_bot: 2,
                col_left: 0,
                col_right: 2,
            }
        );
        assert!(fp.closeable());
    }

    #[test]
    fn footprint_is_not_closeable_when_one_cell_wide() {
        let plot = Rect::new(0, 0, 3, 3);
        let fp = footprint(plot, &hollow(0.0, 1.0, 0.0, 2.0));
        assert_eq!(fp.col_left, fp.col_right);
        assert!(!fp.closeable());
    }

    #[test]
    fn footprint_is_not_closeable_when_one_cell_tall() {
        let plot = Rect::new(0, 0, 3, 3);
        // Both edges round to row 0, so there is no distinct bottom edge.
        let fp = footprint(plot, &hollow(0.0, 3.0, 0.0, 0.4));
        assert_eq!(fp.row_top, fp.row_bot);
        assert!(!fp.closeable());
    }

    #[test]
    fn three_by_three_hollow_traces_a_full_outline() {
        let plot = Rect::new(0, 0, 3, 3);
        let mut buf = buffer(3, 3);

        draw_candle(&mut buf, plot, &hollow(0.0, 3.0, 0.0, 2.0));

        assert_eq!(grid(&buf, 3, 3), ["┌─┐", "│ │", "└─┘"]);
        assert_eq!(buf[(0, 0)].fg, BODY);
        assert_eq!(buf[(0, 0)].bg, BG);
        // The interior is left untouched.
        assert_eq!(buf[(1, 1)].symbol(), " ");
    }

    #[test]
    fn wide_hollow_fuses_the_wick_into_its_edges_with_tees() {
        let plot = Rect::new(0, 0, 3, 6);
        let mut buf = buffer(3, 6);

        // A three-wide body in rows 2..4, with the high above and the low below.
        let geometry = CandleGeometry {
            high_row: 0.0,
            low_row: 6.0,
            ..hollow(0.0, 3.0, 2.0, 4.0)
        };
        draw_candle(&mut buf, plot, &geometry);

        // The wick runs down the center column and fuses into the top and bottom
        // edges, which become tees instead of plain horizontal runs.
        assert_eq!(buf[(1, 0)].symbol(), "│");
        assert_eq!(buf[(1, 0)].fg, WICK);
        assert_eq!(buf[(1, 2)].symbol(), "┴", "top edge fuses the upper wick");
        assert_eq!(buf[(1, 2)].fg, BODY);
        assert_eq!(
            buf[(1, 4)].symbol(),
            "┬",
            "bottom edge fuses the lower wick"
        );
        assert_eq!(buf[(1, 5)].symbol(), "│");

        // The corners are untouched by the fusion.
        assert_eq!(buf[(0, 2)].symbol(), "┌");
        assert_eq!(buf[(2, 2)].symbol(), "┐");
    }

    #[test]
    fn narrow_hollow_leaves_corners_intact_and_the_wick_floats() {
        let plot = Rect::new(0, 0, 2, 4);
        let mut buf = buffer(2, 4);

        // A two-wide body: the center column lands on a corner, so no tee is
        // drawn and the wick floats above the ring.
        let geometry = CandleGeometry {
            high_row: 0.0,
            ..hollow(0.0, 2.0, 1.0, 2.0)
        };
        draw_candle(&mut buf, plot, &geometry);

        assert_eq!(buf[(0, 1)].symbol(), "┌");
        assert_eq!(buf[(1, 1)].symbol(), "┐", "corner is not replaced by a tee");
        assert_eq!(buf[(1, 0)].symbol(), "│", "wick floats above the corner");
        assert_eq!(buf[(1, 0)].fg, WICK);
    }

    #[test]
    fn hollow_without_wick_reach_keeps_plain_edges() {
        let plot = Rect::new(0, 0, 3, 3);
        let mut buf = buffer(3, 3);

        // The high and low sit at the body edges, so no wick extends past them
        // and the edges stay plain horizontal runs.
        draw_candle(&mut buf, plot, &hollow(0.0, 3.0, 0.0, 2.0));

        assert_eq!(buf[(1, 0)].symbol(), "─");
        assert_eq!(buf[(1, 2)].symbol(), "─");
    }

    #[test]
    fn two_by_two_hollow_is_a_corners_only_ring() {
        let plot = Rect::new(0, 0, 2, 2);
        let mut buf = buffer(2, 2);

        draw_candle(&mut buf, plot, &hollow(0.0, 2.0, 0.0, 1.0));

        assert_eq!(grid(&buf, 2, 2), ["┌┐", "└┘"]);
    }

    #[test]
    fn three_by_three_filled_fills_the_outline_bounds_with_quadrant_blocks() {
        let plot = Rect::new(0, 0, 3, 3);
        let mut buf = buffer(3, 3);

        draw_candle(&mut buf, plot, &filled(0.0, 3.0, 0.0, 2.0));

        // The solid fill is inset to the outline's cell-center bounds, so the
        // boundary cells are quadrant blocks and only the interior is full.
        assert_eq!(grid(&buf, 3, 3), ["▗▄▖", "▐█▌", "▝▀▘"]);
        assert_eq!(buf[(1, 1)].fg, BODY);
    }

    #[test]
    fn filled_and_hollow_occupy_the_same_space() {
        let plot = Rect::new(0, 0, 5, 5);

        let mut filled_buf = buffer(5, 5);
        draw_candle(&mut filled_buf, plot, &filled(1.0, 4.0, 1.0, 3.0));

        let mut hollow_buf = buffer(5, 5);
        draw_candle(&mut hollow_buf, plot, &hollow(1.0, 4.0, 1.0, 3.0));

        assert_eq!(bounds(&filled_buf, 5, 5), bounds(&hollow_buf, 5, 5));
        assert_eq!(bounds(&filled_buf, 5, 5), Some((1, 3, 1, 3)));
    }

    #[test]
    fn one_cell_wide_hollow_fills_the_same_space_as_filled() {
        let plot = Rect::new(0, 0, 1, 3);

        let mut hollow_buf = buffer(1, 3);
        draw_candle(&mut hollow_buf, plot, &hollow(0.0, 1.0, 0.0, 3.0));

        let mut filled_buf = buffer(1, 3);
        draw_candle(&mut filled_buf, plot, &filled(0.0, 1.0, 0.0, 3.0));

        // A one-cell-wide hollow body cannot form an outline, so it fills solid
        // exactly like a filled body of the same geometry.
        assert_eq!(hollow_buf, filled_buf);
    }

    #[test]
    fn doji_with_both_wicks_is_a_cross() {
        let plot = Rect::new(0, 0, 3, 5);
        let mut buf = buffer(3, 5);

        // A zero-height body in row 2 with the high above and the low below.
        let geometry = CandleGeometry {
            high_row: 0.0,
            low_row: 5.0,
            ..hollow(0.0, 3.0, 2.0, 2.0)
        };
        draw_candle(&mut buf, plot, &geometry);

        // The body is a flat line, crossed by the wick at its center.
        assert_eq!(grid(&buf, 3, 5), [" │ ", " │ ", "─┼─", " │ ", " │ "]);
        assert_eq!(buf[(1, 2)].symbol(), "┼");
        assert_eq!(buf[(1, 2)].fg, BODY);
        assert_eq!(buf[(1, 0)].fg, WICK);
    }

    #[test]
    fn short_body_with_one_wick_is_a_tee() {
        let plot = Rect::new(0, 0, 3, 4);
        let mut buf = buffer(3, 4);

        // A single-row body with only the high reaching above it.
        let geometry = CandleGeometry {
            high_row: 0.0,
            low_row: 2.0,
            ..hollow(0.0, 3.0, 2.0, 2.0)
        };
        draw_candle(&mut buf, plot, &geometry);

        assert_eq!(
            buf[(1, 2)].symbol(),
            "┴",
            "the line fuses the upper wick only"
        );
        assert_eq!(buf[(0, 2)].symbol(), "─");
        assert_eq!(buf[(2, 2)].symbol(), "─");
    }

    #[test]
    fn flat_body_without_wicks_is_a_plain_line() {
        let plot = Rect::new(0, 0, 3, 3);
        let mut buf = buffer(3, 3);

        // Open, high, low, and close all coincide: a bare horizontal line.
        draw_candle(&mut buf, plot, &hollow(0.0, 3.0, 1.0, 1.0));

        assert_eq!(grid(&buf, 3, 3), ["   ", "───", "   "]);
        assert_eq!(buf[(1, 1)].fg, BODY);
    }

    #[test]
    fn near_flat_body_lands_on_the_nearest_cell_center() {
        let plot = Rect::new(0, 0, 3, 4);
        let mut buf = buffer(3, 4);

        // A 0.2-row body straddling the row 1/2 boundary: its midpoint (1.75) is
        // nearest row 1's center, so the line lands there, not at round(1.65) = 2.
        let geometry = CandleGeometry {
            high_row: 0.0,
            low_row: 4.0,
            ..hollow(0.0, 3.0, 1.65, 1.85)
        };
        draw_candle(&mut buf, plot, &geometry);

        assert_eq!(grid(&buf, 3, 4), [" │ ", "─┼─", " │ ", " │ "]);
    }

    #[test]
    fn body_above_the_flat_threshold_quantizes_to_quadrant_blocks() {
        let plot = Rect::new(0, 0, 3, 4);
        let mut buf = buffer(3, 4);

        // A 0.3-row body is nearer the half row a quadrant sub-cell spans than to
        // flat, so it renders as quadrant blocks rather than a doji line.
        draw_candle(&mut buf, plot, &hollow(0.0, 3.0, 1.1, 1.4));

        assert_eq!(grid(&buf, 3, 4), ["   ", "▗▄▖", "   ", "   "]);
    }

    #[test]
    fn filled_and_hollow_dojis_are_identical() {
        let plot = Rect::new(0, 0, 3, 5);
        let geometry = |fill| CandleGeometry {
            high_row: 0.0,
            low_row: 5.0,
            ..candle(0.0, 3.0, 2.0, 2.0, fill)
        };

        let mut filled_buf = buffer(3, 5);
        draw_candle(&mut filled_buf, plot, &geometry(BodyFill::Filled));

        let mut hollow_buf = buffer(3, 5);
        draw_candle(&mut hollow_buf, plot, &geometry(BodyFill::Hollow));

        // A single row has no interior, so fill makes no difference.
        assert_eq!(filled_buf, hollow_buf);
    }

    #[test]
    fn filled_body_draws_a_wick_above_it() {
        let plot = Rect::new(0, 0, 1, 4);
        let mut buf = buffer(1, 4);

        // Body in the lower rows with the high reaching the top of the plot.
        let geometry = CandleGeometry {
            high_row: 0.0,
            ..filled(0.0, 1.0, 2.0, 3.0)
        };
        draw_candle(&mut buf, plot, &geometry);

        assert_eq!(buf[(0, 0)].symbol(), "│");
        assert_eq!(buf[(0, 0)].fg, WICK);
    }
}
