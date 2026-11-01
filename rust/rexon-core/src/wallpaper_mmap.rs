//! Чистая логика mmap-стриминга обоев и X11-фолбэка — порт
//! `rexon/wallpaper_mmap.py` (сам модуль снят с дерева 2026-08-23 вместе
//! с Python-хостом обоев; здесь — вся уцелевшая логика).
//!
//! Оригинал обслуживает два пути рендера:
//!   * **mmap** (GNOME/Cinnamon extension): при заданном `REXON_WP_OUTPUT`
//!     движки пишут RGBA8888-кадры в shared-файл
//!     `/run/user/$UID/rexon-wp-frame.rgba` (`width × height × 4` байта),
//!     extension читает его через `GLib.MappedFile`;
//!   * **X11-фолбэк** (XFCE/MATE/Budgie/Cinnamon-X11): окно прибивается к
//!     рабочему столу по EWMH — тип `_NET_WM_WINDOW_TYPE_DESKTOP` и
//!     состояние до показа, просьбы корневому окну после
//!     ([`x11_desktop_plan`]).
//!
//! Здесь — ТОЛЬКО детерминированная часть (вход → выход): разбор ENV-размера
//! экрана, пути вывода и частоты кадров, расчёт layout кадра ([`prime_plan`]:
//! размер файла, число строк, чёрная непрозрачная строка, права), детект
//! Wayland-сессии, план X11-фолбэка ([`x11_desktop_plan`]: свойства до показа,
//! просьбы EWMH после, раскладка ClientMessage), интервал headless-тика.
//! Побочные действия — файлы, mmap, flock, запросы к X-серверу, Qt — у хоста
//! обоев (`rexon-gui::wallpaper_host`), который исполняет то, что решило
//! ядро (пути, размеры, байты, атомы). Чисел раскладки в хосте нет.
//!
//! Наблюдения за средой (значения `REXON_WP_*`, `XDG_SESSION_TYPE`,
//! `WAYLAND_DISPLAY`, runtime-каталог) приходят ПАРАМЕТРАМИ.
//!
//! Паритет с живым Python — golden `tests/wallpaper_mmap_parity.rs` (эталон
//! снят read-only: xprop НЕ запускался — subprocess подменён стабом с захватом
//! argv; Qt не стартовал — PySide6 подменён стабами; файлы prime писались
//! только во временный каталог).
//!
//! Сохранённый квирк оригинала (не баг): в `env_screen_size` ошибка разбора
//! ЛЮБОЙ из двух переменных сбрасывает ОБЕ в дефолт 1920×1080 (общий
//! `try/except ValueError` вокруг обеих `int()`), — атомарный фолбэк.

// ── константы ──────────────────────────────────────────────────────────────

/// Ширина кадра по умолчанию (`REXON_WP_WIDTH` не задан/не разобран).
pub const DEFAULT_WIDTH: i64 = 1920;
/// Высота кадра по умолчанию (`REXON_WP_HEIGHT` не задан/не разобран).
pub const DEFAULT_HEIGHT: i64 = 1080;
/// Байт на пиксель RGBA8888.
pub const BYTES_PER_PIXEL: u64 = 4;
/// Имя mmap-файла кадра в runtime-каталоге (то же, что `FRAME_PATH` в
/// GNOME/Cinnamon extension.js).
pub const FRAME_FILE_NAME: &str = "rexon-wp-frame.rgba";
/// Права mmap-файла — `os.open(..., 0o600)` в `prime_output_file` и
/// `MmapFrameSink.__init__` (per-user tmpfs, чужим читать нечего).
pub const FRAME_FILE_MODE: u32 = 0o600;
/// Имя Qt-приложения headless-цикла (`app.setApplicationName`).
///
/// Источник истины — таблица точек входа [`crate::qt_app::ENTRY_IDENTITIES`]
/// (`rexon.wallpaper_mmap`); здесь дубль для тех, кому не нужен весь
/// Qt-модуль. Совпадение проверяет `tests/wallpaper_mmap_parity.rs`.
pub const HEADLESS_APP_NAME: &str = "RexonWallpaperHeadless";
/// FPS mmap-потока по умолчанию (`run_headless_loop(..., fps=30)`,
/// `stream_window_to_mmap(..., fps=30)`, откат `_env_fps`).
pub const DEFAULT_FPS: i64 = 30;
/// Наименьшая осмысленная частота кадров mmap-потока (`1 <= fps` в `_env_fps`).
pub const MIN_FPS: i64 = 1;
/// Наибольшая принимаемая частота кадров (`fps <= 120`): выше — почти
/// наверняка опечатка в переменной окружения, а не желание.
pub const MAX_FPS: i64 = 120;

// ── пути и layout кадра ────────────────────────────────────────────────────

