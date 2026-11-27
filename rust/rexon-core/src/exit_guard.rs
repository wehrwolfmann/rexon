//! Draining background tasks on close: a port of `install_exit_guard` and
//! `stop_background_workers` from `rexon/utils.py` (61 lines of decisions).
//!
//! WHY. A customer closes the window without waiting for the theme store to
//! answer or for the scan of installed items. At that moment a background task
//! sits inside a blocking network call, the event loop has already finished,
//! and the process either crashes (in Qt: `QThread: Destroyed while thread is
//! still running` → SIGABRT) or cuts the task off in the middle of writing a
//! file. To the person this looks like "Rexon quit unexpectedly", although
//! they merely closed the window.
//!
//! ONE DOOR (rule #12). The shutdown order — "ask → wait → count the pool" —
//! lives HERE, once per process. Tabs do not need to remember their threads:
//! it is enough that a task is started through [`spawn`] (it will be counted)
//! and, if it has a cancellation, that the tab puts that cancellation into
//! [`register_stopper`]. Precisely because the original had no such door, the
//! fixed wait lived in `closeEvent` of two tabs, Qt never called those
//! methods, and the crash kept coming back.
//!
//! HOW IT DIFFERS FROM THE ORIGINAL (rule #16: we do not preserve pitfalls).
//! Python searched for live threads by walking `gc.get_objects()`: a full walk
//! of the object tree on every call, and blind to threads started by C++
//! (they have no Python wrapper, hence the separate fiddling with
//! `QThreadPool`). Here the live tasks are counted by [`WorkerGuard`] itself —
//! a counter, not a walk — so waiting does not get more expensive with the
//! size of the object tree, and it does not lie.
//!
//! WHAT IS KEPT 1:1: waiting in slices of [`WAIT_SLICE_MS`] rather than one
//! wait for the whole period (exiting must not cost a single extra
//! millisecond if the tasks finished earlier); the budget
//! [`DEFAULT_BUDGET_MS`]; the outcome of waiting for the shared pool MUST make
//! it into the answer (measured 22.08.2026: the pool was waited on for the
//! side effect, but only the thread count was returned, so the drain answered
//! "nobody is left" while background work was running); the "shutting down"
//! flag is raised for the duration of a non-final drain and restored to its
//! previous value afterwards, otherwise it latches forever and the next window
//! in the same process is left without background tasks.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long in total we wait for background tasks. This is a CEILING, not a
/// planned wait: cancellation has already been raised, tasks abort their
/// requests themselves, and normally the drain takes milliseconds.
pub const DEFAULT_BUDGET_MS: u64 = 8000;

/// The wait slice. We wait in slices rather than one sleep for the whole
/// budget so that we leave as soon as the last task has finished.
pub const WAIT_SLICE_MS: u64 = 100;

/// Whether shutdown is in progress. When set, no new background tasks may be started.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// How many background tasks are alive right now (counted by [`WorkerGuard`]).
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// Requests to wrap up: they abort network requests and raise cancellation tokens.
type Stopper = Box<dyn Fn() + Send + Sync + 'static>;

/// Accounting for someone else's workers (Qt's shared `QThreadPool`): receives
/// the remaining budget in milliseconds and answers with the number of tasks
/// NOT waited for.
type Waiter = Box<dyn Fn(u64) -> usize + Send + Sync + 'static>;

static STOPPERS: OnceLock<Mutex<Vec<Stopper>>> = OnceLock::new();
static WAITERS: OnceLock<Mutex<Vec<Waiter>>> = OnceLock::new();

fn stoppers() -> &'static Mutex<Vec<Stopper>> {
    STOPPERS.get_or_init(|| Mutex::new(Vec::new()))
}

fn waiters() -> &'static Mutex<Vec<Waiter>> {
    WAITERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Whether the process is shutting down.
///
/// Ask BEFORE starting a background task: otherwise a deferred timer would
/// start a new one exactly at the moment the drain is waiting for the old ones.
pub fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst)
}

/// How many background tasks are alive right now.
pub fn live_workers() -> usize {
    LIVE.load(Ordering::SeqCst)
}

