//! Pure logic of YouTube wallpapers — port of `rexon/youtube_wallpaper.py`.
//!
//! The original `YouTubeWallpaperManager` renders video from YouTube (and ~1800
//! other sites via yt-dlp) as wallpaper in two modes:
//!   * **stream**   — `yt-dlp --get-url` returns a direct stream URL → mpv/mpvpaper;
//!   * **download** — `yt-dlp -f … -o …` downloads into `~/.cache/rexon/youtube/` →
//!     the local file is played.
//!
//! The class mixes deterministic decisions (URL validation/classification,
//! format choice by quality, building yt-dlp argv for info/stream/download/
//! playlist, cache paths by URL hash, parsing yt-dlp stdout — the direct URL, metadata
//! JSON, download progress, the format/playlist list, choosing files
//! for cache eviction) with side effects (real yt-dlp/mpv `subprocess`
//! calls, downloading, `os.scandir`/`makedirs`/`unlink`, reading/writing the
//! JSON cache, `threading`). Here — ONLY the pure part: input → output, without
//! the file system, network and processes.
//!
//! Observations of the environment (whether yt-dlp/ffmpeg is installed, the contents of yt-dlp
//! stdout, sizes/mtime of cache files, `time.time()`) are passed as PARAMETERS.
//! Actually running yt-dlp/mpv, downloading and working with the FS stay in the
//! Python shim `youtube_wallpaper.py`, which executes what the core decided
//! (argv, paths, format, the set of files to delete).
//!
//! Byte-for-byte parity with live Python — `tests/youtube_wallpaper_parity.rs`
//! (the reference was captured read-only, real yt-dlp/downloads were NOT run; stdout
//! is synthetic, HOME/cache normalised to `/HOME`).
//!
//! ⚠️ No crash bugs were found in the original during the port. The noted quirks
//! are kept as is — this is deliberate behaviour of the original:
//!   * `get_stream_url` takes only the FIRST line of `--get-url` (the video URL),
//!     dropping the audio URL with separate tracks — as in the original
//!     (the comment «Take the first one (video)»); this does not break mpv/mpvpaper.
//!   * empty/whitespace `--get-url` stdout raises `IndexError` in Python,
//!     which is swallowed by `except Exception → None`; here we honestly return
//!     `None` — the observable result is the same.

use serde_json::{Map, Value};

// ============================================================================
// Constants (port of the youtube_wallpaper.py module constants)
// ============================================================================

/// Video cache limit (MB) — port of `MAX_CACHE_MB`.
pub const MAX_CACHE_MB: u64 = 2048;
/// Maximum duration for downloading (s) — port of `MAX_DURATION_SEC`.
pub const MAX_DURATION_SEC: i64 = 3600;
/// Download timeout (s) — port of `MAX_DOWNLOAD_TIMEOUT`.
pub const MAX_DOWNLOAD_TIMEOUT: u64 = 600;

/// Freshness of the metadata cache (s) — port of `age < 3600` in `get_video_info`.
pub const META_CACHE_FRESH_SEC: f64 = 3600.0;

/// The `--socket-timeout` value in all yt-dlp calls.
const SOCKET_TIMEOUT: &str = "15";

/// Default yt-dlp format for streaming — port of `STREAM_FORMAT`
/// (matches `QUALITY_MAP["1080p"]`).
pub const STREAM_FORMAT: &str =
    "bestvideo[height<=1080][ext=mp4]+bestaudio[ext=m4a]/best[height<=1080]";

/// The "quality → yt-dlp format" table — port of `QUALITY_MAP`.
pub const QUALITY_MAP: &[(&str, &str)] = &[
    (
        "4k",
        "bestvideo[height<=2160][ext=mp4]+bestaudio[ext=m4a]/best[height<=2160]",
    ),
    (
        "1440p",
        "bestvideo[height<=1440][ext=mp4]+bestaudio[ext=m4a]/best[height<=1440]",
    ),
    (
        "1080p",
        "bestvideo[height<=1080][ext=mp4]+bestaudio[ext=m4a]/best[height<=1080]",
    ),
    (
        "720p",
        "bestvideo[height<=720][ext=mp4]+bestaudio[ext=m4a]/best[height<=720]",
    ),
    (
        "480p",
        "bestvideo[height<=480][ext=mp4]+bestaudio[ext=m4a]/best[height<=480]",
    ),
    ("best", "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best"),
];

/// Extensions of unfinished files excluded from the cache — port of the tuple
/// `(".part", ".temp", ".ytdl")`.
pub const INCOMPLETE_EXTS: &[&str] = &[".part", ".temp", ".ytdl"];

// ============================================================================
// Format choice — port of QUALITY_MAP.get(...)
// ============================================================================

