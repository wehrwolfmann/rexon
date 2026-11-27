//! Палитра цветов виджета Conky: все цвета, найденные в файлах виджета,
//! одним списком — с заменой во всех местах сразу и возвратом.
//!
//! ЗАЧЕМ. Цвета у виджетов задаются тремя разными способами, и человеку
//! незачем знать, каким: именованные цвета настроек (`default_color`,
//! `color0`…`color9`), цвета прямо в тексте (`${color #ff8800}`, цвета
//! графиков `${cpugraph 30,200 000000 ff0000}`) и цвета сценариев Lua (кольца,
//! стрелки часов: `fg_colour = 0xFF8800`, `cairo_set_source_rgba(cr, 1, 0.53,
//! 0, 1)`, таблицы долей `{0.2, 0.2, 0.2, 0.8}`). Решение Wehrwolfmann
//! 14.11.2026: один способ для всех видов — палитра найденных цветов.
//! Одинаковые цвета собраны в один кружок («#ff8800 — 12 мест»).
//!
//! КАК МЕНЯЕТСЯ. В отличие от накладки ([`crate::conky_overlay`]) палитра
//! правит САМИ файлы виджета: цвет в сценарии Lua накладкой не достать.
//! Поэтому:
//! * каждое место хранится точно — файл и байтовые промежутки; заменяются
//!   только они, остальное — байт в байт (переводы строк, отступы, регистр,
//!   комментарии);
//! * место пишется в своём виде: hex остаётся hex (с `#` или без, в том же
//!   регистре), `0x…` — `0x…`, доли Lua — долями (точность — как у автора,
//!   но не грубее трёх знаков: иначе выбранный цвет не возвращался бы из
//!   долей тем же и кружок распадался бы на два); имя цвета X11 (`white`)
//!   становится hex без `#` — его conky понимает везде;
//! * перед ПЕРВОЙ правкой файла его копия ложится в
//!   `~/.local/share/rexon/conky-colour-originals/<ключ виджета>/` (рядом
//!   `origin.json` — откуда какой файл), как копии описаний обоев Axon
//!   (`axon_prepare`). «Вернуть цвета» кладёт копии на место и убирает их;
//! * файл, который не принадлежит человеку, лежит вне его дома или
//!   только для чтения, не меняется вовсе — честный отказ ДО первой записи
//!   (все или ничего).
//!
//! ЧТО НЕ ТРОГАЕТСЯ. Цвета-выражения (`r * 0.5`, цвет по значению,
//! `${color ${exec …}}`), неизвестные имена цветов и цвета в картинках.
//! О них палитра говорит одной строкой: менять их — через «Открыть файл
//! настроек». Поле `own_window_colour` — фон окна — меняется в блоке «Фон»
//! настроек (накладкой), в палитру не входит.
//!
//! После правки: виджет, разрешённый человеком в прежнем виде
//! ([`crate::conky_safety::trusted_as_is`]), разрешается заново — правка
//! цвета команд не добавляет; накладки файлов переписываются (копия 1.9 →
//! 1.10 берётся заново), запущенные процессы получают `SIGUSR1` (conky
//! перечитывает настройки и сценарии), снимок для плитки переснимается
//! ([`crate::conky_preview::want`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::de_apply::Ctx;
use crate::i18n::{ntr, tr, trf};

/// Подкаталог данных Rexon с копиями файлов до первой смены цвета.
pub const BACKUP_SUBDIR: &str = "rexon/conky-colour-originals";
/// Опись копий в каталоге виджета.
pub const ORIGIN_FILE: &str = "origin.json";
/// Файлы больше этого не разбираются (не код виджета).
const MAX_FILE: usize = 2 * 1024 * 1024;

// ════════════════════════════════════════════════════════════════════════════
//  Что найдено
// ════════════════════════════════════════════════════════════════════════════

/// Где задан цвет.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Именованный цвет настроек (`default_color`, `color1`…).
    Setting,
    /// Цвет в тексте виджета (`${color …}`).
    Text,
    /// Цвет графика (`${cpugraph … 000000 ff0000}`).
    Graph,
    /// Цвет в сценарии Lua.
    Script,
}

/// В каком виде цвет записан — в нём же пишется новый.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    /// `ff8800`, `#FF8800`, `#f80`.
    Hex { hash: bool, upper: bool },
    /// Имя цвета X11 (`white`, `Tan1`).
    Name,
    /// Число Lua `0xFF8800`.
    Lua { upper: bool },
    /// Три доли 0..1; `decimals` — сколько знаков писать.
    Fractions { decimals: usize },
}

/// Одно место цвета.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub file: String,
    /// Байтовые промежутки `[начало, конец)`: один у hex и имени, три у
    /// долей (красный, зелёный, синий).
    pub spans: Vec<(usize, usize)>,
    form: Form,
    pub kind: Kind,
    pub rgb: [u8; 3],
}

/// Всё найденное в файлах виджета.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub places: Vec<Place>,
    /// Мест, которые Rexon менять не берётся: выражения, неизвестные имена.
    pub computed: usize,
    /// У виджета есть картинки (`${image}`, PNG в сценариях).
    pub pictures: bool,
}

/// Как читать файл.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Настройки 1.10 (`conky.config = {…}`, `conky.text = [[…]]`).
    Config,
    /// Настройки до 1.10 (`имя значение`, `TEXT`).
    OldConfig,
    /// Сценарий Lua (`lua_load`).
    Script,
}

