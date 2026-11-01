//! Кто рисует живые обои в текущей сессии — единое решение на весь Rexon.
//!
//! До этого модуля знание было размазано: `_is_kde()` в
//! `waywallen_kde_plugin`, `is_wayland_session()` в `wallpaper_mmap`,
//! ветки `if self.de == "kde"` в старом движке обоев. Из-за этого
//! появлялись пути, дерущиеся за один и тот же экран. Здесь — одна
//! функция [`pick`], которой пользуются все (правило #12).
//!
//! Решение зеркалит выбор БЭКЕНДА ВЫВОДА в самом демоне waywallen
//! (с 0.5.0: определения — `src/plugin/display_registry.rs`,
//! `builtin_display_defs`; выбор — `src/wallframe/display/spawner.rs`,
//! `detect_de` + `pick_backend`; сверка 24.10.2026) — знать его нужно
//! потому, что от режима запуска зависит, обязан ли Rexon что-то делать:
//!
//! * `kde-plasma` → `SpawnMode::External`, приоритет 100, жёсткое
//!   правило «KDE всегда идёт сюда и НИКОГДА не откатывается».
//!   External = демон плагин не запускает, его ставит и выбирает
//!   кто-то снаружи. Снаружи — это мы: Rexon ставит kpackage
//!   `org.waywallen.kde` через `kpackagetool6` и выбирает его на
//!   десктоп-контейнментах.
//! * `layer-shell` → `SpawnMode::Daemon`, приоритет 50, список
//!   рабочих столов `hyprland, sway, niri, river, cosmic, wayfire`,
//!   требования `wlr-layer-shell` + `linux-dmabuf-v4`. Daemon = демон САМ находит
//!   рядом с собой бинарь `waywallen-layer-shell` и запускает его.
//!   **Rexon тут не делает ничего** — достаточно того, что демон
//!   поднят. Своего пути для этих сессий нам не нужно.
//!
//! Отсюда главный вывод, ради которого модуль и написан: наш
//! собственный subprocess (хост `rexon-wallpaper-host` на LayerShellQt)
//! на wlr-композиторах — ЛИШНИЙ, он дублирует штатный
//! `waywallen-layer-shell` и дерётся с ним за поверхность. Его
//! настоящая ниша — X11. Сам демон от X11 не зависит (он рисует в
//! dma-buf и раздаёт кадры клиентам вывода); на X11 нет только КЛИЕНТА
//! вывода: `waywallen-layer-shell` — это Wayland. Исключение — Plasma на
//! X11: плагин `org.waywallen.kde` работает и там (plasmashell нужен
//! `QT_XCB_GL_INTEGRATION=xcb_egl`, README waywallen-display), и жёсткое
//! правило KDE в демоне тип сеанса не смотрит. [`pick`] пока отправляет
//! любой X11 к нашему процессу, KDE тоже: перевод Plasma-X11 на плагин —
//! отдельное решение (настройка plasmashell), здесь не принято.
//!
//! ⚠️ `river` есть в списке демона, но отсутствует в таблице поддержки
//! их README — берём как есть из кода, код честнее таблицы.
//!
//! ⚠️ Wayfire сам `XDG_CURRENT_DESKTOP` не выставляет, поэтому демон узнаёт
//! его ещё и по сокету: если среди токенов `XDG_CURRENT_DESKTOP` нет ничего,
//! кроме `wayland` (или переменная пуста), а `WAYFIRE_SOCKET` непуст, к
//! токенам дописывается `wayfire` (`desktop_tokens` в spawner.rs). То же
//! правило — в [`session_desktops`].
//!
//! Не зеркалим сознательно одно: у демона 0.5.0 есть ещё определение
//! `gnome-shell` (`SpawnMode::External`) — его ставит расширение оболочки
//! от самого waywallen. У Rexon на GNOME/Cinnamon своё расширение
//! (`ShellExtension` ниже), и выбор демона на это решение не влияет.

/// Кто именно показывает живые обои.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallpaperBackend {
    /// KDE Plasma: Plasma-плагин `org.waywallen.kde` внутри containment.
    /// Значки рабочего стола и клики остаются на месте.
    /// **Rexon обязан** поставить плагин и выбрать его.
    KdePlugin,
    /// wlr-композиторы: демон сам поднимает `waywallen-layer-shell`.
    /// **Rexon не делает ничего** сверх запуска демона.
    DaemonLayerShell,
    /// GNOME/Cinnamon на Wayland: layer-shell нет (Mutter и его форк
    /// Muffin его не реализуют), поэтому кадры уходят расширению
    /// оболочки через mmap-файл.
    ShellExtension,
    /// X11: своего клиента вывода у waywallen здесь нет. Видео, веб и
    /// Flash рисует наш хост; сцены Wallpaper Engine рисует демон, а на
    /// стол их выводит тот же хост в режиме клиента вывода
    /// ([`DisplaySetup::RexonHost`]). Окна уводятся в фон через
    /// `_NET_WM_WINDOW_TYPE_DESKTOP`.
    X11Subprocess,
    /// Сессия Wayland на композиторе, которого демон не знает.
    /// Демон уйдёт в headless, обоев не будет.
    Unsupported,
}

/// Рабочие столы, на которых демон поднимает layer-shell сам
/// (`builtin_display_defs`, поле `de` определения `layer-shell`).
pub const DAEMON_LAYER_SHELL_DESKTOPS: &[&str] =
    &["hyprland", "sway", "niri", "river", "cosmic", "wayfire"];

