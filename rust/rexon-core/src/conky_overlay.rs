//! Настройки виджета Conky поверх файла автора — «накладка».
//!
//! ЗАЧЕМ. Человек хочет сдвинуть виджет в другой угол, сделать фон
//! прозрачнее или поменять часы на 12-часовые. Программы-менеджеры conky
//! для этого переписывают файл автора построчно: при обновлении виджета из
//! магазина правки пропадают, регистр и форматирование портятся, а поле,
//! которого в файле не было, не вставляется вовсе. Rexon файл автора не
//! трогает. Рядом, в `$XDG_STATE_HOME/rexon/conky/overlay/`, лежит свой
//! файл настроек — накладка ([`render`]): он выполняет файл автора
//! (`dofile` с абсолютным путём) и переписывает нужные поля таблицы
//! `conky.config`. conky запускается с накладкой (`-c <накладка>`) и с
//! рабочим каталогом = папка файла автора, поэтому относительные пути
//! виджета (`lua_load ./x.lua`, `${image ./bg.png}`) работают как прежде.
//! «Сбросить» — удалить накладку.
//!
//! Замеры стенда (conky 1.25.2, Xvfb, свой HOME, 19–20.10.2026):
//! * `dofile` + замена `conky.config.gap_x` двигает окно; `conky.text`
//!   приходит из файла автора и в накладке меняется (`gsub`);
//! * conky следит за изменением ТОЛЬКО файла из `-c`: правка накладки
//!   подхватывается сама (≤2 с), правка файла автора — нет;
//! * `dofile` файла синтаксиса 1.9 падает с синтаксической ошибкой — такой
//!   файл сначала переводится в 1.10 в копию, накладка выполняет копию;
//! * в состоянии Lua файла настроек нет ни `conky_version`, ни
//!   `conky_config` — версию conky Rust узнаёт сам (`conky -v`).
//!
//! Разбор файла автора ([`parse_config`]) понимает только литералы: поле,
//! заданное выражением (`gap_x = 10 * scale`), показывается «заданным
//! виджетом», и его ползунок выключен, пока человек не задаст своё.
//!
//! Процесс conky с накладкой узнаётся по пути: имя накладки — закодированный
//! путь файла автора ([`original_of_launch`]), так что «Остановить»,
//! «Запущенные» и автозапуск видят виджет, как будто он запущен с файлом
//! автора.

use std::collections::BTreeMap;

use crate::animations::LookIo;
use crate::de_apply::Ctx;
use crate::i18n::{tr, trf};

/// Часть пути накладок: `…/rexon/conky/overlay/<ключ>.rexon.conf`.
pub const OVERLAY_SEGMENT: &str = "/rexon/conky/overlay/";
/// Окончание имени накладки.
pub const OVERLAY_SUFFIX: &str = ".rexon.conf";
/// Окончание имени файла с выбранными настройками.
pub const SETTINGS_SUFFIX: &str = ".json";
/// Окончание имени копии, переведённой из синтаксиса 1.9.
pub const CONVERTED_SUFFIX: &str = ".v110.lua";

// ════════════════════════════════════════════════════════════════════════════
//  Где лежит
// ════════════════════════════════════════════════════════════════════════════

/// Файлы накладки одного файла настроек виджета.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Накладка — с ней запускается conky.
    pub overlay: String,
    /// Выбор человека ([`Overrides`] в JSON): из него накладка пишется
    /// заново.
    pub settings: String,
    /// Копия файла синтаксиса 1.9, переведённая в 1.10.
    pub converted: String,
}

/// Каталог накладок: `<state>/rexon/conky/overlay`.
pub fn overlay_dir(ctx: &Ctx, xdg_state_home: Option<&str>) -> String {
    format!("{}/overlay", crate::conky_log::log_dir(ctx, xdg_state_home))
}

/// Ключ файла настроек `conf` — его путь, где `/` записан как `+s`, а сам
/// `+` — как `+p`. Знака `%` в ключе нет намеренно: путь накладки попадает
/// в `Exec=` автозапуска, где `%` служебный.
pub fn key(conf: &str) -> String {
    let mut out = String::new();
    for c in conf.trim_start_matches('/').chars() {
        match c {
            '+' => out.push_str("+p"),
            '/' => out.push_str("+s"),
            c => out.push(c),
        }
    }
    out
}

/// Файл настроек по ключу ([`key`] наоборот).
pub fn conf_of_key(key: &str) -> String {
    let mut out = String::from("/");
    let mut it = key.chars();
    while let Some(c) = it.next() {
        if c != '+' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('s') => out.push('/'),
            Some('p') => out.push('+'),
            Some(o) => {
                out.push('+');
                out.push(o);
            }
            None => out.push('+'),
        }
    }
    out
}

pub fn paths(ctx: &Ctx, xdg_state_home: Option<&str>, conf: &str) -> Paths {
    let dir = overlay_dir(ctx, xdg_state_home);
    let k = key(conf);
    Paths {
        overlay: format!("{dir}/{k}{OVERLAY_SUFFIX}"),
        settings: format!("{dir}/{k}{SETTINGS_SUFFIX}"),
        converted: format!("{dir}/{k}{CONVERTED_SUFFIX}"),
    }
}

/// Живые пути (`$XDG_STATE_HOME` из окружения).
pub fn live_paths(ctx: &Ctx, conf: &str) -> Paths {
    paths(ctx, std::env::var("XDG_STATE_HOME").ok().as_deref(), conf)
}

/// Файл автора, если `launch` — накладка Rexon; иначе `None`.
pub fn original_of_launch(launch: &str) -> Option<String> {
    let at = launch.rfind(OVERLAY_SEGMENT)?;
    let name = &launch[at + OVERLAY_SEGMENT.len()..];
    let key = name.strip_suffix(OVERLAY_SUFFIX)?;
    (!key.is_empty() && !key.contains('/')).then(|| conf_of_key(key))
}

// ════════════════════════════════════════════════════════════════════════════
//  Выбор человека
// ════════════════════════════════════════════════════════════════════════════

/// Что человек поменял у виджета. `None` — оставить, как у автора.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Overrides {
    /// Угол или край экрана — полное имя conky (`top_right`).
    pub alignment: Option<String>,
    pub gap_x: Option<i64>,
    pub gap_y: Option<i64>,
    pub min_width: Option<i64>,
    pub min_height: Option<i64>,
    /// Без фона (полностью прозрачный).
    pub bg_none: Option<bool>,
    /// Непрозрачность фона, 0–100 %.
    pub bg_opacity: Option<u8>,
    /// Цвет фона `RRGGBB`.
    pub bg_colour: Option<String>,
    /// Окно — как советует Rexon для этого рабочего стола.
    pub window_preset: Option<bool>,
    /// Обновлять раз в N секунд.
    pub update_interval: Option<f64>,
    /// Экран X11 (`xinerama_head`).
    pub xinerama_head: Option<u32>,
    /// Часы: `true` — 24 часа, `false` — 12.
    pub clock_24h: Option<bool>,
    /// Сетевой интерфейс в тексте виджета: прежнее имя → новое.
    pub net_ifaces: BTreeMap<String, String>,
}

impl Overrides {
    pub fn is_empty(&self) -> bool {
        *self == Overrides::default()
    }

    /// Задан ли фон хоть одним полем.
    pub fn touches_background(&self) -> bool {
        self.bg_none.is_some() || self.bg_opacity.is_some() || self.bg_colour.is_some()
    }
}

/// Полные имена выравнивания conky — сетка 3×3 по строкам.
pub const ALIGNMENTS: [&str; 9] = [
    "top_left",
    "top_middle",
    "top_right",
    "middle_left",
    "middle_middle",
    "middle_right",
    "bottom_left",
    "bottom_middle",
    "bottom_right",
];

/// Полное имя выравнивания по любому написанию conky (`tr`, `top_right`,
/// `top`, `center`); `None` — не выравнивание (`none` и прочее).
pub fn normalize_alignment(v: &str) -> Option<&'static str> {
    let v = v.trim().to_ascii_lowercase();
    Some(match v.as_str() {
        "tl" | "top_left" => "top_left",
        "tm" | "top_middle" | "top" => "top_middle",
        "tr" | "top_right" => "top_right",
        "ml" | "middle_left" | "left" => "middle_left",
        "mm" | "middle_middle" | "center" => "middle_middle",
        "mr" | "middle_right" | "right" => "middle_right",
        "bl" | "bottom_left" => "bottom_left",
        "bm" | "bottom_middle" | "bottom" => "bottom_middle",
        "br" | "bottom_right" => "bottom_right",
        _ => return None,
    })
}

// ════════════════════════════════════════════════════════════════════════════
//  Разбор таблицы conky.config (только литералы)
// ════════════════════════════════════════════════════════════════════════════

/// Значение поля файла автора.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Num(f64),
    Bool(bool),
    Str(String),
    /// Не литерал: выражение, вызов, таблица — Rexon его не вычисляет.
    Expr,
}

impl Value {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Num(n) if n.fract() == 0.0 => Some(*n as i64),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Name(String),
    Str(String),
    Num(f64),
    Sym(String),
}

