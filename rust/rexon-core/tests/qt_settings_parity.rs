//! Golden comparison of `qt_settings.rs` — the «Настройки» tab.
//!
//! WHAT THE GOLDEN DATA IS HERE (since 02.10.2026). Python in Rexon is only an
//! archive: it is neither a reference nor a bridge. `golden/qt_settings_cases.json`
//! and `golden/qt_settings_ru_before.json` are frozen OWN golden files of the
//! Rust code; comparisons against the widget tab's source are removed, their
//! numbers and colours are recorded here (`LAYOUT_FROZEN`, `status_colors_are_frozen`).
//!
//! Where the snapshot comes from. It was captured by running the former module
//! `rexon/qt_settings/window.py` (commit `94d8d57`) under offscreen Qt:
//!   * `_detect_wallpaper_format()` — twenty paths, including traps
//!     ("a directory with a dot in its name", a file without extension, UPPER case);
//!   * sound themes — the RULE on a fake tree (`sound_rules`), not the machine's
//!     contents: what counts as a theme, who is on the exclusion list, how two
//!     directories are merged, which extension wins, what happens on an empty
//!     place, the order of the «послушать» buttons, event icons and labels.
//!     Previously a snapshot of the LIVE machine stood here: five installed themes.
//!     Such a check lied both ways — it went red where `budgie` and `mate` were
//!     not installed, and green with a broken rule if the needed directories
//!     happened to be in place; the September 2026 recapture broke four
//!     sections without changing a single line of code;
//!   * `_parse_font_str()` — twelve font entries (GNOME, KDE, fractional size,
//!     garbage instead of a size) and three DE key tables;
//!   * `_pretty_desktop_id()`, `_pretty_backup_ts()`, `_files_word()`.
//!
//! Sanctioned differences (fixes of the original, rule #16) are NOT reflected
//! in the golden data — it records Python's behaviour as is; each one is
//! checked by a separate test at the bottom of the file:
//!   1. the size was lost if the font string was quoted or had a trailing
//!      space (`gsettings get` returns exactly the quotes);
//!   2. `_pretty_backup_ts` now counts the month length — «30 февраля» stays
//!      as is (in Python `strptime` did that; we check that we do the same);
//!   3. the user's sound theme directory is taken from `XDG_DATA_HOME`.
//!
//! SOUND COMPARISON BOUNDARY. The original hard-coded the pair of directories
//! (`/usr/share/sounds` and `~/.local/share/sounds`), so the generator swaps
//! them, and HERE the pair is built by [`qs::sound_theme_dirs`] — that is fix 3.
//! What exactly goes into the pair is checked by
//! `sound_dirs_follow_xdg_data_home`; the rules for walking this pair by the
//! checks below; the very fact that the product calls `sound_theme_dirs` with
//! the user's XDG directory is not covered by any core check.
//!
//! THE "golden data must catch a regression" GATE was verified by mutation:
//! nine targeted breakages of the implementation (`gif` classed as static;
//! `.tif` lost from the extension list; sound lookup without extension
//! priority; showcase without recognizable events first; `pretty_desktop_id`
//! takes the first segment instead of the last; day and month swapped in
//! `pretty_backup_ts`; «alsa» and other non-themes get into the theme list;
//! KDE sounds go down the gsettings branch; the «файлов» plural is computed
//! from the last digit) — each turns the tests red. Two of them survived the
//! first edition of the checks and needed polishing: iterating over the
//! extension list itself turned out to be a tautology (now the filter is
//! pinned as a string), and the plural was checked only directly, bypassing
//! the backup string (a case for 11 was added).
//!
//! COMPARISON LANGUAGE (since 28.09.2026). The tab's strings moved to the
//! translation catalog: English `msgid` in the core, Russian in `po/ru.po`. The
//! Python golden data remembers RUSSIAN texts, so comparisons against it run in
//! Russian built from the real `po/ru.po` (`i18n_pin::ru()`) — they are the
//! "Russian as before" check. Separately: `russian_catalog_keeps_old_texts`
//! compares the WHOLE tab catalog with the pre-migration snapshot
//! (`golden/qt_settings_ru_before.json`), and `english_*` the English
//! interface (`i18n_pin::en()`).

mod i18n_pin;

use std::collections::BTreeMap;
use std::fs;

use rexon_core::i18n;
use rexon_core::qt_settings as qs;
use serde_json::Value;

/// Russian word form by number: «1 файл», «3 файла», «5 файлов» — the check's
/// own oracle, independent of the translation catalog it verifies.
///
/// The rule is the Russian one from CLDR (one / few / many): last digit 1,
/// except 11, — first form; 2–4, except 12–14, — second; the rest — third.
fn plural<'a>(n: i64, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    let n = n.unsigned_abs();
    let (last, last_two) = (n % 10, n % 100);
    if last == 1 && last_two != 11 {
        one
    } else if (2..=4).contains(&last) && !(12..=14).contains(&last_two) {
        few
    } else {
        many
    }
}

/// ⭐ EMPTY SAMPLE GUARD: "there was nothing to compare" is a failure, not a success.
///
/// ⚠️ This is a class, not a single case. A check "the text does NOT contain
/// a forbidden string" passes by itself on empty input: emptiness contains
/// nothing, the forbidden included. A completely empty guarded file was caught
/// before — positive checks next to it failed — but DERIVED text (a cut-out
/// piece, filtered lines) could become empty unnoticed. Measured 19.09: a page
/// source of THREE comment lines containing the searched-for calls passed the
/// banner guard GREEN — the positive checks read the comments, and the
/// prohibitions looked into the emptiness left after the filter.
///
/// So every sample — read from disk or derived — must first prove that it is
/// non-empty, and only then take part in comparisons.
fn nonempty<'a>(text: &'a str, what: &str) -> &'a str {
    assert!(
        !text.trim().is_empty(),
        "{what}: образец пуст — сверка прошла бы впустую, не проверив ничего"
    );
    text
}

/// Reads a source for a text check and proves there was something to read.
fn read_sample(path: &str, what: &str) -> String {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{what}: не удалось прочитать {path}: {e}"));
    nonempty(&text, what);
    text
}

fn golden() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/qt_settings_cases.json"
    );
    let text = read_sample(path, "эталон qt_settings_cases.json");
    serde_json::from_str(&text).expect("эталон не разбирается как JSON")
}

/// A fake sound theme tree from the golden data: built in a temporary
/// directory exactly by the `sound_rules.tree` list recorded by the generator,
/// and returns the "system, user" pair of directories — in the same order in
/// which the original walked them.
///
/// ⚠️ The directory is unique per test name: libtest runs checks inside one
/// binary IN PARALLEL, and they would wipe a shared tree from under each other.
struct SoundTree {
    root: std::path::PathBuf,
}

impl SoundTree {
    fn new(tag: &str, entries: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "rexon-snd-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        for rel in entries {
            let full = root.join(rel);
            if rel.ends_with('/') {
                fs::create_dir_all(&full).unwrap();
            } else {
                fs::create_dir_all(full.parent().unwrap()).unwrap();
                fs::write(&full, b"").unwrap();
            }
        }
        // Both directories must exist even when empty: the original always
        // has them too, they are just sometimes without contents.
        fs::create_dir_all(root.join("sys")).unwrap();
        fs::create_dir_all(root.join("user")).unwrap();
        Self { root }
    }

    /// The tree from the golden data.
    fn from_golden(tag: &str, g: &Value) -> Self {
        let entries: Vec<&str> = g["sound_rules"]["tree"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        Self::new(tag, &entries)
    }

    /// The directory pair in the original's order: system first, then the user's.
    fn dirs(&self) -> Vec<String> {
        vec![
            self.root.join("sys").to_string_lossy().into_owned(),
            self.root.join("user").to_string_lossy().into_owned(),
        ]
    }

    /// A path inside the tree — as the generator recorded it.
    fn rel(&self, path: &str) -> String {
        path.strip_prefix(&format!("{}/", self.root.display()))
            .unwrap_or(path)
            .to_string()
    }
}

impl Drop for SoundTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn wallpaper_format_matches_python() {
    for case in golden()["wallpaper_format"].as_array().unwrap() {
        let path = case[0].as_str().unwrap();
        let want = case[1].as_str().unwrap();
        assert_eq!(
            qs::detect_wallpaper_format(path),
            want,
            "формат обоев для {path:?}"
        );
    }
}

/// The theme list: the rule "what counts as a theme", not the machine's contents.
///
/// WHAT BROKE. The previous edition called `qs::list_sound_themes` on the LIVE
/// `/usr/share/sounds` and `$HOME/.local/share/sounds` — the theme list was a
/// property of the machine, not of the code. Then the check went by the Python
/// golden data, where a directory with only sounds inside also counted as a
/// theme; Python has been an archive since 02.10.2026, and the rule turned out
/// wrong: GNOME/MATE/Budgie sets without `index.theme` and archive wrappers got
/// into the list, although KDE and libcanberra do not consider them themes and
/// replace the chosen name with `freedesktop`.
///
/// HOW IT IS NOW (03.10.2026). The tree is built right here. A theme is a
/// TOP-level directory with `index.theme` and a `[Sound Theme]` section;
/// `alsa`, `speech-dispatcher` and `sox` are not themes even with it;
/// directories are merged without repeats; the order is ascending; the rule
/// does not depend on names.
#[test]
fn sound_themes_are_only_real_themes() {
    const THEME: &str = "[Sound Theme]\nName=T\nDirectories=stereo\n[stereo]\nOutputProfile=stereo\n";
    for own in ["oxygen", "чужая-тема"] {
        let root = std::env::temp_dir().join(format!("rexon-snd-{}-real-{own}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let put = |rel: &str, text: &str| {
            let p = root.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, text).unwrap();
        };
        put(&format!("sys/{own}/index.theme"), THEME);
        put(&format!("sys/{own}/stereo/bell.oga"), "");
        put("sys/alsa/index.theme", THEME);
        put("sys/gnome/default/alerts/bark.ogg", "");
        put("sys/mate/default/alerts/drip.ogg", "");
        put("sys/budgie/default/alerts/x.oga", "");
        put("sys/значки/index.theme", "[Icon Theme]\nName=I\n");
        put("sys/не-каталог.oga", "");
        fs::create_dir_all(root.join("sys/пустая")).unwrap();
        put("user/Ocean/index.theme", THEME);
        put(&format!("user/{own}/index.theme"), THEME);
        put("user/zorinos2/sounds/Zorin/index.theme", THEME);
        put("user/тема с пробелом/index.theme", THEME);
        let dirs = vec![
            root.join("sys").to_string_lossy().into_owned(),
            root.join("user").to_string_lossy().into_owned(),
        ];
        let mut want = vec!["Ocean".to_string(), own.to_string(), "тема с пробелом".to_string()];
        want.sort();
        assert_eq!(qs::list_sound_themes(&dirs), want, "темы при имени {own}");
        let _ = fs::remove_dir_all(&root);
    }
}

/// An empty place: both sides fall back to the default theme instead of giving
/// the buyer a drop-down list without a single item.
#[test]
fn sound_themes_on_empty_dirs_fall_back() {
    let g = golden();
    let tree = SoundTree::new("empty", &[]);
    assert_eq!(
        qs::list_sound_themes(&tree.dirs()),
        str_vec(&g["sound_rules"]["themes_when_empty"]),
        "пустые каталоги"
    );
    assert_eq!(
        qs::list_sound_themes(&[]),
        vec![qs::DEFAULT_SOUND_THEME.to_string()],
        "каталогов нет вовсе"
    );
}

#[test]
fn theme_events_match_python() {
    let g = golden();
    let tree = SoundTree::from_golden("events", &g);
    let dirs = tree.dirs();
    for (theme, events) in g["sound_rules"]["theme_events"].as_object().unwrap() {
        assert_eq!(
            qs::list_theme_events(&dirs, theme),
            str_vec(events),
            "события темы {theme}"
        );
    }
}

/// Event file lookup: extension priority (`.oga` → `.ogg` → `.wav`) beats
/// nesting depth, and directory order beats extension — the system one
/// answers first, even when the user has a "better" file.
#[test]
fn find_sound_file_matches_python() {
    let g = golden();
    let tree = SoundTree::from_golden("find", &g);
    let dirs = tree.dirs();
    for (theme, probes) in g["sound_rules"]["find_file"].as_object().unwrap() {
        for (event, expected) in probes.as_object().unwrap() {
            let got = qs::find_sound_file(&dirs, theme, event).map(|p| tree.rel(&p));
            assert_eq!(
                got.as_deref(),
                expected.as_str(),
                "файл события {event} в теме {theme}"
            );
        }
    }
}

