//! Мост «не гасить экран»: `org.freedesktop.ScreenSaver` там, где его нет.
//!
//! ЗАЧЕМ. Браузеры и проигрыватели просят «не гасить экран, идёт видео» через
//! службу `org.freedesktop.ScreenSaver` (`Inhibit(приложение, причина)` →
//! куки, `UnInhibit(куки)`). В Plasma эту службу даёт сам сеанс, а в MATE её
//! нет вовсе: хранитель экрана там отзывается только на `org.mate.ScreenSaver`,
//! вызов уходит в пустоту (`ServiceUnknown`), и экран гаснет посреди фильма.
//! Замер на живой машине 16.10.2026 (MATE 1.28, X11, Firefox 157): на шине
//! владельца имени нет; портал `org.freedesktop.portal.Inhibit` до
//! `mate-session` не доходит (`IsInhibited(8)` остаётся `false`); прямой
//! `org.gnome.SessionManager.Inhibit(…, 8)` от живого клиента —
//! `IsInhibited(8)` = `true`.
//!
//! ЧТО ДЕЛАЕТ. Rexon живёт весь сеанс (значок в лотке), поэтому имя занимает
//! он: если в сеансе у `org.freedesktop.ScreenSaver` нет хозяина, Rexon ставит
//! объект на оба пути, как у Plasma (`/org/freedesktop/ScreenSaver` и
//! `/ScreenSaver`), и каждое удержание пробрасывает в то, что в этой среде
//! действительно держит экран: у MATE это `org.gnome.SessionManager.Inhibit`
//! с флагом 8 («простой»; его держит mate-session), у XFCE —
//! `org.freedesktop.PowerManagement.Inhibit` (xfce4-power-manager). Там, где
//! экран гасит служба, слушающая только протокол Wayland (swayidle у sway и
//! Budgie, KIdleTime у LXQt — оба через `ext-idle-notify-v1`), по шине
//! удержать нечем: мост держит запрет простоя протоколом Wayland
//! `idle-inhibit-unstable-v1` на своей невидимой слой-поверхности
//! ([`WAYLAND_IDLE`]) — но только у композиторов, про которых по исходникам
//! известно, что такую поверхность они учитывают ([`WAYLAND_IDLE_COMPOSITORS`]).
//! У прочих имя мост НЕ занимает: нативный Firefox, не получив ответа по
//! шине, сам доходит до Wayland idle-inhibit на своём окне, а занятое нами
//! имя остановило бы его на нас. Удержание снимается по `UnInhibit` и само —
//! когда клиент исчезает с шины (закрыли браузер, он упал): иначе экран не
//! гас бы уже никогда.
//!
//! ⚠️ Удержание у менеджера сеанса числится за ПОДКЛЮЧЕНИЕМ Rexon к шине и
//! снимается само, когда это подключение закрывается. Поэтому пробрасывать
//! можно только из долгоживущего подключения процесса — не из `gdbus` и не
//! из другого короткого процесса: тот выйдет, и удержание исчезнет вместе с
//! ним.
//!
//! ГРАНИЦА. Здесь только решения: нужен ли мост в этом окружении, во что
//! пробрасывать, учёт куки и что ответить. Шина — на исполнителе
//! (`rexon-gui/src/screensaver_bridge.rs` поверх шима
//! `cpp/rexon_screensaver_bridge.cpp`). Всё ниже проверяется обычными тестами
//! ядра, без шины.
//!
//! Описание интерфейса — по спецификации freedesktop (черновик
//! idle-inhibition, <https://specifications.freedesktop.org/idle-inhibit-spec/latest/>)
//! и по описанию интерфейса KDE, которое ставит kscreenlocker
//! (`/usr/share/dbus-1/interfaces/kf6_org.freedesktop.ScreenSaver.xml`).
//! Реализуем то, что можем исполнить честно: `Inhibit`, `UnInhibit`,
//! `GetActive`, `SetActive`, `GetActiveTime`, `SimulateUserActivity`, `Lock`.
//! `Throttle`/`GetSessionIdleTime` и сигнал `ActiveChanged` не объявлены:
//! исполнить их нечем, а объявить и молчать — хуже, чем не объявить.

use std::collections::BTreeMap;
use std::time::Duration;

/// Имя службы на шине сеанса.
pub const SERVICE: &str = "org.freedesktop.ScreenSaver";
/// Имя интерфейса.
pub const INTERFACE: &str = "org.freedesktop.ScreenSaver";
/// Пути объекта: оба, как у Plasma. Клиенты зовут то один, то другой
/// (Firefox и Chromium — `/ScreenSaver`, часть проигрывателей —
/// `/org/freedesktop/ScreenSaver`).
pub const OBJECT_PATHS: [&str; 2] = ["/org/freedesktop/ScreenSaver", "/ScreenSaver"];

/// Сколько ждать после запуска Rexon, прежде чем смотреть, свободно ли имя.
///
/// Rexon поднимается автозапуском при входе — вместе со службами сеанса.
/// Настоящий хозяин имени (в средах, где он есть) мог ещё не встать, и
/// занять имя раньше него значило бы отнять его у сеанса. Окружения, где
/// хозяин известен, мост и так обходит ([`desktop_policy`]); пауза — защита
/// для тех, кого в таблице нет, и для MATE с хранителем экрана, который имя
/// даёт сам (с 1.28.1): он берёт имя без очереди и без замены, и занятое
/// раньше него имя ему уже не достанется.
pub const START_DELAY: Duration = Duration::from_secs(30);

/// С этой версии mate-screensaver сам держит `org.freedesktop.ScreenSaver`
/// на обоих путях (выпуск 1.28.1 от 18.05.2026) — мост в MATE не нужен.
pub const MATE_SCREENSAVER_FDO_SINCE: (u32, u32, u32) = (1, 28, 1);

/// Команда, которой узнаём версию mate-screensaver. Клиент, а не сам
/// хранитель: тот на `--version` поднимает GTK и лезет к шине доступности.
/// Пакет у них один, версия одна.
pub const MATE_SCREENSAVER_VERSION_ARGV: [&str; 2] = ["mate-screensaver-command", "--version"];

/// Версия из строки вида `mate-screensaver-command 1.28.0`.
pub fn parse_version(output: &str) -> Option<(u32, u32, u32)> {
    output.split_whitespace().rev().find_map(|word| {
        let mut parts = word.split('.').map(|p| p.parse::<u32>());
        let major = parts.next()?.ok()?;
        let minor = parts.next()?.ok()?;
        let patch = match parts.next() {
            Some(p) => p.ok()?,
            None => 0,
        };
        Some((major, minor, patch))
    })
}

/// Нужно ли исполнителю узнать версию mate-screensaver для [`decide`].
pub fn wants_mate_screensaver_version(de: Option<&str>) -> bool {
    de == Some("mate")
}

/// Флаг «простой» у `org.gnome.SessionManager.Inhibit`: не считать сеанс
/// бездействующим — не включать хранитель экрана и не гасить экран.
pub const GSM_INHIBIT_IDLE: u32 = 8;

/// Ошибки D-Bus, которыми мост отвечает.
pub const ERROR_UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";
pub const ERROR_INVALID_ARGS: &str = "org.freedesktop.DBus.Error.InvalidArgs";
pub const ERROR_NOT_SUPPORTED: &str = "org.freedesktop.DBus.Error.NotSupported";
pub const ERROR_FAILED: &str = "org.freedesktop.DBus.Error.Failed";

