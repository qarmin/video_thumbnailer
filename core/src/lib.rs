use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use image::{DynamicImage, GenericImage, RgbImage};
use serde::{Deserialize, Serialize};

//  Windows: suppress console popup 
#[cfg(target_os = "windows")]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}
#[cfg(not(target_os = "windows"))]
fn hide_console(_cmd: &mut Command) {}

//  ffmpeg / ffprobe availability 
pub fn check_ffmpeg() -> bool {
    let check = |bin: &str| {
        let mut cmd = Command::new(bin);
        hide_console(&mut cmd);
        cmd.arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    check("ffmpeg") && check("ffprobe")
}

//  ffprobe JSON structures 
#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    #[serde(default)]
    avg_frame_rate: String,
    #[serde(default)]
    r_frame_rate: String,
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
    bit_rate: Option<String>,
}

fn parse_fps(s: &str) -> Option<f64> {
    if s.is_empty() || s == "0/0" {
        return None;
    }
    if let Some((n, d)) = s.split_once('/') {
        let nv: f64 = n.parse().ok()?;
        let dv: f64 = d.parse().ok()?;
        if dv == 0.0 {
            return None;
        }
        Some(nv / dv)
    } else {
        s.parse().ok()
    }
}

//  Video metadata 
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VideoMetadata {
    pub fps: Option<f64>,
    pub codec: Option<String>,
    pub bitrate: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
}

impl VideoMetadata {
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let mut cmd = Command::new("ffprobe");
        hide_console(&mut cmd);
        let out = cmd
            .args([
                "-v",
                "quiet",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| format!("ffprobe exec failed: {e}"))?;

        if !out.status.success() {
            return Err(format!("ffprobe failed for: {}", path.display()));
        }

        let probe: ProbeOutput = serde_json::from_slice(&out.stdout)
            .map_err(|e| format!("ffprobe JSON parse error: {e}"))?;

        let (codec, bitrate, width, height, fps) = if let Some(vs) = probe
            .streams
            .iter()
            .find(|s| s.codec_type.as_deref() == Some("video"))
        {
            let br = vs
                .bit_rate
                .as_ref()
                .or(probe.format.bit_rate.as_ref())
                .and_then(|s| s.parse().ok());
            (
                vs.codec_name.clone(),
                br,
                vs.width.filter(|&w| w > 0).map(|w| w as u32),
                vs.height.filter(|&h| h > 0).map(|h| h as u32),
                parse_fps(&vs.avg_frame_rate).or_else(|| parse_fps(&vs.r_frame_rate)),
            )
        } else {
            (None, None, None, None, None)
        };

        let meta = VideoMetadata {
            duration: probe
                .format
                .duration
                .as_deref()
                .and_then(|s| s.parse().ok()),
            codec,
            bitrate,
            width,
            height,
            fps,
        };

        Ok(meta)
    }
}

//  Config types 
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum OutputFormat {
    #[default]
    Jpg,
    Png,
    Webp,
}

impl OutputFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            OutputFormat::Jpg => "jpg",
            OutputFormat::Png => "png",
            OutputFormat::Webp => "webp",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThumbnailMode {
    /// One frame at `seek_percent` % of the video duration (0–100).
    Single { seek_percent: f64 },
    /// Grid of (cols × rows) frames assembled into one image.
    Grid { cols: u32, rows: u32 },
    /// `count` individual frames saved as separate files.
    Sequence { count: u32 },
}

impl Default for ThumbnailMode {
    fn default() -> Self {
        ThumbnailMode::Single { seek_percent: 10.0 }
    }
}

//  Overlay config 
/// Where to stamp the timestamp on each frame.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum TimestampPosition {
    #[default]
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

/// Where to attach the metadata bar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum BarPosition {
    #[default]
    Top,
    Bottom,
}

/// How the metadata bar arranges its fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum BarLayout {
    /// All fields joined into a single line with " | " separators.
    #[default]
    Horizontal,
    /// Each field on its own line, stacked vertically.
    Vertical,
}

/// How a font size is determined for an overlay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum FontSizing {
    /// Sublinear auto-fit. Stays readable on small grid tiles and doesn't
    /// take excessive space on large single frames.
    #[default]
    Auto,
    /// Fixed pixel size, ignoring frame dimensions.
    Pixels,
    /// Percent of the relevant frame dimension. Simple but doesn't behave
    /// well across very different image sizes (e.g. grid tiles vs. single frames).
    Percent,
}

