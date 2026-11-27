//! Check of the "prose or technical" FILTER (`rexon_core::translator`,
//! [`is_prose_segment`] and the code context).
//!
//! ⚠️ This is NOT parity with Python: `translator.py` has no filter at all, it called
//! the engine on every text node. The check guards the NEW behavior; the old one
//! lies next door in `translator_parity.rs` and is not touched here.
//!
//! WHERE THE STRINGS COME FROM. Almost all are from live material: `mt-bench/samples.md`,
//! 12 real appearance descriptions from the OCS store (a dump of the public API
//! `api.opendesktop.org`) together with what three translation engines turn them
//! into. References to the samples are given for each group. Only the forms
//! not found in the 12 samples are made up: file extensions, `::`, `$`,
//! `~/` and a three-part version — they are marked "form not in the samples".

use rexon_core::translator::{
    apply_term_fixes, invented_latin_word, is_code_env_tag, is_product_name_token, is_prose_segment,
    is_technical_token, is_version_or_resolution_token, mask_names, mask_tech_names,
    name_casing_matches, name_placeholder, render_segments, render_segments_filtered,
    segment_html, technical_spans, translatable_flags, translatable_flags_by_sentence,
    unmask_tech_names, CodeEnvStack, Segment,
};

// ── (1) not a single letter ─────────────────────────────────────────────────

