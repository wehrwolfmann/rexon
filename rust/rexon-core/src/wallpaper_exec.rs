//! Живой исполнитель планов обоев: то, чем ПЛАН движка становится делом.
//!
//! ЗАЧЕМ. Решения об обоях давно на Rust целиком — [`wallpaper_engine`]
//! умеет собрать план любой операции ([`Step`]-ы `static_plan`,
//! `restore_static_plan`, `animated_plan`, `video_native_plan`,
//! `youtube_plan`, `web_plan`), знает, чем возвращать стол после остановки,
//! и знает, где живые обои рисует не наш процесс, а расширение оболочки.
//! Но ДЕЛАТЬ он не умеет ничего: план — это данные. Исполнителем планов до
//! сих пор был только Python (`WallpaperApplier` в
//! `rexon/wallpaper_engine.py`), и из-за этого объект побочных действий
//! вкладки «Настройки» на Rust честно отвечал «движка нет» на все двенадцать
//! кнопок обоев (`settings_actions.rs`, граница 1).
//!
//! Здесь появляется исполнитель. Разделение прежнее и оно принципиальное —
//! **движок решает, исполнитель делает**:
//!   * ЧТО запускать, в каком порядке, какими словами отвечать — [`wpe`];
//!   * ЧЕМ это делать — шов [`WpSys`] (подпроцессы, файлы, окружение,
//!     часы, мост) и его живая реализация [`LiveSys`];
//!   * КАК прогнать план — [`WallpaperExec::run_plan`].
//!
//! Своих правил здесь нет ни одного. Даже сроки — отсрочка «жив ли
//! запущенный процесс», шаг опроса, ожидание смерти по сигналу, таймауты
//! plasmashell и swww — лежат в [`wpe`] рядом со своим обоснованием, а не
//! числами по коду: ровно на этом уже обжигались, когда у шима завелась
//! своя копия отсрочки.
//!
//! ⚠️ ПОЧЕМУ ШОВ, А НЕ ПРЯМЫЕ ВЫЗОВЫ. Исполнитель обоев меняет РАБОЧИЙ СТОЛ
//! владельца машины. Сверка, которая зовёт его напрямую, снимала бы у
//! человека обои при каждом `cargo test` — поэтому все побочные действия
//! проходят через [`WpSys`]: живьём это [`LiveSys`], в сверках — подставной
//! `FakeSys`, который только записывает, что его просили сделать. Это же
//! даёт возможность проверить порядок шагов и откаты плана без единого
//! запущенного процесса.
//!
//! Образец — живые исполнители, которые уже есть в ядре: [`crate::splash_exec`]
//! (подпроцессы, сокеты, замок, журнал) и [`crate::ext_exec`].

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::session_splash::RunOut;
use crate::system::DesktopShell;
use crate::utils;
use crate::wallpaper_engine as wpe;

// ============================================================================
// Журнал
// ============================================================================

/// Приёмник журнала обоев: `(уровень, сообщение)`.
///
/// Свой, а не общий с заставкой: у Python эти строки уходили в РАЗНЫЕ
/// логгеры (`rexon.wallpaper_engine` и `rexon.session_splash`), и мост,
/// поставив один приёмник на оба, склеил бы два журнала в один.
type LogSink = Box<dyn Fn(&str, &str) + Send + Sync>;

fn log_sink() -> &'static Mutex<Option<LogSink>> {
    static SINK: OnceLock<Mutex<Option<LogSink>>> = OnceLock::new();
    SINK.get_or_init(|| Mutex::new(None))
}

/// Поставить приёмник журнала (мост направляет его в `rexon.wallpaper_engine`).
pub fn set_log_sink(sink: impl Fn(&str, &str) + Send + Sync + 'static) {
    if let Ok(mut slot) = log_sink().lock() {
        *slot = Some(Box::new(sink));
    }
}

fn log_line(level: &str, message: &str) {
    if let Ok(slot) = log_sink().lock() {
        if let Some(sink) = slot.as_ref() {
            sink(level, message);
            return;
        }
    }
    crate::applog::write(crate::applog::level_from_name(level), "wallpaper", message);
}

// ============================================================================
// Итог операции
// ============================================================================

/// Итог операции с обоями — порт `WallpaperResult`.
///
/// Сообщение всегда непустое: молчаливый отказ покупатель читает как
/// поломку кнопки (правило #13), поэтому пустых исходов у исполнителя нет.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallpaperResult {
    /// Получилось ли.
    pub ok: bool,
    /// Что сказать человеку.
    pub message: String,
    /// PID поднятого процесса обоев, если он поднимался.
    pub pid: Option<i64>,
}

impl WallpaperResult {
    /// Удача с сообщением.
    pub fn ok(message: impl Into<String>) -> Self {
        WallpaperResult { ok: true, message: message.into(), pid: None }
    }

    /// Отказ с сообщением.
    pub fn fail(message: impl Into<String>) -> Self {
        WallpaperResult { ok: false, message: message.into(), pid: None }
    }
}

/// ⚠️ ГРАНИЦА ИСПОЛНИТЕЛЯ, НАЗВАННАЯ ВСЛУХ.
///
/// Не перенесён один формат — Flash (SWF): у исполнителя нет пути Ruffle
/// (для него строится отдельный хост). Молчать об этом нельзя (правило
/// #13): молчаливое «ничего не произошло» покупатель читает как поломку
/// кнопки, поэтому исполнитель ОТВЕЧАЕТ этой строкой, а не делает вид, что
/// обои встали.
pub fn live_formats_not_carried() -> String {
    crate::i18n::tr("this kind of live wallpaper is still shown by the old engine")
}

/// Носит ли исполнитель этот формат обоев сам — то есть применит ли его
/// [`WallpaperExec::apply`], а не ответит границей [`live_formats_not_carried`].
///
/// Правило одно на продукт и живёт здесь, рядом с самой границей: окно,
/// которое решает, звать ли исполнитель или отослать человека в классическое
/// окно, спрашивает его, а не держит свою копию списка. Иначе перенос
/// следующего формата сюда молча не доехал бы до кнопки.
///
/// Сегодня исполнитель носит ВСЕ форматы: картинку, GIF, видео, YouTube,
/// веб и — последним переехавший — Flash (SWF), который показывает режим
/// `rexon-wallpaper-host --mode swf` (Ruffle как библиотека `rexon-swf`).
/// «Носит» значит и «отвечает окончательно»: сцены, обои-приложения и
/// незнакомый формат исполнитель отклоняет теми же словами, что и
/// классическое окно. Функция остаётся точкой решения: если формат когда-то
/// снова окажется только у классического окна, это объявляется здесь, и
/// кнопки узнают об этом сами.
pub fn carries_format(_fmt: &str) -> bool {
    true
}

/// Окончательный отказ для формата, которого у Rexon нет вовсе, — те же
/// слова, что у Python (`apply`: `scene_unsupported`,
/// `application_unsupported`, `unknown_format`). `None` — формат известен и
/// применяется.
fn retired_format_refusal(fmt: &str) -> Option<String> {
    match fmt {
        "static" | "animated" | "video" | "youtube" | "web" | "swf" => None,
        "scene" => Some(wpe::scene_unsupported()),
        "application" => Some(wpe::application_unsupported()),
        other => Some(wpe::unknown_format_message(other)),
    }
}

/// Громкость потока YouTube, % — у Python `volume` по умолчанию 50: поток
/// играет mpv/mpvpaper, до которого надзиратель звука хоста не дотягивается.
const YOUTUBE_STREAM_VOLUME: i64 = 50;

/// Звук видео и YouTube, когда вызывающий выбора не даёт, — умолчание
/// Python (`video_has_audio = True`). Громкость и «звук вкл/выкл» при этом
/// живут в `settings.json` (`wallpaper_audio`) и действуют на ходу.
pub const VIDEO_AUDIO_DEFAULT: bool = true;

/// Нужен ли живым обоям этого формата надзор за нагрузкой: видео и YouTube
/// (их хост декодирует кадры всё время работы) и Flash (Ruffle рисует кадр за
/// кадром). Пауза у Flash та же, что у видео, — SIGSTOP/SIGCONT процесса
/// хоста; после продолжения ролик не проматывается: шаг его времени
/// ограничен в `rexon-swf` (`Settings::dt_cap`).
pub fn format_wants_load_monitor(fmt: Option<&str>) -> bool {
    matches!(fmt, Some("video") | Some("youtube") | Some("swf"))
}

/// Сила размытия подложки под живыми обоями — умолчание Python
/// (`blur_radius = 20`).
const BLUR_BACKDROP_RADIUS: i64 = 20;

/// Проверки файловой системы для поиска наших хостов — те же, что у Python
/// `ExecFsProbe`: исполняемый файл на месте и поиск по `PATH`.
struct ExecProbe;

impl crate::we_client::DaemonFs for ExecProbe {
    fn is_exec_file(&mut self, path: &str) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    fn which(&mut self, name: &str) -> Option<String> {
        utils::which(name, None)
    }
}

/// Один фоновый звук: отдельный процесс mpv на каждый.
#[derive(Debug, Clone)]
struct Ambient {
    id: String,
    path: String,
    name: String,
    volume: i64,
    /// Зацикленный. Разовый звук — законный случай, а не сбой: расширение
    /// задаёт режим само, и от режима зависит, чем мерить успех запуска.
    looped: bool,
    proc: ProcHandle,
}

/// Играющий фоновый звук — то, что видит вкладка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbientInfo {
    /// Идентификатор звука.
    pub id: String,
    /// Название для человека.
    pub name: String,
    /// Громкость, проценты.
    pub volume: i64,
    /// Играет ли.
    pub active: bool,
}

/// Снимок состояния движка обоев.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Идут ли живые обои прямо сейчас.
    pub running: bool,
    /// Формат того, что играет.
    pub format: Option<String>,
    /// Путь того, что играет.
    pub path: Option<String>,
    /// Опознанное окружение.
    pub de: String,
    /// Опознанный тип сессии.
    pub session_type: String,
    /// PID процесса обоев, пока он жив.
    pub pid: Option<i64>,
    /// Стоит ли размытая подложка.
    pub blur_active: bool,
    /// Играющие фоновые звуки.
    pub ambient: Vec<AmbientInfo>,
}

// ============================================================================
// Шов побочных действий
// ============================================================================

/// Дескриптор поднятого процесса — не PID: PID переиспользуется системой, а
/// исполнитель обязан снимать ТОЛЬКО свой процесс и никогда чужой по имени.
pub type ProcHandle = i64;

/// Всё, чем исполнитель трогает живую машину.
///
/// Ни одно решение сюда не переехало: реализации только делают то, что им
/// сказали. Живая — [`LiveSys`], подставная — в сверках этого модуля.
pub trait WpSys {
    /// Запустить команду и дождаться её. `None` — команды нет или вышло
    /// время: тот же безопасный дефолт, что давал Python на `OSError`.
    fn run(&mut self, argv: &[String], timeout: f64) -> Option<RunOut>;

    /// Поднять процесс. `new_session` — свой сеанс (`start_new_session`),
    /// чтобы внуки (mpv/ffmpeg) снимались вместе с родителем.
    fn spawn(&mut self, argv: &[String], new_session: bool) -> Result<ProcHandle, String>;

    /// Код возврата, если процесс уже завершился; `None` — жив.
    fn poll(&mut self, proc: ProcHandle) -> Option<i64>;

    /// PID процесса — для сообщения `{pid}` и PID-файла.
    fn pid(&mut self, proc: ProcHandle) -> Option<i64>;

    /// Снять процесс: SIGTERM группе → ждать → SIGKILL группе → ждать.
    fn kill_process(&mut self, proc: ProcHandle);

    /// Есть ли такая программа в системе.
    fn has_tool(&mut self, tool: &str) -> bool;

    /// Подсказка об имени пакета для текущего дистрибутива (или пусто).
    fn package_hint(&mut self, tool: &str) -> String;

    /// Файл или каталог на месте.
    fn exists(&mut self, path: &str) -> bool;

    /// Это каталог.
    fn is_dir(&mut self, path: &str) -> bool;

    /// Отсортированное содержимое каталога (имена без пути).
    fn listdir(&mut self, path: &str) -> Vec<String>;

    /// Текст файла или `None`.
    fn read_text(&mut self, path: &str) -> Option<String>;

    /// Записать текст, создав каталоги. Неудача журналируется, не рушит.
    fn write_text(&mut self, path: &str, content: &str);

    /// Удалить файл, если он есть.
    fn remove_file(&mut self, path: &str);

    /// Переменная окружения или пустая строка.
    fn env(&mut self, name: &str) -> String;

    /// Монотонные часы, секунды.
    fn monotonic(&mut self) -> f64;

    /// Подождать.
    fn sleep(&mut self, seconds: f64);

    /// Сказать мосту, идут ли живые обои (`announce` — ещё и событием).
    fn bridge_notify(&mut self, live: bool, name: &str, announce: bool);

    /// Размыть картинку `source` и записать JPEG в `output`. По умолчанию —
    /// отказ: подставные швы размывать не обязаны.
    fn blur_image(&mut self, _source: &str, _output: &str, _radius: i64) -> Result<(), String> {
        Err("размытие этим швом не поддерживается".to_string())
    }

    /// Заморозить (`pause`) либо разморозить процесс обоев — SIGSTOP/SIGCONT
    /// ему самому, как у Python. По умолчанию — ничего.
    fn pause_process(&mut self, _proc: ProcHandle, _pause: bool) {}

    /// Собранный хост обоев (`rexon-wallpaper-host`, режимы видео и веб)
    /// либо `None` — его нет нигде. Порядок поиска держит движок
    /// (`wallpaper_engine::find_wallpaper_host_binary`).
    fn wallpaper_host(&mut self) -> Option<String> {
        self.video_host()
    }

    /// ⚠️ Переходное имя [`WpSys::wallpaper_host`] с тех пор, как хостов
    /// было два. Им ещё отвечает тестовая подстановка в
    /// `rexon-gui/src/settings_actions.rs` (файл правит соседняя работа);
    /// своего поиска за ним нет, исполнитель спрашивает только
    /// `wallpaper_host`. Снять вместе с переходом той подстановки.
    fn video_host(&mut self) -> Option<String> {
        None
    }

    /// Размер файла в байтах либо `None`.
    fn file_size(&mut self, _path: &str) -> Option<u64> {
        None
    }

    /// Путь к faststart-версии видео (moov в начале) — порт
    /// `_ensure_faststart`. По умолчанию — исходный путь.
    fn ensure_faststart(&mut self, path: &str) -> String {
        path.to_string()
    }

    /// Достать кадр из видео (картинку — скопировать) в `output`. Истина —
    /// кадр на месте. По умолчанию — нет.
    fn extract_frame(&mut self, _source: &str, _output: &str, _timestamp: f64) -> bool {
        false
    }

    /// Скачать ролик YouTube в кеш: путь к файлу либо `None`. По умолчанию
    /// — нет.
    fn download_youtube(&mut self, _url: &str, _quality: &str) -> Option<String> {
        None
    }

    /// Чужой шелл (Noctalia, DankMaterialShell), который сейчас держит слой
    /// обоев в этом окружении: его поверхность легла бы ПОВЕРХ наших обоев.
    /// По умолчанию — никого: подставные швы живой сеанс не видят. Решение
    /// «чей фон» принимает ядро (`wallpaper_engine::wallpaper_layer_owner`),
    /// живая реализация только добывает наблюдения.
    fn wallpaper_shell_owner(&mut self, _de: &str, _session_type: &str) -> Option<DesktopShell> {
        None
    }

    /// Синхронно снять живые обои Wallpaper Engine (рендереры демона
    /// waywallen): `(статус, сколько снято)` в словах
    /// [`crate::we_client::stop_active_result`]. `None` — снимать некому:
    /// процесс без Qt (ядро, CLI) крюк не ставил.
    ///
    /// По умолчанию — крюк процесса ([`set_we_stopper`]): сокет к демону
    /// живёт в Qt, и ставит его окно (`rexon-gui`). Подставные швы его не
    /// видят, пока не переопределят метод.
    fn stop_we_wallpapers(&mut self, timeout_ms: u64) -> Option<(String, i64)> {
        we_stopper().map(|stop| stop(timeout_ms))
    }

    /// Rexon поставил обоями стола картинку `path` — следом пересчитать
    /// «Цвет акцента из обоев» вне Plasma ([`crate::accent::follow_wallpaper`]).
    /// По умолчанию ничего: подставные швы настоящих настроек не трогают.
    fn static_wallpaper_changed(&mut self, _path: &str) {}
}

// ============================================================================
// Живые обои Wallpaper Engine под своими
// ============================================================================
//
// WE-обои рисует демон waywallen своими рендерерами — поверх стола, где бы он
// ни показывал (плагин Plasma, layer-shell, расширения GNOME/Cinnamon). Свои
// обои Rexon, поставленные при живой WE-обое, оказываются ПОД её поверхностью:
// человек жмёт «Применить», а видит прежнюю картинку (правило #13). Поэтому
// любые свои обои сначала снимают WE-обои — одним запросом к демону
// (`renderer_list` → `renderer_kill` каждому), без разбора, какой стол их
// показывает.

/// Крюк «снять живые WE-обои»: таймаут в мс → `(статус, сколько снято)`.
pub type WeStopper = fn(u64) -> (String, i64);

fn we_stopper_slot() -> &'static Mutex<Option<WeStopper>> {
    static SLOT: Mutex<Option<WeStopper>> = Mutex::new(None);
    &SLOT
}

/// Поставить крюк снятия WE-обоев (зовёт окно на Qt один раз при старте).
pub fn set_we_stopper(stop: WeStopper) {
    if let Ok(mut slot) = we_stopper_slot().lock() {
        *slot = Some(stop);
    }
}

fn we_stopper() -> Option<WeStopper> {
    we_stopper_slot().lock().ok().and_then(|slot| *slot)
}

/// Снимают ли обои этого формата живые WE-обои перед собой. Да — все свои
/// форматы Rexon: каждый ложится на тот же слой стола, что и WE-обоя.
pub fn yields_we_wallpapers(fmt: &str) -> bool {
    matches!(fmt, "static" | "animated" | "video" | "youtube" | "web" | "swf")
}

/// Что сказать человеку после попытки снять WE-обои: `None` — ничего
/// (сняли, снимать было нечего, демона нет); `Some` — приписка к итогу:
/// свои обои встали, но WE-обоя осталась поверх.
pub fn we_yield_note(status: &str) -> Option<String> {
    (status == crate::we_client::STOP_ERROR).then(|| {
        crate::i18n::tr(
            "⚠ The live Wallpaper Engine wallpaper could not be stopped — stop it in \
             the Wallpaper Engine tab to see this one.",
        )
    })
}

// ============================================================================
// Значки рабочего стола X11 — швы исполнителя для решений ядра
// ============================================================================

/// Подготовка и откат значков стола (`wallpaper_backend::prepare_desktop_icons`,
/// `restore_desktop_icons`) через тот же шов [`WpSys`], что у исполнителя:
/// один живой путь и одна подстановка в сверках.
pub struct DesktopIconsSys<'a, S: WpSys> {
    sys: &'a mut S,
    state_path: String,
}

impl<'a, S: WpSys> DesktopIconsSys<'a, S> {
    pub fn new(sys: &'a mut S) -> Self {
        let state_path = crate::wallpaper_backend::desktop_icons_state_path(&sys.env("XDG_STATE_HOME"), &sys.env("HOME"));
        Self { sys, state_path }
    }
}

impl<S: WpSys> crate::wallpaper_backend::DesktopIconsIo for DesktopIconsSys<'_, S> {
    fn run(&mut self, argv: &[&str]) -> Option<String> {
        let argv: Vec<String> = argv.iter().map(|a| a.to_string()).collect();
        match self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC) {
            Some(out) if out.rc == 0 => Some(out.stdout),
            _ => None,
        }
    }

    fn read_state(&mut self) -> Option<String> {
        self.sys.read_text(&self.state_path)
    }

    fn write_state(&mut self, content: Option<&str>) -> bool {
        match content {
            Some(text) => self.sys.write_text(&self.state_path, text),
            None => self.sys.remove_file(&self.state_path),
        }
        true
    }

    fn pause_ms(&mut self, ms: u64) {
        self.sys.sleep(ms as f64 / 1000.0);
    }
}

// ============================================================================
// Живая реализация шва
// ============================================================================

/// Побочные действия на настоящей машине.
pub struct LiveSys {
    procs: HashMap<ProcHandle, Child>,
    next: ProcHandle,
    epoch: Instant,
    family: String,
}

impl Default for LiveSys {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveSys {
    /// Новый исполнитель побочных действий.
    pub fn new() -> LiveSys {
        LiveSys {
            procs: HashMap::new(),
            next: 1,
            epoch: Instant::now(),
            family: String::new(),
        }
    }

    fn handle(&mut self) -> ProcHandle {
        let h = self.next;
        self.next += 1;
        h
    }

    /// Семейство пакетного менеджера — спрашивается ОДИН раз на процесс:
    /// подсказку про пакет движок просит на каждый план, а `/etc/os-release`
    /// за время работы не меняется.
    fn family(&mut self) -> String {
        if self.family.is_empty() {
            self.family = crate::system::DistroDetector::detect_live()
                .family_live()
                .as_str()
                .to_string();
        }
        self.family.clone()
    }

    /// Послать сигнал всей группе процесса. `false` — группы нет или это
    /// НАША группа (самоубийство недопустимо).
    fn signal_group(pid: libc::pid_t, sig: libc::c_int) -> bool {
        let pgid = unsafe { libc::getpgid(pid) };
        if pgid <= 0 {
            return false;
        }
        if pgid == unsafe { libc::getpgrp() } {
            return false;
        }
        unsafe { libc::killpg(pgid, sig) == 0 }
    }

    /// Дождаться завершения ребёнка не дольше срока. `true` — дождались.
    fn wait_child(child: &mut Child, seconds: f64) -> bool {
        let deadline = Instant::now() + Duration::from_secs_f64(seconds.max(0.0));
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => {}
                Err(_) => return false,
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_secs_f64(wpe::ALIVE_POLL_STEP_SEC));
        }
    }
}

impl WpSys for LiveSys {
    fn static_wallpaper_changed(&mut self, path: &str) {
        crate::accent::live_follow_wallpaper(path);
    }

    fn run(&mut self, argv: &[String], timeout: f64) -> Option<RunOut> {
        crate::splash_exec::run_capture(argv, timeout)
    }

    fn spawn(&mut self, argv: &[String], new_session: bool) -> Result<ProcHandle, String> {
        let Some((bin, rest)) = argv.split_first() else {
            return Err(crate::i18n::tr("empty command"));
        };
        let mut cmd = Command::new(bin);
        cmd.args(rest)
            .stdin(Stdio::null())
            // Вывод процесса обоев намеренно не читается — почему, сказано
            // у `wpe::spawn_died_message`: труба на живых обоях переполнилась
            // бы и заткнула хост.
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if new_session {
            unsafe {
                use std::os::unix::process::CommandExt;
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        match cmd.spawn() {
            Ok(child) => {
                let h = self.handle();
                self.procs.insert(h, child);
                Ok(h)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn poll(&mut self, proc: ProcHandle) -> Option<i64> {
        let child = self.procs.get_mut(&proc)?;
        match child.try_wait() {
            Ok(Some(st)) => Some(st.code().unwrap_or(-1) as i64),
            _ => None,
        }
    }

    fn pid(&mut self, proc: ProcHandle) -> Option<i64> {
        self.procs.get(&proc).map(|c| c.id() as i64)
    }

    fn kill_process(&mut self, proc: ProcHandle) {
        let Some(mut child) = self.procs.remove(&proc) else {
            return;
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            return; // уже умер сам — статус забран, больше ничего не надо
        }
        let pid = child.id() as libc::pid_t;
        if !LiveSys::signal_group(pid, libc::SIGTERM) {
            unsafe { libc::kill(pid, libc::SIGTERM) };
        }
        if LiveSys::wait_child(&mut child, wpe::KILL_WAIT_SEC) {
            return;
        }
        log_line("warning", &format!("процесс обоев PID={pid} не ушёл по SIGTERM — SIGKILL"));
        if !LiveSys::signal_group(pid, libc::SIGKILL) {
            let _ = child.kill();
        }
        if !LiveSys::wait_child(&mut child, wpe::KILL_WAIT_SEC) {
            log_line("error", &format!("процесс обоев PID={pid} не умирает даже по SIGKILL"));
        }
    }

    fn has_tool(&mut self, tool: &str) -> bool {
        utils::resolve_tool(tool, &[], std::env::var("PATH").ok().as_deref()).is_some()
    }

    fn package_hint(&mut self, tool: &str) -> String {
        let family = self.family();
        utils::tool_package_hint(tool, &family)
    }

    fn exists(&mut self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn is_dir(&mut self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(path) else {
            return Vec::new();
        };
        // Порядок отсортированный: у `os.listdir` он был порядком ФС, и
        // «первый попавшийся пакет обоев» менялся от машины к машине.
        let mut out: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        out.sort();
        out
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        std::fs::read(path)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    fn write_text(&mut self, path: &str, content: &str) {
        if let Some(dir) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = crate::utils::atomic_write(path, content) {
            log_line("debug", &format!("не записался {path}: {e}"));
        }
    }

    fn remove_file(&mut self, path: &str) {
        if std::path::Path::new(path).is_file() {
            let _ = std::fs::remove_file(path);
        }
    }

    fn env(&mut self, name: &str) -> String {
        std::env::var(name).unwrap_or_default()
    }

    fn monotonic(&mut self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    fn sleep(&mut self, seconds: f64) {
        std::thread::sleep(Duration::from_secs_f64(seconds.max(0.0)));
    }

    fn blur_image(&mut self, source: &str, output: &str, radius: i64) -> Result<(), String> {
        wpe::blur_image_file(source, output, radius)
    }

    fn wallpaper_host(&mut self) -> Option<String> {
        wpe::find_wallpaper_host_binary(&mut ExecProbe, &crate::cli::repo_root())
    }

    fn file_size(&mut self, path: &str) -> Option<u64> {
        std::fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| m.len())
    }

    fn ensure_faststart(&mut self, path: &str) -> String {
        use std::io::Read;
        let (_, ext) = wpe::split_ext(path);
        if !wpe::FASTSTART_EXTS.contains(&ext.to_lowercase().as_str()) {
            return path.to_string();
        }
        let mut header = vec![0u8; wpe::MOOV_HEADER_BYTES];
        let read = std::fs::File::open(path).and_then(|mut f| f.read(&mut header));
        let Ok(n) = read else {
            return path.to_string(); // не прочитался — файл не трогаем
        };
        header.truncate(n);
        if wpe::moov_at_front(&header) {
            return path.to_string();
        }
        if utils::which(wpe::FFMPEG_BIN, None).is_none() {
            log_line("warning", &format!("нет ffmpeg — видео без faststart может играть чёрным: {path}"));
            return path.to_string();
        }
        let Ok(meta) = std::fs::metadata(path) else {
            return path.to_string();
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let abs = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());
        let key = wpe::faststart_key(&abs, meta.len(), mtime);
        let dir = format!("{}/rexon/faststart", cache_home_live());
        if std::fs::create_dir_all(&dir).is_err() {
            return path.to_string();
        }
        let out = format!("{dir}/{key}.mp4");
        let ready = |p: &str| std::fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false);
        if ready(&out) {
            return out;
        }
        log_line("info", &format!("видео без faststart — переношу moov в начало: {path}"));
        let argv = wpe::faststart_argv(path, &out);
        match crate::splash_exec::run_capture(&argv, wpe::FASTSTART_TIMEOUT_SEC as f64) {
            Some(r) if r.rc == 0 && ready(&out) => out,
            _ => {
                log_line("warning", "faststart-ремукс не удался — играю исходный файл");
                path.to_string()
            }
        }
    }

    fn download_youtube(&mut self, url: &str, quality: &str) -> Option<String> {
        crate::youtube_wallpaper::download_live(url, quality)
    }

    fn wallpaper_shell_owner(&mut self, de: &str, session_type: &str) -> Option<DesktopShell> {
        use crate::desktop_shell::{shell_wallpaper_state, LiveFacts, ShellHome};
        let detection = LiveFacts::collect().detect();
        shell_wallpaper_state(de, session_type, detection, &ShellHome::live()).owner.shell()
    }

    fn extract_frame(&mut self, source: &str, output: &str, timestamp: f64) -> bool {
        if let Some(dir) = std::path::Path::new(output).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let (_, ext) = wpe::split_ext(source);
        if wpe::IMAGE_EXTS.contains(&ext.to_lowercase().as_str()) {
            // Картинку берём копированием — ffmpeg тут не нужен.
            return std::fs::copy(source, output).is_ok();
        }
        if utils::which(wpe::FFMPEG_BIN, None).is_none() {
            return false;
        }
        let argv = wpe::extract_frame_argv(source, output, timestamp);
        matches!(crate::splash_exec::run_capture(&argv, 15.0), Some(r) if r.rc == 0)
            && std::path::Path::new(output).is_file()
    }

    fn pause_process(&mut self, proc: ProcHandle, pause: bool) {
        let Some(child) = self.procs.get_mut(&proc) else {
            return;
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            return; // уже завершился — сигналить некому
        }
        let pid = child.id() as libc::pid_t;
        let sig = if pause { libc::SIGSTOP } else { libc::SIGCONT };
        unsafe { libc::kill(pid, sig) };
    }

    fn bridge_notify(&mut self, live: bool, name: &str, announce: bool) {
        let bridge = crate::rexon_bridge::RexonBridge::new_default();
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        // Метку времени печатает ядро (`engine_update::now_iso`) — второй
        // копии календаря в дереве быть не должно.
        let now = crate::engine_update::now_iso(since.as_secs() as i64);
        let epoch_ms = since.as_millis() as u64;
        if let Err(e) = bridge.notify_video_wallpaper(live, name, announce, &now, epoch_ms) {
            log_line("debug", &format!("мост не принял весть об обоях: {e}"));
        }
    }
}

// ============================================================================
// Выходы (мониторы): один опрос на весь прогон плана
// ============================================================================

/// Один выход стола: как он зовётся, какое у него разрешение и где он
/// стоит.
///
/// Имя нужно движкам, умеющим адресовать выход (`swww --outputs`,
/// `mpvpaper <выход>`), разрешение с положением — тем, кто просит геометрию
/// окна (`xwinwrap -g ШхВ+X+Y`): без смещения окно второго монитора легло
/// бы поверх первого. Все три величины приходят из ОДНОГО опроса: раньше
/// имя брал `wlr-randr`, а разрешение — отдельный `xrandr`, и на одной и
/// той же машине они могли говорить о РАЗНЫХ мониторах.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenOutput {
    /// Имя выхода (`DP-1`, `eDP-1`, `HDMI-A-1`).
    pub name: String,
    /// Разрешение выхода, `ШхВ`.
    pub resolution: String,
    /// Где выход стоит на столе, `(X, Y)` в точках. `None` — опрос не
    /// сказал: такое бывает у урезанного вывода и чужих форм ответа.
    pub position: Option<(i64, i64)>,
}

impl ScreenOutput {
    /// Геометрия окна на этом выходе, `ШхВ+X+Y` — то, что просит
    /// `xwinwrap -g`.
    ///
    /// ⚠️ Положение неизвестно — смещение нулевое: получается ровно та
    /// геометрия, что стояла в команде плана до раскладки по мониторам, и
    /// обои встают как раньше, а не перестают ставиться.
    pub fn geometry(&self) -> String {
        let (x, y) = self.position.unwrap_or((0, 0));
        format!("{}+{x}+{y}", self.resolution)
    }

    /// Размер выхода в точках. `None` — разрешение не разобралось (урезанный
    /// вывод, чужая форма ответа): такой выход не участвует в сравнении
    /// размеров, а не считается нулевым.
    pub fn size(&self) -> Option<(u64, u64)> {
        let (w, h) = self.resolution.split_once(['x', 'X'])?;
        let w: u64 = w.trim().parse().ok()?;
        let h: u64 = h.trim().parse().ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        Some((w, h))
    }
}

/// Имя выхода, когда опросить систему не удалось: «все» на языке движков.
const ANY_OUTPUT: &str = "*";

/// Разрешение, когда опросить систему не удалось.
const ANY_RESOLUTION: &str = "1920x1080";

/// Выходы из JSON `wlr-randr --json` или `hyprctl monitors -j`.
///
/// Формы разные, а нужное в обеих есть: у `wlr-randr` размер лежит в
/// текущем режиме (`modes[i].current`), у `hyprctl` — прямо в мониторе
/// (`width`/`height`). Выключенные выходы пропускаем: обои на погашенный
/// монитор не ставятся.
pub fn parse_outputs_json(stdout: &str) -> Vec<ScreenOutput> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        // `wlr-randr`: enabled=false; `hyprctl`: disabled=true.
        if item.get("enabled").and_then(|v| v.as_bool()) == Some(false)
            || item.get("disabled").and_then(|v| v.as_bool()) == Some(true)
        {
            continue;
        }
        let name = item
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let size = |v: &serde_json::Value, key: &str| -> Option<i64> {
            v.get(key).and_then(|n| n.as_i64())
        };
        let mut resolution = match (size(item, "width"), size(item, "height")) {
            (Some(w), Some(h)) if w > 0 && h > 0 => format!("{w}x{h}"),
            _ => String::new(),
        };
        if resolution.is_empty() {
            if let Some(modes) = item.get("modes").and_then(|m| m.as_array()) {
                let current = modes
                    .iter()
                    .find(|m| m.get("current").and_then(|c| c.as_bool()) == Some(true))
                    .or_else(|| modes.first());
                if let Some(m) = current {
                    if let (Some(w), Some(h)) = (size(m, "width"), size(m, "height")) {
                        if w > 0 && h > 0 {
                            resolution = format!("{w}x{h}");
                        }
                    }
                }
            }
        }
        if resolution.is_empty() {
            resolution = ANY_RESOLUTION.to_string();
        }
        // Положение: у `wlr-randr` оно вложено (`position.x`), у `hyprctl`
        // лежит прямо в мониторе (`x`/`y`). Нет ни того, ни другого —
        // `None`, и геометрия выйдет с нулевым смещением.
        let at = |v: &serde_json::Value, key: &str| -> Option<i64> {
            v.get(key).and_then(|n| n.as_i64())
        };
        let position = match item.get("position") {
            Some(pos) if pos.is_object() => match (at(pos, "x"), at(pos, "y")) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            },
            _ => match (at(item, "x"), at(item, "y")) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            },
        };
        out.push(ScreenOutput { name, resolution, position });
    }
    out
}

/// Выходы из `xrandr --current`.
///
/// Берём строки подключённых выходов (`DP-1 connected 1920x1080+0+0 …`):
/// имя — первое слово, разрешение — размер до первого `+`. Режимы
/// перечислены ниже отдельными строками, и в них имени нет вовсе — поэтому,
/// если ни одной строки `connected` не нашлось (урезанный вывод, чужая
/// локаль), отступаем к прежнему правилу «строка с активным режимом» и
/// отдаём один безымянный выход.
pub fn parse_xrandr_outputs(stdout: &str) -> Vec<ScreenOutput> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        if line.starts_with(char::is_whitespace) {
            continue; // строка режима, а не выхода
        }
        let mut words = line.split_whitespace();
        let Some(name) = words.next() else { continue };
        if words.next() != Some("connected") {
            continue;
        }
        let geometry = line
            .split_whitespace()
            .find(|w| w.contains('+') && w.contains('x'));
        let resolution = geometry
            .and_then(|g| g.split('+').next())
            .filter(|r| r.contains('x'))
            .unwrap_or(ANY_RESOLUTION)
            .to_string();
        // Смещение выхода печатается той же строкой, что и размер
        // (`1920x1080+1920+0`), — берём его оттуда же, вторым и третьим
        // куском.
        let position = geometry.and_then(|g| {
            let mut parts = g.split('+').skip(1);
            match (parts.next(), parts.next()) {
                (Some(x), Some(y)) => match (x.parse::<i64>(), y.parse::<i64>()) {
                    (Ok(x), Ok(y)) => Some((x, y)),
                    _ => None,
                },
                _ => None,
            }
        });
        out.push(ScreenOutput { name: name.to_string(), resolution, position });
    }
    if out.is_empty() {
        if let Some(res) = wpe::parse_xrandr_resolution(stdout) {
            out.push(ScreenOutput {
                name: ANY_OUTPUT.to_string(),
                resolution: res,
                position: None,
            });
        }
    }
    out
}

