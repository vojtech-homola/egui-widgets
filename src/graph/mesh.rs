//! Projection, clipping, tessellation and the caches behind them.

use std::sync::Arc;

use egui::epaint::tessellator::Path;
use egui::epaint::{Mesh, PathStroke, TessellationOptions};
use egui::{Color32, Id, Painter, Pos2, Rect, Shape, Stroke, pos2};

use super::line::{self, BREAK, LineQuery, Work};
use super::points::{self, PointQuery};
use super::{GraphBounds, GraphData, GraphError, GraphSeries, GraphViewport};
use super::{LineReduction, PointReduction};

// GraphStats -----------------------------------------------------------------
/// What a [`GraphMeshCache::prepare`] call drew and how much work it took.
///
/// The work counters (`samples_read`, `nodes_visited` and the rebuilds) count only the
/// work of that call, so they are zero when nothing changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GraphStats {
    /// Samples in all series.
    pub input_samples: usize,
    /// Samples in the visible X range (with a small guard) of the lines.
    pub line_candidates: usize,
    /// Samples drawn as lines.
    pub line_samples: usize,
    /// Samples in the visible X range (with a small guard) of the markers.
    pub marker_candidates: usize,
    /// Markers drawn.
    pub markers: usize,
    /// Raw samples read during this call.
    pub samples_read: usize,
    /// Extrema tree nodes visited during this call.
    pub nodes_visited: usize,
    /// Vertices of all meshes.
    pub vertices: usize,
    /// Triangle indices of all meshes.
    pub indices: usize,
    /// Memory of all meshes in bytes.
    pub mesh_bytes: usize,
    /// Line selections rebuilt during this call.
    pub line_selections: usize,
    /// Marker selections rebuilt during this call.
    pub marker_selections: usize,
    /// Meshes rebuilt during this call.
    pub mesh_rebuilds: usize,
}

impl GraphStats {
    fn add_mesh(&mut self, mesh: &Mesh) {
        self.vertices += mesh.vertices.len();
        self.indices += mesh.indices.len();
        self.mesh_bytes += std::mem::size_of_val(mesh.vertices.as_slice())
            + std::mem::size_of_val(mesh.indices.as_slice());
    }
}

// Cache entries --------------------------------------------------------------
/// Everything the line mesh depends on except the selection.
#[derive(Clone, Copy, PartialEq)]
struct LineMeshKey {
    bounds: GraphBounds,
    rect: Rect,
    pixels_per_point: f32,
    stroke: Stroke,
    feathering: f32,
}

/// Everything the marker mesh depends on except the selection.
#[derive(Clone, Copy, PartialEq)]
struct MarkerMeshKey {
    bounds: GraphBounds,
    rect: Rect,
    pixels_per_point: f32,
    color: Color32,
    radius: f32,
    feathering: f32,
}

/// A selection of samples and the mesh drawn from it.
struct Layer<Q, M> {
    query: Option<Q>,
    indices: Vec<u32>,
    candidates: usize,
    /// Selected samples, without the [`BREAK`]s.
    selected: usize,
    mesh_key: Option<M>,
    mesh: Arc<Mesh>,
}

impl<Q, M> Default for Layer<Q, M> {
    fn default() -> Self {
        Self {
            query: None,
            indices: Vec::new(),
            candidates: 0,
            selected: 0,
            mesh_key: None,
            mesh: Arc::default(),
        }
    }
}

impl<Q, M> Layer<Q, M> {
    fn clear(&mut self) {
        if self.query.is_some() || self.mesh_key.is_some() {
            *self = Self::default();
        }
    }

    fn invalidate(&mut self) {
        self.query = None;
        self.mesh_key = None;
    }
}

struct SeriesCache {
    id: Id,
    /// Kept so the dataset can't be freed and its address reused while cached.
    data: GraphData,
    line: Layer<LineQuery, LineMeshKey>,
    markers: Layer<PointQuery, MarkerMeshKey>,
}

// GraphMeshCache -------------------------------------------------------------
/// The rendering engine of [`crate::Graph`]: turns series into meshes, and caches them.
///
/// Each call of [`Self::prepare`] compares the series and viewport with the previous call
/// and redoes only what changed. When nothing did, no sample is read and the same meshes are
/// returned.
///
/// One entry is kept per series id, for the series of the last call only.
#[derive(Default)]
pub struct GraphMeshCache {
    entries: Vec<SeriesCache>,
    /// Scratch memory, kept to avoid allocations.
    ids: Vec<u64>,
    grid: Vec<u64>,
    path: Path,
    points: Vec<Pos2>,
    template: Mesh,
    /// The last call had an empty rectangle.
    hidden: bool,
    stats: GraphStats,
}

