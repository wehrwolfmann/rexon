//! Сток фоновых задач при закрытии — порт `install_exit_guard` и
//! `stop_background_workers` из `rexon/utils.py` (61 строка решений).
//!
//! ЗАЧЕМ. Покупатель закрывает окно, не дождавшись ответа магазина тем или
//! скана установленного. Фоновая задача в этот момент сидит внутри
//! блокирующего сетевого вызова, цикл событий уже кончился — и процесс либо
//! падает (у Qt: `QThread: Destroyed while thread is still running` →
//! SIGABRT), либо обрывает задачу посреди записи файла. Человеку это
//! выглядит как «Rexon неожиданно завершился», хотя он просто закрыл окно.
//!
//! ОДНА ДВЕРЬ (правило #12). Порядок остановки — «попросить → дождаться →
//! досчитать пул» — живёт ЗДЕСЬ, один на процесс. Вкладкам не надо помнить
//! про свои потоки: достаточно, чтобы задача заводилась через
//! [`spawn`] (её посчитают) и, если у неё есть отмена, чтобы вкладка
//! положила эту отмену в [`register_stopper`]. Ровно из-за отсутствия такой
//! двери в оригинале фикс-ожидание лежало в `closeEvent` двух вкладок, эти
//! методы Qt никогда не вызывал, и падение возвращалось.
//!
//! ЧЕМ ОТЛИЧАЕТСЯ ОТ ОРИГИНАЛА (правило #16 — грабли не консервируем).
//! Python искал живые потоки обходом `gc.get_objects()`: полный обход дерева
//! объектов на каждый вызов, да ещё и слепой к потокам, заведённым C++ (у
//! них нет питоновской обёртки — отсюда отдельная возня с `QThreadPool`).
//! Здесь живые задачи считает сама [`WorkerGuard`] — счётчик, а не обход,
//! поэтому ожидание не дорожает с размером дерева объектов и не врёт.
//!
//! ЧТО СОХРАНЕНО 1-в-1: ломтями по [`WAIT_SLICE_MS`], а не одним ожиданием
//! на весь срок (выход не должен дорожать ни на миллисекунду, если задачи
//! кончились раньше); бюджет [`DEFAULT_BUDGET_MS`]; исход ожидания общего
//! пула ОБЯЗАН попасть в ответ (замер 22.08.2026: пул ждали ради побочного
//! действия, а возвращали только число потоков — сток отвечал «не осталось
//! никого» при работающем фоне); признак «идёт остановка» на время
//! неокончательного стока взводится и возвращается в прежнее значение,
//! иначе он защёлкивается навсегда и следующее окно в том же процессе
//! остаётся без фоновых задач.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Сколько всего ждём фоновые задачи. Это ПОТОЛОК, а не плановое ожидание:
/// отмена уже взведена, задачи рвут запросы сами, и в норме сток занимает
/// миллисекунды.
pub const DEFAULT_BUDGET_MS: u64 = 8000;

/// Ломоть ожидания. Ждём ломтями, а не одним сном на весь бюджет, чтобы
/// уйти сразу, как только последняя задача закончилась.
pub const WAIT_SLICE_MS: u64 = 100;

/// Идёт ли остановка. Взведён — новых фоновых задач заводить нельзя.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Сколько фоновых задач сейчас живо (считает [`WorkerGuard`]).
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// Просьбы закругляться: рвут сетевые запросы, взводят токены отмены.
type Stopper = Box<dyn Fn() + Send + Sync + 'static>;

/// Досчёт чужого хозяйства (общий `QThreadPool` Qt): получает остаток
/// бюджета в миллисекундах, отвечает числом НЕ дождавшихся.
type Waiter = Box<dyn Fn(u64) -> usize + Send + Sync + 'static>;

static STOPPERS: OnceLock<Mutex<Vec<Stopper>>> = OnceLock::new();
static WAITERS: OnceLock<Mutex<Vec<Waiter>>> = OnceLock::new();

fn stoppers() -> &'static Mutex<Vec<Stopper>> {
    STOPPERS.get_or_init(|| Mutex::new(Vec::new()))
}

fn waiters() -> &'static Mutex<Vec<Waiter>> {
    WAITERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Идёт ли остановка процесса.
///
/// Спрашивать ПЕРЕД тем, как завести фоновую задачу: иначе отложенный
/// таймер заведёт новую ровно в тот миг, когда сток дожидается старых.
pub fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst)
}

/// Сколько фоновых задач живо прямо сейчас.
pub fn live_workers() -> usize {
    LIVE.load(Ordering::SeqCst)
}