/// Separators from live descriptions (sample 10 "Qogir theme", sample 8
/// "Breeze-Noir-White-Blue Icons") — they have no letters, there is no point
/// handing them to the engine (argos appended a period to them).
#[test]
fn separators_without_letters_are_not_prose() {
    for s in [
        "---------------------------------------",
        "____",
        "_____",
        "__",
        "1.2.3", // form from the task, a version as a separate segment is not in the samples
        "5.8",
        "- - -",
    ] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

// ── (2) command line head ────────────────────────────────────────────

/// Three install commands from the Qogir description (sample 10) — exactly the ones
/// engines break into «sudo apt-get установить gtk2-двигатели-муррин».
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

/// The remaining heads from the list — on live packages from the same descriptions
/// (form not in the samples: only apt-get/yum/pacman occurred in the 12 descriptions).
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
        // Added 03.10: previously these lines were rejected because of the path in them (rule 4).
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

/// ⚠️ MIRROR of the same rule: a command head is recognized only in lower
/// case and only in a short line, otherwise the rule would eat prose —
/// English `make`, `cd`, `git` are ordinary words.
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

// ── (4) technical token ──────────────────────────────────────────────────

/// Links and paths from live descriptions (samples 2, 9, 10, 12) — lines that
/// START with something technical. Since 03.10 a line with something technical INSIDE prose is translated
/// (`prose_with_technique_inside_is_prose` below); here is what remained
/// technical as a whole.
#[test]
fn links_and_paths_are_not_prose() {
    for s in [
        "https://www.pling.com/p/1229126",
        "https://github.com/vinceliuice/Qogir-theme/blob/master/src/firefox",
        "https://github.com/Luwx/material-decoration",
        // Form not in the samples — we check each sign separately.
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

/// File extensions (form not in the samples — not a single file name in the 12 descriptions).
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

/// ⚠️ MIRROR: `--` counts as a flag only when glued to letters. A live phrase
/// from sample 12 "Aritim-Light" contains the arrow `-->` and must be translated.
#[test]
fn arrow_in_live_prose_is_not_a_command_flag() {
    let s = "You can find all the wallpapers by browsing the related collection --> Aritim-Light Wallpapers Collection";
    assert!(is_prose_segment(s), "стрелка в прозе — не ключ командной строки");
    assert!(is_technical_token("--user"), "ключ командной строки — техника");
    assert!(!is_prose_segment("kvantummanager --set Nordic"));
}

// ── (5) product names as a whole segment ────────────────────────────────────

/// The live "Supported desktops" list from the Qogir description (sample 10): engines give
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

/// Each name from the list as a separate segment, in both cases.
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

// ── (6) one token / (8) fewer than two words ──────────────────────────────────

/// Wallet addresses from sample 12 "Aritim-Light" — one token with digits.
#[test]
fn single_opaque_token_is_not_prose() {
    for s in [
        "bc1qfr86cgxt74qv0pzeayezx0j03myn69grqw5upg",
        "iota1qrp0r6f69mewngr8x5xsx88d4xzk33mkq6sfwdv5ul2y0wptw5r0ck0r7r4",
        "gtk2-engines-murrine",
        "win11OS",
        "Qt", // a product name AND too short for a "human word"
    ] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

/// The rule "fewer than two words of two or more letters" (form not in the samples —
/// we check the form itself: captions of one-letter words).
#[test]
fn fewer_than_two_real_words_is_not_prose() {
    for s in ["R G B", "a b c", "X Y"] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

// ── (7) share of non-letters above a third ────────────────────────────────────────────

/// Live lines of the INFO section of the Qogir description (sample 10). ⚠️ AN HONEST BOUNDARY:
/// these are semi-technical lines, and the non-letter share rule decides them in favor of
/// "do not translate". Little meaning is lost (`GTK+ 3.20 or later`), and nobody gets
/// a chance to break the version number.
#[test]
fn number_heavy_lines_are_not_prose() {
    for s in ["1. GTK+ 3.20 or later", "(1). GTK2 engine Murrine 0.98.1.1 or later."] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

// ── MIRROR: prose that must NOT be cut off ────────────────────────────────

/// Description headings. `Features:`, `Installation:`, `Note:` — forms from
/// the task statement; the rest are taken live from samples 6, 7, 10, 11, 12.
#[test]
fn description_headings_are_prose() {
    for s in [
        "Features:",
        "Installation:",
        "Note:",
        "INFO",                // sample 10
        "Important",           // sample 7
        "- Note -",            // sample 11
        "¡IMPORTANT!",         // sample 11
        "Suggested settings:", // sample 6
        "Suggested setings:",  // sample 7, the author's typo kept
        "RELATED PRODUCTS:",   // sample 12
        "WALLPAPERS",          // sample 12
        "FOLLOW ME!",          // sample 12
        "Folders",             // sample 6
        "DONATIONS: (Thank You!)",
    ] {
        assert!(is_prose_segment(s), "должно быть прозой: {s:?}");
    }
}

/// Ordinary description phrases — samples 1, 2, 6, 7, 8, 12.
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

/// Russian prose (the interface is in Russian, and descriptions sometimes contain Cyrillic too) —
/// the filter works on Unicode letters, not on ASCII.
#[test]
fn russian_prose_is_prose() {
    for s in ["Тёмная тема со шрифтом покрупнее", "Обои из снимка лежат здесь"] {
        assert!(is_prose_segment(s), "должно быть прозой: {s:?}");
    }
}

// ── versions and resolutions ────────────────────────────────────────────────────

#[test]
fn version_and_resolution_tokens() {
    for t in ["1.2.3", "5.8", "0.98.1.1", "v0.1", "3840x2160", "1920X1080"] {
        assert!(is_version_or_resolution_token(t), "версия/разрешение: {t:?}");
    }
    for t in ["Qogir", "gtk2", "win11OS", "10k", "3.20or"] {
        assert!(!is_version_or_resolution_token(t), "не версия: {t:?}");
    }
    // A version does NOT get in the way of neighboring prose.
    assert!(is_prose_segment("Aurorae theme for KDE 5.8 and higer"));
    assert!(is_prose_segment("Based on Yosemite v0.1"));
    // A resolution as a separate segment is technical.
    assert!(!is_prose_segment("3840x2160"));
}

// ── code context ──────────────────────────────────────────────────────

#[test]
fn code_env_tags_listed() {
    for t in ["code", "pre", "kbd", "samp", "tt", "CODE", "Pre"] {
        assert!(is_code_env_tag(t), "тег кодового окружения: {t:?}");
    }
    for t in ["p", "div", "b", "precise", "codex"] {
        assert!(!is_code_env_tag(t), "не кодовое окружение: {t:?}");
    }
}

/// Inside `code`/`pre`/`kbd`/`samp`/`tt` NOTHING is translated — not even
/// flawless prose: the author said explicitly that this is code.
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

/// The code context stack: nesting, closing, returning to prose after
/// `</pre>`, self-closing tags and literals that are not tags at all.
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

/// A live piece of the Qogir description as a whole: we translate the prose, but not the two commands and
/// the separators. The callback marker ‹…› shows who was called.
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
            // Since 03.10 a heading with a list of names separated by slashes is prose:
            // the names go under a placeholder, "distros" gets translated.
            "<p>‹Ubuntu/Mint/Debian distros:›</p>",
            "<pre>sudo apt-get install gtk2-engines-murrine gtk2-engines-pixbuf</pre>",
            "<p>‹ArchLinux:›</p>",
            "<p>pacman -S gtk-engine-murrine gtk-engines</p>",
        )
    );
}

/// Markup, spaces and order — as in the unfiltered assembly: the filter changes
/// only the SET of engine calls, not the reassembly (its port is covered by the check
/// `translator_parity.rs`).
#[test]
fn filter_changes_only_who_gets_called() {
    // Prose: the filter and the old assembly give the same result.
    let prose = "<p>Nordic is a dark theme created using the awesome Nord color palette.</p>";
    let segs = segment_html(prose);
    assert_eq!(
        render_segments_filtered(&segs, |s| format!("‹{s}›")),
        render_segments(&segs, |s| format!("‹{s}›"))
    );
    // A command: the old assembly translated it, the filter does not.
    let cmd = "<p>pacman -S gtk-engine-murrine gtk-engines</p>";
    let segs = segment_html(cmd);
    assert_eq!(render_segments_filtered(&segs, |s| format!("‹{s}›")), cmd);
    assert_ne!(render_segments(&segs, |s| format!("‹{s}›")), cmd);
    // An empty engine answer still means "keep the original".
    assert_eq!(render_segments_filtered(&segs, |_| String::new()), cmd);
}

/// Flags are lined up with segments one to one, and the engine is never called
/// on literals.
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

/// The set must not "shrink" to formalities: many live lines must
/// remain in the check.
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

// ── Protecting names INSIDE a phrase ───────────────────────────────────────────────
//
// The filter above hands these phrases to the engine — they are prose. The checks below guard what
// is done with the names BEFORE the call and AFTER it. The lines are live:
// "Sweet Kvantum theme" — sample 5 (Sweet-Mars), "A Kvantum theme heavily
// based on Microsoft's Fluent Design elements." — sample 9 ("Fluent Dark"),
// the desktop list — sample 10 ("Qogir theme").

/// A name inside a phrase goes under a placeholder, and the phrase itself does not change in
/// any other way.
#[test]
fn a_name_inside_a_phrase_goes_under_a_placeholder() {
    let masked = mask_tech_names("Sweet Kvantum theme");
    assert_eq!(masked.text, "Sweet @0@ theme");
    assert_eq!(masked.names, vec!["Kvantum".to_string()]);

    let masked = mask_tech_names("This theme is based on Cinnamon.");
    assert_eq!(masked.text, "This theme is based on @0@.");
    assert_eq!(masked.names, vec!["Cinnamon".to_string()]);

    // A phrase without names is not touched at all — and then there is no substitution.
    let clean = mask_tech_names("Nordic is a dark theme created using the awesome palette.");
    assert_eq!(clean.text, "Nordic is a dark theme created using the awesome palette.");
    assert!(clean.names.is_empty());
}

/// A chain of consecutive names goes under ONE placeholder: `KDE Plasma` is one name, and
/// the whole desktop list is one place in the text. Measured to be more robust this way: the engine
/// gets confused precisely by adjacent placeholders (see `mask_tech_names`).
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

/// A name is recognized BY WORD BOUNDARIES and by case.
#[test]
fn names_are_matched_on_word_boundaries_and_by_case() {
    // `kde` inside a word is not a name.
    for text in ["Edit kdeglobals by hand", "the gtkrc file", "Plasmatic look", "Matelight"] {
        let masked = mask_tech_names(text);
        assert!(masked.names.is_empty(), "тронули слово целиком: {text:?} → {:?}", masked.text);
    }
    // An all-lowercase spelling does not count as a name: the list holds ordinary
    // English words (`mate`, `unity`, `plasma`).
    for text in ["works with your mate", "brings unity to the desktop", "a plasma ball"] {
        let masked = mask_tech_names(text);
        assert!(masked.names.is_empty(), "подменили обычное слово: {text:?} → {:?}", masked.text);
    }
    // But the live spellings of names do count, all three forms.
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

/// There is ONE list of names: what the filter considers a product name is what gets hidden from
/// the phrase. Should the lists drift apart, this check goes red.
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
        // The filter still does not care about case — it judges the whole segment.
        assert!(is_product_name_token(&name.to_lowercase()));
    }
}

/// The circle is closed: hidden, restored, the original text comes out.
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

/// ⭐ THE MAIN THING: if there are fewer or more placeholders, the translation is NOT TAKEN.
/// Each line below is a real corruption seen on the live Firefox model while
/// choosing the placeholder form.
#[test]
fn a_lost_or_multiplied_placeholder_refuses_the_translation() {
    let names = vec!["Nordic".to_string(), "Kvantum".to_string()];
    for spoiled in [
        // a stub: the number eaten, the sign left («Nordic @ Kvantum тема»)
        "@0@ @ тема",
        // the placeholder lost completely
        "скандинавская тема",
        "@0@ тема",
        // multiplied
        "@0@ @1@ и @1@ тема",
        // a made-up number we never gave
        "@0@ @1@ @7@ тема",
        // the sign stuck to a word ("@@1@@@")
        "@0@ @1@@ тема",
    ] {
        assert_eq!(
            unmask_tech_names(spoiled, &names),
            None,
            "порча прошла как перевод: {spoiled:?}"
        );
    }
    // But an intact answer passes.
    assert_eq!(
        unmask_tech_names("@0@ @1@ тема", &names).as_deref(),
        Some("Nordic Kvantum тема")
    );
}

/// An `@` in the source turns substitution off completely: a placeholder could not be told
/// apart from the author's sign. Such text is translated as before the fix.
#[test]
fn an_at_sign_in_the_source_switches_the_mask_off() {
    let text = "Write to kvantum@example.invalid about the Kvantum theme";
    let masked = mask_tech_names(text);
    assert_eq!(masked.text, text);
    assert!(masked.names.is_empty());
    // And the translation of such text is taken as is, even if an `@` remained in it.
    assert_eq!(
        unmask_tech_names("перевод с @ внутри", &masked.names).as_deref(),
        Some("перевод с @ внутри")
    );
}

/// The placeholder form: the trailing mark keeps numbers from gluing together — `@1@` is
/// not the beginning of `@10@`, so substituting the tenth name does not affect
/// the first.
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

// ── Names from the caller: the item title and the author ───────────────────────────
//
// ⚠️ There is no list of theme titles and cannot be, but the window knows the title of the open item
// — it arrives next to the description. The lines below are live: "Nordic
// Kvantum theme" was returned by the engine as «Скандинавская Kvantum тема», "Sweet
// Kvantum theme" as «Сладкая Kvantum тема» (runs of 21.09). The technology name
// in them was already protected, but the TITLE got translated.

/// ⭐ THE MAIN THING: the item title goes under a placeholder AS A WHOLE, even if it consists of
/// several words, and together with an adjacent technology name — under ONE
/// placeholder (the engine breaks adjacent placeholders).
#[test]
fn the_item_name_goes_under_a_placeholder_whole() {
    // Title = the first word, then a technology name: a chain, one placeholder.
    let masked = mask_names("Nordic Kvantum theme", &["Nordic"]);
    assert_eq!(masked.text, "@0@ theme");
    assert_eq!(masked.names, vec!["Nordic Kvantum".to_string()]);

    let masked = mask_names("Sweet Kvantum theme", &["Sweet"]);
    assert_eq!(masked.text, "@0@ theme");
    assert_eq!(masked.names, vec!["Sweet Kvantum".to_string()]);

    // A two-word title also goes as a whole, not word by word.
    let masked = mask_names("NT Legacy is a global theme", &["NT Legacy"]);
    assert_eq!(masked.text, "@0@ is a global theme");
    assert_eq!(masked.names, vec!["NT Legacy".to_string()]);

    // A two-word title that fully matches the item title
    // "Sweet Kvantum" is hidden with one placeholder even without the shared list.
    let masked = mask_names("Sweet Kvantum theme", &["Sweet Kvantum"]);
    assert_eq!(masked.text, "@0@ theme");
    assert_eq!(masked.names, vec!["Sweet Kvantum".to_string()]);
}

/// A title IN THE MIDDLE of a phrase and in a different case: the comparison ignores case, and
/// exactly the spelling that stood in the source is returned into the text.
#[test]
fn the_item_name_is_found_mid_phrase_and_in_other_case() {
    let masked = mask_names("Install NT Legacy and enjoy the look", &["NT Legacy"]);
    assert_eq!(masked.text, "Install @0@ and enjoy the look");
    assert_eq!(masked.names, vec!["NT Legacy".to_string()]);

    // The description spells the title differently from the item caption.
    for (text, want) in [
        ("Based on the nt legacy theme", "nt legacy"),
        ("Based on the NT LEGACY theme", "NT LEGACY"),
        ("Based on the Nt Legacy theme", "Nt Legacy"),
    ] {
        let masked = mask_names(text, &["NT Legacy"]);
        assert_eq!(masked.names, vec![want.to_string()], "не узнали название в {text:?}");
        // ⭐ The source spelling is returned VERBATIM, not ours.
        assert_eq!(
            unmask_tech_names(&masked.text, &masked.names).as_deref(),
            Some(text),
            "круг не замкнулся на {text:?}"
        );
    }

    // A seam inside the title: a hyphen in the description and a space in the caption are one name.
    let masked = mask_names("The Aritim-Light wallpapers collection", &["Aritim Light"]);
    assert_eq!(masked.names, vec!["Aritim-Light".to_string()]);

    // Word boundaries: a title inside a word is not a title.
    let masked = mask_names("Check the nordicsettings file", &["Nordic"]);
    assert!(masked.names.is_empty(), "съели слово целиком: {}", masked.text);
}

/// ⛔ A title of ONE dictionary word does not eat that word in another
/// meaning: an all-lowercase spelling does not count as a title (the same rule
/// `mate`, `unity`, `plasma` from the shared list already live by).
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
    // But the item itself, capitalized or in all caps, is hidden.
    for (text, want) in [("Dark is a global theme", "Dark"), ("DARK is here", "DARK")] {
        let masked = mask_names(text, &["Dark"]);
        assert_eq!(masked.names, vec![want.to_string()], "не узнали товар в {text:?}");
    }
    // Case is required ONLY of single-word titles: a multi-word title
    // has no accidental matches.
    let masked = mask_names("a dark forest theme in the woods", &["Dark Forest"]);
    assert_eq!(masked.names, vec!["dark forest".to_string()]);
}

/// ⛔ An empty or very short title is not hidden at all, and the path stays exactly
/// as it was before the fix.
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
    // Technology names are protected as always — the core's list is in place.
    let masked = mask_names("A Kvantum theme", &["", "A"]);
    assert_eq!(masked.text, "A @0@ theme");

    // An empty list is byte-for-byte `mask_tech_names`.
    for text in [
        "Sweet Kvantum theme",
        "The theme supports Gnome, Unity, Budgie and Mate desktops.",
        "Nordic is a dark theme created using the awesome Nord color palette.",
    ] {
        assert_eq!(mask_names(text, &[]), mask_tech_names(text), "пустой список изменил подмену");
    }
}

