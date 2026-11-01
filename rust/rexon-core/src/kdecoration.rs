//! Встроенные рамки окон KDE — модули KDecoration (Breeze, Oxygen,
//! сторонние вроде Klassy) и темы QML движка Aurorae (Plastik).
//!
//! Раздел «Оформление окон» (114) раньше видел только темы Aurorae SVG из
//! `aurorae/themes`. Рамка KDE по умолчанию — модуль Breeze, а не тема
//! Aurorae, поэтому у человека на виде KDE по умолчанию в сетке не было
//! подсвечено ничего, и вернуть Breeze после темы Aurorae было нечем.
//!
//! Что показывает окно «Оформление окон» Plasma 6.8 (замер на этой машине):
//! все модули пространства `org.kde.kdecoration3` (Plasma 6.0–6.2 —
//! `org.kde.kdecoration2`) в каталогах модулей Qt. Модуль без
//! `"themes": true` в своём разделе метаданных — одна рамка: в `kwinrc
//! [org.kde.kdecoration2]` ложатся `library=<id модуля>` и `theme=<имя
//! модуля>`. Модуль-движок (`"themes": true`) сам рамкой не показывается —
//! показываются его темы:
//!   * `org.kde.kwin.aurorae` — темы QML, пакеты `KWin/Decoration` в
//!     `<data>/kwin/decorations/<id>`; `theme=<id пакета>` (Plastik);
//!   * `org.kde.kwin.aurorae.v2` (появился в Plasma 6.8) — темы SVG
//!     `aurorae/themes/<имя>` с `metadata.desktop`;
//!     `theme=__aurorae__svg__<имя>`. Прежде темы SVG шли под первым
//!     модулем — поэтому `library` темы SVG выбирается по тому, какой модуль
//!     установлен ([`svg_library`]).
//! Темы прочих движков перечисляет только код самого модуля — его мы не
//! загружаем, такие модули пропускаются.
//!
//! Метаданные модуля Qt зашиты в сам `.so`: заметка ELF `qt-project!`
//! (Qt ≥ 6.3) либо метка `QTMETADATA !` (раньше) — четыре байта заголовка и
//! карта CBOR, где под ключом 4 лежит тот же JSON, что `KPluginMetaData`.
//! Разбор — свой, без загрузки модуля: [`qt_plugin_metadata`].
//!
//! Номер плитки — `local:aurorae:<library>/<theme>`: косой черты в имени
//! каталога темы Aurorae не бывает, поэтому с плитками SVG
//! (`local:aurorae:<каталог>`) он не путается ([`parse_entry`]).

use serde_json::{Map, Value};

use crate::i18n::tr;
use crate::models::ThemeItem;

/// Группа `kwinrc`, где KWin держит выбор рамки (имя не сменилось с
/// KDecoration2).
pub const KWINRC_GROUP: &str = "org.kde.kdecoration2";
/// Рамка KDE по умолчанию (`kwindecorationsettings.kcfg`).
pub const DEFAULT_LIBRARY: &str = "org.kde.breeze";
/// Движок тем Aurorae: темы QML, а до Plasma 6.8 — и SVG.
pub const AURORAE_V1: &str = "org.kde.kwin.aurorae";
/// Движок тем Aurorae SVG Plasma 6.8.
pub const AURORAE_V2: &str = "org.kde.kwin.aurorae.v2";
/// Приставка темы Aurorae SVG в `theme=`.
pub const SVG_PREFIX: &str = "__aurorae__svg__";
/// Пространства имён модулей рамок: Plasma 6.3+ и прежнее.
pub const PLUGIN_NAMESPACES: &[&str] = &["org.kde.kdecoration3", "org.kde.kdecoration2"];
/// Подкаталог данных с пакетами тем QML движка Aurorae.
pub const QML_PACKAGE_DIR: &str = "kwin/decorations";

/// Одна рамка в сетке: что писать в `kwinrc` и что показать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoration {
    /// `library=` — id модуля.
    pub library: String,
    /// `theme=` — имя модуля (так пишет KDE) либо id пакета QML.
    pub theme: String,
    /// Тема движка (у модуля без тем выбор — по одному `library`).
    pub engine_theme: bool,
    /// Тема по умолчанию своего движка (`defaultTheme`): она включена, когда
    /// `theme=` пуст.
    pub engine_default: bool,
    /// Имя на языке интерфейса.
    pub name: String,
    /// Описание на языке интерфейса (пусто — нет).
    pub description: String,
    /// Файл модуля либо каталог пакета.
    pub path: String,
    /// Лежит в каталоге пользователя.
    pub user: bool,
}

