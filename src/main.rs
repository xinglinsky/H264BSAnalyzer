fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1200.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "H264BSAnalyzer",
        options,
        Box::new(|cc| Ok(Box::new(h264bsanalyzer::gui::App::new(cc)))),
    )
}
