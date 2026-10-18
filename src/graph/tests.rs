//! Tests of the data preparation, reduction and caching.

use std::sync::Arc;

use egui::epaint::TessellationOptions;
use egui::{Color32, Rect, Stroke, pos2};

use super::data::Summary;
use super::line::{self, BREAK, LineQuery, Work};
use super::points::{self, PointQuery};
use super::*;

/// Deterministic pseudo random numbers (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// In `[0, 1)`.
    fn f64(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Test datasets: noise with ties, gaps and spikes, irregular X with duplicates.
fn datasets() -> Vec<GraphData> {
    let mut rng = Rng(7);
    let mut out = Vec::new();

    // Uniform, values from a small set so there are many equal extrema.
    let values = (0..5000).map(|_| rng.below(8) as f64).collect();
    out.push(GraphData::from_uniform(-3.0, 0.01, values).unwrap());

    // Uniform with gaps and spikes.
    let values = (0..20_000)
        .map(|i| match rng.below(100) {
            0..3 => f64::NAN,
            3 => 1000.0 * (rng.f64() - 0.5),
            _ => (i as f64 * 0.01).sin(),
        })
        .collect();
    out.push(GraphData::from_uniform(0.0, 1.0, values).unwrap());

    // Irregular X with duplicates and gaps.
    let mut x = 0.0;
    let points = (0..10_000)
        .map(|_| {
            if rng.below(4) != 0 {
                x += rng.f64() * rng.f64() * 10.0;
            }
            let y = if rng.below(50) == 0 {
                f64::INFINITY
            } else {
                rng.f64()
            };
            [x, y]
        })
        .collect();
    out.push(GraphData::from_xy(points).unwrap());

    // Clustered in X: dense bursts and long empty stretches.
    let mut points = Vec::new();
    for burst in 0..20 {
        let x0 = burst as f64 * 100.0;
        for _ in 0..rng.below(2000) {
            points.push([x0 + rng.f64(), rng.f64() * burst as f64]);
        }
    }
    points.sort_by(|a, b| a[0].total_cmp(&b[0]));
    out.push(GraphData::from_xy(points).unwrap());
    out
}

fn brute_summary(data: &GraphData, range: std::ops::Range<usize>) -> Summary {
    let mut s = Summary::EMPTY;
    for i in range {
        let y = data.y(i);
        if y.is_finite() {
            s = s.then(Summary {
                first: i as u32,
                last: i as u32,
                min: i as u32,
                max: i as u32,
                y_min: y,
                y_max: y,
            });
        }
    }
    s
}

fn brute_runs(data: &GraphData) -> Vec<[u32; 2]> {
    let mut runs = Vec::new();
    let mut start = None;
    for i in 0..data.len() {
        if data.y(i).is_finite() {
            start.get_or_insert(i as u32);
        } else if let Some(s) = start.take() {
            runs.push([s, i as u32]);
        }
    }
    if let Some(s) = start {
        runs.push([s, data.len() as u32]);
    }
    runs
}

// Data -----------------------------------------------------------------------
#[test]
fn empty_and_degenerate_datasets() {
    let empty = GraphData::from_xy(Vec::new()).unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.bounds(), None);
    assert_eq!(empty.finite_runs(), 0);
    assert_eq!(empty.y_extrema(0..0), None);
    assert_eq!(empty.index_range(-1.0, 1.0), 0..0);

    let single = GraphData::from_xy(vec![[2.0, 3.0]]).unwrap();
    let b = single.bounds().unwrap();
    assert_eq!((b.x, b.y), ([2.0, 2.0], [3.0, 3.0]));
    assert_eq!(single.y_extrema(0..1), Some([0, 0]));

    let gaps = GraphData::from_uniform(0.0, 1.0, vec![f64::NAN; 100]).unwrap();
    assert_eq!(gaps.bounds(), None);
    assert_eq!(gaps.finite_runs(), 0);
    assert_eq!(gaps.y_extrema(0..100), None);

    let constant = GraphData::from_uniform(0.0, 1.0, vec![5.0; 1000]).unwrap();
    assert_eq!(constant.y_extrema(10..900), Some([10, 10]));
    let fit = GraphBounds::fit(constant.bounds().unwrap()).unwrap();
    assert!(fit.y()[0] < 5.0 && fit.y()[1] > 5.0);
    let zero = GraphBounds::fit(DataBounds {
        x: [0.0, 0.0],
        y: [0.0, 0.0],
    })
    .unwrap();
    assert!(zero.x()[0] < -1.0 && zero.x()[1] > 1.0);
}

