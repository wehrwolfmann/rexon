//! Образы «Как в дистрибутиве» (`data/distro-looks/<id>/`) — наборы `.rxn`
//! v5 как данные: каждый проходит модель `rxn_manifest`, на Plasma даёт
//! родные варианты, на XFCE и GNOME — то, что эти столы понимают, а
//! уроки 25.10 (запрет `jupiter-hw-support`, имена AUR только с `aur/`)
//! записаны в самих манифестах.
//!
//! Вторая половина — применение и «Вернуть как было» (`distro_looks`) на
//! временном профиле: команды стола — поддельные программы из своего
//! каталога (запуск только оттуда, PATH процесса не участвует), сеть и
//! пакеты — подставные. Возврат сверяется с профилем до применения побайтно.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rexon_core::rxn_manifest::{
    self, check_publishable, choose_all, license_class, unmet_preconditions, verify_dependency,
    verify_package_change, Accuracy, Choice, DepSource, LicenseClass, Package, PackagePlan, PropValues, Session,
    SOURCES_DIR,
};
use rexon_core::system::PkgFamily;
use serde_json::Value;

const LOOKS: &[&str] = &["garuda-dr460nized", "steamos"];

fn look_dir(id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/distro-looks").join(id)
}

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn load(id: &str) -> Package {
    let meta = read_json(&look_dir(id).join("manifest.json"));
    rxn_manifest::parse(&meta).unwrap_or_else(|e| panic!("{id}:\n{}", rxn_manifest::errors_text(&e)))
}

fn settings(id: &str) -> Value {
    read_json(&look_dir(id).join("native/plasma-settings.json"))
}

fn session(consumers: &[&str], versions: &[(&str, &str)]) -> Session {
    Session {
        consumers: consumers.iter().map(|s| s.to_string()).collect(),
        versions: versions.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
    }
}

/// Потребители сеансов, как их даст матрица (столбца `consumer` в
/// `de_customization` пока нет — здесь список задан явно).
fn plasma6() -> Session {
    session(
        &["kde-lnf", "kde-colors", "plasma", "aurorae", "kvantum", "kde-layout", "kconfig", "kwin", "gtk"],
        &[("kde-lnf", "6.8.1"), ("kde-layout", "6.8.1"), ("plasma", "6.8.1")],
    )
}

fn plasma5() -> Session {
    session(
        &["kde-lnf", "kde-colors", "plasma", "aurorae", "kvantum", "kde-layout", "kconfig", "kwin", "gtk"],
        &[("kde-lnf", "5.27.11"), ("kde-layout", "5.27.11"), ("plasma", "5.27.11")],
    )
}

fn xfce() -> Session {
    session(&["xfwm4", "gtk", "xfce4-panel"], &[])
}

fn gnome() -> Session {
    session(&["gtk-ssd", "gnome-shell", "gtk"], &[])
}

fn no_gen(_: &str, _: &str) -> Option<Accuracy> {
    None
}

/// Выбор по видам: вид → потребитель родного варианта или `None` (отказ).
fn by_kind(pkg: &Package, plan: &PackagePlan) -> BTreeMap<String, Option<String>> {
    pkg.items
        .iter()
        .zip(&plan.choices)
        .map(|(item, c)| {
            let consumer = match c {
                Choice::Native { consumer, .. } => Some(consumer.clone()),
                Choice::Refuse { .. } => None,
                other => panic!("{}: неожиданный выбор {other:?}", item.kind),
            };
            (item.kind.clone(), consumer)
        })
        .collect()
}

fn expect(plan: &BTreeMap<String, Option<String>>, want: &[(&str, Option<&str>)]) {
    for (kind, consumer) in want {
        assert_eq!(plan.get(*kind).map(Option::as_deref), Some(*consumer), "вид {kind}: {plan:?}");
    }
}

/// Все пути вида `sources/<id>/…` в значении JSON.
fn source_paths(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) if s.starts_with("sources/") => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| source_paths(x, out)),
        Value::Object(o) => o.values().for_each(|x| source_paths(x, out)),
        _ => {}
    }
}

fn source_id(path: &str) -> &str {
    path.strip_prefix(SOURCES_DIR).and_then(|r| r.strip_prefix('/')).and_then(|r| r.split('/').next()).unwrap()
}

// ── общие для всех образов ──────────────────────────────────────────────

#[test]
fn every_look_parses_and_is_v5() {
    for id in LOOKS {
        let pkg = load(id);
        assert_eq!(pkg.id, *id);
        assert_eq!(pkg.source_format, rxn_manifest::V5_FORMAT_VERSION);
        assert!(rxn_manifest::validate(&pkg).is_empty());
    }
}

#[test]
fn texts_in_en_ru_de() {
    for id in LOOKS {
        let pkg = load(id);
        let desc = pkg.description.as_ref().expect("описание");
        for text in [&pkg.name, desc] {
            for lang in ["en", "ru", "de"] {
                assert!(!text.get(lang).trim().is_empty(), "{id}: нет текста {lang}");
            }
            assert_ne!(text.get("ru"), text.get("en"), "{id}: русский не переведён");
            assert_ne!(text.get("de"), text.get("en"), "{id}: немецкий не переведён");
        }
        for item in &pkg.items {
            if let Some(note) = &item.note {
                for lang in ["en", "ru", "de"] {
                    assert!(!note.get(lang).trim().is_empty(), "{id}/{}: пометка без {lang}", item.kind);
                }
            }
        }
    }
}

#[test]
fn own_files_exist_and_sources_are_declared_and_used() {
    for id in LOOKS {
        let pkg = load(id);
        let dir = look_dir(id);
        let mut refs = Vec::new();
        for item in &pkg.items {
            for path in item.native.values().chain(item.substitute.values()) {
                if path.starts_with("sources/") {
                    refs.push(path.clone());
                } else {
                    assert!(dir.join(path).is_file(), "{id}: нет своего файла {path}");
                }
            }
        }
        source_paths(&settings(id), &mut refs);
        for path in &refs {
            assert!(pkg.source(source_id(path)).is_some(), "{id}: {path} — источник не объявлен");
        }
        for src in pkg.sources.iter().filter(|s| s.id.is_some()) {
            let sid = src.id.as_deref().unwrap();
            assert!(refs.iter().any(|p| source_id(p) == sid), "{id}: источник {sid} нигде не используется");
        }
    }
}

#[test]
fn licenses_are_free_and_downloads_are_not_published() {
    for id in LOOKS {
        let pkg = load(id);
        assert_eq!(license_class(pkg.license.as_deref()), LicenseClass::Free, "{id}");
        for src in &pkg.sources {
            assert_eq!(license_class(src.license.as_deref()), LicenseClass::Free, "{id}: {}", src.what);
        }
        // В Мастерскую образ не выкладывается: он скачивает чужие файлы с
        // первоисточника. Других причин (лицензий) быть не должно.
        let errs = check_publishable(&pkg);
        let fetched: Vec<String> = pkg
            .sources
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_fetched())
            .map(|(i, _)| format!("sources[{i}]"))
            .collect();
        assert!(!fetched.is_empty(), "{id}");
        assert_eq!(errs.iter().map(|e| e.field.clone()).collect::<Vec<_>>(), fetched, "{id}");
    }
}

