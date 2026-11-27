//! Golden check of the `system.rs` port against the original `rexon/system.py`.
//!
//! References were captured from Python itself (the generator imports the live
//! `rexon.system` from the repo root and dumps outputs via
//! `json.dump(..., ensure_ascii=False)`):
//!   * `golden/system_detect_cases.json` — `DistroDetector` on a battery of
//!     os-release fixtures (CachyOS/SteamOS/Astra/ROSA/ALT/NixOS/Ubuntu/
//!     Tumbleweed/Bazzite/Silverblue + quotes/comments/empty NAME/
//!     /usr/lib/os-release fallback/binary detection); `open()` in the oracle
//!     is mocked to the fixture text, `shutil.which` to a list of binaries.
//!   * `golden/system_de_db.json` — `ID_MAP`, the whole `_DE_DATABASE` (9 DEs,
//!     including LXQt), `_DE_META` and a run of
//!     get_packages/is_available/get_aur_packages for each DE × ALL 12
//!     families (+ an unknown DE).
//!   * `golden/system_installer_cases.json` — `_expand_groups` (groups
//!     mate/mate-extra, dedup of a shared member, empty output), `_qi_field`
//!     (version constraints — "gtk3>=…"→"gtk3" by the leftmost separator,
//!     field None, a line without a colon), `_required_by_outside` and
//!     `_filter_removable_transitive` (the docstring example, a transitive
//!     chain, a CYCLE in dependencies, all-safe, non-Arch); `safe_run` in the
//!     oracle is mocked to pacman output fixtures. Separate cases
//!     `russian-locale-*` — pacman output on a Russian locale (the LANG=C
//!     trap): fields are not parsed, Rust behaviour matches Python on the same
//!     fixture.
//!
//! Any divergence is a port bug, not a test bug.

use std::collections::HashSet;

use rexon_core::system::{
    expand_groups, filter_removable_transitive, get_aur_packages, get_check_binary,
    get_config_paths, get_packages, is_available, list_de_ids, parse_group_members,
    polkit_missing_message, qi_field, required_by_outside, DistroDetector, PkgFamily,
    DE_DATABASE, DE_META, ID_MAP,
};
use rexon_core::system_plan::{
    file_owner_queries, foreign_packages_query, gentoo_overlay_for,
    gentoo_overlay_tool_install_cmd, install_argv, install_cmd, package_group_query,
    privileged_pkg_cmd, sync_repo_cmd, FILE_OWNER_BATCH, GENTOO_OVERLAYS,
    GENTOO_OVERLAY_TOOL_PACKAGE, PKG_HELPER_FAMILIES,
};
use serde_json::{json, Map, Value};

mod i18n_pin;

fn detect_fx() -> Value {
    serde_json::from_str(include_str!("golden/system_detect_cases.json")).unwrap()
}

fn db_fx() -> Value {
    serde_json::from_str(include_str!("golden/system_de_db.json")).unwrap()
}

fn installer_fx() -> Value {
    // The helper directory is a live answer of this machine (the golden was
    // captured where the package was absent): `system_plan::retarget_helper_paths`.
    let raw = rexon_core::system_plan::retarget_helper_paths(include_str!(
        "golden/system_installer_cases.json"
    ));
    serde_json::from_str(&raw).unwrap()
}

/// A «command stdout per package» closure from a fixture JSON object
/// (missing key = returncode != 0 = None).
fn output_map(fixture: &Value) -> impl Fn(&str) -> Option<String> + '_ {
    let map = fixture.as_object().expect("объект pkg→stdout");
    move |pkg: &str| map.get(pkg).and_then(Value::as_str).map(String::from)
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

// ============================================================================
// DistroDetector
// ============================================================================

