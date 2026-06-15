// GUI app: don't spawn a separate console window on Windows. Activity is shown
// in the in-app console pane instead.
#![windows_subsystem = "windows"]

mod app;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    // Any path arguments pre-populate the queue (supports drag-onto-icon,
    // "Open with…", and `wiliplayerconvert video1.mp4 video2.mkv`).
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([720.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "FreeWili Player Movie Convertor",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, files)))),
    )
}
