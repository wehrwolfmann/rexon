// AxonActions — модель источника «Razer Axon» раздела «Оформление → Обои HD»
// и его блока в «⚙️ Настройках» того же раздела.
//
// Логики здесь нет: что попадает в сетку, в каком порядке, что сказать
// человеку — ядро (`rexon_core::axon_window`); чтение библиотеки —
// `axon_library`, подготовка и наши настройки — `axon_prepare`. Объект
// держит только прочитанную библиотеку и запрос (поиск, режим, сортировка,
// фильтры), чтобы поиск по набору не перечитывал файлы Axon.
//
// Файлы Axon источник только читает (решение Wehrwolfmann 14.11.2026):
// «Применить» готовит обои в кэш Rexon или берёт подготовленные заранее и
// ставит их общим исполнителем обоев — тем же, что у наборов обоев.
//
// До 14.11.2026 те же действия жили временным окном «Razer Axon (тест)» на
// странице «Расширения» (`ExtensionsActions::axon*`).

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
        /// Фоновое дело («Применить», возврат описаний) закончилось:
        /// `message` — готовая строка итога.
        #[qsignal]
        fn finished(self: Pin<&mut AxonActions>, message: QString);

        /// Доехали обложки плиток: сетку и подробности стоит перечитать.
        #[qsignal]
        #[cxx_name = "coversReady"]
        fn covers_ready(self: Pin<&mut AxonActions>);
    }

    extern "RustQt" {
        /// Все подписи источника и блока настроек одним JSON.
        #[qinvokable]
        fn texts(self: &AxonActions) -> QString;

        /// Перечитать библиотеку Axon (заход в источник, «Обновить»).
        #[qinvokable]
        fn reload(self: Pin<&mut AxonActions>);

        /// Сетка по текущему запросу: JSON `axon_window::view_json`.
        #[qinvokable]
        fn view(self: &AxonActions) -> QString;

        /// Строка поиска.
        #[qinvokable]
        #[cxx_name = "setSearch"]
        fn set_search(self: Pin<&mut AxonActions>, text: &QString);

        /// Режим поиска: `titles` | `tags`.
        #[qinvokable]
        #[cxx_name = "setSearchMode"]
        fn set_search_mode(self: Pin<&mut AxonActions>, key: &QString);

        /// Сортировка: `name` | `date`.
        #[qinvokable]
        #[cxx_name = "setSort"]
        fn set_sort(self: Pin<&mut AxonActions>, key: &QString);

        /// Флажок панели фильтров (`axon_window::FilterFlag::key`) либо пункт
        /// «Тип» (`Dynamic` | `Static` | `Interactive`).
        #[qinvokable]
        #[cxx_name = "setFilter"]
        fn set_filter(self: Pin<&mut AxonActions>, key: &QString, on: bool);

        /// «Очистить» панели фильтров.
        #[qinvokable]
        #[cxx_name = "clearFilters"]
        fn clear_filters(self: Pin<&mut AxonActions>);

        /// Ссылка «Очистить фильтр»: фильтры и поиск.
        #[qinvokable]
        #[cxx_name = "clearFilter"]
        fn clear_filter(self: Pin<&mut AxonActions>);

        /// Подробности обоев `id`: JSON `axon_window::details_json`; пусто —
        /// таких нет.
        #[qinvokable]
        fn details(self: &AxonActions, id: &QString) -> QString;

        /// Подготовить обои `id` и поставить их на рабочий стол (на все
        /// экраны). Возвращает строку «готовлю…»; итог — сигналом `finished`.
        #[qinvokable]
        fn apply(self: Pin<&mut AxonActions>, id: &QString) -> QString;

        /// «Перейти к расположению файла»: папка обоев в файловом менеджере.
        #[qinvokable]
        #[cxx_name = "openLocation"]
        fn open_location(self: &AxonActions, id: &QString) -> bool;

        /// Открыть папку `path` в файловом менеджере.
        #[qinvokable]
        #[cxx_name = "openFolder"]
        fn open_folder(self: &AxonActions, path: &QString) -> bool;

        /// Запомнить папку с обоями Axon (`WallpaperSource`), выбранную
        /// человеком (`file://…` или путь). JSON `{ok, message}`.
        #[qinvokable]
        #[cxx_name = "addFolder"]
        fn add_folder(self: Pin<&mut AxonActions>, url: &QString) -> QString;

        /// Блок настроек: JSON `{line, running, enabled, dirs}` — строка
        /// «Подготовлено: N, осталось: M», переключатель и папки библиотеки.
        #[qinvokable]
        #[cxx_name = "settingsState"]
        fn settings_state(self: &AxonActions) -> QString;

        /// «Подготавливать обои заранее»: запомнить и, если включили, сразу
        /// запустить фоновую подготовку.
        #[qinvokable]
        #[cxx_name = "setPrepareAhead"]
        fn set_prepare_ahead(self: &AxonActions, on: bool);

        /// «Вернуть оригинальные описания»: выключает подготовку заранее и
        /// возвращает ResourceConfig.txt из резервных копий. Итог — `finished`.
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

/// Прочитанная библиотека и запрос сетки.
#[derive(Default)]
pub struct AxonActionsRust {
    lib: AxonLibrary,
    query: Query,
    /// Адреса обложек, которые сейчас качаются или не скачались в этом
    /// сеансе: повторный заход в источник их второй раз не просит.
    covers_asked: std::collections::HashSet<String>,
}

/// Готовая копия обложки по адресу — в общем кэше превью «Оформления»
/// (тот же путь, по которому её кладёт загрузка KDE Store и Wallhaven).
fn cached_cover(url: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    let p = std::path::PathBuf::from(rexon_core::appearance_page::preview_cache_path(&home, url));
    p.is_file().then_some(p)
}

/// Скачать обложки по очереди в общий кэш превью; доехавшие отдаются в
/// поток интерфейса пачками, чтобы сетка не перестраивалась на каждую.
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
            None => rexon_core::log_warn!("axon", "обложка не скачалась: {url}"),
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
                    rexon_core::log_warn!("axon", "обложки не доставлены: {e:?}");
                }
                return;
            }
        }
    }
}