/// Format lookup by quality key — port of `QUALITY_MAP.get(quality)`.
pub fn quality_format(quality: &str) -> Option<&'static str> {
    QUALITY_MAP
        .iter()
        .find(|(k, _)| *k == quality)
        .map(|(_, v)| *v)
}

/// Format for streaming — port of `QUALITY_MAP.get(quality, STREAM_FORMAT)`.
pub fn stream_format(quality: &str) -> &'static str {
    quality_format(quality).unwrap_or(STREAM_FORMAT)
}

/// Format for downloading — port of `QUALITY_MAP.get(quality, QUALITY_MAP["1080p"])`
/// (unknown quality → 1080p, as in the original).
pub fn download_format(quality: &str) -> &'static str {
    quality_format(quality).unwrap_or_else(|| quality_format("1080p").unwrap())
}

// ============================================================================
// Cache paths — port of _CACHE_DIR / _META_DIR / output template / meta_path
// ============================================================================

/// Video cache directory — port of `_CACHE_DIR`.
/// `xdg_cache_home` = `$XDG_CACHE_HOME` (Some — set, None — not set → `~/.cache`).
pub fn cache_dir(xdg_cache_home: Option<&str>, home: &str) -> String {
    let base = match xdg_cache_home {
        Some(x) => x.to_string(),
        None => format!("{home}/.cache"),
    };
    format!("{base}/rexon/youtube")
}

/// Metadata directory — port of `_META_DIR`.
pub fn meta_dir(cache_dir: &str) -> String {
    format!("{cache_dir}/meta")
}

/// Path of the metadata JSON cache by hash — port of `os.path.join(_META_DIR, f"{h}.json")`.
pub fn meta_path(meta_dir: &str, vid_hash: &str) -> String {
    format!("{meta_dir}/{vid_hash}.json")
}

/// The `-o` template for yt-dlp — port of `os.path.join(_CACHE_DIR, f"{h}.%(ext)s")`.
pub fn output_template(cache_dir: &str, vid_hash: &str) -> String {
    format!("{cache_dir}/{vid_hash}.%(ext)s")
}

/// Stable URL hash for the cache file name — port of `_video_id_hash`:
/// `sha256(url.utf8).hexdigest()[:16]`. The URL is passed already trimmed (`strip`)
/// — as the callers do in the original.
pub fn video_id_hash(url: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(url.as_bytes());
    let mut hex = String::with_capacity(16);
    for b in digest {
        if hex.len() >= 16 {
            break;
        }
        hex.push_str(&format!("{b:02x}"));
    }
    hex.truncate(16);
    hex
}

/// Whether the metadata cache is fresh — port of `age < 3600`.
pub fn meta_cache_fresh(age_sec: f64) -> bool {
    age_sec < META_CACHE_FRESH_SEC
}

// ============================================================================
// URL classification / validation — port of is_supported_url / is_youtube_url / …
// ============================================================================

/// Quick "is the URL supported" check — port of `is_supported_url`:
/// after `strip` it must start with `http://`/`https://`; empty → false.
pub fn is_supported_url(url: &str) -> bool {
    let u = url.trim();
    if u.is_empty() {
        return false;
    }
    u.starts_with("http://") || u.starts_with("https://")
}

/// Whether this is a YouTube URL — port of `is_youtube_url` (the `_YOUTUBE_RE.search` regex,
/// IGNORECASE). The regex is ported via substrings keeping the `search` and
/// `watch\?.*v=` semantics; the `https?://`/`www.|m.` prefixes are optional and do not
/// affect the `search` result, so they take no part in the check.
pub fn is_youtube_url(url: &str) -> bool {
    let l = url.to_lowercase();
    if l.contains("youtu.be/") {
        return true;
    }
    for pat in [
        "youtube.com/shorts/",
        "youtube.com/live/",
        "youtube.com/embed/",
        "youtube.com/v/",
        "youtube.com/playlist?",
    ] {
        if l.contains(pat) {
            return true;
        }
    }
    // `youtube.com/watch?` … `v=` on the same line (music.youtube.com is a special
    // case of the same substring). Port of `watch\?.*v=` (`.` does not match `\n`).
    watch_then_v(&l)
}

/// `youtube.com/watch?` followed by `v=` within the same line.
fn watch_then_v(lower: &str) -> bool {
    let needle = "youtube.com/watch?";
    let mut from = 0;
    while let Some(rel) = lower[from..].find(needle) {
        let after = from + rel + needle.len();
        // end of the current line (`.` in the regex does not match `\n`)
        let line_end = lower[after..]
            .find('\n')
            .map(|n| after + n)
            .unwrap_or(lower.len());
        if lower[after..line_end].contains("v=") {
            return true;
        }
        from = after;
    }
    false
}

/// Whether this is a YouTube playlist — port of `is_playlist_url` (`"playlist?list=" in url`).
pub fn is_playlist_url(url: &str) -> bool {
    url.contains("playlist?list=")
}