#[test]
fn invalid_datasets_are_rejected() {
    assert_eq!(
        GraphData::from_xy(vec![[0.0, 0.0], [1.0, 0.0], [0.5, 0.0]]).unwrap_err(),
        GraphError::DescendingX { index: 2 }
    );
    assert_eq!(
        GraphData::from_xy(vec![[0.0, 0.0], [f64::NAN, 0.0]]).unwrap_err(),
        GraphError::NonFiniteX { index: 1 }
    );
    assert_eq!(
        GraphData::from_xy(vec![[f64::INFINITY, 0.0]]).unwrap_err(),
        GraphError::NonFiniteX { index: 0 }
    );
    for step in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            GraphData::from_uniform(0.0, step, vec![1.0]),
            Err(GraphError::InvalidStep(_))
        ));
    }
    assert_eq!(
        GraphData::from_uniform(1e308, 1e307, vec![0.0; 20]).unwrap_err(),
        GraphError::NonFiniteX { index: 19 }
    );
    assert!(GraphData::from_uniform(f64::NAN, 1.0, vec![]).is_ok());
}

#[test]
fn uniform_x_and_index_ranges() {
    let data = GraphData::from_uniform(10.0, 0.5, vec![0.0; 11]).unwrap();
    assert_eq!(data.x(0), 10.0);
    assert_eq!(data.x(10), 15.0);
    assert_eq!(data.index_range(11.0, 12.0), 2..5);
    assert_eq!(data.index_range(10.9, 12.1), 2..5);
    assert_eq!(data.index_range(-5.0, 9.0), 0..0);
    assert_eq!(data.index_range(16.0, 20.0), 11..11);
    assert_eq!(data.index_range(12.0, 11.0), 4..4);

    let dup = GraphData::from_xy(vec![
        [0.0, 0.0],
        [1.0, 1.0],
        [1.0, 2.0],
        [1.0, 3.0],
        [2.0, 4.0],
    ])
    .unwrap();
    assert_eq!(dup.index_range(1.0, 1.0), 1..4);
    assert_eq!(dup.as_xy().unwrap().len(), 5);
    assert!(dup.as_uniform().is_none());
}

#[test]
fn runs_and_summaries_match_brute_force() {
    let mut rng = Rng(42);
    for data in datasets() {
        assert_eq!(data.0.runs, brute_runs(&data));
        let n = data.len();
        for _ in 0..2000 {
            let a = rng.below(n + 1);
            let len = match rng.below(3) {
                0 => rng.below(70),
                1 => rng.below(700),
                _ => rng.below(n + 1),
            };
            let b = (a + len).min(n);
            let s = data.0.summary(a..b, &mut 0, &mut 0);
            assert_eq!(s, brute_summary(&data, a..b), "range {a}..{b}");
        }
        assert_eq!(data.0.tree[1], brute_summary(&data, 0..n));
    }
}

#[test]
fn equal_extrema_pick_the_earliest() {
    let data = GraphData::from_uniform(0.0, 1.0, vec![1.0, 3.0, 0.0, 3.0, 0.0, 1.0]).unwrap();
    assert_eq!(data.y_extrema(0..6), Some([2, 1]));
    assert_eq!(data.y_extrema(3..6), Some([4, 3]));
    // Ties across leaves and tree nodes.
    let mut values = vec![1.0; 10_000];
    for i in [70, 71, 700, 701, 5000, 5001, 9999] {
        values[i] = 9.0;
    }
    values[3000] = -1.0;
    values[8000] = -1.0;
    let data = GraphData::from_uniform(0.0, 1.0, values).unwrap();
    assert_eq!(data.y_extrema(0..10_000), Some([3000, 70]));
    assert_eq!(data.y_extrema(72..10_000), Some([3000, 700]));
    assert_eq!(data.y_extrema(5001..10_000), Some([8000, 5001]));
}

// Lines ----------------------------------------------------------------------
fn line_query(x: [f64; 2], width_px: f64, exact: bool) -> LineQuery {
    LineQuery {
        x,
        width_px,
        guard_px: 2.5,
        exact,
    }
}