/// Уровень длинной скобки `[==[` с позиции `i` (там `[`); `None` — не она.
fn long_bracket(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') {
        j += 1;
    }
    (b.get(j) == Some(&b'[')).then_some(j - i - 1)
}

/// Конец длинной строки уровня `level`, начатой открывающей скобкой у `i`:
/// `(содержимое, позиция за закрывающей скобкой)`.
fn long_string(s: &str, i: usize, level: usize) -> (String, usize) {
    let open = i + level + 2;
    let close = format!("]{}]", "=".repeat(level));
    let mut start = open;
    // Перевод строки сразу за открывающей скобкой в строку не входит.
    if s[start..].starts_with("\r\n") {
        start += 2;
    } else if s[start..].starts_with('\n') {
        start += 1;
    }
    match s[start..].find(&close) {
        Some(n) => (s[start..start + n].to_string(), start + n + close.len()),
        None => (s[start..].to_string(), s.len()),
    }
}

fn tokenize(s: &str) -> Vec<Tok> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'-' && b.get(i + 1) == Some(&b'-') {
            if let Some(level) = long_bracket(b, i + 2) {
                i = long_string(s, i + 2, level).1;
            } else {
                i = s[i..].find('\n').map_or(b.len(), |n| i + n + 1);
            }
            continue;
        }
        if let Some(level) = long_bracket(b, i) {
            let (text, end) = long_string(s, i, level);
            out.push(Tok::Str(text));
            i = end;
            continue;
        }
        if c == b'"' || c == b'\'' {
            let (text, end) = quoted(s, i);
            out.push(Tok::Str(text));
            i = end;
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let start = i;
            let hex = c == b'0' && matches!(b.get(i + 1), Some(b'x' | b'X'));
            if hex {
                i += 2;
            }
            while i < b.len() {
                let d = b[i];
                let exp = if hex { matches!(d, b'p' | b'P') } else { matches!(d, b'e' | b'E') };
                if exp && matches!(b.get(i + 1), Some(b'+' | b'-')) {
                    i += 2;
                } else if d.is_ascii_alphanumeric() || d == b'.' {
                    i += 1;
                } else {
                    break;
                }
            }
            let text = &s[start..i];
            let n = if hex {
                i64::from_str_radix(&text[2..], 16).ok().map(|v| v as f64)
            } else {
                text.parse::<f64>().ok()
            };
            out.push(n.map_or_else(|| Tok::Sym(text.to_string()), Tok::Num));
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok::Name(s[start..i].to_string()));
            continue;
        }
        // Двухзнаковые знаки — одним, чтобы `==` не читался как `=`.
        let two = s.get(i..i + 2).unwrap_or("");
        if matches!(two, "==" | "~=" | "<=" | ">=" | "..") {
            out.push(Tok::Sym(two.to_string()));
            i += 2;
            continue;
        }
        let ch = s[i..].chars().next().unwrap_or(' ');
        out.push(Tok::Sym(ch.to_string()));
        i += ch.len_utf8();
    }
    out
}

/// Строка в кавычках с позиции `i`: `(текст, позиция за ней)`.
fn quoted(s: &str, i: usize) -> (String, usize) {
    let quote = s.as_bytes()[i] as char;
    let mut out = String::new();
    let mut it = s[i + 1..].char_indices().peekable();
    while let Some((n, c)) = it.next() {
        if c == quote {
            return (out, i + 1 + n + 1);
        }
        if c == '\n' {
            return (out, i + 1 + n);
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some((_, e)) = it.next() else { break };
        match e {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'a' => out.push('\x07'),
            'b' => out.push('\x08'),
            'f' => out.push('\x0c'),
            'v' => out.push('\x0b'),
            '\n' => out.push('\n'),
            'z' => {
                while it.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                    it.next();
                }
            }
            'x' => {
                let h: String = (0..2).filter_map(|_| it.next().map(|(_, c)| c)).collect();
                if let Ok(v) = u8::from_str_radix(&h, 16) {
                    out.push(v as char);
                }
            }
            d if d.is_ascii_digit() => {
                let mut v = d.to_digit(10).unwrap_or(0);
                for _ in 0..2 {
                    match it.peek() {
                        Some((_, c)) if c.is_ascii_digit() => {
                            v = v * 10 + c.to_digit(10).unwrap_or(0);
                            it.next();
                        }
                        _ => break,
                    }
                }
                if let Some(c) = char::from_u32(v) {
                    out.push(c);
                }
            }
            other => out.push(other),
        }
    }
    (out, s.len())
}

/// Значение из токенов поля: литерал — само значение, иначе [`Value::Expr`].
fn literal(toks: &[Tok]) -> Value {
    match toks {
        [Tok::Num(n)] => Value::Num(*n),
        [Tok::Sym(m), Tok::Num(n)] if m == "-" => Value::Num(-n),
        [Tok::Str(s)] => Value::Str(s.clone()),
        [Tok::Name(n)] if n == "true" => Value::Bool(true),
        [Tok::Name(n)] if n == "false" => Value::Bool(false),
        _ => Value::Expr,
    }
}

/// Поля таблицы `conky.config = { … }` файла настроек 1.10. `None` —
/// таблицы нет (файл 1.9 или собирает её кодом).
pub fn parse_config(text: &str) -> Option<BTreeMap<String, Value>> {
    let toks = tokenize(text);
    let start = toks.windows(5).position(|w| {
        matches!(
            w,
            [Tok::Name(a), Tok::Sym(dot), Tok::Name(b), Tok::Sym(eq), Tok::Sym(open)]
                if a == "conky" && dot == "." && b == "config" && eq == "=" && open == "{"
        )
    })? + 5;
    let mut out = BTreeMap::new();
    let mut i = start;
    while i < toks.len() {
        if matches!(&toks[i], Tok::Sym(s) if s == "}") {
            break;
        }
        // Ключ: `имя =` или `["имя"] =`.
        let (key, value_at) = match (&toks[i], toks.get(i + 1), toks.get(i + 2), toks.get(i + 3)) {
            (Tok::Name(k), Some(Tok::Sym(eq)), _, _) if eq == "=" => (Some(k.clone()), i + 2),
            (Tok::Sym(o), Some(Tok::Str(k)), Some(Tok::Sym(c)), Some(Tok::Sym(eq)))
                if o == "[" && c == "]" && eq == "=" =>
            {
                (Some(k.clone()), i + 4)
            }
            _ => (None, i),
        };
        // Значение — до запятой или точки с запятой своего уровня.
        let mut depth = 0i32;
        let mut j = value_at;
        while j < toks.len() {
            match &toks[j] {
                Tok::Sym(s) if matches!(s.as_str(), "{" | "(" | "[") => depth += 1,
                Tok::Sym(s) if matches!(s.as_str(), "}" | ")" | "]") => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                Tok::Sym(s) if depth == 0 && (s == "," || s == ";") => break,
                _ => {}
            }
            j += 1;
        }
        if let Some(k) = key {
            out.insert(k, literal(&toks[value_at..j]));
        }
        i = j;
        if matches!(toks.get(i), Some(Tok::Sym(s)) if s == "," || s == ";") {
            i += 1;
        }
    }
    Some(out)
}

// ════════════════════════════════════════════════════════════════════════════
//  Синтаксис 1.9 → 1.10 (в копию)
// ════════════════════════════════════════════════════════════════════════════
//
// Файл conky до 1.10: строки `имя значение` до строки `TEXT`, после неё —
// текст виджета как есть. `#` начинает комментарий (`\#` — сам знак).
// conky 1.10+ переводит такой файл сам, но только когда он передан в `-c`:
// `dofile` в накладке падает на нём синтаксической ошибкой. Поэтому
// накладка поверх виджета 1.9 выполняет его копию, переведённую Rexon по
// описанию формата ([`convert_19`]); сам файл автора не трогается, а без
// правок виджет запускается с ним, как раньше.

/// Синтаксис файла настроек.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Syntax {
    /// conky 1.10+: Lua, `conky.config` / `conky.text`.
    V110,
    /// До 1.10: `имя значение` и `TEXT`.
    V19,
}

/// Синтаксис по содержимому: строка `TEXT` без `conky.config`/`conky.text`
/// — 1.9.
pub fn syntax(text: &str) -> Syntax {
    let lua = tokenize(text).windows(3).any(|w| {
        matches!(w, [Tok::Name(a), Tok::Sym(d), Tok::Name(b)] if a == "conky" && d == "." && (b == "config" || b == "text"))
    });
    if !lua && text.lines().any(|l| l.trim() == "TEXT") {
        Syntax::V19
    } else {
        Syntax::V110
    }
}

/// Поля 1.9 вида «ширина высота» — по полю 1.10 на каждое число.
const SPLIT_FIELDS: &[(&str, &str, &str)] = &[
    ("minimum_size", "minimum_width", "minimum_height"),
    ("default_bar_size", "default_bar_width", "default_bar_height"),
    ("default_graph_size", "default_graph_width", "default_graph_height"),
    ("default_gauge_size", "default_gauge_width", "default_gauge_height"),
];

