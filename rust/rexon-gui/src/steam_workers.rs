// Фоновые задачи вкладки «Steam» — порт пяти `QThread` из
// `rexon/qt_steam/window.py` на async (tokio).
//
// Оригинал держал пять наследников QThread, и каждый жил сам по себе:
//
//   1. `_SplashMetaWorker`  — весь публичный инвентарь → метки видеозаставок
//                             → дисковый кэш → сигнал `done(meta)`;
//   2. `_SplashPosterWorker`— кадры-превью роликов (ffprobe + ffmpeg) →
//                             сигнал `ready(id, png)` на каждый готовый;
//   3. `_InventoryWorker`   — владеемые фоны через WebAPI, иначе весь
//                             публичный инвентарь → сигнал `done(items, err)`;
//   4. `_ApplyBgWorker`     — скачать фон и применить обоями →
//                             сигнал `done(ok, msg)`;
//   5. `_ThumbLoader`       — миниатюры карточек текущей страницы →
//                             сигнал `loaded(id, path)` на каждую.
//
// Здесь это пять задач одного рантайма, а сигналы Qt заменены одной очередью
// событий ([`SteamEvent`]): владелец страницы забирает их из своего потока
// ([`SteamTasks::poll_event`]) и раскладывает по модели. Очередь, а не
// колбэки, — чтобы данные пересекали границу потоков ровно в одном месте.
//
// ⚠️ ОТМЕНА. В Qt поток можно было бросить; в async нужен явный токен, и он
// здесь ДЕРЕВОМ: корень вкладки → узел витрины → задача. Закрытие вкладки
// гасит корень, смена страницы фонов — только узел миниатюр, пересборка
// витрины — только узел постеров. Токен доходит и до самого запроса:
// синхронный ureq ядра умеет рвать уже отправленный запрос
// (`rexon_core::net::CancelToken`), и наш узел взводит его.
//
// ⚠️ Починки оригинала (правило #16), каждая помечена `Починка:` по месту:
//   1. ДВОЙНАЯ ВЫГРУЗКА ИНВЕНТАРЯ. `_InventoryWorker` и `_SplashMetaWorker`
//      тянули один и тот же публичный инвентарь каждый сам (до 20 страниц по
//      2000 предметов — дважды на каждое открытие вкладки; у владельца это
//      2029 предметов). Теперь страницы качаются ОДИН раз и кормят обе
//      витрины;
//   2. ОСИРОТЕВШИЕ ПОТОКИ. `_render_bg_page` звал `stop()` у прежнего
//      `_ThumbLoader` и тут же ЗАМЕЩАЛ ссылку на него новым — прежний поток
//      никто больше не ждал. При быстром листании оставалась пачка живых
//      потоков, а `closeEvent` ждал только последний: отсюда падение
//      «QThread: Destroyed while thread is still running». Здесь все задачи
//      считает `TaskTracker`, и закрытие ждёт каждую;
//   3. ЗАКРЫТИЕ ВИСЛО ДО 30 СЕКУНД. `stop()` оригинала — флажок, который
//      проверялся МЕЖДУ запросами, поэтому `th.wait(30000)` в `closeEvent`
//      честно ждал конца сетевого таймаута. Токен рвёт запрос на месте;
//   4. У `_ApplyBgWorker` отмены не было ВОВСЕ: `done` летел в закрывающееся
//      окно. Теперь применение сидит на общем токене вкладки, и результат в
//      закрытую вкладку не доставляется;
//   5. МИНИАТЮРЫ КАЧАЛИСЬ ПО ОДНОЙ. 25 файлов страницы шли строго
//      последовательно. Теперь до `THUMB_CONCURRENCY` разом — порядок
//      появления перестал быть порядком списка, но карточки и раньше
//      наполнялись по мере готовности, а не рядами;
//   6. НЕДОКАЧАННЫЙ `.part` оставался в кэше на любой неудаче
//      (`_download_community` заводил его и не убирал). Теперь временный
//      файл снимается на любом исходе.
//
// Чего здесь НЕТ: решений. Что показать, в каком порядке и какими словами,
// отвечает `rexon_core::qt_steam`; побочные действия, которых на Rust ещё
// нет (движок обоев, добыча токена WebAPI из cookie Steam), приходят
// объектами снаружи.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rexon_core::i18n::tr;
use rexon_core::net::{self, NetIo, UreqNet};
use rexon_core::qt_steam as qs;
use rexon_core::session_splash as ss;
use rexon_core::steam_profile;
use rexon_core::steam_profile_items::{self as spi, ProfileBackgroundItem};
use rexon_core::steam_token;
use rexon_core::wallpaper_exec::{self, ExecSlot, LiveSys, WpSys};
use serde_json::Value;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Сколько миниатюр качаем разом. Починка №5: оригинал качал по одной, и
/// страница из 25 карточек наполнялась ровно 25 последовательными
/// запросами. Шесть — компромисс: заметно быстрее и при этом не похоже на
/// налёт на CDN Steam.
pub const THUMB_CONCURRENCY: usize = 6;

/// Сколько ждём завершения задач при закрытии вкладки. Это ПОТОЛОК, а не
/// плановое ожидание: токен уже взведён, задачи рвут запросы сами, и в норме
/// ожидание занимает миллисекунды. Оригинал ждал до 30 секунд И БЕЗ отмены.
pub const SHUTDOWN_GRACE_MS: u64 = 2000;

/// Рабочих потоков рантайма. Задач немного и все они ждут сеть либо ffmpeg,
/// поэтому пары потоков хватает: тяжёлое всё равно уходит в блокирующий пул.
const WORKER_THREADS: usize = 2;

// ============================================================================
// События наружу
// ============================================================================

