//! The Friendly Converter window.

pub mod circuit;
pub mod theme;
pub mod widgets;
mod worker;

use circuit::Circuit;
use eframe::egui::{
    self, Align, Align2, Color32, Frame, Id, Key, LayerId, Layout, Margin, Order, Pos2, Rect, RichText, ScrollArea,
    Sense, Shape, Stroke, Ui, Vec2,
};
use friendly_converter::convert::Options;
use friendly_converter::formats::{Category, Format, Target};
use friendly_converter::util::{human_size, open_in_file_manager, reveal_file};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use theme::{alpha, mono, panel_fill, title_font, GOLD, GOLD_DIM, GREEN, GREEN_SOFT, RED, TEXT, TEXT_DIM};
use widgets::{badge, caption, corners, glow_text, hint, neon_button};
use worker::{Msg, Task};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
enum Status {
    Ready,
    Queued,
    Running,
    Done { outputs: Vec<PathBuf>, secs: f32 },
    Failed(String),
}

struct Job {
    id: u64,
    path: PathBuf,
    name: String,
    size: u64,
    from: Format,
    target: Target,
    status: Status,
}

#[derive(Clone, Copy, PartialEq)]
enum LogKind {
    Info,
    Ok,
    Warn,
    Err,
}

struct LogLine {
    t: f32,
    kind: LogKind,
    text: String,
}

pub struct App {
    jobs: Vec<Job>,
    next_id: u64,
    log: Vec<LogLine>,
    rx: Option<Receiver<Msg>>,
    batch_started: Option<Instant>,
    opts: Options,
    circuit: Circuit,
    start: Instant,
    animate: bool,
}

fn category_color(f: Format) -> Color32 {
    match f.category() {
        Category::Document => GREEN,
        Category::Image => GOLD,
        Category::Data => GREEN_SOFT,
        Category::Table => Color32::from_rgb(236, 206, 110),
    }
}

