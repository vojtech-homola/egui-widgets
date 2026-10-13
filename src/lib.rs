//! Reusable widgets for [`egui`].
//!
//! - [`WheelBox`]: numeric input box for integers and floats, stepped with the arrow
//!   keys or the mouse wheel.

#![warn(missing_docs)]

mod wheel_box;

pub use wheel_box::{WheelBox, WheelBoxValue};
