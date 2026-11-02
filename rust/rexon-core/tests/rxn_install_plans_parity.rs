//! Страховка перед слиянием установщиков: эталон планов `.rxn` v3/v4.
//!
//! ЗАЧЕМ. Установщик пакетов (`ext_manager::safe_install_plan` /
//! `safe_uninstall_plan` и кнопки разделов `de_enable_steps` /
//! `de_disable_steps`) сливается с установщиком «Оформления». Сверки с Python
//! (`ext_manager_parity.rs`) держат по одному столу на тип; здесь — КАЖДЫЙ
//! допустимый тип в КАЖДОМ из восьми окружений, чтобы слияние не поменяло
//! ни одного шага молча.
//!
//! Что снимается для пары «тип × окружение»:
//!   * `install`   — план установки чистого пакета на чистый профиль;
//!   * `uninstall` — план удаления, когда всё, что положила установка, на месте;
//!   * `reinstall` — план повторной установки поверх уже установленного;
//!   * `enable` / `disable` — шаги кнопки раздела для каждого объекта пакета.
//!
//! Эталон — `golden/rxn_install_plans.json`. Расхождение = поведение
//! изменилось. Намеренную перемену записать:
//! `REXON_UPDATE_GOLDEN=1 cargo test -p rexon-core --test rxn_install_plans_parity`
//! и просмотреть разницу эталона в `git diff` построчно.
//!
//! Ничего не исполняется: снимок файловой системы — `FsSnap`, программы —
//! подставные пути, пробы (`run_ok`) отвечают «нет».

use std::collections::{BTreeMap, HashMap};

use rexon_core::constants;
use rexon_core::ext_manager::{
    de_disable_steps, de_enable_steps, de_object_names, safe_install_plan, safe_uninstall_plan,
    ExtEnv, ExtInfo, ExtStep, FsSnap, TmpAlloc,
};
use serde_json::{json, Map, Value};

const HOME: &str = "/home/t";

/// Окружения: значение `XDG_CURRENT_DESKTOP` и вид сеанса — как на живых
/// машинах этих столов.
const DESKTOPS: &[(&str, bool)] = &[
    ("KDE", true),
    ("GNOME", true),
    ("XFCE", false),
    ("X-Cinnamon", false),
    ("MATE", false),
    ("LXQt", false),
    ("sway", true),
    ("", false),
];

/// Программы «богатой» машины: всё, чем планировщик умеет пользоваться.
const TOOLS: &[&str] = &[
    "kwriteconfig6", "kreadconfig6", "plasma-apply-colorscheme", "plasma-apply-cursortheme",
    "plasma-apply-lookandfeel", "plasma-apply-desktoptheme", "kvantummanager", "gsettings",
    "xfconf-query", "qdbus6", "kpackagetool6", "dbus-send", "pkexec", "conky", "cinnamon",
    "gnome-extensions", "mkinitcpio", "fc-cache", "gtk-update-icon-cache", "plymouth-set-default-theme",
];

/// Пакет «похож на что угодно»: подпапка с курсорами, look-and-feel,
/// метаданными, рамками и оболочками, файлы цветов, шрифта, действия Nemo,
/// конфиг Conky, картинка обоев и `.plymouth`.
fn package_snapshot() -> Value {
    json!({
        "dirs": {
            "/ext": ["data", "manifest.json"],
            "/ext/data": ["MyTheme", "MyTheme.colors", "my.nemo_action", "Font.ttf", "bg.png"],
            "/ext/data/MyTheme": [
                "cursors", "contents", "index.theme", "metadata.json", "xfwm4",
                "metacity-1", "gnome-shell", "cinnamon", "lxqt-panel.qss", "conky.conf",
                "gtk-3.0", "MyTheme.plymouth", "Main.qml",
            ],
            "/ext/data/MyTheme/cursors": [],
            "/ext/data/MyTheme/contents": [],
            "/ext/data/MyTheme/xfwm4": [],
            "/ext/data/MyTheme/metacity-1": [],
            "/ext/data/MyTheme/gnome-shell": [],
            "/ext/data/MyTheme/cinnamon": [],
            "/ext/data/MyTheme/gtk-3.0": [],
        },
        "files": {
            "/ext/manifest.json": {"size": 10, "text": "{}"},
            "/ext/data/MyTheme.colors": {"size": 10, "text": "[General]\n"},
            "/ext/data/my.nemo_action": {"size": 10, "text": "[Nemo Action]\n"},
            "/ext/data/Font.ttf": {"size": 10, "text": null},
            "/ext/data/bg.png": {"size": 10, "text": null},
            "/ext/data/MyTheme/index.theme": {"size": 10, "text": "[Icon Theme]\nName=MyTheme\n"},
            "/ext/data/MyTheme/metadata.json": {"size": 10, "text": "{\"uuid\": \"my@rexon\", \"KPlugin\": {\"Id\": \"org.rexon.mytheme\"}}"},
            "/ext/data/MyTheme/lxqt-panel.qss": {"size": 10, "text": ""},
            "/ext/data/MyTheme/conky.conf": {"size": 10, "text": "conky.text = [[x]]"},
            "/ext/data/MyTheme/MyTheme.plymouth": {"size": 10, "text": "[Plymouth Theme]\nName=MyTheme\nModuleName=script\n"},
            "/ext/data/MyTheme/Main.qml": {"size": 10, "text": "import QtQuick\n"},
        },
    })
}

