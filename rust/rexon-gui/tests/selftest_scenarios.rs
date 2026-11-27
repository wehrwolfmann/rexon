//! Windowless `rexon-gui-selftest` scenarios in `cargo test -p rexon-gui`.
//!
//! The binary's self-tests (Discover results and queries, the "Wallpapers"
//! page, 64-bit integers in QML, live models in QML) used to be run only by
//! hand — and so they silently fell behind the code: the scenarios of the
//! `rexon-gui-demo` test bench window disappeared together with it, and
//! nobody noticed. Here every scenario is a separate test, and the verdict
//! is the binary's exit code.
//!
//! Run environment: an empty environment (`env -i`), our own HOME in
//! `$HOME/.cache` (not /tmp — that is RAM), offscreen, software rendering,
//! no core dumps — so that a crash does not bring up the crash report
//! window. The wallpaper layer (`--layer-shell-selftest`) is not included
//! here: it needs a live Wayland session.

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

/// The developer's `~/.local/share` goes before the system data
/// directories.
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
        // QML modules built by the developer (`tools/build_qml_*.sh` write to
        // `~/.local/share/rexon/qml_modules`) — READ-only, through the data
        // directories: our own HOME hides them, and without this the test bench
        // took the modules of the INSTALLED package — a different revision than
        // the tree's pin (`ui/QML_MATERIAL_PIN`). Caught on 24.10.2026: markup
        // 0.5.0 versus Qcm.Material 0.4.4 from the package. Writing still goes to
        // our own HOME.
        .env("XDG_DATA_DIRS", developer_data_dirs())
        .env("LANG", "C.UTF-8")
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("QSG_RHI_BACKEND", "software")
        .env("QT_FORCE_STDERR_LOGGING", "1");
    // ulimit -c 0: a crashed scenario must not leave a dump and bring up the
    // crash report window.
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

/// Removing the live wallpaper from a worker thread against a fake daemon.
#[test]
fn we_stop_selftest() {
    run_scenario("--we-stop-selftest");
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
