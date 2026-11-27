//! Keyboard layout in the configs of tiling desktops and Wayfire.
//!
//! sway, Hyprland, niri, river and Wayfire take the layout only from their
//! own config, and without a line there they turn on a single `us` (niri with
//! an empty `xkb {}` reads org.freedesktop.locale1, but many have it empty
//! too). Full environments (KDE, GNOME, XFCE…) take the layout from the
//! system themselves, while here a Russian speaker could not type in Russian
//! after logging in. So after installing such a desktop Rexon adds the
//! layout to its config.
//!
//! Structure: one function that determines the layout ([`detect`]) and one
//! output function per desktop ([`apply`]); both are pure, I/O lives in
//! [`ensure_after_install`].
//!
//! Where the layout comes from (in order):
//! 1. what is already set up in the system: `localectl status` (X11 Layout /
//!    Variant / Options), without it `/etc/X11/xorg.conf.d/00-keyboard.conf`
//!    and `XKBLAYOUT=` in `/etc/vconsole.conf`;
//! 2. if that is empty or a single `us`, by the system language (`LC_ALL`,
//!    `LC_MESSAGES`, `LANG`): ru → `us,ru`, de → `us,de`, uk → `us,ua`…
//!    ([`LANGUAGE_LAYOUTS`]); English and languages with an input method
//!    (CJK) get a single `us`, nothing to write.
//!
//! Switching is Alt+Shift (`grp:alt_shift_toggle`), unless the system has
//! its own `grp:`.
//!
//! ⚠️ A layout the person has already set (in the main config or in an
//! included file) is never touched by Rexon ([`Apply::Kept`]).
//! There is one exception: `kb_layout = us` in Hyprland, which is a line of
//! its sample config, not the person's choice.
//!
//! | desktop  | file                                   | what is written                                     |
//! |----------|----------------------------------------|-----------------------------------------------------|
//! | sway     | `~/.config/sway/config`                | `input type:keyboard { xkb_layout …; xkb_options … }` |
//! | Hyprland | `hypr/hyprland.lua` / `hyprland.conf`  | `hl.config({ input = { kb_layout = … } })` / `input { kb_layout = … }` |
//! | niri     | `niri/config.kdl`                      | `layout "…"` / `options "…"` in `input { keyboard { xkb { } } }` |
//! | river    | `river/init`                           | `riverctl keyboard-layout -options … …`             |
//! | Wayfire  | `wayfire.ini`                          | `[input]` `xkb_layout = …`, `xkb_options = …`       |

use crate::i18n::trf;
use crate::system_de_exec::DeOpsIo;

// ============================================================================
// Determining the layout
// ============================================================================

/// A layout as XKB defines it: comma-separated strings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KbLayout {
    pub layout: String,
    pub variant: String,
    pub options: String,
}

impl KbLayout {
    fn new(layout: &str, variant: &str, options: &str) -> Self {
        KbLayout { layout: layout.trim().into(), variant: variant.trim().into(), options: options.trim().into() }
    }

    /// Nothing set, or a single `us` without variants and options.
    fn is_plain_us(&self) -> bool {
        (self.layout.is_empty() || self.layout == "us") && self.variant.is_empty()
    }

    /// Alt+Shift switching, as named by the log line.
    pub fn switches_with_alt_shift(&self) -> bool {
        self.options.split(',').any(|o| o.trim() == ALT_SHIFT)
    }
}

/// Default layout switching.
pub const ALT_SHIFT: &str = "grp:alt_shift_toggle";

/// Language → XKB layout. Languages whose Latin script is already native or
/// that are typed with an input method (Chinese, Japanese, Korean) are not needed here.
pub const LANGUAGE_LAYOUTS: &[(&str, &str)] = &[
    ("ru", "ru"), ("uk", "ua"), ("be", "by"), ("kk", "kz"), ("ky", "kg"),
    ("tg", "tj"), ("uz", "uz"), ("az", "az"), ("hy", "am"), ("ka", "ge"),
    ("de", "de"), ("fr", "fr"), ("es", "es"), ("ca", "es"), ("it", "it"),
    ("pt", "pt"), ("pl", "pl"), ("cs", "cz"), ("sk", "sk"), ("hu", "hu"),
    ("ro", "ro"), ("bg", "bg"), ("sr", "rs"), ("mk", "mk"), ("hr", "hr"),
    ("bs", "ba"), ("sl", "si"), ("el", "gr"), ("tr", "tr"), ("he", "il"),
    ("ar", "ara"), ("fa", "ir"), ("sv", "se"), ("fi", "fi"), ("da", "dk"),
    ("nb", "no"), ("nn", "no"), ("no", "no"), ("is", "is"), ("et", "ee"),
    ("lv", "lv"), ("lt", "lt"), ("th", "th"), ("hi", "in"), ("vi", "vn"),
    ("ga", "ie"), ("mt", "mt"), ("sq", "al"), ("eo", "epo"),
];

/// A language with a country-specific layout (`pt_BR` → Brazilian, etc.).
const COUNTRY_LAYOUTS: &[(&str, &str)] = &[
    ("pt_BR", "br"), ("de_CH", "ch"), ("fr_CH", "ch"), ("de_AT", "at"),
    ("fr_CA", "ca"), ("fr_BE", "be"), ("nl_BE", "be"),
];

/// Layouts without Latin letters: with only one of them in the system not a
/// single command can be typed in a terminal, so `us` goes first.
const NON_LATIN: &[&str] = &[
    "ru", "ua", "by", "kz", "kg", "tj", "bg", "rs", "mk", "gr", "il", "ara",
    "ir", "ge", "am", "th", "in",
];

/// The locale value that decides the language: `LC_ALL`, then `LC_MESSAGES`,
/// then `LANG`; `C`/`POSIX` do not count as a language.
pub fn pick_locale<'a>(lc_all: &'a str, lc_messages: &'a str, lang: &'a str) -> &'a str {
    [lc_all, lc_messages, lang]
        .into_iter()
        .map(str::trim)
        .find(|v| !v.is_empty() && !matches!(*v, "C" | "POSIX") && !v.starts_with("C."))
        .unwrap_or("")
}

/// The layout for a language (`ru_RU.UTF-8` → `ru`); `None` means nothing to write.
pub fn layout_for_language(locale: &str) -> Option<&'static str> {
    let base = locale.split(['.', '@']).next().unwrap_or("");
    if let Some((_, l)) = COUNTRY_LAYOUTS.iter().find(|(k, _)| *k == base) {
        return Some(l);
    }
    let lang = base.split('_').next().unwrap_or("").to_ascii_lowercase();
    LANGUAGE_LAYOUTS.iter().find(|(k, _)| *k == lang).map(|(_, l)| *l)
}

/// `localectl status` (with `LANG=C`): the `X11 Layout:`, `X11 Variant:`,
/// `X11 Options:` lines.
pub fn parse_localectl(text: &str) -> KbLayout {
    let field = |name: &str| {
        text.lines()
            .filter_map(|l| l.trim().strip_prefix(name))
            .find_map(|rest| rest.trim_start().strip_prefix(':'))
            .map(|v| v.trim().to_string())
            .filter(|v| v != "n/a")
            .unwrap_or_default()
    };
    KbLayout::new(&field("X11 Layout"), &field("X11 Variant"), &field("X11 Options"))
}

