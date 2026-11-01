//! Единая точка привилегированной операции (пункт Б8 плана, см. [`super`]).
//!
//! [`run`] — то, что зовут исполнители окна вместо `pkexec`: операция
//! ([`PrivOp`]) уходит помощнику сеанса прав, сеанс поднимается при первом
//! запросе и дальше живёт один на процесс окна (один пароль). Клиент —
//! процессный синглтон за мьютексом: поток, который ждёт итога, мьютекс не
//! держит, поэтому «Запретить сейчас» ([`shutdown`]) из другого потока не
//! ждёт конца операции.
//!
//! # Когда прежний путь (правило З5)
//!
//! | Итог клиента | [`RunOutcome`] | Прежний путь |
//! |---|---|---|
//! | исполнено (`Done`, успех или нет) | [`RunOutcome::Done`] | нет |
//! | помощника нет / недопустим владелец | [`RunOutcome::Legacy`] ([`LegacyReason::HelperUnavailable`]) | **да** |
//! | помощник эту операцию не исполняет (`not_supported_yet`, `not_for_helper`) | [`RunOutcome::Legacy`] ([`LegacyReason::NotSupported`]) | **да** |
//! | отказ проверки (`Refused` прочими ключами) | [`RunOutcome::Failed`] | нет |
//! | пароль не подтверждён (126/127) | [`RunOutcome::Failed`] | нет: второе окно пароля на отказ человека |
//! | `BuildMismatch` | [`RunOutcome::Failed`] | нет: «Rexon обновлён — перезапустите» |
//! | `Revoked`, `SessionLost`, `Closed`, `Busy`, `Spawn`, `HelperFailed` | [`RunOutcome::Failed`] | нет |
//! | `Cancelled`, `InvalidRequest` | [`RunOutcome::Failed`] | нет |
//!
//! Прежний путь [`run`] сам НЕ исполняет: он возвращает [`RunOutcome::Legacy`],
//! и вызывающий идёт своим прежним исполнителем (`io.run`, `run_logged`,
//! скрипт через `pkexec bash`) — тем же argv, что и до «Б». Для мест без
//! своего argv есть [`legacy_command`] (`PrivOp::pkexec_argv` / скрипт).
//! Своего запуска `pkexec` здесь нет.
//!
//! Операцию, которую помощник заведомо не исполняет
//! ([`super::helper::executes`]), [`run`] отдаёт прежнему пути, не поднимая
//! сеанс: иначе человек ввёл бы пароль ради отказа и потом ещё раз.

use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};

use super::client::{Client, ClientError, Outcome, Pending};
use super::helper;
use super::op::{Helpers, PrivOp};
use super::revoke::Reason;

// ============================================================================
// Шов к клиенту
// ============================================================================

/// Ожидание итога поставленной операции.
pub trait Waiter: Send {
    fn wait(self: Box<Self>, on_progress: &mut dyn FnMut(&str)) -> Result<Outcome, ClientError>;
}

/// Клиент сеанса прав глазами [`run`]. Живой — [`Client`]; в тестах —
/// подделка.
pub trait Backend: Send {
    /// Поставить операцию (поднимает сеанс, если его нет).
    fn submit(&mut self, op: &PrivOp, fds: Vec<OwnedFd>) -> Result<Box<dyn Waiter>, ClientError>;
    /// Есть живой сеанс.
    fn has_session(&self) -> bool;
    /// Закрыть сеанс (окно закрыто, «Запретить сейчас»).
    fn shutdown(&mut self);
}

impl Waiter for Pending {
    fn wait(self: Box<Self>, on_progress: &mut dyn FnMut(&str)) -> Result<Outcome, ClientError> {
        Pending::wait(*self, on_progress)
    }
}

impl Backend for Client {
    fn submit(&mut self, op: &PrivOp, fds: Vec<OwnedFd>) -> Result<Box<dyn Waiter>, ClientError> {
        Client::submit(self, op, fds).map(|p| Box::new(p) as Box<dyn Waiter>)
    }

    fn has_session(&self) -> bool {
        Client::has_session(self)
    }

    fn shutdown(&mut self) {
        Client::shutdown(self)
    }
}

// ============================================================================
// Итог
// ============================================================================

/// Почему операция идёт прежним путём.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyReason {
    /// Помощника сеанса нет или он недопустим (владелец, права):
    /// [`ClientError::raw_fallback_allowed`].
    HelperUnavailable(ClientError),
    /// Помощник такую операцию не исполняет: ключ
    /// [`helper::NOT_SUPPORTED_YET`] или [`helper::NOT_FOR_HELPER`].
    NotSupported(String),
}

/// Итог [`run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    /// Помощник исполнил операцию (успех — `ok`).
    Done(Outcome),
    /// Идти прежним путём — исполнителем вызывающего, как до «Б».
    Legacy(LegacyReason),
    /// Не исполнено (или исход неизвестен); прежнего пути НЕТ (З5).
    Failed(ClientError),
}

impl RunOutcome {
    /// Разрешён прежний путь.
    pub fn is_legacy(&self) -> bool {
        matches!(self, RunOutcome::Legacy(_))
    }