/// Все типы, которые ставит планировщик (скриптовые — `install.sh` пакета,
/// плана у них нет).
fn plan_types() -> Vec<&'static str> {
    constants::valid_types().into_iter().filter(|t| !constants::is_scripted(t)).collect()
}

fn tools() -> HashMap<String, String> {
    TOOLS.iter().map(|t| ((*t).to_string(), format!("/usr/bin/{t}"))).collect()
}

fn manifest(t: &str) -> Map<String, Value> {
    json!({"id": "my-theme", "name": "My Theme", "type": t, "version": "1.0"})
        .as_object()
        .unwrap()
        .clone()
}

/// Прогнать `f` с окружением плана для стола `xdg`.
fn with_env<R>(fs: &FsSnap, xdg: &str, wayland: bool, f: impl FnOnce(&ExtEnv) -> R) -> R {
    let tools = tools();
    let tmp = TmpAlloc::new("/tmp/t");
    let which = |n: &str| tools.get(n).cloned();
    let run_ok = |_: &[String]| false;
    let env = ExtEnv {
        home: HOME,
        fs,
        which: &which,
        run_ok: &run_ok,
        xdg_current_desktop: xdg,
        xdg_session_type: if wayland { "wayland" } else { "x11" },
        wayland_display: wayland,
        stage_helper_available: false,
        system_helper_path: "/usr/lib/rexon/rexon-system-helper",
        system_helper_exists: true,
        helper_family: "arch",
        tmp: &tmp,
    };
    f(&env)
}

fn lines(steps: &[ExtStep]) -> Vec<Value> {
    steps.iter().map(|s| Value::String(serde_json::to_string(s).unwrap())).collect()
}

