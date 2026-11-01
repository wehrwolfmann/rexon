//! Журнал сеанса — точный источник «какой модуль KWin или Plasma не загрузился».
//!
//! Сверка по версиям в файлах ([`crate::aeroshell`]) угадывает поломку по
//! косвенным признакам: опознаватель внутри модуля эффекта, имя библиотеки
//! форка, `ldd`. Сами KWin и plasmashell знают точно — и говорят об этом в
//! журнал пользовательского сеанса (`journalctl --user`, без root): в Plasma 6
//! оба запущены пользовательскими юнитами systemd, и их вывод уходит туда
//! вместе с полем `_COMM` (проверено 05.10.2026 на этой машине, systemd 262).
//!
//! Что ищем (сообщения журнала НЕ переводятся у `qCWarning`, но переводятся
//! у строк, собранных через i18n, — поэтому опираемся на непереводимые
//! части: служебные фразы предупреждений, идентификаторы в кавычках, пути):
//!
//! * KWin, эффект собран под другую версию KWin. Начиная с KWin 6.8 это
//!   предупреждение: `Effect "<id>" was built for a different version of
//!   KWin and needs to be rebuilt, expected "<IID><версия>" got "<IID><версия>"`.
//!   До 6.8 — отладочная строка `"<id>"  has not matching plugin version, …`,
//!   её в журнале нет без включённой отладки: тогда журнал просто молчит, и
//!   остаётся сверка по версиям.
//! * KWin, модуль эффекта не загрузился: `Did not get KPluginFactory for "<id>":…`.
//! * KWin, плагин (не эффект): `"<id>" has mismatching plugin version`,
//!   `Failed to get plugin factory for "<id>" …`.
//! * Любой процесс, библиотека модуля не загрузилась (KCoreAddons):
//!   `Could not load plugin from <путь>.so: <причина>` — по пути видно, что
//!   это: эффект KWin, апплет Plasma, оформление окон.
//! * plasmashell, апплет не поднялся: `error when loading applet "<id>"
//!   QList("…", "…")`. Внутри — переведённые строки, поэтому вид поломки
//!   узнаётся по путям: `.so` в тексте — не загрузилась библиотека (в том
//!   числе QML-модуля виджета), `file://…qml:строка:` — сломан QML виджета,
//!   ни того ни другого — «пакет не найден». Последнее в Plasma до правки
//!   libplasma говорится и тогда, когда пакет на месте, а не грузится его
//!   `.so`; это различается по диску в [`resolve`].
//!
//! Здесь — чистый разбор и решение; чтение журнала, `pacman -Qo` и диск —
//! у исполнителя (`system_de_exec`).

use crate::i18n::{tr, trf};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// Процессы, чьи сообщения читаем (поле `_COMM`).
pub const JOURNAL_PROCESSES: &[&str] = &["kwin_wayland", "kwin_x11", "plasmashell"];

/// Отбор на стороне journalctl (`--grep`, PCRE2): только нужные строки.
pub const JOURNAL_PATTERN: &str = "was built for a different version of KWin|has not matching plugin version|\
Did not get KPluginFactory for|has mismatching plugin version|Failed to get plugin factory for|\
Could not load plugin from|error when loading applet";

/// Таймаут чтения журнала, секунды.
pub const JOURNAL_TIMEOUT_SECS: u64 = 10;

/// Сколько последних строк брать, если journalctl не умеет `--grep`.
pub const JOURNAL_FALLBACK_LINES: usize = 50_000;

/// Сколько байт ответа разбирать (хвост — свежее).
pub const JOURNAL_MAX_BYTES: usize = 32 << 20;

/// Сколько находок показывать человеку.
pub const SHOWN_FINDINGS: usize = 6;

/// Команда чтения журнала текущей загрузки. `grep` — с отбором на стороне
/// journalctl; без него — последние [`JOURNAL_FALLBACK_LINES`] строк и без
/// `--output-fields` (его нет у старых systemd).
pub fn journal_argv(grep: bool) -> Vec<String> {
    let mut argv: Vec<String> = ["journalctl", "--user", "-b", "--no-pager", "-q", "-o", "json"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if grep {
        argv.push("--output-fields=MESSAGE,_COMM".to_string());
        argv.push("--case-sensitive=true".to_string());
        argv.push(format!("--grep={JOURNAL_PATTERN}"));
    } else {
        argv.push("-n".to_string());
        argv.push(JOURNAL_FALLBACK_LINES.to_string());
    }
    argv.extend(JOURNAL_PROCESSES.iter().map(|p| format!("_COMM={p}")));
    argv
}

/// Что делать с ответом journalctl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalAnswer {
    /// Разбирать stdout.
    Text,
    /// Ничего не нашлось.
    Empty,
    /// `--grep` не понят (journalctl без PCRE2 или старый) — повторить без него.
    RetryWithoutGrep,
    /// Журнала нет или читать нельзя — молча пропустить.
    Unavailable,
}

/// Разбор кода возврата journalctl. С `--grep` код 1 и пустой вывод —
/// «совпадений нет» (проверено: systemd 262).
pub fn journal_answer(grep: bool, rc: Option<i32>, stdout: &str, stderr: &str) -> JournalAnswer {
    match rc {
        Some(0) if stdout.trim().is_empty() => JournalAnswer::Empty,
        Some(0) => JournalAnswer::Text,
        Some(1) if grep && stdout.trim().is_empty() && stderr.trim().is_empty() => JournalAnswer::Empty,
        _ if grep => JournalAnswer::RetryWithoutGrep,
        _ => JournalAnswer::Unavailable,
    }
}

/// Одна строка журнала: текст и время записи (микросекунды эпохи).
#[derive(Debug, Clone, PartialEq)]
pub struct JournalLine {
    pub message: String,
    pub time_us: u64,
}

/// Разбор `journalctl -o json`: по объекту на строку. `MESSAGE` бывает
/// массивом байт (не UTF-8) — тогда читается с заменой. Разбирается только
/// хвост длиной [`JOURNAL_MAX_BYTES`].
pub fn parse_journal_json(text: &str) -> Vec<JournalLine> {
    let text = if text.len() > JOURNAL_MAX_BYTES {
        let mut from = text.len() - JOURNAL_MAX_BYTES;
        while !text.is_char_boundary(from) {
            from += 1;
        }
        let tail = &text[from..];
        tail.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
    } else {
        text
    };
    text.lines()
        .filter_map(|line| {
            let v: Value = serde_json::from_str(line.trim()).ok()?;
            let message = match v.get("MESSAGE")? {
                Value::String(s) => s.clone(),
                Value::Array(bytes) => {
                    let raw: Vec<u8> = bytes.iter().filter_map(|b| b.as_u64().map(|b| b as u8)).collect();
                    String::from_utf8_lossy(&raw).into_owned()
                }
                _ => return None,
            };
            let time_us = v
                .get("__REALTIME_TIMESTAMP")
                .and_then(|t| t.as_str().and_then(|s| s.parse().ok()).or_else(|| t.as_u64()))
                .unwrap_or(0);
            Some(JournalLine { message, time_us })
        })
        .collect()
}