#[test]
fn detect_cases_match_python() {
    let fx = detect_fx();
    let cases = fx["cases"].as_array().expect("cases[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");

    for case in cases {
        let label = case["label"].as_str().unwrap();
        let d = DistroDetector::from_os_release_files(
            case["etc_os_release"].as_str(),
            case["usr_lib_os_release"].as_str(),
        );
        let binaries: HashSet<&str> = case["binaries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_str().unwrap())
            .collect();
        let exp = &case["expected"];

        assert_eq!(
            Value::Object(d.info.clone()),
            exp["info"],
            "{label}: разбор os-release"
        );
        assert_eq!(json!(d.name()), exp["name"], "{label}: name");
        assert_eq!(json!(d.pretty_name()), exp["pretty_name"], "{label}: pretty_name");
        assert_eq!(json!(d.distro_id()), exp["distro_id"], "{label}: distro_id");
        assert_eq!(json!(d.id_like()), exp["id_like"], "{label}: id_like");
        assert_eq!(json!(d.version_id()), exp["version_id"], "{label}: version_id");
        let family = d.family(&|b: &str| binaries.contains(b));
        assert_eq!(json!(family.as_str()), exp["family"], "{label}: family");
        assert_eq!(json!(d.is_steamos()), exp["is_steamos"], "{label}: is_steamos");
        assert_eq!(json!(d.is_immutable()), exp["is_immutable"], "{label}: is_immutable");
    }
}

// ============================================================================
// ID_MAP + PackageDB
// ============================================================================

#[test]
fn id_map_matches_python() {
    let fx = db_fx();
    let mut m = Map::new();
    for (id, fam) in ID_MAP {
        m.insert((*id).to_string(), Value::from(fam.as_str()));
    }
    assert_eq!(Value::Object(m), fx["id_map"], "ID_MAP");
    let order: Vec<&str> = ID_MAP.iter().map(|(k, _)| *k).collect();
    assert_eq!(json!(order), fx["id_map_order"], "порядок ID_MAP");
}

/// Kylin: openKylin is Debian by the table (it has no `ID_LIKE`), Kylin OS —
/// by the machine's package manager (one `ID=kylin` for the Debian and RPM editions).
#[test]
fn kylin_family_by_id_or_binaries() {
    let det = |text: &str| DistroDetector::from_os_release_files(Some(text), None);
    let none = |_: &str| false;
    let openkylin = det("ID=openkylin\nNAME=\"openKylin\"\nVERSION_ID=\"3.0\"\n");
    assert_eq!(openkylin.family(&none), PkgFamily::Debian);

    let kylin = det("ID=kylin\nNAME=\"Kylin\"\nVERSION_ID=\"V10\"\n");
    assert_eq!(kylin.family(&|b: &str| matches!(b, "apt-get" | "dpkg")), PkgFamily::Debian);
    assert_eq!(kylin.family(&|b: &str| matches!(b, "dnf" | "rpm")), PkgFamily::Fedora);
    assert_eq!(kylin.family(&|b: &str| b == "yum"), PkgFamily::Fedora);
    assert_eq!(kylin.family(&none), PkgFamily::Unknown);
}

/// A UKUI session is recognised by `XDG_CURRENT_DESKTOP` and by the session
/// name, on X11 and on Wayland, but the desktop stays «recognised and
/// unsupported»: not tiling, not MATE, without a catalog entry — and with an
/// honest line for the person.
#[test]
fn ukui_session_recognized_but_unsupported() {
    use rexon_core::system::{detect_current_de, unsupported_desktop_name, unsupported_desktop_notice};
    for (xdg, session, wayland) in [
        ("UKUI", "ukui", false),
        ("UKUI", "kylin-wlcom", true),
        ("ubuntukylin:UKUI", "ukui-wayland", true),
        ("", "ukui", false),
        ("", "kylin-wlcom", true),
    ] {
        assert_eq!(detect_current_de(xdg, session, wayland).as_deref(), Some("ukui"), "{xdg:?} / {session:?}");
    }
    assert!(!list_de_ids().contains(&"ukui"), "UKUI пока не в каталоге");
    assert_eq!(rexon_core::de_customization::de_from_id("ukui"), None, "не тайловый и не MATE");
    assert!(!rexon_core::qt_desktops::TILING_IDS.contains(&"ukui"));
    assert_eq!(unsupported_desktop_name("ukui"), Some("UKUI"));
    assert_eq!(unsupported_desktop_name("mate"), None);
    assert_eq!(unsupported_desktop_notice("kde"), None);
    let _lang = i18n_pin::en();
    let notice = unsupported_desktop_notice("ukui").unwrap();
    assert!(notice.contains("UKUI") && notice.contains("doesn't support"), "{notice}");
}

#[test]
fn de_database_matches_python() {
    let fx = db_fx();
    assert_eq!(json!(list_de_ids()), fx["de_ids"], "list_de_ids");

    for (de_id, entry) in DE_DATABASE {
        let want = &fx["db"][*de_id];
        assert!(want.is_object(), "{de_id}: лишний DE в Rust-таблице");

        let mut packages = Map::new();
        for (fam, pkgs) in entry.packages {
            packages.insert(fam.as_str().to_string(), json!(pkgs));
        }
        assert_eq!(Value::Object(packages), want["packages"], "{de_id}: packages");
        let fam_order: Vec<&str> = entry.packages.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(json!(fam_order), want["packages_order"], "{de_id}: порядок семейств");
        assert_eq!(json!(entry.aur), want["aur"], "{de_id}: aur");
        assert_eq!(
            json!(entry.check_package),
            want["check_package"],
            "{de_id}: check_package"
        );
        assert_eq!(
            json!(entry.config_paths),
            want["config_paths"],
            "{de_id}: config_paths"
        );
    }
    assert_eq!(
        DE_DATABASE.len(),
        fx["db"].as_object().unwrap().len(),
        "число DE в базе"
    );
}

/// Desktop metadata for the showcase — as in Python, on the Russian catalog.
///
/// Texts for the person in `DE_META` are English `msgid`s; the reference was
/// captured from the Russian original, so the check goes through `po/ru.po`
/// the same way the interface shows them (`description_text`/`compositor_text`,
/// load — as the `CpuLoad` label).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn de_meta_matches_python_in_russian() {
    let _lang = i18n_pin::ru();
    let fx = db_fx();
    let mut ours = serde_json::to_value(DE_META).unwrap();
    for (entry, m) in ours.as_array_mut().unwrap().iter_mut().zip(DE_META) {
        entry["description"] = json!(m.description_text());
        entry["compositor"] = json!(m.compositor_text());
    }
    assert_eq!(ours, fx["de_meta"], "_DE_META");
}