// ============================================================================
// yt-dlp argv — port of cmd in get_video_info / get_stream_url / download / playlist
// ============================================================================

/// argv of `yt-dlp --dump-json` (metadata) — port of cmd from `get_video_info`.
/// `url` is passed already trimmed (`strip`), as in the original.
pub fn info_argv(url: &str) -> Vec<String> {
    vec![
        "yt-dlp".into(),
        "--no-warnings".into(),
        "--no-playlist".into(),
        "--dump-json".into(),
        "--no-download".into(),
        "--socket-timeout".into(),
        SOCKET_TIMEOUT.into(),
        url.into(),
    ]
}

/// argv of `yt-dlp --get-url` (direct stream URL) — port of cmd from `get_stream_url`.
/// `fmt` is the result of [`stream_format`]; `url` is already trimmed.
pub fn stream_argv(url: &str, fmt: &str) -> Vec<String> {
    vec![
        "yt-dlp".into(),
        "--no-warnings".into(),
        "--no-playlist".into(),
        "--get-url".into(),
        "-f".into(),
        fmt.into(),
        "--socket-timeout".into(),
        SOCKET_TIMEOUT.into(),
        url.into(),
    ]
}

/// argv of `yt-dlp` for downloading into the cache — port of cmd from `download`.
/// `fmt` is the result of [`download_format`]; `output_tmpl` — [`output_template`];
/// `url` is already trimmed.
pub fn download_argv(url: &str, fmt: &str, output_tmpl: &str) -> Vec<String> {
    vec![
        "yt-dlp".into(),
        "--no-warnings".into(),
        "--no-playlist".into(),
        "-f".into(),
        fmt.into(),
        "--merge-output-format".into(),
        "mp4".into(),
        "--socket-timeout".into(),
        SOCKET_TIMEOUT.into(),
        "--no-part".into(),
        "--newline".into(),
        "-o".into(),
        output_tmpl.into(),
        url.into(),
    ]
}

/// argv of `yt-dlp --flat-playlist --dump-json` — port of cmd from `get_playlist_entries`.
/// `url` is already trimmed.
pub fn playlist_argv(url: &str, max_entries: u64) -> Vec<String> {
    vec![
        "yt-dlp".into(),
        "--no-warnings".into(),
        "--flat-playlist".into(),
        "--dump-json".into(),
        "--playlist-end".into(),
        max_entries.to_string(),
        "--socket-timeout".into(),
        SOCKET_TIMEOUT.into(),
        url.into(),
    ]
}

// ── argv for installing / updating / querying the version of yt-dlp ───────
//
// Port of the "raw" fallback argv from install_ytdlp / update_ytdlp / ytdlp_version.
// The named privileged variant (`privileged_pkg_cmd`) is decided by the system layer —
// it is an observation/side effect of the executor; here only deterministic argv.

/// Name of the yt-dlp package — the same in all families.
pub const YTDLP_PACKAGE: &str = "yt-dlp";

/// Candidates for the pip binary in priority order.
///
/// On Debian/Ubuntu the `python3-pip` package installs `pip3`, and `pip` appears
/// only if `python-is-python3` is installed; Arch/Fedora/openSUSE have
/// both. So `pip3` first.
pub const PIP_BINARIES: [&str; 2] = ["pip3", "pip"];

/// argv for installing yt-dlp via pip.
///
/// `--break-system-packages` is NOT added by default: it is needed only where
/// the environment is marked by PEP 668 (Arch, Debian/Ubuntu), while Fedora and
/// openSUSE have no such mark and the extra flag is just noise. The right order
/// is first without the flag, and only if pip refused because of PEP 668
/// ([`pip_break_system_needed`]), retry with it.
pub fn pip_argv(pip_bin: &str, upgrade: bool, break_system: bool) -> Vec<String> {
    let mut v = vec![pip_bin.into(), "install".into(), "--user".into()];
    if break_system {
        v.push("--break-system-packages".into());
    }
    if upgrade {
        v.push("--upgrade".into());
    }
    v.push(YTDLP_PACKAGE.into());
    v
}

/// A pip refusal because of PEP 668 ("externally-managed-environment") — means
/// it is worth retrying with `--break-system-packages`.
pub fn pip_break_system_needed(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("externally-managed-environment") || s.contains("externally managed")
}

/// Result of installing via the distribution package manager.
pub fn msg_installed_via(pkg_manager: &str) -> String {
    crate::i18n::trf("yt-dlp installed with %1", &[&pkg_manager])
}

/// Result of installing via pip.
pub fn msg_installed_via_pip(pip_bin: &str) -> String {
    crate::i18n::trf("yt-dlp installed with %1 (--user)", &[&pip_bin])
}

