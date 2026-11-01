//! Обои чужого шелла (Noctalia, DankMaterialShell): рисует ли он фон, и —
//! с согласия человека — «Отдать обои Rexon» / «Вернуть оболочке».
//!
//! ЗАЧЕМ. Оба шелла рисуют фон САМИ, своей полноэкранной поверхностью на
//! слое Background, и она ложится поверх обоев Rexon (разбор — над
//! `system::DesktopShell`). Драться за слой бесполезно, молчать нельзя.
//! Честный путь один: спросить человека и выключить обои шелла его же
//! штатным выключателем, запомнив прежнее значение, чтобы «Вернуть»
//! восстановило ровно его.
//!
//! Выключатели (сверено по исходникам noctalia v5.2.0 и DMS v1.6.2, разведка
//! 01.10.2026):
//!   * Noctalia — `[wallpaper] enabled = false`. Писать ТОЛЬКО в файл
//!     состояния `settings.toml` (`$NOCTALIA_STATE_HOME/noctalia/`,
//!     `$XDG_STATE_HOME/noctalia/` или `~/.local/state/noctalia/`): он
//!     грузится последним и перебивает `~/.config/noctalia/*.toml`, туда же
//!     пишет и GUI шелла. Правку в `~/.config` состояние перебило бы. Через
//!     `noctalia msg` выключить нельзя — такой команды нет. Шелл следит за
//!     файлом и перечитывает его сам.
//!   * DMS — `screenPreferences.wallpaper = []` в
//!     `$XDG_CONFIG_HOME/DankMaterialShell/settings.json` («Disable Built-in
//!     Wallpapers»). Пустой список убирает и основную поверхность, и вторую
//!     (`dms:blurwallpaper` на niri) — обе строятся по одному фильтру
//!     экранов. ⚠️ `dms ipc call wallpaper clear` НЕ годится: он стирает
//!     путь, а поверхность остаётся и заливается цветом. Через
//!     `dms ipc call settings set` массив не поставить (массивы там
//!     запрещены). Файл шелл перечитывает сам.
//!
//! Правило записи: правится ОДИН управляемый ключ, остальной файл остаётся
//! байт в байт; правка проверяется разбором до и после (новое значение
//! обязано отличаться от старого РОВНО этим ключом — иначе отказ, а не
//! порча); прежнее содержимое кладётся рядом резервной копией; запись —
//! подменой имени (`utils::atomic_write`, символьная ссылка разворачивается
//! в цель). Прежнее значение ключа запоминается в
//! `$XDG_STATE_HOME/rexon/shell-wallpaper.json` ДО записи в файл шелла:
//! упади запись памятки после правки — вернуть было бы нечем.
//!
//! ⚠️ Граница: ни один шелл на машине разработки не установлен. Всё здесь
//! проверено подставными файлами (`tests/desktop_shell.rs`) и стендом с
//! приватным HOME, живьём — нет.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::i18n::{tr, trf};
use crate::system::{detect_shell, DesktopShell, ShellDetection, ShellFacts, SHELL_ENV_NAMES};
use crate::wallpaper_engine::{wallpaper_layer_owner, WallpaperOwner};

// ============================================================================
// Где лежат настройки шеллов
// ============================================================================

/// Домашние корни, от которых шеллы считают свои пути. Пустая строка —
/// переменная не задана (пустая = не задана, как у XDG).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellHome {
    pub home: String,
    pub xdg_config_home: String,
    pub xdg_state_home: String,
    /// `NOCTALIA_CONFIG_HOME` — корень вида XDG (шелл читает `<он>/noctalia`).
    pub noctalia_config_home: String,
    /// `NOCTALIA_STATE_HOME` — то же для состояния.
    pub noctalia_state_home: String,
}

impl ShellHome {
    /// Корни по функции чтения окружения — так их добывает и живой вызов, и
    /// сверка.
    pub fn from_env(get: impl Fn(&str) -> String) -> ShellHome {
        ShellHome {
            home: get("HOME"),
            xdg_config_home: get("XDG_CONFIG_HOME"),
            xdg_state_home: get("XDG_STATE_HOME"),
            noctalia_config_home: get("NOCTALIA_CONFIG_HOME"),
            noctalia_state_home: get("NOCTALIA_STATE_HOME"),
        }
    }

    /// Корни этого процесса.
    pub fn live() -> ShellHome {
        ShellHome::from_env(|k| std::env::var(k).unwrap_or_default())
    }

    fn config_home(&self) -> PathBuf {
        if self.xdg_config_home.is_empty() {
            Path::new(&self.home).join(".config")
        } else {
            PathBuf::from(&self.xdg_config_home)
        }
    }

    fn state_home(&self) -> PathBuf {
        if self.xdg_state_home.is_empty() {
            Path::new(&self.home).join(".local/state")
        } else {
            PathBuf::from(&self.xdg_state_home)
        }
    }

    /// Каталог конфигурации Noctalia (все `*.toml` в нём грузятся по
    /// алфавиту). Порядок корней — как у самого шелла.
    pub fn noctalia_config_dir(&self) -> PathBuf {
        if !self.noctalia_config_home.is_empty() {
            Path::new(&self.noctalia_config_home).join("noctalia")
        } else {
            self.config_home().join("noctalia")
        }
    }

    /// Файл состояния Noctalia `settings.toml` — слой, который перебивает
    /// конфигурацию; сюда и пишем.
    pub fn noctalia_state_file(&self) -> PathBuf {
        let dir = if !self.noctalia_state_home.is_empty() {
            Path::new(&self.noctalia_state_home).join("noctalia")
        } else {
            self.state_home().join("noctalia")
        };
        dir.join("settings.toml")
    }

    /// Настройки DankMaterialShell (`StandardPaths.ConfigLocation`).
    pub fn dms_settings_file(&self) -> PathBuf {
        self.config_home().join("DankMaterialShell/settings.json")
    }

    /// Файл, в котором правит Rexon для этого шелла.
    pub fn managed_file(&self, shell: DesktopShell) -> PathBuf {
        match shell {
            DesktopShell::Noctalia => self.noctalia_state_file(),
            DesktopShell::DankMaterialShell => self.dms_settings_file(),
        }
    }

