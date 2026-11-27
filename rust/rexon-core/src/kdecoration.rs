//! Built-in KDE window decorations — KDecoration modules (Breeze, Oxygen,
//! third-party ones like Klassy) and QML themes of the Aurorae engine (Plastik).
//!
//! The "Window decorations" section (114) used to see only Aurorae SVG themes
//! from `aurorae/themes`. KDE's default decoration is the Breeze module, not an
//! Aurorae theme, so a person with KDE's default look had nothing highlighted
//! in the grid, and there was no way to get Breeze back after an Aurorae theme.
//!
//! What the "Window Decorations" window of Plasma 6.8 shows (measured on this
//! machine): all modules of the `org.kde.kdecoration3` namespace (Plasma 6.0–6.2
//! — `org.kde.kdecoration2`) in the Qt plugin directories. A module without
//! `"themes": true` in its metadata section is a single decoration: `kwinrc
//! [org.kde.kdecoration2]` gets `library=<module id>` and `theme=<module
//! name>`. An engine module (`"themes": true`) is not shown as a decoration
//! itself — its themes are shown:
//!   * `org.kde.kwin.aurorae` — QML themes, `KWin/Decoration` packages in
//!     `<data>/kwin/decorations/<id>`; `theme=<package id>` (Plastik);
//!   * `org.kde.kwin.aurorae.v2` (appeared in Plasma 6.8) — SVG themes
//!     `aurorae/themes/<name>` with `metadata.desktop`;
//!     `theme=__aurorae__svg__<name>`. Previously SVG themes went under the
//!     first module — so the `library` of an SVG theme is chosen by which module
//!     is installed ([`svg_library`]).
//! Themes of other engines are listed only by the module's own code — we do not
//! load it, such modules are skipped.
//!
//! A Qt plugin's metadata is embedded in the `.so` itself: the ELF note
//! `qt-project!` (Qt ≥ 6.3) or the `QTMETADATA !` marker (earlier) — four
//! header bytes and a CBOR map where key 4 holds the same JSON as
//! `KPluginMetaData`. Parsing is our own, without loading the module:
//! [`qt_plugin_metadata`].
//!
//! The tile id is `local:aurorae:<library>/<theme>`: an Aurorae theme directory
//! name never contains a slash, so it is not confused with SVG tiles
//! (`local:aurorae:<directory>`) ([`parse_entry`]).

use serde_json::{Map, Value};

use crate::i18n::tr;
use crate::models::ThemeItem;

/// The `kwinrc` group where KWin keeps the decoration choice (the name has not
/// changed since KDecoration2).
pub const KWINRC_GROUP: &str = "org.kde.kdecoration2";
/// KDE's default decoration (`kwindecorationsettings.kcfg`).
pub const DEFAULT_LIBRARY: &str = "org.kde.breeze";
/// The Aurorae theme engine: QML themes, and before Plasma 6.8 SVG ones too.
pub const AURORAE_V1: &str = "org.kde.kwin.aurorae";
/// The Aurorae SVG theme engine of Plasma 6.8.
pub const AURORAE_V2: &str = "org.kde.kwin.aurorae.v2";
/// Prefix of an Aurorae SVG theme in `theme=`.
pub const SVG_PREFIX: &str = "__aurorae__svg__";
/// Namespaces of decoration modules: Plasma 6.3+ and the earlier one.
pub const PLUGIN_NAMESPACES: &[&str] = &["org.kde.kdecoration3", "org.kde.kdecoration2"];
/// Data subdirectory with the packages of the Aurorae engine's QML themes.
pub const QML_PACKAGE_DIR: &str = "kwin/decorations";

/// One decoration in the grid: what to write to `kwinrc` and what to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoration {
    /// `library=` — the module id.
    pub library: String,
    /// `theme=` — the module name (that is how KDE writes it) or the QML package id.
    pub theme: String,
    /// An engine theme (for a module without themes, the choice is by `library` alone).
    pub engine_theme: bool,
    /// The default theme of its engine (`defaultTheme`): it is enabled when
    /// `theme=` is empty.
    pub engine_default: bool,
    /// Name in the interface language.
    pub name: String,
    /// Description in the interface language (empty — none).
    pub description: String,
    /// Module file or package directory.
    pub path: String,
    /// Lies in the user's directory.
    pub user: bool,
}

impl Decoration {
    /// Tile name after `local:aurorae:`.
    pub fn entry(&self) -> String {
        entry_id(&self.library, &self.theme)
    }

    /// Whether this decoration is enabled when `current` = `library/theme` from
    /// `kwinrc` ([`current`]).
    pub fn is_current(&self, current: &str) -> bool {
        let Some((library, theme)) = parse_entry(current) else { return false };
        if library != self.library {
            return false;
        }
        // A module without themes draws itself with any `theme=`.
        !self.engine_theme || theme == self.theme || (theme.is_empty() && self.engine_default)
    }
}

/// `library/theme` — the tile name of a built-in decoration.
pub fn entry_id(library: &str, theme: &str) -> String {
    format!("{library}/{theme}")
}

/// `(library, theme)` from a tile name; `None` — this is an Aurorae SVG theme
/// (a directory name).
pub fn parse_entry(entry: &str) -> Option<(&str, &str)> {
    entry.split_once('/').filter(|(library, _)| !library.is_empty())
}

/// What is enabled in `kwinrc` now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Current {
    /// An Aurorae SVG theme — a directory name.
    Svg(String),
    /// A built-in decoration — `library/theme`.
    Builtin(String),
    /// Cannot be determined.
    Unknown,
}

/// The current decoration from the values of `kwinrc [org.kde.kdecoration2]`.
///
/// `library` — `None` when the key was not read (nothing to read it with): then
/// as before, by `theme=` alone. An empty `library` is the KDE default
/// (Breeze). An SVG theme counts only under the Aurorae engine: under Breeze
/// `theme=__aurorae__…` is left over from a previous choice, and KWin still
/// draws Breeze.
pub fn current(library: Option<&str>, theme: Option<&str>) -> Current {
    let theme = theme.map(str::trim);
    let Some(library) = library.map(str::trim) else {
        return match theme.and_then(|t| t.strip_prefix(SVG_PREFIX)) {
            Some(dir) => Current::Svg(dir.to_string()),
            None => Current::Unknown,
        };
    };
    let library = if library.is_empty() { DEFAULT_LIBRARY } else { library };
    let theme = theme.unwrap_or("");
    if library == AURORAE_V1 || library == AURORAE_V2 {
        if let Some(dir) = theme.strip_prefix(SVG_PREFIX) {
            return Current::Svg(dir.to_string());
        }
    }
    Current::Builtin(entry_id(library, theme))
}

/// `library` for an Aurorae SVG theme: the v2 module if present (Plasma 6.8+),
/// otherwise v1. `installed` — ids of the installed decoration modules.
pub fn svg_library<S: AsRef<str>>(installed: &[S]) -> &'static str {
    if installed.iter().any(|id| id.as_ref() == AURORAE_V2) {
        AURORAE_V2
    } else {
        AURORAE_V1
    }
}

