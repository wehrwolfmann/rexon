//! Conky widget settings on top of the author's file — the "overlay".
//!
//! WHY. A person wants to move a widget to another corner, make the background
//! more transparent or switch the clock to 12-hour. Conky manager programs
//! rewrite the author's file line by line for this: when the widget is updated from
//! the store the edits are lost, case and formatting get spoiled, and a field
//! that was not in the file is not inserted at all. Rexon does not touch the author's
//! file. Next to it, in `$XDG_STATE_HOME/rexon/conky/overlay/`, lies our own
//! settings file — the overlay ([`render`]): it executes the author's file
//! (`dofile` with an absolute path) and overrides the needed fields of the
//! `conky.config` table. conky is started with the overlay (`-c <overlay>`) and with
//! the working directory = the folder of the author's file, so the widget's relative paths
//! (`lua_load ./x.lua`, `${image ./bg.png}`) work as before.
//! "Reset" — delete the overlay.
//!
//! Test bench measurements (conky 1.25.2, Xvfb, own HOME, 19–20.10.2026):
//! * `dofile` + replacing `conky.config.gap_x` moves the window; `conky.text`
//!   comes from the author's file and can be changed in the overlay (`gsub`);
//! * conky watches for changes ONLY of the file from `-c`: an overlay edit
//!   is picked up by itself (≤2 s), an edit of the author's file is not;
//! * `dofile` of a 1.9-syntax file fails with a syntax error — such a
//!   file is first converted to 1.10 into a copy, and the overlay executes the copy;
//! * the Lua state of the settings file has neither `conky_version` nor
//!   `conky_config` — Rust finds out the conky version itself (`conky -v`).
//!
//! Parsing the author's file ([`parse_config`]) understands only literals: a field
//! set by an expression (`gap_x = 10 * scale`) is shown as "set by the
//! widget", and its slider is disabled until the person sets their own.
//!
//! A conky process with an overlay is recognized by its path: the overlay name is the encoded
//! path of the author's file ([`original_of_launch`]), so "Stop",
//! "Running" and autostart see the widget as if it were started with the author's
//! file.

use std::collections::BTreeMap;

use crate::animations::LookIo;
use crate::de_apply::Ctx;
use crate::i18n::{tr, trf};

/// Part of the overlay path: `…/rexon/conky/overlay/<key>.rexon.conf`.
pub const OVERLAY_SEGMENT: &str = "/rexon/conky/overlay/";
/// The overlay name ending.
pub const OVERLAY_SUFFIX: &str = ".rexon.conf";
/// The mark of the XWayland launch file: `<key>+x.rexon.conf`. The key
/// ([`key`]) writes `+` only as `+s`/`+p` — it cannot be confused with an overlay.
pub const XWAYLAND_KEY_TAG: &str = "+x";
/// The name ending of the file with the chosen settings.
pub const SETTINGS_SUFFIX: &str = ".json";
/// The name ending of a copy converted from the 1.9 syntax.
pub const CONVERTED_SUFFIX: &str = ".v110.lua";

// ════════════════════════════════════════════════════════════════════════════
//  Where it lives
// ════════════════════════════════════════════════════════════════════════════

/// The overlay files of one widget settings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The overlay — conky is started with it.
    pub overlay: String,
    /// The person's choice ([`Overrides`] in JSON): the overlay is rewritten
    /// from it.
    pub settings: String,
    /// A copy of a 1.9-syntax file converted to 1.10.
    pub converted: String,
}

/// The overlays directory: `<state>/rexon/conky/overlay`.
pub fn overlay_dir(ctx: &Ctx, xdg_state_home: Option<&str>) -> String {
    format!("{}/overlay", crate::conky_log::log_dir(ctx, xdg_state_home))
}

/// The key of the settings file `conf` is its path, where `/` is written as `+s`, and `+`
/// itself as `+p`. There is deliberately no `%` in the key: the overlay path goes
/// into the autostart `Exec=`, where `%` is special.
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

/// The settings file by key ([`key`] in reverse).
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

/// Live paths (`$XDG_STATE_HOME` from the environment).
pub fn live_paths(ctx: &Ctx, conf: &str) -> Paths {
    paths(ctx, std::env::var("XDG_STATE_HOME").ok().as_deref(), conf)
}

/// The author's file if `launch` is a Rexon overlay; otherwise `None`.
pub fn original_of_launch(launch: &str) -> Option<String> {
    let at = launch.rfind(OVERLAY_SEGMENT)?;
    let name = &launch[at + OVERLAY_SEGMENT.len()..];
    let key = name.strip_suffix(OVERLAY_SUFFIX)?;
    let key = key.strip_suffix(XWAYLAND_KEY_TAG).unwrap_or(key);
    (!key.is_empty() && !key.contains('/')).then(|| conf_of_key(key))
}

// ════════════════════════════════════════════════════════════════════════════
//  The person's choice
// ════════════════════════════════════════════════════════════════════════════

/// What the person changed in the widget. `None` — leave it as the author made it.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Overrides {
    /// A screen corner or edge — the full conky name (`top_right`).
    pub alignment: Option<String>,
    pub gap_x: Option<i64>,
    pub gap_y: Option<i64>,
    pub min_width: Option<i64>,
    pub min_height: Option<i64>,
    /// No background (fully transparent).
    pub bg_none: Option<bool>,
    /// Background opacity, 0–100 %.
    pub bg_opacity: Option<u8>,
    /// Background color `RRGGBB`.
    pub bg_colour: Option<String>,
    /// The window — as Rexon advises for this desktop.
    pub window_preset: Option<bool>,
    /// Update every N seconds.
    pub update_interval: Option<f64>,
    /// The X11 screen (`xinerama_head`).
    pub xinerama_head: Option<u32>,
    /// Clock: `true` — 24-hour, `false` — 12.
    pub clock_24h: Option<bool>,
    /// A network interface in the widget text: old name → new.
    pub net_ifaces: BTreeMap<String, String>,
}