/// Оболочки, куда кадры отдаются расширением через mmap.
///
/// Это СОБСТВЕННЫЙ путь Rexon (наш `rexon-wallpaper-host` + расширение
/// `rexon-wallpaper@rexon.app` для GNOME и Cinnamon; Unity — тот же
/// Mutter-род), демон waywallen его не знает и в выборе не участвует.
/// Сходство с определением `gnome-shell` демона — только в имени.
pub const SHELL_EXTENSION_DESKTOPS: &[&str] = &["gnome", "cinnamon", "unity"];

impl WallpaperBackend {
    /// Требуется ли от Rexon действие, чтобы обои поехали.
    ///
    /// Только KDE: там демон объявляет `spawn=External` и ждёт, что
    /// плагин поставит и выберет кто-то снаружи.
    pub fn needs_rexon_setup(self) -> bool {
        self == WallpaperBackend::KdePlugin
    }

    /// Рисует ли обои сам движок waywallen (а не наш код).
    pub fn is_waywallen(self) -> bool {
        matches!(
            self,
            WallpaperBackend::KdePlugin | WallpaperBackend::DaemonLayerShell
        )
    }

    /// Объяснение для пользователя — по-человечески, без жаргона
    /// (правило #13: покупателю не показываем имена бинарей и протоколов).
    pub fn human_note(self) -> &'static str {
        match self {
            WallpaperBackend::KdePlugin => {
                "Живые обои встраиваются в рабочий стол Plasma — значки и \
                 щелчки по столу работают как обычно."
            }
            WallpaperBackend::DaemonLayerShell => {
                "Живые обои показывает движок обоев — отдельной настройки \
                 не требуется."
            }
            WallpaperBackend::ShellExtension => {
                "В этой оболочке обои показывает расширение Rexon. Если \
                 обои не появились, включите расширение и перезайдите в \
                 систему."
            }
            WallpaperBackend::X11Subprocess => {
                "Сеанс X11: живые обои показывает собственный проигрыватель \
                 Rexon. Часть возможностей движка обоев здесь недоступна."
            }
            WallpaperBackend::Unsupported => {
                "Оконный менеджер этого сеанса не поддерживает живые обои. \
                 Статичную картинку поставить можно."
            }
        }
    }
}

/// Признак сеанса Wayland: `XDG_SESSION_TYPE=wayland` ЛИБО непустой
/// `WAYLAND_DISPLAY` (второе спасает там, где session_type не выставлен —
/// вложенные сеансы, запуск из TTY).
pub fn is_wayland_session(session_type: &str, wayland_display: &str) -> bool {
    session_type.trim().eq_ignore_ascii_case("wayland") || !wayland_display.trim().is_empty()
}

/// Есть ли токен среди значений `XDG_CURRENT_DESKTOP` (он
/// двоеточие-разделённый и регистр не гарантирует: «KDE», «Hyprland»,
/// «pop:GNOME»).
fn has_desktop(xdg_current_desktop: &str, wanted: &[&str]) -> bool {
    xdg_current_desktop.split(':').any(|token| {
        let t = token.trim();
        wanted.iter().any(|w| t.eq_ignore_ascii_case(w))
    })
}

/// Рабочие столы сеанса так, как их видит демон: `XDG_CURRENT_DESKTOP` и,
/// если он ничего не говорит (пуст или только `wayland`), а `WAYFIRE_SOCKET`
/// непуст, — `wayfire`. Wayfire эту переменную сам не выставляет.
pub fn session_desktops(xdg_current_desktop: &str, wayfire_socket: &str) -> String {
    let silent = xdg_current_desktop
        .split(':')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .all(|t| t.eq_ignore_ascii_case("wayland"));
    if silent && !wayfire_socket.is_empty() {
        if xdg_current_desktop.trim().is_empty() {
            "wayfire".to_string()
        } else {
            format!("{xdg_current_desktop}:wayfire")
        }
    } else {
        xdg_current_desktop.to_string()
    }
}

/// Выбрать бэкенд обоев для сессии.
///
/// Порядок проверок повторяет демон: сперва отсекается X11 (waywallen
/// туда не умеет), затем жёсткое правило KDE, затем список
/// wlr-композиторов, затем оболочки с расширением. `wayfire_socket` —
/// значение `WAYFIRE_SOCKET` (пусто, если не выставлена), см.
/// [`session_desktops`].
pub fn pick(
    xdg_current_desktop: &str,
    session_type: &str,
    wayland_display: &str,
    wayfire_socket: &str,
) -> WallpaperBackend {
    if !is_wayland_session(session_type, wayland_display) {
        return WallpaperBackend::X11Subprocess;
    }
    let desktops = session_desktops(xdg_current_desktop, wayfire_socket);
    let xdg_current_desktop = desktops.as_str();
    // Жёсткое правило демона: KDE всегда идёт своим путём и никогда не
    // откатывается на layer-shell (иначе окно перекрыло бы containment
    // и пропали бы значки рабочего стола). Признак — ровно как у демона
    // (`DeCaps::is_kde`): токен `kde` и только он. Прежний второй токен
    // `plasma` демон не узнаёт: на таком сеансе он не выбрал бы плагин, и
    // наша установка плагина ничего бы не дала. Plasma сама выставляет
    // `XDG_CURRENT_DESKTOP=KDE`.
    if has_desktop(xdg_current_desktop, &["kde"]) {
        return WallpaperBackend::KdePlugin;
    }
    if has_desktop(xdg_current_desktop, DAEMON_LAYER_SHELL_DESKTOPS) {
        return WallpaperBackend::DaemonLayerShell;
    }
    if has_desktop(xdg_current_desktop, SHELL_EXTENSION_DESKTOPS) {
        return WallpaperBackend::ShellExtension;
    }
    WallpaperBackend::Unsupported
}