/// Чем спрашивать выходы: команда и разбор её вывода.
///
/// Порядок зависит от сеанса — под X11 первым отвечает `xrandr`, под
/// Wayland его может не быть вовсе (или он покажет один XWayland-экран).
fn output_probes(session_type: &str) -> Vec<Vec<String>> {
    let wlr = vec!["wlr-randr".to_string(), "--json".to_string()];
    let hypr = vec![
        "hyprctl".to_string(),
        "monitors".to_string(),
        "-j".to_string(),
    ];
    let xr = vec!["xrandr".to_string(), "--current".to_string()];
    if session_type == "x11" {
        vec![xr, wlr, hypr]
    } else {
        vec![wlr, hypr, xr]
    }
}

/// ЕДИНСТВЕННЫЙ источник списка выходов.
///
/// Раньше правило было размазано: имя монитора добывал свой обход
/// `wlr-randr`/`hyprctl`, разрешение — свой вызов `xrandr`, и оба брали
/// ПЕРВОЕ, что попалось. Теперь опрос один, отдаёт он все выходы с именем
/// и разрешением, и все — метки в командах, раскладка по мониторам,
/// надзор за полноэкранным окном — спрашивают его.
///
/// Пустой ответ — законный исход: инструмента опроса может не быть в
/// системе. Тогда вызывающий обязан поставить обои так, как ставил до
/// всякого перечисления выходов.
fn screen_outputs<S: WpSys>(sys: &mut S, session_type: &str) -> Vec<ScreenOutput> {
    for cmd in output_probes(session_type) {
        let timeout = utils::default_timeout_secs(&cmd) as f64;
        let Some(out) = sys.run(&cmd, timeout) else {
            continue; // нет такой команды или вышло время
        };
        if out.rc != 0 || out.stdout.trim().is_empty() {
            continue;
        }
        let found = if cmd[0] == "xrandr" {
            parse_xrandr_outputs(&out.stdout)
        } else {
            parse_outputs_json(&out.stdout)
        };
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

// ============================================================================
// Наблюдения, которые план просит метками
// ============================================================================

/// Что уже спросили у системы за этот прогон плана.
///
/// Наблюдения дорогие, а нужны не каждому шагу: ядро просит их метками
/// `{monitor}`/`{resolution}`, и спрашиваем мы систему только когда шаг с
/// меткой ДЕЙСТВИТЕЛЬНО дошёл до исполнения. Иначе установка обоев через
/// swww дёргала бы `wlr-randr` впустую.
#[derive(Debug, Default)]
struct Seen {
    /// Тип сеанса — им выбирается порядок опроса выходов.
    session: String,
    /// Выходы, если их уже спрашивали за этот прогон (пустой список —
    /// спрашивали и не узнали, это НЕ повод спрашивать снова).
    outputs: Option<Vec<ScreenOutput>>,
    /// Какой выход подставляется в метки по умолчанию: `None` — первый.
    pinned: Option<ScreenOutput>,
}

impl Seen {
    fn new(session_type: &str) -> Self {
        Seen { session: session_type.to_string(), ..Seen::default() }
    }

    /// Выходы этого прогона — опрос делается не больше одного раза.
    fn outputs<S: WpSys>(&mut self, sys: &mut S) -> Vec<ScreenOutput> {
        if self.outputs.is_none() {
            let found = screen_outputs(sys, &self.session);
            if found.is_empty() {
                log_line(
                    "warning",
                    "не удалось перечислить мониторы — обои ставлю общей командой, \
                     как без перечисления",
                );
            }
            self.outputs = Some(found);
        }
        self.outputs.clone().unwrap_or_default()
    }

    /// Чем заполнять метки сейчас: закреплённый выход, иначе первый из
    /// списка, иначе — «все»/умолчание.
    fn marks<S: WpSys>(&mut self, sys: &mut S) -> ScreenOutput {
        if let Some(one) = &self.pinned {
            return one.clone();
        }
        self.outputs(sys).first().cloned().unwrap_or(ScreenOutput {
            name: ANY_OUTPUT.to_string(),
            resolution: ANY_RESOLUTION.to_string(),
            position: None,
        })
    }
}

/// Выход с наибольшей и наименьшей площадью — крайние из перечисленных.
///
/// ЗАЧЕМ ОБА. Когда на стол идёт ОДНА картинка на всех (пачка обоев,
/// движок-`Whole`), мерить надо по САМОМУ БОЛЬШОМУ выходу: большой монитор
/// получит свой размер, маленький картинку ужмёт, и это заметно лучше
/// обратного — растянутой с маленького. Когда спрашивают «занимает ли окно
/// весь экран», мерить надо по САМОМУ МАЛЕНЬКОМУ: полноэкранная игра живёт
/// на своём мониторе, и меркой ей служит он, а не сосед.
///
/// Выход с неразобранным разрешением в сравнении не участвует; список пуст
/// или ни один размер не прочитался — `None`, и вызывающий делает то же,
/// что делал без всякого перечисления выходов. При равной площади берётся
/// выход с меньшим именем: одна и та же машина обязана давать один и тот же
/// ответ.
fn extreme_output(outputs: &[ScreenOutput], biggest: bool) -> Option<&ScreenOutput> {
    let mut best: Option<(&ScreenOutput, u64)> = None;
    for o in outputs {
        let Some((w, h)) = o.size() else { continue };
        let area = w.saturating_mul(h);
        let better = match &best {
            None => true,
            Some((prev, prev_area)) => {
                if area == *prev_area {
                    o.name < prev.name
                } else if biggest {
                    area > *prev_area
                } else {
                    area < *prev_area
                }
            }
        };
        if better {
            best = Some((o, area));
        }
    }
    best.map(|(o, _)| o)
}

/// Самый большой выход — мерка для ОДНОЙ картинки на весь стол.
pub fn largest_output(outputs: &[ScreenOutput]) -> Option<&ScreenOutput> {
    extreme_output(outputs, true)
}

/// Самый маленький выход — мерка для «окно занимает весь свой экран».
pub fn smallest_output(outputs: &[ScreenOutput]) -> Option<&ScreenOutput> {
    extreme_output(outputs, false)
}

/// Выходы живой машины — вход для тех, у кого нет своего шва [`WpSys`].
///
/// Нужен вкладке «Оформление»: она ставит фоном картинку ИЗ ПАЧКИ и меряет
/// экран, чтобы выбрать какую. Своего опроса у неё больше нет — правда о
/// выходах одна на продукт и живёт здесь.
pub fn live_screen_outputs(session_type: &str) -> Vec<ScreenOutput> {
    screen_outputs(&mut LiveSys::new(), session_type)
}

/// Метки шага, которыми план просит наблюдения за выходом.
///
/// `{geometry}` — размер ВМЕСТЕ с положением (`xwinwrap -g`), `{resolution}`
/// — один размер, и старая метка остаётся для тех, кому смещение не нужно.
fn asks_about_output(text: &str) -> bool {
    text.contains("{monitor}")
        || text.contains("{resolution}")
        || text.contains(wpe::GEOMETRY_MARK)
}

fn fill<S: WpSys>(sys: &mut S, text: &str, seen: &mut Seen) -> String {
    if !asks_about_output(text) {
        return text.to_string();
    }
    let one = seen.marks(sys);
    text.replace("{monitor}", &one.name)
        .replace(wpe::GEOMETRY_MARK, &one.geometry())
        .replace("{resolution}", &one.resolution)
}

fn fill_argv<S: WpSys>(sys: &mut S, argv: &[String], seen: &mut Seen) -> Vec<String> {
    argv.iter().map(|a| fill(sys, a, seen)).collect()
}

// ============================================================================
// Таблица движков: чем раскладывать обои по выходам
// ============================================================================

/// Как движок покрывает несколько мониторов.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spread {
    /// Движок кладёт обои на все выходы САМ — команда одна, как была.
    /// Сюда же попадает всё, чей стол принадлежит окружению (gsettings,
    /// Plasma, xfconf), и демоны, у которых «все выходы» — умолчание.
    Whole,
    /// Одна команда, но выходы надо перечислить: флаг и разделитель имён
    /// (`swww img --outputs DP-1,HDMI-A-1`).
    List {
        /// Флаг, после которого идёт список.
        flag: &'static str,
        /// Чем склеены имена выходов.
        sep: &'static str,
    },
    /// Команда повторяется по разу на выход, и метки `{monitor}`,
    /// `{resolution}` и `{geometry}` в ней означают КОНКРЕТНЫЙ выход, а не
    /// первый.
    Each,
}

/// Чем раскладывать обои по выходам — свойство движка, а не догадка кода.
///
/// Имя строки — `Step::backend`, то самое, которым движок зовёт свой шаг.
/// Чего нет в таблице, то считается `Whole`: незнакомый движок обязан
/// вести себя ровно так, как вёл до появления раскладки.
const ENGINE_SPREAD: &[(&str, Spread)] = &[
    // `swww img` без флага кроет выходы по своему умолчанию, а оно менялось
    // от версии к версии, и демон помнит прежнюю раскладку по выходам.
    // Перечисляем сами — тогда ответ не зависит ни от версии, ни от памяти
    // демона.
    ("swww", Spread::List { flag: "--outputs", sep: "," }),
    // `hyprctl hyprpaper wallpaper ,путь` — своя форма «всем мониторам»:
    // пустое имя монитора перед запятой и значит «все».
    ("hyprpaper", Spread::Whole),
    // swaybg и wbg без `-o` держат ВСЕ выходы; pcmanfm-qt, feh, gsettings,
    // Plasma и xfconf-query владеют столом целиком.
    ("swaybg", Spread::Whole),
    ("wbg", Spread::Whole),
    // `swaymsg output * bg` — все выходы одной командой.
    ("sway", Spread::Whole),
    ("pcmanfm-qt", Spread::Whole),
    ("feh", Spread::Whole),
    ("gsettings", Spread::Whole),
    ("kde-dbus", Spread::Whole),
    ("plasma-apply-wallpaperimage", Spread::Whole),
    ("xfconf-query", Spread::Whole),
    // mpvpaper принимает РОВНО ОДИН выход позиционным аргументом — на
    // второй монитор он попадает только вторым запуском.
    ("mpvpaper", Spread::Each),
    // xwinwrap кроет корневое окно X по геометрии `-g ШхВ+X+Y`: куда
    // попадёт окно, решает смещение. Плану оно приходит меткой
    // `{geometry}` — своей на каждый выход, поэтому на два монитора
    // xwinwrap поднимается дважды. Тем же путём (xwinwrap) идут веб-обои
    // через surf.
    ("xwinwrap", Spread::Each),
    ("surf", Spread::Each),
];

/// Строка таблицы для движка; незнакомый — `Whole`.
pub fn engine_spread(backend: &str) -> Spread {
    ENGINE_SPREAD
        .iter()
        .find(|(name, _)| *name == backend)
        .map(|(_, spread)| *spread)
        .unwrap_or(Spread::Whole)
}

// ============================================================================
// Исполнитель
// ============================================================================

/// Живой исполнитель планов обоев.
///
/// Состояние — ровно то, что было у `WallpaperApplier`: что сейчас играет,
/// каким процессом, что вернуть после остановки. Ничего сверх этого
/// исполнитель не помнит и ни о чём не решает сам.
pub struct WallpaperExec<S: WpSys> {
    sys: S,
    de: String,
    session_type: String,

    current_format: Option<String>,
    current_path: Option<String>,
    current_name: String,

    /// Процесс живых обоев.
    process: Option<ProcHandle>,
    /// Процесс, УДЕРЖИВАЮЩИЙ статичный фон (swaybg/wbg): живёт дольше живых.
    static_holder: Option<ProcHandle>,
    /// Процессы обоев на ОСТАЛЬНЫХ мониторах: движки вроде `mpvpaper`
    /// принимают ровно один выход, поэтому на двух экранах процессов
    /// столько же, сколько экранов. Спутники живут и умирают вместе с
    /// главным процессом — иначе «Остановить» гасило бы обои на одном
    /// мониторе и оставляло их играть на другом.
    companions: Vec<ProcHandle>,

    /// Обои, которые стояли до нас, — их возвращает остановка.
    saved_wallpaper: Option<String>,
    /// Окружение, в котором эти обои были сохранены.
    saved_de: Option<String>,
    /// KDE: ЧУЖОЙ плагин обоев стола, если стол был занят им.
    saved_kde_plugin: Option<String>,
    /// Размытый кадр, поставленный подложкой под живые обои.
    blur_path: Option<String>,
    /// Фоновые звуки: отдельный процесс на каждый, порядок — как заводили.
    ambient: Vec<Ambient>,

    /// Что мосту уже сказано — не то, что с обоями на самом деле.
    bridge_live: bool,
    bridge_name: String,
    bridge_said_at: Option<f64>,
    bridge_flag: Option<bool>,

    /// Процесс обоев заморожен надзором за нагрузкой (SIGSTOP).
    perf_paused: bool,
    /// Приписка «WE-обоя осталась поверх» к итогу текущего применения.
    we_note: Option<String>,
}

// ============================================================================
// Надзор за нагрузкой
// ============================================================================

/// Что надзор за нагрузкой узнал о машине за один замер.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoadObservation {
    /// Батарея ниже порога и не заряжается.
    pub battery_low: bool,
    /// Активно полноэкранное окно (и человек разрешил на нём паузу).
    pub fullscreen: bool,
    /// Процесс обоев грузит процессор выше порога.
    pub cpu_high: bool,
}

/// Настройки надзора на один замер.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorConfig {
    /// Ставить ли обои на паузу при полноэкранном окне.
    pub pause_on_fullscreen: bool,
    /// Порог батареи, %.
    pub battery_pause_pct: i64,
}

/// Что делать надзору дальше.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorVerdict {
    /// Следить дальше.
    Continue,
    /// Следить больше не за чем: процесс сменился, упал или остановлен.
    Done,
}

/// Настройки надзора из карты настроек Rexon.
///
/// ⭐ Починка: у Python пауза на полноэкранном окне была включена жёстко
/// (`pause_on_fullscreen = True`) и не слушала человека. Если он снимал
/// «Останавливать обои, когда их не видно», хост переставал ставить плеер
/// на паузу, а надзор всё равно замораживал процесс. Теперь решение одно
/// — та же настройка, по которой паузу делает сам хост.
pub fn monitor_config_from(settings: &serde_json::Map<String, serde_json::Value>) -> MonitorConfig {
    let audio = crate::wallpaper_audio::AudioSettings::from_map(settings);
    MonitorConfig {
        pause_on_fullscreen: audio.actions.fullscreen.pauses(),
        battery_pause_pct: wpe::BATTERY_PAUSE_PCT,
    }
}

/// Корень кеша пользователя живой машины (`$XDG_CACHE_HOME`, иначе
/// `~/.cache`).
fn cache_home_live() -> String {
    match std::env::var("XDG_CACHE_HOME") {
        Ok(v) if !v.is_empty() => v,
        _ => format!("{}/.cache", std::env::var("HOME").unwrap_or_else(|_| ".".to_string())),
    }
}

/// Настройки надзора с диска — перечитываются на каждом замере, чтобы
/// правка галочки действовала сразу, без перезапуска обоев.
pub fn monitor_config_live() -> MonitorConfig {
    monitor_config_from(&crate::settings::load_settings(&crate::settings::settings_file()))
}

/// Разряжена ли батарея — порт `_is_battery_low` (первая батарея `BAT*`).
fn battery_low<S: WpSys>(sys: &mut S, threshold: i64) -> bool {
    if !sys.is_dir(wpe::POWER_SUPPLY_DIR) {
        return false;
    }
    for name in sys.listdir(wpe::POWER_SUPPLY_DIR) {
        if !name.starts_with("BAT") {
            continue;
        }
        let dir = format!("{}/{name}", wpe::POWER_SUPPLY_DIR);
        let Some(pct) = sys
            .read_text(&format!("{dir}/capacity"))
            .and_then(|t| t.trim().parse::<i64>().ok())
        else {
            continue;
        };
        let status = sys.read_text(&format!("{dir}/status")).unwrap_or_default();
        return wpe::battery_should_pause(pct, status.trim(), threshold);
    }
    false
}

/// Об отсутствующем инструменте опознания пишем в журнал ОДИН раз: замер
/// идёт каждые 5 секунд, и журнал заваливался бы одной и той же строкой.
fn probe_tool<S: WpSys>(sys: &mut S, name: &str) -> bool {
    if sys.has_tool(name) {
        return true;
    }
    static MISSING: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut said = MISSING
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if !said.iter().any(|n| n == name) {
        said.push(name.to_string());
        log_line("info", &format!("нет {name} — пауза обоев при полноэкранном окне отключена"));
    }
    false
}

/// Активно ли полноэкранное окно — порт `_is_fullscreen_app_active`. Чем
/// спрашивать и как разбирать ответ — движок.
///
/// `wallpaper_pid` — процесс, который прямо сейчас рисует обои: его окно
/// «окном поверх обоев» не считается (см. [`wpe::WALLPAPER_SAFE_WINDOW_TYPES`]).
fn fullscreen_active<S: WpSys>(
    sys: &mut S,
    de: &str,
    session_type: &str,
    wallpaper_pid: i64,
) -> bool {
    let backend = wpe::fullscreen_backend(de, session_type);
    if backend.is_empty() || !probe_tool(sys, backend) {
        return false;
    }
    let ok_out = |out: Option<RunOut>| out.filter(|o| o.rc == 0 && !o.stdout.is_empty());
    match backend {
        "xdotool" => {
            let Some(out) = ok_out(sys.run(&wpe::xdotool_geometry_argv(), 5.0)) else {
                return false;
            };
            // ⚠️ Мерка — САМЫЙ МАЛЕНЬКИЙ выход, а не первый попавшийся: окно
            // во весь экран занимает СВОЙ монитор, и на мониторе поменьше оно
            // меньше первого — прежняя мерка «первое разрешение» такое окно
            // просто не замечала, и обои не вставали на паузу. Выходы не
            // перечислились — остаётся умолчание движка, как было.
            let outputs = screen_outputs(sys, session_type);
            let screen = smallest_output(&outputs)
                .map(|o| o.resolution.clone())
                .unwrap_or_else(|| ANY_RESOLUTION.to_string());
            // ⚠️ Одной геометрии мало: активным окном оконный менеджер
            // называет и сам рабочий стол, а обои и есть рабочий стол во
            // весь экран, — иначе пауза стояла бы всегда. Решение одно на
            // все надзиратели и живёт в ядре звука обоев (правило #12):
            // хост обоев спрашивает ту же функцию.
            let has_xprop = sys.has_tool("xprop");
            crate::wallpaper_audio::x11_window_conditions(
                &out.stdout,
                &screen,
                &[wallpaper_pid],
                |id| {
                    if !has_xprop {
                        return None;
                    }
                    ok_out(sys.run(&wpe::xprop_window_info_argv(id), 5.0)).map(|o| o.stdout)
                },
            )
            .fullscreen
        }
        "hyprctl" => ok_out(sys.run(&wpe::hyprctl_active_argv(), 5.0))
            .map(|o| {
                !wpe::hyprctl_window_is_wallpaper_layer(&o.stdout, &[wallpaper_pid])
                    && wpe::parse_hyprctl_fullscreen(&o.stdout)
            })
            .unwrap_or(false),
        "gdbus" => ok_out(sys.run(&wpe::gdbus_fullscreen_argv(), 5.0))
            .map(|o| wpe::parse_gdbus_fullscreen(&o.stdout))
            .unwrap_or(false),
        "kdotool" => {
            let Some(out) = ok_out(sys.run(&wpe::kdotool_active_argv(), 5.0)) else {
                return false;
            };
            let id = out.stdout.trim().to_string();
            if id.is_empty() {
                return false;
            }
            // Состояние окна отдаёт сам KWin (`getWindowInfo`); спрашиваем
            // тем qdbus, что есть, — разбор понимает его вид ответа.
            for tool in ["qdbus6", "qdbus"] {
                if !sys.has_tool(tool) {
                    continue;
                }
                let mut argv: Vec<String> = std::iter::once(tool.to_string())
                    .chain(wpe::KWIN_WINDOW_INFO.iter().map(|s| (*s).to_string()))
                    .collect();
                argv.push(id.clone());
                return ok_out(sys.run(&argv, 5.0))
                    .map(|o| {
                        !wpe::kwin_window_is_wallpaper_layer(&o.stdout, &[wallpaper_pid])
                            && wpe::parse_kwin_fullscreen(&o.stdout)
                    })
                    .unwrap_or(false);
            }
            false
        }
        _ => false,
    }
}

/// Грузит ли процесс процессор выше порога — порт `_is_process_cpu_high`.
fn cpu_high<S: WpSys>(sys: &mut S, pid: i64) -> bool {
    let Some(out) = sys.run(&wpe::ps_cpu_argv(pid), 5.0) else {
        return false;
    };
    if out.rc != 0 {
        return false;
    }
    match out.stdout.trim().parse::<f64>() {
        Ok(cpu) if cpu > wpe::CPU_HIGH_THRESHOLD => {
            log_line("debug", &format!("процесс обоев PID={pid} грузит процессор: {cpu:.1}%"));
            true
        }
        _ => false,
    }
}

/// Один замер надзора. Идёт БЕЗ замка исполнителя: опрос оконного
/// менеджера и `ps` — это десятки миллисекунд, и окно в это время не должно
/// видеть обои «занятыми».
pub fn observe_load<S: WpSys>(
    sys: &mut S,
    de: &str,
    session_type: &str,
    pid: i64,
    cfg: &MonitorConfig,
) -> LoadObservation {
    LoadObservation {
        battery_low: battery_low(sys, cfg.battery_pause_pct),
        fullscreen: cfg.pause_on_fullscreen && fullscreen_active(sys, de, session_type, pid),
        cpu_high: cpu_high(sys, pid),
    }
}

/// Поднять живой надзор за процессом, который сейчас рисует обои.
///
/// Порт `_start_performance_monitor` + `_performance_monitor_loop`: замер
/// раз в [`wpe::MONITOR_INTERVAL_SEC`], решения — у исполнителя
/// ([`WallpaperExec::monitor_apply`]). Надзор кончается сам, как только
/// процесс сменился, упал или остановлен, — поэтому звать его можно после
/// каждого применения живых обоев, без учёта прежнего потока.
///
/// Окно само его не зовёт: применения идут через [`run_in_slot`], и надзор
/// поднимается там — в одном месте на все страницы. Шов обобщён ради
/// сверок: замеры идут тем же швом, что у исполнителя в слоте.
pub fn spawn_load_monitor<S: WpSys + Default + Send + 'static>(slot: ExecSlot<S>) {
    let target = {
        let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_mut().and_then(|exec| {
            // Следить нужно не за всякими живыми обоями — решает ядро.
            if !format_wants_load_monitor(exec.current_format()) {
                return None;
            }
            exec.monitor_target()
                .map(|(handle, pid)| (handle, pid, exec.de.clone(), exec.session_type.clone()))
        })
    };
    let Some((watched, pid, de, session_type)) = target else {
        return;
    };
    log_line("info", &format!("надзор за нагрузкой начат: PID={pid}"));
    let spawned = std::thread::Builder::new()
        .name("rexon-perf-monitor".to_string())
        .spawn(move || {
            let mut probe = S::default();
            let mut strikes = 0i64;
            loop {
                let cfg = monitor_config_live();
                let obs = observe_load(&mut probe, &de, &session_type, pid, &cfg);
                let verdict = {
                    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
                    match guard.as_mut() {
                        Some(exec) => exec.monitor_apply(watched, &mut strikes, &obs),
                        None => MonitorVerdict::Done,
                    }
                };
                if verdict == MonitorVerdict::Done {
                    break;
                }
                std::thread::sleep(Duration::from_secs(wpe::MONITOR_INTERVAL_SEC.max(1) as u64));
            }
        });
    if let Err(e) = spawned {
        log_line("error", &format!("надзор за нагрузкой не поднялся: {e}"));
    }
}

// ============================================================================
// Исполнитель процесса: один на все страницы окна
// ============================================================================

/// Слот исполнителя обоев: пуст до первого обращения.
pub type ExecSlot<S> = std::sync::Arc<Mutex<Option<WallpaperExec<S>>>>;

/// Слот живого исполнителя.
pub type LiveSlot = ExecSlot<LiveSys>;

/// Слот исполнителя обоев процесса — ОДИН на все страницы окна.
///
/// ПОЧЕМУ ОДИН. Исполнитель помнит, какой процесс сейчас рисует обои, что
/// стояло на столе до нас и что вернуть после остановки. Пока у «Настроек»
/// и «Steam» были свои исполнители, видео, поставленное одной страницей,
/// другая не видела и снять не могла, а применение из второй ставило второй
/// хост поверх первого.
///
/// ПОЧЕМУ В ЯДРЕ, а не в модуле окна. Всё, что должно быть одним на
/// процесс, уже здесь: живой шов [`LiveSys`] и надзор за нагрузкой
/// ([`spawn_load_monitor`]), которому нужен именно этот слот. Слот в модуле
/// одной страницы (так было в `steam_workers`) делал бы остальные страницы
/// зависимыми от неё ради общего состояния, а отдельный модуль окна был бы
/// второй дверью к тому же исполнителю. Здесь же — и единственная дверь
/// применения с надзором ([`run_in_slot`]).
///
/// ⚠️ Граница: слот один на КОПИЮ ядра. Все Rust-страницы окна живут в
/// одном процессе `rexon-main-window` (крейт `rexon-gui` вшит статически)
/// и видят общий слот.
pub fn live_slot() -> LiveSlot {
    static SLOT: OnceLock<LiveSlot> = OnceLock::new();
    SLOT.get_or_init(|| std::sync::Arc::new(Mutex::new(None))).clone()
}

/// Сделать работу исполнителем слота, заведя его при первом обращении
/// (окружение опознаётся швом). Замок ждётся — для фоновых потоков.
///
/// Замок отравлен — значит, фоновое применение упало с паникой; состояние
/// исполнителя от этого не портится, работать с ним можно.
pub fn with_slot<S: WpSys + Default, R>(
    slot: &Mutex<Option<WallpaperExec<S>>>,
    work: impl FnOnce(&mut WallpaperExec<S>) -> R,
) -> R {
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    let exec = guard.get_or_insert_with(|| WallpaperExec::detect(S::default()));
    work(exec)
}

/// То же без ожидания: `None` — исполнитель сейчас занят (применением с
/// любой страницы либо замером надзора). Для действий из потока окна:
/// ждать замок там значило бы заморозить окно на время команд оболочки.
pub fn try_with_slot<S: WpSys + Default, R>(
    slot: &Mutex<Option<WallpaperExec<S>>>,
    work: impl FnOnce(&mut WallpaperExec<S>) -> R,
) -> Option<R> {
    let mut guard = match slot.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return None,
    };
    let exec = guard.get_or_insert_with(|| WallpaperExec::detect(S::default()));
    Some(work(exec))
}

/// Применение обоев исполнителем слота и затем надзор за нагрузкой — ОДНА
/// дверь для всех страниц окна. Замок отпускается ДО надзора: надзор сам
/// берёт его на каждом замере. Нужен ли надзор этим обоям, решает
/// [`spawn_load_monitor`] (видео и YouTube — да, прочим — нет).
pub fn run_in_slot<S: WpSys + Default + Send + 'static, R>(
    slot: &ExecSlot<S>,
    work: impl FnOnce(&mut WallpaperExec<S>) -> R,
) -> R {
    let out = with_slot(slot, work);
    spawn_load_monitor(slot.clone());
    out
}

impl<S: WpSys> WallpaperExec<S> {
    /// Исполнитель для явно названного окружения — так его зовут сверки.
    pub fn with_env(sys: S, de: &str, session_type: &str) -> WallpaperExec<S> {
        WallpaperExec {
            sys,
            de: de.to_string(),
            session_type: session_type.to_string(),
            current_format: None,
            current_path: None,
            current_name: String::new(),
            process: None,
            static_holder: None,
            companions: Vec::new(),
            saved_wallpaper: None,
            saved_de: None,
            saved_kde_plugin: None,
            blur_path: None,
            ambient: Vec::new(),
            bridge_live: false,
            bridge_name: String::new(),
            bridge_said_at: None,
            bridge_flag: None,
            perf_paused: false,
            we_note: None,
        }
    }

    /// Исполнитель, опознавший окружение сам — по тем же переменным, по
    /// которым его опознаёт движок.
    pub fn detect(mut sys: S) -> WallpaperExec<S> {
        let de = wpe::detect_de(&sys.env("XDG_CURRENT_DESKTOP"), &sys.env("DESKTOP_SESSION"));
        let session = wpe::detect_session_type(
            &sys.env("XDG_SESSION_TYPE"),
            &sys.env("WAYLAND_DISPLAY"),
            &sys.env("DISPLAY"),
        );
        WallpaperExec::with_env(sys, &de, &session)
    }

