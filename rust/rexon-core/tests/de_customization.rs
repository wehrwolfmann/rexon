//! Матрица кастомизации окружений: согласованность таблиц и планы установки.
//!
//! Эталона в Python у модуля нет (это новая функциональность, а не порт),
//! поэтому проверяется не байтовое совпадение, а два свойства:
//!   * все параллельные таблицы типов обновлены (правило #12) — тип из матрицы
//!     обязан быть в `VALID_TYPES`, в категории «Оформление», иметь путь,
//!     метку, иконку и градиент, причём путь — ТОТ ЖЕ, что в матрице;
//!   * план установки/удаления делает ровно то, что обещает строка матрицы:
//!     кладёт в нужный каталог под нужным именем, включает нужной командой и
//!     обновляет окружение только там, где это безопасно.

mod i18n_pin;

use std::collections::HashMap;

use rexon_core::constants;
use rexon_core::de_customization::{self as dec, De, Support};
use rexon_core::ext_manager::{
    safe_install_plan, safe_uninstall_plan, ExtEnv, ExtInfo, ExtStep, FsSnap, TmpAlloc,
};
use serde_json::{json, Map, Value};

// ============================================================================
// Согласованность таблиц
// ============================================================================

#[test]
fn every_matrix_kind_is_registered_everywhere() {
    for kind in dec::kinds() {
        assert!(constants::is_valid_type(kind), "{kind} нет в VALID_TYPES");
        assert!(constants::is_appearance(kind), "{kind} нет в категории «Оформление»");
        assert!(!constants::is_scripted(kind), "{kind} не должен быть скриптовым");
        assert!(constants::type_label(kind).is_some(), "{kind} без метки");
        assert!(constants::type_icon(kind).is_some(), "{kind} без иконки");
        assert!(constants::type_gradient(kind).is_some(), "{kind} без градиента");
    }
}

/// `VALID_TYPES` теперь ВЫВОДИТСЯ из матрицы. Состав обязан остаться прежним:
/// список ниже — снимок того, что было до слияния двух таблиц (16 «безопасных»
/// типов оформления + adaptive-обои + 16 типов кастомизации окружений + 2
/// скриптовых). Расхождение = потерянный или самовольно заведённый тип.
#[test]
fn valid_types_derived_from_matrix_keeps_composition() {
    const BEFORE: &[&str] = &[
        "theme-gtk", "theme-icons", "theme-cursors", "theme-sounds",
        "theme-kde-global", "theme-kde-colors", "theme-kde-plasma",
        "theme-kvantum", "theme-window-decoration", "theme-login-screen",
        "theme-plymouth", "theme-conky", "theme-bundle", "theme-plasmoid",
        "wallpaper-pack", "font-pack", "wallpaper-video-adaptive",
        "kde-kwin-script", "kde-kwin-effect", "kde-window-switcher",
        "kde-wallpaper-plugin", "theme-kde-splash", "theme-kde-lockscreen",
        "gnome-shell-extension", "theme-gnome-shell", "cinnamon-applet",
        "cinnamon-desklet", "cinnamon-extension", "theme-cinnamon",
        "nemo-action", "theme-xfwm4", "theme-marco", "theme-lxqt",
        // 01.11.2026: рамки Openbox и labwc (пункт 5.10 паритета).
        "theme-openbox",
        "desktop-tool", "plugin",
    ];
    let mut got = constants::valid_types();
    let mut want = BEFORE.to_vec();
    got.sort_unstable();
    want.sort_unstable();
    assert_eq!(got, want, "состав VALID_TYPES изменился");

    // Категория «Оформление» = вся матрица, скриптовые типы в неё не входят.
    let mut appearance = constants::category_appearance();
    appearance.sort_unstable();
    let mut want_app: Vec<&str> =
        BEFORE.iter().copied().filter(|t| !constants::is_scripted(t)).collect();
    want_app.sort_unstable();
    assert_eq!(appearance, want_app);
}

#[test]
fn install_paths_match_matrix() {
    for kind in dec::kinds() {
        let row = dec::row(kind).unwrap();
        let expected = if row.dest.is_empty() { None } else { Some(row.dest) };
        assert_eq!(
            constants::install_path_suffix(kind),
            expected,
            "путь {kind} разъехался с матрицей"
        );
    }
    // Набор обоев применяется прямо из пакета — каталога установки у него нет
    // (так было и в исходном INSTALL_PATHS).
    assert_eq!(constants::install_path_suffix("wallpaper-pack"), None);
    assert_eq!(constants::install_path_suffix("plugin"), None);
}

/// Фильтр каталога: покупателю не показывают то, что в его окружении
/// физически не работает, — но и не отнимают то, что работает.
#[test]
fn matrix_filters_by_desktop() {
    let kinds_in = |de: De| -> Vec<&'static str> {
        dec::rows_for(de).iter().map(|r| r.kind).collect()
    };
    // В GNOME нет плазмоидов, зато есть расширения оболочки.
    let gnome = kinds_in(De::Gnome);
    assert!(!gnome.contains(&"theme-plasmoid"));
    assert!(gnome.contains(&"gnome-shell-extension"));
    // В KDE есть и то, и другое своё, и тема GTK (Firefox, GIMP, kde-gtk-config).
    let kde = kinds_in(De::Kde);
    assert!(kde.contains(&"theme-plasmoid"));
    assert!(kde.contains(&"theme-gtk"));
    assert!(!kde.contains(&"gnome-shell-extension"));
    // Budgie оформляется темой GTK — она обязана быть.
    assert!(kinds_in(De::Budgie).contains(&"theme-gtk"));
    // Тайловые: обои и шрифты да, KDE-шные виджеты нет.
    let tiling = kinds_in(De::Wlroots);
    assert!(tiling.contains(&"wallpaper-pack") && tiling.contains(&"font-pack"));
    assert!(!tiling.contains(&"theme-plasmoid"));
}

#[test]
fn unsupported_rows_explain_themselves() {
    let blocked: Vec<&dec::Row> = dec::ROWS
        .iter()
        .filter(|r| !r.support.installable())
        .collect();
    // Нативные плагины панелей XFCE/MATE/Budgie/LXQt, тема GDM, конфиги
    // тайловых, темы COSMIC — все семь строк обязаны нести причину.
    assert!(blocked.len() >= 6);
    for r in blocked {
        let reason = r.support.reason().expect("причина отказа");
        assert!(reason.len() > 20, "слишком короткое объяснение: {reason}");
    }
}

#[test]
fn each_desktop_is_covered() {
    for de in [De::Kde, De::Gnome, De::Cinnamon, De::Xfce, De::Mate, De::Lxqt] {
        assert!(!dec::rows_hosted_by(de).is_empty(), "{} без строк", de.label());
    }
    // Cinnamon — три РАЗНЫХ типа дополнений плюс тема и действия Nemo.
    assert_eq!(dec::rows_hosted_by(De::Cinnamon).len(), 5);
}

// ============================================================================
// Документация — из той же таблицы
// ============================================================================