/// Что за модуль.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    /// Эффект KWin.
    Effect,
    /// Апплет (виджет) Plasma.
    Applet,
    /// Оформление окон (KDecoration).
    Decoration,
    /// Плагин KWin (не эффект).
    KwinPlugin,
}

/// Почему не загрузился.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cause {
    /// Собран под другую версию KWin: `built_for` — версия сборки,
    /// `expected` — версия KWin сейчас (None — сообщение версий не назвало).
    KwinVersion { built_for: Option<String>, expected: Option<String> },
    /// Библиотека модуля не загрузилась (символ, soname, Qt): пересборка.
    LoadFailed { detail: String },
    /// Сломан QML виджета (тип недоступен, свойства нет и т. п.).
    QmlBroken { detail: String },
    /// Пакета виджета нет вовсе.
    NotFound,
}

impl Cause {
    /// Насколько точно сказано: при слиянии остаётся самое точное.
    fn rank(&self) -> u8 {
        match self {
            Cause::KwinVersion { .. } => 4,
            Cause::LoadFailed { .. } => 3,
            Cause::QmlBroken { .. } => 2,
            Cause::NotFound => 1,
        }
    }

    /// Короткая техническая строка для журнала Rexon и подробностей.
    pub fn technical(&self) -> String {
        match self {
            Cause::KwinVersion { built_for, expected } => format!(
                "built for KWin {}, KWin {} installed",
                built_for.as_deref().unwrap_or("?"),
                expected.as_deref().unwrap_or("?")
            ),
            Cause::LoadFailed { detail } if detail.is_empty() => "library failed to load".to_string(),
            Cause::LoadFailed { detail } => detail.clone(),
            Cause::QmlBroken { detail } => detail.clone(),
            Cause::NotFound => "package not found".to_string(),
        }
    }
}

/// Одна поломка из журнала.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Failure {
    pub kind: PluginKind,
    /// Идентификатор модуля (`pluginId`).
    pub id: String,
    pub cause: Cause,
    /// Файлы, названные в сообщении (`.so`, `.qml`).
    pub paths: Vec<String>,
    /// Время последнего сообщения, микросекунды эпохи.
    pub time_us: u64,
}

const EFFECT_IID: &str = "org.kde.kwin.EffectPluginFactory";

/// Строки в двойных кавычках подряд (с разбором `\"`, `\\`, `\n`).
fn quoted_strings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut s = String::new();
        let mut closed = false;
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some(o) => s.push(o),
                    None => break,
                },
                '"' => {
                    closed = true;
                    break;
                }
                _ => s.push(c),
            }
        }
        if closed {
            out.push(s);
        }
    }
    out
}

/// Версии после опознавателя фабрики эффектов, по порядку вхождения.
fn iid_versions(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find(EFFECT_IID) {
        rest = &rest[pos + EFFECT_IID.len()..];
        let v: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let v = v.trim_end_matches('.').to_string();
        if v.contains('.') {
            out.push(v);
        }
    }
    out
}

/// Пути к `.so` и `.qml`, названные в тексте.
pub fn extract_paths(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |p: String| {
        if !p.is_empty() && !out.contains(&p) {
            out.push(p);
        }
    };
    // QML: file:///…/main.qml:20:1
    let mut rest = text;
    while let Some(pos) = rest.find("file://") {
        rest = &rest[pos + "file://".len()..];
        let token: String = rest.chars().take_while(|c| !c.is_whitespace() && !"\"«»'()".contains(*c)).collect();
        let path = token.split(".qml").next().unwrap_or("");
        if token.contains(".qml") && path.starts_with('/') {
            push(format!("{path}.qml"));
        }
    }
    // Библиотеки: путь начинается с «/» после пробела, кавычки, скобки.
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let starts = chars[i] == '/'
            && (i == 0 || matches!(chars[i - 1], ' ' | '«' | '"' | '(' | '\'' | '\t' | '\n'));
        if !starts {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && !chars[j].is_whitespace() && !"\"«»'():".contains(chars[j]) {
            j += 1;
        }
        let token: String = chars[i..j].iter().collect();
        let name = token.rsplit('/').next().unwrap_or("");
        if name.ends_with(".so") || name.contains(".so.") {
            push(token);
        }
        i = j.max(i + 1);
    }
    out
}

/// Вид модуля по пути его библиотеки.
fn kind_by_path(path: &str) -> Option<PluginKind> {
    if path.contains("/kwin/effects/plugins/") {
        Some(PluginKind::Effect)
    } else if path.contains("/plasma/applets/") {
        Some(PluginKind::Applet)
    } else if path.contains("kdecoration") {
        Some(PluginKind::Decoration)
    } else if path.contains("/kwin/plugins/") {
        Some(PluginKind::KwinPlugin)
    } else {
        None
    }
}

fn file_stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.split(".so").next().unwrap_or(name).to_string()
}

/// Обрезать подробность до читаемой длины.
fn short(text: &str) -> String {
    let t = text.trim().replace('\n', " ");
    if t.chars().count() > 300 {
        format!("{}…", t.chars().take(300).collect::<String>())
    } else {
        t
    }
}

/// Хвост после пути библиотеки: «libprocesscore.so.10: cannot open …».
fn after_path(text: &str, path: &str) -> String {
    match text.rfind(path) {
        Some(pos) => short(text[pos + path.len()..].trim_start_matches([':', '»', ' '])),
        None => short(text),
    }
}

