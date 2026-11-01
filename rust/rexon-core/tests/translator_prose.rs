//! Сверка ФИЛЬТРА «проза или техника» (`rexon_core::translator`,
//! [`is_prose_segment`] и кодовое окружение).
//!
//! ⚠️ Это НЕ паритет с Python: в `translator.py` фильтра нет вовсе, он звал
//! движок на каждый текстовый узел. Сверка стережёт НОВОЕ поведение; старое
//! лежит рядом в `translator_parity.rs` и здесь не трогается.
//!
//! ОТКУДА СТРОКИ. Почти все — из живого материала: `mt-bench/samples.md`,
//! 12 настоящих описаний оформления из магазина OCS (выгрузка публичного API
//! `api.opendesktop.org`) вместе с тем, во что их превращают три движка
//! перевода. Ссылки на образцы стоят у каждой группы. Выдуманы только те
//! формы, которых в 12 образцах не нашлось: расширения файлов, `::`, `$`,
//! `~/` и трёхчастная версия — они помечены «формы нет в образцах».

use rexon_core::translator::{
    apply_term_fixes, invented_latin_word, is_code_env_tag, is_product_name_token, is_prose_segment,
    is_technical_token, is_version_or_resolution_token, mask_names, mask_tech_names,
    name_casing_matches, name_placeholder, render_segments, render_segments_filtered,
    segment_html, technical_spans, translatable_flags, translatable_flags_by_sentence,
    unmask_tech_names, CodeEnvStack, Segment,
};

// ── (1) нет ни одной буквы ─────────────────────────────────────────────────

