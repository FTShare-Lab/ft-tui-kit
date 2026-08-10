//! A straight line drawn on the chart's value scale.

use ratatui_core::buffer::Buffer;
use ratatui_core::style::{Color, Modifier, Style};

use crate::overlay::{Label, OverlayDraw};
use crate::render::{self, PlotLayout};

/// Whether a [`TrendLine`] is drawn solid or dashed.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum LineStyle {
    /// A continuous line (`─`).
    #[default]
    Solid,
    /// A broken line (`╌`).
    Dashed,
}

/// The shape a [`TrendLine`] traces on the chart.
#[derive(Debug, Clone)]
enum Geometry {
    /// A horizontal line at a value, spanning the full plot width.
    Level(f64),
}

/// A straight line drawn on the chart's value scale.
#[derive(Debug, Clone)]
pub struct TrendLine<'a> {
    geometry: Geometry,
    style: Style,
    line: LineStyle,
    label: Option<Label<'a>>,
    autoscale: bool,
}

impl<'a> TrendLine<'a> {
    /// Creates a solid gray horizontal line at `value` with no label.
    pub fn at(value: f64) -> Self {
        Self {
            geometry: Geometry::Level(value),
            style: Style::new().fg(Color::Gray),
            line: LineStyle::Solid,
            label: None,
            autoscale: true,
        }
    }

    /// Sets the line style. Its foreground is the line color.
    #[must_use]
    pub fn style(mut self, style: impl Into<Style>) -> Self {
        self.style = style.into();
        self
    }

    /// Sets whether the line is solid or dashed.
    #[must_use]
    pub fn line(mut self, line: LineStyle) -> Self {
        self.line = line;
        self
    }

    /// Draws the line dashed. Shorthand for [`line`](Self::line) with
    /// [`LineStyle::Dashed`].
    #[must_use]
    pub fn dashed(mut self) -> Self {
        self.line = LineStyle::Dashed;
        self
    }

    /// Sets the line's [`Label`]. A `&str` works directly for an unstyled,
    /// right-aligned label; build a [`Label`] for other alignments, an inset, or
    /// an explicit color.
    #[must_use]
    pub fn label(mut self, label: impl Into<Label<'a>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets whether the chart expands its value axis to keep this line in view.
    /// On by default.
    #[must_use]
    pub fn autoscale(mut self, autoscale: bool) -> Self {
        self.autoscale = autoscale;
        self
    }

    /// Draws a horizontal level across the plot, with its label if any.
    fn draw_level(&self, value: f64, buf: &mut Buffer, layout: &PlotLayout) {
        let plot = layout.plot;
        let row = layout.value.value_to_row(value);
        let glyph = match self.line {
            LineStyle::Solid => "─",
            LineStyle::Dashed => "╌",
        };

        // Paint on the chart background so the line reads as an opaque mark over
        // the candles. Clearing REVERSED keeps a candle cell's foreground and
        // background inversion from swapping the colors we set.
        let bg = self.style.bg.unwrap_or(layout.bg);
        let line_style = self.style.bg(bg).remove_modifier(Modifier::REVERSED);

        for col in 0..plot.width {
            render::put(buf, plot, u32::from(col), u32::from(row), glyph, line_style);
        }

        if let Some(label) = &self.label {
            let line_color = self.style.fg.unwrap_or(Color::Reset);
            label.draw_along_row(buf, plot, row, line_color, bg);
        }
    }
}

impl OverlayDraw for TrendLine<'_> {
    fn value_bounds(&self) -> Option<(f64, f64)> {
        if !self.autoscale {
            return None;
        }
        match self.geometry {
            Geometry::Level(value) => Some((value, value)),
        }
    }

    fn draw(&self, buf: &mut Buffer, layout: &PlotLayout) {
        let plot = layout.plot;
        if plot.width == 0 || plot.height == 0 {
            return;
        }

        match self.geometry {
            Geometry::Level(value) => self.draw_level(value, buf, layout),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_bounds_pins_to_the_level_and_honors_autoscale() {
        assert_eq!(TrendLine::at(42.0).value_bounds(), Some((42.0, 42.0)));
        assert_eq!(TrendLine::at(42.0).autoscale(false).value_bounds(), None);
    }

    #[test]
    fn defaults_are_solid_and_unlabeled() {
        let line = TrendLine::at(1.0);
        assert_eq!(line.line, LineStyle::Solid);
        assert!(line.label.is_none());
        assert!(line.autoscale);
    }
}