/// The English desktop showcase — without a single Russian letter.
#[test]
fn de_meta_english_has_no_russian() {
    let _lang = i18n_pin::en();
    let text = serde_json::to_string(DE_META).unwrap();
    assert!(
        !text.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')),
        "русское в английской мете столов: {text}"
    );
    let kde = DE_META.iter().find(|m| m.id == "kde").unwrap();
    assert_eq!(kde.cpu_usage.label(), "Medium");
    assert_eq!(kde.description_text(), "A powerful, customizable desktop with beautiful effects");
}

#[test]
fn db_queries_match_python() {
    let fx = db_fx();
    for q in fx["queries"].as_array().expect("queries[]") {
        let de = q["de"].as_str().unwrap();
        let fam = PkgFamily::from_value(q["family"].as_str().unwrap()).unwrap();
        assert_eq!(
            json!(get_packages(de, fam)),
            q["get_packages"],
            "{de}/{}: get_packages",
            fam.as_str()
        );
        assert_eq!(
            json!(is_available(de, fam)),
            q["is_available"],
            "{de}/{}: is_available",
            fam.as_str()
        );
        assert_eq!(
            json!(get_aur_packages(de, fam)),
            q["get_aur_packages"],
            "{de}/{}: get_aur_packages",
            fam.as_str()
        );
    }
    for (de, want) in fx["check_binary"].as_object().unwrap() {
        assert_eq!(json!(get_check_binary(de)), *want, "{de}: get_check_binary");
    }
    for (de, want) in fx["config_paths"].as_object().unwrap() {
        assert_eq!(json!(get_config_paths(de)), *want, "{de}: get_config_paths");
    }
}

// ============================================================================
// Pure installer algorithms
// ============================================================================

#[test]
fn expand_groups_matches_python() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["expand_groups"].as_array().expect("expand_groups[]") {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let packages = str_vec(&case["packages"]);
        let qg = output_map(&case["qg"]);
        assert_eq!(
            json!(expand_groups(family, &packages, &qg)),
            case["expected"],
            "{label}: expand_groups"
        );
    }
}

/// The engine question «what is in a GROUP» — a command per family.
///
/// Not everyone has groups: Debian has no such concept at all, dnf and zypper
/// call them differently and print another format not verified by hand. Such
/// families must answer `None` — an invented command would silently give
/// empty lists instead of an answer.
#[test]
fn package_group_query_only_where_groups_exist() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["group_query"]["commands"].as_array().expect("commands[]") {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let got = package_group_query(family, case["group"].as_str().unwrap());
        match got {
            None => assert!(
                case["argv"].is_null(),
                "{label}: движок отказался, а голден ждёт команду"
            ),
            Some(q) => {
                assert_eq!(json!(q.cmd.argv), case["argv"], "{label}: argv");
                assert_eq!(json!(q.binary), case["binary"], "{label}: бинарь");
                assert_eq!(json!(q.cmd.lang_c), case["lang_c"], "{label}: LANG=C");
                assert_eq!(json!(q.cmd.timeout_secs), case["timeout"], "{label}: таймаут");
            }
        }
    }
}

/// Parsing the answer: group members, «this is not a group» and garbage in the output.
#[test]
fn parse_group_members_matches_golden() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["group_query"]["members"].as_array().expect("members[]") {
        let label = case["label"].as_str().unwrap();
        let got = parse_group_members(
            case["ok"].as_bool().unwrap(),
            case["stdout"].as_str().unwrap(),
        );
        assert_eq!(json!(got), case["expected"], "{label}: parse_group_members");
    }
}

