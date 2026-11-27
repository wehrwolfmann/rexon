//! Stand for the live wiring of «Проверить и доустановить» on fakes.
//!
//! The checks in `system_de_exec` run the completion executor on fake
//! in-memory executors. Here it is the LIVE `completeness_survey_live` and
//! `complete_de_live`: real process launching by the core (`utils::run`),
//! the real `LiveDeOpsIo` and `LiveComplianceIo`. Only the programs are faked:
//! fake `pkexec` and `pacman` come first in `PATH` and write their
//! arguments to the stand's log.
//!
//! ⛔ The stand NEVER calls the real `pkexec` or the real `pacman`: before
//! the first run it verifies that both programs are found in the fakes
//! directory, and fails without launching anything otherwise. `HOME` and
//! `XDG_CONFIG_HOME` are private, in the build's temporary directory.
//!
//! The file is separate on purpose: the stand changes the process's environment variables, and
//! each `tests/` file gets its own process.
//!
//! The package manager lock is faked too: the executor waits for a lock file in the
//! stand's temporary directory (`LiveDeOpsIo::with_pkg_lock_paths`), not the
//! real `/var/lib/pacman/db.lck`. Previously the owner's live `pacman -Syu`
//! held the stand for up to 300 s. A spy on top of the live executor records every
//! path the FS was asked about — it is used to verify that the system lock is not
//! read at all.
//!
//! Boundary: the distro family is read from the real `/etc/os-release`
//! (the executor has no way to fake it). On non-Arch the package checks are not
//! run at all — the stand then says «НЕ ПРОВЕРЕНО» and exits.

mod i18n_pin;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rexon_core::system::{DistroDetector, PkgFamily};
use rexon_core::system_de_exec as dx;
use rexon_core::system_exec::{RunOut, SystemIo};
use rexon_core::system_plan::{self as plan, LogTail, LogTailKind};
use rexon_core::utils;

/// Live executor with its own lock + a record of the paths the FS was asked about.
struct SpyIo {
    inner: dx::LiveDeOpsIo,
    exists_calls: Mutex<Vec<String>>,
}

impl SystemIo for SpyIo {
    fn exists(&self, path: &str) -> bool {
        self.exists_calls.lock().unwrap().push(path.to_string());
        self.inner.exists(path)
    }
    fn is_file(&self, path: &str) -> bool {
        self.inner.is_file(path)
    }
    fn is_dir(&self, path: &str) -> bool {
        self.inner.is_dir(path)
    }
    fn is_executable(&self, path: &str) -> bool {
        self.inner.is_executable(path)
    }
    fn realpath(&self, path: &str) -> Option<String> {
        self.inner.realpath(path)
    }
    fn read_text(&self, path: &str) -> Option<String> {
        self.inner.read_text(path)
    }
    fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
        self.inner.list_dir(dir)
    }
    fn which(&self, name: &str) -> bool {
        self.inner.which(name)
    }
    fn run(&self, argv: &[String], timeout_secs: Option<u64>, lang_c: bool) -> RunOut {
        self.inner.run(argv, timeout_secs, lang_c)
    }
    fn env(&self, name: &str) -> String {
        self.inner.env(name)
    }
    fn which_path(&self, name: &str) -> Option<String> {
        self.inner.which_path(name)
    }
    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        self.inner.write_text(path, content)
    }
    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        self.inner.copy_file(src, dst)
    }
}

