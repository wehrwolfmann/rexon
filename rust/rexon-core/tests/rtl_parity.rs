//! Guard of right-to-left languages (Arabic, Persian, Hebrew) in the layout.
//!
//! How it works: mirroring is turned on by the ROOT of each of our windows
//! (`LayoutMirroring.enabled` by `Qt.application.layoutDirection` and
//! `childrenInherit`), text is aligned to the start of the line by its own type
//! `RexonText` (an explicit `AlignLeft`, which mirroring flips), and
//! the vendored Wallpaper Engine tab stays left-to-right with a stub on
//! the host. The check reads the sources and goes red if a new window is added without
//! mirroring, the stub is removed or `RexonText` loses its alignment.
//!
//! Our own controls (`Rexon*`) place their indicators by hand — the combo box arrow,
//! the checkbox square, the slider fill — and mirroring does not flip manual `x`
//! values. So at every such place the check expects `control.mirrored` to be taken into
//! account, and in all our QML — not a single bare `Text` (except
//! named exceptions and physical zones), and every `TextArea`/`TextEdit` field
//! sets its alignment explicitly.
//!
//! Directional icons (❮ ❯ ▸ ◂) and the ← → keys are not flipped by mirroring either:
//! the «back» button moves to the right, while the icon and the key stay
//! the same. They are decided by one type, `RexonArrows`, by the mirroring in effect at
//! the place; the check makes sure the icons are written only in it, and that pagers and
//! chevrons take the icon, the key step and the hint arrow from there.
//!
//! Physical zones — where "left" and "right" mean the real side of the screen
//! or the window (window title buttons, the screen layout, the screen corner grid,
//! the Steam profile snapshot) — are NOT mirrored in a right-to-left language: their
//! root has a local `LayoutMirroring.enabled: false`. The check keeps the list of zones
//! by name: a zone was lost or its stub removed — red; mirroring
//! was turned off somewhere else without an entry — red too. The ← → keys bypassing
//! `RexonArrows` are allowed only inside such a zone.
//!
//! Manual `x:` coordinates are not flipped by mirroring either. Offsets
//! from an edge are written with anchors; where an anchor does not fit — `RexonArrows.placeX`;
//! other `x:` values outside physical zones are listed by name in `MANUAL_X` with a reason.
//!
//! `%1` substitutions that QML assembles itself go through `RexonBidi`
//! (an FSI/PDI isolate, like `i18n::fill` in the core), and every catalog string with
//! a ← → arrow carries a note to the translator — the arrow is written the way it
//! points in their language.
//!
//! WHAT THE CHECK DOES NOT MEASURE: how the window looks with `-reverse`. That is a snapshot on
//! the bench.
//!
//! Inside physical zones `RexonText` is not allowed (its `AlignLeft` would not
//! flip there) — zone labels use a bare `Text` aligned to the center.

use std::fs;
use std::path::{Path, PathBuf};

const MIRROR_ENABLED: &str =
    "LayoutMirroring.enabled:Qt.application.layoutDirection===Qt.RightToLeft";
const MIRROR_INHERIT: &str = "LayoutMirroring.childrenInherit:true";

/// Windows that do not need mirroring — by name and with a reason.
/// Both are internal test benches: the buyer does not see them, there is no text for
/// a person in them.
const WINDOW_EXCEPTIONS: &[&str] = &["LayerShellProbe.qml", "LiveModelsProbe.qml"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(rel: &str) -> String {
    let path = root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("нет исходника {path:?}: {e}"))
}