/// The group contents parsing is ONE for the whole project: `expand_groups`
/// must answer exactly the same as `parse_group_members`, otherwise a copy of
/// the parsing will appear again.
#[test]
fn expand_groups_uses_the_single_parser() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["group_query"]["members"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let stdout = case["stdout"].as_str().unwrap().to_string();
        let ok = case["ok"].as_bool().unwrap();
        let qg = |_pkg: &str| if ok { Some(stdout.clone()) } else { None };
        let asked = vec!["группа".to_string()];
        let want: Vec<String> = match parse_group_members(ok, &stdout) {
            // Order-preserving dedup is already `expand_groups`'s job.
            Some(members) => {
                let mut seen = HashSet::new();
                members.into_iter().filter(|m| seen.insert(m.clone())).collect()
            }
            None => asked.clone(),
        };
        assert_eq!(
            expand_groups(PkgFamily::Arch, &asked, &qg),
            want,
            "{label}: expand_groups разошёлся с общим разбором"
        );
    }
}

/// Families without groups do not pay a single query: `expand_groups` returns
/// the list as is and does not call the callback at all.
#[test]
fn families_without_groups_are_not_asked() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let asked = std::cell::Cell::new(0usize);
    let qg = |_pkg: &str| {
        asked.set(asked.get() + 1);
        Some("группа член\n".to_string())
    };
    let packages = vec!["mate".to_string(), "mate-extra".to_string()];
    for case in fx["group_query"]["commands"].as_array().unwrap() {
        if !case["argv"].is_null() {
            continue;
        }
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        if family == PkgFamily::Arch {
            continue; // invalid name — the family has nothing to do with it
        }
        assert_eq!(
            expand_groups(family, &packages, &qg),
            packages,
            "{}: список должен пройти насквозь",
            case["label"].as_str().unwrap()
        );
    }
    assert_eq!(asked.get(), 0, "у семейства без групп спрашивать нечего");
}

/// Not every family can tell the origin of an installed package («from the
/// repositories or built locally»): Debian has no such concept at all, and
/// `dpkg --print-foreign-architectures` is a false friend, «foreign» there is
/// about architecture. Whoever cannot must answer `None`: an invented command
/// would silently put AUR themes into «System».
#[test]
fn foreign_packages_query_only_where_origin_is_known() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let cases = fx["foreign_query"]["commands"].as_array().expect("commands[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");
    for case in cases {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        match foreign_packages_query(family) {
            None => assert!(
                case["argv"].is_null(),
                "{label}: движок отказался, а голден ждёт команду"
            ),
            Some(q) => {
                assert_eq!(json!(q.cmd.argv), case["argv"], "{label}: argv");
                assert_eq!(json!(q.binary), case["binary"], "{label}: бинарь");
                assert_eq!(json!(q.cmd.lang_c), case["lang_c"], "{label}: LANG=C");
                assert_eq!(json!(q.cmd.timeout_secs), case["timeout"], "{label}: таймаут");
            }
        }
    }
}

/// The file owner question: valid paths go in batches, invalid ones do not go
/// at all, and a family with a different answer format answers `None`. An
/// empty command list and `None` are DIFFERENT: the first is «nothing to
/// ask», the second «nothing to ask with».
#[test]
fn file_owner_queries_only_where_format_is_known() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let cases = fx["file_owner_query"]["commands"].as_array().expect("commands[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");
    for case in cases {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let paths = str_vec(&case["paths"]);
        match file_owner_queries(family, &paths) {
            None => assert!(
                case["batches"].is_null(),
                "{label}: движок отказался, а голден ждёт команды"
            ),
            Some(qs) => {
                let argvs: Vec<Vec<String>> = qs.iter().map(|q| q.cmd.argv.clone()).collect();
                assert_eq!(json!(argvs), case["batches"], "{label}: argv по пачкам");
                for q in &qs {
                    assert_eq!(json!(q.binary), case["binary"], "{label}: бинарь");
                    assert_eq!(json!(q.cmd.lang_c), case["lang_c"], "{label}: LANG=C");
                    assert_eq!(
                        json!(q.cmd.timeout_secs),
                        case["timeout"],
                        "{label}: таймаут"
                    );
                }
            }
        }
    }
}

