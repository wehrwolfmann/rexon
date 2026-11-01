//! Шим `rexon-sudo-shim` (пункт Б10): настоящий бинарь шима, поддельный
//! сборщик AUR (скрипт bash вместо paru — настоящие paru/yay/makepkg здесь
//! не запускаются) и окно — посредник [`Broker`] этого процесса тестов.
//! Сеанс прав — [`Runner`] с поддельным клиентом: он считает подъёмы сеанса
//! (окна пароля) и записывает, что ушло бы помощнику. Ни root, ни pkexec.

use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rexon_core::privilege::aur::{Broker, ExecFn, Registration, SOCKET_DIR};
use rexon_core::privilege::client::{ClientError, Outcome};
use rexon_core::privilege::op::PrivOp;
use rexon_core::privilege::run::{Backend, Runner, Waiter};

const SHIM: &str = env!("CARGO_BIN_EXE_rexon-sudo-shim");
const WAIT: Duration = Duration::from_secs(60);

/// Что ушло «помощнику» и сколько раз поднимался сеанс.
#[derive(Default)]
struct Seen {
    ops: Mutex<Vec<(PrivOp, usize)>>,
    raises: AtomicU32,
}

struct FakeBackend {
    seen: Arc<Seen>,
    session: bool,
}

struct Done;

impl Waiter for Done {
    fn wait(self: Box<Self>, on_progress: &mut dyn FnMut(&str)) -> Result<Outcome, ClientError> {
        on_progress("(pacman помощника)");
        Ok(Outcome { ok: true, code: Some(0), message_key: None })
    }
}

impl Backend for FakeBackend {
    fn submit(&mut self, op: &PrivOp, fds: Vec<OwnedFd>) -> Result<Box<dyn Waiter>, ClientError> {
        if !self.session {
            // Подъём сеанса = одно окно пароля.
            self.seen.raises.fetch_add(1, Ordering::SeqCst);
            self.session = true;
        }
        self.seen.ops.lock().unwrap().push((op.clone(), fds.len()));
        Ok(Box::new(Done))
    }
    fn has_session(&self) -> bool {
        self.session
    }
    fn shutdown(&mut self) {
        self.session = false;
    }
}

/// Стенд: каталог «XDG_RUNTIME_DIR», посредник окна, поддельный сборщик.
struct Stand {
    root: PathBuf,
    broker: Arc<Broker>,
    seen: Arc<Seen>,
}