/// Adds a "wrap up" request; it is triggered FIRST during the drain.
///
/// This is where a tab puts its cancellation (a `rexon_core::net::CancelToken`
/// token, `TaskTracker::close`): without it the task sits until the request
/// times out, and no wait will ever see it finish.
pub fn register_stopper(f: impl Fn() + Send + Sync + 'static) {
    if let Ok(mut v) = stoppers().lock() {
        v.push(Box::new(f));
    }
}

/// Adds accounting for someone else's workers: it receives the remaining
/// budget (ms) and answers with the number of tasks NOT waited for.
///
/// Needed for what our counter cannot see in principle — Qt's shared
/// `QThreadPool`: its worker threads are started by C++.
pub fn register_waiter(f: impl Fn(u64) -> usize + Send + Sync + 'static) {
    if let Ok(mut v) = waiters().lock() {
        v.push(Box::new(f));
    }
}

/// Takes the list of hooks UNDER the lock and releases the lock.
///
/// The hooks are one-shot: the process exits once, and a non-final drain
/// (closing the window) raises the cancellation of the window that will be
/// opened later anew. Putting them back would be worse than losing them: a
/// cancelled token stays cancelled forever, and the next window would be
/// left without background work.
fn take_hooks<T>(cell: &'static Mutex<Vec<T>>) -> Vec<T> {
    match cell.lock() {
        Ok(mut v) => std::mem::take(&mut *v),
        Err(e) => std::mem::take(&mut *e.into_inner()),
    }
}

/// A live background task, for as long as this guard is held.
///
/// It is created at the START of background work and dropped at the end
/// (including on panic: the counter is decremented by `Drop`, not by the last
/// line of the body).
#[derive(Debug)]
pub struct WorkerGuard {
    /// The guard is neither clonable nor `Copy`: two counts for one task would
    /// mean waiting forever.
    _private: (),
}

