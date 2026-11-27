//! Guard of the «one truth for paths» rule: ALL Rexon config stores are
//! computed from `$XDG_CONFIG_HOME`, not from a literal `~/.config/rexon`.
//!
//! Why a separate file. The path to the Rexon settings folder was glued
//! together by a dozen modules, and some of them did it literally from a bare
//! `$HOME`: the state bridge (`bridge.json`, `bridge_commands/`) and the
//! screenshots directory. On a machine with a non-standard `XDG_CONFIG_HOME`
//! this produced TWO settings folders: one half of the product wrote to one,
//! the other read the other, and uninstalling Rexon removed only the XDG one.
//! The check holds both ends:
//!   * pure builders (`*_xdg`) — by checking the value directly;
//!   * those that take the root from the ENVIRONMENT — by a child run in a
//!     mocked `HOME`/`XDG_CONFIG_HOME` (the environment cannot be changed in
//!     the shared process: checks run in several threads, and a neighbour
//!     would end up in the live profile).
//!
//! ⚠️ The child run writes NOTHING to disk — it only computes paths, so the
//! mocked directories need not exist.

use std::path::{Path, PathBuf};

use rexon_core::{
    engine_update as eu, favorites, ocs_store, qt_app, qt_extensions, rexon_bridge,
    ruffle_manager as rm, screenshots as sc, settings, themes, utils, we_window as ww,
    window_menu as wm,
};

/// Child-run marker (also forbids running the body by hand).
const CHILD_ENV: &str = "REXON_XDG_ROOTS_CHILD";

/// Mocked config root of the child run.
const CHILD_XDG: &str = "/подставной/xdg-config";

/// Mocked home of the child run: NOT a single path may end up under it —
/// that is exactly where the second truth went.
const CHILD_HOME: &str = "/подставной/дом";

#[test]
fn pure_builders_follow_xdg_config_home() {
    let home = "/дом";
    let xdg = Some("/xdg/конфиг");
    let root = "/xdg/конфиг/rexon";

    assert_eq!(utils::rexon_config_dir_at(xdg, home), root);
    assert_eq!(sc::screenshot_dir_xdg(xdg, home), format!("{root}/screenshots"));
    assert_eq!(themes::user_json_path_xdg(xdg, home), format!("{root}/themes.json"));
    assert_eq!(eu::state_path_xdg(xdg, home), format!("{root}/dev_upstream_state.json"));
    assert_eq!(rm::ruffle_paths_xdg(xdg, home).dir, format!("{root}/ruffle"));
    assert_eq!(rm::ruffle_paths_xdg(xdg, home).bin, format!("{root}/ruffle/ruffle"));
    assert_eq!(qt_app::crash_dir(xdg, home), format!("{root}/crashes"));
    assert_eq!(
        ocs_store::install_log_path_xdg(xdg, home),
        format!("{root}/ocs_installed.json")
    );
    assert_eq!(
        wm::recover_backup_path(home, xdg),
        format!("{root}/last_kde_lookandfeel.txt")
    );
    assert_eq!(
        utils::expand_user(ww::DEV_MARKER_PATH, home, xdg),
        format!("{root}/dev_mode")
    );

    // Root not set — the standard `~/.config`, as with `os.path.expanduser`.
    assert_eq!(utils::rexon_config_dir_at(None, home), "/дом/.config/rexon");
    assert_eq!(sc::screenshot_dir(home), format!("{home}/{}", sc::SCREENSHOT_SUBDIR));
    // An empty string means «not set» (that is how the XDG spec treats it).
    assert_eq!(utils::rexon_config_dir_at(Some(""), home), "/дом/.config/rexon");
    assert_eq!(sc::screenshot_dir_xdg(Some(""), home), format!("{home}/{}", sc::SCREENSHOT_SUBDIR));
}

/// Paths that take the root from the environment — in a mocked environment.
///
/// The body runs in a separate process: `std::env::set_var` changes the
/// environment of the whole process, and checks run in parallel — a
/// neighbour could end up in Wehrwolfmann's live profile.
#[test]
fn env_backed_paths_follow_xdg_config_home() {
    let out = std::process::Command::new(std::env::current_exe().expect("путь бинаря"))
        .args(["--exact", "env_backed_paths_child", "--ignored", "--test-threads=1"])
        .env(CHILD_ENV, "1")
        .env("HOME", CHILD_HOME)
        .env("XDG_CONFIG_HOME", CHILD_XDG)
        .output()
        .expect("дочерний прогон");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "дочерний прогон не прошёл:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Check body — only in the child process (see above).
#[test]
#[ignore = "запускается дочерним процессом env_backed_paths_follow_xdg_config_home"]
fn env_backed_paths_child() {
    assert!(std::env::var_os(CHILD_ENV).is_some(), "только дочерний прогон");
    let root = PathBuf::from(CHILD_XDG).join("rexon");

    assert_eq!(utils::config_home_env(), CHILD_XDG);
    assert_eq!(utils::rexon_config_dir(), root);

    let checked: Vec<(&str, PathBuf)> = vec![
        ("избранное", favorites::default_path()),
        ("каталог моста", rexon_bridge::bridge_dir()),
        ("файл моста", rexon_bridge::default_bridge_file()),
        ("команды моста", rexon_bridge::default_commands_dir()),
        ("настройки", settings::settings_file()),
        ("каталог расширений", qt_extensions::extensions_dir()),
        ("состояние расширений", qt_extensions::state_file()),
    ];
    let expected = [
        root.join("favorites.json"),
        root.clone(),
        root.join("bridge.json"),
        root.join("bridge_commands"),
        root.join("settings.json"),
        root.join("extensions"),
        root.join("extensions_state.json"),
    ];
    for ((what, got), want) in checked.iter().zip(expected.iter()) {
        assert_eq!(got, want, "{what}");
        // No path looks into `$HOME/.config` — that was the second truth.
        assert!(
            !got.starts_with(Path::new(CHILD_HOME)),
            "{what}: путь мимо XDG, в голом $HOME — {got:?}"
        );
    }
}
