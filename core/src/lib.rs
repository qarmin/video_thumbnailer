use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use image::{DynamicImage, GenericImage, RgbImage};
use serde::{Deserialize, Serialize};

// ── Windows: suppress console popup ──────────────────────────────────────────
#[cfg(target_os = "windows")]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}
#[cfg(not(target_os = "windows"))]
fn hide_console(_cmd: &mut Command) {}

// ── ffmpeg / ffprobe availability ─────────────────────────────────────────────
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

// ── ffprobe JSON structures ───────────────────────────────────────────────────
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

// ── Video metadata ────────────────────────────────────────────────────────────
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

// ── Config types ──────────────────────────────────────────────────────────────
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

// ── Overlay config ────────────────────────────────────────────────────────────
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

/// Overlay and metadata-bar configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayConfig {
    /// Draw the current timestamp on each extracted frame.
    pub show_timestamp: bool,
    pub timestamp_position: TimestampPosition,
    /// Font size in pixels for the timestamp overlay.
    pub timestamp_font_size: u32,

    /// Add a dark bar with metadata to the image.
    pub show_metadata_bar: bool,
    pub bar_position: BarPosition,
    /// Font size in pixels for the metadata bar text.
    pub bar_font_size: u32,
    /// Which fields to include in the metadata bar (displayed in order).
    pub metadata_fields: Vec<MetadataField>,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            show_timestamp: false,
            timestamp_position: TimestampPosition::default(),
            timestamp_font_size: 28,
            show_metadata_bar: false,
            bar_position: BarPosition::default(),
            bar_font_size: 22,
            metadata_fields: Vec::new(),
        }
    }
}

// ── Main config ───────────────────────────────────────────────────────────────
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
        }
    }
}

// ── Frame extraction ──────────────────────────────────────────────────────────
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

// ── Image saving ──────────────────────────────────────────────────────────────
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

// ── Text / overlay helpers ────────────────────────────────────────────────────

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

/// Build the metadata bar string from selected fields.
fn build_bar_text(
    fields: &[MetadataField],
    meta: &VideoMetadata,
    video_path: &Path,
    file_size: u64,
    timestamp: f64,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    for f in fields {
        match f {
            MetadataField::Filename => {
                if let Some(n) = video_path.file_name() {
                    parts.push(n.to_string_lossy().into_owned());
                }
            }
            MetadataField::Duration => {
                if let Some(d) = meta.duration {
                    parts.push(fmt_timestamp(d));
                }
            }
            MetadataField::Fps => {
                if let Some(fps) = meta.fps {
                    parts.push(format!("{fps:.2} fps"));
                }
            }
            MetadataField::Resolution => {
                if let (Some(w), Some(h)) = (meta.width, meta.height) {
                    parts.push(format!("{w}×{h}"));
                }
            }
            MetadataField::FileSize => {
                if file_size > 0 {
                    parts.push(fmt_file_size(file_size));
                }
            }
            MetadataField::Timestamp => {
                parts.push(fmt_timestamp(timestamp));
            }
            MetadataField::Codec => {
                if let Some(c) = &meta.codec {
                    parts.push(c.to_uppercase());
                }
            }
            MetadataField::Bitrate => {
                if let Some(br) = meta.bitrate {
                    parts.push(fmt_bitrate(br));
                }
            }
        }
    }
    parts.join("  |  ")
}

/// Stamp the frame timestamp in a corner of `img`.
fn apply_timestamp(
    img: &mut RgbImage,
    font: &ab_glyph::FontArc,
    timestamp: f64,
    position: &TimestampPosition,
    font_size: u32,
) {
    let text = fmt_timestamp(timestamp);
    let fs = font_size as f32;
    let pad = 8i32;
    let tw = text_width(font, &text, fs) as i32;
    let th = text_line_height(font, fs) as i32;
    let w = img.width() as i32;
    let h = img.height() as i32;

    let (x, y) = match position {
        TimestampPosition::BottomRight => (w - tw - pad, h - th - pad),
        TimestampPosition::BottomLeft => (pad, h - th - pad),
        TimestampPosition::TopRight => (w - tw - pad, pad),
        TimestampPosition::TopLeft => (pad, pad),
    };
    draw_text_shadowed(img, font, &text, x, y, fs, [255, 230, 80]);
}

/// Add a solid dark bar (top or bottom) containing `text` to the image.
/// Returns a new `RgbImage` with increased height.
fn add_metadata_bar(
    src: RgbImage,
    font: &ab_glyph::FontArc,
    text: &str,
    position: &BarPosition,
    font_size: u32,
) -> RgbImage {
    if text.is_empty() {
        return src;
    }

    let pad = 8u32;
    let lh = text_line_height(font, font_size as f32).ceil() as u32;
    let bar_h = lh + pad * 2;
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

    // Draw text centred vertically in the bar
    let text_y = bar_top as i32 + pad as i32;
    draw_text(
        &mut out,
        font,
        text,
        pad as i32,
        text_y,
        font_size as f32,
        [215, 215, 215],
    );

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
    if overlay.show_timestamp && overlay.timestamp_font_size > 0 {
        apply_timestamp(
            &mut img,
            font,
            timestamp,
            &overlay.timestamp_position,
            overlay.timestamp_font_size,
        );
    }
    if overlay.show_metadata_bar && !overlay.metadata_fields.is_empty() && overlay.bar_font_size > 0
    {
        let text = build_bar_text(
            &overlay.metadata_fields,
            meta,
            video_path,
            file_size,
            timestamp,
        );
        img = add_metadata_bar(
            img,
            font,
            &text,
            &overlay.bar_position,
            overlay.bar_font_size,
        );
    }
    img
}

// ── Processing result ─────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
pub struct ProcessingResult {
    pub video_path: PathBuf,
    pub output_files: Vec<PathBuf>,
    pub error: Option<String>,
}

// ── Main public entry point ───────────────────────────────────────────────────
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
        // ── Single frame ──────────────────────────────────────────────────────
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

        // ── Grid ──────────────────────────────────────────────────────────────
        ThumbnailMode::Grid { cols, rows } => {
            let total = (cols * rows) as usize;
            let tile_w = (config.max_width / cols).max(1);
            let tile_h = (config.max_height / rows).max(1);
            let mut frames: Vec<RgbImage> = Vec::with_capacity(total);

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
                        // Timestamp on each tile (no bar — that would break grid layout).
                        let img = if let Some(f) = &font_opt {
                            let mut img = img;
                            if ov.show_timestamp && ov.timestamp_font_size > 0 {
                                apply_timestamp(
                                    &mut img,
                                    f,
                                    t,
                                    &ov.timestamp_position,
                                    ov.timestamp_font_size,
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
                if ov.show_metadata_bar && !ov.metadata_fields.is_empty() {
                    let bar_ts = duration / 2.0;
                    let text =
                        build_bar_text(&ov.metadata_fields, &meta, video_path, file_size, bar_ts);
                    add_metadata_bar(grid, f, &text, &ov.bar_position, ov.bar_font_size)
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

        // ── Sequence ──────────────────────────────────────────────────────────
        ThumbnailMode::Sequence { count } => {
            for i in 0..*count {
                if stop_flag.load(Ordering::Relaxed) {
                    return result;
                }
                let t = duration * (i + 1) as f64 / (count + 2) as f64;
                let out = out_dir.join(format!("{prefix}{stem}_{:04}.{ext}", i + 1));

                progress_cb(
                    i as f32 / *count as f32,
                    &format!("Frame {}/{}", i + 1, count),
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

    result
}
