//! Golden check of the `compliance.rs` port against the original `rexon/compliance.py`
//! (`ComplianceCheck`).
//!
//! The golden files were captured from LIVE Python read-only: a one-off harness ran
//! the real `run_for_de` with fully mocked I/O (safe_run/os/shutil/
//! tempfile/open — nothing privileged was executed, HOME fixed to
//! `/home/tester`) and recorded EVERY call at the level of the [`ComplianceIo`] trait
//! with arguments and result into a journal (`golden/compliance_cases.json`),
//! plus the full log and the check results.
//!
//! Here the journal is replayed: [`MockIo`] checks the op and arguments of each call of the Rust port
//! against the next entry (the order must match 1-to-1, the
//! write_text texts byte for byte) and returns the recorded result. At the end
//! the journal is exhausted, the log and results matched byte for byte. All 8 DEs
//! and check branches are covered (a valid/invalid/diverging icon theme, downloading
//! named/raw layouts, installs/fallbacks, first run, "no checks").

mod i18n_pin;

use rexon_core::compliance::{
    Compliance, ComplianceIo, PkgState, Results, RunResult, ScanEntry,
};
use rexon_core::system::PkgFamily;
use rexon_core::system_plan::query_cmd;
use serde_json::{json, Value};

fn fx() -> Value {
    // The helper directory is the one this machine chooses: the golden has it
    // from a manual install, while with the package installed the engine calls
    // the helper from `/usr/lib`. Why this does not hollow out the pin —
    // `system_plan::retarget_helper_paths`.
    let raw = rexon_core::system_plan::retarget_helper_paths(include_str!(
        "golden/compliance_cases.json"
    ));
    serde_json::from_str(&raw).unwrap()
}

struct MockIo {
    case: String,
    journal: Vec<Value>,
    pos: usize,
}

impl MockIo {
    fn new(case: &str, journal: &Value) -> Self {
        MockIo {
            case: case.to_string(),
            journal: journal.as_array().expect("journal — массив").clone(),
            pos: 0,
        }
    }

    fn next(&mut self, op: &str) -> Value {
        assert!(
            self.pos < self.journal.len(),
            "{}: журнал исчерпан на шаге {}, Rust просит op={op}",
            self.case,
            self.pos
        );
        let e = self.journal[self.pos].clone();
        assert_eq!(
            e["op"].as_str().unwrap_or(""),
            op,
            "{}: шаг {}: Rust просит {op}, эталон {}",
            self.case,
            self.pos,
            e["op"]
        );
        self.pos += 1;
        e
    }

    fn eq_field(&self, e: &Value, field: &str, got: &Value) {
        assert_eq!(
            &e[field], got,
            "{}: шаг {}: поле {field} не совпало (эталон {}, Rust {got})",
            self.case,
            self.pos - 1,
            e[field]
        );
    }
}

fn to_run_result(v: &Value) -> Option<RunResult> {
    if v.is_null() {
        None
    } else {
        Some(RunResult {
            ok: v["ok"].as_bool().unwrap(),
            stdout: v["stdout"].as_str().unwrap().to_string(),
        })
    }
}

impl ComplianceIo for MockIo {
    fn run(&mut self, argv: &[String], timeout: Option<u64>) -> Option<RunResult> {
        let e = self.next("run");
        let argv_json: Value = argv.iter().cloned().collect();
        self.eq_field(&e, "argv", &argv_json);
        let t = match timeout {
            Some(n) => Value::from(n),
            None => Value::Null,
        };
        self.eq_field(&e, "timeout", &t);
        to_run_result(&e["result"])
    }

    fn which(&mut self, name: &str) -> bool {
        let e = self.next("which");
        self.eq_field(&e, "name", &Value::from(name));
        e["result"].as_bool().unwrap()
    }

    fn env(&mut self, name: &str) -> String {
        let e = self.next("env");
        self.eq_field(&e, "name", &Value::from(name));
        e["result"].as_str().unwrap().to_string()
    }

    fn exists(&mut self, path: &str) -> bool {
        let e = self.next("exists");
        self.eq_field(&e, "path", &Value::from(path));
        e["result"].as_bool().unwrap()
    }

    fn is_file(&mut self, path: &str) -> bool {
        let e = self.next("is_file");
        self.eq_field(&e, "path", &Value::from(path));
        e["result"].as_bool().unwrap()
    }

    fn is_dir(&mut self, path: &str) -> bool {
        let e = self.next("is_dir");
        self.eq_field(&e, "path", &Value::from(path));
        e["result"].as_bool().unwrap()
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        let e = self.next("read_text");
        self.eq_field(&e, "path", &Value::from(path));
        e["result"].as_str().map(String::from)
    }

