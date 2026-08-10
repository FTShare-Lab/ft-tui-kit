//! A line series: index-aligned values drawn as a connected line.

use ratatui_core::buffer::Buffer;
use ratatui_core::style::{Color, Style};

use crate::marker::Marker;
use crate::render::{PlotLayout, draw_value_line, line_value_bounds};

/// A series of values drawn as a connected line, such as an indicator like a
/// moving average, RSI, or a MACD line.
///
/// This is the dataset a [`LineChart`](crate::LineChart) renders. Each value is
/// aligned to the column of its index in the slice, matching how a
/// [`CandleSeries`](crate::CandleSeries) is laid out, so a line chart can stack
/// beneath a candlestick chart with its points centered on the candles. A `None`
/// breaks the line, leaving a gap for a period with no value (such as the warmup
/// before an indicator has enough data).
#[derive(Debug, Clone)]
pub struct LineSeries<'a> {
    pub(crate) values: &'a [Option<f64>],
    style: Style,
    marker: Marker,
}

impl<'a> LineSeries<'a> {
    /// Creates a white [`Braille`](Marker::Braille) line from values aligned
    /// one-to-one with the chart's columns, where `None` breaks the line.
    pub fn new(values: &'a [Option<f64>]) -> Self {
        Self {
            values,
            style: Style::new().fg(Color::White),
            marker: Marker::Braille,
        }
    }

    /// Sets the line style. Its foreground is the line color.
    #[must_use]
    pub fn style(mut self, style: impl Into<Style>) -> Self {
        self.style = style.into();
        self
    }

    /// Sets the glyph family the line is drawn with. Defaults to
    /// [`Marker::Braille`].
    ///
    /// [`Block`](Marker::Block) and [`BoxDrawing`](Marker::BoxDrawing) fall back
    /// to [`Quadrant`](Marker::Quadrant).
    #[must_use]
    pub fn marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }

    /// The value span the line occupies, or `None` when it has no values. The
    /// chart autoscales its value axis from this.
    pub(crate) fn value_bounds(&self) -> Option<(f64, f64)> {
        line_value_bounds(self.values)
    }

    /// The number of indices the line spans, used to lay out the time axis.
    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    /// Draws the visible portion of the line into the plot.
    pub(crate) fn draw(&self, buf: &mut Buffer, layout: &PlotLayout) {
        draw_value_line(buf, layout, self.values, self.style, self.marker);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_to_a_white_braille_line() {
        let values = [Some(1.0)];
        let series = LineSeries::new(&values);
        assert_eq!(series.style.fg, Some(Color::White));
        assert_eq!(series.marker, Marker::Braille);
    }

    #[test]
    fn value_bounds_span_some_values_and_ignore_none() {
        let values = [None, Some(10.0), Some(30.0), None, Some(20.0)];
        assert_eq!(LineSeries::new(&values).value_bounds(), Some((10.0, 30.0)));
        assert_eq!(LineSeries::new(&[None, None]).value_bounds(), None);
        assert_eq!(LineSeries::new(&[]).value_bounds(), None);
    }

    #[test]
    fn len_counts_every_index_including_gaps() {
        let values = [Some(1.0), None, Some(3.0)];
        assert_eq!(LineSeries::new(&values).len(), 3);
    }
}
