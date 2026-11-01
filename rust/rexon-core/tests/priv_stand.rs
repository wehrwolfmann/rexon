//! Стенд уровня 2 («один пароль на запуск», см. docs/testing.md): помощник сеанса
//! прав — ОБЫЧНЫЙ ПРОЦЕСС. Тест — настоящее окно ([`Client`]: поток сеанса,
//! рукопожатие, очередь), помощник — бинарь `rexon-priv-stand` (тот же
//! `helper::run_session`, «сухой» `RootIo` с корнем-префиксом). Между ними —
//! `socketpair` на fd 0/1 дочки, как это делает `pkexec`; «шим pkexec» здесь —
//! [`StandSpawner`]: вместо `pkexec <помощник> …` он запускает сам помощник с
//! `PKEXEC_UID`. Ни root, ни pkexec, ни pacman: команды помощника только
//! записываются в журнал стенда, и тест сверяет журнал argv.
//!
//! ⛔ Собирается только с feature `stand` (`required-features`):
//! `BUILD_QUEUE_SLOTS=1 scripts/build-queue test -p rexon-core --features stand --test priv_stand`
//! или `scripts/priv-stand`.

use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rexon_core::privilege::client::{
    Client, ClientError, FileMeta, HelperChild, HelperFs, RealFs, Spawner, BUILD_ID_FLAG, PKEXEC,
};
use rexon_core::privilege::op::{FontFile, LocalFile, PkgSource, PrivOp};
use rexon_core::privilege::{proto, validate};
use rexon_core::system::PkgFamily;

const STAND: &str = env!("CARGO_BIN_EXE_rexon-priv-stand");
const WAIT: Duration = Duration::from_secs(10);
const SESSION: &str = "7";

fn uid() -> u32 {
    // SAFETY: getuid без предусловий.
    unsafe { libc::getuid() }
}

// ── каталог стенда ──────────────────────────────────────────────────────

struct Bench {
    dir: PathBuf,
}

impl Bench {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rexon-priv-stand-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("root");
        for d in ["etc", "usr/bin", "var/tmp", "home/stand"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("etc/os-release"), "ID=arch\nNAME=\"Arch Linux\"\n").unwrap();
        for p in ["pacman", "systemctl", "loginctl"] {
            let f = root.join("usr/bin").join(p);
            std::fs::write(&f, "").unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let b = Bench { dir };
        b.set_session(uid(), true, false);
        b
    }

    fn set_session(&self, user: u32, active: bool, locked: bool) {
        let yn = |b: bool| if b { "yes" } else { "no" };
        self.put("session", &format!("User={user}\nActive={}\nLockedHint={}\nDesktop=KDE\n", yn(active), yn(locked)));
    }

    fn put(&self, name: &str, text: &str) {
        // Замена одним шагом: помощник читает файл в своём потоке опроса.
        let tmp = self.dir.join(format!(".{name}.tmp"));
        std::fs::write(&tmp, text).unwrap();
        std::fs::rename(&tmp, self.dir.join(name)).unwrap();
    }

    fn journal(&self) -> String {
        std::fs::read_to_string(self.dir.join("journal")).unwrap_or_default()
    }

    fn lines(&self, prefix: &str) -> Vec<String> {
        self.journal().lines().filter(|l| l.starts_with(prefix)).map(str::to_string).collect()
    }

