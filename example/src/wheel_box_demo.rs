use eframe::egui::{self, RichText};
use egui_widgets::{WheelBox, WheelBoxValue};

const BOX_WIDTH: f32 = 120.;

pub struct WheelBoxDemo {
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

impl Default for WheelBoxDemo {
    fn default() -> Self {
        Self {
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
        }
    }
}

impl WheelBoxDemo {
    fn integers(&mut self, ui: &mut egui::Ui) {
        ui.heading("Integers");
        ui.add_space(4.);

        egui::Grid::new("integers_grid")
            .num_columns(3)
            .spacing([16., 8.])
            .striped(true)
            .show(ui, |ui| {
                header(ui);

                row(
                    ui,
                    "i32, default",
                    WheelBox::new(&mut self.int_basic).desired_width(BOX_WIDTH),
                );
                bound(ui, self.int_basic.to_string());

                row(
                    ui,
                    "i32, range [0, 100], suffix, step 5",
                    WheelBox::new(&mut self.int_range)
                        .range([0, 100])
                        .suffix("%")
                        .default_single_step(5)
                        .desired_width(BOX_WIDTH),
                );
                bound(ui, self.int_range.to_string());

                row(
                    ui,
                    "u8, saturating",
                    WheelBox::new(&mut self.int_u8)
                        .default_single_step(64)
                        .desired_width(BOX_WIDTH),
                );
                bound(ui, self.int_u8.to_string());

                row(
                    ui,
                    "i64, external step",
                    WheelBox::new(&mut self.int_external)
                        .single_step(&mut self.int_external_step)
                        .default_single_step(100)
                        .desired_width(BOX_WIDTH),
                );
                bound(
                    ui,
                    format!("{} (step {})", self.int_external, self.int_external_step),
                );

                let blue = if ui.visuals().dark_mode {
                    egui::Color32::LIGHT_BLUE
                } else {
                    egui::Color32::BLUE
                };
                row(
                    ui,
                    "i32, always_update(false), text color",
                    WheelBox::new(&mut self.int_no_update)
                        .always_update(false)
                        .text_color(blue)
                        .desired_width(BOX_WIDTH),
                );
                bound(ui, self.int_no_update.to_string());

                row(
                    ui,
                    "i32, prefix + suffix",
                    WheelBox::new(&mut self.int_affixes)
                        .prefix("x: ")
                        .suffix("px")
                        .desired_width(BOX_WIDTH),
                );
                bound(ui, self.int_affixes.to_string());

                row(
                    ui,
                    "i64, narrow: suffix hidden",
                    WheelBox::new(&mut self.int_narrow)
                        .prefix("x: ")
                        .suffix("nm")
                        .desired_width(80.),
                );
                bound(ui, self.int_narrow.to_string());

                row(
                    ui,
                    "i64, very narrow: value truncated",
                    WheelBox::new(&mut self.int_long)
                        .prefix("x: ")
                        .suffix("nm")
                        .desired_width(60.),
                );
                bound(ui, self.int_long.to_string());
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

                row(
                    ui,
                    "f64, 2 decimals",
                    WheelBox::new(&mut self.float_basic)
                        .decimals(2)
                        .desired_width(BOX_WIDTH),
                );
                bound(ui, self.float_basic.to_string());

                row(
                    ui,
                    "f64, range [0, 1], step 0.1, default decimals",
                    WheelBox::new(&mut self.float_range)
                        .range([0.0, 1.0])
                        .default_single_step(0.1)
                        .desired_width(BOX_WIDTH),
                );
                bound(ui, self.float_range.to_string());

                row(
                    ui,
                    "f32, weak suffix",
                    WheelBox::new(&mut self.float_f32)
                        .decimals(1)
                        .suffix(RichText::new("mm").weak())
                        .desired_width(BOX_WIDTH),
                );
                bound(ui, self.float_f32.to_string());

                row(
                    ui,
                    "f64, external step",
                    WheelBox::new(&mut self.float_external)
                        .decimals(4)
                        .single_step(&mut self.float_external_step)
                        .default_single_step(0.5)
                        .desired_width(BOX_WIDTH),
                );
                bound(
                    ui,
                    format!(
                        "{} (step {})",
                        self.float_external, self.float_external_step
                    ),
                );

                row(
                    ui,
                    "f64, icon prefix",
                    WheelBox::new(&mut self.float_icon)
                        .decimals(1)
                        .prefix("⏱ ")
                        .suffix("ms")
                        .desired_width(BOX_WIDTH),
                );
                bound(ui, self.float_icon.to_string());
            });
    }
}

impl WheelBoxDemo {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::both().show(ui, |ui| {
            help(ui);
            ui.separator();
            self.integers(ui);
            ui.add_space(16.);
            self.floats(ui);

            ui.add_space(8.);
            if ui.button("Reset all values from outside").clicked() {
                *self = Self::default();
            }
        });
    }
}

fn header(ui: &mut egui::Ui) {
    ui.label(RichText::new("Configuration").strong());
    ui.label(RichText::new("Widget").strong());
    ui.label(RichText::new("Bound value").strong());
    ui.end_row();
}

/// The configuration, which also names the box for screen readers, and the box.
fn row<T: WheelBoxValue>(ui: &mut egui::Ui, configuration: &str, wheel_box: WheelBox<'_, T>) {
    let label = ui.label(configuration);
    ui.add(wheel_box).labelled_by(label.id);
}

/// The bound value, read after the box changed it, and the end of the row.
fn bound(ui: &mut egui::Ui, value: String) {
    ui.monospace(value);
    ui.end_row();
}

fn help(ui: &mut egui::Ui) {
    ui.label("Click a box (or Tab to it) to focus it, then:");
    egui::Grid::new("help_grid")
        .num_columns(2)
        .spacing([16., 2.])
        .show(ui, |ui| {
            for (key, action) in [
                ("↑ / ↓", "step the value up / down"),
                ("mouse wheel", "step the value (while hovered)"),
                ("Q / W", "halve / double the step"),
                ("R", "reset the step to default"),
                ("0-9 . , -", "type a value (replaces the selected text)"),
                ("Enter / Tab", "commit the typed value"),
                ("Esc", "discard the typed value"),
            ] {
                ui.monospace(key);
                ui.label(action);
                ui.end_row();
            }
        });
}
