//! Graph engine benchmarks: `cargo bench --bench graph`.
//!
//! Filter with Criterion's arguments, e.g. `cargo bench --bench graph -- line/`.

use std::hint::black_box;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use egui::epaint::{ClippedShape, TessellationOptions, Tessellator};
use egui::{Rect, Shape, pos2, vec2};
use egui_widgets::graph::{GraphMeshCache, GraphViewport, bench};
use egui_widgets::{GraphBounds, GraphData, GraphMode, GraphSeries};

const SIZES: [usize; 3] = [100_000, 1_000_000, 10_000_000];
const WIDTHS: [f32; 3] = [800.0, 1920.0, 3840.0];

/// Deterministic pseudo random numbers in `[-1, 1)` (xorshift64*).
struct Rng(u64);

impl Rng {
    fn noise(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let bits = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11;
        2.0 * bits as f64 / (1u64 << 53) as f64 - 1.0
    }
}

#[derive(Clone, Copy, Debug)]
enum Signal {
    /// White noise: every pixel column spans the full Y range, the worst case for lines.
    Noise,
    /// Smooth sine with noise.
    Sine,
    /// Sine with a gap every ~1000 samples.
    Gaps,
}

fn values(signal: Signal, n: usize) -> Vec<f64> {
    let mut rng = Rng(0x1234_5678);
    (0..n)
        .map(|i| {
            let t = i as f64 / n as f64;
            match signal {
                Signal::Noise => rng.noise(),
                Signal::Sine => (t * 60.0).sin() + 0.1 * rng.noise(),
                Signal::Gaps if i % 1000 < 3 => f64::NAN,
                Signal::Gaps => (t * 60.0).sin(),
            }
        })
        .collect()
}

/// Uniform data over X `[0, 1)`.
fn data(signal: Signal, n: usize) -> GraphData {
    GraphData::from_uniform(0.0, 1.0 / n as f64, values(signal, n)).unwrap()
}

fn viewport(x: [f64; 2], width: f32) -> GraphViewport {
    GraphViewport {
        bounds: GraphBounds::new(x, [-1.5, 1.5]).unwrap(),
        rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 600.0)),
        pixels_per_point: 1.0,
    }
}

fn options() -> TessellationOptions {
    TessellationOptions {
        prerasterized_discs: false,
        ..Default::default()
    }
}

fn preparation(c: &mut Criterion) {
    let mut group = c.benchmark_group("prepare");
    for n in SIZES {
        let ys = values(Signal::Gaps, n);
        group.bench_function(BenchmarkId::new("uniform", n), |b| {
            b.iter_batched(
                || ys.clone(),
                |ys| GraphData::from_uniform(0.0, 1.0, ys).unwrap(),
                BatchSize::LargeInput,
            );
        });
        let xy: Vec<[f64; 2]> = ys.iter().enumerate().map(|(i, &y)| [i as f64, y]).collect();
        group.bench_function(BenchmarkId::new("xy", n), |b| {
            b.iter_batched(
                || xy.clone(),
                |xy| GraphData::from_xy(xy).unwrap(),
                BatchSize::LargeInput,
            );
        });
    }
}

fn queries(c: &mut Criterion) {
    let mut group = c.benchmark_group("query");
    for n in SIZES {
        let d = data(Signal::Noise, n);
        let mut rng = Rng(99);
        let ranges: Vec<_> = (0..1024)
            .map(|_| {
                let a = ((rng.noise() + 1.0) * 0.5 * n as f64) as usize;
                let b = ((rng.noise() + 1.0) * 0.5 * n as f64) as usize;
                a.min(b)..a.max(b)
            })
            .collect();
        group.bench_function(BenchmarkId::new("y_extrema_x1024", n), |b| {
            b.iter(|| {
                for r in &ranges {
                    black_box(d.y_extrema(r.clone()));
                }
            });
        });
        group.bench_function(BenchmarkId::new("index_range", n), |b| {
            b.iter(|| d.index_range(black_box(0.25), black_box(0.75)));
        });
    }
}

fn lines(c: &mut Criterion) {
    let mut group = c.benchmark_group("line");
    let mut out = Vec::new();
    for signal in [Signal::Noise, Signal::Gaps] {
        for n in SIZES {
            let d = data(signal, n);
            for width in WIDTHS {
                let w = f64::from(width);
                for (view, x) in [("full", [0.0, 1.0]), ("zoom1%", [0.5, 0.51])] {
                    let id = format!("{signal:?}/{view}/{width}");
                    group.bench_function(BenchmarkId::new(format!("indexed/{id}"), n), |b| {
                        b.iter(|| bench::select_line(&d, x, w, false, &mut out));
                    });
                    // The scan reads every visible sample: only the full views where it
                    // matters, and not at every width.
                    if width == 1920.0 && view == "full" {
                        group.bench_function(BenchmarkId::new(format!("scan/{id}"), n), |b| {
                            b.iter(|| bench::select_line_scan(&d, x, w, false, &mut out));
                        });
                    }
                }
            }
        }
    }
}