    /// Опознанное окружение.
    pub fn de(&self) -> &str {
        &self.de
    }

    /// Опознанный тип сессии.
    pub fn session_type(&self) -> &str {
        &self.session_type
    }

    /// Тип сеанса для плана статичных обоев: у Deepin — `treeland`, если
    /// композитор сеанса и правда Treeland ([`crate::dde::treeland_session_live`]);
    /// иначе план Deepin — отказ.
    fn static_session(&self) -> String {
        if self.de == "deepin" && crate::dde::treeland_session_live() {
            wpe::TREELAND_SESSION.to_string()
        } else {
            self.session_type.clone()
        }
    }

    /// Формат того, что играет сейчас.
    pub fn current_format(&self) -> Option<&str> {
        self.current_format.as_deref()
    }

    /// Путь того, что играет сейчас.
    pub fn current_path(&self) -> Option<&str> {
        self.current_path.as_deref()
    }

    /// Название сцены для моста (его знает только тот, кто применял обои).
    pub fn set_current_name(&mut self, name: &str) {
        self.current_name = name.to_string();
    }

    /// Доступ к шву — мосту и сверкам, чтобы посмотреть на побочный след.
    pub fn sys_mut(&mut self) -> &mut S {
        &mut self.sys
    }

    /// Какие из интересных движку программ есть в системе.
    pub fn plan_tools(&mut self) -> Vec<String> {
        wpe::PLAN_TOOLS
            .iter()
            .filter(|t| self.sys.has_tool(t))
            .map(|t| (*t).to_string())
            .collect()
    }

    /// Подсказки об именах пакетов текущего дистрибутива.
    pub fn hints(&mut self) -> wpe::Hints {
        wpe::Hints {
            mpvpaper: self.sys.package_hint("mpvpaper"),
            xwinwrap: self.sys.package_hint("xwinwrap"),
            cvlc: self.sys.package_hint("cvlc"),
            webkit: self.sys.package_hint("webkit2gtk-4.1"),
            swww: self.sys.package_hint("swww"),
            ytdlp: self.sys.package_hint("yt-dlp"),
        }
    }

    // ── прогон плана ──────────────────────────────────────────────────────

    /// Исполнить план движка: первый шаг, давший исход, и есть ответ.
    pub fn run_plan(&mut self, steps: &[wpe::Step], path: &str) -> WallpaperResult {
        let mut seen = Seen::new(&self.session_type);
        for step in steps {
            if let Some(res) = self.run_step(step, path, &mut seen) {
                return res;
            }
        }
        // Сюда попадаем, только если план кончился, а исхода не дал ни один
        // шаг: планы движка заканчиваются терминальным шагом или отказом.
        log_line(
            "error",
            &format!("план обоев кончился без исхода ({} шагов)", steps.len()),
        );
        WallpaperResult::fail(crate::i18n::tr("Could not apply the wallpaper"))
    }

    /// Один шаг плана. `None` — шаг не сработал, пробуем следующий.
    fn run_step(
        &mut self,
        step: &wpe::Step,
        path: &str,
        seen: &mut Seen,
    ) -> Option<WallpaperResult> {
        if step.kind == wpe::step_kind::FAIL {
            let msg = fill(&mut self.sys, &step.err_message, seen);
            return Some(WallpaperResult::fail(msg));
        }

        if step.ensure_swww {
            self.ensure_swww_daemon();
        }
        for pre in &step.pre {
            let cmd = fill_argv(&mut self.sys, pre, seen);
            let timeout = utils::default_timeout_secs(&cmd) as f64;
            self.sys.run(&cmd, timeout);
        }

        let cmds = self.spread_argv(step, seen);
        let argv = cmds
            .first()
            .map(|(_, a)| a.clone())
            .unwrap_or_default();

        let (ok, stderr) = match step.kind.as_str() {
            wpe::step_kind::RUN => {
                // Команд может быть несколько — по одной на выход. Успехом
                // считается, если обои встали ХОТЯ БЫ на один: отказ на
                // втором мониторе не повод переставлять всё запасным
                // движком и терять уже поставленное на первом.
                let mut any_ok = false;
                let mut first_err = String::new();
                for (output, cmd) in &cmds {
                    let timeout = step
                        .timeout
                        .unwrap_or_else(|| utils::default_timeout_secs(cmd))
                        as f64;
                    let out = self.sys.run(cmd, timeout);
                    let (step_ok, err) = match &out {
                        // Команды нет или вышло время: успехом это
                        // считается, только если сам шаг об этом попросил
                        // (`ok_on_none`).
                        None => (step.ok_on_none, String::new()),
                        Some(r) => (r.rc == 0, r.stderr.clone()),
                    };
                    if step_ok {
                        any_ok = true;
                        continue;
                    }
                    if first_err.is_empty() {
                        first_err = err;
                    }
                    if cmds.len() > 1 {
                        log_line(
                            "warning",
                            &format!(
                                "{}: обои не встали на монитор {output}",
                                step.backend
                            ),
                        );
                    }
                }
                (any_ok, first_err)
            }
            wpe::step_kind::PLASMA_SCRIPT => {
                let script = argv.first().cloned().unwrap_or_default();
                (self.plasma_evaluate(&script).is_some(), String::new())
            }
            wpe::step_kind::XFCONF_ALL => {
                return Some(self.run_xfconf(step, &argv, path, seen));
            }
            wpe::step_kind::COSMIC_BG => {
                let ok = self.write_cosmic_bg(path);
                if ok {
                    // Наш прежний swaybg/wbg лёг бы поверх cosmic-bg.
                    self.kill_static_holder();
                }
                (ok, String::new())
            }
            wpe::step_kind::SPAWN
            | wpe::step_kind::SPAWN_SESSION
            | wpe::step_kind::SPAWN_HOLD => {
                return Some(self.run_spawn(step, &cmds, path, seen));
            }
            other => {
                log_line("error", &format!("неизвестный вид шага плана обоев: {other}"));
                return None;
            }
        };

        let (ok, stderr) = match (&step.verify_ini, ok) {
            (Some(check), true) if !self.ini_written(check, path) => {
                log_line(
                    "warning",
                    &format!("{}: код возврата 0, но обои в {} не записаны", step.backend, check.file),
                );
                (false, crate::i18n::tr("the wallpaper was not saved in the desktop settings"))
            }
            _ => (ok, stderr),
        };

        if ok {
            if step.drops_holder {
                self.kill_static_holder();
            }
            for post in &step.post {
                let cmd = fill_argv(&mut self.sys, post, seen);
                let timeout = utils::default_timeout_secs(&cmd) as f64;
                self.sys.run(&cmd, timeout);
            }
            return Some(self.succeeded(step, path, seen, None));
        }
        self.failed(step, seen, &stderr)
    }

    /// `$XDG_CONFIG_HOME` сеанса (или `~/.config`).
    fn config_home(&mut self) -> String {
        let home = self.home();
        let cfg = self.sys.env("XDG_CONFIG_HOME");
        utils::config_home(if cfg.is_empty() { None } else { Some(cfg.as_str()) }, &home)
    }

    /// COSMIC: картинка в родной ключ cosmic-bg ([`wpe::apply_cosmic_bg`]).
    fn write_cosmic_bg(&mut self, path: &str) -> bool {
        let cfg = self.config_home();
        let sys = std::cell::RefCell::new(&mut self.sys);
        wpe::apply_cosmic_bg(
            &cfg,
            path,
            &mut |p| sys.borrow_mut().read_text(p),
            &mut |p, text| {
                sys.borrow_mut().write_text(p, text);
                true
            },
        )
    }

    /// Записала ли программа обои к себе в настройки (`Step::verify_ini`).
    ///
    /// Запись у pcmanfm-qt идёт уже запущенным экземпляром, то есть не
    /// мгновенно: файл смотрим несколько раз с короткой паузой (до ~2 с).
    fn ini_written(&mut self, check: &wpe::IniCheck, path: &str) -> bool {
        let home = self.home();
        let cfg = self.sys.env("XDG_CONFIG_HOME");
        let file = utils::expand_user(
            &check.file,
            &home,
            if cfg.is_empty() { None } else { Some(cfg.as_str()) },
        );
        for attempt in 0..8 {
            if attempt > 0 {
                self.sys.sleep(0.25);
            }
            let text = self.sys.read_text(&file);
            if wpe::ini_check_passes(text.as_deref(), check, path) {
                return true;
            }
        }
        false
    }

    /// Команды шага, разложенные по выходам: пара «имя выхода, команда».
    ///
    /// Сколько команд получится, решает [`ENGINE_SPREAD`] — свойство самого
    /// движка, а не догадка по виду команды. Имя выхода в паре нужно только
    /// журналу: пустая строка значит «команда общая, про выходы в ней ничего».
    ///
    /// ⚠️ Защита от дурака одна на все ветки: как только чего-то не хватает
    /// — выходы не перечислились, инструмента опроса нет, в команде нет
    /// меток выхода — отдаём РОВНО ту команду, что была в плане. Обои
    /// встанут так же, как вставали до появления раскладки, а не перестанут
    /// ставиться вовсе.
    fn spread_argv(&mut self, step: &wpe::Step, seen: &mut Seen) -> Vec<(String, Vec<String>)> {
        let whole = |sys: &mut S, seen: &mut Seen| {
            vec![(String::new(), fill_argv(sys, &step.argv, seen))]
        };
        match engine_spread(&step.backend) {
            Spread::Whole => whole(&mut self.sys, seen),
            Spread::List { flag, sep } => {
                let outputs = seen.outputs(&mut self.sys);
                let mut argv = fill_argv(&mut self.sys, &step.argv, seen);
                let names: Vec<&str> =
                    outputs.iter().map(|o| o.name.as_str()).collect();
                // Нечего перечислять; движок уже назвал выходы сам; имя
                // выхода нам неизвестно («все») — во всех трёх случаях
                // команда уходит как была.
                if names.is_empty()
                    || argv.iter().any(|a| a == flag)
                    || names.contains(&ANY_OUTPUT)
                {
                    return vec![(String::new(), argv)];
                }
                let list = names.join(sep);
                argv.push(flag.to_string());
                argv.push(list.clone());
                vec![(list, argv)]
            }
            Spread::Each => {
                let marked = step.argv.iter().any(|a| asks_about_output(a));
                if !marked {
                    // Повтор одной и той же команды ничего не добавит:
                    // выбирать выход ей нечем.
                    log_line(
                        "debug",
                        &format!(
                            "{}: в команде не названо, на какой монитор ставить;                              ставлю одной командой",
                            step.backend
                        ),
                    );
                    return whole(&mut self.sys, seen);
                }
                let outputs = seen.outputs(&mut self.sys);
                if outputs.len() < 2 {
                    // Один выход или ни одного — команда одна, метки в ней
                    // заполнит общий путь.
                    return whole(&mut self.sys, seen);
                }
                // ⚠️ Защита от дурака: команда выбирает монитор геометрией,
                // а опрос не сказал, где мониторы стоят. Вторая команда
                // легла бы поверх первой — уходит одна, как было.
                let by_geometry = step
                    .argv
                    .iter()
                    .any(|a| a.contains(wpe::GEOMETRY_MARK));
                if by_geometry && outputs.iter().any(|o| o.position.is_none()) {
                    log_line(
                        "warning",
                        &format!(
                            "{}: опрос не сказал, где стоят мониторы — ставлю \
                             обои одной командой, на один экран",
                            step.backend
                        ),
                    );
                    return whole(&mut self.sys, seen);
                }
                let mut cmds = Vec::with_capacity(outputs.len());
                for one in outputs {
                    let name = one.name.clone();
                    seen.pinned = Some(one);
                    cmds.push((name, fill_argv(&mut self.sys, &step.argv, seen)));
                }
                seen.pinned = None;
                cmds
            }
        }
    }

    /// Xfce: перечислить свойства фона и записать путь в каждое `/last-image`.
    fn run_xfconf(
        &mut self,
        step: &wpe::Step,
        argv: &[String],
        path: &str,
        seen: &mut Seen,
    ) -> WallpaperResult {
        let timeout = utils::default_timeout_secs(argv) as f64;
        let out = self.sys.run(argv, timeout);
        let stdout = match &out {
            Some(r) if !r.stdout.is_empty() => r.stdout.clone(),
            _ => return WallpaperResult::fail(step.err_message.clone()),
        };
        let Some(setter) = step.post.first() else {
            return WallpaperResult::fail(step.err_message.clone());
        };
        let mut set_any = false;
        for line in stdout.lines() {
            if !line.contains("/last-image") {
                continue;
            }
            let prop = line.trim();
            let cmd: Vec<String> = setter.iter().map(|a| a.replace("{prop}", prop)).collect();
            let timeout = utils::default_timeout_secs(&cmd) as f64;
            self.sys.run(&cmd, timeout);
            set_any = true;
        }
        if set_any {
            return self.succeeded(step, path, seen, None);
        }
        WallpaperResult::fail(step.alt_message.clone())
    }

    /// Запуск процесса обоев (или удерживающего фон демона).
    ///
    /// Команд может быть несколько — по одной на монитор (движки вроде
    /// `mpvpaper` принимают ровно один выход). Первая держит стол и живёт
    /// как «тот самый» процесс обоев, остальные идут спутниками: их
    /// снимают, замораживают и размораживают вместе с ней.
    fn run_spawn(
        &mut self,
        step: &wpe::Step,
        cmds: &[(String, Vec<String>)],
        path: &str,
        seen: &mut Seen,
    ) -> WallpaperResult {
        let kind = step.kind.as_str();
        if kind == wpe::step_kind::SPAWN_SESSION {
            // Прежние живые обои снимаем без отката фона: новые встают
            // сразу, и откат мигнул бы старой картинкой.
            self.kill_running();
        }
        if kind == wpe::step_kind::SPAWN_HOLD {
            self.kill_static_holder();
        }
        self.kill_companions();
        let new_session = kind != wpe::step_kind::SPAWN;
        let empty: Vec<String> = Vec::new();
        let argv = cmds.first().map(|(_, a)| a).unwrap_or(&empty);
        let proc = match self.sys.spawn(argv, new_session) {
            Ok(p) => p,
            Err(e) => {
                log_line("error", &format!("не удалось запустить {}: {e}", step.backend));
                return self
                    .failed(step, seen, &e)
                    .unwrap_or_else(|| WallpaperResult::fail(e));
            }
        };

        // Запуск удался — это ещё НЕ значит, что обои встали: все шаги
        // `spawn*` поднимают процесс, который рисует (или держит) стол,
        // пока жив, а мёртвый умирает через миллисекунды. Отсрочка и её
        // обоснование живут в движке, своей копии здесь нет.
        if let Some(rc) = self.wait_alive(proc) {
            log_line(
                "error",
                &format!("процесс {} умер сразу после запуска (код {rc})", step.backend),
            );
            let died = wpe::spawn_died_message(Some(rc as i32));
            self.sys.kill_process(proc);
            return self
                .failed(step, seen, &died)
                .unwrap_or_else(|| WallpaperResult::fail(died));
        }

        // Остальные мониторы. Неудача на них НЕ роняет весь шаг: на первом
        // обои уже идут, и уводить план на запасной движок значило бы
        // погасить то, что встало.
        for (output, cmd) in cmds.iter().skip(1) {
            let extra = match self.sys.spawn(cmd, new_session) {
                Ok(p) => p,
                Err(e) => {
                    log_line(
                        "warning",
                        &format!("{}: не запустился на мониторе {output}: {e}", step.backend),
                    );
                    continue;
                }
            };
            if let Some(rc) = self.wait_alive(extra) {
                log_line(
                    "warning",
                    &format!(
                        "{}: на мониторе {output} процесс умер сразу (код {rc})",
                        step.backend
                    ),
                );
                self.sys.kill_process(extra);
                continue;
            }
            self.companions.push(extra);
        }

        if kind == wpe::step_kind::SPAWN_HOLD {
            // swaybg/wbg не «ставят и выходят» — они ДЕРЖАТ фон, пока живы.
            self.static_holder = Some(proc);
            return self.succeeded(step, path, seen, None);
        }
        self.process = Some(proc);
        let pid = self.sys.pid(proc);
        self.succeeded(step, path, seen, pid)
    }

    /// Пережил ли процесс отсрочку? `None` — жив, иначе код возврата.
    fn wait_alive(&mut self, proc: ProcHandle) -> Option<i64> {
        let deadline = self.sys.monotonic() + wpe::SPAWN_ALIVE_GRACE_SEC;
        loop {
            if let Some(rc) = self.sys.poll(proc) {
                return Some(rc);
            }
            let left = deadline - self.sys.monotonic();
            if left <= 0.0 {
                return None;
            }
            self.sys.sleep(wpe::ALIVE_POLL_STEP_SEC.min(left));
        }
    }

    fn succeeded(
        &mut self,
        step: &wpe::Step,
        path: &str,
        seen: &mut Seen,
        pid: Option<i64>,
    ) -> WallpaperResult {
        self.current_format = Some(step.format.clone());
        self.current_path = Some(path.to_string());
        let fmt = step.format.clone();
        // Каким путём поставлены обои — только для журнала: покупателю
        // служебное имя пути не показывается (`WALLPAPER_SET_MESSAGE`).
        log_line(
            "info",
            &format!("обои поставлены: путь {}, формат {fmt}", step.backend),
        );
        self.notify_live_wallpaper(Some(&fmt), true);
        let mut message = fill(&mut self.sys, &step.ok_message, seen);
        if let Some(pid) = pid {
            message = message.replace("{pid}", &pid.to_string());
        }
        WallpaperResult { ok: true, message, pid }
    }

    fn failed(
        &mut self,
        step: &wpe::Step,
        seen: &mut Seen,
        stderr: &str,
    ) -> Option<WallpaperResult> {
        if !step.terminal {
            return None;
        }
        let message = fill(&mut self.sys, &step.err_message, seen);
        Some(WallpaperResult::fail(message.replace("{stderr}", stderr)))
    }

    // ── общие мелочи прогона ──────────────────────────────────────────────

    /// Поднять демон swww/awww, если он ещё не отвечает.
    ///
    /// Подкоманда `init` в свежих версиях удалена — там демон запускают
    /// отдельным процессом, поэтому пробуем оба пути. Имена бинарей
    /// (`swww` → `awww` после переименования апстримом) разрешает общий
    /// резолвер инструментов, второй таблицы имён здесь нет.
    fn ensure_swww_daemon(&mut self) {
        let query = vec!["swww".to_string(), "query".to_string()];
        if let Some(r) = self.sys.run(&query, wpe::SWWW_QUERY_TIMEOUT_SEC) {
            if r.rc == 0 {
                return; // демон уже отвечает
            }
        }
        let init = vec!["swww".to_string(), "init".to_string()];
        if let Some(r) = self.sys.run(&init, wpe::SWWW_INIT_TIMEOUT_SEC) {
            if r.rc == 0 {
                return;
            }
        }
        if !self.sys.has_tool("swww-daemon") {
            return;
        }
        if self
            .sys
            .spawn(&["swww-daemon".to_string()], true)
            .is_ok()
        {
            self.sys.sleep(wpe::SWWW_DAEMON_SETTLE_SEC);
        }
    }

    /// Исполнить JS в plasmashell. `None` — не вышло; иначе напечатанное.
    fn plasma_evaluate(&mut self, script: &str) -> Option<String> {
        let argv = wpe::plasma_dbus_reply_argv(script);
        let out = self.sys.run(&argv, wpe::PLASMA_SCRIPT_TIMEOUT_SEC)?;
        if out.rc != 0 {
            return None;
        }
        Some(out.stdout)
    }

    /// Снять ранее запущенный процесс обоев, ничего больше не трогая.
    ///
    /// Отличие от остановки: та ещё и возвращает статичные обои, иконки
    /// стола и фоновые звуки. Здесь обои СРАЗУ сменяются новыми, поэтому
    /// откатывать нечего — иначе на смене мигал бы старый фон.
    fn kill_running(&mut self) {
        self.resume_if_paused();
        self.kill_companions();
        let Some(proc) = self.process.take() else {
            return;
        };
        if let Some(pid) = self.sys.pid(proc) {
            log_line(
                "info",
                &format!("останавливаю прежние обои PID={pid} перед запуском новых"),
            );
        }
        self.sys.kill_process(proc);
    }

    /// Снять процессы обоев на остальных мониторах.
    fn kill_companions(&mut self) {
        for proc in std::mem::take(&mut self.companions) {
            self.sys.kill_process(proc);
        }
    }

    /// Разморозить процесс обоев, если его заморозил надзор.
    fn resume_if_paused(&mut self) {
        if !self.perf_paused {
            return;
        }
        if let Some(proc) = self.process {
            self.sys.pause_process(proc, false);
        }
        self.pause_companions(false);
        self.perf_paused = false;
    }

    /// Заморозить или разморозить обои на остальных мониторах — то же, что
    /// надзор делает с главным процессом.
    fn pause_companions(&mut self, pause: bool) {
        for proc in self.companions.clone() {
            self.sys.pause_process(proc, pause);
        }
    }

    // ── надзор за нагрузкой: решения ──────────────────────────────────────

    /// За каким процессом следить надзору: дескриптор и PID. `None` — живых
    /// обоев своим процессом сейчас нет.
    pub fn monitor_target(&mut self) -> Option<(ProcHandle, i64)> {
        let proc = self.process?;
        if self.sys.poll(proc).is_some() {
            return None;
        }
        let pid = self.sys.pid(proc)?;
        Some((proc, pid))
    }

    /// Применить замер надзора — тело цикла `_performance_monitor_loop`.
    ///
    /// Порядок как у Python: процесс сменился — следить не за чем; упал —
    /// вернуть стол; процессор выше порога [`wpe::CPU_HIGH_STRIKES`] замеров
    /// подряд — принудительная остановка; разряжена батарея или полноэкранное
    /// окно — заморозить, причина ушла — разморозить.
    pub fn monitor_apply(
        &mut self,
        watched: ProcHandle,
        cpu_strikes: &mut i64,
        obs: &LoadObservation,
    ) -> MonitorVerdict {
        if self.process != Some(watched) {
            return MonitorVerdict::Done;
        }
        if let Some(rc) = self.sys.poll(watched) {
            self.on_process_crashed(rc);
            return MonitorVerdict::Done;
        }
        *cpu_strikes = if obs.cpu_high { *cpu_strikes + 1 } else { 0 };
        if *cpu_strikes >= wpe::CPU_HIGH_STRIKES {
            log_line(
                "error",
                "процесс обоев слишком долго грузит процессор выше порога — принудительная остановка",
            );
            self.stop(true);
            return MonitorVerdict::Done;
        }
        let should_pause = obs.battery_low || obs.fullscreen;
        if should_pause && !self.perf_paused {
            self.sys.pause_process(watched, true);
            self.pause_companions(true);
            self.perf_paused = true;
            let reason = if obs.battery_low { "низкий заряд" } else { "полноэкранное окно" };
            log_line("info", &format!("надзор за нагрузкой: пауза ({reason})"));
        } else if !should_pause && self.perf_paused {
            self.sys.pause_process(watched, false);
            self.pause_companions(false);
            self.perf_paused = false;
            log_line("info", "надзор за нагрузкой: возобновление");
        }
        MonitorVerdict::Continue
    }

    /// Процесс живых обоев упал сам — порт `_on_process_crashed`: вернуть
    /// стол в тот вид, в каком его вернула бы остановка по кнопке.
    fn on_process_crashed(&mut self, rc: i64) {
        log_line(
            "warning",
            &format!("процесс живых обоев упал (код {rc}) — возвращаю рабочий стол"),
        );
        if let Some(proc) = self.process.take() {
            // Процесс уже мёртв: снятие только забирает его статус.
            self.sys.kill_process(proc);
        }
        self.kill_companions();
        self.perf_paused = false;
        self.set_desktop_icons(true);
        self.restore_wallpaper();
        self.blur_path = None;
        self.current_format = None;
        self.current_path = None;
        self.current_name.clear();
        self.remove_pidfile();
        // Для слушателя моста падение — та же остановка, что и по кнопке.
        self.notify_live_wallpaper(None, true);
    }

    /// Снять удерживающий фон процесс (swaybg/wbg), если он наш.
    fn kill_static_holder(&mut self) {
        if let Some(proc) = self.static_holder.take() {
            self.sys.kill_process(proc);
        }
    }

    /// Сказать мосту, идут ли сейчас живые обои.
    ///
    /// Какой формат считается живым и когда весть подавляется порогом
    /// молчания — решает движок; здесь только доставка.
    fn notify_live_wallpaper(&mut self, fmt: Option<&str>, announce: bool) {
        let live = fmt.map(wpe::is_live_format).unwrap_or(false);
        let name = if live {
            self.current_name.trim().to_string()
        } else {
            String::new()
        };
        let now = self.sys.monotonic();
        let say = announce
            && wpe::bridge_should_announce(
                live,
                &name,
                self.bridge_live,
                &self.bridge_name,
                self.bridge_said_at,
                now,
            );
        // Ни вести, ни новости в признаке — мост не трогаем вовсе: иначе
        // каждая картинка плейлиста переписывала бы ему одно и то же.
        if !say && Some(live) == self.bridge_flag {
            return;
        }
        self.bridge_flag = Some(live);
        if say {
            self.bridge_live = live;
            self.bridge_name = name.clone();
            self.bridge_said_at = Some(now);
        }
        self.sys.bridge_notify(live, &name, say);
    }

    // ── применение и остановка ────────────────────────────────────────────

    /// Применить обои: путь и формат.
    ///
    /// `name` — название сцены ДЛЯ ЧЕЛОВЕКА, если вызывающий его знает.
    /// Пустое — честный ответ «не знаю»: выдумывать имя из имени файла
    /// нельзя, оно уедет прямо в чужую реплику через мост.
    ///
    /// ⚠️ ГРАНИЦА, НАЗВАННАЯ ВСЛУХ. Не перенесён только Flash (SWF) — на
    /// него исполнитель ОТВЕЧАЕТ строкой [`live_formats_not_carried`], а не
    /// делает вид, что обои встали (правило #13). Сцены, обои-приложения и
    /// незнакомый формат получают окончательный отказ теми же словами, что у
    /// Python, — см. [`carries_format`].
    ///
    /// Звук у видео и YouTube — умолчание [`VIDEO_AUDIO_DEFAULT`]; кто даёт
    /// человеку выбор, зовёт [`WallpaperExec::apply_with_audio`].
    /// Отказ, если слоем обоев владеет чужой шелл.
    ///
    /// ⚠️ Узел правила #12: зовут ОБА публичных входа применения
    /// ([`WallpaperExec::apply_with_audio`] — картинки, GIF, видео, веб,
    /// Flash; [`WallpaperExec::apply_youtube`]) и ДО всякого дела: прежние
    /// обои не снимаются, стол не запоминается. Положить обои под чужую
    /// поверхность и отрапортовать успех — ровно то, чего быть не должно;
    /// текст говорит человеку, что нажать («Отдать обои Rexon»).
    fn shell_owner_refusal(&mut self) -> Option<WallpaperResult> {
        let (de, session) = (self.de.clone(), self.session_type.clone());
        let shell = self.sys.wallpaper_shell_owner(&de, &session)?;
        log_line(
            "info",
            &format!("обои не применены: слой обоев держит шелл {}", shell.as_str()),
        );
        Some(WallpaperResult::fail(wpe::shell_owner_message(shell)))
    }

    pub fn apply(&mut self, path: &str, fmt: &str, name: &str) -> WallpaperResult {
        self.apply_with_audio(path, fmt, name, VIDEO_AUDIO_DEFAULT)
    }

    /// Применить обои с выбором звука — порт `apply(path, fmt, audio=…)`.
    ///
    /// Путь один на всех вызывающих: `apply` — это он же с умолчанием.
    /// `audio` действует на видео из файла и на YouTube по умолчаниям
    /// качества и режима (у YouTube с выбором качества и режима — свой вход
    /// [`WallpaperExec::apply_youtube`], звук там тем же аргументом); прочим
    /// форматам звук не нужен, и они его не читают. `false` — ролик идёт
    /// немым (`--no-audio` хосту либо расширению оболочки).
    ///
    /// ⚠️ Flash (`swf`) этот аргумент НЕ читает, и это не пропуск. `audio`
    /// здесь значит «у ЭТОЙ обоины звук предусмотрен» и приходит из
    /// `project.json` видео-обоины либо из галочки фона Steam; у SWF нет ни
    /// того, ни другого — Flash-файл кладут как есть. Звук Flash-обоев
    /// целиком живёт в общих настройках (`wallpaper_audio_*`) и меняется на
    /// ходу, а разовое «эту обоину — немой» задаётся ключом `--no-audio`
    /// самому хосту.
    pub fn apply_with_audio(
        &mut self,
        path: &str,
        fmt: &str,
        name: &str,
        audio: bool,
    ) -> WallpaperResult {
        self.we_note = None;
        let mut res = self.apply_with_audio_inner(path, fmt, name, audio);
        if res.ok && fmt == "static" {
            self.sys.static_wallpaper_changed(path);
        }
        self.attach_we_note(&mut res);
        self.attach_desktop_icons_note(&mut res, fmt);
        res
    }

    /// Живые обои окном хоста на X11 легли над окном значков стола или под
    /// ним — подготовить стол и сказать человеку, что со значками
    /// (`wallpaper_backend::prepare_desktop_icons`). Только для обоев окном
    /// хоста: картинки окна не создают, а GNOME/Cinnamon-Wayland показывают
    /// обои расширением оболочки.
    fn attach_desktop_icons_note(&mut self, res: &mut WallpaperResult, fmt: &str) {
        if !res.ok || !matches!(fmt, "video" | "web" | "swf") || wpe::needs_shell_extension(&self.de, &self.session_type) {
            return;
        }
        let Some(owner) = crate::wallpaper_backend::desktop_icons_owner(&self.de, &self.session_type) else {
            return;
        };
        let mut io = DesktopIconsSys::new(&mut self.sys);
        if let Some(note) = crate::wallpaper_backend::prepare_desktop_icons(&mut io, owner) {
            res.message = format!("{} {note}", res.message).trim().to_string();
        }
    }

    /// Снять живые WE-обои перед своими (см. [`yields_we_wallpapers`]) и
    /// запомнить приписку, если снять не вышло.
    fn yield_we_wallpapers(&mut self, fmt: &str) {
        if !yields_we_wallpapers(fmt) {
            return;
        }
        let Some((status, killed)) = self.sys.stop_we_wallpapers(crate::we_client::STOP_TIMEOUT_MS)
        else {
            return;
        };
        if status == crate::we_client::STOP_STOPPED && killed > 0 {
            log_line("info", &format!("сняты живые обои Wallpaper Engine: {killed}"));
        }
        self.we_note = we_yield_note(&status);
        if self.we_note.is_some() {
            log_line("warn", "живые обои Wallpaper Engine снять не удалось — свои лягут под них");
        }
    }

    /// Приписать к удачному итогу «WE-обоя осталась поверх».
    fn attach_we_note(&mut self, res: &mut WallpaperResult) {
        if let Some(note) = self.we_note.take() {
            if res.ok {
                res.message = format!("{} {note}", res.message).trim().to_string();
            }
        }
    }

    /// Дойдёт ли применение до дела: без файла или хоста обоев — отказ, и
    /// чужие обои ради отказа не снимаются.
    fn apply_can_start(&mut self, path: &str, fmt: &str) -> bool {
        match fmt {
            "static" | "animated" => self.sys.exists(path),
            "video" => self.sys.exists(path) && self.sys.wallpaper_host().is_some(),
            "swf" => {
                self.sys.wallpaper_host().is_some() && self.sys.exists(path) && !self.sys.is_dir(path)
            }
            "web" => self.sys.wallpaper_host().is_some(),
            _ => true,
        }
    }