/// Описание интерфейса для `Introspect` (только `<interface>`: узел и
/// стандартные интерфейсы добавляет QtDBus).
pub const INTROSPECTION_XML: &str = r#"<interface name="org.freedesktop.ScreenSaver">
  <method name="Inhibit">
    <arg name="application_name" type="s" direction="in"/>
    <arg name="reason_for_inhibit" type="s" direction="in"/>
    <arg name="cookie" type="u" direction="out"/>
  </method>
  <method name="UnInhibit">
    <arg name="cookie" type="u" direction="in"/>
  </method>
  <method name="GetActive">
    <arg type="b" direction="out"/>
  </method>
  <method name="SetActive">
    <arg name="e" type="b" direction="in"/>
    <arg type="b" direction="out"/>
  </method>
  <method name="GetActiveTime">
    <arg name="seconds" type="u" direction="out"/>
  </method>
  <method name="SimulateUserActivity">
  </method>
  <method name="Lock">
  </method>
</interface>
"#;

/// Стандартный интерфейс описания объекта.
pub const INTROSPECTABLE: &str = "org.freedesktop.DBus.Introspectable";

/// Полный ответ на `Introspect`: узел с описанием и нашим интерфейсом.
pub fn introspection_node() -> String {
    format!(
        "<!DOCTYPE node PUBLIC \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\" \
         \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\n<node>\n\
         <interface name=\"{INTROSPECTABLE}\">\n  <method name=\"Introspect\">\n    \
         <arg name=\"xml_data\" type=\"s\" direction=\"out\"/>\n  </method>\n</interface>\n\
         {INTROSPECTION_XML}</node>\n"
    )
}

// ============================================================================
// Окружения: где мост нужен
// ============================================================================

/// Как в окружении обстоит дело с `org.freedesktop.ScreenSaver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopPolicy {
    /// Имя даёт само окружение (названо, кто). Мост не нужен и НЕ ставится,
    /// даже если в эту минуту имя свободно: его хозяин мог перезапускаться,
    /// и занятое нами имя он бы уже не получил.
    OwnProvider(&'static str),
    /// Своего хозяина нет, но и пробросить по шине нечем: экран там гасит
    /// служба, которая смотрит только на протокол Wayland idle-inhibit, а
    /// не на менеджер сеанса. Держать можно только Wayland-запретом
    /// ([`WAYLAND_IDLE`]); нечем — имя НЕ занимаем: клиент получил бы куки
    /// при гаснущем экране.
    NoBusTarget(&'static str),
    /// Своего хозяина у окружения нет — мост ставится, если есть во что
    /// пробрасывать.
    Bridge,
}

/// Окружения со своим хозяином имени. Ключ — идентификатор окружения ядра
/// ([`crate::system::detect_current_de`]).
///
/// Источники — в таблице отчёта к коммиту и в комментариях строк.
const OWN_PROVIDERS: &[(&str, &str)] = &[
    // Plasma: служба экрана блокировки (libKScreenLocker в ksmserver) и
    // PowerDevil — оба пути, `Inhibit` уходит в управление питанием.
    ("kde", "ksmserver (kscreenlocker) + PowerDevil"),
    // GNOME: gsd-screensaver-proxy из gnome-settings-daemon, юнит
    // `org.gnome.SettingsDaemon.ScreensaverProxy` — пробрасывает в
    // gnome-session с флагом «простой».
    ("gnome", "gsd-screensaver-proxy"),
    // Cinnamon: csd-screensaver-proxy из cinnamon-settings-daemon,
    // автозапуск с `OnlyShowIn=X-Cinnamon`, тот же проброс в cinnamon-session.
    ("cinnamon-mint", "csd-screensaver-proxy"),
    // COSMIC: cosmic-idle (запускает cosmic-session), оба пути, Inhibit и
    // UnInhibit снимают таймеры гашения экрана и сна.
    ("cosmic", "cosmic-idle"),
    // niri с 0.1.2 сам держит имя на обоих путях и переводит Inhibit в запрет
    // простоя (src/dbus/freedesktop_screensaver.rs) — в сеансовом экземпляре.
    // Под LXQt niri запущен без `--session`, имя не берёт, и окружение там
    // «lxqt», а не «niri».
    ("niri", "niri"),
];

/// Окружения, где по шине удержать экран нечем — только протоколом Wayland.
const NO_BUS_TARGET: &[(&str, &str)] = &[
    // Budgie 10.10 (labwc): экран гасит `swayidle -w`, запущенный
    // budgie-daemon (src/daemon/screenlock.vala); он слушает только Wayland.
    // budgie-session держит org.gnome.SessionManager, но флаг «простой» там
    // меняет лишь присутствие (Presence), а swayidle на него не смотрит.
    ("budgie", "swayidle (только Wayland idle-inhibit)"),
];

/// Решение по окружению. `de` — ответ [`crate::system::detect_current_de`].
pub fn desktop_policy(de: Option<&str>) -> DesktopPolicy {
    let Some(de) = de else {
        return DesktopPolicy::Bridge;
    };
    if let Some((_, who)) = OWN_PROVIDERS.iter().find(|(k, _)| *k == de) {
        return DesktopPolicy::OwnProvider(who);
    }
    if let Some((_, who)) = NO_BUS_TARGET.iter().find(|(k, _)| *k == de) {
        return DesktopPolicy::NoBusTarget(who);
    }
    DesktopPolicy::Bridge
}

// ============================================================================
// Куда пробрасывать
// ============================================================================

/// Служба, которая в этой среде действительно держит экран.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Upstream {
    pub service: &'static str,
    pub path: &'static str,
    pub iface: &'static str,
    pub kind: UpstreamKind,
}

/// Каким вызовом служба берёт и снимает удержание.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamKind {
    /// `Inhibit(s app_id, u toplevel_xid, s reason, u flags) → u`, `Uninhibit(u)`.
    GnomeSession,
    /// `Inhibit(s application, s reason) → u`, `UnInhibit(u)`.
    PowerManagement,
    /// Хранитель экрана: удержаний через него не берём.
    Companion,
    /// Не служба шины, а запрет простоя протоколом Wayland: один на все
    /// удержания, держит исполнитель ([`Action::HoldIdle`]).
    WaylandIdleInhibit,
}

/// Менеджер сеанса семейства gnome-session: mate-session, budgie-session,
/// gnome-session, cinnamon-session — все держат это имя.
/// `Inhibit(s app_id, u toplevel_xid, s reason, u flags) → u`, `Uninhibit(u)`.
pub const GNOME_SESSION: Upstream = Upstream {
    service: "org.gnome.SessionManager",
    path: "/org/gnome/SessionManager",
    iface: "org.gnome.SessionManager",
    kind: UpstreamKind::GnomeSession,
};

/// Управление питанием XFCE (xfce4-power-manager): `Inhibit` отключает и
/// DPMS, и хранитель экрана (через libxfce4ui зовёт org.xfce.ScreenSaver).
/// Голый `org.xfce.ScreenSaver.Inhibit` DPMS у xfpm не трогает, поэтому
/// пробрасываем сюда.
pub const POWER_MANAGEMENT: Upstream = Upstream {
    service: "org.freedesktop.PowerManagement",
    path: "/org/freedesktop/PowerManagement/Inhibit",
    iface: "org.freedesktop.PowerManagement.Inhibit",
    kind: UpstreamKind::PowerManagement,
};