    /// Ждать строку журнала, содержащую `needle`.
    fn wait_for(&self, needle: &str) {
        let start = Instant::now();
        while start.elapsed() < WAIT {
            if self.journal().contains(needle) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("в журнале стенда нет «{needle}» за {WAIT:?}:\n{}", self.journal());
    }

    /// Сколько помощников вышло (строк `exit`).
    fn exits(&self) -> Vec<String> {
        self.lines("exit ")
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

// ── «шим pkexec» и файловая система для поиска помощника ────────────────

/// Как запускается помощник.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Launch {
    /// Как pkexec: помощник заменяет шим (`exec`), его родитель — окно.
    Exec,
    /// Посредник: оболочка запускает помощника своим потомком — у сокета
    /// чужой для помощника собеседник (родитель ≠ тот, кто создал пару).
    Forked,
}

struct StandSpawner {
    dir: PathBuf,
    env: Vec<(String, String)>,
    spawned: Arc<AtomicU32>,
    probe: bool,
    launch: Launch,
}

struct StandChild(std::process::Child);

impl HelperChild for StandChild {
    fn wait(&mut self) -> std::io::Result<Option<i32>> {
        self.0.wait().map(|st| st.code())
    }
}

impl StandSpawner {
    fn command(&self, program: &str) -> Command {
        let mut cmd = match self.launch {
            Launch::Exec => Command::new(program),
            Launch::Forked => {
                let mut c = Command::new("/bin/sh");
                c.arg("-c").arg("\"$@\"; exit $?").arg("sh").arg(program);
                c
            }
        };
        cmd.env("REXON_STAND_DIR", &self.dir);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd
    }
}

impl Spawner for StandSpawner {
    fn spawn(&mut self, argv: &[String], helper_end: OwnedFd) -> std::io::Result<Box<dyn HelperChild>> {
        // Окно зовёт `pkexec <помощник> [--session …] [--lang …]`; шим делает
        // то же, что pkexec после пароля: тот же fd 0/1 и `PKEXEC_UID`.
        assert_eq!(argv[0], PKEXEC, "окно должно звать pkexec: {argv:?}");
        self.spawned.fetch_add(1, Ordering::SeqCst);
        let out = helper_end.try_clone()?;
        let mut cmd = self.command(&argv[1]);
        cmd.args(&argv[2..])
            .env("PKEXEC_UID", uid().to_string())
            .stdin(Stdio::from(helper_end))
            .stdout(Stdio::from(out))
            .stderr(Stdio::inherit());
        let child = cmd.spawn();
        drop(cmd);
        Ok(Box::new(StandChild(child?)))
    }

    fn probe_build_id(&mut self, helper: &Path) -> Option<String> {
        if !self.probe {
            return None; // «старый помощник без флага»: сверка — в рукопожатии
        }
        let out = self.command(&helper.to_string_lossy()).arg(BUILD_ID_FLAG).stdin(Stdio::null()).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && !text.is_empty()).then_some(text)
    }
}

/// Бинарь стенда лежит в каталоге сборки пользователя; проверка владения
/// (она проверена юнит-тестами `client.rs`) видит его файлом root без записи
/// для группы и всех.
struct RootLookingFs;

impl HelperFs for RootLookingFs {
    fn lstat(&self, path: &Path) -> std::io::Result<FileMeta> {
        let m = RealFs.lstat(path)?;
        Ok(FileMeta { kind: m.kind, uid: 0, mode: m.mode & !0o022 })
    }
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        RealFs.canonicalize(path)
    }
}

struct Setup {
    env: Vec<(String, String)>,
    probe: bool,
    launch: Launch,
}

impl Default for Setup {
    fn default() -> Self {
        Setup { env: Vec::new(), probe: true, launch: Launch::Exec }
    }
}

fn client(bench: &Bench, setup: Setup) -> (Client, Arc<AtomicU32>) {
    let spawned = Arc::new(AtomicU32::new(0));
    let spawner = StandSpawner {
        dir: bench.dir.clone(),
        env: setup.env,
        spawned: Arc::clone(&spawned),
        probe: setup.probe,
        launch: setup.launch,
    };
    let c = Client::new(Box::new(spawner), Box::new(RootLookingFs), proto::build_id(), Some(SESSION.into()))
        .with_candidates(vec![PathBuf::from(STAND)]);
    (c, spawned)
}

fn install(pkg: &str) -> PrivOp {
    PrivOp::PkgInstall { family: PkgFamily::Arch, packages: vec![pkg.into()], source: PkgSource::YtDlp }
}

fn remove(pkgs: &[&str]) -> PrivOp {
    PrivOp::PkgRemove { family: PkgFamily::Arch, packages: pkgs.iter().map(|s| s.to_string()).collect() }
}

fn request(c: &mut Client, op: &PrivOp) -> Result<rexon_core::privilege::client::Outcome, ClientError> {
    c.request(op, Vec::new(), &mut |_| {})
}

const YT_DLP_RUN: &str = "run pacman -S --needed --noconfirm yt-dlp";

