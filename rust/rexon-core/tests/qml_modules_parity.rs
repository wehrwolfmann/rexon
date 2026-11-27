//! Check of [`rexon_core::qml_modules`] against the Python original.
//!
//! The original is `rexon/qt_wallpaper_engine/qml_ui/__init__.py`
//! (`_module_dirs`, `_resolve_qcm_dir`, `qcm_material_available`,
//! `rexon_gui_candidates`, `upstream_tag`), `qml_ui/bridge.py`
//! (`_rust_filter_model_candidates`), `qml_ui/host.py` (`NET_CACHE_*`,
//! `_page_url`) and `rexon/qt_main/__init__.py` (`rust_gui_candidates`).
//!
//! The values are copied from the original verbatim. Where the answer deliberately
//! DIFFERS, the check is named `pochinka_` ("fix") and says how exactly.

use rexon_core::qml_modules as qm;

/// A fake FS: exactly the listed paths count as usable.
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
// Module directories
// ============================================================================

#[test]
fn module_dirs_default_layout() {
    // The original without XDG_DATA_DIRS and without multiarch directories:
    //   ~/.local/share/rexon/qml_modules, /usr/lib/..., /usr/lib64/...
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    assert_eq!(
        dirs,
        vec![
            format!("{HOME}/.local/share/rexon/qml_modules"),
            "/usr/lib/rexon/qml_modules".to_string(),
            "/usr/lib64/rexon/qml_modules".to_string(),
            // Fix of 15.09.2026: system install (the package layout).
            "/usr/local/lib/rexon/qml_modules".to_string(),
            // Fix of 15.09.2026: install into the home folder.
            format!("{HOME}/.local/lib/rexon/qml_modules"),
        ],
        "порядок каталогов оригинала"
    );
}

#[test]
fn module_dirs_xdg_data_dirs_inserted_between() {
    // The original: user → XDG_DATA_DIRS → /usr/lib → /usr/lib64.
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
    // The original: `if base.strip()` — empty pieces "a::b" and a trailing ":" are skipped.
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
    // The original: seen/out — a repeat is thrown out, the first occurrence stays.
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
    // Fix 3: the original had the literal `~/.local/share`, and with
    // a non-standard XDG_DATA_HOME the module directory was not searched at all.
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
    // An empty variable = not set (`utils::data_home` behaves the same way).
    assert_eq!(
        qm::module_dirs(Some(""), HOME, "", &[]),
        qm::module_dirs(None, HOME, "", &[])
    );
}

// ============================================================================
// Choosing the import directory
// ============================================================================

#[test]
fn resolve_import_dir_picks_dir_with_material() {
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    let mut fs = FakeFs::new(&["/usr/lib64/rexon/qml_modules/Qcm/Material/libqml_materialplugin.so"]);
    assert_eq!(qm::resolve_import_dir(&mut fs, &dirs), "/usr/lib64/rexon/qml_modules");
}

