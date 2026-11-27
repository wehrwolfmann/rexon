//! A sample window frame for a Marco/Metacity window manager theme (the
//! "Window borders (MATE)" section).
//!
//! The frame of such a theme is described not by pictures but by lists of
//! drawing commands in the `metacity-1/metacity-theme-{1,2,3}.xml` file. The
//! module reads the file, assembles the frame of an active normal window and
//! translates its commands into [`DrawOp`] operations — the window executes
//! them, as with the other samples.
//!
//! Done following the description of the "Metacity theme format"
//! (`doc/theme-format.txt` in the Marco and Metacity distribution): file versions
//! and choosing the newest, the `version` attribute on elements,
//! `<frame_geometry>`, `<constant>`, `<draw_ops>` with `<include>` and `<tile>`,
//! `<frame_style>` with `<piece>` and `<button>`, `<frame_style_set>`,
//! `<window>`, expressions in coordinates and color notation.
//!
//! What is supported: the commands `line`, `rectangle`, `arc`, `clip`, `tint`,
//! `gradient`, `image` (stretching and tiling, `alpha`, `colorize`), `title`,
//! `icon`, `include`, `tile`; `gtk_box`, `gtk_arrow`, `gtk_vline` — simplified
//! (the real GTK theme engine is not run). One state is drawn: a normal focused
//! window, buttons not pressed. `theme:` icons from the icon theme, dashed
//! lines, and separate styles for dialogs, minimized and maximized windows are
//! not drawn. An unknown command is skipped; a theme from which nothing came out
//! keeps the kind's placeholder.
//!
//! The theme file is foreign input: the file size, the number and nesting of
//! elements, the depth of `include`, the length of inheritance chains, the
//! number of emitted operations and the sizes of images are limited.

use std::collections::HashMap;
use std::path::Path;

use super::{DrawOp, Palette, Rect, Rgba, SampleInput, ShadeDirection};

// ── limits for foreign input ─────────────────────────────────────────────

/// No more elements than this are read from one theme file.
const MAX_NODES: usize = 100_000;
/// Elements are not nested deeper than this.
const MAX_XML_DEPTH: usize = 32;
/// Command lists are not included into each other deeper than this (and a
/// cyclic `include` is cut off right here).
const MAX_INCLUDE_DEPTH: usize = 12;
/// A `parent` chain is never longer than this (and a cyclic one is cut off).
const MAX_PARENT_DEPTH: usize = 12;
/// No more operations than this are emitted for one sample.
const MAX_OPS: usize = 3000;
/// No more commands than this are executed for one sample (lists that include
/// each other several times grow exponentially).
const MAX_STEPS: usize = 20_000;
/// No more repetitions than this are done for one `<tile>` command.
const MAX_TILES: usize = 400;
/// A coordinate expression longer than this is not parsed.
const MAX_EXPR_LEN: usize = 512;
/// Brackets in an expression and nested colors deeper than this are not parsed.
const MAX_NESTING: usize = 16;
/// Frame geometry distances are clamped to this limit.
const MAX_DISTANCE: i64 = 200;

/// The newest format version the parser understands: the `version` attribute of
/// the elements of a version-3 file is checked against it.
const FORMAT_VERSION: (u32, u32) = (3, 5);

// ── parsing XML into a tree ──────────────────────────────────────────────

/// An element of the theme file: name, attributes, nested elements. Text is not
/// stored — the format carries data only in attributes (except the `<info>`
/// section).
#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
}

impl Node {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
}

/// How the file parsing ended.
#[derive(Debug)]
enum Parsed {
    /// A tree with the `<metacity_theme>` root.
    Tree(Node),
    /// The root requires a different format version — take the file of the lower
    /// version.
    WrongVersion,
    /// Not XML or not a theme file.
    Broken,
}

/// Whether the condition `version="[<|<=|>=|>] MAJOR.MINOR"` holds. An
/// unreadable condition is considered not to hold.
fn version_satisfied(condition: &str) -> bool {
    let c = condition.trim();
    let (op, rest) = ["<=", ">=", "=>", "=<", "<", ">", "="]
        .iter()
        .find_map(|op| c.strip_prefix(op).map(|rest| (*op, rest)))
        .unwrap_or(("=", c));
    let mut parts = rest.trim().splitn(2, '.');
    let major = parts.next().and_then(|v| v.trim().parse::<u32>().ok());
    let minor = parts.next().map(|v| v.trim().parse::<u32>().ok()).unwrap_or(Some(0));
    let (Some(major), Some(minor)) = (major, minor) else { return false };
    let (have, want) = (FORMAT_VERSION, (major, minor));
    match op {
        "<" => have < want,
        "<=" | "=<" => have <= want,
        ">" => have > want,
        ">=" | "=>" => have >= want,
        _ => have == want,
    }
}

/// The theme file tree. With `versioned` (a version-3 file) an element whose
/// `version` condition does not hold is dropped together with its children.
fn parse_xml(text: &str, versioned: bool) -> Parsed {
    use quick_xml::events::{BytesStart, Event};
    let node_of = |e: &BytesStart| -> Option<Node> {
        let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
        let mut attrs = Vec::new();
        for attr in e.attributes() {
            let attr = attr.ok()?;
            let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
            let value = attr.unescape_value().ok()?.into_owned();
            attrs.push((key, value));
        }
        Some(Node { name, attrs, children: Vec::new() })
    };
    let wanted = |node: &Node| !versioned || node.attr("version").is_none_or(version_satisfied);

    let mut reader = quick_xml::Reader::from_str(text);
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    // How many open elements are being skipped now (inside a dropped one).
    let mut skipping = 0usize;
    let mut count = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if skipping > 0 {
                    skipping += 1;
                    continue;
                }
                let Some(node) = node_of(&e) else { return Parsed::Broken };
                count += 1;
                if count > MAX_NODES || stack.len() >= MAX_XML_DEPTH || root.is_some() {
                    return Parsed::Broken;
                }
                if !wanted(&node) {
                    if stack.is_empty() {
                        return Parsed::WrongVersion;
                    }
                    skipping = 1;
                    continue;
                }
                stack.push(node);
            }
            Ok(Event::Empty(e)) => {
                if skipping > 0 {
                    continue;
                }
                let Some(node) = node_of(&e) else { return Parsed::Broken };
                count += 1;
                if count > MAX_NODES {
                    return Parsed::Broken;
                }
                if !wanted(&node) {
                    continue;
                }
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => return Parsed::Broken,
                }
            }
            Ok(Event::End(_)) => {
                if skipping > 0 {
                    skipping -= 1;
                    continue;
                }
                let Some(node) = stack.pop() else { return Parsed::Broken };
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Parsed::Broken,
        }
    }
    match root {
        Some(node) if stack.is_empty() && node.name == "metacity_theme" => Parsed::Tree(node),
        _ => Parsed::Broken,
    }
}

/// Theme file names from the newest format version to the oldest.
const THEME_FILES: &[(&str, bool)] =
    &[("metacity-theme-3.xml", true), ("metacity-theme-2.xml", false), ("metacity-theme-1.xml", false)];

/// The theme tree from the `metacity-1` directory: the file of the newest format
/// version among those that exist and are readable. The second value is the name
/// of the file taken.
fn load_tree(dir: &Path) -> Option<(Node, &'static str)> {
    THEME_FILES.iter().find_map(|(name, versioned)| {
        let text = super::read_text(&dir.join(name))?;
        match parse_xml(&text, *versioned) {
            Parsed::Tree(node) => Some((node, *name)),
            Parsed::WrongVersion | Parsed::Broken => None,
        }
    })
}

// ── a theme Marco will not load ─────────────────────────────────────────

/// Values of `function` on `<button>` per the format description
/// (`theme-format.txt` and `marco-theme.dtd` in the Marco distribution) with the
/// format version in which the button appeared (`1000 × major + minor`): four
/// buttons and six backgrounds of version 1, six buttons of version 2, two
/// backgrounds of version 3.3, `appmenu` of version 3.5. `unmaximize` is NOT in
/// this list — it is allowed only on `<menu_icon>`.
const MARCO_BUTTON_FUNCTIONS: &[(&str, u32)] = &[
    ("close", 1000), ("maximize", 1000), ("minimize", 1000), ("menu", 1000),
    ("left_left_background", 1000), ("left_middle_background", 1000), ("left_right_background", 1000),
    ("right_left_background", 1000), ("right_middle_background", 1000), ("right_right_background", 1000),
    ("shade", 2000), ("unshade", 2000), ("above", 2000), ("unabove", 2000), ("stick", 2000), ("unstick", 2000),
    ("left_single_background", 3003), ("right_single_background", 3003),
    ("appmenu", 3005),
];

/// Buttons that EVERY `<frame_style>` must describe (itself or through
/// `parent`) if the style's format version is not older than the button's
/// appearance. Button backgrounds are optional. The order is the one in which
/// Marco goes through them.
const MARCO_REQUIRED_BUTTONS: &[&str] =
    &["close", "maximize", "minimize", "menu", "appmenu", "shade", "above", "stick", "unshade", "unabove", "unstick"];

/// Mandatory button states. In Marco `prelight` falls back to `normal`, so it is
/// not required separately.
const MARCO_REQUIRED_STATES: &[&str] = &["normal", "pressed"];

/// Mandatory `<info>` fields.
const MARCO_REQUIRED_INFO: &[&str] = &["name", "author", "date", "description", "copyright"];

/// Window types for each of which the theme must name a style set
/// (`<window type=…>`); `attached` is optional.
const MARCO_REQUIRED_WINDOWS: &[&str] = &["normal", "dialog", "modal_dialog", "utility", "menu", "border"];

/// Why Marco will reject the frame theme entirely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarcoProblem {
    /// A button with a function that does not exist in the format (or in the
    /// format version of this file: `shade` in `metacity-theme-1.xml`).
    UnknownButton(String),
    /// A frame style lacks a mandatory button (an example — Win8 themes without
    /// `shade`): "<button function="shade" state="normal"…> must be specified for
    /// this frame style".
    MissingButton(String),
    /// A mandatory part of the file is missing: an `<info>` field or a
    /// `<window type=…>` (the text is as the part is written in the file).
    MissingPart(String),
}

/// The lowest format version required by a satisfied `version` condition
/// (`>= 3.2` → 3002, `> 3.2` → 3003, `<…` → 0).
fn version_floor(condition: &str) -> u32 {
    let c = condition.trim();
    let (strict, rest) = if let Some(rest) = c.strip_prefix(">=").or_else(|| c.strip_prefix("=>")) {
        (false, rest)
    } else if let Some(rest) = c.strip_prefix('>') {
        (true, rest)
    } else {
        return 0;
    };
    let mut parts = rest.trim().splitn(2, '.');
    let major = parts.next().and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
    let v = major.saturating_mul(1000).saturating_add(minor);
    if strict { v.saturating_add(1) } else { v }
}

/// The format version Marco requires of an element: from the parent plus its
/// own `version` condition.
fn required_version(node: &Node, parent: u32) -> u32 {
    parent.max(node.attr("version").map(version_floor).unwrap_or(0))
}

/// What will prevent Marco from loading the theme — in the theme file it will
/// take. With any of these errors Marco rejects the theme ENTIRELY (measured on
/// MATE with marco 1.29, 16.10.2026: "Unknown function "unmaximize" for
/// button"; Win8 themes from the store without a `shade` button) and stays with
/// the previous one, although the theme key has already been written.
///
/// `dirs` — `metacity-1` directories in Marco's search order (`~/.themes`,
/// `$XDG_DATA_HOME/themes`, the `$XDG_DATA_DIRS` directories); `read` — a file's
/// text by its full path. The file is chosen the same way as by Marco: the
/// newest format version in all directories, then an older one; a file whose
/// root requires a newer format version is skipped. Marco does not parse
/// elements with an unsatisfied `version` condition — and neither do we.
///
/// Buttons (unknown and mandatory), `<info>` fields and window types are
/// checked. A broken file, references to undescribed `draw_ops`, incomplete
/// `<frame_style_set>` sets and other parsing errors do not belong here (`None`).
pub fn marco_theme_problem(dirs: &[String], read: &dyn Fn(&str) -> Option<String>) -> Option<MarcoProblem> {
    for (index, (name, versioned)) in THEME_FILES.iter().enumerate() {
        // metacity-theme-3.xml → 3000, -2 → 2000, -1 → 1000.
        let base = 1000 * (THEME_FILES.len() - index) as u32;
        for dir in dirs {
            let Some(text) = read(&format!("{dir}/{name}")) else { continue };
            return match parse_xml(&text, *versioned) {
                Parsed::Tree(root) => problem_in(&root, base),
                Parsed::WrongVersion => continue,
                Parsed::Broken => None,
            };
        }
    }
    None
}

