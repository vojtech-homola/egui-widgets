mod app;

use app::DemoApp;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let viewport = eframe::egui::ViewportBuilder::default().with_inner_size([640., 640.]);

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "egui_widgets demo",
        native_options,
        Box::new(|cc| Ok(DemoApp::new(cc))),
    )
    .unwrap();
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        eframe::WebRunner::new()
            .start(canvas, web_options, Box::new(|cc| Ok(DemoApp::new(cc))))
            .await
            .expect("Failed to start eframe WebRunner");
    });
}
