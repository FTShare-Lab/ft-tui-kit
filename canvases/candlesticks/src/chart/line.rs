//! The line chart widget.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Style, Styled};
use ratatui_core::widgets::Widget;
use ratatui_widgets::block::{Block, BlockExt};

use crate::axis::{self, TimeAxis, ValueAxis};
use crate::overlay::{self, Overlay};
use crate::render::PlotLayout;
use crate::scale::{TimeScale, ValueScale};
use crate::series::LineSeries;

/// A line chart: one or more [`LineSeries`] drawn as connected lines with a
/// [`ValueAxis`] and a [`TimeAxis`].
///
/// This plots continuous indicators such as a moving average, RSI, or the lines
/// of a MACD. Every line shares one autoscaling value axis that fits the data in
/// view, like a [`CandlestickChart`](crate::CandlestickChart), and the most
/// recent points that fit are drawn, right-aligned.
///
/// Reference levels, such as the overbought and oversold lines of an RSI, are
/// added as [`TrendLine`](crate::TrendLine) overlays.
///
/// To stack a line chart beneath a [`CandlestickChart`](crate::CandlestickChart)
/// with their time axes aligned column-for-column, give the line chart the same
/// [`width`](Self::width) and [`gap`](Self::gap) as the candle series, the same
/// number of values as candles, the same value-axis width, and lay them out at
/// the same width.
#[derive(Debug, Clone)]
pub struct LineChart<'a> {
    block: Option<Block<'a>>,
    series: Vec<LineSeries<'a>>,
    base: Style,
    pad_frac: f64,
    width: f64,
    gap: f64,
    value_axis: ValueAxis<'a>,
    time_axis: TimeAxis<'a>,
    show_axes: bool,
    underlays: Vec<Overlay<'a>>,
    overlays: Vec<Overlay<'a>>,
}

impl<'a> LineChart<'a> {
    /// Creates a chart that draws `series` with default axes and padding. Add
    /// more lines to the same pane with [`line`](Self::line).
    pub fn new(series: LineSeries<'a>) -> Self {
        Self {
            block: None,
            series: vec![series],
            base: Style::new(),
            pad_frac: 0.05,
            width: 3.0,
            gap: 1.0,
            value_axis: ValueAxis::new(),
            time_axis: TimeAxis::new(),
            show_axes: true,
            underlays: Vec::new(),
            overlays: Vec::new(),
        }
    }

    /// Adds another line to the pane, drawn over the existing ones and sharing
    /// the same value and time axes.
    #[must_use]
    pub fn line(mut self, series: LineSeries<'a>) -> Self {
        self.series.push(series);
        self
    }

    /// Adds several lines to the pane, drawn in order over the existing ones.
    #[must_use]
    pub fn lines(mut self, series: impl IntoIterator<Item = LineSeries<'a>>) -> Self {
        self.series.extend(series);
        self
    }

    /// Sets the base style. Its background is the color partial cells blend
    /// against, so set it to your terminal background for crisp lines.
    #[must_use]
    pub fn style(mut self, style: impl Into<Style>) -> Self {
        self.base = style.into();
        self
    }

    /// Sets the headroom kept above and below the lines as a fraction of their
    /// value span.
    #[must_use]
    pub fn padding(mut self, pad_frac: f64) -> Self {
        self.pad_frac = pad_frac;
        self
    }

    /// Sets the column width of each index slot, matching the candle
    /// [`width`](crate::CandleSeries::width) so the lines align with a candle
    /// chart's columns. Defaults to three columns.
    #[must_use]
    pub fn width(mut self, width: f64) -> Self {
        self.width = width;
        self
    }

    /// Sets the gap between index slots, matching the candle
    /// [`gap`](crate::CandleSeries::gap) so the lines align with a candle chart's
    /// columns. Defaults to one column.
    #[must_use]
    pub fn gap(mut self, gap: f64) -> Self {
        self.gap = gap;
        self
    }

