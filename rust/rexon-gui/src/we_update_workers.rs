// Фоновые задачи страницы «Разработчик» вкладки «Wallpaper Engine» — порт
// трёх `QThread` из `rexon/qt_wallpaper_engine/window.py` вместе с их
// исполнительным слоем из `rexon/qt_wallpaper_engine/engine_update.py`.
//
// Оригинал держал три наследника QThread:
//
//   1. `_UpstreamCheckWorker` — проверка обеих upstream-целей (waywallen и
//      OWE): наша версия ↔ GitHub Releases, changelog диапазона, запись
//      отчёта в стейт → сигнал `done(statuses)`;
//   2. `_EngineInstallWorker` — скачать → проверить → установить AppImage
//      движка (с `.bak`-откатом) → сигналы `progress(str)` и `done(ok, msg)`;
//   3. `_OwePluginWorker`     — скачать → проверить (→ установить) плагин-zip
//      OWE → сигналы `progress(str)` и `done(ok, msg, zip_path)`.
//
// Здесь это три задачи одного рантайма, а сигналы Qt заменены одной очередью
// событий ([`UpdateEvent`]): владелец страницы забирает их из своего потока
// ([`WeUpdateTasks::poll_event`]). Очередь, а не колбэки, — чтобы данные
// пересекали границу потоков ровно в одном месте, как это уже сделано у
// вкладки «Steam» (`steam_workers.rs`).
//
// Чего здесь НЕТ: решений. Адреса API, разбор ответов, сравнение версий,
// вердикт валидации AppImage, разбор плагин-архива, планы установки и отката,
// формат стейта и ВСЕ подписи для человека живут в `rexon_core::engine_update`
// и `rexon_core::we_window`. На Rust переехало только исполнение: сеть,
// распаковка, файловые операции и запуск подпроцессов.
//
// ⚠️ ЖИВАЯ ВЕРСИЯ ДЕМОНА. У оригинала был путь `_daemon_version_via_qt`: он
// создавал `DaemonControl` ПРЯМО В ФОНОВОМ ПОТОКЕ и уже ронял процесс
// (починено коммитом a38d22f — колбэки больше не ставятся в конструкторе).
// Здесь этого пути нет вовсе и быть не может: ни одна задача этого модуля не
// трогает Qt. Версию живого демона, если она известна, подаёт ВЛАДЕЛЕЦ
// страницы из своего потока — параметром [`WeUpdateTasks::start_check`].
// Остальные источники (родные для фонового потока) считает ядро: `--version`
// у того бинаря, который реально будет запущен, затем `busctl`, затем стейт.
//
// ⚠️ Починки оригинала (правило #16), каждая помечена `Починка:` по месту:
//   1. ГЕЙТ СОВМЕСТИМОСТИ МОРОЗИЛ ИНТЕРФЕЙС. `_start_owe_install` звал
//      `waywallen_meets_owe_min()` в GUI-потоке, а тот при незапущенном
//      демоне уходит в `busctl` (потолок 5 с) и в пробу `--version` (ещё 5 с).
//      Десять секунд неподвижного окна на нажатие кнопки. Гейт переехал
//      внутрь задачи, а отказ приезжает событием [`UpdateEvent::OweGateRefused`]
//      с текстом, собранным ядром, — вид у отказа прежний;
//   2. ОТМЕНЫ НЕ БЫЛО У ПРОВЕРКИ ОБНОВЛЕНИЙ. `_UpstreamCheckWorker` не умел
//      прерываться вовсе: `_wait_workers` ждал его до минуты
//      (`WORKER_WAIT_MS`), пока не истекут сетевые таймауты. Здесь у вкладки
//      дерево отмены, и токен рвёт уже отправленный запрос;
//   3. ДВА ВОРКЕРА ПИСАЛИ СТЕЙТ ВРАЗНОБОЙ. Проверка (`_store_report`) и
//      установка (`mark_installed`) читали-меняли-писали один файл из разных
//      потоков без всякой синхронизации: одновременная запись теряла отчёт
//      либо версию. Здесь правка стейта идёт под общим замком процесса.
//
// ⛔ Установку движка живьём НЕ гонять: это ~200 МиБ и системные пути.
// Проверки этого модуля работают на локальных файлах и временных каталогах, а
// сверка идёт с записанным эталоном живого Python
// (`rust/rexon-gui/tests/golden/engine_update_reference.json`).

use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use rexon_core::engine_update as eu;
use rexon_core::net::{self, NetIo, UreqNet};
use rexon_core::we_window as ww;
use serde_json::{Map, Value};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::steam_workers::blocking_cancellable;
use crate::we_env;

/// Сколько ждём завершения задач при закрытии вкладки. Это ПОТОЛОК, а не
/// плановое ожидание: токен уже взведён, задачи рвут запросы сами. Оригинал
/// ждал до `WORKER_WAIT_MS` (минута) И БЕЗ отмены у проверки обновлений.
pub const SHUTDOWN_GRACE_MS: u64 = 2000;

/// Рабочих потоков рантайма. Задач три, все они ждут сеть либо подпроцесс,
/// поэтому пары потоков хватает: тяжёлое уходит в блокирующий пул.
const WORKER_THREADS: usize = 2;

// ============================================================================
// События наружу
// ============================================================================

/// То, что задачи отдают владельцу страницы. Замена сигналов трёх QThread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateEvent {
    /// `_UpstreamCheckWorker.done(statuses)`. Пустой список — как в
    /// оригинале, «не удалось проверить».
    CheckDone(Vec<eu::TargetStatus>),
    /// `progress(str)` обоих устанавливающих воркеров: готовая строка
    /// состояния из ядра.
    Progress(String),
    /// `_EngineInstallWorker.done(ok, msg)`.
    EngineDone { ok: bool, message: String },
    /// `_OwePluginWorker.done(ok, msg, zip_path)`. Непустой `zip_path` —
    /// архив проверен и передан GUI-потоку под hot-install через IPC демона
    /// (QWebSocket из чужого потока трогать нельзя).
    OweDone {
        ok: bool,
        message: String,
        zip_path: String,
    },
    /// Гейт совместимости waywallen ≥ 0.2.3 не пройден: текст уже собран
    /// ядром (`we_window::owe_gate_refused_text`). Починка 1 — гейт считает
    /// задача, а не GUI-поток.
    OweGateRefused(String),
    /// Живая версия движка для подписи «Версия waywallen: …».
    ///
    /// ⚠️ Починка 4 (правило #16): у оригинала подписи версии фонового пути
    /// не было вовсе — `_refresh_version` звал `current_version()` ПРЯМО В
    /// GUI-ПОТОКЕ, а тот уходит в `--version` бинаря (потолок 5 с) и в
    /// `busctl` (ещё 5 с). Страница открывалась и замирала на эти секунды
    /// при каждом заходе и после каждой установки. Здесь версию считает
    /// задача, а подпись приезжает событием.
    EngineVersion(String),
}

// ============================================================================
// Окружение
// ============================================================================

/// Пути, от которых зависят стейт, каталог плагинов и управляемый AppImage.
/// Снимаются один раз при постройке: читать `$HOME` из рабочей задачи незачем,
/// а проверки подставляют своё.
#[derive(Debug, Clone, Default)]
pub struct UpdateEnv {
    pub home: String,
    pub xdg_config_home: Option<String>,
    pub xdg_data_home: Option<String>,
    /// Каталог движка из пакета (`we_client::ENGINE_DIR`); проверки
    /// подставляют свою песочницу.
    pub engine_dir: String,
    /// Демон движка из пакета дистрибутива (`we_client::SYSTEM_DAEMON`).
    pub system_daemon: String,
    /// Каталог данных движка пакетов дистрибутива
    /// (`we_client::SYSTEM_ENGINE_DIR`, в нём `plugins/`).
    pub system_engine_dir: String,
}

impl UpdateEnv {
    /// Живое окружение процесса.
    pub fn from_process() -> UpdateEnv {
        UpdateEnv {
            home: we_env::home(),
            xdg_config_home: we_env::xdg_config_home(),
            xdg_data_home: we_env::xdg_data_home(),
            engine_dir: rexon_core::we_client::ENGINE_DIR.to_string(),
            system_daemon: rexon_core::we_client::SYSTEM_DAEMON.to_string(),
            system_engine_dir: rexon_core::we_client::SYSTEM_ENGINE_DIR.to_string(),
        }
    }