    fn apply_with_audio_inner(
        &mut self,
        path: &str,
        fmt: &str,
        name: &str,
        audio: bool,
    ) -> WallpaperResult {
        self.current_name = name.to_string();

        if !carries_format(fmt) {
            return WallpaperResult::fail(format!(
                "{}: {}",
                wpe::format_label(fmt),
                live_formats_not_carried()
            ));
        }
        // ⭐ Починка при переносе: Python отвечал этот отказ ПОСЛЕ того, как
        // снял прежние живые обои и запомнил стол, — плитка сцены из
        // библиотеки молча гасила работающее видео. Отказ — до всякого дела.
        if let Some(refusal) = retired_format_refusal(fmt) {
            log_line("info", &format!("обои не применены: формат={fmt}: {refusal}"));
            return WallpaperResult::fail(refusal);
        }
        if let Some(refusal) = self.shell_owner_refusal() {
            return refusal;
        }
        // YouTube снимает их сам (`apply_youtube`): у него свои проверки.
        if fmt != "youtube" && self.apply_can_start(path, fmt) {
            self.yield_we_wallpapers(fmt);
        }
        if fmt == "web" {
            return self.apply_web(path);
        }
        if fmt == "youtube" {
            // Без выбора качества и режима — умолчания Python: 1080p, поток.
            return self.apply_youtube(path, "1080p", "stream", audio);
        }
        if fmt == "swf" {
            return self.apply_swf(path);
        }
        if fmt == "video" {
            if !self.sys.exists(path) {
                return WallpaperResult::fail(wpe::file_missing_message(path));
            }
            if self.process.is_some() {
                self.stop(false);
            }
            self.save_current_wallpaper();
            log_line(
                "info",
                &format!(
                    "применяю видеообои: окружение={}, сессия={}, путь={path}",
                    self.de, self.session_type
                ),
            );
            let res = self.apply_video(path, audio);
            if res.ok {
                self.write_pidfile();
            }
            return res;
        }
        if !self.sys.exists(path) {
            return WallpaperResult::fail(wpe::file_missing_message(path));
        }
        if fmt == "animated" {
            return self.apply_animated(path);
        }
        // Прежние живые обои снимаем БЕЗ вести мосту: обои сейчас же
        // сменятся новыми, и пара «встали — пошли» слушателя обманывала бы.
        if self.process.is_some() {
            self.stop(false);
        }
        self.save_current_wallpaper();

        log_line(
            "info",
            &format!(
                "применяю обои: формат={fmt}, окружение={}, сессия={}, путь={path}",
                self.de, self.session_type
            ),
        );
        let tools = self.plan_tools();
        let hint = self.sys.package_hint("swww");
        let plan = wpe::static_plan(&self.de, &self.static_session(), path, &tools, &hint);
        let res = self.run_plan(&plan, path);
        if res.ok {
            self.write_pidfile();
        }
        res
    }

    /// Корень кеша пользователя (`$XDG_CACHE_HOME`, иначе `~/.cache`).
    fn cache_home(&mut self) -> String {
        let c = self.sys.env("XDG_CACHE_HOME");
        if c.is_empty() {
            format!("{}/.cache", self.home())
        } else {
            c
        }
    }

    /// Анимация GIF/APNG — порт `_apply_animated`: размытая подложка, затем
    /// план движка (`animated_plan`: swww, иначе mpvpaper на Wayland;
    /// xwinwrap + mpv на X11). Надзора за нагрузкой у GIF нет — как у Python.
    ///
    /// ⭐ Починка при переносе: там, где план — один отказ (GNOME и
    /// Cinnamon-Wayland: слоя стола для swww и mpvpaper нет), Python успевал
    /// снять прежние живые обои и поставить размытую подложку, а потом
    /// отказывал — стол оставался размытым кадром GIF. Здесь отказ говорится
    /// до всякого дела.
    fn apply_animated(&mut self, path: &str) -> WallpaperResult {
        let tools = self.plan_tools();
        let hints = self.hints();
        let plan = wpe::animated_plan(
            &self.de,
            &self.session_type,
            path,
            &tools,
            "{monitor}",
            wpe::GEOMETRY_MARK,
            &hints,
        );
        if plan.first().is_some_and(|s| s.kind == wpe::step_kind::FAIL) {
            return self.run_plan(&plan, path);
        }
        // Прежние живые обои — без вести мосту: их сейчас же сменят.
        if self.process.is_some() {
            self.stop(false);
        }
        self.save_current_wallpaper();
        log_line(
            "info",
            &format!(
                "применяю GIF-обои: окружение={}, сессия={}, путь={path}",
                self.de, self.session_type
            ),
        );
        self.apply_blur_background(path);
        let res = self.run_plan(&plan, path);
        if res.ok {
            self.write_pidfile();
        }
        res
    }

    /// Видеообои своим процессом-хостом — порт `_apply_video`.
    ///
    /// Хост ищется ПЕРВЫМ: не нашли — показывать нечем ни своим окном, ни
    /// расширением оболочки, и готовить файл впустую незачем. Дальше, как у
    /// Python: проверка файла, faststart-ремукс, запоминание картинки стола
    /// KDE, размытая подложка, показ — расширением оболочки там, где
    /// wlr-layer-shell нет (GNOME, Cinnamon-Wayland), иначе своим окном.
    /// Надзор за нагрузкой поднимает вызывающий (`spawn_load_monitor`).
    fn apply_video(&mut self, path: &str, has_audio: bool) -> WallpaperResult {
        let Some(host) = self.sys.wallpaper_host() else {
            log_line("error", &wpe::video_host_missing_message());
            return WallpaperResult::fail(wpe::video_host_missing_message());
        };
        let exists = self.sys.exists(path) && !self.sys.is_dir(path);
        let size = self.sys.file_size(path).unwrap_or(0);
        let (ok, message) = wpe::validate_video(path, exists, size, wpe::MAX_VIDEO_FILE_MB);
        if !ok {
            return WallpaperResult::fail(message);
        }
        let original_name = path.rsplit('/').next().unwrap_or(path).to_string();
        let path = self.sys.ensure_faststart(path);

        // KDE: запомнить настоящую картинку стола ДО подложки, чтобы
        // остановка вернула её, а не размытый кадр.
        if wpe::uses_plasma_config(&self.de) && self.saved_wallpaper.is_none() {
            if let Some(image) = self.kde_current_static_image() {
                let exists = self.sys.exists(&image);
                if wpe::clean_image_candidate_ok(&image, exists) {
                    self.saved_wallpaper = Some(image);
                    self.saved_de = Some(self.de.clone());
                }
            }
        }

        self.apply_blur_background(&path);

        let res = if wpe::needs_shell_extension(&self.de, &self.session_type) {
            self.video_shell_extension(&path, &host, has_audio)
        } else {
            let plan = wpe::video_native_plan(&host, &path, has_audio);
            self.run_plan(&plan, &path)
        };
        // Играет служебная копия с перенесённым заголовком (имя — отпечаток
        // вроде `63f038e7….mp4`); человеку называем его собственный файл.
        let mut res = res;
        let played_name = path.rsplit('/').next().unwrap_or(&path).to_string();
        if played_name != original_name {
            res.message = res.message.replace(&played_name, &original_name);
        }
        if res.ok {
            self.current_format = Some("video".to_string());
            self.current_path = Some(path);
        }
        res
    }

    /// YouTube-обои — порт `_apply_youtube`: качество, режим (поток либо
    /// скачивание) и звук выбирает человек.
    ///
    /// Режим «поток» там, где он неисполним (GNOME, Cinnamon-Wayland — обои
    /// показывает расширение оболочки, ему нужен файл), становится
    /// скачиванием, и человеку говорится почему. Поток играет mpv/mpvpaper
    /// планом движка; скачанный ролик ставится тем же путём, что видео из
    /// файла. Надзор за нагрузкой поднимает вызывающий.
    pub fn apply_youtube(
        &mut self,
        url: &str,
        quality: &str,
        mode: &str,
        audio: bool,
    ) -> WallpaperResult {
        self.we_note = None;
        let mut res = self.apply_youtube_inner(url, quality, mode, audio);
        self.attach_we_note(&mut res);
        res
    }

    fn apply_youtube_inner(
        &mut self,
        url: &str,
        quality: &str,
        mode: &str,
        audio: bool,
    ) -> WallpaperResult {
        let url = url.trim();
        if let Some(refusal) = self.shell_owner_refusal() {
            return refusal;
        }
        if !self.sys.has_tool("yt-dlp") {
            let hint = self.sys.package_hint("yt-dlp");
            return WallpaperResult::fail(wpe::youtube_no_ytdlp_message(&hint));
        }
        if !crate::youtube_wallpaper::is_supported_url(url) {
            return WallpaperResult::fail(wpe::youtube_unsupported_message(url));
        }
        self.yield_we_wallpapers("youtube");
        let quality = if quality.trim().is_empty() { "1080p" } else { quality };
        let effective = wpe::youtube_effective_mode(&self.de, &self.session_type, mode);
        let note = wpe::youtube_mode_downgraded_note(&self.de, &self.session_type, mode);
        if !note.is_empty() {
            log_line("info", &format!("YouTube: {note}"));
        }
        if self.process.is_some() {
            self.stop(false);
        }
        self.save_current_wallpaper();
        // Все окружения: скрыть иконки стола (остановка их вернёт).
        self.set_desktop_icons(false);

        if effective == "download" {
            log_line("info", &format!("YouTube: скачиваю, качество {quality}"));
            let Some(local) = self.sys.download_youtube(url, quality) else {
                return WallpaperResult::fail(wpe::youtube_download_failed_message(
                    &self.de,
                    &self.session_type,
                ));
            };
            let mut res = self.apply_video(&local, audio);
            if res.ok {
                self.write_pidfile();
                // Просили поток, а получили скачивание — сказать почему.
                if !note.is_empty() {
                    res.message = format!("{} {note}", res.message);
                }
            }
            return res;
        }

        log_line("info", &format!("YouTube: поток, качество {quality}"));
        let tools = self.plan_tools();
        let hints = self.hints();
        let plan = wpe::youtube_plan(
            &self.de,
            &self.session_type,
            url,
            audio,
            YOUTUBE_STREAM_VOLUME,
            quality,
            &tools,
            "{monitor}",
            wpe::GEOMETRY_MARK,
            &hints,
        );
        let res = self.run_plan(&plan, url);
        if res.ok {
            self.write_pidfile();
        }
        res
    }

    /// Размытый кадр живых обоев фоном под ними — порт
    /// `apply_blur_background` (у Python включён по умолчанию). Не вышло —
    /// не беда: живые обои от этого не зависят.
    fn apply_blur_background(&mut self, source: &str) {
        let cache = self.cache_home();
        let frame = format!("{cache}/rexon/blur/frame.png");
        // С 3-й секунды — мимо заставок; ролик короче — с начала.
        if !self.sys.extract_frame(source, &frame, 3.0)
            && !self.sys.extract_frame(source, &frame, 0.0)
        {
            return;
        }
        let output = wpe::blur_cache_output_path(&cache, &frame);
        if let Err(e) = self.sys.blur_image(&frame, &output, BLUR_BACKDROP_RADIUS) {
            log_line("warning", &format!("размытая подложка не вышла: {e}"));
            return;
        }
        let tools = self.plan_tools();
        let hint = self.sys.package_hint("swww");
        let plan = wpe::static_plan(&self.de, &self.static_session(), &output, &tools, &hint);
        if self.run_plan(&plan, &output).ok {
            self.blur_path = Some(output);
        }
    }

    /// Отдать видео расширению оболочки через его же настройки — порт
    /// `_video_shell_extension`. Своего процесса здесь нет: хост запускает
    /// само расширение по готовой строке запуска.
    fn video_shell_extension(&mut self, path: &str, host: &str, has_audio: bool) -> WallpaperResult {
        let plan = wpe::shell_ext_plan(&self.de, path, host, has_audio);
        self.run_shell_extension(plan, path, "video")
    }

    /// Передать обои расширению оболочки — ОДИН путь для видео и Flash:
    /// проверка схемы, `engine`, строка запуска демона, весть мосту.
    fn run_shell_extension(
        &mut self,
        plan: Option<wpe::ShellExtPlan>,
        path: &str,
        fmt: &str,
    ) -> WallpaperResult {
        let Some(plan) = plan else {
            return WallpaperResult::fail(crate::i18n::tr(
                "This desktop environment does not need the shell extension",
            ));
        };
        let ran_ok = |sys: &mut S, argv: &[String]| {
            let timeout = utils::default_timeout_secs(argv) as f64;
            matches!(sys.run(argv, timeout), Some(ref out) if out.rc == 0)
        };
        // Пишем через схему из каталога самого расширения, если он есть:
        // в общем каталоге схем её может не оказаться (см.
        // `wpe::shell_ext_schema_dirs`).
        let schema_dir = self.shell_extension_schema_dir();
        let gsettings = |cmd: &[String]| match &schema_dir {
            Some(dir) => wpe::with_schemadir(cmd, dir),
            None => cmd.to_vec(),
        };
        // Схема есть только у установленного расширения — это и проверка.
        if !ran_ok(&mut self.sys, &gsettings(&plan.source_argv)) {
            return WallpaperResult::fail(plan.not_installed_message);
        }
        ran_ok(&mut self.sys, &gsettings(&plan.engine_argv));
        // Итог строки запуска ОБЯЗАТЕЛЬНО проверяется: у устаревшего
        // расширения ключа нет, и запасного пути у него больше нет.
        if !ran_ok(&mut self.sys, &gsettings(&plan.daemon_argv)) {
            log_line("error", "расширение оболочки не приняло строку запуска хоста");
            return WallpaperResult::fail(plan.daemon_failed_message);
        }
        self.current_format = Some(fmt.to_string());
        self.current_path = Some(path.to_string());
        // Своего процесса нет и план не исполнялся — мосту говорим сами.
        self.notify_live_wallpaper(Some(fmt), true);
        let state = self.shell_extension_state();
        match wpe::shell_ext_state_message(state, &plan.disabled_message) {
            None => WallpaperResult::ok(plan.ok_message),
            Some((ok, message)) => {
                log_line("warning", &format!("расширение оболочки не работает: {state:?}"));
                if ok {
                    WallpaperResult::ok(message)
                } else {
                    WallpaperResult::fail(message)
                }
            }
        }
    }

    /// Flash-обои (SWF) — режим `rexon-wallpaper-host --mode swf`: Ruffle как
    /// библиотека (крейт `rexon-swf`), без внешнего `ruffle`, скачивания и
    /// HTML-обёртки классического окна.
    ///
    /// Всё проверяется ДО снятия прежних обоев: хост (нет — показывать
    /// нечем) и файл (нет или это каталог — отказ с именем). Дальше как у
    /// видео: окно слоя или X11 своим сеансом, на GNOME и Cinnamon-Wayland —
    /// расширение оболочки, которому хост пишет кадры в файл. Размытой
    /// подложки нет: кадра до старта Ruffle не существует. Надзор за нагрузкой
    /// поднимает вызывающий ([`format_wants_load_monitor`]).
    fn apply_swf(&mut self, path: &str) -> WallpaperResult {
        let Some(host) = self.sys.wallpaper_host() else {
            log_line("error", &wpe::video_host_missing_message());
            return WallpaperResult::fail(wpe::video_host_missing_message());
        };
        if !self.sys.exists(path) || self.sys.is_dir(path) {
            return WallpaperResult::fail(wpe::swf_missing_message(path));
        }
        // Прежние живые обои — без вести мосту: их сейчас же сменят.
        if self.process.is_some() {
            self.stop(false);
        }
        self.save_current_wallpaper();
        log_line(
            "info",
            &format!(
                "применяю Flash-обои: окружение={}, сессия={}, путь={path}",
                self.de, self.session_type
            ),
        );
        let res = if wpe::needs_shell_extension(&self.de, &self.session_type) {
            let plan = wpe::swf_shell_ext_plan(&self.de, path, &host);
            self.run_shell_extension(plan, path, "swf")
        } else {
            let plan = wpe::swf_host_plan(&host, path);
            self.run_plan(&plan, path)
        };
        if res.ok {
            self.current_format = Some("swf".to_string());
            self.current_path = Some(path.to_string());
            self.write_pidfile();
        }
        res
    }

    /// Каталог собранной схемы нашего расширения — первый из
    /// [`wpe::shell_ext_schema_dirs`], где лежит `gschemas.compiled`.
    /// `None` — ни одного: пишем голым `gsettings` (схема, может быть, в
    /// общем каталоге, куда её кладёт пакет).
    fn shell_extension_schema_dir(&mut self) -> Option<String> {
        let home = self.sys.env("HOME");
        let data_home = self.sys.env("XDG_DATA_HOME");
        let data_dirs = self.sys.env("XDG_DATA_DIRS");
        wpe::shell_ext_schema_dirs(&self.de, &home, &data_home, &data_dirs)
            .into_iter()
            .find(|dir| self.sys.exists(&format!("{dir}/gschemas.compiled")))
    }

    /// В каком состоянии наше расширение оболочки. Спросить не удалось —
    /// так и говорим ([`wpe::ShellExtState::Unknown`]): прежде здесь стояло
    /// «проверить нечем — считаем включённым», и человек получал «готово»
    /// при пустом столе.
    fn shell_extension_state(&mut self) -> wpe::ShellExtState {
        let argv = wpe::shell_ext_state_argv(&self.de);
        let timeout = utils::default_timeout_secs(&argv) as f64;
        let out = self.sys.run(&argv, timeout).filter(|o| o.rc == 0);
        wpe::parse_shell_ext_state(&self.de, out.as_ref().map(|o| o.stdout.as_str()))
    }

    /// Веб-обои: адрес (`http(s)://`, `file://`) либо локальный HTML-файл —
    /// порт `WallpaperApplier._apply_web` вместе с веткой `apply` для веба.
    ///
    /// Своим сеансом поднимается хост `rexon-wallpaper-host --mode web` (план движка
    /// `web_plan`); нет хоста — план сам отвечает честным отказом до запуска.
    fn apply_web(&mut self, path: &str) -> WallpaperResult {
        // ⭐ Починка: окно превращает локальный путь в `file://…`
        // (`qt_settings::web_wallpaper_url`), а проверка существования у
        // Python смотрела на саму эту строку — такого файла нет, и локальный
        // HTML-файл всегда получал «не найден». Проверяется путь без схемы.
        let local = path.strip_prefix("file://").unwrap_or(path);
        if !wpe::is_url(path) && (!self.sys.exists(local) || self.sys.is_dir(local)) {
            return WallpaperResult::fail(wpe::web_missing_message(local));
        }
        // Прежние живые обои снимаются без вести мосту: их сейчас же сменят.
        if self.process.is_some() {
            self.stop(false);
        }
        self.save_current_wallpaper();
        log_line(
            "info",
            &format!("применяю веб-обои: сессия={}, адрес={path}", self.session_type),
        );
        let tools = self.plan_tools();
        let hints = self.hints();
        let host = self.sys.wallpaper_host();
        // Абсолютный путь нужен только запасному пути surf для локального
        // файла; окно отдаёт сюда уже готовый адрес, путь — как есть.
        let plan = wpe::web_plan(
            &self.session_type,
            "",
            path,
            path,
            &tools,
            wpe::GEOMETRY_MARK,
            &hints,
            host.as_deref(),
        );
        let res = self.run_plan(&plan, path);
        if res.ok {
            self.write_pidfile();
        }
        res
    }

    /// Остановить живые обои и вернуть рабочий стол в исходный вид.
    ///
    /// `announce` — рассказывать ли мосту об остановке. Ложь означает
    /// «обои снимаются под замену»: признак у моста обновится, вести не
    /// будет.
    pub fn stop(&mut self, announce: bool) -> WallpaperResult {
        // Формат снимаем ПЕРВЫМ делом, до всякого возврата: возврат ставит
        // статичную картинку тем же планом, что и обычное применение, а тот
        // записывает в состояние свой формат — и собранное в конце сообщение
        // называло бы `static` даже после видео.
        let fmt = self.current_format.clone();

        // Замороженный надзором процесс сперва размораживается: SIGTERM до
        // остановленного процесса не доходит, и остановка ждала бы SIGKILL.
        self.resume_if_paused();
        if let Some(proc) = self.process.take() {
            self.sys.kill_process(proc);
        }
        // Обои на остальных мониторах — часть тех же обоев.
        self.kill_companions();

        // Фоновые звуки — часть тех же обоев: оставить их играть на голом
        // столе значило бы остановить обои наполовину.
        self.stop_all_ambient();

        // Все окружения: вернуть иконки рабочего стола.
        self.set_desktop_icons(true);
        // X11: вернуть стол, как был до живых обоев (сквозной фон Caja,
        // композитор Marco). Только по «Остановить» человека: остановка
        // перед сменой обоев (`announce == false`) переключала бы окно стола
        // Caja туда-обратно на каждой смене.
        if announce {
            crate::wallpaper_backend::restore_desktop_icons(&mut DesktopIconsSys::new(&mut self.sys));
        }

        // KDE: снять с десктоп-контейнментов УДАЛЁННЫЙ плагин обоев Rexon и
        // вернуть штатный. У обновившегося со старой версии он мог остаться
        // выбранным — тогда Plasma грузит несуществующий плагин, и рабочий
        // стол остаётся пустым.
        if wpe::uses_plasma_config(&self.de) && self.kde_stale_rexon_plugin() {
            let clean = self.kde_clean_static_image();
            let script = wpe::kde_reset_script(&clean);
            let argv = wpe::plasma_dbus_argv(&script);
            self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC);
        }

        // GNOME/Cinnamon-Wayland: демоном владеет расширение оболочки, а не
        // мы — снять его можно только через его же настройки, иначе обои
        // продолжали бы играть после «Остановить».
        if wpe::needs_shell_extension(&self.de, &self.session_type) {
            if let Some(argv) = wpe::shell_ext_stop_argv(&self.de) {
                let argv = match self.shell_extension_schema_dir() {
                    Some(dir) => wpe::with_schemadir(&argv, &dir),
                    None => argv,
                };
                let timeout = utils::default_timeout_secs(&argv) as f64;
                self.sys.run(&argv, timeout);
            }
        }

        self.restore_wallpaper();
        // …и, если столом до нас владел другой движок обоев, вернуть ему
        // стол. ПОСЛЕ возврата картинки: тот на KDE безусловно ставит
        // org.kde.image, и обратный порядок затёр бы возврат плагина.
        self.restore_kde_wallpaper_plugin();
        self.blur_path = None;

        self.current_format = None;
        self.current_path = None;
        if announce {
            // Под замену название уже принадлежит НОВЫМ обоям: применение
            // выставляет его до того, как снять прежние. Стереть здесь —
            // значит потерять его к моменту вести о запуске.
            self.current_name.clear();
        }
        self.remove_pidfile();

        // Мост: живых обоев больше нет.
        self.notify_live_wallpaper(None, announce);