/// Снимок «после установки»: пакет плюс всё, что положил план.
/// Копия каталога получает содержимое источника (на один уровень — этого
/// планировщику удаления хватает), копия файла — файл, ссылка — ссылку.
fn installed_snapshot(pkg: &Value, steps: &[ExtStep]) -> Value {
    let mut snap = pkg.clone();
    let add_child = |snap: &mut Value, path: &str| {
        if let Some((parent, name)) = path.rsplit_once('/') {
            let parent = if parent.is_empty() { "/" } else { parent };
            let list = snap["dirs"][parent].as_array().cloned().unwrap_or_default();
            if !list.iter().any(|n| n == name) {
                let mut list = list;
                list.push(json!(name));
                snap["dirs"][parent] = Value::Array(list);
            }
        }
    };
    let add_dir = |snap: &mut Value, path: &str, children: Value| {
        snap["dirs"][path] = children;
    };
    // `CopyFile` в каталог — файл кладётся под своим именем.
    let file_dest = |snap: &Value, src: &str, dest: &str| -> String {
        if snap["dirs"].get(dest).is_some() {
            format!("{dest}/{}", src.rsplit('/').next().unwrap_or(src))
        } else {
            dest.to_string()
        }
    };
    for step in steps {
        match step {
            ExtStep::MakeDirs { path } => {
                if snap["dirs"].get(path).is_none() {
                    add_dir(&mut snap, path, json!([]));
                    add_child(&mut snap, path);
                }
            }
            ExtStep::CopyTree { src, dest } => {
                let children = pkg["dirs"].get(src).cloned().unwrap_or(json!([]));
                add_dir(&mut snap, dest, children.clone());
                add_child(&mut snap, dest);
                for c in children.as_array().unwrap() {
                    let name = c.as_str().unwrap();
                    let (s, d) = (format!("{src}/{name}"), format!("{dest}/{name}"));
                    if let Some(f) = pkg["files"].get(&s).cloned() {
                        snap["files"][d] = f;
                    } else if let Some(sub) = pkg["dirs"].get(&s).cloned() {
                        snap["dirs"][d] = sub;
                    }
                }
            }
            ExtStep::CopyFile { src, dest } => {
                let d = file_dest(&snap, src, dest);
                let f = pkg["files"].get(src).cloned().unwrap_or(json!({"size": 1, "text": null}));
                snap["files"][d.clone()] = f;
                add_child(&mut snap, &d);
            }
            ExtStep::WriteFile { path, content } => {
                snap["files"][path.clone()] = json!({"size": content.len(), "text": content});
                add_child(&mut snap, path);
            }
            ExtStep::Symlink { link, .. } => {
                let mut links = snap["links"].as_array().cloned().unwrap_or_default();
                links.push(json!(link));
                snap["links"] = Value::Array(links);
                snap["files"][link.clone()] = json!({"size": 0, "text": null});
                add_child(&mut snap, link);
            }
            _ => {}
        }
    }
    snap
}

/// Куда установка положила что-то в профиле (копии, записи, ссылки).
/// Копия файла в каталог (в том числе созданный тем же планом) — под своим
/// именем.
fn home_dests(steps: &[ExtStep], snap_before: &Value) -> Vec<String> {
    let mut made: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for s in steps {
        let p = match s {
            ExtStep::MakeDirs { path } => {
                made.push(path.clone());
                None
            }
            ExtStep::CopyTree { dest, .. } => {
                made.push(dest.clone());
                Some(dest.clone())
            }
            ExtStep::CopyFile { src, dest } => {
                Some(if snap_before["dirs"].get(dest).is_some() || made.contains(dest) {
                    format!("{dest}/{}", src.rsplit('/').next().unwrap_or(src))
                } else {
                    dest.clone()
                })
            }
            ExtStep::WriteFile { path, .. } => Some(path.clone()),
            ExtStep::Symlink { link, .. } => Some(link.clone()),
            _ => None,
        };
        if let Some(p) = p.filter(|p| p.starts_with(&format!("{HOME}/"))) {
            out.push(p);
        }
    }
    out
}

/// Пути, которые план удаляет (`RemoveTree`/`RemoveFile`).
fn removed_by(steps: &[ExtStep]) -> Vec<String> {
    steps
        .iter()
        .filter_map(|s| match s {
            ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => Some(path.clone()),
            _ => None,
        })
        .collect()
}

fn covered(path: &str, roots: &[String]) -> bool {
    roots.iter().any(|r| path == r || path.starts_with(&format!("{r}/")))
}

/// Снять все планы.
fn collect() -> BTreeMap<String, Value> {
    let pkg = package_snapshot();
    let clean = FsSnap::from_json(&pkg);
    let mut out = BTreeMap::new();
    for &(xdg, wayland) in DESKTOPS {
        let de = if xdg.is_empty() { "none" } else { xdg };
        for t in plan_types() {
            let m = manifest(t);
            let ext = ExtInfo { dir: "/ext", manifest: &m };
            let install = with_env(&clean, xdg, wayland, |env| safe_install_plan(env, &ext));
            let after = installed_snapshot(&pkg, &install);
            let after_fs = FsSnap::from_json(&after);
            let uninstall = with_env(&after_fs, xdg, wayland, |env| safe_uninstall_plan(env, &ext));
            let reinstall = with_env(&after_fs, xdg, wayland, |env| safe_install_plan(env, &ext));
            let names = with_env(&clean, xdg, wayland, |env| de_object_names(env, &ext));
            let mut enable = Map::new();
            let mut disable = Map::new();
            for n in &names {
                let on = with_env(&after_fs, xdg, wayland, |env| de_enable_steps(env, t, n));
                let off = with_env(&after_fs, xdg, wayland, |env| de_disable_steps(env, t, n));
                if !on.is_empty() {
                    enable.insert(n.clone(), Value::Array(lines(&on)));
                }
                if !off.is_empty() {
                    disable.insert(n.clone(), Value::Array(lines(&off)));
                }
            }
            out.insert(
                format!("{de} / {t}"),
                json!({
                    "install": lines(&install),
                    "uninstall": lines(&uninstall),
                    "reinstall": lines(&reinstall),
                    "enable": enable,
                    "disable": disable,
                }),
            );
        }
    }
    out
}

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/rxn_install_plans.json");