/// Sublinear font size auto-fit: `clamp(MIN, MAX, k * sqrt(dim) + b)`.
/// Tuned so a 108-px tile gets ~11px and a 1080-px frame gets ~30px.
fn auto_font_size(dim: u32) -> f32 {
    const MIN: f32 = 10.0;
    const MAX: f32 = 80.0;
    const SLOPE: f32 = 0.85;
    const OFFSET: f32 = 2.0;
    (SLOPE * (dim as f32).sqrt() + OFFSET).clamp(MIN, MAX)
}

/// Fields that may appear in the metadata bar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MetadataField {
    Filename,
    Duration,
    Fps,
    Resolution,
    FileSize,
    /// Current frame timestamp (h:mm:ss).
    Timestamp,
    Codec,
    Bitrate,
}

pub const BRANDING_TEXT: &str =
    "Created via video-thumbnailer — github.com/qarmin/video_thumbnailer";

/// Overlay and metadata-bar configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayConfig {
    /// Draw the current timestamp on each extracted frame.
    pub show_timestamp: bool,
    pub timestamp_position: TimestampPosition,
    pub timestamp_font_sizing: FontSizing,
    /// Used when `timestamp_font_sizing == Pixels`.
    pub timestamp_font_size: u32,
    /// Used when `timestamp_font_sizing == Percent`. Percent of `min(width, height)`.
    pub timestamp_font_size_percent: f32,

    /// Add a dark bar with metadata to the image.
    pub show_metadata_bar: bool,
    pub bar_position: BarPosition,
    pub bar_layout: BarLayout,
    pub bar_font_sizing: FontSizing,
    /// Used when `bar_font_sizing == Pixels`.
    pub bar_font_size: u32,
    /// Used when `bar_font_sizing == Percent`. Percent of the underlying frame height.
    pub bar_font_size_percent: f32,
    /// Which fields to include in the metadata bar (displayed in order).
    pub metadata_fields: Vec<MetadataField>,
    /// Append a final branding line to the metadata bar.
    pub show_branding: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            show_timestamp: false,
            timestamp_position: TimestampPosition::default(),
            timestamp_font_sizing: FontSizing::default(),
            timestamp_font_size: 28,
            timestamp_font_size_percent: 5.0,
            show_metadata_bar: false,
            bar_position: BarPosition::default(),
            bar_layout: BarLayout::default(),
            bar_font_sizing: FontSizing::default(),
            bar_font_size: 22,
            bar_font_size_percent: 5.0,
            metadata_fields: Vec::new(),
            show_branding: true,
        }
    }
}

//  Main config 
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbnailConfig {
    /// `None` → output next to the source file.
    pub output_dir: Option<PathBuf>,
    pub mode: ThumbnailMode,
    /// Maximum pixel width of each extracted frame.
    pub max_width: u32,
    /// Maximum pixel height of each extracted frame.
    pub max_height: u32,
    pub format: OutputFormat,
    /// Encoding quality (1–100). Used for JPEG and WebP.
    pub quality: u8,
    pub overwrite: bool,
    /// Optional string prepended to every output filename.
    pub output_prefix: String,
    pub overlay: OverlayConfig,
    /// Extract frames in parallel using all available CPU cores (default: true).
    pub parallel: bool,
}

impl Default for ThumbnailConfig {
    fn default() -> Self {
        Self {
            output_dir: None,
            mode: ThumbnailMode::default(),
            max_width: 1920,
            max_height: 1080,
            format: OutputFormat::Jpg,
            quality: 85,
            overwrite: false,
            output_prefix: String::new(),
            overlay: OverlayConfig::default(),
            parallel: true,
        }
    }
}

