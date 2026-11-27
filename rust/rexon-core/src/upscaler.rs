//! Upscaling of low-resolution wallpapers — port of `rexon/extensions/upscaler.py`
//! (the `WallpaperUpscaler` class).
//!
//! Pure logic (deterministic input→output) is moved into the core:
//!   * `suggest_scale` — the recommended factor by resolution (thresholds
//!     480/800/1024/1920) and `needs_upscale`/`MIN_DESKTOP_WIDTH`;
//!   * parsing of `identify` output (`%w %h` → (w, h), error → (0, 0));
//!   * building the argv of EVERY strategy byte for byte: Real-ESRGAN (single
//!     `-s min(scale,4)` and two-pass x4→x2 when scale>4), waifu2x
//!     (`-s min(scale,4) -n 2`), center+blur (`convert (+clone …) +swap …`,
//!     target 1920×1080), ImageMagick Lanczos (`w*scale × h*scale`);
//!   * CHOOSING the strategy: the auto-probe order `realesrgan → waifu2x → center`,
//!     resolving the Real-ESRGAN binary (system PATH → local copy → download),
//!     the default dest (`{base}_upscaled{ext}`), early exits (identify
//!     failed / scale ≤ 1);
//!   * `process_wallpapers_in_dir` — walking a folder, filtering extensions, upscaling
//!     small ones in place.
//!
//! DESIGN: upscaling is interactive — the flow depends on subprocess results
//! (identify/realesrgan/waifu2x/convert) and on files that appear after them.
//! So, as in `importers.rs`, instead of a static plan there is the [`UpIo`] trait:
//! EVERY side-effecting call of the original (subprocess.run / shutil.which /
//! os.path.exists|isfile / os.access / os.remove / os.listdir / os.replace /
//! Real-ESRGAN download) is one trait method, in the same order. The parity
//! reference is a call log of live Python, read-only (subprocess intercepted by
//! a proxy, real upscale/identify/download were NOT run;
//! `tests/golden/upscaler_cases.json`); Rust must make the same calls in the same
//! order (argv/timeout byte for byte) and reach the same result.
//!
//! Quirks of the original are KEPT:
//!   * `upscale` checks only `w == 0` (not `h`) — with identify "100 0"
//!     the upscale still runs (kept);
//!   * `process_wallpapers_in_dir` has an `auto=True` parameter that is never
//!     used — omitted in Rust as dead;
//!   * with `method="auto"` `identify` is called many times for one file
//!     (the recursive `upscale` re-reads the size for every method) — kept;
//!   * center+blur ignores `scale` (always the 1920×1080 target) — kept;
//!   * FIXED (port, rule "fix bugs while porting"): the core first asks the
//!     executor for the picture size ([`UpIo::image_size`]) and only
//!     then calls `identify`. The original does exactly that — Pillow first, because
//!     SteamOS has no ImageMagick; without this hook the Rust path on such a
//!     machine always got (0, 0) and the upscale silently did not run. The log
//!     replay in parity tests is not affected: `image_size` defaults to `None`.
//!   * bug of the original: `_ensure_realesrgan` in the download branch uses
//!     `zipfile`/`_safe_zip_extract`, which are not imported in the module → any
//!     auto-download fails with `NameError`, is caught by `except` and gives ""
//!     (Real-ESRGAN never appears via auto-download). Here the download is
//!     delegated to the [`UpIo::download_realesrgan`] method; for parity with
//!     the original the executor returns `None` (the bug reproduced), while the live
//!     executor really downloads — this does NOT change the core.
//!
//! The EXECUTOR is not here: actually running identify/the upscalers, the
//!   download+unpacking of Real-ESRGAN itself (the body of `download_realesrgan`), file
//!   operations and logs. The live Rust executor is [`crate::imp_live::LiveUpIo`]
//!   (used by the tail of the Windows theme import); in a process with Python its role
//!   is played by `_LiveUpIo` from `rexon/extensions/upscaler.py`. Both override
//!   [`UpIo::image_size`]: Python via Pillow, Rust by parsing the header of
//!   the file itself ([`image_size_from_header`]), and neither of them needs ImageMagick
//!   for that. If the size cannot be obtained AT ALL, the executor must
//!   say so in words ([`note_upscale_skipped`]) rather than skip the step
//!   silently.

