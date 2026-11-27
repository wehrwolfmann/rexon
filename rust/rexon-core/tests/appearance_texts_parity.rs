//! Guard of the SINGLE source of captions for the «Оформление» tab.
//!
//! WHY. The tab's captions live in the core (`rexon_core::appearance_page`,
//! `pub fn name() -> String { tr("…") }`). Two truths for one label are
//! not cosmetics:
//!
//!   * the caption is changed in the core, while yesterday's stays on screen — nobody
//!     sees the mismatch until they stumble on it by eye;
//!   * a caption works as a decision KEY. Right here, in this tab, the look
//!     of a local card is determined by searching for the «активна» badge in the
//!     author caption (`local_active_badge`). Rename the text in one of the two
//!     copies — the branch silently stops firing, and the work ends
//!     "successfully" without doing the main thing.
//!
//! WHAT WE GUARD. The checks read the sources of the core and the Rust page and turn
//! red on their own, without a human:
//!
//!   1. every core caption has a Russian translation in `po/ru.po`;
//!   2. every caption is shown by someone: the Rust page (`rexon-gui`) or
//!      the core itself; the ones known not to be wired up are named one by one
//!      ([`NOT_YET_SHOWN`]), and the list must match the truth in both
//!      directions;
//!   3. the Rust page (`appearance_page.rs`, `appearance_actions.rs`,
//!      `AppearancePage.qml`) has no string literal literally equal to a
//!      core caption — neither in English nor in Russian — i.e. there is no second
//!      truth.
//!
//! WHAT IT WAS BEFORE. Until 02.10.2026 the second and third checks looked at
//! Python: the `apage_texts` table of the `rexon-py` bridge and the frozen widget
//! page `golden/frozen_appearance_page.py`. By the owner's decision of
//! 02.10.2026 Python in Rexon is only an archive, neither a golden source nor a bridge, so
//! the guard was moved to the Rust consumer of the captions.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The core: caption declarations.
const CORE_RS: &str = include_str!("../src/appearance_page.rs");
/// The Rust page — the consumer of the captions.
const GUI_PAGE_RS: &str = include_str!("../../rexon-gui/src/appearance_page.rs");
const GUI_ACTIONS_RS: &str = include_str!("../../rexon-gui/src/appearance_actions.rs");
const GUI_PAGE_QML: &str = include_str!("../../rexon-gui/qml/AppearancePage.qml");
/// The Russian catalog: a second truth could also appear in Russian letters.
const RU_PO: &str = include_str!("../../../po/ru.po");

/// How many captions the tab is known to have at least.
///
/// This is not an estimate for the future but insurance for THE PARSING ITSELF: should the search
/// for declarations break, "no duplicates found" would become true on an empty
/// set, and the guard green and useless. If captions decreased for a reason,
/// fix the number together with the change.
const KNOWN_CAPTIONS: usize = 39;

// ── Parsing the core declarations ─────────────────────────────────────

/// Unescape a Rust string literal.
fn unescape_rust(body: &str) -> String {
    let chars: Vec<char> = body.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\\' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        match chars.get(i + 1) {
            // A line break in the source: the break itself and the indent of the next
            // line are not part of the value.
            Some('\n') => {
                i += 2;
                while matches!(chars.get(i), Some(' ') | Some('\t')) {
                    i += 1;
                }
            }
            Some('u') => {
                let end = chars[i..].iter().position(|c| *c == '}').unwrap() + i;
                let hex: String = chars[i + 3..end].iter().collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                i = end + 1;
            }
            Some('n') => {
                out.push('\n');
                i += 2;
            }
            Some('t') => {
                out.push('\t');
                i += 2;
            }
            Some('r') => {
                out.push('\r');
                i += 2;
            }
            Some(c) => {
                out.push(*c);
                i += 2;
            }
            None => break,
        }
    }
    out
}

/// Core captions: `pub fn name() -> String { … tr("…") … }` → (name, msgid).
///
/// The first literal passed to `tr`/`trf` in the body of a function without
/// parameters is taken. Functions without a translation inside (`gallery_counter_qss`)
/// are not captions and do not count.
fn core_captions() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut rest = CORE_RS;
    while let Some(pos) = rest.find("pub fn ") {
        rest = &rest[pos + "pub fn ".len()..];
        let Some(open) = rest.find('(') else { break };
        let name = rest[..open].trim().to_string();
        if !rest[open..].starts_with("() -> String {") {
            continue;
        }
        let body_start = open + "() -> String {".len();
        let Some(body_len) = rest[body_start..].find("\n}") else { break };
        let body = &rest[body_start..body_start + body_len];
        let Some(call) = ["tr(", "trf("]
            .iter()
            .filter_map(|k| body.find(k).map(|p| p + k.len()))
            .min()
        else {
            continue;
        };
        let after = body[call..].trim_start();
        let Some(lit) = after.strip_prefix('"') else { continue };
        let chars: Vec<char> = lit.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '\\' => i += 2,
                '"' => break,
                _ => i += 1,
            }
        }
        let raw: String = chars[..i].iter().collect();
        out.insert(name, unescape_rust(&raw));
    }
    out
}