/// Поле-цвет: значение — строка, даже если состоит из одних цифр (`000000`).
fn is_colour_field(name: &str) -> bool {
    name.contains("color") || name.contains("colour")
}

/// Строка 1.9 без комментария (`\#` — знак `#`, а не начало комментария).
fn strip_comment_19(line: &str) -> String {
    let mut out = String::new();
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' if it.peek() == Some(&'#') => {
                out.push('#');
                it.next();
            }
            '#' => break,
            c => out.push(c),
        }
    }
    out
}

/// Значение поля 1.9 строкой Lua.
fn value_19(name: &str, raw: &str) -> String {
    let v = raw.trim();
    if v.is_empty() {
        return "true".into();
    }
    match v.to_ascii_lowercase().as_str() {
        "yes" | "true" => return "true".into(),
        "no" | "false" => return "false".into(),
        _ => {}
    }
    let numeric = v.parse::<f64>().is_ok() && v.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'));
    if numeric && !is_colour_field(name) {
        return v.trim_start_matches('+').to_string();
    }
    let unquoted = v.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(v);
    lua_str(unquoted)
}

/// Длинная строка Lua с текстом `text` — уровень скобки такой, что в тексте
/// не встречается её закрытия.
fn long_lua_string(text: &str) -> String {
    let mut level = 0;
    while text.contains(&format!("]{}", "=".repeat(level))) && level < 64 {
        level += 1;
    }
    let eq = "=".repeat(level);
    // Перевод строки сразу за открывающей скобкой Lua отбрасывает сам.
    format!("[{eq}[\n{text}]{eq}]")
}

/// Перевести файл 1.9 в 1.10. Порядок полей сохраняется; комментарии
/// настроек отбрасываются, текст после `TEXT` переносится как есть (conky
/// 1.10 убирает из него комментарии так же, как 1.9 — сверено стендом).
pub fn convert_19(text: &str) -> String {
    let mut fields: Vec<String> = Vec::new();
    let mut lines = text.split_inclusive('\n');
    let mut body = String::new();
    for raw in lines.by_ref() {
        let line = raw.trim_end_matches(['\n', '\r']);
        if line.trim() == "TEXT" {
            break;
        }
        let clean = strip_comment_19(line);
        let clean = clean.trim();
        if clean.is_empty() {
            continue;
        }
        let (name, value) = clean.split_once(char::is_whitespace).unwrap_or((clean, ""));
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        if let Some((_, w, h)) = SPLIT_FIELDS.iter().find(|(n, _, _)| *n == name) {
            let mut it = value.split_whitespace();
            if let Some(v) = it.next() {
                fields.push(format!("    {w} = {},", value_19(w, v)));
            }
            if let Some(v) = it.next() {
                fields.push(format!("    {h} = {},", value_19(h, v)));
            }
            continue;
        }
        // У 1.10 шрифт Xft — просто `font`.
        let name = if name == "xftfont" { "font" } else { name };
        fields.push(format!("    {name} = {},", value_19(name, value)));
    }
    for raw in lines {
        body.push_str(raw);
    }
    let body = body.strip_suffix('\n').unwrap_or(&body);
    let mut out = String::from("-- Rexon: a copy of a Conky 1.9 file translated to the 1.10 syntax.\n");
    out.push_str("conky.config = {\n");
    for f in fields {
        out.push_str(&f);
        out.push('\n');
    }
    out.push_str("}\n");
    out.push_str(&format!("conky.text = {}\n", long_lua_string(body)));
    out
}