/// Канонический путь mmap-файла кадра: `<runtime_dir>/rexon-wp-frame.rgba`.
///
/// `runtime_dir` — наблюдение (`/run/user/$UID`, у extension'ов —
/// `GLib.get_user_runtime_dir()`); Python-сторона кладёт результат в
/// `REXON_WP_OUTPUT` для движков.
pub fn default_frame_path(runtime_dir: &str) -> String {
    format!("{runtime_dir}/{FRAME_FILE_NAME}")
}

/// Размер экрана из значений ENV — порт `env_screen_size`.
///
/// `width`/`height` — сырые значения `REXON_WP_WIDTH`/`REXON_WP_HEIGHT`
/// (`None` = переменная не задана → дефолт как строка). Разбор — семантика
/// Python `int()`: обрезка пробелов, знак; дробные/мусор → ValueError.
/// Ошибка разбора ЛЮБОЙ переменной → обе в 1920×1080 (квирк оригинала,
/// см. модульный док). Затем `max(1, …)` на каждую.
pub fn env_screen_size(width: Option<&str>, height: Option<&str>) -> (i64, i64) {
    let w_raw = width.unwrap_or("1920");
    let h_raw = height.unwrap_or("1080");
    let (w, h) = match (py_int(w_raw), py_int(h_raw)) {
        (Some(w), Some(h)) => (w, h),
        _ => (DEFAULT_WIDTH, DEFAULT_HEIGHT),
    };
    (w.max(1), h.max(1))
}

/// Путь mmap-файла из ENV — порт `env_output_path`:
/// `os.environ.get("REXON_WP_OUTPUT") or None` — не задан или пустая строка
/// → `None` (обычный QML-режим).
pub fn env_output_path(raw: Option<&str>) -> Option<String> {
    match raw {
        Some(s) if !s.is_empty() => Some(s.to_string()),
        _ => None,
    }
}

/// Размер mmap-файла кадра в байтах — `max(1, width) * max(1, height) * 4`
/// (как в `prime_output_file`; `MmapFrameSink` получает уже клампнутые
/// размеры из [`env_screen_size`]).
pub fn frame_size(width: i64, height: i64) -> u64 {
    width.max(1) as u64 * height.max(1) as u64 * BYTES_PER_PIXEL
}

/// Одна строка кадра ЧЁРНЫМ НЕПРОЗРАЧНЫМ для прайминга — порт
/// `row = b"\x00\x00\x00\xff" * max(1, width)` из `prime_output_file`.
///
/// RGBA8888: R=0 G=0 B=0 A=255. НЕ нули — нулевая альфа дала бы прозрачный
/// кадр и «вспышку» на старте scene/web-обоев. Python-шим пишет эту строку
/// [`PrimePlan::rows`] раз под `flock(LOCK_EX)`.
pub fn black_row(width: i64) -> Vec<u8> {
    let mut row = Vec::with_capacity(width.max(1) as usize * 4);
    for _ in 0..width.max(1) {
        row.extend_from_slice(&[0x00, 0x00, 0x00, 0xff]);
    }
    row
}

/// Что именно шим записывает в mmap-файл при прайминге — весь расчёт
/// `prime_output_file` разом, чтобы `max(1, height)` не оставался числом в
/// Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimePlan {
    /// Ширина кадра после клампа (`max(1, width)`) — столько пикселей в
    /// строке [`PrimePlan::row`].
    pub width: i64,
    /// Высота кадра после клампа (`max(1, height)`), она же
    /// [`PrimePlan::rows`].
    pub height: i64,
    /// Итоговый размер файла (`os.ftruncate`), байт.
    pub size: u64,
    /// Сколько раз записать [`PrimePlan::row`] (порт `range(max(1, height))`).
    pub rows: u64,
    /// Одна строка кадра — чёрный непрозрачный RGBA8888.
    pub row: Vec<u8>,
    /// Права на файл (`os.open(..., mode)`).
    pub mode: u32,
}

/// План прайминга mmap-файла кадра — порт расчётной части
/// `prime_output_file`. Побочные эффекты (makedirs/open/ftruncate/flock/
/// write/fsync) остаются шиму, числа приходят отсюда.
pub fn prime_plan(width: i64, height: i64) -> PrimePlan {
    PrimePlan {
        width: width.max(1),
        height: height.max(1),
        size: frame_size(width, height),
        rows: height.max(1) as u64,
        row: black_row(width),
        mode: FRAME_FILE_MODE,
    }
}

// ── детект Wayland-сессии ──────────────────────────────────────────────────

/// Wayland ли сессия — порт `is_wayland_session` (выбор между
/// LayerShellQt-QML и обычным QML с xprop-триком).
///
/// `xdg_session_type`/`wayland_display` — значения одноимённых ENV (не задана
/// → пустая строка, как `os.environ.get(..., "")`). True, если
/// `XDG_SESSION_TYPE` == "wayland" (без учёта регистра) ИЛИ `WAYLAND_DISPLAY`
/// непуст.
///
/// Своей реализации здесь НЕТ (правило #12): признак сеанса Wayland один на
/// весь Rexon и живёт в [`crate::wallpaper_backend::is_wayland_session`] —
/// две копии одного признака уже разъезжались (там пробелы по краям
/// срезаются, здесь раньше нет), а от него зависит, окном рисовать обои или
/// отдавать кадры расширению оболочки.
pub fn is_wayland_session(xdg_session_type: &str, wayland_display: &str) -> bool {
    crate::wallpaper_backend::is_wayland_session(xdg_session_type, wayland_display)
}