/// Cutting paths into batches is the engine's job: the executor no longer has
/// its own number. One batch is exactly `FILE_OWNER_BATCH` paths, an extra
/// path starts a second one, and together they give the original list without
/// losses and without reordering.
#[test]
fn file_owner_queries_batch_by_the_engine_limit() {
    let paths: Vec<String> = (0..FILE_OWNER_BATCH + 1)
        .map(|i| format!("/usr/share/themes/T{i}"))
        .collect();
    let qs = file_owner_queries(PkgFamily::Arch, &paths).expect("Arch отвечает");
    assert_eq!(qs.len(), 2, "201 путь — две пачки");
    assert_eq!(qs[0].cmd.argv.len(), 2 + FILE_OWNER_BATCH, "первая пачка полная");
    assert_eq!(qs[1].cmd.argv.len(), 2 + 1, "во второй остаток");
    let seen: Vec<String> = qs
        .iter()
        .flat_map(|q| q.cmd.argv[2..].to_vec())
        .collect();
    assert_eq!(seen, paths, "пути не потерялись и не переставились");
    // Exactly the boundary: there must be no extra empty batch.
    let exact = file_owner_queries(PkgFamily::Arch, &paths[..FILE_OWNER_BATCH]).unwrap();
    assert_eq!(exact.len(), 1, "ровно предел — одна пачка");
}

/// The engine's ninth question is «synchronise ONE repository». A separately
/// attached repository exists only in Gentoo (that is the overlay): pacman
/// and apt update all databases at once, and dnf and zypper do have the
/// syntax, but Rexon does not attach repositories there — there is nowhere to
/// take a name from. Whoever cannot must answer `None`: an invented
/// privileged command is worse than an honest «don't know».
///
/// ⚠️ There is no Gentoo on the build machine: both the command and the
/// refusals are pinned ONLY by goldens, nothing was executed.
#[test]
fn sync_repo_cmd_only_where_repos_are_separate() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let cases = fx["sync_repo"]["commands"].as_array().expect("commands[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");
    for case in cases {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let got = sync_repo_cmd(family, case["repo"].as_str().unwrap());
        match got {
            None => assert!(
                case["argv"].is_null(),
                "{label}: движок отказался, а голден ждёт команду"
            ),
            Some(argv) => assert_eq!(json!(argv), case["argv"], "{label}: argv"),
        }
    }
}

/// The engine takes overlay names from its own table — so each must pass the
/// repository name check and reach the command as the last argument.
/// Otherwise «attached but not synchronised» would happen silently.
#[test]
fn every_known_overlay_can_be_synced() {
    for (de_id, overlay) in GENTOO_OVERLAYS {
        assert_eq!(gentoo_overlay_for(de_id), Some(*overlay), "{de_id}: таблица");
        let argv = sync_repo_cmd(PkgFamily::Gentoo, overlay)
            .unwrap_or_else(|| panic!("{de_id}: overlay '{overlay}' движок не берёт"));
        assert_eq!(argv.last().map(String::as_str), Some(*overlay), "{de_id}: имя");
    }
}

/// The overlay tool install command: the named path via rexon-pkg-helper and
/// the raw fallback. Non-Gentoo answers `None` in any helper state.
#[test]
fn gentoo_overlay_tool_install_cmd_matches_golden() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let cases = fx["gentoo_overlay_tool"]["commands"].as_array().expect("commands[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");
    assert_eq!(
        json!(GENTOO_OVERLAY_TOOL_PACKAGE),
        fx["gentoo_overlay_tool"]["package"],
        "имя пакета инструмента"
    );
    for case in cases {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let avail = case["helper_available"].as_bool().unwrap();
        match gentoo_overlay_tool_install_cmd(family, avail) {
            None => assert!(
                case["argv"].is_null(),
                "{label}: движок отказался, а голден ждёт команду"
            ),
            Some(argv) => assert_eq!(json!(argv), case["argv"], "{label}: argv"),
        }
    }
}

/// There must be no second copy of the install command: the raw fallback must
/// match `pkexec` + [`install_cmd`] letter for letter. This very divergence
/// is where it was caught — the hand-written copy in `ensure_gentoo_overlay`
/// went without `--quiet`, which is in both the engine table and the helper template.
#[test]
fn overlay_tool_raw_path_is_the_engine_install_cmd() {
    let packages = vec![GENTOO_OVERLAY_TOOL_PACKAGE.to_string()];
    let mut want = vec!["pkexec".to_string()];
    want.extend(install_cmd(PkgFamily::Gentoo, &packages).expect("gentoo умеет ставить"));
    assert_eq!(
        gentoo_overlay_tool_install_cmd(PkgFamily::Gentoo, false),
        Some(want),
        "сырой откат разошёлся с install_cmd"
    );
}