/// Both sieves work on the caller's names too: a placeholder lost, multiplied
/// or nibbled — the translation is NOT taken; the "hidden — restored" circle is closed.
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
    // An item name with `@` inside the source — no substitution at all, as before.
    let masked = mask_names("Write to sweet@example.invalid", &["Sweet Kvantum"]);
    assert!(masked.names.is_empty());
}

// ── A title of SEVERAL words: also recognized by a significant word ──────────────
//
// ⚠️ Live acceptance 23.09: only the WHOLE chain of title words was protected, and
// compound titles were left unprotected. The lines below are verbatim from the descriptions
// of workshop items, together with what the engine returned for them.

/// ⭐ THE MAIN THING: the item "Nordic KDE" protects its name in "Nordic Plasma theme",
/// even though the whole chain `["Nordic","KDE"]` is not there.
///
/// Before the fix the engine returned: «Скандинавская - это темная тема, созданная с
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
        // The circle is closed: the name is returned verbatim.
        assert_eq!(unmask_tech_names(&masked.text, &masked.names).as_deref(), Some(text));
    }

    // The whole chain still wins over a lone word: "Nordic KDE"
    // goes as ONE placeholder, not two.
    let masked = mask_names("Nordic KDE is here", &[name]);
    assert_eq!(masked.text, "@0@ is here");
    assert_eq!(masked.names, vec!["Nordic KDE".to_string()]);
}

