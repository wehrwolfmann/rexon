//! Thumbnails for wallpaper cards the engine gave no ready preview for.
//!
//! The wallpaper tab draws a ready preview directly; only the rest gets here —
//! wallpapers without a picture (first of all videos from one's own folder: a
//! clip has no preview until someone takes one). For them a frame is taken
//! from the file itself and put into the desktop's shared thumbnail cache
//! (Thumbnail Managing Standard, freedesktop.org):
//! `<cache>/thumbnails/x-large/<md5 of the URI>.png`, with the `Thumb::URI`,
//! `Thumb::MTime`, `Thumb::Size` keys inside the PNG. There are no requests to
//! the daemon — everything is decided on the spot, from the file.
//!
//! The whole decision is here: what counts as the source, where the thumbnail
//! lies, whether it is fresh, how to call `ffmpeg`, how to build the PNG. The
//! QML object (`rexon-gui/src/thumbnail_request.rs`) only executes: it calls
//! [`live_lookup`] on the main thread and [`live_generate`] on a worker.
//!
//! Contract with the page (read by `component/ThumbnailImage.qml`): the
//! properties `source`, `resource`, `wpType` — input; `state`
//! (`Idle`/`Loading`/`Ready`/`Failed`), `cachePath` (the address of the ready
//! PNG) and `error` — output.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

use md5::{Digest, Md5};

/// Request states — the order is the same as the page's `State` enum
/// (`W.ThumbnailRequest.Ready` is read from there as a number).
pub const STATE_IDLE: i32 = 0;
pub const STATE_LOADING: i32 = 1;
pub const STATE_READY: i32 = 2;
pub const STATE_FAILED: i32 = 3;

/// Long edge of a thumbnail. The card asks for no more than 512 (`maxSize`).
pub const MAX_EDGE: u32 = 512;

/// How many frames from the start of a clip `ffmpeg` looks through to pick the
/// most representative: the first frame is often black (a fade, an intro).
pub const VIDEO_PROBE_FRAMES: u32 = 30;

/// How long we wait for `ffmpeg` per file. A broken or network file must not
/// occupy one of the thumbnail queue slots forever.
pub const FRAME_TIMEOUT_SECS: u64 = 20;

/// Refusal causes — strings of the `error` property. In English: this field is
/// for the log and debugging, the page does not show it to the person.
pub const ERR_NO_SOURCE: &str = "no preview source";
pub const ERR_NOT_FOUND: &str = "source file not found";

/// What exactly to turn into a thumbnail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// The source file (as it came from the page, not yet absolute).
    pub path: String,
    /// `true` — a clip (`ffmpeg` gets the frame), `false` — a picture.
    pub is_video: bool,
}

/// Choosing the source by the request's three properties.
///
/// * there is `source` (a ready preview) — the thumbnail is made from it;
/// * otherwise, if the wallpaper is a video or a picture — from the wallpaper
///   file itself;
/// * otherwise there is nothing to take it from: a scene or a web page cannot
///   be turned into a picture without a renderer.
pub fn resolve_job(source: &str, resource: &str, wp_type: &str) -> Result<Job, &'static str> {
    if !source.is_empty() {
        return Ok(Job { path: source.to_string(), is_video: false });
    }
    if !resource.is_empty() && (wp_type == "video" || wp_type == "image") {
        return Ok(Job { path: resource.to_string(), is_video: wp_type == "video" });
    }
    Err(ERR_NO_SOURCE)
}

/// Refusal causes of waywallen 0.6.0 (the original's `ThumbnailRequest::submit`).
pub const ERR_NO_PREVIEW: &str = "No thumbnail preview source";
pub const ERR_NOT_LOCAL: &str = "Thumbnail generation requires a local resource";
/// A «timeline» thumbnail (time-of-day wallpapers: KDE/GNOME) the original
/// assembles from the description's pictures. We do not have that — without a
/// ready preview the card is honestly empty.
pub const ERR_TIMELINE: &str = "Timeline thumbnails are not supported";

