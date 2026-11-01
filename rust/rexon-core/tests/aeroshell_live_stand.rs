//! Стенд живой проводки сверки и пересборки оформления AeroShell на подменах.
//!
//! Сверки в `system_de_exec` гоняют исполнитель на подставном исполнителе в
//! памяти. Здесь — ЖИВЫЕ `aeroshell_survey_live` и `rebuild_aeroshell_live`:
//! настоящий запуск процессов ядра, настоящий читатель журнала сборки,
//! настоящее чтение модуля эффекта с диска. Подменены только программы:
//! фальшивые `pacman`, `yay` и `pkexec` лежат в каталоге стенда и пишут свои
//! аргументы в журнал.
//!
//! ⛔ На машине стоят настоящие paru и yay. Поэтому `PATH` стенда — ТОЛЬКО
//! каталог подмен (плюс ссылка на безвредный `env`), и до первого запуска
//! стенд сверяет: `pacman`, `yay`, `pkexec` и `env` находятся в нём, а `paru`,
//! `makepkg`, `mkarchroot` и `sudo` не находятся вовсе. Не так — падает,
//! ничего не запустив. `HOME` — приватный, во временном каталоге сборки.
//!
//! Файл отдельный намеренно: стенд меняет окружение процесса, а у каждого
//! файла `tests/` свой процесс.
//!
//! Замок пакетного менеджера подменён: исполнитель ждёт lock-файл во
//! временном каталоге стенда (`LiveDeOpsIo::with_pkg_lock_paths`), а не
//! настоящий замок pacman. Раньше занятый живым `pacman -Syu` замок
//! выключал стенд целиком.
//!
//! Граница: семейство дистрибутива читается из настоящего `/etc/os-release`.
//! Не Arch — стенд говорит «НЕ ПРОВЕРЕНО» и выходит.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use rexon_core::aeroshell;
use rexon_core::system::{DistroDetector, PkgFamily};
use rexon_core::system_de_exec as dx;
use rexon_core::system_plan as plan;
use rexon_core::utils;

const PACKAGE: &str = "aeroshell-kwin-components-git";

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

impl Recorder {
    fn messages(&self, stage: &str) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(s, _)| s == stage)
            .map(|(_, m)| m.clone())
            .collect()
    }
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

fn rebuild(io: &dx::LiveDeOpsIo, packages: &[String]) -> (dx::DeOpOutcome, Arc<Recorder>) {
    let rec = Arc::new(Recorder::default());
    let sink: Arc<dyn dx::DeOpSink> = rec.clone();
    (dx::rebuild_aeroshell_with(io, sink, dx::CancelToken::new(), packages), rec)
}

