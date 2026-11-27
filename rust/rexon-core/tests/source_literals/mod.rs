//! String literals of source files, for the checks that read the sources.
//!
//! WHY. Several guards read the tree's own sources and look only at
//! string literals: comments and names in the code are not text a person
//! sees. A regular expression breaks on this silently (raw strings
//! `r#"…"#`, escaped quotes, `'x'` next to the lifetime `'a`, `//` inside a
//! URL), and a silent check is worse than a missing one — so the parsing is
//! hand-written and shared.
//!
//! Wiring: `mod source_literals;` in the check file (a directory, not a
//! top-level file — so that cargo does not take it for a separate check).

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Source language: Rust, or the JavaScript family (QML, GNOME/Cinnamon
/// extensions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Js,
}

impl Lang {
    /// Language by file extension; `None` for files the checks do not read.
    pub fn of(path: &Path) -> Option<Lang> {
        match path.extension()?.to_str()? {
            "rs" => Some(Lang::Rust),
            "qml" | "js" | "mjs" => Some(Lang::Js),
            _ => None,
        }
    }
}

/// One string literal: the lines it starts and ends on (1-based) and its
/// content. Escapes are replaced with a space; a Rust `\` + line break is
/// eaten together with the indent, as the compiler does it.
#[derive(Clone, Debug)]
pub struct Literal {
    pub line: usize,
    pub end_line: usize,
    pub text: String,
}

/// Files with the given extensions under `dir`, recursively and sorted, skipping
/// directories whose name is in `skip_dirs` or starts with `target`.
pub fn collect_files(dir: &Path, exts: &[&str], skip_dirs: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if skip_dirs.contains(&name) || name.starts_with("target") {
                continue;
            }
            collect_files(&path, exts, skip_dirs, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.contains(&e))
        {
            out.push(path);
        }
    }
}

/// `.rs` files under `dir`, recursively.
pub fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    collect_files(dir, &["rs"], &[], out);
}

/// String literals of a Rust source: `(line number, content)`.
pub fn string_literals(text: &str) -> Vec<(usize, String)> {
    literals(text, Lang::Rust).into_iter().map(|l| (l.line, l.text)).collect()
}

/// String literals of a source in the given language.
pub fn literals(text: &str, lang: Lang) -> Vec<Literal> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;
    // The last significant (non-space, non-comment) char: tells a JS regex
    // literal from a division.
    let mut prev_sig: Option<char> = None;
    let mut prev_word = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // Comments.
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            let mut depth = 1;
            i += 2;
            while i < chars.len() && depth > 0 {
                if chars[i] == '\n' {
                    line += 1;
                }
                if lang == Lang::Rust && chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                    continue;
                }
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    continue;
                }
                i += 1;
            }
            continue;
        }
        let after_ident = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        // Rust raw string: r"…" / r#"…"# / br#"…"#.
        if lang == Lang::Rust
            && !after_ident
            && (c == 'r' || (c == 'b' && chars.get(i + 1) == Some(&'r')))
        {
            let mut j = if c == 'b' { i + 2 } else { i + 1 };
            let hashes_start = j;
            while chars.get(j) == Some(&'#') {
                j += 1;
            }
            if chars.get(j) == Some(&'"') {
                let hashes = j - hashes_start;
                let start_line = line;
                let mut buf = String::new();
                j += 1;
                loop {
                    match chars.get(j) {
                        None => break,
                        Some('"') if count_hashes(&chars, j + 1) >= hashes => {
                            j += 1 + hashes;
                            break;
                        }
                        Some(&ch) => {
                            if ch == '\n' {
                                line += 1;
                            }
                            buf.push(ch);
                            j += 1;
                        }
                    }
                }
                out.push(Literal { line: start_line, end_line: line, text: buf });
                i = j;
                prev_sig = Some('"');
                continue;
            }
        }
        // Ordinary string; in JS also '…' and `…`.
        let quote = c == '"' || (lang == Lang::Js && (c == '\'' || c == '`'));
        if quote {
            let start_line = line;
            let mut buf = String::new();
            i += 1;
            while i < chars.len() && chars[i] != c {
                if chars[i] == '\\' {
                    if chars.get(i + 1) == Some(&'\n') {
                        line += 1;
                        i += 2;
                        if lang == Lang::Rust {
                            while matches!(chars.get(i), Some(' ') | Some('\t')) {
                                i += 1;
                            }
                        }
                        continue;
                    }
                    i += 2;
                    buf.push(' ');
                    continue;
                }
                // An unterminated JS quote ends at the line break: better lose
                // one literal than swallow the rest of the file.
                if chars[i] == '\n' {
                    if lang == Lang::Js && c != '`' {
                        break;
                    }
                    line += 1;
                }
                buf.push(chars[i]);
                i += 1;
            }
            i += 1;
            out.push(Literal { line: start_line, end_line: line, text: buf });
            prev_sig = Some('"');
            continue;
        }
        // JS regex literal: `/` where an expression starts, not a division.
        if lang == Lang::Js && c == '/' && regex_may_start(prev_sig, &prev_word) {
            let mut in_class = false;
            i += 1;
            while i < chars.len() && chars[i] != '\n' {
                match chars[i] {
                    '\\' => i += 1,
                    '[' => in_class = true,
                    ']' => in_class = false,
                    '/' if !in_class => break,
                    _ => {}
                }
                i += 1;
            }
            i += 1;
            prev_sig = Some('"');
            continue;
        }
        // Rust char or lifetime.
        if lang == Lang::Rust && c == '\'' {
            if chars.get(i + 1) == Some(&'\\') {
                i += 2;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if chars.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
            prev_sig = Some('\'');
            continue;
        }
        if c.is_alphanumeric() || c == '_' || c == '$' {
            if !after_ident {
                prev_word.clear();
            }
            prev_word.push(c);
        } else {
            prev_word.clear();
        }
        prev_sig = Some(c);
        i += 1;
    }
    out
}

/// Can a `/` after this char (or keyword) start a regular expression?
fn regex_may_start(prev: Option<char>, prev_word: &str) -> bool {
    match prev {
        None => true,
        Some(c) if c.is_alphanumeric() || c == '_' || c == '$' => {
            matches!(prev_word, "return" | "typeof" | "case" | "in" | "of" | "delete" | "void")
        }
        Some(c) => !matches!(c, ')' | ']' | '}' | '"' | '\''),
    }
}

fn count_hashes(chars: &[char], from: usize) -> usize {
    let mut n = 0;
    while chars.get(from + n) == Some(&'#') {
        n += 1;
    }
    n
}
