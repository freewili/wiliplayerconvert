mod app;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([560.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "FWMV Video Converter",
        options,
        Box::new(|_cc| Ok(Box::<app::App>::default())),
    )
}