fn random_view(rng: &mut Rng, data: &GraphData) -> [f64; 2] {
    let b = data.bounds().unwrap();
    let span = b.x[1] - b.x[0];
    let center = b.x[0] - 0.1 * span + rng.f64() * 1.2 * span;
    let half = span * 10f64.powf(-4.0 * rng.f64()) * 0.6;
    [center - half, center + half]
}

/// Checks the invariants of a line selection and returns its fragments.
fn check_line(data: &GraphData, query: &LineQuery, selection: &[u32]) -> Vec<Vec<u32>> {
    let fragments: Vec<Vec<u32>> = selection
        .split(|&i| i == BREAK)
        .filter(|f| !f.is_empty())
        .map(<[u32]>::to_vec)
        .collect();
    assert_eq!(selection.last().copied().unwrap_or(BREAK), BREAK);
    let mut previous = None;
    for f in &fragments {
        for w in f.windows(2) {
            assert!(w[0] < w[1], "not in source order");
            for i in w[0]..=w[1] {
                assert!(data.y(i as usize).is_finite(), "line crosses a gap");
            }
        }
        if let (Some(p), Some(&first)) = (previous, f.first()) {
            assert!(p < first);
        }
        previous = f.last().copied();
    }

    // The extremes of each column of each run are kept.
    let [x0, x1] = query.x;
    let per_x = query.width_px / (x1 - x0);
    let kept: std::collections::HashSet<u32> = selection.iter().copied().collect();
    let range = data.index_range(x0 - query.guard_px / per_x, x1 + query.guard_px / per_x);
    // Including the neighbors outside of the range.
    let range = range.start.saturating_sub(1)..(range.end + 1).min(data.len());
    let mut i = range.start;
    while i < range.end {
        if !data.y(i).is_finite() {
            i += 1;
            continue;
        }
        let c = ((data.x(i) - x0) * per_x).floor();
        let mut j = i;
        while j < range.end && data.y(j).is_finite() && ((data.x(j) - x0) * per_x).floor() == c {
            j += 1;
        }
        let s = brute_summary(data, i..j);
        for k in [s.first, s.last, s.min, s.max] {
            assert!(kept.contains(&k), "sample {k} missing from column {c}");
        }
        i = j;
    }
    fragments
}

#[test]
fn line_reduction_matches_scan_and_keeps_extrema() {
    let mut rng = Rng(3);
    let (mut indexed, mut scanned) = (Vec::new(), Vec::new());
    for data in datasets() {
        for round in 0..300 {
            let width = [1.0, 7.0, 100.0, 800.0, 1920.0][round % 5];
            let exact = round % 7 == 0;
            let query = line_query(random_view(&mut rng, &data), width, exact);
            let mut work = Work::default();
            let a = line::select_line(&data.0, &query, &mut indexed, &mut work);
            let b = line::select_line_scan(&data.0, &query, &mut scanned);
            assert_eq!(a, b);
            assert_eq!(indexed, scanned, "query {query:?}");
            check_line(&data, &query, &indexed);

            let runs = data.0.runs_in(0..data.len()).len();
            let columns = (width + 2.0 * query.guard_px).ceil() as usize + 2;
            let selected = indexed.iter().filter(|&&i| i != BREAK).count();
            if !exact {
                assert!(selected <= 4 * columns + 4 * runs + 2);
            }
        }
    }
}

#[test]
fn line_reduction_bound_for_one_run() {
    let n: usize = 1_000_000;
    let values = (0..n).map(|i| ((i * 7919) % 1000) as f64).collect();
    let data = GraphData::from_uniform(0.0, 1.0, values).unwrap();
    let query = line_query([0.0, n as f64], 800.0, false);
    let mut out = Vec::new();
    let mut work = Work::default();
    line::select_line(&data.0, &query, &mut out, &mut work);
    let selected = out.iter().filter(|&&i| i != BREAK).count();
    assert!(selected <= 4 * (800 + 5 + 2) + 2, "{selected}");
    // Reads depend on the width, not on the number of samples.
    assert!(work.reads < 807 * 160, "{}", work.reads);
    check_line(&data, &query, &out);
}

