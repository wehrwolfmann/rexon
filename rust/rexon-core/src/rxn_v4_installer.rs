//! Installer of `.rxn` v4 adaptive wallpapers — port of
//! `rexon/rxn_v4_installer.py` (PURE logic; side effects go through a plan or
//! through injected observations).
//!
//! The original unpacks an `.rxn` (a ZIP with a v4 `manifest.json`) into the
//! standard location `~/.local/share/rexon/wallpapers/<title-slug>-<short-hash>/`
//! and lays out the video/audio tracks. The whole deterministic part is ported
//! here:
//!   * [`slugify`]       — `title` → a file-system-safe slug (the same algorithm);
//!   * [`short_hash`]    — SHA-256 of the first 16 MiB of the file → 8 hex
//!     characters (the same algorithm: a prefix hash as a unique identifier, not
//!     a signature);
//!   * [`collect_manifest_paths`] / [`verify_paths_present`] — collecting the
//!     paths declared in the manifest and checking that they are present in the
//!     archive (error texts byte for byte, including the `{p!r}` repr);
//!   * [`plan_install_v4`] — the whole `install_rxn_v4` pipeline UP TO the real
//!     write: schema validation (reusing [`crate::manifest_v4`]), path checks,
//!     computing slug+hash and the target directory, the "already installed /
//!     install" decision — the result is an [`InstallOutcome`] with a PLAN of
//!     steps ([`InstallStep`]) modeled on `ext_manager.rs`/`system_plan.rs`;
//!   * [`build_listed_entry`] — enriching the manifest when listing
//!     (`_videos_abs`/`_audio_abs`/`_preview_path`/… — laying the tracks out as
//!     absolute paths, key order identical to Python);
//!   * [`remove_slug_error`] — slug validation for removal (anti-traversal).
//!
//! NOT ported (execution stays with Python/the bridge): reading `manifest.json`
//! from the ZIP itself (`parse_v4_manifest` — the types of its exceptions
//! FileNotFoundError/BadZipFile/KeyError/JSONDecodeError are part of the
//! contract, see `manifest_v4.py`), reading the archive's `namelist()`, the real
//! unpacking (`_safe_zip_extract`), creating directories, scanning
//! `INSTALL_ROOT`, `os.path.realpath`/`is_file` while listing, the
//! `_safe_rmtree` rollback when unpacking fails, logging, the CLI. The executor
//! does these, and the result (namelist, short hash, whether the directory
//! exists, whether files are present) is fed here as input. The parity
//! reference is golden data from live Python (`rxn_v4_installer_parity.rs`,
//! captured read-only).
//!
//! No bugs of the original were found during the review — the code is clean
//! (see the port report); there are no sanctioned divergences from Python.

use std::collections::HashSet;

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::manifest_v4::validate_v4_manifest;

/// Length of the short hash in hex characters (`_HASH_LEN`).
pub const HASH_LEN: usize = 8;
/// How many bytes of the file are hashed (the first 16 MiB, as in `_short_hash`).
pub const HASH_INPUT_LIMIT: usize = 16 * 1024 * 1024;

// ============================================================================
// Python semantics helpers
// ============================================================================

/// `os.path.join`/`PurePath /` for POSIX (two arguments are enough).
fn join(dir: &str, rel: &str) -> String {
    if rel.starts_with('/') || dir.is_empty() {
        rel.to_string()
    } else if dir.ends_with('/') {
        format!("{dir}{rel}")
    } else {
        format!("{dir}/{rel}")
    }
}

/// The last path component (`Path.name`).
fn basename(p: &str) -> &str {
    p.trim_end_matches('/').rsplit('/').next().unwrap_or(p)
}

/// Lexical normalization of an absolute POSIX path (the analogue of
/// `Path.resolve()` for a tree without symbolic links): removes `.`, collapses
/// `..`.
fn normalize_abs(p: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for comp in p.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            c => stack.push(c),
        }
    }
    format!("/{}", stack.join("/"))
}

/// `str((sub / rel).resolve())` for an already resolved absolute `sub` and a
/// relative `rel` (there are no symbolic links in a freshly unpacked tree — a
/// deliberate deviation from Python's realpath, as in `ext_utils.rs`).
fn resolve_join(sub: &str, rel: &str) -> String {
    normalize_abs(&join(sub, rel))
}

