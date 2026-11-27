//! Заставка при простое на рабочем столе: решения ядра.
//!
//! Заставка внутри самого Steam Big Picture — отдельный модуль
//! [`crate::steam_screensaver`]; здесь — своя заставка Rexon на любом столе.
//!
//! ЧТО ЗДЕСЬ. Только решения, без шины, без Wayland и без окон:
//! * [`route`] — каким путём в этом сеансе узнать о простое и чем показать
//!   картинку поверх всего: штатным модулем хранителя экрана (MATE, XFCE,
//!   Deepin, xscreensaver), своим слоем OVERLAY на Wayland, окном
//!   override-redirect на X11, нашим расширением оболочки (GNOME/Cinnamon на
//!   Wayland) — или честное «здесь нельзя» с причиной;
//! * [`show_policy`] — как исполнитель показа обязан вести себя на этом пути
//!   (страж безопасности экрана блокировки, см. ниже);
//! * [`may_start`] / [`must_stop`] — когда показ можно начать и когда его
//!   надо снять немедленно;
//! * [`read_timers`] и парсеры конфигов окружений — когда окружение само
//!   затемнит, погасит и заблокирует экран ([`DesktopTimers`]), чтобы
//!   заставка вставала раньше ([`order_problem`], [`suggested_delay`]).
//!
//! Исполнители — слушатель простоя в лотке рядом с мостом
//! `screensaver_bridge` и режим показа `rexon-wallpaper-host --screensaver`
//! (оба в `rexon-gui`) — приносят наблюдения сюда параметрами и делают то,
//! что ответило ядро.
//!
//! ⛔ БЕЗОПАСНОСТЬ ЭКРАНА БЛОКИРОВКИ.
//! * X11: хранитель экрана блокирует, только если сумеет захватить
//!   клавиатуру и мышь. Заставка, взявшая захват сама, оставит машину
//!   НЕЗАБЛОКИРОВАННОЙ. Окно, поднимаемое наверх по таймеру, перекроет окно
//!   блокировки. Поэтому на X11 — ни захвата ввода, ни подъёма циклом: ввод
//!   слушается сырыми событиями XInput2 с корня (захвата они не требуют),
//!   окно поднимается один раз при показе и только если хранитель не активен.
//!   В [`RaisePolicy`] варианта «периодически» нет намеренно — его нельзя
//!   даже выразить.
//! * Wayland: поверхности блокировки по протоколу выше слоя OVERLAY; вред
//!   один — заставка останется висеть после разблокировки. Поэтому уход по
//!   первому же сигналу блокировки ([`must_stop`]).
//! * Заставка НИКОГДА не держит экран (`Inhibit`, `XResetScreenSaver`,
//!   `idle-inhibit` на своей поверхности): иначе перестанут гаснуть монитор
//!   и засыпать ноутбук.

use crate::animations::{LookCtx, LookIo};
use crate::screensaver_bridge::{self as bridge, Upstream, UpstreamKind, WaylandFacts};

// ============================================================================
// Сеанс
// ============================================================================

/// Какой графический сеанс.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Wayland,
    X11,
    /// Ни Wayland, ни X11 (консоль, ssh): показывать некуда.
    None,
}

/// Вид сеанса по переменным окружения (не задана — пустая строка).
/// Признак Wayland в Rexon один — [`crate::wallpaper_backend::is_wayland_session`].
pub fn session_kind(xdg_session_type: &str, wayland_display: &str, display: &str) -> SessionKind {
    if crate::wallpaper_backend::is_wayland_session(xdg_session_type, wayland_display) {
        SessionKind::Wayland
    } else if !display.trim().is_empty() {
        SessionKind::X11
    } else {
        SessionKind::None
    }
}

// ============================================================================
// Маршрут
// ============================================================================

/// Штатный модуль хранителя экрана: заставка Rexon встаёт в его список тем,
/// и порядок «заставка → блокировка» держит сам хранитель.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeModule {
    /// mate-screensaver: тема — `.desktop` с `Categories=Screensaver;`,
    /// окно приходит в `XSCREENSAVER_WINDOW`.
    Mate,
    /// xfce4-screensaver: то же устройство, меню garcon.
    Xfce,
    /// deepin-screensaver: модуль QML или исполняемый файл в каталоге модулей.
    Deepin,
    /// xscreensaver: «хак» в списке `programs:`, окно в `-window-id` /
    /// `XSCREENSAVER_WINDOW`.
    XScreenSaver,
}

impl NativeModule {
    /// Идентификатор для настроек и журнала.
    pub fn id(self) -> &'static str {
        match self {
            NativeModule::Mate => "mate-screensaver",
            NativeModule::Xfce => "xfce4-screensaver",
            NativeModule::Deepin => "deepin-screensaver",
            NativeModule::XScreenSaver => "xscreensaver",
        }
    }

    /// Порядок предпочтения, если в сеансе живут несколько хранителей.
    pub fn all() -> &'static [NativeModule] {
        &[NativeModule::Mate, NativeModule::Xfce, NativeModule::Deepin, NativeModule::XScreenSaver]
    }

    /// Имя хранителя на шине сеанса, если оно у него есть. Имена — из общей
    /// таблицы моста ([`bridge::COMPANIONS`]), не своей копией.
    pub fn bus_name(self) -> Option<&'static str> {
        let wanted = match self {
            NativeModule::Mate => "org.mate.ScreenSaver",
            NativeModule::Xfce => "org.xfce.ScreenSaver",
            // deepin-screensaver и xscreensaver узнаются по процессу.
            NativeModule::Deepin | NativeModule::XScreenSaver => return None,
        };
        bridge::COMPANIONS.iter().map(|c| c.service).find(|s| *s == wanted)
    }
}

/// Откуда слушатель узнаёт о простое.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleSource {
    /// Wayland `ext_idle_notifier_v1.get_idle_notification` (версия 1:
    /// учитывает запреты простоя — пока видео держит idle-inhibit, `idled`
    /// не придёт).
    ExtIdleNotify,
    /// D-Bus `org.gnome.Mutter.IdleMonitor` (GNOME). Запреты менеджера сеанса
    /// он не видит — их проверяет [`may_start`].
    MutterIdleMonitor,
    /// Тот же монитор у Muffin (Cinnamon).
    MuffinIdleMonitor,
    /// X11 MIT-SCREEN-SAVER (`XScreenSaverQueryInfo`), опрос раз в 5–10 с.
    XScreenSaverExt,
}

/// Наше расширение оболочки, которое показывает заставку само.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellExtension {
    Gnome,
    Cinnamon,
}

/// Чем показать картинку поверх всего.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowMethod {
    /// wlr-layer-shell, слой OVERLAY, поверхность на каждый вывод.
    LayerShellOverlay,
    /// X11: окно override-redirect на каждый монитор.
    X11OverrideRedirect,
    /// Полноэкранный актёр нашего расширения оболочки (Mutter/Muffin на
    /// Wayland: layer-shell там нет).
    ShellExtension(ShellExtension),
}

/// Почему в этом сеансе своей заставки быть не может. Для честной подписи
/// на вкладке, а не для молчаливого «не работает».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsupported {
    /// Графического сеанса нет.
    NoDisplay,
    /// Сеанс Wayland, но композитор не ответил (слушатель не смог
    /// подключиться): решать не по чему.
    NoCompositorFacts,
    /// GNOME/Cinnamon на Wayland без включённого расширения Rexon: путь есть,
    /// но нужен один шаг человека («включить расширение»).
    ShellExtensionOff(ShellExtension),
    /// Pantheon на Wayland: Gala не даёт ни layer-shell, ни расширений.
    PantheonWayland,
    /// Композитор не объявил нужных глобалов (имена — какие).
    MissingGlobals(Vec<&'static str>),
}

/// Решение: каким путём здесь работает заставка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Игровой режим SteamOS (gamescope): простой и заставку ведёт сам
    /// Steam; своё окно поверх gamescope не показываем. Заставка Rexon там —
    /// папка в `uioverrides` ([`crate::steam_screensaver`]).
    SteamOnly,
    /// Штатный модуль хранителя экрана.
    Native(NativeModule),
    /// Свой показ: слушатель простоя + исполнитель показа.
    Own { idle: IdleSource, show: ShowMethod },
    /// Здесь нельзя — с причиной.
    Unsupported(Unsupported),
}

/// Какие хранители экрана живут в сеансе. Добывает исполнитель: имена на
/// шине — [`NativeSavers::from_bus`], процессы — сам.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeSavers {
    pub mate: bool,
    pub xfce: bool,
    pub deepin: bool,
    pub xscreensaver: bool,
}

impl NativeSavers {
    /// Хранители с именем на шине — по `has_owner`; прочие — фактами.
    pub fn from_bus(has_owner: impl Fn(&str) -> bool, deepin: bool, xscreensaver: bool) -> Self {
        let on_bus = |m: NativeModule| m.bus_name().is_some_and(&has_owner);
        NativeSavers {
            mate: on_bus(NativeModule::Mate),
            xfce: on_bus(NativeModule::Xfce),
            deepin,
            xscreensaver,
        }
    }

    pub fn has(&self, m: NativeModule) -> bool {
        match m {
            NativeModule::Mate => self.mate,
            NativeModule::Xfce => self.xfce,
            NativeModule::Deepin => self.deepin,
            NativeModule::XScreenSaver => self.xscreensaver,
        }
    }
}

/// Наблюдения за сеансом для [`route`].
#[derive(Debug, Clone, Copy)]
pub struct RouteFacts<'a> {
    /// Ответ [`crate::system::detect_current_de`].
    pub de: Option<&'a str>,
    pub session: SessionKind,
    /// Что объявил композитор (тот же снимок, что у моста:
    /// `rexon-gui::screensaver_bridge::wayland_idle::probe`).
    pub wayland: Option<&'a WaylandFacts>,
    /// Задан `GAMESCOPE_WAYLAND_DISPLAY` (игровой режим SteamOS).
    pub gamescope: bool,
    pub savers: NativeSavers,
    /// Включено ли наше расширение оболочки (GNOME/Cinnamon), разбор —
    /// [`crate::wallpaper_engine::parse_shell_ext_state`].
    pub shell_extension_enabled: bool,
}

/// Глобалы композитора, без которых своего показа на Wayland нет.
pub const OVERLAY_GLOBALS_NEEDED: [&str; 2] = ["zwlr_layer_shell_v1", "ext_idle_notifier_v1"];

/// Расширение оболочки для окружения — по той же таблице, что у обоев
/// ([`crate::wallpaper_engine::mutter_family_wayland`]): нужно ровно там,
/// где Mutter/Muffin на Wayland.
fn shell_extension_for(de: &str, session: SessionKind) -> Option<ShellExtension> {
    let session_type = if session == SessionKind::Wayland { "wayland" } else { "x11" };
    if !crate::wallpaper_engine::mutter_family_wayland(de, session_type) {
        return None;
    }
    Some(if de == "gnome" { ShellExtension::Gnome } else { ShellExtension::Cinnamon })
}