    /// Памятки Rexon: чей выключатель и какое значение было до нас (список —
    /// по записи на шелл).
    pub fn handover_file(&self) -> PathBuf {
        self.state_home().join("rexon/shell-wallpaper.json")
    }
}

/// Имя резервной копии рядом с файлом шелла.
pub fn backup_path(file: &Path) -> PathBuf {
    let mut name = file.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".rexon-backup");
    file.with_file_name(name)
}

// ============================================================================
// Рисует ли шелл обои
// ============================================================================

/// Значение `[wallpaper] enabled` в разобранной таблице Noctalia.
fn noctalia_enabled_in(table: &toml::Table) -> Option<bool> {
    table
        .get("wallpaper")
        .and_then(|w| w.as_table())
        .and_then(|w| w.get("enabled"))
        .and_then(|v| v.as_bool())
}

/// Рисует ли Noctalia обои при таких слоях настроек.
///
/// `config_layers` — тексты конфигурации В ПОРЯДКЕ ЗАГРУЗКИ (как их сложил
/// [`noctalia_config_layers`]: для каждого файла сначала его `[include]`,
/// потом он сам); `state` — `settings.toml` или `None`, если файла нет.
/// Побеждает последний слой, где ключ задан; по умолчанию обои включены.
///
/// `None` — ответить нельзя (какой-то слой не разобрался: шелл в таком
/// случае ведёт себя по-своему, и гадать за него мы не будем). Вызывающий
/// трактует `None` как «рисует» — осторожная сторона, см.
/// [`wallpaper_layer_owner`].
///
/// ⚠️ Помониторные `[wallpaper.monitor.<выход>] enabled = false` здесь не
/// учитываются: ответ «рисует хотя бы на одном выходе» при общем `enabled =
/// true` верен, а какие выходы есть, ядро не знает.
pub fn noctalia_draws_wallpaper(config_layers: &[String], state: Option<&str>) -> Option<bool> {
    let mut enabled = true;
    for text in config_layers.iter().map(String::as_str).chain(state) {
        let table: toml::Table = toml::from_str(text).ok()?;
        if let Some(v) = noctalia_enabled_in(&table) {
            enabled = v;
        }
    }
    Some(enabled)
}

/// Рисует ли DMS обои при таком `settings.json` (`None` — файла нет).
///
/// Ключа нет, `null` или список с экранами — рисует (на всех либо на
/// названных); пустой список — не рисует нигде. `None` — файл не разобран.
pub fn dms_draws_wallpaper(settings: Option<&str>) -> Option<bool> {
    let Some(text) = settings else {
        return Some(true);
    };
    let value: Value = serde_json::from_str(text).ok()?;
    let prefs = value.get("screenPreferences").and_then(|p| p.get("wallpaper"));
    Some(!matches!(prefs, Some(Value::Array(a)) if a.is_empty()))
}

/// Тексты конфигурации Noctalia в порядке загрузки: все `*.toml` каталога
/// по алфавиту, для каждого — сначала его `[include]` (файлы и каталоги,
/// пути от каталога включающего файла, `~` раскрывается), потом он сам.
/// Если хоть один корневой файл ставит `[include] autoload = false`,
/// грузятся только такие файлы (так делает шелл).
///
/// Нечитаемый файл пропускается молча, неразборный — попадает в список как
/// есть, чтобы [`noctalia_draws_wallpaper`] честно ответила «не знаю».
/// Пути с `$VAR` не раскрываются (редкость; такой файл просто не учтён).
pub fn noctalia_config_layers(home: &ShellHome) -> Vec<String> {
    let dir = home.noctalia_config_dir();
    let roots = toml_files_in(&dir);
    let parsed: Vec<(PathBuf, String, Option<toml::Table>)> = roots
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let table = toml::from_str::<toml::Table>(&text).ok();
            Some((p, text, table))
        })
        .collect();
    let no_autoload = |t: &Option<toml::Table>| {
        t.as_ref()
            .and_then(|t| t.get("include"))
            .and_then(|i| i.get("autoload"))
            .and_then(|a| a.as_bool())
            == Some(false)
    };
    let selective = parsed.iter().any(|(_, _, t)| no_autoload(t));
    let mut out = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for (path, text, table) in &parsed {
        if selective && !no_autoload(table) {
            continue;
        }
        push_layer(home, path, text, table.as_ref(), &mut out, &mut seen, 0);
    }
    out
}

fn toml_files_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "toml") && p.is_file())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn push_layer(
    home: &ShellHome,
    path: &Path,
    text: &str,
    table: Option<&toml::Table>,
    out: &mut Vec<String>,
    seen: &mut Vec<PathBuf>,
    depth: usize,
) {
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if seen.contains(&canon) || depth > 16 {
        return;
    }
    seen.push(canon);
    let includes: Vec<String> = table
        .and_then(|t| t.get("include"))
        .and_then(|i| i.get("files"))
        .and_then(|f| f.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let base = path.parent().unwrap_or(Path::new("/")).to_path_buf();
    for inc in includes {
        if inc.contains('$') {
            continue;
        }
        let p = if let Some(rest) = inc.strip_prefix("~/") {
            Path::new(&home.home).join(rest)
        } else if inc == "~" {
            PathBuf::from(&home.home)
        } else {
            base.join(&inc)
        };
        let files = if p.is_dir() { toml_files_in(&p) } else { vec![p] };
        for f in files {
            if let Ok(t) = std::fs::read_to_string(&f) {
                let tb = toml::from_str::<toml::Table>(&t).ok();
                push_layer(home, &f, &t, tb.as_ref(), out, seen, depth + 1);
            }
        }
    }
    out.push(text.to_string());
}

/// Рисует ли шелл обои — по настоящим файлам под `home`.
pub fn shell_draws_wallpaper(shell: DesktopShell, home: &ShellHome) -> Option<bool> {
    match shell {
        DesktopShell::Noctalia => {
            let layers = noctalia_config_layers(home);
            let state = read_optional(&home.noctalia_state_file()).ok()?;
            noctalia_draws_wallpaper(&layers, state.as_deref())
        }
        DesktopShell::DankMaterialShell => {
            let settings = read_optional(&home.dms_settings_file()).ok()?;
            dms_draws_wallpaper(settings.as_deref())
        }
    }
}

/// Текст файла; `Ok(None)` — файла нет; `Err` — есть, но не читается.
fn read_optional(path: &Path) -> Result<Option<String>, std::io::Error> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

// ============================================================================
// Правка одного ключа: что было до нас
// ============================================================================

/// Что стояло на месте управляемого ключа до правки Rexon. Ровно это и
/// возвращает «Вернуть оболочке».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Previous {
    /// Файла не было — Rexon создал его сам; возврат снимает ключ и, если
    /// в файле больше ничего нет, удаляет файл.
    FileAbsent,
    /// Ключа не было. `parent_created` — Rexon завёл и родителя (таблицу
    /// `[wallpaper]` у Noctalia, объект `screenPreferences` у DMS);
    /// `prefix` — что Rexon дописал перед новой таблицей в конец файла
    /// (переводы строк), чтобы возврат снял ровно это.
    KeyAbsent {
        parent_created: bool,
        #[serde(default)]
        prefix: String,
    },
    /// Ключ был — его значение ДОСЛОВНО, как стояло в файле.
    Value { raw: String },
}

