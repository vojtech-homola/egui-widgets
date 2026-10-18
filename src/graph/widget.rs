//! The interactive widget.

use egui::{
    AsIdSalt, CursorIcon, Event, IdSalt, PointerButton, Pos2, Rect, Response, Sense, StrokeKind,
    Ui, Vec2, WidgetInfo, WidgetType,
};

use super::{GraphBounds, GraphError, GraphMeshCache, GraphSeries, GraphStats, GraphViewport};

/// Scale factor per point of right-button drag, as in PyQtGraph.
const ZOOM_PER_POINT: f64 = 1.02;

/// Smallest span relative to the magnitude of the bounds, well above `f64` precision.
const MIN_RELATIVE_SPAN: f64 = 1e-12;
const MIN_SPAN: f64 = 1e-290;
const MAX_SPAN: f64 = 1e300;

// GraphState -----------------------------------------------------------------
/// A gesture in progress: everything is computed from its start, so the result doesn't
/// depend on how many frames it takes.
#[derive(Clone, Copy, Debug)]
struct Drag {
    button: PointerButton,
    origin: Pos2,
    bounds: GraphBounds,
    rect: Rect,
}

/// The view of a [`Graph`] and its caches. Keep it in the application, one per graph.
pub struct GraphState {
    bounds: GraphBounds,
    /// Fit to the first data shown.
    fit_pending: bool,
    drag: Option<Drag>,
    cache: GraphMeshCache,
}

impl Default for GraphState {
    /// Fits the view to the first series with drawable data shown, `[-1, 1]` until then.
    fn default() -> Self {
        Self {
            bounds: GraphBounds::default(),
            fit_pending: true,
            drag: None,
            cache: GraphMeshCache::default(),
        }
    }
}

impl GraphState {
    /// Start with `bounds`, without fitting to the data.
    pub fn new(bounds: GraphBounds) -> Self {
        Self {
            bounds,
            fit_pending: false,
            ..Self::default()
        }
    }

    /// The visible part of the data.
    pub fn bounds(&self) -> GraphBounds {
        self.bounds
    }

    /// Show `bounds`. Cancels a drag in progress.
    pub fn set_bounds(&mut self, bounds: GraphBounds) {
        self.bounds = bounds;
        self.fit_pending = false;
        self.drag = None;
    }