#[test]
fn aur_names_are_bare_and_installed_with_prefix() {
    for id in LOOKS {
        let pkg = load(id);
        for dep in &pkg.requires.packages {
            for (key, names) in &dep.names {
                for n in names.as_array().into_iter().flatten() {
                    let n = n.as_str().unwrap();
                    assert!(!n.contains('/'), "{id}/{}: имя {key}={n} — префикс ставит Rexon сам", dep.role);
                }
            }
            if dep.source == DepSource::Aur {
                assert!(!dep.required, "{id}/{}: AUR не может быть обязательным", dep.role);
                assert!(!dep.source.installs_automatically());
                let names = dep.install_names(PkgFamily::Arch).expect("имена AUR");
                assert!(!names.is_empty() && names.iter().all(|n| n.starts_with("aur/")), "{names:?}");
                assert_eq!(dep.install_names(PkgFamily::Fedora), None);
            }
            if dep.fallback.contains(&DepSource::Aur) {
                // Запасной AUR: элемент тоже необязательный, и сам он не ставится.
                assert!(!dep.required, "{id}/{}: запасной AUR не может быть обязательным", dep.role);
                let r = dep.resolve(PkgFamily::Arch, &|_| false).expect("на чистом Arch — AUR");
                assert_eq!(r.source, DepSource::Aur);
                assert!(!r.installs_automatically());
                assert!(r.install_names().iter().all(|n| n.starts_with("aur/")), "{r:?}");
            }
        }
    }
}

#[test]
fn garuda_beautyline_repo_first_then_aur() {
    let pkg = load("garuda-dr460nized");
    let dep = pkg.requires.packages.iter().find(|d| d.role == "icons-beautyline").unwrap();
    // CachyOS: есть в репозитории — оттуда и сам.
    let cachy = dep.resolve(PkgFamily::Arch, &|n| n == "beautyline").unwrap();
    assert_eq!((cachy.source, cachy.install_names()), (DepSource::Repo, vec!["beautyline".to_string()]));
    assert!(cachy.installs_automatically());
    // Чистый Arch: только AUR — с префиксом, без автоустановки.
    let arch = dep.resolve(PkgFamily::Arch, &|_| false).unwrap();
    assert_eq!(arch.install_names(), ["aur/beautyline"]);
    assert!(!arch.installs_automatically());
    // Вне Arch пакета нет.
    assert_eq!(dep.resolve(PkgFamily::Fedora, &|_| true), None);
}

#[test]
fn nothing_of_the_distro_system_is_taken() {
    let banned = [
        "garuda-common-settings",
        "garuda-fish-config",
        "garuda-dr460nized",
        "plasma-login-manager",
        "jupiter-hw-support",
        "gamescope-session-cachyos",
        "steamdeck-kde-presets",
    ];
    for id in LOOKS {
        let pkg = load(id);
        for dep in &pkg.requires.packages {
            for names in dep.names.values() {
                for n in names.as_array().into_iter().flatten() {
                    assert!(!banned.contains(&n.as_str().unwrap()), "{id}: {n} ставить нельзя");
                }
            }
        }
        let s = settings(id);
        let text = s["kconfig"].to_string();
        for bad in ["RepeatDelay", "Autolock", "SingleClick", "environment.d", "Round-Corners", "Xwayland", "fish"] {
            assert!(!text.contains(bad), "{id}: в ключах есть {bad}");
        }
        for k in s["kconfig"].as_array().unwrap() {
            let file = k["file"].as_str().unwrap();
            let group = k["group"][0].as_str().unwrap();
            assert!(!(file == "kcminputrc" && group == "Keyboard"), "{id}: клавиатура — поведение");
            assert!(!(file == "kscreenlockerrc" && group == "Daemon"), "{id}: автоблокировка — поведение");
            assert!(!file.contains('/'), "{id}: ключи пишутся только в файлы настроек KDE");
        }
    }
}

// ── Garuda ──────────────────────────────────────────────────────────────

#[test]
fn garuda_plasma_is_all_native() {
    let pkg = load("garuda-dr460nized");
    let plan = choose_all(&pkg, &plasma6(), &no_gen);
    assert!(plan.installable());
    assert!(plan.skipped().is_empty(), "{:?}", plan.skipped());
    expect(
        &by_kind(&pkg, &plan),
        &[
            ("global-look", Some("kde-lnf")),
            ("palette", Some("kde-colors")),
            ("window-decoration", Some("aurorae")),
            ("shell-theme", Some("plasma")),
            ("qt-style", Some("kvantum")),
            ("gtk-theme", Some("gtk")),
            ("icons", Some("freedesktop")),
            ("cursors", Some("freedesktop")),
            ("fonts", Some("freedesktop")),
            ("wallpaper", Some("any")),
            ("panel-layout", Some("kde-layout")),
            ("behaviour", Some("kconfig")),
        ],
    );
    // Тема GNOME Shell — не для Plasma: user-theme здесь не нужен.
    assert!(!pkg.deps_for_choice("shell-theme", "plasma").iter().any(|d| d.role == "user-theme"));
}

#[test]
fn garuda_plasma5_skips_plasma6_parts() {
    let pkg = load("garuda-dr460nized");
    let plan = choose_all(&pkg, &plasma5(), &no_gen);
    assert!(plan.installable());
    expect(
        &by_kind(&pkg, &plan),
        &[("global-look", None), ("panel-layout", None), ("shell-theme", None), ("palette", Some("kde-colors"))],
    );
}

#[test]
fn garuda_on_xfce_and_gnome() {
    let pkg = load("garuda-dr460nized");
    let plan = choose_all(&pkg, &xfce(), &no_gen);
    assert!(plan.installable(), "обязательные части есть и на XFCE");
    expect(
        &by_kind(&pkg, &plan),
        &[
            ("window-decoration", Some("xfwm4")),
            ("gtk-theme", Some("gtk")),
            ("icons", Some("freedesktop")),
            ("cursors", Some("freedesktop")),
            ("wallpaper", Some("any")),
            ("global-look", None),
            ("palette", None),
            ("shell-theme", None),
            ("qt-style", None),
            ("panel-layout", None),
            ("behaviour", None),
        ],
    );

    let plan = choose_all(&pkg, &gnome(), &no_gen);
    assert!(plan.installable());
    expect(
        &by_kind(&pkg, &plan),
        &[
            ("window-decoration", Some("gtk-ssd")),
            ("shell-theme", Some("gnome-shell")),
            ("gtk-theme", Some("gtk")),
            ("global-look", None),
            ("panel-layout", None),
        ],
    );
    let deps = pkg.deps_for_choice("shell-theme", "gnome-shell");
    let ut = deps.iter().find(|d| d.role == "user-theme").expect("user-theme для темы оболочки");
    assert_eq!(ut.source, DepSource::GnomeExtensions);
}

#[test]
fn garuda_forbids_appearing_system_parts_but_keeps_what_was_there() {
    let pkg = load("garuda-dr460nized");
    for bad in ["garuda-common-settings", "garuda-fish-config", "garuda-dr460nized", "plasma-login-manager"] {
        assert!(pkg.requires.forbid.iter().any(|f| f == bad), "{bad} — запрет на весь пакет");
    }
    let none = |_: &str| None;
    let plm = |n: &str| (n == "plasma-login-manager").then(|| n.to_string());
    // Экран входа человека уже на Plasma Login Manager — образ ставится.
    assert!(verify_package_change(&pkg, &plm, &plm).is_ok());
    // Установка его притянула — отказ и откат.
    assert!(verify_package_change(&pkg, &none, &plm).is_err());
}

#[test]
fn plasma_looks_refuse_forked_libplasma() {
    for id in LOOKS {
        let pkg = load(id);
        let stock = |n: &str| (n == "libplasma").then(|| n.to_string());
        assert!(unmet_preconditions(&pkg, &stock).is_empty(), "{id}");
        let none = |_: &str| None;
        assert!(unmet_preconditions(&pkg, &none).is_empty(), "{id}: без Plasma условие не мешает");
        let fork = |n: &str| (n == "libplasma").then(|| "aeroshell-libplasma-git".to_string());
        let unmet = unmet_preconditions(&pkg, &fork);
        assert_eq!(unmet.len(), 1, "{id}");
        assert!(unmet[0].text("en").contains("AeroShell"));
        assert!(unmet[0].text("ru").contains("форком"));
        assert!(unmet[0].text("de").contains("Fork"));
    }
}