/// Поля файла автора и его синтаксис (1.9 — через перевод в памяти).
pub fn author_config(text: &str) -> (Syntax, Option<BTreeMap<String, Value>>) {
    match syntax(text) {
        Syntax::V19 => (Syntax::V19, parse_config(&convert_19(text))),
        Syntax::V110 => (Syntax::V110, parse_config(text)),
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Текущие значения: накладка поверх автора
// ════════════════════════════════════════════════════════════════════════════

/// Цвет conky (`#RGB`, `#ARGB`, `RRGGBB`, `#RRGGBB`, `#AARRGGBB`, несколько
/// имён) → `(RRGGBB, альфа 0–255, если задана)`.
pub fn parse_colour(v: &str) -> Option<(String, Option<u8>)> {
    let v = v.trim();
    let named = match v.to_ascii_lowercase().as_str() {
        "black" => Some("000000"),
        "white" => Some("FFFFFF"),
        "red" => Some("FF0000"),
        "green" => Some("00FF00"),
        "blue" => Some("0000FF"),
        "grey" | "gray" => Some("BEBEBE"),
        _ => None,
    };
    if let Some(n) = named {
        return Some((n.to_string(), None));
    }
    let hex = v.strip_prefix('#').unwrap_or(v);
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let dup = |s: &str| s.chars().flat_map(|c| [c, c]).collect::<String>().to_ascii_uppercase();
    match hex.len() {
        3 => Some((dup(hex), None)),
        4 if v.starts_with('#') => Some((dup(&hex[1..]), u8::from_str_radix(&dup(&hex[..1]), 16).ok())),
        6 => Some((hex.to_ascii_uppercase(), None)),
        8 if v.starts_with('#') => Some((hex[2..].to_ascii_uppercase(), u8::from_str_radix(&hex[..2], 16).ok())),
        _ => None,
    }
}

/// Поле с подписью «откуда».
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Field<T> {
    pub value: T,
    /// Задано человеком в Rexon (иначе — как у автора).
    pub changed: bool,
    /// У автора не литерал: Rexon не знает значения, ползунок выключен.
    pub locked: bool,
}

/// Значения виджета, как они действуют сейчас.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Current {
    pub alignment: Field<String>,
    pub gap_x: Field<i64>,
    pub gap_y: Field<i64>,
    pub min_width: Field<i64>,
    pub min_height: Field<i64>,
    pub bg_none: Field<bool>,
    pub bg_opacity: Field<u8>,
    pub bg_colour: Field<String>,
    pub update_interval: Field<f64>,
    pub xinerama_head: Field<i64>,
    pub window_preset: bool,
}

fn field<T: Clone>(own: Option<T>, author: Option<&Value>, read: impl Fn(&Value) -> Option<T>, default: T) -> Field<T> {
    if let Some(v) = own {
        return Field { value: v, changed: true, locked: false };
    }
    match author {
        Some(Value::Expr) => Field { value: default, changed: false, locked: true },
        Some(v) => Field { value: read(v).unwrap_or(default), changed: false, locked: false },
        None => Field { value: default, changed: false, locked: false },
    }
}

/// Непрозрачность фона у автора (0–100) и «без фона».
fn author_background(cfg: &BTreeMap<String, Value>) -> (bool, u8, String) {
    let colour = cfg
        .get("own_window_colour")
        .or_else(|| cfg.get("own_window_color"))
        .and_then(Value::as_str)
        .and_then(parse_colour);
    let hex = colour.as_ref().map(|(c, _)| c.clone()).unwrap_or_else(|| "000000".into());
    if cfg.get("own_window_transparent").and_then(Value::as_bool) == Some(true) {
        return (true, 0, hex);
    }
    if let Some(a) = colour.as_ref().and_then(|(_, a)| *a) {
        return (a == 0, percent(a), hex);
    }
    if cfg.get("own_window_argb_visual").and_then(Value::as_bool) == Some(true) {
        if let Some(a) = cfg.get("own_window_argb_value").and_then(Value::as_i64) {
            let a = a.clamp(0, 255) as u8;
            return (a == 0, percent(a), hex);
        }
    }
    (false, 100, hex)
}

fn percent(alpha: u8) -> u8 {
    ((alpha as u32 * 100 + 127) / 255) as u8
}

fn alpha_of(percent: u8) -> u8 {
    ((percent.min(100) as u32 * 255 + 50) / 100) as u8
}

/// Как виджет выглядит сейчас: выбор человека поверх файла автора.
pub fn current(cfg: &BTreeMap<String, Value>, ov: &Overrides) -> Current {
    let (none, opacity, colour) = author_background(cfg);
    let bg_locked = ["own_window_transparent", "own_window_argb_value", "own_window_colour", "own_window_color"]
        .iter()
        .any(|k| cfg.get(*k) == Some(&Value::Expr));
    Current {
        alignment: field(
            ov.alignment.clone(),
            cfg.get("alignment"),
            |v| v.as_str().and_then(normalize_alignment).map(str::to_string),
            "top_left".to_string(),
        ),
        gap_x: field(ov.gap_x, cfg.get("gap_x"), Value::as_i64, 0),
        gap_y: field(ov.gap_y, cfg.get("gap_y"), Value::as_i64, 0),
        min_width: field(ov.min_width, cfg.get("minimum_width"), Value::as_i64, 0),
        min_height: field(ov.min_height, cfg.get("minimum_height"), Value::as_i64, 0),
        bg_none: Field {
            value: ov.bg_none.unwrap_or(none),
            changed: ov.bg_none.is_some(),
            locked: bg_locked && ov.bg_none.is_none(),
        },
        bg_opacity: Field {
            value: ov.bg_opacity.unwrap_or(opacity),
            changed: ov.bg_opacity.is_some(),
            locked: bg_locked && ov.bg_opacity.is_none(),
        },
        bg_colour: Field {
            value: ov.bg_colour.clone().unwrap_or(colour),
            changed: ov.bg_colour.is_some(),
            locked: bg_locked && ov.bg_colour.is_none(),
        },
        update_interval: field(ov.update_interval, cfg.get("update_interval"), Value::as_f64, 1.0),
        xinerama_head: field(ov.xinerama_head.map(i64::from), cfg.get("xinerama_head"), Value::as_i64, -1),
        window_preset: ov.window_preset == Some(true),
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Накладка
// ════════════════════════════════════════════════════════════════════════════

/// Строка Lua в кавычках.
pub fn lua_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\{:03}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn lua_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Версия conky `(старшая, младшая)` из вывода `conky -v`
/// (`conky 1.25.2-pre-6caddca3 compiled …`).
pub fn parse_version(out: &str) -> Option<(u32, u32)> {
    let first = out.lines().find(|l| l.trim_start().starts_with("conky "))?;
    let ver = first.trim_start().strip_prefix("conky ")?.split_whitespace().next()?;
    let mut it = ver.split(|c: char| !c.is_ascii_digit());
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

/// Версия установленного conky; `None` — не узнать.
pub fn conky_version(io: &dyn LookIo) -> Option<(u32, u32)> {
    let (ok, out) = io.run(&["conky".to_string(), "-v".to_string()]);
    if !ok {
        return None;
    }
    parse_version(&out)
}

/// С какой версии conky прозрачность фона — альфа цвета (`#AARRGGBB`), а
/// `own_window_argb_value`/`own_window_transparent` устарели.
pub const ALPHA_COLOUR_SINCE: (u32, u32) = (1, 23);

/// Обстановка, под которую пишется накладка.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Env {
    /// Версия conky; `None` — не узнали (пишутся поля, понятные всем).
    pub version: Option<(u32, u32)>,
    /// Рабочий стол (`Ctx::de`) — для советов по окну.
    pub de: String,
}

// ── Окно по рабочему столу ──────────────────────────────────────────────
//
// Какое окно conky держится «на столе», зависит от рабочего стола и сеанса
// (замеры и описание настроек conky ≥ 1.25):
// * X11 — обычное окно с подсказками «без рамки, под всеми окнами, на всех
//   столах, не в панели задач»: тип `desktop` на столах со своим окном
//   значков (Caja, Nemo, xfdesktop) перекрывается этим окном при щелчке;
// * Wayland с wlr-layer-shell (KWin, wlroots, COSMIC, labwc, Treeland) —
//   слой фона: тип `desktop`; у KWin с conky 1.25 — обычное окно с теми же
//   подсказками и пространством имён `desktop` (виден при «Показать рабочий
//   стол», `dock` резервировал бы место у края);
// * niri: `desktop` не попадает в фон обзора — обычное окно с `below`
//   (нижний слой);
// * GNOME, Cinnamon, Pantheon на Wayland слоёв не знают: виджет — обычное
//   окно, никакие поля этого не меняют.

/// Подсказки окна X11 (и нижний слой на Wayland у conky 1.25).
pub const DESKTOP_HINTS: &str = "undecorated,below,sticky,skip_taskbar,skip_pager";

/// Совет Rexon для окна виджета.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WindowPreset {
    pub own_window_type: &'static str,
    pub hints: Option<&'static str>,
    /// `own_window_namespace` (conky ≥ 1.25, только Wayland).
    pub namespace: Option<&'static str>,
}

/// Рабочие столы, у которых на Wayland нет wlr-layer-shell (то же решение
/// говорит и пояснение группы виджетов, `categories::group_note`).
pub fn wayland_without_layers(de_id: &str) -> bool {
    use crate::de_customization::De;
    matches!(crate::de_customization::de_from_id(de_id), Some(De::Gnome | De::Cinnamon | De::Pantheon))
}

/// Окно виджета для рабочего стола `de_id` и сеанса. `None` — подходящего
/// окна нет (Wayland без слоёв).
pub fn window_preset(de_id: &str, wayland: bool, version: Option<(u32, u32)>) -> Option<WindowPreset> {
    let normal = WindowPreset { own_window_type: "normal", hints: Some(DESKTOP_HINTS), namespace: None };
    if !wayland {
        return Some(normal);
    }
    if wayland_without_layers(de_id) {
        return None;
    }
    let desktop = WindowPreset { own_window_type: "desktop", hints: None, namespace: None };
    // Слой по подсказкам у обычного окна и пространство имён — с conky 1.25.
    if !version.is_some_and(|v| v >= (1, 25)) {
        return Some(desktop);
    }
    Some(match de_id {
        "kde" => WindowPreset { namespace: Some("desktop"), ..normal },
        "niri" => normal,
        _ => desktop,
    })
}

/// Что сказать о виджете на этом рабочем столе; `None` — нечего.
pub fn preset_note(de_id: &str, wayland: bool) -> Option<String> {
    (wayland && wayland_without_layers(de_id)).then(|| {
        tr("In the Wayland session of this desktop the widget is a regular window: it can be covered by other \
            windows and shows up in Alt+Tab. The X11 session (chosen on the login screen) keeps it on the desktop.")
    })
}

fn preset_lines(p: &WindowPreset, version: Option<(u32, u32)>, indent: &str, out: &mut Vec<String>) {
    out.push(format!("{indent}c.own_window_type = {}", lua_str(p.own_window_type)));
    match p.hints {
        Some(h) => out.push(format!("{indent}c.own_window_hints = {}", lua_str(h))),
        None => out.push(format!("{indent}c.own_window_hints = nil")),
    }
    if version.is_some_and(|v| v >= (1, 25)) {
        match p.namespace {
            Some(n) => out.push(format!("{indent}c.own_window_namespace = {}", lua_str(n))),
            None => out.push(format!("{indent}c.own_window_namespace = nil")),
        }
    }
}

/// Строки Lua окна по совету Rexon: ветка Wayland и ветка X11 — сеанс
/// узнаётся при запуске conky (`WAYLAND_DISPLAY`), автозапуск в другом
/// сеансе тоже получит верное окно.
fn window_lines(env: &Env, out: &mut Vec<String>) {
    let x11 = window_preset(&env.de, false, env.version);
    let wl = window_preset(&env.de, true, env.version);
    out.push("c.own_window = true".into());
    if env.version.is_some_and(|v| v < ALPHA_COLOUR_SINCE) {
        out.push("c.own_window_argb_visual = true".into());
    }
    out.push("if os.getenv(\"WAYLAND_DISPLAY\") then".into());
    match &wl {
        Some(p) => preset_lines(p, env.version, "    ", out),
        // Слоёв нет — окно остаётся, каким его задал автор.
        None => out.push("    -- no layer-shell here: the author's window settings stay".into()),
    }
    out.push("else".into());
    if let Some(p) = &x11 {
        preset_lines(p, env.version, "    ", out);
    }
    out.push("end".into());
}

/// Строки Lua для фона.
fn background_lines(cfg: &BTreeMap<String, Value>, ov: &Overrides, env: &Env, out: &mut Vec<String>) {
    let (author_none, author_opacity, author_colour) = author_background(cfg);
    let none = ov.bg_none.unwrap_or(author_none && ov.bg_opacity.is_none());
    let opacity = if none { 0 } else { ov.bg_opacity.unwrap_or(author_opacity) };
    let colour = ov.bg_colour.clone().unwrap_or(author_colour);
    let alpha = alpha_of(opacity);
    out.push("c.own_window = true".into());
    let modern = env.version.is_some_and(|v| v >= ALPHA_COLOUR_SINCE);
    if modern {
        // Альфа — в самом цвете; устаревшие поля автора иначе спорили бы с
        // ним. Имя поля с 1.2x — `own_window_color`, прежнее
        // `own_window_colour` conky понимает с предупреждением, но оба сразу
        // в таблице — кто победит, не определено.
        out.push("c.own_window_transparent = nil".into());
        out.push("c.own_window_argb_value = nil".into());
        out.push("c.own_window_colour = nil".into());
        out.push(format!("c.own_window_color = {}", lua_str(&format!("#{alpha:02X}{colour}"))));
    } else {
        out.push(format!("c.own_window_transparent = {}", alpha == 0));
        out.push("c.own_window_argb_visual = true".into());
        out.push(format!("c.own_window_argb_value = {alpha}"));
        out.push("c.own_window_color = nil".into());
        out.push(format!("c.own_window_colour = {}", lua_str(&colour)));
    }
}

/// Текст накладки: выполнить `source` (файл автора или его перевод в 1.10)
/// и переписать выбранные поля. `cfg` — поля автора ([`parse_config`];
/// для фона, когда человек задал не всё).
pub fn render(source: &str, cfg: &BTreeMap<String, Value>, ov: &Overrides, env: &Env) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(a) = ov.alignment.as_deref().and_then(normalize_alignment) {
        lines.push(format!("c.alignment = {}", lua_str(a)));
    }
    for (name, v) in [
        ("gap_x", ov.gap_x),
        ("gap_y", ov.gap_y),
        ("minimum_width", ov.min_width),
        ("minimum_height", ov.min_height),
    ] {
        if let Some(v) = v {
            lines.push(format!("c.{name} = {v}"));
        }
    }
    if ov.window_preset == Some(true) {
        window_lines(env, &mut lines);
    }
    if ov.touches_background() {
        background_lines(cfg, ov, env, &mut lines);
    }
    if let Some(v) = ov.update_interval.filter(|v| *v > 0.0) {
        lines.push(format!("c.update_interval = {}", lua_num(v)));
    }
    if let Some(h) = ov.xinerama_head {
        lines.push(format!("c.xinerama_head = {h}"));
    }
    text_lines(ov, &mut lines);
    // Окно и фон оба включают своё окно — одной строки хватит.
    let mut seen_own_window = false;
    lines.retain(|l| l != "c.own_window = true" || !std::mem::replace(&mut seen_own_window, true));
    let mut out = String::new();
    out.push_str("-- Rexon: the widget's settings on top of the author's file.\n");
    out.push_str("-- The author's file stays untouched; Rexon rewrites this file on every change.\n");
    out.push_str(&format!("dofile({})\n", lua_str(source)));
    out.push_str("conky.config = conky.config or {}\n");
    out.push_str("local c = conky.config\n");
    for l in lines {
        out.push_str(&l);
        out.push('\n');
    }
    out
}

// ════════════════════════════════════════════════════════════════════════════
//  Экран (X11)
// ════════════════════════════════════════════════════════════════════════════
//
// `xinerama_head` — номер экрана в списке Xinerama. Сервер X с RandR 1.5
// строит этот список из мониторов RandR в том же порядке, что печатает
// `xrandr --listmonitors` (основной — первым). На Wayland conky выбирать
// экран не умеет (один вывод на всё, открытые задачи conky #2106, #2111) —
// пункт скрыт с пояснением.

/// Монитор X11.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Monitor {
    /// Номер для `xinerama_head`.
    pub index: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub primary: bool,
}

