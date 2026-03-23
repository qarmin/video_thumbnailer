use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use slint::ComponentHandle;

use crate::{
    AppWindow, BarLayoutMode, BarSide, FontSize, OutputFormat, Settings as SlintSettings, ThumbMode,
    TimestampCorner,
};

/// Persisted user preferences. `#[serde(default)]` lets old configs load
/// after new fields are added.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedSettings {
    pub output_dir: String,
    pub output_format: i32,
    pub quality: i32,
    pub frame_max_width: i32,
    pub frame_max_height: i32,
    pub mode: i32,
    pub seek_percent: i32,
    pub grid_cols: i32,
    pub grid_rows: i32,
    pub frame_count: i32,
    pub overwrite: bool,
    pub prefix: String,
    pub show_timestamp: bool,
    pub timestamp_pos: i32,
    pub timestamp_size_mode: i32,
    pub timestamp_size: i32,
    pub timestamp_size_percent: i32,
    pub show_meta_bar: bool,
    pub bar_pos: i32,
    pub bar_layout: i32,
    pub bar_size_mode: i32,
    pub bar_size: i32,
    pub bar_size_percent: i32,
    pub show_branding: bool,
    pub meta_filename: bool,
    pub meta_duration: bool,
    pub meta_fps: bool,
    pub meta_resolution: bool,
    pub meta_filesize: bool,
    pub meta_timestamp: bool,
    pub meta_codec: bool,
    pub meta_bitrate: bool,
    pub dark_theme: bool,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            output_dir: String::new(),
            output_format: 0,
            quality: 85,
            frame_max_width: 1920,
            frame_max_height: 1080,
            mode: 0,
            seek_percent: 10,
            grid_cols: 3,
            grid_rows: 3,
            frame_count: 5,
            overwrite: true,
            prefix: String::new(),
            show_timestamp: false,
            timestamp_pos: 0,
            timestamp_size_mode: 0,
            timestamp_size: 28,
            timestamp_size_percent: 5,
            show_meta_bar: false,
            bar_pos: 0,
            bar_layout: 0,
            bar_size_mode: 0,
            bar_size: 22,
            bar_size_percent: 5,
            show_branding: true,
            meta_filename: true,
            meta_duration: true,
            meta_fps: false,
            meta_resolution: false,
            meta_filesize: false,
            meta_timestamp: false,
            meta_codec: false,
            meta_bitrate: false,
            dark_theme: false,
        }
    }
}

fn settings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("vthumb-gui")
        .join("settings.json")
}

pub fn load() -> PersistedSettings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

pub fn save(win: &AppWindow) {
    let s = win.global::<SlintSettings>();
    let snapshot = PersistedSettings {
        output_dir: s.get_output_dir().to_string(),
        output_format: format_to_int(s.get_output_format()),
        quality: s.get_quality(),
        frame_max_width: s.get_frame_max_width(),
        frame_max_height: s.get_frame_max_height(),
        mode: mode_to_int(s.get_mode()),
        seek_percent: s.get_seek_percent(),
        grid_cols: s.get_grid_cols(),
        grid_rows: s.get_grid_rows(),
        frame_count: s.get_frame_count(),
        overwrite: s.get_overwrite(),
        prefix: s.get_prefix().to_string(),
        show_timestamp: s.get_show_timestamp(),
        timestamp_pos: ts_corner_to_int(s.get_timestamp_pos()),
        timestamp_size_mode: font_size_to_int(s.get_timestamp_size_mode()),
        timestamp_size: s.get_timestamp_size(),
        timestamp_size_percent: s.get_timestamp_size_percent(),
        show_meta_bar: s.get_show_meta_bar(),
        bar_pos: bar_side_to_int(s.get_bar_pos()),
        bar_layout: bar_layout_to_int(s.get_bar_layout()),
        bar_size_mode: font_size_to_int(s.get_bar_size_mode()),
        bar_size: s.get_bar_size(),
        bar_size_percent: s.get_bar_size_percent(),
        show_branding: s.get_show_branding(),
        meta_filename: s.get_meta_filename(),
        meta_duration: s.get_meta_duration(),
        meta_fps: s.get_meta_fps(),
        meta_resolution: s.get_meta_resolution(),
        meta_filesize: s.get_meta_filesize(),
        meta_timestamp: s.get_meta_timestamp(),
        meta_codec: s.get_meta_codec(),
        meta_bitrate: s.get_meta_bitrate(),
        dark_theme: s.get_dark_theme(),
    };

    let path = settings_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(&snapshot) {
        let _ = std::fs::write(path, json);
    }
}