fn shorten(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let tail: String = s.chars().skip(n - (max - 1)).collect();
    format!("\u{2026}{tail}")
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> App {
        theme::install(&cc.egui_ctx);
        let mut app = App {
            jobs: Vec::new(),
            next_id: 1,
            log: Vec::new(),
            rx: None,
            batch_started: None,
            opts: Options::default(),
            circuit: Circuit::generate(0x5EED_0F_F1E2D),
            start: Instant::now(),
            animate: true,
        };
        app.push_log(LogKind::Info, format!("Friendly Converter v{VERSION} ready. Everything runs offline."));
        app.push_log(LogKind::Info, "documents  pdf docx md html txt".into());
        app.push_log(LogKind::Info, "images     png jpg webp bmp gif tiff ico tga".into());
        app.push_log(LogKind::Info, "data       json yaml toml xml".into());
        app.push_log(LogKind::Info, "tables     csv xlsx xls ods".into());
        app.push_log(LogKind::Info, "Drop files on the window or press Ctrl+O.".into());
        app
    }

    fn push_log(&mut self, kind: LogKind, text: String) {
        let t = self.start.elapsed().as_secs_f32();
        self.log.push(LogLine { t, kind, text });
        if self.log.len() > 2000 {
            self.log.drain(..500);
        }
    }

    fn is_running(&self) -> bool {
        self.rx.is_some()
    }

    fn counts(&self) -> (usize, usize, usize) {
        let done = self.jobs.iter().filter(|j| matches!(j.status, Status::Done { .. })).count();
        let failed = self.jobs.iter().filter(|j| matches!(j.status, Status::Failed(_))).count();
        (self.jobs.len(), done, failed)
    }

    // ------------------------------------------------------------ files

    fn add_paths(&mut self, paths: Vec<PathBuf>) {
        let mut expanded = Vec::new();
        for p in paths {
            if p.is_dir() {
                match std::fs::read_dir(&p) {
                    Ok(entries) => {
                        let mut files: Vec<PathBuf> =
                            entries.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
                        files.sort();
                        expanded.extend(files);
                    }
                    Err(e) => self.push_log(LogKind::Err, format!("cannot read folder {}: {e}", p.display())),
                }
            } else {
                expanded.push(p);
            }
        }
        let mut added = 0usize;
        let mut skipped = Vec::new();
        for path in expanded {
            if self.jobs.iter().any(|j| j.path == path) {
                continue;
            }
            let Some(from) = Format::from_path(&path) else {
                skipped.push(file_name(&path));
                continue;
            };
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            self.jobs.push(Job {
                id: self.next_id,
                name: file_name(&path),
                path,
                size,
                from,
                target: from.default_target(),
                status: Status::Ready,
            });
            self.next_id += 1;
            added += 1;
        }
        if added > 0 {
            self.push_log(LogKind::Info, format!("loaded {added} file(s)"));
        }
        if !skipped.is_empty() {
            let list = skipped.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
            let more = if skipped.len() > 5 { format!(" and {} more", skipped.len() - 5) } else { String::new() };
            self.push_log(LogKind::Warn, format!("unsupported format, skipped: {list}{more}"));
        }
    }

    fn pick_files(&mut self) {
        if let Some(files) = rfd::FileDialog::new().set_title("Add files to convert").pick_files() {
            self.add_paths(files);
        }
    }

    // ------------------------------------------------------------ work

    fn convert_all(&mut self, ctx: &egui::Context) {
        if self.is_running() || self.jobs.is_empty() {
            return;
        }
        let mut pending: Vec<usize> = self
            .jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| matches!(j.status, Status::Ready | Status::Failed(_)))
            .map(|(i, _)| i)
            .collect();
        if pending.is_empty() {
            pending = (0..self.jobs.len()).collect();
        }
        let mut tasks = Vec::new();
        for i in pending {
            let job = &mut self.jobs[i];
            job.status = Status::Queued;
            tasks.push(Task::Convert { id: job.id, input: job.path.clone(), from: job.from, target: job.target });
        }
        let n = tasks.len();
        self.push_log(LogKind::Info, format!("converting {n} file(s)..."));
        self.batch_started = Some(Instant::now());
        self.rx = Some(worker::spawn(tasks, self.opts.clone(), ctx.clone()));
    }

    fn run_tool(&mut self, ctx: &egui::Context, task: Task, what: String) {
        if self.is_running() {
            return;
        }
        self.push_log(LogKind::Info, what);
        self.batch_started = Some(Instant::now());
        self.rx = Some(worker::spawn(vec![task], self.opts.clone(), ctx.clone()));
    }

    fn poll(&mut self) {
        let msgs: Vec<Msg> = match &self.rx {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };
        let mut finished = false;
        for msg in msgs {
            match msg {
                Msg::Started(id) => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        j.status = Status::Running;
                    }
                }
                Msg::Done { id, outputs, secs } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        let names: Vec<String> = outputs.iter().map(|p| file_name(p)).collect();
                        let shown = if names.len() > 3 {
                            format!("{} files in {}", names.len(), outputs[0].parent().map(file_name).unwrap_or_default())
                        } else {
                            names.join(", ")
                        };
                        let text = format!("{} -> {} ({secs:.2}s)", j.name, shown);
                        j.status = Status::Done { outputs, secs };
                        self.push_log(LogKind::Ok, text);
                    }
                }
                Msg::Failed { id, error } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        let text = format!("{}: {error}", j.name);
                        j.status = Status::Failed(error);
                        self.push_log(LogKind::Err, text);
                    }
                }
                Msg::Tool { label, result, secs } => match result {
                    Ok(path) => self.push_log(LogKind::Ok, format!("{label} -> {} ({secs:.2}s)", file_name(&path))),
                    Err(e) => self.push_log(LogKind::Err, format!("{label}: {e}")),
                },
                Msg::Finished => finished = true,
            }
        }
        if finished {
            self.rx = None;
            for j in &mut self.jobs {
                if matches!(j.status, Status::Queued | Status::Running) {
                    j.status = Status::Ready;
                }
            }
            if let Some(t0) = self.batch_started.take() {
                let (_, done, failed) = self.counts();
                self.push_log(
                    LogKind::Info,
                    format!("finished in {:.2}s  /  done {done}, failed {failed}", t0.elapsed().as_secs_f32()),
                );
            }
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            if self.is_running() {
                self.push_log(LogKind::Warn, "wait for the current conversion to finish".into());
            } else {
                self.add_paths(dropped);
            }
        }
        let (open, run) = ctx.input(|i| {
            (i.modifiers.command && i.key_pressed(Key::O), i.modifiers.command && i.key_pressed(Key::Enter))
        });
        if open && !self.is_running() {
            self.pick_files();
        }
        if run {
            self.convert_all(ctx);
        }
    }

    // ------------------------------------------------------------ drawing

    fn header(&mut self, ui: &mut Ui, intro: f32) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 60.0), Sense::hover());
        let time = ui.input(|i| i.time);
        let painter = ui.painter().clone();

        let chip = Rect::from_center_size(Pos2::new(rect.left() + 22.0, rect.center().y), Vec2::splat(38.0));
        draw_logo(&painter, chip);

        let title = "FRIENDLY//CONVERTER";
        let shown = ((title.chars().count() as f32) * (intro * 1.3).min(1.0)).ceil() as usize;
        let text: String = title.chars().take(shown).collect();
        let title_pos = Pos2::new(chip.right() + 18.0, rect.top() + 20.0);
        let r = glow_text(&painter, title_pos, Align2::LEFT_CENTER, &text, title_font(26.0), GREEN);
        let blink = (time * 1.6) as i64 % 2 == 0;
        if intro < 1.0 || blink {
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(r.right() + 5.0, r.top() + 4.0), Vec2::new(11.0, (r.height() - 8.0).max(8.0))),
                0.0,
                alpha(GREEN, 210),
            );
        }
        painter.text(
            Pos2::new(chip.right() + 19.0, rect.top() + 46.0),
            Align2::LEFT_CENTER,
            "offline converter for documents, images, data and tables",
            mono(12.0),
            TEXT_DIM,
        );

        // Counters on the right.
        let (total, done, failed) = self.counts();
        let mut x = rect.right();
        for (label, value, color) in [("failed", failed, RED), ("done", done, GREEN), ("in list", total, GOLD)] {
            let w = 92.0;
            let cell = Rect::from_min_size(Pos2::new(x - w, rect.top() + 8.0), Vec2::new(w - 8.0, 44.0));
            painter.rect_filled(cell, 0.0, panel_fill());
            painter.rect_stroke(cell, 0.0, Stroke::new(1.0, alpha(color, 90)));
            painter.text(
                Pos2::new(cell.center().x, cell.top() + 17.0),
                Align2::CENTER_CENTER,
                value.to_string(),
                title_font(20.0),
                color,
            );
            painter.text(Pos2::new(cell.center().x, cell.bottom() - 9.0), Align2::CENTER_CENTER, label, mono(10.5), TEXT_DIM);
            x -= w;
        }

        // Bus line under the header.
        let y = rect.bottom() + 8.0;
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, alpha(GREEN, 80)));
        let mut nx = rect.left() + 60.0;
        while nx < rect.right() - 20.0 {
            painter.circle_filled(Pos2::new(nx, y), 2.2, alpha(GOLD, 150));
            nx += 140.0;
        }
        ui.add_space(10.0);
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let running = self.is_running();
        let has_jobs = !self.jobs.is_empty();
        ui.horizontal_wrapped(|ui| {
            if neon_button(ui, "+ Add files", GREEN, !running).on_hover_text("Ctrl+O").clicked() {
                self.pick_files();
            }
            let label = if running { "Converting..." } else { "Convert all" };
            if neon_button(ui, label, GOLD, !running && has_jobs).on_hover_text("Ctrl+Enter").clicked() {
                self.convert_all(&ctx);
            }
            if neon_button(ui, "Clear list", TEXT_DIM, !running && has_jobs).clicked() {
                self.jobs.clear();
                self.push_log(LogKind::Info, "list cleared".into());
            }

            ui.add_space(14.0);
            let pdfs: Vec<PathBuf> = self.jobs.iter().filter(|j| j.from == Format::Pdf).map(|j| j.path.clone()).collect();
            let images: Vec<PathBuf> =
                self.jobs.iter().filter(|j| j.from.category() == Category::Image).map(|j| j.path.clone()).collect();

            let merge_label = format!("Merge PDFs ({})", pdfs.len());
            if neon_button(ui, &merge_label, GREEN_SOFT, !running && pdfs.len() >= 2)
                .on_hover_text("Join every PDF in the list into merged.pdf, in list order")
                .clicked()
            {
                let n = pdfs.len();
                self.run_tool(&ctx, Task::MergePdfs { inputs: pdfs }, format!("merging {n} PDF files..."));
            }
            let img_label = format!("Images to one PDF ({})", images.len());
            if neon_button(ui, &img_label, GREEN_SOFT, !running && images.len() >= 2)
                .on_hover_text("One page per image, in list order")
                .clicked()
            {
                let n = images.len();
                self.run_tool(&ctx, Task::ImagesToPdf { inputs: images }, format!("building a PDF from {n} images..."));
            }
        });

        if has_jobs {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                caption(ui, "set every file to", GOLD_DIM);
                let mut union: Vec<Target> = Vec::new();
                for j in &self.jobs {
                    for t in j.from.targets() {
                        if !union.contains(&t) {
                            union.push(t);
                        }
                    }
                }
                let mut chosen: Option<Target> = None;
                ui.add_enabled_ui(!running, |ui| {
                    egui::ComboBox::new("set_all_targets", "")
                        .width(110.0)
                        .selected_text(RichText::new("choose").color(TEXT_DIM))
                        .show_ui(ui, |ui| {
                            for t in &union {
                                if ui.selectable_label(false, t.label()).clicked() {
                                    chosen = Some(*t);
                                }
                            }
                        });
                });
                if let Some(t) = chosen {
                    let mut changed = 0;
                    for j in &mut self.jobs {
                        if j.from.targets().contains(&t) {
                            j.target = t;
                            j.status = Status::Ready;
                            changed += 1;
                        }
                    }
                    self.push_log(LogKind::Info, format!("{changed} file(s) set to {}", t.label()));
                }
                hint(ui, "files that cannot become this format keep their choice");
            });
        }
        ui.add_space(10.0);
    }

    fn drop_zone(&mut self, ui: &mut Ui) {
        let size = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let painter = ui.painter().clone();
        let hovered = response.hovered();
        let time = ui.input(|i| i.time) as f32;

        painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(2, 9, 4, 205));
        let inner = rect.shrink(10.0);
        let pts = [inner.left_top(), inner.right_top(), inner.right_bottom(), inner.left_bottom(), inner.left_top()];
        let border = if hovered { alpha(GREEN, 230) } else { alpha(GREEN, 120) };
        painter.extend(Shape::dashed_line(&pts, Stroke::new(1.4, border), 10.0, 7.0));
        corners(&painter, rect, GOLD, 22.0);

        let c = rect.center();
        // A socket: the chip waiting for files.
        let socket = Rect::from_center_size(c - Vec2::new(0.0, 92.0), Vec2::new(120.0, 74.0));
        let pulse = 0.5 + 0.5 * (time * 2.2).sin();
        painter.rect_filled(socket, 0.0, Color32::from_rgb(6, 16, 9));
        painter.rect_stroke(socket, 0.0, Stroke::new(1.5, alpha(GOLD, (150.0 + 100.0 * pulse) as u8)));
        for i in 0..6 {
            let x = socket.left() + 14.0 + i as f32 * 18.4;
            painter.line_segment([Pos2::new(x, socket.top()), Pos2::new(x, socket.top() - 12.0)], Stroke::new(2.0, alpha(GOLD, 170)));
            painter.line_segment(
                [Pos2::new(x, socket.bottom()), Pos2::new(x, socket.bottom() + 12.0)],
                Stroke::new(2.0, alpha(GOLD, 170)),
            );
        }
        painter.text(socket.center(), Align2::CENTER_CENTER, "+", title_font(34.0), alpha(GREEN, (160.0 + 90.0 * pulse) as u8));

        glow_text(&painter, c + Vec2::new(0.0, 6.0), Align2::CENTER_CENTER, "Drop files here", title_font(24.0), GREEN);
        painter.text(
            c + Vec2::new(0.0, 36.0),
            Align2::CENTER_CENTER,
            "or click to choose them. Folders work too.",
            mono(12.5),
            TEXT_DIM,
        );
        let rows = [
            ("documents", "pdf  docx  md  html  txt", GREEN),
            ("images", "png  jpg  webp  bmp  gif  tiff  ico  tga", GOLD),
            ("data", "json  yaml  toml  xml", GREEN_SOFT),
            ("tables", "csv  xlsx  xls  ods", Color32::from_rgb(236, 206, 110)),
        ];
        let mut y = c.y + 76.0;
        for (label, formats, color) in rows {
            painter.text(Pos2::new(c.x - 30.0, y), Align2::RIGHT_CENTER, label, mono(12.0), TEXT_DIM);
            painter.text(Pos2::new(c.x - 14.0, y), Align2::LEFT_CENTER, formats, mono(12.5), color);
            y += 22.0;
        }
        if response.clicked() {
            self.pick_files();
        }
    }

    fn queue(&mut self, ui: &mut Ui) {
        self.toolbar(ui);
        if self.jobs.is_empty() {
            self.drop_zone(ui);
            return;
        }
        let running = self.is_running();
        let time = ui.input(|i| i.time);
        let mut remove: Option<u64> = None;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for job in &mut self.jobs {
                job_row(ui, job, running, time, &mut remove);
                ui.add_space(5.0);
            }
        });
        if let Some(id) = remove {
            self.jobs.retain(|j| j.id != id);
        }
    }

    fn terminal(&mut self, ui: &mut Ui) {
        let time = ui.input(|i| i.time);
        let frame = Frame::none()
            .fill(Color32::from_rgba_unmultiplied(1, 8, 3, 236))
            .stroke(Stroke::new(1.0, alpha(GREEN, 70)))
            .inner_margin(Margin::same(12.0));
        let response = frame.show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.horizontal(|ui| {
                caption(ui, "terminal", GREEN);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if neon_button(ui, "Clear", TEXT_DIM, !self.log.is_empty()).clicked() {
                        self.log.clear();
                    }
                });
            });
            let (line_rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
            ui.painter().line_segment(
                [line_rect.left_center(), line_rect.right_center()],
                Stroke::new(1.0, alpha(GREEN, 50)),
            );
            ui.push_id("terminal_scroll", |ui| {
                ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    for line in &self.log {
                        let (tag, color) = match line.kind {
                            LogKind::Info => ("[..]", TEXT_DIM),
                            LogKind::Ok => ("[ok]", GREEN),
                            LogKind::Warn => ("[!!]", GOLD),
                            LogKind::Err => ("[err]", RED),
                        };
                        let text_color = match line.kind {
                            LogKind::Err => Color32::from_rgb(255, 160, 170),
                            LogKind::Ok => TEXT,
                            _ => Color32::from_rgb(170, 210, 178),
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(RichText::new(format!("{:>8.3}", line.t)).font(mono(11.0)).color(alpha(GREEN, 80)));
                            ui.label(RichText::new(tag).font(mono(11.5)).color(color));
                            ui.label(RichText::new(&line.text).font(mono(11.5)).color(text_color));
                        });
                    }
                    let cursor = if (time * 1.6) as i64 % 2 == 0 { "\u{2588}" } else { " " };
                    ui.label(RichText::new(format!("friendly@converter:~$ {cursor}")).font(mono(11.5)).color(GREEN));
                });
            });
        });
        corners(ui.painter(), response.response.rect, GOLD, 12.0);
    }

    fn footer(&mut self, ui: &mut Ui) {
        let (line_rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 4.0), Sense::hover());
        ui.painter().line_segment([line_rect.left_top(), line_rect.right_top()], Stroke::new(1.0, alpha(GREEN, 60)));
        ui.horizontal_wrapped(|ui| {
            caption(ui, "save to", GOLD);
            let same = self.opts.out_dir.is_none();
            if ui.selectable_label(same, "next to the source file").clicked() {
                self.opts.out_dir = None;
            }
            let folder_label = match &self.opts.out_dir {
                Some(d) => shorten(&d.display().to_string(), 36),
                None => "a folder...".to_string(),
            };
            if ui.selectable_label(!same, folder_label).on_hover_text("Choose the output folder").clicked() {
                if let Some(dir) = rfd::FileDialog::new().set_title("Output folder").pick_folder() {
                    self.push_log(LogKind::Info, format!("output folder: {}", dir.display()));
                    self.opts.out_dir = Some(dir);
                }
            }
            ui.add_space(10.0);
            caption(ui, "csv separator", GOLD);
            let delim_label = match self.opts.csv_delimiter {
                b';' => "; (Excel RU/EU)",
                b',' => ", (comma)",
                b'\t' => "tab",
                _ => "?",
            };
            egui::ComboBox::new("csv_delimiter", "").width(140.0).selected_text(delim_label).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.opts.csv_delimiter, b';', "; (Excel RU/EU)");
                ui.selectable_value(&mut self.opts.csv_delimiter, b',', ", (comma)");
                ui.selectable_value(&mut self.opts.csv_delimiter, b'\t', "tab");
            });
            ui.add_space(10.0);
            caption(ui, "jpeg quality", GOLD);
            ui.add(egui::Slider::new(&mut self.opts.jpeg_quality, 50..=100));
            ui.add_space(10.0);
            ui.checkbox(&mut self.animate, "signal animation");

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                hint(ui, &format!("v{VERSION}"));
                if neon_button(ui, "Open output folder", GREEN, true).clicked() {
                    let dir = self
                        .opts
                        .out_dir
                        .clone()
                        .or_else(|| self.jobs.first().and_then(|j| j.path.parent().map(|p| p.to_path_buf())));
                    match dir {
                        Some(d) => open_in_file_manager(&d),
                        None => self.push_log(LogKind::Warn, "no output folder yet: add files first".into()),
                    }
                }
            });
        });
    }

    fn drop_overlay(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("drop_overlay")));
        let screen = ctx.screen_rect();
        painter.rect_filled(screen, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 195));
        let r = screen.shrink(36.0);
        painter.rect_stroke(r, 0.0, Stroke::new(2.0, GREEN));
        corners(&painter, r.expand(6.0), GOLD, 28.0);
        glow_text(&painter, screen.center(), Align2::CENTER_CENTER, "Release to load the files", title_font(28.0), GREEN);
    }
}