/// Truthiness of a value in the Python sense (`if v:`).
fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `repr()` of a string: quotes and escaping as in CPython (for `{p!r}`,
/// `{slug!r}` in error texts). Printable non-ASCII characters stay as they are;
/// `\`, `\n`, `\r`, `\t`, the quote and C0/DEL are escaped.
fn py_repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

// ============================================================================
// slugify + short_hash
// ============================================================================

/// `_slugify`: `title` → a file-system-safe slug.
///
/// `"The Witcher 3 — Geralt Theme"` → `"the-witcher-3-geralt-theme"`.
/// Matches Python's `re.sub(r"[^a-z0-9]+", "-", text.lower().strip())
/// .strip("-") or "wallpaper"`: only ASCII `a-z`/`0-9` survive (ASCII case
/// folds identically in Python and Rust), everything else becomes a run of `-`.
pub fn slugify(text: &str) -> String {
    let lowered = text.to_lowercase();
    let mut out = String::new();
    let mut dash = false;
    for c in lowered.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let stripped = out.trim_matches('-');
    if stripped.is_empty() {
        "wallpaper".to_string()
    } else {
        stripped.to_string()
    }
}

/// `_short_hash`: SHA-256 of the first `HASH_INPUT_LIMIT` bytes of `data` → the
/// first `HASH_LEN` hex characters. `data` is the contents of the `.rxn` (the
/// executor reads the file; anything beyond 16 MiB is truncated here, as in the
/// Python loop over `remaining`).
pub fn short_hash(data: &[u8]) -> String {
    let slice = if data.len() > HASH_INPUT_LIMIT {
        &data[..HASH_INPUT_LIMIT]
    } else {
        data
    };
    let digest = Sha256::digest(slice);
    let mut hex = String::with_capacity(HASH_LEN);
    for b in digest.iter() {
        if hex.len() >= HASH_LEN {
            break;
        }
        hex.push_str(&format!("{b:02x}"));
    }
    hex.truncate(HASH_LEN);
    hex
}

// ============================================================================
// Manifest paths + checking their presence in the archive
// ============================================================================

/// `_collect_manifest_paths`: collect all paths inside the zip that the
/// manifest promises, in Python order — the values of `videos`, the values of
/// `audio_tracks`, then `fallback_video`/`fallback_audio`/`preview` (only if
/// truthy). `videos`/`audio_tracks` are taken only if they are a dictionary
/// (`(meta.get(k) or {}).values()`); the values are assumed to be strings
/// (guaranteed by validation before the call).
pub fn collect_manifest_paths(meta: &Map<String, Value>) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for key in ["videos", "audio_tracks"] {
        if let Some(Value::Object(m)) = meta.get(key) {
            for (_k, v) in m {
                paths.push(v.as_str().unwrap_or_default().to_string());
            }
        }
    }
    for key in ["fallback_video", "fallback_audio", "preview"] {
        if let Some(v) = meta.get(key) {
            if is_truthy(v) {
                paths.push(v.as_str().unwrap_or_default().to_string());
            }
        }
    }
    paths
}

/// The pure part of `_verify_paths_inside_zip`: `names` is the set of names
/// inside the archive (`ZipFile.namelist()`, read by the executor). For every
/// truthy (non-empty) declared path it checks presence; if missing — an `Err`
/// with the text byte for byte (`{p!r}` repr). BadZipFile on opening the archive
/// is on the executor's side (the text `Повреждённый ZIP: {e}`, "Corrupted
/// ZIP").
pub fn verify_paths_present(
    names: &HashSet<String>,
    paths: &[String],
) -> Result<(), String> {
    for p in paths {
        if !p.is_empty() && !names.contains(p) {
            return Err(format!(
                "Объявленный в manifest файл отсутствует в архиве: {}",
                py_repr_str(p)
            ));
        }
    }
    Ok(())
}

// ============================================================================
// Install plan
// ============================================================================

/// A step of the install plan — a side-effecting call that `install_rxn_v4`
/// would make after all checks. Executed when connected to live Python.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op")]
pub enum InstallStep {
    /// `Path.mkdir(parents=True, exist_ok=<exist_ok>)`.
    /// `INSTALL_ROOT` — `exist_ok=true`; the target directory — `exist_ok=false`.
    MakeDirs { path: String, exist_ok: bool },
    /// `_safe_zip_extract(ZipFile(archive), dest)`. On an exception the executor
    /// must perform the `_safe_rmtree(dest)` rollback and return the
    /// [`extract_error_message`] error.
    ExtractZip { archive: String, dest: String },
}

