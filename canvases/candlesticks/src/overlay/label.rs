//! A styled text label positioned along a horizontal line.

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::{Alignment, Rect};
use ratatui_core::style::{Color, Modifier, Style};

/// A text label drawn along a horizontal line, such as a
/// [`TrendLine`](crate::TrendLine)'s.
///
/// The label is placed by its [`alignment`](Self::alignment) (against the left
/// edge, centered, or against the right edge), optionally [`inset`](Self::inset)
/// by some columns of line, with a [`padding`](Self::padding) gap separating it
/// from the line. Its style's foreground is the text color, falling back to the
/// line's color when unset.
#[derive(Debug, Clone)]
pub struct Label<'a> {
    text: &'a str,
    style: Style,
    alignment: Alignment,
    inset: u16,
    padding: u16,
}

impl<'a> Label<'a> {
    /// Creates a right-aligned label with one column of padding that inherits
    /// the line's color.
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            style: Style::new(),
            alignment: Alignment::Right,
            inset: 0,
            padding: 1,
        }
    }

    /// Sets the label style. Its foreground is the text color; without one the
    /// label takes the line's color.
    #[must_use]
    pub fn style(mut self, style: impl Into<Style>) -> Self {
        self.style = style.into();
        self
    }

    /// Sets how the label is aligned along the line: against the left edge,
    /// centered, or against the right edge. Defaults to [`Alignment::Right`].
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Sets how many columns of line lead in from the aligned edge before the
    /// label, so it sits inset in the line rather than flush against the edge
    /// (`──RESISTANCE────`). Ignored when the label is centered. Defaults to `0`.
    #[must_use]
    pub fn inset(mut self, inset: u16) -> Self {
        self.inset = inset;
        self
    }

    /// Sets how many blank columns separate the label from the line on each
    /// line-bearing side, so the line breaks cleanly around the text. Defaults
    /// to `1`.
    #[must_use]
    pub fn padding(mut self, padding: u16) -> Self {
        self.padding = padding;
        self
    }

    /// Draws the label along `row` of `plot`, breaking the line around it.
    ///
    /// The text takes its style's foreground, or `line_color` when unset, and is
    /// painted over `bg` (the line's background) so it reads as an opaque mark on
    /// the chart rather than inheriting a candle cell's inversion.
    pub(crate) fn draw_along_row(
        &self,
        buf: &mut Buffer,
        plot: Rect,
        row: u16,
        line_color: Color,
        bg: Color,
    ) {
        let len = self.text.chars().count() as u16;
        let right = plot.x + plot.width;
        let pad = self.padding;
        let inset = self.inset;
        let max_start = right.saturating_sub(len).max(plot.x);

        // `inset` columns of line lead in from the aligned edge before the
        // label. A padding gap separates the label from the line on each side
        // that carries line: the inner side always, and the lead-in (edge) side
        // only when the label is inset. A flush label keeps no gap against its
        // edge. The lead-in columns themselves stay as line.
        let edge_gap = if inset > 0 { pad } else { 0 };
        let (start, gap_start, gap_end) = match self.alignment {
            Alignment::Left => {
                let start = (plot.x + inset + edge_gap).min(max_start);
                (start, plot.x + inset, start + len + pad)
            }
            Alignment::Right => {
                let start = right
                    .saturating_sub(inset)
                    .saturating_sub(edge_gap)
                    .saturating_sub(len)
                    .clamp(plot.x, max_start);
                (
                    start,
                    start.saturating_sub(pad),
                    right.saturating_sub(inset),
                )
            }
            Alignment::Center => {
                let start = (plot.x + plot.width.saturating_sub(len) / 2).min(max_start);
                (start, start.saturating_sub(pad), start + len + pad)
            }
        };

        let y = plot.y + row;
        let fg = self.style.fg.unwrap_or(line_color);
        let text_style = self
            .style
            .fg(fg)
            .bg(self.style.bg.unwrap_or(bg))
            .remove_modifier(Modifier::REVERSED);
        let blank = Style::new().bg(bg).remove_modifier(Modifier::REVERSED);

        for cx in gap_start.max(plot.x)..gap_end.min(right) {
            buf.set_string(cx, y, " ", blank);
        }
        buf.set_string(start, y, self.text, text_style);
    }
}

impl<'a> From<&'a str> for Label<'a> {
    fn from(text: &'a str) -> Self {
        Self::new(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    #[test]
    fn defaults_are_right_aligned_flush_with_one_column_of_padding() {
        let label = Label::new("x");
        assert_eq!(label.alignment, Alignment::Right);
        assert_eq!(label.inset, 0);
        assert_eq!(label.padding, 1);
        assert_eq!(label.style.fg, None);
    }

    #[test]
    fn inherits_the_line_color_without_an_explicit_foreground() {
        let plot = Rect::new(0, 0, 10, 1);
        let mut buf = buffer(10, 1);
        Label::new("AB")
            .alignment(Alignment::Left)
            .padding(0)
            .draw_along_row(&mut buf, plot, 0, Color::Red, Color::Black);

        assert_eq!(buf[(0, 0)].symbol(), "A");
        assert_eq!(buf[(0, 0)].fg, Color::Red);
        assert_eq!(buf[(0, 0)].bg, Color::Black);
    }

    #[test]
    fn an_explicit_foreground_overrides_the_line_color() {
        let plot = Rect::new(0, 0, 10, 1);
        let mut buf = buffer(10, 1);
        Label::new("AB")
            .style(Style::new().fg(Color::Green))
            .alignment(Alignment::Left)
            .padding(0)
            .draw_along_row(&mut buf, plot, 0, Color::Red, Color::Black);

        assert_eq!(buf[(0, 0)].fg, Color::Green);
    }
}
