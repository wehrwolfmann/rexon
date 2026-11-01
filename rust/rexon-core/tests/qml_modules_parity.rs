//! Сверка [`rexon_core::qml_modules`] с оригиналом на Python.
//!
//! Оригинал — `rexon/qt_wallpaper_engine/qml_ui/__init__.py`
//! (`_module_dirs`, `_resolve_qcm_dir`, `qcm_material_available`,
//! `rexon_gui_candidates`, `upstream_tag`), `qml_ui/bridge.py`
//! (`_rust_filter_model_candidates`), `qml_ui/host.py` (`NET_CACHE_*`,
//! `_page_url`) и `rexon/qt_main/__init__.py` (`rust_gui_candidates`).
//!
//! Значения выписаны из оригинала дословно. Там, где ответ намеренно
//! ОТЛИЧАЕТСЯ, проверка названа `починка:` и говорит, чем именно.

use rexon_core::qml_modules as qm;

/// Подставная ФС: годными считаются ровно перечисленные пути.
struct FakeFs(Vec<String>);

impl FakeFs {
    fn new(files: &[&str]) -> Self {
        FakeFs(files.iter().map(|s| s.to_string()).collect())
    }
}

impl qm::ModuleFs for FakeFs {
    fn is_file(&mut self, path: &str) -> bool {
        self.0.iter().any(|p| p == path)
    }
}

const HOME: &str = "/home/wehrwolfmann";
const REPO: &str = "/home/wehrwolfmann/Загрузки/Rexon/rexon_v5";

// ============================================================================
// Каталоги модулей
// ============================================================================

#[test]
fn module_dirs_default_layout() {
    // Оригинал без XDG_DATA_DIRS и без мультиарх-каталогов:
    //   ~/.local/share/rexon/qml_modules, /usr/lib/..., /usr/lib64/...
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    assert_eq!(
        dirs,
        vec![
            format!("{HOME}/.local/share/rexon/qml_modules"),
            "/usr/lib/rexon/qml_modules".to_string(),
            "/usr/lib64/rexon/qml_modules".to_string(),
            // Починка 15.09.2026: установка в систему (раскладка пакета).
            "/usr/local/lib/rexon/qml_modules".to_string(),
            // Починка 15.09.2026: установка в домашнюю папку.
            format!("{HOME}/.local/lib/rexon/qml_modules"),
        ],
        "порядок каталогов оригинала"
    );
}

#[test]
fn module_dirs_xdg_data_dirs_inserted_between() {
    // Оригинал: пользовательский → XDG_DATA_DIRS → /usr/lib → /usr/lib64.
    let dirs = qm::module_dirs(None, HOME, "/opt/share:/usr/share", &[]);
    assert_eq!(
        dirs,
        vec![
            format!("{HOME}/.local/share/rexon/qml_modules"),
            "/opt/share/rexon/qml_modules".to_string(),
            "/usr/share/rexon/qml_modules".to_string(),
            "/usr/lib/rexon/qml_modules".to_string(),
            "/usr/lib64/rexon/qml_modules".to_string(),
            "/usr/local/lib/rexon/qml_modules".to_string(),
            format!("{HOME}/.local/lib/rexon/qml_modules"),
        ]
    );
}

#[test]
fn module_dirs_skips_blank_entries() {
    // Оригинал: `if base.strip()` — пустые куски "a::b" и хвост ":" мимо.
    let dirs = qm::module_dirs(None, HOME, ":/opt/share:: ", &[]);
    assert_eq!(dirs[1], "/opt/share/rexon/qml_modules");
    assert_eq!(dirs.len(), 6, "пустые куски XDG_DATA_DIRS не дают каталогов");
}

#[test]
fn module_dirs_multiarch_last() {
    let multi = vec!["/usr/lib/x86_64-linux-gnu/rexon/qml_modules".to_string()];
    let dirs = qm::module_dirs(None, HOME, "", &multi);
    assert_eq!(dirs.last().unwrap(), &multi[0], "мультиарх идёт последним");
}

#[test]
fn module_dirs_deduplicates_keeping_first() {
    // Оригинал: seen/out — повтор выбрасывается, первое вхождение остаётся.
    let multi = vec!["/usr/lib/rexon/qml_modules".to_string()];
    let dirs = qm::module_dirs(None, HOME, "/usr/lib", &multi);
    assert_eq!(
        dirs.iter().filter(|d| *d == "/usr/lib/rexon/qml_modules").count(),
        1,
        "повторов нет"
    );
    assert_eq!(dirs[1], "/usr/lib/rexon/qml_modules", "первое вхождение на месте");
}

#[test]
fn pochinka_xdg_data_home_respected() {
    // Починка 3: в оригинале стоял литерал `~/.local/share`, и при
    // нестандартном XDG_DATA_HOME каталог модулей не искался вовсе.
    let dirs = qm::module_dirs(Some("/data/xdg"), HOME, "", &[]);
    assert_eq!(dirs[0], "/data/xdg/rexon/qml_modules");
    assert_eq!(
        dirs[1],
        format!("{HOME}/.local/share/rexon/qml_modules"),
        "старый каталог остаётся кандидатом: build_qml_*.sh пишут туда"
    );
}

#[test]
fn pochinka_xdg_data_home_empty_is_unset() {
    // Пустая переменная = не задана (так же ведёт себя `utils::data_home`).
    assert_eq!(
        qm::module_dirs(Some(""), HOME, "", &[]),
        qm::module_dirs(None, HOME, "", &[])
    );
}

// ============================================================================
// Выбор каталога импорта
// ============================================================================

