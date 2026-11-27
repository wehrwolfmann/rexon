//! Translation of the Rexon interface: one gettext catalog for the whole product.
//!
//! WHY. Labels used to be hard-coded in Russian right in the code, and adding
//! Polish or Czech would have meant editing hundreds of places. Now the source
//! language of the strings is English (that is the gettext convention, and any
//! translator will understand them), and every language is one file
//! `po/<code>.po`. Adding a language = putting a `.po` and adding the code to
//! `po/LINGUAS`; the code is not touched.
//!
//! HOW IT WORKS.
//! * Strings in Rust are marked with the functions of this module: [`tr`],
//!   [`trc`] (with a context), [`ntr`] (plural form by number), [`trf`] (with
//!   `%1`, `%2`… substitution) and the [`n_`] marker for tables and constants
//!   where a function cannot be called. Their names are `xgettext` keywords in
//!   `scripts/i18n-update.sh`: renaming a function = re-teaching the script.
//! * Placeholders are in the Qt style (`%1`, `%n`), not `{}`: the same catalog
//!   string goes both here and into QML (`qsTranslate`), and the translator sees
//!   one syntax.
//! * The catalog is read from `.mo` in pure Rust — without libintl and without
//!   `setlocale`: the Rexon language is chosen by its own setting and does not
//!   have to match the process locale.
//! * QML reads THE SAME `.mo`: `qsTranslate("rexon", …)` goes to the window's
//!   translator (a `QTranslator` subclass in
//!   `rexon-gui/cpp/rexon_gui_factory.cpp`), which asks [`qt_translate`]. Rexon
//!   has no second catalog (`.qm`): the translation is stored in one place, and
//!   QML cannot diverge from Rust.
//!
//! LANGUAGE CHOICE: the user setting (`ui_language` in `settings.json`) ▸ the
//! system locale (`LANGUAGE`, then the first non-empty of `LC_ALL`,
//! `LC_MESSAGES`, `LANG`), falling back to the base language (`pt_BR` → `pt`) ▸
//! English. English is the source language, it has no catalog.
//!
//! WHERE CATALOGS ARE LOOKED FOR (the first one found): `$REXON_LOCALEDIR` ▸
//! `locale/` next to the executable (that is where the `cargo build-app` build
//! puts them; it is the development tree path) ▸ `/usr/share/locale` (the
//! package). Inside — the ordinary gettext layout:
//! `<language>/LC_MESSAGES/rexon.mo`.
//!
//! The `.mo` format is per the GNU gettext manual, section "The Format of GNU MO
//! Files"; the `Plural-Forms` expression — ibid., section "Plural forms".

use std::collections::HashMap;
use std::fmt::Display;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

/// Catalog name (gettext domain): `rexon.mo`, `rexon.pot`.
pub const DOMAIN: &str = "rexon";

/// The Qt context the window's translator answers to (see [`qt_translate`]).
/// QML must call `qsTranslate("rexon", "…")`, not `qsTr("…")`: for `qsTr` the
/// context is the file name, and the vendored waywallen in the same process has
/// its own contexts with the same names (`SettingsPage`), so the catalogs would
/// overlap. The translator does not touch strings of foreign contexts.
pub const QT_CONTEXT: &str = "rexon";

/// The source language of the strings. There is no catalog for it and there
/// must not be one.
pub const SOURCE_LANGUAGE: &str = "en";

/// Environment variable with the translation directory for development and checks.
pub const ENV_LOCALEDIR: &str = "REXON_LOCALEDIR";

/// The system translation directory (the package installs there).
pub const SYSTEM_LOCALEDIR: &str = "/usr/share/locale";

/// The language setting key in `settings.json`. Empty or `"system"` — as in the
/// system.
pub const SETTINGS_KEY: &str = "ui_language";

// ============================================================================
// Marking strings
// ============================================================================

/// A marker "this string is translatable" that does nothing by itself.
///
/// For tables and `const`, where [`tr`] cannot be called: `xgettext` will
/// extract the string, and the place that shows it will translate it —
/// `tr(STEP.title)`.
pub const fn n_(msgid: &'static str) -> &'static str {
    msgid
}

/// Translate a string.
pub fn tr(msgid: &str) -> String {
    lookup(None, msgid).unwrap_or_else(|| msgid.to_string())
}

/// Translate a string with a context — when one English word means different
/// things ("Open" — a verb on a button and an adjective in a status).
pub fn trc(context: &str, msgid: &str) -> String {
    lookup(Some(context), msgid).unwrap_or_else(|| msgid.to_string())
}

/// Translate a string and substitute `%1`, `%2`… in order.
pub fn trf(msgid: &str, args: &[&dyn Display]) -> String {
    fill(&tr(msgid), args)
}

/// A string's form by number `n` with `%n` substitution.
///
/// A language has from one to six forms, and which one is needed is decided by
/// the catalog's `Plural-Forms` expression — not by the code. Without a catalog
/// the English rule works: one thing — `singular`, the rest — `plural`.
pub fn ntr(singular: &str, plural: &str, n: u64) -> String {
    let form = catalog()
        .and_then(|c| c.plural_lookup(singular, n))
        .unwrap_or_else(|| if n == 1 { singular } else { plural }.to_string());
    form.replace("%n", &isolate(&n.to_string()))
}

/// Substitute `%1`…`%99` into an already translated template.
///
/// The number is taken as a whole (`%12` is the twelfth, not the first followed
/// by "2"); a foreign number and a lone `%` stay as they are: a visible
/// placeholder is better than a silently missing word.
///
/// With a right-to-left interface language every substitution is wrapped in an
/// isolate ([`isolate`]). Text for a console, a file or a command goes through
/// [`fill_plain`].
pub fn fill(template: &str, args: &[&dyn Display]) -> String {
    fill_with(template, args, ui_is_rtl())
}

/// [`fill`] without isolates — for what is read not by a Qt window: the core's
/// text console (it has no bidirectional writing, and it draws U+2068 as
/// garbage), files, commands the person copies into a terminal.
pub fn fill_plain(template: &str, args: &[&dyn Display]) -> String {
    fill_with(template, args, false)
}

fn fill_with(template: &str, args: &[&dyn Display], isolated: bool) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let digits = bytes[i + 1..]
                .iter()
                .take(2)
                .take_while(|b| b.is_ascii_digit())
                .count();
            if digits > 0 {
                let index: usize = template[i + 1..i + 1 + digits].parse().unwrap_or(0);
                if index >= 1 && index <= args.len() {
                    let value = args[index - 1].to_string();
                    if isolated && !value.is_empty() {
                        out.push(FSI);
                        out.push_str(&value);
                        out.push(PDI);
                    } else {
                        out.push_str(&value);
                    }
                    i += 1 + digits;
                    continue;
                }
            }
        }
        // Byte by byte is not allowed — it is UTF-8; we go to the next `%`. The
        // `%` itself is a single byte, so `i + 1` after it is a character
        // boundary.
        let from = if bytes[i] == b'%' { i + 1 } else { i };
        let next = template[from..].find('%').map_or(bytes.len(), |p| from + p);
        out.push_str(&template[i..next]);
        i = next;
    }
    out
}

// ============================================================================
// Right-to-left writing
// ============================================================================
//
// In Arabic, Hebrew and Persian an interface string is a right-to-left
// sentence, while what is substituted into it (a program name, a path, a
// version, a number) is often written in Latin script. Without markup the
// bidirectional text algorithm (Unicode UAX #9) glues the substitution to
// neighboring punctuation and digits and rearranges them: "Firefox 2.0" with a
// period at the end of the sentence moves to the wrong place, and the direction
// of the whole paragraph is taken from the first strong letter — a Latin one.
// The FIRST STRONG ISOLATE … POP DIRECTIONAL ISOLATE isolate (U+2068 … U+2069)
// makes the substitution a separate piece with its own direction, and for the
// surrounding sentence — a single neutral character.
//
// Qt 6.12 understands isolates (checked by building against the system Qt on
// 06.11.2026): `QString::isRightToLeft` skips the contents of an isolate — the
// phrase "FSI Firefox PDI + Arabic" counts as right-to-left, without the isolate
// as left-to-right; `QTextLayout` lays an isolate out as one piece. The
// alignment of `RexonText` does not depend on the contents at all (an explicit
// `AlignLeft`, flipped by mirroring), so the isolate does not affect it.
//
// With left-to-right writing nothing is added: the ru/en/de/zh_CN strings stay
// byte for byte as before.