fn markers(c: &mut Criterion) {
    let mut group = c.benchmark_group("markers");
    let (mut out, mut grid) = (Vec::new(), Vec::new());
    for signal in [Signal::Sine, Signal::Noise] {
        for n in SIZES {
            let d = data(signal, n);
            let bounds = GraphBounds::new([0.0, 1.0], [-1.5, 1.5]).unwrap();
            let size = [1920.0, 1080.0];
            group.bench_function(BenchmarkId::new(format!("indexed/{signal:?}"), n), |b| {
                b.iter(|| bench::select_points(&d, bounds, size, Some(4.0), &mut out, &mut grid));
            });
            group.bench_function(BenchmarkId::new(format!("scan/{signal:?}"), n), |b| {
                b.iter(|| {
                    bench::select_points_scan(&d, bounds, size, Some(4.0), &mut out, &mut grid)
                });
            });
        }
    }
}

/// Whole engine calls: selection, projection and tessellation together.
fn engine(c: &mut Criterion) {
    let mut group = c.benchmark_group("engine");
    for n in SIZES {
        let noise = data(Signal::Noise, n);
        let sine = data(Signal::Sine, n);
        for width in WIDTHS {
            let line = [GraphSeries::new("a", &noise)];
            // Pan: new X bounds every call, so the line is selected and tessellated again.
            group.bench_function(BenchmarkId::new(format!("line_pan/{width}"), n), |b| {
                let mut cache = GraphMeshCache::default();
                let mut shift = 0.0;
                b.iter(|| {
                    shift = (shift + 1e-4) % 0.01;
                    let vp = viewport([shift, 0.99 + shift], width);
                    cache.prepare(&line, vp, options()).unwrap().stats()
                });
            });
            // Y only zoom: the line selection is kept, the mesh is rebuilt.
            group.bench_function(BenchmarkId::new(format!("line_y_zoom/{width}"), n), |b| {
                let mut cache = GraphMeshCache::default();
                let mut zoom = 1.0;
                b.iter(|| {
                    zoom = if zoom > 3.0 { 1.0 } else { zoom * 1.01 };
                    let vp = GraphViewport {
                        bounds: GraphBounds::new([0.0, 1.0], [-zoom, zoom]).unwrap(),
                        ..viewport([0.0, 1.0], width)
                    };
                    cache.prepare(&line, vp, options()).unwrap().stats()
                });
            });
            // Unchanged: only metadata comparisons.
            group.bench_function(BenchmarkId::new(format!("unchanged/{width}"), n), |b| {
                let mut cache = GraphMeshCache::default();
                let vp = viewport([0.0, 1.0], width);
                cache.prepare(&line, vp, options()).unwrap();
                b.iter(|| cache.prepare(&line, vp, options()).unwrap().stats());
            });
        }

        // Several series in combined mode while panning.
        let series = [
            GraphSeries::new("noise", &noise).mode(GraphMode::LinePoints),
            GraphSeries::new("sine", &sine).mode(GraphMode::LinePoints),
        ];
        group.bench_function(BenchmarkId::new("line_points_pan_x2/1920", n), |b| {
            let mut cache = GraphMeshCache::default();
            let mut shift = 0.0;
            b.iter(|| {
                shift = (shift + 1e-4) % 0.01;
                let vp = viewport([shift, 0.99 + shift], 1920.0);
                cache.prepare(&series, vp, options()).unwrap().stats()
            });
        });
    }
}

/// What a repaint of cached meshes still costs in egui: batching copies them.
fn repaint(c: &mut Criterion) {
    let mut group = c.benchmark_group("repaint");
    let n = 1_000_000;
    let noise = data(Signal::Noise, n);
    let sine = data(Signal::Sine, n);
    for (name, series) in [
        ("line", GraphSeries::new("a", &noise)),
        (
            "line_points",
            GraphSeries::new("a", &sine).mode(GraphMode::LinePoints),
        ),
    ] {
        let mut cache = GraphMeshCache::default();
        let vp = viewport([0.0, 1.0], 1920.0);
        let prepared = cache.prepare(&[series], vp, options()).unwrap();
        let shapes: Vec<ClippedShape> = prepared
            .meshes()
            .map(|m| ClippedShape {
                clip_rect: vp.rect,
                shape: Shape::Mesh(m.clone()),
            })
            .collect();
        let mut tessellator = Tessellator::new(1.0, options(), [1, 1], Vec::new());
        group.bench_function(BenchmarkId::new(name, n), |b| {
            b.iter(|| tessellator.tessellate_shapes(shapes.clone()));
        });
    }
}

fn config() -> Criterion {
    Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2))
}

criterion_group! {
    name = benches;
    config = config();
    targets = preparation, queries, lines, markers, engine, repaint
}
criterion_main!(benches);