#[test]
fn live_aeroshell_on_fake_pacman_and_yay() {
    if DistroDetector::detect_live().family_live() != PkgFamily::Arch {
        eprintln!("НЕ ПРОВЕРЕНО: машина не Arch — пакетов оформления не бывает");
        return;
    }

    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("aeroshell_stand_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bin = root.join("bin");
    let home = root.join("home");
    let plugins = root.join("lib/qt6/plugins/kwin/effects/plugins");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(home.join(".config")).unwrap();
    std::fs::create_dir_all(&plugins).unwrap();
    let effect = plugins.join("aeroglide.so");
    std::fs::write(&effect, b"\x7fELF\0org.kde.kwin.EffectPluginFactory6.7.4\0").unwrap();
    let pacman_log = root.join("pacman.log");
    let yay_log = root.join("yay.log");
    let pkexec_log = root.join("pkexec.log");
    let scenario = root.join("yay.scenario");
    // Подставной замок пакетного менеджера — вместо настоящего замка pacman.
    let io = dx::LiveDeOpsIo::with_pkg_lock_paths(vec![root.join("db.lck").display().to_string()]);

    // Фальшивый pacman: стоит один пакет оформления, собранный под KWin
    // 6.7.4, а KWin уже 6.7.5.
    write_script(
        &bin.join("pacman"),
        &format!(
            "#!/bin/sh\necho \"$*\" >> '{log}'\ncase \"$*\" in\n  \
             '-Qq') printf 'bash\\n{PACKAGE}\\nkwin\\n' ;;\n  \
             '-Q kwin') echo 'kwin 6.7.5-1' ;;\n  \
             '-Q plasma-workspace') echo 'plasma-workspace 6.7.5-1' ;;\n  \
             '-Ql {PACKAGE}') echo '{PACKAGE} {so}' ;;\n  \
             *) exit 1 ;;\nesac\nexit 0\n",
            log = pacman_log.display(),
            so = effect.display()
        ),
    );
    // Фальшивый yay: записать аргументы, сказать пару строк сборки и выйти
    // по сценарию (`rc=…` в файле; 126 — окно пароля закрыли).
    write_script(
        &bin.join("yay"),
        &format!(
            "#!/bin/sh\necho \"$*\" >> '{log}'\nrc=0\n. '{scenario}'\n\
             echo \"==> Making package: {PACKAGE}\"\necho \"locale=$LC_ALL\"\n\
             if [ \"$rc\" = 126 ]; then\n  \
             echo 'Error executing command as another user: Request dismissed' >&2\n  \
             echo ' -> error installing repo packages: exit status 126' >&2\n  exit 1\nfi\n\
             [ \"$rc\" = 0 ] || echo '==> ERROR: A failure occurred in build().'\n\
             exit \"$rc\"\n",
            log = yay_log.display(),
            scenario = scenario.display()
        ),
    );
    // Фальшивый pkexec: нужен только как «есть чем спросить пароль».
    write_script(
        &bin.join("pkexec"),
        &format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 0\n", pkexec_log.display()),
    );
    std::os::unix::fs::symlink("/usr/bin/env", bin.join("env")).unwrap();

    let path = bin.display().to_string();
    set_env("PATH", &path);
    set_env("HOME", &home.display().to_string());
    set_env("XDG_CONFIG_HOME", &home.join(".config").display().to_string());
    set_env("LANG", "ru_RU.UTF-8");

    // ⛔ Защита: подмены — в каталоге стенда, настоящих сборщиков не видно.
    for prog in ["pacman", "yay", "pkexec", "env"] {
        let found = utils::which(prog, Some(&path)).unwrap_or_default();
        assert!(
            found.starts_with(&path),
            "{prog} нашёлся не в каталоге подмен ({found}) — стенд остановлен до запуска"
        );
    }
    for prog in ["paru", "makepkg", "mkarchroot", "sudo"] {
        assert_eq!(
            utils::which(prog, Some(&path)),
            None,
            "{prog} виден стенду — стенд остановлен до запуска"
        );
    }

    // ── Сверка без пароля ──────────────────────────────────────────────
    let report = dx::aeroshell_survey_live();
    assert!(report.needs_rebuild, "{report:?}");
    assert_eq!(report.stale, [PACKAGE]);
    assert_eq!(report.rebuild, [PACKAGE]);
    assert_eq!(report.packages[0].built_for.as_deref(), Some("6.7.4"));
    assert!(read_log(&yay_log).is_empty(), "сверка запустила сборщик");
    assert!(read_log(&pkexec_log).is_empty(), "сверка спросила пароль");

    // ── Пересборка удалась ─────────────────────────────────────────────
    std::fs::write(&scenario, "rc=0\n").unwrap();
    let (done, rec) = rebuild(&io, &report.rebuild);
    assert!(done.ok, "{done:?}");
    assert_eq!(done.message, aeroshell::rebuild_success_message());
    let calls = read_log(&yay_log);
    assert_eq!(
        calls,
        [format!(
            "-S --rebuild --diffmenu=false --noconfirm --sudo pkexec --sudoflags  {PACKAGE}"
        )]
    );
    // Ход сборки пришёл живым читателем журнала, сборщик говорил по-английски.
    let lines = rec.messages(aeroshell::REBUILD_LINE_STAGE);
    assert_eq!(
        lines,
        [format!("==> Making package: {PACKAGE}"), "locale=C".to_string()]
    );
    assert_eq!(rec.messages("done"), [aeroshell::rebuild_done_message()]);
    // Подставной замок свободен — ожидания чужой транзакции не было.
    assert!(rec.messages(plan::PKG_LOCK_WAIT_STAGE).is_empty(), "стенд ждал замок");

    // ── Отказ пароля ───────────────────────────────────────────────────
    std::fs::write(&yay_log, "").unwrap();
    std::fs::write(&scenario, "rc=126\n").unwrap();
    let (denied, _) = rebuild(&io, &report.rebuild);
    assert!(!denied.ok && !denied.cancelled, "{denied:?}");
    assert_eq!(denied.message, aeroshell::rebuild_denied_message());
    assert_eq!(read_log(&yay_log).len(), 1);

    // ── Сбой сборки ────────────────────────────────────────────────────
    std::fs::write(&scenario, "rc=1\n").unwrap();
    let (failed, _) = rebuild(&io, &report.rebuild);
    assert!(!failed.ok);
    assert!(failed.message.starts_with(&aeroshell::rebuild_failed_head()), "{}", failed.message);
    assert!(failed.message.ends_with("==> ERROR: A failure occurred in build()."), "{}", failed.message);

    assert!(read_log(&pkexec_log).is_empty(), "стенд позвал pkexec");
    let _ = std::fs::remove_dir_all(&root);
}