impl Decoration {
    /// Имя плитки после `local:aurorae:`.
    pub fn entry(&self) -> String {
        entry_id(&self.library, &self.theme)
    }

    /// Включена ли эта рамка при `current` = `library/theme` из `kwinrc`
    /// ([`current`]).
    pub fn is_current(&self, current: &str) -> bool {
        let Some((library, theme)) = parse_entry(current) else { return false };
        if library != self.library {
            return false;
        }
        // Модуль без тем рисует себя при любом `theme=`.
        !self.engine_theme || theme == self.theme || (theme.is_empty() && self.engine_default)
    }
}

/// `library/theme` — имя плитки встроенной рамки.
pub fn entry_id(library: &str, theme: &str) -> String {
    format!("{library}/{theme}")
}

/// `(library, theme)` из имени плитки; `None` — это тема Aurorae SVG (имя
/// каталога).
pub fn parse_entry(entry: &str) -> Option<(&str, &str)> {
    entry.split_once('/').filter(|(library, _)| !library.is_empty())
}

/// Что включено в `kwinrc` сейчас.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Current {
    /// Тема Aurorae SVG — имя каталога.
    Svg(String),
    /// Встроенная рамка — `library/theme`.
    Builtin(String),
    /// Не узнать.
    Unknown,
}

/// Текущая рамка по значениям `kwinrc [org.kde.kdecoration2]`.
///
/// `library` — `None`, когда ключ не читали (нечем): тогда как прежде,
/// по одному `theme=`. Пустой `library` — умолчание KDE (Breeze). Тема SVG
/// засчитывается только под движком Aurorae: под Breeze `theme=__aurorae__…`
/// остаётся от прежнего выбора, а рисует KWin всё равно Breeze.
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

/// `library` для темы Aurorae SVG: модуль v2, если он есть (Plasma 6.8+),
/// иначе v1. `installed` — id установленных модулей рамок.
pub fn svg_library<S: AsRef<str>>(installed: &[S]) -> &'static str {
    if installed.iter().any(|id| id.as_ref() == AURORAE_V2) {
        AURORAE_V2
    } else {
        AURORAE_V1
    }
}

/// Что писать в `kwinrc` для плитки `entry`: `(library, theme)`.
/// `svg_library` — [`svg_library`] этой машины.
pub fn choice(entry: &str, svg_library: &str) -> (String, String) {
    match parse_entry(entry) {
        Some((library, theme)) => (library.to_string(), theme.to_string()),
        None => (svg_library.to_string(), format!("{SVG_PREFIX}{entry}")),
    }
}

/// argv записей выбора рамки: сперва `library`, затем `theme`.
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

// ── Метаданные модуля Qt ────────────────────────────────────────────────

/// Метаданные модуля Qt (то, что `KPluginMetaData::rawData`) из байтов его
/// файла; `None` — метаданных нет или они битые.
pub fn qt_plugin_metadata(bytes: &[u8]) -> Option<Value> {
    let cbor = note_payload(bytes).or_else(|| legacy_payload(bytes))?;
    // Четыре байта заголовка: версия формата, мажор и минор Qt, требования.
    let body = cbor.get(4..)?;
    let mut reader = Cbor { data: body, pos: 0 };
    let top = reader.value(0)?;
    match top {
        Value::Object(map) => map.get("4").cloned().filter(Value::is_object),
        _ => None,
    }
}

/// Тело заметки ELF `qt-project!` (Qt ≥ 6.3).
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
        let start = pos + NAME.len(); // 12 — уже кратно четырём
        if let Some(desc) = bytes.get(start..start.checked_add(size)?) {
            return Some(desc);
        }
    }
    None
}