    /// Отказ ставить вторую копию цели `key`, которую поставил пакетный
    /// менеджер (`engine_update::system_update_refusal`); `None` — можно.
    pub fn system_refusal(&self, key: &str) -> Option<String> {
        let system_engine = std::path::Path::new(&self.system_daemon).exists();
        let owe = eu::owe_manifest_path(&rexon_core::we_client::engine_plugins_path(&self.system_engine_dir));
        let system_owe = std::path::Path::new(&owe).is_file();
        eu::system_update_refusal(key, system_engine, system_owe)
    }

    /// Файл стейта dev-обновлений.
    pub fn state_path(&self) -> String {
        eu::state_path_xdg(self.xdg_config_home.as_deref(), &self.home)
    }

    /// Пользовательский каталог плагинов waywallen.
    pub fn plugins_dir(&self) -> String {
        eu::plugins_dir(self.xdg_data_home.as_deref(), &self.home)
    }

    /// Управляемый AppImage движка.
    pub fn managed_appimage(&self) -> String {
        eu::managed_appimage(&self.home)
    }
}

// ============================================================================
// Рантайм страницы
// ============================================================================

/// Все фоновые задачи страницы «Разработчик» и их отмена.
pub struct WeUpdateTasks {
    /// ⚠️ `Option`, а не просто `Runtime`: обычный `Drop` рантайма tokio
    /// ждёт блокирующие задачи БЕЗ СРОКА, и одна застрявшая не даёт
    /// процессу выйти вовсе. Разбор — у близнеца этой страницы
    /// (`steam_workers::SteamTasks`), правило #12: порядок завершения у
    /// обоих рантаймов один.
    rt: Option<Runtime>,
    /// Корень дерева отмены: закрытие вкладки гасит ВСЁ.
    root: CancellationToken,
    /// Учёт живых задач — чтобы закрытие дождалось каждой.
    tracker: TaskTracker,
    tx: UnboundedSender<UpdateEvent>,
    rx: Mutex<UnboundedReceiver<UpdateEvent>>,
    /// Идёт ли проверка (оригинальный `_check_worker.isRunning()`).
    checking: std::sync::Arc<AtomicBool>,
    /// Идёт ли установка движка (`_install_worker.isRunning()`).
    installing: std::sync::Arc<AtomicBool>,
    /// Идёт ли работа с плагином (`_owe_worker.isRunning()`).
    owe_busy: std::sync::Arc<AtomicBool>,
    env: UpdateEnv,
}

impl WeUpdateTasks {
    /// Поднять рантайм страницы. `None` — рантайм не создался (в живом Rexon
    /// это значит «фоновых задач не будет», а не падение).
    pub fn new(env: UpdateEnv) -> Option<WeUpdateTasks> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(WORKER_THREADS)
            .thread_name("rexon-we-update")
            // ⚠️ enable_io обязателен, а не «на всякий случай»: без него
            // `tokio::process` ПАНИКУЕТ в рабочем потоке при первом же
            // подпроцессе («A Tokio 1.x context was found, but IO is
            // disabled»), а подпроцессов здесь три — `--version` бинаря,
            // busctl и проба AppImage. Поймано живым прогоном.
            .enable_io()
            .enable_time()
            .build()
            .ok()?;
        let (tx, rx) = unbounded_channel();
        let root = CancellationToken::new();
        // ⭐ Общая дверь выхода: без этой записи сток фоновых задач не видит
        // рабочих потоков рантайма и отвечает «не осталось никого» при
        // работающей проверке обновлений. То же у соседа —
        // `steam_workers::SteamTasks::new`.
        {
            let root = root.clone();
            rexon_core::exit_guard::register_stopper(move || root.cancel());
        }
        Some(WeUpdateTasks {
            rt: Some(rt),
            root,
            tracker: TaskTracker::new(),
            tx,
            rx: Mutex::new(rx),
            checking: std::sync::Arc::new(AtomicBool::new(false)),
            installing: std::sync::Arc::new(AtomicBool::new(false)),
            owe_busy: std::sync::Arc::new(AtomicBool::new(false)),
            env,
        })
    }

    /// Забрать очередное событие; `None` — очередь пуста. Зовётся из потока
    /// интерфейса (таймером страницы), никогда не блокирует.
    pub fn poll_event(&self) -> Option<UpdateEvent> {
        self.rx.lock().ok()?.try_recv().ok()
    }

    /// Окружение, с которым работают задачи (нужно владельцу для подписей
    /// путей и для проверок).
    pub fn env(&self) -> &UpdateEnv {
        &self.env
    }

    pub fn is_checking(&self) -> bool {
        self.checking.load(Ordering::SeqCst)
    }

    pub fn is_installing(&self) -> bool {
        self.installing.load(Ordering::SeqCst)
    }

    pub fn is_owe_running(&self) -> bool {
        self.owe_busy.load(Ordering::SeqCst)
    }

    // ── Задача 1: проверка обновлений ──────────────────────────────────

    /// Порт `_UpstreamCheckWorker`. `false` — проверка уже идёт (оригинальный
    /// ранний выход из `_start_upstream_check`).
    ///
    /// `daemon_version` — свойство `Version` живого демона, если владелец его
    /// знает; «» — источник пропускается. Спрашивать демона ОТСЮДА нельзя:
    /// это Qt, а мы в чужом потоке (см. шапку модуля).
    pub fn start_check(&self, daemon_version: String) -> bool {
        if self.checking.swap(true, Ordering::SeqCst) {
            return false;
        }
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let env = self.env.clone();
        let busy = self.checking.clone();
        self.spawn(async move {
            let statuses = check_all_targets(&env, &token, &daemon_version).await;
            busy.store(false, Ordering::SeqCst);
            if !token.is_cancelled() {
                let _ = tx.send(UpdateEvent::CheckDone(statuses));
            }
        });
        true
    }

    /// Обновить подпись версии движка в фоне (починка 4).
    ///
    /// Отдельной задачи в оригинале не было: он спрашивал версию прямо в
    /// потоке интерфейса. Своего флага занятости у неё нет — задача
    /// короткая, идемпотентная и ничего не пишет.
    pub fn start_version_refresh(&self, daemon_version: String) {
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let env = self.env.clone();
        self.spawn(async move {
            let version = engine_current_version(&env, &daemon_version).await;
            if !token.is_cancelled() {
                let _ = tx.send(UpdateEvent::EngineVersion(version));
            }
        });
    }

    // ── Задача 2: установка AppImage движка ────────────────────────────

    /// Порт `_EngineInstallWorker`. `false` — установка уже идёт.
    pub fn start_engine_install(&self, asset_url: String, latest_tag: String) -> bool {
        if self.installing.swap(true, Ordering::SeqCst) {
            return false;
        }
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let env = self.env.clone();
        let busy = self.installing.clone();
        self.spawn(async move {
            // Движок стоит пакетом дистрибутива — вторую копию не качаем.
            if let Some(message) = env.system_refusal("waywallen") {
                busy.store(false, Ordering::SeqCst);
                if !token.is_cancelled() {
                    let _ = tx.send(UpdateEvent::EngineDone { ok: false, message });
                }
                return;
            }
            let out = engine_install(&env, &token, &tx, &asset_url, &latest_tag).await;
            busy.store(false, Ordering::SeqCst);
            if !token.is_cancelled() {
                let _ = tx.send(UpdateEvent::EngineDone {
                    ok: out.ok,
                    message: out.message,
                });
            }
        });
        true
    }

    // ── Задача 3: OWE-плагин ───────────────────────────────────────────

    /// Порт `_OwePluginWorker` вместе с гейтом совместимости (починка 1).
    ///
    /// `fs_install` решает владелец ДО старта: `we_window::plugin_fs_install`
    /// от «подключён ли демон». `false` — работа с плагином уже идёт.
    pub fn start_owe_install(
        &self,
        asset_url: String,
        latest_tag: String,
        fs_install: bool,
        daemon_version: String,
    ) -> bool {
        if self.owe_busy.swap(true, Ordering::SeqCst) {
            return false;
        }
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let env = self.env.clone();
        let busy = self.owe_busy.clone();
        self.spawn(async move {
            // Плагин стоит пакетом дистрибутива — вторую копию не качаем.
            // Отказ идёт тем же событием, что и гейт: кнопка возвращается.
            if let Some(message) = env.system_refusal("owe") {
                busy.store(false, Ordering::SeqCst);
                if !token.is_cancelled() {
                    let _ = tx.send(UpdateEvent::OweGateRefused(message));
                }
                return;
            }
            // Починка 1: гейт «waywallen ≥ 0.2.3» считается здесь, а не в
            // потоке интерфейса — он уходит в busctl и в пробу `--version`.
            let version = engine_current_version(&env, &daemon_version).await;
            let (allowed, why) = eu::waywallen_meets_owe_min(&version);
            if !allowed {
                busy.store(false, Ordering::SeqCst);
                if !token.is_cancelled() {
                    let _ = tx.send(UpdateEvent::OweGateRefused(ww::owe_gate_refused_text(&why)));
                }
                return;
            }
            let out = owe_install(&env, &token, &tx, &asset_url, &latest_tag, fs_install).await;
            busy.store(false, Ordering::SeqCst);
            if !token.is_cancelled() {
                let _ = tx.send(UpdateEvent::OweDone {
                    ok: out.ok,
                    message: out.message,
                    zip_path: out.zip_path,
                });
            }
        });
        true
    }

    // ── Услуги потока интерфейса ───────────────────────────────────────

    /// Порт `engine_update.mark_seen` — «прочитал changelog, ставить не буду».
    /// Версия пишется тегом, как в `_on_mark_owe_seen`.
    pub fn mark_seen(&self, key: &str, version: &str) {
        let now = now_iso();
        edit_state(&self.env.state_path(), |st| {
            eu::mark_seen(st, key, version, &now);
        });
    }

    /// Порт `mark_installed(key, version_tag_to_state(tag))` — так его зовёт
    /// `_on_owe_ipc_resp` после успешной установки плагина демоном.
    pub fn mark_installed_tag(&self, key: &str, tag: &str) {
        let version = eu::version_tag_to_state(tag);
        let now = now_iso();
        edit_state(&self.env.state_path(), |st| {
            eu::mark_installed(st, key, &version, &now);
        });
    }

    /// ФС-фолбэк установки плагина ПРЯМО В ПОТОКЕ ИНТЕРФЕЙСА — порт ветки
    /// `_on_owe_worker_done`, где демон умер, пока качали архив. Сети здесь
    /// нет, только распаковка уже скачанного файла, поэтому оригинал делал
    /// это на месте — и мы делаем так же.
    pub fn install_plugin_zip_now(&self, zip_path: &str, tag: &str) -> (bool, String) {
        install_plugin_zip_fs(&self.env, zip_path, tag, "")
    }

    // ── Закрытие ───────────────────────────────────────────────────────

    /// Погасить ВСЕ задачи и дождаться их — порт `_wait_workers`.
    ///
    /// Оригинал ждал до минуты, потому что отмены у проверки обновлений не
    /// было вовсе (починка 2). Здесь токен взводится первым, задачи рвут свои
    /// запросы сами, а ожидание — страховка с потолком [`SHUTDOWN_GRACE_MS`].
    pub fn shutdown(&self) {
        self.root.cancel();
        self.tracker.close();
        let Some(rt) = self.rt.as_ref() else {
            return;
        };
        let tracker = self.tracker.clone();
        rt.block_on(async move {
            let _ = tokio::time::timeout(
                std::time::Duration::from_millis(SHUTDOWN_GRACE_MS),
                tracker.wait(),
            )
            .await;
        });
    }

    /// Завести задачу под учётом трекера.
    fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        if self.root.is_cancelled() {
            return;
        }
        let Some(rt) = self.rt.as_ref() else {
            return;
        };
        let _guard = rt.enter();
        self.tracker.spawn(fut);
    }
}