/// Мониторы из вывода `xrandr --listmonitors`
/// (` 0: +*eDP-1 1920/344x1080/193+0+0  eDP-1`).
pub fn parse_monitors(out: &str) -> Vec<Monitor> {
    let mut list = Vec::new();
    for line in out.lines() {
        let Some((idx, rest)) = line.trim().split_once(':') else { continue };
        let Ok(index) = idx.trim().parse::<u32>() else { continue };
        let mut parts = rest.split_whitespace();
        let (Some(flags_name), Some(geom)) = (parts.next(), parts.next()) else { continue };
        let primary = flags_name.trim_start_matches('+').starts_with('*');
        let name = flags_name.trim_start_matches(['+', '*']).to_string();
        // 1920/344x1080/193+0+0
        let Some((w, rest)) = geom.split_once('/') else { continue };
        let Some((_, rest)) = rest.split_once('x') else { continue };
        let Some((h, rest)) = rest.split_once('/') else { continue };
        let pos: String = rest.chars().skip_while(|c| c.is_ascii_digit()).collect();
        // Положение — `+X+Y`, отрицательное — `+-200`.
        let nums: Vec<&str> = pos.split('+').filter(|p| !p.is_empty()).collect();
        let (Ok(width), Ok(height)) = (w.parse(), h.parse()) else { continue };
        let x = nums.first().and_then(|n| n.parse().ok()).unwrap_or(0);
        let y = nums.get(1).and_then(|n| n.parse().ok()).unwrap_or(0);
        list.push(Monitor { index, name, width, height, x, y, primary });
    }
    list
}

/// Мониторы X11 этого сеанса; пусто — сеанс Wayland, один монитор не
/// спрашивается, или узнать нечем (`xrandr` нет).
pub fn monitors(io: &dyn LookIo, wayland: bool) -> Vec<Monitor> {
    if wayland || !io.have("xrandr") {
        return Vec::new();
    }
    let (ok, out) = io.run(&["xrandr".to_string(), "--listmonitors".to_string()]);
    if ok { parse_monitors(&out) } else { Vec::new() }
}

/// Почему выбора экрана нет; `None` — он есть.
pub fn monitor_note(wayland: bool) -> Option<String> {
    wayland.then(|| tr("Conky can't choose a screen in a Wayland session yet — the widget appears where the desktop puts it."))
}

// ════════════════════════════════════════════════════════════════════════════
//  Часы и сеть в тексте виджета
// ════════════════════════════════════════════════════════════════════════════
//
// Формат часов и имя сетевого интерфейса живут в тексте виджета
// (`${time %H:%M}`, `${downspeed eth0}`), а не в полях `conky.config`.
// Накладка меняет их на лету: `conky.text` из файла автора проходит через
// замену внутри переменных `${…}` (замер стенда: `gsub` в накладке
// меняет текст, файл автора не трогается). Сеть: у каждого прежнего
// имени — своя замена, за один проход по словам, так что `eth0 → wlan0` и
// `wlan0 → eth0` разом не сливаются в одно имя (у программ-менеджеров conky
// оба имени становились одним). Lua-сценарии виджета (кольца с `arg =
// 'eth0'`) так не меняются — это код автора.

/// Переменные conky, чей первый аргумент — сетевой интерфейс.
pub const NET_VARS: &[&str] = &[
    "addr",
    "addrs",
    "v6addrs",
    "downspeed",
    "downspeedf",
    "downspeedgraph",
    "upspeed",
    "upspeedf",
    "upspeedgraph",
    "totaldown",
    "totalup",
    "if_up",
];

fn is_net_var(name: &str) -> bool {
    NET_VARS.contains(&name) || name.starts_with("wireless_")
}

/// Переменные времени: формат strftime — их аргумент.
pub const TIME_VARS: &[&str] = &["time", "utime", "tztime"];

/// Начала переменных `${…}` текста — `(имя, аргументы до вложенной
/// переменной)`, вложенные — тоже.
fn var_heads(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'$' && b[i + 1] == b'{' {
            let start = i + 2;
            let mut j = start;
            while j < b.len() && b[j] != b'}' && !(b[j] == b'$' && b.get(j + 1) == Some(&b'{')) {
                j += 1;
            }
            let head = &text[start..j];
            let trimmed = head.trim_start();
            let name: String = trimmed.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            let args = trimmed[name.len()..].to_string();
            out.push((name, args));
            i = start;
            continue;
        }
        i += 1;
    }
    out
}

/// Какие часы в тексте: `Some(true)` — 24 часа (`%H`, `%k`, `%R`, `%T`),
/// `Some(false)` — 12 (`%I`, `%l`, `%r`); `None` — часов в тексте нет.
pub fn clock_style(text: &str) -> Option<bool> {
    let mut found = None;
    for (name, args) in var_heads(text) {
        if !TIME_VARS.contains(&name.as_str()) {
            continue;
        }
        let codes: Vec<char> = args.split('%').skip(1).filter_map(|p| p.chars().next()).collect();
        if codes.iter().any(|c| matches!(c, 'H' | 'k' | 'R' | 'T')) {
            return Some(true);
        }
        if codes.iter().any(|c| matches!(c, 'I' | 'l' | 'r')) {
            found = Some(false);
        }
    }
    found
}

