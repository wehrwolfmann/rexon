// Запрос миниатюры карточки обои.
//
// Карточки с готовым превью (`source`) рисуются напрямую в
// `ThumbnailImage.qml`, и этот объект в них не участвует. Он нужен остатку:
// обоям без картинки — в первую очередь роликам из своей папки. Для них
// кадр берётся из самого файла и кладётся в общий кэш миниатюр рабочего
// стола; страница получает адрес готового PNG в `cachePath`.
//
// Логики здесь нет: источник, место в кэше, свежесть, вызов `ffmpeg` и
// сборка PNG — ядро (`rexon_core::we_thumbnail`). Здесь только исполнение:
//
// * свойства на вход (`source`, `resource`, `wpType`) и на выход (`state`,
//   `cachePath`, `error`); перечисление `State` страница читает статически
//   (`W.ThumbnailRequest.Ready`);
// * пересчёт — один раз после того, как страница выставила все свойства:
//   каждая смена ставит отложенный вызов в очередь главного потока, и
//   сколько бы свойств ни сменилось за один проход, вызов один;
// * быстрая проверка кэша (`stat`, без декодирования) — в главном потоке,
//   сама работа — в общей очереди из нескольких рабочих потоков; одинаковые
//   запросы разных карточек сливаются в одну работу.

/// Мост cxx-qt: запрос миниатюры одной карточки.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
    }

    /// Состояние миниатюры — 1:1 с `ThumbnailRequest::State` оригинала.
    #[qenum(ThumbnailRequest)]
    enum State {
        Idle,
        Loading,
        Ready,
        Failed,
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, source, READ = source, WRITE = set_source,
                    NOTIFY = source_changed)]
        #[qproperty(QString, resource, READ = resource, WRITE = set_resource,
                    NOTIFY = resource_changed)]
        #[qproperty(QString, wp_type, cxx_name = "wpType", READ = wp_type,
                    WRITE = set_wp_type, NOTIFY = wp_type_changed)]
        #[qproperty(i32, state, READ = state, NOTIFY = state_changed)]
        #[qproperty(QUrl, cache_path, cxx_name = "cachePath", READ = cache_path,
                    NOTIFY = cache_path_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        /// waywallen 0.6.0: обои-«таймлайн» (по времени суток), запрос
        /// включён (карточка видна), предзагрузка, держать прежнюю картинку,
        /// пока готовится новая, и длинная сторона миниатюры.
        #[qproperty(bool, timeline, READ = timeline, WRITE = set_timeline,
                    NOTIFY = timeline_changed)]
        #[qproperty(bool, enabled, READ = enabled, WRITE = set_enabled,
                    NOTIFY = enabled_changed)]
        #[qproperty(bool, prefetch, READ = prefetch, WRITE = set_prefetch,
                    NOTIFY = prefetch_changed)]
        #[qproperty(bool, retain_while_loading, cxx_name = "retainWhileLoading",
                    READ = retain_while_loading, WRITE = set_retain_while_loading,
                    NOTIFY = retain_while_loading_changed)]
        #[qproperty(i32, maximum_edge, cxx_name = "maximumEdge", READ = maximum_edge,
                    WRITE = set_maximum_edge, NOTIFY = maximum_edge_changed)]
        type ThumbnailRequest = super::ThumbnailRequestRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "sourceChanged"]
        fn source_changed(self: Pin<&mut ThumbnailRequest>);
        #[qsignal]
        #[cxx_name = "resourceChanged"]
        fn resource_changed(self: Pin<&mut ThumbnailRequest>);
        #[qsignal]
        #[cxx_name = "wpTypeChanged"]
        fn wp_type_changed(self: Pin<&mut ThumbnailRequest>);
        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut ThumbnailRequest>);
        #[qsignal]
        #[cxx_name = "cachePathChanged"]
        fn cache_path_changed(self: Pin<&mut ThumbnailRequest>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut ThumbnailRequest>);
        #[qsignal]
        #[cxx_name = "timelineChanged"]
        fn timeline_changed(self: Pin<&mut ThumbnailRequest>);
        #[qsignal]
        #[cxx_name = "enabledChanged"]
        fn enabled_changed(self: Pin<&mut ThumbnailRequest>);
        #[qsignal]
        #[cxx_name = "prefetchChanged"]
        fn prefetch_changed(self: Pin<&mut ThumbnailRequest>);
        #[qsignal]
        #[cxx_name = "retainWhileLoadingChanged"]
        fn retain_while_loading_changed(self: Pin<&mut ThumbnailRequest>);
        #[qsignal]
        #[cxx_name = "maximumEdgeChanged"]
        fn maximum_edge_changed(self: Pin<&mut ThumbnailRequest>);
    }

    extern "RustQt" {
        fn source(self: &ThumbnailRequest) -> QString;
        #[cxx_name = "setSource"]
        fn set_source(self: Pin<&mut ThumbnailRequest>, value: QString);
        fn resource(self: &ThumbnailRequest) -> QString;
        #[cxx_name = "setResource"]
        fn set_resource(self: Pin<&mut ThumbnailRequest>, value: QString);
        #[cxx_name = "wpType"]
        fn wp_type(self: &ThumbnailRequest) -> QString;
        #[cxx_name = "setWpType"]
        fn set_wp_type(self: Pin<&mut ThumbnailRequest>, value: QString);
        fn state(self: &ThumbnailRequest) -> i32;
        #[cxx_name = "cachePath"]
        fn cache_path(self: &ThumbnailRequest) -> QUrl;
        fn error(self: &ThumbnailRequest) -> QString;
        fn timeline(self: &ThumbnailRequest) -> bool;
        #[cxx_name = "setTimeline"]
        fn set_timeline(self: Pin<&mut ThumbnailRequest>, value: bool);
        fn enabled(self: &ThumbnailRequest) -> bool;
        #[cxx_name = "setEnabled"]
        fn set_enabled(self: Pin<&mut ThumbnailRequest>, value: bool);
        fn prefetch(self: &ThumbnailRequest) -> bool;
        #[cxx_name = "setPrefetch"]
        fn set_prefetch(self: Pin<&mut ThumbnailRequest>, value: bool);
        #[cxx_name = "retainWhileLoading"]
        fn retain_while_loading(self: &ThumbnailRequest) -> bool;
        #[cxx_name = "setRetainWhileLoading"]
        fn set_retain_while_loading(self: Pin<&mut ThumbnailRequest>, value: bool);
        #[cxx_name = "maximumEdge"]
        fn maximum_edge(self: &ThumbnailRequest) -> i32;
        #[cxx_name = "setMaximumEdge"]
        fn set_maximum_edge(self: Pin<&mut ThumbnailRequest>, value: i32);
    }

    extern "RustQt" {
        /// waywallen 0.6.0: пересчитать заново (`ThumbnailImage` после
        /// ошибки картинки).
        #[qinvokable]
        fn reload(self: Pin<&mut ThumbnailRequest>);
    }

    impl cxx_qt::Threading for ThumbnailRequest {}
    impl cxx_qt::Initialize for ThumbnailRequest {}
}

