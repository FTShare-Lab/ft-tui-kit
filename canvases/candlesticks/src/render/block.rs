//! Eighth-block rasterizer.
//!
//! A candle arrives as fractional row positions (from the price scale) plus the
//! colors to paint it in. This module owns everything specific to the block
//! character set: quantizing a row to one of eight vertical steps and the
//! foreground/background inversion that lets a body edge land between two rows.
//! Wicks are drawn by the shared [`wick`](crate::render::wick) module.
//!
//! A body confined to a single row that touches neither the top nor the bottom
//! of that cell is drawn flush to the cell bottom. Block glyphs fill from the
//! bottom or from the top by inversion, but cannot float a segment in the middle
//! of a cell. A body is never shorter than one eighth, so it always remains
//! visible. Body endpoints resolve to an eighth of a row.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};

use crate::render::{self, BarGeometry, BodyFill, CandleGeometry, Rasterizer, wick};

/// Eighth-block rasterizer backend.
///
/// Quantizes a candle's fractional rows to eighths (bodies) and halves (wick
/// tips) and paints them with the block and vertical-line glyphs.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Block;

impl Rasterizer for Block {
    fn draw_candle(&self, buf: &mut Buffer, plot: Rect, geometry: &CandleGeometry) {
        draw_candle(buf, plot, geometry);
    }
}

/// Vertical sub-cell steps per terminal row (one eighth-block each).
const EIGHTHS_PER_ROW: u32 = 8;

/// Eighth-block fills indexed by how many eighths are lit from the bottom of the
/// cell (`0` is empty, `8` is full).
const EIGHTHS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

/// Draws one candle into `plot`.
pub(crate) fn draw_candle(buf: &mut Buffer, plot: Rect, geometry: &CandleGeometry) {
    if plot.width == 0 || plot.height == 0 {
        return;
    }

    let CandleGeometry {
        body_left,
        body_right,
        body_top_row,
        body_bottom_row,
        body,
        bg,
        fill,
        ..
    } = *geometry;

    let max_sub = u32::from(plot.height) * EIGHTHS_PER_ROW;

    // Body endpoints to the nearest eighth, at least one eighth tall so a doji
    // still shows a body.
    let (top_sub, bot_sub) =
        render::quantize_span(body_top_row, body_bottom_row, EIGHTHS_PER_ROW, max_sub);

    let row_top = top_sub / EIGHTHS_PER_ROW;
    let row_bot = (bot_sub - 1) / EIGHTHS_PER_ROW;

    // Wicks reach from the body out to the high and low, drawn with vertical
    // line glyphs at half-row tip resolution.
    wick::draw(buf, plot, geometry, row_top, row_bot);

    // Body edges to the nearest whole column, at least one column wide,
    // plot-relative.
    let left_col = body_left.round() as u32;
    let mut right_col = body_right.round() as u32;
    if right_col <= left_col {
        right_col = left_col + 1;
    }
    let col_end = right_col.min(u32::from(plot.width));

    for row in row_top..=row_bot {
        let cell_top = row * EIGHTHS_PER_ROW;
        let a = top_sub.max(cell_top) - cell_top;
        let b = bot_sub.min(cell_top + EIGHTHS_PER_ROW) - cell_top;

        if b <= a {
            continue;
        }

        let (symbol, style) = body_segment(a as u16, b as u16, body, bg);
        for col in left_col..col_end {
            render::put(buf, plot, col, row, symbol, style);
        }
    }

    // A hollow body is the filled body with its interior cleared, leaving a
    // one-cell-thick border drawn in the same eighth-block glyphs. The body must
    // be at least three columns wide so an interior column exists between the
    // side walls; narrower bodies (and those too short to leave an interior row)
    // have nothing to clear and stay solid.
    if fill == BodyFill::Hollow && col_end.saturating_sub(left_col) >= 3 {
        clear_body_interior(buf, plot, left_col, col_end, row_top, row_bot, bg);
    }
}