    /// Sets the value (vertical) axis.
    #[must_use]
    pub fn value_axis(mut self, axis: ValueAxis<'a>) -> Self {
        self.value_axis = axis;
        self
    }

    /// Sets the time (horizontal) axis.
    #[must_use]
    pub fn time_axis(mut self, axis: TimeAxis<'a>) -> Self {
        self.time_axis = axis;
        self
    }

    /// Shows or hides both axes.
    #[must_use]
    pub fn axes(mut self, show: bool) -> Self {
        self.show_axes = show;
        self
    }

    /// Wraps the chart with the given [`Block`]
    #[must_use]
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Adds an [`Overlay`] drawn on top of the lines.
    ///
    /// Overlays are drawn in the order they are added, after the lines and before
    /// the axes. By default they expand the value axis so they stay in view.
    #[must_use]
    pub fn overlay(mut self, overlay: impl Into<Overlay<'a>>) -> Self {
        self.overlays.push(overlay.into());
        self
    }

    /// Adds several [`Overlay`]s, drawn in order on top of the lines.
    #[must_use]
    pub fn overlays(mut self, overlays: impl IntoIterator<Item = Overlay<'a>>) -> Self {
        self.overlays.extend(overlays);
        self
    }

    /// Adds an [`Overlay`] drawn behind the lines, so the lines draw over it.
    ///
    /// Underlays are drawn in the order they are added, before the lines. By
    /// default they expand the value axis so they stay in view.
    #[must_use]
    pub fn underlay(mut self, underlay: impl Into<Overlay<'a>>) -> Self {
        self.underlays.push(underlay.into());
        self
    }

    /// Adds several [`Overlay`]s, drawn in order behind the lines.
    #[must_use]
    pub fn underlays(mut self, underlays: impl IntoIterator<Item = Overlay<'a>>) -> Self {
        self.underlays.extend(underlays);
        self
    }

    fn render_chart(&self, area: Rect, buf: &mut Buffer) {
        buf.set_style(area, self.base);

        self.block.as_ref().render(area, buf);
        let chart_area = self.block.inner_if_some(area);

        let bg = self.base.bg.unwrap_or(Color::Reset);
        let Some(layout) = self.layout(chart_area, bg) else {
            return;
        };

        overlay::draw_all(&self.underlays, buf, &layout);
        for series in &self.series {
            series.draw(buf, &layout);
        }
        overlay::draw_all(&self.overlays, buf, &layout);

        if self.show_axes {
            axis::draw_value_axis(
                buf,
                layout.plot,
                &layout.value,
                &self.value_axis,
                &|v, step| axis::format_price(v, step),
            );
            axis::draw_time_axis(buf, layout.plot, &layout.time, &self.time_axis);
        }
    }

    /// Computes the plot rectangle and the value and time scales for `area`.
    ///
    /// The value scale fits every line in view with `pad_frac` of headroom above
    /// and below, expanded to keep any autoscaling overlay visible. The time
    /// scale spans the longest line. Returns `None` when no plot fits or there is
    /// nothing to draw.
    fn layout(&self, area: Rect, bg: Color) -> Option<PlotLayout> {
        let right_axis_w = if self.show_axes {
            self.value_axis.width
        } else {
            0
        };
        let bottom_axis_h = if self.show_axes { 1 } else { 0 };

        if area.width <= right_axis_w || area.height <= bottom_axis_h {
            return None;
        }

        let plot = Rect {
            x: area.x,
            y: area.y,
            width: area.width - right_axis_w,
            height: area.height - bottom_axis_h,
        };

        let value = match self.value_axis.bounds {
            Some([min, max]) => ValueScale::new(min, max, plot.height),
            None => {
                let (lo, hi) = self
                    .series
                    .iter()
                    .filter_map(LineSeries::value_bounds)
                    .reduce(|(lo, hi), (olo, ohi)| (lo.min(olo), hi.max(ohi)))?;
                let (lo, hi) = overlay::union_bounds((lo, hi), &self.underlays);
                let (lo, hi) = overlay::union_bounds((lo, hi), &self.overlays);
                ValueScale::autoscale(lo, hi, plot.height, self.pad_frac)
            }
        };

        let count = self.series.iter().map(LineSeries::len).max()?;
        let time = TimeScale::new(plot.width, count, self.width, self.gap);

        Some(PlotLayout {
            plot,
            value,
            time,
            bg,
        })
    }
}

