use eframe::egui::{self, RichText};
use egui_widgets::WheelBox;

const BOX_WIDTH: f32 = 120.;

pub struct DemoApp {
    int_basic: i32,
    int_range: i32,
    int_u8: u8,
    int_external: i64,
    int_external_step: i64,
    int_no_update: i32,
    int_affixes: i32,
    int_narrow: i64,
    int_long: i64,

    float_basic: f64,
    float_range: f64,
    float_f32: f32,
    float_external: f64,
    float_external_step: f64,
    float_icon: f64,
}

impl DemoApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Box<Self> {
        Box::new(Self {
            int_basic: 0,
            int_range: 50,
            int_u8: 128,
            int_external: 1000,
            int_external_step: 100,
            int_no_update: 0,
            int_affixes: 10,
            int_narrow: 1_234_567,
            int_long: 1_234_567_890_123,

            float_basic: 0.0,
            float_range: 0.5,
            float_f32: 1.0,
            float_external: 10.0,
            float_external_step: 0.5,
            float_icon: 12.5,
        })
    }

    fn integers(&mut self, ui: &mut egui::Ui) {
        ui.heading("Integers");
        ui.add_space(4.);

        egui::Grid::new("integers_grid")
            .num_columns(3)
            .spacing([16., 8.])
            .striped(true)
            .show(ui, |ui| {
                header(ui);

                ui.label("i32, default");
                ui.add(WheelBox::new(&mut self.int_basic).desired_width(BOX_WIDTH));
                ui.monospace(self.int_basic.to_string());
                ui.end_row();

                ui.label("i32, range [0, 100], suffix, step 5");
                ui.add(
                    WheelBox::new(&mut self.int_range)
                        .range([0, 100])
                        .suffix("%")
                        .default_single_step(5)
                        .desired_width(BOX_WIDTH),
                );
                ui.monospace(self.int_range.to_string());
                ui.end_row();

                ui.label("u8, saturating");
                ui.add(
                    WheelBox::new(&mut self.int_u8)
                        .default_single_step(64)
                        .desired_width(BOX_WIDTH),
                );
                ui.monospace(self.int_u8.to_string());
                ui.end_row();

                ui.label("i64, external step");
                ui.add(
                    WheelBox::new(&mut self.int_external)
                        .single_step(&mut self.int_external_step)
                        .default_single_step(100)
                        .desired_width(BOX_WIDTH),
                );
                ui.monospace(format!(
                    "{} (step {})",
                    self.int_external, self.int_external_step
                ));
                ui.end_row();

                ui.label("i32, always_update(false)");
                ui.add(
                    WheelBox::new(&mut self.int_no_update)
                        .always_update(false)
                        .text_color(egui::Color32::LIGHT_BLUE)
                        .desired_width(BOX_WIDTH),
                );
                ui.monospace(self.int_no_update.to_string());
                ui.end_row();

                ui.label("i32, prefix + suffix");
                ui.add(
                    WheelBox::new(&mut self.int_affixes)
                        .prefix("x: ")
                        .suffix("px")
                        .desired_width(BOX_WIDTH),
                );
                ui.monospace(self.int_affixes.to_string());
                ui.end_row();

                ui.label("i64, narrow: suffix hidden");
                ui.add(
                    WheelBox::new(&mut self.int_narrow)
                        .prefix("x: ")
                        .suffix("nm")
                        .desired_width(80.),
                );
                ui.monospace(self.int_narrow.to_string());
                ui.end_row();

                ui.label("i64, very narrow: value truncated");
                ui.add(
                    WheelBox::new(&mut self.int_long)
                        .prefix("x: ")
                        .suffix("nm")
                        .desired_width(60.),
                );
                ui.monospace(self.int_long.to_string());
                ui.end_row();
            });
    }

    fn floats(&mut self, ui: &mut egui::Ui) {
        ui.heading("Floats");
        ui.add_space(4.);

        egui::Grid::new("floats_grid")
            .num_columns(3)
            .spacing([16., 8.])
            .striped(true)
            .show(ui, |ui| {
                header(ui);

                ui.label("f64, 2 decimals");
                ui.add(
                    WheelBox::new(&mut self.float_basic)
                        .decimals(2)
                        .desired_width(BOX_WIDTH),
                );
                ui.monospace(self.float_basic.to_string());
                ui.end_row();

                ui.label("f64, range [0, 1], step 0.1, default decimals");
                ui.add(
                    WheelBox::new(&mut self.float_range)
                        .range([0.0, 1.0])
                        .default_single_step(0.1)
                        .desired_width(BOX_WIDTH),
                );
                ui.monospace(self.float_range.to_string());
                ui.end_row();

                ui.label("f32, weak suffix");
                ui.add(
                    WheelBox::new(&mut self.float_f32)
                        .decimals(1)
                        .suffix(RichText::new("mm").weak())
                        .desired_width(BOX_WIDTH),
                );
                ui.monospace(self.float_f32.to_string());
                ui.end_row();

                ui.label("f64, external step");
                ui.add(
                    WheelBox::new(&mut self.float_external)
                        .decimals(4)
                        .single_step(&mut self.float_external_step)
                        .default_single_step(0.5)
                        .desired_width(BOX_WIDTH),
                );
                ui.monospace(format!(
                    "{} (step {})",
                    self.float_external, self.float_external_step
                ));
                ui.end_row();

                ui.label("f64, icon prefix");
                ui.add(
                    WheelBox::new(&mut self.float_icon)
                        .decimals(1)
                        .prefix("⏱ ")
                        .suffix("ms")
                        .desired_width(BOX_WIDTH),
                );
                ui.monospace(self.float_icon.to_string());
                ui.end_row();
            });

        ui.add_space(8.);
        if ui.button("Reset values from outside").clicked() {
            self.float_basic = 0.0;
            self.float_range = 0.5;
            self.float_f32 = 1.0;
            self.float_external = 10.0;
            self.float_icon = 12.5;
        }
    }
}

impl eframe::App for DemoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("top_panel").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("egui_widgets demo");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::widgets::global_theme_preference_buttons(ui);
                });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                help(ui);
                ui.separator();
                self.integers(ui);
                ui.add_space(16.);
                self.floats(ui);
            });
        });
    }
}

fn header(ui: &mut egui::Ui) {
    ui.label(RichText::new("Configuration").strong());
    ui.label(RichText::new("Widget").strong());
    ui.label(RichText::new("Bound value").strong());
    ui.end_row();
}

fn help(ui: &mut egui::Ui) {
    ui.label("Click a box to focus it, then:");
    egui::Grid::new("help_grid")
        .num_columns(2)
        .spacing([16., 2.])
        .show(ui, |ui| {
            for (key, action) in [
                ("↑ / ↓", "step the value up / down"),
                ("mouse wheel", "step the value (while hovered)"),
                ("Q / W", "halve / double the step"),
                ("R", "reset the step to default"),
                ("Enter", "commit the typed value"),
            ] {
                ui.monospace(key);
                ui.label(action);
                ui.end_row();
            }
        });
}