#[test]
fn resolve_import_dir_picks_dir_with_material() {
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    let mut fs = FakeFs::new(&["/usr/lib64/rexon/qml_modules/Qcm/Material/libqml_materialplugin.so"]);
    assert_eq!(qm::resolve_import_dir(&mut fs, &dirs), "/usr/lib64/rexon/qml_modules");
}

#[test]
fn resolve_import_dir_falls_back_to_first() {
    // Оригинал: ни в одном каталоге плагина нет → _QCM_MODULES_DIRS[0].
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    let mut fs = FakeFs::new(&[]);
    assert_eq!(
        qm::resolve_import_dir(&mut fs, &dirs),
        format!("{HOME}/.local/share/rexon/qml_modules")
    );
}

#[test]
fn resolve_import_dir_prefers_earlier_dir() {
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    let user = format!("{HOME}/.local/share/rexon/qml_modules/Qcm/Material/libqml_materialplugin.so");
    let mut fs = FakeFs::new(&[
        &user,
        "/usr/lib/rexon/qml_modules/Qcm/Material/libqml_materialplugin.so",
    ]);
    assert_eq!(
        qm::resolve_import_dir(&mut fs, &dirs),
        format!("{HOME}/.local/share/rexon/qml_modules"),
        "пользовательская сборка предпочтительнее системной"
    );
}

// ============================================================================
// Готовность плагинов
// ============================================================================

#[test]
fn plugins_ready_needs_both() {
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    let d = format!("{HOME}/.local/share/rexon/qml_modules");
    let material = format!("{d}/Qcm/Material/libqml_materialplugin.so");
    let control = format!("{d}/waywallen/control/libwaywallen-controlplugin.so");

    assert!(!qm::qml_plugins_ready(&mut FakeFs::new(&[]), &dirs), "нет ни одного");
    assert!(
        !qm::qml_plugins_ready(&mut FakeFs::new(&[&material]), &dirs),
        "только Qcm.Material — страница «Обои» не построится"
    );
    assert!(
        !qm::qml_plugins_ready(&mut FakeFs::new(&[&control]), &dirs),
        "только waywallen.control"
    );
    assert!(qm::qml_plugins_ready(&mut FakeFs::new(&[&material, &control]), &dirs));
}

#[test]
fn pochinka_split_install_now_ready() {
    // Починка 2 со стороны готовности: у оригинала оба плагина обязаны были
    // лежать в ОДНОМ каталоге (том, где нашёлся Qcm.Material). Сборка
    // пользователя + пакетный control давали «QML-модули не собраны» при
    // двух живых файлах на диске.
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    let material =
        format!("{HOME}/.local/share/rexon/qml_modules/Qcm/Material/libqml_materialplugin.so");
    let control =
        "/usr/lib/rexon/qml_modules/waywallen/control/libwaywallen-controlplugin.so".to_string();
    assert!(qm::qml_plugins_ready(&mut FakeFs::new(&[&material, &control]), &dirs));
    assert_eq!(
        qm::import_dirs(&mut FakeFs::new(&[&material, &control]), &dirs),
        vec![
            format!("{HOME}/.local/share/rexon/qml_modules"),
            "/usr/lib/rexon/qml_modules".to_string(),
        ],
        "оба каталога уходят в addImportPath, иначе половина типов не импортируется"
    );
}

#[test]
fn import_dirs_empty_when_nothing_built() {
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    assert!(qm::import_dirs(&mut FakeFs::new(&[]), &dirs).is_empty());
}

// ============================================================================
// Кандидаты на Rust-сборки
// ============================================================================

#[test]
fn pochinka_rust_lib_candidates_cover_all_module_dirs() {
    // Починка 1: `qt_main.rust_gui_candidates` знал только ~/.local/share и
    // /usr/lib — на Fedora (/usr/lib64) и Debian (мультиарх) пакетная сборка
    // не находилась. Тот же список теперь у библиотеки модели фильтров.
    let multi = vec!["/usr/lib/x86_64-linux-gnu/rexon/qml_modules".to_string()];
    let dirs = qm::module_dirs(None, HOME, "", &multi);
    let got = qm::filter_lib_candidates(REPO, &dirs);
    assert!(
        got.contains(
            &"/usr/lib64/rexon/qml_modules/waywallen/control/librexon_qt_filter.so".to_string()
        ),
        "Fedora и openSUSE"
    );
    assert!(
        got.contains(
            &"/usr/lib/x86_64-linux-gnu/rexon/qml_modules/waywallen/control/librexon_qt_filter.so"
                .to_string()
        ),
        "Debian и Ubuntu"
    );
}

#[test]
fn pochinka_rust_lib_candidates_independent_of_material() {
    // Починка 2: у оригинала каталог `.so` брался от того, где нашёлся
    // Qcm.Material. Нет Qcm.Material — каталог схлопывался в ~/.local/share,
    // и пакетная /usr/lib/.../*.so не проверялась ВООБЩЕ.
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    let packaged =
        "/usr/lib/rexon/qml_modules/waywallen/control/librexon_qt_filter.so".to_string();
    let mut fs = FakeFs::new(&[&packaged]);
    let got = qm::filter_lib_candidates(REPO, &dirs);
    assert_eq!(qm::pick_existing(&mut fs, &got), Some(packaged));
}

#[test]
fn filter_candidates_order_matches_original_head() {
    // Оригинал: release → debug → каталог(и) модулей.
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    let got = qm::filter_lib_candidates(REPO, &dirs);
    assert_eq!(got[0], format!("{REPO}/rust/target/release/librexon_qt_filter.so"));
    assert_eq!(got[1], format!("{REPO}/rust/target/debug/librexon_qt_filter.so"));
    assert_eq!(
        got[2],
        format!("{HOME}/.local/share/rexon/qml_modules/waywallen/control/librexon_qt_filter.so")
    );
}