/// Все планы совпадают с эталоном.
/// Язык Rexon один на процесс, а тесты файла идут параллельно: эталон снят
/// на русском, сторож чужих тем ищет английское «skipped». Оба закрепляют язык
/// через `i18n_pin`: русский там собирается из `po/ru.po` дерева, поэтому
/// исход не зависит ни от локали машины, ни от каталога в /usr/share/locale.
mod i18n_pin;

#[test]
fn rxn_plans_match_golden() {
    let _language = i18n_pin::ru();
    let now = collect();
    let text = serde_json::to_string_pretty(&now).unwrap() + "\n";
    if std::env::var("REXON_UPDATE_GOLDEN").is_ok() {
        std::fs::write(GOLDEN, &text).unwrap();
        return;
    }
    let want: BTreeMap<String, Value> = serde_json::from_str(
        &std::fs::read_to_string(GOLDEN).expect("эталона нет — снять: REXON_UPDATE_GOLDEN=1"),
    )
    .unwrap();
    // Состав пар — первым: новый тип или стол без эталона виден по имени.
    let now_keys: Vec<&String> = now.keys().collect();
    let want_keys: Vec<&String> = want.keys().collect();
    assert_eq!(now_keys, want_keys, "состав «тип × стол» изменился");
    let mut diverged = Vec::new();
    for (k, v) in &now {
        for part in ["install", "uninstall", "reinstall", "enable", "disable"] {
            if v[part] != want[k][part] {
                diverged.push(format!(
                    "{k} [{part}]\n  было:  {}\n  стало: {}",
                    want[k][part], v[part]
                ));
            }
        }
    }
    assert!(
        diverged.is_empty(),
        "планы .rxn изменились ({} мест) — если намеренно, обнови эталон \
         (REXON_UPDATE_GOLDEN=1):\n{}",
        diverged.len(),
        diverged.join("\n")
    );
}

/// Эталон не пуст там, где тип живёт: самые ходовые типы на своих столах
/// что-то кладут. Сторож от «эталон сняли с пустых планов».
#[test]
fn common_types_install_something_on_their_desktops() {
    let plans = collect();
    for (de, t) in [
        ("KDE", "theme-kde-global"), ("KDE", "theme-kde-plasma"), ("KDE", "theme-kde-colors"),
        ("KDE", "theme-window-decoration"), ("KDE", "theme-kvantum"), ("KDE", "theme-plasmoid"),
        ("KDE", "theme-icons"), ("KDE", "theme-cursors"), ("GNOME", "theme-gtk"),
        ("GNOME", "theme-icons"), ("GNOME", "theme-cursors"), ("GNOME", "gnome-shell-extension"),
        ("XFCE", "theme-gtk"), ("XFCE", "theme-xfwm4"), ("MATE", "theme-marco"),
        ("MATE", "theme-gtk"), ("X-Cinnamon", "theme-cinnamon"), ("X-Cinnamon", "cinnamon-applet"),
        ("X-Cinnamon", "theme-icons"),
    ] {
        let p = &plans[&format!("{de} / {t}")];
        assert!(!p["install"].as_array().unwrap().is_empty(), "{t} на {de}: установка пустая");
        assert!(
            !p["uninstall"].as_array().unwrap().is_empty(),
            "{t} на {de}: удалять нечего, хотя установка клала файлы"
        );
    }
}