/// То, что задачи отдают владельцу страницы. Замена сигналов пяти QThread.
#[derive(Debug, Clone, PartialEq)]
pub enum SteamEvent {
    /// `_InventoryWorker.done(items, error)`. Пустая ошибка = всё хорошо.
    InventoryDone {
        items: Vec<ProfileBackgroundItem>,
        error: String,
    },
    /// Чем кончился запрос владеемых предметов — единственный источник
    /// анимированных фонов. Приходит ДО `InventoryDone` при каждой загрузке
    /// списка; `AnimatedGap::None` снимает прежнюю причину.
    OwnedAnswer(qs::AnimatedGap),
    /// `_SplashMetaWorker.done(meta)` — метки видеозаставок из инвентаря.
    SplashMeta(qs::MetaMap),
    /// `_SplashPosterWorker.ready(id, png)` — кадр-превью ролика.
    Poster { id: String, png: String },
    /// `_ThumbLoader.loaded(id, path)` — миниатюра карточки фона.
    Thumb { id: String, path: String },
    /// `_ApplyBgWorker.done(ok, msg)`.
    ApplyDone { ok: bool, message: String },
    /// Полноразмерный фон для крупного просмотра добыт (или нет): `still` —
    /// картинка, `movie` — ролик анимированного фона; пустая строка — этой
    /// части нет либо она не скачалась.
    Full { id: String, still: String, movie: String },
}

// ============================================================================
// Побочные действия, которых на Rust ещё нет
// ============================================================================

/// Движок обоев. Живая реализация — [`ExecApplier`] (исполнитель ядра),
/// подменные — в сверках.
///
/// Метод синхронный и возвращает «получилось ли» вместе с текстом для
/// строки состояния — ровно то, что оригинал доставал из `res.message`.
pub trait WallpaperApply: Send + Sync {
    fn apply(&self, path: &str, format: &str, audio: bool, name: &str) -> (bool, String);
}

/// Живой движок обоев вкладки: общий исполнитель ядра, без своих правил.
///
/// Что и как применять, решает ядро ([`qs::apply_background_with`] →
/// `WallpaperExec::apply_with_audio`); слот исполнителя — один на процесс
/// и общий с «Настройками» (`wallpaper_exec::live_slot`); применение и
/// надзор за нагрузкой — общая дверь ядра (`wallpaper_exec::run_in_slot`).
///
/// ПОЧЕМУ ОДИН НА ПРОЦЕСС, а не на страницу: страница «Steam» живёт в
/// `Loader` главного окна и пересоздаётся, а исполнитель помнит, какой
/// процесс рисует обои и что вернуть после остановки. Свой исполнитель у
/// страницы не знал бы о видео, запущенном прежней её копией или
/// «Настройками», и «Остановить» там не сняло бы поставленное здесь.
pub struct ExecApplier<S: WpSys = LiveSys> {
    slot: ExecSlot<S>,
}

impl ExecApplier {
    /// Движок на исполнителе процесса.
    pub fn live() -> ExecApplier {
        ExecApplier { slot: wallpaper_exec::live_slot() }
    }
}

impl<S: WpSys> ExecApplier<S> {
    /// Движок на явно данном слоте — так сверки подставляют исполнитель с
    /// подменным швом.
    pub fn on(slot: ExecSlot<S>) -> ExecApplier<S> {
        ExecApplier { slot }
    }
}

impl<S: WpSys + Default + Send + 'static> WallpaperApply for ExecApplier<S> {
    // `format` не нужен: ядро выводит его из пути той же функцией
    // (`qs::apply_format`), что посчитала его и здесь. `audio` — галочка
    // «🔊 Звук видео-обоев», уходит исполнителю как есть.
    fn apply(&self, path: &str, _format: &str, audio: bool, name: &str) -> (bool, String) {
        wallpaper_exec::run_in_slot(&self.slot, |exec| {
            qs::apply_background_with(exec, path, audio, name)
        })
    }
}

// ============================================================================
// Окружение
// ============================================================================

/// Пути, от которых зависят кэши. Снимаются один раз при постройке: читать
/// `$HOME` из рабочей задачи незачем, а тесты подставляют своё.
#[derive(Debug, Clone, Default)]
pub struct SteamEnv {
    pub home: String,
    pub xdg_cache_home: Option<String>,
    pub xdg_config_home: Option<String>,
}

impl SteamEnv {
    /// Живое окружение процесса.
    pub fn from_process() -> SteamEnv {
        let nonempty = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        SteamEnv {
            home: std::env::var("HOME").unwrap_or_default(),
            xdg_cache_home: nonempty("XDG_CACHE_HOME"),
            xdg_config_home: nonempty("XDG_CONFIG_HOME"),
        }
    }

    fn cache(&self) -> Option<&str> {
        self.xdg_cache_home.as_deref()
    }

    fn config(&self) -> Option<&str> {
        self.xdg_config_home.as_deref()
    }
}

/// Заказ на загрузку инвентаря.
#[derive(Debug, Clone, Default)]
pub struct InventoryRequest {
    /// SteamID64 аккаунта.
    pub sid: String,
    /// Надетый в профиле фон (разобран из `localconfig.vdf` заранее).
    pub equipped: Option<ProfileBackgroundItem>,
    /// Токен WebAPI для владеемых фонов, если хозяин страницы принёс его
    /// сам (сверки). Пустой — задача добывает его сама, в блокирующем пуле:
    /// `rexon_core::steam_token::access_token` ходит в сеть, и в потоке
    /// интерфейса ему не место.
    ///
    /// ⚠️ До 13.10.2026 токен обязан был ПРИЕХАТЬ со страницы
    /// (`actions.webapiToken()`), а метод с 08.09.2026 не был виден из QML
    /// (потерян `#[qinvokable]`): страница всегда стартовала с пустым
    /// токеном, владеемые фоны не запрашивались, и анимированных в списке
    /// не было ни одного. Теперь страница токен не носит вовсе.
    pub webapi_token: String,
}

// ============================================================================
// Рантайм вкладки
// ============================================================================

/// Все фоновые задачи вкладки «Steam» и их отмена.
pub struct SteamTasks {
    /// ⚠️ `Option`, а не просто `Runtime`, и это не украшение. Обычный
    /// `Drop` рантайма tokio ждёт блокирующие задачи БЕЗ СРОКА: одна
    /// застрявшая — и процесс не выходит никогда. Здесь рантайм забирают из
    /// поля и роняют [`Runtime::shutdown_timeout`] — с потолком.
    rt: Option<Runtime>,
    /// Корень дерева отмены: закрытие вкладки гасит ВСЁ.
    root: CancellationToken,
    /// Учёт живых задач — чтобы закрытие дождалось каждой (починка №2).
    tracker: TaskTracker,
    tx: UnboundedSender<SteamEvent>,
    rx: Mutex<UnboundedReceiver<SteamEvent>>,
    /// Узел отмены миниатюр: живёт ровно до следующей страницы.
    thumbs: Mutex<Option<CancellationToken>>,
    /// Узел отмены кадров-превью: живёт ровно до следующей пересборки витрины.
    posters: Mutex<Option<CancellationToken>>,
    /// Идёт ли применение обоев (оригинальный `_wp_worker.isRunning()`).
    applying: Arc<AtomicBool>,
    env: SteamEnv,
}

