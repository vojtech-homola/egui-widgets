//! Internals exposed for benchmarks and tests. Not part of the public API.

use super::line::{self, LineQuery, Work};
use super::points::{self, PointQuery};
use super::{GraphBounds, GraphData};

/// Work counters of a selection: `[samples read, tree nodes visited]`.
pub type WorkCounts = [usize; 2];

/// Line selection over `x` for a plot `width_px` physical pixels wide, as used by the graph.
pub fn select_line(
    data: &GraphData,
    x: [f64; 2],
    width_px: f64,
    exact: bool,
    out: &mut Vec<u32>,
) -> (usize, WorkCounts) {
    let query = LineQuery {
        x,
        width_px,
        guard_px: 2.0,
        exact,
    };
    let mut work = Work::default();
    let candidates = line::select_line(&data.0, &query, out, &mut work);
    (candidates, [work.reads, work.nodes])
}

/// [`select_line`] by scanning every sample in view.
pub fn select_line_scan(
    data: &GraphData,
    x: [f64; 2],
    width_px: f64,
    exact: bool,
    out: &mut Vec<u32>,
) -> usize {
    let query = LineQuery {
        x,
        width_px,
        guard_px: 2.0,
        exact,
    };
    line::select_line_scan(&data.0, &query, out)
}

/// Marker selection in `bounds` for a plot of `size_px` physical pixels. `cell_px: None`
/// keeps every sample in view.
pub fn select_points(
    data: &GraphData,
    bounds: GraphBounds,
    size_px: [f64; 2],
    cell_px: Option<f64>,
    out: &mut Vec<u32>,
    grid: &mut Vec<u64>,
) -> (usize, WorkCounts) {
    let query = PointQuery {
        bounds,
        size_px,
        guard_px: 3.0,
        cell_px,
    };
    let mut work = Work::default();
    let candidates = points::select_points(&data.0, &query, out, grid, &mut work);
    (candidates, [work.reads, work.nodes])
}

/// [`select_points`] by checking every sample in view in order.
pub fn select_points_scan(
    data: &GraphData,
    bounds: GraphBounds,
    size_px: [f64; 2],
    cell_px: Option<f64>,
    out: &mut Vec<u32>,
    grid: &mut Vec<u64>,
) -> usize {
    let query = PointQuery {
        bounds,
        size_px,
        guard_px: 3.0,
        cell_px,
    };
    points::select_points_scan(&data.0, &query, out, grid)
}

/// Separator of line fragments in [`select_line`] output.
pub const BREAK: u32 = line::BREAK;