/// Итог возврата: новый текст файла либо «удалить файл» (его создал Rexon,
/// и после возврата в нём пусто).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restored {
    Write(String),
    Delete,
}

/// Почему править нельзя. Тексты — для человека.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// Файл не разбирается (шелл в таком виде его и сам не прочтёт).
    Unparsable,
    /// Файл разбирается, но записан в форме, которую Rexon не правит
    /// безопасно (ключ через точку, таблица в строку, не тот тип…).
    UnsafeForm,
}

impl EditError {
    pub fn message(&self) -> String {
        match self {
            EditError::Unparsable => tr("the settings file could not be read as valid settings"),
            EditError::UnsafeForm => {
                tr("the settings file is written in a form Rexon cannot edit safely")
            }
        }
    }
}

// ── Noctalia: TOML построчно ───────────────────────────────────────────────

/// Строки текста с их концами (`\n` остаётся в строке).
fn lines_with_ends(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// Имя таблицы в строке-заголовке `[имя]` (без `[[…]]`), иначе `None`.
fn toml_header(line: &str) -> Option<Option<String>> {
    let t = line.trim_start();
    if !t.starts_with('[') {
        return None;
    }
    if t.starts_with("[[") {
        return Some(None);
    }
    let close = t.find(']')?;
    let rest = t[close + 1..].trim();
    if !(rest.is_empty() || rest.starts_with('#')) {
        return Some(None);
    }
    Some(Some(t[1..close].trim().to_string()))
}

/// Строки внутри многострочных строк TOML (`"""`/`'''`) — не заголовки и не
/// ключи. Грубый счёт кавычек; ошибку счёта ловит сверка разбором.
fn toml_line_kinds(lines: &[&str]) -> Vec<bool> {
    let mut inside: Option<&str> = None;
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        out.push(inside.is_none());
        let mut rest: &str = line;
        loop {
            match inside {
                None => {
                    let a = rest.find("\"\"\"");
                    let b = rest.find("'''");
                    let (pos, q) = match (a, b) {
                        (Some(x), Some(y)) if x <= y => (x, "\"\"\""),
                        (Some(_), Some(y)) => (y, "'''"),
                        (Some(x), None) => (x, "\"\"\""),
                        (None, Some(y)) => (y, "'''"),
                        (None, None) => break,
                    };
                    inside = Some(q);
                    rest = &rest[pos + 3..];
                }
                Some(q) => match rest.find(q) {
                    Some(pos) => {
                        inside = None;
                        rest = &rest[pos + 3..];
                    }
                    None => break,
                },
            }
        }
    }
    out
}

/// Где в тексте таблица `[wallpaper]` и ключ `enabled` в ней.
struct NoctaliaSpot {
    /// Номер строки заголовка `[wallpaper]`.
    header: Option<usize>,
    /// Номер строки `enabled = …` в этой таблице и байтовые границы
    /// значения внутри строки.
    key: Option<(usize, usize, usize)>,
}

fn noctalia_spot(lines: &[&str]) -> NoctaliaSpot {
    let plain = toml_line_kinds(lines);
    let mut header = None;
    let mut key = None;
    let mut in_wallpaper = false;
    for (i, line) in lines.iter().enumerate() {
        if !plain[i] {
            continue;
        }
        if let Some(name) = toml_header(line) {
            in_wallpaper = name.as_deref() == Some("wallpaper");
            if in_wallpaper && header.is_none() {
                header = Some(i);
            }
            continue;
        }
        if !in_wallpaper || key.is_some() {
            continue;
        }
        let lead = line.len() - line.trim_start().len();
        let body = &line[lead..];
        let Some(after) = body.strip_prefix("enabled") else { continue };
        let ws = after.len() - after.trim_start().len();
        let Some(after_eq) = after[ws..].strip_prefix('=') else { continue };
        let vs = lead + "enabled".len() + ws + 1 + (after_eq.len() - after_eq.trim_start().len());
        let ve = line[vs..]
            .find(|c: char| c.is_whitespace() || c == '#')
            .map(|p| vs + p)
            .unwrap_or(line.len());
        key = Some((i, vs, ve));
    }
    NoctaliaSpot { header, key }
}

/// Разобранная таблица с подменённым `[wallpaper] enabled`: `Some(v)` —
/// поставить, `None` — снять ключ (и пустую таблицу, если `drop_empty`).
fn noctalia_expect(base: &toml::Table, value: Option<toml::Value>, drop_empty: bool) -> toml::Table {
    let mut t = base.clone();
    match value {
        Some(v) => {
            let w = t
                .entry("wallpaper")
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            if let Some(w) = w.as_table_mut() {
                w.insert("enabled".to_string(), v);
            }
        }
        None => {
            if let Some(w) = t.get_mut("wallpaper").and_then(|w| w.as_table_mut()) {
                w.remove("enabled");
                if drop_empty && w.is_empty() {
                    t.remove("wallpaper");
                }
            }
        }
    }
    t
}

fn parse_toml(text: &str) -> Result<toml::Table, EditError> {
    toml::from_str::<toml::Table>(text).map_err(|_| EditError::Unparsable)
}