/// Запрет простоя протоколом Wayland (`zwp_idle_inhibit_manager_v1`) на
/// невидимой слой-поверхности 1×1 из своего подключения Rexon к композитору.
/// Шины здесь нет: путь и интерфейс пусты, имя — для журнала.
pub const WAYLAND_IDLE: Upstream = Upstream {
    service: "zwp_idle_inhibit_manager_v1",
    path: "",
    iface: "",
    kind: UpstreamKind::WaylandIdleInhibit,
};

/// Глобалы композитора, без которых Wayland-запрет не поставить.
pub const WAYLAND_GLOBALS_NEEDED: [&str; 4] =
    ["wl_compositor", "wl_shm", "zwlr_layer_shell_v1", "zwp_idle_inhibit_manager_v1"];

/// Композиторы, которые запрет на невидимой слой-поверхности учитывают, —
/// по их исходникам. Ключ — имя процесса композитора (`/proc/PID/comm`
/// хозяина сокета Wayland), а не окружение: под LXQt и Budgie композиторы
/// разные, а `XDG_CURRENT_DESKTOP` у них один.
pub const WAYLAND_IDLE_COMPOSITORS: &[(&str, &str)] = &[
    // sway/desktop/idle_inhibit_v1.c: запрет слой-поверхности действует,
    // пока она отображена на включённом выходе («layer surfaces are always
    // on screen after they have been mapped»).
    ("sway", "sway"),
    // src/idle.c: любой живой запрет = простой запрещён, видимость не
    // проверяется. Композитор Budgie 10.10 и сеанса LXQt по умолчанию.
    ("labwc", "labwc"),
    // src/managers/input/IdleInhibitor.cpp: запрет действует, если вид
    // отображён и принимает ввод; у слой-поверхности ввод доступен всегда
    // (CLayerSurface::focusAvailable). Обычно имя там держит hypridle, и мост
    // не встаёт вовсе; это — на случай swayidle вместо него.
    ("Hyprland", "Hyprland"),
];

/// Что исполнитель узнал о сеансе Wayland: имя процесса композитора и
/// объявленные им глобалы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaylandFacts {
    pub compositor: String,
    pub globals: Vec<String>,
}

/// Можно ли здесь держать экран Wayland-запретом; `Err` — почему нет.
pub fn wayland_idle_check(facts: Option<&WaylandFacts>) -> Result<(), String> {
    let Some(f) = facts else {
        return Err("сеанс не Wayland или композитор не ответил".into());
    };
    if !WAYLAND_IDLE_COMPOSITORS.iter().any(|(comm, _)| *comm == f.compositor) {
        let name = if f.compositor.is_empty() { "?" } else { f.compositor.as_str() };
        return Err(format!(
            "композитор «{name}» не проверен: учитывает ли он запрет на невидимой поверхности, неизвестно"
        ));
    }
    let missing: Vec<&str> = WAYLAND_GLOBALS_NEEDED
        .iter()
        .copied()
        .filter(|g| !f.globals.iter().any(|have| have == g))
        .collect();
    if !missing.is_empty() {
        return Err(format!("у композитора {} нет {}", f.compositor, missing.join(", ")));
    }
    Ok(())
}

/// Кандидаты проброса в порядке предпочтения: менеджер сеанса держит и
/// хранитель экрана, и гашение (MATE: mate-screensaver смотрит Presence,
/// mate-power-manager — `IsInhibited(8)`), поэтому он первый.
pub const UPSTREAMS: &[Upstream] = &[GNOME_SESSION, POWER_MANAGEMENT];

/// Хранитель экрана окружения — у него спрашиваем `GetActive` и прочее.
/// У всех ниже интерфейс наследника gnome-screensaver 2.x: `GetActive() → b`,
/// `GetActiveTime() → u`, `SetActive(b)`, `SimulateUserActivity()`, `Lock()`.
pub const COMPANIONS: &[Upstream] = &[
    Upstream {
        service: "org.mate.ScreenSaver",
        path: "/org/mate/ScreenSaver",
        iface: "org.mate.ScreenSaver",
        kind: UpstreamKind::Companion,
    },
    Upstream {
        service: "org.cinnamon.ScreenSaver",
        path: "/org/cinnamon/ScreenSaver",
        iface: "org.cinnamon.ScreenSaver",
        kind: UpstreamKind::Companion,
    },
    Upstream {
        service: "org.gnome.ScreenSaver",
        path: "/org/gnome/ScreenSaver",
        iface: "org.gnome.ScreenSaver",
        kind: UpstreamKind::Companion,
    },
    Upstream {
        service: "org.xfce.ScreenSaver",
        path: "/org/xfce/ScreenSaver",
        iface: "org.xfce.ScreenSaver",
        kind: UpstreamKind::Companion,
    },
];

/// Что поставить.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub upstream: Upstream,
    pub companion: Option<Upstream>,
}

/// Ответ «ставить ли мост».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Start(Plan),
    /// Не ставить — с причиной для журнала.
    Skip(String),
}

/// Решить, ставить ли мост. `has_owner(имя)` — есть ли у имени хозяин на
/// шине сеанса (`None` — шины нет).
/// `mate_screensaver_version` — вывод [`MATE_SCREENSAVER_VERSION_ARGV`]
/// (`None` — не спрашивали или команды нет).
/// `wayland()` — что известно о сеансе Wayland (`None` — не Wayland или
/// композитор не ответил); спрашивается, только когда по шине держать нечем.
pub fn decide(
    de: Option<&str>,
    mate_screensaver_version: Option<&str>,
    has_owner: impl Fn(&str) -> Option<bool>,
    wayland: impl FnOnce() -> Option<WaylandFacts>,
) -> Decision {
    if wants_mate_screensaver_version(de) {
        if let Some(v) = mate_screensaver_version.and_then(parse_version) {
            if v >= MATE_SCREENSAVER_FDO_SINCE {
                return Decision::Skip(format!(
                    "mate-screensaver {}.{}.{} даёт {SERVICE} сам",
                    v.0, v.1, v.2
                ));
            }
        }
    }
    let policy = desktop_policy(de);
    if let DesktopPolicy::OwnProvider(who) = policy {
        return Decision::Skip(format!(
            "окружение {} даёт {SERVICE} само ({who})",
            de.unwrap_or("?")
        ));
    }
    match has_owner(SERVICE) {
        None => return Decision::Skip("шины сеанса нет".into()),
        Some(true) => return Decision::Skip(format!("у {SERVICE} уже есть хозяин")),
        Some(false) => {}
    }
    let bus_upstream = match policy {
        DesktopPolicy::NoBusTarget(_) => None,
        _ => UPSTREAMS.iter().find(|u| has_owner(u.service) == Some(true)),
    };
    if let Some(upstream) = bus_upstream {
        let companion = COMPANIONS
            .iter()
            .find(|c| has_owner(c.service) == Some(true))
            .copied();
        return Decision::Start(Plan {
            upstream: *upstream,
            companion,
        });
    }
    // По шине держать нечем — остаётся протокол Wayland.
    let facts = wayland();
    match wayland_idle_check(facts.as_ref()) {
        Ok(()) => Decision::Start(Plan { upstream: WAYLAND_IDLE, companion: None }),
        Err(why) => Decision::Skip(match policy {
            DesktopPolicy::NoBusTarget(who) => format!(
                "в окружении {} экран гасит {who}, а запрет Wayland поставить нельзя: {why}",
                de.unwrap_or("?")
            ),
            _ => format!(
                "{SERVICE} свободно, но пробрасывать некуда (нет {}), а запрет Wayland поставить нельзя: {why}",
                UPSTREAMS.iter().map(|u| u.service).collect::<Vec<_>>().join(", ")
            ),
        }),
    }
}

