//! Candle data and the candle series.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Style};

use crate::marker::Marker;
use crate::render::{BodyFill, CandleGeometry, PlotLayout, Series};
use crate::scale::TimeScale;

use super::Direction;

/// A single open/high/low/close bar.
///
/// Prices are `f64`. Time is intentionally absent: a bar's position on the
/// x-axis is its index in the slice handed to the chart, so callers are free to
/// use any time representation (or none).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candle {
    /// Opening price.
    pub open: f64,
    /// Highest traded price.
    pub high: f64,
    /// Lowest traded price.
    pub low: f64,
    /// Closing price.
    pub close: f64,
}

impl Candle {
    /// Creates a candle from its four prices.
    pub const fn new(open: f64, high: f64, low: f64, close: f64) -> Self {
        Self {
            open,
            high,
            low,
            close,
        }
    }

    /// `true` when the bar closed at or above its open (drawn with the bull color).
    pub fn is_bullish(&self) -> bool {
        self.close >= self.open
    }

    /// Whether the bar closed up, down, or unchanged from its open.
    pub fn direction(&self) -> Direction {
        if self.close > self.open {
            Direction::Up
        } else if self.close < self.open {
            Direction::Down
        } else {
            Direction::Flat
        }
    }

    /// The higher of open/close, the top edge of the body.
    pub fn body_top(&self) -> f64 {
        self.open.max(self.close)
    }

    /// The lower of open/close, the bottom edge of the body.
    pub fn body_bottom(&self) -> f64 {
        self.open.min(self.close)
    }
}

/// The lowest low and highest high across a set of candles.
///
/// Returns `None` for an empty slice. Used by the chart to autoscale the price
/// axis to the data in view.
pub fn price_bounds(candles: &[Candle]) -> Option<(f64, f64)> {
    let mut iter = candles.iter();
    let first = iter.next()?;
    let mut lo = first.low;
    let mut hi = first.high;
    for c in iter {
        if c.low < lo {
            lo = c.low;
        }
        if c.high > hi {
            hi = c.high;
        }
    }
    Some((lo, hi))
}

/// A series of candles together with how it is drawn.
///
/// This is the dataset a [`CandlestickChart`](crate::CandlestickChart) renders.
///
/// Rendering can be customized with the [`marker`](Self::marker) method. The
/// [`width`](Self::width) and [`gap`](Self::gap) methods set the column geometry.
#[derive(Debug, Clone)]
pub struct CandleSeries<'a> {
    pub(crate) candles: &'a [Candle],
    bull: Style,
    bear: Style,
    wick: Option<Style>,
    highlighted: Option<&'a [usize]>,
    highlight: Style,
    bull_fill: BodyFill,
    bear_fill: BodyFill,
    marker: Marker,
    pub(crate) width: f64,
    pub(crate) gap: f64,
}

impl<'a> CandleSeries<'a> {
    /// Creates a series over `candles` with the default green-up / red-down
    /// scheme, three-column bodies, and a one-column gap.
    pub fn new(candles: &'a [Candle]) -> Self {
        Self {
            candles,
            bull: Style::new().fg(Color::Green),
            bear: Style::new().fg(Color::Red),
            wick: None,
            highlighted: None,
            highlight: Style::new().fg(Color::Yellow),
            bull_fill: BodyFill::Filled,
            bear_fill: BodyFill::Filled,
            marker: Marker::default(),
            width: 3.0,
            gap: 1.0,
        }
    }

    /// Sets the style for bull (close at or above open) bodies. Its foreground
    /// is the body color.
    #[must_use]
    pub fn bull_style(mut self, style: impl Into<Style>) -> Self {
        self.bull = style.into();
        self
    }

    /// Sets the style for bear (close below open) bodies. Its foreground is the
    /// body color.
    #[must_use]
    pub fn bear_style(mut self, style: impl Into<Style>) -> Self {
        self.bear = style.into();
        self
    }

    /// Sets an explicit wick style. Without one, a wick takes its candle's body
    /// color.
    #[must_use]
    pub fn wick_style(mut self, style: impl Into<Style>) -> Self {
        self.wick = Some(style.into());
        self
    }