/// Окружение библиотеки Axon: процесс плюс папки из настроек Rexon.
fn axon_env() -> ax::AxonEnv {
    ap::live_env().0
}

/// Доставить итог фоновой работы в поток интерфейса.
fn deliver(handle: cxx_qt::CxxQtThread<qobject::AxonActions>, status: String) {
    if let Err(e) = handle.queue(move |mut object| {
        object.as_mut().finished(QString::from(&status));
    }) {
        if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
            rexon_core::log_warn!("axon", "итог не доставлен: {e:?}");
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
                "clear": aw::clear_text(),
                "clearFilter": aw::clear_filter_text(),
                "apply": aw::apply_text(),
                "applyTooltip": aw::apply_tooltip(),
                "openLocation": aw::open_location_text(),
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
        let lib = ax::load_library(&axon_env());
        for p in &lib.problems {
            rexon_core::log_warn!("axon", "библиотека не прочитана: {p}");
        }
        self.as_mut().rust_mut().lib = lib;
        self.as_mut().resolve_covers();
    }

    /// Обложки плиток: что уже на диске — сразу, недостающие — фоновой
    /// загрузкой в общий кэш превью. Выбор — ядро (`axon_window::cover_plan`).
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
            // Подготовленные заранее — прямо из `Resource/`, прочие — через кэш.
            let status = match ap::prepare_for_apply(&w, Some(extractor.as_ref()), &env) {
                Ok(ready) => {
                    rexon_core::log_info!("axon", "обои {} готовы: {}", w.id, ready.path.display());
                    let path = ready.path.to_string_lossy().into_owned();
                    // Та же дверь ядра, что у «Применить» набора обоев: один
                    // исполнитель на процесс, вывод под окружение решает он.
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
            rexon_core::log_warn!("axon", "папка {} не сохранена в настройках: {e}", dir.display());
        }
        // Новая папка — подготовить и её обои заранее (выключено — поток
        // сразу выйдет).
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
                    rexon_core::log_warn!("axon", "настройка подготовки заранее не сохранена: {e}");
                }
            }
            Err(e) => rexon_core::log_warn!("axon", "настройки не открылись: {e}"),
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
            rexon_core::log_info!("axon", "возврат исходных описаний: {report:?}");
            deliver(handle, ap::restore_done_text(report));
        });
    }
}