#[test]
fn resolve_import_dir_falls_back_to_first() {
    // The original: no plugin in any directory → _QCM_MODULES_DIRS[0].
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
// Plugin readiness
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
    // Fix 2 on the readiness side: in the original both plugins had to
    // lie in ONE directory (the one where Qcm.Material was found). A user
    // build + the packaged control gave «QML modules are not built» with
    // two live files on disk.
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
// Candidates for the Rust builds
// ============================================================================

#[test]
fn pochinka_rust_lib_candidates_cover_all_module_dirs() {
    // Fix 1: `qt_main.rust_gui_candidates` knew only ~/.local/share and
    // /usr/lib — on Fedora (/usr/lib64) and Debian (multiarch) the packaged build
    // was not found. The filter model library now has the same list.
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
    // Fix 2: in the original the `.so` directory was taken from where
    // Qcm.Material was found. No Qcm.Material — the directory collapsed to ~/.local/share,
    // and the packaged /usr/lib/.../*.so was not checked AT ALL.
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    let packaged =
        "/usr/lib/rexon/qml_modules/waywallen/control/librexon_qt_filter.so".to_string();
    let mut fs = FakeFs::new(&[&packaged]);
    let got = qm::filter_lib_candidates(REPO, &dirs);
    assert_eq!(qm::pick_existing(&mut fs, &got), Some(packaged));
}

#[test]
fn filter_candidates_order_matches_original_head() {
    // The original: release → debug → module directory(ies).
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
// Paths of the vendored QML
// ============================================================================

#[test]
fn vendor_page_path_matches_original() {
    // The original `_page_url`: os.path.join(QML_IMPORT_DIR, "waywallen", "ui",
    // "qml", "page", page_qml).
    assert_eq!(
        qm::vendor_page_path("/пакет/qml_ui", "DiscoverPage.qml"),
        "/пакет/qml_ui/waywallen/ui/qml/page/DiscoverPage.qml"
    );
}

#[test]
fn vendor_page_path_empty_stays_empty() {
    // The original: an empty name → QUrl() (an empty URL, the Loader is empty).
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
    // The original: fh.read().strip(); OSError → "?".
    assert_eq!(qm::upstream_tag(Some("v0.3.5\n")), "v0.3.5");
    assert_eq!(qm::upstream_tag(None), "?");
}

#[test]
fn pochinka_upstream_tag_blank_file() {
    // Fix: the original returned an empty (or whitespace-only) file as an empty
    // string, and it went straight into the window title — «waywallen ».
    assert_eq!(qm::upstream_tag(Some("   \n")), "?");
    assert_eq!(qm::upstream_tag(Some("")), "?");
}

#[test]
fn ui_version_from_upstream_tag() {
    // The «Version %1» line on «About» (patch 0002): the same form as the
    // daemon version over D-Bus, otherwise the second line is always visible.
    assert_eq!(qm::ui_version(Some("v0.5.0 (564eb02)\n")), "0.5.0");
    assert_eq!(qm::ui_version(Some("0.5.0")), "0.5.0");
    assert_eq!(qm::ui_version(Some("v")), "?");
    assert_eq!(qm::ui_version(Some("  ")), "?");
    assert_eq!(qm::ui_version(None), "?");
}

// ============================================================================
// Disk cache of thumbnails
// ============================================================================

#[test]
fn net_cache_matches_original() {
    // The original host.py: os.path.expanduser("~/.cache/rexon/qml_net"),
    // 200 * 1024 * 1024.
    assert_eq!(qm::net_cache_dir(None, HOME), format!("{HOME}/.cache/rexon/qml_net"));
    assert_eq!(qm::NET_CACHE_MAX_BYTES, 200 * 1024 * 1024);
}

#[test]
fn pochinka_net_cache_respects_xdg() {
    // The same class as Fix 3: the original had the literal `~/.cache`.
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
// File names — verbatim from the original
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
// Qt version of the Qcm.Material build (09.10.2026)
// ============================================================================

/// An FS with versions: each plugin has its own build Qt version, the process has its own.
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
    // The head of the `.note.qt.metadata` note of a real libqml_materialplugin.so,
    // built for Qt 6.11 (bytes taken with readelf on 09.10.2026).
    let mut bytes = vec![0x0c, 0, 0, 0, 0x5d, 0, 0, 0, 0x01, 0, b'Q', b't'];
    bytes.extend_from_slice(b"qt-project!\0");
    bytes.extend_from_slice(&[0x01, 0x06, 0x0b, 0x04, 0xbf]);
    assert_eq!(qm::plugin_qt_version(&bytes), Some((6, 11)));
    assert_eq!(qm::plugin_qt_version(b"no metadata here"), None);
    // A truncated note is not a version.
    assert_eq!(qm::plugin_qt_version(b"qt-project!\0\x01\x06"), None);
}

#[test]
fn qcm_built_for_other_qt_minor_is_not_used() {
    let dirs = qm::module_dirs(None, HOME, "", &[]);
    let user = format!("{HOME}/.local/share/rexon/qml_modules");
    let sys = "/usr/lib/rexon/qml_modules".to_string();
    let qcm = |d: &str| format!("{d}/Qcm/Material/libqml_materialplugin.so");
    let ctl = |d: &str| format!("{d}/waywallen/control/libwaywallen-controlplugin.so");

    // An outdated build in the user directory, a usable one in the system one.
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
    // A directory with an unusable build is not given to the engine at all — otherwise
    // `import Qcm.Material` would find it before the usable one.
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![sys.clone()]);
    assert_eq!(
        qm::incompatible_qcm(&mut fs, &dirs),
        Some((qcm(&user), (6, 11), (6, 12)))
    );

    // No usable build anywhere — the tab is not ready (a placeholder instead of a crash).
    let mut fs = VersionedFs {
        files: vec![(qcm(&sys), Some((6, 11))), (ctl(&sys), None)],
        running: Some((6, 12)),
    };
    assert!(!qm::qml_plugins_ready(&mut fs, &dirs));

    // The version is unknown on either side — the check does not get in the way.
    let mut fs = VersionedFs {
        files: vec![(qcm(&sys), None), (ctl(&sys), None)],
        running: Some((6, 12)),
    };
    assert!(qm::qml_plugins_ready(&mut fs, &dirs));
    assert!(qm::incompatible_qcm(&mut fs, &dirs).is_none());
}

// ============================================================================
// Build protocol of waywallen.control (10.10.2026)
// ============================================================================

/// An FS with stamps: the listed files exist, some of them have text.
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

/// A directory with both plugins; `stamp` is the protocol stamp (`None` — none).
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

    // The module build script writes the same sum into the same file: the same two
    // files in the same order. If they diverge, any built module will silently
    // become "outdated".
    let script = include_str!("../../../tools/build_qml_control.sh");
    assert!(script.contains(
        "cat \"$WORK/proto/control.proto\" \"$WORK/proto/filter.proto\" | sha256sum"
    ));
    let name = qm::CONTROL_STAMP_REL.rsplit('/').next().unwrap();
    assert!(script.contains(&format!("\"$DEST/{}\"", qm::CONTROL_STAMP_REL)));
    // A package without a stamp is not built.
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
    // Rust builds next to an outdated module are not taken either.
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

    // Without a stamp next to a fresh one — it yields (a machine with a build from before 10.10.2026
    // and a new package).
    let mut fs = StampedFs([module_dir(&user, None), module_dir(&sys, Some(fresh))].concat());
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![sys.clone()]);
    assert_eq!(
        qm::stale_control_dirs(&mut fs, &dirs),
        vec![(user.clone(), qm::ControlBuild::Unmarked)]
    );

    // One without a stamp — it stays.
    let mut fs = StampedFs(module_dir(&user, None));
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![user.clone()]);
    assert!(qm::qml_plugins_ready(&mut fs, &dirs));

    // Both without a stamp (a home build and an old-style package) — the order
    // is as before, nobody is discarded.
    let mut fs = StampedFs([module_dir(&user, None), module_dir(&sys, None)].concat());
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![user.clone(), sys.clone()]);

    // Without a stamp next to an outdated one, no fresh one — nobody is discarded either.
    let mut fs = StampedFs([module_dir(&user, None), module_dir(&sys, Some(OLD_STAMP))].concat());
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![user.clone(), sys.clone()]);

    // An empty stamp file is the same as no stamp; the raw output of sha256sum
    // and upper-case letters read as fresh.
    let stamp_path = qm::plugin_path(&sys, qm::CONTROL_STAMP_REL);
    let ctl = qm::plugin_path(&sys, qm::WAYWALLEN_CONTROL_REL);
    let mut fs = StampedFs(vec![(ctl.clone(), None), (stamp_path.clone(), Some(" \n".into()))]);
    assert_eq!(qm::control_build(&mut fs, &sys), qm::ControlBuild::Unmarked);
    let raw = format!("{}  -\n", fresh.to_uppercase());
    let mut fs = StampedFs(vec![(ctl, None), (stamp_path.clone(), Some(raw))]);
    assert_eq!(qm::control_build(&mut fs, &sys), qm::ControlBuild::Fresh);
    // A stamp without a module means nothing.
    let mut fs = StampedFs(vec![(stamp_path, Some(fresh.into()))]);
    assert_eq!(qm::control_build(&mut fs, &sys), qm::ControlBuild::Missing);
}