impl WorkerGuard {
    /// Registers the task as live.
    #[allow(clippy::new_without_default)]
    pub fn new() -> WorkerGuard {
        LIVE.fetch_add(1, Ordering::SeqCst);
        WorkerGuard { _private: () }
    }
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Starts a background thread that the drain WILL SEE.
///
/// Exactly the same as `std::thread::spawn`, plus a live task guard for the
/// whole running time. A separate door rather than "do not forget to create a
/// `WorkerGuard`": a single forgotten place is enough for the drain to lie
/// about empty background again.
///
/// Returns `None` when shutdown is already in progress: starting work that
/// would immediately have to be waited for means keeping the person in front
/// of a closing window.
pub fn spawn<F>(f: F) -> Option<std::thread::JoinHandle<()>>
where
    F: FnOnce() + Send + 'static,
{
    if is_shutting_down() {
        return None;
    }
    let guard = WorkerGuard::new();
    Some(std::thread::spawn(move || {
        let _guard = guard;
        f();
    }))
}

/// Stops background tasks and waits for them. Returns how many did NOT stop.
///
/// `final_ = false` is a drain after which the process KEEPS living: this is
/// how the main window is closed (it can be reopened in the same process). The
/// "shutting down" flag is still raised for the duration of the drain, but is
/// restored to its previous value afterwards.
///
/// The order is as in the original, and it matters: first ASK (network
/// requests are aborted on the spot), then wait. Waiting without asking means
/// honestly sitting out the whole network timeout.
pub fn stop_background_workers(budget_ms: u64, final_: bool) -> usize {
    let was = SHUTTING_DOWN.swap(true, Ordering::SeqCst);

    // We release the lock BEFORE calling the requests: a request itself may
    // register another one (a tab cancels a child cancellation node), and a
    // `Mutex` is not reentrant — holding it during the call would deadlock the
    // drain.
    for stop in take_hooks(stoppers()) {
        stop();
    }

    let deadline = Instant::now() + Duration::from_millis(budget_ms);
    while live_workers() > 0 {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        if left.is_zero() {
            break;
        }
        std::thread::sleep(left.min(Duration::from_millis(WAIT_SLICE_MS)));
    }

    let left_ms = deadline
        .checked_duration_since(Instant::now())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut pool_left = 0usize;
    for wait in take_hooks(waiters()) {
        pool_left += wait(left_ms);
    }

    SHUTTING_DOWN.store(final_ || was, Ordering::SeqCst);
    live_workers() + pool_left
}

/// A log line about tasks that were not waited for, or `None` if all is clean.
///
/// Staying silent is not allowed (rule #13): a process hanging on exit is what
/// a person sees as "the program does not close", and the cause must remain
/// in the log.
pub fn drain_report(left: usize, pool_left: usize) -> Option<String> {
    if left == 0 {
        return None;
    }
    Some(format!(
        "при выходе не остановилось фоновых задач: {left} \
         (из них задач общего пула: {pool_left}; вероятно блокирующий \
         системный вызов)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The counter is one per process, so this module's checks run UNDER a shared
    /// lock: otherwise parallel libtest would mix up their counters.
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static M: Mutex<()> = Mutex::new(());
        M.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn reset() {
        SHUTTING_DOWN.store(false, Ordering::SeqCst);
        LIVE.store(0, Ordering::SeqCst);
        if let Ok(mut v) = stoppers().lock() {
            v.clear();
        }
        if let Ok(mut v) = waiters().lock() {
            v.clear();
        }
    }

    #[test]
    fn пустой_сток_уходит_сразу_и_ничего_не_ждёт() {
        let _l = lock();
        reset();
        let started = Instant::now();
        assert_eq!(stop_background_workers(DEFAULT_BUDGET_MS, true), 0);
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "пустой сток обязан уходить мгновенно, а не выжидать бюджет"
        );
    }

    #[test]
    fn сток_ждёт_живую_задачу_и_не_врёт_про_пустой_фон() {
        let _l = lock();
        reset();
        let h = spawn(|| std::thread::sleep(Duration::from_millis(250))).unwrap();
        assert_eq!(live_workers(), 1);
        assert_eq!(stop_background_workers(DEFAULT_BUDGET_MS, true), 0);
        assert_eq!(live_workers(), 0);
        h.join().unwrap();
    }

    #[test]
    fn сток_сдаётся_по_бюджету_и_честно_считает_остаток() {
        let _l = lock();
        reset();
        let h = spawn(|| std::thread::sleep(Duration::from_millis(600))).unwrap();
        let started = Instant::now();
        assert_eq!(stop_background_workers(150, true), 1, "задача ещё жива");
        assert!(started.elapsed() < Duration::from_millis(500), "бюджет не соблюдён");
        h.join().unwrap();
        reset();
    }

    #[test]
    fn просьбу_дёргают_до_ожидания() {
        let _l = lock();
        reset();
        let asked = std::sync::Arc::new(AtomicBool::new(false));
        let a = asked.clone();
        register_stopper(move || a.store(true, Ordering::SeqCst));
        stop_background_workers(10, true);
        assert!(asked.load(Ordering::SeqCst), "отмена не сработала");
    }

    /// The outcome of waiting for the shared pool MUST make it into the answer,
    /// otherwise the drain answers "nobody is left" while background work is
    /// running (measured 22.08.2026).
    #[test]
    fn остаток_общего_пула_попадает_в_ответ() {
        let _l = lock();
        reset();
        register_waiter(|_ms| 3);
        assert_eq!(stop_background_workers(10, true), 3);
    }

    /// A non-final drain restores the flag to its previous value: otherwise it
    /// latches forever and the next window is left without background tasks.
    #[test]
    fn неокончательный_сток_не_защёлкивает_признак() {
        let _l = lock();
        reset();
        stop_background_workers(10, false);
        assert!(!is_shutting_down());
        stop_background_workers(10, true);
        assert!(is_shutting_down());
        reset();
    }

    /// No new tasks are started during shutdown.
    #[test]
    fn при_остановке_новую_задачу_не_заводим() {
        let _l = lock();
        reset();
        stop_background_workers(10, true);
        assert!(spawn(|| {}).is_none());
        reset();
    }

    #[test]
    fn отчёт_молчит_когда_всё_чисто() {
        assert_eq!(drain_report(0, 0), None);
        assert!(drain_report(2, 1).unwrap().contains("общего пула: 1"));
    }
}