/// Имена сетевых интерфейсов, которые упоминает текст, по порядку.
pub fn net_names(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (name, args) in var_heads(text) {
        if !is_net_var(&name) {
            continue;
        }
        if let Some(first) = args.split([' ', '\t', ',']).find(|w| !w.is_empty()) {
            let n = first.trim().to_string();
            if !n.is_empty() && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// Сетевой интерфейс машины.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Iface {
    pub name: String,
    /// `wifi`, `ethernet` или `virtual`.
    pub kind: &'static str,
}

/// Интерфейсы из `/sys/class/net` (без `lo`): сначала настоящие — Wi-Fi и
/// провод, — затем виртуальные.
pub fn interfaces(io: &dyn LookIo) -> Vec<Iface> {
    let mut out: Vec<Iface> = io
        .list_dir("/sys/class/net")
        .into_iter()
        .filter(|n| n != "lo")
        .map(|name| {
            let uevent = io.read(&format!("/sys/class/net/{name}/uevent")).unwrap_or_default();
            let kind = if uevent.lines().any(|l| l.trim() == "DEVTYPE=wlan") {
                "wifi"
            } else if io.read(&format!("/sys/class/net/{name}/device/uevent")).is_some() {
                "ethernet"
            } else {
                "virtual"
            };
            Iface { name, kind }
        })
        .collect();
    out.sort_by_key(|i| (i.kind == "virtual", i.kind != "wifi", i.name.clone()));
    out
}

/// Lua: обход переменных `${…}` текста с заменой начала каждой (`f`).
const LUA_EACH_VAR: &str = r#"local function rexon_each(t, f)
    return (t:gsub("%$(%b{})", function(b)
        local inner = rexon_each(b:sub(2, -2), f)
        local head, tail = inner:match("^(.-)(%$.*)$")
        if not head then head, tail = inner, "" end
        return "${" .. f(head) .. tail .. "}"
    end))
end
"#;

fn text_lines(ov: &Overrides, out: &mut Vec<String>) {
    let clock = ov.clock_24h;
    let nets: Vec<(&String, &String)> = ov.net_ifaces.iter().filter(|(a, b)| a != b && !b.is_empty()).collect();
    if clock.is_none() && nets.is_empty() {
        return;
    }
    out.push("if type(conky.text) == \"string\" then".into());
    out.extend(LUA_EACH_VAR.lines().map(|l| format!("    {l}")));
    if !nets.is_empty() {
        let map = nets.iter().map(|(a, b)| format!("[{}] = {}", lua_str(a), lua_str(b))).collect::<Vec<_>>().join(", ");
        let vars = NET_VARS.iter().map(|v| format!("{v} = true")).collect::<Vec<_>>().join(", ");
        out.push(format!("    local rexon_net = {{ {map} }}"));
        out.push(format!("    local rexon_net_vars = {{ {vars} }}"));
        out.push("    conky.text = rexon_each(conky.text, function(h)".into());
        out.push("        local lead, name, rest = h:match(\"^(%s*)([%w_]+)(.*)$\")".into());
        out.push("        if not name or not (rexon_net_vars[name] or name:match(\"^wireless_\")) then return h end".into());
        out.push("        return lead .. name .. rest:gsub(\"[^%s,]+\", function(w) return rexon_net[w] or w end)".into());
        out.push("    end)".into());
    }
    if let Some(h24) = clock {
        out.push("    conky.text = rexon_each(conky.text, function(h)".into());
        out.push("        local lead, name, rest = h:match(\"^(%s*)([%w_]+)(.*)$\")".into());
        out.push("        if name ~= \"time\" and name ~= \"utime\" and name ~= \"tztime\" then return h end".into());
        if h24 {
            out.push("        rest = rest:gsub(\"%%([Ilr])\", { I = \"%H\", l = \"%k\", r = \"%H:%M:%S\" })".into());
            out.push("        rest = rest:gsub(\"%s*%%[pP]\", \"\")".into());
        } else {
            out.push("        local n".into());
            out.push("        rest, n = rest:gsub(\"%%([HkRT])\", { H = \"%I\", k = \"%l\", R = \"%I:%M\", T = \"%I:%M:%S\" })".into());
            out.push("        if n > 0 and not rest:find(\"%%[pP]\") then rest = rest:gsub(\"%s*$\", \"\") .. \" %p\" end".into());
        }
        out.push("        return lead .. name .. rest".into());
        out.push("    end)".into());
    }
    out.push("end".into());
}

// ════════════════════════════════════════════════════════════════════════════
//  Хранение, запуск, применение
// ════════════════════════════════════════════════════════════════════════════

/// Выбор человека для файла `conf`; нет — пусто.
pub fn load(io: &dyn LookIo, p: &Paths) -> Overrides {
    io.read(&p.settings).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Что выполняет накладка и поля автора: `(файл для dofile, поля)`.
/// `None` — файла автора нет или его поля не прочитать.
///
/// Файл 1.9 переводится в копию ([`convert_19`]) при каждом вызове — файл
/// автора мог обновиться.
pub fn source_of(io: &dyn LookIo, conf: &str, p: &Paths) -> Option<(String, BTreeMap<String, Value>)> {
    let text = io.read(conf)?;
    match syntax(&text) {
        Syntax::V110 => Some((conf.to_string(), parse_config(&text)?)),
        Syntax::V19 => {
            let converted = convert_19(&text);
            let cfg = parse_config(&converted)?;
            io.write(&p.converted, &converted).then(|| (p.converted.clone(), cfg))
        }
    }
}

/// Записать выбор и накладку (пустой выбор — убрать обе). `Err` — причина.
pub fn save(io: &dyn LookIo, p: &Paths, conf: &str, ov: &Overrides, env: &Env) -> Result<(), String> {
    if ov.is_empty() {
        remove(p);
        return Ok(());
    }
    let Some((source, cfg)) = source_of(io, conf, p) else {
        return Err(tr("Rexon can't read this widget's settings file, so it can't change them."));
    };
    let json = serde_json::to_string_pretty(ov).unwrap_or_default();
    if !io.write(&p.settings, &json) || !io.write(&p.overlay, &render(&source, &cfg, ov, env)) {
        return Err(tr("Couldn't save the widget's settings."));
    }
    Ok(())
}

/// Убрать накладку, выбор и копию.
pub fn remove(p: &Paths) {
    for f in [&p.overlay, &p.settings, &p.converted] {
        let _ = std::fs::remove_file(f);
    }
}

/// Убрать накладки всех файлов под каталогом виджета `dir` (виджет
/// удаляют — его настройки больше не нужны).
pub fn forget_under(ctx: &Ctx, dir: &str) {
    let base = overlay_dir(ctx, std::env::var("XDG_STATE_HOME").ok().as_deref());
    let prefix = format!("{}/", dir.trim_end_matches('/'));
    let Ok(rd) = std::fs::read_dir(&base) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let k = [OVERLAY_SUFFIX, SETTINGS_SUFFIX, CONVERTED_SUFFIX].iter().find_map(|s| name.strip_suffix(s));
        if k.is_some_and(|k| conf_of_key(k).starts_with(&prefix)) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// С каким файлом запускать conky для `conf`: накладка, если человек что-то
/// менял (она пишется заново — файл автора мог обновиться), иначе сам
/// `conf`.
pub fn launch_conf(io: &dyn LookIo, ctx: &Ctx, p: &Paths, conf: &str) -> String {
    let ov = load(io, p);
    if ov.is_empty() {
        return conf.to_string();
    }
    let env = Env { version: conky_version(io), de: ctx.de.clone() };
    match save(io, p, conf, &ov, &env) {
        Ok(()) => p.overlay.clone(),
        Err(_) => conf.to_string(),
    }
}

/// Процессы conky файла `conf`: `(номер, запущен с накладкой)`.
pub fn processes_of(io: &dyn LookIo, conf: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for pid in io.list_dir("/proc") {
        if !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Some(cmd) = io.read(&format!("/proc/{pid}/cmdline")) else { continue };
        let Some(launch) = crate::de_apply::conky_launch_of_cmdline(&cmd) else { continue };
        match original_of_launch(&launch) {
            Some(orig) if orig == conf => out.push((pid, true)),
            None if launch == conf => out.push((pid, false)),
            _ => {}
        }
    }
    out
}

/// Перезапустить процессы `pids` файла `conf`: погасить, дождаться выхода
/// (иначе новый запуск принял бы старый процесс за свой) и запустить.
fn restart(io: &dyn LookIo, ctx: &Ctx, title: &str, conf: &str, pids: Vec<String>) -> (bool, String) {
    let (ok, msg) = crate::conky_procs::stop_pids(io, title, pids.clone());
    if !ok {
        return (false, msg);
    }
    for _ in 0..30 {
        if pids.iter().all(|p| io.read(&format!("/proc/{p}/cmdline")).is_none_or(|c| c.is_empty())) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    crate::conky_procs::start_conf(io, ctx, title, conf)
}

/// Переписать записи автозапуска, которые запускают `conf` (с накладкой
/// или без): после «Сбросить» вход не должен звать удалённую накладку, а
/// после первой правки — должен звать её. Задержка и окружение записи
/// сохраняются, строки композиторов переписываются вместе с ней.
pub fn refresh_autostart(io: &dyn LookIo, ctx: &Ctx, p: &Paths, title: &str, conf: &str, use_overlay: bool) {
    let dir = ctx.config("autostart");
    let quoted = crate::installer::desktop_exec_arg(conf);
    let quoted_overlay = crate::installer::desktop_exec_arg(&p.overlay);
    let env = crate::conky_autostart::session_env(ctx);
    for name in io.list_dir(&dir) {
        let path = format!("{dir}/{name}");
        let Some(key) = crate::conky_autostart::key_of_path(&path) else { continue };
        let Some(text) = io.read(&path) else { continue };
        let exec = text.lines().find_map(|l| l.strip_prefix("Exec=")).unwrap_or("");
        if !exec.ends_with(&format!(" {quoted}")) && !exec.contains(&quoted_overlay) {
            continue;
        }
        let launch = use_overlay.then_some(p.overlay.as_str());
        let opts = crate::conky_autostart::options_of(&text);
        let built = crate::conky_autostart::build(&env, key, title, conf, launch, &opts);
        io.write(&path, &built.desktop);
    }
}

/// С чем запускать `conf` при входе: накладка, если она есть.
pub fn autostart_launch(io: &dyn LookIo, p: &Paths) -> Option<String> {
    (!load(io, p).is_empty() && io.read(&p.overlay).is_some()).then(|| p.overlay.clone())
}

/// Применить выбор человека к файлу `conf` виджета `title`: записать
/// накладку; виджет, запущенный без неё, перезапустить с ней (запущенный с
/// ней conky перечитает накладку сам); автозапуск — на накладку.
/// `(получилось, сообщение)`.
pub fn apply(io: &dyn LookIo, ctx: &Ctx, p: &Paths, title: &str, conf: &str, ov: &Overrides) -> (bool, String) {
    let env = Env { version: conky_version(io), de: ctx.de.clone() };
    if let Err(why) = save(io, p, conf, ov, &env) {
        return (false, why);
    }
    let use_overlay = !ov.is_empty();
    refresh_autostart(io, ctx, p, title, conf, use_overlay);
    let procs = processes_of(io, conf);
    // Без накладки (сброс) перезапускаются те, что шли с ней; с накладкой —
    // те, что шли без неё.
    let stale: Vec<String> = procs.iter().filter(|(_, via)| *via != use_overlay).map(|(p, _)| p.clone()).collect();
    if !stale.is_empty() {
        let (ok, msg) = restart(io, ctx, title, conf, stale);
        if !ok {
            return (false, msg);
        }
    }
    if use_overlay {
        (true, trf("✅ “%1”: settings changed", &[&title]))
    } else {
        (true, trf("↩️ “%1”: back to the author's settings", &[&title]))
    }
}

/// «Сбросить»: как у автора.
pub fn reset(io: &dyn LookIo, ctx: &Ctx, p: &Paths, title: &str, conf: &str) -> (bool, String) {
    apply(io, ctx, p, title, conf, &Overrides::default())
}

// ════════════════════════════════════════════════════════════════════════════
//  Для карточки: что показать и как поменять
// ════════════════════════════════════════════════════════════════════════════

/// Клетка сетки выравнивания.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AlignCell {
    pub value: &'static str,
    pub arrow: &'static str,
    pub label: String,
}

/// Пункт выпадающего списка.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Choice {
    pub value: serde_json::Value,
    pub label: String,
}

/// Сетевой интерфейс текста виджета и чем его заменить.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct NetRow {
    pub from: String,
    pub to: String,
    pub label: String,
    pub choices: Vec<Choice>,
}

/// Подписи блока настроек.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SettingsTexts {
    pub title: String,
    pub reset: String,
    pub reset_tooltip: String,
    pub position: String,
    pub gap_x: String,
    pub gap_y: String,
    pub min_width: String,
    pub min_height: String,
    pub locked: String,
    pub background: String,
    pub bg_none: String,
    pub bg_opacity: String,
    pub bg_colour: String,
    pub window: String,
    pub window_preset: String,
    pub window_preset_tooltip: String,
    pub screen: String,
    pub update_interval: String,
    pub clock_24h: String,
    pub net_note: String,
}

pub fn settings_texts() -> SettingsTexts {
    SettingsTexts {
        title: tr("Widget settings"),
        reset: tr("↩️ As the author made it"),
        reset_tooltip: tr("Forget the changes made in Rexon. The widget's own files were never changed."),
        position: tr("Position and size"),
        gap_x: tr("Distance from the side edge"),
        gap_y: tr("Distance from the top or bottom"),
        min_width: tr("Minimum width"),
        min_height: tr("Minimum height"),
        locked: tr("The widget computes this value itself — Rexon can't show it, but you can set your own."),
        background: tr("Background"),
        bg_none: tr("No background"),
        bg_opacity: tr("Opacity"),
        bg_colour: tr("Colour"),
        window: tr("Window"),
        window_preset: tr("Keep the widget on the desktop"),
        window_preset_tooltip: tr(
            "Rexon picks the window settings that keep the widget under other windows and on every \
             workspace on this desktop.",
        ),
        screen: tr("Screen"),
        update_interval: tr("Update every, seconds"),
        clock_24h: tr("24-hour clock"),
        net_note: tr("Only the widget's text is changed; network rings drawn by its scripts keep their own setting."),
    }
}

fn align_cells() -> Vec<AlignCell> {
    let labels = [
        tr("Top left"),
        tr("Top centre"),
        tr("Top right"),
        tr("Middle left"),
        tr("Centre"),
        tr("Middle right"),
        tr("Bottom left"),
        tr("Bottom centre"),
        tr("Bottom right"),
    ];
    let arrows = ["↖", "↑", "↗", "←", "•", "→", "↙", "↓", "↘"];
    ALIGNMENTS.iter().zip(arrows).zip(labels).map(|((v, a), l)| AlignCell { value: v, arrow: a, label: l }).collect()
}

/// Блок настроек виджета для карточки.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SettingsView {
    pub conf: String,
    /// Настройки доступны (файл прочитан и разобран).
    pub available: bool,
    /// Почему нет.
    pub reason: String,
    pub syntax: Option<Syntax>,
    /// Человек что-то менял (есть накладка).
    pub changed: bool,
    pub cur: Option<Current>,
    pub alignments: Vec<AlignCell>,
    pub preset_note: Option<String>,
    pub compositor_note: Option<String>,
    pub monitors: Vec<Monitor>,
    pub screen_choices: Vec<Choice>,
    pub monitor_note: Option<String>,
    /// Часы в тексте: `Some(24 часа ли у автора)`; `None` — часов нет.
    pub clock: Option<bool>,
    pub clock_24h: bool,
    pub nets: Vec<NetRow>,
    pub t: SettingsTexts,
}