/// Result of `install_rxn_v4` (before the real write).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result")]
pub enum InstallOutcome {
    /// `(ok=False, message, install_dir=None)`.
    Error { message: String },
    /// `(ok=True, "Уже установлено: {install_dir}", Some(install_dir))` ("Already
    /// installed") — the directory already existed, there are no steps.
    AlreadyInstalled { install_dir: String },
    /// Execute `steps`; on success — `(ok=True, "Установлено: {install_dir}",
    /// Some(install_dir))` ("Installed").
    Plan {
        install_dir: String,
        steps: Vec<InstallStep>,
    },
}

/// Message of a successful installation (the `install_rxn_v4` happy path).
pub fn installed_message(install_dir: &str) -> String {
    format!("Установлено: {install_dir}")
}

/// The "already installed" message.
pub fn already_installed_message(install_dir: &str) -> String {
    format!("Уже установлено: {install_dir}")
}

/// Message of a failed unpacking (the executor substitutes the exception text).
pub fn extract_error_message(detail: &str) -> String {
    format!("Ошибка распаковки: {detail}")
}

/// Port of `install_rxn_v4` from step 2 (schema validation) to computing the
/// unpacking plan. The early side-effecting steps are on the executor:
///   * `rxn_path.is_file()` → `Файл не найден: {rxn_path}` ("File not found");
///   * `parse_v4_manifest` → `Не удалось прочитать manifest: {e}` ("Could not
///     read manifest"; the exception types are the contract of the Python
///     bridge).
///
/// Injections:
///   * `manifest` — the already parsed `manifest.json`;
///   * `names` — `ZipFile(rxn_path).namelist()`;
///   * `short_hash` — [`short_hash`] of the first 16 MiB of the file;
///   * `install_root` — the absolute `INSTALL_ROOT`
///     (`~/.local/share/rexon/wallpapers`);
///   * `dir_exists` — whether the target directory already exists;
///   * `archive_path` — path to the `.rxn` (for the unpacking step).
pub fn plan_install_v4(
    manifest: &Value,
    names: &HashSet<String>,
    short_hash: &str,
    install_root: &str,
    dir_exists: bool,
    archive_path: &str,
) -> InstallOutcome {
    // 2. Schema validation.
    let (ok, err) = validate_v4_manifest(manifest);
    if !ok {
        return InstallOutcome::Error {
            message: format!("Неверный manifest: {err}"),
        };
    }
    // A valid v4 manifest is always an object.
    let meta = match manifest.as_object() {
        Some(m) => m,
        None => {
            return InstallOutcome::Error {
                message: format!("Неверный manifest: {err}"),
            }
        }
    };

    // 3. Checking that the declared paths are present in the archive.
    let paths = collect_manifest_paths(meta);
    if let Err(message) = verify_paths_present(names, &paths) {
        return InstallOutcome::Error { message };
    }

    // 4. Target directory: slug + short hash.
    let title = meta.get("title").and_then(Value::as_str).unwrap_or_default();
    let slug = slugify(title);
    let install_dir = join(install_root, &format!("{slug}-{short_hash}"));

    if dir_exists {
        return InstallOutcome::AlreadyInstalled { install_dir };
    }

    // 5. Unpacking plan: mkdir INSTALL_ROOT (exist_ok) → mkdir the target
    //    (without exist_ok) → safe unpacking.
    let steps = vec![
        InstallStep::MakeDirs {
            path: install_root.to_string(),
            exist_ok: true,
        },
        InstallStep::MakeDirs {
            path: install_dir.clone(),
            exist_ok: false,
        },
        InstallStep::ExtractZip {
            archive: archive_path.to_string(),
            dest: install_dir.clone(),
        },
    ];
    InstallOutcome::Plan { install_dir, steps }
}

// ============================================================================
// Listing: enriching the manifest with absolute paths
// ============================================================================