    /// Итог в виде ответа исполнителя команды — для мест, которые дальше
    /// разбирают код и вывод `pkexec` ([`crate::system_exec::RunOut`]).
    /// `None` — [`RunOutcome::Legacy`]: пусть исполнитель сам запустит
    /// прежнюю команду. Строки хода помощника — `stdout`.
    ///
    /// Исполнено — код программы, `stderr` пуст: пояснение помощник уже
    /// прислал строкой хода на языке окна, и разбор «последняя строка —
    /// причина» берёт её из `stdout`. Отказ пароля отдаётся кодом `pkexec`
    /// (126/127): у вызывающих уже есть понятное человеку объяснение на
    /// него. Прочие отказы — код 1 и текст ошибки в `stderr`.
    pub fn to_run_out(&self, stdout: String) -> Option<crate::system_exec::RunOut> {
        use crate::utils::RunOutcome as Rc;
        let (outcome, returncode, stderr) = match self {
            RunOutcome::Legacy(_) => return None,
            RunOutcome::Done(o) => (if o.ok { Rc::Ok } else { Rc::Failed }, o.code, String::new()),
            RunOutcome::Failed(ClientError::NotAuthorized { code }) => (Rc::Failed, Some(*code), self.failure_text()),
            RunOutcome::Failed(ClientError::Spawn(_)) => (Rc::Error, None, self.failure_text()),
            RunOutcome::Failed(_) => (Rc::Failed, Some(1), self.failure_text()),
        };
        Some(crate::system_exec::RunOut { outcome, returncode, stdout, stderr })
    }

    /// Текст отказа для человека на языке окна
    /// ([`ClientError::human_text`], пункт Б13). Техническая строка
    /// (`Display`) уходит в журнал: человеку она ничего не скажет.
    fn failure_text(&self) -> String {
        match self {
            RunOutcome::Failed(e) => {
                crate::log_warn!("privilege", "операция не исполнена: {e}");
                e.human_text()
            }
            _ => String::new(),
        }
    }
}

/// Команда прежнего пути, если у вызывающего нет своей.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyCommand {
    /// argv (`pkexec <прежний помощник> …` или сырой `pkexec …`).
    Argv(Vec<String>),
    /// `pkexec bash <файл>`: префикс временного файла и текст скрипта.
    Script { prefix: &'static str, text: String },
}

/// Прежний путь операции: [`PrivOp::pkexec_argv`], иначе
/// [`PrivOp::fallback_script`]. `None` — прежнего пути нет (З5:
/// `switch-dm`, системные темы без помощника) или он не одна команда.
pub fn legacy_command(op: &PrivOp, helpers: &Helpers) -> Option<LegacyCommand> {
    if let Some(argv) = op.pkexec_argv(helpers) {
        return Some(LegacyCommand::Argv(argv));
    }
    op.fallback_script().map(|(prefix, text)| LegacyCommand::Script { prefix, text })
}

/// Разбор ошибки клиента по правилу З5.
fn classify(err: ClientError) -> RunOutcome {
    if err.raw_fallback_allowed() {
        return RunOutcome::Legacy(LegacyReason::HelperUnavailable(err));
    }
    match err {
        ClientError::Refused { message_key }
            if message_key == helper::NOT_SUPPORTED_YET || message_key == helper::NOT_FOR_HELPER =>
        {
            RunOutcome::Legacy(LegacyReason::NotSupported(message_key))
        }
        other => RunOutcome::Failed(other),
    }
}

// ============================================================================
// Исполнитель
// ============================================================================

/// Клиент сеанса прав за мьютексом. Процессу нужен один — [`run`]; свой
/// экземпляр — для тестов.
///
/// ⚠️ Постановка держит мьютекс и через окно пароля (подъём сеанса —
/// `pkexec` и рукопожатие), то есть сколько угодно долго. Поэтому
/// [`Runner::has_session`] и [`Runner::shutdown`], которые зовёт поток окна
/// (опрос «Настроек», меню значка, крестик), мьютекса не ждут: занят — ответ
/// по последнему известному состоянию, а «закрыть» откладывается до момента,
/// когда постановка его отпустит (Б13).
pub struct Runner {
    backend: Mutex<Box<dyn Backend>>,
    /// Последний ответ `has_session` клиента.
    session_seen: AtomicBool,
    /// Закрыть сеанс, как только мьютекс освободится.
    shutdown_pending: AtomicBool,
}