impl Drop for WeUpdateTasks {
    /// Потолок на роспуск рантайма — тот же порядок, что у
    /// `steam_workers::SteamTasks` (правило #12): человек, закрывший окно,
    /// обязан получить закрытое окно, чем бы ни была занята фоновая задача.
    fn drop(&mut self) {
        self.root.cancel();
        self.tracker.close();
        if let Some(rt) = self.rt.take() {
            rt.shutdown_timeout(std::time::Duration::from_millis(SHUTDOWN_GRACE_MS));
        }
    }
}

// ============================================================================
// Исходы задач
// ============================================================================

/// Итог задачи установки — порт `WorkerOutcome` ядра, доведённый до события.
struct TaskOutcome {
    ok: bool,
    message: String,
    /// Непустой — архив передан GUI-потоку под IPC-установку.
    zip_path: String,
}

// ============================================================================
// Задача 1: проверка обеих целей
// ============================================================================

/// Порт `check_all_targets` — обе цели подряд, как в оригинале.
async fn check_all_targets(
    env: &UpdateEnv,
    token: &CancellationToken,
    daemon_version: &str,
) -> Vec<eu::TargetStatus> {
    // Живая версия движка нужна только цели `waywallen`, и спрашиваем её
    // ОДИН раз: оригинал звал `current_version()` внутри цикла по целям.
    let engine_version = engine_current_version(env, daemon_version).await;
    let state_path = env.state_path();
    let mut out = Vec::new();
    for target in eu::targets(&env.home) {
        if token.is_cancelled() {
            break;
        }
        let state = eu::load_state(read_state_text(&state_path).as_deref());
        let installed = eu::installed_version(
            &target.key,
            if target.key == "waywallen" {
                &engine_version
            } else {
                ""
            },
            &owe_installed_version(env),
            &eu::state_field(&state, &target.key, "installed_version"),
            &eu::state_field(&state, &target.key, "seen_version"),
        );
        let since = eu::state_field(&state, &target.key, "last_check");
        let repo = target.repo.clone();
        let fetch = blocking_cancellable(token, move |cancel| fetch_outcome(&repo, &since, &cancel))
            .await;
        let Some(fetch) = fetch else { break };
        let st = eu::check_target(&target, &installed, &fetch);
        if st.ok {
            let now = now_iso();
            let key = target.key.clone();
            let snapshot = st.clone();
            edit_state(&state_path, |state| {
                eu::store_report(state, &key, &snapshot, &now);
            });
        }
        out.push(st);
    }
    out
}

/// Поход в GitHub API за релизами, с фолбэком на коммиты default-ветки —
/// порт сетевой части `check_target`.
fn fetch_outcome(repo: &str, since_iso: &str, cancel: &net::CancelToken) -> eu::FetchOutcome {
    match api_json(&eu::releases_url(repo), cancel) {
        Err(e) => eu::FetchOutcome::ReleasesFailed(e),
        Ok(body) => match eu::parse_releases(&body) {
            Err(e) => eu::FetchOutcome::ReleasesFailed(e),
            Ok(rels) if !rels.is_empty() => eu::FetchOutcome::Releases(rels),
            // У репо нет релизов — фолбэк на коммиты default-ветки.
            Ok(_) => match api_json(&eu::commits_url(repo, since_iso), cancel) {
                Err(e) => eu::FetchOutcome::CommitsFailed(e),
                Ok(body) => match eu::parse_commits(&body) {
                    Err(e) => eu::FetchOutcome::CommitsFailed(e),
                    Ok(commits) => eu::FetchOutcome::Commits(commits),
                },
            },
        },
    }
}

/// `GET` к API GitHub — порт `_api_json`. Тело как есть, разбирает ядро.
fn api_json(url: &str, cancel: &net::CancelToken) -> Result<String, String> {
    let mut net = api_net(cancel.clone());
    let body = net
        .get(url, eu::API_USER_AGENT, eu::API_TIMEOUT_SECS)
        .map_err(|e| e.human())?;
    String::from_utf8(body).map_err(|e| e.to_string())
}