/// Выключить обои Noctalia в тексте `settings.toml` (`None` — файла нет).
/// Возвращает новый текст и то, что стояло до правки.
pub fn noctalia_disable(text: Option<&str>) -> Result<(String, Previous), EditError> {
    let Some(text) = text else {
        let new = "[wallpaper]\nenabled = false\n".to_string();
        return Ok((new, Previous::FileAbsent));
    };
    let before = parse_toml(text)?;
    if let Some(w) = before.get("wallpaper") {
        if !w.is_table() {
            return Err(EditError::UnsafeForm);
        }
        if let Some(v) = w.get("enabled") {
            if !v.is_bool() {
                return Err(EditError::UnsafeForm);
            }
        }
    }
    let lines = lines_with_ends(text);
    let spot = noctalia_spot(&lines);
    let (new, prev) = match (spot.header, spot.key) {
        (_, Some((i, vs, ve))) => {
            let raw = lines[i][vs..ve].to_string();
            let mut out = String::with_capacity(text.len());
            for (j, l) in lines.iter().enumerate() {
                if j == i {
                    out.push_str(&l[..vs]);
                    out.push_str("false");
                    out.push_str(&l[ve..]);
                } else {
                    out.push_str(l);
                }
            }
            (out, Previous::Value { raw })
        }
        (Some(h), None) => {
            let prefix = if lines[h].ends_with('\n') { "" } else { "\n" };
            let mut out = String::with_capacity(text.len() + 20);
            for (j, l) in lines.iter().enumerate() {
                out.push_str(l);
                if j == h {
                    out.push_str(prefix);
                    out.push_str("enabled = false\n");
                }
            }
            (out, Previous::KeyAbsent { parent_created: false, prefix: prefix.to_string() })
        }
        (None, None) => {
            let prefix = if text.is_empty() {
                ""
            } else if text.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            let out = format!("{text}{prefix}[wallpaper]\nenabled = false\n");
            (out, Previous::KeyAbsent { parent_created: true, prefix: prefix.to_string() })
        }
    };
    let after = parse_toml(&new).map_err(|_| EditError::UnsafeForm)?;
    if after != noctalia_expect(&before, Some(toml::Value::Boolean(false)), false) {
        return Err(EditError::UnsafeForm);
    }
    Ok((new, prev))
}

/// Вернуть обоям Noctalia то, что стояло до правки Rexon.
pub fn noctalia_restore(text: Option<&str>, prev: &Previous) -> Result<Restored, EditError> {
    let Some(text) = text else {
        // Файл удалили без нас. Было «ключа нет» или «файла нет» — так и
        // осталось. Было значение — ставим его обратно в новый файл.
        return Ok(match prev {
            Previous::Value { raw } => Restored::Write(format!("[wallpaper]\nenabled = {raw}\n")),
            _ => Restored::Delete,
        });
    };
    let before = parse_toml(text)?;
    let lines = lines_with_ends(text);
    let spot = noctalia_spot(&lines);
    let (new, expect) = match prev {
        Previous::Value { raw } => {
            let raw_value: toml::Value = toml::from_str::<toml::Table>(&format!("v = {raw}"))
                .ok()
                .and_then(|t| t.get("v").cloned())
                .ok_or(EditError::UnsafeForm)?;
            let new = match (spot.header, spot.key) {
                (_, Some((i, vs, ve))) => {
                    let mut out = String::with_capacity(text.len());
                    for (j, l) in lines.iter().enumerate() {
                        if j == i {
                            out.push_str(&l[..vs]);
                            out.push_str(raw);
                            out.push_str(&l[ve..]);
                        } else {
                            out.push_str(l);
                        }
                    }
                    out
                }
                (Some(h), None) => {
                    let mut out = String::new();
                    for (j, l) in lines.iter().enumerate() {
                        out.push_str(l);
                        if j == h {
                            if !l.ends_with('\n') {
                                out.push('\n');
                            }
                            out.push_str(&format!("enabled = {raw}\n"));
                        }
                    }
                    out
                }
                (None, None) => {
                    let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
                    format!("{text}{sep}[wallpaper]\nenabled = {raw}\n")
                }
            };
            (new, noctalia_expect(&before, Some(raw_value), false))
        }
        Previous::KeyAbsent { parent_created, prefix } => {
            let new = noctalia_remove_key(&lines, &spot, *parent_created, prefix);
            (new, noctalia_expect(&before, None, *parent_created))
        }
        Previous::FileAbsent => {
            let new = noctalia_remove_key(&lines, &spot, true, "");
            let expect = noctalia_expect(&before, None, true);
            if new.trim().is_empty() && expect.is_empty() {
                return Ok(Restored::Delete);
            }
            (new, expect)
        }
    };
    let after = parse_toml(&new).map_err(|_| EditError::UnsafeForm)?;
    if after != expect {
        return Err(EditError::UnsafeForm);
    }
    Ok(Restored::Write(new))
}

/// Снять строку `enabled` из `[wallpaper]`; если таблицу заводил Rexon и
/// она опустела — снять и заголовок; если правка стояла в самом конце —
/// снять и дописанный тогда `prefix`.
fn noctalia_remove_key(lines: &[&str], spot: &NoctaliaSpot, drop_empty: bool, prefix: &str) -> String {
    let Some((ki, _, _)) = spot.key else {
        return lines.concat();
    };
    let mut drop: Vec<usize> = vec![ki];
    if drop_empty {
        if let Some(h) = spot.header {
            // Таблица пуста, если до следующего заголовка (или конца) нет
            // ничего, кроме снимаемой строки.
            let end = (h + 1..lines.len())
                .find(|&j| toml_header(lines[j]).is_some())
                .unwrap_or(lines.len());
            // Пустые строки внутри не в счёт (их мог добавить сам шелл,
            // переписав файл) и остаются на месте — снимаются только
            // заголовок и ключ.
            if (h + 1..end).all(|j| j == ki || lines[j].trim().is_empty()) {
                drop.push(h);
            }
        }
    }
    // Правка стояла в самом конце, если после первой снятой строки не
    // остаётся ничего, кроме снятых.
    let first = drop.iter().copied().min().unwrap_or(lines.len());
    let at_end = (first..lines.len()).all(|j| drop.contains(&j));
    let mut out: String = lines
        .iter()
        .enumerate()
        .filter(|(j, _)| !drop.contains(j))
        .map(|(_, l)| *l)
        .collect();
    if at_end && !prefix.is_empty() && out.ends_with(prefix) {
        out.truncate(out.len() - prefix.len());
    }
    out
}