#[test]
fn pick_existing_takes_first_match() {
    let cands = vec![
        "/нет/такого".to_string(),
        "/есть/первый".to_string(),
        "/есть/второй".to_string(),
    ];
    let mut fs = FakeFs::new(&["/есть/первый", "/есть/второй"]);
    assert_eq!(qm::pick_existing(&mut fs, &cands), Some("/есть/первый".to_string()));
    assert_eq!(qm::pick_existing(&mut FakeFs::new(&[]), &cands), None);
}

// ============================================================================
// Пути вендоренного QML
// ============================================================================

#[test]
fn vendor_page_path_matches_original() {
    // Оригинал `_page_url`: os.path.join(QML_IMPORT_DIR, "waywallen", "ui",
    // "qml", "page", page_qml).
    assert_eq!(
        qm::vendor_page_path("/пакет/qml_ui", "DiscoverPage.qml"),
        "/пакет/qml_ui/waywallen/ui/qml/page/DiscoverPage.qml"
    );
}

#[test]
fn vendor_page_path_empty_stays_empty() {
    // Оригинал: пустое имя → QUrl() (пустой URL, Loader пуст).
    assert_eq!(qm::vendor_page_path("/пакет/qml_ui", ""), "");
}

#[test]
fn qml_path_for_host_and_prewarm() {
    assert_eq!(
        qm::qml_path("/пакет/qml_ui", qm::PAGE_HOST_QML),
        "/пакет/qml_ui/PageHost.qml"
    );
    assert_eq!(
        qm::qml_path("/пакет/qml_ui", qm::PREWARM_QML),
        "/пакет/qml_ui/_prewarm_control.qml"
    );
    assert_eq!(
        qm::qml_path("/пакет/qml_ui", qm::UPSTREAM_TAG_REL),
        "/пакет/qml_ui/waywallen/ui/UPSTREAM_TAG"
    );
}

#[test]
fn upstream_tag_trims_and_falls_back() {
    // Оригинал: fh.read().strip(); OSError → "?".
    assert_eq!(qm::upstream_tag(Some("v0.3.5\n")), "v0.3.5");
    assert_eq!(qm::upstream_tag(None), "?");
}

#[test]
fn pochinka_upstream_tag_blank_file() {
    // Починка: пустой (или из одних пробелов) файл оригинал отдавал пустой
    // строкой, и она попадала прямо в заголовок окна — «waywallen ».
    assert_eq!(qm::upstream_tag(Some("   \n")), "?");
    assert_eq!(qm::upstream_tag(Some("")), "?");
}

#[test]
fn ui_version_from_upstream_tag() {
    // Строка «Версия %1» на «О программе» (патч 0002): тот же вид, что у
    // версии демона по D-Bus, иначе вторая строка видна всегда.
    assert_eq!(qm::ui_version(Some("v0.5.0 (564eb02)\n")), "0.5.0");
    assert_eq!(qm::ui_version(Some("0.5.0")), "0.5.0");
    assert_eq!(qm::ui_version(Some("v")), "?");
    assert_eq!(qm::ui_version(Some("  ")), "?");
    assert_eq!(qm::ui_version(None), "?");
}

// ============================================================================
// Дисковый кэш миниатюр
// ============================================================================

#[test]
fn net_cache_matches_original() {
    // Оригинал host.py: os.path.expanduser("~/.cache/rexon/qml_net"),
    // 200 * 1024 * 1024.
    assert_eq!(qm::net_cache_dir(None, HOME), format!("{HOME}/.cache/rexon/qml_net"));
    assert_eq!(qm::NET_CACHE_MAX_BYTES, 200 * 1024 * 1024);
}

#[test]
fn pochinka_net_cache_respects_xdg() {
    // Тот же класс, что Починка 3: в оригинале стоял литерал `~/.cache`.
    assert_eq!(
        qm::net_cache_dir(Some("/кэш/xdg"), HOME),
        "/кэш/xdg/rexon/qml_net"
    );
    assert_eq!(
        qm::net_cache_dir(Some(""), HOME),
        format!("{HOME}/.cache/rexon/qml_net"),
        "пустая переменная = не задана"
    );
}

// ============================================================================
// Имена файлов — дословно из оригинала
// ============================================================================

#[test]
fn plugin_names_match_original() {
    assert_eq!(qm::QCM_MATERIAL_REL, "Qcm/Material/libqml_materialplugin.so");
    assert_eq!(
        qm::WAYWALLEN_CONTROL_REL,
        "waywallen/control/libwaywallen-controlplugin.so"
    );
    assert_eq!(qm::FILTER_LIB_NAME, "librexon_qt_filter.so");
    assert_eq!(qm::RUST_LIB_SUBDIR, "waywallen/control");
    assert_eq!(qm::MULTIARCH_GLOB, "/usr/lib/*/rexon/qml_modules");
    assert_eq!(qm::MODULES_REL, "rexon/qml_modules");
    assert_eq!(qm::TARGET_PROFILES, ["release", "debug"]);
}

#[test]
fn plugin_path_normalises_trailing_slash() {
    assert_eq!(
        qm::plugin_path("/каталог/", qm::FILTER_LIB_NAME),
        "/каталог/librexon_qt_filter.so"
    );
}

// ============================================================================
// Версия Qt сборки Qcm.Material (09.10.2026)
// ============================================================================