#[test]
fn showcase_matches_python() {
    let g = golden();
    for (theme, want) in g["sound_rules"]["showcase"].as_object().unwrap() {
        let events = str_vec(&g["sound_rules"]["theme_events"][theme]);
        assert_eq!(
            qs::showcase_events(&events, 5),
            str_vec(want),
            "кнопки «послушать» темы {theme}"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn event_meta_matches_python() {
    let _lang = i18n_pin::ru();
    for case in golden()["sound_event_meta"].as_array().unwrap() {
        let event = case[0].as_str().unwrap();
        let want = (
            case[1][0].as_str().unwrap().to_string(),
            case[1][1].as_str().unwrap().to_string(),
        );
        assert_eq!(qs::event_meta(event), want, "подпись события {event:?}");
    }
}

/// Size `0` in the core means "it was not in the string" — Qt substitutes its
/// default. Compare only the family, and the size where it IS in the string.
#[test]
fn parse_font_matches_python() {
    // Two entries are moved to the `font_spec_keeps_size_with_quotes` fix.
    let fixed = ["'Noto Sans 11'", "Ubuntu 11 "];
    for case in golden()["parse_font"].as_array().unwrap() {
        let raw = case[0].as_str().unwrap();
        if fixed.contains(&raw) {
            continue;
        }
        let want_family = case[1][0].as_str().unwrap();
        let want_size = case[1][1].as_i64().unwrap() as i32;
        let (family, size) = qs::parse_font_spec(raw);
        if raw.is_empty() {
            assert_eq!(family, "", "пустая строка шрифта");
            assert_eq!(size, 0);
            continue;
        }
        assert_eq!(family, want_family, "семейство из {raw:?}");
        if size != 0 {
            assert_eq!(size, want_size, "кегль из {raw:?}");
        }
    }
}

#[test]
fn font_backend_matches_python() {
    let g = golden();
    for (target, keys) in g["font_keys"].as_object().unwrap() {
        match qs::font_backend("gnome", target) {
            qs::SoundBackend::GSettings { schema, key } => {
                assert_eq!(schema, keys["gnome"][0].as_str().unwrap(), "{target}");
                assert_eq!(key, keys["gnome"][1].as_str().unwrap(), "{target}");
            }
            other => panic!("для GNOME ожидался gsettings, а не {other:?}"),
        }
        match qs::font_backend("kde", target) {
            qs::SoundBackend::KConfig { group, key, .. } => {
                assert_eq!(group, keys["kde"][0].as_str().unwrap(), "{target}");
                assert_eq!(key, keys["kde"][1].as_str().unwrap(), "{target}");
            }
            other => panic!("для KDE ожидался kdeglobals, а не {other:?}"),
        }
        match qs::font_backend("xfce", target) {
            qs::SoundBackend::Xfconf { channel, prop } => {
                assert_eq!(channel, keys["xfce"][0].as_str().unwrap(), "{target}");
                assert_eq!(prop, keys["xfce"][1].as_str().unwrap(), "{target}");
            }
            other => panic!("для XFCE ожидался xfconf, а не {other:?}"),
        }
    }
}

#[test]
fn pretty_desktop_id_matches_python() {
    for case in golden()["pretty_desktop_id"].as_array().unwrap() {
        let raw = case[0].as_str().unwrap();
        assert_eq!(
            qs::pretty_desktop_id(raw),
            case[1].as_str().unwrap(),
            "имя ярлыка {raw:?}"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn pretty_backup_ts_matches_python() {
    let _lang = i18n_pin::ru();
    for case in golden()["pretty_backup_ts"].as_array().unwrap() {
        let raw = case[0].as_str().unwrap();
        assert_eq!(
            qs::pretty_backup_ts(raw),
            case[1].as_str().unwrap(),
            "метка копии {raw:?}"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn files_word_matches_python() {
    let _lang = i18n_pin::ru();
    // The file count in the backup string is now declined by the translation
    // catalog (`Plural-Forms` in `po/ru.po`), not by our own table: check that its
    // forms give the same words as the original's `_files_word`.
    for case in golden()["files_word"].as_array().unwrap() {
        let n = case[0].as_i64().unwrap();
        let want = case[1].as_str().unwrap();
        assert_eq!(plural(n, "файл", "файла", "файлов"), want, "склонение для {n}");
        assert_eq!(
            i18n::ntr("%n file", "%n files", n as u64),
            format!("{n} {want}"),
            "форма каталога для {n}"
        );
    }
}

// ── Fixes of the original (not reflected in the golden data) ─────────────

/// Fix 1: `gsettings get` returns the value IN QUOTES, and any manual input
/// leaves a trailing space. In both cases the original lost the size and
/// returned the "default" font.
#[test]
fn font_spec_keeps_size_with_quotes() {
    assert_eq!(
        qs::parse_font_spec("'Noto Sans 11'"),
        ("Noto Sans".to_string(), 11)
    );
    assert_eq!(qs::parse_font_spec("Ubuntu 11 "), ("Ubuntu".to_string(), 11));
}

/// Fix 2: month length. `strptime` knew it, a hand-written parser might not —
/// check that «30 февраля» stays undecoded.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn pretty_backup_ts_checks_month_length() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::pretty_backup_ts("20260230_010203"), "20260230_010203");
    assert_eq!(qs::pretty_backup_ts("20260229_010203"), "20260229_010203");
    assert_eq!(qs::pretty_backup_ts("20240229_010203"), "29.02.2024 в 01:02");
}

/// Fix 3: the user's theme directory comes from `XDG_DATA_HOME`, not from a
/// hard-coded `~/.local/share`.
#[test]
fn sound_dirs_follow_xdg_data_home() {
    assert_eq!(
        qs::sound_theme_dirs("/custom/data"),
        vec![
            "/usr/share/sounds".to_string(),
            "/custom/data/sounds".to_string()
        ]
    );
}

// ── Logic that had no separate function in Python ────────────────────────

#[test]
fn sound_backend_covers_every_de() {
    // Tiling and unknown sessions must land on the common gsettings path
    // instead of staying without sound (rule #12).
    for de in ["hyprland", "niri", "sway", "unknown", "", "labwc"] {
        assert!(matches!(
            qs::sound_backend(de, qs::SoundSetting::Theme),
            qs::SoundBackend::GSettings { .. }
        ));
    }
    assert_eq!(
        qs::sound_backend("kde", qs::SoundSetting::EventSounds),
        qs::SoundBackend::KConfig {
            file: "kdeglobals".into(),
            group: "Sounds".into(),
            // KF6 Notifications reads `[Sounds] Enable` (notifybyaudio.cpp);
            // nobody read the former `EnableNotificationSounds`.
            key: "Enable".into(),
        }
    );
    // On a clean profile the key is absent, and KF6 treats it as enabled — read
    // with the same default.
    assert_eq!(
        qs::sound_read_argv_for("kde", qs::SoundSetting::EventSounds, "/usr/bin/kreadconfig6")
            .unwrap()
            .last()
            .unwrap(),
        "true"
    );
    assert_eq!(
        qs::sound_read_argv_for("kde", qs::SoundSetting::Theme, "/usr/bin/kreadconfig6")
            .unwrap()
            .last()
            .unwrap(),
        ""
    );
}

#[test]
fn sound_argv_needs_kde_tool() {
    let kde = qs::sound_backend("kde", qs::SoundSetting::Theme);
    assert!(qs::sound_read_argv(&kde, "").is_none());
    assert!(qs::sound_write_argv(&kde, "ocean", "").is_none());
    assert_eq!(
        qs::sound_write_argv(&kde, "ocean", "/usr/bin/kwriteconfig6").unwrap(),
        vec![
            "/usr/bin/kwriteconfig6",
            "--file",
            "kdeglobals",
            "--group",
            "Sounds",
            "--key",
            "Theme",
            "--notify",
            "ocean"
        ]
    );
    let gnome = qs::sound_backend("gnome", qs::SoundSetting::Theme);
    assert_eq!(
        qs::sound_read_argv(&gnome, "").unwrap(),
        vec!["gsettings", "get", "org.gnome.desktop.sound", "theme-name"]
    );
}

/// Sound theme change plan: the environment's main mechanism plus the common
/// gsettings branch for GTK programs, and a mandatory check command.
#[test]
fn sound_theme_plan_writes_both_and_verifies() {
    // KDE: kdeglobals + gsettings (GTK programs read libcanberra).
    let plan = qs::sound_theme_plan(
        "kde", "zorinos2", "/usr/bin/kwriteconfig6", "/usr/bin/kreadconfig6", true);
    assert_eq!(plan.write.len(), 2);
    assert_eq!(plan.write[0][0], "/usr/bin/kwriteconfig6");
    assert_eq!(plan.write[0].last().unwrap(), "zorinos2");
    assert_eq!(
        plan.write[1],
        vec!["gsettings", "set", "org.gnome.desktop.sound", "theme-name", "zorinos2"]
    );
    assert_eq!(plan.verify.unwrap()[0], "/usr/bin/kreadconfig6");

    // No gsettings — no auxiliary command is offered.
    let plan = qs::sound_theme_plan(
        "kde", "zorinos2", "/usr/bin/kwriteconfig6", "/usr/bin/kreadconfig6", false);
    assert_eq!(plan.write.len(), 1);

    // GNOME: the main mechanism is the common branch itself — there must be no duplicate.
    let plan = qs::sound_theme_plan("gnome", "ocean", "", "", true);
    assert_eq!(plan.write.len(), 1);
    assert_eq!(plan.write[0][2], "org.gnome.desktop.sound");

    // KDE without kwriteconfig6: nothing to write with the main mechanism, but
    // the common branch remains — the only available path, without crashing.
    let plan = qs::sound_theme_plan("kde", "ocean", "", "", true);
    assert_eq!(plan.write.len(), 1);
    assert_eq!(plan.write[0][0], "gsettings");
    assert!(plan.verify.is_none());

    // A non-KDE environment with kwriteconfig6: the theme reaches KDE/Qt programs
    // too (kdeglobals [Sounds] Theme, notifying the running ones).
    let plan = qs::sound_theme_plan(
        "cinnamon-mint", "ocean", "/usr/bin/kwriteconfig6", "/usr/bin/kreadconfig6", true);
    assert_eq!(
        plan.write,
        vec![
            svec(&["gsettings", "set", "org.cinnamon.desktop.sound", "theme-name", "ocean"]),
            svec(&["gsettings", "set", "org.gnome.desktop.sound", "theme-name", "ocean"]),
            svec(&[
                "/usr/bin/kwriteconfig6", "--file", "kdeglobals", "--group", "Sounds",
                "--key", "Theme", "--notify", "ocean",
            ]),
        ]
    );
    assert_eq!(
        plan.verify.unwrap(),
        svec(&["gsettings", "get", "org.cinnamon.desktop.sound", "theme-name"])
    );
}

fn svec(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// «Звуки событий» — the same common plan: the native key, the common GNOME and
/// KDE layer, in KDE also removal of a junk key from earlier versions.
#[test]
fn event_sounds_plan_per_desktop() {
    let kw = "/usr/bin/kwriteconfig6";
    let kr = "/usr/bin/kreadconfig6";
    let kde_enable = |v: &str| {
        svec(&[kw, "--file", "kdeglobals", "--group", "Sounds", "--key", "Enable", "--notify", v])
    };
    let drop_stale = svec(&[
        kw, "--file", "kdeglobals", "--group", "Sounds", "--key", "EnableNotificationSounds",
        "--delete",
    ]);
    let gnome = |v: &str| svec(&["gsettings", "set", "org.gnome.desktop.sound", "event-sounds", v]);

    let plan = qs::sound_plan("kde", qs::SoundSetting::EventSounds, "false", kw, kr, true);
    assert_eq!(plan.write, vec![kde_enable("false"), gnome("false"), drop_stale.clone()]);
    assert_eq!(plan.verify.unwrap()[0], kr);

    let plan = qs::sound_plan("gnome", qs::SoundSetting::EventSounds, "true", kw, kr, true);
    assert_eq!(plan.write, vec![gnome("true"), kde_enable("true"), drop_stale.clone()]);

    let plan = qs::sound_plan("mate", qs::SoundSetting::EventSounds, "false", "", "", true);
    assert_eq!(
        plan.write,
        vec![
            svec(&["gsettings", "set", "org.mate.sound", "event-sounds", "false"]),
            gnome("false"),
        ]
    );

    let plan = qs::sound_plan("xfce", qs::SoundSetting::EventSounds, "true", "", "", false);
    assert_eq!(
        plan.write,
        vec![svec(&[
            "xfconf-query", "-c", "xsettings", "-p", "/Net/EnableEventSounds", "-n", "-t", "bool", "-s",
            "true",
        ])]
    );
}

/// D6 (environment check 02.10.2026): XFCE title font on a profile where it was
/// never changed — the channel has no `/general/title_font` property, and a
/// write without `-n -t string` failed. The write must be able to create the property.
#[test]
fn xfce_font_write_creates_missing_property() {
    for (target, channel, prop) in [
        ("title", "xfwm4", "/general/title_font"),
        ("system", "xsettings", "/Gtk/FontName"),
        ("mono", "xsettings", "/Gtk/MonospaceFontName"),
    ] {
        assert_eq!(
            qs::font_write_argv("xfce", target, "Noto Sans", 12, "").unwrap(),
            svec(&["xfconf-query", "-c", channel, "-p", prop, "-n", "-t", "string", "-s", "Noto Sans 12"]),
            "{target}"
        );
    }
    let plan = qs::sound_plan("xfce", qs::SoundSetting::Theme, "ocean", "", "", false);
    assert_eq!(
        plan.write[0],
        svec(&["xfconf-query", "-c", "xsettings", "-p", "/Net/SoundThemeName", "-n", "-t", "string", "-s", "ocean"])
    );
}

/// An unset theme reaches the «— не задана —» label, a set one comes as is.
///
/// WHAT BROKE (live check of the tab 18.09). An empty answer was replaced with
/// `freedesktop`, and on a clean profile the «Звуковая тема» row showed the name
/// of a theme the person never chose; the placeholder row was unreachable.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn unset_sound_theme_reaches_the_label() {
    let _lang = i18n_pin::ru();
    // Nothing to show — said so with an empty string: the page sets the
    // placeholder row exactly by this sign.
    for raw in ["", "  \n", "''\n", "\"\"\n"] {
        assert_eq!(qs::parse_sound_theme(raw), "", "ответ {raw:?}");
    }
    // The label the page puts instead of the empty place lives in the core.
    assert_eq!(
        qs::text("snd.theme_unset", &[]).as_deref(),
        Some("— не задана —")
    );
    // A theme is set — it is shown itself, without any substitution.
    assert_eq!(qs::parse_sound_theme("'ocean'\n"), "ocean");
    assert_eq!(qs::parse_sound_theme("zorinos2\n"), "zorinos2");
}

/// Sound still needs a default — but it is taken by the SECOND path, at the
/// file lookup, not at the label: otherwise an empty theme name would mean the
/// whole themes directory, and «послушать» would play a sound of another theme.
#[test]
fn unset_theme_plays_freedesktop_and_only_it() {
    let tree = SoundTree::new(
        "effective",
        &[
            "sys/freedesktop/stereo/bell.oga",
            "sys/ocean/stereo/complete.oga",
        ],
    );
    let dirs = tree.dirs();
    assert_eq!(qs::effective_sound_theme(""), "freedesktop");
    assert_eq!(qs::effective_sound_theme("  "), "freedesktop");
    assert_eq!(qs::effective_sound_theme("ocean"), "ocean");
    assert_eq!(
        qs::list_theme_events(&dirs, ""),
        vec!["bell".to_string()],
        "события незаданной темы = события freedesktop"
    );
    assert_eq!(
        qs::find_sound_file(&dirs, "", "bell").map(|p| tree.rel(&p)),
        Some("sys/freedesktop/stereo/bell.oga".to_string())
    );
    assert!(
        qs::find_sound_file(&dirs, "", "complete").is_none(),
        "звук чужой темы при незаданной своей брать не вправе"
    );
}

#[test]
fn parse_sound_values() {
    assert_eq!(qs::parse_sound_theme("'ocean'\n"), "ocean");
    assert_eq!(qs::parse_sound_theme("  \n"), "");
    assert!(qs::parse_event_sounds("true\n"));
    assert!(qs::parse_event_sounds("'True'"));
    assert!(qs::parse_event_sounds("1"));
    assert!(!qs::parse_event_sounds("false"));
    assert!(!qs::parse_event_sounds(""));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn compliance_summary_words() {
    let _lang = i18n_pin::ru();
    let case = |pairs: &[(&str, &str)]| {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        qs::compliance_summary(&owned)
    };
    assert_eq!(case(&[]), "Для этого рабочего стола проверок пока нет");
    assert_eq!(
        case(&[("a", "ok"), ("b", "ok")]),
        "✅ Всё на месте (проверено пунктов: 2)"
    );
    assert_eq!(case(&[("a", "fixed")]), "✅ Доустановлено: 1");
    assert_eq!(
        case(&[("a", "fixed"), ("b", "failed"), ("c", "needs_action")]),
        "⚠️ Доустановлено: 1, не удалось: 1, осталось доустановить: 1"
    );
    assert_eq!(
        case(&[("a", "skipped")]),
        "✅ Проверка пакетов доступна только на Arch/CachyOS"
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn backup_row_label_reads_like_a_sentence() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        qs::backup_row_label("20260620_011005", "manual", 3),
        "📁 20.06.2026 в 01:10  ·  вручную  ·  3 файла"
    );
    assert_eq!(
        qs::backup_row_label("20260620_011005", "before_install", 1),
        "📁 20.06.2026 в 01:10  ·  авто, перед установкой  ·  1 файл"
    );
    // 11 is the very trap of Russian counting: «11 файлов», not «11 файл».
    assert_eq!(
        qs::backup_row_label("20260620_011005", "manual", 11),
        "📁 20.06.2026 в 01:10  ·  вручную  ·  11 файлов"
    );
    assert_eq!(qs::backup_reason_label("своё"), "своё");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn de_display_never_shows_unknown() {
    let _lang = i18n_pin::ru();
    // The desktop name as on the «Рабочие столы» tab, not an abbreviation.
    assert_eq!(qs::de_display("kde", "KDE", ""), "KDE Plasma");
    assert_eq!(qs::de_display("unknown", "niri:wlroots", ""), "NIRI");
    assert_eq!(qs::de_display("unknown", "", "sway"), "SWAY");
    // Fix (rule #13): the fallback name was «ВАШ WM» — an abbreviation from the
    // developer's vocabulary that tells the buyer nothing.
    assert_eq!(qs::de_display("unknown", "", ""), "ВАШ РАБОЧИЙ СТОЛ");
    assert_eq!(qs::de_display("", "", ""), "ВАШ РАБОЧИЙ СТОЛ");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn youtube_lists_are_paired() {
    let _lang = i18n_pin::ru();
    let by_label: BTreeMap<String, &str> = qs::yt_qualities().into_iter().collect();
    assert_eq!(by_label["4K"], "4k");
    assert_eq!(by_label["Лучшее"], "best");
    // Fix (rule #13): it was «Стриминг» — a calque, not a Russian word.
    assert_eq!(qs::yt_modes()[0], ("Онлайн".to_string(), "stream"));
    assert_eq!(qs::yt_modes()[1], ("Скачать".to_string(), "download"));
}

#[test]
fn font_gtk_sync_only_for_system() {
    assert!(qs::font_syncs_to_gtk("system"));
    assert!(!qs::font_syncs_to_gtk("mono"));
    assert!(!qs::font_syncs_to_gtk("title"));
    assert_eq!(qs::gnome_font_spec("Noto Sans", 11), "Noto Sans 11");
    // ⚠️ The difference from the Python original is deliberate: Qt 6 reads its
    // entry `Noto Sans,11,-1,5,Regular,0,0,0,0,0` with weight 100 (Thin) — ten
    // fields are parsed by the old scale, "Regular" → 0. The Qt 6 entry of
    // sixteen fields gives the normal weight 400, as in Plasma 6 itself.
    assert_eq!(
        qs::kde_font_spec("Noto Sans", 11),
        "Noto Sans,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"
    );
    // And the parser understands both entries — old configs are read as before.
    assert_eq!(
        qs::parse_font_spec("Noto Sans,11,-1,5,Regular,0,0,0,0,0"),
        ("Noto Sans".to_string(), 11)
    );
    assert_eq!(
        qs::parse_font_spec(&qs::kde_font_spec("Noto Sans", 11)),
        ("Noto Sans".to_string(), 11)
    );
}

/// What is read and written in each environment: the native store, companions,
/// companion commands. Key sources are in the `font_backend` and `font_plan`
/// docs (gsettings schemas, Cinnamon, MATE, Budgie, COSMIC, LXQt, qt6ct sources).
#[test]
fn font_plan_table_per_desktop() {
    use qs::{FontFile, FontStore, SoundBackend};
    let gs = |schema: &str, key: &str| {
        FontStore::Cmd(SoundBackend::GSettings {
            schema: schema.to_string(),
            key: key.to_string(),
        })
    };
    let kc = |group: &str, key: &str| {
        FontStore::Cmd(SoundBackend::KConfig {
            file: "kdeglobals".to_string(),
            group: group.to_string(),
            key: key.to_string(),
        })
    };
    let xf = |channel: &str, prop: &str| {
        FontStore::Cmd(SoundBackend::Xfconf {
            channel: channel.to_string(),
            prop: prop.to_string(),
        })
    };
    let cosmic = |key: &str| FontStore::File(FontFile::CosmicTk { key: key.to_string() });
    let lxqt = |key: &str| {
        FontStore::File(FontFile::QtIni {
            path: "lxqt/lxqt.conf".to_string(),
            group: "Qt".to_string(),
            key: key.to_string(),
            create: true,
        })
    };
    let qt6ct = |key: &str| {
        FontStore::File(FontFile::QtIni {
            path: "qt6ct/qt6ct.conf".to_string(),
            group: "Fonts".to_string(),
            key: key.to_string(),
            create: false,
        })
    };
    const IFACE: &str = "org.gnome.desktop.interface";
    let g_sys = || gs(IFACE, "font-name");
    let g_mono = || gs(IFACE, "monospace-font-name");
    let g_title = || gs("org.gnome.desktop.wm.preferences", "titlebar-font");

    // (environment, target, native store, companions)
    let table: Vec<(&str, &str, FontStore, Vec<FontStore>)> = vec![
        ("kde", "system", kc("General", "font"), vec![g_sys()]),
        ("kde", "mono", kc("General", "fixed"), vec![g_mono()]),
        ("kde", "title", kc("WM", "activeFont"), vec![]),
        ("xfce", "system", xf("xsettings", "/Gtk/FontName"), vec![g_sys(), qt6ct("general")]),
        ("xfce", "mono", xf("xsettings", "/Gtk/MonospaceFontName"), vec![g_mono(), qt6ct("fixed")]),
        ("xfce", "title", xf("xfwm4", "/general/title_font"), vec![]),
        ("gnome", "system", g_sys(), vec![qt6ct("general")]),
        ("gnome", "mono", g_mono(), vec![qt6ct("fixed")]),
        ("gnome", "title", g_title(), vec![]),
        ("budgie", "system", g_sys(), vec![qt6ct("general")]),
        ("budgie", "mono", g_mono(), vec![qt6ct("fixed")]),
        ("budgie", "title", g_title(), vec![]),
        (
            "cinnamon-mint",
            "system",
            gs("org.cinnamon.desktop.interface", "font-name"),
            vec![g_sys(), qt6ct("general")],
        ),
        ("cinnamon-mint", "mono", g_mono(), vec![qt6ct("fixed")]),
        (
            "cinnamon-mint",
            "title",
            gs("org.cinnamon.desktop.wm.preferences", "titlebar-font"),
            vec![],
        ),
        ("mate", "system", gs("org.mate.interface", "font-name"), vec![g_sys(), qt6ct("general")]),
        (
            "mate",
            "mono",
            gs("org.mate.interface", "monospace-font-name"),
            vec![g_mono(), qt6ct("fixed")],
        ),
        ("mate", "title", gs("org.mate.Marco.general", "titlebar-font"), vec![]),
        ("cosmic", "system", cosmic("interface_font"), vec![qt6ct("general")]),
        ("cosmic", "mono", cosmic("monospace_font"), vec![qt6ct("fixed")]),
        ("lxqt", "system", lxqt("font"), vec![g_sys()]),
        ("lxqt", "mono", lxqt("fixedFont"), vec![g_mono()]),
        ("sway", "system", g_sys(), vec![qt6ct("general")]),
        ("hyprland", "mono", g_mono(), vec![qt6ct("fixed")]),
        ("niri", "system", g_sys(), vec![qt6ct("general")]),
        ("", "system", g_sys(), vec![qt6ct("general")]),
    ];
    for (de, target, primary, also) in table {
        let plan = qs::font_plan(de, target);
        assert_eq!(plan.primary, primary, "{de}/{target}: родное хранилище");
        assert_eq!(plan.also, also, "{de}/{target}: спутники");
        assert_eq!(plan.gtk_ini, target == "system", "{de}/{target}: settings.ini");
        // The native command store matches what font_backend returns
        // (the Python bridge still uses it).
        if let FontStore::Cmd(backend) = &plan.primary {
            assert_eq!(*backend, qs::font_backend(de, target), "{de}/{target}");
        }
    }

    // Title: together with titlebar-font, titlebar-uses-system-font of the same
    // schema is cleared — otherwise the font key is ignored.
    for (de, schema) in [
        ("gnome", "org.gnome.desktop.wm.preferences"),
        ("budgie", "org.gnome.desktop.wm.preferences"),
        ("cinnamon-mint", "org.cinnamon.desktop.wm.preferences"),
        ("mate", "org.mate.Marco.general"),
    ] {
        assert_eq!(
            qs::font_plan(de, "title").extra,
            vec![vec![
                "gsettings".to_string(),
                "set".to_string(),
                schema.to_string(),
                "titlebar-uses-system-font".to_string(),
                "false".to_string(),
            ]],
            "{de}"
        );
    }
    for de in ["kde", "xfce"] {
        assert!(qs::font_plan(de, "title").extra.is_empty(), "{de}");
    }
    for de in ["gnome", "cinnamon-mint", "mate", "cosmic", "lxqt", "kde", "xfce"] {
        for target in ["system", "mono"] {
            assert!(qs::font_plan(de, target).extra.is_empty(), "{de}/{target}");
        }
    }
}

/// The title font is offered only where there is somewhere to write it;
/// «маленький», «панели инструментов» and «меню» everywhere (outside KDE they
/// are carried by Rexon's common layer).
#[test]
fn font_title_offered_only_where_a_wm_reads_it() {
    for de in ["kde", "gnome", "budgie"] {
        assert_eq!(
            qs::font_targets_for(de),
            vec!["system", "mono", "small", "toolbar", "menu", "title", "document"],
            "{de}"
        );
    }
    // Desktop icon labels — only where the desktop has a key.
    for de in ["xfce", "cinnamon-mint", "cinnamon", "mate"] {
        assert_eq!(
            qs::font_targets_for(de),
            vec!["system", "mono", "small", "toolbar", "menu", "title", "document", "desktop"],
            "{de}"
        );
    }
    for de in ["cosmic", "sway", "hyprland", "niri", "deepin", ""] {
        assert_eq!(qs::font_targets_for(de), vec!["system", "mono", "small", "toolbar", "menu", "document"], "{de}");
    }
    // LXQt: title only with a recognized window manager, icon labels
    // — from pcmanfm-qt.
    assert_eq!(qs::font_targets_for("lxqt"), vec!["system", "mono", "small", "toolbar", "menu", "document", "desktop"]);
}

/// «Маленький», «панели инструментов», «меню»: in KDE the native kdeglobals
/// keys, in all other environments Rexon's common layer: our block in GTK 3
/// gtk.css (it is also the native one — the font is read back from it) and
/// GTK 4, the qt6ct/qt5ct style sheet (except LXQt, whose platform theme does
/// not read style sheets) and kdeglobals for KDE programs. Buttons are enabled
/// everywhere.
#[test]
fn extra_fonts_reach_every_desktop() {
    use qs::{FontFile, FontStore, SoundBackend};
    let css = |v: &str, t: &str| FontStore::File(FontFile::GtkCss { version: v.to_string(), target: t.to_string() });
    let qss = |tool: &str, t: &str| FontStore::File(FontFile::QtCtQss { tool: tool.to_string(), target: t.to_string() });
    for (target, key) in [("small", "smallestReadableFont"), ("toolbar", "toolBarFont"), ("menu", "menuFont")] {
        let kde = FontStore::Cmd(SoundBackend::KConfig {
            file: "kdeglobals".to_string(),
            group: "General".to_string(),
            key: key.to_string(),
        });
        let plan = qs::font_plan("kde", target);
        assert_eq!(plan.primary, kde, "{target}");
        assert_eq!(plan.also, vec![css("3.0", target), css("4.0", target)], "kde/{target}");
        assert!(plan.extra.is_empty() && !plan.gtk_ini, "{target}");
        for de in ["gnome", "xfce", "cinnamon-mint", "mate", "budgie", "pantheon", "cosmic", "deepin", "hyprland", "sway", "niri", ""] {
            assert!(qs::font_target_available(de, target), "{de}/{target}");
            let plan = qs::font_plan(de, target);
            assert_eq!(plan.primary, css("3.0", target), "{de}/{target}");
            assert_eq!(
                plan.also,
                vec![css("4.0", target), qss("qt6ct", target), qss("qt5ct", target), kde.clone()],
                "{de}/{target}"
            );
            assert!(plan.extra.is_empty() && !plan.gtk_ini, "{de}/{target}");
        }
        let plan = qs::font_plan("lxqt", target);
        assert_eq!(plan.primary, css("3.0", target), "lxqt/{target}");
        assert_eq!(plan.also, vec![css("4.0", target), kde.clone()], "lxqt/{target}");
    }
    assert_eq!(
        qs::font_targets_shown("gnome"),
        vec![
            ("system", true),
            ("mono", true),
            ("small", true),
            ("toolbar", true),
            ("menu", true),
            ("title", true),
            ("document", true),
            ("desktop", false),
        ]
    );
    // The title is still not shown where there is nowhere to write;
    // desktop icon labels are visible but disabled.
    assert!(qs::font_targets_shown("sway").iter().all(|(t, ok)| *t != "title" && (*ok || *t == "desktop")));
    assert!(qs::font_targets_shown("kde").iter().all(|(t, ok)| *ok != (*t == "desktop")));
    assert!(qs::font_targets_shown("mate").iter().all(|(_, ok)| *ok));
}

/// The "what it affects" label is honest per environment.
#[test]
fn extra_fonts_scope_is_honest() {
    let _lang = i18n_pin::en();
    let gnome = qs::extra_fonts_scope_note("gnome");
    assert!(gnome.contains("gtk.css") && gnome.contains("qt6ct") && gnome.contains("GNOME Shell"), "{gnome}");
    let cosmic = qs::extra_fonts_scope_note("cosmic");
    assert!(cosmic.contains("In COSMIC apps only the interface and monospace fonts change, without the size"), "{cosmic}");
    let lxqt = qs::extra_fonts_scope_note("lxqt");
    assert!(lxqt.contains("LXQt's Qt apps don't take them") && !lxqt.contains("qt6ct"), "{lxqt}");
    let kde = qs::extra_fonts_scope_note("kde");
    assert!(kde.contains("GTK apps") && !kde.contains("qt6ct"), "{kde}");
    for de in ["gnome", "cosmic", "lxqt", "kde", "xfce", ""] {
        assert!(qs::extra_fonts_scope_note(de).contains("Electron"), "{de}");
    }
}

/// The gtk.css block: foreign rules and `@import` stay in place, our block is
/// single and at the end, three fonts coexist and are read back; a broken block
/// is not touched; a link to a theme is replaced by a file with that theme's `@import`.
#[test]
fn extra_fonts_gtk_css_block() {
    use qs::{FontFile, FontFileIo};
    struct Fake(Vec<(String, String)>, Option<String>);
    impl FontFileIo for Fake {
        fn read(&self, rel: &str) -> Option<String> {
            self.0.iter().find(|(p, _)| p == rel).map(|(_, t)| t.clone())
        }
        fn link_target(&self, _rel: &str) -> Option<String> {
            self.1.clone()
        }
    }
    let menu = FontFile::GtkCss { version: "3.0".to_string(), target: "menu".to_string() };
    let small = FontFile::GtkCss { version: "3.0".to_string(), target: "small".to_string() };
    let before = "@import 'colors.css';\n\nbutton { color: red; }\n";
    let text = menu.new_text(Some(before), "Noto Sans", 11);
    assert!(text.starts_with("@import 'colors.css';\n\nbutton { color: red; }\n\n/* >>> Rexon: fonts"), "{text}");
    assert!(text.contains("menubar, menubar > menuitem, menu, menuitem, popover.menu, popover.menu modelbutton {\n  font-family: \"Noto Sans\";\n  font-size: 11pt;\n}"), "{text}");
    let text = small.new_text(Some(&text), "Cantarell", 8);
    let text = menu.new_text(Some(&text), "Fira Sans", 12);
    assert_eq!(text.matches("/* >>> Rexon: fonts").count(), 1, "{text}");
    assert_eq!(qs::parse_font_spec(&menu.read_spec(&text)), ("Fira Sans".to_string(), 12));
    assert_eq!(qs::parse_font_spec(&small.read_spec(&text)), ("Cantarell".to_string(), 8));
    assert!(text.contains("statusbar, .caption, .caption-heading {"), "{text}");
    assert!(text.trim_end().ends_with("/* <<< Rexon: fonts */"), "{text}");
    assert!(text.contains("button { color: red; }"));
    // Quotes in the name are escaped.
    let odd = menu.new_text(None, "Odd \"Name\"", 10);
    assert!(odd.contains("font-family: \"Odd \\\"Name\\\"\";"), "{odd}");
    // GTK 4 — its own nodes.
    let gtk4 = FontFile::GtkCss { version: "4.0".to_string(), target: "toolbar".to_string() };
    assert!(gtk4.new_text(None, "Inter", 10).contains(".toolbar, .toolbar button {"));
    // A broken block: not touched.
    let broken = "a {}\n/* >>> Rexon: fonts (managed by Rexon, edits are overwritten) */\nb {}\n";
    assert_eq!(menu.try_new_text(Some(broken), "X", 10), None);
    assert_eq!(menu.file_writes("/c", &Fake(vec![("gtk-3.0/gtk.css".into(), broken.into())], None), "X", 10), None);
    // A link to a theme: a regular file with the theme's @import and our block.
    let w = menu
        .file_writes("/c", &Fake(vec![], Some("/home/u/.themes/My Theme/gtk-3.0/gtk.css".into())), "X", 10)
        .unwrap();
    assert_eq!(w.len(), 1);
    assert!(w[0].replace_link);
    assert!(w[0].text.starts_with("@import url(\"file:///home/u/.themes/My%20Theme/gtk-3.0/gtk.css\");\n"), "{}", w[0].text);
}

/// Style of the menu, toolbar and small fonts in our block: the "write → read"
/// round trip (Bold Italic, Semi-Bold, Italic), regular after bold — without
/// `font-weight`/`font-style` (the theme's weight is not overridden), an
/// unselected style keeps the previous one, markers of earlier versions
/// without a style field read as the regular font. «Изменить все шрифты»
/// carries the read style further — into the block and into kdeglobals.
#[test]
fn extra_fonts_keep_face() {
    use qs::{FontFace, FontFile, FontStore, SoundBackend};
    let norm = |(f, s, face): (String, i32, FontFace)| (f, s, if face.weight == 0 { 400 } else { face.weight }, face.italic);
    let bold_italic = FontFace { weight: 700, italic: true };
    for file in [
        FontFile::GtkCss { version: "3.0".to_string(), target: "menu".to_string() },
        FontFile::GtkCss { version: "4.0".to_string(), target: "toolbar".to_string() },
        FontFile::QtCtQss { tool: "qt6ct".to_string(), target: "small".to_string() },
    ] {
        let mut text: Option<String> = None;
        for face in [
            bold_italic,
            FontFace { weight: 600, italic: false },
            FontFace { weight: 400, italic: true },
            FontFace { weight: 400, italic: false },
        ] {
            let written = file.try_new_text_face(text.as_deref(), "Noto Sans", 9, face).unwrap();
            assert_eq!(
                norm(qs::parse_font_spec_face(&file.read_spec(&written))),
                ("Noto Sans".to_string(), 9, face.weight, face.italic),
                "{file:?} {face:?}: {written}"
            );
            assert_eq!(written.contains("font-weight:"), face.weight != 400, "{written}");
            assert_eq!(written.contains("font-style: italic;"), face.italic, "{written}");
            text = Some(written);
        }
        let bold = file.try_new_text_face(text.as_deref(), "Noto Sans", 9, bold_italic).unwrap();
        assert_eq!(file.read_spec(&bold), "Noto Sans Bold Italic 9");
        assert!(bold.contains("/* rexon:font ") && bold.contains(" 9 face=700,1 Noto Sans */"), "{bold}");
        assert!(bold.contains("  font-size: 9pt;\n  font-weight: 700;\n  font-style: italic;\n}"), "{bold}");
        // No style selected (an old call without the dialog) — the previous one stays.
        let kept = file.try_new_text(Some(&bold), "Inter", 10).unwrap();
        assert_eq!(file.read_spec(&kept), "Inter Bold Italic 10");
        // «Изменить все шрифты»: the style is read from the block and written
        // with the new family — both into the block and into kdeglobals.
        let (_, _, face) = qs::parse_font_spec_face(&file.read_spec(&bold));
        let changed = file.try_new_text_face(Some(&bold), "Fira Sans", 11, face).unwrap();
        assert_eq!(file.read_spec(&changed), "Fira Sans Bold Italic 11");
        let kde = FontStore::Cmd(SoundBackend::KConfig {
            file: "kdeglobals".to_string(),
            group: "General".to_string(),
            key: "menuFont".to_string(),
        });
        let argv = qs::font_store_write_argv_face(&kde, "Fira Sans", 11, face, "kwriteconfig6").unwrap();
        assert_eq!(argv.last().map(String::as_str), Some("Fira Sans,11,-1,5,700,1,0,0,0,0,0,0,0,0,0,1"));
    }
    // QSS: properties that Qt style sheets understand.
    let qss = FontFile::QtCtQss { tool: "qt6ct".to_string(), target: "toolbar".to_string() };
    let t = qss.try_new_text_face(None, "Inter", 10, FontFace { weight: 600, italic: true }).unwrap();
    assert!(t.contains("QToolButton {\n  font-family: \"Inter\";\n  font-size: 10pt;\n  font-weight: 600;\n  font-style: italic;\n}"), "{t}");
    // A marker of an earlier version — without a style; the new one next to it — with one.
    let old = "/* >>> Rexon: fonts (managed by Rexon, edits are overwritten) */\n/* rexon:font menu 9 Noto Sans */\nmenu {}\n/* rexon:font small 8 face=300,0 Cantarell Light */\n/* <<< Rexon: fonts */\n";
    let fonts = qs::extra_fonts_parse(old);
    assert_eq!(fonts.len(), 2);
    assert_eq!((fonts[0].family.as_str(), fonts[0].size, fonts[0].face), ("Noto Sans", 9, FontFace::default()));
    assert_eq!((fonts[1].family.as_str(), fonts[1].face), ("Cantarell Light", FontFace { weight: 300, italic: false }));
    let menu = FontFile::GtkCss { version: "3.0".to_string(), target: "menu".to_string() };
    assert_eq!(menu.read_spec(old), "Noto Sans 9");
    // A corrupted style field is part of the family, as with old markers.
    assert_eq!(qs::extra_fonts_parse("/* >>> Rexon: fonts */\n/* rexon:font menu 9 face=x Odd */\n/* <<< Rexon: fonts */")[0].family, "face=x Odd");
}

/// qt6ct: the style sheet is written only if qt6ct is set up (`qt6ct.conf`
/// exists); its path is appended to `[Interface] stylesheets` after foreign ones, not twice.
#[test]
fn extra_fonts_qtct_stylesheet() {
    use qs::{FontFile, FontFileIo};
    struct Fake(Vec<(String, String)>);
    impl FontFileIo for Fake {
        fn read(&self, rel: &str) -> Option<String> {
            self.0.iter().find(|(p, _)| p == rel).map(|(_, t)| t.clone())
        }
        fn link_target(&self, _rel: &str) -> Option<String> {
            None
        }
    }
    let menu = FontFile::QtCtQss { tool: "qt6ct".to_string(), target: "menu".to_string() };
    assert_eq!(menu.rel_path(), "qt6ct/qss/rexon-fonts.qss");
    assert_eq!(menu.file_writes("/c", &Fake(vec![]), "Noto Sans", 10), None, "qt6ct не настроен");
    let conf = "[Appearance]\nstyle=Fusion\n\n[Interface]\nstylesheets=/usr/share/qt6ct/qss/fusion-fixes.qss\n";
    let w = menu.file_writes("/c", &Fake(vec![("qt6ct/qt6ct.conf".into(), conf.into())]), "Noto Sans", 10).unwrap();
    assert_eq!(w.len(), 2);
    assert!(w[0].text.contains("QMenuBar, QMenu {\n  font-family: \"Noto Sans\";\n  font-size: 10pt;\n}"), "{}", w[0].text);
    assert!(!w[0].text.contains("//"), "qt6ct вырезает «//…» до конца строки");
    assert_eq!(
        qs::qtct_conf_stylesheets(&w[1].text),
        vec!["/usr/share/qt6ct/qss/fusion-fixes.qss".to_string(), "/c/qt6ct/qss/rexon-fonts.qss".to_string()]
    );
    assert!(w[1].text.contains("style=Fusion"));
    // Already listed — conf is not rewritten; the second font coexists with the first.
    let fs = Fake(vec![("qt6ct/qt6ct.conf".into(), w[1].text.clone()), ("qt6ct/qss/rexon-fonts.qss".into(), w[0].text.clone())]);
    let tool = FontFile::QtCtQss { tool: "qt6ct".to_string(), target: "toolbar".to_string() };
    let w2 = tool.file_writes("/c", &fs, "Inter", 9).unwrap();
    assert_eq!(w2.len(), 1);
    assert!(w2[0].text.contains("QToolButton {") && w2[0].text.contains("QMenuBar, QMenu {"), "{}", w2[0].text);
    // An empty QSettings list and paths with a comma.
    assert_eq!(qs::qtct_conf_with_stylesheet("[Interface]\nstylesheets=@Invalid()\n", "/a.qss").map(|t| qs::qtct_conf_stylesheets(&t)), Some(vec!["/a.qss".to_string()]));
    let t = qs::qtct_conf_with_stylesheet("[Interface]\nstylesheets=\"/x,y.qss\", /z.qss\n", "/a.qss").unwrap();
    assert_eq!(qs::qtct_conf_stylesheets(&t), vec!["/x,y.qss".to_string(), "/z.qss".to_string(), "/a.qss".to_string()]);
}

/// Writing to each store: command argv and file text.
#[test]
fn font_store_writes() {
    use qs::{FontFile, FontStore};
    // gsettings — "Family Size", Cinnamon's native key.
    let plan = qs::font_plan("cinnamon-mint", "system");
    assert_eq!(
        qs::font_store_write_argv(&plan.primary, "Noto Sans", 11, ""),
        Some(vec![
            "gsettings".to_string(),
            "set".to_string(),
            "org.cinnamon.desktop.interface".to_string(),
            "font-name".to_string(),
            "Noto Sans 11".to_string(),
        ])
    );
    // kdeglobals — a QFont entry; without kwriteconfig6 there is nothing to write with.
    let plan = qs::font_plan("kde", "system");
    assert_eq!(
        qs::font_store_write_argv(&plan.primary, "Noto Sans", 11, "/usr/bin/kwriteconfig6")
            .unwrap()
            .last()
            .unwrap(),
        "Noto Sans,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"
    );
    assert_eq!(qs::font_store_write_argv(&plan.primary, "Noto Sans", 11, ""), None);
    // A file is not written by a command.
    let cosmic = FontStore::File(FontFile::CosmicTk { key: "interface_font".to_string() });
    assert_eq!(qs::font_store_write_argv(&cosmic, "Noto Sans", 11, ""), None);

    // COSMIC: the FontConfig RON structure, as cosmic-config writes it.
    let FontStore::File(file) = qs::font_plan("cosmic", "mono").primary else {
        panic!("у COSMIC родное хранилище — файл");
    };
    assert_eq!(file.rel_path(), "cosmic/com.system76.CosmicTk/v1/monospace_font");
    let text = file.new_text(None, "Fira Mono", 12);
    assert_eq!(
        text,
        "// Rexon: font (managed by Rexon)\n(\n    family: \"Fira Mono\",\n    weight: Normal,\n    stretch: Normal,\n    style: Normal,\n)"
    );
    assert_eq!(file.read_spec(&text), "Fira Mono");
    assert_eq!(qs::font_value_text(&file.read_spec(&text)).contains("Fira Mono"), true);
    assert!(file.may_create());

    // LXQt: [Qt] font in lxqt.conf, a quoted QFont string; neighbouring keys and
    // sections are not touched.
    let FontStore::File(file) = qs::font_plan("lxqt", "system").primary else {
        panic!("у LXQt родное хранилище — файл");
    };
    assert_eq!(file.rel_path(), "lxqt/lxqt.conf");
    let before = "[General]\ntheme=frost\n\n[Qt]\nfont=\"Sans,11,-1,5,50,0,0,0,0,0\"\nstyle=Fusion\n";
    let text = file.new_text(Some(before), "Noto Sans", 10);
    assert!(text.contains("font=\"Noto Sans,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\""), "{text}");
    assert!(text.contains("theme=frost") && text.contains("style=Fusion"), "{text}");
    assert!(!text.contains("Sans,11,"), "{text}");
    assert_eq!(qs::parse_font_spec(&file.read_spec(&text)), ("Noto Sans".to_string(), 10));
    // fixedFont is not confused with font.
    let FontStore::File(mono) = qs::font_plan("lxqt", "mono").primary else {
        panic!("у LXQt родное хранилище — файл");
    };
    let text = mono.new_text(Some(&text), "Hack", 9);
    assert!(text.contains("font=\"Noto Sans,10,"), "{text}");
    assert!(text.contains("fixedFont=\"Hack,9,"), "{text}");
    assert_eq!(qs::parse_font_spec(&mono.read_spec(&text)), ("Hack".to_string(), 9));

    // qt6ct — only into an already existing file.
    let plan = qs::font_plan("sway", "system");
    let Some(FontStore::File(qt6ct)) = plan.also.last() else {
        panic!("последний спутник — qt6ct");
    };
    assert!(!qt6ct.may_create());
    assert_eq!(qt6ct.rel_path(), "qt6ct/qt6ct.conf");
    let text = qt6ct.new_text(Some("[Appearance]\nstyle=Fusion\n"), "Noto Sans", 10);
    assert!(text.contains("[Fonts]\ngeneral=\"Noto Sans,10,-1,5,400,"), "{text}");
}

/// The file dialog filter is built from the same lists as format detection.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn dialog_filter_lists_every_known_ext() {
    let _lang = i18n_pin::ru();
    // The set is pinned as a string: iterating over the list itself would be a
    // tautology and would miss a lost extension (verified by the ".tif forgotten"
    // mutation). The dialog opens on the button's "native" group — it is first,
    // as it was before the other groups appeared.
    assert!(qs::wallpaper_dialog_filter("static").starts_with(
        "Изображения (*.jpg *.jpeg *.png *.webp *.bmp *.tiff *.tif);;"
    ));
    assert!(qs::wallpaper_dialog_filter("video").starts_with(
        "Видео (*.mp4 *.webm *.mkv *.avi *.mov *.m4v *.wmv *.mpg *.mpeg *.gif);;"
    ));
    let images = qs::wallpaper_dialog_filter("static");
    for ext in qs::STATIC_EXTS {
        assert!(images.contains(&format!("*.{ext}")), "{images} без {ext}");
        assert_eq!(qs::detect_wallpaper_format(&format!("x.{ext}")), "static");
    }
    let video = qs::wallpaper_dialog_filter("video");
    for ext in qs::VIDEO_EXTS {
        assert!(video.contains(&format!("*.{ext}")), "{video} без {ext}");
        assert_eq!(qs::detect_wallpaper_format(&format!("x.{ext}")), "video");
    }
}

/// Flash is reachable from the file dialog, not only from an `.rxn` manifest.
///
/// The engine handles the `swf` format (`WallpaperApplier.apply`, `case "swf"`),
/// but the tab did not know about it: detection returned `"static"`, the filter
/// did not show `.swf`, and the clip the person picked went to the static image display.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn swf_is_reachable_from_file_dialog() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::detect_wallpaper_format("/home/w/ролик.swf"), "swf");
    assert_eq!(qs::detect_wallpaper_format("/home/w/РОЛИК.SWF"), "swf");
    // The tab has no buttons for Flash, so .swf must be reachable from under ANY
    // existing button — otherwise the format still has no entry point. The group
    // order does not change: the dialog still opens on the "native" group of the
    // pressed button.
    for kind in ["static", "video", "auto"] {
        let f = qs::wallpaper_dialog_filter(kind);
        assert!(f.contains("Flash (*.swf)"), "{kind}: {f}");
        assert!(f.contains("Все обои ("), "{kind}: {f}");
    }
    assert!(qs::wallpaper_dialog_filter("swf").starts_with("Flash (*.swf);;"));
    // No group appears twice.
    let f = qs::wallpaper_dialog_filter("static");
    assert_eq!(f.matches("Изображения (").count(), 1, "{f}");
}

/// The "choose wallpaper" dialog shows ALL applicable files in one list.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn auto_dialog_shows_every_wallpaper_kind() {
    let _lang = i18n_pin::ru();
    let f = qs::wallpaper_dialog_filter("auto");
    assert!(f.starts_with("Все обои ("), "{f}");
    for ext in qs::all_wallpaper_exts() {
        assert!(f.contains(&format!("*.{ext}")), "{f} без {ext}");
        // Every extension from the filter can be detected by the tab into a format
        // the engine accepts (not "static by default" for .swf).
        let fmt = qs::detect_wallpaper_format(&format!("x.{ext}"));
        assert!(
            ["static", "video", "swf"].contains(&fmt),
            "{ext} → {fmt}"
        );
    }
    // The groups for those who want to narrow down are still there.
    for group in ["Изображения (", "Видео (", "Flash ("] {
        assert!(f.contains(group), "{f} без группы {group}");
    }
    assert!(qs::wallpaper_dialog_title("auto").contains("обои"));
    assert!(qs::wallpaper_dialog_title("swf").contains("Flash"));
}

// ── Tab lists and texts ─────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn fm_choices_match_python() {
    let _lang = i18n_pin::ru();
    let g = golden();
    let want: Vec<(String, String)> = g["fm_options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c[0].as_str().unwrap().to_string(),
                c[1].as_str().unwrap().to_string(),
            )
        })
        .collect();
    // Fix (rule #13): the item «Автоматически (определять по DE)» named the
    // desktop with an abbreviation from the developer's vocabulary. Compare with
    // the golden data, substituting the new wording — the other items must match
    // word for word.
    let want: Vec<(String, String)> = want
        .into_iter()
        .map(|(short, label)| {
            if short == "auto" {
                assert_eq!(label, "⚙️  Автоматически (определять по DE)");
                (short, "⚙️  Автоматически (по рабочему столу)".to_string())
            } else {
                (short, label)
            }
        })
        .collect();
    assert_eq!(qs::fm_choices(), want, "пункты списка файловых менеджеров");
}

/// The set of items must match the set the core knows: a new file manager
/// added to `appdefaults` must not get lost on its way to the dialog (rule #12).
#[test]
fn fm_display_covers_every_known_manager() {
    use std::collections::BTreeSet;
    let shown: BTreeSet<&str> = qs::FM_DISPLAY_ORDER.iter().copied().collect();
    let known: BTreeSet<&str> = rexon_core::appdefaults::FM_SHORT_NAMES
        .iter()
        .copied()
        .collect();
    assert_eq!(shown, known, "список в окне разошёлся со списком ядра");
    for short in qs::FM_DISPLAY_ORDER {
        let (icon, label) = qs::fm_label(short);
        assert!(!label.is_empty(), "{short} без подписи");
        assert!(!icon.is_empty(), "{short} без значка");
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn font_target_texts_match_python() {
    let _lang = i18n_pin::ru();
    let g = golden();
    // Fix (rule #13): two hints were written for a developer — «Шрифт UI
    // приложений…» and a list of window managers in parentheses. The button
    // labels must still match the original word for word.
    let tip_fixes: &[(&str, &str)] = &[
        ("system", "Шрифт UI приложений (заголовки, кнопки, текст)"),
        ("title", "Шрифт заголовков окон (KWin/Mutter/Xfwm)"),
    ];
    for case in g["font_targets"].as_array().unwrap() {
        let target = case[0].as_str().unwrap();
        let (button, tip, _title) = qs::font_target_texts(target);
        assert_eq!(button, case[1].as_str().unwrap(), "кнопка {target}");
        match tip_fixes.iter().find(|(t, _)| *t == target) {
            Some((_, was)) => {
                assert_eq!(case[2].as_str().unwrap(), *was, "эталон {target}");
                assert_ne!(tip, *was, "{target}: починка подсказки не применена");
            }
            None => assert_eq!(tip, case[2].as_str().unwrap(), "подсказка {target}"),
        }
    }
    for (target, title) in g["font_titles"].as_object().unwrap() {
        let (_b, _t, got) = qs::font_target_texts(target);
        assert_eq!(got, title.as_str().unwrap(), "заголовок окна {target}");
    }
    // The original's three fonts remain, in the same order; «маленький»,
    // «панели инструментов» and «меню» are added to them (as on the KDE page).
    let listed: Vec<&str> = g["font_targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c[0].as_str().unwrap())
        .collect();
    // The document font and desktop icon labels were added later,
    // following the GNOME, Cinnamon and MATE pages.
    let kept: Vec<&str> = qs::FONT_TARGETS
        .iter()
        .copied()
        .filter(|t| !qs::EXTRA_FONT_TARGETS.contains(t) && !["document", "desktop"].contains(t))
        .collect();
    assert_eq!(listed, kept);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn wallpaper_status_matches_python() {
    let _lang = i18n_pin::ru();
    for case in golden()["wallpaper_status"].as_array().unwrap() {
        let st = &case[0];
        let running = st["running"].as_bool().unwrap_or(false);
        let format = st.get("format").and_then(|v| v.as_str());
        let path = st.get("path").and_then(|v| v.as_str());
        // Fix (rule #13): the original inserted the engine's service word into the
        // string — «Активны живые обои: video». The human-readable name comes from the
        // common `wallpaper_engine::format_label`, so the golden string is compared
        // with the same name substituted into it.
        let want = case[1].as_str().unwrap().to_string();
        let want = match format.filter(|f| !f.is_empty()) {
            Some(fmt) => {
                let label = rexon_core::wallpaper_engine::format_label(fmt);
                assert_ne!(label, fmt, "{fmt}: имя формата осталось служебным");
                want.replace(&format!("</b> {fmt} —"), &format!("</b> {label} —"))
            }
            None => want,
        };
        assert_eq!(
            qs::wallpaper_status_text(running, format, path),
            want,
            "строка состояния обоев для {st}"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn restore_native_message_matches_python() {
    let _lang = i18n_pin::ru();
    for case in golden()["restore_native"].as_array().unwrap() {
        let restored = case[0].as_u64().unwrap() as usize;
        let missing: Vec<String> = case[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        // Fix (rule #13): the count had one ending for all numbers
        // («Восстановлено 1 приложений»), now it is declined by the common `plural`.
        let want = case[3].as_str().unwrap().replace(
            &format!("{restored} приложений"),
            &format!(
                "{restored} {}",
                plural(restored as i64, "приложение", "приложения", "приложений")
            ),
        );
        assert_eq!(
            qs::restore_native_message(restored, &missing, case[2].as_str().unwrap()),
            want
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn protection_status_matches_python() {
    let _lang = i18n_pin::ru();
    for case in golden()["protection_status"].as_array().unwrap() {
        let changed: Vec<(String, String, String)> = case[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c[0].as_str().unwrap().to_string(),
                    c[1].as_str().unwrap().to_string(),
                    c[2].as_str().unwrap().to_string(),
                )
            })
            .collect();
        // The golden data was captured from the original, which had no notion of a
        // protection snapshot at all: all its cases are "something is already
        // remembered". Their wording is kept word for word; the "nothing remembered"
        // case is checked by `protection_reports_green_only_after_a_snapshot` below.
        assert_eq!(
            qs::protection_status_text(1, &changed),
            case[1].as_str().unwrap()
        );
    }
}

/// ⭐ «✅ Всё на месте» is said ONLY after a snapshot has been taken.
///
/// Defect (the «Настройки» tab, the «Защита приложений по умолчанию» section):
/// an empty list of differences counted as success. On a clean profile, where
/// nothing is remembered yet and there is nothing to compare against, the
/// section showed a green «Всё на месте» — the buyer read it as "protection
/// works", while they were not protected at all. The check holds both halves:
/// without a snapshot there is no green answer and the button that fixes it is
/// named; with a snapshot the former wording is untouched.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn protection_reports_green_only_after_a_snapshot() {
    let _lang = i18n_pin::ru();
    let nothing = qs::protection_status_text(0, &[]);
    assert!(
        !nothing.contains("Всё на месте") && !nothing.contains('✅'),
        "без эталона раздел рапортует успех: {nothing}"
    );
    assert!(
        nothing.contains("не запомнил"),
        "не сказано, что запоминать ещё нечего: {nothing}"
    );
    // Exactly the button standing next to it in the section is named — otherwise
    // the advice drifts away from the window at the first label edit.
    let button = qs::text("prot.snapshot", &[]).unwrap();
    assert!(nothing.contains(&button), "не названа кнопка «{button}»: {nothing}");

    // Snapshot taken, no differences — now it is green.
    assert!(qs::protection_status_text(2, &[]).contains("✅ Всё на месте"));
    // Snapshot taken and the application replaced — the conversation as before.
    let changed = vec![(
        "Терминал".to_string(),
        "konsole".to_string(),
        "gnome-terminal".to_string(),
    )];
    let text = qs::protection_status_text(2, &changed);
    assert!(text.contains("Терминал: konsole → gnome-terminal"), "{text}");
}

/// Fix 4: the theme file dialog did not show `.gadget`, although the importer
/// accepts such files. All former groups are kept word for word, the missing
/// one is added.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn import_filter_keeps_old_groups_and_adds_gadget() {
    let _lang = i18n_pin::ru();
    use rexon_core::importers;
    let golden_filter = golden()["import_filters"].as_str().unwrap().to_string();
    let now = importers::import_dialog_filter();
    for group in golden_filter.split(";;") {
        if group.starts_with("Все темы Windows") {
            // The combined group grew by exactly .gadget and (03.10.2026) by cursor
            // set archives with a .crs roles file — right after the cursors.
            let want = group
                .trim_end_matches(')')
                .replace("*.curxptheme", "*.curxptheme *.zip *.rar *.7z *.crs")
                + " *.gadget)";
            assert!(now.contains(&want), "сводная группа: {now}");
            continue;
        }
        assert!(now.contains(group), "потеряна группа «{group}» из {now}");
    }
    assert!(now.contains("Гаджеты рабочего стола (*.gadget)"), "{now}");
    assert!(now.contains("Наборы курсоров (*.zip *.rar *.7z *.crs)"), "{now}");
    assert!(now.ends_with("Все файлы (*)"), "{now}");
    // The list of accepted extensions is the same as the dispatcher's.
    for ext in [
        ".themepack", ".THEME", ".msstyles", ".ico", ".7tsp", ".cur", ".inf", ".dream", ".wmv",
        ".gadget", ".zip", ".rar", ".7z", ".crs",
    ] {
        assert!(importers::is_importable_ext(ext), "{ext} потерян");
    }
    // .zip is accepted only as a cursor set: any other ZIP is rejected by the
    // importer after looking inside (`cursor_can_import`).
    for ext in [".rxn", ".png", ""] {
        assert!(!importers::is_importable_ext(ext), "{ext} принят зря");
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn ambient_texts() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::ambient_sound_id("/a/b/дождь.ogg"), "manual_дождь.ogg");
    assert_eq!(qs::ambient_sound_id("дождь.ogg"), "manual_дождь.ogg");
    assert!(qs::ambient_file_filter().contains("*.opus"));
    assert!(qs::ambient_file_filter().starts_with("Аудио (*.ogg"));
    assert!(qs::ambient_file_filter().ends_with(";;Все файлы (*)"));
}

// ============================================================================
// Second migration batch: sections, label catalog, tab decisions
// ============================================================================
//
// The golden data was captured from the tab version BEFORE this migration
// (commit `d305e9d`) by PARSING THE SOURCE: `tab_phrases` got both the
// messages visible only on click and the MATE section, which never appears
// on the comparison machine. F-strings are normalized to "{}" in place of
// substitution points — catalog templates are compared in the same form.

/// A catalog template in the same form in which the golden data remembers the
/// Python f-string.
///
/// Catalog substitution points are `%1`, `%2` (Qt style, as `po/ru.po` writes
/// them); the former `{0}` form is understood too.
fn as_phrase(template: &str) -> String {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' && chars.peek().is_some_and(|d| d.is_ascii_digit() && *d != '0') {
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
            out.push_str("{}");
            continue;
        }
        if c == '{' {
            // "{0}", "{1}", … — a substitution point
            let mut digits = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    digits.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            if !digits.is_empty() && chars.peek() == Some(&'}') {
                chars.next();
                out.push_str("{}");
                continue;
            }
            out.push('{');
            out.push_str(&digits);
            continue;
        }
        out.push(c);
    }
    out
}

/// Labels deliberately absent from the original (rule #16 — bugs are fixed
/// during the port): jargon in the buyer's face is replaced with human words.
const TEXT_FIXES: &[(&str, &str)] = &[
    ("prot.snapshot_error", "⚠️ Ошибка snapshot: {}"),
    ("plasma.backup_error", "⚠️ Ошибка backup: {}"),
    // The hint listed formats, and Flash was not among them — although the engine
    // plays it. An omission in a list reads as "we can't".
    (
        "wp.file_hint",
        "Картинка: JPG, PNG, WebP.  Видео: MP4, WebM, MKV, GIF.",
    ),
    // The checkbox was called «Звук видео-обоев», but it controls the sound of
    // Flash wallpapers too: they share the settings keys (16.09.2026). A label
    // about video would mean the person mutes Flash with a button that says
    // nothing about Flash.
    ("wp.video_audio", "🔊 Звук видео-обоев"),
    // The font name was substituted into the middle of a phrase, lowercased —
    // in German that breaks nouns. Now it goes as is, at the start of the line
    // (01.10.2026).
    ("font.failed", "⚠️ Не удалось применить {}"),
    // ── Pass of 06.09: developer jargon across the whole tab (rule #13) ──
    //
    // The tab was reviewed live, and the buyer read «wallpaper-pack'и»,
    // «синкается в GTK», «ищи в AUR» in it. These are not three separate typos
    // but one class: the strings were written for ourselves. So the WHOLE catalog
    // was rewritten, not just the spotted places (rule #12). Technology names
    // (GTK, AUR, mpv, Pillow, Vimeo, YouTube) stayed in Latin script — the
    // wording around them changed.
    (
        "skin.hint",
        "💡 «Системный» использует тему Plasma/Qt как есть. Остальные скины \
         задают свою палитру независимо от системной темы.",
    ),
    (
        "ob.no_main_window",
        "🎓 Обучение доступно в главном окне Rexon — запустите его без флага \
         одиночной вкладки",
    ),
    ("fm.auto_hint", "← по DE"),
    (
        "quickfix.unknown_de",
        "Для окружения «{}» набор родных приложений неизвестен. Используй \
         ручное управление в блоке «Защита» ниже.",
    ),
    ("yt.url_label", "URL:"),
    (
        "yt.url_hint",
        "https://youtube.com/watch?v=…  или  vimeo / другой поддерживаемый",
    ),
    ("yt.no_url", "⚠️ Вставь URL видео"),
    (
        "yt.no_ytdlp",
        "⚠️ yt-dlp не установлен — нажми «Установить yt-dlp»",
    ),
    (
        "yt.audio_next",
        "🔊 Звук YouTube: {} (применится к следующему ▶️ Применить)",
    ),
    (
        "web.hint",
        "URL веб-страницы / WebGL-визуализации или путь к локальному HTML.",
    ),
    ("web.no_url", "⚠️ Вставь URL или путь к HTML"),
    (
        "lib.empty",
        "Пусто. Установи wallpaper-pack через Магазин тем OCS или перенеси \
         .rxn в окно «Расширения».",
    ),
    ("lib.preview_tip", "Открыть файл системно (предпросмотр)"),
    (
        "blur.no_pillow",
        "⚠️ Pillow не установлен — размытие недоступно (<tt>pip install \
         Pillow</tt> или пакет python-pillow)",
    ),
    // ── Pass of 18.09: remaining jargon in what the buyer reads ──
    //
    // The blur strength slider value was labelled «20px». A radius in pixels is
    // a quantity for whoever writes the filter, not for whoever moves the
    // slider: nothing can be decided by it, and units in a label next to the
    // word «Сила:» only confuse. The bare scale division remained. ⭐The
    // difference from the original is deliberate: Python spoke jargon, and we do
    // not bend the golden data but diverge from it in favour of the person
    // (rule #13).
    ("blur.px", "{}px"),
    // The sample path in the web wallpaper field was the English
    // «/path/to/index.html». A buyer coming from Windows reads a foreign language
    // and the service word "path" in their window — although the hint one line
    // above already explained everything in Russian. ⭐The difference from the
    // original is deliberate.
    (
        "web.url_hint",
        "https://example.com  или  /path/to/index.html",
    ),
    ("blur.apply", "🌫️ Размыть текущие"),
    ("blur.remove", "🔆 Убрать блюр"),
    ("blur.applying", "⏳ Размываю текущие обои (radius={})…"),
    ("blur.removing", "⏳ Убираю блюр, восстанавливаю обои…"),
    ("blur.failed", "Не удалось размыть (нет статичных обоев / нет Pillow)"),
    ("blur.removed", "Блюр снят, обои восстановлены"),
    ("amb.no_manager", "⚠️ Менеджер звуков недоступен (нет движка обоев)"),
    (
        "amb.no_mpv",
        "⚠️ mpv не установлен — фоновые звуки недоступны (установи пакет \
         <tt>mpv</tt>)",
    ),
    ("amb.add_tip", "Запустить любой аудиофайл фоном (mpv loop)"),
    ("amb.dialog_title", "Выбери аудиофайл"),
    (
        "amb.volume_hint",
        "Громкость применяется к <b>новому</b> добавляемому звуку. Для \
         активного — кнопка 🔊 у его строки переприменит.",
    ),
    ("amb.empty", "Нет активных фоновых звуков. Нажми «Добавить файл…»."),
    ("amb.setvol_tip", "Применить громкость слайдера к этому звуку"),
    (
        "snd.themes_hint",
        "💡 Мало тем? Установи: <tt>sound-theme-freedesktop</tt>, \
         <tt>sound-theme-smooth</tt> или ищи в AUR.",
    ),
    ("snd.event_tip", "Event: {}"),
    // «(не задан)» read as "there is no font"; in fact the desktop's default
    // font applies — and that is what it says now (acceptance 10.10.2026).
    ("font.unset", "<i>(не задан)</i>"),
    (
        "plasma.backup_tip",
        "Технически: копируются plasma-org.kde.plasma.desktop-appletsrc, \
         plasmashellrc, kglobalshortcutsrc",
    ),
    (
        "plasma.confirm_text",
        "Восстановить Plasma-панель из бэкапа «{}»?\n\nТекущая конфигурация \
         будет перезаписана.",
    ),
];

/// Labels that did NOT exist and could not exist in the tab's original: they
/// appeared together with the wallpaper sound settings, which the tab did not
/// have (there was one checkbox without storage and without volume). The list
/// is closed — a new string must get here deliberately, not slip past the check.
const TEXT_ADDITIONS: &[&str] = &[
    // Resetting sites "without asking" (`link_policy`, 16.11.2026).
    "links.reset",
    // Protecting terminal colours from themes (`terminal_guard`, 06.11.2026).
    "prot.terminal_colors",
    "prot.terminal_colors_tip",
    // Background sound failure with mpv installed: the original had one answer
    // for any failure, «нужен mpv» — untrue when it is the file itself that won't play.
    "amb.start_unplayable",
    // Changing the volume of a sound that no longer plays: the original kept silent about it.
    "amb.not_playing",
    // The panel backup restore confirmation moved from a Qt window into the
    // page (QML tab): the answer labels are now our own.
    "dialog.yes",
    "dialog.no",
    "wp.audio_live",
    "wp.audio_volume",
    "wp.audio_mute_other",
    "wp.audio_mute_covered",
    "wp.audio_hint",
    // Battery and game in the common wallpaper pause model (15.11.2026): the
    // tab's original had no such conditions at all.
    "wp.pause_battery",
    "wp.pause_game",
    // A wallpaper apply result with a caveat (15.11.2026): a warning icon
    // instead of a green check — the tab's original had no such result.
    "wp.applied_warn",
    // The «Значок в системном лотке» section: the tab's original had no icon at
    // all, so it could not have labels for it either.
    "tray.autostart",
    "tray.autostart_hint",
    "tray.autostart_on",
    "tray.autostart_off",
    "tray.autostart_failed",
    "tray.missing",
    // «Тема не задана»: the tab's original did not know this string — with an
    // empty or unknown theme it silently showed the first installed one.
    "snd.theme_unset",
    // The system names a theme that is not on disk: the original showed the first
    // installed one, the Rust tab until 10.10.2026 said «не задана».
    "snd.theme_missing",
    // The «Язык» section (30.09): there was no language choice either in the tab's
    // original or in the Russian text snapshot — see [`AFTER_RU_SNAPSHOT_TEXTS`].
    "lang.row",
    "lang.system",
    "lang.restart_hint",
    "lang.restart",
    "lang.saved",
    "lang.restart_failed",
    // Font antialiasing (04.10): the tab's original had only three fonts and no
    // rendering at all — see [`AFTER_RU_SNAPSHOT_TEXTS`].
    "font.render_title",
    "font.aa",
    "font.aa_exclude",
    "font.aa_to",
    "font.pt",
    "font.subpixel",
    "font.hinting",
    "font.render_hint",
    "font.render_saved",
    "font.render_native_failed",
    "font.fc_refused",
    "font.fc_failed",
    // «Изменить все шрифты» and forced DPI (04.10).
    "font.all_mode",
    "font.dpi",
    // Ready-made rendering presets, as in MATE (14.11).
    "font.render_presets",
    // Fetching icons and cursor for the theme (17.10): the original had no session
    // appearance watcher (`theme_watch`).
    "tray.theme_deps",
    "tray.theme_deps_hint",
    "tray.theme_deps_on",
    "tray.theme_deps_off",
    // The «Rexon может изменять систему до закрытия окна» bar and «Запретить
    // сейчас» (B13, 29.10): the original had no privilege session.
    "admin.info",
    "admin.revoke",
];

/// Labels that appeared AFTER the Russian text snapshot
/// (`golden/qt_settings_ru_before.json`): they have no "as before", their
/// Russian is checked separately ([`language_section_russian`]).
const AFTER_RU_SNAPSHOT_TEXTS: &[&str] = &[
    // Resetting sites "without asking" (16.11.2026) — Russian is checked in
    // `links_texts_russian`.
    "links.reset",
    // Battery and game in the common wallpaper pause model (15.11.2026) — Russian
    // is checked in `wallpaper_pause_texts_russian`.
    "wp.pause_battery",
    "wp.pause_game",
    // A result with a caveat (15.11.2026) — Russian is checked in
    // `wallpaper_done_with_warning_is_not_a_green_check`.
    "wp.applied_warn",
    "prot.terminal_colors",
    "prot.terminal_colors_tip",
    "snd.theme_missing",
    "lang.row",
    "lang.system",
    "lang.restart_hint",
    "lang.restart",
    "lang.saved",
    "lang.restart_failed",
    // Font antialiasing — Russian is checked in `font_rendering_texts_russian`.
    "font.render_title",
    "font.aa",
    "font.aa_exclude",
    "font.aa_to",
    "font.pt",
    "font.subpixel",
    "font.hinting",
    "font.render_hint",
    "font.render_saved",
    "font.render_native_failed",
    "font.fc_refused",
    "font.fc_failed",
    // «Изменить все шрифты» and forced DPI (04.10).
    "font.all_mode",
    "font.dpi",
    // Ready-made presets — Russian is checked in `font_render_presets_russian`.
    "font.render_presets",
    "tray.theme_deps",
    "tray.theme_deps_hint",
    "tray.theme_deps_on",
    "tray.theme_deps_off",
    "admin.info",
    "admin.revoke",
];

/// Sections that appeared after the Russian text snapshot.
const AFTER_RU_SNAPSHOT_SECTIONS: &[&str] = &["langGroup", "linksGroup"];

/// Sections of the original REMOVED from the tab deliberately, and their
/// labels. The golden files are frozen and remember them; here it is stated
/// that the loss is a decision, not damage.
///
/// `mateIconsGroup` (08.11.2026, Wehrwolfmann's decision): a section needed by
/// MATE alone has no place in the common settings. Rexon arranges Caja icons
/// under live wallpapers itself (`wallpaper_backend::prepare_desktop_icons`).
const SECTION_REMOVALS: &[&str] = &["mateIconsGroup"];
const TEXT_REMOVALS: &[&str] = &[
    "mate.icons_on",
    "mate.icons_off",
    "mate.restart_caja",
    "mate.no_api",
    "mate.icons_state",
    "mate.caja_restarted",
    "mate.icons_failed",
    "mate.caja_failed",
    // Wallpaper sound changes on the fly (15.11.2026): «применится к следующему
    // видео» and «⏳ Включаю звук видео-обоев…» were untrue and were shown nowhere.
    "wp.audio_next",
    "wp.audio_reapply",
    "wp.audio_enabling",
    "wp.audio_disabling",
];

/// The removed section and labels really are removed: neither the core tables
/// nor the page markup have them.
#[test]
fn removed_sections_and_texts_are_gone() {
    const SETTINGS_QML: &str = include_str!("../../rexon-gui/qml/SettingsPage.qml");
    for id in SECTION_REMOVALS {
        assert!(!qs::SECTIONS.iter().any(|(s, _, _)| s == id), "{id} снова в таблице разделов");
        assert!(!qs::section_order().contains(id), "{id} снова в порядке показа");
        assert!(!SETTINGS_QML.contains(&format!("\"{id}\"")), "{id} снова в разметке страницы");
    }
    for key in TEXT_REMOVALS {
        assert!(qs::text_template(key).is_none(), "подпись {key} снова в каталоге");
    }
}

/// Sections whose description was rewritten AFTER the Russian text snapshot
/// because the section grew: `(section, new Russian description)`. The
/// snapshot must remember the former one, and the catalog must return the new
/// one word for word.
const AFTER_RU_SNAPSHOT_REWORDED: &[(&str, &str)] = &[(
    // 04.10: small, toolbar and menu fonts and text antialiasing were added to
    // the three fonts.
    "fontsGroup",
    "Шрифты интерфейса, терминала, мелких подписей, панелей инструментов, меню и \
     заголовков окон, а также сглаживание текста. Применяется к текущему рабочему столу \
     и к программам на других библиотеках интерфейса",
)];

/// Sections that did NOT exist in the tab's original. The list is closed for
/// the same reason as [`TEXT_ADDITIONS`]: a new section must get here
/// deliberately, not slip past the check.
const SECTION_ADDITIONS: &[&str] = &[
    // The question before an external link from foreign text (16.11.2026): it did
    // not exist at all — links opened silently.
    "linksGroup",
    // Rexon had no tray icon in any version of the tab.
    "trayGroup",
    // Nor an interface language choice: Rexon was Russian only.
    "langGroup",
];

/// Sections whose title or description was rewritten from developer jargon
/// into human Russian (rule #13, pass of 06.09) — the triple "frame name,
/// former title, former description". The check is the same as for
/// [`TEXT_FIXES`]: the golden data must remember the former wording, and the
/// new one must differ from it, otherwise the "fix" stayed in a comment.
const SECTION_FIXES: &[(&str, &str, &str)] = &[
    (
        "fmGroup",
        "📁 Файловый менеджер",
        "Файловый менеджер по умолчанию для папок и MIME-типа inode/directory",
    ),
    (
        "libraryGroup",
        "📚 Библиотека обоев",
        "Установленные wallpaper-pack'и (.rxn). «Применить» ставит обой; \
         ставятся через Магазин тем OCS или окно «Расширения»",
    ),
    (
        "blurGroup",
        "🌫️ Размытие обоев (Blur)",
        "Размыть текущие статичные обои для эстетики (Gaussian blur). \
         Оригинал сохраняется — блюр можно убрать одной кнопкой",
    ),
    (
        "ambientGroup",
        "🔊 Фоновые звуки (ambient)",
        "Зацикленные фоновые звуки (дождь, ветер, музыка) через mpv. \
         Громкость регулируется тут или системным микшером",
    ),
    (
        "soundThemeGroup",
        "🔔 Звуки событий",
        "Системная звуковая тема для {} (клики, уведомления, ошибки). \
         Применяется через нативные API DE",
    ),
    (
        "fontsGroup",
        "🔤 Системные шрифты",
        "Выбрать шрифт интерфейса, моноширинный и шрифт заголовков окон. \
         Применяется к текущему DE и синкается в GTK-приложения",
    ),
];

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn every_tab_text_comes_from_python() {
    let _lang = i18n_pin::ru();
    let g = golden();
    let known: BTreeMap<String, ()> = g["tab_phrases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v.as_str().unwrap().to_string(), ()))
        .collect();
    for (key, _) in qs::TEXTS {
        let template = &qs::text_template(key).unwrap();
        if TEXT_ADDITIONS.contains(key) {
            // An added label must really be NEW: if it matches the golden one, it is
            // not an addition but damage to an existing one.
            assert!(
                !known.contains_key(&as_phrase(template)),
                "«{key}» помечена как новая, но такая подпись во вкладке уже была"
            );
            continue;
        }
        if let Some((_, was)) = TEXT_FIXES.iter().find(|(k, _)| k == key) {
            // A fixed string must differ from the former one — otherwise the
            // "fix" stayed in a comment, not in the code.
            assert!(
                known.contains_key(*was),
                "эталон не помнит прежней формулировки {key}"
            );
            assert_ne!(as_phrase(template), *was, "{key}: починка не применена");
            continue;
        }
        assert!(
            known.contains_key(&as_phrase(template)),
            "подпись «{key}» = {template:?} расходится с оригиналом вкладки"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn tab_sections_match_python() {
    let _lang = i18n_pin::ru();
    let g = golden();
    let want = g["tab_sections"].as_object().unwrap();
    assert_eq!(
        want.len() + SECTION_ADDITIONS.len() - SECTION_REMOVALS.len(),
        qs::SECTIONS.len(),
        "число секций разошлось с оригиналом"
    );
    for (id, title, desc) in qs::SECTIONS {
        // The table holds English `msgid`s; the golden data remembers Russian.
        // gettext returns the catalog header for an empty `msgid` — not translated.
        let title = &if title.is_empty() { String::new() } else { i18n::tr(title) };
        let desc = &i18n::tr(desc);
        if SECTION_ADDITIONS.contains(id) {
            // An added section must really be NEW: if it is found in the golden
            // data, it is not an addition but damage to an existing one.
            assert!(!want.contains_key(*id), "{id} помечен новым, а он был");
            continue;
        }
        let row = want
            .get(*id)
            .unwrap_or_else(|| panic!("в оригинале нет секции {id}"));
        let want_desc = row[1].as_str().unwrap_or("");
        if let Some((_, was_title, was_desc)) = SECTION_FIXES.iter().find(|(k, _, _)| k == id) {
            // A rewritten section: the golden data must remember the former words, and
            // the new ones must differ from them in at least one of the two fields.
            assert_eq!(want_desc, *was_desc, "эталон не помнит описания {id}");
            assert_eq!(row[0].as_str().unwrap_or(""), *was_title, "эталон {id}");
            assert!(
                as_phrase(desc) != *was_desc || *title != *was_title,
                "{id}: починка формулировки не применена"
            );
            continue;
        }
        assert_eq!(
            as_phrase(desc),
            want_desc,
            "описание секции {id} разошлось с оригиналом"
        );
        match row[0].as_str() {
            Some(want_title) => assert_eq!(
                *title, want_title,
                "заголовок секции {id} разошёлся с оригиналом"
            ),
            // The original assembled the onboarding section title in place —
            // now the core builds it from the onboarding texts themselves.
            None => {
                assert!(title.is_empty(), "{id}: заголовок обязан быть у обучения");
                let (built, _) = qs::section_texts(id, "KDE").unwrap();
                assert_eq!(
                    built,
                    format!("🎓 {}", rexon_core::onboarding::settings_label())
                );
            }
        }
    }
}

#[test]
fn section_order_matches_python() {
    let g = golden();
    let want: Vec<String> = g["tab_section_order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .filter(|id| !SECTION_REMOVALS.contains(&id.as_str()))
        .collect();
    let ours: Vec<String> = qs::section_order()
        .into_iter()
        .filter(|id| !SECTION_ADDITIONS.contains(id))
        .map(str::to_owned)
        .collect();
    assert_eq!(ours, want, "порядок секций вкладки");
    // An added section must stand in display order, not "somewhere".
    for id in SECTION_ADDITIONS {
        assert!(qs::section_order().iter().any(|s| s == id), "{id} не в порядке показа");
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn sound_section_names_current_de() {
    let _lang = i18n_pin::ru();
    let (_, desc) = qs::section_texts("soundThemeGroup", "COSMIC").unwrap();
    assert!(
        desc.contains("для COSMIC"),
        "имя окружения не подставилось: {desc}"
    );
    assert!(!desc.contains("%1"), "место подстановки осталось: {desc}");
    assert!(qs::section_texts("нет такой секции", "KDE").is_none());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn text_lookup_is_loud_about_unknown_keys() {
    let _lang = i18n_pin::ru();
    // An empty label in the window looks "intended" — the core must answer
    // "no such key", not an empty string.
    assert!(qs::text("такого.ключа.нет", &[]).is_none());
    assert_eq!(
        qs::text("fm.current", &["nemo".to_string()]).unwrap(),
        "Текущий файловый менеджер: <b>nemo</b>"
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn clip_counts_letters_not_bytes() {
    let _lang = i18n_pin::ru();
    // A Russian string truncated by bytes falls apart in the middle of a letter.
    let text = "яяяяя";
    assert_eq!(qs::clip(text, 3), "яяя");
    assert_eq!(qs::clip(text, 99), text);
    assert_eq!(qs::wallpaper_done(true, &"я".repeat(200)).chars().count(), 160 + "✅ Обои применены: ".chars().count());
}

/// The wallpaper is set, but with a caveat: a warning icon instead of a green
/// check, and the caveat itself is not truncated together with a long name.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn wallpaper_done_with_warning_is_not_a_green_check() {
    let _lang = i18n_pin::ru();
    let warning = "Остановите их во вкладке «Wallpaper Engine». ".repeat(6);
    let line = qs::wallpaper_done_warned(true, &"я".repeat(200), Some(&warning));
    assert!(line.starts_with("⚠️ Обои применены: "), "{line}");
    assert!(!line.contains('✅'), "{line}");
    assert!(line.ends_with(warning.trim()), "оговорка обрезана: {line}");
    assert!(qs::status_is_warning(&line));
    // Without a caveat and on failure — the former strings.
    assert_eq!(qs::wallpaper_done_warned(true, "лес", None), qs::wallpaper_done(true, "лес"));
    assert_eq!(qs::wallpaper_done_warned(true, "лес", Some("  ")), qs::wallpaper_done(true, "лес"));
    assert_eq!(qs::wallpaper_done_warned(false, "нет", Some("x")), qs::wallpaper_done(false, "нет"));
    assert!(!qs::status_is_warning(&qs::wallpaper_done(true, "лес")));
    assert!(qs::status_is_warning(&qs::wallpaper_done(false, "нет")));
    // YouTube and web — the same fork.
    let yt = qs::done_or_warned(true, "ролик", Some("оговорка"), qs::youtube_done);
    assert!(yt.starts_with("⚠️ ") && yt.ends_with("оговорка"), "{yt}");
    assert_eq!(qs::done_or_warned(true, "ролик", None, qs::youtube_done), qs::youtube_done(true, "ролик"));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn ytdlp_button_follows_system() {
    let _lang = i18n_pin::ru();
    // Fix of the original: after installation the button must become «Обновить».
    let (label, button, action) = qs::ytdlp_row(true, "2026.07.04");
    assert!(label.contains("2026.07.04"));
    assert_eq!(action, "update");
    assert!(button.contains("Обновить"));
    let (label, button, action) = qs::ytdlp_row(false, "");
    assert!(label.contains("не установлен"));
    assert_eq!(action, "install");
    assert!(button.contains("Установить"));

    let (btn, status) = qs::ytdlp_working("update");
    assert!(btn.contains("Обновляю") && status.contains("пароль"));
    let (btn, _) = qs::ytdlp_working("install");
    assert!(btn.contains("Устанавливаю"));
}

#[test]
fn library_preview_covers_every_video_ext() {
    // Fix: the tile's own list knew only part of the video formats.
    for fmt in qs::VIDEO_EXTS {
        assert!(
            qs::library_shows_preview(fmt),
            "{fmt}: движок это проигрывает, а кнопки предпросмотра нет"
        );
    }
    for fmt in ["video", "swf", "MP4", "gif"] {
        assert!(qs::library_shows_preview(fmt), "{fmt}");
    }
    for fmt in ["static", "web", "scene", "", "application"] {
        assert!(!qs::library_shows_preview(fmt), "{fmt}");
    }
    // The original's former set — each of its elements still has a button.
    for fmt in ["video", "gif", "swf", "webm", "mkv", "mp4"] {
        assert!(qs::library_shows_preview(fmt), "потеряли прежний {fmt}");
    }
}

#[test]
fn web_url_becomes_file_scheme() {
    assert_eq!(
        qs::web_wallpaper_url("/home/u/index.html", true),
        "file:///home/u/index.html"
    );
    // No file — the path stays as is, the engine will say so itself.
    assert_eq!(qs::web_wallpaper_url("/nope.html", false), "/nope.html");
    assert_eq!(
        qs::web_wallpaper_url("https://example.com", false),
        "https://example.com"
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn import_failure_speaks_human() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::import_failure_reason(true, "нечто"), "файл не найден");
    assert_eq!(qs::import_failure_reason(false, "  "), 
               "файл повреждён или формат не поддерживается");
    assert_eq!(qs::import_failure_reason(false, " битый zip "), "битый zip");
    let (note, status) = qs::import_success_texts("  ", &[]);
    assert!(note.contains("«тема»") && status.contains("тема"));
    let (note, _) = qs::import_success_texts(" Aero ", &[]);
    assert!(note.contains("«Aero»"));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn blur_words_match_the_action() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::blur_result("blur", true), "Текущие обои размыты");
    assert!(qs::blur_result("blur", false).contains("Не удалось размыть"));
    assert_eq!(
        qs::blur_result("unblur", true),
        "Размытие снято, обои восстановлены"
    );
    assert!(qs::blur_result("unblur", false).contains("Нет сохранённых"));
    let (low, high, step, default) = qs::BLUR_RANGE;
    assert!(low < default && default < high && step > 0);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn protection_types_keep_terminal_last() {
    let _lang = i18n_pin::ru();
    let types = qs::protection_types();
    assert_eq!(types.last(), Some(&"terminal"));
    assert_eq!(types.len(), rexon_core::defaults::MIME_TYPES.len() + 1);
    // Each checkbox label comes from the core, not from the window.
    let (label, tip) = qs::protection_row_texts("terminal", "org.kde.konsole.desktop");
    assert_eq!(label, "🖥️ Терминал   ·  запомнено: Konsole");
    assert_eq!(tip, "Файл ярлыка: org.kde.konsole.desktop");
    let (label, tip) = qs::protection_row_texts("browser", "");
    assert_eq!(label, "🌐 Браузер   ·  пока не запомнено");
    assert!(tip.is_empty());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn statuses_pick_the_right_sign() {
    let _lang = i18n_pin::ru();
    assert!(qs::restore_native_status("x", true).starts_with("⚠️"));
    assert!(qs::restore_native_status("x", false).starts_with("✅"));
    assert!(qs::compliance_status("x", false).starts_with("ℹ️"));
    assert_eq!(qs::compliance_status("x", true), "x");
    assert!(qs::plasma_status(true, "готово").starts_with("✅"));
    assert!(qs::plasma_status(false, "нет копии").starts_with("⚠️"));
    assert!(qs::fm_apply_status(true).starts_with("✅"));
    assert!(qs::sound_theme_status(false, "ocean").contains("ocean"));
    assert!(qs::event_sounds_status(true, false).contains("выключены"));
    assert!(qs::event_sounds_status(false, true).contains("Не удалось"));
    assert!(qs::font_value_text("  ").contains("по умолчанию"));
    assert_eq!(qs::font_value_text("Noto Sans 11"), "<i>Noto Sans 11</i>");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn plasma_backups_are_counted_and_confirmed() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        qs::plasma_backups_header(10),
        "<b>Сохранённые копии (10):</b>"
    );
    let (title, question) = qs::plasma_restore_confirm("20260620_011005");
    assert_eq!(title, "Восстановить?");
    // ⭐A deliberate difference from the original (rule #13): Python put the
    // machine label «20260620_011005» into the question, although it labelled
    // the list row the person had just clicked «20.06.2026 в 01:10». The same
    // backup cannot be called by two names in one window, so the question asks
    // with the human date too.
    assert!(
        question.contains("20.06.2026 в 01:10") && question.contains("будет заменён"),
        "вопрос о возврате копии показывает машинную метку времени: {question}"
    );
    // An unparsed label stays as is — the question must not turn into
    // «из копии от ?».
    let (_, broken) = qs::plasma_restore_confirm("мусор");
    assert!(broken.contains("мусор"));
    assert_eq!(qs::PLASMA_BACKUPS_SHOWN, 5);
}

// ============================================================================
// Section set and status colours (tab migration to QML, 2026-08-23)
// ============================================================================

/// Section display conditions in the tab's ORIGINAL — the very `if`s that stood
/// in section construction before the decision moved to the core.
///
/// The pair "section → the fact without which it was not drawn":
///   * `obGroup` — `onboarding.available()`;
///   * `plasmaGroup` — `PlasmaProtector.is_plasma_running() or has_backups()`;
///   * `aeroGroup` — `detect_plasma_forks()`.
/// The other sections were always drawn.
/// (`mateIconsGroup` is removed since 08.11.2026 — see [`SECTION_REMOVALS`].)
const CONDITIONAL_SECTIONS: &[&str] = &["obGroup", "plasmaGroup", "aeroGroup"];

#[test]
fn sections_without_conditions_are_always_shown() {
    // No fact confirmed — exactly the unconditional sections remain.
    let nothing = qs::SectionFacts::default();
    let shown = qs::visible_sections(&nothing);
    for id in qs::section_order() {
        let conditional = CONDITIONAL_SECTIONS.contains(&id);
        assert_eq!(
            shown.contains(&id),
            !conditional,
            "раздел {id}: показан не так, как в оригинале вкладки"
        );
    }
}

#[test]
fn every_condition_opens_exactly_its_own_section() {
    // Each fact separately: enables its own section and NOT A SINGLE other one.
    let cases: &[(&str, qs::SectionFacts)] = &[
        ("obGroup", qs::SectionFacts::new(true, false, false)),
        ("plasmaGroup", qs::SectionFacts::new(false, true, false)),
        ("aeroGroup", qs::SectionFacts::new(false, false, true)),
    ];
    for (id, facts) in cases {
        let shown = qs::visible_sections(facts);
        assert!(shown.contains(id), "{id}: свой факт раздел не открыл");
        for other in CONDITIONAL_SECTIONS {
            if other == id {
                continue;
            }
            assert!(
                !shown.contains(other),
                "{id}: заодно открылся чужой раздел {other}"
            );
        }
    }
}

#[test]
fn all_facts_show_every_section_in_core_order() {
    // Everything confirmed — ALL sections are visible, in the same order as in the table.
    let everything = qs::SectionFacts::new(true, true, true);
    assert_eq!(qs::visible_sections(&everything), qs::section_order());
}

/// Tab status colours — the golden values are recorded here (they used to be
/// compared with the Python widget tab; since 02.10.2026 Python is only an archive).
#[test]
fn status_colors_are_frozen() {
    assert_eq!(qs::COLOR_OK, "#4ade80", "всё на месте");
    assert_eq!(qs::COLOR_BAD, "#ef4444", "нет / остановить");
    assert_eq!(qs::COLOR_WARN, "#f59e0b", "внимание");

    assert_eq!(qs::fm_badge_color(true), qs::COLOR_OK);
    assert_eq!(qs::fm_badge_color(false), qs::COLOR_BAD);
    // A mark and its colour are a pair: "installed" is green, "not installed" red.
    assert!(qs::fm_badge(true).starts_with("✅"));
    assert!(qs::fm_badge(false).starts_with("❌"));
}

/// Every section the core knows must be buildable by the page.
///
/// The widget tab catches this at startup (`raise RuntimeError`), the QML one
/// has a `switch` in `SettingsPage.qml`. The guard reads the markup itself: a
/// section added to the core and forgotten in the page turns the test red
/// instead of showing the buyer an empty frame (rule #12).
#[test]
fn qml_page_builds_every_section() {
    let qml = read_sample(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../rexon-gui/qml/SettingsPage.qml"),
        "QML-страница «Настройки»",
    );

    // An empty section table would run the loop zero times and declare the page
    // fine without checking a single section.
    assert!(
        !qs::section_order().is_empty(),
        "ядро не знает ни одного раздела: сверять страницу не с чем"
    );
    for id in qs::section_order() {
        assert!(
            qml.contains(&format!("case \"{id}\":")),
            "раздел {id} есть в ядре, но QML-страница его не строит"
        );
    }
}

/// The label under the font button must not be a raw config entry.
///
/// Fix during the tab migration (rule #16): KDE stores a ten-field QFont
/// serialization, and it reached the buyer as is. Now the string is parsed by
/// the same `parse_font_spec` that opens the chooser dialog.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn font_value_is_human_not_config_line() {
    let _lang = i18n_pin::ru();
    let kde = "Segoe UI,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,,0,0";
    let shown = qs::font_value_text(kde);
    assert!(
        shown.contains("Segoe UI 9"),
        "имя и кегль потерялись: {shown}"
    );
    assert!(
        !shown.contains(",-1,"),
        "сырая запись конфига видна покупателю: {shown}"
    );
    // The GNOME entry is human-readable already — its look does not change.
    assert_eq!(qs::font_value_text("Noto Sans 11"), "<i>Noto Sans 11</i>");
    // Nothing recorded — so we say.
    assert!(qs::font_value_text("   ").contains("по умолчанию"));
    // A completely unintelligible string is shown as is: that is more honest than
    // «не задан» where something is recorded.
    assert!(qs::font_value_text("???").contains("???"));
}

/// Tab layout and paints that the QML page takes from the core through the
/// `styles` map. Previously each number was compared with the source of the
/// Python widget tab; since 02.10.2026 Python is only an archive, and the
/// reference is the [`LAYOUT_FROZEN`] record below, in the same form the
/// numbers had in the widgets (`setSpacing(8)`, `neutral_rgba(0.04)`). A shift
/// of any core number turns the check red until the reference is updated deliberately.
#[test]
fn layout_and_colors_are_frozen() {
    let (group_bg, group_border) = qs::group_colors();
    let (banner_bg, banner_border) = qs::polkit_banner_colors();
    let btn = qs::primary_button_colors();
    let (danger_off_bg, danger_off_text) = qs::danger_button_disabled_colors();
    let (lib_border, lib_hover) = qs::lib_card_colors(true);
    // `neutral_rgba(0.04)` in Python is a call, in the core a ready string:
    // compare by the number inside, not by the notation.
    let neutral = |css: &str| {
        let alpha = css
            .trim_start_matches("rgba(127,127,127,")
            .trim_end_matches(')');
        format!("neutral_rgba({alpha})")
    };

    let expected = [
        format!(
            "setContentsMargins({}, {}, {}, {})",
            qs::PAGE_MARGIN, qs::PAGE_MARGIN, qs::PAGE_MARGIN, qs::PAGE_MARGIN
        ),
        format!("setSpacing({})", qs::PAGE_SPACING),
        format!(
            "setContentsMargins({}, {}, {}, {})",
            qs::GROUP_MARGIN_H, qs::GROUP_MARGIN_V, qs::GROUP_MARGIN_H, qs::GROUP_MARGIN_V
        ),
        format!("setSpacing({})", qs::GROUP_SPACING),
        format!("border-radius: {}px", qs::GROUP_RADIUS),
        neutral(&group_bg),
        neutral(&group_border),
        format!(
            "border-radius: {}px; padding: {}px",
            qs::POLKIT_BANNER_RADIUS, qs::POLKIT_BANNER_PADDING
        ),
        neutral(&banner_bg),
        neutral(&banner_border),
        format!("background-color: {}", btn.background),
        format!("background-color: {}", btn.hover),
        format!("background-color: {}", btn.pressed),
        format!("border: 1px solid {}", btn.border),
        format!(
            "padding: {}px {}px",
            qs::PRIMARY_BTN_PADDING_V, qs::PRIMARY_BTN_PADDING_H
        ),
        neutral(&btn.disabled_background),
        neutral(&btn.disabled_border),
        neutral(&btn.disabled_text),
        format!(
            "padding: {}px {}px",
            qs::DANGER_BTN_PADDING_V, qs::DANGER_BTN_PADDING_H
        ),
        neutral(&danger_off_bg),
        neutral(&danger_off_text),
        format!(
            "setContentsMargins({}, {}, {}, {})",
            qs::FM_ROW_MARGIN_H, qs::FM_ROW_MARGIN_V, qs::FM_ROW_MARGIN_H, qs::FM_ROW_MARGIN_V
        ),
        format!("setSpacing({})", qs::FM_ROW_SPACING),
        format!("border-radius: {}px", qs::FM_ROW_RADIUS),
        format!("background-color: {}", qs::fm_row_focus_color()),
        format!("setMinimumWidth({})", qs::FIELD_LABEL_MIN_WIDTH),
        format!("setMinimumWidth({})", qs::VOLUME_LABEL_MIN_WIDTH),
        format!("setMaximumWidth({})", qs::VOLUME_SLIDER_MAX_WIDTH),
        format!("setMinimumWidth({})", qs::URL_LABEL_MIN_WIDTH),
        format!("setMinimumWidth({})", qs::BLUR_LABEL_MIN_WIDTH),
        format!("setMinimumWidth({})", qs::BLUR_VALUE_MIN_WIDTH),
        format!("setMaximumWidth({})", qs::AMBIENT_SLIDER_MAX_WIDTH),
        format!("setMinimumWidth({})", qs::AMBIENT_PERCENT_MIN_WIDTH),
        format!(
            "setContentsMargins(0, {}, 0, 0)",
            qs::AMBIENT_LIST_TOP_MARGIN
        ),
        format!("setSpacing({})", qs::AMBIENT_LIST_SPACING),
        format!(
            "setContentsMargins({}, {}, {}, {})",
            qs::AMBIENT_ROW_MARGIN_H,
            qs::AMBIENT_ROW_MARGIN_V,
            qs::AMBIENT_ROW_MARGIN_H,
            qs::AMBIENT_ROW_MARGIN_V
        ),
        format!("background-color: {}", qs::ambient_row_color(true)),
        format!("setFixedWidth({})", qs::AMBIENT_ROW_BUTTON_WIDTH),
        format!(
            "rgba(220,80,80,{})",
            qs::AMBIENT_STOP_FILL_ALPHA
        ),
        format!(
            "rgba(220,80,80,{})",
            qs::AMBIENT_STOP_FILL_HOVER_ALPHA
        ),
        format!(
            "padding: {}px {}px",
            qs::AMBIENT_STOP_PADDING_V, qs::AMBIENT_STOP_PADDING_H
        ),
        format!("setSpacing({})", qs::LIB_GRID_SPACING),
        format!("setContentsMargins(0, {}, 0, 0)", qs::LIB_GRID_TOP_MARGIN),
        format!("border: 1px solid {}", lib_border),
        format!("border-color: {}", lib_hover),
        format!("border-radius: {}px", qs::LIB_CARD_RADIUS),
        format!(
            "setContentsMargins({}, {}, {}, {})",
            qs::LIB_INFO_MARGIN_H, qs::LIB_INFO_MARGIN_V, qs::LIB_INFO_MARGIN_H, qs::LIB_INFO_MARGIN_V
        ),
        format!("setSpacing({})", qs::LIB_INFO_SPACING),
        format!("setSpacing({})", qs::LIB_BUTTON_SPACING),
        format!("setFixedWidth({})", qs::LIB_PREVIEW_BUTTON_WIDTH),
        format!("font-size: {}pt", qs::LIB_PLACEHOLDER_POINT_SIZE),
        format!("background-color: {}", qs::lib_placeholder_color(true)),
        format!("padding: {}px;", qs::LIB_EMPTY_PADDING),
        format!("setSpacing({})", qs::SOUND_PREVIEW_SPACING),
        format!("setSpacing({})", qs::FONT_ROW_SPACING),
        format!("setSpacing({})", qs::FONT_COL_SPACING),
        format!("setSpacing({})", qs::PLASMA_ROW_SPACING),
        format!("setSpacing({})", qs::INFO_BOX_SPACING),
    ];
    assert_eq!(expected.to_vec(), LAYOUT_FROZEN, "раскладка или краски вкладки сдвинулись");
}

const LAYOUT_FROZEN: &[&str] = &[
    "setContentsMargins(24, 24, 24, 24)",
    "setSpacing(14)",
    "setContentsMargins(16, 14, 16, 14)",
    "setSpacing(8)",
    "border-radius: 6px",
    "neutral_rgba(0.04)",
    "neutral_rgba(0.14)",
    "border-radius: 8px; padding: 10px",
    "neutral_rgba(0.1)",
    "neutral_rgba(0.25)",
    "background-color: rgba(59,130,246,0.30)",
    "background-color: rgba(59,130,246,0.45)",
    "background-color: rgba(59,130,246,0.60)",
    "border: 1px solid rgba(59,130,246,0.45)",
    "padding: 6px 14px",
    "neutral_rgba(0.15)",
    "neutral_rgba(0.25)",
    "neutral_rgba(0.7)",
    "padding: 6px 12px",
    "neutral_rgba(0.12)",
    "neutral_rgba(0.6)",
    "setContentsMargins(8, 4, 8, 4)",
    "setSpacing(8)",
    "border-radius: 4px",
    "background-color: rgba(59,130,246,0.22)",
    "setMinimumWidth(150)",
    "setMinimumWidth(190)",
    "setMaximumWidth(200)",
    "setMinimumWidth(60)",
    "setMinimumWidth(60)",
    "setMinimumWidth(48)",
    "setMaximumWidth(160)",
    "setMinimumWidth(42)",
    "setContentsMargins(0, 4, 0, 0)",
    "setSpacing(4)",
    "setContentsMargins(8, 4, 8, 4)",
    "background-color: rgba(255,255,255,0.05)",
    "setFixedWidth(40)",
    "rgba(220,80,80,0.25)",
    "rgba(220,80,80,0.4)",
    "padding: 4px 6px",
    "setSpacing(8)",
    "setContentsMargins(0, 4, 0, 0)",
    "border: 1px solid rgba(255,255,255,0.12)",
    "border-color: rgba(255,255,255,0.30)",
    "border-radius: 6px",
    "setContentsMargins(10, 8, 10, 8)",
    "setSpacing(2)",
    "setSpacing(4)",
    "setFixedWidth(36)",
    "font-size: 26pt",
    "background-color: rgba(255,255,255,0.05)",
    "padding: 8px;",
    "setSpacing(6)",
    "setSpacing(6)",
    "setSpacing(2)",
    "setSpacing(6)",
    "setSpacing(0)",
];

/// Guard of agreement between the WIDGET toast banner and what goes into QML.
/// The model is `widget_qss_and_qml_control_colors_agree` of the «Оформление» tab.
///
/// ⚠️ WHY. The banner lies ON TOP of the content, its tone is not taken from
/// the skin palette, and Qt itself paints nothing on it: both sides draw it
/// entirely by hand. While QML took paints via tokens
/// (`bg_secondary`/`border`/`text_primary`), the banner came out opaque instead
/// of translucent, with radius 6 instead of 8 and at the bottom CENTRE, while
/// the widget one sits at the bottom LEFT. The guard holds three places at
/// once: the numbers inside the style sheet, the door into QML
/// (`styles().toast`) and the markup itself.
#[test]
fn toast_qss_and_qml_toast_style_agree() {
    use rexon_core::ui_base;

    // ── 1. The widget banner's style sheet is built from the door ────────
    let prop = |body: &str, name: &str| -> String {
        body.split(';')
            .map(str::trim)
            .find_map(|part| part.strip_prefix(&format!("{name}:")))
            .unwrap_or_else(|| panic!("в таблице стилей тоста нет свойства {name}"))
            .trim()
            .to_string()
    };
    for dark in [true, false] {
        let qss = ui_base::toast_qss(dark);
        let c = ui_base::toast_colors(dark);
        let body = qss
            .split_once('{')
            .expect("таблица стилей тоста без тела правила")
            .1
            .rsplit_once('}')
            .expect("правило тоста не закрыто")
            .0;
        let body = nonempty(body, "тело правила таблицы стилей плашки");
        assert_eq!(prop(body, "background-color"), c.background, "заливка");
        assert_eq!(prop(body, "color"), c.text, "надпись");
        assert_eq!(
            prop(body, "border"),
            format!("{}px solid {}", ui_base::TOAST_BORDER_WIDTH, c.border),
            "рамка"
        );
        assert_eq!(
            prop(body, "border-radius"),
            format!("{}px", ui_base::TOAST_RADIUS),
            "скругление"
        );
        assert_eq!(
            prop(body, "padding"),
            format!(
                "{}px {}px",
                ui_base::TOAST_PADDING_V,
                ui_base::TOAST_PADDING_H
            ),
            "поля"
        );
    }
    // Dark and light must DIFFER: identical paints would mean the door lost
    // track of the theme and one of the themes became unreadable.
    assert_ne!(
        ui_base::toast_colors(true).background,
        ui_base::toast_colors(false).background,
        "краски тоста перестали зависеть от тёмности темы"
    );

    // ── 2. There is ONE door for the whole product ───────────────────────
    //
    // ⚠️ The guard looked here at the old address and failed when the `toast`
    // group was moved from the «Настройки» page into a common module (the commit
    // about one banner for all tabs). The intent was not violated but
    // strengthened: the number of copies of the numbers went from two to zero.
    // So the address was fixed and the rule tightened — now it also checks that
    // nobody goes around the door.
    let door = read_sample(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../rexon-gui/src/lib.rs"),
        "исходник крейта GUI",
    );
    let toast_mod = door
        .split_once("pub mod toast {")
        .expect("в крейте GUI нет модуля `toast`: у плашки снова нет общей двери")
        .1
        .split_once("\n}")
        .expect("модуль `toast` не закрыт")
        .0;
    let toast_mod = nonempty(toast_mod, "модуль `toast` крейта GUI");
    for needle in [
        "ui_base::toast_colors(dark)",
        "col(skin.background)",
        "col(skin.border)",
        "col(skin.text)",
        "ui_base::TOAST_BORDER_WIDTH",
        "ui_base::TOAST_RADIUS",
        "ui_base::TOAST_PADDING_H",
        "ui_base::TOAST_PADDING_V",
        "ui_base::TOAST_MARGIN",
        "ui_base::TOAST_TIMEOUT_MS",
        "ui_base::toast_width(",
        "ui_base::TOAST_TEXT_SLACK",
        "ui_base::TOAST_MAX_WIDTH",
    ] {
        assert!(
            toast_mod.contains(needle),
            "дверь плашки не берёт «{needle}» из ядра — у плашки завёлся свой набор чисел"
        );
    }

    // ── 3. Both pages go THROUGH the door, not around it ─────────────────
    //
    // The "message never fades" defect was introduced three times, each time
    // because a tab set up the banner anew. That is what is guarded here: the
    // page must take paints, timeout and width from the door and must not know
    // a single banner constant.
    for (name, file) in [
        ("Настройки", "/../rexon-gui/src/settings_page.rs"),
        ("Steam", "/../rexon-gui/src/steam_page.rs"),
    ] {
        let src = read_sample(
            &format!("{}{}", env!("CARGO_MANIFEST_DIR"), file),
            &format!("исходник страницы «{name}»"),
        );
        // The prohibitions below are checked against CODE, not prose: the page must
        // explain in a comment where the banner's numbers come from ("the timeout
        // comes from `ui_base::TOAST_TIMEOUT_MS`"), and punishing it for that would
        // mean forbidding the explanation itself.
        let code: String = src
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // ⚠️ Here it is, the empty derived sample: the prohibitions below look
        // EXACTLY here, and with comments alone they would have nothing to search.
        nonempty(&code, &format!("код страницы «{name}» без комментариев"));
        for needle in [
            "crate::toast::style_map(",
            "crate::toast::timeout_ms()",
            "crate::toast::width(",
        ] {
            assert!(
                src.contains(needle),
                "страница «{name}» не ходит через дверь плашки: нет «{needle}»"
            );
        }
        // The page cannot have its own paints and its own numbers — that is what
        // the guard is for.
        for forbidden in [
            "ui_base::toast_colors",
            "ui_base::TOAST_RADIUS",
            "ui_base::TOAST_PADDING_H",
            "ui_base::TOAST_MARGIN",
            "ui_base::TOAST_TIMEOUT_MS",
        ] {
            assert!(
                !code.contains(forbidden),
                "страница «{name}» снова красит плашку сама: «{forbidden}»"
            );
        }
    }

    // ── 4. The banner markup takes everything from the door's skin ───────
    let qml = read_sample(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../rexon-gui/qml/RexonToast.qml"),
        "общая разметка плашки",
    );
    for needle in [
        "required property var skin",
        "required property int timeoutMs",
        "required property var metrics",
        "color: plate.skin.background",
        "border.color: plate.skin.border",
        "border.width: plate.skin.borderWidth",
        "radius: plate.skin.radius",
        "color: plate.skin.textColor",
        "anchors.leftMargin: plate.skin.margin",
        "anchors.bottomMargin: plate.skin.margin",
        "plate.metrics.toastWidth(",
        // ⭐ This line is why the guard was rewritten: without the timer the
        // banner never fades, and that is exactly how it hung for 29 minutes.
        "interval: plate.timeoutMs",
    ] {
        assert!(
            qml.contains(needle),
            "разметка плашки потеряла «{needle}»"
        );
    }
    // Palette tokens and "centre anchoring" cannot be in the banner: both are
    // exactly the door that was closed.
    for forbidden in ["root.pal", "root.col", "anchors.horizontalCenter", "anchors.centerIn"] {
        assert!(
            !qml.contains(forbidden),
            "плашка снова рисует себя сама: «{forbidden}»"
        );
    }

    // ── 5. Tabs place exactly it, not a rectangle of their own ───────────
    for (name, file) in [
        ("Настройки", "/../rexon-gui/qml/SettingsPage.qml"),
        ("Steam", "/../rexon-gui/qml/SteamPage.qml"),
    ] {
        let page_qml = read_sample(
            &format!("{}{}", env!("CARGO_MANIFEST_DIR"), file),
            &format!("разметка вкладки «{name}»"),
        );
        for needle in ["RexonToast {", "timeoutMs: page.toastTimeoutMs", "metrics: page"] {
            assert!(
                page_qml.contains(needle),
                "вкладка «{name}» ставит не общую плашку: нет «{needle}»"
            );
        }
    }
}

/// Words after which an unrecognized desktop's name must be in an inflected
/// case. Written out here anew, not taken from the core: a check against the
/// list it itself verifies would verify nothing.
const CASE_WORDS: &[&str] = &[
    "для", "на", "в", "во", "у", "с", "со", "к", "ко", "о", "об", "из", "от", "под", "при",
    "по", "до", "без", "окружения", "приложения",
];

/// The text, if the unrecognized desktop's name in it stands after a
/// preposition or in quotes — that is, substituted right where a case is needed.
fn unknown_de_after_preposition(text: &str) -> Option<String> {
    let name = rexon_core::screenshots::unknown_de_name();
    let name = name.as_str();
    let mut from = 0;
    while let Some(off) = text[from..].find(name) {
        let pos = from + off;
        let head = &text[..pos];
        // Quotes by themselves do not require a case («Применяю «{0}»» is a
        // wallpaper name); only a desktop name in quotes after a word is caught.
        if head.to_lowercase().ends_with("окружения «") {
            return Some(text.to_string());
        }
        let head = head.strip_suffix('«').unwrap_or(head);
        if let Some(before) = head.strip_suffix(' ') {
            let word: String = before
                .chars()
                .rev()
                .take_while(|c| c.is_alphabetic())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if CASE_WORDS.contains(&word.to_lowercase().as_str()) {
                return Some(text.to_string());
            }
        }
        from = pos + name.len();
    }
    None
}

/// ⭐ An unrecognized desktop's name is not substituted as is into a phrase
/// with a preposition.
///
/// Defect: «Системная звуковая тема для ВАШ РАБОЧИЙ СТОЛ». The name placeholder
/// is declinable, but substitution was a plain replace. The check runs the
/// WHOLE tab catalog, all sections, the application restore result and the
/// wallpaper engine refusal with the unrecognized desktop's name at every
/// substitution point.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn unknown_desktop_is_never_put_after_a_preposition() {
    let _lang = i18n_pin::ru();
    let name = rexon_core::screenshots::unknown_de_name();
    assert_eq!(name, "ВАШ РАБОЧИЙ СТОЛ");
    let name = name.as_str();
    // The check itself catches what it must catch, and nothing extra.
    assert!(unknown_de_after_preposition(&format!("для {name}")).is_some());
    assert!(unknown_de_after_preposition(&format!("Для окружения «{name}»")).is_some());
    assert!(unknown_de_after_preposition(&format!("Текущее окружение: <b>{name}</b>")).is_none());

    let mut texts: Vec<String> = Vec::new();
    let args = vec![name.to_string(); 4];
    for (key, _) in qs::TEXTS {
        if qs::text_template(key).unwrap().contains("%1") {
            texts.push(qs::text(key, &args).unwrap());
        }
    }
    for id in qs::section_order() {
        texts.push(qs::section_texts(id, name).unwrap().1);
    }
    let unknown = rexon_core::screenshots::display_name("");
    texts.push(qs::restore_native_message(1, &Vec::<String>::new(), &unknown));
    texts.push(qs::restore_native_message(2, &["Терминал".to_string()], name));
    for de in ["", "unknown", "generic"] {
        for step in
            rexon_core::wallpaper_engine::static_plan(de, "wayland", "/tmp/a.png", &[], "swww")
        {
            texts.push(step.err_message);
        }
    }
    for text in &texts {
        assert!(
            unknown_de_after_preposition(text).is_none(),
            "имя неопознанного стола подставлено после предлога: {text}"
        );
    }

    // The correct forms really are in place.
    let (_, sound) = qs::section_texts("soundThemeGroup", name).unwrap();
    assert!(sound.contains("для вашего рабочего стола"), "{sound}");
    let quick = qs::text("quickfix.unknown_de", &[name.to_string()]).unwrap();
    assert!(quick.starts_with("Для вашего рабочего стола набор"), "{quick}");
    assert!(texts.iter().any(|t| t.contains("на вашем рабочем столе")), "отказ обоев не склонён");
    // A known name is substituted as is.
    assert_eq!(
        qs::text("quickfix.button", &["KDE Plasma".to_string()]).unwrap(),
        "Восстановить приложения KDE Plasma"
    );
    assert_eq!(rexon_core::screenshots::display_name("unknown"), name);
}

/// ⭐ The background sound failure names the real cause: «нужен mpv» only when
/// mpv really is missing; when it is installed — that the file does not play.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn ambient_failure_names_the_real_reason() {
    let _lang = i18n_pin::ru();
    let without = qs::ambient_start_failed_status(false);
    let with = qs::ambient_start_failed_status(true);
    assert!(without.contains("mpv"), "{without}");
    assert!(!with.contains("mpv"), "при установленном mpv просят mpv: {with}");
    assert_ne!(with, without);
    assert!(!with.is_empty());
}

/// ⭐ Volume change result: the sound's name, not a service identifier; a
/// sound that did not survive a restart is a failure, not «громкость задана»;
/// the sound is gone — and so it says.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn ambient_volume_status_tells_the_truth() {
    let _lang = i18n_pin::ru();
    let ok = qs::ambient_volume_status(Some("дождь.ogg"), 30, true, true);
    assert_eq!(ok, "🔊 Громкость «дождь.ogg» → 30%");
    let failed = qs::ambient_volume_status(Some("дождь.ogg"), 30, false, true);
    assert_eq!(failed, qs::ambient_start_failed_status(true));
    let gone = qs::ambient_volume_status(None, 30, false, true);
    assert!(!gone.is_empty() && gone != ok && gone != failed, "{gone}");
    assert!(!ok.contains("manual_"), "в ответ попал идентификатор: {ok}");
}

/// ⭐ The sound stop result names the sound, and does not report «остановлен»
/// about a sound that is already gone.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn ambient_stopped_status_tells_the_truth() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::ambient_stopped_status(Some("дождь.ogg")), "⏹️ Остановлен: дождь.ogg");
    let gone = qs::ambient_stopped_status(None);
    assert!(!gone.contains("Остановлен"), "о пропавшем звуке сказано «остановлен»: {gone}");
    assert_eq!(gone, qs::ambient_volume_status(None, 50, false, true));
}

// ============================================================================
// Translation: Russian as before, English without a single Russian letter
// ============================================================================

/// The battery and game checkboxes of the «Обои» section — in Russian and in human words.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn wallpaper_pause_texts_russian() {
    let _lang = i18n_pin::ru();
    let get = |k: &str| qs::text(k, &[]).unwrap();
    assert_eq!(get("wp.pause_battery"), "Ставить обои на паузу при работе от батареи");
    assert_eq!(get("wp.pause_game"), "Ставить обои на паузу во время игры (игровой режим)");
}

/// The «Ссылки на сайты» section (the question before an external link, 16.11.2026).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn links_texts_russian() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::text("links.reset", &[]).unwrap(), "Снова спрашивать для всех сайтов");
    let (_, title, _) = qs::SECTIONS.iter().find(|(id, _, _)| *id == "linksGroup").unwrap();
    assert_eq!(i18n::tr(title), "🔗 Ссылки на сайты");
}

/// Snapshot of the tab's Russian texts before the strings moved to the catalog.
fn ru_before() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/qt_settings_ru_before.json"
    );
    serde_json::from_str(&read_sample(path, "снимок qt_settings_ru_before.json"))
        .expect("снимок не разбирается как JSON")
}

/// ⭐ NOTHING changed for the Russian interface: every catalog label, every
/// section and every assembled phrase of the tab is word for word the same as
/// in the code before the migration (the snapshot was taken from that day's
/// `qt_settings.rs`).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn russian_catalog_keeps_old_texts() {
    let _lang = i18n_pin::ru();
    let before = ru_before();

    let texts = before["texts"].as_object().unwrap();
    assert_eq!(
        texts.len() + AFTER_RU_SNAPSHOT_TEXTS.len() - TEXT_REMOVALS.len(),
        qs::TEXTS.len(),
        "ключи каталога разошлись со снимком"
    );
    for (key, want) in texts {
        if TEXT_REMOVALS.contains(&key.as_str()) {
            continue;
        }
        assert_eq!(
            qs::text_template(key).as_deref(),
            Some(want.as_str().unwrap()),
            "подпись «{key}»"
        );
    }

    let sections = before["sections"].as_object().unwrap();
    assert_eq!(
        sections.len() + AFTER_RU_SNAPSHOT_SECTIONS.len() - SECTION_REMOVALS.len(),
        qs::SECTIONS.len()
    );
    for (id, title, desc) in qs::SECTIONS {
        if AFTER_RU_SNAPSHOT_SECTIONS.contains(id) {
            assert!(!sections.contains_key(*id), "{id} помечен новым, а в снимке он был");
            continue;
        }
        let want = &sections[*id];
        if !title.is_empty() {
            assert_eq!(i18n::tr(title), want[0].as_str().unwrap(), "заголовок {id}");
        }
        if let Some((_, now)) = AFTER_RU_SNAPSHOT_REWORDED.iter().find(|(k, _)| k == id) {
            assert_ne!(want[1].as_str().unwrap(), *now, "{id}: снимок уже знает новое описание");
            assert_eq!(i18n::tr(desc), *now, "описание {id}");
            continue;
        }
        assert_eq!(i18n::tr(desc), want[1].as_str().unwrap(), "описание {id}");
    }

    let ru = i18n_pin::ru_catalog();
    for pair in before["phrases"].as_array().unwrap() {
        let msgid = pair[0].as_str().unwrap();
        assert_eq!(ru.get(None, msgid), pair[1].as_str(), "фраза «{msgid}»");
    }

    // Assembled strings — whole, as the person saw them.
    assert_eq!(qs::pretty_backup_ts("20260620_011005"), "20.06.2026 в 01:10");
    assert_eq!(
        qs::restore_native_message(2, &["Терминал".to_string()], "KDE"),
        "Восстановлено 2 приложения для KDE, не нашлось в системе: Терминал"
    );
    assert_eq!(
        qs::restore_native_message(5, &[], "KDE"),
        "Восстановлено 5 приложений для KDE"
    );
    assert_eq!(
        qs::msg_no_event_in_theme("ocean"),
        "В теме «ocean» нет звука для этого события"
    );
    assert_eq!(qs::wallpaper_dialog_title("static"), "Выбрать: картинка");
    assert_eq!(qs::wallpaper_dialog_title("auto"), "Выбрать: обои");
    assert_eq!(qs::event_meta("complete"), ("✅".to_string(), "Готово".to_string()));
    assert_eq!(
        qs::fm_choices()[0].1,
        "⚙️  Автоматически (по рабочему столу)"
    );
    assert_eq!(rexon_core::constants::type_label("theme-gtk").as_deref(), Some("Тема GTK"));
}

/// The tab's English interface: not a single Russian letter in the catalog,
/// sections or assembled phrases — a forgotten string shows up here, not to
/// the buyer.
///
/// There is one named exception: the unrecognized desktop's name
/// (`screenshots::UNKNOWN_DE_NAME`) lives in another module and is not part of
/// this tab's translation — so a known name is substituted.
#[test]
fn english_tab_has_no_russian() {
    let _lang = i18n_pin::en();
    let cyrillic = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
    let args = vec!["KDE".to_string(), "x".to_string(), "y".to_string()];
    let mut all: Vec<String> = Vec::new();
    for (key, _) in qs::TEXTS {
        all.push(qs::text(key, &args).unwrap());
    }
    for id in qs::section_order() {
        let (title, desc) = qs::section_texts(id, "KDE").unwrap();
        all.push(desc);
        // The onboarding section title belongs to the onboarding itself (already translated).
        all.push(title);
    }
    for (_, label) in qs::fm_choices() {
        all.push(label);
    }
    for (id, _, _) in qs::SOUND_EVENT_META {
        all.push(qs::event_meta(id).1);
    }
    for target in qs::FONT_TARGETS {
        let (a, b, c) = qs::font_target_texts(target);
        all.extend([a, b, c]);
    }
    for (label, _) in qs::yt_qualities().into_iter().chain(qs::yt_modes()) {
        all.push(label);
    }
    for kind in ["static", "video", "swf", "auto"] {
        all.push(qs::wallpaper_dialog_filter(kind));
        all.push(qs::wallpaper_dialog_title(kind));
    }
    let pairs = |p: &[(&str, &str)]| -> Vec<(String, String)> {
        p.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    };
    all.extend([
        qs::ambient_file_filter(),
        qs::msg_no_event_in_theme("ocean"),
        qs::pretty_backup_ts("20260620_011005"),
        qs::backup_row_label("20260620_011005", "manual", 3),
        qs::backup_row_label("20260620_011005", "before_install", 1),
        qs::compliance_summary(&[]),
        qs::compliance_summary(&pairs(&[("a", "ok")])),
        qs::compliance_summary(&pairs(&[
            ("a", "fixed"),
            ("b", "failed"),
            ("c", "needs_action"),
            ("d", "skipped"),
        ])),
        qs::wallpaper_status_text(false, None, None),
        qs::wallpaper_status_text(true, None, Some("/a/b.mp4")),
        qs::restore_native_message(2, &["Terminal".to_string()], "KDE"),
        qs::protection_status_text(0, &[]),
        qs::protection_status_text(1, &[]),
        qs::protection_status_text(
            1,
            &[("Browser".to_string(), "a".to_string(), "b".to_string())],
        ),
        qs::youtube_starting_status("best", "stream", true),
        qs::plasma_restore_confirm("20260620_011005").1,
        qs::onboarding_replay_button(),
    ]);
    for text in &all {
        assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
    }
    for (t, label) in rexon_core::constants::type_labels_ordered() {
        let shown = rexon_core::constants::type_label(t).unwrap();
        assert_eq!(shown, *label, "{t}: исходный язык — сам msgid");
        assert!(!cyrillic(&shown), "{t}: {shown}");
    }
    for f in rexon_core::constants::WALLPAPER_FORMATS {
        let shown = rexon_core::constants::wallpaper_format_label(f).unwrap();
        assert!(!cyrillic(&shown), "{f}: {shown}");
    }
}

/// English wording assembled from parts reads as phrases.
#[test]
fn english_phrases_read_naturally() {
    let _lang = i18n_pin::en();
    assert_eq!(qs::pretty_backup_ts("20260620_011005"), "20.06.2026 at 01:10");
    assert_eq!(
        qs::backup_row_label("20260620_011005", "manual", 1),
        "📁 20.06.2026 at 01:10  ·  manually  ·  1 file"
    );
    assert_eq!(
        qs::backup_row_label("20260620_011005", "before_install", 11),
        "📁 20.06.2026 at 01:10  ·  automatically, before installing  ·  11 files"
    );
    assert_eq!(
        qs::compliance_summary(&[
            ("a".to_string(), "fixed".to_string()),
            ("b".to_string(), "failed".to_string()),
        ]),
        "⚠️ Installed: 1, failed: 1"
    );
    assert_eq!(
        qs::restore_native_message(1, &[], "KDE"),
        "Restored 1 app for KDE"
    );
    assert_eq!(
        qs::restore_native_message(3, &["Terminal".to_string()], "GNOME"),
        "Restored 3 apps for GNOME, not found on the system: Terminal"
    );
    assert_eq!(
        qs::text("quickfix.button", &["KDE Plasma".to_string()]).unwrap(),
        "Restore KDE Plasma Apps"
    );
    let (_, desc) = qs::section_texts("soundThemeGroup", "COSMIC").unwrap();
    assert!(desc.starts_with("System sound theme for COSMIC:"), "{desc}");
    assert!(qs::wallpaper_dialog_filter("static").starts_with("Images (*.jpg"));
    // An unrecognized desktop — the fallback name in English, same case.
    assert_eq!(qs::de_display("unknown", "", ""), "YOUR DESKTOP");
    assert_eq!(qs::de_display("unknown", "niri:wlroots", ""), "NIRI");
    assert_eq!(qs::wallpaper_dialog_title("video"), "Choose a video");
    assert_eq!(qs::font_apply_status(false, "System Font", "", 0), "⚠️ System Font: could not apply");
    assert_eq!(
        qs::youtube_starting_status("best", "stream", false),
        "⏳ YouTube (Best available, Stream, sound off) — this may take a while…"
    );
    // The button in the advice is the same as on screen.
    assert!(qs::protection_status_text(0, &[]).contains("“📌 Remember Now”"));
    assert_eq!(qs::event_meta("device-added").1, "Device connected");
}

// ============================================================================
// The «Язык» section
// ============================================================================

/// Russian labels of the «Язык» section — exactly as they were approved.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn language_section_russian() {
    let _lang = i18n_pin::ru();
    let (title, desc) = qs::section_texts("langGroup", "KDE").unwrap();
    assert_eq!(title, "🌐 Язык");
    assert_eq!(desc, "Язык окон, меню и сообщений Rexon");
    let one = |key: &str, a: &str| qs::text(key, &[a.to_string()]).unwrap();
    assert_eq!(one("lang.row", ""), "<b>Язык:</b>");
    assert_eq!(one("lang.system", "Русский"), "Как в системе (Русский)");
    assert_eq!(one("lang.restart_hint", ""), "Язык сменится после перезапуска Rexon.");
    assert_eq!(one("lang.restart", ""), "Перезапустить Rexon");
    assert_eq!(one("lang.saved", "English"), "🌐 Язык сохранён: English");
    assert_eq!(
        qs::language_restart_failed("нет файла"),
        "⚠️ Не удалось перезапустить Rexon: нет файла"
    );
}

/// The system-change permission bar (B13): in Russian and word for word as the
/// icon menu items — it is one state and one action.
#[test]
fn admin_strip_russian_matches_tray() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::text_template("admin.info").as_deref(), Some("Rexon может изменять систему до закрытия окна"));
    assert_eq!(qs::text_template("admin.revoke").as_deref(), Some("Запретить сейчас"));
    assert_eq!(qs::text_msgid("admin.info"), rexon_core::tray::label_msgid("admin.info"));
    assert_eq!(qs::text_msgid("admin.revoke"), rexon_core::tray::label_msgid("admin.revoke"));
}

/// English labels of the «Язык» section.
#[test]
fn language_section_english() {
    let _lang = i18n_pin::en();
    let (title, desc) = qs::section_texts("langGroup", "KDE").unwrap();
    assert_eq!(title, "🌐 Language");
    assert_eq!(desc, "The language of Rexon's windows, menus and messages");
    let one = |key: &str, a: &str| qs::text(key, &[a.to_string()]).unwrap();
    assert_eq!(one("lang.system", "English"), "Same as system (English)");
    assert_eq!(one("lang.restart_hint", ""), "The language will change after Rexon restarts.");
    assert_eq!(one("lang.restart", ""), "Restart Rexon");
}

/// The language list: "as in the system" first, then English, each language
/// by its own name, not a translation (otherwise a person who accidentally
/// switched to a foreign language would not find their own).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn language_choices_use_native_names() {
    let _lang = i18n_pin::ru();
    let choices = qs::language_choices();
    assert_eq!(choices[0].0, "system");
    assert!(choices[0].1.starts_with("Как в системе ("), "{:?}", choices[0]);
    assert_eq!(choices[1], ("en".to_string(), "English".to_string()));
    // The check built the Russian catalog itself — so «Русский» is in the list.
    assert!(choices.contains(&("ru".to_string(), "Русский".to_string())), "{choices:?}");
    assert!(qs::language_choice_valid("en"));
    assert!(qs::language_choice_valid("system"));
    assert!(!qs::language_choice_valid("xx"));
    assert_eq!(qs::language_saved_status("en"), "🌐 Язык сохранён: English");
    drop(_lang);
    let _lang = i18n_pin::en();
    let choices = qs::language_choices();
    assert_eq!(choices[1].1, "English");
    assert!(choices.contains(&("ru".to_string(), "Русский".to_string())));
    assert!(choices[0].1.starts_with("Same as system ("));
}

/// A restart for the language reopens the window on «Настройки» and does not
/// hide it in the tray.
#[test]
fn restart_args_reopen_settings() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(qs::restart_args(&[]), s(&["--section=settings"]));
    assert_eq!(
        qs::restart_args(&s(&["--tray", "--section=steam", "--page=compare", "--selftest", "-v"])),
        s(&["-v", "--section=settings"])
    );
}

/// D5 (environment check 02.10.2026): the interface font is carried into
/// `~/.config/gtk-3.0` and `gtk-4.0`, not into `gtk-gtk-3.0` — GTK does not read
/// that directory, and GTK programs on LXQt/X11 and XWayland did not get the font.
#[test]
fn gtk_font_goes_to_real_gtk_dirs() {
    assert_eq!(
        qs::gtk_font_ini_paths("/h/.config/"),
        ["/h/.config/gtk-3.0/settings.ini", "/h/.config/gtk-4.0/settings.ini"]
    );
    assert_eq!(
        qs::stray_gtk_font_dirs("/h/.config"),
        [
            ("/h/.config/gtk-gtk-3.0".to_string(), "/h/.config/gtk-gtk-3.0/settings.ini".to_string()),
            ("/h/.config/gtk-gtk-4.0".to_string(), "/h/.config/gtk-gtk-4.0/settings.ini".to_string()),
        ]
    );
}

/// The junk directory of the former entry is removed only if it holds exactly
/// our file: one `settings.ini` with one `gtk-font-name` line.
#[test]
fn stray_gtk_font_dir_removed_only_if_ours() {
    let only = vec!["settings.ini".to_string()];
    let ours = "[Settings]\ngtk-font-name=Noto Serif 13\n";
    assert!(qs::stray_gtk_font_dir_removable(&only, Some(ours)));
    // An extra file next to it — the directory is not ours.
    assert!(!qs::stray_gtk_font_dir_removable(
        &["settings.ini".to_string(), "gtk.css".to_string()],
        Some(ours)
    ));
    // An extra key — someone edited it by hand.
    assert!(!qs::stray_gtk_font_dir_removable(
        &only,
        Some("[Settings]\ngtk-font-name=Noto Serif 13\ngtk-theme-name=Adwaita\n")
    ));
    assert!(!qs::stray_gtk_font_dir_removable(&only, Some("[Other]\ngtk-font-name=X 1\n")));
    assert!(!qs::stray_gtk_font_dir_removable(&only, None));
    assert!(!qs::stray_gtk_font_dir_removable(&[], Some(ours)));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Sound theme existence before applying (03.10.2026): live, kdeglobals had
//  the theme «Sounds», which does not exist — the tab said «применена», and
//  the system silently played freedesktop.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sound_theme_exists_only_on_top_level_with_index() {
    let base = std::env::temp_dir().join(format!("rexon_snd_exists_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let put = |rel: &str, text: &str| {
        let p = base.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    put("user/sounds/Ocean/index.theme", "[Sound Theme]\nName=Ocean\n");
    put("user/sounds/Sounds/Zorin/index.theme", "[Sound Theme]\nName=Zorin\n");
    put("user/sounds/NoIndex/stereo/bell.oga", "x");
    put("user/sounds/IconLike/index.theme", "[Icon Theme]\nName=x\n");
    put("sys/sounds/freedesktop/index.theme", "[Sound Theme]\nName=Default\n");
    let dirs = vec![
        base.join("user/sounds").to_string_lossy().into_owned(),
        base.join("sys/sounds").to_string_lossy().into_owned(),
    ];
    assert!(qs::sound_theme_exists(&dirs, "Ocean"));
    assert!(qs::sound_theme_exists(&dirs, "freedesktop"), "второй каталог XDG тоже ищется");
    for bad in ["Sounds", "Zorin", "NoIndex", "IconLike", "", "Sounds/Zorin", "..", "Нет"] {
        assert!(!qs::sound_theme_exists(&dirs, bad), "«{bad}» не тема на верхнем уровне");
    }
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn xdg_sound_theme_dirs_follow_spec_order() {
    assert_eq!(
        qs::xdg_sound_theme_dirs("/h/.local/share/", None),
        ["/h/.local/share/sounds", "/usr/local/share/sounds", "/usr/share/sounds"]
    );
    assert_eq!(
        qs::xdg_sound_theme_dirs("/h/d", Some("/opt/share:/usr/share::/opt/share/")),
        ["/h/d/sounds", "/opt/share/sounds", "/usr/share/sounds"]
    );
}

#[test]
fn missing_sound_theme_message_names_the_theme() {
    let _lang = i18n_pin::ru();
    let msg = qs::sound_theme_missing_message("Sounds");
    assert!(msg.contains("«Sounds»"), "{msg}");
    assert!(!msg.contains("примен"), "это отказ, а не «применена»: {msg}");
}

// ── Font rendering: antialiasing, subpixels, hinting ─────────────────────

/// Native rendering keys per environment. Sources: gsettings schemas
/// (`org.gnome.desktop.interface`, `org.cinnamon.settings-daemon.plugins.
/// xsettings`, `org.mate.font-rendering`), the XFCE `xsettings` channel
/// (`xfce4-appearance-settings`) and `kdeglobals [General]` of the Plasma 6
/// «Шрифты» page.
#[test]
fn font_rendering_native_keys_per_desktop() {
    use qs::{RenderDialect, RenderKey, SoundBackend};
    let names = |de: &str| -> Option<(RenderDialect, Vec<(RenderKey, String)>)> {
        qs::render_native_store(de).map(|s| {
            (
                s.dialect,
                s.keys
                    .iter()
                    .map(|(k, b)| {
                        let name = match b {
                            SoundBackend::GSettings { schema, key } => format!("{schema} {key}"),
                            SoundBackend::Xfconf { channel, prop } => format!("{channel} {prop}"),
                            SoundBackend::KConfig { file, group, key } => format!("{file} [{group}] {key}"),
                            SoundBackend::Dde { .. } => "dde".to_string(),
                        };
                        (*k, name)
                    })
                    .collect(),
            )
        })
    };
    let (d, keys) = names("kde").unwrap();
    assert_eq!(d, RenderDialect::Kde);
    assert_eq!(
        keys,
        vec![
            (RenderKey::Antialias, "kdeglobals [General] XftAntialias".to_string()),
            (RenderKey::HintStyle, "kdeglobals [General] XftHintStyle".to_string()),
            (RenderKey::SubPixel, "kdeglobals [General] XftSubPixel".to_string()),
        ]
    );
    let (d, keys) = names("xfce").unwrap();
    assert_eq!(d, RenderDialect::Xfce);
    assert_eq!(
        keys.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>(),
        ["xsettings /Xft/Antialias", "xsettings /Xft/Hinting", "xsettings /Xft/HintStyle", "xsettings /Xft/RGBA"]
    );
    for (de, schema, prefix) in [
        ("gnome", "org.gnome.desktop.interface", "font-"),
        ("budgie", "org.gnome.desktop.interface", "font-"),
        ("pantheon", "org.gnome.desktop.interface", "font-"),
        ("cinnamon", "org.cinnamon.settings-daemon.plugins.xsettings", ""),
        ("cinnamon-mint", "org.cinnamon.settings-daemon.plugins.xsettings", ""),
        ("mate", "org.mate.font-rendering", ""),
    ] {
        let (d, keys) = names(de).unwrap();
        assert_eq!(d, RenderDialect::Gnome, "{de}");
        assert_eq!(
            keys.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>(),
            ["antialiasing", "hinting", "rgba-order"]
                .iter()
                .map(|k| format!("{schema} {prefix}{k}"))
                .collect::<Vec<_>>(),
            "{de}"
        );
    }
    // No page of its own — fonts.conf itself is the native store.
    for de in ["lxqt", "cosmic", "deepin", "hyprland", "sway", "niri", "unknown", ""] {
        assert!(qs::render_native_store(de).is_none(), "{de}");
    }
}

/// Values in each store's entry and the common GNOME layer for GTK.
#[test]
fn font_rendering_plan_values() {
    let r = qs::FontRendering {
        antialias: true,
        exclude: None,
        subpixel: "bgr".to_string(),
        hinting: "full".to_string(),
    };
    let plan = qs::rendering_plan("xfce", &r, "", true);
    let xf = |prop: &str, ty: &str, v: &str| -> Vec<String> {
        ["xfconf-query", "-c", "xsettings", "-p", prop, "-n", "-t", ty, "-s", v]
            .iter()
            .map(|s| s.to_string())
            .collect()
    };
    assert_eq!(
        plan.primary,
        vec![
            xf("/Xft/Antialias", "int", "1"),
            xf("/Xft/Hinting", "int", "1"),
            xf("/Xft/HintStyle", "string", "hintfull"),
            xf("/Xft/RGBA", "string", "bgr"),
        ]
    );
    let gs = |k: &str, v: &str| -> Vec<String> {
        ["gsettings", "set", "org.gnome.desktop.interface", k, v]
            .iter()
            .map(|s| s.to_string())
            .collect()
    };
    assert_eq!(
        plan.also,
        vec![gs("font-antialiasing", "rgba"), gs("font-hinting", "full"), gs("font-rgba-order", "bgr")]
    );
    // Without gsettings the common layer is not offered.
    assert!(qs::rendering_plan("xfce", &r, "", false).also.is_empty());
    // In GNOME the common layer is the native one — no duplicate.
    let gnome = qs::rendering_plan("gnome", &r, "", true);
    assert_eq!(gnome.primary.len(), 3);
    assert!(gnome.also.is_empty());
    // KDE: without kwriteconfig6 nothing to write with, with it — kdeglobals with a notification.
    assert!(qs::rendering_plan("kde", &r, "", true).primary.is_empty());
    let kde = qs::rendering_plan("kde", &r, "/usr/bin/kwriteconfig6", true);
    assert_eq!(
        kde.primary[1],
        ["/usr/bin/kwriteconfig6", "--file", "kdeglobals", "--group", "General", "--key", "XftHintStyle", "--notify", "hintfull"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
    // No antialiasing: GNOME "none", subpixels removed.
    let off = qs::FontRendering { antialias: false, ..r.clone() }.normalized();
    assert_eq!(off.subpixel, "none");
    assert_eq!(off.gnome_antialiasing(), "none");
    let gray = qs::FontRendering { subpixel: "none".to_string(), ..r.clone() };
    assert_eq!(gray.gnome_antialiasing(), "grayscale");
    assert_eq!(gray.gnome_rgba_order(), "rgb");
    // Tiling sessions: no native store, but GTK gets the GNOME keys.
    let tiling = qs::rendering_plan("hyprland", &r, "", true);
    assert!(tiling.primary.is_empty());
    assert_eq!(tiling.also.len(), 3);
}

/// Store answers are parsed into one setting; the native one beats fonts.conf.
#[test]
fn font_rendering_answers_and_merge() {
    use qs::{RenderDialect as D, RenderKey as K};
    let gnome = qs::parse_render_answers(
        D::Gnome,
        &[(K::Antialias, "'rgba'\n".into()), (K::HintStyle, "'medium'".into()), (K::SubPixel, "'vbgr'".into())],
    );
    assert_eq!(gnome.antialias, Some(true));
    assert_eq!(gnome.subpixel.as_deref(), Some("vbgr"));
    assert_eq!(gnome.hinting.as_deref(), Some("medium"));
    let gray = qs::parse_render_answers(D::Gnome, &[(K::Antialias, "'grayscale'".into())]);
    assert_eq!(gray.subpixel.as_deref(), Some("none"));
    let xfce = qs::parse_render_answers(
        D::Xfce,
        &[
            (K::Antialias, "0".into()),
            (K::Hinting, "0".into()),
            (K::HintStyle, "hintfull".into()),
            (K::SubPixel, "rgb".into()),
        ],
    );
    assert_eq!(xfce.antialias, Some(false));
    assert_eq!(xfce.hinting.as_deref(), Some("none"));
    // "-1" in XFCE means unset.
    assert_eq!(qs::parse_render_answers(D::Xfce, &[(K::Antialias, "-1".into())]).antialias, None);
    let kde = qs::parse_render_answers(D::Kde, &[(K::Antialias, "".into()), (K::HintStyle, "hintslight".into())]);
    assert_eq!(kde.antialias, None);
    assert_eq!(kde.hinting.as_deref(), Some("slight"));

    let fc = qs::RenderingParts {
        antialias: Some(false),
        subpixel: Some("bgr".into()),
        hinting: Some("full".into()),
        exclude: Some((7, 12)),
        dpi: None,
    };
    let merged = qs::merge_rendering(&gnome, &fc);
    assert!(merged.antialias);
    assert_eq!(merged.subpixel, "vbgr");
    assert_eq!(merged.hinting, "medium");
    assert_eq!(merged.exclude, Some((7, 12)), "диапазон — только из fonts.conf");
    assert_eq!(qs::merge_rendering(&Default::default(), &Default::default()), qs::FontRendering::default());
}

const FOREIGN_FONTS_CONF: &str = r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <!-- своё правило человека -->
  <alias>
    <family>monospace</family>
    <prefer><family>Iosevka</family></prefer>
  </alias>
  <match target="font">
    <test name="family"><string>Terminus</string></test>
    <edit name="antialias" mode="assign"><bool>false</bool></edit>
  </match>
</fontconfig>
"#;

/// Our block merges into a foreign fonts.conf without touching foreign content,
/// is replaced on a repeated write and is read back.
#[test]
fn fontconfig_block_merges_and_round_trips() {
    let r = qs::FontRendering {
        antialias: true,
        exclude: Some((8, 15)),
        subpixel: "rgb".to_string(),
        hinting: "slight".to_string(),
    };
    let block = qs::fontconfig_block(&r, None);
    // No file — it is created whole.
    let fresh = qs::fontconfig_with_block(None, &block).unwrap();
    assert!(fresh.starts_with("<?xml"));
    assert!(fresh.contains("<fontconfig>") && fresh.trim_end().ends_with("</fontconfig>"));
    let back = qs::fontconfig_rendering(&fresh);
    assert_eq!(back.antialias, Some(true));
    assert_eq!(back.subpixel.as_deref(), Some("rgb"));
    assert_eq!(back.hinting.as_deref(), Some("slight"));
    assert_eq!(back.exclude, Some((8, 15)));
    // Range in pixels — at 96 DPI 8pt = 11px, 15pt = 20px.
    assert!(fresh.contains("<double>11</double>") && fresh.contains("<double>20</double>"), "{fresh}");

    // A foreign file: everything foreign in place, our block last.
    let merged = qs::fontconfig_with_block(Some(FOREIGN_FONTS_CONF), &block).unwrap();
    for line in FOREIGN_FONTS_CONF.lines().filter(|l| *l != "</fontconfig>") {
        assert!(merged.contains(line), "пропало «{line}»:\n{merged}");
    }
    assert!(merged.find("Terminus").unwrap() < merged.find(">>> Rexon").unwrap());
    // A person's rule with <test name="family"> is not ours, we do not read it.
    assert_eq!(qs::fontconfig_rendering(&merged).antialias, Some(true));

    // A repeated write replaces the block instead of accumulating.
    let r2 = qs::FontRendering { hinting: "full".to_string(), exclude: None, ..r.clone() };
    let again = qs::fontconfig_with_block(Some(&merged), &qs::fontconfig_block(&r2, Some(120))).unwrap();
    assert_eq!(again.matches(">>> Rexon").count(), 1, "{again}");
    assert_eq!(again.matches("<<< Rexon").count(), 1);
    let back = qs::fontconfig_rendering(&again);
    assert_eq!(back.hinting.as_deref(), Some("full"));
    assert_eq!(back.exclude, None);
    assert_eq!(back.dpi, Some(120));
    // And the same thing twice in a row — without text drift.
    let twice = qs::fontconfig_with_block(Some(&again), &qs::fontconfig_block(&r2, Some(120))).unwrap();
    assert_eq!(twice, again);
    // The foreign content did not move meanwhile.
    assert!(again.starts_with(&FOREIGN_FONTS_CONF[..FOREIGN_FONTS_CONF.find("</fontconfig>").unwrap()]));
}

/// A broken file and a cut block mean refusal, not a guess: an error in
/// fonts.conf breaks fonts for all programs.
#[test]
fn fontconfig_block_refuses_to_guess() {
    let block = qs::fontconfig_block(&qs::FontRendering::default(), None);
    for bad in [
        "<fontconfig><match></fontconfig>",
        "<?xml version=\"1.0\"?>\n<fontconfig>\n",
        "<other/>",
        "просто текст",
    ] {
        assert_eq!(
            qs::fontconfig_with_block(Some(bad), &block),
            Err(qs::FontconfigRefusal::Malformed),
            "{bad}"
        );
    }
    let cut = "<fontconfig>\n  <!-- >>> Rexon: font rendering (managed by Rexon, edits are overwritten) -->\n</fontconfig>\n";
    assert_eq!(qs::fontconfig_with_block(Some(cut), &block), Err(qs::FontconfigRefusal::BrokenBlock));
    // An empty `<fontconfig/>` root is expanded.
    let out = qs::fontconfig_with_block(Some("<?xml version=\"1.0\"?>\n<fontconfig/>\n"), &block).unwrap();
    assert!(out.contains("<fontconfig>\n  <!-- >>> Rexon"), "{out}");
    assert_eq!(qs::fontconfig_rendering(&out).antialias, Some(true));
}

/// The file maintained by the KDE «Шрифты» page (its form of the entry) is read
/// the same way as our block: a person who set up fonts in KDE sees their own
/// settings in Rexon, not defaults.
#[test]
fn fontconfig_reads_kde_shaped_file() {
    let kde = r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
 <match target="pattern">
  <edit mode="assign" name="rgba">
   <const>vrgb</const>
  </edit>
 </match>
 <match target="pattern">
  <edit mode="assign" name="hintstyle">
   <const>hintmedium</const>
  </edit>
 </match>
 <match target="pattern">
  <test compare="more_eq" name="size" qual="any">
   <double>6</double>
  </test>
  <test compare="less_eq" name="size" qual="any">
   <double>10</double>
  </test>
  <edit mode="assign" name="antialias">
   <bool>false</bool>
  </edit>
 </match>
 <match target="pattern">
  <edit mode="assign" name="antialias">
   <bool>true</bool>
  </edit>
 </match>
</fontconfig>
"#;
    let parts = qs::fontconfig_rendering(kde);
    assert_eq!(parts.subpixel.as_deref(), Some("vrgb"));
    assert_eq!(parts.hinting.as_deref(), Some("medium"));
    assert_eq!(parts.exclude, Some((6, 10)));
    assert_eq!(parts.antialias, Some(true));
}

/// Antialiasing labels in Russian: the catalog knows them instead of returning English.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn font_rendering_texts_russian() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::text_template("font.aa").as_deref(), Some("Сглаживание"));
    assert_eq!(qs::text_template("font.aa_to").as_deref(), Some("до"));
    assert_eq!(qs::text_template("font.hinting").as_deref(), Some("Хинтинг:"));
    let hint: Vec<String> = qs::hinting_choices().into_iter().map(|(_, l)| l).collect();
    assert_eq!(hint, ["Нет", "Лёгкий", "Средний", "Полный"]);
    let sub: Vec<String> = qs::subpixel_choices().into_iter().map(|(_, l)| l).collect();
    assert_eq!(sub, ["Нет", "RGB", "BGR", "Вертикальная RGB", "Вертикальная BGR"]);
    let refused = qs::rendering_status(&qs::RenderingOutcome::FontconfigRefused("/h/.config/fontconfig/fonts.conf".into()));
    assert!(refused.contains("/h/.config/fontconfig/fonts.conf") && refused.contains("не тронул"), "{refused}");
    assert!(qs::rendering_status(&qs::RenderingOutcome::Saved).starts_with("✅ Сглаживание"));
}

/// Ready-made rendering presets — MATE values; the person's "do not antialias"
/// range and subpixel order are not lost, the preset is recognized back.
#[test]
fn font_render_presets_follow_mate() {
    let mine = qs::FontRendering {
        antialias: true,
        exclude: Some((6, 10)),
        subpixel: "bgr".to_string(),
        hinting: "none".to_string(),
    };
    assert_eq!(qs::render_preset_of(&mine), None, "своя настройка — ни один набор");
    let want = [
        ("monochrome", false, "none", "full"),
        ("best_shapes", true, "none", "medium"),
        ("best_contrast", true, "none", "full"),
        ("subpixel", true, "bgr", "slight"),
    ];
    for (id, aa, sub, hint) in want {
        let p = qs::render_preset(id, &mine).expect(id);
        assert_eq!((p.antialias, p.subpixel.as_str(), p.hinting.as_str()), (aa, sub, hint), "{id}");
        assert_eq!(p.exclude, Some((6, 10)), "{id}: диапазон остаётся");
        assert_eq!(qs::render_preset_of(&p), Some(id), "{id}: узнаётся обратно");
    }
    // Without antialiasing subpixels are "none" — «Субпиксельное» sets RGB.
    let gray = qs::render_preset("best_shapes", &mine).unwrap();
    assert_eq!(qs::render_preset("subpixel", &gray).unwrap().subpixel, "rgb");
    assert!(qs::render_preset("nonsense", &mine).is_none());
    assert_eq!(qs::render_preset_choices().len(), qs::RENDER_PRESETS.len());
}

/// Ready-made preset labels in Russian.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn font_render_presets_russian() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::text_template("font.render_presets").as_deref(), Some("Быстрый выбор:"));
    let labels: Vec<String> = qs::render_preset_choices().into_iter().map(|(_, l, _)| l).collect();
    assert_eq!(labels, ["Монохромный", "Лучшие формы", "Лучший контраст", "Субпиксельное (ЖК-экраны)"]);
    assert!(qs::render_preset_choices().iter().all(|(_, _, tip)| tip.chars().any(|c| ('а'..='я').contains(&c))));
}

// ── «Изменить все шрифты» and forced DPI ─────────────────────────────────

fn now(list: &[(&str, &str, i32)]) -> Vec<qs::FontNow> {
    list.iter()
        .map(|(t, f, s)| qs::FontNow { target: t.to_string(), family: f.to_string(), size: *s })
        .collect()
}

/// "Change all" rules as on the KDE page: small is 2 pt smaller (from 9 pt),
/// monospace does not get the regular family.
#[test]
fn change_all_fonts_rules() {
    let cur = now(&[
        ("system", "Noto Sans", 10),
        ("mono", "Hack", 10),
        ("small", "Noto Sans", 8),
        ("menu", "Noto Sans", 10),
        ("title", "Noto Sans", 10),
    ]);
    let (plan, kept) = qs::change_all_fonts(&cur, "Inter", 12, "both", false);
    let got: Vec<(&str, &str, i32)> = plan.iter().map(|f| (f.target.as_str(), f.family.as_str(), f.size)).collect();
    assert_eq!(
        got,
        [("system", "Inter", 12), ("mono", "Hack", 12), ("small", "Inter", 10), ("menu", "Inter", 12), ("title", "Inter", 12)]
    );
    assert!(kept);
    // A monospace family goes into monospace too.
    let (plan, kept) = qs::change_all_fonts(&cur, "JetBrains Mono", 8, "both", true);
    assert!(!kept);
    assert_eq!(plan[1].family, "JetBrains Mono");
    assert_eq!(plan[2].size, 8, "мельче 9 пт маленький не уменьшается");
    // Size only — the families stay their own.
    let (plan, _) = qs::change_all_fonts(&cur, "Inter", 11, "size", false);
    assert_eq!(plan[1].family, "Hack");
    assert_eq!(plan[0].family, "Noto Sans");
    assert_eq!(plan[2].size, 9);
    // Family only — the sizes stay their own.
    let (plan, _) = qs::change_all_fonts(&cur, "Inter", 20, "family", false);
    assert_eq!(plan.iter().map(|f| f.size).collect::<Vec<_>>(), [10, 10, 8, 10, 10]);
    // No monospace of its own, and a regular one is chosen — monospace is not touched.
    let bare = now(&[("system", "Noto Sans", 10), ("mono", "", 0)]);
    let (plan, _) = qs::change_all_fonts(&bare, "Inter", 11, "both", false);
    assert_eq!(plan.len(), 1);
}

#[test]
fn fc_spacing_decides_monospace() {
    assert_eq!(qs::fc_spacing_argv("DejaVu Sans Mono"), ["fc-list", ":family=DejaVu Sans Mono", "spacing"]);
    assert_eq!(qs::fc_spacing_argv("Foo-Bar:X")[1], ":family=Foo\\-Bar\\:X");
    assert!(qs::parse_fc_spacing(":spacing=100\n:spacing=100\n"));
    assert!(qs::parse_fc_spacing(":spacing=90\n"));
    assert!(!qs::parse_fc_spacing(":\n:spacing=100\n"), "есть пропорциональное начертание");
    assert!(!qs::parse_fc_spacing(""), "семейство не найдено");
}

/// DPI store per environment and writing values. For GNOME-like ones it is the
/// text scaling factor: the GNOME settings daemon computes Xft/DPI from it (96 × factor).
#[test]
fn font_dpi_store_per_desktop() {
    use qs::FontDpiStore as S;
    assert_eq!(qs::font_dpi_store("kde", false), Some(S::Kde));
    assert_eq!(qs::font_dpi_store("kde", true), None, "Plasma Wayland: шрифты следуют масштабу экрана");
    assert_eq!(qs::font_dpi_store("xfce", true), Some(S::Xfce));
    assert_eq!(qs::font_dpi_store("mate", false), Some(S::Mate));
    assert_eq!(qs::font_dpi_store("lxqt", false), Some(S::Fontconfig));
    assert_eq!(
        qs::font_dpi_store("cinnamon-mint", false),
        Some(S::TextScaling { schema: "org.cinnamon.desktop.interface".into() })
    );
    for de in ["gnome", "budgie", "pantheon", "cosmic", "deepin", "hyprland", ""] {
        assert_eq!(
            qs::font_dpi_store(de, true),
            Some(S::TextScaling { schema: "org.gnome.desktop.interface".into() }),
            "{de}"
        );
    }
    let v = |argv: Option<Vec<String>>| argv.unwrap().join(" ");
    assert_eq!(
        v(qs::font_dpi_write_argv(&S::Kde, Some(120), "kwriteconfig6")),
        "kwriteconfig6 --file kcmfonts --group General --key forceFontDPI --notify 120"
    );
    assert_eq!(v(qs::font_dpi_write_argv(&S::Kde, None, "kwriteconfig6")).rsplit(' ').next(), Some("0"));
    assert_eq!(
        v(qs::font_dpi_write_argv(&S::Xfce, None, "")),
        "xfconf-query -c xsettings -p /Xft/DPI -n -t int -s -1"
    );
    assert_eq!(v(qs::font_dpi_write_argv(&S::Mate, Some(110), "")), "gsettings set org.mate.font-rendering dpi 110.0");
    let gnome = S::TextScaling { schema: "org.gnome.desktop.interface".into() };
    assert_eq!(
        v(qs::font_dpi_write_argv(&gnome, Some(120), "")),
        "gsettings set org.gnome.desktop.interface text-scaling-factor 1.25"
    );
    assert_eq!(qs::font_dpi_value(&gnome, Some(1000)), "3.00", "схема не пускает выше 3.0");
    assert_eq!(qs::font_dpi_value(&gnome, None), "1.0");
    assert!(qs::font_dpi_write_argv(&S::Fontconfig, Some(120), "").is_none());
    // Reading back.
    assert_eq!(qs::parse_font_dpi(&gnome, "1.25\n"), Some(120));
    assert_eq!(qs::parse_font_dpi(&gnome, "1.0"), None);
    assert_eq!(qs::parse_font_dpi(&S::Xfce, "-1"), None);
    assert_eq!(qs::parse_font_dpi(&S::Kde, ""), None);
    assert_eq!(qs::parse_font_dpi(&S::Kde, "0"), None);
    assert_eq!(qs::parse_font_dpi(&S::Mate, "96.0"), Some(96));
    // There is an explanation where the action is not instant or not literal.
    assert!(!qs::font_dpi_note(None).is_empty());
    assert!(!qs::font_dpi_note(Some(&gnome)).is_empty());
    assert!(qs::font_dpi_note(Some(&S::Xfce)).is_empty());
}

/// Deepin outside Wayland is an X11 session, which Rexon does not support: there
/// is no store, and the label says «Rexon поддерживает Deepin только на
/// Treeland», not something about a Plasma session.
#[test]
fn deepin_x11_font_dpi_is_refused() {
    // The language is process-wide, while answers of different calls are compared:
    // without pinning, a neighbouring check switches the language between them.
    let _lang = i18n_pin::en();
    assert_eq!(qs::font_dpi_store("deepin", false), None);
    let note = qs::font_dpi_note_for("deepin", None);
    assert_eq!(note, rexon_core::dde::not_treeland_message());
    // The others keep the former label.
    assert_eq!(qs::font_dpi_note_for("kde", None), qs::font_dpi_note(None));
    let gnome = qs::FontDpiStore::TextScaling { schema: "org.gnome.desktop.interface".into() };
    assert_eq!(qs::font_dpi_note_for("deepin", Some(&gnome)), qs::font_dpi_note(Some(&gnome)));
}

/// Old GNOME: rendering lives in the settings daemon schema, with the same
/// enums as the new keys; written and read only if the keys exist.
#[test]
fn legacy_gnome_xsettings_rendering() {
    let r = qs::FontRendering {
        antialias: true,
        exclude: None,
        subpixel: "bgr".to_string(),
        hinting: "full".to_string(),
    };
    let argv: Vec<String> = qs::legacy_rendering_argv(&r).into_iter().map(|a| a.join(" ")).collect();
    assert_eq!(
        argv,
        vec![
            "gsettings set org.gnome.settings-daemon.plugins.xsettings antialiasing rgba",
            "gsettings set org.gnome.settings-daemon.plugins.xsettings hinting full",
            "gsettings set org.gnome.settings-daemon.plugins.xsettings rgba-order bgr",
        ]
    );
    assert_eq!(
        qs::legacy_render_probe_argv().join(" "),
        "gsettings writable org.gnome.settings-daemon.plugins.xsettings antialiasing"
    );
    assert!(qs::legacy_render_available("true\n"));
    assert!(!qs::legacy_render_available(""), "ключа нет — gsettings ничего не напечатал");
    let (dialect, reads) = qs::legacy_rendering_read_argv();
    assert_eq!(reads.len(), 3);
    let parts = qs::parse_render_answers(
        dialect,
        &reads.iter().map(|(k, _)| (*k, match k {
            qs::RenderKey::Antialias => "'grayscale'".to_string(),
            qs::RenderKey::HintStyle => "'medium'".to_string(),
            _ => "'rgb'".to_string(),
        })).collect::<Vec<_>>(),
    );
    assert_eq!(parts.antialias, Some(true));
    assert_eq!(parts.subpixel.as_deref(), Some("none"));
    assert_eq!(parts.hinting.as_deref(), Some("medium"));
    // Old keys are read only when the new ones gave nothing.
    assert!(qs::rendering_needs_legacy(&qs::RenderingParts::default(), true));
    assert!(!qs::rendering_needs_legacy(&qs::RenderingParts::default(), false));
    assert!(!qs::rendering_needs_legacy(&parts, true));
    for de in ["gnome", "budgie", "pantheon"] {
        assert!(qs::legacy_render_is_native(de), "{de}");
    }
    for de in ["kde", "xfce", "cinnamon-mint", "mate", "lxqt", ""] {
        assert!(!qs::legacy_render_is_native(de), "{de}");
    }
}

/// Font DPI in an X11 session without its own settings daemon — `Xft.dpi` in
/// our block of `~/.Xresources`; LXQt on X11 — there as well.
#[test]
fn font_dpi_xresources() {
    use qs::FontDpiStore as S;
    for de in ["", "i3", "openbox", "hyprland"] {
        assert_eq!(qs::font_dpi_store(de, false), Some(S::Xresources), "{de}");
    }
    // Deepin outside Wayland — refused, see `deepin_x11_font_dpi_is_refused`.
    for de in ["gnome", "budgie", "pantheon"] {
        assert_eq!(
            qs::font_dpi_store(de, false),
            Some(S::TextScaling { schema: "org.gnome.desktop.interface".into() }),
            "{de}: своя служба настроек раздаёт Xft/DPI сама"
        );
    }
    assert!(qs::font_dpi_xresources_also("lxqt", false));
    assert!(!qs::font_dpi_xresources_also("lxqt", true));
    assert!(!qs::font_dpi_xresources_also("xfce", false));
    assert_eq!(qs::font_dpi_write_argv(&S::Xresources, Some(120), ""), None, "это файл");

    let user = "! мои ресурсы\nXTerm*faceName: Hack\nXft.dpi: 96\n";
    let text = qs::xresources_with_dpi(Some(user), Some(144)).unwrap();
    assert!(text.starts_with(user.trim_end()), "{text}");
    assert!(text.ends_with("! >>> Rexon: font DPI (managed by Rexon, edits are overwritten)\nXft.dpi: 144\n! <<< Rexon: font DPI\n"), "{text}");
    assert_eq!(qs::xresources_dpi(&text), Some(144), "наш блок последний — он и действует");
    let again = qs::xresources_with_dpi(Some(&text), Some(120)).unwrap();
    assert_eq!(again.matches(">>> Rexon").count(), 1);
    assert_eq!(qs::xresources_dpi(&again), Some(120));
    let off = qs::xresources_with_dpi(Some(&again), None).unwrap();
    assert_eq!(off, user);
    assert_eq!(qs::xresources_dpi(&off), Some(96), "чужое значение остаётся");
    assert_eq!(qs::xresources_with_dpi(None, Some(110)).unwrap().lines().nth(1), Some("Xft.dpi: 110"));
    assert_eq!(qs::xresources_with_dpi(Some("! >>> Rexon: font DPI\nXft.dpi: 1\n"), Some(2)), None, "разорванный блок");
    assert_eq!(qs::xresources_dpi("! Xft.dpi: 200\n"), None, "комментарий");
    assert_eq!(qs::xrdb_merge_argv("/r/x.ad").join(" "), "xrdb -nocpp -merge /r/x.ad");
    let _lang = i18n_pin::en();
    assert!(qs::font_dpi_note(Some(&S::Xresources)).contains("~/.Xresources"));
}

/// «Управление шрифтами»: fc-list parsing, user/system, grouping, search, file
/// check and install location.
#[test]
fn font_manager_list_and_files() {
    let dirs = qs::font_user_dirs("/home/u", "/home/u/.local/share");
    assert_eq!(dirs, vec!["/home/u/.local/share/fonts".to_string(), "/home/u/.fonts".to_string()]);
    assert_eq!(qs::fc_list_fonts_argv().join(" "), "fc-list --format %{family[0]}\t%{style[0]}\t%{file}\t%{foundry}\t%{lang}\n");
    let out = "Noto Sans\tBold\t/usr/share/fonts/noto/NotoSans-Bold.ttf\n\
               Noto Sans\tRegular\t/usr/share/fonts/noto/NotoSans-Regular.ttf\n\
               Noto Sans\tThin Italic\t/usr/share/fonts/noto/NotoSans-ThinItalic.ttf\n\
               Noto Sans\tRegular\t/usr/share/fonts/noto/NotoSans-Regular.ttf\n\
               alpha Mono\t\t/home/u/.local/share/fonts/rexon-x/Alpha.otf\n\
               Old\tRegular\t/home/u/.fonts/old.pcf.gz\n\
               broken line\n\
               Evil\tRegular\t/home/u/.local/share/fonts/../../../../usr/share/fonts/x.ttf\n";
    let fonts = qs::parse_fc_list_fonts(out, &dirs);
    assert_eq!(fonts.len(), 6, "повтор снят, битая строка пропущена");
    assert!(fonts.iter().find(|f| f.family == "alpha Mono").unwrap().user);
    assert_eq!(fonts.iter().find(|f| f.family == "alpha Mono").unwrap().style, "Regular");
    assert!(fonts.iter().find(|f| f.family == "Old").unwrap().user);
    assert!(!fonts.iter().find(|f| f.family == "Evil").unwrap().user, "`..` не выводит из каталога");
    assert!(!fonts.iter().find(|f| f.family == "Noto Sans").unwrap().user);
    assert!(!qs::font_file_is_user("/home/u/.local/share/fonts", &dirs), "сам каталог — не файл");
    assert!(!qs::font_file_is_user("/home/u/.local/share/fontsX/a.ttf", &dirs));

    // A variable font: the whole file (without a name) and its named styles —
    // the italic file gets no extra «Regular».
    let var = "Adwaita Sans\t\t/f/AdwaitaSans-Italic.ttf\n\
               Adwaita Sans\tItalic\t/f/AdwaitaSans-Italic.ttf\n\
               Adwaita Sans\tBold Italic\t/f/AdwaitaSans-Italic.ttf\n\
               Adwaita Sans\t\t/f/AdwaitaSans-Regular.ttf\n\
               Adwaita Sans\tRegular\t/f/AdwaitaSans-Regular.ttf\n";
    let styles: Vec<(String, String)> =
        qs::parse_fc_list_fonts(var, &dirs).into_iter().map(|f| (f.style, f.file)).collect();
    assert_eq!(
        styles,
        vec![
            ("Italic".to_string(), "/f/AdwaitaSans-Italic.ttf".to_string()),
            ("Bold Italic".to_string(), "/f/AdwaitaSans-Italic.ttf".to_string()),
            ("Regular".to_string(), "/f/AdwaitaSans-Regular.ttf".to_string()),
        ]
    );

    let groups = qs::group_font_families(&fonts);
    let names: Vec<&str> = groups.iter().map(|g| g.family.as_str()).collect();
    assert_eq!(names, vec!["alpha Mono", "Evil", "Noto Sans", "Old"], "по алфавиту без учёта регистра");
    let noto = groups.iter().find(|g| g.family == "Noto Sans").unwrap();
    let styles: Vec<&str> = noto.styles.iter().map(|f| f.style.as_str()).collect();
    assert_eq!(styles, vec!["Thin Italic", "Regular", "Bold"], "от тонкого к жирному");
    assert!(noto.has_system() && !noto.has_user());
    assert!(qs::font_family_matches("Noto Sans Mono", "sans mono"));
    assert!(!qs::font_family_matches("Noto Sans", "serif"));

    assert!(qs::font_install_ext_ok("/x/A.TTF") && qs::font_install_ext_ok("a.woff2") && qs::font_install_ext_ok("b.pcf.gz"));
    assert!(!qs::font_install_ext_ok("a.zip"));
    assert!(qs::font_install_filter().contains("*.otf"));
    assert_eq!(qs::fc_query_families_argv("/x/a.ttc").join(" "), "fc-query --format %{family[0]}\n /x/a.ttc");
    assert_eq!(qs::parse_fc_query_families("A\nB\nA\n\n"), vec!["A".to_string(), "B".to_string()]);
    assert!(qs::parse_fc_query_families("").is_empty());
    assert_eq!(
        qs::font_install_dest("/home/u/.local/share", "/tmp/dl/My Font.ttf").as_deref(),
        Some("/home/u/.local/share/fonts/My Font.ttf")
    );
    assert_eq!(qs::font_install_dest("/d", "/"), None);
    assert_eq!(qs::fc_cache_argv("/d/fonts").join(" "), "fc-cache -f /d/fonts");
}

/// «Управление шрифтами» labels: the question before deletion also names the
/// file's neighbours (.ttc); system fonts cannot be deleted — said honestly.
#[test]
fn font_manager_texts_english() {
    let _lang = i18n_pin::en();
    let (title, text) = qs::font_remove_confirm("Inter", 2, &["Inter".to_string(), "Inter Display".to_string()]);
    assert_eq!(title, "Remove Font");
    assert!(text.starts_with("Remove the font Inter (2 files)?"), "{text}");
    assert!(text.contains("Inter Display"), "{text}");
    let (_, one) = qs::font_remove_confirm("Inter", 1, &["Inter".to_string()]);
    assert!(one.starts_with("Remove the font Inter (1 file)?") && !one.contains("also hold"), "{one}");
    assert!(qs::font_remove_status("Inter", 0, 0).contains("together with that package"));
    assert_eq!(qs::font_remove_status("Inter", 2, 0), "✅ Font removed: Inter");
    assert_eq!(qs::font_manager_summary(3, 1), "3 font families, yours: 1");
    assert_eq!(
        qs::font_install_status(&qs::FontInstallOutcome::Installed(vec!["A".into(), "B".into()])),
        "✅ Font installed: A, B"
    );
    assert!(qs::font_install_status(&qs::FontInstallOutcome::NotAFont("x.txt".into())).contains("x.txt"));
    assert_eq!(
        qs::font_install_status(&qs::FontInstallOutcome::InstalledForAll(vec!["A".into()])),
        "✅ Font installed for all users: A"
    );
    assert!(qs::font_remove_shared_note().contains("password"));
    let keys: Vec<&str> = qs::font_manager_texts().iter().map(|(k, _)| *k).collect();
    for k in [
        "open", "close", "search", "install", "remove", "system", "user", "empty", "pick", "sample", "no_fclist",
        "shared", "install_all_tip", "scope_tip", "copy_shared",
    ] {
        assert!(keys.contains(&k), "{k}");
    }
}

/// «Управление шрифтами»: the «Все / Личные / Системные» filter and search by
/// family and style; sample size 12–48.
#[test]
fn font_manager_origin_filter_and_sample() {
    let dirs = qs::font_user_dirs("/home/u", "/home/u/.local/share");
    let out = "Inter\tRegular\t/usr/share/fonts/inter/Inter-Regular.otf\n\
               Inter\tItalic\t/home/u/.local/share/fonts/Inter-Italic.otf\n\
               Inter\tBold\t/usr/share/fonts/inter/Inter-Bold.otf\n\
               Mine\tRegular\t/home/u/.fonts/mine.ttf\n\
               Noto Sans\tRegular\t/usr/share/fonts/noto/NotoSans-Regular.ttf\n";
    let groups = qs::group_font_families(&qs::parse_fc_list_fonts(out, &dirs));
    let names = |v: &[qs::FontFamilyGroup]| v.iter().map(|g| g.family.clone()).collect::<Vec<_>>();
    assert_eq!(names(&qs::font_group_view(&groups, "all", "")), vec!["Inter", "Mine", "Noto Sans"]);
    let user = qs::font_group_view(&groups, "user", "");
    assert_eq!(names(&user), vec!["Inter", "Mine"]);
    assert_eq!(user[0].styles.len(), 1, "у смешанного семейства в «Личных» только свои начертания");
    assert_eq!(user[0].styles[0].style, "Italic");
    let system = qs::font_group_view(&groups, "system", "");
    assert_eq!(names(&system), vec!["Inter", "Noto Sans"]);
    assert_eq!(system[0].styles.len(), 2);
    assert_eq!(names(&qs::font_group_view(&groups, "nonsense", "")).len(), 3, "неизвестный фильтр — все");
    let italic = qs::font_group_view(&groups, "all", "INTER italic");
    assert_eq!(names(&italic), vec!["Inter"]);
    assert_eq!(italic[0].styles.iter().map(|f| f.style.as_str()).collect::<Vec<_>>(), vec!["Italic"]);
    assert!(qs::font_group_view(&groups, "system", "mine").is_empty());
    assert_eq!(names(&qs::font_group_view(&groups, "all", "sans")), vec!["Noto Sans"]);
    // «Для всех пользователей» (/usr/local/share/fonts): neither the user's own
    // nor from a package — deleted with a password; a neighbouring directory is not it.
    let shared = qs::parse_fc_list_fonts(
        "Ad\tRegular\t/usr/local/share/fonts/Ad.ttf\n\
         Ad\tBold\t/usr/local/share/fonts/sub/Ad-Bold.ttf\n\
         Ad\tItalic\t/usr/local/share/fonts2/Ad-Italic.ttf\n\
         Ad\tLight\t/usr/local/share/fonts/../fonts/Ad-Light.ttf\n",
        &dirs,
    );
    assert_eq!(shared.iter().map(|f| f.shared).collect::<Vec<_>>(), vec![true, true, false, false]);
    assert!(shared.iter().all(|f| !f.user));
    let g = &qs::group_font_families(&shared)[0];
    assert!(g.has_shared() && g.has_packaged() && g.has_system() && !g.has_user());
    assert!(groups.iter().all(|g| !g.has_shared()));
    assert_eq!(qs::FONT_ORIGINS, &["all", "user", "system"]);
    let _lang = i18n_pin::en();
    let labels: Vec<String> = qs::font_origin_choices().into_iter().map(|(_, l)| l).collect();
    assert_eq!(labels, vec!["All", "Personal", "System"]);
    assert_eq!(qs::FONT_SAMPLE_SIZE, (12, 48, 16));
    // «Для себя / для всех»: an unknown value means "for myself" (no password).
    assert_eq!(qs::FONT_INSTALL_SCOPES, &["user", "all"]);
    let scopes: Vec<String> = qs::font_install_scope_choices(true).into_iter().map(|(_, l)| l).collect();
    assert_eq!(scopes, vec!["Just for me", "For all users"]);
    // NixOS and SteamOS: "for everyone" is not offered (the helper would refuse there).
    let only_me: Vec<&str> = qs::font_install_scope_choices(false).into_iter().map(|(id, _)| id).collect();
    assert_eq!(only_me, vec!["user"]);
    use rexon_core::system::PkgFamily;
    assert!(qs::font_shared_supported(PkgFamily::Arch, false));
    assert!(!qs::font_shared_supported(PkgFamily::Nix, false));
    assert!(!qs::font_shared_supported(PkgFamily::Arch, true));
    assert!(qs::font_install_scope_is_all("all"));
    assert!(!qs::font_install_scope_is_all("user") && !qs::font_install_scope_is_all("") && !qs::font_install_scope_is_all("ALL"));
    assert_eq!(qs::font_shared_dest("/home/u/Мой шрифт.ttf").as_deref(), Some("/usr/local/share/fonts/Мой шрифт.ttf"));
    assert_eq!(qs::font_shared_dest("/"), None);
    let texts = qs::font_manager_texts();
    let sample = &texts.iter().find(|(k, _)| *k == "sample_size").unwrap().1;
    assert!(sample.contains("%1"), "кегль подставляет страница: {sample}");
}

/// Disabling a font without deleting: our `<rejectfont>` block in fonts.conf
/// (fonts-conf(5)), foreign file content is not touched, an empty block is
/// removed, a file with nothing left is bare (it gets deleted), a broken block
/// and a broken file mean refusal.
#[test]
fn font_manager_disable_rejectfont_block() {
    let files = vec!["/home/u/.fonts/A&B.ttf".to_string(), "/usr/share/fonts/x/X.otf".to_string()];
    let block = qs::fontconfig_reject_block(&files);
    assert_eq!(
        block,
        "  <!-- >>> Rexon: disabled fonts (managed by Rexon, edits are overwritten) -->\n  <selectfont>\n    <rejectfont>\n      <glob>/home/u/.fonts/A&amp;B.ttf</glob>\n      <glob>/usr/share/fonts/x/X.otf</glob>\n    </rejectfont>\n  </selectfont>\n  <!-- <<< Rexon: disabled fonts -->\n"
    );
    assert_eq!(qs::fontconfig_reject_block(&[]), "");
    assert_eq!(qs::fontconfig_reject_block(&["/f/*.ttf".to_string(), "rel.ttf".to_string()]), "", "шаблон и относительный путь не пишутся");
    assert!(qs::font_glob_safe("/usr/share/fonts/a b.ttf"));
    assert!(!qs::font_glob_safe("/f/a?.ttf") && !qs::font_glob_safe("/f/../a.ttf") && !qs::font_glob_safe("a.ttf"));

    // A new file.
    let fresh = qs::fontconfig_with_disabled(None, &files).unwrap();
    assert!(fresh.starts_with("<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig"), "{fresh}");
    assert_eq!(qs::fontconfig_disabled_files(&fresh), files);
    // A foreign file with a rendering block and its own rejectfont: everything foreign in place.
    let user = "<?xml version=\"1.0\"?>\n<fontconfig>\n  <selectfont><rejectfont><glob>/theirs/*.pcf.gz</glob></rejectfont></selectfont>\n  <match target=\"pattern\"><edit name=\"dpi\" mode=\"assign\"><double>96</double></edit></match>\n</fontconfig>\n";
    let with_render = qs::fontconfig_with_block(Some(user), &qs::fontconfig_block(&qs::FontRendering::default(), None)).unwrap();
    let text = qs::fontconfig_with_disabled(Some(&with_render), &files).unwrap();
    assert!(text.contains("<glob>/theirs/*.pcf.gz</glob>") && text.contains(">>> Rexon: font rendering"), "{text}");
    assert_eq!(qs::fontconfig_disabled_files(&text), files, "чужой rejectfont — не наш");
    assert_eq!(qs::fontconfig_rendering(&text), qs::fontconfig_rendering(&with_render), "отрисовка не задета");
    // A repeat gives one block; rewritten rendering does not touch the disabled ones.
    let again = qs::fontconfig_with_disabled(Some(&text), &files[..1]).unwrap();
    assert_eq!(again.matches(">>> Rexon: disabled fonts").count(), 1);
    assert_eq!(qs::fontconfig_disabled_files(&again), files[..1].to_vec());
    let rerender = qs::fontconfig_with_block(Some(&again), &qs::fontconfig_block(&qs::FontRendering::default(), Some(120))).unwrap();
    assert_eq!(qs::fontconfig_disabled_files(&rerender), files[..1].to_vec());
    // A font reset removes only the rendering block.
    let reset = qs::fontconfig_without_block(&rerender).unwrap();
    assert_eq!(qs::fontconfig_disabled_files(&reset), files[..1].to_vec());
    // Everything enabled back: the block is removed, foreign content as it was.
    let back = qs::fontconfig_with_disabled(Some(&text), &[]).unwrap();
    assert_eq!(back, with_render);
    assert!(qs::fontconfig_is_bare(&qs::fontconfig_with_disabled(Some(&fresh), &[]).unwrap()), "наш файл без остатка — удалить");
    assert!(qs::fontconfig_is_bare(&qs::fontconfig_with_disabled(None, &[]).unwrap()));
    // Refusals.
    assert_eq!(qs::fontconfig_with_disabled(Some("<fontconfig><oops></fontconfig>"), &files), Err(qs::FontconfigRefusal::Malformed));
    let broken = "<fontconfig>\n  <!-- >>> Rexon: disabled fonts -->\n</fontconfig>\n";
    assert_eq!(qs::fontconfig_with_disabled(Some(broken), &files), Err(qs::FontconfigRefusal::BrokenBlock));
    assert!(qs::fontconfig_disabled_files(broken).is_empty());

    // Which files are toggled and what comes out.
    let dirs = qs::font_user_dirs("/home/u", "/home/u/.local/share");
    let fonts = qs::parse_fc_list_fonts(
        "Inter\tRegular\t/usr/share/fonts/Inter-Regular.otf\n\
         Inter\tBold\t/usr/share/fonts/Inter-Bold.otf\n\
         Mine\tRegular\t/home/u/.fonts/mine.ttf\n",
        &dirs,
    );
    assert_eq!(qs::font_toggle_files(&fonts, "Inter", ""), vec!["/usr/share/fonts/Inter-Bold.otf", "/usr/share/fonts/Inter-Regular.otf"]);
    assert_eq!(qs::font_toggle_files(&fonts, "Inter", "/usr/share/fonts/Inter-Bold.otf"), vec!["/usr/share/fonts/Inter-Bold.otf"]);
    assert!(qs::font_toggle_files(&fonts, "Nope", "").is_empty());
    let cur = vec!["/a".to_string()];
    assert_eq!(qs::font_disabled_after(&cur, &["/b".to_string(), "/a".to_string()], false), vec!["/a", "/b"]);
    assert_eq!(qs::font_disabled_after(&cur, &["/a".to_string()], true), Vec::<String>::new());
    // Protection: the system font and "all fonts" cannot be disabled; a single style can.
    let all_inter = qs::font_toggle_files(&fonts, "Inter", "");
    assert_eq!(qs::font_disable_refusal(&fonts, &all_inter, &["inter".to_string()]), Some(qs::FontToggleOutcome::InUse("Inter".to_string())));
    assert_eq!(qs::font_disable_refusal(&fonts, &all_inter[..1], &["Inter".to_string()]), None);
    assert_eq!(qs::font_disable_refusal(&fonts, &all_inter, &["Mine".to_string(), String::new()]), None);
    let everything: Vec<String> = fonts.iter().map(|f| f.file.clone()).collect();
    assert_eq!(qs::font_disable_refusal(&fonts, &everything, &[]), Some(qs::FontToggleOutcome::LastFonts));

    let _lang = i18n_pin::en();
    assert!(qs::font_toggle_status(&qs::FontToggleOutcome::Disabled("Inter".into())).starts_with("⏸️ Font disabled: Inter."));
    assert!(qs::font_toggle_status(&qs::FontToggleOutcome::InUse("Inter".into())).contains("system font"));
    assert!(qs::font_toggle_status(&qs::FontToggleOutcome::Refused("/c/fonts.conf".into())).contains("/c/fonts.conf"));
    assert_eq!(qs::font_disable_confirm("Inter", &["Inter".to_string()]), None, "обратимо — без вопроса");
    let (title, text) = qs::font_disable_confirm("Inter", &["Inter".to_string(), "Inter Display".to_string()]).unwrap();
    assert_eq!(title, "Disable Font");
    assert!(text.contains("Inter Display"), "{text}");
}

/// Duplicates: the same family and style in different files; a link to the same
/// file and disabled ones are not copies; bitmap fonts are not asked about.
#[test]
fn font_manager_duplicates() {
    assert_eq!(
        qs::fc_list_scalable_argv().join(" "),
        "fc-list :scalable=true --format %{family[0]}\t%{style[0]}\t%{file}\t%{foundry}\t%{lang}\n"
    );
    let dirs = qs::font_user_dirs("/home/u", "/home/u/.local/share");
    let mut fonts = qs::parse_fc_list_fonts(
        "Goha\tRegular\t/usr/share/fonts/TTF/Goha.ttf\n\
         Goha\tRegular\t/usr/share/fonts/OTF/Goha.otf\n\
         Inter\tBold\t/home/u/.local/share/fonts/Inter-Bold.otf\n\
         Inter\tBold\t/usr/share/fonts/Inter-Bold.otf\n\
         Inter\tRegular\t/usr/share/fonts/Inter-Regular.otf\n\
         Inter\tRegular\t/home/u/.fonts/Inter-Regular.otf\n\
         Link\tRegular\t/home/u/.fonts/link.ttf\n\
         Link\tRegular\t/usr/share/fonts/link.ttf\n\
         Off\tRegular\t/home/u/.fonts/off.ttf\n\
         Off\tRegular\t/usr/share/fonts/off.ttf\n",
        &dirs,
    );
    for f in fonts.iter_mut().filter(|f| f.file == "/home/u/.fonts/off.ttf") {
        f.disabled = true;
    }
    let real = |p: &str| if p == "/home/u/.fonts/link.ttf" { "/usr/share/fonts/link.ttf".to_string() } else { p.to_string() };
    let dups = qs::find_font_duplicates(&fonts, &real);
    let keys: Vec<(String, String)> = dups.iter().map(|d| (d.family.clone(), d.style.clone())).collect();
    assert_eq!(
        keys,
        vec![
            ("Goha".to_string(), "Regular".to_string()),
            ("Inter".to_string(), "Regular".to_string()),
            ("Inter".to_string(), "Bold".to_string()),
        ]
    );
    let inter = &dups[2];
    assert_eq!(inter.copies.iter().map(|c| c.user).collect::<Vec<_>>(), vec![false, true], "системная — первой");
    assert!(dups[0].copies.iter().all(|c| !c.user), "системные дубликаты показываются, удалить нечего");
    let _lang = i18n_pin::en();
    assert_eq!(qs::font_duplicates_summary(3), "3 styles are installed more than once");
    assert!(qs::font_duplicates_summary(0).starts_with("No duplicates"));
}

/// COSMIC: native programs (libcosmic) read only `interface_font` and
/// `monospace_font` from `com.system76.CosmicTk/v1` — RON `FontConfig`
/// (family, weight, stretch, style), without a size. Weight and slant from the
/// chooser dialog are carried there; the entry is read back by the same parser.
#[test]
fn cosmic_font_config_face() {
    use qs::{FontFace, FontFile, FontFileIo, FontStore};
    assert_eq!(qs::parse_font_face("Fira Sans,11,700,1"), FontFace { weight: 700, italic: true });
    assert_eq!(qs::parse_font_face("Fira Sans,11"), FontFace::default(), "старый ответ окна — без начертания");
    assert_eq!(qs::parse_font_spec("Fira Sans,11,700,1"), ("Fira Sans".to_string(), 11), "кегль читается по-прежнему");
    for (w, name) in [(0, "Normal"), (100, "Thin"), (200, "ExtraLight"), (300, "Light"), (400, "Normal"),
                      (500, "Medium"), (600, "Semibold"), (700, "Bold"), (800, "ExtraBold"), (900, "Black"), (650, "Bold")] {
        assert_eq!(qs::cosmic_weight_name(w), name, "{w}");
    }
    let FontStore::File(file) = qs::font_plan("cosmic", "system").primary else { panic!("файл") };
    assert_eq!(file, FontFile::CosmicTk { key: "interface_font".to_string() });
    let text = file.try_new_text_face(None, "Fira Sans", 11, FontFace { weight: 600, italic: true }).unwrap();
    assert_eq!(text, "// Rexon: font (managed by Rexon)\n(\n    family: \"Fira Sans\",\n    weight: Semibold,\n    stretch: Normal,\n    style: Italic,\n)");
    assert_eq!(file.read_spec(&text), "Fira Sans");
    assert_eq!(qs::cosmic_font_face_fields(&text), ("Semibold".to_string(), "Italic".to_string()));
    // Through the write plan — the same text into the same file.
    struct NoFiles;
    impl FontFileIo for NoFiles {
        fn read(&self, _: &str) -> Option<String> { None }
        fn link_target(&self, _: &str) -> Option<String> { None }
    }
    let w = file.file_writes_face("/c", &NoFiles, "Fira Mono", 12, FontFace { weight: 700, italic: false }).unwrap();
    assert_eq!(w[0].rel_path, "cosmic/com.system76.CosmicTk/v1/interface_font");
    assert_eq!(qs::cosmic_font_face_fields(&w[0].text), ("Bold".to_string(), "Normal".to_string()));
    // LXQt keeps the style in QFont fields (weight, slant).
    let FontStore::File(lxqt) = qs::font_plan("lxqt", "system").primary else { panic!("файл") };
    assert_eq!(
        lxqt.try_new_text_face(None, "A", 10, FontFace { weight: 700, italic: true }).as_deref(),
        Some("[Qt]\nfont=\"A,10,-1,5,700,1,0,0,0,0,0,0,0,0,0,1\"\n")
    );
}

/// Font reset plan: exactly ours is removed — foreign content stays, a file
/// with nothing left is deleted, links and blocks broken by hand are not touched.
#[test]
fn font_layers_reset_plan_takes_only_ours() {
    use qs::FontLayerUndo::{Relink, Remove, Write};
    use std::collections::{BTreeMap, BTreeSet};
    struct Mem {
        files: BTreeMap<String, String>,
        links: BTreeSet<String>,
    }
    impl qs::FontLayerFs for Mem {
        fn read(&self, p: &str) -> Option<String> { self.files.get(p).cloned() }
        fn is_link(&self, p: &str) -> bool { self.links.contains(p) }
    }
    let r = qs::FontRendering::default();
    let fc_foreign = "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n<fontconfig>\n  <!-- KDE -->\n  <match target=\"font\">\n    <edit name=\"embeddedbitmap\" mode=\"assign\">\n      <bool>false</bool>\n    </edit>\n  </match>\n</fontconfig>\n";
    let fc_ours = qs::fontconfig_with_block(Some(fc_foreign), &qs::fontconfig_block(&r, Some(110))).unwrap();
    let css_ours = qs::text_with_extra_fonts_block(
        Some("@import url(\"file:///usr/share/themes/A%20B/gtk-3.0/gtk.css\");\n"),
        "/* >>> Rexon: fonts (managed by Rexon, edits are overwritten) */\n/* <<< Rexon: fonts */\n",
    )
    .unwrap();
    let mut files = BTreeMap::new();
    files.insert("/c/fontconfig/fonts.conf".to_string(), fc_ours);
    files.insert("/c/gtk-3.0/gtk.css".to_string(), css_ours);
    files.insert("/usr/share/themes/A B/gtk-3.0/gtk.css".to_string(), "x".to_string());
    // A block broken by hand — do not touch.
    files.insert("/c/gtk-4.0/gtk.css".to_string(), "/* >>> Rexon: fonts */\nmenu {}\n".to_string());
    files.insert("/h/.Xresources".to_string(), qs::xresources_with_dpi(None, Some(120)).unwrap());
    // A link is not our file, even with a marker inside.
    files.insert("/c/qt6ct/qss/rexon-fonts.qss".to_string(), "/* >>> Rexon: fonts */".to_string());
    let links: BTreeSet<String> = ["/c/qt6ct/qss/rexon-fonts.qss".to_string()].into();
    // COSMIC without a marker — rewritten by «Параметры», not ours.
    files.insert("/c/cosmic/com.system76.CosmicTk/v1/monospace_font".to_string(), "(family: \"X\")".to_string());
    files.insert(
        "/c/cosmic/com.system76.CosmicTk/v1/interface_font".to_string(),
        qs::cosmic_font_ron("Fira Sans"),
    );
    let plan = qs::font_layers_reset_plan("/c", "/h", &Mem { files, links });
    assert_eq!(
        plan,
        vec![
            Relink { path: "/c/gtk-3.0/gtk.css".into(), target: "/usr/share/themes/A B/gtk-3.0/gtk.css".into() },
            Write { path: "/c/fontconfig/fonts.conf".into(), text: fc_foreign.into() },
            Remove { path: "/h/.Xresources".into() },
            Remove { path: "/c/cosmic/com.system76.CosmicTk/v1/interface_font".into() },
        ]
    );
    // An empty fonts.conf — only our skeleton.
    let bare = qs::fontconfig_with_block(None, &qs::fontconfig_block(&r, None)).unwrap();
    assert!(qs::fontconfig_is_bare(&qs::fontconfig_without_block(&bare).unwrap()));
    assert!(!qs::fontconfig_is_bare(fc_foreign));
    assert!(!qs::fontconfig_is_bare("<fontconfig>\n  <!-- моё -->\n</fontconfig>\n"), "комментарий человека — не пусто");
    // A foreign gtk.css line before and after our block stays.
    let mid = qs::text_without_extra_fonts_block(
        "a {}\n/* >>> Rexon: fonts (x) */\nmenu {}\n/* <<< Rexon: fonts */\nb {}\n",
    )
    .unwrap();
    assert_eq!(mid, "a {}\n\nb {}\n");
    assert_eq!(qs::text_without_extra_fonts_block("a {}\n"), None);
    assert_eq!(qs::gtk_css_import_only("@import url(\"colors.css\");"), None, "относительный адрес — не наша ссылка");
}

// ── Title font in LXQt and labwc ─────────────────────────────────────────

/// Editing Openbox and labwc `rc.xml` against `golden/rc_xml_title_font.json`:
/// only the name and size of the active and inactive window change, the rest
/// of the file stays as it was; what is written is read back.
#[test]
fn rc_xml_title_font_golden() {
    let golden: Value = serde_json::from_str(include_str!("golden/rc_xml_title_font.json")).unwrap();
    for case in golden["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let wm = match case["wm"].as_str().unwrap() {
            "openbox" => qs::TitleWm::Openbox,
            _ => qs::TitleWm::Labwc,
        };
        let input = case["input"].as_str();
        let family = case["family"].as_str().unwrap();
        let size = case["size"].as_i64().unwrap() as i32;
        if let Some(before) = case["read_before"].as_array() {
            let want = (before[0].as_str().unwrap().to_string(), before[1].as_i64().unwrap() as i32);
            assert_eq!(qs::rc_xml_title_font(input.unwrap()), Some(want), "{name}");
        }
        let got = qs::rc_xml_with_title_font(input, wm, family, size);
        assert_eq!(got.as_deref(), case["expected"].as_str(), "{name}");
        if let Some(text) = got {
            assert_eq!(qs::rc_xml_title_font(&text), Some((family.to_string(), size)), "{name}: читается обратно");
            assert_eq!(qs::rc_xml_with_title_font(Some(&text), wm, family, size).as_deref(), Some(text.as_str()), "{name}: повтор ничего не меняет");
            let file = qs::FontFile::RcXml { wm };
            // The read string also carries the style from the file's `<weight>`/`<slant>`,
            // which a write without a style does not touch.
            let (f, s, face) = qs::parse_font_spec_face(&file.read_spec(&text));
            assert_eq!((f.as_str(), s), (family, size), "{name}");
            assert_eq!(Some(face), qs::rc_xml_title_font_face(&text).map(|t| t.2), "{name}");
        }
    }
}

/// Who draws the titles: LXQt — by `session.conf` (the user's before the
/// system one, X11 and Wayland are different keys), bare labwc — labwc itself.
#[test]
fn title_wm_from_lxqt_session_conf() {
    use qs::TitleWm as W;
    let user = "[General]\nwindow_manager=openbox\ncompositor=labwc\n";
    assert_eq!(qs::title_wm_for("lxqt", &[user], false), Some(W::Openbox));
    assert_eq!(qs::title_wm_for("lxqt", &[user], true), Some(W::Labwc));
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\nwindow_manager=/usr/bin/xfwm4 --replace\n"], false), Some(W::Xfwm4));
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\nwindow_manager=kwin_x11\n"], false), Some(W::KWin));
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\ncompositor=kwin_wayland\n"], true), Some(W::KWin));
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\nwindow_manager=marco\n"], false), Some(W::Marco));
    // The user's file without the key — the system one applies.
    let system = "[General]\nwindow_manager=openbox\n";
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\nleave_confirmation=true\n", system], false), Some(W::Openbox));
    // Not chosen, unknown or foreign to the session — no title font is offered.
    assert_eq!(qs::title_wm_for("lxqt", &[], false), None);
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\nwindow_manager=icewm\n"], false), None);
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\ncompositor=Hyprland\n"], true), None);
    assert_eq!(qs::title_wm_for("lxqt", &[user.replace("openbox", "labwc").as_str()], false), None, "labwc на X11 не бывает");
    assert_eq!(qs::title_wm_for("labwc", &[], true), Some(W::Labwc));
    assert_eq!(qs::title_wm_for("sway", &[user], true), Some(W::Sway));
}

/// Title font plan by window manager: Openbox/labwc — `rc.xml` and
/// `--reconfigure` after writing; xfwm4/Marco/KWin — their native keys.
#[test]
fn title_font_plan_follows_session_wm() {
    use qs::{FontFile, FontStore, TitleWm as W};
    for (wm, prog, path) in [(W::Openbox, "openbox", "openbox/rc.xml"), (W::Labwc, "labwc", "labwc/rc.xml")] {
        let plan = qs::font_plan_wm("lxqt", "title", Some(wm));
        assert_eq!(plan.primary, FontStore::File(FontFile::RcXml { wm }));
        assert!(plan.also.is_empty() && plan.extra.is_empty() && !plan.gtk_ini);
        assert_eq!(plan.notify, vec![vec![prog.to_string(), "--reconfigure".to_string()]]);
        assert_eq!(FontFile::RcXml { wm }.rel_path(), path);
        assert!(qs::font_target_available_wm("lxqt", "title", Some(wm)));
    }
    assert!(FontFile::RcXml { wm: W::Labwc }.may_create());
    assert!(!FontFile::RcXml { wm: W::Openbox }.may_create(), "свой rc.xml Openbox заменяет системный целиком");
    assert_eq!(qs::font_plan_wm("lxqt", "title", Some(W::Xfwm4)), qs::font_plan("xfce", "title"));
    assert_eq!(qs::font_plan_wm("lxqt", "title", Some(W::Marco)), qs::font_plan("mate", "title"));
    assert_eq!(qs::font_plan_wm("lxqt", "title", Some(W::KWin)), qs::font_plan("kde", "title"));
    assert_eq!(qs::font_plan_wm("labwc", "title", Some(W::Labwc)).primary, FontStore::File(FontFile::RcXml { wm: W::Labwc }));
    // Other targets in LXQt do not depend on the window manager.
    assert_eq!(qs::font_plan_wm("lxqt", "system", Some(W::Openbox)), qs::font_plan("lxqt", "system"));
    // Other environments are not overridden by the session's window manager.
    assert_eq!(qs::font_plan_wm("mate", "title", Some(W::Openbox)), qs::font_plan("mate", "title"));
    assert!(qs::font_targets_shown_wm("lxqt", Some(W::Openbox)).contains(&("title", true)));
    assert!(qs::font_plan("kde", "title").notify.is_empty());
}

/// sway: the title font is a `font pango:…` line in our file, which is
/// included into the sway config by a managed block (the accent mechanism).
#[test]
fn sway_title_font_plan_and_pango() {
    use qs::{FontFace, FontFile, FontStore, TitleWm as W};
    assert!(qs::font_target_available_wm("sway", "title", Some(W::Sway)));
    assert!(qs::font_targets_shown_wm("sway", Some(W::Sway)).contains(&("title", true)));
    let plan = qs::font_plan_wm("sway", "title", Some(W::Sway));
    assert_eq!(plan.primary, FontStore::File(FontFile::SwayConf));
    assert!(plan.also.is_empty() && plan.extra.is_empty() && plan.notify.is_empty() && !plan.gtk_ini);
    assert_eq!(qs::font_plan_wm("sway", "system", Some(W::Sway)), qs::font_plan("sway", "system"));
    assert_eq!(FontFile::SwayConf.rel_path(), "sway/rexon-title-font.conf");
    // Not written as a single file: a block in the config with a sway check is needed.
    struct NoFiles;
    impl qs::FontFileIo for NoFiles {
        fn read(&self, _: &str) -> Option<String> {
            None
        }
        fn link_target(&self, _: &str) -> Option<String> {
            None
        }
    }
    assert!(FontFile::SwayConf.file_writes("/h/.config", &NoFiles, "Noto Sans", 11).is_none());

    // Pango description: weight and slant as words, regular without a word.
    assert_eq!(qs::pango_font_desc("Noto Sans", 11, FontFace::default()), "Noto Sans 11");
    assert_eq!(qs::pango_font_desc("Noto Sans", 11, FontFace { weight: 700, italic: true }), "Noto Sans Bold Italic 11");
    assert_eq!(qs::pango_font_desc("Inter", 9, FontFace { weight: 600, italic: false }), "Inter Semi-Bold 9");
    assert_eq!(qs::pango_font_desc("Inter", 9, FontFace { weight: 400, italic: false }), "Inter 9");
    for (desc, family, size, weight, italic) in [
        ("pango:Noto Sans Bold Italic 11", "Noto Sans", 11, 700, true),
        ("monospace 10", "monospace", 10, 0, false),
        ("\"pango:DejaVu Sans Mono 9.5\"", "DejaVu Sans Mono", 10, 0, false),
        ("pango:Inter Semi-Bold 9", "Inter", 9, 600, false),
        ("Cantarell, Noto Sans 12", "Cantarell", 12, 0, false),
        ("pango:Bold 10", "Bold", 10, 0, false),
        ("Noto Sans", "Noto Sans", 10, 0, false),
    ] {
        let (f, s, face) = qs::parse_pango_font_desc(desc);
        assert_eq!((f.as_str(), s, face.weight, face.italic), (family, size, weight, italic), "{desc}");
    }
    let content = qs::sway_title_font_content("Noto Sans", 11, FontFace { weight: 700, italic: false });
    assert!(content.lines().any(|l| l == "font pango:Noto Sans Bold 11"), "{content}");
    assert_eq!(FontFile::SwayConf.read_spec(&content), "Noto Sans Bold 11");

    // The last top-level `font`; `bar { font … }` is the bar font.
    let config = "font pango:Old 8\n# font pango:Commented 20\nbar {\n    font pango:Bar 30\n    colors {\n    }\n}\nfont pango:New Bold 12\nmode \"resize\" {\n    font x 1\n}\n";
    assert_eq!(qs::sway_font_value(config).as_deref(), Some("pango:New Bold 12"));
    assert_eq!(qs::sway_font_value("bar {\n font pango:Bar 30\n}\n"), None);
}

/// Writing and reading the sway title font on a fake: our file, an `include`
/// block in the config, `swaymsg font …`; reading — from the block, otherwise
/// from the config, otherwise the sway default font.
#[test]
fn sway_title_font_apply_writes_block_and_reads_back() {
    use rexon_core::animations::{LookCtx, LookIo};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    struct Fake {
        files: RefCell<BTreeMap<String, String>>,
        ran: RefCell<Vec<Vec<String>>>,
    }
    impl LookIo for Fake {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(argv.to_vec());
            (true, String::new())
        }
        fn have(&self, tool: &str) -> bool {
            tool == "swaymsg"
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            self.files.borrow_mut().insert(path.to_string(), text.to_string());
            true
        }
        fn list_dir(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }
    let ctx = LookCtx::new("sway", "/h", None);
    let config = "/h/.config/sway/config";
    let own = "/h/.config/sway/rexon-title-font.conf";
    let io = Fake { files: RefCell::new(BTreeMap::new()), ran: RefCell::new(Vec::new()) };
    // No config anywhere — the sway default font.
    assert_eq!(qs::sway_title_font_current(&io, &ctx), qs::SWAY_DEFAULT_FONT);
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "monospace 10");
    io.write(config, "font pango:Old Sans 8\nbar {\n    font pango:Bar 30\n}\n");
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "Old Sans 8");

    let face = qs::FontFace { weight: 700, italic: false };
    qs::sway_title_font_apply(&io, &ctx, "Noto Sans", 11, face).unwrap();
    let text = io.read(config).unwrap();
    assert!(text.starts_with("font pango:Old Sans 8\n"), "чужое не тронуто: {text}");
    assert!(text.contains(">>> Rexon: title bar font"), "{text}");
    assert!(text.lines().any(|l| l == format!("include {own}")), "{text}");
    assert_eq!(io.read(own).as_deref().and_then(qs::sway_font_value).as_deref(), Some("pango:Noto Sans Bold 11"));
    assert!(io.ran.borrow().iter().any(|a| a == &["swaymsg".to_string(), "font pango:Noto Sans Bold 11".to_string()]));
    assert!(!io.ran.borrow().iter().any(|a| a.iter().any(|s| s == "reload")), "без reload: он перезапускает swaybg");
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "Noto Sans Bold 11");

    // A repeat rewrites the same block instead of adding a second one.
    qs::sway_title_font_apply(&io, &ctx, "Inter", 9, qs::FontFace::default()).unwrap();
    let text = io.read(config).unwrap();
    assert_eq!(text.matches(">>> Rexon: title bar font").count(), 1, "{text}");
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "Inter 9");

    // The block was removed by hand — our file no longer applies, the config is read.
    io.write(config, "font pango:Old Sans 8\n");
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "Old Sans 8");

    // No personal config — the system one is seeded, and the block goes into it.
    let io = Fake { files: RefCell::new(BTreeMap::new()), ran: RefCell::new(Vec::new()) };
    io.write("/etc/sway/config", "font pango:System 10\n");
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "System 10");
    qs::sway_title_font_apply(&io, &ctx, "Noto Sans", 11, qs::FontFace::default()).unwrap();
    let text = io.read(config).unwrap();
    assert!(text.starts_with("font pango:System 10\n") && text.contains(&format!("include {own}")), "{text}");
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "Noto Sans 11");
}