    /// Sets candle indices to draw with the highlight style.
    ///
    /// Indices are relative to the slice passed to [`new`](Self::new). The
    /// highlight style's foreground is used for both the candle body and wick.
    #[must_use]
    pub fn highlighted(mut self, indices: &'a [usize]) -> Self {
        self.highlighted = Some(indices);
        self
    }

    /// Sets the style for highlighted candles.
    ///
    /// Its foreground is the highlight color. Defaults to yellow.
    #[must_use]
    pub fn highlight_style(mut self, style: impl Into<Style>) -> Self {
        self.highlight = style.into();
        self
    }

    /// Sets the fill style for both directions.
    #[must_use]
    pub fn fill(mut self, fill: BodyFill) -> Self {
        self.bull_fill = fill;
        self.bear_fill = fill;
        self
    }

    /// Sets how bull (close at or above open) bodies are filled. Defaults to
    /// [`BodyFill::Filled`].
    #[must_use]
    pub fn bull_fill(mut self, fill: BodyFill) -> Self {
        self.bull_fill = fill;
        self
    }

    /// Sets how bear (close below open) bodies are filled. Defaults to
    /// [`BodyFill::Filled`].
    #[must_use]
    pub fn bear_fill(mut self, fill: BodyFill) -> Self {
        self.bear_fill = fill;
        self
    }

    /// Sets the glyph family the candles are drawn with. Defaults to
    /// [`Marker::Block`].
    #[must_use]
    pub fn marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }

    /// Sets the candle body width in columns.
    ///
    /// May be fractional. Each backend quantizes the width to its horizontal grid.
    #[must_use]
    pub fn width(mut self, cols: f64) -> Self {
        self.width = cols;
        self
    }

    /// Sets the gap, in columns, between adjacent candles.
    ///
    /// May be fractional. Each backend quantized the width to its horizontal grid.
    #[must_use]
    pub fn gap(mut self, gap: f64) -> Self {
        self.gap = gap;
        self
    }

    /// The body color for a candle, taken from the bull or bear style foreground.
    pub(crate) fn body_color(&self, candle: Candle) -> Color {
        let style = if candle.is_bullish() {
            self.bull
        } else {
            self.bear
        };
        style.fg.unwrap_or(Color::Reset)
    }

    /// The wick color for a candle, honoring an explicit wick style and falling
    /// back to the body color.
    pub(crate) fn wick_color(&self, candle: Candle) -> Color {
        self.wick
            .and_then(|w| w.fg)
            .unwrap_or_else(|| self.body_color(candle))
    }

    /// The fill style for a candle, chosen by its direction.
    pub(crate) fn body_fill(&self, candle: Candle) -> BodyFill {
        if candle.is_bullish() {
            self.bull_fill
        } else {
            self.bear_fill
        }
    }

    fn is_highlighted(&self, index: usize) -> bool {
        self.highlighted
            .is_some_and(|indices| indices.contains(&index))
    }

    fn highlight_color(&self) -> Color {
        self.highlight.fg.unwrap_or(Color::Yellow)
    }
}