/// Draws one bottom-anchored bar into `plot`.
pub(crate) fn draw_bar(buf: &mut Buffer, plot: Rect, geometry: &BarGeometry) {
    if plot.width == 0 || plot.height == 0 {
        return;
    }

    let BarGeometry {
        left,
        right,
        value_row,
        color,
        bg,
    } = *geometry;

    let max_sub = u32::from(plot.height) * EIGHTHS_PER_ROW;

    // The top of the bar resolves to the nearest eighth. At least one eighth
    // tall so a small bar stays visible.
    let (top_sub, bot_sub) =
        render::quantize_span(value_row, f64::from(plot.height), EIGHTHS_PER_ROW, max_sub);

    let row_top = top_sub / EIGHTHS_PER_ROW;
    let row_bot = (bot_sub - 1) / EIGHTHS_PER_ROW;

    // Bar edges to the nearest whole column, at least one column wide.
    let left_col = left.round() as u32;
    let mut right_col = right.round() as u32;
    if right_col <= left_col {
        right_col = left_col + 1;
    }
    let col_end = right_col.min(u32::from(plot.width));

    for row in row_top..=row_bot {
        let cell_top = row * EIGHTHS_PER_ROW;
        let a = top_sub.max(cell_top) - cell_top;
        let b = bot_sub.min(cell_top + EIGHTHS_PER_ROW) - cell_top;

        if b <= a {
            continue;
        }

        let (symbol, style) = body_segment(a as u16, b as u16, color, bg);
        for col in left_col..col_end {
            render::put(buf, plot, col, row, symbol, style);
        }
    }
}

/// Clears the cells strictly inside the body border (columns `[x_start, x_end)`,
/// rows `row_top..=row_bot`) to `bg`, hollowing a filled body. Coordinates are
/// plot-relative.
fn clear_body_interior(
    buf: &mut Buffer,
    plot: Rect,
    x_start: u32,
    x_end: u32,
    row_top: u32,
    row_bot: u32,
    bg: Color,
) {
    // Ensure that the REVERSED modifier is unset.
    let style = Style::default()
        .fg(bg)
        .bg(bg)
        .remove_modifier(Modifier::REVERSED);
    for row in (row_top + 1)..row_bot {
        for col in (x_start + 1)..(x_end - 1) {
            render::put(buf, plot, col, row, " ", style);
        }
    }
}

