//! Сверка порта `rexon/qt_appearance/gtk_sanitizer.py` с живым Python.
//!
//! Эталон `golden/gtk_sanitizer_cases.json` снят с ЭТОЙ машины эталонными
//! `_py_*`-ветками модуля, ТОЛЬКО ЧТЕНИЕМ:
//! * `live` — настоящие `~/.config/gtk-{3,4}.0/settings.ini`, `kdeglobals` и
//!   `kcminputrc` вместе со снимком настоящих каталогов тем
//!   (`/usr/share/icons`, `/usr/share/themes`, `/usr/share/sounds`,
//!   `~/.local/share/*`). Ни один файл пользователя не менялся — эталон
//!   содержит лишь то, что записал бы санитайзер.
//! * `broken` — те же конфиги, но с нарочно битыми ссылками, в отдельном
//!   временном `HOME`; `XDG_DATA_HOME` НАРОЧНО вынесен за пределы
//!   `$HOME/.local/share`, поэтому случай ловит починку XDG-путей (старый код
//!   искал темы от `Path.home()` и живой темы из `XDG_DATA_HOME` не видел).
//! * `empty` — конфигов нет вовсе.
//!
//! Машинно-зависимые пути токенизированы: `{HOME}` — домашний каталог,
//! `{DATA}` — корень пользовательских данных. Ядро получает ровно эти токены
//! как `home`/`data_home`, поэтому сверка литеральная.

use std::collections::HashMap;

mod i18n_pin;

use rexon_core::gtk_sanitizer as g;
use rexon_core::installed_scan::FsSnapshot;
use serde_json::Value;

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/gtk_sanitizer_cases.json")).unwrap()
}

fn snapshot(case: &Value) -> FsSnapshot {
    FsSnapshot::from_json(&case["snapshot"])
}

/// `None` для JSON `null` — так эталон различает «файла нет» и «пустой файл».
fn opt_str(v: &Value) -> Option<&str> {
    if v.is_null() {
        None
    } else {
        Some(v.as_str().unwrap())
    }
}

fn map_of_str(v: &Value) -> HashMap<String, String> {
    v.as_object()
        .unwrap()
        .iter()
        .map(|(k, val)| (k.clone(), val.as_str().unwrap().to_string()))
        .collect()
}

fn kde_ini(v: &Value) -> HashMap<String, HashMap<String, String>> {
    v.as_object()
        .unwrap()
        .iter()
        .map(|(k, val)| (k.clone(), map_of_str(val)))
        .collect()
}

const HOME: &str = "{HOME}";
const DATA: &str = "{DATA}";

fn scenarios() -> Vec<&'static str> {
    vec!["live", "broken", "empty"]
}

// ── таблицы и каталоги ───────────────────────────────────────────────────

