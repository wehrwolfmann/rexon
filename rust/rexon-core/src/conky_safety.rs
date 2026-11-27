//! Dangerous directives and the health of a Conky widget (the Conky
//! Manager parity plan, items 42 and 40).
//!
//! A Conky widget is not a picture: a settings file can run commands
//! (`${exec…}`, `${execi…}`, `${texeci…}`), load Lua scripts
//! (`lua_load`, and in them `os.execute`, `io.popen`) and go online
//! (`${curl}`, `${rss}`, `${weather}`). A theme downloaded from the store does this
//! with the person's rights. So before the FIRST launch of such a widget Rexon
//! shows what exactly it runs, and starts it only with permission.
//!
//! The permission is remembered by the fingerprint (SHA-256) of the widget's code files: the settings
//! files, Lua and shell scripts, files its commands
//! reference. Editing any of them means the question again, with the explanation "the files
//! have changed". Data the widget writes into its own directory (a weather cache,
//! pictures) does not touch the fingerprint — otherwise the question would come at every launch.
//!
//! Permissions live in `~/.config/rexon/conky-trust.json`.
//!
//! The same parsing also answers "what the widget is missing" ([`health`]):
//! scripts it references and programs from its commands — with a package
//! for the distribution family, to install it before the launch (without a question:
//! the protection is the polkit password window; if it did not install, the widget does not start, see
//! [`start_plan`]).
//!
//! Parsing — without running Lua: comments are cut out, strings stay as
//! they are, directives are searched for in the remaining text. The goal is to honestly show
//! the person the commands, not to prove safety: a command assembled by
//! a script from strings (`local cmd = 'cava -p ' .. CONF` → `io.popen(cmd)`)
//! is shown as text, where the parts unknown at parse time are "…"; with no strings at all
//! — as a call `os.execute(…)` with an expression.

use sha2::{Digest, Sha256};

use crate::animations::LookIo;
use crate::de_apply::Ctx;
use crate::i18n::{ntr, tr, trf};

/// What the widget does besides drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskKind {
    /// Runs a shell command.
    Command,
    /// Loads a Lua script.
    LuaScript,
    /// Downloads from the internet.
    Network,
}

/// One dangerous directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Risk {
    pub kind: RiskKind,
    /// A command, script or address — as written in the file.
    pub what: String,
    /// The file where the directive is: relative to the widget directory (outside it —
    /// the full path).
    pub file: String,
}

/// Directives with a command: `(name, the first argument is an interval)`.
const COMMAND_DIRECTIVES: &[(&str, bool)] = &[
    ("exec", false),
    ("execp", false),
    ("execbar", false),
    ("execgauge", false),
    ("execgraph", false),
    ("execi", true),
    ("execpi", true),
    ("execibar", true),
    ("execigauge", true),
    ("execigraph", true),
    ("texeci", true),
    ("texecpi", true),
];

/// Directives that download: the first argument is an address.
const NETWORK_DIRECTIVES: &[&str] = &["curl", "rss", "weather", "weather_forecast"];

/// Code file extensions: editing them changes the widget's behavior.
const CODE_EXTS: &[&str] = &["lua", "sh", "bash", "zsh", "py", "pl", "rb", "js", "php", "tcl", "awk", "sed"];

/// The traversal depth of the widget directory.
const MAX_DEPTH: usize = 6;

/// We do not look at more code files than this (a widget archive with thousands of files is no longer
/// a widget; the fingerprint changes anyway when the first ones are edited).
const MAX_FILES: usize = 400;

// ============================================================================
// Widget files
// ============================================================================

/// The widget directory and its settings files.
struct Widget {
    /// The full path of the widget (a directory or a single file) — the permission key.
    path: String,
    /// The directory paths in messages are written relative to.
    base: String,
    confs: Vec<String>,
}

fn widget(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Widget {
    let path = crate::conky_scan::widget_path(ctx, id);
    let is_dir = !io.list_dir(&path).is_empty();
    let base = if is_dir { path.clone() } else { crate::de_apply::conky_work_dir(&path).to_string() };
    let primary = if crate::conky_scan::is_found_name(id) { None } else { crate::de_apply::conky_conf(io, ctx, id) };
    let confs = crate::conky_scan::widget_confs(io, ctx, id, primary);
    Widget { path, base, confs }
}

fn rel(base: &str, path: &str) -> String {
    path.strip_prefix(&format!("{}/", base.trim_end_matches('/')))
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string())
}