/// The first error of the theme tree in the order Marco will meet it: frame
/// styles in file order (first their buttons, then completeness), then the
/// `<info>` fields and window types — Marco checks them at the end of the file.
fn problem_in(root: &Node, base: u32) -> Option<MarcoProblem> {
    let format = required_version(root, base);
    let mut styles: HashMap<&str, &Node> = HashMap::new();
    for style in root.children_named("frame_style") {
        let version = required_version(style, format);
        for button in style.children_named("button") {
            let Some(function) = button.attr("function") else { continue };
            let known = MARCO_BUTTON_FUNCTIONS
                .iter()
                .find(|(f, _)| *f == function)
                .is_some_and(|(_, since)| *since <= required_version(button, version));
            if !known {
                return Some(MarcoProblem::UnknownButton(function.to_string()));
            }
        }
        if let Some(name) = style.attr("name") {
            styles.insert(name, style);
        }
        // The `parent` chain (previously described styles), the style itself first.
        let mut chain = vec![style];
        while chain.len() <= MAX_PARENT_DEPTH {
            let Some(&parent) = chain.last().copied().and_then(|s| s.attr("parent")).and_then(|p| styles.get(p)) else {
                break;
            };
            if chain.iter().any(|s| std::ptr::eq(*s, parent)) {
                break;
            }
            chain.push(parent);
        }
        let has = |function: &str, state: &str| {
            chain.iter().any(|s| {
                s.children_named("button").any(|b| b.attr("function") == Some(function) && b.attr("state") == Some(state))
            })
        };
        for function in MARCO_REQUIRED_BUTTONS {
            let since = MARCO_BUTTON_FUNCTIONS.iter().find(|(f, _)| f == function).map_or(1000, |(_, v)| *v);
            if since > version {
                continue;
            }
            if MARCO_REQUIRED_STATES.iter().any(|state| !has(function, state)) {
                return Some(MarcoProblem::MissingButton(function.to_string()));
            }
        }
    }
    let info: Vec<&str> = root.children_named("info").flat_map(|i| i.children.iter().map(|c| c.name.as_str())).collect();
    if let Some(field) = MARCO_REQUIRED_INFO.iter().find(|f| !info.contains(*f)) {
        return Some(MarcoProblem::MissingPart(format!("<{field}>")));
    }
    let windows: Vec<&str> = root.children_named("window").filter_map(|w| w.attr("type")).collect();
    MARCO_REQUIRED_WINDOWS
        .iter()
        .find(|t| !windows.contains(*t))
        .map(|t| MarcoProblem::MissingPart(format!("<window type=\"{t}\">")))
}

// ── theme: named parts ───────────────────────────────────────────────────

/// Named parts of the theme file.
struct Theme<'t> {
    constants: HashMap<&'t str, &'t str>,
    draw_ops: HashMap<&'t str, &'t Node>,
    geometries: HashMap<&'t str, &'t Node>,
    styles: HashMap<&'t str, &'t Node>,
    style_sets: HashMap<&'t str, &'t Node>,
    /// Style set of a normal window (`<window type="normal">`).
    normal_set: Option<&'t str>,
}

impl<'t> Theme<'t> {
    fn new(root: &'t Node) -> Theme<'t> {
        let mut theme = Theme {
            constants: HashMap::new(),
            draw_ops: HashMap::new(),
            geometries: HashMap::new(),
            styles: HashMap::new(),
            style_sets: HashMap::new(),
            normal_set: None,
        };
        for node in &root.children {
            let name = node.attr("name");
            match (node.name.as_str(), name) {
                ("constant", Some(name)) => {
                    if let Some(value) = node.attr("value") {
                        theme.constants.entry(name).or_insert(value);
                    }
                }
                ("draw_ops", Some(name)) => {
                    theme.draw_ops.entry(name).or_insert(node);
                }
                ("frame_geometry", Some(name)) => {
                    theme.geometries.entry(name).or_insert(node);
                }
                ("frame_style", Some(name)) => {
                    theme.styles.entry(name).or_insert(node);
                }
                ("frame_style_set", Some(name)) => {
                    theme.style_sets.entry(name).or_insert(node);
                }
                ("window", _) if node.attr("type") == Some("normal") => {
                    theme.normal_set = theme.normal_set.or(node.attr("style_set"));
                }
                _ => {}
            }
        }
        theme
    }

    /// An element and its ancestors via `parent`, from the element itself to the root.
    fn chain(&self, table: &HashMap<&'t str, &'t Node>, name: &str) -> Vec<&'t Node> {
        let mut out: Vec<&'t Node> = Vec::new();
        let mut next = table.get(name).copied();
        while let Some(node) = next {
            if out.len() >= MAX_PARENT_DEPTH || out.iter().any(|seen| std::ptr::eq(*seen, node)) {
                break;
            }
            out.push(node);
            next = node.attr("parent").and_then(|p| table.get(p).copied());
        }
        out
    }

    /// Frame style of a normal focused window: `<window type="normal">` → the style
    /// set → `<frame focus="yes" state="normal">` (first with resizing in both
    /// directions, then any) → the style chain from it to its ancestors.
    fn focused_style(&self) -> Option<Vec<&'t Node>> {
        let sets = self.chain(&self.style_sets, self.normal_set?);
        let frames = || sets.iter().flat_map(|set| set.children_named("frame"));
        let focused = |f: &&Node| f.attr("focus") == Some("yes") && f.attr("state") == Some("normal");
        let frame = frames()
            .find(|f| focused(f) && f.attr("resize") == Some("both"))
            .or_else(|| frames().find(focused))
            .or_else(|| frames().find(|f| f.attr("state") == Some("normal")))?;
        let chain = self.chain(&self.styles, frame.attr("style")?);
        (!chain.is_empty()).then_some(chain)
    }

    /// The command list of a style element: named by the `draw_ops` attribute or
    /// nested.
    fn ops_of(&self, holder: &'t Node) -> Option<&'t Node> {
        match holder.attr("draw_ops") {
            Some(name) => self.draw_ops.get(name).copied(),
            None => holder.children_named("draw_ops").next(),
        }
    }

    /// Commands of frame part `position`: on the style itself, otherwise on the
    /// nearest ancestor.
    fn piece(&self, style: &[&'t Node], position: &str) -> Option<&'t Node> {
        let holder = style
            .iter()
            .find_map(|s| s.children_named("piece").find(|p| p.attr("position") == Some(position)))?;
        self.ops_of(holder)
    }

    /// Commands of button `function` in the "not pressed" state.
    fn button(&self, style: &[&'t Node], function: &str) -> Option<&'t Node> {
        let holder = style.iter().find_map(|s| {
            s.children_named("button")
                .find(|b| b.attr("function") == Some(function) && b.attr("state") == Some("normal"))
        })?;
        self.ops_of(holder)
    }
}

// ── expressions in coordinates ───────────────────────────────────────────

/// Value of an expression: a number and an "integer" flag (integer division
/// truncates).
#[derive(Debug, Clone, Copy)]
struct Num {
    v: f64,
    int: bool,
}

/// Where an expression takes its names from: the variables of the drawing place
/// and the theme constants.
struct Scope<'a> {
    vars: &'a [(&'static str, f64)],
    constants: &'a HashMap<&'a str, &'a str>,
}

impl Scope<'_> {
    fn lookup(&self, name: &str) -> Option<Num> {
        if let Some((_, v)) = self.vars.iter().find(|(n, _)| *n == name) {
            return Some(Num { v: *v, int: true });
        }
        number(self.constants.get(name)?)
    }
}

/// A number written as `12` or `0.5`.
fn number(text: &str) -> Option<Num> {
    let t = text.trim();
    let int = !t.contains(['.', 'e', 'E']);
    let v = t.parse::<f64>().ok().filter(|v| v.is_finite())?;
    Some(Num { v, int })
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64, bool),
    Name(String),
    Op(char),
    Open,
    Close,
}

/// Expression tokens. Operators: `+ - * / %` and, in backquotes, `` `max` ``,
/// `` `min` `` (written as `M` and `m`); `` `div` `` and `` `mod` `` are accepted
/// as `/` and `%`.
fn tokenize(expr: &str) -> Option<Vec<Token>> {
    if expr.len() > MAX_EXPR_LEN {
        return None;
    }
    let chars: Vec<char> = expr.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let n = number(&text)?;
            out.push(Token::Num(n.v, n.int));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token::Name(chars[start..i].iter().collect()));
        } else if c == '`' {
            let end = chars[i + 1..].iter().position(|c| *c == '`')? + i + 1;
            let word: String = chars[i + 1..end].iter().collect();
            out.push(Token::Op(match word.as_str() {
                "max" => 'M',
                "min" => 'm',
                "div" => '/',
                "mod" => '%',
                _ => return None,
            }));
            i = end + 1;
        } else if matches!(c, '+' | '-' | '*' | '/' | '%') {
            out.push(Token::Op(c));
            i += 1;
        } else if c == '(' {
            out.push(Token::Open);
            i += 1;
        } else if c == ')' {
            out.push(Token::Close);
            i += 1;
        } else {
            return None;
        }
    }
    Some(out)
}

/// Parsing an expression by precedence: `* / %`, then `+ -`, then `max`/`min`.
struct Expr<'a> {
    tokens: &'a [Token],
    at: usize,
    scope: &'a Scope<'a>,
}

impl Expr<'_> {
    fn peek_op(&self, ops: &[char]) -> Option<char> {
        match self.tokens.get(self.at) {
            Some(Token::Op(c)) if ops.contains(c) => Some(*c),
            _ => None,
        }
    }

    /// Precedence level `level`: 0 — `max`/`min`, 1 — `+ -`, 2 — `* / %`.
    fn level(&mut self, level: usize, depth: usize) -> Option<Num> {
        const LEVELS: [&[char]; 3] = [&['M', 'm'], &['+', '-'], &['*', '/', '%']];
        if level == LEVELS.len() {
            return self.unary(depth);
        }
        let mut left = self.level(level + 1, depth)?;
        while let Some(op) = self.peek_op(LEVELS[level]) {
            self.at += 1;
            let right = self.level(level + 1, depth)?;
            left = apply(op, left, right)?;
        }
        Some(left)
    }

    fn unary(&mut self, depth: usize) -> Option<Num> {
        if self.peek_op(&['-']).is_some() {
            self.at += 1;
            let n = self.unary(depth)?;
            return Some(Num { v: -n.v, int: n.int });
        }
        if self.peek_op(&['+']).is_some() {
            self.at += 1;
            return self.unary(depth);
        }
        let token = self.tokens.get(self.at)?;
        self.at += 1;
        match token {
            Token::Num(v, int) => Some(Num { v: *v, int: *int }),
            Token::Name(name) => self.scope.lookup(name),
            Token::Open => {
                if depth >= MAX_NESTING {
                    return None;
                }
                let inner = self.level(0, depth + 1)?;
                match self.tokens.get(self.at) {
                    Some(Token::Close) => {
                        self.at += 1;
                        Some(inner)
                    }
                    _ => None,
                }
            }
            Token::Op(_) | Token::Close => None,
        }
    }
}

fn apply(op: char, a: Num, b: Num) -> Option<Num> {
    let int = a.int && b.int;
    let v = match op {
        '+' => a.v + b.v,
        '-' => a.v - b.v,
        '*' => a.v * b.v,
        '/' | '%' if b.v == 0.0 => return None,
        '/' if int => (a.v / b.v).trunc(),
        '/' => a.v / b.v,
        '%' => a.v % b.v,
        'M' => a.v.max(b.v),
        'm' => a.v.min(b.v),
        _ => return None,
    };
    v.is_finite().then_some(Num { v, int })
}

/// Value of a coordinate expression reduced to an integer number of points.
/// `None` — the expression was not parsed, a name is unknown, or division by
/// zero.
fn eval(expr: &str, scope: &Scope) -> Option<f64> {
    let tokens = tokenize(expr)?;
    let mut parser = Expr { tokens: &tokens, at: 0, scope };
    let value = parser.level(0, 0)?;
    (parser.at == tokens.len() && value.v.abs() < 1.0e6).then_some(value.v.trunc())
}

// ── colors ───────────────────────────────────────────────────────────────

type Rgb = (u8, u8, u8);

/// Colors of the GTK theme the frame theme refers to.
#[derive(Debug, Clone)]
pub struct GtkColors {
    /// The window's main colors.
    pub palette: Palette,
    /// Named colors of the stylesheet (`@define-color`) — for
    /// `gtk:custom(name,fallback)`.
    pub named: HashMap<String, String>,
}

impl GtkColors {
    /// A neutral light set — when there is no GTK theme next to the frame theme.
    pub fn fallback() -> GtkColors {
        let window = (237, 236, 235);
        GtkColors {
            palette: Palette {
                window,
                view: (255, 255, 255),
                text: (46, 52, 54),
                selection: (74, 144, 217),
                selection_text: (255, 255, 255),
                button: window,
                titlebar: window,
            },
            named: HashMap::new(),
        }
    }

    /// GTK theme colors from the theme directory: the GTK 3/4 stylesheet, and
    /// without it the GTK 2 settings; if there is neither — the neutral set.
    pub fn of_theme(theme_dir: &Path) -> GtkColors {
        for sub in ["gtk-3.0", "gtk-4.0", "gtk-3.20"] {
            let Some(css) = super::gtk_css_text(&theme_dir.join(sub)) else { continue };
            if let Some(palette) = super::gtk_css_palette(&css) {
                return GtkColors { palette, named: super::gtk_defined_colors(&css) };
            }
        }
        let gtk2 = super::read_text(&theme_dir.join("gtk-2.0/gtkrc")).and_then(|rc| super::gtk2_palette(&rc));
        match gtk2 {
            Some(palette) => GtkColors { palette, named: HashMap::new() },
            None => GtkColors::fallback(),
        }
    }