/// ФС с версиями: у каждого плагина своя версия Qt сборки, у процесса — своя.
struct VersionedFs {
    files: Vec<(String, Option<(u32, u32)>)>,
    running: Option<(u32, u32)>,
}

impl qm::ModuleFs for VersionedFs {
    fn is_file(&mut self, path: &str) -> bool {
        self.files.iter().any(|(p, _)| p == path)
    }
    fn plugin_qt_version(&mut self, path: &str) -> Option<(u32, u32)> {
        self.files.iter().find(|(p, _)| p == path).and_then(|(_, v)| *v)
    }
    fn running_qt_version(&mut self) -> Option<(u32, u32)> {
        self.running
    }
}

#[test]
fn plugin_qt_version_reads_the_qt_metadata_note() {
    // Голова заметки `.note.qt.metadata` настоящего libqml_materialplugin.so,
    // собранного под Qt 6.11 (байты сняты readelf 09.10.2026).
    let mut bytes = vec![0x0c, 0, 0, 0, 0x5d, 0, 0, 0, 0x01, 0, b'Q', b't'];
    bytes.extend_from_slice(b"qt-project!\0");
    bytes.extend_from_slice(&[0x01, 0x06, 0x0b, 0x04, 0xbf]);
    assert_eq!(qm::plugin_qt_version(&bytes), Some((6, 11)));
    assert_eq!(qm::plugin_qt_version(b"no metadata here"), None);
    // Обрезанная заметка — не версия.
    assert_eq!(qm::plugin_qt_version(b"qt-project!\0\x01\x06"), None);
}

#[test]
fn qcm_built_for_other_qt_minor_is_not_used() {
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    let user = format!("{HOME}/.local/share/rexon/qml_modules");
    let sys = "/usr/lib/rexon/qml_modules".to_string();
    let qcm = |d: &str| format!("{d}/Qcm/Material/libqml_materialplugin.so");
    let ctl = |d: &str| format!("{d}/waywallen/control/libwaywallen-controlplugin.so");

    // Устаревшая сборка в каталоге пользователя, годная — в системном.
    let mut fs = VersionedFs {
        files: vec![
            (qcm(&user), Some((6, 11))),
            (ctl(&user), None),
            (qcm(&sys), Some((6, 12))),
            (ctl(&sys), None),
        ],
        running: Some((6, 12)),
    };
    assert!(qm::qml_plugins_ready(&mut fs, &dirs));
    assert_eq!(qm::resolve_import_dir(&mut fs, &dirs), sys);
    // Каталог с негодной сборкой не отдаётся движку вовсе — иначе
    // `import Qcm.Material` нашёл бы её раньше годной.
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![sys.clone()]);
    assert_eq!(
        qm::incompatible_qcm(&mut fs, &dirs),
        Some((qcm(&user), (6, 11), (6, 12)))
    );

    // Годной сборки нет нигде — вкладка не готова (заглушка вместо падения).
    let mut fs = VersionedFs {
        files: vec![(qcm(&sys), Some((6, 11))), (ctl(&sys), None)],
        running: Some((6, 12)),
    };
    assert!(!qm::qml_plugins_ready(&mut fs, &dirs));

    // Версия неизвестна с любой стороны — проверка не мешает.
    let mut fs = VersionedFs {
        files: vec![(qcm(&sys), None), (ctl(&sys), None)],
        running: Some((6, 12)),
    };
    assert!(qm::qml_plugins_ready(&mut fs, &dirs));
    assert!(qm::incompatible_qcm(&mut fs, &dirs).is_none());
}

// ============================================================================
// Протокол сборки waywallen.control (10.10.2026)
// ============================================================================

/// ФС с отметками: перечисленные файлы есть, у части — текст.
struct StampedFs(Vec<(String, Option<String>)>);

impl qm::ModuleFs for StampedFs {
    fn is_file(&mut self, path: &str) -> bool {
        self.0.iter().any(|(p, _)| p == path)
    }
    fn read_text(&mut self, path: &str) -> Option<String> {
        self.0.iter().find(|(p, _)| p == path).and_then(|(_, t)| t.clone())
    }
}

const OLD_STAMP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Каталог с обоими плагинами; `stamp` — отметка протокола (`None` — нет).
fn module_dir(dir: &str, stamp: Option<&str>) -> Vec<(String, Option<String>)> {
    let mut files = vec![
        (qm::plugin_path(dir, qm::QCM_MATERIAL_REL), None),
        (qm::plugin_path(dir, qm::WAYWALLEN_CONTROL_REL), None),
    ];
    if let Some(s) = stamp {
        files.push((qm::plugin_path(dir, qm::CONTROL_STAMP_REL), Some(format!("{s}\n"))));
    }
    files
}

fn user_and_sys() -> (Vec<String>, String, String) {
    (
        qm::module_dirs(None, HOME, "", &[]),
        format!("{HOME}/.local/share/rexon/qml_modules"),
        "/usr/lib/rexon/qml_modules".to_string(),
    )
}

#[test]
fn control_stamp_is_sha256_of_both_protos_as_the_build_script_writes_it() {
    let stamp = qm::control_proto_stamp();
    assert_eq!(stamp.len(), 64);
    assert!(stamp.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));

    // Скрипт сборки модуля пишет ту же сумму в тот же файл: те же два
    // файла в том же порядке. Разойдутся — любой собранный модуль станет
    // «устаревшим» молча.
    let script = include_str!("../../../tools/build_qml_control.sh");
    assert!(script.contains(
        "cat \"$WORK/proto/control.proto\" \"$WORK/proto/filter.proto\" | sha256sum"
    ));
    let name = qm::CONTROL_STAMP_REL.rsplit('/').next().unwrap();
    assert!(script.contains(&format!("\"$DEST/{}\"", qm::CONTROL_STAMP_REL)));
    // Пакет без отметки не собирается.
    let pkgbuild = include_str!("../../../packaging/aur/PKGBUILD");
    assert!(pkgbuild.contains(&format!("[ -s \"$qmod/{}\" ]", qm::CONTROL_STAMP_REL)), "{name}");
}