/// FIRST STRONG ISOLATE: the start of a substitution with its own direction.
pub const FSI: char = '\u{2068}';
/// POP DIRECTIONAL ISOLATE: the end of the isolate.
pub const PDI: char = '\u{2069}';

/// Languages written right to left (the base code, without a region).
const RTL_LANGUAGES: &[&str] = &["ar", "fa", "he", "iw", "ckb", "ps", "ur", "yi", "dv", "ug"];

/// Whether language `code` is written right to left: `"ar"`, `"fa_IR"`,
/// `"he-IL"`, `"ar_EG.UTF-8"`, `"ur@latin"` (Latin script — no).
pub fn is_rtl_language(code: &str) -> bool {
    if code.contains("@latin") {
        return false;
    }
    let base = code.split(['_', '-', '.', '@']).next().unwrap_or("");
    RTL_LANGUAGES.iter().any(|l| l.eq_ignore_ascii_case(base))
}

/// The current interface language is written right to left.
pub fn ui_is_rtl() -> bool {
    is_rtl_language(&current_language())
}

/// Wrap a substitution in an isolate if the interface is right to left;
/// otherwise — as is. For places where the substitution is not done by
/// [`fill`] (`{label}` in a foreign template, `%n` in [`ntr`]).
pub fn isolate(text: &str) -> String {
    isolate_if(text, ui_is_rtl())
}

// ============================================================================
// Numbers for the person
// ============================================================================

/// A number with `decimals` digits after the decimal separator, as it is written
/// in the interface language: "1.4" in English, "1,4" in Russian and German. One
/// path for the whole product — file sizes, seconds, popularity, rating.
///
/// The decimal separator is the catalog string "decimal separator" (its
/// translator decides). Digits: for Persian — Persian "۰–۹", for the other
/// languages, Arabic included (Wehrwolfmann's decision of 06.11.2026) —
/// ordinary 0–9. `decimals == 0` — an integer without a separator.
pub fn format_decimal(value: f64, decimals: usize) -> String {
    // TRANSLATORS: decimal separator in numbers like "1.4 MiB".
    let sep = trc("decimal separator", ".");
    format_decimal_in(value, decimals, &current_language(), &sep)
}

fn format_decimal_in(value: f64, decimals: usize, language: &str, sep: &str) -> String {
    let plain = format!("{value:.decimals$}");
    let persian = language.split(['_', '-', '.', '@']).next() == Some("fa");
    let mut out = String::with_capacity(plain.len() + 8);
    for c in plain.chars() {
        match c {
            '.' => out.push_str(sep),
            '0'..='9' if persian => {
                out.push(char::from_u32('\u{06F0}' as u32 + (c as u32 - '0' as u32)).unwrap_or(c))
            }
            _ => out.push(c),
        }
    }
    out
}

fn isolate_if(text: &str, rtl: bool) -> String {
    if rtl && !text.is_empty() {
        format!("{FSI}{text}{PDI}")
    } else {
        text.to_string()
    }
}

// ============================================================================
// The .mo catalog
// ============================================================================

/// A parsed catalog of one language.
#[derive(Debug, Clone)]
pub struct Catalog {
    /// The key is the `msgid` (for plural forms — the singular), with the
    /// context joined by `\u{4}`, as `msgfmt` writes it.
    messages: HashMap<String, Vec<String>>,
    plural: Option<PluralExpr>,
    nplurals: usize,
}

/// The `.mo` magic number in the file's byte order.
const MO_MAGIC: u32 = 0x9504_12de;

impl Catalog {
    /// Read a `.mo` from disk.
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse_mo(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Parse a `.mo` from memory. Any corruption is an error, not a panic: the
    /// catalog lies on disk and may have been truncated.
    pub fn parse_mo(data: &[u8]) -> Result<Self, String> {
        let word = |at: usize, big: bool| -> Result<u32, String> {
            let raw: [u8; 4] = data
                .get(at..at + 4)
                .and_then(|s| s.try_into().ok())
                .ok_or_else(|| format!("файл обрывается на смещении {at}"))?;
            Ok(if big { u32::from_be_bytes(raw) } else { u32::from_le_bytes(raw) })
        };
        let big = match word(0, false)? {
            MO_MAGIC => false,
            m if m.swap_bytes() == MO_MAGIC => true,
            _ => return Err("это не каталог .mo (нет магического числа)".into()),
        };
        let count = word(8, big)? as usize;
        let originals = word(12, big)? as usize;
        let translations = word(16, big)? as usize;
        let string_at = |table: usize, index: usize| -> Result<&str, String> {
            let len = word(table + index * 8, big)? as usize;
            let off = word(table + index * 8 + 4, big)? as usize;
            let raw = data
                .get(off..off.checked_add(len).ok_or("переполнение смещения")?)
                .ok_or_else(|| format!("строка {index} выходит за конец файла"))?;
            std::str::from_utf8(raw).map_err(|_| format!("строка {index} не в UTF-8"))
        };

        let mut messages = HashMap::with_capacity(count);
        let mut header = "";
        for index in 0..count {
            let original = string_at(originals, index)?;
            let translated = string_at(translations, index)?;
            if original.is_empty() {
                header = translated;
                continue;
            }
            // For plural forms the original is "singular\0plural", while lookups
            // will be by the singular.
            let key = original.split('\0').next().unwrap_or(original);
            let forms = translated.split('\0').map(str::to_string).collect();
            messages.insert(key.to_string(), forms);
        }

        let (nplurals, plural) = parse_plural_header(header);
        Ok(Self { messages, plural, nplurals })
    }

    /// Translation of a string; `None` — it is not in the catalog (or the
    /// translation is empty).
    pub fn get(&self, context: Option<&str>, msgid: &str) -> Option<&str> {
        let forms = match context {
            Some(ctx) => self.messages.get(&format!("{ctx}\u{4}{msgid}")),
            None => self.messages.get(msgid),
        }?;
        forms.first().map(String::as_str).filter(|s| !s.is_empty())
    }

    /// Form by number; `None` — no string, no form, or it is empty.
    pub fn plural_lookup(&self, singular: &str, n: u64) -> Option<String> {
        let forms = self.messages.get(singular)?;
        forms.get(self.plural_index(n)).filter(|s| !s.is_empty()).cloned()
    }

    /// Form number by number — per the catalog's `Plural-Forms` expression.
    fn plural_index(&self, n: u64) -> usize {
        let index = match &self.plural {
            Some(expr) => expr.eval(n) as usize,
            // No header — the rule of Germanic languages, as in gettext.
            None => usize::from(n != 1),
        };
        if self.nplurals > 0 { index.min(self.nplurals - 1) } else { index }
    }

    /// A translation in the sense of `QTranslator::translate`: what QML gets from
    /// `qsTranslate("rexon", source, disambiguation, n)`. `None` — there is no
    /// translation, and Qt will show the source (substituting `%n` itself).
    ///
    /// The Qt rules are repeated where the QML author expects them:
    /// * disambiguation = `msgctxt`; if there is no entry with such disambiguation,
    ///   the entry without it is taken (gettext has no such fallback, Qt does);
    /// * `n < 0` — "no number": the first form;
    /// * the plural form follows the catalog's `Plural-Forms`, by the same rule as
    ///   [`ntr`] in Rust, not by Qt's own tables.
    ///
    /// Checked on 10.10.2026 against the former `.qm` catalog (the same `.po` via
    /// `lconvert6` + `lrelease6`) on ru and de: all QML strings and the whole
    /// template with its own disambiguations, for strings with `%n` — 15 different
    /// numbers, 0 differences. Only requests that exist neither in QML nor in the
    /// catalog differ, and both times on purpose:
    /// * the entry has one form (no `msgid_plural` in the `.po`), and a number is
    ///   given — here the translation, the `.qm` gave the source (Qt looked for the
    ///   form with the number, while the entry has only one);
    /// * the string exists in the catalog only under ANOTHER disambiguation — here
    ///   the source, while the `.qm` gave that entry's translation (`lrelease` also
    ///   writes a key without disambiguation). A foreign meaning must not be slipped
    ///   in; `xgettext` already puts QML strings without disambiguation into the
    ///   catalog as a separate entry.
    pub fn qt_lookup(&self, disambiguation: Option<&str>, source: &str, n: i32) -> Option<String> {
        let with_context = disambiguation
            .filter(|d| !d.is_empty())
            .and_then(|d| self.messages.get(&format!("{d}\u{4}{source}")));
        let forms = with_context.or_else(|| self.messages.get(source))?;
        let form = match u64::try_from(n) {
            Ok(n) if forms.len() > 1 => forms.get(self.plural_index(n)),
            _ => forms.first(),
        };
        form.filter(|s| !s.is_empty()).cloned()
    }

    /// All "original → translation" pairs (for catalog checks).
    pub fn entries(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.messages.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// How many plural forms the language has (from the catalog header).
    pub fn nplurals(&self) -> usize {
        self.nplurals
    }
}

/// Parse `Plural-Forms: nplurals=N; plural=EXPRESSION;` from the header.
fn parse_plural_header(header: &str) -> (usize, Option<PluralExpr>) {
    let Some(line) = header
        .lines()
        .find_map(|l| l.trim().strip_prefix("Plural-Forms:"))
    else {
        return (2, None);
    };
    let mut nplurals = 2;
    let mut expr = None;
    for part in line.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("nplurals=") {
            nplurals = v.trim().parse().unwrap_or(2);
        } else if let Some(v) = part.strip_prefix("plural=") {
            expr = PluralExpr::parse(v.trim()).ok();
        }
    }
    (nplurals, expr)
}

// ============================================================================
// The Plural-Forms expression
// ============================================================================
//
// This is a subset of C: `n`, integers, parentheses, `?:`, `||`, `&&`,
// comparisons, `+ - * / %` and `!`. Parsing is by recursive descent over C
// precedences.

/// A parsed plural form expression.
#[derive(Debug, Clone, PartialEq)]
pub enum PluralExpr {
    N,
    Num(u64),
    Not(Box<PluralExpr>),
    Bin(Box<PluralExpr>, BinOp, Box<PluralExpr>),
    Cond(Box<PluralExpr>, Box<PluralExpr>, Box<PluralExpr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl PluralExpr {
    /// Parse an expression; a trailing `;` is allowed.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim().trim_end_matches(';');
        let mut p = ExprParser { s: text.as_bytes(), i: 0 };
        let expr = p.ternary()?;
        p.skip_ws();
        if p.i != p.s.len() {
            return Err(format!("лишний хвост выражения на позиции {}", p.i));
        }
        Ok(expr)
    }

    /// Evaluate for `n`. Division by zero gives 0, not a panic: the expression came
    /// from a translator's file.
    pub fn eval(&self, n: u64) -> u64 {
        match self {
            PluralExpr::N => n,
            PluralExpr::Num(v) => *v,
            PluralExpr::Not(e) => u64::from(e.eval(n) == 0),
            PluralExpr::Cond(c, a, b) => {
                if c.eval(n) != 0 {
                    a.eval(n)
                } else {
                    b.eval(n)
                }
            }
            PluralExpr::Bin(a, op, b) => {
                let x = a.eval(n);
                // `||` and `&&` — short-circuiting, as in C.
                match op {
                    BinOp::Or => return u64::from(x != 0 || b.eval(n) != 0),
                    BinOp::And => return u64::from(x != 0 && b.eval(n) != 0),
                    _ => {}
                }
                let y = b.eval(n);
                match op {
                    BinOp::Eq => u64::from(x == y),
                    BinOp::Ne => u64::from(x != y),
                    BinOp::Lt => u64::from(x < y),
                    BinOp::Le => u64::from(x <= y),
                    BinOp::Gt => u64::from(x > y),
                    BinOp::Ge => u64::from(x >= y),
                    BinOp::Add => x.wrapping_add(y),
                    BinOp::Sub => x.wrapping_sub(y),
                    BinOp::Mul => x.wrapping_mul(y),
                    BinOp::Div => x.checked_div(y).unwrap_or(0),
                    BinOp::Rem => x.checked_rem(y).unwrap_or(0),
                    BinOp::Or | BinOp::And => unreachable!(),
                }
            }
        }
    }
}

struct ExprParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl ExprParser<'_> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, token: &str) -> bool {
        self.skip_ws();
        if self.s[self.i..].starts_with(token.as_bytes()) {
            self.i += token.len();
            true
        } else {
            false
        }
    }