impl Overrides {
    pub fn is_empty(&self) -> bool {
        *self == Overrides::default()
    }

    /// Whether the background is set by at least one field.
    pub fn touches_background(&self) -> bool {
        self.bg_none.is_some() || self.bg_opacity.is_some() || self.bg_colour.is_some()
    }
}

/// Full conky alignment names — a 3×3 grid by rows.
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

/// The full alignment name for any conky spelling (`tr`, `top_right`,
/// `top`, `center`); `None` — not an alignment (`none` and others).
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
//  Parsing the conky.config table (literals only)
// ════════════════════════════════════════════════════════════════════════════

/// A field value of the author's file.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Num(f64),
    Bool(bool),
    Str(String),
    /// Not a literal: an expression, a call, a table — Rexon does not evaluate it.
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

/// The level of a long bracket `[==[` at position `i` (where `[` is); `None` — not one.
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

/// The end of a long string of level `level` started by the opening bracket at `i`:
/// `(contents, position after the closing bracket)`.
fn long_string(s: &str, i: usize, level: usize) -> (String, usize) {
    let open = i + level + 2;
    let close = format!("]{}]", "=".repeat(level));
    let mut start = open;
    // A newline right after the opening bracket is not part of the string.
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
        // Two-character tokens as one, so that `==` is not read as `=`.
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

/// A quoted string at position `i`: `(text, position after it)`.
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

/// The value from a field's tokens: a literal — the value itself, otherwise [`Value::Expr`].
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

/// The fields of the `conky.config = { … }` table of a 1.10 settings file. `None` —
/// there is no table (a 1.9 file, or one that builds it with code).
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
        // The key: `name =` or `["name"] =`.
        let (key, value_at) = match (&toks[i], toks.get(i + 1), toks.get(i + 2), toks.get(i + 3)) {
            (Tok::Name(k), Some(Tok::Sym(eq)), _, _) if eq == "=" => (Some(k.clone()), i + 2),
            (Tok::Sym(o), Some(Tok::Str(k)), Some(Tok::Sym(c)), Some(Tok::Sym(eq)))
                if o == "[" && c == "]" && eq == "=" =>
            {
                (Some(k.clone()), i + 4)
            }
            _ => (None, i),
        };
        // The value — up to a comma or semicolon of its own level.
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
//  Syntax 1.9 → 1.10 (into a copy)
// ════════════════════════════════════════════════════════════════════════════
//
// A conky file before 1.10: `name value` lines up to the `TEXT` line, after it —
// the widget text as is. `#` starts a comment (`\#` is the character itself).
// conky 1.10+ converts such a file itself, but only when it is passed in `-c`:
// `dofile` in the overlay fails on it with a syntax error. So
// an overlay on top of a 1.9 widget executes its copy, converted by Rexon following
// the format description ([`convert_19`]); the author's file itself is not touched, and without
// edits the widget is started with it, as before.

/// The settings file syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Syntax {
    /// conky 1.10+: Lua, `conky.config` / `conky.text`.
    V110,
    /// Before 1.10: `name value` and `TEXT`.
    V19,
}

/// The syntax by contents: a `TEXT` line without `conky.config`/`conky.text`
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

/// 1.9 fields of the "width height" kind — one 1.10 field per number.
const SPLIT_FIELDS: &[(&str, &str, &str)] = &[
    ("minimum_size", "minimum_width", "minimum_height"),
    ("default_bar_size", "default_bar_width", "default_bar_height"),
    ("default_graph_size", "default_graph_width", "default_graph_height"),
    ("default_gauge_size", "default_gauge_width", "default_gauge_height"),
];

/// A color field: the value is a string, even if it consists only of digits (`000000`).
fn is_colour_field(name: &str) -> bool {
    name.contains("color") || name.contains("colour")
}

/// A 1.9 line without the comment (`\#` is the `#` character, not the start of a comment).
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

/// A 1.9 field value as a Lua string.
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

/// A Lua long string with the text `text` — with a bracket level such that its closing
/// does not occur in the text.
fn long_lua_string(text: &str) -> String {
    let mut level = 0;
    while text.contains(&format!("]{}", "=".repeat(level))) && level < 64 {
        level += 1;
    }
    let eq = "=".repeat(level);
    // Lua itself drops a newline right after the opening bracket.
    format!("[{eq}[\n{text}]{eq}]")
}

/// Convert a 1.9 file to 1.10. The field order is kept; settings comments
/// are dropped, the text after `TEXT` is carried over as is (conky
/// 1.10 strips comments from it the same way 1.9 does — checked on the test bench).
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
        // In 1.10 the Xft font is just `font`.
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