/// Каким путём здесь работает заставка (таблица окружений — раздел 2.2
/// плана; там, где можно, решение выводится из фактов, а не из имени стола:
/// поддержку ext-idle-notify и layer-shell композитор объявляет сам).
pub fn route(f: &RouteFacts) -> Route {
    match f.session {
        SessionKind::None => Route::Unsupported(Unsupported::NoDisplay),
        SessionKind::X11 => {
            // Штатный модуль — у живого хранителя. Под MATE это
            // mate-screensaver, под XFCE — xfce4-screensaver, под LXQt-X11 —
            // обычно xscreensaver; порядок — [`NativeModule::all`].
            if let Some(m) = NativeModule::all().iter().copied().find(|m| f.savers.has(*m)) {
                return Route::Native(m);
            }
            // KDE-X11, GNOME-X11, Cinnamon (cinnamon-screensaver без
            // хаков), Budgie-X11, Pantheon-X11, XFCE с light-locker…
            Route::Own { idle: IdleSource::XScreenSaverExt, show: ShowMethod::X11OverrideRedirect }
        }
        SessionKind::Wayland => {
            if f.gamescope || f.wayland.is_some_and(|w| w.compositor == "gamescope") {
                return Route::SteamOnly;
            }
            let de = f.de.unwrap_or("");
            if de == "pantheon" {
                return Route::Unsupported(Unsupported::PantheonWayland);
            }
            if let Some(ext) = shell_extension_for(de, f.session) {
                if !f.shell_extension_enabled {
                    return Route::Unsupported(Unsupported::ShellExtensionOff(ext));
                }
                let idle = match ext {
                    ShellExtension::Gnome => IdleSource::MutterIdleMonitor,
                    ShellExtension::Cinnamon => IdleSource::MuffinIdleMonitor,
                };
                return Route::Own { idle, show: ShowMethod::ShellExtension(ext) };
            }
            let Some(w) = f.wayland else {
                return Route::Unsupported(Unsupported::NoCompositorFacts);
            };
            let missing: Vec<&'static str> = OVERLAY_GLOBALS_NEEDED
                .iter()
                .copied()
                .filter(|g| !w.globals.iter().any(|have| have == g))
                .collect();
            if !missing.is_empty() {
                return Route::Unsupported(Unsupported::MissingGlobals(missing));
            }
            Route::Own { idle: IdleSource::ExtIdleNotify, show: ShowMethod::LayerShellOverlay }
        }
    }
}

// ============================================================================
// Как вести себя при показе (страж блокировки)
// ============================================================================

/// Когда исполнитель показа может поднять своё окно наверх.
///
/// ⛔ Варианта «периодически» нет и быть не должно: окно, которое
/// поднимается по таймеру, перекрывает окно блокировки на X11.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaisePolicy {
    /// Никогда: порядок слоёв держит протокол (OVERLAY), хранитель экрана
    /// (своё окно) или оболочка (расширение).
    Never,
    /// Один раз при показе — и только если хранитель экрана не активен
    /// (`GetActive` = false у всех [`lock_signal_services`]).
    OnceIfSaverInactive,
}

/// Чем ловить ввод, который снимает заставку.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputWatch {
    /// Клавиатура слоя OVERLAY в режиме exclusive: первое нажатие не уходит
    /// в программу под заставкой. Это не захват X11: экран блокировки
    /// Wayland всё равно выше.
    LayerExclusiveKeyboard,
    /// X11: сырые события XInput2 (`XI_RawKeyPress`, `XI_RawButtonPress`,
    /// `XI_RawMotion`) с корневого окна — без захвата.
    XInput2Raw,
    /// Ввод ведёт хозяин окна (хранитель экрана, оболочка); исполнитель
    /// уходит по его сигналу или SIGTERM.
    Owner,
}

/// Правила поведения исполнителя показа на выбранном пути.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShowPolicy {
    /// XGrabKeyboard / xcb_grab_keyboard и любые их обёртки.
    pub grab_keyboard: bool,
    /// XGrabPointer и обёртки.
    pub grab_pointer: bool,
    pub raise: RaisePolicy,
    pub input: InputWatch,
    /// Держать экран (`Inhibit`, `XResetScreenSaver`, `SimulateUserActivity`,
    /// idle-inhibit на поверхности). Всегда `false`.
    pub hold_screen: bool,
    /// Съесть первое нажатие или клик, которым будят заставку.
    pub eat_wake_input: bool,
    /// Первый кадр не нарисован за это время — выйти (невидимая стена).
    pub first_frame_timeout_ms: u32,
    /// Непрерывный показ дольше — выйти (сторож зависшего слушателя).
    pub max_show_s: u32,
}

/// Сторож первого кадра (раздел 5 плана, п. 2).
pub const FIRST_FRAME_TIMEOUT_MS: u32 = 2_000;
/// Самый долгий непрерывный показ: 4 часа.
pub const MAX_SHOW_S: u32 = 4 * 60 * 60;

/// Правила показа для пути; `None` — показывает не Rexon (Steam) или
/// показывать нечем.
pub fn show_policy(r: &Route) -> Option<ShowPolicy> {
    let base = ShowPolicy {
        grab_keyboard: false,
        grab_pointer: false,
        raise: RaisePolicy::Never,
        input: InputWatch::Owner,
        hold_screen: false,
        eat_wake_input: false,
        first_frame_timeout_ms: FIRST_FRAME_TIMEOUT_MS,
        max_show_s: MAX_SHOW_S,
    };
    match r {
        Route::SteamOnly | Route::Unsupported(_) => None,
        // Окно дал хранитель: над ним он сам держит блокировку и гасит нас
        // SIGTERM-ом. Ни поднимать, ни ловить ввод не нужно.
        Route::Native(_) => Some(base),
        Route::Own { show, .. } => Some(match show {
            ShowMethod::LayerShellOverlay => ShowPolicy {
                input: InputWatch::LayerExclusiveKeyboard,
                eat_wake_input: true,
                ..base
            },
            // Клик достаётся нашему окну, потому что оно сверху; под ним никто
            // ничего не получает. Если сверху уже блокировка — клик её.
            ShowMethod::X11OverrideRedirect => ShowPolicy {
                raise: RaisePolicy::OnceIfSaverInactive,
                input: InputWatch::XInput2Raw,
                eat_wake_input: true,
                ..base
            },
            ShowMethod::ShellExtension(_) => ShowPolicy { eat_wake_input: true, ..base },
        }),
    }
}

// ============================================================================
// Сигналы блокировки и запреты простоя: откуда их брать
// ============================================================================

/// Службы, у которых слушаем `ActiveChanged(true)` и спрашиваем `GetActive`:
/// `org.freedesktop.ScreenSaver` (KDE и те, кто его держит) и хранители
/// окружений из общей таблицы моста.
pub fn lock_signal_services() -> Vec<&'static str> {
    let mut out = vec![bridge::SERVICE];
    out.extend(bridge::COMPANIONS.iter().map(|c| c.service));
    out
}

/// logind: сигнал `Session.Lock`, свойство `Session.LockedHint` и
/// `Manager.PrepareForSleep` — общие для всех окружений (wlroots-блокировщики
/// дают только `LockedHint`, и то не все).
pub const LOGIN1_SERVICE: &str = "org.freedesktop.login1";
pub const LOGIN1_SESSION_IFACE: &str = "org.freedesktop.login1.Session";
pub const LOGIN1_MANAGER_IFACE: &str = "org.freedesktop.login1.Manager";

/// Как спросить у службы, держит ли кто-нибудь экран сейчас: метод и флаг.
/// Службы — те же, куда мост пробрасывает удержания ([`bridge::UPSTREAMS`]).
pub fn inhibit_query(u: &Upstream) -> Option<(&'static str, Option<u32>)> {
    match u.kind {
        UpstreamKind::GnomeSession => Some(("IsInhibited", Some(bridge::GSM_INHIBIT_IDLE))),
        UpstreamKind::PowerManagement => Some(("HasInhibit", None)),
        UpstreamKind::Companion | UpstreamKind::WaylandIdleInhibit => None,
    }
}

// ============================================================================
// Когда можно начать и когда уходить
// ============================================================================

/// Настройки заставки, которые влияют на решения.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Через сколько секунд простоя от сети.
    pub idle_after_ac_s: u32,
    /// Через сколько на батарее; `None` — на батарее не показывать
    /// (по умолчанию, как у Valve: батарея дороже).
    pub idle_after_battery_s: Option<u32>,
    /// Показывать поверх окна на весь экран и запущенной игры Steam. По
    /// умолчанию нет: игра на геймпаде простоем выглядит, а заставка посреди
    /// неё — худшее, что можно сделать.
    pub show_over_fullscreen: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { idle_after_ac_s: 180, idle_after_battery_s: None, show_over_fullscreen: false }
    }
}

impl Settings {
    /// Порог простоя сейчас; `None` — сейчас не показывать вовсе.
    pub fn threshold_s(&self, on_battery: bool) -> Option<u32> {
        if on_battery {
            self.idle_after_battery_s
        } else {
            Some(self.idle_after_ac_s)
        }
    }
}

/// Кто держит экран прямо сейчас.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inhibits {
    /// `org.gnome.SessionManager.IsInhibited(8)` (GNOME, Cinnamon, MATE, Budgie).
    pub session_idle: bool,
    /// `org.freedesktop.PowerManagement.Inhibit.HasInhibit()` (KDE, XFCE).
    pub power_management: bool,
    /// Удержаний в книге нашего моста (`screensaver_bridge::Book::active`).
    pub bridge_holds: usize,
}

impl Inhibits {
    pub fn any(&self) -> bool {
        self.session_idle || self.power_management || self.bridge_holds > 0
    }
}

/// Снимок сеанса в момент, когда слушатель решает, показывать ли.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Moment {
    /// Простой в мс (для ext-idle-notify — порог, на котором пришёл `idled`).
    pub idle_ms: u64,
    pub on_battery: bool,
    /// `LockedHint` logind или `GetActive` = true у любого хранителя.
    pub locked: bool,
    /// Экран погашен (DPMS off).
    pub display_off: bool,
    /// Пришёл `PrepareForSleep(true)`, сна ещё не было.
    pub preparing_sleep: bool,
    /// Сеанс активен (logind `Active`): не переключились на другой VT или
    /// пользователя.
    pub session_active: bool,
    pub inhibits: Inhibits,
    /// Окно в фокусе — на весь экран.
    pub fullscreen_focused: bool,
    /// Запущена игра Steam.
    pub steam_game_running: bool,
}