#[test]
fn fresh_user_and_fresh_package_keep_user_first() {
    let (dirs, user, sys) = user_and_sys();
    let fresh = qm::control_proto_stamp();
    let mut fs = StampedFs([module_dir(&user, Some(fresh)), module_dir(&sys, Some(fresh))].concat());
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![user.clone(), sys.clone()]);
    assert!(qm::stale_control_dirs(&mut fs, &dirs).is_empty());
    assert_eq!(qm::control_build(&mut fs, &user), qm::ControlBuild::Fresh);
}

#[test]
fn stale_user_build_does_not_shadow_fresh_package() {
    let (dirs, user, sys) = user_and_sys();
    let fresh = qm::control_proto_stamp();
    let mut fs = StampedFs([module_dir(&user, Some(OLD_STAMP)), module_dir(&sys, Some(fresh))].concat());
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![sys.clone()]);
    assert_eq!(qm::usable_dirs(&mut fs, &dirs).iter().filter(|d| **d == user).count(), 0);
    assert!(qm::qml_plugins_ready(&mut fs, &dirs));
    assert_eq!(
        qm::stale_control_dirs(&mut fs, &dirs),
        vec![(user.clone(), qm::ControlBuild::Stale)]
    );
    assert_eq!(qm::fresh_control_dir(&mut fs, &dirs), Some(sys.clone()));
    // Rust-сборки рядом с устаревшим модулем тоже не берутся.
    let usable = qm::usable_dirs(&mut fs, &dirs);
    assert!(!qm::filter_lib_candidates("", &usable).iter().any(|p| p.starts_with(&user)));
}

#[test]
fn stale_user_build_alone_stays() {
    let (dirs, user, _) = user_and_sys();
    let mut fs = StampedFs(module_dir(&user, Some(OLD_STAMP)));
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![user.clone()]);
    assert!(qm::qml_plugins_ready(&mut fs, &dirs));
    assert!(qm::stale_control_dirs(&mut fs, &dirs).is_empty());
    assert_eq!(qm::control_build(&mut fs, &user), qm::ControlBuild::Stale);
}

#[test]
fn unmarked_build_yields_only_to_a_fresh_one() {
    let (dirs, user, sys) = user_and_sys();
    let fresh = qm::control_proto_stamp();

    // Без отметки рядом со свежей — уступает (машина со сборкой до 10.10.2026
    // и новым пакетом).
    let mut fs = StampedFs([module_dir(&user, None), module_dir(&sys, Some(fresh))].concat());
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![sys.clone()]);
    assert_eq!(
        qm::stale_control_dirs(&mut fs, &dirs),
        vec![(user.clone(), qm::ControlBuild::Unmarked)]
    );

    // Без отметки один — остаётся.
    let mut fs = StampedFs(module_dir(&user, None));
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![user.clone()]);
    assert!(qm::qml_plugins_ready(&mut fs, &dirs));

    // Без отметки оба (домашняя сборка и пакет старого образца) — порядок
    // прежний, не отброшен никто.
    let mut fs = StampedFs([module_dir(&user, None), module_dir(&sys, None)].concat());
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![user.clone(), sys.clone()]);

    // Без отметки рядом с устаревшей, свежей нет — тоже никто не отброшен.
    let mut fs = StampedFs([module_dir(&user, None), module_dir(&sys, Some(OLD_STAMP))].concat());
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![user.clone(), sys.clone()]);

    // Пустой файл отметки — то же, что её отсутствие; сырой вывод sha256sum
    // и прописные буквы читаются как свежая.
    let stamp_path = qm::plugin_path(&sys, qm::CONTROL_STAMP_REL);
    let ctl = qm::plugin_path(&sys, qm::WAYWALLEN_CONTROL_REL);
    let mut fs = StampedFs(vec![(ctl.clone(), None), (stamp_path.clone(), Some(" \n".into()))]);
    assert_eq!(qm::control_build(&mut fs, &sys), qm::ControlBuild::Unmarked);
    let raw = format!("{}  -\n", fresh.to_uppercase());
    let mut fs = StampedFs(vec![(ctl, None), (stamp_path.clone(), Some(raw))]);
    assert_eq!(qm::control_build(&mut fs, &sys), qm::ControlBuild::Fresh);
    // Отметка без модуля ничего не значит.
    let mut fs = StampedFs(vec![(stamp_path, Some(fresh.into()))]);
    assert_eq!(qm::control_build(&mut fs, &sys), qm::ControlBuild::Missing);
}

#[test]
fn stale_dir_with_the_only_material_is_demoted_not_dropped() {
    // Оформление собрано дома, value-типы свежие — только в пакете: домашний
    // каталог уходит в конец, а не выбрасывается, иначе вкладка осталась бы
    // без `Qcm.Material`.
    let (dirs, user, sys) = user_and_sys();
    let fresh = qm::control_proto_stamp();
    let mut files = module_dir(&user, None);
    files.push((qm::plugin_path(&sys, qm::WAYWALLEN_CONTROL_REL), None));
    files.push((qm::plugin_path(&sys, qm::CONTROL_STAMP_REL), Some(fresh.into())));
    let mut fs = StampedFs(files);
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![sys.clone(), user.clone()]);
    assert!(qm::qml_plugins_ready(&mut fs, &dirs));
    let usable = qm::usable_dirs(&mut fs, &dirs);
    assert_eq!(
        qm::find_plugin(&mut fs, &usable, qm::WAYWALLEN_CONTROL_REL),
        Some(qm::plugin_path(&sys, qm::WAYWALLEN_CONTROL_REL))
    );
}