/// The item "Sweet Mars LAF Plasma 6": a title word is caught both across a seam
/// (`Sweet-Mars` in the text — `Sweet Mars` in the caption) and alone.
///
/// Before the fix: «Сладкий марс Kvantum тема», «Сладкая Plasma тема», «Сладкие
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

/// ⛔ Junk and descriptive title words are NOT protected: otherwise `laf`,
/// `theme`, `global`, `dark` and the number `6` from item captions would take ordinary
/// prose under a placeholder.
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
    // A title number is not a name either: "6" from "Plasma 6" does not touch prose.
    let masked = mask_names("It needs 6 files", &["Sweet Mars LAF Plasma 6"]);
    assert!(masked.names.is_empty(), "охранили число: {}", masked.text);
}

/// ⭐ The control case for case on a COMPOUND title: the rule is the same —
/// a lone word is protected only when the case matches. The theme "Dark Glass"
/// does not eat `dark`/`glass` in prose, but hides a capitalized "Glass".
#[test]
fn a_meaningful_word_of_the_name_still_obeys_the_casing_rule() {
    let masked = mask_names("a dark glass look in the woods", &["Dark Glass"]);
    assert_eq!(masked.names, vec!["dark glass".to_string()], "цепочка целиком — это товар");

    let masked = mask_names("a dark and quiet look with glass accents", &["Dark Glass"]);
    assert!(masked.names.is_empty(), "съели прозу строчными словами: {}", masked.text);

    let masked = mask_names("Glass is the second look here", &["Dark Glass"]);
    assert_eq!(masked.names, vec!["Glass".to_string()], "слово с большой буквы — товар");
}

