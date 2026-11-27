//! Сторож языка справа налево (арабский, персидский, иврит) в разметке.
//!
//! Как устроено: зеркалирование включает КОРЕНЬ каждого нашего окна
//! (`LayoutMirroring.enabled` по `Qt.application.layoutDirection` и
//! `childrenInherit`), текст выравнивается по началу строки своим типом
//! `RexonText` (явное `AlignLeft`, которое зеркалирование переворачивает), а
//! вендорная вкладка Wallpaper Engine остаётся слева направо заглушкой на
//! хосте. Сверка читает исходники и краснеет, если новое окно завели без
//! зеркалирования, сняли заглушку или `RexonText` потерял выравнивание.
//!
//! Свои контролы (`Rexon*`) ставят индикаторы руками — стрелку списка,
//! квадрат флажка, заливку ползунка, — а ручные `x` зеркалирование не
//! переворачивает. Поэтому у каждого такого места сверка ждёт учёт
//! `control.mirrored`, а во всём нашем QML — ни одного голого `Text` (кроме
//! поимённых исключений и физических зон), и каждое поле `TextArea`/`TextEdit`
//! задаёт выравнивание явно.
//!
//! Направленные значки (❮ ❯ ▸ ◂) и клавиши ← → зеркалирование тоже не
//! переворачивает: кнопка «назад» уезжает вправо, а значок и клавиша остаются
//! прежними. Их решает один тип `RexonArrows` по действующему зеркалированию
//! места; сверка следит, что значки пишутся только в нём, а листалки и
//! шевроны берут значок, шаг клавиши и стрелку подсказки оттуда.
//!
//! Физические зоны — где «лево» и «право» значат настоящую сторону экрана
//! или окна (кнопки заголовка окна, макет экрана, сетка углов экрана,
//! снимок профиля Steam), — при языке справа налево НЕ зеркалятся: на их
//! корне местное `LayoutMirroring.enabled: false`. Сверка держит список зон
//! поимённо: зону потеряли или сняли заглушку — красное; выключили
//! зеркалирование где-то ещё без записи — тоже красное. Клавиши ← → мимо
//! `RexonArrows` разрешены только внутри такой зоны.
//!
//! Ручные координаты `x:` зеркалирование тоже не переворачивает. Отступы
//! от края пишутся якорями; где якорь не годится — `RexonArrows.placeX`;
//! прочие `x:` вне физических зон — поимённо в `MANUAL_X` с причиной.
//!
//! Подстановки `%1`, которые собирает сам QML, идут через `RexonBidi`
//! (изолят FSI/PDI, как `i18n::fill` в ядре), а каждая строка каталога со
//! стрелкой ← → несёт пометку переводчику — стрелку пишут так, как она
//! смотрит на его языке.
//!
//! ЧЕГО СВЕРКА НЕ МЕРИТ: как окно выглядит при `-reverse`. Это — снимок на
//! стенде.
//!
//! Внутри физических зон `RexonText` нельзя (его `AlignLeft` там не
//! перевернётся) — подписи зон голым `Text` выравниваются по центру.

use std::fs;
use std::path::{Path, PathBuf};

const MIRROR_ENABLED: &str =
    "LayoutMirroring.enabled:Qt.application.layoutDirection===Qt.RightToLeft";
const MIRROR_INHERIT: &str = "LayoutMirroring.childrenInherit:true";

/// Окна, которым зеркалирование не нужно, — поимённо и с причиной.
/// Оба — служебные стенды проверок: покупатель их не видит, текста для
/// человека в них нет.
const WINDOW_EXCEPTIONS: &[&str] = &["LayerShellProbe.qml", "LiveModelsProbe.qml"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(rel: &str) -> String {
    let path = root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("нет исходника {path:?}: {e}"))
}

/// Код без строк-комментариев и без пробелов: сверка не зависит от записи.
fn squash_code(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .flat_map(|l| l.chars())
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// Блоки окон в разметке: от строки `ApplicationWindow {` / `Window {` до
/// парной скобки. Возвращает номер строки и сжатый код блока.
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

/// Свойства самого окна, без вложенных объектов: только верхний уровень
/// блока, чтобы зеркалирование вложенного элемента не засчиталось окну.
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
    // Главное окно и полноэкранный просмотр снимков «Рабочих столов».
    assert!(seen >= 2, "окон найдено {seen} — разбор разметки сломался");
}

