// AxonActions — the model of the "Razer Axon" source of the "Appearance →
// HD Wallpapers" section and of its block in the "⚙️ Settings" of the same
// section.
//
// There is no logic here: what goes into the grid, in what order and what
// to tell the user belong to the core (`rexon_core::axon_window`); reading
// the library — `axon_library`, preparation and our settings —
// `axon_prepare`. The object holds only the library it has read and the
// query (search, mode, sorting, filters), so that searching through the
// set does not re-read the Axon files.
//
// The source only reads Axon's files (Wehrwolfmann's decision 14.11.2026):
// "Apply" prepares the wallpaper into the Rexon cache or takes one prepared
// in advance and sets it with the shared wallpaper executor — the same as
// for wallpaper sets.
//
// Until 14.11.2026 the same actions lived in the temporary "Razer Axon
// (test)" window on the "Extensions" page (`ExtensionsActions::axon*`).

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type AxonActions = super::AxonActionsRust;
    }

    unsafe extern "RustQt" {
        /// A background job ("Apply", restoring descriptions) has finished:
        /// `message` is the ready result line.
        #[qsignal]
        fn finished(self: Pin<&mut AxonActions>, message: QString);

        /// The tile covers arrived: the grid and the details should be re-read.
        #[qsignal]
        #[cxx_name = "coversReady"]
        fn covers_ready(self: Pin<&mut AxonActions>);
    }

    extern "RustQt" {
        /// All labels of the source and the settings block as one JSON.
        #[qinvokable]
        fn texts(self: &AxonActions) -> QString;

        /// Re-read the Axon library (entering the source, "Refresh").
        #[qinvokable]
        fn reload(self: Pin<&mut AxonActions>);

        /// Grid for the current query: JSON `axon_window::view_json`.
        #[qinvokable]
        fn view(self: &AxonActions) -> QString;

        /// Search string.
        #[qinvokable]
        #[cxx_name = "setSearch"]
        fn set_search(self: Pin<&mut AxonActions>, text: &QString);

        /// Search mode: `titles` | `tags`.
        #[qinvokable]
        #[cxx_name = "setSearchMode"]
        fn set_search_mode(self: Pin<&mut AxonActions>, key: &QString);

        /// Sorting: `name` | `date`.
        #[qinvokable]
        #[cxx_name = "setSort"]
        fn set_sort(self: Pin<&mut AxonActions>, key: &QString);

        /// A filter panel check box (`axon_window::FilterFlag::key`) or a "Type"
        /// item (`Dynamic` | `Static` | `Interactive`).
        #[qinvokable]
        #[cxx_name = "setFilter"]
        fn set_filter(self: Pin<&mut AxonActions>, key: &QString, on: bool);

        /// "Clear" of the filter panel.
        #[qinvokable]
        #[cxx_name = "clearFilters"]
        fn clear_filters(self: Pin<&mut AxonActions>);

        /// The "Clear filter" link: filters and search.
        #[qinvokable]
        #[cxx_name = "clearFilter"]
        fn clear_filter(self: Pin<&mut AxonActions>);

        /// Details of wallpaper `id`: JSON `axon_window::details_json`; empty means
        /// there is no such wallpaper.
        #[qinvokable]
        fn details(self: &AxonActions, id: &QString) -> QString;

        /// Prepare wallpaper `id` and put it on the desktop (on all screens).
        /// Returns a "preparing…" line; the result comes with the `finished`
        /// signal.
        #[qinvokable]
        fn apply(self: Pin<&mut AxonActions>, id: &QString) -> QString;

        /// "Go to file location": the wallpaper folder in the file manager.
        #[qinvokable]
        #[cxx_name = "openLocation"]
        fn open_location(self: &AxonActions, id: &QString) -> bool;

        /// Open folder `path` in the file manager.
        #[qinvokable]
        #[cxx_name = "openFolder"]
        fn open_folder(self: &AxonActions, path: &QString) -> bool;
        /// Whether there is something to launch Razer Axon with (the "Open
        /// Razer Axon" button under the "Available to sync" section); decided
        /// when the library is read.
        #[qinvokable]
        #[cxx_name = "canLaunch"]
        fn can_launch(self: &AxonActions) -> bool;
        /// "Open Razer Axon": launch it separately from Rexon. Returns the
        /// result line for the status bar.
        #[qinvokable]
        #[cxx_name = "launchAxon"]
        fn launch_axon(self: &AxonActions) -> QString;

        /// Remember the Axon wallpaper folder (`WallpaperSource`) chosen by the user
        /// (`file://…` or a path). JSON `{ok, message}`.
        #[qinvokable]
        #[cxx_name = "addFolder"]
        fn add_folder(self: Pin<&mut AxonActions>, url: &QString) -> QString;

        /// Settings block: JSON `{line, running, enabled, dirs}` — the line
        /// "Prepared: N, remaining: M", the switch and the library folders.
        #[qinvokable]
        #[cxx_name = "settingsState"]
        fn settings_state(self: &AxonActions) -> QString;

        /// "Prepare wallpapers in advance": remember it and, if turned on, start
        /// the background preparation at once.
        #[qinvokable]
        #[cxx_name = "setPrepareAhead"]
        fn set_prepare_ahead(self: &AxonActions, on: bool);

        /// "Restore original descriptions": turns off preparing in advance and
        /// restores ResourceConfig.txt from the backups. The result comes with
        /// `finished`.
        #[qinvokable]
        #[cxx_name = "restoreOriginals"]
        fn restore_originals(self: Pin<&mut AxonActions>);
    }

    impl cxx_qt::Threading for AxonActions {}
}