// ============================================================================
// QExtra: настоящий плагин или QML-подмена (10.10.2026)
// ============================================================================

/// ФС для выбора QExtra: файлы с текстом отметок, версии Qt у плагинов и у
/// процесса.
struct QExtraFs {
    files: Vec<(String, Option<String>)>,
    plugin_qt: Vec<(String, (u32, u32))>,
    running: Option<(u32, u32)>,
}

impl qm::ModuleFs for QExtraFs {
    fn is_file(&mut self, path: &str) -> bool {
        self.files.iter().any(|(p, _)| p == path)
    }
    fn read_text(&mut self, path: &str) -> Option<String> {
        self.files.iter().find(|(p, _)| p == path).and_then(|(_, t)| t.clone())
    }
    fn plugin_qt_version(&mut self, path: &str) -> Option<(u32, u32)> {
        self.plugin_qt.iter().find(|(p, _)| p == path).map(|(_, v)| *v)
    }
    fn running_qt_version(&mut self) -> Option<(u32, u32)> {
        self.running
    }
}

/// Плагин QExtra в каталоге; `stamp` — текст отметки версии Qt.
fn qextra_dir(dir: &str, stamp: Option<&str>) -> Vec<(String, Option<String>)> {
    let mut files = vec![(qm::plugin_path(dir, qm::QEXTRA_PLUGIN_REL), None)];
    if let Some(s) = stamp {
        files.push((qm::plugin_path(dir, qm::QEXTRA_QT_STAMP_REL), Some(s.to_string())));
    }
    files
}

fn qextra_fs(files: Vec<(String, Option<String>)>) -> QExtraFs {
    QExtraFs { files, plugin_qt: Vec::new(), running: Some((6, 12)) }
}

#[test]
fn qextra_names_match_the_build_script() {
    // Те же имена пишет tools/build_qml_qextra.sh — сверка с его текстом.
    let script = include_str!("../../../tools/build_qml_qextra.sh");
    assert_eq!(qm::QEXTRA_PLUGIN_REL, "QExtra/libqextraplugin.so");
    assert_eq!(qm::QEXTRA_QT_STAMP_REL, "QExtra/QT_VERSION");
    assert!(script.contains("$NEW/libqextraplugin.so"));
    assert!(script.contains("> \"$NEW/QT_VERSION\""));
    assert!(script.contains("mv \"$NEW\" \"$DEST/QExtra\""));
    // Модуль плагина зовётся так же, как подмена в каталоге вендора.
    let qmldir = include_str!("../../../tools/qextra_plugin/qmldir");
    assert!(qmldir.starts_with("module QExtra\n"));
    assert!(qmldir.contains("plugin qextraplugin"));
}

#[test]
fn qt_stamp_is_parsed_as_major_minor() {
    assert_eq!(qm::parse_qt_stamp("6.12.0\n"), Some((6, 12)));
    assert_eq!(qm::parse_qt_stamp("6.12"), Some((6, 12)));
    assert_eq!(qm::parse_qt_stamp("  6.9.3  примечание"), Some((6, 9)));
    assert_eq!(qm::parse_qt_stamp(""), None);
    assert_eq!(qm::parse_qt_stamp("6"), None);
    assert_eq!(qm::parse_qt_stamp("шесть.двенадцать"), None);
}

#[test]
fn scene_graph_rhi_by_platform_and_backend() {
    // Платформа сеанса: переменная не задана, Wayland, X11.
    assert!(qm::scene_graph_uses_rhi(None, None));
    assert!(qm::scene_graph_uses_rhi(Some("wayland"), None));
    assert!(qm::scene_graph_uses_rhi(Some("wayland;xcb"), Some("")));
    assert!(qm::scene_graph_uses_rhi(Some("xcb"), Some("rhi")));
    // Без RHI: стенды и эталоны под offscreen, программный граф по просьбе.
    assert!(!qm::scene_graph_uses_rhi(Some("offscreen"), None));
    assert!(!qm::scene_graph_uses_rhi(Some("offscreen:size=1280x720"), None));
    assert!(!qm::scene_graph_uses_rhi(Some("minimal"), None));
    assert!(!qm::scene_graph_uses_rhi(Some("vnc:port=5900"), None));
    assert!(!qm::scene_graph_uses_rhi(Some("linuxfb"), None));
    assert!(!qm::scene_graph_uses_rhi(Some("wayland"), Some("software")));
    assert!(!qm::scene_graph_uses_rhi(None, Some("Software")));
    assert!(!qm::scene_graph_uses_rhi(None, Some("openvg")));
    // Судим по первой платформе списка — её Qt и пробует первой.
    assert!(!qm::scene_graph_uses_rhi(Some("offscreen;wayland"), None));
}

