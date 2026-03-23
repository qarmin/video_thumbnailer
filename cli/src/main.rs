use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use clap::{Parser, ValueEnum};
use thumbnailer_core::{
    BarPosition, MetadataField, OutputFormat, OverlayConfig, ThumbnailConfig, ThumbnailMode,
    TimestampPosition, check_ffmpeg, process_video,
};

// ── CLI argument types ────────────────────────────────────────────────────────
#[derive(Clone, Debug, ValueEnum)]
enum CliFormat {
    Jpg,
    Png,
    Webp,
}

#[derive(Clone, Debug, ValueEnum)]
enum CliMode {
    /// Single frame at a given seek position.
    Single,
    /// NxM grid assembled into one image.
    Grid,
    /// Multiple individual frames saved as separate files.
    Sequence,
}

#[derive(Clone, Debug, ValueEnum)]
enum CliTsPos {
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

#[derive(Clone, Debug, ValueEnum)]
enum CliBarPos {
    Top,
    Bottom,
}

#[derive(Clone, Debug, ValueEnum)]
enum CliMetaField {
    Filename,
    Duration,
    Fps,
    Resolution,
    Filesize,
    Timestamp,
    Codec,
    Bitrate,
}

// ── Argument struct ───────────────────────────────────────────────────────────
#[derive(Parser, Debug)]
#[command(
    name = "vthumb",
    about = "Generate video thumbnails using ffmpeg",
    long_about = None,
)]
struct Args {
    /// Input video files (or directories when --recursive is set).
    #[arg(required = true, value_name = "FILE")]
    input: Vec<PathBuf>,

    /// Output directory.  Defaults to the directory containing each source file.
    #[arg(short, long, value_name = "DIR")]
    output_dir: Option<PathBuf>,

    /// Thumbnail mode.
    #[arg(short, long, default_value = "single", value_name = "MODE")]
    mode: CliMode,

    /// Output image format.
    #[arg(short, long, default_value = "jpg", value_name = "FORMAT")]
    format: CliFormat,

    /// Encoding quality for JPEG / WebP (1–100).
    #[arg(short, long, default_value_t = 85, value_name = "N")]
    quality: u8,

    /// Maximum frame width in pixels.
    #[arg(long, default_value_t = 1920, value_name = "PX")]
    max_width: u32,

    /// Maximum frame height in pixels.
    #[arg(long, default_value_t = 1080, value_name = "PX")]
    max_height: u32,

    /// Seek position as % of video duration (single mode, 0–100).
    #[arg(long, default_value_t = 10.0, value_name = "PCT")]
    seek_percent: f64,

    /// Grid columns (grid mode).
    #[arg(long, default_value_t = 3, value_name = "N")]
    grid_cols: u32,

    /// Grid rows (grid mode).
    #[arg(long, default_value_t = 3, value_name = "N")]
    grid_rows: u32,

    /// Number of frames to extract (sequence mode).
    #[arg(long, default_value_t = 5, value_name = "N")]
    frames: u32,

    /// Overwrite existing output files.
    #[arg(long)]
    overwrite: bool,

    /// Optional prefix added to every output filename.
    #[arg(long, default_value = "", value_name = "STRING")]
    prefix: String,

    /// Scan directories recursively for video files.
    #[arg(short, long)]
    recursive: bool,

    // ── Overlay ───────────────────────────────────────────────────────────────
    /// Stamp the current timestamp on each extracted frame.
    #[arg(long)]
    timestamp: bool,

    /// Corner where the timestamp is drawn.
    #[arg(long, default_value = "bottom-right", value_name = "POS")]
    timestamp_pos: CliTsPos,

    /// Font size (px) for the timestamp overlay.
    #[arg(long, default_value_t = 28, value_name = "PX")]
    timestamp_size: u32,

    /// Add a dark metadata bar to the image.
    #[arg(long)]
    metadata_bar: bool,

    /// Whether the metadata bar is placed at the top or bottom.
    #[arg(long, default_value = "top", value_name = "POS")]
    bar_pos: CliBarPos,

    /// Font size (px) for the metadata bar text.
    #[arg(long, default_value_t = 22, value_name = "PX")]
    bar_size: u32,

    /// Fields shown in the metadata bar (space-separated, repeatable).
    /// Default when --metadata-bar is set: filename duration.
    /// Example: --meta filename duration fps resolution
    #[arg(long = "meta", num_args = 1.., value_name = "FIELD")]
    meta_fields: Vec<CliMetaField>,
}