use core::pin::Pin;

use cxx_qt::{CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::QString;
use serde_json::{json, Value};

use rexon_core::axon_library::{self as ax, AxonLibrary};
use rexon_core::axon_prepare as ap;
use rexon_core::axon_window::{self as aw, EffectKind, FilterFlag, Query, SearchMode, SortOrder};
use rexon_core::wallpaper_screens::{orientation_group_label, WallpaperOrientation};

/// The library that was read and the grid query.
#[derive(Default)]
pub struct AxonActionsRust {
    lib: AxonLibrary,
    query: Query,
    /// Cover addresses that are being downloaded now or failed to download in
    /// this session: entering the source again does not ask for them a second
    /// time.
    covers_asked: std::collections::HashSet<String>,
    /// How to launch Razer Axon itself (`axon_library::launch_plan_for`);
    /// `None` — nothing to launch it with, and there is no button.
    launch: Option<ax::AxonLaunch>,
}

/// A ready copy of a cover by address — in the shared "Appearance" preview
/// cache (the same path where the KDE Store and Wallhaven downloads put it).
fn cached_cover(url: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    let p = std::path::PathBuf::from(rexon_core::appearance_page::preview_cache_path(&home, url));
    p.is_file().then_some(p)
}

/// Download covers one by one into the shared preview cache; the ones that
/// arrived are handed to the interface thread in batches, so the grid does
/// not rebuild for each one.
fn fetch_covers(handle: cxx_qt::CxxQtThread<qobject::AxonActions>, urls: Vec<String>) {
    use rexon_core::net::UreqNet;
    use rexon_core::ocs::{LiveOcsIo, OcsClient};
    const BATCH: usize = 8;
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    let io = LiveOcsIo::with_net(UreqNet::new(), &home);
    let preview_dir = io.preview_dir();
    let mut client = OcsClient::new(io, &preview_dir);
    let mut done: Vec<(String, std::path::PathBuf)> = Vec::new();
    let total = urls.len();
    for (n, url) in urls.into_iter().enumerate() {
        match client
            .get_preview_image(&url)
            .filter(|p| crate::appearance_actions::usable_preview(p, &preview_dir))
        {
            Some(p) => done.push((url, p.into())),
            None => rexon_core::log_warn!("axon", "cover not downloaded: {url}"),
        }
        if !done.is_empty() && (done.len() >= BATCH || n + 1 == total) {
            let batch = std::mem::take(&mut done);
            let queued = handle.queue(move |mut object| {
                {
                    let mut me = object.as_mut().rust_mut();
                    for w in me.lib.wallpapers.iter_mut() {
                        if let Some((_, p)) = batch.iter().find(|(u, _)| *u == w.cover_url.trim()) {
                            w.cover_local = Some(p.clone());
                        }
                    }
                    for (u, _) in &batch {
                        me.covers_asked.remove(u);
                    }
                }
                object.as_mut().covers_ready();
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("axon", "covers not delivered: {e:?}");
                }
                return;
            }
        }
    }
}

/// Launch Razer Axon in its own process group, without input or output:
/// closing Rexon does not kill it. It is waited for on a side thread so a
/// closed Axon does not linger as a zombie.
fn spawn_launch(plan: &ax::AxonLaunch) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let Some((program, args)) = plan.argv.split_first() else {
        return Err(rexon_core::i18n::tr("empty command"));
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    if let Some(dir) = plan.dir.as_ref().filter(|d| d.is_dir()) {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Axon library environment: the process plus the folders from the Rexon
/// settings.
fn axon_env() -> ax::AxonEnv {
    ap::live_env().0
}

/// Deliver the result of background work to the interface thread.
fn deliver(handle: cxx_qt::CxxQtThread<qobject::AxonActions>, status: String) {
    if let Err(e) = handle.queue(move |mut object| {
        object.as_mut().finished(QString::from(&status));
    }) {
        if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
            rexon_core::log_warn!("axon", "result not delivered: {e:?}");
        }
    }
}

impl qobject::AxonActions {
    fn texts(&self) -> QString {
        use rexon_core::i18n::tr;
        let pairs = |list: Vec<(&str, String)>| -> Vec<Value> {
            list.into_iter().map(|(key, label)| json!({"key": key, "label": label})).collect()
        };
        QString::from(
            &json!({
                "searchPlaceholder": aw::search_placeholder(),
                "searchIn": aw::search_in_label(),
                "modes": pairs(SearchMode::ALL.iter().map(|m| (m.key(), m.label())).collect()),
                "sorts": pairs(SortOrder::ALL.iter().map(|s| (s.key(), s.label())).collect()),
                "sortTooltip": aw::sort_tooltip(),
                "flags": pairs(FilterFlag::ALL.iter().map(|f| (f.key(), f.label())).collect()),
                "typeLabel": aw::type_label(),
                "kinds": pairs(EffectKind::ALL.iter().map(|k| (k.key(), k.label())).collect()),
                "orientationLabel": orientation_group_label(),
                "orientations": pairs(WallpaperOrientation::ALL.iter().map(|o| (o.key(), o.label())).collect()),
                "clear": aw::clear_text(),
                "clearFilter": aw::clear_filter_text(),
                "apply": aw::apply_text(),
                "applyTooltip": aw::apply_tooltip(),
                "openLocation": aw::open_location_text(),
                "launch": aw::launch_text(),
                "details": aw::details_text(),
                "tileTooltip": aw::tile_tooltip(),
                "close": tr("Close"),
                "pick": ax::pick_folder_text(),
                "pickTooltip": ax::pick_folder_tooltip(),
                "pickTitle": ax::pick_folder_title(),
                "open": ax::open_folder_text(),
                "openTooltip": ax::open_folder_tooltip(),
                "settingsTitle": aw::settings_title(),
                "settingsDescription": aw::settings_description(),
                "prepareAhead": ap::toggle_text(),
                "prepareAheadTooltip": ap::toggle_tooltip(),
                "restore": ap::restore_button_text(),
                "restoreWarning": ap::restore_warning_text(),
                "restoreConfirm": ap::restore_confirm_text(),
                "cancel": tr("Cancel"),
            })
            .to_string(),
        )
    }

    fn reload(mut self: Pin<&mut Self>) {
        let env = axon_env();
        let lib = ax::load_library(&env);
        for p in &lib.problems {
            rexon_core::log_warn!("axon", "library not read: {p}");
        }
        let launch = ax::launch_plan_for(&env, &lib);
        {
            let mut me = self.as_mut().rust_mut();
            me.lib = lib;
            me.launch = launch;
        }
        self.as_mut().resolve_covers();
    }

    /// Tile covers: what is already on disk — at once, the missing ones — by a
    /// background download into the shared preview cache. The choice is the
    /// core's (`axon_window::cover_plan`).
    fn resolve_covers(mut self: Pin<&mut Self>) {
        let mut wanted: Vec<String> = Vec::new();
        {
            let mut me = self.as_mut().rust_mut();
            let me = &mut *me;
            for w in me.lib.wallpapers.iter_mut() {
                match aw::cover_plan(w, &cached_cover) {
                    aw::CoverPlan::Ready(p) => w.cover_local = Some(p),
                    aw::CoverPlan::Download(url) => {
                        if !wanted.contains(&url) && me.covers_asked.insert(url.clone()) {
                            wanted.push(url);
                        }
                    }
                    aw::CoverPlan::Missing => {}
                }
            }
        }
        if wanted.is_empty() {
            return;
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || fetch_covers(handle, wanted));
    }

    fn view(&self) -> QString {
        let me = self.rust();
        QString::from(&aw::view_json(&me.lib, &me.query).to_string())
    }

    fn set_search(mut self: Pin<&mut Self>, text: &QString) {
        self.as_mut().rust_mut().query.text = text.to_string();
    }

    fn set_search_mode(mut self: Pin<&mut Self>, key: &QString) {
        self.as_mut().rust_mut().query.mode = SearchMode::from_key(&key.to_string());
    }

    fn set_sort(mut self: Pin<&mut Self>, key: &QString) {
        self.as_mut().rust_mut().query.sort = SortOrder::from_key(&key.to_string());
    }

    fn set_filter(mut self: Pin<&mut Self>, key: &QString, on: bool) {
        let key = key.to_string();
        let mut me = self.as_mut().rust_mut();
        if let Some(flag) = FilterFlag::from_key(&key) {
            me.query.filters.set(flag, on);
        } else if let Some(kind) = EffectKind::from_axon(&key) {
            me.query.filters.set_kind(kind, on);
        } else if let Some(o) = WallpaperOrientation::from_key(&key) {
            me.query.filters.set_orientation(o, on);
        }
    }

    fn clear_filters(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().query.filters.clear();
    }

    fn clear_filter(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().query.clear_filter();
    }

    fn details(&self, id: &QString) -> QString {
        match self.rust().lib.get(&id.to_string()) {
            Some(w) => QString::from(&aw::details_json(w).to_string()),
            None => QString::default(),
        }
    }

    fn apply(mut self: Pin<&mut Self>, id: &QString) -> QString {
        use rexon_core::{qt_settings as qs, wallpaper_exec};
        let id = id.to_string();
        let Some(w) = self.rust().lib.get(&id).cloned() else {
            return QString::from(&ax::prepare_failed_text(&id));
        };
        let env = axon_env();
        let preparing = ax::preparing_text(&ax::display_title(&w));
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let extractor = ax::default_extractor();
            // Ones prepared in advance — straight from `Resource/`, the others through
            // the cache.
            let status = match ap::prepare_for_apply(&w, Some(extractor.as_ref()), &env) {
                Ok(ready) => {
                    rexon_core::log_info!("axon", "wallpaper {} ready: {}", w.id, ready.path.display());
                    let path = ready.path.to_string_lossy().into_owned();
                    // The same core door as "Apply" of a wallpaper set: one executor per
                    // process, and it decides the output for the environment.
                    wallpaper_exec::run_in_slot(
                        &wallpaper_exec::live_slot(),
                        move |exec: &mut wallpaper_exec::WallpaperExec<wallpaper_exec::LiveSys>| {
                            let res = exec.apply_with_audio(&path, ready.format, &ready.name, ready.audio);
                            qs::wallpaper_done_warned(res.ok, &res.message, res.warning.as_deref())
                        },
                    )
                }
                Err(why) => format!("❌ {why}"),
            };
            deliver(handle, status);
        });
        QString::from(&preparing)
    }

    fn open_location(&self, id: &QString) -> bool {
        let Some(dir) = self.rust().lib.get(&id.to_string()).and_then(|w| w.resource_dir.clone()) else {
            return false;
        };
        crate::extensions_actions::qobject::open_folder_url(&QString::from(&dir.to_string_lossy().into_owned()))
    }

    fn open_folder(&self, path: &QString) -> bool {
        crate::extensions_actions::qobject::open_folder_url(path)
    }

    fn can_launch(&self) -> bool {
        self.rust().launch.is_some()
    }

    fn launch_axon(&self) -> QString {
        let Some(plan) = self.rust().launch.clone() else {
            return QString::default();
        };
        QString::from(&match spawn_launch(&plan) {
            Ok(()) => aw::launch_started_text(),
            Err(why) => {
                rexon_core::log_warn!("axon", "Razer Axon did not start: {why}");
                aw::launch_failed_text(&why)
            }
        })
    }

    fn add_folder(mut self: Pin<&mut Self>, url: &QString) -> QString {
        let raw = url.to_string();
        let path = std::path::PathBuf::from(rexon_core::local_install::file_url_to_path(&raw));
        let answer = |ok: bool, message: String| QString::from(&json!({"ok": ok, "message": message}).to_string());
        let dir = match ax::check_source_dir(&path) {
            Ok(d) => d,
            Err(why) => return answer(false, why),
        };
        let count = ax::read_source_dir(&dir).map(|l| l.len()).unwrap_or(0);
        let saved = match rexon_core::settings::SettingsManager::new() {
            Ok(mut m) => {
                let list = ax::with_source_dir(&ax::saved_source_dirs(&m.settings), &dir);
                m.set(ax::SETTINGS_KEY, list).map_err(|e| e.to_string())
            }
            Err(e) => Err(e.to_string()),
        };
        if let Err(e) = saved {
            rexon_core::log_warn!("axon", "folder {} not saved in the settings: {e}", dir.display());
        }
        // A new folder — prepare its wallpapers in advance too (when off, the
        // thread exits at once).
        ap::spawn_background(std::time::Duration::ZERO);
        self.as_mut().reload();
        answer(true, ax::folder_added_text(count))
    }

    fn settings_state(&self) -> QString {
        let (env, settings) = ap::live_env();
        let lib = ax::load_library(&env);
        let counts = ap::counts(&lib, &env.originals_root());
        let running = ap::is_running();
        let line = if counts.prepared + counts.remaining == 0 {
            String::new()
        } else {
            ap::status_text(counts, running)
        };
        let dirs: Vec<String> = lib.source_dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect();
        QString::from(
            &json!({"line": line, "running": running, "enabled": ap::enabled(&settings), "dirs": dirs}).to_string(),
        )
    }

    fn set_prepare_ahead(&self, on: bool) {
        match rexon_core::settings::SettingsManager::new() {
            Ok(mut m) => {
                if let Err(e) = m.set(ap::PREPARE_AHEAD_KEY, Value::Bool(on)) {
                    rexon_core::log_warn!("axon", "the prepare-in-advance setting not saved: {e}");
                }
            }
            Err(e) => rexon_core::log_warn!("axon", "settings did not open: {e}"),
        }
        if on {
            ap::spawn_background(std::time::Duration::ZERO);
        } else {
            ap::request_stop();
        }
    }

    fn restore_originals(mut self: Pin<&mut Self>) {
        self.set_prepare_ahead(false);
        let root = axon_env().originals_root();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let report = ap::restore_originals(&root);
            rexon_core::log_info!("axon", "restoring the original descriptions: {report:?}");
            deliver(handle, ap::restore_done_text(report));
        });
    }
}