#[test]
fn line_keeps_neighbors_outside_of_view() {
    // No sample in view, but the segment crosses it.
    let data = GraphData::from_xy(vec![[0.0, 0.0], [10.0, 10.0]]).unwrap();
    let mut out = Vec::new();
    let query = line_query([4.0, 6.0], 100.0, false);
    assert_eq!(
        line::select_line(&data.0, &query, &mut out, &mut Work::default()),
        0
    );
    assert_eq!(out, [0, 1, BREAK]);

    // A gap right outside of the view: nothing to connect to.
    let data = GraphData::from_xy(vec![[0.0, 0.0], [1.0, f64::NAN], [10.0, 10.0]]).unwrap();
    line::select_line(&data.0, &query, &mut out, &mut Work::default());
    assert_eq!(out, [2, BREAK]);
}

#[test]
fn line_keeps_duplicate_x_in_order() {
    let data = GraphData::from_xy(vec![
        [0.0, 0.0],
        [1.0, 5.0],
        [1.0, -5.0],
        [1.0, 2.0],
        [2.0, 0.0],
    ])
    .unwrap();
    let mut out = Vec::new();
    line::select_line(
        &data.0,
        &line_query([0.0, 2.0], 100.0, false),
        &mut out,
        &mut Work::default(),
    );
    assert_eq!(out, [0, 1, 2, 3, 4, BREAK]);
}

#[test]
fn gap_inside_a_pixel_column() {
    let mut values = vec![0.0; 1000];
    values[500] = f64::NAN;
    values[250] = 10.0;
    values[750] = -10.0;
    let data = GraphData::from_uniform(0.0, 1.0, values).unwrap();
    let query = line_query([0.0, 1000.0], 1.0, false);
    let mut out = Vec::new();
    line::select_line(&data.0, &query, &mut out, &mut Work::default());
    let fragments = check_line(&data, &query, &out);
    assert_eq!(fragments.len(), 2);
    assert!(fragments[0].contains(&250) && fragments[0].contains(&499));
    assert!(fragments[1].contains(&501) && fragments[1].contains(&750));
}

// Points ---------------------------------------------------------------------
#[test]
fn point_reduction_matches_scan() {
    let mut rng = Rng(11);
    let (mut indexed, mut scanned, mut grid) = (Vec::new(), Vec::new(), Vec::new());
    for data in datasets() {
        let b = data.bounds().unwrap();
        for round in 0..200 {
            let x = random_view(&mut rng, &data);
            let y_span = (b.y[1] - b.y[0]).max(1.0);
            let yc = b.y[0] + rng.f64() * y_span;
            let half = y_span * 10f64.powf(-2.0 * rng.f64());
            let bounds = GraphBounds::new(x, [yc - half, yc + half]).unwrap();
            let query = PointQuery {
                bounds,
                size_px: [[300.0, 801.5, 37.0][round % 3], [200.0, 13.0][round % 2]],
                guard_px: 3.5,
                cell_px: [None, Some(1.0), Some(2.5), Some(8.0), Some(64.0)][round % 5],
            };
            let mut work = Work::default();
            let a = points::select_points(&data.0, &query, &mut indexed, &mut grid, &mut work);
            let b = points::select_points_scan(&data.0, &query, &mut scanned, &mut grid);
            assert_eq!(a, b);
            assert_eq!(indexed, scanned, "query {query:?}");
            assert!(indexed.windows(2).all(|w| w[0] < w[1]));
        }
    }
}

#[test]
fn point_reduction_skips_occupied_cells() {
    let n: usize = 1_000_000;
    let values = (0..n).map(|i| (i as f64 * 1e-4).sin()).collect();
    let data = GraphData::from_uniform(0.0, 1.0, values).unwrap();
    let bounds = GraphBounds::new([0.0, n as f64], [-1.1, 1.1]).unwrap();
    let query = PointQuery {
        bounds,
        size_px: [800.0, 600.0],
        guard_px: 3.0,
        cell_px: Some(4.0),
    };
    let (mut out, mut grid, mut work) = (Vec::new(), Vec::new(), Work::default());
    points::select_points(&data.0, &query, &mut out, &mut grid, &mut work);
    assert!(out.len() < 202 * 152);
    assert!(work.reads < n / 4, "{}", work.reads);
    // One marker per occupied cell.
    let mut cells = std::collections::HashSet::new();
    for &i in &out {
        let c = (((data.x(i as usize)) * 800.0 / n as f64 + 3.0) / 4.0).floor() as i64;
        let r = (((1.1 - data.y(i as usize)) * 600.0 / 2.2 + 3.0) / 4.0).floor() as i64;
        assert!(cells.insert((c, r)));
    }
}