/// The fields of the author's file and its syntax (1.9 — via an in-memory conversion).
pub fn author_config(text: &str) -> (Syntax, Option<BTreeMap<String, Value>>) {
    match syntax(text) {
        Syntax::V19 => (Syntax::V19, parse_config(&convert_19(text))),
        Syntax::V110 => (Syntax::V110, parse_config(text)),
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Current values: the overlay on top of the author
// ════════════════════════════════════════════════════════════════════════════

/// A conky color (`#RGB`, `#ARGB`, `RRGGBB`, `#RRGGBB`, `#AARRGGBB`, a few
/// names) → `(RRGGBB, alpha 0–255 if set)`.
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

/// A field with a "where from" label.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Field<T> {
    pub value: T,
    /// Set by the person in Rexon (otherwise — as the author made it).
    pub changed: bool,
    /// Not a literal at the author's: Rexon does not know the value, the slider is disabled.
    pub locked: bool,
}

/// The widget values as they are in effect now.
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

/// The author's background opacity (0–100) and "no background".
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

/// How the widget looks now: the person's choice on top of the author's file.
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
            // Defaults — as in conky itself (without the field it puts the window
            // in the bottom left corner with gaps of 5 and 60).
            crate::conky_layout::DEFAULT_ALIGNMENT.to_string(),
        ),
        gap_x: field(ov.gap_x, cfg.get("gap_x"), Value::as_i64, crate::conky_layout::DEFAULT_GAP_X),
        gap_y: field(ov.gap_y, cfg.get("gap_y"), Value::as_i64, crate::conky_layout::DEFAULT_GAP_Y),
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
//  The overlay
// ════════════════════════════════════════════════════════════════════════════

/// A quoted Lua string.
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

/// The conky version `(major, minor)` from the `conky -v` output
/// (`conky 1.25.2-pre-6caddca3 compiled …`).
pub fn parse_version(out: &str) -> Option<(u32, u32)> {
    let first = out.lines().find(|l| l.trim_start().starts_with("conky "))?;
    let ver = first.trim_start().strip_prefix("conky ")?.split_whitespace().next()?;
    let mut it = ver.split(|c: char| !c.is_ascii_digit());
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

/// The installed conky version; `None` — cannot find out.
pub fn conky_version(io: &dyn LookIo) -> Option<(u32, u32)> {
    let (ok, out) = io.run(&["conky".to_string(), "-v".to_string()]);
    if !ok {
        return None;
    }
    parse_version(&out)
}

/// Since which conky version the background transparency is the color's alpha (`#AARRGGBB`), and
/// `own_window_argb_value`/`own_window_transparent` are deprecated.
pub const ALPHA_COLOUR_SINCE: (u32, u32) = (1, 23);

/// The circumstances the overlay is written for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Env {
    /// The conky version; `None` — not found out (fields understood by all are written).
    pub version: Option<(u32, u32)>,
    /// The desktop (`Ctx::de`) — for window advice.
    pub de: String,
}

// ── Window per desktop ──────────────────────────────────────────────────
//
// Which conky window stays "on the desktop" depends on the desktop and the session
// (measurements and the conky ≥ 1.25 settings description):
// * X11 — a normal window with the hints "undecorated, below all windows, on all
//   desktops, not in the taskbar": the `desktop` type on desktops with their own icons
//   window (Caja, Nemo, xfdesktop) gets covered by that window on a click;
// * Wayland with wlr-layer-shell (KWin, wlroots, COSMIC, labwc, Treeland) —
//   the background layer: the `desktop` type; for KWin with conky 1.25 — a normal window with the same
//   hints and the `desktop` namespace (visible with "Show desktop",
//   `dock` would reserve space at the edge);
// * niri: `desktop` does not get into the overview background — a normal window with `below`
//   (the bottom layer);
// * GNOME, Cinnamon, Pantheon on Wayland know no layers: the widget is a normal
//   window, no fields change that.

/// X11 window hints (and the bottom layer on Wayland for conky 1.25).
pub const DESKTOP_HINTS: &str = "undecorated,below,sticky,skip_taskbar,skip_pager";

/// Rexon's advice for the widget window.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WindowPreset {
    pub own_window_type: &'static str,
    pub hints: Option<&'static str>,
    /// `own_window_namespace` (conky ≥ 1.25, Wayland only).
    pub namespace: Option<&'static str>,
}

/// Desktops that have no wlr-layer-shell on Wayland (the widgets group
/// explanation says the same, `categories::group_note`).
pub fn wayland_without_layers(de_id: &str) -> bool {
    use crate::de_customization::De;
    matches!(crate::de_customization::de_from_id(de_id), Some(De::Gnome | De::Cinnamon | De::Pantheon))
}

/// The widget window for the desktop `de_id` and the session. `None` — there is no suitable
/// window (Wayland without layers).
pub fn window_preset(de_id: &str, wayland: bool, version: Option<(u32, u32)>) -> Option<WindowPreset> {
    let normal = WindowPreset { own_window_type: "normal", hints: Some(DESKTOP_HINTS), namespace: None };
    if !wayland {
        return Some(normal);
    }
    if wayland_without_layers(de_id) {
        return None;
    }
    let desktop = WindowPreset { own_window_type: "desktop", hints: None, namespace: None };
    // A layer by hints for a normal window and a namespace — since conky 1.25.
    if !version.is_some_and(|v| v >= (1, 25)) {
        return Some(desktop);
    }
    Some(match de_id {
        "kde" => WindowPreset { namespace: Some("desktop"), ..normal },
        "niri" => normal,
        _ => desktop,
    })
}

/// What to say about the widget on this desktop; `None` — nothing.
pub fn preset_note(de_id: &str, wayland: bool) -> Option<String> {
    (wayland && wayland_without_layers(de_id)).then(|| {
        tr("In the Wayland session of this desktop the widget is a regular window: it can be covered by other \
            windows and shows up in Alt+Tab. The X11 session (chosen on the login screen) keeps it on the desktop.")
    })
}