/// `00-keyboard.conf`: `Option "XkbLayout" "…"` and its kin.
pub fn parse_xorg_keyboard_conf(text: &str) -> KbLayout {
    let field = |name: &str| {
        text.lines()
            .map(|l| l.split('#').next().unwrap_or("").trim())
            .filter_map(|l| l.strip_prefix("Option"))
            .find_map(|rest| {
                let mut q = rest.split('"').skip(1).step_by(2);
                let (k, v) = (q.next()?, q.next()?);
                k.eq_ignore_ascii_case(name).then(|| v.to_string())
            })
            .unwrap_or_default()
    };
    KbLayout::new(&field("XkbLayout"), &field("XkbVariant"), &field("XkbOptions"))
}

/// `/etc/vconsole.conf`: `XKBLAYOUT=`, `XKBVARIANT=`, `XKBOPTIONS=`
/// (written by systemd-localed).
pub fn parse_vconsole(text: &str) -> KbLayout {
    let field = |name: &str| {
        text.lines()
            .filter_map(|l| l.trim().strip_prefix(name)?.strip_prefix('='))
            .map(|v| v.trim().trim_matches('"').to_string())
            .last()
            .unwrap_or_default()
    };
    KbLayout::new(&field("XKBLAYOUT"), &field("XKBVARIANT"), &field("XKBOPTIONS"))
}

/// Options with switching: `grp:` is added only if there is none of its own.
fn with_switch(options: &str) -> String {
    merge_options(options, ALT_SHIFT)
}

/// Merge options: the existing ones stay, whatever is missing is added from
/// `ours`, and `grp:` only if there is no switching of its own at all.
pub fn merge_options(theirs: &str, ours: &str) -> String {
    let mut out: Vec<&str> = theirs.split(',').map(str::trim).filter(|o| !o.is_empty()).collect();
    let has_grp = out.iter().any(|o| o.starts_with("grp:"));
    for o in ours.split(',').map(str::trim).filter(|o| !o.is_empty()) {
        if (o.starts_with("grp:") && has_grp) || out.contains(&o) {
            continue;
        }
        out.push(o);
    }
    out.join(",")
}

/// The layout for a desktop. `localectl`, `xorg`, `vconsole` are the texts of
/// the sources (`None` means no source), `locale` comes from [`pick_locale`].
/// `None` means nothing to write: a single `us` both in the system and by language.
pub fn detect(localectl: Option<&str>, xorg: Option<&str>, vconsole: Option<&str>, locale: &str) -> Option<KbLayout> {
    let system: Vec<KbLayout> = [
        localectl.map(parse_localectl),
        xorg.map(parse_xorg_keyboard_conf),
        vconsole.map(parse_vconsole),
    ]
    .into_iter()
    .flatten()
    .collect();

    // 1. The system's own layout, as is.
    if let Some(kb) = system.iter().find(|k| !k.is_plain_us() && !k.layout.is_empty()) {
        let mut kb = kb.clone();
        if NON_LATIN.contains(&kb.layout.as_str()) {
            kb.layout = format!("us,{}", kb.layout);
            if !kb.variant.is_empty() {
                kb.variant = format!(",{}", kb.variant);
            }
        }
        if kb.layout.contains(',') {
            kb.options = with_switch(&kb.options);
        }
        return Some(kb);
    }

    // 2. A single `us` in the system (possibly with options): the language's layout.
    let us = system.iter().find(|k| !k.options.is_empty()).cloned().unwrap_or_default();
    match layout_for_language(locale) {
        Some(l) => Some(KbLayout {
            layout: format!("us,{l}"),
            variant: String::new(),
            options: with_switch(&us.options),
        }),
        None if !us.options.is_empty() => Some(KbLayout::new("us", "", &us.options)),
        None => None,
    }
}

// ============================================================================
// Output to the desktop
// ============================================================================

/// The desktop whose config receives the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desk {
    Sway,
    HyprConf,
    HyprLua,
    Niri,
    River,
    Wayfire,
}

impl Desk {
    pub fn display_name(self) -> &'static str {
        match self {
            Desk::Sway => "Sway",
            Desk::HyprConf | Desk::HyprLua => "Hyprland",
            Desk::Niri => "niri",
            Desk::River => "River",
            Desk::Wayfire => "Wayfire",
        }
    }
}

/// What to do with the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Apply {
    /// The person has already set a layout: leave it alone.
    Kept,
    /// The new text of the file.
    Write(String),
    /// Cannot tell where to insert (unusual file structure): the person is
    /// shown a ready-made snippet.
    Manual(String),
}

const HEADER: &str = "Keyboard layouts (added by Rexon from your system settings; change freely)";

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Snippet for sway (`sway-input(5)`).
pub fn sway_snippet(kb: &KbLayout) -> String {
    let mut s = format!("# {HEADER}\ninput type:keyboard {{\n    xkb_layout {}\n", quote(&kb.layout));
    if !kb.variant.is_empty() {
        s.push_str(&format!("    xkb_variant {}\n", quote(&kb.variant)));
    }
    if !kb.options.is_empty() {
        s.push_str(&format!("    xkb_options {}\n", quote(&kb.options)));
    }
    s.push_str("}\n");
    s
}

/// Snippet for `hyprland.conf` (hyprlang): the `input` category may be
/// opened again, a later value overrides an earlier one.
pub fn hypr_conf_snippet(kb: &KbLayout) -> String {
    let mut s = format!("# {HEADER}\ninput {{\n    kb_layout = {}\n", kb.layout);
    if !kb.variant.is_empty() {
        s.push_str(&format!("    kb_variant = {}\n", kb.variant));
    }
    s.push_str(&format!("    kb_options = {}\n}}\n", kb.options));
    s
}

/// Snippet for `hyprland.lua`: `hl.config` sets the listed keys, and a
/// later call overrides an earlier one.
pub fn hypr_lua_snippet(kb: &KbLayout) -> String {
    let mut fields = vec![format!("kb_layout = {}", quote(&kb.layout))];
    if !kb.variant.is_empty() {
        fields.push(format!("kb_variant = {}", quote(&kb.variant)));
    }
    fields.push(format!("kb_options = {}", quote(&kb.options)));
    let body: String = fields.iter().map(|f| format!("        {f},\n")).collect();
    format!("-- {HEADER}\nhl.config({{\n    input = {{\n{body}    }},\n}})\n")
}

/// Lines for the niri `xkb` block (without indent).
fn niri_xkb_lines(kb: &KbLayout, with_options: bool) -> Vec<String> {
    let mut v = vec![format!("// {HEADER}"), format!("layout {}", quote(&kb.layout))];
    if !kb.variant.is_empty() {
        v.push(format!("variant {}", quote(&kb.variant)));
    }
    if with_options && !kb.options.is_empty() {
        v.push(format!("options {}", quote(&kb.options)));
    }
    v
}