impl GraphMeshCache {
    /// Update the meshes for `series` drawn in `viewport`, in order: a series' line, then its
    /// markers, then the next series.
    ///
    /// Meshes are built for the whole `viewport.rect`, so a clip rectangle can change without
    /// a rebuild. A rectangle with no area gives no meshes but keeps the caches.
    ///
    /// # Errors
    /// If two series have the same id, or the viewport is not finite. The caches are kept.
    pub fn prepare(
        &mut self,
        series: &[GraphSeries<'_>],
        viewport: GraphViewport,
        options: TessellationOptions,
    ) -> Result<PreparedGraph<'_>, GraphError> {
        self.ids.clear();
        self.ids.extend(series.iter().map(|s| s.id.value()));
        self.ids.sort_unstable();
        if let Some(w) = self.ids.windows(2).find(|w| w[0] == w[1]) {
            let id = series.iter().find(|s| s.id.value() == w[0]).map(|s| s.id);
            return Err(GraphError::DuplicateSeriesId(id.unwrap_or(Id::NULL)));
        }
        let rect = viewport.rect;
        let ppp = viewport.pixels_per_point;
        if !(rect.is_finite() && ppp.is_finite() && ppp > 0.0) {
            return Err(GraphError::InvalidViewport);
        }
        self.stats = GraphStats::default();
        self.hidden = !rect.is_positive();
        if self.hidden {
            return Ok(PreparedGraph {
                entries: &[],
                stats: self.stats,
            });
        }

        // Match the order of the entries to `series`, dropping the ones no longer used.
        for (i, s) in series.iter().enumerate() {
            match self.entries[i..].iter().position(|e| e.id == s.id) {
                Some(j) => self.entries.swap(i, i + j),
                None => self.entries.insert(
                    i,
                    SeriesCache {
                        id: s.id,
                        data: s.data.clone(),
                        line: Layer::default(),
                        markers: Layer::default(),
                    },
                ),
            }
        }
        self.entries.truncate(series.len());

        let feathering_px = if options.feathering {
            options.feathering_size_in_pixels
        } else {
            0.0
        };
        let feathering = feathering_px / ppp;
        let size_px = [
            f64::from(rect.width() * ppp),
            f64::from(rect.height() * ppp),
        ];
        let mut work = Work::default();
        let stats = &mut self.stats;

        for (entry, s) in self.entries.iter_mut().zip(series) {
            if !entry.data.ptr_eq(s.data) {
                entry.data = s.data.clone();
                entry.line.invalidate();
                entry.markers.invalidate();
            }
            let data = &*entry.data.0;
            stats.input_samples += data.len();

            // Line.
            let stroke = s.stroke;
            if s.mode.has_line() && stroke.width > 0.0 && stroke.color != Color32::TRANSPARENT {
                let layer = &mut entry.line;
                let guard_px = f64::from(stroke.width * ppp * 0.5 + feathering_px + 1.0);
                let query = LineQuery {
                    x: viewport.bounds.x(),
                    width_px: size_px[0],
                    guard_px,
                    exact: s.line_reduction == LineReduction::Exact,
                };
                let mut rebuilt = false;
                if layer.query != Some(query) {
                    layer.candidates =
                        line::select_line(data, &query, &mut layer.indices, &mut work);
                    layer.indices.shrink_to(layer.indices.len() * 2);
                    layer.selected = layer.indices.iter().filter(|&&i| i != BREAK).count();
                    layer.query = Some(query);
                    stats.line_selections += 1;
                    rebuilt = true;
                }
                let key = LineMeshKey {
                    bounds: viewport.bounds,
                    rect,
                    pixels_per_point: ppp,
                    stroke,
                    feathering,
                };
                if rebuilt || layer.mesh_key != Some(key) {
                    let mut mesh = Mesh::default();
                    line_mesh(
                        data,
                        &layer.indices,
                        &key,
                        guard_px / f64::from(ppp),
                        &mut self.path,
                        &mut self.points,
                        &mut mesh,
                    );
                    work.reads += layer.indices.len();
                    layer.mesh = Arc::new(mesh);
                    layer.mesh_key = Some(key);
                    stats.mesh_rebuilds += 1;
                }
                stats.line_candidates += layer.candidates;
                stats.line_samples += layer.selected;
                stats.add_mesh(&layer.mesh);
            } else {
                entry.line.clear();
            }

            // Markers.
            let radius = s.marker_radius;
            let color = s.marker_color;
            if s.mode.has_points() && radius > 0.0 && color != Color32::TRANSPARENT {
                let layer = &mut entry.markers;
                let diameter_px = f64::from(2.0 * radius * ppp);
                let cell_px = match s.point_reduction {
                    PointReduction::Auto if color.is_opaque() => Some(diameter_px.max(1.0)),
                    PointReduction::Auto | PointReduction::Exact => None,
                    PointReduction::ScreenGrid { cell_size_pixels } => {
                        Some(f64::from(cell_size_pixels).max(1.0))
                    }
                };
                let guard_px = f64::from(radius * ppp + feathering_px + 1.0);
                let query = PointQuery {
                    bounds: viewport.bounds,
                    size_px,
                    guard_px,
                    cell_px,
                };
                let mut rebuilt = false;
                if layer.query != Some(query) {
                    layer.candidates = points::select_points(
                        data,
                        &query,
                        &mut layer.indices,
                        &mut self.grid,
                        &mut work,
                    );
                    layer.indices.shrink_to(layer.indices.len() * 2);
                    layer.query = Some(query);
                    stats.marker_selections += 1;
                    rebuilt = true;
                }
                let key = MarkerMeshKey {
                    bounds: viewport.bounds,
                    rect,
                    pixels_per_point: ppp,
                    color,
                    radius,
                    feathering,
                };
                if rebuilt || layer.mesh_key != Some(key) {
                    let mut mesh = Mesh::default();
                    marker_mesh(
                        data,
                        &layer.indices,
                        &key,
                        &mut self.path,
                        &mut self.template,
                        &mut mesh,
                    );
                    work.reads += layer.indices.len();
                    layer.mesh = Arc::new(mesh);
                    layer.mesh_key = Some(key);
                    stats.mesh_rebuilds += 1;
                }
                stats.marker_candidates += layer.candidates;
                stats.markers += layer.indices.len();
                stats.add_mesh(&layer.mesh);
            } else {
                entry.markers.clear();
            }
        }
        stats.samples_read = work.reads;
        stats.nodes_visited = work.nodes;

        Ok(PreparedGraph {
            entries: &self.entries,
            stats: self.stats,
        })
    }

