//! Volume data and the volume series.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use ratatui_core::style::{Color, Style, Styled};

use crate::render::{BarGeometry, PlotLayout, Series, draw_bar};
use crate::scale::TimeScale;

use super::Direction;

/// A single volume measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume {
    /// The amount traded.
    pub quantity: f64,
    /// Which way the period closed, used to color the bar.
    pub direction: Direction,
}

impl Volume {
    /// Creates a new volume measurement with no direction.
    pub fn new(quantity: f64) -> Self {
        Self {
            quantity,
            direction: Direction::Flat,
        }
    }

    /// Sets the direction of the volume measurement.
    #[must_use]
    pub fn with_direction(mut self, direction: Direction) -> Self {
        self.direction = direction;
        self
    }
}

impl From<f64> for Volume {
    fn from(quantity: f64) -> Self {
        Self {
            quantity,
            direction: Direction::Flat,
        }
    }
}

/// A series of trading volumes together with how it is drawn.
///
/// This is the dataset a [`VolumeChart`](crate::VolumeChart) renders, as bars
/// rising from a zero baseline. A bar's position on the x-axis is its index in
/// the slice, matching how a [`CandleSeries`](crate::CandleSeries) is laid out.
///
/// Bars are a single bar [`style`](Self::style) color by default. A bar whose
/// [`Volume`] carries an [`Up`](Direction::Up) or [`Down`](Direction::Down)
/// direction takes the [`bull_style`](Self::bull_style) or
/// [`bear_style`](Self::bear_style) instead.
#[derive(Debug, Clone)]
pub struct VolumeSeries<'a> {
    pub(crate) volumes: &'a [Volume],
    bar: Style,
    bull: Style,
    bear: Style,
    highlighted: Option<&'a [usize]>,
    highlight: Style,
    pub(crate) width: f64,
    pub(crate) gap: f64,
}

impl<'a> VolumeSeries<'a> {
    /// Creates a series over `volumes` drawn as single-color gray bars three
    /// columns wide with a one-column gap.
    pub fn new(volumes: &'a [Volume]) -> Self {
        Self {
            volumes,
            bar: Style::new().fg(Color::Gray),
            bull: Style::new().fg(Color::Green),
            bear: Style::new().fg(Color::Red),
            highlighted: None,
            highlight: Style::new().fg(Color::Yellow),
            width: 3.0,
            gap: 1.0,
        }
    }

    /// Sets the color of bars whose direction is up. Its foreground is the bar
    /// color.
    #[must_use]
    pub fn bull_style(mut self, style: impl Into<Style>) -> Self {
        self.bull = style.into();
        self
    }

    /// Sets the color of bars whose direction is down. Its foreground is the bar
    /// color.
    #[must_use]
    pub fn bear_style(mut self, style: impl Into<Style>) -> Self {
        self.bear = style.into();
        self
    }

    /// Sets bar indices to draw with the highlight style.
    ///
    /// Indices are relative to the slice passed to [`new`](Self::new). The
    /// highlight style's foreground is used as the bar color.
    #[must_use]
    pub fn highlighted(mut self, indices: &'a [usize]) -> Self {
        self.highlighted = Some(indices);
        self
    }

    /// Sets the style for highlighted bars.
    ///
    /// Its foreground is the highlight color. Defaults to yellow.
    #[must_use]
    pub fn highlight_style(mut self, style: impl Into<Style>) -> Self {
        self.highlight = style.into();
        self
    }

    /// Sets the bar width in columns. May be fractional.
    #[must_use]
    pub fn width(mut self, cols: f64) -> Self {
        self.width = cols;
        self
    }

    /// Sets the gap, in columns, between adjacent bars. May be fractional.
    #[must_use]
    pub fn gap(mut self, gap: f64) -> Self {
        self.gap = gap;
        self
    }

    /// The color for a bar, taken from the up or down style by its direction,
    /// otherwise the single bar style.
    pub(crate) fn bar_color(&self, bar: Volume) -> Color {
        let style = match bar.direction {
            Direction::Up => self.bull,
            Direction::Down => self.bear,
            Direction::Flat => self.bar,
        };
        style.fg.unwrap_or(Color::Reset)
    }