/// Транспорт для API GitHub: заголовки ровно те, что назвало ядро
/// (`api_headers` — `Accept` и `User-Agent`).
fn api_net(cancel: net::CancelToken) -> UreqNet {
    let mut extra = eu::api_headers();
    // `User-Agent` транспорт ставит сам отдельным аргументом каждого запроса —
    // здесь остаётся только то, чего он не знает.
    extra.retain(|(name, _)| !name.eq_ignore_ascii_case("user-agent"));
    UreqNet::new().with_cancel(cancel).with_headers(extra)
}

// ============================================================================
// Наша текущая версия
// ============================================================================

/// Версия движка waywallen — порт `current_version` со всеми источниками.
///
/// Порядок и вердикт — целиком ядро (`engine_update::current_version`): здесь
/// только добыча наблюдений. `daemon_version` приходит от владельца страницы,
/// потому что спрашивать демона по D-Bus умеет только Qt-слой.
async fn engine_current_version(env: &UpdateEnv, daemon_version: &str) -> String {
    let binary = binary_version().await;
    let busctl = run_capture(
        &eu::busctl_version_argv(),
        eu::BUSCTL_TIMEOUT_SECS,
    )
    .await
    .map(|(stdout, _)| stdout);
    let state = eu::load_state(read_state_text(&env.state_path()).as_deref());
    eu::current_version(
        Some(&binary),
        Some(daemon_version),
        busctl.as_deref(),
        &eu::state_field(&state, "waywallen", "installed_version"),
    )
}

/// `waywallen --version` у ТОГО бинаря, который реально будет запущен.
/// Разные сборки печатают версию то в stdout, то в stderr — смотрим оба.
async fn binary_version() -> String {
    let Some(bin) = we_env::daemon_binary() else {
        return String::new();
    };
    let Some((stdout, stderr)) = run_capture(
        &eu::binary_version_argv(&bin),
        eu::BINARY_VERSION_TIMEOUT_SECS,
    )
    .await
    else {
        return String::new();
    };
    let v = eu::parse_binary_version(&stdout);
    if v.is_empty() {
        eu::parse_binary_version(&stderr)
    } else {
        v
    }
}

/// Версия OWE-плагина, которую РЕАЛЬНО загрузит демон — правда с ФС
/// (`plugin.toml`), порт `owe_installed_version`.
///
/// С 28.09.2026 копий может быть две: у человека
/// (`$XDG_DATA_HOME/waywallen/plugins`) и в пакете Rexon
/// (`/usr/lib/rexon/engine/plugins`). Демон берёт новую из двух (при равенстве
/// — копию человека); версию считаем тем же правилом
/// (`we_client::active_plugin_source`), иначе страница «Разработчик»
/// предлагала бы обновить плагин, который уже стоит в пакете.
pub fn owe_installed_version(env: &UpdateEnv) -> String {
    let read = |plugins: &str| {
        std::fs::read_to_string(eu::owe_manifest_path(plugins))
            .map(|text| eu::owe_version_from_manifest(&text))
            .unwrap_or_default()
    };
    let user = read(&env.plugins_dir());
    // Копия пакета: из пакета дистрибутива, а нет её — вшитая в Rexon
    // (порядок `we_client::engine_layouts`).
    let bundled = Some(read(&rexon_core::we_client::engine_plugins_path(&env.system_engine_dir)))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| read(&rexon_core::we_client::engine_plugins_path(&env.engine_dir)));
    match (bundled.is_empty(), user.is_empty()) {
        (true, _) => user,
        (false, true) => bundled,
        (false, false) => match rexon_core::we_client::active_plugin_source(&bundled, &user) {
            rexon_core::we_client::PluginSource::Bundled => bundled,
            rexon_core::we_client::PluginSource::User => user,
        },
    }
}

// ============================================================================
// Задача 2: скачать → проверить → установить AppImage движка
// ============================================================================

async fn engine_install(
    env: &UpdateEnv,
    token: &CancellationToken,
    tx: &UnboundedSender<UpdateEvent>,
    url: &str,
    tag: &str,
) -> TaskOutcome {
    let prefix = ww::PREFIX_ENGINE;
    let _ = tx.send(UpdateEvent::Progress(ww::download_start_text(prefix)));
    let tmp = download_step(token, tx, url, eu::DownloadKind::AppImage, prefix).await;
    if let Some(out) = ww::after_download(prefix, tmp.is_some(), token.is_cancelled(), None) {
        if out.remove_tmp {
            drop_tmp(tmp.as_deref().unwrap_or_default());
        }
        return done(out);
    }
    let tmp = tmp.unwrap_or_default();

    let _ = tx.send(UpdateEvent::Progress(ww::text_validate_appimage().to_string()));
    let (ok, msg) = validate_appimage(&tmp).await;
    if let Some(out) = ww::after_validate(false, ok, &msg) {
        if out.remove_tmp {
            drop_tmp(&tmp);
        }
        return done(out);
    }

    // Остановить движок ДО подмены (грабля 27.08.2026: подмена под живым
    // демоном — несовпадение версий и SIGBUS при остановке). Не вышло
    // остановить — файл не трогаем вовсе.
    let _ = tx.send(UpdateEvent::Progress(rexon_core::i18n::tr(
        "Stopping the wallpaper engine…",
    )));
    let swap = match stop_engine_for_swap().await {
        Ok(plan) => plan,
        Err(message) => {
            drop_tmp(&tmp);
            return done(ww::after_install_appimage(false, &message));
        }
    };

    let env2 = env.clone();
    let src = tmp.clone();
    let version = tag.to_string();
    let (ok, imsg) = tokio::task::spawn_blocking(move || install_appimage(&env2, &src, &version))
        .await
        .unwrap_or_else(|e| (false, eu::appimage_install_fail_message(&e.to_string())));
    // Поднять службу в ЛЮБОМ исходе: при откате вернулась прежняя версия, и
    // без обоев человек остаться не должен.
    let restarted = match &swap.start_argv {
        Some(argv) => run_status(argv, rexon_core::we_client::SYSTEMCTL_TIMEOUT_SECS).await == Some(0),
        None => false,
    };
    let imsg = match (ok, restarted, swap.quit_daemon) {
        (true, true, _) => eu::appimage_install_restarted_message(&env.managed_appimage()),
        (true, false, true) => eu::appimage_install_quit_message(&env.managed_appimage()),
        _ => imsg,
    };
    let out = ww::after_install_appimage(ok, &imsg);
    if out.remove_tmp {
        drop_tmp(&tmp);
    }
    done(out)
}

// ============================================================================
// Задача 3: скачать → проверить (→ установить) плагин-zip OWE
// ============================================================================

async fn owe_install(
    env: &UpdateEnv,
    token: &CancellationToken,
    tx: &UnboundedSender<UpdateEvent>,
    url: &str,
    tag: &str,
    fs_install: bool,
) -> TaskOutcome {
    let prefix_text = ww::prefix_owe();
    let prefix = prefix_text.as_str();
    let _ = tx.send(UpdateEvent::Progress(ww::download_start_text(prefix)));
    let tmp = download_step(token, tx, url, eu::DownloadKind::PluginZip, prefix).await;
    if let Some(out) = ww::after_download(prefix, tmp.is_some(), token.is_cancelled(), None) {
        if out.remove_tmp {
            drop_tmp(tmp.as_deref().unwrap_or_default());
        }
        return done(out);
    }
    let tmp = tmp.unwrap_or_default();

    let _ = tx.send(UpdateEvent::Progress(ww::text_validate_plugin().to_string()));
    let path = tmp.clone();
    let validation = tokio::task::spawn_blocking(move || eu::validate_plugin_zip(Path::new(&path)))
        .await
        .unwrap_or_default();
    if let Some(out) = ww::after_validate(true, validation.ok, &validation.msg) {
        if out.remove_tmp {
            drop_tmp(&tmp);
        }
        return done(out);
    }

    let out = ww::after_validate_plugin_route(fs_install, &validation.msg);
    if out.hand_over_tmp {
        // Установку сделает GUI-поток через IPC демона: QWebSocket из чужого
        // потока трогать нельзя, воркер только доносит путь.
        return TaskOutcome {
            ok: out.ok,
            message: out.message,
            zip_path: tmp,
        };
    }

    let _ = tx.send(UpdateEvent::Progress(
        ww::text_install_plugin_fs().to_string(),
    ));
    let env2 = env.clone();
    let src = tmp.clone();
    let version = tag.to_string();
    let (ok, imsg) =
        tokio::task::spawn_blocking(move || install_plugin_zip_fs(&env2, &src, &version, ""))
            .await
            .unwrap_or_else(|e| (false, eu::plugin_install_fail_message(&e.to_string())));
    let out = ww::after_install_plugin_fs(ok, &imsg);
    if out.remove_tmp {
        drop_tmp(&tmp);
    }
    done(out)
}