/// Result of updating via pip.
pub fn msg_updated_via_pip(pip_bin: &str) -> String {
    crate::i18n::trf("yt-dlp updated with %1", &[&pip_bin])
}

/// Complete installation failure: what the person should do next.
pub fn msg_install_failed() -> String {
    crate::i18n::tr("Could not install yt-dlp automatically. Install it manually: pip install yt-dlp")
}

/// No yt-dlp — nothing to update.
pub fn msg_not_installed() -> String {
    crate::i18n::tr("yt-dlp is not installed")
}

// ── live download into the cache ───────────────────────────────────────────

/// The downloaded clip file among the names of the cache directory — port of `_find_cached`.
/// The order of names is sorted: in Python it was the file system order.
pub fn cached_file_name(names: &[String], vid_hash: &str) -> Option<String> {
    let mut hits: Vec<&String> = names.iter().filter(|n| is_cached_file(n, vid_hash)).collect();
    hits.sort();
    hits.first().map(|n| (*n).clone())
}

/// Files of the cache directory: (path, name, mtime, size).
fn cache_files_live(dir: &str) -> Vec<(String, String, f64, u64)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read.filter_map(Result::ok)
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            Some((
                e.path().to_string_lossy().into_owned(),
                e.file_name().to_string_lossy().into_owned(),
                mtime,
                meta.len(),
            ))
        })
        .collect()
}

fn find_cached_live(dir: &str, vid_hash: &str) -> Option<String> {
    let names: Vec<String> = cache_files_live(dir).into_iter().map(|f| f.1).collect();
    cached_file_name(&names, vid_hash).map(|name| format!("{dir}/{name}"))
}

/// Clip metadata — port of `get_video_info`: a fresh cached card, otherwise
/// `yt-dlp --dump-json` and writing the card to the cache.
fn video_info_live(url: &str, cache: &str, vid_hash: &str) -> Option<Value> {
    let meta_dir = meta_dir(cache);
    let meta = meta_path(&meta_dir, vid_hash);
    if let Ok(m) = std::fs::metadata(&meta) {
        let age = m
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(f64::MAX);
        if meta_cache_fresh(age) {
            if let Some(v) = std::fs::read_to_string(&meta)
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            {
                return Some(v);
            }
        }
    }
    let (rc, stdout, stderr) = run_live(&info_argv(url), Some(30))?;
    if rc != 0 {
        crate::log_warn!("yt-dlp", "сведения о ролике не получены: {}", stderr.trim());
        return None;
    }
    let data: Value = serde_json::from_str(&stdout).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let info = build_video_info(&data, url, vid_hash, now);
    if std::fs::create_dir_all(&meta_dir).is_ok() {
        if let Ok(text) = serde_json::to_string_pretty(&info) {
            let _ = crate::utils::atomic_write(&meta, &text);
        }
    }
    Some(info)
}

/// Download a clip into the cache — port of `YouTubeWallpaperManager.download`. The path to
/// the file or `None` (the reason is in the log). An already downloaded one is taken from the cache;
/// a too long clip and a live stream are not downloaded; before downloading the cache
/// is shrunk to the limit.
pub fn download_live(url: &str, quality: &str) -> Option<String> {
    crate::utils::which("yt-dlp", None)?;
    let url = url.trim();
    let vid_hash = video_id_hash(url);
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let xdg = std::env::var("XDG_CACHE_HOME").ok().filter(|v| !v.is_empty());
    let cache = cache_dir(xdg.as_deref(), &home);
    std::fs::create_dir_all(&cache).ok()?;

    if let Some(path) = find_cached_live(&cache, &vid_hash) {
        crate::log_info!("yt-dlp", "ролик найден в кеше: {path}");
        return Some(path);
    }
    if let Some(info) = video_info_live(url, &cache, &vid_hash) {
        let duration = info
            .get("duration")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let live = info.get("is_live").and_then(Value::as_bool).unwrap_or(false);
        if let Some(reason) = download_block_reason(duration as i64, live) {
            crate::log_info!("yt-dlp", "ролик не скачивается: {reason}");
            return None;
        }
    }
    // Free up space in the cache.
    let files = cache_files_live(&cache);
    let sizes: Vec<u64> = files.iter().map(|f| f.3).collect();
    let triples: Vec<(String, f64, u64)> =
        files.iter().map(|f| (f.0.clone(), f.2, f.3)).collect();
    for path in select_evictions(dir_size_mb(&sizes), &triples) {
        let _ = std::fs::remove_file(path);
    }

    let fmt = download_format(quality);
    let argv = download_argv(url, fmt, &output_template(&cache, &vid_hash));
    let (rc, _, stderr) = run_live(&argv, Some(MAX_DOWNLOAD_TIMEOUT))?;
    if rc != 0 {
        crate::log_warn!("yt-dlp", "загрузка не удалась (код {rc}): {}", stderr.trim());
        return None;
    }
    find_cached_live(&cache, &vid_hash)
}