/// Данные после метки `QTMETADATA !` (Qt до 6.3).
fn legacy_payload(bytes: &[u8]) -> Option<&[u8]> {
    const MAGIC: &[u8] = b"QTMETADATA !";
    let pos = find(bytes, MAGIC)?;
    bytes.get(pos + MAGIC.len()..)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Чтение CBOR (RFC 8949) ровно настолько, насколько его пишет Qt в
/// метаданные: числа, строки, массивы, карты (в том числе неопределённой
/// длины), метки, true/false/null, числа с точкой. Ключи-числа становятся
/// строками.
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

    /// Аргумент заголовка; `None` у «неопределённой длины» — отдельно.
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
            // Метка: значение под ней.
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

// ── Рамки из метаданных ─────────────────────────────────────────────────

/// Строка `KPlugin.<base>` на языке `locale` (ключи — как у тем,
/// [`crate::installed_scan::locale_keys`]).
fn localized(kplugin: Option<&Map<String, Value>>, base: &str, locale: &str) -> String {
    let Some(kp) = kplugin else { return String::new() };
    crate::installed_scan::locale_keys(base, locale)
        .iter()
        .find_map(|k| kp.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()))
        .unwrap_or("")
        .to_string()
}

/// Раздел настроек рамки в метаданных модуля.
fn deco_settings(meta: &Value) -> Option<&Map<String, Value>> {
    PLUGIN_NAMESPACES.iter().find_map(|ns| meta.get(*ns).and_then(Value::as_object))
}

/// Что за модуль рамки перед нами.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plugin {
    /// Сам себе рамка.
    Plain(Decoration),
    /// Движок тем: `id`, тема по умолчанию (`defaultTheme`, пусто — нет).
    Engine { id: String, default_theme: String },
}

/// Модуль рамки из файла `path` с метаданными `meta` ([`qt_plugin_metadata`]).
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
    // Так KDE и пишет `theme=`: имя модуля (на языке интерфейса), а без
    // имени — его id.
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

/// Тема QML движка Aurorae v1 из пакета `dir` с `metadata.json` = `meta`.
/// `None` — пакет не рамки.
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

/// Что лежит на диске — сырьё [`decorations`].
#[derive(Debug, Default, Clone)]
pub struct Found {
    /// (путь модуля, его метаданные), в порядке каталогов поиска.
    pub plugins: Vec<(String, Value)>,
    /// (каталог пакета QML, его `metadata.json`, каталог пользователя).
    pub qml_packages: Vec<(String, Value, bool)>,
}

/// Встроенные рамки для сетки (темы SVG сюда не входят — их показывает
/// скан каталогов Aurorae) и id всех найденных модулей. Первый модуль с
/// данным id побеждает, как у Qt (каталоги — по порядку поиска).
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

/// Плитки сетки раздела: включённая — первой. `current` — значение
/// [`Current::Builtin`] (пусто — встроенная рамка сейчас не выбрана).
/// `cat_id`, `icon` — раздела Aurorae.
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

// ── Живой диск ──────────────────────────────────────────────────────────

/// Где искать: каталоги модулей Qt и каталоги данных (пользователя — первым).
#[derive(Debug, Clone)]
pub struct Roots {
    pub plugin_dirs: Vec<String>,
    pub user_data: String,
    pub data_dirs: Vec<String>,
}

impl Roots {
    /// Каталоги этой машины (`QT_PLUGIN_PATH`, `XDG_DATA_DIRS`).
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