/// Почему показ сейчас не начинаем.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Locked,
    PreparingSleep,
    SessionInactive,
    DisplayOff,
    /// Кто-то держит экран (видео).
    Inhibited,
    /// На батарее заставка выключена.
    OnBattery,
    Fullscreen,
    SteamGame,
    NotIdleYet,
}

/// Можно ли начать показ сейчас. Порядок проверок — сначала безопасность
/// (блокировка, сон, чужой сеанс), потом вежливость (видео, батарея, игра),
/// потом время.
pub fn may_start(m: &Moment, s: &Settings) -> Result<(), Refusal> {
    safety_check(m)?;
    if m.inhibits.any() {
        return Err(Refusal::Inhibited);
    }
    let Some(threshold_s) = s.threshold_s(m.on_battery) else {
        return Err(Refusal::OnBattery);
    };
    if !s.show_over_fullscreen {
        if m.steam_game_running {
            return Err(Refusal::SteamGame);
        }
        if m.fullscreen_focused {
            return Err(Refusal::Fullscreen);
        }
    }
    if m.idle_ms < u64::from(threshold_s) * 1000 {
        return Err(Refusal::NotIdleYet);
    }
    Ok(())
}

/// Предпросмотр по кнопке: простой, видео, батарея и игра не важны (человек
/// сам нажал), но поверх блокировки, во сне и в чужом сеансе — никогда.
pub fn may_preview(m: &Moment) -> Result<(), Refusal> {
    safety_check(m)
}

fn safety_check(m: &Moment) -> Result<(), Refusal> {
    if m.locked {
        return Err(Refusal::Locked);
    }
    if m.preparing_sleep {
        return Err(Refusal::PreparingSleep);
    }
    if !m.session_active {
        return Err(Refusal::SessionInactive);
    }
    if m.display_off {
        return Err(Refusal::DisplayOff);
    }
    Ok(())
}

/// Событие во время показа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Нажатие, клик, движение мыши.
    Input,
    /// Источник простоя сообщил «пользователь вернулся» (`resumed`,
    /// `AddUserActiveWatch`, idle MIT-SCREEN-SAVER сбросился).
    IdleResumed,
    /// `ActiveChanged(b)` любой из [`lock_signal_services`].
    SaverActiveChanged(bool),
    /// Свойство logind `LockedHint`.
    LockedHint(bool),
    /// Сигнал logind `Session.Lock`.
    SessionLock,
    /// `PrepareForSleep(b)`.
    PrepareForSleep(bool),
    /// Экран включён (`true`) или погашен (`false`).
    DisplayPower(bool),
    /// Сеанс стал активным / неактивным (смена VT, пользователя).
    SessionActive(bool),
    /// Переход на батарею (`true`) или на сеть (`false`).
    OnBattery(bool),
    /// Появилось удержание экрана (запустили видео по расписанию).
    InhibitAdded,
    /// Исполнитель простоя пропал (лоток упал).
    ListenerGone,
    /// Первый кадр не нарисован за [`FIRST_FRAME_TIMEOUT_MS`].
    FirstFrameTimeout,
    /// Показ длится дольше [`MAX_SHOW_S`].
    ShownTooLong,
}

/// Снять ли показ немедленно. `preview` — показ по кнопке «Предпросмотр»:
/// его не снимают батарея и удержания (человек сам попросил), всё прочее —
/// снимает.
pub fn must_stop(e: &Event, s: &Settings, preview: bool) -> bool {
    match *e {
        Event::Input
        | Event::IdleResumed
        | Event::SaverActiveChanged(true)
        | Event::LockedHint(true)
        | Event::SessionLock
        | Event::PrepareForSleep(true)
        | Event::DisplayPower(false)
        | Event::SessionActive(false)
        | Event::ListenerGone
        | Event::FirstFrameTimeout
        | Event::ShownTooLong => true,
        Event::OnBattery(true) => !preview && s.idle_after_battery_s.is_none(),
        Event::InhibitAdded => !preview,
        Event::SaverActiveChanged(false)
        | Event::LockedHint(false)
        | Event::PrepareForSleep(false)
        | Event::DisplayPower(true)
        | Event::SessionActive(true)
        | Event::OnBattery(false) => false,
    }
}


// ============================================================================
// Таймеры окружения: когда оно затемнит, погасит и заблокирует экран
// ============================================================================
//
// Свой показ обязан встать РАНЬШЕ, чем окружение затемнит экран, погасит его
// или заблокирует: иначе заставку никто не увидит. Здесь — чтение этих
// таймеров (только чтение: чужую политику питания Rexon не меняет). Разбор —
// чистые функции над текстом конфигов и значениями gsettings/xfconf;
// значения добывают обёртки над [`LookIo`] ([`gsettings_reader`],
// [`xfconf_reader`]), в тестах — подделки.
//
// Умолчания и единицы — по схемам самих окружений (gschema, kcfg, заголовки
// xfconf-ключей) и их документации; где у окружения умолчание считается в
// коде, это сказано у функции.

/// Шаг, которым окружение уводит экран от заставки.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// Затемнение подсветки.
    Dim,
    /// Экран гаснет любым способом: DPMS, хранитель экрана окружения, сон.
    Blank,
    /// Блокировка.
    Lock,
}

/// Таймеры простоя окружения, секунды от начала простоя; `None` — шага нет.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DesktopTimers {
    pub dim_s: Option<u32>,
    pub blank_s: Option<u32>,
    pub lock_s: Option<u32>,
}

impl DesktopTimers {
    /// Самый ранний шаг (при равенстве — более жёсткий: блокировка важнее).
    pub fn first(&self) -> Option<(Stage, u32)> {
        [(Stage::Dim, self.dim_s), (Stage::Blank, self.blank_s), (Stage::Lock, self.lock_s)]
            .into_iter()
            .filter_map(|(st, t)| t.map(|t| (st, t)))
            .min_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
    }

    fn note(&mut self, stage: Stage, t: u32) {
        let slot = match stage {
            Stage::Dim => &mut self.dim_s,
            Stage::Blank => &mut self.blank_s,
            Stage::Lock => &mut self.lock_s,
        };
        *slot = Some(slot.map_or(t, |old| old.min(t)));
    }
}

/// Нарушен ли порядок «заставка → затемнение/гашение/блокировка» при
/// задержке заставки `t_ss`: какой шаг окружения наступит раньше или
/// одновременно. Это подсказка для вкладки, не ошибка. Только для своего
/// показа: у штатного модуля порядок держит сам хранитель экрана.
pub fn order_problem(t_ss: u32, timers: &DesktopTimers) -> Option<(Stage, u32)> {
    timers.first().filter(|(_, t)| *t <= t_ss)
}

/// Задержка заставки, которая точно успеет до первого шага окружения:
/// на 20 % раньше (но не меньше чем на 10 с и не короче минуты). Если
/// окружение ничего не делает — умолчание [`Settings`].
pub fn suggested_delay(timers: &DesktopTimers) -> u32 {
    let default = Settings::default().idle_after_ac_s;
    let Some((_, first)) = timers.first() else {
        return default;
    };
    let margin = (first / 5).max(10);
    first.saturating_sub(margin).clamp(60, default.max(60))
}

/// Профиль питания KDE (группы `powerdevilrc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerProfile {
    Ac,
    Battery,
    LowBattery,
}

impl PowerProfile {
    pub fn group(self) -> &'static str {
        match self {
            PowerProfile::Ac => "AC",
            PowerProfile::Battery => "Battery",
            PowerProfile::LowBattery => "LowBattery",
        }
    }

    pub fn from_battery(on_battery: bool) -> Self {
        if on_battery {
            PowerProfile::Battery
        } else {
            PowerProfile::Ac
        }
    }
}

fn positive(v: i64) -> Option<u32> {
    (v > 0).then(|| u32::try_from(v).unwrap_or(u32::MAX))
}

fn parse_int(v: Option<String>) -> Option<i64> {
    v?.trim().parse::<i64>().ok()
}

fn parse_bool(v: Option<String>) -> Option<bool> {
    crate::system_plan::kconfig_bool(&v?)
}

fn minutes(v: u32) -> u32 {
    v.saturating_mul(60)
}

// ── KDE Plasma 6 ────────────────────────────────────────────────────────────

/// Plasma 6: `kscreenlockerrc` + `powerdevilrc`.
///
/// * `[Daemon] Autolock` (да) и `Timeout` — МИНУТЫ, дробные (5);
/// * `[<профиль>][Display] DimDisplayWhenIdle` (да) +
///   `DimDisplayIdleTimeoutSec`, `TurnOffDisplayWhenIdle` (да) +
///   `TurnOffDisplayIdleTimeoutSec`, `LockBeforeTurnOffDisplay` (да только
///   на «мобильных»). Умолчания секунд PowerDevil считает в коде по профилю
///   и признаку «мобильное устройство» — они повторены в [`kde_defaults`].
pub fn kde_timers(
    kscreenlockerrc: Option<&str>,
    powerdevilrc: Option<&str>,
    profile: PowerProfile,
    is_mobile: bool,
) -> DesktopTimers {
    let ksl = |k: &str| kscreenlockerrc.and_then(|t| crate::shell_palette::ini_get(t, "Daemon", k));
    let group = format!("{}][Display", profile.group());
    let pd = |k: &str| powerdevilrc.and_then(|t| crate::shell_palette::ini_get(t, &group, k));
    let (dim_default, off_default) = kde_defaults(profile, is_mobile);

    let mut t = DesktopTimers::default();
    if parse_bool(pd("DimDisplayWhenIdle")).unwrap_or(true) {
        t.dim_s = positive(parse_int(pd("DimDisplayIdleTimeoutSec")).unwrap_or(dim_default));
    }
    if parse_bool(pd("TurnOffDisplayWhenIdle")).unwrap_or(true) {
        t.blank_s = positive(parse_int(pd("TurnOffDisplayIdleTimeoutSec")).unwrap_or(off_default));
    }
    if parse_bool(ksl("Autolock")).unwrap_or(true) {
        let mins = ksl("Timeout").and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(5.0);
        t.lock_s = positive((mins * 60.0).round() as i64);
    }
    if parse_bool(pd("LockBeforeTurnOffDisplay")).unwrap_or(is_mobile) {
        if let Some(off) = t.blank_s {
            t.note(Stage::Lock, off);
        }
    }
    t
}

/// Умолчания PowerDevil (затемнение, гашение), секунды.
pub fn kde_defaults(profile: PowerProfile, is_mobile: bool) -> (i64, i64) {
    match (profile, is_mobile) {
        (PowerProfile::Ac, _) => (300, 600),
        (PowerProfile::Battery, false) => (120, 300),
        (PowerProfile::LowBattery, false) => (60, 120),
        (PowerProfile::Battery, true) => (60, 120),
        (PowerProfile::LowBattery, true) => (30, 60),
    }
}