// ── дисплей для демона waywallen, когда его ещё нет ─────────────────────
//
// ⛔ Дефект 30.10.2026 (MATE на X11, r1898): на KDE круг «кнопка
// „Применить“ ждёт дисплей — дисплей появляется после применения» Rexon
// разрывает сам (назначает Plasma-плагин, `waywallen_kde_plugin::apply_gate`),
// а на X11 вне KDE у демона нет клиента вывода вовсе: демон пишет «no
// display backend matched xdg_desktop=["mate"]», дисплеев ноль, кнопка
// серая, страница «Мониторы» молчит. Сцены Wallpaper Engine рисует демон, а
// не наш хост, так что «X11 — собственный проигрыватель Rexon» к ним не
// относился.
//
// Выход без правки демона: клиентом вывода становится хост обоев Rexon в
// режиме `--mode waywallen`. Он грузит элемент вывода waywallen из того же
// пакета `org.waywallen.kde`, что Rexon уже везёт для Plasma (библиотека
// waywallen-display, MIT, только Qt6; на X11 — через EGL), и кладёт по
// окну-рабочему столу на монитор. Проверено на Xvfb 30.10.2026: дисплей
// регистрируется, сцена идёт ~30 к/с.
//
// Кто его поднимает:
// * демон — при каждом своём старте, по определению вывода в
//   `$XDG_DATA_HOME/waywallen/displays/` ([`rexon_display_manifest`]);
//   демон же перезапускает упавший хост;
// * Rexon — сразу по «Применить», если дисплеев нет: демон выбирает вывод
//   только на старте, а перезапуск демона оборвал бы само применение.
// Второй экземпляр хоста ждёт блокировку ([`rexon_host_lock_path`]) и
// подхватывает вывод, если первый умрёт, — двух окон на монитор не бывает.

/// Как у демона появляется дисплей, когда его ещё нет.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplaySetup {
    /// Rexon тут ничего не поднимает: дисплей даёт сам демон (layer-shell)
    /// или вывода на этом столе нет.
    None,
    /// KDE: Rexon назначает Plasma-плагин обоями стола.
    KdePlugin,
    /// X11: клиент вывода — хост обоев Rexon (`--mode waywallen`).
    RexonHost,
}

impl WallpaperBackend {
    /// Что Rexon делает, чтобы у демона появился дисплей.
    pub fn display_setup(self) -> DisplaySetup {
        match self {
            WallpaperBackend::KdePlugin => DisplaySetup::KdePlugin,
            WallpaperBackend::X11Subprocess => DisplaySetup::RexonHost,
            WallpaperBackend::DaemonLayerShell
            | WallpaperBackend::ShellExtension
            | WallpaperBackend::Unsupported => DisplaySetup::None,
        }
    }
}

impl DisplaySetup {
    /// Поднимает ли Rexon дисплей сам (тогда «Применить» доступна и без
    /// дисплеев, а кадр применения ждёт дисплея).
    pub fn rexon_brings_display(self) -> bool {
        self != DisplaySetup::None
    }
}

/// Имя определения вывода у демона (поле `name`, видно в его журнале и в
/// `display_backend_status`).
pub const REXON_DISPLAY_NAME: &str = "rexon-x11";

/// Режим хоста обоев, в котором он — клиент вывода демона.
pub const REXON_DISPLAY_HOST_ARGS: &[&str] = &["--mode", "waywallen"];

/// Рабочие столы (токены `XDG_CURRENT_DESKTOP`, без регистра), на которых
/// демон поднимает наш хост. Только столы без собственного вывода waywallen:
/// `kde` у демона под жёстким правилом, `gnome` — у его расширения, wlr —
/// у `waywallen-layer-shell`. Часть этих столов бывает и на Wayland (XFCE и
/// LXQt на labwc, Cinnamon Wayland) — там хост ничего не показывает и только
/// ждёт ([`rexon_host_should_idle`]), чтобы демон не перезапускал его по
/// кругу.
pub const REXON_DISPLAY_DESKTOPS: &[&str] = &[
    "mate",
    "xfce",
    "x-cinnamon",
    "cinnamon",
    "budgie",
    "budgie-desktop",
    "lxqt",
    "lxde",
    "pantheon",
    "deepin",
    "unity",
    "enlightenment",
];

/// Приоритет определения: выше встроенного `x11` (50; демон с
/// waywallen#354 сам поднимает `waywallen-x11` на MATE, Cinnamon и Xfce).
/// Иначе на этих столах демон выберет свой вывод: без бинаря рядом обоев
/// нет вовсе, с бинарем его окно встаёт под непрозрачную Caja. Списки
/// столов с `layer-shell`, `kde` и `gnome` не пересекаются — у них 60
/// ничего не отнимает.
pub const REXON_DISPLAY_PRIORITY: i32 = 60;

/// Где лежит определение вывода для демона: пользовательский каталог
/// `displays` демона (`$XDG_DATA_HOME/waywallen/displays`). Его читает
/// любой демон — AppImage, вшитый в Rexon, и пакет дистрибутива.
pub fn rexon_display_manifest_path(xdg_data_home: &str) -> String {
    format!("{}/waywallen/displays/{REXON_DISPLAY_NAME}.toml", xdg_data_home.trim_end_matches('/'))
}

