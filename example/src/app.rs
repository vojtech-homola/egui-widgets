use eframe::egui;

use crate::graph_demo::GraphDemo;
use crate::wheel_box_demo::WheelBoxDemo;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    WheelBox,
    Graph,
}

pub struct DemoApp {
    tab: Tab,
    wheel_box: WheelBoxDemo,
    graph: GraphDemo,
}

impl DemoApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            tab: Tab::WheelBox,
            wheel_box: WheelBoxDemo::default(),
            graph: GraphDemo::default(),
        }
    }
}

impl eframe::App for DemoApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::Panel::top("top_panel").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("egui_widgets demo");
                ui.separator();
                ui.selectable_value(&mut self.tab, Tab::WheelBox, "WheelBox");
                ui.selectable_value(&mut self.tab, Tab::Graph, "Graph");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::widgets::global_theme_preference_buttons(ui);
                });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::WheelBox => self.wheel_box.ui(ui),
            Tab::Graph => self.graph.ui(ui, frame.info().cpu_usage),
        });
    }
}
