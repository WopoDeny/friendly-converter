//! Background conversion thread. The UI never blocks on file work.

use eframe::egui;
use friendly_converter::convert::{self, Options};
use friendly_converter::formats::{Format, Target};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::Instant;

pub enum Task {
    Convert { id: u64, input: PathBuf, from: Format, target: Target },
    MergePdfs { inputs: Vec<PathBuf> },
    ImagesToPdf { inputs: Vec<PathBuf> },
}

pub enum Msg {
    Started(u64),
    Done { id: u64, outputs: Vec<PathBuf>, secs: f32 },
    Failed { id: u64, error: String },
    Tool { label: &'static str, result: Result<PathBuf, String>, secs: f32 },
    Finished,
}

fn error_text(e: anyhow::Error) -> String {
    format!("{e:#}")
}

pub fn spawn(tasks: Vec<Task>, opts: Options, ctx: egui::Context) -> Receiver<Msg> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("converter".into())
        .spawn(move || {
            for task in tasks {
                match task {
                    Task::Convert { id, input, from, target } => {
                        let _ = tx.send(Msg::Started(id));
                        ctx.request_repaint();
                        let t0 = Instant::now();
                        let result = catch_unwind(AssertUnwindSafe(|| convert::convert_file(&input, from, target, &opts)));
                        let secs = t0.elapsed().as_secs_f32();
                        let msg = match result {
                            Ok(Ok(outputs)) => Msg::Done { id, outputs, secs },
                            Ok(Err(e)) => Msg::Failed { id, error: error_text(e) },
                            Err(_) => Msg::Failed { id, error: "internal error while converting this file".into() },
                        };
                        let _ = tx.send(msg);
                    }
                    Task::MergePdfs { inputs } => {
                        let t0 = Instant::now();
                        let result = catch_unwind(AssertUnwindSafe(|| convert::merge_pdfs(&inputs, &opts)));
                        let result = match result {
                            Ok(r) => r.map_err(error_text),
                            Err(_) => Err("internal error while merging".into()),
                        };
                        let _ = tx.send(Msg::Tool { label: "merge PDFs", result, secs: t0.elapsed().as_secs_f32() });
                    }
                    Task::ImagesToPdf { inputs } => {
                        let t0 = Instant::now();
                        let result = catch_unwind(AssertUnwindSafe(|| convert::images_to_one_pdf(&inputs, &opts)));
                        let result = match result {
                            Ok(r) => r.map_err(error_text),
                            Err(_) => Err("internal error while building the PDF".into()),
                        };
                        let _ = tx.send(Msg::Tool { label: "images to PDF", result, secs: t0.elapsed().as_secs_f32() });
                    }
                }
                ctx.request_repaint();
            }
            let _ = tx.send(Msg::Finished);
            ctx.request_repaint();
        })
        .expect("cannot start worker thread");
    rx
}