/// Вид файла по имени и содержимому.
pub fn role_of(path: &str, text: &str) -> Role {
    if crate::conky_scan::looks_like_config(text) {
        return match crate::conky_overlay::syntax(text) {
            crate::conky_overlay::Syntax::V19 => Role::OldConfig,
            crate::conky_overlay::Syntax::V110 => Role::Config,
        };
    }
    if path.to_ascii_lowercase().ends_with(".lua") {
        Role::Script
    } else {
        Role::Config
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Значения цветов
// ════════════════════════════════════════════════════════════════════════════

/// Имена цветов, которые понимает conky (база X11). Где X11 расходится с
/// CSS (`gray`, `green`, `maroon`, `purple`), взято значение X11 — conky
/// читает имена по ней. Номерных вариантов X11 (`Tan1`, `gray50`) здесь
/// нет, кроме серых (считаются) и самых частых в виджетах.
const NAMES: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff), ("antiquewhite", 0xfaebd7), ("aqua", 0x00ffff), ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff), ("beige", 0xf5f5dc), ("bisque", 0xffe4c4), ("black", 0x000000),
    ("blanchedalmond", 0xffebcd), ("blue", 0x0000ff), ("blueviolet", 0x8a2be2), ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887), ("cadetblue", 0x5f9ea0), ("chartreuse", 0x7fff00), ("chocolate", 0xd2691e),
    ("coral", 0xff7f50), ("cornflowerblue", 0x6495ed), ("cornsilk", 0xfff8dc), ("crimson", 0xdc143c),
    ("cyan", 0x00ffff), ("darkblue", 0x00008b), ("darkcyan", 0x008b8b), ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9), ("darkgreen", 0x006400), ("darkgrey", 0xa9a9a9), ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b), ("darkolivegreen", 0x556b2f), ("darkorange", 0xff8c00), ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000), ("darksalmon", 0xe9967a), ("darkseagreen", 0x8fbc8f), ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f), ("darkslategrey", 0x2f4f4f), ("darkturquoise", 0x00ced1), ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493), ("deepskyblue", 0x00bfff), ("dimgray", 0x696969), ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff), ("firebrick", 0xb22222), ("floralwhite", 0xfffaf0), ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff), ("gainsboro", 0xdcdcdc), ("ghostwhite", 0xf8f8ff), ("gold", 0xffd700),
    ("goldenrod", 0xdaa520), ("gray", 0xbebebe), ("green", 0x00ff00), ("greenyellow", 0xadff2f),
    ("grey", 0xbebebe), ("honeydew", 0xf0fff0), ("hotpink", 0xff69b4), ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082), ("ivory", 0xfffff0), ("khaki", 0xf0e68c), ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5), ("lawngreen", 0x7cfc00), ("lemonchiffon", 0xfffacd), ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080), ("lightcyan", 0xe0ffff), ("lightgoldenrod", 0xeedd82), ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3), ("lightgreen", 0x90ee90), ("lightgrey", 0xd3d3d3), ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a), ("lightseagreen", 0x20b2aa), ("lightskyblue", 0x87cefa), ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899), ("lightsteelblue", 0xb0c4de), ("lightyellow", 0xffffe0), ("lime", 0x00ff00),
    ("limegreen", 0x32cd32), ("linen", 0xfaf0e6), ("magenta", 0xff00ff), ("maroon", 0xb03060),
    ("mediumaquamarine", 0x66cdaa), ("mediumblue", 0x0000cd), ("mediumorchid", 0xba55d3), ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371), ("mediumslateblue", 0x7b68ee), ("mediumspringgreen", 0x00fa9a), ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585), ("midnightblue", 0x191970), ("mintcream", 0xf5fffa), ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5), ("navajowhite", 0xffdead), ("navy", 0x000080), ("navyblue", 0x000080),
    ("oldlace", 0xfdf5e6), ("olive", 0x808000), ("olivedrab", 0x6b8e23), ("orange", 0xffa500),
    ("orangered", 0xff4500), ("orchid", 0xda70d6), ("palegoldenrod", 0xeee8aa), ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee), ("palevioletred", 0xdb7093), ("papayawhip", 0xffefd5), ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f), ("pink", 0xffc0cb), ("plum", 0xdda0dd), ("powderblue", 0xb0e0e6),
    ("purple", 0xa020f0), ("rebeccapurple", 0x663399), ("red", 0xff0000), ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1), ("saddlebrown", 0x8b4513), ("salmon", 0xfa8072), ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57), ("seashell", 0xfff5ee), ("sienna", 0xa0522d), ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb), ("slateblue", 0x6a5acd), ("slategray", 0x708090), ("slategrey", 0x708090),
    ("snow", 0xfffafa), ("springgreen", 0x00ff7f), ("steelblue", 0x4682b4), ("tan", 0xd2b48c),
    ("tan1", 0xffa54f), ("teal", 0x008080), ("thistle", 0xd8bfd8), ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0), ("violet", 0xee82ee), ("violetred", 0xd02090), ("wheat", 0xf5deb3),
    ("white", 0xffffff), ("whitesmoke", 0xf5f5f5), ("yellow", 0xffff00), ("yellowgreen", 0x9acd32),
];

fn rgb_of(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// Цвет по имени X11 (без учёта регистра); `gray0`…`gray100` считаются.
pub fn named(name: &str) -> Option<[u8; 3]> {
    let n = name.to_ascii_lowercase();
    if let Some(&(_, v)) = NAMES.iter().find(|(k, _)| *k == n) {
        return Some(rgb_of(v));
    }
    let num = n.strip_prefix("gray").or_else(|| n.strip_prefix("grey"))?;
    let k: u32 = num.parse().ok().filter(|k| *k <= 100)?;
    let c = ((k as f64) * 2.55).round() as u8;
    Some([c, c, c])
}

fn is_hex(b: &[u8]) -> bool {
    !b.is_empty() && b.iter().all(u8::is_ascii_hexdigit)
}

fn hex_rgb(b: &[u8]) -> Option<[u8; 3]> {
    let s = std::str::from_utf8(b).ok()?;
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(rgb_of(v))
}

fn has_upper(b: &[u8]) -> bool {
    b.iter().any(u8::is_ascii_uppercase)
}

/// Значение цвета настроек или текста: hex (6 или 3 знака, с `#` или без)
/// или имя X11. `None` — не цвет либо имя неизвестно.
fn parse_value(s: &[u8]) -> Option<([u8; 3], Form)> {
    let (hash, body) = match s.first() {
        Some(b'#') => (true, &s[1..]),
        _ => (false, s),
    };
    if body.len() == 6 && is_hex(body) {
        return Some((hex_rgb(body)?, Form::Hex { hash, upper: has_upper(body) }));
    }
    if body.len() == 3 && is_hex(body) {
        let long: Vec<u8> = body.iter().flat_map(|&c| [c, c]).collect();
        return Some((hex_rgb(&long)?, Form::Hex { hash, upper: has_upper(body) }));
    }
    if !hash && body.first().is_some_and(u8::is_ascii_alphabetic) && body.iter().all(u8::is_ascii_alphanumeric) {
        let name = std::str::from_utf8(body).ok()?;
        return named(name).map(|rgb| (rgb, Form::Name));
    }
    None
}

/// Похоже ли значение на попытку задать цвет (чтобы неизвестное имя
/// попало в «вычисляется», а не пропало молча).
fn looks_like_colour(s: &[u8]) -> bool {
    !s.is_empty() && s.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'#')
}

/// Строка цвета `rgb` в виде места (`i` — номер промежутка у долей).
fn render(form: Form, rgb: [u8; 3], i: usize) -> String {
    let hex = format!("{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
    match form {
        Form::Hex { hash, upper } => {
            let body = if upper { hex.to_ascii_uppercase() } else { hex };
            if hash {
                format!("#{body}")
            } else {
                body
            }
        }
        Form::Name => hex,
        Form::Lua { upper } => format!("0x{}", if upper { hex.to_ascii_uppercase() } else { hex }),
        Form::Fractions { decimals } => format!("{:.*}", decimals, f64::from(rgb[i.min(2)]) / 255.0),
    }
}

/// `#rrggbb` для показа.
pub fn hex_label(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// `#rrggbb` / `rrggbb` → цвет.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let b = s.trim().trim_start_matches('#').as_bytes();
    (b.len() == 6 && is_hex(b)).then(|| hex_rgb(b)).flatten()
}

// ════════════════════════════════════════════════════════════════════════════
//  Разбор Lua: код, строки, комментарии
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seg {
    /// Код `[начало, конец)`.
    Code(usize, usize),
    /// Содержимое строки (без кавычек и скобок) `[начало, конец)`.
    Str(usize, usize),
}

/// Длинная скобка `[==[` в позиции `i`: `(уровень, длина открытия)`.
fn long_open(b: &[u8], i: usize) -> Option<(usize, usize)> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') {
        j += 1;
    }
    (b.get(j) == Some(&b'[')).then_some((j - i - 1, j - i + 1))
}