/// The update succeeded.
pub fn msg_updated() -> String {
    crate::i18n::tr("yt-dlp updated")
}

/// Updating failed both with yt-dlp itself and via pip.
pub fn msg_update_failed() -> String {
    crate::i18n::tr("Could not update yt-dlp")
}

/// Result of running a command: (exit code, stdout, stderr); `None` — the command did not
/// get to an exit code (no program, timed out).
pub type RunResult = Option<(i32, String, String)>;

/// Run pip; on a PEP 668 refusal retry with `--break-system-packages` —
/// port of `YouTubeWallpaperManager._pip_run`. Running is done outside (`run`), so that
/// the order of attempts is checked without a single process.
///
/// The flag is not added up front: it is needed only where the environment is marked by
/// PEP 668 (Arch, Debian/Ubuntu); we decide by the actual refusal.
pub fn pip_run_with(run: &mut dyn FnMut(&[String]) -> RunResult, pip_bin: &str, upgrade: bool) -> bool {
    for break_system in [false, true] {
        let argv = pip_argv(pip_bin, upgrade, break_system);
        let Some((rc, stdout, stderr)) = run(&argv) else {
            return false;
        };
        if rc == 0 {
            return true;
        }
        if break_system {
            return false;
        }
        let said = if stderr.is_empty() { stdout } else { stderr };
        if !pip_break_system_needed(&said) {
            return false;
        }
    }
    false
}

