//! Сторож правила «одна правда путей»: ВСЕ хранилища Rexon в конфигах
//! считаются от `$XDG_CONFIG_HOME`, а не от литерального `~/.config/rexon`.
//!
//! Зачем отдельный файл. Путь к папке настроек Rexon склеивали десяток
//! модулей, и часть из них делала это литералом от голого `$HOME`: мост
//! состояния (`bridge.json`, `bridge_commands/`) и каталог скриншотов. На
//! машине с нестандартным `XDG_CONFIG_HOME` получались ДВЕ папки настроек:
//! одна половина продукта писала в одну, другая читала другую, а удаление
//! Rexon сносило только XDG-шную. Сверка держит оба конца:
//!   * чистые строители (`*_xdg`) — прямой проверкой значения;
//!   * те, что снимают корень с ОКРУЖЕНИЯ, — дочерним прогоном в подставном
//!     `HOME`/`XDG_CONFIG_HOME` (менять окружение в общем процессе нельзя:
//!     сверки идут в несколько потоков, и соседняя ушла бы в живой профиль).
//!
//! ⚠️ Дочерний прогон НИЧЕГО не пишет на диск — только считает пути, поэтому
//! подставные каталоги и не обязаны существовать.

use std::path::{Path, PathBuf};

use rexon_core::{
    engine_update as eu, favorites, ocs_store, qt_app, qt_extensions, rexon_bridge,
    ruffle_manager as rm, screenshots as sc, settings, themes, utils, we_window as ww,
    window_menu as wm,
};

/// Признак дочернего прогона (он же — запрет запускать тело вручную).
const CHILD_ENV: &str = "REXON_XDG_ROOTS_CHILD";

/// Подставной корень конфигов дочернего прогона.
const CHILD_XDG: &str = "/подставной/xdg-config";

/// Подставной дом дочернего прогона: под ним НЕ должно оказаться ни одного
/// пути — именно туда уезжала вторая правда.
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

    // Корень не задан — стандартный `~/.config`, как у `os.path.expanduser`.
    assert_eq!(utils::rexon_config_dir_at(None, home), "/дом/.config/rexon");
    assert_eq!(sc::screenshot_dir(home), format!("{home}/{}", sc::SCREENSHOT_SUBDIR));
    // Пустая строка — это «не задан» (так её трактует спецификация XDG).
    assert_eq!(utils::rexon_config_dir_at(Some(""), home), "/дом/.config/rexon");
    assert_eq!(sc::screenshot_dir_xdg(Some(""), home), format!("{home}/{}", sc::SCREENSHOT_SUBDIR));
}

/// Пути, которые снимают корень с окружения, — в подставном окружении.
///
/// Тело в отдельном процессе: `std::env::set_var` меняет окружение всего
/// процесса, а сверки идут параллельно — соседняя могла бы уйти в живой
/// профиль Wehrwolfmann.
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

/// Тело сверки — только в дочернем процессе (см. выше).
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
        // Ни один путь не смотрит в `$HOME/.config` — это и была вторая правда.
        assert!(
            !got.starts_with(Path::new(CHILD_HOME)),
            "{what}: путь мимо XDG, в голом $HOME — {got:?}"
        );
    }
}