// ── X11 output via XWayland ─────────────────────────────────────────────
//
// In a Wayland session conky can draw with its Wayland output or with the
// X11 output via XWayland. The X11 output is taken where the Wayland output is no good
// (measurements of 15.11.2026, Cinnamon 6.8 Wayland):
// * GNOME, Cinnamon, Pantheon: there is no desktop layer for conky — the Wayland window
//   is a normal one, covered by windows and visible in Alt+Tab; via XWayland an X11
//   window with the hints "below all, on all desktops" is held by the desktop properly
//   (checked live: a transparent widget on top of live wallpapers);
// * conky 1.25.0–1.25.2 on any desktop: the Wayland output crashes right at
//   startup (`display-wayland.cc … set_font: Assertion f < pango_fonts.size()`,
//   fixed in 1.25.3);
// * the widget draws with a Lua script through `cairo_xlib_*` — that is X11 only.
// Without XWayland (`DISPLAY` is empty) the Wayland output remains — there are no others.
// The switch is made by two changes in conky's environment: without `WAYLAND_DISPLAY` and with
// `XDG_SESSION_TYPE=x11` (this way conky itself chooses the X11 output, and the window branch
// of Rexon's overlay, [`window_lines`], — the X11 hints).

/// The conky version `(major, minor, patch)` from the `conky -v` output.
pub fn parse_version_full(out: &str) -> Option<(u32, u32, u32)> {
    let first = out.lines().find(|l| l.trim_start().starts_with("conky "))?;
    let ver = first.trim_start().strip_prefix("conky ")?.split_whitespace().next()?;
    let mut it = ver.split(|c: char| !c.is_ascii_digit());
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    Some((major, minor, it.next().and_then(|p| p.parse().ok()).unwrap_or(0)))
}

/// A conky release whose Wayland output crashes at startup.
pub fn wayland_output_aborts(v: (u32, u32, u32)) -> bool {
    v.0 == 1 && v.1 == 25 && v.2 <= 2
}

/// Whether conky goes through XWayland in this session (the X11 output). `display` —
/// XWayland exists (`DISPLAY` is set); `needs_x11` — the widget draws through X11.
pub fn via_xwayland(de_id: &str, wayland: bool, display: bool, version: Option<(u32, u32, u32)>, needs_x11: bool) -> bool {
    wayland && display && (needs_x11 || wayland_without_layers(de_id) || version.is_some_and(wayland_output_aborts))
}

/// Whether the session has an X server (`DISPLAY`; in a Wayland session — XWayland).
pub fn live_display() -> bool {
    std::env::var("DISPLAY").is_ok_and(|d| !d.is_empty())
}

/// [`via_xwayland`] for this session and the installed conky (without taking
/// the widget into account).
pub fn live_via_xwayland(io: &dyn LookIo, ctx: &Ctx, needs_x11: bool) -> bool {
    if !ctx.wayland || !live_display() {
        return false;
    }
    let version = io.run(&["conky".to_string(), "-v".to_string()]).1;
    via_xwayland(&ctx.de, ctx.wayland, true, parse_version_full(&version), needs_x11)
}

/// The session through conky's eyes: if it goes through XWayland, then for the layout, screens
/// and caveats this is an X11 session.
pub fn conky_ctx(io: &dyn LookIo, ctx: &Ctx) -> Ctx {
    let mut c = ctx.clone();
    if live_via_xwayland(io, ctx, false) {
        c.wayland = false;
    }
    c
}

/// Whether the widget with the file `conf` draws through X11: a Lua script in the file's folder
/// (and its subfolders down to the second level) calls `cairo_xlib_*`.
pub fn widget_needs_x11(conf: &str) -> bool {
    fn walk(dir: &std::path::Path, depth: usize, budget: &mut usize) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else { return false };
        for e in entries.flatten() {
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            let path = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() && depth < 2 {
                if walk(&path, depth + 1, budget) {
                    return true;
                }
            } else if ft.is_file() && path.extension().is_some_and(|x| x == "lua") {
                let small = e.metadata().is_ok_and(|m| m.len() < (1 << 20));
                if small && std::fs::read_to_string(&path).is_ok_and(|t| t.contains("cairo_xlib_surface_create")) {
                    return true;
                }
            }
        }
        false
    }
    let dir = crate::de_apply::conky_work_dir(conf);
    let mut budget = 400;
    dir != "/" && dir != "." && walk(std::path::Path::new(dir), 0, &mut budget)
}

/// `XDG_CURRENT_DESKTOP` names of desktops without a layer for conky
/// ([`wayland_without_layers`]) — for the autostart script.
pub const XWAYLAND_DESKTOP_NAMES: &[&str] = &["GNOME", "X-Cinnamon", "Cinnamon", "Pantheon"];

/// What switching to XWayland changes in the environment (its marker in the autostart
/// record too).
pub const XWAYLAND_SESSION_TYPE: &str = "XDG_SESSION_TYPE=x11";

/// The autostart script variable with the XWayland launch file
/// ([`xwayland_path`]); empty — conky goes with the file from the arguments.
pub const XWAYLAND_LAUNCH_VAR: &str = "rexon_xw";