// ── X11-фолбэк: окно обоев как рабочий стол (EWMH) ──────────────────────

// Прежде окно уводилось в фон двумя `xprop -set` ПОСЛЕ показа. По EWMH так
// работает только половина: тип окна (`_NET_WM_WINDOW_TYPE`) оконный
// менеджер читает при показе, а `_NET_WM_STATE` показанного окна клиенту
// писать нельзя вовсе — менеджер его не перечитывает и вправе затереть.
// Поэтому теперь два шага:
//   * ДО показа — свойства прямо на окне (тип, состояние, «на всех столах»):
//     EWMH разрешает клиенту ставить их окну, которое ещё не показано;
//   * ПОСЛЕ показа — просьбы корневому окну (ClientMessage), как EWMH велит
//     менять состояние показанного окна. Тип окна переписывается повторно:
//     Muffin, Marco и xfwm4 перечитывают его по смене свойства.
// Здесь только расчёт — что и в каком порядке; запрос к X-серверу делает
// хост обоев (`rexon-gui::wallpaper_host::x11_desktop`).

/// Тип окна обоев: рабочий стол — WM кладёт его под все приложения.
pub const X11_DESKTOP_WINDOW_TYPE: &str = "_NET_WM_WINDOW_TYPE_DESKTOP";

/// Состояния окна обоев: без панели задач и переключателя, на всех рабочих
/// столах.
///
/// ⚠️ `_NET_WM_STATE_BELOW` (его ставил прежний `xprop`) окну типа «рабочий
/// стол» не ставится. Marco и Muffin кладут такое окно в слой стола, не
/// глядя на BELOW (`get_standalone_layer`), а xfwm4 4.20 по BELOW переносит
/// окно из слоя стола (`WIN_LAYER_DESKTOP`, 0) ВЫШЕ — в `WIN_LAYER_BELOW`
/// (2; `clientUpdateLayerState`). Там окно обоев закрывало значки
/// nemo-desktop, и ни ConfigureWindow, ни подъём окна значков его не
/// опускали: слой важнее порядка (стенд Nemo, 31.10.2026).
pub const X11_DESKTOP_STATES: [&str; 3] = [
    "_NET_WM_STATE_SKIP_TASKBAR",
    "_NET_WM_STATE_SKIP_PAGER",
    "_NET_WM_STATE_STICKY",
];

/// `_NET_WM_DESKTOP` = «на всех рабочих столах» (EWMH: 0xFFFFFFFF).
pub const NET_WM_DESKTOP_ALL: u32 = 0xFFFF_FFFF;

/// `_NET_WM_STATE_ADD` — действие в `data.l[0]` просьбы `_NET_WM_STATE`.
pub const NET_WM_STATE_ADD: u32 = 1;

/// Источник просьбы — обычное приложение (EWMH: 1; 2 — пейджер).
pub const EWMH_SOURCE_APPLICATION: u32 = 1;

/// Атомы предопределённых типов свойств X11 (`XA_ATOM`, `XA_CARDINAL`).
pub const XA_ATOM: u32 = 4;
pub const XA_CARDINAL: u32 = 6;

/// Маска событий просьбы к корневому окну:
/// `SubstructureRedirectMask | SubstructureNotifyMask` (EWMH, «Root Window
/// Messages»).
pub const ROOT_MESSAGE_EVENT_MASK: u32 = (1 << 20) | (1 << 19);

/// Код события ClientMessage в протоколе X11.
pub const X11_CLIENT_MESSAGE: u8 = 33;

/// Значение свойства окна или слова просьбы: имя атома (его номер узнаётся у
/// сервера) либо число.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11Word {
    Atom(&'static str),
    Num(u32),
}

/// Свойство, которое ставится окну напрямую.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11Property {
    /// Имя свойства (`_NET_WM_WINDOW_TYPE`, …).
    pub name: &'static str,
    /// Тип значения: [`XA_ATOM`] или [`XA_CARDINAL`].
    pub kind: u32,
    /// Значение — 32-битные слова.
    pub values: Vec<X11Word>,
}

/// Просьба корневому окну про окно обоев (EWMH ClientMessage, формат 32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11ClientMessage {
    /// `message_type` события (`_NET_WM_STATE`, `_NET_WM_DESKTOP`).
    pub message_type: &'static str,
    /// `data.l[0..5]`.
    pub data: [X11Word; 5],
}

