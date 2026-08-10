//! The chart's two axes and the tick selection behind them.
//!
//! [`ValueAxis`] and [`TimeAxis`] are small style-and-layout configurations the
//! chart composes. The tick helpers pick where value labels go and how they read.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::{Alignment, Rect};
use ratatui_core::style::{Color, Style, Styled};

use crate::scale::{TimeScale, ValueScale};

/// Configuration for a vertical value axis, such as a chart's price or volume
/// axis.
///
/// Carries how the labels are styled, how many columns the axis reserves on the
/// right, and how the labels sit within those columns. How each value reads as a
/// label is chosen by the chart that draws the axis.
///
/// By default the value range and tick positions are chosen automatically from
/// the data in view. Pin the range with [`bounds`](Self::bounds), pin the tick
/// positions with [`ticks`](Self::ticks), or nudge how many ticks are chosen
/// automatically with [`tick_count`](Self::tick_count).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValueAxis<'a> {
    pub(crate) style: Style,
    pub(crate) width: u16,
    pub(crate) labels_alignment: Alignment,
    pub(crate) bounds: Option<[f64; 2]>,
    pub(crate) ticks: Option<&'a [f64]>,
    pub(crate) tick_count: usize,
}

/// The vertical axis of a [`CandlestickChart`](crate::CandlestickChart).
pub type PriceAxis<'a> = ValueAxis<'a>;

impl<'a> ValueAxis<'a> {
    /// A value axis with gray, right-aligned labels reserving eight columns, an
    /// autoscaled range, and six automatically chosen ticks.
    pub fn new() -> Self {
        Self {
            style: Style::new().fg(Color::Gray),
            width: 8,
            labels_alignment: Alignment::Right,
            bounds: None,
            ticks: None,
            tick_count: 6,
        }
    }

    /// Sets the label style.
    #[must_use]
    pub fn style(mut self, style: impl Into<Style>) -> Self {
        self.style = style.into();
        self
    }

    /// Sets the width, in columns, reserved for the axis labels.
    #[must_use]
    pub fn width(mut self, cols: u16) -> Self {
        self.width = cols;
        self
    }

    /// Sets how labels are aligned within the columns the axis reserves.
    #[must_use]
    pub fn labels_alignment(mut self, alignment: Alignment) -> Self {
        self.labels_alignment = alignment;
        self
    }

    /// Pins the value range to `[min, max]`, overriding the autoscale that
    /// otherwise fits the data in view.
    ///
    /// Padding and overlays no longer expand the range, and the chart renders
    /// the fixed range even when it has no data to draw.
    #[must_use]
    pub fn bounds(mut self, bounds: [f64; 2]) -> Self {
        self.bounds = Some(bounds);
        self
    }

    /// Pins the tick positions to `values`, overriding the round-numbered ticks
    /// chosen automatically. Ticks outside the value range are not drawn.
    #[must_use]
    pub fn ticks(mut self, values: &'a [f64]) -> Self {
        self.ticks = Some(values);
        self
    }

    /// Sets the target number of automatically chosen ticks. Ignored when the
    /// tick positions are pinned with [`ticks`](Self::ticks). Defaults to six.
    #[must_use]
    pub fn tick_count(mut self, count: usize) -> Self {
        self.tick_count = count;
        self
    }
}

impl Default for ValueAxis<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Styled for ValueAxis<'a> {
    type Item = ValueAxis<'a>;

    fn style(&self) -> Style {
        self.style
    }

    fn set_style<S: Into<Style>>(mut self, style: S) -> Self::Item {
        self.style = style.into();
        self
    }
}

/// Configuration for the time (horizontal) axis.
///
/// Carries the label style, optionally the text for each candle aligned to the
/// full series (index `i` labels `candles[i]`), and how each label sits relative
/// to its candle's center column. Without labels the axis shows candle indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeAxis<'a> {
    pub(crate) style: Style,
    pub(crate) labels: Option<&'a [String]>,
    pub(crate) labels_alignment: Alignment,
}