/// Live run of a command with a time limit.
fn run_live(argv: &[String], timeout_secs: Option<u64>) -> RunResult {
    let report = crate::utils::run(
        &crate::utils::RunSpec {
            argv: argv.to_vec(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs,
            ..Default::default()
        },
        None,
    );
    let rc = report.returncode?;
    Some((
        rc,
        crate::utils::decode_text(&report.stdout),
        crate::utils::decode_text(&report.stderr),
    ))
}

/// The pip binary on the system — in the [`PIP_BINARIES`] preference order.
fn pip_binary_live() -> Option<String> {
    PIP_BINARIES
        .iter()
        .find(|name| crate::utils::which(name, None).is_some())
        .map(|name| (*name).to_string())
}

/// Install yt-dlp — port of `YouTubeWallpaperManager.install_ytdlp`.
///
/// First the shared package installation mechanism (`system_plan::install_plan` —
/// the same as for installing desktops: it knows all families and itself
/// chooses the polkit helper or raw pkexec), then pip. Live run:
/// asks for the password via polkit.
pub fn install_ytdlp_live() -> (bool, String) {
    let sys = crate::system_exec::live();
    let family = sys.family();
    if family != crate::system::PkgFamily::Unknown {
        let packages = vec![YTDLP_PACKAGE.to_string()];
        if let Ok(plan) = crate::system_plan::install_plan(
            family,
            &packages,
            false,
            None,
            sys.pkg_helper_available(),
        ) {
            if plan.early_result.is_none() {
                let cmd = plan.steps.iter().find_map(|step| match step {
                    crate::system_plan::PlanStep::Run { cmd, .. } => Some(cmd.clone()),
                    _ => None,
                });
                if let Some(cmd) = cmd {
                    // The window's privilege session (one password); without it — the previous plan
                    // command. Neither worked — then pip as the
                    // user (not raw pkexec).
                    let op = crate::privilege::op::PrivOp::PkgInstall {
                        family,
                        packages: packages.clone(),
                        source: crate::privilege::op::PkgSource::YtDlp,
                    };
                    let session = if cmd.argv.first().map(String::as_str) == Some("pkexec") {
                        crate::privilege::run::run_live(&op, &[], None, &mut |_| {})
                    } else {
                        None
                    };
                    let rc = match session {
                        Some(out) => out.returncode,
                        None => run_live(&cmd.argv, cmd.timeout_secs).map(|(rc, _, _)| rc),
                    };
                    if rc == Some(0) {
                        return (
                            true,
                            msg_installed_via(&crate::system_plan::pkg_manager_name(family, None)),
                        );
                    }
                }
            }
        }
    }
    if let Some(pip) = pip_binary_live() {
        if pip_run_with(&mut |argv| run_live(argv, Some(120)), &pip, false) {
            return (true, msg_installed_via_pip(&pip));
        }
    }
    (false, msg_install_failed())
}

/// Update yt-dlp — port of `YouTubeWallpaperManager.update_ytdlp`.
///
/// ⭐ Fix: on success Python showed the person the raw English output of
/// `yt-dlp --update` ("Updated yt-dlp to stable@…"). Now the answer is a translated
/// phrase, and the output stays in the log.
pub fn update_ytdlp_live() -> (bool, String) {
    if crate::utils::which("yt-dlp", None).is_none() {
        return (false, msg_not_installed());
    }
    if let Some((0, stdout, _)) = run_live(&update_self_argv(), Some(60)) {
        let said = stdout.trim();
        if !said.is_empty() {
            crate::log_info!("yt-dlp", "обновление: {said}");
        }
        return (true, msg_updated());
    }
    if let Some(pip) = pip_binary_live() {
        if pip_run_with(&mut |argv| run_live(argv, Some(120)), &pip, true) {
            return (true, msg_updated_via_pip(&pip));
        }
    }
    (false, msg_update_failed())
}

// ── legacy argv ────────────────────────────────────────────────────────────
//
// The three functions below are the "own pacman/apt/dnf enumeration" because of which on
// openSUSE (priority 3 in support) the button answered «Unknown package
// manager». The shim does NOT call them any more: installation goes through the shared mechanism
// `system.PackageManager.install` (rule #12), which knows all families.
// They are kept only because the bridge exports still look at them
// (`rust/rexon-py/src/lib.rs`); remove them together with the exports.

/// `pkexec pacman -S --noconfirm yt-dlp` (Arch/CachyOS).
pub fn install_pacman_argv() -> Vec<String> {
    vec![
        "pkexec".into(),
        "pacman".into(),
        "-S".into(),
        "--noconfirm".into(),
        "yt-dlp".into(),
    ]
}

/// `pkexec apt install -y yt-dlp` (Ubuntu/Debian).
pub fn install_apt_argv() -> Vec<String> {
    vec![
        "pkexec".into(),
        "apt".into(),
        "install".into(),
        "-y".into(),
        "yt-dlp".into(),
    ]
}

/// `pkexec dnf install -y yt-dlp` (Fedora).
pub fn install_dnf_argv() -> Vec<String> {
    vec![
        "pkexec".into(),
        "dnf".into(),
        "install".into(),
        "-y".into(),
        "yt-dlp".into(),
    ]
}

/// `pip install --user --break-system-packages yt-dlp` (fallback).
pub fn install_pip_argv() -> Vec<String> {
    vec![
        "pip".into(),
        "install".into(),
        "--user".into(),
        "--break-system-packages".into(),
        "yt-dlp".into(),
    ]
}

/// `yt-dlp --update` — self-update.
pub fn update_self_argv() -> Vec<String> {
    vec!["yt-dlp".into(), "--update".into()]
}

/// `pip install --user --break-system-packages --upgrade yt-dlp` (fallback).
pub fn update_pip_argv() -> Vec<String> {
    vec![
        "pip".into(),
        "install".into(),
        "--user".into(),
        "--break-system-packages".into(),
        "--upgrade".into(),
        "yt-dlp".into(),
    ]
}

/// `yt-dlp --version`.
pub fn version_argv() -> Vec<String> {
    vec!["yt-dlp".into(), "--version".into()]
}

// ============================================================================
// Parsing yt-dlp stdout
// ============================================================================

/// Parsing the stdout of `yt-dlp --get-url` — port of the tail of `get_stream_url`:
/// take the FIRST line of the trimmed output, trim it and return it if it
/// starts with `http`. Empty/non-URL-like output → None.
pub fn parse_stream_url(stdout: &str) -> Option<String> {
    let first = stdout.trim().lines().next()?;
    let stream = first.trim();
    if stream.starts_with("http") {
        Some(stream.to_string())
    } else {
        None
    }
}

/// Parsing a progress line — port of `_parse_progress`:
///   * `[download]<spaces><number>%` → the number (float);
///   * the line contains `[Merger]`/`[ExtractAudio]` → 99.0;
///   * otherwise None.
///
/// A non-numeric capture (`float(...)` → ValueError) is swallowed in Python → falls through to
/// the merge check; here we do the same on a failed parse.
pub fn parse_progress(line: &str) -> Option<f64> {
    if let Some(pct) = download_pct(line) {
        return Some(pct);
    }
    if line.contains("[Merger]") || line.contains("[ExtractAudio]") {
        return Some(99.0);
    }
    None
}

/// Implementation of the regex `\[download\]\s+([\d.]+)%` with `re.search` semantics.
fn download_pct(line: &str) -> Option<f64> {
    let needle = "[download]";
    let chars: Vec<char> = line.chars().collect();
    let mut from = 0;
    while let Some(rel) = line[from..].find(needle) {
        let byte_pos = from + rel;
        let char_pos = line[..byte_pos].chars().count();
        let mut i = char_pos + needle.chars().count();
        // \s+ — at least one whitespace character
        let ws_start = i;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i > ws_start {
            // [\d.]+ — at least one character from digits/dot
            let num_start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i > num_start && i < chars.len() && chars[i] == '%' {
                let num: String = chars[num_start..i].iter().collect();
                if let Ok(v) = num.parse::<f64>() {
                    return Some(v);
                }
                // ValueError in Python → fall through (return None below)
            }
        }
        from = byte_pos + needle.len();
    }
    None
}

// ============================================================================
// Parsing yt-dlp JSON: format list / video card / playlist
// ============================================================================

/// Truthiness of a value by the rules of Python `bool(x)`
/// (for ports of expressions like `A or B`, `if not h`).
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `dict.get(key, default)` — the value if the key is present (even `null`),
/// otherwise `default`.
fn get_or(obj: &Value, key: &str, default: Value) -> Value {
    obj.get(key).cloned().unwrap_or(default)
}

/// Extract the list of video formats — port of `_extract_format_list`.
/// Skip those without height / `<240` / a repeated height; sort by height ↓.
pub fn extract_format_list(data: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let empty = Vec::new();
    let formats = data
        .get("formats")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    for fmt in formats {
        let h = fmt.get("height").cloned().unwrap_or(Value::Null);
        // `if not h or h < 240`
        if !truthy(&h) {
            continue;
        }
        let hi = match h.as_i64() {
            Some(n) => n,
            None => continue, // not a number → like "no height"
        };
        if hi < 240 {
            continue;
        }
        let res_key = format!("{hi}p");
        if seen.contains(&res_key) {
            continue;
        }
        seen.push(res_key.clone());

        // filesize: `fmt.get("filesize") or fmt.get("filesize_approx", 0)`
        let fs = fmt.get("filesize").cloned().unwrap_or(Value::Null);
        let filesize = if truthy(&fs) {
            fs
        } else {
            get_or(fmt, "filesize_approx", Value::from(0))
        };

        let mut m = Map::new();
        m.insert("resolution".into(), Value::from(res_key));
        m.insert("height".into(), Value::from(hi));
        m.insert("fps".into(), get_or(fmt, "fps", Value::from(30)));
        m.insert("vcodec".into(), get_or(fmt, "vcodec", Value::from("")));
        m.insert("filesize".into(), filesize);
        out.push(Value::Object(m));
    }
    // sort by height ↓ (stable, like list.sort in Python)
    out.sort_by(|a, b| {
        let hb = b["height"].as_i64().unwrap_or(0);
        let ha = a["height"].as_i64().unwrap_or(0);
        hb.cmp(&ha)
    });
    out
}

/// Build the cacheable video card from yt-dlp JSON — port of building `info`
/// in `get_video_info`. `vid_hash` — [`video_id_hash`], `fetched_at` —
/// `time.time()` (a side effect of the executor), `url` — the original one (fallback
/// `webpage_url`).
pub fn build_video_info(data: &Value, url: &str, vid_hash: &str, fetched_at: f64) -> Value {
    // description: (data.get("description","") or "")[:500]
    let desc_raw = get_or(data, "description", Value::from(""));
    let desc = if desc_raw.is_string() && truthy(&desc_raw) {
        desc_raw.as_str().unwrap().to_string()
    } else {
        String::new()
    };
    let description = truncate_chars(&desc, 500);

    let mut m = Map::new();
    m.insert("id".into(), get_or(data, "id", Value::from("")));
    m.insert(
        "title".into(),
        get_or(data, "title", Value::from("Без названия")),
    );
    m.insert("duration".into(), get_or(data, "duration", Value::from(0)));
    m.insert("thumbnail".into(), get_or(data, "thumbnail", Value::from("")));
    m.insert("uploader".into(), get_or(data, "uploader", Value::from("")));
    m.insert(
        "view_count".into(),
        get_or(data, "view_count", Value::from(0)),
    );
    m.insert(
        "webpage_url".into(),
        get_or(data, "webpage_url", Value::from(url)),
    );
    m.insert("is_live".into(), get_or(data, "is_live", Value::from(false)));
    m.insert("description".into(), Value::from(description));
    m.insert("formats".into(), Value::from(extract_format_list(data)));
    m.insert("_hash".into(), Value::from(vid_hash));
    m.insert("_fetched_at".into(), Value::from(fetched_at));
    Value::Object(m)
}

/// Parse the stdout of `yt-dlp --flat-playlist --dump-json` — port of the parsing in
/// `get_playlist_entries`: one line per video, invalid JSON is skipped.
pub fn parse_playlist(stdout: &str) -> Vec<Value> {
    let mut entries = Vec::new();
    for line in stdout.trim().lines() {
        let d: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // JSONDecodeError → continue
        };
        // url: d.get("url", d.get("webpage_url", ""))
        let url = if d.get("url").is_some() {
            d["url"].clone()
        } else {
            get_or(&d, "webpage_url", Value::from(""))
        };
        // thumbnail: d.get("thumbnails",[{}])[-1].get("url","") if d.get("thumbnails") else ""
        let thumbnail = match d.get("thumbnails") {
            Some(t) if truthy(t) => t
                .as_array()
                .and_then(|a| a.last())
                .map(|last| get_or(last, "url", Value::from("")))
                .unwrap_or_else(|| Value::from("")),
            _ => Value::from(""),
        };

        let mut m = Map::new();
        m.insert("id".into(), get_or(&d, "id", Value::from("")));
        m.insert(
            "title".into(),
            get_or(&d, "title", Value::from("Без названия")),
        );
        m.insert("duration".into(), get_or(&d, "duration", Value::from(0)));
        m.insert("url".into(), url);
        m.insert("thumbnail".into(), thumbnail);
        entries.push(Value::Object(m));
    }
    entries
}