fn stand(tag: &str) -> Stand {
    let root = std::env::temp_dir().join(format!("rexon-b10-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("pkgs")).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let seen = Arc::new(Seen::default());
    let runner = Arc::new(Runner::new(Box::new(FakeBackend { seen: Arc::clone(&seen), session: false })));
    let exec: Box<ExecFn> = Box::new(move |op, fds, progress| runner.run(op, fds, progress));
    let broker = Broker::start(&root.join(SOCKET_DIR), std::process::id() as i32, exec).unwrap();
    // Поддельный paru: берёт `--sudo`/`--sudoflags` из своей строки и
    // зовёт «sudo» с каждой строкой файла команд — как paru/yay.
    let script = root.join("paru");
    std::fs::write(
        &script,
        r#"#!/bin/bash
while [ $# -gt 0 ]; do
  case "$1" in
    --sudo) S="$2"; shift 2;;
    --sudoflags) F="$2"; shift 2;;
    *) shift;;
  esac
done
while IFS= read -r line; do
  [ -z "$line" ] && continue
  "$S" $F $line >/dev/null 2>>"$FAKE_ERR"
  echo $? >> "$FAKE_OUT"
done < "$FAKE_CMDS"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    Stand { root, broker, seen }
}

impl Stand {
    fn package(&self, file: &str) -> String {
        let p = self.root.join("pkgs").join(file);
        std::fs::write(&p, b"pkg").unwrap();
        p.to_string_lossy().into_owned()
    }

    /// Запустить поддельный сборщик с меткой `reg` и командами `cmds`;
    /// итог — коды «sudo» по строкам и stderr шима.
    fn build(&self, reg: &Registration, cmds: &[String]) -> (Vec<i32>, String) {
        let out = self.root.join("out");
        let err = self.root.join("err");
        let list = self.root.join("cmds");
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&err);
        std::fs::write(&list, cmds.join("\n") + "\n").unwrap();
        let mut child = Command::new(self.root.join("paru"))
            .args(["-S", "--needed", "--noconfirm", "--sudo", SHIM, "--sudoflags", &reg.flag, "t"])
            .env("XDG_RUNTIME_DIR", &self.root)
            .env("FAKE_CMDS", &list)
            .env("FAKE_OUT", &out)
            .env("FAKE_ERR", &err)
            .spawn()
            .unwrap();
        wait_child(&mut child);
        let codes = std::fs::read_to_string(&out)
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().parse().unwrap())
            .collect();
        (codes, std::fs::read_to_string(&err).unwrap_or_default())
    }

    /// Шим напрямую из процесса тестов (не из сборщика).
    fn shim_direct(&self, args: &[&str]) -> i32 {
        let mut child = Command::new(SHIM)
            .args(args)
            .env("XDG_RUNTIME_DIR", &self.root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        wait_child(&mut child).code().unwrap_or(-1)
    }

    fn ops(&self) -> Vec<(PrivOp, usize)> {
        self.seen.ops.lock().unwrap().clone()
    }

    fn raises(&self) -> u32 {
        self.seen.raises.load(Ordering::SeqCst)
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn wait_child(child: &mut std::process::Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            return st;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("процесс не вышел за {WAIT:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn сборка_целиком_одним_подъёмом_сеанса() {
    let st = stand("ok");
    let t = st.package("t-1.0-1-x86_64.pkg.tar.zst");
    let reg = st.broker.register(&s(&["t"]), false);
    // Как paru: зависимости из репозитория, собранное, причины установки,
    // снятие сборочных. И как yay: флаги в другом порядке, `--config`.
    let (codes, err) = st.build(
        &reg,
        &[
            "pacman --sync --needed --noconfirm -- extra/cmake ninja".into(),
            format!("pacman --upgrade --noconfirm -- {t}"),
            "pacman --database --noconfirm --asdeps -- cmake".into(),
            "pacman --database --noconfirm --asexplicit -- t".into(),
            "pacman -R -s -u --noconfirm --config /etc/pacman.conf -- cmake ninja".into(),
        ],
    );
    assert_eq!(codes, vec![0, 0, 0, 0, 0], "{err}");
    let ops = st.ops();
    assert_eq!(ops.len(), 3, "{ops:?}");
    assert_eq!(ops[0].0, PrivOp::AurDeps { names: s(&["cmake", "ninja"]) });
    match &ops[1] {
        (PrivOp::AurInstallBuilt { packages, allowed, replace }, fds) => {
            assert_eq!(*fds, 1, "файл уходит дескриптором");
            assert_eq!(packages[0].label, t);
            assert_eq!(allowed, &s(&["t"]));
            assert!(!replace);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(ops[2].0, PrivOp::AurDropMake { names: s(&["cmake", "ninja"]) });
    assert_eq!(st.raises(), 1, "одна сборка — один подъём сеанса");

    // Следующая сборка при живом сеансе — без пароля.
    let reg2 = st.broker.register(&s(&["t"]), false);
    let (codes, err) = st.build(&reg2, &[format!("pacman -U --noconfirm -- {t}")]);
    assert_eq!(codes, vec![0], "{err}");
    assert_eq!(st.raises(), 1);
}

#[test]
fn запрещённое_отказ_без_пароля() {
    let st = stand("deny");
    let evil = st.package("evil-1-1-any.pkg.tar.zst");
    let t = st.package("t-1-1-any.pkg.tar.zst");
    let reg = st.broker.register(&s(&["t"]), false);
    let (codes, err) = st.build(
        &reg,
        &[
            format!("pacman -U --noconfirm -- {evil}"),           // чужое имя
            format!("pacman -U --ask=4 -- {t}"),                  // замена без согласия
            format!("pacman -U --overwrite=* -- {t}"),            // флаг вне списка
            format!("pacman -U --dbpath /tmp -- {t}"),
            "pacman -S --noconfirm --config /tmp/evil.conf -- cmake".into(),
            "pacman -R --noconfirm -- glibc".into(),              // не ставилось операцией
            "pacman -D --asexplicit -- linux".into(),
            "pacman -Syu --".into(),
            "bash -c id".into(),
            "pacman -S cmake".into(),                             // цели без `--`
        ],
    );
    assert_eq!(codes, vec![1; 10], "{err}");
    assert!(st.ops().is_empty(), "ничего не ушло помощнику: {:?}", st.ops());
    assert_eq!(st.raises(), 0, "на отказ пароль не спрашивался");
    assert!(err.contains("evil"), "причина видна в выводе сборщика: {err}");
}

#[test]
fn токен_одноразовый_и_привязан_к_сборщику() {
    let st = stand("token");
    let t = st.package("t-1-1-any.pkg.tar.zst");
    let reg = st.broker.register(&s(&["t"]), false);
    let flag = reg.flag.clone();

    // Чужой процесс (родитель шима — не сборщик с этой меткой) операцию
    // не привяжет.
    assert_eq!(st.shim_direct(&[&flag, "pacman", "-U", "--", &t]), 1);
    // Неизвестный токен и метка не по форме.
    let fake = format!("--rexon-op={}.{}", std::process::id(), "0".repeat(32));
    assert_eq!(st.shim_direct(&[&fake, "pacman", "-U", "--", &t]), 1);
    assert_eq!(st.shim_direct(&["pacman", "-U", "--", &t]), 1);
    assert!(st.ops().is_empty());

    // Сборка прошла и вышла.
    let (codes, err) = st.build(&reg, &[format!("pacman -U -- {t}")]);
    assert_eq!(codes, vec![0], "{err}");
    assert_eq!(st.ops().len(), 1);

    // После конца операции: та же метка у нового «сборщика» и напрямую — отказ.
    let (codes, _) = st.build(&reg, &[format!("pacman -U -- {t}")]);
    assert_eq!(codes, vec![1]);
    assert_eq!(st.shim_direct(&[&flag, "pacman", "-U", "--", &t]), 1);
    assert_eq!(st.ops().len(), 1, "после конца операции ничего не исполнено");
}

#[test]
fn продление_sudo_без_окна() {
    let st = stand("v");
    // yay с `sudoloop` зовёт `<sudo> -v` без флагов — успех, ничего не исполнено.
    assert_eq!(st.shim_direct(&["-v"]), 0);
    assert!(st.ops().is_empty());
}

#[test]
fn сокет_в_каталоге_0700() {
    let st = stand("dir");
    let dir = st.root.join(SOCKET_DIR);
    let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    assert!(Path::new(&st.broker.socket_path()).starts_with(&dir));
    // Каталог с правами шире — посредник не поднимается.
    let other = st.root.join("wide");
    std::fs::create_dir(&other).unwrap();
    std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o755)).unwrap();
    let exec: Box<ExecFn> = Box::new(|_, _, _| unreachable!());
    assert!(Broker::start(&other, 4242, exec).is_err());
}