/// Что сделать с окном обоев на X11 — до показа и после.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11DesktopPlan {
    /// Свойства окну ДО показа (тип первым — WM читает его при показе).
    pub before_map: Vec<X11Property>,
    /// Свойства, которые повторно пишутся показанному окну (только тип:
    /// состояние показанного окна так не меняют).
    pub after_map_properties: Vec<X11Property>,
    /// Просьбы корневому окну для показанного окна.
    pub after_map_messages: Vec<X11ClientMessage>,
}

fn window_type_property() -> X11Property {
    X11Property {
        name: "_NET_WM_WINDOW_TYPE",
        kind: XA_ATOM,
        values: vec![X11Word::Atom(X11_DESKTOP_WINDOW_TYPE)],
    }
}

/// План X11-фолбэка: тип, состояние и «на всех столах» — до показа окна,
/// просьбы `_NET_WM_STATE`/`_NET_WM_DESKTOP` — после.
///
/// В одной просьбе `_NET_WM_STATE` — не больше двух состояний
/// (`data.l[1]`, `data.l[2]`), поэтому три состояния идут двумя
/// просьбами; пустое второе место — 0 (EWMH).
pub fn x11_desktop_plan() -> X11DesktopPlan {
    let [skip_taskbar, skip_pager, sticky] = X11_DESKTOP_STATES;
    let state_add = |first: &'static str, second: Option<&'static str>| X11ClientMessage {
        message_type: "_NET_WM_STATE",
        data: [
            X11Word::Num(NET_WM_STATE_ADD),
            X11Word::Atom(first),
            second.map_or(X11Word::Num(0), X11Word::Atom),
            X11Word::Num(EWMH_SOURCE_APPLICATION),
            X11Word::Num(0),
        ],
    };
    X11DesktopPlan {
        before_map: vec![
            window_type_property(),
            X11Property {
                name: "_NET_WM_STATE",
                kind: XA_ATOM,
                values: X11_DESKTOP_STATES.iter().map(|a| X11Word::Atom(*a)).collect(),
            },
            X11Property {
                name: "_NET_WM_DESKTOP",
                kind: XA_CARDINAL,
                values: vec![X11Word::Num(NET_WM_DESKTOP_ALL)],
            },
        ],
        after_map_properties: vec![window_type_property()],
        after_map_messages: vec![
            state_add(skip_taskbar, Some(skip_pager)),
            state_add(sticky, None),
            X11ClientMessage {
                message_type: "_NET_WM_DESKTOP",
                data: [
                    X11Word::Num(NET_WM_DESKTOP_ALL),
                    X11Word::Num(EWMH_SOURCE_APPLICATION),
                    X11Word::Num(0),
                    X11Word::Num(0),
                    X11Word::Num(0),
                ],
            },
        ],
    }
}

/// Все имена атомов, которые плану нужно узнать у сервера, без повторов, в
/// порядке первого упоминания.
pub fn x11_desktop_plan_atoms(plan: &X11DesktopPlan) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let mut push = |name: &'static str| {
        if !out.contains(&name) {
            out.push(name);
        }
    };
    let props = plan.before_map.iter().chain(&plan.after_map_properties);
    for p in props {
        push(p.name);
        for v in &p.values {
            if let X11Word::Atom(a) = v {
                push(*a);
            }
        }
    }
    for m in &plan.after_map_messages {
        push(m.message_type);
        for v in &m.data {
            if let X11Word::Atom(a) = v {
                push(*a);
            }
        }
    }
    out
}

/// Слова в числа: атомы — через `atom` (номер от сервера; `None` — сервер
/// атом не дал, и слово ставить нельзя).
pub fn x11_resolve_words(
    words: &[X11Word],
    atom: impl Fn(&str) -> Option<u32>,
) -> Option<Vec<u32>> {
    words
        .iter()
        .map(|w| match w {
            X11Word::Num(n) => Some(*n),
            X11Word::Atom(a) => atom(a),
        })
        .collect()
}

/// 32 байта события ClientMessage (формат 32) в порядке байтов машины — так,
/// как его ждёт `xcb_send_event`: код, формат, номер (0, ставит сервер),
/// окно, `message_type`, пять слов `data.l`.
pub fn x11_client_message_bytes(window: u32, message_type: u32, data: [u32; 5]) -> [u8; 32] {
    let mut ev = [0u8; 32];
    ev[0] = X11_CLIENT_MESSAGE;
    ev[1] = 32;
    ev[4..8].copy_from_slice(&window.to_ne_bytes());
    ev[8..12].copy_from_slice(&message_type.to_ne_bytes());
    for (i, word) in data.iter().enumerate() {
        let at = 12 + i * 4;
        ev[at..at + 4].copy_from_slice(&word.to_ne_bytes());
    }
    ev
}