/// Разобрать одно сообщение. None — не про загрузку модулей.
pub fn parse_message(message: &str, time_us: u64) -> Option<Failure> {
    let quoted = quoted_strings(message);
    let make = |kind, id: &str, cause, paths| Failure {
        kind,
        id: id.to_string(),
        cause,
        paths,
        time_us,
    };

    // KWin ≥ 6.8 (предупреждение) и KWin < 6.8 (отладка).
    if message.contains("was built for a different version of KWin")
        || message.contains("has not matching plugin version")
    {
        let id = if let Some(pos) = message.find("Effect \"") {
            quoted_strings(&message[pos..]).into_iter().next()?
        } else {
            quoted.first()?.clone()
        };
        let v = iid_versions(message);
        return Some(make(
            PluginKind::Effect,
            &id,
            Cause::KwinVersion {
                expected: v.first().cloned(),
                built_for: v.get(1).cloned(),
            },
            Vec::new(),
        ));
    }
    if message.contains("Did not get KPluginFactory for") {
        let id = quoted.first()?.clone();
        let detail = quoted.get(1).map(|d| short(d)).unwrap_or_default();
        return Some(make(PluginKind::Effect, &id, Cause::LoadFailed { detail }, extract_paths(message)));
    }
    if message.contains("has mismatching plugin version") {
        let id = quoted.first()?.clone();
        return Some(make(
            PluginKind::KwinPlugin,
            &id,
            Cause::KwinVersion { built_for: None, expected: None },
            Vec::new(),
        ));
    }
    if message.contains("Failed to get plugin factory for") {
        let id = quoted.first()?.clone();
        let detail = quoted.get(1).map(|d| short(d)).unwrap_or_default();
        return Some(make(PluginKind::KwinPlugin, &id, Cause::LoadFailed { detail }, extract_paths(message)));
    }
    if let Some(pos) = message.find("Could not load plugin from ") {
        let rest = &message[pos + "Could not load plugin from ".len()..];
        let path = extract_paths(&format!(" {rest}")).into_iter().find(|p| !p.ends_with(".qml"))?;
        let kind = kind_by_path(&path)?;
        let detail = after_path(rest.trim_end_matches('"'), &path);
        return Some(make(kind, &file_stem(&path), Cause::LoadFailed { detail }, vec![path]));
    }
    if let Some(pos) = message.find("error when loading applet ") {
        let rest = &message[pos..];
        let mut strings = quoted_strings(rest);
        if strings.is_empty() {
            return None;
        }
        let id = strings.remove(0);
        let mut paths = Vec::new();
        for s in &strings {
            for p in extract_paths(s) {
                if !paths.contains(&p) {
                    paths.push(p);
                }
            }
        }
        let library = strings.iter().find_map(|s| {
            extract_paths(s)
                .into_iter()
                .find(|p| !p.ends_with(".qml"))
                .map(|p| after_path(s, &p))
        });
        let qml = strings
            .iter()
            .find(|s| s.contains("file://") || s.starts_with("qrc:"))
            .map(|s| short(s));
        let cause = match (library, qml) {
            (Some(detail), _) => Cause::LoadFailed { detail },
            (None, Some(detail)) => Cause::QmlBroken { detail },
            // Правка libplasma (MR !1597) говорит это прямо; путь в такой
            // строке есть всегда, но фраза — запасной признак.
            _ if strings.iter().any(|s| s.contains("built for a different version of Plasma")) => {
                Cause::LoadFailed { detail: String::new() }
            }
            _ => Cause::NotFound,
        };
        return Some(make(PluginKind::Applet, &id, cause, paths));
    }
    None
}

/// Все поломки из ответа journalctl: по одной на модуль, самая точная
/// причина, время — последнего сообщения.
pub fn failures_from_journal(text: &str) -> Vec<Failure> {
    let mut merged: BTreeMap<(PluginKind, String), Failure> = BTreeMap::new();
    for line in parse_journal_json(text) {
        let Some(f) = parse_message(&line.message, line.time_us) else { continue };
        match merged.get_mut(&(f.kind, f.id.clone())) {
            None => {
                merged.insert((f.kind, f.id.clone()), f);
            }
            Some(old) => {
                if f.cause.rank() > old.cause.rank()
                    || (f.cause.rank() == old.cause.rank() && f.time_us >= old.time_us)
                {
                    old.cause = f.cause;
                }
                old.time_us = old.time_us.max(f.time_us);
                for p in f.paths {
                    if !old.paths.contains(&p) {
                        old.paths.push(p);
                    }
                }
            }
        }
    }
    merged.into_values().collect()
}

// ── Метаданные внутри модуля Qt (CBOR) ────────────────────────────────────
//
// Qt 6 кладёт метаданные плагина (`metadata.json`, вшитый при сборке) в
// модуль в виде CBOR. Имя эффекта для человека («SMOD Glow») есть только
// там. Разбираем минимально: от ключа `KPlugin` — одно значение.

fn cbor_arg(data: &[u8], pos: &mut usize, info: u8) -> Option<Option<u64>> {
    let take = |pos: &mut usize, n: usize| -> Option<u64> {
        let bytes = data.get(*pos..*pos + n)?;
        *pos += n;
        Some(bytes.iter().fold(0u64, |acc, b| (acc << 8) | *b as u64))
    };
    Some(match info {
        0..=23 => Some(info as u64),
        24 => Some(take(pos, 1)?),
        25 => Some(take(pos, 2)?),
        26 => Some(take(pos, 4)?),
        27 => Some(take(pos, 8)?),
        31 => None,
        _ => return None,
    })
}

fn cbor_item(data: &[u8], pos: &mut usize, depth: u32) -> Option<Value> {
    if depth > 32 {
        return None;
    }
    let ib = *data.get(*pos)?;
    *pos += 1;
    let (major, info) = (ib >> 5, ib & 0x1f);
    if major == 7 {
        return Some(match info {
            20 => Value::Bool(false),
            21 => Value::Bool(true),
            22 | 23 => Value::Null,
            25 => {
                *pos += 2;
                Value::Null
            }
            26 => {
                let b = data.get(*pos..*pos + 4)?;
                *pos += 4;
                serde_json::json!(f32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64)
            }
            27 => {
                let b = data.get(*pos..*pos + 8)?;
                *pos += 8;
                let mut a = [0u8; 8];
                a.copy_from_slice(b);
                serde_json::json!(f64::from_be_bytes(a))
            }
            _ => return None,
        });
    }
    let arg = cbor_arg(data, pos, info)?;
    let limit = data.len() as u64;
    match major {
        0 => Some(serde_json::json!(arg?)),
        1 => Some(serde_json::json!(-1i64 - arg? as i64)),
        2 | 3 => {
            let mut raw = Vec::new();
            match arg {
                Some(n) if n <= limit => {
                    raw.extend_from_slice(data.get(*pos..*pos + n as usize)?);
                    *pos += n as usize;
                }
                Some(_) => return None,
                None => loop {
                    if *data.get(*pos)? == 0xff {
                        *pos += 1;
                        break;
                    }
                    let chunk = cbor_item(data, pos, depth + 1)?;
                    raw.extend_from_slice(chunk.as_str().unwrap_or("").as_bytes());
                },
            }
            Some(if major == 3 {
                Value::String(String::from_utf8_lossy(&raw).into_owned())
            } else {
                Value::Null
            })
        }
        4 => {
            let mut items = Vec::new();
            match arg {
                Some(n) if n <= limit => {
                    for _ in 0..n {
                        items.push(cbor_item(data, pos, depth + 1)?);
                    }
                }
                Some(_) => return None,
                None => loop {
                    if *data.get(*pos)? == 0xff {
                        *pos += 1;
                        break;
                    }
                    items.push(cbor_item(data, pos, depth + 1)?);
                },
            }
            Some(Value::Array(items))
        }
        5 => {
            let mut map = serde_json::Map::new();
            let mut entry = |pos: &mut usize| -> Option<()> {
                let key = cbor_item(data, pos, depth + 1)?;
                let value = cbor_item(data, pos, depth + 1)?;
                let key = match key {
                    Value::String(s) => s,
                    other => other.to_string(),
                };
                map.insert(key, value);
                Some(())
            };
            match arg {
                Some(n) if n <= limit => {
                    for _ in 0..n {
                        entry(pos)?;
                    }
                }
                Some(_) => return None,
                None => loop {
                    if *data.get(*pos)? == 0xff {
                        *pos += 1;
                        break;
                    }
                    entry(pos)?;
                },
            }
            Some(Value::Object(map))
        }
        6 => cbor_item(data, pos, depth + 1),
        _ => None,
    }
}