/// What to write to `kwinrc` for tile `entry`: `(library, theme)`.
/// `svg_library` — [`svg_library`] of this machine.
pub fn choice(entry: &str, svg_library: &str) -> (String, String) {
    match parse_entry(entry) {
        Some((library, theme)) => (library.to_string(), theme.to_string()),
        None => (svg_library.to_string(), format!("{SVG_PREFIX}{entry}")),
    }
}

/// argv of the decoration choice writes: first `library`, then `theme`.
pub fn write_argv(entry: &str, svg_library: &str) -> [Vec<String>; 2] {
    let (library, theme) = choice(entry, svg_library);
    let kwrite = |key: &str, value: String| {
        vec![
            "kwriteconfig6".to_string(),
            "--file".to_string(),
            "kwinrc".to_string(),
            "--group".to_string(),
            KWINRC_GROUP.to_string(),
            "--key".to_string(),
            key.to_string(),
            value,
        ]
    };
    [kwrite("library", library), kwrite("theme", theme)]
}

// ── Qt plugin metadata ──────────────────────────────────────────────────

/// A Qt plugin's metadata (what `KPluginMetaData::rawData` returns) from the
/// bytes of its file; `None` — there is no metadata or it is broken.
pub fn qt_plugin_metadata(bytes: &[u8]) -> Option<Value> {
    let cbor = note_payload(bytes).or_else(|| legacy_payload(bytes))?;
    // Four header bytes: format version, Qt major and minor, requirements.
    let body = cbor.get(4..)?;
    let mut reader = Cbor { data: body, pos: 0 };
    let top = reader.value(0)?;
    match top {
        Value::Object(map) => map.get("4").cloned().filter(Value::is_object),
        _ => None,
    }
}

/// Body of the ELF note `qt-project!` (Qt ≥ 6.3).
fn note_payload(bytes: &[u8]) -> Option<&[u8]> {
    const NAME: &[u8] = b"qt-project!\0";
    let mut from = 0;
    while let Some(off) = find(&bytes[from..], NAME) {
        let pos = from + off;
        from = pos + 1;
        if pos < 12 {
            continue;
        }
        let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        if word(pos - 12) != NAME.len() {
            continue;
        }
        let size = word(pos - 8);
        let start = pos + NAME.len(); // 12 — already a multiple of four
        if let Some(desc) = bytes.get(start..start.checked_add(size)?) {
            return Some(desc);
        }
    }
    None
}

/// Data after the `QTMETADATA !` marker (Qt before 6.3).
fn legacy_payload(bytes: &[u8]) -> Option<&[u8]> {
    const MAGIC: &[u8] = b"QTMETADATA !";
    let pos = find(bytes, MAGIC)?;
    bytes.get(pos + MAGIC.len()..)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Reading CBOR (RFC 8949) exactly as far as Qt writes it into metadata:
/// numbers, strings, arrays, maps (including indefinite-length ones), tags,
/// true/false/null, floating-point numbers. Numeric keys become strings.
struct Cbor<'a> {
    data: &'a [u8],
    pos: usize,
}

const BREAK: u8 = 0xff;
const MAX_DEPTH: usize = 64;

impl Cbor<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.pos.checked_add(n)?;
        let out = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }

    /// The header argument; `None` for "indefinite length" — separately.
    fn arg(&mut self, info: u8) -> Option<Option<u64>> {
        Some(Some(match info {
            0..=23 => u64::from(info),
            24 => u64::from(self.byte()?),
            25 => u64::from(u16::from_be_bytes(self.take(2)?.try_into().ok()?)),
            26 => u64::from(u32::from_be_bytes(self.take(4)?.try_into().ok()?)),
            27 => u64::from_be_bytes(self.take(8)?.try_into().ok()?),
            31 => return Some(None),
            _ => return None,
        }))
    }

    fn at_break(&mut self) -> bool {
        if self.data.get(self.pos) == Some(&BREAK) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn text(&mut self, len: Option<u64>, major: u8) -> Option<Vec<u8>> {
        match len {
            Some(n) => Some(self.take(usize::try_from(n).ok()?)?.to_vec()),
            None => {
                let mut out = Vec::new();
                while !self.at_break() {
                    let b = self.byte()?;
                    if b >> 5 != major {
                        return None;
                    }
                    let n = self.arg(b & 0x1f)??;
                    out.extend_from_slice(self.take(usize::try_from(n).ok()?)?);
                }
                Some(out)
            }
        }
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        let b = self.byte()?;
        let (major, info) = (b >> 5, b & 0x1f);
        if major == 7 {
            return match info {
                20 => Some(Value::Bool(false)),
                21 => Some(Value::Bool(true)),
                22 | 23 => Some(Value::Null),
                25 => {
                    let h = u16::from_be_bytes(self.take(2)?.try_into().ok()?);
                    Some(float(half(h)))
                }
                26 => Some(float(f64::from(f32::from_be_bytes(self.take(4)?.try_into().ok()?)))),
                27 => Some(float(f64::from_be_bytes(self.take(8)?.try_into().ok()?))),
                0..=19 => Some(Value::Null),
                24 => {
                    self.byte()?;
                    Some(Value::Null)
                }
                _ => None,
            };
        }
        let len = self.arg(info)?;
        match major {
            0 => Some(Value::from(len?)),
            1 => Some(Value::from(-1 - i64::try_from(len?).ok()?)),
            2 => self.text(len, 2).map(|_| Value::Null),
            3 => self.text(len, 3).map(|t| Value::String(String::from_utf8_lossy(&t).into_owned())),
            4 => {
                let mut items = Vec::new();
                match len {
                    Some(n) => {
                        for _ in 0..n {
                            items.push(self.value(depth + 1)?);
                        }
                    }
                    None => {
                        while !self.at_break() {
                            items.push(self.value(depth + 1)?);
                        }
                    }
                }
                Some(Value::Array(items))
            }
            5 => {
                let mut map = Map::new();
                let mut pair = |me: &mut Self| -> Option<()> {
                    let key = match me.value(depth + 1)? {
                        Value::String(s) => s,
                        other => other.to_string(),
                    };
                    let value = me.value(depth + 1)?;
                    map.insert(key, value);
                    Some(())
                };
                match len {
                    Some(n) => {
                        for _ in 0..n {
                            pair(self)?;
                        }
                    }
                    None => {
                        while !self.at_break() {
                            pair(self)?;
                        }
                    }
                }
                Some(Value::Object(map))
            }
            // A tag: the value under it.
            6 => self.value(depth + 1),
            _ => None,
        }
    }
}

fn float(f: f64) -> Value {
    serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
}

fn half(h: u16) -> f64 {
    let exp = (h >> 10) & 0x1f;
    let frac = f64::from(h & 0x3ff);
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    sign * match exp {
        0 => frac * 2f64.powi(-24),
        31 => f64::INFINITY,
        e => (1.0 + frac / 1024.0) * 2f64.powi(i32::from(e) - 15),
    }
}