// ── gsettings: GNOME, Cinnamon, MATE, Budgie ─────────────────────────────────

/// Источник значений «схема/канал, ключ → строка» (gsettings или xfconf).
pub type Get<'a> = &'a dyn Fn(&str, &str) -> Option<String>;

/// Значения gsettings через уже принятый в Rexon способ
/// ([`crate::animations::gsettings_get`]: `gsettings get`, типовая приставка
/// `uint32` срезается).
pub fn gsettings_reader(io: &dyn LookIo) -> impl Fn(&str, &str) -> Option<String> + '_ {
    move |schema, key| crate::animations::gsettings_get(io, schema, key)
}

/// Значения xfconf ([`crate::animations::xfconf_get`]).
pub fn xfconf_reader(io: &dyn LookIo) -> impl Fn(&str, &str) -> Option<String> + '_ {
    move |channel, prop| crate::animations::xfconf_get(io, channel, prop)
}

/// GNOME (gnome-shell + gnome-settings-daemon).
///
/// * `org.gnome.desktop.session idle-delay` (с, 300; 0 — никогда) — щит
///   экрана: экран гаснет;
/// * блокировка — `idle-delay + org.gnome.desktop.screensaver lock-delay`
///   (с, 0), если `lock-enabled` (да) и не `org.gnome.desktop.lockdown
///   disable-lock-screen`;
/// * затемнение — если `org.gnome.settings-daemon.plugins.power idle-dim`
///   (да): на половине `idle-delay` (короче 10 с — не затемняет), а при
///   `idle-delay` = 0 — через 60 с. Режим энергосбережения затемняет через
///   30 с — его не видно в настройках, не учитываем.
pub fn gnome_timers(get: Get) -> DesktopTimers {
    let idle = parse_int(get("org.gnome.desktop.session", "idle-delay")).unwrap_or(300).max(0) as u32;
    let mut t = DesktopTimers { blank_s: positive(idle.into()), ..Default::default() };
    t.lock_s = gnome_family_lock(get, "org.gnome.desktop", idle, 1);
    if parse_bool(get("org.gnome.settings-daemon.plugins.power", "idle-dim")).unwrap_or(true) {
        t.dim_s = if idle == 0 { Some(60) } else { Some(idle / 2).filter(|d| *d >= 10) };
    }
    t
}

/// Блокировка у наследников gnome-screensaver: `<prefix>.screensaver
/// lock-enabled` / `lock-delay` (в `unit` секундах) после активации в
/// `idle_s`, если её не запретил `<prefix>.lockdown disable-lock-screen`.
fn gnome_family_lock(get: Get, prefix: &str, idle_s: u32, unit: u32) -> Option<u32> {
    if idle_s == 0 {
        return None;
    }
    let saver = format!("{prefix}.screensaver");
    let enabled = parse_bool(get(&saver, "lock-enabled")).unwrap_or(true);
    let forbidden = parse_bool(get(&format!("{prefix}.lockdown"), "disable-lock-screen")).unwrap_or(false);
    if !enabled || forbidden {
        return None;
    }
    let delay = parse_int(get(&saver, "lock-delay")).unwrap_or(0).max(0) as u32;
    Some(idle_s.saturating_add(delay.saturating_mul(unit)))
}

/// Cinnamon (cinnamon-screensaver + cinnamon-settings-daemon).
///
/// * `org.cinnamon.desktop.session idle-delay` (с, 900) — включается
///   хранитель, если `org.cinnamon.desktop.screensaver
///   idle-activation-enabled` (да);
/// * блокировка — плюс `lock-delay` (с, 0), если `lock-enabled` (да);
/// * `org.cinnamon.settings-daemon.plugins.power`: затемнение
///   `idle-dim-ac` (нет) / `idle-dim-battery` (да) через `idle-dim-time`
///   (с, 90); гашение `sleep-display-ac` / `sleep-display-battery` (с, 1800;
///   0 — никогда).
pub fn cinnamon_timers(get: Get, on_battery: bool) -> DesktopTimers {
    let idle = parse_int(get("org.cinnamon.desktop.session", "idle-delay")).unwrap_or(900).max(0) as u32;
    let activates =
        parse_bool(get("org.cinnamon.desktop.screensaver", "idle-activation-enabled")).unwrap_or(true);
    let saver_s = if activates { idle } else { 0 };
    let mut t = DesktopTimers {
        blank_s: positive(saver_s.into()),
        lock_s: gnome_family_lock(get, "org.cinnamon.desktop", saver_s, 1),
        ..Default::default()
    };
    power_dim_and_sleep(get, "org.cinnamon.settings-daemon.plugins.power", on_battery, (false, true), 90, (1800, 1800), &mut t);
    t
}

/// MATE (mate-screensaver + mate-power-manager).
///
/// * `org.mate.session idle-delay` — МИНУТЫ (5) — включается хранитель, если
///   `org.mate.screensaver idle-activation-enabled` (да);
/// * блокировка — плюс `lock-delay` — МИНУТЫ (0), если `lock-enabled` (да)
///   и не `org.mate.lockdown disable-lock-screen`;
/// * `org.mate.power-manager`: затемнение `idle-dim-ac` (нет) /
///   `idle-dim-battery` (да) через `idle-dim-time` (с, 10); гашение
///   `sleep-display-ac` (с, 1800) / `sleep-display-battery` (с, 600).
pub fn mate_timers(get: Get, on_battery: bool) -> DesktopTimers {
    let idle_min = parse_int(get("org.mate.session", "idle-delay")).unwrap_or(5).max(0) as u32;
    let activates = parse_bool(get("org.mate.screensaver", "idle-activation-enabled")).unwrap_or(true);
    let saver_s = if activates { minutes(idle_min) } else { 0 };
    let mut t = DesktopTimers {
        blank_s: positive(saver_s.into()),
        lock_s: gnome_family_lock(get, "org.mate", saver_s, 60),
        ..Default::default()
    };
    power_dim_and_sleep(get, "org.mate.power-manager", on_battery, (false, true), 10, (1800, 600), &mut t);
    t
}

/// Общая часть менеджеров питания семейства gnome-power-manager (MATE и
/// Cinnamon): затемнение и гашение экрана, секунды.
fn power_dim_and_sleep(
    get: Get,
    schema: &str,
    on_battery: bool,
    dim_default: (bool, bool),
    dim_time_default: i64,
    sleep_default: (i64, i64),
    t: &mut DesktopTimers,
) {
    let (dim_key, dim_on, sleep_key, sleep_def) = if on_battery {
        ("idle-dim-battery", dim_default.1, "sleep-display-battery", sleep_default.1)
    } else {
        ("idle-dim-ac", dim_default.0, "sleep-display-ac", sleep_default.0)
    };
    if parse_bool(get(schema, dim_key)).unwrap_or(dim_on) {
        if let Some(d) = positive(parse_int(get(schema, "idle-dim-time")).unwrap_or(dim_time_default)) {
            t.note(Stage::Dim, d);
        }
    }
    if let Some(s) = positive(parse_int(get(schema, sleep_key)).unwrap_or(sleep_def)) {
        t.note(Stage::Blank, s);
    }
}

/// Budgie. На X11 (10.9) — хранитель на схемах GNOME и gnome-settings-daemon,
/// таймеры как у [`gnome_timers`]. На Wayland (10.10, labwc) budgie-daemon
/// сам собирает строку `swayidle -w` из тех же схем:
/// * затемнение — `com.solus-project.budgie-wm dimming-delay` (с; 0 — нет;
///   сжимается в 30…1800), если `org.gnome.settings-daemon.plugins.power
///   idle-dim` (да). ⚠️ budgie-daemon ставит его, только когда доступна
///   подсветка, — этого отсюда не видно;
/// * гашение — `org.gnome.desktop.session idle-delay` (с, 300);
/// * блокировка — `idle-delay + lock-delay`, как у GNOME.
pub fn budgie_timers(get: Get, wayland: bool) -> DesktopTimers {
    if !wayland {
        return gnome_timers(get);
    }
    let idle = parse_int(get("org.gnome.desktop.session", "idle-delay")).unwrap_or(300).max(0) as u32;
    let mut t = DesktopTimers {
        blank_s: positive(idle.into()),
        lock_s: gnome_family_lock(get, "org.gnome.desktop", idle, 1),
        ..Default::default()
    };
    if parse_bool(get("org.gnome.settings-daemon.plugins.power", "idle-dim")).unwrap_or(true) {
        t.dim_s = parse_int(get("com.solus-project.budgie-wm", "dimming-delay"))
            .and_then(positive)
            .map(|d| d.clamp(30, 1800));
    }
    t
}

// ── XFCE: xfconf ────────────────────────────────────────────────────────────

/// XFCE: канал `xfce4-screensaver` (если хранитель стоит) и канал
/// `xfce4-power-manager` (если стоит менеджер питания).
///
/// * `/saver/enabled` (да), `/saver/idle-activation/enabled` (да),
///   `/saver/idle-activation/delay` — МИНУТЫ (5);
/// * блокировка: `/lock/enabled` (да), `/lock/saver-activation/enabled` (да),
///   `/lock/saver-activation/delay` — МИНУТЫ (0) после хранителя;
/// * `/xfce4-power-manager/dpms-enabled` (да): `dpms-on-ac-sleep` /
///   `-off` (мин, 10/15), `dpms-on-battery-sleep` / `-off` (мин, 5/10);
///   0 — никогда;
/// * затемнение `brightness-on-ac` / `brightness-on-battery` (с, 9/300),
///   значение 9 — «никогда».
pub fn xfce_timers(get: Get, saver_installed: bool, power_manager_installed: bool, on_battery: bool) -> DesktopTimers {
    let mut t = DesktopTimers::default();
    if saver_installed {
        let ch = "xfce4-screensaver";
        let on = |p: &str| parse_bool(get(ch, p)).unwrap_or(true);
        if on("/saver/enabled") && on("/saver/idle-activation/enabled") {
            let saver = positive(minutes(parse_int(get(ch, "/saver/idle-activation/delay")).unwrap_or(5).max(0) as u32).into());
            t.blank_s = saver;
            if let Some(s) = saver {
                if on("/lock/enabled") && on("/lock/saver-activation/enabled") {
                    let d = parse_int(get(ch, "/lock/saver-activation/delay")).unwrap_or(0).max(0) as u32;
                    t.lock_s = Some(s.saturating_add(minutes(d)));
                }
            }
        }
    }
    if power_manager_installed {
        let ch = "xfce4-power-manager";
        let prop = |k: &str| get(ch, &format!("/xfce4-power-manager/{k}"));
        let (sleep_k, off_k, sleep_d, off_d, bri_k, bri_d) = if on_battery {
            ("dpms-on-battery-sleep", "dpms-on-battery-off", 5, 10, "brightness-on-battery", 300)
        } else {
            ("dpms-on-ac-sleep", "dpms-on-ac-off", 10, 15, "brightness-on-ac", 9)
        };
        if parse_bool(prop("dpms-enabled")).unwrap_or(true) {
            for (k, d) in [(sleep_k, sleep_d), (off_k, off_d)] {
                if let Some(m) = positive(parse_int(prop(k)).unwrap_or(d)) {
                    t.note(Stage::Blank, minutes(m));
                }
            }
        }
        let bri = parse_int(prop(bri_k)).unwrap_or(bri_d);
        if bri != XFPM_BRIGHTNESS_NEVER {
            if let Some(s) = positive(bri) {
                t.note(Stage::Dim, s);
            }
        }
    }
    t
}