impl Series for CandleSeries<'_> {
    fn value_bounds(&self) -> Option<(f64, f64)> {
        price_bounds(self.candles)
    }

    fn time_scale(&self, plot: Rect) -> TimeScale {
        TimeScale::new(plot.width, self.candles.len(), self.width, self.gap)
    }

    fn draw(&self, buf: &mut Buffer, layout: &PlotLayout) {
        let rasterizer = self.marker.rasterizer();
        let plot = layout.plot;
        let scale = layout.value;
        let time = layout.time;
        let bg = layout.bg;

        for vi in 0..time.visible() {
            let index = time.first_visible() + vi;
            let candle = self.candles[index];
            let body_left = time.index_to_left(vi);
            let highlighted = self.is_highlighted(index);
            let body = if highlighted {
                self.highlight_color()
            } else {
                self.body_color(candle)
            };
            let wick = if highlighted {
                self.highlight_color()
            } else {
                self.wick_color(candle)
            };

            let geometry = CandleGeometry {
                body_left,
                body_right: body_left + time.candle_width(),
                body_top_row: scale.value_to_row_f64(candle.body_top()),
                body_bottom_row: scale.value_to_row_f64(candle.body_bottom()),
                high_row: scale.value_to_row_f64(candle.high),
                low_row: scale.value_to_row_f64(candle.low),
                body,
                wick,
                bg,
                fill: self.body_fill(candle),
            };
            rasterizer.draw_candle(buf, plot, &geometry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bullish_when_close_at_or_above_open() {
        assert!(Candle::new(100.0, 105.0, 99.0, 104.0).is_bullish());
        assert!(Candle::new(100.0, 105.0, 99.0, 100.0).is_bullish());
        assert!(!Candle::new(100.0, 105.0, 95.0, 96.0).is_bullish());
    }

    #[test]
    fn direction_distinguishes_up_down_and_flat() {
        assert_eq!(
            Candle::new(100.0, 105.0, 99.0, 104.0).direction(),
            Direction::Up
        );
        assert_eq!(
            Candle::new(104.0, 105.0, 95.0, 96.0).direction(),
            Direction::Down
        );
        assert_eq!(
            Candle::new(100.0, 105.0, 99.0, 100.0).direction(),
            Direction::Flat
        );
    }

    #[test]
    fn body_edges_are_ordered_regardless_of_direction() {
        let bull = Candle::new(100.0, 110.0, 99.0, 108.0);
        assert_eq!(bull.body_top(), 108.0);
        assert_eq!(bull.body_bottom(), 100.0);

        let bear = Candle::new(108.0, 109.0, 95.0, 100.0);
        assert_eq!(bear.body_top(), 108.0);
        assert_eq!(bear.body_bottom(), 100.0);
    }

    #[test]
    fn price_bounds_spans_lowest_low_and_highest_high() {
        let candles = [
            Candle::new(100.0, 106.0, 98.0, 105.0),
            Candle::new(105.0, 112.0, 104.0, 110.0),
            Candle::new(110.0, 111.0, 90.0, 92.0),
        ];
        assert_eq!(price_bounds(&candles), Some((90.0, 112.0)));
    }

    #[test]
    fn price_bounds_is_none_for_empty() {
        assert_eq!(price_bounds(&[]), None);
    }

    #[test]
    fn series_has_green_up_red_down_defaults() {
        let candles = [
            Candle::new(100.0, 110.0, 99.0, 108.0), // bull
            Candle::new(108.0, 109.0, 95.0, 96.0),  // bear
        ];
        let series = CandleSeries::new(&candles);
        assert_eq!(series.width, 3.0);
        assert_eq!(series.gap, 1.0);
        assert_eq!(series.body_color(candles[0]), Color::Green);
        assert_eq!(series.body_color(candles[1]), Color::Red);
    }

    #[test]
    fn body_fill_defaults_to_filled_and_is_chosen_per_direction() {
        let candles = [
            Candle::new(100.0, 110.0, 99.0, 108.0), // bull
            Candle::new(108.0, 109.0, 95.0, 96.0),  // bear
        ];

        let default = CandleSeries::new(&candles);
        assert_eq!(default.body_fill(candles[0]), BodyFill::Filled);
        assert_eq!(default.body_fill(candles[1]), BodyFill::Filled);

        let series = CandleSeries::new(&candles)
            .bull_fill(BodyFill::Hollow)
            .bear_fill(BodyFill::Filled);
        assert_eq!(series.body_fill(candles[0]), BodyFill::Hollow);
        assert_eq!(series.body_fill(candles[1]), BodyFill::Filled);
    }

    #[test]
    fn wick_color_falls_back_to_body_then_honors_override() {
        let candles = [Candle::new(100.0, 110.0, 99.0, 108.0)]; // bull
        let series = CandleSeries::new(&candles).bull_style(Color::Cyan);
        assert_eq!(series.wick_color(candles[0]), Color::Cyan);

        let series = series.wick_style(Color::Gray);
        assert_eq!(series.wick_color(candles[0]), Color::Gray);
    }
}