#[test]
fn таблицы_дефолтов_совпадают_с_python() {
    let g_ = golden();
    let c = &g_["const"];
    for (t, want) in c["SAFE_DEFAULTS"].as_object().unwrap() {
        assert_eq!(g::safe_default(t), want.as_str().unwrap(), "SAFE_DEFAULTS[{t}]");
    }
    for (t, want) in c["DEFAULT_FALLBACKS"].as_object().unwrap() {
        let want: Vec<&str> = want
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert_eq!(g::default_fallbacks(t), want.as_slice(), "DEFAULT_FALLBACKS[{t}]");
    }
    let mut unsafe_themes = g::UNSAFE_CURSOR_THEMES.to_vec();
    unsafe_themes.sort_unstable();
    let want: Vec<&str> = c["UNSAFE_CURSOR_THEMES"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert_eq!(unsafe_themes, want);
    let versions: Vec<&str> = c["GTK_VERSIONS"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert_eq!(g::GTK_VERSIONS, versions.as_slice());
    assert_eq!(g::KWRITECONFIG_TIMEOUT, c["KWRITECONFIG_TIMEOUT"].as_u64().unwrap());
    assert_eq!(g::COLORSCHEME_DEFAULT, c["COLORSCHEME_DEFAULT"].as_str().unwrap());
    let keys: Vec<(String, String)> = c["INI_KEYS"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let a = p.as_array().unwrap();
            (
                a[0].as_str().unwrap().to_string(),
                a[1].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let mine: Vec<(String, String)> = g::INI_KEYS
        .iter()
        .map(|(k, t)| (k.to_string(), t.to_string()))
        .collect();
    assert_eq!(mine, keys, "порядок проверки ключей значим");
}

#[test]
fn каталоги_поиска_совпадают_с_python() {
    for (t, want) in golden()["theme_dirs"].as_object().unwrap() {
        let want: Vec<String> = want
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect();
        assert_eq!(g::theme_dirs(t, HOME, DATA), want, "theme_dirs({t})");
    }
}

#[test]
fn путь_settings_ini_как_у_python() {
    for case in golden()["live"]["gtk"].as_array().unwrap() {
        let ver = case["version"].as_str().unwrap();
        // Эталон записал абсолютный путь с токеном {HOME}; корень конфигов
        // на этой машине — {HOME}/.config (XDG_CONFIG_HOME не задан).
        assert_eq!(
            g::gtk_ini_path(&format!("{HOME}/.config"), ver),
            case["ini_path"].as_str().unwrap()
        );
    }
}

// ── разбор конфигов ──────────────────────────────────────────────────────

#[test]
fn разбор_settings_ini_как_у_python() {
    for c in golden()["read_ini"].as_array().unwrap() {
        let text = opt_str(&c["text"]);
        assert_eq!(
            g::read_ini_settings(text),
            map_of_str(&c["result"]),
            "read_ini_settings({text:?})"
        );
    }
}

#[test]
fn разбор_kde_конфига_как_у_python() {
    for c in golden()["read_kde"].as_array().unwrap() {
        let text = opt_str(&c["text"]);
        assert_eq!(
            g::read_kde_ini(text),
            kde_ini(&c["result"]),
            "read_kde_ini(...)"
        );
    }
}

#[test]
fn чтение_курсора_kcminputrc_как_у_python() {
    for c in golden()["read_kcminput"].as_array().unwrap() {
        let text = opt_str(&c["text"]);
        assert_eq!(
            g::read_kcminput_cursor(text),
            c["result"].as_str().unwrap(),
            "read_kcminput_cursor({text:?})"
        );
    }
}

// ── формирование нового текста settings.ini ──────────────────────────────

#[test]
fn новый_текст_settings_ini_байт_в_байт() {
    for c in golden()["write_ini"].as_array().unwrap() {
        let existing = opt_str(&c["existing"]);
        let key = c["key"].as_str().unwrap();
        let value = c["value"].as_str().unwrap();
        assert_eq!(
            g::write_ini_setting(existing, key, value),
            c["result"].as_str().unwrap(),
            "write_ini_setting({existing:?}, {key}, {value})"
        );
    }
}

#[test]
fn метка_ini_в_журнале_как_у_python() {
    for c in golden()["ini_label"].as_array().unwrap() {
        assert_eq!(
            g::ini_label(c["path"].as_str().unwrap()),
            c["result"].as_str().unwrap()
        );
    }
}

#[test]
fn отчёт_verbose_байт_в_байт() {
    // Эталон снят с оригинала, а он печатал по-русски — сверяем русский
    // из `po/ru.po`: для русского интерфейса ничего не поменялось.
    let _lang = i18n_pin::ru();
    for c in golden()["verbose"].as_array().unwrap() {
        let changes: Vec<String> = c["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect();
        assert_eq!(g::verbose_report(&changes), c["result"].as_str().unwrap());
    }
}

#[test]
fn english_verbose_report_has_no_russian() {
    let _lang = i18n_pin::en();
    assert_eq!(g::verbose_report(&[]), "✅ All GTK/KDE configs are OK, nothing was fixed\n");
    assert_eq!(
        g::verbose_report(&["gtk-3.0: icon theme".to_string()]),
        "✅ Fixed broken links:\n  • gtk-3.0: icon theme\n"
    );
}

// ── решения на РЕАЛЬНЫХ данных машины ────────────────────────────────────

#[test]
fn план_правок_settings_ini_как_у_python() {
    let gold = golden();
    for name in scenarios() {
        let case = &gold[name];
        for ini in case["gtk"].as_array().unwrap() {
            let mut fs = snapshot(case);
            let path = ini["ini_path"].as_str().unwrap();
            let settings = map_of_str(&ini["settings"]);
            let edits = g::plan_gtk_ini_edits(&mut fs, path, &settings, HOME, DATA);
            let want = ini["plan"].as_array().unwrap();
            assert_eq!(edits.len(), want.len(), "{name}/{path}: число правок");
            for (got, exp) in edits.iter().zip(want) {
                assert_eq!(got.key, exp["key"].as_str().unwrap(), "{name}/{path}");
                assert_eq!(got.value, exp["value"].as_str().unwrap(), "{name}/{path}");
                assert_eq!(got.change, exp["change"].as_str().unwrap(), "{name}/{path}");
            }
        }
    }
}

/// Последовательное применение плана — как это делает исполнитель (файл
/// перечитывается на каждый ключ) — даёт тот же файл, что и Python.
#[test]
fn результат_применения_плана_совпадает_с_python() {
    let gold = golden();
    for name in scenarios() {
        let case = &gold[name];
        for ini in case["gtk"].as_array().unwrap() {
            let mut fs = snapshot(case);
            let path = ini["ini_path"].as_str().unwrap();
            let settings = map_of_str(&ini["settings"]);
            let edits = g::plan_gtk_ini_edits(&mut fs, path, &settings, HOME, DATA);
            let mut cur = opt_str(&ini["text"]).map(String::from);
            for e in &edits {
                cur = Some(g::write_ini_setting(cur.as_deref(), &e.key, &e.value));
            }
            let want = opt_str(&ini["result_text"]).map(String::from);
            assert_eq!(cur, want, "{name}/{path}: итоговый settings.ini");
        }
    }
}

#[test]
fn план_правок_kde_как_у_python() {
    let gold = golden();
    for name in scenarios() {
        let case = &gold[name];
        let mut fs = snapshot(case);
        let kde = kde_ini(&case["kdeglobals"]);
        let cursor = case["kcminput_cursor"].as_str().unwrap();
        let edits = g::plan_kde_edits(&mut fs, &kde, cursor, HOME, DATA);
        let want = case["kde_plan"].as_array().unwrap();
        assert_eq!(edits.len(), want.len(), "{name}: число KDE-правок");
        for (got, exp) in edits.iter().zip(want) {
            assert_eq!(got.file, exp["file"].as_str().unwrap(), "{name}");
            assert_eq!(got.group, exp["group"].as_str().unwrap(), "{name}");
            assert_eq!(got.key, exp["key"].as_str().unwrap(), "{name}");
            assert_eq!(got.value, exp["value"].as_str().unwrap(), "{name}");
            assert_eq!(got.change, exp["change"].as_str().unwrap(), "{name}");
            let argv: Vec<String> = exp["argv"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap().to_string())
                .collect();
            assert_eq!(g::kwriteconfig_argv("kwriteconfig6", got), argv, "{name}");
        }
    }
}

/// На живой машине санитайзеру НЕЧЕГО чинить — эталон это фиксирует, и это
/// же гарантия, что тест ничего не «исправляет» в конфигах пользователя.
#[test]
fn на_живой_машине_правок_нет() {
    let gold = golden();
    let live = &gold["live"];
    for ini in live["gtk"].as_array().unwrap() {
        assert!(
            ini["plan"].as_array().unwrap().is_empty(),
            "живой {}: правок быть не должно",
            ini["ini_path"]
        );
    }
    assert!(live["kde_plan"].as_array().unwrap().is_empty());
}

// ── согласие с pre-flight курсоров в themes.rs ───────────────────────────

/// Правило «безопасна ли Inherits-цепочка» портировано дважды: здесь и как
/// [`rexon_core::themes::is_cursor_theme_chain_safe`] (pre-flight применения
/// курсорной темы, поверх `FsSnapshot`). Разъехаться они не должны — гоняем
/// обе реализации по всем курсорным темам из снимка.
#[test]
fn цепочка_курсоров_согласна_с_themes() {
    let gold = golden();
    for name in scenarios() {
        let case = &gold[name];
        let dirs: Vec<String> = case["snapshot"]["dirs"]
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| k.ends_with("/cursors"))
            .map(|k| k.trim_end_matches("/cursors").to_string())
            .collect();
        assert!(!dirs.is_empty(), "{name}: в снимке нет ни одной курсорной темы");
        for dir in dirs {
            let snap = snapshot(case);
            let parent_has_cursors = |parent: &str| {
                g::cursor_parent_dirs(DATA)
                    .iter()
                    .any(|b| snap.is_dir(&format!("{b}/{parent}/cursors")))
            };
            let via_themes = rexon_core::themes::is_cursor_theme_chain_safe(
                &snap,
                &dir,
                parent_has_cursors,
            );
            let mut fs = snapshot(case);
            let mine = g::is_cursor_theme_chain_safe(&mut fs, &dir, DATA);
            assert_eq!(mine, via_themes, "{name}: цепочка {dir}");
        }
    }
}

/// Курсорные темы из `UNSAFE_CURSOR_THEMES` считаются битыми даже когда
/// физически стоят в системе (на этой машине `Adwaita` установлена).
#[test]
fn unsafe_курсоры_отвергаются_на_живых_данных() {
    let gold = golden();
    let case = &gold["live"];
    for theme in g::UNSAFE_CURSOR_THEMES {
        let mut fs = snapshot(case);
        assert!(
            !g::theme_exists(&mut fs, "cursor-theme", theme, HOME, DATA),
            "{theme} должна считаться небезопасной"
        );
    }
}