// ============================================================================
// Учёт удержаний
// ============================================================================

/// Что сделать исполнителю. Исполняется по порядку.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Ответить на квитанцию значениями по сигнатуре.
    Reply {
        token: u64,
        signature: &'static str,
        values: Vec<String>,
    },
    /// Ответить ошибкой.
    ReplyError {
        token: u64,
        name: &'static str,
        text: String,
    },
    /// Позвать чужую службу; ответ вернуть в [`Book::on_call_done`] с `ctx`.
    Call {
        ctx: u64,
        target: Upstream,
        method: &'static str,
        signature: &'static str,
        values: Vec<String>,
        /// Сколько ждать ответа, мс; [`CALL_TIMEOUT_DEFAULT`] — срок шины.
        timeout_ms: i32,
    },
    /// Держать (`true`) или отпустить (`false`) запрет простоя протоколом
    /// Wayland ([`WAYLAND_IDLE`]). Один на все удержания: «держать» — когда
    /// появилось первое, «отпустить» — когда ушло последнее.
    HoldIdle(bool),
    /// Следить за исчезновением имени / перестать.
    Watch(String),
    Unwatch(String),
    /// Строка в журнал.
    Note(String),
}

/// Состояние проброса одного удержания.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    /// Вызов к менеджеру сеанса ушёл, ответа ещё нет.
    Pending,
    /// Менеджер сеанса выдал свою куки.
    Upstream(u32),
    /// Менеджер сеанса отказал.
    Failed,
    /// Держит сам мост общим запретом Wayland — снимать у сеанса нечего.
    Local,
}

#[derive(Debug, Clone)]
struct Entry {
    sender: String,
    app: String,
    held: Held,
}

/// Чего ждёт вызов к чужой службе.
#[derive(Debug, Clone, Copy)]
enum Purpose {
    /// Проброс удержания с нашей куки.
    Inhibit(u32),
    /// Снятие проброшенного удержания.
    Release,
    /// Вопрос хранителю экрана; ответить на квитанцию клиента.
    Companion { token: u64, reply: CompanionReply },
}

/// Как превратить ответ хранителя экрана в ответ клиенту.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompanionReply {
    /// `b` из ответа; при ошибке — `false`.
    Bool,
    /// `u` из ответа; при ошибке — 0.
    Uint,
    /// Успех → `true`, ошибка → `false` (`SetActive`).
    Success,
    /// Пустой ответ в любом случае (`SimulateUserActivity`).
    Empty,
    /// Пустой ответ при успехе, ошибка — клиенту (`Lock`).
    EmptyOrError,
}

/// Срок ответа шины по умолчанию (QtDBus: `-1`, около 25 с).
pub const CALL_TIMEOUT_DEFAULT: i32 = -1;
/// Срок ответа хранителя экрана: его ответа ждёт клиент, служба локальная.
pub const COMPANION_TIMEOUT_MS: i32 = 2000;

/// Первая куки живого моста: от часов, чтобы у мостов разных запусков
/// Rexon номера не совпадали. Никогда не 0 и не 1.
pub fn live_first_cookie() -> u32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let mixed = (now.as_secs() as u32).wrapping_mul(1000).wrapping_add(now.subsec_millis());
    mixed.max(2)
}

/// Учёт удержаний моста: куки, кто взял, что выдал менеджер сеанса.
#[derive(Debug)]
pub struct Book {
    plan: Plan,
    next_cookie: u32,
    next_ctx: u64,
    entries: BTreeMap<u32, Entry>,
    calls: BTreeMap<u64, Purpose>,
    /// Держится ли сейчас запрет Wayland (что последним велели исполнителю).
    holding_idle: bool,
}

impl Book {
    pub fn new(plan: Plan) -> Self {
        Self {
            plan,
            next_cookie: 1,
            next_ctx: 1,
            entries: BTreeMap::new(),
            calls: BTreeMap::new(),
            holding_idle: false,
        }
    }

    /// Учёт, чьи куки начинаются с `first` ([`live_first_cookie`] у живого
    /// моста). С единицы у каждого процесса нельзя: клиент, взявший куки 1 у
    /// прежнего Rexon, после перезапуска снял бы ею чужое удержание нового.
    pub fn starting_at(plan: Plan, first: u32) -> Self {
        Self { next_cookie: first.max(1), ..Self::new(plan) }
    }

    /// Сколько удержаний живо (для журнала и тестов).
    pub fn active(&self) -> usize {
        self.entries.len()
    }

    fn alloc_cookie(&mut self) -> u32 {
        loop {
            let c = self.next_cookie;
            self.next_cookie = self.next_cookie.wrapping_add(1).max(1);
            if c != 0 && !self.entries.contains_key(&c) {
                return c;
            }
        }
    }

    fn call(&mut self, purpose: Purpose, target: Upstream, method: &'static str,
            signature: &'static str, values: Vec<String>) -> Action {
        let ctx = self.next_ctx;
        self.next_ctx += 1;
        // Ответа хранителя ждёт клиент — срок короткий. Проброс и снятие
        // удержания никто не ждёт, а выброшенный по сроку ответ оставил бы
        // выданную сеансом куки без хозяина.
        let timeout_ms = match purpose {
            Purpose::Companion { .. } => COMPANION_TIMEOUT_MS,
            Purpose::Inhibit(_) | Purpose::Release => CALL_TIMEOUT_DEFAULT,
        };
        self.calls.insert(ctx, purpose);
        Action::Call { ctx, target, method, signature, values, timeout_ms }
    }

    /// Запрет Wayland: держать, пока живо хоть одно удержание.
    fn sync_idle_hold(&mut self, out: &mut Vec<Action>) {
        if self.plan.upstream.kind != UpstreamKind::WaylandIdleInhibit {
            return;
        }
        let want = !self.entries.is_empty();
        if want != self.holding_idle {
            self.holding_idle = want;
            out.push(Action::HoldIdle(want));
        }
    }

    fn sender_has_entries(&self, sender: &str) -> bool {
        self.entries.values().any(|e| e.sender == sender)
    }

    /// Снять проброшенное удержание, если оно есть.
    fn release(&mut self, held: Held, out: &mut Vec<Action>) {
        if let Held::Upstream(up) = held {
            let target = self.plan.upstream;
            // У менеджера сеанса — `Uninhibit`, у управления питанием —
            // `UnInhibit`: имена различаются регистром одной буквы.
            let method = match target.kind {
                UpstreamKind::GnomeSession => "Uninhibit",
                UpstreamKind::PowerManagement
                | UpstreamKind::Companion
                | UpstreamKind::WaylandIdleInhibit => "UnInhibit",
            };
            out.push(self.call(Purpose::Release, target, method, "u", vec![up.to_string()]));
        }
    }