// ── X11: порядок окна обоев и окна значков рабочего стола ─────────────────
//
// Значки стола на X11 рисует файловый менеджер в СВОЁМ окне того же типа
// «рабочий стол»: Caja (MATE), nemo-desktop (Cinnamon), xfdesktop (XFCE).
// Оконный менеджер кладёт все такие окна в один нижний слой и внутри слоя
// упорядочивает по времени показа — кто показан позже, тот выше. Окно
// обоев показывается после значков и закрывает их (MATE 1.28, 31.10.2026:
// окно хоста выше окна Caja, значки не видны).
//
// Куда ставить окно обоев, решает фон окна значков:
//   * сквозной (у окна 32-битная глубина — альфа-канал — И работает
//     композитор): обои — ПОД значками, они просвечивают между значками.
//     Так всегда у nemo-desktop и у Caja с ключом
//     `org.mate.caja.desktop transparent-background`;
//   * непрозрачный (Caja без ключа, xfdesktop, любое окно без композитора):
//     обои — НАД ним, иначе их не видно вовсе; значки на это время скрыты,
//     и человеку об этом говорится при применении
//     (`wallpaper_backend::prepare_desktop_icons`).
// Окно, которое рисует только фон (`csd-background` Cinnamon под XWayland),
// остаётся под обоями в любом случае.
//
// Здесь — только решение по наблюдениям; запросы к серверу делает хост
// (`rexon-gui::wallpaper_host::x11_desktop::keep_order`).

/// `WM_CLASS` окон, которые рисуют только фон стола и лежат под обоями.
pub const X11_BACKGROUND_ONLY_CLASSES: [&str; 1] = ["csd-background"];

/// Корневые свойства, которыми файловые менеджеры называют своё окно стола.
pub const X11_ICONS_WINDOW_ROOT_PROPERTIES: [&str; 2] = ["CAJA_DESKTOP_WINDOW_ID", "NAUTILUS_DESKTOP_WINDOW_ID"];

/// Источник просьбы `_NET_RESTACK_WINDOW` — пейджер: прямое действие,
/// которое менеджеры исполняют без проверок кражи фокуса (EWMH).
pub const EWMH_SOURCE_PAGER: u32 = 2;

/// `stack_mode` ConfigureWindow и `detail` `_NET_RESTACK_WINDOW`.
pub const X11_STACK_ABOVE: u32 = 0;
pub const X11_STACK_BELOW: u32 = 1;

/// Глубина окна с альфа-каналом (визуал ARGB).
pub const X11_ALPHA_DEPTH: u8 = 32;

/// Наблюдения о чужом окне из `_NET_CLIENT_LIST_STACKING`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct X11ClientFacts {
    /// Тип окна — `_NET_WM_WINDOW_TYPE_DESKTOP`.
    pub desktop_type: bool,
    /// Корневое окно называет его окном стола файлового менеджера
    /// ([`X11_ICONS_WINDOW_ROOT_PROPERTIES`]).
    pub announced_icons: bool,
    /// `WM_CLASS`: имя экземпляра и класс.
    pub instance: String,
    pub class: String,
    /// Глубина окна (бит на пиксель); 0 — не узнали.
    pub depth: u8,
}

/// Чем окно приходится обоям.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11StackRole {
    /// Наше окно обоев.
    Own,
    /// Окно значков стола; `see_through` — его фон сквозной.
    Icons { see_through: bool },
    /// Окно, которое рисует только фон стола.
    Background,
    /// Любое другое окно — на порядок обоев не влияет.
    Other,
}

/// Роль чужого окна. `compositing` — у экрана есть композитор (владелец
/// выбора `_NET_WM_CM_Sn`): без него альфа окна не смешивается, и окно с
/// 32-битной глубиной всё равно закрывает то, что под ним.
pub fn x11_stack_role(facts: &X11ClientFacts, compositing: bool) -> X11StackRole {
    let named = |names: &[&str]| {
        names
            .iter()
            .any(|n| facts.instance.eq_ignore_ascii_case(n) || facts.class.eq_ignore_ascii_case(n))
    };
    if !facts.announced_icons && !facts.desktop_type {
        return X11StackRole::Other;
    }
    if !facts.announced_icons && named(&X11_BACKGROUND_ONLY_CLASSES) {
        return X11StackRole::Background;
    }
    X11StackRole::Icons { see_through: compositing && facts.depth == X11_ALPHA_DEPTH }
}

/// Куда переставить окно обоев относительно соседа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11Restack {
    Below(u32),
    Above(u32),
}

impl X11Restack {
    /// Сосед и `stack_mode` для запроса.
    pub fn sibling_and_mode(self) -> (u32, u32) {
        match self {
            X11Restack::Below(w) => (w, X11_STACK_BELOW),
            X11Restack::Above(w) => (w, X11_STACK_ABOVE),
        }
    }
}