// ── DMS: JSON по байтовым границам ─────────────────────────────────────────

/// Член объекта JSON: ключ и байтовые границы значения.
#[derive(Debug, Clone)]
struct Member {
    key: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// Конец строки JSON, начинающейся в `i` (на кавычке): индекс после неё.
fn string_end(b: &[u8], mut i: usize) -> Option<usize> {
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// Конец значения JSON, начинающегося в `i`.
fn value_end(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'"' => string_end(b, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'"' => {
                        j = string_end(b, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            None
        }
        _ => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r') {
                j += 1;
            }
            Some(j)
        }
    }
}

/// Члены объекта, открытого в `open` (`{`).
fn object_members(text: &str, open: usize) -> Option<Vec<Member>> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = skip_ws(b, open + 1);
    if b.get(i) == Some(&b'}') {
        return Some(out);
    }
    loop {
        if b.get(i) != Some(&b'"') {
            return None;
        }
        let ke = string_end(b, i)?;
        let key: String = serde_json::from_str(&text[i..ke]).ok()?;
        let mut j = skip_ws(b, ke);
        if b.get(j) != Some(&b':') {
            return None;
        }
        j = skip_ws(b, j + 1);
        let ve = value_end(b, j)?;
        out.push(Member { key, key_start: i, value_start: j, value_end: ve });
        let k = skip_ws(b, ve);
        match b.get(k) {
            Some(b',') => i = skip_ws(b, k + 1),
            Some(b'}') => return Some(out),
            _ => return None,
        }
    }
}

/// Вставить член первым в объект `{` на `open` — с тем же отступом, что у
/// бывшего первого члена.
fn insert_member(text: &str, open: usize, members: &[Member], member: &str) -> String {
    let mut out = String::with_capacity(text.len() + member.len() + 8);
    out.push_str(&text[..open + 1]);
    if let Some(first) = members.first() {
        let ws = &text[open + 1..first.key_start];
        out.push_str(ws);
        out.push_str(member);
        out.push(',');
        out.push_str(&text[open + 1..]);
    } else {
        out.push_str(member);
        out.push_str(&text[open + 1..]);
    }
    out
}

/// Снять член `idx` из объекта `{` на `open` вместе с его запятой.
fn remove_member(text: &str, open: usize, members: &[Member], idx: usize) -> String {
    let b = text.as_bytes();
    let m = &members[idx];
    let (start, end) = if members.len() == 1 {
        (open + 1, m.value_end)
    } else if idx + 1 < members.len() {
        let start = if idx == 0 {
            open + 1
        } else {
            skip_ws(b, members[idx - 1].value_end) + 1
        };
        let end = skip_ws(b, m.value_end) + 1;
        (start, end)
    } else {
        (skip_ws(b, members[idx - 1].value_end), m.value_end)
    };
    format!("{}{}", &text[..start], &text[end..])
}

/// Где объект `screenPreferences` и ключ `wallpaper` в нём.
struct DmsSpot {
    root_open: usize,
    root: Vec<Member>,
    prefs: Option<usize>,
    prefs_members: Vec<Member>,
    wallpaper: Option<usize>,
}

fn dms_spot(text: &str) -> Result<DmsSpot, EditError> {
    let b = text.as_bytes();
    let root_open = skip_ws(b, 0);
    if b.get(root_open) != Some(&b'{') {
        return Err(EditError::UnsafeForm);
    }
    let root = object_members(text, root_open).ok_or(EditError::UnsafeForm)?;
    let prefs = root.iter().position(|m| m.key == "screenPreferences");
    let mut prefs_members = Vec::new();
    let mut wallpaper = None;
    if let Some(p) = prefs {
        let at = root[p].value_start;
        if b[at] != b'{' {
            return Err(EditError::UnsafeForm);
        }
        prefs_members = object_members(text, at).ok_or(EditError::UnsafeForm)?;
        wallpaper = prefs_members.iter().position(|m| m.key == "wallpaper");
    }
    Ok(DmsSpot { root_open, root, prefs, prefs_members, wallpaper })
}

fn parse_json(text: &str) -> Result<Value, EditError> {
    let v: Value = serde_json::from_str(text).map_err(|_| EditError::Unparsable)?;
    if !v.is_object() {
        return Err(EditError::UnsafeForm);
    }
    Ok(v)
}

/// Разобранные настройки с подменённым `screenPreferences.wallpaper`.
fn dms_expect(base: &Value, value: Option<Value>, drop_empty: bool) -> Value {
    let mut v = base.clone();
    let root = v.as_object_mut().expect("корень — объект (проверено разбором)");
    match value {
        Some(x) => {
            let p = root
                .entry("screenPreferences")
                .or_insert_with(|| Value::Object(Default::default()));
            if let Some(p) = p.as_object_mut() {
                p.insert("wallpaper".to_string(), x);
            }
        }
        None => {
            let mut empty = false;
            if let Some(p) = root.get_mut("screenPreferences").and_then(|p| p.as_object_mut()) {
                p.remove("wallpaper");
                empty = p.is_empty();
            }
            if drop_empty && empty {
                root.remove("screenPreferences");
            }
        }
    }
    v
}

const DMS_EMPTY: &str = "[]";