/// The start of the autostart `sh` script: the same decision as [`via_xwayland`],
/// but at login — the session and the conky version are found out there. `needs_x11` — the widget
/// draws through X11 (decided when writing). `xwayland_conf` — the launch file
/// via XWayland: on switching it (if any) goes into
/// [`XWAYLAND_LAUNCH_VAR`], the widget window — below windows. The script itself
/// adds no `%` characters (special in `Exec=`; the record escapes the path).
pub fn xwayland_shell(needs_x11: bool, xwayland_conf: Option<&str>) -> String {
    let desks = if needs_x11 {
        "x=1; ".to_string()
    } else {
        let cases: Vec<String> = XWAYLAND_DESKTOP_NAMES.iter().map(|n| format!("*:{n}:*")).collect();
        format!("x=; case \":$XDG_CURRENT_DESKTOP:\" in {}) x=1;; esac; ", cases.join("|"))
    };
    let launch = match xwayland_conf {
        Some(f) => {
            let q = crate::compositor_autostart::shell_quote(f);
            format!(" [ -r {q} ] && {XWAYLAND_LAUNCH_VAR}={q};")
        }
        None => String::new(),
    };
    format!(
        "{desks}case \"$(conky -v 2>/dev/null)\" in *\"conky 1.25.\"[012][!0-9]*) x=1;; esac; \
         [ -n \"$x\" ] && [ -n \"$WAYLAND_DISPLAY\" ] && [ -n \"$DISPLAY\" ] && \
         {{ unset WAYLAND_DISPLAY; export {XWAYLAND_SESSION_TYPE};{launch} }}; "
    )
}

/// Apply the switch to XWayland to a conky launch.
pub fn apply_xwayland(cmd: &mut std::process::Command) {
    cmd.env_remove("WAYLAND_DISPLAY");
    cmd.env("XDG_SESSION_TYPE", "x11");
}

// ── A window on the desktop via XWayland ────────────────────────────────
//
// A conky X11 window under Muffin/Mutter in a Wayland session (Muffin 6.8 sources,
// `compositor.c: get_window_group_for_window`, `window-x11.c:
// get_standalone_layer`; conky 1.25 `output/x11.cc`):
// * `override` — an unmanaged window (override-redirect): the layer
//   META_LAYER_OVERRIDE_REDIRECT, the group top_window_group — ABOVE all
//   windows and panels; conky's `XLowerWindow` means nothing there. conky
//   creates `utility` as override-redirect too;
// * `desktop` — the group bottom_window_group together with the desktop icons window
//   (nemo-desktop): a click on the desktop raises the icons above the widget;
// * `normal` with the hints "undecorated, below all, on all desktops, not in the
//   taskbar" (`_NET_WM_STATE_BELOW` → META_LAYER_BOTTOM): below normal
//   windows, above the icons and wallpaper, takes no focus (`input = False` for an
//   undecorated window), does not get into Alt+Tab.
// So via XWayland conky goes with a file where the window is as Rexon
// advises for X11 ([`window_preset`]): on top of the author's file and the person's choice
// ([`load`]). It does not touch the person's choice; it plays no part in an X11 session.

/// The XWayland launch file for the overlay `p`.
pub fn xwayland_path(p: &Paths) -> String {
    let base = p.overlay.strip_suffix(OVERLAY_SUFFIX).unwrap_or(&p.overlay);
    format!("{base}{XWAYLAND_KEY_TAG}{OVERLAY_SUFFIX}")
}

/// The text of the XWayland launch file: the person's choice `ov` and an "on the
/// desktop" window per Rexon's advice.
pub fn render_xwayland(source: &str, cfg: &BTreeMap<String, Value>, ov: &Overrides, env: &Env) -> String {
    let ov = Overrides { window_preset: Some(true), ..ov.clone() };
    render(source, cfg, &ov, env)
}

/// Write the XWayland launch file for `conf` ([`xwayland_path`]).
/// `None` — the author's file cannot be read or written.
pub fn write_xwayland(io: &dyn LookIo, p: &Paths, conf: &str, env: &Env) -> Option<String> {
    write_xwayland_with(io, p, conf, &load(io, p), env)
}

/// [`write_xwayland`] with the person's choice `ov` (just saved —
/// without rereading it from disk).
fn write_xwayland_with(io: &dyn LookIo, p: &Paths, conf: &str, ov: &Overrides, env: &Env) -> Option<String> {
    let (source, cfg) = source_of(io, conf, p)?;
    let path = xwayland_path(p);
    io.write(&path, &render_xwayland(&source, &cfg, ov, env)).then_some(path)
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

/// Lua window lines per Rexon's advice: a Wayland branch and an X11 branch — the session
/// is found out when conky starts (`WAYLAND_DISPLAY`), so autostart in another
/// session also gets the right window.
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
        // No layers — the window stays as the author set it.
        None => out.push("    -- no layer-shell here: the author's window settings stay".into()),
    }
    out.push("else".into());
    if let Some(p) = &x11 {
        preset_lines(p, env.version, "    ", out);
    }
    out.push("end".into());
}

