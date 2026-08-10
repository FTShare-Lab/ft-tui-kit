//! Sub-cell polyline rasterizer.
//!
//! Draws a connected line through fractional plot points by lighting sub-cells
//! along each segment and folding them into a glyph family's characters.

use std::collections::BTreeMap;

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};

use crate::marker::Marker;
use crate::render::PlotLayout;

/// The value span covered by `values`, ignoring `None` gaps, or `None` when no
/// value is present. Used to autoscale a chart to an index-aligned line.
pub(crate) fn line_value_bounds(values: &[Option<f64>]) -> Option<(f64, f64)> {
    let mut iter = values.iter().flatten().copied();
    let first = iter.next()?;
    let (mut lo, mut hi) = (first, first);
    for value in iter {
        lo = lo.min(value);
        hi = hi.max(value);
    }
    Some((lo, hi))
}

/// Draws `values` as a connected line over the plot, aligned one-to-one with the
/// columns of `layout`, where `None` breaks the line. Each value is aligned to
/// the center of its index's column, drawn in `style`'s foreground over its
/// background (falling back to the plot background) with `marker`'s glyphs.
pub(crate) fn draw_value_line(
    buf: &mut Buffer,
    layout: &PlotLayout,
    values: &[Option<f64>],
    style: Style,
    marker: Marker,
) {
    let plot = layout.plot;
    if plot.width == 0 || plot.height == 0 {
        return;
    }

    let time = &layout.time;
    let scale = &layout.value;
    let color = style.fg.unwrap_or(Color::Reset);
    let bg = style.bg.unwrap_or(layout.bg);

    let points: Vec<Option<(f64, f64)>> = (0..time.visible())
        .map(|vi| {
            let value = (*values.get(time.first_visible() + vi)?)?;
            let col = time.index_to_left(vi) + time.candle_width() / 2.0;
            Some((col, scale.value_to_row_f64(value)))
        })
        .collect();

    marker
        .line_rasterizer()
        .draw_polyline(buf, plot, &points, color, bg);
}

/// A backend that draws a polyline by quantizing it to a glyph family's sub-cell
/// grid.
pub(crate) trait LineRasterizer {
    /// Draws the connected runs of `points` into `plot`, in `color` over `bg`. A
    /// `None` entry breaks the line so the runs on either side are not joined.
    fn draw_polyline(
        &self,
        buf: &mut Buffer,
        plot: Rect,
        points: &[Option<(f64, f64)>],
        color: Color,
        bg: Color,
    );
}

/// Rasterizes `points` onto a sub-cell grid of `(sub_x, sub_y)` sub-cells per
/// cell, folding each touched cell's lit sub-cells into a glyph via `glyph`.
///
/// Each touched cell is overwritten with the line glyph in `color` over `bg`.
/// Modifiers are cleared so it can print correctly over candles.
pub(crate) fn rasterize(
    buf: &mut Buffer,
    plot: Rect,
    points: &[Option<(f64, f64)>],
    grid: (u32, u32),
    glyph: impl Fn(u16) -> char,
    color: Color,
    bg: Color,
) {
    if plot.width == 0 || plot.height == 0 {
        return;
    }

    let (sub_x, sub_y) = grid;
    let max_x = u32::from(plot.width) * sub_x;
    let max_y = u32::from(plot.height) * sub_y;

    // Convert fractional coordinates to sub-cell coordinates.
    let to_sub = |(col, row): (f64, f64)| -> (i64, i64) {
        let sx = ((col * sub_x as f64).floor() as i64).clamp(0, max_x as i64 - 1);
        let sy = ((row * sub_y as f64).floor() as i64).clamp(0, max_y as i64 - 1);
        (sx, sy)
    };

    let mut cells: BTreeMap<(u16, u16), u16> = BTreeMap::new();
    let mut light = |x: i64, y: i64| {
        let (x, y) = (x as u32, y as u32);
        let cell = ((x / sub_x) as u16, (y / sub_y) as u16);
        let bit = 1u16 << ((y % sub_y) * sub_x + (x % sub_x));
        *cells.entry(cell).or_insert(0) |= bit;
    };

    let mut prev: Option<(i64, i64)> = None;
    for point in points {
        match *point {
            Some(p) => {
                let cur = to_sub(p);
                match prev {
                    Some(start) => draw_segment(start, cur, &mut light),
                    None => light(cur.0, cur.1),
                }
                prev = Some(cur);
            }
            None => prev = None,
        }
    }

    let mut symbol = [0u8; 4];
    for ((cell_x, cell_y), bits) in cells {
        if let Some(cell) = buf.cell_mut((plot.x + cell_x, plot.y + cell_y)) {
            cell.set_symbol(glyph(bits).encode_utf8(&mut symbol));
            cell.fg = color;
            cell.bg = bg;
            cell.modifier = Modifier::empty();
        }
    }
}

/// Walks the integer sub-cells of the segment from `a` to `b` (inclusive),
/// lighting each with `light`, using Bresenham's line algorithm.
fn draw_segment(a: (i64, i64), b: (i64, i64), light: &mut impl FnMut(i64, i64)) {
    let (mut x0, mut y0) = a;
    let (x1, y1) = b;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let step_x = if x0 < x1 { 1 } else { -1 };
    let step_y = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        light(x0, y0);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += step_x;
        }
        if e2 <= dx {
            err += dx;
            y0 += step_y;
        }
    }
}
