slint::include_modules!();

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use slint::{StandardListViewItem, VecModel};
use thumbnailer_core::{
    BarPosition, MetadataField, OutputFormat, OverlayConfig, ThumbnailConfig, ThumbnailMode,
    TimestampPosition, check_ffmpeg, process_video,
};

const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts", "mts", "m2ts",
    "3gp", "ogv", "rmvb", "vob", "divx",
];

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn collect_from_dir(dir: &PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && is_video(&p) {
                out.push(p);
            }
        }
    }
    out
}

fn main() {
    let window = AppWindow::new().expect("Failed to create window");

    if !check_ffmpeg() {
        window.set_status_text(
            "WARNING: ffmpeg / ffprobe not found in PATH. Processing will fail.".into(),
        );
    }

    // ── Shared state ─────────────────────────────────────────────────────────
    // The actual PathBuf list (Slint's model only holds display strings).
    let files_store: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));

    // The Slint list-view model.
    let file_model = Rc::new(VecModel::<StandardListViewItem>::default());
    window.set_file_items(file_model.clone().into());

    // Global stop flag shared between the UI and the processing thread.
    let stop_flag: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    // ── Helper: append a path to both the store and the visual model ──────────
    let add_path = {
        let files_store = Rc::clone(&files_store);
        let file_model = Rc::clone(&file_model);
        move |path: PathBuf| {
            // Avoid duplicates.
            if files_store.borrow().contains(&path) {
                return;
            }
            let mut item = StandardListViewItem::default();
            item.text = path.to_string_lossy().to_string().into();
            file_model.push(item);
            files_store.borrow_mut().push(path);
        }
    };

    // ── Callback: Add Files ───────────────────────────────────────────────────
    window.on_add_files({
        let add_path = add_path.clone();
        move || {
            if let Some(paths) = rfd::FileDialog::new()
                .add_filter(
                    "Videos",
                    &[
                        "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg",
                        "ts", "mts", "m2ts", "3gp", "ogv", "rmvb", "vob", "divx",
                    ],
                )
                .add_filter("All files", &["*"])
                .pick_files()
            {
                for p in paths {
                    add_path(p);
                }
            }
        }
    });

    // ── Callback: Add Folder ─────────────────────────────────────────────────
    window.on_add_folder({
        let add_path = add_path.clone();
        move || {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                for p in collect_from_dir(&dir) {
                    add_path(p);
                }
            }
        }
    });

    // ── Callback: Clear ───────────────────────────────────────────────────────
    window.on_clear_files({
        let files_store = Rc::clone(&files_store);
        let file_model = Rc::clone(&file_model);
        move || {
            files_store.borrow_mut().clear();
            file_model.set_vec(Vec::new());
        }
    });

    // ── Callback: Select output directory ────────────────────────────────────
    window.on_select_output_dir({
        let window_weak = window.as_weak();
        move || {
            if let Some(dir) = rfd::FileDialog::new().pick_folder()
                && let Some(win) = window_weak.upgrade()
            {
                win.set_output_dir(dir.to_string_lossy().to_string().into());
            }
        }
    });

    // ── Callback: Stop ────────────────────────────────────────────────────────
    window.on_stop_processing({
        let stop_flag = Arc::clone(&stop_flag);
        move || {
            stop_flag.store(true, Ordering::Relaxed);
        }
    });

    // ── Callback: Start ───────────────────────────────────────────────────────
    window.on_start({
        let window_weak = window.as_weak();
        let files_store = Rc::clone(&files_store);
        let stop_flag = Arc::clone(&stop_flag);

        move || {
            let Some(win) = window_weak.upgrade() else {
                return;
            };

            // ── Read config from the window ───────────────────────────────────
            let mode_idx = win.get_mode();
            let fmt_idx = win.get_output_format();
            let quality = win.get_quality() as u8;
            let max_width = win.get_frame_max_width() as u32;
            let max_height = win.get_frame_max_height() as u32;
            let seek_pct = win.get_seek_percent() as f64;
            let grid_cols = win.get_grid_cols() as u32;
            let grid_rows = win.get_grid_rows() as u32;
            let frame_count = win.get_frame_count() as u32;
            let overwrite = win.get_overwrite();
            let prefix = win.get_prefix().to_string();
            let out_dir_str = win.get_output_dir().to_string();

            let mode = match mode_idx {
                1 => ThumbnailMode::Grid {
                    cols: grid_cols,
                    rows: grid_rows,
                },
                2 => ThumbnailMode::Sequence { count: frame_count },
                _ => ThumbnailMode::Single {
                    seek_percent: seek_pct,
                },
            };
            let format = match fmt_idx {
                1 => OutputFormat::Png,
                2 => OutputFormat::Webp,
                _ => OutputFormat::Jpg,
            };
            let output_dir = if out_dir_str.is_empty() {
                None
            } else {
                Some(PathBuf::from(out_dir_str))
            };

            // ── Overlay config ────────────────────────────────────────────────
            let ts_position = match win.get_timestamp_pos() {
                1 => TimestampPosition::BottomLeft,
                2 => TimestampPosition::TopRight,
                3 => TimestampPosition::TopLeft,
                _ => TimestampPosition::BottomRight,
            };
            let b_pos = match win.get_bar_pos() {
                1 => BarPosition::Bottom,
                _ => BarPosition::Top,
            };
            let mut meta_fields: Vec<MetadataField> = Vec::new();
            if win.get_meta_filename() {
                meta_fields.push(MetadataField::Filename);
            }
            if win.get_meta_duration() {
                meta_fields.push(MetadataField::Duration);
            }
            if win.get_meta_fps() {
                meta_fields.push(MetadataField::Fps);
            }
            if win.get_meta_resolution() {
                meta_fields.push(MetadataField::Resolution);
            }
            if win.get_meta_filesize() {
                meta_fields.push(MetadataField::FileSize);
            }
            if win.get_meta_timestamp() {
                meta_fields.push(MetadataField::Timestamp);
            }
            if win.get_meta_codec() {
                meta_fields.push(MetadataField::Codec);
            }
            if win.get_meta_bitrate() {
                meta_fields.push(MetadataField::Bitrate);
            }
            let overlay = OverlayConfig {
                show_timestamp: win.get_show_timestamp(),
                timestamp_position: ts_position,
                timestamp_font_size: win.get_timestamp_size() as u32,
                show_metadata_bar: win.get_show_meta_bar(),
                bar_position: b_pos,
                bar_font_size: win.get_bar_size() as u32,
                metadata_fields: meta_fields,
            };

            let config = ThumbnailConfig {
                output_dir,
                mode,
                max_width,
                max_height,
                format,
                quality,
                overwrite,
                output_prefix: prefix,
                overlay,
            };

            let files: Vec<PathBuf> = files_store.borrow().clone();
            if files.is_empty() {
                return;
            }

            // Reset state
            stop_flag.store(false, Ordering::Relaxed);
            win.set_is_processing(true);
            win.set_progress(0.0);
            win.set_log_text("".into());
            win.set_status_text("Starting…".into());

            let stop = Arc::clone(&stop_flag);
            let window_weak = window_weak.clone();

            std::thread::spawn(move || {
                let total = files.len();

                for (i, path) in files.iter().enumerate() {
                    if stop.load(Ordering::Relaxed) {
                        ui_update(&window_weak, |win| {
                            win.set_status_text("Stopped by user.".into());
                            win.set_is_processing(false);
                        });
                        return;
                    }

                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    // Initial status for this file
                    {
                        let s = format!("[{}/{}] {}…", i + 1, total, name);
                        let ww = window_weak.clone();
                        ui_update(&ww, move |win| win.set_status_text(s.into()));
                    }

                    let result = process_video(&config, path, &stop, &|frac, msg| {
                        let overall = (i as f32 + frac) / total as f32;
                        let s = format!("[{}/{}] {} — {}", i + 1, total, name, msg);
                        let ww = window_weak.clone();
                        ui_update(&ww, move |win| {
                            win.set_progress(overall);
                            win.set_status_text(s.into());
                        });
                    });

                    // Append result to log
                    let log_line = if let Some(err) = &result.error {
                        format!("✗ {name}: {err}")
                    } else {
                        format!("✓ {name}: {} output file(s)", result.output_files.len())
                    };
                    let ww = window_weak.clone();
                    ui_update(&ww, move |win| {
                        let cur = win.get_log_text().to_string();
                        let new = if cur.is_empty() {
                            log_line
                        } else {
                            format!("{cur}\n{log_line}")
                        };
                        win.set_log_text(new.into());
                    });
                }

                // All done
                ui_update(&window_weak, |win| {
                    win.set_is_processing(false);
                    win.set_progress(1.0);
                    win.set_status_text("Done!".into());
                });
            });
        }
    });

    window.run().expect("Window run failed");
}

/// Run a closure on the Slint event-loop thread (safe to call from any thread).
fn ui_update(weak: &slint::Weak<AppWindow>, f: impl FnOnce(AppWindow) + Send + 'static) {
    let weak = weak.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(win) = weak.upgrade() {
            f(win);
        }
    })
    .ok();
}