impl Runner {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self { backend: Mutex::new(backend), session_seen: AtomicBool::new(false), shutdown_pending: AtomicBool::new(false) }
    }

    fn backend(&self) -> MutexGuard<'_, Box<dyn Backend>> {
        // Паника в чужом потоке не должна запирать права навсегда: клиент
        // сам следит за живостью сеанса.
        let mut guard = self.backend.lock().unwrap_or_else(|e| e.into_inner());
        self.apply_pending(&mut guard);
        guard
    }

    /// Отложенное «закрыть» — под взятым мьютексом.
    fn apply_pending(&self, backend: &mut Box<dyn Backend>) {
        if self.shutdown_pending.swap(false, Ordering::SeqCst) {
            backend.shutdown();
        }
        self.session_seen.store(backend.has_session(), Ordering::SeqCst);
    }

    /// Исполнить операцию (см. [`run`]).
    pub fn run(&self, op: &PrivOp, fds: Vec<OwnedFd>, progress: &mut dyn FnMut(&str)) -> RunOutcome {
        if let Err(key) = helper::executes(op) {
            return RunOutcome::Legacy(LegacyReason::NotSupported(key.to_string()));
        }
        // Мьютекс — только на постановку: ожидание идёт без него. «Закрыть»,
        // пришедшее за время окна пароля, исполняется сразу после неё.
        let pending = {
            let mut backend = self.backend();
            let pending = backend.submit(op, fds);
            self.apply_pending(&mut backend);
            pending
        };
        match pending.and_then(|p| p.wait(progress)) {
            Ok(outcome) => RunOutcome::Done(outcome),
            Err(e) => {
                // Помощник другой сборки — пакет обновили при открытом окне:
                // то же уведомление «Rexon обновлён», что у сторожа бинаря,
                // сразу, а не к его опросу (Б13).
                if matches!(e, ClientError::BuildMismatch { .. } | ClientError::HelperOtherBuild { .. }) {
                    crate::single_instance::nudge_self_update_watch();
                }
                classify(e)
            }
        }
    }

    /// Есть живой сеанс. Не ждёт: пока идёт постановка (окно пароля) —
    /// последнее известное состояние.
    pub fn has_session(&self) -> bool {
        match self.backend.try_lock() {
            Ok(mut guard) => {
                self.apply_pending(&mut guard);
                self.session_seen.load(Ordering::SeqCst)
            }
            Err(TryLockError::Poisoned(e)) => {
                let mut guard = e.into_inner();
                self.apply_pending(&mut guard);
                self.session_seen.load(Ordering::SeqCst)
            }
            Err(TryLockError::WouldBlock) => {
                !self.shutdown_pending.load(Ordering::SeqCst) && self.session_seen.load(Ordering::SeqCst)
            }
        }
    }

    /// Закрыть сеанс. Не ждёт: пока идёт постановка — закроет сразу после
    /// неё (поставленная операция тогда получит отказ, а не исполнится).
    pub fn shutdown(&self) {
        self.shutdown_pending.store(true, Ordering::SeqCst);
        self.session_seen.store(false, Ordering::SeqCst);
        match self.backend.try_lock() {
            Ok(mut guard) => self.apply_pending(&mut guard),
            Err(TryLockError::Poisoned(e)) => self.apply_pending(&mut e.into_inner()),
            Err(TryLockError::WouldBlock) => {}
        }
    }

    /// Закрыть сеанс по причине `reason` (окно ушло в лоток, «Запретить
    /// сейчас» в меню значка). `Some(reason)` — живое право было и погасло,
    /// человеку надо сказать, что пароль спросят снова; `None` — гасить было
    /// нечего. Не ждёт, как и [`Runner::shutdown`].
    ///
    /// Причину знает только окно: помощник видит лишь конец связи и
    /// понимает его как закрытие окна, о котором сообщать некому. Поэтому
    /// текст причины показывает окно, а не ответ помощника.
    pub fn end_session(&self, reason: Reason) -> Option<Reason> {
        let had = self.has_session();
        self.shutdown();
        had.then_some(reason)
    }
}

static LIVE: OnceLock<Runner> = OnceLock::new();

/// Процессный исполнитель на живом клиенте ([`Client::live`]). Сам клиент
/// ничего не запускает, пока нет запроса.
pub fn live() -> &'static Runner {
    LIVE.get_or_init(|| Runner::new(Box::new(Client::live())))
}

/// Исполнить привилегированную операцию через сеанс прав окна.
///
/// `fds` — открытые файлы операции в порядке [`PrivOp::local_files`];
/// `progress` — строки хода. Итог — [`RunOutcome`]; при
/// [`RunOutcome::Legacy`] вызывающий исполняет прежнюю команду сам.
pub fn run(op: &PrivOp, fds: Vec<OwnedFd>, progress: &mut dyn FnMut(&str)) -> RunOutcome {
    live().run(op, fds, progress)
}

/// Открыть файл операции стороной окна: обычный файл — как есть, каталог
/// (тема, раскладка экрана входа) — несжатым ustar в памяти
/// ([`super::unpack::pack_tree_memfd`]): каталог по дескриптору помощник не
/// принимает (З2).
pub fn open_local(path: &str) -> std::io::Result<OwnedFd> {
    let p = std::path::Path::new(path);
    if std::fs::metadata(p)?.is_dir() {
        super::unpack::pack_tree_memfd(p)
    } else {
        std::fs::File::open(p).map(OwnedFd::from)
    }
}