/// Lua lines for the background.
fn background_lines(cfg: &BTreeMap<String, Value>, ov: &Overrides, env: &Env, out: &mut Vec<String>) {
    let (author_none, author_opacity, author_colour) = author_background(cfg);
    let none = ov.bg_none.unwrap_or(author_none && ov.bg_opacity.is_none());
    let opacity = if none { 0 } else { ov.bg_opacity.unwrap_or(author_opacity) };
    let colour = ov.bg_colour.clone().unwrap_or(author_colour);
    let alpha = alpha_of(opacity);
    out.push("c.own_window = true".into());
    let modern = env.version.is_some_and(|v| v >= ALPHA_COLOUR_SINCE);
    if modern {
        // The alpha is in the color itself; the author's deprecated fields would otherwise argue with
        // it. The field name since 1.2x is `own_window_color`, conky understands the old
        // `own_window_colour` with a warning, but with both at once
        // in the table, which one wins is undefined.
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

/// The overlay text: execute `source` (the author's file or its 1.10 conversion)
/// and override the chosen fields. `cfg` — the author's fields ([`parse_config`];
/// for the background, when the person did not set everything).
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
    // The window and the background both turn on their own window — one line is enough.
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
//  Screen (X11)
// ════════════════════════════════════════════════════════════════════════════
//
// `xinerama_head` is the screen number in the Xinerama list. An X server with RandR 1.5
// builds this list from the RandR monitors in the same order that
// `xrandr --listmonitors` prints (the primary first). On Wayland conky cannot choose
// a screen (one output for everything, open conky issues #2106, #2111) —
// the item is hidden with an explanation.

/// An X11 monitor.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Monitor {
    /// The number for `xinerama_head`.
    pub index: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub primary: bool,
}

/// Monitors from the `xrandr --listmonitors` output
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
        // Position — `+X+Y`, a negative one — `+-200`.
        let nums: Vec<&str> = pos.split('+').filter(|p| !p.is_empty()).collect();
        let (Ok(width), Ok(height)) = (w.parse(), h.parse()) else { continue };
        let x = nums.first().and_then(|n| n.parse().ok()).unwrap_or(0);
        let y = nums.get(1).and_then(|n| n.parse().ok()).unwrap_or(0);
        list.push(Monitor { index, name, width, height, x, y, primary });
    }
    list
}

/// X11 monitors of this session; empty — a Wayland session, a single monitor is not
/// asked about, or there is nothing to find out with (no `xrandr`).
pub fn monitors(io: &dyn LookIo, wayland: bool) -> Vec<Monitor> {
    if wayland || !io.have("xrandr") {
        return Vec::new();
    }
    let (ok, out) = io.run(&["xrandr".to_string(), "--listmonitors".to_string()]);
    if ok { parse_monitors(&out) } else { Vec::new() }
}

/// Why there is no screen choice; `None` — there is one.
pub fn monitor_note(wayland: bool) -> Option<String> {
    wayland.then(|| tr("Conky can't choose a screen in a Wayland session yet — the widget appears where the desktop puts it."))
}

// ════════════════════════════════════════════════════════════════════════════
//  Clock and network in the widget text
// ════════════════════════════════════════════════════════════════════════════
//
// The clock format and the network interface name live in the widget text
// (`${time %H:%M}`, `${downspeed eth0}`), not in `conky.config` fields.
// The overlay changes them on the fly: `conky.text` from the author's file goes through
// a replacement inside `${…}` variables (a test bench measurement: `gsub` in the overlay
// changes the text, the author's file is not touched). Network: each old
// name has its own replacement, in a single pass over the words, so `eth0 → wlan0` and
// `wlan0 → eth0` at once do not merge into one name (in conky manager programs
// both names became one). The widget's Lua scripts (rings with `arg =
// 'eth0'`) are not changed this way — that is the author's code.

/// conky variables whose first argument is a network interface.
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

/// Time variables: the strftime format is their argument.
pub const TIME_VARS: &[&str] = &["time", "utime", "tztime"];

/// The starts of the text's `${…}` variables — `(name, arguments up to a nested
/// variable)`, nested ones too.
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

/// Which clock is in the text: `Some(true)` — 24-hour (`%H`, `%k`, `%R`, `%T`),
/// `Some(false)` — 12 (`%I`, `%l`, `%r`); `None` — no clock in the text.
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

/// Network interface names the text mentions, in order.
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

/// A network interface of the machine.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Iface {
    pub name: String,
    /// `wifi`, `ethernet` or `virtual`.
    pub kind: &'static str,
}

/// Interfaces from `/sys/class/net` (without `lo`): first the real ones — Wi-Fi and
/// wired — then the virtual ones.
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

/// Lua: walking the text's `${…}` variables replacing the start of each one (`f`).
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
//  Storing, launching, applying
// ════════════════════════════════════════════════════════════════════════════

/// The person's choice for the file `conf`; none — empty.
pub fn load(io: &dyn LookIo, p: &Paths) -> Overrides {
    io.read(&p.settings).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// What the overlay executes and the author's fields: `(file for dofile, fields)`.
/// `None` — there is no author's file or its fields cannot be read.
///
/// A 1.9 file is converted into a copy ([`convert_19`]) on every call — the author's file
/// might have been updated.
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

/// Write the choice and the overlay (an empty choice — remove both). `Err` — the reason.
pub fn save(io: &dyn LookIo, p: &Paths, conf: &str, ov: &Overrides, env: &Env) -> Result<(), String> {
    let saved = save_overlay(io, p, conf, ov, env);
    // The XWayland launch file follows the choice: a conky started with it
    // rereads it itself, autostart at login takes the new one.
    if io.read(&xwayland_path(p)).is_some() {
        let _ = write_xwayland_with(io, p, conf, ov, env);
    }
    saved
}

fn save_overlay(io: &dyn LookIo, p: &Paths, conf: &str, ov: &Overrides, env: &Env) -> Result<(), String> {
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

/// Remove the overlay, the choice and the copy.
pub fn remove(p: &Paths) {
    for f in [&p.overlay, &p.settings, &p.converted] {
        let _ = std::fs::remove_file(f);
    }
}

/// Remove the overlays of all files under the widget directory `dir` (the widget
/// is being deleted — its settings are no longer needed).
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

/// Which file to start conky with for `conf`: the overlay if the person changed
/// something (it is rewritten — the author's file might have been updated), otherwise `conf`
/// itself.
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

/// conky processes of the file `conf`: `(pid, started with the overlay)`.
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

/// Restart the processes `pids` of the file `conf`: stop them, wait for them to exit
/// (otherwise a new launch would take the old process for its own) and start.
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

/// Rewrite the autostart records that launch `conf` (with the overlay
/// or without): after "Reset" login must not call the deleted overlay, and
/// after the first edit it must call it. The record's delay and environment
/// are kept, the compositor lines are rewritten together with it.
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
        // The XWayland launch file has already been rewritten following the choice
        // ([`save`]); if it is missing, it is written here.
        let ready = xwayland_path(p);
        let xwayland = if io.read(&ready).is_some() { Some(ready) } else { autostart_xwayland(io, ctx, p, conf) };
        let built = crate::conky_autostart::build(&env, key, title, conf, launch, xwayland.as_deref(), &opts);
        io.write(&path, &built.desktop);
    }
}