/// ⭐ A LOST WORD: abbreviations from the list are protected in LOWERCASE spelling too.
///
/// Live measurement 23.09 (the item "Iridescent"): "A colorful theme inspired by sweet
/// kde theme." was returned by the model as «красочная тема, вдохновленная сладкой
/// темой.» — the word `kde` was NOT in the translation at all; `sweet KDE theme` and
/// the placeholder arrived intact. So protection cures it.
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

    // ⛔ The relaxation is ONLY for abbreviations from the list. Names that are
    // English words are still strict, and word boundaries are intact.
    for text in ["works with your mate", "a plasma ball", "Edit kdeglobals by hand"] {
        assert!(mask_tech_names(text).names.is_empty(), "подменили лишнее: {text:?}");
    }
    // ⛔ And ITEM TITLES get no relaxation: an all-caps caption must not
    // eat the same word in prose.
    for name in ["SWEET", "DARK"] {
        let masked = mask_names("a sweet and dark taste of the old days", &[name]);
        assert!(masked.names.is_empty(), "подпись {name:?} съела прозу: {}", masked.text);
    }
}

// ── Defects from live acceptance 24.09 ────────────────────────────────────────────
//
// Four cases came from live acceptance of the package, and three of the four turned out to be
// ours, not the engine's. The checks below guard them one by one; the verbatim
// answers of the live model "before" and "after" are in `translator_engine.rs`, the check
// `live_acceptance_defects_of_24_09`.