// ── Decorations from metadata ───────────────────────────────────────────

/// The `KPlugin.<base>` string in language `locale` (keys — as for themes,
/// [`crate::installed_scan::locale_keys`]).
fn localized(kplugin: Option<&Map<String, Value>>, base: &str, locale: &str) -> String {
    let Some(kp) = kplugin else { return String::new() };
    crate::installed_scan::locale_keys(base, locale)
        .iter()
        .find_map(|k| kp.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()))
        .unwrap_or("")
        .to_string()
}

/// The decoration's settings section in the module's metadata.
fn deco_settings(meta: &Value) -> Option<&Map<String, Value>> {
    PLUGIN_NAMESPACES.iter().find_map(|ns| meta.get(*ns).and_then(Value::as_object))
}

/// What kind of decoration module this is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plugin {
    /// A decoration by itself.
    Plain(Decoration),
    /// A theme engine: `id`, the default theme (`defaultTheme`, empty — none).
    Engine { id: String, default_theme: String },
}

/// A decoration module from file `path` with metadata `meta` ([`qt_plugin_metadata`]).
pub fn plugin_from_meta(path: &str, meta: &Value, locale: &str) -> Plugin {
    let kplugin = meta.get("KPlugin").and_then(Value::as_object);
    let id = kplugin
        .and_then(|kp| kp.get("Id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            let file = path.rsplit('/').next().unwrap_or(path);
            file.strip_suffix(".so").unwrap_or(file).to_string()
        });
    let settings = deco_settings(meta);
    if settings.and_then(|s| s.get("themes")).and_then(Value::as_bool) == Some(true) {
        let default_theme = settings
            .and_then(|s| s.get("defaultTheme"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        return Plugin::Engine { id, default_theme };
    }
    // That is how KDE writes `theme=`: the module name (in the interface
    // language), and without a name — its id.
    let name = Some(localized(kplugin, "Name", locale)).filter(|s| !s.is_empty()).unwrap_or_else(|| id.clone());
    Plugin::Plain(Decoration {
        library: id,
        theme: name.clone(),
        engine_theme: false,
        engine_default: false,
        name,
        description: localized(kplugin, "Description", locale),
        path: path.to_string(),
        user: false,
    })
}

/// A QML theme of the Aurorae v1 engine from package `dir` with `metadata.json`
/// = `meta`. `None` — the package is not a decoration.
pub fn qml_theme(dir: &str, meta: &Value, default_theme: &str, locale: &str, user: bool) -> Option<Decoration> {
    let structure = meta.get("KPackageStructure").and_then(Value::as_str).unwrap_or("KWin/Decoration");
    if structure != "KWin/Decoration" {
        return None;
    }
    let kplugin = meta.get("KPlugin").and_then(Value::as_object);
    let id = kplugin
        .and_then(|kp| kp.get("Id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| dir.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string());
    if id.is_empty() || id.contains('/') {
        return None;
    }
    let name = Some(localized(kplugin, "Name", locale)).filter(|s| !s.is_empty()).unwrap_or_else(|| id.clone());
    Some(Decoration {
        library: AURORAE_V1.to_string(),
        engine_theme: true,
        engine_default: id == default_theme,
        theme: id,
        name,
        description: localized(kplugin, "Description", locale),
        path: dir.to_string(),
        user,
    })
}

/// What is on disk — the raw material for [`decorations`].
#[derive(Debug, Default, Clone)]
pub struct Found {
    /// (module path, its metadata), in the order of the search directories.
    pub plugins: Vec<(String, Value)>,
    /// (QML package directory, its `metadata.json`, the user's directory).
    pub qml_packages: Vec<(String, Value, bool)>,
}

/// Built-in decorations for the grid (SVG themes are not included — the scan of
/// Aurorae directories shows them) and the ids of all modules found. The first
/// module with a given id wins, as in Qt (directories in search order).
pub fn decorations(found: &Found, locale: &str) -> (Vec<Decoration>, Vec<String>) {
    let mut out = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    let mut qml_default: Option<String> = None;
    for (path, meta) in &found.plugins {
        let plugin = plugin_from_meta(path, meta, locale);
        let id = match &plugin {
            Plugin::Plain(d) => d.library.clone(),
            Plugin::Engine { id, .. } => id.clone(),
        };
        if ids.contains(&id) {
            continue;
        }
        ids.push(id);
        match plugin {
            Plugin::Plain(d) => out.push(d),
            Plugin::Engine { id, default_theme } if id == AURORAE_V1 => qml_default = Some(default_theme),
            Plugin::Engine { .. } => {}
        }
    }
    if let Some(default_theme) = qml_default {
        let mut seen: Vec<String> = Vec::new();
        for (dir, meta, user) in &found.qml_packages {
            if let Some(d) = qml_theme(dir, meta, &default_theme, locale, *user) {
                if !seen.contains(&d.theme) {
                    seen.push(d.theme.clone());
                    out.push(d);
                }
            }
        }
    }
    (out, ids)
}

/// Tiles of the section grid: the enabled one first. `current` — the value of
/// [`Current::Builtin`] (empty — no built-in decoration is chosen now).
/// `cat_id`, `icon` — of the Aurorae section.
pub fn section_items(decos: &[Decoration], current: &str, cat_id: i64, icon: &str) -> Vec<ThemeItem> {
    let mut items: Vec<ThemeItem> = decos
        .iter()
        .map(|d| {
            let active = !current.is_empty() && d.is_current(current);
            let badge = if active {
                crate::appearance_page::local_active_badge()
            } else if d.user {
                tr("⬇️ Downloaded")
            } else {
                tr("📦 System")
            };
            ThemeItem {
                id: format!("local:aurorae:{}", d.entry()),
                name: d.name.clone(),
                description: d.description.clone(),
                author: format!("{icon} {} · {badge}", tr("Window Decorations")),
                category_id: cat_id,
                installed: true,
                install_path: d.path.clone(),
                local_kind: active.then(|| "active".to_string()),
                source_name: if d.name == d.theme { String::new() } else { d.theme.clone() },
                ..Default::default()
            }
        })
        .collect();
    items.sort_by_key(|it| it.local_kind.is_none());
    items
}

// ── Live disk ───────────────────────────────────────────────────────────

/// Where to look: Qt plugin directories and data directories (the user's first).
#[derive(Debug, Clone)]
pub struct Roots {
    pub plugin_dirs: Vec<String>,
    pub user_data: String,
    pub data_dirs: Vec<String>,
}

impl Roots {
    /// Directories of this machine (`QT_PLUGIN_PATH`, `XDG_DATA_DIRS`).
    pub fn live(home: &str) -> Self {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        let user_data = std::env::var("XDG_DATA_HOME")
            .ok()
            .filter(|d| d.starts_with('/'))
            .unwrap_or_else(|| format!("{}/.local/share", home.trim_end_matches('/')));
        Roots {
            plugin_dirs: crate::plugin_journal::qt_plugin_dirs(&env("QT_PLUGIN_PATH")),
            user_data,
            data_dirs: crate::plugin_journal::data_dirs(&env("XDG_DATA_DIRS")),
        }
    }

    /// Under the root `usr_root` (test stands and tests): `<usr>/lib/qt6/plugins`,
    /// `<usr>/share`.
    pub fn under(home: &str, usr_root: &str) -> Self {
        let usr = usr_root.trim_end_matches('/');
        Roots {
            plugin_dirs: vec![format!("{usr}/lib/qt6/plugins")],
            user_data: format!("{}/.local/share", home.trim_end_matches('/')),
            data_dirs: vec![format!("{usr}/share")],
        }
    }
}

fn sorted_names(dir: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// Decoration modules and QML theme packages on disk.
pub fn scan(roots: &Roots) -> Found {
    let mut found = Found::default();
    for dir in &roots.plugin_dirs {
        for ns in PLUGIN_NAMESPACES {
            let base = format!("{}/{ns}", dir.trim_end_matches('/'));
            for name in sorted_names(&base) {
                if !name.ends_with(".so") {
                    continue;
                }
                let path = format!("{base}/{name}");
                let Ok(bytes) = std::fs::read(&path) else { continue };
                if let Some(meta) = qt_plugin_metadata(&bytes) {
                    found.plugins.push((path, meta));
                }
            }
        }
    }
    let data = std::iter::once((roots.user_data.as_str(), true))
        .chain(roots.data_dirs.iter().map(|d| (d.as_str(), false)));
    for (root, user) in data {
        let base = format!("{}/{QML_PACKAGE_DIR}", root.trim_end_matches('/'));
        for name in sorted_names(&base) {
            let dir = format!("{base}/{name}");
            let Ok(text) = std::fs::read_to_string(format!("{dir}/metadata.json")) else { continue };
            if let Ok(meta) = serde_json::from_str::<Value>(&text) {
                found.qml_packages.push((dir, meta, user));
            }
        }
    }
    found
}

/// `library` of an Aurorae SVG theme on this machine ([`svg_library`]): looks
/// only at module file names, without reading metadata.
pub fn live_svg_library() -> &'static str {
    let env = std::env::var("QT_PLUGIN_PATH").unwrap_or_default();
    let v2 = crate::plugin_journal::qt_plugin_dirs(&env).iter().any(|dir| {
        PLUGIN_NAMESPACES
            .iter()
            .any(|ns| std::path::Path::new(&format!("{dir}/{ns}/{AURORAE_V2}.so")).is_file())
    });
    if v2 {
        AURORAE_V2
    } else {
        AURORAE_V1
    }
}

// ── Decoration snapshot for a tile ──────────────────────────────────────
//
// KDE's "Window Decorations" window does not draw the decoration itself: each
// tile is a live decoration drawn by its module through the KWin QML module
// `org.kde.kwin.private.kdecoration` (types Bridge, Settings, Decoration — an
// inactive window behind, an active one in front). Rexon does the same, but as
// a snapshot in a separate process: the script `data/kdecoration-preview/
// preview.qml` under Qt6's `qml` launcher on the `offscreen` platform saves the
// frame as a PNG, and the tile shows it on top of the shared background. A
// foreign decoration module thus never gets into the Rexon process (just as
// with reading metadata), and a crash or hang of the module costs one snapshot
// — the process is killed on timeout, and the tile gets its previous picture
// (a sample from SVG for Aurorae SVG themes, a placeholder for the others).

/// The snapshot script (put next to the picture cache and handed to `qml`).
pub const SHOT_SCRIPT: &str = include_str!("../../../data/kdecoration-preview/preview.qml");
/// File name of the script in the cache.
pub const SHOT_SCRIPT_NAME: &str = "kdecoration-preview.qml";
/// Qt6's QML launcher in distribution locations. `/usr/bin/qml` is deliberately
/// not in the list: in Arch it is the Qt5 launcher.
pub const QML_RUNNERS: &[&str] = &[
    "/usr/lib/qt6/bin/qml",
    "/usr/lib64/qt6/bin/qml",
    "/usr/lib/x86_64-linux-gnu/qt6/bin/qml",
    "/usr/lib/aarch64-linux-gnu/qt6/bin/qml",
    "/usr/bin/qml6",
];
/// Qt6 QML module directories in distribution locations.
pub const QML_IMPORT_DIRS: &[&str] = &[
    "/usr/lib/qt6/qml",
    "/usr/lib64/qt6/qml",
    "/usr/lib/x86_64-linux-gnu/qt6/qml",
    "/usr/lib/aarch64-linux-gnu/qt6/qml",
];
/// File of KWin's decoration preview QML module inside the module directory.
pub const PREVIEW_MODULE_QMLDIR: &str = "org/kde/kwin/private/kdecoration/qmldir";
/// Environment of the snapshot process: no display, software rendering, scale 1
/// (otherwise the frame comes out twice the size of the tile).
pub const SHOT_ENV: &[(&str, &str)] =
    &[("QT_QPA_PLATFORM", "offscreen"), ("QT_QUICK_BACKEND", "software"), ("QT_SCALE_FACTOR", "1")];
/// Variables not passed to the snapshot process: scale and input method. The
/// snapshot has nothing to type, and `QT_IM_MODULE=ibus` makes Qt knock on the
/// session's IBus daemon: on 16.11.2026 in Cinnamon 6.8 Wayland the Breeze
/// snapshot hung on it until the timeout (15 s per tile); without the variables
/// — 0.8 s.
pub const SHOT_ENV_REMOVE: &[&str] =
    &["QT_SCREEN_SCALE_FACTORS", "QT_AUTO_SCREEN_SCALE_FACTOR", "QT_IM_MODULE", "QT_IM_MODULES"];
/// How long to wait for a snapshot, seconds.
pub const SHOT_TIMEOUT_SECS: u64 = 15;
/// KWin border sizes in the order of the `Settings.borderSizesIndex` numbers.
pub const BORDER_SIZES: &[&str] = &["None", "NoSides", "Tiny", "Normal", "Large", "VeryLarge", "Huge", "VeryHuge", "Oversized"];
/// Number of the `Normal` size — KDE takes it when the size is "per theme".
pub const BORDER_NORMAL: usize = 3;

/// What to capture: the module, the theme, the window title, the border size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotRequest {
    pub library: String,
    pub theme: String,
    pub caption: String,
    pub border_index: usize,
}

/// The snapshot launcher: the first `qml` from [`QML_RUNNERS`] that has KWin's
/// preview module next to it ([`PREVIEW_MODULE_QMLDIR`]). `None` — there is
/// nothing to capture with (KWin is not installed, or Qt6 has no launcher).
pub fn shot_runner(is_file: &dyn Fn(&str) -> bool) -> Option<String> {
    let module = QML_IMPORT_DIRS.iter().any(|d| is_file(&format!("{d}/{PREVIEW_MODULE_QMLDIR}")));
    if !module {
        return None;
    }
    QML_RUNNERS.iter().find(|r| is_file(r)).map(|r| r.to_string())
}

/// Border size number for the snapshot from `kwinrc [org.kde.kdecoration2]`
/// `BorderSize` and `BorderSizeAuto`, as for KDE's tiles: the chosen size, and
/// for "per theme" (the default) — `Normal` (Breeze and Oxygen have no
/// recommended size of their own, KDE takes the same `Normal`).
pub fn border_index(border_size: Option<&str>, border_auto: Option<&str>) -> usize {
    let auto = border_auto.map(|v| v.trim() != "false").unwrap_or(true);
    if auto {
        return BORDER_NORMAL;
    }
    border_size
        .and_then(|s| BORDER_SIZES.iter().position(|b| *b == s.trim()))
        .unwrap_or(BORDER_NORMAL)
}

/// What to capture for tile `local_id`: a built-in decoration (`library/theme`)
/// as is, an Aurorae SVG theme (a directory name) under the `svg_library`
/// engine. `None` — the tile is not from the KDE decorations section.
pub fn shot_request(local_id: &str, caption: &str, svg_library: &str, border_index: usize) -> Option<ShotRequest> {
    let entry = local_id.strip_prefix("local:aurorae:").filter(|e| !e.is_empty())?;
    let (library, theme) = choice(entry, svg_library);
    Some(ShotRequest { library, theme, caption: caption.to_string(), border_index })
}

/// Snapshot argv: the launcher, the script, and after `--` the module, theme,
/// title, border size number and where to save.
pub fn shot_argv(runner: &str, script: &str, req: &ShotRequest, out: &str) -> Vec<String> {
    vec![
        runner.to_string(),
        script.to_string(),
        "--".to_string(),
        req.library.clone(),
        req.theme.clone(),
        req.caption.clone(),
        req.border_index.to_string(),
        out.to_string(),
    ]
}

/// Addition to the cache key of a decoration picture: the frame depends on
/// whether there is anything to capture with, on the border size and on the
/// colors (`kdeglobals` — KDE decorations take title bar colors from the
/// scheme).
pub fn shot_cache_tag(runner: Option<&str>, border_index: usize, colors_stamp: i64) -> String {
    match runner {
        Some(_) => format!("shot\u{0}{border_index}\u{0}{colors_stamp}"),
        None => "noshot".to_string(),
    }
}

/// What is known about snapshots on this machine — gathered once per batch of
/// tiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotSetup {
    pub runner: Option<String>,
    pub border_index: usize,
    pub svg_library: &'static str,
    /// [`shot_cache_tag`].
    pub tag: String,
}

impl ShotSetup {
    /// Live machine: the launcher, `kwinrc` and the `kdeglobals` mark from
    /// `$XDG_CONFIG_HOME` (otherwise `~/.config`).
    pub fn live(home: &str) -> Self {
        let runner = shot_runner(&|p| std::path::Path::new(p).is_file());
        let cfg = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|d| d.starts_with('/'))
            .unwrap_or_else(|| format!("{}/.config", home.trim_end_matches('/')));
        let kwinrc = std::fs::read_to_string(format!("{cfg}/kwinrc")).ok();
        let ini = crate::gtk_sanitizer::read_kde_ini(kwinrc.as_deref());
        let value = |key: &str| {
            let v = crate::gtk_sanitizer::kde_value(&ini, KWINRC_GROUP, key);
            (!v.is_empty()).then_some(v)
        };
        let border_index = border_index(value("BorderSize").as_deref(), value("BorderSizeAuto").as_deref());
        let colors_stamp = std::fs::metadata(format!("{cfg}/kdeglobals"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64);
        let tag = shot_cache_tag(runner.as_deref(), border_index, colors_stamp);
        ShotSetup { runner, border_index, svg_library: live_svg_library(), tag }
    }

    /// What to capture for a tile (`None` — not a KDE decoration or nothing to
    /// capture with).
    pub fn request(&self, local_id: &str, caption: &str) -> Option<ShotRequest> {
        self.runner.as_ref()?;
        shot_request(local_id, caption, self.svg_library, self.border_index)
    }
}

/// Capture the decoration into `out` (PNG [`crate::local_preview::PREVIEW_WIDTH`]
/// × [`crate::local_preview::PREVIEW_HEIGHT`]). The script is put into
/// `script_dir`. A process that does not finish within [`SHOT_TIMEOUT_SECS`] is
/// killed. `false` — no snapshot (the module or theme was not found, the module
/// crashed, the frame has the wrong size); an unfinished file is erased.
pub fn take_shot(runner: &str, script_dir: &str, req: &ShotRequest, out: &str) -> bool {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let script = format!("{}/{SHOT_SCRIPT_NAME}", script_dir.trim_end_matches('/'));
    if std::fs::read_to_string(&script).ok().as_deref() != Some(SHOT_SCRIPT) {
        let _ = std::fs::create_dir_all(script_dir);
        if std::fs::write(&script, SHOT_SCRIPT).is_err() {
            return false;
        }
    }
    let _ = std::fs::remove_file(out);
    let argv = shot_argv(runner, &script, req, out);
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for (k, v) in SHOT_ENV {
        cmd.env(k, v);
    }
    for k in SHOT_ENV_REMOVE {
        cmd.env_remove(k);
    }
    let Ok(mut child) = cmd.spawn() else { return false };
    let deadline = Instant::now() + Duration::from_secs(SHOT_TIMEOUT_SECS);
    let ok = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let want = (crate::local_preview::PREVIEW_WIDTH as f64, crate::local_preview::PREVIEW_HEIGHT as f64);
    let good = ok && crate::local_preview::image_size(std::path::Path::new(out)) == Some(want);
    if !good {
        let _ = std::fs::remove_file(out);
    }
    good
}

// ── "Configure…" for a decoration ───────────────────────────────────────
//
// KDE's "Window Decorations" window (checked against kwin `src/kcms/decoration`
// and aurorae 6.8.2): a tile has an edit button, which can be pressed when the
// decoration has a settings module. A decoration module names it itself — the
// `X-KDE-ConfigModule` key in its metadata (Breeze — `kcm_breezedecoration`,
// Oxygen — `kcm_oxygendecoration`, third-party ones — their own). Aurorae
// themes have a single module, `kcm_auroraedecoration`: SVG themes always have
// it (button size), QML themes only with `contents/ui/config.ui` and
// `contents/config/main.xml` in the theme package. The button builds a KDE
// dialog with this module from the `org.kde.kdecoration3.kcm` namespace and
// hands it the theme; after saving, KWin re-reads the settings.
//
// Rexon opens the same dialog with the same KWin QML module, in a separate
// `qml` process with the script `data/kdecoration-preview/configure.qml`. If
// there is no launcher, the decoration module is opened in `kcmshell6` by its
// plugin path; the theme cannot be passed that way (`kcmshell6 --args` gives the
// module strings, while Aurorae expects a dictionary), so an Aurorae theme then
// opens the whole KDE "Window Decorations" module — there is an edit button on
// its tile. KDE decorations are drawn only by KWin: outside Plasma the button
// is disabled with a reason.

/// The settings window script (put into the cache and handed to `qml`).
pub const CONFIG_SCRIPT: &str = include_str!("../../../data/kdecoration-preview/configure.qml");
/// File name of the script in the cache.
pub const CONFIG_SCRIPT_NAME: &str = "kdecoration-configure.qml";
/// Namespaces of decoration settings modules: Plasma 6.3+ and the earlier one.
pub const KCM_NAMESPACES: &[&str] = &["org.kde.kdecoration3.kcm", "org.kde.kdecoration2.kcm"];
/// The settings module of Aurorae themes.
pub const AURORAE_KCM: &str = "kcm_auroraedecoration";
/// The whole KDE "Window Decorations" module.
pub const DECORATION_KCM: &str = "kcm_kwindecoration";
/// The program that shows a KDE module as a separate window.
pub const KCMSHELL: &str = "kcmshell6";

/// The decoration whose settings are looked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigTarget {
    pub library: String,
    pub theme: String,
    /// An Aurorae engine theme (the settings module needs a theme).
    pub engine_theme: bool,
    /// Settings module; empty — the decoration has no settings of its own.
    pub kcmodule: String,
    /// Title of the settings window — the decoration name.
    pub caption: String,
}

/// Settings module of a decoration module from its metadata (`X-KDE-ConfigModule`).
pub fn plugin_config_module(meta: &Value) -> String {
    meta.get("X-KDE-ConfigModule").and_then(Value::as_str).map(str::trim).unwrap_or("").to_string()
}

/// Decoration of tile `local_id` with path `install_path` and name `caption`.
/// `svg_library` — [`svg_library`] of this machine, `is_file` — whether a file
/// exists, `plugin_meta` — metadata of the decoration module by its file path.
/// `None` — the tile is not from the KDE decorations section.
pub fn config_target(
    local_id: &str,
    install_path: &str,
    caption: &str,
    svg_library: &str,
    is_file: &dyn Fn(&str) -> bool,
    plugin_meta: &dyn Fn(&str) -> Option<Value>,
) -> Option<ConfigTarget> {
    let entry = local_id.strip_prefix("local:aurorae:").filter(|e| !e.is_empty())?;
    let (library, theme) = choice(entry, svg_library);
    let dir = install_path.trim_end_matches('/');
    let (engine_theme, kcmodule) = match parse_entry(entry) {
        // An SVG theme: the settings module always exists.
        None => (true, AURORAE_KCM.to_string()),
        Some((lib, _)) if lib == AURORAE_V1 || lib == AURORAE_V2 => {
            let own = is_file(&format!("{dir}/contents/ui/config.ui"))
                && is_file(&format!("{dir}/contents/config/main.xml"));
            (true, if own { AURORAE_KCM.to_string() } else { String::new() })
        }
        Some(_) => (false, plugin_meta(install_path).map(|m| plugin_config_module(&m)).unwrap_or_default()),
    };
    Some(ConfigTarget { library, theme, engine_theme, kcmodule, caption: caption.to_string() })
}

/// Plugin path of settings module `kcmodule` (`<namespace>/<module>`), if its
/// file is in one of the Qt plugin directories.
pub fn kcm_plugin(kcmodule: &str, plugin_dirs: &[String], is_file: &dyn Fn(&str) -> bool) -> Option<String> {
    if kcmodule.is_empty() || kcmodule.contains('/') {
        return None;
    }
    plugin_dirs.iter().find_map(|dir| {
        KCM_NAMESPACES.iter().find_map(|ns| {
            let file = format!("{}/{ns}/{kcmodule}.so", dir.trim_end_matches('/'));
            is_file(&file).then(|| format!("{ns}/{kcmodule}"))
        })
    })
}

/// What the machine has for the settings window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFacts {
    /// A Plasma session (windows are drawn by KWin).
    pub plasma: bool,
    /// The `qml` launcher with KWin's preview module ([`shot_runner`]).
    pub runner: Option<String>,
    /// [`kcm_plugin`] of the decoration's settings module.
    pub kcm: Option<String>,
    /// Whether [`KCMSHELL`] is present.
    pub kcmshell: bool,
}