/// Where there is no title font button — an honest reason; where there is one — silence.
#[test]
fn title_font_note_is_honest() {
    // The language is process-wide, and English words of the explanation are
    // checked: without pinning, a neighbouring check switches the language to Russian.
    let _lang = i18n_pin::en();
    assert_eq!(qs::title_font_note("kde", None), None);
    assert_eq!(qs::title_font_note("lxqt", Some(qs::TitleWm::Openbox)), None);
    assert!(qs::title_font_note("lxqt", None).unwrap().contains("Openbox"));
    assert!(qs::title_font_note("cosmic", None).unwrap().contains("interface font"));
    assert_eq!(qs::title_font_note("sway", Some(qs::TitleWm::Sway)), None);
    for de in ["hyprland", "niri"] {
        assert!(qs::title_font_note(de, None).unwrap().contains("no title bars"), "{de}");
    }
}

/// A font with a style in both notations: Pango ("Sans Bold 10" — gsettings,
/// xfconf) and QFont (kdeglobals, Qt 6 and old Qt 5). Weight and slant are kept
/// separately from the family and carried between notations without loss.
#[test]
fn font_face_between_pango_and_qfont() {
    use qs::FontFace;
    let face = |weight, italic| FontFace { weight, italic };
    for (raw, family, size, f) in [
        ("'Sans Bold 10'", "Sans", 10, face(700, false)),
        ("Sans Italic 10", "Sans", 10, face(0, true)),
        ("Noto Sans Semi-Bold Italic 11", "Noto Sans", 11, face(600, true)),
        ("Ubuntu 11", "Ubuntu", 11, face(0, false)),
        ("Cantarell", "Cantarell", 0, face(0, false)),
        ("Noto Sans,11", "Noto Sans", 11, face(0, false)),
        ("Sans,10,-1,5,700,0,0,0,0,0,0,0,0,0,0,1", "Sans", 10, face(700, false)),
        ("Sans,10,-1,5,400,1,0,0,0,0,0,0,0,0,0,1", "Sans", 10, face(400, true)),
        ("Inter,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,SemiBold", "Inter", 9, face(600, false)),
        // Qt 5: weight on the old scale (75 bold, 63 semi-bold).
        ("Sans,10,-1,5,75,0,0,0,0,0", "Sans", 10, face(700, false)),
        ("Sans,10,-1,5,63,1,0,0,0,0,Semibold Italic", "Sans", 10, face(600, true)),
        // An old Rexon entry with a word in the weight field.
        ("Noto Sans,11,-1,5,Regular,0,0,0,0,0", "Noto Sans", 11, face(400, false)),
    ] {
        assert_eq!(qs::parse_font_spec_face(raw), (family.to_string(), size, f), "{raw}");
    }
    assert_eq!(qs::kde_font_spec_face("Sans", 10, face(700, false)), "Sans,10,-1,5,700,0,0,0,0,0,0,0,0,0,0,1");
    assert_eq!(qs::kde_font_spec_face("Sans", 10, face(0, true)), "Sans,10,-1,5,400,1,0,0,0,0,0,0,0,0,0,1");
    assert_eq!(qs::kde_font_spec_face("Sans", 10, FontFace::default()), qs::kde_font_spec("Sans", 10));
    // Round trip Pango → QFont → Pango.
    for desc in ["Sans Bold 10", "Sans Italic 10", "Noto Sans Semi-Bold Italic 11", "Ubuntu 11"] {
        let (family, size, f) = qs::parse_font_spec_face(desc);
        let (family, size, f) = qs::parse_font_spec_face(&qs::kde_font_spec_face(&family, size, f));
        assert_eq!(qs::pango_font_desc(&family, size, f), desc);
    }
    // Writing per store: KDE — QFont fields, gsettings/xfconf — Pango words.
    let bold = face(700, true);
    assert_eq!(
        qs::font_write_argv_face("mate", "title", "Sans", 10, bold, "").unwrap().last().unwrap(),
        "Sans Bold Italic 10"
    );
    assert_eq!(
        qs::font_write_argv_face("xfce", "system", "Sans", 10, bold, "").unwrap().last().unwrap(),
        "Sans Bold Italic 10"
    );
    assert_eq!(
        qs::font_write_argv_face("kde", "title", "Sans", 10, bold, "kwriteconfig6").unwrap().last().unwrap(),
        "Sans,10,-1,5,700,1,0,0,0,0,0,0,0,0,0,1"
    );
    assert_eq!(
        qs::font_write_argv_face("mate", "system", "Sans", 10, FontFace::default(), ""),
        qs::font_write_argv("mate", "system", "Sans", 10, "")
    );
}