/// Удаление снимает всё, что установка положила в профиль: каждый путь
/// назначения (или его предок) удаляется, либо пакет снимает штатная
/// утилита (`kpackagetool -r`).
///
/// Исключения — известные и записанные: см. [`uninstall_leaves_known_leftovers`].
#[test]
fn uninstall_removes_what_install_put() {
    let pkg = package_snapshot();
    let clean = FsSnap::from_json(&pkg);
    let mut leftovers = Vec::new();
    for &(xdg, wayland) in DESKTOPS {
        for t in plan_types() {
            let m = manifest(t);
            let ext = ExtInfo { dir: "/ext", manifest: &m };
            let install = with_env(&clean, xdg, wayland, |env| safe_install_plan(env, &ext));
            let after = installed_snapshot(&pkg, &install);
            let after_fs = FsSnap::from_json(&after);
            let uninstall = with_env(&after_fs, xdg, wayland, |env| safe_uninstall_plan(env, &ext));
            let removed = removed_by(&uninstall);
            let by_tool = uninstall.iter().any(|s| {
                matches!(s, ExtStep::Run { argv, .. }
                    if argv[0].contains("kpackagetool") && argv.get(1).map(String::as_str) == Some("-r"))
            });
            for d in home_dests(&install, &pkg) {
                let gone = covered(&d, &removed);
                if !gone && !by_tool {
                    leftovers.push(format!("{} / {t}: {d}", if xdg.is_empty() { "none" } else { xdg }));
                }
            }
        }
    }
    let known = known_leftovers();
    let unknown: Vec<&String> = leftovers.iter().filter(|l| !known.iter().any(|k| l.contains(k))).collect();
    assert!(unknown.is_empty(), "удаление оставляет в профиле положенное установкой:\n{unknown:#?}");
}

/// Известные хвосты удаления. Д1–Д3 (найдены этим набором 25.10.2026)
/// починены меткой владельца копии (`ext_manager::OWNER_MARK`). Новый
/// найденный хвост — строкой сюда и `#[ignore]`-тестом ниже; починили —
/// убрать строку отсюда и `#[ignore]` с теста.
fn known_leftovers() -> Vec<&'static str> {
    Vec::new()
}

/// Повторная установка поверх установленного не теряет файлов: всё, что
/// было на месте после первой установки, после повтора тоже на месте —
/// либо не тронуто, либо снято и положено заново.
#[test]
fn reinstall_keeps_every_installed_file() {
    let pkg = package_snapshot();
    let clean = FsSnap::from_json(&pkg);
    let mut lost = Vec::new();
    for &(xdg, wayland) in DESKTOPS {
        for t in plan_types() {
            let m = manifest(t);
            let ext = ExtInfo { dir: "/ext", manifest: &m };
            let install = with_env(&clean, xdg, wayland, |env| safe_install_plan(env, &ext));
            let after = installed_snapshot(&pkg, &install);
            let after_fs = FsSnap::from_json(&after);
            let again = with_env(&after_fs, xdg, wayland, |env| safe_install_plan(env, &ext));
            let removed = removed_by(&again);
            let put = home_dests(&again, &after);
            for d in home_dests(&install, &pkg) {
                if covered(&d, &removed) && !put.iter().any(|p| d == *p || d.starts_with(&format!("{p}/"))) {
                    lost.push(format!("{} / {t}: {d}", if xdg.is_empty() { "none" } else { xdg }));
                }
            }
        }
    }
    assert!(lost.is_empty(), "повторная установка снимает и не кладёт обратно:\n{lost:#?}");
}

// ============================================================================
// Найденные дефекты (тест — как должно быть; Д1–Д3 починены 25.10.2026)
// ============================================================================

/// План удаления после установки на столе `xdg`.
fn install_then_uninstall(t: &str, xdg: &str, wayland: bool) -> (Vec<ExtStep>, Vec<ExtStep>, Vec<ExtStep>) {
    let pkg = package_snapshot();
    let clean = FsSnap::from_json(&pkg);
    let m = manifest(t);
    let ext = ExtInfo { dir: "/ext", manifest: &m };
    let install = with_env(&clean, xdg, wayland, |env| safe_install_plan(env, &ext));
    let after_fs = FsSnap::from_json(&installed_snapshot(&pkg, &install));
    let uninstall = with_env(&after_fs, xdg, wayland, |env| safe_uninstall_plan(env, &ext));
    let again = with_env(&after_fs, xdg, wayland, |env| safe_install_plan(env, &ext));
    (install, uninstall, again)
}