#[test]
fn garuda_plasma_settings_edits_layout_scripts() {
    let s = settings("garuda-dr460nized");
    assert_eq!(s["schema"], "kde-settings/1");
    let edits = s["edits"].to_string();
    assert!(edits.contains("RepeatDelay"), "повтор клавиш вычёркивается из скриптов раскладки");
    assert!(edits.contains("${CONFIG}/panel-colorizer/presets/"));
    for app in ["garuda-toolbox.desktop", "snapper-tools.desktop", "octopi.desktop"] {
        assert!(edits.contains(app), "ярлык {app} убирается из дока");
    }
    // Пресеты Panel Colorizer — в каталог пользователя, не в чужой пакет.
    for f in s["files"].as_array().unwrap() {
        let to = f["to"].as_str().unwrap();
        assert!(to.starts_with("${CONFIG}/") || to.starts_with("${DATA}/"), "{to}: только домашняя папка");
    }
}

// ── SteamOS ─────────────────────────────────────────────────────────────

#[test]
fn steamos_plasma_is_all_native() {
    let pkg = load("steamos");
    let plan = choose_all(&pkg, &plasma6(), &no_gen);
    assert!(plan.installable());
    assert!(plan.skipped().is_empty(), "{:?}", plan.skipped());
    expect(
        &by_kind(&pkg, &plan),
        &[
            ("global-look", Some("kde-lnf")),
            ("palette", Some("kde-colors")),
            ("shell-theme", Some("plasma")),
            ("gtk-theme", Some("gtk")),
            ("icons", Some("freedesktop")),
            ("cursors", Some("freedesktop")),
            ("behaviour", Some("kconfig")),
            ("game-session", Some("freedesktop")),
        ],
    );
}

#[test]
fn steamos_on_xfce_and_gnome_keeps_gtk_icons_and_game_mode() {
    let pkg = load("steamos");
    for s in [xfce(), gnome()] {
        let plan = choose_all(&pkg, &s, &no_gen);
        assert!(plan.installable());
        expect(
            &by_kind(&pkg, &plan),
            &[
                ("gtk-theme", Some("gtk")),
                ("icons", Some("freedesktop")),
                ("game-session", Some("freedesktop")),
                ("global-look", None),
                ("palette", None),
                ("shell-theme", None),
                ("behaviour", None),
            ],
        );
    }
}

#[test]
fn steamos_game_mode_forbids_jupiter_hw_support() {
    let pkg = load("steamos");
    let gs = pkg.items.iter().find(|i| i.kind == "game-session").expect("игровой режим");
    assert!(!gs.required, "игровой режим — по желанию");
    assert!(gs.note.as_ref().is_some_and(|n| n.get("en").contains("two graphics cards")));
    assert!(gs.note.as_ref().unwrap().get("ru").contains("двумя видеокартами"));

    let deps = pkg.deps_for("game-session");
    let aur = deps.iter().find(|d| d.source == DepSource::Aur).expect("сеанс из AUR");
    assert!(aur.forbid.iter().any(|f| f == "jupiter-hw-support"));
    assert_eq!(
        aur.install_names(PkgFamily::Arch).unwrap(),
        ["aur/gamescope-session-git", "aur/gamescope-session-steam-git"]
    );

    let r = aur.resolve(PkgFamily::Arch, &|_| true).expect("источник AUR");
    assert_eq!(r.source, DepSource::Aur);
    // Встало ровно то, что просили, — годится.
    let asked = ["gamescope-session-git", "gamescope-session-steam-git"];
    let before = |_: &str| None;
    let clean = |n: &str| asked.contains(&n).then(|| n.to_string());
    assert!(verify_dependency(aur, &r, &before, &clean).is_ok());
    // Притянулся jupiter-hw-support — отказ.
    let pulled = |n: &str| (asked.contains(&n) || n == "jupiter-hw-support").then(|| n.to_string());
    assert!(verify_dependency(aur, &r, &before, &pulled).is_err());
    // Подмена провайдером из репозитория CachyOS — отказ.
    let swapped = |n: &str| match n {
        "gamescope-session-steam-git" => Some("gamescope-session-cachyos".to_string()),
        n if asked.contains(&n) => Some(n.to_string()),
        _ => None,
    };
    assert!(verify_dependency(aur, &r, &before, &swapped).is_err());

    let s = settings("steamos");
    assert_eq!(s["game_session"]["autologin"], false);
    assert_eq!(s["game_session"]["autostart_steam"], false);
}

#[test]
fn steamos_variants_match_property_and_skip_logos() {
    let pkg = load("steamos");
    let prop = pkg.property("variant").expect("свойство variant");
    let s = settings("steamos");
    let variants = s["variants"].as_object().unwrap();
    assert_eq!(variants.keys().map(String::as_str).collect::<Vec<_>>(), prop.option_values());
    assert!(variants.contains_key(prop.default.as_str().unwrap()));
    // Подписи вариантов — на трёх языках.
    for o in &prop.options {
        for lang in ["en", "ru", "de"] {
            assert_ne!(o.label(lang), o.value(), "вариант {} без подписи {lang}", o.value());
        }
    }
    assert_ne!(prop.options[1].label("ru"), prop.options[1].label("en"));
    // VGUI: курсор KDE_Classic требует свою зависимость — и только он.
    let role = variants["vgui"]["needs_role"].as_str().unwrap();
    let dep = pkg.requires.packages.iter().find(|d| d.role == role).expect("зависимость курсора");
    assert_eq!(dep.when.get("variant"), Some(&serde_json::json!("vgui")));
    let roles = |values: &PropValues| {
        pkg.deps_for_choice_with("behaviour", "kconfig", values).iter().map(|d| d.role.clone()).collect::<Vec<_>>()
    };
    let vgui = PropValues::from([("variant".to_string(), serde_json::json!("vgui"))]);
    let vapor = PropValues::from([("variant".to_string(), serde_json::json!("vapor"))]);
    assert!(roles(&vgui).contains(&role.to_string()));
    assert!(!roles(&vapor).contains(&role.to_string()));
    assert!(!roles(&PropValues::new()).contains(&role.to_string()), "по умолчанию — Vapor");
    assert!(!pkg.deps_for_choice("behaviour", "kconfig").iter().any(|d| d.role == role));
    // Логотипы и обои Steam Deck не берутся (марка Valve).
    let exclude = s["exclude"].to_string();
    for p in ["wallpapers", "com.valve.vapor.deck.desktop", "contents/splash", "usr/share/icons"] {
        assert!(exclude.contains(p), "{p} должен быть исключён");
    }
    assert!(!pkg.items.iter().any(|i| i.kind == "wallpaper" || i.kind == "session-splash"));
}

// ── применение и «Вернуть как было» ─────────────────────────────────────