/// Вердикт ядра → исход задачи (архив никому не передан).
fn done(out: ww::WorkerOutcome) -> TaskOutcome {
    TaskOutcome {
        ok: out.ok,
        message: out.message,
        zip_path: String::new(),
    }
}

// ============================================================================
// Скачивание
// ============================================================================

/// Шаг скачивания с сообщениями прогресса — общий для обеих задач.
async fn download_step(
    token: &CancellationToken,
    tx: &UnboundedSender<UpdateEvent>,
    url: &str,
    kind: eu::DownloadKind,
    prefix: &str,
) -> Option<String> {
    let spec = eu::download_spec(kind);
    let url = url.to_string();
    let prefix = prefix.to_string();
    let tx = tx.clone();
    blocking_cancellable(token, move |cancel| {
        download_asset(&url, &spec, &cancel, |done_b, total_b| {
            if let Some(text) = ww::download_progress(&prefix, done_b, total_b) {
                let _ = tx.send(UpdateEvent::Progress(text));
            }
        })
    })
    .await
    .flatten()
}

/// Порт `download_asset`/`download_plugin_zip`: release-ассет во временный
/// файл. Путь или `None`. Локальный путь (для проверок) тоже принимается —
/// как в оригинале.
fn download_asset(
    url: &str,
    spec: &eu::DownloadSpec,
    cancel: &net::CancelToken,
    on_progress: impl FnMut(u64, u64),
) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    let tmp = mktemp_file(&spec.prefix, &spec.suffix)?;
    let fetched = if Path::new(url).is_file() {
        std::fs::copy(url, &tmp).map(|_| ()).map_err(|_| ())
    } else {
        stream_to_file(url, &tmp, spec, cancel, on_progress)
    };
    if fetched.is_err()
        || std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(spec.mode)).is_err()
    {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    Some(tmp)
}

/// Тело ответа в файл кусками по `spec.chunk` с кооперативной отменой между
/// ними — порт цикла `while True: chunk = r.read(...)`.
fn stream_to_file(
    url: &str,
    dest: &str,
    spec: &eu::DownloadSpec,
    cancel: &net::CancelToken,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), ()> {
    let mut net = UreqNet::new().with_cancel(cancel.clone());
    let total = net
        .open(url, eu::API_USER_AGENT, spec.timeout_secs)
        .map_err(|_| ())?
        .max(0) as u64;
    let mut file = std::fs::File::create(dest).map_err(|_| ())?;
    let mut got: u64 = 0;
    loop {
        if cancel.is_cancelled() {
            return Err(());
        }
        let chunk = net.read(spec.chunk as usize).map_err(|_| ())?;
        if chunk.is_empty() {
            break;
        }
        file.write_all(&chunk).map_err(|_| ())?;
        got += chunk.len() as u64;
        on_progress(got, total);
    }
    net.close();
    file.flush().map_err(|_| ())
}

// ============================================================================
// Валидация и установка AppImage
// ============================================================================

/// Порт `validate_appimage`: наблюдения снимаем здесь, вердикт выносит ядро.
async fn validate_appimage(path: &str) -> (bool, String) {
    let meta = std::fs::metadata(path);
    let is_file = meta.as_ref().map(|m| m.is_file()).unwrap_or(false);
    let is_exec = is_file
        && meta
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    if !(is_file && is_exec) {
        return eu::validate_appimage_verdict(is_file, is_exec, None, "");
    }
    match run_capture(&eu::appimage_probe_argv(path), eu::PROBE_TIMEOUT_SECS).await {
        Some((stdout, stderr)) => {
            eu::validate_appimage_verdict(true, true, None, &format!("{stdout}{stderr}"))
        }
        // Не запустился либо не уложился в потолок — для ядра это тот же
        // «spawn_error», что и исключение `subprocess.run` в оригинале.
        None => eu::validate_appimage_verdict(true, true, Some(&probe_failed()), ""),
    }
}

/// Пояснение к несостоявшемуся запуску пробы: ядро вставляет его в свою
/// фразу «не запускается: …», которую видит человек на странице
/// «Разработчик», — поэтому и оно из каталога перевода.
fn probe_failed() -> String {
    rexon_core::i18n::tr("the process did not start or did not respond in time")
}

/// Порт `install_appimage`: атомарная подмена управляемого AppImage с
/// `.bak`-откатом. План (пути, права, версия для стейта) строит ядро.
fn install_appimage(env: &UpdateEnv, new_path: &str, tag: &str) -> (bool, String) {
    let plan = eu::appimage_install_plan_for_dest(&env.managed_appimage(), tag);
    let _ = std::fs::create_dir_all(&plan.dest_dir);
    match place_appimage(&plan, new_path) {
        Ok(()) => {
            let _ = std::fs::remove_file(new_path);
            if !plan.state_version.is_empty() {
                let now = now_iso();
                edit_state(&env.state_path(), |st| {
                    eu::mark_installed(st, "waywallen", &plan.state_version, &now);
                });
            }
            (true, eu::appimage_install_ok_message(&plan.dest))
        }
        Err(e) => {
            // Недоделанный `.new-update` не остаётся мусором в каталоге движка.
            let _ = std::fs::remove_file(&plan.tmp);
            if Path::new(&plan.bak).is_file() {
                let _ = std::fs::rename(&plan.bak, &plan.dest);
            }
            (false, eu::appimage_install_fail_message(&e))
        }
    }
}

/// Файловые операции подмены: копия прежней версии в `.bak`, новая — через
/// временное имя В ТОЙ ЖЕ папке и `rename` (писать поверх работающего бинаря
/// нельзя — «Text file busy»).
fn place_appimage(plan: &eu::AppimageInstallPlan, new_path: &str) -> Result<(), String> {
    if Path::new(&plan.dest).is_file() {
        std::fs::copy(&plan.dest, &plan.bak).map_err(|e| e.to_string())?;
    }
    std::fs::copy(new_path, &plan.tmp).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&plan.tmp, std::fs::Permissions::from_mode(plan.mode))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&plan.tmp, &plan.dest).map_err(|e| e.to_string())
}

// ============================================================================
// Установка плагин-zip в файловую систему
// ============================================================================

/// Порт `install_plugin_zip_fs`: staging → атомарный перенос в
/// `plugins_dir()/<id>`, прежняя версия в `.bak-update`, откат по плану ядра.
///
/// `plugins_root` — переопределение каталога («» = штатный).
pub fn install_plugin_zip_fs(
    env: &UpdateEnv,
    zip_path: &str,
    tag: &str,
    plugins_root: &str,
) -> (bool, String) {
    let validation = eu::validate_plugin_zip(Path::new(zip_path));
    let plan = eu::plugin_install_plan(&validation, &env.plugins_dir(), plugins_root, tag);
    if !plan.ok {
        return (false, plan.message);
    }
    let Some(staging) = mktemp_dir(&plan.staging_prefix) else {
        return (
            false,
            eu::plugin_install_fail_message(&rexon_core::i18n::tr("could not create the staging directory")),
        );
    };
    let extract_root = format!("{staging}/{}", plan.extract_subdir);
    let mut moved_to_bak = false;
    let mut target_created = false;
    let outcome = unpack_into_place(
        zip_path,
        &plan,
        &extract_root,
        &mut moved_to_bak,
        &mut target_created,
    );
    rmtree_quiet(&staging);
    match outcome {
        Ok(()) => {
            if !plan.state_version.is_empty() {
                let now = now_iso();
                edit_state(&env.state_path(), |st| {
                    eu::mark_installed(st, "owe", &plan.state_version, &now);
                });
            }
            (
                true,
                eu::plugin_install_ok_message(&plan.info.id, &plan.target),
            )
        }
        Err(e) => {
            let roll = eu::plugin_rollback_plan(moved_to_bak, target_created);
            if roll.remove_target {
                rmtree_quiet(&plan.target);
            }
            if roll.restore_bak && Path::new(&plan.bak).exists() {
                let _ = std::fs::rename(&plan.bak, &plan.target);
            }
            (false, eu::plugin_install_fail_message(&e))
        }
    }
}