/// Окно о крахе — разметка строкой в Rust, а не файлом модуля.
#[test]
fn crash_dialog_window_mirrors_from_its_root() {
    let text = source("rust/rexon-gui/src/crash_report.rs");
    let blocks = window_blocks(&text);
    assert_eq!(blocks.len(), 1, "в разметке окна о крахе ждали ровно одно окно");
    assert_mirrored("crash_report.rs", &blocks[0].1);
}

/// Вендорная вкладка Wallpaper Engine остаётся слева направо до апстрима.
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

/// `RexonText` есть, собирается в модуль и выравнивает явно по левому краю.
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

/// Ручное место индикатора в своих контролах — с учётом зеркалирования.
/// Каждая строка — сжатый код (без пробелов), который обязан стоять в файле.
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
        // Подсказка поля повторяет его выравнивание, только если оно задано
        // явно; без него и текст, и подсказка встают по своей строке.
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

/// Где голые `Text` и `Label` разрешены во всём файле — поимённо и с
/// причиной. В остальном нашем QML всё текстовое — `RexonText`; в файлах с
/// физической зоной (`PHYSICAL_ZONES`) голый `Text` разрешён только внутри
/// неё: там `AlignLeft` не перевернётся, и подписи зон стоят по центру.
const BARE_TEXT_EXCEPTIONS: &[(&str, &str)] = &[
    // Сам тип: он и есть `Text` с выравниванием по началу строки.
    ("RexonText.qml", "основа `RexonText`"),
    // Служебные стенды проверок: покупатель их не видит (как в
    // `WINDOW_EXCEPTIONS`).
    ("LayerShellProbe.qml", "проба слоя обоев"),
    ("LiveModelsProbe.qml", "проба живых моделей"),
];

/// Номера строк, где объявлен объект типа `Text` или `Label` (а не
/// `RexonText`, `TextMetrics`, `Text.AlignLeft`). `Label` из Qt Quick
/// Controls выравнивается так же, как `Text`, — по самой строке.
fn bare_text_lines(text: &str) -> Vec<usize> {
    object_lines(text, &["Text", "Label"])
}

/// Номера строк, где объявлен объект одного из типов `types` (имя целиком,
/// не часть другого и не перечисление после точки).
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
    // Сам разбор: голый тип ловится, свой и соседние — нет.
    assert_eq!(bare_text_lines("Text {\n  contentItem: Text{\n  Label {\n"), vec![1, 2, 3]);
    assert!(bare_text_lines("RexonText {\nTextMetrics {\nx: Text.AlignLeft\nMyLabel {\n").is_empty());
}

/// Поля ввода и многострочный текст: их выравнивание без явного значения
/// тоже берётся из самой строки, а `RexonText` для них не годится.
const TEXT_EDIT_TYPES: &[&str] = &["TextArea", "TextEdit", "TextInput", "TextField"];

/// Поля без выравнивания — поимённо (файл, `id`, причина).
const TEXT_EDIT_EXCEPTIONS: &[(&str, &str, &str)] = &[
    // Невидимое поле: через него текст кладётся в буфер обмена, на экране
    // его нет.
    ("WindowMenuDialogs.qml", "logClipboard", "буфер обмена"),
];

/// Блок объекта от строки `line` (с единицы) до парной скобки, сжатый.
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

/// Каждое поле `TextArea`/`TextEdit` (и `TextInput`/`TextField`) в нашем QML
/// задаёт `horizontalAlignment` само: обычный текст — `AlignLeft`
/// (зеркалирование перевернёт), код, журналы и пути — физически слева
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
        // Служебные стенды проверок покупатель не видит.
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
    // Сам разбор: поле ловится, вложенная подложка не засчитывается полю.
    let probe = "TextArea {\n  background: Rectangle { horizontalAlignment: 1 }\n}\n";
    assert_eq!(object_lines(probe, TEXT_EDIT_TYPES), vec![1]);
    assert!(!top_level(&block_from(probe, 1)).contains("horizontalAlignment:"));
}