/// Font style through the "write → read" round trip in every store the
/// «Оформление» tab writes it to: MATE gsettings, xfconf, kdeglobals, Openbox
/// and labwc `rc.xml`, the sway `font` line. Bold, italic and semi-bold come
/// back the same; regular after bold comes back regular.
#[test]
fn font_face_round_trip_every_store() {
    use qs::{FontFace, FontFile, TitleWm};
    let faces = [
        ("Bold", FontFace { weight: 700, italic: false }),
        ("Italic", FontFace { weight: 400, italic: true }),
        ("Semi-Bold", FontFace { weight: 600, italic: false }),
        ("Bold Italic", FontFace { weight: 700, italic: true }),
        ("Regular", FontFace { weight: 400, italic: false }),
    ];
    // Regular weight reads sometimes as 0 (Pango, rc.xml), sometimes as 400 (QFont).
    let norm = |(f, s, face): (String, i32, FontFace)| (f, s, if face.weight == 0 { 400 } else { face.weight }, face.italic);

    // Commands: the value is the last argument; `gsettings get` (quoted),
    // `xfconf-query` and `kreadconfig6` return it the same way.
    for (de, target, quoted) in [
        ("mate", "title", true),
        ("mate", "system", true),
        ("mate", "mono", true),
        ("xfce", "title", false),
        ("xfce", "system", false),
        ("kde", "title", false),
        ("kde", "system", false),
    ] {
        for (name, face) in faces {
            let argv = qs::font_write_argv_face(de, target, "Noto Sans", 11, face, "kwriteconfig6").unwrap();
            let value = argv.last().unwrap();
            let raw = if quoted { format!("'{value}'\n") } else { format!("{value}\n") };
            assert_eq!(
                norm(qs::parse_font_spec_face(&raw)),
                ("Noto Sans".to_string(), 11, face.weight, face.italic),
                "{de}/{target} {name}: {raw}"
            );
        }
    }
    // Pango words and QFont fields — the ones the desktops write themselves.
    let mate = qs::font_write_argv_face("mate", "title", "Sans", 10, faces[2].1, "").unwrap();
    assert_eq!(mate.last().map(String::as_str), Some("Sans Semi-Bold 10"));
    let kde = qs::font_write_argv_face("kde", "title", "Sans", 10, faces[0].1, "kwriteconfig6").unwrap();
    assert_eq!(kde.last().map(String::as_str), Some("Sans,10,-1,5,700,0,0,0,0,0,0,0,0,0,0,1"));

    // rc.xml: Openbox has only `Bold`/`Normal` weight — semi-bold becomes bold;
    // labwc keeps the Pango steps.
    let openbox = "<?xml version=\"1.0\"?>\n<openbox_config>\n  <theme>\n    <name>Clearlooks</name>\n    <font place=\"ActiveWindow\">\n      <name>sans</name>\n      <size>8</size>\n      <weight>Bold</weight>\n      <slant>Normal</slant>\n    </font>\n  </theme>\n  <keyboard/>\n</openbox_config>\n";
    for (wm, start) in [(TitleWm::Openbox, Some(openbox)), (TitleWm::Labwc, None)] {
        let file = FontFile::RcXml { wm };
        let mut text = start.map(str::to_string);
        for (name, face) in faces {
            let written = file.try_new_text_face(text.as_deref(), "Noto Sans", 11, face).unwrap();
            let want = if wm == TitleWm::Openbox && face.weight == 600 { 700 } else { face.weight };
            assert_eq!(
                norm(qs::parse_font_spec_face(&file.read_spec(&written))),
                ("Noto Sans".to_string(), 11, want, face.italic),
                "{wm:?} {name}: {written}"
            );
            // The inactive window gets the same style, no second element is created.
            assert_eq!(written.matches("place=\"InactiveWindow\"").count(), 1, "{written}");
            assert_eq!(written.matches("<weight>").count(), 2, "{written}");
            text = Some(written);
        }
        // No style chosen — `<weight>`/`<slant>` stay as they were.
        let bold = file.try_new_text_face(text.as_deref(), "Noto Sans", 11, faces[0].1).unwrap();
        let kept = file.try_new_text_face(Some(&bold), "Inter", 9, FontFace::default()).unwrap();
        assert_eq!(norm(qs::parse_font_spec_face(&file.read_spec(&kept))), ("Inter".to_string(), 9, 700, false));
    }
    let labwc = FontFile::RcXml { wm: TitleWm::Labwc }.try_new_text_face(None, "Inter", 9, faces[2].1).unwrap();
    assert!(labwc.contains("<weight>semibold</weight>") && labwc.contains("<slant>normal</slant>"), "{labwc}");
    let ob = FontFile::RcXml { wm: TitleWm::Openbox }.try_new_text_face(Some(openbox), "Inter", 9, faces[1].1).unwrap();
    assert!(ob.contains("<weight>Normal</weight>") && ob.contains("<slant>Italic</slant>"), "{ob}");
    assert!(ob.contains("<name>Clearlooks</name>") && ob.contains("<keyboard/>"), "чужое не тронуто: {ob}");

    // sway: our file with a `font pango:…` line.
    for (name, face) in faces {
        let content = qs::sway_title_font_content("Noto Sans", 11, face);
        assert_eq!(
            norm(qs::parse_font_spec_face(&FontFile::SwayConf.read_spec(&content))),
            ("Noto Sans".to_string(), 11, face.weight, face.italic),
            "sway {name}: {content}"
        );
    }

    // The tab label shows the style from a QFont entry too.
    assert!(qs::font_value_text("Sans,10,-1,5,700,1,0,0,0,0,0,0,0,0,0,1").contains("Sans Bold Italic 10"));
    assert!(qs::font_value_text("'Sans Semi-Bold 10'").contains("Sans Semi-Bold 10"));
}