/// Result of `subprocess.run`: `ok` = returncode == 0, plus stdout (needed
/// for parsing `identify` output).
pub struct RunResult {
    pub ok: bool,
    pub stdout: Vec<u8>,
}

/// Executor interface: each method is exactly one side-effecting call of
/// the original, in execution order. In parity tests it is implemented by a log
/// replay; in the live bridge — by real I/O.
pub trait UpIo {
    /// `subprocess.run(argv, capture_output=True, timeout=...)`.
    fn run(&mut self, argv: Vec<String>, timeout: u64) -> RunResult;
    /// `shutil.which(name)` → a path or None.
    fn which(&mut self, name: &str) -> Option<String>;
    /// `os.path.exists`.
    fn exists(&mut self, path: &str) -> bool;
    /// `os.path.isfile`.
    fn is_file(&mut self, path: &str) -> bool;
    /// `os.access(path, os.X_OK)`.
    fn access_x(&mut self, path: &str) -> bool;
    /// `os.remove(path)` (errors suppressed by the original).
    fn remove_file(&mut self, path: &str);
    /// The Real-ESRGAN auto-download branch (`_ensure_realesrgan`, step 3): path to
    /// the binary or None. In the original always None (NameError, see the header).
    fn download_realesrgan(&mut self) -> Option<String>;
    /// `os.listdir(path)`.
    fn listdir(&mut self, path: &str) -> Vec<String>;
    /// `os.replace(src, dst)`.
    fn replace(&mut self, src: &str, dst: &str);

    /// The complete `get_image_size` answer, if the executor can measure the
    /// size itself.
    ///
    /// The original asks Pillow for the size FIRST (a hard dependency of
    /// Rexon, reads only the header) and only then runs `identify` —
    /// precisely because SteamOS has no ImageMagick at all. The core cannot
    /// call Pillow, and without this hook the Rust path on such a machine always
    /// got (0, 0) and the upscale silently did not happen, whereas the Python fallback
    /// worked. The live bridge answers here with the result of
    /// `WallpaperUpscaler.get_image_size` (which has both Pillow and `identify`).
    ///
    /// `None` — the executor cannot, the core takes the fallback path via
    /// `identify` (the log replay in parity tests stays that way).
    ///
    /// ⚠️ The answer `Some((0, 0))` is legitimate and means "could not measure with ANYTHING".
    /// The live executor answers so only after telling the person: a silently
    /// skipped upscale would be taken by the customer for normal operation.
    fn image_size(&mut self, _path: &str) -> Option<(i64, i64)> {
        None
    }
}

// ============================================================================
// Constants (port of the class fields)
// ============================================================================

/// `MIN_DESKTOP_WIDTH` — the `needs_upscale` threshold (max side smaller → upscale).
pub const MIN_DESKTOP_WIDTH: i64 = 1024;
/// `MIN_DESKTOP_HEIGHT` (declared in the original but not used in the logic).
pub const MIN_DESKTOP_HEIGHT: i64 = 768;

/// Real-ESRGAN auto-download URL (`_ESRGAN_URL`) — for the executor.
pub const ESRGAN_URL: &str =
    "https://github.com/xinntao/Real-ESRGAN/releases/download/\
     v0.2.5.0/realesrgan-ncnn-vulkan-20220424-ubuntu.zip";

/// Auto-probe order of strategies with `method="auto"`.
pub const AUTO_METHODS: [&str; 3] = ["realesrgan", "waifu2x", "center"];

/// The Real-ESRGAN model passed via `-n`.
const REALESRGAN_MODEL: &str = "realesrgan-x4plus";

/// Path of the local Real-ESRGAN copy (`_ESRGAN_DIR/realesrgan-ncnn-vulkan`) from
/// the home directory — the executor substitutes `expanduser("~")`.
pub fn esrgan_local_exe(home: &str) -> String {
    format!("{home}/.local/share/rexon/bin/realesrgan/realesrgan-ncnn-vulkan")
}

/// Image extensions for `process_wallpapers_in_dir` (`img_exts`).
const IMG_EXTS: [&str; 5] = [".jpg", ".jpeg", ".png", ".bmp", ".webp"];

// ============================================================================
// Helpers for Python semantics (os.path, int)
// ============================================================================

