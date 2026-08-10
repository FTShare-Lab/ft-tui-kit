//! Point annotations layered over a chart, such as buy/sell markers.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Modifier, Style};

use crate::overlay::OverlayDraw;
use crate::render::{self, PlotLayout};

/// Where an [`Annotation`] sits relative to its anchor point.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum Anchor {
    /// Above the point, growing upward (symbol nearest, label beyond).
    Above,
    /// Below the point, growing downward.
    Below,
    /// On the point, with any label stacked just above it.
    #[default]
    On,
}

/// A point annotation: an optional symbol and label anchored at a candle index
/// and value.
#[derive(Debug, Clone)]
pub struct Annotation<'a> {
    index: usize,
    value: f64,
    symbol: Option<&'a str>,
    label: Option<&'a str>,
    style: Style,
    anchor: Anchor,
}

impl<'a> Annotation<'a> {
    /// Creates a neutral annotation at `index` and `value` with no symbol or
    /// label, anchored on the point.
    pub fn new(index: usize, value: f64) -> Self {
        Self {
            index,
            value,
            symbol: None,
            label: None,
            style: Style::new(),
            anchor: Anchor::On,
        }
    }

    /// A green `▲` buy marker labeled `BUY`, anchored below the point.
    pub fn buy(index: usize, value: f64) -> Self {
        Self::new(index, value)
            .symbol("▲")
            .label("BUY")
            .style(Color::Green)
            .anchor(Anchor::Below)
    }

    /// A red `▼` sell marker labeled `SELL`, anchored above the point.
    pub fn sell(index: usize, value: f64) -> Self {
        Self::new(index, value)
            .symbol("▼")
            .label("SELL")
            .style(Color::Red)
            .anchor(Anchor::Above)
    }

    /// Sets the glyph drawn at the anchor point.
    #[must_use]
    pub fn symbol(mut self, symbol: &'a str) -> Self {
        self.symbol = Some(symbol);
        self
    }

    /// Sets the text stacked beyond the symbol in the anchor direction.
    #[must_use]
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// Sets the style. Its foreground is the symbol and label color.
    #[must_use]
    pub fn style(mut self, style: impl Into<Style>) -> Self {
        self.style = style.into();
        self
    }

    /// Sets where the annotation sits relative to its point.
    #[must_use]
    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Draws the annotation, with its symbol centered on `center_col` and the
    /// value at `value_row` (both plot-relative).
    fn draw(&self, buf: &mut Buffer, plot: Rect, center_col: u16, value_row: u16, bg: Color) {
        // Must clear REVERSED to ensure that the label is printed correctly
        // over candles.
        let fg = self.style.fg.unwrap_or(Color::Reset);
        let style = self.style.fg(fg).bg(bg).remove_modifier(Modifier::REVERSED);

        let col = i32::from(center_col);
        let row = i32::from(value_row);

        // Direction to offset the annotation.
        let dir: i32 = match self.anchor {
            Anchor::Above => -1,
            Anchor::Below => 1,
            Anchor::On => 0,
        };

        // Direction to offset the label. Label will be above symbol for On.
        let label_dir: i32 = match self.anchor {
            Anchor::Above | Anchor::On => -1,
            Anchor::Below => 1,
        };

        let symbol_row = row + dir;
        let label_row = if self.symbol.is_some() {
            // Place label next to symbol.
            symbol_row + label_dir
        } else {
            // No symbol, use the label in place of the symbol.
            symbol_row
        };

        if let Some(symbol) = self.symbol {
            put_text(buf, plot, col, symbol_row, symbol, style);
        }
        if let Some(label) = self.label {
            let len = label.chars().count() as i32;
            let label_col = col - len / 2;
            put_text(buf, plot, label_col, label_row, label, style);
        }
    }
}

/// Writes `text` starting at the plot-relative cell `(col, row)`, one cell per
/// character, skipping any that fall outside the plot. Coordinates are signed so
/// an annotation anchored near an edge clips cleanly.
fn put_text(buf: &mut Buffer, plot: Rect, col: i32, row: i32, text: &str, style: Style) {
    if row < 0 || row >= i32::from(plot.height) {
        return;
    }
    for (i, ch) in text.chars().enumerate() {
        let c = col + i as i32;
        if c < 0 || c >= i32::from(plot.width) {
            continue;
        }
        let mut bytes = [0u8; 4];
        render::put(
            buf,
            plot,
            c as u32,
            row as u32,
            ch.encode_utf8(&mut bytes),
            style,
        );
    }
}

/// A set of [`Annotation`]s drawn over a chart, aligned to its candles.
///
/// Annotations whose index has scrolled out of view are skipped. By default the
/// chart expands its value axis to keep the annotations' values in view.
#[derive(Debug, Clone)]
pub struct Annotations<'a> {
    items: &'a [Annotation<'a>],
    autoscale: bool,
}

impl<'a> Annotations<'a> {
    /// Creates an overlay drawing `items`.
    pub fn new(items: &'a [Annotation<'a>]) -> Self {
        Self {
            items,
            autoscale: true,
        }
    }

    /// Sets whether the chart expands its value axis to keep the annotations in
    /// view. On by default.
    #[must_use]
    pub fn autoscale(mut self, autoscale: bool) -> Self {
        self.autoscale = autoscale;
        self
    }
}

impl OverlayDraw for Annotations<'_> {
    fn value_bounds(&self) -> Option<(f64, f64)> {
        if !self.autoscale {
            return None;
        }
        let mut iter = self.items.iter();
        let first = iter.next()?;
        let (mut lo, mut hi) = (first.value, first.value);
        for item in iter {
            lo = lo.min(item.value);
            hi = hi.max(item.value);
        }
        Some((lo, hi))
    }

    fn draw(&self, buf: &mut Buffer, layout: &PlotLayout) {
        let plot = layout.plot;
        if plot.width == 0 || plot.height == 0 {
            return;
        }

        let time = &layout.time;
        for item in self.items {
            let first = time.first_visible();
            if item.index < first {
                continue;
            }
            let visible_index = item.index - first;
            if visible_index >= time.visible() {
                continue;
            }
            let center_col = time.index_to_center_col(visible_index);
            let value_row = layout.value.value_to_row(item.value);
            item.draw(buf, plot, center_col, value_row, layout.bg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_bounds_span_the_item_values_and_honor_autoscale() {
        let items = [
            Annotation::buy(0, 100.0),
            Annotation::sell(2, 130.0),
            Annotation::new(4, 115.0),
        ];
        assert_eq!(
            Annotations::new(&items).value_bounds(),
            Some((100.0, 130.0))
        );
        assert_eq!(
            Annotations::new(&items).autoscale(false).value_bounds(),
            None
        );
        assert_eq!(Annotations::new(&[]).value_bounds(), None);
    }

    #[test]
    fn buy_and_sell_have_conventional_defaults() {
        let buy = Annotation::buy(1, 100.0);
        assert_eq!(buy.symbol, Some("▲"));
        assert_eq!(buy.label, Some("BUY"));
        assert_eq!(buy.style.fg, Some(Color::Green));
        assert_eq!(buy.anchor, Anchor::Below);

        let sell = Annotation::sell(1, 100.0);
        assert_eq!(sell.symbol, Some("▼"));
        assert_eq!(sell.label, Some("SELL"));
        assert_eq!(sell.style.fg, Some(Color::Red));
        assert_eq!(sell.anchor, Anchor::Above);
    }
}