/// ⭐ D4b: a node with prose AND a link is handed to the engine AS SENTENCES rather than
/// rejected as a whole.
///
/// maxkaspar's comment on "Awesome Widgets" (08.12.2013): "hddtemp shows only
/// the first digit. here is a screenshot http://…". Rule 4 of
/// [`is_prose_segment`] ("a significant word is technical in form") is right for
/// the sentence with the link and wrong for the whole node: the customer saw the whole
/// comment in English. Now the node qualifies if at least one of its sentences is prose,
/// and the fate of each sentence is decided by the same rule. Since 03.10
/// the second phrase is prose too: the link in it goes under a placeholder.
#[test]
fn a_node_with_prose_and_a_link_is_judged_by_sentences() {
    let comment =
        "hddtemp shows only the first digit. here is a screenshot http://i.imgur.com/abc.png";
    // Since 03.10 a link inside a phrase does not reject it (it goes under a placeholder):
    // both the whole node and each of its sentences are prose.
    assert!(is_prose_segment(comment), "ссылка внутри прозы снова отвергает узел");
    assert!(is_prose_segment("hddtemp shows only the first digit."));
    assert!(is_prose_segment("here is a screenshot http://i.imgur.com/abc.png"));

    let segs = segment_html(comment);
    let by_sentence = translatable_flags_by_sentence(&segs, &split_sentences);
    assert_eq!(by_sentence, vec![true], "узел с прозой движку так и не достался");
    assert_eq!(translatable_flags(&segs), vec![true]);

    // ⛔ The flip side: where no sentence has prose, nothing has
    // changed — commands, paths and links do not reach the engine.
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

/// A sentence splitter for the check — on a period followed by a space. The live path takes
/// the boundaries from the translation crate itself; what matters here is not the precision of the boundaries but that
/// the decision is computed BY THEM.
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

/// ⭐ D4a: a GLUED repeat of a placeholder is cured, a repeat in different places is not.
///
/// viz67's comment on "Nordic KDE" (23.04.2022): to "Can we expect soon the
/// @0@ Bluish theme of @1@ for @2@ too?" the live model answered «…@0@ Bluish
/// тему @1@ @1@ для @2@ тоже?», the sieve rejected the translation, and the customer saw
/// the WHOLE comment in English — together with "Very nice.", which did get translated.
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
    // More than two copies in a row and whitespace of any width — the same case.
    assert_eq!(
        unmask_tech_names("тема @0@ @0@  @0@ готова", &["Nordic".to_string()]).as_deref(),
        Some("тема Nordic готова")
    );

    // ⛔ A repeat in DIFFERENT places of the line is still a refusal: there the second copy may
    // be a genuine second mention of the name, and we have no right to choose.
    assert_eq!(unmask_tech_names("@0@ тема на основе @0@", &["Nordic".to_string()]), None);
    // ⛔ And the earlier kinds of corruption are judged as before.
    assert_eq!(unmask_tech_names("тема без имени", &["Nordic".to_string()]), None);
    assert_eq!(unmask_tech_names("тема @ Nordic", &["Nordic".to_string()]), None);
    assert_eq!(unmask_tech_names("тема @@0@@@", &["Nordic".to_string()]), None);
}