impl dx::DeOpsIo for SpyIo {
    fn run_to_file(&self, argv: &[String], timeout_secs: Option<u64>, log_path: &str) -> RunOut {
        self.inner.run_to_file(argv, timeout_secs, log_path)
    }
    fn temp_file(&self, prefix: &str, suffix: &str) -> Result<String, String> {
        self.inner.temp_file(prefix, suffix)
    }
    fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String> {
        self.inner.temp_dir(prefix, parent)
    }
    fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
        self.inner.write_file_mode(path, content, mode)
    }
    fn make_dirs(&self, path: &str) -> Result<(), String> {
        self.inner.make_dirs(path)
    }
    fn remove_file(&self, path: &str) {
        self.inner.remove_file(path)
    }
    fn remove_tree(&self, path: &str) {
        self.inner.remove_tree(path)
    }
    fn elapsed_secs(&self) -> f64 {
        self.inner.elapsed_secs()
    }
    fn sleep_secs(&self, secs: u64) {
        self.inner.sleep_secs(secs)
    }
    fn clock(&self) -> dx::Clock {
        self.inner.clock()
    }
    fn tail_log(
        &self,
        path: &str,
        kind: LogTailKind,
        emit: Arc<dyn Fn(&str, &str) + Send + Sync>,
    ) -> Option<LogTail> {
        self.inner.tail_log(path, kind, emit)
    }
    fn panel_snapshots(&self, reason: &str) -> Vec<(String, Result<(), String>)> {
        self.inner.panel_snapshots(reason)
    }
    fn app_defaults_switch(&self, de_id: &str) -> (bool, Vec<String>) {
        self.inner.app_defaults_switch(de_id)
    }
    fn de_config_backup(&self, de_id: &str, paths: &[String]) -> Result<(String, usize), String> {
        self.inner.de_config_backup(de_id, paths)
    }
    fn bridge_notify(&self, event: &dx::BridgeEvent) {
        self.inner.bridge_notify(event)
    }
    fn pkg_lock_paths(&self, family: PkgFamily) -> Vec<String> {
        self.inner.pkg_lock_paths(family)
    }
}

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<(String, String)>>,
}

impl dx::DeOpSink for Recorder {
    fn progress(&self, stage: &str, message: &str, _fraction: Option<f64>) {
        self.events
            .lock()
            .unwrap()
            .push((stage.to_string(), message.to_string()));
    }
    fn cancellable(&self, _allowed: bool) {}
}