#[test]
fn stale_dir_with_the_only_material_is_demoted_not_dropped() {
    // The look is built at home, fresh value types are only in the package: the home
    // directory goes to the end rather than being thrown out, otherwise the tab would be left
    // without `Qcm.Material`.
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
// QExtra: the real plugin or the QML substitute (10.10.2026)
// ============================================================================

/// An FS for choosing QExtra: files with stamp texts, Qt versions of the plugins and of
/// the process.
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

/// The QExtra plugin in a directory; `stamp` is the text of the Qt version stamp.
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
    // tools/build_qml_qextra.sh writes the same names — compared against its text.
    let script = include_str!("../../../tools/build_qml_qextra.sh");
    assert_eq!(qm::QEXTRA_PLUGIN_REL, "QExtra/libqextraplugin.so");
    assert_eq!(qm::QEXTRA_QT_STAMP_REL, "QExtra/QT_VERSION");
    assert!(script.contains("$NEW/libqextraplugin.so"));
    assert!(script.contains("> \"$NEW/QT_VERSION\""));
    assert!(script.contains("mv \"$NEW\" \"$DEST/QExtra\""));
    // The plugin's module is named the same as the substitute in the vendor directory.
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
    // Session platform: the variable is not set, Wayland, X11.
    assert!(qm::scene_graph_uses_rhi(None, None));
    assert!(qm::scene_graph_uses_rhi(Some("wayland"), None));
    assert!(qm::scene_graph_uses_rhi(Some("wayland;xcb"), Some("")));
    assert!(qm::scene_graph_uses_rhi(Some("xcb"), Some("rhi")));
    // Without RHI: benches and goldens under offscreen, the software graph on request.
    assert!(!qm::scene_graph_uses_rhi(Some("offscreen"), None));
    assert!(!qm::scene_graph_uses_rhi(Some("offscreen:size=1280x720"), None));
    assert!(!qm::scene_graph_uses_rhi(Some("minimal"), None));
    assert!(!qm::scene_graph_uses_rhi(Some("vnc:port=5900"), None));
    assert!(!qm::scene_graph_uses_rhi(Some("linuxfb"), None));
    assert!(!qm::scene_graph_uses_rhi(Some("wayland"), Some("software")));
    assert!(!qm::scene_graph_uses_rhi(None, Some("Software")));
    assert!(!qm::scene_graph_uses_rhi(None, Some("openvg")));
    // We judge by the first platform in the list — Qt tries it first.
    assert!(!qm::scene_graph_uses_rhi(Some("offscreen;wayland"), None));
}