/// What to do with a thumbnail request (waywallen 0.6.0).
///
/// Since 0.6.0 the card ALWAYS shows the request's `cachePath`, so a ready
/// preview (and a picture without a preview) is handed over as an address as
/// is — an animated preview stays animated. A frame needs to be made only for
/// a clip without a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// The request is disabled or there is no input.
    Idle,
    /// Show this address as is.
    Direct(String),
    /// Refusal with a cause.
    Failed(String),
    /// A frame from the clip (`resource`) — through the cache ([`live_lookup`]).
    Generate,
}

/// The decision by the request's properties — port of `ThumbnailRequest::submit`.
/// `cwd` — the current directory (a relative path from QML).
pub fn plan(
    source: &str,
    resource: &str,
    wp_type: &str,
    timeline: bool,
    enabled: bool,
    cwd: &str,
) -> Plan {
    if !enabled || (source.is_empty() && resource.is_empty()) {
        return Plan::Idle;
    }
    if timeline {
        return if source.is_empty() {
            Plan::Failed(ERR_TIMELINE.to_string())
        } else {
            Plan::Direct(input_url(source, cwd))
        };
    }
    let generate = source.is_empty() && wp_type == "video";
    if !generate {
        if source.is_empty() && wp_type != "image" {
            return Plan::Failed(ERR_NO_PREVIEW.to_string());
        }
        let input = if source.is_empty() { resource } else { source };
        return Plan::Direct(input_url(input, cwd));
    }
    if !input_url(resource, cwd).starts_with("file://") {
        return Plan::Failed(ERR_NOT_LOCAL.to_string());
    }
    Plan::Generate
}

/// The original's `input_url`: `:/…` is a Qt resource (`qrc:/…`), a string with
/// a scheme is an address as is, the rest is a file path (a relative one —
/// from `cwd`).
pub fn input_url(value: &str, cwd: &str) -> String {
    if let Some(rest) = value.strip_prefix(":/") {
        return format!("qrc:/{rest}");
    }
    let has_scheme = value
        .split_once(':')
        .is_some_and(|(scheme, _)| {
            scheme.len() > 1
                && scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        });
    if has_scheme {
        return value.to_string();
    }
    let path = if value.starts_with('/') {
        value.to_string()
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), value)
    };
    format!("file://{}", clean_path(&path))
}

/// `QDir::cleanPath`: without `.`, `..` and repeated `/`.
fn clean_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}

/// Root of the thumbnail cache: `WAYWALLEN_THUMB_DIR`, otherwise
/// `$XDG_CACHE_HOME/thumbnails`, otherwise `~/.cache/thumbnails`. The same
/// directory the wallpaper engine and the file manager put thumbnails into —
/// taken once, it is visible to everyone.
pub fn thumb_root(thumb_dir: Option<&str>, xdg_cache_home: Option<&str>, home: &str) -> String {
    match thumb_dir {
        Some(v) if !v.is_empty() => v.trim_end_matches('/').to_string(),
        _ => format!("{}/thumbnails", crate::utils::cache_home(xdg_cache_home, home)),
    }
}

/// The file URI under which the thumbnail is registered in the cache and in the
/// `Thumb::URI` key. Without percent-encoding — the wallpaper engine computes
/// the key the same way, and only when the key matches is our cache shared
/// with it.
pub fn thumb_uri(abs_path: &str) -> String {
    format!("file://{abs_path}")
}

/// The `x-large` thumbnail directory inside the cache root.
pub fn cache_dir(root: &str) -> String {
    format!("{}/x-large", root.trim_end_matches('/'))
}

/// The thumbnail file: `<root>/x-large/<md5 of the URI>.png`.
pub fn cache_file(root: &str, abs_path: &str) -> String {
    let digest = Md5::digest(thumb_uri(abs_path).as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}/{hex}.png", cache_dir(root))
}

