//! Стенд уровня 3 («один пароль на запуск», см. docs/testing.md): НАСТОЯЩИЙ
//! `rexon-priv-helper` с настоящим `LiveRootIo` от «root» — в пространстве
//! имён пользователя (`unshare -r --mount`), без root на хосте. Подмены
//! `/usr/bin` (pacman, loginctl, systemctl — скрипты стенда), `/etc`,
//! `/usr/share`, `/var/tmp` ставит `packaging/test/priv-ns-enter.sh`
//! bind-монтированием, видны они только помощнику. Проверяется то, что на
//! уровне 2 «сухое»: настоящие файловые операции (права, `openat2`,
//! ссылки при распаковке и на месте приёмника), окружение и дескрипторы
//! программ помощника, EOF посреди транзакции, `PR_SET_PDEATHSIG`.
//!
//! Нет `unshare` или ядро не даёт пространства имён пользователю — тесты
//! ПРОПУСКАЮТСЯ с причиной в выводе (`--nocapture`), а не падают.
//!
//! Здесь же сторож «стенд не попадает в релиз».

use std::io::Write;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rexon_core::privilege::client::{
    Client, ClientError, FileMeta, HelperChild, HelperFs, RealFs, Spawner, BUILD_ID_FLAG, PKEXEC,
};
use rexon_core::privilege::op::{FontFile, LocalFile, PkgSource, PrivOp, SystemThemeKind, SystemThemeOp, ThemeFamily};
use rexon_core::privilege::proto::{self, ClientMsg, Hello, HelperMsg};
use rexon_core::privilege::unpack;
use rexon_core::system::PkgFamily;

const HELPER: &str = env!("CARGO_BIN_EXE_rexon-priv-helper");
const WAIT: Duration = Duration::from_secs(15);

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn enter_script() -> PathBuf {
    repo().join("packaging/test/priv-ns-enter.sh")
}

// ── каталог стенда ──────────────────────────────────────────────────────

struct Ns {
    t: PathBuf,
}

fn script(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

impl Ns {
    fn new(name: &str) -> Self {
        let t = std::env::temp_dir().join(format!("rexon-priv-userns-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        for d in ["realbin", "fakebin", "etc", "share", "vartmp", "nojournal", "outside"] {
            std::fs::create_dir_all(t.join(d)).unwrap();
        }
        std::fs::set_permissions(t.join("vartmp"), std::fs::Permissions::from_mode(0o1777)).unwrap();
        std::fs::write(t.join("etc/os-release"), "ID=arch\nNAME=\"Arch Linux\"\n").unwrap();
        std::fs::write(t.join("etc/passwd"), "root:x:0:0:root:/root:/bin/sh\n").unwrap();
        std::fs::write(t.join("etc/group"), "root:x:0:\n").unwrap();
        std::fs::write(t.join("session"), "User=0\nActive=yes\nLockedHint=no\nDesktop=KDE\n").unwrap();
        let ts = t.to_string_lossy().into_owned();
        let sh = format!("#!{ts}/realbin/sh\nR={ts}/realbin\nT={ts}\n");
        script(
            &t.join("fakebin/loginctl"),
            &format!("{sh}[ \"$1\" = show-session ] && exec $R/cat $T/session\nexit 1\n"),
        );
        script(&t.join("fakebin/systemctl"), &format!("{sh}exit 0\n"));
        // pacman стенда: argv, окружение и дескрипторы — в файлы стенда;
        // `hold` держит «транзакцию», пока файл есть.
        script(
            &t.join("fakebin/pacman"),
            &format!(
                "{sh}echo \"start $*\" >> $T/pacman.log\n\
                 $R/env > $T/pacman.env\n\
                 for f in /proc/$$/fd/*; do $R/readlink \"$f\"; done > $T/pacman.fds\n\
                 while [ -e $T/hold ]; do $R/sleep 0.05; done\n\
                 echo \"done $*\" >> $T/pacman.log\n\
                 echo 'pacman стенда: готово'\n"
            ),
        );
        Ns { t }
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.t.join(name)).unwrap_or_default()
    }

    fn wait_for(&self, file: &str, needle: &str) {
        let start = Instant::now();
        while start.elapsed() < WAIT {
            if self.read(file).contains(needle) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("в {file} нет «{needle}» за {WAIT:?}:\n{}\n— stderr помощника:\n{}", self.read(file), self.read("stderr.log"));
    }
}

/// Команда запуска помощника в пространстве имён (то, что делает pkexec,
/// плюс подмены стенда).
fn ns_command(t: &Path, helper_args: &[String]) -> Command {
    let mut cmd = Command::new("unshare");
    cmd.args(["-r", "--mount"])
        .arg(enter_script())
        .arg(t)
        .arg(HELPER)
        .args(helper_args)
        .env("PKEXEC_UID", "0")
        .stderr(Stdio::from(
            std::fs::OpenOptions::new().create(true).append(true).open(t.join("stderr.log")).unwrap(),
        ));
    cmd
}

impl Drop for Ns {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.t);
        }
    }
}