#[test]
fn qpa_platform_argument_overrides_variable() {
    let bin = "rexon-main-window";
    let none: [&str; 0] = [];
    // No switch — the variable applies (or the session platform).
    assert_eq!(qm::requested_qpa_platform(&none, None), None);
    assert_eq!(qm::requested_qpa_platform(&[bin], None), None);
    assert_eq!(
        qm::requested_qpa_platform(&[bin, "--section=wallpaper_engine"], Some("wayland")),
        Some("wayland".to_owned())
    );
    // The switch overrides the variable; two hyphens are the same switch.
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
    // Of several switches the last one applies; a switch without a value — none.
    assert_eq!(
        qm::requested_qpa_platform(&[bin, "-platform", "wayland", "-platform", "minimal"], None),
        Some("minimal".to_owned())
    );
    assert_eq!(
        qm::requested_qpa_platform(&[bin, "-platform"], Some("xcb")),
        Some("xcb".to_owned())
    );
    // The zeroth argument does not count as a switch, the joined form is not a Qt switch.
    assert_eq!(qm::requested_qpa_platform(&["-platform", "offscreen"], None), None);
    assert_eq!(qm::requested_qpa_platform(&[bin, "-platform=offscreen"], None), None);
    // Together with the RHI flag: a `-platform offscreen` launch without the variable.
    let asked = qm::requested_qpa_platform(&[bin, "-platform", "offscreen"], None);
    assert!(!qm::scene_graph_uses_rhi(asked.as_deref(), None));
}