    fn ternary(&mut self) -> Result<PluralExpr, String> {
        let cond = self.binary(0)?;
        if self.eat("?") {
            let a = self.ternary()?;
            if !self.eat(":") {
                return Err(format!("ждали «:» на позиции {}", self.i));
            }
            let b = self.ternary()?;
            return Ok(PluralExpr::Cond(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Ok(cond)
    }

    /// C precedence levels from bottom to top. Two-character operators come
    /// before their one-character prefixes (`<=` before `<`).
    const LEVELS: &'static [&'static [(&'static str, BinOp)]] = &[
        &[("||", BinOp::Or)],
        &[("&&", BinOp::And)],
        &[("==", BinOp::Eq), ("!=", BinOp::Ne)],
        &[("<=", BinOp::Le), (">=", BinOp::Ge), ("<", BinOp::Lt), (">", BinOp::Gt)],
        &[("+", BinOp::Add), ("-", BinOp::Sub)],
        &[("*", BinOp::Mul), ("/", BinOp::Div), ("%", BinOp::Rem)],
    ];

    fn binary(&mut self, level: usize) -> Result<PluralExpr, String> {
        if level == Self::LEVELS.len() {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        'outer: loop {
            for (token, op) in Self::LEVELS[level] {
                // `!=` must not be eaten as `!` by the level below, and single `|`/`&`
                // never occur in the expression.
                if self.eat(token) {
                    let right = self.binary(level + 1)?;
                    left = PluralExpr::Bin(Box::new(left), *op, Box::new(right));
                    continue 'outer;
                }
            }
            return Ok(left);
        }
    }

    fn unary(&mut self) -> Result<PluralExpr, String> {
        self.skip_ws();
        if self.s.get(self.i) == Some(&b'!') && self.s.get(self.i + 1) != Some(&b'=') {
            self.i += 1;
            return Ok(PluralExpr::Not(Box::new(self.unary()?)));
        }
        if self.eat("(") {
            let e = self.ternary()?;
            if !self.eat(")") {
                return Err(format!("ждали «)» на позиции {}", self.i));
            }
            return Ok(e);
        }
        if self.eat("n") {
            return Ok(PluralExpr::N);
        }
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
        }
        if start == self.i {
            return Err(format!("непонятный знак на позиции {start}"));
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()
            .and_then(|t| t.parse().ok())
            .map(PluralExpr::Num)
            .ok_or_else(|| "слишком большое число".to_string())
    }
}

// ============================================================================
// Choosing the language
// ============================================================================

/// Language codes for the catalog search, in decreasing order of preference.
///
/// `setting` — the value of the setting (`None`/empty/`"system"` — as in the
/// system), `env` — reading an environment variable (from outside — so that
/// checks do not depend on the run's environment). Each code gives itself and
/// its base language: `pt_BR.UTF-8` → `pt_BR`, `pt`. English (the source) and
/// `C`/`POSIX` cut the list off: there is no point in searching further — the
/// strings are English anyway.
pub fn language_candidates(
    setting: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut raw: Vec<String> = Vec::new();
    match setting.map(str::trim).filter(|s| !s.is_empty() && *s != "system") {
        Some(chosen) => raw.push(chosen.to_string()),
        None => {
            let nonempty = |k: &str| env(k).filter(|v| !v.trim().is_empty());
            let locale = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| nonempty(k));
            // `LANGUAGE` is a list separated by ":"; gettext ignores it with the
            // `C` locale, but we do not: `LANGUAGE=en rexon` is exactly the way
            // to check English that a developer expects.
            if let Some(list) = nonempty("LANGUAGE") {
                raw.extend(list.split(':').map(str::to_string));
            }
            raw.extend(locale);
        }
    }

    let mut out: Vec<String> = Vec::new();
    for item in raw {
        // `ru_RU.UTF-8@euro` → `ru_RU`.
        let code = item.split(['.', '@']).next().unwrap_or("").trim().replace('-', "_");
        if code.is_empty() {
            continue;
        }
        if code == "C" || code == "POSIX" {
            break;
        }
        let base = code.split('_').next().unwrap_or(&code).to_string();
        if base == SOURCE_LANGUAGE {
            break;
        }
        for c in [code.clone(), base] {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

/// Translation directories in search order.
pub fn locale_dirs(env: &dyn Fn(&str) -> Option<String>, exe_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = env(ENV_LOCALEDIR).filter(|d| !d.trim().is_empty()) {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(exe) = exe_dir {
        dirs.push(exe.join("locale"));
    }
    dirs.push(PathBuf::from(SYSTEM_LOCALEDIR));
    dirs
}

/// Path to the catalog file: `<dir>/<language>/LC_MESSAGES/rexon.<ext>`.
pub fn catalog_file(dir: &Path, language: &str, ext: &str) -> PathBuf {
    dir.join(language).join("LC_MESSAGES").join(format!("{DOMAIN}.{ext}"))
}

/// The first language from `candidates` for which a `.mo` was found, and the path
/// to it. `None` — we stay with English.
pub fn resolve(
    candidates: &[String],
    dirs: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> Option<(String, PathBuf)> {
    candidates.iter().find_map(|lang| {
        dirs.iter()
            .map(|d| catalog_file(d, lang, "mo"))
            .find(|p| exists(p))
            .map(|p| (lang.clone(), p))
    })
}

// ============================================================================
// The process's current catalog
// ============================================================================

#[derive(Debug, Default)]
struct Active {
    /// Language code; empty — English.
    language: String,
    mo: Option<PathBuf>,
    catalog: Option<Arc<Catalog>>,
}

fn state() -> &'static RwLock<Active> {
    static STATE: OnceLock<RwLock<Active>> = OnceLock::new();
    STATE.get_or_init(|| {
        // A lazy choice — by the environment only, WITHOUT reading the
        // settings: checks and the Python bridge call `tr` without `init`, and
        // they have no reason to read a person's live profile. [`init`] takes
        // the setting into account.
        //
        // The library's own checks (`cargo test --lib`) start with the SOURCE
        // language rather than the machine's locale: otherwise a text check
        // went red for someone with `LANG=ru_RU` and an installed catalog (the
        // package in /usr/share/locale or `REXON_LOCALEDIR`) and green for
        // everyone else.
        //
        // The same for the checks from `tests/` (each is its own binary, and the
        // library is built WITHOUT `cfg(test)`): on 15.11.2026 at the owner's
        // (LANG=ru_RU, the package put the catalog into /usr/share/locale) the
        // checks comparing against an English literal got Russian strings.
        // Pinning the language in each of a hundred checks is a trap for the
        // next one; so there is a single decision here as well. A check that
        // needs Russian still asks for it explicitly (`tests/i18n_pin::ru()` or
        // `set_language(Some("ru"))`).
        if cfg!(test) || exe_is_test_harness(std::env::current_exe().ok().as_deref()) {
            return RwLock::new(Active::default());
        }
        RwLock::new(activate(None))
    })
}

/// The binary is a cargo check (`cargo test`/`cargo bench`), not a program.
///
/// cargo names a check binary `<name>-<hash>` (16 hexadecimal digits) and puts
/// it: before cargo 1.100 — into `<profile>/deps/`, since 1.100 (the new build
/// directory layout) — into `<profile>/build/<package>/<hash>/out/`, where the
/// directory above `out` is named with the same hash. Programs
/// (`rexon-main-window`, the core's helpers) cargo puts into `<profile>/`
/// without a hash, and the package installs them into `/usr/lib/rexon` and
/// `/usr/bin` — they do not match the sign. The checks in `tests/` have no
/// build sign (`cfg`): cargo builds the library for them as an ordinary one —
/// so we look at the launch location.
fn exe_is_test_harness(exe: Option<&Path>) -> bool {
    fn name(p: Option<&Path>) -> Option<&str> {
        p.and_then(Path::file_name).and_then(|n| n.to_str())
    }
    let Some(exe) = exe else { return false };
    let Some((_, hash)) = name(Some(exe)).and_then(|n| n.rsplit_once('-')) else {
        return false;
    };
    if hash.len() != 16 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let parent = exe.parent();
    match name(parent) {
        Some("deps") => true,
        Some("out") => name(parent.and_then(Path::parent)) == Some(hash),
        _ => false,
    }
}

/// Process environment for choosing the language. The locale variables are the
/// ORIGINAL ones, captured before the window substituted `LANGUAGE`
/// ([`original_locale_env`]).
fn process_env(key: &str) -> Option<String> {
    if let Some((_, value)) = original_locale_env().iter().find(|(k, _)| *k == key) {
        return value.clone();
    }
    std::env::var(key).ok()
}

// ============================================================================
// Original locale variables
// ============================================================================
//
// A live case on 08.10.2026. The window sets `LANGUAGE=<Rexon language>`
// (without it the standard Qt buttons under Plasma speak the system language),
// while the "as in the system" language choice read the same `LANGUAGE` —
// already substituted. At the owner's (system ru_RU, LC_* formats = de_DE, "de"
// chosen) the list item said «Wie im System (Deutsch)», and a restart after
// choosing "as in the system" brought German back: the new process inherited
// the substituted environment.
//
// Now the original values are captured ONCE, before the substitution, and the
// "system" language is computed only from them. The original `LANGUAGE`
// travels to Rexon's child processes in the [`ORIGINAL_LANGUAGE_ENV`] variable,
// and a restart restores the language environment as it was
// ([`restore_original_locale_env`]). Formats (`LC_NUMERIC`, `LC_TIME`…) do not
// affect the interface language and are not part of the snapshot.

/// Locale variables by which the language is chosen — in gettext order.
pub const LOCALE_ENV_KEYS: [&str; 4] = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"];

/// The original `LANGUAGE` of the Rexon parent process that substituted its own:
/// empty — there was no variable. Set by [`override_ui_language_env`].
pub const ORIGINAL_LANGUAGE_ENV: &str = "REXON_ORIGINAL_LANGUAGE";

/// Snapshot of the original locale variables from environment `env`. If `env`
/// carries [`ORIGINAL_LANGUAGE_ENV`] (the process was started by Rexon with a
/// substituted `LANGUAGE`), that is considered the original.
pub fn snapshot_locale_env(
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<(&'static str, Option<String>)> {
    LOCALE_ENV_KEYS
        .iter()
        .map(|&key| {
            let value = if key == "LANGUAGE" {
                match env(ORIGINAL_LANGUAGE_ENV) {
                    Some(orig) => Some(orig).filter(|v| !v.is_empty()),
                    None => env(key),
                }
            } else {
                env(key)
            };
            (key, value)
        })
        .collect()
}

/// The original locale variables of this process (captured on first access — in
/// the window this is `i18n::init`, before `LANGUAGE` is substituted).
pub fn original_locale_env() -> &'static [(&'static str, Option<String>)] {
    static ORIGINAL: OnceLock<Vec<(&'static str, Option<String>)>> = OnceLock::new();
    ORIGINAL.get_or_init(|| snapshot_locale_env(&|k| std::env::var(k).ok()))
}

/// Substitute the process's `LANGUAGE` with Rexon language `value`, remembering
/// the original for children in [`ORIGINAL_LANGUAGE_ENV`].
///
/// Warning: call before threads are created (it changes the process environment).
pub fn override_ui_language_env(value: &str) {
    let original = original_locale_env()
        .iter()
        .find(|(k, _)| *k == "LANGUAGE")
        .and_then(|(_, v)| v.clone())
        .unwrap_or_default();
    unsafe {
        std::env::set_var(ORIGINAL_LANGUAGE_ENV, original);
        std::env::set_var("LANGUAGE", value);
    }
}

/// Give a child command back the language environment this process was started
/// with: a new Rexon (a restart) chooses the language itself, by the setting
/// and the ORIGINAL locale, and a substituted `LANGUAGE` only gets in its way.
pub fn restore_original_locale_env(cmd: &mut std::process::Command) {
    for (key, value) in original_locale_env() {
        match value {
            Some(v) => cmd.env(key, v),
            None => cmd.env_remove(key),
        };
    }
    cmd.env_remove(ORIGINAL_LANGUAGE_ENV);
}

/// A log line about the language variables: the original and the current ones.
pub fn locale_env_report() -> String {
    let show = |v: &Option<String>| v.clone().unwrap_or_else(|| "—".to_string());
    let orig: Vec<String> =
        original_locale_env().iter().map(|(k, v)| format!("{k}={}", show(v))).collect();
    let now: Vec<String> = LOCALE_ENV_KEYS
        .iter()
        .map(|k| format!("{k}={}", show(&std::env::var(k).ok())))
        .collect();
    format!("исходные [{}], сейчас [{}]", orig.join(" "), now.join(" "))
}

/// The `.mo` catalog of the current language; `None` — English.
pub fn mo_path() -> Option<PathBuf> {
    state().read().ok()?.mo.clone()
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

fn activate(setting: Option<&str>) -> Active {
    let candidates = language_candidates(setting, &process_env);
    let dirs = locale_dirs(&process_env, exe_dir().as_deref());
    let Some((language, mo)) = resolve(&candidates, &dirs, &|p| p.is_file()) else {
        return Active::default();
    };
    match Catalog::load(&mo) {
        Ok(catalog) => Active { language, mo: Some(mo), catalog: Some(Arc::new(catalog)) },
        Err(e) => {
            crate::log_warn!(
                "lang",
                "каталог {} не прочитан, интерфейс будет на английском: {e}",
                mo.display()
            );
            Active::default()
        }
    }
}

fn catalog() -> Option<Arc<Catalog>> {
    state().read().ok()?.catalog.clone()
}

fn lookup(context: Option<&str>, msgid: &str) -> Option<String> {
    catalog()?.get(context, msgid).map(str::to_string)
}

/// Translation of a QML string by the current catalog — the door of the window's
/// translator (`rexon_gui::install_app_translator`). The rules are
/// [`Catalog::qt_lookup`].
///
/// Called from ANY thread: Qt also translates in the QML loading thread. The
/// catalog is read under an `RwLock`, without a write lock — the GUI thread does
/// not wait.
pub fn qt_translate(disambiguation: Option<&str>, source: &str, n: i32) -> Option<String> {
    catalog()?.qt_lookup(disambiguation, source, n)
}

/// Choose the language taking the user setting into account. The application
/// entry point calls this before the first window. Returns the code of the
/// chosen language (`"en"` — the source).
pub fn init() -> String {
    let settings = crate::settings::load_settings(&crate::settings::settings_file());
    let setting = settings.get(SETTINGS_KEY).and_then(|v| v.as_str()).map(str::to_string);
    set_language(setting.as_deref())
}

/// Switch the language: a code (`"ru"`, `"pt_BR"`), `"en"` — the source, `None`
/// or `"system"` — as in the system. Returns what came out.
///
/// Strings already shown will not update by themselves: after this the Qt side
/// has to reinstall the window's translator (it has a string cache), call the
/// engine's `retranslate()` and rebuild the page models.
pub fn set_language(language: Option<&str>) -> String {
    let next = activate(language);
    let code = current_code(&next);
    if let Ok(mut guard) = state().write() {
        *guard = next;
    }
    code
}

fn current_code(active: &Active) -> String {
    if active.language.is_empty() {
        SOURCE_LANGUAGE.to_string()
    } else {
        active.language.clone()
    }
}

/// Code of the current language (`"en"` — the source, no catalog).
pub fn current_language() -> String {
    state().read().map(|a| current_code(&a)).unwrap_or_else(|_| SOURCE_LANGUAGE.into())
}

// ============================================================================
// Language choice by the person: the list, labels, the setting
// ============================================================================
//
// The list of languages in "Settings" is built from `po/LINGUAS` — the same
// file from which the build assembles the catalogs. Add a `pl` line to LINGUAS
// and put `po/pl.po` — Polish appears in the list by itself, the code is not
// touched (rule #12). Only languages whose catalog was actually found are
// shown: an item that translates nothing would be a deception.

/// Contents of `po/LINGUAS` at build time.
pub const LINGUAS: &str = include_str!("../../../po/LINGUAS");

/// The value of the "as in the system" setting.
pub const SYSTEM_SETTING: &str = "system";

/// Language codes from the `LINGUAS` text: separated by spaces and line breaks,
/// `#` — a comment until the end of the line (the same way
/// `scripts/i18n-update.sh` reads it). Duplicates are removed, the order is as in
/// the file.
pub fn parse_linguas(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        for code in line.split_whitespace() {
            if !out.iter().any(|c| c == code) {
                out.push(code.to_string());
            }
        }
    }
    out
}

/// The language's own name (endonym) — the way a native speaker looks for it in
/// the list, not a translation into the interface language: a person who
/// accidentally switched on a foreign language must find their own without
/// reading a foreign one.
///
/// The table covers all languages of the plan in advance (the owner's decision
/// of 29.09: 36 languages), so that a new language appears in the list without
/// editing the code. A code that is not here is labeled with the code itself —
/// visible, but not empty.
pub fn native_name(code: &str) -> String {
    const NAMES: &[(&str, &str)] = &[
        ("en", "English"),
        ("ru", "Русский"),
        ("ar", "العربية"),
        ("be", "Беларуская"),
        ("bg", "Български"),
        ("cs", "Čeština"),
        ("da", "Dansk"),
        ("de", "Deutsch"),
        ("el", "Ελληνικά"),
        ("es", "Español"),
        ("fa", "فارسی"),
        ("fi", "Suomi"),
        ("fil", "Filipino"),
        ("fr", "Français"),
        ("he", "עברית"),
        ("hi", "हिन्दी"),
        ("hu", "Magyar"),
        ("hy", "Հայերեն"),
        ("id", "Bahasa Indonesia"),
        ("it", "Italiano"),
        ("ja", "日本語"),
        ("ka", "ქართული"),
        ("kk", "Қазақ тілі"),
        ("ko", "한국어"),
        ("ms", "Bahasa Melayu"),
        ("nb", "Norsk bokmål"),
        ("nl", "Nederlands"),
        ("pl", "Polski"),
        ("pt", "Português"),
        ("pt_BR", "Português (Brasil)"),
        ("ro", "Română"),
        ("sk", "Slovenčina"),
        ("sr", "Српски"),
        ("sv", "Svenska"),
        ("th", "ไทย"),
        ("tr", "Türkçe"),
        ("uk", "Українська"),
        ("vi", "Tiếng Việt"),
        ("zh_CN", "简体中文"),
    ];
    let code = code.trim();
    NAMES
        .iter()
        .find(|(c, _)| *c == code)
        .or_else(|| {
            // `ru_RU` → «Русский»: the country adds nothing to the language name if
            // there is no separate translation for it.
            let base = code.split(['_', '-', '.', '@']).next().unwrap_or(code);
            NAMES.iter().find(|(c, _)| *c == base)
        })
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| code.to_string())
}

/// Languages that can be chosen: English (the source, no catalog needed) and
/// every language from `linguas` for which a `.mo` was found.
///
/// `exists` — from outside, so that the check does not depend on the run's disk.
pub fn selectable_languages(
    linguas: &[String],
    dirs: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<String> {
    let mut out = vec![SOURCE_LANGUAGE.to_string()];
    for code in linguas {
        if code == SOURCE_LANGUAGE || out.contains(code) {
            continue;
        }
        if dirs.iter().any(|d| exists(&catalog_file(d, code, "mo"))) {
            out.push(code.clone());
        }
    }
    out
}

/// Languages for the list in "Settings" on this machine (see
/// [`selectable_languages`]).
pub fn available_languages() -> Vec<String> {
    let dirs = locale_dirs(&process_env, exe_dir().as_deref());
    selectable_languages(&parse_linguas(LINGUAS), &dirs, &|p| p.is_file())
}

/// Translation of a string into every language from `linguas` whose `.mo` was
/// found in `dirs` — without changing the process language. Duplicates are
/// removed, the order is as in `linguas`.
///
/// WHY. Some things Rexon writes into FOREIGN files in the interface language
/// (the name of a KWin rule is visible in KDE's "Window Rules"). The person may
/// have changed the language after writing, and their previous entry has to be
/// found by any of the translations, not only the current one.
pub fn translations_in(msgid: &str, linguas: &[String], dirs: &[PathBuf]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for code in linguas {
        let Some(mo) = dirs.iter().map(|d| catalog_file(d, code, "mo")).find(|p| p.is_file())
        else {
            continue;
        };
        let Ok(catalog) = Catalog::load(&mo) else { continue };
        if let Some(text) = catalog.get(None, msgid) {
            if !out.iter().any(|t| t == text) {
                out.push(text.to_string());
            }
        }
    }
    out
}

/// [`translations_in`] over all `po/LINGUAS` languages installed on this machine.
pub fn translations_everywhere(msgid: &str) -> Vec<String> {
    let dirs = locale_dirs(&process_env, exe_dir().as_deref());
    translations_in(msgid, &parse_linguas(LINGUAS), &dirs)
}

/// What language this setting would give — without loading the catalog.
/// `None`/`"system"` — as in the system. The answer is a code (`"en"`, `"ru"`).
pub fn language_for_setting(setting: Option<&str>) -> String {
    let dirs = locale_dirs(&process_env, exe_dir().as_deref());
    resolve_setting(setting, &process_env, &dirs, &|p| p.is_file())
}

/// The pure part of [`language_for_setting`]: the environment and the disk come
/// from outside. A language without a catalog gives English, not emptiness.
pub fn resolve_setting(
    setting: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
    dirs: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> String {
    resolve(&language_candidates(setting, env), dirs, exists)
        .map(|(lang, _)| lang)
        .unwrap_or_else(|| SOURCE_LANGUAGE.to_string())
}

/// The person's saved choice: `"system"`, `"en"`, `"ru"`… Empty and a missing
/// key — `"system"`.
pub fn saved_setting() -> String {
    let settings = crate::settings::load_settings(&crate::settings::settings_file());
    normalize_setting(settings.get(SETTINGS_KEY).and_then(|v| v.as_str()))
}

/// Bring the setting value to the list's form: empty → `"system"`.
pub fn normalize_setting(value: Option<&str>) -> String {
    match value.map(str::trim) {
        None | Some("") => SYSTEM_SETTING.to_string(),
        Some(v) => v.to_string(),
    }
}

/// Whether a restart is needed for the saved choice to take effect: the language
/// it gives does not match this process's language.
///
/// The language deliberately does not change on the fly: the tabs' strings are
/// assembled in Rust when the models are built, and a half-translated window is
/// worse than an honest "after a restart". A process keeps its language until it
/// exits.
pub fn restart_needed(setting: &str) -> bool {
    language_for_setting(Some(setting)) != current_language()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `.mo` in memory — like `msgfmt`, without the hash table (it is
    /// optional, and the reader does not use it).
    pub(crate) fn build_mo(entries: &[(&str, &str)], big_endian: bool) -> Vec<u8> {
        let mut sorted: Vec<_> = entries.to_vec();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        let n = sorted.len();
        let header_len = 28;
        let orig_table = header_len;
        let trans_table = orig_table + n * 8;
        let mut strings_at = trans_table + n * 8;
        let mut tables = Vec::new();
        let mut blob = Vec::new();
        let put = |v: u32, out: &mut Vec<u8>| {
            out.extend_from_slice(&if big_endian { v.to_be_bytes() } else { v.to_le_bytes() })
        };
        let mut orig = Vec::new();
        let mut trans = Vec::new();
        for (o, _) in &sorted {
            orig.push((o.len(), strings_at));
            blob.extend_from_slice(o.as_bytes());
            blob.push(0);
            strings_at += o.len() + 1;
        }
        for (_, t) in &sorted {
            trans.push((t.len(), strings_at));
            blob.extend_from_slice(t.as_bytes());
            blob.push(0);
            strings_at += t.len() + 1;
        }
        let mut out = Vec::new();
        for v in [MO_MAGIC, 0, n as u32, orig_table as u32, trans_table as u32, 0, 0] {
            put(v, &mut out);
        }
        for (len, off) in orig.iter().chain(trans.iter()) {
            put(*len as u32, &mut tables);
            put(*off as u32, &mut tables);
        }
        out.extend(tables);
        out.extend(blob);
        out
    }

    const RU_PLURAL: &str = "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && \
                             n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);";

    fn ru_catalog(big: bool) -> Catalog {
        let header = format!("Content-Type: text/plain; charset=UTF-8\nPlural-Forms: {RU_PLURAL}\n");
        let mo = build_mo(
            &[
                ("", &header),
                ("Next", "Далее"),
                ("verb\u{4}Open", "Открыть"),
                ("%n file\0%n files", "%n файл\0%n файла\0%n файлов"),
                ("Untranslated", ""),
            ],
            big,
        );
        Catalog::parse_mo(&mo).expect("каталог разбирается")
    }

    #[test]
    fn mo_reader_both_byte_orders() {
        for big in [false, true] {
            let c = ru_catalog(big);
            assert_eq!(c.get(None, "Next"), Some("Далее"));
            assert_eq!(c.get(Some("verb"), "Open"), Some("Открыть"));
            assert_eq!(c.get(None, "Open"), None, "контекст не теряется");
            assert_eq!(c.get(None, "Untranslated"), None, "пустой перевод — нет перевода");
            assert_eq!(c.nplurals(), 3);
        }
    }

    #[test]
    fn russian_plural_forms() {
        let c = ru_catalog(false);
        let got: Vec<_> = [0, 1, 2, 4, 5, 11, 12, 21, 22, 25, 101, 111]
            .iter()
            .map(|&n| c.plural_lookup("%n file", n).unwrap().replace("%n", &n.to_string()))
            .collect();
        assert_eq!(
            got,
            [
                "0 файлов", "1 файл", "2 файла", "4 файла", "5 файлов", "11 файлов",
                "12 файлов", "21 файл", "22 файла", "25 файлов", "101 файл", "111 файлов"
            ]
        );
    }

    /// The `qsTranslate` rules on top of `.mo` (the check against the former `.qm`
    /// is in [`Catalog::qt_lookup`]).
    #[test]
    fn qt_lookup_follows_qtranslator_rules() {
        let c = ru_catalog(false);
        let t = |d: Option<&str>, s: &str, n: i32| c.qt_lookup(d, s, n);
        assert_eq!(t(None, "Next", -1).as_deref(), Some("Далее"));
        // An empty disambiguation is the same as its absence.
        assert_eq!(t(Some(""), "Next", -1).as_deref(), Some("Далее"));
        assert_eq!(t(Some("verb"), "Open", -1).as_deref(), Some("Открыть"));
        // There is no entry without disambiguation — an entry under another
        // disambiguation is not substituted (the former `.qm` substituted it, see
        // `qt_lookup`).
        assert_eq!(t(None, "Open", -1), None);
        // A foreign disambiguation — fall back to the entry without it, as QTranslator does.
        assert_eq!(t(Some("no such"), "Next", -1).as_deref(), Some("Далее"));
        // The entry has one form — it serves any number.
        assert_eq!(t(None, "Next", 5).as_deref(), Some("Далее"));
        // Plural forms follow the catalog's Plural-Forms; Qt substitutes `%n`.
        assert_eq!(t(None, "%n file", 1).as_deref(), Some("%n файл"));
        assert_eq!(t(None, "%n file", 3).as_deref(), Some("%n файла"));
        assert_eq!(t(None, "%n file", 11).as_deref(), Some("%n файлов"));
        assert_eq!(t(None, "%n file", -1).as_deref(), Some("%n файл"));
        // An empty translation and an unknown string — the source (None).
        assert_eq!(t(None, "Untranslated", -1), None);
        assert_eq!(t(None, "Nowhere", -1), None);
    }

    #[test]
    fn broken_mo_is_an_error_not_a_panic() {
        assert!(Catalog::parse_mo(b"").is_err());
        assert!(Catalog::parse_mo(b"not a catalog at all, clearly").is_err());
        let mut mo = build_mo(&[("Next", "Далее")], false);
        mo.truncate(mo.len() - 4);
        assert!(Catalog::parse_mo(&mo).is_err());
    }

    #[test]
    fn plural_expressions_of_target_languages() {
        // The expressions come from the catalog headers of these languages in GNU gettext.
        let cases: &[(&str, &[(u64, u64)])] = &[
            ("n != 1", &[(0, 1), (1, 0), (2, 1)]),
            ("n > 1", &[(0, 0), (1, 0), (2, 1)]),
            ("(n==1) ? 0 : (n>=2 && n<=4) ? 1 : 2", &[(1, 0), (3, 1), (5, 2), (0, 2)]),
            (
                "(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<12 || n%100>14) ? 1 : 2)",
                &[(1, 0), (2, 1), (5, 2), (12, 2), (22, 1), (112, 2)],
            ),
            ("0", &[(0, 0), (7, 0)]),
            ("!(n == 1)", &[(1, 0), (3, 1)]),
        ];
        for (text, expected) in cases {
            let e = PluralExpr::parse(text).unwrap_or_else(|err| panic!("{text}: {err}"));
            for (n, want) in *expected {
                assert_eq!(e.eval(*n), *want, "{text} при n={n}");
            }
        }
        assert!(PluralExpr::parse("n %").is_err());
        assert!(PluralExpr::parse("(n == 1").is_err());
        assert_eq!(PluralExpr::parse("n / 0").unwrap().eval(3), 0);
    }

    #[test]
    fn fill_placeholders() {
        assert_eq!(fill("Step %1 of %2", &[&3, &8]), "Step 3 of 8");
        assert_eq!(fill("%2 — %1", &[&"a", &"b"]), "b — a");
        assert_eq!(fill("Шаг %1 из %2", &[&3, &8]), "Шаг 3 из 8");
        assert_eq!(fill("100% done, %3 left", &[&1]), "100% done, %3 left");
        assert_eq!(fill("%12", &(1..=12).map(|_| &0 as &dyn Display).collect::<Vec<_>>()), "0");
        assert_eq!(fill("", &[]), "");
    }

    #[test]
    fn rtl_languages_by_code() {
        for code in ["ar", "fa", "he", "ar_EG", "fa_IR.UTF-8", "he-IL", "AR", "ur", "iw"] {
            assert!(is_rtl_language(code), "{code}");
        }
        for code in ["", "en", "ru", "de", "zh_CN", "pt_BR", "ur@latin", "arn", "hr", "fr_CA"] {
            assert!(!is_rtl_language(code), "{code}");
        }
    }

    #[test]
    fn decimals_follow_the_language() {
        assert_eq!(format_decimal_in(1.44, 1, "en", "."), "1.4");
        assert_eq!(format_decimal_in(1.45, 1, "ru", ","), "1,4"); // like `{:.1}`: 1.45 as a double is slightly less
        assert_eq!(format_decimal_in(12.0, 2, "de", ","), "12,00");
        assert_eq!(format_decimal_in(3.6, 0, "ru", ","), "4");
        assert_eq!(format_decimal_in(1.4, 1, "zh_CN", "."), "1.4");
        assert_eq!(format_decimal_in(1.4, 1, "ar", "٫"), "1٫4");
        assert_eq!(format_decimal_in(10.25, 2, "fa_IR", "٫"), "۱۰٫۲۵");
        assert_eq!(format_decimal_in(-0.5, 1, "fa", "٫"), "-۰٫۵");
        // Without a catalog (the library checks run in the source language) — a dot.
        assert_eq!(format_decimal(2.25, 1), format!("{:.1}", 2.25));
    }

    #[test]
    fn fill_isolates_substitutions_only_for_rtl() {
        // Left to right — byte for byte as before, not a single invisible character.
        let ltr = fill_with("Installed %1 (%2).", &[&"Firefox", &"2.0"], false);
        assert_eq!(ltr, "Installed Firefox (2.0).");
        assert_eq!(fill_plain("%1 — %2", &[&"a", &"b"]), "a — b");
        // Right to left — each substitution in its own isolate, the template untouched.
        let rtl = fill_with("تم تثبيت %1 (%2).", &[&"Firefox", &"2.0"], true);
        assert_eq!(rtl, "تم تثبيت \u{2068}Firefox\u{2069} (\u{2068}2.0\u{2069}).");
        // An empty substitution and a foreign number — without an isolate.
        assert_eq!(fill_with("%1|%3", &[&""], true), "|%3");
        assert_eq!(isolate_if("x", true), "\u{2068}x\u{2069}");
        assert_eq!(isolate_if("x", false), "x");
        assert_eq!(isolate_if("", true), "");
        // The library checks run in the source language: a plain `fill` — without
        // isolates (`ntr` is not called here: xgettext would carry the check's
        // string off into the catalog).
        assert_eq!(fill("%1", &[&"a"]), "a");
    }

    #[test]
    fn test_harness_is_told_from_programs() {
        let yes = |p: &str| exe_is_test_harness(Some(Path::new(p)));
        assert!(yes("/r/target-wt/debug/deps/gnome_ego-f98ca29bd4e205d1"));
        assert!(yes("/r/target/x86_64-unknown-linux-gnu/release/deps/rexon_core-0123456789abcdef"));
        // The cargo 1.100 layout: `build/<package>/<hash>/out/<name>-<hash>`.
        assert!(yes("/r/target-wt/debug/build/rexon-core/3ebdb07b8fb945e3/out/rexon_core-3ebdb07b8fb945e3"));
        assert!(!yes("/r/target-wt/debug/build/rexon-core/3ebdb07b8fb945e3/out/rexon_core-0123456789abcdef"));
        // Programs — in the profile directory and in the package.
        assert!(!yes("/r/target/debug/rexon-main-window"));
        assert!(!yes("/usr/lib/rexon/rexon-wallpaper-host"));
        assert!(!yes("/usr/bin/rexon"));
        // A `deps` directory without a hash in the name is not a check.
        assert!(!yes("/opt/deps/rexon-main-window"));
        assert!(!exe_is_test_harness(None));
        // This check binary itself is recognized.
        assert!(exe_is_test_harness(std::env::current_exe().ok().as_deref()));
    }

    #[test]
    fn language_candidates_from_environment() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert_eq!(language_candidates(None, &env(&[("LANG", "ru_RU.UTF-8")])), ["ru_RU", "ru"]);
        assert_eq!(
            language_candidates(None, &env(&[("LANG", "ru_RU.UTF-8"), ("LC_ALL", "pt_BR.UTF-8")])),
            ["pt_BR", "pt"]
        );
        assert_eq!(
            language_candidates(None, &env(&[("LANGUAGE", "cs:pl"), ("LANG", "de_DE.UTF-8")])),
            ["cs", "pl", "de_DE", "de"]
        );
        // English and C cut the search off: `LANGUAGE=en` is the English check.
        assert!(language_candidates(None, &env(&[("LANGUAGE", "en"), ("LANG", "ru_RU.UTF-8")]))
            .is_empty());
        assert!(language_candidates(None, &env(&[("LANG", "C.UTF-8")])).is_empty());
        assert!(language_candidates(None, &env(&[])).is_empty());
        // The setting is stronger than the environment; "system" means the environment.
        assert_eq!(language_candidates(Some("pl"), &env(&[("LANG", "ru_RU.UTF-8")])), ["pl"]);
        assert_eq!(
            language_candidates(Some("system"), &env(&[("LANG", "ru_RU.UTF-8")])),
            ["ru_RU", "ru"]
        );
        assert!(language_candidates(Some("en"), &env(&[("LANG", "ru_RU.UTF-8")])).is_empty());
    }

    /// A live case on 08.10.2026: system ru, formats de, de chosen — the window
    /// substituted `LANGUAGE=de`, but "as in the system" must remain Russian.
    #[test]
    fn системный_язык_считается_по_исходной_локали() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        // A Rexon child process: LANGUAGE is substituted, the original is in the marker.
        let child = env(&[
            ("LANG", "ru_RU.UTF-8"),
            ("LANGUAGE", "de"),
            ("LC_NUMERIC", "de_DE.UTF-8"),
            (ORIGINAL_LANGUAGE_ENV, "ru"),
        ]);
        let snap = snapshot_locale_env(&child);
        let lookup = |k: &str| snap.iter().find(|(n, _)| *n == k).and_then(|(_, v)| v.clone());
        assert_eq!(lookup("LANGUAGE").as_deref(), Some("ru"));
        assert_eq!(language_candidates(None, &lookup), ["ru", "ru_RU"]);
        // The marker is empty — there was no original LANGUAGE at all.
        let unset = env(&[("LANG", "ru_RU.UTF-8"), ("LANGUAGE", "de"), (ORIGINAL_LANGUAGE_ENV, "")]);
        let snap = snapshot_locale_env(&unset);
        let lookup = |k: &str| snap.iter().find(|(n, _)| *n == k).and_then(|(_, v)| v.clone());
        assert_eq!(lookup("LANGUAGE"), None);
        assert_eq!(language_candidates(None, &lookup), ["ru_RU", "ru"]);
        // Formats (LC_NUMERIC) do not affect the language.
        let formats = env(&[("LANG", "ru_RU.UTF-8"), ("LC_NUMERIC", "de_DE.UTF-8"), ("LC_TIME", "de_DE.UTF-8")]);
        assert_eq!(language_candidates(None, &formats), ["ru_RU", "ru"]);
        // Without a marker (an ordinary launch) — as is.
        let plain = env(&[("LANGUAGE", "ru"), ("LANG", "ru_RU.UTF-8")]);
        let snap = snapshot_locale_env(&plain);
        assert_eq!(snap[0], ("LANGUAGE", Some("ru".to_string())));
    }

    #[test]
    fn resolve_takes_first_existing_catalog() {
        let dirs = vec![PathBuf::from("/dev-tree/locale"), PathBuf::from("/usr/share/locale")];
        let have = |p: &Path| {
            p == Path::new("/usr/share/locale/pt/LC_MESSAGES/rexon.mo")
                || p == Path::new("/dev-tree/locale/ru/LC_MESSAGES/rexon.mo")
        };
        let c = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // `pt_BR` was not found — we take the base `pt`.
        assert_eq!(
            resolve(&c(&["pt_BR", "pt"]), &dirs, &have),
            Some(("pt".into(), PathBuf::from("/usr/share/locale/pt/LC_MESSAGES/rexon.mo")))
        );
        assert_eq!(
            resolve(&c(&["ru_RU", "ru"]), &dirs, &have).map(|(l, _)| l),
            Some("ru".into())
        );
        assert_eq!(resolve(&c(&["cs"]), &dirs, &have), None);
    }

    // ── Checks of the po/ catalogs themselves ────────────────────────────

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Numbers of the `%1`, `%2`… placeholders of a string (without `%n`).
    fn placeholders(text: &str) -> std::collections::BTreeSet<u32> {
        let b = text.as_bytes();
        let mut out = std::collections::BTreeSet::new();
        for i in 0..b.len() {
            if b[i] == b'%' {
                let digits: String =
                    b[i + 1..].iter().take(2).take_while(|c| c.is_ascii_digit()).map(|&c| c as char).collect();
                if let Ok(n) = digits.parse() {
                    out.insert(n);
                }
            }
        }
        out
    }

    /// Every language from `po/LINGUAS` is built with a real `msgfmt --check`, read
    /// by our reader, and every translation has the same placeholders as the
    /// original: a `%2` lost by a translator would give a string without a number,
    /// and that would be noticed only by eye.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/*.po настоящим msgfmt")]
    fn every_catalog_compiles_and_keeps_placeholders() {
        let po = repo_root().join("po");
        let linguas = std::fs::read_to_string(po.join("LINGUAS")).expect("po/LINGUAS на месте");
        let langs: Vec<&str> = linguas
            .lines()
            .map(|l| l.split('#').next().unwrap_or("").trim())
            .filter(|l| !l.is_empty())
            .collect();
        assert!(!langs.is_empty(), "в po/LINGUAS ни одного языка");
        for lang in langs {
            let out = std::env::temp_dir().join(format!("rexon-i18n-{lang}-{}.mo", std::process::id()));
            let status = std::process::Command::new("msgfmt")
                .arg("--check")
                .arg("-o")
                .arg(&out)
                .arg(po.join(format!("{lang}.po")))
                .status()
                .expect("msgfmt (пакет gettext) запускается");
            assert!(status.success(), "msgfmt отверг po/{lang}.po");
            let catalog = Catalog::load(&out).expect("каталог читается");
            let _ = std::fs::remove_file(&out);
            for (key, forms) in catalog.entries() {
                let msgid = key.rsplit('\u{4}').next().unwrap_or(key);
                let want = placeholders(msgid);
                for form in forms.iter().filter(|f| !f.is_empty()) {
                    assert_eq!(placeholders(form), want, "{lang}: «{msgid}» → «{form}»");
                }
            }
        }
    }

    /// The `po/rexon.pot` template has not fallen behind the sources: a new string
    /// without a template update would not reach a single translator.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: извлекает строки настоящим xgettext")]
    fn template_is_up_to_date() {
        let out = std::process::Command::new("bash")
            .arg(repo_root().join("scripts/i18n-update.sh"))
            .arg("--check")
            .output()
            .expect("bash запускается");
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn linguas_parsing_and_selectable_languages() {
        assert_eq!(parse_linguas("ru\n# comment\npl cs # tail\n\nru\n"), ["ru", "pl", "cs"]);
        // The real LINGUAS is read, and it contains Russian.
        assert!(parse_linguas(LINGUAS).iter().any(|c| c == "ru"));

        let dirs = vec![PathBuf::from("/dev-tree/locale"), PathBuf::from("/usr/share/locale")];
        let have = |p: &Path| p == Path::new("/usr/share/locale/ru/LC_MESSAGES/rexon.mo");
        let linguas: Vec<String> = ["ru", "pl", "en"].iter().map(|s| s.to_string()).collect();
        // English — always and first; Polish without a catalog is not shown.
        assert_eq!(selectable_languages(&linguas, &dirs, &have), ["en", "ru"]);
        assert_eq!(selectable_languages(&[], &dirs, &have), ["en"]);
    }

    #[test]
    fn native_names() {
        assert_eq!(native_name("en"), "English");
        assert_eq!(native_name("ru"), "Русский");
        assert_eq!(native_name("ru_RU"), "Русский");
        assert_eq!(native_name("pt_BR"), "Português (Brasil)");
        assert_eq!(native_name("xx"), "xx");
        // Every language of the plan is labeled with its endonym, not a code.
        for code in [
            "ar", "zh_CN", "ja", "it", "es", "de", "fr", "pt_BR", "tr", "pl", "he", "cs", "vi",
            "th", "ko", "fil", "hi", "sv", "nb", "hy", "ka", "uk", "nl", "fi", "da", "hu", "ro",
            "be", "el", "bg", "id", "ms", "fa", "kk", "sk", "sr",
        ] {
            assert_ne!(native_name(code), code, "{code} без самоназвания");
        }
    }

    /// Three promises of the language choice: `en` on a Russian system — English;
    /// "as in the system" — the system's language; a language without a catalog —
    /// English.
    #[test]
    fn setting_resolves_to_language() {
        let ru_system = |k: &str| (k == "LANG").then(|| "ru_RU.UTF-8".to_string());
        let dirs = vec![PathBuf::from("/usr/share/locale")];
        let have = |p: &Path| p == Path::new("/usr/share/locale/ru/LC_MESSAGES/rexon.mo");
        assert_eq!(resolve_setting(Some("en"), &ru_system, &dirs, &have), "en");
        assert_eq!(resolve_setting(Some("system"), &ru_system, &dirs, &have), "ru");
        assert_eq!(resolve_setting(None, &ru_system, &dirs, &have), "ru");
        assert_eq!(resolve_setting(Some("ru"), &|_| None, &dirs, &have), "ru");
        assert_eq!(resolve_setting(Some("pl"), &ru_system, &dirs, &have), "en");
        let de_system = |k: &str| (k == "LANG").then(|| "de_DE.UTF-8".to_string());
        assert_eq!(resolve_setting(Some("system"), &de_system, &dirs, &have), "en");
    }

    #[test]
    fn setting_normalization() {
        assert_eq!(normalize_setting(None), "system");
        assert_eq!(normalize_setting(Some("  ")), "system");
        assert_eq!(normalize_setting(Some("ru")), "ru");
    }

    #[test]
    fn locale_dirs_order() {
        let env = |k: &str| (k == ENV_LOCALEDIR).then(|| "/tmp/po-build".to_string());
        assert_eq!(
            locale_dirs(&env, Some(Path::new("/opt/x"))),
            [
                PathBuf::from("/tmp/po-build"),
                PathBuf::from("/opt/x/locale"),
                PathBuf::from(SYSTEM_LOCALEDIR)
            ]
        );
    }
}