/// Что известно о машине для блока настроек.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Machine {
    pub monitors: Vec<Monitor>,
    pub interfaces: Vec<Iface>,
    /// На X11 работает композитор (`None` — не узнать или Wayland).
    pub compositor: Option<bool>,
}

/// Собрать блок настроек файла `conf`.
pub fn settings_view(io: &dyn LookIo, ctx: &Ctx, p: &Paths, conf: &str, m: &Machine) -> SettingsView {
    let ov = load(io, p);
    let mut v = SettingsView {
        conf: conf.to_string(),
        available: false,
        reason: String::new(),
        syntax: None,
        changed: !ov.is_empty(),
        cur: None,
        alignments: align_cells(),
        preset_note: preset_note(&ctx.de, ctx.wayland),
        compositor_note: None,
        monitors: m.monitors.clone(),
        screen_choices: Vec::new(),
        monitor_note: monitor_note(ctx.wayland),
        clock: None,
        clock_24h: false,
        nets: Vec::new(),
        t: settings_texts(),
    };
    let Some(text) = io.read(conf) else {
        v.reason = tr("Rexon can't read this widget's settings file, so it can't change them.");
        return v;
    };
    let (syntax, cfg) = author_config(&text);
    v.syntax = Some(syntax);
    let Some(cfg) = cfg else {
        v.reason = tr("This widget builds its settings with code, so Rexon can't change them.");
        return v;
    };
    let cur = current(&cfg, &ov);
    // Прозрачный фон без композитора X11 рисуется чёрным.
    let see_through = cur.bg_none.value || cur.bg_opacity.value < 100;
    if !ctx.wayland && see_through && m.compositor == Some(false) {
        v.compositor_note = Some(tr(
            "No compositor is running, so a see-through background will look black. Turn on compositing \
             in the window manager settings of this desktop.",
        ));
    }
    v.screen_choices.push(Choice { value: serde_json::json!(-1), label: tr("As the widget says") });
    for mon in &m.monitors {
        let mut label = format!("{} — {}×{}", mon.name, mon.width, mon.height);
        if mon.primary {
            label = trf("%1 (main)", &[&label]);
        }
        v.screen_choices.push(Choice { value: serde_json::json!(mon.index), label });
    }
    v.clock = clock_style(&text);
    v.clock_24h = ov.clock_24h.or(v.clock).unwrap_or(true);
    for from in net_names(&text) {
        let to = ov.net_ifaces.get(&from).cloned().unwrap_or_else(|| from.clone());
        let mut choices = vec![Choice { value: serde_json::json!(from), label: trf("%1 (as the widget says)", &[&from]) }];
        for i in &m.interfaces {
            if i.name == from {
                continue;
            }
            let kind = match i.kind {
                "wifi" => tr("Wi-Fi"),
                "ethernet" => tr("cable"),
                _ => tr("virtual"),
            };
            choices.push(Choice { value: serde_json::json!(i.name), label: format!("{} ({kind})", i.name) });
        }
        if !choices.iter().any(|c| c.value == serde_json::json!(to)) {
            choices.push(Choice { value: serde_json::json!(to), label: to.clone() });
        }
        v.nets.push(NetRow { label: trf("Network “%1”:", &[&from]), from, to, choices });
    }
    v.cur = Some(cur);
    v.available = true;
    v
}

