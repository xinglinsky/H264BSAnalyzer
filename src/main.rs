fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 {
        let path = std::path::Path::new(&args[1]);
        if path.exists() && path.is_file() {
            return match h264bsanalyzer::cli::run(path) {
                Ok(()) => Ok(()),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            };
        }
    }
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