    /// Пришёл вызов на наш объект.
    pub fn on_message(
        &mut self,
        token: u64,
        sender: &str,
        iface: &str,
        member: &str,
        signature: &str,
        args: &[String],
    ) -> Vec<Action> {
        let mut out = Vec::new();
        // QtDBus отдаёт объекту-наследнику QDBusVirtualObject и вызов
        // Introspect (замер стенда 17.10.2026) — отвечаем описанием узла.
        if iface == INTROSPECTABLE && member == "Introspect" {
            out.push(Action::Reply { token, signature: "s", values: vec![introspection_node()] });
            return out;
        }
        // Интерфейс в вызове необязателен (D-Bus разрешает звать по одному
        // имени метода); чужой — не наш.
        if !iface.is_empty() && iface != INTERFACE {
            out.push(Action::ReplyError {
                token,
                name: ERROR_UNKNOWN_METHOD,
                text: format!("нет интерфейса {iface}"),
            });
            return out;
        }
        let expected = match member {
            "Inhibit" => "ss",
            "UnInhibit" => "u",
            "SetActive" => "b",
            "GetActive" | "GetActiveTime" | "SimulateUserActivity" | "Lock" => "",
            _ => {
                out.push(Action::ReplyError {
                    token,
                    name: ERROR_UNKNOWN_METHOD,
                    text: format!("нет метода {member}"),
                });
                return out;
            }
        };
        if signature != expected || args.len() != expected.len() {
            out.push(Action::ReplyError {
                token,
                name: ERROR_INVALID_ARGS,
                text: format!("{member}: ожидалась сигнатура «{expected}», получена «{signature}»"),
            });
            return out;
        }
        match member {
            "Inhibit" => self.inhibit(token, sender, &args[0], &args[1], &mut out),
            "UnInhibit" => {
                let cookie = args[0].parse::<u32>().unwrap_or(0);
                self.uninhibit(token, cookie, &mut out);
            }
            "GetActive" => self.ask_companion(token, "GetActive", "", vec![], CompanionReply::Bool, &mut out),
            "GetActiveTime" => {
                self.ask_companion(token, "GetActiveTime", "", vec![], CompanionReply::Uint, &mut out)
            }
            "SetActive" => self.ask_companion(
                token,
                "SetActive",
                "b",
                vec![args[0].clone()],
                CompanionReply::Success,
                &mut out,
            ),
            "SimulateUserActivity" => self.ask_companion(
                token,
                "SimulateUserActivity",
                "",
                vec![],
                CompanionReply::Empty,
                &mut out,
            ),
            "Lock" => self.ask_companion(token, "Lock", "", vec![], CompanionReply::EmptyOrError, &mut out),
            _ => unreachable!("метод проверен выше"),
        }
        out
    }

    fn inhibit(&mut self, token: u64, sender: &str, app: &str, reason: &str, out: &mut Vec<Action>) {
        let cookie = self.alloc_cookie();
        let first_of_sender = !self.sender_has_entries(sender);
        // Менеджер сеанса отказывает в удержании без имени приложения или без
        // причины — подставляем то, что знаем точно: имя клиента на шине и
        // имя приложения.
        let app_id = if app.trim().is_empty() { sender.to_string() } else { app.to_string() };
        let why = if reason.trim().is_empty() { app_id.clone() } else { reason.to_string() };
        self.entries.insert(
            cookie,
            Entry {
                sender: sender.to_string(),
                app: app_id.clone(),
                held: Held::Pending,
            },
        );
        // Клиенту — сразу: ему нужна куки, а не ответ менеджера сеанса.
        out.push(Action::Reply { token, signature: "u", values: vec![cookie.to_string()] });
        if first_of_sender {
            out.push(Action::Watch(sender.to_string()));
        }
        let target = self.plan.upstream;
        let (signature, values) = match target.kind {
            UpstreamKind::GnomeSession => (
                "susu",
                vec![app_id.clone(), "0".into(), why.clone(), GSM_INHIBIT_IDLE.to_string()],
            ),
            UpstreamKind::PowerManagement | UpstreamKind::Companion => {
                ("ss", vec![app_id.clone(), why.clone()])
            }
            UpstreamKind::WaylandIdleInhibit => ("", vec![]),
        };
        if target.kind == UpstreamKind::WaylandIdleInhibit {
            if let Some(e) = self.entries.get_mut(&cookie) {
                e.held = Held::Local;
            }
            self.sync_idle_hold(out);
        } else {
            out.push(self.call(Purpose::Inhibit(cookie), target, "Inhibit", signature, values));
        }
        out.push(Action::Note(format!(
            "удержание {cookie}: «{app_id}» ({why}) от {sender}"
        )));
    }

    fn uninhibit(&mut self, token: u64, cookie: u32, out: &mut Vec<Action>) {
        // Спецификация ошибки для чужой/неизвестной куки не задаёт; как и у
        // Plasma, отвечаем пусто — клиенту нечего с этим делать.
        out.push(Action::Reply { token, signature: "", values: vec![] });
        let Some(entry) = self.entries.remove(&cookie) else {
            out.push(Action::Note(format!("снятие {cookie}: такой куки нет")));
            return;
        };
        self.release(entry.held, out);
        self.sync_idle_hold(out);
        if !self.sender_has_entries(&entry.sender) {
            out.push(Action::Unwatch(entry.sender.clone()));
        }
        out.push(Action::Note(format!("удержание {cookie} («{}») снято клиентом", entry.app)));
    }

    fn ask_companion(
        &mut self,
        token: u64,
        method: &'static str,
        signature: &'static str,
        values: Vec<String>,
        reply: CompanionReply,
        out: &mut Vec<Action>,
    ) {
        match self.plan.companion {
            Some(c) => out.push(self.call(Purpose::Companion { token, reply }, c, method, signature, values)),
            None => out.push(companion_reply(token, reply, None)),
        }
    }

    /// Пришёл ответ чужой службы на `Action::Call`.
    pub fn on_call_done(&mut self, ctx: u64, ok: bool, value: &str) -> Vec<Action> {
        let mut out = Vec::new();
        let Some(purpose) = self.calls.remove(&ctx) else {
            return out;
        };
        match purpose {
            Purpose::Inhibit(cookie) => {
                let up = if ok { value.parse::<u32>().ok() } else { None };
                match (self.entries.get_mut(&cookie), up) {
                    (Some(e), Some(up)) => {
                        e.held = Held::Upstream(up);
                        out.push(Action::Note(format!("удержание {cookie} передано сеансу (куки {up})")));
                    }
                    (Some(e), None) => {
                        e.held = Held::Failed;
                        out.push(Action::Note(format!("сеанс не принял удержание {cookie}: {value}")));
                    }
                    // Клиент успел снять удержание (или исчез), пока сеанс
                    // отвечал, — снять и у сеанса, иначе оно повиснет.
                    (None, Some(up)) => self.release(Held::Upstream(up), &mut out),
                    (None, None) => {}
                }
            }
            Purpose::Release => {
                if !ok {
                    out.push(Action::Note(format!("сеанс не снял удержание: {value}")));
                }
            }
            Purpose::Companion { token, reply } => {
                out.push(companion_reply(token, reply, ok.then_some(value)));
            }
        }
        out
    }

    /// Имя ушло другому хозяину (или мост сходит): снять у сеанса всё.
    ///
    /// Удержания, ответа на которые сеанс ещё не дал, снимутся по его ответу
    /// ([`Book::on_call_done`] видит, что записи уже нет) — если исполнитель
    /// к тому времени ещё жив.
    pub fn release_all(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        let all = std::mem::take(&mut self.entries);
        for (cookie, e) in all {
            self.release(e.held, &mut out);
            out.push(Action::Note(format!("удержание {cookie} («{}») снято: мост сошёл", e.app)));
        }
        self.sync_idle_hold(&mut out);
        out
    }