/// Document font: MATE's native key (`org.mate.interface`) with the GNOME key
/// alongside; in all other environments the GNOME key
/// (`org.gnome.desktop.interface document-font-name`), including KDE, XFCE,
/// LXQt and COSMIC, which have no key of their own. Without qt6ct and
/// settings.ini: neither Qt nor GTK itself reads this font.
#[test]
fn document_font_goes_to_gsettings_everywhere() {
    use qs::{FontStore, SoundBackend};
    let gs = |schema: &str, key: &str| FontStore::Cmd(SoundBackend::GSettings { schema: schema.into(), key: key.into() });
    let gnome = gs("org.gnome.desktop.interface", "document-font-name");
    let plan = qs::font_plan("mate", "document");
    assert_eq!(plan.primary, gs("org.mate.interface", "document-font-name"));
    assert_eq!(plan.also, vec![gnome.clone()]);
    assert!(plan.extra.is_empty() && plan.notify.is_empty() && !plan.gtk_ini);
    for de in ["gnome", "cinnamon", "cinnamon-mint", "budgie", "pantheon", "kde", "xfce", "lxqt", "cosmic", "sway", "hyprland", ""] {
        assert!(qs::font_target_available(de, "document"), "{de}");
        let plan = qs::font_plan(de, "document");
        assert_eq!(plan.primary, gnome, "{de}");
        assert!(plan.also.is_empty() && plan.extra.is_empty() && !plan.gtk_ini, "{de}");
    }
    assert_eq!(
        qs::font_write_argv_face("kde", "document", "Noto Serif", 12, qs::FontFace { weight: 700, italic: false }, "kwriteconfig6")
            .unwrap(),
        ["gsettings", "set", "org.gnome.desktop.interface", "document-font-name", "Noto Serif Bold 12"]
    );
    // The explanation says who does not read it.
    let _lang = i18n_pin::en();
    assert!(qs::document_font_note().contains("Qt"));
    let (label, _tip, title) = qs::font_target_texts("document");
    assert!(label.contains("Documents") && title == "Document Font", "{label} / {title}");
    // A KDE font reset removes it too.
    assert_eq!(
        rexon_core::app::document_font_reset_argv("/usr/bin/gsettings"),
        ["/usr/bin/gsettings", "reset", "org.gnome.desktop.interface", "document-font-name"]
    );
}

