//! Quadrant-block rasterizer.
//!
//! Quadrant glyphs split a cell into a 2x2 grid, doubling both the vertical and
//! horizontal resolution of a whole cell. A candle's body arrives as
//! fractional-row geometry and is quantized to a 2x2 sub-cell grid.
//!
//! Wicks are drawn by the shared [`wick`](crate::render::wick) module.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Color;

use crate::render::line::{self, LineRasterizer};
use crate::render::{self, BodyFill, CandleGeometry, Rasterizer, wick};

/// Quadrant-block rasterizer backend.
///
/// Quantizes a candle's body to a 2x2 sub-cell grid.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Quadrant;

impl Rasterizer for Quadrant {
    fn draw_candle(&self, buf: &mut Buffer, plot: Rect, geometry: &CandleGeometry) {
        draw_candle(buf, plot, geometry);
    }
}

impl LineRasterizer for Quadrant {
    fn draw_polyline(
        &self,
        buf: &mut Buffer,
        plot: Rect,
        points: &[Option<(f64, f64)>],
        color: Color,
        bg: Color,
    ) {
        line::rasterize(
            buf,
            plot,
            points,
            (u32::from(SUB_X), SUB_Y),
            |pattern| QUADRANTS[pattern as usize].chars().next().unwrap_or(' '),
            color,
            bg,
        );
    }
}

/// Sub-cell columns per terminal column.
const SUB_X: u16 = 2;

/// Sub-cell rows per terminal row.
const SUB_Y: u32 = 2;

/// Glyphs indexed by which of the four sub-cells are lit.
const QUADRANTS: [&str; 16] = [
    " ", // ----
    "▘", // TL
    "▝", // TR
    "▀", // TL TR
    "▖", // BL
    "▌", // TL BL
    "▞", // TR BL
    "▛", // TL TR BL
    "▗", // BR
    "▚", // TL BR
    "▐", // TR BR
    "▜", // TL TR BR
    "▄", // BL BR
    "▙", // TL BL BR
    "▟", // TR BL BR
    "█", // TL TR BL BR
];

/// One lit sub-cell, by absolute sub-cell coordinates within the plot.
struct Sub {
    x: u32,
    y: u32,
}

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

    let max_sub_y = u32::from(plot.height) * SUB_Y;
    let max_sub_x = u32::from(plot.width) * u32::from(SUB_X);

    // Body endpoints to the nearest sub-cell row, at least one sub-cell tall so
    // a doji still shows a body.
    let (top_sub, bot_sub) = render::quantize_span(body_top_row, body_bottom_row, SUB_Y, max_sub_y);

    // Body edges to the nearest sub-cell column, at least one sub-cell wide.
    let (left_sub, right_sub) =
        render::quantize_span(body_left, body_right, u32::from(SUB_X), max_sub_x);

    // The wick runs from the body's top and bottom cells out to the high and
    // low, drawn with line glyphs by the shared module.
    let row_top = top_sub / SUB_Y;
    let row_bot = (bot_sub - 1) / SUB_Y;
    wick::draw(buf, plot, geometry, row_top, row_bot);

    // The body fills every sub-cell it spans. A hollow body lights only its
    // border sub-cells; a body too small to have an interior renders solid.
    let rect = SubRect {
        left: left_sub,
        right: right_sub,
        top: top_sub,
        bot: bot_sub,
    };
    fill_subcells(buf, plot, rect, fill == BodyFill::Hollow, body, bg);
}

/// A rectangle of sub-cells `[left, right) x [top, bot)` within a plot, in
/// absolute sub-cell coordinates.
pub(crate) struct SubRect {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bot: u32,
}

/// Fills `rect` with quadrant glyphs in `fg` over `bg`. When `hollow`, only the
/// border sub-cells are lit, tracing a one sub-cell thick ring; a rectangle too
/// small to have an interior is drawn solid either way.
pub(crate) fn fill_subcells(
    buf: &mut Buffer,
    plot: Rect,
    rect: SubRect,
    hollow: bool,
    fg: Color,
    bg: Color,
) {
    let SubRect {
        left,
        right,
        top,
        bot,
    } = rect;

    let mut subs: Vec<Sub> = Vec::new();
    for x in left..right {
        for y in top..bot {
            if !hollow || render::on_border(x, y, left, right, top, bot) {
                subs.push(Sub { x, y });
            }
        }
    }

    accumulate(buf, plot, &subs, fg, bg);
}