/// Экранировать строку для TOML (базовая строка в кавычках).
fn toml_str(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Текст определения вывода (`DisplayManifest` демона: секция `[display]`).
/// Демон запускает `bin --socket <путь> <extra_args>` и следит за ним сам.
pub fn rexon_display_manifest(host_bin: &str) -> String {
    let list = |items: &[&str]| items.iter().map(|s| toml_str(s)).collect::<Vec<_>>().join(", ");
    format!(
        "# Rexon: живые обои на рабочих столах X11, где у waywallen нет своего\n\
         # вывода. Файл пишет Rexon сам; правки руками он перезапишет.\n\
         [display]\n\
         name = {}\n\
         bin = {}\n\
         extra_args = [{}]\n\
         de = [{}]\n\
         priority = {REXON_DISPLAY_PRIORITY}\n\
         spawn = \"daemon\"\n",
        toml_str(REXON_DISPLAY_NAME),
        toml_str(host_bin),
        list(REXON_DISPLAY_HOST_ARGS),
        list(REXON_DISPLAY_DESKTOPS),
    )
}

/// Переписывать ли определение: нет файла или он не тот (другой путь к
/// хосту после переустановки, старый формат).
pub fn rexon_display_manifest_stale(current: Option<&str>, host_bin: &str) -> bool {
    current != Some(rexon_display_manifest(host_bin).as_str())
}

/// Хост обоев рядом с запущенным бинарём Rexon (дерево разработчика:
/// `target*/release/`), иначе — поиск по `PATH` у исполнителя (пакет кладёт
/// хост в `/usr/bin`).
pub fn rexon_display_host_sibling(self_exe: &str) -> Option<String> {
    let dir = std::path::Path::new(self_exe).parent()?;
    Some(dir.join(crate::wallpaper_engine::WALLPAPER_HOST_BINARY).to_string_lossy().into_owned())
}

/// Строка, которой Rexon запускает хост-клиент вывода сам, — та же, что у
/// демона по определению вывода (`bin --socket <путь> <extra_args>`): один
/// разбор ключей у хоста на оба пути.
pub fn rexon_display_host_argv(host_bin: &str, socket: &str) -> Vec<String> {
    let mut argv = vec![host_bin.to_string()];
    if !socket.is_empty() {
        argv.push("--socket".to_string());
        argv.push(socket.to_string());
    }
    argv.extend(REXON_DISPLAY_HOST_ARGS.iter().map(|s| s.to_string()));
    argv
}

/// Блокировка «вывод уже показывает другой экземпляр хоста».
pub fn rexon_host_lock_path(xdg_runtime_dir: &str) -> String {
    format!("{}/rexon-waywallen-display.lock", xdg_runtime_dir.trim_end_matches('/'))
}

/// Должен ли хост-клиент вывода только ждать, ничего не показывая.
///
/// Да на Wayland: окна X11 там легли бы поверх стола обычными окнами
/// (через XWayland), а слой фона — забота других выводов. Нет `DISPLAY` —
/// окну негде быть вовсе.
pub fn rexon_host_should_idle(session_type: &str, wayland_display: &str, display: &str) -> bool {
    is_wayland_session(session_type, wayland_display) || display.trim().is_empty()
}

/// Имя бэкенда для журнала и для моста (стабильные строки, не для UI).
pub fn backend_id(backend: WallpaperBackend) -> &'static str {
    match backend {
        WallpaperBackend::KdePlugin => "kde-plugin",
        WallpaperBackend::DaemonLayerShell => "daemon-layer-shell",
        WallpaperBackend::ShellExtension => "shell-extension",
        WallpaperBackend::X11Subprocess => "x11-subprocess",
        WallpaperBackend::Unsupported => "unsupported",
    }
}

// ── значки рабочего стола под живыми обоями (X11) ───────────────────────
//
// Дефект 31.10.2026 (MATE 1.28, X11): «Применить» живых обоев работает, но
// окно хоста встаёт над окном значков Caja — значков не видно. Порядок окон
// решает хост сам (`wallpaper_mmap::x11_restack_goal`): под значками — когда
// фон окна значков сквозной, над ними — когда непрозрачный. Здесь — что
// сделать со столом при применении и что вернуть потом:
//
// * Caja с ключом `org.mate.caja.desktop transparent-background` (наш
//   mate-desktop/caja#1916; есть ли ключ — спрашиваем у схемы, а не по
//   версии): ключ включается, композитор Marco — тоже, если выключен (без
//   него альфа окна не смешивается), окно стола пересоздаётся (ключ Caja
//   читает при создании окна). Прежние значения записываются и
//   возвращаются кнопкой «Остановить живые обои» в настройках и откатом
//   неудачного применения сцены. Остановка сцены во вкладке Wallpaper
//   Engine ключ не трогает: без окна обоев под ним сквозной стол Caja
//   показывает обычную картинку стола (корневое окно), проверено на стенде.
// * Caja без ключа и xfdesktop фон рисуют непрозрачным — значки на время
//   живых обоев скрыты под ними, и человеку это говорится словами. Сами
//   значки НЕ выключаются: окно обоев ушло — значки снова видны.
// * nemo-desktop прозрачен всегда (визуал RGBA), но при запуске сам
//   отказывается рисовать стол («Desktop already managed»), если самое
//   нижнее окно типа «рабочий стол» в `_NET_CLIENT_LIST_STACKING` чужое —
//   а это окно обоев, если оно показано раньше nemo-desktop (вход в сеанс,
//   перезапуск Nemo). Исключения — ключ `org.nemo.desktop
//   ignored-desktop-handlers`: подстрока `WM_CLASS` (res_class) без учёта
//   регистра (Nemo 6.6 `desktop_handler_is_ignored`); у всех хостов обоев
//   класс начинается с `Rexon`. Где эта схема есть, Rexon дописывает туда
//   `rexon`, не трогая чужие значения, а прежнее значение записывает и
//   возвращает тем же «Остановить живые обои».
//
// ⚠️ Пересоздание окна стола — переключение `org.mate.background
// show-desktop-icons` «выкл → вкл», а не `caja -q`: выход Caja закрыл бы
// открытые окна файлового менеджера, а под mate-session Caja ещё и
// перезапускается сама (X-MATE-AutoRestart).