/// The XWayland launch file for the autostart record of `conf`: written
/// anew (login to a Wayland GNOME/Cinnamon/Pantheon session takes it, an
/// X11 session does not). `None` — the author's file cannot be read.
pub fn autostart_xwayland(io: &dyn LookIo, ctx: &Ctx, p: &Paths, conf: &str) -> Option<String> {
    let env = Env { version: conky_version(io), de: ctx.de.clone() };
    write_xwayland(io, p, conf, &env)
}

/// What to launch `conf` with at login: the overlay, if there is one.
pub fn autostart_launch(io: &dyn LookIo, p: &Paths) -> Option<String> {
    (!load(io, p).is_empty() && io.read(&p.overlay).is_some()).then(|| p.overlay.clone())
}

/// Apply the person's choice to the file `conf` of the widget `title`: write
/// the overlay; a widget started without it is restarted with it (a conky started with
/// it rereads the overlay itself); autostart — to the overlay.
/// `(succeeded, message)`.
pub fn apply(io: &dyn LookIo, ctx: &Ctx, p: &Paths, title: &str, conf: &str, ov: &Overrides) -> (bool, String) {
    let env = Env { version: conky_version(io), de: ctx.de.clone() };
    if let Err(why) = save(io, p, conf, ov, &env) {
        return (false, why);
    }
    let use_overlay = !ov.is_empty();
    refresh_autostart(io, ctx, p, title, conf, use_overlay);
    let procs = processes_of(io, conf);
    // Without an overlay (a reset) those that ran with it are restarted; with an overlay —
    // those that ran without it.
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

/// "Reset": as the author made it.
pub fn reset(io: &dyn LookIo, ctx: &Ctx, p: &Paths, title: &str, conf: &str) -> (bool, String) {
    apply(io, ctx, p, title, conf, &Overrides::default())
}

// ════════════════════════════════════════════════════════════════════════════
//  For the card: what to show and how to change it
// ════════════════════════════════════════════════════════════════════════════

/// A cell of the alignment grid.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AlignCell {
    pub value: &'static str,
    pub arrow: &'static str,
    pub label: String,
}

/// A drop-down list item.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Choice {
    pub value: serde_json::Value,
    pub label: String,
}

/// A network interface of the widget text and what to replace it with.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct NetRow {
    pub from: String,
    pub to: String,
    pub label: String,
    pub choices: Vec<Choice>,
}

/// Labels of the settings block.
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

/// The label of a screen corner or edge (`top_right` → "Top right").
pub fn align_label(value: &str) -> Option<String> {
    let full = normalize_alignment(value)?;
    align_cells().into_iter().find(|c| c.value == full).map(|c| c.label)
}

/// The widget settings block for the card.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SettingsView {
    pub conf: String,
    /// The settings are available (the file was read and parsed).
    pub available: bool,
    /// Why not.
    pub reason: String,
    pub syntax: Option<Syntax>,
    /// The person changed something (there is an overlay).
    pub changed: bool,
    pub cur: Option<Current>,
    pub alignments: Vec<AlignCell>,
    pub preset_note: Option<String>,
    pub compositor_note: Option<String>,
    pub monitors: Vec<Monitor>,
    pub screen_choices: Vec<Choice>,
    pub monitor_note: Option<String>,
    /// The clock in the text: `Some(whether the author has 24-hour)`; `None` — no clock.
    pub clock: Option<bool>,
    pub clock_24h: bool,
    pub nets: Vec<NetRow>,
    pub t: SettingsTexts,
}

/// What is known about the machine for the settings block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Machine {
    pub monitors: Vec<Monitor>,
    pub interfaces: Vec<Iface>,
    /// A compositor is running on X11 (`None` — cannot find out, or Wayland).
    pub compositor: Option<bool>,
}

/// Assemble the settings block of the file `conf`.
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
    // A transparent background without an X11 compositor is drawn black.
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