//  Frame extraction 
/// Extract a single frame from `video_path` at `timestamp` seconds.
/// The frame is scaled down to fit within `max_width` × `max_height`
/// while preserving the original aspect ratio.
pub fn extract_frame(
    video_path: &Path,
    timestamp: f64,
    max_width: u32,
    max_height: u32,
) -> Result<RgbImage, String> {
    let vf = format!(
        "scale='min({max_width},iw)':'min({max_height},ih)':force_original_aspect_ratio=decrease"
    );

    let mut cmd = Command::new("ffmpeg");
    hide_console(&mut cmd);
    let out = cmd
        .arg("-threads")
        .arg("1")
        .arg("-ss")
        .arg(format!("{timestamp:.3}"))
        .arg("-i")
        .arg(video_path)
        .arg("-vf")
        .arg(&vf)
        .arg("-vframes")
        .arg("1")
        .arg("-f")
        .arg("image2pipe")
        .arg("-vcodec")
        .arg("png")
        .arg("pipe:1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("ffmpeg exec failed: {e}"))?;

    if !out.status.success() || out.stdout.is_empty() {
        return Err(format!(
            "ffmpeg returned status {} for '{}' at t={timestamp:.2}s",
            out.status,
            video_path.display()
        ));
    }

    let img =
        image::load_from_memory(&out.stdout).map_err(|e| format!("Frame decode error: {e}"))?;
    Ok(img.into_rgb8())
}

//  Image saving 
fn save_image(
    img: &RgbImage,
    path: &Path,
    format: &OutputFormat,
    quality: u8,
) -> Result<(), String> {
    use image::ImageEncoder;

    match format {
        OutputFormat::Jpg => {
            let mut buf: Vec<u8> = Vec::new();
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
            enc.write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| format!("JPEG encode error: {e}"))?;
            std::fs::write(path, buf).map_err(|e| format!("File write error: {e}"))
        }
        OutputFormat::Png => img.save(path).map_err(|e| format!("PNG save error: {e}")),
        OutputFormat::Webp => DynamicImage::ImageRgb8(img.clone())
            .save(path)
            .map_err(|e| format!("WebP save error: {e}")),
    }
}

//  Text / overlay helpers 

/// Try to load a TTF/OTF font from well-known system paths.
/// Returns `None` if nothing is found; overlays are silently skipped in that case.
/// On Linux install `fonts-dejavu` (`sudo apt install fonts-dejavu`) if no font is found.
fn load_system_font() -> Option<ab_glyph::FontArc> {
    #[cfg(target_os = "windows")]
    let candidates: &[&str] = &[
        r"C:\Windows\Fonts\consola.ttf",
        r"C:\Windows\Fonts\cour.ttf",
        r"C:\Windows\Fonts\arialbd.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ];
    #[cfg(target_os = "macos")]
    let candidates: &[&str] = &[
        "/System/Library/Fonts/Monaco.ttf",
        "/Library/Fonts/Courier New.ttf",
        "/Library/Fonts/Arial.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
    ];
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let candidates: &[&str] = &[
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/freefont/FreeMono.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
    ];

    candidates
        .iter()
        .filter_map(|p| std::fs::read(p).ok())
        .find_map(|data| ab_glyph::FontArc::try_from_vec(data).ok())
}

/// Blend a single pixel with alpha coverage (0..1).
#[inline]
fn blend_pixel(img: &mut RgbImage, ix: i32, iy: i32, color: [u8; 3], cov: f32) {
    if ix < 0 || iy < 0 || ix >= img.width() as i32 || iy >= img.height() as i32 {
        return;
    }
    let a = (cov * 255.0).round() as u32;
    if a == 0 {
        return;
    }
    let p = img.get_pixel_mut(ix as u32, iy as u32);
    p[0] = ((color[0] as u32 * a + p[0] as u32 * (255 - a)) / 255) as u8;
    p[1] = ((color[1] as u32 * a + p[1] as u32 * (255 - a)) / 255) as u8;
    p[2] = ((color[2] as u32 * a + p[2] as u32 * (255 - a)) / 255) as u8;
}

/// Rasterise `text` onto `img` starting at pixel `(x, y)` (top-left of baseline).
fn draw_text(
    img: &mut RgbImage,
    font: &ab_glyph::FontArc,
    text: &str,
    x: i32,
    y: i32,
    font_size: f32,
    color: [u8; 3],
) {
    use ab_glyph::{Font, PxScale, ScaleFont, point};
    let scale = PxScale::from(font_size);
    let sf = font.as_scaled(scale);
    let mut cx = x as f32;
    for ch in text.chars() {
        if ch.is_control() {
            continue;
        }
        let gid = sf.glyph_id(ch);
        let glyph = gid.with_scale_and_position(scale, point(cx, y as f32 + sf.ascent()));
        cx += sf.h_advance(gid);
        if let Some(og) = font.outline_glyph(glyph) {
            let b = og.px_bounds();
            og.draw(|px, py, cov| {
                blend_pixel(
                    img,
                    b.min.x as i32 + px as i32,
                    b.min.y as i32 + py as i32,
                    color,
                    cov,
                );
            });
        }
    }
}

/// Draw text with a 1-pixel dark outline so it's readable on any background.
fn draw_text_shadowed(
    img: &mut RgbImage,
    font: &ab_glyph::FontArc,
    text: &str,
    x: i32,
    y: i32,
    font_size: f32,
    color: [u8; 3],
) {
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx != 0 || dy != 0 {
                draw_text(img, font, text, x + dx, y + dy, font_size, [0, 0, 0]);
            }
        }
    }
    draw_text(img, font, text, x, y, font_size, color);
}

