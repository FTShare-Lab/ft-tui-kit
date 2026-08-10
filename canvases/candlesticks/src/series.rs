//! Data the charts render.
//!
//! Input is plain values. Chandelier does not fetch, compute, or persist
//! anything. Callers pass already-computed OHLC and volume data.

mod candle;
mod line;
mod volume;

pub use candle::{Candle, CandleSeries, price_bounds};
pub use line::LineSeries;
pub use volume::{Volume, VolumeSeries};

/// Which way a period closed: up, down, or unchanged.
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Direction {
    /// Closed above the open.
    Up,
    /// Closed below the open.
    Down,
    /// Closed level with the open.
    #[default]
    Flat,
}