/// Russian translation of a caption — from the tree's `po/ru.po` (the `.po` parsing here
/// is simple: `msgid`/`msgstr` on one or several lines, no context).
fn russian(msgid: &str) -> Option<String> {
    let mut id: Option<String> = None;
    let mut cur_id = String::new();
    let mut cur_str = String::new();
    let mut ctxt = false;
    let mut field = 0; // 1 — msgid, 2 — msgstr
    let unq = |l: &str| unescape_rust(l.trim().trim_matches('"'));
    for line in RU_PO.lines().chain(std::iter::once("")) {
        if line.starts_with("msgctxt ") {
            ctxt = true;
            field = 0;
        } else if let Some(v) = line.strip_prefix("msgid ") {
            cur_id = unq(v);
            field = 1;
        } else if let Some(v) = line.strip_prefix("msgstr ") {
            cur_str = unq(v);
            field = 2;
        } else if line.starts_with('"') {
            match field {
                1 => cur_id.push_str(&unq(line)),
                2 => cur_str.push_str(&unq(line)),
                _ => {}
            }
        } else if field == 2 {
            if !ctxt && cur_id == msgid && !cur_str.is_empty() {
                id = Some(cur_str.clone());
            }
            ctxt = false;
            field = 0;
        }
    }
    id
}

// ── Who shows the caption ─────────────────────────────────────────────

/// Core captions shown neither by the Rust page nor by the core itself.
/// The list is a wiring debt, not a permission: wired up a caption — remove
/// it from here (the guard will turn red and remind you); added a new caption without
/// showing it — the guard turns red too. The last three (the «Открыть в AUR» button
/// with its tooltip and the «нет превью» placeholder) were wired up on 02.10.2026 — the debt is empty.
const NOT_YET_SHOWN: &[&str] = &[];

/// All `.rs` under a directory (recursively).
fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("нет {}: {e}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path);
        }
    }
}

/// Whether the text has a call `name(` — with a word boundary on the left and not in the
/// declaration `pub fn name(` itself.
fn calls(text: &str, name: &str) -> bool {
    let needle = format!("{name}(");
    let mut from = 0;
    while let Some(pos) = text[from..].find(&needle) {
        let at = from + pos;
        from = at + needle.len();
        let before = text[..at].chars().next_back();
        if before.map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false) {
            continue;
        }
        if text[..at].ends_with("fn ") {
            continue;
        }
        return true;
    }
    false
}

/// A literal `"text"` in the source (in quotes — so a prose mention in
/// a comment «…» does not count as a second truth).
fn quoted(src: &str, text: &str) -> bool {
    src.contains(&format!("\"{text}\""))
}

// ── The checks themselves ─────────────────────────────────────────────

#[test]
fn every_caption_has_a_russian_translation() {
    let captions = core_captions();
    assert!(
        captions.len() >= KNOWN_CAPTIONS,
        "подписей найдено {}, а известно {KNOWN_CAPTIONS} — либо их убрали, \
         либо сломался разбор объявлений (тогда сторож молчит впустую)",
        captions.len()
    );
    let untranslated: Vec<String> = captions
        .iter()
        .filter(|(_, msgid)| russian(msgid).is_none())
        .map(|(name, msgid)| format!("{name}: «{msgid}»"))
        .collect();
    assert!(
        untranslated.is_empty(),
        "у подписей нет русского перевода в po/ru.po:\n  {}",
        untranslated.join("\n  ")
    );
}

#[test]
fn every_caption_is_shown_by_rust() {
    let captions = core_captions();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_sources(&manifest.join("src"), &mut files);
    rust_sources(&manifest.join("../rexon-gui/src"), &mut files);
    assert!(files.len() > 50, "исходников найдено {} — сломан обход", files.len());
    let texts: Vec<String> = files
        .iter()
        .map(|f| fs::read_to_string(f).unwrap_or_else(|e| panic!("{}: {e}", f.display())))
        .collect();

    let unshown: Vec<String> = captions
        .keys()
        .filter(|name| !texts.iter().any(|t| calls(t, name)))
        .cloned()
        .collect();
    let mut known: Vec<String> = NOT_YET_SHOWN.iter().map(|s| s.to_string()).collect();
    known.sort();
    assert_eq!(
        unshown, known,
        "подписи без показа разошлись со списком NOT_YET_SHOWN: подключённую \
         уберите из списка, новую — подключите к странице"
    );
}

#[test]
fn rust_page_keeps_no_second_copy_of_captions() {
    let captions = core_captions();
    let sources = [
        ("rexon-gui/src/appearance_page.rs", GUI_PAGE_RS),
        ("rexon-gui/src/appearance_actions.rs", GUI_ACTIONS_RS),
        ("rexon-gui/qml/AppearancePage.qml", GUI_PAGE_QML),
    ];
    let mut dupes = Vec::new();
    for (name, msgid) in &captions {
        let mut forms = vec![msgid.clone()];
        forms.extend(russian(msgid));
        for form in forms {
            for (file, src) in sources {
                if quoted(src, &form) {
                    dupes.push(format!("{file}: «{form}» — копия подписи {name}"));
                }
            }
        }
    }
    assert!(
        dupes.is_empty(),
        "на Rust-странице снова заведена своя копия подписей ядра — берите их \
         из `rexon_core::appearance_page`:\n  {}",
        dupes.join("\n  ")
    );
}

/// Self-check of the call and literal search: without it the checks above could
/// turn green simply because the search stopped finding anything.
#[test]
fn search_helpers_understand_the_tricky_cases() {
    let sample = "\
pub fn page_title() -> String { tr(\"Appearance\") }
let a = ap::source_label();
let b = my_source_label();
// упоминание «Appearance» в комментарии
let c = \"Appearance\";
";
    assert!(calls(sample, "source_label"));
    assert!(!calls(sample, "page_title"), "объявление — не вызов");
    assert!(!calls("let b = my_source_label();", "source_label"), "чужое имя с хвостом");
    assert!(quoted(sample, "Appearance"));
    assert!(!quoted("// «Appearance»", "Appearance"));
}