    /// Под корнем `usr_root` (стенды и тесты): `<usr>/lib/qt6/plugins`,
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

/// Модули рамок и пакеты тем QML на диске.
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

/// `library` темы Aurorae SVG на этой машине ([`svg_library`]): смотрит
/// только имена файлов модулей, без чтения метаданных.
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

// ── Снимок рамки для плитки ─────────────────────────────────────────────
//
// Окно «Оформление окон» KDE не рисует рамку само: каждая плитка — живая
// рамка, которую рисует её модуль через QML-модуль KWin
// `org.kde.kwin.private.kdecoration` (типы Bridge, Settings, Decoration —
// неактивное окно сзади, активное спереди). Rexon делает то же самое, но
// снимком в отдельном процессе: сценарий `data/kdecoration-preview/
// preview.qml` под запускателем `qml` из Qt6 на платформе `offscreen`
// сохраняет кадр в PNG, плитка показывает его поверх общей подложки.
// Чужой модуль рамки так не попадает в процесс Rexon (как и при чтении
// метаданных), падение или зависание модуля стоит одного снимка — процесс
// убивается по сроку, плитка получает прежнюю картинку (образец из SVG у
// тем Aurorae SVG, заглушку у прочих).

/// Сценарий снимка (кладётся рядом с кешем картинок и отдаётся `qml`).
pub const SHOT_SCRIPT: &str = include_str!("../../../data/kdecoration-preview/preview.qml");
/// Имя файла сценария в кеше.
pub const SHOT_SCRIPT_NAME: &str = "kdecoration-preview.qml";
/// Запускатель QML из Qt6 по местам дистрибутивов. `/usr/bin/qml` нет в
/// списке намеренно: в Arch это запускатель Qt5.
pub const QML_RUNNERS: &[&str] = &[
    "/usr/lib/qt6/bin/qml",
    "/usr/lib64/qt6/bin/qml",
    "/usr/lib/x86_64-linux-gnu/qt6/bin/qml",
    "/usr/lib/aarch64-linux-gnu/qt6/bin/qml",
    "/usr/bin/qml6",
];
/// Каталоги модулей QML Qt6 по местам дистрибутивов.
pub const QML_IMPORT_DIRS: &[&str] = &[
    "/usr/lib/qt6/qml",
    "/usr/lib64/qt6/qml",
    "/usr/lib/x86_64-linux-gnu/qt6/qml",
    "/usr/lib/aarch64-linux-gnu/qt6/qml",
];
/// Файл QML-модуля превью рамок KWin внутри каталога модулей.
pub const PREVIEW_MODULE_QMLDIR: &str = "org/kde/kwin/private/kdecoration/qmldir";
/// Окружение процесса снимка: без экрана, программная отрисовка, масштаб 1
/// (иначе кадр выходит вдвое больше плитки).
pub const SHOT_ENV: &[(&str, &str)] =
    &[("QT_QPA_PLATFORM", "offscreen"), ("QT_QUICK_BACKEND", "software"), ("QT_SCALE_FACTOR", "1")];
/// Переменные масштаба, которые процессу снимка не передаются.
pub const SHOT_ENV_REMOVE: &[&str] = &["QT_SCREEN_SCALE_FACTORS", "QT_AUTO_SCREEN_SCALE_FACTOR"];
/// Сколько ждать снимка, секунд.
pub const SHOT_TIMEOUT_SECS: u64 = 15;
/// Размеры рамки KWin по порядку номеров `Settings.borderSizesIndex`.
pub const BORDER_SIZES: &[&str] = &["None", "NoSides", "Tiny", "Normal", "Large", "VeryLarge", "Huge", "VeryHuge", "Oversized"];
/// Номер размера `Normal` — его KDE берёт, когда размер «по теме».
pub const BORDER_NORMAL: usize = 3;

/// Что снимать: модуль, тема, заголовок окна, размер рамки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotRequest {
    pub library: String,
    pub theme: String,
    pub caption: String,
    pub border_index: usize,
}

/// Запускатель снимков: первый `qml` из [`QML_RUNNERS`], если рядом есть
/// модуль превью KWin ([`PREVIEW_MODULE_QMLDIR`]). `None` — снимать нечем
/// (KWin не стоит или Qt6 без запускателя).
pub fn shot_runner(is_file: &dyn Fn(&str) -> bool) -> Option<String> {
    let module = QML_IMPORT_DIRS.iter().any(|d| is_file(&format!("{d}/{PREVIEW_MODULE_QMLDIR}")));
    if !module {
        return None;
    }
    QML_RUNNERS.iter().find(|r| is_file(r)).map(|r| r.to_string())
}

/// Номер размера рамки для снимка по `kwinrc [org.kde.kdecoration2]`
/// `BorderSize` и `BorderSizeAuto`, как у плиток KDE: выбранный размер, а при
/// «по теме» (умолчание) — `Normal` (своего рекомендованного размера у Breeze
/// и Oxygen нет, KDE берёт тот же `Normal`).
pub fn border_index(border_size: Option<&str>, border_auto: Option<&str>) -> usize {
    let auto = border_auto.map(|v| v.trim() != "false").unwrap_or(true);
    if auto {
        return BORDER_NORMAL;
    }
    border_size
        .and_then(|s| BORDER_SIZES.iter().position(|b| *b == s.trim()))
        .unwrap_or(BORDER_NORMAL)
}

/// Что снимать для плитки `local_id`: встроенная рамка (`library/theme`) —
/// как есть, тема Aurorae SVG (имя каталога) — под движком `svg_library`.
/// `None` — плитка не из раздела рамок KDE.
pub fn shot_request(local_id: &str, caption: &str, svg_library: &str, border_index: usize) -> Option<ShotRequest> {
    let entry = local_id.strip_prefix("local:aurorae:").filter(|e| !e.is_empty())?;
    let (library, theme) = choice(entry, svg_library);
    Some(ShotRequest { library, theme, caption: caption.to_string(), border_index })
}