#[test]
fn qextra_real_needs_plugin_stamp_and_rhi() {
    let (dirs, user, sys) = user_and_sys();

    // Everything matched: the plugin, a stamp of the same minor version, RHI.
    let mut fs = qextra_fs(qextra_dir(&user, Some("6.12.0\n")));
    let choice = qm::choose_qextra(&mut fs, &dirs, true);
    assert_eq!(choice, qm::QExtraChoice::Real { dir: user.clone(), qt: (6, 12) });
    let line = qm::qextra_choice_line(&choice);
    assert!(line.contains("настоящий плагин") && line.contains("Qt 6.12"), "{line}");
    assert!(line.contains(&format!("{user}/QExtra/libqextraplugin.so")), "{line}");

    // The same directory, but the scene graph is software — the substitute.
    let choice = qm::choose_qextra(&mut fs, &dirs, false);
    assert_eq!(choice, qm::QExtraChoice::Shim(qm::QExtraFallback::NoRhi));
    assert!(qm::qextra_choice_line(&choice).contains("не на RHI"));

    // No plugin anywhere.
    let mut fs = qextra_fs(module_dir(&sys, None));
    let choice = qm::choose_qextra(&mut fs, &dirs, true);
    assert_eq!(choice, qm::QExtraChoice::Shim(qm::QExtraFallback::NoPlugin));
    assert!(qm::qextra_choice_line(&choice).contains("подмена"));
}

#[test]
fn qextra_with_bad_or_missing_stamp_falls_back() {
    let (dirs, user, _sys) = user_and_sys();
    let path = qm::plugin_path(&user, qm::QEXTRA_PLUGIN_REL);

    // A stamp of another minor version.
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

    // No stamp; the stamp is garbage. Unlike Qcm.Material, an unknown
    // version here is a refusal: there is a substitute.
    for stamp in [None, Some("не версия\n"), Some("")] {
        let mut fs = qextra_fs(qextra_dir(&user, stamp));
        assert_eq!(
            qm::choose_qextra(&mut fs, &dirs, true),
            qm::QExtraChoice::Shim(qm::QExtraFallback::NoStamp { path: path.clone() }),
            "отметка {stamp:?}"
        );
    }

    // The version of the running Qt is unknown — nothing to compare with.
    let mut fs = qextra_fs(qextra_dir(&user, Some("6.12.0")));
    fs.running = None;
    assert_eq!(
        qm::choose_qextra(&mut fs, &dirs, true),
        qm::QExtraChoice::Shim(qm::QExtraFallback::RunningQtUnknown { path: path.clone() })
    );

    // The stamp matched, but the plugin itself is built for another version — we trust the plugin.
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
    // The user's is for the previous Qt, the package's is usable.
    let mut files = qextra_dir(&user, Some("6.11.0"));
    files.extend(qextra_dir(&sys, Some("6.12.0")));
    let mut fs = qextra_fs(files);
    assert_eq!(
        qm::choose_qextra(&mut fs, &dirs, true),
        qm::QExtraChoice::Real { dir: sys.clone(), qt: (6, 12) }
    );
    // Both are usable — the user directory comes first.
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
    // The chosen directory goes first in the import paths. If it holds an
    // outdated waywallen.control (the directory was left at the tail for the sake of
    // Qcm.Material), it would again shadow the fresh one — we do not take such a directory.
    let (dirs, user, sys) = user_and_sys();
    let mut files = module_dir(&user, Some(OLD_STAMP));
    files.extend(qextra_dir(&user, Some("6.12.0")));
    files.push((qm::plugin_path(&sys, qm::WAYWALLEN_CONTROL_REL), None));
    files.push((
        qm::plugin_path(&sys, qm::CONTROL_STAMP_REL),
        Some(qm::control_proto_stamp().to_string()),
    ));
    let mut fs = qextra_fs(files);
    // The user directory stayed in use — it went to the tail behind the fresh one.
    assert_eq!(qm::import_dirs(&mut fs, &dirs), vec![sys.clone(), user.clone()]);
    assert_eq!(
        qm::choose_qextra(&mut fs, &dirs, true),
        qm::QExtraChoice::Shim(qm::QExtraFallback::NoPlugin)
    );
}

// ── tools/build_qml_material.sh: icon fonts without the network ──────