    fn is_highlighted(&self, index: usize) -> bool {
        self.highlighted
            .is_some_and(|indices| indices.contains(&index))
    }

    fn highlight_color(&self) -> Color {
        self.highlight.fg.unwrap_or(Color::Yellow)
    }
}

impl<'a> Styled for VolumeSeries<'a> {
    type Item = VolumeSeries<'a>;

    /// The single bar style, used for bars without an
    /// [`Up`](Direction::Up) or [`Down`](Direction::Down) direction.
    fn style(&self) -> Style {
        self.bar
    }

    fn set_style<S: Into<Style>>(mut self, style: S) -> Self::Item {
        self.bar = style.into();
        self
    }
}

impl Series for VolumeSeries<'_> {
    fn value_bounds(&self) -> Option<(f64, f64)> {
        let hi = self.volumes.iter().map(|v| v.quantity).reduce(f64::max)?;

        Some((0.0, hi))
    }

    fn time_scale(&self, plot: Rect) -> TimeScale {
        TimeScale::new(plot.width, self.volumes.len(), self.width, self.gap)
    }

    fn draw(&self, buf: &mut Buffer, layout: &PlotLayout) {
        let plot = layout.plot;
        let scale = layout.value;
        let time = layout.time;
        let bg = layout.bg;

        for vi in 0..time.visible() {
            let oi = time.first_visible() + vi;
            let volume = self.volumes[oi];
            // A zero or negative bar has nothing to draw.
            if volume.quantity <= 0.0 {
                continue;
            }

            let left = time.index_to_left(vi);
            let color = if self.is_highlighted(oi) {
                self.highlight_color()
            } else {
                self.bar_color(volume)
            };
            let geometry = BarGeometry {
                left,
                right: left + time.candle_width(),
                value_row: scale.value_to_row_f64(volume.quantity),
                color,
                bg,
            };
            draw_bar(buf, plot, &geometry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_bar_color_defaults_to_a_single_color() {
        let volumes = [Volume::new(10.0)];
        let series = VolumeSeries::new(&volumes);
        assert_eq!(series.bar_color(Volume::new(10.0)), Color::Gray);

        let series = VolumeSeries::new(&volumes).set_style(Color::Blue);
        assert_eq!(series.bar_color(Volume::new(20.0)), Color::Blue);
    }

    #[test]
    fn volume_bar_color_follows_direction() {
        let volumes = [Volume::new(10.0)];
        let series = VolumeSeries::new(&volumes)
            .bull_style(Color::Green)
            .bear_style(Color::Red)
            .set_style(Color::Blue);

        let up = Volume::new(10.0).with_direction(Direction::Up);
        let down = Volume::new(10.0).with_direction(Direction::Down);
        let flat = Volume::new(10.0).with_direction(Direction::Flat);
        assert_eq!(series.bar_color(up), Color::Green);
        assert_eq!(series.bar_color(down), Color::Red);
        assert_eq!(series.bar_color(flat), Color::Blue);
    }

    #[test]
    fn volume_without_a_direction_uses_the_single_color() {
        let volumes = [Volume::new(10.0)];
        let series = VolumeSeries::new(&volumes)
            .bull_style(Color::Green)
            .set_style(Color::Blue);
        assert_eq!(series.bar_color(Volume::new(30.0)), Color::Blue);
        assert_eq!(series.bar_color(Volume::from(30.0)), Color::Blue);
    }

    #[test]
    fn volume_value_bounds_anchors_at_zero() {
        let volumes = [Volume::new(10.0), Volume::new(45.0), Volume::new(30.0)];
        assert_eq!(
            VolumeSeries::new(&volumes).value_bounds(),
            Some((0.0, 45.0))
        );
    }

    #[test]
    fn volume_value_bounds_is_none_only_when_empty() {
        assert_eq!(VolumeSeries::new(&[]).value_bounds(), None);
        let zeros = [Volume::new(0.0), Volume::new(0.0)];
        assert_eq!(VolumeSeries::new(&zeros).value_bounds(), Some((0.0, 0.0)));
    }
}