mod apply {
    use super::*;
    use rexon_core::distro_looks::{self as dl, FetchJob, Machine, Prepared, Probes, Request, Target};
    use rexon_core::install_plan::{no_sync_env, Ctx};
    use rexon_core::utils::RunReport;
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    /// Поддельные программы стола: пишут вызов в журнал и изображают
    /// запись настроек в файлы профиля (по ним идёт сверка возврата).
    const FAKES: &[(&str, &str)] = &[
        ("kwriteconfig6", r#"f=""; g=""; k=""; while [ $# -gt 1 ]; do case "$1" in --file) f="$2"; shift 2;; --group) g="$g[$2]"; shift 2;; --key) k="$2"; shift 2;; *) shift;; esac; done
mkdir -p "$(dirname "$f")"; printf '%s\n%s=%s\n' "$g" "$k" "$1" >> "$f""#),
        ("plasma-apply-lookandfeel", r#"mkdir -p "$HOME/.config"; printf '[KDE]\nLookAndFeelPackage=%s\n' "$2" >> "$HOME/.config/kdeglobals"; [ "$3" = "--resetLayout" ] && echo layout >> "$HOME/.config/plasma-org.kde.plasma.desktop-appletsrc"; exit 0"#),
        ("plasma-apply-colorscheme", r#"printf '[General]\nColorScheme=%s\n' "$1" >> "$HOME/.config/kdeglobals""#),
        ("plasma-apply-desktoptheme", r#"printf '[Theme]\nname=%s\n' "$1" >> "$HOME/.config/plasmarc""#),
        ("plasma-apply-cursortheme", r#"printf '[Mouse]\ncursorTheme=%s\n' "$1" >> "$HOME/.config/kcminputrc""#),
        ("qdbus", "exit 0"),
        ("fc-cache", "exit 0"),
        ("xfconf-query", r#"c=""; p=""; v=""; set_=0; reset=0; while [ $# -gt 0 ]; do case "$1" in -c) c="$2"; shift 2;; -p) p="$2"; shift 2;; -s) v="$2"; set_=1; shift 2;; -t) shift 2;; -r) reset=1; shift;; *) shift;; esac; done
d="$HOME/.xfstore/$c$p"; if [ $reset = 1 ]; then rm -f "$d"; x="$(dirname "$d")"; while [ "$x" != "$HOME" ] && rmdir "$x" 2>/dev/null; do x="$(dirname "$x")"; done; elif [ $set_ = 1 ]; then mkdir -p "$(dirname "$d")"; printf '%s' "$v" > "$d"; else [ -f "$d" ] && cat "$d" && echo; fi"#),
        // Как настоящий: значения схемы по умолчанию (`GSETTINGS_BACKEND=memory`)
        // и сброс; строку в кавычках GVariant хранит без кавычек.
        ("gsettings", r#"d="$HOME/.gsstore/$2/$3"; case "$1" in set) v="$4"; case "$v" in \'*\') v="${v#\'}"; v="${v%\'}";; esac; mkdir -p "$(dirname "$d")"; printf '%s' "$v" > "$d";; get) if [ "$GSETTINGS_BACKEND" != memory ] && [ -f "$d" ]; then printf "'%s'\n" "$(cat "$d")"; else echo "'(default)'"; fi;; reset) rm -f "$d"; rmdir "$(dirname "$d")" "$HOME/.gsstore" 2>/dev/null; true;; esac"#),
        ("env", r#"while [ $# -gt 0 ]; do case "$1" in *=*) export "$1"; shift;; *) break;; esac; done; exec "$@""#),
        ("sh", r#"exec /bin/sh "$@""#),
        // Панель MATE: ветка dconf — один файл; раскладка пишет в неё своё имя.
        ("dconf", r#"f="$HOME/.dconfstore/mate-panel.txt"; case "$1" in dump) [ -f "$f" ] && cat "$f"; true;; load) mkdir -p "$(dirname "$f")"; cat > "$f";; *) true;; esac"#),
        ("mate-panel", r#"[ "$1" = "--layout" ] && { mkdir -p "$HOME/.dconfstore"; printf 'layout=%s\n' "$2" > "$HOME/.dconfstore/mate-panel.txt"; }; true"#),
        // Compiz: 3D есть, процессы «живы», отсоединённый запуск только
        // записывается — живой стол стенд не трогает ни при каком исходе.
        ("glxinfo", r#"printf 'direct rendering: Yes
OpenGL renderer string: AMD Radeon 780M
'"#),
        ("setsid", "true"),
        ("pgrep", "true"),
        ("sleep", "true"),
        // Автозапуск Compiz на XFCE кладётся копией ярлыка.
        ("cp", r#"exec /bin/cp "$@""#),
        ("kvantummanager", r#"[ "$1" = "--set" ] && { mkdir -p "$HOME/.config/Kvantum"; printf '[General]\ntheme=%s\n' "$2" > "$HOME/.config/Kvantum/kvantum.kvconfig"; }; true"#),
    ];

    /// Ярлыки «в системе» стенда: по пути, как в `/usr/share/applications`.
    const DESKTOPS: &[&str] = &["mate-terminal.desktop", "firefox.desktop", "caja-browser.desktop", "matecc.desktop", "thunar.desktop", "xfce4-terminal.desktop", "nemo.desktop"];

    struct Bench {
        root: PathBuf,
        home: PathBuf,
        bin: PathBuf,
        log: PathBuf,
    }

    impl Bench {
        fn new(tag: &str) -> Bench {
            let root = std::env::temp_dir().join(format!("rexon-looks-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let home = root.join("home");
            let bin = root.join("bin");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::create_dir_all(&bin).unwrap();
            for (name, body) in FAKES {
                let p = bin.join(name);
                std::fs::write(&p, format!("#!/bin/sh\necho \"{name} $*\" >> \"$LOG\"\n{body}\n")).unwrap();
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            let log = root.join("calls.log");
            Bench { root, home, bin, log }
        }

        fn target(&self, de: &str) -> Target {
            let h = self.home.to_string_lossy().into_owned();
            Target {
                de: de.into(),
                session: dl::session_for(de, BTreeMap::new()),
                family: PkgFamily::Arch,
                config: format!("{h}/.config"),
                data: format!("{h}/.local/share"),
                cache: format!("{h}/.cache"),
                home: h,
                lang: "en".into(),
                wayland: false,
                cursor_size: None,
                aur_helper: Some("yay".into()),
            }
        }

        /// Запуск ТОЛЬКО из своего каталога поддельных программ.
        fn run(&self, argv: &[String], missing: &RefCell<Vec<String>>) -> RunReport {
            let exe = self.bin.join(&argv[0]);
            if argv[0].contains('/') || !exe.is_file() {
                missing.borrow_mut().push(argv.join(" "));
                return RunReport { returncode: Some(127), ..Default::default() };
            }
            let out = std::process::Command::new(&exe)
                .args(&argv[1..])
                .env_clear()
                .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
                .env("HOME", &self.home)
                .env("LOG", &self.log)
                .output()
                .unwrap();
            RunReport { returncode: out.status.code(), stdout: out.stdout, stderr: out.stderr, ..Default::default() }
        }

        fn calls(&self) -> String {
            std::fs::read_to_string(&self.log).unwrap_or_default()
        }

        /// Профиль побайтно, без каталогов Rexon (журнал, кэш).
        fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
            fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
                let Ok(rd) = std::fs::read_dir(dir) else { return };
                for e in rd.flatten() {
                    let p = e.path();
                    let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                    if rel == ".config/rexon" || rel == ".cache" {
                        continue;
                    }
                    let md = std::fs::symlink_metadata(&p).unwrap();
                    if md.is_dir() {
                        out.insert(format!("{rel}/"), Vec::new());
                        walk(&p, root, out);
                    } else if md.file_type().is_symlink() {
                        out.insert(rel, std::fs::read_link(&p).unwrap().to_string_lossy().as_bytes().to_vec());
                    } else {
                        out.insert(rel, std::fs::read(&p).unwrap());
                    }
                }
            }
            let mut out = BTreeMap::new();
            walk(&self.home, &self.home, &mut out);
            out
        }

        /// Подменить поддельную программу стенда.
        fn fake(&self, name: &str, body: &str) {
            let p = self.bin.join(name);
            std::fs::write(&p, format!("#!/bin/sh\necho \"{name} $*\" >> \"$LOG\"\n{body}\n")).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        fn put(&self, rel: &str, text: &str) {
            let p = self.home.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Подставная загрузка: файлы, которые нужны образам, и метка закрепления.
    fn fake_fetch(job: &FetchJob) -> Result<(), String> {
        let d = &job.dir;
        match job.id.as_str() {
            "valve" => {
                for lnf in ["com.valve.vapor.desktop", "com.valve.vgui.desktop", "com.valve.vapor.deck.desktop"] {
                    let base = d.join("usr/share/plasma/look-and-feel").join(lnf);
                    write(&base.join("metadata.json"), "{}");
                    write(&base.join("contents/defaults"), "[kdeglobals][General]\nColorScheme=Vapor\n");
                    write(&base.join("contents/splash/Splash.qml"), "import QtQuick\n");
                }
                write(&d.join("usr/share/color-schemes/Vapor.colors"), "[General]\nName=Vapor\n");
                write(&d.join("usr/share/color-schemes/VGUI.colors"), "[General]\nName=VGUI\n");
                write(&d.join("usr/share/plasma/desktoptheme/Vapor/metadata.json"), "{}");
                write(&d.join("usr/share/themes/Vapor/gtk-3.0/gtk.css"), "* {}\n");
                write(&d.join("usr/share/wallpapers/deck.png"), "png");
            }
            "sweet-gtk" => {
                write(&d.join("Sweet-Dark/index.theme"), "[Desktop Entry]\nName=Sweet-Dark\n");
                write(&d.join("Sweet-Dark/gtk-3.0/gtk.css"), "* {}\n");
                write(&d.join("Sweet-Dark/xfwm4/themerc"), "x=1\n");
            }
            "sweet" => {
                write(&d.join("kde/cursors/Sweet-cursors/index.theme"), "[Icon Theme]\nName=Sweet-cursors\n");
                write(&d.join("kde/cursors/Sweet-cursors/cursors/left_ptr"), "cur");
                std::os::unix::fs::symlink("left_ptr", d.join("kde/cursors/Sweet-cursors/cursors/default")).unwrap();
                write(&d.join("kde/colorschemes/Sweet.colors"), "[General]\n");
                write(&d.join("kde/aurorae/Sweet-Dark/Sweet-Darkrc"), "[General]\n");
            }
            "garuda" => {
                write(&d.join("usr/share/wallpapers/Maldrakor/contents/images/3840x1920.jpg"), "jpg");
                write(&d.join("usr/share/wallpapers/Maldrakor/metadata.json"), "{}");
                let lnf = d.join("usr/share/plasma/look-and-feel/Dr460nized");
                write(&lnf.join("metadata.json"), "{}");
                write(&lnf.join("contents/defaults"), "[kdeglobals][General]\nColorScheme=Sweet\n");
                write(
                    &lnf.join("contents/layouts/org.kde.plasma.desktop-layout.js"),
                    "loadTemplate(\"org.garuda.desktop.defaultPanel\")\nkbd.writeEntry('RepeatDelay', 250);\n",
                );
                write(&d.join("usr/share/plasma/desktoptheme/Dr460nized/metadata.json"), "{}");
                write(&d.join("usr/share/Kvantum/Dr460nized/Dr460nized.kvconfig"), "[%General]\n");
                let lt = d.join("usr/share/plasma/layout-templates");
                write(
                    &lt.join("org.garuda.desktop.defaultPanel/contents/layout.js"),
                    "var w = panel.addWidget(\"org.kde.windowtitle\");\nw.writeConfig(\"a\", 1);\npanel.addWidget(\"org.kde.plasma.kickoff\");\n",
                );
                write(
                    &lt.join("org.garuda.desktop.defaultDock/contents/layout.js"),
                    "tasks.writeConfig(\"launchers\", \"applications:garuda-toolbox.desktop,preferred://browser\")\n",
                );
                write(&lt.join("org.garuda.desktop.defaultPanel/metadata.json"), "{}");
                write(&lt.join("org.garuda.desktop.defaultDock/metadata.json"), "{}");
                write(&d.join("usr/share/aurorae/themes/Sweet-Dark/Sweet-Darkrc-dr460nized"), "[General]\nGaruda=1\n");
            }
            other => return Err(format!("неожиданный источник {other}")),
        }
        write(&d.join(dl::PIN_MARK), &format!("{}\n", match &job.pin {
            dl::Pin::Git(s) | dl::Pin::Sha256(s) => s.clone(),
        }));
        Ok(())
    }

    /// Система пакетов: что стоит; установка дописывает в неё.
    struct Pkgs {
        have: RefCell<BTreeMap<String, String>>,
        installed: RefCell<Vec<String>>,
        removed: RefCell<Vec<String>>,
    }

    impl Pkgs {
        fn new(have: &[(&str, &str)]) -> Pkgs {
            Pkgs {
                have: RefCell::new(have.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()),
                installed: RefCell::new(Vec::new()),
                removed: RefCell::new(Vec::new()),
            }
        }
        fn owner(&self, n: &str) -> Option<String> {
            self.have.borrow().get(n).cloned()
        }
    }

    fn look(id: &str) -> dl::Look {
        dl::builtin(id).unwrap().unwrap()
    }

    fn ready(p: Prepared) -> dl::LookPlan {
        match p {
            Prepared::Ready(plan) => plan,
            Prepared::Refused { reason, .. } => panic!("отказ: {reason}"),
        }
    }

    /// Применить и вернуть с машиной стенда; `extra` — что ещё «приносит»
    /// установка (подмена провайдером, запрещённое).
    fn run_apply(
        b: &Bench,
        de: &str,
        id: &str,
        pkgs: &Pkgs,
        in_repos: &dyn Fn(&str) -> bool,
        install_fails: bool,
        extra: &[(&str, &str)],
    ) -> (Result<dl::AppliedLook, String>, Vec<String>) {
        let t = b.target(de);
        let missing = RefCell::new(Vec::new());
        let run = |argv: &[String], _: Option<u64>| b.run(argv, &missing);
        let have = |p: &str| b.bin.join(p).is_file();
        let spawn = |argv: &[String]| panic!("запуск без ожидания в тесте: {argv:?}");
        let ctx = Ctx { install: true, steamos: false, privileged: None, run: &run, have: &have, spawn: &spawn, wayland: false, sync_env: &no_sync_env };
        let owner = |n: &str| pkgs.owner(n);
        let no_ext = |_: &str| false;
        let probes = Probes { owner_of: &owner, in_repos, extension_installed: &no_ext };
        let lk = look(id);
        let plan = ready(dl::plan(&lk, &t, &Request::default(), &probes));
        let install = |names: &[String]| -> Result<(), String> {
            if install_fails {
                return Err("окно пароля закрыто".into());
            }
            let mut h = pkgs.have.borrow_mut();
            for n in names {
                h.insert(n.clone(), n.clone());
                pkgs.installed.borrow_mut().push(n.clone());
            }
            for (a, o) in extra {
                h.insert(a.to_string(), o.to_string());
            }
            Ok(())
        };
        let remove = |names: &[String]| -> Result<(), String> {
            pkgs.removed.borrow_mut().extend(names.iter().cloned());
            Ok(())
        };
        let install_aur = |names: &[String]| -> Result<(), String> {
            let mut h = pkgs.have.borrow_mut();
            for n in names {
                h.insert(n.clone(), n.clone());
                pkgs.installed.borrow_mut().push(format!("aur/{n}"));
            }
            Ok(())
        };
        let kpkg = |_: &str, _: &str| false;
        let desktop = |_: &str| false;
        let desktop_path = |n: &str| DESKTOPS.contains(&n).then(|| format!("/usr/share/applications/{n}"));
        let m = Machine {
            ctx: &ctx,
            fetch: &fake_fetch,
            install_packages: &install,
            remove_packages: &remove,
            owner_of: &owner,
            kpackage_installed: &kpkg,
            desktop_installed: &desktop,
            desktop_path: &desktop_path,
            install_aur: &install_aur,
        };
        let r = dl::apply(&lk, &plan, &t, &m);
        let miss = missing.borrow().clone();
        (r, miss)
    }

    fn run_revert(b: &Bench, de: &str, id: &str) -> dl::Reverted {
        let t = b.target(de);
        let missing = RefCell::new(Vec::new());
        let run = |argv: &[String], _: Option<u64>| b.run(argv, &missing);
        let have = |p: &str| b.bin.join(p).is_file();
        let spawn = |argv: &[String]| panic!("запуск без ожидания в тесте: {argv:?}");
        let ctx = Ctx { install: true, steamos: false, privileged: None, run: &run, have: &have, spawn: &spawn, wayland: false, sync_env: &no_sync_env };
        let r = dl::revert(id, &t, &ctx).unwrap();
        assert!(missing.borrow().is_empty(), "возврат звал программы вне стенда: {:?}", missing.borrow());
        r
    }

    fn plasma_profile(b: &Bench) {
        b.put(".config/kdeglobals", "[General]\nColorScheme=BreezeDark\n");
        b.put(".config/kwinrc", "[Plugins]\nblurEnabled=true\n");
        b.put(".config/gtk-3.0/settings.ini", "[Settings]\ngtk-theme-name=Breeze\n");
    }

    #[test]
    fn steamos_on_plasma_applies_and_reverts_byte_for_byte() {
        let b = Bench::new("steam-plasma");
        plasma_profile(&b);
        let before = b.snapshot();
        let pkgs = Pkgs::new(&[("libplasma", "libplasma"), ("breeze-cursors", "breeze-cursors")]);
        let (r, missing) = run_apply(&b, "kde", "steamos", &pkgs, &|_| true, false, &[]);
        let applied = r.unwrap();
        assert!(missing.is_empty(), "команды вне стенда: {missing:?}");
        assert_eq!(*pkgs.installed.borrow(), vec!["breeze-icons".to_string()], "только репозиторий и только нужное");
        let calls = b.calls();
        for want in [
            "plasma-apply-lookandfeel -a com.valve.vapor.desktop",
            "plasma-apply-colorscheme Vapor",
            "plasma-apply-desktoptheme Vapor",
            "plasma-apply-cursortheme breeze_cursors",
            "kwriteconfig6 --file",
            "qdbus org.kde.KWin /KWin org.kde.KWin.reconfigure",
        ] {
            assert!(calls.contains(want), "нет вызова «{want}»:\n{calls}");
        }
        let data = b.home.join(".local/share");
        assert!(data.join("plasma/look-and-feel/com.valve.vapor.desktop/.rexon-owner").is_file());
        assert!(!data.join("plasma/look-and-feel/com.valve.vapor.deck.desktop").exists(), "исключённое не кладётся");
        assert!(!data.join("plasma/look-and-feel/com.valve.vgui.desktop/contents/splash").exists());
        assert!(data.join("color-schemes/Vapor.colors").is_file());
        assert!(data.join("themes/Vapor/gtk-3.0/gtk.css").is_file());
        let gtk = std::fs::read_to_string(b.home.join(".config/gtk-3.0/settings.ini")).unwrap();
        assert!(gtk.contains("gtk-theme-name=Vapor"), "{gtk}");
        assert!(applied.skipped.iter().any(|s| s.kind == "game-session"), "игровой режим — только по желанию");
        assert!(dl::is_applied("steamos", &b.target("kde").rexon_dir()));

        let back = run_revert(&b, "kde", "steamos");
        assert!(back.reverted && back.not_restored.is_empty(), "{back:?}");
        assert_eq!(back.packages, vec!["breeze-icons".to_string()], "пакеты называются, но не снимаются");
        assert!(back.restart_shell);
        assert_eq!(b.snapshot(), before, "профиль после «Вернуть как было» не совпал побайтно");
        assert!(!dl::is_applied("steamos", &b.target("kde").rexon_dir()));
        assert!(pkgs.removed.borrow().is_empty());
    }

    #[test]
    fn garuda_on_xfce_applies_gtk_part_and_reverts_byte_for_byte() {
        let b = Bench::new("garuda-xfce");
        for (p, v) in [
            ("xsettings/Net/ThemeName", "Adwaita"),
            ("xsettings/Net/IconThemeName", "Papirus"),
            ("xsettings/Gtk/CursorThemeName", "Adwaita"),
            ("xsettings/Gtk/FontName", "Sans 10"),
            ("xfwm4/general/theme", "Default"),
        ] {
            b.put(&format!(".xfstore/{p}"), v);
        }
        b.put(".config/gtk-3.0/settings.ini", "[Settings]\ngtk-theme-name=Adwaita\n");
        let before = b.snapshot();
        let pkgs = Pkgs::new(&[]);
        // BeautyLine в репозиториях нет — только AUR: значки пропускаются.
        let in_repos = |n: &str| n != "beautyline";
        let (r, missing) = run_apply(&b, "xfce", "garuda-dr460nized", &pkgs, &in_repos, false, &[]);
        let applied = r.unwrap();
        let calls = b.calls();
        assert!(missing.iter().all(|m| !m.starts_with("xfconf-query")), "{missing:?}");
        let store = |p: &str| std::fs::read_to_string(b.home.join(".xfstore").join(p)).unwrap_or_default();
        assert_eq!(store("xsettings/Net/ThemeName"), "Sweet-Dark", "{calls}");
        assert_eq!(store("xfwm4/general/theme"), "Sweet-Dark");
        assert_eq!(store("xsettings/Gtk/CursorThemeName"), "Sweet-cursors");
        assert_eq!(store("xsettings/Gtk/FontName"), "Fira Sans 10");
        assert_eq!(store("xsettings/Net/IconThemeName"), "Papirus", "значки из AUR сами не ставятся");
        assert!(applied.aur.iter().any(|a| a.packages == vec!["aur/beautyline".to_string()]));
        assert!(applied.skipped.iter().any(|s| s.kind == "icons"));
        // Глобальная тема вне Plasma — набор этих же частей: не «пропущена».
        assert!(!applied.skipped.iter().any(|s| s.kind == "global-look"));
        let data = b.home.join(".local/share");
        assert!(data.join("themes/Sweet-Dark/.rexon-owner").is_file());
        assert!(data.join("icons/Sweet-cursors/cursors/default").is_symlink(), "ссылки курсоров сохраняются");
        assert!(data.join("wallpapers/Maldrakor/contents/images/3840x1920.jpg").is_file());
        assert!(pkgs.installed.borrow().contains(&"ttf-fira-sans".to_string()));

        run_revert(&b, "xfce", "garuda-dr460nized");
        assert_eq!(b.snapshot(), before, "профиль после «Вернуть как было» не совпал побайтно");
    }

    #[test]
    fn forked_libplasma_refuses_before_anything() {
        let b = Bench::new("fork");
        let t = b.target("kde");
        let owner = |n: &str| (n == "libplasma").then(|| "aeroshell-libplasma".to_string());
        let yes = |_: &str| true;
        let no = |_: &str| false;
        let probes = Probes { owner_of: &owner, in_repos: &yes, extension_installed: &no };
        match dl::plan(&look("garuda-dr460nized"), &t, &Request::default(), &probes) {
            Prepared::Refused { reason, .. } => assert!(reason.contains("libplasma"), "{reason}"),
            Prepared::Ready(_) => panic!("форк libplasma пропущен"),
        }
    }

    #[test]
    fn closed_password_window_applies_nothing() {
        let b = Bench::new("all-or-nothing");
        plasma_profile(&b);
        let before = b.snapshot();
        let pkgs = Pkgs::new(&[("libplasma", "libplasma"), ("breeze-cursors", "breeze-cursors")]);
        let (r, _) = run_apply(&b, "kde", "steamos", &pkgs, &|_| true, true, &[]);
        let why = r.unwrap_err();
        assert!(why.contains("breeze-icons"), "{why}");
        assert_eq!(b.calls(), "", "ни одной команды стола");
        assert_eq!(b.snapshot(), before);
        assert!(!dl::is_applied("steamos", &b.target("kde").rexon_dir()));
    }

    #[test]
    fn forbidden_package_appearing_rolls_installation_back() {
        let b = Bench::new("forbid");
        plasma_profile(&b);
        let before = b.snapshot();
        let pkgs = Pkgs::new(&[("libplasma", "libplasma"), ("breeze-cursors", "breeze-cursors")]);
        let (r, _) = run_apply(&b, "kde", "steamos", &pkgs, &|_| true, false, &[("steamdeck-kde-presets", "steamdeck-kde-presets")]);
        let why = r.unwrap_err();
        assert!(why.contains("steamdeck-kde-presets"), "{why}");
        let removed: BTreeSet<String> = pkgs.removed.borrow().iter().cloned().collect();
        assert!(removed.contains("breeze-icons") && removed.contains("steamdeck-kde-presets"), "{removed:?}");
        assert_eq!(b.calls(), "");
        assert_eq!(b.snapshot(), before);
    }

    // ── эталоны: план, вызовы, профиль после и окно вопроса ────────────

    /// Каталог эталонов образов.
    fn golden_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/distro_looks")
    }

    /// Снять применение образа `id` на столе `de` в один текст: окно
    /// вопроса, план, вызовы программ стола и профиль после применения
    /// (пути дома заменены на `$HOME`). `REXON_UPDATE_GOLDEN=1` —
    /// переписать эталон, иначе — сверить побайтно.
    fn check_golden(de: &str, id: &str, prepare: &dyn Fn(&Bench), have: &[(&str, &str)]) {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let b = Bench::new(&format!("golden-{id}-{de}"));
        prepare(&b);
        let t = b.target(de);
        let pkgs = Pkgs::new(have);
        let owner = |n: &str| pkgs.owner(n);
        let yes = |_: &str| true;
        let no = |_: &str| false;
        let probes = Probes { owner_of: &owner, in_repos: &yes, extension_installed: &no };
        let offer = dl::offer(look(id), &t, &probes, &|_| false);
        let confirm = rexon_core::qt_desktops::look_confirm(&offer, &PropValues::new(), false);
        let plan = ready(dl::plan(&look(id), &t, &Request::default(), &probes));
        let (r, missing) = run_apply(&b, de, id, &pkgs, &yes, false, &[]);
        let applied = r.unwrap();
        let home = b.home.to_string_lossy().into_owned();
        let uid = String::from_utf8(std::process::Command::new("id").arg("-u").output().unwrap().stdout).unwrap();
        let uid = format!("-u {}", uid.trim());
        let norm = |s: &str| s.replace(&home, "$HOME").replace(&uid, "-u $UID");
        let profile: BTreeMap<String, String> =
            b.snapshot().into_iter().map(|(k, v)| (k, norm(&String::from_utf8_lossy(&v)))).collect();
        let doc = serde_json::json!({
            "confirm": serde_json::to_value(&confirm).unwrap(),
            "plan": serde_json::from_str::<Value>(&norm(&serde_json::to_string(&plan).unwrap())).unwrap(),
            "calls": norm(&b.calls()).lines().collect::<Vec<_>>(),
            "missing": missing.iter().map(|m| norm(m)).collect::<Vec<_>>(),
            "message": applied.message,
            "installed": *pkgs.installed.borrow(),
            "profile": profile,
        });
        let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
        let path = golden_dir().join(format!("{id}-{de}.json"));
        if std::env::var("REXON_UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
            std::fs::create_dir_all(golden_dir()).unwrap();
            std::fs::write(&path, &text).unwrap();
        } else {
            let want = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e} (снять: REXON_UPDATE_GOLDEN=1)", path.display()));
            assert!(want == text, "эталон {} разошёлся:\n{text}", path.display());
        }
    }

    /// Plasma: образ Garuda не меняется от того, что умеют прочие столы.
    #[test]
    fn golden_garuda_on_kde() {
        check_golden("kde", "garuda-dr460nized", &plasma_profile, &[("libplasma", "libplasma")]);
    }

    /// Профиль MATE до применения: тема, рамки, панель.
    fn mate_profile(b: &Bench) {
        b.put(".gsstore/org.mate.interface/gtk-theme", "Menta");
        b.put(".gsstore/org.mate.Marco.general/theme", "Menta");
        b.put(".gsstore/org.mate.Marco.general/button-layout", "menu:minimize,maximize,close");
        b.put(".dconfstore/mate-panel.txt", "[general]\nobject-id-list=['menu-bar', 'clock']\n");
        b.put(".config/gtk-3.0/settings.ini", "[Settings]\ngtk-theme-name=Menta\n");
        b.put(".config/qt6ct/qt6ct.conf", "[Appearance]\nstyle=Fusion\n");
    }

    fn xfce_profile(b: &Bench) {
        for (p, v) in [("xsettings/Net/ThemeName", "Adwaita"), ("xfwm4/general/theme", "Default"), ("xfwm4/general/button_layout", "O|HMC")] {
            b.put(&format!(".xfstore/{p}"), v);
        }
        b.put(".config/gtk-3.0/settings.ini", "[Settings]\ngtk-theme-name=Adwaita\n");
    }

    fn cinnamon_profile(b: &Bench) {
        b.put(".gsstore/org.cinnamon/enabled-applets", "['panel1:left:0:menu@cinnamon.org:0']");
        b.put(".gsstore/org.cinnamon/panels-enabled", "['1:0:bottom']");
        b.put(".gsstore/org.cinnamon.theme/name", "Mint-Y");
        b.put(".config/gtk-3.0/settings.ini", "[Settings]\ngtk-theme-name=Mint-Y\n");
    }

    const NON_KDE_HAVE: &[(&str, &str)] = &[];

    #[test]
    fn golden_garuda_on_mate() {
        check_golden("mate", "garuda-dr460nized", &mate_profile, NON_KDE_HAVE);
    }

    #[test]
    fn golden_garuda_on_xfce() {
        check_golden("xfce", "garuda-dr460nized", &xfce_profile, NON_KDE_HAVE);
    }

    #[test]
    fn golden_garuda_on_cinnamon() {
        check_golden("cinnamon", "garuda-dr460nized", &cinnamon_profile, NON_KDE_HAVE);
    }

    /// Вне Plasma: всё, что образ собрал сам (палитра и стиль Qt, панели,
    /// поведение окна), «Вернуть как было» снимает побайтно — и раскладку
    /// панели MATE из снимка dconf тоже.
    #[test]
    fn garuda_on_mate_and_cinnamon_reverts_byte_for_byte() {
        for (de, prep) in [("mate", &mate_profile as &dyn Fn(&Bench)), ("cinnamon", &cinnamon_profile), ("xfce", &xfce_profile)] {
            let b = Bench::new(&format!("garuda-revert-{de}"));
            prep(&b);
            let before = b.snapshot();
            let pkgs = Pkgs::new(&[]);
            let (r, _) = run_apply(&b, de, "garuda-dr460nized", &pkgs, &|_| true, false, &[]);
            let applied = r.unwrap();
            assert!(applied.skipped.iter().all(|s| s.kind != "palette" && s.kind != "qt-style"), "{de}: {:?}", applied.skipped);
            assert_ne!(b.snapshot(), before, "{de}: ничего не применилось");
            run_revert(&b, de, "garuda-dr460nized");
            let after = b.snapshot();
            let diff: Vec<&String> = before.keys().chain(after.keys()).filter(|k| before.get(*k) != after.get(*k)).collect();
            assert!(diff.is_empty(), "{de}: после «Вернуть как было» разошлось: {diff:?}");
        }
    }

    #[test]
    fn golden_steamos_on_mate() {
        check_golden("mate", "steamos", &mate_profile, NON_KDE_HAVE);
    }

    fn sway_profile(b: &Bench) {
        b.put(".config/sway/config", "set $mod Mod4\n");
        b.put(".config/gtk-3.0/settings.ini", "[Settings]\ngtk-theme-name=Adwaita\n");
    }

    #[test]
    fn golden_garuda_on_sway() {
        check_golden("sway", "garuda-dr460nized", &sway_profile, NON_KDE_HAVE);
    }

    #[test]
    fn golden_steamos_on_sway() {
        check_golden("sway", "steamos", &sway_profile, NON_KDE_HAVE);
    }

    /// Столы, где Compiz встаёт вместо оконного менеджера: стол, профиль
    /// до применения, прежний менеджер и где лежит «ключ сеанса».
    const COMPIZ_DESKS: &[(&str, fn(&Bench), &str, &str)] = &[
        ("mate", mate_profile, "Marco", ".gsstore/org.mate.session.required-components/windowmanager"),
        ("xfce", xfce_profile, "xfwm4", ".config/autostart/rexon-compiz.desktop"),
    ];

    /// Compiz упал после запуска: обратно прежний менеджер, сеанс не тронут,
    /// остальное образа применено, причина названа; возврат — побайтно.
    #[test]
    fn compiz_that_dies_gives_the_window_manager_back() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        for &(de, profile, wm, session_key) in COMPIZ_DESKS {
            let b = Bench::new(&format!("compiz-dies-{de}"));
            profile(&b);
            b.fake("pgrep", "exit 1");
            let before = b.snapshot();
            let pkgs = Pkgs::new(&[]);
            let (r, _) = run_apply(&b, de, "garuda-dr460nized", &pkgs, &|_| true, false, &[]);
            let applied = r.unwrap();
            assert!(applied.notes.iter().any(|n| n.contains(&format!("switched back to {wm}"))), "{de}: {:?}", applied.notes);
            let back = format!("setsid -f {} --replace", wm.to_lowercase());
            assert!(b.calls().lines().any(|l| l.starts_with(&back)), "{de}: {}", b.calls());
            assert!(!b.home.join(session_key).exists(), "{de}: сеанс не должен звать Compiz");
            let theme_set = match de {
                "mate" => b.home.join(".gsstore/org.mate.interface/gtk-theme").exists(),
                _ => b.home.join(".xfstore/xsettings/Net/ThemeName").exists(),
            };
            assert!(theme_set, "{de}: остальное образа применено");
            run_revert(&b, de, "garuda-dr460nized");
            assert_eq!(b.snapshot(), before, "{de}: профиль после «Вернуть как было» не совпал побайтно");
        }
    }

    /// Виртуальная машина без пробы 3D и программная отрисовка (llvmpipe):
    /// Compiz не включается вовсе, прежний менеджер остаётся.
    #[test]
    fn compiz_is_not_switched_on_without_real_3d() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        for &(de, profile, wm, session_key) in COMPIZ_DESKS {
            for (case, glx, virt, why) in [
                ("vm", "exit 1", Some("echo kvm"), "virtual machine (kvm)"),
                ("llvmpipe", "printf 'direct rendering: Yes\\nOpenGL renderer string: llvmpipe (LLVM 20.1.8, 256 bits)\\n'", None, "llvmpipe"),
            ] {
                let b = Bench::new(&format!("compiz-{case}-{de}"));
                profile(&b);
                b.fake("glxinfo", glx);
                if let Some(v) = virt {
                    b.fake("systemd-detect-virt", v);
                }
                let pkgs = Pkgs::new(&[]);
                let (r, _) = run_apply(&b, de, "garuda-dr460nized", &pkgs, &|_| true, false, &[]);
                let applied = r.unwrap();
                assert!(
                    applied.notes.iter().any(|n| n.contains(why) && n.contains(&format!("{wm} stays"))),
                    "{de}/{case}: {:?}",
                    applied.notes
                );
                assert!(!b.calls().contains("compiz --replace"), "{de}/{case}: {}", b.calls());
                assert!(!b.home.join(session_key).exists(), "{de}/{case}");
            }
        }
    }

    /// XFCE: Compiz прошёл сторожа — автозапуск с запасным xfwm4, рамки
    /// gtk-window-decorator из ключей GNOME (тема образа, кнопки слева);
    /// «Вернуть как было» — xfwm4 обратно и профиль побайтно.
    #[test]
    fn compiz_on_xfce_starts_from_autostart_and_reverts() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let b = Bench::new("compiz-xfce");
        xfce_profile(&b);
        b.put(".gsstore/org.gnome.desktop.wm.preferences/theme", "Adwaita");
        let before = b.snapshot();
        let pkgs = Pkgs::new(&[]);
        let (r, _) = run_apply(&b, "xfce", "garuda-dr460nized", &pkgs, &|_| true, false, &[]);
        let applied = r.unwrap();
        assert!(!applied.notes.iter().any(|n| n.contains("switched back")), "{:?}", applied.notes);
        let autostart = std::fs::read_to_string(b.home.join(".config/autostart/rexon-compiz.desktop")).unwrap();
        assert!(autostart.contains("Exec=sh -c \"env -u SESSION_MANAGER compiz --replace ccp || exec xfwm4 --replace\""), "{autostart}");
        assert!(autostart.contains("OnlyShowIn=XFCE;"));
        let gs = |k: &str| std::fs::read_to_string(b.home.join(format!(".gsstore/org.gnome.desktop.wm.preferences/{k}"))).unwrap();
        assert_eq!(gs("theme"), "Sweet-Dark");
        assert_eq!(gs("button-layout"), "close,maximize,minimize:");
        let calls = b.calls();
        let start = calls.find("setsid -f env -u SESSION_MANAGER compiz --replace ccp").expect("Compiz не запускался");
        assert!(calls.find("\ncp ").is_some_and(|c| c > start), "автозапуск — только после сторожа: {calls}");
        assert!(!calls.lines().any(|l| l.starts_with("setsid -f xfwm4")), "{calls}");
        assert!(!b.home.join(".xfstore/xfce4-session").exists(), "сеанс xfce4-session не трогается");
        run_revert(&b, "xfce", "garuda-dr460nized");
        assert!(b.calls().lines().any(|l| l.starts_with("setsid -f xfwm4 --replace")), "{}", b.calls());
        assert_eq!(b.snapshot(), before, "профиль после «Вернуть как было» не совпал побайтно");
    }

    #[test]
    fn xfwm4_buttons_from_gtk_layout() {
        assert_eq!(dl::xfwm4_button_layout("close,maximize,minimize:"), "CMH|");
        assert_eq!(dl::xfwm4_button_layout("menu:minimize,maximize,close"), "O|HMC");
    }

    #[test]
    fn golden_steamos_on_kde() {
        check_golden("kde", "steamos", &plasma_profile, &[("libplasma", "libplasma"), ("breeze-cursors", "breeze-cursors")]);
    }

    #[test]
    fn required_part_from_aur_only_refuses_whole_look() {
        // Причину отказа сверяем по исходному (английскому) тексту.
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        // Курсоры Garuda обязательны и берутся из git, а значки — нет; здесь
        // проверяем отказ целиком на обязательных значках SteamOS из AUR-only
        // системы: имени в репозиториях нет.
        let b = Bench::new("required");
        let t = b.target("kde");
        let owner = |n: &str| (n == "libplasma").then(|| n.to_string());
        let no_repo = |_: &str| false;
        let no = |_: &str| false;
        let probes = Probes { owner_of: &owner, in_repos: &no_repo, extension_installed: &no };
        match dl::plan(&look("steamos"), &t, &Request::default(), &probes) {
            Prepared::Refused { reason, .. } => assert!(reason.contains("icons"), "{reason}"),
            Prepared::Ready(p) => panic!("обязательные значки без пакета: {:?}", p.skipped),
        }
    }
}