    /// The result of the last [`Self::prepare`] call, empty before the first one or after one
    /// that failed or had no area.
    pub fn prepared(&self) -> PreparedGraph<'_> {
        PreparedGraph {
            entries: if self.hidden { &[] } else { &self.entries },
            stats: self.stats,
        }
    }

    /// Drop all cached selections and meshes.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

// PreparedGraph --------------------------------------------------------------
/// The meshes of a [`GraphMeshCache`], borrowed from it.
#[derive(Clone, Copy)]
pub struct PreparedGraph<'a> {
    entries: &'a [SeriesCache],
    stats: GraphStats,
}

impl<'a> PreparedGraph<'a> {
    /// What was drawn and how much work it took.
    pub fn stats(&self) -> GraphStats {
        self.stats
    }

    /// The meshes in drawing order, skipping empty ones.
    pub fn meshes(&self) -> impl Iterator<Item = &'a Arc<Mesh>> + use<'a> {
        self.entries
            .iter()
            .flat_map(|e| [&e.line.mesh, &e.markers.mesh])
            .filter(|m| !m.is_empty())
    }

    /// Add the meshes to `painter`, which should clip to the plot rectangle.
    pub fn paint(&self, painter: &Painter) {
        for mesh in self.meshes() {
            painter.add(Shape::Mesh(Arc::clone(mesh)));
        }
    }
}

// Projection -----------------------------------------------------------------
/// Data to screen coordinates relative to the top left of the plot, in points and `f64`, so
/// coordinates far from zero keep their precision until they are relative to the view.
#[derive(Clone, Copy)]
struct Projection {
    x0: f64,
    y1: f64,
    kx: f64,
    ky: f64,
}

impl Projection {
    fn new(bounds: GraphBounds, rect: Rect) -> Self {
        Self {
            x0: bounds.x()[0],
            y1: bounds.y()[1],
            kx: f64::from(rect.width()) / bounds.x_span(),
            ky: f64::from(rect.height()) / bounds.y_span(),
        }
    }

    /// `None` if the result is not finite, which only happens far outside of the view.
    #[inline]
    fn project(&self, x: f64, y: f64) -> Option<[f64; 2]> {
        let p = [(x - self.x0) * self.kx, (self.y1 - y) * self.ky];
        (p[0].is_finite() && p[1].is_finite()).then_some(p)
    }
}

/// Clip the segment `a → b` to `[min, max]` (Liang–Barsky). Unclipped ends are returned
/// unchanged.
fn clip(a: [f64; 2], b: [f64; 2], min: [f64; 2], max: [f64; 2]) -> Option<[[f64; 2]; 2]> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for axis in 0..2 {
        for (p, q) in [
            (-d[axis], a[axis] - min[axis]),
            (d[axis], max[axis] - a[axis]),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    return None;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
    }
    if t0 > t1 {
        return None;
    }
    let at = |t: f64| [a[0] + t * d[0], a[1] + t * d[1]];
    let start = if t0 > 0.0 { at(t0) } else { a };
    let end = if t1 < 1.0 { at(t1) } else { b };
    (start.iter().chain(&end).all(|v| v.is_finite())).then_some([start, end])
}