fn ext_of(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// All text files of the widget directory (without `.git`).
fn walk(io: &dyn LookIo, dir: &str, depth: usize, out: &mut Vec<String>) {
    for name in io.list_dir(dir) {
        if out.len() >= MAX_FILES {
            return;
        }
        let path = format!("{dir}/{name}");
        if !io.list_dir(&path).is_empty() {
            if depth < MAX_DEPTH && name != ".git" {
                walk(io, &path, depth + 1, out);
            }
        } else {
            out.push(path);
        }
    }
}

/// A path from a directive — to a file on disk. conky and its commands work from
/// the settings file's folder (`de_apply::conky_work_dir`), `~` is the home
/// directory. A bare name without a slash is a program from PATH, not a file.
fn resolve(ctx: &Ctx, work_dir: &str, token: &str) -> Option<String> {
    let t = token.trim_matches(|c| c == '"' || c == '\'');
    if let Some(rest) = t.strip_prefix("~/") {
        return Some(format!("{}/{rest}", ctx.home.trim_end_matches('/')));
    }
    if let Some(rest) = t.strip_prefix("$HOME/").or_else(|| t.strip_prefix("${HOME}/")) {
        return Some(format!("{}/{rest}", ctx.home.trim_end_matches('/')));
    }
    if t.starts_with('/') {
        return Some(t.to_string());
    }
    if t.contains('/') && !t.contains("://") {
        let t = t.strip_prefix("./").unwrap_or(t);
        return Some(format!("{}/{t}", work_dir.trim_end_matches('/')));
    }
    None
}

// ============================================================================
// Parsing
// ============================================================================

/// The `=` level of a Lua long bracket at position `i` (`[[` — 0, `[==[` — 2).
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

/// The end of a long bracket of level `level` started at `i`: the position after `]=*]`.
fn long_end(b: &[u8], i: usize, level: usize) -> usize {
    let close: Vec<u8> = std::iter::once(b']').chain(std::iter::repeat_n(b'=', level)).chain(std::iter::once(b']')).collect();
    let start = i + level + 2;
    b[start.min(b.len())..]
        .windows(close.len())
        .position(|w| w == close.as_slice())
        .map(|p| start + p + close.len())
        .unwrap_or(b.len())
}

/// Lua text without comments: comments are replaced with spaces (newlines
/// stay), strings — as they were.
fn strip_lua_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    let blank = |out: &mut Vec<u8>, part: &[u8]| {
        out.extend(part.iter().map(|&c| if c == b'\n' { b'\n' } else { b' ' }));
    };
    while i < b.len() {
        let c = b[i];
        if c == b'-' && b.get(i + 1) == Some(&b'-') {
            let end = match long_bracket(b, i + 2) {
                Some(level) => long_end(b, i + 2, level),
                None => b[i..].iter().position(|&c| c == b'\n').map(|p| i + p).unwrap_or(b.len()),
            };
            blank(&mut out, &b[i..end]);
            i = end;
        } else if let Some(level) = long_bracket(b, i) {
            let end = long_end(b, i, level);
            out.extend_from_slice(&b[i..end]);
            i = end;
        } else if c == b'"' || c == b'\'' {
            let mut j = i + 1;
            while j < b.len() && b[j] != c && b[j] != b'\n' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(b.len());
            out.extend_from_slice(&b[i..end]);
            i = end;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// A Lua string literal from position `i` (spaces before it are skipped):
/// `(contents, position after)`.
fn lua_string_at(s: &str, i: usize) -> Option<(String, usize)> {
    let b = s.as_bytes();
    let mut i = i;
    while b.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
        i += 1;
    }
    let q = *b.get(i)?;
    if q == b'"' || q == b'\'' {
        let mut j = i + 1;
        let mut val: Vec<u8> = Vec::new();
        while j < b.len() && b[j] != q && b[j] != b'\n' {
            if b[j] == b'\\' && j + 1 < b.len() {
                val.push(b[j + 1]);
                j += 2;
            } else {
                val.push(b[j]);
                j += 1;
            }
        }
        return Some((String::from_utf8_lossy(&val).into_owned(), (j + 1).min(b.len())));
    }
    let level = long_bracket(b, i)?;
    let end = long_end(b, i, level);
    let inner_start = (i + level + 2).min(end);
    let inner_end = end.saturating_sub(level + 2).max(inner_start);
    Some((s[inner_start..inner_end].to_string(), end))
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Positions of the whole word `word` (not part of another name).
fn word_positions<'a>(s: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    let b = s.as_bytes();
    s.match_indices(word).map(|(p, _)| p).filter(move |&p| {
        (p == 0 || !is_ident(b[p - 1]) && b[p - 1] != b'.') && b.get(p + word.len()).is_none_or(|&c| !is_ident(c))
    })
}

/// `${…}` directives in the text: `(kind, what)`.
fn directives(text: &str) -> Vec<(RiskKind, String)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = text[i..].find("${") {
        let start = i + p + 2;
        let mut j = start;
        while j < b.len() && is_ident(b[j]) {
            j += 1;
        }
        let name = &text[start..j];
        // The end of the directive is the matching `}` (there may be nested `${…}` inside).
        let mut depth = 1;
        let mut k = j;
        while k < b.len() {
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
        let args = text[j..k.min(b.len())].trim();
        if let Some((_, interval)) = COMMAND_DIRECTIVES.iter().find(|(n, _)| *n == name) {
            let cmd = if *interval {
                args.split_once(char::is_whitespace).map(|(_, rest)| rest.trim()).unwrap_or("")
            } else {
                args
            };
            if !cmd.is_empty() {
                out.push((RiskKind::Command, cmd.to_string()));
            }
        } else if NETWORK_DIRECTIVES.contains(&name) {
            if let Some(url) = args.split_whitespace().next() {
                out.push((RiskKind::Network, url.to_string()));
            }
        }
        // Nested directives are parsed by the next search step.
        i = j.max(start);
    }
    out
}

/// conky directives in Lua code (a 1.10 settings file, a widget script).
/// A directive broken by string concatenation (`'${execi 2 bash ' .. HOME ..
/// '/s/gpu.sh}'`) is assembled as a Lua expression ([`lua_command_text`]):
/// otherwise the quotes and `..` went into the command, the path lost its start
/// (`/.config/…` instead of `~/.config/…`) and the file check falsely said
/// "no widget files" (a live check of 15.11.2026, the skyrim widget).
/// An unparsed part stays as [`UNKNOWN_PART`] — such paths are not
/// checked: better to keep quiet than to lie.
fn lua_directives(code: &str) -> Vec<(RiskKind, String)> {
    directives(code)
        .into_iter()
        .map(|(kind, what)| {
            if kind != RiskKind::Command {
                return (kind, what);
            }
            let Some(q) = lua_split_quote(&what) else { return (kind, what) };
            let expr = format!("{q}{what}{q}");
            let text = lua_command_text(code, &expr, code.len(), 0).unwrap_or_else(|| UNKNOWN_PART.to_string());
            (kind, text)
        })
        .collect()
}

/// The quote at which the directive's command is broken by Lua string concatenation
/// (`' .. x .. '`); `None` — the whole command is in one string.
fn lua_split_quote(cmd: &str) -> Option<char> {
    ['\'', '"'].into_iter().find(|&q| {
        cmd.match_indices(q).any(|(i, _)| {
            let after = cmd[i + 1..].trim_start();
            let before = cmd[..i].trim_end();
            after.starts_with("..") || before.ends_with("..")
        })
    })
}

/// The old syntax (1.9): settings up to the `TEXT` line, after it — the text.
fn is_old_syntax(text: &str) -> bool {
    !text.contains("conky.config") && !text.contains("conky.text") && text.lines().any(|l| l.trim() == "TEXT")
}

/// A 1.9 line without the `#` comment (`\#` is the character, not a comment).
fn strip_hash_comment(line: &str) -> &str {
    let b = line.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'#' && (i == 0 || b[i - 1] != b'\\') {
            return &line[..i];
        }
    }
    line
}

/// Directives of a settings file: commands, addresses and `lua_load` scripts.
fn scan_conf(text: &str) -> Vec<(RiskKind, String)> {
    let mut out = Vec::new();
    if is_old_syntax(text) {
        let mut in_text = false;
        let mut body = String::new();
        for line in text.lines() {
            if in_text {
                body.push_str(line);
                body.push('\n');
                continue;
            }
            if line.trim() == "TEXT" {
                in_text = true;
                continue;
            }
            let line = strip_hash_comment(line).trim();
            if let Some(rest) = line.strip_prefix("lua_load").filter(|r| r.starts_with(char::is_whitespace)) {
                out.extend(rest.split_whitespace().map(|s| (RiskKind::LuaScript, s.to_string())));
            }
        }
        out.extend(directives(&body));
        return out;
    }
    let code = strip_lua_comments(text);
    for p in word_positions(&code, "lua_load").collect::<Vec<_>>() {
        let after = &code[p + "lua_load".len()..];
        let Some(eq) = after.find(|c: char| !c.is_whitespace()).filter(|&e| after[e..].starts_with('=')) else {
            continue;
        };
        let at = p + "lua_load".len() + eq + 1;
        if let Some((value, _)) = lua_string_at(&code, at) {
            out.extend(value.split_whitespace().map(|s| (RiskKind::LuaScript, s.to_string())));
        }
    }
    out.extend(lua_directives(&code));
    out
}

/// A part of a command assembled by a Lua script that is unknown at parse time.
const UNKNOWN_PART: &str = "…";

/// Pieces of the expression `expr` separated by `sep` outside strings and brackets.
fn top_level_split<'a>(expr: &'a str, sep: &[u8]) -> Vec<&'a str> {
    let b = expr.as_bytes();
    let mut out = Vec::new();
    let (mut depth, mut start, mut i) = (0i32, 0, 0);
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\'' || long_bracket(b, i).is_some() {
            i = lua_string_at(expr, i).map(|(_, end)| end).unwrap_or(b.len()).max(i + 1);
            continue;
        }
        match c {
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => depth -= 1,
            _ => {}
        }
        if depth == 0 && b[i..].starts_with(sep) {
            out.push(&expr[start..i]);
            i += sep.len();
            start = i;
            continue;
        }
        i += 1;
    }
    out.push(&expr[start..]);
    out
}

/// The last assignment `name = …` (and `local name =
/// …`) before the position `before`: `(expression, assignment position)`. The expression goes to the end of the line, and
/// a line broken on `..` continues with the next one.
fn lua_assignment<'a>(code: &'a str, name: &str, before: usize) -> Option<(&'a str, usize)> {
    let p = word_positions(code, name)
        .filter(|&p| p < before)
        .filter(|&p| {
            let after = code[p + name.len()..].trim_start();
            after.starts_with('=') && !after.starts_with("==")
        })
        .last()?;
    let eq = p + name.len() + code[p + name.len()..].find('=')? + 1;
    let mut end = eq;
    loop {
        end += code[end..].find('\n').unwrap_or(code.len() - end);
        if end >= code.len() || !code[eq..end].trim_end().ends_with("..") {
            break;
        }
        end += 1;
    }
    Some((&code[eq..end], p))
}

