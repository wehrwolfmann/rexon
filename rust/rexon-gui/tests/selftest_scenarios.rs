//! Безоконные сценарии `rexon-gui-selftest` в `cargo test -p rexon-gui`.
//!
//! Самопроверки бинаря (выдача и запросы Discover, страница «Обои»,
//! 64-битные целые в QML, живые модели в QML) раньше запускались только
//! руками — и потому молча отставали от кода: сценарии стенд-окна
//! `rexon-gui-demo` пропали вместе с ним, и никто этого не заметил. Здесь
//! каждый сценарий — отдельный тест, приговор — код возврата бинаря.
//!
//! Обстановка прогона: пустое окружение (`env -i`), свой HOME в
//! `$HOME/.cache` (не /tmp — там ОЗУ), offscreen, программный рендер, без
//! дампов памяти — чтобы падение не подняло окно отчёта о сбое.
//! Слой обоев (`--layer-shell-selftest`) сюда не входит: ему нужна живая
//! сессия Wayland.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

fn private_home(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = PathBuf::from(std::env::var("HOME").expect("HOME"))
        .join(".cache")
        .join(format!("rexon-selftest-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(dir.join("run")).unwrap();
    dir
}

/// `~/.local/share` разработчика впереди системных каталогов данных.
fn developer_data_dirs() -> String {
    let home = std::env::var("HOME").expect("HOME");
    format!("{home}/.local/share:/usr/local/share:/usr/share")
}

fn run_scenario(arg: &str) {
    let tag = arg.trim_start_matches('-');
    let home = private_home(tag);
    let mut command = Command::new(env!("CARGO_BIN_EXE_rexon-gui-selftest"));
    command
        .arg(arg)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        // Собранные разработчиком QML-модули (`tools/build_qml_*.sh` пишут
        // в `~/.local/share/rexon/qml_modules`) — только на ЧТЕНИЕ, через
        // каталоги данных: свой HOME их прячет, и без этого стенд брал
        // модули УСТАНОВЛЕННОГО пакета — другой редакции, чем пин дерева
        // (`ui/QML_MATERIAL_PIN`). Пойман 24.10.2026: разметка 0.5.0 против
        // Qcm.Material 0.4.4 из пакета. Пишется по-прежнему в свой HOME.
        .env("XDG_DATA_DIRS", developer_data_dirs())
        .env("LANG", "C.UTF-8")
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QSG_RHI_BACKEND", "software")
        .env("QT_FORCE_STDERR_LOGGING", "1");
    // ulimit -c 0: упавший сценарий не должен оставлять дамп и поднимать
    // окно отчёта о сбое.
    unsafe {
        command.pre_exec(|| {
            let zero = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            libc::setrlimit(libc::RLIMIT_CORE, &zero);
            Ok(())
        });
    }
    let out = command.output().expect("бинарь rexon-gui-selftest запускается");
    let _ = std::fs::remove_dir_all(&home);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "сценарий {arg} провален: {}\n── stdout ──\n{stdout}\n── stderr ──\n{stderr}",
        out.status
    );
}

#[test]
fn remote_selftest() {
    run_scenario("--remote-selftest");
}

#[test]
fn discover_selftest() {
    run_scenario("--discover-selftest");
}

#[test]
fn wallpaper_selftest() {
    run_scenario("--wallpaper-selftest");
}

#[test]
fn qml_int64_probe() {
    run_scenario("--qml-int64-probe");
}

#[test]
fn remote_model_signals_selftest() {
    run_scenario("--remote-model-signals-selftest");
}

#[test]
fn user_property_selftest() {
    run_scenario("--user-property-selftest");
}

#[test]
fn toast_selftest() {
    run_scenario("--toast-selftest");
}

#[test]
fn image_viewer_selftest() {
    run_scenario("--image-viewer-selftest");
}

#[test]
fn store_files_selftest() {
    run_scenario("--store-files-selftest");
}

#[test]
fn steam_backgrounds_selftest() {
    run_scenario("--steam-backgrounds-selftest");
}

#[test]
fn steam_showcase_wizard_selftest() {
    run_scenario("--steam-showcase-wizard-selftest");
}
