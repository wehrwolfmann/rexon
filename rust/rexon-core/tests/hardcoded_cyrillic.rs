//! Guard against text hardcoded in the code instead of the translation catalog.
//!
//! WHY. Everything a person sees goes through gettext: an English msgid in the
//! code, translations in `po/*.po` (owner's decision of 24.11.2026). Before
//! that, Russian strings were written straight into the code — and a German
//! buyer saw a Russian wallpaper sound status, because a literal is not
//! translated by anything. Logs, developer errors and tests are written in
//! English and are not translated at all. So Cyrillic inside a string literal
//! of the code is either a string that has not moved to the catalog yet, or
//! recognition data.
//!
//! WHAT IS READ: string literals (not comments, not names) of
//!   * `rust/**/*.rs` except `tests/`, `vendor/` and build directories;
//!   * `rust/rexon-gui/qml/**/*.qml`;
//!   * `data/**/*.js` (GNOME/Cinnamon extensions, the screensaver page).
//!
//! EXCEPTIONS:
//!   * recognition data — Russian words we LOOK FOR in someone else's files or
//!     output (a `.desktop` name, a localized command output) — carry a marker
//!     comment with a reason: `// i18n-exempt: <reason>` on a line of the
//!     literal or alone on the line above it; a table of such words goes
//!     between `// i18n-exempt-begin: <reason>` and `// i18n-exempt-end`;
//!   * strings not moved yet — `tests/data/hardcoded_cyrillic_allowlist.txt`:
//!     a path and the number of such literals in the file (not line numbers,
//!     so an edit above does not break the list).
//!
//! ⭐ The list only SHRINKS. More literals than listed (or a file that is not
//! listed) is red: write the new text through `tr`/`qsTranslate`. Fewer is red
//! too, with the number to write down — otherwise the freed place would be
//! silently taken by the next hardcoded string. `REXON_UPDATE_ALLOWLIST=1`
//! writes the smaller numbers by itself and never raises one.
//! `REXON_CYRILLIC_DUMP=<file>` writes every found literal (file, line,
//! exempt, text) as TSV — the input for the per-module plan.

mod source_literals;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use source_literals::{collect_files, literals, Lang, Literal};

