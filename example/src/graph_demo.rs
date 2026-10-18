use eframe::egui::{self, Color32, RichText};
use egui_widgets::graph::GraphStats;
use egui_widgets::{
    Graph, GraphData, GraphMode, GraphSeries, GraphState, LineReduction, PointReduction,
};

/// X range of the generated data.
const X_SPAN: f64 = 1000.0;

#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_SAMPLES: usize = 1_000_000;
#[cfg(target_arch = "wasm32")]
const DEFAULT_SAMPLES: usize = 100_000;

#[derive(Clone, Copy, PartialEq)]
enum Sampling {
    Regular,
    Irregular,
}

#[derive(Clone, Copy, PartialEq)]
enum Signal {
    NoisySine,
    Spikes,
    Gaps,
}

const SIGNALS: [(Signal, &str, Color32); 3] = [
    (
        Signal::NoisySine,
        "noisy sine",
        Color32::from_rgb(110, 170, 255),
    ),
    (
        Signal::Spikes,
        "narrow spikes",
        Color32::from_rgb(255, 150, 80),
    ),
    (
        Signal::Gaps,
        "sine with gaps",
        Color32::from_rgb(120, 210, 120),
    ),
];

#[derive(Clone, Copy, PartialEq)]
enum Points {
    Auto,
    Exact,
    Grid,
}

pub struct GraphDemo {
    state: GraphState,
    samples: usize,
    sampling: Sampling,
    shown: [bool; 3],
    data: [Option<GraphData>; 3],
    seed: u64,

    mode: GraphMode,
    line_reduction: LineReduction,
    points: Points,
    cell_size: f32,
    line_width: f32,
    marker_radius: f32,
    translucent: bool,
    continuous: bool,

    /// Statistics of the last frame.
    stats: GraphStats,
    /// Selection and mesh rebuilds since the start.
    rebuilds: [usize; 3],
    /// Time to generate and prepare the data, and to show the graph.
    #[cfg(not(target_arch = "wasm32"))]
    timings: [Option<std::time::Duration>; 2],
}

impl Default for GraphDemo {
    fn default() -> Self {
        Self {
            state: GraphState::default(),
            samples: DEFAULT_SAMPLES,
            sampling: Sampling::Regular,
            shown: [true, true, true],
            data: [None, None, None],
            seed: 1,
            mode: GraphMode::Line,
            line_reduction: LineReduction::Auto,
            points: Points::Auto,
            cell_size: 4.0,
            line_width: 1.0,
            marker_radius: 2.0,
            translucent: false,
            continuous: false,
            stats: GraphStats::default(),
            rebuilds: [0; 3],
            #[cfg(not(target_arch = "wasm32"))]
            timings: [None; 2],
        }
    }
}