    /// Colors for the frame theme from directory `theme_dir`: its own GTK theme,
    /// and for a "borders only" set (`<name>-border`) — the GTK theme of the
    /// neighboring directory `<name>`, with which it is applied.
    pub fn for_frames(theme_dir: &Path) -> GtkColors {
        let has_gtk = |dir: &Path| ["gtk-3.0", "gtk-4.0", "gtk-3.20", "gtk-2.0"].iter().any(|sub| dir.join(sub).is_dir());
        let sibling = || {
            let name = theme_dir.file_name()?.to_str()?.strip_suffix("-border")?;
            Some(theme_dir.with_file_name(name)).filter(|dir| has_gtk(dir))
        };
        match (has_gtk(theme_dir), sibling()) {
            (false, Some(dir)) => GtkColors::of_theme(&dir),
            _ => GtkColors::of_theme(theme_dir),
        }
    }

    /// Color of part `component` (`bg`, `fg`, `base`, `text`, `light`, `dark`,
    /// `mid`, `text_aa`) in state `state` (`NORMAL`, `ACTIVE`, `PRELIGHT`,
    /// `SELECTED`, `INSENSITIVE`).
    fn component(&self, component: &str, state: &str) -> Option<Rgb> {
        let p = &self.palette;
        let bg = match state {
            "NORMAL" | "INSENSITIVE" => p.window,
            "ACTIVE" => super::mix(p.window, p.text, 0.12),
            "PRELIGHT" => super::mix(p.window, (255, 255, 255), 0.3),
            "SELECTED" => p.selection,
            _ => return None,
        };
        let ink = |normal: Rgb| match state {
            "SELECTED" => p.selection_text,
            "INSENSITIVE" => super::mix(normal, p.window, 0.5),
            _ => normal,
        };
        let base = match state {
            "SELECTED" => p.selection,
            "ACTIVE" => super::mix(p.selection, p.view, 0.5),
            "INSENSITIVE" => p.window,
            _ => p.view,
        };
        Some(match component.to_ascii_lowercase().as_str() {
            "bg" => bg,
            "fg" => ink(p.text),
            "text" => ink(p.text),
            "base" => base,
            "light" => shade(bg, 1.3),
            "dark" => shade(bg, 0.7),
            "mid" => super::mix(shade(bg, 1.3), shade(bg, 0.7), 0.5),
            "text_aa" => super::mix(ink(p.text), base, 0.5),
            _ => return None,
        })
    }
}

/// A color lighter or darker: lightness and saturation are multiplied by `factor`.
fn shade(color: Rgb, factor: f64) -> Rgb {
    let (r, g, b) = (color.0 as f64 / 255.0, color.1 as f64 / 255.0, color.2 as f64 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let mut l = (max + min) / 2.0;
    let (mut h, mut s) = (0.0, 0.0);
    if max > min {
        let d = max - min;
        s = if l <= 0.5 { d / (max + min) } else { d / (2.0 - max - min) };
        h = if max == r {
            (g - b) / d
        } else if max == g {
            2.0 + (b - r) / d
        } else {
            4.0 + (r - g) / d
        } * 60.0;
        if h < 0.0 {
            h += 360.0;
        }
    }
    l = (l * factor).clamp(0.0, 1.0);
    s = (s * factor).clamp(0.0, 1.0);
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if s == 0.0 {
        return (byte(l), byte(l), byte(l));
    }
    let m2 = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let m1 = 2.0 * l - m2;
    let channel = |hue: f64| {
        let hue = hue.rem_euclid(360.0);
        if hue < 60.0 {
            m1 + (m2 - m1) * hue / 60.0
        } else if hue < 180.0 {
            m2
        } else if hue < 240.0 {
            m1 + (m2 - m1) * (240.0 - hue) / 60.0
        } else {
            m1
        }
    };
    (byte(channel(h + 120.0)), byte(channel(h)), byte(channel(h - 120.0)))
}

/// A color by name: common names and the gray shades `grayN` (0–100).
fn named_color(name: &str) -> Option<Rgb> {
    let key: String = name.chars().filter(|c| !c.is_whitespace() && *c != '_').collect::<String>().to_ascii_lowercase();
    let key = key.replace("grey", "gray");
    if let Some(level) = key.strip_prefix("gray").and_then(|n| n.parse::<u32>().ok()) {
        let v = (level.min(100) as f64 * 2.55).round() as u8;
        return Some((v, v, v));
    }
    Some(match key.as_str() {
        "white" => (255, 255, 255),
        "black" => (0, 0, 0),
        "gray" => (190, 190, 190),
        "darkgray" => (169, 169, 169),
        "lightgray" => (211, 211, 211),
        "dimgray" => (105, 105, 105),
        "silver" => (192, 192, 192),
        "red" => (255, 0, 0),
        "darkred" => (139, 0, 0),
        "green" => (0, 255, 0),
        "darkgreen" => (0, 100, 0),
        "blue" => (0, 0, 255),
        "darkblue" => (0, 0, 139),
        "navy" => (0, 0, 128),
        "lightblue" => (173, 216, 230),
        "yellow" => (255, 255, 0),
        "cyan" => (0, 255, 255),
        "magenta" => (255, 0, 255),
        "orange" => (255, 165, 0),
        "brown" => (165, 42, 42),
        "pink" => (255, 192, 203),
        "purple" => (160, 32, 240),
        _ => return None,
    })
}

/// Parsing a color notation: `#rrggbb`, a name, `rgb(…)`/`rgba(…)`,
/// `gtk:part[STATE]`, `gtk:custom(name,fallback)`, `blend/bottom/top/fraction`,
/// `shade/color/factor`, the name of a theme color constant.
struct ColorReader<'a> {
    text: &'a str,
    at: usize,
    gtk: &'a GtkColors,
    constants: &'a HashMap<&'a str, &'a str>,
}

impl<'a> ColorReader<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    fn eat(&mut self, prefix: &str) -> bool {
        let hit = self.rest().starts_with(prefix);
        if hit {
            self.at += prefix.len();
        }
        hit
    }

    /// The piece up to the first of `stops` (the sign itself is not taken).
    fn until(&mut self, stops: &[char]) -> &'a str {
        let rest = self.rest();
        let len = rest.find(stops).unwrap_or(rest.len());
        self.at += len;
        &rest[..len]
    }

    fn factor(&mut self) -> Option<f64> {
        self.until(&['/', ',', ')']).trim().parse::<f64>().ok().filter(|v| v.is_finite())
    }

    fn color(&mut self, depth: usize) -> Option<Rgba> {
        if depth > MAX_NESTING {
            return None;
        }
        self.at += self.rest().len() - self.rest().trim_start().len();
        if self.eat("blend/") {
            let under = self.color(depth + 1)?;
            self.eat("/").then_some(())?;
            let over = self.color(depth + 1)?;
            self.eat("/").then_some(())?;
            let t = self.factor()?.clamp(0.0, 1.0);
            let ch = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
            return Some((ch(under.0, over.0), ch(under.1, over.1), ch(under.2, over.2), ch(under.3, over.3)));
        }
        if self.eat("shade/") {
            let base = self.color(depth + 1)?;
            self.eat("/").then_some(())?;
            let (r, g, b) = shade((base.0, base.1, base.2), self.factor()?.clamp(0.0, 10.0));
            return Some((r, g, b, base.3));
        }
        if self.eat("gtk:custom(") {
            let name = self.until(&[',', ')']).trim();
            self.eat(",").then_some(())?;
            let fallback = self.color(depth + 1);
            self.at += self.rest().len() - self.rest().trim_start().len();
            self.eat(")").then_some(())?;
            let own = self.gtk.named.get(name).and_then(|v| super::gtk_resolve(&self.gtk.named, v, 4));
            return own.map(|(r, g, b)| (r, g, b, 255)).or(fallback);
        }
        if self.eat("gtk:") {
            let component = self.until(&['[']).trim();
            self.eat("[").then_some(())?;
            let state = self.until(&[']']).trim().to_ascii_uppercase();
            self.eat("]").then_some(())?;
            let (r, g, b) = self.gtk.component(component, &state)?;
            return Some((r, g, b, 255));
        }
        if self.rest().to_ascii_lowercase().starts_with("rgb") {
            let rest = self.rest();
            let len = rest.find(')')? + 1;
            self.at += len;
            return super::css_color_alpha(&rest[..len]);
        }
        let atom = self.until(&['/', ',', ')']).trim();
        if atom.starts_with('#') {
            return super::css_color_alpha(atom);
        }
        if let Some(value) = self.constants.get(atom) {
            return parse_color_at(value, self.gtk, self.constants, depth + 1);
        }
        named_color(atom).map(|(r, g, b)| (r, g, b, 255))
    }
}

fn parse_color_at(text: &str, gtk: &GtkColors, constants: &HashMap<&str, &str>, depth: usize) -> Option<Rgba> {
    if text.len() > MAX_EXPR_LEN {
        return None;
    }
    let mut reader = ColorReader { text, at: 0, gtk, constants };
    let color = reader.color(depth)?;
    reader.rest().trim().is_empty().then_some(color)
}

/// A color from the theme's notation; `None` — the notation was not parsed.
fn parse_color(text: &str, gtk: &GtkColors, constants: &HashMap<&str, &str>) -> Option<Rgba> {
    parse_color_at(text, gtk, constants, 0)
}

// ── frame geometry ───────────────────────────────────────────────────────

/// Size of a title bar button: given explicitly or by aspect ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonSize {
    /// Width and height.
    Fixed(i64, i64),
    /// Height by the title bar height, width — the height divided by the number.
    Aspect(f64),
}

/// Frame geometry (`<frame_geometry>` with all its ancestors).
#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    /// Left, right and bottom sides of the frame.
    pub left_width: i64,
    pub right_width: i64,
    pub bottom_height: i64,
    /// Title bar edges: left, right, top, bottom.
    pub left_titlebar_edge: i64,
    pub right_titlebar_edge: i64,
    pub top_titlebar_edge: i64,
    pub bottom_titlebar_edge: i64,
    /// Padding above and below the title bar contents.
    pub title_vertical_pad: i64,
    /// Padding of the window title and the button: left, right, top, bottom.
    pub title_border: [i64; 4],
    pub button_border: [i64; 4],
    pub button: ButtonSize,
    /// Whether the text height is included in the title bar height.
    pub has_title: bool,
    /// Font size multiplier of the window title.
    pub title_scale: f64,
    /// Corner radii: top left, top right, bottom left, bottom right.
    pub rounded: [f64; 4],
    pub hide_buttons: bool,
}

impl Default for Geometry {
    fn default() -> Geometry {
        Geometry {
            left_width: 0,
            right_width: 0,
            bottom_height: 0,
            left_titlebar_edge: 0,
            right_titlebar_edge: 0,
            top_titlebar_edge: 0,
            bottom_titlebar_edge: 0,
            title_vertical_pad: 0,
            title_border: [0; 4],
            button_border: [0; 4],
            button: ButtonSize::Aspect(1.0),
            has_title: true,
            title_scale: 1.0,
            rounded: [0.0; 4],
            hide_buttons: false,
        }
    }
}

/// An integer distance: a number or the name of an integer theme constant.
fn distance(value: &str, constants: &HashMap<&str, &str>) -> Option<i64> {
    let text = constants.get(value.trim()).copied().unwrap_or(value);
    let n = number(text)?;
    Some((n.v.trunc() as i64).clamp(0, MAX_DISTANCE))
}

/// Font size multiplier by size word (`xx-small` … `xx-large`).
fn title_scale(word: &str) -> Option<f64> {
    Some(match word.trim() {
        "xx-small" => 0.5787,
        "x-small" => 0.6944,
        "small" => 0.8333,
        "medium" => 1.0,
        "large" => 1.2,
        "x-large" => 1.44,
        "xx-large" => 1.728,
        _ => return None,
    })
}