/// Desktop icon labels: Cinnamon — Nemo, MATE — Caja (a Pango description),
/// XFCE — only the xfdesktop size with its custom-size switch; in other
/// environments the item is disabled and honestly labelled.
#[test]
fn desktop_icon_font_per_desktop() {
    use qs::{FontStore, SoundBackend};
    let gs = |schema: &str, key: &str| FontStore::Cmd(SoundBackend::GSettings { schema: schema.into(), key: key.into() });
    for de in ["cinnamon", "cinnamon-mint"] {
        let plan = qs::font_plan(de, "desktop");
        assert_eq!(plan.primary, gs("org.nemo.desktop", "font"), "{de}");
        assert!(plan.also.is_empty() && plan.extra.is_empty() && !plan.gtk_ini, "{de}");
    }
    let plan = qs::font_plan("mate", "desktop");
    assert_eq!(plan.primary, gs("org.mate.caja.desktop", "font"));
    assert!(plan.also.is_empty() && plan.extra.is_empty() && !plan.gtk_ini);
    assert_eq!(
        qs::font_write_argv_face("mate", "desktop", "Ubuntu", 11, qs::FontFace { weight: 0, italic: true }, "").unwrap(),
        ["gsettings", "set", "org.mate.caja.desktop", "font", "Ubuntu Italic 11"]
    );

    // XFCE: the size as a number in /desktop-icons/font-size (double), the switch
    // before writing the size.
    assert!(qs::font_target_size_only("xfce", "desktop"));
    assert!(!qs::font_target_size_only("mate", "desktop") && !qs::font_target_size_only("xfce", "system"));
    let plan = qs::font_plan("xfce", "desktop");
    let store = FontStore::Cmd(SoundBackend::Xfconf { channel: "xfce4-desktop".into(), prop: "/desktop-icons/font-size".into() });
    assert_eq!(plan.primary, store);
    assert!(plan.also.is_empty() && !plan.gtk_ini);
    assert_eq!(
        plan.extra,
        vec![vec![
            "xfconf-query", "-c", "xfce4-desktop", "-p", "/desktop-icons/use-custom-font-size", "-n", "-t", "bool", "-s",
            "true",
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()]
    );
    assert_eq!(
        qs::font_store_write_argv_face(&store, "Ignored Family", 9, qs::FontFace::default(), "").unwrap(),
        ["xfconf-query", "-c", "xfce4-desktop", "-p", "/desktop-icons/font-size", "-n", "-t", "double", "-s", "9"]
    );
    // Reading: custom size enabled — the interface font with that size; disabled
    // or unset — the interface font as a whole.
    assert_eq!(qs::xfdesktop_font_spec("true", "12.000000", "Sans Bold 10"), "Sans Bold 12");
    assert_eq!(qs::xfdesktop_font_spec("false", "12.000000", "Sans 10"), "Sans 10");
    assert_eq!(qs::xfdesktop_font_spec("", "", "'Sans 10'"), "Sans 10");
    assert_eq!(qs::xfdesktop_font_spec("true", "oops", "Sans 10"), "Sans 10");
    assert_eq!(
        qs::xfdesktop_use_size_read_argv(),
        ["xfconf-query", "-c", "xfce4-desktop", "-p", "/desktop-icons/use-custom-font-size"]
    );

    // Where there is no key — disabled, with an honest reason.
    for de in ["kde", "gnome", "budgie", "pantheon", "cosmic", "sway", "hyprland", "deepin", ""] {
        assert!(!qs::font_target_available(de, "desktop"), "{de}");
        assert!(qs::desktop_font_note(de).is_some(), "{de}");
    }
    for de in ["cinnamon", "cinnamon-mint", "mate", "lxqt"] {
        assert!(qs::font_target_available(de, "desktop"), "{de}");
        assert_eq!(qs::desktop_font_note(de), None, "{de}");
    }
    let _lang = i18n_pin::en();
    assert!(qs::desktop_font_note("xfce").unwrap().contains("only the size"));
    assert!(qs::desktop_font_note("kde").unwrap().contains("interface font"));
    let (label, _tip, title) = qs::font_target_texts("desktop");
    assert!(label.contains("Desktop") && title == "Desktop Font", "{label} / {title}");
}

// ── LXQt desktop icon labels (pcmanfm-qt) ───────────────────────────────

/// A fake of a live desktop: whether it runs, whether it quits on request, how
/// many times it was closed and started. "Quit" saves the file, as pcmanfm-qt
/// itself does (`Settings::save` in `aboutToQuit`).
struct FakeDesktop {
    running: std::cell::Cell<bool>,
    obeys_quit: bool,
    on_quit: Box<dyn Fn()>,
    quits: std::cell::Cell<usize>,
    starts: std::cell::Cell<usize>,
}

impl FakeDesktop {
    fn new(running: bool) -> Self {
        FakeDesktop {
            running: running.into(),
            obeys_quit: true,
            on_quit: Box::new(|| {}),
            quits: 0.into(),
            starts: 0.into(),
        }
    }
}

impl qs::PcmanfmBus for FakeDesktop {
    fn running(&self) -> bool {
        self.running.get()
    }
    fn quit(&self) -> bool {
        self.quits.set(self.quits.get() + 1);
        if self.obeys_quit {
            (self.on_quit)();
            self.running.set(false);
        }
        true
    }
    fn start(&self) -> bool {
        self.starts.set(self.starts.get() + 1);
        self.running.set(true);
        true
    }
    fn pause(&self) {}
}

/// Temporary directories: our own `$XDG_CONFIG_HOME`, a system directory
/// instead of `$XDG_CONFIG_DIRS` (the real `/etc/xdg` and `/usr/share` are not
/// looked at) and the backup directory.
struct LxqtStand {
    root: std::path::PathBuf,
    paths: qs::PcmanfmPaths,
}

impl LxqtStand {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("rexon-lxqt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("cfg")).unwrap();
        let s = |p: &str| root.join(p).to_string_lossy().into_owned();
        let paths = qs::PcmanfmPaths {
            config_home: s("cfg"),
            config_dirs: vec![s("empty-dir"), s("sys")],
            backup_dir: qs::pcmanfm_backup_dir(&s("data")),
        };
        LxqtStand { root, paths }
    }
    fn put_system(&self, text: &str) {
        let dir = self.root.join("sys/pcmanfm-qt/lxqt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.conf"), text).unwrap();
    }
    fn put_own(&self, text: &str) {
        std::fs::create_dir_all(self.paths.own_dir()).unwrap();
        std::fs::write(self.paths.own_file(), text).unwrap();
    }
    fn own(&self) -> Option<String> {
        std::fs::read_to_string(self.paths.own_file()).ok()
    }
}