// Cache ----------------------------------------------------------------------
fn viewport(x: [f64; 2], y: [f64; 2]) -> GraphViewport {
    GraphViewport {
        bounds: GraphBounds::new(x, y).unwrap(),
        rect: Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(400.0, 300.0)),
        pixels_per_point: 1.0,
    }
}

fn options() -> TessellationOptions {
    TessellationOptions::default()
}

fn sine(n: usize, phase: f64) -> GraphData {
    let values = (0..n).map(|i| (i as f64 * 0.001 + phase).sin()).collect();
    GraphData::from_uniform(0.0, 1.0, values).unwrap()
}

/// Mesh pointers in drawing order.
fn mesh_ptrs(cache: &GraphMeshCache) -> Vec<*const egui::epaint::Mesh> {
    cache.prepared().meshes().map(Arc::as_ptr).collect()
}

fn rebuilds(s: GraphStats) -> [usize; 3] {
    [s.line_selections, s.marker_selections, s.mesh_rebuilds]
}

#[test]
fn unchanged_prepare_reuses_everything() {
    let data = sine(100_000, 0.0);
    let series = [GraphSeries::new("a", &data).mode(GraphMode::LinePoints)];
    let vp = viewport([0.0, 100_000.0], [-1.5, 1.5]);
    let mut cache = GraphMeshCache::default();
    let first = cache.prepare(&series, vp, options()).unwrap().stats();
    assert_eq!(rebuilds(first), [1, 1, 2]);
    assert!(first.samples_read > 0 && first.vertices > 0 && first.markers > 0);
    let meshes = mesh_ptrs(&cache);
    assert_eq!(meshes.len(), 2);

    let again = cache.prepare(&series, vp, options()).unwrap().stats();
    assert_eq!(rebuilds(again), [0, 0, 0]);
    assert_eq!((again.samples_read, again.nodes_visited), (0, 0));
    assert_eq!(again.vertices, first.vertices);
    assert_eq!(again.line_samples, first.line_samples);
    assert_eq!(again.markers, first.markers);
    assert_eq!(mesh_ptrs(&cache), meshes);
}

#[test]
fn cache_invalidation() {
    let data = sine(50_000, 0.0);
    let base = GraphSeries::new("a", &data).mode(GraphMode::LinePoints);
    let vp = viewport([0.0, 50_000.0], [-1.5, 1.5]);
    let mut cache = GraphMeshCache::default();
    let mut run = |series: GraphSeries<'_>, vp: GraphViewport| {
        rebuilds(cache.prepare(&[series], vp, options()).unwrap().stats())
    };
    run(base, vp);

    // Same length, different dataset.
    let other = sine(50_000, 1.0);
    assert_eq!(
        run(
            GraphSeries {
                data: &other,
                ..base
            },
            vp
        ),
        [1, 1, 2]
    );
    assert_eq!(run(base, vp), [1, 1, 2]);

    // Y only: the line selection is kept.
    assert_eq!(run(base, viewport([0.0, 50_000.0], [-2.0, 1.5])), [0, 1, 2]);
    assert_eq!(run(base, vp), [0, 1, 2]);
    // X: everything.
    assert_eq!(run(base, viewport([1.0, 50_000.0], [-1.5, 1.5])), [1, 1, 2]);
    assert_eq!(run(base, vp), [1, 1, 2]);

    // Moved rectangle: only meshes.
    let moved = GraphViewport {
        rect: vp.rect.translate(egui::vec2(5.0, 7.0)),
        ..vp
    };
    assert_eq!(run(base, moved), [0, 0, 2]);
    assert_eq!(run(base, vp), [0, 0, 2]);
    // Taller: markers and meshes.
    let taller = GraphViewport {
        rect: vp.rect.with_max_y(vp.rect.max.y + 10.0),
        ..vp
    };
    assert_eq!(run(base, taller), [0, 1, 2]);
    assert_eq!(run(base, vp), [0, 1, 2]);

    // Colors: their mesh only.
    assert_eq!(
        run(base.stroke(Stroke::new(1.0, Color32::RED)), vp),
        [0, 0, 1]
    );
    assert_eq!(run(base.marker_color(Color32::RED), vp), [0, 0, 2]);
    assert_eq!(run(base, vp), [0, 0, 1]);
    // Translucent markers turn off the automatic reduction.
    let translucent = Color32::from_rgba_unmultiplied(255, 0, 0, 100);
    assert_eq!(run(base.marker_color(translucent), vp), [0, 1, 1]);
    assert_eq!(run(base, vp), [0, 1, 1]);

    // Line width changes the guard; marker radius the cells.
    assert_eq!(
        run(base.stroke(Stroke::new(5.0, base.stroke.color)), vp),
        [1, 0, 1]
    );
    assert_eq!(run(base, vp), [1, 0, 1]);
    assert_eq!(run(base.marker_radius(4.0), vp), [0, 1, 1]);
    assert_eq!(run(base, vp), [0, 1, 1]);
    // Reduction settings.
    assert_eq!(
        run(base.line_reduction(LineReduction::Exact), vp),
        [1, 0, 1]
    );
    assert_eq!(run(base, vp), [1, 0, 1]);
    let grid = PointReduction::ScreenGrid {
        cell_size_pixels: 10.0,
    };
    assert_eq!(run(base.point_reduction(grid), vp), [0, 1, 1]);
    assert_eq!(run(base, vp), [0, 1, 1]);

    // Pixels per point: everything.
    let hidpi = GraphViewport {
        pixels_per_point: 2.0,
        ..vp
    };
    assert_eq!(run(base, hidpi), [1, 1, 2]);
    assert_eq!(run(base, vp), [1, 1, 2]);

    // Mode: dropping the line clears it, adding it rebuilds it.
    assert_eq!(run(base.mode(GraphMode::Points), vp), [0, 0, 0]);
    assert_eq!(run(base, vp), [1, 0, 1]);
    assert_eq!(run(base, vp), [0, 0, 0]);
}