/// Code without comment lines and without spaces: the check does not depend on formatting.
fn squash_code(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .flat_map(|l| l.chars())
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// Window blocks in the layout: from the line `ApplicationWindow {` / `Window {` to
/// the matching brace. Returns the line number and the squashed code of the block.
fn window_blocks(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if !(t == "ApplicationWindow {" || t == "Window {") {
            continue;
        }
        let mut depth = 0i32;
        let mut block = String::new();
        for l in &lines[i..] {
            if l.trim_start().starts_with("//") {
                continue;
            }
            for c in l.chars() {
                match c {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            block.push_str(l);
            block.push('\n');
            if depth == 0 {
                break;
            }
        }
        out.push((i + 1, squash_code(&block)));
    }
    out
}

/// Properties of the window itself, without nested objects: only the top level of
/// the block, so that mirroring of a nested element is not credited to the window.
fn top_level(block: &str) -> String {
    let mut depth = 0i32;
    let mut out = String::new();
    for c in block.chars() {
        match c {
            '{' => {
                depth += 1;
                continue;
            }
            '}' => {
                depth -= 1;
                continue;
            }
            _ => {}
        }
        if depth == 1 {
            out.push(c);
        }
    }
    out
}

fn assert_mirrored(place: &str, block: &str) {
    let own = top_level(block);
    assert!(
        own.contains(MIRROR_ENABLED),
        "{place}: окно без `LayoutMirroring.enabled: Qt.application.layoutDirection === \
         Qt.RightToLeft` — при языке справа налево его раскладка не перевернётся"
    );
    assert!(
        own.contains(MIRROR_INHERIT),
        "{place}: окно без `LayoutMirroring.childrenInherit: true` — зеркалирование \
         не дойдёт до детей"
    );
}

#[test]
fn every_window_of_our_qml_mirrors_from_its_root() {
    let dir = root().join("rust/rexon-gui/qml");
    let mut seen = 0usize;
    let mut names: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("нет каталога {dir:?}: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".qml"))
        .collect();
    names.sort();
    for name in &names {
        let text = source(&format!("rust/rexon-gui/qml/{name}"));
        let blocks = window_blocks(&text);
        if WINDOW_EXCEPTIONS.contains(&name.as_str()) {
            assert!(
                !blocks.is_empty(),
                "{name} в исключениях, но окна в нём больше нет — убрать из списка"
            );
            continue;
        }
        for (line, block) in blocks {
            assert_mirrored(&format!("{name}:{line}"), &block);
            seen += 1;
        }
    }
    // The main window and the full-screen screenshot view of «Desktops».
    assert!(seen >= 2, "окон найдено {seen} — разбор разметки сломался");
}

/// The crash window — the layout is a string in Rust, not a module file.
#[test]
fn crash_dialog_window_mirrors_from_its_root() {
    let text = source("rust/rexon-gui/src/crash_report.rs");
    let blocks = window_blocks(&text);
    assert_eq!(blocks.len(), 1, "в разметке окна о крахе ждали ровно одно окно");
    assert_mirrored("crash_report.rs", &blocks[0].1);
}

/// The vendored Wallpaper Engine tab stays left-to-right until upstream handles it.
#[test]
fn vendor_wallpaper_tab_stays_left_to_right() {
    let code = squash_code(&source("rust/rexon-gui/qml/WallpaperEnginePage.qml"));
    let at = code
        .find("Loader{id:vendorHost")
        .expect("хост вендорной вкладки (`Loader { id: vendorHost`) не найден");
    let tail = &code[at..];
    let end = tail.find("}").expect("блок хоста не закрыт");
    let host = &tail[..end];
    assert!(
        host.contains("LayoutMirroring.enabled:false"),
        "с хоста вендорной вкладки сняли `LayoutMirroring.enabled: false` — разметка \
         waywallen перевернётся, а на зеркалирование она не рассчитана"
    );
    assert!(
        host.contains(MIRROR_INHERIT),
        "заглушке на хосте вендорной вкладки нужен `childrenInherit: true`, иначе \
         она не дойдёт до страниц"
    );
}

/// `RexonText` exists, is built into the module and aligns explicitly to the left edge.
#[test]
fn rexon_text_exists_and_aligns_left_explicitly() {
    let code = squash_code(&source("rust/rexon-gui/qml/RexonText.qml"));
    assert!(code.contains("Text{"), "RexonText.qml больше не на основе `Text`");
    assert!(
        code.contains("horizontalAlignment:Text.AlignLeft"),
        "RexonText потерял явное `AlignLeft` — при языке справа налево строки без \
         арабских букв снова встанут влево"
    );
    let build = source("rust/rexon-gui/build.rs");
    assert!(
        build.contains("\"qml/RexonText.qml\""),
        "RexonText.qml не перечислен в `rust/rexon-gui/build.rs` — тип не попадёт в модуль"
    );
}

/// The manual indicator place in our own controls — with mirroring taken into account.
/// Each line is squashed code (without spaces) that must be present in the file.
const MIRRORED_INDICATORS: &[(&str, &[(&str, &str)])] = &[
    (
        "RexonSlider.qml",
        &[
            ("x:control.mirrored?parent.width-width:0", "заливка не встаёт от правого края"),
            ("width:control.position*parent.width", "ширина заливки не по `position`"),
            ("control.mirrored?1-ratio:ratio", "деления не отсчитываются от правого края"),
        ],
    ),
    (
        "RexonCheckBox.qml",
        &[
            (
                "x:control.mirrored?control.width-width-control.rightPadding:control.leftPadding",
                "квадрат флажка не переезжает вправо",
            ),
            ("leftPadding:control.mirrored?0:", "место под квадрат слева не снимается"),
            (
                "rightPadding:control.mirrored?control.indicator.width+control.spacing:0",
                "место под квадрат справа не даётся",
            ),
        ],
    ),
    (
        "RexonComboBox.qml",
        &[
            ("x:control.mirrored?10:control.width-width-10", "стрелка списка не переезжает влево"),
            ("leftPadding:control.mirrored?28:10", "отступ под стрелку слева не даётся"),
            ("rightPadding:control.mirrored?10:28", "отступ под стрелку справа не снимается"),
            ("x:control.mirrored?control.width-width:0", "выпадашка не растёт влево"),
        ],
    ),
    (
        // The field's placeholder repeats its alignment only if that is set
        // explicitly; without it both the text and the placeholder follow their own line.
        "RexonTextField.qml",
        &[(
            "horizontalAlignment:TextInput.AlignLeft",
            "вводимый текст и подсказка не встают от правого края",
        )],
    ),
];

#[test]
fn rexon_controls_place_indicators_with_mirroring() {
    for (name, needs) in MIRRORED_INDICATORS {
        let code = squash_code(&source(&format!("rust/rexon-gui/qml/{name}")));
        for (need, what) in *needs {
            assert!(
                code.contains(need),
                "{name}: {what} при языке справа налево — нет `{need}`"
            );
        }
    }
}

/// Where bare `Text` and `Label` are allowed in the whole file — by name and with
/// a reason. In the rest of our QML everything textual is `RexonText`; in files with
/// a physical zone (`PHYSICAL_ZONES`) a bare `Text` is allowed only inside
/// it: there `AlignLeft` would not flip, and the zone labels are centered.
const BARE_TEXT_EXCEPTIONS: &[(&str, &str)] = &[
    // The type itself: it is exactly a `Text` aligned to the start of the line.
    ("RexonText.qml", "основа `RexonText`"),
    // Internal test benches: the buyer does not see them (as in
    // `WINDOW_EXCEPTIONS`).
    ("LayerShellProbe.qml", "проба слоя обоев"),
    ("LiveModelsProbe.qml", "проба живых моделей"),
];

/// Line numbers where an object of type `Text` or `Label` is declared (and not
/// `RexonText`, `TextMetrics`, `Text.AlignLeft`). `Label` from Qt Quick
/// Controls aligns the same way as `Text` — by the line itself.
fn bare_text_lines(text: &str) -> Vec<usize> {
    object_lines(text, &["Text", "Label"])
}

/// Line numbers where an object of one of the `types` is declared (the whole name,
/// not part of another one and not an enum value after a dot).
fn object_lines(text: &str, types: &[&str]) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let bytes = line.as_bytes();
        let found = types.iter().any(|ty| {
            let mut from = 0;
            while let Some(off) = line[from..].find(ty) {
                let at = from + off;
                from = at + ty.len();
                let before_ok = at == 0 || {
                    let c = bytes[at - 1];
                    !(c.is_ascii_alphanumeric() || c == b'_' || c == b'.')
                };
                if before_ok && line[from..].trim_start().starts_with('{') {
                    return true;
                }
            }
            false
        });
        if found {
            out.push(i + 1);
        }
    }
    out
}

#[test]
fn our_qml_uses_rexon_text_only() {
    let names = qml_names();
    for (name, why) in BARE_TEXT_EXCEPTIONS {
        assert!(
            names.iter().any(|n| n == name),
            "{name} ({why}) записан в `BARE_TEXT_EXCEPTIONS`, но файла нет — убрать запись"
        );
    }
    for name in &names {
        if BARE_TEXT_EXCEPTIONS.iter().any(|(n, _)| n == name) {
            continue;
        }
        let text = source(&format!("rust/rexon-gui/qml/{name}"));
        let zones: Vec<(usize, usize)> = PHYSICAL_ZONES
            .iter()
            .filter(|(n, _, _)| n == name)
            .map(|(_, id, what)| {
                let (a, b, _) = object_by_id(&text, id)
                    .unwrap_or_else(|| panic!("{name}: физическая зона «{what}» (`id: {id}`) не найдена"));
                (a + 1, b + 1)
            })
            .collect();
        let bad: Vec<usize> = bare_text_lines(&text)
            .into_iter()
            .filter(|l| !zones.iter().any(|(a, b)| (*a..=*b).contains(l)))
            .collect();
        assert!(
            bad.is_empty(),
            "{name}: голый `Text` или `Label` в строках {bad:?} — нужен `RexonText`, иначе при \
             языке справа налево строка без арабских букв встанет не с той стороны (голый `Text` \
             можно только внутри физической зоны из `PHYSICAL_ZONES`)"
        );
    }
    // The parsing itself: a bare type is caught, ours and the neighbouring ones are not.
    assert_eq!(bare_text_lines("Text {\n  contentItem: Text{\n  Label {\n"), vec![1, 2, 3]);
    assert!(bare_text_lines("RexonText {\nTextMetrics {\nx: Text.AlignLeft\nMyLabel {\n").is_empty());
}

/// Input fields and multi-line text: without an explicit value their alignment
/// is also taken from the line itself, and `RexonText` does not fit them.
const TEXT_EDIT_TYPES: &[&str] = &["TextArea", "TextEdit", "TextInput", "TextField"];

/// Fields without alignment — by name (file, `id`, reason).
const TEXT_EDIT_EXCEPTIONS: &[(&str, &str, &str)] = &[
    // An invisible field: text is put into the clipboard through it, it is not
    // on the screen.
    ("WindowMenuDialogs.qml", "logClipboard", "буфер обмена"),
];

/// An object block from line `line` (1-based) to the matching brace, squashed.
fn block_from(text: &str, line: usize) -> String {
    let mut depth = 0i32;
    let mut block = String::new();
    for l in text.lines().skip(line - 1) {
        if l.trim_start().starts_with("//") {
            continue;
        }
        for c in l.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        block.push_str(l);
        block.push('\n');
        if depth == 0 {
            break;
        }
    }
    squash_code(&block)
}

/// Every `TextArea`/`TextEdit` field (and `TextInput`/`TextField`) in our QML
/// sets `horizontalAlignment` itself: ordinary text — `AlignLeft`
/// (mirroring will flip it), code, logs and paths — physically on the left
/// (`LayoutMirroring.enabled ? AlignRight : AlignLeft`).
#[test]
fn text_edits_set_alignment_explicitly() {
    for (name, id, why) in TEXT_EDIT_EXCEPTIONS {
        let text = source(&format!("rust/rexon-gui/qml/{name}"));
        assert!(
            object_by_id(&text, id).is_some(),
            "{name}: поле `id: {id}` ({why}) из `TEXT_EDIT_EXCEPTIONS` не найдено — убрать запись"
        );
    }
    for name in qml_names() {
        // The buyer does not see internal test benches.
        if WINDOW_EXCEPTIONS.contains(&name.as_str()) {
            continue;
        }
        let text = source(&format!("rust/rexon-gui/qml/{name}"));
        for line in object_lines(&text, TEXT_EDIT_TYPES) {
            let own = top_level(&block_from(&text, line));
            if TEXT_EDIT_EXCEPTIONS
                .iter()
                .any(|(n, id, _)| *n == name && own.contains(&format!("id:{id}")))
            {
                continue;
            }
            assert!(
                own.contains("horizontalAlignment:"),
                "{name}:{line}: поле без явного `horizontalAlignment` — при языке справа налево \
                 оно встанет по самой строке; обычный текст — `TextEdit.AlignLeft`, код и \
                 журналы — `LayoutMirroring.enabled ? TextEdit.AlignRight : TextEdit.AlignLeft`"
            );
        }
    }
    // The parsing itself: a field is caught, a nested backing is not credited to the field.
    let probe = "TextArea {\n  background: Rectangle { horizontalAlignment: 1 }\n}\n";
    assert_eq!(object_lines(probe, TEXT_EDIT_TYPES), vec![1]);
    assert!(!top_level(&block_from(probe, 1)).contains("horizontalAlignment:"));
}


/// Icons with a side: mirroring does not flip them, and Qt does not
/// flip a label consisting of a single icon without letters (measured with `-reverse`).
/// «▶» is not included here: in our QML it is the play icon («▶ Preview»,
/// the sound queue), and that one is not flipped right-to-left. The triangle of
/// a collapsed row «▶» is taken from `RexonArrows.collapsedRow`, and its pair «◀»
/// literally lives only there.
const DIRECTIONAL_GLYPHS: &[char] = &['❮', '❯', '▸', '◂', '◀'];

/// The only file where directional icons are written literally.
const ARROWS_FILE: &str = "RexonArrows.qml";

/// Places that must take icons and keys from `RexonArrows` — by name:
/// the file and code pieces without which the icon, key or hint in a right-to-left
/// language point the wrong way.
const ARROW_USERS: &[(&str, &[(&str, &str)])] = &[
    (
        "RexonImageViewer.qml",
        &[
            ("propertyRexonArrowsarrows:RexonArrows{mirrored:viewer.mirrored}", "свой RexonArrows по зеркалированию просмотра"),
            ("text:viewer.arrows.previous", "значок кнопки «назад»"),
            ("text:viewer.arrows.next", "значок кнопки «вперёд»"),
            ("caseQt.Key_Left:viewer.step(viewer.arrows.leftStep)", "клавиша ←"),
            ("caseQt.Key_Right:viewer.step(viewer.arrows.rightStep)", "клавиша →"),
            ("viewer.arrows.keyHint(qsTranslate(\"rexon\",\"Previous(←)\"))", "стрелка в подсказке «назад»"),
            ("viewer.arrows.keyHint(qsTranslate(\"rexon\",\"Next(→)\"))", "стрелка в подсказке «вперёд»"),
        ],
    ),
    (
        "DesktopsPage.qml",
        &[
            ("RexonArrows{mirrored:carousel.LayoutMirroring.enabled}", "свой RexonArrows у листалки снимков"),
            ("text:carousel.arrows.previous", "значок «назад» листалки"),
            ("text:carousel.arrows.next", "значок «вперёд» листалки"),
            ("RexonArrows{mirrored:zoomBody.LayoutMirroring.enabled}", "свой RexonArrows у просмотра во весь экран"),
            ("text:zoomBody.arrows.previous", "значок «назад» просмотра"),
            ("text:zoomBody.arrows.next", "значок «вперёд» просмотра"),
            ("caseQt.Key_Left:caseQt.Key_A:carousel.step(zoomBody.arrows.leftStep)", "клавиши ←/A"),
            ("caseQt.Key_Right:caseQt.Key_D:carousel.step(zoomBody.arrows.rightStep)", "клавиши →/D"),
            ("zoomBody.arrows.keyHint(root.carouselTexts.zoomPrevTooltip)", "стрелка в подсказке «назад»"),
            ("zoomBody.arrows.keyHint(root.carouselTexts.zoomNextTooltip)", "стрелка в подсказке «вперёд»"),
        ],
    ),
    (
        "DayNightSection.qml",
        &[
            ("RexonArrows{mirrored:headerRow.LayoutMirroring.enabled}", "свой RexonArrows у заголовка раздела"),
            ("text:section.expanded?\"▾\":arrows.collapsed", "шеврон свёрнутого раздела"),
        ],
    ),
    (
        "ComparePage.qml",
        &[
            ("RexonArrows{mirrored:cardHeader.mirrored}", "свой RexonArrows у шапки карточки"),
            ("(card.expanded?\"▼\":arrows.collapsedRow)", "треугольник свёрнутой карточки"),
            // A desktop name in Latin letters would make the header a left-to-right line, and
            // the triangle would stand at its left end, in the middle of the card.
            ("(cardHeader.mirrored?\"\\u200f\":\"\")", "знак RLM в начале шапки"),
        ],
    ),
    (
        "AppearancePage.qml",
        &[
            ("RexonArrows{mirrored:root.LayoutMirroring.enabled}", "свой RexonArrows у страницы"),
            // The icons «◀️ ▶️» come from the core; mirroring
            // swaps the buttons but not the icons, so they swap places.
            ("text:root.arrows.mirrored?page.overlayNextText:page.overlayPrevText", "значок «назад» галереи"),
            ("text:root.arrows.mirrored?page.overlayPrevText:page.overlayNextText", "значок «вперёд» галереи"),
            ("text:root.arrows.keyHint(page.detailBackText)", "стрелка «Назад к каталогу»"),
            ("text:root.arrows.keyHint(page.pagePrevText)", "стрелка «Назад» листалки страниц"),
            ("text:root.arrows.keyHint(page.pageNextText)", "стрелка «Вперёд» листалки страниц"),
        ],
    ),
    (
        "SteamPage.qml",
        &[
            // «‹ ›» are mirrored in Unicode: in a right-to-left line Qt
            // flips them itself. An RLM mark makes an untranslated string
            // a right-to-left line; swapping the icon by hand would mean
            // flipping it twice in a translated one.
            ("text:(root.LayoutMirroring.enabled?\"\\u200f\":\"\")+page.bgPrevText", "знак RLM у «‹ Prev» листалки фонов"),
            ("text:(root.LayoutMirroring.enabled?\"\\u200f\":\"\")+page.bgNextText", "знак RLM у «Next ›» листалки фонов"),
        ],
    ),
];

/// The triangle of the «Comparison» header is placed by the page: the model returns the header without
/// it (the core knows nothing about the place's mirroring).
#[test]
fn compare_card_header_arrow_is_placed_by_the_page() {
    let model = source("rust/rexon-gui/src/compare_page.rs");
    assert!(
        model.contains("qc::card_header_tail(") && !model.contains("qc::card_header("),
        "compare_page.rs: роль `headerText` снова со стрелкой ядра — справа налево \
         свёрнутая карточка смотрела бы вправо"
    );
}

fn qml_names() -> Vec<String> {
    let dir = root().join("rust/rexon-gui/qml");
    let mut names: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("нет каталога {dir:?}: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".qml"))
        .collect();
    names.sort();
    names
}

#[test]
fn directional_glyphs_live_only_in_rexon_arrows() {
    let mut checked = 0usize;
    for name in qml_names() {
        if name == ARROWS_FILE {
            continue;
        }
        let text = source(&format!("rust/rexon-gui/qml/{name}"));
        for (i, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if line.trim_start().starts_with("//") {
                continue;
            }
            if let Some(g) = DIRECTIONAL_GLYPHS.iter().find(|g| code.contains(**g)) {
                panic!(
                    "{name}:{}: значок «{g}» написан буквально — при языке справа налево \
                     он не перевернётся; брать из `RexonArrows`",
                    i + 1
                );
            }
        }
        checked += 1;
    }
    assert!(checked > 20, "файлов QML проверено {checked} — обход каталога сломался");
}

#[test]
fn rexon_arrows_follow_mirroring() {
    let code = squash_code(&source(&format!("rust/rexon-gui/qml/{ARROWS_FILE}")));
    for (need, what) in [
        ("propertyboolmirrored:false", "входа `mirrored`"),
        ("previous:arrows.mirrored?\"❯\":\"❮\"", "значка «назад»"),
        ("next:arrows.mirrored?\"❮\":\"❯\"", "значка «вперёд»"),
        ("collapsed:arrows.mirrored?\"◂\":\"▸\"", "шеврона свёрнутого"),
        ("collapsedRow:arrows.mirrored?\"◀\":\"▶\"", "треугольника свёрнутой строки"),
        ("leftStep:arrows.mirrored?1:-1", "шага клавиши ←"),
        ("rightStep:-arrows.leftStep", "шага клавиши →"),
        ("functionkeyHint(", "подстановки стрелки в подсказку"),
        ("returnarrows.mirrored?outer-start-width:start", "отсчёта `placeX` от начала строки"),
    ] {
        assert!(code.contains(need), "{ARROWS_FILE}: нет {what} (`{need}`)");
    }
    let build = source("rust/rexon-gui/build.rs");
    assert!(
        build.contains("\"qml/RexonArrows.qml\""),
        "RexonArrows.qml не перечислен в `rust/rexon-gui/build.rs` — тип не попадёт в модуль"
    );
}

#[test]
fn pagers_and_chevrons_take_arrows_from_rexon_arrows() {
    for (name, needs) in ARROW_USERS {
        let code = squash_code(&source(&format!("rust/rexon-gui/qml/{name}")));
        for (need, what) in *needs {
            assert!(
                code.contains(need),
                "{name}: {what} не из `RexonArrows` — справа налево смотрит не туда (нет `{need}`)"
            );
        }
    }
}

/// Where the ← → keys mean a PHYSICAL side and do not go through `RexonArrows` —
/// by name and with a reason.
const ARROW_KEY_EXCEPTIONS: &[(&str, &str)] = &[
    // It sends keys rather than parsing them: an internal test bench.
    ("LiveModelsProbe.qml", "стенд"),
];

const ARROW_KEYS: &[&str] = &["Qt.Key_Left", "Qt.Key_Right", "onLeftPressed", "onRightPressed"];

/// The ← → keys in our QML are parsed only where the step is taken from
/// `RexonArrows`, or inside a physical zone (there ← is the left edge of the screen or
/// the window in any language), except for named exceptions.
#[test]
fn arrow_keys_are_handled_only_by_known_pagers() {
    let users: Vec<&str> = ARROW_USERS.iter().map(|(n, _)| *n).collect();
    let mut in_zones = 0usize;
    for name in qml_names() {
        if ARROW_KEY_EXCEPTIONS.iter().any(|(n, _)| *n == name) {
            continue;
        }
        if users.contains(&name.as_str()) {
            continue;
        }
        let text = source(&format!("rust/rexon-gui/qml/{name}"));
        let zones: Vec<(usize, usize)> = PHYSICAL_ZONES
            .iter()
            .filter(|(n, _, _)| *n == name)
            .filter_map(|(_, id, _)| object_by_id(&text, id).map(|(a, b, _)| (a, b)))
            .collect();
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") || !ARROW_KEYS.iter().any(|k| line.contains(k)) {
                continue;
            }
            assert!(
                zones.iter().any(|(a, b)| (*a..=*b).contains(&i)),
                "{name}:{}: клавиши ← → разбираются мимо `RexonArrows` и вне физической \
                 зоны — добавить файл в `ARROW_USERS` и брать шаг из `leftStep`/`rightStep` \
                 (или, если сторона тут настоящая, завести зону в `PHYSICAL_ZONES`)",
                i + 1
            );
            in_zones += 1;
        }
    }
    // Window title buttons: ← and → on the layout button.
    assert!(in_zones >= 2, "клавиш внутри физических зон найдено {in_zones} — разбор сломался");
}