/// Распаковка и перенос на место. Флаги отката выставляются ровно там, где
/// это делал оригинал: до переноса прежней версии в `.bak` каталог плагина
/// ещё НЕ наш, и при сбое трогать его нельзя.
fn unpack_into_place(
    zip_path: &str,
    plan: &eu::PluginInstallPlan,
    extract_root: &str,
    moved_to_bak: &mut bool,
    target_created: &mut bool,
) -> Result<(), String> {
    std::fs::create_dir_all(&plan.root).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(extract_root).map_err(|e| e.to_string())?;
    extract_zip(zip_path, extract_root)?;
    if Path::new(&plan.bak).exists() {
        rmtree_quiet(&plan.bak);
    }
    if Path::new(&plan.target).exists() {
        std::fs::rename(&plan.target, &plan.bak).map_err(|e| e.to_string())?;
        *moved_to_bak = true;
    }
    // С этого момента target — НАШ: его можно сносить при откате.
    *target_created = true;
    if std::fs::rename(extract_root, &plan.target).is_err() {
        // Стейджинг на другой файловой системе (обычно так и есть: временный
        // каталог в /tmp, плагины в $HOME) — копируем дерево.
        copy_tree(Path::new(extract_root), Path::new(&plan.target))?;
    }
    Ok(())
}

/// Распаковать архив, восстановив unix-права записей.
///
/// Права нужны бинарям рендереров (демон делает то же в `apply_zip_mode`), а
/// маску задаёт ядро (`zip_entry_mode`). ⚠️ У архивов, собранных не на unix,
/// крейт `zip` домысливает права там, где `ZipFile` оригинала видел ноль;
/// плагины waywallen собираются на Linux, где значение то же самое.
fn extract_zip(zip_path: &str, root: &str) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(rexon_core::i18n::trf("unsafe path in the archive: %1", &[&entry.name()]));
        };
        let out = Path::new(root).join(rel);
        let mode = entry
            .unix_mode()
            .map(|m| eu::zip_entry_mode(m << 16))
            .unwrap_or(0);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let mut dest = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            // Чтение до конца заодно сверяет CRC записи: битый архив падает
            // здесь, ДО того как тронута установленная версия.
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            dest.write_all(&buf).map_err(|e| e.to_string())?;
        }
        if mode != 0 {
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode));
        }
    }
    Ok(())
}

/// Рекурсивная копия дерева с правами — замена `shutil.copytree` для случая
/// «стейджинг на другой файловой системе».
fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        if meta.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
        let _ = std::fs::set_permissions(&to, meta.permissions());
    }
    Ok(())
}

/// Порт `_rmtree_quiet`: снести файл либо дерево, не мешая исходу шага.
fn rmtree_quiet(path: &str) {
    let p = Path::new(path);
    if p.is_dir() {
        let _ = std::fs::remove_dir_all(p);
    } else if p.exists() {
        let _ = std::fs::remove_file(p);
    }
}

/// Порт `_drop_tmp`: убрать временный файл воркера.
pub fn drop_tmp(path: &str) {
    if path.is_empty() {
        return;
    }
    let _ = std::fs::remove_file(path);
}

// ============================================================================
// Стейт ~/.config/rexon/dev_upstream_state.json
// ============================================================================

/// Замок правки стейта. Починка 3: проверка и установка меняли один файл из
/// разных потоков без всякой синхронизации, и одновременная запись теряла то
/// отчёт, то записанную версию.
fn state_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Порт `_read_state_text`: сырой текст файла (`None` — нет/не читается).
fn read_state_text(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Прочитать-изменить-записать стейт под общим замком.
fn edit_state(path: &str, edit: impl FnOnce(&mut Map<String, Value>)) {
    let _guard = state_lock().lock();
    let mut state = eu::load_state(read_state_text(path).as_deref());
    edit(&mut state);
    write_state_text(path, &eu::serialize_state(&state));
}

/// Порт `_write_state_text`: атомарная запись через временное имя из ядра.
fn write_state_text(path: &str, text: &str) {
    if let Some(dir) = Path::new(path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // ⭐ Единственная атомарная дверь профиля: временный файл РЯДОМ с целью,
    // `fsync`, переименование. Своей пары «запись + переименование» здесь
    // больше нет — без `fsync` после сбоя питания на месте целого прежнего
    // состояния оказывалось целое новое из нулей, и страница показывала бы
    // «движок не проверялся» на проверенном движке. Сторож —
    // `rexon-core/tests/profile_writes_atomic.rs`.
    let _ = rexon_core::utils::atomic_write(path, text);
}

/// Порт `_now_iso`: метка времени стейта в формате ядра.
fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    eu::now_iso(secs)
}

// ============================================================================
// Временные файлы и подпроцессы
// ============================================================================

/// Порядковый номер временного имени — чтобы две задачи одного процесса не
/// целились в один путь в пределах наносекунды.
fn temp_counter() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Хвост уникального имени: время, идентификатор процесса и счётчик.
fn temp_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 + d.as_secs())
        .unwrap_or(0);
    format!("{:x}{:x}{:x}", std::process::id(), nanos, temp_counter())
}