/// argv снимка: запускатель, сценарий, а после `--` — модуль, тема,
/// заголовок, номер размера рамки и куда сохранить.
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

/// Добавка к ключу кеша картинки рамки: кадр зависит от того, есть ли чем
/// снимать, от размера рамки и от цветов (`kdeglobals` — рамки KDE берут
/// цвета заголовка из схемы).
pub fn shot_cache_tag(runner: Option<&str>, border_index: usize, colors_stamp: i64) -> String {
    match runner {
        Some(_) => format!("shot\u{0}{border_index}\u{0}{colors_stamp}"),
        None => "noshot".to_string(),
    }
}

/// Что известно о снимках на этой машине — собирается один раз на пачку
/// плиток.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotSetup {
    pub runner: Option<String>,
    pub border_index: usize,
    pub svg_library: &'static str,
    /// [`shot_cache_tag`].
    pub tag: String,
}

impl ShotSetup {
    /// Живая машина: запускатель, `kwinrc` и отметка `kdeglobals` из
    /// `$XDG_CONFIG_HOME` (иначе `~/.config`).
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

    /// Что снимать для плитки (`None` — не рамка KDE либо снимать нечем).
    pub fn request(&self, local_id: &str, caption: &str) -> Option<ShotRequest> {
        self.runner.as_ref()?;
        shot_request(local_id, caption, self.svg_library, self.border_index)
    }
}

/// Снять рамку в `out` (PNG [`crate::local_preview::PREVIEW_WIDTH`] ×
/// [`crate::local_preview::PREVIEW_HEIGHT`]). Сценарий кладётся в `script_dir`.
/// Процесс, не уложившийся в [`SHOT_TIMEOUT_SECS`], убивается. `false` —
/// снимка нет (модуль или тема не нашлись, модуль упал, кадр не того
/// размера); недоделанный файл стирается.
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

// ── «Настроить…» у рамки ────────────────────────────────────────────────
//
// Окно «Оформление окон» KDE (сверено с kwin `src/kcms/decoration` и
// aurorae 6.8.2): у плитки кнопка правки, нажать её можно, когда у рамки есть
// модуль настроек. Модуль-рамка называет его сам — ключ `X-KDE-ConfigModule`
// в своих метаданных (Breeze — `kcm_breezedecoration`, Oxygen —
// `kcm_oxygendecoration`, у сторонних — свой). У тем Aurorae модуль один,
// `kcm_auroraedecoration`: у тем SVG он есть всегда (размер кнопок), у тем
// QML — только при `contents/ui/config.ui` и `contents/config/main.xml` в
// пакете темы. Кнопка собирает диалог KDE с этим модулем из пространства
// `org.kde.kdecoration3.kcm` и отдаёт ему тему; после сохранения KWin
// перечитывает настройки.
//
// Rexon открывает тот же диалог тем же QML-модулем KWin, отдельным
// процессом `qml` по сценарию `data/kdecoration-preview/configure.qml`.
// Запускателя нет — модуль-рамка открывается в `kcmshell6` по своему пути
// плагина; тему так не передать (`kcmshell6 --args` даёт модулю строки, а
// Aurorae ждёт словарь), поэтому тема Aurorae тогда открывает весь модуль KDE
// «Оформление окон» — там кнопка правки на её плитке. Рамки KDE рисует
// только KWin: вне Plasma кнопка выключена с причиной.

/// Сценарий окна настроек (кладётся в кеш и отдаётся `qml`).
pub const CONFIG_SCRIPT: &str = include_str!("../../../data/kdecoration-preview/configure.qml");
/// Имя файла сценария в кеше.
pub const CONFIG_SCRIPT_NAME: &str = "kdecoration-configure.qml";
/// Пространства модулей настроек рамок: Plasma 6.3+ и прежнее.
pub const KCM_NAMESPACES: &[&str] = &["org.kde.kdecoration3.kcm", "org.kde.kdecoration2.kcm"];
/// Модуль настроек тем Aurorae.
pub const AURORAE_KCM: &str = "kcm_auroraedecoration";
/// Модуль KDE «Оформление окон» целиком.
pub const DECORATION_KCM: &str = "kcm_kwindecoration";
/// Программа, показывающая модуль KDE отдельным окном.
pub const KCMSHELL: &str = "kcmshell6";