/// Д1. Рамки окон (`theme-window-decoration`) на GNOME, Cinnamon, Budgie,
/// Pantheon и XFCE установка копирует ещё и в `~/.themes/<тема>`
/// (`install_window_decoration_plan`), а удаление снимает только каталог
/// Aurorae — копия в `~/.themes` оставалась навсегда и была видна в списках
/// тем. Так же было в Python (эталон `twd_gnome`). Починено: копия кладётся
/// с меткой владельца и снимается по ней.
#[test]
fn defect_decoration_copy_in_themes_is_removed() {
    for (xdg, wayland) in [("GNOME", true), ("X-Cinnamon", false), ("XFCE", false)] {
        let (_, uninstall, _) = install_then_uninstall("theme-window-decoration", xdg, wayland);
        assert!(
            covered("/home/t/.themes/MyTheme", &removed_by(&uninstall)),
            "{xdg}: ~/.themes/MyTheme не снимается: {uninstall:?}"
        );
    }
}

/// Д1б. Там же повторная установка (обновление пакета) копию в `~/.themes`
/// не обновляет: условие `!exists(dest)` — старая версия остаётся, хотя
/// каталог Aurorae уже новый.
#[test]
fn defect_decoration_copy_in_themes_is_refreshed() {
    for (xdg, wayland) in [("GNOME", true), ("X-Cinnamon", false), ("XFCE", false)] {
        let (_, _, again) = install_then_uninstall("theme-window-decoration", xdg, wayland);
        assert!(
            again.iter().any(|s| matches!(s, ExtStep::CopyTree { dest, .. } if dest == "/home/t/.themes/MyTheme")),
            "{xdg}: повтор не кладёт ~/.themes/MyTheme заново: {again:?}"
        );
    }
}

/// Д2. Комплект (`theme-bundle`): каждая подпапка `data/` копируется в
/// `~/.local/share/rexon/bundles/<подпапка>`, а удаление снимает только
/// части комплекта по его карте — копии в `bundles/` не снимаются никогда.
/// Так же было в Python (эталоны `bundle_std` / `bundle`).
#[test]
fn defect_bundle_copy_stays() {
    let (_, uninstall, _) = install_then_uninstall("theme-bundle", "KDE", true);
    assert!(
        covered("/home/t/.local/share/rexon/bundles/MyTheme", &removed_by(&uninstall)),
        "bundles/MyTheme не снимается: {uninstall:?}"
    );
}

/// Д3. Conky (`theme-conky`): кроме `~/.config/conky/<id>` установка кладёт
/// каждую подпапку `data/` ещё и в `~/.config/conky/<подпапка>` (у Python
/// так ложились `fonts/`), а удаление снимает только `<id>` и автозапуск.
/// Общие имена (`fonts`) к тому же могут принадлежать другому виджету —
/// при починке снимать только то, что положил этот пакет.
#[test]
fn defect_conky_subfolder_copy_stays() {
    let (_, uninstall, _) = install_then_uninstall("theme-conky", "KDE", true);
    assert!(
        covered("/home/t/.config/conky/MyTheme", &removed_by(&uninstall)),
        "conky/MyTheme не снимается: {uninstall:?}"
    );
}

/// Снимок «в профиле уже лежит `path`» (каталог с одним файлом `inner`,
/// у которого текст `text`) поверх пакета.
fn with_foreign_dir(path: &str, inner: &str, text: &str) -> Value {
    let mut pkg = package_snapshot();
    let (parent, name) = path.rsplit_once('/').unwrap();
    let mut list = pkg["dirs"][parent].as_array().cloned().unwrap_or_default();
    list.push(json!(name));
    pkg["dirs"][parent] = Value::Array(list);
    pkg["dirs"][path] = json!([inner]);
    pkg["files"][format!("{path}/{inner}")] = json!({"size": text.len(), "text": text});
    pkg
}