fn draw_logo(painter: &egui::Painter, chip: Rect) {
    for i in 0..4 {
        let t = chip.top() + 8.0 + i as f32 * 7.5;
        painter.line_segment([Pos2::new(chip.left() - 6.0, t), Pos2::new(chip.left(), t)], Stroke::new(1.6, GOLD));
        painter.line_segment([Pos2::new(chip.right(), t), Pos2::new(chip.right() + 6.0, t)], Stroke::new(1.6, GOLD));
    }
    painter.rect_filled(chip, 0.0, Color32::from_rgb(8, 18, 10));
    painter.rect_stroke(chip, 0.0, Stroke::new(1.6, GOLD));
    glow_text(painter, chip.center(), Align2::CENTER_CENTER, "FC", title_font(16.0), GREEN);
}

fn status_view(ui: &mut Ui, status: &Status, time: f64) {
    match status {
        Status::Ready => {
            ui.label(RichText::new("ready").color(TEXT_DIM));
        }
        Status::Queued => {
            ui.label(RichText::new("queued").color(GOLD_DIM));
        }
        Status::Running => {
            let frames = ["|", "/", "-", "\\"];
            let f = frames[((time * 10.0) as usize) % frames.len()];
            ui.label(RichText::new(format!("{f} working")).color(GREEN));
        }
        Status::Done { secs, .. } => {
            ui.label(RichText::new(format!("done {secs:.1}s")).color(GREEN));
        }
        Status::Failed(e) => {
            ui.label(RichText::new("failed").color(RED)).on_hover_text(e.as_str());
        }
    }
}