/// The "Configure…" button of a decoration.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ConfigButton {
    pub visible: bool,
    pub enabled: bool,
    pub text: String,
    /// What will open — or an honest reason why there is nothing to open it with.
    pub tooltip: String,
    /// The command; the launcher is the first element. Empty — cannot be pressed.
    #[serde(skip)]
    pub argv: Vec<String>,
}

/// The "Configure…" button for decoration `target` (`None` — the tile is not a
/// KDE decoration, there is no button). `script` — the path of the
/// [`CONFIG_SCRIPT`] script in the cache.
pub fn config_button(target: Option<&ConfigTarget>, facts: &ConfigFacts, script: &str) -> ConfigButton {
    let Some(t) = target else {
        return ConfigButton {
            visible: false,
            enabled: false,
            text: String::new(),
            tooltip: String::new(),
            argv: Vec::new(),
        };
    };
    let off = |tooltip: String| ConfigButton {
        visible: true,
        enabled: false,
        text: tr("Configure…"),
        tooltip,
        argv: Vec::new(),
    };
    let on = |argv: Vec<String>, tooltip: String| ConfigButton {
        visible: true,
        enabled: true,
        text: tr("Configure…"),
        tooltip,
        argv,
    };
    if !facts.plasma {
        return off(tr(
            "KDE window decorations are drawn only by KWin in Plasma; on this desktop windows are framed by its own window manager, so there's nothing to configure here.",
        ));
    }
    if t.kcmodule.is_empty() {
        return off(tr("This window decoration has no settings of its own."));
    }
    let Some(kcm) = facts.kcm.as_deref() else {
        return off(tr("The settings module of this window decoration isn't installed."));
    };
    if let Some(runner) = facts.runner.as_deref() {
        return on(
            vec![
                runner.to_string(),
                "-a".to_string(),
                "widget".to_string(),
                script.to_string(),
                "--".to_string(),
                t.library.clone(),
                t.theme.clone(),
                t.kcmodule.clone(),
                t.caption.clone(),
            ],
            crate::i18n::trf("Opens the settings of “%1” — the same window as in KDE System Settings", &[&t.caption]),
        );
    }
    if facts.kcmshell {
        if t.engine_theme {
            return on(
                vec![KCMSHELL.to_string(), DECORATION_KCM.to_string()],
                tr("Opens the KDE “Window Decorations” module: the settings of this theme are under the edit button on its tile"),
            );
        }
        return on(
            vec![KCMSHELL.to_string(), kcm.to_string(), "--caption".to_string(), t.caption.clone()],
            crate::i18n::trf("Opens the settings of “%1” — the same window as in KDE System Settings", &[&t.caption]),
        );
    }
    off(tr("The settings of this window decoration open in a KDE window, and there's nothing to show it with: install KWin or kcmutils (kcmshell6)."))
}