/// Change one choice field. `Err` — an unknown field or an invalid value.
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
    if key == "position" {
        return set_position(ov, value);
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
            // The slider is moved so that there is a background: "no background" goes away.
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

/// The `position` field (the screen mini-map, `conky_layout`): alignment,
/// gaps and the screen at once — `{alignment, gap_x, gap_y, xinerama_head}`,
/// `null` — as the author made it, no key — do not touch. One overlay write —
/// conky rereads it once.
fn set_position(ov: &mut Overrides, value: &serde_json::Value) -> Result<(), String> {
    let Some(obj) = value.as_object() else {
        return Err(tr("This value can't be used here."));
    };
    let mut next = ov.clone();
    for key in ["alignment", "gap_x", "gap_y", "xinerama_head"] {
        if let Some(v) = obj.get(key) {
            let v = if key == "xinerama_head" && v.is_null() { &serde_json::json!(-1) } else { v };
            set_value(&mut next, key, v)?;
        }
    }
    *ov = next;
    Ok(())
}

/// Whether the file `conf` belongs to the widget `name` (settings of someone else's file are not
/// written).
fn part_of(io: &dyn LookIo, ctx: &Ctx, name: &str, conf: &str) -> bool {
    crate::conky_procs::parts(io, ctx, name).iter().any(|p| p.conf == conf)
}

/// Change the field `key` of the file `conf` of the widget `name` and apply.
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

/// An X11 compositor is running: the screen has an owner of the `_NET_WM_CM_S<n>` selection
/// (that is how X11 compositors announce themselves). `None` — X11 cannot be opened.
pub fn live_x11_compositor() -> Option<bool> {
    use x11_dl::xlib;
    let lib = xlib::Xlib::open().ok()?;
    // SAFETY: the display is opened and closed right here, the pointers do not
    // leave the function.
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

/// The live settings block.
pub fn live_settings_view(name: &str, conf: &str) -> SettingsView {
    let io = crate::animations::SystemIo;
    // Via XWayland conky is in an X11 session: screens and places — as there.
    let ctx = conky_ctx(&io, &Ctx::live());
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
    // The tile snapshot — with the new settings (`conky_preview`).
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

    #[test]
    fn xwayland_choice() {
        assert_eq!(parse_version_full("conky 1.25.2-pre-6caddca3 compiled\n"), Some((1, 25, 2)));
        assert_eq!(parse_version_full("conky 1.26.0\n"), Some((1, 26, 0)));
        assert_eq!(parse_version_full("conky 1.9\n"), Some((1, 9, 0)));
        let v = |s: &str| parse_version_full(s);
        // Desktops without a layer — via XWayland, if it exists.
        for de in ["gnome", "cinnamon", "pantheon"] {
            assert!(via_xwayland(de, true, true, v("conky 1.26.0"), false), "{de}");
            assert!(!via_xwayland(de, true, false, v("conky 1.26.0"), false), "{de}: без XWayland");
            assert!(!via_xwayland(de, false, true, v("conky 1.26.0"), false), "{de}: сеанс X11");
        }
        // Desktops with layers — their own Wayland output, except 1.25.0–1.25.2 and X11 widgets.
        for de in ["kde", "sway", "hyprland", "niri"] {
            assert!(!via_xwayland(de, true, true, v("conky 1.25.3"), false), "{de}");
            assert!(!via_xwayland(de, true, true, None, false), "{de}: версия неизвестна");
            assert!(via_xwayland(de, true, true, v("conky 1.25.2-pre-6caddca3"), false), "{de}: 1.25.2");
            assert!(via_xwayland(de, true, true, v("conky 1.25.0"), false), "{de}: 1.25.0");
            assert!(via_xwayland(de, true, true, v("conky 1.26.0"), true), "{de}: виджет на X11");
        }
        // The autostart script names — exactly the desktops without a layer.
        for name in XWAYLAND_DESKTOP_NAMES {
            let de = crate::system::detect_current_de(name, "", true).unwrap_or_default();
            assert!(wayland_without_layers(&de), "{name} → {de}");
        }
    }

    /// The autostart script in action: a fake conky prints the version and
    /// the environment it was started with.
    #[test]
    fn xwayland_shell_really_switches() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!("rexon-conky-xw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let run = |version: &str, desk: &str, wl: &str, x11: bool| -> String {
            let fake = tmp.join("conky");
            std::fs::write(
                &fake,
                format!("#!/bin/sh\n[ \"$1\" = -v ] && {{ echo 'conky {version} compiled'; exit 0; }}\necho \"wl=$WAYLAND_DISPLAY type=$XDG_SESSION_TYPE\"\n"),
            )
            .unwrap();
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
            let script = format!("{}exec conky -c x", xwayland_shell(x11, None));
            let out = std::process::Command::new("sh")
                .args(["-c", &script])
                .env("PATH", format!("{}:/usr/bin:/bin", tmp.display()))
                .env("XDG_CURRENT_DESKTOP", desk)
                .env("WAYLAND_DISPLAY", wl)
                .env("DISPLAY", ":7")
                .env("XDG_SESSION_TYPE", "wayland")
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let x = "wl= type=x11";
        let w = "wl=wayland-0 type=wayland";
        let got = [
            run("1.26.0", "X-Cinnamon", "wayland-0", false),
            run("1.26.0", "ubuntu:GNOME", "wayland-0", false),
            run("1.26.0", "KDE", "wayland-0", false),
            run("1.25.2-pre-6caddca3", "KDE", "wayland-0", false),
            run("1.25.20", "KDE", "wayland-0", false),
            run("1.26.0", "KDE", "wayland-0", true),
            run("1.26.0", "X-Cinnamon", "", false),
        ];
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(got, [x, x, w, x, w, x, "wl= type=wayland"], "{got:?}");
        assert!(!xwayland_shell(false, None).contains('%') && !xwayland_shell(true, None).contains('%'));
    }
}
