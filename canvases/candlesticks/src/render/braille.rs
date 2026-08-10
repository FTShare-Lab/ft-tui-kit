//! Braille-dot rasterizer.
//!
//! Braille patterns pack a 2x4 grid of dots into a single cell, giving four
//! times the vertical and twice the horizontal resolution of a whole cell. A
//! candle arrives as fractional-row geometry and is quantized to the dot grid.
//!
//! Each cell is monochrome: a cell's dots all share one foreground color. Where
//! a body and a wick would meet in the same cell the body color wins.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::Color;
use ratatui_core::symbols::braille::BRAILLE;

use crate::render::line::{self, LineRasterizer};
use crate::render::{self, BodyFill, CandleGeometry, Rasterizer};

/// Braille-dot rasterizer backend.
///
/// Quantizes a candle's fractional rows to a 2x4 dot grid per cell and
/// accumulates them into braille glyphs. Each cell is a single color.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Braille;

impl Rasterizer for Braille {
    fn draw_candle(&self, buf: &mut Buffer, plot: Rect, geometry: &CandleGeometry) {
        draw_candle(buf, plot, geometry);
    }
}

impl LineRasterizer for Braille {
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
            (u32::from(DOTS_X), DOTS_Y),
            |pattern| BRAILLE[pattern as usize],
            color,
            bg,
        );
    }
}

/// Braille dot columns per terminal column.
const DOTS_X: u16 = 2;

/// Braille dot rows per terminal row.
const DOTS_Y: u32 = 4;

/// One lit dot accumulated into the grid, by absolute dot coordinates.
///
/// The color is carried so a later body dot can overwrite an earlier wick dot's
/// color within the shared cell.
struct Dot {
    x: u32,
    y: u32,
    color: Color,
}

/// Draws one candle into `plot` using braille dots.
pub(crate) fn draw_candle(buf: &mut Buffer, plot: Rect, geometry: &CandleGeometry) {
    if plot.width == 0 || plot.height == 0 {
        return;
    }

    let CandleGeometry {
        body_left,
        body_right,
        body_top_row,
        body_bottom_row,
        high_row,
        low_row,
        body,
        wick,
        bg,
        fill,
    } = *geometry;

    let max_dot_y = u32::from(plot.height) * DOTS_Y;
    let max_dot_x = u32::from(plot.width) * u32::from(DOTS_X);

    // Body endpoints to the nearest dot row, at least one dot tall so a doji
    // still shows a body.
    let (top_dot, bot_dot) =
        render::quantize_span(body_top_row, body_bottom_row, DOTS_Y, max_dot_y);

    let last_dot_y = max_dot_y - 1;
    let high_dot = ((high_row * DOTS_Y as f64).round() as u32).min(last_dot_y);
    let low_dot = ((low_row * DOTS_Y as f64).round() as u32).min(last_dot_y);

    // Body edges to the nearest dot column, at least one dot wide.
    let (left_dot, right_dot) =
        render::quantize_span(body_left, body_right, u32::from(DOTS_X), max_dot_x);

    let mut dots: Vec<Dot> = Vec::new();

    // The wick runs up and down the dot column nearest the body's center.
    let center_dot = (geometry.center() * f64::from(DOTS_X) - 0.5).round() as u32;
    if center_dot < max_dot_x {
        for y in high_dot..top_dot {
            dots.push(Dot {
                x: center_dot,
                y,
                color: wick,
            });
        }
        for y in bot_dot..=low_dot {
            dots.push(Dot {
                x: center_dot,
                y,
                color: wick,
            });
        }
    }

    // The body fills every dot column it spans. Pushed after the wick so a
    // shared cell takes the body color. A hollow body lights only its border
    // dots; a body too small to have an interior renders solid.
    let hollow = fill == BodyFill::Hollow;
    for dot_x in left_dot..right_dot {
        for y in top_dot..bot_dot {
            let on_border = render::on_border(dot_x, y, left_dot, right_dot, top_dot, bot_dot);
            if !hollow || on_border {
                dots.push(Dot {
                    x: dot_x,
                    y,
                    color: body,
                });
            }
        }
    }

    accumulate(buf, plot, &dots, bg);
}