/// Stroke the selected samples, separated by [`BREAK`], as lines.
fn line_mesh(
    data: &super::data::Prepared,
    indices: &[u32],
    key: &LineMeshKey,
    pad: f64,
    path: &mut Path,
    points: &mut Vec<Pos2>,
    out: &mut Mesh,
) {
    let stroke = PathStroke::from(key.stroke);
    line_paths(data, indices, key.bounds, key.rect, pad, points, |points| {
        path.clear();
        path.add_open_points(points);
        path.stroke_open(key.feathering, &stroke, out);
    });
}

/// The screen paths of the selected samples, separated by [`BREAK`], clipped to `rect`
/// expanded by `pad` points. Each path given to `emit` has at least two points, all
/// finite, and no two consecutive ones equal. `points` is scratch memory.
fn line_paths(
    data: &super::data::Prepared,
    indices: &[u32],
    bounds: GraphBounds,
    rect: Rect,
    pad: f64,
    points: &mut Vec<Pos2>,
    mut emit: impl FnMut(&[Pos2]),
) {
    let projection = Projection::new(bounds, rect);
    let min = [-pad, -pad];
    let max = [
        f64::from(rect.width()) + pad,
        f64::from(rect.height()) + pad,
    ];
    let origin = rect.min;
    let s = &data.samples;

    let mut flush = |points: &mut Vec<Pos2>| {
        if points.len() >= 2 {
            emit(points);
        }
        points.clear();
    };
    let push = |points: &mut Vec<Pos2>, p: [f64; 2]| {
        let p = origin + egui::vec2(p[0] as f32, p[1] as f32);
        if p.is_finite() && points.last() != Some(&p) {
            points.push(p);
        }
    };

    points.clear();
    // The previous sample, and the end of the path being built if it ends there.
    let mut previous: Option<[f64; 2]> = None;
    let mut path_end: Option<[f64; 2]> = None;
    for &i in indices {
        let p = if i == BREAK {
            None
        } else {
            projection.project(s.x(i as usize), s.y(i as usize))
        };
        let Some(p) = p else {
            flush(points);
            previous = None;
            path_end = None;
            continue;
        };
        if let Some(q) = previous {
            match clip(q, p, min, max) {
                Some([a, b]) => {
                    if path_end != Some(a) {
                        flush(points);
                        push(points, a);
                    }
                    push(points, b);
                    path_end = Some(b);
                }
                None => {
                    flush(points);
                    path_end = None;
                }
            }
        }
        previous = Some(p);
    }
    flush(points);
}

/// A filled circle at each selected sample.
fn marker_mesh(
    data: &super::data::Prepared,
    indices: &[u32],
    key: &MarkerMeshKey,
    path: &mut Path,
    template: &mut Mesh,
    out: &mut Mesh,
) {
    template.clear();
    path.clear();
    path.add_circle(Pos2::ZERO, key.radius);
    path.fill(key.feathering, key.color, template);
    if template.is_empty() {
        return;
    }
    let projection = Projection::new(key.bounds, key.rect);
    let origin = key.rect.min;
    let s = &data.samples;
    out.reserve_vertices(indices.len() * template.vertices.len());
    out.reserve_triangles(indices.len() * template.indices.len() / 3);
    for &i in indices {
        let Some(p) = projection.project(s.x(i as usize), s.y(i as usize)) else {
            continue;
        };
        let center = origin + egui::vec2(p[0] as f32, p[1] as f32);
        if !center.is_finite() {
            continue;
        }
        let base = out.vertices.len() as u32;
        out.vertices.extend(template.vertices.iter().map(|v| {
            let mut v = *v;
            v.pos = pos2(v.pos.x + center.x, v.pos.y + center.y);
            v
        }));
        out.indices
            .extend(template.indices.iter().map(|&index| index + base));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_keeps_inside_segments() {
        let s = clip([1.0, 1.0], [2.0, 3.0], [0.0, 0.0], [10.0, 10.0]);
        assert_eq!(s, Some([[1.0, 1.0], [2.0, 3.0]]));
    }

    #[test]
    fn clip_crossing_segment() {
        let s = clip([-10.0, 5.0], [20.0, 5.0], [0.0, 0.0], [10.0, 10.0]).unwrap();
        assert_eq!(s, [[0.0, 5.0], [10.0, 5.0]]);
        assert!(clip([-10.0, -5.0], [20.0, -5.0], [0.0, 0.0], [10.0, 10.0]).is_none());
        // Diagonal passing outside the corner.
        assert!(clip([-1.0, 0.5], [0.5, -1.0], [0.0, 0.0], [10.0, 10.0]).is_none());
    }
}