/// Нужно ли переставить окно обоев `own` в стеке `stack` (снизу вверх, как
/// `_NET_CLIENT_LIST_STACKING`), и куда. `None` — стоит где надо.
///
/// * Есть хоть одно непрозрачное окно значков — обои над самым верхним
///   окном значков (иначе их не видно).
/// * Все окна значков сквозные — обои под самым нижним из них.
/// * Окна, рисующие только фон, всегда остаются под обоями.
///
/// Повторный расчёт после исполненного запроса даёт `None`: на каждом
/// круге сверки переставляется только то, что стоит не на месте.
pub fn x11_restack_goal(stack: &[(u32, X11StackRole)], own: u32) -> Option<X11Restack> {
    let own_at = stack.iter().position(|(w, role)| *w == own && *role == X11StackRole::Own)?;
    let icons: Vec<(usize, bool)> = stack
        .iter()
        .enumerate()
        .filter_map(|(i, (_, role))| match role {
            X11StackRole::Icons { see_through } => Some((i, *see_through)),
            _ => None,
        })
        .collect();
    let background_top = stack.iter().rposition(|(_, role)| *role == X11StackRole::Background);
    if icons.is_empty() {
        return match background_top {
            Some(bg) if own_at < bg => Some(X11Restack::Above(stack[bg].0)),
            _ => None,
        };
    }
    if icons.iter().any(|(_, see_through)| !see_through) {
        let top = icons.iter().map(|(i, _)| *i).max().unwrap_or(0);
        return (own_at < top).then(|| X11Restack::Above(stack[top].0));
    }
    let lowest = icons.iter().map(|(i, _)| *i).min().unwrap_or(0);
    if own_at > lowest {
        return Some(X11Restack::Below(stack[lowest].0));
    }
    match background_top {
        Some(bg) if own_at < bg => Some(X11Restack::Above(stack[bg].0)),
        _ => None,
    }
}

/// Сколько раз подряд просить оконный менеджер о том же порядке при том же
/// стеке, прежде чем признать отказ. Менеджер исполняет просьбу за
/// миллисекунды, а сверка ходит раз в 2 с — три круга с запасом покрывают
/// медленный старт.
pub const X11_RESTACK_TRIES: u32 = 3;

/// Что хосту делать на этом круге сверки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X11RestackStep {
    /// Попросить менеджер о порядке.
    Ask(X11Restack),
    /// Просьбы не исполнены [`X11_RESTACK_TRIES`] раз — один раз сказать
    /// это в журнал и больше не просить.
    GiveUp(X11Restack),
    /// Отказ уже признан, стек тот же — молчать.
    Quiet,
}

/// Учёт просьб о порядке одного окна обоев между кругами сверки.
///
/// Оконный менеджер вправе просьбу молча не исполнить: окно в другом слое
/// (xfwm4 с BELOW держал окно обоев над слоем стола), политика менеджера.
/// Тогда сверка раз в 2 с просила бы одно и то же бесконечно и столько же
/// писала в журнал. Отказом считается та же цель при том же стеке после
/// [`X11_RESTACK_TRIES`] просьб; любое изменение нижнего слоя (окно значков
/// пересоздано, менеджер сменился, цель стала другой) начинает счёт заново.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct X11RestackTracker {
    /// Цель и нижний слой (окна не `Other`, снизу вверх) последней просьбы.
    last: Option<(X11Restack, Vec<(u32, X11StackRole)>)>,
    tries: u32,
    /// Окно значков уже пересоздавали ради этого окна обоев.
    icons_recreated: bool,
}

impl X11RestackTracker {
    /// Шаг сверки для окна `own` в стеке `stack` (снизу вверх). `None` —
    /// окно стоит где надо, просить нечего.
    pub fn step(&mut self, stack: &[(u32, X11StackRole)], own: u32) -> Option<X11RestackStep> {
        let Some(goal) = x11_restack_goal(stack, own) else {
            self.last = None;
            self.tries = 0;
            return None;
        };
        let layer: Vec<(u32, X11StackRole)> =
            stack.iter().copied().filter(|(_, role)| *role != X11StackRole::Other).collect();
        let same = matches!(&self.last, Some((g, l)) if *g == goal && *l == layer);
        if !same {
            self.last = Some((goal, layer));
            self.tries = 0;
        }
        self.tries = self.tries.saturating_add(1);
        Some(match self.tries {
            n if n <= X11_RESTACK_TRIES => X11RestackStep::Ask(goal),
            n if n == X11_RESTACK_TRIES + 1 => X11RestackStep::GiveUp(goal),
            _ => X11RestackStep::Quiet,
        })
    }

    /// После [`X11RestackStep::GiveUp`]: пересоздать ли окно значков.
    ///
    /// xfwm4 окно обоев под окно значков не опускает (просьбы о порядке
    /// окну типа «рабочий стол» он отбрасывает), но новое окно значков
    /// кладёт поверх обоев. Значит, сквозные значки, показанные раньше
    /// хоста, достаются пересозданием их окна (как это сделать — решает
    /// `wallpaper_backend::icons_window_toggle` по классу окна). Только
    /// для цели «под значки» (над непрозрачными значками новое окно не
    /// поможет) и один раз на окно обоев: не помогло — больше не дёргаем
    /// значки по кругу.
    pub fn take_icons_recreate(&mut self, goal: X11Restack) -> bool {
        if !matches!(goal, X11Restack::Below(_)) || self.icons_recreated {
            return false;
        }
        self.icons_recreated = true;
        true
    }
}