/// Порт `tempfile.mkstemp(prefix, suffix)`: создать пустой файл под своим
/// именем. `create_new` — чужой файл с таким именем перехвачен не будет.
fn mktemp_file(prefix: &str, suffix: &str) -> Option<String> {
    let dir = std::env::temp_dir();
    for _ in 0..64 {
        let path = dir.join(format!("{prefix}{}{suffix}", temp_token()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(_) => return Some(path.to_string_lossy().into_owned()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// Порт `tempfile.mkdtemp(prefix)`.
fn mktemp_dir(prefix: &str) -> Option<String> {
    let dir = std::env::temp_dir();
    for _ in 0..64 {
        let path = dir.join(format!("{prefix}{}", temp_token()));
        match std::fs::create_dir(&path) {
            Ok(()) => {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
                return Some(path.to_string_lossy().into_owned());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// `subprocess.run(argv, capture_output=True, timeout=…)` — стандартный вывод
/// и поток ошибок. `None` — не запустилось либо не уложилось в потолок.
/// Код выхода процесса (`None` — не запустился или не уложился в срок).
async fn run_status(argv: &[String], timeout_secs: u64) -> Option<i32> {
    let (exe, args) = argv.split_first()?;
    let fut = tokio::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), fut)
        .await
        .ok()?
        .ok()?
        .code()
}

/// Отвечает ли демон на шине (свойство `Version` читается).
async fn daemon_on_bus() -> bool {
    run_capture(&eu::busctl_version_argv(), eu::BUSCTL_TIMEOUT_SECS)
        .await
        .map(|(stdout, _)| !eu::parse_busctl_version(&stdout).is_empty())
        .unwrap_or(false)
}

/// Остановить движок перед подменой его файла — по плану ядра
/// (`engine_update::engine_swap_plan`): служба — `systemctl --user stop`,
/// демон вне службы — его же метод `Quit`. `Err` — остановить не вышло, и
/// подменять файл НЕЛЬЗЯ.
async fn stop_engine_for_swap() -> Result<eu::EngineSwapPlan, String> {
    let unit_active = run_status(
        &eu::engine_unit_active_argv(),
        rexon_core::we_client::SYSTEMCTL_TIMEOUT_SECS,
    )
    .await
        == Some(0);
    let plan = eu::engine_swap_plan(unit_active, daemon_on_bus().await);
    if let Some(argv) = &plan.stop_argv {
        if run_status(argv, rexon_core::we_client::SYSTEMCTL_TIMEOUT_SECS).await != Some(0) {
            return Err(eu::appimage_install_busy_message());
        }
    }
    if plan.quit_daemon {
        let _ = run_status(&eu::busctl_quit_argv(), eu::BUSCTL_TIMEOUT_SECS).await;
        let mut gone = false;
        for _ in 0..eu::QUIT_WAIT_STEPS {
            if !daemon_on_bus().await {
                gone = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(eu::QUIT_WAIT_STEP_MS)).await;
        }
        if !gone {
            return Err(eu::appimage_install_busy_message());
        }
    }
    Ok(plan)
}

async fn run_capture(argv: &[String], timeout_secs: u64) -> Option<(String, String)> {
    let (exe, args) = argv.split_first()?;
    let fut = tokio::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output();
    let out = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), fut)
        .await
        .ok()?
        .ok()?;
    Some((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

// ============================================================================
// Проверки
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Записанный эталон живого Python (мост `rexon-py` ушёл в архив).
    const REFERENCE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/engine_update_reference.json"
    ));

    fn reference() -> Value {
        serde_json::from_str(REFERENCE).expect("эталон engine_update обязан быть валидным JSON")
    }

    /// Сообщения сверяются на ИСХОДНОМ языке. С 30.09 (65e9b32) тексты —
    /// английские msgid каталога, русский приезжает переводом; эталон
    /// Python-эпохи был русским, и сверка зеленела только на русской локали
    /// с установленным каталогом — и то пока соседняя сверка процесса не
    /// переключала язык на английский. Язык процесса — общий, поэтому
    /// закрепляем тот же, что ставят соседи крейта (`SOURCE_LANGUAGE`).
    fn source_language() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    }

    /// Значение эталона с подстановкой настоящего домашнего каталога вместо
    /// плейсхолдера `<H>`, которым его записал Python.
    fn expect_str(key: &str, home: &str) -> String {
        reference()[key]
            .as_str()
            .unwrap_or_else(|| panic!("в эталоне нет строки {key}"))
            .replace("<H>", home)
    }

    fn expect_pair(key: &str, home: &str) -> (bool, String) {
        let v = reference();
        let arr = v[key]
            .as_array()
            .unwrap_or_else(|| panic!("в эталоне нет пары {key}"));
        (
            arr[0].as_bool().unwrap_or(false),
            arr[1].as_str().unwrap_or_default().replace("<H>", home),
        )
    }

    struct Sandbox(String);

    impl Sandbox {
        fn new(tag: &str) -> Sandbox {
            let path = std::env::temp_dir().join(format!("rexon-we-upd-{tag}-{}", temp_token()));
            std::fs::create_dir_all(&path).expect("песочница обязана создаться");
            Sandbox(path.to_string_lossy().into_owned())
        }

        fn env(&self) -> UpdateEnv {
            UpdateEnv {
                home: self.0.clone(),
                xdg_config_home: None,
                xdg_data_home: None,
                engine_dir: format!("{}/engine", self.0),
                system_daemon: format!("{}/usr/bin/waywallen", self.0),
                system_engine_dir: format!("{}/usr/share/waywallen", self.0),
            }
        }

        fn join(&self, rel: &str) -> String {
            format!("{}/{rel}", self.0)
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            rmtree_quiet(&self.0);
        }
    }

    fn write_exec(path: &str, body: &[u8], mode: u32) {
        if let Some(dir) = Path::new(path).parent() {
            std::fs::create_dir_all(dir).expect("каталог обязан создаться");
        }
        std::fs::write(path, body).expect("файл обязан записаться");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .expect("права обязаны выставиться");
    }

    fn mode_of(path: &str) -> u32 {
        std::fs::metadata(path)
            .expect("файл обязан существовать")
            .permissions()
            .mode()
            & 0o7777
    }

    /// Стейт с подменённой меткой времени: её живой Python замораживал, а мы
    /// снимаем настоящую — сверяем всё остальное побайтово.
    fn state_with_frozen_stamp(path: &str) -> String {
        let text = std::fs::read_to_string(path).expect("стейт обязан быть записан");
        let mut out = String::new();
        for line in text.lines() {
            if let Some(head) = line.split_once("_at\": \"") {
                out.push_str(head.0);
                out.push_str("_at\": \"2026-07-25T17:34:56Z\"");
                if line.ends_with(',') {
                    out.push(',');
                }
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }
        out.trim_end_matches('\n').to_string()
    }

    // ── Скачивание с локального пути (эталон dl_*) ──────────────────────

    #[test]
    fn download_local_path_matches_reference() {
        let sb = Sandbox::new("dl");
        let src = sb.join("local.bin");
        std::fs::write(&src, b"LOCAL").expect("исходник обязан записаться");
        let cancel = net::CancelToken::new();

        let spec = eu::download_spec(eu::DownloadKind::AppImage);
        let p = download_asset(&src, &spec, &cancel, |_, _| {}).expect("копия обязана получиться");
        let name = Path::new(&p)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let dl_app = reference()["dl_app"].clone();
        assert!(name.starts_with(dl_app[0].as_str().unwrap()));
        assert!(name.ends_with(dl_app[1].as_str().unwrap()));
        assert_eq!(format!("{:#o}", mode_of(&p)).replace("0o", "0o"), "0o755");
        assert_eq!(std::fs::read(&p).unwrap(), b"LOCAL");
        drop_tmp(&p);

        let spec = eu::download_spec(eu::DownloadKind::PluginZip);
        let p = download_asset(&src, &spec, &cancel, |_, _| {}).expect("копия обязана получиться");
        let name = Path::new(&p)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let dl_zip = reference()["dl_zip"].clone();
        assert!(name.starts_with(dl_zip[0].as_str().unwrap()));
        assert!(name.ends_with(dl_zip[1].as_str().unwrap()));
        assert_eq!(mode_of(&p), 0o644);
        drop_tmp(&p);

        // dl_empty: пустой адрес — ничего не создаём.
        assert!(download_asset("", &spec, &cancel, |_, _| {}).is_none());
    }

    // ── Установка AppImage (эталон ia_*) ────────────────────────────────

    #[test]
    fn install_appimage_matches_reference() {
        source_language();
        let sb = Sandbox::new("ia");
        let env = sb.env();
        let dest = env.managed_appimage();

        // ia_fresh: чистая установка.
        let src = sb.join("new.AppImage");
        write_exec(&src, b"NEW", 0o755);
        let got = install_appimage(&env, &src, "v0.2.5");
        assert_eq!(got, expect_pair("ia_fresh", &sb.0));
        assert_eq!(mode_of(&dest), 0o755);
        assert_eq!(std::fs::read(&dest).unwrap(), b"NEW");
        assert!(!Path::new(&src).exists(), "ia_src_removed");
        assert_eq!(
            state_with_frozen_stamp(&env.state_path()),
            expect_str("ia_fresh_state", &sb.0)
        );

        // ia_over: поверх прежней — прежняя уезжает в .bak.
        let src2 = sb.join("new2.AppImage");
        write_exec(&src2, b"NEWER", 0o755);
        let got = install_appimage(&env, &src2, "0.2.6");
        assert_eq!(got, expect_pair("ia_over", &sb.0));
        assert_eq!(std::fs::read(format!("{dest}.bak-update")).unwrap(), b"NEW");
        assert_eq!(std::fs::read(&dest).unwrap(), b"NEWER");
        assert_eq!(
            state_with_frozen_stamp(&env.state_path()),
            expect_str("ia_over_state", &sb.0)
        );

        // ia_missing: исходника нет — откат, мусора не остаётся.
        let (ok, msg) = install_appimage(&env, &sb.join("nope"), "v1");
        assert!(!ok);
        assert_eq!(msg.split(':').next().unwrap(), "installation failed, rolled back");
        assert!(
            !Path::new(&format!("{dest}.new-update")).exists(),
            "ia_no_tmp_left"
        );
        assert_eq!(std::fs::read(&dest).unwrap(), b"NEWER", "ia_rollback_dest");
    }

    // ── Валидация AppImage без запуска (эталон va_missing/va_noexec) ────

    #[test]
    fn validate_appimage_file_checks_match_reference() {
        source_language();
        let sb = Sandbox::new("va");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("рантайм проверки обязан создаться");

        let missing = sb.join("absent");
        assert_eq!(
            rt.block_on(validate_appimage(&missing)),
            expect_pair("va_missing", &sb.0)
        );

        let noexec = sb.join("noexec");
        write_exec(&noexec, b"x", 0o644);
        assert_eq!(
            rt.block_on(validate_appimage(&noexec)),
            expect_pair("va_noexec", &sb.0)
        );
    }

    // ── Установка плагин-zip (эталон ipz_*) ─────────────────────────────

    const MANIFEST_OK: &str = "[plugin]\n\
id = \"org.waywallen.open-wallpaper-engine\"\n\
name = \"Open Wallpaper Engine\"\n\
version = \"0.1.10\"\n";

    fn make_zip(path: &str, entries: &[(&str, &str, u32)]) -> String {
        let file = std::fs::File::create(path).expect("архив обязан создаться");
        let mut zw = zip::ZipWriter::new(file);
        for (name, body, mode) in entries {
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(*mode);
            zw.start_file(*name, opts).expect("запись обязана начаться");
            zw.write_all(body.as_bytes()).expect("тело обязано лечь");
        }
        zw.finish().expect("архив обязан закрыться");
        path.to_string()
    }

    #[test]
    fn install_plugin_zip_fs_matches_reference() {
        source_language();
        let sb = Sandbox::new("ipz");
        let env = sb.env();
        // Эталон снят с plugins_root = <H>/work/plugins.
        let proot = sb.join("work/plugins");
        let zp = make_zip(
            &sb.join("install.zip"),
            &[
                ("plugin.toml", MANIFEST_OK, 0o644),
                ("files.txt", "bin/renderer\n", 0o644),
                ("bin/renderer", "#!/bin/sh\necho hi\n", 0o755),
            ],
        );

        let got = install_plugin_zip_fs(&env, &zp, "v0.2.0", &proot);
        assert_eq!(got, expect_pair("ipz_fresh", &sb.0));

        let tgt = format!("{proot}/{}", eu::OWE_PLUGIN_ID);
        let mut tree: Vec<String> = Vec::new();
        collect_files(Path::new(&tgt), Path::new(&tgt), &mut tree);
        tree.sort();
        let want: Vec<String> = reference()["ipz_tree"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(tree, want, "ipz_tree");
        assert_eq!(mode_of(&format!("{tgt}/bin/renderer")), 0o755, "ipz_mode");
        assert_eq!(
            state_with_frozen_stamp(&env.state_path()),
            expect_str("ipz_state", &sb.0)
        );

        // ipz_over: повторная установка — прежняя версия в .bak-update.
        let got = install_plugin_zip_fs(&env, &zp, "0.2.1", &proot);
        assert_eq!(got, expect_pair("ipz_over", &sb.0));
        assert!(
            Path::new(&format!("{tgt}.bak-update")).is_dir(),
            "ipz_bak"
        );

        // ipz_invalid: архив без plugin.toml.
        let bad = make_zip(&sb.join("bad.zip"), &[("files.txt", "x", 0o644)]);
        assert_eq!(
            install_plugin_zip_fs(&env, &bad, "1", &proot),
            expect_pair("ipz_invalid", &sb.0)
        );

        // ipz_broken: валидация проходит, а распаковка падает на CRC —
        // рабочий установленный плагин обязан УЦЕЛЕТЬ.
        let broken = sb.join("corrupt.zip");
        make_stored_zip(
            &broken,
            &[
                ("plugin.toml", MANIFEST_OK),
                ("files.txt", "bin/renderer\n"),
                ("bin/renderer", "RENDERER-PAYLOAD-XYZ\n"),
            ],
        );
        corrupt_payload(&broken, b"RENDERER-PAYLOAD-XYZ", b"ZZZZ");
        assert!(
            eu::validate_plugin_zip(Path::new(&broken)).ok,
            "ipz_broken_valid"
        );
        let (ok, _) = install_plugin_zip_fs(&env, &broken, "0.2.2", &proot);
        assert!(!ok, "ipz_broken");
        assert!(
            Path::new(&format!("{tgt}/plugin.toml")).is_file(),
            "ipz_survived"
        );
    }

    /// Копия дерева переживает перенос между файловыми системами — ветка,
    /// которую в живой установке включает `/tmp` в оперативной памяти.
    #[test]
    fn copy_tree_keeps_exec_bit() {
        let sb = Sandbox::new("tree");
        write_exec(&sb.join("src/bin/renderer"), b"#!/bin/sh\n", 0o755);
        write_exec(&sb.join("src/plugin.toml"), MANIFEST_OK.as_bytes(), 0o644);
        copy_tree(Path::new(&sb.join("src")), Path::new(&sb.join("dst")))
            .expect("копия дерева обязана удаться");
        assert_eq!(mode_of(&sb.join("dst/bin/renderer")), 0o755);
        assert_eq!(mode_of(&sb.join("dst/plugin.toml")), 0o644);
    }

    /// Отметки стейта — те же ключи и тот же формат, что у оригинала.
    #[test]
    fn state_marks_roundtrip() {
        let sb = Sandbox::new("state");
        let env = sb.env();
        let tasks = WeUpdateTasks::new(env.clone()).expect("рантайм обязан подняться");
        tasks.mark_seen("owe", "v0.1.11");
        tasks.mark_installed_tag("owe", "v0.1.12");
        let state = eu::load_state(read_state_text(&env.state_path()).as_deref());
        assert_eq!(eu::state_field(&state, "owe", "seen_version"), "v0.1.11");
        assert_eq!(
            eu::state_field(&state, "owe", "installed_version"),
            "0.1.12"
        );
        tasks.shutdown();
    }

    fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_files(root, &p, out);
            } else if let Ok(rel) = p.strip_prefix(root) {
                out.push(rel.to_string_lossy().into_owned());
            }
        }
    }

    fn make_stored_zip(path: &str, entries: &[(&str, &str)]) {
        let file = std::fs::File::create(path).expect("архив обязан создаться");
        let mut zw = zip::ZipWriter::new(file);
        for (name, body) in entries {
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o644);
            zw.start_file(*name, opts).expect("запись обязана начаться");
            zw.write_all(body.as_bytes()).expect("тело обязано лечь");
        }
        zw.finish().expect("архив обязан закрыться");
    }

    fn corrupt_payload(path: &str, needle: &[u8], patch: &[u8]) {
        let mut raw = std::fs::read(path).expect("архив обязан читаться");
        let at = raw
            .windows(needle.len())
            .position(|w| w == needle)
            .expect("подпись обязана найтись в архиве без сжатия");
        raw[at..at + patch.len()].copy_from_slice(patch);
        std::fs::write(path, raw).expect("архив обязан переписаться");
    }

    // ── Живая проверка релизов ──────────────────────────────────────────

    /// Настоящий поход в GitHub за релизами обеих целей.
    ///
    /// Запуск: `cargo test -p rexon-gui --lib upstream_check_live -- --ignored
    /// --nocapture`. Отдельно от обычного прогона — сеть в тестах не
    /// обязательна; ставить движок этот тест не умеет и не пытается.
    #[test]
    #[ignore = "нужен интернет"]
    fn upstream_check_live() {
        let sb = Sandbox::new("live");
        let env = sb.env();
        let tasks = WeUpdateTasks::new(env).expect("рантайм обязан подняться");
        assert!(tasks.start_check(String::new()));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        let mut statuses = None;
        while std::time::Instant::now() < deadline {
            if let Some(UpdateEvent::CheckDone(v)) = tasks.poll_event() {
                statuses = Some(v);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        tasks.shutdown();
        let statuses = statuses.expect("проверка обязана ответить");
        assert_eq!(statuses.len(), 2, "целей ровно две");
        for st in &statuses {
            println!(
                "{}: ok={} наша={:?} последняя={:?} обновление={} ассет={:?}\n  {}",
                st.key, st.ok, st.installed, st.latest, st.update_available, st.asset_url, st.note
            );
            assert!(st.ok, "цель {} не проверилась: {}", st.key, st.note);
            assert!(!st.latest.is_empty(), "у {} нет последнего тега", st.key);
        }
    }
}
