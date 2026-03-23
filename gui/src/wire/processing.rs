use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use slint::ComponentHandle;
use thumbnailer_core::{ThumbnailConfig, process_video};

use crate::config;
use crate::state::AppCtx;
use crate::util::ui_update;
use crate::{AppWindow, Callabler, GuiState};

pub fn wire(win: &AppWindow, ctx: Rc<AppCtx>) {
    let cb = win.global::<Callabler>();

    cb.on_stop_processing({
        let stop = ctx.stop_flag.clone();
        move || stop.store(true, Ordering::Relaxed)
    });

    cb.on_start({
        let weak = win.as_weak();
        let ctx = ctx.clone();
        move || {
            let win = weak.upgrade().expect("AppWindow destroyed during start()");
            start_processing(&win, &ctx);
        }
    });
}

fn start_processing(win: &AppWindow, ctx: &Rc<AppCtx>) {
    let files: Vec<PathBuf> = ctx.files.borrow().clone();
    if files.is_empty() {
        return;
    }
    let config = config::build(win);

    ctx.stop_flag.store(false, Ordering::Relaxed);
    let gs = win.global::<GuiState>();
    gs.set_is_processing(true);
    gs.set_progress(0.0);
    gs.set_log_text("".into());
    gs.set_status_text("Starting…".into());

    let stop = ctx.stop_flag.clone();
    let weak = win.as_weak();

    std::thread::spawn(move || worker(weak, files, config, stop));
}

fn worker(
    weak: slint::Weak<AppWindow>,
    files: Vec<PathBuf>,
    config: ThumbnailConfig,
    stop: Arc<AtomicBool>,
) {
    let total = files.len();
    let mut success = 0usize;
    let mut failed = 0usize;

    for (i, path) in files.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            let msg = format!(
                "Stopped by user. {}/{} processed ({} ok, {} failed).",
                success + failed,
                total,
                success,
                failed,
            );
            ui_update(&weak, move |win| {
                let gs = win.global::<GuiState>();
                gs.set_status_text(msg.into());
                gs.set_is_processing(false);
            });
            return;
        }

        let name = file_display_name(path);
        push_initial_status(&weak, i, total, &name);

        let result = process_video(&config, path, &stop, &|frac, msg| {
            report_progress(&weak, i, total, frac, &name, msg);
        });

        if result.error.is_some() {
            failed += 1;
        } else {
            success += 1;
        }

        let line = format_result_line(&name, &result);
        append_log(&weak, line);
    }

    let msg = if failed == 0 {
        format!("Done! {success}/{total} succeeded.")
    } else {
        format!("Done! {success}/{total} succeeded, {failed} failed.")
    };
    ui_update(&weak, move |win| {
        let gs = win.global::<GuiState>();
        gs.set_is_processing(false);
        gs.set_progress(1.0);
        gs.set_status_text(msg.into());
    });
}

fn file_display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

fn push_initial_status(weak: &slint::Weak<AppWindow>, i: usize, total: usize, name: &str) {
    let s = format!("[{}/{}] {}…", i + 1, total, name);
    ui_update(weak, move |win| {
        win.global::<GuiState>().set_status_text(s.into())
    });
}

fn report_progress(
    weak: &slint::Weak<AppWindow>,
    i: usize,
    total: usize,
    frac: f32,
    name: &str,
    msg: &str,
) {
    let overall = ((i as f32 + frac) / total as f32).clamp(0.0, 1.0);
    let s = format!("[{}/{}] {} — {}", i + 1, total, name, msg);
    ui_update(weak, move |win| {
        let gs = win.global::<GuiState>();
        gs.set_progress(overall);
        gs.set_status_text(s.into());
    });
}

fn format_result_line(name: &str, result: &thumbnailer_core::ProcessingResult) -> String {
    match &result.error {
        Some(err) => format!("✗ {name}: {err}"),
        None => format!("✓ {name}: {} output file(s)", result.output_files.len()),
    }
}

fn append_log(weak: &slint::Weak<AppWindow>, line: String) {
    ui_update(weak, move |win| {
        let gs = win.global::<GuiState>();
        let cur = gs.get_log_text().to_string();
        let new = if cur.is_empty() {
            line
        } else {
            format!("{cur}\n{line}")
        };
        gs.set_log_text(new.into());
    });
}