/// `os.path.splitext`: leading dots of the basename do not count as an extension.
fn splitext(p: &str) -> (&str, &str) {
    let base_start = p.rfind('/').map_or(0, |i| i + 1);
    let base = &p[base_start..];
    if let Some(dot_rel) = base.rfind('.') {
        if base[..dot_rel].chars().any(|c| c != '.') {
            let dot = base_start + dot_rel;
            return (&p[..dot], &p[dot..]);
        }
    }
    (p, "")
}

/// `os.path.join` for POSIX (two arguments are enough).
fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_string()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// Default dest: `{base}_upscaled{ext}` (port of the `dest_path is None` branch).
fn default_dest(src: &str) -> String {
    let (base, ext) = splitext(src);
    format!("{base}_upscaled{ext}")
}

/// `int(token)` (decimal, sign, ASCII digits). None — not a number.
fn py_int(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let (neg, digits) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i64>().ok().map(|v| if neg { -v } else { v })
}

/// `int(parts[0]), int(parts[1])` of `stdout.strip().split()`; on
/// IndexError/ValueError → (0, 0).
pub fn parse_identify(stdout: &[u8]) -> (i64, i64) {
    let s = String::from_utf8_lossy(stdout);
    let mut parts = s.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some(a), Some(b)) => match (py_int(a), py_int(b)) {
            (Some(w), Some(h)) => (w, h),
            _ => (0, 0),
        },
        _ => (0, 0),
    }
}

// ============================================================================
// Pure logic: thresholds
// ============================================================================

/// `suggest_scale(w, h)` → factor 2/4/8/1. Thresholds by the larger side.
/// Intentional divergence (structural, without behaviour change): the original has
/// two separate branches `<800 → 4` (XP/Vista) and `<1024 → 4` (HD-ready) with
/// the same result; here they are merged into `< 1024 → 4` (the values are
/// identical, golden covers 799/800/1023/1024).
pub fn suggest_scale(w: i64, h: i64) -> i64 {
    let longest = w.max(h);
    if longest < 480 {
        8 // mobile
    } else if longest < 1024 {
        4 // XP/Vista / HD-ready
    } else if longest < 1920 {
        2 // HD → Full HD
    } else {
        1 // already enough
    }
}

/// `get_image_size` — first ask the executor (the live bridge has
/// Pillow first there, as in the original), otherwise run identify + parse. (0, 0)
/// on error.
fn get_image_size(io: &mut dyn UpIo, path: &str) -> (i64, i64) {
    if let Some(size) = io.image_size(path) {
        return size;
    }
    let r = io.run(
        vec![
            "identify".into(),
            "-format".into(),
            "%w %h".into(),
            path.into(),
        ],
        10,
    );
    if r.ok {
        parse_identify(&r.stdout)
    } else {
        (0, 0)
    }
}

/// `needs_upscale` — the max side is smaller than `MIN_DESKTOP_WIDTH`. (0,0) → false.
pub fn needs_upscale(io: &mut dyn UpIo, path: &str) -> bool {
    let (w, h) = get_image_size(io, path);
    if w == 0 || h == 0 {
        return false;
    }
    w.max(h) < MIN_DESKTOP_WIDTH
}

// ============================================================================
// Real-ESRGAN resolution and strategies (argv byte for byte)
// ============================================================================

/// `_ensure_realesrgan`: system PATH → local copy → download.
/// None — not found (in the original the download always gives "" because of NameError).
fn ensure_realesrgan(io: &mut dyn UpIo, local_exe: &str) -> Option<String> {
    if let Some(exe) = io.which("realesrgan-ncnn-vulkan") {
        return Some(exe);
    }
    if io.is_file(local_exe) && io.access_x(local_exe) {
        return Some(local_exe.to_string());
    }
    io.download_realesrgan()
}