impl SteamTasks {
    /// Поднять рантайм вкладки. `None` — рантайм не создался (в живом Rexon
    /// это значит «фоновых задач не будет», а не падение).
    pub fn new(env: SteamEnv) -> Option<SteamTasks> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(WORKER_THREADS)
            .thread_name("rexon-steam")
            // ⚠️ Починка: без enable_io `tokio::process` ПАНИКУЕТ в рабочем
            // потоке («A Tokio 1.x context was found, but IO is disabled»), а
            // через него идут ffprobe и ffmpeg задачи кадров-превью. Поймано
            // живым прогоном соседней вкладки, где тот же рантайм запускает
            // подпроцессы.
            .enable_io()
            .enable_time()
            .build()
            .ok()?;
        let (tx, rx) = unbounded_channel();
        let root = CancellationToken::new();
        // ⭐ ОБЩАЯ ДВЕРЬ ВЫХОДА (правило #12). Сток фоновых задач
        // (`exit_guard`) считает только то, что ему показали: рабочие потоки
        // рантайма он не видит и без этой записи честно отвечал «не
        // осталось никого» при работающей выгрузке инвентаря. Просьбу
        // дёргают ДО разбора окна — значит сеть рвётся раньше, чем начнут
        // сноситься объекты, которых задача могла бы коснуться.
        {
            let root = root.clone();
            rexon_core::exit_guard::register_stopper(move || root.cancel());
        }
        Some(SteamTasks {
            rt: Some(rt),
            root,
            tracker: TaskTracker::new(),
            tx,
            rx: Mutex::new(rx),
            thumbs: Mutex::new(None),
            posters: Mutex::new(None),
            applying: Arc::new(AtomicBool::new(false)),
            env,
        })
    }

    /// Забрать очередное событие; `None` — очередь пуста. Зовётся из потока
    /// интерфейса (таймером страницы), никогда не блокирует.
    pub fn poll_event(&self) -> Option<SteamEvent> {
        self.rx.lock().ok()?.try_recv().ok()
    }

    /// Идёт ли применение обоев прямо сейчас.
    pub fn is_applying(&self) -> bool {
        self.applying.load(Ordering::SeqCst)
    }

    /// Отменена ли вкладка целиком (закрытие уже началось).
    pub fn is_cancelled(&self) -> bool {
        self.root.is_cancelled()
    }

    // ── Задача 1+3: инвентарь и метки заставок ─────────────────────────

    /// Порт `_InventoryWorker` И `_SplashMetaWorker` одной задачей.
    ///
    /// Починка №1: страницы публичного инвентаря качаются ОДИН раз. Дальше
    /// один и тот же ответ разбирается дважды — на фоны профиля
    /// ([`spi::parse_inventory_backgrounds`]) и на метки видеозаставок
    /// ([`qs::splash_item_meta`]), и оба события уходят наружу по
    /// готовности, как и раньше двумя сигналами.
    ///
    /// Путь владеемых фонов (WebAPI) сохранён: только он отдаёт ссылки на
    /// АНИМАЦИЮ. Но метки заставок он не отдаёт, поэтому публичный инвентарь
    /// тянется и в этом случае — как в оригинале, просто уже не дважды.
    pub fn start_inventory(&self, req: InventoryRequest) {
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let env = self.env.clone();
        self.spawn(async move {
            // Владеемые фоны: единственный источник ссылок на анимацию.
            // Порядок шагов и приговор — ядро (`owned_backgrounds_with`),
            // отсюда только ключ и сеть.
            let mut answered = false;
            let given = req.webapi_token.clone();
            let home = env.home.clone();
            let owned = blocking_cancellable(&token, move |cancel| {
                qs::owned_backgrounds_with(
                    |force| {
                        if !given.is_empty() {
                            return (given.clone(), String::new());
                        }
                        if force {
                            steam_token::forget_token();
                        }
                        steam_token::access_token(&home, force)
                    },
                    |key| fetch_owned(key, cancel.clone()),
                )
            })
            .await;
            let Some((owned, gap)) = owned else { return };
            let _ = tx.send(SteamEvent::OwnedAnswer(gap));
            if !owned.is_empty() {
                let items = spi::finalize_items(owned, req.equipped.clone());
                let _ = tx.send(SteamEvent::InventoryDone {
                    items,
                    error: String::new(),
                });
                answered = true;
            }

            // Публичный инвентарь: и фоны (если владеемые не вышли), и
            // метки заставок — с ОДНОЙ выгрузки.
            let sid = req.sid.clone();
            let pages = blocking_cancellable(&token, move |cancel| {
                fetch_inventory_pages(&sid, cancel)
            })
            .await;
            let Some(pages) = pages else { return };

            let data = match pages {
                Ok(v) => v,
                Err(e) => {
                    if !answered {
                        let _ = tx.send(SteamEvent::InventoryDone {
                            items: Vec::new(),
                            error: e,
                        });
                    }
                    return;
                }
            };
            if token.is_cancelled() {
                return;
            }

            if !answered {
                let eq_id = req.equipped.as_ref().map(|e| e.id.clone());
                let parsed = spi::parse_inventory_backgrounds(&data, eq_id.as_deref());
                let items = spi::finalize_items(parsed, req.equipped.clone());
                let _ = tx.send(SteamEvent::InventoryDone {
                    items,
                    error: String::new(),
                });
            }

            // Метки заставок + их дисковый кэш.
            let descs = data
                .get("descriptions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let meta = qs::splash_item_meta(&descs);
            if meta.is_empty() || token.is_cancelled() {
                return;
            }
            let path = qs::splash_meta_cache_path(env.cache(), &env.home, &req.sid);
            let text = qs::meta_cache_json(&meta);
            let _ = tokio::task::spawn_blocking(move || write_atomic(&path, text.as_bytes())).await;
            if !token.is_cancelled() {
                let _ = tx.send(SteamEvent::SplashMeta(meta));
            }
        });
    }

    // ── Задача 2: кадры-превью роликов ─────────────────────────────────

    /// Порт `_SplashPosterWorker`. `jobs` — пути роликов, у которых кадра
    /// ещё нет. Прежняя пачка отменяется: витрина пересобралась, старые
    /// кадры уже никому не нужны (починка №2 — прежняя задача не сиротеет,
    /// а гасится).
    pub fn start_posters(&self, jobs: Vec<String>) {
        let token = self.swap_node(&self.posters);
        if jobs.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let env = self.env.clone();
        self.spawn(async move {
            for path in jobs {
                if token.is_cancelled() {
                    return;
                }
                let env2 = env.clone();
                let p = path.clone();
                let png = tokio::select! {
                    biased;
                    _ = token.cancelled() => return,
                    out = preview_frame(env2, p) => out,
                };
                if let Some(png) = png {
                    if token.is_cancelled() {
                        return;
                    }
                    let _ = tx.send(SteamEvent::Poster { id: path, png });
                }
            }
        });
    }

    // ── Задача 5: миниатюры карточек фонов ─────────────────────────────

    /// Порт `_ThumbLoader`. `jobs` — пары «id фона → адрес превью» ТОЛЬКО
    /// текущей страницы. Прежняя пачка гасится вместе со своим узлом.
    pub fn start_thumbs(&self, jobs: Vec<(String, String)>) {
        let token = self.swap_node(&self.thumbs);
        let jobs: Vec<(String, String)> = jobs.into_iter().filter(|(_, u)| !u.is_empty()).collect();
        if jobs.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let dir = qs::thumb_cache_dir(self.env.cache(), &self.env.home);
        self.spawn(async move {
            let _ = tokio::fs::create_dir_all(&dir).await;
            // Починка №5: до THUMB_CONCURRENCY разом вместо строго по одной.
            let gate = Arc::new(Semaphore::new(THUMB_CONCURRENCY));
            let mut set = tokio::task::JoinSet::new();
            for (id, url) in jobs {
                let gate = gate.clone();
                let token = token.clone();
                let tx = tx.clone();
                let dir = dir.clone();
                set.spawn(async move {
                    let Ok(_permit) = gate.acquire().await else {
                        return;
                    };
                    if token.is_cancelled() {
                        return;
                    }
                    let dest = format!("{dir}/{}", qs::thumb_cache_file_name(&id, &url));
                    let out = blocking_cancellable(&token, move |cancel| {
                        download_cached(&url, &dest, qs::THUMB_DOWNLOAD_TIMEOUT_S, cancel)
                    })
                    .await;
                    // Одна осечка не рвёт список — как в оригинале.
                    if let Some(Ok(path)) = out {
                        if !token.is_cancelled() {
                            let _ = tx.send(SteamEvent::Thumb { id, path });
                        }
                    }
                });
            }
            // Ждём свои подзадачи здесь: иначе `TaskTracker` считал бы
            // законченной родительскую, а внучки продолжали бы качать.
            while set.join_next().await.is_some() {
                if token.is_cancelled() {
                    set.abort_all();
                }
            }
        });
    }

    // ── Задача 6: полноразмерный фон для крупного просмотра ────────────

    /// Добыть фон целиком для крупного просмотра: картинку (`still_url`) и,
    /// у анимированного, ролик (`movie_url`; пусто — ролика нет).
    ///
    /// Слой загрузки и кэши — те же, что у миниатюр и у «Применить»:
    /// картинка ложится в кэш миниатюр (Steam отдаёт её сразу в полном
    /// размере, отдельного уменьшенного файла у фона нет), ролик — в кэш
    /// фонов, откуда его потом возьмёт и «Применить», не качая второй раз.
    ///
    /// Задача сидит на корневом токене, а не на узле страницы: листание
    /// просмотра не должно рваться оттого, что сетка перешла на другую
    /// страницу. Ответ приходит ВСЕГДА — и при неудаче тоже, иначе просмотр
    /// вечно показывал бы «загружается».
    pub fn start_full(&self, id: String, still_url: String, movie_url: String) {
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let thumb_dir = qs::thumb_cache_dir(self.env.cache(), &self.env.home);
        let bg_dir = qs::bg_cache_dir(self.env.cache(), &self.env.home);
        self.spawn(async move {
            let mut still = String::new();
            let mut movie = String::new();
            if !still_url.is_empty() {
                let dest = format!("{thumb_dir}/{}", qs::thumb_cache_file_name(&id, &still_url));
                let out = blocking_cancellable(&token, move |cancel| {
                    download_cached(&still_url, &dest, qs::THUMB_DOWNLOAD_TIMEOUT_S, cancel)
                })
                .await;
                if let Some(Ok(path)) = out {
                    still = path;
                }
            }
            if !movie_url.is_empty() && !token.is_cancelled() {
                let dest = format!("{bg_dir}/{}", qs::bg_cache_file_name(&id, &movie_url));
                let out = blocking_cancellable(&token, move |cancel| {
                    download_cached(&movie_url, &dest, qs::BG_DOWNLOAD_TIMEOUT_S, cancel)
                })
                .await;
                if let Some(Ok(path)) = out {
                    movie = path;
                }
            }
            if !token.is_cancelled() {
                let _ = tx.send(SteamEvent::Full { id, still, movie });
            }
        });
    }

    // ── Задача 4: скачать фон и применить обоями ───────────────────────

    /// Порт `_ApplyBgWorker`. `false` — уже применяем (оригинальный ответ
    /// «⏳ Уже применяю обои — подожди»).
    ///
    /// Починка №4: у задачи есть отмена. Она сидит на корневом токене
    /// вкладки — применение обоев нельзя рвать сменой страницы, но при
    /// закрытии вкладки результат уже никому не нужен.
    pub fn apply_background(
        &self,
        item: ProfileBackgroundItem,
        audio: bool,
        applier: Arc<dyn WallpaperApply>,
    ) -> bool {
        if self.applying.swap(true, Ordering::SeqCst) {
            return false;
        }
        let token = self.root.child_token();
        let tx = self.tx.clone();
        let busy = self.applying.clone();
        let dir = qs::bg_cache_dir(self.env.cache(), &self.env.home);
        self.spawn(async move {
            let _ = tokio::fs::create_dir_all(&dir).await;
            let dest = format!("{dir}/{}", qs::bg_cache_file_name(&item.id, &item.url));
            let url = item.url.clone();
            let dest2 = dest.clone();
            let downloaded = blocking_cancellable(&token, move |cancel| {
                download_cached(&url, &dest2, qs::BG_DOWNLOAD_TIMEOUT_S, cancel)
            })
            .await;

            let (ok, message) = match downloaded {
                None => {
                    // Вкладку закрыли — молча уходим, никого не тревожа.
                    busy.store(false, Ordering::SeqCst);
                    return;
                }
                Some(Err(e)) => (false, e),
                Some(Ok(path)) => {
                    let fmt = qs::apply_format(&path);
                    let name = item.name.clone();
                    let res = tokio::task::spawn_blocking(move || {
                        applier.apply(&path, fmt, audio, &name)
                    })
                    .await;
                    match res {
                        Ok(v) => v,
                        Err(e) => (false, e.to_string()),
                    }
                }
            };
            busy.store(false, Ordering::SeqCst);
            if !token.is_cancelled() {
                let _ = tx.send(SteamEvent::ApplyDone { ok, message });
            }
        });
    true
    }

    // ── Закрытие ───────────────────────────────────────────────────────

    /// Погасить ВСЕ задачи и дождаться их — порт `closeEvent`.
    ///
    /// Оригинал ждал до 30 секунд, потому что отмены у него не было. Здесь
    /// токен взводится первым, задачи рвут свои запросы сами, а ожидание —
    /// страховка с потолком [`SHUTDOWN_GRACE_MS`].
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

    // ── Внутреннее ─────────────────────────────────────────────────────

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

    /// Сменить узел отмены витрины: прежнюю пачку гасим, новую заводим.
    fn swap_node(&self, slot: &Mutex<Option<CancellationToken>>) -> CancellationToken {
        let fresh = self.root.child_token();
        if let Ok(mut cur) = slot.lock() {
            if let Some(old) = cur.replace(fresh.clone()) {
                old.cancel();
            }
        }
        fresh
    }
}