/// The engine's only answer to «which command installs these packages».
///
/// The product no longer has its own install tables: both `wallpaper_engine`
/// and `compliance` ask this function. The former copies knew three families
/// out of eleven and had already drifted from the engine — `pacman -S
/// --noconfirm` without `--needed` and `apt install -y` instead of
/// `apt-get install -y --no-install-recommends`.
///
/// ⛔ Not a single command was executed: the reference is written by hand, the
/// check installs nothing.
#[test]
fn install_argv_matches_golden() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let cases = fx["install_argv"]["commands"].as_array().expect("commands[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");
    for case in cases {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let packages = str_vec(&case["packages"]);
        let avail = case["helper_available"].as_bool().unwrap();
        match install_argv(family, &packages, avail) {
            None => assert!(
                case["argv"].is_null(),
                "{label}: движок отказался, а голден ждёт команду"
            ),
            Some(argv) => assert_eq!(json!(argv), case["argv"], "{label}: argv"),
        }
    }
}

/// Guard against a second table: for EVERY family the raw path must match
/// `pkexec` + [`install_cmd`] letter for letter, and the named one
/// [`privileged_pkg_cmd`]. This very divergence was caught twice, so the check
/// goes not over the golden's list but over all twelve enum values: a new
/// family will not slip past it.
#[test]
fn install_argv_has_no_table_of_its_own() {
    // The language is per process, and answers of different calls are compared:
    // without pinning, a neighbouring check switches the language between them.
    let _lang = i18n_pin::en();
    let packages = vec!["mpv".to_string()];
    for family in [
        PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse,
        PkgFamily::Gentoo, PkgFamily::Alpine, PkgFamily::Void, PkgFamily::Nix,
        PkgFamily::Solus, PkgFamily::Clear, PkgFamily::Alt, PkgFamily::Unknown,
    ] {
        let fam = family.as_str();
        let Ok(base) = install_cmd(family, &packages) else {
            // UNKNOWN: no command in any helper state.
            assert_eq!(install_argv(family, &packages, false), None, "{fam}: сырой");
            assert_eq!(install_argv(family, &packages, true), None, "{fam}: именованный");
            continue;
        };
        // NixOS installs into the user profile — pkexec would be a mistake here.
        let want_raw = if family == PkgFamily::Nix {
            base.clone()
        } else {
            let mut v = vec!["pkexec".to_string()];
            v.extend(base.iter().cloned());
            v
        };
        assert_eq!(
            install_argv(family, &packages, false),
            Some(want_raw),
            "{fam}: сырой путь разошёлся с install_cmd"
        );
        let want_named = privileged_pkg_cmd(family, "install", &packages, true)
            .unwrap_or_else(|| install_argv(family, &packages, false).unwrap());
        assert_eq!(
            install_argv(family, &packages, true),
            Some(want_named),
            "{fam}: именованный путь разошёлся с privileged_pkg_cmd"
        );
        // And the helper's own built-in template comes from the same table: a family
        // is either in the helper's list and goes by the named path, or not.
        assert_eq!(
            PKG_HELPER_FAMILIES.contains(&fam),
            privileged_pkg_cmd(family, "install", &packages, true).is_some(),
            "{fam}: список семейств helper'а и его же ответ разошлись"
        );
    }
}

/// «Install nothing» cannot be expressed as a command: `pacman -S --needed
/// --noconfirm` without a single name would return a usage error as root. An
/// empty list is a refusal for any family and any helper state.
#[test]
fn install_argv_refuses_empty_package_list() {
    for family in [
        PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse,
        PkgFamily::Gentoo, PkgFamily::Alpine, PkgFamily::Void, PkgFamily::Nix,
        PkgFamily::Solus, PkgFamily::Clear, PkgFamily::Alt, PkgFamily::Unknown,
    ] {
        for avail in [false, true] {
            assert_eq!(
                install_argv(family, &[], avail),
                None,
                "{}: пустой список дал команду (helper={avail})",
                family.as_str()
            );
        }
    }
}

#[test]
fn qi_field_matches_python() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["qi_field"].as_array().expect("qi_field[]") {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let qi = output_map(&case["qi"]);
        assert_eq!(
            json!(qi_field(
                family,
                case["package"].as_str().unwrap(),
                case["field"].as_str().unwrap(),
                &qi
            )),
            case["expected"],
            "{label}: qi_field"
        );
    }
}

#[test]
fn required_by_outside_matches_python() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["required_by_outside"].as_array().expect("required_by_outside[]") {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let candidate: HashSet<String> = str_vec(&case["candidate"]).into_iter().collect();
        let qi = output_map(&case["qi"]);
        assert_eq!(
            json!(required_by_outside(
                family,
                case["package"].as_str().unwrap(),
                &candidate,
                &qi
            )),
            case["expected"],
            "{label}: required_by_outside"
        );
    }
}

#[test]
fn filter_removable_matches_python() {
    // Language before parsing the reference: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["filter_removable"].as_array().expect("filter_removable[]") {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let packages = str_vec(&case["packages"]);
        let qi = output_map(&case["qi"]);
        let (safe, skipped) = filter_removable_transitive(family, &packages, &qi);
        assert_eq!(json!(safe), case["safe"], "{label}: safe");
        assert_eq!(json!(skipped), case["skipped"], "{label}: skipped");
    }
}