/// `_upscale_realesrgan`: single `-s min(scale,4)` or two-pass
/// x4→x2 when scale>4 (Real-ESRGAN cannot do x8 in one pass).
fn upscale_realesrgan(
    io: &mut dyn UpIo,
    src: &str,
    dest: &str,
    scale: i64,
    local_exe: &str,
) -> String {
    let exe = match ensure_realesrgan(io, local_exe) {
        Some(e) => e,
        None => return src.to_string(),
    };
    let r = if scale > 4 {
        let tmp = format!("{dest}.tmp.png");
        let r1 = io.run(
            vec![
                exe.clone(),
                "-i".into(),
                src.into(),
                "-o".into(),
                tmp.clone(),
                "-s".into(),
                "4".into(),
                "-n".into(),
                REALESRGAN_MODEL.into(),
            ],
            300,
        );
        if !r1.ok {
            return src.to_string();
        }
        let r2 = io.run(
            vec![
                exe.clone(),
                "-i".into(),
                tmp.clone(),
                "-o".into(),
                dest.into(),
                "-s".into(),
                "2".into(),
                "-n".into(),
                REALESRGAN_MODEL.into(),
            ],
            300,
        );
        io.remove_file(&tmp);
        r2
    } else {
        let s = scale.min(4);
        io.run(
            vec![
                exe,
                "-i".into(),
                src.into(),
                "-o".into(),
                dest.into(),
                "-s".into(),
                s.to_string(),
                "-n".into(),
                REALESRGAN_MODEL.into(),
            ],
            300,
        )
    };
    if r.ok && io.exists(dest) {
        dest.to_string()
    } else {
        src.to_string()
    }
}

/// `_upscale_waifu2x`: `-s min(scale,4) -n 2` (noise level 2, max x4).
fn upscale_waifu2x(io: &mut dyn UpIo, src: &str, dest: &str, scale: i64) -> String {
    let exe = match io.which("waifu2x-ncnn-vulkan") {
        Some(e) => e,
        None => return src.to_string(),
    };
    let s = scale.min(4);
    let r = io.run(
        vec![
            exe,
            "-i".into(),
            src.into(),
            "-o".into(),
            dest.into(),
            "-s".into(),
            s.to_string(),
            "-n".into(),
            "2".into(),
        ],
        300,
    );
    if r.ok && io.exists(dest) {
        dest.to_string()
    } else {
        src.to_string()
    }
}

/// `_upscale_center_blur`: the center on a blurred enlarged background (target tw×th).
/// `scale` is not used (the original's quirk kept).
fn upscale_center_blur(
    io: &mut dyn UpIo,
    src: &str,
    dest: &str,
    tw: i64,
    th: i64,
) -> String {
    let r = io.run(
        vec![
            "convert".into(),
            src.into(),
            "(".into(),
            "+clone".into(),
            "-resize".into(),
            format!("{tw}x{th}!"),
            "-blur".into(),
            "0x30".into(),
            "-brightness-contrast".into(),
            "-20x-10".into(),
            ")".into(),
            "+swap".into(),
            "-gravity".into(),
            "center".into(),
            "-composite".into(),
            "-resize".into(),
            format!("{tw}x{th}"),
            dest.into(),
        ],
        30,
    );
    if r.ok && io.exists(dest) {
        dest.to_string()
    } else {
        src.to_string()
    }
}

/// `_upscale_imagemagick`: Lanczos resize to `w*scale × h*scale` (re-reads
/// the original's size via identify).
fn upscale_imagemagick(io: &mut dyn UpIo, src: &str, dest: &str, scale: i64) -> String {
    let (w, h) = get_image_size(io, src);
    let nw = w * scale;
    let nh = h * scale;
    let r = io.run(
        vec![
            "convert".into(),
            src.into(),
            "-resize".into(),
            format!("{nw}x{nh}"),
            "-filter".into(),
            "Lanczos".into(),
            dest.into(),
        ],
        30,
    );
    if r.ok && io.exists(dest) {
        dest.to_string()
    } else {
        src.to_string()
    }
}

/// `WallpaperUpscaler.upscale`. `method`: "auto" (realesrgan→waifu2x→center),
/// "realesrgan"/"waifu2x"/"center"/"imagemagick"; otherwise — return src.
/// `dest_opt=None` → `{base}_upscaled{ext}`. `scale=0` → `suggest_scale`.
/// Returns the path to the result (or the source src on failure/when not needed).
pub fn upscale(
    io: &mut dyn UpIo,
    src: &str,
    dest_opt: Option<&str>,
    scale: i64,
    method: &str,
    local_exe: &str,
) -> String {
    let (w, h) = get_image_size(io, src);
    if w == 0 {
        return src.to_string();
    }
    let mut scale = scale;
    if scale == 0 {
        scale = suggest_scale(w, h);
    }
    if scale <= 1 {
        return src.to_string();
    }
    let dest = match dest_opt {
        Some(d) => d.to_string(),
        None => default_dest(src),
    };
    match method {
        "auto" => {
            for m in AUTO_METHODS {
                let result = upscale(io, src, Some(&dest), scale, m, local_exe);
                if result != src && io.exists(&result) {
                    return result;
                }
            }
            src.to_string()
        }
        "realesrgan" => upscale_realesrgan(io, src, &dest, scale, local_exe),
        "waifu2x" => upscale_waifu2x(io, src, &dest, scale),
        "center" => upscale_center_blur(io, src, &dest, 1920, 1080),
        "imagemagick" => upscale_imagemagick(io, src, &dest, scale),
        _ => src.to_string(),
    }
}

