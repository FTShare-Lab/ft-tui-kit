//! Glyph family selection for candle rendering.

use crate::render::{Block, BoxDrawing, Braille, LineRasterizer, Quadrant, Rasterizer};

/// The glyph family a chart rasterizes candles with.
///
/// This mirrors Ratatui's [`symbols::Marker`](ratatui_core::symbols::Marker):
/// it selects which character set quantizes the fractional-row geometry into
/// terminal cells. Pass it to
/// [`CandleSeries::marker`](crate::CandleSeries::marker).
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum Marker {
    /// Eighth-block glyphs (`█`, `▄`, ...). Bodies resolve to an eighth of a
    /// row and wick tips to a half, painting full-width bars.
    #[default]
    Block,
    /// [Braille](https://en.wikipedia.org/wiki/Braille_Patterns) dots, a 2x4
    /// grid per cell. This quadruples the vertical and doubles the horizontal
    /// resolution, at one color per cell.
    ///
    /// Support depends on the terminal and font; terminals without Unicode
    /// Braille show replacement glyphs instead of dots.
    Braille,
    /// Quadrant and half-block glyphs (`█`, `▀`, `▄`, `▌`, `▝`, ...), a 2x2 grid
    /// per cell. This doubles both the vertical and horizontal resolution, with
    /// one color per cell, so bodies resolve to a half row and a half column like
    /// braille. A hollow body lights only the border sub-cells of the same
    /// footprint, so it is an outline of exactly the size of the filled body.
    /// Wick tips resolve to a half row, as with [`Block`](Self::Block).
    Quadrant,
    /// Box-drawing glyphs (`─`, `│`, `┌`, `┴`, ...). A hollow body is traced as a
    /// rectangle outline, and a wide enough body fuses its wick into the top and
    /// bottom edges with tee glyphs. Because box-drawing lines run through the
    /// center of a cell, body edges resolve to a whole row offset by half a row,
    /// which is coarser than the other markers. Solid bodies, and outlines too
    /// small to close, are filled with [`Quadrant`](Self::Quadrant) blocks inset
    /// to the same bounds the outline traces, so a filled and a hollow body of
    /// the same geometry occupy exactly the same space.
    BoxDrawing,
}

impl Marker {
    /// The rasterizer backend that draws candles in this glyph family.
    pub(crate) fn rasterizer(self) -> &'static dyn Rasterizer {
        match self {
            Self::Block => &Block,
            Self::Braille => &Braille,
            Self::Quadrant => &Quadrant,
            Self::BoxDrawing => &BoxDrawing,
        }
    }

    /// The backend that draws a connected line in this glyph family.
    pub(crate) fn line_rasterizer(self) -> &'static dyn LineRasterizer {
        match self {
            Self::Braille => &Braille,
            Self::Quadrant | Self::Block | Self::BoxDrawing => &Quadrant,
        }
    }
}