/// Собрать `docs/de_customization_matrix.md` из матрицы.
fn render_doc() -> String {
    let mut s = String::new();
    s.push_str(
        "# Кастомизация рабочих столов Linux: разбор по окружениям\n\n\
         Полная карта того, чем оформляется каждый рабочий стол, куда это \
         кладётся, чем включается и что нужно перезапустить. Таблица — рабочая \
         основа универсального `.rxn`: типы из колонки «Тип `.rxn`» Rexon ставит \
         и включает сам.\n\n\
         > Файл СОБИРАЕТСЯ из `rust/rexon-core/src/de_customization.rs`\n\
         > (тест `matrix_doc_is_up_to_date`). Править нужно матрицу, а не этот\n\
         > текст: обновление — `REXON_UPDATE_DOC=1 cargo test -p rexon-core\n\
         > --test de_customization`.\n",
    );

    // Общие каталоги: куда класть — одинаково везде. Колонка «Где работает»
    // отвечает на ВТОРОЙ вопрос матрицы: общий каталог не значит «во всех
    // окружениях» (тема Kvantum).
    s.push_str("\n## Общий каталог (кладётся одинаково везде)\n\n");
    s.push_str(
        "| Тип `.rxn` | Что это | Куда положить | Где работает | Чем включить |\n\
         |---|---|---|---|---|\n",
    );
    for r in dec::rows_hosted_by(De::Any) {
        let path = if r.dest.is_empty() {
            "— (применяется на месте)".to_string()
        } else {
            format!("`~/{}`", r.dest)
        };
        s.push_str(&format!(
            "| `{}` | {} | {path} | {} | {} |\n",
            r.kind,
            r.what,
            squash(&r.applies.hint()),
            r.activate.hint(),
        ));
    }
    let common_notes: Vec<&dec::Row> = dec::rows_hosted_by(De::Any)
        .into_iter()
        .filter(|r| !r.note.is_empty())
        .collect();
    if !common_notes.is_empty() {
        s.push_str("\nПодводные камни:\n\n");
        for r in common_notes {
            s.push_str(&format!(
                "* **{}** — {}\n",
                constants::type_label(r.kind).unwrap_or_else(|| r.kind.to_string()),
                squash(r.note)
            ));
        }
    }

    // Оговорки: тип работает, но при условии. Это НЕ то же, что «не работает»
    // — раздел каталога остаётся на месте, а условие человеку называется.
    let caveats: Vec<(&dec::Row, dec::Caveat)> = dec::ROWS
        .iter()
        .filter_map(|r| match r.applies {
            dec::Applies::EverywhereWith(c) | dec::Applies::EverywhereWithExcept(_, c) => Some((r, c)),
            _ => None,
        })
        .collect();
    if !caveats.is_empty() {
        s.push_str("\nРаботает, но с оговоркой:\n\n");
        for (r, c) in caveats {
            s.push_str(&format!(
                "* **{}** ({}) — {}\n",
                constants::type_label(r.kind).unwrap_or_else(|| r.kind.to_string()),
                c.list()
                    .into_iter()
                    .map(|d| d.display_label())
                    .collect::<Vec<_>>()
                    .join(", "),
                squash(&c.text())
            ));
        }
    }

    for de in [
        De::Kde,
        De::Gnome,
        De::Cinnamon,
        De::Xfce,
        De::Mate,
        De::Budgie,
        De::Lxqt,
        De::Cosmic,
        De::Wlroots,
    ] {
        let rows = dec::rows_hosted_by(de);
        if rows.is_empty() {
            continue;
        }
        s.push_str(&format!("\n## {}\n", de.label()));

        let supported: Vec<&dec::Row> =
            rows.iter().copied().filter(|r| r.support.installable()).collect();
        if !supported.is_empty() {
            s.push('\n');
            s.push_str(
                "| Тип `.rxn` | Что это | Куда положить | Чем включить | Что перезапустить |\n\
                 |---|---|---|---|---|\n",
            );
            for r in &supported {
                s.push_str(&format!(
                    "| `{}` | {} | `~/{}` | {} | {} |\n",
                    r.kind,
                    r.what,
                    r.dest,
                    r.activate.hint(),
                    r.refresh.hint(),
                ));
            }
            let notes: Vec<&&dec::Row> =
                supported.iter().filter(|r| !r.note.is_empty()).collect();
            if !notes.is_empty() {
                s.push_str("\nПодводные камни:\n\n");
                for r in notes {
                    s.push_str(&format!(
                        "* **{}** — {}\n",
                        constants::type_label(r.kind).unwrap_or_else(|| r.kind.to_string()),
                        squash(r.note)
                    ));
                }
            }
        }

        let blocked: Vec<&dec::Row> =
            rows.iter().copied().filter(|r| !r.support.installable()).collect();
        if !blocked.is_empty() {
            s.push_str("\nЧерез `.rxn` не раздаётся:\n\n");
            for r in blocked {
                s.push_str(&format!(
                    "* **{}** (`{}`) — {}.{}\n",
                    r.what,
                    r.dest,
                    squash(r.support.reason().unwrap()),
                    if r.note.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", squash(r.note))
                    }
                ));
            }
        }
    }
    s
}

/// Свернуть перенос строки исходника в один пробел (в Rust строки таблицы
/// разбиты по ширине, в markdown они должны быть цельными).
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Документация не должна отставать от таблицы: обновляется тем же тестом с
/// `REXON_UPDATE_DOC=1`.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn matrix_doc_is_up_to_date() {
    // Документ пишется по-русски: подписи типов берутся из `po/ru.po`.
    let _lang = i18n_pin::ru();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/de_customization_matrix.md");
    let doc = render_doc();
    if std::env::var("REXON_UPDATE_DOC").is_ok() {
        std::fs::write(path, &doc).expect("запись документа");
        return;
    }
    let on_disk = std::fs::read_to_string(path).expect("docs/de_customization_matrix.md");
    assert_eq!(
        on_disk, doc,
        "документ отстал от матрицы — обнови: REXON_UPDATE_DOC=1 cargo test \
         -p rexon-core --test de_customization"
    );
}

// ============================================================================
// Стенд для планов
// ============================================================================

struct Case {
    fs: FsSnap,
    which: HashMap<String, String>,
    ok: bool,
    wayland: bool,
}

impl Case {
    fn new(snapshot: Value, tools: &[&str], ok: bool, wayland: bool) -> Self {
        Case {
            fs: FsSnap::from_json(&snapshot),
            which: tools
                .iter()
                .map(|t| ((*t).to_string(), format!("/usr/bin/{t}")))
                .collect(),
            ok,
            wayland,
        }
    }

    /// Запущенный стол стенда: Cinnamon, если среди программ есть `cinnamon`
    /// (перезапуск оболочки — только в своём сеансе), иначе не опознан.
    fn xdg(&self) -> &'static str {
        if self.which.contains_key("cinnamon") { "X-Cinnamon" } else { "" }
    }

    fn plan(&self, manifest: &Map<String, Value>, uninstall: bool) -> Vec<ExtStep> {
        let tmp = TmpAlloc::new("/tmp/t");
        let which = |n: &str| self.which.get(n).cloned();
        let run_ok = |_: &[String]| self.ok;
        let env = ExtEnv {
            home: "/home/t",
            fs: &self.fs,
            which: &which,
            run_ok: &run_ok,
            xdg_current_desktop: self.xdg(),
            xdg_session_type: if self.wayland { "wayland" } else { "x11" },
            wayland_display: self.wayland,
            stage_helper_available: false,
            system_helper_path: "/usr/lib/rexon/helper",
            system_helper_exists: false,
            helper_family: "arch",
            tmp: &tmp,
        };
        let ext = ExtInfo { dir: "/ext", manifest };
        if uninstall {
            safe_uninstall_plan(&env, &ext)
        } else {
            safe_install_plan(&env, &ext)
        }
    }

    /// Шаги кнопки «Применить»/«Включить» раздела окружения — то, что до
    /// 04.10.2026 делала установка.
    fn enable(&self, kind: &str, name: &str) -> Vec<ExtStep> {
        let tmp = TmpAlloc::new("/tmp/t");
        let which = |n: &str| self.which.get(n).cloned();
        let run_ok = |_: &[String]| self.ok;
        let env = ExtEnv {
            home: "/home/t",
            fs: &self.fs,
            which: &which,
            run_ok: &run_ok,
            xdg_current_desktop: self.xdg(),
            xdg_session_type: if self.wayland { "wayland" } else { "x11" },
            wayland_display: self.wayland,
            stage_helper_available: false,
            system_helper_path: "/usr/lib/rexon/helper",
            system_helper_exists: false,
            helper_family: "arch",
            tmp: &tmp,
        };
        rexon_core::ext_manager::de_enable_steps(&env, kind, name)
    }
}

fn manifest(id: &str, t: &str) -> Map<String, Value> {
    json!({"id": id, "type": t}).as_object().unwrap().clone()
}

/// Снимок: `data/<sub>` с файлом метаданных заданного содержимого.
fn snap_with_meta(sub: &str, meta_name: &str, meta_text: &str, extra_dirs: &[&str]) -> Value {
    let mut dirs = json!({
        "/ext": ["data"],
        "/ext/data": [sub],
        format!("/ext/data/{sub}"): [meta_name],
    });
    for d in extra_dirs {
        dirs[format!("/ext/data/{sub}/{d}")] = json!([]);
    }
    json!({
        "dirs": dirs,
        "files": { format!("/ext/data/{sub}/{meta_name}"): {"size": 1, "text": meta_text} },
    })
}

fn argvs(steps: &[ExtStep]) -> Vec<Vec<String>> {
    steps
        .iter()
        .filter_map(|s| match s {
            ExtStep::Run { argv, .. } | ExtStep::Spawn { argv } => Some(argv.clone()),
            _ => None,
        })
        .collect()
}

fn copies(steps: &[ExtStep]) -> Vec<(String, String)> {
    steps
        .iter()
        .filter_map(|s| match s {
            ExtStep::CopyTree { src, dest } | ExtStep::CopyFile { src, dest } => {
                Some((src.clone(), dest.clone()))
            }
            _ => None,
        })
        .collect()
}