/// The whole `input` block for niri (a config without `input`).
pub fn niri_snippet(kb: &KbLayout) -> String {
    let inner: String = niri_xkb_lines(kb, true).iter().map(|l| format!("            {l}\n")).collect();
    format!("input {{\n    keyboard {{\n        xkb {{\n{inner}        }}\n    }}\n}}\n")
}

fn sh_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || ",:_-.+".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Line for `river/init` (`riverctl(1)`, `keyboard-layout`).
pub fn river_snippet(kb: &KbLayout) -> String {
    let mut s = format!("# {HEADER}\nriverctl keyboard-layout");
    if !kb.variant.is_empty() {
        s.push_str(&format!(" -variant {}", sh_quote(&kb.variant)));
    }
    if !kb.options.is_empty() {
        s.push_str(&format!(" -options {}", sh_quote(&kb.options)));
    }
    s.push_str(&format!(" {}\n", sh_quote(&kb.layout)));
    s
}

/// Lines of the Wayfire `[input]` section.
fn wayfire_lines(kb: &KbLayout) -> Vec<(&'static str, String)> {
    let mut v = vec![("xkb_layout", kb.layout.clone())];
    if !kb.variant.is_empty() {
        v.push(("xkb_variant", kb.variant.clone()));
    }
    if !kb.options.is_empty() {
        v.push(("xkb_options", kb.options.clone()));
    }
    v
}

/// Lines without a `#` comment (quotes are not parsed: layout values never
/// contain `#`).
pub(crate) fn hash_code(line: &str) -> &str {
    line.split('#').next().unwrap_or("").trim()
}

/// The value after a keyword: `xkb_layout "us"` → `us`,
/// `kb_layout = us,ru` → `us,ru`, `kb_layout = "us",` → `us`.
fn value_after<'a>(code: &'a str, key: &str) -> Option<&'a str> {
    let pos = code.find(key)?;
    // The key is a separate word (`kb_layout`, not `xkb_layout` in Hyprland).
    let before = code[..pos].chars().last();
    if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let rest = &code[pos + key.len()..];
    if rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || "=:]'\"".contains(c)).trim();
    Some(rest.trim_end_matches([',', ';', '}', ')']).trim().trim_matches(['"', '\'']).trim())
}

/// The last value of the key in the live lines `texts`.
fn last_value(texts: &[&str], keys: &[&str], code: fn(&str) -> &str) -> Option<String> {
    let mut found = None;
    for t in texts {
        for line in t.lines() {
            let c = code(line);
            for k in keys {
                if let Some(v) = value_after(c, k) {
                    found = Some(v.to_string());
                }
            }
        }
    }
    found
}

pub(crate) fn lua_code(line: &str) -> &str {
    line.split("--").next().unwrap_or("").trim()
}

pub(crate) fn append(text: &str, snippet: &str) -> String {
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(snippet);
    out
}

/// The layout with options merged with those the person already set.
fn with_their_options(kb: &KbLayout, theirs: Option<String>) -> KbLayout {
    match theirs {
        Some(t) if !t.is_empty() => KbLayout { options: merge_options(&t, &kb.options), ..kb.clone() },
        _ => kb.clone(),
    }
}

/// Add the layout `kb` to the desktop config. `main` is the text of the main
/// file, `included` are the texts of the files it includes (a layout there
/// also counts as set by the person).
pub fn apply(desk: Desk, main: &str, included: &[String], kb: &KbLayout) -> Apply {
    let mut all: Vec<&str> = vec![main];
    all.extend(included.iter().map(String::as_str));
    match desk {
        Desk::Sway => {
            if last_value(&all, &["xkb_layout", "xkb_file"], hash_code).is_some_and(|v| !v.is_empty()) {
                return Apply::Kept;
            }
            let kb = with_their_options(kb, last_value(&all, &["xkb_options"], hash_code));
            Apply::Write(append(main, &sway_snippet(&kb)))
        }
        Desk::HyprConf | Desk::HyprLua => {
            let code = if desk == Desk::HyprLua { lua_code } else { hash_code };
            let file = last_value(&all, &["kb_file"], code).is_some_and(|v| !v.is_empty());
            let layout = last_value(&all, &["kb_layout"], code);
            // `us` is a line of the Hyprland sample config, not the person's choice.
            if file || layout.is_some_and(|v| !v.is_empty() && v != "us") {
                return Apply::Kept;
            }
            let kb = with_their_options(kb, last_value(&all, &["kb_options"], code));
            let snippet = if desk == Desk::HyprLua { hypr_lua_snippet(&kb) } else { hypr_conf_snippet(&kb) };
            Apply::Write(append(main, &snippet))
        }
        Desk::Niri => apply_niri(main, included, kb),
        Desk::River => {
            let set = all.iter().any(|t| {
                t.lines().any(|l| {
                    let c = hash_code(l);
                    c.contains("keyboard-layout") && c.contains("riverctl")
                })
            });
            if set {
                return Apply::Kept;
            }
            Apply::Write(insert_after_header(main, &river_snippet(kb)))
        }
        Desk::Wayfire => apply_wayfire(main, kb),
    }
}

/// Insert the snippet after the script header (`#!` and the comments right
/// after it): river runs init with the socket already in place, and `riverctl`
/// works from the first line, while anything appended at the end would not run after `exec`.
fn insert_after_header(text: &str, snippet: &str) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let header = lines.iter().take_while(|l| l.trim_start().starts_with('#')).count();
    let mut out: String = lines[..header].concat();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out.push_str(snippet);
    let rest = lines[header..].concat();
    if !rest.is_empty() && !rest.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(&rest);
    out
}

// ── niri (KDL) ──────────────────────────────────────────────────────────────

/// What was found in `config.kdl` along the path `input` → `keyboard` → `xkb`.
#[derive(Debug, Default)]
struct NiriScan {
    /// A live `layout` or `file` inside `xkb`.
    layout_set: bool,
    /// The line with a live `options "…"` in `xkb` (the whole line is just that) and
    /// its value.
    options: Option<(usize, String)>,
    /// Lines where the block opens (with nothing after `{` on the line).
    xkb_open: Option<usize>,
    /// `xkb {}` on one line.
    xkb_empty_line: Option<usize>,
    keyboard_open: Option<usize>,
    input_open: Option<usize>,
    /// The block of the wanted node exists but is not opened in a way that allows inserting into it.
    odd: bool,
}

/// Strip KDL comments (`//` to the end of the line, `/* … */`) outside strings.
pub(crate) fn kdl_strip(line: &str, in_block: &mut bool) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if *in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                *in_block = false;
            }
            continue;
        }
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => break,
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                *in_block = true;
            }
            _ => out.push(c),
        }
    }
    out
}