/// [`run`] для живых исполнителей: `files` — пути файлов операции в
/// порядке [`PrivOp::local_files`] (открываются здесь — [`open_local`] — и
/// уходят дескрипторами), строки хода дописываются в журнал `log_path` (если
/// есть), отдаются в `line` и собираются в `stdout` ответа. `None` — прежний
/// путь ([`RunOutcome::Legacy`]): исполнитель запускает свою прежнюю команду.
///
/// Файлы открываются только после того, как ясно, что помощник операцию
/// исполняет ([`helper::executes`]): каталог не упаковывается ради прежнего
/// пути.
pub fn run_live(
    op: &PrivOp,
    files: &[String],
    log_path: Option<&str>,
    line: &mut dyn FnMut(&str),
) -> Option<crate::system_exec::RunOut> {
    use std::io::Write;
    if helper::executes(op).is_err() {
        return None;
    }
    let mut fds = Vec::with_capacity(files.len());
    for f in files {
        match open_local(f) {
            Ok(h) => fds.push(h),
            // Помощника сеанса нет — решает прежний путь, как до «Б» (каталог
            // со ссылкой прежний помощник принимал).
            Err(_) if !session_possible() => return None,
            // Файла нет (или каталог не упаковать) — помощнику сеанса нечего
            // передать, а прежнего пути при нём нет (З5).
            Err(_) => return Some(crate::system_exec::RunOut::missing()),
        }
    }
    let mut log = log_path.and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok());
    let mut stdout = String::new();
    let outcome = run(op, fds, &mut |l: &str| {
        if let Some(f) = log.as_mut() {
            let _ = writeln!(f, "{l}");
        }
        stdout.push_str(l);
        stdout.push('\n');
        line(l);
    });
    outcome.to_run_out(stdout)
}

/// Сеанс прав есть или его можно поднять: помощник найден и прошёл
/// проверку владения ([`super::client::locate_helper`]).
fn session_possible() -> bool {
    use super::client::{helper_candidates, locate_helper, Located, RealFs};
    has_session() || matches!(locate_helper(&RealFs, &helper_candidates()), Located::Ready(_))
}

/// Есть живой сеанс прав (следующая операция пароля не спросит).
pub fn has_session() -> bool {
    LIVE.get().is_some_and(Runner::has_session)
}

/// Закрыть сеанс прав, если он есть: окно закрыто или свёрнуто в лоток,
/// «Запретить сейчас» (Б13).
pub fn shutdown() {
    if let Some(r) = LIVE.get() {
        r.shutdown();
    }
}

/// Закрыть сеанс прав по причине `reason` ([`Runner::end_session`]).
/// `Some` — право было и погасло.
pub fn end_session(reason: Reason) -> Option<Reason> {
    LIVE.get().and_then(|r| r.end_session(reason))
}

/// Что сказать человеку, когда крестик спрятал окно в лоток:
/// `(заголовок, текст)` уведомления или `None` — молчать.
///
/// `first_time` — подсказку «Rexon остался в лотке» ещё не показывали (она
/// одна на установку: каждый раз — навязчиво). `ended` — итог
/// [`end_session`]. О погасшем праве говорим КАЖДЫЙ раз, а не однажды:
/// иначе человек, увидевший подсказку вчера, не узнает, почему сегодня
/// снова спрашивают пароль (отметка «подсказку видели» глушила и это —
/// текст причины `window-to-tray` не показывался никогда).
pub fn tray_close_notice(first_time: bool, ended: Option<Reason>) -> Option<(String, String)> {
    let (title, hint) = crate::tray::hidden_notice();
    let body = match (first_time, ended) {
        (false, None) => return None,
        (true, None) => hint,
        (false, Some(r)) => r.message(),
        (true, Some(r)) => format!("{hint}\n{}", r.message()),
    };
    Some((title, body))
}