fn text_width(font: &ab_glyph::FontArc, text: &str, font_size: f32) -> f32 {
    use ab_glyph::{Font, PxScale, ScaleFont};
    let sf = font.as_scaled(PxScale::from(font_size));
    text.chars().map(|c| sf.h_advance(sf.glyph_id(c))).sum()
}

fn text_line_height(font: &ab_glyph::FontArc, font_size: f32) -> f32 {
    use ab_glyph::{Font, PxScale, ScaleFont};
    let sf = font.as_scaled(PxScale::from(font_size));
    sf.ascent() - sf.descent()
}

fn fmt_timestamp(secs: f64) -> String {
    let s = secs as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    let ms = ((secs.fract()) * 1000.0) as u64;
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m:02}:{sec:02}.{ms:03}")
    }
}

fn fmt_file_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{} KB", bytes / 1024)
    }
}

fn fmt_bitrate(bps: u64) -> String {
    if bps >= 1_000_000 {
        format!("{:.1} Mbps", bps as f64 / 1_000_000.0)
    } else {
        format!("{} kbps", bps / 1_000)
    }
}

/// Build a (label, formatted value) pair for `field`, or `None` if the value is
/// missing. `layout` is consulted only when the unit-suffix would duplicate the
/// label (e.g. "FPS: 30.00 fps" is silly; with the label we drop the suffix).
fn field_entry(
    field: &MetadataField,
    meta: &VideoMetadata,
    video_path: &Path,
    file_size: u64,
    timestamp: f64,
    layout: &BarLayout,
) -> Option<(&'static str, String)> {
    match field {
        MetadataField::Filename => video_path
            .file_name()
            .map(|n| ("File", n.to_string_lossy().into_owned())),
        MetadataField::Duration => meta.duration.map(|d| ("Duration", fmt_timestamp(d))),
        MetadataField::Fps => meta.fps.map(|fps| {
            let value = match layout {
                BarLayout::Vertical => format!("{fps:.2}"),
                BarLayout::Horizontal => format!("{fps:.2} fps"),
            };
            ("FPS", value)
        }),
        MetadataField::Resolution => match (meta.width, meta.height) {
            (Some(w), Some(h)) => Some(("Resolution", format!("{w}×{h}"))),
            _ => None,
        },
        MetadataField::FileSize => (file_size > 0).then(|| ("Size", fmt_file_size(file_size))),
        MetadataField::Timestamp => Some(("Time", fmt_timestamp(timestamp))),
        MetadataField::Codec => meta.codec.as_ref().map(|c| ("Codec", c.to_uppercase())),
        MetadataField::Bitrate => meta.bitrate.map(|br| ("Bitrate", fmt_bitrate(br))),
    }
}

/// Build the metadata bar lines from selected fields, layout, and branding flag.
/// Horizontal layout joins bare values with " | ". Vertical layout prefixes each
/// line with the field label, e.g. "Duration: 12:34".
fn build_bar_lines(
    fields: &[MetadataField],
    meta: &VideoMetadata,
    video_path: &Path,
    file_size: u64,
    timestamp: f64,
    layout: &BarLayout,
    show_branding: bool,
) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for f in fields {
        let Some((label, value)) = field_entry(f, meta, video_path, file_size, timestamp, layout)
        else {
            continue;
        };
        let line = match layout {
            BarLayout::Horizontal => value,
            BarLayout::Vertical => format!("{label}: {value}"),
        };
        parts.push(line);
    }

    let mut lines: Vec<String> = match layout {
        BarLayout::Horizontal => {
            if parts.is_empty() {
                Vec::new()
            } else {
                vec![parts.join("  |  ")]
            }
        }
        BarLayout::Vertical => parts,
    };
    if show_branding {
        lines.push(BRANDING_TEXT.to_string());
    }
    lines
}

