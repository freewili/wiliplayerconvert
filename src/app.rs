use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui;
use egui_extras::{Column, TableBuilder};
use wiliplayerconvert::convert;

/// Logical CPUs (fallback 4 if unknown).
fn cpu_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

/// Default simultaneous conversions: a memory-safe fraction of the cores that
/// avoids oversubscribing FFmpeg's own per-file decode threads.
fn default_concurrency() -> usize {
    (cpu_count() / 4).clamp(2, 8)
}

/// Human-readable byte size for the console log.
fn human_size(bytes: u64) -> String {
    const MB: u64 = 1 << 20;
    const KB: u64 = 1 << 10;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Cap on retained console lines, so a long batch can't grow memory unbounded.
const MAX_LOG_LINES: usize = 1000;

const KEY_DEST: &str = "dest";
const KEY_CONCURRENCY: &str = "concurrency";

#[derive(Clone, PartialEq)]
enum Status {
    Queued,
    Converting(f32),
    Done(PathBuf),
    Failed(String),
}

impl Status {
    fn is_finished(&self) -> bool {
        matches!(self, Status::Done(_) | Status::Failed(_))
    }
}

struct Item {
    path: PathBuf,
    status: Status,
}

enum Msg {
    Progress(usize, f32),
    Done(usize, PathBuf),
    Failed(usize, String),
    Log(String),
    Batch,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Queue,
    Console,
}

pub struct App {
    items: Vec<Item>,
    dest: Option<PathBuf>,
    rx: Option<Receiver<Msg>>,
    running: bool,
    concurrency: usize,
    log: Vec<String>,
    tab: Tab,
}

impl App {
    /// Build the app: apply the light theme, restore remembered settings
    /// (destination + concurrency) from storage, and pre-queue any path args.
    pub fn new(cc: &eframe::CreationContext<'_>, files: impl IntoIterator<Item = PathBuf>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::light());

        let mut dest = None;
        let mut concurrency = default_concurrency();
        if let Some(storage) = cc.storage {
            if let Some(d) = storage.get_string(KEY_DEST) {
                if !d.is_empty() && std::path::Path::new(&d).is_dir() {
                    dest = Some(PathBuf::from(d));
                }
            }
            if let Some(c) = storage.get_string(KEY_CONCURRENCY) {
                if let Ok(n) = c.parse::<usize>() {
                    concurrency = n.clamp(1, cpu_count());
                }
            }
        }

        let mut app = Self {
            items: Vec::new(),
            dest,
            rx: None,
            running: false,
            concurrency,
            log: Vec::new(),
            tab: Tab::Queue,
        };
        for p in files {
            if !app.items.iter().any(|i| i.path == p) {
                app.items.push(Item {
                    path: p,
                    status: Status::Queued,
                });
            }
        }
        if !app.items.is_empty() {
            app.log(format!("Queued {} file(s) from arguments.", app.items.len()));
        }
        app
    }

    /// Append a line to the in-app console, trimming to the retention cap.
    fn log(&mut self, msg: impl Into<String>) {
        self.log.push(msg.into());
        if self.log.len() > MAX_LOG_LINES {
            let drop = self.log.len() - MAX_LOG_LINES;
            self.log.drain(0..drop);
        }
    }

    fn add_files(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Video", &["mp4", "mkv", "mov", "avi", "webm", "m4v"])
            .pick_files()
        {
            let mut added = 0;
            for p in paths {
                if !self.items.iter().any(|i| i.path == p) {
                    self.items.push(Item {
                        path: p,
                        status: Status::Queued,
                    });
                    added += 1;
                }
            }
            if added > 0 {
                self.log(format!("Added {added} file(s)."));
            }
        }
    }

    fn choose_dest(&mut self) {
        if let Some(d) = rfd::FileDialog::new().pick_folder() {
            self.log(format!("Destination: {}", d.display()));
            self.dest = Some(d);
        }
    }

    /// Remove already-converted (done/failed) rows from the queue.
    fn clear_finished(&mut self) {
        self.items.retain(|i| !i.status.is_finished());
    }

    fn has_finished(&self) -> bool {
        self.items.iter().any(|i| i.status.is_finished())
    }

    fn has_pending(&self) -> bool {
        self.items.iter().any(|i| !matches!(i.status, Status::Done(_)))
    }

    fn start(&mut self, ctx: egui::Context) {
        let Some(dest) = self.dest.clone() else {
            return;
        };
        // Reserve collision-safe output paths for the whole batch up front, in
        // queue order, so parallel workers never race two same-named inputs onto
        // the same `.fwmv`. Each job carries its pre-assigned (idx, input, out).
        let mut reserved: HashSet<PathBuf> = HashSet::new();
        let jobs: Vec<(usize, PathBuf, PathBuf)> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| !matches!(i.status, Status::Done(_)))
            .map(|(i, it)| {
                let out = convert::output_path(&dest, &it.path, &reserved);
                reserved.insert(out.clone());
                (i, it.path.clone(), out)
            })
            .collect();
        if jobs.is_empty() {
            return;
        }
        let (tx, rx): (Sender<Msg>, Receiver<Msg>) = channel();
        self.rx = Some(rx);
        self.running = true;
        for it in &mut self.items {
            if !matches!(it.status, Status::Done(_)) {
                it.status = Status::Queued;
            }
        }

        let n_workers = self.concurrency.clamp(1, jobs.len());
        // Split the CPU across the pool: each file decodes with this many threads
        // so workers * threads ~= cores (no oversubscription, no idle cores).
        let decode_threads = (cpu_count() / n_workers).max(1);
        self.log(format!(
            "Starting {} file(s), up to {} at a time ({} decode thread(s) each).",
            jobs.len(),
            n_workers,
            decode_threads
        ));
        let queue: Arc<Mutex<VecDeque<(usize, PathBuf, PathBuf)>>> =
            Arc::new(Mutex::new(jobs.into_iter().collect()));

        // Coordinator: spawn a bounded pool of workers that pull from the shared
        // queue, then signal Batch once they all finish.
        thread::spawn(move || {
            let mut handles = Vec::with_capacity(n_workers);
            for _ in 0..n_workers {
                let q = queue.clone();
                let tx = tx.clone();
                let ctx = ctx.clone();
                handles.push(thread::spawn(move || loop {
                    let job = q.lock().unwrap().pop_front();
                    let Some((idx, input, out)) = job else {
                        break;
                    };
                    let name = input
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let _ = tx.send(Msg::Log(format!("{name}: converting...")));
                    let txp = tx.clone();
                    let ctxp = ctx.clone();
                    let r = convert::convert_to(&input, &out, decode_threads, |f| {
                        let _ = txp.send(Msg::Progress(idx, f));
                        ctxp.request_repaint();
                    });
                    match r {
                        Ok(()) => {
                            let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
                            let outname = out
                                .file_name()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let _ = tx.send(Msg::Log(format!(
                                "{name}: done -> {outname} ({})",
                                human_size(size)
                            )));
                            let _ = tx.send(Msg::Done(idx, out));
                        }
                        Err(e) => {
                            let _ = tx.send(Msg::Log(format!("{name}: FAILED - {e}")));
                            let _ = tx.send(Msg::Failed(idx, format!("{e}")));
                        }
                    }
                    ctx.request_repaint();
                }));
            }
            for h in handles {
                let _ = h.join();
            }
            let _ = tx.send(Msg::Log("Batch complete.".to_string()));
            let _ = tx.send(Msg::Batch);
            ctx.request_repaint();
        });
    }

    fn drain(&mut self) {
        let mut done = false;
        let mut logged: Vec<String> = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                match m {
                    Msg::Progress(i, f) => self.items[i].status = Status::Converting(f),
                    Msg::Done(i, p) => self.items[i].status = Status::Done(p),
                    Msg::Failed(i, e) => self.items[i].status = Status::Failed(e),
                    Msg::Log(line) => logged.push(line),
                    Msg::Batch => done = true,
                }
            }
        }
        for line in logged {
            self.log(line);
        }
        if done {
            self.running = false;
            self.rx = None;
        }
    }

    /// The resizable queue table, filling the available space.
    fn queue_table(&self, ui: &mut egui::Ui) {
        if self.items.is_empty() {
            ui.weak("No files added yet — click \u{201c}Add files\u{2026}\u{201d}.");
            return;
        }
        let green = egui::Color32::from_rgb(30, 140, 60);
        let red = egui::Color32::from_rgb(180, 40, 40);
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            // File absorbs the slack (and clips long names); Status and Progress
            // are fixed widths so the progress bar is always fully on screen.
            .column(Column::remainder().at_least(120.0).clip(true)) // File
            .column(Column::initial(80.0).at_least(60.0).clip(true)) // Status
            .column(Column::initial(160.0).at_least(120.0).clip(true)) // Progress
            .auto_shrink([false, false])
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.strong("File");
                });
                header.col(|ui| {
                    ui.strong("Status");
                });
                header.col(|ui| {
                    ui.strong("Progress");
                });
            })
            .body(|mut body| {
                for it in &self.items {
                    body.row(22.0, |mut row| {
                        row.col(|ui| {
                            let name = it.path.file_name().unwrap().to_string_lossy();
                            ui.add(egui::Label::new(name.as_ref()).truncate())
                                .on_hover_text(it.path.display().to_string());
                        });
                        row.col(|ui| match &it.status {
                            Status::Queued => {
                                ui.weak("queued");
                            }
                            Status::Converting(_) => {
                                ui.label("converting\u{2026}");
                            }
                            Status::Done(p) => {
                                ui.colored_label(green, "done")
                                    .on_hover_text(p.display().to_string());
                            }
                            Status::Failed(e) => {
                                ui.colored_label(red, "failed").on_hover_text(e.as_str());
                            }
                        });
                        row.col(|ui| {
                            let frac = match &it.status {
                                Status::Queued => 0.0,
                                Status::Converting(f) => *f,
                                Status::Done(_) => 1.0,
                                Status::Failed(_) => 0.0,
                            };
                            ui.add(
                                egui::ProgressBar::new(frac)
                                    .text(format!("{:.0}%", frac * 100.0)),
                            );
                        });
                    });
                }
            });
    }

    /// The console/log view, filling the available space.
    fn console_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.log.is_empty(), egui::Button::new("Clear"))
                .clicked()
            {
                self.log.clear();
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("console")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.log.is_empty() {
                    ui.weak("(idle)");
                } else {
                    for line in &self.log {
                        ui.label(egui::RichText::new(line).monospace().small());
                    }
                }
            });
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Some(d) = &self.dest {
            storage.set_string(KEY_DEST, d.to_string_lossy().to_string());
        }
        storage.set_string(KEY_CONCURRENCY, self.concurrency.to_string());
    }

    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.drain();
        egui::CentralPanel::default().show(ctx, |ui| {
            // Top action row: Add files, Choose destination, Convert.
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.running, egui::Button::new("Add files…"))
                    .clicked()
                {
                    self.add_files();
                }
                if ui
                    .add_enabled(!self.running, egui::Button::new("Choose destination…"))
                    .clicked()
                {
                    self.choose_dest();
                }
                let can_start = !self.running && self.dest.is_some() && self.has_pending();
                if ui
                    .add_enabled(can_start, egui::Button::new("Convert"))
                    .clicked()
                {
                    self.start(ctx.clone());
                }
            });

            ui.horizontal(|ui| {
                ui.label("Destination:");
                match &self.dest {
                    Some(d) => {
                        let full = d.display().to_string();
                        ui.add(
                            egui::Label::new(egui::RichText::new(&full).monospace()).truncate(),
                        )
                        .on_hover_text(full);
                    }
                    None => {
                        ui.weak("(none chosen)");
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("Simultaneous conversions:");
                ui.add_enabled(
                    !self.running,
                    egui::DragValue::new(&mut self.concurrency).range(1..=cpu_count()),
                )
                .on_hover_text(
                    "How many files convert at once. Higher = faster batches but \
                     more RAM (each file is fully buffered in memory) and, past a \
                     point, CPU oversubscription vs FFmpeg's own decode threads.",
                );
                ui.separator();
                if ui
                    .add_enabled(
                        !self.running && self.has_finished(),
                        egui::Button::new("Clear finished"),
                    )
                    .clicked()
                {
                    self.clear_finished();
                }
            });

            let total = self.items.len();
            let done = self
                .items
                .iter()
                .filter(|i| matches!(i.status, Status::Done(_)))
                .count();
            if total > 0 {
                ui.add(
                    egui::ProgressBar::new(done as f32 / total as f32)
                        .text(format!("{done}/{total} files")),
                );
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Queue, "Queue");
                ui.selectable_value(&mut self.tab, Tab::Console, "Console");
            });
            ui.separator();

            // The selected tab fills the entire remaining bottom area.
            match self.tab {
                Tab::Queue => self.queue_table(ui),
                Tab::Console => self.console_view(ui),
            }
        });
    }
}