impl Geometry {
    /// Geometry along the chain of elements from the element itself to its
    /// ancestors: an ancestor's values are overridden by the descendant.
    fn from_chain(chain: &[&Node], constants: &HashMap<&str, &str>) -> Geometry {
        let mut g = Geometry::default();
        let truth = |v: &str| matches!(v.trim(), "true" | "TRUE" | "yes" | "1");
        for node in chain.iter().rev() {
            if let Some(v) = node.attr("has_title") {
                g.has_title = truth(v);
            }
            if let Some(v) = node.attr("hide_buttons") {
                g.hide_buttons = truth(v);
            }
            if let Some(scale) = node.attr("title_scale").and_then(title_scale) {
                g.title_scale = scale;
            }
            for (i, key) in ["rounded_top_left", "rounded_top_right", "rounded_bottom_left", "rounded_bottom_right"]
                .iter()
                .enumerate()
            {
                // The word "true" — radius 5, "false" — 0; otherwise the radius as a number.
                if let Some(v) = node.attr(key) {
                    g.rounded[i] = match v.trim() {
                        "true" => 5.0,
                        "false" => 0.0,
                        other => distance(other, constants).unwrap_or(0).min(40) as f64,
                    };
                }
            }
            for child in &node.children {
                let name = child.attr("name").unwrap_or("");
                match child.name.as_str() {
                    "distance" => {
                        let Some(v) = child.attr("value").and_then(|v| distance(v, constants)) else { continue };
                        let (bw, bh) = match g.button {
                            ButtonSize::Fixed(w, h) => (w, h),
                            ButtonSize::Aspect(_) => (0, 0),
                        };
                        match name {
                            "left_width" => g.left_width = v,
                            "right_width" => g.right_width = v,
                            "bottom_height" => g.bottom_height = v,
                            "left_titlebar_edge" => g.left_titlebar_edge = v,
                            "right_titlebar_edge" => g.right_titlebar_edge = v,
                            "top_titlebar_edge" => g.top_titlebar_edge = v,
                            "bottom_titlebar_edge" => g.bottom_titlebar_edge = v,
                            "title_vertical_pad" => g.title_vertical_pad = v,
                            "button_width" => g.button = ButtonSize::Fixed(v, bh),
                            "button_height" => g.button = ButtonSize::Fixed(bw, v),
                            _ => {}
                        }
                    }
                    "aspect_ratio" if name == "button" => {
                        if let Some(n) = child.attr("value").and_then(number).filter(|n| n.v > 0.05 && n.v < 20.0) {
                            g.button = ButtonSize::Aspect(n.v);
                        }
                    }
                    "border" => {
                        let side = |key: &str| child.attr(key).and_then(|v| distance(v, constants)).unwrap_or(0);
                        let sides = [side("left"), side("right"), side("top"), side("bottom")];
                        match name {
                            "title_border" => g.title_border = sides,
                            "button_border" => g.button_border = sides,
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        g
    }
}

/// A rectangle in integer points: `(x, y, width, height)`.
type Area = (i64, i64, i64, i64);

/// Layout of a frame of a given size.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub width: i64,
    pub height: i64,
    /// Title bar height (the distance from the top of the frame to the window contents).
    pub top_height: i64,
    /// Place of the window title.
    pub title: Area,
    /// Places of the buttons on the left and on the right, left to right.
    pub left_buttons: Vec<Area>,
    pub right_buttons: Vec<Area>,
}

/// Height of the window title line with font size multiplier 1.
const TITLE_TEXT_HEIGHT: f64 = 17.0;
/// Font size of the window title in screen points with multiplier 1.
const TITLE_PIXEL_SIZE: f64 = 13.0;

impl Geometry {
    fn text_height(&self) -> i64 {
        (TITLE_TEXT_HEIGHT * self.title_scale).round() as i64
    }

    /// Layout of a `width`×`height` frame with `left` buttons on the left and
    /// `right` on the right. The title bar height is the larger of "the title line
    /// with its padding and the `title_vertical_pad` addition" and "a button with its
    /// padding"; a button given by aspect ratio takes the title bar's height minus
    /// its padding. Buttons stand from the title bar edges towards the middle, the
    /// title between them.
    pub fn layout(&self, width: i64, height: i64, left: usize, right: usize) -> Layout {
        let (tb, bb) = (self.title_border, self.button_border);
        let text = if self.has_title { self.text_height() } else { 0 };
        let title_block = text + self.title_vertical_pad + tb[2] + tb[3];
        let (bw, bh) = match self.button {
            ButtonSize::Fixed(w, h) => (w, h),
            ButtonSize::Aspect(ratio) => {
                let h = (title_block - bb[2] - bb[3]).max(1);
                (((h as f64) / ratio).round() as i64, h)
            }
        };
        let buttons_block = bh + bb[2] + bb[3];
        let top_height = title_block.max(buttons_block);
        let button_y = (top_height - buttons_block) / 2 + bb[2];
        let (left, right) = if self.hide_buttons { (0, 0) } else { (left, right) };
        let mut left_buttons = Vec::new();
        let mut x = self.left_titlebar_edge;
        for _ in 0..left {
            left_buttons.push((x + bb[0], button_y, bw, bh));
            x += bb[0] + bw + bb[1];
        }
        let left_end = x;
        let mut right_buttons = Vec::new();
        let mut x = width - self.right_titlebar_edge;
        for _ in 0..right {
            x -= bb[1] + bw;
            right_buttons.push((x, button_y, bw, bh));
            x -= bb[0];
        }
        right_buttons.reverse();
        let title = (
            left_end + tb[0],
            tb[2],
            (x - left_end - tb[0] - tb[1]).max(0),
            (top_height - tb[2] - tb[3]).max(0),
        );
        Layout { width, height, top_height, title, left_buttons, right_buttons }
    }
}

// ── executing command lists ──────────────────────────────────────────────

/// What is known while executing the commands of one frame.
struct Painter<'t> {
    theme: &'t Theme<'t>,
    gtk: &'t GtkColors,
    /// Directory of the theme's images (`metacity-1`).
    dir: &'t Path,
    /// Top left corner of the frame on the tile picture.
    origin: (f64, f64),
    layout: &'t Layout,
    geometry: &'t Geometry,
    /// The window title and its size on the picture.
    title: &'t str,
    title_size: (i64, i64),
    out: Vec<DrawOp>,
    /// Sizes of theme images: one image occurs many times in the commands.
    image_sizes: HashMap<String, Option<(f64, f64)>>,
    /// Lists being executed right now: a list that includes itself (including via
    /// others) is not started a second time.
    running: Vec<&'t Node>,
    /// How many commands have been executed already.
    steps: usize,
}

/// Side of the window icon in the title bar and of the large icon.
const MINI_ICON: f64 = 16.0;
const ICON: f64 = 32.0;

impl<'t> Painter<'t> {
    fn full(&self) -> bool {
        self.out.len() >= MAX_OPS
    }

    fn push(&mut self, op: DrawOp) {
        if !self.full() {
            self.out.push(op);
        }
    }

    fn fill(&mut self, area: Area, at: (f64, f64, f64, f64), color: Rgba) {
        if at.2 > 0.0 && at.3 > 0.0 && color.3 > 0 {
            self.push(DrawOp::Fill { rect: (self.origin.0 + area.0 as f64 + at.0, self.origin.1 + area.1 as f64 + at.1, at.2, at.3), color });
        }
    }

    fn rect_in(&self, area: Area, at: (f64, f64, f64, f64)) -> Rect {
        (self.origin.0 + area.0 as f64 + at.0, self.origin.1 + area.1 as f64 + at.1, at.2, at.3)
    }

    fn color(&self, node: &Node, key: &str) -> Option<Rgba> {
        parse_color(node.attr(key)?, self.gtk, &self.theme.constants)
    }

    /// Size of a theme image by file name; `None` — there is no file, the name leads
    /// outside the theme directory, or the size was not read.
    fn image(&mut self, filename: &str) -> Option<(String, f64, f64)> {
        if filename.is_empty() || filename.starts_with('/') || filename.starts_with("theme:") || filename.split('/').any(|p| p == "..") {
            return None;
        }
        let path = self.dir.join(filename);
        let key = path.to_string_lossy().into_owned();
        let size = *self.image_sizes.entry(key.clone()).or_insert_with(|| super::image_size(&path));
        size.map(|(w, h)| (key, w, h))
    }

    /// Commands of list `list` in place `area`, clipped by its bounds. `true` — the
    /// list drew something.
    fn in_area(&mut self, list: &'t Node, area: Area) -> bool {
        if area.2 <= 0 || area.3 <= 0 || self.full() {
            return false;
        }
        let clip = self.rect_in(area, (0.0, 0.0, area.2 as f64, area.3 as f64));
        let mark = self.out.len();
        self.out.push(DrawOp::Clip { rect: clip, radii: [0.0; 4] });
        self.run(list, area, 0);
        if self.out.len() == mark + 1 {
            self.out.pop();
            return false;
        }
        self.out.push(DrawOp::Unclip);
        true
    }

    /// Commands of list `list` in place `area` (in frame points).
    fn run(&mut self, list: &'t Node, area: Area, depth: usize) {
        if depth > MAX_INCLUDE_DEPTH || self.running.iter().any(|seen| std::ptr::eq(*seen, list)) {
            return;
        }
        self.running.push(list);
        // How many clips are opened by the `<clip>` commands of this list.
        let mut clips = 0usize;
        for op in &list.children {
            if self.full() || self.steps >= MAX_STEPS {
                break;
            }
            self.steps += 1;
            if self.one(op, area, depth) {
                clips += 1;
            }
        }
        for _ in 0..clips {
            self.out.push(DrawOp::Unclip);
        }
        self.running.pop();
    }

    /// One command. `true` — the command opened a clip until the end of the list.
    fn one(&mut self, op: &'t Node, area: Area, depth: usize) -> bool {
        let (frame_w, frame_h) = (self.layout.width as f64, self.layout.height as f64);
        let object = match op.name.as_str() {
            "image" => op.attr("filename").and_then(|f| self.image(f)),
            _ => None,
        };
        let (object_w, object_h) = match (&object, op.name.as_str()) {
            (Some((_, w, h)), _) => (*w, *h),
            (None, "icon") => (MINI_ICON, MINI_ICON),
            _ => (0.0, 0.0),
        };
        let vars = [
            ("width", area.2 as f64),
            ("height", area.3 as f64),
            ("object_width", object_w),
            ("object_height", object_h),
            ("left_width", self.geometry.left_width as f64),
            ("right_width", self.geometry.right_width as f64),
            ("top_height", self.layout.top_height as f64),
            ("bottom_height", self.geometry.bottom_height as f64),
            ("mini_icon_width", MINI_ICON),
            ("mini_icon_height", MINI_ICON),
            ("icon_width", ICON),
            ("icon_height", ICON),
            ("title_width", self.title_size.0 as f64),
            ("title_height", self.title_size.1 as f64),
            ("frame_x_center", (frame_w / 2.0).trunc() - area.0 as f64),
            ("frame_y_center", (frame_h / 2.0).trunc() - area.1 as f64),
        ];
        let theme = self.theme;
        let scope = Scope { vars: &vars, constants: &theme.constants };
        let num = |key: &str| op.attr(key).and_then(|e| eval(e, &scope));
        let or = |key: &str, default: f64| match op.attr(key) {
            Some(e) => eval(e, &scope),
            None => Some(default),
        };
        let real = |key: &str| op.attr(key).and_then(|v| v.split(':').next()).and_then(number).map(|n| n.v);
        let flag = |key: &str| op.attr(key).is_some_and(|v| v.trim() == "true");
        // Place of a command: `x`, `y`, `width`, `height`; whatever is missing — the
        // whole place.
        let place = || -> Option<(f64, f64, f64, f64)> {
            Some((or("x", 0.0)?, or("y", 0.0)?, or("width", area.2 as f64)?, or("height", area.3 as f64)?))
        };
        match op.name.as_str() {
            "line" => {
                let (Some(color), Some(x1), Some(y1), Some(x2), Some(y2)) =
                    (self.color(op, "color"), num("x1"), num("y1"), num("x2"), num("y2"))
                else {
                    return false;
                };
                let thick = or("width", 1.0).unwrap_or(1.0).clamp(1.0, 64.0);
                let shift = (thick / 2.0).floor();
                if y1 == y2 {
                    // A straight line along points: both end points are included.
                    self.fill(area, (x1.min(x2), y1 - shift, (x1 - x2).abs() + 1.0, thick), color);
                } else if x1 == x2 {
                    self.fill(area, (x1 - shift, y1.min(y2), thick, (y1 - y2).abs() + 1.0), color);
                } else {
                    let (ox, oy) = (self.origin.0 + area.0 as f64, self.origin.1 + area.1 as f64);
                    self.push(DrawOp::Line { from: (ox + x1, oy + y1), to: (ox + x2, oy + y2), width: thick, color });
                }
            }
            "rectangle" => {
                let (Some(color), Some((x, y, w, h))) = (self.color(op, "color"), place()) else { return false };
                if flag("filled") {
                    self.fill(area, (x, y, w, h), color);
                } else if w >= 0.0 && h >= 0.0 {
                    // A one-point outline: the end points are included.
                    self.fill(area, (x, y, w + 1.0, 1.0), color);
                    self.fill(area, (x, y + h, w + 1.0, 1.0), color);
                    self.fill(area, (x, y + 1.0, 1.0, h - 1.0), color);
                    self.fill(area, (x + w, y + 1.0, 1.0, h - 1.0), color);
                }
            }
            "arc" => {
                let (Some(color), Some((x, y, w, h))) = (self.color(op, "color"), place()) else { return false };
                let angle = |key: &str| real(key);
                let start = angle("start_angle").or(angle("from")).unwrap_or(0.0);
                let extent = angle("extent_angle").or(angle("to").map(|to| to - start)).unwrap_or(360.0);
                if w > 0.0 && h > 0.0 {
                    let rect = self.rect_in(area, (x, y, w, h));
                    self.push(DrawOp::Arc { rect, start, extent: extent.clamp(-360.0, 360.0), filled: flag("filled"), color });
                }
            }
            "clip" => {
                let Some(at) = place() else { return false };
                let rect = self.rect_in(area, (at.0, at.1, at.2.max(0.0), at.3.max(0.0)));
                if self.full() {
                    return false;
                }
                self.out.push(DrawOp::Clip { rect, radii: [0.0; 4] });
                return true;
            }
            "tint" => {
                let (Some(color), Some(at)) = (self.color(op, "color"), place()) else { return false };
                let alpha = real("alpha").unwrap_or(1.0).clamp(0.0, 1.0);
                self.fill(area, at, (color.0, color.1, color.2, (color.3 as f64 * alpha).round() as u8));
            }
            "gradient" => {
                let Some(at) = place() else { return false };
                let alpha = real("alpha").unwrap_or(1.0).clamp(0.0, 1.0);
                let colors: Vec<Rgba> = op
                    .children_named("color")
                    .take(32)
                    .filter_map(|c| self.color(c, "value"))
                    .map(|c| (c.0, c.1, c.2, (c.3 as f64 * alpha).round() as u8))
                    .collect();
                let direction = match op.attr("type") {
                    Some("horizontal") => ShadeDirection::Horizontal,
                    Some("diagonal") => ShadeDirection::Diagonal,
                    _ => ShadeDirection::Vertical,
                };
                if at.2 > 0.0 && at.3 > 0.0 {
                    match colors.len() {
                        0 => {}
                        1 => self.fill(area, at, colors[0]),
                        _ => {
                            let rect = self.rect_in(area, at);
                            self.push(DrawOp::Shade { rect, direction, colors });
                        }
                    }
                }
            }
            "image" => {
                let Some((path, w, h)) = object else { return false };
                let (Some(x), Some(y), Some(width), Some(height)) = (or("x", 0.0), or("y", 0.0), or("width", w), or("height", h)) else {
                    return false;
                };
                if width > 0.0 && height > 0.0 {
                    let rect = self.rect_in(area, (x, y, width, height));
                    self.push(DrawOp::Picture {
                        rect,
                        path,
                        tile: op.attr("fill_type") == Some("tile"),
                        alpha: real("alpha").unwrap_or(1.0).clamp(0.0, 1.0),
                        colorize: op.attr("colorize").and_then(|_| self.color(op, "colorize")),
                    });
                }
            }
            "title" => {
                let (Some(color), Some(x), Some(y)) = (self.color(op, "color"), or("x", 0.0), or("y", 0.0)) else { return false };
                if !self.title.is_empty() {
                    let rect = self.rect_in(area, (x, y, self.title_size.0 as f64, self.title_size.1 as f64));
                    let pixel_size = (TITLE_PIXEL_SIZE * self.geometry.title_scale).round() as i64;
                    self.push(DrawOp::Label { rect, text: self.title.to_string(), pixel_size, bold: true, color });
                }
            }
            "icon" => {
                let Some((x, y, w, h)) = place() else { return false };
                self.window_icon(area, (x, y, w, h), real("alpha").unwrap_or(1.0).clamp(0.0, 1.0));
            }
            "gtk_box" => {
                let Some((x, y, w, h)) = place() else { return false };
                self.bevel(area, (x, y, w, h), op.attr("state"), op.attr("shadow") == Some("in"));
            }
            "gtk_vline" => {
                let (Some(x), Some(y1), Some(y2)) = (num("x"), num("y1"), num("y2")) else { return false };
                let state = gtk_state(op.attr("state"));
                let side = |painter: &Self, part: &str| painter.gtk.component(part, state).map(|(r, g, b)| (r, g, b, 255));
                if let (Some(dark), Some(light)) = (side(self, "dark"), side(self, "light")) {
                    self.fill(area, (x, y1.min(y2), 1.0, (y1 - y2).abs()), dark);
                    self.fill(area, (x + 1.0, y1.min(y2), 1.0, (y1 - y2).abs()), light);
                }
            }
            "gtk_arrow" => {
                let Some((x, y, w, h)) = place() else { return false };
                let ink = self.gtk.component("fg", gtk_state(op.attr("state"))).map(|(r, g, b)| (r, g, b, 255));
                if let Some(ink) = ink {
                    self.arrow(area, (x, y, w, h), op.attr("arrow").unwrap_or("down"), ink);
                }
            }
            "include" => {
                let (Some(name), Some((x, y, w, h))) = (op.attr("name"), place()) else { return false };
                if let Some(list) = self.theme.draw_ops.get(name).copied() {
                    self.run(list, (area.0 + x as i64, area.1 + y as i64, w as i64, h as i64), depth + 1);
                }
            }
            "tile" => {
                let (Some(name), Some((x, y, w, h))) = (op.attr("name"), place()) else { return false };
                let (Some(tw), Some(th)) = (num("tile_width"), num("tile_height")) else { return false };
                let (Some(dx), Some(dy)) = (or("tile_xoffset", 0.0), or("tile_yoffset", 0.0)) else { return false };
                let Some(list) = self.theme.draw_ops.get(name).copied() else { return false };
                if tw < 1.0 || th < 1.0 || w <= 0.0 || h <= 0.0 || (w / tw).ceil() * (h / th).ceil() > MAX_TILES as f64 {
                    return false;
                }
                let rect = self.rect_in(area, (x, y, w, h));
                self.push(DrawOp::Clip { rect, radii: [0.0; 4] });
                let mut ty = y - dy.rem_euclid(th);
                while ty < y + h {
                    let mut tx = x - dx.rem_euclid(tw);
                    while tx < x + w {
                        self.run(list, (area.0 + tx as i64, area.1 + ty as i64, tw as i64, th as i64), depth + 1);
                        tx += tw;
                    }
                    ty += th;
                }
                self.out.push(DrawOp::Unclip);
            }
            // An unknown command — skipped: the rest is drawn.
            _ => {}
        }
        false
    }

    /// The window icon: the sample has no real window, so a stylized small
    /// window in the GTK theme's colors is drawn.
    fn window_icon(&mut self, area: Area, at: (f64, f64, f64, f64), alpha: f64) {
        let (x, y, w, h) = at;
        if w < 4.0 || h < 4.0 {
            return;
        }
        let a = (alpha * 255.0).round() as u8;
        let p = self.gtk.palette;
        let with = |c: Rgb| (c.0, c.1, c.2, a);
        let bar = (h / 4.0).round().max(1.0);
        self.fill(area, (x, y, w, h), with(super::mix(p.view, p.text, 0.55)));
        self.fill(area, (x + 1.0, y + 1.0, w - 2.0, h - 2.0), with(p.view));
        self.fill(area, (x + 1.0, y + 1.0, w - 2.0, bar), with(p.selection));
    }

    /// A simplified GTK theme button frame: a fill and light and dark edges.
    fn bevel(&mut self, area: Area, at: (f64, f64, f64, f64), state: Option<&str>, sunken: bool) {
        let (x, y, w, h) = at;
        let state = gtk_state(state);
        let part = |name: &str| self.gtk.component(name, state).map(|(r, g, b)| (r, g, b, 255));
        let (Some(bg), Some(light), Some(dark)) = (part("bg"), part("light"), part("dark")) else { return };
        let (first, second) = if sunken { (dark, light) } else { (light, dark) };
        self.fill(area, (x, y, w, h), bg);
        self.fill(area, (x, y, w, 1.0), first);
        self.fill(area, (x, y, 1.0, h), first);
        self.fill(area, (x, y + h - 1.0, w, 1.0), second);
        self.fill(area, (x + w - 1.0, y, 1.0, h), second);
    }

    /// A simplified arrow: a stepped triangle made of strips.
    fn arrow(&mut self, area: Area, at: (f64, f64, f64, f64), direction: &str, ink: Rgba) {
        let (x, y, w, h) = at;
        let size = w.min(h).min(24.0);
        let rows = (size / 2.0).floor();
        if rows < 2.0 {
            return;
        }
        let (cx, cy) = (x + (w / 2.0).floor(), y + ((h - rows) / 2.0).floor());
        let vertical = matches!(direction, "up" | "down");
        for i in 0..rows as i64 {
            let step = i as f64;
            // Strip number i: one point wide at the tip, wide at the base.
            let grow = if matches!(direction, "up" | "left") { step } else { rows - 1.0 - step };
            if vertical {
                self.fill(area, (cx - grow, cy + step, grow * 2.0 + 1.0, 1.0), ink);
            } else {
                let (lx, ly) = (x + ((w - rows) / 2.0).floor(), y + (h / 2.0).floor());
                self.fill(area, (lx + step, ly - grow, 1.0, grow * 2.0 + 1.0), ink);
            }
        }
    }
}

/// GTK theme state by the command's word (`normal`, `active`, `prelight`,
/// `selected`, `insensitive`).
fn gtk_state(word: Option<&str>) -> &'static str {
    match word.map(|w| w.trim().to_ascii_lowercase()).as_deref() {
        Some("active") => "ACTIVE",
        Some("prelight") => "PRELIGHT",
        Some("selected") => "SELECTED",
        Some("insensitive") => "INSENSITIVE",
        _ => "NORMAL",
    }
}

// ── the whole frame ──────────────────────────────────────────────────────

/// Title bar buttons of the sample: the window menu on the left, "minimize",
/// "maximize", "close" on the right — MATE's default layout.
const LEFT_BUTTONS: &[&str] = &["menu"];
const RIGHT_BUTTONS: &[&str] = &["minimize", "maximize", "close"];

/// Average glyph width of the bold window title as a fraction of the font size.
const TITLE_CHAR_WIDTH: f64 = 0.62;

/// The window title truncated to a width of `room` points, and its width.
fn fitted_title(name: &str, pixel_size: f64, room: i64) -> (String, i64) {
    let step = pixel_size * TITLE_CHAR_WIDTH;
    let fits = (room as f64 / step).floor() as usize;
    let count = name.chars().count();
    let text: String = if count <= fits {
        name.to_string()
    } else if fits >= 4 {
        name.chars().take(fits - 1).chain(std::iter::once('…')).collect()
    } else {
        String::new()
    };
    let width = (text.chars().count() as f64 * step).round() as i64;
    (text, width)
}

/// Operations of the theme's active window frame on rectangle `rect` of the
/// picture: the shadow, the window contents, the frame parts in order, the
/// buttons. `None` — the theme has no style for a normal focused window, or not
/// a single operation came out of it.
fn frame_ops(root: &Node, gtk: &GtkColors, dir: &Path, name: &str, rect: Rect) -> Option<Vec<DrawOp>> {
    let theme = Theme::new(root);
    let style = theme.focused_style()?;
    let geometry_name = style.iter().find_map(|s| s.attr("geometry"))?;
    let geometry = Geometry::from_chain(&theme.chain(&theme.geometries, geometry_name), &theme.constants);
    let (x, y, w, h) = rect;
    let (width, height) = (w as i64, h as i64);
    let layout = geometry.layout(width, height, LEFT_BUTTONS.len(), RIGHT_BUTTONS.len());
    // A title bar taller than half the sample — the frame does not fit into the picture.
    if layout.top_height < 1 || layout.top_height > height / 2 || geometry.left_width + geometry.right_width > width / 2 {
        return None;
    }
    let pixel_size = TITLE_PIXEL_SIZE * geometry.title_scale;
    let (title, title_width) = fitted_title(name, pixel_size, layout.title.2);
    let mut painter = Painter {
        theme: &theme,
        gtk,
        dir,
        origin: (x, y),
        layout: &layout,
        geometry: &geometry,
        title: &title,
        title_size: (title_width, geometry.text_height()),
        out: Vec::new(),
        image_sizes: HashMap::new(),
        running: Vec::new(),
        steps: 0,
    };
    let p = gtk.palette;
    let solid = |c: Rgb| (c.0, c.1, c.2, 255);
    // The shadow, then everything else — in the window's outline with rounded corners.
    painter.out.push(DrawOp::Fill { rect: (x + 3.0, y + 4.0, w, h), color: (0, 0, 0, 70) });
    painter.out.push(DrawOp::Clip { rect, radii: geometry.rounded });
    // Frame background: the style's color (`background`), otherwise the GTK
    // theme's window background.
    let background = style
        .iter()
        .find_map(|s| s.attr("background"))
        .and_then(|c| parse_color(c, gtk, &theme.constants))
        .unwrap_or(solid(p.window));
    painter.fill((0, 0, width, height), (0.0, 0.0, w, h), background);
    let (top, bottom) = (layout.top_height, geometry.bottom_height.min(height / 3));
    let before = painter.out.len();
    let (lte, rte) = (geometry.left_titlebar_edge.min(width / 2), geometry.right_titlebar_edge.min(width / 2));
    let (tte, bte) = (geometry.top_titlebar_edge.min(top / 2), geometry.bottom_titlebar_edge.min(top / 2));
    let side_h = height - top - bottom;
    // Frame parts in drawing order: each next one goes on top.
    let pieces: [(&str, Area); 12] = [
        ("entire_background", (0, 0, width, height)),
        ("titlebar", (0, 0, width, top)),
        ("titlebar_middle", (lte, tte, width - lte - rte, top - tte - bte)),
        ("left_titlebar_edge", (0, tte, lte, top - tte - bte)),
        ("right_titlebar_edge", (width - rte, tte, rte, top - tte - bte)),
        ("top_titlebar_edge", (0, 0, width, tte)),
        ("bottom_titlebar_edge", (0, top - bte, width, bte)),
        ("title", layout.title),
        ("left_edge", (0, top, geometry.left_width, side_h)),
        ("right_edge", (width - geometry.right_width, top, geometry.right_width, side_h)),
        ("bottom_edge", (0, height - bottom, width, bottom)),
        ("overlay", (0, 0, width, height)),
    ];
    let mut drawn = 0usize;
    for (position, area) in pieces {
        if let Some(list) = theme.piece(&style, position) {
            drawn += painter.in_area(list, area) as usize;
        }
    }
    // Buttons: under each — the background of its place in the row, on top — the
    // button itself.
    for (side, functions, places) in
        [("left", LEFT_BUTTONS, &layout.left_buttons), ("right", RIGHT_BUTTONS, &layout.right_buttons)]
    {
        for (i, (function, area)) in functions.iter().zip(places.iter()).enumerate() {
            let slot = match (places.len(), i) {
                (1, _) => "single",
                (_, 0) => "left",
                (n, i) if i + 1 == n => "right",
                _ => "middle",
            };
            let backing = theme
                .button(&style, &format!("{side}_{slot}_background"))
                .or_else(|| (slot == "single").then(|| theme.button(&style, &format!("{side}_{side}_background"))).flatten());
            if let Some(list) = backing {
                drawn += painter.in_area(list, *area) as usize;
            }
            if let Some(list) = theme.button(&style, function) {
                drawn += painter.in_area(list, *area) as usize;
            }
        }
    }
    if drawn == 0 || painter.out.len() == before {
        return None;
    }
    // The window contents — on top of the frame, as with a real window: the
    // background and lines of text in the GTK theme's colors.
    let client = (geometry.left_width, top, width - geometry.left_width - geometry.right_width, height - top - bottom);
    painter.fill(client, (0.0, 0.0, client.2 as f64, client.3 as f64), solid(p.window));
    let ink = super::mix(p.window, p.text, 0.55);
    for (line_y, line_w) in [(18.0, 120.0), (34.0, 86.0), (50.0, 104.0)] {
        if line_y + 6.0 < client.3 as f64 {
            painter.fill(client, (14.0, line_y, line_w, 6.0), (ink.0, ink.1, ink.2, 200));
        }
    }
    painter.out.push(DrawOp::Unclip);
    Some(painter.out)
}

/// Place of the window on the tile picture — the same as for the XFCE frame sample.
const WINDOW_RECT: Rect = (30.0, 16.0, 240.0, 138.0);

/// A MATE frame theme: a window in its frame — a title bar with the theme name
/// and buttons, the sides and the bottom, as the theme's commands draw them for
/// an active window.
pub(super) fn marco_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    let theme_dir = Path::new(input.install_path);
    let dir = theme_dir.join("metacity-1");
    let (root, _) = load_tree(&dir)?;
    let gtk = GtkColors::for_frames(theme_dir);
    let name = theme_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    frame_ops(&root, &gtk, &dir, &name, WINDOW_RECT)
}

/// Why a MATE frame theme has no sample — for the test stand. An empty string —
/// the sample came out.
pub fn marco_failure(install_path: &str) -> &'static str {
    let theme_dir = Path::new(install_path);
    let dir = theme_dir.join("metacity-1");
    if !THEME_FILES.iter().any(|(name, _)| dir.join(name).is_file()) {
        return "нет файла metacity-theme-{1,2,3}.xml";
    }
    let Some((root, _)) = load_tree(&dir) else { return "файл темы не разобран" };
    let theme = Theme::new(&root);
    if theme.focused_style().is_none() {
        return "нет стиля обычного окна в фокусе";
    }
    let name = theme_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match frame_ops(&root, &GtkColors::for_frames(theme_dir), &dir, &name, WINDOW_RECT) {
        Some(_) => "",
        None => "рамка не вышла: заголовок не помещается либо команды ничего не нарисовали",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_preview::{card_picture, PreviewSource};
    use std::path::PathBuf;

    /// Our own check directory.
    fn tmp(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("rexon-metacity-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn tree(text: &str) -> Node {
        match parse_xml(text, true) {
            Parsed::Tree(node) => node,
            other => panic!("файл не разобран: {other:?}"),
        }
    }

    /// A small theme for checks: geometry with an ancestor, constants as a number
    /// and as a color, named command lists, a style with an ancestor.
    const SAMPLE: &str = r##"<?xml version="1.0"?>
<metacity_theme>
<info><name>Sample</name><author>Rexon</author></info>
<constant name="Pad" value="2"/>
<constant name="C_edge" value="#102030"/>
<frame_geometry name="base" rounded_top_left="4" rounded_top_right="true">
  <distance name="left_width" value="3"/>
  <distance name="right_width" value="3"/>
  <distance name="bottom_height" value="4"/>
  <distance name="left_titlebar_edge" value="3"/>
  <distance name="right_titlebar_edge" value="3"/>
  <distance name="button_width" value="16"/>
  <distance name="button_height" value="16"/>
  <distance name="title_vertical_pad" value="Pad"/>
  <border name="title_border" left="4" right="4" top="1" bottom="1"/>
  <border name="button_border" left="1" right="1" top="2" bottom="2"/>
</frame_geometry>
<frame_geometry name="normal" parent="base">
  <distance name="bottom_height" value="5"/>
</frame_geometry>
<draw_ops name="bar">
  <gradient type="vertical" x="0" y="0" width="width" height="height">
    <color value="gtk:bg[SELECTED]"/>
    <color value="blend/gtk:bg[SELECTED]/black/0.5"/>
  </gradient>
</draw_ops>
<draw_ops name="cross">
  <line color="white" x1="Pad" y1="Pad" x2="width - Pad - 1" y2="height - Pad - 1"/>
  <wobble color="red"/>
  <line color="white" x1="Pad" y1="height / 2" x2="width - Pad - 1" y2="height / 2"/>
</draw_ops>
<frame_style name="plain" geometry="normal">
  <piece position="entire_background">
    <draw_ops><rectangle color="C_edge" x="0" y="0" width="width - 1" height="height - 1"/></draw_ops>
  </piece>
  <button function="close" state="normal" draw_ops="cross"/>
  <button function="close" state="pressed"><draw_ops><tint color="red" alpha="0.5"/></draw_ops></button>
</frame_style>
<frame_style name="focused" parent="plain">
  <piece position="titlebar"><draw_ops><include name="bar"/></draw_ops></piece>
  <piece position="title">
    <draw_ops>
      <title color="gtk:fg[SELECTED]" x="(width - title_width) / 2" y="(height - title_height) / 2"/>
    </draw_ops>
  </piece>
</frame_style>
<frame_style_set name="set">
  <frame focus="no" state="normal" resize="both" style="plain"/>
  <frame focus="yes" state="normal" resize="both" style="focused"/>
</frame_style_set>
<window type="normal" style_set="set"/>
</metacity_theme>
"##;

    #[test]
    fn expressions_follow_the_format_rules() {
        let constants: HashMap<&str, &str> = [("Pad", "2"), ("Half", "0.5"), ("Tint", "#ff0000")].into_iter().collect();
        let vars = [("width", 100.0), ("height", 21.0), ("title_width", 40.0)];
        let scope = Scope { vars: &vars, constants: &constants };
        let value = |e: &str| eval(e, &scope);
        assert_eq!(value("1 + 2 * 3"), Some(7.0));
        assert_eq!(value("(1 + 2) * 3"), Some(9.0));
        assert_eq!(value("(width - title_width) / 2"), Some(30.0));
        // Integer division truncates; with a fractional number it is fractional, and
        // the result drops the fraction.
        assert_eq!(value("height / 2"), Some(10.0));
        assert_eq!(value("height / 2 * 2"), Some(20.0));
        assert_eq!(value("height / 2.0 * 2"), Some(21.0));
        assert_eq!(value("width * Half"), Some(50.0));
        assert_eq!(value("height % 4"), Some(1.0));
        assert_eq!(value("height `mod` 4 + height `div` 4"), Some(6.0));
        // `max` and `min` bind more loosely than addition.
        assert_eq!(value("2 `max` 5"), Some(5.0));
        assert_eq!(value("10 `min` 3 + 1"), Some(4.0));
        assert_eq!(value("0 `max` width - 200"), Some(0.0));
        assert_eq!(value("-Pad + 5"), Some(3.0));
        assert_eq!(value("width-Pad-1"), Some(97.0));
        // What cannot be parsed has no value, without a panic.
        for bad in ["", "width +", "nope + 1", "1 / 0", "5 % 0", "((1)", "1)", "2 `pow` 3", "Tint + 1", "1 $ 2", "`max"] {
            assert_eq!(value(bad), None, "{bad:?}");
        }
        assert_eq!(value(&"(".repeat(200)), None);
        assert_eq!(value(&format!("{}1{}", "(".repeat(40), ")".repeat(40))), None, "скобки глубже предела");
        assert_eq!(value(&"1+".repeat(400)), None, "длиннее предела");
    }

    #[test]
    fn colors_are_read_in_every_notation() {
        let mut gtk = GtkColors::fallback();
        gtk.named.insert("wm_title".into(), "#010203".into());
        gtk.named.insert("wm_link".into(), "@wm_title".into());
        let constants: HashMap<&str, &str> =
            [("C_edge", "#102030"), ("C_soft", "blend/C_edge/white/0.5"), ("C_loop", "C_loop"), ("Pad", "2")].into_iter().collect();
        let color = |text: &str| parse_color(text, &gtk, &constants);
        assert_eq!(color("#102030"), Some((16, 32, 48, 255)));
        assert_eq!(color("#fff"), Some((255, 255, 255, 255)));
        assert_eq!(color("White"), Some((255, 255, 255, 255)));
        assert_eq!(color("dark gray"), Some((169, 169, 169, 255)));
        assert_eq!(color("grey50"), Some((127, 127, 127, 255)));
        assert_eq!(color("rgba(10, 20, 30, 0.5)"), Some((10, 20, 30, 128)));
        // GTK theme colors by part and state.
        assert_eq!(color("gtk:bg[NORMAL]"), Some((237, 236, 235, 255)));
        assert_eq!(color("gtk:bg[SELECTED]"), Some((74, 144, 217, 255)));
        assert_eq!(color("gtk:fg[SELECTED]"), Some((255, 255, 255, 255)));
        assert_eq!(color("gtk:base[normal]"), Some((255, 255, 255, 255)));
        assert_eq!(color("gtk:text[NORMAL]"), Some((46, 52, 54, 255)));
        assert!(color("gtk:light[NORMAL]").unwrap().0 > 237 && color("gtk:dark[NORMAL]").unwrap().0 < 200);
        // Blend: the fraction of the second color; shade: the lightness multiplier.
        assert_eq!(color("blend/#000000/#ffffff/0.5"), Some((128, 128, 128, 255)));
        assert_eq!(color("blend/gtk:fg[NORMAL]/gtk:bg[NORMAL]/0.0"), Some((46, 52, 54, 255)));
        assert_eq!(color("shade/#808080/0.5"), Some((64, 64, 64, 255)));
        assert_eq!(color("shade/#808080/3"), Some((255, 255, 255, 255)));
        assert_eq!(color("shade/gtk:bg[SELECTED]/1.0"), Some((74, 144, 217, 255)));
        assert_eq!(color("blend/shade/#808080/0.5/blend/#000/#fff/1.0/0.5"), Some((160, 160, 160, 255)));
        // A named GTK theme color: its own, by reference, and if it is missing — the
        // fallback.
        assert_eq!(color("gtk:custom(wm_title,gtk:fg[NORMAL])"), Some((1, 2, 3, 255)));
        assert_eq!(color("gtk:custom(wm_link, #ffffff)"), Some((1, 2, 3, 255)));
        assert_eq!(color("gtk:custom(wm_none,gtk:fg[NORMAL])"), Some((46, 52, 54, 255)));
        assert_eq!(color("gtk:custom(wm_none,blend/#000000/#ffffff/0.5)"), Some((128, 128, 128, 255)));
        assert_eq!(color("shade/gtk:custom(wm_none,#808080)/0.5"), Some((64, 64, 64, 255)));
        // Theme color constants, including inside a blend.
        assert_eq!(color("C_edge"), Some((16, 32, 48, 255)));
        assert_eq!(color("C_soft"), Some((136, 144, 152, 255)));
        assert_eq!(color("blend/C_edge/C_edge/0.3"), Some((16, 32, 48, 255)));
        // Garbage, a cyclic constant, a number instead of a color — no color.
        for bad in ["", "nope", "gtk:bg[WEIRD]", "gtk:nope[NORMAL]", "gtk:bg[NORMAL", "blend/#000/#fff", "shade/#000", "#12", "C_loop", "Pad", "gtk:custom(a"] {
            assert_eq!(color(bad), None, "{bad:?}");
        }
        assert_eq!(color(&"shade/".repeat(60)), None);
    }

    #[test]
    fn geometry_inherits_and_lays_out_the_titlebar() {
        let root = tree(SAMPLE);
        let theme = Theme::new(&root);
        let geometry = Geometry::from_chain(&theme.chain(&theme.geometries, "normal"), &theme.constants);
        assert_eq!((geometry.left_width, geometry.right_width), (3, 3));
        assert_eq!(geometry.bottom_height, 5, "потомок перекрыл значение предка");
        assert_eq!(geometry.title_vertical_pad, 2, "расстояние названо константой");
        assert_eq!(geometry.title_border, [4, 4, 1, 1]);
        assert_eq!(geometry.button_border, [1, 1, 2, 2]);
        assert_eq!(geometry.button, ButtonSize::Fixed(16, 16));
        assert_eq!(geometry.rounded, [4.0, 5.0, 0.0, 0.0], "радиус числом и словом true");
        // Title bar: the larger of "the title with padding and the addition"
        // (17 + 2 + 2) and "the button with padding" (16 + 4).
        let layout = geometry.layout(240, 138, 1, 3);
        assert_eq!(layout.top_height, 21);
        assert_eq!(layout.left_buttons, vec![(4, 2, 16, 16)]);
        assert_eq!(layout.right_buttons, vec![(184, 2, 16, 16), (202, 2, 16, 16), (220, 2, 16, 16)]);
        assert_eq!(layout.title, (25, 1, 154, 19));

        // A button by aspect ratio, a title bar without text, without buttons.
        let aspect = tree(
            r#"<metacity_theme><frame_geometry name="g" title_scale="small" hide_buttons="true">
               <aspect_ratio name="button" value="0.5"/>
               <border name="title_border" left="0" right="0" top="2" bottom="2"/>
               <distance name="left_width" value="99999"/></frame_geometry></metacity_theme>"#,
        );
        let theme = Theme::new(&aspect);
        let geometry = Geometry::from_chain(&theme.chain(&theme.geometries, "g"), &theme.constants);
        assert_eq!(geometry.button, ButtonSize::Aspect(0.5));
        assert_eq!(geometry.left_width, MAX_DISTANCE, "расстояние приведено к пределу");
        let layout = geometry.layout(200, 100, 1, 3);
        assert_eq!(layout.top_height, 14 + 4, "строка кегля small — 14 точек");
        assert!(layout.left_buttons.is_empty() && layout.right_buttons.is_empty());
        let shown = Geometry { hide_buttons: false, ..geometry };
        assert_eq!(shown.layout(200, 100, 1, 0).left_buttons, vec![(0, 0, 36, 18)]);
    }

    #[test]
    fn newest_readable_file_version_is_taken() {
        let root = tmp("versions");
        let dir = root.join("metacity-1");
        let body = |mark: &str| format!(r#"<metacity_theme><constant name="Mark" value="{mark}"/></metacity_theme>"#);
        put(&dir.join("metacity-theme-1.xml"), &body("1"));
        let taken = |dir: &Path| {
            let (node, name) = load_tree(dir).unwrap();
            (name, Theme::new(&node).constants.get("Mark").map(|v| v.to_string()))
        };
        assert_eq!(taken(&dir), ("metacity-theme-1.xml", Some("1".into())));
        put(&dir.join("metacity-theme-2.xml"), &body("2"));
        assert_eq!(taken(&dir).0, "metacity-theme-2.xml");
        put(&dir.join("metacity-theme-3.xml"), &body("3"));
        assert_eq!(taken(&dir).0, "metacity-theme-3.xml");
        // A version-3 root requires a format newer than understood — the older file
        // is taken.
        put(&dir.join("metacity-theme-3.xml"), r#"<metacity_theme version="&gt; 9.0"><constant name="Mark" value="3"/></metacity_theme>"#);
        assert_eq!(taken(&dir).0, "metacity-theme-2.xml");
        // A broken newest file — also the older one.
        put(&dir.join("metacity-theme-3.xml"), "<metacity_theme><constant");
        assert_eq!(taken(&dir).0, "metacity-theme-2.xml");
        // A version condition on an element: an unsatisfied one removes the element
        // with its children.
        put(
            &dir.join("metacity-theme-3.xml"),
            r#"<metacity_theme version="&gt;= 3.0">
               <constant name="Mark" value="old" version="&lt; 3.1"/>
               <constant name="Mark" value="new" version="&gt;= 3.4"/>
               <frame_style name="s" version="&gt; 3.5"><piece position="title"/></frame_style>
               </metacity_theme>"#,
        );
        assert_eq!(taken(&dir), ("metacity-theme-3.xml", Some("new".into())));
        let (node, _) = load_tree(&dir).unwrap();
        assert!(Theme::new(&node).styles.is_empty());
        // In a version-1 file the version attribute is not treated as a condition.
        std::fs::remove_file(dir.join("metacity-theme-3.xml")).unwrap();
        std::fs::remove_file(dir.join("metacity-theme-2.xml")).unwrap();
        put(&dir.join("metacity-theme-1.xml"), r#"<metacity_theme><constant name="Mark" value="kept" version="&gt; 9.0"/></metacity_theme>"#);
        assert_eq!(taken(&dir).1, Some("kept".into()));
        for (condition, want) in [("< 3.6", true), ("<= 3.5", true), ("> 3.5", false), (">= 3.5", true), ("=> 3.2", true), ("< 3.1", false), ("3.5", true), ("мусор", false), ("", false)] {
            assert_eq!(version_satisfied(condition), want, "{condition:?}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    fn sample_ops(xml: &str, tag: &str) -> Option<Vec<DrawOp>> {
        let root = tmp(tag);
        let theme_dir = root.join("Sample");
        put(&theme_dir.join("metacity-1/metacity-theme-1.xml"), xml);
        let path = theme_dir.to_string_lossy().into_owned();
        let got = marco_sample(&SampleInput { kind: "marco", install_path: &path, icon_themes: &[] });
        let _ = std::fs::remove_dir_all(&root);
        got
    }

    /// Reference drawing plan of a small theme: the order of parts, expanded
    /// includes and constants, a skipped unknown command, a button only in the "not
    /// pressed" state.
    #[test]
    fn frame_plan_of_a_small_theme() {
        let got = sample_ops(SAMPLE, "plan").unwrap();
        let fill = |x: f64, y: f64, w: f64, h: f64, color: Rgba| DrawOp::Fill { rect: (x, y, w, h), color };
        let clip = |x: f64, y: f64, w: f64, h: f64| DrawOp::Clip { rect: (x, y, w, h), radii: [0.0; 4] };
        let (edge, white) = ((16, 32, 48, 255), (255, 255, 255, 255));
        let want = vec![
            // The shadow and the window outline with rounded top corners.
            fill(33.0, 20.0, 240.0, 138.0, (0, 0, 0, 70)),
            DrawOp::Clip { rect: (30.0, 16.0, 240.0, 138.0), radii: [4.0, 5.0, 0.0, 0.0] },
            // Frame background.
            fill(30.0, 16.0, 240.0, 138.0, (237, 236, 235, 255)),
            // entire_background — from the style's ancestor: an outline with a color constant.
            clip(30.0, 16.0, 240.0, 138.0),
            fill(30.0, 16.0, 240.0, 1.0, edge),
            fill(30.0, 153.0, 240.0, 1.0, edge),
            fill(30.0, 17.0, 1.0, 136.0, edge),
            fill(269.0, 17.0, 1.0, 136.0, edge),
            DrawOp::Unclip,
            // titlebar — an include of a named list: a gradient in the GTK theme's colors.
            clip(30.0, 16.0, 240.0, 21.0),
            DrawOp::Shade {
                rect: (30.0, 16.0, 240.0, 21.0),
                direction: ShadeDirection::Vertical,
                colors: vec![(74, 144, 217, 255), (37, 72, 109, 255)],
            },
            DrawOp::Unclip,
            // title — the theme name in the center of the place between the buttons.
            clip(55.0, 17.0, 154.0, 19.0),
            DrawOp::Label { rect: (108.0, 18.0, 48.0, 17.0), text: "Sample".into(), pixel_size: 13, bold: true, color: white },
            DrawOp::Unclip,
            // The "close" button: a diagonal line and a line along points; <wobble> is skipped.
            clip(250.0, 18.0, 16.0, 16.0),
            DrawOp::Line { from: (252.0, 20.0), to: (263.0, 31.0), width: 1.0, color: white },
            fill(252.0, 26.0, 12.0, 1.0, white),
            DrawOp::Unclip,
            // The window contents on top of the frame: the background and three lines of text.
            fill(33.0, 37.0, 234.0, 112.0, (237, 236, 235, 255)),
            fill(47.0, 55.0, 120.0, 6.0, (132, 135, 135, 200)),
            fill(47.0, 71.0, 86.0, 6.0, (132, 135, 135, 200)),
            fill(47.0, 87.0, 104.0, 6.0, (132, 135, 135, 200)),
            DrawOp::Unclip,
        ];
        assert_eq!(got.len(), want.len(), "{got:#?}");
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert_eq!(g, w, "операция {i}");
        }
        // Clips are paired: the executor closes exactly as many as it opened.
        let opened = got.iter().filter(|op| matches!(op, DrawOp::Clip { .. })).count();
        let closed = got.iter().filter(|op| matches!(op, DrawOp::Unclip)).count();
        assert_eq!(opened, closed);
    }

    #[test]
    fn hostile_files_leave_the_placeholder_and_never_hang() {
        // Broken XML, a foreign root, an empty file, a theme without a window style.
        for (tag, xml) in [
            ("cut", &SAMPLE[..SAMPLE.len() / 2]),
            ("root", "<theme><window type=\"normal\" style_set=\"set\"/></theme>"),
            ("empty", ""),
            ("binary", "\u{0}\u{1}<<<>>>&&&"),
            ("nostyle", "<metacity_theme><window type=\"normal\" style_set=\"none\"/></metacity_theme>"),
            ("two-roots", "<metacity_theme/><metacity_theme/>"),
        ] {
            assert!(sample_ops(xml, tag).is_none(), "{tag}");
        }
        // The kind's placeholder — through the shared tile picture chain.
        let root = tmp("stub");
        put(&root.join("Bad/metacity-1/metacity-theme-1.xml"), "<metacity_theme>");
        let path = root.join("Bad").to_string_lossy().into_owned();
        let picture = card_picture(&SampleInput { kind: "marco", install_path: &path, icon_themes: &[] }, "🪟");
        assert_eq!(picture.source, PreviewSource::Placeholder);
        assert_eq!(marco_failure(&path), "файл темы не разобран");
        assert_eq!(marco_failure(&root.join("None").to_string_lossy()), "нет файла metacity-theme-{1,2,3}.xml");
        let _ = std::fs::remove_dir_all(&root);

        // Element nesting that is too deep.
        let deep = format!("<metacity_theme>{}{}</metacity_theme>", "<a>".repeat(200), "</a>".repeat(200));
        assert!(matches!(parse_xml(&deep, false), Parsed::Broken));

        // Cyclic includes, parent of a style, geometry and set; a tile of a million
        // repetitions; an image outside the theme directory; division by zero. The
        // frame is drawn anyway — with whatever could be parsed.
        let looped = r##"<metacity_theme>
<frame_geometry name="g" parent="h"><distance name="left_width" value="2"/></frame_geometry>
<frame_geometry name="h" parent="g"><distance name="right_width" value="2"/></frame_geometry>
<draw_ops name="a"><include name="b"/><rectangle color="#ff0000" filled="true" x="0" y="0" width="1" height="1"/></draw_ops>
<draw_ops name="b"><include name="a"/><include name="b"/><include name="missing"/></draw_ops>
<draw_ops name="dot"><rectangle color="#00ff00" filled="true" x="0" y="0" width="1" height="1"/></draw_ops>
<frame_style name="s" parent="t" geometry="g">
  <piece position="entire_background" draw_ops="a"/>
  <piece position="titlebar"><draw_ops>
    <tile name="dot" tile_width="1" tile_height="1"/>
    <tile name="dot" tile_width="0" tile_height="0"/>
    <tile name="dot" x="0" y="0" width="4" height="2" tile_width="2" tile_height="2"/>
    <image filename="../../../../etc/passwd" x="0" y="0" width="10" height="10"/>
    <image filename="/etc/passwd" x="0" y="0" width="10" height="10"/>
    <image filename="missing.png" x="0" y="0" width="object_width" height="object_height"/>
    <rectangle color="#0000ff" filled="true" x="1 / 0" y="0" width="5" height="5"/>
    <clip x="0" y="0" width="5" height="5"/>
    <arc color="#0000ff" from="0" to="90" filled="true" x="0" y="0" width="10" height="10"/>
    <gtk_box state="normal" shadow="out" x="0" y="0" width="8" height="8"/>
    <gtk_arrow state="normal" arrow="up" x="0" y="0" width="8" height="8"/>
    <gtk_vline state="normal" x="2" y1="0" y2="height"/>
    <icon x="0" y="0" width="mini_icon_width" height="mini_icon_height"/>
  </draw_ops></piece>
</frame_style>
<frame_style name="t" parent="s"/>
<frame_style_set name="x" parent="y"><frame focus="yes" state="normal" resize="both" style="s"/></frame_style_set>
<frame_style_set name="y" parent="x"/>
<window type="normal" style_set="x"/>
</metacity_theme>"##;
        let ops = sample_ops(looped, "loops").expect("рамка рисуется тем, что разобрано");
        assert!(ops.len() < MAX_OPS);
        let reds = ops.iter().filter(|op| matches!(op, DrawOp::Fill { color: (255, 0, 0, 255), .. })).count();
        assert_eq!(reds, 1, "список, включающий сам себя, второй раз не начинается");
        let greens = ops.iter().filter(|op| matches!(op, DrawOp::Fill { color: (0, 255, 0, 255), .. })).count();
        assert_eq!(greens, 2, "плитка 4×2 из клеток 2×2; без счёта и из миллиона клеток — пропущены");
        assert!(!ops.iter().any(|op| matches!(op, DrawOp::Picture { .. })), "чужие и отсутствующие рисунки не берутся");
        assert!(ops.iter().any(|op| matches!(op, DrawOp::Arc { start, extent, .. } if *start == 0.0 && *extent == 90.0)));
        let opened = ops.iter().filter(|op| matches!(op, DrawOp::Clip { .. })).count();
        let closed = ops.iter().filter(|op| matches!(op, DrawOp::Unclip)).count();
        assert_eq!(opened, closed, "отсечение команды <clip> закрыто в конце списка");

        // Lists including the next one three times each: without counting commands that
        // is 3^11 executions.
        let lists: String = (0..11).map(|i| format!(r#"<draw_ops name="l{i}"><include name="l{n}"/><include name="l{n}"/><include name="l{n}"/></draw_ops>"#, n = i + 1)).collect();
        let fan = format!(
            r##"<metacity_theme><frame_geometry name="g"/>{lists}<draw_ops name="l11"><clip x="0" y="0" width="1" height="1"/></draw_ops>
            <frame_style name="s" geometry="g"><piece position="entire_background" draw_ops="l0"/><piece position="titlebar" draw_ops="l0"/></frame_style>
            <frame_style_set name="x"><frame focus="yes" state="normal" resize="both" style="s"/></frame_style_set>
            <window type="normal" style_set="x"/></metacity_theme>"##
        );
        let started = std::time::Instant::now();
        let _ = sample_ops(&fan, "fan");
        assert!(started.elapsed().as_secs() < 5, "исполнение ограничено счётом команд");

        // The operation count limit: thousands of commands in one list.
        let many = format!(
            r##"<metacity_theme><frame_geometry name="g"/><frame_style name="s" geometry="g">
            <piece position="entire_background"><draw_ops>{}</draw_ops></piece></frame_style>
            <frame_style_set name="x"><frame focus="yes" state="normal" resize="both" style="s"/></frame_style_set>
            <window type="normal" style_set="x"/></metacity_theme>"##,
            r##"<rectangle color="#fff" filled="true" x="0" y="0" width="1" height="1"/>"##.repeat(MAX_OPS * 2)
        );
        assert!(sample_ops(&many, "many").unwrap().len() <= MAX_OPS + 4);
    }

    #[test]
    fn images_and_gtk_colors_come_from_the_theme_directory() {
        let root = tmp("images");
        let theme_dir = root.join("Pictured");
        // PNG: the signature and a 20×10 IHDR block — enough to read the size.
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&[0, 0, 0, 20, 0, 0, 0, 10, 8, 6, 0, 0, 0]);
        std::fs::create_dir_all(theme_dir.join("metacity-1")).unwrap();
        std::fs::write(theme_dir.join("metacity-1/close.png"), &png).unwrap();
        put(
            &theme_dir.join("gtk-3.0/gtk.css"),
            "@define-color theme_bg_color #202020;\n@define-color theme_fg_color #f0f0f0;\n@define-color theme_selected_bg_color #aa0000;\n@define-color wm_title #00ff00;\n",
        );
        put(
            &theme_dir.join("metacity-1/metacity-theme-2.xml"),
            r##"<metacity_theme><frame_geometry name="g"><distance name="button_width" value="20"/><distance name="button_height" value="20"/></frame_geometry>
<frame_style name="s" geometry="g" background="gtk:bg[SELECTED]">
  <piece position="titlebar"><draw_ops>
    <tint color="gtk:custom(wm_title,#000000)" alpha="0.5" x="0" y="0" width="width" height="height"/>
    <image filename="close.png" colorize="gtk:fg[NORMAL]" alpha="0.7" x="width - object_width" y="0" width="object_width" height="object_height"/>
    <image filename="close.png" fill_type="tile" x="0" y="0" width="width" height="height"/>
  </draw_ops></piece>
</frame_style>
<frame_style_set name="x"><frame focus="yes" state="normal" style="s"/></frame_style_set>
<window type="normal" style_set="x"/></metacity_theme>"##,
        );
        let path = theme_dir.to_string_lossy().into_owned();
        let ops = marco_sample(&SampleInput { kind: "marco", install_path: &path, icon_themes: &[] }).unwrap();
        let file = theme_dir.join("metacity-1/close.png").to_string_lossy().into_owned();
        // Frame background — the style's color, which is also the selection color of the
        // GTK theme from the neighboring directory.
        assert_eq!(ops[2], DrawOp::Fill { rect: (30.0, 16.0, 240.0, 138.0), color: (170, 0, 0, 255) });
        assert!(ops.contains(&DrawOp::Fill { rect: (30.0, 16.0, 240.0, 20.0), color: (0, 255, 0, 128) }), "{ops:#?}");
        // The window contents — in the colors of the same GTK theme.
        assert!(ops.contains(&DrawOp::Fill { rect: (30.0, 36.0, 240.0, 118.0), color: (32, 32, 32, 255) }));

        // A "borders only" set takes the colors of the GTK theme of the neighboring
        // directory.
        std::fs::create_dir_all(root.join("Pictured-border/metacity-1")).unwrap();
        assert_eq!(GtkColors::for_frames(&root.join("Pictured-border")).palette.window, (32, 32, 32));
        assert_eq!(GtkColors::for_frames(&root.join("Other-border")).palette.window, GtkColors::fallback().palette.window);
        assert!(ops.contains(&DrawOp::Picture {
            rect: (250.0, 16.0, 20.0, 10.0),
            path: file.clone(),
            tile: false,
            alpha: 0.7,
            colorize: Some((240, 240, 240, 255)),
        }));
        assert!(ops.contains(&DrawOp::Picture { rect: (30.0, 16.0, 240.0, 20.0), path: file, tile: true, alpha: 1.0, colorize: None }));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A theme file that Marco accepts: all `<info>` fields, all window types and,
    /// in the frame style, all mandatory buttons of version `version` except
    /// `skip`; `extra` — what else to put into the style.
    fn marco_theme(version: u32, skip: &[&str], extra: &str) -> String {
        let buttons: String = MARCO_REQUIRED_BUTTONS
            .iter()
            .filter(|f| !skip.contains(f))
            .filter(|f| MARCO_BUTTON_FUNCTIONS.iter().any(|(g, since)| g == *f && *since <= version))
            .flat_map(|f| {
                ["normal", "pressed"].map(|st| format!(r#"<button function="{f}" state="{st}" draw_ops="x"/>"#))
            })
            .collect();
        let windows: String = MARCO_REQUIRED_WINDOWS
            .iter()
            .map(|t| format!(r#"<window type="{t}" style_set="set"/>"#))
            .collect();
        format!(
            r#"<?xml version="1.0"?>
<metacity_theme>
<info><name>W</name><author>a</author><copyright>c</copyright><date>d</date><description>t</description></info>
<draw_ops name="x"/>
<frame_style name="normal_focused" geometry="g">
  {buttons}
  {extra}
</frame_style>
<menu_icon function="unmaximize" state="normal" draw_ops="restore_button"/>
{windows}
</metacity_theme>"#
        )
    }

    /// A button with a function that does not exist in the format (an example —
    /// WinShine-blue-48: `unmaximize`, allowed only on `<menu_icon>`) — Marco will
    /// not load the theme; the file is taken in its search order.
    #[test]
    fn marco_unknown_button_function_is_named() {
        let files: std::cell::RefCell<HashMap<String, String>> = Default::default();
        let set = |path: &str, text: &str| {
            files.borrow_mut().insert(path.to_string(), text.to_string());
        };
        let check = |dirs: &[&str]| {
            let dirs: Vec<String> = dirs.iter().map(|d| d.to_string()).collect();
            marco_theme_problem(&dirs, &|p: &str| files.borrow().get(p).cloned())
        };
        let unknown = |f: &str| Some(MarcoProblem::UnknownButton(f.to_string()));
        let bad = marco_theme(3000, &[], r#"<button function="unmaximize"  state="normal"   draw_ops="restore_button"/>"#);
        let good = marco_theme(3000, &[], "");

        set("/u/metacity-1/metacity-theme-3.xml", &bad);
        assert_eq!(check(&["/u/metacity-1"]), unknown("unmaximize"));
        // Nothing was read — and there is nothing to say.
        assert_eq!(check(&["/нет/metacity-1"]), None);

        // `unmaximize` on `<menu_icon>` is legitimate and must not get in the way.
        set("/u/metacity-1/metacity-theme-3.xml", &good);
        assert_eq!(check(&["/u/metacity-1"]), None);

        // Marco does not parse a button under an unsatisfied version condition.
        set(
            "/u/metacity-1/metacity-theme-3.xml",
            &marco_theme(3000, &[], r#"<button function="unmaximize" state="normal" draw_ops="x" version="&gt;= 9.0"/>"#),
        );
        assert_eq!(check(&["/u/metacity-1"]), None);

        // Search order: the newest version in ALL directories before an older one in
        // the first; within a version — the first directory.
        set("/u/metacity-1/metacity-theme-3.xml", &good);
        set("/u/metacity-1/metacity-theme-2.xml", &bad);
        assert_eq!(check(&["/u/metacity-1"]), None, "взят файл третьей версии");
        files.borrow_mut().remove("/u/metacity-1/metacity-theme-3.xml");
        set("/s/metacity-1/metacity-theme-3.xml", &good);
        assert_eq!(check(&["/u/metacity-1", "/s/metacity-1"]), None, "третья версия из системного каталога");
        set("/h/metacity-1/metacity-theme-3.xml", &bad);
        assert_eq!(check(&["/h/metacity-1", "/s/metacity-1"]), unknown("unmaximize"));
        // The root requires a format newer than understood — the older file is taken.
        set("/h/metacity-1/metacity-theme-3.xml", r#"<metacity_theme version="&gt; 9.0"><info/></metacity_theme>"#);
        set("/h/metacity-1/metacity-theme-2.xml", &bad);
        assert_eq!(check(&["/h/metacity-1"]), unknown("unmaximize"));
        // A broken file is not this incompatibility.
        set("/b/metacity-1/metacity-theme-3.xml", "<metacity_theme><button");
        assert_eq!(check(&["/b/metacity-1"]), None);

        // Marco does not know version-2 buttons at all in a version-1 file.
        set("/one/metacity-1/metacity-theme-1.xml", &marco_theme(2000, &[], ""));
        assert_eq!(check(&["/one/metacity-1"]), unknown("shade"));
    }

    /// A frame style without a mandatory button (an example — Win8 themes from the
    /// store without `shade`): Marco rejects the theme entirely. What is mandatory
    /// depends on the style's format version.
    #[test]
    fn marco_required_button_missing_is_named() {
        let check = |file: &str, text: String| {
            let dir = "/t/metacity-1".to_string();
            let path = format!("{dir}/{file}");
            marco_theme_problem(&[dir], &move |p: &str| (p == path).then(|| text.clone()))
        };
        let missing = |f: &str| Some(MarcoProblem::MissingButton(f.to_string()));

        // Versions 2 and 3: shade and others are mandatory.
        assert_eq!(check("metacity-theme-2.xml", marco_theme(2000, &["shade"], "")), missing("shade"));
        assert_eq!(check("metacity-theme-3.xml", marco_theme(3000, &["shade"], "")), missing("shade"));
        assert_eq!(check("metacity-theme-3.xml", marco_theme(3000, &["close"], "")), missing("close"));
        // Version 1: shade is not needed.
        assert_eq!(check("metacity-theme-1.xml", marco_theme(1000, &[], "")), None);
        // The pressed state is also mandatory; prelight is not (it falls back to normal).
        let no_pressed = marco_theme(3000, &[], "").replace(
            r#"<button function="stick" state="pressed" draw_ops="x"/>"#,
            "",
        );
        assert_eq!(check("metacity-theme-3.xml", no_pressed), missing("stick"));

        // appmenu is mandatory only for a style of version 3.5 and newer.
        assert_eq!(check("metacity-theme-3.xml", marco_theme(3000, &["appmenu"], "")), None);
        let v35 = marco_theme(3005, &["appmenu"], "").replace("<metacity_theme>", r#"<metacity_theme version="&gt;= 3.5">"#);
        assert_eq!(check("metacity-theme-3.xml", v35), missing("appmenu"));
        let style35 = marco_theme(3005, &["appmenu"], "")
            .replace(r#"<frame_style name="normal_focused""#, r#"<frame_style version="&gt; 3.4" name="normal_focused""#);
        assert_eq!(check("metacity-theme-3.xml", style35), missing("appmenu"));

        // Buttons are inherited from `parent`: a child style without its own buttons is
        // legitimate.
        let child = marco_theme(3000, &[], "").replace(
            "<menu_icon",
            r#"<frame_style name="child" parent="normal_focused" geometry="g"><piece position="entire_background" draw_ops="x"/></frame_style><menu_icon"#,
        );
        assert_eq!(check("metacity-theme-3.xml", child), None);
        // …but without a parent it is not.
        let orphan = marco_theme(3000, &[], "").replace(
            "<menu_icon",
            r#"<frame_style name="orphan" geometry="g"><piece position="entire_background" draw_ops="x"/></frame_style><menu_icon"#,
        );
        assert_eq!(check("metacity-theme-3.xml", orphan), missing("close"));
    }

    /// Without a mandatory `<info>` field or window type Marco does not take the theme.
    #[test]
    fn marco_required_parts_missing_are_named() {
        let check = |text: String| {
            let dir = "/t/metacity-1".to_string();
            let path = format!("{dir}/metacity-theme-3.xml");
            marco_theme_problem(&[dir], &move |p: &str| (p == path).then(|| text.clone()))
        };
        let part = |p: &str| Some(MarcoProblem::MissingPart(p.to_string()));
        assert_eq!(check(marco_theme(3000, &[], "").replace("<author>a</author>", "")), part("<author>"));
        assert_eq!(check(marco_theme(3000, &[], "").replace(r#"<window type="border" style_set="set"/>"#, "")), part(r#"<window type="border">"#));
        // attached is optional — and the sample does not have it.
        assert_eq!(check(marco_theme(3000, &[], "")), None);
    }
}