impl Drop for SteamTasks {
    /// ⚠️ ПОТОЛОК ЗДЕСЬ ОБЯЗАТЕЛЕН, иначе Rexon не закрывается.
    ///
    /// Обычный `Drop` рантайма tokio ждёт блокирующие задачи БЕЗ СРОКА —
    /// ровно поэтому 19.09.2026 самопроверка главного окна не завершалась
    /// сама в 9 прогонах из 12: выгрузка инвентаря Steam застревала в
    /// сетевом чтении, разбор окна упирался в неё, и процесс добивал
    /// сторож. Первопричина того застревания вылечена в ядре
    /// (`rexon_core::net`), но зависимость «одна застрявшая задача = живой
    /// навсегда процесс» на этом никуда не делась, и закрывать её надо
    /// здесь: человек, закрывший окно, обязан получить закрытое окно, чем
    /// бы ни была занята фоновая задача.
    ///
    /// Оставшуюся задачу это бросает как есть — она ничего не портит:
    /// наружу задачи ходят только очередью событий, и отправка в закрытую
    /// очередь просто не доходит.
    fn drop(&mut self) {
        self.root.cancel();
        self.tracker.close();
        if let Some(rt) = self.rt.take() {
            rt.shutdown_timeout(std::time::Duration::from_millis(SHUTDOWN_GRACE_MS));
        }
    }
}