use core::pin::Pin;
use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex, MutexGuard};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QString, QUrl};
use rexon_core::we_thumbnail as th;

/// Сколько миниатюр делается одновременно: декодирование грузит процессор,
/// а карточек на странице сотни.
const MAX_WORKERS: usize = 4;

type Handle = cxx_qt::CxxQtThread<qobject::ThumbnailRequest>;

/// Общая очередь миниатюр — одна на процесс.
#[derive(Default)]
struct Service {
    /// Кто ждёт миниатюру файла: ключ — абсолютный путь источника,
    /// значение — запросы с номером поколения, под которым они ждут.
    waiting: HashMap<String, Vec<(Handle, u64)>>,
    queue: VecDeque<th::Pending>,
    workers: usize,
}

static SERVICE: LazyLock<Mutex<Service>> = LazyLock::new(Mutex::default);

fn service() -> MutexGuard<'static, Service> {
    // Отравленный замок не повод остаться без миниатюр: состояние очереди
    // цельно при любом обрыве (каждая правка — одно действие).
    SERVICE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Поставить работу в очередь либо подписаться на уже идущую.
fn submit(job: th::Pending, handle: Handle, generation: u64) {
    let mut svc = service();
    if let Some(waiters) = svc.waiting.get_mut(&job.path) {
        waiters.push((handle, generation));
        return;
    }
    svc.waiting.insert(job.path.clone(), vec![(handle, generation)]);
    svc.queue.push_back(job);
    let limit = std::thread::available_parallelism().map_or(1, |n| n.get()).min(MAX_WORKERS);
    if svc.workers < limit {
        svc.workers += 1;
        drop(svc);
        let spawned = std::thread::Builder::new().name("rexon-thumbnail".into()).spawn(worker);
        if spawned.is_err() {
            service().workers -= 1;
        }
    }
}