impl<'a> TimeAxis<'a> {
    /// A time axis with gray labels, no explicit label text, and labels anchored
    /// from their candle's center column.
    pub fn new() -> Self {
        Self {
            style: Style::new().fg(Color::Gray),
            labels: None,
            labels_alignment: Alignment::Left,
        }
    }

    /// Sets the label style.
    #[must_use]
    pub fn style(mut self, style: impl Into<Style>) -> Self {
        self.style = style.into();
        self
    }

    /// Sets the label text, aligned to the full candle slice.
    #[must_use]
    pub fn labels(mut self, labels: &'a [String]) -> Self {
        self.labels = Some(labels);
        self
    }

    /// Sets how each label sits relative to its candle's center column.
    ///
    /// [`Left`](Alignment::Left) starts the label at the center column,
    /// [`Center`](Alignment::Center) centers it on the column, and
    /// [`Right`](Alignment::Right) ends it at the column.
    #[must_use]
    pub fn labels_alignment(mut self, alignment: Alignment) -> Self {
        self.labels_alignment = alignment;
        self
    }
}

impl Default for TimeAxis<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Styled for TimeAxis<'a> {
    type Item = TimeAxis<'a>;

    fn style(&self) -> Style {
        self.style
    }

    fn set_style<S: Into<Style>>(mut self, style: S) -> Self::Item {
        self.style = style.into();
        self
    }
}

/// Rounds a span to a "nice" number (1, 2, 5 times a power of ten).
///
/// With `round`, snaps to the nearest nice number; otherwise rounds up so the
/// result covers the span. Used to keep axis ticks on human-friendly values.
fn nice_num(span: f64, round: bool) -> f64 {
    if span <= 0.0 {
        return 1.0;
    }

    let exp = span.log10().floor();
    let frac = span / 10f64.powf(exp);
    let nice = if round {
        if frac < 1.5 {
            1.0
        } else if frac < 3.0 {
            2.0
        } else if frac < 7.0 {
            5.0
        } else {
            10.0
        }
    } else if frac <= 1.0 {
        1.0
    } else if frac <= 2.0 {
        2.0
    } else if frac <= 5.0 {
        5.0
    } else {
        10.0
    };

    nice * 10f64.powf(exp)
}