/// Разделители из живых описаний (образец 10 «Qogir theme», образец 8
/// «Breeze-Noir-White-Blue Icons») — в них нет букв, движку их отдавать не за
/// чем (argos дописывал им точку в конце).
#[test]
fn separators_without_letters_are_not_prose() {
    for s in [
        "---------------------------------------",
        "____",
        "_____",
        "__",
        "1.2.3", // форма из задачи, версии как отдельного сегмента в образцах нет
        "5.8",
        "- - -",
    ] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

// ── (2) голова командной строки ────────────────────────────────────────────

/// Три установочные команды из описания Qogir (образец 10) — ровно те, что
/// движки ломают в «sudo apt-get установить gtk2-двигатели-муррин».
#[test]
fn command_lines_from_live_descriptions_are_not_prose() {
    for s in [
        "sudo apt-get install gtk2-engines-murrine gtk2-engines-pixbuf",
        "yum install gtk-murrine-engine gtk2-engines",
        "pacman -S gtk-engine-murrine gtk-engines",
    ] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

/// Остальные головы из списка — на живых пакетах из тех же описаний
/// (формы нет в образцах: в 12 описаниях встретились только apt-get/yum/pacman).
#[test]
fn every_command_head_is_recognised() {
    for s in [
        "sudo pacman -S kvantum",
        "apt install kvantum",
        "apt-get install gtk2-engines-murrine",
        "pacman -S gtk-engine-murrine",
        "yay -S kvantum-theme-nordic",
        "paru -S kvantum",
        "dnf install kvantum",
        "yum install gtk2-engines",
        "zypper install kvantum",
        "flatpak install flathub org.kde.dolphin",
        "snap install dolphin",
        "git clone Qogir-theme",
        "cd Qogir-theme",
        "chmod +x install",
        "chown user themes",
        "systemctl restart sddm",
        "makepkg -si",
        "cmake ..",
        "make install",
        // Добавлены 03.10: раньше эти строки отвергал путь в них (правило 4).
        "cp -r Nordic ~/.themes",
        "mv Nordic ~/.themes",
        "mkdir -p ~/.local/share/sounds",
        "tar xf Nordic.tar.gz",
        "kpackagetool6 -i Nordic.tar.gz",
        "lookandfeeltool -a org.kde.nordic",
        "gsettings set org.gnome.desktop.interface gtk-theme Nordic",
    ] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

/// ⚠️ ЗЕРКАЛО того же правила: голова команды опознаётся только в нижнем
/// регистре и только в короткой строке, иначе правило съедало бы прозу —
/// английские `make`, `cd`, `git` это обычные слова.
#[test]
fn prose_starting_with_command_word_survives() {
    for s in [
        "Make sure to install the Kvantum engine first",
        "Made with love for the Plasma community",
        "make the desktop look the way you want it to look, nothing else is needed here",
    ] {
        assert!(is_prose_segment(s), "должно быть прозой: {s:?}");
    }
}

// ── (4) технический токен ──────────────────────────────────────────────────

/// Ссылки и пути из живых описаний (образцы 2, 9, 10, 12) — строки, которые
/// техникой НАЧИНАЮТСЯ. С 03.10 строка с техникой ВНУТРИ прозы переводится
/// (`prose_with_technique_inside_is_prose` ниже); здесь — то, что осталось
/// техникой целиком.
#[test]
fn links_and_paths_are_not_prose() {
    for s in [
        "https://www.pling.com/p/1229126",
        "https://github.com/vinceliuice/Qogir-theme/blob/master/src/firefox",
        "https://github.com/Luwx/material-decoration",
        // Формы нет в образцах — проверяем каждый признак отдельно.
        "~/.config/Kvantum",
        "$HOME/.local/share/plasma",
        "C:\\Windows\\Fonts",
        "KConfigGroup::readEntry",
        "kvantummanager --set Nordic",
        "wehrwolfmann@example.org",
    ] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

/// Расширения файлов (формы нет в образцах — ни одного имени файла в 12 описаниях).
#[test]
fn file_names_are_not_prose() {
    for s in [
        "install.sh",
        "kvantum.conf",
        "rexon.desktop",
        "theme.py",
        "SteamPage.qml",
        "metadata.json",
        "index.theme",
        "Nordic.tar.gz",
    ] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

/// ⚠️ ЗЕРКАЛО: `--` считается ключом только приклеенным к буквам. Живая фраза
/// из образца 12 «Aritim-Light» содержит стрелку `-->` и обязана переводиться.
#[test]
fn arrow_in_live_prose_is_not_a_command_flag() {
    let s = "You can find all the wallpapers by browsing the related collection --> Aritim-Light Wallpapers Collection";
    assert!(is_prose_segment(s), "стрелка в прозе — не ключ командной строки");
    assert!(is_technical_token("--user"), "ключ командной строки — техника");
    assert!(!is_prose_segment("kvantummanager --set Nordic"));
}

// ── (5) имена продуктов целым сегментом ────────────────────────────────────

/// Живой список «Supported desktops» описания Qogir (образец 10): движки дают
/// «1 Гном», «2 Единство», «6 Корица», «3 Баджи».
#[test]
fn product_name_labels_from_live_list_are_not_prose() {
    for s in [
        "1 Gnome > 3.20",
        "2 Unity",
        "3 Budgie",
        "5 Xfce 4.12 - 4.14",
        "6 Cinnamon",
        "7 Mate",
        "8 Lxde",
        "9 Kde Plasma",
    ] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

/// Каждое имя из списка — отдельным сегментом, в обоих регистрах.
#[test]
fn every_product_name_is_recognised() {
    for s in [
        "Gnome", "GNOME", "KDE", "Kde", "Plasma", "Qt", "GTK", "gtk", "Kvantum", "Papirus",
        "Dolphin", "Konsole", "Latte", "SDDM", "Sddm", "Plymouth", "XFCE", "Xfce", "Cinnamon",
        "MATE", "Unity", "Budgie", "Lxde", "Pantheon",
    ] {
        assert!(!is_prose_segment(s), "имя продукта не переводим: {s:?}");
        assert!(is_product_name_token(s), "не опознано как имя продукта: {s:?}");
    }
    assert!(is_product_name_token("GTK+"), "краевая пунктуация обрезается");
    assert!(!is_product_name_token("Nordic"), "имя темы — не имя продукта");
}

// ── (6) один токен / (8) меньше двух слов ──────────────────────────────────

/// Адреса кошельков из образца 12 «Aritim-Light» — один токен с цифрами.
#[test]
fn single_opaque_token_is_not_prose() {
    for s in [
        "bc1qfr86cgxt74qv0pzeayezx0j03myn69grqw5upg",
        "iota1qrp0r6f69mewngr8x5xsx88d4xzk33mkq6sfwdv5ul2y0wptw5r0ck0r7r4",
        "gtk2-engines-murrine",
        "win11OS",
        "Qt", // имя продукта И слишком короткое для «человеческого слова»
    ] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

/// Правило «слов длиной от двух букв меньше двух» (формы нет в образцах —
/// проверяем саму форму: подписи из однобуквенных слов).
#[test]
fn fewer_than_two_real_words_is_not_prose() {
    for s in ["R G B", "a b c", "X Y"] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

// ── (7) доля не-букв выше трети ────────────────────────────────────────────

/// Живые строки раздела INFO описания Qogir (образец 10). ⚠️ ЧЕСТНАЯ ГРАНИЦА:
/// это строки-полутехника, и правило доли не-букв решает их в пользу
/// «не переводить». Смысл теряется мало (`GTK+ 3.20 or later`), а сломать
/// номер версии некому.
#[test]
fn number_heavy_lines_are_not_prose() {
    for s in ["1. GTK+ 3.20 or later", "(1). GTK2 engine Murrine 0.98.1.1 or later."] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

// ── ЗЕРКАЛО: проза, которую отсекать НЕЛЬЗЯ ────────────────────────────────

/// Заголовки описаний. `Features:`, `Installation:`, `Note:` — формы из
/// постановки задачи; остальные взяты живьём из образцов 6, 7, 10, 11, 12.
#[test]
fn description_headings_are_prose() {
    for s in [
        "Features:",
        "Installation:",
        "Note:",
        "INFO",                // образец 10
        "Important",           // образец 7
        "- Note -",            // образец 11
        "¡IMPORTANT!",         // образец 11
        "Suggested settings:", // образец 6
        "Suggested setings:",  // образец 7, опечатка автора сохранена
        "RELATED PRODUCTS:",   // образец 12
        "WALLPAPERS",          // образец 12
        "FOLLOW ME!",          // образец 12
        "Folders",             // образец 6
        "DONATIONS: (Thank You!)",
    ] {
        assert!(is_prose_segment(s), "должно быть прозой: {s:?}");
    }
}

/// Обычные фразы описаний — образцы 1, 2, 6, 7, 8, 12.
#[test]
fn live_description_sentences_are_prose() {
    for s in [
        "Sddm theme to fit Sweet-Mars global theme",
        "Materia pack for Plasma 5 desktop.",
        "Please use Kvantum Engine and Theme for better looking:",
        "Install full Materia pack from my collection.",
        "Nordic is a dark theme created using the awesome Nord color palette.",
        "A dark theme with awesome colors for plasma desktop",
        "Wallpaper in screenshot here",
        "Icons showed here",
        "The folders change color depending on the Color Scheme You Set",
        "omg, thanks for 10k downloads everyone",
        "Click on the \"Homepage\" link for additional infos",
        "Other: Search for the engines in your distributions repository or install the engines from source.",
        "Colorful sddm theme to fit my Sweet theme",
        "4 Pantheon (Elementary default desktop)",
    ] {
        assert!(is_prose_segment(s), "должно быть прозой: {s:?}");
    }
}

/// Русская проза (интерфейс русскоязычный, а в описаниях бывает и кириллица) —
/// фильтр работает по буквам Unicode, а не по ASCII.
#[test]
fn russian_prose_is_prose() {
    for s in ["Тёмная тема со шрифтом покрупнее", "Обои из снимка лежат здесь"] {
        assert!(is_prose_segment(s), "должно быть прозой: {s:?}");
    }
}

// ── версии и разрешения ────────────────────────────────────────────────────

#[test]
fn version_and_resolution_tokens() {
    for t in ["1.2.3", "5.8", "0.98.1.1", "v0.1", "3840x2160", "1920X1080"] {
        assert!(is_version_or_resolution_token(t), "версия/разрешение: {t:?}");
    }
    for t in ["Qogir", "gtk2", "win11OS", "10k", "3.20or"] {
        assert!(!is_version_or_resolution_token(t), "не версия: {t:?}");
    }
    // Версия НЕ мешает соседней прозе.
    assert!(is_prose_segment("Aurorae theme for KDE 5.8 and higer"));
    assert!(is_prose_segment("Based on Yosemite v0.1"));
    // Разрешение отдельным сегментом — техника.
    assert!(!is_prose_segment("3840x2160"));
}

// ── кодовое окружение ──────────────────────────────────────────────────────

#[test]
fn code_env_tags_listed() {
    for t in ["code", "pre", "kbd", "samp", "tt", "CODE", "Pre"] {
        assert!(is_code_env_tag(t), "тег кодового окружения: {t:?}");
    }
    for t in ["p", "div", "b", "precise", "codex"] {
        assert!(!is_code_env_tag(t), "не кодовое окружение: {t:?}");
    }
}

/// Внутри `code`/`pre`/`kbd`/`samp`/`tt` не переводится НИЧЕГО — даже
/// безупречная проза: автор явно сказал, что это код.
#[test]
fn prose_inside_code_env_is_not_translated() {
    for tag in ["code", "pre", "kbd", "samp", "tt", "PRE"] {
        let html = format!("<p>Installation:</p><{tag}>Suggested settings:</{tag}>");
        let segs = segment_html(&html);
        let flags = translatable_flags(&segs);
        let allowed: Vec<&str> = segs
            .iter()
            .zip(&flags)
            .filter(|(_, f)| **f)
            .map(|(s, _)| match s {
                Segment::Text { stripped, .. } => stripped.as_str(),
                Segment::Literal { text } => text.as_str(),
            })
            .collect();
        assert_eq!(allowed, vec!["Installation:"], "внутри <{tag}> переводить нечего");
    }
}

/// Стек кодового окружения: вложенность, закрытие, возврат к прозе после
/// `</pre>`, самозакрывающиеся теги и литералы, которые вовсе не теги.
#[test]
fn code_env_stack_tracks_nesting() {
    let mut env = CodeEnvStack::new();
    assert!(!env.in_code());
    env.feed_literal("&nbsp;");
    env.feed_literal("<p>");
    assert!(!env.in_code(), "обычные теги окружение не открывают");
    env.feed_literal("<pre class=\"cmd\">");
    assert!(env.in_code());
    env.feed_literal("<code>");
    env.feed_literal("</code>");
    assert!(env.in_code(), "вложенный </code> не закрывает <pre>");
    env.feed_literal("</pre>");
    assert!(!env.in_code());
    env.feed_literal("<br/>");
    assert!(!env.in_code(), "самозакрывающийся тег ничего не открывает");
    env.feed_literal("</code>");
    assert!(!env.in_code(), "лишнее закрытие не роняет стек");
}

/// Живой кусок описания Qogir целиком: прозу переводим, две команды и
/// разделители — нет. Колбэк-маркер ‹…› показывает, кого позвали.
#[test]
fn live_qogir_block_filtered_end_to_end() {
    let html = concat!(
        "<p>Qogir is a flat Design theme for GTK 3, GTK 2 and Gnome-Shell.</p>",
        "<p>---------------------------------------</p>",
        "<p>Supported desktops:</p><p>2 Unity</p><p>6 Cinnamon</p>",
        "<p>Ubuntu/Mint/Debian distros:</p>",
        "<pre>sudo apt-get install gtk2-engines-murrine gtk2-engines-pixbuf</pre>",
        "<p>ArchLinux:</p>",
        "<p>pacman -S gtk-engine-murrine gtk-engines</p>",
    );
    let segs = segment_html(html);
    let got = render_segments_filtered(&segs, |s| format!("‹{s}›"));
    assert_eq!(
        got,
        concat!(
            "<p>‹Qogir is a flat Design theme for GTK 3, GTK 2 and Gnome-Shell.›</p>",
            "<p>---------------------------------------</p>",
            "<p>‹Supported desktops:›</p><p>2 Unity</p><p>6 Cinnamon</p>",
            // С 03.10 заголовок со списком имён через косую черту — проза:
            // имена уйдут под заполнитель, «distros» переведётся.
            "<p>‹Ubuntu/Mint/Debian distros:›</p>",
            "<pre>sudo apt-get install gtk2-engines-murrine gtk2-engines-pixbuf</pre>",
            "<p>‹ArchLinux:›</p>",
            "<p>pacman -S gtk-engine-murrine gtk-engines</p>",
        )
    );
}

/// Разметка, пробелы и порядок — как у нефильтрованной сборки: фильтр меняет
/// только СОСТАВ вызовов движка, а не реассемблинг (его порт закрыт сверкой
/// `translator_parity.rs`).
#[test]
fn filter_changes_only_who_gets_called() {
    // Проза: фильтр и старая сборка дают одно и то же.
    let prose = "<p>Nordic is a dark theme created using the awesome Nord color palette.</p>";
    let segs = segment_html(prose);
    assert_eq!(
        render_segments_filtered(&segs, |s| format!("‹{s}›")),
        render_segments(&segs, |s| format!("‹{s}›"))
    );
    // Команда: старая сборка её переводила, фильтр — нет.
    let cmd = "<p>pacman -S gtk-engine-murrine gtk-engines</p>";
    let segs = segment_html(cmd);
    assert_eq!(render_segments_filtered(&segs, |s| format!("‹{s}›")), cmd);
    assert_ne!(render_segments(&segs, |s| format!("‹{s}›")), cmd);
    // Пустой ответ движка по-прежнему означает «оставить оригинал».
    assert_eq!(render_segments_filtered(&segs, |_| String::new()), cmd);
}

/// Флаги выстроены по сегментам один к одному, и на литералы движок не зовут
/// никогда.
#[test]
fn flags_line_up_with_segments() {
    let html = "<b>Important</b> &mdash; <code>sudo pacman -S kvantum</code>";
    let segs = segment_html(html);
    let flags = translatable_flags(&segs);
    assert_eq!(flags.len(), segs.len());
    for (s, f) in segs.iter().zip(&flags) {
        if matches!(s, Segment::Literal { .. }) {
            assert!(!f, "литерал движку не отдают: {s:?}");
        }
    }
    assert_eq!(flags.iter().filter(|f| **f).count(), 1, "переводится только «Important»");
}

/// Набор нельзя «усохнуть» до формальностей: живых строк в сверке должно
/// оставаться много.
#[test]
fn live_material_coverage_is_not_thin() {
    let live_technical = [
        "sudo apt-get install gtk2-engines-murrine gtk2-engines-pixbuf",
        "yum install gtk-murrine-engine gtk2-engines",
        "pacman -S gtk-engine-murrine gtk-engines",
        "---------------------------------------",
        "____",
        "1 Gnome > 3.20",
        "2 Unity",
        "3 Budgie",
        "5 Xfce 4.12 - 4.14",
        "6 Cinnamon",
        "7 Mate",
        "8 Lxde",
        "9 Kde Plasma",
        "https://www.pling.com/p/1229126",
        "https://github.com/Luwx/material-decoration",
        "bc1qfr86cgxt74qv0pzeayezx0j03myn69grqw5upg",
        "1. GTK+ 3.20 or later",
    ];
    let live_prose = [
        "Sddm theme to fit Sweet-Mars global theme",
        "Materia pack for Plasma 5 desktop.",
        "Nordic is a dark theme created using the awesome Nord color palette.",
        "Suggested settings:",
        "Folders",
        "Wallpaper in screenshot here",
        "Important",
        "- Note -",
        "INFO",
        "WALLPAPERS",
        "Fedora/RedHat distros:",
        "Ubuntu/Mint/Debian distros:",
        "You can find all the wallpapers by browsing the related collection --> Aritim-Light Wallpapers Collection",
    ];
    assert!(live_technical.len() >= 15 && live_prose.len() >= 10, "мало живых строк");
    for s in live_technical {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
    for s in live_prose {
        assert!(is_prose_segment(s), "должно быть прозой: {s:?}");
    }
}

// ── Защита имён ВНУТРИ фразы ───────────────────────────────────────────────
//
// Фильтр выше отдаёт эти фразы движку — они проза. Сверки ниже стерегут то,
// что делается с именами ПЕРЕД вызовом и ПОСЛЕ него. Строки живые:
// «Sweet Kvantum theme» — образец 5 (Sweet-Mars), «A Kvantum theme heavily
// based on Microsoft's Fluent Design elements.» — образец 9 («Fluent Dark»),
// список сред — образец 10 («Qogir theme»).

/// Имя внутри фразы уходит под заполнитель, а сама фраза не меняется больше
/// ни в чём.
#[test]
fn a_name_inside_a_phrase_goes_under_a_placeholder() {
    let masked = mask_tech_names("Sweet Kvantum theme");
    assert_eq!(masked.text, "Sweet @0@ theme");
    assert_eq!(masked.names, vec!["Kvantum".to_string()]);

    let masked = mask_tech_names("This theme is based on Cinnamon.");
    assert_eq!(masked.text, "This theme is based on @0@.");
    assert_eq!(masked.names, vec!["Cinnamon".to_string()]);

    // Фраза без имён не трогается вовсе — и подмены тогда нет.
    let clean = mask_tech_names("Nordic is a dark theme created using the awesome palette.");
    assert_eq!(clean.text, "Nordic is a dark theme created using the awesome palette.");
    assert!(clean.names.is_empty());
}

/// Цепочка имён подряд — под ОДНИМ заполнителем: `KDE Plasma` это одно имя, а
/// список сред целиком — одно место в тексте. Так замерено живучéе: движок
/// путается именно в соседних заполнителях (см. `mask_tech_names`).
#[test]
fn a_run_of_names_shares_one_placeholder() {
    let masked = mask_tech_names("works with GNOME and KDE Plasma");
    assert_eq!(masked.text, "works with @0@ and @1@");
    assert_eq!(masked.names, vec!["GNOME".to_string(), "KDE Plasma".to_string()]);

    let live = "The theme supports Gnome, Unity, Budgie, Pantheon, XFCE, Cinnamon and Mate desktops.";
    let masked = mask_tech_names(live);
    assert_eq!(masked.text, "The theme supports @0@ and @1@ desktops.");
    assert_eq!(
        masked.names,
        vec!["Gnome, Unity, Budgie, Pantheon, XFCE, Cinnamon".to_string(), "Mate".to_string()]
    );
}

/// Имя опознаётся ПО ГРАНИЦАМ СЛОВА и по регистру.
#[test]
fn names_are_matched_on_word_boundaries_and_by_case() {
    // `kde` внутри слова — не имя.
    for text in ["Edit kdeglobals by hand", "the gtkrc file", "Plasmatic look", "Matelight"] {
        let masked = mask_tech_names(text);
        assert!(masked.names.is_empty(), "тронули слово целиком: {text:?} → {:?}", masked.text);
    }
    // Полностью строчная запись именем не считается: в списке лежат обычные
    // английские слова (`mate`, `unity`, `plasma`).
    for text in ["works with your mate", "brings unity to the desktop", "a plasma ball"] {
        let masked = mask_tech_names(text);
        assert!(masked.names.is_empty(), "подменили обычное слово: {text:?} → {:?}", masked.text);
    }
    // А живые записи имён — считаются, все три формы.
    for (text, want) in [
        ("A GNOME theme", "GNOME"),
        ("A Gnome theme", "Gnome"),
        ("A Kde Plasma theme", "Kde Plasma"),
        ("A KVANTUM theme", "KVANTUM"),
        ("Sddm theme to fit Sweet-Mars global theme", "Sddm"),
    ] {
        let masked = mask_tech_names(text);
        assert_eq!(masked.names, vec![want.to_string()], "не узнали имя в {text:?}");
    }
    assert!(name_casing_matches("GNOME", "Gnome"));
    assert!(name_casing_matches("Kvantum", "KVANTUM"));
    assert!(!name_casing_matches("Mate", "mate"));
}

/// Список имён ОДИН: что фильтр считает именем продукта, то и прячется из
/// фразы. Разъедься списки — эта сверка покраснеет.
#[test]
fn the_filter_and_the_mask_share_one_list_of_names() {
    for name in [
        "GNOME", "KDE", "Plasma", "Qt", "GTK", "Kvantum", "Papirus", "Dolphin", "Konsole",
        "Latte", "SDDM", "Plymouth", "XFCE", "Cinnamon", "Mate", "Unity", "Budgie", "LXDE",
        "Pantheon", "Aurorae", "Firefox", "Nautilus", "Murrine",
    ] {
        assert!(is_product_name_token(name), "фильтр не знает имени {name:?}");
        let masked = mask_tech_names(&format!("A {name} theme for everyone"));
        assert_eq!(masked.names, vec![name.to_string()], "подмена не знает имени {name:?}");
        // Фильтру регистр по-прежнему безразличен — он судит целый сегмент.
        assert!(is_product_name_token(&name.to_lowercase()));
    }
}

/// Круг замкнут: спрятали — вернули, вышел исходный текст.
#[test]
fn hiding_and_restoring_a_name_is_a_round_trip() {
    for text in [
        "Sweet Kvantum theme",
        "A Kvantum theme heavily based on Microsoft's Fluent Design elements.",
        "This theme is based on Cinnamon and works with GNOME and KDE Plasma.",
        "Nordic Kvantum theme",
        "GTK2 engine Murrine 0.98.1.1 or later.",
        "If you want a different logo on Nautilus headerbar you can install it.",
        "Nordic is a dark theme created using the awesome Nord color palette.",
    ] {
        let masked = mask_tech_names(text);
        assert_eq!(
            unmask_tech_names(&masked.text, &masked.names).as_deref(),
            Some(text),
            "круг не замкнулся на {text:?}"
        );
    }
}

/// ⭐ ГЛАВНОЕ: заполнителей стало меньше или больше — перевод НЕ БЕРЁТСЯ.
/// Каждая строка ниже — настоящая порча, увиденная на живой модели Firefox при
/// подборе формы заполнителя.
#[test]
fn a_lost_or_multiplied_placeholder_refuses_the_translation() {
    let names = vec!["Nordic".to_string(), "Kvantum".to_string()];
    for spoiled in [
        // огрызок: номер съеден, знак остался («Nordic @ Kvantum тема»)
        "@0@ @ тема",
        // заполнитель потерян совсем
        "скандинавская тема",
        "@0@ тема",
        // размножен
        "@0@ @1@ и @1@ тема",
        // выдуманный номер, которого мы не давали
        "@0@ @1@ @7@ тема",
        // знак прилип к слову («@@1@@@»)
        "@0@ @1@@ тема",
    ] {
        assert_eq!(
            unmask_tech_names(spoiled, &names),
            None,
            "порча прошла как перевод: {spoiled:?}"
        );
    }
    // А целый ответ — проходит.
    assert_eq!(
        unmask_tech_names("@0@ @1@ тема", &names).as_deref(),
        Some("Nordic Kvantum тема")
    );
}

/// `@` в исходнике выключает подмену целиком: заполнитель было бы не отличить
/// от авторского знака. Такой текст переводится как до правки.
#[test]
fn an_at_sign_in_the_source_switches_the_mask_off() {
    let text = "Write to kvantum@example.invalid about the Kvantum theme";
    let masked = mask_tech_names(text);
    assert_eq!(masked.text, text);
    assert!(masked.names.is_empty());
    // И перевод такого текста берётся как есть, даже если `@` в нём остался.
    assert_eq!(
        unmask_tech_names("перевод с @ внутри", &masked.names).as_deref(),
        Some("перевод с @ внутри")
    );
}

/// Форма заполнителя: хвостовая метка не даёт номерам склеиться — `@1@` не
/// является началом `@10@`, поэтому подстановка десятого имени не задевает
/// первое.
#[test]
fn placeholder_numbers_do_not_collide() {
    assert_eq!(name_placeholder(0), "@0@");
    assert_eq!(name_placeholder(10), "@10@");
    assert!(!name_placeholder(10).contains(&name_placeholder(1)));
    let names: Vec<String> = (0..12).map(|i| format!("Имя{i}")).collect();
    let line: String =
        (0..12).map(|i| format!("{} ", name_placeholder(i))).collect::<String>();
    let restored = unmask_tech_names(&line, &names).expect("двенадцать имён сошлись");
    assert!(restored.starts_with("Имя0 Имя1 "), "подстановка поехала: {restored}");
    assert!(restored.contains("Имя10 Имя11"), "подстановка поехала: {restored}");
}

// ── Имена от вызывающего: название товара и автор ───────────────────────────
//
// ⚠️ Списка названий тем нет и быть не может, но название открытого товара
// окно знает — оно приходит рядом с описанием. Строки ниже живые: «Nordic
// Kvantum theme» движок отдавал как «Скандинавская Kvantum тема», «Sweet
// Kvantum theme» — как «Сладкая Kvantum тема» (прогоны 21.09). Имя технологии
// в них уже было защищено, а НАЗВАНИЕ переводилось.

/// ⭐ ГЛАВНОЕ: название товара уходит под заполнитель ЦЕЛИКОМ, даже если оно из
/// нескольких слов, и вместе с соседним именем технологии — под ОДИН
/// заполнитель (соседние заполнители движок ломает).
#[test]
fn the_item_name_goes_under_a_placeholder_whole() {
    // Название = первое слово, дальше имя технологии: цепочка, один заполнитель.
    let masked = mask_names("Nordic Kvantum theme", &["Nordic"]);
    assert_eq!(masked.text, "@0@ theme");
    assert_eq!(masked.names, vec!["Nordic Kvantum".to_string()]);

    let masked = mask_names("Sweet Kvantum theme", &["Sweet"]);
    assert_eq!(masked.text, "@0@ theme");
    assert_eq!(masked.names, vec!["Sweet Kvantum".to_string()]);

    // Название из двух слов — тоже целиком, а не по словам.
    let masked = mask_names("NT Legacy is a global theme", &["NT Legacy"]);
    assert_eq!(masked.text, "@0@ is a global theme");
    assert_eq!(masked.names, vec!["NT Legacy".to_string()]);

    // Название из двух слов, которое целиком совпало с названием товара
    // «Sweet Kvantum», прячется одним заполнителем и без общего списка.
    let masked = mask_names("Sweet Kvantum theme", &["Sweet Kvantum"]);
    assert_eq!(masked.text, "@0@ theme");
    assert_eq!(masked.names, vec!["Sweet Kvantum".to_string()]);
}

/// Название СЕРЕДИНЕ фразы и в другом регистре: сверка без учёта регистра, а в
/// текст возвращается ровно то написание, что стояло в исходнике.
#[test]
fn the_item_name_is_found_mid_phrase_and_in_other_case() {
    let masked = mask_names("Install NT Legacy and enjoy the look", &["NT Legacy"]);
    assert_eq!(masked.text, "Install @0@ and enjoy the look");
    assert_eq!(masked.names, vec!["NT Legacy".to_string()]);

    // В описании название написано иначе, чем в подписи товара.
    for (text, want) in [
        ("Based on the nt legacy theme", "nt legacy"),
        ("Based on the NT LEGACY theme", "NT LEGACY"),
        ("Based on the Nt Legacy theme", "Nt Legacy"),
    ] {
        let masked = mask_names(text, &["NT Legacy"]);
        assert_eq!(masked.names, vec![want.to_string()], "не узнали название в {text:?}");
        // ⭐ Возвращается ДОСЛОВНО исходное написание, а не наше.
        assert_eq!(
            unmask_tech_names(&masked.text, &masked.names).as_deref(),
            Some(text),
            "круг не замкнулся на {text:?}"
        );
    }

    // Шов внутри названия: дефис в описании и пробел в подписи — одно имя.
    let masked = mask_names("The Aritim-Light wallpapers collection", &["Aritim Light"]);
    assert_eq!(masked.names, vec!["Aritim-Light".to_string()]);

    // Границы слова: название внутри слова — не название.
    let masked = mask_names("Check the nordicsettings file", &["Nordic"]);
    assert!(masked.names.is_empty(), "съели слово целиком: {}", masked.text);
}

/// ⛔ Название из ОДНОГО словарного слова не съедает это слово в другом
/// значении: полностью строчная запись названием не считается (то же правило,
/// по которому уже живут `mate`, `unity`, `plasma` из общего списка).
#[test]
fn a_one_word_dictionary_name_does_not_eat_the_word_in_prose() {
    for (text, name) in [
        ("A dark and quiet theme for everyone", "Dark"),
        ("The blue accents are soft", "Blue"),
        ("A sweet taste of the old days", "Sweet"),
    ] {
        let masked = mask_names(text, &[name]);
        assert!(
            masked.names.is_empty(),
            "подменили обычное слово по названию {name:?}: {}",
            masked.text
        );
    }
    // А сам товар — с большой буквы или заглавными — прячется.
    for (text, want) in [("Dark is a global theme", "Dark"), ("DARK is here", "DARK")] {
        let masked = mask_names(text, &["Dark"]);
        assert_eq!(masked.names, vec![want.to_string()], "не узнали товар в {text:?}");
    }
    // Регистр требуется ТОЛЬКО от однословных: у названия из нескольких слов
    // случайных совпадений не бывает.
    let masked = mask_names("a dark forest theme in the woods", &["Dark Forest"]);
    assert_eq!(masked.names, vec!["dark forest".to_string()]);
}

/// ⛔ Пустое и коротенькое название не прячется вовсе, и путь остаётся ровно
/// таким, каким был до правки.
#[test]
fn empty_and_short_names_are_not_hidden() {
    for name in ["", "   ", "A", "UI", "!!", "-"] {
        let masked = mask_names("A dark theme with a UI twist", &[name]);
        assert!(
            masked.names.is_empty(),
            "спрятали по короткому названию {name:?}: {}",
            masked.text
        );
    }
    // Имена технологий при этом защищены как всегда — список ядра на месте.
    let masked = mask_names("A Kvantum theme", &["", "A"]);
    assert_eq!(masked.text, "A @0@ theme");

    // Пустой список — байт-в-байт `mask_tech_names`.
    for text in [
        "Sweet Kvantum theme",
        "The theme supports Gnome, Unity, Budgie and Mate desktops.",
        "Nordic is a dark theme created using the awesome Nord color palette.",
    ] {
        assert_eq!(mask_names(text, &[]), mask_tech_names(text), "пустой список изменил подмену");
    }
}

/// Оба сита работают и на именах вызывающего: заполнитель потерян, размножен
/// или обгрызен — перевод НЕ берётся; круг «спрятали — вернули» замкнут.
#[test]
fn the_two_sieves_cover_caller_names_too() {
    let masked = mask_names("Sweet Kvantum theme by Rtlewis", &["Sweet Kvantum", "Rtlewis"]);
    assert_eq!(masked.text, "@0@ theme by @1@");
    assert_eq!(
        unmask_tech_names(&masked.text, &masked.names).as_deref(),
        Some("Sweet Kvantum theme by Rtlewis")
    );
    for spoiled in ["@0@ тема", "@0@ тема от @1@ и @1@", "@0@ тема от @", "@0@ тема от @1@@"] {
        assert_eq!(
            unmask_tech_names(spoiled, &masked.names),
            None,
            "порча прошла как перевод: {spoiled:?}"
        );
    }
    // Имя товара с `@` внутри исходника — подмены нет вовсе, как и раньше.
    let masked = mask_names("Write to sweet@example.invalid", &["Sweet Kvantum"]);
    assert!(masked.names.is_empty());
}

// ── Название из НЕСКОЛЬКИХ слов: узнаётся и по значащему слову ──────────────
//
// ⚠️ Живая приёмка 23.09: охранялась только ЦЕЛАЯ цепочка слов названия, и
// составные названия оставались без защиты. Строки ниже — дословно из описаний
// товаров мастерской, вместе с тем, что отдавал на них движок.

/// ⭐ ГЛАВНОЕ: товар «Nordic KDE» защищает своё имя в «Nordic Plasma theme»,
/// хотя целая цепочка `["Nordic","KDE"]` там не стоит.
///
/// До правки движок отдавал: «Скандинавская - это темная тема, созданная с
/// использованием потрясающего Норд цветовая палитра цветов.», «Скандинавская
/// Plasma тема», «Скандинавская цветовая схема».
#[test]
fn a_multi_word_item_name_is_recognised_by_a_meaningful_word() {
    let name = "Nordic KDE";
    for (text, want_text, want_names) in [
        (
            "Nordic is a dark theme created using the awesome Nord color palette.",
            "@0@ is a dark theme created using the awesome @1@ color palette.",
            vec!["Nordic", "Nord"],
        ),
        ("Nordic Plasma theme", "@0@ theme", vec!["Nordic Plasma"]),
        ("Nordic Kvantum theme", "@0@ theme", vec!["Nordic Kvantum"]),
        ("Nordic color scheme", "@0@ color scheme", vec!["Nordic"]),
    ] {
        let masked = mask_names(text, &[name]);
        assert_eq!(masked.text, want_text, "не та подмена на {text:?}");
        assert_eq!(masked.names, want_names, "не те имена на {text:?}");
        // Круг замкнут: имя возвращается дословно.
        assert_eq!(unmask_tech_names(&masked.text, &masked.names).as_deref(), Some(text));
    }

    // Целая цепочка по-прежнему выигрывает у одинокого слова: «Nordic KDE»
    // уходит ОДНИМ заполнителем, а не двумя.
    let masked = mask_names("Nordic KDE is here", &[name]);
    assert_eq!(masked.text, "@0@ is here");
    assert_eq!(masked.names, vec!["Nordic KDE".to_string()]);
}

/// Товар «Sweet Mars LAF Plasma 6»: слово названия ловится и через шов
/// (`Sweet-Mars` в тексте — `Sweet Mars` в подписи), и в одиночку.
///
/// До правки: «Сладкий марс Kvantum тема», «Сладкая Plasma тема», «Сладкие
/// обои».
#[test]
fn a_long_item_name_is_recognised_by_its_parts_and_across_a_seam() {
    let name = "Sweet Mars LAF Plasma 6";
    for (text, want_text, want_names) in [
        ("Sweet-Mars Kvantum theme", "@0@ theme", vec!["Sweet-Mars Kvantum"]),
        ("Sweet Plasma theme", "@0@ theme", vec!["Sweet Plasma"]),
        ("Sweet wallpaper", "@0@ wallpaper", vec!["Sweet"]),
    ] {
        let masked = mask_names(text, &[name]);
        assert_eq!(masked.text, want_text, "не та подмена на {text:?}");
        assert_eq!(masked.names, want_names, "не те имена на {text:?}");
        assert_eq!(unmask_tech_names(&masked.text, &masked.names).as_deref(), Some(text));
    }
}

/// ⛔ Мусорные и описательные слова названия НЕ охраняются: иначе `laf`,
/// `theme`, `global`, `dark` и число `6` из подписей товаров забирали бы под
/// заполнитель обычную прозу.
#[test]
fn filler_words_of_the_item_name_are_not_guarded() {
    for (text, name) in [
        ("The LAF of this desktop is calm", "Sweet Mars LAF Plasma 6"),
        ("A Theme for the whole desktop", "Dark Glass Theme"),
        ("Global Edition for everyone", "Global Edition Icons"),
        ("Light and airy Colors with Icons", "Light Colors Icons Pack"),
    ] {
        let masked = mask_names(text, &[name]);
        assert!(masked.names.is_empty(), "охранили мусорное слово по {name:?}: {}", masked.text);
    }
    // Число названия — тоже не имя: «6» из «Plasma 6» прозу не трогает.
    let masked = mask_names("It needs 6 files", &["Sweet Mars LAF Plasma 6"]);
    assert!(masked.names.is_empty(), "охранили число: {}", masked.text);
}

/// ⭐ Контрольный случай регистра на СОСТАВНОМ названии: правило прежнее —
/// одинокое слово охраняется только при совпадении регистра. Тема «Dark Glass»
/// не съедает `dark`/`glass` в прозе, а «Glass» с большой буквы прячет.
#[test]
fn a_meaningful_word_of_the_name_still_obeys_the_casing_rule() {
    let masked = mask_names("a dark glass look in the woods", &["Dark Glass"]);
    assert_eq!(masked.names, vec!["dark glass".to_string()], "цепочка целиком — это товар");

    let masked = mask_names("a dark and quiet look with glass accents", &["Dark Glass"]);
    assert!(masked.names.is_empty(), "съели прозу строчными словами: {}", masked.text);

    let masked = mask_names("Glass is the second look here", &["Dark Glass"]);
    assert_eq!(masked.names, vec!["Glass".to_string()], "слово с большой буквы — товар");
}

/// ⭐ ПОТЕРЯ СЛОВА: аббревиатуры списка охраняются и в СТРОЧНОЙ записи.
///
/// Живой замер 23.09 (товар «Iridescent»): «A colorful theme inspired by sweet
/// kde theme.» модель отдавала как «красочная тема, вдохновленная сладкой
/// темой.» — слова `kde` в переводе НЕ БЫЛО вовсе; `sweet KDE theme` и
/// заполнитель доходили целыми. Значит лечится охраной.
#[test]
fn lowercase_acronyms_of_the_list_are_still_names() {
    for (text, want_text, want_names) in [
        (
            "A colorful theme inspired by sweet kde theme.",
            "A colorful theme inspired by sweet @0@ theme.",
            vec!["kde"],
        ),
        ("It works on kde and gnome.", "It works on @0@ and @1@.", vec!["kde", "gnome"]),
        ("Tuned for the xfce panel", "Tuned for the @0@ panel", vec!["xfce"]),
        ("Login screen for sddm", "Login screen for @0@", vec!["sddm"]),
    ] {
        let masked = mask_tech_names(text);
        assert_eq!(masked.text, want_text, "не та подмена на {text:?}");
        assert_eq!(masked.names, want_names, "не те имена на {text:?}");
        assert_eq!(unmask_tech_names(&masked.text, &masked.names).as_deref(), Some(text));
    }

    // ⛔ Послабление — ТОЛЬКО аббревиатурам списка. Имена, которые являются
    // английскими словами, по-прежнему строгие, и границы слова целы.
    for text in ["works with your mate", "a plasma ball", "Edit kdeglobals by hand"] {
        assert!(mask_tech_names(text).names.is_empty(), "подменили лишнее: {text:?}");
    }
    // ⛔ И НАЗВАНИЯ ТОВАРОВ послабления не получают: подпись заглавными не
    // должна съедать это же слово в прозе.
    for name in ["SWEET", "DARK"] {
        let masked = mask_names("a sweet and dark taste of the old days", &[name]);
        assert!(masked.names.is_empty(), "подпись {name:?} съела прозу: {}", masked.text);
    }
}

// ── Дефекты живой приёмки 24.09 ────────────────────────────────────────────
//
// Четыре случая пришли с живой приёмки пакета, и три из четырёх оказались
// нашими, а не движковыми. Сверки ниже стерегут их по отдельности; дословные
// ответы живой модели «до» и «после» — в `translator_engine.rs`, сверка
// `live_acceptance_defects_of_24_09`.

/// ⭐ D4b: узел с прозой И ссылкой отдаётся движку ПРЕДЛОЖЕНИЯМИ, а не
/// отвергается целиком.
///
/// Комментарий maxkaspar к «Awesome Widgets» (08.12.2013): «hddtemp shows only
/// the first digit. here is a screenshot http://…». Правило 4
/// [`is_prose_segment`] («значимое слово технично по форме») верно для
/// предложения со ссылкой и неверно для узла целиком: покупатель видел весь
/// комментарий английским. Теперь узел годится, если проза — хоть одно его
/// предложение, а судьбу каждого предложения решает то же правило. С 03.10
/// и вторая фраза — проза: ссылка в ней уходит под заполнитель.
#[test]
fn a_node_with_prose_and_a_link_is_judged_by_sentences() {
    let comment =
        "hddtemp shows only the first digit. here is a screenshot http://i.imgur.com/abc.png";
    // С 03.10 ссылка внутри фразы её не отвергает (уходит под заполнитель):
    // проза и узел целиком, и каждое его предложение.
    assert!(is_prose_segment(comment), "ссылка внутри прозы снова отвергает узел");
    assert!(is_prose_segment("hddtemp shows only the first digit."));
    assert!(is_prose_segment("here is a screenshot http://i.imgur.com/abc.png"));

    let segs = segment_html(comment);
    let by_sentence = translatable_flags_by_sentence(&segs, &split_sentences);
    assert_eq!(by_sentence, vec![true], "узел с прозой движку так и не достался");
    assert_eq!(translatable_flags(&segs), vec![true]);

    // ⛔ Обратная сторона: где прозы нет ни в одном предложении, ничего не
    // изменилось — команды, пути и ссылки движку не достаются.
    for technical in [
        "sudo apt-get install gtk2-engines-murrine",
        "cp -r Nordic ~/.themes",
        "https://example.invalid/shot.png",
        "cmake --build build",
    ] {
        let segs = segment_html(technical);
        assert_eq!(
            translatable_flags_by_sentence(&segs, &split_sentences),
            vec![false],
            "техника поехала к движку: {technical:?}"
        );
    }
}

/// Разбиватель предложений для сверки — по точке с пробелом. Живой путь берёт
/// границы у самого крейта перевода; здесь важна не точность границ, а то, что
/// решение считается ПО НИМ.
fn split_sentences(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        if matches!(c, '.' | '!' | '?') && chars.peek().is_some_and(|n| n.is_whitespace()) {
            out.push(current.trim().to_string());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// ⭐ D4a: СЛИПШИЙСЯ повтор заполнителя лечится, повтор в разных местах — нет.
///
/// Комментарий viz67 к «Nordic KDE» (23.04.2022): на «Can we expect soon the
/// @0@ Bluish theme of @1@ for @2@ too?» живая модель ответила «…@0@ Bluish
/// тему @1@ @1@ для @2@ тоже?», сито отвергало перевод, и покупатель видел
/// английским ВЕСЬ комментарий — вместе с «Very nice.», которое перевелось.
#[test]
fn a_glued_repeat_of_a_placeholder_is_healed() {
    let names = vec!["Nordic".to_string(), "Gnome".to_string(), "KDE".to_string()];
    assert_eq!(
        unmask_tech_names(
            "можем ли мы ожидать в ближайшее время @0@ Bluish тему @1@ @1@ для @2@ тоже?",
            &names
        )
        .as_deref(),
        Some("можем ли мы ожидать в ближайшее время Nordic Bluish тему Gnome для KDE тоже?")
    );
    // Больше двух копий подряд и пробел любой ширины — тот же случай.
    assert_eq!(
        unmask_tech_names("тема @0@ @0@  @0@ готова", &["Nordic".to_string()]).as_deref(),
        Some("тема Nordic готова")
    );

    // ⛔ Повтор в РАЗНЫХ местах строки по-прежнему отказ: там вторая копия может
    // быть настоящим вторым упоминанием имени, и выбирать мы не вправе.
    assert_eq!(unmask_tech_names("@0@ тема на основе @0@", &["Nordic".to_string()]), None);
    // ⛔ И прежние виды порчи судятся как раньше.
    assert_eq!(unmask_tech_names("тема без имени", &["Nordic".to_string()]), None);
    assert_eq!(unmask_tech_names("тема @ Nordic", &["Nordic".to_string()]), None);
    assert_eq!(unmask_tech_names("тема @@0@@@", &["Nordic".to_string()]), None);
}

/// ⭐ D2: ВЫДУМАННОЕ латинское слово в переводе — порча, которую не видят ни
/// фильтр прозы, ни сита заполнителей.
///
/// Товар «Awesome Widgets» (arcanis, id 998913): «graphical item support -
/// tooltips, bars» живая модель отдала как «служба графических элементов -
/// toolbep, bars». `toolbep` — не смесь алфавитов (все семь букв ASCII), а
/// слово, собранное подсловным декодировщиком из `tooltips`; имён в строке нет
/// вовсе, значит ловить порчу можно только сравнением с источником.
#[test]
fn an_invented_latin_word_is_caught_by_comparing_with_the_source() {
    let source = "graphical item support - tooltips, bars";
    assert_eq!(
        invented_latin_word(source, "служба графических элементов - toolbep, bars").as_deref(),
        Some("toolbep")
    );
    // Латиница ИЗ источника — своя, в любом регистре и внутри слова.
    for clean in [
        "Kvantum тема на основе Fluent Design",
        "тема для KDE Plasma",
        "цветовая палитра Nord",
        "служба графических элементов - tooltips, bars",
    ] {
        let source = "Kvantum theme based on Fluent Design elements for KDE Plasma \
                      with the Nord palette and graphical item support - tooltips, bars";
        assert_eq!(invented_latin_word(source, clean), None, "своя латиница признана порчей");
    }
    // Слово из одной буквы не судится, целиком русский перевод — тоже чист.
    assert_eq!(invented_latin_word("3 x 4 tiles", "3 х 4 плитки"), None);
    assert_eq!(invented_latin_word("a dark theme", "тёмная тема"), None);
    // Склонённая латиница своя: `Kvantum` из `Kvantum's`.
    assert_eq!(invented_latin_word("Kvantum's engine", "движок Kvantum"), None);
}

// ── Техника ВНУТРИ прозы (дефект владельца 03.10) ──────────────────────────
//
// KDE Store, «ZorinOS 6 Sound Theme»: первая фраза описания переводилась, а
// «Just unzip the file and put in the /usr/share/sounds folder to add the to
// your desktop.» оставалась английской целиком — прежнее правило 4 отвергало
// строку за один путь в ней. Теперь путь уходит под заполнитель, остальное
// переводится, путь возвращается дословно.

/// Дефект владельца дословно.
const SOUNDS_SENTENCE: &str =
    "Just unzip the file and put in the /usr/share/sounds folder to add the to your desktop.";

/// Фраза с путём, ссылкой, именем файла, переменной или `кодом` — проза.
#[test]
fn prose_with_technique_inside_is_prose() {
    for s in [
        SOUNDS_SENTENCE,
        "Copy the folder to ~/.themes and pick it in Settings.",
        "Extract the archive into ~/.local/share/icons/ and select it.",
        "Visit https://www.pling.com/p/1229126 for more themes.",
        "Qogir icon theme: https://www.opendesktop.org/p/1296407/",
        "Preview: https://example.invalid/shot.png",
        "Download the Nordic.tar.gz archive and unpack it.",
        "Put the index.theme file next to the icons.",
        "Set $HOME/.icons as your icon folder.",
        "Run `make install` to build it.",
        "Run install.sh to set it up",
        "Use the --user flag if you have no root access.",
        "The light and dark versions are bundled (see ~/.themes).",
    ] {
        assert!(is_prose_segment(s), "должно быть прозой: {s:?}");
    }
}

/// ⛔ Обратная сторона: строка-команда и строка, которая техникой НАЧИНАЕТСЯ,
/// по-прежнему не проза; почта отвергает строку, как прежде.
#[test]
fn pure_commands_and_paths_stay_technical() {
    for s in [
        "sudo pacman -S gtk-engine-murrine",
        "make install",
        "cp -r Nordic ~/.themes",
        "unzip Nordic.zip -d ~/.themes",
        "kvantummanager --set Nordic",
        "./install.sh --user",
        "~/.config/Kvantum",
        "/usr/share/sounds",
        "`make install`",
        "$HOME/.local/share/plasma",
        "https://www.pling.com/p/1229126",
        "Write to wehrwolfmann@example.org for help",
        // Живые строки KDE Store: команда ВНУТРИ фразы и команда на
        // неразрывных дефисах («Sugar Candy for SDDM»).
        "Copy: cp -r bigsur ~/.local/share/sounds/",
        "Use the following command: kpackagetool6 -i Alba.P6.tar.xz",
        "sddm\u{2011}greeter \u{2011}\u{2011}test\u{2011}mode \u{2011}\u{2011}theme /usr/share/sddm/themes/sugar\u{2011}candy",
        "kwriteconfig6 --file ~/.config/kwinrc --group Windows --key BorderlessMaximizedWindows true",
        "$ sudo tar -xzvf ~/Downloads/sugar-dark.tar.gz -C /usr/share/sddm/themes",
    ] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

/// Куски техники: краевая пунктуация фразы в кусок не входит, пара обычных
/// слов через косую черту — не кусок, почта — не кусок.
#[test]
fn technical_spans_cut_exactly_the_technique() {
    let pieces = |t: &str| -> Vec<String> {
        technical_spans(t).iter().map(|(a, b)| t[*a..*b].to_string()).collect()
    };
    assert_eq!(pieces(SOUNDS_SENTENCE), vec!["/usr/share/sounds"]);
    assert_eq!(pieces("Copy it to ~/.themes."), vec!["~/.themes"]);
    assert_eq!(pieces("(see ~/.themes)"), vec!["~/.themes"]);
    assert_eq!(pieces("Visit https://x.invalid/p/1, then"), vec!["https://x.invalid/p/1"]);
    assert_eq!(pieces("Run `make install` now"), vec!["`make install`"]);
    assert_eq!(pieces("Set $HOME and *.tar.gz here"), vec!["$HOME", "*.tar.gz"]);
    assert_eq!(pieces("Put index.theme there"), vec!["index.theme"]);
    assert!(pieces("light and/or dark").is_empty(), "and/or — проза, не путь");
    assert!(pieces("browse the collection --> Aritim").is_empty(), "стрелка — не ключ");
    assert!(pieces("Write to me@example.org").is_empty(), "почта не маскируется");
    assert!(pieces("Version 1.2.3 is out").is_empty(), "версия — не техника");
}

/// Подмена: техника уходит под заполнитель и возвращается БАЙТ В БАЙТ, даже
/// когда перевод склоняет слова вокруг неё («в папку /usr/share/sounds»).
#[test]
fn technique_is_masked_and_restored_byte_for_byte() {
    let masked = mask_tech_names(SOUNDS_SENTENCE);
    assert_eq!(
        masked.text,
        "Just unzip the file and put in the @0@ folder to add the to your desktop."
    );
    assert_eq!(masked.names, vec!["/usr/share/sounds".to_string()]);
    let russian = "Просто распакуйте файл и положите в папку @0@, чтобы добавить на рабочий стол.";
    assert_eq!(
        unmask_tech_names(russian, &masked.names).as_deref(),
        Some("Просто распакуйте файл и положите в папку /usr/share/sounds, чтобы добавить на рабочий стол.")
    );
    // Круг замкнут для каждой формы техники.
    for text in [
        SOUNDS_SENTENCE,
        "Visit https://www.pling.com/p/1229126/ for more.",
        "Run `sudo make install` and log out.",
        "Set $HOME/.icons, ~/.local/share/icons and *.tar.gz here.",
        "Copy Nordic.colors to ~/.local/share/color-schemes/ and apply it in KDE Plasma.",
    ] {
        let masked = mask_tech_names(text);
        assert!(!masked.names.is_empty(), "техника не спрятана: {text:?}");
        assert!(
            !masked.text.contains('/') && !masked.text.contains('`') && !masked.text.contains('$'),
            "техника осталась на виду движка: {:?}",
            masked.text
        );
        assert_eq!(unmask_tech_names(&masked.text, &masked.names).as_deref(), Some(text));
    }
    // Имя ВНУТРИ пути отдельно не прячется: путь уходит целиком.
    let masked = mask_tech_names("Copy it to ~/.local/share/KDE/themes now");
    assert_eq!(masked.names, vec!["~/.local/share/KDE/themes".to_string()]);
    // Имя и путь подряд — один заполнитель (два подряд живой движок ломает).
    let masked = mask_tech_names("Open Kvantum ~/.config/Kvantum and pick it");
    assert_eq!(masked.names, vec!["Kvantum ~/.config/Kvantum".to_string()]);
    // Путь и путь через «or» — два заполнителя.
    let masked = mask_tech_names("Put it in /usr/share/icons or ~/.icons today");
    assert_eq!(masked.text, "Put it in @0@ or @1@ today");
}

/// Метка пути потерялась, размножилась или обгрызена — перевод не берётся.
#[test]
fn a_lost_technique_placeholder_rejects_the_translation() {
    let masked = mask_tech_names(SOUNDS_SENTENCE);
    for spoiled in [
        "Просто распакуйте файл и положите в папку, чтобы добавить.",
        "Распакуйте в @0@ и в @0@ тоже.",
        "Распакуйте в папку @ и добавьте.",
        "Распакуйте в папку @1@.",
    ] {
        assert_eq!(unmask_tech_names(spoiled, &masked.names), None, "порчу взяли: {spoiled:?}");
    }
}

// ── Словарь поправок терминов (после движка) ───────────────────────────────

/// Пары «источник → ответ живой модели Firefox en→ru → ждём» — ответы
/// ДОСЛОВНО из прогона 03.10 (зонд по 375 фразам описаний KDE Store из кеша
/// OCS и стенд `live_technique_inside_prose_survives_the_real_engine`).
#[test]
fn term_fixes_turn_engine_slips_into_rexon_terms() {
    let cases = [
        (
            "Just unzip the file and put in the /usr/share/sounds folder to add the to your desktop.",
            "Просто расстегните файл и поместите в папку /usr/share/sounds, чтобы добавить на рабочий стол.",
            "Просто распакуйте файл и поместите в папку /usr/share/sounds, чтобы добавить на рабочий стол.",
        ),
        ("Unzip it.", "Расстегни его.", "Распакуй его."),
        (
            "Download bigsur-sound-theme.tar.gz from Files",
            "Скачать скачать bigsur-sound-theme.tar.gz с Файлы",
            "Скачать bigsur-sound-theme.tar.gz со вкладки «Файлы»",
        ),
        (
            "Extract the archive into ~/.local/share/sounds.",
            "Извлеките архив в ~/.local/share/sounds.",
            "Распакуйте архив в ~/.local/share/sounds.",
        ),
        (
            "After installing Kvantum Manager, download and extract \"maliceandspitekv.zip\" from this page",
            "После установки Kvantum Manager скачать и вытащить \"maliceandspitekv.zip\" с этой страницы",
            "После установки Kvantum Manager скачать и распаковать \"maliceandspitekv.zip\" с этой страницы",
        ),
        (
            "Move all the folders inside the extracted archive to ~/.themes.",
            "Переместите все папки внутри извлеченного архива в ~/.themes.",
            "Переместите все папки внутри распакованного архива в ~/.themes.",
        ),
        (
            "After extracting, copy the folder to ~/.themes.",
            "После извлечения скопируйте папку в ~/.themes.",
            "После распаковки скопируйте папку в ~/.themes.",
        ),
        (
            "Download the archive and unpack it.",
            "Скашляйте архив и распаковывайте его.",
            "Скачайте архив и распаковывайте его.",
        ),
        (
            "Download the `master.zip` package and extract it.",
            "Загрузите пакет `master.zip` и извлеките его.",
            "Скачайте пакет `master.zip` и распакуйте его.",
        ),
        (
            "Drag and drop the file into the window.",
            "Перетащите и высадите файл в окно.",
            "Перетащите файл в окно.",
        ),
        (
            "Installation: Open the ZIP file and extract the folder \"MacOS Sounds\" to ~/.local/share/sounds",
            "Инсталляция: Откройте ZIP-файл и извайте папку \"MacOS Sounds\" на ~/.local/share/sounds",
            "Установка: Откройте ZIP-файл и распакуйте папку \"MacOS Sounds\" на ~/.local/share/sounds",
        ),
        (
            "To install, unzip the archive to ~/.icons.",
            "Для установки распечатайте архив на ~/.icons.",
            "Для установки распакуйте архив на ~/.icons.",
        ),
        (
            "Install the icon pack from the Get New Icons button.",
            "Установите пакет значков из кнопки Get New Icons.",
            "Установите набор значков из кнопки Get New Icons.",
        ),
    ];
    for (source, engine, want) in cases {
        assert_eq!(apply_term_fixes("en", "ru", source, engine), want, "источник: {source}");
    }
}

/// Правка ТОЛЬКО по паре: без исходного термина, при `extract` не про архив,
/// с другим `load` рядом, на чужой паре языков — перевод не трогается.
#[test]
fn term_fixes_leave_alone_what_they_cannot_prove() {
    let untouched = [
        // «Расстегните» не от unzip — может, правда застёжка.
        ("Undo the zipper on the bag.", "Расстегните молнию на сумке."),
        // extract не про архив: цвет, звуки «извлечены из».
        (
            "~Dynamic Accent Colors: Automatically extracts and applies the dominant color from your wallpaper to UI elements.",
            "Динамичные цвета акцента: автоматически извлекает и применяет доминирующий цвет от ваших обоев к элементам пользовательского интерфейса.",
        ),
        (
            "All sounds have been extracted from MacOS Monterey and converted to ogg using ffmpeg.",
            "Все звуки были извлечены из MacOS Monterey и преобразованы в ogg с помощью ffmpeg.",
        ),
        (
            "Native PNG Rendering: Uses lightweight QML frame-sequencing directly from extracted source files.",
            "Нативный PNG-рендеринг: использует QML непосредственно из извлеченных исходных файлов.",
        ),
        // Существительное без предлога — род у замены другой, не трогаем.
        (
            "If only 1 or 2 cursors show up in the theme, then the extraction of the archive didn't work.",
            "Если в теме появляются только 1 или 2 курсора, то извлечение архива не сработало.",
        ),
        // Рядом другой load — «загрузить» может быть про него.
        (
            "Download the theme and load it in the settings.",
            "Загрузите тему и загрузите её в настройках.",
        ),
        // Имя окна KDE «Download New …» — у KDE ru «Загрузить новые …».
        (
            "The Download New Global Themes program seems pretty bad at searching, search for win98",
            "Загружаемая программа «Новые глобальные темы» кажется довольно плохой в поиске, поиск win98",
        ),
        // «Если вы загрузите» — будущее, не повелительное.
        (
            "If you download the archive, you get two folders.",
            "Если вы загрузите архив, вы получите две папки.",
        ),
    ];
    for (source, engine) in untouched {
        assert_eq!(apply_term_fixes("en", "ru", source, engine), engine, "источник: {source}");
    }
    // Чужая пара языков: групп нет — как есть.
    assert_eq!(
        apply_term_fixes("en", "de", "Unzip it.", "Расстегни его."),
        "Расстегни его."
    );
}

/// Форма сохраняется: повелительное → повелительное, инфинитив → инфинитив,
/// причастие — с тем же окончанием, ё и регистр первой буквы не мешают.
#[test]
fn term_fixes_keep_the_engine_form() {
    let source = "Unzip the archive.";
    for (engine, want) in [
        ("Расстегните архив.", "Распакуйте архив."),
        ("Расстегнуть архив.", "Распаковать архив."),
        ("расстегнуть архив.", "распаковать архив."),
        ("Расстёгнутый архив.", "Распакованный архив."),
        ("В расстёгнутой папке.", "В распакованной папке."),
        ("После расстегивания архива.", "После распаковки архива."),
    ] {
        assert_eq!(apply_term_fixes("en", "ru", source, engine), want);
    }
    // Слипшийся повтор сворачивается только у верной формы своей группы.
    assert_eq!(
        apply_term_fixes("en", "ru", "Download it", "скачать скачать его"),
        "скачать его"
    );
    assert_eq!(
        apply_term_fixes("en", "ru", "Very very nice", "очень очень мило"),
        "очень очень мило"
    );
}
