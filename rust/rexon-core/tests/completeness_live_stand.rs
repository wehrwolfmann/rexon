//! Стенд живой проводки «Проверить и доустановить» на подменах.
//!
//! Сверки в `system_de_exec` гоняют исполнитель доустановки на подставных
//! исполнителях в памяти. Здесь — ЖИВЫЕ `completeness_survey_live` и
//! `complete_de_live`: настоящий запуск процессов ядра (`utils::run`),
//! настоящие `LiveDeOpsIo` и `LiveComplianceIo`. Подменены только программы:
//! фальшивые `pkexec` и `pacman` лежат первыми в `PATH` и пишут свои
//! аргументы в журнал стенда.
//!
//! ⛔ Настоящий `pkexec` и настоящий `pacman` стенд не зовёт НИКОГДА: до
//! первого запуска он сверяет, что обе программы находятся в каталоге
//! подмен, и падает, ничего не запустив, если это не так. `HOME` и
//! `XDG_CONFIG_HOME` — приватные, во временном каталоге сборки.
//!
//! Файл отдельный намеренно: стенд меняет переменные окружения процесса, а
//! у каждого файла `tests/` свой процесс.
//!
//! Замок пакетного менеджера тоже подменён: исполнитель ждёт lock-файл во
//! временном каталоге стенда (`LiveDeOpsIo::with_pkg_lock_paths`), а не
//! настоящий `/var/lib/pacman/db.lck`. Раньше живой `pacman -Syu` владельца
//! держал стенд до 300 с. Шпион поверх живого исполнителя записывает каждый
//! путь, о котором спросили ФС, — по нему сверяется, что системный замок не
//! читается вовсе.
//!
//! Граница: семейство дистрибутива читается из настоящего `/etc/os-release`
//! (подменить его исполнителю нечем). На не-Arch проверки пакетов не
//! выполняются вовсе — стенд тогда говорит «НЕ ПРОВЕРЕНО» и выходит.

mod i18n_pin;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rexon_core::system::{DistroDetector, PkgFamily};
use rexon_core::system_de_exec as dx;
use rexon_core::system_exec::{RunOut, SystemIo};
use rexon_core::system_plan::{self as plan, LogTail, LogTailKind};
use rexon_core::utils;

/// Живой исполнитель со своим замком + запись путей, о которых спросили ФС.
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
    fn plasma_backup(&self, reason: &str) -> Result<(bool, String), String> {
        self.inner.plasma_backup(reason)
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
    // Процесс стенда однопоточный на время подготовки: тест в файле один.
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
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn live_completeness_on_fake_pkexec_and_pacman() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
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
    // Подставной замок пакетного менеджера — вместо /var/lib/pacman/db.lck.
    let lock = root.join("db.lck");
    let lock_str = lock.display().to_string();
    let io = SpyIo {
        inner: dx::LiveDeOpsIo::with_pkg_lock_paths(vec![lock_str.clone()]),
        exists_calls: Mutex::new(Vec::new()),
    };

    // Фальшивый pacman: «не установлены» только обои и значки COSMIC.
    write_script(
        &bin.join("pacman"),
        &format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$*\" in\n  \
             '-Q cosmic-wallpapers'|'-Q cosmic-icon-theme') exit 1 ;;\nesac\nexit 0\n",
            pacman_log.display()
        ),
    );
    // Фальшивый pkexec: записать аргументы и выйти с кодом из файла сценария.
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

    // ⛔ Защита: обе программы обязаны находиться в каталоге подмен.
    for prog in ["pkexec", "pacman"] {
        let found = utils::which(prog, Some(&path)).unwrap_or_default();
        assert!(
            found.starts_with(&bin.display().to_string()),
            "{prog} нашёлся не в каталоге подмен ({found}) — стенд остановлен до запуска"
        );
    }

    // ── Шаг 1: проверка без пароля ─────────────────────────────────────
    let survey = dx::completeness_survey_live();
    assert_eq!(survey.de_id.as_deref(), Some("cosmic"), "{survey:?}");
    let keys: Vec<&str> = survey.fixes.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["cosmic_wallpapers", "cosmic_icons"], "{survey:?}");
    assert!(read_log(&pkexec_log).is_empty(), "проверка спросила пароль");
    assert!(read_log(&pacman_log).contains(&"-Q cosmic-wallpapers".to_string()));

    // ── Шаг 2: отказ пароля — одно окно на всё ─────────────────────────
    std::fs::write(&rc_file, "126").unwrap();
    let rec = Arc::new(Recorder::default());
    let sink: Arc<dyn dx::DeOpSink> = rec.clone();
    let denied = dx::complete_de_with(&io, sink, dx::CancelToken::new(), &survey.fixes);
    assert!(!denied.outcome.ok, "{denied:?}");
    assert_eq!(denied.outcome.message, dx::completeness_denied_message());
    assert_eq!(read_log(&pkexec_log).len(), 1, "{:?}", read_log(&pkexec_log));

    // ── Шаг 3: пароль принят — доустановлено всё ───────────────────────
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

    // ── Шаг 4: системный замок стенд не читает ─────────────────────────
    // Подменить настоящий /var/lib/pacman/db.lck нельзя, поэтому сверяется
    // сам путь: исполнитель ждёт только замок во временном каталоге стенда,
    // и ни разу не спросил ФС о системном.
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

    // ── Шаг 5: подставной замок занят — стенд ждёт и дожидается ───────
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
    // Дождавшись замка, обе починки прошли.
    assert_eq!(read_log(&pkexec_log).len(), 2, "{:?}", read_log(&pkexec_log));

    let _ = std::fs::remove_dir_all(&root);
}
