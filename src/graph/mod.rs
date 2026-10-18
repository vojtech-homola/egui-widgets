//! A 2D graph for datasets with millions of samples per series.
//!
//! - [`GraphData`] holds a prepared, immutable dataset.
//! - [`GraphSeries`] says how to draw one: [`GraphMode`], colors, sizes and reduction.
//! - [`Graph`] is the interactive widget, keeping its view and caches in a [`GraphState`].
//! - [`GraphMeshCache`] is the rendering engine behind it, usable without a [`egui::Ui`].
//!
//! Only what is needed for the current view and resolution is selected from a dataset:
//! lines keep the first, smallest, largest and last sample of each physical pixel column,
//! markers keep one sample per occupied screen cell. Both selections and the resulting meshes
//! are cached, so a frame where nothing changed doesn't touch the samples at all.

mod data;
mod line;
mod mesh;
mod points;
#[cfg(test)]
mod tests;
mod widget;

#[cfg(feature = "bench")]
#[doc(hidden)]
pub mod bench;

use egui::{Color32, Id, Rect, Stroke};

pub use data::{DataBounds, GraphData};
pub use mesh::{GraphMeshCache, GraphStats, PreparedGraph};
pub use widget::{Graph, GraphResponse, GraphState};

// GraphError -----------------------------------------------------------------
/// Why a dataset, view or set of series was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphError {
    /// The X value of sample `index` is NaN or infinite.
    NonFiniteX {
        /// Index of the sample.
        index: usize,
    },
    /// The X value of sample `index` is smaller than the one before it.
    DescendingX {
        /// Index of the sample.
        index: usize,
    },
    /// The step of uniform data is not finite and positive.
    InvalidStep(f64),
    /// Datasets are limited to `u32::MAX - 1` samples.
    TooManySamples(usize),
    /// Bounds must be finite with a finite, positive span in both dimensions.
    InvalidBounds,
    /// The viewport rectangle or pixels per point are not finite, or the latter is not
    /// positive.
    InvalidViewport,
    /// Two series have the same id.
    DuplicateSeriesId(Id),
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteX { index } => write!(f, "X of sample {index} is not finite"),
            Self::DescendingX { index } => {
                write!(f, "X of sample {index} is smaller than the one before")
            }
            Self::InvalidStep(step) => write!(f, "X step {step} is not finite and positive"),
            Self::TooManySamples(len) => write!(f, "{len} samples are too many"),
            Self::InvalidBounds => f.write_str("bounds are not finite with a positive span"),
            Self::InvalidViewport => f.write_str("viewport is not finite"),
            Self::DuplicateSeriesId(id) => write!(f, "series id {id:?} is used twice"),
        }
    }
}

impl std::error::Error for GraphError {}

// GraphBounds ----------------------------------------------------------------
/// The visible part of the data: finite X and Y ranges with finite, positive spans.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphBounds {
    x: [f64; 2],
    y: [f64; 2],
}

impl GraphBounds {
    /// `x` and `y` are `[min, max]`.
    ///
    /// # Errors
    /// [`GraphError::InvalidBounds`] unless all values are finite and `min < max` with a
    /// finite `max - min`.
    pub fn new(x: [f64; 2], y: [f64; 2]) -> Result<Self, GraphError> {
        let valid = |[min, max]: [f64; 2]| {
            let span = max - min;
            min.is_finite() && max.is_finite() && span.is_finite() && span > 0.0
        };
        if valid(x) && valid(y) {
            Ok(Self { x, y })
        } else {
            Err(GraphError::InvalidBounds)
        }
    }

    /// `[min, max]` of X.
    pub fn x(&self) -> [f64; 2] {
        self.x
    }

    /// `[min, max]` of Y.
    pub fn y(&self) -> [f64; 2] {
        self.y
    }

    /// `max - min` of X.
    pub fn x_span(&self) -> f64 {
        self.x[1] - self.x[0]
    }

    /// `max - min` of Y.
    pub fn y_span(&self) -> f64 {
        self.y[1] - self.y[0]
    }

    /// Bounds showing `data` with 5% padding. A dimension where all values are equal gets a
    /// span around the value. `None` if that doesn't give valid bounds.
    pub fn fit(data: DataBounds) -> Option<Self> {
        fn axis([min, max]: [f64; 2]) -> [f64; 2] {
            let (min, max) = if min < max {
                (min, max)
            } else if min == 0.0 {
                (-1.0, 1.0)
            } else {
                (min - min.abs() * 0.05, max + max.abs() * 0.05)
            };
            let pad = (max - min) * 0.05;
            [min - pad, max + pad]
        }
        Self::new(axis(data.x), axis(data.y)).ok()
    }
}

impl Default for GraphBounds {
    /// `[-1, 1]` in both dimensions.
    fn default() -> Self {
        Self {
            x: [-1.0, 1.0],
            y: [-1.0, 1.0],
        }
    }
}