/// Выключить встроенные обои DMS в тексте `settings.json` (`None` — файла
/// нет).
pub fn dms_disable(text: Option<&str>) -> Result<(String, Previous), EditError> {
    let Some(text) = text else {
        return Ok(("{\"screenPreferences\": {\"wallpaper\": []}}\n".to_string(), Previous::FileAbsent));
    };
    let before = parse_json(text)?;
    let spot = dms_spot(text)?;
    let (new, prev) = match (spot.prefs, spot.wallpaper) {
        (Some(_), Some(w)) => {
            let m = &spot.prefs_members[w];
            let raw = text[m.value_start..m.value_end].to_string();
            let new = format!("{}{DMS_EMPTY}{}", &text[..m.value_start], &text[m.value_end..]);
            (new, Previous::Value { raw })
        }
        (Some(p), None) => {
            let open = spot.root[p].value_start;
            let new = insert_member(text, open, &spot.prefs_members, "\"wallpaper\": []");
            (new, Previous::KeyAbsent { parent_created: false, prefix: String::new() })
        }
        (None, _) => {
            let new = insert_member(
                text,
                spot.root_open,
                &spot.root,
                "\"screenPreferences\": {\"wallpaper\": []}",
            );
            (new, Previous::KeyAbsent { parent_created: true, prefix: String::new() })
        }
    };
    let after = parse_json(&new).map_err(|_| EditError::UnsafeForm)?;
    if after != dms_expect(&before, Some(Value::Array(Vec::new())), false) {
        return Err(EditError::UnsafeForm);
    }
    Ok((new, prev))
}

/// Вернуть встроенным обоям DMS то, что стояло до правки Rexon.
pub fn dms_restore(text: Option<&str>, prev: &Previous) -> Result<Restored, EditError> {
    let Some(text) = text else {
        return Ok(match prev {
            Previous::Value { raw } => {
                Restored::Write(format!("{{\"screenPreferences\": {{\"wallpaper\": {raw}}}}}\n"))
            }
            _ => Restored::Delete,
        });
    };
    let before = parse_json(text)?;
    let spot = dms_spot(text)?;
    let (new, expect) = match prev {
        Previous::Value { raw } => {
            let raw_value: Value = serde_json::from_str(raw).map_err(|_| EditError::UnsafeForm)?;
            let new = match (spot.prefs, spot.wallpaper) {
                (Some(_), Some(w)) => {
                    let m = &spot.prefs_members[w];
                    format!("{}{raw}{}", &text[..m.value_start], &text[m.value_end..])
                }
                (Some(p), None) => insert_member(
                    text,
                    spot.root[p].value_start,
                    &spot.prefs_members,
                    &format!("\"wallpaper\": {raw}"),
                ),
                (None, _) => insert_member(
                    text,
                    spot.root_open,
                    &spot.root,
                    &format!("\"screenPreferences\": {{\"wallpaper\": {raw}}}"),
                ),
            };
            (new, dms_expect(&before, Some(raw_value), false))
        }
        Previous::KeyAbsent { .. } | Previous::FileAbsent => {
            let drop_parent = matches!(
                prev,
                Previous::FileAbsent | Previous::KeyAbsent { parent_created: true, .. }
            );
            let mut new = text.to_string();
            if let (Some(p), Some(w)) = (spot.prefs, spot.wallpaper) {
                let open = spot.root[p].value_start;
                new = remove_member(text, open, &spot.prefs_members, w);
                if drop_parent && spot.prefs_members.len() == 1 {
                    let again = dms_spot(&new)?;
                    if let Some(p2) = again.prefs {
                        new = remove_member(&new, again.root_open, &again.root, p2);
                    }
                }
            }
            let expect = dms_expect(&before, None, drop_parent);
            if matches!(prev, Previous::FileAbsent)
                && expect.as_object().is_some_and(|o| o.is_empty())
            {
                return Ok(Restored::Delete);
            }
            (new, expect)
        }
    };
    let after = parse_json(&new).map_err(|_| EditError::UnsafeForm)?;
    if after != expect {
        return Err(EditError::UnsafeForm);
    }
    Ok(Restored::Write(new))
}

// ============================================================================
// Исполнение: файлы на диске
// ============================================================================

/// Памятка Rexon о выключенных обоях шелла.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handover {
    /// `noctalia` | `dms`.
    pub shell: String,
    /// Какой файл правили (как назван, до разворота ссылки).
    pub file: String,
    /// Что стояло до правки.
    pub previous: Previous,
}

impl Handover {
    pub fn desktop_shell(&self) -> Option<DesktopShell> {
        DesktopShell::all().iter().copied().find(|s| s.as_str() == self.shell)
    }
}