fn niri_scan(text: &str) -> NiriScan {
    let mut scan = NiriScan::default();
    // (node name, whether it is live: `/-` disables a node with all its children)
    let mut stack: Vec<(String, bool)> = Vec::new();
    let mut in_block = false;
    for (i, raw) in text.lines().enumerate() {
        let line = kdl_strip(raw, &mut in_block);
        let mut buf = String::new();
        let mut in_str = false;
        let chars: Vec<char> = line.chars().collect();
        let mut j = 0;
        let alive_path = |stack: &[(String, bool)]| stack.iter().all(|(_, a)| *a);
        let path_is = |stack: &[(String, bool)], want: &[&str]| {
            stack.len() == want.len() && stack.iter().zip(want).all(|((n, _), w)| n == w)
        };
        let leaf = |buf: &mut String, stack: &[(String, bool)], scan: &mut NiriScan| {
            let stmt = buf.trim().to_string();
            buf.clear();
            if stmt.is_empty() || stmt.starts_with("/-") || !alive_path(stack) {
                return;
            }
            if path_is(stack, &["input", "keyboard", "xkb"]) {
                let name = stmt.split_whitespace().next().unwrap_or("");
                if matches!(name, "layout" | "file") {
                    let v = stmt[name.len()..].trim().trim_matches('"');
                    if !v.is_empty() {
                        scan.layout_set = true;
                    }
                } else if name == "options" {
                    let v = stmt[name.len()..].trim().trim_matches('"').to_string();
                    let whole = line.trim() == stmt;
                    scan.options = Some((if whole { i } else { usize::MAX }, v));
                }
            }
        };
        while j < chars.len() {
            let c = chars[j];
            if in_str {
                buf.push(c);
                if c == '\\' && j + 1 < chars.len() {
                    buf.push(chars[j + 1]);
                    j += 1;
                } else if c == '"' {
                    in_str = false;
                }
                j += 1;
                continue;
            }
            match c {
                '"' => {
                    in_str = true;
                    buf.push(c);
                }
                '{' => {
                    let stmt = buf.trim().to_string();
                    buf.clear();
                    let dead = stmt.starts_with("/-");
                    let name = stmt.trim_start_matches("/-").split_whitespace().next().unwrap_or("").to_string();
                    let alive = !dead && alive_path(&stack);
                    let rest_empty = chars[j + 1..].iter().all(|c| c.is_whitespace());
                    let rest_closes = chars[j + 1..].iter().collect::<String>().trim() == "}";
                    let want = match name.as_str() {
                        "input" if stack.is_empty() => Some(&mut scan.input_open),
                        "keyboard" if path_is(&stack, &["input"]) => Some(&mut scan.keyboard_open),
                        "xkb" if path_is(&stack, &["input", "keyboard"]) => Some(&mut scan.xkb_open),
                        _ => None,
                    };
                    if alive {
                        if let Some(slot) = want {
                            if rest_empty {
                                slot.get_or_insert(i);
                            } else if name == "xkb" && rest_closes && line.trim().starts_with("xkb") {
                                scan.xkb_empty_line.get_or_insert(i);
                            } else {
                                scan.odd = true;
                            }
                        }
                    }
                    stack.push((name, alive));
                }
                '}' => {
                    leaf(&mut buf, &stack, &mut scan);
                    stack.pop();
                }
                ';' => leaf(&mut buf, &stack, &mut scan),
                _ => buf.push(c),
            }
            j += 1;
        }
        leaf(&mut buf, &stack, &mut scan);
    }
    scan
}

fn indent_of(line: &str) -> String {
    line.chars().take_while(|c| c.is_whitespace()).collect()
}