/// Уведомление «право погасло» по причине `reason`, когда окна может не
/// быть на экране (пункт «Запретить сейчас» в меню значка).
pub fn permission_ended_notice(reason: Reason) -> (String, String) {
    (crate::tray::APP_ID.to_string(), reason.message())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privilege::client::{FileKind, FileMeta, HelperChild, HelperFs, Spawner};
    use crate::privilege::op::{LocalFile, PkgSource};
    use crate::privilege::proto::{self, ClientMsg, ErrorCode, Hello, HelperMsg};
    use crate::system::PkgFamily;
    use crate::system_plan as plan;
    use std::io;
    use std::os::fd::AsFd;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread;
    use std::time::Duration;

    const HELPER: &str = "/usr/lib/rexon-priv-helper";
    const BUILD: &str = "0.5.0+run-test";
    /// Предел ожидания любого итога в тестах: зависание — ошибка теста.
    const LIMIT: Duration = Duration::from_secs(20);

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn install_op() -> PrivOp {
        PrivOp::PkgInstall { family: PkgFamily::Arch, packages: names(&["sddm"]), source: PkgSource::DisplayManager }
    }

    /// ФС: каталоги и (если `with_helper`) помощник — root 0755; иначе
    /// помощник принадлежит пользователю (`bad_owner`) или его нет.
    struct Fs {
        helper: Option<u32>,
    }

    impl HelperFs for Fs {
        fn lstat(&self, path: &Path) -> io::Result<FileMeta> {
            let dirs = ["/", "/usr", "/usr/lib", "/usr/local", "/usr/local/lib", "/var", "/var/lib"];
            if dirs.iter().any(|d| Path::new(d) == path) {
                return Ok(FileMeta { kind: FileKind::Dir, uid: 0, mode: 0o755 });
            }
            match (self.helper, path == Path::new(HELPER)) {
                (Some(uid), true) => Ok(FileMeta { kind: FileKind::Regular, uid, mode: 0o755 }),
                _ => Err(io::Error::from(io::ErrorKind::NotFound)),
            }
        }
        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            Ok(path.to_path_buf())
        }
    }

    /// Что поддельный помощник отвечает на запрос.
    #[derive(Clone)]
    enum Reply {
        Done,
        Refuse(&'static str),
    }

    type Script = dyn Fn(OwnedFd) -> i32 + Send + Sync;

    struct CountingSpawner {
        spawns: Arc<AtomicUsize>,
        script: Arc<Script>,
    }

    struct Child(Option<thread::JoinHandle<i32>>);

    impl HelperChild for Child {
        fn wait(&mut self) -> io::Result<Option<i32>> {
            Ok(self.0.take().map(|h| h.join().unwrap_or(-1)))
        }
    }

    impl Spawner for CountingSpawner {
        fn spawn(&mut self, _argv: &[String], helper_end: OwnedFd) -> io::Result<Box<dyn HelperChild>> {
            self.spawns.fetch_add(1, Ordering::SeqCst);
            let script = Arc::clone(&self.script);
            Ok(Box::new(Child(Some(thread::spawn(move || script(helper_end))))))
        }
    }

    /// Помощник, отвечающий на рукопожатие и на каждый запрос `reply`.
    fn helper_script(build: &'static str, reply: Reply, seen: Arc<AtomicUsize>) -> Arc<Script> {
        Arc::new(move |sock: OwnedFd| {
            let Ok(Some(ClientMsg::Hello(Hello { .. }))) = proto::recv_msg::<ClientMsg<PrivOp>>(sock.as_fd()) else {
                return 1;
            };
            proto::send_msg(sock.as_fd(), &HelperMsg::Welcome { build_id: build.into() }).unwrap();
            loop {
                let Ok(Some((frame, _fds))) = proto::recv_frame_with_fds(sock.as_fd()) else {
                    return 0;
                };
                let Ok(ClientMsg::Request { id, .. }) = proto::decode::<ClientMsg<PrivOp>>(&frame) else {
                    return 1;
                };
                seen.fetch_add(1, Ordering::SeqCst);
                let msg = match &reply {
                    Reply::Done => {
                        proto::send_msg(sock.as_fd(), &HelperMsg::Progress { id, line: format!("ход {id}") }).unwrap();
                        HelperMsg::Done { id, ok: true, code: Some(0), message_key: None }
                    }
                    Reply::Refuse(key) => {
                        HelperMsg::Error { id: Some(id), code: ErrorCode::Refused, message_key: key.to_string() }
                    }
                };
                proto::send_msg(sock.as_fd(), &msg).unwrap();
            }
        })
    }

    /// Исполнитель на настоящем [`Client`] с подделанными ФС и запуском.
    fn runner(fs: Fs, script: Arc<Script>) -> (Arc<Runner>, Arc<AtomicUsize>) {
        let spawns = Arc::new(AtomicUsize::new(0));
        let spawner = CountingSpawner { spawns: Arc::clone(&spawns), script };
        let client = Client::new(Box::new(spawner), Box::new(fs), BUILD.into(), Some("2".into()))
            .with_candidates(vec![PathBuf::from(HELPER)]);
        (Arc::new(Runner::new(Box::new(client))), spawns)
    }

    /// `run` в отдельном потоке с пределом времени.
    fn run_limited(r: &Arc<Runner>, op: PrivOp) -> (RunOutcome, Vec<String>) {
        let (tx, rx) = mpsc::channel();
        let r = Arc::clone(r);
        thread::spawn(move || {
            let mut lines = Vec::new();
            let out = r.run(&op, Vec::new(), &mut |l: &str| lines.push(l.to_string()));
            let _ = tx.send((out, lines));
        });
        rx.recv_timeout(LIMIT).expect("run завис")
    }

    #[test]
    fn many_operations_one_spawner() {
        let seen = Arc::new(AtomicUsize::new(0));
        let (r, spawns) = runner(Fs { helper: Some(0) }, helper_script(BUILD, Reply::Done, Arc::clone(&seen)));
        for i in 0..4 {
            let (out, lines) = run_limited(&r, install_op());
            assert!(matches!(&out, RunOutcome::Done(o) if o.ok), "{i}: {out:?}");
            assert_eq!(lines, [format!("ход {}", i + 1)]);
        }
        assert_eq!(spawns.load(Ordering::SeqCst), 1, "один pkexec на все операции");
        assert_eq!(seen.load(Ordering::SeqCst), 4);
        assert!(r.has_session());
        r.shutdown();
        assert!(!r.has_session());
    }

    /// Deepin на Treeland целиком: сборочные пересборки, своя сборка
    /// deepin-session и набор стола — один подъём помощника (один пароль).
    /// SteamOS: поставить и снять образ стола — тоже одним.
    #[test]
    fn deepin_rebuild_and_set_and_sysext_one_spawner() {
        use crate::privilege::op::SysextOp;
        let seen = Arc::new(AtomicUsize::new(0));
        let (r, spawns) = runner(Fs { helper: Some(0) }, helper_script(BUILD, Reply::Done, Arc::clone(&seen)));
        let built = PrivOp::InstallBuilt {
            package: LocalFile { slot: 0, label: "/home/u/.cache/rexon/rebuild/deepin-session-treeland-1-1-x86_64.pkg.tar.zst".into() },
            build_only: names(&["cmake"]),
            replace: false,
        };
        let set = PrivOp::DeInstallSet {
            family: PkgFamily::Arch,
            de_id: Some("deepin".into()),
            packages: names(&["treeland", "ddm", "deepin-anything-dkms"]),
            deps_only: names(&["deepin-anything-dkms"]),
            replace: vec![],
            update_first: true,
        };
        let ops = [
            PrivOp::BuildDeps { names: names(&["cmake"]) },
            built,
            set,
            PrivOp::Sysext { op: SysextOp::Install, de_id: "xfce".into() },
            PrivOp::Sysext { op: SysextOp::Remove, de_id: "xfce".into() },
        ];
        for op in ops {
            let (tx, rx) = mpsc::channel();
            let r2 = Arc::clone(&r);
            let label = format!("{op:?}");
            thread::spawn(move || {
                let fds = op
                    .local_files()
                    .iter()
                    .map(|_| OwnedFd::from(std::fs::File::open("/dev/null").unwrap()))
                    .collect();
                let _ = tx.send(r2.run(&op, fds, &mut |_| {}));
            });
            let out = rx.recv_timeout(LIMIT).expect("run завис");
            assert!(matches!(&out, RunOutcome::Done(o) if o.ok), "{label}: {out:?}");
        }
        assert_eq!(spawns.load(Ordering::SeqCst), 1, "один pkexec на всё");
        assert_eq!(seen.load(Ordering::SeqCst), 5);
        r.shutdown();
    }

    #[test]
    fn no_helper_goes_legacy_with_todays_argv() {
        let (r, spawns) = runner(Fs { helper: None }, helper_script(BUILD, Reply::Done, Arc::default()));
        let (out, _) = run_limited(&r, install_op());
        assert_eq!(out, RunOutcome::Legacy(LegacyReason::HelperUnavailable(ClientError::HelperMissing)));
        assert_eq!(out.to_run_out(String::new()), None);
        assert_eq!(spawns.load(Ordering::SeqCst), 0);
        // Прежняя команда — та же, что строит план установки сегодня.
        for pkg in [true, false] {
            let helpers = Helpers { pkg, ..Helpers::default() };
            let today = plan::install_plan(PkgFamily::Arch, &names(&["sddm"]), false, None, pkg).unwrap();
            let want = today
                .steps
                .iter()
                .find_map(|s| match s {
                    plan::PlanStep::Run { id: plan::RunStepId::PkgInstall, cmd, .. } => Some(cmd.argv.clone()),
                    _ => None,
                })
                .unwrap();
            assert_eq!(legacy_command(&install_op(), &helpers), Some(LegacyCommand::Argv(want)));
        }
        // Скрипт там, где одной команды нет.
        assert_eq!(
            legacy_command(&PrivOp::MateGschemaCleanup, &Helpers::default()),
            Some(LegacyCommand::Script { prefix: "rexon_gschema_", text: plan::mate_gschema_cleanup_script() })
        );
        // switch-dm без помощника — прежнего пути нет.
        let switch = PrivOp::SwitchDm { family: PkgFamily::Arch, unit: "sddm".into(), greeter: None };
        assert_eq!(legacy_command(&switch, &Helpers::default()), None);
    }

    #[test]
    fn bad_owner_goes_legacy_without_spawn() {
        let (r, spawns) = runner(Fs { helper: Some(1000) }, helper_script(BUILD, Reply::Done, Arc::default()));
        let (out, _) = run_limited(&r, install_op());
        assert!(
            matches!(out, RunOutcome::Legacy(LegacyReason::HelperUnavailable(ClientError::HelperBadOwner(_)))),
            "{out:?}"
        );
        assert_eq!(spawns.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn refusal_of_check_has_no_raw_path() {
        let (r, _) = runner(Fs { helper: Some(0) }, helper_script(BUILD, Reply::Refuse("refused.not-in-table"), Arc::default()));
        let (out, _) = run_limited(&r, install_op());
        assert_eq!(out, RunOutcome::Failed(ClientError::Refused { message_key: "refused.not-in-table".into() }));
        assert!(!out.is_legacy());
        let ro = out.to_run_out(String::new()).unwrap();
        assert_eq!(ro.returncode, Some(1));
        // Человеку — фраза на языке окна, а не ключ помощника (Б13).
        assert_eq!(ro.stderr, crate::privilege::client::key_text("refused.not-in-table"));
        assert!(!ro.stderr.contains("refused.") && !ro.stderr.contains("rexon-priv-helper"), "{ro:?}");
    }

    #[test]
    fn dismissed_password_and_build_mismatch_have_no_raw_path() {
        // Окно пароля закрыто: pkexec вышел с 126 до рукопожатия.
        let dismissed: Arc<Script> = Arc::new(|sock: OwnedFd| {
            drop(sock);
            126
        });
        let (r, _) = runner(Fs { helper: Some(0) }, dismissed);
        let (out, _) = run_limited(&r, install_op());
        assert_eq!(out, RunOutcome::Failed(ClientError::NotAuthorized { code: 126 }));
        // Код pkexec сохраняется: у исполнителей есть объяснение на него.
        assert_eq!(out.to_run_out(String::new()).unwrap().returncode, Some(126));

        let (r, spawns) = runner(Fs { helper: Some(0) }, helper_script("0.4.0+other", Reply::Done, Arc::default()));
        let (out, _) = run_limited(&r, install_op());
        assert!(matches!(out, RunOutcome::Failed(ClientError::BuildMismatch { .. })), "{out:?}");
        // Повтор не поднимает сеанс снова и сырого пути тоже не даёт.
        let (again, _) = run_limited(&r, install_op());
        assert!(matches!(again, RunOutcome::Failed(ClientError::BuildMismatch { .. })), "{again:?}");
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn not_supported_reply_is_the_only_legacy_after_session() {
        for key in [helper::NOT_SUPPORTED_YET, helper::NOT_FOR_HELPER] {
            let (r, _) = runner(Fs { helper: Some(0) }, helper_script(BUILD, Reply::Refuse(key), Arc::default()));
            let (out, _) = run_limited(&r, install_op());
            assert_eq!(out, RunOutcome::Legacy(LegacyReason::NotSupported(key.into())));
            assert_eq!(out.to_run_out(String::new()), None);
        }
        // Похожий, но другой ключ — отказ.
        assert!(matches!(
            classify(ClientError::Refused { message_key: "not_supported".into() }),
            RunOutcome::Failed(_)
        ));
    }

    #[test]
    fn known_unsupported_ops_skip_the_password() {
        let seen = Arc::new(AtomicUsize::new(0));
        let (r, spawns) = runner(Fs { helper: Some(0) }, helper_script(BUILD, Reply::Done, Arc::clone(&seen)));
        let aur = PrivOp::AurInstall { helper: "paru".into(), packages: names(&["x"]), replace_conflicts: false };
        let (out, _) = run_limited(&r, aur);
        assert_eq!(out, RunOutcome::Legacy(LegacyReason::NotSupported(helper::NOT_FOR_HELPER.into())));
        let (out, _) = run_limited(&r, PrivOp::MateGschemaCleanup);
        assert_eq!(out, RunOutcome::Legacy(LegacyReason::NotSupported(helper::NOT_SUPPORTED_YET.into())));
        assert_eq!(spawns.load(Ordering::SeqCst), 0, "ради заведомого отказа пароль не спрашивается");
        assert_eq!(seen.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn every_client_error_classified_by_z5() {
        let legacy = [
            ClientError::HelperMissing,
            ClientError::HelperOtherBuild { helper_build: "0.4.9+old".into() },
        ];
        for e in legacy {
            assert!(classify(e).is_legacy());
        }
        let failed = [
            ClientError::NotAuthorized { code: 127 },
            ClientError::BuildMismatch { helper_build: None },
            ClientError::Refused { message_key: "refused.vital".into() },
            ClientError::HelperFailed { message_key: "x".into() },
            ClientError::Cancelled,
            ClientError::Revoked { message_key: "idle".into() },
            ClientError::SessionLost { outcome_unknown: true, detail: "x".into() },
            ClientError::Closed,
            ClientError::Busy,
            ClientError::Spawn("x".into()),
            ClientError::InvalidRequest("x".into()),
        ];
        for e in failed {
            let out = classify(e.clone());
            assert_eq!(out, RunOutcome::Failed(e));
            assert!(out.to_run_out(String::new()).is_some());
        }
    }

    // ---------------------------------------------------------------- подмена трейтом

    /// Подделка клиента: помнит операции, отвечает заданным.
    struct FakeBackend {
        calls: Arc<Mutex<Vec<(PrivOp, usize)>>>,
        answer: Result<Outcome, ClientError>,
    }

    struct Ready(Result<Outcome, ClientError>);

    impl Waiter for Ready {
        fn wait(self: Box<Self>, on_progress: &mut dyn FnMut(&str)) -> Result<Outcome, ClientError> {
            on_progress("строка");
            self.0
        }
    }

    impl Backend for FakeBackend {
        fn submit(&mut self, op: &PrivOp, fds: Vec<OwnedFd>) -> Result<Box<dyn Waiter>, ClientError> {
            self.calls.lock().unwrap().push((op.clone(), fds.len()));
            Ok(Box::new(Ready(self.answer.clone())))
        }
        fn has_session(&self) -> bool {
            true
        }
        fn shutdown(&mut self) {}
    }

    #[test]
    fn fake_backend_gets_op_files_and_progress() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ok = Outcome { ok: false, code: Some(3), message_key: Some("pkg_failed".into()) };
        let r = Runner::new(Box::new(FakeBackend { calls: Arc::clone(&calls), answer: Ok(ok.clone()) }));
        let file = std::fs::File::open("/dev/null").unwrap();
        let op = PrivOp::InstallBuilt {
            package: LocalFile { slot: 0, label: "/home/u/.cache/rexon/rebuild/x.pkg.tar.zst".into() },
            build_only: vec![],
            replace: false,
        };
        let mut lines = Vec::new();
        let out = r.run(&op, vec![OwnedFd::from(file)], &mut |l: &str| lines.push(l.to_string()));
        assert_eq!(out, RunOutcome::Done(ok));
        assert_eq!(lines, ["строка"]);
        assert_eq!(calls.lock().unwrap().as_slice(), [(op, 1)]);
        // Неуспех программы — это исполнено, а не повод для прежнего пути.
        let ro = out.to_run_out("строка\n".into()).unwrap();
        assert_eq!((ro.returncode, ro.stdout.as_str()), (Some(3), "строка\n"));
        assert_eq!(ro.outcome, crate::utils::RunOutcome::Failed);
    }

    /// Клиент, у которого постановка висит, пока не отпустят (окно пароля).
    struct StuckBackend {
        release: Arc<Mutex<mpsc::Receiver<()>>>,
        /// Весть «постановка началась» (мьютекс уже взят и `apply_pending`
        /// перед ней отработал).
        entered: mpsc::Sender<()>,
        alive: Arc<std::sync::atomic::AtomicBool>,
        shutdowns: Arc<AtomicUsize>,
    }

    impl Backend for StuckBackend {
        fn submit(&mut self, _op: &PrivOp, _fds: Vec<OwnedFd>) -> Result<Box<dyn Waiter>, ClientError> {
            let _ = self.entered.send(());
            let _ = self.release.lock().unwrap().recv_timeout(LIMIT);
            self.alive.store(true, Ordering::SeqCst);
            Err(ClientError::Closed)
        }
        fn has_session(&self) -> bool {
            self.alive.load(Ordering::SeqCst)
        }
        fn shutdown(&mut self) {
            self.alive.store(false, Ordering::SeqCst);
            self.shutdowns.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Б13: пока висит окно пароля, поток окна спрашивает «есть ли сеанс» и
    /// жмёт «Запретить сейчас» / прячет окно в лоток — и не встаёт. Закрытие
    /// исполняется сразу, как постановка отпустит клиента.
    #[test]
    fn window_thread_never_waits_for_password() {
        let (tx, rx) = mpsc::channel();
        let (entered_tx, entered_rx) = mpsc::channel();
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let r = Arc::new(Runner::new(Box::new(StuckBackend {
            release: Arc::new(Mutex::new(rx)),
            entered: entered_tx,
            alive: Arc::clone(&alive),
            shutdowns: Arc::clone(&shutdowns),
        })));
        let worker = {
            let r = Arc::clone(&r);
            thread::spawn(move || r.run(&install_op(), vec![], &mut |_| {}))
        };
        // Дождаться, пока исполнитель войдёт в постановку. Ждать одного
        // «мьютекс занят» мало: занят он и в `apply_pending` до неё, и
        // «закрыть» тогда исполнялось раньше постановки — тест падал
        // наперегонки.
        entered_rx.recv_timeout(LIMIT).expect("постановка не началась");
        assert!(r.backend.try_lock().is_err());
        let started = std::time::Instant::now();
        assert!(!r.has_session());
        r.shutdown();
        assert!(!r.has_session());
        assert!(started.elapsed() < Duration::from_secs(2), "поток окна ждал окна пароля");
        assert_eq!(shutdowns.load(Ordering::SeqCst), 0, "пока постановка идёт, клиента не трогаем");
        tx.send(()).unwrap();
        let out = worker.join().unwrap();
        assert_eq!(out, RunOutcome::Failed(ClientError::Closed));
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1, "отложенное «закрыть» исполнено");
        assert!(!alive.load(Ordering::SeqCst));
        assert!(!r.has_session());
    }

    /// Крестик прячет окно в лоток: живое право гаснет, и окно знает
    /// причину, чтобы сказать её человеку; второй раз гасить нечего.
    #[test]
    fn end_session_reports_reason_only_when_permission_was_alive() {
        let (_tx, rx) = mpsc::channel::<()>();
        let (entered, _) = mpsc::channel();
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let r = Runner::new(Box::new(StuckBackend {
            release: Arc::new(Mutex::new(rx)),
            entered,
            alive: Arc::clone(&alive),
            shutdowns: Arc::clone(&shutdowns),
        }));
        assert!(r.has_session());
        assert_eq!(r.end_session(Reason::WindowToTray), Some(Reason::WindowToTray));
        assert!(!r.has_session());
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
        assert_eq!(r.end_session(Reason::WindowToTray), None, "права уже нет — молчим");
    }

    /// Текст причины «свёрнут в лоток» доходит до человека каждый раз, когда
    /// право погасло, а подсказка «остался в лотке» — только впервые.
    #[test]
    fn tray_close_notice_tells_ended_permission_every_time() {
        let (title, hint) = crate::tray::hidden_notice();
        let why = Reason::WindowToTray.message();
        assert_eq!(tray_close_notice(false, None), None);
        assert_eq!(tray_close_notice(true, None), Some((title.clone(), hint.clone())));
        assert_eq!(tray_close_notice(false, Some(Reason::WindowToTray)), Some((title.clone(), why.clone())));
        let (t, body) = tray_close_notice(true, Some(Reason::WindowToTray)).unwrap();
        assert_eq!(t, title);
        assert!(body.starts_with(&hint) && body.ends_with(&why), "{body}");
        assert_eq!(
            permission_ended_notice(Reason::RevokedByUser),
            ("Rexon".to_string(), Reason::RevokedByUser.message())
        );
    }

    #[test]
    fn module_shutdown_without_live_client_is_noop() {
        // Живой клиент не создаётся ради вопроса «есть ли сеанс».
        if LIVE.get().is_none() {
            assert!(!has_session());
            shutdown();
            assert!(LIVE.get().is_none());
        }
    }
}