const ALLOWLIST: &str = "tests/data/hardcoded_cyrillic_allowlist.txt";
const MARKER: &str = "i18n-exempt:";
const BEGIN: &str = "i18n-exempt-begin:";
const END: &str = "i18n-exempt-end";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn has_cyrillic(text: &str) -> bool {
    text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// Source files under the guard, as paths relative to the repository root.
fn guarded_files(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_files(&root.join("rust"), &["rs"], &["tests", "vendor"], &mut files);
    collect_files(&root.join("rust/rexon-gui/qml"), &["qml"], &[], &mut files);
    collect_files(&root.join("data"), &["js"], &[], &mut files);
    let mut rel: Vec<String> = files
        .iter()
        .map(|p| p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/"))
        .collect();
    rel.sort();
    rel.dedup();
    rel
}

/// A marker problem found while reading a file.
#[derive(Debug)]
struct BadMarker {
    line: usize,
    what: &'static str,
}

/// Lines (1-based) covered by exemption markers, and broken markers.
fn exempt_lines(text: &str) -> (BTreeSet<usize>, Vec<BadMarker>) {
    let mut lines = BTreeSet::new();
    let mut bad = Vec::new();
    let mut open: Option<usize> = None;
    let all: Vec<&str> = text.lines().collect();
    for (k, raw) in all.iter().enumerate() {
        let n = k + 1;
        let Some(at) = raw.find("//") else { continue };
        let comment = &raw[at..];
        if let Some(pos) = comment.find(BEGIN) {
            if comment[pos + BEGIN.len()..].trim().is_empty() {
                bad.push(BadMarker { line: n, what: "exemption block without a reason" });
            }
            if open.is_some() {
                bad.push(BadMarker { line: n, what: "exemption block opened twice" });
            }
            open = Some(n);
        } else if comment.contains(END) {
            match open.take() {
                Some(from) => lines.extend(from..=n),
                None => bad.push(BadMarker { line: n, what: "exemption block end without a start" }),
            }
        } else if let Some(pos) = comment.find(MARKER) {
            if comment[pos + MARKER.len()..].trim().is_empty() {
                bad.push(BadMarker { line: n, what: "exemption marker without a reason" });
            }
            lines.insert(n);
            // A marker alone on its line covers the next line.
            if raw[..at].trim().is_empty() {
                lines.insert(n + 1);
            }
        }
    }
    if let Some(from) = open {
        bad.push(BadMarker { line: from, what: "exemption block is never closed" });
    }
    (lines, bad)
}

/// Cyrillic literals of one file: `(literal, exempt)`.
fn cyrillic_literals(rel: &str, text: &str) -> (Vec<(Literal, bool)>, Vec<BadMarker>) {
    let lang = Lang::of(Path::new(rel)).expect("guarded file of an unknown language");
    let (exempt, bad) = exempt_lines(text);
    let found = literals(text, lang)
        .into_iter()
        .filter(|l| has_cyrillic(&l.text))
        .map(|l| {
            let free = (l.line..=l.end_line).any(|n| exempt.contains(&n));
            (l, free)
        })
        .collect();
    (found, bad)
}

fn read_allowlist(path: &Path) -> BTreeMap<String, usize> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut out = BTreeMap::new();
    for (k, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(file), Some(count), None) = (parts.next(), parts.next(), parts.next()) else {
            panic!("{ALLOWLIST}:{}: expected `<path> <count>`, got {line:?}", k + 1);
        };
        let count: usize = count
            .parse()
            .unwrap_or_else(|_| panic!("{ALLOWLIST}:{}: count is not a number: {line:?}", k + 1));
        assert!(
            out.insert(file.to_string(), count).is_none(),
            "{ALLOWLIST}:{}: {file} is listed twice",
            k + 1
        );
    }
    out
}

/// Rewrite the list with the given numbers, keeping its header comments.
fn write_allowlist(path: &Path, counts: &BTreeMap<String, usize>) {
    let old = fs::read_to_string(path).unwrap_or_default();
    let mut text: String = old
        .lines()
        .take_while(|l| l.trim().is_empty() || l.starts_with('#'))
        .map(|l| format!("{l}\n"))
        .collect();
    for (file, n) in counts.iter().filter(|(_, n)| **n > 0) {
        text.push_str(&format!("{file} {n}\n"));
    }
    fs::write(path, text).expect("cannot write the allowlist");
}

fn one_line(text: &str) -> String {
    let flat: String = text.chars().map(|c| if c == '\n' || c == '\t' { ' ' } else { c }).collect();
    let short: String = flat.chars().take(80).collect();
    if flat.chars().count() > 80 { format!("{short}…") } else { short }
}