/// Чья программа рисует значки рабочего стола X11.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopIconsOwner {
    /// MATE.
    Caja,
    /// Cinnamon.
    Nemo,
    /// XFCE.
    Xfdesktop,
}

impl DesktopIconsOwner {
    /// Имя программы для человека (имена программ не переводятся).
    pub fn program_name(self) -> &'static str {
        match self {
            DesktopIconsOwner::Caja => "Caja",
            DesktopIconsOwner::Nemo => "Nemo",
            DesktopIconsOwner::Xfdesktop => "xfdesktop",
        }
    }
}

/// Кто рисует значки на этом столе; `None` — не X11 либо стол, где окно
/// обоев значкам не мешает (или мы его не знаем).
///
/// `de` — имя окружения от `wallpaper_engine::detect_de`, `session_type` —
/// от `wallpaper_engine::detect_session_type`.
pub fn desktop_icons_owner(de: &str, session_type: &str) -> Option<DesktopIconsOwner> {
    if session_type != "x11" {
        return None;
    }
    match de {
        "mate" => Some(DesktopIconsOwner::Caja),
        "cinnamon" | "cinnamon-mint" => Some(DesktopIconsOwner::Nemo),
        "xfce" => Some(DesktopIconsOwner::Xfdesktop),
        _ => None,
    }
}

pub const MATE_BACKGROUND_SCHEMA: &str = "org.mate.background";
pub const MATE_SHOW_ICONS_KEY: &str = "show-desktop-icons";
pub const CAJA_DESKTOP_SCHEMA: &str = "org.mate.caja.desktop";
pub const CAJA_TRANSPARENT_KEY: &str = "transparent-background";
pub const MATE_SESSION_COMPONENTS_SCHEMA: &str = "org.mate.session.required-components";
pub const MATE_WINDOW_MANAGER_KEY: &str = "windowmanager";
pub const MARCO_GENERAL_SCHEMA: &str = "org.mate.Marco.general";
pub const MARCO_COMPOSITING_KEY: &str = "compositing-manager";

/// Пауза между «значки выкл» и «значки вкл» при пересоздании окна стола
/// Caja: оба изменения, пришедшие разом, Caja сливает в одно и окно не
/// пересоздаёт (замер на стенде 31.10.2026).
pub const CAJA_DESKTOP_RECREATE_PAUSE_MS: u64 = 1500;

/// Чем подготовка и откат значков трогают машину. Живая реализация —
/// `wallpaper_exec::DesktopIconsSys`, подставные — в сверках.
pub trait DesktopIconsIo {
    /// Вывод команды, если она отработала с кодом 0; иначе `None`.
    fn run(&mut self, argv: &[&str]) -> Option<String>;
    /// Записанное прежнее состояние стола либо `None`.
    fn read_state(&mut self) -> Option<String>;
    /// Записать состояние (`None` — стереть). Истина — получилось.
    fn write_state(&mut self, content: Option<&str>) -> bool;
    /// Подождать.
    fn pause_ms(&mut self, ms: u64);
}

/// Файл прежнего состояния стола: `$XDG_STATE_HOME/rexon/desktop-icons.state`
/// (по умолчанию `~/.local/state/…`).
pub fn desktop_icons_state_path(xdg_state_home: &str, home: &str) -> String {
    let base = if xdg_state_home.trim().is_empty() {
        format!("{}/.local/state", home.trim_end_matches('/'))
    } else {
        xdg_state_home.trim_end_matches('/').to_string()
    };
    format!("{base}/rexon/desktop-icons.state")
}

fn gsettings_get(io: &mut impl DesktopIconsIo, schema: &str, key: &str) -> Option<String> {
    io.run(&["gsettings", "get", schema, key]).map(|out| out.trim().trim_matches('\'').to_string())
}

