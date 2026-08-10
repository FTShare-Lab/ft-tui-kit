//! The candlestick chart widget.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Style, Styled};
use ratatui_core::widgets::Widget;
use ratatui_widgets::block::{Block, BlockExt};

use crate::axis::{self, PriceAxis, TimeAxis};
use crate::overlay::{self, Overlay};
use crate::render::{PlotLayout, Series};
use crate::scale::PriceScale;
use crate::series::CandleSeries;

/// A candlestick chart: a [`CandleSeries`] drawn with a [`PriceAxis`] and a
/// [`TimeAxis`].
///
/// The chart autoscales the price axis to the data in view and draws the most
/// recent candles that fit, right-aligned.
#[derive(Debug, Clone)]
pub struct CandlestickChart<'a> {
    block: Option<Block<'a>>,
    series: CandleSeries<'a>,
    base: Style,
    pad_frac: f64,
    price_axis: PriceAxis<'a>,
    time_axis: TimeAxis<'a>,
    show_axes: bool,
    underlays: Vec<Overlay<'a>>,
    overlays: Vec<Overlay<'a>>,
}

impl<'a> CandlestickChart<'a> {
    /// Creates a chart that draws `series` with default axes and padding.
    pub fn new(series: CandleSeries<'a>) -> Self {
        Self {
            block: None,
            series,
            base: Style::new(),
            pad_frac: 0.05,
            price_axis: PriceAxis::new(),
            time_axis: TimeAxis::new(),
            show_axes: true,
            underlays: Vec::new(),
            overlays: Vec::new(),
        }
    }

    /// Sets the base style. Its background is the color partial cells blend
    /// against, so set it to your terminal background for a crisp body top edge.
    #[must_use]
    pub fn style(mut self, style: impl Into<Style>) -> Self {
        self.base = style.into();
        self
    }

    /// Sets the autoscale padding as a fraction of the price span (per end).
    #[must_use]
    pub fn padding(mut self, pad_frac: f64) -> Self {
        self.pad_frac = pad_frac;
        self
    }

    /// Sets the price (vertical) axis.
    #[must_use]
    pub fn price_axis(mut self, axis: PriceAxis<'a>) -> Self {
        self.price_axis = axis;
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

    /// Adds an [`Overlay`] drawn on top of the candles.
    ///
    /// Overlays are drawn in the order they are added, after the series and
    /// before the axes, and by default expand the price axis so they stay in
    /// view.
    #[must_use]
    pub fn overlay(mut self, overlay: impl Into<Overlay<'a>>) -> Self {
        self.overlays.push(overlay.into());
        self
    }

    /// Adds several [`Overlay`]s, drawn in order on top of the candles.
    #[must_use]
    pub fn overlays(mut self, overlays: impl IntoIterator<Item = Overlay<'a>>) -> Self {
        self.overlays.extend(overlays);
        self
    }

    /// Adds an [`Overlay`] drawn behind the candles, so the candles occlude it.
    ///
    /// Useful for a moving average or band that should sit under the price.
    /// Underlays are drawn in the order they are added, before the series, and
    /// by default expand the price axis so they stay in view.
    #[must_use]
    pub fn underlay(mut self, underlay: impl Into<Overlay<'a>>) -> Self {
        self.underlays.push(underlay.into());
        self
    }

    /// Adds several [`Overlay`]s, drawn in order behind the candles.
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
                &self.price_axis,
                &|v, step| axis::format_price(v, step),
            );
            axis::draw_time_axis(buf, layout.plot, &layout.time, &self.time_axis);
        }
    }

    /// Computes the plot rectangle and the price and time scales for `area`.
    ///
    /// This is the single place the drawn plot geometry is laid out, so the
    /// series, the axes, and anything aligning to the same columns and rows all
    /// share one [`PlotLayout`]. Returns `None` when no plot fits.
    fn layout(&self, area: Rect, bg: Color) -> Option<PlotLayout> {
        let right_axis_w = if self.show_axes {
            self.price_axis.width
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

        let value = match self.price_axis.bounds {
            Some([min, max]) => PriceScale::new(min, max, plot.height),
            None => {
                let (lo, hi) = self.series.value_bounds()?;
                let (lo, hi) = overlay::union_bounds((lo, hi), &self.underlays);
                let (lo, hi) = overlay::union_bounds((lo, hi), &self.overlays);
                PriceScale::autoscale(lo, hi, plot.height, self.pad_frac)
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

impl Widget for &CandlestickChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_chart(area, buf);
    }
}

impl Widget for CandlestickChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_chart(area, buf);
    }
}

impl<'a> Styled for CandlestickChart<'a> {
    type Item = CandlestickChart<'a>;

    fn style(&self) -> Style {
        self.base
    }

    fn set_style<S: Into<Style>>(mut self, style: S) -> Self::Item {
        self.base = style.into();
        self
    }
}