// ============================================================================
// Отмена: мост из токена tokio в токен сетевого слоя ядра
// ============================================================================

/// Выполнить синхронную работу в блокирующем пуле так, чтобы её МОЖНО было
/// оборвать: наружу отдаётся [`net::CancelToken`], который взводится, как
/// только погас `token`.
///
/// `None` — вкладка отменена (результат никому не нужен) либо задача упала.
///
/// Ничего «стимовского» здесь нет: тем же мостом пользуются фоновые задачи
/// страницы «Разработчик» вкладки обоев (`we_update_workers.rs`). Второй
/// копии этой связки в крейте быть не должно (правило #12), поэтому она
/// открыта на весь крейт, а не приватна модулю.
pub(crate) async fn blocking_cancellable<T, F>(token: &CancellationToken, f: F) -> Option<T>
where
    F: FnOnce(net::CancelToken) -> T + Send + 'static,
    T: Send + 'static,
{
    if token.is_cancelled() {
        return None;
    }
    let net_cancel = net::CancelToken::new();
    // Сторож живёт ровно столько, сколько сама работа: как только она
    // закончилась, `done_tx` роняется и сторож уходит. Без этого сторожа
    // копились бы по одному на каждый запрос.
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let nc = net_cancel.clone();
        let t = token.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = t.cancelled() => nc.cancel(),
                _ = done_rx => {}
            }
        });
    }
    let out = tokio::task::spawn_blocking(move || f(net_cancel)).await.ok();
    drop(done_tx);
    if token.is_cancelled() {
        None
    } else {
        out
    }
}

/// Сетевой транспорт ядра с уже подключённой отменой.
fn net_with(cancel: net::CancelToken) -> UreqNet {
    UreqNet::new().with_cancel(cancel)
}

// ============================================================================
// Сеть: инвентарь, владеемые фоны, скачивание ассетов
// ============================================================================

/// Порт `_fetch_inventory_pages`: ВЕСЬ публичный инвентарь одним ответом.
///
/// Страницы идут, пока Steam говорит `more_items` (починка оригинала от
/// 2026-07-28 — прежний код брал одну страницу и терял всё свежее).
fn fetch_inventory_pages(sid: &str, cancel: net::CancelToken) -> Result<Value, String> {
    let mut net = net_with(cancel.clone());
    let mut pages: Vec<Value> = Vec::new();
    let mut start = String::new();
    for _ in 0..qs::INVENTORY_MAX_PAGES {
        if cancel.is_cancelled() {
            return Err(String::new());
        }
        let url = qs::inventory_page_url(sid, &start);
        let body = net
            .get(&url, qs::USER_AGENT, qs::INVENTORY_TIMEOUT_S)
            .map_err(|e| e.human())?;
        let page: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        let next = qs::inventory_next_start(&page);
        pages.push(page);
        match next {
            Some(v) => start = v,
            None => break,
        }
    }
    Ok(qs::merge_inventory_pages(&pages))
}