    /// Show all drawable samples of `series`, with 5% padding. Returns `false` and keeps the
    /// bounds if there are none.
    pub fn fit_to_data(&mut self, series: &[GraphSeries<'_>]) -> bool {
        let fitted = series
            .iter()
            .filter_map(|s| s.data.bounds())
            .reduce(|a, b| super::DataBounds {
                x: [a.x[0].min(b.x[0]), a.x[1].max(b.x[1])],
                y: [a.y[0].min(b.y[0]), a.y[1].max(b.y[1])],
            })
            .and_then(GraphBounds::fit);
        if let Some(bounds) = fitted {
            self.set_bounds(bounds);
        }
        fitted.is_some()
    }

    /// Whether a pan or zoom gesture is in progress.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The rendering engine with the meshes of the last frame.
    pub fn cache(&self) -> &GraphMeshCache {
        &self.cache
    }

    /// Apply the pointer to the bounds. Returns whether they changed.
    fn interact(&mut self, ui: &Ui, response: &Response, rect: Rect) -> bool {
        let (pos, origin, focus_lost) = ui.input(|i| {
            let focus_lost = i
                .events
                .iter()
                .any(|e| matches!(e, Event::WindowFocused(false)));
            (i.pointer.latest_pos(), i.pointer.press_origin(), focus_lost)
        });
        if let Some(drag) = self.drag
            && (!ui.is_enabled() || focus_lost || drag.rect != rect)
        {
            self.drag = None;
        }
        if self.drag.is_none()
            && ui.is_enabled()
            && let Some(origin) = origin
            && let Some(button) = [PointerButton::Primary, PointerButton::Secondary]
                .into_iter()
                .find(|&b| response.drag_started_by(b))
        {
            self.drag = Some(Drag {
                button,
                origin,
                bounds: self.bounds,
                rect,
            });
        }
        let Some(drag) = self.drag else {
            return false;
        };

        let old = self.bounds;
        if let Some(pos) = pos {
            let delta = pos - drag.origin;
            let new = match drag.button {
                PointerButton::Primary => pan(drag.bounds, delta, rect.size()),
                _ => zoom(drag.bounds, drag.origin - rect.min, delta, rect.size()),
            };
            if let Some(new) = new {
                self.bounds = new;
            }
        }
        let held = ui.input(|i| i.pointer.button_down(drag.button));
        if !held || !(response.dragged() || response.drag_stopped()) {
            self.drag = None;
        } else {
            ui.ctx().set_cursor_icon(match drag.button {
                PointerButton::Primary => CursorIcon::Grabbing,
                _ => CursorIcon::AllScroll,
            });
        }
        self.bounds != old
    }
}

/// `bounds` moved so the data follows a pointer moved by `delta` over a plot of `size`.
fn pan(bounds: GraphBounds, delta: Vec2, size: Vec2) -> Option<GraphBounds> {
    let dx = -f64::from(delta.x) * bounds.x_span() / f64::from(size.x);
    let dy = f64::from(delta.y) * bounds.y_span() / f64::from(size.y);
    let [x0, x1] = bounds.x();
    let [y0, y1] = bounds.y();
    GraphBounds::new([x0 + dx, x1 + dx], [y0 + dy, y1 + dy]).ok()
}

/// `bounds` scaled by a right drag of `delta`, around the data under `anchor` (relative to
/// the top left of a plot of `size`). Dragging right or up zooms in.
fn zoom(bounds: GraphBounds, anchor: Vec2, delta: Vec2, size: Vec2) -> Option<GraphBounds> {
    let [x0, x1] = bounds.x();
    let [y0, y1] = bounds.y();
    let ax = x0 + f64::from(anchor.x / size.x) * bounds.x_span();
    let ay = y1 - f64::from(anchor.y / size.y) * bounds.y_span();
    let sx = ZOOM_PER_POINT.powf(-f64::from(delta.x));
    let sy = ZOOM_PER_POINT.powf(f64::from(delta.y));
    GraphBounds::new(zoom_axis([x0, x1], ax, sx), zoom_axis([y0, y1], ay, sy)).ok()
}

/// `[min, max]` scaled by `scale` around `anchor`, keeping the span representable.
fn zoom_axis([min, max]: [f64; 2], anchor: f64, scale: f64) -> [f64; 2] {
    let span = max - min;
    let magnitude = anchor.abs().max(min.abs()).max(max.abs());
    let min_span = (magnitude * MIN_RELATIVE_SPAN).max(MIN_SPAN);
    let scale = scale.clamp(min_span / span, MAX_SPAN / span);
    [
        anchor + (min - anchor) * scale,
        anchor + (max - anchor) * scale,
    ]
}

// Graph ----------------------------------------------------------------------
/// What [`Graph::show`] did.
pub struct GraphResponse {
    /// `changed()` when the user panned or zoomed.
    pub response: Response,
    /// The bounds after this frame's interaction.
    pub bounds: GraphBounds,
    /// What was drawn and the work it took. Default when nothing was drawn.
    pub stats: GraphStats,
    /// Why nothing was drawn, e.g. two series with the same id.
    pub error: Option<GraphError>,
}

/// An interactive plot of [`GraphSeries`].
///
/// - Drag with the left button to pan.
/// - Drag with the right button to zoom around the point where the drag started: right /
///   left zooms X in / out, up / down zooms Y in / out.
///
/// ```no_run
/// # use egui_widgets::{Graph, GraphData, GraphSeries, GraphState};
/// # fn ui(ui: &mut egui::Ui, state: &mut GraphState, data: &GraphData) {
/// let series = [GraphSeries::new("signal", data)];
/// Graph::new(state).series(&series).show(ui);
/// # }
/// ```
#[must_use = "call `show` to add the graph"]
pub struct Graph<'a> {
    state: &'a mut GraphState,
    series: &'a [GraphSeries<'a>],
    id_salt: Option<IdSalt>,
    desired_size: Option<Vec2>,
}

impl<'a> Graph<'a> {
    /// A graph showing the view of `state`.
    pub fn new(state: &'a mut GraphState) -> Self {
        Self {
            state,
            series: &[],
            id_salt: None,
            desired_size: None,
        }
    }