#[test]
fn qpa_platform_argument_overrides_variable() {
    let bin = "rexon-main-window";
    let none: [&str; 0] = [];
    // Ключа нет — действует переменная (или платформа сеанса).
    assert_eq!(qm::requested_qpa_platform(&none, None), None);
    assert_eq!(qm::requested_qpa_platform(&[bin], None), None);
    assert_eq!(
        qm::requested_qpa_platform(&[bin, "--section=wallpaper_engine"], Some("wayland")),
        Some("wayland".to_owned())
    );
    // Ключ перекрывает переменную; два дефиса — тот же ключ.
    assert_eq!(
        qm::requested_qpa_platform(&[bin, "--section=wallpaper_engine", "-platform", "offscreen"], None),
        Some("offscreen".to_owned())
    );
    assert_eq!(
        qm::requested_qpa_platform(&[bin, "--platform", "offscreen:size=1280x720"], Some("wayland")),
        Some("offscreen:size=1280x720".to_owned())
    );
    assert_eq!(
        qm::requested_qpa_platform(&[bin, "-platform", "wayland"], Some("offscreen")),
        Some("wayland".to_owned())
    );
    // Из нескольких ключей действует последний; ключ без значения — никакой.
    assert_eq!(
        qm::requested_qpa_platform(&[bin, "-platform", "wayland", "-platform", "minimal"], None),
        Some("minimal".to_owned())
    );
    assert_eq!(
        qm::requested_qpa_platform(&[bin, "-platform"], Some("xcb")),
        Some("xcb".to_owned())
    );
    // Нулевой аргумент ключом не считается, слитная запись — не ключ Qt.
    assert_eq!(qm::requested_qpa_platform(&["-platform", "offscreen"], None), None);
    assert_eq!(qm::requested_qpa_platform(&[bin, "-platform=offscreen"], None), None);
    // Вместе с признаком RHI: запуск `-platform offscreen` без переменной.
    let asked = qm::requested_qpa_platform(&[bin, "-platform", "offscreen"], None);
    assert!(!qm::scene_graph_uses_rhi(asked.as_deref(), None));
}

#[test]
fn qextra_real_needs_plugin_stamp_and_rhi() {
    let (dirs, user, sys) = user_and_sys();

    // Всё сошлось: плагин, отметка той же младшей версии, RHI.
    let mut fs = qextra_fs(qextra_dir(&user, Some("6.12.0\n")));
    let choice = qm::choose_qextra(&mut fs, &dirs, true);
    assert_eq!(choice, qm::QExtraChoice::Real { dir: user.clone(), qt: (6, 12) });
    let line = qm::qextra_choice_line(&choice);
    assert!(line.contains("настоящий плагин") && line.contains("Qt 6.12"), "{line}");
    assert!(line.contains(&format!("{user}/QExtra/libqextraplugin.so")), "{line}");

    // Тот же каталог, но граф сцены программный — подмена.
    let choice = qm::choose_qextra(&mut fs, &dirs, false);
    assert_eq!(choice, qm::QExtraChoice::Shim(qm::QExtraFallback::NoRhi));
    assert!(qm::qextra_choice_line(&choice).contains("не на RHI"));

    // Плагина нет нигде.
    let mut fs = qextra_fs(module_dir(&sys, None));
    let choice = qm::choose_qextra(&mut fs, &dirs, true);
    assert_eq!(choice, qm::QExtraChoice::Shim(qm::QExtraFallback::NoPlugin));
    assert!(qm::qextra_choice_line(&choice).contains("подмена"));
}

#[test]
fn qextra_with_bad_or_missing_stamp_falls_back() {
    let (dirs, user, _sys) = user_and_sys();
    let path = qm::plugin_path(&user, qm::QEXTRA_PLUGIN_REL);

    // Отметка другой младшей версии.
    let mut fs = qextra_fs(qextra_dir(&user, Some("6.11.2\n")));
    let choice = qm::choose_qextra(&mut fs, &dirs, true);
    assert_eq!(
        choice,
        qm::QExtraChoice::Shim(qm::QExtraFallback::WrongQt {
            path: path.clone(),
            built: (6, 11),
            running: (6, 12),
        })
    );
    let line = qm::qextra_choice_line(&choice);
    assert!(line.contains("Qt 6.11") && line.contains("Qt 6.12") && line.contains(&path), "{line}");

    // Отметки нет; отметка — мусор. В отличие от Qcm.Material неизвестная
    // версия здесь — отказ: есть подмена.
    for stamp in [None, Some("не версия\n"), Some("")] {
        let mut fs = qextra_fs(qextra_dir(&user, stamp));
        assert_eq!(
            qm::choose_qextra(&mut fs, &dirs, true),
            qm::QExtraChoice::Shim(qm::QExtraFallback::NoStamp { path: path.clone() }),
            "отметка {stamp:?}"
        );
    }

    // Версия запущенного Qt неизвестна — сверять не с чем.
    let mut fs = qextra_fs(qextra_dir(&user, Some("6.12.0")));
    fs.running = None;
    assert_eq!(
        qm::choose_qextra(&mut fs, &dirs, true),
        qm::QExtraChoice::Shim(qm::QExtraFallback::RunningQtUnknown { path: path.clone() })
    );

    // Отметка сошлась, а сам плагин собран под другую версию — верим плагину.
    let mut fs = qextra_fs(qextra_dir(&user, Some("6.12.0")));
    fs.plugin_qt = vec![(path.clone(), (6, 11))];
    assert_eq!(
        qm::choose_qextra(&mut fs, &dirs, true),
        qm::QExtraChoice::Shim(qm::QExtraFallback::WrongQt {
            path,
            built: (6, 11),
            running: (6, 12),
        })
    );
}