/// Значки со стороной: зеркалирование их не переворачивает, а Qt не
/// переворачивает надпись из одного значка без букв (замер при `-reverse`).
/// «▶» сюда не входит: в нашем QML это значок воспроизведения («▶ Предпросмотр»,
/// очередь звуков), а он справа налево не переворачивается. Треугольник
/// свёрнутой строки «▶» берётся из `RexonArrows.collapsedRow`, и его пара «◀»
/// буквально живёт только там.
const DIRECTIONAL_GLYPHS: &[char] = &['❮', '❯', '▸', '◂', '◀'];

/// Единственный файл, где направленные значки пишутся буквально.
const ARROWS_FILE: &str = "RexonArrows.qml";

/// Места, которые обязаны брать значки и клавиши из `RexonArrows`, — поимённо:
/// файл и куски кода, без которых значок, клавиша или подсказка в языке справа
/// налево смотрят не туда.
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
            // Имя стола латиницей сделало бы шапку строкой слева направо, и
            // треугольник встал бы у её левого конца, посреди карточки.
            ("(cardHeader.mirrored?\"\\u200f\":\"\")", "знак RLM в начале шапки"),
        ],
    ),
    (
        "AppearancePage.qml",
        &[
            ("RexonArrows{mirrored:root.LayoutMirroring.enabled}", "свой RexonArrows у страницы"),
            // Значки «◀️ ▶️» отдаёт ядро; кнопки зеркалирование
            // переставляет, а значки — нет, поэтому меняются местами.
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
            // «‹ ›» в Unicode зеркальные: в строке справа налево Qt
            // переворачивает их сам. Не переведённую строку знак RLM делает
            // строкой справа налево; поменять значок руками — значит
            // перевернуть его дважды в переведённой.
            ("text:(root.LayoutMirroring.enabled?\"\\u200f\":\"\")+page.bgPrevText", "знак RLM у «‹ Prev» листалки фонов"),
            ("text:(root.LayoutMirroring.enabled?\"\\u200f\":\"\")+page.bgNextText", "знак RLM у «Next ›» листалки фонов"),
        ],
    ),
];

/// Треугольник шапки «Сравнения» ставит страница: модель отдаёт шапку без
/// него (ядро о зеркалировании места не знает).
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

/// Где клавиши ← → значат ФИЗИЧЕСКУЮ сторону и через `RexonArrows` не идут —
/// поимённо и с причиной.
const ARROW_KEY_EXCEPTIONS: &[(&str, &str)] = &[
    // Клавиши шлёт, а не разбирает: служебный стенд проверок.
    ("LiveModelsProbe.qml", "стенд"),
];

const ARROW_KEYS: &[&str] = &["Qt.Key_Left", "Qt.Key_Right", "onLeftPressed", "onRightPressed"];

/// Клавиши ← → в нашем QML разбираются только там, где шаг берут из
/// `RexonArrows`, либо внутри физической зоны (там ← — левый край экрана или
/// окна при любом языке), кроме поимённых исключений.
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
    // Кнопки заголовка окна: ← и → у кнопки макета.
    assert!(in_zones >= 2, "клавиш внутри физических зон найдено {in_zones} — разбор сломался");
}

/// Физические зоны: файл, `id` корня зоны и почему сторона там настоящая.
/// Корень несёт `LayoutMirroring.enabled: false` и `childrenInherit: true`.
const PHYSICAL_ZONES: &[(&str, &str, &str)] = &[
    // Макет заголовка окна: группы кнопок у левого и правого края окна
    // системы; ← и «Левее» двигают кнопку к левому краю.
    ("WindowButtonsSection.qml", "titleBar", "кнопки заголовка окна"),
    // Пары «Левее/Правее» и «Добавить слева/справа»: левая кнопка пары — левая.
    ("WindowButtonsSection.qml", "moveRow", "кнопки «Левее/Правее»"),
    ("WindowButtonsSection.qml", "addRow", "кнопки «Добавить слева/справа»"),
    // Образец рабочего стола под шейдерами эффектов экрана.
    ("ScreenEffectsSection.qml", "sample", "картинка экрана"),
    // Сетка углов и краёв экрана, куда встаёт виджет Conky.
    ("ConkySettings.qml", "alignmentGrid", "углы экрана"),
    // Мини-карта экрана: мониторы и окна виджетов Conky на своих местах.
    ("ConkyScreenMap.qml", "canvas", "схема экрана"),
    // Снимок профиля Steam: части витрины по координатам картинки.
    ("SteamShowcaseSection.qml", "preview", "профиль Steam"),
    // Сверка «как в профиле / расчётная»: части витрины — куски одной
    // картинки рядом, большая слева, боковая справа, как в профиле Steam.
    ("SteamShowcaseSection.qml", "cmpParts", "части витрины рядом"),
];