/// [`config_button`] on the live machine: the desktop — by
/// `XDG_CURRENT_DESKTOP`, modules — by the Qt plugin directories, programs — by
/// PATH. The script is put into the Rexon cache only for a button that calls it.
pub fn config_button_live(local_id: &str, install_path: &str, caption: &str) -> ConfigButton {
    let is_file = |p: &str| std::path::Path::new(p).is_file();
    let read_meta = |p: &str| std::fs::read(p).ok().and_then(|b| qt_plugin_metadata(&b));
    let Some(target) = config_target(local_id, install_path, caption, live_svg_library(), &is_file, &read_meta) else {
        return config_button(None, &live_config_facts(""), "");
    };
    let facts = live_config_facts(&target.kcmodule);
    let home = std::env::var("HOME").unwrap_or_default();
    let script = format!("{}/{CONFIG_SCRIPT_NAME}", crate::appearance_cache::cache_dir(&home));
    config_button(Some(&target), &facts, &script)
}

fn live_config_facts(kcmodule: &str) -> ConfigFacts {
    let is_file = |p: &str| std::path::Path::new(p).is_file();
    let env = std::env::var("QT_PLUGIN_PATH").unwrap_or_default();
    let path = std::env::var("PATH").ok();
    ConfigFacts {
        plasma: crate::app::is_plasma_desktop(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()),
        runner: shot_runner(&is_file),
        kcm: kcm_plugin(kcmodule, &crate::plugin_journal::qt_plugin_dirs(&env), &is_file),
        kcmshell: crate::utils::which(KCMSHELL, path.as_deref()).is_some(),
    }
}