/// Picks roughly `target` evenly-spaced, round-numbered price ticks spanning
/// `[min, max]`. Ticks outside the domain are dropped by the caller as needed.
pub(crate) fn value_ticks(min: f64, max: f64, target: usize) -> Vec<f64> {
    let target = target.max(2);
    let range = nice_num(max - min, false);
    let step = nice_num(range / (target as f64 - 1.0), true);

    if step <= 0.0 || !step.is_finite() {
        return vec![min, max];
    }

    let graph_min = (min / step).floor() * step;
    let graph_max = (max / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut t = graph_min;

    // Guard against runaway loops from pathological inputs.
    let max_ticks = target * 4;
    while t <= graph_max + step * 0.5 && ticks.len() < max_ticks {
        ticks.push(t);
        t += step;
    }

    ticks
}

/// Formats a price for an axis label, choosing decimal places from the tick
/// spacing so small ranges keep precision and large ones stay compact.
pub(crate) fn format_price(value: f64, step: f64) -> String {
    let decimals = if step >= 1.0 {
        0
    } else if step >= 0.1 {
        1
    } else if step >= 0.01 {
        2
    } else {
        3
    };

    format!("{value:.decimals$}")
}

/// Formats a volume for an axis label with a magnitude suffix ('K', 'M', 'B').
pub(crate) fn format_volume(value: f64, _step: f64) -> String {
    let abs = value.abs();
    let (scaled, suffix) = if abs >= 1e9 {
        (value / 1e9, "B")
    } else if abs >= 1e6 {
        (value / 1e6, "M")
    } else if abs >= 1e3 {
        (value / 1e3, "K")
    } else {
        return format!("{value:.0}");
    };

    let decimals = if scaled.abs() >= 100.0 { 0 } else { 1 };
    format!("{scaled:.decimals$}{suffix}")
}

/// Draws the right-hand value axis.
///
/// Ticks are the axis's pinned ticks, or round-number ticks across the scale.
/// Labels are formatted by the provided function and aligned within the axis
/// columns.
pub(crate) fn draw_value_axis(
    buf: &mut Buffer,
    plot: Rect,
    scale: &ValueScale,
    axis: &ValueAxis<'_>,
    format: &dyn Fn(f64, f64) -> String,
) {
    let auto;
    let ticks: &[f64] = match axis.ticks {
        Some(ticks) => ticks,
        None => {
            auto = value_ticks(scale.min(), scale.max(), axis.tick_count);
            &auto
        }
    };
    let step = if ticks.len() >= 2 {
        (ticks[1] - ticks[0]).abs()
    } else {
        1.0
    };
    let axis_x = plot.x + plot.width;
    let width = axis.width as usize;

    for &t in ticks.iter() {
        if t < scale.min() || t > scale.max() {
            continue;
        }

        let row = scale.value_to_row(t);
        let label = format(t, step);
        let padded = match axis.labels_alignment {
            Alignment::Left => format!("{label:<width$}"),
            Alignment::Center => format!("{label:^width$}"),
            Alignment::Right => format!("{label:>width$}"),
        };
        buf.set_string(axis_x, plot.y + row, padded, axis.style);
    }
}

/// Draws the bottom time axis.
///
/// Labels are aligned relative to a candle's center column. Labels that would
/// overlap a previous one are skipped to avoid collisions.
pub(crate) fn draw_time_axis(buf: &mut Buffer, plot: Rect, time: &TimeScale, axis: &TimeAxis<'_>) {
    let y = plot.y + plot.height;
    let mut next_free: u16 = plot.x;

    for vi in 0..time.visible() {
        let orig = time.first_visible() + vi;
        let label = match axis.labels {
            Some(labels) if orig < labels.len() => labels[orig].clone(),
            _ => orig.to_string(),
        };
        let cx = plot.x + time.index_to_center_col(vi);
        let len = label.chars().count() as u16;
        let start = match axis.labels_alignment {
            Alignment::Left => cx,
            Alignment::Center => cx.saturating_sub(len / 2),
            Alignment::Right => cx.saturating_sub(len.saturating_sub(1)),
        };

        if start >= next_free && start + len <= plot.x + plot.width {
            buf.set_string(start, y, &label, axis.style);
            next_free = start + len + 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_round_and_evenly_spaced() {
        let ticks = value_ticks(0.0, 100.0, 6);
        assert!(ticks.len() >= 2);
        let step = ticks[1] - ticks[0];
        // The step is a nice number, and spacing is uniform.
        assert!((step - 20.0).abs() < 1e-9, "unexpected step {step}");
        for pair in ticks.windows(2) {
            assert!((pair[1] - pair[0] - step).abs() < 1e-9);
        }
    }

    #[test]
    fn ticks_bracket_the_domain() {
        let ticks = value_ticks(13.0, 87.0, 5);
        assert!(ticks.first().unwrap() <= &13.0);
        assert!(ticks.last().unwrap() >= &87.0);
    }

    #[test]
    fn format_price_picks_decimals_from_step() {
        assert_eq!(format_price(123.456, 5.0), "123");
        assert_eq!(format_price(123.456, 0.5), "123.5");
        assert_eq!(format_price(1.2345, 0.05), "1.23");
        assert_eq!(format_price(1.2345, 0.005), "1.234");
    }

    #[test]
    fn format_volume_scales_to_compact_suffixes() {
        assert_eq!(format_volume(0.0, 0.0), "0");
        assert_eq!(format_volume(500.0, 0.0), "500");
        assert_eq!(format_volume(1_500.0, 0.0), "1.5K");
        assert_eq!(format_volume(950_000.0, 0.0), "950K");
        assert_eq!(format_volume(1_200_000.0, 0.0), "1.2M");
        assert_eq!(format_volume(12_500_000.0, 0.0), "12.5M");
        assert_eq!(format_volume(1_000_000_000.0, 0.0), "1.0B");
    }

    #[test]
    fn degenerate_span_does_not_panic() {
        let ticks = value_ticks(50.0, 50.0, 6);
        assert!(!ticks.is_empty());
        assert!(ticks.iter().all(|t| t.is_finite()));
    }

    #[test]
    fn price_axis_defaults_and_builders() {
        let axis = PriceAxis::default();
        assert_eq!(axis.width, 8);
        assert_eq!(axis.style.fg, Some(Color::Gray));
        assert_eq!(axis.labels_alignment, Alignment::Right);

        let axis = axis
            .width(10)
            .style(Color::Red)
            .labels_alignment(Alignment::Left);
        assert_eq!(axis.width, 10);
        assert_eq!(axis.style.fg, Some(Color::Red));
        assert_eq!(axis.labels_alignment, Alignment::Left);
    }

    #[test]
    fn value_axis_bounds_and_tick_builders() {
        let axis = ValueAxis::default();
        assert!(axis.bounds.is_none());
        assert!(axis.ticks.is_none());
        assert_eq!(axis.tick_count, 6);

        let ticks = [0.0, 50.0, 100.0];
        let axis = axis.bounds([0.0, 100.0]).ticks(&ticks).tick_count(10);
        assert_eq!(axis.bounds, Some([0.0, 100.0]));
        assert_eq!(axis.ticks, Some(&ticks[..]));
        assert_eq!(axis.tick_count, 10);
    }

    #[test]
    fn fixed_ticks_are_drawn_instead_of_auto_ticks() {
        let scale = ValueScale::new(0.0, 100.0, 10);
        let ticks = [0.0, 50.0, 100.0];
        let axis = ValueAxis::new().ticks(&ticks).width(6);
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 10));

        draw_value_axis(
            &mut buf,
            Rect::new(0, 0, 6, 10),
            &scale,
            &axis,
            &|v, step| format_price(v, step),
        );

        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("100"), "pinned tick 100 should be labelled");
        assert!(text.contains("50"), "pinned tick 50 should be labelled");
    }

    #[test]
    fn ticks_outside_the_range_are_not_drawn() {
        let scale = ValueScale::new(0.0, 100.0, 10);
        // 250 sits above the range and must be dropped.
        let ticks = [50.0, 250.0];
        let axis = ValueAxis::new().ticks(&ticks).width(6);
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 10));

        draw_value_axis(
            &mut buf,
            Rect::new(0, 0, 6, 10),
            &scale,
            &axis,
            &|v, step| format_price(v, step),
        );

        let text: String = buf.content().iter().map(|c| c.symbol()).collect();
        assert!(text.contains("50"));
        assert!(!text.contains("250"), "out-of-range tick must be dropped");
    }

    #[test]
    fn time_axis_defaults_and_builders() {
        let axis = TimeAxis::default();
        assert_eq!(axis.style.fg, Some(Color::Gray));
        assert!(axis.labels.is_none());
        assert_eq!(axis.labels_alignment, Alignment::Left);

        let labels = [String::from("a"), String::from("b")];
        let axis = axis
            .labels(&labels)
            .style(Color::Blue)
            .labels_alignment(Alignment::Center);
        assert_eq!(axis.labels.unwrap().len(), 2);
        assert_eq!(axis.style.fg, Some(Color::Blue));
        assert_eq!(axis.labels_alignment, Alignment::Center);
    }
}