#[test]
fn series_order_removal_and_duplicates() {
    let (a, b) = (sine(10_000, 0.0), sine(10_000, 2.0));
    let sa = GraphSeries::new("a", &a);
    let sb = GraphSeries::new("b", &b).mode(GraphMode::Points);
    let vp = viewport([0.0, 10_000.0], [-1.5, 1.5]);
    let mut cache = GraphMeshCache::default();
    cache.prepare(&[sa, sb], vp, options()).unwrap();
    let ab = mesh_ptrs(&cache);

    let stats = cache.prepare(&[sb, sa], vp, options()).unwrap().stats();
    assert_eq!(rebuilds(stats), [0, 0, 0]);
    assert_eq!(mesh_ptrs(&cache), [ab[1], ab[0]]);

    // Duplicates are rejected, keeping the caches.
    let err = cache.prepare(&[sa, sb, sa], vp, options()).err();
    assert_eq!(err, Some(GraphError::DuplicateSeriesId(sa.id)));
    let stats = cache.prepare(&[sb, sa], vp, options()).unwrap().stats();
    assert_eq!(rebuilds(stats), [0, 0, 0]);

    // Removed series are dropped: adding it again rebuilds it.
    cache.prepare(&[sa], vp, options()).unwrap();
    assert_eq!(mesh_ptrs(&cache), [ab[0]]);
    let stats = cache.prepare(&[sa, sb], vp, options()).unwrap().stats();
    assert_eq!(rebuilds(stats), [0, 1, 1]);
}

#[test]
fn zero_sized_rect_keeps_caches() {
    let data = sine(10_000, 0.0);
    let series = [GraphSeries::new("a", &data)];
    let vp = viewport([0.0, 10_000.0], [-1.5, 1.5]);
    let mut cache = GraphMeshCache::default();
    cache.prepare(&series, vp, options()).unwrap();
    let empty = GraphViewport {
        rect: Rect::from_min_size(vp.rect.min, egui::vec2(0.0, 300.0)),
        ..vp
    };
    let prepared = cache.prepare(&series, empty, options()).unwrap();
    assert_eq!(prepared.meshes().count(), 0);
    assert_eq!(prepared.stats(), GraphStats::default());
    let stats = cache.prepare(&series, vp, options()).unwrap().stats();
    assert_eq!(rebuilds(stats), [0, 0, 0]);

    let invalid = GraphViewport {
        pixels_per_point: f32::NAN,
        ..vp
    };
    assert_eq!(
        cache.prepare(&series, invalid, options()).err(),
        Some(GraphError::InvalidViewport)
    );
}

fn check_meshes(cache: &GraphMeshCache) {
    for mesh in cache.prepared().meshes() {
        assert!(mesh.is_valid());
        assert!(mesh.vertices.iter().all(|v| v.pos.is_finite()));
        assert_eq!(mesh.indices.len() % 3, 0);
    }
}