/// У xfce4-power-manager наименьшее значение ползунка затемнения значит
/// «никогда».
const XFPM_BRIGHTNESS_NEVER: i64 = 9;

// ── hypridle, swayidle ──────────────────────────────────────────────────────

/// Одно правило «через N секунд простоя выполнить команду».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdleRule {
    pub timeout_s: u32,
    pub command: String,
}

/// Что делает команда простоя: опознаём по общеизвестным программам.
/// `lock_cmd` — команда блокировки самого демона (`general:lock_cmd` у
/// hypridle, событие `lock` у swayidle): ею кончается `loginctl lock-session`.
pub fn classify_idle_command(cmd: &str, lock_cmd: Option<&str>) -> Option<Stage> {
    const LOCK: &[&str] = &["lock-session", "hyprlock", "swaylock", "gtklock", "waylock", "i3lock", "xsecurelock"];
    const BLANK: &[&str] = &[
        "dpms off",
        "dpms, off",
        "power off",
        "power-off-monitors",
        "wlopm --off",
        "xset dpms force",
        "systemctl suspend",
        "systemctl hibernate",
        "systemctl hybrid-sleep",
        "suspend-then-hibernate",
        "loginctl suspend",
    ];
    const DIM: &[&str] = &["brightnessctl", "light -S", "light -U", "Screenlock.Dim", "ddcutil"];
    let c = cmd.trim();
    if c.is_empty() {
        return None;
    }
    if LOCK.iter().any(|m| c.contains(m)) || lock_cmd.is_some_and(|l| !l.trim().is_empty() && c.contains(l.trim())) {
        return Some(Stage::Lock);
    }
    if BLANK.iter().any(|m| c.contains(m)) {
        return Some(Stage::Blank);
    }
    if DIM.iter().any(|m| c.contains(m)) {
        return Some(Stage::Dim);
    }
    None
}

/// Таймеры из правил демона простоя.
pub fn timers_from_rules(rules: &[IdleRule], lock_cmd: Option<&str>) -> DesktopTimers {
    let mut t = DesktopTimers::default();
    for r in rules {
        if r.timeout_s == 0 {
            continue;
        }
        if let Some(stage) = classify_idle_command(&r.command, lock_cmd) {
            t.note(stage, r.timeout_s);
        }
    }
    t
}

/// Разбор `hypridle.conf` (синтаксис hyprlang): блоки `listener { timeout =
/// N; on-timeout = … }` и `general { lock_cmd = … }`. Комментарии — `#`
/// (`##` — сама решётка). Переменные `$имя = …` подставляются.
pub fn parse_hypridle(text: &str) -> (Vec<IdleRule>, Option<String>) {
    let mut vars: Vec<(String, String)> = Vec::new();
    let mut rules = Vec::new();
    let mut lock_cmd = None;
    let mut stack: Vec<String> = Vec::new();
    let mut cur: Option<(Option<u32>, String)> = None;
    for raw in text.lines() {
        let line = strip_hypr_comment(raw);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_suffix('{') {
            let name = name.trim().to_string();
            if name == "listener" {
                cur = Some((None, String::new()));
            }
            stack.push(name);
            continue;
        }
        if line == "}" {
            if stack.pop().as_deref() == Some("listener") {
                if let Some((Some(timeout_s), command)) = cur.take() {
                    rules.push(IdleRule { timeout_s, command });
                }
                cur = None;
            }
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let mut v = v.trim().to_string();
        for (name, value) in vars.iter().rev() {
            v = v.replace(&format!("${name}"), value);
        }
        if let Some(name) = k.strip_prefix('$') {
            if stack.is_empty() {
                vars.push((name.to_string(), v));
                vars.sort_by(|a, b| a.0.len().cmp(&b.0.len()));
            }
            continue;
        }
        match (stack.last().map(String::as_str), k) {
            (Some("general"), "lock_cmd") => lock_cmd = Some(v),
            (Some("listener"), "timeout") => {
                if let Some(c) = cur.as_mut() {
                    c.0 = v.parse::<u32>().ok();
                }
            }
            (Some("listener"), "on-timeout") => {
                if let Some(c) = cur.as_mut() {
                    c.1 = v;
                }
            }
            _ => {}
        }
    }
    (rules, lock_cmd)
}

fn strip_hypr_comment(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '#' {
            if chars.peek() == Some(&'#') {
                chars.next();
                out.push('#');
                continue;
            }
            break;
        }
        out.push(c);
    }
    out
}

/// Таймеры hypridle.
pub fn hypridle_timers(text: &str) -> DesktopTimers {
    let (rules, lock_cmd) = parse_hypridle(text);
    timers_from_rules(&rules, lock_cmd.as_deref())
}

/// Слова командной строки по правилам sh: одинарные кавычки — буквально,
/// двойные — с `\`, вне кавычек `\` экранирует следующий знак.
pub fn split_shell_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    cur.push(q);
                }
            }
            '"' => {
                in_word = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' => {
                            if let Some(n) = chars.next() {
                                if !matches!(n, '"' | '\\' | '$' | '`') {
                                    cur.push('\\');
                                }
                                cur.push(n);
                            }
                        }
                        _ => cur.push(q),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        out.push(cur);
    }
    out
}

/// Аргументы swayidle из строки запуска (`exec swayidle -w timeout 300 …`
/// в конфиге sway/river/niri/Hyprland, `Exec=` автозапуска, строка
/// процесса) — всё после слова `swayidle`; `None` — swayidle в строке нет.
pub fn swayidle_args_in_line(line: &str) -> Option<Vec<String>> {
    let words = split_shell_words(line);
    let at = words.iter().position(|w| w.rsplit('/').next() == Some("swayidle"))?;
    Some(words[at + 1..].to_vec())
}

/// Аргументы из файла конфигурации swayidle (`-C`, по умолчанию
/// `$XDG_CONFIG_HOME/swayidle/config`): строки — те же команды, что в
/// командной строке, `#` — комментарий.
pub fn parse_swayidle_config(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .flat_map(split_shell_words)
        .collect()
}

/// Файл из `-C <путь>`, если он задан.
pub fn swayidle_config_arg(args: &[String]) -> Option<&str> {
    args.iter().position(|a| a == "-C").and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// Правила swayidle: `timeout <с> <команда> [resume <команда>]`, событие
/// `lock <команда>` (им кончается `loginctl lock-session`); прочие события
/// (`before-sleep`, `after-resume`, `unlock`, `idlehint`) и ключи (`-w`,
/// `-d`, `-C`, `-S`) таймеров простоя не дают.
pub fn parse_swayidle(args: &[String]) -> (Vec<IdleRule>, Option<String>) {
    let mut rules = Vec::new();
    let mut lock_cmd = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "timeout" => {
                let t = args.get(i + 1).and_then(|v| v.parse::<u32>().ok());
                let cmd = args.get(i + 2).cloned();
                if let (Some(timeout_s), Some(command)) = (t, cmd) {
                    rules.push(IdleRule { timeout_s, command });
                }
                i += 3;
                if args.get(i).map(String::as_str) == Some("resume") {
                    i += 2;
                }
            }
            "lock" => {
                lock_cmd = args.get(i + 1).cloned();
                i += 2;
            }
            "before-sleep" | "after-resume" | "unlock" | "idlehint" | "-C" | "-S" => i += 2,
            _ => i += 1,
        }
    }
    (rules, lock_cmd)
}

/// Таймеры swayidle.
pub fn swayidle_timers(args: &[String]) -> DesktopTimers {
    let (rules, lock_cmd) = parse_swayidle(args);
    timers_from_rules(&rules, lock_cmd.as_deref())
}

/// Wayfire, плагин `idle` в `wayfire.ini`: `dpms_timeout` и
/// `screensaver_timeout` (своя «заставка»-куб) — секунды, −1 — выключено.
pub fn wayfire_timers(ini: &str) -> DesktopTimers {
    let mut t = DesktopTimers::default();
    for k in ["dpms_timeout", "screensaver_timeout"] {
        if let Some(s) = parse_int(crate::shell_palette::ini_get(ini, "idle", k)).and_then(positive) {
            t.note(Stage::Blank, s);
        }
    }
    t
}

// ── общий вход ──────────────────────────────────────────────────────────────