/// Добавить просьбу «закругляйся» — её дёрнут ПЕРВЫМ делом при стоке.
///
/// Сюда кладут отмену вкладки (токен `rexon_core::net::CancelToken`,
/// `TaskTracker::close`): без неё задача досиживает до таймаута запроса, и
/// никакое ожидание её не дождётся.
pub fn register_stopper(f: impl Fn() + Send + Sync + 'static) {
    if let Ok(mut v) = stoppers().lock() {
        v.push(Box::new(f));
    }
}

/// Добавить досчёт чужого хозяйства: получает остаток бюджета (мс), отвечает
/// числом НЕ дождавшихся задач.
///
/// Нужен для того, что наш счётчик не видит в принципе, — общего
/// `QThreadPool` Qt: его рабочие потоки заводит C++.
pub fn register_waiter(f: impl Fn(u64) -> usize + Send + Sync + 'static) {
    if let Ok(mut v) = waiters().lock() {
        v.push(Box::new(f));
    }
}

/// Забрать список зацепок ПОД замком и отпустить замок.
///
/// Зацепки одноразовые: процесс выходит один раз, а неокончательный сток
/// (закрытие окна) заново взводит отмену того окна, которое откроют потом.
/// Вернуть их назад было бы хуже, чем потерять: отменённый токен отменяет
/// вечно, и следующее окно осталось бы без фона.
fn take_hooks<T>(cell: &'static Mutex<Vec<T>>) -> Vec<T> {
    match cell.lock() {
        Ok(mut v) => std::mem::take(&mut *v),
        Err(e) => std::mem::take(&mut *e.into_inner()),
    }
}

/// Живая фоновая задача, пока держат эту метку.
///
/// Заводится в НАЧАЛЕ фоновой работы и роняется в конце (в том числе при
/// панике: снимает счётчик `Drop`, а не последняя строка тела).
#[derive(Debug)]
pub struct WorkerGuard {
    /// Метка неклонируема и не `Copy`: два счётчика на одну задачу дали бы
    /// вечное ожидание.
    _private: (),
}

impl WorkerGuard {
    /// Записать задачу в живые.
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

/// Завести фоновый поток, который сток УВИДИТ.
///
/// Ровно то же, что `std::thread::spawn`, плюс метка живой задачи на всё
/// время работы. Отдельная дверь, а не «не забудьте завести `WorkerGuard`»:
/// достаточно одного забытого места, чтобы сток снова врал про пустой фон.
///
/// Отвечает `None`, когда остановка уже идёт: заводить работу, которую тут
/// же придётся ждать, — значит держать человека перед закрывающимся окном.
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

/// Остановить фоновые задачи и дождаться их. → сколько НЕ остановилось.
///
/// `final_ = false` — осушение, после которого процесс ПРОДОЛЖИТ жить: так
/// закрывается главное окно (его можно открыть заново в том же процессе).
/// Признак «идёт остановка» на время осушения всё равно взводится, но по
/// окончании возвращается в прежнее значение.
///
/// Порядок — как в оригинале и он важен: сначала ПОПРОСИТЬ (сетевые запросы
/// рвутся на месте), потом ждать. Ждать не попросив — значит честно
/// досидеть до конца сетевого таймаута.
pub fn stop_background_workers(budget_ms: u64, final_: bool) -> usize {
    let was = SHUTTING_DOWN.swap(true, Ordering::SeqCst);

    // Замок отпускаем ДО вызова просьб: сама просьба может завести ещё
    // одну (вкладка гасит дочерний узел отмены), а `Mutex` не рекурсивен —
    // держать его на время вызова значило бы висеть намертво в стоке.
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

/// Строка для журнала о недождавшихся задачах, либо `None` — всё чисто.
///
/// Молчать нельзя (правило #13): подвисший на выходе процесс — это то, что
/// человек увидит как «программа не закрывается», и в журнале обязана
/// остаться причина.
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

    /// Счётчик один на процесс, поэтому сверки этого модуля идут ПОД общим
    /// замком: параллельный libtest иначе смешал бы их счётчики.
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

    /// Исход ожидания общего пула ОБЯЗАН попасть в ответ — иначе сток
    /// отвечает «никого не осталось» при работающем фоне (замер 22.08.2026).
    #[test]
    fn остаток_общего_пула_попадает_в_ответ() {
        let _l = lock();
        reset();
        register_waiter(|_ms| 3);
        assert_eq!(stop_background_workers(10, true), 3);
    }

    /// Неокончательный сток возвращает признак в прежнее значение: иначе он
    /// защёлкивается навсегда и следующее окно остаётся без фоновых задач.
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

    /// Во время остановки новых задач не заводим.
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