/// Чужое под тем же именем не трогается: тема человека в `~/.themes`
/// (без метки) и каталог другого пакета (метка с ЕГО id) — ни установка
/// рамок, ни их удаление, ни повтор установки их не снимают и не
/// перезаписывают (Д1).
#[test]
fn decoration_never_touches_foreign_theme_in_themes() {
    let dest = "/home/t/.themes/MyTheme";
    for (inner, text) in [("gtk-3.0", "human"), (".rexon-owner", "other-pack\n")] {
        let pkg = with_foreign_dir(dest, inner, text);
        let before = FsSnap::from_json(&pkg);
        let m = manifest("theme-window-decoration");
        let ext = ExtInfo { dir: "/ext", manifest: &m };
        for (xdg, wayland) in [("GNOME", true), ("X-Cinnamon", false), ("XFCE", false)] {
            let install = with_env(&before, xdg, wayland, |env| safe_install_plan(env, &ext));
            let touches = |steps: &[ExtStep]| {
                steps.iter().any(|s| match s {
                    ExtStep::CopyTree { dest: d, .. } => d == dest,
                    ExtStep::WriteFile { path, .. } => path.starts_with(dest),
                    ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => covered(dest, &[path.clone()]),
                    _ => false,
                })
            };
            assert!(!touches(&install), "{xdg} ({inner}): установка тронула чужую тему: {install:?}");
            let after = FsSnap::from_json(&installed_snapshot(&pkg, &install));
            let uninstall = with_env(&after, xdg, wayland, |env| safe_uninstall_plan(env, &ext));
            assert!(!touches(&uninstall), "{xdg} ({inner}): удаление сняло чужую тему: {uninstall:?}");
            let again = with_env(&after, xdg, wayland, |env| safe_install_plan(env, &ext));
            assert!(!touches(&again), "{xdg} ({inner}): повтор тронул чужую тему: {again:?}");
        }
    }
}

/// Conky: подпапка `~/.config/conky/<подпапка>` другого виджета или
/// человека (общая `fonts` и т. п.) не перезаписывается установкой и не
/// снимается удалением — снимается только своя, с меткой этого пакета (Д3).
/// То же у копий комплекта в `~/.local/share/rexon/bundles` (Д2).
#[test]
fn conky_and_bundle_never_touch_foreign_subfolder() {
    for (t, dest) in [
        ("theme-conky", "/home/t/.config/conky/MyTheme"),
        ("theme-bundle", "/home/t/.local/share/rexon/bundles/MyTheme"),
    ] {
        for (inner, text) in [("conky.conf", "human"), (".rexon-owner", "other-pack\n")] {
            let pkg = with_foreign_dir(dest, inner, text);
            let before = FsSnap::from_json(&pkg);
            let m = manifest(t);
            let ext = ExtInfo { dir: "/ext", manifest: &m };
            let install = with_env(&before, "KDE", true, |env| safe_install_plan(env, &ext));
            let touches = |steps: &[ExtStep]| {
                steps.iter().any(|s| match s {
                    ExtStep::CopyTree { dest: d, .. } => d == dest,
                    ExtStep::WriteFile { path, .. } => path.starts_with(dest),
                    ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => covered(dest, &[path.clone()]),
                    _ => false,
                })
            };
            assert!(!touches(&install), "{t} ({inner}): установка тронула чужую папку: {install:?}");
            let after = FsSnap::from_json(&installed_snapshot(&pkg, &install));
            let uninstall = with_env(&after, "KDE", true, |env| safe_uninstall_plan(env, &ext));
            assert!(!touches(&uninstall), "{t} ({inner}): удаление сняло чужую папку: {uninstall:?}");
            // Свой каталог виджета при этом снимается как прежде.
            if t == "theme-conky" {
                assert!(
                    covered("/home/t/.config/conky/my-theme", &removed_by(&uninstall)),
                    "conky/<id> не снимается: {uninstall:?}"
                );
            }
        }
    }
}

