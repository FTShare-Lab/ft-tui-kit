//! The volume chart widget.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Style, Styled};
use ratatui_core::widgets::Widget;
use ratatui_widgets::block::{Block, BlockExt};

use crate::axis::{self, TimeAxis, ValueAxis};
use crate::overlay::{self, Overlay};
use crate::render::{PlotLayout, Series};
use crate::scale::ValueScale;
use crate::series::VolumeSeries;

/// A volume chart: a [`VolumeSeries`] drawn as bars with a [`ValueAxis`] and a
/// [`TimeAxis`].
///
/// The value axis scales to the largest volume in view and the most recent bars
/// that fit are drawn, right-aligned.
///
/// To stack a volume chart beneath a
/// [`CandlestickChart`](crate::CandlestickChart) with their time axes aligned
/// column-for-column, give the two series the same bar
/// [`width`](VolumeSeries::width), [`gap`](VolumeSeries::gap), and length, give
/// both charts the same value-axis width, and lay them out at the same width.
#[derive(Debug, Clone)]
pub struct VolumeChart<'a> {
    block: Option<Block<'a>>,
    series: VolumeSeries<'a>,
    base: Style,
    pad_frac: f64,
    value_axis: ValueAxis<'a>,
    time_axis: TimeAxis<'a>,
    show_axes: bool,
    underlays: Vec<Overlay<'a>>,
    overlays: Vec<Overlay<'a>>,
}

impl<'a> VolumeChart<'a> {
    /// Creates a chart that draws `series` with default axes and padding.
    pub fn new(series: VolumeSeries<'a>) -> Self {
        Self {
            block: None,
            series,
            base: Style::new(),
            pad_frac: 0.05,
            value_axis: ValueAxis::new(),
            time_axis: TimeAxis::new(),
            show_axes: true,
            underlays: Vec::new(),
            overlays: Vec::new(),
        }
    }

    /// Sets the base style. Its background is the color partial cells blend
    /// against, so set it to your terminal background for a crisp bar top edge.
    #[must_use]
    pub fn style(mut self, style: impl Into<Style>) -> Self {
        self.base = style.into();
        self
    }

    /// Sets the headroom above the tallest bar as a fraction of the volume span.
    /// The baseline stays at zero.
    #[must_use]
    pub fn padding(mut self, pad_frac: f64) -> Self {
        self.pad_frac = pad_frac;
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

    /// Adds an [`Overlay`] drawn on top of the bars.
    ///
    /// Overlays are drawn in the order they are added, after the series and
    /// before the axes. By default they raise the top of the value axis so they
    /// stay in view, while the baseline stays at zero.
    #[must_use]
    pub fn overlay(mut self, overlay: impl Into<Overlay<'a>>) -> Self {
        self.overlays.push(overlay.into());
        self
    }

    /// Adds several [`Overlay`]s, drawn in order on top of the bars.
    #[must_use]
    pub fn overlays(mut self, overlays: impl IntoIterator<Item = Overlay<'a>>) -> Self {
        self.overlays.extend(overlays);
        self
    }

    /// Adds an [`Overlay`] drawn behind the bars, so the bars occlude it.
    ///
    /// Underlays are drawn in the order they are added, before the series. By
    /// default they raise the top of the value axis to stay in view, while the
    /// baseline stays at zero.
    #[must_use]
    pub fn underlay(mut self, underlay: impl Into<Overlay<'a>>) -> Self {
        self.underlays.push(underlay.into());
        self
    }

    /// Adds several [`Overlay`]s, drawn in order behind the bars.
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
        self.series.draw(buf, &layout);
        overlay::draw_all(&self.overlays, buf, &layout);

        if self.show_axes {
            axis::draw_value_axis(
                buf,
                layout.plot,
                &layout.value,
                &self.value_axis,
                &|v, step| axis::format_volume(v, step),
            );
            axis::draw_time_axis(buf, layout.plot, &layout.time, &self.time_axis);
        }
    }

    /// Computes the plot rectangle and the value and time scales for `area`.
    ///
    /// The value scale is anchored at zero so bars rise from the baseline, with
    /// `pad_frac` of headroom above the tallest bar. A pinned range on the value
    /// axis overrides this, baseline and all. Returns `None` when no plot fits or
    /// there is nothing to draw.
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
                let (_, hi) = self.series.value_bounds()?;
                // Overlays may raise the top, but the baseline stays at zero.
                let hi = overlay::union_bounds((0.0, hi), &self.underlays).1;
                let hi = overlay::union_bounds((0.0, hi), &self.overlays).1;
                ValueScale::new(0.0, hi * (1.0 + self.pad_frac), plot.height)
            }
        };
        let time = self.series.time_scale(plot);

        Some(PlotLayout {
            plot,
            value,
            time,
            bg,
        })
    }
}

impl Widget for &VolumeChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_chart(area, buf);
    }
}

impl Widget for VolumeChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_chart(area, buf);
    }
}

impl<'a> Styled for VolumeChart<'a> {
    type Item = VolumeChart<'a>;

    fn style(&self) -> Style {
        self.base
    }

    fn set_style<S: Into<Style>>(mut self, style: S) -> Self::Item {
        self.base = style.into();
        self
    }
}