/// Physical zones: the file, the `id` of the zone root and why the side there is real.
/// The root carries `LayoutMirroring.enabled: false` and `childrenInherit: true`.
const PHYSICAL_ZONES: &[(&str, &str, &str)] = &[
    // The window title layout: button groups at the left and right edges of the system's
    // window; ← and «Move left» move a button to the left edge.
    ("WindowButtonsSection.qml", "titleBar", "кнопки заголовка окна"),
    // The «Move left/Move right» and «Add on the left/right» pairs: the left button of the pair is the left one.
    ("WindowButtonsSection.qml", "moveRow", "кнопки «Левее/Правее»"),
    ("WindowButtonsSection.qml", "addRow", "кнопки «Добавить слева/справа»"),
    // The desktop sample under the screen effect shaders.
    ("ScreenEffectsSection.qml", "sample", "картинка экрана"),
    // The grid of screen corners and edges where a Conky widget goes.
    ("ConkySettings.qml", "alignmentGrid", "углы экрана"),
    // The screen mini-map: monitors and Conky widget windows in their places.
    ("ConkyScreenMap.qml", "canvas", "схема экрана"),
    // The Steam profile snapshot: storefront parts by picture coordinates.
    ("SteamShowcaseSection.qml", "preview", "профиль Steam"),
    // The «as in the profile / computed» comparison: the storefront parts are pieces of one
    // picture side by side, the large one on the left, the side one on the right, as in the Steam profile.
    ("SteamShowcaseSection.qml", "cmpParts", "части витрины рядом"),
];

