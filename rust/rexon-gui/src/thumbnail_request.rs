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

#[derive(Default)]
pub struct ThumbnailRequestRust {
    source: String,
    resource: String,
    wp_type: String,
    state: i32,
    /// Путь готового PNG; пусто — миниатюры нет.
    cache_path: String,
    error: String,
    /// Номер пересчёта. Ответ рабочего потока принимается, только если с
    /// его отправки свойства не менялись.
    generation: u64,
    /// Пересчёт уже стоит в очереди главного потока.
    scheduled: bool,
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
        } else {
            QUrl::from_local_file(&QString::from(&self.cache_path))
        }
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
        match th::live_lookup(&self.source, &self.resource, &self.wp_type) {
            th::Lookup::Ready(path) => self.apply(th::STATE_READY, path, String::new()),
            th::Lookup::Failed(why) => self.apply(th::STATE_FAILED, String::new(), why),
            th::Lookup::Miss(job) => {
                self.as_mut().apply(th::STATE_LOADING, String::new(), String::new());
                submit(job, self.qt_thread(), generation);
            }
        }
    }

    /// Ответ рабочего потока (уже в главном потоке).
    fn apply_result(self: Pin<&mut Self>, generation: u64, result: Result<String, String>) {
        if self.generation != generation {
            return;
        }
        match result {
            Ok(path) => self.apply(th::STATE_READY, path, String::new()),
            Err(why) => self.apply(th::STATE_FAILED, String::new(), why),
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