/// Таймеры текущего окружения по его конфигам. swayidle сюда не входит: его
/// аргументы знает только процесс (слушатель берёт их из строки процесса и
/// зовёт [`swayidle_timers`]). `None` — окружение ведёт простой само
/// неизвестным способом (COSMIC, LXQt, niri без swayidle…).
pub fn read_timers(io: &dyn LookIo, ctx: &LookCtx, on_battery: bool) -> Option<DesktopTimers> {
    let gs = gsettings_reader(io);
    Some(match ctx.de.as_str() {
        "kde" => kde_timers(
            io.read(&ctx.cfg("kscreenlockerrc")).as_deref(),
            io.read(&ctx.cfg("powerdevilrc")).as_deref(),
            PowerProfile::from_battery(on_battery),
            false,
        ),
        "gnome" => gnome_timers(&gs),
        "cinnamon-mint" => cinnamon_timers(&gs, on_battery),
        "mate" => mate_timers(&gs, on_battery),
        "budgie" => budgie_timers(&gs, ctx.wayland),
        "xfce" => {
            let xf = xfconf_reader(io);
            xfce_timers(&xf, io.have("xfce4-screensaver"), io.have("xfce4-power-manager"), on_battery)
        }
        "hyprland" => hypridle_timers(&io.read(&ctx.cfg("hypr/hypridle.conf"))?),
        "wayfire" => wayfire_timers(&io.read(&ctx.cfg("wayfire.ini"))?),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wl(compositor: &str, globals: &[&str]) -> WaylandFacts {
        WaylandFacts {
            compositor: compositor.into(),
            globals: globals.iter().map(|g| g.to_string()).collect(),
        }
    }

    fn full_wl(compositor: &str) -> WaylandFacts {
        wl(compositor, &["wl_compositor", "wl_shm", "zwlr_layer_shell_v1", "ext_idle_notifier_v1"])
    }

    fn facts<'a>(de: &'a str, session: SessionKind, wayland: Option<&'a WaylandFacts>) -> RouteFacts<'a> {
        RouteFacts {
            de: Some(de),
            session,
            wayland,
            gamescope: false,
            savers: NativeSavers::default(),
            shell_extension_enabled: false,
        }
    }

    const OVERLAY: Route = Route::Own { idle: IdleSource::ExtIdleNotify, show: ShowMethod::LayerShellOverlay };
    const X11_OWN: Route = Route::Own { idle: IdleSource::XScreenSaverExt, show: ShowMethod::X11OverrideRedirect };

    #[test]
    fn session_kind_follows_env() {
        assert_eq!(session_kind("wayland", "", ":0"), SessionKind::Wayland);
        assert_eq!(session_kind("x11", "wayland-0", ":0"), SessionKind::Wayland);
        assert_eq!(session_kind("x11", "", ":0"), SessionKind::X11);
        assert_eq!(session_kind("tty", "", ""), SessionKind::None);
    }

    #[test]
    fn wayland_compositors_with_layer_shell_get_overlay() {
        for (de, comp) in [
            ("kde", "kwin_wayland"),
            ("sway", "sway"),
            ("hyprland", "Hyprland"),
            ("niri", "niri"),
            ("river", "river"),
            ("wayfire", "wayfire"),
            ("labwc", "labwc"),
            ("cosmic", "cosmic-comp"),
            ("budgie", "labwc"),
            ("lxqt", "labwc"),
            ("deepin", "treeland"),
            ("xfce", "labwc"),
        ] {
            let w = full_wl(comp);
            assert_eq!(route(&facts(de, SessionKind::Wayland, Some(&w))), OVERLAY, "{de}/{comp}");
        }
    }

    #[test]
    fn missing_globals_are_named() {
        let w = wl("weston", &["wl_compositor", "wl_shm"]);
        assert_eq!(
            route(&facts("weston", SessionKind::Wayland, Some(&w))),
            Route::Unsupported(Unsupported::MissingGlobals(vec!["zwlr_layer_shell_v1", "ext_idle_notifier_v1"]))
        );
        let w = wl("x", &["zwlr_layer_shell_v1"]);
        assert_eq!(
            route(&facts("x", SessionKind::Wayland, Some(&w))),
            Route::Unsupported(Unsupported::MissingGlobals(vec!["ext_idle_notifier_v1"]))
        );
        assert_eq!(
            route(&facts("sway", SessionKind::Wayland, None)),
            Route::Unsupported(Unsupported::NoCompositorFacts)
        );
    }

    #[test]
    fn gnome_and_cinnamon_wayland_need_our_extension() {
        let w = wl("gnome-shell", &["wl_compositor"]);
        let mut f = facts("gnome", SessionKind::Wayland, Some(&w));
        assert_eq!(route(&f), Route::Unsupported(Unsupported::ShellExtensionOff(ShellExtension::Gnome)));
        f.shell_extension_enabled = true;
        assert_eq!(
            route(&f),
            Route::Own {
                idle: IdleSource::MutterIdleMonitor,
                show: ShowMethod::ShellExtension(ShellExtension::Gnome)
            }
        );
        let mut f = facts("cinnamon-mint", SessionKind::Wayland, None);
        assert_eq!(route(&f), Route::Unsupported(Unsupported::ShellExtensionOff(ShellExtension::Cinnamon)));
        f.shell_extension_enabled = true;
        assert_eq!(
            route(&f),
            Route::Own {
                idle: IdleSource::MuffinIdleMonitor,
                show: ShowMethod::ShellExtension(ShellExtension::Cinnamon)
            }
        );
    }

    #[test]
    fn pantheon_wayland_and_gamescope() {
        let w = full_wl("gala");
        assert_eq!(
            route(&facts("pantheon", SessionKind::Wayland, Some(&w))),
            Route::Unsupported(Unsupported::PantheonWayland)
        );
        let g = wl("gamescope", &["wl_compositor"]);
        assert_eq!(route(&facts("kde", SessionKind::Wayland, Some(&g))), Route::SteamOnly);
        let mut f = facts("kde", SessionKind::Wayland, None);
        f.gamescope = true;
        assert_eq!(route(&f), Route::SteamOnly);
    }

    #[test]
    fn x11_prefers_living_native_saver() {
        let mut f = facts("mate", SessionKind::X11, None);
        assert_eq!(route(&f), X11_OWN, "хранителя нет — свой показ");
        f.savers.mate = true;
        assert_eq!(route(&f), Route::Native(NativeModule::Mate));
        let mut f = facts("xfce", SessionKind::X11, None);
        f.savers.xfce = true;
        assert_eq!(route(&f), Route::Native(NativeModule::Xfce));
        let mut f = facts("lxqt", SessionKind::X11, None);
        f.savers.xscreensaver = true;
        assert_eq!(route(&f), Route::Native(NativeModule::XScreenSaver));
        let mut f = facts("deepin", SessionKind::X11, None);
        f.savers.deepin = true;
        assert_eq!(route(&f), Route::Native(NativeModule::Deepin));
        for de in ["kde", "gnome", "cinnamon-mint", "budgie", "pantheon"] {
            assert_eq!(route(&facts(de, SessionKind::X11, None)), X11_OWN, "{de}");
        }
        assert_eq!(route(&facts("kde", SessionKind::None, None)), Route::Unsupported(Unsupported::NoDisplay));
    }

    #[test]
    fn native_savers_from_bus_use_bridge_table() {
        assert_eq!(NativeModule::Mate.bus_name(), Some("org.mate.ScreenSaver"));
        assert_eq!(NativeModule::Xfce.bus_name(), Some("org.xfce.ScreenSaver"));
        let s = NativeSavers::from_bus(|n| n == "org.mate.ScreenSaver", false, true);
        assert_eq!(s, NativeSavers { mate: true, xfce: false, deepin: false, xscreensaver: true });
        // У cinnamon-screensaver и gnome-shell хаков нет — штатным модулем они не становятся.
        let s = NativeSavers::from_bus(|n| n == "org.cinnamon.ScreenSaver" || n == "org.gnome.ScreenSaver", false, false);
        assert_eq!(s, NativeSavers::default());
    }

    /// Все маршруты, какие умеет [`route`], — для стражей.
    fn every_route() -> Vec<Route> {
        let mut out = vec![Route::SteamOnly, Route::Unsupported(Unsupported::NoDisplay)];
        out.extend(NativeModule::all().iter().map(|m| Route::Native(*m)));
        for idle in [
            IdleSource::ExtIdleNotify,
            IdleSource::MutterIdleMonitor,
            IdleSource::MuffinIdleMonitor,
            IdleSource::XScreenSaverExt,
        ] {
            for show in [
                ShowMethod::LayerShellOverlay,
                ShowMethod::X11OverrideRedirect,
                ShowMethod::ShellExtension(ShellExtension::Gnome),
                ShowMethod::ShellExtension(ShellExtension::Cinnamon),
            ] {
                out.push(Route::Own { idle, show });
            }
        }
        out
    }

    /// ⛔ Страж (раздел 0 п. 3 и раздел 5 п. 1 плана): ни на одном пути
    /// заставка не захватывает ввод и не держит экран.
    #[test]
    fn guard_no_grab_and_no_screen_hold_anywhere() {
        for r in every_route() {
            if let Some(p) = show_policy(&r) {
                assert!(!p.grab_keyboard && !p.grab_pointer, "захват ввода на {r:?}");
                assert!(!p.hold_screen, "заставка держит экран на {r:?}");
                assert_eq!(p.first_frame_timeout_ms, FIRST_FRAME_TIMEOUT_MS);
                assert!(p.max_show_s > 0);
            }
        }
    }

    /// ⛔ Страж X11: на каждом X11-пути, до которого доходит [`route`], окно
    /// поднимается не более одного раза и только при неактивном хранителе,
    /// ввод — сырыми событиями без захвата.
    #[test]
    fn guard_x11_never_raises_over_lock() {
        let mut x11_routes = Vec::new();
        for de in ["kde", "gnome", "cinnamon-mint", "mate", "xfce", "budgie", "pantheon", "lxqt", "deepin"] {
            for savers in [
                NativeSavers::default(),
                NativeSavers { mate: true, ..Default::default() },
                NativeSavers { xfce: true, ..Default::default() },
                NativeSavers { deepin: true, ..Default::default() },
                NativeSavers { xscreensaver: true, ..Default::default() },
            ] {
                let mut f = facts(de, SessionKind::X11, None);
                f.savers = savers;
                x11_routes.push(route(&f));
            }
        }
        for r in &x11_routes {
            let p = show_policy(r).expect("на X11 показ есть всегда");
            assert!(!p.grab_keyboard && !p.grab_pointer, "{r:?}");
            match r {
                Route::Native(_) => {
                    assert_eq!(p.raise, RaisePolicy::Never, "в чужом окне не поднимаемся: {r:?}");
                    assert_eq!(p.input, InputWatch::Owner);
                }
                Route::Own { idle, show } => {
                    assert_eq!(*idle, IdleSource::XScreenSaverExt);
                    assert_eq!(*show, ShowMethod::X11OverrideRedirect);
                    assert_eq!(p.raise, RaisePolicy::OnceIfSaverInactive);
                    assert_eq!(p.input, InputWatch::XInput2Raw);
                }
                other => panic!("на X11 неожиданный путь {other:?}"),
            }
        }
        // На Wayland поднимать нечего: слой держит протокол.
        let w = full_wl("sway");
        let p = show_policy(&route(&facts("sway", SessionKind::Wayland, Some(&w)))).unwrap();
        assert_eq!(p.raise, RaisePolicy::Never);
        assert_eq!(p.input, InputWatch::LayerExclusiveKeyboard);
    }

    /// ⛔ Страж исходников будущих исполнителей показа и простоя в rexon-gui
    /// (`idle_screensaver*` / `screensaver_show*` в `src/` и `cpp/`): в них не
    /// должно появиться ни захвата ввода, ни удержания экрана. Каталоги
    /// обязаны существовать — иначе страж молча ничего не проверял бы.
    #[test]
    fn guard_gui_sources_have_no_grab_or_hold() {
        const FORBIDDEN: &[&str] = &[
            "XGrabKeyboard",
            "XGrabPointer",
            "XGrabServer",
            "xcb_grab_keyboard",
            "xcb_grab_pointer",
            "setKeyboardGrabEnabled",
            "setMouseGrabEnabled",
            "grabKeyboard",
            "grabMouse",
            "XResetScreenSaver",
            "SimulateUserActivity",
            "zwp_idle_inhibit",
        ];
        let gui = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../rexon-gui");
        let mut checked = 0;
        for sub in ["src", "cpp"] {
            let dir = gui.join(sub);
            let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !(name.starts_with("idle_screensaver") || name.starts_with("screensaver_show")) {
                    continue;
                }
                let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
                for bad in FORBIDDEN {
                    assert!(!text.contains(bad), "{name}: запрещено {bad}");
                }
                checked += 1;
            }
        }
        // Пока исполнителей нет, проверять нечего; число — для журнала.
        eprintln!("исходников исполнителей заставки проверено: {checked}");
    }

    #[test]
    fn lock_signals_and_inhibit_queries_come_from_bridge() {
        let s = lock_signal_services();
        assert_eq!(s[0], "org.freedesktop.ScreenSaver");
        for c in bridge::COMPANIONS {
            assert!(s.contains(&c.service));
        }
        assert_eq!(inhibit_query(&bridge::GNOME_SESSION), Some(("IsInhibited", Some(8))));
        assert_eq!(inhibit_query(&bridge::POWER_MANAGEMENT), Some(("HasInhibit", None)));
        assert_eq!(inhibit_query(&bridge::WAYLAND_IDLE), None);
        assert!(bridge::UPSTREAMS.iter().all(|u| inhibit_query(u).is_some()));
    }

    fn idle_moment(idle_s: u64) -> Moment {
        Moment { idle_ms: idle_s * 1000, session_active: true, ..Default::default() }
    }

    #[test]
    fn may_start_waits_for_threshold() {
        let s = Settings::default();
        assert_eq!(may_start(&idle_moment(179), &s), Err(Refusal::NotIdleYet));
        assert_eq!(may_start(&idle_moment(180), &s), Ok(()));
    }

    #[test]
    fn may_start_safety_beats_everything() {
        let s = Settings { show_over_fullscreen: true, idle_after_battery_s: Some(60), ..Default::default() };
        let mut m = idle_moment(10_000);
        m.locked = true;
        m.inhibits.session_idle = true;
        assert_eq!(may_start(&m, &s), Err(Refusal::Locked));
        assert_eq!(may_preview(&m), Err(Refusal::Locked));
        let mut m = idle_moment(10_000);
        m.preparing_sleep = true;
        assert_eq!(may_start(&m, &s), Err(Refusal::PreparingSleep));
        let mut m = idle_moment(10_000);
        m.session_active = false;
        assert_eq!(may_start(&m, &s), Err(Refusal::SessionInactive));
        assert_eq!(may_preview(&m), Err(Refusal::SessionInactive));
        let mut m = idle_moment(10_000);
        m.display_off = true;
        assert_eq!(may_start(&m, &s), Err(Refusal::DisplayOff));
    }

    #[test]
    fn may_start_respects_video_battery_and_games() {
        let s = Settings::default();
        for inh in [
            Inhibits { session_idle: true, ..Default::default() },
            Inhibits { power_management: true, ..Default::default() },
            Inhibits { bridge_holds: 2, ..Default::default() },
        ] {
            let mut m = idle_moment(10_000);
            m.inhibits = inh;
            assert_eq!(may_start(&m, &s), Err(Refusal::Inhibited), "{inh:?}");
            assert_eq!(may_preview(&m), Ok(()), "предпросмотр видео не держит");
        }
        let mut m = idle_moment(10_000);
        m.on_battery = true;
        assert_eq!(may_start(&m, &s), Err(Refusal::OnBattery), "на батарее по умолчанию выкл");
        let on_batt = Settings { idle_after_battery_s: Some(60), ..s };
        m.idle_ms = 59_000;
        assert_eq!(may_start(&m, &on_batt), Err(Refusal::NotIdleYet));
        m.idle_ms = 60_000;
        assert_eq!(may_start(&m, &on_batt), Ok(()));

        let mut m = idle_moment(10_000);
        m.fullscreen_focused = true;
        assert_eq!(may_start(&m, &s), Err(Refusal::Fullscreen));
        m.fullscreen_focused = false;
        m.steam_game_running = true;
        assert_eq!(may_start(&m, &s), Err(Refusal::SteamGame));
        let over = Settings { show_over_fullscreen: true, ..s };
        m.fullscreen_focused = true;
        assert_eq!(may_start(&m, &over), Ok(()));
    }

    #[test]
    fn must_stop_on_lock_input_and_sleep() {
        let s = Settings::default();
        for e in [
            Event::Input,
            Event::IdleResumed,
            Event::SaverActiveChanged(true),
            Event::LockedHint(true),
            Event::SessionLock,
            Event::PrepareForSleep(true),
            Event::DisplayPower(false),
            Event::SessionActive(false),
            Event::ListenerGone,
            Event::FirstFrameTimeout,
            Event::ShownTooLong,
        ] {
            assert!(must_stop(&e, &s, false), "{e:?}");
            assert!(must_stop(&e, &s, true), "предпросмотр тоже снимается: {e:?}");
        }
        for e in [
            Event::SaverActiveChanged(false),
            Event::LockedHint(false),
            Event::PrepareForSleep(false),
            Event::DisplayPower(true),
            Event::SessionActive(true),
            Event::OnBattery(false),
        ] {
            assert!(!must_stop(&e, &s, false), "{e:?}");
        }
    }

    #[test]
    fn must_stop_battery_and_inhibit_spare_preview() {
        let s = Settings::default();
        assert!(must_stop(&Event::OnBattery(true), &s, false));
        assert!(!must_stop(&Event::OnBattery(true), &s, true));
        let on_batt = Settings { idle_after_battery_s: Some(120), ..s };
        assert!(!must_stop(&Event::OnBattery(true), &on_batt, false));
        assert!(must_stop(&Event::InhibitAdded, &s, false));
        assert!(!must_stop(&Event::InhibitAdded, &s, true));
    }
}

#[cfg(test)]
mod timer_tests {
    use super::*;
    use std::collections::HashMap;

    /// Подделка gsettings/xfconf: `(схема, ключ) → значение`.
    fn fake(pairs: &[(&str, &str, &str)]) -> impl Fn(&str, &str) -> Option<String> {
        let map: HashMap<(String, String), String> =
            pairs.iter().map(|(s, k, v)| ((s.to_string(), k.to_string()), v.to_string())).collect();
        move |s, k| map.get(&(s.to_string(), k.to_string())).cloned()
    }

    fn t(dim: Option<u32>, blank: Option<u32>, lock: Option<u32>) -> DesktopTimers {
        DesktopTimers { dim_s: dim, blank_s: blank, lock_s: lock }
    }

    #[test]
    fn first_and_order_problem() {
        let x = t(Some(150), Some(300), Some(300));
        assert_eq!(x.first(), Some((Stage::Dim, 150)));
        assert_eq!(order_problem(120, &x), None);
        assert_eq!(order_problem(150, &x), Some((Stage::Dim, 150)));
        let y = t(None, Some(300), Some(300));
        assert_eq!(y.first(), Some((Stage::Lock, 300)), "при равенстве — блокировка");
        assert_eq!(order_problem(600, &DesktopTimers::default()), None);
    }

    #[test]
    fn suggested_delay_stays_before_first_stage() {
        assert_eq!(suggested_delay(&t(Some(150), Some(300), None)), 120);
        assert_eq!(suggested_delay(&t(None, None, Some(600))), 180, "не длиннее умолчания");
        assert_eq!(suggested_delay(&t(Some(30), None, None)), 60, "не короче минуты");
        assert_eq!(suggested_delay(&DesktopTimers::default()), 180);
        let d = suggested_delay(&t(None, Some(200), None));
        assert!(d < 200 && order_problem(d, &t(None, Some(200), None)).is_none());
    }

    #[test]
    fn kde_defaults_without_files() {
        assert_eq!(kde_timers(None, None, PowerProfile::Ac, false), t(Some(300), Some(600), Some(300)));
        assert_eq!(kde_timers(None, None, PowerProfile::Battery, false), t(Some(120), Some(300), Some(300)));
        // На «мобильном» блокировка — не позже гашения.
        assert_eq!(kde_timers(None, None, PowerProfile::LowBattery, true), t(Some(30), Some(60), Some(60)));
    }

    #[test]
    fn kde_reads_kscreenlocker_and_powerdevil() {
        let ksl = "[Daemon]\nAutolock=true\nTimeout=2.5\nLockGrace=5\n";
        let pd = "[AC][Display]\nDimDisplayIdleTimeoutSec=90\nTurnOffDisplayIdleTimeoutSec=-1\n\
                  [Battery][Display]\nDimDisplayWhenIdle=false\nLockBeforeTurnOffDisplay=true\n";
        assert_eq!(kde_timers(Some(ksl), Some(pd), PowerProfile::Ac, false), t(Some(90), None, Some(150)));
        assert_eq!(kde_timers(Some(ksl), Some(pd), PowerProfile::Battery, false), t(None, Some(300), Some(150)));
        let off = "[Daemon]\nAutolock=false\n";
        let pd2 = "[AC][Display]\nTurnOffDisplayWhenIdle=false\nDimDisplayWhenIdle=false\n";
        assert_eq!(kde_timers(Some(off), Some(pd2), PowerProfile::Ac, false), DesktopTimers::default());
    }

    #[test]
    fn gnome_defaults_and_values() {
        let none = fake(&[]);
        assert_eq!(gnome_timers(&none), t(Some(150), Some(300), Some(300)));
        let g = fake(&[
            ("org.gnome.desktop.session", "idle-delay", "600"),
            ("org.gnome.desktop.screensaver", "lock-delay", "30"),
        ]);
        assert_eq!(gnome_timers(&g), t(Some(300), Some(600), Some(630)));
        let never = fake(&[("org.gnome.desktop.session", "idle-delay", "0")]);
        assert_eq!(gnome_timers(&never), t(Some(60), None, None), "без гашения gsd затемняет через минуту");
        let short = fake(&[
            ("org.gnome.desktop.session", "idle-delay", "15"),
            ("org.gnome.desktop.lockdown", "disable-lock-screen", "true"),
        ]);
        assert_eq!(gnome_timers(&short), t(None, Some(15), None));
        let nodim = fake(&[
            ("org.gnome.settings-daemon.plugins.power", "idle-dim", "false"),
            ("org.gnome.desktop.screensaver", "lock-enabled", "false"),
        ]);
        assert_eq!(gnome_timers(&nodim), t(None, Some(300), None));
    }

    #[test]
    fn cinnamon_timers_by_power_source() {
        let none = fake(&[]);
        assert_eq!(cinnamon_timers(&none, false), t(None, Some(900), Some(900)));
        assert_eq!(cinnamon_timers(&none, true), t(Some(90), Some(900), Some(900)));
        let c = fake(&[
            ("org.cinnamon.desktop.session", "idle-delay", "300"),
            ("org.cinnamon.desktop.screensaver", "lock-delay", "60"),
            ("org.cinnamon.settings-daemon.plugins.power", "sleep-display-ac", "240"),
            ("org.cinnamon.settings-daemon.plugins.power", "idle-dim-ac", "true"),
            ("org.cinnamon.settings-daemon.plugins.power", "idle-dim-time", "120"),
        ]);
        assert_eq!(cinnamon_timers(&c, false), t(Some(120), Some(240), Some(360)));
        let off = fake(&[
            ("org.cinnamon.desktop.screensaver", "idle-activation-enabled", "false"),
            ("org.cinnamon.settings-daemon.plugins.power", "sleep-display-ac", "0"),
        ]);
        assert_eq!(cinnamon_timers(&off, false), DesktopTimers::default());
    }

    #[test]
    fn mate_counts_minutes() {
        let none = fake(&[]);
        assert_eq!(mate_timers(&none, false), t(None, Some(300), Some(300)));
        assert_eq!(mate_timers(&none, true), t(Some(10), Some(300), Some(300)));
        let m = fake(&[
            ("org.mate.session", "idle-delay", "10"),
            ("org.mate.screensaver", "lock-delay", "2"),
            ("org.mate.power-manager", "sleep-display-ac", "0"),
        ]);
        assert_eq!(mate_timers(&m, false), t(None, Some(600), Some(720)));
        let locked_down = fake(&[("org.mate.lockdown", "disable-lock-screen", "true")]);
        assert_eq!(mate_timers(&locked_down, false).lock_s, None);
    }

    #[test]
    fn budgie_wayland_matches_its_swayidle_line() {
        let b = fake(&[
            ("org.gnome.desktop.session", "idle-delay", "600"),
            ("org.gnome.desktop.screensaver", "lock-delay", "30"),
            ("com.solus-project.budgie-wm", "dimming-delay", "10"),
        ]);
        let from_gsettings = budgie_timers(&b, true);
        assert_eq!(from_gsettings, t(Some(30), Some(600), Some(630)), "затемнение сжато до 30 с");
        // Та же строка, которую budgie-daemon собирает для swayidle (образец
        // составлен вручную по описанию его логики).
        let line = "swayidle -w timeout 30 'dbus-send --type=method_call --dest=org.buddiesofbudgie.BudgieScreenlock \
                    /org/buddiesofbudgie/Screenlock org.buddiesofbudgie.BudgieScreenlock.Dim' resume 'dbus-send x.Undim' \
                    timeout 600 'dbus-send a.ScreenPowerSuspend && wlopm --off \\*' resume 'wlopm --on \\*' \
                    timeout 630 'gtklock -d' before-sleep 'gtklock -d'";
        let args = swayidle_args_in_line(line).unwrap();
        assert_eq!(swayidle_timers(&args), from_gsettings);
        assert_eq!(budgie_timers(&fake(&[]), false), gnome_timers(&fake(&[])), "X11 — как GNOME");
        let nodim = fake(&[]);
        assert_eq!(budgie_timers(&nodim, true).dim_s, None, "dimming-delay не задан — затемнения нет");
    }

    #[test]
    fn xfce_screensaver_and_power_manager() {
        let none = fake(&[]);
        assert_eq!(xfce_timers(&none, true, true, false), t(None, Some(300), Some(300)));
        assert_eq!(xfce_timers(&none, true, true, true), t(Some(300), Some(300), Some(300)));
        assert_eq!(xfce_timers(&none, false, false, false), DesktopTimers::default());
        let x = fake(&[
            ("xfce4-screensaver", "/saver/idle-activation/delay", "15"),
            ("xfce4-screensaver", "/lock/saver-activation/delay", "1"),
            ("xfce4-power-manager", "/xfce4-power-manager/dpms-on-ac-sleep", "0"),
            ("xfce4-power-manager", "/xfce4-power-manager/dpms-on-ac-off", "20"),
            ("xfce4-power-manager", "/xfce4-power-manager/brightness-on-ac", "120"),
        ]);
        assert_eq!(xfce_timers(&x, true, true, false), t(Some(120), Some(900), Some(960)));
        let nolock = fake(&[
            ("xfce4-screensaver", "/lock/saver-activation/enabled", "false"),
            ("xfce4-power-manager", "/xfce4-power-manager/dpms-enabled", "false"),
        ]);
        assert_eq!(xfce_timers(&nolock, true, true, false), t(None, Some(300), None));
    }

    #[test]
    fn hypridle_listeners_are_classified() {
        let conf = "# пример\n$lock = pidof hyprlock || hyprlock\n\
            general {\n    lock_cmd = $lock\n    before_sleep_cmd = loginctl lock-session\n}\n\
            listener {\n    timeout = 150 # полторы минуты\n    on-timeout = brightnessctl -s set 10\n    on-resume = brightnessctl -r\n}\n\
            listener {\n    timeout = 300\n    on-timeout = loginctl lock-session\n}\n\
            listener {\n    timeout = 330\n    on-timeout = hyprctl dispatch dpms off\n    on-resume = hyprctl dispatch dpms on\n}\n\
            listener {\n    timeout = 1800\n    on-timeout = systemctl suspend\n}\n\
            listener {\n    timeout = 60\n    on-timeout = notify-send 'скоро'\n}\n";
        let (rules, lock) = parse_hypridle(conf);
        assert_eq!(rules.len(), 5);
        assert_eq!(lock.as_deref(), Some("pidof hyprlock || hyprlock"));
        assert_eq!(hypridle_timers(conf), t(Some(150), Some(330), Some(300)));
        assert_eq!(hypridle_timers(""), DesktopTimers::default());
    }

    #[test]
    fn swayidle_from_args_config_and_exec_line() {
        let line = r#"exec swayidle -w timeout 300 'swaylock -f -c 000000' timeout 600 'swaymsg "output * power off"' resume 'swaymsg "output * power on"' before-sleep 'swaylock -f'"#;
        let args = swayidle_args_in_line(line).unwrap();
        assert_eq!(args[0], "-w");
        assert_eq!(args[6], r#"swaymsg "output * power off""#);
        assert_eq!(swayidle_timers(&args), t(None, Some(600), Some(300)));
        assert_eq!(swayidle_args_in_line("exec mako"), None);
        assert!(swayidle_args_in_line("/usr/bin/swayidle").unwrap().is_empty());

        let cfg = "# конфиг\ntimeout 240 'loginctl lock-session'\nlock 'swaylock -f'\ntimeout 120 'brightnessctl set 20%' resume 'brightnessctl -r'\n";
        let args = parse_swayidle_config(cfg);
        assert_eq!(swayidle_timers(&args), t(Some(120), None, Some(240)));
        let with_c: Vec<String> = ["-w", "-C", "/x/cfg"].iter().map(|s| s.to_string()).collect();
        assert_eq!(swayidle_config_arg(&with_c), Some("/x/cfg"));
        assert_eq!(swayidle_timers(&with_c), DesktopTimers::default());
    }

    #[test]
    fn shell_words_follow_sh_rules() {
        assert_eq!(split_shell_words(r#"a 'b c' "d \"e\"" f\ g ''"#), vec!["a", "b c", "d \"e\"", "f g", ""]);
        assert_eq!(split_shell_words(r#""x\y""#), vec!["x\\y"]);
        assert!(split_shell_words("   ").is_empty());
    }

    #[test]
    fn classify_known_commands() {
        assert_eq!(classify_idle_command("loginctl lock-session", None), Some(Stage::Lock));
        assert_eq!(classify_idle_command("my-locker", Some("my-locker")), Some(Stage::Lock));
        assert_eq!(classify_idle_command("niri msg action power-off-monitors", None), Some(Stage::Blank));
        assert_eq!(classify_idle_command("systemctl suspend-then-hibernate", None), Some(Stage::Blank));
        assert_eq!(classify_idle_command("light -S 10", None), Some(Stage::Dim));
        assert_eq!(classify_idle_command("notify-send hi", None), None);
        assert_eq!(classify_idle_command("", Some("")), None);
    }

    #[test]
    fn wayfire_idle_plugin() {
        let ini = "[idle]\ndpms_timeout = 600\nscreensaver_timeout = -1\n[core]\nplugins = idle\n";
        assert_eq!(wayfire_timers(ini), t(None, Some(600), None));
        assert_eq!(wayfire_timers("[core]\n"), DesktopTimers::default());
    }

    /// Обёртки ходят в gsettings и xfconf тем же способом, что остальной
    /// Rexon, — проверка на подделке [`LookIo`] (ничего не запускается).
    struct FakeIo {
        files: HashMap<String, String>,
        tools: Vec<&'static str>,
    }

    impl LookIo for FakeIo {
        fn run(&self, argv: &[String]) -> (bool, String) {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["gsettings", "get", "org.mate.session", "idle-delay"] => (true, "10\n".into()),
                ["gsettings", "get", "org.gnome.desktop.session", "idle-delay"] => (true, "uint32 120\n".into()),
                ["xfconf-query", "-c", "xfce4-screensaver", "-p", "/saver/idle-activation/delay"] => (true, "3\n".into()),
                _ => (false, "нет ключа".into()),
            }
        }
        fn have(&self, tool: &str) -> bool {
            self.tools.contains(&tool)
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.get(path).cloned()
        }
        fn write(&self, _path: &str, _text: &str) -> bool {
            false
        }
        fn list_dir(&self, _path: &str) -> Vec<String> {
            Vec::new()
        }
    }

    #[test]
    fn read_timers_dispatches_by_desktop() {
        let io = FakeIo { files: HashMap::new(), tools: vec!["gsettings", "xfconf-query", "xfce4-screensaver"] };
        let ctx = |de: &str| LookCtx::new(de, "/home/u", None);
        assert_eq!(read_timers(&io, &ctx("mate"), false), Some(t(None, Some(600), Some(600))));
        assert_eq!(read_timers(&io, &ctx("gnome"), false), Some(t(Some(60), Some(120), Some(120))));
        assert_eq!(read_timers(&io, &ctx("xfce"), false), Some(t(None, Some(180), Some(180))));
        assert_eq!(read_timers(&io, &ctx("cosmic"), false), None);
        assert_eq!(read_timers(&io, &ctx("hyprland"), false), None, "нет hypridle.conf");

        let mut files = HashMap::new();
        files.insert("/home/u/.config/kscreenlockerrc".to_string(), "[Daemon]\nTimeout=10\n".to_string());
        files.insert(
            "/home/u/.config/hypr/hypridle.conf".to_string(),
            "listener {\ntimeout = 200\non-timeout = hyprlock\n}\n".to_string(),
        );
        let io = FakeIo { files, tools: vec![] };
        assert_eq!(read_timers(&io, &ctx("kde"), false), Some(t(Some(300), Some(600), Some(600))));
        assert_eq!(read_timers(&io, &ctx("hyprland"), false), Some(t(None, None, Some(200))));
        // Без gsettings в PATH — умолчания схем, а не паника.
        assert_eq!(read_timers(&io, &ctx("gnome"), false), Some(gnome_timers(&fake(&[]))));
    }
}
