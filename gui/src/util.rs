use std::path::{Path, PathBuf};
use std::process::Command;

use crate::AppWindow;

pub const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts", "mts", "m2ts",
    "3gp", "ogv", "rmvb", "vob", "divx",
];

pub fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn collect_from_dir(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_video(p))
        .collect()
}

pub fn open_url(url: &str) {
    #[cfg(target_os = "windows")]
    let result = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(url).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let result = Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = result {
        eprintln!("Failed to open URL '{url}': {e}");
    }
}

/// Run a closure on the Slint event-loop thread (safe to call from any thread).
/// Logs and returns if the window has been destroyed.
pub fn ui_update(weak: &slint::Weak<AppWindow>, f: impl FnOnce(AppWindow) + Send + 'static) {
    let weak = weak.clone();
    slint::invoke_from_event_loop(move || {
        let Some(win) = weak.upgrade() else {
            eprintln!("AppWindow gone — skipping UI update");
            return;
        };
        f(win);
    })
    .ok();
}
