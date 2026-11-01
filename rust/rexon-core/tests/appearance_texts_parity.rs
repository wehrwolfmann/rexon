//! Сторож ЕДИНСТВЕННОГО источника подписей вкладки «Оформление».
//!
//! ЗАЧЕМ. Подписи вкладки живут в ядре (`rexon_core::appearance_page`,
//! `pub fn имя() -> String { tr("…") }`). Две правды на одну надпись — это
//! не косметика:
//!
//!   * подпись меняют в ядре, а на экране остаётся вчерашняя — расхождение
//!     никто не видит, пока не наткнётся глазами;
//!   * подпись работает КЛЮЧОМ решения. Здесь же, в этой вкладке, вид
//!     локальной карточки определяется поиском бейджа «активна» в подписи
//!     автора (`local_active_badge`). Переименуй текст в одной из двух
//!     копий — ветка молча перестаёт срабатывать, и работа «успешно»
//!     заканчивается, не сделав главного.
//!
//! ЧТО СТЕРЕЖЁМ. Проверки читают исходники ядра и Rust-страницы и краснеют
//! сами, без человека:
//!
//!   1. у каждой подписи ядра есть русский перевод в `po/ru.po`;
//!   2. каждую подпись кто-то показывает: Rust-страница (`rexon-gui`) или
//!      само ядро; заведомо не подключённые названы поимённо
//!      ([`NOT_YET_SHOWN`]), и список обязан совпадать с правдой в обе
//!      стороны;
//!   3. на Rust-странице (`appearance_page.rs`, `appearance_actions.rs`,
//!      `AppearancePage.qml`) нет строкового литерала, дословно равного
//!      подписи ядра — ни по-английски, ни по-русски, — то есть второй
//!      правды нет.
//!
//! ЧТО БЫЛО РАНЬШЕ. До 02.10.2026 вторая и третья проверки смотрели на
//! Python: таблицу `apage_texts` моста `rexon-py` и замороженную виджетную
//! страницу `golden/frozen_appearance_page.py`. По решению владельца
//! 02.10.2026 Python в Rexon — только архив, не эталон и не мост, поэтому
//! сторож перенесён на Rust-потребителя подписей.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Ядро: объявления подписей.
const CORE_RS: &str = include_str!("../src/appearance_page.rs");
/// Rust-страница — потребитель подписей.
const GUI_PAGE_RS: &str = include_str!("../../rexon-gui/src/appearance_page.rs");
const GUI_ACTIONS_RS: &str = include_str!("../../rexon-gui/src/appearance_actions.rs");
const GUI_PAGE_QML: &str = include_str!("../../rexon-gui/qml/AppearancePage.qml");
/// Русский каталог: вторая правда могла бы завестись и в русских буквах.
const RU_PO: &str = include_str!("../../../po/ru.po");

/// Сколько подписей у вкладки заведомо есть.
///
/// Это не смета на будущее, а страховка САМОГО РАЗБОРА: сломайся поиск
/// объявлений — и «дубликатов не найдено» стало бы правдой на пустом
/// множестве, а сторож — зелёным и бесполезным. Подписей убыло по делу —
/// поправьте число вместе с правкой.
const KNOWN_CAPTIONS: usize = 39;

// ── Разбор объявлений ядра ────────────────────────────────────────────

/// Развернуть экранирование строкового литерала Rust.
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
            // Перенос строки в исходнике: сам перенос и отступ следующей
            // строки в значение не входят.
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

/// Подписи ядра: `pub fn имя() -> String { … tr("…") … }` → (имя, msgid).
///
/// Берётся первый литерал, отданный `tr`/`trf`, в теле функции без
/// параметров. Функции без перевода внутри (`gallery_counter_qss`) —
/// не подписи и в счёт не идут.
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

/// Перевод подписи на русский — из `po/ru.po` дерева (разбор `.po` здесь
/// простой: `msgid`/`msgstr` в одну или несколько строк, без контекста).
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

// ── Кто показывает подпись ────────────────────────────────────────────

/// Подписи ядра, которые не показывает ни Rust-страница, ни само ядро.
/// Список — долг подключения, а не разрешение: подключили подпись — уберите
/// её отсюда (сторож покраснеет и напомнит); завели новую подпись без
/// показа — сторож покраснеет тоже. Последние три (кнопка «Открыть в AUR»
/// с подсказкой и заглушка «нет превью») подключены 02.10.2026 — долг пуст.
const NOT_YET_SHOWN: &[&str] = &[];

/// Все `.rs` под каталогом (рекурсивно).
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

/// Есть ли в тексте вызов `name(` — с границей слова слева и не в самом
/// объявлении `pub fn name(`.
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

/// Литерал `"text"` в исходнике (в кавычках — так прозаическое упоминание в
/// комментарии «…» за вторую правду не сойдёт).
fn quoted(src: &str, text: &str) -> bool {
    src.contains(&format!("\"{text}\""))
}

// ── Сами сверки ───────────────────────────────────────────────────────

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

/// Самопроверка поиска вызовов и литералов: без неё сверки выше могли бы
/// стать зелёными просто оттого, что поиск перестал что-либо находить.
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