/// Номер экрана X из `DISPLAY` (`:0.1` → 1); без номера — 0. Нужен для
/// имени выбора композитора `_NET_WM_CM_S<номер>`.
pub fn x11_screen_number(display: &str) -> u32 {
    let after_colon = display.rsplit_once(':').map(|(_, rest)| rest).unwrap_or("");
    after_colon
        .split_once('.')
        .and_then(|(_, screen)| screen.parse().ok())
        .unwrap_or(0)
}

/// Имя выбора, которым владеет композитор экрана (EWMH).
pub fn x11_compositor_selection(screen: u32) -> String {
    format!("_NET_WM_CM_S{screen}")
}

// ── headless render-loop ───────────────────────────────────────────────────

/// Интервал тика QTimer (мс) — порт `max(8, int(1000 / max(fps, 1)))` из
/// `run_headless_loop`. `fps <= 0` → 1000 мс; верх ограничен ~125 Гц (8 мс).
pub fn headless_tick_ms(fps: i64) -> i64 {
    (1000 / fps.max(1)).max(8)
}

/// Частота кадров mmap-потока из значения `REXON_WP_FPS` — порт `_env_fps`
/// из `video_wallpaper.py`.
///
/// Переменную выставляет расширение оболочки. Мусор в ней не должен
/// оставлять человека без обоев, поэтому непонятое значение и значение вне
/// [`MIN_FPS`]..=[`MAX_FPS`] откатываются к [`DEFAULT_FPS`].
///
/// Живёт здесь, а не в `wallpaper.rs`: переменная описывает именно
/// mmap-поток кадров, рядом с [`headless_tick_ms`], который из неё считает
/// интервал таймера.
pub fn env_fps(raw: Option<&str>) -> i64 {
    let Some(v) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return DEFAULT_FPS;
    };
    match v.parse::<i64>() {
        Ok(fps) if (MIN_FPS..=MAX_FPS).contains(&fps) => fps,
        _ => DEFAULT_FPS,
    }
}

// ── платформа Qt хоста в режиме потока кадров ─────────────────────────────

/// Платформа Qt без окон и без видеокарты: на ней хост живёт, пока отдаёт
/// кадры расширению, если кадр не рисует `QQuickRenderControl`.
pub const OFFSCREEN_QPA_PLATFORM: &str = "offscreen";
/// Платформа Qt сеанса Wayland (GNOME/Cinnamon-Wayland — `WAYLAND_DISPLAY`).
pub const WAYLAND_QPA_PLATFORM: &str = "wayland";
/// Платформа Qt сеанса X11 или XWayland (`DISPLAY`).
pub const XCB_QPA_PLATFORM: &str = "xcb";

/// Чем хост снимает кадр в режиме потока кадров.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamCapture {
    /// Кадр берётся не из сцены Qt Quick: у видео — из `QVideoSink`
    /// (VideoOutput на `offscreen` не рисует), у Flash — wgpu пишет сам. Рендер
    /// Qt не нужен, платформа остаётся `offscreen` — там окна нет по устройству.
    OwnFrames,
    /// Сцена Qt Quick рисуется `QQuickRenderControl` в текстуру и читается
    /// обратно (веб). Окно QML не показывается НИКОГДА, поэтому платформе
    /// можно быть настоящей — и получить видеокарту.
    RenderControl,
}

/// Чем режим хоста снимает кадр в режиме потока кадров — ОДНА таблица.
pub fn host_stream_capture(mode: crate::wallpaper_engine::HostMode) -> StreamCapture {
    use crate::wallpaper_engine::HostMode;
    match mode {
        HostMode::Web => StreamCapture::RenderControl,
        // Клиент вывода в поток кадров не пишет никогда; ответ — ради
        // полноты таблицы.
        HostMode::Video | HostMode::Swf | HostMode::Waywallen => StreamCapture::OwnFrames,
    }
}