/// Folds lit sub-cells into quadrant glyphs, one cell per touched `(col, row)`,
/// and writes them into `buf` in `fg` over `bg`.
fn accumulate(buf: &mut Buffer, plot: Rect, subs: &[Sub], fg: Color, bg: Color) {
    use std::collections::BTreeMap;

    // Per cell: the accumulated sub-cell bit-pattern. Cell coordinates are
    // plot-relative; the plot offset is added on write.
    let mut cells: BTreeMap<(u16, u16), u8> = BTreeMap::new();

    for sub in subs {
        let cell_x = (sub.x / u32::from(SUB_X)) as u16;
        let cell_y = (sub.y / SUB_Y) as u16;
        if cell_x >= plot.width || cell_y >= plot.height {
            continue;
        }

        // Convert the sub-cell coordinates to the bit position in the glyph.
        let bit_x = sub.x % u32::from(SUB_X);
        let bit_y = sub.y % SUB_Y;
        let bit = 1u8 << (bit_y * u32::from(SUB_X) + bit_x);

        *cells.entry((plot.x + cell_x, plot.y + cell_y)).or_insert(0) |= bit;
    }

    for ((x, y), bits) in cells {
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_symbol(QUADRANTS[bits as usize]);
            cell.fg = fg;
            cell.bg = bg;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: Color = Color::Rgb(0, 200, 120);
    const WICK: Color = Color::Rgb(110, 116, 130);
    const BG: Color = Color::Rgb(10, 10, 12);

    fn buffer(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    /// A single-column filled candle at column 0.
    fn geometry(top: f64, bottom: f64, high: f64, low: f64) -> CandleGeometry {
        CandleGeometry {
            body_left: 0.0,
            body_right: 1.0,
            body_top_row: top,
            body_bottom_row: bottom,
            high_row: high,
            low_row: low,
            body: BODY,
            wick: WICK,
            bg: BG,
            fill: BodyFill::Filled,
        }
    }

    #[test]
    fn full_cell_is_a_solid_block() {
        let plot = Rect::new(0, 0, 1, 1);
        let mut buf = buffer(1, 1);

        draw_candle(&mut buf, plot, &geometry(0.0, 1.0, 0.0, 1.0));

        let cell = &buf[(0, 0)];
        assert_eq!(cell.symbol(), "█");
        assert_eq!(cell.fg, BODY);
    }

    #[test]
    fn top_half_is_an_upper_half_block() {
        let plot = Rect::new(0, 0, 1, 1);
        let mut buf = buffer(1, 1);

        // Body covers the top half-row (rows 0.0 .. 0.5 => sub-row 0).
        draw_candle(&mut buf, plot, &geometry(0.0, 0.5, 0.0, 0.5));

        let cell = &buf[(0, 0)];
        assert_eq!(cell.symbol(), "▀");
        assert_eq!(cell.fg, BODY);
        assert_eq!(cell.bg, BG);
    }

    #[test]
    fn bottom_half_is_a_lower_half_block() {
        let plot = Rect::new(0, 0, 1, 1);
        let mut buf = buffer(1, 1);

        // Body covers the bottom half-row (rows 0.5 .. 1.0 => sub-row 1).
        draw_candle(&mut buf, plot, &geometry(0.5, 1.0, 0.5, 1.0));

        let cell = &buf[(0, 0)];
        assert_eq!(cell.symbol(), "▄");
        assert_eq!(cell.fg, BODY);
    }

    #[test]
    fn partial_body_renders_over_a_transparent_background() {
        let plot = Rect::new(0, 0, 1, 1);
        let mut buf = buffer(1, 1);

        // A top-half body over a Reset (terminal default) background: the half
        // block already lights its top half in the foreground, so the body color
        // stays in the foreground and the empty half resolves to the terminal
        // default. No inversion is needed.
        let geometry = CandleGeometry {
            bg: Color::Reset,
            ..geometry(0.0, 0.5, 0.0, 0.5)
        };
        draw_candle(&mut buf, plot, &geometry);

        let cell = &buf[(0, 0)];
        assert_eq!(cell.symbol(), "▀");
        assert_eq!(cell.fg, BODY);
        assert_eq!(cell.bg, Color::Reset);
    }

    #[test]
    fn half_width_body_lights_one_sub_cell_column() {
        let plot = Rect::new(0, 0, 1, 1);
        let mut buf = buffer(1, 1);

        // A body covering the left half of the column and the full row: the two
        // left sub-cells light, giving the left half block.
        let geometry = CandleGeometry {
            body_left: 0.0,
            body_right: 0.5,
            ..geometry(0.0, 1.0, 0.0, 1.0)
        };
        draw_candle(&mut buf, plot, &geometry);

        let cell = &buf[(0, 0)];
        assert_eq!(cell.symbol(), "▌");
        assert_eq!(cell.fg, BODY);
    }

    #[test]
    fn wick_uses_the_block_line_glyphs() {
        let plot = Rect::new(0, 0, 1, 4);
        let mut buf = buffer(1, 4);

        // Body in row 0; the low sits at row 2.0 (a half-row boundary), so the
        // tip cell shows the upper-half glyph, exactly as the block backend.
        draw_candle(&mut buf, plot, &geometry(0.0, 1.0, 0.0, 2.0));

        assert_eq!(buf[(0, 1)].symbol(), "│", "full wick cell below the body");
        assert_eq!(buf[(0, 2)].symbol(), "╵", "half-row tip at the low");
        assert_eq!(buf[(0, 2)].fg, WICK);
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
    fn hollow_body_lights_only_its_border_sub_cells() {
        let plot = Rect::new(0, 0, 3, 3);
        let mut buf = buffer(3, 3);

        // A three-wide, full-height body: the border sub-cells trace a one
        // sub-cell thick ring, leaving the center cell empty.
        draw_candle(&mut buf, plot, &hollow(0, 3, 0.0, 3.0));

        let grid: Vec<String> = (0..3)
            .map(|y| (0..3).map(|x| buf[(x, y)].symbol()).collect())
            .collect();
        assert_eq!(grid, ["▛▀▜", "▌ ▐", "▙▄▟"]);

        // The border is drawn in the body color; the interior cell is untouched.
        assert_eq!(buf[(0, 0)].fg, BODY);
        assert_eq!(buf[(1, 1)].symbol(), " ");
    }

    #[test]
    fn hollow_two_by_two_clears_only_the_center_sub_cells() {
        let plot = Rect::new(0, 0, 2, 2);
        let mut buf = buffer(2, 2);

        // The only interior is the center 2x2 sub-cells, so each cell loses just
        // its inner corner.
        draw_candle(&mut buf, plot, &hollow(0, 2, 0.0, 2.0));

        let grid: Vec<String> = (0..2)
            .map(|y| (0..2).map(|x| buf[(x, y)].symbol()).collect())
            .collect();
        assert_eq!(grid, ["▛▜", "▙▟"]);
    }

    #[test]
    fn hollow_falls_back_to_a_solid_body_when_one_column_wide() {
        let plot = Rect::new(0, 0, 1, 3);
        let mut buf = buffer(1, 3);

        // One column is two sub-cells wide: every sub-cell is a left or right
        // border, so a hollow body renders the same solid blocks as a filled one.
        draw_candle(&mut buf, plot, &hollow(0, 1, 0.0, 3.0));

        for y in 0..3 {
            assert_eq!(buf[(0, y)].symbol(), "█", "cell (0, {y}) stays solid");
        }
    }

    #[test]
    fn hollow_falls_back_to_a_solid_body_when_one_cell_tall() {
        let plot = Rect::new(0, 0, 3, 1);
        let mut buf = buffer(3, 1);

        // One cell is two sub-cells tall: every sub-cell is a top or bottom
        // border, so the body is drawn solid.
        draw_candle(&mut buf, plot, &hollow(0, 3, 0.0, 1.0));

        for x in 0..3 {
            assert_eq!(buf[(x, 0)].symbol(), "█", "cell ({x}, 0) stays solid");
        }
    }
}
