//! The coordinate substrate every renderer draws through.
//!
//! A [`ValueScale`] maps a value domain onto the rows of a plot area and a
//! [`TimeScale`] maps candle indices onto columns. Both directions of each map
//! are exposed so a value (or column) can be turned back into the other so that
//! a crosshair or inspection readout can be displayed.
//!
//! The value map works in continuous fractional rows rather than whole rows or
//! a fixed sub-cell grid. A renderer asks where a value lands as a fractional
//! row and quantizes that to whatever vertical resolution its glyphs offer, so
//! the scale stays independent of the character set used to draw.

/// Maps a value domain `[min, max]` onto a plot `height` (in rows), in both
/// directions, at continuous (fractional-row) resolution.
///
/// Row `0.0` is the top of the plot and higher values map to smaller row
/// values, matching screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ValueScale {
    min: f64,
    max: f64,
    height: u16,
}

pub(crate) type PriceScale = ValueScale;

impl ValueScale {
    /// Builds a scale over `[min, max]` for a plot `height` in rows.
    ///
    /// A zero or inverted span is widened to a small non-zero range so a flat
    /// series still renders instead of dividing by zero.
    pub fn new(min: f64, max: f64, height: u16) -> Self {
        let (min, max) = if (max - min).abs() < f64::EPSILON {
            (min - 1.0, max + 1.0)
        } else {
            (min, max)
        };
        Self {
            min,
            max,
            height: height.max(1),
        }
    }

    /// Builds a scale from the series' value bounds, padded by `pad_frac` of
    /// the span on each end (e.g. `0.05` for 5% headroom top and bottom).
    pub fn autoscale(min: f64, max: f64, height: u16, pad_frac: f64) -> Self {
        let pad = (max - min).abs() * pad_frac;
        Self::new(min - pad, max + pad, height)
    }

    /// Lowest value in the domain.
    pub fn min(&self) -> f64 {
        self.min
    }

    /// Highest value in the domain.
    pub fn max(&self) -> f64 {
        self.max
    }

    /// Plot height in rows.
    #[allow(unused)]
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Maps a value to a fractional row measured from the top of the plot,
    /// clamped to the plot. Smaller values are higher on screen.
    pub fn value_to_row_f64(&self, value: f64) -> f64 {
        let frac = (value - self.min) / (self.max - self.min);
        let from_top = (1.0 - frac) * f64::from(self.height);
        from_top.clamp(0.0, f64::from(self.height))
    }

    /// Inverse of [`value_to_row_f64`](Self::value_to_row_f64): turns a fractional
    /// row back into a value. Not clamped, so it round-trips values outside the
    /// domain as well.
    #[allow(unused)] // This will be used for crosshairs in the future
    pub fn row_f64_to_value(&self, row_f: f64) -> f64 {
        let frac = 1.0 - row_f / f64::from(self.height);
        self.min + frac * (self.max - self.min)
    }

    /// Maps a value to its whole terminal row within the plot.
    pub fn value_to_row(&self, value: f64) -> u16 {
        (self.value_to_row_f64(value) as u16).min(self.height - 1)
    }
}

/// Maps candle indices to columns of the plot, in both directions.
///
/// Each candle occupies `candle_width` columns followed by a `gap` of empty
/// columns, both measured in fractional columns so a backend can place candle
/// edges on its own sub-column grid (such as braille's half-columns). The most
/// recent candles that fit are shown, right-aligned, so a growing series scrolls
/// like a real chart instead of overflowing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TimeScale {
    width: u16,
    candle_width: f64,
    gap: f64,
    /// Index of the leftmost visible candle in the full series.
    first_visible: usize,
    /// Number of visible candles.
    visible: usize,
}

impl TimeScale {
    /// Lays out `candle_count` candles into a plot `width`, drawing each
    /// `candle_width` columns wide with `gap` columns between them. Both may be
    /// fractional. A backend quantizes them to its horizontal resolution.
    pub fn new(width: u16, candle_count: usize, candle_width: f64, gap: f64) -> Self {
        let candle_width = candle_width.max(0.0);
        let gap = gap.max(0.0);
        let slot = candle_width + gap;
        let capacity = if slot > 0.0 {
            // Calculate the number of candles that can fit into the plot width.
            // The last candle doesn't need a trailing gap.
            ((f64::from(width) + gap) / slot).floor() as usize
        } else {
            // If the slot is zero, treat every candle as fitting.
            candle_count
        };
        let visible = capacity.min(candle_count);
        let first_visible = candle_count - visible;
        Self {
            width,
            candle_width,
            gap,
            first_visible,
            visible,
        }
    }