#[test]
fn qextra_bad_build_does_not_hide_a_good_one_further() {
    let (dirs, user, sys) = user_and_sys();
    // У пользователя — под прежний Qt, в пакете — годный.
    let mut files = qextra_dir(&user, Some("6.11.0"));
    files.extend(qextra_dir(&sys, Some("6.12.0")));
    let mut fs = qextra_fs(files);
    assert_eq!(
        qm::choose_qextra(&mut fs, &dirs, true),
        qm::QExtraChoice::Real { dir: sys.clone(), qt: (6, 12) }
    );
    // Оба годны — первым идёт каталог пользователя.
    let mut files = qextra_dir(&user, Some("6.12.0"));
    files.extend(qextra_dir(&sys, Some("6.12.0")));
    let mut fs = qextra_fs(files);
    assert_eq!(
        qm::choose_qextra(&mut fs, &dirs, true),
        qm::QExtraChoice::Real { dir: user, qt: (6, 12) }
    );
}

#[test]
fn qextra_is_not_taken_from_a_dir_with_stale_control() {
    // Выбранный каталог встаёт в путях импорта первым. Если в нём лежит
    // устаревший waywallen.control (каталог оставлен в хвосте ради
    // Qcm.Material), он снова заслонил бы свежий — такой каталог не берём.
    let (dirs, user, sys) = user_and_sys();
    let mut files = module_dir(&user, Some(OLD_STAMP));
    files.extend(qextra_dir(&user, Some("6.12.0")));
    files.push((qm::plugin_path(&sys, qm::WAYWALLEN_CONTROL_REL), None));
    files.push((
        qm::plugin_path(&sys, qm::CONTROL_STAMP_REL),
        Some(qm::control_proto_stamp().to_string()),
    ));
    let mut fs = qextra_fs(files);
    // Каталог пользователя в ходу остался — ушёл в хвост за свежим.
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![sys.clone(), user.clone()]);
    assert_eq!(
        qm::choose_qextra(&mut fs, &dirs, true),
        qm::QExtraChoice::Shim(qm::QExtraFallback::NoPlugin)
    );
}

// ── tools/build_qml_material.sh: шрифты значков без сети ──────────────

/// Прогон сценария сборки Qcm.Material на подставном клоне: `git` настоящий,
/// `curl` всегда без сети, `cmake` лишь создаёт каталог модуля. Возвращает
/// код выхода и содержимое шрифта в собранном модуле (если дошло до копии).
fn run_material_script(prepared_font: Option<&str>) -> (bool, Option<String>, String) {
    use std::process::Command;
    let root = std::env::temp_dir().join(format!(
        "rexon-qm-fonts-{}-{}",
        std::process::id(),
        prepared_font.is_some()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let (work, bin, dest) = (root.join("src"), root.join("bin"), root.join("dest"));
    std::fs::create_dir_all(work.join("assets")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .arg("-C")
            .arg(&work)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
        String::from_utf8(ok.stdout).unwrap()
    };
    let font = work.join("assets/MaterialSymbolsRounded.test.woff2");
    std::fs::write(&font, "version https://git-lfs.github.com/spec/v1\noid sha256:00\nsize 1\n").unwrap();
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "pin"]);
    let pin = git(&["rev-parse", "HEAD"]).trim().to_string();
    // Указатель origin на сам клон: `fetch origin <пин>` проходит без сети.
    git(&["remote", "add", "origin", work.to_str().unwrap()]);
    if let Some(real) = prepared_font {
        // Так кладёт настоящий шрифт prepare() пакета.
        std::fs::write(&font, real).unwrap();
    }
    let stub = |name: &str, body: &str| {
        use std::os::unix::fs::PermissionsExt;
        let p = bin.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    stub("curl", "exit 6");
    stub(
        "cmake",
        &format!(
            "[ \"$1\" = --build ] || exit 0\nmkdir -p '{w}/build/qml_modules/Qcm/Material/assets'\n\
             cp '{w}'/assets/* '{w}/build/qml_modules/Qcm/Material/assets/'\n\
             touch '{w}/build/libqml_material.so'",
            w = work.display()
        ),
    );
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/build_qml_material.sh");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let out = Command::new("bash")
        .arg(script)
        .arg(&dest)
        .env("PATH", path)
        .env("QML_MATERIAL_SRC", &work)
        .env("QML_MATERIAL_PIN", &pin)
        .output()
        .unwrap();
    let built = std::fs::read_to_string(dest.join("Qcm/Material/assets/MaterialSymbolsRounded.test.woff2")).ok();
    let _ = std::fs::remove_dir_all(&root);
    let log = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), built, log)
}

/// Дефект 30.10.2026 (ВМ, пакет r1893): prepare() пакета клал настоящие
/// шрифты, сценарий делал `checkout -f` на том же пине и возвращал
/// указатели git-lfs; без сети curl молчал, и в пакет уезжали указатели —
/// все значки вкладки обоев пустыми квадратами.
#[test]
fn material_script_keeps_prepared_fonts_offline() {
    let (ok, built, log) = run_material_script(Some("REAL-FONT"));
    assert!(ok, "сборка без сети с готовыми шрифтами проходит:\n{log}");
    assert_eq!(built.as_deref(), Some("REAL-FONT"), "в модуль уходит шрифт, а не указатель");
}

#[test]
fn material_script_fails_instead_of_shipping_lfs_pointers() {
    let (ok, built, log) = run_material_script(None);
    assert!(!ok, "указатель вместо шрифта и нет сети — сборка проваливается:\n{log}");
    assert!(log.contains("указатель git-lfs"), "провал — на проверке шрифтов, а не где-то ещё:\n{log}");
    assert!(built.is_none(), "модуль с указателями не собирается: {built:?}");
}