// GraphViewport --------------------------------------------------------------
/// Where and how a graph is drawn: the data `bounds` are mapped onto `rect`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphViewport {
    /// The visible part of the data.
    pub bounds: GraphBounds,
    /// The plot area in logical points. Y grows upwards in the data, downwards on screen.
    pub rect: Rect,
    /// Physical pixels per logical point.
    pub pixels_per_point: f32,
}

// Series configuration -------------------------------------------------------
/// How a series is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GraphMode {
    /// Samples connected by lines.
    #[default]
    Line,
    /// A marker at each sample.
    Points,
    /// Both. Line and markers are reduced independently.
    LinePoints,
}

impl GraphMode {
    fn has_line(self) -> bool {
        matches!(self, Self::Line | Self::LinePoints)
    }

    fn has_points(self) -> bool {
        matches!(self, Self::Points | Self::LinePoints)
    }
}

/// Which samples of a line are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineReduction {
    /// Draw the first, smallest, largest and last sample of each physical pixel column when
    /// there are more than about four samples per column, otherwise all of them.
    ///
    /// Each kept sample is an original one, and the extremes of every column are kept, so
    /// spikes never disappear.
    #[default]
    Auto,
    /// Draw all samples in view. Slow for millions of samples.
    Exact,
}

/// Which samples get a marker.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum PointReduction {
    /// Like [`Self::ScreenGrid`] with cells of the marker's diameter (at least one pixel), so
    /// markers that would cover each other are skipped. [`Self::Exact`] if the marker color
    /// is translucent, so overlapping markers keep adding up.
    #[default]
    Auto,
    /// A marker for every sample in view. Slow for millions of samples.
    Exact,
    /// Divide the plot into square cells of `cell_size_pixels` physical pixels and draw a
    /// marker only for the earliest sample in each cell.
    ScreenGrid {
        /// Cell size in physical pixels, at least 1.
        cell_size_pixels: f32,
    },
}

/// Color of the line and markers of a new [`GraphSeries`].
pub const DEFAULT_SERIES_COLOR: Color32 = Color32::from_rgb(110, 170, 255);

/// One dataset in a graph and how to draw it.
///
/// Cheap to create every frame: it only borrows the [`GraphData`].
#[derive(Clone, Copy, Debug)]
pub struct GraphSeries<'a> {
    pub(crate) id: Id,
    pub(crate) data: &'a GraphData,
    pub(crate) mode: GraphMode,
    pub(crate) stroke: Stroke,
    pub(crate) marker_color: Color32,
    pub(crate) marker_radius: f32,
    pub(crate) line_reduction: LineReduction,
    pub(crate) point_reduction: PointReduction,
}

impl<'a> GraphSeries<'a> {
    /// `id` must be unique within a graph and stay the same from frame to frame, as the
    /// cached drawing of the series is kept under it.
    pub fn new(id: impl Into<Id>, data: &'a GraphData) -> Self {
        Self {
            id: id.into(),
            data,
            mode: GraphMode::default(),
            stroke: Stroke::new(1.0, DEFAULT_SERIES_COLOR),
            marker_color: DEFAULT_SERIES_COLOR,
            marker_radius: 2.0,
            line_reduction: LineReduction::default(),
            point_reduction: PointReduction::default(),
        }
    }

    /// Draw a line, markers or both. Default: [`GraphMode::Line`].
    #[inline]
    pub fn mode(mut self, mode: GraphMode) -> Self {
        self.mode = mode;
        self
    }

    /// Line width (in points) and color. Default: 1 point.
    #[inline]
    pub fn stroke(mut self, stroke: impl Into<Stroke>) -> Self {
        self.stroke = stroke.into();
        self
    }

    /// Fill color of the markers.
    #[inline]
    pub fn marker_color(mut self, color: Color32) -> Self {
        self.marker_color = color;
        self
    }

    /// Set the line and marker color.
    #[inline]
    pub fn color(mut self, color: Color32) -> Self {
        self.stroke.color = color;
        self.marker_color = color;
        self
    }

    /// Marker radius in points. Default: 2.
    #[inline]
    pub fn marker_radius(mut self, radius: f32) -> Self {
        self.marker_radius = radius;
        self
    }

    /// Default: [`LineReduction::Auto`].
    #[inline]
    pub fn line_reduction(mut self, reduction: LineReduction) -> Self {
        self.line_reduction = reduction;
        self
    }

    /// Default: [`PointReduction::Auto`].
    #[inline]
    pub fn point_reduction(mut self, reduction: PointReduction) -> Self {
        self.point_reduction = reduction;
        self
    }

    /// The series id.
    pub fn id(&self) -> Id {
        self.id
    }

    /// The dataset.
    pub fn data(&self) -> &'a GraphData {
        self.data
    }
}
