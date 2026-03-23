use std::path::PathBuf;

use slint::ComponentHandle;
use thumbnailer_core::{
    self as core, BarLayout, BarPosition, FontSizing, MetadataField, OverlayConfig,
    ThumbnailConfig, ThumbnailMode, TimestampPosition,
};

use crate::{
    AppWindow, BarLayoutMode, BarSide, FontSize, OutputFormat, Settings, ThumbMode,
    TimestampCorner,
};

pub fn build(win: &AppWindow) -> ThumbnailConfig {
    let s = win.global::<Settings>();

    let mode = match s.get_mode() {
        ThumbMode::Single => ThumbnailMode::Single {
            seek_percent: s.get_seek_percent() as f64,
        },
        ThumbMode::Grid => ThumbnailMode::Grid {
            cols: s.get_grid_cols() as u32,
            rows: s.get_grid_rows() as u32,
        },
        ThumbMode::Sequence => ThumbnailMode::Sequence {
            count: s.get_frame_count() as u32,
        },
    };

    let format = match s.get_output_format() {
        OutputFormat::Jpeg => core::OutputFormat::Jpg,
        OutputFormat::Png => core::OutputFormat::Png,
        OutputFormat::Webp => core::OutputFormat::Webp,
    };

    let output_dir = {
        let dir = s.get_output_dir().to_string();
        if dir.is_empty() {
            None
        } else {
            Some(PathBuf::from(dir))
        }
    };

    ThumbnailConfig {
        output_dir,
        mode,
        max_width: s.get_frame_max_width() as u32,
        max_height: s.get_frame_max_height() as u32,
        format,
        quality: s.get_quality() as u8,
        overwrite: s.get_overwrite(),
        output_prefix: s.get_prefix().to_string(),
        overlay: build_overlay(win),
        parallel: true,
    }
}

fn build_overlay(win: &AppWindow) -> OverlayConfig {
    let s = win.global::<Settings>();

    let timestamp_position = match s.get_timestamp_pos() {
        TimestampCorner::BottomRight => TimestampPosition::BottomRight,
        TimestampCorner::BottomLeft => TimestampPosition::BottomLeft,
        TimestampCorner::TopRight => TimestampPosition::TopRight,
        TimestampCorner::TopLeft => TimestampPosition::TopLeft,
    };
    let bar_position = match s.get_bar_pos() {
        BarSide::Top => BarPosition::Top,
        BarSide::Bottom => BarPosition::Bottom,
    };
    let bar_layout = match s.get_bar_layout() {
        BarLayoutMode::Horizontal => BarLayout::Horizontal,
        BarLayoutMode::Vertical => BarLayout::Vertical,
    };

    OverlayConfig {
        show_timestamp: s.get_show_timestamp(),
        timestamp_position,
        timestamp_font_sizing: map_font_sizing(s.get_timestamp_size_mode()),
        timestamp_font_size: s.get_timestamp_size() as u32,
        timestamp_font_size_percent: s.get_timestamp_size_percent() as f32,
        show_metadata_bar: s.get_show_meta_bar(),
        bar_position,
        bar_layout,
        bar_font_sizing: map_font_sizing(s.get_bar_size_mode()),
        bar_font_size: s.get_bar_size() as u32,
        bar_font_size_percent: s.get_bar_size_percent() as f32,
        metadata_fields: collect_fields(win),
        show_branding: s.get_show_branding(),
    }
}

fn map_font_sizing(f: FontSize) -> FontSizing {
    match f {
        FontSize::Auto => FontSizing::Auto,
        FontSize::Pixels => FontSizing::Pixels,
        FontSize::Percent => FontSizing::Percent,
    }
}

fn collect_fields(win: &AppWindow) -> Vec<MetadataField> {
    let s = win.global::<Settings>();
    let mut out = Vec::new();
    if s.get_meta_filename()   { out.push(MetadataField::Filename); }
    if s.get_meta_duration()   { out.push(MetadataField::Duration); }
    if s.get_meta_fps()        { out.push(MetadataField::Fps); }
    if s.get_meta_resolution() { out.push(MetadataField::Resolution); }
    if s.get_meta_filesize()   { out.push(MetadataField::FileSize); }
    if s.get_meta_timestamp()  { out.push(MetadataField::Timestamp); }
    if s.get_meta_codec()      { out.push(MetadataField::Codec); }
    if s.get_meta_bitrate()    { out.push(MetadataField::Bitrate); }
    out
}