/// Where mirroring is turned off not by a physical zone — by name: the vendored
/// Wallpaper Engine tab (its own check above).
const OTHER_MIRROR_OFF: &[(&str, &str)] = &[("WallpaperEnginePage.qml", "vendorHost")];

const MIRROR_OFF: &str = "LayoutMirroring.enabled:false";

/// A layout object that has `id: <id>` at its own level:
/// the numbers of the first and last line (0-based) and the squashed code of the block.
fn object_by_id(text: &str, id: &str) -> Option<(usize, usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let want = format!("id: {id}");
    let mut stack: Vec<usize> = Vec::new();
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        if line.trim() == want {
            start = stack.last().copied();
            break;
        }
        for c in line.chars() {
            match c {
                '{' => stack.push(i),
                '}' => {
                    stack.pop();
                }
                _ => {}
            }
        }
    }
    let start = start?;
    let mut depth = 0i32;
    let mut block = String::new();
    for (i, l) in lines.iter().enumerate().skip(start) {
        if l.trim_start().starts_with("//") {
            continue;
        }
        for c in l.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        block.push_str(l);
        block.push('\n');
        if depth == 0 {
            return Some((start, i, squash_code(&block)));
        }
    }
    None
}

#[test]
fn physical_zones_stay_left_to_right() {
    for (name, id, what) in PHYSICAL_ZONES {
        let text = source(&format!("rust/rexon-gui/qml/{name}"));
        let (line, _, block) = object_by_id(&text, id)
            .unwrap_or_else(|| panic!("{name}: физическая зона «{what}» (`id: {id}`) не найдена"));
        let own = top_level(&block);
        assert!(
            own.contains(MIRROR_OFF) && own.contains(MIRROR_INHERIT),
            "{name}:{}: с физической зоны «{what}» сняли `LayoutMirroring.enabled: false` \
             или `childrenInherit: true` — при языке справа налево её стороны перевернутся",
            line + 1
        );
        // Inside a zone an explicit "left edge" does not flip and stays left for
        // an Arabic line too: zone labels are centered.
        for bad in ["RexonText{", "Text.AlignLeft", "Text.AlignRight"] {
            assert!(
                !block.contains(bad),
                "{name}: в физической зоне «{what}» стоит `{bad}` — зеркалирование тут \
                 выключено, и подпись на языке справа налево встанет не с той стороны; \
                 выравнивать по центру"
            );
        }
    }
    // The parsing itself: the `id` of a nested object is not credited to the outer one.
    let probe = "Item {\n    id: outer\n    Row {\n        id: inner\n        x: 1\n    }\n}\n";
    assert_eq!(object_by_id(probe, "inner").map(|(a, b, _)| (a, b)), Some((2, 5)));
    assert_eq!(object_by_id(probe, "outer").map(|(a, b, _)| (a, b)), Some((0, 6)));
}