/// `process_wallpapers_in_dir`: upscale small wallpapers in a folder in place.
/// Returns the number processed. The original's `auto` parameter is omitted (dead).
pub fn process_wallpapers_in_dir(
    io: &mut dyn UpIo,
    data_dir: &str,
    local_exe: &str,
) -> i64 {
    let mut processed = 0i64;
    for f in io.listdir(data_dir) {
        let ext = splitext(&f).1.to_lowercase();
        if !IMG_EXTS.contains(&ext.as_str()) {
            continue;
        }
        let full = join(data_dir, &f);
        if !needs_upscale(io, &full) {
            continue;
        }
        let (w, h) = get_image_size(io, &full);
        let scale = suggest_scale(w, h);
        let tmp = format!("{full}.upscaled.png");
        let result = upscale(io, &full, Some(&tmp), scale, "auto", local_exe);
        if result != full && io.exists(&tmp) {
            io.replace(&tmp, &full);
            processed += 1;
        } else if io.exists(&tmp) {
            io.remove_file(&tmp);
        }
    }
    processed
}

// ============================================================================
// Picture size from its own header
// ============================================================================
//
// WHY. The original asked Pillow for the size and kept `identify` as the fallback
// path. In a process without Python there is no Pillow, and on a machine without ImageMagick
// the executor had nothing to measure with: the size came out (0, 0), `needs_upscale`
// answered "not needed", and the wallpapers from an imported Windows theme stayed at their
// 800×600 — SILENTLY. The customer would decide that this is how the program works.
//
// Most of the time, however, there is nothing to measure and no need to: width and height lie in the first
// bytes of the file itself, and parsing them is cheaper than running a foreign program.
// All five formats that ever reach the upscaler (`IMG_EXTS`) are parsed here,
// plus GIF — it costs three lines. `identify` remains the fallback
// for exotic cases (TIFF, HEIF, broken files), and if it is missing too, the executor
// must say so in words rather than stay silent (see [`note_upscale_skipped`]).

/// How many bytes from the start of the file are enough to learn the size.
///
/// For PNG/BMP/GIF/WebP the size lies in the first three dozen bytes, but JPEG
/// puts the frame (SOF marker) AFTER the metadata, and EXIF with a thumbnail takes
/// tens of kilobytes. The margin is generous on purpose: only the start of the
/// file is read anyway, not the whole file (wallpapers can be 20 MB).
pub const HEADER_PROBE_BYTES: usize = 256 * 1024;

fn be16(b: &[u8], at: usize) -> i64 {
    ((b[at] as i64) << 8) | b[at + 1] as i64
}

fn le16(b: &[u8], at: usize) -> i64 {
    (b[at] as i64) | ((b[at + 1] as i64) << 8)
}

fn le24(b: &[u8], at: usize) -> i64 {
    (b[at] as i64) | ((b[at + 1] as i64) << 8) | ((b[at + 2] as i64) << 16)
}

fn be32(b: &[u8], at: usize) -> i64 {
    ((b[at] as i64) << 24)
        | ((b[at + 1] as i64) << 16)
        | ((b[at + 2] as i64) << 8)
        | b[at + 3] as i64
}

fn le32(b: &[u8], at: usize) -> i64 {
    (b[at] as i64)
        | ((b[at + 1] as i64) << 8)
        | ((b[at + 2] as i64) << 16)
        | ((b[at + 3] as i64) << 24)
}

/// Picture size from the start of the file. `None` — the format is not recognised or the header
/// is truncated (then the calling side takes the fallback path via `identify`).
///
/// Zero and negative sides count as unrecognised: the answer "0×0" already
/// means "don't know" here, and there must not be a second value with the same
/// meaning.
pub fn image_size_from_header(head: &[u8]) -> Option<(i64, i64)> {
    let size = png_size(head)
        .or_else(|| gif_size(head))
        .or_else(|| bmp_size(head))
        .or_else(|| webp_size(head))
        .or_else(|| jpeg_size(head))?;
    if size.0 > 0 && size.1 > 0 {
        Some(size)
    } else {
        None
    }
}