    /// Index of the leftmost visible candle in the original series.
    pub fn first_visible(&self) -> usize {
        self.first_visible
    }

    /// How many candles are visible.
    pub fn visible(&self) -> usize {
        self.visible
    }

    /// Width drawn for each candle body, in fractional columns.
    pub fn candle_width(&self) -> f64 {
        self.candle_width
    }

    /// The fractional left edge of the visible candle at `visible_index`
    /// (`0` is the leftmost visible candle), measured from the plot's left.
    pub fn index_to_left(&self, visible_index: usize) -> f64 {
        let slot = self.candle_width + self.gap;
        visible_index as f64 * slot
    }

    /// Center column of the visible candle at `visible_index`, rounded to a
    /// whole column. Used to align items with the candle's center.
    pub fn index_to_center_col(&self, visible_index: usize) -> u16 {
        let center = self.index_to_left(visible_index) + self.candle_width / 2.0;
        center.floor() as u16
    }

    /// Inverse of [`index_to_left`](Self::index_to_left): the visible candle a
    /// column falls in, or `None` for a gap or out-of-range column. A column is
    /// matched when its center lands within a candle's fractional body span.
    #[allow(unused)] // This will be used for crosshairs in the future
    pub fn col_to_index(&self, col: u16) -> Option<usize> {
        if col >= self.width {
            return None;
        }
        let slot = self.candle_width + self.gap;
        if slot <= 0.0 {
            return None;
        }
        let center = f64::from(col) + 0.5;
        let idx = (center / slot).floor() as usize;
        let within = center - idx as f64 * slot;
        if within < self.candle_width && idx < self.visible {
            Some(idx)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_round_trips_through_row_f() {
        let scale = ValueScale::new(100.0, 200.0, 20);
        for value in [100.0, 125.0, 150.0, 199.0] {
            let row = scale.value_to_row_f64(value);
            let back = scale.row_f64_to_value(row);
            assert!((value - back).abs() < 1e-9, "{value} -> {row} -> {back}");
        }
    }

    #[test]
    fn higher_values_map_to_smaller_rows() {
        let scale = ValueScale::new(0.0, 100.0, 10);
        assert!(scale.value_to_row_f64(90.0) < scale.value_to_row_f64(10.0));
        assert_eq!(scale.value_to_row(100.0), 0);
        assert_eq!(scale.value_to_row(0.0), 9);
    }

    #[test]
    fn flat_series_does_not_divide_by_zero() {
        let scale = ValueScale::new(50.0, 50.0, 10);
        let row = scale.value_to_row_f64(50.0);
        assert!(row.is_finite());
    }

    #[test]
    fn column_round_trips_through_index() {
        let time = TimeScale::new(40, 8, 3.0, 1.0);
        for vi in 0..time.visible() {
            let col = time.index_to_left(vi) as u16;
            assert_eq!(time.col_to_index(col), Some(vi));
        }
        // A gap column maps to no candle.
        let gap_col = time.index_to_left(0) as u16 + 3; // just past a 3-wide body
        assert_eq!(time.col_to_index(gap_col), None);
    }

    #[test]
    fn fractional_candles_tile_to_exact_sub_column_boundaries() {
        // A 1.5-column body with a 0.5-column gap is a 2.0-column slot, so each
        // candle's left edge lands on a whole column and braille's half-columns.
        let time = TimeScale::new(40, 8, 1.5, 0.5);
        assert_eq!(time.index_to_left(0), 0.0);
        assert_eq!(time.index_to_left(1), 2.0);
        assert_eq!(time.index_to_left(2), 4.0);
        assert_eq!(time.candle_width(), 1.5);
    }

    #[test]
    fn shows_most_recent_candles_when_space_is_tight() {
        // Room for only a few candles out of many: the latest are kept.
        let time = TimeScale::new(12, 100, 3.0, 1.0);
        assert!(time.visible() < 100);
        assert_eq!(time.first_visible() + time.visible(), 100);
    }

    #[test]
    fn center_column_lands_on_the_candle() {
        // Width-1 candles tile every other column; the center is the candle's own
        // column, not the empty gap to its right.
        let time = TimeScale::new(20, 5, 1.0, 1.0);
        assert_eq!(time.index_to_center_col(0), 0);
        assert_eq!(time.index_to_center_col(1), 2);
        assert_eq!(time.index_to_center_col(2), 4);

        // Width-3 candles: the center is the middle column of the body, where the
        // wick is drawn.
        let time = TimeScale::new(20, 5, 3.0, 1.0);
        assert_eq!(time.index_to_center_col(0), 1); // body columns 0, 1, 2
        assert_eq!(time.index_to_center_col(1), 5); // body columns 4, 5, 6
    }
}