/// A human-readable line about the video — port of `format_info_text`.
pub fn format_info_text(info: &Value) -> String {
    // dur = info.get("duration", 0) or 0
    let dur_val = get_or(info, "duration", Value::from(0));
    let dur = if truthy(&dur_val) {
        dur_val.as_f64().unwrap_or(0.0)
    } else {
        0.0
    };

    let dur_str = if dur > 0.0 {
        let total = dur.trunc() as i64; // int(dur)
        let (mins, secs) = (total / 60, total % 60);
        let (hrs, mins) = (mins / 60, mins % 60);
        if hrs != 0 {
            format!("{hrs}:{mins:02}:{secs:02}")
        } else {
            format!("{mins}:{secs:02}")
        }
    } else if truthy(&get_or(info, "is_live", Value::Null)) {
        "♾️ live".to_string()
    } else {
        "?".to_string()
    };

    // title = info.get("title", ""); truncated to 57 + "…" when len > 60
    let title_s = value_as_str(&get_or(info, "title", Value::from("")));
    let title = if title_s.chars().count() > 60 {
        format!("{}…", truncate_chars(&title_s, 57))
    } else {
        title_s
    };

    let uploader = value_as_str(&get_or(info, "uploader", Value::from("")));

    // max_res = formats[0]["resolution"] if formats else "?"
    let max_res = info
        .get("formats")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .and_then(|a| a[0].get("resolution"))
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string();

    format!("{title}  •  {uploader}  •  {dur_str}  •  до {max_res}")
}