/// Folds lit dots into braille glyphs, one cell per touched `(col, row)`, and
/// writes them into `buf`.
fn accumulate(buf: &mut Buffer, plot: Rect, dots: &[Dot], bg: Color) {
    use std::collections::BTreeMap;

    // Per cell: the accumulated dot bit-pattern and the last color written.
    // Cell coordinates are plot-relative; the plot offset is added on write.
    let mut cells: BTreeMap<(u16, u16), (u16, Color)> = BTreeMap::new();

    for dot in dots {
        let cell_x = (dot.x / u32::from(DOTS_X)) as u16;
        let cell_y = (dot.y / DOTS_Y) as u16;
        if cell_x >= plot.width || cell_y >= plot.height {
            continue;
        }

        // Convert the dot coordinates to the bit position in the braille pattern.
        let bit_x = dot.x % u32::from(DOTS_X);
        let bit_y = dot.y % DOTS_Y;
        let bit = 1u16 << (bit_y * u32::from(DOTS_X) + bit_x);

        let entry = cells
            .entry((plot.x + cell_x, plot.y + cell_y))
            .or_insert((0, bg));
        entry.0 |= bit;
        entry.1 = dot.color;
    }

    for ((x, y), (pattern, color)) in cells {
        if let Some(cell) = buf.cell_mut((x, y)) {
            cell.set_symbol(BRAILLE[pattern as usize].encode_utf8(&mut [0; 4]));
            cell.fg = color;
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

    /// A single-column candle at column 0.
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
    fn full_cell_lights_every_dot() {
        let plot = Rect::new(0, 0, 1, 1);
        let mut buf = buffer(1, 1);

        // A body filling the whole single row, no wick reach.
        draw_candle(&mut buf, plot, &geometry(0.0, 1.0, 0.0, 1.0));

        let cell = &buf[(0, 0)];
        // All eight dots lit is the last braille glyph.
        assert_eq!(cell.symbol(), "\u{28FF}");
        assert_eq!(cell.fg, BODY);
        assert_eq!(cell.bg, BG);
    }

    #[test]
    fn top_quarter_lights_only_the_top_dot_row() {
        let plot = Rect::new(0, 0, 1, 1);
        let mut buf = buffer(1, 1);

        // Top quarter of the cell: dot rows cover [0, 1). The high and low round
        // into the body's own dot row, so no wick extends past it and only the
        // body dots light.
        draw_candle(&mut buf, plot, &geometry(0.0, 0.25, 0.1, 0.1));

        // Both top dots (bits 0 and 1) lit: pattern 0b11 -> BRAILLE[3].
        let cell = &buf[(0, 0)];
        assert_eq!(cell.symbol(), BRAILLE[3].to_string());
        assert_eq!(cell.fg, BODY);
    }

    #[test]
    fn wick_lights_the_center_dot_column_above_the_body() {
        let plot = Rect::new(0, 0, 1, 2);
        let mut buf = buffer(1, 2);

        // Body in the lower cell; wick reaches up through the upper cell.
        draw_candle(&mut buf, plot, &geometry(1.0, 2.0, 0.0, 2.0));

        // The upper cell carries only the wick, in the right dot column.
        let top = &buf[(0, 0)];
        assert_eq!(top.fg, WICK);
        // Right dot column lit across all four dot rows: bits 1,3,5,7.
        let expected = (1 << 1) | (1 << 3) | (1 << 5) | (1 << 7);
        assert_eq!(top.symbol(), BRAILLE[expected].to_string());
    }

    #[test]
    fn hollow_body_lights_only_its_border_dots() {
        let plot = Rect::new(0, 0, 2, 1);
        let mut buf = buffer(2, 1);

        // A two-column, full-height body. The dot grid is 4 wide by 4 tall, so a
        // hollow body has an interior to clear.
        let geometry = CandleGeometry {
            body_left: 0.0,
            body_right: 2.0,
            body_top_row: 0.0,
            body_bottom_row: 1.0,
            high_row: 0.0,
            low_row: 1.0,
            body: BODY,
            wick: WICK,
            bg: BG,
            fill: BodyFill::Hollow,
        };
        draw_candle(&mut buf, plot, &geometry);

        // Left cell: the left dot column is fully lit (border) and the inner dot
        // column lit only at the top and bottom rows. Bits 3 and 5, the two
        // interior dots, stay dark.
        let left = &buf[(0, 0)];
        assert_eq!(left.symbol(), BRAILLE[0b1101_0111].to_string());
        assert_eq!(left.fg, BODY);
    }

    #[test]
    fn hollow_falls_back_to_filled_when_a_single_column_has_no_interior() {
        let plot = Rect::new(0, 0, 1, 1);
        let geometry = |fill| CandleGeometry {
            body_left: 0.0,
            body_right: 1.0,
            body_top_row: 0.0,
            body_bottom_row: 1.0,
            high_row: 0.0,
            low_row: 1.0,
            body: BODY,
            wick: WICK,
            bg: BG,
            fill,
        };

        let mut filled = buffer(1, 1);
        draw_candle(&mut filled, plot, &geometry(BodyFill::Filled));
        let mut hollow = buffer(1, 1);
        draw_candle(&mut hollow, plot, &geometry(BodyFill::Hollow));

        // One column is two dots wide: every dot is a left or right border, so a
        // hollow body renders the same solid glyph as a filled one.
        assert_eq!(hollow[(0, 0)].symbol(), filled[(0, 0)].symbol());
        assert_eq!(filled[(0, 0)].symbol(), "\u{28FF}");
    }

    #[test]
    fn body_color_wins_over_wick_in_a_shared_cell() {
        let plot = Rect::new(0, 0, 1, 1);
        let mut buf = buffer(1, 1);

        // Body covers the top half; wick reaches the low at the bottom, so both
        // land in the one cell. The body is drawn last and owns the color.
        draw_candle(&mut buf, plot, &geometry(0.0, 0.5, 0.0, 1.0));

        let cell = &buf[(0, 0)];
        assert_eq!(cell.fg, BODY);
    }
}
