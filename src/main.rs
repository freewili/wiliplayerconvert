mod app;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    // Any path arguments pre-populate the queue (supports drag-onto-icon,
    // "Open with…", and `fileconvert video1.mp4 video2.mkv`).
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([620.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "FWMV Video Converter",
        options,
        Box::new(move |_cc| Ok(Box::new(app::App::with_initial_files(files)))),
    )
}
