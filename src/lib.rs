//! Reusable widgets for [`egui`].
//!
//! - [`WheelBox`]: numeric input box for integers and floats, stepped with the arrow
//!   keys or the mouse wheel.
//! - [`Graph`]: 2D plot of lines and markers for datasets with millions of samples, see
//!   [`graph`].

#![warn(missing_docs)]

pub mod graph;
mod wheel_box;

pub use graph::{
    Graph, GraphBounds, GraphData, GraphMode, GraphResponse, GraphSeries, GraphState,
    LineReduction, PointReduction,
};
pub use wheel_box::{WheelBox, WheelBoxValue};