/// PNG: the signature, then the IHDR chunk — width and height 4 bytes each from offset 16.
fn png_size(b: &[u8]) -> Option<(i64, i64)> {
    const SIGN: &[u8] = b"\x89PNG\r\n\x1a\n";
    if b.len() < 24 || &b[..8] != SIGN {
        return None;
    }
    Some((be32(b, 16), be32(b, 20)))
}

/// GIF: the "logical screen" right after the signature, both sides 2 bytes each.
fn gif_size(b: &[u8]) -> Option<(i64, i64)> {
    if b.len() < 10 || (&b[..6] != b"GIF87a" && &b[..6] != b"GIF89a") {
        return None;
    }
    Some((le16(b, 6), le16(b, 8)))
}

/// BMP: the file header is followed by the DIB, and its SIZE tells which
/// variant it is. The oldest one (BITMAPCOREHEADER, 12 bytes) has 2-byte sides,
/// all the others — 4-byte ones. The height can be negative (rows top to bottom),
/// so its absolute value is taken.
fn bmp_size(b: &[u8]) -> Option<(i64, i64)> {
    if b.len() < 26 || &b[..2] != b"BM" {
        return None;
    }
    if le32(b, 14) == 12 {
        return Some((le16(b, 18), le16(b, 20)));
    }
    let w = le32(b, 18) as i32 as i64;
    let h = le32(b, 22) as i32 as i64;
    Some((w.abs(), h.abs()))
}