/// Рамка, у которой ищутся настройки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigTarget {
    pub library: String,
    pub theme: String,
    /// Тема движка Aurorae (модулю настроек нужна тема).
    pub engine_theme: bool,
    /// Модуль настроек; пусто — своих настроек у рамки нет.
    pub kcmodule: String,
    /// Заголовок окна настроек — имя рамки.
    pub caption: String,
}

/// Модуль настроек модуля-рамки по его метаданным (`X-KDE-ConfigModule`).
pub fn plugin_config_module(meta: &Value) -> String {
    meta.get("X-KDE-ConfigModule").and_then(Value::as_str).map(str::trim).unwrap_or("").to_string()
}

/// Рамка плитки `local_id` с путём `install_path` и именем `caption`.
/// `svg_library` — [`svg_library`] этой машины, `is_file` — есть ли файл,
/// `plugin_meta` — метаданные модуля-рамки по пути его файла. `None` — плитка
/// не из раздела рамок KDE.
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
        // Тема SVG: модуль настроек есть всегда.
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

/// Путь плагина модуля настроек `kcmodule` (`<пространство>/<модуль>`), если
/// его файл есть в одном из каталогов модулей Qt.
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

/// Что есть на машине для окна настроек.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFacts {
    /// Сеанс Plasma (окна рисует KWin).
    pub plasma: bool,
    /// Запускатель `qml` с модулем превью KWin ([`shot_runner`]).
    pub runner: Option<String>,
    /// [`kcm_plugin`] модуля настроек рамки.
    pub kcm: Option<String>,
    /// Есть ли [`KCMSHELL`].
    pub kcmshell: bool,
}

/// Кнопка «Настроить…» рамки.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ConfigButton {
    pub visible: bool,
    pub enabled: bool,
    pub text: String,
    /// Что откроется — или честная причина, почему нечем.
    pub tooltip: String,
    /// Команда; первым элементом — запускатель. Пусто — нажать нельзя.
    #[serde(skip)]
    pub argv: Vec<String>,
}

/// Кнопка «Настроить…» для рамки `target` (`None` — плитка не рамка KDE,
/// кнопки нет). `script` — путь сценария [`CONFIG_SCRIPT`] в кеше.
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

/// [`config_button`] на живой машине: стол — по `XDG_CURRENT_DESKTOP`,
/// модули — по каталогам модулей Qt, программы — по PATH. Сценарий
/// кладётся в кеш Rexon только для кнопки, которая его зовёт.
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

/// Положить сценарий окна настроек туда, куда смотрит `argv` кнопки (если
/// кнопка зовёт его). `false` — записать не вышло.
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

    /// Минимальная запись CBOR: текст.
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

    /// Файл модуля с заметкой ELF так, как её пишет Qt 6.
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
        bytes.truncate(bytes.len() - 20); // обрезанная карта
        assert_eq!(qt_plugin_metadata(&bytes), None);
        // Слишком глубокая вложенность не роняет разбор.
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
                // Тот же модуль из второго каталога — не второй раз.
                plain("org.kde.oxygen", "Oxygen 2"),
                // Чужой движок тем: его темы нам не перечислить.
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
        // Ключ не читали — как прежде, по `theme=`.
        assert_eq!(current(None, Some("__aurorae__svg__Sweet\n")), Current::Svg("Sweet".into()));
        assert_eq!(current(None, Some("kwin4_decoration_qml_plastik")), Current::Unknown);
        // Пустой `library` — Breeze, даже с прежней темой SVG в `theme=`.
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
        // Модуль без тем включён при любом `theme=`.
        assert!(breeze.is_current("org.kde.breeze/__aurorae__svg__Sweet"));
        assert!(breeze.is_current("org.kde.breeze/"));
        assert!(!breeze.is_current("org.kde.oxygen/Oxygen"));
        // Тема движка — по имени темы; пустое имя — тема по умолчанию.
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
        // Тема SVG — под движком этой машины.
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
        // Без движка v1 темы QML не показываются (KDE их тоже не покажет).
        let (decos, _) = decorations(&found, "");
        assert_eq!(decos.iter().map(Decoration::entry).collect::<Vec<_>>(), ["org.kde.breeze/Breeze"]);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