/// A run of the Qcm.Material build script on a fake clone: `git` is real,
/// `curl` is always without the network, `cmake` only creates the module directory. Returns
/// the exit code and the font content in the built module (if it got as far as copying).
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
    // The origin pointer to the clone itself: `fetch origin <pin>` works without the network.
    git(&["remote", "add", "origin", work.to_str().unwrap()]);
    if let Some(real) = prepared_font {
        // This is how the package's prepare() puts the real font.
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

/// Defect of 30.10.2026 (VM, package r1893): the package's prepare() put the real
/// fonts, the script did `checkout -f` on the same pin and brought back
/// the git-lfs pointers; without the network curl was silent, and the pointers went into the package —
/// all icons of the wallpaper tab became empty squares.
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

// ============================================================================
// User build versus packaged build (02.11.2026)
// ============================================================================

/// An FS with stamp texts and Qt versions of the plugins.
struct PkgFs {
    files: Vec<(String, Option<String>)>,
    qt: Vec<(String, (u32, u32))>,
}

impl qm::ModuleFs for PkgFs {
    fn is_file(&mut self, path: &str) -> bool {
        self.files.iter().any(|(p, _)| p == path)
    }
    fn read_text(&mut self, path: &str) -> Option<String> {
        self.files.iter().find(|(p, _)| p == path).and_then(|(_, t)| t.clone())
    }
    fn plugin_qt_version(&mut self, path: &str) -> Option<(u32, u32)> {
        self.qt.iter().find(|(p, _)| p == path).map(|(_, v)| *v)
    }
    fn running_qt_version(&mut self) -> Option<(u32, u32)> {
        Some((6, 12))
    }
}

/// All three modules with stamps in a directory: revision, protocol, Qt.
fn full_dir(fs: &mut PkgFs, dir: &str, pin: &str, qt: (u32, u32)) {
    let qt_text = format!("{}.{}.0\n", qt.0, qt.1);
    for (rel, text) in [
        (qm::QCM_MATERIAL_REL, None),
        (qm::QCM_MATERIAL_PIN_REL, Some(format!("{pin}\n"))),
        (qm::WAYWALLEN_CONTROL_REL, None),
        (qm::CONTROL_STAMP_REL, Some(qm::control_proto_stamp().to_string())),
        (qm::QEXTRA_PLUGIN_REL, None),
        (qm::QEXTRA_PIN_REL, Some(format!("{pin}\n"))),
        (qm::QEXTRA_QT_STAMP_REL, Some(qt_text)),
    ] {
        fs.files.push((qm::plugin_path(dir, rel), text));
    }
    for rel in [qm::QCM_MATERIAL_REL, qm::WAYWALLEN_CONTROL_REL, qm::QEXTRA_PLUGIN_REL] {
        fs.qt.push((qm::plugin_path(dir, rel), qt));
    }
}

fn layout() -> (Vec<String>, Vec<String>, String, String) {
    let (dirs, user, sys) = user_and_sys();
    (dirs, qm::user_module_dirs(None, HOME), user, sys)
}

#[test]
fn user_modules_first_only_in_dev_tree_without_package_flag() {
    assert!(qm::user_modules_first(false, true));
    assert!(!qm::user_modules_first(true, true));
    assert!(!qm::user_modules_first(false, false));
    assert!(!qm::user_modules_first(true, false));
    // $XDG_DATA_HOME and ~/.local/share — both are user ones, without repeats.
    assert_eq!(
        qm::user_module_dirs(Some("/data/xdg"), HOME),
        vec!["/data/xdg/rexon/qml_modules".to_string(), format!("{HOME}/.local/share/rexon/qml_modules")]
    );
    assert_eq!(qm::user_module_dirs(None, HOME).len(), 1);
}

#[test]
fn package_copy_wins_over_same_user_build() {
    let (dirs, user_dirs, user, sys) = layout();
    let mut fs = PkgFs { files: vec![], qt: vec![] };
    full_dir(&mut fs, &user, "aaaa", (6, 12));
    full_dir(&mut fs, &sys, "aaaa", (6, 12));
    let order = qm::order_module_dirs(&mut fs, &dirs, &user_dirs, false);
    assert_eq!(order.dirs.first(), Some(&sys));
    assert!(!order.dirs.contains(&user), "пользовательский каталог не нужен: {:?}", order.dirs);
    assert_eq!(order.notes.len(), 1);
    assert_eq!(order.notes[0].used, qm::UserCopyUse::Skipped);
    assert!(order.notes[0].diffs.is_empty());
    let (warn, line) = qm::user_copy_note_line(&order.notes[0]).unwrap();
    assert!(!warn);
    assert!(line.contains(&sys) && line.contains(&user), "{line}");
    // Downstream: QExtra and the import paths come from the package.
    let rhi = true;
    match qm::choose_qextra(&mut fs, &order.dirs, rhi) {
        qm::QExtraChoice::Real { dir, .. } => assert_eq!(dir, sys),
        other => panic!("ждали настоящий плагин из пакета: {other:?}"),
    }
}

#[test]
fn dev_tree_keeps_user_build_first() {
    let (dirs, user_dirs, user, sys) = layout();
    let mut fs = PkgFs { files: vec![], qt: vec![] };
    full_dir(&mut fs, &user, "aaaa", (6, 12));
    full_dir(&mut fs, &sys, "aaaa", (6, 12));
    let order = qm::order_module_dirs(&mut fs, &dirs, &user_dirs, true);
    assert_eq!(order.dirs, dirs, "у разработки порядок прежний");
    assert_eq!(order.notes[0].used, qm::UserCopyUse::DevTree);
    assert_eq!(qm::user_copy_note_line(&order.notes[0]), None);
}

#[test]
fn user_build_used_when_package_has_no_copy() {
    let (dirs, user_dirs, user, _sys) = layout();
    let mut fs = PkgFs { files: vec![], qt: vec![] };
    full_dir(&mut fs, &user, "aaaa", (6, 12));
    let order = qm::order_module_dirs(&mut fs, &dirs, &user_dirs, false);
    assert!(order.dirs.contains(&user));
    assert_eq!(order.notes[0].used, qm::UserCopyUse::NoPackageCopy);
    assert!(qm::qml_plugins_ready(&mut fs, &order.dirs));
    assert_eq!(qm::import_dirs(&mut fs, &order.dirs), vec![user.clone()]);
}

#[test]
fn outdated_user_build_is_skipped_and_named_in_log() {
    let (dirs, user_dirs, user, sys) = layout();
    let mut fs = PkgFs { files: vec![], qt: vec![] };
    full_dir(&mut fs, &user, "old0", (6, 11));
    full_dir(&mut fs, &sys, "new1", (6, 12));
    let order = qm::order_module_dirs(&mut fs, &dirs, &user_dirs, false);
    assert!(!order.dirs.contains(&user));
    let note = &order.notes[0];
    assert_eq!(note.used, qm::UserCopyUse::Skipped);
    assert_eq!(note.package_dir.as_deref(), Some(sys.as_str()));
    // The difference for each module where there is one: revisions, Qt stamp, build Qt.
    let has = |m: &str, w: &str| note.diffs.iter().any(|d| d.module == m && d.what == w);
    assert!(has("Qcm.Material", "ревизия") && has("Qcm.Material", "Qt сборки"));
    assert!(has("QExtra", "ревизия") && has("QExtra", "отметка Qt"));
    assert!(has("waywallen.control", "Qt сборки") && !has("waywallen.control", "протокол"));
    let (warn, line) = qm::user_copy_note_line(note).unwrap();
    assert!(warn);
    assert!(line.contains("устарела") && line.contains("old0") && line.contains("new1"), "{line}");

    // The same in the source tree: our own build is taken, but the difference is named.
    let order = qm::order_module_dirs(&mut fs, &dirs, &user_dirs, true);
    assert_eq!(order.dirs.first(), Some(&user));
    let (warn, line) = qm::user_copy_note_line(&order.notes[0]).unwrap();
    assert!(warn && line.contains("дерева исходников"), "{line}");
}

#[test]
fn user_only_module_kept_behind_package() {
    // The package has no QExtra, the user has it — the directory stays, but behind.
    let (dirs, user_dirs, user, sys) = layout();
    let mut fs = PkgFs { files: vec![], qt: vec![] };
    full_dir(&mut fs, &user, "aaaa", (6, 12));
    full_dir(&mut fs, &sys, "aaaa", (6, 12));
    fs.files.retain(|(p, _)| !p.starts_with(&format!("{sys}/QExtra/")));
    let order = qm::order_module_dirs(&mut fs, &dirs, &user_dirs, false);
    let pos = |d: &str| order.dirs.iter().position(|x| x == d).unwrap();
    assert!(pos(&sys) < pos(&user));
    assert_eq!(order.notes[0].used, qm::UserCopyUse::OnlyHere(vec!["QExtra"]));
    assert_eq!(qm::import_dirs(&mut fs, &order.dirs).first(), Some(&sys));
}