/// Put the settings window script where the button's `argv` looks for it (if
/// the button calls it). `false` — writing failed.
pub fn ensure_config_script(argv: &[String]) -> bool {
    let Some(script) = argv.iter().find(|a| a.ends_with(CONFIG_SCRIPT_NAME)) else {
        return true;
    };
    if std::fs::read_to_string(script).ok().as_deref() == Some(CONFIG_SCRIPT) {
        return true;
    }
    if let Some(dir) = std::path::Path::new(script).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(script, CONFIG_SCRIPT).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A minimal CBOR entry: text.
    fn t(s: &str) -> Vec<u8> {
        let mut v = Vec::new();
        let n = s.len();
        if n < 24 {
            v.push(0x60 | n as u8);
        } else {
            v.push(0x78);
            v.push(n as u8);
        }
        v.extend_from_slice(s.as_bytes());
        v
    }

    /// A module file with an ELF note the way Qt 6 writes it.
    fn plugin_bytes(meta_cbor: &[u8]) -> Vec<u8> {
        let mut cbor = vec![0xbf, 0x02];
        cbor.extend(t("org.kde.KPluginFactory"));
        cbor.push(0x03);
        cbor.extend(t("BreezeDecoFactory"));
        cbor.push(0x04);
        cbor.extend_from_slice(meta_cbor);
        cbor.push(0xff);
        let mut desc = vec![1, 6, 12, 4];
        desc.extend(cbor);
        let mut out = vec![0x7f, b'E', b'L', b'F', 0, 0, 0, 0];
        out.extend(12u32.to_le_bytes());
        out.extend((desc.len() as u32).to_le_bytes());
        out.extend(0x7451_0001u32.to_le_bytes());
        out.extend_from_slice(b"qt-project!\0");
        out.extend(desc);
        out.extend([0u8; 16]);
        out
    }

    #[test]
    fn reads_breeze_like_metadata_from_elf_note() {
        // {"KPlugin": {"Name": "Breeze", "Name[ru]": "Бриз", "Description": "D"}}
        let mut meta = vec![0xa1];
        meta.extend(t("KPlugin"));
        meta.push(0xa3);
        meta.extend(t("Name"));
        meta.extend(t("Breeze"));
        meta.extend(t("Name[ru]"));
        meta.extend(t("Бриз"));
        meta.extend(t("Description"));
        meta.extend(t("D"));
        let got = qt_plugin_metadata(&plugin_bytes(&meta)).unwrap();
        assert_eq!(got, json!({"KPlugin": {"Name": "Breeze", "Name[ru]": "Бриз", "Description": "D"}}));
        let Plugin::Plain(d) = plugin_from_meta("/q/org.kde.kdecoration3/org.kde.breeze.so", &got, "ru_RU.UTF-8") else {
            panic!()
        };
        assert_eq!((d.library.as_str(), d.theme.as_str(), d.name.as_str()), ("org.kde.breeze", "Бриз", "Бриз"));
        let Plugin::Plain(d) = plugin_from_meta("/q/org.kde.breeze.so", &got, "de") else { panic!() };
        assert_eq!(d.theme, "Breeze");
    }

    #[test]
    fn engine_flags_and_values_decode() {
        // {"org.kde.kdecoration3": {"themes": true, "defaultTheme": "p", "n": 300, "x": [1, -2, null, 1.5]}}
        let mut meta = vec![0xa1];
        meta.extend(t("org.kde.kdecoration3"));
        meta.push(0xa4);
        meta.extend(t("themes"));
        meta.push(0xf5);
        meta.extend(t("defaultTheme"));
        meta.extend(t("p"));
        meta.extend(t("n"));
        meta.extend([0x19, 0x01, 0x2c]);
        meta.extend(t("x"));
        meta.extend([0x84, 0x01, 0x21, 0xf6, 0xf9, 0x3e, 0x00]);
        let got = qt_plugin_metadata(&plugin_bytes(&meta)).unwrap();
        assert_eq!(got["org.kde.kdecoration3"]["n"], json!(300));
        assert_eq!(got["org.kde.kdecoration3"]["x"], json!([1, -2, null, 1.5]));
        assert_eq!(
            plugin_from_meta("/q/org.kde.kwin.aurorae.so", &got, ""),
            Plugin::Engine { id: "org.kde.kwin.aurorae".into(), default_theme: "p".into() }
        );
    }

    #[test]
    fn broken_or_missing_metadata_is_none() {
        assert_eq!(qt_plugin_metadata(b"\x7fELF nothing here"), None);
        let mut bytes = plugin_bytes(&[0xa1]);
        bytes.truncate(bytes.len() - 20); // a truncated map
        assert_eq!(qt_plugin_metadata(&bytes), None);
        // Too deep nesting does not break the parsing.
        let deep = vec![0x81; 200];
        assert_eq!(qt_plugin_metadata(&plugin_bytes(&deep)), None);
    }

    fn plain(id: &str, name: &str) -> (String, Value) {
        (format!("/q/org.kde.kdecoration3/{id}.so"), json!({"KPlugin": {"Name": name}}))
    }

    fn found() -> Found {
        Found {
            plugins: vec![
                plain("org.kde.breeze", "Breeze"),
                (
                    "/q/org.kde.kdecoration3/org.kde.kwin.aurorae.so".into(),
                    json!({"org.kde.kdecoration3": {"themes": true, "themeListKeyword": "themes", "defaultTheme": "kwin4_decoration_qml_plastik"}}),
                ),
                (
                    "/q/org.kde.kdecoration3/org.kde.kwin.aurorae.v2.so".into(),
                    json!({"org.kde.kdecoration3": {"themes": true, "themeListKeyword": "themes"}}),
                ),
                plain("org.kde.oxygen", "Oxygen"),
                // The same module from a second directory — not a second time.
                plain("org.kde.oxygen", "Oxygen 2"),
                // A foreign theme engine: we cannot list its themes.
                ("/q/org.kde.kdecoration3/x.engine.so".into(), json!({"org.kde.kdecoration3": {"themes": true}})),
            ],
            qml_packages: vec![
                (
                    "/u/kwin/decorations/kwin4_decoration_qml_plastik".into(),
                    json!({"KPackageStructure": "KWin/Decoration", "KPlugin": {"Id": "kwin4_decoration_qml_plastik", "Name": "Plastik", "Name[ru]": "Пластик"}}),
                    false,
                ),
                ("/u/kwin/decorations/other".into(), json!({"KPackageStructure": "Plasma/Applet"}), false),
            ],
        }
    }

    #[test]
    fn lists_plain_plugins_and_qml_themes_like_kde() {
        let (decos, ids) = decorations(&found(), "ru");
        let entries: Vec<String> = decos.iter().map(Decoration::entry).collect();
        assert_eq!(
            entries,
            ["org.kde.breeze/Breeze", "org.kde.oxygen/Oxygen", "org.kde.kwin.aurorae/kwin4_decoration_qml_plastik"]
        );
        assert_eq!(decos[2].name, "Пластик");
        assert!(decos[2].engine_default);
        assert_eq!(svg_library(&ids), AURORAE_V2);
        assert_eq!(svg_library(&["org.kde.kwin.aurorae"]), AURORAE_V1);
    }

    #[test]
    fn current_follows_library_not_only_theme() {
        // The key was not read — as before, by `theme=`.
        assert_eq!(current(None, Some("__aurorae__svg__Sweet\n")), Current::Svg("Sweet".into()));
        assert_eq!(current(None, Some("kwin4_decoration_qml_plastik")), Current::Unknown);
        // An empty `library` — Breeze, even with a previous SVG theme in `theme=`.
        assert_eq!(current(Some("\n"), Some("__aurorae__svg__Sweet")), Current::Builtin("org.kde.breeze/__aurorae__svg__Sweet".into()));
        assert_eq!(current(Some("org.kde.kwin.aurorae.v2"), Some("__aurorae__svg__Sweet")), Current::Svg("Sweet".into()));
        assert_eq!(current(Some("org.kde.kwin.aurorae"), Some("__aurorae__svg__Sweet")), Current::Svg("Sweet".into()));
        assert_eq!(current(Some("org.kde.oxygen"), None), Current::Builtin("org.kde.oxygen/".into()));
    }

    #[test]
    fn highlight_matches_kde_rules() {
        let (decos, _) = decorations(&found(), "");
        let breeze = &decos[0];
        let plastik = &decos[2];
        // A module without themes is enabled with any `theme=`.
        assert!(breeze.is_current("org.kde.breeze/__aurorae__svg__Sweet"));
        assert!(breeze.is_current("org.kde.breeze/"));
        assert!(!breeze.is_current("org.kde.oxygen/Oxygen"));
        // An engine theme — by the theme name; an empty name — the default theme.
        assert!(plastik.is_current("org.kde.kwin.aurorae/kwin4_decoration_qml_plastik"));
        assert!(plastik.is_current("org.kde.kwin.aurorae/"));
        assert!(!plastik.is_current("org.kde.kwin.aurorae/other"));
        let items = section_items(&decos, "org.kde.oxygen/Oxygen", 114, "🪟");
        assert_eq!(items[0].id, "local:aurorae:org.kde.oxygen/Oxygen");
        assert_eq!(items[0].local_kind.as_deref(), Some("active"));
        assert!(items[1..].iter().all(|it| it.local_kind.is_none()));
        assert_eq!(items[0].install_path, "/q/org.kde.kdecoration3/org.kde.oxygen.so");
        assert!(section_items(&decos, "", 114, "🪟").iter().all(|it| it.local_kind.is_none()));
    }

    #[test]
    fn writes_library_then_theme() {
        let [lib, theme] = write_argv("org.kde.breeze/Breeze", AURORAE_V2);
        assert_eq!(lib[lib.len() - 2..], ["library".to_string(), "org.kde.breeze".to_string()]);
        assert_eq!(theme[theme.len() - 2..], ["theme".to_string(), "Breeze".to_string()]);
        assert_eq!(lib[..6], ["kwriteconfig6", "--file", "kwinrc", "--group", "org.kde.kdecoration2", "--key"]);
        // An SVG theme — under this machine's engine.
        assert_eq!(choice("Sweet", AURORAE_V2), (AURORAE_V2.to_string(), "__aurorae__svg__Sweet".to_string()));
        assert_eq!(choice("Sweet", AURORAE_V1), (AURORAE_V1.to_string(), "__aurorae__svg__Sweet".to_string()));
        assert_eq!(parse_entry("Sweet"), None);
        assert_eq!(parse_entry("/x"), None);
    }

    #[test]
    fn scans_disk_under_root() {
        let tmp = std::env::temp_dir().join(format!("rexon-kdeco-{}", std::process::id()));
        let usr = tmp.join("usr");
        let home = tmp.join("home");
        let plugins = usr.join("lib/qt6/plugins/org.kde.kdecoration3");
        std::fs::create_dir_all(&plugins).unwrap();
        let mut meta = vec![0xa1];
        meta.extend(t("KPlugin"));
        meta.push(0xa1);
        meta.extend(t("Name"));
        meta.extend(t("Breeze"));
        std::fs::write(plugins.join("org.kde.breeze.so"), plugin_bytes(&meta)).unwrap();
        std::fs::write(plugins.join("junk.so"), b"not a plugin").unwrap();
        let qml = home.join(".local/share/kwin/decorations/mine");
        std::fs::create_dir_all(&qml).unwrap();
        std::fs::write(qml.join("metadata.json"), r#"{"KPlugin": {"Id": "mine", "Name": "Mine"}}"#).unwrap();
        let roots = Roots::under(home.to_str().unwrap(), usr.to_str().unwrap());
        let found = scan(&roots);
        assert_eq!(found.plugins.len(), 1);
        assert_eq!(found.qml_packages.len(), 1);
        assert!(found.qml_packages[0].2, "пакет из каталога пользователя");
        // Without the v1 engine QML themes are not shown (KDE will not show them either).
        let (decos, _) = decorations(&found, "");
        assert_eq!(decos.iter().map(Decoration::entry).collect::<Vec<_>>(), ["org.kde.breeze/Breeze"]);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