    fn scandir(&mut self, path: &str) -> Option<Vec<ScanEntry>> {
        let e = self.next("scandir");
        self.eq_field(&e, "path", &Value::from(path));
        e["result"].as_array().map(|arr| {
            arr.iter()
                .map(|x| ScanEntry {
                    name: x["name"].as_str().unwrap().to_string(),
                    is_dir: x["is_dir"].as_bool().unwrap(),
                })
                .collect()
        })
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let e = self.next("listdir");
        self.eq_field(&e, "path", &Value::from(path));
        e["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect()
    }

    fn makedirs(&mut self, path: &str) {
        let e = self.next("makedirs");
        self.eq_field(&e, "path", &Value::from(path));
    }

    fn write_text(&mut self, path: &str, content: &str) {
        let e = self.next("write_text");
        self.eq_field(&e, "path", &Value::from(path));
        self.eq_field(&e, "content", &Value::from(content));
    }

    fn replace(&mut self, src: &str, dst: &str) {
        let e = self.next("replace");
        self.eq_field(&e, "src", &Value::from(src));
        self.eq_field(&e, "dst", &Value::from(dst));
    }

    fn realpath(&mut self, path: &str) -> String {
        let e = self.next("realpath");
        self.eq_field(&e, "path", &Value::from(path));
        e["result"].as_str().unwrap().to_string()
    }

    fn mkdtemp(&mut self, prefix: &str, dir: Option<&str>) -> String {
        let e = self.next("mkdtemp");
        self.eq_field(&e, "prefix", &Value::from(prefix));
        let d = match dir {
            Some(s) => Value::from(s),
            None => Value::Null,
        };
        self.eq_field(&e, "dir", &d);
        e["result"].as_str().unwrap().to_string()
    }

    fn mkstemp(&mut self, prefix: &str, suffix: &str) -> String {
        let e = self.next("mkstemp");
        self.eq_field(&e, "prefix", &Value::from(prefix));
        self.eq_field(&e, "suffix", &Value::from(suffix));
        e["result"].as_str().unwrap().to_string()
    }

    fn chmod(&mut self, path: &str, mode: u32) {
        let e = self.next("chmod");
        self.eq_field(&e, "path", &Value::from(path));
        self.eq_field(&e, "mode", &Value::from(mode));
    }

    fn remove_file(&mut self, path: &str) {
        let e = self.next("remove_file");
        self.eq_field(&e, "path", &Value::from(path));
    }

    fn remove_tree(&mut self, path: &str) {
        let e = self.next("remove_tree");
        self.eq_field(&e, "path", &Value::from(path));
    }

    fn copy_file(&mut self, src: &str, dst: &str) {
        let e = self.next("copy_file");
        self.eq_field(&e, "src", &Value::from(src));
        self.eq_field(&e, "dst", &Value::from(dst));
    }
}

fn expected_results(v: &Value) -> Results {
    v.as_array()
        .unwrap()
        .iter()
        .map(|pair| {
            let a = pair.as_array().unwrap();
            (
                a[0].as_str().unwrap().to_string(),
                a[1].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn expected_log(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn all_cases_byte_identical() {
    // The golden values are the original's Russian texts: checked through po/ru.po.
    let _lang = i18n_pin::ru();
    let f = fx();
    let cases = f["cases"].as_array().unwrap();
    assert!(cases.len() >= 24, "ожидалось ≥24 кейса, получено {}", cases.len());
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let de_id = case["de_id"].as_str().unwrap();
        let home = case["home"].as_str().unwrap();
        let pkg_helper = case["pkg_helper_available"].as_bool().unwrap();
        let stage_helper = case["stage_helper_available"].as_bool().unwrap();

        let mut io = MockIo::new(name, &case["journal"]);
        let mut log: Vec<String> = Vec::new();
        let results = {
            // The golden files were captured on Arch/CachyOS with repair allowed —
            // family `arch`, `allow_privileged = true`.
            let mut comp = Compliance::new(
                &mut io, home, PkgFamily::Arch, true, pkg_helper, stage_helper,
            );
            comp.run_for_de(de_id, &mut log)
        };

        assert_eq!(
            io.pos,
            io.journal.len(),
            "{name}: журнал не исчерпан ({}/{} шагов)",
            io.pos,
            io.journal.len()
        );
        assert_eq!(log, expected_log(&case["log"]), "{name}: лог разошёлся");
        assert_eq!(
            results,
            expected_results(&case["results"]),
            "{name}: результаты разошлись"
        );
    }
}

/// Executor for the non-Arch branch: finds nothing, writes everything to the protocol.
/// There must be NO package commands here — that is what is checked.
#[derive(Default)]
struct RecordingIo {
    argv_log: Vec<Vec<String>>,
}

impl ComplianceIo for RecordingIo {
    fn run(&mut self, argv: &[String], _timeout: Option<u64>) -> Option<RunResult> {
        self.argv_log.push(argv.to_vec());
        None
    }
    fn which(&mut self, _name: &str) -> bool {
        false
    }
    fn env(&mut self, _name: &str) -> String {
        String::new()
    }
    fn exists(&mut self, _path: &str) -> bool {
        false
    }
    fn is_file(&mut self, _path: &str) -> bool {
        false
    }
    fn is_dir(&mut self, _path: &str) -> bool {
        false
    }
    fn read_text(&mut self, _path: &str) -> Option<String> {
        None
    }
    fn scandir(&mut self, _path: &str) -> Option<Vec<ScanEntry>> {
        None
    }
    fn listdir(&mut self, _path: &str) -> Vec<String> {
        Vec::new()
    }
    fn makedirs(&mut self, _path: &str) {}
    fn write_text(&mut self, _path: &str, _content: &str) {}
    fn replace(&mut self, _src: &str, _dst: &str) {}
    fn realpath(&mut self, path: &str) -> String {
        path.to_string()
    }
    fn mkdtemp(&mut self, _prefix: &str, _dir: Option<&str>) -> String {
        String::new()
    }
    fn mkstemp(&mut self, _prefix: &str, _suffix: &str) -> String {
        String::new()
    }
    fn chmod(&mut self, _path: &str, _mode: u32) {}
    fn remove_file(&mut self, _path: &str) {}
    fn remove_tree(&mut self, _path: &str) {}
    fn copy_file(&mut self, _src: &str, _dst: &str) {}
}

/// The package name if `argv` is the common engine's "is it installed" check command
/// for this family (`system_plan::query_cmd`).
fn queried_pkg<'a>(family: PkgFamily, argv: &'a [String]) -> Option<&'a str> {
    let pkg = argv.last()?;
    match query_cmd(family, pkg) {
        Ok(expected) if expected == argv => Some(pkg.as_str()),
        _ => None,
    }
}

/// Executor for a single package probe: answers with a preset outcome and
/// records EVERYTHING that was launched (it must be proven that nothing is launched
/// behind the gate).
struct ProbeIo {
    result: Option<RunResult>,
    argv_log: Vec<Vec<String>>,
}

impl ComplianceIo for ProbeIo {
    fn run(&mut self, argv: &[String], _timeout: Option<u64>) -> Option<RunResult> {
        self.argv_log.push(argv.to_vec());
        self.result.clone()
    }
    fn which(&mut self, _name: &str) -> bool {
        false
    }
    fn env(&mut self, _name: &str) -> String {
        String::new()
    }
    fn exists(&mut self, _path: &str) -> bool {
        false
    }
    fn is_file(&mut self, _path: &str) -> bool {
        false
    }
    fn is_dir(&mut self, _path: &str) -> bool {
        false
    }
    fn read_text(&mut self, _path: &str) -> Option<String> {
        None
    }
    fn scandir(&mut self, _path: &str) -> Option<Vec<ScanEntry>> {
        None
    }
    fn listdir(&mut self, _path: &str) -> Vec<String> {
        Vec::new()
    }
    fn makedirs(&mut self, _path: &str) {}
    fn write_text(&mut self, _path: &str, _content: &str) {}
    fn replace(&mut self, _src: &str, _dst: &str) {}
    fn realpath(&mut self, path: &str) -> String {
        path.to_string()
    }
    fn mkdtemp(&mut self, _prefix: &str, _dir: Option<&str>) -> String {
        "/tmp/rexon-test".to_string()
    }
    fn mkstemp(&mut self, _prefix: &str, _suffix: &str) -> String {
        "/tmp/rexon-test.sh".to_string()
    }
    fn chmod(&mut self, _path: &str, _mode: u32) {}
    fn remove_file(&mut self, _path: &str) {}
    fn remove_tree(&mut self, _path: &str) {}
    fn copy_file(&mut self, _src: &str, _dst: &str) {}
}

fn state_name(s: PkgState) -> &'static str {
    match s {
        PkgState::Installed => "installed",
        PkgState::Missing => "missing",
        PkgState::Unsupported => "unsupported",
    }
}

/// The "is the package installed" check: the command comes from the COMMON engine, the decision is by
/// the command's outcome, the output is not read at all. Previously there was a hard-coded
/// `pacman -Qi` with its own parsing — a third copy of the same question.
#[test]
fn pkg_query_matches_golden() {
    // The language — before parsing the golden file: the helper's `--lang` depends on it
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    for c in fx()["pkg_query"].as_array().unwrap() {
        let note = c["note"].as_str().unwrap();
        let family = PkgFamily::from_value(c["family"].as_str().unwrap()).unwrap();
        let pkg = c["pkg"].as_str().unwrap();

        // 1. The command is taken from the common engine and depends on the family.
        assert_eq!(
            json!(query_cmd(family, pkg).ok()),
            c["engine_argv"],
            "{note}: команда общего движка"
        );

        // 2. The package state and the FACT of a launch (nothing is launched behind the gate).
        let mut io = ProbeIo {
            result: to_run_result(&c["run"]),
            argv_log: Vec::new(),
        };
        let state = {
            let mut comp =
                Compliance::new(&mut io, "/home/tester", family, true, false, false);
            comp.pkg_state(pkg)
        };
        assert_eq!(state_name(state), c["state"].as_str().unwrap(), "{note}");
        let expected_calls = if c["argv"].is_null() {
            json!([])
        } else {
            json!([c["argv"]])
        };
        assert_eq!(json!(io.argv_log), expected_calls, "{note}: что запускали");
    }
}

/// One and the same check command for all three that ask this question:
/// the package engine, the Plymouth theme package check and compliance.
#[test]
fn pkg_query_is_the_same_everywhere() {
    let mut io = ProbeIo {
        result: Some(RunResult { ok: true, stdout: String::new() }),
        argv_log: Vec::new(),
    };
    {
        let mut comp =
            Compliance::new(&mut io, "/home/tester", PkgFamily::Arch, true, false, false);
        comp.pkg_state("plymouth");
    }
    assert_eq!(
        io.argv_log,
        vec![query_cmd(PkgFamily::Arch, "plymouth").unwrap()],
        "compliance обязан спрашивать ровно тем же, чем спрашивает движок"
    );
}

/// The family gate (finding K2 of the cross-distro audit): on non-Arch the package
/// steps are not run at all — no package checks, no `pkexec`, no AUR helper;
/// status `skipped`, ONE log line per run; non-package checks live on.
#[test]
fn non_arch_family_skips_package_steps() {
    use rexon_core::compliance::{pkg_unsupported_note, SKIPPED};
    let _lang = i18n_pin::en();

    // Keys of each DE's package steps — they must get `skipped`.
    let expect_skipped: &[(&str, &[&str])] = &[
        ("mate", &["mate_layouts", "mate_yaru_theme", "mate_ubuntu_fonts", "mate_aur_pkgs"]),
        ("kde", &["kde_sddm"]),
        ("gnome", &["gnome_gdm", "gnome_tweaks"]),
        ("cosmic", &["cosmic_wallpapers", "cosmic_icons", "cosmic_keyring"]),
        ("xfce", &["xfce_lightdm"]),
        ("cinnamon-mint", &["cinnamon_nemo", "cinnamon_themes", "cinnamon_lightdm"]),
        ("budgie", &["budgie_extras", "budgie_nm_applet"]),
        (
            "hyprland",
            &[
                "hyprland_hyprland", "hyprland_waybar", "hyprland_wofi", "hyprland_kitty",
                "hyprland_grim", "hyprland_slurp", "hyprland_wl-clipboard",
                "hyprland_portal", "hyprland_polkit",
            ],
        ),
    ];

    for family in [PkgFamily::Fedora, PkgFamily::Debian, PkgFamily::Suse, PkgFamily::Unknown] {
        for (de_id, keys) in expect_skipped {
            let mut io = RecordingIo::default();
            let mut log: Vec<String> = Vec::new();
            let results = {
                let mut comp =
                    Compliance::new(&mut io, "/home/tester", family, true, false, false);
                comp.run_for_de(de_id, &mut log)
            };

            for key in *keys {
                let got = results
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.as_str());
                assert_eq!(
                    got,
                    Some(SKIPPED),
                    "{de_id}/{family:?}: шаг {key} ожидался {SKIPPED}, получено {got:?}"
                );
            }

            // Not a single package/privileged command.
            for argv in &io.argv_log {
                let head = argv.first().map(String::as_str).unwrap_or("");
                assert!(
                    !matches!(head, "pacman" | "pkexec" | "yay" | "paru" | "trizen" | "pikaur"),
                    "{de_id}/{family:?}: запущена пакетная команда {argv:?}"
                );
            }

            // Exactly one explanation line for the whole run.
            let notes = log.iter().filter(|l| **l == pkg_unsupported_note()).count();
            assert_eq!(notes, 1, "{de_id}/{family:?}: строк-объяснений {notes}, ожидалась 1");
            assert!(
                log.last().is_some_and(|l| l.contains(" skipped")),
                "{de_id}/{family:?}: в итоге нет числа пропущенных: {:?}",
                log.last()
            );
        }
    }

    // Non-package checks keep working: KDE keeps the icon theme and
    // the terminal, XFCE — helpers.rc, Hyprland — the config.
    let mut io = RecordingIo::default();
    let mut log: Vec<String> = Vec::new();
    let results = {
        let mut comp =
            Compliance::new(&mut io, "/home/tester", PkgFamily::Fedora, true, false, false);
        comp.run_for_de("kde", &mut log)
    };
    assert!(
        results.iter().any(|(k, v)| k == "kde_terminal" && v != SKIPPED),
        "kde_terminal должен проверяться на любом дистрибутиве: {results:?}"
    );
    assert!(
        results.iter().any(|(k, _)| k == "kde_icon_theme"),
        "kde_icon_theme должен проверяться на любом дистрибутиве: {results:?}"
    );
}

/// A "live Arch" executor: packages and layouts are set up by the case,
/// commands are written to the protocol.
struct ArchIo {
    installed: Vec<String>,
    layouts_present: bool,
    helpers_found: bool,
    argv_log: Vec<Vec<String>>,
}

impl ArchIo {
    fn new(installed: &[&str], layouts_present: bool, helpers_found: bool) -> Self {
        ArchIo {
            installed: installed.iter().map(|s| s.to_string()).collect(),
            layouts_present,
            helpers_found,
            argv_log: Vec::new(),
        }
    }
}

impl ComplianceIo for ArchIo {
    fn run(&mut self, argv: &[String], _timeout: Option<u64>) -> Option<RunResult> {
        self.argv_log.push(argv.to_vec());
        // We recognize the check command BY THE COMMON ENGINE, not by a hard-coded
        // string: if the family's command changes, the stand will follow it by itself.
        if let Some(pkg) = queried_pkg(PkgFamily::Arch, argv) {
            return Some(RunResult {
                ok: self.installed.iter().any(|p| p == pkg),
                stdout: String::new(),
            });
        }
        None
    }
    fn which(&mut self, _name: &str) -> bool {
        self.helpers_found
    }
    fn env(&mut self, _name: &str) -> String {
        String::new()
    }
    fn exists(&mut self, path: &str) -> bool {
        if path.starts_with("/usr/share/mate-panel/layouts") {
            return self.layouts_present;
        }
        false
    }
    fn is_file(&mut self, _path: &str) -> bool {
        false
    }
    fn is_dir(&mut self, _path: &str) -> bool {
        false
    }
    fn read_text(&mut self, _path: &str) -> Option<String> {
        None
    }
    fn scandir(&mut self, _path: &str) -> Option<Vec<ScanEntry>> {
        None
    }
    fn listdir(&mut self, _path: &str) -> Vec<String> {
        Vec::new()
    }
    fn makedirs(&mut self, _path: &str) {}
    fn write_text(&mut self, _path: &str, _content: &str) {}
    fn replace(&mut self, _src: &str, _dst: &str) {}
    fn realpath(&mut self, path: &str) -> String {
        path.to_string()
    }
    fn mkdtemp(&mut self, _prefix: &str, _dir: Option<&str>) -> String {
        "/tmp/rexon-test".to_string()
    }
    fn mkstemp(&mut self, _prefix: &str, _suffix: &str) -> String {
        "/tmp/rexon-test.sh".to_string()
    }
    fn chmod(&mut self, _path: &str, _mode: u32) {}
    fn remove_file(&mut self, _path: &str) {}
    fn remove_tree(&mut self, _path: &str) {}
    fn copy_file(&mut self, _src: &str, _dst: &str) {}
}

/// Autostart at login: diagnostics run, but the password and the network are
/// NOT touched for ANY set of missing packages and layouts.
#[test]
fn unattended_never_asks_password_or_network() {
    use rexon_core::compliance::{privileged_deferred_note, NEEDS_ACTION};
    let _lang = i18n_pin::en();
    let deferred_note = privileged_deferred_note();

    let all_des = [
        "mate", "kde", "gnome", "cosmic", "xfce", "cinnamon-mint", "budgie", "hyprland",
    ];
    // Sets of "what is already installed" — from empty to full, plus a partial one.
    let installed_sets: [&[&str]; 3] = [
        &[],
        &["ubuntu-mate-themes", "ttf-ubuntu-font-family", "mate-tweak", "cosmic-wallpapers"],
        &[
            "ubuntu-mate-themes", "ubuntu-mate-icon-themes", "ttf-ubuntu-font-family",
            "mate-tweak", "brisk-menu", "mate-indicator-applet", "vala-panel-appmenu-mate",
            "vala-panel-appmenu-registrar",
            "ayatana-indicator-application", "ayatana-indicator-session",
            "ayatana-indicator-datetime", "ayatana-indicator-sound",
            "ayatana-indicator-notifications", "plasma-login-manager", "sddm", "gdm",
            "gnome-tweaks", "cosmic-wallpapers", "cosmic-icon-theme", "gnome-keyring",
            "lightdm", "nemo", "mint-themes", "mint-y-icons", "budgie-extras",
            "network-manager-applet", "hyprland", "waybar", "wofi", "kitty", "grim",
            "slurp", "wl-clipboard", "xdg-desktop-portal-hyprland", "polkit-gnome",
        ],
    ];

    for de_id in all_des {
        for installed in installed_sets {
            for layouts_present in [false, true] {
                for helpers_found in [false, true] {
                    let mut io = ArchIo::new(installed, layouts_present, helpers_found);
                    let mut log: Vec<String> = Vec::new();
                    let results = {
                        let mut comp = Compliance::new(
                            &mut io,
                            "/home/tester",
                            PkgFamily::Arch,
                            false, // autostart: repair is not allowed
                            true,  // helpers "exist" — a path without raw pkexec
                            true,
                        );
                        comp.run_for_de(de_id, &mut log)
                    };
                    let ctx = format!(
                        "{de_id}/пакетов={}/раскладки={layouts_present}/хелперы={helpers_found}",
                        installed.len()
                    );

                    for argv in &io.argv_log {
                        let head = argv.first().map(String::as_str).unwrap_or("");
                        // The installed check only reads the database,
                        // does not ask for a password. Recognized by the common engine.
                        let read_only_query =
                            queried_pkg(PkgFamily::Arch, argv).is_some();
                        assert!(
                            read_only_query
                                || !matches!(
                                    head,
                                    "pacman" | "pkexec" | "git" | "yay" | "paru" | "trizen"
                                        | "pikaur" | "sudo"
                                ),
                            "{ctx}: при входе в систему запущено {argv:?}"
                        );
                        assert!(
                            !argv.iter().any(|a| a.contains("org.rexon.pkg")
                                || a.contains("org.rexon.stage")),
                            "{ctx}: при входе в систему вызван привилегированный helper {argv:?}"
                        );
                    }

                    // Not a single "fixed": at login we repair nothing.
                    assert!(
                        !results.iter().any(|(_, v)| v == "fixed"),
                        "{ctx}: что-то починено в режиме автозапуска: {results:?}"
                    );

                    let notes = log.iter().filter(|l| **l == deferred_note).count();
                    let needs =
                        results.iter().filter(|(_, v)| v == NEEDS_ACTION).count();
                    assert!(notes <= 1, "{ctx}: строк-объяснений {notes}, ожидалось ≤1");
                    assert_eq!(
                        notes > 0,
                        needs > 0,
                        "{ctx}: строка-объяснение и статусы разошлись ({notes}/{needs})"
                    );
                }
            }
        }
    }

    // Control: the same MATE with repair allowed — layouts and packages
    // really are repaired (git clone + the AUR helper are used).
    let mut io = ArchIo::new(&[], false, true);
    let mut log: Vec<String> = Vec::new();
    {
        let mut comp =
            Compliance::new(&mut io, "/home/tester", PkgFamily::Arch, true, false, false);
        comp.run_for_de("mate", &mut log);
    }
    assert!(
        io.argv_log.iter().any(|a| a.first().map(String::as_str) == Some("git")),
        "интерактивный режим: git clone раскладок не запускался"
    );
    assert!(
        io.argv_log.iter().any(|a| a.first().map(String::as_str) == Some("yay")),
        "интерактивный режим: AUR-хелпер не запускался"
    );
}

#[test]
fn detect_current_de_maps() {
    use rexon_core::compliance::detect_current_de;
    assert_eq!(detect_current_de("KDE").as_deref(), Some("kde"));
    assert_eq!(detect_current_de("plasma").as_deref(), Some("kde"));
    assert_eq!(detect_current_de("X-Cinnamon").as_deref(), Some("cinnamon-mint"));
    assert_eq!(detect_current_de("cinnamon").as_deref(), Some("cinnamon-mint"));
    assert_eq!(detect_current_de("Budgie:GNOME").as_deref(), Some("budgie"));
    assert_eq!(detect_current_de("GNOME:Unity").as_deref(), Some("gnome"));
    assert_eq!(detect_current_de("").as_deref(), None);
    // Tiling ones no longer slip past the checks: previously there was None here, and
    // in a sway/niri session the desktop was "not detected" at all.
    assert_eq!(detect_current_de("sway").as_deref(), Some("sway"));
    assert_eq!(detect_current_de("niri").as_deref(), Some("niri"));
    assert_eq!(detect_current_de("Hyprland").as_deref(), Some("hyprland"));
    assert_eq!(detect_current_de("LXQt").as_deref(), Some("lxqt"));
}

// ══════════════════════════════════════════════════════════════════════════
//  Run header for all DEs + "DE not recognized" (section `run_texts_v2`)
// ══════════════════════════════════════════════════════════════════════════

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn run_texts_match_python() {
    // The golden values are the original's Russian texts: checked through po/ru.po.
    let _lang = i18n_pin::ru();
    let f = fx();
    let g = &f["run_texts_v2"];
    assert_eq!(
        serde_json::json!(rexon_core::compliance::msg_unknown_de()),
        g["unknown_de"]
    );
    for (de, want) in g["has_checks"].as_object().unwrap() {
        assert_eq!(
            serde_json::json!(rexon_core::compliance::has_checks(de)),
            *want,
            "de={de}"
        );
    }
    for (de, want) in g["banner"].as_object().unwrap() {
        assert_eq!(
            serde_json::json!(rexon_core::compliance::de_banner_lines(de)),
            *want,
            "de={de}"
        );
    }
}

// ── installing a package by repair ─────────────────────────────────────────
//
// ⛔ Not a single command below was executed: the check only BUILDS argv.

/// The repair no longer has a hand-written fallback. It repeated `pkexec` + the engine
/// table word for word — a copy that lives exactly until the first edit of the table: in
/// a neighbouring place of the same class (`wallpaper_engine`) such a copy had already
/// diverged, losing `--needed`.
#[test]
fn pkg_install_arch_asks_the_package_engine() {
    use rexon_core::compliance::pkg_install_arch;
    use rexon_core::system_plan::install_argv;
    // The language is common to the process, and both sides of the comparison ask for it
    // anew (`--lang=<code>` to the helper): without pinning, a neighbouring check,
    // switching the language between calls, gave an extra `--lang=ru` to one of them.
    let _lang = i18n_pin::en();

    for pkg in ["kvantum", "papirus-icon-theme", "qt6ct"] {
        for avail in [false, true] {
            assert_eq!(
                pkg_install_arch(pkg, avail),
                install_argv(PkgFamily::Arch, &[pkg.to_string()], avail),
                "{pkg}: починка ответила своё (helper={avail})"
            );
        }
    }
}

/// What actually comes out — written as strings, so that an edit of the engine
/// table does not slip through silently. The named path goes through
/// rexon-pkg-helper (no extra password), the raw one — `pkexec` + `install_cmd`.
#[test]
fn pkg_install_arch_commands_are_pinned() {
    use rexon_core::compliance::pkg_install_arch;
    // English: with another language the helper also gets `--lang=<code>`
    // (if the installed helper understands it) — the lines below are about the rest.
    let _lang = i18n_pin::en();

    assert_eq!(
        pkg_install_arch("kvantum", false),
        Some(vec![
            "pkexec".into(), "pacman".into(), "-S".into(), "--needed".into(),
            "--noconfirm".into(), "kvantum".into(),
        ])
    );
    assert_eq!(
        pkg_install_arch("kvantum", true),
        Some(vec![
            "pkexec".into(),
            rexon_core::system_plan::pkg_helper_path().into(),
            "arch".into(), "install".into(), "kvantum".into(),
        ])
    );
}

/// An empty package name is a defect of the caller, not "nothing to do": there is no
/// command, nothing to launch. The former copy silently built `pacman -S --needed
/// --noconfirm ""` and handed it to pkexec.
#[test]
fn pkg_install_arch_refuses_empty_package_name() {
    use rexon_core::compliance::pkg_install_arch;

    assert_eq!(pkg_install_arch("", false), None);
    assert_eq!(pkg_install_arch("", true), None);
}

// ══════════════════════════════════════════════════════════════════════════
//  Language of the check log: English, "in Russian as it was", no Cyrillic
// ══════════════════════════════════════════════════════════════════════════
//
// The «Проверить и доустановить» log is seen by the buyer, so it is translated.
// Below are four runs on a fake machine (nothing is launched), their
// English lines in full, the same runs in Russian — verbatim as the
// former code printed, and a guard: not a single Russian letter in the English log.

/// Everything installed on a "full" Arch machine (for the Hyprland run).
const HYPRLAND_FULL: &[&str] = &[
    "hyprland", "waybar", "wofi", "kitty", "grim", "slurp", "wl-clipboard",
    "xdg-desktop-portal-hyprland", "polkit-gnome",
];

/// A run of one desktop: the log and the results.
fn run_log<IO: ComplianceIo>(
    io: &mut IO,
    de_id: &str,
    family: PkgFamily,
    allow_privileged: bool,
) -> Vec<String> {
    let mut log = Vec::new();
    let mut comp = Compliance::new(io, "/home/tester", family, allow_privileged, false, false);
    comp.run_for_de(de_id, &mut log);
    log
}

/// Four runs of the language check: KDE not on Arch (package steps skipped),
/// COSMIC at login (repair deferred), Hyprland on a full machine
/// and MATE on an empty machine with repair allowed but failing.
fn language_runs() -> Vec<(&'static str, Vec<String>)> {
    vec![
        ("kde/fedora", run_log(&mut RecordingIo::default(), "kde", PkgFamily::Fedora, true)),
        ("cosmic/вход", run_log(&mut ArchIo::new(&[], false, true), "cosmic", PkgFamily::Arch, false)),
        (
            "hyprland/полная",
            run_log(&mut ArchIo::new(HYPRLAND_FULL, false, true), "hyprland", PkgFamily::Arch, false),
        ),
        ("mate/пустая", run_log(&mut ArchIo::new(&[], false, true), "mate", PkgFamily::Arch, true)),
    ]
}

fn lines(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn english_log_lines() {
    use rexon_core::compliance::{
        de_banner_lines, fix_label, hyprland_component_label, msg_unknown_de,
    };
    let _lang = i18n_pin::en();
    let runs = language_runs();
    let want: [Vec<String>; 4] = [
        lines(&[
            "🔍 Checking desktop kde...",
            "  🖼️ Checking the icon theme...",
            "    ℹ️ kdeglobals: no icon theme set",
            "  🖥️ Checking the KDE terminal...",
            "    ℹ️ kdeglobals not found (first launch?)",
            "  ℹ️ Checking and installing system packages is only available on Arch/CachyOS \
for now — all other settings were checked",
            "✅ Check finished: 2 OK, 0 fixed, 0 failed, 1 skipped",
        ]),
        lines(&[
            "🔍 Checking desktop cosmic...",
            "  🖼️ Checking COSMIC wallpapers...",
            "    ⚠️ cosmic-wallpapers isn't installed",
            "  ℹ️ Some parts are missing — Rexon will install them when you click \
“Check and Install Missing” in Settings; we don't ask for a password at login",
            "  🎨 Checking COSMIC icons...",
            "    ⚠️ cosmic-icon-theme isn't installed",
            "  🔐 Checking gnome-keyring...",
            "    ⚠️ gnome-keyring isn't installed",
            "✅ Check finished: 0 OK, 0 fixed, 1 failed, 2 to install",
        ]),
        lines(&[
            "🔍 Checking desktop hyprland...",
            "  🧱 Checking Hyprland components...",
            "    ✅ Compositor: hyprland",
            "    ✅ Panel: waybar",
            "    ✅ Launcher: wofi",
            "    ✅ Terminal: kitty",
            "    ✅ Screenshots: grim",
            "    ✅ Area selection: slurp",
            "    ✅ Clipboard: wl-clipboard",
            "  📝 Checking the configuration...",
            "    ⚠️ hyprland.conf not found",
            "    ℹ️ Create it: cp /usr/share/hyprland/hyprland.conf ~/.config/hypr/",
            "  🌐 Checking xdg-desktop-portal...",
            "    ✅ xdg-desktop-portal-hyprland is installed",
            "  🔐 Checking the polkit agent...",
            "    ✅ polkit-gnome is installed",
            "✅ Check finished: 9 OK, 0 fixed, 1 failed",
        ]),
        lines(&[
            "🔍 Checking desktop mate...",
            "  🖥️ Checking the default terminal...",
            "  📐 Checking all 8 Ubuntu MATE panel layouts...",
            "    ⚠️ Missing (8): familiar, redmond, cupertino, mutiny, pantheon, contemporary, \
netbook, traditional",
            "    📥 Downloading layouts from ubuntu-mate-settings...",
            "    ⚠️ Couldn't clone the repository",
            "  🎨 Checking the gschema override (Yaru-MATE, Ubuntu fonts)...",
            "    ⚠️ gschema override is missing",
            "    ℹ️ Reinstall MATE with Rexon to set it up automatically",
            "  🎨 Checking the Yaru-MATE theme...",
            "    ⚠️ Not installed: ubuntu-mate-themes, ubuntu-mate-icon-themes",
            "    📥 Installing with yay...",
            "    ⚠️ Couldn't install (try installing manually)",
            "  🔤 Checking Ubuntu fonts...",
            "    ⚠️ Ubuntu fonts aren't installed",
            "    ⚠️ Installation failed",
            "  📦 Checking MATE AUR packages and the Ayatana stack...",
            "    ⚠️ Not installed (10): mate-tweak, brisk-menu, mate-indicator-applet, \
vala-panel-appmenu-mate, vala-panel-appmenu-registrar...",
            "    📥 Installing with yay...",
            "    ⚠️ Couldn't install (try installing manually)",
            "  🖱️ Checking the Yaru cursor...",
            "✅ Check finished: 0 OK, 0 fixed, 7 failed",
        ]),
    ];
    for ((name, got), want) in runs.iter().zip(want.iter()) {
        assert_eq!(got, want, "{name}");
    }

    assert_eq!(msg_unknown_de(), "⚠️ Couldn't detect the current desktop");
    assert_eq!(de_banner_lines("kde")[1], "  Desktop: kde");
    assert_eq!(fix_label("mate_layouts").as_deref(), Some("MATE panel layouts (downloaded from the internet)"));
    assert_eq!(fix_label("budgie_extras").as_deref(), Some("Budgie add-ons"));
    assert_eq!(fix_label("нет такого"), None);
    assert_eq!(hyprland_component_label("slurp"), "Area selection");
    assert_eq!(hyprland_component_label("foot"), "foot");
}

/// A snapshot of the Russian log — verbatim as the code printed before the translation.
/// Lines that are not in the Python golden file (`all_cases_byte_identical`):
/// skipping package steps, deferred repair, a summary with «требуют
/// доустановки» and «пропущено», captions of the missing items and Hyprland components.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn russian_log_as_before() {
    use rexon_core::compliance::{
        de_banner_lines, fix_label, hyprland_component_label, msg_unknown_de,
        pkg_unsupported_note, privileged_deferred_note, FIX_LABELS,
    };
    let _lang = i18n_pin::ru();
    const PKG_NOTE: &str = "  ℹ️ Проверка и установка системных пакетов пока доступны только на \
Arch/CachyOS — остальные настройки проверены";
    const DEFERRED_NOTE: &str = "  ℹ️ Кое-чего не хватает — Rexon доустановит это по кнопке \
«Проверить и доустановить» в настройках; при входе в систему пароль не спрашиваем";
    assert_eq!(pkg_unsupported_note(), PKG_NOTE);
    assert_eq!(privileged_deferred_note(), DEFERRED_NOTE);

    let runs = language_runs();
    let want: [Vec<String>; 4] = [
        lines(&[
            "🔍 Проверка рабочего стола kde...",
            "  🖼️ Проверка темы значков...",
            "    ℹ️ kdeglobals: тема значков не задана",
            "  🖥️ Проверка терминала KDE...",
            "    ℹ️ kdeglobals не найден (первый запуск?)",
            PKG_NOTE,
            "✅ Проверка завершена — в порядке: 2, исправлено: 0, с ошибкой: 0, пропущено: 1",
        ]),
        lines(&[
            "🔍 Проверка рабочего стола cosmic...",
            "  🖼️ Проверка обоев COSMIC...",
            "    ⚠️ cosmic-wallpapers не установлен",
            DEFERRED_NOTE,
            "  🎨 Проверка значков COSMIC...",
            "    ⚠️ cosmic-icon-theme не установлен",
            "  🔐 Проверка gnome-keyring...",
            "    ⚠️ gnome-keyring не установлен",
            "✅ Проверка завершена — в порядке: 0, исправлено: 0, с ошибкой: 1, нужно доустановить: 2",
        ]),
        lines(&[
            "🔍 Проверка рабочего стола hyprland...",
            "  🧱 Проверка компонентов Hyprland...",
            "    ✅ Композитор: hyprland",
            "    ✅ Панель: waybar",
            "    ✅ Меню запуска: wofi",
            "    ✅ Терминал: kitty",
            "    ✅ Скриншоты: grim",
            "    ✅ Выделение области: slurp",
            "    ✅ Буфер обмена: wl-clipboard",
            "  📝 Проверка конфигурации...",
            "    ⚠️ hyprland.conf не найден",
            "    ℹ️ Создайте: cp /usr/share/hyprland/hyprland.conf ~/.config/hypr/",
            "  🌐 Проверка xdg-desktop-portal...",
            "    ✅ xdg-desktop-portal-hyprland установлен",
            "  🔐 Проверка polkit-агента...",
            "    ✅ polkit-gnome установлен",
            "✅ Проверка завершена — в порядке: 9, исправлено: 0, с ошибкой: 1",
        ]),
        lines(&[
            "🔍 Проверка рабочего стола mate...",
            "  🖥️ Проверка терминала по умолчанию...",
            "  📐 Проверка всех 8 макетов панели Ubuntu MATE...",
            "    ⚠️ Отсутствуют (8): familiar, redmond, cupertino, mutiny, pantheon, contemporary, \
netbook, traditional",
            "    📥 Загрузка макетов панели из ubuntu-mate-settings...",
            "    ⚠️ Не удалось клонировать репозиторий",
            "  🎨 Проверка файла стандартных настроек gschema (Yaru-MATE, шрифты Ubuntu)...",
            "    ⚠️ Файла стандартных настроек gschema нет",
            "    ℹ️ Переустановите MATE через Rexon для автоматической установки",
            "  🎨 Проверка Yaru-MATE темы...",
            "    ⚠️ Не установлены: ubuntu-mate-themes, ubuntu-mate-icon-themes",
            "    📥 Установка через yay...",
            "    ⚠️ Не удалось установить (попробуйте вручную)",
            "  🔤 Проверка шрифтов Ubuntu...",
            "    ⚠️ Шрифты Ubuntu не установлены",
            "    ⚠️ Не удалось установить",
            "  📦 Проверка AUR-пакетов MATE + Ayatana-стек...",
            "    ⚠️ Не установлены (10): mate-tweak, brisk-menu, mate-indicator-applet, \
vala-panel-appmenu-mate, vala-panel-appmenu-registrar...",
            "    📥 Установка через yay...",
            "    ⚠️ Не удалось установить (попробуйте вручную)",
            "  🖱️ Проверка курсора Yaru...",
            "✅ Проверка завершена — в порядке: 0, исправлено: 0, с ошибкой: 7",
        ]),
    ];
    for ((name, got), want) in runs.iter().zip(want.iter()) {
        assert_eq!(got, want, "{name}");
    }

    assert_eq!(msg_unknown_de(), "⚠️ Не удалось определить текущий рабочий стол");
    assert_eq!(de_banner_lines("kde")[1], "  Рабочий стол: kde");

    // Captions of the missing items (the «Проверить и доустановить» question window).
    const LABELS_BEFORE: &[(&str, &str)] = &[
        ("mate_layouts", "раскладки панели MATE (скачиваются из интернета)"),
        ("mate_yaru_theme", "тема оформления Yaru для MATE (из AUR)"),
        ("mate_ubuntu_fonts", "шрифты Ubuntu"),
        ("mate_aur_pkgs", "дополнения панели MATE и значки области уведомлений (из AUR)"),
        ("cosmic_wallpapers", "обои COSMIC"),
        ("cosmic_icons", "значки COSMIC"),
        ("cinnamon_themes", "темы оформления Linux Mint (из AUR)"),
        ("budgie_extras", "дополнения Budgie"),
    ];
    assert_eq!(LABELS_BEFORE.len(), FIX_LABELS.len());
    for (key, before) in LABELS_BEFORE {
        assert_eq!(fix_label(key).as_deref(), Some(*before), "{key}");
    }

    const HYPRLAND_BEFORE: &[(&str, &str)] = &[
        ("hyprland", "Композитор"),
        ("waybar", "Панель"),
        ("wofi", "Меню запуска"),
        ("kitty", "Терминал"),
        ("grim", "Скриншоты"),
        ("slurp", "Выделение области"),
        ("wl-clipboard", "Буфер обмена"),
    ];
    for (pkg, before) in HYPRLAND_BEFORE {
        assert_eq!(hyprland_component_label(pkg), *before, "{pkg}");
    }
}

fn cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// The English check log — not a single Russian letter: a forgotten string
/// is seen here, not by the buyer. All desktops are run on all families,
/// on an empty and a full machine, at login and from the settings, plus all cases
/// of the Python golden file (their executor log does not depend on the language).
///
/// The "which package to install" hint belongs to `utils` (its translation is
/// a separate pass) and is subtracted from the checked line.
#[test]
fn english_compliance_has_no_russian() {
    use rexon_core::compliance::{
        de_banner_lines, fix_label, msg_unknown_de, pkg_unsupported_note,
        privileged_deferred_note, FIX_LABELS,
    };
    let _lang = i18n_pin::en();
    let all_des = [
        "mate", "kde", "gnome", "cosmic", "xfce", "cinnamon-mint", "budgie", "hyprland", "lxqt",
    ];
    let full: Vec<&str> = HYPRLAND_FULL
        .iter()
        .copied()
        .chain([
            "ubuntu-mate-themes", "ubuntu-mate-icon-themes", "ttf-ubuntu-font-family", "sddm",
            "gdm", "gnome-tweaks", "cosmic-wallpapers", "cosmic-icon-theme", "gnome-keyring",
            "lightdm", "nemo", "mint-themes", "mint-y-icons", "budgie-extras",
            "network-manager-applet",
        ])
        .collect();

    let mut all: Vec<String> = vec![
        msg_unknown_de(),
        pkg_unsupported_note(),
        privileged_deferred_note(),
    ];
    all.extend(de_banner_lines("kde"));
    for (key, _) in FIX_LABELS {
        all.push(fix_label(key).unwrap());
    }
    for de in all_des {
        for family in [PkgFamily::Arch, PkgFamily::Fedora, PkgFamily::Debian] {
            for allow in [false, true] {
                all.extend(run_log(&mut RecordingIo::default(), de, family, allow));
                for installed in [&[][..], &full[..]] {
                    for layouts in [false, true] {
                        for helpers in [false, true] {
                            let mut io = ArchIo::new(installed, layouts, helpers);
                            all.extend(run_log(&mut io, de, family, allow));
                        }
                    }
                }
            }
        }
    }
    let f = fx();
    for case in f["cases"].as_array().unwrap() {
        let mut io = MockIo::new(case["name"].as_str().unwrap(), &case["journal"]);
        let mut comp = Compliance::new(
            &mut io,
            case["home"].as_str().unwrap(),
            PkgFamily::Arch,
            true,
            case["pkg_helper_available"].as_bool().unwrap(),
            case["stage_helper_available"].as_bool().unwrap(),
        );
        comp.run_for_de(case["de_id"].as_str().unwrap(), &mut all);
    }

    let hints: Vec<String> = ["arch", "fedora", "debian"]
        .iter()
        .map(|fam| rexon_core::utils::tool_package_hint("git", fam))
        .filter(|h| !h.is_empty())
        .collect();
    assert!(all.len() > 500, "прогонов подозрительно мало: {}", all.len());
    for line in &all {
        let mut own = line.clone();
        for h in &hints {
            own = own.replace(h.as_str(), "");
        }
        assert!(!cyrillic(&own), "русский текст в английском журнале: {line:?}");
    }
}

// ============================================================================
// Terminal: the person's choice beats the native one (Wehrwolfmann's decision 08.11.2026)
// ============================================================================
//
// The choice in the «Настройки» tab lies in `<desktop>-xdg-terminals.list` with
// the mark "chosen by the person". The login check puts IT into the desktop's native
// place, not the native terminal; without a choice — the native one, as before; one that is chosen
// but not installed — the native one, and the log says so.

/// A machine for the terminal check: files, installed programs, answers of
/// `gsettings get`; launched commands accumulate in `argv_log`.
#[derive(Default)]
struct TermIo {
    files: std::collections::BTreeMap<String, String>,
    tools: Vec<String>,
    gsettings: std::collections::BTreeMap<String, String>,
    argv_log: Vec<Vec<String>>,
}

impl ComplianceIo for TermIo {
    fn run(&mut self, argv: &[String], _timeout: Option<u64>) -> Option<RunResult> {
        self.argv_log.push(argv.to_vec());
        if argv.len() == 4 && argv[0] == "gsettings" && argv[1] == "get" {
            let key = format!("{} {}", argv[2], argv[3]);
            return self.gsettings.get(&key).map(|v| RunResult { ok: true, stdout: format!("'{v}'\n") });
        }
        Some(RunResult { ok: true, stdout: String::new() })
    }
    fn which(&mut self, name: &str) -> bool {
        self.tools.iter().any(|t| t == name)
    }
    fn env(&mut self, _name: &str) -> String {
        String::new()
    }
    fn exists(&mut self, path: &str) -> bool {
        self.files.contains_key(path)
    }
    fn is_file(&mut self, path: &str) -> bool {
        self.files.contains_key(path)
    }
    fn is_dir(&mut self, _path: &str) -> bool {
        false
    }
    fn read_text(&mut self, path: &str) -> Option<String> {
        self.files.get(path).cloned()
    }
    fn scandir(&mut self, _path: &str) -> Option<Vec<ScanEntry>> {
        None
    }
    fn listdir(&mut self, _path: &str) -> Vec<String> {
        Vec::new()
    }
    fn makedirs(&mut self, _path: &str) {}
    fn write_text(&mut self, path: &str, content: &str) {
        self.files.insert(path.to_string(), content.to_string());
    }
    fn replace(&mut self, _src: &str, _dst: &str) {}
    fn realpath(&mut self, path: &str) -> String {
        path.to_string()
    }
    fn mkdtemp(&mut self, _prefix: &str, _dir: Option<&str>) -> String {
        String::new()
    }
    fn mkstemp(&mut self, _prefix: &str, _suffix: &str) -> String {
        String::new()
    }
    fn chmod(&mut self, _path: &str, _mode: u32) {}
    fn remove_file(&mut self, _path: &str) {}
    fn remove_tree(&mut self, _path: &str) {}
    fn copy_file(&mut self, _src: &str, _dst: &str) {}
}

const TERM_HOME: &str = "/home/tester";

impl TermIo {
    fn new(tools: &[&str]) -> Self {
        TermIo { tools: tools.iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }
    fn chosen(mut self, list: &str, terminal: &str) -> Self {
        self.files.insert(
            format!("{TERM_HOME}/.config/{list}"),
            rexon_core::defaults::desktop_terminals_list_text(terminal, true),
        );
        self
    }
    fn gset(mut self, schema: &str, value: &str) -> Self {
        self.gsettings.insert(format!("{schema} exec"), value.to_string());
        self
    }
    fn sets(&self, schema: &str) -> Vec<String> {
        self.argv_log
            .iter()
            .filter(|a| a.len() == 5 && a[0] == "gsettings" && a[1] == "set" && a[2] == schema)
            .map(|a| format!("{}={}", a[3], a[4]))
            .collect()
    }
}

fn term_run(io: &mut TermIo, desktop: &str) -> (Results, Vec<String>) {
    let mut log = Vec::new();
    let results = Compliance::new(io, TERM_HOME, PkgFamily::Unknown, false, false, false)
        .run_for_current_de(desktop, &mut log);
    (results, log)
}

fn result_of<'a>(results: &'a Results, key: &str) -> Option<&'a str> {
    results.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

const MATE_SCHEMA: &str = "org.mate.applications-terminal";
const GNOME_SCHEMA: &str = "org.gnome.desktop.default-applications.terminal";

#[test]
fn chosen_terminal_wins_over_native_in_mate() {
    let _lang = i18n_pin::en();
    // The person's choice is konsole: it goes into the MATE schema, with its exec-arg.
    let mut io = TermIo::new(&["konsole", "mate-terminal"])
        .chosen("mate-xdg-terminals.list", "org.kde.konsole.desktop")
        .gset(MATE_SCHEMA, "mate-terminal");
    let (results, _) = term_run(&mut io, "MATE");
    assert_eq!(io.sets(MATE_SCHEMA), vec!["exec=konsole", "exec-arg=-e"]);
    assert_eq!(result_of(&results, "mate_terminal"), Some("fixed"));

    // The chosen one is already set — we do not touch it.
    let mut io = TermIo::new(&["konsole", "mate-terminal"])
        .chosen("mate-xdg-terminals.list", "org.kde.konsole.desktop")
        .gset(MATE_SCHEMA, "konsole");
    let (results, _) = term_run(&mut io, "MATE");
    assert!(io.sets(MATE_SCHEMA).is_empty(), "{:?}", io.argv_log);
    assert_eq!(result_of(&results, "mate_terminal"), Some("ok"));

    // Without a choice — the native one, as before.
    let mut io = TermIo::new(&["konsole", "mate-terminal"]).gset(MATE_SCHEMA, "konsole");
    term_run(&mut io, "MATE");
    assert_eq!(io.sets(MATE_SCHEMA), vec!["exec=mate-terminal", "exec-arg=-x"]);

    // The chosen one is not installed — the native one, and the log says so.
    let mut io = TermIo::new(&["mate-terminal"])
        .chosen("mate-xdg-terminals.list", "kitty.desktop")
        .gset(MATE_SCHEMA, "xterm");
    let (_, log) = term_run(&mut io, "MATE");
    assert_eq!(io.sets(MATE_SCHEMA), vec!["exec=mate-terminal", "exec-arg=-x"]);
    assert!(
        log.iter().any(|l| l.contains("Terminal: mate-terminal (kitty isn't installed)")),
        "{log:?}"
    );
}

#[test]
fn chosen_terminal_wins_over_native_in_kde() {
    let kdeglobals = format!("{TERM_HOME}/.config/kdeglobals");
    let kw = |io: &TermIo| -> Vec<String> {
        io.argv_log
            .iter()
            .filter(|a| a.first().map(String::as_str) == Some("kwriteconfig6"))
            .map(|a| a[a.len() - 2..].join("="))
            .collect()
    };
    let mut io = TermIo::new(&["kitty", "konsole", "kwriteconfig6"]).chosen("kde-xdg-terminals.list", "kitty.desktop");
    io.files.insert(kdeglobals.clone(), "[General]\nTerminalApplication=konsole\n".into());
    let (results, _) = term_run(&mut io, "KDE");
    assert_eq!(kw(&io), vec!["TerminalApplication=kitty", "TerminalService=kitty.desktop"]);
    assert_eq!(result_of(&results, "kde_terminal"), Some("fixed"));

    // Without a choice konsole stays konsole.
    let mut io = TermIo::new(&["kitty", "konsole", "kwriteconfig6"]);
    io.files.insert(kdeglobals.clone(), "[General]\nTerminalApplication=konsole\n".into());
    let (results, _) = term_run(&mut io, "KDE");
    assert!(kw(&io).is_empty(), "{:?}", io.argv_log);
    assert_eq!(result_of(&results, "kde_terminal"), Some("ok"));
}

#[test]
fn chosen_terminal_wins_over_native_in_gnome() {
    let mut io = TermIo::new(&["alacritty", "gnome-terminal"])
        .chosen("gnome-xdg-terminals.list", "Alacritty.desktop")
        .gset(GNOME_SCHEMA, "kgx");
    let (results, _) = term_run(&mut io, "GNOME");
    assert_eq!(io.sets(GNOME_SCHEMA), vec!["exec=alacritty", "exec-arg=-e"]);
    assert_eq!(result_of(&results, "gnome_terminal"), Some("fixed"));

    // Without a choice the native GNOME terminal is not touched.
    let mut io = TermIo::new(&["alacritty", "gnome-terminal"]).gset(GNOME_SCHEMA, "kgx");
    term_run(&mut io, "GNOME");
    assert!(io.sets(GNOME_SCHEMA).is_empty(), "{:?}", io.argv_log);
}
