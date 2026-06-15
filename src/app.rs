use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui;
use fileconvert::convert;

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

#[derive(Clone, PartialEq)]
enum Status {
    Queued,
    Converting(f32),
    Done(PathBuf),
    Failed(String),
}

struct Item {
    path: PathBuf,
    status: Status,
}

enum Msg {
    Progress(usize, f32),
    Done(usize, PathBuf),
    Failed(usize, String),
    Batch,
}

pub struct App {
    items: Vec<Item>,
    dest: Option<PathBuf>,
    rx: Option<Receiver<Msg>>,
    running: bool,
    concurrency: usize,
}

impl Default for App {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            dest: None,
            rx: None,
            running: false,
            concurrency: default_concurrency(),
        }
    }
}

impl App {
    fn add_files(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Video", &["mp4", "mkv", "mov", "avi", "webm", "m4v"])
            .pick_files()
        {
            for p in paths {
                if !self.items.iter().any(|i| i.path == p) {
                    self.items.push(Item {
                        path: p,
                        status: Status::Queued,
                    });
                }
            }
        }
    }

    fn choose_dest(&mut self) {
        if let Some(d) = rfd::FileDialog::new().pick_folder() {
            self.dest = Some(d);
        }
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
                    let txp = tx.clone();
                    let ctxp = ctx.clone();
                    let r = convert::convert_to(&input, &out, |f| {
                        let _ = txp.send(Msg::Progress(idx, f));
                        ctxp.request_repaint();
                    });
                    match r {
                        Ok(()) => {
                            let _ = tx.send(Msg::Done(idx, out));
                        }
                        Err(e) => {
                            let _ = tx.send(Msg::Failed(idx, format!("{e}")));
                        }
                    }
                    ctx.request_repaint();
                }));
            }
            for h in handles {
                let _ = h.join();
            }
            let _ = tx.send(Msg::Batch);
            ctx.request_repaint();
        });
    }

    fn drain(&mut self) {
        let mut done = false;
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                match m {
                    Msg::Progress(i, f) => self.items[i].status = Status::Converting(f),
                    Msg::Done(i, p) => self.items[i].status = Status::Done(p),
                    Msg::Failed(i, e) => self.items[i].status = Status::Failed(e),
                    Msg::Batch => done = true,
                }
            }
        }
        if done {
            self.running = false;
            self.rx = None;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.drain();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("FWMV Video Converter");
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
            });
            ui.label(match &self.dest {
                Some(d) => format!("Destination: {}", d.display()),
                None => "Destination: (none chosen)".into(),
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
            });
            ui.separator();
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
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    for it in &self.items {
                        let name = it.path.file_name().unwrap().to_string_lossy();
                        let s = match &it.status {
                            Status::Queued => "queued".to_string(),
                            Status::Converting(f) => format!("converting… {:.0}%", f * 100.0),
                            Status::Done(_) => "done".to_string(),
                            Status::Failed(e) => format!("failed: {e}"),
                        };
                        ui.label(format!("{name}  —  {s}"));
                    }
                });
            ui.separator();
            let can_start = !self.running
                && self.dest.is_some()
                && self
                    .items
                    .iter()
                    .any(|i| !matches!(i.status, Status::Done(_)));
            if ui
                .add_enabled(can_start, egui::Button::new("Convert"))
                .clicked()
            {
                self.start(ctx.clone());
            }
        });
    }
}