/// Mirroring may be turned off only in the recorded places: a new zone comes
/// with an entry in `PHYSICAL_ZONES` and a reason.
#[test]
fn mirroring_is_switched_off_only_in_known_places() {
    for name in qml_names() {
        let code = squash_code(&source(&format!("rust/rexon-gui/qml/{name}")));
        let found = code.matches(MIRROR_OFF).count();
        let known = PHYSICAL_ZONES.iter().filter(|(n, _, _)| *n == name).count()
            + OTHER_MIRROR_OFF.iter().filter(|(n, _)| *n == name).count();
        assert_eq!(
            found, known,
            "{name}: `LayoutMirroring.enabled: false` стоит {found} раз, а записано мест \
             {known} — новую физическую зону записать в `PHYSICAL_ZONES` с причиной"
        );
    }
}

/// A manual `x:` coordinate outside physical zones — by name, as the full expression
/// (without spaces) and with a reason. Mirroring does not flip it: an offset
/// from an edge is written with an anchor (`anchors.left` + `leftMargin`, a symmetric one —
/// `anchors.left`/`right` + `margins`), and where an anchor does not fit — via
/// `RexonArrows.placeX` or the control's own `mirrored`.
const MANUAL_X: &[(&str, &str, &str)] = &[
    // Our own controls: the indicator by `control.mirrored` / `visualPosition`.
    ("RexonSlider.qml", "x:control.leftPadding", "полоса во всю доступную ширину — симметрична"),
    ("RexonSlider.qml", "x:control.mirrored?parent.width-width:0", "заливка"),
    (
        "RexonSlider.qml",
        "x:control.leftPadding+control.handle.width/2+visualRatio*(control.availableWidth-control.handle.width)-width/2",
        "штрих шкалы по `visualRatio`",
    ),
    (
        "RexonSlider.qml",
        "x:control.leftPadding+control.visualPosition*(control.availableWidth-width)",
        "ручка по `visualPosition`",
    ),
    (
        "RexonCheckBox.qml",
        "x:control.mirrored?control.width-width-control.rightPadding:control.leftPadding",
        "квадрат флажка",
    ),
    ("RexonComboBox.qml", "x:control.mirrored?10:control.width-width-10", "стрелка списка"),
    ("RexonComboBox.qml", "x:control.mirrored?control.width-width:0", "выпадающий список"),
    (
        "SettingsPage.qml",
        "x:fmButton.mirrored?fmButton.width-width-fmButton.rightPadding:fmButton.leftPadding",
        "кружок радиокнопки файлового менеджера",
    ),
    // Window-wide layers and window coordinates from `mapToItem(null, …)` —
    // real places on the screen in any language.
    ("OnboardingOverlay.qml", "x:0", "слой тура во всё окно и его затемнение"),
    ("OnboardingOverlay.qml", "x:tour.hole.x", "вырез вокруг цели в координатах окна"),
    ("RexonImageViewer.qml", "x:0", "слой просмотра во всё окно"),
    (
        "RexonImageViewer.qml",
        "x:Math.round((stage.width-canvas.width)/2+viewer.panX)",
        "картинка по центру и сдвиг рукой",
    ),
    // The core's computation relative to the start of the line, converted by `placeX`.
    (
        "AppearancePage.qml",
        "x:root.arrows.placeX(suggestBox.box.x,suggestBox.box.width,root.width)",
        "выпадашка подсказок поиска",
    ),
];