fn apply_niri(main: &str, included: &[String], kb: &KbLayout) -> Apply {
    if included.iter().any(|t| niri_scan(t).layout_set) {
        return Apply::Kept;
    }
    let scan = niri_scan(main);
    if scan.layout_set {
        return Apply::Kept;
    }
    let mut lines: Vec<String> = main.lines().map(str::to_string).collect();
    let block = |indent: &str, inner: Vec<String>| -> Vec<String> { inner.into_iter().map(|l| format!("{indent}{l}")).collect() };
    let xkb = |with_options| niri_xkb_lines(kb, with_options);
    if let Some(i) = scan.xkb_open {
        let ind = format!("{}    ", indent_of(&lines[i]));
        // The person's own `options` stay, switching is appended to them.
        let own_options = scan.options.clone();
        if let Some((oi, v)) = &own_options {
            if *oi != usize::MAX {
                let merged = merge_options(v, &kb.options);
                lines[*oi] = format!("{}options {}", indent_of(&lines[*oi]), quote(&merged));
            }
        }
        let new = block(&ind, xkb(own_options.is_none()));
        lines.splice(i + 1..i + 1, new);
    } else if let Some(i) = scan.xkb_empty_line {
        let ind = indent_of(&lines[i]);
        let mut new = vec![format!("{ind}xkb {{")];
        new.extend(block(&format!("{ind}    "), xkb(true)));
        new.push(format!("{ind}}}"));
        lines.splice(i..=i, new);
    } else if scan.odd {
        return Apply::Manual(niri_snippet(kb));
    } else if let Some(i) = scan.keyboard_open {
        let ind = format!("{}    ", indent_of(&lines[i]));
        let mut new = vec![format!("{ind}xkb {{")];
        new.extend(block(&format!("{ind}    "), xkb(true)));
        new.push(format!("{ind}}}"));
        lines.splice(i + 1..i + 1, new);
    } else if let Some(i) = scan.input_open {
        let ind = format!("{}    ", indent_of(&lines[i]));
        let mut new = vec![format!("{ind}keyboard {{"), format!("{ind}    xkb {{")];
        new.extend(block(&format!("{ind}        "), xkb(true)));
        new.push(format!("{ind}    }}"));
        new.push(format!("{ind}}}"));
        lines.splice(i + 1..i + 1, new);
    } else {
        return Apply::Write(append(main, &format!("// {HEADER}\n{}", niri_snippet(kb).replacen(&format!("            // {HEADER}\n"), "", 1))));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    Apply::Write(out)
}

// ── Wayfire (INI) ───────────────────────────────────────────────────────────

fn ini_section(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

fn ini_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let t = line.trim();
    if t.starts_with('#') || t.starts_with(';') {
        return None;
    }
    let (k, v) = t.split_once('=')?;
    (k.trim() == key).then(|| v.trim())
}

fn apply_wayfire(main: &str, kb: &KbLayout) -> Apply {
    let mut lines: Vec<String> = main.lines().map(str::to_string).collect();
    let mut section = "";
    let mut header: Option<usize> = None;
    let mut found: Vec<(usize, &'static str, String)> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some(s) = ini_section(l) {
            section = if s == "input" { "input" } else { "" };
            if section == "input" && header.is_none() {
                header = Some(i);
            }
            continue;
        }
        if section != "input" {
            continue;
        }
        for key in ["xkb_layout", "xkb_variant", "xkb_options"] {
            if let Some(v) = ini_key(l, key) {
                found.push((i, key, v.to_string()));
            }
        }
    }
    if found.iter().any(|(_, k, v)| *k == "xkb_layout" && !v.is_empty()) {
        return Apply::Kept;
    }
    let theirs = found.iter().rev().find(|(_, k, _)| *k == "xkb_options").map(|(_, _, v)| v.clone());
    let kb = with_their_options(kb, theirs);
    let mut pending: Vec<(&'static str, String)> = Vec::new();
    for (key, value) in wayfire_lines(&kb) {
        let new_line = format!("{key} = {value}");
        // Empty key (or own options): the same line, a new value.
        match found.iter().rev().find(|(_, k, _)| *k == key) {
            Some((i, _, _)) => lines[*i] = new_line,
            None => pending.push((key, value)),
        }
    }
    let new: Vec<String> = pending.iter().map(|(k, v)| format!("{k} = {v}")).collect();
    match header {
        Some(h) => {
            let mut ins = vec![format!("# {HEADER}")];
            ins.extend(new);
            if ins.len() > 1 {
                lines.splice(h + 1..h + 1, ins);
            }
        }
        None => {
            if !lines.is_empty() && !lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push("[input]".into());
            lines.push(format!("# {HEADER}"));
            lines.extend(new);
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    Apply::Write(out)
}

// ── Included files ──────────────────────────────────────────────────────────

/// Files the config includes (a layout in them is also the person's
/// choice). Paths as in the file, `~` expanded, relative ones resolved from
/// `dir`; a `*` in the last component is expanded by the caller.
pub fn included_paths(desk: Desk, text: &str, dir: &str, home: &str) -> Vec<String> {
    let resolve = |p: &str| -> String {
        let p = p.trim().trim_matches(['"', '\'']);
        if let Some(rest) = p.strip_prefix("~/") {
            format!("{}/{rest}", home.trim_end_matches('/'))
        } else if p.starts_with('/') {
            p.to_string()
        } else {
            format!("{}/{p}", dir.trim_end_matches('/'))
        }
    };
    let mut out = Vec::new();
    for line in text.lines() {
        match desk {
            Desk::Sway => {
                let c = hash_code(line);
                if let Some(p) = c.strip_prefix("include ") {
                    out.push(resolve(p));
                }
            }
            Desk::HyprConf => {
                let c = hash_code(line);
                if let Some((k, v)) = c.split_once('=') {
                    if k.trim() == "source" {
                        out.push(resolve(v));
                    }
                }
            }
            Desk::HyprLua => {
                let c = lua_code(line);
                for (call, lua_module) in [("require", true), ("dofile", false)] {
                    if let Some(pos) = c.find(call) {
                        let arg = c[pos + call.len()..].trim_start_matches(['(', ' ']);
                        let Some(q) = arg.chars().next().filter(|q| *q == '"' || *q == '\'') else { continue };
                        let name: String = arg[1..].chars().take_while(|ch| *ch != q).collect();
                        if lua_module {
                            out.push(format!("{}/{}.lua", dir.trim_end_matches('/'), name.replace('.', "/")));
                        } else {
                            out.push(resolve(&name));
                        }
                    }
                }
            }
            Desk::Niri => {
                let c = kdl_strip(line, &mut false);
                if let Some(p) = c.trim().strip_prefix("include ") {
                    out.push(resolve(p));
                }
            }
            Desk::River | Desk::Wayfire => {}
        }
    }
    out
}

// ============================================================================
// Execution after a desktop is installed
// ============================================================================

/// Config paths of a desktop. The main file is the one the desktop reads first.
struct Paths {
    desk: Desk,
    path: String,
    exists: bool,
    /// When the file is missing: its beginning (`include` of the system one for
    /// sway) or a sample to create it from; `None` means it must not be created.
    seed: Option<Seed>,
}

enum Seed {
    Text(String),
    File(String),
}

fn config_home<IO: DeOpsIo + ?Sized>(io: &IO, home: &str) -> String {
    let x = io.env("XDG_CONFIG_HOME");
    if x.is_empty() { format!("{}/.config", home.trim_end_matches('/')) } else { x.trim_end_matches('/').to_string() }
}

fn resolve<IO: DeOpsIo + ?Sized>(io: &IO, de_id: &str, home: &str) -> Option<Paths> {
    let cfg = config_home(io, home);
    let first = |cands: &[(String, Desk)]| cands.iter().find(|(p, _)| io.is_file(p)).cloned();
    let found = |path: String, desk: Desk| Some(Paths { desk, path, exists: true, seed: None });
    match de_id {
        "sway" => {
            let cands = [
                (format!("{home}/.sway/config"), Desk::Sway),
                (format!("{cfg}/sway/config"), Desk::Sway),
                (format!("{home}/.i3/config"), Desk::Sway),
                (format!("{cfg}/i3/config"), Desk::Sway),
            ];
            if let Some((p, d)) = first(&cands) {
                return found(p, d);
            }
            // An own file would shadow the system one entirely (keybindings included):
            // it is created only with an `include` of the system one in front.
            let system = ["/etc/sway/config", "/etc/i3/config"].into_iter().find(|p| io.is_file(p));
            let seed = system.map(|s| Seed::Text(format!("include {}\n", crate::themes::sway_path(s))));
            Some(Paths { desk: Desk::Sway, path: format!("{cfg}/sway/config"), exists: false, seed })
        }
        "hyprland" => {
            let lua = format!("{cfg}/hypr/hyprland.lua");
            let conf = format!("{cfg}/hypr/hyprland.conf");
            if io.is_file(&lua) {
                return found(lua, Desk::HyprLua);
            }
            if io.is_file(&conf) {
                return found(conf, Desk::HyprConf);
            }
            // Without a config Hyprland puts its own on first login, while a file made
            // of layout lines only would leave the person without keybindings. So it
            // is created only as a copy of the sample from the package.
            for (sample, path, desk) in
                [("/usr/share/hypr/hyprland.lua", lua, Desk::HyprLua), ("/usr/share/hypr/hyprland.conf", conf, Desk::HyprConf)]
            {
                if io.is_file(sample) {
                    return Some(Paths { desk, path, exists: false, seed: Some(Seed::File(sample.into())) });
                }
            }
            Some(Paths { desk: Desk::HyprLua, path: format!("{cfg}/hypr/hyprland.lua"), exists: false, seed: None })
        }
        "niri" => {
            let p = format!("{cfg}/niri/config.kdl");
            if io.is_file(&p) {
                return found(p, Desk::Niri);
            }
            // Same as Hyprland: niri puts a config itself on first login, and the same
            // one lies as a sample in the package documentation.
            let sample = ["/usr/share/doc/niri/default-config.kdl", "/usr/share/niri/default-config.kdl"]
                .into_iter()
                .find(|s| io.is_file(s))
                .map(|s| Seed::File(s.into()));
            Some(Paths { desk: Desk::Niri, path: p, exists: false, seed: sample })
        }
        "river" => {
            let p = crate::system_plan::river_init_path(&io.env("XDG_CONFIG_HOME"), home);
            let exists = io.is_file(&p);
            Some(Paths { desk: Desk::River, path: p, exists, seed: None })
        }
        "wayfire" => {
            let vendored = format!("{cfg}/wayfire/wayfire.ini");
            let p = if io.is_file(&vendored) { vendored } else { format!("{cfg}/wayfire.ini") };
            let exists = io.is_file(&p);
            // An own wayfire.ini disables nothing: Wayfire reads the defaults
            // from defaults.ini.
            Some(Paths { desk: Desk::Wayfire, path: p, exists, seed: Some(Seed::Text(String::new())) })
        }
        _ => None,
    }
}

/// The system layout on a live machine (through `io`).
pub fn detect_with<IO: DeOpsIo + ?Sized>(io: &IO) -> Option<KbLayout> {
    let localectl = if io.which("localectl") {
        let r = io.run(&["localectl".to_string(), "status".to_string()], Some(10), true);
        (r.rc() == Some(0)).then_some(r.stdout)
    } else {
        None
    };
    let xorg = io.read_text("/etc/X11/xorg.conf.d/00-keyboard.conf");
    let vconsole = io.read_text("/etc/vconsole.conf");
    let (lc_all, lc_messages, lang) = (io.env("LC_ALL"), io.env("LC_MESSAGES"), io.env("LANG"));
    detect(localectl.as_deref(), xorg.as_deref(), vconsole.as_deref(), pick_locale(&lc_all, &lc_messages, &lang))
}

fn read_included<IO: DeOpsIo + ?Sized>(io: &IO, desk: Desk, text: &str, dir: &str, home: &str, depth: u8) -> Vec<String> {
    let mut out = Vec::new();
    for p in included_paths(desk, text, dir, home) {
        let files: Vec<String> = match p.rsplit_once('/') {
            Some((d, name)) if name.contains('*') => {
                let (pre, post) = name.split_once('*').unwrap_or((name, ""));
                let mut v: Vec<String> = io
                    .list_dir(d)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|n| n.starts_with(pre) && n.ends_with(post))
                    .map(|n| format!("{d}/{n}"))
                    .collect();
                v.sort();
                v
            }
            _ => vec![p],
        };
        for f in files {
            if let Some(t) = io.read_text(&f) {
                if depth > 0 {
                    let sub_dir = f.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
                    out.extend(read_included(io, desk, &t, &sub_dir, home, depth - 1));
                }
                out.push(t);
            }
        }
    }
    out
}

/// Log line: the layout has been added.
fn written_line(desk: Desk, kb: &KbLayout, path: &str) -> String {
    if kb.switches_with_alt_shift() {
        // TRANSLATORS: %1 is a desktop (Sway, Hyprland…), %2 the keyboard
        // layouts (“us,ru”), %3 a file path.
        trf("⌨️ %1: keyboard layouts %2 added to %3 — switch them with Alt+Shift", &[&desk.display_name(), &kb.layout, &path])
    } else {
        // TRANSLATORS: %1 is a desktop (Sway, Hyprland…), %2 the keyboard
        // layout (“us,ru”), %3 a file path.
        trf("⌨️ %1: keyboard layout %2 added to %3", &[&desk.display_name(), &kb.layout, &path])
    }
}

/// A desktop's config as the desktop itself will see it: the main file (or
/// its starting text when there is no file yet) and the texts of included
/// files. Shared by the keyboard layout and the screen lock
/// ([`crate::system_de_exec`]).
pub(crate) struct DeskConfig {
    pub desk: Desk,
    /// The main file — the one the desktop reads first.
    pub path: String,
    /// Text to edit: the file itself, or, when it is missing, a starting text
    /// (sway: `include` of the system config) or the package's sample.
    /// `None` — no file and none may be created.
    pub text: Option<String>,
    /// Texts of included files (settings there are the user's choice too).
    pub included: Vec<String>,
}

impl DeskConfig {
    pub fn dir(&self) -> String {
        self.path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_else(|| ".".into())
    }

    /// File mode on write: River's init is executable.
    pub fn mode(&self) -> u32 {
        if self.desk == Desk::River { 0o755 } else { 0o644 }
    }
}

/// Read the config of desktop `de_id`; `None` — not a tiling desktop or Wayfire.
pub(crate) fn load_config<IO: DeOpsIo + ?Sized>(io: &IO, de_id: &str, home: &str) -> Option<DeskConfig> {
    let paths = resolve(io, de_id, home)?;
    let text = if paths.exists {
        Some(io.read_text(&paths.path).unwrap_or_default())
    } else {
        match &paths.seed {
            Some(Seed::Text(t)) => Some(t.clone()),
            Some(Seed::File(f)) => io.read_text(f),
            None => None,
        }
    };
    let mut cfg = DeskConfig { desk: paths.desk, path: paths.path, text, included: Vec::new() };
    if let Some(t) = &cfg.text {
        cfg.included = read_included(io, cfg.desk, t, &cfg.dir(), home, 1);
    }
    Some(cfg)
}

/// After installing the desktop `de_id` (and on «Check / Repair») add the
/// layout to its config. `None` means the desktop is not one of these or
/// there is nothing to write; otherwise (success, log line). A failure is a
/// warning, not a failed install.
pub fn ensure_after_install<IO: DeOpsIo + ?Sized>(io: &IO, de_id: &str, home: &str) -> Option<(bool, String)> {
    let cfg = load_config(io, de_id, home)?;
    let kb = detect_with(io)?;
    let name = cfg.desk.display_name();
    let Some(main) = &cfg.text else {
        return Some((true, no_config_line(name, &cfg.path)));
    };
    match apply(cfg.desk, main, &cfg.included, &kb) {
        Apply::Kept => Some((true, kept_line(name, &cfg.path))),
        Apply::Manual(snippet) => Some((true, manual_line(name, &cfg.path, &snippet))),
        Apply::Write(text) => {
            let written = io.make_dirs(&cfg.dir()).and_then(|()| io.write_file_mode(&cfg.path, &text, cfg.mode()));
            match written {
                Ok(()) => Some((true, written_line(cfg.desk, &kb, &cfg.path))),
                Err(e) => Some((false, failed_line(name, &cfg.path, &e))),
            }
        }
    }
}

fn kept_line(name: &str, path: &str) -> String {
    // TRANSLATORS: %1 is a desktop (Sway, Hyprland…), %2 a file path.
    trf("ℹ️ %1: the keyboard layout is already set in your settings (%2) — left as is", &[&name, &path])
}

fn no_config_line(name: &str, path: &str) -> String {
    // TRANSLATORS: %1 is a desktop (Hyprland, niri…), %2 a file path.
    trf(
        "ℹ️ %1: there is no settings file yet (%2). Log in once, then run “Check / Repair” and Rexon will add the keyboard layouts.",
        &[&name, &path],
    )
}

fn manual_line(name: &str, path: &str, snippet: &str) -> String {
    // TRANSLATORS: %1 is a desktop, %2 a file path, %3 a settings snippet.
    trf("ℹ️ %1: Rexon couldn't tell where to add the keyboard layouts in %2. Add this yourself:\n%3", &[&name, &path, &snippet])
}

fn failed_line(name: &str, path: &str, why: &str) -> String {
    // TRANSLATORS: %1 is a desktop, %2 a file path, %3 the reason.
    trf("⚠️ %1: couldn't add the keyboard layouts to %2: %3", &[&name, &path, &why])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kb(l: &str, o: &str) -> KbLayout {
        KbLayout::new(l, "", o)
    }

    const LOCALECTL_RU: &str = "   System Locale: LANG=ru_RU.UTF-8\n       VC Keymap: ru\n      X11 Layout: us,ru\n     X11 Options: grp:alt_shift_toggle\n";
    const LOCALECTL_EMPTY: &str = "   System Locale: LANG=ru_RU.UTF-8\n       VC Keymap: n/a\n      X11 Layout: n/a\n";

    #[test]
    fn localectl_and_files_are_parsed() {
        assert_eq!(parse_localectl(LOCALECTL_RU), kb("us,ru", "grp:alt_shift_toggle"));
        assert_eq!(parse_localectl(LOCALECTL_EMPTY), KbLayout::default());
        let x = "Section \"InputClass\"\n  Identifier \"system-keyboard\"\n  Option \"XkbLayout\" \"us,de\"\n  Option \"XkbVariant\" \",nodeadkeys\"\n  Option \"XkbOptions\" \"grp:win_space_toggle\"\nEndSection\n";
        assert_eq!(parse_xorg_keyboard_conf(x), KbLayout::new("us,de", ",nodeadkeys", "grp:win_space_toggle"));
        let v = "KEYMAP=ru\nXKBLAYOUT=us,ru\nXKBOPTIONS=grp:alt_shift_toggle\n";
        assert_eq!(parse_vconsole(v), kb("us,ru", "grp:alt_shift_toggle"));
    }

    #[test]
    fn language_table() {
        assert_eq!(layout_for_language("ru_RU.UTF-8"), Some("ru"));
        assert_eq!(layout_for_language("uk_UA.UTF-8"), Some("ua"));
        assert_eq!(layout_for_language("de_DE.UTF-8"), Some("de"));
        assert_eq!(layout_for_language("de_CH.UTF-8"), Some("ch"));
        assert_eq!(layout_for_language("pt_BR.UTF-8"), Some("br"));
        assert_eq!(layout_for_language("sr_RS@latin"), Some("rs"));
        assert_eq!(layout_for_language("en_US.UTF-8"), None);
        assert_eq!(layout_for_language("ja_JP.UTF-8"), None);
        assert_eq!(layout_for_language(""), None);
        assert_eq!(pick_locale("", "", "ru_RU.UTF-8"), "ru_RU.UTF-8");
        assert_eq!(pick_locale("C", "de_DE.UTF-8", "ru_RU.UTF-8"), "de_DE.UTF-8");
        assert_eq!(pick_locale("C.UTF-8", "", ""), "");
    }

    #[test]
    fn system_layout_wins_language_fills_in() {
        // The system is already set up: taken as is.
        assert_eq!(detect(Some(LOCALECTL_RU), None, None, "de_DE.UTF-8"), Some(kb("us,ru", "grp:alt_shift_toggle")));
        // The system's own grp: stays, a second one is not added.
        let x = "Option \"XkbLayout\" \"us,de\"\nOption \"XkbOptions\" \"grp:win_space_toggle\"\n";
        assert_eq!(detect(Some(LOCALECTL_EMPTY), Some(x), None, "ru_RU.UTF-8"), Some(kb("us,de", "grp:win_space_toggle")));
        // Empty: by language.
        assert_eq!(detect(Some(LOCALECTL_EMPTY), None, None, "ru_RU.UTF-8"), Some(kb("us,ru", ALT_SHIFT)));
        assert_eq!(detect(None, None, None, "uk_UA.UTF-8"), Some(kb("us,ua", ALT_SHIFT)));
        // A single us with options: the options are kept.
        let us = "      X11 Layout: us\n     X11 Options: ctrl:nocaps\n";
        assert_eq!(detect(Some(us), None, None, "fr_FR.UTF-8"), Some(kb("us,fr", &format!("ctrl:nocaps,{ALT_SHIFT}"))));
        // English and a single us: nothing to write; us with options: the options.
        assert_eq!(detect(Some(LOCALECTL_EMPTY), None, None, "en_US.UTF-8"), None);
        assert_eq!(detect(Some(us), None, None, "en_US.UTF-8"), Some(kb("us", "ctrl:nocaps")));
        // A single Cyrillic layout in the system: us goes first, or no command could be typed.
        let ru = "      X11 Layout: ru\n";
        assert_eq!(detect(Some(ru), None, None, "ru_RU.UTF-8"), Some(kb("us,ru", ALT_SHIFT)));
        // A single Latin one (de): as is, without switching.
        let de = "      X11 Layout: de\n";
        assert_eq!(detect(Some(de), None, None, "ru_RU.UTF-8"), Some(kb("de", "")));
    }

    #[test]
    fn merge_keeps_own_switch() {
        assert_eq!(merge_options("", ALT_SHIFT), ALT_SHIFT);
        assert_eq!(merge_options("grp:caps_toggle", ALT_SHIFT), "grp:caps_toggle");
        assert_eq!(merge_options("ctrl:nocaps", ALT_SHIFT), format!("ctrl:nocaps,{ALT_SHIFT}"));
    }

    fn ru() -> KbLayout {
        kb("us,ru", ALT_SHIFT)
    }

    fn write(a: Apply) -> String {
        match a {
            Apply::Write(t) => t,
            other => panic!("ожидалась запись, а не {other:?}"),
        }
    }

    #[test]
    fn sway_block_and_respect() {
        let out = write(apply(Desk::Sway, "include /etc/sway/config\n", &[], &ru()));
        assert_eq!(
            out,
            format!("include /etc/sway/config\n\n# {HEADER}\ninput type:keyboard {{\n    xkb_layout \"us,ru\"\n    xkb_options \"grp:alt_shift_toggle\"\n}}\n")
        );
        // The layout is the person's own, in the main or in an included file.
        assert_eq!(apply(Desk::Sway, &out, &[], &ru()), Apply::Kept);
        let own = "input * xkb_layout de\n".to_string();
        assert_eq!(apply(Desk::Sway, "include ~/.config/sway/config.d/*\n", &[own], &ru()), Apply::Kept);
        // A commented-out sample line is not the person's own.
        let sample = "#   input type:keyboard {\n#       xkb_layout \"eu\"\n#   }\n".to_string();
        assert!(matches!(apply(Desk::Sway, "x\n", &[sample], &ru()), Apply::Write(_)));
        // Own options without a layout are kept.
        let out = write(apply(Desk::Sway, "input type:keyboard xkb_options ctrl:nocaps\n", &[], &ru()));
        assert!(out.contains("xkb_options \"ctrl:nocaps,grp:alt_shift_toggle\""), "{out}");
    }

    #[test]
    fn hyprland_conf_and_lua() {
        let sample = "input {\n    kb_layout = us\n    kb_variant =\n    kb_options =\n    follow_mouse = 1\n}\n";
        let out = write(apply(Desk::HyprConf, sample, &[], &ru()));
        assert!(out.starts_with(sample));
        assert!(out.ends_with("input {\n    kb_layout = us,ru\n    kb_options = grp:alt_shift_toggle\n}\n"), "{out}");
        assert_eq!(apply(Desk::HyprConf, &out, &[], &ru()), Apply::Kept);
        assert_eq!(apply(Desk::HyprConf, "input:kb_layout = de\n", &[], &ru()), Apply::Kept);
        assert_eq!(apply(Desk::HyprConf, "x\n", &["input {\n kb_layout = fr\n}\n".into()], &ru()), Apply::Kept);

        let lua = "hl.config({\n    input = {\n        kb_layout  = \"us\",\n        kb_variant = \"\",\n        kb_options = \"\",\n    },\n})\n";
        let out = write(apply(Desk::HyprLua, lua, &[], &ru()));
        assert!(out.ends_with(&format!(
            "-- {HEADER}\nhl.config({{\n    input = {{\n        kb_layout = \"us,ru\",\n        kb_options = \"grp:alt_shift_toggle\",\n    }},\n}})\n"
        )), "{out}");
        assert_eq!(apply(Desk::HyprLua, &out, &[], &ru()), Apply::Kept);
        // Commented out in Lua is not the person's own.
        assert!(matches!(apply(Desk::HyprLua, "-- kb_layout = \"de\"\n", &[], &ru()), Apply::Write(_)));
        assert_eq!(apply(Desk::HyprLua, "hl.config({ [\"input.kb_layout\"] = \"de\" })\n", &[], &ru()), Apply::Kept);
    }

    /// The beginning of the niri sample (`default-config.kdl`): an empty `xkb` with
    /// comments and `numlock` next to it.
    const NIRI_SAMPLE: &str = "input {\n    keyboard {\n        xkb {\n            // For example:\n            // layout \"us,ru\"\n        }\n\n        numlock\n    }\n}\n\n/-output \"eDP-1\" {\n    mode \"1920x1080\"\n}\n";

    #[test]
    fn niri_inserts_into_xkb() {
        let out = write(apply(Desk::Niri, NIRI_SAMPLE, &[], &ru()));
        assert!(out.contains(&format!(
            "        xkb {{\n            // {HEADER}\n            layout \"us,ru\"\n            options \"grp:alt_shift_toggle\"\n            // For example:"
        )), "{out}");
        assert_eq!(apply(Desk::Niri, &out, &[], &ru()), Apply::Kept);
        // Own options stay, switching is added to them.
        let own = "input {\n    keyboard {\n        xkb {\n            options \"ctrl:nocaps\"\n        }\n    }\n}\n";
        let out = write(apply(Desk::Niri, own, &[], &ru()));
        assert!(out.contains("            options \"ctrl:nocaps,grp:alt_shift_toggle\"\n"), "{out}");
        assert_eq!(out.matches("options").count(), 1, "{out}");
        // A disabled `/-xkb` is not an own layout; an input node without keyboard.
        let off = "input {\n    /-keyboard {\n        xkb {\n            layout \"de\"\n        }\n    }\n    focus-follows-mouse\n}\n";
        let out = write(apply(Desk::Niri, off, &[], &ru()));
        assert!(out.starts_with("input {\n    keyboard {\n        xkb {\n            // "), "{out}");
        // No input at all: the block goes at the end.
        let out = write(apply(Desk::Niri, "layout { gaps 16; }\n", &[], &ru()));
        assert!(out.ends_with("input {\n    keyboard {\n        xkb {\n            layout \"us,ru\"\n            options \"grp:alt_shift_toggle\"\n        }\n    }\n}\n"), "{out}");
        // An empty xkb on one line.
        let out = write(apply(Desk::Niri, "input {\n    keyboard {\n        xkb {}\n    }\n}\n", &[], &ru()));
        assert!(out.contains("        xkb {\n            // "), "{out}");
        // The layout is in an included file.
        assert_eq!(apply(Desk::Niri, NIRI_SAMPLE, &["input { keyboard { xkb { layout \"de\"; } } }\n".into()], &ru()), Apply::Kept);
        // A one-line keyboard with nodes: no guessing.
        assert!(matches!(apply(Desk::Niri, "input {\n    keyboard { numlock; }\n}\n", &[], &ru()), Apply::Manual(_)));
    }

    #[test]
    fn river_line_after_header() {
        let init = "#!/bin/sh\n# mine\n\nriverctl map normal Super Return spawn foot\nexec rivertile\n";
        let out = write(apply(Desk::River, init, &[], &ru()));
        assert_eq!(
            out,
            format!("#!/bin/sh\n# mine\n\n# {HEADER}\nriverctl keyboard-layout -options grp:alt_shift_toggle us,ru\n\nriverctl map normal Super Return spawn foot\nexec rivertile\n")
        );
        assert_eq!(apply(Desk::River, &out, &[], &ru()), Apply::Kept);
        let v = KbLayout::new("us,de", ",nodeadkeys", ALT_SHIFT);
        assert!(river_snippet(&v).contains("riverctl keyboard-layout -variant ,nodeadkeys -options grp:alt_shift_toggle us,de\n"));
    }

    #[test]
    fn wayfire_input_section() {
        assert_eq!(write(apply(Desk::Wayfire, "", &[], &ru())), format!("[input]\n# {HEADER}\nxkb_layout = us,ru\nxkb_options = grp:alt_shift_toggle\n"));
        let ini = "[core]\nplugins = autostart\n\n[input]\nxkb_layout =\nmouse_accel_profile = flat\n\n[autostart]\nbar = wf-panel\n";
        let out = write(apply(Desk::Wayfire, ini, &[], &ru()));
        assert!(out.contains(&format!("[input]\n# {HEADER}\nxkb_options = grp:alt_shift_toggle\nxkb_layout = us,ru\nmouse_accel_profile = flat\n")), "{out}");
        assert_eq!(apply(Desk::Wayfire, &out, &[], &ru()), Apply::Kept);
        assert_eq!(apply(Desk::Wayfire, "[input]\nxkb_layout = de\n", &[], &ru()), Apply::Kept);
        // A key in someone else's section does not count.
        assert!(matches!(apply(Desk::Wayfire, "[core]\nxkb_layout = de\n", &[], &ru()), Apply::Write(_)));
    }

    #[test]
    fn includes_are_found() {
        assert_eq!(
            included_paths(Desk::Sway, "include /etc/sway/config\n# include /x\ninclude ~/.config/sway/config.d/*\n", "/h/.config/sway", "/h"),
            vec!["/etc/sway/config".to_string(), "/h/.config/sway/config.d/*".to_string()]
        );
        assert_eq!(included_paths(Desk::HyprConf, "source = ~/.config/hypr/input.conf\n", "/h/.config/hypr", "/h"), vec!["/h/.config/hypr/input.conf"]);
        assert_eq!(included_paths(Desk::HyprLua, "require(\"conf.input\")\n", "/h/.config/hypr", "/h"), vec!["/h/.config/hypr/conf/input.lua"]);
        assert_eq!(included_paths(Desk::Niri, "include \"input.kdl\"\n", "/h/.config/niri", "/h"), vec!["/h/.config/niri/input.kdl"]);
    }
}