/// Пространство имён пользователя с bind-монтированием доступно? `Err` —
/// причина пропуска.
fn userns_ready() -> Result<(), String> {
    // Свой каталог пробы у каждого теста: тесты идут параллельно.
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let probe = std::env::temp_dir().join(format!("rexon-priv-userns-probe-{}-{n}", std::process::id()));
    let _ = std::fs::create_dir_all(probe.join("a"));
    let _ = std::fs::create_dir_all(probe.join("b"));
    let out = Command::new("unshare")
        .args(["-r", "--mount", "mount", "--bind"])
        .arg(probe.join("a"))
        .arg(probe.join("b"))
        .stdin(Stdio::null())
        .output();
    let _ = std::fs::remove_dir_all(&probe);
    match out {
        Err(e) => Err(format!("нет программы unshare (util-linux): {e}")),
        Ok(o) if !o.status.success() => Err(format!(
            "ядро не дало пространство имён пользователя или mount в нём ({}): {}",
            o.status,
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Ok(_) => Ok(()),
    }
}

macro_rules! require_userns {
    () => {
        if let Err(why) = userns_ready() {
            eprintln!("ПРОПУЩЕНО (стенд уровня 3): {why}");
            return;
        }
    };
}

// ── окно ────────────────────────────────────────────────────────────────

struct NsSpawner {
    t: PathBuf,
    spawned: Arc<AtomicU32>,
}

struct Child(std::process::Child);

impl HelperChild for Child {
    fn wait(&mut self) -> std::io::Result<Option<i32>> {
        self.0.wait().map(|st| st.code())
    }
}

impl Spawner for NsSpawner {
    fn spawn(&mut self, argv: &[String], helper_end: OwnedFd) -> std::io::Result<Box<dyn HelperChild>> {
        assert_eq!(argv[0], PKEXEC);
        assert_eq!(argv[1], HELPER);
        self.spawned.fetch_add(1, Ordering::SeqCst);
        let out = helper_end.try_clone()?;
        let mut cmd = ns_command(&self.t, &argv[2..]);
        cmd.stdin(Stdio::from(helper_end)).stdout(Stdio::from(out));
        let child = cmd.spawn();
        drop(cmd);
        Ok(Box::new(Child(child?)))
    }
}

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

fn client(ns: &Ns) -> (Client, Arc<AtomicU32>) {
    let spawned = Arc::new(AtomicU32::new(0));
    let c = Client::new(
        Box::new(NsSpawner { t: ns.t.clone(), spawned: Arc::clone(&spawned) }),
        Box::new(RootLookingFs),
        proto::build_id(),
        Some("2".into()),
    )
    .with_candidates(vec![PathBuf::from(HELPER)]);
    (c, spawned)
}

fn install_yt_dlp() -> PrivOp {
    PrivOp::PkgInstall { family: PkgFamily::Arch, packages: vec!["yt-dlp".into()], source: PkgSource::YtDlp }
}

fn sddm_install(theme: &str) -> PrivOp {
    PrivOp::SystemTheme(SystemThemeOp::Install {
        kind: SystemThemeKind::Sddm,
        theme: theme.into(),
        source: LocalFile { slot: 0, label: format!("/home/stand/{theme}") },
        family: ThemeFamily::Arch,
    })
}

// ── ustar вручную (для злых архивов: pack_tree их не делает) ─────────────

fn ustar(entries: &[(&str, u8, u32, &[u8], &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, flag, mode, data, link) in entries {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..108].copy_from_slice(format!("{mode:07o}\0").as_bytes());
        h[108..116].copy_from_slice(b"0000000\0");
        h[116..124].copy_from_slice(b"0000000\0");
        h[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
        h[136..148].copy_from_slice(b"00000000000\0");
        h[156] = *flag;
        h[157..157 + link.len()].copy_from_slice(link.as_bytes());
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    out
}

fn memfd(bytes: &[u8]) -> OwnedFd {
    // SAFETY: литерал с NUL; новый дескриптор — наш.
    let fd = unsafe { libc::memfd_create(c"stand-tar".as_ptr(), libc::MFD_CLOEXEC) };
    assert!(fd >= 0);
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut f = std::fs::File::from(fd.try_clone().unwrap());
    f.write_all(bytes).unwrap();
    // Смещение у копий дескриптора общее: читать помощник начнёт с начала.
    use std::io::Seek;
    f.seek(std::io::SeekFrom::Start(0)).unwrap();
    fd
}

fn mode(p: &Path) -> u32 {
    std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
}

// ── сценарии ────────────────────────────────────────────────────────────

fn theme_apply(kind: SystemThemeKind, theme: &str) -> PrivOp {
    PrivOp::SystemTheme(SystemThemeOp::Apply { kind, theme: theme.into(), family: ThemeFamily::Arch })
}

/// «Применить» у уже установленных системных тем настоящим помощником, без
/// root: SDDM — тема меняется во ВСЕХ файлах `/etc`, где задана (и в
/// `/etc/sddm.conf`, который перекрывает `sddm.conf.d`), прочие строки целы;
/// файл-ссылка — отказ без записи. Plymouth — тема выбрана и образ ядра
/// пересобран; пересборка не удалась — отказ и прежняя тема в настройках.
/// Всё одним сеансом — один пароль.
#[test]
fn real_helper_applies_installed_system_themes() {
    require_userns!();
    let ns = Ns::new("apply");
    let t = &ns.t;
    let ts = t.to_string_lossy().into_owned();
    let sh = format!("#!{ts}/realbin/sh\nR={ts}/realbin\nT={ts}\n");
    // Plymouth стенда: без аргументов — текущая тема, с аргументом — выбор.
    script(
        &t.join("fakebin/plymouth-set-default-theme"),
        &format!("{sh}if [ $# -eq 0 ]; then exec $R/cat $T/plymouth-current; fi\necho \"$1\" > $T/plymouth-current\n"),
    );
    // Пересборщики образа: запуск — в журнал; `initrd-fail` — сбой.
    for p in ["mkinitcpio", "update-initramfs", "limine-mkinitcpio"] {
        script(
            &t.join("fakebin").join(p),
            &format!("{sh}echo \"{p} $*\" >> $T/initrd.log\n[ ! -e $T/initrd-fail ]\n"),
        );
    }
    std::fs::write(t.join("plymouth-current"), "bgrt\n").unwrap();
    for (dir, file, body) in [
        ("sddm/themes/stand-theme", "Main.qml", "Item {}\n"),
        ("plymouth/themes/stand", "stand.plymouth", "[Plymouth Theme]\nModuleName=script\n"),
        ("plymouth/themes/stand2", "stand2.plymouth", "[Plymouth Theme]\nModuleName=two-step\n"),
    ] {
        std::fs::create_dir_all(t.join("share").join(dir)).unwrap();
        std::fs::write(t.join("share").join(dir).join(file), body).unwrap();
    }
    std::fs::create_dir_all(t.join("etc/sddm.conf.d")).unwrap();
    std::fs::write(t.join("etc/sddm.conf.d/kde_settings.conf"), "[Theme]\nCurrent=breeze\nCursorTheme=x\n").unwrap();
    std::fs::write(t.join("etc/sddm.conf"), "[General]\nNumlock=on\n\n[Theme]\nCurrent=breeze\n").unwrap();
    let (mut c, spawned) = client(&ns);

    // 1. SDDM: оба файла, свой файл не нужен.
    let out = c.request(&theme_apply(SystemThemeKind::Sddm, "stand-theme"), Vec::new(), &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}\n{}", ns.read("stderr.log"));
    assert_eq!(ns.read("etc/sddm.conf"), "[General]\nNumlock=on\n\n[Theme]\nCurrent=stand-theme\n");
    assert_eq!(ns.read("etc/sddm.conf.d/kde_settings.conf"), "[Theme]\nCurrent=stand-theme\nCursorTheme=x\n");
    assert!(!t.join("etc/sddm.conf.d/zz-rexon-theme.conf").exists());

    // 2. SDDM: `/etc/sddm.conf` — ссылка наружу: отказ, цель не тронута.
    std::fs::write(t.join("outside/real.conf"), "[Theme]\nCurrent=breeze\n").unwrap();
    std::fs::remove_file(t.join("etc/sddm.conf")).unwrap();
    std::os::unix::fs::symlink(t.join("outside/real.conf"), t.join("etc/sddm.conf")).unwrap();
    let r = c.request(&theme_apply(SystemThemeKind::Sddm, "stand-theme"), Vec::new(), &mut |_| {});
    assert!(!matches!(r, Ok(ref o) if o.ok), "{r:?}");
    assert_eq!(ns.read("outside/real.conf"), "[Theme]\nCurrent=breeze\n");

    // 3. Plymouth: выбрана и образ пересобран.
    let out = c.request(&theme_apply(SystemThemeKind::Plymouth, "stand"), Vec::new(), &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}\n{}", ns.read("stderr.log"));
    assert_eq!(ns.read("plymouth-current"), "stand\n");
    assert!(!ns.read("initrd.log").is_empty(), "образ не пересобирался:\n{}", ns.read("stderr.log"));

    // 4. Plymouth: пересборка не удалась — отказ, прежняя тема на месте.
    std::fs::write(t.join("initrd-fail"), "").unwrap();
    let r = c.request(&theme_apply(SystemThemeKind::Plymouth, "stand2"), Vec::new(), &mut |_| {});
    assert!(!matches!(r, Ok(ref o) if o.ok), "{r:?}");
    assert_eq!(ns.read("plymouth-current"), "stand\n", "прежняя тема не вернулась:\n{}", ns.read("stderr.log"));

    assert_eq!(spawned.load(Ordering::SeqCst), 1, "всё одним сеансом — один пароль");
}

/// Настоящий помощник, один сеанс: pacman стенда с чистым окружением и без
/// сокета окна; тема SDDM ложится настоящими файлами с нормальными правами,
/// ссылка на месте приёмника заменяется, а не пишется насквозь; архивы со
/// ссылкой и `..` отвергаются, не тронув систему; рабочие каталоги убраны.
#[test]
fn real_helper_file_operations() {
    require_userns!();
    let ns = Ns::new("files");
    let (mut c, spawned) = client(&ns);

    // 1. pacman: argv, окружение, дескрипторы.
    let out = c.request(&install_yt_dlp(), Vec::new(), &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}\n{}", ns.read("stderr.log"));
    assert!(ns.read("pacman.log").contains("done -S --needed --noconfirm yt-dlp"), "{}", ns.read("pacman.log"));
    let env = ns.read("pacman.env");
    assert!(env.lines().any(|l| l == "PATH=/usr/bin:/usr/sbin"), "{env}");
    assert!(env.lines().any(|l| l == "LC_ALL=C"), "{env}");
    for bad in ["PKEXEC_UID=", "HOME=", "XDG_", "LD_", "REXON", "CARGO", "RUST"] {
        assert!(!env.lines().any(|l| l.starts_with(bad)), "окружение окна протекло в pacman ({bad}):\n{env}");
    }
    let fds = ns.read("pacman.fds");
    assert!(!fds.is_empty());
    assert!(!fds.contains("socket:"), "программа помощника видит сокет окна:\n{fds}");

    // 2. Тема SDDM: файлы пользователя с «плохими» правами.
    let src = ns.t.join("outside/src/stand-theme");
    std::fs::create_dir_all(src.join("faces")).unwrap();
    std::fs::write(src.join("Main.qml"), "Item {}\n").unwrap();
    std::fs::write(src.join("metadata.desktop"), "[SddmGreeterTheme]\nName=Стенд\n").unwrap();
    std::fs::write(src.join("faces/x.png"), b"png").unwrap();
    std::fs::set_permissions(src.join("Main.qml"), std::fs::Permissions::from_mode(0o600)).unwrap();
    // На месте приёмника — ссылка наружу: помощник обязан её заменить.
    let dest = ns.t.join("share/sddm/themes/stand-theme");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(ns.t.join("outside/victim"), "не трогать").unwrap();
    std::os::unix::fs::symlink(ns.t.join("outside/victim"), dest.join("Main.qml")).unwrap();
    let fd = unpack::pack_tree_memfd(&src).unwrap();
    let out = c.request(&sddm_install("stand-theme"), vec![fd], &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}\n{}", ns.read("stderr.log"));
    assert_eq!(std::fs::read_to_string(ns.t.join("outside/victim")).unwrap(), "не трогать", "запись сквозь ссылку");
    assert!(std::fs::symlink_metadata(dest.join("Main.qml")).unwrap().is_file());
    assert_eq!(std::fs::read_to_string(dest.join("Main.qml")).unwrap(), "Item {}\n");
    assert_eq!(mode(&dest.join("Main.qml")), 0o644);
    assert_eq!(mode(&dest.join("faces/x.png")), 0o644);
    assert_eq!(mode(&dest), 0o755);
    assert_eq!(mode(&dest.join("faces")), 0o755);

    // 3. Злые архивы: ссылка, `..`, абсолютный путь — отказ, приёмника нет.
    let cases: [(&str, Vec<u8>, &str); 3] = [
        ("evil-link", ustar(&[("Main.qml", b'2', 0o777, b"", "/etc/passwd")]), "refused.link"),
        ("evil-dotdot", ustar(&[("../escape.qml", b'0', 0o644, b"x", "")]), "refused.dot-dot"),
        ("evil-abs", ustar(&[("/etc/evil", b'0', 0o644, b"x", "")]), "refused.absolute"),
    ];
    for (theme, tar, key) in cases {
        match c.request(&sddm_install(theme), vec![memfd(&tar)], &mut |_| {}) {
            Err(ClientError::Refused { message_key }) => assert_eq!(message_key, key, "{theme}"),
            other => panic!("{theme}: ждали отказ {key}, пришло {other:?}"),
        }
        assert!(!ns.t.join("share/sddm/themes").join(theme).exists(), "{theme}: приёмник появился");
    }
    assert!(!ns.t.join("share/sddm/themes/escape.qml").exists());
    assert!(!ns.t.join("share/sddm/escape.qml").exists());

    // 4. Рабочие каталоги помощника за собой убраны.
    let left: Vec<_> = std::fs::read_dir(ns.t.join("vartmp")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(left.is_empty(), "остались рабочие каталоги: {left:?}");

    assert_eq!(spawned.load(Ordering::SeqCst), 1, "всё одним сеансом — один пароль");
    c.shutdown();
    ns.wait_for("stderr.log", "выход: Revoked(WindowClosed)");
}

/// Шрифты «для всех пользователей» настоящим помощником: файл ложится в
/// `/usr/local/share/fonts` читаемым для всех (0644), `fc-cache` каталога
/// зовётся; не-шрифт, занятое имя и ссылка на месте приёмника — отказ без
/// следа (жертва ссылки цела); удаляется только обычный файл каталога —
/// ссылка внутри него на шрифт пакета не удаляется, шрифт пакета цел.
#[test]
fn real_helper_shared_fonts() {
    require_userns!();
    let ns = Ns::new("fonts");
    let fonts = ns.t.join("local/share/fonts");
    std::fs::create_dir_all(ns.t.join("local/share")).unwrap();
    std::fs::create_dir_all(ns.t.join("share/fonts/noto")).unwrap();
    std::fs::write(ns.t.join("share/fonts/noto/N.ttf"), "FONT пакета").unwrap();
    let ts = ns.t.to_string_lossy().into_owned();
    let sh = format!("#!{ts}/realbin/sh\nR={ts}/realbin\nT={ts}\n");
    // fc-query стенда: «шрифт» — файл, начинающийся с FONT; путь — по копии
    // root (рабочий каталог в /var/tmp), не по пути пользователя.
    script(
        &ns.t.join("fakebin/fc-query"),
        &format!("{sh}echo \"$3\" >> $T/fc-query.log\n[ \"$($R/head -c 4 \"$3\")\" = FONT ] || exit 1\necho 'Stand Sans'\n"),
    );
    script(&ns.t.join("fakebin/fc-cache"), &format!("{sh}echo \"$*\" >> $T/fc-cache.log\n"));
    let (mut c, spawned) = client(&ns);
    let user_font = |name: &str, body: &[u8]| -> OwnedFd {
        let p = ns.t.join("outside").join(name);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        OwnedFd::from(std::fs::File::open(&p).unwrap())
    };
    let install = |names: &[&str]| PrivOp::FontInstall {
        fonts: names
            .iter()
            .enumerate()
            .map(|(i, n)| FontFile { source: LocalFile { slot: i as u8, label: format!("/home/stand/{n}") }, name: n.to_string() })
            .collect(),
    };

    // 1. Шрифт ложится; каталога ещё нет — заводится 0755.
    let out = c.request(&install(&["Мой шрифт.ttf"]), vec![user_font("a", b"FONT a")], &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}\n{}", ns.read("stderr.log"));
    let dest = fonts.join("Мой шрифт.ttf");
    assert_eq!(std::fs::read(&dest).unwrap(), b"FONT a");
    assert_eq!(mode(&dest), 0o644, "шрифт для всех должен читаться всеми");
    assert_eq!(mode(&fonts), 0o755);
    assert!(ns.read("fc-cache.log").contains("-f /usr/local/share/fonts"), "{}", ns.read("fc-cache.log"));
    assert!(ns.read("fc-query.log").starts_with("/var/tmp/"), "fc-query не по копии root: {}", ns.read("fc-query.log"));

    // 2. Отказы без следа: не шрифт, имя занято (в том числе вторым файлом
    //    операции — первый тоже не ложится), ссылка на месте приёмника.
    std::fs::write(ns.t.join("outside/victim"), "не трогать").unwrap();
    std::os::unix::fs::symlink(ns.t.join("outside/victim"), fonts.join("link.ttf")).unwrap();
    let cases: Vec<(PrivOp, Vec<OwnedFd>, &str)> = vec![
        (install(&["b.ttf"]), vec![user_font("b", b"#!/bin/sh\nid\n")], "refused.not-a-font"),
        (install(&["c.ttf", "Мой шрифт.ttf"]), vec![user_font("c", b"FONT c"), user_font("d", b"FONT d")], "refused.font-exists"),
        (install(&["link.ttf"]), vec![user_font("e", b"FONT e")], "refused.font-exists"),
        (install(&["../../share/fonts/x.ttf"]), vec![user_font("f", b"FONT f")], "refused.font-file"),
    ];
    for (op, fds, key) in cases {
        match c.request(&op, fds, &mut |_| {}) {
            Err(ClientError::Refused { message_key }) => assert_eq!(message_key, key, "{op:?}"),
            other => panic!("{op:?}: ждали отказ {key}, пришло {other:?}"),
        }
    }
    assert!(!fonts.join("b.ttf").exists() && !fonts.join("c.ttf").exists());
    assert_eq!(std::fs::read(&dest).unwrap(), b"FONT a", "чужой файл перезаписан");
    assert_eq!(std::fs::read_to_string(ns.t.join("outside/victim")).unwrap(), "не трогать", "запись сквозь ссылку");

    // 3. Удаление: ссылка внутри каталога на шрифт пакета и сам шрифт
    //    пакета — отказ; свой файл из подкаталога — удалён, подкаталог снят.
    std::fs::create_dir_all(fonts.join("sub")).unwrap();
    std::fs::write(fonts.join("sub/s.otf"), "FONT s").unwrap();
    std::os::unix::fs::symlink("/usr/share/fonts/noto/N.ttf", fonts.join("pkg.ttf")).unwrap();
    for (path, key) in [
        ("/usr/local/share/fonts/pkg.ttf", "refused.font-file"),
        ("/usr/share/fonts/noto/N.ttf", "refused.outside-root"),
    ] {
        let op = PrivOp::FontRemove { files: vec![path.into()] };
        match c.request(&op, Vec::new(), &mut |_| {}) {
            Err(ClientError::Refused { message_key }) => assert_eq!(message_key, key, "{path}"),
            other => panic!("{path}: ждали отказ {key}, пришло {other:?}"),
        }
    }
    assert_eq!(std::fs::read_to_string(ns.t.join("share/fonts/noto/N.ttf")).unwrap(), "FONT пакета");
    assert!(std::fs::symlink_metadata(fonts.join("pkg.ttf")).is_ok());
    let op = PrivOp::FontRemove {
        files: vec!["/usr/local/share/fonts/Мой шрифт.ttf".into(), "/usr/local/share/fonts/sub/s.otf".into()],
    };
    let out = c.request(&op, Vec::new(), &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}\n{}", ns.read("stderr.log"));
    assert!(!dest.exists() && !fonts.join("sub").exists() && fonts.is_dir());

    let left: Vec<_> = std::fs::read_dir(ns.t.join("vartmp")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(left.is_empty(), "остались рабочие каталоги: {left:?}");
    assert_eq!(spawned.load(Ordering::SeqCst), 1, "всё одним сеансом — один пароль");
    c.shutdown();
    ns.wait_for("stderr.log", "выход: Revoked(WindowClosed)");
}

/// Экран входа SDDM настоящим помощником: фон темы ложится файлом 0644 в
/// каталог темы, имя — в `theme.conf.user`; не картинка — отказ без следа;
/// «фон темы» снимает ключи и свою картинку. Перенос оформления: ключи — в
/// `/etc/sddm.conf.d/kde_settings.conf`, файлы — в дом `sddm` ТОЛЬКО через
/// `setpriv` (стенд пишет его argv в журнал); «Сбросить» — снимает их.
/// Всё одним сеансом.
#[test]
fn real_helper_changes_sddm_background_and_look() {
    require_userns!();
    let ns = Ns::new("sddm-greeter");
    let t = &ns.t;
    let ts = t.to_string_lossy().into_owned();
    let sh = format!("#!{ts}/realbin/sh\nR={ts}/realbin\nT={ts}\nPATH=$R\n");
    let theme = t.join("share/sddm/themes/breeze");
    std::fs::create_dir_all(&theme).unwrap();
    std::fs::write(theme.join("metadata.desktop"), "[SddmGreeterTheme]\nName=Breeze\nConfigFile=theme.conf\n").unwrap();
    std::fs::write(theme.join("theme.conf"), "[General]\ntype=image\nbackground=/usr/share/wallpapers/Next.png\n").unwrap();
    std::fs::create_dir_all(t.join("etc/sddm.conf.d")).unwrap();
    std::fs::write(t.join("etc/sddm.conf.d/kde_settings.conf"), "[Theme]\nCurrent=breeze\n").unwrap();
    let home = t.join("sddmhome");
    std::fs::create_dir_all(home.join(".cache/qml")).unwrap();
    let hs = home.to_string_lossy().into_owned();
    script(&t.join("fakebin/getent"), &format!("{sh}[ \"$2\" = sddm ] && echo 'sddm:x:955:955:SDDM:{hs}:/usr/bin/nologin' && exit 0\nexit 2\n"));
    // setpriv стенда: argv — в журнал, дальше исполняет команду после `--`.
    script(
        &t.join("fakebin/setpriv"),
        &format!("{sh}echo \"$*\" >> $T/setpriv.log\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n"),
    );
    let (mut c, spawned) = client(&ns);
    let user = |name: &str, body: &[u8]| -> OwnedFd {
        let p = t.join("outside").join(name);
        std::fs::write(&p, body).unwrap();
        OwnedFd::from(std::fs::File::open(&p).unwrap())
    };
    let bg = |image: bool| PrivOp::SddmBackground {
        theme: "breeze".into(),
        image: image.then(|| LocalFile { slot: 0, label: "/home/stand/фон.png".into() }),
    };

    // 1. PNG ложится; 2. JPEG его сменяет — прежний свой снят.
    let out = c.request(&bg(true), vec![user("a.png", b"\x89PNG\r\n\x1a\nPNG-stand")], &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}\n{}", ns.read("stderr.log"));
    assert_eq!(std::fs::read(theme.join("rexon-background.png")).unwrap(), b"\x89PNG\r\n\x1a\nPNG-stand");
    assert_eq!(mode(&theme.join("rexon-background.png")), 0o644);
    assert_eq!(ns.read("share/sddm/themes/breeze/theme.conf.user"), "[General]\nbackground=rexon-background.png\ntype=image\n");
    let out = c.request(&bg(true), vec![user("b.jpg", b"\xff\xd8\xff\xe0JPEG")], &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}");
    assert!(theme.join("rexon-background.jpg").is_file() && !theme.join("rexon-background.png").exists());
    // 3. Не картинка — отказ без следа.
    match c.request(&bg(true), vec![user("c.sh", b"#!/bin/sh\nid\n")], &mut |_| {}) {
        Err(ClientError::Refused { message_key }) => assert_eq!(message_key, "refused.login-image"),
        other => panic!("ждали отказ login-image, пришло {other:?}"),
    }
    assert!(ns.read("share/sddm/themes/breeze/theme.conf.user").contains("rexon-background.jpg"));
    // 4. Фон темы — ключи и своя картинка сняты, файл настроек темы цел.
    let out = c.request(&bg(false), Vec::new(), &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}");
    assert_eq!(ns.read("share/sddm/themes/breeze/theme.conf.user"), "", "опустевшая группа снята");
    assert!(!theme.join("rexon-background.jpg").exists());
    assert!(ns.read("share/sddm/themes/breeze/theme.conf").contains("Next.png"));
    // 4а. «Показывать часы»: ключ в `.user`, фон темы не трогается.
    let clock = |show: bool| PrivOp::SddmClock { theme: "breeze".into(), show };
    let out = c.request(&clock(false), Vec::new(), &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}\n{}", ns.read("stderr.log"));
    assert_eq!(ns.read("share/sddm/themes/breeze/theme.conf.user"), "[General]\nshowClock=false\n");
    let out = c.request(&clock(true), Vec::new(), &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}");
    assert!(ns.read("share/sddm/themes/breeze/theme.conf.user").contains("showClock=true"));
    match c.request(&PrivOp::SddmClock { theme: "nope".into(), show: false }, Vec::new(), &mut |_| {}) {
        Err(ClientError::Refused { message_key }) => assert_eq!(message_key, "refused.sddm-theme"),
        other => panic!("ждали отказ sddm-theme, пришло {other:?}"),
    }
    // 4б. WebP: тема на Qt 5 (нет QtVersion) — принимается, только если у
    // Qt 5 этой машины есть модуль WebP (каталог /usr/lib стенд не
    // подменяет — помощник смотрит настоящий).
    let webp = b"RIFF\x10\0\0\0WEBPVP8 stand";
    let has = rexon_core::system_plan::qt_has_webp_live(5);
    match c.request(&bg(true), vec![user("d.webp", webp)], &mut |_| {}) {
        Ok(out) => {
            assert!(has && out.ok, "{out:?}");
            assert!(theme.join("rexon-background.webp").is_file());
            let out = c.request(&bg(false), Vec::new(), &mut |_| {}).unwrap();
            assert!(out.ok && !theme.join("rexon-background.webp").exists());
        }
        Err(ClientError::Refused { message_key }) => {
            assert!(!has, "модуль WebP есть, а помощник отказал");
            assert_eq!(message_key, "refused.login-image");
        }
        Err(e) => panic!("{e:?}"),
    }

    // 5. Перенос оформления.
    let look = PrivOp::SddmLook {
        look: Some(rexon_core::system_plan::SddmLook {
            cursor_theme: Some("Bibata Modern Ice".into()),
            cursor_size: Some(24),
            dpi: Some(120),
            ..Default::default()
        }),
        files: vec![
            rexon_core::privilege::op::LookFile {
                source: LocalFile { slot: 0, label: "/home/stand/.config/kdeglobals".into() },
                name: "kdeglobals".into(),
            },
            rexon_core::privilege::op::LookFile {
                source: LocalFile { slot: 1, label: "/home/stand/.config/fontconfig/fonts.conf".into() },
                name: "fontconfig/fonts.conf".into(),
            },
        ],
    };
    let fds = vec![user("kdeglobals", b"[Colors:Window]\nBackgroundNormal=1,2,3\n"), user("fonts.conf", b"<fontconfig/>\n")];
    let out = c.request(&look, fds, &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}\n{}", ns.read("stderr.log"));
    let kde = ns.read("etc/sddm.conf.d/kde_settings.conf");
    assert!(kde.contains("Current=breeze") && kde.contains("CursorTheme=Bibata Modern Ice") && kde.contains("CursorSize=24"), "{kde}");
    assert!(kde.contains("-dpi 120"), "{kde}");
    assert_eq!(ns.read("sddmhome/.config/kdeglobals"), "[Colors:Window]\nBackgroundNormal=1,2,3\n");
    assert_eq!(ns.read("sddmhome/.config/fontconfig/fonts.conf"), "<fontconfig/>\n");
    assert_eq!(mode(&home.join(".config/kdeglobals")) & 0o044, 0o044, "файл приветствия читается");
    assert!(!home.join(".cache").exists(), "кэш приветствия стёрт");
    let log = ns.read("setpriv.log");
    assert!(log.lines().count() >= 4 && log.lines().all(|l| l.starts_with("--reuid=955 --regid=955 --clear-groups -- ")), "{log}");
    // 6. Сбросить.
    let out = c.request(&PrivOp::SddmLook { look: None, files: Vec::new() }, Vec::new(), &mut |_| {}).unwrap();
    assert!(out.ok, "{out:?}\n{}", ns.read("stderr.log"));
    let kde = ns.read("etc/sddm.conf.d/kde_settings.conf");
    assert!(kde.contains("Current=breeze") && !kde.contains("Cursor") && !kde.contains("-dpi"), "{kde}");
    assert!(!home.join(".config/kdeglobals").exists() && !home.join(".config/fontconfig/fonts.conf").exists());

    assert_eq!(spawned.load(Ordering::SeqCst), 1, "всё одним сеансом — один пароль");
    c.shutdown();
    ns.wait_for("stderr.log", "выход: Revoked(WindowClosed)");
}

/// Окно закрыто (EOF) посреди транзакции pacman: настоящий помощник её
/// доводит и только потом выходит.
#[test]
fn real_helper_finishes_transaction_after_eof() {
    require_userns!();
    let ns = Ns::new("eof");
    let (mut c, _) = client(&ns);
    std::fs::write(ns.t.join("hold"), "").unwrap();
    let pending = c.submit(&install_yt_dlp(), Vec::new()).unwrap();
    ns.wait_for("pacman.log", "start -S");
    c.shutdown();
    std::thread::sleep(Duration::from_millis(300));
    assert!(!ns.read("stderr.log").contains("выход:"), "вышел посреди транзакции:\n{}", ns.read("stderr.log"));
    std::fs::remove_file(ns.t.join("hold")).unwrap();
    ns.wait_for("pacman.log", "done -S --needed --noconfirm yt-dlp");
    ns.wait_for("stderr.log", "выход: Revoked(WindowClosed), операций исполнено 1");
    assert!(pending.wait(&mut |_| {}).is_err(), "окно ушло раньше итога");
}

/// `PR_SET_PDEATHSIG`: поток окна, поднявший помощника, умер, а сокет ещё
/// открыт (держит другой поток) — помощник всё равно выходит сам.
#[test]
fn real_helper_exits_on_parent_death_signal() {
    require_userns!();
    let ns = Ns::new("pdeath");
    let t = ns.t.clone();
    let (sock, mut child) = std::thread::spawn(move || {
        let (ours, theirs) = proto::seqpacket_pair().unwrap();
        let out = theirs.try_clone().unwrap();
        let mut cmd = ns_command(&t, &["--session".into(), "2".into()]);
        cmd.stdin(Stdio::from(theirs)).stdout(Stdio::from(out));
        let child = cmd.spawn().unwrap();
        drop(cmd);
        use std::os::fd::AsFd;
        proto::send_msg(ours.as_fd(), &ClientMsg::<PrivOp>::Hello(Hello { build_id: proto::build_id() })).unwrap();
        let welcome: Option<HelperMsg> = proto::recv_msg(ours.as_fd()).unwrap();
        assert!(matches!(welcome, Some(HelperMsg::Welcome { .. })), "{welcome:?}");
        (ours, child)
        // поток кончается здесь — PDEATHSIG помощника срабатывает
    })
    .join()
    .unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break st;
        }
        assert!(start.elapsed() < WAIT, "помощник не вышел по PDEATHSIG:\n{}", ns.read("stderr.log"));
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(0), "{}", ns.read("stderr.log"));
    assert!(ns.read("stderr.log").contains("выход: Revoked(WindowClosed)"), "{}", ns.read("stderr.log"));
    drop(sock); // сокет жил до конца: вышел помощник не по EOF
}

/// Отпечаток сборки настоящего помощника (без root и без канала) совпадает
/// с отпечатком окна той же сборки.
#[test]
fn real_helper_build_id_matches_window() {
    let out = Command::new(HELPER).arg(BUILD_ID_FLAG).env_clear().output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), proto::build_id());
}

/// ⛔ Стенд не попадает в релиз: бинарь стенда собирается только с feature
/// `stand`, библиотека и настоящий помощник от неё не зависят, пакет её не
/// включает; в настоящем помощнике нет ни строчки стенда.
#[test]
fn stand_code_stays_out_of_release() {
    let core = Path::new(env!("CARGO_MANIFEST_DIR"));
    let toml = std::fs::read_to_string(core.join("Cargo.toml")).unwrap();
    let bin = toml.split("[[bin]]").find(|b| b.contains("\"rexon-priv-stand\"")).expect("[[bin]] rexon-priv-stand");
    let bin = bin.split("\n[").next().unwrap();
    assert!(bin.contains("required-features = [\"stand\"]"), "бинарь стенда собирается без feature:\n{bin}");

    let stand_src = core.join("src/bin/rexon-priv-stand.rs");
    let mut stack = vec![core.join("src")];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") && p != stand_src {
                let text = std::fs::read_to_string(&p).unwrap();
                for needle in ["feature = \"stand\"", "REXON_STAND_DIR", "REXON_STAND_BUILD", "rexon-priv-stand"] {
                    assert!(!text.contains(needle), "код стенда вне бинаря стенда: {} ({needle})", p.display());
                }
            }
        }
    }

    let pkgbuild = std::fs::read_to_string(repo().join("packaging/aur/PKGBUILD")).unwrap();
    assert!(!pkgbuild.contains("rexon-priv-stand"), "пакет ставит бинарь стенда");
    assert!(!pkgbuild.contains("--features"), "пакет собирается с feature — проверить, что не stand");

    let helper = std::fs::read(HELPER).unwrap();
    for needle in [&b"REXON_STAND_DIR"[..], b"REXON_STAND_BUILD", b"rexon-priv-stand", b"priv-ns-enter"] {
        assert!(!helper.windows(needle.len()).any(|w| w == needle), "в rexon-priv-helper строка стенда");
    }
}