/// Раздел `KPlugin` метаданных, вшитых в модуль Qt. None — не нашли.
pub fn embedded_kplugin(data: &[u8]) -> Option<Value> {
    embedded_value(data, "KPlugin", |v| v.is_object())
}

/// Значение ключа верхнего уровня метаданных, вшитых в модуль Qt
/// (`X-KDE-ConfigModule`, `org.kde.kwin.effect`). Ищется строка-ключ CBOR, за
/// ней — значение; `accept` отсекает случайные совпадения в строковой
/// таблице модуля. None — не нашли.
pub fn embedded_value(data: &[u8], key: &str, accept: impl Fn(&Value) -> bool) -> Option<Value> {
    let mut needle = match key.len() {
        n @ 0..=23 => vec![0x60 | n as u8],
        n @ 24..=255 => vec![0x78, n as u8],
        _ => return None,
    };
    needle.extend_from_slice(key.as_bytes());
    let mut from = 0;
    while let Some(off) = data[from..].windows(needle.len()).position(|w| w == needle.as_slice()) {
        let mut pos = from + off + needle.len();
        if let Some(v) = cbor_item(data, &mut pos, 0).filter(|v| accept(v)) {
            return Some(v);
        }
        from += off + 1;
    }
    None
}

/// Имя по языку из раздела `KPlugin` (`keys` — от точного к общему).
fn kplugin_name(kplugin: &Value, keys: &[String]) -> Option<String> {
    keys.iter().find_map(|k| {
        kplugin
            .get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

/// Идентификатор из раздела `KPlugin`.
fn kplugin_id(kplugin: &Value) -> Option<String> {
    kplugin.get("Id").and_then(Value::as_str).map(str::to_string)
}

// ── Сопоставление с диском и пакетами ─────────────────────────────────────

/// Откуда модуль.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    /// Из пакета; `foreign` — не из репозиториев (AUR, собран на машине).
    Package { name: String, foreign: bool },
    /// Из домашнего каталога (магазин KDE, ручная установка).
    UserDir,
    /// Владельца не нашли.
    Unknown,
}

/// Поломка, сопоставленная с файлом и пакетом.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub failure: Failure,
    /// Имя для человека (из метаданных; иначе идентификатор).
    pub name: String,
    pub origin: Origin,
    /// Главный файл модуля на диске.
    pub file: Option<String>,
}

impl Finding {
    /// Пакет-владелец.
    pub fn package(&self) -> Option<&str> {
        match &self.origin {
            Origin::Package { name, .. } => Some(name),
            _ => None,
        }
    }
}

/// Всё, что нужно для сопоставления, — снаружи.
pub struct ResolveEnv<'a> {
    pub home: &'a str,
    /// `~/.local/share` (XDG_DATA_HOME).
    pub data_home: &'a str,
    /// `~/.config` (XDG_CONFIG_HOME).
    pub config_home: &'a str,
    /// Системные каталоги данных (XDG_DATA_DIRS).
    pub data_dirs: &'a [String],
    /// Корни плагинов Qt 6 (`/usr/lib/qt6/plugins` и QT_PLUGIN_PATH).
    pub plugin_dirs: &'a [String],
    /// Язык имён (`ru`, `de_DE`).
    pub locale: &'a str,
    /// Время изменения файла, микросекунды эпохи; None — файла нет.
    pub mtime_us: &'a dyn Fn(&str) -> Option<u64>,
    /// Содержимое файла.
    pub read: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    /// Файлы каталога (полные пути).
    pub list: &'a dyn Fn(&str) -> Vec<String>,
    /// Владельцы файлов: путь → пакет (`pacman -Qo`).
    pub owners: &'a dyn Fn(&[String]) -> HashMap<String, String>,
    /// Пакет не из репозиториев (`pacman -Qm`).
    pub foreign: &'a dyn Fn(&str) -> bool,
}

/// Корни плагинов Qt 6: из QT_PLUGIN_PATH и обычные места дистрибутивов.
pub fn qt_plugin_dirs(qt_plugin_path: &str) -> Vec<String> {
    let mut dirs: Vec<String> = qt_plugin_path
        .split(':')
        .filter(|d| d.starts_with('/'))
        .map(|d| d.trim_end_matches('/').to_string())
        .collect();
    for d in [
        "/usr/lib/qt6/plugins",
        "/usr/lib64/qt6/plugins",
        "/usr/lib/x86_64-linux-gnu/qt6/plugins",
        "/usr/lib/aarch64-linux-gnu/qt6/plugins",
    ] {
        if !dirs.iter().any(|x| x == d) {
            dirs.push(d.to_string());
        }
    }
    dirs
}

/// Системные каталоги данных из XDG_DATA_DIRS (по умолчанию — по спецификации).
pub fn data_dirs(xdg_data_dirs: &str) -> Vec<String> {
    let raw = if xdg_data_dirs.trim().is_empty() { "/usr/local/share:/usr/share" } else { xdg_data_dirs };
    raw.split(':')
        .filter(|d| d.starts_with('/'))
        .map(|d| d.trim_end_matches('/').to_string())
        .collect()
}