/// Fit a size into the `max_edge` square keeping proportions. A smaller one is
/// not stretched. `None` — the size is degenerate.
pub fn fit_inside(width: u32, height: u32, max_edge: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 || max_edge == 0 {
        return None;
    }
    if width <= max_edge && height <= max_edge {
        return Some((width, height));
    }
    let (w, h, m) = (u64::from(width), u64::from(height), u64::from(max_edge));
    if width >= height {
        Some((max_edge, (h * m / w).max(1) as u32))
    } else {
        Some(((w * m / h).max(1) as u32, max_edge))
    }
}

/// Whether a thumbnail lying in the cache is usable: it must not be older than
/// the source. The modification times of the files themselves are compared
/// (seconds) — opening the PNG for the `Thumb::MTime` key on the main thread is
/// too expensive.
pub fn cache_is_fresh(source_mtime: i64, thumb_mtime: i64) -> bool {
    thumb_mtime >= source_mtime
}

/// A command that gets one frame and outputs it as PNG to stdout.
///
/// For a clip the frame is chosen by the `thumbnail` filter from the first
/// [`VIDEO_PROBE_FRAMES`] frames; for a picture (the fallback path when the
/// format cannot be read by our own means) the single frame is taken. Scale —
/// fit into [`MAX_EDGE`] without stretching.
pub fn frame_argv(path: &str, is_video: bool) -> Vec<String> {
    let scale = format!(
        "scale=w='min({e},iw)':h='min({e},ih)':force_original_aspect_ratio=decrease",
        e = MAX_EDGE
    );
    let filter = if is_video {
        format!("thumbnail=n={VIDEO_PROBE_FRAMES},{scale}")
    } else {
        scale
    };
    [
        crate::wallpaper_engine::FFMPEG_BIN,
        "-v", "error", "-nostdin",
        "-i", path,
        "-map", "0:v:0", "-an", "-sn",
        "-vf", &filter,
        "-frames:v", "1",
        "-f", "image2pipe", "-c:v", "png", "-",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Build a thumbnail PNG from RGBA pixels with the standard's three keys.
pub fn encode_thumb_png(
    rgba: &[u8],
    width: u32,
    height: u32,
    uri: &str,
    source_mtime: i64,
    source_size: u64,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || rgba.len() != (width as usize) * (height as usize) * 4 {
        return Err(format!("bad frame: {width}x{height}, {} bytes", rgba.len()));
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // The standard names `tEXt`, but it holds only Latin-1: a URI with
        // non-Latin characters goes into `iTXt` (UTF-8 without loss).
        if uri.is_ascii() {
            encoder.add_text_chunk("Thumb::URI".to_string(), uri.to_string())
        } else {
            encoder.add_itxt_chunk("Thumb::URI".to_string(), uri.to_string())
        }
        .map_err(|e| e.to_string())?;
        encoder
            .add_text_chunk("Thumb::MTime".to_string(), source_mtime.to_string())
            .map_err(|e| e.to_string())?;
        encoder
            .add_text_chunk("Thumb::Size".to_string(), source_size.to_string())
            .map_err(|e| e.to_string())?;
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Decode a picture from memory and shrink it to [`MAX_EDGE`].
pub fn decode_and_fit(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let (w, h) = fit_inside(img.width(), img.height(), MAX_EDGE)
        .ok_or_else(|| "empty image".to_string())?;
    let img = if (w, h) == (img.width(), img.height()) {
        img
    } else {
        img.resize_exact(w, h, image::imageops::FilterType::Triangle)
    };
    Ok((img.to_rgba8().into_raw(), w, h))
}

// ── the live part: disk and ffmpeg ─────────────────────────────────────────

/// There is no thumbnail — it must be made. Everything the worker thread needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// Absolute path of the source; it is also the key by which identical
    /// requests of different cards merge into one job.
    pub path: String,
    pub is_video: bool,
    /// Where to write the ready PNG.
    pub cache_path: String,
    pub source_mtime: i64,
    pub source_size: u64,
}

/// Result of the quick check on the main thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// The thumbnail already lies in the cache — the path to the PNG.
    Ready(String),
    /// Nothing to make it from — the cause.
    Failed(String),
    /// Background work is needed.
    Miss(Pending),
}

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// [`lookup_in`] with the cache root from the process environment.
pub fn live_lookup(source: &str, resource: &str, wp_type: &str) -> Lookup {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    let root = thumb_root(
        std::env::var("WAYWALLEN_THUMB_DIR").ok().as_deref(),
        std::env::var("XDG_CACHE_HOME").ok().as_deref(),
        &home,
    );
    lookup_in(&root, source, resource, wp_type)
}

/// A quick check without decoding: is there a source? is there a thumbnail in
/// the cache that is not older than it? Only `stat` — fit for the main thread.
pub fn lookup_in(root: &str, source: &str, resource: &str, wp_type: &str) -> Lookup {
    let job = match resolve_job(source, resource, wp_type) {
        Ok(job) => job,
        Err(e) => return Lookup::Failed(e.to_string()),
    };
    let abs = std::path::absolute(&job.path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(job.path);
    let Ok(meta) = std::fs::metadata(&abs) else {
        return Lookup::Failed(ERR_NOT_FOUND.to_string());
    };
    if !meta.is_file() {
        return Lookup::Failed(ERR_NOT_FOUND.to_string());
    }
    let source_mtime = mtime_secs(&meta);
    let cache_path = cache_file(root, &abs);
    if let Ok(thumb) = std::fs::metadata(&cache_path) {
        if thumb.len() > 0 && cache_is_fresh(source_mtime, mtime_secs(&thumb)) {
            return Lookup::Ready(cache_path);
        }
    }
    Lookup::Miss(Pending {
        path: abs,
        is_video: job.is_video,
        cache_path,
        source_mtime,
        source_size: meta.len(),
    })
}

/// Run `ffmpeg` and take the frame's PNG from its output, no longer than
/// [`FRAME_TIMEOUT_SECS`].
fn live_ffmpeg_frame(path: &str, is_video: bool) -> Result<Vec<u8>, String> {
    let argv = frame_argv(path, is_video);
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", argv[0]))?;
    // The output is read by separate threads: otherwise a full pipe would
    // stop ffmpeg, while we would wait for it to finish.
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let mut stderr = child.stderr.take().ok_or("no stderr")?;
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        buf
    });
    let deadline = Instant::now() + Duration::from_secs(FRAME_TIMEOUT_SECS);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let png = out_reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();
    match status {
        None => Err(format!("{}: timeout", argv[0])),
        Some(s) if s.success() && !png.is_empty() => Ok(png),
        Some(_) => {
            let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no frame");
            Err(format!("{}: {}", argv[0], line.trim()))
        }
    }
}

/// Write a file so that a reader never sees half of it: into a temporary file
/// next to it (owner-only access) and rename it into place.
fn live_write_atomic(cache_path: &str, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let dir = Path::new(cache_path).parent().ok_or("no cache dir")?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let nonce = UNIX_EPOCH.elapsed().map_or(0, |d| d.subsec_nanos());
    let tmp = format!("{cache_path}.tmp.{}.{nonce:x}", std::process::id());
    let written = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_data()?;
        std::fs::rename(&tmp, cache_path)
    })();
    written.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{cache_path}: {e}")
    })
}

/// Make a thumbnail and put it into the cache. Slow (decoding, `ffmpeg`) — only
/// from a worker thread. Returns the path to the PNG.
pub fn live_generate(job: &Pending) -> Result<String, String> {
    // We read the picture ourselves; if that failed (a format we do not have)
    // — the same way as a clip.
    let own = if job.is_video {
        None
    } else {
        std::fs::read(&job.path).ok().and_then(|bytes| decode_and_fit(&bytes).ok())
    };
    let (rgba, width, height) = match own {
        Some(frame) => frame,
        None => decode_and_fit(&live_ffmpeg_frame(&job.path, job.is_video)?)?,
    };
    let png = encode_thumb_png(
        &rgba,
        width,
        height,
        &thumb_uri(&job.path),
        job.source_mtime,
        job.source_size,
    )?;
    live_write_atomic(&job.cache_path, &png)?;
    Ok(job.cache_path.clone())
}