fn wait_no_session(c: &Client) {
    let start = Instant::now();
    while c.has_session() {
        assert!(start.elapsed() < WAIT, "окно не узнало о конце сеанса");
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ── сценарии ────────────────────────────────────────────────────────────

/// Одна сессия → несколько операций без повторного подъёма (одно «окно
/// пароля»), журнал argv — ровно команды таблицы ядра.
#[test]
fn one_password_many_operations() {
    let b = Bench::new("many");
    let (mut c, spawned) = client(&b, Setup::default());
    let mut progress = Vec::new();
    for _ in 0..3 {
        let out = c.request(&install("yt-dlp"), Vec::new(), &mut |l| progress.push(l.to_string())).unwrap();
        assert!(out.ok, "{out:?}");
    }
    assert_eq!(spawned.load(Ordering::SeqCst), 1, "три операции — один подъём (один пароль)");
    assert_eq!(b.lines("run "), vec![YT_DLP_RUN; 3], "журнал argv:\n{}", b.journal());
    assert!(progress.iter().any(|l| l.contains("pacman -S")), "ход операции не дошёл до окна: {progress:?}");
    assert!(c.has_session());
    c.shutdown();
    b.wait_for("exit 0");
}

/// Закрытие окна (EOF на сокете) → помощник выходит сам, с кодом 0.
#[test]
fn window_closed_eof_helper_exits() {
    let b = Bench::new("eof");
    let (mut c, _) = client(&b, Setup::default());
    request(&mut c, &install("yt-dlp")).unwrap();
    drop(c); // Drop окна = shutdown сеанса
    b.wait_for("exit 0");
    assert!(b.journal().contains("end Revoked(WindowClosed)"), "{}", b.journal());
}

/// Окно закрыли посреди транзакции: помощник её доводит и только потом
/// выходит (З4 — EOF не обрывает pacman).
#[test]
fn eof_mid_operation_is_finished() {
    let b = Bench::new("eof-mid");
    let (mut c, _) = client(&b, Setup::default());
    b.put("hold", "");
    let pending = c.submit(&install("yt-dlp"), Vec::new()).unwrap();
    b.wait_for("held pacman -S");
    c.shutdown();
    std::thread::sleep(Duration::from_millis(200));
    assert!(b.exits().is_empty(), "помощник вышел, не доведя операцию:\n{}", b.journal());
    std::fs::remove_file(b.dir.join("hold")).unwrap();
    b.wait_for("exit 0");
    assert_eq!(b.lines("run "), vec![YT_DLP_RUN]);
    assert!(b.journal().contains("ops 1"), "{}", b.journal());
    // Окно ушло раньше итога: исход ему неизвестен, но не «не исполнено».
    assert!(matches!(pending.wait(&mut |_| {}), Err(ClientError::SessionLost { .. } | ClientError::Closed)));
}

fn revoked_by(b: &Bench, c: &mut Client, spawned: &AtomicU32, reason: &str, act: impl FnOnce()) {
    request(c, &install("yt-dlp")).unwrap();
    // Первый опрос logind — точка отсчёта блокировки: дать ему пройти.
    std::thread::sleep(Duration::from_millis(250));
    act();
    b.wait_for(&format!("право погашено: {reason}"));
    b.wait_for("exit 0");
    wait_no_session(c);
    let runs_before = b.lines("run ").len();
    // Новая команда старым правом не исполняется: нужен новый подъём —
    // новое окно пароля.
    b.set_session(uid(), true, false);
    request(c, &install("yt-dlp")).unwrap();
    assert_eq!(spawned.load(Ordering::SeqCst), 2, "после отзыва — новый пароль");
    assert_eq!(b.lines("run ").len(), runs_before + 1);
}

/// Блокировка экрана (`LockedHint` нет → да) гасит право.
#[test]
fn screen_lock_revokes() {
    let b = Bench::new("lock");
    let (mut c, spawned) = client(&b, Setup::default());
    revoked_by(&b, &mut c, &spawned, "screen-locked", || b.set_session(uid(), true, true));
}

/// 15 минут без действий (подменные часы) гасят право.
#[test]
fn idle_fifteen_minutes_revokes() {
    let b = Bench::new("idle");
    let (mut c, spawned) = client(&b, Setup::default());
    revoked_by(&b, &mut c, &spawned, "idle", || b.put("clock_ms", &(15 * 60 * 1000 + 1000).to_string()));
}

/// 14 минут простоя — право ещё живо.
#[test]
fn idle_under_limit_keeps_session() {
    let b = Bench::new("idle-under");
    let (mut c, spawned) = client(&b, Setup::default());
    request(&mut c, &install("yt-dlp")).unwrap();
    b.put("clock_ms", &(14 * 60 * 1000).to_string());
    std::thread::sleep(Duration::from_millis(300));
    request(&mut c, &install("yt-dlp")).unwrap();
    assert_eq!(spawned.load(Ordering::SeqCst), 1);
    assert!(!b.journal().contains("право погашено"), "{}", b.journal());
}

/// Сон машины (скачок `BOOTTIME − MONOTONIC`) гасит право.
#[test]
fn sleep_revokes() {
    let b = Bench::new("sleep");
    let (mut c, spawned) = client(&b, Setup::default());
    revoked_by(&b, &mut c, &spawned, "slept", || b.put("slept_ms", "5000"));
}

/// Переключили пользователя (сеанс неактивен) — право погашено.
#[test]
fn inactive_session_revokes() {
    let b = Bench::new("inactive");
    let (mut c, spawned) = client(&b, Setup::default());
    revoked_by(&b, &mut c, &spawned, "session-inactive", || b.set_session(uid(), false, false));
}

/// Отзыв посреди операции: идущая доводится, после неё помощник выходит.
#[test]
fn revoke_during_operation_finishes_it() {
    let b = Bench::new("lock-mid");
    let (mut c, _) = client(&b, Setup::default());
    request(&mut c, &install("yt-dlp")).unwrap();
    std::thread::sleep(Duration::from_millis(250));
    b.put("hold", "");
    let _pending = c.submit(&install("yt-dlp"), Vec::new()).unwrap();
    b.wait_for("held pacman -S");
    b.set_session(uid(), true, true);
    b.wait_for("право погашено: screen-locked");
    std::thread::sleep(Duration::from_millis(200));
    assert!(b.exits().is_empty(), "вышел посреди операции:\n{}", b.journal());
    std::fs::remove_file(b.dir.join("hold")).unwrap();
    b.wait_for("exit 0");
    assert_eq!(b.lines("run ").len(), 2, "{}", b.journal());
}

/// Вечное (ядро, mesa, glibc…) и пакеты текущего стола помощник не снимает,
/// ни одной команды не запускается; отказ сеанс не роняет.
#[test]
fn vital_and_current_desktop_packages_refused() {
    let b = Bench::new("vital");
    let (mut c, spawned) = client(&b, Setup::default());
    for pkg in ["linux-cachyos", "mesa", "glibc", "systemd"] {
        match request(&mut c, &remove(&[pkg])) {
            Err(ClientError::Refused { message_key }) => assert_eq!(message_key, "refused.vital", "{pkg}"),
            other => panic!("{pkg}: ждали отказ, пришло {other:?}"),
        }
    }
    let kde = validate::desktop_packages(PkgFamily::Arch, "kde");
    let own = kde.first().expect("у KDE в базе есть пакеты").clone();
    match request(&mut c, &remove(&["gnome-shell", &own])) {
        Err(ClientError::Refused { message_key }) => assert_eq!(message_key, "refused.current-desktop-package"),
        other => panic!("{own}: ждали отказ, пришло {other:?}"),
    }
    // Не из таблицы — тоже отказ (окно не может назвать что угодно).
    match request(&mut c, &install("sddm")) {
        Err(ClientError::Refused { message_key }) => assert_eq!(message_key, "refused.not-in-table"),
        other => panic!("ждали отказ, пришло {other:?}"),
    }
    assert!(b.lines("run ").is_empty(), "отказ запустил команду:\n{}", b.journal());
    // Чужой стол снять можно — тем же сеансом.
    assert!(request(&mut c, &remove(&["gnome-shell"])).unwrap().ok);
    assert_eq!(b.lines("run "), vec!["run pacman -Rns --noconfirm gnome-shell"]);
    assert_eq!(spawned.load(Ordering::SeqCst), 1);
}

/// Помощник другой сборки, узнанный ДО пароля (`--build-id`): пароль не
/// спрашивается, сеанс не поднимается, путь «как сейчас» разрешён.
#[test]
fn other_build_found_before_password() {
    let b = Bench::new("build-probe");
    let setup = Setup { env: vec![("REXON_STAND_BUILD".into(), "0.0.0+чужой".into())], ..Setup::default() };
    let (mut c, spawned) = client(&b, setup);
    match request(&mut c, &install("yt-dlp")) {
        Err(e @ ClientError::HelperOtherBuild { .. }) => assert!(e.raw_fallback_allowed()),
        other => panic!("ждали HelperOtherBuild, пришло {other:?}"),
    }
    assert_eq!(spawned.load(Ordering::SeqCst), 0, "пароль спрошен впустую");
}

/// Другая сборка, узнанная только в рукопожатии (помощник без
/// `--build-id`): `BuildMismatch`, липкий — второго пароля не будет, сырого
/// пути тоже (З5).
#[test]
fn build_mismatch_in_handshake_is_sticky() {
    let b = Bench::new("build-hello");
    let setup =
        Setup { env: vec![("REXON_STAND_BUILD".into(), "0.0.0+чужой".into())], probe: false, ..Setup::default() };
    let (mut c, spawned) = client(&b, setup);
    for _ in 0..2 {
        match request(&mut c, &install("yt-dlp")) {
            Err(e @ ClientError::BuildMismatch { .. }) => assert!(!e.raw_fallback_allowed()),
            other => panic!("ждали BuildMismatch, пришло {other:?}"),
        }
    }
    assert_eq!(spawned.load(Ordering::SeqCst), 1, "после BuildMismatch помощник не поднимается");
    b.wait_for("exit 5");
    assert!(b.lines("run ").is_empty());
}

/// Сеанс logind не того пользователя — рукопожатие отвергнуто, ничего не
/// исполнено.
#[test]
fn foreign_logind_session_rejected() {
    let b = Bench::new("foreign-session");
    b.set_session(uid() + 1, true, false);
    let (mut c, _) = client(&b, Setup::default());
    assert!(request(&mut c, &install("yt-dlp")).is_err());
    b.wait_for("exit 5");
    assert!(b.journal().contains("рукопожатие отвергнуто: session_mismatch"), "{}", b.journal());
    assert!(b.lines("run ").is_empty());
}

/// Чужой процесс не находит канала:
/// * помощник, у которого собеседник по сокету — не его родитель (посредник
///   держит пару), не говорит ни слова и выходит;
/// * у сокета нет имени (только `socketpair`), и программы, запущенные окном
///   во время сеанса, его не наследуют.
#[test]
fn stranger_cannot_reach_channel() {
    let b = Bench::new("stranger");
    let (mut c, spawned) = client(&b, Setup { launch: Launch::Forked, ..Setup::default() });
    match request(&mut c, &install("yt-dlp")) {
        Err(ClientError::SessionLost { outcome_unknown: false, .. }) => {}
        other => panic!("ждали обрыв до рукопожатия, пришло {other:?}"),
    }
    assert_eq!(spawned.load(Ordering::SeqCst), 1);
    b.wait_for("exit 4");
    assert!(b.journal().contains("чужой собеседник на сокете"), "{}", b.journal());
    assert!(b.lines("run ").is_empty());

    // Обычный сеанс: дочка окна не видит ни одного сокета окна.
    let b2 = Bench::new("stranger-fd");
    let (mut c2, _) = client(&b2, Setup::default());
    request(&mut c2, &install("yt-dlp")).unwrap();
    let own: Vec<String> = sockets_of("/proc/self/fd");
    assert!(!own.is_empty(), "у окна должен быть сокет сеанса");
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg("for f in /proc/$$/fd/*; do readlink \"$f\"; done")
        .output()
        .unwrap();
    let child = String::from_utf8_lossy(&out.stdout);
    for s in &own {
        assert!(!child.lines().any(|l| l == s), "дочка окна унаследовала {s}:\n{child}");
    }
    // Имени у пары нет: в /proc/net/unix у этих сокетов пустой путь.
    let unix = std::fs::read_to_string("/proc/net/unix").unwrap_or_default();
    for s in &own {
        let inode = s.trim_start_matches("socket:[").trim_end_matches(']');
        for line in unix.lines().filter(|l| l.split_whitespace().nth(6) == Some(inode)) {
            assert_eq!(line.split_whitespace().count(), 7, "у сокета сеанса есть имя: {line}");
        }
    }
}

fn sockets_of(dir: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| std::fs::read_link(e.path()).ok())
                .map(|p| p.to_string_lossy().into_owned())
                .filter(|p| p.starts_with("socket:"))
                .collect()
        })
        .unwrap_or_default()
}

