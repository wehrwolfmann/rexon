//! Importers of themes from other platforms — port of `rexon/extensions/importers.py`
//! (pure logic, no I/O).
//!
//! The original has 5 classes / 7 import paths, all of the Windows family:
//!   * ThemepackImporter        — .themepack / .deskthemepack (CAB);
//!   * WindowsThemeImporter     — dispatcher + .theme (INI), DreamScene
//!     (.wmv/.mpeg/.mpg/.dream), .gadget (ZIP+XML);
//!   * WindowsCursorImporter    — .cur/.ani/.inf/.crs/.CurXPTheme/folder/archive → Xcursor
//!     (since 03.10.2026 — our own converter `crate::wincursor`, all animation frames);
//!   * WindowsIconImporter      — .ico/.icl/.dll/.exe/.iconpackage/.ip/.7tsp;
//!   * MSStyImporter            — .msstyles (PE) → GTK+Aurorae+Plasma+colors.
//!
//! There are NO Symbian/PSP importers in importers.py (the old comment in the
//! project structure description is out of date); Wehrwolfmann's decision of
//! 2026-07-17 not to port them is fulfilled automatically.
//!
//! SCHEME: the importers are interactive — the parsing flow depends on the
//! results of subprocesses (convert/ffmpeg/wrestool/...) and on files that
//! appear after them. So instead of a static plan (as in `ext_manager`) there
//! is the [`ImpIo`] trait: EVERY side call and EVERY observation of the Python
//! original (os/shutil/subprocess/tempfile/zipfile/open/_write_manifest/_pack_rxn/
//! _safe_zip_extract/ET.parse) is one trait method, in the same order. The
//! parity reference is a call log of live Python in a sandbox
//! (`tests/golden/importers_cases.json`, captured by a "virtual FS + log"
//! harness); Rust must make the same calls in the same order and arrive at the
//! same result/error (texts byte for byte).
//!
//! Parsed in Rust: a mini-configparser (CPython 3.14 semantics: SECTCRE with
//! greedy `.+`, ParsingError with accumulation, BasicInterpolation with exact
//! error texts), the CLSID regex, gadget.xml (traversal in document order,
//! like `root.iter()`), ICON type/PART_MAP mappings (cursor roles — `wincursor`),
//! generation of manifest.json / index.theme / gtk.css / aurorae rc /
//! metadata.desktop / SVG wrappers (base64) — texts 1-to-1.
//!
//! PITFALLS OF THE ORIGINAL FIXED (sanctioned divergences from Python;
//! Wehrwolfmann's decision of 2026-07-17: fix only in Rust, don't touch Python):
//!   * `WallpaperUpscaler` is NOT imported in importers.py → in Python every
//!     .themepack and .theme import fails with NameError right after copying
//!     the wallpapers; here the upscale is modelled by the
//!     [`ImpIo::upscale_wallpapers`] method and the tails of both imports are
//!     ported in full;
//!   * `os.path.join(..., exist_ok=True)` in WindowsIconImporter → in Python
//!     every icon import fails with TypeError; here `exist_ok` has makedirs
//!     semantics, the tail of the icon import is ported (the same typo in the
//!     icon branch of ThemepackImporter is fixed the same way);
//!   * `"group_icon" in r.stdout.lower()` — str-in-bytes → TypeError in Python
//!     in can_import for .dll/.exe with non-empty wrestool stdout; here stdout
//!     is decoded as text;
//!   * `f"#{bytes}"` in `_extract_dominant_colors` produced strings like
//!     `#b'336699'` → ValueError in `_generate_kde_colorscheme`; here convert's
//!     stdout is decoded and the color scheme is generated;
//!   * substring lookup in PART_MAP took the FIRST key in dict order:
//!     `startmenu` was caught by the key `menu`, `startpanel` — by `pane`,
//!     `taskitemactive` — by `taskitem`; here the longest (most specific)
//!     matching key is chosen;
//!   * the 7z fallback for .msstyles searched for bitmaps NON-recursively
//!     (os.listdir), although 7z puts PE resources into subfolders; here the
//!     search after 7z is recursive.
//!
//! THE EXECUTOR is not here: real file operations and running converters are
//! done by the caller. The live Rust executor lives in [`crate::imp_live`]
//! (used both by `ext_exec::install_from_file` and by the PyO3 bridge); the
//! wallpaper upscale is plugged into it as a closure (`upscale_wallpapers`),
//! because the upscaler has its own executor. Logs are on the caller's side too.
//! Known assumptions: a binary missing from PATH (FileNotFoundError from
//! subprocess.run) is not modelled — the executor has `which` available up
//! front; non-UTF-8 XML and exotic non-letter unprintable characters in repr
//! are outside the fixtures (see `py_repr`).

use quick_xml::events::Event;
use quick_xml::Reader;
use crate::i18n::{n_, ntr, tr, trc, trf};
use crate::wincursor::{self, Role};
use serde_json::{json, Map, Value};

// ============================================================================
// Errors and the executor interface
// ============================================================================

/// Import error: Python exception name + text (byte for byte).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpError {
    pub kind: String,
    pub message: String,
}

impl ImpError {
    fn new(kind: &str, message: impl Into<String>) -> Self {
        ImpError { kind: kind.to_string(), message: message.into() }
    }
}

fn verr(message: impl Into<String>) -> ImpError {
    ImpError::new("ValueError", message)
}

fn rerr(message: impl Into<String>) -> ImpError {
    ImpError::new("RuntimeError", message)
}

/// `open()` could not open the file (CPython text for FileNotFoundError).
fn oserr(path: &str) -> ImpError {
    ImpError::new(
        "FileNotFoundError",
        format!("[Errno 2] No such file or directory: '{path}'"),
    )
}

/// Result of `subprocess.run`: `ok` = returncode == 0.
pub struct RunResult {
    pub ok: bool,
    pub stdout: Vec<u8>,
}

/// An `os.walk` tuple: (root, subdirectories, files) in listing order.
pub type WalkEntry = (String, Vec<String>, Vec<String>);

/// Executor interface: each method is exactly one side call or observation of
/// the Python original, in execution order. In parity tests it is implemented
/// by replaying a log; in the live bridge — by real I/O.
pub trait ImpIo {
    /// `tempfile.mkdtemp(prefix=...)` → path.
    fn mkdtemp(&mut self, prefix: &str) -> String;
    /// `os.makedirs(path, exist_ok=True)`.
    fn makedirs(&mut self, path: &str);
    /// `subprocess.run(argv, capture_output=True, timeout=...)`.
    fn run(&mut self, argv: Vec<String>, timeout: u64) -> RunResult;
    /// `os.walk(root)` (topdown, in full).
    fn walk(&mut self, root: &str) -> Vec<WalkEntry>;
    /// `os.listdir(path)`.
    fn listdir(&mut self, path: &str) -> Vec<String>;
    /// `os.path.isfile`.
    fn is_file(&mut self, path: &str) -> bool;
    /// `os.path.isdir`.
    fn is_dir(&mut self, path: &str) -> bool;
    /// `os.path.exists`.
    fn exists(&mut self, path: &str) -> bool;
    /// `os.path.getsize`.
    fn getsize(&mut self, path: &str) -> u64;
    /// `open(path, encoding="utf-8", errors="replace").read()`;
    /// None — the file didn't open (OSError).
    fn read_text(&mut self, path: &str) -> Option<String>;
    /// the same, but `.read(n)` (n characters).
    fn read_head_text(&mut self, path: &str, n: usize) -> Option<String>;
    /// `open(path, "rb").read(n)`.
    fn read_head_bytes(&mut self, path: &str, n: usize) -> Option<Vec<u8>>;
    /// `open(path, "rb").read()`.
    fn read_bytes(&mut self, path: &str) -> Option<Vec<u8>>;
    /// `zipfile.is_zipfile(path)`; None — OSError on open.
    fn is_zipfile(&mut self, path: &str) -> Option<bool>;
    /// `zipfile.ZipFile(path)`: false — BadZipFile.
    fn zip_open(&mut self, path: &str) -> bool;
    /// `shutil.which(name) is not None`.
    fn which(&mut self, name: &str) -> bool;
    /// `shutil.copy2(src, dest)`.
    fn copy_file(&mut self, src: &str, dest: &str);
    /// `shutil.copytree(src, dest)`.
    fn copy_tree(&mut self, src: &str, dest: &str);
    /// `os.symlink(target, link)`; false — an exception (suppressed by the original).
    fn symlink(&mut self, target: &str, link: &str) -> bool;
    /// `os.remove(path)`.
    fn remove_file(&mut self, path: &str);
    /// `shutil.rmtree(path, ignore_errors=True)`.
    fn remove_tree(&mut self, path: &str);
    /// Write text to a file (`open(...,"w").write` or `_write_manifest`).
    fn write_file(&mut self, path: &str, content: &str);
    /// `_safe_zip_extract(zf, dest)`.
    fn extract_zip(&mut self, archive: &str, dest: &str);
    /// `_pack_rxn(src, dest)`.
    fn pack_rxn(&mut self, src: &str, dest: &str);
    /// Write bytes to a file (converted cursors, cover image). No Python
    /// counterpart: cursor conversion since 03.10.2026 is our own Rust code.
    fn write_bytes(&mut self, path: &str, data: &[u8]);
    /// ZIP entry names; None — the file didn't open or isn't a ZIP.
    fn zip_names(&mut self, path: &str) -> Option<Vec<String>>;
    /// Whether this is a symbolic link (without following it): links from a
    /// foreign archive are not read and are not put into `.rxn`.
    fn is_symlink(&mut self, path: &str) -> bool;
    /// `WallpaperUpscaler.process_wallpapers_in_dir(data_dir)` → number of
    /// wallpapers processed. A deliberate divergence from the Python original:
    /// there it is a NameError (WallpaperUpscaler not imported), fixed here — the
    /// executor performs the upscale (the live bridge calls the real
    /// upscaler.py; the upscaler is a separate module and is not ported here).
    fn upscale_wallpapers(&mut self, data_dir: &str) -> u64;
}

// ============================================================================
// Python semantics helpers (os.path, str, int, repr, base64)
// ============================================================================

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

fn basename(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

fn dirname(p: &str) -> String {
    match p.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => p[..i].to_string(),
        None => String::new(),
    }
}

/// `os.path.splitext`: a dot at the start of the basename (and all leading
/// dots) is not considered an extension.
fn splitext(p: &str) -> (&str, &str) {
    let base_start = p.rfind('/').map_or(0, |i| i + 1);
    let base = &p[base_start..];
    if let Some(dot_rel) = base.rfind('.') {
        // is there a non-dot before the last dot?
        if base[..dot_rel].chars().any(|c| c != '.') {
            let dot = base_start + dot_rel;
            return (&p[..dot], &p[dot..]);
        }
    }
    (p, "")
}

fn ext_lower(p: &str) -> String {
    splitext(p).1.to_lowercase()
}

fn stem(p: &str) -> &str {
    splitext(basename(p)).0
}

/// A whitespace character in the Python `str` sense (wider than Rust
/// `is_whitespace` by U+001C..U+001F).
fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

fn py_strip(s: &str) -> &str {
    s.trim_matches(py_space)
}

fn py_rstrip(s: &str) -> &str {
    s.trim_end_matches(py_space)
}