/// Рабочий поток: берёт работы, пока они есть, и уходит.
fn worker() {
    loop {
        let job = {
            let mut svc = service();
            match svc.queue.pop_front() {
                Some(job) => job,
                None => {
                    svc.workers -= 1;
                    return;
                }
            }
        };
        // Падение декодера на кривом файле не должно унести поток вместе с
        // местом в очереди и оставить карточки ждать вечно.
        let result = std::panic::catch_unwind(|| th::live_generate(&job))
            .unwrap_or_else(|_| Err("thumbnail worker panicked".to_string()));
        if let Err(why) = &result {
            rexon_core::log_warn!("thumbnail", "миниатюра не сделана: {} — {why}", job.path);
        }
        let waiters = service().waiting.remove(&job.path).unwrap_or_default();
        for (handle, generation) in waiters {
            let result = result.clone();
            // Карточку могли уже убрать — тогда доставлять некому.
            let _ = handle.queue(move |object| object.apply_result(generation, result));
        }
    }
}

pub struct ThumbnailRequestRust {
    source: String,
    resource: String,
    wp_type: String,
    timeline: bool,
    enabled: bool,
    prefetch: bool,
    retain: bool,
    maximum_edge: i32,
    state: i32,
    /// Адрес картинки: путь готового PNG либо адрес превью как есть
    /// (`file://`, `qrc:`, `https://`); пусто — миниатюры нет.
    cache_path: String,
    error: String,
    /// Номер пересчёта. Ответ рабочего потока принимается, только если с
    /// его отправки свойства не менялись.
    generation: u64,
    /// Пересчёт уже стоит в очереди главного потока.
    scheduled: bool,
}

impl Default for ThumbnailRequestRust {
    fn default() -> Self {
        Self {
            source: String::new(),
            resource: String::new(),
            wp_type: String::new(),
            timeline: false,
            // Как у оригинала: включён, пока страница не скажет иного.
            enabled: true,
            prefetch: false,
            retain: false,
            maximum_edge: th::MAX_EDGE as i32,
            state: th::STATE_IDLE,
            cache_path: String::new(),
            error: String::new(),
            generation: 0,
            scheduled: false,
        }
    }
}

/// Сеттер булева свойства, пересчитывающий миниатюру.
macro_rules! flag_setter {
    ($set:ident, $field:ident, $signal:ident) => {
        pub fn $set(mut self: Pin<&mut Self>, value: bool) {
            if self.$field == value {
                return;
            }
            self.as_mut().rust_mut().$field = value;
            self.as_mut().$signal();
            self.schedule();
        }
    };
}

impl cxx_qt::Initialize for qobject::ThumbnailRequest {
    fn initialize(self: Pin<&mut Self>) {
        // Первый пересчёт нужен, даже если страница не выставит ни одного
        // свойства: тогда запрос честно становится `Failed`, а не висит `Idle`.
        self.schedule();
    }
}

impl qobject::ThumbnailRequest {
    pub fn source(&self) -> QString {
        QString::from(&self.source)
    }

    pub fn set_source(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.source == next {
            return;
        }
        self.as_mut().rust_mut().source = next;
        self.as_mut().source_changed();
        self.schedule();
    }

    pub fn resource(&self) -> QString {
        QString::from(&self.resource)
    }

    pub fn set_resource(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.resource == next {
            return;
        }
        self.as_mut().rust_mut().resource = next;
        self.as_mut().resource_changed();
        self.schedule();
    }

    pub fn wp_type(&self) -> QString {
        QString::from(&self.wp_type)
    }

    pub fn set_wp_type(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.wp_type == next {
            return;
        }
        self.as_mut().rust_mut().wp_type = next;
        self.as_mut().wp_type_changed();
        self.schedule();
    }

    pub fn state(&self) -> i32 {
        self.state
    }

    pub fn cache_path(&self) -> QUrl {
        if self.cache_path.is_empty() {
            QUrl::default()
        } else if self.cache_path.starts_with('/') {
            QUrl::from_local_file(&QString::from(&self.cache_path))
        } else {
            QUrl::from(&QString::from(&self.cache_path))
        }
    }