fn write_script(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn set_env(key: &str, value: &str) {
    // The stand process is single-threaded during setup: there is one test in the file.
    #[allow(unused_unsafe)]
    unsafe {
        std::env::set_var(key, value);
    }
}

fn read_log(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn live_completeness_on_fake_pkexec_and_pacman() {
    // The golden values are the original's Russian texts: checked through po/ru.po.
    let _lang = i18n_pin::ru();
    if DistroDetector::detect_live().family_live() != PkgFamily::Arch {
        eprintln!("НЕ ПРОВЕРЕНО: машина не Arch — проверки пакетов не выполняются");
        return;
    }

    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "completeness_stand_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let bin = root.join("bin");
    let home = root.join("home");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(home.join(".config")).unwrap();
    let pkexec_log = root.join("pkexec.log");
    let pacman_log = root.join("pacman.log");
    let rc_file = root.join("pkexec.rc");
    // Fake package manager lock — instead of /var/lib/pacman/db.lck.
    let lock = root.join("db.lck");
    let lock_str = lock.display().to_string();
    let io = SpyIo {
        inner: dx::LiveDeOpsIo::with_pkg_lock_paths(vec![lock_str.clone()]),
        exists_calls: Mutex::new(Vec::new()),
    };

    // Fake pacman: only the COSMIC wallpapers and icons are "not installed".
    write_script(
        &bin.join("pacman"),
        &format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$*\" in\n  \
             '-Q cosmic-wallpapers'|'-Q cosmic-icon-theme') exit 1 ;;\nesac\nexit 0\n",
            pacman_log.display()
        ),
    );
    // Fake pkexec: record the arguments and exit with the code from the scenario file.
    write_script(
        &bin.join("pkexec"),
        &format!(
            "#!/bin/sh\necho \"$*\" >> '{log}'\nrc=$(cat '{rc}' 2>/dev/null || echo 0)\n\
             [ \"$rc\" = 126 ] && echo 'Error executing command as another user: Request dismissed' >&2\n\
             exit \"$rc\"\n",
            log = pkexec_log.display(),
            rc = rc_file.display()
        ),
    );

    let path = format!("{}:/usr/bin:/bin", bin.display());
    set_env("PATH", &path);
    set_env("HOME", &home.display().to_string());
    set_env("XDG_CONFIG_HOME", &home.join(".config").display().to_string());
    set_env("XDG_CURRENT_DESKTOP", "COSMIC");
    set_env("LANG", "ru_RU.UTF-8");

    // ⛔ Guard: both programs must be in the fakes directory.
    for prog in ["pkexec", "pacman"] {
        let found = utils::which(prog, Some(&path)).unwrap_or_default();
        assert!(
            found.starts_with(&bin.display().to_string()),
            "{prog} нашёлся не в каталоге подмен ({found}) — стенд остановлен до запуска"
        );
    }

    // ── Step 1: check without a password ───────────────────────────────
    let survey = dx::completeness_survey_live();
    assert_eq!(survey.de_id.as_deref(), Some("cosmic"), "{survey:?}");
    let keys: Vec<&str> = survey.fixes.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["cosmic_wallpapers", "cosmic_icons"], "{survey:?}");
    assert!(read_log(&pkexec_log).is_empty(), "проверка спросила пароль");
    assert!(read_log(&pacman_log).contains(&"-Q cosmic-wallpapers".to_string()));

    // ── Step 2: password refused — one window for everything ───────────
    std::fs::write(&rc_file, "126").unwrap();
    let rec = Arc::new(Recorder::default());
    let sink: Arc<dyn dx::DeOpSink> = rec.clone();
    let denied = dx::complete_de_with(&io, sink, dx::CancelToken::new(), &survey.fixes);
    assert!(!denied.outcome.ok, "{denied:?}");
    assert_eq!(denied.outcome.message, dx::completeness_denied_message());
    assert_eq!(read_log(&pkexec_log).len(), 1, "{:?}", read_log(&pkexec_log));

    // ── Step 3: password accepted — everything completed ───────────────
    std::fs::write(&pkexec_log, "").unwrap();
    std::fs::write(&rc_file, "0").unwrap();
    let rec = Arc::new(Recorder::default());
    let sink: Arc<dyn dx::DeOpSink> = rec.clone();
    let done = dx::complete_de_with(&io, sink, dx::CancelToken::new(), &survey.fixes);
    assert!(done.outcome.ok, "{done:?}");
    let calls = read_log(&pkexec_log);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[0].ends_with("cosmic-wallpapers"), "{calls:?}");
    assert!(calls[1].ends_with("cosmic-icon-theme"), "{calls:?}");
    let steps: Vec<String> = rec
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(stage, _)| stage == "install")
        .map(|(_, message)| message.clone())
        .collect();
    assert_eq!(
        steps,
        ["📦 Устанавливаю: cosmic-wallpapers", "📦 Устанавливаю: cosmic-icon-theme"]
    );

    // ── Step 4: the stand does not read the system lock ────────────────
    // The real /var/lib/pacman/db.lck cannot be faked, so the path itself
    // is checked: the executor waits only for the lock in the stand's temporary directory,
    // and never asked the FS about the system one.
    let system_locks = plan::lock_paths(PkgFamily::Arch);
    let asked = io.exists_calls.lock().unwrap().clone();
    assert!(asked.contains(&lock_str), "замок стенда не проверялся: {asked:?}");
    for path in &asked {
        assert!(
            !system_locks.contains(&path.as_str()),
            "стенд читает системный замок {path} — живой pacman владельца его задержит"
        );
    }
    let used = dx::DeOpsIo::pkg_lock_paths(&io, PkgFamily::Arch);
    assert!(!used.is_empty());
    for path in &used {
        assert!(
            Path::new(path).starts_with(&root),
            "замок {path} вне временного каталога стенда {}",
            root.display()
        );
    }

    // ── Step 5: the fake lock is held — the stand waits and gets it ───
    std::fs::write(&lock, "").unwrap();
    std::fs::write(&pkexec_log, "").unwrap();
    let release = {
        let lock = lock.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            std::fs::remove_file(&lock).unwrap();
        })
    };
    let started = std::time::Instant::now();
    let rec = Arc::new(Recorder::default());
    let sink: Arc<dyn dx::DeOpSink> = rec.clone();
    let waited = dx::complete_de_with(&io, sink, dx::CancelToken::new(), &survey.fixes);
    release.join().unwrap();
    assert!(waited.outcome.ok, "{waited:?}");
    assert!(
        started.elapsed().as_secs_f64() >= 3.0,
        "починка не дождалась замка: {:?}",
        started.elapsed()
    );
    let stages: Vec<String> = rec.events.lock().unwrap().iter().map(|(s, _)| s.clone()).collect();
    assert!(
        stages.iter().any(|s| s == plan::PKG_LOCK_WAIT_STAGE),
        "человеку не сказали про ожидание замка: {stages:?}"
    );
    // Having got the lock, both repairs went through.
    assert_eq!(read_log(&pkexec_log).len(), 2, "{:?}", read_log(&pkexec_log));

    let _ = std::fs::remove_dir_all(&root);
}