/// ⭐ D2: a MADE-UP Latin word in the translation is a corruption seen neither by
/// the prose filter nor by the placeholder sieves.
///
/// The item "Awesome Widgets" (arcanis, id 998913): "graphical item support -
/// tooltips, bars" was returned by the live model as «служба графических элементов -
/// toolbep, bars». `toolbep` is not a mix of alphabets (all seven letters are ASCII) but
/// a word assembled by the subword decoder from `tooltips`; there are no names in the line
/// at all, so the corruption can only be caught by comparing with the source.
#[test]
fn an_invented_latin_word_is_caught_by_comparing_with_the_source() {
    let source = "graphical item support - tooltips, bars";
    assert_eq!(
        invented_latin_word(source, "служба графических элементов - toolbep, bars").as_deref(),
        Some("toolbep")
    );
    // Latin letters FROM the source are ours, in any case and inside a word.
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
    // A one-letter word is not judged, an entirely Russian translation is clean too.
    assert_eq!(invented_latin_word("3 x 4 tiles", "3 х 4 плитки"), None);
    assert_eq!(invented_latin_word("a dark theme", "тёмная тема"), None);
    // Inflected Latin is ours: `Kvantum` from `Kvantum's`.
    assert_eq!(invented_latin_word("Kvantum's engine", "движок Kvantum"), None);
}

// ── Technical bits INSIDE prose (owner's defect 03.10) ──────────────────────────
//
// KDE Store, "ZorinOS 6 Sound Theme": the first phrase of the description was translated, but
// "Just unzip the file and put in the /usr/share/sounds folder to add the to
// your desktop." stayed entirely in English — the old rule 4 rejected
// the line for the single path in it. Now the path goes under a placeholder, the rest
// is translated, the path is returned verbatim.

/// The owner's defect verbatim.
const SOUNDS_SENTENCE: &str =
    "Just unzip the file and put in the /usr/share/sounds folder to add the to your desktop.";

/// A phrase with a path, a link, a file name, a variable or `code` is prose.
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