/// Где зеркалирование выключено не физической зоной — поимённо: вендорная
/// вкладка Wallpaper Engine (своя сверка выше).
const OTHER_MIRROR_OFF: &[(&str, &str)] = &[("WallpaperEnginePage.qml", "vendorHost")];

const MIRROR_OFF: &str = "LayoutMirroring.enabled:false";

/// Объект разметки, у которого `id: <id>` стоит на его собственном уровне:
/// номера первой и последней строки (с нуля) и сжатый код блока.
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
        // Внутри зоны явное «по левому краю» не переворачивается и для
        // арабской строки останется левым: подписи зон — по центру.
        for bad in ["RexonText{", "Text.AlignLeft", "Text.AlignRight"] {
            assert!(
                !block.contains(bad),
                "{name}: в физической зоне «{what}» стоит `{bad}` — зеркалирование тут \
                 выключено, и подпись на языке справа налево встанет не с той стороны; \
                 выравнивать по центру"
            );
        }
    }
    // Сам разбор: `id` вложенного объекта не засчитывается внешнему.
    let probe = "Item {\n    id: outer\n    Row {\n        id: inner\n        x: 1\n    }\n}\n";
    assert_eq!(object_by_id(probe, "inner").map(|(a, b, _)| (a, b)), Some((2, 5)));
    assert_eq!(object_by_id(probe, "outer").map(|(a, b, _)| (a, b)), Some((0, 6)));
}

/// Выключить зеркалирование можно только в записанных местах: новая зона —
/// с записью в `PHYSICAL_ZONES` и причиной.
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

/// Ручная координата `x:` вне физических зон — поимённо, полным выражением
/// (без пробелов) и с причиной. Зеркалирование её не переворачивает: отступ
/// от края пишется якорем (`anchors.left` + `leftMargin`, симметричный —
/// `anchors.left`/`right` + `margins`), а где якорь не годится — через
/// `RexonArrows.placeX` или `mirrored` самого контрола.
const MANUAL_X: &[(&str, &str, &str)] = &[
    // Свои контролы: индикатор по `control.mirrored` / `visualPosition`.
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
    // Слои во всё окно и координаты окна от `mapToItem(null, …)` —
    // настоящие места на экране при любом языке.
    ("OnboardingOverlay.qml", "x:0", "слой тура во всё окно и его затемнение"),
    ("OnboardingOverlay.qml", "x:tour.hole.x", "вырез вокруг цели в координатах окна"),
    ("RexonImageViewer.qml", "x:0", "слой просмотра во всё окно"),
    (
        "RexonImageViewer.qml",
        "x:Math.round((stage.width-canvas.width)/2+viewer.panX)",
        "картинка по центру и сдвиг рукой",
    ),
    // Расчёт ядра в отсчёте от начала строки, переведённый `placeX`.
    (
        "AppearancePage.qml",
        "x:root.arrows.placeX(suggestBox.box.x,suggestBox.box.width,root.width)",
        "выпадашка подсказок поиска",
    ),
];

/// Привязка `x:` со строки `start`: сама строка и продолжения до следующего
/// свойства, скобки или пустой строки — сжатая, без пробелов.
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
        // `x: 230; y: 34; …` в одну строку — только сама `x`.
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
    // Макет экрана и снимок профиля Steam: координаты картинки.
    assert!(in_zones >= 10, "ручных `x` в физических зонах найдено {in_zones} — разбор сломался");
    // Сам разбор: продолжение выражения склеивается, соседнее свойство — нет.
    let probe = ["    x: a", "       ? b : c", "    y: 0"];
    assert_eq!(x_binding(&probe, 0), "x:a?b:c");
    assert_eq!(x_binding(&["x: 1; y: 2"], 0), "x:1");
}