/// Порт `_fetch_owned` из `rexon/steam_profile_items.py`: владеемые фоны со
/// ссылками на анимацию.
///
/// Отказ различается по роду: «ключ не принят» (ядро перечитает ключ и
/// повторит) и «не ответил». Текст ошибки сети наружу НЕ идёт: в нём адрес
/// запроса, а в адресе — ключ.
fn fetch_owned(
    token: &str,
    cancel: net::CancelToken,
) -> Result<Vec<ProfileBackgroundItem>, qs::OwnedError> {
    let url = format!(
        "https://api.steampowered.com/IPlayerService/GetProfileItemsOwned/v1/\
?access_token={}&language={}",
        percent_encode(token),
        // Язык названий — язык интерфейса (прежде был прибит `russian`).
        qs::steam_language()
    );
    let mut net = net_with(cancel);
    let body = net
        .get(&url, qs::USER_AGENT, qs::INVENTORY_TIMEOUT_S)
        .map_err(|e| {
            qs::owned_error_of_status(match e {
                net::NetError::Status { code, .. } => Some(code),
                net::NetError::Transport { .. } => None,
            })
        })?;
    let data = serde_json::from_slice::<Value>(&body).map_err(|_| qs::OwnedError::NoAnswer)?;
    Ok(steam_profile::parse_owned_backgrounds(&data)
        .into_iter()
        .map(|b| ProfileBackgroundItem {
            id: b.id,
            name: b.name,
            game: b.game,
            url: b.url,
            preview: b.preview,
            animated: b.animated,
            equipped: false,
        })
        .collect())
}

/// Порт `_download_community` + проверки кэша из `_fetch`/`_download`.
///
/// Файл уже на месте и непустой — возвращаем его, не трогая сеть. Иначе
/// качаем, перебирая варианты CDN-хостов (Steam-гниль: один и тот же путь
/// может существовать только на одном из двух).
///
/// Починка №6: временный `.part` снимается на ЛЮБОМ исходе.
pub(crate) fn download_cached(
    url: &str,
    dest: &str,
    timeout: u64,
    cancel: net::CancelToken,
) -> Result<String, String> {
    if std::fs::metadata(dest).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(dest.to_string());
    }
    if let Some(dir) = dest.rsplit_once('/').map(|(d, _)| d) {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = qs::part_path(dest);
    let mut net = net_with(cancel.clone());
    // TRANSLATORS: why a Steam background could not be downloaded: Steam
    // gave no download address for it.
    let mut last = tr("no download address");
    for candidate in spi::community_url_variants(url) {
        if cancel.is_cancelled() {
            let _ = std::fs::remove_file(&tmp);
            return Err(String::new());
        }
        match net.get(&candidate, qs::USER_AGENT, timeout) {
            Ok(body) => {
                if let Err(e) = std::fs::write(&tmp, &body) {
                    last = e.to_string();
                    continue;
                }
                if let Err(e) = std::fs::rename(&tmp, dest) {
                    last = e.to_string();
                    continue;
                }
                return Ok(dest.to_string());
            }
            Err(e) => last = e.human(),
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last)
}

/// `urllib.parse.quote` для токена в адресе.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ============================================================================
// Кадр-превью ролика (ffprobe + ffmpeg)
// ============================================================================

/// Порт `session_splash.preview_frame` для async-задачи.
///
/// Решения — из ядра целиком: каталог кэша, имя файла по «путь + время
/// правки», момент кадра, обе строки запуска и повторная попытка «первый
/// кадр» для коротких/битых роликов. Здесь только запуск процессов.
async fn preview_frame(env: SteamEnv, video: String) -> Option<String> {
    if video.is_empty() || !tokio::fs::try_exists(&video).await.unwrap_or(false) {
        return None;
    }
    let stamp = tokio::fs::metadata(&video)
        .await
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let abs = std::fs::canonicalize(&video)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| video.clone());
    let cache_dir = ss::preview_cache_dir(env.cache(), &env.home);
    let out = ss::preview_cache_path(&cache_dir, &abs, stamp);
    if tokio::fs::metadata(&out)
        .await
        .map(|m| m.len() > 0)
        .unwrap_or(false)
    {
        return Some(out);
    }

    let ffmpeg = which("ffmpeg")?;
    tokio::fs::create_dir_all(&cache_dir).await.ok()?;

    let seek = video_duration(&video).await * ss::PREVIEW_AT;
    let tmp = format!("{out}{}", ss::PREVIEW_TMP_SUFFIX);
    let ok = run_argv(
        &ss::preview_ffmpeg_argv(&ffmpeg, &video, &tmp, seek),
        ss::FFMPEG_TIMEOUT_SEC,
    )
    .await
    .map(|(rc, _)| rc == 0)
    .unwrap_or(false)
        && tokio::fs::try_exists(&tmp).await.unwrap_or(false);
    if !ok {
        // Ролик короче ожидаемого либо битый — берём самый первый кадр.
        let second = run_argv(
            &ss::preview_ffmpeg_argv(&ffmpeg, &video, &tmp, 0.0),
            ss::FFMPEG_TIMEOUT_SEC,
        )
        .await
        .map(|(rc, _)| rc == 0)
        .unwrap_or(false)
            && tokio::fs::try_exists(&tmp).await.unwrap_or(false);
        if !second {
            let _ = tokio::fs::remove_file(&tmp).await;
            return None;
        }
    }
    tokio::fs::rename(&tmp, &out).await.ok()?;
    Some(out)
}

/// Длительность ролика через ffprobe; нет ffprobe — 0 (кадр возьмётся с
/// начала, как и в оригинале).
async fn video_duration(path: &str) -> f64 {
    let Some(exe) = which("ffprobe") else {
        return 0.0;
    };
    match run_argv(
        &ss::ffprobe_duration_argv(&exe, path),
        ss::FFPROBE_TIMEOUT_SEC,
    )
    .await
    {
        Some((_, stdout)) => ss::parse_duration(&stdout),
        None => 0.0,
    }
}