/// Начало закрытия `]==]` уровня `level` от позиции `from`.
fn long_close(b: &[u8], from: usize, level: usize) -> Option<usize> {
    let mut i = from;
    while i < b.len() {
        if b[i] == b']' {
            let mut j = i + 1;
            while j < b.len() && b[j] == b'=' {
                j += 1;
            }
            if j - i - 1 == level && b.get(j) == Some(&b']') {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Текст Lua — кусками кода и строк; комментарии выброшены.
fn lua_segments(b: &[u8]) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut code = 0;
    let mut i = 0;
    let n = b.len();
    let flush = |out: &mut Vec<Seg>, from: usize, to: usize| {
        if to > from {
            out.push(Seg::Code(from, to));
        }
    };
    while i < n {
        let c = b[i];
        if c == b'-' && b.get(i + 1) == Some(&b'-') {
            flush(&mut out, code, i);
            i += 2;
            if let Some((level, len)) = long_open(b, i) {
                i = long_close(b, i + len, level).map_or(n, |e| e + level + 2);
            } else {
                while i < n && b[i] != b'\n' {
                    i += 1;
                }
            }
            code = i;
            continue;
        }
        if c == b'\'' || c == b'"' {
            flush(&mut out, code, i);
            let mut j = i + 1;
            while j < n && b[j] != c && b[j] != b'\n' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let end = j.min(n);
            out.push(Seg::Str(i + 1, end));
            i = (end + 1).min(n);
            code = i;
            continue;
        }
        if c == b'[' {
            if let Some((level, len)) = long_open(b, i) {
                flush(&mut out, code, i);
                let start = i + len;
                let end = long_close(b, start, level).unwrap_or(n);
                out.push(Seg::Str(start, end));
                i = (end + level + 2).min(n);
                code = i;
                continue;
            }
        }
        i += 1;
    }
    flush(&mut out, code, n);
    out
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Позиции слова `word` целиком в `[s, e)`.
fn words(b: &[u8], s: usize, e: usize, word: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = s;
    while i + word.len() <= e {
        if &b[i..i + word.len()] == word
            && (i == 0 || !is_ident(b[i - 1]))
            && b.get(i + word.len()).is_none_or(|c| !is_ident(*c))
        {
            out.push(i);
            i += word.len();
        } else {
            i += 1;
        }
    }
    out
}

fn skip_ws(b: &[u8], mut i: usize, e: usize) -> usize {
    while i < e && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn trim_span(b: &[u8], mut s: usize, mut e: usize) -> (usize, usize) {
    while s < e && b[s].is_ascii_whitespace() {
        s += 1;
    }
    while e > s && b[e - 1].is_ascii_whitespace() {
        e -= 1;
    }
    (s, e)
}

// ════════════════════════════════════════════════════════════════════════════
//  Поиск мест
// ════════════════════════════════════════════════════════════════════════════

/// Имя поля-цвета настроек (оба написания, `colour` и `color`); фон окна
/// (`own_window_colour`) — не здесь, он у блока «Фон».
fn is_setting(name: &str) -> bool {
    let n = name.to_ascii_lowercase().replace("colour", "color");
    matches!(n.as_str(), "default_color" | "default_outline_color" | "default_shade_color")
        || n.strip_prefix("color").is_some_and(|d| d.len() == 1 && d.as_bytes()[0].is_ascii_digit())
}

struct Scanner<'a> {
    b: &'a [u8],
    file: &'a str,
    found: &'a mut Found,
}

impl Scanner<'_> {
    fn push(&mut self, spans: Vec<(usize, usize)>, form: Form, kind: Kind, rgb: [u8; 3]) {
        self.found.places.push(Place { file: self.file.to_string(), spans, form, kind, rgb });
    }

    /// Значение настройки/текста в `[s, e)`: цвет — место, неизвестное
    /// имя — «вычисляется».
    fn value(&mut self, s: usize, e: usize, kind: Kind) {
        let (s, e) = trim_span(self.b, s, e);
        match parse_value(&self.b[s..e]) {
            Some((rgb, form)) => self.push(vec![(s, e)], form, kind, rgb),
            None if looks_like_colour(&self.b[s..e]) => self.found.computed += 1,
            None => {}
        }
    }

    /// Текст виджета `[s, e)`: `${color …}`, цвета графиков, картинки.
    fn text(&mut self, s: usize, e: usize) {
        let b = self.b;
        let mut i = s;
        while i + 1 < e {
            if b[i] != b'$' {
                i += 1;
                continue;
            }
            if b[i + 1] == b'$' {
                i += 2;
                continue;
            }
            if b[i + 1] != b'{' {
                i += 1;
                continue;
            }
            let name_start = i + 2;
            let mut j = name_start;
            while j < e && is_ident(b[j]) {
                j += 1;
            }
            // Закрывающая скобка с учётом вложенных `${…}`.
            let mut depth = 1usize;
            let mut k = i + 2;
            while k < e {
                match b[k] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            let name = String::from_utf8_lossy(&b[name_start..j]).to_ascii_lowercase();
            let (args_s, args_e) = (j, k.min(e));
            match name.as_str() {
                "color" | "colour" | "outlinecolor" | "shadecolor" => {
                    let (a, z) = trim_span(b, args_s, args_e);
                    if a < z {
                        if b[a..z].contains(&b'$') {
                            self.found.computed += 1;
                        } else {
                            self.value(a, z, Kind::Text);
                        }
                    }
                }
                "image" => self.found.pictures = true,
                n if n.ends_with("graph") || n.ends_with("graph_read") || n.ends_with("graph_write") => {
                    self.graph(args_s, args_e);
                }
                _ => {}
            }
            i = j.max(i + 2);
        }
    }

    /// Цвета графика: пара hex сразу после размера `высота,ширина`, а без
    /// размера — первая пара hex подряд.
    fn graph(&mut self, s: usize, e: usize) {
        let b = self.b;
        let mut toks: Vec<(usize, usize)> = Vec::new();
        let mut i = s;
        while i < e {
            i = skip_ws(b, i, e);
            if i >= e {
                break;
            }
            let start = i;
            if b[i] == b'"' {
                i += 1;
                while i < e && b[i] != b'"' {
                    i += 1;
                }
                i = (i + 1).min(e);
            } else {
                while i < e && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
            }
            if b[start] != b'-' {
                toks.push((start, i));
            }
        }
        let hex6 = |&(a, z): &(usize, usize)| {
            let t = &b[a..z];
            let t = t.strip_prefix(b"#").unwrap_or(t);
            t.len() == 6 && is_hex(t)
        };
        let size = toks.iter().position(|&(a, z)| {
            let t = &b[a..z];
            t.iter().filter(|c| **c == b',').count() == 1 && t.iter().all(|c| c.is_ascii_digit() || *c == b',')
        });
        let at = match size {
            Some(p) => (toks.len() > p + 2 && hex6(&toks[p + 1]) && hex6(&toks[p + 2])).then_some(p + 1),
            None => (0..toks.len().saturating_sub(1)).find(|&p| hex6(&toks[p]) && hex6(&toks[p + 1])),
        };
        if let Some(p) = at {
            for &(a, z) in &toks[p..p + 2] {
                if let Some((rgb, form)) = parse_value(&b[a..z]) {
                    self.push(vec![(a, z)], form, Kind::Graph, rgb);
                }
            }
        }
    }

    /// Файл настроек 1.10.
    fn config(&mut self) {
        let b = self.b;
        let segs = lua_segments(b);
        for (n, seg) in segs.iter().enumerate() {
            match *seg {
                Seg::Str(s, e) => self.text(s, e),
                Seg::Code(s, e) => {
                    let mut i = s;
                    while i < e {
                        if !is_ident(b[i]) || (i > s && is_ident(b[i - 1])) {
                            i += 1;
                            continue;
                        }
                        let mut j = i;
                        while j < e && is_ident(b[j]) {
                            j += 1;
                        }
                        let name = String::from_utf8_lossy(&b[i..j]).into_owned();
                        if is_setting(&name) {
                            let at = skip_ws(b, j, e);
                            if at < e && b[at] == b'=' && b.get(at + 1) != Some(&b'=') && skip_ws(b, at + 1, e) == e {
                                // Значение — строка сразу за `=`.
                                if let Some(Seg::Str(vs, ve)) = segs.get(n + 1) {
                                    self.value(*vs, *ve, Kind::Setting);
                                }
                            }
                        }
                        i = j;
                    }
                }
            }
        }
    }

    /// Файл настроек до 1.10.
    fn old_config(&mut self) {
        let b = self.b;
        let mut i = 0;
        while i < b.len() {
            let end = b[i..].iter().position(|c| *c == b'\n').map_or(b.len(), |p| i + p);
            let (ls, le) = trim_span(b, i, end);
            if &b[ls..le] == b"TEXT" {
                self.text(end, b.len());
                return;
            }
            // `#` — комментарий до конца строки (`\#` — знак).
            let mut cut = le;
            let mut k = ls;
            while k < le {
                if b[k] == b'#' && (k == 0 || b[k - 1] != b'\\') {
                    cut = k;
                    break;
                }
                k += 1;
            }
            let mut j = ls;
            while j < cut && !b[j].is_ascii_whitespace() {
                j += 1;
            }
            let name = String::from_utf8_lossy(&b[ls..j]).into_owned();
            if is_setting(&name) {
                let vs = skip_ws(b, j, cut);
                let mut ve = vs;
                while ve < cut && !b[ve].is_ascii_whitespace() {
                    ve += 1;
                }
                if vs < ve {
                    self.value(vs, ve, Kind::Setting);
                }
            }
            i = end + 1;
        }
    }

    /// Сценарий Lua.
    fn script(&mut self) {
        let b = self.b;
        for seg in lua_segments(b) {
            match seg {
                Seg::Str(s, e) => self.text(s, e),
                Seg::Code(s, e) => {
                    let code = &b[s..e];
                    if contains(code, b"cairo_image_surface_create_from_png") || contains(code, b"imlib_load_image") {
                        self.found.pictures = true;
                    }
                    self.lua_hex(s, e);
                    for (call, skip) in CAIRO_CALLS {
                        for p in words(b, s, e, call.as_bytes()) {
                            self.cairo(p + call.len(), e, *skip);
                        }
                    }
                    self.tables(s, e);
                }
            }
        }
    }

    /// `0xRRGGBB` там, где он похож на цвет: значение поля, элемент таблицы
    /// или довод вызова — но не маска битовой операции и не слагаемое.
    fn lua_hex(&mut self, s: usize, e: usize) {
        let b = self.b;
        let mut i = s;
        while i + 8 <= e {
            if !(b[i] == b'0' && (b[i + 1] == b'x' || b[i + 1] == b'X')) || (i > 0 && is_ident(b[i - 1])) {
                i += 1;
                continue;
            }
            let digits = &b[i + 2..i + 8];
            let after = b.get(i + 8).copied();
            if !is_hex(digits) || after.is_some_and(|c| is_ident(c) || c == b'.') {
                i += 2;
                continue;
            }
            let mut p = i;
            while p > s && b[p - 1].is_ascii_whitespace() && b[p - 1] != b'\n' {
                p -= 1;
            }
            let prev = (p > s).then(|| b[p - 1]);
            let prev2 = (p > s + 1).then(|| b[p - 2]);
            let q = skip_ws(b, i + 8, e);
            let next = (q < e).then(|| b[q]);
            let after_ws = i + 8 < e && b[i + 8].is_ascii_whitespace();
            let prev_ok = match prev {
                Some(b'=') => !matches!(prev2, Some(b'=' | b'~' | b'<' | b'>')),
                Some(b',' | b'{' | b'(') => true,
                _ => false,
            };
            let next_ok = match next {
                None | Some(b',' | b'}' | b')' | b';') => true,
                Some(b'-') => b.get(q + 1) == Some(&b'-'),
                _ => after_ws && next.is_some_and(|c| c.is_ascii_alphabetic()) && starts_new_statement(b, q, e),
            } || (q > i + 8 && b[i + 8..q].contains(&b'\n'));
            let line_start = b[s..i].iter().rposition(|c| *c == b'\n').map_or(s, |x| s + x + 1);
            let line = &b[line_start..i];
            let bitop = ["band(", "bor(", "bxor(", "rshift(", "lshift(", "bit32."].iter().any(|w| contains(line, w.as_bytes()));
            if prev_ok && next_ok && !bitop {
                if let Some(rgb) = hex_rgb(digits) {
                    self.push(vec![(i, i + 8)], Form::Lua { upper: has_upper(digits) }, Kind::Script, rgb);
                }
            }
            i += 8;
        }
    }

    /// Вызов cairo с цветом: `(` сразу за именем, `skip` первых доводов —
    /// не цвет (поверхность, смещение остановки).
    fn cairo(&mut self, from: usize, e: usize, skip: usize) {
        let b = self.b;
        let open = skip_ws(b, from, e);
        if open >= e || b[open] != b'(' {
            return;
        }
        let Some(args) = call_args(b, open, e) else { return };
        let colour = args.get(skip..).unwrap_or(&[]);
        let lits: Vec<Option<(f64, usize, (usize, usize))>> = colour.iter().take(3).map(|&(a, z)| number(b, a, z)).collect();
        if lits.len() == 3 && lits.iter().all(|l| l.is_some_and(|(v, _, _)| (0.0..=1.0).contains(&v))) {
            self.fractions(&lits.into_iter().flatten().collect::<Vec<_>>());
            return;
        }
        // Не числа: ссылка на найденные цвета (`rgb_to_r_g_b(c, a)`,
        // `col[1]`) или выражение (`r * 0.5`) — второе не трогаем, но
        // говорим о нём.
        if colour.iter().take(3).any(|&(a, z)| is_expression(&b[a..z])) {
            self.found.computed += 1;
        }
    }

    /// Таблица долей с цветовым именем: `bg_colour = {0.2, 0.2, 0.2, 0.8}`.
    fn tables(&mut self, s: usize, e: usize) {
        let b = self.b;
        let mut i = s;
        while i < e {
            if b[i] != b'{' {
                i += 1;
                continue;
            }
            // Имя слева от `= {`.
            let mut p = i;
            while p > s && b[p - 1].is_ascii_whitespace() {
                p -= 1;
            }
            let named_ok = p > s && b[p - 1] == b'=' && !(p > s + 1 && matches!(b[p - 2], b'=' | b'~' | b'<' | b'>')) && {
                let mut q = p - 1;
                while q > s && b[q - 1].is_ascii_whitespace() {
                    q -= 1;
                }
                let mut r = q;
                while r > s && is_ident(b[r - 1]) {
                    r -= 1;
                }
                colour_name(&String::from_utf8_lossy(&b[r..q]))
            };
            if named_ok {
                if let Some(args) = call_args_with(b, i, e, b'{', b'}') {
                    if args.len() == 3 || args.len() == 4 {
                        let lits: Vec<_> = args.iter().take(3).map(|&(a, z)| number(b, a, z)).collect();
                        if lits.iter().all(|l| l.is_some_and(|(v, _, _)| (0.0..=1.0).contains(&v))) {
                            self.fractions(&lits.into_iter().flatten().collect::<Vec<_>>());
                        }
                    }
                }
            }
            i += 1;
        }
    }

    fn fractions(&mut self, lits: &[(f64, usize, (usize, usize))]) {
        let rgb = [0, 1, 2].map(|k| (lits[k].0 * 255.0).round().clamp(0.0, 255.0) as u8);
        let decimals = lits.iter().map(|l| l.1).max().unwrap_or(0).max(3);
        self.push(lits.iter().map(|l| l.2).collect(), Form::Fractions { decimals }, Kind::Script, rgb);
    }
}

/// Вызовы cairo с цветом и сколько доводов перед ним.
const CAIRO_CALLS: &[(&str, usize)] = &[
    ("cairo_set_source_rgb", 1),
    ("cairo_set_source_rgba", 1),
    ("cairo_pattern_add_color_stop_rgb", 2),
    ("cairo_pattern_add_color_stop_rgba", 2),
    ("cairo_pattern_create_rgb", 0),
    ("cairo_pattern_create_rgba", 0),
];

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Следующее слово начинает новый оператор Lua (поле в таблице без запятой
/// не бывает, а `local x = 0xffffff` и следом `local y` — бывает на
/// одной строке через пробел).
fn starts_new_statement(b: &[u8], q: usize, e: usize) -> bool {
    let mut j = q;
    while j < e && is_ident(b[j]) {
        j += 1;
    }
    matches!(&b[q..j], b"local" | b"end" | b"return" | b"if" | b"for" | b"while" | b"function")
}

/// Имя таблицы-цвета: `…col…`, `bg`, `fg`, `bg_…`, `…_fg`.
fn colour_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    !n.is_empty()
        && (n.contains("col")
            || n == "bg"
            || n == "fg"
            || n.starts_with("bg_")
            || n.starts_with("fg_")
            || n.ends_with("_bg")
            || n.ends_with("_fg"))
}

/// Выражение (арифметика), а не ссылка на имя или вызов.
fn is_expression(t: &[u8]) -> bool {
    t.iter().any(|c| matches!(c, b'*' | b'/' | b'+' | b'%' | b'^')) || contains(t, b"math.")
}

/// Доводы скобки с позиции `open` до парной: промежутки без пробелов.
fn call_args(b: &[u8], open: usize, e: usize) -> Option<Vec<(usize, usize)>> {
    call_args_with(b, open, e, b'(', b')')
}

fn call_args_with(b: &[u8], open: usize, e: usize, lp: u8, rp: u8) -> Option<Vec<(usize, usize)>> {
    if b.get(open) != Some(&lp) {
        return None;
    }
    let mut depth = 0usize;
    let mut start = open + 1;
    let mut out = Vec::new();
    let mut i = open;
    while i < e {
        let c = b[i];
        if c == b'(' || c == b'{' || c == b'[' {
            depth += 1;
        } else if c == b')' || c == b'}' || c == b']' {
            depth -= 1;
            if depth == 0 {
                if c != rp {
                    return None;
                }
                let (a, z) = trim_span(b, start, i);
                if a < z {
                    out.push((a, z));
                }
                return Some(out);
            }
        } else if c == b',' && depth == 1 {
            out.push(trim_span(b, start, i));
            start = i + 1;
        }
        i += 1;
    }
    None
}

/// Число Lua целиком в `[a, z)`: `(значение, знаков после точки, промежуток)`.
fn number(b: &[u8], a: usize, z: usize) -> Option<(f64, usize, (usize, usize))> {
    let t = &b[a..z];
    if t.is_empty() || !t.iter().all(|c| c.is_ascii_digit() || *c == b'.') || t.iter().filter(|c| **c == b'.').count() > 1 {
        return None;
    }
    let s = std::str::from_utf8(t).ok()?;
    let v: f64 = s.parse().ok()?;
    let decimals = s.split_once('.').map_or(0, |(_, d)| d.len());
    Some((v, decimals, (a, z)))
}

/// Найти цвета в одном файле.
pub fn scan_bytes(file: &str, bytes: &[u8], role: Role, found: &mut Found) {
    let mut sc = Scanner { b: bytes, file, found };
    match role {
        Role::Config => sc.config(),
        Role::OldConfig => sc.old_config(),
        Role::Script => sc.script(),
    }
}

/// Прочитать и разобрать файлы; нечитаемые и слишком большие пропускаются.
/// Второе — содержимое прочитанных (для замены).
pub fn scan_files(files: &[String]) -> (Found, BTreeMap<String, Vec<u8>>) {
    let mut found = Found::default();
    let mut texts = BTreeMap::new();
    for f in files {
        let Ok(bytes) = std::fs::read(f) else { continue };
        if bytes.len() > MAX_FILE || bytes.contains(&0) {
            continue;
        }
        let role = role_of(f, &String::from_utf8_lossy(&bytes));
        scan_bytes(f, &bytes, role, &mut found);
        texts.insert(f.clone(), bytes);
    }
    (found, texts)
}

// ════════════════════════════════════════════════════════════════════════════
//  Палитра и замена
// ════════════════════════════════════════════════════════════════════════════

/// Кружок палитры: один цвет и все его места.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Group {
    /// `#rrggbb`.
    pub hex: String,
    pub count: usize,
    /// Подпись счётчика («12 мест»).
    pub count_label: String,
    /// Подсказка: где именно.
    pub tip: String,
}

/// Кружки палитры: частые цвета первыми.
pub fn groups(found: &Found) -> Vec<Group> {
    let mut by: BTreeMap<[u8; 3], BTreeMap<Kind, usize>> = BTreeMap::new();
    for p in &found.places {
        *by.entry(p.rgb).or_default().entry(p.kind).or_default() += 1;
    }
    let mut out: Vec<Group> = by
        .into_iter()
        .map(|(rgb, kinds)| {
            let count: usize = kinds.values().sum();
            let lines: Vec<String> = kinds
                .iter()
                .map(|(k, n)| {
                    let n = *n as u64;
                    match k {
                        Kind::Setting => ntr("Widget settings: %n", "Widget settings: %n", n),
                        Kind::Text => ntr("Widget text: %n", "Widget text: %n", n),
                        Kind::Graph => ntr("Graphs: %n", "Graphs: %n", n),
                        Kind::Script => ntr("Scripts: %n", "Scripts: %n", n),
                    }
                })
                .collect();
            Group {
                hex: hex_label(rgb),
                count,
                count_label: ntr("%n place", "%n places", count as u64),
                tip: format!("{}\n{}", hex_label(rgb), lines.join("\n")),
            }
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.hex.cmp(&b.hex)));
    out
}

/// Содержимое файлов после замены цвета `from` на `to` во всех его местах;
/// в ответе — только изменившиеся файлы.
pub fn replaced(texts: &BTreeMap<String, Vec<u8>>, found: &Found, from: [u8; 3], to: [u8; 3]) -> BTreeMap<String, Vec<u8>> {
    let mut edits: BTreeMap<&str, Vec<((usize, usize), String)>> = BTreeMap::new();
    for p in found.places.iter().filter(|p| p.rgb == from) {
        for (i, span) in p.spans.iter().enumerate() {
            edits.entry(p.file.as_str()).or_default().push((*span, render(p.form, to, i)));
        }
    }
    let mut out = BTreeMap::new();
    for (file, mut list) in edits {
        let Some(orig) = texts.get(file) else { continue };
        list.sort_by(|a, b| b.0 .0.cmp(&a.0 .0));
        list.dedup_by(|a, b| a.0 == b.0);
        let mut bytes = orig.clone();
        for ((s, e), text) in list {
            bytes.splice(s..e, text.into_bytes());
        }
        if &bytes != orig {
            out.insert(file.to_string(), bytes);
        }
    }
    out
}

// ════════════════════════════════════════════════════════════════════════════
//  Резервная копия и запись
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Origin {
    version: u32,
    /// Путь виджета.
    widget: String,
    files: Vec<OriginFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct OriginFile {
    /// Файл виджета.
    path: String,
    /// Имя копии в каталоге.
    copy: String,
    /// SHA-256 того, что Rexon записал последним: не совпало — файл с тех
    /// пор правили руками.
    written: String,
}

fn sha(bytes: &[u8]) -> String {
    <sha2::Sha256 as sha2::Digest>::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Каталог копий виджета `widget_path` под корнем `root`.
pub fn backup_dir(root: &Path, widget_path: &str) -> PathBuf {
    root.join(crate::conky_overlay::key(widget_path))
}

fn load_origin(dir: &Path) -> Option<Origin> {
    serde_json::from_slice(&std::fs::read(dir.join(ORIGIN_FILE)).ok()?).ok()
}

fn save_origin(dir: &Path, o: &Origin) -> Result<(), String> {
    let text = serde_json::to_string_pretty(o).unwrap_or_default();
    crate::utils::atomic_write(&dir.join(ORIGIN_FILE).to_string_lossy(), &text).map_err(|e| e.to_string())
}

/// Есть ли копия (цвета менялись).
pub fn has_backup(dir: &Path) -> bool {
    load_origin(dir).is_some_and(|o| !o.files.is_empty())
}

/// Файлы, которые правили после последней смены цвета (не Rexon).
pub fn changed_outside(dir: &Path) -> Vec<String> {
    let Some(o) = load_origin(dir) else { return Vec::new() };
    o.files
        .iter()
        .filter(|f| std::fs::read(&f.path).map(|b| sha(&b) != f.written).unwrap_or(false))
        .map(|f| f.path.clone())
        .collect()
}

/// Можно ли Rexon менять файл: обычный файл (ссылка — по цели) в доме
/// человека, его владелец — сам человек, запись разрешена. `Err` — отказ
/// для человека.
pub fn check_writable(path: &str, home: &str) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let refuse = || {
        trf(
            "Rexon can't change “%1”: the file belongs to another user or is read-only. Change its colours with “📝 Open settings file”.",
            &[&path.rsplit('/').next().unwrap_or(path)],
        )
    };
    let real = std::fs::canonicalize(path).map_err(|_| refuse())?;
    let home = std::fs::canonicalize(home).map_err(|_| refuse())?;
    if !real.starts_with(&home) {
        return Err(refuse());
    }
    let meta = std::fs::metadata(&real).map_err(|_| refuse())?;
    // SAFETY: geteuid без доводов, ошибок не бывает.
    let uid = unsafe { libc::geteuid() };
    if !meta.is_file() || meta.uid() != uid {
        return Err(refuse());
    }
    // Открыть на запись, не обрезая: так видны и права, и том только для
    // чтения.
    std::fs::OpenOptions::new().write(true).open(&real).map_err(|_| refuse())?;
    Ok(())
}

/// Заменить цвет `from` на `to` во всех местах файлов `files` виджета
/// `widget_path`. Перед первой правкой файла — его копия в `dir`. Все или
/// ничего: отказ одного файла — не меняется ни один. `Ok(мест)`.
pub fn apply_change(files: &[String], home: &str, dir: &Path, widget_path: &str, from: [u8; 3], to: [u8; 3]) -> Result<usize, String> {
    let (found, texts) = scan_files(files);
    let count = found.places.iter().filter(|p| p.rgb == from).count();
    if count == 0 {
        return Err(tr("This colour is no longer in the widget's files — the palette has been refreshed."));
    }
    if from == to {
        return Ok(0);
    }
    let new = replaced(&texts, &found, from, to);
    for file in new.keys() {
        check_writable(file, home)?;
    }
    let save_failed = || tr("Rexon couldn't save the widget's files.");
    std::fs::create_dir_all(dir).map_err(|_| save_failed())?;
    let mut origin = load_origin(dir).unwrap_or_else(|| Origin { version: 1, widget: widget_path.to_string(), files: Vec::new() });
    // 1. Копии — до первой записи файла; опись — до записи самих файлов.
    for file in new.keys() {
        if origin.files.iter().any(|f| &f.path == file) {
            continue;
        }
        let copy = format!("{}.orig", origin.files.len());
        crate::utils::atomic_write_bytes(&dir.join(&copy).to_string_lossy(), &texts[file]).map_err(|_| save_failed())?;
        origin.files.push(OriginFile { path: file.clone(), copy, written: sha(&texts[file]) });
    }
    save_origin(dir, &origin).map_err(|_| save_failed())?;
    // 2. Файлы.
    for (file, bytes) in &new {
        crate::utils::atomic_write_bytes(file, bytes).map_err(|_| save_failed())?;
        if let Some(f) = origin.files.iter_mut().find(|f| &f.path == file) {
            f.written = sha(bytes);
        }
    }
    save_origin(dir, &origin).map_err(|_| save_failed())?;
    Ok(count)
}

/// «Вернуть цвета»: положить копии на место и убрать их. `Ok(файлов)`.
pub fn revert(dir: &Path, home: &str) -> Result<usize, String> {
    let Some(origin) = load_origin(dir) else {
        return Err(tr("There is no copy of the widget's files, so Rexon can't restore the colours."));
    };
    let mut copies = Vec::new();
    for f in &origin.files {
        let bytes = std::fs::read(dir.join(&f.copy))
            .map_err(|_| tr("There is no copy of the widget's files, so Rexon can't restore the colours."))?;
        if std::path::Path::new(&f.path).exists() {
            check_writable(&f.path, home)?;
        }
        copies.push((f.path.clone(), bytes));
    }
    for (path, bytes) in &copies {
        // Файл удалили вместе с виджетом — возвращать некуда.
        if !std::path::Path::new(path).exists() {
            continue;
        }
        crate::utils::atomic_write_bytes(path, bytes).map_err(|_| tr("Rexon couldn't save the widget's files."))?;
    }
    let _ = std::fs::remove_dir_all(dir);
    Ok(copies.len())
}

// ════════════════════════════════════════════════════════════════════════════
//  Для карточки
// ════════════════════════════════════════════════════════════════════════════

/// Подписи блока палитры.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Texts {
    pub title: String,
    pub hint: String,
    pub revert: String,
    pub revert_tooltip: String,
    pub pick_title: String,
    pub empty: String,
}

pub fn texts() -> Texts {
    Texts {
        title: tr("Widget colours"),
        hint: tr("Click a colour to change it everywhere the widget uses it."),
        revert: tr("↩️ Restore colours"),
        revert_tooltip: tr("Put the widget's files back the way they were before the first colour change."),
        pick_title: tr("Pick a colour"),
        empty: tr("Rexon found no colours it can change in this widget's files."),
    }
}

/// Блок палитры для страницы (JSON).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct View {
    pub available: bool,
    pub colours: Vec<Group>,
    /// «Часть цветов задана картинками или вычисляется».
    pub note: Option<String>,
    /// Своя картинка автора: плитка новых цветов не покажет.
    pub picture_note: Option<String>,
    pub can_revert: bool,
    pub t: Texts,
}

/// Блок палитры по найденному.
pub fn view(found: &Found, any_file: bool, own_picture: bool, can_revert: bool) -> View {
    let note = (found.computed > 0 || found.pictures).then(|| {
        tr("Some colours are drawn by pictures or computed by the widget's scripts — change them with “📝 Open settings file”.")
    });
    let picture_note = own_picture.then(|| {
        tr("The widget comes with its own picture, so the tile won't show the new colours. Start the widget to see them.")
    });
    View { available: any_file, colours: groups(found), note, picture_note, can_revert, t: texts() }
}

/// Корень копий на этой машине.
pub fn live_backup_root(ctx: &Ctx) -> PathBuf {
    let data = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), &ctx.home);
    Path::new(&data).join(BACKUP_SUBDIR)
}