/// Поменять одно поле выбора. `Err` — неизвестное поле или негодное значение.
pub fn set_value(ov: &mut Overrides, key: &str, value: &serde_json::Value) -> Result<(), String> {
    use serde_json::Value as J;
    let bad = || Err(tr("This value can't be used here."));
    let int = |v: &J| v.as_i64().filter(|n| (-10000..=10000).contains(n));
    if let Some(from) = key.strip_prefix("net:") {
        match value.as_str() {
            Some(to) if to == from || to.is_empty() => {
                ov.net_ifaces.remove(from);
            }
            Some(to) if to.chars().all(|c| c.is_ascii_alphanumeric() || "_.:-@".contains(c)) => {
                ov.net_ifaces.insert(from.to_string(), to.to_string());
            }
            None if value.is_null() => {
                ov.net_ifaces.remove(from);
            }
            _ => return bad(),
        }
        return Ok(());
    }
    let null = value.is_null();
    match key {
        "alignment" => match value.as_str().and_then(normalize_alignment) {
            Some(a) => ov.alignment = Some(a.to_string()),
            None if null => ov.alignment = None,
            None => return bad(),
        },
        "gap_x" | "gap_y" | "min_width" | "min_height" => {
            let v = if null { None } else { Some(int(value).ok_or_else(|| tr("This value can't be used here."))?) };
            let v = if key.starts_with("min_") { v.map(|n| n.max(0)) } else { v };
            match key {
                "gap_x" => ov.gap_x = v,
                "gap_y" => ov.gap_y = v,
                "min_width" => ov.min_width = v,
                _ => ov.min_height = v,
            }
        }
        "bg_none" => ov.bg_none = if null { None } else { Some(value.as_bool().ok_or_else(|| tr("This value can't be used here."))?) },
        "bg_opacity" => {
            ov.bg_opacity = match value.as_u64() {
                Some(n) => Some(n.min(100) as u8),
                None if null => None,
                None => return bad(),
            };
            // Ползунок двигают, чтобы фон был: «без фона» уходит.
            if ov.bg_opacity.is_some_and(|o| o > 0) && ov.bg_none == Some(true) {
                ov.bg_none = Some(false);
            }
        }
        "bg_colour" => match value.as_str().and_then(parse_colour) {
            Some((hex, _)) => ov.bg_colour = Some(hex),
            None if null => ov.bg_colour = None,
            None => return bad(),
        },
        "window_preset" => ov.window_preset = value.as_bool().filter(|b| *b),
        "update_interval" => match value.as_f64() {
            Some(n) if n > 0.0 && n <= 3600.0 => ov.update_interval = Some(n),
            None if null => ov.update_interval = None,
            _ => return bad(),
        },
        "xinerama_head" => match value.as_i64() {
            Some(n) if n >= 0 => ov.xinerama_head = Some(n as u32),
            Some(_) => ov.xinerama_head = None,
            None if null => ov.xinerama_head = None,
            None => return bad(),
        },
        "clock_24h" => ov.clock_24h = if null { None } else { value.as_bool() },
        _ => return bad(),
    }
    Ok(())
}

/// Принадлежит ли файл `conf` виджету `name` (настройки чужого файла не
/// пишутся).
fn part_of(io: &dyn LookIo, ctx: &Ctx, name: &str, conf: &str) -> bool {
    crate::conky_procs::parts(io, ctx, name).iter().any(|p| p.conf == conf)
}

/// Поменять поле `key` файла `conf` виджета `name` и применить.
pub fn set_field(io: &dyn LookIo, ctx: &Ctx, p: &Paths, name: &str, conf: &str, key: &str, value: &serde_json::Value) -> (bool, String) {
    if !part_of(io, ctx, name, conf) {
        return (false, tr("This file isn't part of the widget."));
    }
    let mut ov = load(io, p);
    if let Err(why) = set_value(&mut ov, key, value) {
        return (false, why);
    }
    apply(io, ctx, p, &crate::conky_scan::widget_title(name), conf, &ov)
}

/// Композитор X11 работает: у экрана есть владелец выделения `_NET_WM_CM_S<n>`
/// (так композиторы X11 заявляют о себе). `None` — X11 не открыть.
pub fn live_x11_compositor() -> Option<bool> {
    use x11_dl::xlib;
    let lib = xlib::Xlib::open().ok()?;
    // SAFETY: дисплей открывается и закрывается здесь же, указатели не
    // уходят из функции.
    unsafe {
        let dpy = (lib.XOpenDisplay)(std::ptr::null());
        if dpy.is_null() {
            return None;
        }
        let screen = (lib.XDefaultScreen)(dpy);
        let name = std::ffi::CString::new(format!("_NET_WM_CM_S{screen}")).ok()?;
        let atom = (lib.XInternAtom)(dpy, name.as_ptr(), 0);
        let owner = (lib.XGetSelectionOwner)(dpy, atom);
        (lib.XCloseDisplay)(dpy);
        Some(owner != 0)
    }
}

/// Живой блок настроек.
pub fn live_settings_view(name: &str, conf: &str) -> SettingsView {
    let io = crate::animations::SystemIo;
    let ctx = Ctx::live();
    let p = live_paths(&ctx, conf);
    let m = Machine {
        monitors: monitors(&io, ctx.wayland),
        interfaces: interfaces(&io),
        compositor: if ctx.wayland { None } else { live_x11_compositor() },
    };
    if !part_of(&io, &ctx, name, conf) {
        let mut v = settings_view(&io, &ctx, &p, conf, &m);
        v.available = false;
        v.reason = tr("This file isn't part of the widget.");
        return v;
    }
    settings_view(&io, &ctx, &p, conf, &m)
}

pub fn live_set(name: &str, conf: &str, key: &str, value: &serde_json::Value) -> (bool, String) {
    let ctx = Ctx::live();
    let done = set_field(&crate::animations::SystemIo, &ctx, &live_paths(&ctx, conf), name, conf, key, value);
    // Снимок плитки — с новыми настройками (`conky_preview`).
    crate::conky_preview::want(&ctx, name);
    done
}

pub fn live_reset(name: &str, conf: &str) -> (bool, String) {
    let io = crate::animations::SystemIo;
    let ctx = Ctx::live();
    if !part_of(&io, &ctx, name, conf) {
        return (false, tr("This file isn't part of the widget."));
    }
    let done = reset(&io, &ctx, &live_paths(&ctx, conf), &crate::conky_scan::widget_title(name), conf);
    crate::conky_preview::want(&ctx, name);
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for conf in ["/h/.conky/A B/conky.conf", "/x/100%/c", "/a+s/b+", "/a++p/c"] {
            assert_eq!(conf_of_key(&key(conf)), conf);
        }
        let c = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        let p = paths(&c, None, "/h/.conky/A/conky.conf");
        assert_eq!(p.overlay, "/h/.local/state/rexon/conky/overlay/h+s.conky+sA+sconky.conf.rexon.conf");
        assert_eq!(original_of_launch(&p.overlay).as_deref(), Some("/h/.conky/A/conky.conf"));
        assert_eq!(original_of_launch("/h/.conky/A/conky.conf"), None);
        assert_eq!(original_of_launch(&p.converted), None);
    }

    #[test]
    fn literals_and_expressions() {
        let cfg = parse_config(
            "-- шапка\nlocal s = 2\nconky.config = {\n  alignment = 'tr', gap_x = 10 * s; gap_y=-5,\n  \
             own_window_colour = \"#80112233\", -- цвет\n  font = [[DejaVu:size=9]],\n  draw = true,\n  \
             f = function() return 1 end,\n  t = { 1, 2 },\n  [\"minimum_width\"] = 0x10,\n}\nconky.text = [[x]]\n",
        )
        .unwrap();
        assert_eq!(cfg["alignment"], Value::Str("tr".into()));
        assert_eq!(cfg["gap_x"], Value::Expr);
        assert_eq!(cfg["gap_y"], Value::Num(-5.0));
        assert_eq!(cfg["font"], Value::Str("DejaVu:size=9".into()));
        assert_eq!(cfg["draw"], Value::Bool(true));
        assert_eq!(cfg["f"], Value::Expr);
        assert_eq!(cfg["t"], Value::Expr);
        assert_eq!(cfg["minimum_width"], Value::Num(16.0));
        assert_eq!(parse_config("TEXT\n${time}\n"), None);
    }

    #[test]
    fn colours() {
        assert_eq!(parse_colour("#80112233"), Some(("112233".into(), Some(0x80))));
        assert_eq!(parse_colour("#8000"), Some(("000000".into(), Some(0x88))));
        assert_eq!(parse_colour("abc"), Some(("AABBCC".into(), None)));
        assert_eq!(parse_colour("black"), Some(("000000".into(), None)));
        assert_eq!(parse_colour("nope"), None);
    }

    #[test]
    fn monitors_from_xrandr() {
        let out = "Monitors: 2\n 0: +*eDP-1 1920/344x1080/193+0+0  eDP-1\n 1: +HDMI-A-1 2560/597x1440/336+1920+-200  HDMI-A-1\n";
        let m = parse_monitors(out);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0], Monitor { index: 0, name: "eDP-1".into(), width: 1920, height: 1080, x: 0, y: 0, primary: true });
        assert_eq!((m[1].name.as_str(), m[1].x, m[1].y, m[1].primary), ("HDMI-A-1", 1920, -200, false));
        assert!(parse_monitors("garbage\n").is_empty());
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("conky 1.25.2-pre-6caddca3 compiled for Linux x86_64\n"), Some((1, 25)));
        assert_eq!(parse_version("conky 1.9.0 compiled Tue\n"), Some((1, 9)));
        assert_eq!(parse_version("garbage"), None);
    }
}