// ============================================================================
// Планы установки
// ============================================================================

/// GNOME: каталог назначения называется по uuid из metadata.json, а НЕ по имени
/// папки в архиве — иначе оболочка расширение не найдёт.
#[test]
fn gnome_extension_installs_under_uuid_and_enables() {
    let case = Case::new(
        snap_with_meta(
            "my-extension-v3",
            "metadata.json",
            r#"{"uuid": "my@ext.example.com", "shell-version": ["47", "48"]}"#,
            &["schemas"],
        ),
        &["gnome-extensions", "glib-compile-schemas"],
        true,
        true,
    );
    let steps = case.plan(&manifest("my-ext", "gnome-shell-extension"), false);

    assert_eq!(
        copies(&steps),
        vec![(
            "/ext/data/my-extension-v3".to_string(),
            "/home/t/.local/share/gnome-shell/extensions/my@ext.example.com".to_string(),
        )]
    );
    // Установка только кладёт (решение 04.10.2026), включает кнопка.
    assert!(argvs(&steps).is_empty(), "{steps:?}");
    let on = case.enable("gnome-shell-extension", "my@ext.example.com");
    let script = &argvs(&on)[0][2];
    assert!(script.contains("gnome-extensions enable 'my@ext.example.com'"));
    assert!(script.contains("glib-compile-schemas"));
    assert!(script.contains("/home/t/.local/share/gnome-shell/extensions/my@ext.example.com/schemas"));
    // Оболочку GNOME не перезапускаем — на Wayland это невозможно.
    assert!(!argvs(&on).iter().any(|a| a.iter().any(|s| s.contains("gnome-shell --replace"))));
}