    pub fn timeline(&self) -> bool {
        self.timeline
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn prefetch(&self) -> bool {
        self.prefetch
    }

    pub fn retain_while_loading(&self) -> bool {
        self.retain
    }

    pub fn maximum_edge(&self) -> i32 {
        self.maximum_edge
    }

    flag_setter!(set_timeline, timeline, timeline_changed);
    flag_setter!(set_enabled, enabled, enabled_changed);

    /// Предзагрузка меняет у оригинала только очерёдность в пуле. Наша
    /// очередь кадров одна на всех и коротка (кадр нужен лишь роликам без
    /// превью), поэтому свойство принимается и запоминается, но порядок не
    /// меняет.
    pub fn set_prefetch(mut self: Pin<&mut Self>, value: bool) {
        if self.prefetch == value {
            return;
        }
        self.as_mut().rust_mut().prefetch = value;
        self.prefetch_changed();
    }

    /// Без удержания картинка пропадает сразу, как только запрос ушёл в
    /// ожидание (оригинал: `publish(state, {})` при `!value && state != Ready`).
    pub fn set_retain_while_loading(mut self: Pin<&mut Self>, value: bool) {
        if self.retain == value {
            return;
        }
        self.as_mut().rust_mut().retain = value;
        if !value && self.state != th::STATE_READY {
            let (state, error) = (self.state, self.error.clone());
            self.as_mut().apply(state, String::new(), error);
        }
        self.retain_while_loading_changed();
    }

    /// Длинная сторона, 1…4096. Кадр ролика наш генератор снимает в
    /// [`th::MAX_EDGE`] (карточка больше не просит), значение — для страницы.
    pub fn set_maximum_edge(mut self: Pin<&mut Self>, value: i32) {
        let value = value.clamp(1, 4096);
        if self.maximum_edge == value {
            return;
        }
        self.as_mut().rust_mut().maximum_edge = value;
        self.as_mut().maximum_edge_changed();
        self.schedule();
    }

    pub fn reload(self: Pin<&mut Self>) {
        self.schedule();
    }

    pub fn error(&self) -> QString {
        QString::from(&self.error)
    }

    /// Отложить пересчёт до возврата в цикл событий — один на сколько
    /// угодно смен свойств подряд.
    fn schedule(mut self: Pin<&mut Self>) {
        if self.scheduled {
            return;
        }
        self.as_mut().rust_mut().scheduled = true;
        let queued = self.qt_thread().queue(|object| object.resolve());
        if queued.is_err() {
            self.as_mut().rust_mut().scheduled = false;
        }
    }

    /// Пересчёт по текущим свойствам: кэш либо отказ — сразу, иначе работа
    /// в общую очередь.
    fn resolve(mut self: Pin<&mut Self>) {
        let generation = self.generation + 1;
        {
            let mut rust = self.as_mut().rust_mut();
            rust.scheduled = false;
            // Прежний ответ, если он ещё в пути, с этого места устарел.
            rust.generation = generation;
        }
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "/".to_string());
        let plan = th::plan(
            &self.source,
            &self.resource,
            &self.wp_type,
            self.timeline,
            self.enabled,
            &cwd,
        );
        match plan {
            th::Plan::Idle => {
                let kept = if self.retain { self.cache_path.clone() } else { String::new() };
                self.apply(th::STATE_IDLE, kept, String::new());
            }
            th::Plan::Direct(url) => self.apply(th::STATE_READY, url, String::new()),
            th::Plan::Failed(why) => self.apply(th::STATE_FAILED, String::new(), why),
            th::Plan::Generate => match th::live_lookup("", &self.resource, "video") {
                th::Lookup::Ready(path) => self.apply(th::STATE_READY, path, String::new()),
                th::Lookup::Failed(why) => self.apply(th::STATE_FAILED, String::new(), why),
                th::Lookup::Miss(job) => {
                    let kept = if self.retain { self.cache_path.clone() } else { String::new() };
                    self.as_mut().apply(th::STATE_LOADING, kept, String::new());
                    submit(job, self.qt_thread(), generation);
                }
            },
        }
    }

    /// Ответ рабочего потока (уже в главном потоке).
    fn apply_result(self: Pin<&mut Self>, generation: u64, result: Result<String, String>) {
        if self.generation != generation {
            return;
        }
        match result {
            Ok(path) => self.apply(th::STATE_READY, path, String::new()),
            Err(why) => {
                // Как у оригинала: при удержании прежняя картинка остаётся.
                let kept = if self.retain { self.cache_path.clone() } else { String::new() };
                self.apply(th::STATE_FAILED, kept, why)
            }
        }
    }

    /// Выставить три выходных свойства, оповестив только о сменившихся.
    /// `state` — последним: страница, увидев `Ready`, уже читает готовый
    /// `cachePath`.
    fn apply(mut self: Pin<&mut Self>, state: i32, cache_path: String, error: String) {
        if self.cache_path != cache_path {
            self.as_mut().rust_mut().cache_path = cache_path;
            self.as_mut().cache_path_changed();
        }
        if self.error != error {
            self.as_mut().rust_mut().error = error;
            self.as_mut().error_changed();
        }
        if self.state != state {
            self.as_mut().rust_mut().state = state;
            self.as_mut().state_changed();
        }
    }
}