/// Убрать копии виджета, лежавшего в `dir` (виджет удалён — возвращать
/// некуда).
pub fn forget(ctx: &Ctx, dir: &str) {
    let _ = std::fs::remove_dir_all(backup_dir(&live_backup_root(ctx), dir.trim_end_matches('/')));
}

struct Live {
    io: crate::animations::SystemIo,
    ctx: Ctx,
    path: String,
    files: Vec<String>,
    dir: PathBuf,
}

fn live(name: &str) -> Live {
    let io = crate::animations::SystemIo;
    let ctx = Ctx::live();
    let path = crate::conky_scan::widget_path(&ctx, name);
    let files = crate::conky_safety::look_files(&io, &ctx, name);
    let dir = backup_dir(&live_backup_root(&ctx), &path);
    Live { io, ctx, path, files, dir }
}

/// Живой блок палитры виджета `name`.
pub fn live_view(name: &str) -> View {
    let l = live(name);
    let (found, texts) = scan_files(&l.files);
    let confs = crate::conky_scan::widget_confs(&l.io, &l.ctx, name, None);
    let own = crate::conky_scan::widget_picture(&l.io, &l.path, &confs).is_some();
    view(&found, !texts.is_empty(), own, has_backup(&l.dir))
}

/// После записи файлов: разрешение, накладки, запущенные процессы, снимок.
fn after_change(l: &Live, name: &str, trusted: bool) {
    if trusted {
        crate::conky_safety::remember(&l.io, &l.ctx, name);
    }
    let env = crate::conky_overlay::Env { version: crate::conky_overlay::conky_version(&l.io), de: l.ctx.de.clone() };
    for conf in crate::conky_procs::parts(&l.io, &l.ctx, name).into_iter().map(|p| p.conf) {
        let p = crate::conky_overlay::live_paths(&l.ctx, &conf);
        let ov = crate::conky_overlay::load(&l.io, &p);
        if !ov.is_empty() {
            let _ = crate::conky_overlay::save(&l.io, &p, &conf, &ov, &env);
        }
        for (pid, _) in crate::conky_overlay::processes_of(&l.io, &conf) {
            if let Ok(pid) = pid.parse::<i32>() {
                // SAFETY: номер — процесс conky этого файла (сверено по
                // командной строке в `processes_of`); SIGUSR1 у conky —
                // «перечитать настройки».
                unsafe {
                    libc::kill(pid, libc::SIGUSR1);
                }
            }
        }
    }
    crate::conky_preview::want(&l.ctx, name);
}