/// ОСНОВНАЯ копия темы (первый каталог, который кладёт установка, — у
/// общего пути и у матрицы окружений) поверх каталога с тем же именем,
/// положенного не этим пакетом: тема человека (без метки — так же выглядит
/// копия прежнего Rexon) или другого пакета (метка с ЕГО id). Ни установка,
/// ни удаление, ни повтор её не трогают (ни копией, ни инструментом KDE), а
/// установка говорит причину заметкой. Дефект 25.10.2026: план снимал
/// каталог и клал свой. Conky и комплект — свои сторожа выше; плазмоид по
/// `KPlugin.Id` и одиночные файлы — `de_customization.rs`
/// (`plasmoid_*`, `single_file_*`, `bundle_parts_never_overwrite_foreign`).
#[test]
fn main_copy_never_touches_foreign_theme() {
    let _language = i18n_pin::en();
    let pkg = package_snapshot();
    let clean = FsSnap::from_json(&pkg);
    let mut checked = 0;
    for &(xdg, wayland) in DESKTOPS {
        for t in plan_types() {
            if matches!(t, "theme-conky" | "theme-bundle" | "theme-plasmoid") {
                continue;
            }
            let m = manifest(t);
            let ext = ExtInfo { dir: "/ext", manifest: &m };
            let install = with_env(&clean, xdg, wayland, |env| safe_install_plan(env, &ext));
            let Some(dest) = install.iter().find_map(|s| match s {
                ExtStep::CopyTree { dest, .. } if dest.starts_with(&format!("{HOME}/")) => Some(dest.clone()),
                _ => None,
            }) else {
                continue;
            };
            for (inner, text) in [("index.theme", "human"), (".rexon-owner", "other-pack\n")] {
                let fpkg = with_foreign_dir(&dest, inner, text);
                let before = FsSnap::from_json(&fpkg);
                let touches = |steps: &[ExtStep]| {
                    steps.iter().any(|s| match s {
                        ExtStep::CopyTree { dest: d, .. } => d == &dest,
                        ExtStep::WriteFile { path, .. } => path.starts_with(&dest),
                        ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => covered(&dest, &[path.clone()]),
                        ExtStep::Run { argv, .. } => argv.first().is_some_and(|a| a.ends_with("kpackagetool6")),
                        _ => false,
                    })
                };
                let key = format!("{t} на «{xdg}» ({inner})");
                let got = with_env(&before, xdg, wayland, |env| safe_install_plan(env, &ext));
                assert!(!touches(&got), "{key}: установка тронула чужую тему {dest}: {got:?}");
                // Причина — заметкой (часть пакета пропущена) либо отказом
                // целиком, если пропущено всё основное.
                assert!(
                    got.iter().any(|s| matches!(s, ExtStep::Notice { text } | ExtStep::Refuse { text }
                        if text.contains("skipped") && text.contains(&dest))),
                    "{key}: установка не сказала, почему тема пропущена: {got:?}"
                );
                let after = FsSnap::from_json(&installed_snapshot(&fpkg, &got));
                let uninstall = with_env(&after, xdg, wayland, |env| safe_uninstall_plan(env, &ext));
                assert!(!touches(&uninstall), "{key}: удаление сняло чужую тему {dest}: {uninstall:?}");
                let again = with_env(&after, xdg, wayland, |env| safe_install_plan(env, &ext));
                assert!(!touches(&again), "{key}: повтор тронул чужую тему {dest}: {again:?}");
                checked += 1;
            }
        }
    }
    assert!(checked > 40, "проверено слишком мало пар: {checked}");
}

/// Пакет без `data/` — план пуст на любом столе (установка ничего не трогает).
#[test]
fn package_without_data_plans_nothing() {
    let fs = FsSnap::from_json(&json!({"dirs": {"/ext": ["manifest.json"]},
        "files": {"/ext/manifest.json": {"size": 2, "text": "{}"}}}));
    for &(xdg, wayland) in DESKTOPS {
        for t in plan_types() {
            let m = manifest(t);
            let ext = ExtInfo { dir: "/ext", manifest: &m };
            let steps = with_env(&fs, xdg, wayland, |env| safe_install_plan(env, &ext));
            let writes: Vec<&ExtStep> = steps
                .iter()
                .filter(|s| !matches!(s, ExtStep::MakeDirs { .. } | ExtStep::Notice { .. }))
                .collect();
            assert!(writes.is_empty(), "{t} на «{xdg}»: без data/ план что-то делает: {writes:?}");
        }
    }
}