/// Все памятки: по одной на шелл, чьи обои Rexon сейчас держит
/// выключенными. Шеллов на машине бывает больше одного (DMS под niri,
/// Noctalia под Hyprland), и выключение одного не должно затирать память о
/// другом.
pub fn read_handovers(home: &ShellHome) -> Vec<Handover> {
    std::fs::read_to_string(home.handover_file())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Памятка об этом шелле, если Rexon держит его обои выключенными.
pub fn read_handover(home: &ShellHome, shell: DesktopShell) -> Option<Handover> {
    read_handovers(home).into_iter().find(|h| h.desktop_shell() == Some(shell))
}

/// Записать памятки; пусто — файла памяток нет вовсе.
fn write_handovers(home: &ShellHome, list: &[Handover]) -> Result<(), String> {
    if list.is_empty() {
        return match std::fs::remove_file(home.handover_file()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let text = serde_json::to_string_pretty(list).expect("памятки сериализуются");
    write_text(&home.handover_file(), &text)
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    crate::utils::atomic_write(&path.to_string_lossy(), text).map_err(|e| format!("{}: {e}", path.display()))
}

fn shell_edit_error(shell: DesktopShell, reason: &str) -> String {
    // TRANSLATORS: %1 is the shell name (Noctalia, DankMaterialShell), %2 the reason.
    trf("Could not change the settings of %1: %2. Nothing was changed.", &[&shell.label(), &reason])
}

/// Разрешена ли запись: существующий файл только для чтения — отказ
/// (DMS сам так помечает настройки, которые человек закрыл от правок).
fn writable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => !m.permissions().readonly(),
        Err(_) => true,
    }
}

/// «Отдать обои Rexon»: выключить обои шелла, запомнив прежнее значение.
///
/// Порядок: прочитать → посчитать правку (с проверкой разбором) →
/// записать памятку → резервная копия → подмена файла. Неудача на любом шаге
/// до подмены ничего у шелла не меняет; неудача подмены снимает памятку.
/// Возвращает текст для человека.
pub fn hand_over(shell: DesktopShell, home: &ShellHome) -> Result<String, String> {
    let before = read_handovers(home);
    let ours = before.iter().any(|h| h.desktop_shell() == Some(shell));
    if ours && shell_draws_wallpaper(shell, home) == Some(false) {
        // Уже выключены нами — вторая правка затёрла бы в памятке значение,
        // стоявшее ДО Rexon, нашим же `false`.
        return Ok(hand_over_done(shell));
    }
    let file = home.managed_file(shell);
    let current = read_optional(&file)
        .map_err(|_| shell_edit_error(shell, &tr("the settings file could not be read")))?;
    if current.is_some() && !writable(&file) {
        return Err(shell_edit_error(shell, &tr("the settings file is read-only")));
    }
    let edited = match shell {
        DesktopShell::Noctalia => noctalia_disable(current.as_deref()),
        DesktopShell::DankMaterialShell => dms_disable(current.as_deref()),
    };
    let (new_text, previous) = edited.map_err(|e| shell_edit_error(shell, &e.message()))?;
    let mut after: Vec<Handover> =
        before.iter().filter(|h| h.desktop_shell() != Some(shell)).cloned().collect();
    after.push(Handover {
        shell: shell.as_str().to_string(),
        file: file.to_string_lossy().into_owned(),
        previous,
    });
    let write_failed = || shell_edit_error(shell, &tr("the settings file could not be written"));
    write_handovers(home, &after).map_err(|_| write_failed())?;
    if let Some(old) = &current {
        if write_text(&backup_path(&file), old).is_err() {
            let _ = write_handovers(home, &before);
            return Err(write_failed());
        }
    }
    if write_text(&file, &new_text).is_err() {
        let _ = write_handovers(home, &before);
        return Err(write_failed());
    }
    Ok(hand_over_done(shell))
}

fn hand_over_done(shell: DesktopShell) -> String {
    // TRANSLATORS: %1 is the shell name (Noctalia, DankMaterialShell).
    trf(
        "The wallpaper of %1 is turned off — the background now belongs to Rexon. \
         If the desktop is empty, apply a Rexon wallpaper.",
        &[&shell.label()],
    )
}

/// «Вернуть оболочке»: поставить обоям `shell` на место то, что стояло до
/// Rexon. Памятку снимает только удавшийся возврат.
pub fn give_back(shell: DesktopShell, home: &ShellHome) -> Result<String, String> {
    let all = read_handovers(home);
    let Some(record) = all.iter().find(|h| h.desktop_shell() == Some(shell)).cloned() else {
        return Err(tr("There is nothing to return: Rexon did not turn off the shell's wallpaper."));
    };
    let file = PathBuf::from(&record.file);
    let current = read_optional(&file)
        .map_err(|_| shell_edit_error(shell, &tr("the settings file could not be read")))?;
    if current.is_some() && !writable(&file) {
        return Err(shell_edit_error(shell, &tr("the settings file is read-only")));
    }
    let restored = match shell {
        DesktopShell::Noctalia => noctalia_restore(current.as_deref(), &record.previous),
        DesktopShell::DankMaterialShell => dms_restore(current.as_deref(), &record.previous),
    }
    .map_err(|e| shell_edit_error(shell, &e.message()))?;
    let write_failed = || shell_edit_error(shell, &tr("the settings file could not be written"));
    match restored {
        Restored::Write(text) => {
            if current.as_deref() != Some(text.as_str()) {
                if let Some(old) = &current {
                    write_text(&backup_path(&file), old).map_err(|_| write_failed())?;
                }
                write_text(&file, &text).map_err(|_| write_failed())?;
            }
        }
        Restored::Delete => {
            if current.is_some() {
                std::fs::remove_file(&file).map_err(|_| write_failed())?;
            }
        }
    }
    let rest: Vec<Handover> = all.into_iter().filter(|h| h.desktop_shell() != Some(shell)).collect();
    let _ = write_handovers(home, &rest);
    // TRANSLATORS: %1 is the shell name (Noctalia, DankMaterialShell).
    Ok(trf("The wallpaper of %1 is back on, with the setting it had before.", &[&shell.label()]))
}

// ============================================================================
// Живой сеанс
// ============================================================================

/// Отсеять протухшие служебные файлы: файл в каталоге сеанса доказывает,
/// что шелл ТУТ БЫЛ (после падения файлы остаются).
///
/// * `danklinux-<pid>.*` — живой, только если жив процесс `<pid>` (это PID
///   бэкенда, он же в имени);
/// * `noctalia-*.sock|.lock` — только если есть живой процесс `noctalia`
///   (флок на чужой файл блокировки не берём: в миг запуска шелла он отнял
///   бы у него единственность);
/// * остальное — как есть.
pub fn live_runtime_files(
    files: &[String],
    pid_alive: impl Fn(u32) -> bool,
    noctalia_running: bool,
) -> Vec<String> {
    files
        .iter()
        .filter(|f| {
            if let Some(rest) = f.strip_prefix("danklinux-") {
                let pid = rest.split('.').next().and_then(|p| p.parse::<u32>().ok());
                return pid.is_some_and(&pid_alive);
            }
            if f.starts_with("noctalia-") && (f.ends_with(".sock") || f.ends_with(".lock")) {
                return noctalia_running;
            }
            true
        })
        .cloned()
        .collect()
}

/// Наблюдения за сеансом, добытые на этой машине (владеющая форма
/// [`ShellFacts`]).
#[derive(Debug, Clone, Default)]
pub struct LiveFacts {
    pub wayland_display: String,
    pub runtime_files: Vec<String>,
    pub process_names: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl LiveFacts {
    /// Собрать без единого внешнего процесса: каталог сеанса, `/proc` своего
    /// пользователя и своё окружение. Имена на шине и юниты systemd здесь
    /// НЕ собираются — это вызовы `busctl`/`systemctl`, а сбор зовут из
    /// потока интерфейса; файла, процесса и метки окружения для живого
    /// шелла достаточно.
    pub fn collect() -> LiveFacts {
        let procs = own_processes();
        let names: Vec<String> = procs.iter().map(|(_, n)| n.clone()).collect();
        let noctalia_running = names.iter().any(|n| n == "noctalia");
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
        let files: Vec<String> = if runtime_dir.is_empty() {
            Vec::new()
        } else {
            std::fs::read_dir(&runtime_dir)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default()
        };
        let alive = |pid: u32| Path::new(&format!("/proc/{pid}")).exists();
        let env = SHELL_ENV_NAMES
            .iter()
            .filter_map(|k| {
                let v = std::env::var(k).ok()?;
                // Метка-путь к сокету, которого больше нет, — память о
                // прошлом шелле, а не он сам.
                if *k == "DMS_SOCKET" && !v.is_empty() && !Path::new(&v).exists() {
                    return None;
                }
                Some((k.to_string(), v))
            })
            .collect();
        LiveFacts {
            wayland_display: std::env::var("WAYLAND_DISPLAY").unwrap_or_default(),
            runtime_files: live_runtime_files(&files, alive, noctalia_running),
            process_names: names,
            env,
        }
    }

    /// Ответ ядра по этим наблюдениям.
    pub fn detect(&self) -> Option<ShellDetection> {
        let env: Vec<(&str, &str)> = self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        detect_shell(&ShellFacts {
            wayland_display: &self.wayland_display,
            runtime_files: &self.runtime_files,
            active_user_units: &[],
            dbus_names: &[],
            process_names: &self.process_names,
            env: &env,
        })
    }
}

/// Процессы своего пользователя: (pid, имя из `/proc/<pid>/comm`).
fn own_processes() -> Vec<(u32, String)> {
    use std::os::unix::fs::MetadataExt;
    let my_uid = std::fs::metadata("/proc/self").map(|m| m.uid()).ok();
    let Ok(rd) = std::fs::read_dir("/proc") else { return Vec::new() };
    rd.filter_map(|e| e.ok())
        .filter_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            if my_uid.is_some() && e.metadata().ok().map(|m| m.uid()) != my_uid {
                return None;
            }
            let comm = std::fs::read_to_string(e.path().join("comm")).ok()?;
            Some((pid, comm.trim_end().to_string()))
        })
        .collect()
}