/// Cinnamon: апплет дописывается строкой в список со счётчиком экземпляров,
/// а на X11 оболочка перезапускается сама.
#[test]
fn cinnamon_applet_enables_and_restarts_on_x11() {
    let case = Case::new(
        snap_with_meta("weather", "metadata.json", r#"{"uuid": "weather@mockturtl"}"#, &[]),
        &["cinnamon"],
        true,
        false,
    );
    let steps = case.plan(&manifest("weather", "cinnamon-applet"), false);

    assert_eq!(
        copies(&steps)[0].1,
        "/home/t/.local/share/cinnamon/applets/weather@mockturtl"
    );
    assert!(argvs(&steps).is_empty() && !steps.iter().any(|s| matches!(s, ExtStep::Spawn { .. })));
    let on = case.enable("cinnamon-applet", "weather@mockturtl");
    let a = argvs(&on);
    assert!(a[0][2].contains("next-applet-id $((id + 1))"));
    // Как окно настроек Cinnamon (Spices.enable_extension): новый апплет — на
    // нулевое место правой зоны первой панели, соседи по зоне сдвигаются.
    assert!(a[0][2].contains("panel1:right:0:weather@mockturtl:$id"), "в начало правой зоны");
    assert!(a[0][2].contains("$1==\"\\047panel1\" && $2==\"right\""), "сдвигается только правая зона первой панели");
    assert!(a[0][2].contains("{$3=$3+1}"), "соседи по зоне сдвигаются на одно");
    // Номер экземпляра не меньше «наибольший занятый + 1».
    assert!(a[0][2].contains("if [ \"$id\" -le \"$max\" ]; then id=$((max + 1)); fi"), "номер не занят");
    assert_eq!(a.last().unwrap(), &vec!["/usr/bin/cinnamon".to_string(), "--replace".to_string()]);
}

/// На Wayland перезапуск Cinnamon невозможен — шага быть не должно.
#[test]
fn cinnamon_applet_does_not_restart_on_wayland() {
    let case = Case::new(
        snap_with_meta("weather", "metadata.json", r#"{"uuid": "weather@mockturtl"}"#, &[]),
        &["cinnamon"],
        true,
        true,
    );
    let steps = case.plan(&manifest("weather", "cinnamon-applet"), false);
    assert!(!steps.iter().any(|s| matches!(s, ExtStep::Spawn { .. })));
    let on = case.enable("cinnamon-applet", "weather@mockturtl");
    assert!(!on.iter().any(|s| matches!(s, ExtStep::Spawn { .. })));
}

/// KDE: пакет ставится своим инструментом, включается ключом по KPlugin.Id
/// (не по имени папки) и применяется через reconfigure.
#[test]
fn kwin_script_uses_kpackagetool_and_plugin_id() {
    let case = Case::new(
        snap_with_meta(
            "krohnkite-src",
            "metadata.json",
            r#"{"KPlugin": {"Id": "krohnkite"}}"#,
            &[],
        ),
        &["kpackagetool6", "qdbus6"],
        true,
        true,
    );
    let steps = case.plan(&manifest("krohnkite", "kde-kwin-script"), false);
    let a = argvs(&steps);

    assert_eq!(a[0][..4], ["/usr/bin/kpackagetool6", "--type", "KWin/Script", "-u"]);
    assert_eq!(a[1][..4], ["/usr/bin/kpackagetool6", "--type", "KWin/Script", "-i"]);
    // Инструмент справился — копирования быть не должно; включения тоже.
    assert!(copies(&steps).is_empty());
    assert_eq!(a.len(), 2, "{a:?}");
    // Метка владельца и у пакета, поставленного инструментом: по ней
    // удаление узнаёт своё.
    assert!(steps.iter().any(|s| matches!(s, ExtStep::WriteFile { path, content }
        if path == "/home/t/.local/share/kwin/scripts/krohnkite/.rexon-owner" && content == "krohnkite\n")), "{steps:?}");
    let on = argvs(&case.enable("kde-kwin-script", "krohnkite"));
    assert_eq!(
        on[0],
        vec![
            "kwriteconfig6", "--file", "kwinrc", "--group", "Plugins",
            "--key", "krohnkiteEnabled", "true",
        ]
    );
    assert_eq!(on[1], vec!["/usr/bin/qdbus6", "org.kde.KWin", "/KWin", "reconfigure"]);
}

/// Скрипт KWin с тем же KPlugin.Id уже стоит и положен не этим пакетом
/// (без метки — руками или прежним Rexon): ни `kpackagetool -u/-i` при
/// установке, ни `-r` при удалении, ни копии — только заметка с причиной.
#[test]
fn kwin_script_never_replaces_foreign_package() {
    english();
    let mut snapshot =
        snap_with_meta("src", "metadata.json", r#"{"KPlugin": {"Id": "myscript"}}"#, &[]);
    snapshot["dirs"]["/home/t/.local/share/kwin/scripts/myscript"] = json!(["main.js"]);
    snapshot["files"]["/home/t/.local/share/kwin/scripts/myscript/main.js"] = json!({"size": 4, "text": "own"});
    let case = Case::new(snapshot, &["kpackagetool6", "qdbus6"], true, true);
    let m = manifest("myscript", "kde-kwin-script");
    let install = case.plan(&m, false);
    let kpt = |steps: &[ExtStep]| {
        steps.iter().any(|s| matches!(s, ExtStep::Run { argv, .. } if argv[0].ends_with("kpackagetool6")))
    };
    assert!(!kpt(&install) && copies(&install).is_empty(), "установка тронула чужой скрипт: {install:?}");
    // Скрипт в пакете один, и он пропущен — установка отказывает целиком
    // (шаг-отказ с причиной), а не «успех» без единого положенного файла.
    assert!(
        matches!(install.as_slice(), [ExtStep::Refuse { text }] if text.contains("skipped") && text.contains("myscript")),
        "{install:?}"
    );
    let uninstall = case.plan(&m, true);
    assert!(!kpt(&uninstall), "удаление сняло чужой скрипт: {uninstall:?}");
    assert!(!uninstall.iter().any(|s| matches!(s, ExtStep::RemoveTree { .. })), "{uninstall:?}");
}

/// Если kpackagetool отработал с ошибкой — падаем на копирование, а не молча
/// «устанавливаем» ничего.
#[test]
fn kwin_script_falls_back_to_copy() {
    let case = Case::new(
        snap_with_meta("src", "metadata.json", r#"{"KPlugin": {"Id": "myscript"}}"#, &[]),
        &["kpackagetool6"],
        false,
        true,
    );
    let steps = case.plan(&manifest("myscript", "kde-kwin-script"), false);
    assert_eq!(
        copies(&steps)[0].1,
        "/home/t/.local/share/kwin/scripts/myscript"
    );
}

/// XFCE: тема окон кладётся в ~/.themes, включает её кнопка xfconf-query.
#[test]
fn xfwm4_theme_copies_to_themes_and_switches() {
    let snapshot = json!({
        "dirs": {
            "/ext": ["data"],
            "/ext/data": ["Arc-Dark"],
            "/ext/data/Arc-Dark": ["xfwm4"],
            "/ext/data/Arc-Dark/xfwm4": ["themerc"],
        },
        "files": {"/ext/data/Arc-Dark/xfwm4/themerc": {"size": 1, "text": ""}},
    });
    let case = Case::new(snapshot, &["xfconf-query"], true, false);
    let steps = case.plan(&manifest("arc", "theme-xfwm4"), false);

    assert_eq!(copies(&steps)[0].1, "/home/t/.themes/Arc-Dark");
    assert!(argvs(&steps).is_empty(), "{steps:?}");
    assert_eq!(
        argvs(&case.enable("theme-xfwm4", "Arc-Dark"))[0],
        vec!["xfconf-query", "-c", "xfwm4", "-p", "/general/theme", "-n", "-t", "string", "-s", "Arc-Dark"]
    );
}

/// Действие Nemo — отдельный файл, включать нечего.
#[test]
fn nemo_action_copies_file_without_activation() {
    let snapshot = json!({
        "dirs": {"/ext": ["data"], "/ext/data": ["open-here.nemo_action", "readme.txt"]},
        "files": {
            "/ext/data/open-here.nemo_action": {"size": 1, "text": ""},
            "/ext/data/readme.txt": {"size": 1, "text": ""},
        },
    });
    let case = Case::new(snapshot, &[], true, false);
    let steps = case.plan(&manifest("open-here", "nemo-action"), false);

    assert_eq!(
        copies(&steps),
        vec![(
            "/ext/data/open-here.nemo_action".to_string(),
            "/home/t/.local/share/nemo/actions/open-here.nemo_action".to_string(),
        )]
    );
    assert!(argvs(&steps).is_empty());
}

/// LXQt: тема включается ключом в личном lxqt.conf, панель не трогаем.
#[test]
fn lxqt_theme_writes_config_key() {
    let snapshot = json!({
        "dirs": {
            "/ext": ["data"],
            "/ext/data": ["Frost"],
            "/ext/data/Frost": ["lxqt-panel.qss"],
        },
        "files": {"/ext/data/Frost/lxqt-panel.qss": {"size": 1, "text": ""}},
    });
    let case = Case::new(snapshot, &[], true, false);
    let steps = case.plan(&manifest("frost", "theme-lxqt"), false);

    assert_eq!(copies(&steps)[0].1, "/home/t/.local/share/lxqt/themes/Frost");
    assert!(argvs(&steps).is_empty(), "{steps:?}");
    let on = case.enable("theme-lxqt", "Frost");
    let script = &argvs(&on)[0][2];
    assert!(script.contains("/home/t/.config/lxqt/lxqt.conf"));
    assert!(script.contains("theme=Frost"));
}

// ============================================================================
// Планы удаления
// ============================================================================

/// Экран блокировки: ключ обязан сбрасываться ДО удаления каталога, иначе
/// KDE останется со ссылкой в никуда и блокировка не соберётся.
#[test]
fn lockscreen_uninstall_resets_key_before_removing() {
    let snapshot = json!({
        "dirs": {
            "/ext": ["data"],
            "/ext/data": ["org.rexon.lock"],
            "/ext/data/org.rexon.lock": ["contents"],
            "/ext/data/org.rexon.lock/contents": [],
            "/home/t/.local/share/plasma/look-and-feel/org.rexon.lock": [".rexon-owner"],
        },
        // Метка владельца: каталог положил этот пакет (чужой удаление не снимает).
        "files": {
            "/home/t/.local/share/plasma/look-and-feel/org.rexon.lock/.rexon-owner": {"size": 5, "text": "lock\n"},
        },
    });
    let case = Case::new(snapshot, &["kwriteconfig6"], true, true);
    let steps = case.plan(&manifest("lock", "theme-kde-lockscreen"), true);

    let reset_at = steps
        .iter()
        .position(|s| matches!(s, ExtStep::Run { argv, .. } if argv[2].contains("kscreenlockerrc")))
        .expect("сброс ключа экрана блокировки");
    let remove_at = steps
        .iter()
        .position(|s| matches!(s, ExtStep::RemoveTree { .. }))
        .expect("удаление каталога");
    assert!(reset_at < remove_at, "каталог удалён раньше сброса ключа");
}

/// Удаление расширения GNOME: выключить и убрать из списка, затем снести.
#[test]
fn gnome_extension_uninstall_disables_and_removes() {
    let mut snapshot = snap_with_meta(
        "my-extension-v3",
        "metadata.json",
        r#"{"uuid": "my@ext.example.com"}"#,
        &[],
    );
    let dest = "/home/t/.local/share/gnome-shell/extensions/my@ext.example.com";
    snapshot["dirs"][dest] = json!([".rexon-owner"]);
    snapshot["files"][format!("{dest}/.rexon-owner")] = json!({"size": 7, "text": "my-ext\n"});
    let case = Case::new(snapshot, &["gnome-extensions"], true, true);
    let steps = case.plan(&manifest("my-ext", "gnome-shell-extension"), true);

    let script = &argvs(&steps)[0][2];
    assert!(script.contains("gnome-extensions disable 'my@ext.example.com'"));
    assert!(script.contains("gsettings set org.gnome.shell enabled-extensions"));
    assert!(steps.iter().any(|s| matches!(
        s,
        ExtStep::RemoveTree { path, .. }
            if path == "/home/t/.local/share/gnome-shell/extensions/my@ext.example.com"
    )));
}

/// Скрипт KWin при удалении обязан выгружаться явно: проверено живьём —
/// `reconfigure` снятый скрипт не выгружает, он работает до конца сеанса.
#[test]
fn kwin_script_uninstall_unloads_script() {
    let mut snapshot =
        snap_with_meta("src", "metadata.json", r#"{"KPlugin": {"Id": "myscript"}}"#, &[]);
    let dest = "/home/t/.local/share/kwin/scripts/myscript";
    snapshot["dirs"][dest] = json!([".rexon-owner"]);
    snapshot["files"][format!("{dest}/.rexon-owner")] = json!({"size": 9, "text": "myscript\n"});
    let case = Case::new(snapshot, &["kpackagetool6", "qdbus6"], true, true);
    let steps = case.plan(&manifest("myscript", "kde-kwin-script"), true);
    let a = argvs(&steps);

    assert!(a.iter().any(|x| x
        == &vec![
            "/usr/bin/qdbus6", "org.kde.KWin", "/Scripting", "unloadScript", "myscript"
        ]));
    // Порядок: выключить → выгрузить → снять пакет → удалить файлы.
    let unload = a.iter().position(|x| x.contains(&"unloadScript".to_string())).unwrap();
    let remove = a.iter().position(|x| x.contains(&"-r".to_string())).unwrap();
    assert!(unload < remove);
}

/// Тема Cinnamon сбрасывается только если выбрана именно она — чужой выбор
/// пользователя удаление нашего пакета ломать не должно.
#[test]
fn cinnamon_theme_uninstall_resets_only_own_theme() {
    let snapshot = json!({
        "dirs": {
            "/ext": ["data"],
            "/ext/data": ["Mint-Y-Dark"],
            "/ext/data/Mint-Y-Dark": ["cinnamon"],
            "/ext/data/Mint-Y-Dark/cinnamon": [],
            "/home/t/.themes/Mint-Y-Dark": [],
        },
        "files": {},
    });
    let case = Case::new(snapshot, &[], true, false);
    let steps = case.plan(&manifest("mint-y", "theme-cinnamon"), true);
    let script = &argvs(&steps)[0][2];
    assert!(script.contains("if [ \"$cur\" = 'Mint-Y-Dark' ]"));
    assert!(script.contains("gsettings reset org.cinnamon.theme name"));
}

/// Строки, которые через .rxn не ставятся, в установщик не попадают вовсе.
#[test]
fn blocked_kinds_have_no_plan() {
    for r in dec::ROWS {
        if let Support::Rxn = r.support {
            continue;
        }
        assert!(dec::row(r.kind).is_none());
    }
}

// ============================================================================
// Установка ≠ применение
// ============================================================================

/// ⛔ Дефект 03.10.2026: установка `.rxn` с темой курсоров молча писала
/// `~/.icons/default/index.theme` (`Inherits=<тема>`) — курсор у X11-программ
/// менялся сразу, до «Применить». Так же при установке применялись цвета,
/// стиль Plasma, глобальная тема, Kvantum, рамки окон KDE, заставка и экран
/// блокировки. Сторож общий: для КАЖДОГО типа, который включает кнопка
/// «Применить» вкладки «Оформление», и в КАЖДОМ окружении план установки
/// только кладёт файлы в каталог типа — ни одной записи настроек, ни одной
/// команды включения. Новый тип с кнопкой попадает под сторож сам.
#[test]
fn rxn_install_never_applies_what_the_button_applies() {
    use rexon_core::appearance_installer::applied_by_button;

    let button_kinds: Vec<&str> =
        dec::kinds().into_iter().filter(|k| applied_by_button(k)).collect();
    // Состав — чтобы сторож не опустел молча, если связь разделов с типами
    // сломается.
    for must in [
        "theme-cursors", "theme-icons", "theme-gtk", "theme-sounds",
        "theme-kvantum", "theme-kde-global", "theme-kde-colors",
        "theme-kde-plasma", "theme-window-decoration", "theme-kde-splash",
        "theme-kde-lockscreen",
        // Решение 04.10.2026: у всех типов, которые раньше включала сама
        // установка, своя кнопка — «Применить», «Включить» или «Запустить».
        "theme-xfwm4", "theme-marco", "theme-gnome-shell", "theme-cinnamon",
        "theme-lxqt", "theme-openbox", "kde-window-switcher", "kde-kwin-script", "kde-kwin-effect",
        "gnome-shell-extension", "cinnamon-applet", "cinnamon-desklet",
        "cinnamon-extension", "nemo-action", "theme-conky", "theme-login-screen",
        "theme-plymouth",
    ] {
        assert!(button_kinds.contains(&must), "{must}: нет кнопки «Применить»?");
    }
    // Виджеты Plasma, шрифты и плагин обоев включать нечем (выбор — в самом
    // окружении) — кнопки у них нет.
    for not in ["theme-plasmoid", "font-pack", "kde-wallpaper-plugin"] {
        assert!(!applied_by_button(not), "{not}: неожиданная кнопка «Применить»");
    }

    // Пакет, похожий на тему любого вида: подпапка с курсорами, содержимым
    // look-and-feel и метаданными плюс файл цветовой схемы.
    // Для типов окружений там же подкаталоги рамок и оболочек, uuid в
    // метаданных, панель LXQt, файл Conky и действие Nemo; каталог Conky уже
    // «скопирован» — прежняя установка по нему писала автозапуск и запускала.
    let snapshot = json!({
        "dirs": {
            "/ext": ["data", "manifest.json"],
            "/ext/data": ["MyTheme", "MyTheme.colors", "my.nemo_action"],
            "/ext/data/MyTheme": [
                "cursors", "contents", "index.theme", "metadata.json", "xfwm4",
                "metacity-1", "gnome-shell", "cinnamon", "lxqt-panel.qss", "conky.conf", "openbox-3",
            ],
            "/ext/data/MyTheme/cursors": [],
            "/ext/data/MyTheme/contents": [],
            "/ext/data/MyTheme/xfwm4": [],
            "/ext/data/MyTheme/metacity-1": [],
            "/ext/data/MyTheme/gnome-shell": [],
            "/ext/data/MyTheme/cinnamon": [],
            "/ext/data/MyTheme/openbox-3": [],
            "/home/t/.config/conky/my-theme": ["conky.conf"],
        },
        "files": {
            "/ext/data/MyTheme.colors": {"size": 10, "text": "[General]\n"},
            "/ext/data/my.nemo_action": {"size": 10, "text": "[Nemo Action]\n"},
            "/ext/data/MyTheme/index.theme": {"size": 10, "text": "[Icon Theme]\nName=MyTheme\n"},
            "/ext/data/MyTheme/metadata.json": {"size": 10, "text": "{\"uuid\": \"my@rexon\", \"KPlugin\": {\"Id\": \"MyTheme\"}}"},
            "/ext/data/MyTheme/lxqt-panel.qss": {"size": 10, "text": ""},
            "/ext/data/MyTheme/conky.conf": {"size": 10, "text": "conky.text = [[x]]"},
            "/home/t/.config/conky/my-theme/conky.conf": {"size": 10, "text": "conky.text = [[x]]"},
        },
    });
    let fs = FsSnap::from_json(&snapshot);
    let tools: HashMap<String, String> = [
        "kwriteconfig6", "plasma-apply-colorscheme", "plasma-apply-cursortheme",
        "plasma-apply-lookandfeel", "plasma-apply-desktoptheme", "kvantummanager",
        "gsettings", "xfconf-query", "qdbus6", "kpackagetool6", "dbus-send",
        "pkexec", "conky", "cinnamon", "gnome-extensions", "mkinitcpio",
    ]
    .iter()
    .map(|t| ((*t).to_string(), format!("/usr/bin/{t}")))
    .collect();
    // Инструменты установки (не включения) — им можно.
    const INSTALL_TOOLS: &[&str] = &["kpackagetool6", "kpackagetool5", "fc-cache"];

    // Кнопка есть не во всех окружениях: раздел рамок (Aurorae) — только в
    // KDE, и на XFCE включение рамок остаётся за установкой (xfwm4), иначе
    // включить их было бы нечем. Сторож — там, где кнопка ЕСТЬ.
    let button_here = |kind: &str, de_id: &str| -> bool {
        rexon_core::categories::categories(std::path::Path::new("/")).iter().any(|c| {
            rexon_core::categories::category_kind(&c.key) == Some(kind)
                && rexon_core::categories::category_available(&c.key, de_id)
        })
    };
    for (xdg, de_id) in [
        ("KDE", "kde"), ("GNOME", "gnome"), ("XFCE", "xfce"), ("X-Cinnamon", "cinnamon"),
        ("MATE", "mate"), ("LXQt", "lxqt"), ("sway", "sway"), ("", ""),
    ] {
        // Системные темы (экран входа, заставка) — и с помощником Rexon, и
        // запасным путём без него.
        for helper in [false, true] {
        for kind in button_kinds.iter().filter(|k| de_id.is_empty() || button_here(k, de_id)) {
            let tmp = TmpAlloc::new("/tmp/t");
            let which = |n: &str| tools.get(n).cloned();
            let run_ok = |_: &[String]| false;
            let env = ExtEnv {
                home: "/home/t",
                fs: &fs,
                which: &which,
                run_ok: &run_ok,
                xdg_current_desktop: xdg,
                xdg_session_type: "wayland",
                wayland_display: true,
                stage_helper_available: false,
                system_helper_path: "/usr/lib/rexon/helper",
                system_helper_exists: helper,
                helper_family: "arch",
                tmp: &tmp,
            };
            let m = manifest("my-theme", kind);
            let steps = safe_install_plan(&env, &ExtInfo { dir: "/ext", manifest: &m });
            for step in &steps {
                let bad = match step {
                    // Свой файл внутри пакета (недостающий `.plymouth`) и метка
                    // владельца копии — не настройка окружения.
                    ExtStep::WriteFile { path, .. } => {
                        !path.starts_with("/ext/")
                            && !path.ends_with(&format!("/{}", rexon_core::ext_manager::OWNER_MARK))
                    }
                    // Системная часть: только копия темы, без выбора.
                    ExtStep::PkexecScript { script, .. } => {
                        script.contains("zz-rexon-theme") || script.contains("plymouth-set-default-theme")
                    }
                    // Проба «какой экран входа работает» ничего не меняет.
                    ExtStep::Run { argv, .. } if argv[0] == "systemctl" => {
                        argv.get(1).map(String::as_str) != Some("is-active")
                    }
                    ExtStep::Run { argv, .. } if argv[0] == "pkexec" => {
                        // Помощник в режиме установки: вид — `sddm`/`plymouth`,
                        // режим выбора (`apply-…`) установке нельзя.
                        argv.iter().any(|a| a.starts_with("apply-"))
                    }
                    ExtStep::Run { argv, .. } | ExtStep::Spawn { argv } => {
                        let tool = argv[0].rsplit('/').next().unwrap_or("");
                        !INSTALL_TOOLS.contains(&tool)
                    }
                    _ => false,
                };
                assert!(!bad, "{kind} в «{xdg}»: установка применяет тему — {step:?}");
                assert!(
                    !format!("{step:?}").contains(".icons/default"),
                    "{kind} в «{xdg}»: установка трогает ~/.icons/default — {step:?}"
                );
            }
            // И сама установка не пустая: файлы темы кладутся (системные —
            // через помощник или запасной сценарий копии).
            // (Экран входа без работающего менеджера входа на стенде честно
            // не ставится вовсе — тема не к кому.)
            if *kind != "wallpaper-pack" && *kind != "theme-login-screen" {
                assert!(
                    steps.iter().any(|s| matches!(
                        s,
                        ExtStep::CopyTree { .. }
                            | ExtStep::CopyFile { .. }
                            | ExtStep::PkexecScript { .. }
                            | ExtStep::Stage { .. }
                    ) || matches!(s, ExtStep::Run { argv, .. } if argv[0].ends_with("kpackagetool6"))),
                    "{kind} в «{xdg}»: план установки ничего не копирует: {steps:?}"
                );
            }
        }
        }
    }

    // Рамки окон (`theme-window-decoration`) на XFCE ложатся в ~/.themes и
    // включаются кнопкой раздела «Рамки окон (XFCE)», а не установкой.
    let tmp = TmpAlloc::new("/tmp/t");
    let which = |n: &str| tools.get(n).cloned();
    let run_ok = |_: &[String]| false;
    let env = ExtEnv {
        home: "/home/t",
        fs: &fs,
        which: &which,
        run_ok: &run_ok,
        xdg_current_desktop: "XFCE",
        xdg_session_type: "x11",
        wayland_display: false,
        stage_helper_available: false,
        system_helper_path: "/usr/lib/rexon/helper",
        system_helper_exists: false,
        helper_family: "arch",
        tmp: &tmp,
    };
    let m = manifest("my-theme", "theme-window-decoration");
    let steps = safe_install_plan(&env, &ExtInfo { dir: "/ext", manifest: &m });
    assert!(
        !steps.iter().any(|s| format!("{s:?}").contains("xfconf-query")),
        "рамки на XFCE: установка включает тему — {steps:?}"
    );
}

/// Кнопка раздела окружения пишет ровно то, что прежде писала установка:
/// шаги включения — те же, что у установки до решения 04.10.2026, а
/// выключение — те же, что удаление делает перед сносом.
#[test]
fn section_button_writes_what_install_used_to() {
    use rexon_core::ext_manager::{de_disable_steps, de_enable_steps};
    let fs = FsSnap::from_json(&json!({}));
    let tmp = TmpAlloc::new("/tmp/t");
    let which = |n: &str| Some(format!("/usr/bin/{n}"));
    let run_ok = |_: &[String]| false;
    let env = ExtEnv {
        home: "/home/t",
        fs: &fs,
        which: &which,
        run_ok: &run_ok,
        xdg_current_desktop: "XFCE",
        xdg_session_type: "x11",
        wayland_display: false,
        stage_helper_available: false,
        system_helper_path: "",
        system_helper_exists: false,
        helper_family: "arch",
        tmp: &tmp,
    };
    let argv = |kind: &str, on: bool| -> Vec<Vec<String>> {
        let steps = if on { de_enable_steps(&env, kind, "Name") } else { de_disable_steps(&env, kind, "Name") };
        steps
            .into_iter()
            .map(|s| match s {
                ExtStep::Run { argv, .. } | ExtStep::Spawn { argv } => argv,
                other => panic!("{kind}: шаг не команда: {other:?}"),
            })
            .collect()
    };
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(argv("theme-xfwm4", true), vec![s(&["xfconf-query", "-c", "xfwm4", "-p", "/general/theme", "-n", "-t", "string", "-s", "Name"])]);
    assert_eq!(argv("theme-marco", true), vec![s(&["gsettings", "set", "org.mate.Marco.general", "theme", "Name"])]);
    assert_eq!(argv("theme-cinnamon", true), vec![s(&["gsettings", "set", "org.cinnamon.theme", "name", "Name"])]);
    assert_eq!(
        argv("theme-gnome-shell", true),
        vec![s(&["gsettings", "set", "org.gnome.shell.extensions.user-theme", "name", "Name"])]
    );
    assert_eq!(
        argv("kde-window-switcher", true)[0],
        s(&["kwriteconfig6", "--file", "kwinrc", "--group", "TabBox", "--key", "LayoutName", "Name"])
    );
    // Скрипт KWin: ключ плагина + reconfigure; выключение — false + выгрузка.
    let on = argv("kde-kwin-script", true);
    assert_eq!(on[0], s(&["kwriteconfig6", "--file", "kwinrc", "--group", "Plugins", "--key", "NameEnabled", "true"]));
    assert!(on[1].ends_with(&s(&["org.kde.KWin", "/KWin", "reconfigure"])));
    let off = argv("kde-kwin-script", false);
    assert_eq!(off[0], s(&["kwriteconfig6", "--file", "kwinrc", "--group", "Plugins", "--key", "NameEnabled", "false"]));
    assert!(off.iter().any(|a| a.contains(&"unloadScript".to_string())));
    // Апплет Cinnamon: добавить в список / убрать из него.
    assert!(argv("cinnamon-applet", true)[0][2].contains("next-applet-id"));
    assert!(argv("cinnamon-applet", false)[0][2].contains("grep -vF ':Name:'"));
    // Расширение GNOME: штатная утилита, запасной путь — список.
    assert!(argv("gnome-shell-extension", true)[0][2].contains("gnome-extensions enable 'Name'"));
    assert!(argv("gnome-shell-extension", false)[0][2].contains("gnome-extensions disable 'Name'"));
}

/// Удаление `.rxn` с темой заставки или экрана входа снимает её каталог в
/// `/usr/share` через системный помощник (режим remove-<вид>) — прежде
/// системная часть оставалась на месте молча. Нет темы в системе или нет
/// помощника — шага нет (окно пароля впустую не показывается).
#[test]
fn rxn_uninstall_removes_system_theme_through_helper() {
    let tmp = TmpAlloc::new("/tmp/t");
    let which = |n: &str| Some(format!("/usr/bin/{n}"));
    let run_ok = |_: &[String]| false;
    for (ext_type, kind) in [("theme-plymouth", "plymouth"), ("theme-login-screen", "sddm")] {
        let themes = format!("/usr/share/{kind}/themes");
        let with_theme = FsSnap::from_json(&json!({"dirs": {
            "/ext": ["data"], "/ext/data": ["neon"], "/ext/data/neon": [],
            (themes.clone()): ["neon"], (format!("{themes}/neon")): [],
        }}));
        let without = FsSnap::from_json(&json!({"dirs": {
            "/ext": ["data"], "/ext/data": ["neon"], "/ext/data/neon": [], (themes.clone()): [],
        }}));
        let m = manifest("neon-pack", ext_type);
        let plan = |fs: &FsSnap, helper: bool| {
            let env = ExtEnv {
                home: "/home/t",
                fs,
                which: &which,
                run_ok: &run_ok,
                xdg_current_desktop: "KDE",
                xdg_session_type: "wayland",
                wayland_display: true,
                stage_helper_available: false,
                system_helper_path: "/nonexistent/rexon-system-helper",
                system_helper_exists: helper,
                helper_family: "arch",
                tmp: &tmp,
            };
            safe_uninstall_plan(&env, &ExtInfo { dir: "/ext", manifest: &m })
        };
        let steps = plan(&with_theme, true);
        let argv: Vec<&Vec<String>> = steps
            .iter()
            .filter_map(|s| match s {
                ExtStep::Run { argv, .. } => Some(argv),
                _ => None,
            })
            .collect();
        assert_eq!(argv.len(), 1, "{ext_type}: {steps:?}");
        assert_eq!(argv[0][0], "pkexec");
        let tail: Vec<&str> = argv[0][argv[0].len() - 4..].iter().map(String::as_str).collect();
        assert_eq!(tail, [format!("remove-{kind}").as_str(), "neon", "-", "arch"], "{ext_type}");
        assert!(plan(&without, true).is_empty(), "{ext_type}: темы в системе нет");
        assert!(plan(&with_theme, false).is_empty(), "{ext_type}: помощника нет");
    }
}

/// Окно виджета Conky по рабочему столу (`conky_overlay::window_preset`):
/// 13 окружений × X11/Wayland. На X11 — везде обычное окно с подсказками
/// «под всеми окнами»; на Wayland со слоями — слой фона, у KWin с conky
/// 1.25 — обычное окно с пространством имён `desktop`, у niri — нижний
/// слой; GNOME, Cinnamon и Pantheon на Wayland слоёв не знают — совета нет,
/// есть предупреждение.
#[test]
fn conky_window_preset_matrix() {
    use rexon_core::conky_overlay::{preset_note, window_preset, DESKTOP_HINTS};
    let v125 = Some((1, 25));
    let des = [
        "kde", "gnome", "cinnamon", "xfce", "mate", "budgie", "pantheon", "deepin", "lxqt", "cosmic", "hyprland",
        "sway", "niri",
    ];
    for de in des {
        let x = window_preset(de, false, v125).unwrap_or_else(|| panic!("{de}: X11"));
        assert_eq!((x.own_window_type, x.hints, x.namespace), ("normal", Some(DESKTOP_HINTS), None), "{de}: X11");
        assert!(preset_note(de, false).is_none(), "{de}: X11 без оговорки");
        let w = window_preset(de, true, v125);
        let wl = w.as_ref().map(|p| (p.own_window_type, p.hints, p.namespace));
        let want = match de {
            "gnome" | "cinnamon" | "pantheon" => None,
            "kde" => Some(("normal", Some(DESKTOP_HINTS), Some("desktop"))),
            "niri" => Some(("normal", Some(DESKTOP_HINTS), None)),
            _ => Some(("desktop", None, None)),
        };
        assert_eq!(wl, want, "{de}: Wayland");
        assert_eq!(preset_note(de, true).is_some(), want.is_none(), "{de}: оговорка Wayland");
        // До 1.25 подсказки не поднимают обычное окно в слой — везде фон.
        if want.is_some() {
            assert_eq!(window_preset(de, true, Some((1, 19))).unwrap().own_window_type, "desktop", "{de}: 1.19");
            assert_eq!(window_preset(de, true, None).unwrap().own_window_type, "desktop", "{de}: версия неизвестна");
        }
    }
    // Оговорка раздела: с conky 1.25 — «обычное окно», без версии — прежняя.
    rexon_core::i18n::set_language(Some("en"));
    let new = rexon_core::de_apply::conky_wayland_note_for("gnome", true, v125).unwrap();
    assert!(new.contains("regular window"), "{new}");
    let old = rexon_core::de_apply::conky_wayland_note_for("gnome", true, None).unwrap();
    assert_eq!(Some(old), rexon_core::de_apply::conky_wayland_note("gnome", true));
    assert!(rexon_core::de_apply::conky_wayland_note_for("kde", true, v125).is_none());
}

// ============================================================================
// Правило владельца: плазмоиды по KPlugin.Id и одиночные файлы
// ============================================================================

/// Положить в снимок файл `path` с текстом `text` (каталог — в листинг).
fn with_file(mut snap: Value, path: &str, text: &str) -> Value {
    let (dir, name) = path.rsplit_once('/').unwrap();
    let mut list = snap["dirs"][dir].as_array().cloned().unwrap_or_default();
    if !list.iter().any(|n| n == name) {
        list.push(json!(name));
    }
    snap["dirs"][dir] = Value::Array(list);
    snap["files"][path] = json!({"size": text.len(), "text": text});
    snap
}

/// Тронул ли план путь `dest`: копия туда, запись внутрь, снятие, инструмент KDE.
fn touches(steps: &[ExtStep], dest: &str) -> bool {
    steps.iter().any(|s| match s {
        ExtStep::CopyTree { dest: d, .. } | ExtStep::CopyFile { dest: d, .. } => d == dest,
        ExtStep::WriteFile { path, .. } => path.starts_with(&format!("{dest}/")),
        ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => path == dest,
        ExtStep::Run { argv, .. } => argv[0].contains("kpackagetool"),
        _ => false,
    })
}

/// Причину пропуска тесты ищут по английскому слову «skipped»: язык
/// процесса задаётся явно, иначе при русской локали машины текст приходит
/// переведённым и тест краснеет (все тесты файла ждут исходный язык).
fn english() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
}

/// План назвал причину пропуска `dest`: заметкой или отказом целиком.
fn says_skipped(steps: &[ExtStep], dest: &str) -> bool {
    steps.iter().any(|s| matches!(s, ExtStep::Notice { text } | ExtStep::Refuse { text }
        if text.contains("skipped") && text.contains(dest)))
}

const PLASMOIDS: &str = "/home/t/.local/share/plasma/plasmoids";

/// Плазмоид каталогом: на месте `plasmoids/<KPlugin.Id>` чужой (без метки —
/// руками, магазином или прежним Rexon; с меткой другого пакета). Ни
/// копии, ни снятия, ни `kpackagetool`; причина названа. Папка названа как
/// id — больше пакету ставить нечего: отказ целиком. Удаление чужое не
/// снимает.
#[test]
fn plasmoid_dir_never_replaces_foreign_with_same_id() {
    english();
    for (inner, text) in [("main.qml", "human"), (".rexon-owner", "other-pack\n")] {
        let snap = snap_with_meta("org.human.clock", "metadata.json", r#"{"KPlugin": {"Id": "org.human.clock"}}"#, &[]);
        let dest = format!("{PLASMOIDS}/org.human.clock");
        let snap = with_file(snap, &format!("{dest}/{inner}"), text);
        let case = Case::new(snap, &["kpackagetool6"], true, false);
        let m = manifest("clock-pack", "theme-plasmoid");
        let install = case.plan(&m, false);
        assert!(!touches(&install, &dest), "{inner}: установка тронула чужой плазмоид: {install:?}");
        assert!(
            matches!(install.as_slice(), [ExtStep::Refuse { text }] if text.contains("skipped") && text.contains(&dest)),
            "{inner}: всё пропущено — нужен отказ с причиной: {install:?}"
        );
        assert_eq!(install.iter().filter(|s| matches!(s, ExtStep::Refuse { .. })).count(), 1);
        let uninstall = case.plan(&m, true);
        assert!(!touches(&uninstall, &dest), "{inner}: удаление сняло чужой плазмоид: {uninstall:?}");
    }
}

/// Свой прежний плазмоид (метка этого пакета) — снимается и кладётся
/// заново, с меткой; удаление его снимает (`kpackagetool -r` и каталог).
#[test]
fn plasmoid_dir_replaces_own_previous_copy() {
    let snap = snap_with_meta("W", "metadata.json", r#"{"KPlugin": {"Id": "org.own.clock"}}"#, &[]);
    let dest = format!("{PLASMOIDS}/org.own.clock");
    let snap = with_file(snap, &format!("{dest}/.rexon-owner"), "clock-pack\n");
    let case = Case::new(snap, &["kpackagetool6"], true, false);
    let m = manifest("clock-pack", "theme-plasmoid");
    let install = case.plan(&m, false);
    assert!(install.iter().any(|s| matches!(s, ExtStep::RemoveTree { path, .. } if *path == dest)), "{install:?}");
    assert!(copies(&install).iter().any(|(_, d)| *d == dest), "{install:?}");
    assert!(install.iter().any(|s| matches!(s, ExtStep::WriteFile { path, content }
        if *path == format!("{dest}/.rexon-owner") && content == "clock-pack\n")), "{install:?}");
    assert!(!says_skipped(&install, &dest), "{install:?}");
    let uninstall = case.plan(&m, true);
    assert!(argvs(&uninstall).iter().any(|a| a == &["/usr/bin/kpackagetool6", "-r", "org.own.clock"]), "{uninstall:?}");
    assert!(uninstall.iter().any(|s| matches!(s, ExtStep::RemoveTree { path, .. } if *path == dest)), "{uninstall:?}");
}

/// Папка в `data/` названа не как id: её общая копия ложится, а сам
/// плазмоид по id (чужой) пропущен — установка частичная, с заметкой.
#[test]
fn plasmoid_partial_skip_is_a_notice_not_a_refusal() {
    let snap = snap_with_meta("W", "metadata.json", r#"{"KPlugin": {"Id": "org.human.clock"}}"#, &[]);
    let dest = format!("{PLASMOIDS}/org.human.clock");
    let snap = with_file(snap, &format!("{dest}/main.qml"), "human");
    let case = Case::new(snap, &[], true, false);
    let install = case.plan(&manifest("clock-pack", "theme-plasmoid"), false);
    assert!(!touches(&install, &dest), "{install:?}");
    assert!(!install.iter().any(|s| matches!(s, ExtStep::Refuse { .. })), "{install:?}");
    assert!(install.iter().any(|s| matches!(s, ExtStep::Notice { text } if text.contains(&dest))), "{install:?}");
}

/// Снимок пакета с архивом `.plasmoid`, который проба плана уже распаковала
/// (`ext_manager::PLASMOID_PROBE_DIR`), — id внутри архива: `org.zip.clock`.
fn plasmoid_archive_snapshot() -> Value {
    let probe = "/tmp/t/rexon_plasmoid_probe/clock.plasmoid";
    json!({
        "dirs": {
            "/ext": ["data"],
            "/ext/data": ["clock.plasmoid"],
            probe: ["contents", "metadata.json"],
            format!("{probe}/contents"): [],
        },
        "files": {
            "/ext/data/clock.plasmoid": {"size": 10, "text": null},
            format!("{probe}/metadata.json"): {"size": 1, "text": r#"{"KPlugin": {"Id": "org.zip.clock"}}"#},
        },
    })
}

/// Архив `.plasmoid`: id известен только после распаковки — его даёт проба
/// снимка, и чужой плазмоид с тем же id не отдаётся `kpackagetool -u` (тот
/// заменил бы его ещё при сборке плана). Отказ с причиной; удаление чужое
/// не трогает.
#[test]
fn plasmoid_archive_never_updates_foreign_with_same_id() {
    let dest = format!("{PLASMOIDS}/org.zip.clock");
    for tools in [&["kpackagetool6"][..], &[][..]] {
        let snap = with_file(plasmoid_archive_snapshot(), &format!("{dest}/main.qml"), "human");
        let case = Case::new(snap, tools, true, false);
        let m = manifest("zip-pack", "theme-plasmoid");
        let install = case.plan(&m, false);
        assert!(!touches(&install, &dest), "{tools:?}: {install:?}");
        assert!(!install.iter().any(|s| matches!(s, ExtStep::ExtractZip { .. })), "{tools:?}: {install:?}");
        assert!(
            matches!(install.as_slice(), [ExtStep::Refuse { text }] if text.contains("org.zip.clock") && text.contains(&dest)),
            "{tools:?}: {install:?}"
        );
        let uninstall = case.plan(&m, true);
        assert!(!touches(&uninstall, &dest), "{tools:?}: удаление: {uninstall:?}");
    }
}

/// Архив `.plasmoid` на чистом месте: инструмент KDE ставит его, и каталог
/// по id из пробы получает метку; удаление снимает плазмоид по НАСТОЯЩЕМУ
/// id (прежде — по id пакета, мимо каталога).
#[test]
fn plasmoid_archive_installed_by_tool_is_marked_and_removed_by_real_id() {
    let dest = format!("{PLASMOIDS}/org.zip.clock");
    let case = Case::new(plasmoid_archive_snapshot(), &["kpackagetool6"], true, false);
    let m = manifest("zip-pack", "theme-plasmoid");
    let install = case.plan(&m, false);
    assert!(argvs(&install).iter().any(|a| a[1] == "-i"), "{install:?}");
    assert!(install.iter().any(|s| matches!(s, ExtStep::WriteFile { path, content }
        if *path == format!("{dest}/.rexon-owner") && content == "zip-pack\n")), "{install:?}");
    let marked = with_file(plasmoid_archive_snapshot(), &format!("{dest}/.rexon-owner"), "zip-pack\n");
    let uninstall = Case::new(marked, &["kpackagetool6"], true, false).plan(&m, true);
    assert!(argvs(&uninstall).iter().any(|a| a == &["/usr/bin/kpackagetool6", "-r", "org.zip.clock"]), "{uninstall:?}");
    assert!(uninstall.iter().any(|s| matches!(s, ExtStep::RemoveTree { path, .. } if *path == dest)), "{uninstall:?}");
}

const SCHEMES: &str = "/home/t/.local/share/color-schemes";

/// Пакет цветовых схем `names` (`(имя файла, текст)`).
fn colors_snapshot(names: &[(&str, &str)]) -> Value {
    let mut snap = json!({"dirs": {"/ext": ["data"], "/ext/data": []}, "files": {}});
    for (n, t) in names {
        snap = with_file(snap, &format!("/ext/data/{n}"), t);
    }
    snap
}

/// Одиночный файл (`.colors`, действие Nemo): на месте чужой файл с другим
/// содержимым — не перезаписывается и не снимается удалением; причина
/// названа; единственный файл пакета — отказ целиком.
#[test]
fn single_file_never_overwrites_foreign() {
    english();
    for (t, base, name) in [
        ("theme-kde-colors", SCHEMES, "Nord.colors"),
        ("nemo-action", "/home/t/.local/share/nemo/actions", "open.nemo_action"),
    ] {
        let dest = format!("{base}/{name}");
        let snap = with_file(colors_snapshot(&[(name, "пакет\n")]), &dest, "человек\n");
        let case = Case::new(snap, &[], true, false);
        let m = manifest("pack", t);
        let install = case.plan(&m, false);
        assert!(!touches(&install, &dest), "{t}: установка перезаписала чужой файл: {install:?}");
        assert!(
            matches!(install.as_slice(), [ExtStep::Refuse { text }] if text.contains("skipped") && text.contains(&dest)),
            "{t}: {install:?}"
        );
        let uninstall = case.plan(&m, true);
        assert!(!touches(&uninstall, &dest), "{t}: удаление сняло чужой файл: {uninstall:?}");
    }
}

/// Тот же файл байт в байт — свой (заменяется, снимается); файл из общего
/// журнала этого пакета (`owned_files`) — свой при любом содержимом; ссылка
/// на месте — чужая.
#[test]
fn single_file_same_bytes_or_journal_is_own() {
    let dest = format!("{SCHEMES}/Nord.colors");
    let m = manifest("pack", "theme-kde-colors");
    let same = Case::new(with_file(colors_snapshot(&[("Nord.colors", "одно\n")]), &dest, "одно\n"), &[], true, false);
    assert_eq!(copies(&same.plan(&m, false)), vec![("/ext/data/Nord.colors".to_string(), dest.clone())]);
    assert!(same.plan(&m, true).iter().any(|s| matches!(s, ExtStep::RemoveFile { path } if *path == dest)));

    let mut journal = with_file(colors_snapshot(&[("Nord.colors", "новое\n")]), &dest, "прежнее своё\n");
    journal["owned_files"] = json!([dest]);
    let own = Case::new(journal, &[], true, false);
    let install = own.plan(&m, false);
    assert_eq!(copies(&install), vec![("/ext/data/Nord.colors".to_string(), dest.clone())], "{install:?}");
    assert!(!says_skipped(&install, &dest), "{install:?}");
    assert!(own.plan(&m, true).iter().any(|s| matches!(s, ExtStep::RemoveFile { path } if *path == dest)));

    let mut link = with_file(colors_snapshot(&[("Nord.colors", "одно\n")]), &dest, "одно\n");
    link["links"] = json!([dest]);
    let linked = Case::new(link, &[], true, false);
    assert!(!touches(&linked.plan(&m, false), &dest), "файл записан сквозь ссылку");
}

/// Две схемы, одна занята чужой: вторая ставится, про первую — заметка
/// (частичный пропуск — не отказ).
#[test]
fn single_file_partial_skip_installs_the_rest() {
    let taken = format!("{SCHEMES}/Nord.colors");
    let free = format!("{SCHEMES}/Gold.colors");
    let snap = with_file(colors_snapshot(&[("Nord.colors", "пакет\n"), ("Gold.colors", "пакет\n")]), &taken, "человек\n");
    let install = Case::new(snap, &[], true, false).plan(&manifest("pack", "theme-kde-colors"), false);
    assert_eq!(copies(&install), vec![("/ext/data/Gold.colors".to_string(), free)], "{install:?}");
    assert!(install.iter().any(|s| matches!(s, ExtStep::Notice { text } if text.contains(&taken))), "{install:?}");
    assert!(!install.iter().any(|s| matches!(s, ExtStep::Refuse { .. })), "{install:?}");
}

/// Комплект: шрифт в общем каталоге шрифтов и тема части комплекта под
/// своим именем — по тем же правилам (чужое цело, причина названа).
#[test]
fn bundle_parts_never_overwrite_foreign() {
    english();
    let font = "/home/t/.local/share/fonts/Sans.ttf";
    let gtk = "/home/t/.themes/BDark";
    let mut snap = json!({
        "dirs": {"/ext": ["data"], "/ext/data": ["fonts", "gtk"], "/ext/data/fonts": [], "/ext/data/gtk": ["BDark"], "/ext/data/gtk/BDark": []},
        "files": {},
    });
    snap = with_file(snap, "/ext/data/fonts/Sans.ttf", "пакет");
    snap = with_file(snap, font, "человек");
    snap = with_file(snap, &format!("{gtk}/gtk.css"), "человек");
    let mut m = manifest("bun", "theme-bundle");
    m.insert("bundle".into(), json!({"font-pack": "data/fonts", "theme-gtk": "data/gtk"}));
    let case = Case::new(snap, &[], true, false);
    let install = case.plan(&m, false);
    for dest in [font, gtk] {
        assert!(!touches(&install, dest), "{dest}: {install:?}");
        assert!(says_skipped(&install, dest), "{dest}: {install:?}");
    }
    let uninstall = case.plan(&m, true);
    for dest in [font, gtk] {
        assert!(!touches(&uninstall, dest), "удаление {dest}: {uninstall:?}");
    }
}