/// The command text from the Lua expression `expr` (of a call at the position `at`):
/// strings as is, names — by their assignment higher up in the text,
/// `string.format("…", …)` — by the template, the rest — [`UNKNOWN_PART`].
/// `None` — the expression has not a single known string.
fn lua_command_text(code: &str, expr: &str, at: usize, depth: usize) -> Option<String> {
    if depth > 4 {
        return None;
    }
    let mut text = String::new();
    let mut known = false;
    for part in top_level_split(expr, b"..") {
        let part = part.trim();
        let piece = if let Some((s, _)) = lua_string_at(part, 0).filter(|(_, end)| *end == part.len()) {
            Some(s)
        } else if is_getenv_home(part) {
            // The home directory — like `~` for shell commands ([`resolve`]).
            Some("~".to_string())
        } else if let Some(args) = part.strip_prefix("string.format").map(str::trim_start).and_then(|a| a.strip_prefix('(')) {
            lua_string_at(args, 0).map(|(f, _)| format_template(&f))
        } else if !part.is_empty() && part.bytes().all(|c| is_ident(c) || c == b'.') && !part.as_bytes()[0].is_ascii_digit() {
            // `cmd = cmd .. ' -x'` takes the previous value — it is searched for above
            // the assignment itself.
            lua_assignment(code, part, at).and_then(|(e, p)| lua_command_text(code, e, p, depth + 1))
        } else {
            None
        };
        match piece {
            Some(s) => {
                known = true;
                text.push_str(&s);
            }
            None => text.push_str(UNKNOWN_PART),
        }
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (known && text != UNKNOWN_PART).then_some(text)
}

/// `os.getenv('HOME')` (also with double quotes, with spaces).
fn is_getenv_home(part: &str) -> bool {
    let squeezed: String = part.chars().filter(|c| !c.is_whitespace()).collect();
    squeezed == "os.getenv('HOME')" || squeezed == "os.getenv(\"HOME\")"
}

/// A `string.format` template: the substitutions `%s`, `%d`… — [`UNKNOWN_PART`],
/// `%%` — the percent sign.
fn format_template(f: &str) -> String {
    let mut out = String::new();
    let mut chars = f.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            out.push('%');
            continue;
        }
        while chars.peek().is_some_and(|c| !c.is_ascii_alphabetic()) {
            chars.next();
        }
        chars.next();
        out.push_str(UNKNOWN_PART);
    }
    out
}

/// Command calls from a Lua script: `os.execute(…)`, `io.popen(…)` and
/// directives in its strings (`conky_parse("${exec …}")`).
fn scan_lua(text: &str) -> Vec<(RiskKind, String)> {
    let code = strip_lua_comments(text);
    let b = code.as_bytes();
    let mut out = Vec::new();
    for call in ["os.execute", "io.popen"] {
        for p in code.match_indices(call).map(|(p, _)| p) {
            if p > 0 && (is_ident(b[p - 1]) || b[p - 1] == b'.') {
                continue;
            }
            let mut j = p + call.len();
            while b.get(j).is_some_and(|c| c.is_ascii_whitespace()) {
                j += 1;
            }
            let paren = b.get(j) == Some(&b'(');
            let at = if paren { j + 1 } else { j };
            // A whole literal is the command itself; `"x" .. var` is an expression.
            let literal = lua_string_at(&code, at).filter(|(_, end)| {
                let rest = code[*end..].trim_start();
                !paren || rest.starts_with(')') || rest.starts_with(',')
            });
            let what = match literal {
                Some((s, _)) => s,
                None if paren => {
                    let mut depth = 1;
                    let mut k = at;
                    while k < b.len() && depth > 0 {
                        match b[k] {
                            b'(' => depth += 1,
                            b')' => depth -= 1,
                            _ => {}
                        }
                        k += 1;
                    }
                    let args = &code[at..k.saturating_sub(1).max(at)];
                    // A command assembled from strings (`local cmd = 'cava -p ' ..
                    // CONFIG .. ' 2>/dev/null'` → `io.popen(cmd, 'r')`) —
                    // as text, the unknown parts are "…": this way it is visible both to the person
                    // and to the missing programs check ([`health`]).
                    match lua_command_text(&code, top_level_split(args, b",").first().copied().unwrap_or(""), p, 0) {
                        Some(text) => text,
                        None => {
                            let expr = args.split_whitespace().collect::<Vec<_>>().join(" ");
                            format!("{call}({})", expr.chars().take(120).collect::<String>())
                        }
                    }
                }
                None => continue,
            };
            if !what.trim().is_empty() {
                out.push((RiskKind::Command, what.trim().to_string()));
            }
        }
    }
    out.extend(lua_directives(&code));
    out
}

// ============================================================================
// The theme's shell scripts (`start.sh`, `scripts/*.sh`)
// ============================================================================

/// Programs that launch scripts call for plumbing (changing to a
/// folder, waiting, output, conky itself): lines consisting only of them are not shown
/// in the question — otherwise the real commands would be lost behind them.
const QUIET_PROGRAMS: &[&str] = &[
    "cd", "dirname", "basename", "pwd", "mkdir", "sleep", "echo", "printf", "test", "true", "false",
    "readlink", "realpath", "wait", "trap", "exit", "date", "chmod", "setsid", "conky",
];

/// Programs that go online.
const NETWORK_PROGRAMS: &[&str] = &["curl", "wget"];

/// A longer line is truncated in the question.
const SHELL_LINE_MAX: usize = 160;

/// The commands of a shell script for the question: lines that run
/// something — except plumbing only ([`QUIET_PROGRAMS`]), control words
/// and function headers. Lines with a `\` continuation are joined. A line that
/// calls `curl`/`wget` also "downloads from the internet".
fn scan_shell(text: &str) -> Vec<(RiskKind, String)> {
    let mut out: Vec<(RiskKind, String)> = Vec::new();
    let mut joined: Vec<String> = Vec::new();
    let mut cur = String::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 && line.starts_with("#!") {
            continue;
        }
        let t = line.trim();
        if let Some(head) = t.strip_suffix('\\') {
            cur.push_str(head);
            cur.push(' ');
            continue;
        }
        cur.push_str(t);
        joined.push(std::mem::take(&mut cur));
    }
    if !cur.is_empty() {
        joined.push(cur);
    }
    for line in joined {
        let s = line.trim();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        let head = s.trim_end_matches(['{', ' ']);
        let function = head.strip_suffix("()").is_some_and(|n| !n.trim().is_empty() && n.trim().bytes().all(is_ident))
            || s.starts_with("function ");
        if function {
            continue;
        }
        let programs = command_programs(s);
        let calls_script = s
            .split_whitespace()
            .map(|w| w.trim_matches(|c| c == '"' || c == '\'' || c == '&' || c == ';'))
            .any(|w| w.contains('/') && CODE_EXTS.contains(&ext_of(w).as_str()));
        let loud = programs.iter().any(|p| !QUIET_PROGRAMS.contains(&p.as_str()));
        if !loud && !calls_script {
            continue;
        }
        let shown: String = if s.chars().count() > SHELL_LINE_MAX {
            s.chars().take(SHELL_LINE_MAX).chain("…".chars()).collect()
        } else {
            s.to_string()
        };
        if programs.iter().any(|p| NETWORK_PROGRAMS.contains(&p.as_str())) {
            out.push((RiskKind::Network, shown.clone()));
        }
        out.push((RiskKind::Command, shown));
    }
    out
}