/// The pure part of `list_rxn_v4_wallpapers` (enriching one entry) — port of
/// building `meta_out`. `sub_dir` is the already resolved absolute install
/// directory (`str(sub.resolve())`); the preview/fallback existence flags are
/// computed by the executor (`Path.is_file()`).
///
/// Returns a copy of the manifest + service keys IN THE SAME ORDER as Python:
/// `_install_dir`, `_manifest_path`, `_preview_path`, `_videos_abs`,
/// `_audio_abs`, `_fallback_video_abs`, `_fallback_audio_abs`, `_slug`.
/// `_videos_abs`/`_audio_abs` are always built (for EVERY track), while
/// `_preview_path`/`_fallback_*_abs` only if the file is truthy AND exists
/// (otherwise `null`).
pub fn build_listed_entry(
    sub_dir: &str,
    manifest: &Map<String, Value>,
    preview_exists: bool,
    fallback_video_exists: bool,
    fallback_audio_exists: bool,
) -> Value {
    // Absolute paths of the tracks (always, regardless of whether the file exists).
    let abs_map = |key: &str| -> Value {
        let mut m = Map::new();
        if let Some(Value::Object(tracks)) = manifest.get(key) {
            for (k, rel) in tracks {
                let rel = rel.as_str().unwrap_or_default();
                m.insert(k.clone(), Value::String(resolve_join(sub_dir, rel)));
            }
        }
        Value::Object(m)
    };

    // `_preview_path`/`_fallback_*_abs`: a path only if the file exists.
    let gated = |key: &str, exists: bool| -> Value {
        if exists {
            let rel = manifest.get(key).and_then(Value::as_str).unwrap_or_default();
            Value::String(resolve_join(sub_dir, rel))
        } else {
            Value::Null
        }
    };

    let mut out: Map<String, Value> = manifest.clone();
    out.insert("_install_dir".into(), Value::String(sub_dir.to_string()));
    out.insert(
        "_manifest_path".into(),
        Value::String(resolve_join(sub_dir, "manifest.json")),
    );
    out.insert("_preview_path".into(), gated("preview", preview_exists));
    out.insert("_videos_abs".into(), abs_map("videos"));
    out.insert("_audio_abs".into(), abs_map("audio_tracks"));
    out.insert(
        "_fallback_video_abs".into(),
        gated("fallback_video", fallback_video_exists),
    );
    out.insert(
        "_fallback_audio_abs".into(),
        gated("fallback_audio", fallback_audio_exists),
    );
    out.insert(
        "_slug".into(),
        Value::String(basename(sub_dir).to_string()),
    );
    Value::Object(out)
}

// ============================================================================
// Removal: slug validation
// ============================================================================

/// The pure part of `remove_rxn_v4_wallpaper`: checking the slug before touching
/// the file system. `Some(msg)` — an invalid slug (empty / contains `/` or `\` /
/// starts with a dot), the text byte for byte (`{slug!r}`); `None` — the slug is
/// acceptable (then the executor checks `INSTALL_ROOT/slug` and removes it via
/// `_safe_rmtree`).
pub fn remove_slug_error(slug: &str) -> Option<String> {
    if slug.is_empty()
        || slug.contains('/')
        || slug.contains('\\')
        || slug.starts_with('.')
    {
        Some(format!("Некорректный slug: {}", py_repr_str(slug)))
    } else {
        None
    }
}

/// The "not installed" message on removal (there is no directory).
pub fn not_installed_message(slug: &str) -> String {
    format!("Не установлено: {slug}")
}

/// Message of a successful removal.
pub fn removed_message(target: &str) -> String {
    format!("Удалено: {target}")
}

/// Message of a failed removal (`_safe_rmtree` returned false).
pub fn remove_failed_message(target: &str) -> String {
    format!("Не удалось удалить {target}")
}

/// Install root of adaptive wallpapers — `~/.local/share/rexon/wallpapers`.
///
/// Warning — rule #16: the original built it as `Path.home()/".local/share"/…`,
/// bypassing the project's shared expander — with a non-standard
/// `$XDG_DATA_HOME` the installer wrote wallpapers NOT where the rest of Rexon
/// keeps its data (the same class as the fixed `PlasmaProtector._expand`). The
/// port moves the path to the XDG data root; in a standard environment the
/// result matches the original byte for byte.
pub fn install_root(home: &str, xdg_data_home: Option<&str>) -> String {
    crate::utils::expand_user_xdg(
        "~/.local/share/rexon/wallpapers",
        home,
        None,
        xdg_data_home,
    )
}

/// `install_rxn_v4`: the given `.rxn` is not on disk.
pub const MSG_FILE_NOT_FOUND_PREFIX: &str = "Файл не найден: ";
/// `install_rxn_v4`: manifest.json cannot be read (followed by the exception text).
pub const MSG_MANIFEST_READ_FAILED_PREFIX: &str =
    "Не удалось прочитать manifest: ";