/// String representation of a value for string fields (`title`/`uploader`).
fn value_as_str(v: &Value) -> String {
    match v.as_str() {
        Some(s) => s.to_string(),
        None => String::new(),
    }
}

/// Truncate a string to `n` CHARACTERS (the Python slice `s[:n]` works on code points).
fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

// ============================================================================
// Cache: identifying a file / eviction / download precheck
// ============================================================================

/// Whether this is our downloaded file — port of the `_find_cached` predicate:
/// the name starts with the hash and does not end with `.part`/`.temp`/`.ytdl`.
pub fn is_cached_file(name: &str, vid_hash: &str) -> bool {
    if !name.starts_with(vid_hash) {
        return false;
    }
    !INCOMPLETE_EXTS.iter().any(|ext| name.ends_with(ext))
}

/// Files for cache eviction — port of `_enforce_cache_limit`.
/// Input — the total cache size (MB) and a list of `(path, mtime, size_bytes)`;
/// output — paths in deletion order. If the cache is within the limit — empty.
/// Delete the oldest (by mtime ↑) until we fit within 80% of the limit.
pub fn select_evictions(size_mb: f64, files: &[(String, f64, u64)]) -> Vec<String> {
    if size_mb <= MAX_CACHE_MB as f64 {
        return Vec::new();
    }
    let mut sorted: Vec<&(String, f64, u64)> = files.iter().collect();
    // old ones first (stable sort by mtime, as in Python)
    sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let target = MAX_CACHE_MB as f64 * 0.8;
    let mut size = size_mb;
    let mut deleted = Vec::new();
    for (path, _, fsize) in sorted {
        if size <= target {
            break;
        }
        deleted.push(path.clone());
        size -= *fsize as f64 / (1024.0 * 1024.0);
    }
    deleted
}

/// Size of a set of files in MB — port of `_dir_size_mb` (sum of bytes / 1 MiB).
pub fn dir_size_mb(sizes: &[u64]) -> f64 {
    let total: u64 = sizes.iter().sum();
    total as f64 / (1024.0 * 1024.0)
}

/// Reason for refusing to download (a status for progress_cb) — port of the checks in
/// `download`: first the duration, then live. `None` — downloading is allowed.
/// The length is checked FIRST (as in the original).
pub fn download_block_reason(duration: i64, is_live: bool) -> Option<String> {
    if duration > MAX_DURATION_SEC {
        return Some(format!("Видео > {} мин", MAX_DURATION_SEC / 60));
    }
    if is_live {
        return Some("Live — используйте стриминг".to_string());
    }
    None
}