impl Drop for LxqtStand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The system file as in the pcmanfm-qt package (abridged): wallpaper, shortcuts, colours.
const PCMANFM_SYSTEM: &str = "[System]\nIconThemeName=elementary\nTerminal=qterminal\n\n[Desktop]\n\
DesktopShortcuts=Home, Trash, Computer, Network\nWallpaperMode=stretch\n\
Wallpaper=/usr/share/lxqt/wallpapers/waves-logo.png\nBgColor=#000000\n\
Font=\"Sans Serif,12,-1,5,50,0,0,0,0,0\"\n\n[Volume]\nMountOnStartup=true\n";

/// All lines except `Font` are the same, in the same order.
fn same_except_font(a: &str, b: &str) {
    let keep =
        |t: &str| t.split_inclusive('\n').filter(|l| !l.starts_with("Font=")).map(str::to_string).collect::<Vec<_>>();
    assert_eq!(keep(a), keep(b));
}

/// Editing the single `Font` line leaves the other bytes as they were: `\r\n`,
/// a missing final `\n`, the order; no key — it goes after `[Desktop]`; no
/// section — it is appended. The value is a quoted QFont.
#[test]
fn pcmanfm_font_line_edit_keeps_other_bytes() {
    let v = qs::pcmanfm_font_value("Noto Sans", 9, qs::FontFace { weight: 700, italic: true });
    assert_eq!(v, "\"Noto Sans,9,-1,5,700,1,0,0,0,0,0,0,0,0,0,1\"");
    let out = qs::pcmanfm_with_desktop_font(Some(PCMANFM_SYSTEM), &v);
    assert_eq!(out, PCMANFM_SYSTEM.replace("Font=\"Sans Serif,12,-1,5,50,0,0,0,0,0\"", &format!("Font={v}")));
    assert_eq!(qs::pcmanfm_desktop_font_raw(&out).as_deref(), Some(v.as_str()));

    let crlf = "[Desktop]\r\nWallpaper=/a.png\r\nFont=\"A,1\"\r\n[X]\r\nFont=keep";
    assert_eq!(
        qs::pcmanfm_with_desktop_font(Some(crlf), "\"B,2\""),
        "[Desktop]\r\nWallpaper=/a.png\r\nFont=\"B,2\"\r\n[X]\r\nFont=keep"
    );
    // `Font` of a foreign section is neither touched nor read.
    assert_eq!(qs::pcmanfm_desktop_font_raw("[X]\nFont=a\n"), None);
    assert_eq!(qs::pcmanfm_with_desktop_font(Some("[X]\nFont=a"), "b"), "[X]\nFont=a\n\n[Desktop]\nFont=b\n");
    assert_eq!(
        qs::pcmanfm_with_desktop_font(Some("[Desktop]\nWallpaper=/a.png\n"), "b"),
        "[Desktop]\nFont=b\nWallpaper=/a.png\n"
    );
    assert_eq!(qs::pcmanfm_with_desktop_font(None, "b"), "[Desktop]\nFont=b\n");
    assert_eq!(
        qs::pcmanfm_without_desktop_font(&out),
        PCMANFM_SYSTEM.replace("Font=\"Sans Serif,12,-1,5,50,0,0,0,0,0\"\n", "")
    );

    // Reading gives a QFont entry without quotes, understood by the common parser.
    let file = qs::FontFile::PcmanfmDesktop;
    assert_eq!(file.rel_path(), "pcmanfm-qt/lxqt/settings.conf");
    assert_eq!(file.read_spec(PCMANFM_SYSTEM), "Sans Serif,12,-1,5,50,0,0,0,0,0");
    assert!(!file.may_create());
    let (family, size, _) = qs::parse_font_spec_face(&file.read_spec(PCMANFM_SYSTEM));
    assert_eq!((family.as_str(), size), ("Sans Serif", 12));
    let plan = qs::font_plan("lxqt", "desktop");
    assert_eq!(plan.primary, qs::FontStore::File(file));
    assert!(plan.also.is_empty() && plan.extra.is_empty() && plan.notify.is_empty() && !plan.gtk_ini);
}