#[test]
fn hardcoded_cyrillic_only_shrinks() {
    let root = repo_root();
    let files = guarded_files(&root);
    assert!(files.len() > 300, "sources not found under {}", root.display());

    let mut actual: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown: BTreeMap<String, Vec<Literal>> = BTreeMap::new();
    let mut bad_markers = Vec::new();
    let mut dump = String::from("file\tline\texempt\ttext\n");
    for rel in &files {
        let text = fs::read_to_string(root.join(rel)).expect("source is not readable");
        let (found, bad) = cyrillic_literals(rel, &text);
        bad_markers.extend(bad.into_iter().map(|b| format!("  {rel}:{}: {}", b.line, b.what)));
        for (lit, exempt) in found {
            dump.push_str(&format!("{rel}\t{}\t{}\t{}\n", lit.line, exempt as u8, one_line(&lit.text)));
            if !exempt {
                *actual.entry(rel.clone()).or_default() += 1;
                shown.entry(rel.clone()).or_default().push(lit);
            }
        }
    }
    if let Ok(out) = std::env::var("REXON_CYRILLIC_DUMP") {
        fs::write(&out, &dump).expect("cannot write the dump");
    }
    assert!(bad_markers.is_empty(), "broken i18n-exempt markers:\n{}", bad_markers.join("\n"));

    let list_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(ALLOWLIST);
    let listed = read_allowlist(&list_path);

    let mut grown = Vec::new();
    let mut shrunk = Vec::new();
    for (file, &n) in &actual {
        let allowed = listed.get(file).copied().unwrap_or(0);
        if n > allowed {
            let lits = shown[file]
                .iter()
                .map(|l| format!("      {}: «{}»", l.line, one_line(&l.text)))
                .collect::<Vec<_>>()
                .join("\n");
            grown.push(format!("  {file}: {n} literal(s), allowed {allowed}\n{lits}"));
        }
    }
    for (file, &allowed) in &listed {
        let n = actual.get(file).copied().unwrap_or(0);
        if n < allowed {
            shrunk.push(format!("  {file}: {allowed} → {n}"));
        }
    }

    if !shrunk.is_empty() && std::env::var("REXON_UPDATE_ALLOWLIST").is_ok() {
        let lowered: BTreeMap<String, usize> = listed
            .iter()
            .map(|(f, &allowed)| (f.clone(), allowed.min(actual.get(f).copied().unwrap_or(0))))
            .collect();
        write_allowlist(&list_path, &lowered);
        shrunk.clear();
    }

    assert!(
        grown.is_empty(),
        "new hardcoded Cyrillic text in string literals. Text for a person goes through \
         gettext (`rexon_core::i18n::tr`/`trf`, `qsTranslate(\"rexon\", …)` in QML) with an \
         English msgid and translations in po/; logs, errors and tests are written in English. \
         Russian words we only look for in someone else's data get `// i18n-exempt: <reason>`.\n{}",
        grown.join("\n")
    );
    assert!(
        shrunk.is_empty(),
        "fewer hardcoded Cyrillic literals than {ALLOWLIST} allows — good; lower the numbers \
         (by hand or with REXON_UPDATE_ALLOWLIST=1), the list only shrinks:\n{}",
        shrunk.join("\n")
    );
}

/// The guard checks itself: planted literals must be seen, comments and
/// marked recognition data must not.
#[test]
fn guard_sees_literals_not_comments() {
    let rust = concat!(
        "// комментарий \"не строка\"\n",
        "let a = \"Привет\"; /* и тут «нет» */\n",
        "let b = r#\"сырая \"строка\"\"#;\n",
        "fn f<'a>(x: &'a str) -> char { 'ж' }\n",
        "let c = \"english\";\n",
        "let d = [\"Рабочий стол\"]; // i18n-exempt: .desktop Name we look for\n",
        "// i18n-exempt: localized pacman output\n",
        "let e = \"Пакет\";\n",
        "// i18n-exempt-begin: names of menu entries in a Russian Cinnamon\n",
        "const M: &[&str] = &[\"Меню\",\n \"Панель\"];\n",
        "// i18n-exempt-end\n",
        "let g = \"конец\";\n",
    );
    let (found, bad) = cyrillic_literals("x.rs", rust);
    assert!(bad.is_empty(), "{bad:?}");
    let free: Vec<_> = found.iter().filter(|(_, e)| !*e).map(|(l, _)| l.text.as_str()).collect();
    assert_eq!(free, ["Привет", "сырая \"строка\"", "конец"]);
    assert_eq!(found.iter().filter(|(_, e)| *e).count(), 4);

    let js = concat!(
        "// 'нет'\n",
        "let a = 'один', b = `два ${x}`;\n",
        "let r = /[\"']+/g; let q = x / 2 / \"три\".length;\n",
        "/* \"нет\" */ let s = \"четыре\";\n",
    );
    let (found, _) = cyrillic_literals("x.js", js);
    let texts: Vec<_> = found.iter().map(|(l, _)| l.text.as_str()).collect();
    assert_eq!(texts, ["один", "два ${x}", "три", "четыре"]);

    let (_, bad) = cyrillic_literals("x.rs", "let a = \"Б\"; // i18n-exempt:\n");
    assert_eq!(bad.len(), 1, "a marker without a reason must be red");
}