/// Поменять цвет `from` (`#rrggbb`) на `to` во всём виджете `name`.
pub fn live_set(name: &str, from: &str, to: &str) -> (bool, String) {
    let (Some(f), Some(t)) = (parse_hex(from), parse_hex(to)) else {
        return (false, tr("This colour is no longer in the widget's files — the palette has been refreshed."));
    };
    let l = live(name);
    let trusted = crate::conky_safety::trusted_as_is(&l.io, &l.ctx, name);
    match apply_change(&l.files, &l.ctx.home, &l.dir, &l.path, f, t) {
        Ok(0) => (true, String::new()),
        Ok(n) => {
            after_change(&l, name, trusted);
            let title = crate::conky_scan::widget_title(name);
            (true, trf("✅ “%1”: %2 changed to %3 (%4)", &[&title, &hex_label(f), &hex_label(t), &ntr("%n place", "%n places", n as u64)]))
        }
        Err(why) => (false, why),
    }
}

/// Вопрос перед «Вернуть цвета», если файлы правили и после Rexon:
/// `(заголовок, текст)`.
pub fn live_revert_question(name: &str) -> Option<(String, String)> {
    let l = live(name);
    if changed_outside(&l.dir).is_empty() {
        return None;
    }
    Some((
        tr("Restore colours?"),
        tr("Some of the widget's files were edited after Rexon changed the colours. Restoring the colours will undo those edits too."),
    ))
}

