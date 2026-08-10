//! Annotations layered on top of a chart's series.
//!
//! An overlay shares the chart's plot geometry through its [`PlotLayout`], so it
//! lands on the same rows and columns as the candles it sits over. Overlays are
//! added to a chart with its `overlay` builder and drawn in the order they are
//! added, after the series and before the axes.

mod annotation;
mod label;
mod line;
mod trend_line;

pub use annotation::{Anchor, Annotation, Annotations};
pub use label::Label;
pub use line::LineOverlay;
pub use trend_line::{LineStyle, TrendLine};

use ratatui_core::buffer::Buffer;

use crate::render::PlotLayout;

/// Annotations or data drawn on top of a chart's series, sharing its plot geometry.
pub(crate) trait OverlayDraw {
    /// The value extent this overlay occupies, or `None` when it should not
    /// affect the chart's value axis. The chart unions this with the series'
    /// bounds when autoscaling, so an overlay stays in view by default.
    fn value_bounds(&self) -> Option<(f64, f64)>;

    /// Draws the overlay into the plot, after the series and before the axes.
    fn draw(&self, buf: &mut Buffer, layout: &PlotLayout);
}

/// An annotation layered over a chart, added with its `overlay` builder.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Overlay<'a> {
    /// A straight line drawn on the value scale.
    Trend(TrendLine<'a>),
    /// A connected line over the candles, such as a moving average.
    Line(LineOverlay<'a>),
    /// Point annotations aligned to the candles.
    Annotations(Annotations<'a>),
}

impl Overlay<'_> {
    pub(crate) fn value_bounds(&self) -> Option<(f64, f64)> {
        match self {
            Overlay::Trend(o) => o.value_bounds(),
            Overlay::Line(o) => o.value_bounds(),
            Overlay::Annotations(o) => o.value_bounds(),
        }
    }

    pub(crate) fn draw(&self, buf: &mut Buffer, layout: &PlotLayout) {
        match self {
            Overlay::Trend(o) => o.draw(buf, layout),
            Overlay::Line(o) => o.draw(buf, layout),
            Overlay::Annotations(o) => o.draw(buf, layout),
        }
    }
}

impl<'a> From<TrendLine<'a>> for Overlay<'a> {
    fn from(line: TrendLine<'a>) -> Self {
        Overlay::Trend(line)
    }
}

impl<'a> From<LineOverlay<'a>> for Overlay<'a> {
    fn from(line: LineOverlay<'a>) -> Self {
        Overlay::Line(line)
    }
}

impl<'a> From<Annotations<'a>> for Overlay<'a> {
    fn from(annotations: Annotations<'a>) -> Self {
        Overlay::Annotations(annotations)
    }
}

/// Unions the value extents of `overlays` into `base`, so the chart autoscales
/// to include any overlay that opts into it.
pub(crate) fn union_bounds(base: (f64, f64), overlays: &[Overlay<'_>]) -> (f64, f64) {
    let (mut lo, mut hi) = base;
    for overlay in overlays {
        if let Some((olo, ohi)) = overlay.value_bounds() {
            lo = lo.min(olo);
            hi = hi.max(ohi);
        }
    }
    (lo, hi)
}

/// Draws every overlay in order, after the series and before the axes.
pub(crate) fn draw_all(overlays: &[Overlay<'_>], buf: &mut Buffer, layout: &PlotLayout) {
    for overlay in overlays {
        overlay.draw(buf, layout);
    }
}