/// `subprocess.run(argv, capture_output=True, timeout=…)` — код возврата и
/// стандартный вывод. `None` — не запустилось либо не уложилось в таймаут.
async fn run_argv(argv: &[String], timeout: f64) -> Option<(i32, String)> {
    let (exe, args) = argv.split_first()?;
    let fut = tokio::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output();
    let out = tokio::time::timeout(std::time::Duration::from_secs_f64(timeout), fut)
        .await
        .ok()?
        .ok()?;
    Some((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// `shutil.which` — первый исполняемый файл с таким именем в `$PATH`.
fn which(name: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(':').filter(|d| !d.is_empty()) {
        let candidate = format!("{dir}/{name}");
        if std::fs::metadata(&candidate).map(|m| m.is_file()).unwrap_or(false) {
            return Some(candidate);
        }
    }
    None
}

// ============================================================================
// Живые чтения файловой системы (сторона исполнителя)
// ============================================================================

/// Порт `_steam_id64`: SteamID64 аккаунта, чей `localconfig.vdf` свежее.
pub fn detect_account(steam_root: &str) -> Option<String> {
    let userdata = format!("{steam_root}/userdata");
    let mut entries: Vec<(String, f64)> = Vec::new();
    for e in std::fs::read_dir(&userdata).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let cfg = format!("{userdata}/{name}/config/localconfig.vdf");
        let Ok(meta) = std::fs::metadata(&cfg) else {
            continue;
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        entries.push((name, mtime));
    }
    qs::pick_account(&entries)
}

/// Порт `_equipped_profile_background`: надетый в профиле фон из
/// `localconfig.vdf` (может быть анимированным).
pub fn read_equipped_background(steam_root: &str) -> Option<ProfileBackgroundItem> {
    let userdata = format!("{steam_root}/userdata");
    for e in std::fs::read_dir(&userdata).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let cfg = format!("{userdata}/{name}/config/localconfig.vdf");
        let Ok(bytes) = std::fs::read(&cfg) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let Some(raw) = qs::extract_equipped_json(&text) else {
            continue;
        };
        let Ok(data) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        if let Some(item) = spi::parse_equipped_background(&data) {
            return Some(item);
        }
    }
    None
}

/// Дисковый кэш СПИСКА инвентаря: показать витрину МГНОВЕННО, пока сеть
/// тихо обновляет её в фоне (stale-while-revalidate оригинала).
pub fn read_inventory_cache(env: &SteamEnv, sid: &str) -> Option<Vec<ProfileBackgroundItem>> {
    let path = qs::inventory_cache_path(env.cache(), &env.home, sid);
    let text = std::fs::read_to_string(path).ok()?;
    let data: Value = serde_json::from_str(&text).ok()?;
    qs::parse_inventory_cache(&data, sid)
}

/// Атомарная запись кэша списка инвентаря (`.tmp` + `rename`).
pub fn write_inventory_cache(env: &SteamEnv, sid: &str, items: &[ProfileBackgroundItem]) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let path = qs::inventory_cache_path(env.cache(), &env.home, sid);
    write_atomic(&path, qs::inventory_cache_json(sid, items, ts).as_bytes());
}

/// Метки видеозаставок из дискового кэша — мгновенно, без сети.
pub fn read_meta_cache(env: &SteamEnv, sid: &str) -> qs::MetaMap {
    let path = qs::splash_meta_cache_path(env.cache(), &env.home, sid);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|v| qs::parse_meta_cache(&v))
        .unwrap_or_default()
}

/// id фона, применённого обоями сейчас (пустая строка — нет такого).
pub fn read_applied_id(env: &SteamEnv) -> String {
    let path = qs::applied_config_path(env.config(), &env.home);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|v| qs::parse_applied_config(&v))
        .unwrap_or_default()
}

/// Запомнить, какой фон применён обоями (движок обоев Steam-предмета не знает).
pub fn write_applied(env: &SteamEnv, id: &str, name: &str) {
    let path = qs::applied_config_path(env.config(), &env.home);
    write_atomic(&path, qs::applied_config_json(id, name).as_bytes());
}

