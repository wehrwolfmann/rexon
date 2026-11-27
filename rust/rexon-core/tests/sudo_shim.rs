//! The `rexon-sudo-shim` shim (item B10): the real shim binary, a fake AUR
//! builder (a bash script instead of paru — real paru/yay/makepkg are not run
//! here) and the window — the [`Broker`] of this test process. The privilege
//! session is a [`Runner`] with a fake client: it counts session raises
//! (password windows) and records what would go to the helper. No root, no pkexec.

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

/// What went to the «helper» and how many times the session was raised.
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
            // Session raise = one password window.
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

/// Stand: an «XDG_RUNTIME_DIR» directory, the window broker, a fake builder.
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
    // Fake paru: takes `--sudo`/`--sudoflags` from its command line and calls
    // «sudo» with every line of the command file — like paru/yay.
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

    /// Run the fake builder with tag `reg` and commands `cmds`; the result is the
    /// «sudo» exit codes per line and the shim's stderr.
    fn build(&self, reg: &Registration, cmds: &[String]) -> (Vec<i32>, String) {
        let out = self.root.join("out");
        let err = self.root.join("err");
        let list = self.root.join("cmds");
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&err);
        std::fs::write(&list, cmds.join("\n") + "\n").unwrap();
        let mut cmd = Command::new(self.root.join("paru"));
        cmd.args(["-S", "--needed", "--noconfirm", "--sudo", SHIM, "--sudoflags", &reg.flag, "t"])
            .env("XDG_RUNTIME_DIR", &self.root)
            .env("FAKE_CMDS", &list)
            .env("FAKE_OUT", &out)
            .env("FAKE_ERR", &err);
        let mut child = spawn_retry_busy(&mut cmd);
        wait_child(&mut child);
        let codes = std::fs::read_to_string(&out)
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().parse().unwrap())
            .collect();
        (codes, std::fs::read_to_string(&err).unwrap_or_default())
    }

    /// The shim directly from the test process (not from the builder).
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

/// Run a freshly written stub script. While a parallel test in the same
/// process forks, the child briefly keeps a write descriptor of our file open,
/// and exec answers ETXTBSY («Text file busy») — a kernel race, not a shim
/// defect. A short retry removes it.
fn spawn_retry_busy(cmd: &mut Command) -> std::process::Child {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match cmd.spawn() {
            Ok(child) => return child,
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("не запустился {:?}: {e}", cmd.get_program()),
        }
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
    // Like paru: dependencies from the repo, the built package, install reasons,
    // removing build deps. And like yay: flags in a different order, `--config`.
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

    // The next build with a live session — no password.
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
            format!("pacman -U --noconfirm -- {evil}"),           // foreign name
            format!("pacman -U --ask=4 -- {t}"),                  // replacement without consent
            format!("pacman -U --overwrite=* -- {t}"),            // flag outside the list
            format!("pacman -U --dbpath /tmp -- {t}"),
            "pacman -S --noconfirm --config /tmp/evil.conf -- cmake".into(),
            "pacman -R --noconfirm -- glibc".into(),              // not installed by the operation
            "pacman -D --asexplicit -- linux".into(),
            "pacman -Syu --".into(),
            "bash -c id".into(),
            "pacman -S cmake".into(),                             // targets without `--`
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

    // A foreign process (the shim's parent is not the builder with this tag)
    // will not bind the operation.
    assert_eq!(st.shim_direct(&[&flag, "pacman", "-U", "--", &t]), 1);
    // Unknown token and a malformed tag.
    let fake = format!("--rexon-op={}.{}", std::process::id(), "0".repeat(32));
    assert_eq!(st.shim_direct(&[&fake, "pacman", "-U", "--", &t]), 1);
    assert_eq!(st.shim_direct(&["pacman", "-U", "--", &t]), 1);
    assert!(st.ops().is_empty());

    // The build went through and exited.
    let (codes, err) = st.build(&reg, &[format!("pacman -U -- {t}")]);
    assert_eq!(codes, vec![0], "{err}");
    assert_eq!(st.ops().len(), 1);

    // After the operation ends: the same tag from a new «builder» and directly — refused.
    let (codes, _) = st.build(&reg, &[format!("pacman -U -- {t}")]);
    assert_eq!(codes, vec![1]);
    assert_eq!(st.shim_direct(&[&flag, "pacman", "-U", "--", &t]), 1);
    assert_eq!(st.ops().len(), 1, "после конца операции ничего не исполнено");
}

#[test]
fn продление_sudo_без_окна() {
    let st = stand("v");
    // yay with `sudoloop` calls `<sudo> -v` without flags — success, nothing executed.
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
    // A directory with wider permissions — the broker does not start.
    let other = st.root.join("wide");
    std::fs::create_dir(&other).unwrap();
    std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o755)).unwrap();
    let exec: Box<ExecFn> = Box::new(|_, _, _| unreachable!());
    assert!(Broker::start(&other, 4242, exec).is_err());
}