/// «Вернуть цвета» виджета `name`.
pub fn live_revert(name: &str) -> (bool, String) {
    let l = live(name);
    let trusted = crate::conky_safety::trusted_as_is(&l.io, &l.ctx, name);
    match revert(&l.dir, &l.ctx.home) {
        Ok(_) => {
            after_change(&l, name, trusted);
            (true, trf("↩️ “%1”: colours restored", &[&crate::conky_scan::widget_title(name)]))
        }
        Err(why) => (false, why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_one(name: &str, text: &str) -> Found {
        let mut f = Found::default();
        scan_bytes(name, text.as_bytes(), role_of(name, text), &mut f);
        f
    }

    fn hexes(f: &Found, kind: Kind) -> Vec<String> {
        f.places.iter().filter(|p| p.kind == kind).map(|p| hex_label(p.rgb)).collect()
    }

    fn change(name: &str, text: &str, from: &str, to: &str) -> String {
        let f = scan_one(name, text);
        let texts = BTreeMap::from([(name.to_string(), text.as_bytes().to_vec())]);
        let out = replaced(&texts, &f, parse_hex(from).unwrap(), parse_hex(to).unwrap());
        String::from_utf8(out.get(name).cloned().unwrap_or_else(|| text.as_bytes().to_vec())).unwrap()
    }

    const LUA_CONF: &str = r##"-- default_color = 'red'
conky.config = {
    default_color = 'white',
    default_outline_color = "#FF8800",
    color1 = 'ff8800',
    color2 = '#f80',
    own_window_colour = '000000',
    lua_load = 'rings.lua',
}
conky.text = [[
${color #ff8800}CPU ${color1}${cpu}% ${color}
${cpugraph cpu0 25,200 FF8800 1e90ff -t}
${color Tan1}Up ${color gold}${color ${exec cat c}}${color nosuchname}
${image ./bg.png -p 0,0}
]]
"##;

    #[test]
    fn lua_config_finds_settings_text_and_graphs() {
        let f = scan_one("/w/conky.conf", LUA_CONF);
        assert_eq!(hexes(&f, Kind::Setting), ["#ffffff", "#ff8800", "#ff8800", "#ff8800"]);
        assert_eq!(hexes(&f, Kind::Text), ["#ff8800", "#ffa54f", "#ffd700"]);
        assert_eq!(hexes(&f, Kind::Graph), ["#ff8800", "#1e90ff"]);
        // `${color ${exec}}` и неизвестное имя — «вычисляется».
        assert_eq!(f.computed, 2);
        assert!(f.pictures);
        let g = groups(&f);
        assert_eq!(g[0].hex, "#ff8800");
        assert_eq!(g[0].count, 5);
    }

    #[test]
    fn lua_config_replacement_keeps_every_form() {
        let out = change("/w/conky.conf", LUA_CONF, "#ff8800", "#123abc");
        let want = LUA_CONF
            .replace("\"#FF8800\"", "\"#123ABC\"")
            .replace("'ff8800'", "'123abc'")
            .replace("'#f80'", "'#123abc'")
            .replace("${color #ff8800}", "${color #123abc}")
            .replace("25,200 FF8800", "25,200 123ABC");
        assert_eq!(out, want);
        // Имя X11 становится hex без `#`; комментарий не тронут.
        let out = change("/w/conky.conf", LUA_CONF, "#ffffff", "#000001");
        assert!(out.starts_with("-- default_color = 'red'\n"));
        assert!(out.contains("default_color = '000001',"));
    }

    const OLD_CONF: &str = "# default_color red\n\
own_window_colour 000000\n\
default_color 777 # серый\n\
color1 FF8800\n\
lua_load ~/x.lua\n\
TEXT\n\
${color1}Hi ${color #ff8800}there ${color white}\n\
${downspeedgraph eth0 30,100 000000 ff8800 100000}\n";

    #[test]
    fn old_conkyrc_is_read_and_changed() {
        let f = scan_one("/w/.conkyrc", OLD_CONF);
        assert_eq!(hexes(&f, Kind::Setting), ["#777777", "#ff8800"]);
        assert_eq!(hexes(&f, Kind::Text), ["#ff8800", "#ffffff"]);
        assert_eq!(hexes(&f, Kind::Graph), ["#000000", "#ff8800"]);
        let out = change("/w/.conkyrc", OLD_CONF, "#ff8800", "#00aa00");
        let want = OLD_CONF
            .replace("color1 FF8800", "color1 00AA00")
            .replace("${color #ff8800}", "${color #00aa00}")
            .replace("000000 ff8800 100000", "000000 00aa00 100000");
        assert_eq!(out, want);
        // Короткий hex становится полным, комментарий остаётся.
        let out = change("/w/.conkyrc", OLD_CONF, "#777777", "#102030");
        assert!(out.contains("default_color 102030 # серый\n"));
    }

    const RINGS: &str = r#"--[[ кольца
bg_colour = 0x123456,
]]
settings_table = {
    { name = 'cpu', bg_colour = 0xffffff, bg_alpha = 0.2, fg_colour=0xFF8800, fg_alpha = 0.8 },
    { name = 'mem', bg_colour = 0xffffff, fg_colour = 0x1e90ff },
}
local bg = {0.2, 0.2, 0.2, 0.8}
local fg_root = {1, 0.5333, 0, 0.9}
local mask = bit.band(c, 0xff0000)
local big = 0x100000 * n
function rgb_to_r_g_b(colour, alpha)
    return ((colour / 0x10000) % 0x100) / 255., ((colour / 0x100) % 0x100) / 255., (colour % 0x100) / 255., alpha
end
function conky_main()
    cairo_set_source_rgba(cr, rgb_to_r_g_b(pt['bg_colour'], pt['bg_alpha']))
    cairo_set_source_rgba(cr, 1, 0.533, 0, 1)
    cairo_set_source_rgb(cr, r * 0.5, g, b)
    cairo_pattern_add_color_stop_rgba(pat, 0.5, 0.2, 0.2, 0.2, 1)
    local s = cairo_image_surface_create_from_png('a.png')
    conky_parse('${color #FF8800}')
end
"#;

    #[test]
    fn lua_script_finds_hex_fractions_and_tables() {
        let f = scan_one("/w/rings.lua", RINGS);
        let s = hexes(&f, Kind::Script);
        assert_eq!(s.iter().filter(|h| *h == "#ffffff").count(), 2);
        assert_eq!(s.iter().filter(|h| *h == "#1e90ff").count(), 1);
        // 0xFF8800, {1, 0.5333, 0}, (1, 0.533, 0) — все #ff8800.
        assert_eq!(s.iter().filter(|h| *h == "#ff8800").count(), 3);
        // {0.2…} и остановка градиента — #333333.
        assert_eq!(s.iter().filter(|h| *h == "#333333").count(), 2);
        // Ни комментарий, ни маска, ни слагаемое.
        assert!(!s.iter().any(|h| h == "#123456" || h == "#ff0000" || h == "#100000"));
        assert_eq!(hexes(&f, Kind::Text), ["#ff8800"]);
        assert_eq!(f.computed, 1);
        assert!(f.pictures);
    }

    #[test]
    fn lua_script_replacement_keeps_form_and_precision() {
        let out = change("/w/rings.lua", RINGS, "#ff8800", "#336699");
        let want = RINGS
            .replace("fg_colour=0xFF8800", "fg_colour=0x336699")
            .replace("{1, 0.5333, 0, 0.9}", "{0.2000, 0.4000, 0.6000, 0.9}")
            .replace("(cr, 1, 0.533, 0, 1)", "(cr, 0.200, 0.400, 0.600, 1)")
            .replace("'${color #FF8800}'", "'${color #336699}'");
        assert_eq!(out, want);
        // Доли возвращаются тем же цветом: кружок не распадается.
        let again = scan_one("/w/rings.lua", &out);
        assert_eq!(hexes(&again, Kind::Script).iter().filter(|h| *h == "#336699").count(), 3);
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-conky-colours-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("w")).unwrap();
        d
    }

    #[test]
    fn change_and_revert_are_byte_exact() {
        let d = temp("revert");
        let home = d.to_string_lossy().to_string();
        let conf = d.join("w/conky.conf").to_string_lossy().to_string();
        let lua = d.join("w/rings.lua").to_string_lossy().to_string();
        let crlf = LUA_CONF.replace('\n', "\r\n");
        std::fs::write(&conf, &crlf).unwrap();
        std::fs::write(&lua, RINGS).unwrap();
        let files = vec![conf.clone(), lua.clone()];
        let backup = d.join("backup");
        let widget = d.join("w").to_string_lossy().to_string();

        let n = apply_change(&files, &home, &backup, &widget, [0xff, 0x88, 0], [1, 2, 3]).unwrap();
        assert_eq!(n, 9);
        assert!(has_backup(&backup));
        assert!(changed_outside(&backup).is_empty());
        let after = std::fs::read_to_string(&conf).unwrap();
        assert!(after.contains("color1 = '010203',\r\n"));
        // Вторая правка — копия остаётся первой.
        apply_change(&files, &home, &backup, &widget, [1, 2, 3], [9, 9, 9]).unwrap();
        // Ручная правка после Rexon — видна.
        std::fs::write(&lua, format!("{RINGS}-- руками\n")).unwrap();
        assert_eq!(changed_outside(&backup), [lua.clone()]);

        assert_eq!(revert(&backup, &home).unwrap(), 2);
        assert_eq!(std::fs::read(&conf).unwrap(), crlf.as_bytes());
        assert_eq!(std::fs::read(&lua).unwrap(), RINGS.as_bytes());
        assert!(!has_backup(&backup));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn read_only_or_foreign_files_are_refused_whole() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp("ro");
        let home = d.join("w").to_string_lossy().to_string();
        let conf = d.join("w/conky.conf").to_string_lossy().to_string();
        let lua = d.join("w/rings.lua").to_string_lossy().to_string();
        std::fs::write(&conf, LUA_CONF).unwrap();
        std::fs::write(&lua, RINGS).unwrap();
        std::fs::set_permissions(&lua, std::fs::Permissions::from_mode(0o444)).unwrap();
        let backup = d.join("backup");
        let files = vec![conf.clone(), lua.clone()];
        let err = apply_change(&files, &home, &backup, &home, [0xff, 0x88, 0], [1, 2, 3]).unwrap_err();
        assert!(err.contains("rings.lua"), "{err}");
        // Не тронут ни один файл, копий нет.
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), LUA_CONF);
        assert!(!has_backup(&backup));
        // Файл вне дома — тоже отказ.
        std::fs::set_permissions(&lua, std::fs::Permissions::from_mode(0o644)).unwrap();
        let outside = d.join("outside.conf");
        std::fs::write(&outside, LUA_CONF).unwrap();
        let o = outside.to_string_lossy().to_string();
        assert!(apply_change(&[o], &home, &backup, &home, [0xff, 0x88, 0], [1, 2, 3]).is_err());
        // Цвета больше нет — понятный отказ.
        assert!(apply_change(&files, &home, &backup, &home, [7, 7, 7], [1, 2, 3]).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn names_and_hex_parsing() {
        assert_eq!(named("White"), Some([255, 255, 255]));
        assert_eq!(named("grey50"), Some([127, 127, 127]));
        assert_eq!(named("green"), Some([0, 255, 0]));
        assert_eq!(named("nosuch"), None);
        assert_eq!(parse_hex("#Ff8800"), Some([255, 136, 0]));
        assert_eq!(parse_hex("ff88"), None);
        assert!(is_setting("color9") && is_setting("default_colour") && !is_setting("own_window_colour") && !is_setting("color10"));
    }
}