fn gsettings_bool(io: &mut impl DesktopIconsIo, schema: &str, key: &str) -> Option<bool> {
    match gsettings_get(io, schema, key)?.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn gsettings_set(io: &mut impl DesktopIconsIo, schema: &str, key: &str, value: bool) -> bool {
    let value = if value { "true" } else { "false" };
    let ok = io.run(&["gsettings", "set", schema, key, value]).is_some();
    if !ok {
        crate::log_warn!("desktop-icons", "не записалось {schema} {key} = {value}");
    }
    ok
}

/// Есть ли у установленной Caja ключ прозрачного фона стола — по схеме.
fn caja_has_transparent_key(io: &mut impl DesktopIconsIo) -> bool {
    io.run(&["gsettings", "list-keys", CAJA_DESKTOP_SCHEMA])
        .is_some_and(|keys| keys.lines().any(|k| k.trim() == CAJA_TRANSPARENT_KEY))
}

/// Оконный менеджер MATE — Marco (его композитор и включаем). Не узнали —
/// Marco: он стоит в MATE по умолчанию.
fn mate_wm_is_marco(io: &mut impl DesktopIconsIo) -> bool {
    gsettings_get(io, MATE_SESSION_COMPONENTS_SCHEMA, MATE_WINDOW_MANAGER_KEY)
        .is_none_or(|wm| wm.is_empty() || wm.eq_ignore_ascii_case("marco"))
}

/// Пересоздать окно стола Caja, не трогая окна файлового менеджера.
fn recreate_caja_desktop(io: &mut impl DesktopIconsIo) {
    gsettings_set(io, MATE_BACKGROUND_SCHEMA, MATE_SHOW_ICONS_KEY, false);
    io.pause_ms(CAJA_DESKTOP_RECREATE_PAUSE_MS);
    gsettings_set(io, MATE_BACKGROUND_SCHEMA, MATE_SHOW_ICONS_KEY, true);
}

/// Окно значков, которое можно пересоздать переключением «значки выкл →
/// вкл»: схема и ключ по `WM_CLASS` окна (экземпляр, класс); `None` —
/// не умеем (xfdesktop фон рисует непрозрачным, пересоздание не поможет).
///
/// Нужно там, где оконный менеджер не ставит окно обоев под окно значков
/// по просьбе: xfwm4 отбрасывает ConfigureWindow окна типа «рабочий стол»
/// (`clientMoveResizeWindow`), а `_NET_RESTACK_WINDOW` не поддерживает.
/// Новое окно значков он кладёт поверх окна обоев — на стенде (Xvfb,
/// xfwm4 4.20 с композитором, 31.10.2026) так и у nemo-desktop, и у Caja
/// со сквозным фоном.
pub fn icons_window_toggle(instance: &str, class: &str) -> Option<(&'static str, &'static str)> {
    let is = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    if is(instance, "nemo-desktop") || is(class, "nemo-desktop") {
        return Some((NEMO_DESKTOP_SCHEMA, NEMO_SHOW_ICONS_KEY));
    }
    if is(instance, "desktop_window") && is(class, "caja") {
        return Some((MATE_BACKGROUND_SCHEMA, MATE_SHOW_ICONS_KEY));
    }
    None
}

/// Пересоздать окно значков переключением `schema key` «выкл → вкл» с
/// паузой [`CAJA_DESKTOP_RECREATE_PAUSE_MS`] (Caja без неё слила бы оба
/// изменения; nemo-desktop пересоздаёт и без паузы, ему она безвредна).
///
/// Ключ выключен (значков нет) или не читается — ничего не трогается.
/// Не включилось обратно — ещё одна попытка: значки нельзя оставить
/// выключенными. Истина — окно пересоздано и ключ снова включён.
pub fn recreate_icons_window(io: &mut impl DesktopIconsIo, schema: &str, key: &str) -> bool {
    if gsettings_bool(io, schema, key) != Some(true) {
        crate::log_info!("desktop-icons", "{schema} {key} не включён — пересоздавать окно значков незачем");
        return false;
    }
    if !gsettings_set(io, schema, key, false) {
        return false;
    }
    io.pause_ms(CAJA_DESKTOP_RECREATE_PAUSE_MS);
    if gsettings_set(io, schema, key, true) {
        return true;
    }
    io.pause_ms(CAJA_DESKTOP_RECREATE_PAUSE_MS);
    let back = gsettings_set(io, schema, key, true);
    if !back {
        crate::log_warn!("desktop-icons", "{schema} {key} не вернулся в true — значки стола выключены");
    }
    back
}

/// Прежнее состояние стола: что Rexon поменял и к чему вернуть.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopIconsSaved {
    /// Часть Caja записана (строка `owner=caja`); повторные применения её
    /// не перезаписывают — они увидели бы уже наши значения.
    pub caja_recorded: bool,
    /// Ключ прозрачного фона Caja был выключен — вернуть `false`.
    pub caja_transparent_was_off: bool,
    /// Композитор Marco был выключен — вернуть `false`.
    pub marco_compositing_was_off: bool,
    /// Rexon дописал себя в `org.nemo.desktop ignored-desktop-handlers`;
    /// здесь прежнее значение — текст GVariant, как его печатает gsettings.
    pub nemo_ignored_before: Option<String>,
}

impl DesktopIconsSaved {
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        if self.caja_recorded {
            text.push_str(&format!(
                "owner=caja\ncaja-transparent-was-off={}\nmarco-compositing-was-off={}\n",
                self.caja_transparent_was_off, self.marco_compositing_was_off
            ));
        }
        if let Some(before) = &self.nemo_ignored_before {
            text.push_str(&format!("nemo-ignored-desktop-handlers-before={}\n", one_line(before)));
        }
        text
    }

    /// Разбор записанного; чужой или битый файл — `None` (откатывать нечего).
    pub fn parse(text: &str) -> Option<Self> {
        let mut saved = DesktopIconsSaved::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let flag = value.trim() == "true";
            match key.trim() {
                "owner" => saved.caja_recorded = value.trim() == "caja",
                "caja-transparent-was-off" => saved.caja_transparent_was_off = flag,
                "marco-compositing-was-off" => saved.marco_compositing_was_off = flag,
                "nemo-ignored-desktop-handlers-before" if parse_gvariant_strv(value).is_some() => {
                    saved.nemo_ignored_before = Some(value.trim().to_string());
                }
                _ => {}
            }
        }
        if !saved.caja_recorded {
            saved.caja_transparent_was_off = false;
            saved.marco_compositing_was_off = false;
        }
        (saved.caja_recorded || saved.nemo_ignored_before.is_some()).then_some(saved)
    }
}

fn one_line(text: &str) -> String {
    text.lines().map(str::trim).collect::<Vec<_>>().join(" ")
}

/// Записанное прежнее состояние, дополненное `change`, — обратно в файл.
/// Части, записанные раньше, не теряются: Caja и Nemo пишут каждая своё.
fn update_saved(io: &mut impl DesktopIconsIo, change: impl FnOnce(&mut DesktopIconsSaved)) {
    let mut saved = io.read_state().and_then(|s| DesktopIconsSaved::parse(&s)).unwrap_or_default();
    change(&mut saved);
    if !io.write_state(Some(&saved.to_text())) {
        crate::log_warn!("desktop-icons", "прежнее состояние стола не записалось — откатить его будет нечем");
    }
}