fn job_row(ui: &mut Ui, job: &mut Job, running: bool, time: f64, remove: &mut Option<u64>) {
    let accent = category_color(job.from);
    let border = match job.status {
        Status::Done { .. } => alpha(GREEN, 120),
        Status::Failed(_) => alpha(RED, 140),
        Status::Running => alpha(GOLD, 200),
        _ => alpha(GREEN, 45),
    };
    let busy = matches!(job.status, Status::Queued | Status::Running);
    let response = Frame::none()
        .fill(panel_fill())
        .stroke(Stroke::new(1.0, border))
        .inner_margin(Margin::symmetric(10.0, 7.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                badge(ui, job.from.label(), accent);
                let right_part = 360.0;
                let name_w = (ui.available_width() - right_part).max(110.0);
                ui.allocate_ui_with_layout(Vec2::new(name_w, 34.0), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(egui::Label::new(RichText::new(&job.name).color(TEXT)).truncate())
                        .on_hover_text(job.path.display().to_string());
                    let sub = match &job.status {
                        Status::Failed(e) => RichText::new(e.as_str()).color(Color32::from_rgb(255, 140, 150)),
                        Status::Done { outputs, .. } => {
                            let first = outputs.first().map(|p| file_name(p)).unwrap_or_default();
                            let extra = if outputs.len() > 1 { format!(" +{}", outputs.len() - 1) } else { String::new() };
                            RichText::new(format!("{}  ->  {first}{extra}", human_size(job.size))).color(TEXT_DIM)
                        }
                        _ => RichText::new(human_size(job.size)).color(TEXT_DIM),
                    };
                    ui.add(egui::Label::new(sub.font(mono(10.5))).truncate());
                });
                ui.label(RichText::new("->").color(alpha(GREEN, 170)));
                ui.add_enabled_ui(!running && !busy, |ui| {
                    egui::ComboBox::new(("target", job.id), "")
                        .width(104.0)
                        .selected_text(RichText::new(job.target.label()).color(GOLD))
                        .show_ui(ui, |ui| {
                            for t in job.from.targets() {
                                let text = if t.hint().is_empty() {
                                    t.label()
                                } else {
                                    format!("{}  ({})", t.label(), t.hint())
                                };
                                if ui.selectable_label(job.target == t, text).clicked() {
                                    job.target = t;
                                    if matches!(job.status, Status::Done { .. } | Status::Failed(_)) {
                                        job.status = Status::Ready;
                                    }
                                }
                            }
                        });
                });
                ui.allocate_ui_with_layout(Vec2::new(92.0, 26.0), Layout::left_to_right(Align::Center), |ui| {
                    status_view(ui, &job.status, time);
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if neon_button(ui, "x", RED, !running && !busy).on_hover_text("Remove from the list").clicked() {
                        *remove = Some(job.id);
                    }
                    if let Status::Done { outputs, .. } = &job.status {
                        if let Some(first) = outputs.first() {
                            if neon_button(ui, "Show", GREEN, true).on_hover_text("Show the result in Explorer").clicked() {
                                reveal_file(first);
                            }
                        }
                    }
                });
            });
        });
    if matches!(job.status, Status::Running) {
        corners(ui.painter(), response.response.rect, GOLD, 8.0);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.handle_input(ctx);

        let intro = (self.start.elapsed().as_secs_f32() / 1.2).min(1.0);
        let time = ctx.input(|i| i.time) as f32;
        let screen = ctx.screen_rect();
        let background = ctx.layer_painter(LayerId::background());
        self.circuit.paint(&background, screen, time, self.animate, 0.2 + 0.8 * intro);

        egui::TopBottomPanel::top("header")
            .show_separator_line(false)
            .frame(Frame::none().inner_margin(Margin { left: 20.0, right: 20.0, top: 14.0, bottom: 6.0 }))
            .show(ctx, |ui| self.header(ui, intro));
        egui::TopBottomPanel::bottom("footer")
            .show_separator_line(false)
            .frame(Frame::none().inner_margin(Margin { left: 20.0, right: 20.0, top: 4.0, bottom: 10.0 }))
            .show(ctx, |ui| self.footer(ui));
        egui::SidePanel::right("terminal")
            .resizable(true)
            .show_separator_line(false)
            .default_width(400.0)
            .min_width(280.0)
            .max_width(720.0)
            .frame(Frame::none().inner_margin(Margin { left: 6.0, right: 20.0, top: 4.0, bottom: 6.0 }))
            .show(ctx, |ui| self.terminal(ui));
        egui::CentralPanel::default()
            .frame(Frame::none().inner_margin(Margin { left: 20.0, right: 10.0, top: 4.0, bottom: 6.0 }))
            .show(ctx, |ui| self.queue(ui));

        self.drop_overlay(ctx);

        if self.animate || self.is_running() || intro < 1.0 {
            ctx.request_repaint_after(Duration::from_millis(33));
        } else {
            // Keep the blinking cursors alive at a low rate.
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}