/// Without pkexec everything privileged silently fails — the message about it
/// is one for the whole project and carries the package name for the current distro.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn polkit_missing_message_matches_python() {
    // The reference is the original's Russian texts: checked via po/ru.po.
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let c = &fx["polkit_missing"];
    assert_eq!(
        polkit_missing_message(&rexon_core::utils::tool_package_hint("pkexec", "suse")),
        c["with_hint"].as_str().unwrap()
    );
    assert_eq!(polkit_missing_message(""), c["no_hint"].as_str().unwrap());
}

// ============================================================================
// Which environment is running
// ============================================================================

/// Autodetection must recognise ALL 11 desktops Rexon can install —
/// including the tiling ones it was rewritten for. Formerly three different
/// detectors answered differently, and none of them knew sway/niri.
#[test]
fn detects_every_installable_desktop() {
    use rexon_core::system::{detect_current_de, list_de_ids};

    // How an environment really presents itself: the XDG_CURRENT_DESKTOP value
    // from each desktop's session.
    let sessions: &[(&str, &str)] = &[
        ("kde", "KDE"),
        ("gnome", "GNOME"),
        ("cinnamon-mint", "X-Cinnamon"),
        ("xfce", "XFCE"),
        ("mate", "MATE"),
        ("budgie", "Budgie:GNOME"),
        ("pantheon", "Pantheon"),
        // DDE v23/v25 sets `DDE` (dde-session), deepin 20 — `Deepin`.
        ("deepin", "DDE"),
        ("lxqt", "LXQt"),
        ("cosmic", "COSMIC"),
        ("hyprland", "Hyprland"),
        ("sway", "sway"),
        ("niri", "niri"),
        // river-classic does not set the variable itself — Rexon's startup init
        // sets it; Wayfire sets `wayfire` itself.
        ("river", "river"),
        ("wayfire", "wayfire"),
    ];
    for (de_id, xdg) in sessions {
        assert_eq!(
            detect_current_de(xdg, "", true).as_deref(),
            Some(*de_id),
            "сеанс {xdg} не опознан"
        );
    }
    // Pantheon is recognised even without Wayland, and by the session name (X11
    // «Classic Session» — pantheon, Wayland «Secure Session» — pantheon-wayland).
    assert_eq!(detect_current_de("Pantheon", "pantheon", false).as_deref(), Some("pantheon"));
    assert_eq!(detect_current_de("", "pantheon-wayland", false).as_deref(), Some("pantheon"));
    // Deepin: deepin 20, the Treeland Wayland session and the X11 session name.
    assert_eq!(detect_current_de("Deepin", "", false).as_deref(), Some("deepin"));
    assert_eq!(detect_current_de("Treeland", "treeland", true).as_deref(), Some("deepin"));
    assert_eq!(detect_current_de("", "deepin", false).as_deref(), Some("deepin"));
    // River from the login screen without the startup init: `XDG_CURRENT_DESKTOP`
    // is empty, but the login screen names the session after the `river.desktop` file.
    assert_eq!(detect_current_de("", "river", true).as_deref(), Some("river"));
    // Not a single desktop from the database is forgotten.
    for de_id in list_de_ids() {
        assert!(
            sessions.iter().any(|(id, _)| *id == de_id),
            "{de_id} есть в базе, но не проверен автоопределением"
        );
    }
}

/// Case, compound values and the fallback by session name.
#[test]
fn detect_handles_case_and_session_fallback() {
    use rexon_core::system::detect_current_de;

    assert_eq!(detect_current_de("kde", "", false).as_deref(), Some("kde"));
    assert_eq!(detect_current_de("KDE:plasma", "", false).as_deref(), Some("kde"));
    assert_eq!(detect_current_de("ubuntu:GNOME", "", false).as_deref(), Some("gnome"));
    // Budgie declares itself together with GNOME — Budgie must win.
    assert_eq!(detect_current_de("Budgie:GNOME", "", false).as_deref(), Some("budgie"));
    // The session name as the fallback (on Plasma it is exactly "plasma").
    assert_eq!(detect_current_de("", "plasma", false).as_deref(), Some("kde"));
    assert_eq!(detect_current_de("", "gnome-xorg", false).as_deref(), Some("gnome"));
    assert_eq!(detect_current_de("", "cinnamon", false).as_deref(), Some("cinnamon-mint"));
    // An unfamiliar compositor on Wayland — returned as is, not «don't know».
    // (river no longer fits for this: since 30.09 it is in the shared table.)
    assert_eq!(detect_current_de("hikari", "", true).as_deref(), Some("hikari"));
    assert_eq!(detect_current_de("hikari", "", false), None);
    // river, wayfire and labwc are recognised even without Wayland and by the
    // session name — the wallpaper engine knew them, now the shared table does.
    assert_eq!(detect_current_de("river", "", false).as_deref(), Some("river"));
    assert_eq!(detect_current_de("labwc:wlroots", "", false).as_deref(), Some("labwc"));
    assert_eq!(detect_current_de("", "wayfire", false).as_deref(), Some("wayfire"));
    assert_eq!(detect_current_de("", "", false), None);
}