/// ⛔ The flip side: a command line and a line that STARTS with something technical
/// are still not prose; an email address rejects the line as before.
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
        // Live KDE Store lines: a command INSIDE a phrase and a command with
        // non-breaking hyphens ("Sugar Candy for SDDM").
        "Copy: cp -r bigsur ~/.local/share/sounds/",
        "Use the following command: kpackagetool6 -i Alba.P6.tar.xz",
        "sddm\u{2011}greeter \u{2011}\u{2011}test\u{2011}mode \u{2011}\u{2011}theme /usr/share/sddm/themes/sugar\u{2011}candy",
        "kwriteconfig6 --file ~/.config/kwinrc --group Windows --key BorderlessMaximizedWindows true",
        "$ sudo tar -xzvf ~/Downloads/sugar-dark.tar.gz -C /usr/share/sddm/themes",
    ] {
        assert!(!is_prose_segment(s), "должно быть техникой: {s:?}");
    }
}

/// Technical pieces: the phrase's edge punctuation is not part of the piece, a pair of ordinary
/// words separated by a slash is not a piece, an email address is not a piece.
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

/// Substitution: the technical bit goes under a placeholder and comes back BYTE FOR BYTE, even
/// when the translation inflects the words around it («в папку /usr/share/sounds»).
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
    // The circle is closed for every technical form.
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
    // A name INSIDE a path is not hidden separately: the path goes as a whole.
    let masked = mask_tech_names("Copy it to ~/.local/share/KDE/themes now");
    assert_eq!(masked.names, vec!["~/.local/share/KDE/themes".to_string()]);
    // A name and a path in a row — one placeholder (the live engine breaks two in a row).
    let masked = mask_tech_names("Open Kvantum ~/.config/Kvantum and pick it");
    assert_eq!(masked.names, vec!["Kvantum ~/.config/Kvantum".to_string()]);
    // A path and a path joined by "or" — two placeholders.
    let masked = mask_tech_names("Put it in /usr/share/icons or ~/.icons today");
    assert_eq!(masked.text, "Put it in @0@ or @1@ today");
}

/// The path mark got lost, multiplied or nibbled — the translation is not taken.
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

// ── Dictionary of term corrections (after the engine) ───────────────────────────────

/// Triples "source → answer of the live Firefox en→ru model → expected" — answers
/// VERBATIM from the run of 03.10 (a probe over 375 phrases of KDE Store descriptions from the
/// OCS cache and the stand `live_technique_inside_prose_survives_the_real_engine`).
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

/// A correction ONLY by the pair: without the source term, with `extract` not about an archive,
/// with another `load` nearby, on a foreign language pair — the translation is not touched.
#[test]
fn term_fixes_leave_alone_what_they_cannot_prove() {
    let untouched = [
        // «Расстегните» not from unzip — maybe it really is a zipper.
        ("Undo the zipper on the bag.", "Расстегните молнию на сумке."),
        // extract not about an archive: a color, sounds «извлечены из».
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
        // A noun without a preposition — the replacement has a different gender, do not touch.
        (
            "If only 1 or 2 cursors show up in the theme, then the extraction of the archive didn't work.",
            "Если в теме появляются только 1 или 2 курсора, то извлечение архива не сработало.",
        ),
        // Another load nearby — «загрузить» may be about it.
        (
            "Download the theme and load it in the settings.",
            "Загрузите тему и загрузите её в настройках.",
        ),
        // The KDE window name "Download New …" — in KDE ru it is «Загрузить новые …».
        (
            "The Download New Global Themes program seems pretty bad at searching, search for win98",
            "Загружаемая программа «Новые глобальные темы» кажется довольно плохой в поиске, поиск win98",
        ),
        // «Если вы загрузите» — future tense, not imperative.
        (
            "If you download the archive, you get two folders.",
            "Если вы загрузите архив, вы получите две папки.",
        ),
    ];
    for (source, engine) in untouched {
        assert_eq!(apply_term_fixes("en", "ru", source, engine), engine, "источник: {source}");
    }
    // A foreign language pair: no groups — as is.
    assert_eq!(
        apply_term_fixes("en", "de", "Unzip it.", "Расстегни его."),
        "Расстегни его."
    );
}

/// The form is kept: imperative → imperative, infinitive → infinitive,
/// a participle keeps the same ending, the letter ё and the case of the first letter do not interfere.
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
    // A glued repeat is collapsed only for the correct form of its group.
    assert_eq!(
        apply_term_fixes("en", "ru", "Download it", "скачать скачать его"),
        "скачать его"
    );
    assert_eq!(
        apply_term_fixes("en", "ru", "Very very nice", "очень очень мило"),
        "очень очень мило"
    );
}