impl Widget for &LineChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_chart(area, buf);
    }
}

impl Widget for LineChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_chart(area, buf);
    }
}

impl<'a> Styled for LineChart<'a> {
    type Item = LineChart<'a>;

    fn style(&self) -> Style {
        self.base
    }

    fn set_style<S: Into<Style>>(mut self, style: S) -> Self::Item {
        self.base = style.into();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::layout::Rect;

    fn layout_of(chart: &LineChart, area: Rect) -> Option<PlotLayout> {
        chart.layout(area, Color::Reset)
    }

    #[test]
    fn layout_unions_value_bounds_across_every_line() {
        let a = [Some(10.0), Some(20.0)];
        let b = [Some(5.0), Some(40.0)];
        let chart = LineChart::new(LineSeries::new(&a)).line(LineSeries::new(&b));
        let layout = layout_of(&chart, Rect::new(0, 0, 40, 20)).expect("a plot fits");
        // With zero padding the axis would span exactly the unioned [5, 40]; the
        // default 5% padding widens it, so the lowest value sits below the top
        // and the highest above the bottom.
        assert!(layout.value.row_f64_to_value(0.0) > 40.0);
        assert!(layout.value.row_f64_to_value(f64::from(layout.plot.height)) < 5.0);
    }

    #[test]
    fn time_scale_spans_the_longest_line() {
        let short = [Some(1.0), Some(2.0)];
        let long = [Some(1.0), Some(2.0), Some(3.0), Some(4.0)];
        let chart = LineChart::new(LineSeries::new(&short)).line(LineSeries::new(&long));
        let layout = layout_of(&chart, Rect::new(0, 0, 40, 20)).expect("a plot fits");
        assert_eq!(layout.time.visible(), 4);
    }

    #[test]
    fn layout_is_none_when_every_line_is_empty() {
        let empty: [Option<f64>; 0] = [];
        let chart = LineChart::new(LineSeries::new(&empty));
        assert!(layout_of(&chart, Rect::new(0, 0, 40, 20)).is_none());
    }

    #[test]
    fn fixed_bounds_override_autoscale_and_padding() {
        let values = [Some(10.0), Some(20.0)];
        let chart = LineChart::new(LineSeries::new(&values))
            .value_axis(ValueAxis::new().bounds([0.0, 200.0]));
        let layout = layout_of(&chart, Rect::new(0, 0, 40, 20)).expect("a plot fits");
        // The pinned range is used verbatim, with no autoscale or padding.
        assert_eq!(layout.value.min(), 0.0);
        assert_eq!(layout.value.max(), 200.0);
    }

    #[test]
    fn fixed_bounds_render_even_without_data() {
        let empty: [Option<f64>; 0] = [];
        let chart = LineChart::new(LineSeries::new(&empty))
            .value_axis(ValueAxis::new().bounds([0.0, 100.0]));
        let layout = layout_of(&chart, Rect::new(0, 0, 40, 20)).expect("fixed bounds still render");
        assert_eq!(layout.value.min(), 0.0);
        assert_eq!(layout.value.max(), 100.0);
    }

    #[test]
    fn layout_is_none_when_no_plot_fits() {
        let values = [Some(1.0), Some(2.0)];
        let chart = LineChart::new(LineSeries::new(&values));
        // The area is no wider than the value axis gutter, so no plot remains.
        assert!(layout_of(&chart, Rect::new(0, 0, 8, 20)).is_none());
    }
}