/// Что записать в `QT_QPA_PLATFORM` хосту обоев ДО создания приложения Qt.
/// `None` — ничего не трогать.
///
/// ОДНО решение на все режимы хоста, правило #12. Прежде его принимал только
/// веб-хост, и видео-хост на GNOME/Cinnamon-Wayland поднимал обычное окно:
/// замер под Wayland-композитором без layer-shell — клиент просит
/// `xdg_surface.get_toplevel` с заголовком «RexonVideoWallpaper» и отдаёт ему
/// кадры, под Xvfb — видимое окно 1920×1080 (`IsViewable`). На Mutter это
/// чёрное окно во весь экран поверх рабочего стола.
///
/// Веб (`StreamCapture::RenderControl`) — на НАСТОЯЩЕЙ платформе сеанса
/// (15.09.2026). На `offscreen` у Qt нет EGL, а GLX только через `DISPLAY`
/// (qtbase `plugins/platforms/offscreen`), и даже с ним QtWebEngine 6.11 на
/// `offscreen` при GPU-композиторе не кладёт кадр в сцену вовсе
/// (`render_widget_host_view_qt_delegate_item.cpp`, QTBUG-132682). Замер на
/// стенде (1920×1080, поток 30 к/с): `offscreen` — CSS 17 к/с при 157 % CPU,
/// WebGL нет; `wayland` + `QQuickRenderControl` — видеокарта (ANGLE, Radeon
/// 780M), CSS 31 к/с при 43 % CPU, WebGL 30 к/с при 25 % с живым кадром; `xcb`
/// — 38 и 26 %. Окна на экране при этом нет: за весь прогон клиент не создал
/// ни одной `wl_surface` (0 `create_surface`, 0 `get_toplevel`). Без
/// `WAYLAND_DISPLAY` и `DISPLAY` — `offscreen`, программный рендер.
///
/// ⚠️ Уже заданное значение ПЕРЕБИВАЕТСЯ, а не уважается: в режиме потока
/// кадров окна на экране быть не должно ни при какой платформе, а
/// `QT_QPA_PLATFORM=wayland` у пользователей GNOME нередко стоит на весь
/// сеанс (так советуют ради Qt-программ) — видео унаследовало бы его от
/// оболочки и снова вывело бы окно поверх стола.
pub fn host_qpa_platform(
    stream_to_file: bool,
    capture: StreamCapture,
    current: Option<&str>,
    wayland_display: &str,
    x_display: &str,
) -> Option<&'static str> {
    if !stream_to_file {
        return None;
    }
    let wanted = stream_qpa_platform(capture, wayland_display, x_display);
    if current == Some(wanted) {
        None
    } else {
        Some(wanted)
    }
}

/// Можно ли в режиме потока показать окно ради снимка кадра. Только на
/// `offscreen`: там у окна нет экрана по устройству. На любой настоящей
/// платформе показанное окно на Mutter выходит поверх рабочего стола —
/// это дефект, ради которого поток кадров вообще живёт без окна.
pub fn stream_window_may_show(platform: &str) -> bool {
    platform == OFFSCREEN_QPA_PLATFORM
}

/// Платформа Qt режима потока кадров — сама по себе, без сверки с текущей.
pub fn stream_qpa_platform(
    capture: StreamCapture,
    wayland_display: &str,
    x_display: &str,
) -> &'static str {
    match capture {
        StreamCapture::OwnFrames => OFFSCREEN_QPA_PLATFORM,
        StreamCapture::RenderControl if !wayland_display.trim().is_empty() => {
            WAYLAND_QPA_PLATFORM
        }
        StreamCapture::RenderControl if !x_display.trim().is_empty() => XCB_QPA_PLATFORM,
        StreamCapture::RenderControl => OFFSCREEN_QPA_PLATFORM,
    }
}

// ── счётчик поколения кадра ────────────────────────────────────────────────

/// Окончание имени файла-счётчика рядом с файлом кадра
/// (`rexon-wp-frame.rgba` → `rexon-wp-frame.rgba.gen`). Та же строка стоит в
/// общем блоке чтения обоих `extension.js` (сторож —
/// `tests/wallpaper_mmap_parity.rs`).
pub const FRAME_GEN_SUFFIX: &str = ".gen";

/// Размер счётчика: u64 little-endian, больше в файле ничего нет.
pub const FRAME_GEN_BYTES: usize = 8;

/// Путь счётчика поколения для файла кадра.
///
/// Зачем он. Хост пишет кадр под `flock(LOCK_EX)`, а расширение оболочки
/// замок взять не может: у GJS нет ни `flock`, ни `fcntl`. Замер на
/// 1920×1080 и 30 кадрах: хост держит замок 6 % времени (запись кадра
/// ~1,9 мс), копия у читателя ~1,7 мс — и 12 рваных кадров из 180 тиков.
///
/// Договор (seqlock): перед записью кадра хост кладёт в счётчик НЕЧЁТНОЕ
/// число, после — следующее чётное. Читатель берёт счётчик до копии кадра и
/// после; нечётный или изменившийся — кадр пропускается (на экране остаётся
/// прежний). Раскладка самого файла кадра НЕ меняется: ровно
/// width×height×4 байта без заголовка, и расширение прежней сборки читает
/// его как раньше. Нет счётчика (хост прежней сборки) — читатель ведёт себя
/// по-старому.
pub fn frame_generation_path(frame_path: &str) -> String {
    format!("{frame_path}{FRAME_GEN_SUFFIX}")
}

/// Байты счётчика поколения — u64 little-endian.
pub fn frame_generation_bytes(generation: u64) -> [u8; FRAME_GEN_BYTES] {
    generation.to_le_bytes()
}

// ── внутреннее ─────────────────────────────────────────────────────────────

/// Семантика Python `int(str)` для ENV-значений: обрезка пробелов, знак,
/// только целые. (Подчёркивания-разделители Python 3.6+ не поддержаны —
/// в ENV-размерах экрана не встречаются.)
fn py_int(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<i64>().ok()
}