        WallpaperResult::ok(wpe::stopped_message(fmt.as_deref()))
    }

    /// Снимок состояния движка — то, чем живут кнопки вкладки.
    pub fn status(&mut self) -> Status {
        let running = match self.process {
            Some(p) => self.sys.poll(p).is_none(),
            None => false,
        };
        let pid = if running {
            self.process.and_then(|p| self.sys.pid(p))
        } else {
            None
        };
        let ambient = self.active_ambient();
        Status {
            running,
            format: self.current_format.clone(),
            path: self.current_path.clone(),
            de: self.de.clone(),
            session_type: self.session_type.clone(),
            pid,
            blur_active: self.blur_path.is_some(),
            ambient,
        }
    }

    // ── фоновые звуки ─────────────────────────────────────────────────────

    /// Запустить фоновый звук. Истина — только если он ЗАЗВУЧАЛ.
    ///
    /// ⚠️ Мерило успеха здесь НЕ то же, что у обоев, и это принципиально. У
    /// обоев процесс рисует стол, пока жив, — там мерило «пережил
    /// отсрочку». Звук так мерить нельзя: разовый короче отсрочки честно
    /// кончается сам, и правило «жив через полсекунды» дало бы ЛОЖНЫЙ отказ
    /// на исправном звуке. Поэтому мерило — соответствие ожиданию:
    ///
    ///   * зацикленный обязан играть и дальше — успех только «жив»;
    ///   * разовый обязан играть ИЛИ честно доиграть — успех «жив или
    ///     вышел с нулём».
    ///
    /// Сама отсрочка одна и та же и берётся из движка: битый файл отдаёт
    /// отказ за свою десятую долю секунды, полную отсрочку платит только
    /// тот, кто играет.
    pub fn play_ambient(
        &mut self,
        id: &str,
        path: &str,
        name: &str,
        volume: i64,
        looped: bool,
    ) -> bool {
        if !self.sys.exists(path) || self.sys.is_dir(path) {
            log_line("error", &format!("звуковой файл не найден: {path}"));
            return false;
        }
        if !self.sys.has_tool("mpv") {
            log_line("error", "mpv не установлен — фоновые звуки недоступны");
            return false;
        }
        // Прежний экземпляр этого же звука снимаем: иначе два mpv играли бы
        // одно и то же поверх друг друга.
        self.stop_ambient(id);

        let argv = wpe::ambient_argv(path, volume, looped);
        let proc = match self.sys.spawn(&argv, false) {
            Ok(p) => p,
            Err(e) => {
                log_line("error", &format!("фоновый звук не запустился: {path}: {e}"));
                return false;
            }
        };
        let rc = self.wait_alive(proc);
        if let Some(rc) = rc {
            if looped || rc != 0 {
                log_line(
                    "error",
                    &format!("фоновый звук не пошёл: {path} (зацикленный={looped}, код {rc})"),
                );
                self.sys.kill_process(proc);
                return false;
            }
        }
        let shown = if name.is_empty() {
            path.rsplit('/').next().unwrap_or(path).to_string()
        } else {
            name.to_string()
        };
        if rc.is_none() {
            log_line("info", &format!("фоновый звук запущен: {shown}"));
        } else {
            log_line("info", &format!("фоновый звук доиграл до конца: {shown}"));
        }
        self.ambient.push(Ambient {
            id: id.to_string(),
            path: path.to_string(),
            name: shown,
            volume,
            // Режим нужен смене громкости: mpv меняет её только
            // перезапуском, и перезапускать надо в ТОМ ЖЕ режиме — иначе
            // разовый звук после правки громкости зацикливался навсегда.
            looped,
            proc,
        });
        true
    }

    /// Остановить один фоновый звук.
    pub fn stop_ambient(&mut self, id: &str) {
        let Some(idx) = self.ambient.iter().position(|a| a.id == id) else {
            return;
        };
        let sound = self.ambient.remove(idx);
        self.sys.kill_process(sound.proc);
        log_line("info", &format!("фоновый звук остановлен: {}", sound.name));
    }

    /// Остановить все фоновые звуки.
    pub fn stop_all_ambient(&mut self) {
        let ids: Vec<String> = self.ambient.iter().map(|a| a.id.clone()).collect();
        for id in ids {
            self.stop_ambient(&id);
        }
    }

    /// Сменить громкость звука — перезапуском в том же режиме.
    pub fn set_ambient_volume(&mut self, id: &str, volume: i64) -> bool {
        let Some(sound) = self.ambient.iter().find(|a| a.id == id) else {
            return false;
        };
        let (path, name, looped) = (sound.path.clone(), sound.name.clone(), sound.looped);
        self.stop_ambient(id);
        self.play_ambient(id, &path, &name, volume, looped)
    }

    /// Играет ли звук прямо сейчас.
    pub fn ambient_playing(&mut self, id: &str) -> bool {
        let Some(proc) = self.ambient.iter().find(|a| a.id == id).map(|a| a.proc) else {
            return false;
        };
        self.sys.poll(proc).is_none()
    }

    /// Список играющих звуков; доигравшие вычищаются из состояния.
    pub fn active_ambient(&mut self) -> Vec<AmbientInfo> {
        let all: Vec<(String, String, i64, ProcHandle)> = self
            .ambient
            .iter()
            .map(|a| (a.id.clone(), a.name.clone(), a.volume, a.proc))
            .collect();
        let mut alive = Vec::new();
        let mut out = Vec::new();
        for (id, name, volume, proc) in all {
            if self.sys.poll(proc).is_some() {
                continue; // доиграл — в списке ему не место
            }
            alive.push(id.clone());
            out.push(AmbientInfo { id, name, volume, active: true });
        }
        self.ambient.retain(|a| alive.contains(&a.id));
        out
    }

    // ── что вернуть после остановки ───────────────────────────────────────

    /// Запомнить обои, которые стоят сейчас, — их вернёт остановка.
    fn save_current_wallpaper(&mut self) {
        // Плагин стола KDE запоминается ОТДЕЛЬНО от картинки: при чужом
        // плагине статичной картинки в конфиге нет вовсе, и ранний выход
        // ниже потерял бы владельца стола.
        self.save_kde_wallpaper_plugin();

        if self.saved_wallpaper.is_some() {
            return; // уже сохранены
        }
        self.saved_de = Some(self.de.clone());
        if self.de == "cosmic" {
            // Тот же ключ, в который COSMIC-ветка пишет картинку.
            let all = format!("{}/{}/all", self.config_home(), wpe::COSMIC_BG_DIR_REL);
            self.saved_wallpaper = self.sys.read_text(&all).and_then(|t| wpe::cosmic_bg_source_path(&t));
            return;
        }
        let Some((schema, key)) = wpe::gsettings_schema(&self.de) else {
            return;
        };
        let argv = wpe::gsettings_get_argv(schema, key);
        let timeout = utils::default_timeout_secs(&argv) as f64;
        if let Some(out) = self.sys.run(&argv, timeout) {
            let raw = out.stdout.trim().trim_matches(['\'', '"']).to_string();
            if !raw.is_empty() {
                self.saved_wallpaper = Some(raw.replace("file://", ""));
            }
        }
    }

    /// Вернуть сохранённые обои.
    fn restore_wallpaper(&mut self) {
        let Some(path) = self.saved_wallpaper.take() else {
            return;
        };
        let de = self.saved_de.take().unwrap_or_else(|| self.de.clone());

        // KDE: сбросить плагин стола на штатный (если стоял наш).
        if wpe::uses_plasma_config(&de) {
            let argv = wpe::plasma_dbus_argv(wpe::KDE_PLAIN_RESET_SCRIPT);
            self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC);
        }

        if !self.sys.exists(&path) {
            return;
        }
        let tools = self.plan_tools();
        let hint = self.sys.package_hint("swww");
        let plan = wpe::restore_static_plan(&de, &path, &tools, &hint);
        if !plan.is_empty() {
            self.run_plan(&plan, &path);
        }
    }

    /// Ответ plasmashell: какие плагины обоев стоят на ВСЕХ столах.
    ///
    /// Единственное место, где исполнитель спрашивает про плагин стола: и
    /// «чей стол, кому его вернуть», и «не остался ли удалённый плагин» —
    /// оба ответа движок считает из ЭТОЙ строки. Читаем у самой Plasma, а
    /// не из конфига: файл на диске отстаёт, plasmashell держит настройки в
    /// памяти и пишет их когда захочет. Молчание отдаём пустой строкой —
    /// за «плагинов нет» его не выдаёт никто.
    fn kde_plugin_reply(&mut self) -> String {
        let argv = wpe::plasma_dbus_reply_argv(wpe::KDE_READ_PLUGIN_SCRIPT);
        match self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC) {
            Some(out) if out.rc == 0 => out.stdout,
            _ => String::new(),
        }
    }

    /// Запомнить ЧУЖОЙ плагин обоев стола, если он там стоял.
    fn save_kde_wallpaper_plugin(&mut self) {
        if !wpe::uses_plasma_config(&self.de) || self.saved_kde_plugin.is_some() {
            return;
        }
        let reply = self.kde_plugin_reply();
        if let Some(plugin) = wpe::kde_foreign_wallpaper_plugin(&reply) {
            log_line(
                "info",
                &format!("стол KDE занят плагином {plugin} — вернём его при остановке"),
            );
            self.saved_kde_plugin = Some(plugin);
        }
    }

    /// Вернуть столу KDE прежний чужой плагин обоев.
    fn restore_kde_wallpaper_plugin(&mut self) {
        let Some(plugin) = self.saved_kde_plugin.take() else {
            return;
        };
        let script = wpe::kde_restore_plugin_script(&plugin);
        let argv = wpe::plasma_dbus_argv(&script);
        match self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC) {
            Some(out) if out.rc == 0 => {
                log_line("info", &format!("столу KDE возвращён плагин обоев {plugin}"));
            }
            other => {
                let why = other
                    .map(|o| o.stderr)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "нет ответа plasmashell".to_string());
                log_line(
                    "warning",
                    &format!("не удалось вернуть плагин обоев {plugin}: {why}"),
                );
            }
        }
    }

    /// Остался ли на столе выбранным УДАЛЁННЫЙ плагин обоев Rexon.
    fn kde_stale_rexon_plugin(&mut self) -> bool {
        let reply = self.kde_plugin_reply();
        wpe::kde_removed_plugin_selected(&reply)
    }

    /// Текущая картинка `org.kde.image` рабочего стола.
    ///
    /// Читается из конфига столов, а не через `kreadconfig6`: тому пришлось
    /// бы называть номер контейнмента, а он у каждой машины свой — запрос по
    /// чужому номеру отдавал пустоту, и остановка возвращала не обой
    /// человека, а умолчание дистрибутива.
    fn kde_current_static_image(&mut self) -> Option<String> {
        let home = self.home();
        let cfg = format!("{home}/.config/plasma-org.kde.plasma.desktop-appletsrc");
        let text = self.sys.read_text(&cfg)?;
        wpe::kde_desktop_static_image(&text)
    }

    /// Чистый обой для возврата после остановки: сохранённый настоящий →
    /// текущий (если это не наш кадр из кеша) → умолчание дистрибутива.
    fn kde_clean_static_image(&mut self) -> String {
        let saved = self.saved_wallpaper.clone();
        let current = self.kde_current_static_image();
        for cand in [saved, current].into_iter().flatten() {
            if cand.is_empty() {
                continue;
            }
            let exists = self.sys.exists(&cand.replace("file://", ""));
            if wpe::clean_image_candidate_ok(&cand, exists) {
                return cand;
            }
        }
        for dir in wpe::DISTRO_WALLPAPER_DIRS {
            if self.sys.is_dir(dir) {
                return format!("{dir}/");
            }
        }
        // Любой установленный пакет обоев Plasma. Порядок отсортированный —
        // иначе «первый попавшийся» менялся бы от машины к машине.
        for name in self.sys.listdir(wpe::PLASMA_WALLPAPER_ROOT) {
            let pkg = format!("{}/{name}", wpe::PLASMA_WALLPAPER_ROOT);
            if self.sys.is_dir(&format!("{pkg}/contents/images")) {
                return format!("{pkg}/");
            }
        }
        String::new()
    }

    // ── иконки рабочего стола и PID-файл ──────────────────────────────────

    /// Показать или скрыть иконки рабочего стола командами движка.
    fn set_desktop_icons(&mut self, visible: bool) {
        if let Some(probe) = wpe::desktop_icons_probe_argv(&self.de, visible) {
            let answered_yes = match self.sys.run(&probe, wpe::DESKTOP_CMD_TIMEOUT_SEC) {
                Some(out) => out.stdout.trim().eq_ignore_ascii_case("true"),
                None => false,
            };
            if !answered_yes {
                return;
            }
        }
        for argv in wpe::desktop_icons_argvs(&self.de, visible) {
            self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC);
        }
    }

    /// MATE: включить или выключить иконки рабочего стола.
    ///
    /// Без проверки владельца стола — в отличие от [`Self::set_desktop_icons`]:
    /// здесь человек нажал кнопку САМ и именно про MATE, и молча ничего не
    /// делать в ответ на прямое нажатие нельзя (правило #13).
    ///
    /// Истина — все команды отработали. ⭐ Починка: прежде итог не смотрелся
    /// вовсе, и кнопка рапортовала «иконки включены» даже там, где схемы
    /// настроек MATE нет.
    pub fn mate_set_desktop_icons(&mut self, visible: bool) -> bool {
        let mut ok = true;
        for argv in wpe::desktop_icons_argvs("mate", visible) {
            let done = matches!(
                self.sys.run(&argv, wpe::DESKTOP_CMD_TIMEOUT_SEC),
                Some(ref out) if out.rc == 0
            );
            if !done {
                log_line("warning", &format!("иконки MATE: команда не отработала: {}", argv.join(" ")));
            }
            ok &= done;
        }
        ok
    }

    // ── размытие обоев ────────────────────────────────────────────────────

    /// Размыть текущие обои-картинку и поставить размытую копию.
    ///
    /// Истина — копия стоит на столе. Настоящие обои запоминаются тем же
    /// путём, что перед живыми обоями, — их вернёт [`Self::unblur_wallpaper`]
    /// или остановка.
    ///
    /// ⭐ Починка: Python узнавал текущие обои только через gsettings, и на
    /// KDE «Размыть» отвечало отказом всегда. Здесь картинка стола KDE
    /// читается из конфига столов (как при возврате после остановки); наш
    /// кадр из кеша за настоящие обои не принимается.
    /// Картинка, которая сейчас стоит обоями стола, — только чтение (набор
    /// виджетов Conky запоминает её рядом с виджетами). Свои обои-картинка
    /// Rexon — по памяти исполнителя; иначе — из настроек окружения: конфиг
    /// столов KDE, ключ COSMIC, gsettings. `None` — узнать не вышло или
    /// стоят не картинка.
    pub fn desktop_static_image(&mut self) -> Option<String> {
        if self.current_format.as_deref() == Some("static") {
            return self.current_path.clone();
        }
        if self.current_format.is_some() {
            return None; // играют живые обои — картинки нет
        }
        let found = if wpe::uses_plasma_config(&self.de) {
            self.kde_current_static_image()
        } else if self.de == "cosmic" {
            let all = format!("{}/{}/all", self.config_home(), wpe::COSMIC_BG_DIR_REL);
            self.sys.read_text(&all).and_then(|t| wpe::cosmic_bg_source_path(&t))
        } else if self.de == "xfce" {
            // xfdesktop держит картинку у каждого монитора и рабочего стола;
            // главный стол — первый монитор, рабочий стол 0.
            let list: Vec<String> = ["xfconf-query", "-c", "xfce4-desktop", "-l"].map(String::from).to_vec();
            let timeout = utils::default_timeout_secs(&list) as f64;
            let prop = wpe::xfce_last_image_prop(&self.sys.run(&list, timeout)?.stdout)?;
            let out = self.sys.run(&wpe::xfconf_get_argv(&prop), timeout)?;
            let raw = out.stdout.trim().to_string();
            (!raw.is_empty()).then_some(raw)
        } else {
            let (schema, key) = wpe::gsettings_schema(&self.de)?;
            let mut get = |schema: &str, key: &str| -> Option<String> {
                let argv = wpe::gsettings_get_argv(schema, key);
                let timeout = utils::default_timeout_secs(&argv) as f64;
                let raw = wpe::gsettings_path_value(&self.sys.run(&argv, timeout)?.stdout);
                (!raw.is_empty()).then_some(raw)
            };
            // GNOME при тёмном оформлении показывает `picture-uri-dark`.
            let dark = wpe::gsettings_dark_key(&self.de).and_then(|(s, k)| {
                let (cs, ck) = crate::themes::COLOR_SCHEME_KEY;
                get(cs, ck).filter(|v| wpe::prefers_dark(v)).and_then(|_| get(s, k))
            });
            dark.or_else(|| get(schema, key))
        };
        found.filter(|p| self.sys.exists(p) && !self.sys.is_dir(p))
    }

    /// Картинка, на которой показать снимок виджета (подложка превью Conky,
    /// `conky_preview`): обои-картинка стола ([`Self::desktop_static_image`]),
    /// у видеообоев — кадр видео (ffmpeg, в кеш), у анимации GIF — сам файл
    /// (декодер берёт первый кадр). `None` — живые обои без картинки
    /// (сцена, веб, Flash) или узнать не вышло: подложка — ровная заливка.
    pub fn desktop_backdrop_image(&mut self) -> Option<String> {
        match self.current_format.clone().as_deref() {
            Some("video") => {
                let path = self.current_path.clone()?;
                let meta = std::fs::metadata(&path).ok();
                let stamp = meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let key = crate::conky_overlay::key(&format!("{path}@{stamp}"));
                let out = format!("{}/rexon/conky-previews/backdrop/{key}.jpg", self.cache_home());
                if !self.sys.exists(&out) {
                    let _ = std::fs::create_dir_all(format!("{}/rexon/conky-previews/backdrop", self.cache_home()));
                    self.sys.run(&wpe::extract_frame_argv(&path, &out, 3.0), 30.0)?;
                }
                self.sys.exists(&out).then_some(out)
            }
            Some("animated") => self.current_path.clone().filter(|p| self.sys.exists(p)),
            _ => self.desktop_static_image(),
        }
    }

    pub fn blur_current_wallpaper(&mut self, radius: i64) -> bool {
        self.save_current_wallpaper();
        if self.saved_wallpaper.is_none() && wpe::uses_plasma_config(&self.de) {
            if let Some(image) = self.kde_current_static_image() {
                let exists = self.sys.exists(&image);
                if wpe::clean_image_candidate_ok(&image, exists) {
                    self.saved_wallpaper = Some(image);
                    self.saved_de = Some(self.de.clone());
                }
            }
        }
        let Some(source) = self.saved_wallpaper.clone() else {
            log_line("warning", "размытие: не удалось узнать текущие обои-картинку");
            return false;
        };
        if !self.sys.exists(&source) || self.sys.is_dir(&source) {
            log_line("warning", &format!("размытие: текущих обоев нет на диске: {source}"));
            return false;
        }
        let cache = self.cache_home();
        let output = wpe::blur_cache_output_path(&cache, &source);
        if let Err(e) = self.sys.blur_image(&source, &output, radius.clamp(1, 50)) {
            log_line("error", &format!("размытие не удалось: {e}"));
            return false;
        }
        let tools = self.plan_tools();
        let hint = self.sys.package_hint("swww");
        let plan = wpe::static_plan(&self.de, &self.static_session(), &output, &tools, &hint);
        let res = self.run_plan(&plan, &output);
        if !res.ok {
            log_line("error", &format!("размытая копия не встала: {}", res.message));
            return false;
        }
        self.blur_path = Some(output);
        true
    }

    /// Снять размытие: вернуть запомненные настоящие обои. Ложь — возвращать
    /// нечего (то же, что у Python).
    pub fn unblur_wallpaper(&mut self) -> bool {
        if self.saved_wallpaper.is_none() {
            return false;
        }
        self.restore_wallpaper();
        self.blur_path = None;
        true
    }

    /// MATE: перезапустить рабочий стол Caja. Истина — стол на месте.
    ///
    /// ⚠️ Починка класса «сами же и убили». `caja --force-desktop` при
    /// работающей Caja передаёт ей просьбу и выходит, а при НЕработающей —
    /// сам становится рабочим столом и живёт дальше. Прежде команда шла
    /// через «запустить и дождаться» со сроком 5 секунд, и по сроку процесс
    /// убивался: человек, у которого стол пропал, нажимал «Перезапустить» и
    /// через пять секунд снова оставался без стола. Теперь процесс поднимается
    /// своим сеансом и не снимается: успех — жив после отсрочки движка ИЛИ
    /// вышел с нулём; отказ — не запустился или вышел с ошибкой.
    pub fn mate_restart_caja_desktop(&mut self) -> bool {
        let argv = wpe::caja_restart_argv();
        let proc = match self.sys.spawn(&argv, true) {
            Ok(proc) => proc,
            Err(e) => {
                log_line("error", &format!("Caja не запустилась: {e}"));
                return false;
            }
        };
        match self.wait_alive(proc) {
            None => true,
            Some(0) => true,
            Some(rc) => {
                log_line("error", &format!("Caja вышла с ошибкой (код {rc})"));
                false
            }
        }
    }

    fn home(&mut self) -> String {
        let home = self.sys.env("HOME");
        if home.is_empty() {
            ".".to_string()
        } else {
            home
        }
    }

    /// Путь PID-файла процесса обоев.
    fn pidfile(&mut self) -> String {
        let dir = self.sys.env("XDG_RUNTIME_DIR");
        let dir = if dir.is_empty() {
            format!("{}/.cache/rexon", self.home())
        } else {
            dir
        };
        format!("{dir}/{}", wpe::PIDFILE_NAME)
    }

    /// Записать PID живого процесса обоев — по нему подбираются сироты
    /// прошлого запуска.
    fn write_pidfile(&mut self) {
        let Some(proc) = self.process else {
            return;
        };
        if self.sys.poll(proc).is_some() {
            return; // процесс уже мёртв — писать нечего
        }
        let Some(pid) = self.sys.pid(proc) else {
            return;
        };
        let path = self.pidfile();
        self.sys.write_text(&path, &pid.to_string());
    }

    /// Удалить PID-файл.
    fn remove_pidfile(&mut self) {
        let path = self.pidfile();
        self.sys.remove_file(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Подставной шов: ничего не делает, всё записывает.
    ///
    /// Живой рабочий стол сверками не трогается вовсе — ни один `cargo test`
    /// не имеет права снять обои у того, кто его запустил.
    #[derive(Default)]
    struct FakeSys {
        /// Что просили запустить и дождаться.
        ran: Vec<Vec<String>>,
        /// Что просили поднять процессом (и своей ли сессией).
        spawned: Vec<(Vec<String>, bool)>,
        /// Ответы на `run` по первому слову команды.
        answers: HashMap<String, RunOut>,
        /// Команды, которых в системе «нет» (`run` отдаёт `None`).
        missing: Vec<String>,
        /// Программы, которые «есть».
        tools: Vec<String>,
        /// Коды возврата поднятых процессов по порядку: `None` — жив.
        spawn_rc: Vec<Option<i64>>,
        /// Снятые процессы.
        killed: Vec<ProcHandle>,
        /// Вести мосту.
        bridge: Vec<(bool, String, bool)>,
        /// Пути, которых «нет» (всё остальное считается существующим).
        missing_paths: Vec<String>,
        /// Пути, которые считаются каталогами.
        dirs: Vec<String>,
        /// Содержимое каталогов.
        listing: HashMap<String, Vec<String>>,
        /// Файлы, которые можно прочитать.
        files: HashMap<String, String>,
        /// Что записали и куда.
        written: Vec<(String, String)>,
        /// Записанное сразу читается обратно (`files`).
        write_through: bool,
        /// Что удалили.
        removed: Vec<String>,
        /// Переменные окружения.
        envs: HashMap<String, String>,
        clock: f64,
        next: ProcHandle,
        /// Что просили размыть: (исходник, копия, сила).
        blurred: Vec<(String, String, i64)>,
        /// Размытие «не удаётся».
        blur_fails: bool,
        /// Заморозки процессов: (дескриптор, заморозить ли).
        signals: Vec<(ProcHandle, bool)>,
        /// Где «лежит» хост обоев (один на видео и веб).
        host: Option<String>,
        /// Кадр из видео «достаётся».
        frame_ok: bool,
        /// Какие кадры просили достать: (исходник, куда).
        frames: Vec<(String, String)>,
        /// Куда «скачивается» ролик YouTube.
        yt_download: Option<String>,
        /// Чужой шелл, который «держит» слой обоев.
        shell_owner: Option<DesktopShell>,
        /// Ответ демона на «сними WE-обои»; `None` — крюка нет.
        we_stop: Option<(String, i64)>,
        /// Когда просили снять WE-обои: (запущено команд, поднято процессов)
        /// на тот миг — по ним видно, что снятие шло ДО своих обоев.
        we_stops: Vec<(usize, usize)>,
    }

    impl FakeSys {
        fn answer(&mut self, bin: &str, rc: i64, stdout: &str) {
            self.answers.insert(
                bin.to_string(),
                RunOut { rc, stdout: stdout.to_string(), stderr: String::new() },
            );
        }
        fn stderr_answer(&mut self, bin: &str, rc: i64, stderr: &str) {
            self.answers.insert(
                bin.to_string(),
                RunOut { rc, stdout: String::new(), stderr: stderr.to_string() },
            );
        }
    }

    impl WpSys for FakeSys {
        fn run(&mut self, argv: &[String], _timeout: f64) -> Option<RunOut> {
            self.ran.push(argv.to_vec());
            let bin = argv.first().cloned().unwrap_or_default();
            if self.missing.contains(&bin) {
                return None;
            }
            Some(self.answers.get(&bin).cloned().unwrap_or(RunOut {
                rc: 0,
                stdout: String::new(),
                stderr: String::new(),
            }))
        }
        fn spawn(&mut self, argv: &[String], new_session: bool) -> Result<ProcHandle, String> {
            self.spawned.push((argv.to_vec(), new_session));
            let bin = argv.first().cloned().unwrap_or_default();
            if self.missing.contains(&bin) {
                return Err("нет такой команды".to_string());
            }
            self.next += 1;
            Ok(self.next)
        }
        fn poll(&mut self, proc: ProcHandle) -> Option<i64> {
            // Порядковый номер процесса = его строка в `spawn_rc`.
            let idx = (proc as usize).checked_sub(1)?;
            self.spawn_rc.get(idx).copied().flatten()
        }
        fn pid(&mut self, proc: ProcHandle) -> Option<i64> {
            Some(1000 + proc)
        }
        fn kill_process(&mut self, proc: ProcHandle) {
            self.killed.push(proc);
        }
        fn has_tool(&mut self, tool: &str) -> bool {
            self.tools.iter().any(|t| t == tool)
        }
        fn package_hint(&mut self, _tool: &str) -> String {
            String::new()
        }
        fn exists(&mut self, path: &str) -> bool {
            !self.missing_paths.iter().any(|p| p == path)
        }
        fn is_dir(&mut self, path: &str) -> bool {
            self.dirs.iter().any(|p| p == path)
        }
        fn listdir(&mut self, path: &str) -> Vec<String> {
            self.listing.get(path).cloned().unwrap_or_default()
        }
        fn read_text(&mut self, path: &str) -> Option<String> {
            self.files.get(path).cloned()
        }
        fn write_text(&mut self, path: &str, content: &str) {
            self.written.push((path.to_string(), content.to_string()));
            if self.write_through {
                self.files.insert(path.to_string(), content.to_string());
            }
        }
        fn remove_file(&mut self, path: &str) {
            self.removed.push(path.to_string());
        }
        fn env(&mut self, name: &str) -> String {
            self.envs.get(name).cloned().unwrap_or_default()
        }
        fn monotonic(&mut self) -> f64 {
            self.clock += 0.01;
            self.clock
        }
        fn sleep(&mut self, seconds: f64) {
            self.clock += seconds;
        }
        fn bridge_notify(&mut self, live: bool, name: &str, announce: bool) {
            self.bridge.push((live, name.to_string(), announce));
        }
        fn blur_image(&mut self, source: &str, output: &str, radius: i64) -> Result<(), String> {
            self.blurred.push((source.to_string(), output.to_string(), radius));
            if self.blur_fails {
                Err("картинка не разобрана".to_string())
            } else {
                Ok(())
            }
        }
        fn pause_process(&mut self, proc: ProcHandle, pause: bool) {
            self.signals.push((proc, pause));
        }
        fn wallpaper_host(&mut self) -> Option<String> {
            self.host.clone()
        }
        fn file_size(&mut self, _path: &str) -> Option<u64> {
            Some(4096)
        }
        fn extract_frame(&mut self, source: &str, output: &str, _timestamp: f64) -> bool {
            self.frames.push((source.to_string(), output.to_string()));
            self.frame_ok
        }
        fn download_youtube(&mut self, _url: &str, _quality: &str) -> Option<String> {
            self.yt_download.clone()
        }
        fn wallpaper_shell_owner(&mut self, _de: &str, _session: &str) -> Option<DesktopShell> {
            self.shell_owner
        }
        fn stop_we_wallpapers(&mut self, _timeout_ms: u64) -> Option<(String, i64)> {
            self.we_stops.push((self.ran.len(), self.spawned.len()));
            self.we_stop.clone()
        }
    }

    fn exec(de: &str, session: &str, sys: FakeSys) -> WallpaperExec<FakeSys> {
        WallpaperExec::with_env(sys, de, session)
    }

    /// Живая WE-обоя и статичная картинка: исполнитель СНАЧАЛА снимает
    /// WE-обою (до первой команды плана), потом ставит картинку — иначе она
    /// легла бы под поверхность WE и человек увидел бы прежние обои.
    #[test]
    fn static_wallpaper_stops_live_we_wallpapers_first() {
        let mut sys = FakeSys::default();
        sys.tools.push("plasma-apply-wallpaperimage".to_string());
        sys.we_stop = Some((crate::we_client::STOP_STOPPED.to_string(), 2));
        let mut e = exec("kde", "wayland", sys);
        let res = e.apply("/home/u/лес.png", "static", "");
        assert!(res.ok, "{}", res.message);
        let sys = e.sys_mut();
        assert_eq!(sys.we_stops, vec![(0, 0)], "WE-обои сняты не первыми: {:?}", sys.we_stops);
        assert!(!sys.ran.is_empty(), "картинка так и не поставлена");
        assert!(!res.message.contains("Wallpaper Engine"), "лишняя приписка: {}", res.message);
    }

    /// Снять WE-обою не вышло: свои обои всё равно ставятся, а человек
    /// узнаёт, почему их не видно. Нет демона / нет крюка — молча.
    #[test]
    fn we_stop_failure_is_told_and_absence_is_silent() {
        for (answer, told) in [
            (Some(crate::we_client::STOP_ERROR), true),
            (Some(crate::we_client::STOP_NOT_RUNNING), false),
            (Some(crate::we_client::STOP_NO_QT), false),
            (None, false),
        ] {
            let mut sys = FakeSys::default();
            sys.tools.push("plasma-apply-wallpaperimage".to_string());
            sys.we_stop = answer.map(|a| (a.to_string(), 0));
            let mut e = exec("kde", "wayland", sys);
            let res = e.apply("/home/u/лес.png", "static", "");
            assert!(res.ok, "{answer:?}: {}", res.message);
            let note = we_yield_note(crate::we_client::STOP_ERROR).unwrap();
            assert_eq!(res.message.contains(&note), told, "{answer:?}: {}", res.message);
        }
    }

    /// Ради отказа чужие обои не снимаются: файла нет, формат снят с
    /// продукта, слой держит чужой шелл.
    #[test]
    fn refusals_do_not_stop_we_wallpapers() {
        let mut sys = FakeSys::default();
        sys.missing_paths.push("/home/u/нет.png".to_string());
        sys.we_stop = Some((crate::we_client::STOP_STOPPED.to_string(), 1));
        let mut e = exec("kde", "wayland", sys);
        assert!(!e.apply("/home/u/нет.png", "static", "").ok);
        assert!(!e.apply("/home/u/сцена", "scene", "").ok);
        assert!(e.sys_mut().we_stops.is_empty(), "сняли ради отказа: {:?}", e.sys_mut().we_stops);

        let mut sys = FakeSys::default();
        sys.shell_owner = Some(DesktopShell::Noctalia);
        sys.we_stop = Some((crate::we_client::STOP_STOPPED.to_string(), 1));
        let mut e = exec("hyprland", "wayland", sys);
        assert!(!e.apply("/home/u/лес.png", "static", "").ok);
        assert!(e.sys_mut().we_stops.is_empty(), "сняли при чужом шелле");
    }

    /// Свои форматы снимают WE-обои все; снятые с продукта — нет.
    #[test]
    fn every_own_format_yields_we_wallpapers() {
        for fmt in ["static", "animated", "video", "youtube", "web", "swf"] {
            assert!(yields_we_wallpapers(fmt), "{fmt}");
        }
        for fmt in ["scene", "application", "", "что-то"] {
            assert!(!yields_we_wallpapers(fmt), "{fmt}");
        }
    }

    /// Первый удавшийся шаг заканчивает план: до второго очередь не доходит.
    #[test]
    fn the_first_step_that_works_ends_the_plan() {
        let mut sys = FakeSys::default();
        sys.tools.push("plasma-apply-wallpaperimage".to_string());
        let plan = wpe::static_plan(
            "kde",
            "wayland",
            "/tmp/нет.png",
            &["plasma-apply-wallpaperimage".to_string()],
            "",
        );
        let mut e = exec("kde", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(res.ok, "первый шаг отдал успех: {}", res.message);
        assert_eq!(e.sys_mut().ran.len(), 1, "запущено больше одного шага");
    }

    /// Неудачный НЕтерминальный шаг уводит к следующему, а не к отказу:
    /// ровно это делает запасные пути плана достижимыми.
    #[test]
    fn a_failed_step_hands_over_to_the_next_one() {
        let mut sys = FakeSys::default();
        // swww отвечает ненулём (демон отвечает на query, картинку не берёт),
        // и очередь обязана дойти до swaybg.
        sys.answer("swww", 1, "");
        sys.tools.push("swww".to_string());
        sys.tools.push("swaybg".to_string());
        sys.spawn_rc = vec![None];
        let tools = vec!["swww".to_string(), "swaybg".to_string()];
        let plan = wpe::static_plan("sway", "wayland", "/tmp/нет.png", &tools, "");
        assert!(plan.len() >= 2, "у плана wlroots нет запасного шага");
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(res.ok, "запасной шаг не отработал: {}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        assert!(
            spawned.iter().any(|(argv, _)| argv[0] == "swaybg"),
            "до запасного шага очередь не дошла: {spawned:?}"
        );
    }

    /// `{stderr}` терминального шага заполняется настоящим выводом команды —
    /// иначе покупатель видел бы отказ без причины.
    #[test]
    fn the_terminal_failure_carries_the_real_stderr() {
        let mut sys = FakeSys::default();
        sys.stderr_answer("gsettings", 1, "схема не найдена");
        let plan = wpe::static_plan("gnome", "wayland", "/tmp/нет.png", &[], "");
        let mut e = exec("gnome", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(!res.ok);
        assert!(!res.message.is_empty(), "отказ без слов");
    }

    /// COSMIC: картинка ложится в родной ключ cosmic-bg (ничего не
    /// запускается), наш прежний swaybg снимается; ключ в `$XDG_CONFIG_HOME`.
    #[test]
    fn cosmic_static_wallpaper_is_written_to_cosmic_bg() {
        let mut sys = FakeSys::default();
        sys.tools = vec!["cosmic-bg".to_string(), "swaybg".to_string()];
        sys.write_through = true;
        sys.envs.insert("HOME".to_string(), "/h".to_string());
        sys.envs.insert("XDG_CONFIG_HOME".to_string(), "/x".to_string());
        let mut e = exec("cosmic", "wayland", sys);
        e.static_holder = Some(77);
        let tools = e.plan_tools();
        let plan = wpe::static_plan("cosmic", "wayland", "/w.png", &tools, "");
        let res = e.run_plan(&plan, "/w.png");
        assert!(res.ok, "{}", res.message);
        let all = "/x/cosmic/com.system76.CosmicBackground/v1/all";
        let written = e.sys_mut().files.get(all).cloned().unwrap_or_default();
        assert_eq!(wpe::cosmic_bg_source_path(&written).as_deref(), Some("/w.png"));
        assert!(e.sys_mut().spawned.is_empty(), "поднят свой демон фона");
        assert_eq!(e.sys_mut().killed, vec![77], "прежний swaybg не снят");
        // Перед живыми обоями запоминается именно этот ключ.
        e.saved_wallpaper = None;
        e.save_current_wallpaper();
        assert_eq!(e.saved_wallpaper.as_deref(), Some("/w.png"));
    }

    /// sway: фон ставит сам sway (`swaymsg output * bg`), свой swaybg не
    /// поднимается, а прежний наш снимается — иначе он висел бы лишним
    /// процессом. Нет сеанса sway (swaymsg не ответил) — запасной swaybg.
    #[test]
    fn sway_static_wallpaper_uses_sway_own_background() {
        let mut sys = FakeSys::default();
        sys.tools = vec!["swaymsg".to_string(), "swaybg".to_string()];
        let mut e = exec("sway", "wayland", sys);
        e.static_holder = Some(77);
        let tools = e.plan_tools();
        let plan = wpe::static_plan("sway", "wayland", "/w.png", &tools, "");
        let res = e.run_plan(&plan, "/w.png");
        assert!(res.ok, "{}", res.message);
        assert!(e.sys_mut().spawned.is_empty(), "поднят свой swaybg");
        assert_eq!(e.sys_mut().killed, vec![77], "прежний swaybg не снят");
        assert!(e.sys_mut().ran.iter().any(|a| a[0] == "swaymsg"), "{:?}", e.sys_mut().ran);

        let mut sys = FakeSys::default();
        sys.tools = vec!["swaymsg".to_string(), "swaybg".to_string()];
        sys.answer("swaymsg", 1, "");
        sys.spawn_rc = vec![None];
        let mut e = exec("sway", "wayland", sys);
        let tools = e.plan_tools();
        let plan = wpe::static_plan("sway", "wayland", "/w.png", &tools, "");
        assert!(e.run_plan(&plan, "/w.png").ok);
        assert_eq!(e.sys_mut().spawned.len(), 1, "запасной swaybg не поднят");
    }

    /// Запись в ключ COSMIC не удалась (не перечиталась) — план идёт дальше,
    /// к внешнему демону, а не рапортует успех.
    #[test]
    fn cosmic_bg_failure_falls_back_to_a_daemon() {
        let mut sys = FakeSys::default();
        sys.tools = vec!["cosmic-bg".to_string(), "swaybg".to_string()];
        sys.spawn_rc = vec![None];
        let mut e = exec("cosmic", "wayland", sys);
        let tools = e.plan_tools();
        let plan = wpe::static_plan("cosmic", "wayland", "/w.png", &tools, "");
        let res = e.run_plan(&plan, "/w.png");
        assert!(res.ok, "{}", res.message);
        assert_eq!(e.sys_mut().spawned.len(), 1, "запасной swaybg не поднят");
    }

    /// Шаг `fail` отвечает отказом, ничего не запуская.
    #[test]
    fn a_fail_step_runs_nothing() {
        let sys = FakeSys::default();
        // GNOME-Wayland: анимацию показать нечем — план из одного отказа.
        let plan = wpe::animated_plan(
            "gnome",
            "wayland",
            "/tmp/нет.gif",
            &[],
            "{monitor}",
            "{resolution}",
            &wpe::Hints::default(),
        );
        let mut e = exec("gnome", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.gif");
        assert!(!res.ok);
        assert!(e.sys_mut().ran.is_empty(), "отказ что-то запустил");
        assert!(e.sys_mut().spawned.is_empty(), "отказ что-то поднял");
    }

    /// Мёртвый запуск НЕ считается успехом: процесс, умерший внутри
    /// отсрочки, уводит план на запасной шаг. Это та самая починка «зелёное
    /// „обои запущены“ на пустом столе».
    #[test]
    fn a_process_that_dies_at_once_is_not_a_success() {
        let mut sys = FakeSys::default();
        sys.tools.push("swaybg".to_string());
        sys.spawn_rc = vec![Some(1)]; // первый же поднятый уже мёртв
        let plan = wpe::static_plan(
            "sway",
            "wayland",
            "/tmp/нет.png",
            &["swaybg".to_string()],
            "",
        );
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(!res.ok, "мёртвый запуск прошёл как успех: {}", res.message);
        assert_eq!(e.sys_mut().spawned.len(), 1);
        assert_eq!(e.sys_mut().killed.len(), 1, "труп не подобран");
    }

    /// Живой запуск — успех, процесс запомнен, а PID подставлен в сообщение.
    #[test]
    fn a_living_process_is_remembered() {
        let mut sys = FakeSys::default();
        sys.tools.push("swaybg".to_string());
        sys.spawn_rc = vec![None]; // жив
        let plan = wpe::static_plan(
            "sway",
            "wayland",
            "/tmp/нет.png",
            &["swaybg".to_string()],
            "",
        );
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(res.ok, "живой запуск не прошёл: {}", res.message);
        assert!(e.static_holder.is_some(), "держатель фона не запомнен");
        assert!(!res.message.contains("{pid}"), "метка PID осталась сырой");
    }

    /// Наблюдения за системой спрашиваются ЛЕНИВО: план без меток не должен
    /// дёргать `wlr-randr`/`xrandr` вовсе.
    #[test]
    fn observations_are_asked_only_when_a_step_needs_them() {
        let mut sys = FakeSys::default();
        sys.tools.push("plasma-apply-wallpaperimage".to_string());
        let plan = wpe::static_plan(
            "kde",
            "wayland",
            "/tmp/нет.png",
            &["plasma-apply-wallpaperimage".to_string()],
            "",
        );
        let mut e = exec("kde", "wayland", sys);
        e.run_plan(&plan, "/tmp/нет.png");
        let asked: Vec<String> = e
            .sys_mut()
            .ran
            .iter()
            .filter_map(|c| c.first().cloned())
            .collect();
        assert!(!asked.iter().any(|b| b == "wlr-randr" || b == "xrandr"));
    }

    /// А когда метка есть — наблюдение спрашивается ОДИН раз на прогон.
    #[test]
    fn an_observation_is_asked_once_per_plan() {
        let mut sys = FakeSys::default();
        sys.answer("xrandr", 0, "   1280x720     60.00*+\n");
        sys.tools.push("xwinwrap".to_string());
        sys.tools.push("mpv".to_string());
        sys.spawn_rc = vec![None];
        let tools = vec!["xwinwrap".to_string(), "mpv".to_string()];
        let plan = wpe::animated_plan(
            "generic",
            "x11",
            "/tmp/нет.gif",
            &tools,
            "{monitor}",
            "{resolution}",
            &wpe::Hints::default(),
        );
        let mut e = exec("generic", "x11", sys);
        e.run_plan(&plan, "/tmp/нет.gif");
        let xrandr = e
            .sys_mut()
            .ran
            .iter()
            .filter(|c| c.first().map(|b| b == "xrandr").unwrap_or(false))
            .count();
        assert_eq!(xrandr, 1, "разрешение спрошено {xrandr} раз вместо одного");
        let spawned = e.sys_mut().spawned.clone();
        assert!(
            spawned
                .iter()
                .any(|(argv, _)| argv.iter().any(|a| a.contains("1280x720"))),
            "метка разрешения не подставлена: {spawned:?}"
        );
    }

    /// Xfce: путь пишется в КАЖДОЕ свойство `/last-image`, а не в первое.
    #[test]
    fn xfce_writes_every_monitor_property() {
        let mut sys = FakeSys::default();
        sys.answer(
            "xfconf-query",
            0,
            "/backdrop/screen0/monitorHDMI-1/workspace0/last-image\n\
             /backdrop/screen0/monitorDP-2/workspace0/last-image\n\
             /backdrop/screen0/monitorDP-2/workspace0/image-style\n",
        );
        let plan = wpe::static_plan("xfce", "x11", "/tmp/нет.png", &[], "");
        let mut e = exec("xfce", "x11", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(res.ok, "{}", res.message);
        let writes = e
            .sys_mut()
            .ran
            .iter()
            .filter(|c| c.iter().any(|a| a.contains("last-image")) && c.contains(&"-s".to_string()))
            .count();
        assert_eq!(writes, 2, "записано {writes} свойств вместо двух");
    }

    /// Xfce без единого свойства фона — отказ ВТОРЫМ сообщением плана, а не
    /// первым: «нет мониторов» и «xfconf-query не ответил» — разные беды.
    #[test]
    fn xfce_without_properties_says_the_other_reason() {
        let mut sys = FakeSys::default();
        sys.answer("xfconf-query", 0, "/backdrop/screen0/monitorDP-2/workspace0/image-style\n");
        let plan = wpe::static_plan("xfce", "x11", "/tmp/нет.png", &[], "");
        let alt = plan[0].alt_message.clone();
        let err = plan[0].err_message.clone();
        let mut e = exec("xfce", "x11", sys);
        let res = e.run_plan(&plan, "/tmp/нет.png");
        assert!(!res.ok);
        assert_eq!(res.message, alt);
        assert_ne!(res.message, err, "две разные беды объяснены одной строкой");
    }

    /// План, который не дал исхода ни одним шагом, всё равно ОТВЕЧАЕТ.
    #[test]
    fn an_exhausted_plan_still_answers() {
        let sys = FakeSys::default();
        let mut e = exec("generic", "x11", sys);
        let res = e.run_plan(&[], "/tmp/нет.png");
        assert!(!res.ok);
        assert!(!res.message.is_empty(), "пустой план ответил молчанием");
    }

    /// Успех статичных обоев мосту не врёт: статика живыми обоями не
    /// является, и признак `video_wallpaper_active` обязан лечь ложью.
    #[test]
    fn static_wallpaper_is_not_announced_as_live() {
        let mut sys = FakeSys::default();
        sys.tools.push("plasma-apply-wallpaperimage".to_string());
        let plan = wpe::static_plan(
            "kde",
            "wayland",
            "/tmp/нет.png",
            &["plasma-apply-wallpaperimage".to_string()],
            "",
        );
        let mut e = exec("kde", "wayland", sys);
        e.run_plan(&plan, "/tmp/нет.png");
        let said = e.sys_mut().bridge.clone();
        assert_eq!(said.len(), 1, "мосту сказано {} раз", said.len());
        assert!(!said[0].0, "статика объявлена живыми обоями");
    }

    /// Демон swww поднимается только когда шаг об этом просит, и повторного
    /// подъёма при живом демоне нет.
    #[test]
    fn the_swww_daemon_is_not_raised_twice() {
        let mut sys = FakeSys::default();
        sys.tools.push("swww".to_string());
        sys.answer("swww", 0, ""); // и query, и img отвечают нулём
        let plan = wpe::animated_plan(
            "sway",
            "wayland",
            "/tmp/нет.gif",
            &["swww".to_string()],
            "{monitor}",
            "{resolution}",
            &wpe::Hints::default(),
        );
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/нет.gif");
        assert!(res.ok, "{}", res.message);
        assert!(e.sys_mut().spawned.is_empty(), "демон поднят при живом демоне");
        let queries = e
            .sys_mut()
            .ran
            .iter()
            .filter(|c| c.get(1).map(|a| a == "query").unwrap_or(false))
            .count();
        assert_eq!(queries, 1);
    }

    // ── остановка и возврат стола ─────────────────────────────────────────

    /// Плоские команды прогона одной строкой — так удобнее искать порядок.
    fn lines(e: &mut WallpaperExec<FakeSys>) -> Vec<String> {
        e.sys_mut().ran.iter().map(|c| c.join(" ")).collect()
    }

    /// Остановка на пустом месте не молчит: покупатель нажал кнопку и обязан
    /// получить ответ, даже когда снимать было нечего.
    #[test]
    fn stopping_nothing_still_answers() {
        let sys = FakeSys::default();
        let mut e = exec("generic", "x11", sys);
        let res = e.stop(true);
        assert!(res.ok);
        assert_eq!(res.message, wpe::stopped_message(None));
        let said = e.sys_mut().bridge.clone();
        assert_eq!(said.len(), 1, "мосту не сказано об остановке");
        assert!(!said[0].0, "после остановки мосту обещаны живые обои");
    }

    /// Остановка называет формат, который ИГРАЛ, а не тот, что записал
    /// возврат фона. Это та самая починка «после видео сказано „static“».
    #[test]
    fn stopping_names_the_format_that_played() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "");
        let mut e = exec("gnome", "wayland", sys);
        e.current_format = Some("video".to_string());
        e.saved_wallpaper = Some("/tmp/старые.png".to_string());
        e.saved_de = Some("gnome".to_string());
        let res = e.stop(true);
        assert!(res.message.contains(&wpe::format_label("video")), "{}", res.message);
        assert!(
            !res.message.contains(&wpe::format_label("static")),
            "возврат фона переписал формат: {}",
            res.message
        );
    }

    /// Возврат ставит именно сохранённые обои — тем же планом, что и обычное
    /// применение.
    #[test]
    fn stopping_puts_the_saved_wallpaper_back() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "");
        let mut e = exec("gnome", "wayland", sys);
        e.saved_wallpaper = Some("/tmp/старые.png".to_string());
        e.saved_de = Some("gnome".to_string());
        e.stop(true);
        assert!(
            lines(&mut e)
                .iter()
                .any(|l| l.contains("picture-uri file:///tmp/старые.png")),
            "сохранённые обои не вернулись: {:?}",
            lines(&mut e)
        );
    }

    /// Обоев, которых на диске нет, возврат не ставит: команда бы прошла, а
    /// стол остался бы пустым — и мы бы этого не заметили.
    #[test]
    fn a_saved_wallpaper_that_vanished_is_not_restored() {
        let mut sys = FakeSys::default();
        sys.missing_paths.push("/tmp/удалённые.png".to_string());
        let mut e = exec("gnome", "wayland", sys);
        e.saved_wallpaper = Some("/tmp/удалённые.png".to_string());
        e.saved_de = Some("gnome".to_string());
        e.stop(true);
        assert!(
            !lines(&mut e).iter().any(|l| l.contains("удалённые.png")),
            "поставлены обои, которых нет"
        );
    }

    /// Обои, стоявшие до нас, читаются ОДИН раз: второе чтение записало бы
    /// нашу же подмену как «то, что было до нас».
    #[test]
    fn the_previous_wallpaper_is_read_only_once() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "'file:///tmp/старые.png'\n");
        let mut e = exec("gnome", "wayland", sys);
        e.save_current_wallpaper();
        e.save_current_wallpaper();
        let gets = lines(&mut e).iter().filter(|l| l.contains("gsettings get")).count();
        assert_eq!(gets, 1, "прежние обои прочитаны {gets} раз");
        assert_eq!(e.saved_wallpaper.as_deref(), Some("/tmp/старые.png"));
    }

    /// KDE: чужой плагин стола возвращается ПОСЛЕ картинки. Обратный порядок
    /// затирал возврат — план картинки на KDE безусловно ставит org.kde.image.
    #[test]
    fn the_foreign_kde_plugin_comes_back_after_the_image() {
        let sys = FakeSys::default();
        let mut e = exec("kde", "wayland", sys);
        e.saved_wallpaper = Some("/tmp/старые.png".to_string());
        e.saved_de = Some("kde".to_string());
        e.saved_kde_plugin = Some("org.waywallen.kde".to_string());
        e.stop(true);
        let out = lines(&mut e);
        let reset = out.iter().position(|l| l.contains("wallpaperPlugin = 'org.kde.image'"));
        let back = out.iter().position(|l| l.contains("org.waywallen.kde"));
        assert!(reset.is_some(), "штатный плагин не возвращался: {out:?}");
        assert!(back.is_some(), "чужой плагин не возвращён: {out:?}");
        assert!(reset < back, "чужой плагин возвращён ДО картинки — затрётся");
    }

    /// Остановка снимает ТОЛЬКО свой процесс — по дескриптору, никогда по
    /// имени.
    #[test]
    fn stopping_kills_only_our_own_process() {
        let mut sys = FakeSys::default();
        sys.spawn_rc = vec![None];
        let mut e = exec("generic", "x11", sys);
        let proc = e.sys_mut().spawn(&["mpv".to_string()], true).unwrap();
        e.process = Some(proc);
        e.stop(true);
        assert_eq!(e.sys_mut().killed, vec![proc]);
        assert!(e.process.is_none());
    }

    /// MATE: иконки не трогаются, если столом владеет не Caja. Проверка есть
    /// только у гашения — возвращать иконки надо в любом случае.
    #[test]
    fn mate_icons_are_left_alone_when_caja_does_not_own_the_desktop() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "false\n");
        let mut e = exec("mate", "x11", sys);
        e.set_desktop_icons(false);
        let hidden = lines(&mut e).iter().filter(|l| l.contains("set org.mate")).count();
        assert_eq!(hidden, 0, "иконки погашены под чужим столом");
        e.set_desktop_icons(true);
        let shown = lines(&mut e)
            .iter()
            .filter(|l| l.contains("set org.mate.background show-desktop-icons true"))
            .count();
        assert_eq!(shown, 1, "иконки не вернулись");
    }

    /// PID-файл пишется живым процессом и снимается остановкой: по нему
    /// подбираются сироты прошлого запуска.
    #[test]
    fn the_pid_file_follows_the_process() {
        let mut sys = FakeSys::default();
        sys.envs.insert("XDG_RUNTIME_DIR".to_string(), "/run/user/1000".to_string());
        sys.spawn_rc = vec![None];
        let mut e = exec("generic", "x11", sys);
        let proc = e.sys_mut().spawn(&["mpv".to_string()], true).unwrap();
        e.process = Some(proc);
        e.write_pidfile();
        let written = e.sys_mut().written.clone();
        assert_eq!(written.len(), 1);
        assert!(written[0].0.ends_with(wpe::PIDFILE_NAME), "{}", written[0].0);
        assert!(!written[0].1.is_empty(), "PID-файл пуст");
        e.stop(true);
        assert_eq!(e.sys_mut().removed.len(), 1, "PID-файл остался после остановки");
    }

    /// Отсутствующий файл — отказ с ИМЕНЕМ файла, а не общее «не вышло».
    #[test]
    fn applying_a_missing_file_names_it() {
        let mut sys = FakeSys::default();
        sys.missing_paths.push("/tmp/нет-такого.png".to_string());
        let mut e = exec("gnome", "wayland", sys);
        let res = e.apply("/tmp/нет-такого.png", "static", "");
        assert!(!res.ok);
        assert!(res.message.contains("нет-такого.png"), "{}", res.message);
        assert!(e.sys_mut().ran.is_empty(), "отказ что-то запустил");
    }

    /// Все живые форматы носит исполнитель — ни один не отвечает границей
    /// «пока показывает классическое окно». Последним переехал Flash.
    #[test]
    fn every_live_format_is_carried() {
        for fmt in wpe::LIVE_FORMATS {
            assert!(carries_format(fmt), "«{fmt}» отослан бы в классическое окно");
            let mut e = exec("kde", "wayland", FakeSys::default());
            let res = e.apply("/tmp/что-то", fmt, "");
            assert!(!res.message.contains(&live_formats_not_carried()), "{fmt}: {}", res.message);
        }
    }

    /// Правило «что носит исполнитель» и сама граница `apply` — одно и то
    /// же: окно спрашивает `carries_format`, и ответ обязан совпадать с тем,
    /// что сделает исполнитель, иначе кнопка обещала бы одно, а делала другое.
    #[test]
    fn the_carried_formats_rule_matches_apply() {
        assert!(carries_format("static"));
        assert!(carries_format("web"));
        assert!(carries_format("video"));
        assert!(carries_format("youtube"));
        assert!(carries_format("swf"), "Flash снова граница");
        // Flash без хоста — честный отказ «показывать нечем», не граница.
        let mut e = exec("gnome", "wayland", FakeSys::default());
        let res = e.apply("/tmp/ролик.swf", "swf", "");
        assert_eq!(res.message, wpe::video_host_missing_message());
        // Остальное носится: либо применяется, либо отказ окончательный —
        // и ни в одном ответе нет «пока показывает прежний движок».
        for fmt in ["static", "animated", "scene", "application", "", "что-то-новое"] {
            assert!(carries_format(fmt), "формат «{fmt}» отослан бы в классическое окно");
            let mut e = exec("gnome", "wayland", FakeSys::default());
            let res = e.apply("/tmp/что-то", fmt, "");
            assert!(!res.message.contains(&live_formats_not_carried()), "{fmt}: {}", res.message);
        }
    }

    /// Сцены, обои-приложения и незнакомый формат — те же слова, что у
    /// Python, и ⭐ починка: отказ НЕ снимает работающие живые обои (Python
    /// сначала гасил их, а потом отказывал).
    #[test]
    fn retired_formats_refuse_without_touching_live_wallpaper() {
        for (fmt, want) in [
            ("scene", wpe::scene_unsupported()),
            ("application", wpe::application_unsupported()),
            ("parallax", wpe::unknown_format_message("parallax")),
        ] {
            let (mut e, handle) = exec_with_live_process();
            let res = e.apply("/tmp/пакет/data/обои", fmt, "");
            assert!(!res.ok, "{fmt}");
            assert_eq!(res.message, want, "{fmt}");
            assert!(!e.sys_mut().killed.contains(&handle), "{fmt}: отказ снял живые обои");
            assert_eq!(e.current_format(), Some("web"), "{fmt}: отказ переписал формат");
        }
    }

    /// GIF на sway со swww: план движка исполняется, формат запоминается
    /// как «анимация» — как у Python `_apply_animated`. `swww img` —
    /// разовая команда демону swww, своего процесса за ней нет, поэтому и
    /// PID-файла нет (он пишется только за живым процессом).
    #[test]
    fn animated_gif_runs_the_engine_plan() {
        let mut sys = FakeSys::default();
        sys.tools.push("swww".to_string());
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply("/tmp/котик.gif", "animated", "");
        assert!(res.ok, "{}", res.message);
        assert_eq!(e.current_format(), Some("animated"));
        let ran = e.sys_mut().ran.clone();
        assert!(
            ran.iter().any(|a| a.len() > 2 && a[0] == "swww" && a[1] == "img" && a[2] == "/tmp/котик.gif"),
            "swww img не запущен: {ran:?}"
        );
        assert!(
            !e.sys_mut().written.iter().any(|(p, _)| p.ends_with(wpe::PIDFILE_NAME)),
            "PID-файл записан без процесса"
        );
        assert!(!format_wants_load_monitor(e.current_format()), "GIF под надзором — у Python нет");
    }

    /// ⭐ Починка: на GNOME-Wayland план GIF — один отказ, и он говорится
    /// ДО дела: живые обои не сняты, подложка не ставилась, ничего не
    /// запущено.
    /// Английские ответы исполнителя — без русских букв (журнал не в счёт:
    /// его читаем мы, он не переводится).
    #[test]
    fn english_exec_answers_have_no_russian() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let cyrillic = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
        let mut e = exec("kde", "wayland", FakeSys::default());
        let no_ext = e.run_shell_extension(None, "/x/a.mp4", "video");
        assert!(!no_ext.ok);
        assert_eq!(no_ext.message, "This desktop environment does not need the shell extension");
        let mut sys = LiveSys::new();
        let empty = sys.spawn(&[], false).err().expect("пустая команда — отказ");
        for text in [live_formats_not_carried(), no_ext.message, empty] {
            assert!(!cyrillic(&text), "русское в английском интерфейсе: {text}");
        }
    }

    #[test]
    fn animated_refusal_on_gnome_touches_nothing() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let (mut e, handle) = exec_with_live_process();
        e.de = "gnome".to_string();
        let ran_before = e.sys_mut().ran.len();
        let spawned_before = e.sys_mut().spawned.len();
        let res = e.apply("/tmp/котик.gif", "animated", "");
        assert!(!res.ok);
        assert!(res.message.contains("there's nothing to show animated wallpapers with"), "{}", res.message);
        assert!(!e.sys_mut().killed.contains(&handle), "отказ снял живые обои");
        assert_eq!(e.sys_mut().ran.len(), ran_before, "отказ что-то запустил");
        assert_eq!(e.sys_mut().spawned.len(), spawned_before, "отказ что-то поднял");
    }

    /// Применение статики запоминает то, что стояло до него, — иначе
    /// возвращать после остановки будет нечего.
    #[test]
    fn applying_remembers_what_stood_before() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "'file:///tmp/старые.png'\n");
        let mut e = exec("gnome", "wayland", sys);
        let res = e.apply("/tmp/новые.png", "static", "Закат");
        assert!(res.ok, "{}", res.message);
        assert_eq!(e.saved_wallpaper.as_deref(), Some("/tmp/старые.png"));
        assert_eq!(e.current_path.as_deref(), Some("/tmp/новые.png"));
    }

    /// Слой обоев держит чужой шелл: ни один вход применения не кладёт
    /// обои под его поверхность и не рапортует успех. Отказ — до всякого
    /// дела: прежние живые обои не сняты, стол не запомнен, ничего не
    /// запущено, а текст называет шелл и кнопку.
    #[test]
    fn a_shell_owning_the_layer_refuses_every_apply_before_any_work() {
        for fmt in ["static", "video", "animated", "web", "swf"] {
            let mut sys = FakeSys::default();
            sys.shell_owner = Some(DesktopShell::Noctalia);
            sys.host = Some("/usr/bin/rexon-wallpaper-host".to_string());
            sys.tools.push("swww".to_string());
            let mut e = exec("niri", "wayland", sys);
            e.process = Some(77);
            let res = e.apply("/tmp/обои", fmt, "");
            assert!(!res.ok, "{fmt}: обои легли под чужой шелл с зелёным рапортом");
            assert!(res.message.contains("Noctalia"), "{fmt}: {}", res.message);
            assert!(res.message.contains("Give Wallpaper to Rexon"), "{fmt}: {}", res.message);
            assert!(e.sys_mut().spawned.is_empty(), "{fmt}: отказ что-то поднял");
            assert!(e.sys_mut().ran.is_empty(), "{fmt}: отказ что-то запустил");
            assert!(e.sys_mut().killed.is_empty(), "{fmt}: отказ снял прежние обои");
            assert!(e.saved_wallpaper.is_none(), "{fmt}: отказ запомнил стол");
        }
        let mut sys = FakeSys::default();
        sys.shell_owner = Some(DesktopShell::DankMaterialShell);
        sys.tools.push("yt-dlp".to_string());
        let mut e = exec("hyprland", "wayland", sys);
        let res = e.apply_youtube("https://www.youtube.com/watch?v=abc", "1080p", "stream", true);
        assert!(!res.ok);
        assert!(res.message.contains("DankMaterialShell"), "{}", res.message);
        assert!(e.sys_mut().spawned.is_empty() && e.sys_mut().ran.is_empty());
    }

    /// Без чужого шелла вход применения работает как прежде (укус сверки
    /// выше: тот же стол, тот же файл — и успех).
    #[test]
    fn without_a_shell_the_same_apply_goes_through() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "'file:///tmp/старые.png'\n");
        let mut e = exec("gnome", "wayland", sys);
        assert!(e.apply("/tmp/новые.png", "static", "").ok);
    }

    /// Д12: LXQt. `pcmanfm-qt --set-wallpaper` отвечает нулём всегда;
    /// «✅» — только если путь обоев действительно лёг в его настройки.
    #[test]
    fn lxqt_reports_success_only_when_pcmanfm_saved_the_wallpaper() {
        let conf = "/home/u/.config/pcmanfm-qt/lxqt/settings.conf";
        let run = |saved: Option<&str>| {
            let mut sys = FakeSys::default();
            sys.tools.push("pcmanfm-qt".to_string());
            // Запасные пути этого стола «не работают»: иначе подставной шов
            // отрапортовал бы успех за них, и проверялся бы не pcmanfm-qt.
            for other in ["gsettings", "feh", "xwallpaper", "nitrogen", "hsetroot", "xsetroot", "swaybg", "swww"] {
                sys.missing.push(other.to_string());
            }
            sys.envs.insert("HOME".to_string(), "/home/u".to_string());
            if let Some(text) = saved {
                sys.files.insert(conf.to_string(), text.to_string());
            }
            let mut e = exec("lxqt", "x11", sys);
            e.apply("/tmp/новые.png", "static", "")
        };
        let ok = run(Some("[Desktop]\nWallpaperMode=fit\nWallpaper=/tmp/новые.png\n"));
        assert!(ok.ok, "{}", ok.message);
        // Записан чужой путь, ключа нет, файла нет — успеха нет.
        for saved in [
            Some("[Desktop]\nWallpaper=/usr/share/lxqt/wallpapers/старые.png\n"),
            Some("[Desktop]\nWallpaperMode=fit\n"),
            Some("[Other]\nWallpaper=/tmp/новые.png\n"),
            None,
        ] {
            let res = run(saved);
            assert!(!res.ok, "ложный успех при {saved:?}: {}", res.message);
        }
    }

    /// Снимок состояния не выдаёт мёртвый процесс за живые обои.
    #[test]
    fn the_status_does_not_call_a_dead_process_alive() {
        let mut sys = FakeSys::default();
        sys.spawn_rc = vec![Some(0)];
        let mut e = exec("generic", "x11", sys);
        let proc = e.sys_mut().spawn(&["mpv".to_string()], true).unwrap();
        e.process = Some(proc);
        e.current_format = Some("video".to_string());
        let st = e.status();
        assert!(!st.running, "мёртвый процесс объявлен живыми обоями");
        assert!(st.pid.is_none(), "у мёртвого процесса назван PID");
        assert_eq!(st.de, "generic");
    }

    /// Возврат стола после остановки берёт чистую картинку, а не наш же
    /// размытый кадр из кеша: иначе «Остановить» оставляло бы человека с
    /// размытием вместо обоев.
    #[test]
    fn the_clean_image_is_never_our_blur_frame() {
        let mut sys = FakeSys::default();
        sys.dirs.push("/usr/share/wallpapers/Next".to_string());
        let mut e = exec("kde", "wayland", sys);
        e.saved_wallpaper = Some("/home/w/.cache/rexon/blur/кадр.jpg".to_string());
        let clean = e.kde_clean_static_image();
        assert!(!clean.contains(".cache/rexon"), "возвращён наш же кадр: {clean}");
        assert_eq!(clean, "/usr/share/wallpapers/Next/");
    }

    /// Нет ни сохранённого обоя, ни каталогов дистрибутива — берётся любой
    /// установленный пакет обоев Plasma, но только НАСТОЯЩИЙ (с картинками).
    #[test]
    fn the_last_resort_package_must_actually_have_images() {
        let mut sys = FakeSys::default();
        sys.listing.insert(
            wpe::PLASMA_WALLPAPER_ROOT.to_string(),
            vec!["Пустой".to_string(), "Сгорание".to_string()],
        );
        sys.dirs
            .push("/usr/share/wallpapers/Сгорание/contents/images".to_string());
        let mut e = exec("kde", "wayland", sys);
        assert_eq!(e.kde_clean_static_image(), "/usr/share/wallpapers/Сгорание/");
    }


    // ── фоновые звуки ─────────────────────────────────────────────────────

    fn with_mpv() -> FakeSys {
        let mut sys = FakeSys::default();
        sys.tools.push("mpv".to_string());
        sys
    }

    /// Зацикленный звук, умерший внутри отсрочки, — сбой: сам он не выходит.
    #[test]
    fn a_looped_sound_that_dies_is_a_failure() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![Some(0)];
        let mut e = exec("kde", "wayland", sys);
        assert!(!e.play_ambient("дождь", "/tmp/дождь.ogg", "Дождь", 50, true));
        assert!(e.active_ambient().is_empty());
        assert_eq!(e.sys_mut().killed.len(), 1, "труп mpv не подобран");
    }

    /// ⭐ А РАЗОВЫЙ звук, честно доигравший внутри отсрочки, — успех.
    /// Мерить его правилом обоев («жив через полсекунды») значило бы
    /// отказывать на исправном коротком звуке.
    #[test]
    fn a_one_shot_sound_that_finished_is_a_success() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![Some(0)];
        let mut e = exec("kde", "wayland", sys);
        assert!(e.play_ambient("щелчок", "/tmp/щелчок.wav", "Щелчок", 50, false));
    }

    /// Но разовый звук, упавший с ненулевым кодом (битый файл), успехом не
    /// считается.
    #[test]
    fn a_one_shot_sound_that_broke_is_still_a_failure() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![Some(2)];
        let mut e = exec("kde", "wayland", sys);
        assert!(!e.play_ambient("битый", "/tmp/битый.wav", "", 50, false));
    }

    /// Нет mpv — отказ, а не тихий вид, что звук пошёл.
    #[test]
    fn without_mpv_the_sound_is_refused() {
        let sys = FakeSys::default(); // mpv в списке нет
        let mut e = exec("kde", "wayland", sys);
        assert!(!e.play_ambient("дождь", "/tmp/дождь.ogg", "Дождь", 50, true));
        assert!(e.sys_mut().spawned.is_empty(), "mpv запускался без mpv");
    }

    /// Повторный запуск того же звука снимает прежний экземпляр: иначе два
    /// mpv играли бы одно и то же поверх друг друга.
    #[test]
    fn replaying_a_sound_stops_the_previous_copy() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![None, None];
        let mut e = exec("kde", "wayland", sys);
        assert!(e.play_ambient("дождь", "/tmp/дождь.ogg", "Дождь", 50, true));
        assert!(e.play_ambient("дождь", "/tmp/дождь.ogg", "Дождь", 50, true));
        assert_eq!(e.sys_mut().killed.len(), 1, "прежний экземпляр остался играть");
        assert_eq!(e.active_ambient().len(), 1);
    }

    /// ⭐ Смена громкости перезапускает mpv в ТОМ ЖЕ режиме: раньше разовый
    /// звук после правки громкости зацикливался навсегда.
    #[test]
    fn changing_the_volume_keeps_the_playback_mode() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![None, None];
        let mut e = exec("kde", "wayland", sys);
        assert!(e.play_ambient("щелчок", "/tmp/щелчок.wav", "Щелчок", 30, false));
        assert!(e.set_ambient_volume("щелчок", 80));
        let spawned = e.sys_mut().spawned.clone();
        assert_eq!(spawned.len(), 2);
        assert!(
            !spawned[1].0.iter().any(|a| a == "--loop=inf"),
            "разовый звук зациклился при смене громкости: {:?}",
            spawned[1].0
        );
        assert!(
            spawned[1].0.iter().any(|a| a == "--volume=80"),
            "громкость не сменилась: {:?}",
            spawned[1].0
        );
    }

    /// Остановка обоев снимает и фоновые звуки: иначе они играли бы на
    /// голом столе.
    #[test]
    fn stopping_the_wallpaper_also_stops_the_sounds() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![None, None];
        let mut e = exec("generic", "x11", sys);
        assert!(e.play_ambient("дождь", "/tmp/дождь.ogg", "Дождь", 50, true));
        assert!(e.play_ambient("ветер", "/tmp/ветер.ogg", "Ветер", 40, true));
        e.stop(true);
        assert!(e.active_ambient().is_empty(), "звуки пережили остановку обоев");
        assert_eq!(e.sys_mut().killed.len(), 2);
    }

    /// Доигравший звук уходит из списка сам, а имя без подписи берётся из
    /// имени файла — выдумывать за расширение нечего.
    #[test]
    fn a_finished_sound_leaves_the_list_by_itself() {
        let mut sys = with_mpv();
        sys.spawn_rc = vec![None];
        let mut e = exec("kde", "wayland", sys);
        assert!(e.play_ambient("дождь", "/tmp/звуки/дождь.ogg", "", 50, true));
        let active = e.active_ambient();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "дождь.ogg");
        assert_eq!(active[0].volume, 50);
        // …процесс кончился.
        e.sys_mut().spawn_rc = vec![Some(0)];
        assert!(e.active_ambient().is_empty(), "доигравший звук остался в списке");
        assert!(!e.ambient_playing("дождь"));
    }

    /// MATE-кнопки делают ровно то, что написано на них, — и делают всегда:
    /// человек нажал сам, и проверка «а Caja ли владеет столом» тут была бы
    /// молчаливым отказом на прямое нажатие.
    #[test]
    fn the_mate_buttons_always_do_what_they_say() {
        let sys = FakeSys::default();
        let mut e = exec("mate", "x11", sys);
        assert!(e.mate_set_desktop_icons(false), "исправные команды — не отказ");
        assert!(
            lines(&mut e)
                .iter()
                .any(|l| l.contains("org.mate.background show-desktop-icons false")),
            "иконки не погасли: {:?}",
            lines(&mut e)
        );
        assert!(e.mate_restart_caja_desktop(), "живая Caja — не отказ");
        let spawned = e.sys_mut().spawned.clone();
        assert!(
            spawned
                .iter()
                .any(|(argv, own_session)| argv.join(" ").contains("caja --force-desktop")
                    && *own_session),
            "стол Caja не поднимался своим сеансом: {spawned:?}"
        );
        // Стол не «запускается и дожидается»: такой запуск убивал Caja по сроку.
        assert!(
            !lines(&mut e).iter().any(|l| l.contains("caja")),
            "Caja запущена с ожиданием — по сроку её снимут"
        );
        assert!(e.sys_mut().killed.is_empty(), "поднятый стол Caja снят");
        // Проверки владельца стола тут быть НЕ должно.
        assert!(
            !lines(&mut e).iter().any(|l| l.contains("gsettings get")),
            "прямое нажатие спросило разрешения"
        );
    }

    // ── видео из файла ───────────────────────────────────────────────────

    const HOST: &str = "/opt/rexon/rexon-wallpaper-host";

    /// Ответ `gdbus` на `GetExtensionInfo` для работающего расширения — вид
    /// словаря как у gnome-shell (`serializeExtension`: числа упакованы `d`).
    const GDBUS_EXT_ACTIVE: &str = "({'uuid': <'rexon-wallpaper@rexon.app'>, \
        'name': <'Rexon Wallpaper Engine'>, 'version': <1.0>, 'type': <2.0>, \
        'state': <1.0>, 'error': <''>, 'hasPrefs': <false>},)\n";

    /// Видео своим окном: хост поднимается своим сеансом, обои живые,
    /// надзор за нагрузкой им нужен.
    #[test]
    fn video_file_raises_our_host() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply("/home/u/волны.mp4", "video", "");
        assert!(res.ok, "{}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        assert!(
            spawned
                .iter()
                .any(|(argv, own)| argv[..4] == [HOST, "--mode", "video", "/home/u/волны.mp4"] && *own),
            "хост видео не поднят своим сеансом: {spawned:?}"
        );
        let st = e.status();
        assert!(st.running);
        assert_eq!(st.format.as_deref(), Some("video"));
        assert!(format_wants_load_monitor(e.current_format()));
        assert!(e.monitor_target().is_some(), "надзору не за кем следить");
    }

    /// MATE на X11, Caja без ключа сквозного фона: видео применилось, и
    /// человеку сказано, что значки под обоями; сами значки не выключены.
    /// «Остановить» без записанного состояния стола ничего не откатывает.
    #[test]
    fn video_on_mate_x11_says_icons_are_hidden_and_keeps_them_on() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        // Любой gsettings отвечает «true»: значки включены, а в списке
        // ключей Caja нет `transparent-background`.
        sys.answer("gsettings", 0, "true\n");
        let mut e = exec("mate", "x11", sys);
        let res = e.apply("/home/u/волны.mp4", "video", "");
        assert!(res.ok, "{}", res.message);
        let hidden = crate::wallpaper_backend::desktop_icons_hidden_text(crate::wallpaper_backend::DesktopIconsOwner::Caja);
        assert!(res.message.ends_with(&hidden), "{}", res.message);
        let ran = lines(&mut e);
        assert!(!ran.iter().any(|l| l.contains("show-desktop-icons false")), "значки выключены: {ran:?}");

        // Картинка окна не создаёт — о значках ни слова.
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "true\n");
        let mut e = exec("mate", "x11", sys);
        let res = e.apply("/home/u/лес.png", "static", "");
        assert!(!res.message.contains(&hidden), "{}", res.message);

        // «Остановить» с записанным прежним состоянием — ключ Caja назад.
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.answer("gsettings", 0, "true\n");
        sys.envs.insert("HOME".into(), "/home/u".into());
        let state = crate::wallpaper_backend::desktop_icons_state_path("", "/home/u");
        let saved = crate::wallpaper_backend::DesktopIconsSaved {
            caja_recorded: true,
            caja_transparent_was_off: true,
            ..Default::default()
        };
        sys.files.insert(state.clone(), saved.to_text());
        let mut e = exec("mate", "x11", sys);
        e.stop(true);
        let ran = lines(&mut e);
        assert!(
            ran.iter().any(|l| l.contains("org.mate.caja.desktop transparent-background false")),
            "{ran:?}"
        );
        assert!(e.sys_mut().removed.contains(&state));
    }

    /// Нет хоста — честный отказ до запуска; не видеофайл — отказ с причиной.
    #[test]
    fn video_file_refuses_honestly() {
        let mut e = exec("sway", "wayland", FakeSys::default());
        let res = e.apply("/home/u/волны.mp4", "video", "");
        assert!(!res.ok);
        assert_eq!(res.message, wpe::video_host_missing_message());
        assert!(e.sys_mut().spawned.is_empty());

        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply("/home/u/заметки.txt", "video", "");
        assert!(!res.ok);
        assert!(res.message.contains(".txt"), "{}", res.message);
        assert!(e.sys_mut().spawned.is_empty());
    }

    /// GNOME Wayland: видео уходит расширению оболочки — своего процесса
    /// нет, строка запуска хоста записана; не установлено — отказ.
    #[test]
    fn video_on_gnome_goes_to_the_shell_extension() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.answer("gdbus", 0, GDBUS_EXT_ACTIVE);
        let mut e = exec("gnome", "wayland", sys);
        let res = e.apply("/home/u/волны.mp4", "video", "");
        assert!(res.ok, "{}", res.message);
        let ran = lines(&mut e);
        assert!(ran.iter().any(|l| l.contains("source /home/u/волны.mp4")), "{ran:?}");
        assert!(ran.iter().any(|l| l.contains(wpe::SHELL_EXT_DAEMON_KEY) && l.contains(HOST)));
        assert!(e.sys_mut().spawned.is_empty(), "на GNOME поднят свой процесс");
        assert_eq!(e.current_format(), Some("video"));

        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.answer("gsettings", 1, "");
        let mut e = exec("gnome", "wayland", sys);
        let res = e.apply("/home/u/волны.mp4", "video", "");
        assert!(!res.ok);
        assert!(res.message.contains("shell extension"), "{}", res.message);
    }

    /// Схема расширения лежит только в его каталоге (в общем каталоге схем
    /// её нет) — ключи пишутся с `--schemadir` этого каталога, своя копия в
    /// домашнем каталоге перекрывает пакетную; каталога нет — голый
    /// `gsettings`.
    #[test]
    fn shell_extension_keys_are_written_through_its_own_schema_dir() {
        let user = format!("/home/u/.local/share/gnome-shell/extensions/{}/schemas", wpe::SHELL_EXT_ID);
        let pkg = format!("/usr/share/gnome-shell/extensions/{}/schemas", wpe::SHELL_EXT_ID);
        let local = format!("/usr/local/share/gnome-shell/extensions/{}/schemas", wpe::SHELL_EXT_ID);
        let written = |missing: &[&String]| {
            let mut sys = FakeSys::default();
            sys.host = Some(HOST.to_string());
            sys.answer("gdbus", 0, GDBUS_EXT_ACTIVE);
            sys.envs.insert("HOME".to_string(), "/home/u".to_string());
            sys.missing_paths = missing.iter().map(|d| format!("{d}/gschemas.compiled")).collect();
            let mut e = exec("gnome", "wayland", sys);
            let res = e.apply("/home/u/волны.mp4", "video", "");
            assert!(res.ok, "{}", res.message);
            e.stop(false);
            e.sys_mut()
                .ran
                .iter()
                .filter(|a| a.first().map(String::as_str) == Some("gsettings"))
                .filter(|a| a.iter().any(|w| w.contains("rexon-wallpaper")))
                .cloned()
                .collect::<Vec<_>>()
        };
        let all = written(&[]);
        assert_eq!(all.len(), 4, "{all:?}");
        assert!(all.iter().all(|a| a[1] == "--schemadir" && a[2] == user), "{all:?}");
        let pkg_only = written(&[&user, &local]);
        assert!(pkg_only.iter().all(|a| a[1] == "--schemadir" && a[2] == pkg), "{pkg_only:?}");
        let none = written(&[&user, &local, &pkg]);
        assert!(none.iter().all(|a| a[1] == "set"), "{none:?}");
    }

    /// «Готово» на GNOME говорится, только когда сама оболочка называет
    /// расширение работающим (`GetExtensionInfo`, state 1). Прежде смотрели
    /// `gnome-extensions list --enabled`, а без ответа считали «включено» —
    /// и рапортовали успех при пустом столе.
    #[test]
    fn gnome_reports_ready_only_when_the_shell_runs_the_extension() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let ok_message = wpe::shell_ext_plan("gnome", "/home/u/волны.mp4", HOST, true)
            .expect("план GNOME")
            .ok_message;
        let apply = |answer: Option<(i64, &str)>| {
            let mut sys = FakeSys::default();
            sys.host = Some(HOST.to_string());
            match answer {
                Some((rc, out)) => sys.answer("gdbus", rc, out),
                None => sys.missing.push("gdbus".to_string()),
            }
            let mut e = exec("gnome", "wayland", sys);
            let res = e.apply("/home/u/волны.mp4", "video", "");
            let asked = lines(&mut e)
                .iter()
                .any(|l| l.contains("org.gnome.Shell.Extensions.GetExtensionInfo"));
            assert!(asked, "оболочку о расширении не спросили");
            res
        };
        let res = apply(Some((0, GDBUS_EXT_ACTIVE)));
        assert!(res.ok && res.message == ok_message, "{}", res.message);

        // Ошибка в расширении и «не та версия» — обоев не будет: отказ.
        for state in ["3.0", "4.0"] {
            let out = GDBUS_EXT_ACTIVE.replace("'state': <1.0>", &format!("'state': <{state}>"));
            let res = apply(Some((0, &out)));
            assert!(!res.ok, "state {state}: {}", res.message);
            assert_ne!(res.message, ok_message);
        }
        // Выключено, не загружено (пустой словарь), оболочка не ответила или
        // gdbus нет — обои переданы, но «готово» не говорится.
        let disabled = GDBUS_EXT_ACTIVE.replace("'state': <1.0>", "'state': <2.0>");
        for answer in [
            Some((0, disabled.as_str())),
            Some((0, "(@a{sv} {},)\n")),
            Some((1, "")),
            None,
        ] {
            let res = apply(answer);
            assert!(res.ok, "{answer:?}: {}", res.message);
            assert_ne!(res.message, ok_message, "{answer:?}");
        }
    }

    /// Flash своим окном: хост `--mode swf` своим сеансом, внешнего `ruffle`
    /// нет, обои под надзором за нагрузкой — тем же путём, что видео.
    #[test]
    fn swf_file_raises_our_host_under_the_load_monitor() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("kde", "wayland", sys);
        let res = e.apply("/home/u/ролик.swf", "swf", "");
        assert!(res.ok, "{}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        assert!(
            spawned
                .iter()
                .any(|(argv, own)| argv[..] == [HOST, "--mode", "swf", "/home/u/ролик.swf"] && *own),
            "хост Flash не поднят своим сеансом: {spawned:?}"
        );
        assert!(
            !spawned.iter().any(|(argv, _)| argv.iter().any(|a| a.contains("ruffle"))),
            "запущен внешний ruffle: {spawned:?}"
        );
        assert_eq!(e.current_format(), Some("swf"));
        assert!(format_wants_load_monitor(e.current_format()));
        assert!(e.monitor_target().is_some(), "надзору не за кем следить");
    }

    /// Flash: нет хоста или нет файла — отказ ДО снятия прежних обоев.
    #[test]
    fn swf_refuses_before_touching_live_wallpaper() {
        let mut e = exec("sway", "wayland", FakeSys::default());
        let res = e.apply("/home/u/ролик.swf", "swf", "");
        assert_eq!(res.message, wpe::video_host_missing_message());
        assert!(e.sys_mut().spawned.is_empty());
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.missing_paths.push("/home/u/нет.swf".to_string());
        sys.dirs.push("/home/u/каталог.swf".to_string());
        let mut e = exec("sway", "wayland", sys);
        for p in ["/home/u/нет.swf", "/home/u/каталог.swf"] {
            let res = e.apply(p, "swf", "");
            assert!(!res.ok);
            assert_eq!(res.message, wpe::swf_missing_message(p));
        }
        assert!(e.sys_mut().spawned.is_empty());
        assert!(e.sys_mut().ran.is_empty(), "отказ что-то запустил");
    }

    /// Flash на GNOME — расширение оболочки получает строку хоста `--mode swf`.
    #[test]
    fn swf_on_gnome_goes_to_the_shell_extension() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.answer("gdbus", 0, GDBUS_EXT_ACTIVE);
        let mut e = exec("gnome", "wayland", sys);
        let res = e.apply("/home/u/ролик.swf", "swf", "");
        assert!(res.ok, "{}", res.message);
        let ran = lines(&mut e);
        assert!(ran.iter().any(|l| l.contains("engine swf")), "{ran:?}");
        assert!(
            ran.iter().any(|l| l.contains(wpe::SHELL_EXT_DAEMON_KEY) && l.contains("\"swf\"")),
            "{ran:?}"
        );
        assert!(e.sys_mut().spawned.is_empty(), "на GNOME поднят свой процесс");
        assert_eq!(e.current_format(), Some("swf"));
    }

    /// Размытая подложка: кадр из видео → размытие в кеш → статичные обои.
    #[test]
    fn video_gets_a_blurred_backdrop() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.frame_ok = true;
        sys.envs.insert("XDG_CACHE_HOME".to_string(), "/home/u/.cache".to_string());
        let mut e = exec("generic", "x11", sys);
        let res = e.apply("/home/u/волны.mp4", "video", "");
        assert!(res.ok, "{}", res.message);
        assert_eq!(
            e.sys_mut().frames[0],
            ("/home/u/волны.mp4".to_string(), "/home/u/.cache/rexon/blur/frame.png".to_string())
        );
        assert_eq!(e.sys_mut().blurred[0].1, "/home/u/.cache/rexon/blur/frame_blur.jpg");
        assert!(e.status().blur_active, "подложка не встала");
        assert_eq!(e.current_format(), Some("video"), "подложка перебила формат видео");
    }

    /// Звук видео выбирает вызывающий: `false` — хост поднят немым,
    /// `true` и умолчание `apply` — со звуком. Один путь на все вызовы.
    #[test]
    fn video_audio_choice_reaches_our_host() {
        let host_argv = |audio: Option<bool>| {
            let mut sys = FakeSys::default();
            sys.host = Some(HOST.to_string());
            let mut e = exec("sway", "wayland", sys);
            let res = match audio {
                Some(a) => e.apply_with_audio("/home/u/волны.mp4", "video", "Волны", a),
                None => e.apply("/home/u/волны.mp4", "video", "Волны"),
            };
            assert!(res.ok, "{}", res.message);
            let spawned = e.sys_mut().spawned.clone();
            spawned
                .into_iter()
                .map(|(argv, _)| argv)
                .find(|argv| argv[0] == HOST)
                .expect("хост видео не поднят")
        };
        assert!(host_argv(Some(false)).iter().any(|a| a == "--no-audio"));
        assert!(!host_argv(Some(true)).iter().any(|a| a == "--no-audio"));
        assert_eq!(VIDEO_AUDIO_DEFAULT, true);
        assert!(!host_argv(None).iter().any(|a| a == "--no-audio"), "умолчание — со звуком");
    }

    /// GNOME Wayland: немое видео — строка запуска хоста у расширения
    /// оболочки несёт `--no-audio`, со звуком — не несёт.
    #[test]
    fn video_audio_choice_reaches_the_shell_extension() {
        let daemon_line = |audio: bool| {
            let mut sys = FakeSys::default();
            sys.host = Some(HOST.to_string());
            sys.answer("gdbus", 0, GDBUS_EXT_ACTIVE);
            let mut e = exec("gnome", "wayland", sys);
            let res = e.apply_with_audio("/home/u/волны.mp4", "video", "", audio);
            assert!(res.ok, "{}", res.message);
            lines(&mut e)
                .into_iter()
                .find(|l| l.contains(wpe::SHELL_EXT_DAEMON_KEY))
                .expect("строка запуска хоста не записана")
        };
        assert!(daemon_line(false).contains("--no-audio"));
        assert!(!daemon_line(true).contains("--no-audio"));
    }

    /// Формат YouTube через общий вход: звук доходит до плеера потока.
    #[test]
    fn youtube_through_apply_keeps_the_audio_choice() {
        let mut sys = FakeSys::default();
        sys.tools = vec!["yt-dlp".to_string(), "mpvpaper".to_string()];
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply_with_audio("https://youtube.com/watch?v=abc", "youtube", "", false);
        assert!(res.ok, "{}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        let argv = &spawned.last().expect("ничего не поднято").0;
        assert!(argv.iter().any(|a| a.contains("no-audio")), "{argv:?}");
    }

    // ── слот исполнителя процесса ────────────────────────────────────────

    /// Слот процесса один: каждый вызов отдаёт тот же, и сам он исполнителя
    /// не заводит (живую машину тест не трогает).
    #[test]
    fn live_slot_is_one_per_process() {
        let a = live_slot();
        assert!(std::sync::Arc::ptr_eq(&a, &live_slot()));
    }

    /// Два держателя слота видят одного исполнителя: поставленное через
    /// первый видно и снимается через второй; занятый замок — `None`, а не
    /// ожидание.
    #[test]
    fn slot_holders_share_one_executor() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let slot: ExecSlot<FakeSys> =
            std::sync::Arc::new(Mutex::new(Some(exec("sway", "wayland", sys))));
        let other = slot.clone();

        let res = with_slot(&slot, |e| e.apply_with_audio("/home/u/волны.mp4", "video", "", false));
        assert!(res.ok, "{}", res.message);
        assert_eq!(try_with_slot(&other, |e| e.status().running), Some(true));

        {
            let _held = slot.lock().unwrap();
            assert!(try_with_slot(&other, |e| e.status().running).is_none(), "занятый замок ждали");
        }

        let stopped = try_with_slot(&other, |e| e.stop(true)).expect("замок свободен");
        assert!(stopped.ok, "{}", stopped.message);
        assert_eq!(with_slot(&slot, |e| e.status().running), false);
        assert!(!with_slot(&slot, |e| e.sys_mut().killed.is_empty()), "хост не снят");
    }

    /// Дверь применения: итог работы возвращается, статичным обоям надзор
    /// не поднимается (ядро отказывает само), слот после двери свободен.
    #[test]
    fn run_in_slot_returns_the_work_and_frees_the_slot() {
        let slot: ExecSlot<FakeSys> =
            std::sync::Arc::new(Mutex::new(Some(exec("gnome", "wayland", FakeSys::default()))));
        let res = run_in_slot(&slot, |e| e.apply("/home/u/картинка.png", "static", ""));
        assert!(res.ok, "{}", res.message);
        assert!(!format_wants_load_monitor(Some("static")));
        assert!(slot.try_lock().is_ok(), "дверь не отпустила замок");
    }

    // ── YouTube ──────────────────────────────────────────────────────────

    /// Поток под Wayland: mpvpaper с форматом по качеству и ссылкой, звук и
    /// громкость ключами; обои живые, надзор им нужен.
    #[test]
    fn youtube_stream_raises_mpvpaper() {
        let mut sys = FakeSys::default();
        sys.tools = vec!["yt-dlp".to_string(), "mpvpaper".to_string()];
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply_youtube("https://youtube.com/watch?v=abc", "720p", "stream", false);
        assert!(res.ok, "{}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        let argv = &spawned.last().expect("ничего не поднято").0;
        assert_eq!(argv[0], "mpvpaper");
        assert!(argv.iter().any(|a| a.contains("ytdl-format=") && a.contains("no-audio")), "{argv:?}");
        assert_eq!(argv.last().unwrap(), "https://youtube.com/watch?v=abc");
        assert_eq!(e.current_format(), Some("youtube"));
        assert!(format_wants_load_monitor(e.current_format()));
    }

    /// GNOME Wayland: поток неисполним — ролик скачивается и ставится как
    /// видео, а человеку сказано, почему скачивание.
    #[test]
    fn youtube_on_gnome_downloads_and_says_why() {
        let mut sys = FakeSys::default();
        sys.tools = vec!["yt-dlp".to_string()];
        sys.host = Some(HOST.to_string());
        sys.yt_download = Some("/home/u/.cache/rexon/youtube/abc.mp4".to_string());
        sys.answer("gdbus", 0, GDBUS_EXT_ACTIVE);
        let mut e = exec("gnome", "wayland", sys);
        let res = e.apply_youtube("https://youtube.com/watch?v=abc", "1080p", "stream", true);
        assert!(res.ok, "{}", res.message);
        let note = wpe::youtube_mode_downgraded_note("gnome", "wayland", "stream");
        assert!(!note.is_empty() && res.message.contains(&note), "{}", res.message);
        assert_eq!(e.current_format(), Some("video"));
    }

    /// Отказы: нет yt-dlp, чужая ссылка, скачивание не удалось.
    #[test]
    fn youtube_refuses_honestly() {
        let mut e = exec("sway", "wayland", FakeSys::default());
        let res = e.apply_youtube("https://youtube.com/watch?v=abc", "1080p", "stream", true);
        assert!(!res.ok);
        assert!(res.message.contains("yt-dlp"), "{}", res.message);

        let mut sys = FakeSys::default();
        sys.tools = vec!["yt-dlp".to_string()];
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply_youtube("ftp://example.com/x", "1080p", "stream", true);
        assert!(!res.ok);
        assert_eq!(res.message, wpe::youtube_unsupported_message("ftp://example.com/x"));

        let mut sys = FakeSys::default();
        sys.tools = vec!["yt-dlp".to_string()];
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply_youtube("https://youtube.com/watch?v=abc", "1080p", "download", true);
        assert!(!res.ok);
        assert_eq!(res.message, wpe::youtube_download_failed_message("sway", "wayland"));
        assert!(e.sys_mut().spawned.is_empty());
    }

    // ── надзор за нагрузкой ──────────────────────────────────────────────

    /// Исполнитель с живым процессом обоев (веб-хост на sway) — у надзора
    /// есть за кем следить.
    fn exec_with_live_process() -> (WallpaperExec<FakeSys>, ProcHandle) {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        assert!(e.apply("https://example.com", "web", "").ok);
        let (handle, pid) = e.monitor_target().expect("есть за кем следить");
        assert!(pid > 0);
        (e, handle)
    }

    const CALM: LoadObservation =
        LoadObservation { battery_low: false, fullscreen: false, cpu_high: false };

    /// Разряженная батарея замораживает обои, зарядка — размораживает;
    /// повторный замер с той же причиной второй раз не сигналит.
    #[test]
    fn monitor_pauses_on_low_battery_and_resumes() {
        let (mut e, h) = exec_with_live_process();
        let mut strikes = 0;
        let low = LoadObservation { battery_low: true, ..CALM };
        assert_eq!(e.monitor_apply(h, &mut strikes, &low), MonitorVerdict::Continue);
        assert_eq!(e.monitor_apply(h, &mut strikes, &low), MonitorVerdict::Continue);
        assert_eq!(e.sys_mut().signals, vec![(h, true)]);
        assert_eq!(e.monitor_apply(h, &mut strikes, &CALM), MonitorVerdict::Continue);
        assert_eq!(e.sys_mut().signals, vec![(h, true), (h, false)]);
    }

    /// Остановка сперва размораживает замороженный процесс — иначе SIGTERM
    /// до него не дойдёт.
    #[test]
    fn stopping_a_paused_wallpaper_resumes_it_first() {
        let (mut e, h) = exec_with_live_process();
        let mut strikes = 0;
        let full = LoadObservation { fullscreen: true, ..CALM };
        e.monitor_apply(h, &mut strikes, &full);
        e.stop(true);
        assert_eq!(e.sys_mut().signals, vec![(h, true), (h, false)]);
        assert_eq!(e.sys_mut().killed, vec![h]);
    }

    /// Процессор выше порога — остановка на N-м замере подряд, не раньше;
    /// спокойный замер обнуляет счёт.
    #[test]
    fn monitor_stops_a_cpu_leak_after_the_strikes() {
        let (mut e, h) = exec_with_live_process();
        let mut strikes = 0;
        let hot = LoadObservation { cpu_high: true, ..CALM };
        for _ in 0..wpe::CPU_HIGH_STRIKES - 1 {
            assert_eq!(e.monitor_apply(h, &mut strikes, &hot), MonitorVerdict::Continue);
        }
        e.monitor_apply(h, &mut strikes, &CALM);
        assert_eq!(strikes, 0, "спокойный замер не обнулил счёт");
        for _ in 0..wpe::CPU_HIGH_STRIKES - 1 {
            e.monitor_apply(h, &mut strikes, &hot);
        }
        assert_eq!(e.monitor_apply(h, &mut strikes, &hot), MonitorVerdict::Done);
        assert!(!e.status().running, "протёкший процесс не остановлен");
        assert_eq!(e.sys_mut().killed, vec![h]);
    }

    /// Упавший процесс: стол возвращается, мост узнаёт об остановке, надзор
    /// кончается.
    #[test]
    fn monitor_restores_the_desktop_when_the_host_crashes() {
        let (mut e, h) = exec_with_live_process();
        e.sys_mut().spawn_rc = vec![Some(1)];
        let mut strikes = 0;
        assert_eq!(e.monitor_apply(h, &mut strikes, &CALM), MonitorVerdict::Done);
        let st = e.status();
        assert!(!st.running);
        assert_eq!(st.format, None);
        assert!(
            matches!(e.sys_mut().bridge.last(), Some((false, _, _))),
            "мост не узнал о падении: {:?}",
            e.sys_mut().bridge
        );
    }

    /// Обои сменились — прежний надзор кончается сам.
    #[test]
    fn monitor_of_replaced_wallpaper_ends() {
        let (mut e, old) = exec_with_live_process();
        assert!(e.apply("https://example.org", "web", "").ok);
        let mut strikes = 0;
        assert_eq!(e.monitor_apply(old, &mut strikes, &CALM), MonitorVerdict::Done);
    }

    /// Батарея: первая `BAT*`, заряд ниже порога и не заряжается.
    #[test]
    fn battery_observation_follows_the_power_supply() {
        let cfg = MonitorConfig { pause_on_fullscreen: false, battery_pause_pct: 20 };
        let setup = |status: &str| {
            let mut sys = FakeSys::default();
            sys.dirs.push(wpe::POWER_SUPPLY_DIR.to_string());
            sys.listing.insert(
                wpe::POWER_SUPPLY_DIR.to_string(),
                vec!["AC".to_string(), "BAT0".to_string()],
            );
            sys.files.insert(format!("{}/BAT0/capacity", wpe::POWER_SUPPLY_DIR), "15\n".into());
            sys.files.insert(format!("{}/BAT0/status", wpe::POWER_SUPPLY_DIR), status.into());
            sys
        };
        let mut sys = setup("Discharging\n");
        assert!(observe_load(&mut sys, "sway", "wayland", 42, &cfg).battery_low);
        let mut sys = setup("Charging\n");
        assert!(!observe_load(&mut sys, "sway", "wayland", 42, &cfg).battery_low);
        let mut sys = FakeSys::default();
        assert!(!observe_load(&mut sys, "sway", "wayland", 42, &cfg).battery_low, "нет батареи");
    }

    /// Полноэкранное окно на X11: окно во весь экран — да; настройка
    /// человека выключена — не спрашиваем вовсе; нет xdotool — нет.
    #[test]
    fn fullscreen_observation_obeys_the_setting() {
        let on = MonitorConfig { pause_on_fullscreen: true, battery_pause_pct: 20 };
        let off = MonitorConfig { pause_on_fullscreen: false, battery_pause_pct: 20 };
        let setup = || {
            let mut sys = FakeSys::default();
            sys.tools.push("xdotool".to_string());
            sys.answer("xdotool", 0, "WINDOW=1\nWIDTH=1920\nHEIGHT=1080\n");
            sys
        };
        let mut sys = setup();
        assert!(observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen);
        let mut sys = setup();
        assert!(!observe_load(&mut sys, "xfce", "x11", 42, &off).fullscreen);
        assert!(!sys.ran.iter().any(|a| a[0] == "xdotool"), "выключенная пауза спросила окно");
        let mut sys = FakeSys::default();
        assert!(!observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "без xdotool");
    }

    /// Окно во весь экран на МЕНЬШЕМ из двух мониторов — тоже полноэкранное.
    ///
    /// Мерка «первое разрешение» видела 1920x1080 и окно на соседнем
    /// 1600x900 не замечала: обои жгли процессор всю игру.
    #[test]
    fn fullscreen_is_measured_by_the_smaller_monitor() {
        let on = MonitorConfig { pause_on_fullscreen: true, battery_pause_pct: 20 };
        let mut sys = FakeSys::default();
        sys.tools.push("xdotool".to_string());
        sys.answer("xdotool", 0, "WINDOW=1\nWIDTH=1600\nHEIGHT=900\n");
        sys.answer("xrandr", 0, XRANDR_TWO);
        assert!(observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen);
    }

    /// Надзор за нагрузкой не принимает окно рабочего стола (и наши
    /// собственные обои) за полноэкранное окно поверх обоев.
    ///
    /// ⭐ Замер 16.09.2026 на стенде X11 с оконным менеджером: активным
    /// окном `xdotool getactivewindow` называл САМИ обои — окно во весь
    /// экран с `_NET_WM_WINDOW_TYPE_DESKTOP`, и обои вставали на паузу
    /// сами на себя.
    #[test]
    fn the_desktop_window_is_not_a_fullscreen_app() {
        let on = MonitorConfig { pause_on_fullscreen: true, battery_pause_pct: 20 };
        let setup = |props: &str, tools: &[&str]| {
            let mut sys = FakeSys::default();
            for t in tools {
                sys.tools.push((*t).to_string());
            }
            sys.answer("xdotool", 0, "WINDOW=99\nWIDTH=1920\nHEIGHT=1080\n");
            sys.answer("xprop", 0, props);
            sys
        };
        let both = ["xdotool", "xprop"];

        let mut sys = setup("_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_DESKTOP\n", &both);
        assert!(!observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "рабочий стол");

        let mut sys = setup("_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_DOCK\n", &both);
        assert!(!observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "панель");

        // Тип не проставился, но окно принадлежит процессу обоев (PID=42).
        let mut sys = setup("_NET_WM_PID(CARDINAL) = 42\n", &both);
        assert!(!observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "свои обои");

        // Чужая игра во весь экран обои по-прежнему ставит на паузу.
        let mut sys = setup(
            "_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_NORMAL\n\
             _NET_WM_PID(CARDINAL) = 7\n",
            &both,
        );
        assert!(observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "игра");

        // Защита от дурака: xprop в системе нет — поведение ПРЕЖНЕЕ, и
        // запускать его никто не пробует.
        let mut sys = setup("", &["xdotool"]);
        assert!(observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "без xprop");
        assert!(!sys.ran.iter().any(|a| a[0] == "xprop"), "xprop звали, хотя его нет");

        // xprop есть, но промолчал — тоже прежнее поведение.
        let mut sys = setup("", &both);
        assert!(observe_load(&mut sys, "xfce", "x11", 42, &on).fullscreen, "xprop молчит");
    }

    /// Процессор: выше порога — да, ниже или без ответа — нет.
    #[test]
    fn cpu_observation_reads_ps() {
        let cfg = MonitorConfig { pause_on_fullscreen: false, battery_pause_pct: 20 };
        let mut sys = FakeSys::default();
        sys.answer("ps", 0, " 95.5\n");
        assert!(observe_load(&mut sys, "sway", "wayland", 42, &cfg).cpu_high);
        let mut sys = FakeSys::default();
        sys.answer("ps", 0, " 3.0\n");
        assert!(!observe_load(&mut sys, "sway", "wayland", 42, &cfg).cpu_high);
        let mut sys = FakeSys::default();
        sys.missing.push("ps".to_string());
        assert!(!observe_load(&mut sys, "sway", "wayland", 42, &cfg).cpu_high);
    }

    /// ⭐ Пауза на полноэкранном окне слушает ту же настройку, что хост.
    #[test]
    fn monitor_config_follows_the_hide_when_covered_setting() {
        let default = monitor_config_from(&serde_json::Map::new());
        assert!(default.pause_on_fullscreen, "умолчание продукта — пауза");
        assert_eq!(default.battery_pause_pct, wpe::BATTERY_PAUSE_PCT);
        let off = crate::wallpaper_audio::AudioSettings {
            actions: crate::wallpaper_audio::actions_for_hide_when_covered(false),
            ..Default::default()
        }
        .to_map();
        assert!(!monitor_config_from(&off).pause_on_fullscreen, "галочка снята, а пауза осталась");
    }

    /// Веб-обои: хост поднимается СВОИМ сеансом со строкой запуска движка и
    /// считается живыми обоями — «Остановить» его снимет.
    #[test]
    fn web_wallpaper_raises_our_host() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply("https://example.com/обои", "web", "");
        assert!(res.ok, "{}", res.message);
        let spawned = e.sys_mut().spawned.clone();
        assert_eq!(
            spawned,
            vec![(
                vec![
                    HOST.to_string(),
                    "--mode".to_string(),
                    "web".to_string(),
                    "https://example.com/обои".to_string()
                ],
                true
            )]
        );
        let st = e.status();
        assert!(st.running, "веб-обои не считаются живыми");
        assert_eq!(st.format.as_deref(), Some("web"));
        e.stop(true);
        assert!(!e.sys_mut().killed.is_empty(), "остановка не сняла веб-хост");
    }

    /// Нет хоста — отказ до запуска; нет HTML-файла — отказ с его именем.
    #[test]
    fn web_wallpaper_refuses_honestly() {
        let mut e = exec("sway", "wayland", FakeSys::default());
        let res = e.apply("https://example.com", "web", "");
        assert!(!res.ok);
        assert_eq!(res.message, wpe::web_host_missing());
        assert!(e.sys_mut().spawned.is_empty(), "без хоста что-то поднялось");

        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        sys.missing_paths.push("/home/u/нет.html".to_string());
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply("/home/u/нет.html", "web", "");
        assert!(!res.ok);
        assert!(res.message.contains("нет.html"), "{}", res.message);
        assert!(e.sys_mut().spawned.is_empty());
        // Тот же отсутствующий файл адресом `file://` — тоже отказ.
        let res = e.apply("file:///home/u/нет.html", "web", "");
        assert!(!res.ok, "адрес отсутствующего файла прошёл");
    }

    /// ⭐ Локальный HTML-файл адресом `file://` (так его отдаёт окно)
    /// применяется, а не отвечает «не найден».
    #[test]
    fn local_web_page_as_file_url_is_applied() {
        let mut sys = FakeSys::default();
        sys.host = Some(HOST.to_string());
        let mut e = exec("sway", "wayland", sys);
        let res = e.apply("file:///home/u/страница.html", "web", "");
        assert!(res.ok, "{}", res.message);
        assert_eq!(e.sys_mut().spawned.len(), 1);
    }

    /// Размытие на GNOME: копия берётся с настоящих обоев, пишется в кеш и
    /// ставится на стол; «Убрать размытие» возвращает настоящие обои.
    #[test]
    fn blurring_uses_the_real_wallpaper_and_comes_back() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "'file:///usr/share/backgrounds/закат.png'\n");
        sys.envs.insert("XDG_CACHE_HOME".to_string(), "/home/u/.cache".to_string());
        let mut e = exec("gnome", "wayland", sys);
        assert!(e.blur_current_wallpaper(20));
        let blurred = e.sys_mut().blurred.clone();
        assert_eq!(
            blurred,
            vec![(
                "/usr/share/backgrounds/закат.png".to_string(),
                "/home/u/.cache/rexon/blur/закат_blur.jpg".to_string(),
                20
            )]
        );
        assert!(
            lines(&mut e).iter().any(|l| l.contains("gsettings set")
                && l.contains("/home/u/.cache/rexon/blur/закат_blur.jpg")),
            "размытая копия не поставлена: {:?}",
            lines(&mut e)
        );
        assert!(e.status().blur_active);
        assert!(e.unblur_wallpaper());
        assert!(
            lines(&mut e).iter().any(|l| l.contains("gsettings set")
                && l.contains("/usr/share/backgrounds/закат.png")),
            "настоящие обои не вернулись"
        );
        assert!(!e.status().blur_active);
        assert!(!e.unblur_wallpaper(), "второй раз возвращать уже нечего");
    }

    /// Размытие не рапортует успех, когда размывать нечего или не вышло.
    #[test]
    fn blurring_reports_failures() {
        // Обоев-картинки нет (окружение без gsettings и не KDE).
        let mut e = exec("sway", "wayland", FakeSys::default());
        assert!(!e.blur_current_wallpaper(20));
        assert!(e.sys_mut().blurred.is_empty());
        // Картинка есть, но не разобралась.
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 0, "'/tmp/обои.png'\n");
        sys.blur_fails = true;
        let mut e = exec("gnome", "wayland", sys);
        assert!(!e.blur_current_wallpaper(20));
        assert!(!e.status().blur_active);
    }

    /// ⭐ Кнопки MATE не рапортуют успех, которого не было.
    #[test]
    fn the_mate_buttons_report_failures() {
        let mut sys = FakeSys::default();
        sys.answer("gsettings", 1, "");
        sys.missing.push("caja".to_string());
        let mut e = exec("mate", "x11", sys);
        assert!(!e.mate_set_desktop_icons(true), "упавшая команда названа успехом");
        assert!(!e.mate_restart_caja_desktop(), "незапустившаяся Caja названа успехом");

        // Caja, вышедшая с ошибкой, — отказ; вышедшая с нулём — успех.
        let mut sys = FakeSys::default();
        sys.spawn_rc = vec![Some(1)];
        let mut e = exec("mate", "x11", sys);
        assert!(!e.mate_restart_caja_desktop());
        let mut sys = FakeSys::default();
        sys.spawn_rc = vec![Some(0)];
        let mut e = exec("mate", "x11", sys);
        assert!(e.mate_restart_caja_desktop(), "просьба работающей Caja — успех");
    }

    // ── живой прогон: настоящие процессы, чужой рабочий стол не трогаем ────

    /// Подставной план из безобидных команд: ни одна не имеет отношения к
    /// обоям, но проходит через ТЕ ЖЕ ветки исполнителя, что и настоящий.
    fn harmless_step(kind: &str, backend: &str, argv: &[&str]) -> wpe::Step {
        wpe::Step {
            kind: kind.to_string(),
            backend: backend.to_string(),
            ensure_swww: false,
            pre: Vec::new(),
            argv: argv.iter().map(|a| a.to_string()).collect(),
            post: Vec::new(),
            ok_message: format!("{backend} отработал (PID {{pid}})"),
            err_message: format!("{backend} не смог: {{stderr}}"),
            alt_message: String::new(),
            format: "static".to_string(),
            terminal: false,
            ok_on_none: false,
            timeout: None,
            verify_ini: None,
            drops_holder: false,
        }
    }

    /// Живой шов БЕЗ единственного побочного действия, которое выходит за
    /// пределы сверки, — вести мосту.
    ///
    /// Мост живёт в `~/.config/rexon/bridge.json`, то есть в настоящем
    /// профиле того, кто запустил `cargo test`; его слушает постороннее
    /// приложение. Писать туда из сверки нельзя даже правду — это уже
    /// разъезд «сверка трогает живой профиль», на котором обжигались с
    /// мостом в сентябре. Всё остальное — процессы, файлы, PATH, часы —
    /// здесь настоящее, иначе прогон не был бы живым.
    struct LiveWithoutBridge {
        inner: LiveSys,
        said: Vec<(bool, String, bool)>,
        /// Подставные `HOME` и `XDG_RUNTIME_DIR`: PID-файл и конфиги должны
        /// лечь в свой каталог, а не в профиль владельца машины.
        sandbox: String,
    }

    impl WpSys for LiveWithoutBridge {
        fn run(&mut self, argv: &[String], timeout: f64) -> Option<RunOut> {
            self.inner.run(argv, timeout)
        }
        fn spawn(&mut self, argv: &[String], new_session: bool) -> Result<ProcHandle, String> {
            self.inner.spawn(argv, new_session)
        }
        fn poll(&mut self, proc: ProcHandle) -> Option<i64> {
            self.inner.poll(proc)
        }
        fn pid(&mut self, proc: ProcHandle) -> Option<i64> {
            self.inner.pid(proc)
        }
        fn kill_process(&mut self, proc: ProcHandle) {
            self.inner.kill_process(proc)
        }
        fn has_tool(&mut self, tool: &str) -> bool {
            self.inner.has_tool(tool)
        }
        fn package_hint(&mut self, tool: &str) -> String {
            self.inner.package_hint(tool)
        }
        fn exists(&mut self, path: &str) -> bool {
            self.inner.exists(path)
        }
        fn is_dir(&mut self, path: &str) -> bool {
            self.inner.is_dir(path)
        }
        fn listdir(&mut self, path: &str) -> Vec<String> {
            self.inner.listdir(path)
        }
        fn read_text(&mut self, path: &str) -> Option<String> {
            self.inner.read_text(path)
        }
        fn write_text(&mut self, path: &str, content: &str) {
            self.inner.write_text(path, content)
        }
        fn remove_file(&mut self, path: &str) {
            self.inner.remove_file(path)
        }
        fn env(&mut self, name: &str) -> String {
            match name {
                "HOME" | "XDG_RUNTIME_DIR" => self.sandbox.clone(),
                _ => self.inner.env(name),
            }
        }
        fn monotonic(&mut self) -> f64 {
            self.inner.monotonic()
        }
        fn sleep(&mut self, seconds: f64) {
            self.inner.sleep(seconds)
        }
        fn bridge_notify(&mut self, live: bool, name: &str, announce: bool) {
            self.said.push((live, name.to_string(), announce));
        }
    }

    /// ⭐ ЖИВОЙ ПРОГОН исполнителя настоящими процессами.
    ///
    /// Всё, что подставной шов изображает, здесь делается по-настоящему:
    /// команда запускается, её код возврата читается, процесс поднимается и
    /// снимается сигналом. Проверяются три вещи, ради которых исполнитель и
    /// написан, — и каждая из них была бы зелёной при поломке, если бы её
    /// мерили только на подставном шве:
    ///
    ///   1. неудача НЕтерминального шага уводит к следующему;
    ///   2. процесс, умерший сразу после запуска, успехом НЕ считается;
    ///   3. живой процесс переживает отсрочку, попадает в состояние и
    ///      снимается по своему дескриптору — не по имени.
    ///
    /// ⛔ Ни одна команда здесь не имеет отношения к обоям: рабочий стол
    /// того, кто запустил сверку, не трогается вовсе.
    #[test]
    #[cfg_attr(
        feature = "isolated-checks",
        ignore = "живой прогон: настоящие подпроцессы машины"
    )]
    fn the_live_executor_runs_real_processes() {
        let sandbox = std::env::temp_dir()
            .join(format!("rexon-wp-exec-{}", std::process::id()))
            .to_string_lossy()
            .into_owned();
        let sys = LiveWithoutBridge {
            inner: LiveSys::new(),
            said: Vec::new(),
            sandbox,
        };
        let mut e = WallpaperExec::with_env(sys, "generic", "x11");

        // 1. Ненулевой код уводит к следующему шагу.
        let plan = vec![
            harmless_step(wpe::step_kind::RUN, "false", &["false"]),
            harmless_step(wpe::step_kind::RUN, "true", &["true"]),
        ];
        let res = e.run_plan(&plan, "/tmp/подставной-путь");
        assert!(res.ok, "живой прогон не дошёл до запасного шага: {}", res.message);
        assert!(res.message.starts_with("true"), "исход отдал не тот шаг: {}", res.message);

        // 2. Процесс, умирающий мгновенно, — не успех. `false` завершается
        //    сразу, то есть внутри отсрочки «жив ли запущенный».
        let plan = vec![harmless_step(wpe::step_kind::SPAWN_SESSION, "false", &["false"])];
        let res = e.run_plan(&plan, "/tmp/подставной-путь");
        assert!(!res.ok, "мгновенно умерший процесс прошёл как успех: {}", res.message);

        // 3. Живой процесс переживает отсрочку и попадает в состояние.
        let plan = vec![harmless_step(
            wpe::step_kind::SPAWN_SESSION,
            "sleep",
            &["sleep", "30"],
        )];
        let res = e.run_plan(&plan, "/tmp/подставной-путь");
        assert!(res.ok, "живой процесс не прошёл: {}", res.message);
        let pid = res.pid.expect("живой запуск не назвал PID");
        assert!(pid > 0);
        assert!(e.status().running, "живой процесс не считается работающим");

        // …и снимается по СВОЕМУ дескриптору. Остановка на окружении
        // `generic` не трогает ни иконок, ни плагинов, ни обоев: возвращать
        // ей нечего, потому что ничего не сохранялось.
        let stopped = e.stop(true);
        assert!(stopped.ok, "{}", stopped.message);
        assert!(!e.status().running, "процесс пережил остановку");
        // PID снятого процесса больше не отзывается (сигнал 0 — только проба).
        let alive = unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
        assert!(!alive, "процесс PID={pid} пережил остановку");
    }


    // ── раскладка обоев по мониторам ──────────────────────────────────────

    /// Журнал вызовов сверки — печатается при `cargo test -- --nocapture`.
    /// Это и есть «что за команды ушли»: сверки ниже сравнивают тот же
    /// журнал строка в строку.
    fn journal(title: &str, e: &mut WallpaperExec<FakeSys>) {
        println!("── {title} ──");
        for cmd in e.sys_mut().ran.clone() {
            println!("  run   {}", cmd.join(" "));
        }
        for (cmd, _) in e.sys_mut().spawned.clone() {
            println!("  spawn {}", cmd.join(" "));
        }
    }

    /// Два выхода в ответе `wlr-randr --json`.
    const WLR_TWO: &str = r#"[
        {"name":"DP-1","enabled":true,"position":{"x":0,"y":0},
         "modes":[{"width":1280,"height":720,"current":false},
                  {"width":2560,"height":1440,"current":true}]},
        {"name":"HDMI-A-1","enabled":true,"position":{"x":2560,"y":0},
         "modes":[{"width":1920,"height":1080,"current":true}]},
        {"name":"DP-9","enabled":false,"modes":[]}
    ]"#;

    /// Те же два выхода, но опрос не сказал, где они стоят.
    const WLR_TWO_NOWHERE: &str = r#"[
        {"name":"DP-1","enabled":true,
         "modes":[{"width":2560,"height":1440,"current":true}]},
        {"name":"HDMI-A-1","enabled":true,
         "modes":[{"width":1920,"height":1080,"current":true}]}
    ]"#;

    /// Один выход в ответе `wlr-randr --json`.
    const WLR_ONE: &str = r#"[{"name":"DP-1","enabled":true,
        "position":{"x":0,"y":0},
        "modes":[{"width":2560,"height":1440,"current":true}]}]"#;

    /// Один выход в ответе `xrandr --current`.
    const XRANDR_ONE: &str = "Screen 0: minimum 320 x 200, current 1920 x 1080\n\
        eDP-1 connected primary 1920x1080+0+0 (normal left inverted) 340mm x 190mm\n\
        \x20  1920x1080     60.00*+\n";

    /// Два выхода в ответе `xrandr --current`.
    const XRANDR_TWO: &str = "Screen 0: minimum 320 x 200, current 3840 x 1080\n\
        DP-1 connected primary 1920x1080+0+0 (normal left inverted) 530mm x 300mm\n\
        \x20  1920x1080     60.00*+  59.94\n\
        HDMI-1 connected 1600x900+1920+0 (normal left inverted) 480mm x 270mm\n\
        \x20  1600x900      60.00*+\n\
        DP-2 disconnected (normal left inverted right x axis y axis)\n";

    /// Разбор `wlr-randr --json`: имя и размер ТЕКУЩЕГО режима, погашенные
    /// выходы — мимо.
    #[test]
    fn outputs_are_read_out_of_wlr_randr() {
        let got = parse_outputs_json(WLR_TWO);
        assert_eq!(
            got,
            vec![
                ScreenOutput {
                    name: "DP-1".into(),
                    resolution: "2560x1440".into(),
                    position: Some((0, 0)),
                },
                ScreenOutput {
                    name: "HDMI-A-1".into(),
                    resolution: "1920x1080".into(),
                    position: Some((2560, 0)),
                },
            ]
        );
    }

    /// Разбор `hyprctl monitors -j`: размер лежит прямо в мониторе, а
    /// выключенный помечен `disabled`.
    #[test]
    fn outputs_are_read_out_of_hyprctl() {
        let json = r#"[{"name":"eDP-1","width":1920,"height":1200,"x":1280,"y":0,
                        "disabled":false},
                       {"name":"DP-3","width":3840,"height":2160,"disabled":true}]"#;
        assert_eq!(
            parse_outputs_json(json),
            vec![ScreenOutput {
                name: "eDP-1".into(),
                resolution: "1920x1200".into(),
                position: Some((1280, 0)),
            }]
        );
    }

    /// Разбор `xrandr --current`: подключённые выходы со своим размером,
    /// отключённые — мимо.
    #[test]
    fn outputs_are_read_out_of_xrandr() {
        assert_eq!(
            parse_xrandr_outputs(XRANDR_TWO),
            vec![
                ScreenOutput {
                    name: "DP-1".into(),
                    resolution: "1920x1080".into(),
                    position: Some((0, 0)),
                },
                ScreenOutput {
                    name: "HDMI-1".into(),
                    resolution: "1600x900".into(),
                    position: Some((1920, 0)),
                },
            ]
        );
    }

    /// Урезанный вывод xrandr (одни строки режимов) — не пустота, а один
    /// безымянный выход с активным разрешением: так вело себя прежнее
    /// правило, и обои от новой раскладки хуже встать не должны.
    #[test]
    fn a_trimmed_xrandr_still_gives_a_resolution() {
        assert_eq!(
            parse_xrandr_outputs("   1280x720     60.00*+\n"),
            vec![ScreenOutput {
                name: "*".into(),
                resolution: "1280x720".into(),
                position: None,
            }]
        );
        // Положения у такого выхода нет — геометрия выходит с прежним
        // нулевым смещением.
        assert_eq!(
            parse_xrandr_outputs("   1280x720     60.00*+\n")[0].geometry(),
            "1280x720+0+0"
        );
    }

    /// swww называет ВСЕ выходы одной командой — это строка таблицы
    /// `List`.
    #[test]
    fn swww_names_every_output_in_one_command() {
        let mut sys = FakeSys::default();
        sys.tools.push("swww".to_string());
        sys.answer("swww", 0, "");
        sys.answer("wlr-randr", 0, WLR_TWO);
        let plan = wpe::static_plan(
            "sway",
            "wayland",
            "/tmp/котик.png",
            &["swww".to_string()],
            "",
        );
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/котик.png");
        assert!(res.ok, "{}", res.message);
        journal("swww, два выхода", &mut e);
        let ran: Vec<String> = e.sys_mut().ran.iter().map(|c| c.join(" ")).collect();
        assert!(
            ran.iter()
                .any(|l| l == "swww img /tmp/котик.png --outputs DP-1,HDMI-A-1"),
            "{ran:?}"
        );
    }

    /// Один выход — та же одна команда, но выход в ней всё равно назван.
    #[test]
    fn swww_names_the_only_output_too() {
        let mut sys = FakeSys::default();
        sys.tools.push("swww".to_string());
        sys.answer("swww", 0, "");
        sys.answer("wlr-randr", 0, WLR_ONE);
        let plan = wpe::static_plan(
            "sway",
            "wayland",
            "/tmp/котик.png",
            &["swww".to_string()],
            "",
        );
        let mut e = exec("sway", "wayland", sys);
        assert!(e.run_plan(&plan, "/tmp/котик.png").ok);
        journal("swww, один выход", &mut e);
        let ran: Vec<String> = e.sys_mut().ran.iter().map(|c| c.join(" ")).collect();
        assert!(
            ran.iter().any(|l| l == "swww img /tmp/котик.png --outputs DP-1"),
            "{ran:?}"
        );
    }

    /// ⚠️ Защита от дурака: инструмента опроса выходов в системе нет —
    /// обои всё равно ставятся, командой ровно той, что была в плане.
    #[test]
    fn without_a_probe_tool_the_command_stays_as_it_was() {
        let mut sys = FakeSys::default();
        sys.tools.push("swww".to_string());
        sys.answer("swww", 0, "");
        for missing in ["wlr-randr", "hyprctl", "xrandr"] {
            sys.missing.push(missing.to_string());
        }
        let plan = wpe::static_plan(
            "sway",
            "wayland",
            "/tmp/котик.png",
            &["swww".to_string()],
            "",
        );
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/котик.png");
        assert!(res.ok, "без опроса выходов обои не встали: {}", res.message);
        journal("swww, инструмента опроса нет", &mut e);
        let ran: Vec<String> = e.sys_mut().ran.iter().map(|c| c.join(" ")).collect();
        assert!(
            ran.iter().any(|l| l == "swww img /tmp/котик.png"),
            "{ran:?}"
        );
        assert!(
            !ran.iter().any(|l| l.contains("--outputs")),
            "выходы названы, хотя их никто не перечислил: {ran:?}"
        );
    }

    /// hyprpaper кроет все мониторы СВОЕЙ формой (пустое имя перед
    /// запятой): команда одна, и выходы для неё не спрашиваются вовсе.
    #[test]
    fn hyprpaper_covers_every_output_by_its_own_form() {
        let mut sys = FakeSys::default();
        sys.tools.push("hyprpaper".to_string());
        sys.answer("wlr-randr", 0, WLR_TWO);
        let plan = wpe::static_plan(
            "hyprland",
            "wayland",
            "/tmp/котик.png",
            &["hyprpaper".to_string()],
            "",
        );
        let mut e = exec("hyprland", "wayland", sys);
        assert!(e.run_plan(&plan, "/tmp/котик.png").ok);
        journal("hyprpaper, два выхода", &mut e);
        let ran: Vec<String> = e.sys_mut().ran.iter().map(|c| c.join(" ")).collect();
        assert_eq!(
            ran,
            vec![
                "hyprctl hyprpaper preload /tmp/котик.png".to_string(),
                "hyprctl hyprpaper wallpaper ,/tmp/котик.png".to_string(),
            ],
            "hyprpaper сходил куда-то ещё"
        );
    }

    /// mpvpaper принимает один выход — значит, на двух мониторах его
    /// поднимают дважды, и в каждом запуске стоит СВОЙ выход.
    #[test]
    fn mpvpaper_is_raised_once_per_output() {
        let mut sys = FakeSys::default();
        sys.tools.push("mpvpaper".to_string());
        sys.answer("wlr-randr", 0, WLR_TWO);
        let plan = wpe::animated_plan(
            "sway",
            "wayland",
            "/tmp/котик.gif",
            &["mpvpaper".to_string()],
            "{monitor}",
            "{resolution}",
            &wpe::Hints::default(),
        );
        let mut e = exec("sway", "wayland", sys);
        let res = e.run_plan(&plan, "/tmp/котик.gif");
        assert!(res.ok, "{}", res.message);
        journal("mpvpaper, два выхода", &mut e);
        let spawned: Vec<String> = e
            .sys_mut()
            .spawned
            .iter()
            .map(|(c, _)| c.join(" "))
            .collect();
        assert_eq!(spawned.len(), 2, "{spawned:?}");
        assert!(spawned[0].ends_with("DP-1 /tmp/котик.gif"), "{spawned:?}");
        assert!(spawned[1].ends_with("HDMI-A-1 /tmp/котик.gif"), "{spawned:?}");
        // Обои на втором мониторе — часть тех же обоев: остановка гасит оба.
        assert_eq!(e.companions.len(), 1, "спутник не запомнен");
        e.stop(true);
        assert_eq!(e.sys_mut().killed.len(), 2, "второй монитор остался играть");
        assert!(e.companions.is_empty(), "спутник пережил остановку");
    }

    /// Один монитор — один запуск: лишнего процесса раскладка не плодит.
    #[test]
    fn mpvpaper_with_one_output_is_raised_once() {
        let mut sys = FakeSys::default();
        sys.tools.push("mpvpaper".to_string());
        sys.answer("wlr-randr", 0, WLR_ONE);
        let plan = wpe::animated_plan(
            "sway",
            "wayland",
            "/tmp/котик.gif",
            &["mpvpaper".to_string()],
            "{monitor}",
            "{resolution}",
            &wpe::Hints::default(),
        );
        let mut e = exec("sway", "wayland", sys);
        assert!(e.run_plan(&plan, "/tmp/котик.gif").ok);
        journal("mpvpaper, один выход", &mut e);
        let spawned: Vec<String> = e
            .sys_mut()
            .spawned
            .iter()
            .map(|(c, _)| c.join(" "))
            .collect();
        assert_eq!(spawned.len(), 1, "{spawned:?}");
        assert!(spawned[0].ends_with("DP-1 /tmp/котик.gif"), "{spawned:?}");
        assert!(e.companions.is_empty(), "лишний процесс на одном мониторе");
    }

    /// ⚠️ Защита от дурака: движок из строки `Each`, а выходы не
    /// перечислились — поднимаем ОДИН процесс с прежним значением метки,
    /// а не ноль и не два одинаковых.
    #[test]
    fn each_engine_without_outputs_is_raised_once() {
        let mut sys = FakeSys::default();
        sys.tools.push("mpvpaper".to_string());
        for missing in ["wlr-randr", "hyprctl", "xrandr"] {
            sys.missing.push(missing.to_string());
        }
        let plan = wpe::animated_plan(
            "sway",
            "wayland",
            "/tmp/котик.gif",
            &["mpvpaper".to_string()],
            "{monitor}",
            "{resolution}",
            &wpe::Hints::default(),
        );
        let mut e = exec("sway", "wayland", sys);
        assert!(e.run_plan(&plan, "/tmp/котик.gif").ok);
        journal("mpvpaper, выходы не перечислились", &mut e);
        let spawned: Vec<String> = e
            .sys_mut()
            .spawned
            .iter()
            .map(|(c, _)| c.join(" "))
            .collect();
        assert_eq!(spawned.len(), 1, "{spawned:?}");
        assert!(spawned[0].ends_with("* /tmp/котик.gif"), "{spawned:?}");
    }

    /// План GIF-обоев под X11 — тот же, что строит исполнитель: геометрию
    /// он просит меткой `{geometry}`.
    fn x11_gif_plan() -> Vec<wpe::Step> {
        let tools = vec!["xwinwrap".to_string(), "mpv".to_string()];
        wpe::animated_plan(
            "generic",
            "x11",
            "/tmp/котик.gif",
            &tools,
            "{monitor}",
            wpe::GEOMETRY_MARK,
            &wpe::Hints::default(),
        )
    }

    /// Исполнитель с xwinwrap и mpv «в системе».
    fn x11_exec(xrandr: Option<&str>) -> WallpaperExec<FakeSys> {
        let mut sys = FakeSys::default();
        sys.tools.push("xwinwrap".to_string());
        sys.tools.push("mpv".to_string());
        match xrandr {
            Some(out) => sys.answer("xrandr", 0, out),
            None => sys.missing.push("xrandr".to_string()),
        }
        exec("generic", "x11", sys)
    }

    /// Что ушло на запуск.
    fn spawned_lines(e: &mut WallpaperExec<FakeSys>) -> Vec<String> {
        e.sys_mut().spawned.iter().map(|(c, _)| c.join(" ")).collect()
    }

    /// Метки `{monitor}` и `{geometry}` берутся из ОДНОГО опроса, и
    /// спрашивается он ровно один раз на весь план — сколько бы команд из
    /// шага ни вышло.
    #[test]
    fn marks_come_from_one_and_the_same_probe() {
        let plan = x11_gif_plan();
        let mut e = x11_exec(Some(XRANDR_TWO));
        assert!(e.run_plan(&plan, "/tmp/котик.gif").ok);
        journal("xwinwrap (X11), два выхода", &mut e);
        let probes = e
            .sys_mut()
            .ran
            .iter()
            .filter(|c| {
                matches!(c.first().map(|b| b.as_str()), Some("xrandr" | "wlr-randr" | "hyprctl"))
            })
            .count();
        assert_eq!(probes, 1, "выходы спрошены {probes} раз вместо одного");
    }

    /// xwinwrap кроет корневое окно по геометрии — значит, на двух
    /// мониторах его поднимают дважды, и у каждого запуска СВОЙ размер со
    /// СВОИМ смещением.
    #[test]
    fn xwinwrap_is_raised_once_per_output() {
        let plan = x11_gif_plan();
        let mut e = x11_exec(Some(XRANDR_TWO));
        let res = e.run_plan(&plan, "/tmp/котик.gif");
        assert!(res.ok, "{}", res.message);
        journal("xwinwrap, два выхода", &mut e);
        let spawned = spawned_lines(&mut e);
        assert_eq!(spawned.len(), 2, "{spawned:?}");
        assert!(spawned[0].contains("-g 1920x1080+0+0"), "{spawned:?}");
        assert!(spawned[1].contains("-g 1600x900+1920+0"), "{spawned:?}");
        // Обои второго монитора — часть тех же обоев: гаснут вместе.
        assert_eq!(e.companions.len(), 1, "спутник не запомнен");
        e.stop(true);
        assert_eq!(e.sys_mut().killed.len(), 2, "второй монитор остался играть");
        assert!(e.companions.is_empty(), "спутник пережил остановку");
    }

    /// Один монитор — одна команда, и геометрия в ней прежняя.
    #[test]
    fn xwinwrap_with_one_output_is_raised_once() {
        let plan = x11_gif_plan();
        let mut e = x11_exec(Some(XRANDR_ONE));
        assert!(e.run_plan(&plan, "/tmp/котик.gif").ok);
        journal("xwinwrap, один выход", &mut e);
        let spawned = spawned_lines(&mut e);
        assert_eq!(spawned.len(), 1, "{spawned:?}");
        assert!(spawned[0].contains("-g 1920x1080+0+0"), "{spawned:?}");
        assert!(e.companions.is_empty(), "лишний процесс на одном мониторе");
    }

    /// ⚠️ Защита от дурака: выходов два, а где они стоят — опрос не сказал.
    /// Вторая команда легла бы поверх первой, поэтому уходит одна, с
    /// прежним нулевым смещением.
    #[test]
    fn xwinwrap_without_output_positions_is_raised_once() {
        let plan = x11_gif_plan();
        let mut e = x11_exec(None);
        e.sys_mut().answer("wlr-randr", 0, WLR_TWO_NOWHERE);
        assert!(e.run_plan(&plan, "/tmp/котик.gif").ok);
        journal("xwinwrap, положение выходов неизвестно", &mut e);
        let spawned = spawned_lines(&mut e);
        assert_eq!(spawned.len(), 1, "{spawned:?}");
        assert!(spawned[0].contains("-g 2560x1440+0+0"), "{spawned:?}");
    }

    /// ⚠️ Защита от дурака: перечислить выходы нечем — команда уходит ровно
    /// та, что была в плане, с умолчанием движка в геометрии.
    #[test]
    fn xwinwrap_without_a_probe_tool_is_raised_once() {
        let plan = x11_gif_plan();
        let mut e = x11_exec(None);
        for missing in ["wlr-randr", "hyprctl"] {
            e.sys_mut().missing.push(missing.to_string());
        }
        assert!(e.run_plan(&plan, "/tmp/котик.gif").ok);
        journal("xwinwrap, инструмента опроса нет", &mut e);
        let spawned = spawned_lines(&mut e);
        assert_eq!(spawned.len(), 1, "{spawned:?}");
        assert!(spawned[0].contains("-g 1920x1080+0+0"), "{spawned:?}");
    }

    /// Старая метка `{resolution}` цела и по-прежнему означает ТОЛЬКО
    /// размер: её берут те, кому смещение не нужно.
    #[test]
    fn the_resolution_mark_still_means_just_the_size() {
        let mut sys = FakeSys::default();
        sys.answer("xrandr", 0, XRANDR_TWO);
        let mut e = exec("generic", "x11", sys);
        let mut seen = Seen::new("x11");
        assert_eq!(
            fill(e.sys_mut(), "{resolution}", &mut seen),
            "1920x1080"
        );
        assert_eq!(
            fill(e.sys_mut(), wpe::GEOMETRY_MARK, &mut seen),
            "1920x1080+0+0"
        );
    }

    /// Таблица движков отвечает за выбор, а не догадка по виду команды:
    /// незнакомый движок ведёт себя как прежде.
    #[test]
    fn an_unknown_engine_keeps_the_old_behaviour() {
        assert_eq!(engine_spread("нет-такого"), Spread::Whole);
        assert_eq!(engine_spread("feh"), Spread::Whole);
        assert_eq!(engine_spread("mpvpaper"), Spread::Each);
        assert!(matches!(engine_spread("swww"), Spread::List { .. }));
    }

    /// Крайние выходы: одна картинка на весь стол меряется по САМОМУ
    /// БОЛЬШОМУ монитору, «окно во весь экран» — по самому маленькому.
    #[test]
    fn the_extreme_outputs_are_picked_by_area() {
        let two = parse_xrandr_outputs(XRANDR_TWO);
        assert_eq!(largest_output(&two).map(|o| o.name.as_str()), Some("DP-1"));
        assert_eq!(largest_output(&two).and_then(|o| o.size()), Some((1920, 1080)));
        assert_eq!(smallest_output(&two).map(|o| o.name.as_str()), Some("HDMI-1"));
        assert_eq!(smallest_output(&two).and_then(|o| o.size()), Some((1600, 900)));
    }

    /// Один выход — он же и самый большой, и самый маленький: ответ тот же,
    /// что давал прежний разбор «первого разрешения».
    #[test]
    fn a_single_output_answers_as_before() {
        let one = vec![ScreenOutput {
            name: "eDP-1".into(),
            resolution: "2560x1600".into(),
            position: Some((0, 0)),
        }];
        assert_eq!(largest_output(&one).and_then(|o| o.size()), Some((2560, 1600)));
        assert_eq!(smallest_output(&one).and_then(|o| o.size()), Some((2560, 1600)));
    }

    /// ⚠️ Защита от дурака: выходов нет вовсе, разрешение не разобралось,
    /// размер нулевой — крайнего выхода не находится, и вызывающий уходит
    /// на своё прежнее поведение.
    #[test]
    fn extremes_survive_a_failed_probe() {
        assert!(largest_output(&[]).is_none());
        assert!(smallest_output(&[]).is_none());
        let junk = vec![
            ScreenOutput { name: "A".into(), resolution: "нет".into(), position: None },
            ScreenOutput { name: "B".into(), resolution: "0x0".into(), position: None },
        ];
        assert!(largest_output(&junk).is_none());
        assert!(smallest_output(&junk).is_none());
        // Разобрался хоть один — берётся он, а не «ничего».
        let mixed = vec![
            ScreenOutput { name: "A".into(), resolution: "нет".into(), position: None },
            ScreenOutput { name: "B".into(), resolution: "1280x720".into(), position: None },
        ];
        assert_eq!(largest_output(&mixed).map(|o| o.name.as_str()), Some("B"));
    }

    /// Равная площадь — решает имя: одна и та же машина обязана давать один
    /// и тот же ответ.
    #[test]
    fn an_equal_area_is_settled_by_the_name() {
        let same = vec![
            ScreenOutput { name: "HDMI-1".into(), resolution: "1920x1080".into(), position: None },
            ScreenOutput { name: "DP-1".into(), resolution: "1920x1080".into(), position: None },
        ];
        assert_eq!(largest_output(&same).map(|o| o.name.as_str()), Some("DP-1"));
        assert_eq!(smallest_output(&same).map(|o| o.name.as_str()), Some("DP-1"));
    }

}