/// Found in the theme's shell script (not in a settings file or Lua):
/// such lines are only shown in the question. [`health`] does not check
/// programs from them — scripts have their own functions and `case`
/// branches, and a "missing program" from them would stop a working widget; the programs
/// the theme needs it names itself (`theme.json`, `requires`).
fn from_shell_script(r: &Risk) -> bool {
    matches!(ext_of(&r.file).as_str(), "sh" | "bash")
}

/// What was found in one file: the dangers and the files they reference.
struct Scan {
    risks: Vec<Risk>,
    refs: Vec<String>,
}

fn scan(io: &dyn LookIo, ctx: &Ctx, w: &Widget) -> Scan {
    let mut risks: Vec<Risk> = Vec::new();
    let mut refs: Vec<String> = Vec::new();
    let push = |risks: &mut Vec<Risk>, kind: RiskKind, what: String, file: &str| {
        let r = Risk { kind, what, file: rel(&w.base, file) };
        if !risks.contains(&r) {
            risks.push(r);
        }
    };
    // Lua scripts: every `.lua` of the widget and whatever `lua_load` loads.
    let mut lua_files: Vec<String> = Vec::new();
    if w.path == w.base {
        let mut all = Vec::new();
        walk(io, &w.base, 1, &mut all);
        lua_files.extend(all.into_iter().filter(|p| ext_of(p) == "lua"));
    }
    for conf in &w.confs {
        let Some(text) = io.read(conf) else { continue };
        let work_dir = crate::de_apply::conky_work_dir(conf).to_string();
        for (kind, what) in scan_conf(&text) {
            match kind {
                RiskKind::LuaScript => {
                    if let Some(p) = resolve(ctx, &work_dir, &what).or_else(|| Some(format!("{work_dir}/{what}"))) {
                        if !lua_files.contains(&p) {
                            lua_files.push(p.clone());
                        }
                        refs.push(p);
                    }
                }
                RiskKind::Command => {
                    refs.extend(what.split_whitespace().filter_map(|t| resolve(ctx, &work_dir, t)));
                }
                RiskKind::Network => {}
            }
            push(&mut risks, kind, what, conf);
        }
    }
    for lua in &lua_files {
        let Some(text) = io.read(lua) else { continue };
        let work_dir = w.confs.first().map(|c| crate::de_apply::conky_work_dir(c).to_string()).unwrap_or_else(|| w.base.clone());
        for (kind, what) in scan_lua(&text) {
            if kind == RiskKind::Command {
                refs.extend(
                    what.split_whitespace().filter(|t| !t.contains(UNKNOWN_PART)).filter_map(|t| resolve(ctx, &work_dir, t)),
                );
            }
            push(&mut risks, kind, what, lua);
        }
    }
    // A theme with a launch script (`start.sh`) is started by it, and the script itself and
    // `scripts/*.sh` are executed entirely — their commands are in the question too. The files
    // themselves are already in the fingerprint (the `.sh` extension), so the answer is remembered
    // until they change.
    if w.path == w.base {
        for script in crate::conky_theme::shell_scripts(io, &w.base) {
            let Some(text) = io.read(&script) else { continue };
            push(&mut risks, RiskKind::Command, rel(&w.base, &script), &script);
            for (kind, what) in scan_shell(&text) {
                push(&mut risks, kind, what, &script);
            }
        }
    }
    Scan { risks, refs }
}