/// Effective metadata-bar font size in pixels, given the underlying frame height.
fn resolve_bar_font_size(ov: &OverlayConfig, frame_height: u32) -> f32 {
    match ov.bar_font_sizing {
        FontSizing::Auto => auto_font_size(frame_height),
        FontSizing::Pixels => ov.bar_font_size as f32,
        FontSizing::Percent => (ov.bar_font_size_percent / 100.0) * frame_height as f32,
    }
}

/// Effective timestamp font size in pixels, based on the shorter frame dimension so
/// the stamp doesn't dominate landscape OR portrait crops.
fn resolve_timestamp_font_size(ov: &OverlayConfig, frame_w: u32, frame_h: u32) -> f32 {
    let dim = frame_w.min(frame_h);
    match ov.timestamp_font_sizing {
        FontSizing::Auto => auto_font_size(dim),
        FontSizing::Pixels => ov.timestamp_font_size as f32,
        FontSizing::Percent => (ov.timestamp_font_size_percent / 100.0) * dim as f32,
    }
}

/// Stamp the frame timestamp in a corner of `img`.
fn apply_timestamp(
    img: &mut RgbImage,
    font: &ab_glyph::FontArc,
    timestamp: f64,
    position: &TimestampPosition,
    font_size: f32,
) {
    if font_size < 1.0 {
        return;
    }
    let text = fmt_timestamp(timestamp);
    let pad = 8i32;
    let tw = text_width(font, &text, font_size) as i32;
    let th = text_line_height(font, font_size) as i32;
    let w = img.width() as i32;
    let h = img.height() as i32;

    let (x, y) = match position {
        TimestampPosition::BottomRight => (w - tw - pad, h - th - pad),
        TimestampPosition::BottomLeft => (pad, h - th - pad),
        TimestampPosition::TopRight => (w - tw - pad, pad),
        TimestampPosition::TopLeft => (pad, pad),
    };
    draw_text_shadowed(img, font, &text, x, y, font_size, [255, 230, 80]);
}

/// Add a solid dark bar (top or bottom) containing one or more `lines` to the image.
/// Returns a new `RgbImage` with increased height.
fn add_metadata_bar(
    src: RgbImage,
    font: &ab_glyph::FontArc,
    lines: &[String],
    position: &BarPosition,
    font_size: f32,
) -> RgbImage {
    if lines.is_empty() || font_size < 1.0 {
        return src;
    }

    let pad = 8u32;
    let lh = text_line_height(font, font_size).ceil().max(1.0) as u32;
    let bar_h = lh * lines.len() as u32 + pad * 2;
    let w = src.width();
    let orig_h = src.height();

    let mut out = RgbImage::new(w, orig_h + bar_h);
    let bar_bg = image::Rgb([20u8, 20, 20]);
    let sep = image::Rgb([60u8, 60, 60]);

    let (bar_top, img_top) = match position {
        BarPosition::Top => (0u32, bar_h),
        BarPosition::Bottom => (orig_h, 0u32),
    };

    // Fill bar background
    for y in bar_top..bar_top + bar_h {
        for x in 0..w {
            out.put_pixel(x, y, bar_bg);
        }
    }
    // 1-px separator line
    let sep_y = match position {
        BarPosition::Top => bar_h - 1,
        BarPosition::Bottom => orig_h,
    };
    for x in 0..w {
        out.put_pixel(x, sep_y, sep);
    }

    // Copy original image
    if let Err(e) = out.copy_from(&src, 0, img_top) {
        eprintln!("Metadata bar copy error: {e}");
        return src;
    }

    // Draw each line stacked vertically.
    for (i, line) in lines.iter().enumerate() {
        let y = bar_top as i32 + pad as i32 + (i as i32) * lh as i32;
        draw_text(
            &mut out,
            font,
            line,
            pad as i32,
            y,
            font_size,
            [215, 215, 215],
        );
    }

    out
}