/// WebP: a RIFF container, and the size lies in the first chunk — different for each of
/// the three variants (lossy, lossless, extended).
fn webp_size(b: &[u8]) -> Option<(i64, i64)> {
    if b.len() < 30 || &b[..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return None;
    }
    match &b[12..16] {
        // VP8 (lossy): after the three-byte frame tag — the signature and the sides,
        // 14 bits each (the top two bits are the scale, we do not need it).
        b"VP8 " => {
            if b[23..26] != [0x9D, 0x01, 0x2A] {
                return None;
            }
            Some((le16(b, 26) & 0x3FFF, le16(b, 28) & 0x3FFF))
        }
        // VP8L (lossless): 14 bits of width and 14 bits of height in a row, both
        // one less than the real ones.
        b"VP8L" => {
            if b[20] != 0x2F {
                return None;
            }
            let bits = le32(b, 21);
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        // VP8X (extended): canvas size in three bytes, also one
        // less than the real one.
        b"VP8X" => Some((le24(b, 24) + 1, le24(b, 27) + 1)),
        _ => None,
    }
}

/// JPEG: walking the markers from the start to the first frame (SOF). The frame lies AFTER
/// the metadata, so we have to follow the chain of lengths, not an offset.
fn jpeg_size(b: &[u8]) -> Option<(i64, i64)> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 1 < b.len() {
        if b[i] != 0xFF {
            // Garbage between segments: look for the next marker rather than give up.
            i += 1;
            continue;
        }
        // Consecutive 0xFF bytes are fill, the marker is the next byte.
        let mut j = i + 1;
        while j < b.len() && b[j] == 0xFF {
            j += 1;
        }
        if j >= b.len() {
            return None;
        }
        let marker = b[j];
        i = j + 1;
        match marker {
            // Markers without a segment: signature, restart, "empty".
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            // End of image or start of compressed data — there is no frame further on.
            0xD9 | 0xDA => return None,
            _ => {}
        }
        if i + 1 >= b.len() {
            return None;
        }
        let seg = be16(b, i) as usize;
        if seg < 2 {
            return None;
        }
        // SOF0…SOF15, except Huffman tables (C4), arithmetic (C8) and
        // the delimiter (CC): they share the range but carry no frame.
        let is_frame =
            (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC;
        if is_frame {
            if i + 6 >= b.len() {
                return None;
            }
            // [length][precision][height][width]
            return Some((be16(b, i + 5), be16(b, i + 3)));
        }
        i += seg;
    }
    None
}

/// The warning "upscale skipped because there is nothing to learn the size with".
///
/// Not an exception: the theme is installed, the wallpapers are in place, only an optional
/// step is skipped. But not silence either — the person must learn what happened and what to install
/// (rule #13). The package name is taken for THIS distribution.
pub fn note_upscale_skipped(family: &str) -> String {
    let hint = crate::utils::tool_package_hint("identify", family);
    let tail = if hint.is_empty() {
        "Поставьте ImageMagick, чтобы Rexon смог их увеличить.".to_string()
    } else {
        format!("{hint}, чтобы Rexon смог их увеличить.")
    };
    format!(
        "Размер части обоев определить нечем — они оставлены как есть, \
         без увеличения. {tail}"
    )
}

#[cfg(test)]
mod header_tests {
    use super::*;

    #[test]
    fn png_header_is_read_without_any_external_program() {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        b.extend_from_slice(&[0, 0, 0, 13]);
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&800u32.to_be_bytes());
        b.extend_from_slice(&600u32.to_be_bytes());
        assert_eq!(image_size_from_header(&b), Some((800, 600)));
    }

    #[test]
    fn gif_and_bmp_headers_are_read() {
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&320u16.to_le_bytes());
        gif.extend_from_slice(&240u16.to_le_bytes());
        assert_eq!(image_size_from_header(&gif), Some((320, 240)));

        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0; 12]);
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&1024i32.to_le_bytes());
        // A negative height is a legitimate "top-down" BMP.
        bmp.extend_from_slice(&(-768i32).to_le_bytes());
        assert_eq!(image_size_from_header(&bmp), Some((1024, 768)));
    }

    #[test]
    fn jpeg_frame_is_found_behind_metadata() {
        let mut b = vec![0xFF, 0xD8];
        // A 16-byte APP0 — the frame lies after it, as in real life.
        b.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
        b.extend_from_slice(&[0u8; 14]);
        // SOF0: length, precision, height, width.
        b.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        b.extend_from_slice(&480u16.to_be_bytes());
        b.extend_from_slice(&640u16.to_be_bytes());
        b.extend_from_slice(&[0u8; 6]);
        assert_eq!(image_size_from_header(&b), Some((640, 480)));
    }

    #[test]
    fn webp_lossy_and_lossless_are_read() {
        let mut lossy = b"RIFF".to_vec();
        lossy.extend_from_slice(&[0; 4]);
        lossy.extend_from_slice(b"WEBPVP8 ");
        lossy.extend_from_slice(&[0; 4]); // chunk length
        lossy.extend_from_slice(&[0, 0, 0]); // frame tag
        lossy.extend_from_slice(&[0x9D, 0x01, 0x2A]);
        lossy.extend_from_slice(&1280u16.to_le_bytes());
        lossy.extend_from_slice(&720u16.to_le_bytes());
        assert_eq!(image_size_from_header(&lossy), Some((1280, 720)));

        let mut ll = b"RIFF".to_vec();
        ll.extend_from_slice(&[0; 4]);
        ll.extend_from_slice(b"WEBPVP8L");
        ll.extend_from_slice(&[0; 4]);
        ll.push(0x2F);
        // 14 bits of width and 14 bits of height, both one less than the real ones.
        let bits: u32 = (100 - 1) | ((50 - 1) << 14);
        ll.extend_from_slice(&bits.to_le_bytes());
        ll.extend_from_slice(&[0; 8]);
        assert_eq!(image_size_from_header(&ll), Some((100, 50)));
    }

    /// A foreign format and a truncated header must honestly answer "don't know" rather
    /// than invent a size: an invented one would lead the upscale to an empty place.
    #[test]
    fn unknown_and_truncated_headers_answer_nothing() {
        assert_eq!(image_size_from_header(b""), None);
        assert_eq!(image_size_from_header(b"II*\x00 tiff"), None);
        // A truncated PNG: the signature is there, the IHDR chunk is not yet.
        assert_eq!(image_size_from_header(b"\x89PNG\r\n\x1a\n\x00\x00"), None);
        // A PNG with a zero side is also "don't know", not (0, N).
        let mut zero = b"\x89PNG\r\n\x1a\n".to_vec();
        zero.extend_from_slice(&[0; 8]);
        zero.extend_from_slice(&0u32.to_be_bytes());
        zero.extend_from_slice(&600u32.to_be_bytes());
        assert_eq!(image_size_from_header(&zero), None);
    }
}