/// Где может лежать модуль: (файлы библиотеки, файлы metadata.json).
fn candidates(f: &Failure, env: &ResolveEnv) -> (Vec<String>, Vec<String>) {
    let mut libs: Vec<String> = f.paths.iter().filter(|p| !p.ends_with(".qml")).cloned().collect();
    let mut meta = Vec::new();
    let (lib_sub, data_sub) = match f.kind {
        PluginKind::Effect => ("kwin/effects/plugins", Some("kwin/effects")),
        PluginKind::Applet => ("plasma/applets", Some("plasma/plasmoids")),
        PluginKind::Decoration | PluginKind::KwinPlugin => ("", None),
    };
    if !lib_sub.is_empty() {
        for dir in env.plugin_dirs {
            libs.push(format!("{dir}/{lib_sub}/{}.so", f.id));
        }
    }
    if let Some(sub) = data_sub {
        meta.push(format!("{}/{sub}/{}/metadata.json", env.data_home, f.id));
        for dir in env.data_dirs {
            meta.push(format!("{dir}/{sub}/{}/metadata.json", f.id));
        }
    }
    // Путь QML виджета внутри пакета → его metadata.json.
    for p in f.paths.iter().filter(|p| p.ends_with(".qml")) {
        if let Some(pos) = p.find("/contents/") {
            meta.push(format!("{}/metadata.json", &p[..pos]));
        }
    }
    (libs, meta)
}

/// Модуль эффекта, чей вшитый `KPlugin.Id` равен `id` (имя файла с id не
/// совпадает: `libkwin_effect_x.so`).
fn effect_by_embedded_id(id: &str, env: &ResolveEnv) -> Option<String> {
    for dir in env.plugin_dirs {
        for path in (env.list)(&format!("{dir}/kwin/effects/plugins")) {
            if !path.ends_with(".so") {
                continue;
            }
            let found = (env.read)(&path)
                .and_then(|d| embedded_kplugin(&d))
                .and_then(|k| kplugin_id(&k))
                .is_some_and(|x| x == id);
            if found {
                return Some(path);
            }
        }
    }
    None
}

/// Виджет всё ещё стоит на панели или столе: `plugin=<id>` в раскладке.
/// Раскладки нет или не читается — считаем, что стоит.
fn applet_placed(id: &str, env: &ResolveEnv) -> bool {
    let path = format!("{}/plasma-org.kde.plasma.desktop-appletsrc", env.config_home);
    match (env.read)(&path) {
        Some(raw) => String::from_utf8_lossy(&raw)
            .lines()
            .any(|l| l.trim().strip_prefix("plugin=") == Some(id)),
        None => true,
    }
}

/// Эффект выключен человеком сам (`<id>Enabled=false` в kwinrc).
fn effect_disabled(id: &str, env: &ResolveEnv) -> bool {
    let path = format!("{}/kwinrc", env.config_home);
    let Some(raw) = (env.read)(&path) else { return false };
    let text = String::from_utf8_lossy(&raw);
    let mut in_plugins = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_plugins = line == "[Plugins]";
        } else if in_plugins && line.replace(' ', "") == format!("{id}Enabled=false") {
            return true;
        }
    }
    false
}

/// Сопоставить поломки с диском и пакетами.
///
/// Отбрасывается то, что журнал помнит, но уже неправда: файл модуля
/// заменён ПОСЛЕ сообщения (пересобран — осталось перезайти), виджет снят
/// с панели, эффект выключен человеком. «Пакет не найден» при пакете на
/// диске — это не загрузившаяся библиотека (так говорит libplasma до
/// правки MR !1597).
pub fn resolve(failures: &[Failure], env: &ResolveEnv) -> Vec<Finding> {
    struct Pending {
        failure: Failure,
        libs: Vec<String>,
        metas: Vec<String>,
    }
    let mut pending = Vec::new();
    for f in failures {
        match f.kind {
            PluginKind::Applet if !applet_placed(&f.id, env) => continue,
            PluginKind::Effect if effect_disabled(&f.id, env) => continue,
            _ => {}
        }
        let (libs, metas) = candidates(f, env);
        let mut libs: Vec<String> = dedup(libs).into_iter().filter(|p| (env.mtime_us)(p).is_some()).collect();
        let metas: Vec<String> = dedup(metas).into_iter().filter(|p| (env.mtime_us)(p).is_some()).collect();
        if libs.is_empty() && f.kind == PluginKind::Effect {
            libs.extend(effect_by_embedded_id(&f.id, env));
        }
        // Заменён после сообщения: журнал устарел.
        let replaced = libs
            .iter()
            .chain(metas.iter())
            .any(|p| (env.mtime_us)(p).is_some_and(|t| f.time_us > 0 && t > f.time_us));
        if replaced {
            continue;
        }
        let mut failure = f.clone();
        if failure.cause == Cause::NotFound && !(libs.is_empty() && metas.is_empty()) {
            failure.cause = Cause::LoadFailed { detail: String::new() };
        }
        pending.push(Pending { failure, libs, metas });
    }

    let all: Vec<String> = pending.iter().flat_map(|p| p.libs.iter().chain(p.metas.iter()).cloned()).collect();
    let owners = if all.is_empty() { HashMap::new() } else { (env.owners)(&all) };
    let keys = crate::installed_scan::locale_name_keys(env.locale);
    let home_prefix = format!("{}/", env.home.trim_end_matches('/'));

    pending
        .into_iter()
        .map(|p| {
            let files: Vec<&String> = p.libs.iter().chain(p.metas.iter()).collect();
            let owner = files.iter().find_map(|f| owners.get(f.as_str()).cloned());
            let origin = match owner {
                Some(name) => {
                    let foreign = (env.foreign)(&name);
                    Origin::Package { name, foreign }
                }
                None if files.iter().any(|f| f.starts_with(&home_prefix)) => Origin::UserDir,
                None => Origin::Unknown,
            };
            let from_json = p.metas.iter().find_map(|m| {
                let raw = (env.read)(m)?;
                let v: Value = serde_json::from_slice(&raw).ok()?;
                kplugin_name(v.get("KPlugin")?, &keys)
            });
            let name = from_json
                .or_else(|| {
                    p.libs.iter().find_map(|l| {
                        let data = (env.read)(l)?;
                        kplugin_name(&embedded_kplugin(&data)?, &keys)
                    })
                })
                .unwrap_or_else(|| p.failure.id.clone());
            let file = p.libs.first().or(p.metas.first()).cloned();
            Finding {
                failure: p.failure,
                name,
                origin,
                file,
            }
        })
        .collect()
}