pub fn apply(win: &AppWindow, p: &PersistedSettings) {
    let s = win.global::<SlintSettings>();
    s.set_output_dir(p.output_dir.clone().into());
    s.set_output_format(int_to_format(p.output_format));
    s.set_quality(p.quality);
    s.set_frame_max_width(p.frame_max_width);
    s.set_frame_max_height(p.frame_max_height);
    s.set_mode(int_to_mode(p.mode));
    s.set_seek_percent(p.seek_percent);
    s.set_grid_cols(p.grid_cols);
    s.set_grid_rows(p.grid_rows);
    s.set_frame_count(p.frame_count);
    s.set_overwrite(p.overwrite);
    s.set_prefix(p.prefix.clone().into());
    s.set_show_timestamp(p.show_timestamp);
    s.set_timestamp_pos(int_to_ts_corner(p.timestamp_pos));
    s.set_timestamp_size_mode(int_to_font_size(p.timestamp_size_mode));
    s.set_timestamp_size(p.timestamp_size);
    s.set_timestamp_size_percent(p.timestamp_size_percent);
    s.set_show_meta_bar(p.show_meta_bar);
    s.set_bar_pos(int_to_bar_side(p.bar_pos));
    s.set_bar_layout(int_to_bar_layout(p.bar_layout));
    s.set_bar_size_mode(int_to_font_size(p.bar_size_mode));
    s.set_bar_size(p.bar_size);
    s.set_bar_size_percent(p.bar_size_percent);
    s.set_show_branding(p.show_branding);
    s.set_meta_filename(p.meta_filename);
    s.set_meta_duration(p.meta_duration);
    s.set_meta_fps(p.meta_fps);
    s.set_meta_resolution(p.meta_resolution);
    s.set_meta_filesize(p.meta_filesize);
    s.set_meta_timestamp(p.meta_timestamp);
    s.set_meta_codec(p.meta_codec);
    s.set_meta_bitrate(p.meta_bitrate);
    s.set_dark_theme(p.dark_theme);
}

//  enum <-> int (storage format)

fn format_to_int(f: OutputFormat) -> i32 {
    match f {
        OutputFormat::Jpeg => 0,
        OutputFormat::Png => 1,
        OutputFormat::Webp => 2,
    }
}
fn int_to_format(i: i32) -> OutputFormat {
    match i {
        1 => OutputFormat::Png,
        2 => OutputFormat::Webp,
        _ => OutputFormat::Jpeg,
    }
}

fn mode_to_int(m: ThumbMode) -> i32 {
    match m {
        ThumbMode::Single => 0,
        ThumbMode::Grid => 1,
        ThumbMode::Sequence => 2,
    }
}
fn int_to_mode(i: i32) -> ThumbMode {
    match i {
        1 => ThumbMode::Grid,
        2 => ThumbMode::Sequence,
        _ => ThumbMode::Single,
    }
}

fn ts_corner_to_int(c: TimestampCorner) -> i32 {
    match c {
        TimestampCorner::BottomRight => 0,
        TimestampCorner::BottomLeft => 1,
        TimestampCorner::TopRight => 2,
        TimestampCorner::TopLeft => 3,
    }
}
fn int_to_ts_corner(i: i32) -> TimestampCorner {
    match i {
        1 => TimestampCorner::BottomLeft,
        2 => TimestampCorner::TopRight,
        3 => TimestampCorner::TopLeft,
        _ => TimestampCorner::BottomRight,
    }
}

fn bar_side_to_int(b: BarSide) -> i32 {
    match b {
        BarSide::Top => 0,
        BarSide::Bottom => 1,
    }
}
fn int_to_bar_side(i: i32) -> BarSide {
    match i {
        1 => BarSide::Bottom,
        _ => BarSide::Top,
    }
}

fn bar_layout_to_int(l: BarLayoutMode) -> i32 {
    match l {
        BarLayoutMode::Horizontal => 0,
        BarLayoutMode::Vertical => 1,
    }
}
fn int_to_bar_layout(i: i32) -> BarLayoutMode {
    match i {
        1 => BarLayoutMode::Vertical,
        _ => BarLayoutMode::Horizontal,
    }
}

fn font_size_to_int(f: FontSize) -> i32 {
    match f {
        FontSize::Auto => 0,
        FontSize::Pixels => 1,
        FontSize::Percent => 2,
    }
}
fn int_to_font_size(i: i32) -> FontSize {
    match i {
        1 => FontSize::Pixels,
        2 => FontSize::Percent,
        _ => FontSize::Auto,
    }
}