impl GraphDemo {
    pub fn ui(&mut self, ui: &mut egui::Ui, cpu_usage: Option<f32>) {
        let mut stats = GraphStats::default();
        egui::Panel::left("graph_controls")
            .resizable(false)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.controls(ui);
                    ui.separator();
                    self.help(ui);
                    ui.separator();
                    self.stats(ui, cpu_usage);
                });
            });

        self.generate_missing();
        let series: Vec<GraphSeries<'_>> = SIGNALS
            .iter()
            .zip(&self.data)
            .zip(self.shown)
            .filter(|(_, shown)| *shown)
            .filter_map(|(((_, name, color), data), _)| {
                let point_reduction = match self.points {
                    Points::Auto => PointReduction::Auto,
                    Points::Exact => PointReduction::Exact,
                    Points::Grid => PointReduction::ScreenGrid {
                        cell_size_pixels: self.cell_size,
                    },
                };
                let marker_color = if self.translucent {
                    color.gamma_multiply(0.3)
                } else {
                    *color
                };
                Some(
                    GraphSeries::new(*name, data.as_ref()?)
                        .mode(self.mode)
                        .stroke((self.line_width, *color))
                        .marker_color(marker_color)
                        .marker_radius(self.marker_radius)
                        .line_reduction(self.line_reduction)
                        .point_reduction(point_reduction),
                )
            })
            .collect();

        egui::Panel::bottom("graph_stats").show(ui, |ui| {
            // Shown after the graph, with the statistics of this frame.
            ui.horizontal(|ui| {
                if ui.button("Fit to data").clicked() {
                    self.state.fit_to_data(&series);
                }
                ui.label(format!(
                    "x: [{:.6}, {:.6}]  y: [{:.4}, {:.4}]",
                    self.state.bounds().x()[0],
                    self.state.bounds().x()[1],
                    self.state.bounds().y()[0],
                    self.state.bounds().y()[1],
                ));
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            #[cfg(not(target_arch = "wasm32"))]
            let start = std::time::Instant::now();
            let response = Graph::new(&mut self.state)
                .id_salt("demo_graph")
                .series(&series)
                .show(ui);
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.timings[1] = Some(start.elapsed());
            }
            stats = response.stats;
            if let Some(error) = response.error {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        });
        drop(series);

        self.rebuilds[0] += stats.line_selections;
        self.rebuilds[1] += stats.marker_selections;
        self.rebuilds[2] += stats.mesh_rebuilds;
        self.stats = stats;

        if self.continuous {
            ui.ctx().request_repaint();
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Data");
        let mut regenerate = false;
        ui.horizontal(|ui| {
            ui.label("Samples per series");
            egui::ComboBox::from_id_salt("samples")
                .selected_text(format_count(self.samples))
                .show_ui(ui, |ui| {
                    for n in [100_000, 1_000_000, 10_000_000] {
                        regenerate |= ui
                            .selectable_value(&mut self.samples, n, format_count(n))
                            .changed();
                    }
                });
        });
        ui.horizontal(|ui| {
            regenerate |= ui
                .radio_value(&mut self.sampling, Sampling::Regular, "regular X")
                .changed();
            regenerate |= ui
                .radio_value(&mut self.sampling, Sampling::Irregular, "irregular X")
                .changed();
        });
        for (i, (_, name, color)) in SIGNALS.iter().enumerate() {
            ui.checkbox(&mut self.shown[i], RichText::new(*name).color(*color));
        }
        if ui
            .button("Replace data")
            .on_hover_text("Generate new datasets, keeping the view")
            .clicked()
        {
            self.seed += 1;
            regenerate = true;
        }
        if regenerate {
            self.data = [None, None, None];
        }

        ui.add_space(8.0);
        ui.heading("Drawing");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, GraphMode::Line, "line");
            ui.selectable_value(&mut self.mode, GraphMode::Points, "points");
            ui.selectable_value(&mut self.mode, GraphMode::LinePoints, "both");
        });
        ui.horizontal(|ui| {
            ui.label("Line");
            ui.selectable_value(&mut self.line_reduction, LineReduction::Auto, "auto");
            ui.selectable_value(&mut self.line_reduction, LineReduction::Exact, "exact");
        });
        ui.horizontal(|ui| {
            ui.label("Points");
            ui.selectable_value(&mut self.points, Points::Auto, "auto");
            ui.selectable_value(&mut self.points, Points::Exact, "exact");
            ui.selectable_value(&mut self.points, Points::Grid, "grid");
        });
        if self.points == Points::Grid {
            ui.add(egui::Slider::new(&mut self.cell_size, 1.0..=32.0).text("cell px"));
        }
        ui.add(egui::Slider::new(&mut self.line_width, 0.5..=5.0).text("line width"));
        ui.add(egui::Slider::new(&mut self.marker_radius, 0.5..=8.0).text("marker radius"));
        ui.checkbox(&mut self.translucent, "translucent markers");
        let exact = self.line_reduction == LineReduction::Exact || self.points == Points::Exact;
        if exact && self.samples >= 10_000_000 {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Exact drawing of 10M samples is slow.",
            );
        }

        ui.add_space(8.0);
        ui.checkbox(&mut self.continuous, "Repaint continuously")
            .on_hover_text("Measure frames where nothing changed");
    }

    fn help(&self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Mouse").strong());
        ui.label("Left drag: pan");
        ui.label("Right drag: zoom around the press point, X horizontally, Y vertically");
    }

    fn stats(&self, ui: &mut egui::Ui, cpu_usage: Option<f32>) {
        let stats = &self.stats;
        ui.label(RichText::new("Last frame").strong());
        egui::Grid::new("graph_stats_grid")
            .num_columns(2)
            .spacing([16.0, 2.0])
            .show(ui, |ui| {
                let mut row = |name: &str, value: String| {
                    ui.label(name);
                    ui.monospace(value);
                    ui.end_row();
                };
                row("input samples", format_count(stats.input_samples));
                row("line candidates", format_count(stats.line_candidates));
                row("line samples", format_count(stats.line_samples));
                row("marker candidates", format_count(stats.marker_candidates));
                row("markers", format_count(stats.markers));
                row("samples read", format_count(stats.samples_read));
                row("tree nodes visited", format_count(stats.nodes_visited));
                row("vertices", format_count(stats.vertices));
                row("indices", format_count(stats.indices));
                row(
                    "mesh memory",
                    format!("{:.2} MB", stats.mesh_bytes as f64 / 1e6),
                );
                row(
                    "rebuilds (frame)",
                    format!(
                        "{} / {} / {}",
                        stats.line_selections, stats.marker_selections, stats.mesh_rebuilds
                    ),
                );
                row(
                    "rebuilds (total)",
                    format!(
                        "{} / {} / {}",
                        self.rebuilds[0], self.rebuilds[1], self.rebuilds[2]
                    ),
                );
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let ms = |d: Option<std::time::Duration>| {
                        d.map_or("-".to_owned(), |d| {
                            format!("{:.3} ms", d.as_secs_f64() * 1e3)
                        })
                    };
                    row("data preparation", ms(self.timings[0]));
                    row("graph show()", ms(self.timings[1]));
                }
                if let Some(cpu) = cpu_usage {
                    row("frame CPU", format!("{:.2} ms", cpu * 1e3));
                }
            });
        ui.weak("rebuilds: line selections / marker selections / meshes");
    }

    fn generate_missing(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        let start = std::time::Instant::now();
        let mut generated = false;
        for (i, (signal, _, _)) in SIGNALS.iter().enumerate() {
            if self.shown[i] && self.data[i].is_none() {
                let seed = self.seed * 31 + i as u64;
                self.data[i] = Some(generate(*signal, self.samples, self.sampling, seed));
                generated = true;
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if generated {
            self.timings[0] = Some(start.elapsed());
        }
        let _ = generated;
    }
}

fn format_count(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Deterministic pseudo random numbers (xorshift64*).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    /// In `[0, 1)`.
    fn f64(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// In `[-1, 1)`.
    fn noise(&mut self) -> f64 {
        2.0 * self.f64() - 1.0
    }
}

fn generate(signal: Signal, n: usize, sampling: Sampling, seed: u64) -> GraphData {
    let mut rng = Rng::new(seed);
    let step = X_SPAN / n as f64;
    let tau = std::f64::consts::TAU;
    let mut gap_left = 0usize;
    let mut y = |x: f64, rng: &mut Rng| match signal {
        Signal::NoisySine => {
            (tau * x / 100.0).sin() + 0.2 * (tau * x / 0.37).sin() + 0.25 * rng.noise()
        }
        Signal::Spikes => {
            if rng.f64() < 30.0 / n as f64 {
                -3.0 + (2.0 + 3.0 * rng.f64()).copysign(rng.noise())
            } else {
                -3.0 + 0.05 * rng.noise()
            }
        }
        Signal::Gaps => {
            if gap_left == 0 && rng.f64() < 40.0 / n as f64 {
                gap_left = (rng.f64() * n as f64 / 60.0) as usize + 1;
            }
            if gap_left > 0 || rng.f64() < 1e-4 {
                gap_left = gap_left.saturating_sub(1);
                f64::NAN
            } else {
                3.0 + 0.5 * (tau * x / 37.0).sin() + 0.1 * rng.noise()
            }
        }
    };

    match sampling {
        Sampling::Regular => {
            let values = (0..n).map(|i| y(i as f64 * step, &mut rng)).collect();
            GraphData::from_uniform(0.0, step, values).expect("valid uniform data")
        }
        Sampling::Irregular => {
            let mut x = 0.0;
            let points = (0..n)
                .map(|_| {
                    // Exponential steps with the same mean, and some duplicate X values.
                    if rng.f64() > 0.05 {
                        x += -step * (1.0 - rng.f64()).ln();
                    }
                    [x, y(x, &mut rng)]
                })
                .collect();
            GraphData::from_xy(points).expect("valid XY data")
        }
    }
}