    /// The series to draw, in order. Their ids must be unique.
    #[inline]
    pub fn series(mut self, series: &'a [GraphSeries<'a>]) -> Self {
        self.series = series;
        self
    }

    /// A stable id, so a drag survives widgets before the graph coming and going.
    #[inline]
    pub fn id_salt(mut self, id_salt: impl AsIdSalt) -> Self {
        self.id_salt = Some(IdSalt::new(id_salt));
        self
    }

    /// Size in points. Default: all available space.
    #[inline]
    pub fn desired_size(mut self, size: impl Into<Vec2>) -> Self {
        self.desired_size = Some(size.into());
        self
    }

    /// Add the graph to `ui`.
    pub fn show(self, ui: &mut Ui) -> GraphResponse {
        let Self {
            state,
            series,
            id_salt,
            desired_size,
        } = self;
        let size = desired_size
            .unwrap_or_else(|| ui.available_size())
            .max(Vec2::ZERO);
        let (auto_id, rect) = ui.allocate_space(size);
        let id = id_salt.map_or(auto_id, |salt| ui.make_persistent_id(salt));
        let mut response = ui.interact(rect, id, Sense::drag());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Other, ui.is_enabled(), "graph"));

        if state.fit_pending {
            state.fit_to_data(series);
        }
        if state.interact(ui, &response, rect) {
            response.mark_changed();
        }

        let mut stats = GraphStats::default();
        let mut error = None;
        if ui.is_rect_visible(rect) {
            let visuals = ui.visuals();
            ui.painter()
                .rect_filled(rect, 0.0, visuals.extreme_bg_color);
            let viewport = GraphViewport {
                bounds: state.bounds,
                rect,
                pixels_per_point: ui.pixels_per_point(),
            };
            let options = ui.ctx().tessellation_options(|o| *o);
            match state.cache.prepare(series, viewport, options) {
                Ok(prepared) => {
                    prepared.paint(&ui.painter_at(rect));
                    stats = prepared.stats();
                }
                Err(e) => error = Some(e),
            }
            ui.painter().rect_stroke(
                rect,
                0.0,
                ui.visuals().widgets.noninteractive.bg_stroke,
                StrokeKind::Inside,
            );
        }

        GraphResponse {
            response,
            bounds: state.bounds,
            stats,
            error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn bounds(x: [f64; 2], y: [f64; 2]) -> GraphBounds {
        GraphBounds::new(x, y).unwrap()
    }

    #[test]
    fn pan_follows_pointer() {
        let b = bounds([0.0, 10.0], [0.0, 100.0]);
        let p = pan(b, vec2(10.0, 10.0), vec2(100.0, 100.0)).unwrap();
        assert_eq!(p.x(), [-1.0, 9.0]);
        assert_eq!(p.y(), [10.0, 110.0]);
    }

    #[test]
    fn zoom_directions_and_anchor() {
        let b = bounds([0.0, 10.0], [0.0, 10.0]);
        let size = vec2(100.0, 100.0);
        // Anchor at data (2.5, 7.5).
        let anchor = vec2(25.0, 25.0);
        let right = zoom(b, anchor, vec2(10.0, 0.0), size).unwrap();
        assert!(right.x_span() < 10.0);
        assert_eq!(right.y(), b.y());
        let s = 1.02_f64.powf(-10.0);
        assert!((right.x()[0] - (2.5 - 2.5 * s)).abs() < 1e-12);
        assert!((right.x()[1] - (2.5 + 7.5 * s)).abs() < 1e-12);

        let down = zoom(b, anchor, vec2(0.0, 10.0), size).unwrap();
        assert!(down.y_span() > 10.0);
        assert_eq!(down.x(), b.x());
        let s = 1.02_f64.powf(10.0);
        assert!((down.y()[0] - (7.5 - 7.5 * s)).abs() < 1e-12);
    }

    #[test]
    fn zoom_is_clamped() {
        let b = bounds([1e6, 1e6 + 1.0], [0.0, 1.0]);
        let z = zoom(b, vec2(50.0, 50.0), vec2(1e5, -1e5), vec2(100.0, 100.0)).unwrap();
        assert!(z.x_span() >= 1e6 * MIN_RELATIVE_SPAN * 0.99);
        assert!(z.y_span() >= MIN_SPAN);
        let z = zoom(b, vec2(50.0, 50.0), vec2(-1e5, 1e5), vec2(100.0, 100.0)).unwrap();
        assert!(z.x_span().is_finite() && z.x_span() <= MAX_SPAN * 1.01);
    }
}