// ── Video file extensions considered ─────────────────────────────────────────
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

fn collect_videos(inputs: Vec<PathBuf>, recursive: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in inputs {
        if path.is_file() {
            out.push(path);
        } else if path.is_dir() {
            collect_from_dir(&path, recursive, &mut out);
        } else {
            eprintln!("Warning: '{}' does not exist, skipping.", path.display());
        }
    }
    out
}

fn collect_from_dir(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Cannot read directory '{}': {e}", dir.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() && is_video(&p) {
            out.push(p);
        } else if recursive && p.is_dir() {
            collect_from_dir(&p, true, out);
        }
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────
fn main() {
    let args = Args::parse();

    if !check_ffmpeg() {
        eprintln!("Error: ffmpeg and/or ffprobe not found in PATH.");
        eprintln!("Install ffmpeg and make sure it is accessible from the command line.");
        std::process::exit(1);
    }

    let mode = match args.mode {
        CliMode::Single => ThumbnailMode::Single {
            seek_percent: args.seek_percent,
        },
        CliMode::Grid => ThumbnailMode::Grid {
            cols: args.grid_cols,
            rows: args.grid_rows,
        },
        CliMode::Sequence => ThumbnailMode::Sequence { count: args.frames },
    };
    let format = match args.format {
        CliFormat::Jpg => OutputFormat::Jpg,
        CliFormat::Png => OutputFormat::Png,
        CliFormat::Webp => OutputFormat::Webp,
    };

    // ── Overlay config ────────────────────────────────────────────────────────
    let ts_pos = match args.timestamp_pos {
        CliTsPos::BottomRight => TimestampPosition::BottomRight,
        CliTsPos::BottomLeft => TimestampPosition::BottomLeft,
        CliTsPos::TopRight => TimestampPosition::TopRight,
        CliTsPos::TopLeft => TimestampPosition::TopLeft,
    };
    let b_pos = match args.bar_pos {
        CliBarPos::Top => BarPosition::Top,
        CliBarPos::Bottom => BarPosition::Bottom,
    };
    // If --metadata-bar is set but no --meta fields given, default to filename + duration.
    let meta_fields: Vec<MetadataField> = if args.metadata_bar && args.meta_fields.is_empty() {
        vec![MetadataField::Filename, MetadataField::Duration]
    } else {
        args.meta_fields
            .iter()
            .map(|f| match f {
                CliMetaField::Filename => MetadataField::Filename,
                CliMetaField::Duration => MetadataField::Duration,
                CliMetaField::Fps => MetadataField::Fps,
                CliMetaField::Resolution => MetadataField::Resolution,
                CliMetaField::Filesize => MetadataField::FileSize,
                CliMetaField::Timestamp => MetadataField::Timestamp,
                CliMetaField::Codec => MetadataField::Codec,
                CliMetaField::Bitrate => MetadataField::Bitrate,
            })
            .collect()
    };
    let overlay = OverlayConfig {
        show_timestamp: args.timestamp,
        timestamp_position: ts_pos,
        timestamp_font_size: args.timestamp_size,
        show_metadata_bar: args.metadata_bar,
        bar_position: b_pos,
        bar_font_size: args.bar_size,
        metadata_fields: meta_fields,
    };

    let config = ThumbnailConfig {
        output_dir: args.output_dir,
        mode,
        max_width: args.max_width,
        max_height: args.max_height,
        format,
        quality: args.quality,
        overwrite: args.overwrite,
        output_prefix: args.prefix,
        overlay,
    };

    let files = collect_videos(args.input, args.recursive);
    if files.is_empty() {
        eprintln!("No video files found.");
        std::process::exit(1);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let total = files.len();
    let mut errors = 0usize;

    for (i, path) in files.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            eprintln!("\nAborted by user.");
            break;
        }

        let name = path.file_name().unwrap_or_default().to_string_lossy();
        eprint!("[{}/{}] {} … ", i + 1, total, name);

        let result = process_video(&config, path, &stop, &|_frac, msg| {
            eprint!("\r[{}/{}] {} — {:<50}", i + 1, total, name, msg)
        });

        if let Some(err) = result.error {
            eprintln!("\n  ERROR: {err}");
            errors += 1;
        } else {
            eprintln!("OK ({} file(s))", result.output_files.len());
            for p in &result.output_files {
                println!("{}", p.display());
            }
        }
    }

    if errors > 0 {
        eprintln!("{errors} file(s) failed.");
        std::process::exit(1);
    }
}