fn dedup(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for x in v {
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

/// AUR-пакеты, пересборка которых чинит найденное.
pub fn rebuild_packages(findings: &[Finding]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in findings {
        if f.failure.cause == Cause::NotFound {
            continue;
        }
        if let Origin::Package { name, foreign: true } = &f.origin {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
    }
    out
}

// ── Слова для человека ────────────────────────────────────────────────────

/// Что сломано — одной фразой.
pub fn finding_problem(f: &Finding) -> String {
    let name = &f.name;
    match (&f.failure.kind, &f.failure.cause) {
        (PluginKind::Effect, Cause::KwinVersion { built_for: Some(b), expected: Some(e) }) => trf(
            // TRANSLATORS: %1 is the effect name, %2 the KWin version it was
            // built for, %3 the KWin version installed now.
            "The “%1” effect is off: it was built for KWin %2, but KWin %3 is installed.",
            &[name, b, e],
        ),
        (PluginKind::Effect | PluginKind::KwinPlugin, Cause::KwinVersion { .. }) => trf(
            "The “%1” effect is off: it was built for a different version of KWin.",
            &[name],
        ),
        (PluginKind::Effect | PluginKind::KwinPlugin, _) => trf(
            "The “%1” effect is off: it doesn't load with the installed KWin.",
            &[name],
        ),
        (PluginKind::Applet, Cause::NotFound) => trf(
            "The “%1” widget is on the panel or desktop, but it isn't installed anymore.",
            &[name],
        ),
        (PluginKind::Applet, Cause::QmlBroken { .. }) => trf(
            "The “%1” widget doesn't start: its parts don't match the installed Plasma.",
            &[name],
        ),
        (PluginKind::Applet, _) => trf(
            "The “%1” widget doesn't start: it was built for a different version of Plasma.",
            &[name],
        ),
        (PluginKind::Decoration, _) => trf(
            "The “%1” window decoration doesn't load: it was built for a different version of the system.",
            &[name],
        ),
    }
}

/// Что с этим делать.
pub fn finding_action(f: &Finding) -> String {
    if f.failure.cause == Cause::NotFound {
        return tr("Remove it from the panel or install it again.");
    }
    match &f.origin {
        Origin::Package { name, foreign: true } => trf(
            // TRANSLATORS: %1 is the name of an AUR package.
            "Rebuild the AUR package %1.",
            &[name],
        ),
        Origin::Package { name, foreign: false } => trf(
            // TRANSLATORS: %1 is the name of a package from the distribution.
            "Update the whole system: the package %1 doesn't match the rest of Plasma.",
            &[name],
        ),
        Origin::UserDir => tr("Update it from the store or remove it."),
        Origin::Unknown => tr("Reinstall it or remove it."),
    }
}

/// Строка для окна и уведомления: что сломано и что делать.
pub fn finding_line(f: &Finding) -> String {
    format!("{} {}", finding_problem(f), finding_action(f))
}

/// Строки для показа (не больше [`SHOWN_FINDINGS`], остаток — числом).
pub fn finding_lines(findings: &[Finding]) -> Vec<String> {
    let mut lines: Vec<String> = findings.iter().take(SHOWN_FINDINGS).map(finding_line).collect();
    if findings.len() > SHOWN_FINDINGS {
        lines.push(crate::i18n::ntr(
            "…and %n more.",
            "…and %n more.",
            (findings.len() - SHOWN_FINDINGS) as u64,
        ));
    }
    lines
}

/// Техническая строка для журнала Rexon.
pub fn finding_technical(f: &Finding) -> String {
    let origin = match &f.origin {
        Origin::Package { name, foreign } => format!("{name}{}", if *foreign { " (AUR)" } else { "" }),
        Origin::UserDir => "~".to_string(),
        Origin::Unknown => "?".to_string(),
    };
    format!(
        "journal: {:?} {} [{origin}]: {}",
        f.failure.kind,
        f.failure.id,
        f.failure.cause.technical()
    )
}

/// Заголовок окна и уведомления о сломанных дополнениях.
pub fn plugins_title() -> String {
    tr("Some Desktop Add-ons Stopped Working")
}

/// Вступление перед списком.
pub fn plugins_intro() -> String {
    tr("KWin and Plasma reported in this session that these add-ons didn't load:")
}

/// Приписка, когда Rexon может пересобрать сам.
pub fn plugins_rebuild_note() -> String {
    tr("Rexon can rebuild the AUR packages now. This takes a few minutes; at the end you'll need to \
        log out and log back in.")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Строки — записанные запуском 05.10.2026 (см. отчёт коммита): KWin из
    // ветки правки и штатный 6.7.5 с отладкой, plasmawindowed со штатной и
    // исправленной libplasma; строки seventasks — из заявок ATP.
    const KWIN_NEW: &str = "Effect \"brokenversiontest\" was built for a different version of KWin and needs to be \
rebuilt, expected \"org.kde.kwin.EffectPluginFactory6.7.5\" got \"org.kde.kwin.EffectPluginFactory6.7.4\"";
    const KWIN_OLD: &str = "\"brokenversiontest\"  has not matching plugin version, expected  \
org.kde.kwin.EffectPluginFactory6.7.5 got  QJsonValue(string, \"org.kde.kwin.EffectPluginFactory6.7.4\")";
    const COREADDONS: &str = "\"Could not load plugin from /r/soname/plasma/applets/org.kde.test.brokenbinary.so: \
Cannot load library /r/soname/plasma/applets/org.kde.test.brokenbinary.so: libPlasma.so.8: cannot open shared \
object file: No such file or directory\"";
    const APPLET_STOCK_RU: &str = "error when loading applet \"org.kde.test.brokenbinary\" QList(\"Ошибка загрузки \
виджета: пакет «org.kde.test.brokenbinary» не существует.\", \"Невозможно найти компонент org.kde.test.brokenbinary\")";
    const APPLET_PATCHED: &str = "error when loading applet \"org.kde.test.brokenbinary\" QList(\"Error loading \
Applet: package org.kde.test.brokenbinary does not exist.\", \"This widget was built for a different version of \
Plasma and needs to be rebuilt. Please contact the widget's author for an updated version.\\n\\nCould not load \
plugin from /r/undefsym/plasma/applets/org.kde.test.brokenbinary.so: Cannot load library \
/r/undefsym/plasma/applets/org.kde.test.brokenbinary.so: /r/undefsym/plasma/applets/org.kde.test.brokenbinary.so: \
undefined symbol: symbol_removed_from_plasma\")";
    const APPLET_PATCHED_RU: &str = "error when loading applet \"org.kde.test.brokenbinary\" QList(\"Ошибка загрузки \
виджета: пакет «org.kde.test.brokenbinary» не существует.\", \"This widget was built for a different version of \
Plasma and needs to be rebuilt. Please contact the widget's author for an updated version.\\n\\nНе удалось загрузить \
модуль из файла «/r/soname/plasma/applets/org.kde.test.brokenbinary.so»: Невозможно загрузить библиотеку \
/r/soname/plasma/applets/org.kde.test.brokenbinary.so: libPlasma.so.8: невозможно открыть разделяемый объектный \
файл: Нет такого файла или каталога\")";
    const SEVENTASKS_LIB: &str = "error when loading applet \"io.gitgud.wackyideas.seventasks\" QList(\
\"file:///usr/share/plasma/plasmoids/io.gitgud.wackyideas.seventasks/contents/ui/main.qml:20:1: Cannot load \
library /usr/lib/qt6/qml/aeroshell/taskmanager/libtaskmanagerplugin.so: libprocesscore.so.10: cannot open shared \
object file: No such file or directory\")";
    const SEVENTASKS_QML: &str = "error when loading applet \"io.gitgud.wackyideas.seventasks\" QList(\
\"file:///home/antonio/.local/share/plasma/plasmoids/io.gitgud.wackyideas.seventasks/contents/ui/main.qml:516:23: \
Type Task unavailable\", \"file:///home/antonio/.local/share/plasma/plasmoids/io.gitgud.wackyideas.seventasks/\
contents/ui/Task.qml:35:5: Cannot assign to non-existent property \\\"windowTitle\\\"\")";
    // Живой журнал этой машины, 03.10.2026: виджеты сняты с диска, но
    // остались в раскладке.
    const LIVE_MISSING: &str = "error when loading applet \"Nori.Weather\" QList(\"Ошибка загрузки виджета: пакет \
«Nori.Weather» не существует.\", \"Невозможно найти компонент Nori.Weather\")";

    fn json_line(message: &str, t: u64) -> String {
        serde_json::json!({"MESSAGE": message, "_COMM": "x", "__REALTIME_TIMESTAMP": t.to_string()}).to_string()
    }

    #[test]
    fn kwin_new_and_old_formats_give_versions() {
        for m in [KWIN_NEW, &format!("kwin_core: {KWIN_NEW}"), KWIN_OLD] {
            let f = parse_message(m, 5).unwrap();
            assert_eq!(f.kind, PluginKind::Effect);
            assert_eq!(f.id, "brokenversiontest");
            assert_eq!(
                f.cause,
                Cause::KwinVersion { built_for: Some("6.7.4".into()), expected: Some("6.7.5".into()) },
                "{m}"
            );
        }
    }

    #[test]
    fn kwin_load_errors_and_plugins() {
        let f = parse_message(
            "Did not get KPluginFactory for \"aeroglide\":\"Cannot load library /usr/lib/qt6/plugins/kwin/effects/plugins/aeroglide.so: undefined symbol: _ZN4KWin1xE\"",
            1,
        )
        .unwrap();
        assert_eq!((f.kind, f.id.as_str()), (PluginKind::Effect, "aeroglide"));
        assert!(matches!(&f.cause, Cause::LoadFailed { detail } if detail.contains("undefined symbol")), "{f:?}");
        assert_eq!(f.paths, ["/usr/lib/qt6/plugins/kwin/effects/plugins/aeroglide.so"]);
        let p = parse_message("\"krohnkite\" has mismatching plugin version", 1).unwrap();
        assert_eq!(p.kind, PluginKind::KwinPlugin);
        assert!(parse_message("Wireless scan on \"wlan0\" failed", 1).is_none());
    }

    #[test]
    fn coreaddons_line_names_module_by_path() {
        let f = parse_message(COREADDONS, 1).unwrap();
        assert_eq!((f.kind, f.id.as_str()), (PluginKind::Applet, "org.kde.test.brokenbinary"));
        assert!(matches!(&f.cause, Cause::LoadFailed { detail } if detail.starts_with("libPlasma.so.8: cannot open")), "{f:?}");
        let deco = parse_message(
            "\"Could not load plugin from /usr/lib/qt6/plugins/org.kde.kdecoration3/org.smod.smod.so: x\"",
            1,
        )
        .unwrap();
        assert_eq!((deco.kind, deco.id.as_str()), (PluginKind::Decoration, "org.smod.smod"));
        // Чужой плагин (не эффект, не апплет, не оформление) — не наш вопрос.
        assert!(parse_message("\"Could not load plugin from /usr/lib/qt6/plugins/kf6/kio/x.so: y\"", 1).is_none());
    }

    #[test]
    fn applet_lines_classified_without_reading_translated_text() {
        let stock = parse_message(APPLET_STOCK_RU, 1).unwrap();
        assert_eq!(stock.cause, Cause::NotFound);
        for m in [APPLET_PATCHED, APPLET_PATCHED_RU] {
            let f = parse_message(m, 1).unwrap();
            assert!(matches!(&f.cause, Cause::LoadFailed { .. }), "{f:?}");
            assert_eq!(f.paths.len(), 1, "{:?}", f.paths);
        }
        let lib = parse_message(SEVENTASKS_LIB, 1).unwrap();
        assert!(matches!(&lib.cause, Cause::LoadFailed { detail } if detail.starts_with("libprocesscore.so.10")), "{lib:?}");
        assert!(lib.paths.contains(&"/usr/lib/qt6/qml/aeroshell/taskmanager/libtaskmanagerplugin.so".to_string()));
        assert!(lib.paths.iter().any(|p| p.ends_with("contents/ui/main.qml")), "{:?}", lib.paths);
        let qml = parse_message(SEVENTASKS_QML, 1).unwrap();
        assert!(matches!(&qml.cause, Cause::QmlBroken { detail } if detail.contains("Type Task unavailable")));
        assert_eq!(parse_message(LIVE_MISSING, 1).unwrap().id, "Nori.Weather");
    }

    #[test]
    fn merge_keeps_most_precise_cause() {
        // Штатная libplasma: сначала строка KCoreAddons с путём, затем
        // «пакет не найден» — итог «библиотека не загрузилась».
        let text = [json_line(COREADDONS, 10), json_line(APPLET_STOCK_RU, 11), json_line(KWIN_NEW, 12), json_line(KWIN_NEW, 20)]
            .join("\n");
        let all = failures_from_journal(&text);
        assert_eq!(all.len(), 2, "{all:#?}");
        let applet = all.iter().find(|f| f.kind == PluginKind::Applet).unwrap();
        assert!(matches!(applet.cause, Cause::LoadFailed { .. }));
        assert_eq!(applet.time_us, 11);
        assert_eq!(all.iter().find(|f| f.kind == PluginKind::Effect).unwrap().time_us, 20);
    }

    #[test]
    fn journal_json_and_answers() {
        let bytes = r#"{"MESSAGE":[104,105],"__REALTIME_TIMESTAMP":"7"}"#;
        assert_eq!(parse_journal_json(bytes), vec![JournalLine { message: "hi".into(), time_us: 7 }]);
        assert_eq!(journal_answer(true, Some(1), "", ""), JournalAnswer::Empty);
        assert_eq!(journal_answer(true, Some(1), "", "Compiled without pattern matching support"), JournalAnswer::RetryWithoutGrep);
        assert_eq!(journal_answer(false, Some(1), "", "No journal files"), JournalAnswer::Unavailable);
        assert_eq!(journal_answer(true, Some(0), "{}", ""), JournalAnswer::Text);
        let argv = journal_argv(true);
        assert!(argv.contains(&"--user".to_string()) && argv.contains(&"_COMM=plasmashell".to_string()));
        assert!(!journal_argv(false).iter().any(|a| a.starts_with("--grep")));
    }

    /// Вшитые метаданные как у SMOD Glow: неопределённой длины карта,
    /// автор с полем Name раньше имени модуля.
    fn cbor_blob() -> Vec<u8> {
        let mut d = b"junk\x00".to_vec();
        d.extend_from_slice(b"\x67KPlugin\xbf");
        d.extend_from_slice(b"\x67Authors\x81\xa1\x64Name\x66Sourish");
        d.extend_from_slice(b"\x70EnabledByDefault\xf4");
        d.extend_from_slice(b"\x64Name\x69SMOD Glow");
        d.extend_from_slice(b"\x68Name[ru]\x78\x0c\xd0\xa1\xd0\xb2\xd0\xb5\xd1\x87\xd0\xb5\xd0\xbd");
        d.extend_from_slice(b"\xff\x00");
        d
    }

    #[test]
    fn embedded_metadata_name_by_language() {
        let k = embedded_kplugin(&cbor_blob()).unwrap();
        let ru = crate::installed_scan::locale_name_keys("ru");
        let de = crate::installed_scan::locale_name_keys("de");
        assert_eq!(kplugin_name(&k, &de).as_deref(), Some("SMOD Glow"));
        assert_eq!(kplugin_name(&k, &ru).as_deref(), Some("Свечен"));
        assert!(embedded_kplugin(b"\x67KPlugin\xb9\xff\xff").is_none());
    }

    struct Disk {
        files: HashMap<String, (u64, Vec<u8>)>,
        owners: HashMap<String, String>,
        foreign: Vec<String>,
    }

    fn with_env<R>(disk: &Disk, f: impl FnOnce(&ResolveEnv) -> R) -> R {
        let mtime = |p: &str| disk.files.get(p).map(|x| x.0);
        let read = |p: &str| disk.files.get(p).map(|x| x.1.clone());
        let list = |dir: &str| {
            disk.files.keys().filter(|k| k.rsplit_once('/').map(|x| x.0) == Some(dir)).cloned().collect()
        };
        let owners = |paths: &[String]| {
            paths.iter().filter_map(|p| disk.owners.get(p).map(|o| (p.clone(), o.clone()))).collect()
        };
        let foreign = |p: &str| disk.foreign.iter().any(|x| x == p);
        let env = ResolveEnv {
            home: "/home/t",
            data_home: "/home/t/.local/share",
            config_home: "/home/t/.config",
            data_dirs: &["/usr/share".to_string()],
            plugin_dirs: &["/usr/lib/qt6/plugins".to_string()],
            locale: "de",
            mtime_us: &mtime,
            read: &read,
            list: &list,
            owners: &owners,
            foreign: &foreign,
        };
        f(&env)
    }

    #[test]
    fn resolve_maps_owner_name_and_drops_stale_evidence() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let glow = "/usr/lib/qt6/plugins/kwin/effects/plugins/smodglow.so";
        let mut disk = Disk {
            files: HashMap::from([
                (glow.to_string(), (100, cbor_blob())),
                (
                    "/home/t/.config/plasma-org.kde.plasma.desktop-appletsrc".to_string(),
                    (1, b"[Containments][1][Applets][2]\nplugin=Nori.Weather\n".to_vec()),
                ),
            ]),
            owners: HashMap::from([(glow.to_string(), "aeroshell-smod-git".to_string())]),
            foreign: vec!["aeroshell-smod-git".to_string()],
        };
        let effect = Failure {
            kind: PluginKind::Effect,
            id: "smodglow".into(),
            cause: Cause::KwinVersion { built_for: Some("6.7.4".into()), expected: Some("6.7.5".into()) },
            paths: vec![],
            time_us: 200,
        };
        let missing = parse_message(LIVE_MISSING, 50).unwrap();
        let mut gone = missing.clone();
        gone.id = "Start.Next.Menu".into();
        let found = with_env(&disk, |env| resolve(&[effect.clone(), missing.clone(), gone.clone()], env));
        assert_eq!(found.len(), 2, "снятый с панели виджет не показывается: {found:#?}");
        assert_eq!(found[0].name, "SMOD Glow");
        assert_eq!(found[0].package(), Some("aeroshell-smod-git"));
        assert_eq!(
            finding_line(&found[0]),
            "The “SMOD Glow” effect is off: it was built for KWin 6.7.4, but KWin 6.7.5 is installed. \
             Rebuild the AUR package aeroshell-smod-git."
        );
        assert_eq!(found[1].origin, Origin::Unknown);
        assert!(finding_line(&found[1]).contains("isn't installed anymore"));
        assert_eq!(rebuild_packages(&found), ["aeroshell-smod-git"]);

        // Пересобран после сообщения — журнал устарел, молчим.
        disk.files.get_mut(glow).unwrap().0 = 300;
        assert!(with_env(&disk, |env| resolve(&[effect.clone()], env)).is_empty());
        // Выключен человеком — тоже молчим.
        disk.files.get_mut(glow).unwrap().0 = 100;
        disk.files.insert("/home/t/.config/kwinrc".into(), (1, b"[Plugins]\nsmodglowEnabled=false\n".to_vec()));
        assert!(with_env(&disk, |env| resolve(&[effect.clone()], env)).is_empty());
    }

    #[test]
    fn resolve_stock_not_found_with_package_on_disk_is_load_failure() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let meta = "/home/t/.local/share/plasma/plasmoids/io.gitgud.wackyideas.seventasks/metadata.json";
        let disk = Disk {
            files: HashMap::from([(
                meta.to_string(),
                (10, br#"{"KPlugin":{"Name":"SevenTasks","Name[de]":"Sieben Aufgaben"}}"#.to_vec()),
            )]),
            owners: HashMap::new(),
            foreign: vec![],
        };
        let mut f = parse_message(APPLET_STOCK_RU, 50).unwrap();
        f.id = "io.gitgud.wackyideas.seventasks".into();
        let found = with_env(&disk, |env| resolve(&[f], env));
        assert_eq!(found.len(), 1);
        assert!(matches!(found[0].failure.cause, Cause::LoadFailed { .. }));
        assert_eq!(found[0].origin, Origin::UserDir);
        assert_eq!(found[0].name, "Sieben Aufgaben");
        assert!(finding_action(&found[0]).contains("from the store"));
        assert!(rebuild_packages(&found).is_empty());
    }
}