    /// Имя клиента исчезло с шины: снять всё, что он держал.
    pub fn on_vanished(&mut self, name: &str) -> Vec<Action> {
        let mut out = Vec::new();
        let gone: Vec<u32> = self
            .entries
            .iter()
            .filter(|(_, e)| e.sender == name)
            .map(|(c, _)| *c)
            .collect();
        if gone.is_empty() {
            return out;
        }
        for cookie in &gone {
            if let Some(e) = self.entries.remove(cookie) {
                self.release(e.held, &mut out);
                out.push(Action::Note(format!(
                    "удержание {cookie} («{}») снято: {name} ушёл с шины",
                    e.app
                )));
            }
        }
        self.sync_idle_hold(&mut out);
        out.push(Action::Unwatch(name.to_string()));
        out
    }
}

/// Ответ клиенту по ответу хранителя экрана (`None` — ошибка или хранителя
/// нет).
fn companion_reply(token: u64, reply: CompanionReply, value: Option<&str>) -> Action {
    match reply {
        CompanionReply::Bool => Action::Reply {
            token,
            signature: "b",
            values: vec![(value == Some("true")).to_string()],
        },
        CompanionReply::Uint => Action::Reply {
            token,
            signature: "u",
            values: vec![value.and_then(|v| v.parse::<u32>().ok()).unwrap_or(0).to_string()],
        },
        CompanionReply::Success => Action::Reply {
            token,
            signature: "b",
            values: vec![value.is_some().to_string()],
        },
        CompanionReply::Empty => Action::Reply { token, signature: "", values: vec![] },
        CompanionReply::EmptyOrError => match value {
            Some(_) => Action::Reply { token, signature: "", values: vec![] },
            None => Action::ReplyError {
                token,
                name: ERROR_NOT_SUPPORTED,
                text: "хранитель экрана окружения не ответил".into(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MATE_SS: Upstream = Upstream {
        service: "org.mate.ScreenSaver",
        path: "/org/mate/ScreenSaver",
        iface: "org.mate.ScreenSaver",
        kind: UpstreamKind::Companion,
    };

    fn plan(companion: Option<Upstream>) -> Plan {
        Plan { upstream: GNOME_SESSION, companion }
    }

    fn s(v: &str) -> String {
        v.to_string()
    }

    /// Номер вызова и его аргументы из списка действий.
    fn calls(actions: &[Action]) -> Vec<(u64, &'static str, Vec<String>)> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Call { ctx, method, values, .. } => Some((*ctx, *method, values.clone())),
                _ => None,
            })
            .collect()
    }

    fn reply_values(actions: &[Action], tok: u64) -> Option<Vec<String>> {
        actions.iter().find_map(|a| match a {
            Action::Reply { token, values, .. } if *token == tok => Some(values.clone()),
            _ => None,
        })
    }

    #[test]
    fn known_providers_are_left_alone() {
        // Даже при свободном имени и живом менеджере сеанса: хозяин имени в
        // этих окружениях свой, и отнимать его нельзя.
        for de in ["kde", "gnome", "cinnamon-mint", "cosmic", "budgie"] {
            let d = decide(Some(de), None, |_| Some(false), || None);
            assert!(matches!(d, Decision::Skip(_)), "{de}: {d:?}");
            let owners = ["org.gnome.SessionManager"];
            let d = decide(Some(de), None, |n| Some(owners.contains(&n)), || None);
            assert!(matches!(d, Decision::Skip(_)), "{de}: {d:?}");
        }
    }

    #[test]
    fn xfce_forwards_to_power_management() {
        let owners = ["org.freedesktop.PowerManagement", "org.xfce.ScreenSaver"];
        let d = decide(Some("xfce"), None, |n| Some(owners.contains(&n)), || None);
        let Decision::Start(p) = d else { panic!("{d:?}") };
        assert_eq!(p.upstream, POWER_MANAGEMENT);
        let mut b = Book::new(p);
        let a = b.on_message(1, ":1.2", INTERFACE, "Inhibit", "ss", &[s("Firefox"), s("video")]);
        let c = calls(&a);
        assert_eq!((c[0].1, c[0].2.clone()), ("Inhibit", vec![s("Firefox"), s("video")]));
        b.on_call_done(c[0].0, true, "17");
        let a = b.on_message(2, ":1.2", INTERFACE, "UnInhibit", "u", &[s("1")]);
        let c = calls(&a);
        assert_eq!((c[0].1, c[0].2.clone()), ("UnInhibit", vec![s("17")]));
    }

    #[test]
    fn mate_gets_bridge_to_session_manager() {
        let owners = ["org.gnome.SessionManager", "org.mate.ScreenSaver"];
        let d = decide(Some("mate"), None, |n| Some(owners.contains(&n)), || None);
        assert_eq!(d, Decision::Start(plan(Some(MATE_SS))));
        // Живая машина 17.10.2026: 1.28.0 — мост нужен.
        let d = decide(Some("mate"), Some("mate-screensaver-command 1.28.0\n"), |n| Some(owners.contains(&n)), || None);
        assert_eq!(d, Decision::Start(plan(Some(MATE_SS))));
    }

    #[test]
    fn mate_screensaver_1_28_1_owns_the_name_itself() {
        // Даже если в эту минуту имя свободно (хранитель ещё не встал или
        // перезапускается): он берёт имя без замены, занимать нельзя.
        let owners = ["org.gnome.SessionManager", "org.mate.ScreenSaver"];
        for v in ["mate-screensaver-command 1.28.1", "mate-screensaver-command 1.30.0"] {
            let d = decide(Some("mate"), Some(v), |n| Some(owners.contains(&n)), || None);
            assert!(matches!(d, Decision::Skip(_)), "{v}: {d:?}");
        }
        assert_eq!(parse_version("mate-screensaver 1.28"), Some((1, 28, 0)));
        assert_eq!(parse_version("ошибка"), None);
    }

    /// Куки прежнего процесса Rexon (клиент держал её до перезапуска) не
    /// снимает чужое удержание нового моста: нумерация у каждого моста
    /// своя, а не с единицы.
    #[test]
    fn stale_cookie_of_previous_bridge_releases_nothing() {
        let mut b = Book::starting_at(plan(None), 70_001);
        let a = b.on_message(1, ":1.9", INTERFACE, "Inhibit", "ss", &[s("mpv"), s("video")]);
        b.on_call_done(calls(&a)[0].0, true, "50");
        let a = b.on_message(2, ":1.7", INTERFACE, "UnInhibit", "u", &[s("1")]);
        assert!(calls(&a).is_empty(), "снято чужое удержание: {a:?}");
        assert_eq!(b.active(), 1);
        // Живой мост начинает не с единицы и не с нуля.
        assert!(live_first_cookie() > 1);
    }

    /// Проброс удержания сеансу ждёт ответа сколько нужно: по истечении
    /// короткого срока ответ выбрасывался, а сеанс всё равно выдавал куки —
    /// удержание висело за Rexon до конца сеанса. Короткий срок — только у
    /// вопросов хранителю, ответа на которые ждёт клиент.
    #[test]
    fn forwarded_inhibit_waits_for_the_session_answer() {
        let timeouts = |actions: &[Action]| -> Vec<(&'static str, i32)> {
            actions
                .iter()
                .filter_map(|a| match a {
                    Action::Call { method, timeout_ms, .. } => Some((*method, *timeout_ms)),
                    _ => None,
                })
                .collect()
        };
        let mut b = Book::new(plan(Some(GNOME_SESSION)));
        let a = b.on_message(1, ":1.4", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        assert_eq!(timeouts(&a), [("Inhibit", CALL_TIMEOUT_DEFAULT)]);
        b.on_call_done(calls(&a)[0].0, true, "41");
        let a = b.on_message(2, ":1.4", INTERFACE, "UnInhibit", "u", &[s("1")]);
        assert_eq!(timeouts(&a), [("Uninhibit", CALL_TIMEOUT_DEFAULT)]);
        let a = b.on_message(3, ":1.4", INTERFACE, "GetActive", "", &[]);
        assert_eq!(timeouts(&a), [("GetActive", COMPANION_TIMEOUT_MS)]);
    }

    #[test]
    fn lost_name_releases_everything() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.4", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        b.on_call_done(calls(&a)[0].0, true, "41");
        b.on_message(2, ":1.5", INTERFACE, "Inhibit", "ss", &[s("b"), s("r")]);
        let a = b.release_all();
        let c = calls(&a);
        assert_eq!(c.len(), 1, "снимается только выданное сеансом");
        assert_eq!(c[0].2, vec![s("41")]);
        assert_eq!(b.active(), 0);
    }

    #[test]
    fn taken_name_or_no_bus_or_no_upstream_means_nothing() {
        assert!(matches!(decide(Some("mate"), None, |_| Some(true), || None), Decision::Skip(_)));
        assert!(matches!(decide(Some("mate"), None, |_| None, || None), Decision::Skip(_)));
        // Имя свободно, но держать экран нечем — имя НЕ занимаем: иначе
        // клиент получил бы «удержано» при гаснущем экране.
        assert!(matches!(decide(Some("sway"), None, |_| Some(false), || None), Decision::Skip(_)));
    }

    #[test]
    fn inhibit_replies_cookie_at_once_and_forwards_idle_flag() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(7, ":1.42", INTERFACE, "Inhibit", "ss", &[s("Firefox"), s("video-playing")]);
        assert_eq!(reply_values(&a, 7), Some(vec![s("1")]));
        assert!(a.contains(&Action::Watch(s(":1.42"))));
        let c = calls(&a);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].1, "Inhibit");
        assert_eq!(c[0].2, vec![s("Firefox"), s("0"), s("video-playing"), s("8")]);
        assert_eq!(b.active(), 1);
    }

    #[test]
    fn uninhibit_releases_upstream_cookie() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.42", "", "Inhibit", "ss", &[s("mpv"), s("play")]);
        let ctx = calls(&a)[0].0;
        b.on_call_done(ctx, true, "555");
        let a = b.on_message(2, ":1.42", INTERFACE, "UnInhibit", "u", &[s("1")]);
        assert_eq!(reply_values(&a, 2), Some(vec![]));
        let c = calls(&a);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].1, c[0].2.clone()), ("Uninhibit", vec![s("555")]));
        assert!(a.contains(&Action::Unwatch(s(":1.42"))));
        assert_eq!(b.active(), 0);
    }

    #[test]
    fn uninhibit_before_session_answered_releases_late_cookie() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.5", INTERFACE, "Inhibit", "ss", &[s("vlc"), s("x")]);
        let ctx = calls(&a)[0].0;
        let a = b.on_message(2, ":1.5", INTERFACE, "UnInhibit", "u", &[s("1")]);
        assert!(calls(&a).is_empty(), "снимать у сеанса пока нечего");
        // Сеанс ответил уже после снятия — его куки обязаны вернуть.
        let a = b.on_call_done(ctx, true, "99");
        let c = calls(&a);
        assert_eq!((c[0].1, c[0].2.clone()), ("Uninhibit", vec![s("99")]));
    }

    #[test]
    fn vanished_client_drops_all_its_inhibitors_only() {
        let mut b = Book::new(plan(None));
        let mut ctxs = Vec::new();
        for (tok, who) in [(1, ":1.10"), (2, ":1.10"), (3, ":1.11")] {
            let a = b.on_message(tok, who, INTERFACE, "Inhibit", "ss", &[s("app"), s("r")]);
            ctxs.push(calls(&a)[0].0);
        }
        // Второе удержание того же клиента следить повторно не просит.
        for (i, ctx) in ctxs.iter().enumerate() {
            b.on_call_done(*ctx, true, &(100 + i).to_string());
        }
        let a = b.on_vanished(":1.10");
        let mut released: Vec<String> = calls(&a).into_iter().map(|c| c.2[0].clone()).collect();
        released.sort();
        assert_eq!(released, vec![s("100"), s("101")]);
        assert!(a.contains(&Action::Unwatch(s(":1.10"))));
        assert_eq!(b.active(), 1);
        assert!(b.on_vanished(":1.99").is_empty());
    }

    #[test]
    fn second_inhibit_of_same_client_does_not_watch_twice() {
        let mut b = Book::new(plan(None));
        b.on_message(1, ":1.3", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        let a = b.on_message(2, ":1.3", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        assert!(!a.iter().any(|x| matches!(x, Action::Watch(_))));
        assert_eq!(reply_values(&a, 2), Some(vec![s("2")]));
    }

    #[test]
    fn rejected_by_session_is_kept_but_not_released() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.3", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        b.on_call_done(calls(&a)[0].0, false, "org.gnome.SessionManager.GeneralError нет");
        let a = b.on_message(2, ":1.3", INTERFACE, "UnInhibit", "u", &[s("1")]);
        assert!(calls(&a).is_empty());
    }

    #[test]
    fn empty_app_and_reason_get_filled() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.8", INTERFACE, "Inhibit", "ss", &[s(""), s(" ")]);
        assert_eq!(calls(&a)[0].2, vec![s(":1.8"), s("0"), s(":1.8"), s("8")]);
    }

    #[test]
    fn cookies_never_zero_and_unique() {
        let mut b = Book::new(plan(None));
        b.next_cookie = u32::MAX;
        let a = b.on_message(1, ":1.1", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        let a2 = b.on_message(2, ":1.1", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        assert_eq!(reply_values(&a, 1), Some(vec![u32::MAX.to_string()]));
        assert_eq!(reply_values(&a2, 2), Some(vec![s("1")]));
    }

    #[test]
    fn bad_calls_get_errors() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.1", INTERFACE, "Throttle", "ss", &[s("a"), s("b")]);
        assert!(matches!(&a[0], Action::ReplyError { name, .. } if *name == ERROR_UNKNOWN_METHOD));
        let a = b.on_message(2, ":1.1", INTERFACE, "Inhibit", "s", &[s("a")]);
        assert!(matches!(&a[0], Action::ReplyError { name, .. } if *name == ERROR_INVALID_ARGS));
        let a = b.on_message(3, ":1.1", "org.freedesktop.DBus.Properties", "Get", "ss", &[s("a"), s("b")]);
        assert!(matches!(&a[0], Action::ReplyError { .. }));
    }

    #[test]
    fn without_companion_answers_are_honest_defaults() {
        let mut b = Book::new(plan(None));
        assert_eq!(reply_values(&b.on_message(1, ":1.1", "", "GetActive", "", &[]), 1), Some(vec![s("false")]));
        assert_eq!(reply_values(&b.on_message(2, ":1.1", "", "GetActiveTime", "", &[]), 2), Some(vec![s("0")]));
        assert_eq!(
            reply_values(&b.on_message(3, ":1.1", "", "SetActive", "b", &[s("true")]), 3),
            Some(vec![s("false")])
        );
        let a = b.on_message(4, ":1.1", "", "Lock", "", &[]);
        assert!(matches!(&a[0], Action::ReplyError { name, .. } if *name == ERROR_NOT_SUPPORTED));
    }

    #[test]
    fn companion_answers_are_forwarded() {
        let mut b = Book::new(plan(Some(MATE_SS)));
        let a = b.on_message(1, ":1.1", "", "GetActive", "", &[]);
        let ctx = calls(&a)[0].0;
        assert_eq!(reply_values(&b.on_call_done(ctx, true, "true"), 1), Some(vec![s("true")]));
        let a = b.on_message(2, ":1.1", "", "SetActive", "b", &[s("true")]);
        let c = calls(&a);
        assert_eq!(c[0].2, vec![s("true")]);
        // У mate-screensaver SetActive ничего не возвращает: успех — `true`.
        assert_eq!(reply_values(&b.on_call_done(c[0].0, true, ""), 2), Some(vec![s("true")]));
        let a = b.on_message(3, ":1.1", "", "GetActiveTime", "", &[]);
        assert_eq!(reply_values(&b.on_call_done(calls(&a)[0].0, false, "err"), 3), Some(vec![s("0")]));
    }

    fn wl(compositor: &str) -> Option<WaylandFacts> {
        Some(WaylandFacts {
            compositor: compositor.into(),
            globals: WAYLAND_GLOBALS_NEEDED.iter().map(|g| g.to_string()).collect(),
        })
    }

    /// sway, Budgie и LXQt на labwc: имя свободно, по шине пробросить некуда
    /// — мост держит экран запретом Wayland.
    #[test]
    fn wayland_without_bus_target_holds_idle_inhibit() {
        let wayland_plan = Decision::Start(Plan { upstream: WAYLAND_IDLE, companion: None });
        for (de, comp) in [("sway", "sway"), ("budgie", "labwc"), ("lxqt", "labwc"), ("hyprland", "Hyprland")] {
            assert_eq!(decide(Some(de), None, |_| Some(false), || wl(comp)), wayland_plan, "{de}/{comp}");
        }
        // Budgie: budgie-session держит org.gnome.SessionManager, но swayidle
        // на него не смотрит — всё равно Wayland.
        let owners = ["org.gnome.SessionManager"];
        assert_eq!(decide(Some("budgie"), None, |n| Some(owners.contains(&n)), || wl("labwc")), wayland_plan);
    }

    /// Композитор, учёт которого не проверен, или нет нужных глобалов, или
    /// сеанс не Wayland — имя НЕ занимаем: нативный Firefox иначе остановился
    /// бы на нас вместо своего Wayland-запрета.
    #[test]
    fn unverified_compositor_or_missing_globals_means_no_bridge() {
        for comp in ["niri", "kwin_wayland", "magpie-wm", "wayfire", "river", ""] {
            let d = decide(Some("lxqt"), None, |_| Some(false), || wl(comp));
            assert!(matches!(d, Decision::Skip(_)), "{comp}: {d:?}");
        }
        let partial = || {
            Some(WaylandFacts { compositor: "sway".into(), globals: vec!["wl_compositor".into(), "wl_shm".into()] })
        };
        let Decision::Skip(why) = decide(Some("sway"), None, |_| Some(false), partial) else { panic!() };
        assert!(why.contains("zwp_idle_inhibit_manager_v1"), "{why}");
        assert!(matches!(decide(Some("budgie"), None, |_| Some(false), || None), Decision::Skip(_)));
    }

    /// По шине есть куда пробросить (XFCE на Wayland) — шина в приоритете,
    /// у композитора даже не спрашиваем.
    #[test]
    fn bus_upstream_wins_over_wayland() {
        let owners = ["org.freedesktop.PowerManagement"];
        let d = decide(Some("xfce"), None, |n| Some(owners.contains(&n)), || panic!("спросили Wayland"));
        assert!(matches!(d, Decision::Start(Plan { upstream: POWER_MANAGEMENT, .. })), "{d:?}");
    }

    /// niri держит имя сам; занятое имя и хозяин окружения — Wayland не
    /// спрашиваем вовсе.
    #[test]
    fn own_provider_or_taken_name_never_asks_wayland() {
        assert!(matches!(decide(Some("niri"), None, |_| Some(false), || panic!()), Decision::Skip(_)));
        assert!(matches!(decide(Some("sway"), None, |_| Some(true), || panic!()), Decision::Skip(_)));
    }

    fn holds(actions: &[Action]) -> Vec<bool> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::HoldIdle(on) => Some(*on),
                _ => None,
            })
            .collect()
    }

    /// Один запрет Wayland на все удержания: «держать» с первым, «отпустить»
    /// с последним, к шине ни одного вызова.
    #[test]
    fn wayland_hold_follows_first_and_last_inhibitor() {
        let mut b = Book::new(Plan { upstream: WAYLAND_IDLE, companion: None });
        let a = b.on_message(1, ":1.20", INTERFACE, "Inhibit", "ss", &[s("firefox"), s("video")]);
        assert_eq!(reply_values(&a, 1), Some(vec![s("1")]));
        assert_eq!(holds(&a), [true]);
        assert!(calls(&a).is_empty(), "{a:?}");
        let a = b.on_message(2, ":1.21", INTERFACE, "Inhibit", "ss", &[s("steam"), s("game")]);
        assert!(holds(&a).is_empty(), "второе удержание держит тот же запрет");
        let a = b.on_message(3, ":1.20", INTERFACE, "UnInhibit", "u", &[s("1")]);
        assert!(holds(&a).is_empty() && calls(&a).is_empty(), "{a:?}");
        // Последний держатель ушёл с шины — отпустить.
        let a = b.on_vanished(":1.21");
        assert_eq!(holds(&a), [false]);
        assert_eq!(b.active(), 0);
        // И снова с нуля.
        let a = b.on_message(4, ":1.22", INTERFACE, "Inhibit", "ss", &[s("mpv"), s("play")]);
        assert_eq!(holds(&a), [true]);
        let a = b.on_message(5, ":1.22", INTERFACE, "UnInhibit", "u", &[s("3")]);
        assert_eq!(holds(&a), [false]);
    }

    /// Имя забрали — запрет Wayland отпускается вместе со всеми удержаниями.
    #[test]
    fn wayland_hold_released_when_bridge_steps_down() {
        let mut b = Book::new(Plan { upstream: WAYLAND_IDLE, companion: None });
        b.on_message(1, ":1.30", INTERFACE, "Inhibit", "ss", &[s("a"), s("r")]);
        let a = b.release_all();
        assert_eq!(holds(&a), [false]);
        assert!(calls(&a).is_empty());
        assert!(holds(&b.release_all()).is_empty(), "дважды не отпускаем");
    }

    #[test]
    fn introspect_call_gets_node() {
        let mut b = Book::new(plan(None));
        let a = b.on_message(1, ":1.1", INTROSPECTABLE, "Introspect", "", &[]);
        let v = reply_values(&a, 1).expect("ответ");
        assert!(v[0].contains("<node>") && v[0].contains("name=\"org.freedesktop.ScreenSaver\""));
    }

    #[test]
    fn introspection_declares_only_implemented_methods() {
        for m in ["Inhibit", "UnInhibit", "GetActive", "SetActive", "GetActiveTime", "SimulateUserActivity", "Lock"] {
            assert!(INTROSPECTION_XML.contains(&format!("<method name=\"{m}\">")), "{m}");
        }
        for m in ["Throttle", "GetSessionIdleTime", "ActiveChanged"] {
            assert!(!INTROSPECTION_XML.contains(m), "{m} объявлен, но не исполняется");
        }
    }
}