/// Lookup order as in `Settings::profileDir`: the user's DIRECTORY shadows the
/// system one, even when empty; the system one is the first existing.
#[test]
fn pcmanfm_seed_follows_profile_dir() {
    let st = LxqtStand::new("seed");
    assert_eq!(qs::pcmanfm_seed(&st.paths).source, qs::PcmanfmSource::Builtin);
    st.put_system(PCMANFM_SYSTEM);
    let seed = qs::pcmanfm_seed(&st.paths);
    assert!(matches!(seed.source, qs::PcmanfmSource::System(ref p) if p.ends_with("sys/pcmanfm-qt/lxqt/settings.conf")));
    assert_eq!(seed.text.as_deref(), Some(PCMANFM_SYSTEM));
    std::fs::create_dir_all(st.paths.own_dir()).unwrap();
    assert_eq!(qs::pcmanfm_seed(&st.paths), qs::PcmanfmSeed { source: qs::PcmanfmSource::Own, text: None });
    assert_eq!(qs::pcmanfm_config_dirs("/a:/b/:/a"), vec!["/a", "/b", "/etc/xdg", "/usr/share"]);
    assert_eq!(qs::pcmanfm_config_dirs(""), vec!["/etc/xdg", "/usr/share"]);
}

/// No user file: it is created as a COPY of the system one with one line changed
/// (wallpaper and the rest in place), before the edit an "as before" record is
/// made; restore removes the created file together with the directories — the
/// state as before Rexon.
#[test]
fn pcmanfm_no_own_file_seeds_from_system_and_restores() {
    let st = LxqtStand::new("seedcopy");
    st.put_system(PCMANFM_SYSTEM);
    let bus = FakeDesktop::new(false);
    let applied =
        qs::pcmanfm_desktop_font_apply(&st.paths, &bus, "DejaVu Serif", 15, qs::FontFace::default()).unwrap();
    assert_eq!(applied, qs::PcmanfmApplied { restarted: false });
    assert_eq!((bus.quits.get(), bus.starts.get()), (0, 0), "стол не работал — не трогаем");
    let own = st.own().expect("свой файл заведён");
    same_except_font(&own, PCMANFM_SYSTEM);
    assert!(own.contains("Wallpaper=/usr/share/lxqt/wallpapers/waves-logo.png\n"));
    assert_eq!(qs::pcmanfm_desktop_font_spec(&st.paths), "DejaVu Serif,15,-1,5,400,0,0,0,0,0,0,0,0,0,0,1");
    let record = qs::pcmanfm_font_record(&st.paths).expect("запись «как было»");
    assert!(!record.had_file);
    assert_eq!(record.font.as_deref(), Some("\"Sans Serif,12,-1,5,50,0,0,0,0,0\""));
    assert_eq!(record.created_dirs.len(), 2);
    // The system file is untouched.
    assert_eq!(std::fs::read_to_string(st.root.join("sys/pcmanfm-qt/lxqt/settings.conf")).unwrap(), PCMANFM_SYSTEM);

    // Second edit: the "as before" record is the former one.
    qs::pcmanfm_desktop_font_apply(&st.paths, &bus, "Noto Sans", 10, qs::FontFace::default()).unwrap();
    assert_eq!(qs::pcmanfm_font_record(&st.paths).unwrap().font, record.font);

    assert_eq!(qs::pcmanfm_desktop_font_restore(&st.paths, &bus), Ok(true));
    assert_eq!(st.own(), None);
    assert!(!std::path::Path::new(&format!("{}/pcmanfm-qt", st.paths.config_home)).exists(), "каталоги сняты");
    assert!(qs::pcmanfm_font_record(&st.paths).is_none());
    assert!(matches!(qs::pcmanfm_seed(&st.paths).source, qs::PcmanfmSource::System(_)));
    assert_eq!(qs::pcmanfm_desktop_font_restore(&st.paths, &bus), Ok(false), "второй раз — нечего");
}

/// A user file exists: one line changes, a copy of the file goes to the backup;
/// restore puts back the former line, and what the person changed after us
/// (wallpaper) stays.
#[test]
fn pcmanfm_own_file_one_line_and_restore_keeps_later_changes() {
    let st = LxqtStand::new("own");
    st.put_system("[Desktop]\nWallpaper=/system.png\n");
    let mine = PCMANFM_SYSTEM.replace("waves-logo.png", "my-cat.jpg");
    st.put_own(&mine);
    let bus = FakeDesktop::new(false);
    qs::pcmanfm_desktop_font_apply(&st.paths, &bus, "Ubuntu", 11, qs::FontFace::default()).unwrap();
    let own = st.own().unwrap();
    same_except_font(&own, &mine);
    assert_ne!(own, mine);
    assert_eq!(std::fs::read_to_string(format!("{}/settings.conf", st.paths.backup_dir)).unwrap(), mine, "копия");
    assert!(qs::pcmanfm_font_record(&st.paths).unwrap().had_file);

    // The person changed the wallpaper after us.
    std::fs::write(st.paths.own_file(), own.replace("my-cat.jpg", "my-dog.jpg")).unwrap();
    assert_eq!(qs::pcmanfm_desktop_font_restore(&st.paths, &bus), Ok(true));
    assert_eq!(st.own().unwrap(), mine.replace("my-cat.jpg", "my-dog.jpg"));

    // There was no `Font` of their own — on restore the line is removed.
    let st = LxqtStand::new("own-nofont");
    st.put_own("[Desktop]\nWallpaper=/w.png\n");
    qs::pcmanfm_desktop_font_apply(&st.paths, &bus, "Ubuntu", 11, qs::FontFace::default()).unwrap();
    assert_eq!(st.own().unwrap(), "[Desktop]\nFont=\"Ubuntu,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\"\nWallpaper=/w.png\n");
    qs::pcmanfm_desktop_font_restore(&st.paths, &bus).unwrap();
    assert_eq!(st.own().unwrap(), "[Desktop]\nWallpaper=/w.png\n");

    // A user directory without a file: pcmanfm-qt reads built-in defaults —
    // a file with one line is equivalent to them, the system one is not mixed in.
    let st = LxqtStand::new("own-emptydir");
    st.put_system(PCMANFM_SYSTEM);
    std::fs::create_dir_all(st.paths.own_dir()).unwrap();
    qs::pcmanfm_desktop_font_apply(&st.paths, &bus, "Ubuntu", 11, qs::FontFace::default()).unwrap();
    assert_eq!(st.own().unwrap(), "[Desktop]\nFont=\"Ubuntu,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\"\n");
    assert!(qs::pcmanfm_font_record(&st.paths).unwrap().created_dirs.is_empty());
    qs::pcmanfm_desktop_font_restore(&st.paths, &bus).unwrap();
    assert_eq!(st.own(), None);
    assert!(std::path::Path::new(&st.paths.own_dir()).is_dir(), "чужой каталог остаётся");
}

/// Neither user nor system settings: touch nothing and say why.
#[test]
fn pcmanfm_without_any_settings_refuses() {
    let _lang = i18n_pin::en();
    let st = LxqtStand::new("none");
    let bus = FakeDesktop::new(false);
    let why = qs::pcmanfm_refusal(&st.paths, &bus).expect("отказ");
    assert!(why.contains("settings.conf"), "{why}");
    let err = qs::pcmanfm_desktop_font_apply(&st.paths, &bus, "Ubuntu", 11, qs::FontFace::default()).unwrap_err();
    assert_eq!(err, why);
    assert_eq!(st.own(), None);
    assert!(!std::path::Path::new(&format!("{}/pcmanfm-qt", st.paths.config_home)).exists());
    assert!(!std::path::Path::new(&st.paths.backup_dir).exists());
    st.put_system(PCMANFM_SYSTEM);
    assert_eq!(qs::pcmanfm_refusal(&st.paths, &bus), None);
}

/// A live desktop: first it is closed (and saves its whole settings itself —
/// the copy is made from THIS file), then the edit, then the start. If it did
/// not close, the file is not touched (on quit it would overwrite it).
#[test]
fn pcmanfm_live_desktop_closed_first_and_restarted() {
    let _lang = i18n_pin::en();
    let st = LxqtStand::new("live");
    st.put_system(PCMANFM_SYSTEM);
    let own_file = st.paths.own_file();
    let own_dir = st.paths.own_dir();
    let saved = PCMANFM_SYSTEM.replace("waves-logo.png", "live.png");
    let saved_on_quit = saved.clone();
    let mut bus = FakeDesktop::new(true);
    bus.on_quit = Box::new(move || {
        std::fs::create_dir_all(&own_dir).unwrap();
        std::fs::write(&own_file, &saved_on_quit).unwrap();
    });
    let applied = qs::pcmanfm_desktop_font_apply(&st.paths, &bus, "Ubuntu", 11, qs::FontFace::default()).unwrap();
    assert_eq!(applied, qs::PcmanfmApplied { restarted: true });
    assert_eq!((bus.quits.get(), bus.starts.get()), (1, 1));
    same_except_font(&st.own().unwrap(), &saved);
    assert_eq!(std::fs::read_to_string(format!("{}/settings.conf", st.paths.backup_dir)).unwrap(), saved);
    assert!(qs::pcmanfm_applied_note(applied).contains("restarted"));
    assert!(qs::pcmanfm_applied_note(qs::PcmanfmApplied { restarted: false }).contains("next time"));

    // Did not close: no copy, no edit, and no second one is started.
    let st = LxqtStand::new("stuck");
    st.put_own(PCMANFM_SYSTEM);
    let mut bus = FakeDesktop::new(true);
    bus.obeys_quit = false;
    let err = qs::pcmanfm_desktop_font_apply(&st.paths, &bus, "Ubuntu", 11, qs::FontFace::default()).unwrap_err();
    assert!(err.contains("didn't close"), "{err}");
    assert_eq!(st.own().unwrap(), PCMANFM_SYSTEM);
    assert!(qs::pcmanfm_font_record(&st.paths).is_none());
    assert_eq!(bus.starts.get(), 0);
}

/// Live desktop commands: quitting pcmanfm-qt and starting the `lxqt-session`
/// module (without waiting for a reply — `startModule` does not send one).
#[test]
fn pcmanfm_bus_commands() {
    assert_eq!(
        qs::pcmanfm_quit_argv(),
        [
            "dbus-send",
            "--session",
            "--print-reply",
            "--reply-timeout=5000",
            "--dest=org.pcmanfm.PCManFM",
            "/Application",
            "org.pcmanfm.Application.quit"
        ]
    );
    let start = qs::lxqt_start_desktop_argv();
    assert!(!start.iter().any(|a| a == "--print-reply"));
    assert_eq!(start.last().map(String::as_str), Some("string:lxqt-desktop.desktop"));
    assert!(start.contains(&"org.lxqt.session.startModule".to_string()));
    assert_eq!(qs::pcmanfm_desktop_spawn_argv(), ["setsid", "-f", "pcmanfm-qt", "--desktop", "--profile=lxqt"]);
    assert_eq!(qs::dbus_name_has_owner_argv("x").last().map(String::as_str), Some("string:x"));
    let _lang = i18n_pin::en();
    assert!(qs::pcmanfm_restore_texts().1.contains("wallpaper"));
    assert!(qs::pcmanfm_restore_status(&Ok(false)).contains("nothing"));
}

/// The pcmanfm-qt directory we created is already occupied by its own file
/// (`recent-files.conf`): it cannot be removed, and if empty it would shadow
/// the system one and leave the desktop without wallpaper — so it keeps the
/// text pcmanfm-qt read before us.
#[test]
fn pcmanfm_restore_keeps_wallpaper_when_dir_stays() {
    let st = LxqtStand::new("dirstays");
    st.put_system(PCMANFM_SYSTEM);
    let bus = FakeDesktop::new(false);
    qs::pcmanfm_desktop_font_apply(&st.paths, &bus, "Ubuntu", 11, qs::FontFace::default()).unwrap();
    std::fs::write(format!("{}/recent-files.conf", st.paths.own_dir()), "[Recent]\n").unwrap();
    assert_eq!(qs::pcmanfm_desktop_font_restore(&st.paths, &bus), Ok(true));
    assert_eq!(st.own().as_deref(), Some(PCMANFM_SYSTEM));
}