/// The `x:` binding from line `start`: the line itself and its continuations up to the next
/// property, brace or empty line — squashed, without spaces.
fn x_binding(lines: &[&str], start: usize) -> String {
    let mut out = String::new();
    for (k, l) in lines.iter().enumerate().skip(start) {
        let t = l.trim();
        if k > start {
            let is_prop = t.split(':').next().is_some_and(|h| {
                !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
            }) && t.contains(':');
            if t.is_empty() || t.starts_with("//") || t.starts_with('}') || is_prop {
                break;
            }
        }
        let code = t.split("//").next().unwrap_or("");
        // `x: 230; y: 34; …` on one line — only the `x` itself.
        let code = if k == start { code.split(';').next().unwrap_or("") } else { code };
        out.extend(code.chars().filter(|c| !c.is_whitespace()));
    }
    out
}

#[test]
fn manual_x_only_in_known_places() {
    let mut used = vec![0usize; MANUAL_X.len()];
    let mut in_zones = 0usize;
    for name in qml_names() {
        if WINDOW_EXCEPTIONS.contains(&name.as_str()) {
            continue;
        }
        let text = source(&format!("rust/rexon-gui/qml/{name}"));
        let lines: Vec<&str> = text.lines().collect();
        let zones: Vec<(usize, usize)> = PHYSICAL_ZONES
            .iter()
            .filter(|(n, _, _)| *n == name)
            .filter_map(|(_, id, _)| object_by_id(&text, id).map(|(a, b, _)| (a, b)))
            .collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.trim_start().starts_with("x:") {
                continue;
            }
            if zones.iter().any(|(a, b)| (*a..=*b).contains(&i)) {
                in_zones += 1;
                continue;
            }
            let binding = x_binding(&lines, i);
            let known = MANUAL_X.iter().position(|(n, b, _)| *n == name && *b == binding);
            match known {
                Some(k) => used[k] += 1,
                None => panic!(
                    "{name}:{}: ручная координата `{binding}` — при языке справа налево \
                     не перевернётся. Отступ от края — якорем (`anchors.left` + \
                     `leftMargin`; симметричный — `anchors.left`/`right` + `margins`), \
                     иначе `RexonArrows.placeX`; настоящая сторона экрана — \
                     физическая зона или запись в `MANUAL_X` с причиной",
                    i + 1
                ),
            }
        }
    }
    for ((name, binding, what), n) in MANUAL_X.iter().zip(&used) {
        assert!(*n > 0, "{name}: записанной ручной координаты «{what}» (`{binding}`) нет — убрать запись");
    }
    // The screen layout and the Steam profile snapshot: picture coordinates.
    assert!(in_zones >= 10, "ручных `x` в физических зонах найдено {in_zones} — разбор сломался");
    // The parsing itself: a continuation of the expression is joined, a neighbouring property is not.
    let probe = ["    x: a", "       ? b : c", "    y: 0"];
    assert_eq!(x_binding(&probe, 0), "x:a?b:c");
    assert_eq!(x_binding(&["x: 1; y: 2"], 0), "x:1");
}