pub const NEMO_DESKTOP_SCHEMA: &str = "org.nemo.desktop";
pub const NEMO_IGNORED_HANDLERS_KEY: &str = "ignored-desktop-handlers";
/// Значки стола nemo-desktop: «выкл → вкл» пересоздаёт его окно.
pub const NEMO_SHOW_ICONS_KEY: &str = "show-desktop-icons";
/// Наше значение в `ignored-desktop-handlers`: подстрока `WM_CLASS` всех
/// хостов обоев (`RexonVideoWallpaper`, `RexonWebWallpaper`, …).
pub const NEMO_REXON_HANDLER: &str = "rexon";

fn is_rexon_handler(entry: &str) -> bool {
    entry.trim().eq_ignore_ascii_case(NEMO_REXON_HANDLER)
}

/// Разбор массива строк GVariant, как его печатает `gsettings get`:
/// `['conky', 'csd-background']`, `@as []`, кавычки одинарные или двойные,
/// `\` — экранирование. Не массив строк — `None`.
pub fn parse_gvariant_strv(text: &str) -> Option<Vec<String>> {
    let t = text.trim();
    let t = t.strip_prefix("@as").map_or(t, str::trim_start);
    let inner = t.strip_prefix('[')?.strip_suffix(']')?;
    let mut out = Vec::new();
    let mut chars = inner.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let quote = match chars.next() {
            None => return Some(out),
            Some(q @ ('\'' | '"')) => q,
            Some(_) => return None,
        };
        let mut item = String::new();
        loop {
            match chars.next()? {
                '\\' => item.push(chars.next()?),
                c if c == quote => break,
                c => item.push(c),
            }
        }
        out.push(item);
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        match chars.next() {
            None => return Some(out),
            Some(',') => {}
            Some(_) => return None,
        }
    }
}