/// ⛔ Desktop package sets on Arch do not drag in a foreign environment and do
/// not break a neighbouring desktop's login screen.
///
/// Checked on 19.10.2026 via AUR RPC v5 (`info`) and `LC_ALL=C pacman -Si`:
///   * `vala-panel-appmenu` is a shared package, depends on `-budgie` (which
///     depends on all of `budgie-desktop`) and on `-valapanel` (pulls in
///     `vala-panel`); the MATE global menu needs only `-mate` and `-registrar`;
///   * `mint-artwork` declares `conflicts=(lightdm-gtk-greeter
///     lightdm-gtk-greeter-settings)` and itself ships
///     `/etc/lightdm/lightdm-gtk-greeter.conf`, while Rexon installs
///     `lightdm-gtk-greeter` with XFCE and MATE: on such a machine pacman would
///     refuse to install Cinnamon, and without the refusal it would wipe the
///     foreign login screen.
/// All places Rexon installs desktop packages from are guarded: the desktop
/// database, the MATE compliance check and the MATE post-install.
#[test]
fn de_package_sets_do_not_pull_foreign_desktops() {
    use rexon_core::compliance::MATE_AUR_PACKAGES;
    use rexon_core::system_plan::MATE_EXTRA_PACKAGES;

    // Package → what is wrong with it.
    const PULLS_FOREIGN_DESKTOP: &[(&str, &str)] = &[
        ("vala-panel-appmenu", "общий пакет: тянет budgie-desktop и vala-panel"),
        ("vala-panel-appmenu-budgie", "тянет budgie-desktop"),
        ("vala-panel-appmenu-valapanel", "тянет vala-panel"),
    ];
    const CONFLICTS_WITH_GTK_GREETER: &[&str] = &["mint-artwork"];

    let mut sets: Vec<(String, Vec<String>)> = Vec::new();
    for (de_id, entry) in DE_DATABASE {
        let mut all: Vec<String> = get_packages(de_id, PkgFamily::Arch);
        all.extend(entry.aur.iter().map(|p| p.to_string()));
        sets.push((format!("стол {de_id}"), all));
    }
    sets.push((
        "проверка соответствия MATE".to_string(),
        MATE_AUR_PACKAGES.iter().map(|p| p.to_string()).collect(),
    ));
    for (tag, _, pkgs) in MATE_EXTRA_PACKAGES {
        sets.push((
            format!("пост-установка MATE «{tag}»"),
            pkgs.iter().map(|p| p.to_string()).collect(),
        ));
    }

    // The GTK login screen is in at least one set — so a conflict with it is
    // unacceptable in any (the machine may carry both desktops).
    let gtk_greeter_used = sets
        .iter()
        .any(|(_, pkgs)| pkgs.iter().any(|p| p == "lightdm-gtk-greeter"));
    assert!(gtk_greeter_used, "в наборах нет lightdm-gtk-greeter — пересмотреть сторож");

    let mut bad = Vec::new();
    for (where_, pkgs) in &sets {
        for p in pkgs {
            if let Some((_, why)) = PULLS_FOREIGN_DESKTOP.iter().find(|(n, _)| n == p) {
                bad.push(format!("{where_}: {p} — {why}"));
            }
            if CONFLICTS_WITH_GTK_GREETER.contains(&p.as_str()) {
                bad.push(format!("{where_}: {p} — конфликтует с lightdm-gtk-greeter"));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");

    // The MATE global menu — exactly its own part and the menu registrar.
    let mate_aur = get_aur_packages("mate", PkgFamily::Arch);
    for p in ["vala-panel-appmenu-mate", "vala-panel-appmenu-registrar"] {
        assert!(mate_aur.iter().any(|x| x == p), "MATE без {p}: {mate_aur:?}");
    }
    // The Mint look stays: themes and icons are in place.
    let mint_aur = get_aur_packages("cinnamon-mint", PkgFamily::Arch);
    for p in ["mint-themes", "mint-y-icons", "mint-x-icons"] {
        assert!(mint_aur.iter().any(|x| x == p), "Cinnamon без {p}: {mint_aur:?}");
    }
}