/// `SplitView` does not take mirroring into account: the panels go left to right in
/// the order of the children. Each of our `SplitView`s must reorder the panels itself by
/// `LayoutMirroring.enabled` (`moveItem`).
#[test]
fn split_views_reorder_panels_when_mirrored() {
    let mut found = 0usize;
    for name in qml_names() {
        let code = squash_code(&source(&format!("rust/rexon-gui/qml/{name}")));
        let splits = code.matches("SplitView{").count();
        if splits == 0 {
            continue;
        }
        found += splits;
        assert!(
            code.contains("panelsMirrored:split.LayoutMirroring.enabled")
                && code.contains("onPanelsMirroredChanged:split.orderPanels()")
                && code.contains("split.moveItem("),
            "{name}: `SplitView` без перестановки панелей по зеркалированию — при языке \
             справа налево панели останутся в порядке слева направо"
        );
    }
    assert!(found >= 1, "ни одного `SplitView` не найдено — разбор сломался");
}

/// Where the content width is reduced by the scroll bar for the sake of SIZE, while
/// the place is set by an anchor — by name: the file, the bar's `id` and why that is fine.
const SCROLLBAR_WIDTH_EXCEPTIONS: &[(&str, &str, &str)] = &[
    // The core needs the viewport width to compute the card; the card itself stands
    // anchored from the start of the line, and right-to-left the bar on the left does not touch it.
    ("DesktopsPage.qml", "cardScroll", "ширина вьюпорта для расчёта карточки"),
];