/// The glyph and style for a body segment lit over `[a, b)` (eighths from the
/// top of the cell), filled in `fill` over `empty`.
///
/// A segment flush with the cell top sets the `REVERSED` attribute so the lit
/// eighths sit at the top: the body goes in the cell background and the empty
/// eighths in the foreground glyph, and the terminal swaps the two when it
/// draws. Doing the swap at display time (rather than swapping the colors here)
/// lets `empty` be [`Color::Reset`], so a body renders correctly without a
/// concrete chart background. Other segments are lit directly from the bottom.
fn body_segment(a: u16, b: u16, fill: Color, empty: Color) -> (&'static str, Style) {
    let eighths = EIGHTHS_PER_ROW as u16;

    let (symbol, reversed) = if b - a == eighths {
        // A full cell: a solid block.
        ("█", false)
    } else if a == 0 {
        // Lit from the top: the lower `8 - b` eighths form the glyph, reversed so
        // the body fills the top `b` eighths through the cell background.
        (EIGHTHS[(eighths - b) as usize], true)
    } else {
        // Lit from the bottom (or a small floating segment).
        (EIGHTHS[(b - a) as usize], false)
    };

    let style = Style::default().fg(fill).bg(empty);
    let style = if reversed {
        style.add_modifier(Modifier::REVERSED)
    } else {
        style.remove_modifier(Modifier::REVERSED)
    };

    (symbol, style)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: Color = Color::Rgb(0, 200, 120);
    const WICK: Color = Color::Rgb(110, 116, 130);
    const BG: Color = Color::Rgb(10, 10, 12);

    const PARTIAL: [&str; 7] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇"];

    fn buffer(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    fn is_partial(symbol: &str) -> bool {
        PARTIAL.contains(&symbol)
    }

    /// A single-column candle at column 0, with the given rows and wick color.
    fn geometry(top: f64, bottom: f64, high: f64, low: f64, wick: Color) -> CandleGeometry {
        CandleGeometry {
            body_left: 0.0,
            body_right: 1.0,
            body_top_row: top,
            body_bottom_row: bottom,
            high_row: high,
            low_row: low,
            body: BODY,
            wick,
            bg: BG,
            fill: BodyFill::Filled,
        }
    }

    #[test]
    fn full_cell_is_a_solid_block() {
        let plot = Rect::new(0, 0, 1, 1);
        let mut buf = buffer(1, 1);

        // A body spanning the whole single row, no wicks.
        draw_candle(&mut buf, plot, &geometry(0.0, 1.0, 0.0, 1.0, WICK));

        let cell = &buf[(0, 0)];
        assert_eq!(cell.symbol(), "█");
        assert_eq!(cell.fg, BODY);
    }

    #[test]
    fn flush_top_segment_uses_reverse_to_fill_the_top() {
        let plot = Rect::new(0, 0, 1, 1);
        let mut buf = buffer(1, 1);

        // Top half of the cell only (rows 0.0 .. 0.5 => eighths 0..4).
        draw_candle(&mut buf, plot, &geometry(0.0, 0.5, 0.0, 0.5, WICK));

        let cell = &buf[(0, 0)];
        assert!(is_partial(cell.symbol()), "got {:?}", cell.symbol());
        // The body is the foreground and the empty eighths the background, with
        // REVERSED so the terminal swaps them. This keeps `empty` (which may be
        // Color::Reset) out of the foreground, so it renders without a chart bg.
        assert_eq!(cell.fg, BODY);
        assert_eq!(cell.bg, BG);
        assert!(cell.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn flush_top_segment_is_transparent_over_a_reset_background() {
        let plot = Rect::new(0, 0, 1, 1);
        let mut buf = buffer(1, 1);

        // Top half of the cell, with a Reset (terminal default) empty color.
        let geometry = CandleGeometry {
            body_left: 0.0,
            body_right: 1.0,
            body_top_row: 0.0,
            body_bottom_row: 0.5,
            high_row: 0.0,
            low_row: 0.5,
            body: BODY,
            wick: WICK,
            bg: Color::Reset,
            fill: BodyFill::Filled,
        };
        draw_candle(&mut buf, plot, &geometry);

        // The body stays in the foreground and Reset in the background, with
        // REVERSED. The terminal resolves Reset to its real background and swaps,
        // so the top shows the body and the bottom the transparent background.
        let cell = &buf[(0, 0)];
        assert!(is_partial(cell.symbol()), "got {:?}", cell.symbol());
        assert_eq!(cell.fg, BODY);
        assert_eq!(cell.bg, Color::Reset);
        assert!(cell.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn bottom_anchored_segment_lights_from_the_bottom() {
        let plot = Rect::new(0, 0, 1, 1);
        let mut buf = buffer(1, 1);

        // Bottom half of the cell (rows 0.5 .. 1.0 => eighths 4..8).
        draw_candle(&mut buf, plot, &geometry(0.5, 1.0, 0.5, 1.0, WICK));

        let cell = &buf[(0, 0)];
        assert!(is_partial(cell.symbol()), "got {:?}", cell.symbol());
        // Lit directly (no reverse): the body is the foreground, the empty half
        // the background.
        assert_eq!(cell.fg, BODY);
        assert_eq!(cell.bg, BG);
        assert!(!cell.modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn wick_floats_above_the_body_without_bleeding_into_its_edge() {
        let plot = Rect::new(0, 0, 1, 4);
        let mut buf = buffer(1, 4);

        // Body top has a partial edge in row 1 (top_sub 12 => a == 4); a high
        // above it puts a wick in row 0 that floats free of the body.
        draw_candle(&mut buf, plot, &geometry(1.5, 3.0, 0.0, 4.0, WICK));

        // Row 0 carries the wick.
        assert_eq!(buf[(0, 0)].symbol(), "│");
        assert_eq!(buf[(0, 0)].fg, WICK);

        // The body's partial edge keeps the background in its empty half. The
        // wick color never leaks into the body cell, so the open/close stays at
        // the body/background boundary.
        let edge = &buf[(0, 1)];
        assert!(is_partial(edge.symbol()), "got {:?}", edge.symbol());
        assert_eq!(edge.fg, BODY);
        assert_eq!(edge.bg, BG);
    }

    #[test]
    fn lower_wick_tip_uses_a_half_glyph_when_the_low_lands_mid_cell() {
        let plot = Rect::new(0, 0, 1, 4);
        let mut buf = buffer(1, 4);

        // Body in row 0; the low sits at row 2.0 (a half-row boundary), so the
        // tip cell (row 2) shows only its upper half.
        draw_candle(&mut buf, plot, &geometry(0.0, 1.0, 0.0, 2.0, WICK));

        assert_eq!(buf[(0, 1)].symbol(), "│", "full wick cell below the body");
        assert_eq!(buf[(0, 2)].symbol(), "╵", "half-row tip at the low");
        assert_eq!(buf[(0, 2)].fg, WICK);
    }

    #[test]
    fn upper_wick_tip_uses_a_half_glyph_when_the_high_lands_mid_cell() {
        let plot = Rect::new(0, 0, 1, 4);
        let mut buf = buffer(1, 4);

        // Body in row 3; the high sits at row 2.5 (a half-row boundary), so the
        // tip cell (row 2) shows only its lower half.
        draw_candle(&mut buf, plot, &geometry(3.0, 4.0, 2.5, 4.0, WICK));

        assert_eq!(buf[(0, 2)].symbol(), "╷", "half-row tip at the high");
        assert_eq!(buf[(0, 2)].fg, WICK);
        assert_eq!(buf[(0, 3)].symbol(), "█", "solid body cell");
    }

    /// A candle at columns `[x0, x1)` spanning the given rows, hollow.
    fn hollow(x0: u16, x1: u16, top: f64, bottom: f64) -> CandleGeometry {
        CandleGeometry {
            body_left: f64::from(x0),
            body_right: f64::from(x1),
            body_top_row: top,
            body_bottom_row: bottom,
            high_row: top,
            low_row: bottom,
            body: BODY,
            wick: WICK,
            bg: BG,
            fill: BodyFill::Hollow,
        }
    }

    #[test]
    fn hollow_body_keeps_an_eighth_block_border_and_clears_the_interior() {
        let plot = Rect::new(0, 0, 3, 3);
        let mut buf = buffer(3, 3);

        // A three-wide, full-height body.
        draw_candle(&mut buf, plot, &hollow(0, 3, 0.0, 3.0));

        // The border is solid eighth blocks, not box-drawing glyphs.
        for x in 0..3 {
            assert_eq!(buf[(x, 0)].symbol(), "█", "top border at {x}");
            assert_eq!(buf[(x, 2)].symbol(), "█", "bottom border at {x}");
        }
        assert_eq!(buf[(0, 1)].symbol(), "█", "left wall");
        assert_eq!(buf[(2, 1)].symbol(), "█", "right wall");
        assert_eq!(buf[(0, 0)].fg, BODY);

        // The single interior cell is cleared to the background.
        assert_eq!(buf[(1, 1)].symbol(), " ");
        assert_eq!(buf[(1, 1)].bg, BG);
    }

    #[test]
    fn hollow_falls_back_to_a_solid_body_below_three_columns() {
        let plot = Rect::new(0, 0, 2, 3);
        let mut buf = buffer(2, 3);

        // Two columns are both side walls with no interior between them, so the
        // body stays solid.
        draw_candle(&mut buf, plot, &hollow(0, 2, 0.0, 3.0));

        for x in 0..2 {
            for y in 0..3 {
                assert_eq!(buf[(x, y)].symbol(), "█", "cell ({x}, {y}) stays solid");
            }
        }
    }

    /// A bar spanning columns `[left, right)` topped at `value_row`.
    fn bar(left: f64, right: f64, value_row: f64) -> BarGeometry {
        BarGeometry {
            left,
            right,
            value_row,
            color: BODY,
            bg: BG,
        }
    }

    #[test]
    fn bar_fills_full_height_with_solid_blocks() {
        let plot = Rect::new(0, 0, 1, 3);
        let mut buf = buffer(1, 3);

        // A bar topped at the plot's top edge fills every row from the floor up.
        draw_bar(&mut buf, plot, &bar(0.0, 1.0, 0.0));

        for y in 0..3 {
            assert_eq!(buf[(0, y)].symbol(), "█", "row {y}");
            assert_eq!(buf[(0, y)].fg, BODY);
        }
    }

    #[test]
    fn bar_top_cell_is_lit_from_the_bottom_without_reverse() {
        let plot = Rect::new(0, 0, 1, 3);
        let mut buf = buffer(1, 3);

        // A bar topped mid-cell (row 1.5) leaves row 0 empty, a partial top in
        // row 1, and a solid floor row 2.
        draw_bar(&mut buf, plot, &bar(0.0, 1.0, 1.5));

        let top = &buf[(0, 1)];
        assert!(is_partial(top.symbol()), "got {:?}", top.symbol());
        assert_eq!(top.fg, BODY);
        // Lit from the bottom of the cell, so no REVERSED inversion is needed.
        assert!(!top.modifier.contains(Modifier::REVERSED));

        assert_eq!(buf[(0, 2)].symbol(), "█", "solid floor cell");
        assert_eq!(buf[(0, 0)].symbol(), " ", "empty above the bar");
    }

    #[test]
    fn tiny_bar_shows_a_sliver_at_the_bottom() {
        let plot = Rect::new(0, 0, 1, 3);
        let mut buf = buffer(1, 3);

        // A bar barely off the floor still shows at least one eighth.
        draw_bar(&mut buf, plot, &bar(0.0, 1.0, 2.9));

        assert_eq!(buf[(0, 2)].symbol(), "▁");
        assert_eq!(buf[(0, 1)].symbol(), " ");
    }

    #[test]
    fn bar_spans_the_given_columns() {
        let plot = Rect::new(0, 0, 4, 1);
        let mut buf = buffer(4, 1);

        // Columns [1, 3) are filled; the ones outside stay empty.
        draw_bar(&mut buf, plot, &bar(1.0, 3.0, 0.0));

        assert_eq!(buf[(0, 0)].symbol(), " ");
        assert_eq!(buf[(1, 0)].symbol(), "█");
        assert_eq!(buf[(2, 0)].symbol(), "█");
        assert_eq!(buf[(3, 0)].symbol(), " ");
    }
}