#[test]
fn meshes_are_finite_for_extreme_data() {
    let cases = [
        // Timestamp-like X with a tiny visible span.
        (
            GraphData::from_uniform(1.7e9, 1e-6, (0..100_000).map(|i| (i % 7) as f64).collect())
                .unwrap(),
            [1.7e9 + 0.01, 1.7e9 + 0.0101],
            [-1.0, 8.0],
        ),
        // Huge values far outside of the view.
        (
            GraphData::from_xy(vec![
                [-1e300, -1e300],
                [-1.0, 1e300],
                [0.0, 0.0],
                [1.0, -1e300],
                [1e300, 1e300],
            ])
            .unwrap(),
            [-2.0, 2.0],
            [-1.0, 1.0],
        ),
        // Tiny spans.
        (
            GraphData::from_xy(vec![[0.0, 0.0], [1e-300, 1e-300], [2e-300, 0.0]]).unwrap(),
            [0.0, 2e-300],
            [0.0, 1e-300],
        ),
    ];
    for (data, x, y) in cases {
        for mode in [GraphMode::Line, GraphMode::Points, GraphMode::LinePoints] {
            for ppp in [1.0, 1.5, 3.0] {
                let series = [GraphSeries::new("a", &data).mode(mode)];
                let vp = GraphViewport {
                    pixels_per_point: ppp,
                    ..viewport(x, y)
                };
                let mut cache = GraphMeshCache::default();
                cache.prepare(&series, vp, options()).unwrap();
                check_meshes(&cache);
            }
        }
    }
}

#[test]
fn single_sample_draws_no_line() {
    let data = GraphData::from_xy(vec![[0.0, 0.0], [1.0, f64::NAN], [2.0, 0.0]]).unwrap();
    let mut cache = GraphMeshCache::default();
    let series = [GraphSeries::new("a", &data).mode(GraphMode::LinePoints)];
    let stats = cache
        .prepare(&series, viewport([-1.0, 3.0], [-1.0, 1.0]), options())
        .unwrap()
        .stats();
    assert_eq!(stats.line_samples, 2);
    assert_eq!(stats.markers, 2);
    // Only the marker mesh.
    assert_eq!(cache.prepared().meshes().count(), 1);
}

#[test]
fn failed_prepare_publishes_nothing() {
    let data = sine(10_000, 0.0);
    let series = [GraphSeries::new("a", &data)];
    let vp = viewport([0.0, 10_000.0], [-1.5, 1.5]);
    let mut cache = GraphMeshCache::default();
    cache.prepare(&series, vp, options()).unwrap();
    assert_eq!(cache.prepared().meshes().count(), 1);

    let twice = [series[0], series[0]];
    assert!(cache.prepare(&twice, vp, options()).is_err());
    assert_eq!(cache.prepared().meshes().count(), 0);
    assert_eq!(cache.prepared().stats(), GraphStats::default());

    let invalid = GraphViewport {
        pixels_per_point: 0.0,
        ..vp
    };
    cache.prepare(&series, vp, options()).unwrap();
    assert!(cache.prepare(&series, invalid, options()).is_err());
    assert_eq!(cache.prepared().meshes().count(), 0);
    assert_eq!(cache.prepared().stats(), GraphStats::default());

    // The caches survived.
    let stats = cache.prepare(&series, vp, options()).unwrap().stats();
    assert_eq!(rebuilds(stats), [0, 0, 0]);
    assert_eq!(cache.prepared().meshes().count(), 1);
}

#[test]
fn segment_between_distant_samples_is_drawn() {
    let data = GraphData::from_xy(vec![[0.0, -1e20], [1.0, 1e20]]).unwrap();
    let series = [GraphSeries::new("a", &data)];
    let mut cache = GraphMeshCache::default();
    let stats = cache
        .prepare(&series, viewport([0.0, 1.0], [-1.0, 1.0]), options())
        .unwrap()
        .stats();
    assert_eq!(stats.line_samples, 2);
    let mesh = cache.prepared().meshes().next().expect("a line mesh");
    // The line crosses the whole plot (y 20..320).
    let (top, bottom) = mesh
        .vertices
        .iter()
        .fold((f32::MAX, f32::MIN), |(t, b), v| {
            (t.min(v.pos.y), b.max(v.pos.y))
        });
    assert!(top <= 20.0 && bottom >= 320.0, "{top} {bottom}");
    check_meshes(&cache);
}