// ============================================================================
// Что показать человеку
// ============================================================================

/// Полоса «чей фон» на вкладке обоев.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellBanner {
    pub visible: bool,
    pub text: String,
    /// Показать «Отдать обои Rexon».
    pub can_give: bool,
    /// Показать «Вернуть оболочке».
    pub can_return: bool,
    /// Какой шелл (`as_str`), пусто — никакой.
    pub shell: String,
}

/// Подпись кнопки «Отдать обои Rexon».
pub fn give_label() -> String {
    // TRANSLATORS: button; turns off the desktop shell's own wallpaper so Rexon's wallpaper shows.
    tr("Give Wallpaper to Rexon")
}

/// Подпись кнопки «Вернуть оболочке».
pub fn return_label() -> String {
    // TRANSLATORS: button; turns the desktop shell's own wallpaper back on, as it was before Rexon.
    tr("Return to Shell")
}

/// Решение полосы. Показывается ТОЛЬКО при живом шелле:
///   * фон держит шелл — «Отдать обои Rexon»;
///   * фон наш, потому что обои этого шелла выключил Rexon, — «Вернуть
///     оболочке»;
///   * фон наш по иной причине (человек выключил обои шелла сам, X11,
///     GNOME) — полосы нет: спорить не о чем.
pub fn shell_banner(
    detected: Option<DesktopShell>,
    owner: WallpaperOwner,
    handed_over: Option<DesktopShell>,
) -> ShellBanner {
    let Some(shell) = detected else {
        return ShellBanner::default();
    };
    if let WallpaperOwner::Shell(s) = owner {
        let text = trf(
            // TRANSLATORS: %1 is the shell name (Noctalia, DankMaterialShell). The quoted names are the two buttons next to this text.
            "The background is drawn by %1, and its own wallpaper covers Rexon's. \
             “Give Wallpaper to Rexon” turns the shell's wallpaper off; Rexon \
             remembers the previous setting, and “Return to Shell” brings it back.",
            &[&s.label()],
        );
        return ShellBanner {
            visible: true,
            text,
            can_give: true,
            can_return: false,
            shell: s.as_str().to_string(),
        };
    }
    if handed_over == Some(shell) {
        let text = trf(
            // TRANSLATORS: %1 is the shell name (Noctalia, DankMaterialShell). “Return to Shell” is the button next to this text.
            "The wallpaper of %1 is turned off at your request, and the background \
             is drawn by Rexon. “Return to Shell” turns it back on.",
            &[&shell.label()],
        );
        return ShellBanner {
            visible: true,
            text,
            can_give: false,
            can_return: true,
            shell: shell.as_str().to_string(),
        };
    }
    ShellBanner::default()
}

/// Состояние «чей фон» в этом сеансе.
#[derive(Debug, Clone)]
pub struct ShellWallpaperState {
    pub detection: Option<ShellDetection>,
    pub owner: WallpaperOwner,
    pub handed_over: Option<DesktopShell>,
    pub banner: ShellBanner,
}

/// Собрать состояние по готовым наблюдениям и корням настроек.
pub fn shell_wallpaper_state(
    de: &str,
    session_type: &str,
    detection: Option<ShellDetection>,
    home: &ShellHome,
) -> ShellWallpaperState {
    let shell = detection.as_ref().map(|d| d.shell);
    let draws = shell.and_then(|s| shell_draws_wallpaper(s, home));
    let owner = wallpaper_layer_owner(de, session_type, shell, draws);
    let handed_over = shell.filter(|s| read_handover(home, *s).is_some());
    let banner = shell_banner(shell, owner, handed_over);
    ShellWallpaperState { detection, owner, handed_over, banner }
}

/// То же по живой машине: окружение и сеанс опознаются так же, как их
/// опознаёт движок обоев.
pub fn live_shell_wallpaper_state() -> ShellWallpaperState {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let de = crate::wallpaper_engine::detect_de(&env("XDG_CURRENT_DESKTOP"), &env("DESKTOP_SESSION"));
    let session = crate::wallpaper_engine::detect_session_type(
        &env("XDG_SESSION_TYPE"),
        &env("WAYLAND_DISPLAY"),
        &env("DISPLAY"),
    );
    shell_wallpaper_state(&de, &session, LiveFacts::collect().detect(), &ShellHome::live())
}