/// Запись через ЕДИНСТВЕННУЮ атомарную дверь профиля
/// ([`rexon_core::utils::atomic_write_bytes`]).
///
/// Своей пары «временный файл + переименование» здесь больше нет: без
/// `fsync` подмена имени может лечь на диск раньше содержимого, и после
/// сбоя питания на месте целой прежней записи оказывается целая новая — из
/// нулей. Сторож общей двери — `rexon-core/tests/profile_writes_atomic.rs`.
fn write_atomic(path: &str, data: &[u8]) {
    if let Some((dir, _)) = path.rsplit_once('/') {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let _ = rexon_core::utils::atomic_write_bytes(path, data);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_at(dir: &std::path::Path) -> SteamEnv {
        SteamEnv {
            home: dir.to_string_lossy().into_owned(),
            xdg_cache_home: Some(format!("{}/cache", dir.to_string_lossy())),
            xdg_config_home: Some(format!("{}/config", dir.to_string_lossy())),
        }
    }

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rexon-steam-workers-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Закрытие вкладки не должно висеть: токен взводится первым, ожидание
    /// упирается в потолок. Оригинал ждал до 30 секунд.
    #[test]
    fn shutdown_is_prompt_and_idempotent() {
        let tasks = SteamTasks::new(SteamEnv::default()).expect("рантайм");
        let started = std::time::Instant::now();
        tasks.shutdown();
        assert!(tasks.is_cancelled());
        assert!(started.elapsed().as_millis() < SHUTDOWN_GRACE_MS as u128 + 500);
        // Повторный вызов безопасен (закрытие вкладки может прийти дважды).
        tasks.shutdown();
    }

    /// После закрытия новые задачи не заводятся: результат некому получать.
    #[test]
    fn no_tasks_after_shutdown() {
        let tasks = SteamTasks::new(SteamEnv::default()).expect("рантайм");
        tasks.shutdown();
        tasks.start_posters(vec!["/несуществующий.webm".into()]);
        tasks.start_thumbs(vec![("1".into(), "https://x/y.jpg".into())]);
        assert_eq!(tasks.poll_event(), None);
    }

    /// Смена страницы гасит прежний узел миниатюр, а не сиротит его
    /// (починка №2).
    #[test]
    fn thumb_node_is_swapped_and_cancelled() {
        let tasks = SteamTasks::new(SteamEnv::default()).expect("рантайм");
        tasks.start_thumbs(vec![("1".into(), "https://invalid.invalid/a.jpg".into())]);
        let first = tasks.thumbs.lock().unwrap().clone().expect("узел заведён");
        tasks.start_thumbs(vec![("2".into(), "https://invalid.invalid/b.jpg".into())]);
        assert!(first.is_cancelled(), "прежний узел обязан быть погашен");
        let second = tasks.thumbs.lock().unwrap().clone().expect("узел заведён");
        assert!(!second.is_cancelled());
        tasks.shutdown();
        assert!(second.is_cancelled(), "закрытие гасит и текущий узел");
    }

    /// Исполнитель обоев один на процесс: пересозданная страница получает
    /// тот же слот, а не свой пустой. Живую машину тест не трогает —
    /// исполнитель в слоте заводится только применением.
    #[test]
    fn wallpaper_slot_is_shared_by_the_process() {
        let a = wallpaper_exec::live_slot();
        let b = ExecApplier::live();
        assert!(Arc::ptr_eq(&a, &b.slot), "страница обязана получить общий слот");
        assert!(Arc::ptr_eq(&a, &ExecApplier::live().slot));
    }

    /// Применение обоев не запускается вторым разом, пока идёт первое.
    #[test]
    fn apply_is_single_flight() {
        struct Slow;
        impl WallpaperApply for Slow {
            fn apply(&self, _p: &str, _f: &str, _a: bool, _n: &str) -> (bool, String) {
                std::thread::sleep(std::time::Duration::from_millis(300));
                (true, String::new())
            }
        }
        let dir = tmpdir("apply");
        let tasks = SteamTasks::new(env_at(&dir)).expect("рантайм");
        let item = ProfileBackgroundItem {
            id: "1".into(),
            name: "Ночь".into(),
            game: String::new(),
            url: "https://invalid.invalid/a.jpg".into(),
            preview: String::new(),
            animated: false,
            equipped: false,
        };
        let a: Arc<dyn WallpaperApply> = Arc::new(Slow);
        assert!(tasks.apply_background(item.clone(), false, a.clone()));
        assert!(
            !tasks.apply_background(item, false, a),
            "второе применение обязано получить отказ"
        );
        tasks.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⭐ Самое вероятное место дефектов: работа УЖЕ идёт, а вкладку
    /// закрывают. Проверяем сам механизм: блокирующая работа получает
    /// сетевой токен, гашение узла взводит его на месте, а результат
    /// наружу не выходит — даже если работа успела досчитать.
    #[test]
    fn inflight_work_is_cut_by_token() {
        let tasks = SteamTasks::new(SteamEnv::default()).expect("рантайм");
        let node = tasks.root.child_token();
        let saw_cancel = Arc::new(AtomicBool::new(false));
        let seen = saw_cancel.clone();
        let node2 = node.clone();
        let out = tasks.rt.as_ref().expect("рантайм жив").block_on(async move {
            let handle = tokio::spawn(async move {
                blocking_cancellable(&node2, move |cancel| {
                    // Ждём, пока сторож взведёт сетевой токен: это ровно
                    // то, что рвёт уже отправленный запрос ureq.
                    for _ in 0..500 {
                        if cancel.is_cancelled() {
                            seen.store(true, Ordering::SeqCst);
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    42u32
                })
                .await
            });
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            node.cancel();
            handle.await.expect("задача не падает")
        });
        assert!(
            saw_cancel.load(Ordering::SeqCst),
            "работа обязана УВИДЕТЬ отмену, а не досчитать до конца"
        );
        assert_eq!(out, None, "результат отменённой работы наружу не идёт");
        tasks.shutdown();
    }

    /// Пересборка витрины гасит прежнюю пачку кадров-превью — тот же
    /// класс, что у миниатюр (починка №2).
    #[test]
    fn poster_node_is_swapped_and_cancelled() {
        let tasks = SteamTasks::new(SteamEnv::default()).expect("рантайм");
        tasks.start_posters(vec!["/нет-такого-1.webm".into()]);
        let first = tasks.posters.lock().unwrap().clone().expect("узел заведён");
        tasks.start_posters(vec!["/нет-такого-2.webm".into()]);
        assert!(first.is_cancelled(), "прежний узел обязан быть погашен");
        tasks.shutdown();
    }

    /// Закрытие вкладки не должно оставлять в очереди события, которые
    /// некому получать: у оригинала именно здесь `done` летел в
    /// разрушаемое окно (починка №4).
    #[test]
    fn no_events_after_shutdown() {
        let tasks = SteamTasks::new(SteamEnv::default()).expect("рантайм");
        tasks.start_thumbs(vec![
            ("1".into(), "https://invalid.invalid/a.jpg".into()),
            ("2".into(), "https://invalid.invalid/b.jpg".into()),
        ]);
        tasks.shutdown();
        // Пока задачи не отменены, они могли успеть положить событие;
        // после закрытия новых не появляется ни одного.
        while tasks.poll_event().is_some() {}
        std::thread::sleep(std::time::Duration::from_millis(120));
        assert_eq!(tasks.poll_event(), None);
    }

    /// Круг «записали кэш → прочитали» на живой файловой системе.
    #[test]
    fn caches_roundtrip_on_disk() {
        let dir = tmpdir("cache");
        let env = env_at(&dir);
        let items = vec![ProfileBackgroundItem {
            id: "1".into(),
            name: "Ночь".into(),
            game: "Portal 2".into(),
            url: "https://cdn/1.jpg".into(),
            preview: String::new(),
            animated: false,
            equipped: true,
        }];
        write_inventory_cache(&env, "765", &items);
        assert_eq!(read_inventory_cache(&env, "765"), Some(items));
        assert_eq!(read_inventory_cache(&env, "999"), None);

        write_applied(&env, "1", "Ночь");
        assert_eq!(read_applied_id(&env), "1");

        assert!(read_meta_cache(&env, "765").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Уже скачанный непустой файл сеть не трогает.
    #[test]
    fn cached_file_short_circuits_download() {
        let dir = tmpdir("dl");
        let dest = format!("{}/a.jpg", dir.to_string_lossy());
        std::fs::write(&dest, b"x").unwrap();
        let out = download_cached("https://invalid.invalid/a.jpg", &dest, 1, net::CancelToken::new());
        assert_eq!(out.as_deref(), Ok(dest.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Починка №6: недокачанный `.part` не остаётся в кэше.
    #[test]
    fn failed_download_leaves_no_part_file() {
        let dir = tmpdir("part");
        let dest = format!("{}/b.jpg", dir.to_string_lossy());
        let cancel = net::CancelToken::new();
        cancel.cancel(); // обрываем ещё до первого обращения
        let out = download_cached("https://invalid.invalid/b.jpg", &dest, 1, cancel);
        assert!(out.is_err());
        assert!(!std::path::Path::new(&qs::part_path(&dest)).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