/// A content width "minus the scroll bar" (`width - bar.width`,
/// `availableWidth - 14`, `width - (contentHeight > height ? 14 : 0)`)
/// keeps room for the bar on the right. In a right-to-left language the bar moves
/// to the left and covers the start of the lines, while an empty strip stays on the right.
/// The right way is a width with `left`/`right` anchors and room for the bar via
/// `anchors.rightMargin`: mirroring moves it over to the bar.
///
/// WHAT THE CHECK DOES NOT CATCH: subtracting a bare number from the width without a link to
/// a bar (`parent.width - 14` next to a `ScrollBar {}` without an `id`) — it cannot be
/// told apart from a symmetric margin; such places are found by a snapshot with
/// `-reverse`.
#[test]
fn content_width_does_not_subtract_the_scrollbar() {
    fn offences(text: &str) -> Vec<String> {
        let code = squash_code(text);
        // Lines are not joined for bar names: otherwise the `id` would stick to
        // the next property.
        let by_lines: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .map(|l| l.chars().filter(|c| !c.is_whitespace()).collect::<String>() + "\n")
            .collect();
        let by_lines = by_lines.replace("{\n", "{");
        let mut out = Vec::new();
        // Named bars: `ScrollBar { id: bar`.
        let mut ids = Vec::new();
        for (at, _) in by_lines.match_indices("ScrollBar{id:") {
            let tail = &by_lines[at + "ScrollBar{id:".len()..];
            let id: String =
                tail.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            ids.push(id);
        }
        for id in &ids {
            for pat in [format!("-{id}.width"), format!("-({id}.visible?{id}.width")] {
                if code.contains(&pat) {
                    out.push(id.clone());
                }
            }
        }
        // `availableWidth - 14`: room for the `ScrollView` bar as a number.
        for (at, m) in code.match_indices("availableWidth-") {
            if code[at + m.len()..].starts_with(|c: char| c.is_ascii_digit()) {
                out.push("availableWidth".into());
            }
        }
        // `width - (flick.contentHeight > flick.height ? 14 : 0)`.
        for (at, m) in code.match_indices("-(") {
            let inner = code[at + m.len()..].split(')').next().unwrap_or("");
            if inner.contains("contentHeight>") {
                out.push("contentHeight".into());
            }
        }
        out.dedup();
        out
    }
    let mut used = vec![0usize; SCROLLBAR_WIDTH_EXCEPTIONS.len()];
    for name in qml_names() {
        for what in offences(&source(&format!("rust/rexon-gui/qml/{name}"))) {
            match SCROLLBAR_WIDTH_EXCEPTIONS.iter().position(|(n, id, _)| *n == name && *id == what) {
                Some(k) => used[k] += 1,
                None => panic!(
                    "{name}: ширина содержимого минус полоса прокрутки (`{what}`) — при языке \
                     справа налево полоса уйдёт влево и наедет на текст. Ширину — якорями \
                     `anchors.left`/`right`, место под полосу — `anchors.rightMargin`"
                ),
            }
        }
    }
    for ((name, id, what), n) in SCROLLBAR_WIDTH_EXCEPTIONS.iter().zip(&used) {
        assert!(*n > 0, "{name}: записанного исключения «{what}» (`{id}`) больше нет — убрать запись");
    }
    // The parsing itself: all three forms are caught, an anchor margin is not.
    let probe = "ScrollBar {\n    id: bar\n    policy: ScrollBar.AlwaysOn\n}\n\
                 Text { width: f.width\n        - (bar.visible ? bar.width : 0) }\n\
                 Label { width: v.availableWidth - 14 }\n\
                 Item { width: f.width - (f.contentHeight > f.height ? 14 : 0) }\n";
    assert_eq!(offences(probe), vec!["bar", "availableWidth", "contentHeight"]);
    let fine = "ScrollBar { id: bar }\nText { anchors.rightMargin: bar.visible ? bar.width : 0 }\n\
                Item { x: (c.availableWidth - h.width) }\n";
    assert!(offences(fine).is_empty());
}

/// `%1` substitutions that QML assembles itself — only via `RexonBidi`.
///
/// The core substitutes via `i18n::fill`, and in a right-to-left language every
/// substitution is placed in an FSI/PDI isolate. `String.arg` in QML does not add
/// isolates: a file name or a number at the end of an Arabic phrase would shuffle the order
/// of the words around it. So `.arg(` in our QML is written only in
/// `RexonBidi.qml`, and `RexonBidi` itself must add an isolate by the application's
/// direction.
#[test]
fn qml_substitutions_go_through_rexon_bidi() {
    let bidi = squash_code(&source("rust/rexon-gui/qml/RexonBidi.qml"));
    for need in [
        "readonlypropertyboolrtl:Qt.application.layoutDirection===Qt.RightToLeft",
        "\"\\u2068\"+s+\"\\u2069\"",
    ] {
        assert!(bidi.contains(need), "RexonBidi.qml потерял «{need}»");
    }
    for name in qml_names() {
        if name == "RexonBidi.qml" {
            continue;
        }
        let text = source(&format!("rust/rexon-gui/qml/{name}"));
        assert!(
            !text.contains("bidi.arg(") || squash_code(&text).contains("RexonBidi{id:bidi}"),
            "{name}: `bidi.arg(` без `RexonBidi {{ id: bidi }}` в файле"
        );
        for (no, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            assert!(
                !line.replace("bidi.arg(", "").contains(".arg("),
                "{name}:{}: подстановка `.arg()` мимо RexonBidi — при языке справа налево \
                 она останется без изолята. Писать `bidi.arg(qsTranslate(…), значение)` \
                 (`RexonBidi {{ id: bidi }}` в корне файла)",
                no + 1
            );
        }
    }
}

/// Every catalog string with a ← → arrow carries a note to the translator.
///
/// The rule is one for the whole product (`RexonArrows.keyHint`): the translator writes
/// the arrow the way it points in their language — in right-to-left languages "then",
/// "forward" and the menu path «Settings → Display» are written with ←. Without the note
/// the translator will copy the arrow as in the English source, and the menu path in
/// the Arabic interface will point backwards. The `TRANSLATORS:` note is put above
/// the `tr`/`n_` call; xgettext attaches it to the FIRST call on a code line,
/// so two translatable strings on one code line are split onto different lines.
#[test]
fn arrows_in_catalog_carry_a_translator_note() {
    let pot = source("po/rexon.pot");
    let mut missing = Vec::new();
    let mut total = 0;
    for entry in pot.split("\n\n") {
        let mut msgid = String::new();
        let mut in_id = false;
        for line in entry.lines() {
            if let Some(rest) =
                line.strip_prefix("msgid ").or_else(|| line.strip_prefix("msgid_plural "))
            {
                in_id = true;
                msgid.push_str(rest.trim_matches('"'));
            } else if in_id && line.starts_with('"') {
                msgid.push_str(line.trim_matches('"'));
            } else {
                in_id = false;
            }
        }
        if !msgid.contains(['←', '→']) {
            continue;
        }
        total += 1;
        // The common note «arrows ← → point the way…» or a custom one, as for the
        // `.rxn` help: «<pre> stay left-to-right…, so keep their arrows ← as they are».
        if !entry.lines().any(|l| l.starts_with("#.") && l.contains("arrows")) {
            missing.push(msgid);
        }
    }
    assert!(total > 40, "строк со стрелками в каталоге подозрительно мало ({total}) — сломан разбор?");
    assert!(
        missing.is_empty(),
        "строки со стрелкой без пометки переводчику («// TRANSLATORS: arrows ← → point the way \
         they should in your language: …» над вызовом перевода):\n{}",
        missing.join("\n")
    );
}
