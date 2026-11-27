// Thumbnail request of a wallpaper card.
//
// Cards with a ready preview (`source`) are drawn directly in
// `ThumbnailImage.qml`, and this object takes no part in them. It is needed
// for the rest: wallpapers without a picture — first of all clips from
// your own folder. For them a frame is taken from the file itself and put
// into the desktop's shared thumbnail cache; the page gets the address of
// the ready PNG in `cachePath`.
//
// There is no logic here: the source, the place in the cache, freshness,
// calling `ffmpeg` and assembling the PNG belong to the core
// (`rexon_core::we_thumbnail`). Here there is only execution:
//
// * input properties (`source`, `resource`, `wpType`) and output ones
//   (`state`, `cachePath`, `error`); the page reads the `State` enum
//   statically (`W.ThumbnailRequest.Ready`);
// * recomputation — once, after the page has set all the properties:
//   every change queues a deferred call in the main thread, and however
//   many properties change in one pass, there is one call;
// * the quick cache check (`stat`, without decoding) is in the main
//   thread, the work itself in a shared queue of several worker threads;
//   identical requests of different cards are merged into one job.

/// cxx-qt bridge: the thumbnail request of one card.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
    }

    /// Thumbnail state — 1:1 with the original's `ThumbnailRequest::State`.
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
        /// waywallen 0.6.0: "timeline" wallpapers (by time of day), whether the
        /// request is enabled (the card is visible), preloading, keeping the
        /// previous picture while a new one is being prepared, and the long side of
        /// the thumbnail.
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
        /// waywallen 0.6.0: recompute anew (`ThumbnailImage` after a picture
        /// error).
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

/// How many thumbnails are made at the same time: decoding loads the CPU,
/// and there are hundreds of cards on a page.
const MAX_WORKERS: usize = 4;

type Handle = cxx_qt::CxxQtThread<qobject::ThumbnailRequest>;

/// Shared thumbnail queue — one per process.
#[derive(Default)]
struct Service {
    /// Who is waiting for a file's thumbnail: the key is the absolute path of
    /// the source, the value — the requests with the generation number they
    /// are waiting under.
    waiting: HashMap<String, Vec<(Handle, u64)>>,
    queue: VecDeque<th::Pending>,
    workers: usize,
}

static SERVICE: LazyLock<Mutex<Service>> = LazyLock::new(Mutex::default);

fn service() -> MutexGuard<'static, Service> {
    // A poisoned lock is no reason to be left without thumbnails: the queue
    // state is consistent after any interruption (every edit is one action).
    SERVICE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Put a job into the queue or subscribe to one already running.
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

/// Worker thread: takes jobs while there are any, then leaves.
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
        // A decoder crash on a bad file must not take the thread down together
        // with its place in the queue and leave the cards waiting forever.
        let result = std::panic::catch_unwind(|| th::live_generate(&job))
            .unwrap_or_else(|_| Err("thumbnail worker panicked".to_string()));
        if let Err(why) = &result {
            rexon_core::log_warn!("thumbnail", "миниатюра не сделана: {} — {why}", job.path);
        }
        let waiters = service().waiting.remove(&job.path).unwrap_or_default();
        for (handle, generation) in waiters {
            let result = result.clone();
            // The card may already have been removed — then there is nobody to
            // deliver to.
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
    /// Picture address: the path of the ready PNG or the preview address as is
    /// (`file://`, `qrc:`, `https://`); empty means no thumbnail.
    cache_path: String,
    error: String,
    /// Recomputation number. A worker thread's answer is accepted only if the
    /// properties have not changed since it was sent.
    generation: u64,
    /// A recomputation is already queued in the main thread.
    scheduled: bool,
}

impl Default for ThumbnailRequestRust {
    fn default() -> Self {
        Self {
            source: String::new(),
            resource: String::new(),
            wp_type: String::new(),
            timeline: false,
            // As in the original: enabled until the page says otherwise.
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

/// Setter of a boolean property that recomputes the thumbnail.
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
        // The first recomputation is needed even if the page sets no property:
        // then the request honestly becomes `Failed` rather than hanging in
        // `Idle`.
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

    /// In the original, preloading changes only the order in the pool. Our
    /// frame queue is one for everybody and short (a frame is only needed for
    /// clips without a preview), so the property is accepted and remembered but
    /// does not change the order.
    pub fn set_prefetch(mut self: Pin<&mut Self>, value: bool) {
        if self.prefetch == value {
            return;
        }
        self.as_mut().rust_mut().prefetch = value;
        self.prefetch_changed();
    }

    /// Without keeping, the picture disappears as soon as the request goes into
    /// waiting (original: `publish(state, {})` with `!value && state != Ready`).
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

    /// The long side, 1…4096. Our generator captures a clip frame at
    /// [`th::MAX_EDGE`] (a card never asks for more); the value is for the page.
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

    /// Defer the recomputation until the return to the event loop — one for any
    /// number of property changes in a row.
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

    /// Recompute by the current properties: a cache hit or a refusal at once,
    /// otherwise a job into the shared queue.
    fn resolve(mut self: Pin<&mut Self>) {
        let generation = self.generation + 1;
        {
            let mut rust = self.as_mut().rust_mut();
            rust.scheduled = false;
            // The previous answer, if it is still on its way, is stale from this point.
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

    /// Answer of the worker thread (already in the main thread).
    fn apply_result(self: Pin<&mut Self>, generation: u64, result: Result<String, String>) {
        if self.generation != generation {
            return;
        }
        match result {
            Ok(path) => self.apply(th::STATE_READY, path, String::new()),
            Err(why) => {
                // As in the original: with keeping, the previous picture stays.
                let kept = if self.retain { self.cache_path.clone() } else { String::new() };
                self.apply(th::STATE_FAILED, kept, why)
            }
        }
    }

    /// Set the three output properties, notifying only about the changed ones.
    /// `state` goes last: a page that sees `Ready` already reads a ready
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
