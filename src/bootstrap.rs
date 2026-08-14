use crate::app::LauncherApp;

pub fn run(initial_uri: Option<String>) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SS14 Lynncher")
            .with_inner_size([800.0, 500.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "SS14 Lynncher",
        options,
        Box::new(move |_cc| Ok(Box::new(LauncherApp::new(initial_uri.clone())))),
    )
}