/// Apply timestamp + metadata bar to a single frame (if overlays are configured).
fn apply_overlays(
    mut img: RgbImage,
    font: &ab_glyph::FontArc,
    overlay: &OverlayConfig,
    meta: &VideoMetadata,
    video_path: &Path,
    file_size: u64,
    timestamp: f64,
) -> RgbImage {
    if overlay.show_timestamp {
        let fs = resolve_timestamp_font_size(overlay, img.width(), img.height());
        apply_timestamp(
            &mut img,
            font,
            timestamp,
            &overlay.timestamp_position,
            fs,
        );
    }
    if overlay.show_metadata_bar {
        let lines = build_bar_lines(
            &overlay.metadata_fields,
            meta,
            video_path,
            file_size,
            timestamp,
            &overlay.bar_layout,
            overlay.show_branding,
        );
        if !lines.is_empty() {
            let fs = resolve_bar_font_size(overlay, img.height());
            img = add_metadata_bar(img, font, &lines, &overlay.bar_position, fs);
        }
    }
    img
}

//  Processing result 
#[derive(Debug, Clone)]
pub struct ProcessingResult {
    pub video_path: PathBuf,
    pub output_files: Vec<PathBuf>,
    pub error: Option<String>,
}

//  Main public entry point 
/// Generate thumbnails for a single video file.
///
/// `progress_cb(fraction 0..1, message)` is called periodically from this thread.
/// Set `stop_flag` to `true` to abort; the function returns early with whatever
/// was already produced.
pub fn process_video(
    config: &ThumbnailConfig,
    video_path: &Path,
    stop_flag: &Arc<AtomicBool>,
    progress_cb: &dyn Fn(f32, &str),
) -> ProcessingResult {
    let mut result = ProcessingResult {
        video_path: video_path.to_path_buf(),
        output_files: Vec::new(),
        error: None,
    };

    if !video_path.exists() {
        result.error = Some(format!("File not found: {}", video_path.display()));
        return result;
    }

    progress_cb(0.0, "Reading metadata…");
    let meta = match VideoMetadata::from_path(video_path) {
        Ok(m) => m,
        Err(e) => {
            result.error = Some(e);
            return result;
        }
    };

    let duration = meta.duration.unwrap_or(30.0).max(0.1);
    let file_size = std::fs::metadata(video_path).map(|m| m.len()).unwrap_or(0);

    // Load font once if any overlay is requested.
    let ov = &config.overlay;
    let font_opt: Option<ab_glyph::FontArc> = if ov.show_timestamp || ov.show_metadata_bar {
        load_system_font()
    } else {
        None
    };

    let out_dir = config
        .output_dir
        .clone()
        .unwrap_or_else(|| video_path.parent().unwrap_or(Path::new(".")).to_path_buf());
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        result.error = Some(format!(
            "Cannot create output dir '{}': {e}",
            out_dir.display()
        ));
        return result;
    }

    let stem = video_path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = config.format.extension();
    let prefix = if config.output_prefix.is_empty() {
        String::new()
    } else {
        format!("{}_", config.output_prefix)
    };

    /// Apply overlays if a font is available; otherwise return image unchanged.
    macro_rules! overlay {
        ($img:expr, $ts:expr) => {
            match &font_opt {
                Some(f) => apply_overlays($img, f, ov, &meta, video_path, file_size, $ts),
                None => $img,
            }
        };
    }

    match &config.mode {
        //  Single frame 
        ThumbnailMode::Single { seek_percent } => {
            let t = duration * seek_percent / 100.0;
            let out = out_dir.join(format!("{prefix}{stem}.{ext}"));

            if out.exists() && !config.overwrite {
                result.output_files.push(out);
                return result;
            }
            if stop_flag.load(Ordering::Relaxed) {
                return result;
            }

            progress_cb(0.1, "Extracting frame…");
            match extract_frame(video_path, t, config.max_width, config.max_height) {
                Ok(img) => {
                    let img = overlay!(img, t);
                    match save_image(&img, &out, &config.format, config.quality) {
                        Ok(()) => {
                            result.output_files.push(out);
                            progress_cb(1.0, "Done");
                        }
                        Err(e) => result.error = Some(e),
                    }
                }
                Err(e) => result.error = Some(e),
            }
        }

        //  Grid 
        ThumbnailMode::Grid { cols, rows } => {
            let total = (cols * rows) as usize;
            let tile_w = (config.max_width / cols).max(1);
            let tile_h = (config.max_height / rows).max(1);

            let frames: Vec<RgbImage> = if config.parallel && total > 1 {
                use std::sync::mpsc;
                let (tx, rx) = mpsc::channel::<(usize, Result<RgbImage, String>)>();
                for i in 0..total {
                    let tx = tx.clone();
                    let vp = video_path.to_path_buf();
                    let stop = Arc::clone(stop_flag);
                    let fo = font_opt.clone();
                    let ov2 = ov.clone();
                    let t = duration * (i + 1) as f64 / (total + 2) as f64;
                    rayon::spawn(move || {
                        if stop.load(Ordering::Relaxed) {
                            let _ = tx.send((i, Err("Aborted".to_string())));
                            return;
                        }
                        let r = extract_frame(&vp, t, tile_w, tile_h).map(|mut img| {
                            if let Some(f) = &fo
                                && ov2.show_timestamp {
                                    let fs = resolve_timestamp_font_size(&ov2, img.width(), img.height());
                                    apply_timestamp(
                                        &mut img,
                                        f,
                                        t,
                                        &ov2.timestamp_position,
                                        fs,
                                    );
                                }
                            img
                        });
                        let _ = tx.send((i, r));
                    });
                }
                drop(tx);
                let mut indexed = vec![None::<RgbImage>; total];
                let mut done = 0usize;
                for (i, r) in rx {
                    match r {
                        Ok(img) => {
                            indexed[i] = Some(img);
                            done += 1;
                            progress_cb(
                                done as f32 / total as f32 * 0.9,
                                &format!("Frame {done}/{total}"),
                            );
                        }
                        Err(_) if stop_flag.load(Ordering::Relaxed) => return result,
                        Err(e) => {
                            result.error = Some(e);
                            return result;
                        }
                    }
                }
                if stop_flag.load(Ordering::Relaxed) {
                    return result;
                }
                indexed.into_iter().flatten().collect()
            } else {
                let mut frames = Vec::with_capacity(total);
                for i in 0..total {
                    if stop_flag.load(Ordering::Relaxed) {
                        return result;
                    }
                    let t = duration * (i + 1) as f64 / (total + 2) as f64;
                    progress_cb(
                        i as f32 / total as f32,
                        &format!("Frame {}/{}", i + 1, total),
                    );
                    match extract_frame(video_path, t, tile_w, tile_h) {
                        Ok(img) => {
                            let img = if let Some(f) = &font_opt {
                                let mut img = img;
                                if ov.show_timestamp {
                                    let fs = resolve_timestamp_font_size(ov, img.width(), img.height());
                                    apply_timestamp(
                                        &mut img,
                                        f,
                                        t,
                                        &ov.timestamp_position,
                                        fs,
                                    );
                                }
                                img
                            } else {
                                img
                            };
                            frames.push(img);
                        }
                        Err(e) => {
                            result.error = Some(e);
                            return result;
                        }
                    }
                }
                frames
            };

            let fw = frames[0].width();
            let fh = frames[0].height();
            if frames.iter().any(|f| f.width() != fw || f.height() != fh) {
                result.error = Some("Grid tiles have inconsistent dimensions".to_string());
                return result;
            }

            let mut grid = RgbImage::new(fw * cols, fh * rows);
            for (idx, frame) in frames.iter().enumerate() {
                let cx = (idx as u32 % cols) * fw;
                let cy = (idx as u32 / cols) * fh;
                if let Err(e) = grid.copy_from(frame, cx, cy) {
                    result.error = Some(format!("Grid compose error: {e}"));
                    return result;
                }
            }

            // Apply metadata bar to the full grid (use video mid-point as timestamp).
            let grid = if let Some(f) = &font_opt {
                if ov.show_metadata_bar {
                    let bar_ts = duration / 2.0;
                    let lines = build_bar_lines(
                        &ov.metadata_fields,
                        &meta,
                        video_path,
                        file_size,
                        bar_ts,
                        &ov.bar_layout,
                        ov.show_branding,
                    );
                    if lines.is_empty() {
                        grid
                    } else {
                        let fs = resolve_bar_font_size(ov, grid.height());
                        add_metadata_bar(grid, f, &lines, &ov.bar_position, fs)
                    }
                } else {
                    grid
                }
            } else {
                grid
            };

            let out = out_dir.join(format!("{prefix}{stem}_grid{cols}x{rows}.{ext}"));
            if out.exists() && !config.overwrite {
                result.output_files.push(out);
                return result;
            }
            match save_image(&grid, &out, &config.format, config.quality) {
                Ok(()) => {
                    result.output_files.push(out);
                    progress_cb(1.0, "Done");
                }
                Err(e) => result.error = Some(e),
            }
        }

        //  Sequence 
        ThumbnailMode::Sequence { count } => {
            let n = *count;
            if config.parallel && n > 1 {
                use std::sync::mpsc;
                let (tx, rx) = mpsc::channel::<(u32, Result<PathBuf, String>)>();
                let stem_str = stem.to_string();
                for i in 0..n {
                    let tx = tx.clone();
                    let vp = video_path.to_path_buf();
                    let stop = Arc::clone(stop_flag);
                    let fo = font_opt.clone();
                    let ov2 = ov.clone();
                    let meta2 = meta.clone();
                    let out_dir2 = out_dir.clone();
                    let prefix2 = prefix.clone();
                    let stem2 = stem_str.clone();
                    let fmt = config.format.clone();
                    let max_w = config.max_width;
                    let max_h = config.max_height;
                    let overwrite = config.overwrite;
                    let quality = config.quality;
                    let t = duration * (i + 1) as f64 / (n + 2) as f64;
                    rayon::spawn(move || {
                        if stop.load(Ordering::Relaxed) {
                            let _ = tx.send((i, Err("Aborted".to_string())));
                            return;
                        }
                        let out = out_dir2.join(format!("{prefix2}{stem2}_{:04}.{ext}", i + 1));
                        if out.exists() && !overwrite {
                            let _ = tx.send((i, Ok(out)));
                            return;
                        }
                        let r = extract_frame(&vp, t, max_w, max_h)
                            .map(|img| match &fo {
                                Some(f) => {
                                    apply_overlays(img, f, &ov2, &meta2, &vp, file_size, t)
                                }
                                None => img,
                            })
                            .and_then(|img| {
                                save_image(&img, &out, &fmt, quality)?;
                                Ok(out)
                            });
                        let _ = tx.send((i, r));
                    });
                }
                drop(tx);
                let mut indexed = vec![None::<PathBuf>; n as usize];
                let mut done = 0u32;
                for (i, r) in rx {
                    match r {
                        Ok(path) => {
                            indexed[i as usize] = Some(path);
                            done += 1;
                            progress_cb(done as f32 / n as f32, &format!("Frame {done}/{n}"));
                        }
                        Err(_) if stop_flag.load(Ordering::Relaxed) => return result,
                        Err(e) => {
                            result.error = Some(e);
                            return result;
                        }
                    }
                }
                if stop_flag.load(Ordering::Relaxed) {
                    return result;
                }
                for path in indexed.into_iter().flatten() {
                    result.output_files.push(path);
                }
                progress_cb(1.0, "Done");
            } else {
                for i in 0..n {
                    if stop_flag.load(Ordering::Relaxed) {
                        return result;
                    }
                    let t = duration * (i + 1) as f64 / (n + 2) as f64;
                    let out = out_dir.join(format!("{prefix}{stem}_{:04}.{ext}", i + 1));

                    progress_cb(
                        i as f32 / n as f32,
                        &format!("Frame {}/{}", i + 1, n),
                    );

                    if out.exists() && !config.overwrite {
                        result.output_files.push(out);
                        continue;
                    }

                    match extract_frame(video_path, t, config.max_width, config.max_height) {
                        Ok(img) => {
                            let img = overlay!(img, t);
                            match save_image(&img, &out, &config.format, config.quality) {
                                Ok(()) => result.output_files.push(out),
                                Err(e) => {
                                    result.error = Some(e);
                                    return result;
                                }
                            }
                        }
                        Err(e) => {
                            result.error = Some(e);
                            return result;
                        }
                    }
                }
                progress_cb(1.0, "Done");
            }
        }
    }

    result
}