/// Массив строк в текст GVariant для `gsettings set`.
pub fn format_gvariant_strv(items: &[String]) -> String {
    if items.is_empty() {
        return "@as []".to_string();
    }
    let quoted: Vec<String> = items
        .iter()
        .map(|s| format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect();
    format!("[{}]", quoted.join(", "))
}

fn nemo_has_ignored_key(io: &mut impl DesktopIconsIo) -> bool {
    io.run(&["gsettings", "list-keys", NEMO_DESKTOP_SCHEMA])
        .is_some_and(|keys| keys.lines().any(|k| k.trim() == NEMO_IGNORED_HANDLERS_KEY))
}

fn nemo_ignored_get(io: &mut impl DesktopIconsIo) -> Option<(String, Vec<String>)> {
    let raw = io.run(&["gsettings", "get", NEMO_DESKTOP_SCHEMA, NEMO_IGNORED_HANDLERS_KEY])?;
    let raw = raw.trim().to_string();
    let list = parse_gvariant_strv(&raw)?;
    Some((raw, list))
}

fn nemo_ignored_set(io: &mut impl DesktopIconsIo, value: &str) -> bool {
    let ok = io.run(&["gsettings", "set", NEMO_DESKTOP_SCHEMA, NEMO_IGNORED_HANDLERS_KEY, value]).is_some();
    if !ok {
        crate::log_warn!("desktop-icons", "не записалось {NEMO_DESKTOP_SCHEMA} {NEMO_IGNORED_HANDLERS_KEY} = {value}");
    }
    ok
}

/// nemo-desktop не должен принимать окно обоев за чужой стол: дописать
/// `rexon` в `ignored-desktop-handlers`, где эта схема есть. Чужие
/// значения остаются; прежнее значение записывается первым дописыванием.
fn prepare_nemo_handlers(io: &mut impl DesktopIconsIo) {
    if !nemo_has_ignored_key(io) {
        return;
    }
    let Some((raw, mut list)) = nemo_ignored_get(io) else {
        crate::log_warn!("desktop-icons", "{NEMO_DESKTOP_SCHEMA} {NEMO_IGNORED_HANDLERS_KEY} не прочитался — не трогаю");
        return;
    };
    if list.iter().any(|e| is_rexon_handler(e)) {
        return;
    }
    update_saved(io, |saved| {
        if saved.nemo_ignored_before.is_none() {
            saved.nemo_ignored_before = Some(raw.clone());
        }
    });
    list.push(NEMO_REXON_HANDLER.to_string());
    if nemo_ignored_set(io, &format_gvariant_strv(&list)) {
        crate::log_info!("desktop-icons", "nemo-desktop: окно обоев в {NEMO_IGNORED_HANDLERS_KEY} (было {raw})");
    }
}

/// Убрать `rexon` из `ignored-desktop-handlers`. Если с тех пор список
/// никто не менял — вернуть записанное значение как было; иначе убрать
/// только наше, чужие правки оставить.
fn restore_nemo_handlers(io: &mut impl DesktopIconsIo, before: &str) {
    let Some((_, current)) = nemo_ignored_get(io) else {
        return;
    };
    if !current.iter().any(|e| is_rexon_handler(e)) {
        return;
    }
    let rest: Vec<String> = current.into_iter().filter(|e| !is_rexon_handler(e)).collect();
    if parse_gvariant_strv(before).as_deref() != Some(rest.as_slice()) {
        nemo_ignored_set(io, &format_gvariant_strv(&rest));
        return;
    }
    // Прежнее значение могло быть умолчанием схемы — тогда сброс вернёт
    // его, не оставив в dconf записи, которой там не было.
    let _ = io.run(&["gsettings", "reset", NEMO_DESKTOP_SCHEMA, NEMO_IGNORED_HANDLERS_KEY]);
    if nemo_ignored_get(io).map(|(_, list)| list) != Some(rest) {
        nemo_ignored_set(io, before);
    }
}

/// Значки будут скрыты: файловый менеджер стола не умеет сквозной фон.
pub fn desktop_icons_hidden_text(owner: DesktopIconsOwner) -> String {
    crate::i18n::trf(
        "Desktop icons will be hidden while the live wallpaper is on: %1 cannot draw a \
         see-through desktop yet. The icons come back when the live wallpaper stops.",
        &[&owner.program_name()],
    )
}

/// Значки остаются: Rexon включил сквозной фон стола Caja (и, если надо,
/// композитор Marco).
pub fn desktop_icons_kept_text(compositing_turned_on: bool) -> String {
    let mut text = crate::i18n::tr(
        "Desktop icons stay on top of the live wallpaper: Rexon turned on the see-through \
         desktop background in Caja.",
    );
    if compositing_turned_on {
        text.push(' ');
        text.push_str(&crate::i18n::tr("Marco's compositing was turned on for this as well."));
    }
    text
}

/// Подготовить стол к живым обоям: что сказать человеку (`None` — нечего).
///
/// Зовётся при каждом применении живых обоев окном на X11; повтор
/// безвреден: уже настроенное не трогается, прежнее состояние записывается
/// только первым применением.
///
/// Ключ nemo-desktop правится на любом из этих столов, где есть его схема:
/// nemo-desktop запускают и вне Cinnamon (его запасной режим), а проверка
/// окна стола у него та же.
pub fn prepare_desktop_icons(io: &mut impl DesktopIconsIo, owner: DesktopIconsOwner) -> Option<String> {
    prepare_nemo_handlers(io);
    match owner {
        DesktopIconsOwner::Nemo => None,
        DesktopIconsOwner::Xfdesktop => {
            // 0 — значков нет; не задано — умолчание xfdesktop (файлы).
            let style = io.run(&["xfconf-query", "-c", "xfce4-desktop", "-p", "/desktop-icons/style"]);
            (style.as_deref().map(str::trim) != Some("0")).then(|| desktop_icons_hidden_text(owner))
        }
        DesktopIconsOwner::Caja => prepare_caja(io),
    }
}

fn prepare_caja(io: &mut impl DesktopIconsIo) -> Option<String> {
    // Значков нет (или нет самой схемы MATE) — закрывать нечего.
    if gsettings_bool(io, MATE_BACKGROUND_SCHEMA, MATE_SHOW_ICONS_KEY) != Some(true) {
        return None;
    }
    if !caja_has_transparent_key(io) {
        crate::log_info!("desktop-icons", "у Caja нет ключа {CAJA_TRANSPARENT_KEY} — значки будут под живыми обоями");
        return Some(desktop_icons_hidden_text(DesktopIconsOwner::Caja));
    }
    let transparent_on = gsettings_bool(io, CAJA_DESKTOP_SCHEMA, CAJA_TRANSPARENT_KEY) == Some(true);
    let marco = mate_wm_is_marco(io);
    let compositing_off = marco && gsettings_bool(io, MARCO_GENERAL_SCHEMA, MARCO_COMPOSITING_KEY) == Some(false);
    if transparent_on && !compositing_off {
        return None;
    }
    // Записать, что было, — только первым применением: повторное увидело
    // бы уже наши значения.
    if !io.read_state().and_then(|s| DesktopIconsSaved::parse(&s)).is_some_and(|s| s.caja_recorded) {
        update_saved(io, |saved| {
            saved.caja_recorded = true;
            saved.caja_transparent_was_off = !transparent_on;
            saved.marco_compositing_was_off = compositing_off;
        });
    }
    if compositing_off && !gsettings_set(io, MARCO_GENERAL_SCHEMA, MARCO_COMPOSITING_KEY, true) {
        return Some(desktop_icons_hidden_text(DesktopIconsOwner::Caja));
    }
    if !transparent_on {
        if !gsettings_set(io, CAJA_DESKTOP_SCHEMA, CAJA_TRANSPARENT_KEY, true) {
            return Some(desktop_icons_hidden_text(DesktopIconsOwner::Caja));
        }
        recreate_caja_desktop(io);
    }
    crate::log_info!(
        "desktop-icons",
        "Caja: сквозной фон стола включён{}",
        if compositing_off { ", композитор Marco включён" } else { "" }
    );
    Some(desktop_icons_kept_text(compositing_off))
}

/// Вернуть стол, как был до первого применения живых обоев. Истина — было
/// что возвращать.
pub fn restore_desktop_icons(io: &mut impl DesktopIconsIo) -> bool {
    let Some(text) = io.read_state() else {
        return false;
    };
    let Some(saved) = DesktopIconsSaved::parse(&text) else {
        io.write_state(None);
        return false;
    };
    if saved.caja_transparent_was_off
        && gsettings_bool(io, CAJA_DESKTOP_SCHEMA, CAJA_TRANSPARENT_KEY) == Some(true)
        && gsettings_set(io, CAJA_DESKTOP_SCHEMA, CAJA_TRANSPARENT_KEY, false)
        && gsettings_bool(io, MATE_BACKGROUND_SCHEMA, MATE_SHOW_ICONS_KEY) == Some(true)
    {
        recreate_caja_desktop(io);
    }
    if saved.marco_compositing_was_off {
        gsettings_set(io, MARCO_GENERAL_SCHEMA, MARCO_COMPOSITING_KEY, false);
    }
    if let Some(before) = &saved.nemo_ignored_before {
        restore_nemo_handlers(io, before);
    }
    io.write_state(None);
    crate::log_info!("desktop-icons", "стол возвращён как был до живых обоев");
    true
}
