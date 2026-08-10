//! Financial charting widgets for [Ratatui](https://ratatui.rs).
//!
//! # Example
//!
//! A chart is a [`CandleSeries`] (the bars and how they are colored) drawn by a
//! [`CandlestickChart`] (the container that autoscales the price axis, lays out
//! the plot, and draws the axes):
//!
//! ```
//! use candlesticks_chart::{Candle, CandleSeries, CandlestickChart};
//! use ratatui_core::buffer::Buffer;
//! use ratatui_core::layout::Rect;
//! use ratatui_core::widgets::Widget;
//!
//! let candles = [
//!     Candle::new(100.0, 105.0, 99.0, 104.0),
//!     Candle::new(104.0, 108.0, 103.0, 106.0),
//!     Candle::new(106.0, 107.0, 101.0, 102.0),
//! ];
//!
//! let chart = CandlestickChart::new(CandleSeries::new(&candles));
//! ```
//!
//! The `candlesticks` binary provides the complete Canvas renderer.
//!
//! # What it draws
//!
//! - A candlestick chart that autoscales to the data in view.
//! - A volume chart that autoscales to the data in view.
//! - A line chart that plots one or more indicator series, such as RSI or the
//!   lines of a MACD, on a shared autoscaling value axis, with reference levels
//!   drawn as [`TrendLine`] overlays.
//! - Sub-cell body endpoints: open and close levels are placed to the nearest
//!   eighth of a row with partial block characters instead of snapping to whole
//!   rows. These render correctly over any background, including the terminal
//!   default, so setting the chart's base style background is optional.
//! - A right-hand price axis with round-numbered ticks and a bottom time axis,
//!   each styled through its own [`PriceAxis`] and [`TimeAxis`].

mod axis;
mod chart;
mod marker;
mod overlay;
mod render;
mod scale;
mod series;

pub use axis::{PriceAxis, TimeAxis, ValueAxis};
pub use chart::{CandlestickChart, LineChart, VolumeChart};
pub use marker::Marker;
pub use overlay::{
    Anchor, Annotation, Annotations, Label, LineOverlay, LineStyle, Overlay, TrendLine,
};
pub use render::BodyFill;
pub use series::{Candle, CandleSeries, Direction, LineSeries, Volume, VolumeSeries, price_bounds};