/// `repr()` of a string: quotes and escaping as in CPython (printable
/// non-ASCII characters stay as is; exotic unprintable ones outside C0/DEL
/// are not escaped — they don't occur in the fixtures).
fn py_repr(s: &str) -> String {
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

/// `int(s)` (decimal): surrounding whitespace, sign, ASCII digits with single
/// underscores between them. None — ValueError (callers suppress it).
fn py_int(s: &str) -> Option<i64> {
    let t = py_strip(s);
    let (neg, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if t.is_empty() || t.starts_with('_') || t.ends_with('_') || t.contains("__") {
        return None;
    }
    if !t.chars().all(|c| c.is_ascii_digit() || c == '_') {
        return None;
    }
    let digits: String = t.chars().filter(|c| *c != '_').collect();
    digits.parse::<i64>().ok().map(|v| if neg { -v } else { v })
}

/// `int(x)` of bytes (identify outputs bytes; int() accepts them as ASCII).
fn py_int_bytes(b: &[u8]) -> Option<i64> {
    std::str::from_utf8(b).ok().and_then(py_int)
}

/// `int(s, 16)`: Err — the ValueError text byte for byte.
fn py_int_base16(s: &str) -> Result<i64, ImpError> {
    let bad = || {
        verr(format!(
            "invalid literal for int() with base 16: {}",
            py_repr(s)
        ))
    };
    let t = py_strip(s);
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let t = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    if t.is_empty() || t.starts_with('_') || t.ends_with('_') || t.contains("__") {
        return Err(bad());
    }
    if !t.chars().all(|c| c.is_ascii_hexdigit() || c == '_') {
        return Err(bad());
    }
    let digits: String = t.chars().filter(|c| *c != '_').collect();
    i64::from_str_radix(&digits, 16)
        .map(|v| if neg { -v } else { v })
        .map_err(|_| bad())
}

/// `re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-")[:60]` — the shared safe_id.
///
/// Difference from Python: Cyrillic (ru/uk/be) is transliterated first — in
/// the original "Аэро" gave an empty id, and the import wrote `msstyles-.rxn`
/// and put the theme straight into `data/gtk/` without its own folder. If
/// there is still no Latin after that (CJK, Arabic script), the id is
/// `theme-` + 8 hex of the name's sha256: stable for the same name and
/// different for different ones.
/// Directory name of an imported icon or cursor theme: a set named
/// `hicolor`/`locolor` would take the place of the shared set of all programs
/// and its `index.theme` would override the system one (defect 09.11.2026,
/// [`crate::icon_hygiene`]) — such a name gets `-imported` appended.
fn icon_set_dir(slug: String) -> String {
    if crate::icon_hygiene::is_reserved_icon_set(&slug) {
        format!("{slug}-imported")
    } else {
        slug
    }
}

pub(crate) fn slug60(s: &str) -> String {
    let lowered = s.to_lowercase();
    let mut out = String::new();
    let mut dash = false;
    for c in lowered.chars() {
        let piece = cyrillic_latin(c);
        if let Some(p) = piece {
            if !p.is_empty() {
                out.push_str(p);
                dash = false;
            }
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let stripped: String = out.trim_matches('-').chars().take(60).collect();
    let stripped = stripped.trim_end_matches('-');
    if !stripped.is_empty() || s.trim().is_empty() {
        return stripped.to_string();
    }
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(s.trim().as_bytes());
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("theme-{hex}")
}

/// Latin for a lowercase Cyrillic letter (ru/uk/be); `None` — not Cyrillic.
/// `ъ`/`ь` give an empty string: they aren't needed in an id.
fn cyrillic_latin(c: char) -> Option<&'static str> {
    Some(match c {
        'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'ґ' => "g", 'д' => "d",
        'е' => "e", 'ё' => "yo", 'є' => "ye", 'ж' => "zh", 'з' => "z", 'и' => "i",
        'і' => "i", 'ї' => "yi", 'й' => "y", 'к' => "k", 'л' => "l", 'м' => "m",
        'н' => "n", 'о' => "o", 'п' => "p", 'р' => "r", 'с' => "s", 'т' => "t",
        'у' => "u", 'ў' => "u", 'ф' => "f", 'х' => "kh", 'ц' => "ts", 'ч' => "ch",
        'ш' => "sh", 'щ' => "shch", 'ъ' | 'ь' => "", 'ы' => "y", 'э' => "e",
        'ю' => "yu", 'я' => "ya",
        _ => return None,
    })
}

/// `bytes.strip()` — ASCII whitespace at both ends.
fn bytes_strip(b: &[u8]) -> &[u8] {
    let is_ws = |c: &u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    let start = b.iter().position(|c| !is_ws(c)).unwrap_or(b.len());
    let end = b.iter().rposition(|c| !is_ws(c)).map_or(start, |i| i + 1);
    &b[start..end]
}

/// `bytes.split()` — on runs of whitespace.
fn bytes_split_ws(b: &[u8]) -> Vec<&[u8]> {
    b.split(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
        .filter(|p| !p.is_empty())
        .collect()
}

/// `base64.b64encode` (standard alphabet, no line breaks).
fn b64encode(data: &[u8]) -> String {
    const AL: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(AL[(n >> 18) as usize & 63] as char);
        out.push(AL[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { AL[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { AL[n as usize & 63] as char } else { '=' });
    }
    out
}

/// `json.dumps(..., indent=2, ensure_ascii=False)` (as in `_write_manifest`).
pub(crate) fn dumps_indent2(v: &Value) -> String {
    serde_json::to_string_pretty(v).expect("Value сериализуется")
}

/// An ordered path dictionary (dict semantics: overwriting keeps the position).
#[derive(Default)]
struct OrdMap(Vec<(String, String)>);

impl OrdMap {
    fn insert(&mut self, k: String, v: String) {
        if let Some(e) = self.0.iter_mut().find(|(ek, _)| *ek == k) {
            e.1 = v;
        } else {
            self.0.push((k, v));
        }
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn iter(&self) -> impl Iterator<Item = &(String, String)> {
        self.0.iter()
    }

    fn first_value(&self) -> Option<&str> {
        self.0.first().map(|(_, v)| v.as_str())
    }
}

// ============================================================================
// Mini-configparser — CPython 3.14 semantics (ConfigParser(strict=False))
// ============================================================================

/// Parsed INI: the DEFAULT section + ordinary sections in order of appearance.
/// Values are already joined (multiline join + rstrip), WITHOUT interpolation.
#[derive(Default)]
struct Ini {
    defaults: Vec<(String, String)>,
    sections: Vec<(String, Vec<(String, String)>)>,
}

enum CurSect {
    Default,
    Idx(usize),
}

impl Ini {
    /// `config.read_string(text)`. Returns the parsed state AND a possible error:
    /// as in CPython, on ParsingError/MissingSectionHeaderError the data parsed
    /// so far stays in the parser (callers that suppress errors use the partial
    /// result).
    fn read_string(text: &str) -> (Ini, Option<ImpError>) {
        // values during the parsing phase are a list of lines (multiline)
        type RawOpts = Vec<(String, Vec<String>)>;
        let mut defaults: RawOpts = Vec::new();
        let mut sections: Vec<(String, RawOpts)> = Vec::new();
        let mut cursect: Option<CurSect> = None;
        let mut optname: Option<String> = None;
        let mut indent_level: usize = 0;
        let mut errors: Vec<(usize, String)> = Vec::new();
        let mut fatal: Option<ImpError> = None;

        for (lineno0, raw) in text.split_inclusive('\n').enumerate() {
            let lineno = lineno0 + 1;
            let trimmed = py_strip(raw);
            let is_comment = trimmed.starts_with('#') || trimmed.starts_with(';');
            let clean = if is_comment { "" } else { trimmed };
            let has_comments = trimmed != clean;

            if clean.is_empty() {
                // an empty line inside a value → '' (empty_lines_in_values)
                if !has_comments {
                    if let (Some(cur), Some(opt)) = (&cursect, &optname) {
                        if !opt.is_empty() {
                            let store = match cur {
                                CurSect::Default => &mut defaults,
                                CurSect::Idx(i) => &mut sections[*i].1,
                            };
                            if let Some(e) =
                                store.iter_mut().find(|(k, _)| k == opt)
                            {
                                e.1.push(String::new());
                            }
                        }
                    }
                }
                continue;
            }

            let cur_indent_level =
                raw.chars().position(|c| !py_space(c)).unwrap_or(0);

            // a value continuation line?
            let is_continue = cursect.is_some()
                && optname.as_deref().is_some_and(|o| !o.is_empty())
                && cur_indent_level > indent_level;
            if is_continue {
                let opt = optname.as_ref().expect("optname проверен");
                let store = match cursect.as_ref().expect("cursect проверен") {
                    CurSect::Default => &mut defaults,
                    CurSect::Idx(i) => &mut sections[*i].1,
                };
                if let Some(e) = store.iter_mut().find(|(k, _)| k == opt) {
                    e.1.push(clean.to_string());
                }
                continue;
            }

            indent_level = cur_indent_level;

            // section header: `\[(.+)\]` — greedy up to the LAST `]`
            let header = if clean.starts_with('[') {
                match clean.rfind(']') {
                    Some(j) if j >= 2 => Some(&clean[1..j]),
                    _ => None,
                }
            } else {
                None
            };

            if let Some(name) = header {
                if let Some(i) = sections.iter().position(|(n, _)| n == name) {
                    cursect = Some(CurSect::Idx(i)); // merge (strict=False)
                } else if name == "DEFAULT" {
                    cursect = Some(CurSect::Default);
                } else {
                    sections.push((name.to_string(), Vec::new()));
                    cursect = Some(CurSect::Idx(sections.len() - 1));
                }
                optname = None;
                continue;
            }

            if cursect.is_none() {
                fatal = Some(ImpError::new(
                    "MissingSectionHeaderError",
                    format!(
                        "File contains no section headers.\nfile: '<string>', line: {lineno}\n{}",
                        py_repr(raw)
                    ),
                ));
                break; // CPython aborts parsing immediately
            }

            // option: the delimiter is the first occurrence of '=' or ':'
            let delim = clean.char_indices().find(|(_, c)| *c == '=' || *c == ':');
            let Some((di, _)) = delim else {
                errors.push((lineno, raw.to_string()));
                continue;
            };
            let name = py_rstrip(&clean[..di]).to_lowercase();
            if name.is_empty() {
                errors.push((lineno, raw.to_string()));
            }
            let after = &clean[di + 1..];
            let optval = py_strip(after).to_string();
            let store = match cursect.as_ref().expect("cursect есть") {
                CurSect::Default => &mut defaults,
                CurSect::Idx(i) => &mut sections[*i].1,
            };
            if let Some(e) = store.iter_mut().find(|(k, _)| k == &name) {
                e.1 = vec![optval];
            } else {
                store.push((name.clone(), vec![optval]));
            }
            optname = Some(name);
        }

        // _join_multiline_values (runs on error too — finally)
        let join_vals = |v: Vec<(String, Vec<String>)>| -> Vec<(String, String)> {
            v.into_iter()
                .map(|(k, lines)| (k, py_rstrip(&lines.join("\n")).to_string()))
                .collect()
        };
        let ini = Ini {
            defaults: join_vals(defaults),
            sections: sections
                .into_iter()
                .map(|(n, opts)| (n, join_vals(opts)))
                .collect(),
        };

        let err = fatal.or_else(|| {
            if errors.is_empty() {
                return None;
            }
            let mut msg = "Source contains parsing errors: '<string>'".to_string();
            for (lineno, line) in &errors {
                msg.push_str(&format!("\n\t[line {lineno:2}]: {}", py_repr(line)));
            }
            Some(ImpError::new("ParsingError", msg))
        });
        (ini, err)
    }

    fn has_section(&self, name: &str) -> bool {
        self.sections.iter().any(|(n, _)| n == name)
    }

    fn section_names(&self) -> Vec<String> {
        self.sections.iter().map(|(n, _)| n.clone()).collect()
    }

    /// `config.options(section)`: DEFAULT keys, then its own (dict order).
    fn options(&self, section: &str) -> Vec<String> {
        let mut out: Vec<String> =
            self.defaults.iter().map(|(k, _)| k.clone()).collect();
        if let Some((_, opts)) = self.sections.iter().find(|(n, _)| n == section) {
            for (k, _) in opts {
                if !out.contains(k) {
                    out.push(k.clone());
                }
            }
        }
        out
    }

    fn raw_lookup(&self, section: &str, option_lower: &str) -> Option<&str> {
        if section != "DEFAULT" {
            if let Some((_, opts)) =
                self.sections.iter().find(|(n, _)| n == section)
            {
                if let Some((_, v)) =
                    opts.iter().find(|(k, _)| k == option_lower)
                {
                    return Some(v);
                }
            } else {
                return None; // NoSectionError → fallback at the caller
            }
        }
        self.defaults
            .iter()
            .find(|(k, _)| k == option_lower)
            .map(|(_, v)| v.as_str())
    }

    /// `config.get(section, option, fallback=...)` with BasicInterpolation.
    /// NoSection/NoOption → fallback; interpolation errors propagate.
    fn get_fb(
        &self,
        section: &str,
        option: &str,
        fallback: &str,
    ) -> Result<String, ImpError> {
        let opt_lower = option.to_lowercase();
        if section != "DEFAULT" && !self.has_section(section) {
            return Ok(fallback.to_string());
        }
        let Some(raw) = self.raw_lookup(section, &opt_lower) else {
            return Ok(fallback.to_string());
        };
        let raw = raw.to_string();
        let mut accum = String::new();
        self.interpolate_some(section, &opt_lower, &mut accum, &raw, &raw, 1)?;
        Ok(accum)
    }

    /// Port of `BasicInterpolation._interpolate_some` (error texts 1-to-1).
    fn interpolate_some(
        &self,
        section: &str,
        option: &str,
        accum: &mut String,
        mut rest: &str,
        rawval: &str,
        depth: usize,
    ) -> Result<(), ImpError> {
        const MAX_INTERPOLATION_DEPTH: usize = 10;
        if depth > MAX_INTERPOLATION_DEPTH {
            return Err(ImpError::new(
                "InterpolationDepthError",
                format!(
                    "Recursion limit exceeded in value substitution: option {} in section {} contains an interpolation key which cannot be substituted in {MAX_INTERPOLATION_DEPTH} steps. Raw value: {}",
                    py_repr(option),
                    py_repr(section),
                    py_repr(rawval)
                ),
            ));
        }
        while !rest.is_empty() {
            let Some(p) = rest.find('%') else {
                accum.push_str(rest);
                return Ok(());
            };
            if p > 0 {
                accum.push_str(&rest[..p]);
                rest = &rest[p..];
            }
            match rest[1..].chars().next() {
                Some('%') => {
                    accum.push('%');
                    rest = &rest[2..];
                }
                Some('(') => {
                    // _KEYCRE = %\(([^)]+)\)s
                    let key_end = rest[2..].find(')').map(|i| i + 2);
                    let matched = key_end.and_then(|j| {
                        if j > 2 && rest[j + 1..].starts_with('s') {
                            Some((&rest[2..j], j + 2))
                        } else {
                            None
                        }
                    });
                    let Some((var_raw, m_end)) = matched else {
                        return Err(ImpError::new(
                            "InterpolationSyntaxError",
                            format!(
                                "bad interpolation variable reference {}",
                                py_repr(rest)
                            ),
                        ));
                    };
                    let var = var_raw.to_lowercase();
                    rest = &rest[m_end..];
                    let v = self.raw_lookup(section, &var).map(str::to_string);
                    let Some(v) = v else {
                        return Err(ImpError::new(
                            "InterpolationMissingOptionError",
                            format!(
                                "Bad value substitution: option {} in section {} contains an interpolation key {} which is not a valid option name. Raw value: {}",
                                py_repr(option),
                                py_repr(section),
                                py_repr(&var),
                                py_repr(rawval)
                            ),
                        ));
                    };
                    if v.contains('%') {
                        self.interpolate_some(
                            section, option, accum, &v, rawval, depth + 1,
                        )?;
                    } else {
                        accum.push_str(&v);
                    }
                }
                _ => {
                    return Err(ImpError::new(
                        "InterpolationSyntaxError",
                        format!(
                            "'%' must be followed by '%' or '(', found: {}",
                            py_repr(rest)
                        ),
                    ));
                }
            }
        }
        Ok(())
    }
}

// ============================================================================
// CLSID regex and gadget.xml
// ============================================================================

/// Match `\[CLSID\\?\{([^}]+)\}\\?DefaultIcon\]\s*DefaultValue\s*=\s*(.+)`
/// (IGNORECASE): (guid, value up to the end of line).
fn clsid_finditer(content: &str) -> Vec<(String, String)> {
    let lower = content.to_lowercase();
    // the pattern and content are ASCII-compatible where it matters; indices are
    // matched by bytes of the lowercased string (to_lowercase is stable for
    // ASCII, and no non-ASCII appears in the pattern literals)
    let lb = lower.as_bytes();
    let cb = content.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    'search: while i < lb.len() {
        let Some(rel) = find_sub(&lb[i..], b"[clsid") else { break };
        let start = i + rel;
        let mut p = start + b"[clsid".len();
        // \\? — optional backslash
        if lb.get(p) == Some(&b'\\') {
            p += 1;
        }
        if lb.get(p) != Some(&b'{') {
            i = start + 1;
            continue 'search;
        }
        p += 1;
        let guid_start = p;
        while p < lb.len() && lb[p] != b'}' {
            p += 1;
        }
        if p >= lb.len() || p == guid_start {
            i = start + 1;
            continue 'search;
        }
        let guid_end = p;
        p += 1; // '}'
        if lb.get(p) == Some(&b'\\') {
            p += 1;
        }
        if !lb[p..].starts_with(b"defaulticon]") {
            i = start + 1;
            continue 'search;
        }
        p += b"defaulticon]".len();
        while p < lb.len() && is_ascii_py_space(lb[p]) {
            p += 1;
        }
        if !lb[p..].starts_with(b"defaultvalue") {
            i = start + 1;
            continue 'search;
        }
        p += b"defaultvalue".len();
        while p < lb.len() && is_ascii_py_space(lb[p]) {
            p += 1;
        }
        if lb.get(p) != Some(&b'=') {
            i = start + 1;
            continue 'search;
        }
        p += 1;
        while p < lb.len() && is_ascii_py_space(lb[p]) {
            p += 1;
        }
        // (.+) — up to the end of line ('.' doesn't match \n), at least one character
        let val_start = p;
        while p < lb.len() && lb[p] != b'\n' {
            p += 1;
        }
        if p == val_start {
            i = start + 1;
            continue 'search;
        }
        let guid =
            String::from_utf8_lossy(&cb[guid_start..guid_end]).into_owned();
        let value = String::from_utf8_lossy(&cb[val_start..p]).into_owned();
        out.push((guid, value));
        i = p; // finditer — non-overlapping
    }
    out
}

fn is_ascii_py_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

/// A gadget.xml element in document order (like `root.iter()`):
/// (localname, value of the name attribute, text up to the first child).
struct XmlElem {
    tag: String,
    name_attr: Option<String>,
    text: Option<String>,
}

/// Parse gadget.xml with quick-xml with ElementTree semantics to the extent we
/// need. Err(()) — ParseError (suppressed by the caller). Unclosed tags at EOF
/// are an error (as with expat).
fn parse_gadget_xml(bytes: &[u8]) -> Result<Vec<XmlElem>, ()> {
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    let mut reader = Reader::from_str(text);
    let mut out: Vec<XmlElem> = Vec::new();
    // stack: indices of open elements in out + a "already has a child" flag
    let mut stack: Vec<(usize, bool)> = Vec::new();
    let push_elem = |out: &mut Vec<XmlElem>,
                         stack: &mut Vec<(usize, bool)>,
                         tag: String,
                         name_attr: Option<String>| {
        if let Some(top) = stack.last_mut() {
            top.1 = true; // the parent got a child → text is closed
        }
        out.push(XmlElem { tag, name_attr, text: None });
        out.len() - 1
    };
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag = local_name(e.name().as_ref());
                let attr = attr_name(&e)?;
                let idx = push_elem(&mut out, &mut stack, tag, attr);
                stack.push((idx, false));
            }
            Ok(Event::Empty(e)) => {
                let tag = local_name(e.name().as_ref());
                let attr = attr_name(&e)?;
                push_elem(&mut out, &mut stack, tag, attr);
            }
            Ok(Event::End(_)) => {
                if stack.pop().is_none() {
                    return Err(());
                }
            }
            Ok(Event::Text(e)) => {
                if let Some(&(idx, has_child)) = stack.last() {
                    if !has_child {
                        let t = e.decode().map_err(|_| ())?;
                        out[idx]
                            .text
                            .get_or_insert_with(String::new)
                            .push_str(&t);
                    }
                }
            }
            Ok(Event::GeneralRef(e)) => {
                let ch = match e.resolve_char_ref().map_err(|_| ())? {
                    Some(c) => c,
                    None => match e.decode().map_err(|_| ())?.as_ref() {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "quot" => '"',
                        "apos" => '\'',
                        _ => return Err(()),
                    },
                };
                if let Some(&(idx, has_child)) = stack.last() {
                    if !has_child {
                        out[idx].text.get_or_insert_with(String::new).push(ch);
                    }
                }
            }
            Ok(Event::CData(e)) => {
                if let Some(&(idx, has_child)) = stack.last() {
                    if !has_child {
                        let s = String::from_utf8_lossy(e.as_ref()).into_owned();
                        out[idx]
                            .text
                            .get_or_insert_with(String::new)
                            .push_str(&s);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(()),
        }
    }
    if !stack.is_empty() {
        return Err(()); // unclosed elements — expat would give a ParseError
    }
    Ok(out)
}

fn local_name(qname: &[u8]) -> String {
    let name = String::from_utf8_lossy(qname).into_owned();
    match name.rfind(':') {
        Some(i) => name[i + 1..].to_string(),
        None => name,
    }
}

/// Value of the `name` attribute (like `elem.get("name")`). Err — broken attributes.
fn attr_name(e: &quick_xml::events::BytesStart) -> Result<Option<String>, ()> {
    for attr in e.attributes() {
        let attr = attr.map_err(|_| ())?;
        if attr.key.as_ref() == b"name" {
            let v = attr.unescape_value().map_err(|_| ())?;
            return Ok(Some(v.into_owned()));
        }
    }
    Ok(None)
}

// ============================================================================
// Mappings (order as in Python, it matters for substring lookup)
// ============================================================================

/// `_CLSID_NAMES` from ThemepackImporter.
const CLSID_NAMES: &[(&str, &str)] = &[
    ("20d04fe0", "computer"),
    ("645ff040", "trash-full"),
    ("59031a47", "user-home"),
    ("f02c1a0d", "network"),
    ("871c5380", "internet"),
];

/// `WindowsIconImporter.ICON_MAP` — file name (without extension) → Linux names.
const ICON_MAP: &[(&str, &[&str])] = &[
    ("my computer", &["computer", "user-desktop"]),
    ("computer", &["computer", "user-desktop"]),
    ("this pc", &["computer", "user-desktop"]),
    ("recycle bin", &["user-trash"]),
    ("recycle bin full", &["user-trash-full"]),
    ("recycle_empty", &["user-trash"]),
    ("recycle_full", &["user-trash-full"]),
    ("trash_empty", &["user-trash"]),
    ("trash_full", &["user-trash-full"]),
    ("my documents", &["folder-documents"]),
    ("documents", &["folder-documents"]),
    ("my music", &["folder-music"]),
    ("music", &["folder-music"]),
    ("my pictures", &["folder-pictures"]),
    ("pictures", &["folder-pictures"]),
    ("my videos", &["folder-videos"]),
    ("videos", &["folder-videos"]),
    ("downloads", &["folder-download", "folder-downloads"]),
    ("network", &["network-workgroup", "network-server"]),
    ("network neighborhood", &["network-workgroup"]),
    ("internet", &["internet-web-browser", "web-browser"]),
    ("internet explorer", &["internet-web-browser"]),
    ("control panel", &["preferences-system", "preferences-desktop"]),
    ("folder", &["folder"]),
    ("folder_open", &["folder-open"]),
    ("open folder", &["folder-open"]),
    ("closed folder", &["folder"]),
    ("file", &["text-x-generic"]),
    ("document", &["text-x-generic", "x-office-document"]),
    ("application", &["application-x-executable"]),
    ("executable", &["application-x-executable"]),
    ("copy", &["edit-copy"]),
    ("cut", &["edit-cut"]),
    ("paste", &["edit-paste"]),
    ("delete", &["edit-delete", "user-trash"]),
    ("undo", &["edit-undo"]),
    ("redo", &["edit-redo"]),
    ("find", &["edit-find", "system-search"]),
    ("search", &["system-search"]),
    ("refresh", &["view-refresh"]),
    ("home", &["go-home", "user-home"]),
    ("back", &["go-previous"]),
    ("forward", &["go-next"]),
    ("up", &["go-up"]),
    ("hard drive", &["drive-harddisk"]),
    ("harddisk", &["drive-harddisk"]),
    ("cdrom", &["media-optical", "drive-optical"]),
    ("cd", &["media-optical"]),
    ("dvd", &["media-optical"]),
    ("floppy", &["media-floppy"]),
    ("usb", &["drive-removable-media"]),
    ("removable", &["drive-removable-media"]),
    ("printer", &["printer"]),
    ("error", &["dialog-error"]),
    ("warning", &["dialog-warning"]),
    ("info", &["dialog-information"]),
    ("question", &["dialog-question"]),
    ("shield", &["security-high"]),
];

/// `WindowsIconImporter.SIZES`.
const ICON_SIZES: &[u32] = &[16, 22, 24, 32, 48, 64, 128, 256];

/// `WindowsIconImporter.CONTEXTS` — Python dict order.
const ICON_CONTEXTS: &[(&str, &[&str])] = &[
    (
        "places",
        &[
            "computer", "user-desktop", "user-trash", "user-trash-full",
            "folder", "folder-open", "folder-documents", "folder-music",
            "folder-pictures", "folder-videos", "folder-download",
            "folder-downloads", "network-workgroup", "network-server",
            "user-home", "go-home",
        ],
    ),
    (
        "devices",
        &[
            "drive-harddisk", "media-optical", "drive-optical",
            "media-floppy", "drive-removable-media", "printer",
        ],
    ),
    (
        "apps",
        &[
            "internet-web-browser", "web-browser", "preferences-system",
            "preferences-desktop", "application-x-executable",
            "system-search",
        ],
    ),
    (
        "actions",
        &[
            "edit-copy", "edit-cut", "edit-paste", "edit-delete",
            "edit-undo", "edit-redo", "edit-find", "view-refresh",
            "go-previous", "go-next", "go-up",
        ],
    ),
    (
        "status",
        &[
            "dialog-error", "dialog-warning", "dialog-information",
            "dialog-question", "security-high",
        ],
    ),
    ("mimetypes", &["text-x-generic", "x-office-document"]),
];

/// `MSStyImporter.PART_MAP`. A deliberate divergence from the Python original:
/// there the substring lookup took the FIRST key in dict order (`startmenu`
/// was caught by the key `menu`, `startpanel` — by `pane`, `taskitemactive` —
/// by `taskitem`); here the longest matching key is chosen.
const PART_MAP: &[(&str, &str)] = &[
    ("caption", "window-titlebar"),
    ("framebottom", "window-bottom"),
    ("frameleft", "window-left"),
    ("frameright", "window-right"),
    ("closebutton", "window-close"),
    ("minbutton", "window-minimize"),
    ("maxbutton", "window-maximize"),
    ("restorebutton", "window-restore"),
    ("mdiclosebutton", "window-close"),
    ("mdiminbutton", "window-minimize"),
    ("pushbutton", "gtk-button"),
    ("radiobutton", "gtk-radio"),
    ("checkbox", "gtk-check"),
    ("combobox", "gtk-combo"),
    ("dropdownbutton", "gtk-combo-button"),
    ("scrollbar", "gtk-scrollbar"),
    ("thumbbtn", "gtk-scrollbar-thumb"),
    ("arrowbtn", "gtk-scrollbar-arrow"),
    ("track", "gtk-scrollbar-track"),
    ("grippervert", "gtk-scrollbar-grip"),
    ("menu", "gtk-menu"),
    ("menuband", "gtk-menubar"),
    ("toolbar", "gtk-toolbar"),
    ("rebar", "gtk-toolbar"),
    ("status", "gtk-statusbar"),
    ("tabitem", "gtk-tab"),
    ("pane", "gtk-pane"),
    ("header", "gtk-header"),
    ("progress", "gtk-progressbar"),
    ("trackbar", "gtk-scale"),
    ("thumb", "gtk-scale-thumb"),
    ("tooltip", "gtk-tooltip"),
    ("balloon", "gtk-tooltip"),
    ("startbutton", "plasma-start"),
    ("startpanel", "plasma-start"),
    ("startmenu", "plasma-start-menu"),
    ("startbanner", "plasma-start-banner"),
    ("logoff", "plasma-start-logoff"),
    ("logoffbuttons", "plasma-start-logoff"),
    ("taskbar", "plasma-panel"),
    ("taskband", "plasma-panel"),
    ("taskbarbackground", "plasma-panel"),
    ("tray", "plasma-systray"),
    ("traynotify", "plasma-systray"),
    ("clock", "plasma-clock"),
    ("showdesktop", "plasma-showdesktop"),
    ("taskitem", "plasma-taskbutton"),
    ("taskitemactive", "plasma-taskbutton-active"),
    ("groupmenu", "plasma-taskgroup"),
];

// ============================================================================
// can_import
// ============================================================================

/// `ThemepackImporter.can_import`: extension + CAB magic "MSCF".
pub fn themepack_can_import(io: &mut dyn ImpIo, path: &str) -> bool {
    let lower = path.to_lowercase();
    if !(lower.ends_with(".themepack") || lower.ends_with(".deskthemepack")) {
        return false;
    }
    match io.read_head_bytes(path, 4) {
        Some(magic) => magic == b"MSCF",
        None => false, // OSError suppressed
    }
}

/// `MSStyImporter.can_import`: extension + PE magic "MZ".
pub fn msstyles_can_import(io: &mut dyn ImpIo, path: &str) -> bool {
    if ext_lower(path) != ".msstyles" {
        return false;
    }
    match io.read_head_bytes(path, 2) {
        Some(magic) => magic == b"MZ",
        None => false,
    }
}

/// `WindowsCursorImporter.can_import`.
pub fn cursor_can_import(io: &mut dyn ImpIo, path: &str) -> bool {
    let ext = ext_lower(path);
    if matches!(ext.as_str(), ".cur" | ".ani" | ".inf" | ".crs" | ".curxptheme") {
        return true;
    }
    // An archive is a cursor set only if it contains .cur/.ani (any other ZIP is
    // installed as it was, e.g. a `.rxn` renamed to `.zip`).
    if is_cursor_archive_ext(&ext) {
        return io.is_file(path) && archive_has_cursors(io, path);
    }
    if io.is_dir(path) {
        for f in io.listdir(path) {
            let l = f.to_lowercase();
            if l.ends_with(".cur") || l.ends_with(".ani") {
                return true;
            }
        }
    }
    false
}

/// `WindowsIconImporter.can_import`.
pub fn icon_can_import(io: &mut dyn ImpIo, path: &str) -> bool {
    let ext = ext_lower(path);
    if matches!(ext.as_str(), ".ico" | ".icl" | ".iconpackage" | ".ip" | ".7tsp") {
        return true;
    }
    if matches!(ext.as_str(), ".dll" | ".exe") {
        if io.which("wrestool") {
            let r = io.run(
                vec!["wrestool".into(), "-l".into(), path.into()],
                10,
            );
            // A deliberate divergence from the Python original: there
            // `"group_icon" in r.stdout.lower()` — str-in-bytes → TypeError
            // on non-empty stdout; fixed here (stdout is decoded).
            return r.ok
                && String::from_utf8_lossy(&r.stdout)
                    .to_lowercase()
                    .contains("group_icon");
        }
        return false;
    }
    if io.is_dir(path) {
        return io
            .listdir(path)
            .iter()
            .any(|f| f.to_lowercase().ends_with(".ico"));
    }
    false
}

/// Format groups for the file chooser: (label, extensions without the dot).
///
/// Rule #12: one list for the whole program. Previously the «⚙️ Settings» tab
/// kept its own filter string, and it drifted away from the dispatcher — the
/// engine accepted desktop gadgets (`.gadget`), but such a file simply wasn't
/// shown to the user in the dialog.
/// Labels are `msgid`s, translated by [`import_dialog_filter`].
pub const IMPORT_GROUPS: &[(&str, &[&str])] = &[
    (
        // TRANSLATORS: file type group in the “Import Windows theme” file dialog.
        n_("Themes and Styles"),
        &["themepack", "deskthemepack", "theme", "msstyles"],
    ),
    (n_("Icons"), &["ico", "icl", "iconpackage", "ip", "7tsp"]),
    (n_("Cursors"), &["cur", "ani", "inf", "curxptheme"]),
    // Cursor sets are downloaded exactly like that — as an archive (Windows
    // cursor sites: zip/rar/7z; RW-Designer: a zip with a .crs roles file). The
    // importer won't accept any other archive: `cursor_can_import` looks inside.
    (n_("Cursor Sets"), &["zip", "rar", "7z", "crs"]),
    (n_("DreamScene Video Wallpapers"), &["dream", "wmv", "mpeg", "mpg"]),
    (n_("Desktop Gadgets"), &["gadget"]),
];

/// Whether the dispatcher accepts a file with this extension (`.themepack`,
/// the dot is required, any case). Built from [`IMPORT_GROUPS`], so "the
/// engine can, but the dialog doesn't show it" can no longer drift apart.
pub fn is_importable_ext(ext_with_dot: &str) -> bool {
    let ext = ext_with_dot.trim_start_matches('.').to_lowercase();
    IMPORT_GROUPS
        .iter()
        .any(|(_, exts)| exts.contains(&ext.as_str()))
}

/// Filter string for the file chooser: the combined group, then by type,
/// then «All files».
pub fn import_dialog_filter() -> String {
    let mask = |exts: &[&str]| -> String {
        exts.iter()
            .map(|e| format!("*.{e}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let all: Vec<&str> = IMPORT_GROUPS
        .iter()
        .flat_map(|(_, exts)| exts.iter().copied())
        .collect();
    // TRANSLATORS: file dialog filter; %1 is the list of file masks.
    let mut parts = vec![trf("All Windows Themes (%1)", &[&mask(&all)])];
    for (title, exts) in IMPORT_GROUPS {
        parts.push(format!("{} ({})", tr(title), mask(exts)));
    }
    parts.push(tr("All files (*)"));
    parts.join(";;")
}

/// `WindowsThemeImporter.can_import` — dispatch by extension.
pub fn windows_theme_can_import(io: &mut dyn ImpIo, path: &str) -> bool {
    let ext = ext_lower(path);
    // The list of accepted extensions is one, from IMPORT_GROUPS: the dispatcher
    // and the file chooser have nothing left to drift apart on.
    if !is_importable_ext(&ext) {
        return false;
    }
    match ext.as_str() {
        ".themepack" | ".deskthemepack" => themepack_can_import(io, path),
        ".theme" => match io.read_head_text(path, 500) {
            Some(head) => {
                let h = head.to_lowercase();
                h.contains("[theme]") || h.contains("[control panel")
            }
            None => false,
        },
        ".dream" | ".wmv" | ".mpeg" | ".mpg" => io.is_file(path),
        ".gadget" => io.is_zipfile(path).unwrap_or(false),
        ".msstyles" => msstyles_can_import(io, path),
        ".cur" | ".ani" | ".inf" | ".crs" | ".curxptheme" | ".zip" | ".rar" | ".7z" => {
            cursor_can_import(io, path)
        }
        ".ico" | ".icl" | ".iconpackage" | ".ip" | ".7tsp" => {
            icon_can_import(io, path)
        }
        _ => false,
    }
}

// ============================================================================
// WindowsThemeImporter.import_to_rxn — dispatcher
// ============================================================================

/// `WindowsThemeImporter.import_to_rxn`.
pub fn windows_theme_import_to_rxn(
    io: &mut dyn ImpIo,
    path: &str,
    output_dir: Option<&str>,
) -> Result<String, ImpError> {
    let ext = ext_lower(path);
    match ext.as_str() {
        ".themepack" | ".deskthemepack" => {
            themepack_import_to_rxn(io, path, output_dir)
        }
        ".theme" => theme_file_import_to_rxn(io, path, output_dir),
        ".wmv" | ".mpeg" | ".mpg" | ".dream" => {
            dreamscene_import_to_rxn(io, path, output_dir)
        }
        ".gadget" => gadget_import_to_rxn(io, path, output_dir),
        ".msstyles" => msstyles_import_to_rxn(io, path, output_dir),
        ".cur" | ".ani" | ".inf" | ".crs" | ".curxptheme" | ".zip" | ".rar" | ".7z" => {
            cursor_import_to_rxn(io, path, "", output_dir)
        }
        ".ico" | ".icl" | ".iconpackage" | ".ip" | ".7tsp" => {
            icon_import_to_rxn(io, path, "", output_dir)
        }
        _ => Err(verr(trf("Unsupported format: %1", &[&ext]))),
    }
}

// ============================================================================
// ThemepackImporter.import_to_rxn
// ============================================================================

/// `ThemepackImporter.import_to_rxn`.
pub fn themepack_import_to_rxn(
    io: &mut dyn ImpIo,
    themepack_path: &str,
    output_dir: Option<&str>,
) -> Result<String, ImpError> {
    let work_dir = io.mkdtemp("rexon_themepack_");
    let result = themepack_inner(io, themepack_path, output_dir, &work_dir);
    io.remove_tree(&work_dir); // finally
    result
}

fn themepack_inner(
    io: &mut dyn ImpIo,
    themepack_path: &str,
    output_dir: Option<&str>,
    work_dir: &str,
) -> Result<String, ImpError> {
    // ── 1. Unpack the CAB ──
    let extract_dir = join(work_dir, "extracted");
    io.makedirs(&extract_dir);
    let mut ok = io
        .run(
            vec![
                "cabextract".into(),
                "-d".into(),
                extract_dir.clone(),
                themepack_path.into(),
            ],
            30,
        )
        .ok;
    if !ok {
        ok = io
            .run(
                vec![
                    "7z".into(),
                    "x".into(),
                    format!("-o{extract_dir}"),
                    themepack_path.into(),
                ],
                30,
            )
            .ok;
    }
    if !ok {
        return Err(rerr(trf("Couldn't unpack the .themepack. %1", &[&legacy_hint(HINT_CABEXTRACT)])));
    }

    // ── 2. Find and parse the .theme ──
    let mut theme_file: Option<String> = None;
    'outer: for (root, _dirs, files) in io.walk(&extract_dir) {
        for f in &files {
            if f.to_lowercase().ends_with(".theme") {
                theme_file = Some(join(&root, f));
                break 'outer;
            }
        }
    }

    let mut theme_name = stem(themepack_path).to_string();
    let mut accent_color = String::new();

    if let Some(tf) = &theme_file {
        // try: open+read+read_string — errors suppressed (partial result kept)
        let mut ini = Ini::default();
        if let Some(content) = io.read_text(tf) {
            let (parsed, _suppressed) = Ini::read_string(&content);
            ini = parsed;
        }
        if ini.has_section("Theme") {
            theme_name = ini.get_fb("Theme", "DisplayName", &theme_name)?;
            // @themeui.dll,-NNN → name from the file
            if theme_name.starts_with('@') {
                theme_name = stem(themepack_path).to_string();
            }
        }
        for section in ini.section_names() {
            if section.to_lowercase().contains("color") {
                for key in ini.options(&section) {
                    if key.to_lowercase().contains("background") {
                        let val = ini.get_fb(&section, &key, "")?;
                        if !val.is_empty() {
                            accent_color = val;
                            break; // only the inner loop — as in Python
                        }
                    }
                }
            }
        }
    }

    // ── 3. Resources: wallpapers, icons, cursors ──
    const IMG_EXTS: &[&str] = &[".jpg", ".jpeg", ".png", ".bmp", ".webp"];
    const ICO_EXTS: &[&str] = &[".ico", ".cur", ".ani"];
    let mut all_images: Vec<(String, u64)> = Vec::new();
    let mut all_icons: Vec<String> = Vec::new();
    let mut themepack_icons = OrdMap::default();

    for (root, _dirs, files) in io.walk(&extract_dir) {
        for f in &files {
            let full = join(&root, f);
            let ext = ext_lower(f);
            if IMG_EXTS.contains(&ext.as_str()) {
                let sz = io.getsize(&full);
                all_images.push((full, sz));
            } else if ICO_EXTS.contains(&ext.as_str()) {
                all_icons.push(full);
            }
        }
    }

    // Icons from the CLSID sections of .theme (the file is read again — as in the original).
    // The `.theme` text from this same read is needed below too — to learn which
    // pictures the package itself calls wallpapers.
    let mut theme_text = String::new();
    if let Some(tf) = &theme_file {
        // try: suppressed
        if let Some(content) = io.read_text(tf) {
            theme_text = content.clone();
            for (guid_raw, icon_val) in clsid_finditer(&content) {
                let guid: String = guid_raw
                    .to_lowercase()
                    .replace('-', "")
                    .chars()
                    .take(8)
                    .collect();
                let icon_name = CLSID_NAMES
                    .iter()
                    .find(|(g, _)| *g == guid)
                    .map_or_else(|| format!("icon-{guid}"), |(_, n)| (*n).to_string());
                let icon_val = py_strip(&icon_val);
                let icon_file = py_strip(icon_val.split(',').next().unwrap_or(""))
                    .trim_matches('"')
                    .replace('\\', "/");
                let base = basename(&icon_file).to_lowercase();
                for (root, _dirs, files) in io.walk(&extract_dir) {
                    for f in &files {
                        if f.to_lowercase() == base {
                            themepack_icons
                                .insert(icon_name.clone(), join(&root, f));
                        }
                    }
                }
            }
        }
    }

    // Wallpapers are what the package itself calls wallpapers: pictures from the
    // `DesktopBackground` directory and those referenced by `.theme`
    // (`Wallpaper=`, `[Slideshow] ItemNPath=`). Previously the file size alone
    // decided (>100 KB), and a 99.9 KB background went off "as an icon" into the
    // icon theme (acceptance 10.10.2026). Size remains a fallback sign for
    // packages without these hints. "Picture icons" are only small PNG/BMP: a
    // photo is never an icon.
    let named_wallpapers: Vec<String> = themepack_wallpaper_names(&theme_text);
    let mut wallpapers: Vec<String> = Vec::new();
    for (path, sz) in &all_images {
        let lower = path.to_lowercase().replace('\\', "/");
        let in_backgrounds = lower.split('/').any(|part| part == "desktopbackground");
        let named = named_wallpapers.iter().any(|n| *n == basename(&lower));
        let ext = ext_lower(path);
        if in_backgrounds || named || *sz > 100_000 {
            wallpapers.push(path.clone());
        } else if *sz > 1_000 && (ext == ".png" || ext == ".bmp") {
            let name = stem(path).to_string();
            themepack_icons.insert(name, path.clone());
        }
    }

    let has_icons = !all_icons.is_empty() || !themepack_icons.is_empty();
    if wallpapers.is_empty() && !has_icons {
        return Err(verr(tr("No wallpapers or icons found in the .themepack")));
    }

    // ── 4. Build the .rxn: copy the wallpapers ──
    let rxn_dir = join(work_dir, "rxn_content");
    let data_dir = join(&rxn_dir, "data");
    io.makedirs(&data_dir);

    let mut wp_manifest: Vec<Value> = Vec::new();
    for (i, wp_path) in wallpapers.iter().enumerate() {
        let mut ext = ext_lower(wp_path);
        if ext == ".bmp" {
            ext = ".png".to_string();
        }
        let mut dest_name = format!("wallpaper_{}{}", i + 1, ext);
        let dest = join(&data_dir, &dest_name);
        if wp_path.to_lowercase().ends_with(".bmp") {
            let r = io.run(
                vec!["convert".into(), wp_path.clone(), dest.clone()],
                15,
            );
            if !r.ok {
                io.copy_file(wp_path, &dest.replace(".png", ".bmp"));
                dest_name = dest_name.replace(".png", ".bmp");
            }
        } else {
            io.copy_file(wp_path, &dest);
        }
        wp_manifest.push(json!({
            "file": format!("data/{dest_name}"),
            "format": "static",
        }));
    }

    // 4a-2. Upscale small wallpapers.
    // A deliberate divergence from the Python original: there it is a NameError
    // (WallpaperUpscaler not imported), fixed here — the call goes through the
    // executor, the tail of the import is ported.
    io.upscale_wallpapers(&data_dir);

    // 4b. Icons → freedesktop icon theme structure
    //
    // The icon theme directory is named after the package: the install puts it
    // into the user's icon directory under THAT name, and the old shared `icons`
    // gave `~/.local/share/icons/icons` — one folder for all packages, each next
    // one overwrote the previous (acceptance 10.10.2026).
    let mut safe_id = slug60(&theme_name);
    if safe_id.is_empty() {
        safe_id = "imported-themepack".to_string();
    }
    let icon_dir_name = format!("{safe_id}-icons");
    let mut icons_manifest: Vec<String> = Vec::new();
    if has_icons {
        let icon_theme_dir = join(&data_dir, &icon_dir_name);
        for size_dir in ["48x48", "64x64", "128x128"] {
            for ctx in ["places", "devices", "apps", "status"] {
                // A deliberate divergence from the Python original: there the typo
                // exist_ok=True inside os.path.join → TypeError; here a
                // correct makedirs(..., exist_ok=True).
                io.makedirs(&join(&join(&icon_theme_dir, size_dir), ctx));
            }
        }

        // Convert .ico → .png
        for ico_path in &all_icons {
            let name = stem(ico_path).to_string();
            let dest = join(
                &join(&join(&icon_theme_dir, "48x48"), "places"),
                &format!("{name}.png"),
            );
            let r = io.run(
                vec![
                    "convert".into(),
                    format!("{ico_path}[0]"),
                    "-resize".into(),
                    "48x48".into(),
                    dest,
                ],
                10,
            );
            if r.ok {
                icons_manifest.push(format!("data/{icon_dir_name}/48x48/places/{name}.png"));
            }
        }

        // Named icons from .theme (computer, trash, ...)
        for (icon_name, icon_path) in themepack_icons.iter() {
            let ext = ext_lower(icon_path);
            if ext == ".ico" {
                let dest = join(
                    &join(&join(&icon_theme_dir, "48x48"), "places"),
                    &format!("{icon_name}.png"),
                );
                io.run(
                    vec![
                        "convert".into(),
                        format!("{icon_path}[0]"),
                        "-resize".into(),
                        "48x48".into(),
                        dest,
                    ],
                    10,
                );
            } else if IMG_EXTS.contains(&ext.as_str()) {
                let dest = join(
                    &join(&join(&icon_theme_dir, "48x48"), "places"),
                    &format!("{icon_name}{ext}"),
                );
                io.copy_file(icon_path, &dest);
            }
        }

        // index.theme for the icon set
        let index_content = format!(
            "[Icon Theme]\nName={theme_name} Icons\nComment=Icons imported from Windows .themepack\nInherits=hicolor\nDirectories=48x48/places,64x64/places,128x128/places\n\n[48x48/places]\nSize=48\nContext=Places\nType=Fixed\n\n[64x64/places]\nSize=64\nContext=Places\nType=Fixed\n\n[128x128/places]\nSize=128\nContext=Places\nType=Fixed\n"
        );
        io.write_file(&join(&icon_theme_dir, "index.theme"), &index_content);
    }

    // Preview — the first wallpaper (scaled down)
    let preview_src = if !wallpapers.is_empty() {
        wallpapers[0].clone()
    } else if !all_icons.is_empty() {
        all_icons[0].clone()
    } else {
        themepack_icons
            .first_value()
            .expect("has_icons гарантирует непустой набор")
            .to_string()
    };
    let preview_dest = join(&rxn_dir, "preview.png");
    let r = io.run(
        vec![
            "convert".into(), preview_src.clone(),
            "-resize".into(), "620x348^".into(),
            "-gravity".into(), "center".into(),
            "-extent".into(), "620x348".into(),
            preview_dest.clone(),
        ],
        15,
    );
    if !r.ok {
        io.copy_file(&preview_src, &preview_dest);
    }

    // manifest.json
    let rxn_type = if !wallpapers.is_empty() {
        "wallpaper-pack"
    } else {
        "theme-icons"
    };

    let mut desc_parts = vec![tr("Imported from a Windows .themepack")];
    if !wallpapers.is_empty() {
        desc_parts.push(ntr("%n wallpaper", "%n wallpapers", wallpapers.len() as u64));
    }
    if has_icons {
        let icon_count = all_icons.len() + themepack_icons.len();
        desc_parts.push(ntr("%n icon", "%n icons", icon_count as u64));
    }
    if !accent_color.is_empty() {
        // TRANSLATORS: part of a theme description; %1 is a color like #3366cc.
        desc_parts.push(trf("color: %1", &[&accent_color]));
    }

    let mut manifest = Map::new();
    manifest.insert("format_version".into(), json!(3));
    manifest.insert("id".into(), json!(format!("themepack-{safe_id}")));
    manifest.insert("name".into(), json!(theme_name));
    manifest.insert("version".into(), json!("1.0"));
    manifest.insert("author".into(), json!("Windows Theme"));
    manifest.insert("description".into(), json!(desc_parts.join(" | ")));
    manifest.insert("type".into(), json!(rxn_type));
    manifest.insert("tags".into(), json!(["windows", "themepack", "imported"]));
    manifest.insert("de_support".into(), json!(["any"]));
    if !wallpapers.is_empty() {
        manifest.insert("wallpapers".into(), json!(wp_manifest));
    }
    if !icons_manifest.is_empty() {
        manifest.insert("icons".into(), json!(icons_manifest));
        manifest.insert("icon_theme_dir".into(), json!(format!("data/{icon_dir_name}")));
    }
    if !accent_color.is_empty() {
        manifest.insert("accent_color".into(), json!(accent_color));
    }
    io.write_file(
        &join(&rxn_dir, "manifest.json"),
        &dumps_indent2(&Value::Object(manifest)),
    );

    // ── 5. Pack into .rxn ──
    let out_dir = match output_dir {
        Some(o) => o.to_string(),
        None => {
            let d = dirname(themepack_path);
            if d.is_empty() { ".".to_string() } else { d }
        }
    };
    let rxn_path = join(&out_dir, &format!("{safe_id}.rxn"));
    io.pack_rxn(&rxn_dir, &rxn_path);
    Ok(rxn_path)
}

/// File names (lowercase, without directories) that `.theme` calls
/// wallpapers: `Wallpaper=` in `[Control Panel\\Desktop]` and `ItemNPath=` in
/// `[Slideshow]`. Parsing is line by line: values in these files are Windows
/// paths with environment variables, and only their last component matters.
pub fn themepack_wallpaper_names(theme_text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in theme_text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim().to_lowercase();
        let is_item = key.starts_with("item") && key.ends_with("path");
        if key != "wallpaper" && !is_item {
            continue;
        }
        let value = value.trim().trim_matches('"').replace('\\', "/").to_lowercase();
        let name = value.rsplit('/').next().unwrap_or("").to_string();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

// ============================================================================
// _import_theme_file
// ============================================================================

/// `WindowsThemeImporter._import_theme_file`.
pub fn theme_file_import_to_rxn(
    io: &mut dyn ImpIo,
    theme_path: &str,
    output_dir: Option<&str>,
) -> Result<String, ImpError> {
    let work_dir = io.mkdtemp("rexon_wintheme_");
    let result = theme_file_inner(io, theme_path, output_dir, &work_dir);
    io.remove_tree(&work_dir);
    result
}

fn theme_file_inner(
    io: &mut dyn ImpIo,
    theme_path: &str,
    output_dir: Option<&str>,
    work_dir: &str,
) -> Result<String, ImpError> {
    let content = io.read_text(theme_path).ok_or_else(|| oserr(theme_path))?;
    // read_string is NOT wrapped in try here — parse errors propagate
    let (ini, err) = Ini::read_string(&content);
    if let Some(e) = err {
        return Err(e);
    }

    let mut theme_name = ini.get_fb("Theme", "DisplayName", "")?;
    if theme_name.is_empty() || theme_name.starts_with('@') {
        theme_name = stem(theme_path).to_string();
    }

    let mut accent = String::new();
    for section in ini.section_names() {
        if section.to_lowercase().contains("color") {
            for key in ini.options(&section) {
                if key.to_lowercase().contains("background") {
                    accent = ini.get_fb(&section, &key, "")?;
                    break; // the break here is unconditional — as in Python
                }
            }
        }
    }

    let mut slideshow_interval: i64 = 0;
    let mut slideshow_shuffle = false;
    if ini.has_section("Slideshow") {
        // int(...) → ValueError suppressed (the value stays 0)
        if let Some(v) = py_int(&ini.get_fb("Slideshow", "Interval", "0")?) {
            slideshow_interval = v;
        }
        slideshow_shuffle = ini.get_fb("Slideshow", "Shuffle", "0")? == "1";
    }

    // Wallpapers next to the .theme file (> 50 KB)
    let theme_dir = dirname(theme_path);
    const IMG_EXTS: &[&str] =
        &[".jpg", ".jpeg", ".png", ".bmp", ".webp", ".tiff"];
    let mut wallpapers: Vec<String> = Vec::new();
    for (root, _dirs, files) in io.walk(&theme_dir) {
        for f in &files {
            if IMG_EXTS.contains(&ext_lower(f).as_str()) {
                let full = join(&root, f);
                if io.getsize(&full) > 50_000 {
                    wallpapers.push(full);
                }
            }
        }
    }
    if wallpapers.is_empty() {
        return Err(verr(tr("No wallpapers found next to the .theme file")));
    }

    let rxn_dir = join(work_dir, "rxn");
    let data_dir = join(&rxn_dir, "data");
    io.makedirs(&data_dir);

    let mut wp_manifest: Vec<Value> = Vec::new();
    for (i, wp) in wallpapers.iter().enumerate() {
        let ext = ext_lower(wp);
        let dest_name = if ext == ".bmp" {
            let dest_name = format!("wallpaper_{}.png", i + 1);
            let dest = join(&data_dir, &dest_name);
            let r = io.run(vec!["convert".into(), wp.clone(), dest], 15);
            if !r.ok {
                let dest_name = format!("wallpaper_{}.bmp", i + 1);
                io.copy_file(wp, &join(&data_dir, &dest_name));
                dest_name
            } else {
                dest_name
            }
        } else {
            let dest_name = format!("wallpaper_{}{}", i + 1, ext);
            io.copy_file(wp, &join(&data_dir, &dest_name));
            dest_name
        };
        wp_manifest.push(json!({
            "file": format!("data/{dest_name}"),
            "format": "static",
        }));
    }

    // Upscale small wallpapers.
    // A deliberate divergence from the Python original: there it is a NameError
    // (WallpaperUpscaler not imported), fixed here — the call goes through the
    // executor, the tail of the import is ported.
    io.upscale_wallpapers(&data_dir);

    // Preview
    let preview = join(&rxn_dir, "preview.png");
    io.run(
        vec![
            "convert".into(), wallpapers[0].clone(),
            "-resize".into(), "620x348^".into(),
            "-gravity".into(), "center".into(),
            "-extent".into(), "620x348".into(),
            preview.clone(),
        ],
        15,
    );
    if !io.exists(&preview) {
        io.copy_file(&wallpapers[0], &preview);
    }

    let safe_id = slug60(&theme_name);
    let mut manifest = Map::new();
    manifest.insert("format_version".into(), json!(3));
    manifest.insert("id".into(), json!(format!("win-theme-{safe_id}")));
    manifest.insert("name".into(), json!(theme_name));
    manifest.insert("version".into(), json!("1.0"));
    manifest.insert("author".into(), json!("Windows Theme"));
    manifest.insert(
        "description".into(),
        json!(format!(
            "{} | {}",
            tr("Imported from a Windows .theme"),
            ntr("%n wallpaper", "%n wallpapers", wallpapers.len() as u64)
        )),
    );
    manifest.insert("type".into(), json!("wallpaper-pack"));
    manifest.insert("tags".into(), json!(["windows", "imported"]));
    manifest.insert("de_support".into(), json!(["any"]));
    manifest.insert("wallpapers".into(), json!(wp_manifest));
    if !accent.is_empty() {
        manifest.insert("accent_color".into(), json!(accent));
    }
    if slideshow_interval > 0 {
        manifest.insert(
            "slideshow".into(),
            json!({
                "interval_ms": slideshow_interval,
                "shuffle": slideshow_shuffle,
            }),
        );
    }
    io.write_file(
        &join(&rxn_dir, "manifest.json"),
        &dumps_indent2(&Value::Object(manifest)),
    );

    let out_dir = match output_dir {
        Some(o) => o.to_string(),
        None => {
            let d = dirname(theme_path);
            if d.is_empty() { ".".to_string() } else { d }
        }
    };
    let rxn_path = join(&out_dir, &format!("{safe_id}.rxn"));
    io.pack_rxn(&rxn_dir, &rxn_path);
    Ok(rxn_path)
}

// ============================================================================
// _import_dreamscene
// ============================================================================

/// `WindowsThemeImporter._import_dreamscene`.
pub fn dreamscene_import_to_rxn(
    io: &mut dyn ImpIo,
    video_path: &str,
    output_dir: Option<&str>,
) -> Result<String, ImpError> {
    // ".dream" files are not supported yet. Previously this produced a package
    // with wallpapers that don't play — it is more honest to refuse right away.
    if ext_lower(video_path) == ".dream" {
        return Err(verr(tr(
            "“.dream” files are not supported yet. Support is planned for a future version.",
        )));
    }
    let work_dir = io.mkdtemp("rexon_dreamscene_");
    let result = dreamscene_inner(io, video_path, output_dir, &work_dir);
    io.remove_tree(&work_dir);
    result
}

fn dreamscene_inner(
    io: &mut dyn ImpIo,
    video_path: &str,
    output_dir: Option<&str>,
    work_dir: &str,
) -> Result<String, ImpError> {
    let name = stem(video_path).to_string();
    let safe_id = slug60(&name);

    let rxn_dir = join(work_dir, "rxn");
    let data_dir = join(&rxn_dir, "data");
    io.makedirs(&data_dir);

    let ext = ext_lower(video_path);
    let dest = if matches!(ext.as_str(), ".wmv" | ".mpeg" | ".mpg") {
        // The video is placed as is, without re-encoding: the wallpaper host plays
        // WMV and MPEG-1/2 itself, and they are decoded by the same system libavcodec
        // as ffmpeg uses — a fallback through ffmpeg would open nothing the host
        // can't, while always losing quality and import time.
        let dest = join(&data_dir, &format!("{safe_id}{ext}"));
        io.copy_file(video_path, &dest);
        dest
    } else {
        // unreachable through the dispatcher, but the method can be called directly
        let dest = join(&data_dir, basename(video_path));
        io.copy_file(video_path, &dest);
        dest
    };

    let dest_name = basename(&dest).to_string();

    // Preview: a frame from the video (the result is ignored)
    let preview = join(&rxn_dir, "preview.png");
    io.run(
        vec![
            "ffmpeg".into(), "-i".into(), dest.clone(),
            "-ss".into(), "00:00:02".into(), "-vframes".into(), "1".into(),
            "-vf".into(),
            "scale=620:348:force_original_aspect_ratio=increase,crop=620:348"
                .into(),
            "-y".into(), preview,
        ],
        30,
    );

    let manifest = json!({
        "format_version": 3,
        "id": format!("dreamscene-{safe_id}"),
        "name": format!("DreamScene: {name}"),
        "version": "1.0",
        "author": "Windows DreamScene",
        "description": tr("Video wallpaper from Windows Vista DreamScene"),
        "type": "wallpaper-pack",
        "tags": ["windows", "dreamscene", "video", "imported"],
        "de_support": ["any"],
        "wallpapers": [{"file": format!("data/{dest_name}"), "format": "video"}],
    });
    io.write_file(&join(&rxn_dir, "manifest.json"), &dumps_indent2(&manifest));

    let out_dir = match output_dir {
        Some(o) => o.to_string(),
        None => {
            let d = dirname(video_path);
            if d.is_empty() { ".".to_string() } else { d }
        }
    };
    let rxn_path = join(&out_dir, &format!("dreamscene-{safe_id}.rxn"));
    io.pack_rxn(&rxn_dir, &rxn_path);
    Ok(rxn_path)
}

// ============================================================================
// _import_gadget
// ============================================================================

/// `WindowsThemeImporter._import_gadget`.
pub fn gadget_import_to_rxn(
    io: &mut dyn ImpIo,
    gadget_path: &str,
    output_dir: Option<&str>,
) -> Result<String, ImpError> {
    let work_dir = io.mkdtemp("rexon_gadget_");
    let result = gadget_inner(io, gadget_path, output_dir, &work_dir);
    io.remove_tree(&work_dir);
    result
}

fn gadget_inner(
    io: &mut dyn ImpIo,
    gadget_path: &str,
    output_dir: Option<&str>,
    work_dir: &str,
) -> Result<String, ImpError> {
    let extract_dir = join(work_dir, "extracted");
    if !io.zip_open(gadget_path) {
        return Err(ImpError::new("BadZipFile", "File is not a zip file"));
    }
    io.extract_zip(gadget_path, &extract_dir);

    let mut name = stem(gadget_path).to_string();
    let mut author = "Windows Gadget".to_string();
    let mut description = String::new();

    let gadget_xml = join(&extract_dir, "gadget.xml");
    if io.is_file(&gadget_xml) {
        // try: ET.parse — errors suppressed
        if let Some(bytes) = io.read_bytes(&gadget_xml) {
            if let Ok(elems) = parse_gadget_xml(&bytes) {
                for el in elems {
                    if el.tag == "name" {
                        if let Some(t) = &el.text {
                            if !t.is_empty() {
                                name = py_strip(t).to_string();
                            }
                        }
                    } else if el.tag == "author" {
                        if let Some(a) = &el.name_attr {
                            if !a.is_empty() {
                                author = a.clone();
                            }
                        }
                    } else if el.tag == "description" {
                        if let Some(t) = &el.text {
                            if !t.is_empty() {
                                description = py_strip(t).to_string();
                            }
                        }
                    }
                }
            }
        }
    }

    let safe_id = slug60(&name);
    let rxn_dir = join(work_dir, "rxn");
    let data_dir = join(&rxn_dir, "data");

    io.copy_tree(&extract_dir, &data_dir);

    let mut main_html = String::new();
    for f in io.listdir(&data_dir) {
        let l = f.to_lowercase();
        if l.ends_with(".html") && l != "settings.html" {
            main_html = f;
            break;
        }
    }

    let desc_base = if description.is_empty() {
        "Windows Gadget"
    } else {
        &description
    };
    let mut manifest = Map::new();
    manifest.insert("format_version".into(), json!(3));
    manifest.insert("id".into(), json!(format!("gadget-{safe_id}")));
    manifest.insert("name".into(), json!(name));
    manifest.insert("version".into(), json!("1.0"));
    manifest.insert("author".into(), json!(author));
    manifest.insert(
        "description".into(),
        json!(format!("{desc_base} | {}", tr("Imported from a .gadget"))),
    );
    manifest.insert("type".into(), json!("theme-conky"));
    manifest.insert(
        "tags".into(),
        json!(["windows", "gadget", "widget", "imported"]),
    );
    manifest.insert("de_support".into(), json!(["any"]));
    if !main_html.is_empty() {
        manifest.insert("web_entry".into(), json!(format!("data/{main_html}")));
    }
    io.write_file(
        &join(&rxn_dir, "manifest.json"),
        &dumps_indent2(&Value::Object(manifest)),
    );

    let out_dir = match output_dir {
        Some(o) => o.to_string(),
        None => {
            let d = dirname(gadget_path);
            if d.is_empty() { ".".to_string() } else { d }
        }
    };
    let rxn_path = join(&out_dir, &format!("gadget-{safe_id}.rxn"));
    io.pack_rxn(&rxn_dir, &rxn_path);
    Ok(rxn_path)
}

// ============================================================================
// WindowsCursorImporter
// ============================================================================
//
// ⭐ Since 03.10.2026 cursor conversion is our own Rust code ([`crate::wincursor`]):
// all `.ani` frames with delays and hotspots, nominal sizes 24/32/48 (+64/96),
// without `win2xcur`/`convert`/`xcursorgen`. The old path took the first
// frame (`convert file[0]`) — animated sets (and Windows cursors have many)
// became static; without `xcursorgen` (it is missing almost everywhere) the
// import didn't work at all. The Python reference for this importer has been
// dropped (Python is an archive).
//
// Roles are taken from the set's descriptor (`install.inf`, `.crs`), not from
// file names; names are only a fallback. The set's ORIGINALS go into `.rxn`
// (`originals/`) — Wehrwolfmann's decision of 03.10: when the converter
// improves, we convert again without re-downloading ([`cursor_retranslate_dir`]).
//
// FORMAT of the `.rxn` manifest for Windows cursors (fields beyond the common v3):
//   "source": "windows"                — the "from Windows" sign (tile 🪟);
//   "source_site", "source_url",       — where the set came from (if known);
//   "license"
//   "author"                           — author (from the storefront or `IART` of .ani);
//   "windows_cursors": {
//     "converter": 2,                  — converter version ([`CURSOR_CONVERTER_VERSION`]);
//     "theme_dir": "<theme directory in data/>",
//     "descriptor": "install.inf"|null — which descriptor the roles came from;
//     "originals": ["Normal.ani", …],  — files in originals/ (as they arrived);
//     "roles": {"Arrow": "Normal.ani", …},
//     "animated": ["Wait", …],         — roles with animation;
//     "frames": {"Wait": 18, …},       — animation steps after merging;
//     "skipped": ["X.cur: reason", …]  — what could not be read.
//   }

/// Where the cursor set came from (for the `.rxn` manifest); empty fields are not written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CursorSource {
    /// Theme name (empty — from the set's descriptor or the file/folder name).
    pub name: String,
    /// Source site (`"rw-designer"`…); empty — a file brought by the person.
    pub site: String,
    /// URL of the set's page.
    pub url: String,
    /// Author of the set.
    pub author: String,
    /// License, as the site named it.
    pub license: String,
    /// id of the previous `.rxn` when converting again (empty — from the theme name).
    pub id: String,
}

/// Cursor converter version in the manifest: when it grows, old `.rxn` can be
/// converted again from their own originals.
pub const CURSOR_CONVERTER_VERSION: u32 = 2;

/// Whom a converter version bump concerns (Wehrwolfmann's decision of
/// 14.11.2026: re-conversion is automatic, but only when it matters: a theme
/// the fix doesn't concern is left alone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorFixScope {
    /// All cursors.
    All,
    /// Only animated `.ani`.
    Animated,
    /// Only those with a hotspot off the picture's center.
    HotspotOffCenter,
    /// Only those with palette images (1/4/8 bit).
    Paletted,
}

impl CursorFixScope {
    /// Whether the fix concerns a set with these traits.
    pub fn touches(self, traits: &wincursor::CursorTraits) -> bool {
        match self {
            CursorFixScope::All => true,
            CursorFixScope::Animated => traits.animated,
            CursorFixScope::HotspotOffCenter => traits.hotspot_off_center,
            CursorFixScope::Paletted => traits.paletted,
        }
    }
}

/// Table "converter version → whom its fix concerns". If you bump
/// [`CURSOR_CONVERTER_VERSION`], add a row (there is a guard in the tests).
/// Version 2 — the Rust converter instead of `convert`/`xcursorgen`: all cursors.
pub const CURSOR_CONVERTER_CHANGES: &[(u32, CursorFixScope)] = &[(2, CursorFixScope::All)];

/// Whether to re-convert a set converted by version `from` when the current
/// one is `current`: yes, when at least one fix between them concerns its
/// traits.
pub fn cursor_retranslate_due(
    from: u32,
    current: u32,
    changes: &[(u32, CursorFixScope)],
    traits: &wincursor::CursorTraits,
) -> bool {
    from < current
        && changes
            .iter()
            .any(|(v, scope)| *v > from && *v <= current && scope.touches(traits))
}

/// Traits of a set from the bytes of its originals; unreadable files don't count.
pub fn cursor_set_traits(files: &[Vec<u8>]) -> wincursor::CursorTraits {
    files
        .iter()
        .filter_map(|b| wincursor::cursor_file_traits(b))
        .fold(wincursor::CursorTraits::default(), wincursor::CursorTraits::merge)
}

/// Converter version of a "from Windows" cursor theme from its manifest;
/// `None` — the theme is not from Windows. No field — 1 (converted before versions existed).
pub fn cursor_manifest_converter(m: &Value) -> Option<u32> {
    if m.get("type").and_then(Value::as_str) != Some("theme-cursors")
        || m.get("source").and_then(Value::as_str) != Some("windows")
    {
        return None;
    }
    Some(
        m.get("windows_cursors")
            .and_then(|w| w.get("converter"))
            .and_then(Value::as_u64)
            .map_or(1, |v| v as u32),
    )
}

/// Write the converter version into the manifest (other fields as they were).
pub fn set_cursor_manifest_converter(m: &mut Value, version: u32) {
    if let Some(obj) = m.as_object_mut() {
        let wc = obj.entry("windows_cursors").or_insert_with(|| json!({}));
        if let Some(wc) = wc.as_object_mut() {
            wc.insert("converter".to_string(), json!(version));
        }
    }
}

/// `index.theme` key of a cursor theme converted from Windows (value
/// `windows`); the installed tile reads it.
pub const WINDOWS_CURSOR_ORIGIN_KEY: &str = "X-Rexon-Source";

/// Default author (the manifest requires a non-empty field).
const DEFAULT_CURSOR_AUTHOR: &str = "Windows Cursors";

/// What from the set's folder goes into `originals/` (preview pictures,
/// readme, descriptors and the cursors themselves) and limits — so `.rxn`
/// doesn't bloat with junk.
const ORIGINAL_EXTS: &[&str] = &[
    ".cur", ".ani", ".inf", ".crs", ".txt", ".png", ".jpg", ".jpeg", ".gif", ".bmp",
    ".ico", ".url", ".theme", ".htm", ".html",
];
const MAX_ORIGINAL_BYTES: u64 = 16 << 20;
const MAX_ORIGINALS_TOTAL: u64 = 128 << 20;

/// Roles on the set's cover — in this order, as many as there are (up to 10).
const PREVIEW_ROLES: &[Role] = &[
    Role::Arrow, Role::Help, Role::AppStarting, Role::Wait, Role::Crosshair,
    Role::IBeam, Role::Hand, Role::NWPen, Role::No, Role::SizeAll,
];

fn is_cursor_file(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with(".cur") || l.ends_with(".ani")
}

/// Whether this is an archive that may contain a cursor set.
fn is_cursor_archive_ext(ext: &str) -> bool {
    matches!(ext, ".zip" | ".rar" | ".7z")
}

/// List of names in a `.rar`/`.7z` archive via an external program; None — nothing to do it with.
fn list_other_archive(io: &mut dyn ImpIo, path: &str) -> Option<Vec<String>> {
    if io.which("bsdtar") {
        let r = io.run(vec!["bsdtar".into(), "-tf".into(), path.into()], 30);
        return Some(
            String::from_utf8_lossy(&r.stdout).lines().map(str::to_string).collect(),
        );
    }
    if io.which("7z") {
        let r = io.run(
            vec!["7z".into(), "l".into(), "-ba".into(), "-slt".into(), path.into()],
            30,
        );
        return Some(
            String::from_utf8_lossy(&r.stdout)
                .lines()
                .filter_map(|l| l.strip_prefix("Path = ").map(str::to_string))
                .collect(),
        );
    }
    None
}

/// Unpack `.rar`/`.7z` into `dest`: `bsdtar` (libarchive: refuses absolute
/// paths and `..`), otherwise `7z`.
fn extract_other_archive(io: &mut dyn ImpIo, archive: &str, dest: &str) {
    io.makedirs(dest);
    if io.which("bsdtar") {
        io.run(
            vec!["bsdtar".into(), "-xf".into(), archive.into(), "-C".into(), dest.into()],
            300,
        );
    } else if io.which("7z") {
        io.run(
            vec!["7z".into(), "x".into(), "-y".into(), format!("-o{dest}"), archive.into()],
            300,
        );
    }
}

/// Whether the archive contains cursors. `.rar`/`.7z` without an unpacker —
/// "yes": next comes an honest "install the program" refusal, not an attempt
/// to install the archive as `.rxn`. A ZIP with `manifest.json` at the root is
/// a `.rxn`, not a Windows set.
fn archive_has_cursors(io: &mut dyn ImpIo, path: &str) -> bool {
    let names = if ext_lower(path) == ".zip" {
        match io.zip_names(path) {
            Some(n) => n,
            None => return false,
        }
    } else {
        match list_other_archive(io, path) {
            Some(n) => n,
            None => return true,
        }
    };
    !names.iter().any(|n| n == "manifest.json") && names.iter().any(|n| is_cursor_file(n))
}

/// `WindowsCursorImporter.import_to_rxn` (old signature: theme name as a string).
pub fn cursor_import_to_rxn(
    io: &mut dyn ImpIo,
    source_path: &str,
    theme_name: &str,
    output_dir: Option<&str>,
) -> Result<String, ImpError> {
    let source = CursorSource { name: theme_name.to_string(), ..Default::default() };
    cursor_import_with_source(io, source_path, &source, output_dir)
}

/// Import a Windows cursor set with information about its source.
///
/// Input: `.cur`/`.ani` (one cursor → the arrow, unless the name says
/// otherwise), `.inf`/`.crs` (the set next to the descriptor), a folder,
/// `.zip`/`.curxptheme` (with our own ZIP reader), `.rar`/`.7z` (`bsdtar` or `7z`).
pub fn cursor_import_with_source(
    io: &mut dyn ImpIo,
    source_path: &str,
    source: &CursorSource,
    output_dir: Option<&str>,
) -> Result<String, ImpError> {
    let work_dir = io.mkdtemp("rexon_wincur_");
    let result = cursor_inner(io, source_path, source, output_dir, &work_dir);
    io.remove_tree(&work_dir);
    result
}

/// Re-convert a cursor theme from the originals lying in an unpacked `.rxn`
/// (or in the installed extension's directory) — without re-downloading. id,
/// name and source information are taken from its manifest.
pub fn cursor_retranslate_dir(
    io: &mut dyn ImpIo,
    rxn_dir: &str,
    output_dir: &str,
) -> Result<String, ImpError> {
    let manifest_path = join(rxn_dir, "manifest.json");
    let text = io.read_text(&manifest_path).ok_or_else(|| oserr(&manifest_path))?;
    let m: Value = serde_json::from_str(&text)
        .map_err(|e| verr(trf("manifest.json can't be read: %1", &[&e])))?;
    let originals = join(rxn_dir, "originals");
    if m.get("source").and_then(Value::as_str) != Some("windows") || !io.is_dir(&originals) {
        return Err(verr(tr("This cursor theme has no Windows originals to convert again")));
    }
    let field = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let author = field("author");
    let source = CursorSource {
        name: field("name"),
        site: field("source_site"),
        url: field("source_url"),
        author: if author == DEFAULT_CURSOR_AUTHOR { String::new() } else { author },
        license: field("license"),
        id: field("id"),
    };
    cursor_import_with_source(io, &originals, &source, Some(output_dir))
}

/// A candidate folder for a set inside the unpacked tree.
struct SetDir {
    dir: String,
    /// Regular files (without symbolic links), alphabetically.
    files: Vec<String>,
    cursors: usize,
    has_descriptor: bool,
}

fn is_descriptor(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with(".inf") || l.ends_with(".crs")
}

/// Walk the tree and collect folders with cursors.
fn collect_set_dirs(io: &mut dyn ImpIo, root: &str) -> Vec<SetDir> {
    let mut out = Vec::new();
    for (dir, _subdirs, files) in io.walk(root) {
        let mut plain = Vec::new();
        for f in files {
            // A link from the archive could lead reading outside the set.
            if !io.is_symlink(&join(&dir, &f)) {
                plain.push(f);
            }
        }
        plain.sort();
        let cursors = plain.iter().filter(|f| is_cursor_file(f)).count();
        if cursors == 0 {
            continue;
        }
        let has_descriptor = plain.iter().any(|f| is_descriptor(f));
        out.push(SetDir { dir, files: plain, cursors, has_descriptor });
    }
    out
}

/// One folder (without descending): the set next to a descriptor the person named.
fn one_set_dir(io: &mut dyn ImpIo, dir: &str) -> SetDir {
    let mut plain = Vec::new();
    for f in io.listdir(dir) {
        let p = join(dir, &f);
        if io.is_file(&p) && !io.is_symlink(&p) {
            plain.push(f);
        }
    }
    plain.sort();
    let cursors = plain.iter().filter(|f| is_cursor_file(f)).count();
    let has_descriptor = plain.iter().any(|f| is_descriptor(f));
    SetDir { dir: dir.to_string(), files: plain, cursors, has_descriptor }
}

/// Pick the set's folder: with a descriptor, of "regular" size (Bibata and
/// the like have Regular/Large/Extra-Large side by side), with more cursors;
/// on a tie — the first one in walk order.
fn pick_set_dir(sets: &[SetDir]) -> Option<usize> {
    let size_bonus = |dir: &str| -> i32 {
        let b = basename(dir).to_lowercase();
        if b.contains("large") || b.contains("big") || b.contains("huge") {
            -1
        } else if ["regular", "normal", "standard", "default", "medium"]
            .iter()
            .any(|k| b.contains(k))
        {
            1
        } else {
            0
        }
    };
    let mut best: Option<((bool, i32, usize), usize)> = None;
    for (i, s) in sets.iter().enumerate() {
        let key = (s.has_descriptor, size_bonus(&s.dir), s.cursors);
        if best.map(|(k, _)| key > k).unwrap_or(true) {
            best = Some((key, i));
        }
    }
    best.map(|(_, i)| i)
}

/// Find a file case-insensitively: first in the set's folder, then in the whole tree.
fn find_file_ci(set: &SetDir, all: &[SetDir], name: &str) -> Option<(String, String)> {
    let want = name.to_lowercase();
    let hit = |s: &SetDir| {
        s.files
            .iter()
            .find(|f| f.to_lowercase() == want)
            .map(|f| (join(&s.dir, f), f.clone()))
    };
    hit(set).or_else(|| all.iter().find_map(hit))
}

fn cursor_inner(
    io: &mut dyn ImpIo,
    source_path: &str,
    source: &CursorSource,
    output_dir: Option<&str>,
    work_dir: &str,
) -> Result<String, ImpError> {
    let ext = ext_lower(source_path);

    // ── 1. Where the set lies ──
    let mut preferred: Option<String> = None;
    let mut single: Option<String> = None;
    let fallback_name: String;
    let scan_root = if io.is_dir(source_path) {
        fallback_name = basename(source_path.trim_end_matches('/')).to_string();
        source_path.to_string()
    } else if ext == ".zip" || ext == ".curxptheme" {
        if !io.zip_open(source_path) {
            return Err(verr(trf("Couldn't unpack the archive: %1", &[&basename(source_path)])));
        }
        let dest = join(work_dir, "src");
        io.extract_zip(source_path, &dest);
        fallback_name = stem(source_path).to_string();
        dest
    } else if is_cursor_archive_ext(&ext) {
        let dest = join(work_dir, "src");
        extract_other_archive(io, source_path, &dest);
        fallback_name = stem(source_path).to_string();
        dest
    } else if ext == ".inf" || ext == ".crs" {
        preferred = Some(basename(source_path).to_string());
        fallback_name = stem(source_path).to_string();
        dirname(source_path)
    } else if ext == ".cur" || ext == ".ani" {
        single = Some(source_path.to_string());
        fallback_name = stem(source_path).to_string();
        dirname(source_path)
    } else {
        return Err(verr(trf("Unsupported format: %1", &[&ext])));
    };

    // ── 2. The set's folder and roles ──
    let all_sets: Vec<SetDir>;
    let set: SetDir;
    if let Some(file) = &single {
        // A single file: we don't touch its neighbours (the whole Downloads folder).
        all_sets = Vec::new();
        set = SetDir {
            dir: dirname(file),
            files: vec![basename(file).to_string()],
            cursors: 1,
            has_descriptor: false,
        };
    } else if preferred.is_some() {
        all_sets = Vec::new();
        set = one_set_dir(io, &scan_root);
        if set.cursors == 0 {
            return Err(verr(tr("No .cur/.ani cursors found")));
        }
    } else {
        let mut found = collect_set_dirs(io, &scan_root);
        let Some(i) = pick_set_dir(&found) else {
            return Err(verr(tr("No .cur/.ani cursors found")));
        };
        set = found.remove(i);
        all_sets = found;
    }

    // Descriptors by precedence: the one the person named, install.inf, other .inf, .crs.
    let mut descriptors: Vec<String> = set.files.iter().filter(|f| is_descriptor(f)).cloned().collect();
    descriptors.sort_by_key(|f| {
        let l = f.to_lowercase();
        (
            Some(f) != preferred.as_ref(),
            l != "install.inf",
            !l.ends_with(".inf"),
            l.clone(),
        )
    });
    let mut scheme = wincursor::SchemeInfo::default();
    let mut used_descriptor: Option<String> = None;
    for d in &descriptors {
        let Some(bytes) = io.read_bytes(&join(&set.dir, d)) else { continue };
        let text = wincursor::decode_text(&bytes);
        let s = if d.to_lowercase().ends_with(".crs") {
            wincursor::roles_from_crs(&text)
        } else {
            wincursor::roles_from_inf(&text)
        };
        if scheme.name.is_none() {
            scheme.name = s.name.clone();
        }
        if !s.roles.is_empty() {
            scheme.roles = s.roles;
            used_descriptor = Some(d.clone());
            break;
        }
    }
    // (role, full path, file name)
    let mut roles: Vec<(Role, String, String)> = Vec::new();
    for (role, fname) in &scheme.roles {
        if let Some((path, name)) = find_file_ci(&set, &all_sets, fname) {
            if is_cursor_file(&name) {
                roles.push((*role, path, name));
            }
        }
    }
    if roles.is_empty() {
        used_descriptor = None;
        for (role, name) in wincursor::roles_from_filenames(&set.files) {
            roles.push((role, join(&set.dir, &name), name));
        }
    }
    if roles.is_empty() {
        // The names say nothing: the first cursor is the arrow (that is how
        // "a single file" behaves too), the rest is kept in the originals.
        if let Some(first) = set.files.iter().find(|f| is_cursor_file(f)) {
            roles.push((Role::Arrow, join(&set.dir, first), first.clone()));
        }
    }
    if roles.is_empty() {
        return Err(verr(tr("No .cur/.ani cursors found")));
    }

    let theme_name = [source.name.as_str(), scheme.name.as_deref().unwrap_or(""), fallback_name.as_str()]
        .iter()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .unwrap_or("Imported Cursors")
        .to_string();

    // ── 3. Conversion to Xcursor ──
    let safe_id = icon_set_dir(match source.id.strip_prefix("wincur-").unwrap_or(&source.id) {
        "" => slug60(&theme_name),
        id => id.to_string(),
    });
    // The theme is written straight into data/ of the future .rxn: a copy via
    // copy_tree would turn synonym links into full copies of the animations.
    let rxn_dir = join(work_dir, "rxn");
    let theme_root = join(&join(&rxn_dir, "data"), &safe_id);
    let cursors_dir = join(&theme_root, "cursors");
    io.makedirs(&cursors_dir);

    let mut done: Vec<(Role, String, wincursor::Animation, usize)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for (role, path, name) in &roles {
        let parsed = match io.read_bytes(path) {
            Some(bytes) => wincursor::parse_cursor_file(&bytes),
            None => Err("file can't be read".to_string()),
        };
        match parsed {
            Ok(anim) => {
                let out = wincursor::to_xcursor(&anim, wincursor::XCURSOR_BUDGET);
                let names = role.xcursor_names();
                io.write_bytes(&join(&cursors_dir, names[0]), &out.bytes);
                for alt in &names[1..] {
                    io.symlink(names[0], &join(&cursors_dir, alt));
                }
                done.push((*role, name.clone(), anim, out.steps));
            }
            Err(e) => skipped.push(format!("{name}: {e}")),
        }
    }
    if done.is_empty() {
        return Err(rerr(trf(
            // TRANSLATORS: %1 lists the cursor files and why each failed.
            "None of the cursors could be read — the files are damaged or aren't Windows cursors: %1",
            &[&skipped.join("; ")],
        )));
    }
    // Roles missing from the set — with the closest cursor of THE SAME set.
    for role in wincursor::ALL_ROLES {
        if done.iter().any(|(r, ..)| *r == role) {
            continue;
        }
        if let Some(fb) = role.fallbacks().iter().find(|fb| done.iter().any(|(r, ..)| r == *fb)) {
            let target = fb.xcursor_names()[0];
            for n in role.xcursor_names() {
                io.symlink(target, &join(&cursors_dir, n));
            }
        }
    }

    // ── 4. index.theme ──
    // What is missing — from common themes (not "default": the desktop writes
    // ~/.icons/default with Inherits pointing at OUR theme, which would make a loop).
    // `X-Rexon-Source=windows` is a marker for the installed theme's tile
    // («🪟 From Windows», [`crate::installed_scan`]): the `.rxn` manifest doesn't
    // lie next to the theme after installation.
    io.write_file(
        &join(&theme_root, "index.theme"),
        &format!(
            "[Icon Theme]\nName={theme_name}\nComment=Imported from Windows cursors ({} cursors)\nInherits=breeze_cursors,Adwaita\n{WINDOWS_CURSOR_ORIGIN_KEY}=windows\n",
            done.len()
        ),
    );

    // ── 5. Packing: originals, cover, manifest ──
    let originals_dir = join(&rxn_dir, "originals");
    io.makedirs(&originals_dir);
    let mut originals: Vec<String> = Vec::new();
    let mut total = 0u64;
    for f in &set.files {
        let l = f.to_lowercase();
        if !ORIGINAL_EXTS.iter().any(|e| l.ends_with(e)) {
            continue;
        }
        let path = join(&set.dir, f);
        let size = io.getsize(&path);
        if size > MAX_ORIGINAL_BYTES || total + size > MAX_ORIGINALS_TOTAL {
            continue;
        }
        total += size;
        io.copy_file(&path, &join(&originals_dir, f));
        originals.push(f.clone());
    }
    // A role's cursor found outside the set's folder goes into the originals too.
    for (_, path, name) in &roles {
        if !originals.contains(name) && dirname(path) != set.dir {
            io.copy_file(path, &join(&originals_dir, name));
            originals.push(name.clone());
        }
    }

    let preview: Vec<&wincursor::Animation> = PREVIEW_ROLES
        .iter()
        .filter_map(|r| done.iter().find(|(x, ..)| x == r).map(|(_, _, a, _)| a))
        .collect();
    let preview: Vec<&wincursor::Animation> = if preview.is_empty() {
        done.iter().map(|(_, _, a, _)| a).take(10).collect()
    } else {
        preview
    };
    if let Some(png) = wincursor::preview_png(&preview, 620, 348, 64) {
        io.write_bytes(&join(&rxn_dir, "preview.png"), &png);
    }

    let author = [source.author.clone()]
        .into_iter()
        .chain(done.iter().filter_map(|(_, _, a, _)| a.artist.clone()))
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_CURSOR_AUTHOR.to_string());
    let mut role_map = Map::new();
    let mut frames = Map::new();
    let mut animated = Vec::new();
    for (role, name, anim, steps) in &done {
        role_map.insert(role.reg_name().to_string(), json!(name));
        if anim.is_animated() {
            animated.push(json!(role.reg_name()));
            frames.insert(role.reg_name().to_string(), json!(steps));
        }
    }
    let mut manifest = json!({
        "format_version": 3,
        "id": format!("wincur-{safe_id}"),
        "name": theme_name,
        "version": "1.0",
        "author": author,
        "description": format!(
            "{} | {}",
            tr("Imported from Windows"),
            // TRANSLATORS: %1 of %2 cursors were converted.
            trf("%1 of %2 cursors", &[&done.len(), &roles.len()])
        ),
        "type": "theme-cursors",
        "tags": ["windows", "cursors", "imported"],
        "de_support": ["any"],
        "source": "windows",
    });
    let obj = manifest.as_object_mut().expect("объект");
    for (key, value) in [
        ("source_site", &source.site),
        ("source_url", &source.url),
        ("license", &source.license),
    ] {
        if !value.trim().is_empty() {
            obj.insert(key.to_string(), json!(value.trim()));
        }
    }
    obj.insert(
        "windows_cursors".to_string(),
        json!({
            "converter": CURSOR_CONVERTER_VERSION,
            "theme_dir": safe_id,
            "descriptor": used_descriptor,
            "originals": originals,
            "roles": role_map,
            "animated": animated,
            "frames": frames,
            "skipped": skipped,
        }),
    );
    io.write_file(&join(&rxn_dir, "manifest.json"), &dumps_indent2(&manifest));

    let out_dir = match output_dir {
        Some(o) => o.to_string(),
        None => {
            if io.is_file(source_path) {
                dirname(source_path)
            } else {
                source_path.to_string()
            }
        }
    };
    let rxn_path = join(&out_dir, &format!("wincur-{safe_id}.rxn"));
    io.pack_rxn(&rxn_dir, &rxn_path);
    Ok(rxn_path)
}

// ============================================================================
// WindowsIconImporter
// ============================================================================

/// `WindowsIconImporter.import_to_rxn`.
pub fn icon_import_to_rxn(
    io: &mut dyn ImpIo,
    source_path: &str,
    theme_name: &str,
    output_dir: Option<&str>,
) -> Result<String, ImpError> {
    let work_dir = io.mkdtemp("rexon_winico_");
    let has_icotool = io.which("icotool");
    let has_wrestool = io.which("wrestool");
    let result = icon_inner(
        io, source_path, theme_name, output_dir, &work_dir, has_icotool,
        has_wrestool,
    );
    io.remove_tree(&work_dir);
    result
}

fn icon_inner(
    io: &mut dyn ImpIo,
    source_path: &str,
    theme_name_arg: &str,
    output_dir: Option<&str>,
    work_dir: &str,
    has_icotool: bool,
    has_wrestool: bool,
) -> Result<String, ImpError> {
    let ext = if io.is_file(source_path) {
        ext_lower(source_path)
    } else {
        String::new()
    };
    let mut ico_files = OrdMap::default();
    let mut theme_name = theme_name_arg.to_string();

    if io.is_dir(source_path) {
        for f in io.listdir(source_path) {
            if f.to_lowercase().ends_with(".ico") {
                ico_files
                    .insert(stem(&f).to_lowercase(), join(source_path, &f));
            }
        }
        if theme_name.is_empty() {
            theme_name = basename(source_path).to_string();
        }
    } else if ext == ".ico" {
        let name = stem(source_path).to_lowercase();
        ico_files.insert(name.clone(), source_path.to_string());
        if theme_name.is_empty() {
            theme_name = name;
        }
    } else if ext == ".icl" || ext == ".dll" || ext == ".exe" {
        if !has_wrestool {
            return Err(rerr(trf("wrestool not found. %1", &[&legacy_hint(HINT_ICOUTILS)])));
        }
        let extract_dir = join(work_dir, "extracted");
        io.makedirs(&extract_dir);
        io.run(
            vec![
                "wrestool".into(), "-x".into(), "-t".into(),
                "group_icon".into(), "-o".into(), extract_dir.clone(),
                source_path.into(),
            ],
            30,
        );
        for f in io.listdir(&extract_dir) {
            if f.to_lowercase().ends_with(".ico") {
                ico_files
                    .insert(stem(&f).to_lowercase(), join(&extract_dir, &f));
            }
        }
        if theme_name.is_empty() {
            theme_name = stem(source_path).to_string();
        }
    } else if matches!(ext.as_str(), ".iconpackage" | ".ip" | ".7tsp") {
        let extract_dir = join(work_dir, "extracted");
        if io.zip_open(source_path) {
            io.extract_zip(source_path, &extract_dir);
        } else {
            // Not a ZIP (real .7tsp can be like that) → try 7z. Fixed while porting:
            // the original ignored the exit code, and 7z pulls a PART of the files out
            // of a truncated archive and exits with code 2 — the import "succeeded",
            // and the user got a theme from a broken archive.
            if !io
                .run(
                    vec![
                        "7z".into(), "x".into(),
                        format!("-o{extract_dir}"),
                        source_path.into(),
                    ],
                    60,
                )
                .ok
            {
                return Err(ImpError::new("BadZipFile", "File is not a zip file"));
            }
        }
        for (root, _dirs, files) in io.walk(&extract_dir) {
            for f in &files {
                if f.to_lowercase().ends_with(".ico") {
                    ico_files.insert(stem(f).to_lowercase(), join(&root, f));
                }
            }
        }
        if theme_name.is_empty() {
            theme_name = stem(source_path).to_string();
        }
    }

    if ico_files.is_empty() {
        return Err(verr(tr("No .ico icons found")));
    }
    if theme_name.is_empty() {
        theme_name = "Imported Icons".to_string();
    }

    // ── 2. Convert .ico → PNG (multi-size) ──
    let safe_id = icon_set_dir(slug60(&theme_name));
    let theme_base = join(&join(work_dir, "theme"), &safe_id);

    // Sizes × contexts structure.
    // A deliberate divergence from the Python original: there the typo
    // exist_ok=True inside os.path.join → TypeError on the very first iteration;
    // here a correct makedirs(..., exist_ok=True), the tail of the import is ported.
    for size in ICON_SIZES {
        for (ctx, _) in ICON_CONTEXTS {
            io.makedirs(&join(
                &join(&theme_base, &format!("{size}x{size}")),
                ctx,
            ));
        }
    }

    let mut converted: u64 = 0;
    for (win_name, ico_path) in ico_files.iter() {
        // Linux names and context
        let linux_names: Vec<String> = ICON_MAP
            .iter()
            .find(|(k, _)| *k == win_name.as_str())
            .map_or_else(
                || vec![win_name.clone()],
                |(_, names)| names.iter().map(|s| (*s).to_string()).collect(),
            );
        let mut ctx = "places";
        for (context, names) in ICON_CONTEXTS {
            if linux_names.iter().any(|n| names.contains(&n.as_str())) {
                ctx = context;
                break;
            }
        }

        if has_icotool {
            // icotool extracts all sizes
            let tmp_extract = join(work_dir, &format!("_ico_{win_name}"));
            io.makedirs(&tmp_extract);
            io.run(
                vec![
                    "icotool".into(), "-x".into(), "-o".into(),
                    tmp_extract.clone(), ico_path.clone(),
                ],
                10,
            );
            // icotool creates files like name_N_NxN.png
            for png_file in io.listdir(&tmp_extract) {
                if !png_file.ends_with(".png") {
                    continue;
                }
                let mut matched = false;
                for size in ICON_SIZES {
                    if png_file.contains(&format!("_{size}x{size}"))
                        || png_file.contains(&format!("_{size}x"))
                    {
                        let dest = join(
                            &join(
                                &join(&theme_base, &format!("{size}x{size}")),
                                ctx,
                            ),
                            &format!("{}.png", linux_names[0]),
                        );
                        io.copy_file(&join(&tmp_extract, &png_file), &dest);
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    // Couldn't determine the size — put it into 48x48
                    let dest = join(
                        &join(&join(&theme_base, "48x48"), ctx),
                        &format!("{}.png", linux_names[0]),
                    );
                    if !io.exists(&dest) {
                        io.copy_file(&join(&tmp_extract, &png_file), &dest);
                    }
                }
            }
        } else {
            // Fallback: ImageMagick (takes the first frame)
            for size in [48u32, 32, 16] {
                let dest = join(
                    &join(&join(&theme_base, &format!("{size}x{size}")), ctx),
                    &format!("{}.png", linux_names[0]),
                );
                io.run(
                    vec![
                        "convert".into(),
                        format!("{ico_path}[0]"),
                        "-resize".into(),
                        format!("{size}x{size}"),
                        dest,
                    ],
                    10,
                );
            }
        }

        // Symlinks for alternative names
        for alt in &linux_names[1..] {
            for size in ICON_SIZES {
                let size_ctx =
                    join(&join(&theme_base, &format!("{size}x{size}")), ctx);
                let src = join(&size_ctx, &format!("{}.png", linux_names[0]));
                let dst = join(&size_ctx, &format!("{alt}.png"));
                if io.exists(&src) && !io.exists(&dst) {
                    // a symlink failure is suppressed by the original
                    io.symlink(&format!("{}.png", linux_names[0]), &dst);
                }
            }
        }

        converted += 1;
    }

    // ── 3. index.theme ──
    let mut dirs_list: Vec<String> = Vec::new();
    for size in ICON_SIZES {
        for (ctx, _) in ICON_CONTEXTS {
            let ctx_dir =
                join(&join(&theme_base, &format!("{size}x{size}")), ctx);
            if !io.listdir(&ctx_dir).is_empty() {
                dirs_list.push(format!("{size}x{size}/{ctx}"));
            }
        }
    }

    let mut index_content = format!(
        "[Icon Theme]\nName={theme_name}\nComment=Imported from Windows ({converted} icons)\nInherits=hicolor\nDirectories={}\n\n",
        dirs_list.join(",")
    );
    for d in &dirs_list {
        let size = d.split('x').next().unwrap_or("");
        let ctx = d.split('/').nth(1).unwrap_or("");
        // str.capitalize(): first letter up, the rest down
        let mut chars = ctx.chars();
        let ctx_cap = match chars.next() {
            Some(c) => {
                c.to_uppercase().collect::<String>()
                    + &chars.as_str().to_lowercase()
            }
            None => String::new(),
        };
        index_content.push_str(&format!(
            "[{d}]\nSize={size}\nContext={ctx_cap}\nType=Fixed\n\n"
        ));
    }
    io.write_file(&join(&theme_base, "index.theme"), &index_content);

    // ── 4. Pack into .rxn ──
    let rxn_dir = join(work_dir, "rxn");
    let data_dir = join(&rxn_dir, "data");
    io.copy_tree(&theme_base, &join(&data_dir, &safe_id));

    // Preview (the convert result is ignored)
    let preview = join(&rxn_dir, "preview.png");
    let first_ico = ico_files
        .first_value()
        .expect("ico_files не пуст — проверено выше")
        .to_string();
    io.run(
        vec![
            "convert".into(),
            format!("{first_ico}[0]"),
            "-resize".into(), "128x128".into(),
            "-gravity".into(), "center".into(),
            "-background".into(), "none".into(),
            "-extent".into(), "620x348".into(),
            preview,
        ],
        15,
    );

    let manifest = json!({
        "format_version": 3,
        "id": format!("winico-{safe_id}"),
        "name": theme_name,
        "version": "1.0",
        "author": "Windows Icons",
        "description": format!(
            "{} | {}",
            tr("Imported from Windows"),
            ntr("%n icon", "%n icons", converted as u64)
        ),
        "type": "theme-icons",
        "tags": ["windows", "icons", "imported"],
        "de_support": ["any"],
    });
    io.write_file(&join(&rxn_dir, "manifest.json"), &dumps_indent2(&manifest));

    let out_dir = match output_dir {
        Some(o) => o.to_string(),
        None => {
            // os.path.dirname without a "." fallback — as in the original
            if io.is_file(source_path) {
                dirname(source_path)
            } else {
                source_path.to_string()
            }
        }
    };
    let rxn_path = join(&out_dir, &format!("winico-{safe_id}.rxn"));
    io.pack_rxn(&rxn_dir, &rxn_path);
    Ok(rxn_path)
}

// ============================================================================
// MSStyImporter
// ============================================================================

/// `MSStyImporter.import_to_rxn`.
pub fn msstyles_import_to_rxn(
    io: &mut dyn ImpIo,
    source_path: &str,
    output_dir: Option<&str>,
) -> Result<String, ImpError> {
    let work_dir = io.mkdtemp("rexon_msstyles_");
    let has_wrestool = io.which("wrestool");
    let result =
        msstyles_inner(io, source_path, output_dir, &work_dir, has_wrestool);
    io.remove_tree(&work_dir);
    result
}

fn msstyles_inner(
    io: &mut dyn ImpIo,
    source_path: &str,
    output_dir: Option<&str>,
    work_dir: &str,
    has_wrestool: bool,
) -> Result<String, ImpError> {
    let theme_name = stem(source_path).to_string();
    let safe_id = slug60(&theme_name);

    // ── 1. Extract bitmap resources ──
    let bitmaps_dir = join(work_dir, "bitmaps");
    io.makedirs(&bitmaps_dir);

    if has_wrestool {
        io.run(
            vec![
                "wrestool".into(), "-x".into(), "-t".into(), "bitmap".into(),
                "-o".into(), bitmaps_dir.clone(), source_path.into(),
            ],
            30,
        );
        io.run(
            vec![
                "wrestool".into(), "-x".into(), "-t".into(), "14".into(),
                "-o".into(), bitmaps_dir.clone(), source_path.into(),
            ],
            30,
        );
    }

    // short-circuit: which("7z") — only when the directory is empty
    let mut used_7z = false;
    if io.listdir(&bitmaps_dir).is_empty() && io.which("7z") {
        io.run(
            vec![
                "7z".into(), "x".into(),
                format!("-o{bitmaps_dir}"), source_path.into(),
            ],
            30,
        );
        used_7z = true;
    }

    // BMP → PNG (the converted one is placed next to the source)
    for (f, full) in list_bitmap_dir(io, &bitmaps_dir, used_7z) {
        if f.to_lowercase().ends_with(".bmp") {
            let dst = join(&dirname(&full), &format!("{}.png", splitext(&f).0));
            io.run(vec!["convert".into(), full.clone(), dst.clone()], 10);
            if io.exists(&dst) {
                io.remove_file(&full);
            }
        }
    }

    // (file name, full path)
    let bitmaps: Vec<(String, String)> =
        list_bitmap_dir(io, &bitmaps_dir, used_7z)
            .into_iter()
            .filter(|(f, _)| {
                let l = f.to_lowercase();
                l.ends_with(".png") || l.ends_with(".bmp")
            })
            .collect();
    if bitmaps.is_empty() {
        return Err(verr(trf(
            "No bitmap resources found in the .msstyles. %1",
            &[&legacy_hint(HINT_ICOUTILS)],
        )));
    }

    // ── 2. Classification ──
    let mut window_parts: Vec<(String, String)> = Vec::new(); // (full path, type)
    let mut gtk_parts: Vec<(String, String)> = Vec::new();
    let mut plasma_parts: Vec<(String, String)> = Vec::new();
    let mut other_parts: Vec<(String, String)> = Vec::new(); // (name, full path)

    for (bmp, full) in &bitmaps {
        let name = splitext(bmp).0.to_lowercase();
        let clean = strip_num_suffix(&name);
        let clean = if clean.chars().count() > 30 {
            strip_to_last_sep(&clean)
        } else {
            clean
        };
        // A deliberate divergence from the Python original: there the FIRST matching
        // key in dict order was taken (collisions startmenu→menu,
        // startpanel→pane, taskitemactive→taskitem); here — the longest one.
        let mut best: Option<(&str, &str)> = None;
        for (part_key, part_type) in PART_MAP {
            if clean.contains(part_key)
                && best.is_none_or(|(bk, _)| part_key.len() > bk.len())
            {
                best = Some((part_key, part_type));
            }
        }
        match best {
            Some((_, part_type)) if part_type.starts_with("window-") => {
                window_parts.push((full.clone(), part_type.to_string()));
            }
            Some((_, part_type)) if part_type.starts_with("plasma-") => {
                plasma_parts.push((full.clone(), part_type.to_string()));
            }
            Some((_, part_type)) => {
                gtk_parts.push((full.clone(), part_type.to_string()));
            }
            None => other_parts.push((bmp.clone(), full.clone())),
        }
    }

    // ── 3. theme-bundle ──
    let rxn_dir = join(work_dir, "rxn");
    let data_dir = join(&rxn_dir, "data");
    let mut bundle: Vec<(&str, &str)> = Vec::new();

    // 3a. Aurorae
    if !window_parts.is_empty() {
        let aurorae_dir = join(&join(&data_dir, "aurorae"), &safe_id);
        io.makedirs(&aurorae_dir);
        for (full, part_type) in &window_parts {
            io.copy_file(full, &join(&aurorae_dir, &format!("{part_type}.png")));
        }
        generate_aurorae_theme(io, &aurorae_dir, &safe_id, &theme_name);
        bundle.push(("theme-window-decoration", "data/aurorae"));
    }

    // 3b. GTK
    if !gtk_parts.is_empty() || !other_parts.is_empty() {
        let gtk_dir = join(&join(&join(&data_dir, "gtk"), &safe_id), "gtk-3.0");
        let assets_dir = join(&gtk_dir, "assets");
        io.makedirs(&assets_dir);
        for (full, part_type) in &gtk_parts {
            io.copy_file(full, &join(&assets_dir, &format!("{part_type}.png")));
        }
        for (bmp, full) in &other_parts {
            io.copy_file(full, &join(&assets_dir, bmp));
        }
        generate_gtk_css(io, &gtk_dir, &assets_dir, &gtk_parts, &theme_name);
        io.write_file(
            &join(&join(&join(&data_dir, "gtk"), &safe_id), "index.theme"),
            &format!(
                "[Desktop Entry]\nType=X-GNOME-Metatheme\nName={theme_name}\nComment=Imported from Windows .msstyles\nEncoding=UTF-8\n\n[X-GNOME-Metatheme]\nGtkTheme={safe_id}\nMetacityTheme={safe_id}\nIconTheme=hicolor\n"
            ),
        );
        bundle.push(("theme-gtk", "data/gtk"));
    }

    // 3c. Plasma Theme
    if !plasma_parts.is_empty() {
        let plasma_dir = join(&join(&data_dir, "plasma"), &safe_id);
        generate_plasma_theme(
            io, &plasma_dir, &safe_id, &theme_name, &plasma_parts,
        );
        bundle.push(("theme-kde-plasma", "data/plasma"));
    }

    // 3d. Color palette
    let head: Vec<String> =
        bitmaps.iter().take(5).map(|(_, full)| full.clone()).collect();
    let colors = extract_dominant_colors(io, &head);
    if !colors.is_empty() {
        let colors_dir = join(&data_dir, "colors");
        io.makedirs(&colors_dir);
        generate_kde_colorscheme(io, &colors_dir, &safe_id, &theme_name, &colors)?;
        bundle.push(("theme-kde-colors", "data/colors"));
    }

    if bundle.is_empty() {
        return Err(verr(tr("Couldn't create a theme from the .msstyles")));
    }

    // Preview collage
    let preview = join(&rxn_dir, "preview.png");
    let preview_srcs: Vec<String> =
        bitmaps.iter().take(6).map(|(_, full)| full.clone()).collect();
    if !preview_srcs.is_empty() {
        let mut argv: Vec<String> = vec!["convert".into()];
        argv.extend(preview_srcs.iter().take(4).cloned());
        argv.extend(
            [
                "+append", "-resize", "620x348^", "-gravity", "center",
                "-extent", "620x348",
            ]
            .iter()
            .map(|s| (*s).to_string()),
        );
        argv.push(preview);
        io.run(argv, 15);
    }

    let mut bundle_map = Map::new();
    for (k, v) in &bundle {
        bundle_map.insert((*k).to_string(), json!(v));
    }
    let manifest = json!({
        "format_version": 3,
        "id": format!("msstyles-{safe_id}"),
        "name": theme_name,
        "version": "1.0",
        "author": "Windows Theme (msstyles)",
        "description": format!(
            "{} | {}",
            tr("Imported from .msstyles"),
            // TRANSLATORS: counts of converted theme parts: %1 window, %2 panel/start
            // menu, %3 GTK, %4 textures.
            trf(
                "%1 window, %2 panel/start, %3 GTK, %4 textures",
                &[&window_parts.len(), &plasma_parts.len(), &gtk_parts.len(), &other_parts.len()],
            )
        ),
        "type": "theme-bundle",
        "tags": ["windows", "xp", "msstyles", "imported"],
        "de_support": ["any"],
        "bundle": bundle_map,
    });
    io.write_file(&join(&rxn_dir, "manifest.json"), &dumps_indent2(&manifest));

    let out_dir = match output_dir {
        Some(o) => o.to_string(),
        None => {
            let d = dirname(source_path);
            if d.is_empty() { ".".to_string() } else { d }
        }
    };
    let rxn_path = join(&out_dir, &format!("msstyles-{safe_id}.rxn"));
    io.pack_rxn(&rxn_dir, &rxn_path);
    Ok(rxn_path)
}

/// Listing of the bitmap directory: (file name, full path).
/// A deliberate divergence from the Python original: there after the 7z
/// fallback the search was NON-recursive (os.listdir), although 7z puts PE
/// resources into subfolders; here after 7z the directory is walked recursively (os.walk).
fn list_bitmap_dir(
    io: &mut dyn ImpIo,
    bitmaps_dir: &str,
    used_7z: bool,
) -> Vec<(String, String)> {
    if used_7z {
        let mut out = Vec::new();
        for (root, _dirs, files) in io.walk(bitmaps_dir) {
            for f in files {
                let full = join(&root, &f);
                out.push((f, full));
            }
        }
        out
    } else {
        io.listdir(bitmaps_dir)
            .into_iter()
            .map(|f| {
                let full = join(bitmaps_dir, &f);
                (f, full)
            })
            .collect()
    }
}

/// `re.sub(r'[_\-]\d+$', '', name)`.
fn strip_num_suffix(name: &str) -> String {
    let b = name.as_bytes();
    let mut i = b.len();
    while i > 0 && b[i - 1].is_ascii_digit() {
        i -= 1;
    }
    if i < b.len() && i > 0 && (b[i - 1] == b'_' || b[i - 1] == b'-') {
        return name[..i - 1].to_string();
    }
    name.to_string()
}

/// `re.sub(r'^.*[_\-]', '', clean)` — greedy up to the LAST separator.
fn strip_to_last_sep(clean: &str) -> String {
    match clean.rfind(['_', '-']) {
        Some(i) => clean[i + 1..].to_string(),
        None => clean.to_string(),
    }
}

/// `MSStyImporter._generate_aurorae_theme` (text 1-to-1).
fn generate_aurorae_theme(
    io: &mut dyn ImpIo,
    aurorae_dir: &str,
    theme_id: &str,
    theme_name: &str,
) {
    let rc_content = format!(
        "[General]\nName={theme_name}\nComment=Imported from Windows XP .msstyles\nAnimation=0\nActiveTextColor=255,255,255\nInactiveTextColor=180,180,180\nUseTextShadow=true\nTextShadowColor=0,0,0\nHaloActive=true\nHaloInactive=false\n\n[Layout]\nTitleEdgeTop=4\nTitleEdgeBottom=2\nTitleEdgeLeft=4\nTitleEdgeRight=4\nTitleBorderBottom=2\nTitleHeight=22\nButtonWidth=22\nButtonHeight=18\nPaddingTop=0\nPaddingBottom=0\nPaddingLeft=0\nPaddingRight=0\n"
    );
    io.write_file(&join(aurorae_dir, &format!("{theme_id}rc")), &rc_content);
}

/// `MSStyImporter._generate_plasma_theme` (`plasma_parts` — full paths).
fn generate_plasma_theme(
    io: &mut dyn ImpIo,
    plasma_dir: &str,
    theme_id: &str,
    theme_name: &str,
    plasma_parts: &[(String, String)],
) {
    io.makedirs(plasma_dir);
    let widgets_dir = join(plasma_dir, "widgets");
    let dialogs_dir = join(plasma_dir, "dialogs");
    let icons_dir = join(plasma_dir, "icons");
    io.makedirs(&widgets_dir);
    io.makedirs(&dialogs_dir);
    io.makedirs(&icons_dir);

    io.write_file(
        &join(plasma_dir, "metadata.desktop"),
        &format!(
            "[Desktop Entry]\nName={theme_name}\nComment=Imported from Windows XP .msstyles\nX-KDE-PluginInfo-Author=Rexon Import\nX-KDE-PluginInfo-Name={theme_id}\nX-KDE-PluginInfo-Version=1.0\nX-KDE-PluginInfo-License=GPL\nX-Plasma-API=5.0\nX-KDE-PluginInfo-Category=\ntype=Service\nServiceTypes=Plasma/Theme\n"
        ),
    );

    for (src, part_type) in plasma_parts {
        let src = src.clone();
        if !io.is_file(&src) {
            continue;
        }
        match part_type.as_str() {
            "plasma-start" => {
                let dst = join(&icons_dir, "start.png");
                io.run(
                    vec![
                        "convert".into(), src.clone(),
                        "-crop".into(), "0x33%+0+0".into(),
                        "+repage".into(), "-resize".into(), "48x48".into(),
                        dst.clone(),
                    ],
                    10,
                );
                if !io.exists(&dst) {
                    io.copy_file(&src, &dst);
                }
            }
            "plasma-panel" => {
                let dst = join(&widgets_dir, "panel-background.png");
                io.copy_file(&src, &dst);
                wrap_png_as_svg(
                    io,
                    &dst,
                    &join(&widgets_dir, "panel-background.svg"),
                    "panel",
                );
            }
            "plasma-start-menu" => {
                io.copy_file(&src, &join(&widgets_dir, "kickoff.png"));
            }
            "plasma-start-banner" => {
                io.copy_file(&src, &join(&widgets_dir, "kickoff-banner.png"));
            }
            "plasma-systray" => {
                io.copy_file(&src, &join(&widgets_dir, "systray.png"));
            }
            "plasma-taskbutton" => {
                let dst = join(&widgets_dir, "tasks.png");
                io.copy_file(&src, &dst);
                wrap_png_as_svg(io, &dst, &join(&widgets_dir, "tasks.svg"), "task");
            }
            "plasma-taskbutton-active" => {
                io.copy_file(&src, &join(&widgets_dir, "tasks-active.png"));
            }
            "plasma-clock" => {
                io.copy_file(&src, &join(&widgets_dir, "clock.png"));
            }
            "plasma-start-logoff" => {
                io.copy_file(&src, &join(&widgets_dir, "logoff.png"));
            }
            _ => {}
        }
    }

    // Dialog background — the first menu/panel texture (no isfile — as in the original)
    for (src, part_type) in plasma_parts {
        if part_type == "plasma-start-menu" || part_type == "plasma-panel" {
            let dst = join(&dialogs_dir, "background.png");
            io.copy_file(src, &dst);
            wrap_png_as_svg(
                io,
                &dst,
                &join(&dialogs_dir, "background.svg"),
                "dialog",
            );
            break;
        }
    }
}

/// `MSStyImporter._wrap_png_as_svg` (SVG text 1-to-1, standard base64).
fn wrap_png_as_svg(io: &mut dyn ImpIo, png_path: &str, svg_path: &str, hint: &str) {
    if !io.is_file(png_path) {
        return;
    }
    let r = io.run(
        vec![
            "identify".into(), "-format".into(), "%w %h".into(),
            png_path.into(),
        ],
        5,
    );
    let (mut w, mut h) = (100i64, 30i64);
    if r.ok {
        // int(parts[0]), int(parts[1]) — the tuple is atomic: any errors
        // (ValueError/IndexError) leave the default values
        let stripped = bytes_strip(&r.stdout);
        let parts = bytes_split_ws(stripped);
        if let (Some(pw), Some(ph)) = (
            parts.first().and_then(|p| py_int_bytes(p)),
            parts.get(1).and_then(|p| py_int_bytes(p)),
        ) {
            w = pw;
            h = ph;
        }
    }
    // the file has just been checked with isfile — executor contract
    let Some(bytes) = io.read_bytes(png_path) else {
        return;
    };
    let b64 = b64encode(&bytes);
    let svg_content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"\n     xmlns:xlink=\"http://www.w3.org/1999/xlink\"\n     width=\"{w}\" height=\"{h}\">\n  <defs>\n    <pattern id=\"bg\" width=\"{w}\" height=\"{h}\" patternUnits=\"userSpaceOnUse\">\n      <image width=\"{w}\" height=\"{h}\"\n             xlink:href=\"data:image/png;base64,{b64}\"/>\n    </pattern>\n  </defs>\n  <!-- {hint} background from Windows XP .msstyles -->\n  <rect id=\"center\" width=\"{w}\" height=\"{h}\" fill=\"url(#bg)\"/>\n</svg>\n"
    );
    io.write_file(svg_path, &svg_content);
}

/// `MSStyImporter._generate_gtk_css`.
fn generate_gtk_css(
    io: &mut dyn ImpIo,
    gtk_dir: &str,
    assets_dir: &str,
    gtk_parts: &[(String, String)],
    theme_name: &str,
) {
    let mut css_lines: Vec<String> = vec![
        format!("/* {theme_name} — imported from Windows XP .msstyles */"),
        "/* Generated by Rexon v5 */\n".to_string(),
    ];

    for (_bmp, part_type) in gtk_parts {
        let asset_name = format!("{part_type}.png");
        match part_type.as_str() {
            "gtk-button" => css_lines.push(format!(
                "button {{\n  background-image: url('assets/{asset_name}');\n  background-size: 100% 100%;\n  border: none;\n  border-radius: 0;\n}}\n"
            )),
            "gtk-menu" => css_lines.push(format!(
                "menu, .menu {{\n  background-image: url('assets/{asset_name}');\n  background-size: cover;\n}}\n"
            )),
            "gtk-scrollbar-thumb" => css_lines.push(format!(
                "scrollbar slider {{\n  background-image: url('assets/{asset_name}');\n  background-size: 100% 100%;\n  border-radius: 0;\n}}\n"
            )),
            "gtk-tab" => css_lines.push(format!(
                "notebook tab {{\n  background-image: url('assets/{asset_name}');\n  background-size: 100% 100%;\n}}\n"
            )),
            "gtk-progressbar" => css_lines.push(format!(
                "progressbar trough progress {{\n  background-image: url('assets/{asset_name}');\n  background-size: 100% 100%;\n}}\n"
            )),
            "gtk-toolbar" => css_lines.push(format!(
                "toolbar, .toolbar {{\n  background-image: url('assets/{asset_name}');\n  background-size: cover;\n}}\n"
            )),
            "gtk-tooltip" => css_lines.push(format!(
                "tooltip, .tooltip {{\n  background-image: url('assets/{asset_name}');\n  background-size: cover;\n  border-radius: 0;\n}}\n"
            )),
            _ => {}
        }
    }

    io.write_file(&join(gtk_dir, "gtk.css"), &css_lines.join("\n"));

    // gtk-4.0 symlinks (try/except: a failure of the first skips the second)
    let gtk4_dir = join(&dirname(gtk_dir), "gtk-4.0");
    io.makedirs(&gtk4_dir);
    if io.symlink(&join(gtk_dir, "gtk.css"), &join(&gtk4_dir, "gtk.css"))
        && io.is_dir(assets_dir)
    {
        io.symlink(assets_dir, &join(&gtk4_dir, "assets"));
    }
}

/// `MSStyImporter._extract_dominant_colors` (`bitmaps` — full paths).
/// A deliberate divergence from the Python original: there f"#{bytes}" gave
/// strings like "#b'336699'" → ValueError in int(..., 16) while generating
/// the scheme; here convert's stdout is decoded as text.
fn extract_dominant_colors(
    io: &mut dyn ImpIo,
    bitmaps: &[String],
) -> Vec<String> {
    let mut colors = Vec::new();
    for full in bitmaps {
        if !io.is_file(full) {
            continue;
        }
        let r = io.run(
            vec![
                "convert".into(), full.clone(),
                "-resize".into(), "1x1!".into(),
                "-format".into(), "%[hex:u]".into(), "info:".into(),
            ],
            5,
        );
        if r.ok && !r.stdout.is_empty() {
            let text = String::from_utf8_lossy(&r.stdout);
            let hex: String = py_strip(&text).chars().take(6).collect();
            if hex.chars().count() >= 6 {
                colors.push(format!("#{hex}"));
            }
        }
    }
    colors
}

/// `MSStyImporter._generate_kde_colorscheme`. Err — ValueError from
/// `int(h[0:2], 16)` (after the color fix it is reachable only with non-hex
/// convert stdout).
fn generate_kde_colorscheme(
    io: &mut dyn ImpIo,
    colors_dir: &str,
    theme_id: &str,
    theme_name: &str,
    colors: &[String],
) -> Result<(), ImpError> {
    let hex_to_rgb = |h: &str| -> Result<String, ImpError> {
        let h = h.trim_start_matches('#');
        let cs: Vec<char> = h.chars().collect();
        let sub = |a: usize, b: usize| -> String {
            cs.iter().skip(a).take(b - a).collect()
        };
        let r = py_int_base16(&sub(0, 2))?;
        let g = py_int_base16(&sub(2, 4))?;
        let b = py_int_base16(&sub(4, 6))?;
        Ok(format!("{r},{g},{b}"))
    };

    let bg = if colors.is_empty() {
        "40,40,40".to_string()
    } else {
        hex_to_rgb(&colors[0])?
    };
    let fg = "255,255,255";
    let accent = if colors.len() > 1 {
        hex_to_rgb(&colors[1])?
    } else {
        "70,130,200".to_string()
    };

    let scheme = format!(
        "[ColorEffects:Disabled]\nColor={bg}\n[ColorEffects:Inactive]\nColor={bg}\n\n[Colors:Window]\nBackgroundNormal={bg}\nForegroundNormal={fg}\n\n[Colors:View]\nBackgroundNormal={bg}\nForegroundNormal={fg}\n\n[Colors:Selection]\nBackgroundNormal={accent}\nForegroundNormal={fg}\n\n[Colors:Button]\nBackgroundNormal={bg}\nForegroundNormal={fg}\nDecorationFocus={accent}\n\n[General]\nName={theme_name}\nColorScheme={theme_id}\n"
    );
    io.write_file(&join(colors_dir, &format!("{theme_id}.colors")), &scheme);
    Ok(())
}

// ============================================================================
// External converters: the set per format and the check verdict (rule #12)
// ============================================================================
//
// Windows appearance importers rely on external programs (cabextract/7z
// unpack, ImageMagick converts, wrestool/icotool extract resources,
// win2xcur/xcursorgen make cursors, ffmpeg handles DreamScene video). None of
// them ships in the base install of the target distributions, and SteamOS
// doesn't even have ImageMagick. So the import has EXACTLY ONE check point —
// the table below: it decides what each step of a format can be done with
// and what to tell the user if there is nothing. Package names are taken
// from `utils::tool_package_name` by distribution family (rule #13: never
// suggest `pacman -S` on Fedora).
//
// Port of this layer from `rexon/extensions/importers.py` (`_IMPORT_TOOLS`,
// `_import_kind`, `_packages_phrase`, `_require_tools`, `_LEGACY_HINTS`,
// `_friendly_error`). Checking whether a program is present is up to the
// executor (`shutil.which`); the core has only the decision and the text.

/// An import step that depends on external programs.
pub struct ToolStep {
    /// What we are doing — substituted into the text "there is no program needed
    /// for {what}". A `msgid` ("unpacking the theme"; in Russian — in the genitive
    /// case: "распаковки темы"), translated by [`require_tools`].
    pub what: &'static str,
    /// Program set variants: ANY variant will do, within a variant ALL programs
    /// are needed.
    pub variants: &'static [&'static [&'static str]],
    /// Whether the step is mandatory: an optional one without programs doesn't
    /// cancel the import — a fallback path works, but a warning stays in the log.
    pub required: bool,
}

const TOOLS_THEMEPACK: &[ToolStep] = &[
    ToolStep {
        what: n_("unpacking the theme"),
        variants: &[&["cabextract"], &["7z"]],
        required: true,
    },
    ToolStep {
        what: n_("preparing images and the cover"),
        variants: &[&["convert"]],
        required: false,
    },
];

const TOOLS_PICTURES: &[ToolStep] = &[ToolStep {
    what: n_("preparing images and the cover"),
    variants: &[&["convert"]],
    required: false,
}];

const TOOLS_DREAMSCENE: &[ToolStep] = &[ToolStep {
    what: n_("re-encoding video wallpapers"),
    variants: &[&["ffmpeg"]],
    required: false,
}];

// Cursors are converted by our own code (`wincursor`) — no external programs needed.
const TOOLS_CURSOR: &[ToolStep] = &[];

/// `.rar`/`.7z` aren't read by our own code: `bsdtar` (libarchive) or `7z` is needed.
const TOOLS_CURSOR_ARCHIVE: &[ToolStep] = &[ToolStep {
    what: n_("unpacking the archive"),
    variants: &[&["bsdtar"], &["7z"]],
    required: true,
}];

const TOOLS_ICON: &[ToolStep] = &[ToolStep {
    what: n_("converting Windows icons"),
    variants: &[&["icotool"], &["convert"]],
    required: true,
}];

/// `.icl`/`.dll`/`.exe` — icons are resources inside a Windows program.
const TOOLS_ICON_PE: &[ToolStep] = &[
    ToolStep {
        what: n_("extracting icons from a Windows file"),
        variants: &[&["wrestool"]],
        required: true,
    },
    ToolStep {
        what: n_("converting Windows icons"),
        variants: &[&["icotool"], &["convert"]],
        required: true,
    },
];

const TOOLS_MSSTYLES: &[ToolStep] = &[
    ToolStep {
        what: n_("extracting images from the visual style"),
        variants: &[&["wrestool"], &["7z"]],
        required: true,
    },
    ToolStep {
        what: n_("preparing images and the cover"),
        variants: &[&["convert"]],
        required: false,
    },
];

/// Import steps for a converter set (`_IMPORT_TOOLS`). An unknown set — empty
/// (nothing to check, like `.get(kind, ())` in the original).
pub fn import_tools(kind: &str) -> &'static [ToolStep] {
    match kind {
        "themepack" => TOOLS_THEMEPACK,
        "theme" => TOOLS_PICTURES,
        "dreamscene" => TOOLS_DREAMSCENE,
        "cursor" => TOOLS_CURSOR,
        "cursor_archive" => TOOLS_CURSOR_ARCHIVE,
        "icon" => TOOLS_ICON,
        "icon_pe" => TOOLS_ICON_PE,
        "msstyles" => TOOLS_MSSTYLES,
        // "gadget" — unpacked with our own ZIP reader, no external programs needed.
        _ => &[],
    }
}

/// Converter set by file extension — port of `_import_kind`. `default` is
/// returned for extensions not in the table.
pub fn import_kind<'a>(path: &str, default: &'a str) -> &'a str {
    match ext_lower(path).as_str() {
        ".themepack" | ".deskthemepack" => "themepack",
        ".theme" => "theme",
        ".dream" | ".wmv" | ".mpeg" | ".mpg" => "dreamscene",
        ".gadget" => "gadget",
        ".msstyles" => "msstyles",
        ".cur" | ".ani" | ".inf" | ".crs" | ".curxptheme" | ".zip" => "cursor",
        ".rar" | ".7z" => "cursor_archive",
        ".icl" | ".dll" | ".exe" => "icon_pe",
        ".ico" | ".iconpackage" | ".ip" | ".7tsp" => "icon",
        _ => default,
    }
}

/// "the “icoutils” package or the “7zip” package" — with THIS distribution's
/// names (port of `_packages_phrase`). A program without a known package is
/// named by its binary.
pub fn packages_phrase(variants: &[&[&str]], family: &str) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(variants.len());
    for variant in variants {
        let names: Vec<String> = variant
            .iter()
            .map(|t| {
                let pkg = crate::utils::tool_package_name(t, family);
                if pkg.is_empty() { (*t).to_string() } else { pkg }
            })
            .collect();
        let listed: Vec<String> = names
            .iter()
            // TRANSLATORS: a package name in quotes inside a sentence.
            .map(|n| crate::i18n::fill(&trc("quoted package name", "“%1”"), &[n]))
            .collect();
        parts.push(if listed.len() == 1 {
            // TRANSLATORS: %1 is a quoted package name: “the “icoutils” package”.
            trf("the %1 package", &[&listed[0]])
        } else {
            // TRANSLATORS: %1 is a list of quoted package names joined with “and”.
            trf("the %1 packages", &[&listed.join(&trc("joining package names", " and "))])
        });
    }
    parts.join(&trc("joining package alternatives", " or "))
}

/// Verdict of the converter check before an import — port of `_require_tools`.
///
/// `have` — whether the program is on the system (`shutil.which`, up to the
/// executor). `Ok(warnings)` — the import is possible; each line is a warning
/// for the log about an optional step without programs. `Err(msg)` — a
/// refusal in the interface language with this distribution's package names
/// (rule #13: neither `FileNotFoundError` nor a bare binary name for the
/// user). Steps go in table order, the refusal is on the FIRST mandatory step
/// that can't be done.
pub fn require_tools(
    kind: &str,
    family: &str,
    have: &dyn Fn(&str) -> bool,
) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();
    for step in import_tools(kind) {
        if step.variants.iter().any(|v| v.iter().all(|t| have(t))) {
            continue;
        }
        let phrase = packages_phrase(step.variants, family);
        if !step.required {
            warnings.push(trf(
                // TRANSLATORS: %1 is what the program is needed for (“unpacking the theme”),
                // %2 which packages to install.
                "No programs for %1 — the import will be simplified (%2)",
                &[&tr(step.what), &phrase],
            ));
            continue;
        }
        return Err(trf(
            // TRANSLATORS: continues a sentence, hence lowercase. %1 is what the program is
            // needed for (“unpacking the theme”), %2 which packages to install.
            "there's no program for %1. Install %2.",
            &[&tr(step.what), &phrase],
        ));
    }
    Ok(warnings)
}

/// The import result PLUS warnings about skipped steps — as one text.
///
/// WHY A SEPARATE FUNCTION. The original only put the [`require_tools`]
/// warnings into the log (`log.warning`), i.e. the user never saw them: the
/// theme was installed "simplified" — without a cover, without prepared
/// pictures — and the person read «Installed» and decided that was how it
/// should be. That is a silent failure, and it is fixed the same way for every
/// place where such warnings arise — so there is one joiner for all (rule #12).
///
/// No warnings — the text doesn't change by a single character.
pub fn with_skipped_notes(text: &str, notes: &[String]) -> String {
    if notes.is_empty() {
        return text.to_string();
    }
    let mut out = text.to_string();
    for n in notes {
        out.push_str("\n⚠️ ");
        out.push_str(n);
    }
    out
}

/// Arch hints baked into import error texts: `msgid` → binary.
///
/// ⚠️ The text is read by code ([`friendly_error_text`] looks for it in the
/// error), so the key is the `msgid`, and both it and its translation in the
/// interface language are searched for: the error is assembled right here,
/// by the same [`legacy_hint`].
const HINT_CABEXTRACT: &str = n_("Install it: sudo pacman -S cabextract");
const HINT_ICOUTILS: &str = n_("Install it: sudo pacman -S icoutils");
const HINT_WIN2XCUR: &str = n_("Install it: pip install win2xcur");
const LEGACY_HINTS: &[(&str, &str)] = &[
    (HINT_CABEXTRACT, "cabextract"),
    (HINT_ICOUTILS, "wrestool"),
    (HINT_WIN2XCUR, "win2xcur"),
];

/// A hint for the error text — in the interface language.
fn legacy_hint(key: &str) -> String {
    tr(key)
}

/// Replace the Arch hints in an error text with THIS system's package — port
/// of `_friendly_error`. `None` — nothing to replace (the text is untouched).
pub fn friendly_error_text(text: &str, family: &str) -> Option<String> {
    let mut out = text.to_string();
    let mut changed = false;
    for (key, binary) in LEGACY_HINTS {
        let shown = legacy_hint(key);
        let legacy = if out.contains(shown.as_str()) {
            shown
        } else if out.contains(key) {
            (*key).to_string()
        } else {
            continue;
        };
        let hint = crate::utils::tool_package_hint(binary, family);
        let repl = if hint.is_empty() { tr("The required program is missing") } else { hint };
        out = out.replace(&legacy, &repl);
        changed = true;
    }
    if changed { Some(out) } else { None }
}

#[cfg(test)]
mod skipped_notes_tests {
    use super::*;

    /// A skipped optional step must become a VISIBLE warning with THIS
    /// distribution's package name, not a line in the log.
    ///
    /// Guard against regression: the original (`_require_tools`) put such
    /// messages into `log.warning`, and the user never learned the theme was
    /// installed simplified — without a cover and prepared pictures.
    #[test]
    fn missing_optional_converter_speaks_up_with_local_package_name() {
        // The mandatory step is doable, the optional one (ImageMagick) is not.
        let have = |tool: &str| tool == "cabextract";
        let warnings = require_tools("themepack", "debian", &have)
            .expect("обязательный шаг выполним — импорт не отменяется");
        assert_eq!(warnings.len(), 1, "предупреждение потерялось: {warnings:?}");
        assert!(
            warnings[0].contains("simplified"),
            "человеку не сказано, что шаг пропущен: {}",
            warnings[0]
        );
        assert!(
            warnings[0].contains("imagemagick"),
            "не назван пакет этого дистрибутива: {}",
            warnings[0]
        );

        let shown = with_skipped_notes("Тема", &warnings);
        assert!(shown.starts_with("Тема"), "имя темы потерялось: {shown}");
        assert!(shown.contains(&warnings[0]), "предупреждение не дошло: {shown}");
        // No warnings — the text doesn't change by a single character.
        assert_eq!(with_skipped_notes("Тема", &[]), "Тема");
    }

    /// A MANDATORY program is missing — a refusal in the interface language with
    /// the package name, not an import attempt that quietly produces an empty theme.
    #[test]
    fn missing_required_converter_refuses_before_any_side_effect() {
        let refusal = require_tools("themepack", "arch", &|_| false)
            .expect_err("без распаковщика импорт невозможен");
        assert!(refusal.contains("unpacking the theme"), "{refusal}");
        assert!(
            refusal.contains("cabextract") || refusal.contains("7z"),
            "не названо, что ставить: {refusal}"
        );
    }
}

#[cfg(test)]
mod cursor_tests {
    //! Windows cursor import on a real FS (temporary directories) with
    //! substitute bytes: roles from the descriptor, all frames, originals in
    //! `.rxn`, synonym links, re-conversion.
    use super::*;
    use crate::imp_live::LiveImpIo;
    use crate::wincursor::testbytes::{ani, cur32};
    use std::path::{Path, PathBuf};

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(name: &str) -> Tmp {
            let p = std::env::temp_dir()
                .join(format!("rexon-curtest-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
        fn path(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }
        fn put(&self, rel: &str, data: &[u8]) {
            let p = self.path(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, data).unwrap();
        }
        fn s(&self, rel: &str) -> String {
            self.path(rel).to_string_lossy().into_owned()
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Animation: 3 frames, seq 0,1,2,1, delays 6/12/3/60 sixtieths.
    fn ani3() -> Vec<u8> {
        let fr = [cur32(32, 1, 2, 10), cur32(32, 3, 4, 20), cur32(32, 5, 6, 30)];
        ani(&fr, 10, Some(&[6, 12, 3, 60]), Some(&[0, 1, 2, 1]), 4)
    }

    const BLUE_INF: &str = "[DefaultInstall]\r\nAddReg = Scheme.Reg\r\n[Scheme.Reg]\r\n\
HKCU,\"Control Panel\\Cursors\\Schemes\",\"%SCHEME_NAME%\",,\"%10%\\%CUR_DIR%\\%pointer%,%10%\\%CUR_DIR%\\%help%,%10%\\%CUR_DIR%\\%work%,%10%\\%CUR_DIR%\\%busy%,%10%\\%CUR_DIR%\\%cross%\"\r\n\
[Strings]\r\nCUR_DIR = \"Cursors\\Blue\"\r\nSCHEME_NAME = \"Blue Trans\"\r\npointer = \"Standard.ani\"\r\nhelp = \"Help.cur\"\r\n\
work = \"Work in Progress.ani\"\r\nbusy = \"\"\r\ncross = \"Pointer.ani\"\r\n";

    fn unpack(rxn: &str, into: &Path) {
        crate::ext_utils::safe_zip_extract(Path::new(rxn), into).unwrap();
    }

    fn manifest(dir: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap()
    }

    fn xcur(path: &Path) -> Vec<wincursor::XcurImageInfo> {
        wincursor::read_xcursor(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn folder_with_inf_keeps_roles_frames_originals_and_links() {
        let t = Tmp::new("inf");
        t.put("set/install.inf", BLUE_INF.as_bytes());
        t.put("set/Standard.ani", &ani3());
        t.put("set/Help.cur", &cur32(32, 7, 7, 1));
        t.put("set/Work In Progress.ani", &ani3());
        t.put("set/Pointer.ani", &cur32(32, 15, 15, 2));
        t.put("set/README.txt", b"by someone");
        t.put("set/junk.exe", b"MZ");
        let mut io = LiveImpIo::new(None);
        let out = t.s("out");
        std::fs::create_dir_all(&out).unwrap();
        let src = CursorSource {
            site: "rw-designer".into(),
            url: "https://www.rw-designer.com/cursor-set/x".into(),
            author: "Автор".into(),
            ..Default::default()
        };
        let rxn = cursor_import_with_source(&mut io, &t.s("set"), &src, Some(&out)).unwrap();
        assert!(io.io_err.is_none(), "{:?}", io.io_err);
        assert!(rxn.ends_with("/wincur-blue-trans.rxn"), "{rxn}");

        // In the archive synonyms are links, not copies.
        let f = std::fs::File::open(&rxn).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        let link = z.by_name("data/blue-trans/cursors/left_ptr").unwrap();
        assert!(link.is_symlink(), "синоним стал копией");
        drop(link);

        let u = t.path("u");
        unpack(&rxn, &u);
        let m = manifest(&u);
        assert_eq!(m["source"], "windows");
        assert_eq!(m["source_site"], "rw-designer");
        assert_eq!(m["source_url"], "https://www.rw-designer.com/cursor-set/x");
        assert_eq!(m["author"], "Автор");
        assert_eq!(m["name"], "Blue Trans");
        let w = &m["windows_cursors"];
        assert_eq!(w["descriptor"], "install.inf");
        assert_eq!(w["roles"]["Arrow"], "Standard.ani");
        assert_eq!(w["roles"]["Crosshair"], "Pointer.ani");
        assert_eq!(w["roles"]["AppStarting"], "Work In Progress.ani");
        assert_eq!(w["animated"], json!(["Arrow", "AppStarting"]));
        assert_eq!(w["frames"]["Arrow"], 4);
        assert_eq!(
            w["originals"],
            json!(["Help.cur", "Pointer.ani", "README.txt", "Standard.ani", "Work In Progress.ani", "install.inf"])
        );
        assert_eq!(std::fs::read(u.join("originals/Standard.ani")).unwrap(), ani3());
        assert!(!u.join("originals/junk.exe").exists());
        assert!(u.join("preview.png").is_file());

        let cur = u.join("data/blue-trans/cursors");
        // Arrow: all 4 steps, delays in ms, hotspots of the frames.
        let imgs = xcur(&cur.join("default"));
        let at32: Vec<_> = imgs.iter().filter(|i| i.nominal == 32).map(|i| (i.hot_x, i.hot_y, i.delay)).collect();
        assert_eq!(at32, vec![(1, 2, 100), (3, 4, 200), (5, 6, 50), (3, 4, 1000)]);
        // A synonym after unpacking is again a link to the main file.
        assert_eq!(std::fs::read_link(cur.join("left_ptr")).unwrap(), Path::new("default"));
        // Crosshair — Pointer.ani per .inf, not per file name.
        assert_eq!(xcur(&cur.join("crosshair"))[1].hot_x, 15);
        // "Busy" is empty in the set — its names point to "working in background".
        assert_eq!(std::fs::read_link(cur.join("wait")).unwrap(), Path::new("progress"));
        let theme = std::fs::read_to_string(u.join("data/blue-trans/index.theme")).unwrap();
        assert!(theme.contains("Name=Blue Trans\n"), "{theme}");
        assert!(!theme.contains("Inherits=default"), "{theme}");
        // The installed theme's tile recognises «🪟 From Windows» by this line.
        assert!(theme.contains("\nX-Rexon-Source=windows\n"), "{theme}");
    }

    #[test]
    fn zip_with_size_variants_takes_regular_and_crs_roles() {
        let t = Tmp::new("zip");
        let mut zipped = Vec::new();
        {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut zipped));
            let o = zip::write::SimpleFileOptions::default();
            use std::io::Write;
            for (name, data) in [
                ("Pack/Large/pointer.cur", cur32(64, 2, 2, 0)),
                ("Pack/Large/install.inf", b"[Strings]\npointer=\"pointer.cur\"\n".to_vec()),
                ("Pack/Regular/pointer.cur", cur32(32, 1, 1, 0)),
                ("Pack/Regular/hand.cur", cur32(32, 0, 31, 0)),
                ("Pack/Regular/link.cur", cur32(32, 9, 0, 0)),
                ("Pack/Regular/set.crs", b"[Arrow]\r\nPath=pointer.cur\r\n[NWPen]\r\nPath=hand.cur\r\n[Hand]\r\nPath=link.cur\r\n".to_vec()),
            ] {
                z.start_file(name, o).unwrap();
                z.write_all(&data).unwrap();
            }
            z.finish().unwrap();
        }
        t.put("Neon Pack.zip", &zipped);
        let mut io = LiveImpIo::new(None);
        assert!(cursor_can_import(&mut io, &t.s("Neon Pack.zip")));
        assert!(windows_theme_can_import(&mut io, &t.s("Neon Pack.zip")));
        let rxn = windows_theme_import_to_rxn(&mut io, &t.s("Neon Pack.zip"), None).unwrap();
        assert_eq!(rxn, t.s("wincur-neon-pack.rxn"));
        let u = t.path("u");
        unpack(&rxn, &u);
        let m = manifest(&u);
        assert_eq!(m["windows_cursors"]["descriptor"], "set.crs");
        assert_eq!(m["windows_cursors"]["roles"]["NWPen"], "hand.cur");
        let cur = u.join("data/neon-pack/cursors");
        assert_eq!(xcur(&cur.join("pencil"))[1].hot_y, 31);
        assert_eq!(xcur(&cur.join("pointer"))[1].hot_x, 9);
        // Regular (32 px) — without the 64/96 HiDPI sizes.
        assert!(xcur(&cur.join("default")).iter().all(|i| i.nominal <= 48));
    }

    #[test]
    fn foreign_zips_are_not_cursor_sets() {
        let t = Tmp::new("foreign");
        let mk = |names: &[&str]| {
            let mut v = Vec::new();
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut v));
            for n in names {
                z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            }
            z.finish().unwrap();
            v
        };
        t.put("photos.zip", &mk(&["a.png"]));
        t.put("renamed.zip", &mk(&["manifest.json", "originals/x.cur"]));
        t.put("broken.zip", b"not a zip");
        let mut io = LiveImpIo::new(None);
        for f in ["photos.zip", "renamed.zip", "broken.zip", "missing.zip"] {
            assert!(!windows_theme_can_import(&mut io, &t.s(f)), "{f}");
        }
    }

    #[test]
    fn single_ani_becomes_animated_arrow_and_retranslates() {
        crate::i18n::set_language(Some("en"));
        let t = Tmp::new("single");
        t.put("dl/Rammstein.ani", &ani3());
        t.put("dl/other.cur", &cur32(32, 0, 0, 0));
        let mut io = LiveImpIo::new(None);
        let rxn = cursor_import_to_rxn(&mut io, &t.s("dl/Rammstein.ani"), "", Some(&t.s("dl"))).unwrap();
        let u = t.path("u");
        unpack(&rxn, &u);
        let m = manifest(&u);
        assert_eq!(m["name"], "Rammstein");
        assert_eq!(m["author"], "Wehrwolfmann", "автор из IART");
        assert_eq!(m["windows_cursors"]["originals"], json!(["Rammstein.ani"]));
        assert_eq!(xcur(&u.join("data/rammstein/cursors/default")).len(), 12);

        // Re-conversion from the originals: the same id, the same roles and frames.
        std::fs::create_dir_all(t.path("again")).unwrap();
        let rxn2 = cursor_retranslate_dir(&mut io, &u.to_string_lossy(), &t.s("again")).unwrap();
        assert_eq!(rxn2, t.s("again/wincur-rammstein.rxn"));
        let u2 = t.path("u2");
        unpack(&rxn2, &u2);
        let m2 = manifest(&u2);
        assert_eq!(m2["id"], m["id"]);
        assert_eq!(m2["author"], "Wehrwolfmann");
        assert_eq!(m2["windows_cursors"]["roles"], m["windows_cursors"]["roles"]);
        assert_eq!(
            std::fs::read(u2.join("data/rammstein/cursors/default")).unwrap(),
            std::fs::read(u.join("data/rammstein/cursors/default")).unwrap()
        );
        // A theme without originals can't be re-converted — a refusal, not a panic.
        std::fs::remove_dir_all(u.join("originals")).unwrap();
        let e = cursor_retranslate_dir(&mut io, &u.to_string_lossy(), &t.s("again")).unwrap_err();
        assert_eq!(e.message, "This cursor theme has no Windows originals to convert again");
    }

    #[test]
    fn symlinks_and_garbage_in_a_set_are_not_read() {
        let t = Tmp::new("links");
        t.put("secret.txt", b"secret");
        t.put("set/pointer.cur", &cur32(32, 0, 0, 0));
        t.put("set/busy.ani", b"RIFF\x04\0\0\0ACON");
        std::os::unix::fs::symlink(t.path("secret.txt"), t.path("set/readme.txt")).unwrap();
        std::os::unix::fs::symlink(t.path("secret.txt"), t.path("set/help.cur")).unwrap();
        let mut io = LiveImpIo::new(None);
        let rxn = cursor_import_to_rxn(&mut io, &t.s("set"), "", Some(&t.s(""))).unwrap();
        let u = t.path("u");
        unpack(&rxn, &u);
        let m = manifest(&u);
        assert_eq!(m["windows_cursors"]["originals"], json!(["busy.ani", "pointer.cur"]));
        assert_eq!(m["windows_cursors"]["roles"], json!({"Arrow": "pointer.cur"}));
        let skipped = m["windows_cursors"]["skipped"].as_array().unwrap();
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].as_str().unwrap().starts_with("busy.ani: "), "{skipped:?}");
    }

    #[test]
    fn nothing_readable_is_refused_in_plain_words() {
        crate::i18n::set_language(Some("en"));
        let t = Tmp::new("bad");
        t.put("set/pointer.cur", b"garbage");
        let mut io = LiveImpIo::new(None);
        let e = cursor_import_to_rxn(&mut io, &t.s("set"), "", Some(&t.s(""))).unwrap_err();
        assert!(
            e.message.starts_with("None of the cursors could be read — the files are damaged or aren't Windows cursors: pointer.cur: "),
            "{}",
            e.message
        );
        let e = cursor_import_to_rxn(&mut io, &t.s("nothing"), "", None).unwrap_err();
        assert_eq!(e.message, "Unsupported format: ");
    }

    #[test]
    fn rar_and_7z_need_an_unpacker_and_say_so() {
        let refusal = require_tools(import_kind("/x/set.rar", "cursor"), "arch", &|_| false).unwrap_err();
        assert!(refusal.contains("unpacking the archive"), "{refusal}");
        assert!(require_tools(import_kind("/x/set.zip", "cursor"), "arch", &|_| false).unwrap().is_empty());
        assert!(require_tools(import_kind("/x/a.ani", "cursor"), "arch", &|_| false).unwrap().is_empty());
    }

    /// `.7z` with a real unpacker — only where it exists.
    #[test]
    fn seven_zip_archive_is_unpacked_when_a_tool_exists() {
        let mut io = LiveImpIo::new(None);
        if !io.which("7z") {
            eprintln!("НЕ ПРОВЕРЕНО: нет 7z для сборки архива");
            return;
        }
        let t = Tmp::new("7z");
        t.put("src/Set/pointer.cur", &cur32(32, 4, 4, 0));
        t.put("src/Set/work.ani", &ani3());
        let ok = std::process::Command::new("7z")
            .args(["a", "-bd", "-y"])
            .arg(t.path("set.7z"))
            .arg(t.path("src/Set"))
            .stdout(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "7z a");
        assert!(cursor_can_import(&mut io, &t.s("set.7z")));
        let rxn = cursor_import_to_rxn(&mut io, &t.s("set.7z"), "", Some(&t.s(""))).unwrap();
        let u = t.path("u");
        unpack(&rxn, &u);
        let m = manifest(&u);
        assert_eq!(m["windows_cursors"]["animated"], json!(["AppStarting"]));
        assert_eq!(m["name"], "set");
    }

    /// Converting real sets: `REXON_CUR_SETS` (paths separated by `:`) →
    /// `.rxn` in `REXON_CUR_OUT`. Run manually: `cargo test … -- --ignored real_sets`.
    #[test]
    #[ignore = "needs real cursor sets on disk"]
    fn real_sets_from_env() {
        let sets = std::env::var("REXON_CUR_SETS").expect("REXON_CUR_SETS");
        let out = std::env::var("REXON_CUR_OUT").expect("REXON_CUR_OUT");
        std::fs::create_dir_all(&out).unwrap();
        for set in sets.split(':') {
            let mut io = LiveImpIo::new(None);
            let started = std::time::Instant::now();
            let r = cursor_import_to_rxn(&mut io, set, "", Some(&out));
            println!("{set} → {r:?} io_err={:?} за {:?}", io.io_err, started.elapsed());
            assert!(r.is_ok() && io.io_err.is_none());
        }
    }

    // ── Scope of converter fixes ──

    #[test]
    fn every_converter_bump_names_whom_it_touches() {
        // Version bumped — a table row is mandatory, versions ascending.
        let versions: Vec<u32> = CURSOR_CONVERTER_CHANGES.iter().map(|(v, _)| *v).collect();
        assert!(versions.windows(2).all(|w| w[0] < w[1]), "{versions:?}");
        assert_eq!(versions.last().copied(), Some(CURSOR_CONVERTER_VERSION));
        // Current themes (version 2) have no reason to be re-converted.
        let any = wincursor::CursorTraits { animated: true, hotspot_off_center: true, paletted: true };
        assert!(!cursor_retranslate_due(CURSOR_CONVERTER_VERSION, CURSOR_CONVERTER_VERSION, CURSOR_CONVERTER_CHANGES, &any));
    }

    #[test]
    fn retranslate_only_where_a_fix_applies() {
        use wincursor::CursorTraits as T;
        // Fake: version 3 fixes only animated ones.
        let changes = [(2, CursorFixScope::All), (3, CursorFixScope::Animated)];
        let animated = T { animated: true, ..T::default() };
        let still = T { hotspot_off_center: true, paletted: true, ..T::default() };
        assert!(cursor_retranslate_due(2, 3, &changes, &animated));
        assert!(!cursor_retranslate_due(2, 3, &changes, &still));
        // From version 1 — through "all cursors" of version 2.
        assert!(cursor_retranslate_due(1, 3, &changes, &still));
        // Already current or newer — no.
        assert!(!cursor_retranslate_due(3, 3, &changes, &animated));
        assert!(!cursor_retranslate_due(4, 3, &changes, &animated));
        // A fix newer than the current version doesn't count.
        assert!(!cursor_retranslate_due(2, 2, &changes, &animated));
        // Other scopes.
        assert!(CursorFixScope::HotspotOffCenter.touches(&still));
        assert!(CursorFixScope::Paletted.touches(&still));
        assert!(!CursorFixScope::Animated.touches(&still));
        // Set traits — from the bytes of the originals, junk doesn't count.
        let set = cursor_set_traits(&[cur32(32, 16, 16, 0), ani3(), b"junk".to_vec()]);
        assert!(set.animated && set.hotspot_off_center && !set.paletted);
    }

    #[test]
    fn converter_version_is_read_and_written_in_the_manifest() {
        let mut m = json!({"type": "theme-cursors", "source": "windows",
                           "windows_cursors": {"converter": 2, "roles": {"Arrow": "a.cur"}}});
        assert_eq!(cursor_manifest_converter(&m), Some(2));
        set_cursor_manifest_converter(&mut m, 3);
        assert_eq!(cursor_manifest_converter(&m), Some(3));
        assert_eq!(m["windows_cursors"]["roles"], json!({"Arrow": "a.cur"}));
        // No field — converted before versions existed.
        assert_eq!(cursor_manifest_converter(&json!({"type": "theme-cursors", "source": "windows"})), Some(1));
        // Not from Windows — none of our business.
        assert_eq!(cursor_manifest_converter(&json!({"type": "theme-cursors"})), None);
        assert_eq!(cursor_manifest_converter(&json!({"type": "theme-icons", "source": "windows"})), None);
    }

}

#[cfg(test)]
mod slug_tests {
    use super::{icon_set_dir, slug60};

    #[test]
    fn slug60_latin_unchanged() {
        assert_eq!(slug60("The Witcher 3 — Geralt Theme"), "the-witcher-3-geralt-theme");
    }

    /// A set from Windows named like the shared `hicolor` gets its own folder.
    #[test]
    fn imported_icon_set_never_takes_shared_name() {
        assert_eq!(icon_set_dir(slug60("HiColor")), "hicolor-imported");
        assert_eq!(icon_set_dir("locolor".into()), "locolor-imported");
        assert_eq!(icon_set_dir(slug60("Aero Icons")), "aero-icons");
    }

    #[test]
    fn slug60_cyrillic_transliterated() {
        assert_eq!(slug60("Аэро Синяя"), "aero-sinyaya");
        assert_eq!(slug60("Щедрий вечір"), "shchedriy-vechir");
        assert_eq!(slug60("Windows 7 Объёмная"), "windows-7-obyomnaya");
    }

    #[test]
    fn slug60_no_latin_gets_stable_hash() {
        let a = slug60("和风");
        assert!(a.starts_with("theme-") && a.len() == "theme-".len() + 8, "{a}");
        assert_eq!(a, slug60("和风"));
        assert_ne!(a, slug60("風"));
        assert_eq!(slug60("   "), "");
    }
    /// The package's wallpapers — per `.theme` itself: `Wallpaper=` and slideshow
    /// lines; Windows paths with variables and quotes are reduced to the file name.
    #[test]
    fn themepack_wallpaper_names_come_from_theme_file() {
        let theme = "[Control Panel\\Desktop]\r\nWallpaper=%SystemRoot%\\Web\\Wallpaper\\Img0.JPG\r\n\
                     TileWallpaper=0\r\n[Slideshow]\r\nInterval=1800000\r\n\
                     Item0Path=DesktopBackground\\img0.jpg\r\nItem1Path=\"DesktopBackground\\b 2.png\"\r\n";
        assert_eq!(crate::importers::themepack_wallpaper_names(theme), vec!["img0.jpg".to_string(), "b 2.png".to_string()]);
        assert!(crate::importers::themepack_wallpaper_names("[Theme]\r\nDisplayName=X\r\n").is_empty());
    }
}