/// `SplitView` зеркалирование не учитывает: панели идут слева направо в
/// порядке детей. Каждый наш `SplitView` обязан сам переставлять панели по
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

/// Где ширину содержимого уменьшают на полосу прокрутки ради РАЗМЕРА, а
/// место задают якорем, — поимённо: файл, `id` полосы и почему так можно.
const SCROLLBAR_WIDTH_EXCEPTIONS: &[(&str, &str, &str)] = &[
    // Ширина вьюпорта нужна ядру для расчёта карточки; сама карточка стоит
    // якорем от начала строки, и справа налево полоса слева её не задевает.
    ("DesktopsPage.qml", "cardScroll", "ширина вьюпорта для расчёта карточки"),
];

/// Ширина содержимого «минус полоса прокрутки» (`width - bar.width`,
/// `availableWidth - 14`, `width - (contentHeight > height ? 14 : 0)`)
/// держит место под полосу справа. При языке справа налево полоса уходит
/// влево и наезжает на начало строк, а пустая полоса остаётся справа.
/// Правильно — ширина якорями `left`/`right` и место под полосу отступом
/// `anchors.rightMargin`: его зеркалирование переносит к полосе.
///
/// ЧЕГО ПРОВЕРКА НЕ ЛОВИТ: вычитание голого числа из ширины без связи с
/// полосой (`parent.width - 14` рядом с `ScrollBar {}` без `id`) — его не
/// отличить от симметричного отступа; такие места ищутся снимком при
/// `-reverse`.
#[test]
fn content_width_does_not_subtract_the_scrollbar() {
    fn offences(text: &str) -> Vec<String> {
        let code = squash_code(text);
        // Для имён полос строки не склеиваются: иначе `id` слипся бы со
        // следующим свойством.
        let by_lines: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .map(|l| l.chars().filter(|c| !c.is_whitespace()).collect::<String>() + "\n")
            .collect();
        let by_lines = by_lines.replace("{\n", "{");
        let mut out = Vec::new();
        // Полосы с именем: `ScrollBar { id: bar`.
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
        // `availableWidth - 14`: место под полосу `ScrollView` числом.
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
    // Сам разбор: все три записи ловятся, отступ якорем — нет.
    let probe = "ScrollBar {\n    id: bar\n    policy: ScrollBar.AlwaysOn\n}\n\
                 Text { width: f.width\n        - (bar.visible ? bar.width : 0) }\n\
                 Label { width: v.availableWidth - 14 }\n\
                 Item { width: f.width - (f.contentHeight > f.height ? 14 : 0) }\n";
    assert_eq!(offences(probe), vec!["bar", "availableWidth", "contentHeight"]);
    let fine = "ScrollBar { id: bar }\nText { anchors.rightMargin: bar.visible ? bar.width : 0 }\n\
                Item { x: (c.availableWidth - h.width) }\n";
    assert!(offences(fine).is_empty());
}

/// Подстановки `%1`, которые собирает сам QML, — только через `RexonBidi`.
///
/// Ядро подставляет через `i18n::fill`, и при языке справа налево каждая
/// подстановка встаёт в изолят FSI/PDI. `String.arg` в QML изолятов не
/// ставит: имя файла или число в конце арабской фразы перетасует порядок
/// слов вокруг себя. Поэтому `.arg(` в нашем QML пишется только в
/// `RexonBidi.qml`, а сам `RexonBidi` обязан ставить изолят по направлению
/// приложения.
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

/// Каждая строка каталога со стрелкой ← → несёт пометку переводчику.
///
/// Правило одно на весь продукт (`RexonArrows.keyHint`): переводчик пишет
/// стрелку так, как она смотрит на его языке, — справа налево «затем»,
/// «вперёд» и путь меню «Настройки → Экран» пишутся через ←. Без пометки
/// переводчик перепишет стрелку как в английском исходнике, и путь меню в
/// арабском интерфейсе покажет назад. Пометку `TRANSLATORS:` ставят над
/// вызовом `tr`/`n_`; xgettext вешает её на ПЕРВЫЙ вызов в строке кода,
/// поэтому две переводимые строки в одной строке кода разносятся по разным.
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
        // Общая пометка «arrows ← → point the way…» либо своя, как у справки
        // `.rxn`: «<pre> stay left-to-right…, so keep their arrows ← as they are».
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