/// Dangerous directives of the widget `id` (a name in the Rexon catalog or the path
/// of one found on disk). Empty — the widget only draws.
pub fn scan_widget(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Vec<Risk> {
    let w = widget(io, ctx, id);
    let mut risks = scan(io, ctx, &w).risks;
    risks.sort_by(|a, b| a.kind.cmp(&b.kind));
    risks
}

// ============================================================================
// Fingerprint and permissions
// ============================================================================

/// The fingerprint of the widget's code files: settings files, scripts (by extension
/// or `#!` in the first line) and files the directives reference.
/// Binary and other data (a cache, pictures) is not included.
fn fingerprint(io: &dyn LookIo, w: &Widget, refs: &[String]) -> String {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut add = |path: &str, text: String| {
        if !files.iter().any(|(p, _)| p == path) {
            files.push((path.to_string(), text));
        }
    };
    for conf in &w.confs {
        if let Some(t) = io.read(conf) {
            add(conf, t);
        }
    }
    if w.path == w.base {
        let mut all = Vec::new();
        walk(io, &w.base, 1, &mut all);
        for p in all {
            let Some(t) = io.read(&p) else { continue };
            let code = CODE_EXTS.contains(&ext_of(&p).as_str())
                || t.starts_with("#!")
                || crate::conky_scan::looks_like_config(&t);
            if code {
                add(&p, t);
            }
        }
    }
    for r in refs {
        if let Some(t) = io.read(r) {
            add(r, t);
        }
    }
    files.sort();
    let mut h = Sha256::new();
    for (p, t) in &files {
        h.update(rel(&w.base, p).as_bytes());
        h.update([0u8]);
        h.update((t.len() as u64).to_le_bytes());
        h.update(t.as_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The fingerprint of the code files of the widget `id` in its current form — the same as the
/// permission's. The widget snapshot for the tile (`conky_preview`) uses it to learn
/// that the widget changed and the snapshot needs to be retaken.
pub fn code_fingerprint(io: &dyn LookIo, ctx: &Ctx, id: &str) -> String {
    let w = widget(io, ctx, id);
    let refs = scan(io, ctx, &w).refs;
    fingerprint(io, &w, &refs)
}

/// The files of the widget `id` where it sets its look: settings files and Lua
/// scripts — every `.lua` of its directory and whatever `lua_load` loads (the same way
/// the dangerous directives check sees them). The color palette
/// (`conky_colors`) searches for colors exactly in them.
pub fn look_files(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Vec<String> {
    let w = widget(io, ctx, id);
    let mut out = w.confs.clone();
    let add = |p: String, out: &mut Vec<String>| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    if w.path == w.base {
        let mut all = Vec::new();
        walk(io, &w.base, 1, &mut all);
        for p in all.into_iter().filter(|p| ext_of(p) == "lua") {
            add(p, &mut out);
        }
    }
    for conf in &w.confs {
        let Some(text) = io.read(conf) else { continue };
        let work_dir = crate::de_apply::conky_work_dir(conf).to_string();
        for (kind, what) in scan_conf(&text) {
            if kind == RiskKind::LuaScript {
                if let Some(p) = resolve(ctx, &work_dir, &what).or_else(|| Some(format!("{work_dir}/{what}"))) {
                    if io.read(&p).is_some() {
                        add(p, &mut out);
                    }
                }
            }
        }
    }
    out
}

/// The widget runs commands, and the person allowed it exactly in its current form.
/// Rexon, when changing only the colors of such a widget, remembers the permission
/// again after the edit ([`remember`]) — our own edit must not make
/// the person answer the question about commands a second time.
pub fn trusted_as_is(io: &dyn LookIo, ctx: &Ctx, id: &str) -> bool {
    let w = widget(io, ctx, id);
    let Scan { risks, refs } = scan(io, ctx, &w);
    !risks.is_empty() && trusted_hash(io, ctx, &w.path).is_some_and(|h| h == fingerprint(io, &w, &refs))
}

/// The permissions file: `~/.config/rexon/conky-trust.json`.
pub fn trust_path(ctx: &Ctx) -> String {
    ctx.config("rexon/conky-trust.json")
}

fn trusted_hash(io: &dyn LookIo, ctx: &Ctx, key: &str) -> Option<String> {
    let text = io.read(&trust_path(ctx))?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("widgets")?.get(key)?.get("sha256")?.as_str().map(str::to_string)
}

/// The widget's state before a launch.
enum Verdict {
    /// Nothing to ask about: it only draws or is already allowed.
    Free,
    /// A question is needed; `changed` — it was allowed, but the files have changed since.
    Ask { risks: Vec<Risk>, changed: bool },
}

fn verdict(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Verdict {
    let w = widget(io, ctx, id);
    let Scan { mut risks, refs } = scan(io, ctx, &w);
    if risks.is_empty() {
        return Verdict::Free;
    }
    risks.sort_by(|a, b| a.kind.cmp(&b.kind));
    match trusted_hash(io, ctx, &w.path) {
        Some(h) if h == fingerprint(io, &w, &refs) => Verdict::Free,
        Some(_) => Verdict::Ask { risks, changed: true },
        None => Verdict::Ask { risks, changed: false },
    }
}

/// Remember the permission for the widget in its current form. `false` — the permissions
/// file was not written.
pub fn remember(io: &dyn LookIo, ctx: &Ctx, id: &str) -> bool {
    let w = widget(io, ctx, id);
    let refs = scan(io, ctx, &w).refs;
    let hash = fingerprint(io, &w, &refs);
    let path = trust_path(ctx);
    let mut v: serde_json::Value = io
        .read(&path)
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    v["version"] = serde_json::json!(1);
    if !v.get("widgets").is_some_and(serde_json::Value::is_object) {
        v["widgets"] = serde_json::json!({});
    }
    v["widgets"][w.path.as_str()] = serde_json::json!({ "sha256": hash });
    let text = serde_json::to_string_pretty(&v).unwrap_or_default();
    let ok = io.write(&path, &format!("{text}\n"));
    // An allowed widget can be snapshotted for the tile without a question.
    if ok {
        crate::conky_preview::want(ctx, id);
    }
    ok
}

// ============================================================================
// Words for the person
// ============================================================================

/// No more than this many lines of each kind in the question.
const LIST_LIMIT: usize = 8;

fn list_block(out: &mut String, header: String, items: &[&Risk], main_file: &str) {
    if items.is_empty() {
        return;
    }
    out.push_str("\n\n");
    out.push_str(&header);
    for r in items.iter().take(LIST_LIMIT) {
        out.push_str("\n• ");
        out.push_str(&r.what);
        if r.file != main_file && r.file != r.what {
            out.push_str(&format!(" ({})", r.file));
        }
    }
    if items.len() > LIST_LIMIT {
        let more = (items.len() - LIST_LIMIT) as u64;
        out.push_str("\n• ");
        out.push_str(&ntr("…and %n more.", "…and %n more.", more));
    }
}

/// The question before a launch: `(title, text)`. `None` — nothing to ask
/// about: the widget only draws or is already allowed in its current form.
pub fn start_question(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Option<(String, String)> {
    let Verdict::Ask { risks, changed } = verdict(io, ctx, id) else { return None };
    let title = crate::conky_scan::widget_title(id);
    let w = widget(io, ctx, id);
    let main_file = w.confs.first().map(|c| rel(&w.base, c)).unwrap_or_default();
    let mut text = String::new();
    if changed {
        text.push_str(&changed_since_allowed_text());
        text.push_str("\n\n");
    }
    text.push_str(&trf(
        // TRANSLATORS: %1 is the widget name.
        "The Conky widget “%1” doesn't just draw: it runs programs on your computer with your rights. \
         Check what it does before starting it.",
        &[&title],
    ));
    let of = |k: RiskKind| risks.iter().filter(|r| r.kind == k).collect::<Vec<_>>();
    list_block(&mut text, tr("Runs commands:"), &of(RiskKind::Command), &main_file);
    list_block(&mut text, tr("Loads Lua scripts:"), &of(RiskKind::LuaScript), &main_file);
    list_block(&mut text, tr("Downloads from the internet:"), &of(RiskKind::Network), &main_file);
    text.push_str("\n\n");
    text.push_str(&tr(
        "Allow it only if you trust where the widget came from. Rexon will remember the answer \
         until the widget's files change.",
    ));
    Some((tr("Allow the widget to run commands?"), text))
}

/// The refusal of the "Start" button while the widget is not allowed (the interface asks
/// the question before the launch; launches that bypass the question come here — "Install
/// and apply" from the store). `None` — it may be started.
pub fn start_refusal(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Option<String> {
    let Verdict::Ask { .. } = verdict(io, ctx, id) else { return None };
    Some(trf(
        // TRANSLATORS: %1 is the widget name; “▶️ Start” is the button on its tile.
        "The widget “%1” wasn't started: it runs commands or scripts, and you haven't allowed that yet. \
         Press “▶️ Start” on its tile to see what it runs.",
        &[&crate::conky_scan::widget_title(id)],
    ))
}

/// The explanation in the question when the widget was allowed, but its files have changed
/// since.
pub fn changed_since_allowed_text() -> String {
    tr("⚠️ The widget's files have changed since you allowed it.")
}

/// The result when the person did not allow it.
pub fn declined_text(id: &str) -> String {
    trf(
        // TRANSLATORS: %1 is the widget name.
        "The widget “%1” wasn't started: you didn't allow it to run commands.",
        &[&crate::conky_scan::widget_title(id)],
    )
}

// ============================================================================
// Health: what the widget is missing (the parity plan, item 40)
// ============================================================================

/// Shell words that are not programs: built-in commands and keywords.
/// Their absence from PATH is no problem.
const SHELL_WORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in",
    "function", "echo", "printf", "cd", "test", "[", "[[", "]", "]]", "export", "local", "read",
    "set", "unset", "eval", "source", ".", "true", "false", "exit", "return", "shift", "let",
    "declare", "type", "wait", "trap", "!", "{", "}", "(", ")",
];

/// Wrappers followed by the real program.
const WRAPPERS: &[&str] = &["env", "sudo", "nice", "nohup", "exec", "command", "stdbuf", "ionice"];

/// A program the widget is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingProgram {
    pub program: String,
    /// The package on this distribution family; `None` — Rexon does not know it
    /// or must not install it (nvidia-smi comes with the driver).
    pub package: Option<String>,
    /// A package from the AUR when the repositories do not have the program: Rexon does not
    /// install it, but names it — the person will install it themselves.
    pub aur: Option<String>,
}

/// What the widget is missing in order to work.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Health {
    /// Scripts and files the widget references that are missing — as
    /// written in the widget.
    pub missing_files: Vec<String>,
    pub missing_programs: Vec<MissingProgram>,
}

impl Health {
    pub fn is_fine(&self) -> bool {
        self.missing_files.is_empty() && self.missing_programs.is_empty()
    }

    /// Packages that can be installed (without repeats).
    pub fn packages(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in self.missing_programs.iter().filter_map(|m| m.package.clone()) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }
}

/// The package of the program `program` for the family `family` — only an exact record of
/// the `utils::TOOL_PACKAGES` table (without "as in Debian" for unfamiliar
/// families and without AUR-only packages: a regular install does not install them).
pub fn program_package(program: &str, family: &str) -> Option<String> {
    crate::utils::TOOL_PACKAGES
        .iter()
        .find(|(bin, _)| *bin == program)?
        .1
        .iter()
        .find(|(f, _)| *f == family)
        .map(|(_, pkg)| *pkg)
        .filter(|pkg| !pkg.is_empty() && *pkg != "—" && !pkg.contains('('))
        .map(str::to_string)
}

/// The AUR package of the program `program` when the repositories of the family
/// `family` do not have it (the "from AUR" mark of the `utils::TOOL_PACKAGES` table).
pub fn program_aur_package(program: &str, family: &str) -> Option<String> {
    crate::utils::TOOL_PACKAGES
        .iter()
        .find(|(bin, _)| *bin == program)?
        .1
        .iter()
        .find(|(f, _)| *f == family)?
        .1
        .strip_suffix(crate::utils::AUR_ONLY_MARK)
        .map(str::to_string)
}

/// The programs of a shell command: the first word of each link (`a | b; c &&
/// d`), without `X=1` assignments, wrappers (`env`, `sudo`) and built-in words.
/// Words with `$`, paths and quoted expressions are skipped: a path is
/// a file, and `$X` is known only at launch.
fn command_programs(cmd: &str) -> Vec<String> {
    let mut text = cmd.to_string();
    for sep in ["&&", "||", "$(", "`", "|", ";", "&", "(", ")"] {
        text = text.replace(sep, "\n");
    }
    let mut out: Vec<String> = Vec::new();
    for seg in text.lines() {
        let mut words = seg.split_whitespace().peekable();
        while let Some(w) = words.peek() {
            let assignment = w.split_once('=').is_some_and(|(k, _)| !k.is_empty() && k.bytes().all(is_ident));
            if assignment || WRAPPERS.contains(w) || w.starts_with('-') {
                words.next();
            } else {
                break;
            }
        }
        let Some(first) = words.next() else { continue };
        let prog = first.trim_matches(|c| c == '"' || c == '\'');
        let plain = !prog.is_empty()
            && !prog.contains(['/', '$', '{', '}', '<', '>', '=', '*', '"', '\'', '…'])
            && !prog.chars().all(|c| c.is_ascii_digit());
        if plain && !SHELL_WORDS.contains(&prog) && !out.iter().any(|p| p == prog) {
            out.push(prog.to_string());
        }
    }
    out
}

/// Programs the commands of the widget `id` call (`cava`, `sensors`) —
/// for the dependency list of a set saved to a file: another machine may
/// not have them. A Lua call with an expression is not parsed (the command is known
/// only at launch), the Conky program itself does not count.
pub fn widget_programs(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Vec<String> {
    let w = widget(io, ctx, id);
    let mut out: Vec<String> = Vec::new();
    for r in scan(io, ctx, &w).risks {
        if r.kind != RiskKind::Command
            || r.what.starts_with("os.execute(")
            || r.what.starts_with("io.popen(")
            || from_shell_script(&r)
        {
            continue;
        }
        for prog in command_programs(&r.what) {
            if prog != "conky" && !out.contains(&prog) {
                out.push(prog);
            }
        }
    }
    for prog in theme_programs(io, &w) {
        if !out.contains(&prog) {
            out.push(prog);
        }
    }
    out
}

/// The health check of the widget `id`: missing scripts (`lua_load`,
/// shell scripts in commands) and programs from commands — with a package for
/// the family `family` (`system::PkgFamily::as_str`).
pub fn health(io: &dyn LookIo, ctx: &Ctx, id: &str, family: &str) -> Health {
    let w = widget(io, ctx, id);
    let risks = scan(io, ctx, &w).risks;
    let work_dir = w.confs.first().map(|c| crate::de_apply::conky_work_dir(c).to_string()).unwrap_or_else(|| w.base.clone());
    let mut h = Health::default();
    let missing_file = |h: &mut Health, what: &str, path: String| {
        if io.read(&path).is_none() && io.list_dir(&path).is_empty() && !h.missing_files.iter().any(|f| f == what) {
            h.missing_files.push(what.to_string());
        }
    };
    for r in &risks {
        match r.kind {
            RiskKind::LuaScript => {
                let path = resolve(ctx, &work_dir, &r.what).unwrap_or_else(|| format!("{work_dir}/{}", r.what));
                missing_file(&mut h, &r.what, path);
            }
            RiskKind::Command => {
                // A Lua call with an expression — the command is known only at
                // launch. Lines of the theme's shell script are only for
                // the question ([`from_shell_script`]).
                if r.what.starts_with("os.execute(") || r.what.starts_with("io.popen(") || from_shell_script(r) {
                    continue;
                }
                for t in r.what.split_whitespace() {
                    let t = t.trim_matches(|c| c == '"' || c == '\'');
                    if !CODE_EXTS.contains(&ext_of(t).as_str()) || t.contains('$') || t.contains(UNKNOWN_PART) {
                        continue;
                    }
                    if let Some(path) = resolve(ctx, &work_dir, t) {
                        missing_file(&mut h, t, path);
                    }
                }
                for prog in command_programs(&r.what) {
                    if io.have(&prog) || h.missing_programs.iter().any(|m| m.program == prog) {
                        continue;
                    }
                    let package = program_package(&prog, family);
                    let aur = program_aur_package(&prog, family);
                    h.missing_programs.push(MissingProgram { program: prog, package, aur });
                }
            }
            RiskKind::Network => {}
        }
    }
    // Programs the theme names itself (`theme.json`, `requires`).
    for prog in theme_programs(io, &w) {
        if io.have(&prog) || h.missing_programs.iter().any(|m| m.program == prog) {
            continue;
        }
        let package = program_package(&prog, family);
        let aur = program_aur_package(&prog, family);
        h.missing_programs.push(MissingProgram { program: prog, package, aur });
    }
    h
}

/// Programs from the theme description (`theme.json`, the `requires` field) — for a widget
/// that is a folder.
fn theme_programs(io: &dyn LookIo, w: &Widget) -> Vec<String> {
    if w.path != w.base {
        return Vec::new();
    }
    crate::conky_theme::meta(io, &w.base).map(|m| crate::conky_theme::required_programs(&m)).unwrap_or_default()
}

/// What to do with launching a widget that is missing something. The owner's rule
/// of 24.10 — "all or nothing": there are no half-working widgets.
/// Rexon installs programs from the official repositories itself, without a
/// "Yes/No" question (the protection is the polkit password window); if it did not install, or the program
/// is only in the AUR or nowhere — the widget does not start, and the result says
/// why and what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartPlan {
    /// There are enough programs — start.
    Go,
    /// First install these packages (repositories only), then
    /// [`after_install`].
    Install(Vec<String>),
    /// Do not start: the reason and what to do.
    Refuse(String),
}

/// The button with which the person will retry: the tile's "▶️ Start" or
/// the widget part's button (a part does not install packages — the tile launch does).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    Tile,
    Part,
    /// "Apply" of a widget set (the window or `rexon --conky-set`).
    Set,
    /// The "Start at login" switch: at login the widget will start
    /// without Rexon, so the programs are installed when it is turned on.
    Autostart,
}

/// A launch refusal: which programs keep the widget from working and what
/// to do with each. `installable` — Rexon can install packages here;
/// `failed` — the reason what was being installed did not install.
fn refusal(id: &str, h: &Health, installable: bool, failed: Option<&str>, retry: Retry) -> String {
    let all: Vec<&str> = h.missing_programs.iter().map(|m| m.program.as_str()).collect();
    let repo: Vec<&MissingProgram> = h.missing_programs.iter().filter(|m| installable && m.package.is_some()).collect();
    let aur: Vec<&MissingProgram> = h.missing_programs.iter().filter(|m| m.package.is_none() && m.aur.is_some()).collect();
    let none: Vec<&str> = h
        .missing_programs
        .iter()
        .filter(|m| m.aur.is_none() && !(installable && m.package.is_some()))
        .map(|m| m.program.as_str())
        .collect();
    let n = all.len() as u64;
    let mut parts = vec![crate::i18n::fill(
        // TRANSLATORS: %1 is the widget name, %2 a list of programs.
        &ntr(
            "The widget “%1” wasn't started: without the program %2 it doesn't fully work.",
            "The widget “%1” wasn't started: without the programs %2 it doesn't fully work.",
            n,
        ),
        &[&crate::conky_scan::widget_title(id), &all.join(", ")],
    )];
    // The password window was closed — that is "changed my mind", not a breakage: we do not
    // repeat the reason, the advice below is about the password anyway.
    if let Some(why) = failed.filter(|w| *w != crate::appearance_installer::password_not_confirmed()) {
        parts.push(trf("Installation failed: %1.", &[&why]));
    }
    if !aur.is_empty() {
        let names: Vec<&str> = aur.iter().map(|m| m.program.as_str()).collect();
        let pkgs: Vec<&str> = aur.iter().filter_map(|m| m.aur.as_deref()).collect();
        parts.push(crate::i18n::fill(
            // TRANSLATORS: %1 is a list of programs, %2 an example command
            // that installs them from the AUR (yay -S …).
            &ntr(
                "%1 is only in the AUR — install it (for example, %2) and start the widget again.",
                "%1 are only in the AUR — install them (for example, %2) and start the widget again.",
                names.len() as u64,
            ),
            &[&names.join(", "), &format!("yay -S {}", pkgs.join(" "))],
        ));
    }
    if !none.is_empty() {
        // TRANSLATORS: %1 is a list of programs.
        parts.push(trf("Install %1 yourself and start the widget again.", &[&none.join(", ")]));
    }
    // Nothing to install by hand — a repeated "Start" will install everything itself.
    if !repo.is_empty() && aur.is_empty() && none.is_empty() {
        let n = repo.len() as u64;
        let start = tr("▶️ Start");
        parts.push(match retry {
            Retry::Tile => crate::i18n::fill(
                // TRANSLATORS: %1 is the label of the widget's start button.
                &ntr(
                    "Press “%1” again and enter the password to install it.",
                    "Press “%1” again and enter the password to install them.",
                    n,
                ),
                &[&start],
            ),
            Retry::Part => crate::i18n::fill(
                // TRANSLATORS: %1 is the label of the start button on the
                // widget's tile.
                &ntr(
                    "Start the widget with “%1” on its tile and enter the password to install it.",
                    "Start the widget with “%1” on its tile and enter the password to install them.",
                    n,
                ),
                &[&start],
            ),
            Retry::Set => ntr(
                "Apply the set again and enter the password to install it.",
                "Apply the set again and enter the password to install them.",
                n,
            ),
            Retry::Autostart => crate::i18n::fill(
                // TRANSLATORS: %1 is the label of the widget's autostart switch.
                &ntr(
                    "Turn on “%1” again and enter the password to install it.",
                    "Turn on “%1” again and enter the password to install them.",
                    n,
                ),
                &[&crate::appearance_page::detail_autostart_text()],
            ),
        });
    }
    parts.join(" ")
}

/// The launch plan of the widget `id` from its check (`installable` — not SteamOS,
/// not an immutable system). Only what is in the official
/// repositories is installed, and only when that is enough: if something will have to be installed
/// by the person (AUR, no package), no password is asked at all.
pub fn start_plan(id: &str, h: &Health, installable: bool) -> StartPlan {
    plan_for(id, h, installable, Retry::Tile)
}

fn plan_for(id: &str, h: &Health, installable: bool, retry: Retry) -> StartPlan {
    if h.missing_programs.is_empty() {
        return StartPlan::Go;
    }
    let self_installable = installable && h.missing_programs.iter().all(|m| m.package.is_some());
    if self_installable {
        StartPlan::Install(h.packages())
    } else {
        StartPlan::Refuse(refusal(id, h, installable, None, retry))
    }
}

/// The plan after installing the [`StartPlan::Install`] packages: `fresh` — the widget
/// check done ANEW (what got installed is decided by the disk, not the return code), `result` —
/// the install outcome. If it did not install, the widget does not start.
pub fn after_install(id: &str, before: &Health, fresh: &Health, result: &Result<(), String>) -> StartPlan {
    after_install_for(id, before, fresh, result, Retry::Tile)
}

fn after_install_for(id: &str, before: &Health, fresh: &Health, result: &Result<(), String>, retry: Retry) -> StartPlan {
    match result {
        Err(why) => StartPlan::Refuse(refusal(id, before, true, Some(why), retry)),
        Ok(()) if fresh.missing_programs.is_empty() => StartPlan::Go,
        // The package got installed, but the program is still missing — there is nothing else to install it with.
        Ok(()) => StartPlan::Refuse(refusal(id, fresh, false, None, retry)),
    }
}

/// Whether the widget is ready to start: `Ok(note)` — start (the note is
/// [`files_note`], to be appended to the result), `Err(refusal)` — do not start.
pub type Readiness = Result<Option<String>, String>;

/// The shared "all or nothing" path before launching the widgets `ids` — for the tile's "▶️
/// Start", a set's "Apply" and `rexon --conky-set`: checking
/// each ([`health`]), the missing packages from the official repositories of
/// ALL widgets — with one `install` (one password window for the whole
/// set, without a "Yes/No" question), then a check anew: if something did not install —
/// that widget does not start, and the refusal says why and what to do
/// (`retry` — which button to retry with). The answer is one per `id`,
/// in the same order.
pub fn prepare_start(
    io: &dyn LookIo,
    ctx: &Ctx,
    ids: &[String],
    family: &str,
    installable: bool,
    retry: Retry,
    install: &dyn Fn(&[String]) -> Result<(), String>,
) -> Vec<Readiness> {
    let checks: Vec<(Health, StartPlan)> = ids
        .iter()
        .map(|id| {
            let h = health(io, ctx, id, family);
            let plan = plan_for(id, &h, installable, retry);
            (h, plan)
        })
        .collect();
    let mut packages: Vec<String> = Vec::new();
    for (_, plan) in &checks {
        if let StartPlan::Install(list) = plan {
            for p in list {
                if !packages.contains(p) {
                    packages.push(p.clone());
                }
            }
        }
    }
    let result = if packages.is_empty() { Ok(()) } else { install(&packages) };
    ids.iter()
        .zip(checks)
        .map(|(id, (h, plan))| {
            let plan = match plan {
                StartPlan::Install(_) => after_install_for(id, &h, &health(io, ctx, id, family), &result, retry),
                other => other,
            };
            match plan {
                StartPlan::Refuse(text) => Err(text),
                _ => Ok(files_note(&h)),
            }
        })
        .collect()
}

/// [`prepare_start`] on this machine: packages — [`install_packages_live`]
/// (pkexec + helper, repositories only).
pub fn live_prepare_start(ids: &[String], retry: Retry) -> Vec<Readiness> {
    let (family, installable) = live_family();
    let install = |packages: &[String]| {
        let result = install_packages_live(packages);
        if let Err(why) = &result {
            crate::log_warn!("conky", "пакеты виджетов Conky не встали ({}): {why}", packages.join(", "));
        }
        result
    };
    prepare_start(&crate::animations::SystemIo, &Ctx::live(), ids, &family, installable, retry, &install)
}

/// A refusal for the widget part's button: it runs in the interface thread and
/// does not install packages. `None` — there are enough programs.
pub fn part_refusal(id: &str, h: &Health, installable: bool) -> Option<String> {
    (!h.missing_programs.is_empty()).then(|| refusal(id, h, installable, None, Retry::Part))
}

/// Missing widget files — appended to the launch result (a package cannot
/// install them). `None` — everything is in place.
pub fn files_note(h: &Health) -> Option<String> {
    (!h.missing_files.is_empty()).then(|| trf("⚠️ The widget's files are missing: %1.", &[&h.missing_files.join(", ")]))
}

/// THIS machine's package family and whether Rexon can install packages on it
/// (on SteamOS and immutable systems a system package will not install).
pub fn live_family() -> (String, bool) {
    let detector = crate::system::DistroDetector::detect_live();
    let installable = !detector.is_steamos() && !detector.is_immutable();
    (detector.family_live().as_str().to_string(), installable)
}

/// Install the widget's missing packages the same way as all of Rexon's package
/// installs (pkexec + `rexon-pkg-helper`), but only from the official
/// repositories (`appearance_installer::live_install_repo_packages`): the AUR
/// is not built without a question.
/// `Err` — the reason for the person.
pub fn install_packages_live(packages: &[String]) -> Result<(), String> {
    if !live_family().1 {
        return Err(tr("Rexon can't install programs on this system by itself."));
    }
    crate::appearance_installer::live_install_repo_packages(packages, crate::privilege::op::PkgSource::Tools)
}

// ============================================================================
// Old Lua scripts on a new conky
// ============================================================================

/// The mark of the lines Rexon appends to a widget's Lua script.
pub const LUA_COMPAT_MARK: &str = "-- Rexon: newer Conky keeps the X11 drawing functions in a separate module.";

/// A widget's Lua script that draws through `cairo_xlib_*` without
/// `require 'cairo_xlib'` — with this line (and `require 'cairo'` if that
/// is missing too), otherwise `None`.
///
/// WHY. The new conky (1.25) keeps the X11 drawing functions in a separate
/// `cairo_xlib` module; a script written for earlier releases (there they were
/// provided by `require 'cairo'`) fails on the very first frame:
/// `cairo_xlib_surface_create` is `nil` (measured 15.11.2026 on Jhalak).
/// It is included via `pcall`: on an old conky that has no such module, the
/// line breaks nothing. A repeat changes nothing.
pub fn lua_xlib_compat(text: &str) -> Option<String> {
    let code = strip_lua_comments(text);
    if !code.contains("cairo_xlib_") || text.contains(LUA_COMPAT_MARK) {
        return None;
    }
    let names = |m: &str| [format!("'{m}'"), format!("\"{m}\""), format!("[[{m}]]")];
    let has = |m: &str| names(m).iter().any(|n| code.contains(n.as_str()));
    if has("cairo_xlib") {
        return None;
    }
    let mut add = format!("{LUA_COMPAT_MARK}\n");
    if !has("cairo") {
        add.push_str("pcall(require, 'cairo')\n");
    }
    add.push_str("pcall(require, 'cairo_xlib')\n");
    // The `#!` line stays first.
    let at = if text.starts_with("#!") { text.find('\n').map_or(text.len(), |p| p + 1) } else { 0 };
    let mut out = String::with_capacity(text.len() + add.len() + 1);
    out.push_str(&text[..at]);
    if at == text.len() && at > 0 && !text.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&add);
    out.push_str(&text[at..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_xlib_compat_adds_require_once() {
        // Jhalak: `require 'cairo'` is there, `cairo_xlib` is not.
        let jhalak = "require 'cairo'\nfunction conky_main()\n  local cs = cairo_xlib_surface_create(conky_window.display)\nend\n";
        let fixed = lua_xlib_compat(jhalak).expect("нужна строка");
        assert!(fixed.starts_with(LUA_COMPAT_MARK), "{fixed}");
        assert!(fixed.contains("pcall(require, 'cairo_xlib')\nrequire 'cairo'\n"), "{fixed}");
        assert!(!fixed.contains("pcall(require, 'cairo')"), "cairo уже подключён: {fixed}");
        assert_eq!(lua_xlib_compat(&fixed), None, "повтор");
        // Not a single require — both; `#!` stays first.
        let bare = "#!/usr/bin/lua\ncs = cairo_xlib_surface_create(a)";
        let fixed = lua_xlib_compat(bare).unwrap();
        assert!(fixed.starts_with("#!/usr/bin/lua\n-- Rexon:"), "{fixed}");
        assert!(fixed.contains("pcall(require, 'cairo')\npcall(require, 'cairo_xlib')\ncs ="), "{fixed}");
        // Already included in any notation; mentioned only in a comment;
        // a script without X11 drawing — not touched.
        assert_eq!(lua_xlib_compat("require(\"cairo_xlib\")\ncairo_xlib_surface_create(a)"), None);
        assert_eq!(lua_xlib_compat("-- cairo_xlib_surface_create\nprint(1)"), None);
        assert_eq!(lua_xlib_compat("require 'cairo'\ncairo_create(conky_surface())"), None);
    }

    #[test]
    fn lua_comments_are_cut_but_strings_kept() {
        let s = strip_lua_comments("a = '--x' -- y\n--[[ z\n]] b = [[--w]]\n");
        assert!(s.contains("'--x'") && !s.contains('y') && !s.contains('z') && s.contains("[[--w]]"), "{s}");
        assert_eq!(s.lines().count(), 3);
    }

    #[test]
    fn directives_with_interval_and_nesting() {
        let d = directives("${execi 10 echo ${time}} ${exec  ls -l } ${curl http://a/b 5} ${time %H}");
        assert_eq!(
            d,
            vec![
                (RiskKind::Command, "echo ${time}".to_string()),
                (RiskKind::Command, "ls -l".to_string()),
                (RiskKind::Network, "http://a/b".to_string()),
            ]
        );
    }

    /// A directive in a Lua script broken by concatenation with the home directory
    /// (the skyrim widget): the path is from `~`, not from the root; an unknown variable is
    /// "…", and the file check does not touch such a path.
    #[test]
    fn lua_directive_split_by_concatenation() {
        let lua = "local HOME = os.getenv('HOME')\n\
                   local a = { expr = '${execi 2 bash ' .. HOME .. '/.config/conky/w/scripts/gpu.sh}' }\n\
                   local b = \"${execi 5 bash \" .. os.getenv( \"HOME\" ) .. \"/w/t.sh}\"\n\
                   local c = '${execi 9 sh ' .. DIR .. '/x.sh}'\n\
                   local d = '${execi 3 date +%H}'\n";
        let d: Vec<String> = scan_lua(lua).into_iter().map(|(_, w)| w).collect();
        assert_eq!(
            d,
            [
                "bash ~/.config/conky/w/scripts/gpu.sh",
                "bash ~/w/t.sh",
                "sh …/x.sh",
                "date +%H",
            ]
        );
        assert!(is_getenv_home(" os.getenv ( 'HOME' ) "));
        assert!(!is_getenv_home("os.getenv('USER')"));
    }

    #[test]
    fn lua_calls_with_expression() {
        let d = scan_lua("os.execute('a b')\nlocal h = io.popen(\"x \" .. y)\nmyos.execute('no')\n");
        assert_eq!(d[0], (RiskKind::Command, "a b".to_string()));
        // A string with an unknown part — as text, the unknown part is "…".
        assert_eq!(d[1], (RiskKind::Command, "x …".to_string()));
        assert_eq!(d.len(), 2);
        // No strings at all — as a call with an expression.
        let d = scan_lua("io.popen(make_cmd(a), 'r')\n");
        assert_eq!(d, vec![(RiskKind::Command, "io.popen(make_cmd(a), 'r')".to_string())]);
    }

    #[test]
    fn lua_command_assembled_from_variables() {
        let lua = "local CONF = './cava.conf'\n\
                   local cmd = 'cava -p ' .. CONF ..\n    ' 2>/dev/null'\n\
                   p = io.popen(cmd, 'r')\n\
                   local n = string.format(\"notify-send %s '%d%%'\", title, v)\n\
                   os.execute(n)\n\
                   local s = 'ls'\n\
                   s = s .. ' -l ' .. dir\n\
                   os.execute(s)\n";
        let d: Vec<String> = scan_lua(lua).into_iter().map(|(_, w)| w).collect();
        assert_eq!(d, vec!["notify-send … '…%'", "ls -l …", "cava -p ./cava.conf 2>/dev/null"], "{d:?}");
        assert_eq!(command_programs("… -x | grep y"), vec!["grep".to_string()]);
    }
}