/// Стенд без каталога — не помощник: отказ, а не «сухой» root.
#[test]
fn stand_without_dir_refuses() {
    let st = Command::new(STAND).env_remove("REXON_STAND_DIR").stdin(Stdio::null()).status().unwrap();
    assert_eq!(st.code(), Some(rexon_core::privilege::helper::EXIT_USAGE));
}

/// Шрифт «для всех» сеансом прав: копия под корнем стенда в
/// `/usr/local/share/fonts`, `fc-query` — по копии root (рабочий каталог),
/// `fc-cache` каталога; занятое имя и не-шрифт — отказ без записи; удаление
/// шрифта пакета — отказ проверки, своего файла — удалён. Один пароль.
#[test]
fn shared_font_install_and_remove() {
    use std::os::unix::fs::PermissionsExt;
    let b = Bench::new("fonts");
    let root = b.dir.join("root");
    for p in ["fc-query", "fc-cache"] {
        let f = root.join("usr/bin").join(p);
        std::fs::write(&f, "").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    b.put("fc-query", "Stand Sans\n");
    let (mut c, spawned) = client(&b, Setup::default());
    let font = |tag: &str, body: &str| -> OwnedFd {
        let p = b.dir.join(format!("user-{tag}.ttf"));
        std::fs::write(&p, body).unwrap();
        OwnedFd::from(std::fs::File::open(&p).unwrap())
    };
    let op = |name: &str| PrivOp::FontInstall {
        fonts: vec![FontFile { source: LocalFile { slot: 0, label: format!("/home/stand/{name}") }, name: name.into() }],
    };
    let out = c.request(&op("Stand.ttf"), vec![font("a", "шрифт")], &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}\n{}", b.journal());
    let dest = root.join("usr/local/share/fonts/Stand.ttf");
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "шрифт");
    assert!(b.journal().contains("copy /usr/local/share/fonts/Stand.ttf"), "{}", b.journal());
    assert!(b.lines("run ").contains(&"run fc-cache -f /usr/local/share/fonts".to_string()), "{}", b.journal());
    let probes = b.lines("probe fc-query ");
    assert!(probes.iter().all(|l| l.contains("/var/tmp/")), "fc-query не по копии root: {probes:?}");

    // Занятое имя — отказ, файл прежний.
    match c.request(&op("Stand.ttf"), vec![font("b", "другой")], &mut |_| {}) {
        Err(ClientError::Refused { message_key }) => assert_eq!(message_key, "refused.font-exists"),
        other => panic!("ждали отказ, пришло {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "шрифт");
    // Не шрифт (fontconfig молчит) — отказ, файла нет.
    std::fs::remove_file(b.dir.join("fc-query")).unwrap();
    match c.request(&op("Evil.ttf"), vec![font("c", "#!/bin/sh")], &mut |_| {}) {
        Err(ClientError::Refused { message_key }) => assert_eq!(message_key, "refused.not-a-font"),
        other => panic!("ждали отказ, пришло {other:?}"),
    }
    assert!(!root.join("usr/local/share/fonts/Evil.ttf").exists());

    // Удаление: шрифт пакета — отказ проверки; свой — удалён.
    let rm = |f: &str| PrivOp::FontRemove { files: vec![f.into()] };
    match request(&mut c, &rm("/usr/share/fonts/noto/N.ttf")) {
        Err(ClientError::Refused { message_key }) => assert_eq!(message_key, "refused.outside-root"),
        other => panic!("ждали отказ, пришло {other:?}"),
    }
    let out = request(&mut c, &rm("/usr/local/share/fonts/Stand.ttf")).unwrap();
    assert!(out.ok, "{out:?}\n{}", b.journal());
    assert!(!dest.exists());
    assert!(b.journal().contains("rm /usr/local/share/fonts/Stand.ttf"), "{}", b.journal());
    assert_eq!(spawned.load(Ordering::SeqCst), 1, "всё одним сеансом — один пароль");
    c.shutdown();
    b.wait_for("exit 0");
}
