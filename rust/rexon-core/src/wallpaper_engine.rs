//! Решения движка обоев — порт `rexon/wallpaper_engine.py`.
//!
//! Модуль отвечает на один вопрос: КАКОЙ командной строкой применить обои в
//! этом окружении. Отсюда и раскладка слоёв, принятая в проекте: здесь —
//! только детерминированная часть (вход → план), а исполнение (`safe_run`,
//! `subprocess.Popen`, сигналы процессам, Pillow, потоки) остаётся в
//! Python-шиме `wallpaper_engine.py`, который исполняет то, что решило ядро.
//!
//! Наблюдения за средой приходят ПАРАМЕТРАМИ: какие бинари есть в системе,
//! имя монитора, разрешение экрана, подсказка об имени пакета для текущего
//! дистрибутива. Ядро ничего не спрашивает у машины само — иначе его нельзя
//! было бы прогнать голденом.
//!
//! Паритет с живым Python — эталон
//! `rust/rexon-py/tests/fixtures/wallpaper_engine_reference.json`: он снят с
//! питоновской версии ДО переноса (155 сценариев, 156 внешних вызовов) и
//! проверен мутацией — восемь порч значений в argv, порядке предпочтения
//! бэкендов, текстах и таблицах роняют сверку.
//!
//! ⚠️ Починка при переносе (правило #16): формат «сцена» запускал
//! `python -m rexon.parallax_engine`, а этого модуля в дереве нет с июля —
//! процесс поднимался и сразу умирал, а покупателю показывали «Scene Engine
//! запущен». Теперь честный отказ, как у обоев-приложений.

use serde::Serialize;

// Проверки файловой системы для поиска бинаря хоста видеообоев. Трейт
// общий с поиском демона waywallen: два метода (`is_exec_file`, `which`)
// описывают ровно те же side-effect'ы, и заводить второй смысла нет.
use crate::we_client::DaemonFs;
// Связка композитор+шелл опознаётся в system.rs — там же, где композитор.
use crate::i18n::{tr, trc, trf};
use crate::system::DesktopShell;

// ── окружение ──────────────────────────────────────────────────────────────

/// Композиторы без собственной «настройки обоев»: фон ставит отдельный
/// демон (swww/hyprpaper/swaybg/wbg), путь для всех один.
pub const WLROOTS_DE: &[&str] = &[
    "cosmic", "hyprland", "labwc", "niri", "river", "sway", "wayfire",
];

/// Текущий DE по `XDG_CURRENT_DESKTOP` / `DESKTOP_SESSION`.
///
/// Своей таблицы окружений здесь нет (правило #12): ответ даёт общий
/// [`crate::system::detect_current_de`]. Прежняя таблица движка искала
/// подстроки, звала Cinnamon `cinnamon` (везде в продукте он
/// `cinnamon-mint`) и не узнавала Plasma по имени сеанса. Поэтому все ветки
/// движка ниже принимают оба id Cinnamon: сюда приходят и ответ ядра, и
/// значения, сохранённые раньше.
///
/// Не опознали — `generic`: общий путь (feh / демон фона), как и прежде.
/// Флаг Wayland намеренно `false`: незнакомый композитор не должен
/// превращаться в «окружение с именем первого токена», у движка для него
/// одна ветка — общая.
pub fn detect_de(desktop: &str, session: &str) -> String {
    crate::system::detect_current_de(desktop, session, false)
        .unwrap_or_else(|| "generic".to_string())
}

/// `wayland` / `x11` / `unknown` по трём переменным сеанса.
pub fn detect_session_type(
    session_type: &str,
    wayland_display: &str,
    display: &str,
) -> String {
    let st = session_type.to_lowercase();
    if st == "wayland" || st == "x11" {
        return st;
    }
    if !wayland_display.is_empty() {
        return "wayland".to_string();
    }
    if !display.is_empty() {
        return "x11".to_string();
    }
    "unknown".to_string()
}

/// Композитор без своей настройки обоев?
pub fn is_wlroots_de(de: &str) -> bool {
    WLROOTS_DE.contains(&de)
}

/// Имя первого монитора из ответа `wlr-randr --json` / `hyprctl monitors -j`.
/// Не разобралось — `None`, вызывающий подставляет `*`.
pub fn parse_monitor_json(stdout: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(stdout).ok()?;
    let first = value.as_array()?.first()?;
    Some(
        first
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("*")
            .to_string(),
    )
}

/// Разрешение активного режима из `xrandr --current` (строка со звёздочкой).
pub fn parse_xrandr_resolution(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .find(|line| line.contains('*'))
        .and_then(|line| line.trim().split_whitespace().next())
        .map(|s| s.to_string())
}

// ── шаг плана ──────────────────────────────────────────────────────────────

/// Что шим делает с шагом.
pub mod step_kind {
    /// `safe_run(argv)`; успех — код 0.
    pub const RUN: &str = "run";
    /// `Popen(argv)` — процесс живых обоев (`self._process`).
    pub const SPAWN: &str = "spawn";
    /// `Popen(argv, start_new_session=True)` — процесс живых обоев.
    pub const SPAWN_SESSION: &str = "spawn_session";
    /// `Popen(argv, start_new_session=True)` — процесс, УДЕРЖИВАЮЩИЙ
    /// статичный фон (swaybg/wbg); живёт дольше живых обоев.
    pub const SPAWN_HOLD: &str = "spawn_hold";
    /// `plasma_evaluate_script(argv[0])`.
    pub const PLASMA_SCRIPT: &str = "plasma_script";
    /// Перечислить свойства xfconf и записать путь в каждое `/last-image`.
    pub const XFCONF_ALL: &str = "xfconf_all";
    /// Ничего не запускать — сразу отказ с `err_message`.
    pub const FAIL: &str = "fail";
    /// COSMIC: записать картинку в родной ключ cosmic-bg
    /// ([`super::apply_cosmic_bg`]) — без команды и без своего процесса.
    pub const COSMIC_BG: &str = "cosmic_bg";
}

/// Сколько секунд шаг `spawn*` ждёт, что поднятый им процесс ЖИВ.
///
/// ⚠️ Починка целого класса «рапортуем успех о мёртвом запуске». Все шаги
/// `spawn`, `spawn_session` и `spawn_hold` поднимают процесс, который рисует
/// (или удерживает) стол, пока жив; успехом же считался сам факт, что запуск
/// не бросил исключение. Последствия были два, и оба тяжёлые:
///
/// 1. Покупатель получал зелёное «обои запущены» на ПУСТОМ столе: прежние
///    обои снимаются до применения, а новый процесс умирал через миллисекунды
///    (`python -m rexon.web_wallpaper` — модуля в дереве нет вовсе).
/// 2. Запасные шаги плана становились недостижимы: `python3` есть всегда,
///    значит первый шаг «удавался» — и до `surf`/`xwinwrap` на X11 очередь
///    не доходила НИКОГДА.
///
/// Величина выбрана по замеру на машине разработчика (CachyOS, 23.08.2026):
/// мгновенный отказ — `python -m` без модуля — укладывается в 10 мс; самый
/// медленный наблюдавшийся отказ, когда процесс сперва грузит тяжёлый стек и
/// только потом падает (GTK+WebKit2 — 190 мс, mpv на несуществующем файле —
/// 93 мс), укладывается в 0,2 с. 0,5 с даёт запас в 2,6 раза и незаметен
/// рядом с самим применением обоев. Больше брать нельзя: задержка платится на
/// КАЖДОМ формате и при каждом возврате фона после «Остановить»; меньше —
/// вернётся ложный успех на медленно падающих бэкендах.
///
/// Ждать надо не «поспать столько-то», а опрашивать: мёртвый запуск отдаёт
/// отказ сразу, полную задержку платит только живой.
pub const SPAWN_ALIVE_GRACE_SEC: f64 = 0.5;

/// Шаг опроса «жив ли процесс» внутри [`SPAWN_ALIVE_GRACE_SEC`], секунд.
///
/// Опрос, а не сон: мёртвый запуск обязан отдать отказ сразу, полную
/// отсрочку платит только живой. 20 мс — 25 проб на всю отсрочку, дешевле
/// самого запуска и мельче любого наблюдавшегося отказа (10 мс у `python -m`
/// без модуля — единственный, кто в шаг укладывается, и тот доедет за две
/// пробы).
pub const ALIVE_POLL_STEP_SEC: f64 = 0.02;

/// Сколько ждать смерти снимаемого процесса обоев НА КАЖДОМ сигнале, секунд.
///
/// Порядок остановки один на все процессы движка: SIGTERM группе → ждём →
/// SIGKILL группе → ждём. Пять секунд — то же, что давал Python, и это не
/// произвол: mpv/mpvpaper по SIGTERM отпускают поверхность и закрывают
/// вывод, а не умирают мгновенно, и убить их раньше времени значит оставить
/// на экране мусор последнего кадра.
pub const KILL_WAIT_SEC: f64 = 5.0;

/// Таймаут вызова `evaluateScript` в plasmashell, секунд.
///
/// Скрипт исполняется В ГЛАВНОМ ПОТОКЕ оболочки, поэтому ждать приходится
/// столько же, сколько ждёт занятая перерисовкой Plasma.
pub const PLASMA_SCRIPT_TIMEOUT_SEC: f64 = 10.0;

/// Таймаут коротких команд рабочего стола (иконки, сброс плагина), секунд.
pub const DESKTOP_CMD_TIMEOUT_SEC: f64 = 5.0;

/// Опрос уже работающего демона swww, секунд.
pub const SWWW_QUERY_TIMEOUT_SEC: f64 = 3.0;

/// Подъём демона старой подкомандой `swww init`, секунд.
pub const SWWW_INIT_TIMEOUT_SEC: f64 = 5.0;

/// Пауза после запуска отдельного `swww-daemon`, секунд: он поднимает свой
/// сокет не мгновенно, и следующая же `swww img` без этой паузы упиралась
/// бы в «нет соединения» на ровном месте.
pub const SWWW_DAEMON_SETTLE_SEC: f64 = 0.5;

/// Текст на место `{stderr}`, когда процесс обоев умер сразу после запуска.
///
/// Вывод самого процесса шим не читает намеренно: труба на живых обоях
/// переполнилась бы и заткнула хост, а файл рос бы всё время их работы
/// (`/tmp` здесь — оперативная память). Код возврата отвечает на вопрос
/// «почему пусто» ровно настолько, насколько нужно покупателю.
pub fn spawn_died_message(code: Option<i32>) -> String {
    match code {
        Some(c) => trf("the process exited right after starting (code %1)", &[&c]),
        None => tr("the process exited right after starting"),
    }
}

/// Один шаг применения обоев. Шаги идут по порядку; первый удавшийся
/// заканчивает применение, неудача уводит к следующему — кроме `terminal`,
/// который отвечает за исход сам.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Step {
    /// Как исполнять — см. [`step_kind`].
    pub kind: String,
    /// Имя бэкенда для журнала.
    pub backend: String,
    /// Поднять демон swww перед основной командой.
    pub ensure_swww: bool,
    /// Команды до основной; их код возврата не смотрится.
    pub pre: Vec<Vec<String>>,
    /// Основная команда (для `plasma_script` — единственный элемент — сам JS).
    pub argv: Vec<String>,
    /// Команды после успеха; их код возврата не смотрится.
    pub post: Vec<Vec<String>>,
    /// Сообщение об успехе.
    pub ok_message: String,
    /// Сообщение об ошибке; `{stderr}` подставляет шим.
    pub err_message: String,
    /// Второе сообщение об ошибке (у `xfconf_all` — «нет мониторов»).
    pub alt_message: String,
    /// Формат, который записывается в состояние движка при успехе.
    pub format: String,
    /// Неудача этого шага завершает применение отказом.
    pub terminal: bool,
    /// `safe_run` вернул `None` (таймаут/нет бинаря) — считать успехом.
    pub ok_on_none: bool,
    /// Таймаут основной команды, секунд.
    pub timeout: Option<u64>,
    /// Проверка, что обои ДЕЙСТВИТЕЛЬНО записаны: после удачного кода
    /// возврата исполнитель читает этот файл и сверяет ключ с путём обоев
    /// ([`ini_check_passes`]). Нужна там, где код возврата ничего не
    /// доказывает (у pcmanfm-qt `--set-wallpaper` отвечает нулём всегда).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_ini: Option<IniCheck>,
    /// Удавшийся шаг ставит фон сам композитор — наш удерживающий процесс
    /// (swaybg/wbg) больше не нужен и снимается.
    pub drops_holder: bool,
}

/// Где программа хранит путь обоев: файл (`~/.config/…`), группа, ключ.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IniCheck {
    pub file: String,
    pub section: String,
    pub key: String,
}

/// Значение ключа `key` в группе `[section]` INI-текста (Qt `QSettings`:
/// путь может быть в кавычках; пробелы вокруг `=` допускаются).
pub fn ini_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_section = t[1..t.len() - 1].trim() == section;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            if k.trim() == key {
                let v = v.trim();
                let v = v
                    .strip_prefix('"')
                    .and_then(|x| x.strip_suffix('"'))
                    .unwrap_or(v);
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Записала ли программа именно эти обои: файл есть, ключ найден и равен
/// пути. Нет файла, нет ключа, другой путь — НЕ записала, «✅» не будет.
pub fn ini_check_passes(text: Option<&str>, check: &IniCheck, path: &str) -> bool {
    text.and_then(|t| ini_value(t, &check.section, &check.key))
        .is_some_and(|v| v == path)
}

/// Файл настроек pcmanfm-qt в профиле LXQt.
pub const PCMANFM_QT_LXQT_SETTINGS: &str = "~/.config/pcmanfm-qt/lxqt/settings.conf";

impl Step {
    fn new(kind: &str, backend: &str, argv: Vec<String>) -> Self {
        Step {
            kind: kind.to_string(),
            backend: backend.to_string(),
            ensure_swww: false,
            pre: Vec::new(),
            argv,
            post: Vec::new(),
            ok_message: String::new(),
            err_message: String::new(),
            alt_message: String::new(),
            format: "static".to_string(),
            terminal: false,
            ok_on_none: false,
            timeout: None,
            verify_ini: None,
            drops_holder: false,
        }
    }

    fn ok(mut self, msg: impl Into<String>) -> Self {
        self.ok_message = msg.into();
        self
    }

    fn err(mut self, msg: impl Into<String>) -> Self {
        self.err_message = msg.into();
        self
    }

    fn fmt(mut self, format: &str) -> Self {
        self.format = format.to_string();
        self
    }

    fn terminal(mut self) -> Self {
        self.terminal = true;
        self
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn fail(message: impl Into<String>) -> Step {
    Step::new(step_kind::FAIL, "нет", Vec::new()).err(message)
}

// ── статичные обои ─────────────────────────────────────────────────────────

/// Схема и ключ gsettings для окружения (пусто — не через gsettings).
pub fn gsettings_schema(de: &str) -> Option<(&'static str, &'static str)> {
    match de {
        // Pantheon: фон рисует Gala по org.gnome.desktop.background
        // (pantheon-default-settings задаёт там picture-uri и -dark).
        "gnome" | "budgie" | "pantheon" | "unity" => {
            Some(("org.gnome.desktop.background", "picture-uri"))
        }
        "cinnamon" | "cinnamon-mint" => {
            Some(("org.cinnamon.desktop.background", "picture-uri"))
        }
        "mate" => Some(("org.mate.background", "picture-filename")),
        _ => None,
    }
}

/// Чтение текущего значения ключа gsettings — так движок узнаёт обои,
/// которые надо вернуть после остановки живых.
pub fn gsettings_get_argv(schema: &str, key: &str) -> Vec<String> {
    argv(&["gsettings", "get", schema, key])
}

/// Ключ тёмных обоев рядом с [`gsettings_schema`]: у GNOME и его соседей
/// при тёмном оформлении (`color-scheme = prefer-dark`) стол показывает
/// `picture-uri-dark`. `None` — у окружения такого ключа нет.
pub fn gsettings_dark_key(de: &str) -> Option<(&'static str, &'static str)> {
    match gsettings_schema(de)? {
        ("org.gnome.desktop.background", _) => Some(("org.gnome.desktop.background", "picture-uri-dark")),
        _ => None,
    }
}

/// Значение gsettings в выводе `gsettings get` (`'file:///a b.jpg'`) —
/// путь без кавычек и `file://`. Пусто — ключ пуст.
pub fn gsettings_path_value(out: &str) -> String {
    out.trim().trim_matches(['\'', '"']).replace("file://", "")
}

/// Тёмное ли оформление по выводу `gsettings get … color-scheme`.
pub fn prefers_dark(color_scheme_out: &str) -> bool {
    gsettings_path_value(color_scheme_out) == "prefer-dark"
}

/// Свойство xfconf с картинкой главного стола из вывода
/// `xfconf-query -c xfce4-desktop -l`: первый монитор, первый рабочий стол
/// (`…/monitor<имя>/workspace0/last-image`), иначе первое `last-image`.
pub fn xfce_last_image_prop(list: &str) -> Option<String> {
    let props: Vec<&str> = list.lines().map(str::trim).filter(|l| l.ends_with("/last-image")).collect();
    props
        .iter()
        .find(|p| p.contains("/workspace0/"))
        .or_else(|| props.first())
        .map(|p| p.to_string())
}

/// Чтение одного свойства xfconf.
pub fn xfconf_get_argv(prop: &str) -> Vec<String> {
    argv(&["xfconf-query", "-c", "xfce4-desktop", "-p", prop])
}

/// Программа, которой берётся кадр и делается faststart-ремукс.
pub const FFMPEG_BIN: &str = "ffmpeg";

/// Проигрыватели VLC: движок считает VLC доступным, если есть любой.
pub const VLC_BINS: &[&str] = &["cvlc", "vlc"];

/// Программы, про которые движок спрашивает подсказку об имени пакета в
/// текущем дистрибутиве. Имена совпадают с полями [`Hints`].
pub const HINT_TOOLS: &[&str] = &[
    "mpvpaper", "xwinwrap", "cvlc", "webkit2gtk-4.1", "swww", "yt-dlp",
];

/// Итог успешной установки статичных обоев — ОДНА фраза на все пути.
///
/// ⭐ Починка (правило #13): прежде в скобках стояло служебное имя пути,
/// которым обои поставились, — «Обои установлены (generic)», «(KDE D-Bus)»,
/// «(swaybg)». Покупателю это слово ничего не говорит, а «generic» читалось
/// ещё и как ошибка. Каким путём поставлено, знает журнал: исполнитель пишет
/// туда [`Step::backend`] шага, давшего успех.
pub fn wallpaper_set_message() -> String {
    tr("Wallpaper set")
}

fn gsettings_step(path: &str, schema: &str, key: &str) -> Step {
    // GNOME ждёт file:// URI, MATE — обычный путь.
    let value = if key.contains("picture-uri") {
        format!("file://{path}")
    } else {
        path.to_string()
    };
    let mut step = Step::new(
        step_kind::RUN,
        "gsettings",
        argv(&["gsettings", "set", schema, key, &value]),
    )
    .ok(wallpaper_set_message())
    .err(tr("gsettings error"))
    .terminal();
    // GNOME 42+ держит отдельную картинку для тёмной темы.
    if key == "picture-uri" {
        step.post
            .push(argv(&["gsettings", "set", schema, "picture-uri-dark", &value]));
    }
    step
}

/// Тип сеанса для [`static_plan`] у Deepin в сеансе Treeland (его задаёт
/// исполнитель по композитору, [`crate::dde::treeland_session_live`]).
pub const TREELAND_SESSION: &str = "treeland";

/// Deepin — только в сеансе Treeland: обои принимает Центр управления
/// ([`crate::dde::control_center_wallpaper_argv`]) — служба оформления там
/// обоев не ставит. Сторонний feh/swaybg не годится: фон dde-shell лёг бы
/// поверх. Прочитать результат у Treeland нечем — сообщение говорит
/// «переданы», а не «поставлены». Вне Treeland — отказ
/// ([`crate::dde::not_treeland_message`]).
fn deepin_static_steps(path: &str, treeland: bool) -> Vec<Step> {
    if !treeland {
        return vec![fail(crate::dde::not_treeland_message())];
    }
    let mut st = Step::new(step_kind::RUN, "deepin-control-center", crate::dde::control_center_wallpaper_argv(path, false))
        .ok(crate::dde::wallpaper_handed_message());
    st.timeout = Some(70);
    vec![st, fail(crate::dde::control_center_failed_message())]
}

fn kde_static_steps(path: &str, tools: &[String]) -> Vec<Step> {
    if has(tools, "plasma-apply-wallpaperimage") {
        return vec![Step::new(
            step_kind::RUN,
            "plasma-apply-wallpaperimage",
            argv(&["plasma-apply-wallpaperimage", path]),
        )
        .ok(wallpaper_set_message())
        .err({
            // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
            tr("plasma-apply-wallpaperimage error: {stderr}")
        })
        .terminal()];
    }
    vec![Step::new(
        step_kind::PLASMA_SCRIPT,
        "kde-dbus",
        vec![kde_static_script(path)],
    )
    .ok(wallpaper_set_message())
    .err(tr("Couldn't set the wallpaper on KDE"))
    .terminal()]
}

fn xfce_static_steps(path: &str) -> Vec<Step> {
    let mut step = Step::new(
        step_kind::XFCONF_ALL,
        "xfconf-query",
        argv(&["xfconf-query", "-c", "xfce4-desktop", "-l"]),
    )
    .ok(wallpaper_set_message())
    .err(tr("xfconf-query didn't respond"))
    .terminal();
    // `{prop}` подставляется шимом на каждое найденное свойство `/last-image`.
    step.post.push(argv(&[
        "xfconf-query", "-c", "xfce4-desktop", "-p", "{prop}", "-s", path,
    ]));
    step.alt_message = tr("No Xfce monitors found");
    vec![step]
}

fn wayland_daemon_steps(de: &str, path: &str, tools: &[String], swww_hint: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    // Порядок: swww (умеет «поставить и выйти») → hyprpaper (где он уже
    // настроен) → swaybg/wbg как удерживающий процесс.
    if has(tools, "swww") {
        let mut step = Step::new(step_kind::RUN, "swww", argv(&["swww", "img", path]))
            .ok(wallpaper_set_message());
        step.ensure_swww = true;
        steps.push(step);
    }
    if has(tools, "hyprpaper") {
        let mut step = Step::new(
            step_kind::RUN,
            "hyprpaper",
            argv(&["hyprctl", "hyprpaper", "wallpaper", &format!(",{path}")]),
        )
        .ok(wallpaper_set_message());
        step.pre
            .push(argv(&["hyprctl", "hyprpaper", "preload", path]));
        steps.push(step);
    }
    // swaybg/wbg не «ставят и выходят» — они ДЕРЖАТ фон, пока живы. Прежний
    // такой процесс шим снимает сам, иначе каждая смена обоев оставляла бы
    // позади ещё один живой процесс.
    for (tool, cmd) in [
        ("swaybg", argv(&["swaybg", "-i", path, "-m", "fill"])),
        ("wbg", argv(&["wbg", path])),
    ] {
        if has(tools, tool) {
            steps.push(
                Step::new(step_kind::SPAWN_HOLD, tool, cmd)
                    .ok(wallpaper_set_message())
                    .err({
                        // TRANSLATORS: %1 is a program name; keep {stderr} as is:
                        // the program replaces it with the error output.
                        trf("Couldn't start %1: {stderr}", &[&tool])
                    })
                    .terminal(),
            );
            break;
        }
    }
    // Хвостовой отказ идёт ВСЕГДА, а не только когда ставить нечем: демон
    // может быть установлен и всё равно не справиться (`swww img` вернул не
    // ноль). Тогда покупателю нужно то же самое — ЧТО поставить, а не
    // молчание. Для успешного применения шаг недостижим, поэтому на журнал
    // вызовов он не влияет.
    let hint = if swww_hint.is_empty() { "swww" } else { swww_hint };
    // ⭐ Починка (правило #13): здесь стоял сырой идентификатор движка —
    // «Для обоев на generic нужна программа фона». Имя для человека и
    // грамотная форма неопознанного стола — у общего `place_de_name`.
    steps.push(fail(crate::screenshots::place_de_name(
        // TRANSLATORS: %1 is the desktop name, %2 a hint on how to install swww.
        &trf(
            "Wallpapers on %1 need a background program. \
             Install swww (%2) — or swaybg/hyprpaper/wbg.",
            &[&"%1", &hint],
        ),
        "%1",
        &crate::screenshots::display_name(de),
    )));
    steps
}

fn fallback_static_steps(
    de: &str,
    session_type: &str,
    path: &str,
    tools: &[String],
    swww_hint: &str,
) -> Vec<Step> {
    let mut steps = Vec::new();
    if session_type == "x11" && has(tools, "feh") {
        steps.push(
            Step::new(step_kind::RUN, "feh", argv(&["feh", "--bg-fill", path]))
                .ok(wallpaper_set_message()),
        );
    }
    if session_type == "wayland" {
        // Раньше любой нераспознанный Wayland-сеанс уходил в gsettings
        // GNOME-схемы: команда молча отрабатывала «успешно», а обои не
        // менялись, потому что схему никто не читает.
        steps.extend(wayland_daemon_steps(de, path, tools, swww_hint));
    } else {
        steps.push(gsettings_step(
            path,
            "org.gnome.desktop.background",
            "picture-uri",
        ));
    }
    steps
}

/// Бинари, наличие которых спрашивают планы движка. Шим пробует ровно этот
/// список и отдаёт найденное параметром `tools` — чтобы имён программ не
/// было ВТОРОЙ копией на Python: там они тихо разъехались бы с планами.
pub const PLAN_TOOLS: &[&str] = &[
    "feh", "hyprpaper", "mpv", "mpvpaper", "pcmanfm-qt",
    "plasma-apply-wallpaperimage", "surf", "swaybg", "swww", "wbg", "xwinwrap",
    "cosmic-bg", "swaymsg",
];

/// Окружения, чей рабочий стол настраивается через Plasma (её конфиг столов,
/// плагин обоев и `evaluateScript`). Отдельным вопросом, потому что этим
/// определяются сразу три ветки: что запоминать перед сменой обоев, чем
/// лечить застрявший плагин и как возвращать стол после остановки.
pub fn uses_plasma_config(de: &str) -> bool {
    de == "kde"
}

/// Окружения, обои которых движок сохраняет перед подменой и возвращает
/// после остановки. На остальных возвращать нечего: там мы фон не читали, и
/// лишняя команда сменила бы обои, которых мы не трогали.
pub const RESTORE_DES: &[&str] = &[
    "budgie", "cinnamon", "cinnamon-mint", "cosmic", "gnome", "kde", "mate", "pantheon", "xfce",
];

/// План возврата сохранённых обоев после остановки живых.
///
/// Это тот же путь, что и обычная установка статичной картинки, но только
/// для окружений [`RESTORE_DES`]; тип сессии тут не спрашивается — ни одна
/// из этих веток от него не зависит.
pub fn restore_static_plan(
    de: &str,
    path: &str,
    tools: &[String],
    swww_hint: &str,
) -> Vec<Step> {
    if !RESTORE_DES.contains(&de) {
        return Vec::new();
    }
    static_plan(de, "", path, tools, swww_hint)
}

/// План применения статичных обоев: упорядоченные попытки на текущем DE.
///
/// `tools` — какие бинари нашлись в системе; `swww_hint` — подсказка про
/// пакет фона для текущего дистрибутива (`tool_package_hint("swww")`).
pub fn static_plan(
    de: &str,
    session_type: &str,
    path: &str,
    tools: &[String],
    swww_hint: &str,
) -> Vec<Step> {
    if de == "kde" {
        return kde_static_steps(path, tools);
    }
    if de == "xfce" {
        return xfce_static_steps(path);
    }
    if de == "deepin" {
        return deepin_static_steps(path, session_type == TREELAND_SESSION);
    }
    if let Some((schema, key)) = gsettings_schema(de) {
        return vec![gsettings_step(path, schema, key)];
    }
    if de == "lxqt" {
        // Рабочий стол LXQt рисует pcmanfm-qt, он же владеет обоями: feh тут
        // не годится — фон перерисовывается поверх и обои «слетают». Нет
        // файлового менеджера (голый openbox+lxqt-panel) — общий путь.
        let mut steps = Vec::new();
        if has(tools, "pcmanfm-qt") {
            let mut step = Step::new(
                step_kind::RUN,
                "pcmanfm-qt",
                argv(&["pcmanfm-qt", "--wallpaper-mode=fit"]),
            )
            .ok(wallpaper_set_message());
            step.pre.push(argv(&["pcmanfm-qt", "--set-wallpaper", path]));
            // `--set-wallpaper` отвечает нулём и при уже запущенном
            // экземпляре; ошибку различаем только по коду второй команды.
            step.ok_on_none = true;
            // ⚠️ Д12: нулевой код ещё не значит «обои стоят» — проверяем,
            // что pcmanfm-qt записал путь к себе в настройки. Не записал —
            // шаг не удался, и человек не увидит ложного «✅».
            step.verify_ini = Some(IniCheck {
                file: PCMANFM_QT_LXQT_SETTINGS.to_string(),
                section: "Desktop".to_string(),
                key: "Wallpaper".to_string(),
            });
            steps.push(step);
        }
        steps.extend(fallback_static_steps(de, session_type, path, tools, swww_hint));
        return steps;
    }
    if de == "cosmic" {
        return cosmic_static_steps(path, tools, swww_hint);
    }
    if de == "sway" {
        let mut steps: Vec<Step> = sway_bg_step(path, tools).into_iter().collect();
        steps.extend(wayland_daemon_steps(de, path, tools, swww_hint));
        return steps;
    }
    if is_wlroots_de(de) {
        return wayland_daemon_steps(de, path, tools, swww_hint);
    }
    fallback_static_steps(de, session_type, path, tools, swww_hint)
}

/// sway рисует фон сам — своим swaybg по команде `output * bg` (sway(5)).
/// Просим его, а не поднимаем второй swaybg рядом: держащий процесс
/// «Оформлению» недоступен вовсе, а вкладка обоев держала бы два фона на
/// одном слое. Путь sway отдаёт wordexp: в одинарных кавычках (внутри
/// двойных — их снимает разбор команды) он остаётся одним словом с
/// пробелом, апострофом и `$`. Двойную кавычку, обратную косую, `$`
/// (sway подставил бы свою переменную) и двойной пробел (sway
/// экранирует его сам, и путь перестаёт находиться) так не донести — тогда
/// шага нет, и фон держит swaybg. Сверено на sway 1.12 без экрана.
fn sway_bg_step(path: &str, tools: &[String]) -> Option<Step> {
    if !has(tools, "swaymsg")
        || path.contains("  ")
        || path.chars().any(|c| c.is_control() || matches!(c, '"' | '\\' | '$'))
    {
        return None;
    }
    let quoted = format!("\"'{}'\"", path.replace('\'', "'\\''"));
    let mut step = Step::new(step_kind::RUN, "sway", argv(&["swaymsg", "output", "*", "bg", &quoted, "fill"]))
        .ok(wallpaper_set_message());
    step.drops_holder = true;
    Some(step)
}

// ── COSMIC: родной ключ cosmic-bg ──────────────────────────────────────────
//
// Фон COSMIC рисует cosmic-bg — такая же поверхность слоя Background, как у
// swww/swaybg. Чужой демон поверх него — лотерея порядка создания, а после
// перезахода cosmic-bg снова показывает своё. Поэтому картинку пишем туда же,
// куда её пишут «Настройки COSMIC → Обои», и cosmic-bg подхватывает ключ
// сам (следит за ним `ConfigWatchSource`).
//
// Формат сверен с исходниками cosmic-bg (`config/src/lib.rs`, эпоха 1.10):
// `NAME = "com.system76.CosmicBackground"`, версия 1; ключ `all` — запись
// `Entry` для всех экранов (поля `output`, `source`, `filter_by_theme`,
// `rotation_frequency`, `filter_method`, `scaling_mode`, `sampling_method`;
// `deny_unknown_fields`, у первых четырёх нет значения по умолчанию); ключ
// `same-on-all` — `bool`, при `false` cosmic-bg берёт записи
// `output.<экран>`, а `all` — только для экранов без своей. Значения пишет
// `cosmic-config` через `ron::ser::to_string_pretty` — в этом же виде
// пишем и мы (образец — `/usr/share/cosmic/com.system76.CosmicBackground/v1/all`).

/// Каталог ключей cosmic-bg от `$XDG_CONFIG_HOME`.
pub const COSMIC_BG_DIR_REL: &str = "cosmic/com.system76.CosmicBackground/v1";

/// Значение однострочного поля верхнего уровня записи RON (`name: value,`).
fn ron_field(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(name)?.trim_start().strip_prefix(':')?;
        Some(rest.trim().trim_end_matches(',').trim().to_string())
    })
}

/// Путь картинки из записи `Entry` cosmic-bg (`source: Path("…")`); цвет,
/// градиент или неразобранная запись — `None`.
pub fn cosmic_bg_source_path(text: &str) -> Option<String> {
    let value = ron_field(text, "source")?;
    let inner = value.strip_prefix("Path(")?.strip_suffix(')')?.trim();
    let quoted = inner.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(chars.next()?);
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// Запись `Entry` для всех экранов с картинкой `path`.
///
/// Как «Настройки COSMIC» (`config_wallpaper_entry`), чужие поля прежней
/// записи `existing` сохраняются — способ масштабирования, фильтр, порядок
/// и частота смены; неразобранное или незнакомое значение заменяется
/// умолчанием cosmic-bg (`Entry::new`: Zoom, Lanczos, Alphanumeric, 900 с).
/// Путь с управляющими символами — `None` (см. [`crate::themes::cosmic_ron_string`]).
pub fn cosmic_bg_entry(existing: Option<&str>, path: &str) -> Option<String> {
    let source = crate::themes::cosmic_ron_string(path)?;
    let old = |name: &str| existing.and_then(|t| ron_field(t, name));
    let pick = |name: &str, allowed: &[&str], default: &str| -> String {
        old(name)
            .filter(|v| allowed.contains(&v.as_str()))
            .unwrap_or_else(|| default.to_string())
    };
    // Поле не используется cosmic-bg для одиночной картинки, но
    // обязательно; `#true` встречается в образце из /usr/share — читаем и так.
    let filter_by_theme = matches!(old("filter_by_theme").as_deref(), Some("true" | "#true"));
    let rotation = old("rotation_frequency")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(900);
    let scaling = old("scaling_mode")
        .filter(|v| {
            matches!(v.as_str(), "Zoom" | "Stretch")
                || (v.starts_with("Fit((") && v.ends_with("))") && !v.contains('\n'))
        })
        .unwrap_or_else(|| "Zoom".to_string());
    Some(format!(
        "(\n    output: \"all\",\n    source: Path({source}),\n    filter_by_theme: {filter_by_theme},\n    \
         rotation_frequency: {rotation},\n    filter_method: {},\n    scaling_mode: {scaling},\n    \
         sampling_method: {},\n)",
        pick("filter_method", &["Nearest", "Linear", "Lanczos"], "Lanczos"),
        pick("sampling_method", &["Alphanumeric", "Random"], "Alphanumeric"),
    ))
}

/// Поставить картинку фоном COSMIC: ключ `all` + `same-on-all = true`
/// (одна картинка на все экраны, как и в остальных окружениях Rexon).
/// `read`/`write` — ввод-вывод исполнителя (у каждого свой шов), пути —
/// полные. Удалось — значит запись перечитана и в ней именно этот путь.
pub fn apply_cosmic_bg(
    config_home: &str,
    path: &str,
    read: &mut dyn FnMut(&str) -> Option<String>,
    write: &mut dyn FnMut(&str, &str) -> bool,
) -> bool {
    let dir = format!("{}/{COSMIC_BG_DIR_REL}", config_home.trim_end_matches('/'));
    let all = format!("{dir}/all");
    let Some(entry) = cosmic_bg_entry(read(&all).as_deref(), path) else {
        return false;
    };
    if !write(&all, &entry) {
        return false;
    }
    let same = format!("{dir}/same-on-all");
    if read(&same).as_deref().map(str::trim) != Some("true") && !write(&same, "true") {
        return false;
    }
    read(&all).and_then(|t| cosmic_bg_source_path(&t)).as_deref() == Some(path)
}

/// Статичные обои на COSMIC: родной ключ, пока cosmic-bg установлен; нет
/// его (cosmic-comp без своего фона) или запись не удалась — общий путь
/// внешних демонов.
fn cosmic_static_steps(path: &str, tools: &[String], swww_hint: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    if has(tools, "cosmic-bg") {
        steps.push(
            Step::new(step_kind::COSMIC_BG, "cosmic-bg", Vec::new()).ok(wallpaper_set_message()),
        );
    }
    steps.extend(wayland_daemon_steps("cosmic", path, tools, swww_hint));
    steps
}

// ── анимация (GIF/APNG) ────────────────────────────────────────────────────

/// План GIF-обоев. На Wayland первым идёт swww (умеет GIF нативно), затем
/// mpvpaper как видео; на X11 — xwinwrap + mpv.
///
/// `de` нужен по той же причине, что и в [`youtube_plan`]: и swww, и
/// mpvpaper кладут картинку СВОИМ окном через `wlr-layer-shell`, а у Mutter
/// (GNOME) и Muffin (Cinnamon) этого протокола нет вовсе. Прежде такой сеанс
/// получал либо «swww или mpvpaper не установлены» при УСТАНОВЛЕННЫХ swww и
/// mpvpaper, либо запуск без картинки. Решение об окружении одно и общее с
/// видео- и YouTube-обоями — [`needs_shell_extension`].
pub fn animated_plan(
    de: &str,
    session_type: &str,
    path: &str,
    tools: &[String],
    monitor: &str,
    resolution: &str,
    hints: &Hints,
) -> Vec<Step> {
    if needs_shell_extension(de, session_type) {
        return vec![fail(trf(
            "On %1 there's nothing to show animated wallpapers with: both swww and \
             mpvpaper draw their own window through wlr-layer-shell, which the shell \
             doesn't have. Live wallpapers there are shown by the shell extension — \
             choose a video wallpaper (MP4/WebM).",
            &[&shell_ext_de_label(de)],
        ))];
    }
    if session_type == "wayland" {
        let mut steps = Vec::new();
        if has(tools, "swww") {
            let mut step = Step::new(
                step_kind::RUN,
                "swww",
                argv(&[
                    "swww",
                    "img",
                    path,
                    "--transition-type",
                    "fade",
                    "--transition-duration",
                    "1",
                ]),
            )
            .ok(tr("GIF wallpaper started"))
            .fmt("animated");
            step.ensure_swww = true;
            steps.push(step);
        }
        if has(tools, "mpvpaper") {
            steps.extend(mpvpaper_steps(path, false, 50, None, monitor, tools, hints));
        } else {
            // Хвостовой отказ нужен и когда swww ЕСТЬ, но `swww img` не
            // справился: подхватить GIF тогда некому, и покупателю говорим
            // что поставить. При успехе swww шаг недостижим.
            steps.push(fail(tr(
                "Neither swww nor mpvpaper is installed for GIF wallpapers on Wayland",
            )));
        }
        return steps;
    }
    xwinwrap_steps(path, false, 50, None, resolution, tools, hints)
}

// ── видео ──────────────────────────────────────────────────────────────────

/// Подсказки об именах пакетов для текущего дистрибутива. Собирает их шим
/// (`tool_package_hint`), ядро только вставляет в текст.
#[derive(Debug, Clone, Default)]
pub struct Hints {
    /// Подсказка про `mpvpaper`.
    pub mpvpaper: String,
    /// Подсказка про `xwinwrap`.
    pub xwinwrap: String,
    /// Подсказка про `cvlc`.
    pub cvlc: String,
    /// Подсказка про `webkit2gtk-4.1`.
    pub webkit: String,
    /// Подсказка про `swww`.
    pub swww: String,
    /// Подсказка про `yt-dlp`.
    pub ytdlp: String,
}

/// Нет `mpvpaper` — видео-обоям под Wayland нечем рисовать.
fn mpvpaper_missing_message(hint: &str) -> String {
    // TRANSLATORS: %1 is a hint on how to install mpvpaper.
    trf("The mpvpaper program wasn't found; video wallpapers can't run on Wayland without it. %1", &[&hint])
}

/// Нет `xwinwrap` — видео-обоям под X11 нечем рисовать.
fn xwinwrap_missing_message(hint: &str) -> String {
    // TRANSLATORS: %1 is a hint on how to install xwinwrap.
    trf("The xwinwrap program wasn't found; video wallpapers can't run on X11 without it. %1", &[&hint])
}

fn mpvpaper_steps(
    path: &str,
    audio: bool,
    volume: i64,
    audio_file: Option<&str>,
    monitor: &str,
    tools: &[String],
    hints: &Hints,
) -> Vec<Step> {
    if !has(tools, "mpvpaper") {
        return vec![fail(mpvpaper_missing_message(&hints.mpvpaper))];
    }
    let audio_opt = if audio { "" } else { " no-audio" };
    let vol_opt = if audio {
        format!(" volume={volume}")
    } else {
        String::new()
    };
    let af_opt = audio_file
        .map(|f| format!(" audio-file={f}"))
        .unwrap_or_default();
    let opts = format!("loop=inf hwdec=auto{audio_opt}{vol_opt}{af_opt}");
    vec![Step::new(
        step_kind::SPAWN,
        "mpvpaper",
        vec![
            "mpvpaper".into(),
            "-o".into(),
            opts,
            monitor.to_string(),
            path.to_string(),
        ],
    )
    .ok(tr("Video wallpaper started"))
    .fmt("video")]
}

/// Метка геометрии выхода в командах плана: `ШхВ+X+Y`.
///
/// `{resolution}` даёт только размер, а `xwinwrap` кроет корневое окно X по
/// геометрии целиком: без смещения окно второго монитора легло бы поверх
/// первого. Исполнитель ядра раскрывает эту метку в размер ВМЕСТЕ с
/// положением КОНКРЕТНОГО выхода, по метке на запуск.
pub const GEOMETRY_MARK: &str = "{geometry}";

/// Значение для `xwinwrap -g`.
///
/// Дали готовую геометрию (метку [`GEOMETRY_MARK`] или строку со смещением)
/// — уходит как есть. Дали один размер (так зовёт мост `rexon-py`) — к нему,
/// как и прежде, дописывается нулевое смещение: окно встаёт в левый верхний
/// угол стола.
fn xwinwrap_geometry(size_or_geometry: &str) -> String {
    if size_or_geometry == GEOMETRY_MARK || size_or_geometry.contains('+') {
        return size_or_geometry.to_string();
    }
    format!("{size_or_geometry}+0+0")
}

fn xwinwrap_steps(
    path: &str,
    audio: bool,
    volume: i64,
    audio_file: Option<&str>,
    resolution: &str,
    tools: &[String],
    hints: &Hints,
) -> Vec<Step> {
    if !has(tools, "xwinwrap") {
        return vec![fail(xwinwrap_missing_message(&hints.xwinwrap))];
    }
    if !has(tools, "mpv") {
        return vec![fail(tr("mpv isn't installed."))];
    }
    let mut cmd = vec![
        "xwinwrap".to_string(),
        "-g".to_string(),
        xwinwrap_geometry(resolution),
        "-ov".into(),
        "-ni".into(),
        "-s".into(),
        "-nf".into(),
        "--".into(),
        "mpv".into(),
        "--loop=inf".into(),
    ];
    if !audio {
        cmd.push("--no-audio".into());
    } else {
        cmd.push(format!("--volume={volume}"));
    }
    if let Some(f) = audio_file {
        cmd.push(format!("--audio-file={f}"));
    }
    cmd.extend(argv(&[
        "--no-osc",
        "--no-osd-bar",
        "--no-input-default-bindings",
        "--hwdec=auto",
        "--gpu-api=auto",
        "--wid=WID",
    ]));
    cmd.push(path.to_string());
    vec![Step::new(step_kind::SPAWN, "xwinwrap", cmd)
        .ok(tr("Video wallpaper started"))
        .fmt("video")]
}

/// Ключ схемы расширения оболочки — у GNOME и Cinnamon схемы разные, ключи
/// одни. `None` — окружение обходится обычным окном.
pub fn shell_ext_schema(de: &str) -> Option<&'static str> {
    match de {
        "gnome" => Some("org.gnome.shell.extensions.rexon-wallpaper"),
        "cinnamon" | "cinnamon-mint" => Some("org.cinnamon.extensions.rexon-wallpaper"),
        _ => None,
    }
}

/// Идентификатор нашего расширения оболочки.
pub const SHELL_EXT_ID: &str = "rexon-wallpaper@rexon.app";

/// Человеческое имя оболочки для сообщений покупателю.
///
/// В движке обои различаются по идентификатору (`gnome`, `cinnamon`), а в
/// тексте на экране идентификатору не место (правило #13): «потоком на
/// gnome ролик не проиграть» читается как сырая строка из конфига. Оба
/// сообщения про расширение оболочки берут имя отсюда, чтобы оно было
/// одним и тем же.
pub fn shell_ext_de_label(de: &str) -> &str {
    match de {
        "gnome" => "GNOME",
        "cinnamon" | "cinnamon-mint" => "Cinnamon",
        other => other,
    }
}

/// Нужен ли путь через расширение оболочки вместо своего окна.
///
/// Только Wayland: Mutter и Muffin не умеют wlr-layer-shell, окном обои туда
/// не положить. На X11 у обоих окружений работает обычное окно с
/// `_NET_WM_WINDOW_TYPE_DESKTOP`.
pub fn needs_shell_extension(de: &str, session_type: &str) -> bool {
    session_type == "wayland" && shell_ext_schema(de).is_some()
}

// ── кто владелец слоя обоев ────────────────────────────────────────────────
//
// ⚠️ Узел правила #12. Вопрос «наш ли фон в этом сеансе» возникает у КАЖДОГО
// плана обоев — картинки, видео, YouTube, обоев-приложений. Если решать его
// веткой в каждом плане, ветки разъедутся ровно так же, как разъехались три
// копии `detect_current_de`. Поэтому решение здесь одно, и все зовут его.

/// Кто рисует фон в этой связке композитор + шелл.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallpaperOwner {
    /// Фон наш: движок Rexon кладёт свою поверхность (или окно) сам.
    Rexon,
    /// Фон рисует чужой шелл. Драться с ним за слой бесполезно: его
    /// поверхность на слое Background останется поверх нашей, покупатель
    /// увидит чужие обои, а рапорт будет зелёным.
    Shell(DesktopShell),
}

impl WallpaperOwner {
    /// Наш ли фон.
    pub fn is_ours(self) -> bool {
        matches!(self, WallpaperOwner::Rexon)
    }

    /// Чужой шелл-владелец, если владелец чужой.
    pub fn shell(self) -> Option<DesktopShell> {
        match self {
            WallpaperOwner::Rexon => None,
            WallpaperOwner::Shell(s) => Some(s),
        }
    }
}

/// Кто владеет слоем обоев.
///
/// `shell` — ответ [`crate::system::detect_shell`]; `shell_draws_wallpaper` —
/// известно ли, что обои шелла ВЫКЛЮЧЕНЫ (`Some(false)`). `None` значит «не
/// узнали»: тогда считаем, что шелл рисует. Осторожная сторона выбрана
/// намеренно — сказать лишний раз, как выключить чужие обои, дешевле, чем
/// молча положить свои под чужие.
///
/// Порядок ответов:
///   * не Wayland — фон наш: оба шелла живут только на Wayland, а на X11
///     обои кладутся окном с `_NET_WM_WINDOW_TYPE_DESKTOP`;
///   * GNOME/Cinnamon на Wayland — фон наш: Mutter и Muffin не умеют
///     wlr-layer-shell, занять слой обоев там нечем ни им, ни нам, и путь
///     один — расширение оболочки ([`needs_shell_extension`]);
///   * шелла нет либо его обои выключены — фон наш;
///   * иначе фон чужой.
pub fn wallpaper_layer_owner(
    de: &str,
    session_type: &str,
    shell: Option<DesktopShell>,
    shell_draws_wallpaper: Option<bool>,
) -> WallpaperOwner {
    if session_type != "wayland" {
        return WallpaperOwner::Rexon;
    }
    if needs_shell_extension(de, session_type) {
        return WallpaperOwner::Rexon;
    }
    match shell {
        Some(s) if shell_draws_wallpaper != Some(false) => WallpaperOwner::Shell(s),
        _ => WallpaperOwner::Rexon,
    }
}

/// Что сказать покупателю, когда слоем обоев владеет чужой шелл.
///
/// Молчать здесь нельзя: снаружи это выглядит как «Rexon не работает», а на
/// деле обои легли бы под чужую поверхность. Поэтому текст говорит и что
/// происходит, и что нажать: выключатель — кнопка «Отдать обои Rexon» на
/// вкладке «Wallpaper Engine» (`crate::desktop_shell::hand_over`), а не
/// путь к файлу и имя ключа (правило #13: жаргону в тексте не место; где
/// какой ключ и почему именно там — сказано в шапке `desktop_shell`). Текст
/// ОДИН и живёт в ядре: второй копии в QML быть не должно.
pub fn shell_owner_message(shell: DesktopShell) -> String {
    trf(
        // TRANSLATORS: %1 is the shell name (Noctalia, DankMaterialShell). “Wallpaper Engine” is the tab name, “Give Wallpaper to Rexon” the button on it.
        "The background is currently drawn by %1: the shell keeps its own full-screen \
         surface on the wallpaper layer, and Rexon's wallpaper would end up underneath it. \
         On the Wallpaper Engine tab, press “Give Wallpaper to Rexon”, then apply the \
         Rexon wallpaper again.",
        &[&shell.label()],
    )
}

/// Команда проверки «в каком состоянии наше расширение оболочки».
///
/// GNOME спрашиваем саму оболочку по D-Bus (`GetExtensionInfo`): прежний
/// `gnome-extensions list --enabled` показывает лишь, что расширение
/// ВКЛЮЧЕНО в настройках, — а упавшее с ошибкой, не подходящее к версии
/// оболочки или ещё не загруженное (Wayland, нет перезахода) там тоже
/// «включено», и Rexon рапортовал «готово» при пустом столе. Разбор —
/// [`parse_shell_ext_state`]. У Cinnamon такого вызова нет — смотрим список
/// включённых расширений в его настройках.
pub fn shell_ext_state_argv(de: &str) -> Vec<String> {
    if de == "gnome" {
        argv(&[
            "gdbus",
            "call",
            "--session",
            "--dest",
            "org.gnome.Shell",
            "--object-path",
            "/org/gnome/Shell",
            "--method",
            "org.gnome.Shell.Extensions.GetExtensionInfo",
            SHELL_EXT_ID,
        ])
    } else {
        argv(&["gsettings", "get", "org.cinnamon", "enabled-extensions"])
    }
}

/// Состояние нашего расширения оболочки глазами самой оболочки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellExtState {
    /// Работает — обои покажутся.
    Enabled,
    /// Выключено в списке расширений.
    Disabled,
    /// Оболочка поймала ошибку в коде расширения.
    Error,
    /// Версии оболочки нет в `shell-version` нашего `metadata.json`.
    OutOfDate,
    /// Оболочка расширения не знает: поставлено, но не загружено (на
    /// Wayland — до перезахода в сеанс).
    NotLoaded,
    /// Спросить не удалось: оболочка не ответила или ответ непонятен.
    Unknown,
}

/// Разобрать ответ на [`shell_ext_state_argv`].
///
/// GNOME: `gdbus` печатает словарь `a{sv}`, состояние — число с плавающей
/// точкой (`'state': <1.0>`: оболочка упаковывает все числа как `d`,
/// `serializeExtension` в `misc/extensionUtils.js`). Значения
/// `ExtensionState` одинаковы в gnome-shell 45–51 (сверено по исходникам;
/// в 47 переименованы ENABLED/DISABLED в ACTIVE/INACTIVE, числа те же):
/// 1 включено, 2 выключено, 3 ошибка, 4 не та версия, 5 скачивается,
/// 6 загружено, но не включалось, 7 выключается, 8 включается,
/// 99 не установлено. Неизвестный uuid — пустой словарь `(@a{sv} {},)`.
///
/// Cinnamon: список `enabled-extensions` из `gsettings`.
///
/// `out = None` — команда не запустилась или вернула ошибку.
pub fn parse_shell_ext_state(de: &str, out: Option<&str>) -> ShellExtState {
    let Some(out) = out else {
        return ShellExtState::Unknown;
    };
    if de != "gnome" {
        if out.trim().is_empty() {
            return ShellExtState::Unknown;
        }
        return if out.contains(&format!("'{SHELL_EXT_ID}'")) {
            ShellExtState::Enabled
        } else {
            ShellExtState::Disabled
        };
    }
    if !out.trim_start().starts_with('(') {
        return ShellExtState::Unknown;
    }
    let Some(at) = out.find("'state': <") else {
        return ShellExtState::NotLoaded;
    };
    let rest = &out[at + "'state': <".len()..];
    let Some(state) = rest
        .split('>')
        .next()
        .and_then(|v| v.trim().parse::<f64>().ok())
    else {
        return ShellExtState::Unknown;
    };
    match state as i64 {
        1 | 8 => ShellExtState::Enabled,
        2 | 6 | 7 => ShellExtState::Disabled,
        3 => ShellExtState::Error,
        4 => ShellExtState::OutOfDate,
        5 | 99 => ShellExtState::NotLoaded,
        _ => ShellExtState::Unknown,
    }
}

/// Что сказать человеку, когда обои отданы расширению, а оно не работает.
///
/// `None` — расширение работает, говорить нечего. Иначе — удалась ли
/// передача и текст. Выключено, не загружено или не ответило — обои
/// переданы и появятся, как только расширение заработает (удача с
/// честным текстом); ошибка и не та версия — само расширение не
/// поправится, обоев не будет (отказ). Выключенному — текст плана
/// (`disabled`), он общий с прежней проверкой.
pub fn shell_ext_state_message(state: ShellExtState, disabled: &str) -> Option<(bool, String)> {
    match state {
        ShellExtState::Enabled => None,
        ShellExtState::Disabled => Some((true, disabled.to_string())),
        ShellExtState::Error => Some((
            false,
            tr("The wallpaper was handed to the Rexon shell extension, but GNOME Shell \
                reports an error in the extension, so the wallpaper won't appear. Log out \
                and back in; if that doesn't help, reinstall Rexon."),
        )),
        ShellExtState::OutOfDate => Some((
            false,
            tr("The wallpaper was handed to the Rexon shell extension, but GNOME Shell \
                considers it incompatible with this shell version and doesn't load it. \
                Update Rexon."),
        )),
        ShellExtState::NotLoaded => Some((
            true,
            tr("The wallpaper was handed to the Rexon shell extension, but the shell \
                hasn't loaded the extension yet. Log out and back in — the wallpaper \
                will appear after that."),
        )),
        ShellExtState::Unknown => Some((
            true,
            tr("The wallpaper was handed to the Rexon shell extension, but the shell \
                didn't say whether the extension is working. If the wallpaper doesn't \
                appear, turn on “Rexon Wallpaper Engine” in the extension list or log out and \
                back in."),
        )),
    }
}

/// Ключ схемы расширения, в который кладётся ГОТОВАЯ строка запуска хоста.
///
/// Расширение — код оболочки, а не наш процесс: `sys.executable` ему
/// недоступен, и голое `python3` из `PATH` у покупателя с пакетом, portable
/// или Flatpak указывает не туда (а то и никуда). Поэтому строку запуска
/// собирает тот, кто её знает, — движок обоев, — и передаёт как данные.
pub const SHELL_EXT_DAEMON_KEY: &str = "daemon-argv";

/// Текстовая запись массива строк для `gsettings set` (тип `as`).
///
/// Формат GVariant: `["a","b"]`; внутри строк экранируются `\` и `"`.
/// Значение уходит ОДНИМ элементом argv, оболочка его не разбирает.
pub fn gvariant_string_array(items: &[String]) -> String {
    let body: Vec<String> = items
        .iter()
        .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("[{}]", body.join(","))
}

/// Тексты и команды передачи обоев расширению оболочки.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ShellExtPlan {
    /// Записать источник (она же проверка установки: чужая схема — ошибка).
    pub source_argv: Vec<String>,
    /// Записать вид движка.
    pub engine_argv: Vec<String>,
    /// Записать строку запуска хоста ([`SHELL_EXT_DAEMON_KEY`]).
    ///
    /// ⚠️ Результат этого шага ОБЯЗАТЕЛЬНО проверять. Ключа может не
    /// оказаться в схеме расширения, оставшегося от прежней версии, —
    /// тогда `gsettings` вернёт ошибку. Раньше это было не страшно:
    /// расширение брало свой запасной путь `python3 -m
    /// rexon.video_wallpaper`. Модуль снят с дерева, запасного пути больше
    /// нет — и незамеченная ошибка означала бы пустой рабочий стол при
    /// зелёном рапорте (правило #13).
    pub daemon_argv: Vec<String>,
    /// Схемы нет — расширение не установлено.
    pub not_installed_message: String,
    /// Ключ [`SHELL_EXT_DAEMON_KEY`] не записался — расширение устарело.
    pub daemon_failed_message: String,
    /// Обои переданы, но расширение выключено.
    pub disabled_message: String,
    /// Всё на месте.
    pub ok_message: String,
}

/// План передачи видеообоев расширению оболочки GNOME/Cinnamon.
///
/// Демон запускает САМО расширение (оно же его снимает и читает кадры) —
/// поэтому здесь только записывается, что показывать и ЧЕМ показывать.
///
/// `host`/`has_audio` — те же, что у [`video_native_plan`]: строку запуска
/// обоим путям собирает одна функция ([`video_host_argv`]), второго решения
/// «чем запускать хост» в проекте быть не должно (правило #12).
pub fn shell_ext_plan(
    de: &str,
    path: &str,
    host: &str,
    has_audio: bool,
) -> Option<ShellExtPlan> {
    let cmd = video_host_argv(host, path, has_audio);
    // TRANSLATORS: %1 is the desktop name (GNOME, Cinnamon).
    let ok = trf("Video wallpaper started (%1, shell extension)", &[&shell_ext_de_label(de)]);
    shell_ext_plan_for(de, path, &cmd, "video", ok)
}

/// Отдать Flash-обои расширению оболочки GNOME/Cinnamon: та же строка
/// запуска, что у своего окна ([`swf_host_plan`]), — хост сам пишет кадры в
/// файл, расширение их показывает. Значение `engine` расширение только
/// проверяет на пустоту и пишет в журнал.
pub fn swf_shell_ext_plan(de: &str, path: &str, host: &str) -> Option<ShellExtPlan> {
    let cmd = wallpaper_host_argv(host, HostMode::Swf, path);
    let ok = trf("Flash wallpaper started (%1, shell extension)", &[&shell_ext_de_label(de)]);
    shell_ext_plan_for(de, path, &cmd, HostMode::Swf.as_arg(), ok)
}

/// Где в окне Rexon лежит расширение оболочки: раздел «Оформления», а не
/// вкладка «Расширения» — там только пакеты .rxn (живая проверка на
/// gnome-shell 51.1, 16.10.2026: подсказка отправляла не туда).
fn shell_ext_place(de: &str) -> String {
    let section = if de == "gnome" {
        trc("appearance section", "GNOME Extensions")
    } else {
        trc("appearance section", "Cinnamon Extensions")
    };
    format!("{} → {}", tr("Appearance"), section)
}

/// Общий план передачи обоев расширению оболочки — ОДИН на все режимы хоста:
/// источник, `engine`, строка запуска демона и слова отказов.
fn shell_ext_plan_for(
    de: &str,
    path: &str,
    cmd: &[String],
    engine: &str,
    ok_message: String,
) -> Option<ShellExtPlan> {
    let schema = shell_ext_schema(de)?;
    Some(ShellExtPlan {
        source_argv: argv(&["gsettings", "set", schema, "source", path]),
        engine_argv: argv(&["gsettings", "set", schema, "engine", engine]),
        daemon_argv: vec![
            "gsettings".to_string(),
            "set".to_string(),
            schema.to_string(),
            SHELL_EXT_DAEMON_KEY.to_string(),
            gvariant_string_array(cmd),
        ],
        not_installed_message: trf(
            // TRANSLATORS: %1 is a desktop id (gnome, cinnamon); %2 is the
            // place in Rexon, e.g. “Appearance → GNOME Extensions”.
            "Live wallpapers on %1 need the Rexon shell extension: it's installed \
             together with the program — turn it on in “%2” and try again.",
            &[&de, &shell_ext_place(de)],
        ),
        daemon_failed_message: trf(
            // TRANSLATORS: %1 is the place in Rexon, e.g. “Appearance → GNOME Extensions”.
            "The Rexon shell extension is left over from an older version and doesn't \
             know how to start the wallpaper. Reinstall it in “%1” and try again.",
            &[&shell_ext_place(de)],
        ),
        disabled_message: tr(
            "The wallpaper was handed to the shell extension, but the extension itself \
             is turned off — turn on “Rexon Wallpaper Engine” in the extension list.",
        ),
        ok_message,
    })
}

/// Каталоги `schemas/` нашего расширения в том порядке, в каком оболочка
/// ищет само расширение: пользовательская копия (`$XDG_DATA_HOME`, иначе
/// `~/.local/share`) перекрывает системные (`$XDG_DATA_DIRS`, иначе
/// `/usr/local/share:/usr/share`).
///
/// ⚠️ Зачем: схема расширения лежит и собрана в его собственном каталоге,
/// а в общем `/usr/share/glib-2.0/schemas` её может не быть (так было в
/// пакете до 16.10.2026; при установке в домашний каталог не бывает
/// никогда). Голый `gsettings set` её тогда не видит — «Схема … не
/// существует», — и обои расширению не передаются вовсе. Расширение же
/// читает настройки именно из схемы своего каталога (`getSettings`), так что
/// писать через неё — писать ровно туда, откуда оно прочтёт.
/// Пустой список — у окружения нет нашего расширения.
pub fn shell_ext_schema_dirs(
    de: &str,
    home: &str,
    xdg_data_home: &str,
    xdg_data_dirs: &str,
) -> Vec<String> {
    let sub = match shell_ext_schema(de) {
        Some(_) if de == "gnome" => "gnome-shell/extensions",
        Some(_) => "cinnamon/extensions",
        None => return Vec::new(),
    };
    let mut bases = Vec::new();
    if !xdg_data_home.is_empty() {
        bases.push(xdg_data_home.to_string());
    } else if !home.is_empty() {
        bases.push(format!("{home}/.local/share"));
    }
    let system = if xdg_data_dirs.is_empty() { "/usr/local/share:/usr/share" } else { xdg_data_dirs };
    bases.extend(system.split(':').filter(|d| !d.is_empty()).map(str::to_string));
    bases
        .iter()
        .map(|b| format!("{}/{sub}/{SHELL_EXT_ID}/schemas", b.trim_end_matches('/')))
        .collect()
}

/// Команда `gsettings` с явным каталогом схем: `--schemadir` встаёт сразу
/// после имени программы (так его ждёт `gsettings`). Не `gsettings` —
/// команда возвращается как есть.
pub fn with_schemadir(cmd: &[String], dir: &str) -> Vec<String> {
    if cmd.first().map(String::as_str) != Some("gsettings") {
        return cmd.to_vec();
    }
    let mut out = vec![cmd[0].clone(), "--schemadir".to_string(), dir.to_string()];
    out.extend_from_slice(&cmd[1..]);
    out
}

/// Снять обои у расширения оболочки: пустой источник = «не показывать».
pub fn shell_ext_stop_argv(de: &str) -> Option<Vec<String>> {
    let schema = shell_ext_schema(de)?;
    Some(argv(&["gsettings", "set", schema, "source", ""]))
}

/// Имя собранного хоста обоев (крейт `rexon-gui`, бинарь) — ОДНОГО на все
/// форматы, которые Rexon показывает своим процессом.
///
/// ⭐ 15.09.2026 хостов было два (`rexon-wallpaper-host` для видео и
/// `rexon-web-host` для веба) с двумя поисками, двумя сборками строки запуска
/// и двумя строками в упаковке. Решение владельца — не плодить хосты: формат
/// выбирается режимом ([`HostMode`]) в строке запуска одного бинаря. Имя
/// оставлено прежнее у видео: его уже записали в gsettings расширения
/// оболочки GNOME/Cinnamon у покупателей.
pub const WALLPAPER_HOST_BINARY: &str = "rexon-wallpaper-host";

/// Ключ режима в строке запуска хоста обоев.
pub const HOST_MODE_FLAG: &str = "--mode";

/// Режим хоста обоев: что показывает процесс `rexon-wallpaper-host`.
///
/// Новый формат добавляется вариантом здесь, а не новым бинарём — так
/// третьим режимом пришёл Flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostMode {
    /// Видеофайл или каталог обоев с видео (QtMultimedia).
    Video,
    /// Ссылка, HTML-файл или каталог веб-обоев (QtWebEngine).
    Web,
    /// Flash-ролик SWF: Ruffle рисует через wgpu (Vulkan), крейт `rexon-swf`.
    /// QML у режима нет — окно размещается тем же слоем, что у видео и веба.
    Swf,
    /// Клиент вывода демона waywallen на X11: окна-рабочие столы, в которых
    /// элемент вывода waywallen показывает кадры демона (сцены Wallpaper
    /// Engine). Запускает его демон по определению вывода
    /// (`wallpaper_backend::rexon_display_manifest`) или Rexon по «Применить».
    Waywallen,
}

impl HostMode {
    /// Все режимы — для справки хоста и сверок.
    pub const ALL: [HostMode; 4] = [HostMode::Video, HostMode::Web, HostMode::Swf, HostMode::Waywallen];

    /// Режим, когда ключа [`HOST_MODE_FLAG`] в строке нет, — видео.
    ///
    /// Это не «определение по содержимому», а совместимость: до объединения
    /// `rexon-wallpaper-host` был видео-хостом, и такую строку без режима
    /// расширение оболочки уже хранит в gsettings покупателя (ключ
    /// [`SHELL_EXT_DAEMON_KEY`]) до следующего применения обоев. Угадывать
    /// формат по пути хост не берётся: формат знает вызывающий, и решение
    /// «что это за обои» в проекте одно — у движка.
    pub const DEFAULT: HostMode = HostMode::Video;

    /// Значение ключа [`HOST_MODE_FLAG`].
    pub fn as_arg(self) -> &'static str {
        match self {
            HostMode::Video => "video",
            HostMode::Web => "web",
            HostMode::Swf => "swf",
            HostMode::Waywallen => "waywallen",
        }
    }

    /// Разбор значения ключа [`HOST_MODE_FLAG`]; незнакомое — `None`.
    pub fn from_arg(raw: &str) -> Option<HostMode> {
        HostMode::ALL.into_iter().find(|m| m.as_arg() == raw)
    }
}

/// Коды возврата хоста обоев — ОДНА таблица на все режимы. Общие коды
/// значат одно и то же в любом режиме; свои у режима не пересекаются.
pub mod host_exit {
    /// Всё хорошо.
    pub const OK: i32 = 0;
    /// Нечего показывать: разбор аргументов, нет файла, чужой тип обоев.
    pub const NOTHING_TO_SHOW: i32 = 1;
    /// QML не поднялся.
    pub const QML_FAILED: i32 = 2;
    /// Поток кадров не пошёл.
    pub const FRAMES_FAILED: i32 = 3;
    /// Видео: звук просили, а звуковой дорожки в файле нет.
    pub const VIDEO_NO_AUDIO: i32 = 4;
    /// Веб: страница не загрузилась.
    pub const WEB_PAGE_FAILED: i32 = 5;
    /// Flash: не нашлось Vulkan-адаптера, способного рисовать в окно обоев,
    /// или поверхность окна не создалась. Коды 7 и 8 совпадают с
    /// `rexon_swf::Error::exit_code`: библиотека и хост говорят одними числами.
    pub const SWF_NO_GPU: i32 = 7;
    /// Flash: ролик не прочитался или не разобрался.
    pub const SWF_MOVIE_FAILED: i32 = 8;
}

/// Отказ, когда хост видеообоев не найден.
///
/// Прежде на этом месте был молчаливый откат на Python-модуль
/// `rexon.video_wallpaper`; модуль снят с дерева, и вместо него человек
/// получает объяснение, ЧТО не так и что с этим делать (правило #13).
/// Пустой экран с зелёным рапортом «обои применены» — ровно тот класс
/// поломок, который в проекте уже чинили.
pub fn video_host_missing_message() -> String {
    tr("There's nothing to show live wallpapers with: the rexon-wallpaper-host \
        program wasn't found. It's installed together with Rexon — reinstall Rexon. \
        If you work from the source tree, build it: cargo build --release -p rexon-gui.")
}

/// Где искать собранный хост обоев — по убыванию свежести: рабочее дерево
/// (`rust/target`, release затем debug), дальше `$PATH` у вызывающего —
/// туда бинарь кладёт установка. Тот же состав и порядок, что у хелпера
/// заставки ([`crate::session_splash::splash_overlay_candidates`]).
pub fn host_candidates(repo_root: &str, binary: &str) -> Vec<String> {
    let root = repo_root.trim_end_matches('/');
    vec![
        format!("{root}/rust/target/release/{binary}"),
        format!("{root}/rust/target/debug/{binary}"),
    ]
}

/// Собранный хост обоев либо `None` — показывать нечем. ОДИН поиск на все
/// режимы (правило #12): пока хостов было два, разъехавшийся порядок
/// означал бы видео из свежей сборки и веб из вчерашней установки.
///
/// `None` не означает «пойдём Python-путём»: запасного пути нет, и
/// вызывающий обязан отказать ([`VIDEO_HOST_MISSING_MESSAGE`],
/// [`WEB_HOST_MISSING`]), а не делать вид, что обои применены. Проверки ФС —
/// снаружи (трейт [`DaemonFs`], тот же, что у поиска демона waywallen).
pub fn find_wallpaper_host_binary<FS: DaemonFs>(fs: &mut FS, repo_root: &str) -> Option<String> {
    for cand in host_candidates(repo_root, WALLPAPER_HOST_BINARY) {
        if fs.is_exec_file(&cand) {
            return Some(cand);
        }
    }
    fs.which(WALLPAPER_HOST_BINARY)
}

/// Строка запуска хоста обоев в режиме `mode` — ОДНА на все форматы и все
/// пути показа: `<host> --mode <режим> <путь>`.
///
/// Режим пишется всегда, в том числе у видео, хотя хост без ключа и так
/// показывает видео ([`HostMode::DEFAULT`]): по строке запуска должно быть
/// видно, что она запускает.
pub fn wallpaper_host_argv(host: &str, mode: HostMode, path: &str) -> Vec<String> {
    vec![
        host.to_string(),
        HOST_MODE_FLAG.to_string(),
        mode.as_arg().to_string(),
        path.to_string(),
    ]
}

/// Строка запуска хоста в режиме видео — [`wallpaper_host_argv`] плюс
/// признак немой обоины.
///
/// Ею запускается и своё окно ([`video_native_plan`]), и демон под
/// расширением оболочки GNOME/Cinnamon ([`shell_ext_plan`]). `host` — путь к
/// собранному бинарю ([`find_wallpaper_host_binary`]); Python в цепочке
/// процессов не участвует нигде и ничем не подменяется: не нашли бинарь —
/// отказ ([`VIDEO_HOST_MISSING_MESSAGE`]), а не другой хост.
pub fn video_host_argv(host: &str, path: &str, has_audio: bool) -> Vec<String> {
    let mut cmd = wallpaper_host_argv(host, HostMode::Video, path);
    if !has_audio {
        cmd.push("--no-audio".into());
    }
    cmd
}

/// Запуск собственного хоста видеообоев.
///
/// Работает на любом Wayland с wlr_layer_shell и на X11 через xprop-трик
/// `_NET_WM_WINDOW_TYPE_DESKTOP`.
///
/// `host` — путь к собранному бинарю ([`find_wallpaper_host_binary`]): он и
/// становится строкой запуска, Python в цепочке не остаётся. Бинаря нет —
/// сюда управление доходить не должно: вызывающий отказывает раньше
/// текстом [`VIDEO_HOST_MISSING_MESSAGE`].
///
/// ⚠️ Громкость и «звук вкл/выкл» здесь БОЛЬШЕ НЕ ПЕРЕДАЮТСЯ, и это не
/// упущение. Пока они ехали ключами запуска, настройка звука доходила до
/// обоев ровно один раз — при старте процесса, — поэтому подвинутый
/// ползунок не делал ничего, пока обои не переставишь заново. Теперь оба
/// значения живут в `settings.json`, а хост следит за файлом и применяет
/// правку на ходу (`rexon-core::wallpaper_audio`).
///
/// `has_audio` — это НЕ настройка человека, а свойство самой обоины: «звук
/// у неё предусмотрен». Разница принципиальная: настройку человек меняет на
/// ходу, а немую обоину звуком не наделишь. Ключ ставится только в
/// отрицательном случае — умолчание берётся из `project.json` обоев.
pub fn video_native_plan(host: &str, path: &str, has_audio: bool) -> Vec<Step> {
    let cmd = video_host_argv(host, path, has_audio);
    let name = base_name(path);
    vec![Step::new(step_kind::SPAWN_SESSION, "video_wallpaper", cmd)
        // TRANSLATORS: %1 is a file name.
        .ok(trf("Video wallpaper started: %1", &[&name]))
        .err({
            // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
            tr("Couldn't start video_wallpaper: {stderr}")
        })
        .fmt("video")
        .terminal()]
}

// ── YouTube ────────────────────────────────────────────────────────────────

/// Формат yt-dlp для качества; неизвестное качество — как `1080p`.
pub fn youtube_format(quality: &str) -> String {
    let h = match quality {
        "4k" => 2160,
        "1440p" => 1440,
        "720p" => 720,
        "480p" => 480,
        "best" => {
            return "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best".to_string();
        }
        _ => 1080,
    };
    format!("bestvideo[height<={h}][ext=mp4]+bestaudio[ext=m4a]/best[height<={h}]")
}

/// Режим YouTube-обоев, который в этой сессии вообще выполним.
///
/// «Стрим» показывает `mpvpaper` (Wayland) или `xwinwrap`+`mpv` (X11) —
/// оба кладут картинку СВОИМ окном. На GNOME-Wayland и Cinnamon-Wayland
/// своего окна нет вовсе: `mpvpaper` работает через wlr-layer-shell,
/// которого у Mutter и Muffin не бывает, — там обои показывает расширение
/// оболочки, а оно берёт кадры у хоста, которому нужен файл на диске.
/// Поэтому «стрим» в таких сеансах молча становится «скачать»: покупателю
/// нужны обои, а не выбор между ошибкой и чёрным экраном (правило #13).
///
/// Решение ОДНО и общее с видео-обоями — [`needs_shell_extension`].
pub fn youtube_effective_mode(de: &str, session_type: &str, mode: &str) -> &'static str {
    if mode == "download" || needs_shell_extension(de, session_type) {
        "download"
    } else {
        "stream"
    }
}

/// Пояснение, почему «стрим» в этом окружении заменён скачиванием.
/// Пустая строка — замены не было.
pub fn youtube_mode_downgraded_note(de: &str, session_type: &str, mode: &str) -> String {
    if mode == "download" || !needs_shell_extension(de, session_type) {
        return String::new();
    }
    let de = shell_ext_de_label(de);
    trf(
        "On %1 live wallpapers are shown by the shell extension, and it needs a file \
         on disk — the video was downloaded instead of streamed.",
        &[&de],
    )
}

/// План YouTube-обоев. На KDE маршрут тот же, что везде, — по типу сессии.
///
/// `de` нужен, чтобы этот план НЕ мог запустить `mpvpaper` там, где ему не
/// на чем рисовать (GNOME/Cinnamon-Wayland): такие сеансы уходят на
/// расширение оболочки через [`youtube_effective_mode`], и сюда управление
/// доходить не должно. Проверка стоит здесь на случай прямого вызова.
pub fn youtube_plan(
    de: &str,
    session_type: &str,
    url: &str,
    audio: bool,
    volume: i64,
    quality: &str,
    tools: &[String],
    monitor: &str,
    resolution: &str,
    hints: &Hints,
) -> Vec<Step> {
    let fmt = youtube_format(quality);
    if needs_shell_extension(de, session_type) {
        return vec![fail(trf(
            "On %1 a YouTube video can't be streamed: wallpapers there are shown by the \
             shell extension, and it needs a file on disk. Choose the “download” mode.",
            &[&shell_ext_de_label(de)],
        ))];
    }
    if session_type == "wayland" {
        if !has(tools, "mpvpaper") {
            return vec![fail(mpvpaper_missing_message(&hints.mpvpaper))];
        }
        let audio_opt = if audio { "" } else { " no-audio" };
        let vol_opt = if audio {
            format!(" volume={volume}")
        } else {
            String::new()
        };
        let opts = format!("loop=inf hwdec=auto ytdl-format={fmt}{audio_opt}{vol_opt}");
        return vec![Step::new(
            step_kind::SPAWN,
            "mpvpaper",
            vec![
                "mpvpaper".into(),
                "-o".into(),
                opts,
                monitor.to_string(),
                url.to_string(),
            ],
        )
        .ok(tr("YouTube wallpaper started"))
        .err({
            // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
            tr("mpvpaper error: {stderr}")
        })
        .fmt("youtube")
        .terminal()];
    }

    if !has(tools, "xwinwrap") {
        return vec![fail(xwinwrap_missing_message(&hints.xwinwrap))];
    }
    if !has(tools, "mpv") {
        return vec![fail(tr("mpv isn't installed."))];
    }
    let mut cmd = vec![
        "xwinwrap".to_string(),
        "-g".to_string(),
        xwinwrap_geometry(resolution),
        "-ov".into(),
        "-ni".into(),
        "-s".into(),
        "-nf".into(),
        "--".into(),
        "mpv".into(),
        "--loop=inf".into(),
        format!("--ytdl-format={fmt}"),
    ];
    if !audio {
        cmd.push("--no-audio".into());
    } else {
        cmd.push(format!("--volume={volume}"));
    }
    cmd.extend(argv(&[
        "--no-osc",
        "--no-osd-bar",
        "--no-input-default-bindings",
        "--hwdec=auto",
        "--gpu-api=auto",
        "--wid=WID",
    ]));
    cmd.push(url.to_string());
    vec![Step::new(step_kind::SPAWN, "xwinwrap", cmd)
        .ok(tr("YouTube wallpaper started"))
        .err({
            // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
            tr("xwinwrap error: {stderr}")
        })
        .fmt("youtube")
        .terminal()]
}

/// Нет yt-dlp — без него видео с YouTube не скачать.
pub fn youtube_no_ytdlp_message(hint: &str) -> String {
    // TRANSLATORS: %1 is a hint on how to install yt-dlp.
    trf("The yt-dlp program wasn't found; YouTube videos can't be downloaded without it. %1", &[&hint])
}

/// Ссылка не с поддерживаемого сайта.
pub fn youtube_unsupported_message(url: &str) -> String {
    trf("Unsupported URL: %1", &[&url])
}

/// Загрузка не удалась.
pub fn youtube_download_failed() -> String {
    tr("Couldn't download the YouTube video. Try another quality or the streaming mode.")
}

/// То же, но с поправкой на окружение.
///
/// ⚠️ Голый [`YOUTUBE_DOWNLOAD_FAILED`] на GNOME и Cinnamon-Wayland советует
/// невозможное: потока там нет вовсе — обои показывает расширение оболочки,
/// а ему нужен файл на диске, поэтому «стрим» и заменяется скачиванием
/// ([`youtube_effective_mode`]). Человек, послушавшись совета, переключил бы
/// режим на «стрим», снова получил бы скачивание — и ходил бы по кругу.
pub fn youtube_download_failed_message(de: &str, session_type: &str) -> String {
    if !needs_shell_extension(de, session_type) {
        return youtube_download_failed();
    }
    let de = shell_ext_de_label(de);
    trf(
        "Couldn't download the YouTube video. Try another quality or another link: \
         on %1 the video can't be streamed — wallpapers there are shown by the shell \
         extension, and it needs a file on disk.",
        &[&de],
    )
}

// ── веб ────────────────────────────────────────────────────────────────────

/// Путь/URL — это ссылка?
pub fn is_url(path: &str) -> bool {
    path.starts_with("http://") || path.starts_with("https://")
}

/// Хоста обоев в системе нет — объяснение покупателю, выбравшему веб-обои.
///
/// ⚠️ Текст именно такой, а не «Ошибка web_wallpaper: код 127». Отказ
/// выносится ДО запуска, потому что `None` здесь означает не «путь не
/// передали», а «ядро уже искало и не нашло ни в дереве сборки, ни в
/// `$PATH`» ([`find_wallpaper_host_binary`]). Спавнить в такой обстановке —
/// это подарить человеку мигание пустого стола и код возврата вместо ответа
/// (правило #13). Отката на Python нет: модуль `rexon.web_wallpaper` удалён
/// из дерева 30.07.2026.
pub fn web_host_missing() -> String {
    tr("There's nothing to show web wallpapers with: the `rexon-wallpaper-host` \
        program wasn't found next to Rexon or in PATH. It's built together with Rexon — \
        reinstall the program or build it again.")
}

/// План веб-обоев: сначала свой хост на QtWebEngine, на X11 — ещё surf.
///
/// ⚠️ `python` больше не участвует в первом шаге и оставлен в подписи
/// намеренно: её знает питоновская сторона движка, а менять её сейчас
/// нельзя. Веб-обои запускает наш бинарь.
///
/// `host` — путь к нему ([`find_wallpaper_host_binary`]); `None` — его нет нигде,
/// и первым шагом идёт честный отказ, а не запуск несуществующего.
pub fn web_plan(
    session_type: &str,
    python: &str,
    path: &str,
    abs_path: &str,
    tools: &[String],
    resolution: &str,
    hints: &Hints,
    host: Option<&str>,
) -> Vec<Step> {
    let _ = python;
    let mut steps = vec![match host {
        Some(bin) => Step::new(
            step_kind::SPAWN_SESSION,
            "web_wallpaper",
            wallpaper_host_argv(bin, HostMode::Web, path),
        )
        .ok(tr("Web wallpaper started"))
        .err({
            // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
            tr("web_wallpaper error: {stderr}")
        })
        .fmt("web"),
        None => fail(web_host_missing()).fmt("web"),
    }];

    if session_type == "wayland" {
        // На Wayland свой хост — единственный путь: xwinwrap там не работает.
        steps[0] = steps[0].clone().terminal();
        return steps;
    }
    if has(tools, "surf") && has(tools, "xwinwrap") {
        // surf принимает и file:// URI, и http(s) URL.
        let uri = if is_url(path) {
            path.to_string()
        } else {
            format!("file://{abs_path}")
        };
        steps.push(
            Step::new(
                step_kind::SPAWN,
                "surf",
                vec![
                    "xwinwrap".into(),
                    "-g".into(),
                    xwinwrap_geometry(resolution),
                    "-ov".into(),
                    "-ni".into(),
                    "-s".into(),
                    "-nf".into(),
                    "--".into(),
                    "surf".into(),
                    "-dfgkKmns".into(),
                    uri,
                ],
            )
            .ok(tr("Web wallpaper started"))
            .err({
                // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
                tr("Couldn't start surf: {stderr}")
            })
            .fmt("web")
            .terminal(),
        );
    } else {
        let hint = if hints.webkit.is_empty() {
            tr("(the webkit2gtk-4.1 package)")
        } else {
            hints.webkit.clone()
        };
        // TRANSLATORS: %1 is a hint on which package to install.
        steps.push(fail(trf("Web wallpapers need the WebKitGTK engine %1", &[&hint])));
    }
    steps
}

/// HTML-файл не найден.
pub fn web_missing_message(path: &str) -> String {
    trf("HTML file not found: %1", &[&path])
}

// ── сцена ──────────────────────────────────────────────────────────────────

/// Обои-сцены сняты с поддержки — ровно как обои-приложения ниже.
///
/// Собственного сценового рендера у Rexon нет и не будет: `parallax_engine`
/// (и весь класс `scene_gl`/`scene_mesh`) убран из дерева осознанно, сцены
/// рисует движок waywallen. Прежний код всё равно запускал
/// `python -m rexon.parallax_engine`: процесс умирал сразу, а покупателю
/// рапортовали «Scene Engine запущен».
///
/// Текст говорит покупателю ровно то, что правда: свой формат сцены (слои
/// `.rxn`) показать нечем, а сцены Мастерской открываются во вкладке
/// «Wallpaper Engine». Обещать, что ЭТОТ файл заработает там, нельзя:
/// waywallen применяет обои по идентификатору своей библиотеки
/// (`apply_by_id`), произвольный путь ему не передать.
pub fn scene_unsupported() -> String {
    tr("Scene wallpapers are no longer supported. Scenes from the Wallpaper Engine \
        Workshop open in the “Wallpaper Engine” tab")
}

// ── Flash (SWF) ────────────────────────────────────────────────────────────

/// Ruffle из CDN — запасной путь, когда нативного бинаря нет.
pub const RUFFLE_CDN: &str = "https://unpkg.com/@ruffle-rs/ruffle";

/// SWF-файл не найден.
pub fn swf_missing_message(path: &str) -> String {
    trf("SWF file not found: %1", &[&path])
}

/// Flash-обои своим хостом: `rexon-wallpaper-host --mode swf <файл>` своим
/// сеансом — Ruffle как библиотека (крейт `rexon-swf`), без внешнего
/// `ruffle`, без скачивания и без HTML-обёртки. Строку запуска собирает та же
/// функция, что у видео и веба ([`wallpaper_host_argv`]).
///
/// ⚠️ Звуковых ключей строка не несёт, и это НЕ значит «немо» (с 16.09.2026).
/// Громкость и «звук вкл/выкл» живут в `settings.json` (`wallpaper_audio_*`)
/// — тех же ключах, что у видео, — а хост следит за файлом и применяет
/// правку на ходу. Ключом их передавать нельзя по той же причине, что у
/// видео: настройка дошла бы до обоев ровно один раз, при старте процесса.
/// Нелинейность шкалы Ruffle (0,5 там даёт уровень примерно в 10 раз тише
/// 1,0) снята переводом внутри `rexon-swf` и наружу не выходит.
pub fn swf_host_plan(host: &str, path: &str) -> Vec<Step> {
    let cmd = wallpaper_host_argv(host, HostMode::Swf, path);
    let name = base_name(path);
    vec![Step::new(step_kind::SPAWN_SESSION, "swf_wallpaper", cmd)
        .ok(trf("Flash wallpaper started: %1", &[&name]))
        .err({
            // TRANSLATORS: keep {stderr} as is: the program replaces it with the error output.
            tr("Couldn't start the Flash wallpaper: {stderr}")
        })
        .fmt("swf")
        .terminal()]
}

/// Запуск внешнего Ruffle в полноэкранном режиме.
///
/// ⚠️ Путь КЛАССИЧЕСКОГО окна (Python `_apply_swf` через мост
/// `wpe_swf_native_plan`). Rust-исполнитель показывает Flash своим хостом —
/// [`swf_host_plan`]. Функция живёт, пока её зовёт Python.
pub fn swf_native_plan(ruffle: &str, swf_abs: &str) -> Vec<Step> {
    vec![Step::new(
        step_kind::SPAWN,
        "ruffle",
        argv(&[ruffle, "--fullscreen", swf_abs]),
    )
    .ok(tr("SWF wallpaper started"))
    .fmt("swf")]
}

/// Две команды xprop, делающие чужое окно рабочим столом на X11.
pub fn xprop_desktop_argvs(wid: &str) -> Vec<Vec<String>> {
    vec![
        argv(&[
            "xprop", "-id", wid, "-f", "_NET_WM_WINDOW_TYPE", "32a", "-set",
            "_NET_WM_WINDOW_TYPE", "_NET_WM_WINDOW_TYPE_DESKTOP",
        ]),
        argv(&[
            "xprop",
            "-id",
            wid,
            "-f",
            "_NET_WM_STATE",
            "32a",
            "-set",
            "_NET_WM_STATE",
            "_NET_WM_STATE_BELOW,_NET_WM_STATE_SKIP_TASKBAR,\
             _NET_WM_STATE_SKIP_PAGER,_NET_WM_STATE_STICKY",
        ]),
    ]
}

/// Поиск окна по PID для xprop-трика.
pub fn xdotool_search_argv(pid: i64) -> Vec<String> {
    vec![
        "xdotool".into(),
        "search".into(),
        "--pid".into(),
        pid.to_string(),
    ]
}

/// Откуда взять Ruffle Web для HTML-обёртки: своя копия → копия из .rxn →
/// CDN (нужен интернет).
pub fn swf_ruffle_src(rexon_copy: Option<&str>, local_copy: Option<&str>) -> String {
    if let Some(p) = rexon_copy {
        return format!("file://{p}");
    }
    if let Some(p) = local_copy {
        return format!("file://{p}");
    }
    RUFFLE_CDN.to_string()
}

/// HTML-обёртка вокруг SWF: Ruffle WASM внутри нашего веб-хоста.
pub fn swf_wrapper_html(swf_name: &str, ruffle_src: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">
<title>Rexon SWF Wallpaper</title>
<style>
  * {{ margin: 0; padding: 0; overflow: hidden; }}
  html, body {{ width: 100vw; height: 100vh; background: #000; }}
  #player {{
    width: 100vw;
    height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
  }}
  ruffle-embed, ruffle-object, ruffle-player, embed, object {{
    width: 100% !important;
    height: 100% !important;
  }}
</style>
</head><body>
<div id="player">
  <embed src="{swf_name}"
         type="application/x-shockwave-flash"
         width="100%" height="100%"
         quality="high" wmode="transparent"
         allowScriptAccess="never"
         loop="true" play="true"
         scale="showAll">
</div>
<script src="{ruffle_src}"></script>
</body></html>"#
    )
}

// ── KDE: скрипты plasmashell и конфиг столов ───────────────────────────────

/// Штатный статичный плагин обоев Plasma.
pub const KDE_IMAGE_PLUGIN: &str = "org.kde.image";

/// Плагин обоев Rexon, удалённый 2026-07-28. Имя нужно только чтобы
/// распознать и вылечить конфиг старой версии: живые обои на KDE рисует
/// движок waywallen.
pub const REMOVED_KDE_PLUGIN: &str = "org.rexon.wallpaper";

/// Скрипт установки статичной картинки на все столы.
pub fn kde_static_script(path: &str) -> String {
    format!(
        "\nvar a = desktops();\nfor (var i = 0; i < a.length; i++) {{\n    \
         a[i].wallpaperPlugin = \"org.kde.image\";\n    \
         a[i].currentConfigGroup = [\"Wallpaper\",\"org.kde.image\",\"General\"];\n    \
         a[i].writeConfig(\"Image\", \"file://{path}\");\n}}\n"
    )
}

/// Скрипт возврата столов на штатный плагин с чистой картинкой.
///
/// ПОРЯДОК ВАЖЕН: сначала пишем чистую картинку в in-memory конфиг
/// `org.kde.image`, ПОТОМ переключаем плагин — иначе плагин грузится со
/// старым (блюр-кадром) Image из памяти plasmashell.
pub fn kde_reset_script(clean_image: &str) -> String {
    let img_js = if clean_image.is_empty() {
        String::new()
    } else {
        format!(
            "  d[i].currentConfigGroup = ['Wallpaper','org.kde.image','General'];\
             \x20 d[i].writeConfig('Image', {});",
            serde_json::Value::String(clean_image.to_string())
        )
    };
    format!(
        "var d = desktops();for (var i = 0; i < d.length; i++) {{{img_js}\
         \x20 d[i].wallpaperPlugin = 'org.kde.image';}}"
    )
}

/// Скрипт возврата столу чужого плагина обоев.
pub fn kde_restore_plugin_script(plugin: &str) -> String {
    format!(
        "var d = desktops();for (var i = 0; i < d.length; i++) {{  \
         d[i].wallpaperPlugin = '{plugin}';}}"
    )
}

/// Скрипт чтения плагинов обоев ВСЕХ рабочих столов — живьём, а не из
/// конфига: файл на диске отстаёт, plasmashell держит настройки в памяти и
/// пишет их когда захочет.
///
/// Один опрос отвечает движку сразу на два вопроса: чей это стол (кому его
/// вернуть после «Остановить») и не остался ли на нём удалённый плагин
/// Rexon. Столов может быть несколько, поэтому печатается СПИСОК через
/// запятую, а не первый попавшийся: раньше бралcя `d[0]`, и застрявший
/// плагин на втором мониторе оставался незамеченным.
///
/// Номер контейнмента здесь не называется вовсе — `desktops()` отдаёт ровно
/// рабочие столы. Это и есть лечение того класса дефекта, на котором
/// обжигались с картинкой обоев: номер у каждой машины свой, и чтение по
/// зашитому возвращало пустоту вместо ответа.
pub const KDE_READ_PLUGIN_SCRIPT: &str =
    "var d = desktops(); var p = []; for (var i = 0; i < d.length; i++) \
     p.push(d[i].wallpaperPlugin); print(p.join(','));";

/// Скрипт сброса плагина на штатный (без картинки) — для восстановления.
pub const KDE_PLAIN_RESET_SCRIPT: &str =
    "var allDesktops = desktops();for (var i = 0; i < allDesktops.length; i++) {\
     \x20 allDesktops[i].wallpaperPlugin = 'org.kde.image';}";

/// Вызов `evaluateScript` через `dbus-send`.
pub fn plasma_dbus_argv(script: &str) -> Vec<String> {
    vec![
        "dbus-send".into(),
        "--session".into(),
        "--dest=org.kde.plasmashell".into(),
        "--type=method_call".into(),
        "/PlasmaShell".into(),
        "org.kde.PlasmaShell.evaluateScript".into(),
        format!("string:{script}"),
    ]
}

/// Тот же вызов, но с ответом (`--print-reply`).
pub fn plasma_dbus_reply_argv(script: &str) -> Vec<String> {
    let mut v = plasma_dbus_argv(script);
    v.insert(2, "--print-reply".into());
    v
}

/// Строковое поле из ответа `dbus-send`.
pub fn parse_dbus_string(stdout: &str) -> Option<String> {
    let idx = stdout.find("string \"")?;
    let rest = &stdout[idx + "string \"".len()..];
    let end = rest.find('"')?;
    let name = rest[..end].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Плагины обоев рабочих столов KDE из ответа `dbus-send` на
/// [`KDE_READ_PLUGIN_SCRIPT`].
///
/// Единственный разбор этого ответа: и «чей стол», и «не остался ли
/// удалённый плагин» считаются отсюда, чтобы два вопроса не разъезжались
/// по двум способам чтения. Пустой ответ (plasmashell молчит) — пустой
/// список: молчание не выдаётся за «плагинов нет».
pub fn kde_desktop_plugins(dbus_stdout: &str) -> Vec<String> {
    parse_dbus_string(dbus_stdout)
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// ЧУЖОЙ плагин обоев стола — тот, которому стол надо вернуть после
/// остановки живых обоев (`None`, если возвращать нечего).
///
/// Отсеиваются ДВА имени, а не одно. Штатный [`KDE_IMAGE_PLUGIN`] — потому
/// что к нему остановка возвращается и так. И [`REMOVED_KDE_PLUGIN`] —
/// потому что его в конфиге больше нечем нарисовать: запомнив удалённый
/// плагин, остановка сначала честно сбрасывала стол на `org.kde.image`, а
/// потом сама же ставила мёртвый плагин обратно, и рабочий стол оставался
/// пустым.
pub fn kde_foreign_wallpaper_plugin(dbus_stdout: &str) -> Option<String> {
    kde_desktop_plugins(dbus_stdout)
        .into_iter()
        .find(|p| p != KDE_IMAGE_PLUGIN && p != REMOVED_KDE_PLUGIN)
}

/// Стоит ли хоть на одном столе УДАЛЁННЫЙ плагин обоев Rexon.
///
/// У пользователя, обновившегося со старой версии, стол мог остаться
/// настроенным на `org.rexon.wallpaper`: Plasma грузит несуществующий
/// плагин, и рабочий стол пустой. Спрашиваем у самой Plasma, а не у
/// `kreadconfig6` по номеру контейнмента — тем чтением уже обжигались.
pub fn kde_removed_plugin_selected(dbus_stdout: &str) -> bool {
    kde_desktop_plugins(dbus_stdout)
        .iter()
        .any(|p| p == REMOVED_KDE_PLUGIN)
}

/// Идентификаторы десктоп-контейнментов из `plasma-…-appletsrc`.
///
/// Проверяем `wallpaperplugin` ИМЕННО у них — иначе застрявший плагин в
/// неактивном контейнменте даёт ложное срабатывание. Ничего не нашлось —
/// исторический `128`.
pub fn desktop_containment_ids(text: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(id) = line
            .strip_prefix("[Containments][")
            .and_then(|r| r.strip_suffix(']'))
            .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
        {
            current = Some(id.to_string());
        } else if let (Some(cur), true) = (current.as_ref(), line.starts_with("plugin=")) {
            if (line.contains("folder") || line.contains("desktopcontainment"))
                && !ids.contains(cur)
            {
                ids.push(cur.clone());
            }
        }
    }
    if ids.is_empty() {
        ids.push("128".to_string());
    }
    ids
}

/// Картинки обоев ВСЕХ рабочих столов KDE из
/// `plasma-org.kde.plasma.desktop-appletsrc`.
///
/// Номер контейнмента здесь НЕ угадывается: берутся все группы
/// `[Containments][N][Wallpaper][org.kde.image][General]` с ключом `Image`,
/// в порядке файла. Это тот же приём, что у [`desktop_containment_ids`], и
/// он надёжнее чтения по зашитому номеру `128`: на машине Wehrwolfmann
/// рабочий стол имеет номер `216`, и запрос по `128` возвращал пустую
/// строку. Команду чтения по зашитому номеру движок больше не собирает
/// вовсе — она ушла вместе с последним, кто её звал.
///
/// Каталог оформления спрашивает отсюда, применена ли пачка обоев прямо
/// сейчас (`installed_scan::wallpaper_pack_name`).
pub fn kde_wallpaper_images(appletsrc: &str) -> Vec<String> {
    kde_wallpaper_image_entries(appletsrc)
        .into_iter()
        .map(|(_, image)| image)
        .collect()
}

/// Пары «номер контейнмента → его картинка `org.kde.image`», в порядке файла.
///
/// Общий разбор для [`kde_wallpaper_images`] и [`kde_desktop_static_image`]:
/// первому нужны все картинки, второму — ещё и чей это контейнмент.
fn kde_wallpaper_image_entries(appletsrc: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in appletsrc.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            current = s
                .strip_prefix("[Containments][")
                .and_then(|r| r.strip_suffix("][Wallpaper][org.kde.image][General]"))
                .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
                .map(|id| id.to_string());
            continue;
        }
        let Some(id) = current.as_ref() else {
            continue;
        };
        if s.starts_with('#') {
            continue;
        }
        if let Some(v) = s.strip_prefix("Image=") {
            let v = v.trim();
            if !v.is_empty() {
                out.push((id.clone(), v.to_string()));
            }
        }
    }
    out
}

/// Статичная картинка РАБОЧЕГО СТОЛА — то, к чему возвращаемся после
/// остановки живых обоев.
///
/// Живые обои меняют `wallpaperplugin` контейнмента, но группа
/// `org.kde.image` остаётся в конфиге нетронутой: там и лежит настоящий
/// обой пользователя. Берём его у контейнмента, который [`desktop_containment_ids`]
/// признал столом: панель тоже получает от Plasma свою группу
/// `org.kde.image` с ключом `Image`, и «первая картинка в файле» выбрала бы
/// её, а не стол.
///
/// Схема `file://` снимается: Plasma пишет в конфиг URL, а движок обоев
/// всюду обращается с сохранённым обоем как с путём в файловой системе —
/// проверяет `os.path.exists` и подставляет в строку запуска. С URL эта
/// проверка не срабатывала, и возврат после остановки тихо не делался.
pub fn kde_desktop_static_image(appletsrc: &str) -> Option<String> {
    let entries = kde_wallpaper_image_entries(appletsrc);
    let desktops = desktop_containment_ids(appletsrc);
    entries
        .iter()
        .find(|(id, _)| desktops.contains(id))
        .or_else(|| entries.first())
        .map(|(_, image)| image.replace("file://", ""))
}

/// Наш кеш в пути картинки = это блюр-кадр, а не настоящие обои.
pub const CACHE_MARKER: &str = "/.cache/rexon/";

/// Годится ли кандидат «чистой» картинкой для возврата после остановки.
///
/// `exists` — наблюдение шима: файл (или каталог пакета обоев) на месте.
pub fn clean_image_candidate_ok(candidate: &str, exists: bool) -> bool {
    exists && !candidate.replace("file://", "").contains(CACHE_MARKER)
}

/// Каталоги обоев дистрибутива — запасной вариант для возврата.
pub const DISTRO_WALLPAPER_DIRS: &[&str] = &[
    "/usr/share/wallpapers/Next",
    "/usr/share/wallpapers/cachyos-wallpapers",
];

/// Где лежат пакеты обоев Plasma — последний запасной вариант возврата,
/// когда ни сохранённого обоя, ни каталогов дистрибутива не нашлось.
///
/// Пакетом считается подкаталог с `contents/images` внутри: имя каталога
/// само по себе ни о чём не говорит, а без картинок Plasma получит пустой
/// стол — то есть ровно то, от чего возврат и страхует.
pub const PLASMA_WALLPAPER_ROOT: &str = "/usr/share/wallpapers";

// ── блюр и кадр ────────────────────────────────────────────────────────────

/// Расширения, для которых кадр берётся копированием, а не через ffmpeg.
pub const IMAGE_EXTS: &[&str] = &[".png", ".jpg", ".jpeg", ".webp", ".bmp"];

/// Извлечение одного кадра: `timestamp` по умолчанию 3 с — пропуск заставок.
pub fn extract_frame_argv(path: &str, output: &str, timestamp: f64) -> Vec<String> {
    vec![
        "ffmpeg".into(),
        "-y".into(),
        "-ss".into(),
        format_float(timestamp),
        "-i".into(),
        path.to_string(),
        "-vframes".into(),
        "1".into(),
        "-q:v".into(),
        "2".into(),
        output.to_string(),
    ]
}

/// Куда класть размытую копию обоев: `<кеш>/rexon/blur/<имя>_blur.jpg`.
///
/// ⭐ Починка: Python писал копию РЯДОМ с исходником ([`blur_output_path`]),
/// а обои дистрибутива лежат в `/usr/share/…`, куда писать нельзя, — и
/// «Размыть» на стандартных обоях не работало вовсе. Кеш Rexon к тому же
/// опознаётся [`CACHE_MARKER`]: остановка не вернёт на стол размытую копию
/// вместо настоящих обоев.
pub fn blur_cache_output_path(cache_home: &str, source: &str) -> String {
    let name = source.rsplit('/').next().unwrap_or(source);
    let (stem, _) = split_ext(name);
    format!("{}/rexon/blur/{stem}_blur.jpg", cache_home.trim_end_matches('/'))
}

/// Размыть картинку и записать JPEG — порт `_blur_image` без Pillow.
///
/// Как у оригинала: большая картинка сперва уменьшается в пределы
/// [`BLUR_MAX_SIZE`] Ланцошем (детали всё равно скроет размытие), затем
/// размытие по Гауссу с силой `radius`, JPEG качества [`BLUR_JPEG_QUALITY`].
pub fn blur_image_file(source: &str, output: &str, radius: i64) -> Result<(), String> {
    let img = image::ImageReader::open(source)
        .map_err(|e| trf("couldn't open the image %1: %2", &[&source, &e]))?
        .with_guessed_format()
        .map_err(|e| trf("couldn't read the image %1: %2", &[&source, &e]))?
        .decode()
        .map_err(|e| trf("couldn't decode the image %1: %2", &[&source, &e]))?;
    let (max_w, max_h) = (BLUR_MAX_SIZE.0.max(1) as u32, BLUR_MAX_SIZE.1.max(1) as u32);
    let img = if img.width() > max_w || img.height() > max_h {
        img.resize(max_w, max_h, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let blurred = img.blur(radius.max(1) as f32).to_rgb8();
    if let Some(dir) = std::path::Path::new(output).parent() {
        std::fs::create_dir_all(dir).map_err(|e| trf("couldn't create the cache folder: %1", &[&e]))?;
    }
    let file = std::fs::File::create(output).map_err(|e| trf("couldn't create %1: %2", &[&output, &e]))?;
    let mut writer = std::io::BufWriter::new(file);
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
        &mut writer,
        BLUR_JPEG_QUALITY.clamp(1, 100) as u8,
    );
    blurred
        .write_with_encoder(encoder)
        .map_err(|e| trf("couldn't write %1: %2", &[&output, &e]))
}

/// Имя файла размытой копии: `<имя>_blur.jpg` рядом с исходником.
pub fn blur_output_path(path: &str) -> String {
    let (stem, _) = split_ext(path);
    format!("{stem}_blur.jpg")
}

/// Максимальный размер, до которого уменьшается кадр перед размытием
/// (детали всё равно скроет блюр).
pub const BLUR_MAX_SIZE: (i64, i64) = (1920, 1080);
/// Качество JPEG размытого кадра.
pub const BLUR_JPEG_QUALITY: i64 = 85;

// ── faststart ──────────────────────────────────────────────────────────────

/// Расширения, у которых проверяется положение атома `moov`.
pub const FASTSTART_EXTS: &[&str] = &[".mp4", ".mov", ".m4v"];

/// Идёт ли `moov` ДО `mdat` в начале файла.
///
/// Неизвестно/ошибка → `true`: файл не трогаем. Нужно потому, что
/// QtMultimedia на больших mp4 с `moov` в конце застревает в
/// `BufferingMedia` и декодит 0 кадров — чёрный экран без звука.
pub fn moov_at_front(header: &[u8]) -> bool {
    let mut off = 0usize;
    for _ in 0..6 {
        if off + 8 > header.len() {
            break;
        }
        let size = u32::from_be_bytes([
            header[off],
            header[off + 1],
            header[off + 2],
            header[off + 3],
        ]) as usize;
        let atom = &header[off + 4..off + 8];
        if atom == b"moov" {
            return true;
        }
        if atom == b"mdat" {
            return false;
        }
        if size < 8 {
            break;
        }
        off += size;
    }
    true
}

/// Сколько байт заголовка читать для [`moov_at_front`].
pub const MOOV_HEADER_BYTES: usize = 64;

/// Ключ кеша faststart: путь + размер + время правки.
pub fn faststart_key(abs_path: &str, size: u64, mtime: i64) -> String {
    let digest = sha1_hex(format!("{abs_path}:{size}:{mtime}").as_bytes());
    digest[..16].to_string()
}

/// Lossless-ремукс: `moov` переносится в начало файла (~0,3 с, без
/// перекодирования).
pub fn faststart_argv(path: &str, out: &str) -> Vec<String> {
    argv(&[
        "ffmpeg", "-v", "error", "-y", "-i", path, "-c", "copy", "-movflags",
        "+faststart", out,
    ])
}

/// Таймаут ремукса, секунд.
pub const FASTSTART_TIMEOUT_SEC: u64 = 180;

// ── зависимости и валидация ────────────────────────────────────────────────

/// Бинари, о которых движок докладывает в `check_dependencies`.
/// `vlc` — это `cvlc` ИЛИ `vlc`, `pillow` считает Python.
pub const DEP_TOOLS: &[&str] = &[
    "mpv", "mpvpaper", "xwinwrap", "swww", "hyprpaper", "surf", "feh",
    "fc-cache", "yt-dlp", "ffmpeg",
];

/// Чего не хватает для формата. `deps` — ответ `check_dependencies`.
pub fn missing_for_format(fmt: &str, session_type: &str, deps: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut missing = Vec::new();
    match fmt {
        "video" => {
            if session_type == "wayland" {
                if !deps("mpvpaper") {
                    missing.push("mpvpaper".to_string());
                }
            } else {
                if !deps("xwinwrap") {
                    missing.push("xwinwrap-git (AUR)".to_string());
                }
                if !deps("mpv") {
                    missing.push("mpv".to_string());
                }
            }
        }
        "youtube" => {
            if !deps("yt-dlp") {
                missing.push("yt-dlp".to_string());
            }
            if !deps("ffmpeg") {
                // TRANSLATORS: an item in a list of missing programs.
                missing.push(trf("%1 (recommended)", &[&"ffmpeg"]));
            }
            missing.extend(missing_for_format("video", session_type, deps));
        }
        "animated" => {
            if session_type == "wayland" {
                if !deps("swww") && !deps("mpvpaper") {
                    // TRANSLATORS: an item in a list of missing programs.
                    missing.push(trf("%1 (recommended) or %2", &[&"swww", &"mpvpaper"]));
                }
            } else {
                if !deps("xwinwrap") {
                    missing.push("xwinwrap-git (AUR)".to_string());
                }
                if !deps("mpv") {
                    missing.push("mpv".to_string());
                }
            }
        }
        // Веб и Flash показывает наш хост (`rexon-wallpaper-host --mode web|swf`,
        // план [`web_plan`] / [`swf_host_plan`]): внешних программ им не нужно
        // ни на Wayland, ни на X11. surf с xwinwrap у веба на X11 — только
        // запасной путь, когда хоста нет, а отсутствие самого хоста говорится
        // при применении ([`WEB_HOST_MISSING`], [`VIDEO_HOST_MISSING_MESSAGE`]),
        // а не списком «доустановите».
        "web" | "swf" => {}
        _ => {}
    }
    missing
}

/// Расширения, которые движок берёт как видео.
pub const VALID_VIDEO_EXTS: &[&str] = &[
    ".avi", ".bik", ".bk2", ".flv", ".gif", ".m4v", ".mkv", ".mov", ".mp4",
    ".mpeg", ".mpg", ".webm", ".wmv",
];

/// Проверка видеофайла. `exists` и `size_bytes` — наблюдения шима.
/// Предел размера видеофайла обоев, МБ — то же число, что у Python
/// (`max_file_size_mb`).
pub const MAX_VIDEO_FILE_MB: i64 = 1024;

pub fn validate_video(
    path: &str,
    exists: bool,
    size_bytes: u64,
    max_mb: i64,
) -> (bool, String) {
    if !exists {
        return (false, tr("File not found"));
    }
    let (_, ext) = split_ext(path);
    let ext = ext.to_lowercase();
    if !VALID_VIDEO_EXTS.contains(&ext.as_str()) {
        return (false, trf("Unsupported format: %1", &[&ext]));
    }
    let size_mb = size_bytes as f64 / (1024.0 * 1024.0);
    if size_mb > max_mb as f64 {
        return (
            false,
            // TRANSLATORS: %1 is the file size, %2 the limit, both in megabytes.
            trf("The file is too large: %1 MB (max. %2 MB)", &[&format!("{size_mb:.1}"), &max_mb]),
        );
    }
    (true, "OK".to_string())
}

/// Обои-приложения (чужой .exe через Wine/Proton) сняты с поддержки:
/// свой оконный хост под них был бы вторым движком обоев рядом с waywallen.
pub fn application_unsupported() -> String {
    tr("Application wallpapers are no longer supported")
}

/// Форматы, которые ДВИЖУТСЯ: их показывает отдельный живой процесс.
///
/// Статичная картинка сюда не входит — она просто записывается в настройки
/// окружения и никакого процесса за собой не оставляет.
pub const LIVE_FORMATS: &[&str] = &["animated", "video", "youtube", "web", "swf"];

/// Живые ли это обои — единственное место, где решается вопрос.
///
/// Спрашивают и движок (о чём сообщать наружу через мост), и всё, что
/// захочет отличать «идёт живая картинка» от «лежит статичная».
pub fn is_live_format(fmt: &str) -> bool {
    LIVE_FORMATS.contains(&fmt)
}

/// Сколько секунд движок молчит про живые обои после собственной вести.
///
/// ⚠️ Защита от пачек. В витрине обои перебирают глазами, и КАЖДОЕ нажатие
/// проходит через движок целиком: прежние обои снимаются, новые ставятся.
/// Без порога десяток нажатий за десяток секунд оставил бы в каталоге команд
/// моста два десятка файлов — мусор, который никто не прочтёт: у читающей
/// стороны своя память, и после первой же реплики она молчит минутами.
/// Значит, из пачки имеет смысл только ПЕРВАЯ весть, и порог отсекает
/// остальные по переднему фронту — весть уходит сразу, молчание идёт после.
///
/// Пять секунд: осмысленный перебор в витрине — одно-два нажатия в секунду,
/// столько и укрывает порог; при этом он много меньше собственной памяти
/// читающей стороны, так что ни одной вести, на которую та могла бы
/// отозваться, порог не съедает. Признак `video_wallpaper_active` в состоянии
/// моста порогом не затрагивается вовсе — он пишется всегда и остаётся
/// запасным путём для того, кто вести пропустил.
pub const BRIDGE_QUIET_SEC: f64 = 5.0;

/// Рассказывать ли мосту о живых обоях прямо сейчас.
///
/// `told_*` — то, что мосту УЖЕ сказано вестью (не то, что с обоями на самом
/// деле: подмену и подавленную пачку мост вестью не видел). `told_at` — момент
/// той вести по монотонным часам, `None` — не говорили ещё ни разу.
///
/// Молчим в двух случаях: сказать нечего (состояние и название те же) либо
/// с прошлой вести не прошло [`BRIDGE_QUIET_SEC`].
pub fn bridge_should_announce(
    live: bool,
    name: &str,
    told_live: bool,
    told_name: &str,
    told_at: Option<f64>,
    now: f64,
) -> bool {
    if live == told_live && name == told_name {
        return false;
    }
    match told_at {
        Some(at) => now - at >= BRIDGE_QUIET_SEC,
        None => true,
    }
}

/// Имя формата обоев ДЛЯ ПОКУПАТЕЛЯ.
///
/// Внутренние метки планов (`video`, `swf`, …) в окно не показываются:
/// это наши служебные слова, а не то, что человек выбирал. Имена
/// технологий остаются латиницей — `YouTube`, `GIF`, `Flash`, `SWF`
/// пишутся так и в самом Rexon, и в оригинале.
pub fn format_label(fmt: &str) -> String {
    match fmt {
        // TRANSLATORS: wallpaper format, lowercase: it's used inside sentences
        // (“Wallpaper stopped (format was: video)”) and capitalized by the program.
        "static" => trc("wallpaper format", "static image"),
        "animated" => trc("wallpaper format", "GIF animation"),
        "video" => trc("wallpaper format", "video"),
        "youtube" => trc("wallpaper format", "YouTube video"),
        "web" => trc("wallpaper format", "web page"),
        "swf" => "Flash (SWF)".to_string(),
        "scene" => trc("wallpaper format", "scene"),
        "application" => trc("wallpaper format", "application"),
        // Формат, о котором ядро ещё не знает: показать как есть честнее,
        // чем подставить чужое имя.
        other => other.to_string(),
    }
}

/// Имя формата обоев заголовком — с заглавной буквы: подпись на карточке
/// библиотеки («Статичная картинка», «Видео», «Веб-страница»).
///
/// Таблица имён одна — [`format_label`]; здесь только заглавная буква, чтобы
/// в продукте не появилось второго словаря форматов. Логика (показывать ли
/// просмотр, чем применять) по-прежнему опирается на служебное значение.
pub fn format_title(fmt: &str) -> String {
    let label = format_label(fmt);
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => label,
    }
}

/// Итог «Остановить».
///
/// `fmt` — формат, который ИГРАЛ до остановки; `None` — не играло ничего.
///
/// ⚠️ Формат сюда обязан приходить снятым ДО возврата обоев. Возврат
/// ставит статичную картинку тем же планом, что и обычное применение, а
/// тот записывает в состояние движка свой формат — и сообщение, собранное
/// после возврата, называло `static` даже после видео- и YouTube-обоев.
/// Пустое состояние оно же выдавало покупателю как «был формат: None».
pub fn stopped_message(fmt: Option<&str>) -> String {
    match fmt.map(str::trim).filter(|f| !f.is_empty()) {
        // TRANSLATORS: %1 is a wallpaper format in lowercase (“video”).
        Some(f) => trf("Wallpaper stopped (format was: %1)", &[&format_label(f)]),
        None => tr("Live wallpaper isn't running — the desktop is back to how it was"),
    }
}

/// Неизвестный формат.
pub fn unknown_format_message(fmt: &str) -> String {
    trf("Unknown format: %1", &[&fmt])
}

/// Файл не найден.
pub fn file_missing_message(path: &str) -> String {
    trf("File not found: %1", &[&path])
}

// ── GPU ────────────────────────────────────────────────────────────────────

/// Что удалось узнать про видеокарту.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct Gpu {
    /// `nvidia` / `amd` / `intel` / `unknown`.
    pub vendor: String,
    /// Драйвер ядра: `nvidia` / `nouveau` / `amdgpu` / `i915` / `xe` / `?`.
    pub driver: String,
    /// NVIDIA с проприетарным драйвером на Wayland — известный риск.
    pub wayland_risk: bool,
    /// Текст предупреждения либо пусто.
    pub warning: String,
}

/// Разбор `lspci -k`: производитель и драйвер первой видеокарты.
pub fn parse_gpu(lspci: &str) -> Gpu {
    let mut vendor = "unknown".to_string();
    let mut driver = "?".to_string();
    let mut vga_found = false;
    for line in lspci.lines() {
        let low = line.to_lowercase();
        if low.contains("vga") || low.contains("3d controller") {
            vga_found = true;
            if low.contains("nvidia") {
                vendor = "nvidia".into();
            } else if low.contains("amd") || low.contains("ati") {
                vendor = "amd".into();
            } else if low.contains("intel") {
                vendor = "intel".into();
            }
        } else if vga_found && low.contains("kernel driver") {
            for name in ["nvidia", "nouveau", "amdgpu", "i915", "xe"] {
                if low.contains(name) {
                    driver = name.to_string();
                    break;
                }
            }
            vga_found = false;
        }
    }
    let wayland_risk = vendor == "nvidia" && driver == "nvidia";
    let warning = if wayland_risk {
        tr("NVIDIA with the proprietary driver on Wayland: hardware acceleration may not \
            work. Suggestions: use the VLC backend, or add __NV_PRIME_RENDER_OFFLOAD=1 \
            to the environment.")
    } else {
        String::new()
    };
    Gpu {
        vendor,
        driver,
        wayland_risk,
        warning,
    }
}

// ── иконки рабочего стола ──────────────────────────────────────────────────

/// Команды показа/скрытия иконок рабочего стола для окружения.
///
/// Живые обои ложатся поверх стола, и на части окружений иконки под ними
/// пропадают насовсем — поэтому их гасят явно и явно возвращают.
pub fn desktop_icons_argvs(de: &str, visible: bool) -> Vec<Vec<String>> {
    let val = if visible { "true" } else { "false" };
    match de {
        // MATE: иконки рисует Caja, и выключать их надо только если он
        // действительно владеет столом (проверку делает шим).
        "mate" => vec![argv(&[
            "gsettings",
            "set",
            "org.mate.background",
            "show-desktop-icons",
            val,
        ])],
        "gnome" | "budgie" => vec![
            // Ubuntu Desktop Icons NG
            argv(&[
                "gsettings",
                "set",
                "org.gnome.shell.extensions.ding",
                "show-desktop-icons",
                val,
            ]),
            // Ванильный GNOME: расширение Desktop Icons
            argv(&[
                "gsettings",
                "set",
                "org.gnome.desktop.background",
                "show-desktop-icons",
                val,
            ]),
        ],
        "cinnamon" | "cinnamon-mint" => vec![argv(&[
            "gsettings",
            "set",
            "org.nemo.desktop",
            "show-desktop-icons",
            val,
        ])],
        // style: 0 = нет, 2 = файлы и ярлыки
        "xfce" => vec![argv(&[
            "xfconf-query",
            "-c",
            "xfce4-desktop",
            "-p",
            "/desktop-icons/style",
            "-s",
            if visible { "2" } else { "0" },
        ])],
        _ => Vec::new(),
    }
}

/// Читает ли MATE стол через Caja (иначе гасить нечего).
pub fn caja_desktop_argv() -> Vec<String> {
    argv(&[
        "gsettings",
        "get",
        "org.mate.background",
        "show-desktop-icons",
    ])
}

/// Перезапуск рабочего стола Caja на MATE.
///
/// Отдельная кнопка, а не часть гашения иконок: стол Caja иногда остаётся
/// пустым после смены обоев или темы, и человеку нужен способ вернуть его,
/// не выходя из сеанса.
pub fn caja_restart_argv() -> Vec<String> {
    argv(&["caja", "--force-desktop", "--no-default-window"])
}

/// Что спросить у системы ПЕРЕД тем, как гасить иконки, либо `None`.
///
/// Только MATE: там иконки рисует Caja, и гасить их имеет смысл, лишь если
/// стол действительно за ним. Возврат иконок такой проверки не требует —
/// вернуть их надо в любом случае. Ответ считается «да», если команда
/// напечатала `true`.
pub fn desktop_icons_probe_argv(de: &str, visible: bool) -> Option<Vec<String>> {
    if de == "mate" && !visible {
        Some(caja_desktop_argv())
    } else {
        None
    }
}

// ── пауза при полноэкранном окне и низкой батарее ──────────────────────────

/// Каким инструментом узнавать полноэкранное окно. Пусто — узнать нечем.
pub fn fullscreen_backend(de: &str, session_type: &str) -> &'static str {
    if session_type == "x11" {
        return "xdotool";
    }
    match de {
        "kde" => "kdotool",
        "hyprland" => "hyprctl",
        "gnome" | "budgie" => "gdbus",
        _ => "",
    }
}

/// Геометрия активного окна на X11.
pub fn xdotool_geometry_argv() -> Vec<String> {
    argv(&[
        "xdotool",
        "getactivewindow",
        "getwindowgeometry",
        "--shell",
    ])
}

/// Активное окно на KDE Wayland (без kdotool узнать нечем).
pub fn kdotool_active_argv() -> Vec<String> {
    argv(&["kdotool", "getactivewindow"])
}

/// Активное окно Hyprland.
pub fn hyprctl_active_argv() -> Vec<String> {
    argv(&["hyprctl", "activewindow", "-j"])
}

/// Запрос состояния окна к GNOME Shell.
pub fn gdbus_fullscreen_argv() -> Vec<String> {
    argv(&[
        "gdbus",
        "call",
        "--session",
        "--dest",
        "org.gnome.Shell",
        "--object-path",
        "/org/gnome/Shell",
        "--method",
        "org.gnome.Shell.Eval",
        "global.display.get_focus_window() && \
         global.display.get_focus_window().is_fullscreen()",
    ])
}

/// Типы окон X11, которые обоев НЕ заслоняют, как бы велики ни были.
///
/// ⚠️ 16.09.2026, замер на стенде X11 с оконным менеджером: активным окном
/// `xdotool getactivewindow` называл НАШЕ ЖЕ окно обоев — оно во весь
/// экран, и правило «поверх играет полноэкранное окно» срабатывало всегда.
/// Flash-обои давали 4 кадра вместо 240 за тот же прогон — вставали на
/// паузу сами на себя. Рабочий стол — это фон, а не окно поверх фона;
/// панель и док — полоска у края, из-за которой обои гасить тем более
/// незачем.
pub const WALLPAPER_SAFE_WINDOW_TYPES: &[&str] = &[
    "_NET_WM_WINDOW_TYPE_DESKTOP",
    "_NET_WM_WINDOW_TYPE_DOCK",
];

/// `NET::WindowType` в ответе KWin: 1 — рабочий стол, 2 — панель/док.
/// Та же мысль, что у [`WALLPAPER_SAFE_WINDOW_TYPES`], на языке Wayland-KWin.
pub const KWIN_SAFE_WINDOW_TYPES: &[i64] = &[1, 2];

/// Свойства окна X11, по которым видно «это фон, а не окно поверх фона».
///
/// Спрашиваем ОДНИМ запуском: тип окна и хозяина. Тип закрывает чужие
/// рабочие столы и панели, PID — наши собственные окна обоев (на случай,
/// когда тип проставить не удалось).
pub fn xprop_window_info_argv(window_id: i64) -> Vec<String> {
    vec![
        "xprop".into(),
        "-id".into(),
        window_id.to_string(),
        "_NET_WM_WINDOW_TYPE".into(),
        "_NET_WM_PID".into(),
    ]
}

/// Идентификатор активного окна из вывода `xdotool … --shell` (`WINDOW=`).
pub fn parse_xdotool_window_id(stdout: &str) -> Option<i64> {
    stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix("WINDOW="))
        .and_then(|v| v.trim().parse::<i64>().ok())
}

/// Это окно — фон рабочего стола (или наше собственное окно обоев)?
///
/// На вход — вывод [`xprop_window_info_argv`]. Ответ, которого не поняли
/// (свойства нет, `xprop` промолчал, мусор), даёт `false`: окно считается
/// обычным, то есть поведение остаётся ПРЕЖНИМ. Отвечать здесь «да» на
/// непонятное значило бы «не глушим никогда» — обои играли бы поверх игры.
pub fn window_is_wallpaper_layer(props: &str, own_pids: &[i64]) -> bool {
    for line in props.lines() {
        let line = line.trim();
        let Some((name, value)) = line.split_once('=') else {
            // `xprop` на отсутствующее свойство печатает
            // «_NET_WM_PID:  not found.» — знака равенства там нет.
            continue;
        };
        let value = value.trim();
        if name.starts_with("_NET_WM_WINDOW_TYPE")
            && value
                .split(',')
                .any(|t| WALLPAPER_SAFE_WINDOW_TYPES.contains(&t.trim()))
        {
            return true;
        }
        if name.starts_with("_NET_WM_PID") {
            if let Ok(pid) = value.parse::<i64>() {
                if own_pids.contains(&pid) {
                    return true;
                }
            }
        }
    }
    false
}

/// Окно занимает весь экран? Разбор вывода `xdotool … --shell`.
pub fn parse_xdotool_fullscreen(stdout: &str, screen: &str) -> bool {
    let Some((sw, sh)) = screen.split_once('x') else {
        return false;
    };
    let (Ok(sw), Ok(sh)) = (sw.parse::<i64>(), sh.parse::<i64>()) else {
        return false;
    };
    let mut w = 0i64;
    let mut h = 0i64;
    for line in stdout.lines() {
        if let Some(v) = line.strip_prefix("WIDTH=") {
            w = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("HEIGHT=") {
            h = v.trim().parse().unwrap_or(0);
        }
    }
    w >= sw && h >= sh
}

/// Куда стучаться за свойствами окна KWin: имя на шине, объект, метод.
///
/// Прежний код спрашивал `org.kde.kwin.Scripting.evaluateScript` — такого
/// метода у KWin 6 нет вовсе, так что пауза обоев на полноэкранном окне не
/// срабатывала НИКОГДА.
pub const KWIN_WINDOW_INFO: &[&str] =
    &["org.kde.KWin", "/KWin", "org.kde.KWin.getWindowInfo"];

/// Разбор ответа GNOME Shell на `Eval` — «есть ли полноэкранное окно».
pub fn parse_gdbus_fullscreen(stdout: &str) -> bool {
    stdout.to_lowercase().contains("true")
}

/// Разбор словаря свойств окна от KWin (`org.kde.KWin.getWindowInfo`).
///
/// `dbus-send` разворачивает словарь в плоский список «ключ, значение»,
/// `qdbus` печатает «fullscreen: false» — понимаем оба вида.
pub fn parse_kwin_fullscreen(info: &str) -> bool {
    let lines: Vec<&str> = info.lines().map(str::trim).collect();
    for (i, line) in lines.iter().enumerate() {
        if let Some(v) = line.strip_prefix("fullscreen:") {
            return v.trim() == "true";
        }
        if *line == "fullscreen" && i + 1 < lines.len() {
            return lines[i + 1] == "true";
        }
    }
    false
}

/// Это окно KWin — фон рабочего стола, панель или наше окно обоев?
///
/// Та же мысль, что у [`window_is_wallpaper_layer`] на X11, только
/// свойства приходят от самого KWin (`getWindowInfo`). Непонятый ответ —
/// `false`, то есть поведение прежнее.
pub fn kwin_window_is_wallpaper_layer(info: &str, own_pids: &[i64]) -> bool {
    for line in info.lines() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "type" => {
                if let Ok(t) = value.parse::<i64>() {
                    if KWIN_SAFE_WINDOW_TYPES.contains(&t) {
                        return true;
                    }
                }
            }
            "pid" => {
                if let Ok(pid) = value.parse::<i64>() {
                    if own_pids.contains(&pid) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// Это окно Hyprland — наше собственное окно обоев?
///
/// Своего «типа окна» у протокола нет: слои (layer-shell), которыми и
/// лежат обои, в `activewindow` не попадают вовсе — остаётся хозяин. Ответ
/// без `pid` — `false`, поведение прежнее.
pub fn hyprctl_window_is_wallpaper_layer(stdout: &str, own_pids: &[i64]) -> bool {
    serde_json::from_str::<serde_json::Value>(stdout)
        .ok()
        .and_then(|v| v.get("pid").and_then(serde_json::Value::as_i64))
        .map(|pid| own_pids.contains(&pid))
        .unwrap_or(false)
}

/// Разбор ответа `hyprctl activewindow -j`.
pub fn parse_hyprctl_fullscreen(stdout: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(stdout)
        .ok()
        .and_then(|v| v.get("fullscreen").cloned())
        .map(|v| v.as_i64().unwrap_or(0) != 0 || v.as_bool().unwrap_or(false))
        .unwrap_or(false)
}

/// Ниже какого заряда батареи надзор ставит живые обои на паузу, % — то же
/// число, что у Python (`battery_pause_pct`).
pub const BATTERY_PAUSE_PCT: i64 = 20;

/// Где ядро ищет батареи.
pub const POWER_SUPPLY_DIR: &str = "/sys/class/power_supply";

/// Порог и правило паузы по батарее: заряжается — не паузим.
pub fn battery_should_pause(percent: i64, status: &str, threshold: i64) -> bool {
    if status == "Charging" || status == "Full" {
        return false;
    }
    percent < threshold
}

/// Загрузка процессора процессом обоев.
pub fn ps_cpu_argv(pid: i64) -> Vec<String> {
    vec![
        "ps".into(),
        "-p".into(),
        pid.to_string(),
        "-o".into(),
        "%cpu".into(),
        "--no-headers".into(),
    ]
}

/// Порог загрузки процессора, при котором обои считаются протёкшими, %.
pub const CPU_HIGH_THRESHOLD: f64 = 80.0;
/// Сколько замеров подряд выше порога — принудительная остановка (×5 с).
pub const CPU_HIGH_STRIKES: i64 = 6;
/// Период цикла мониторинга, секунд.
pub const MONITOR_INTERVAL_SEC: i64 = 5;

// ── сироты ─────────────────────────────────────────────────────────────────

/// Процессы, которые могли пережить прошлый запуск. Убиваем только те, у
/// кого в командной строке есть след Rexon.
pub const ORPHAN_NAMES: &[&str] = &["mpvpaper", "xwinwrap", "cvlc"];

/// Наш ли это процесс — по командной строке.
pub fn is_rexon_cmdline(cmdline: &str) -> bool {
    cmdline.to_lowercase().contains("rexon")
}

/// Поиск процессов по имени.
pub fn pgrep_argv(name: &str) -> Vec<String> {
    argv(&["pgrep", "-f", name])
}

/// Имя PID-файла процесса обоев.
pub const PIDFILE_NAME: &str = "rexon-wallpaper.pid";

// ── ambient-звуки ──────────────────────────────────────────────────────────

/// Отдельный mpv на каждый фоновый звук.
pub fn ambient_argv(path: &str, volume: i64, looped: bool) -> Vec<String> {
    let mut cmd = vec![
        "mpv".to_string(),
        "--no-video".into(),
        "--no-terminal".into(),
        format!("--volume={volume}"),
    ];
    if looped {
        cmd.push("--loop=inf".into());
    }
    cmd.push(path.to_string());
    cmd
}

// ── установка зависимостей ─────────────────────────────────────────────────

/// Имена пакетов для ручной доустановки видеозависимостей.
pub fn install_deps_packages(family: &str, session_type: &str) -> Vec<String> {
    let mut pkgs = vec!["mpv".to_string()];
    if family == "arch" && session_type == "wayland" {
        pkgs.push("mpvpaper".to_string());
    }
    pkgs
}

/// Команда, которой ставятся видеозависимости.
///
/// Своей таблицы здесь больше нет: и флаги, и выбор «именованный путь
/// org.rexon.pkg или сырой pkexec» отдаёт движок пакетов
/// ([`crate::system_plan::install_argv`]) — тот же, которым Rexon ставит
/// рабочие столы. Прежняя копия знала три семейства из одиннадцати и успела с
/// движком разойтись: `pacman -S --noconfirm` без `--needed` (переустанавливал
/// уже стоящий mpv) и `apt install -y` вместо
/// `apt-get install -y --no-install-recommends` (нестабильный для скриптов
/// интерфейс плюс хвост рекомендованных пакетов). Восемь остальных семейств
/// получали «Неизвестный пакетный менеджер» при полностью известном менеджере.
///
/// `None` — семейство не опознано (`unknown` или строка вне таблицы) либо
/// ставить нечего; вызывающий говорит человеку, что доустановить руками
/// ([`install_deps_unknown_message`]).
pub fn install_deps_argv(
    family: &str,
    pkgs: &[String],
    pkg_helper_available: bool,
) -> Option<Vec<String>> {
    let fam = crate::system::PkgFamily::from_value(family)?;
    crate::system_plan::install_argv(fam, pkgs, pkg_helper_available)
}

/// Пакетный менеджер не опознан — говорим, что доустановить руками.
pub fn install_deps_unknown_message(missing: &[String]) -> String {
    trf("Unknown package manager. Install manually: %1", &[&missing.join(", ")])
}

/// Имя пакета из строки «чего не хватает»: снимаем пометки «(AUR)»,
/// «(рекомендуется)» и «или …».
///
/// ⚠️ Строку собирает [`missing_for_format`] на языке интерфейса, поэтому
/// слово «или» здесь не ищется: имя пакета — первое слово до скобки
/// (в именах пакетов пробелов не бывает).
pub fn dep_package_name(item: &str) -> String {
    item.split('(')
        .next()
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_string()
}

// ── библиотека установленных обоев ─────────────────────────────────────────

/// Расширения файлов, которые библиотека берёт из `data/`, когда в манифесте
/// нет явного списка обоев.
pub fn library_data_exts() -> Vec<String> {
    let mut exts: Vec<String> = VALID_VIDEO_EXTS.iter().map(|s| s.to_string()).collect();
    for e in [".png", ".jpg", ".jpeg", ".webp", ".gif", ".apng", ".swf"] {
        if !exts.iter().any(|x| x == e) {
            exts.push(e.to_string());
        }
    }
    exts.sort();
    exts
}

/// Одни обои из установленного пакета обоев — плитка библиотеки.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibraryItem {
    /// Идентификатор пакета.
    pub id: String,
    /// Название для человека.
    pub name: String,
    /// Путь к файлу обоев.
    pub path: String,
    /// Формат обоев (`static`, `video`, …).
    pub format: String,
    /// Путь к картинке-превью пакета (файла может не быть).
    pub preview: String,
}

/// Обои одного пакета по его `manifest.json` — порт тела
/// `WallpaperApplier.list_installed_wallpapers`. Наблюдения за диском —
/// снаружи: `exists` и `data_listing` (имена в каталоге `data/`).
///
/// ⭐ Починка: у обоев с пустым `file` Python получал путь САМОГО каталога
/// `data/` (`os.path.join(data, "")`), тот «существовал» — и каталог
/// попадал в библиотеку плиткой, которую нельзя применить.
pub fn library_items_from_manifest(
    pack_dir: &str,
    dir_name: &str,
    manifest: &serde_json::Value,
    exists: &dyn Fn(&str) -> bool,
    data_listing: &dyn Fn(&str) -> Vec<String>,
) -> Vec<LibraryItem> {
    use serde_json::Value;
    if manifest.get("type").and_then(Value::as_str) != Some("wallpaper-pack") {
        return Vec::new();
    }
    let text = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);
    let id = text(manifest.get("id")).unwrap_or_else(|| dir_name.to_string());
    let pack_name = text(manifest.get("name")).unwrap_or_else(|| dir_name.to_string());
    let pack_format =
        text(manifest.get("wallpaper_format")).unwrap_or_else(|| "static".to_string());
    let preview = format!("{pack_dir}/preview.png");
    let data_dir = format!("{pack_dir}/data");
    let listed = manifest
        .get("wallpapers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::new();
    for wp in &listed {
        let file = text(wp.get("file")).unwrap_or_default();
        if file.trim().is_empty() {
            continue;
        }
        // Путь в манифесте пишут по-разному: относительно `data/` (свои
        // пакеты), относительно корня пакета (товар магазина кладёт картинку
        // рядом с манифестом) и с `data/` в самом пути (импорт тем Windows).
        // Прежде проверялся только первый вид, и «Библиотека обоев» стояла
        // пустой при установленных наборах (приёмка 10.10.2026).
        let Some(path) = [format!("{data_dir}/{file}"), format!("{pack_dir}/{file}")]
            .into_iter()
            .find(|p| exists(p))
        else {
            continue;
        };
        let format = text(wp.get("format")).unwrap_or_else(|| pack_format.clone());
        // Набор из нескольких картинок: у каждой плитки своя картинка и свой
        // номер. С общим превью пакета восемь обоев выглядели восемью
        // одинаковыми плитками, и выбрать нужную было нельзя.
        let many = listed.len() > 1;
        let own_picture = many && format == "static";
        out.push(LibraryItem {
            id: id.clone(),
            name: text(wp.get("name")).unwrap_or_else(|| {
                if many {
                    format!("{pack_name} — {}", out.len() + 1)
                } else {
                    pack_name.clone()
                }
            }),
            preview: if own_picture { path.clone() } else { preview.clone() },
            path,
            format,
        });
    }
    // Нет списка обоев — берём подходящие файлы из `data/`.
    if listed.is_empty() {
        let exts = library_data_exts();
        let mut files = data_listing(&data_dir);
        files.sort();
        for file in files {
            let ext = file.rfind('.').map(|i| file[i..].to_lowercase()).unwrap_or_default();
            if !exts.iter().any(|e| *e == ext) {
                continue;
            }
            out.push(LibraryItem {
                id: id.clone(),
                name: file.clone(),
                path: format!("{data_dir}/{file}"),
                format: pack_format.clone(),
                preview: preview.clone(),
            });
        }
    }
    out
}

/// Все установленные обои из каталога расширений — живой обход диска.
///
/// ⭐ Починка: порядок пакетов — по имени каталога. У Python он был порядком
/// файловой системы, и плитки библиотеки менялись местами от машины к
/// машине. Битый манифест пропускается (остальные пакеты показываются).
pub fn list_installed_wallpapers(ext_dir: &str) -> Vec<LibraryItem> {
    let Ok(read) = std::fs::read_dir(ext_dir) else {
        return Vec::new();
    };
    let mut packs: Vec<(String, String)> = read
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| {
            (
                e.path().to_string_lossy().into_owned(),
                e.file_name().to_string_lossy().into_owned(),
            )
        })
        .collect();
    packs.sort_by(|a, b| a.1.cmp(&b.1));

    let exists = |p: &str| std::path::Path::new(p).exists();
    let listing = |d: &str| -> Vec<String> {
        std::fs::read_dir(d)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter(|e| e.path().is_file())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for (dir, name) in packs {
        let Ok(raw) = std::fs::read_to_string(format!("{dir}/manifest.json")) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        out.extend(library_items_from_manifest(&dir, &name, &manifest, &exists, &listing));
    }
    out
}

// ── мелочи ─────────────────────────────────────────────────────────────────

fn has(tools: &[String], name: &str) -> bool {
    tools.iter().any(|t| t == name)
}

fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Разбить путь на «до расширения» и «расширение с точкой» — как
/// `os.path.splitext`.
pub fn split_ext(path: &str) -> (&str, &str) {
    let name = base_name(path);
    match name.rfind('.') {
        // Точка в начале имени — это скрытый файл, а не расширение.
        Some(i) if i > 0 => {
            let cut = path.len() - name.len() + i;
            (&path[..cut], &path[cut..])
        }
        _ => (path, ""),
    }
}

/// `str(float)` Python для целых значений печатает «3.0», а не «3».
fn format_float(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e16 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// SHA-1 в hex — тот же ключ кеша, что считал Python (`hashlib.sha1`).
fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// ⭐ Текст успеха — для покупателя: в нём нет служебного имени пути,
    /// которым поставлены обои («generic», «KDE D-Bus», «swaybg», «PID»).
    /// Путь уходит в журнал (`Step::backend`). Сверка обходит все окружения
    /// и наборы программ, и новый шаг с меткой в скобках её покрасит.
    #[test]
    fn success_texts_carry_no_path_labels() {
        let des = [
            "kde", "gnome", "cinnamon", "cinnamon-mint", "mate", "budgie", "pantheon", "unity",
            "xfce", "lxqt", "sway",
            "niri", "river", "wayfire", "labwc", "hyprland", "generic",
        ];
        let tool_sets = [
            t(&[]),
            t(&["plasma-apply-wallpaperimage", "pcmanfm-qt", "feh"]),
            t(&["swww", "hyprpaper", "swaybg"]),
            t(&["wbg"]),
        ];
        let mut steps = Vec::new();
        for de in des {
            for session in ["wayland", "x11", ""] {
                for tools in &tool_sets {
                    steps.extend(static_plan(de, session, "/tmp/a.png", tools, "swww"));
                }
            }
        }
        steps.extend(swf_native_plan("/usr/bin/ruffle", "/tmp/a.swf"));
        let labels = [
            "generic", "KDE D-Bus", "KDE Plasma", "Xfce", "swww", "hyprpaper", "swaybg",
            "wbg", "feh", "pcmanfm", "mpvpaper", "xwinwrap", "QtWebEngine", "surf", "Ruffle",
            "PID", "(",
        ];
        let mut checked = 0;
        for step in steps.iter().filter(|s| !s.ok_message.is_empty()) {
            checked += 1;
            assert!(
                !step.ok_message.contains(&step.backend),
                "в тексте успеха имя пути «{}»: {}",
                step.backend,
                step.ok_message
            );
            for label in labels {
                assert!(
                    !step.ok_message.contains(label),
                    "в тексте успеха служебная метка «{label}»: {}",
                    step.ok_message
                );
            }
        }
        assert!(checked > 10, "сверка ничего не проверила ({checked})");
    }

    /// ⭐ Каждый формат пакета обоев имеет подпись для человека: служебное
    /// слово (`static`, `video`…) на карточке библиотеки не показывается.
    #[test]
    fn every_wallpaper_format_has_a_title() {
        for fmt in crate::constants::WALLPAPER_FORMATS {
            let title = format_title(fmt);
            assert_ne!(title, *fmt, "у формата «{fmt}» нет подписи");
            assert!(
                title.chars().next().is_some_and(char::is_uppercase),
                "подпись «{title}» не с заглавной буквы"
            );
        }
        assert_eq!(format_title("static"), "Static image");
        assert_eq!(format_title("video"), "Video");
        assert_eq!(format_title("web"), "Web page");
        assert_eq!(format_title("scene"), "Scene");
        // Незнакомый формат — как есть, без выдуманного имени.
        assert_eq!(format_title("нечто"), "Нечто");
    }

    /// Размытая копия ложится в кеш Rexon — и опознаётся как НЕ настоящие
    /// обои, чтобы остановка не вернула её на стол.
    #[test]
    fn blur_copy_goes_to_the_cache() {
        let out = blur_cache_output_path("/home/u/.cache/", "/usr/share/backgrounds/закат.png");
        assert_eq!(out, "/home/u/.cache/rexon/blur/закат_blur.jpg");
        assert!(!clean_image_candidate_ok(&out, true), "размытая копия сошла за обои");
    }

    /// Настоящее размытие: PNG больше предела уменьшается и пишется JPEG.
    #[test]
    fn blur_writes_a_small_jpeg() {
        let dir = std::env::temp_dir().join(format!("rexon-blur-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.png");
        let big = image::RgbImage::from_fn(2400, 1200, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        big.save(&src).unwrap();
        let out = dir.join("кеш/out_blur.jpg");
        blur_image_file(src.to_str().unwrap(), out.to_str().unwrap(), 10).expect("размытие");
        let read = image::ImageReader::open(&out)
            .unwrap()
            .with_guessed_format()
            .unwrap();
        assert_eq!(read.format(), Some(image::ImageFormat::Jpeg));
        let img = read.decode().unwrap();
        assert!(img.width() <= BLUR_MAX_SIZE.0 as u32 && img.height() <= BLUR_MAX_SIZE.1 as u32);
        assert!(blur_image_file("/нет/такой.png", out.to_str().unwrap(), 10).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Библиотека: пакет со списком обоев, пакет без списка (файлы из
    /// `data/`), чужой тип и пустое имя файла.
    #[test]
    fn library_reads_wallpaper_packs() {
        let exists = |p: &str| !p.ends_with("/нет.mp4");
        let listing = |_: &str| t(&["b.png", "a.jpg", "readme.txt"]);
        let listed = serde_json::json!({
            "type": "wallpaper-pack", "id": "pack1", "name": "Пакет",
            "wallpaper_format": "video",
            "wallpapers": [
                {"file": "закат.mp4", "name": "Закат"},
                {"file": "нет.mp4"},
                {"file": ""},
                {"file": "кадр.png", "format": "static"}
            ]
        });
        let items = library_items_from_manifest("/x/pack1", "pack1", &listed, &exists, &listing);
        assert_eq!(items.len(), 2, "{items:?}");
        assert_eq!(items[0].name, "Закат");
        assert_eq!(items[0].format, "video");
        assert_eq!(items[0].path, "/x/pack1/data/закат.mp4");
        assert_eq!(items[0].preview, "/x/pack1/preview.png");
        // В наборе из нескольких обоев запасное имя несёт номер, а у
        // картинки своя плитка-образец: иначе плитки набора неотличимы.
        assert_eq!(items[1].name, "Пакет — 2");
        assert_eq!(items[1].format, "static");
        assert_eq!(items[1].preview, items[1].path);

        // Один файл в наборе — имя пакета без номера. Путь из манифеста
        // находится и от корня пакета (товар магазина), и с `data/` в самом
        // пути (импорт тем Windows).
        let root_only = |p: &str| p == "/x/shop/картинка.jpg" || p == "/x/win/data/обои.jpg";
        let shop = serde_json::json!({
            "type": "wallpaper-pack", "name": "Магазин",
            "wallpapers": [{"file": "картинка.jpg", "format": "static"}]
        });
        let got = library_items_from_manifest("/x/shop", "shop", &shop, &root_only, &listing);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!((got[0].name.as_str(), got[0].path.as_str()), ("Магазин", "/x/shop/картинка.jpg"));
        assert_eq!(got[0].preview, "/x/shop/preview.png");
        let win = serde_json::json!({
            "type": "wallpaper-pack", "name": "Windows",
            "wallpapers": [{"file": "data/обои.jpg", "format": "static"}]
        });
        let got = library_items_from_manifest("/x/win", "win", &win, &root_only, &listing);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].path, "/x/win/data/обои.jpg");
        assert!(
            !items.iter().any(|i| i.path.ends_with("/data/")),
            "каталог data/ попал в библиотеку плиткой"
        );

        let bare = serde_json::json!({"type": "wallpaper-pack"});
        let files = library_items_from_manifest("/x/p2", "p2", &bare, &exists, &listing);
        let names: Vec<&str> = files.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["a.jpg", "b.png"], "файлы data/ не по порядку или лишние");
        assert!(files.iter().all(|i| i.id == "p2" && i.format == "static"));

        let other = serde_json::json!({"type": "theme", "wallpapers": [{"file": "a.png"}]});
        assert!(library_items_from_manifest("/x/p3", "p3", &other, &exists, &listing).is_empty());
    }

    /// Живой обход: пакеты по порядку имён, битый манифест не мешает
    /// остальным. Всё — во временном каталоге сверки.
    #[test]
    fn library_walk_is_sorted_and_survives_broken_manifests() {
        let root = std::env::temp_dir().join(format!("rexon-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (pack, manifest) in [
            ("b-pack", r#"{"type":"wallpaper-pack","wallpapers":[{"file":"b.png"}]}"#),
            ("a-pack", r#"{"type":"wallpaper-pack","wallpapers":[{"file":"a.png"}]}"#),
            ("broken", "{ это не json"),
        ] {
            let data = root.join(pack).join("data");
            std::fs::create_dir_all(&data).unwrap();
            std::fs::write(root.join(pack).join("manifest.json"), manifest).unwrap();
            std::fs::write(data.join(format!("{}.png", &pack[..1])), b"x").unwrap();
        }
        let items = list_installed_wallpapers(root.to_str().unwrap());
        let _ = std::fs::remove_dir_all(&root);
        let names: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
        assert_eq!(names, ["a-pack", "b-pack"]);
    }

    /// Номер контейнмента KDE не угадывается: обои читаются из всех групп
    /// `org.kde.image`, какой бы номер у рабочего стола ни был.
    #[test]
    fn kde_wallpaper_images_reads_every_containment() {
        let conf = "\
[Containments][216]
plugin=org.kde.plasma.folder

[Containments][216][Wallpaper][org.kde.image][General]
Image=/usr/share/wallpapers/Next/
PreviewImage=/usr/share/wallpapers/Next/

[Containments][216][Wallpaper][org.waywallen.kde][General]
Image=/tmp/live.mp4

[Containments][3][Wallpaper][org.kde.image][General]
Image=/home/u/.local/share/wallpapers/Пачка/contents/images/1920x1080.png
";
        assert_eq!(
            kde_wallpaper_images(conf),
            vec![
                "/usr/share/wallpapers/Next/".to_string(),
                "/home/u/.local/share/wallpapers/Пачка/contents/images/1920x1080.png"
                    .to_string(),
            ]
        );
        assert!(kde_wallpaper_images("").is_empty());
    }

    /// Один ответ plasmashell отвечает на оба вопроса о плагине стола.
    #[test]
    fn kde_plugins_answer_both_questions_from_one_read() {
        let reply = |v: &str| format!("method return time=1\n   string \"{v}\"\n");

        // Два монитора: чужой плагин найден, даже если первым идёт штатный.
        let two = reply("org.kde.image,org.waywallen.kde");
        assert_eq!(
            kde_desktop_plugins(&two),
            vec!["org.kde.image", "org.waywallen.kde"]
        );
        assert_eq!(
            kde_foreign_wallpaper_plugin(&two).as_deref(),
            Some("org.waywallen.kde")
        );
        assert!(!kde_removed_plugin_selected(&two));

        // Удалённый плагин на ВТОРОМ столе: прежнее чтение брало только
        // `d[0]` и такой стол пропускало.
        let stale = reply("org.kde.image,org.rexon.wallpaper");
        assert!(kde_removed_plugin_selected(&stale));
        // ...и запоминать его нельзя: остановка вернула бы мёртвый плагин
        // обратно и оставила стол пустым.
        assert_eq!(kde_foreign_wallpaper_plugin(&stale), None);

        // Штатный плагин чужим не считается: к нему остановка возвращается.
        assert_eq!(kde_foreign_wallpaper_plugin(&reply("org.kde.image")), None);

        // plasmashell молчит — молчание не выдаётся за «плагинов нет».
        assert!(kde_desktop_plugins("").is_empty());
        assert!(!kde_removed_plugin_selected(""));
        assert_eq!(kde_foreign_wallpaper_plugin(""), None);
    }

    /// Сообщение об остановке называет формат, который ИГРАЛ (исходный язык —
    /// английский; русский сверяет каталог).
    #[test]
    fn stopped_message_names_the_format_that_played() {
        assert_eq!(
            stopped_message(Some("youtube")),
            "Wallpaper stopped (format was: YouTube video)"
        );
        assert_eq!(
            stopped_message(Some("swf")),
            "Wallpaper stopped (format was: Flash (SWF))"
        );
        // Ничего не играло — так и говорим, а не «был формат: None».
        assert_eq!(
            stopped_message(None),
            "Live wallpaper isn't running — the desktop is back to how it was"
        );
        assert_eq!(stopped_message(Some("")), stopped_message(None));
        // Неизвестное ядру имя показываем как есть, не подменяя чужим.
        assert_eq!(
            stopped_message(Some("шейдер")),
            "Wallpaper stopped (format was: шейдер)"
        );
    }

    /// Текущие обои XFCE и тёмные обои GNOME — чтение для подложки снимков
    /// виджетов Conky.
    #[test]
    fn xfce_and_dark_gnome_wallpaper_reading() {
        let list = "/backdrop/screen0/monitorHDMI-1/workspace1/last-image\n\
                    /backdrop/screen0/monitorHDMI-1/workspace0/last-image\n\
                    /backdrop/screen0/monitorHDMI-1/workspace0/image-style\n";
        assert_eq!(
            xfce_last_image_prop(list).as_deref(),
            Some("/backdrop/screen0/monitorHDMI-1/workspace0/last-image")
        );
        assert_eq!(xfce_last_image_prop("/backdrop/single-workspace-mode\n"), None);
        assert_eq!(gsettings_dark_key("gnome"), Some(("org.gnome.desktop.background", "picture-uri-dark")));
        assert_eq!(gsettings_dark_key("mate"), None);
        assert!(prefers_dark("'prefer-dark'\n"));
        assert!(!prefers_dark("'default'"));
        assert_eq!(gsettings_path_value("'file:///usr/share/a b.jpg'\n"), "/usr/share/a b.jpg");
    }

    /// Возврат после остановки живых обоев берёт картинку СТОЛА: панель
    /// тоже держит группу `org.kde.image`, а в файле может стоять раньше.
    #[test]
    fn kde_desktop_static_image_prefers_desktop_containment() {
        let conf = "\
[Containments][1]
plugin=org.kde.panel

[Containments][1][Wallpaper][org.kde.image][General]
Image=/usr/share/wallpapers/Next/

[Containments][216]
plugin=org.kde.plasma.folder
wallpaperplugin=org.waywallen.kde

[Containments][216][Wallpaper][org.kde.image][General]
Image=file:///home/u/.local/share/wallpapers/мой.png
";
        // Схема снята: дальше путь идёт в проверку существования файла.
        assert_eq!(
            kde_desktop_static_image(conf).as_deref(),
            Some("/home/u/.local/share/wallpapers/мой.png")
        );
        // Столов не нашлось — лучше любая картинка, чем дефолт дистрибутива.
        assert_eq!(
            kde_desktop_static_image(
                "[Containments][9][Wallpaper][org.kde.image][General]\nImage=/w/один.png\n"
            )
            .as_deref(),
            Some("/w/один.png")
        );
        assert!(kde_desktop_static_image("").is_none());
    }

    #[test]
    fn de_detection_order_matters() {
        // budgie содержит gnome, mate обязан идти до gnome
        assert_eq!(detect_de("Budgie:GNOME", ""), "budgie");
        assert_eq!(detect_de("ubuntu:GNOME", ""), "gnome");
        assert_eq!(detect_de("", "mate"), "mate");
        assert_eq!(detect_de("KDE", ""), "kde");
        // Было «generic»: прежняя таблица движка не смотрела на имя сеанса
        // Plasma, и обои на KDE без XDG_CURRENT_DESKTOP шли общим путём.
        assert_eq!(detect_de("", "plasma"), "kde");
        assert_eq!(detect_de("sway", ""), "sway");
        assert_eq!(detect_de("i3", ""), "generic");
    }

    /// Pantheon: картинку ставит родной ключ, который читает Gala, — вместе
    /// с картинкой для тёмного стиля; а после живых обоев возвращается то,
    /// что было.
    #[test]
    fn pantheon_static_wallpaper_uses_gnome_background_keys() {
        let plan = static_plan("pantheon", "x11", "/tmp/a.png", &[], "swww");
        assert_eq!(plan.len(), 1);
        assert_eq!(
            plan[0].argv,
            t(&["gsettings", "set", "org.gnome.desktop.background", "picture-uri", "file:///tmp/a.png"])
        );
        assert_eq!(
            plan[0].post,
            vec![t(&[
                "gsettings", "set", "org.gnome.desktop.background", "picture-uri-dark",
                "file:///tmp/a.png",
            ])]
        );
        assert!(RESTORE_DES.contains(&"pantheon"));
        assert_eq!(restore_static_plan("pantheon", "/tmp/a.png", &[], "").len(), 1);
    }

    /// ⭐ Движок обоев опознаёт окружение ТЕМ ЖЕ ответом, что и весь продукт
    /// (правило #12), и не теряет ни river/wayfire/labwc, ни «не знаю».
    /// Сверка — по живым значениям XDG_CURRENT_DESKTOP / DESKTOP_SESSION.
    #[test]
    fn detect_de_is_the_core_answer() {
        let cases: &[(&str, &str, &str)] = &[
            ("KDE", "plasma", "kde"),
            ("", "plasmawayland", "kde"),
            ("GNOME", "gnome", "gnome"),
            ("GNOME-Flashback:GNOME", "gnome-flashback-metacity", "gnome"),
            ("", "gnome-xorg", "gnome"),
            ("X-Cinnamon", "cinnamon", "cinnamon-mint"),
            ("", "cinnamon2d", "cinnamon-mint"),
            ("XFCE", "xfce", "xfce"),
            ("MATE", "mate", "mate"),
            ("Budgie:GNOME", "budgie-desktop", "budgie"),
            ("Pantheon", "pantheon", "pantheon"),
            ("Pantheon", "pantheon-wayland", "pantheon"),
            ("", "pantheon-wayland", "pantheon"),
            ("LXQt", "lxqt", "lxqt"),
            ("pop:COSMIC", "", "cosmic"),
            ("Hyprland", "hyprland", "hyprland"),
            ("sway", "sway", "sway"),
            ("niri", "niri", "niri"),
            ("river", "river", "river"),
            ("", "river", "river"),
            ("wayfire", "wayfire", "wayfire"),
            ("labwc:wlroots", "labwc", "labwc"),
            ("", "labwc", "labwc"),
            ("LXDE", "LXDE", "generic"),
            ("i3", "i3", "generic"),
            ("", "", "generic"),
        ];
        for (xdg, session, want) in cases {
            assert_eq!(detect_de(xdg, session), *want, "{xdg:?} / {session:?}");
            let core = crate::system::detect_current_de(xdg, session, false);
            assert_eq!(core.as_deref().unwrap_or("generic"), *want, "ядро: {xdg:?}");
        }
        // Всё, что движок считает «фон ставит внешний демон», опознаётся.
        for de in WLROOTS_DE {
            assert_eq!(detect_de(de, ""), *de, "{de} не опознан");
        }
    }

    /// Cinnamon в продукте — `cinnamon-mint`, в старых данных движка —
    /// `cinnamon`. Каждая ветка движка обязана понимать оба.
    #[test]
    fn both_cinnamon_ids_take_the_same_branches() {
        for (a, b) in [("cinnamon", "cinnamon-mint")] {
            assert_eq!(gsettings_schema(a), gsettings_schema(b));
            assert!(gsettings_schema(b).is_some());
            assert_eq!(RESTORE_DES.contains(&a), RESTORE_DES.contains(&b));
            assert_eq!(shell_ext_schema(a), shell_ext_schema(b));
            assert_eq!(shell_ext_de_label(b), "Cinnamon");
            assert!(needs_shell_extension(b, "wayland"));
            for visible in [true, false] {
                assert_eq!(desktop_icons_argvs(a, visible), desktop_icons_argvs(b, visible));
                assert!(!desktop_icons_argvs(b, visible).is_empty());
            }
            let t = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            assert_eq!(
                static_plan(a, "x11", "/w.png", &t(&["feh"]), ""),
                static_plan(b, "x11", "/w.png", &t(&["feh"]), ""),
            );
        }
    }

    #[test]
    fn session_type_falls_back_to_display_vars() {
        assert_eq!(detect_session_type("wayland", "", ""), "wayland");
        assert_eq!(detect_session_type("", "wayland-0", ""), "wayland");
        assert_eq!(detect_session_type("tty", "", ":0"), "x11");
        assert_eq!(detect_session_type("", "", ""), "unknown");
    }

    #[test]
    fn wlroots_prefers_swww_then_hyprpaper_then_holders() {
        let all = t(&["swww", "hyprpaper", "swaybg", "wbg"]);
        let plan = static_plan("sway", "wayland", "/w.png", &all, "");
        assert_eq!(plan[0].backend, "swww");
        assert_eq!(plan[1].backend, "hyprpaper");
        assert_eq!(plan[2].backend, "swaybg");
        // wbg — только если swaybg нет; хвостом всегда отказ с подсказкой
        assert_eq!(plan.len(), 4);
        assert_eq!(plan[3].kind, step_kind::FAIL);
        let only_wbg = static_plan("sway", "wayland", "/w.png", &t(&["wbg"]), "");
        assert_eq!(only_wbg[0].backend, "wbg");
        assert_eq!(only_wbg[0].kind, step_kind::SPAWN_HOLD);
    }

    /// sway рисует фон сам (`output * bg`, свой swaybg): Rexon просит его
    /// командой, а не поднимает второй swaybg рядом. Иначе «Оформление» на
    /// sway обои не ставило никогда (держащий процесс ему недоступен), а
    /// вкладка обоев держала два фона на одном слое. Путь — в одинарных
    /// кавычках внутри двойных: так sway отдаёт его wordexp одним словом с
    /// пробелом, апострофом и `$` (сверено на sway 1.12 без экрана).
    #[test]
    fn sway_static_wallpaper_goes_through_swaymsg_first() {
        let tools = t(&["swaymsg", "swaybg"]);
        let plan = static_plan("sway", "wayland", "/w/it's a.png", &tools, "");
        assert_eq!(plan[0].kind, step_kind::RUN);
        assert_eq!(plan[0].backend, "sway");
        assert_eq!(plan[0].argv, argv(&["swaymsg", "output", "*", "bg", "\"'/w/it'\\''s a.png'\"", "fill"]));
        assert!(plan[0].drops_holder);
        assert_eq!(plan[1].backend, "swaybg");
        // Двойную кавычку и двойной пробел sway не донесёт — сразу swaybg.
        for bad in ["/w/a\"b.png", "/w/a  b.png", "/w/a\\b.png", "/w/$mod.png"] {
            assert_eq!(static_plan("sway", "wayland", bad, &tools, "")[0].backend, "swaybg", "{bad}");
        }
        // Только sway: у niri своей команды фона нет.
        assert_eq!(static_plan("niri", "wayland", "/w.png", &tools, "")[0].backend, "swaybg");
        assert!(PLAN_TOOLS.contains(&"swaymsg"));
    }

    #[test]
    fn restore_touches_only_saved_des() {
        assert!(restore_static_plan("hyprland", "/w.png", &[], "").is_empty());
        assert!(restore_static_plan("lxqt", "/w.png", &[], "").is_empty());
        let mate = restore_static_plan("mate", "/w.png", &[], "");
        assert_eq!(mate[0].argv[2], "org.mate.background");
    }

    #[test]
    fn caja_probed_only_before_hiding() {
        assert!(desktop_icons_probe_argv("mate", false).is_some());
        assert!(desktop_icons_probe_argv("mate", true).is_none());
        assert!(desktop_icons_probe_argv("gnome", false).is_none());
    }

    #[test]
    fn empty_wlroots_names_the_package() {
        let plan = static_plan("niri", "wayland", "/w.png", &[], "пакет swww");
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].kind, step_kind::FAIL);
        assert!(plan[0].err_message.contains("пакет swww"));
    }

    #[test]
    fn kde_does_not_fall_through_after_plasma_apply() {
        let plan = static_plan(
            "kde",
            "wayland",
            "/w.png",
            &t(&["plasma-apply-wallpaperimage"]),
            "",
        );
        assert_eq!(plan.len(), 1);
        assert!(plan[0].terminal);
    }

    #[test]
    fn gnome_also_writes_dark_variant() {
        let plan = static_plan("gnome", "wayland", "/w.png", &[], "");
        assert_eq!(plan[0].argv[4], "file:///w.png");
        assert_eq!(plan[0].post[0][3], "picture-uri-dark");
        // MATE берёт обычный путь, не URI
        let mate = static_plan("mate", "x11", "/w.png", &[], "");
        assert_eq!(mate[0].argv[4], "/w.png");
        assert!(mate[0].post.is_empty());
    }

    #[test]
    fn unknown_wayland_goes_to_daemon_not_gsettings() {
        // раньше проваливалось в gsettings GNOME — команда «успешна»,
        // а обои не менялись
        let plan = static_plan("generic", "wayland", "/w.png", &t(&["swww"]), "");
        assert_eq!(plan[0].backend, "swww");
    }

    #[test]
    fn ini_value_reads_qsettings_forms() {
        let t = "[General]\nWallpaper=/x.png\n[Desktop]\nWallpaperMode=fit\nWallpaper = \"/home/u/a b.png\"\n";
        assert_eq!(ini_value(t, "Desktop", "Wallpaper").as_deref(), Some("/home/u/a b.png"));
        assert_eq!(ini_value(t, "General", "Wallpaper").as_deref(), Some("/x.png"));
        assert_eq!(ini_value(t, "Desktop", "Nope"), None);
        let c = IniCheck { file: String::new(), section: "Desktop".into(), key: "Wallpaper".into() };
        assert!(ini_check_passes(Some(t), &c, "/home/u/a b.png"));
        assert!(!ini_check_passes(Some(t), &c, "/x.png"));
        assert!(!ini_check_passes(None, &c, "/x.png"));
    }

    #[test]
    fn lxqt_falls_back_when_pcmanfm_missing() {
        let plan = static_plan("lxqt", "x11", "/w.png", &t(&["feh"]), "");
        assert_eq!(plan[0].backend, "feh");
        let with_fm = static_plan("lxqt", "x11", "/w.png", &t(&["pcmanfm-qt", "feh"]), "");
        assert_eq!(with_fm[0].backend, "pcmanfm-qt");
        assert!(with_fm[0].ok_on_none);
        assert_eq!(
            with_fm[0].verify_ini.as_ref().map(|c| (c.file.as_str(), c.section.as_str(), c.key.as_str())),
            Some((PCMANFM_QT_LXQT_SETTINGS, "Desktop", "Wallpaper"))
        );
        assert_eq!(with_fm[1].backend, "feh");
    }

    #[test]
    fn video_plan_leaves_volume_to_settings() {
        // Громкости в строке запуска больше нет намеренно: пока она была
        // там, подвинутый ползунок не делал ничего до перезапуска обоев.
        let host = "/opt/rexon/bin/rexon-wallpaper-host";
        let plan = video_native_plan(host, "/v.mp4", true);
        assert!(!plan[0].argv.iter().any(|a| a == "--volume"));
        assert!(!plan[0].argv.iter().any(|a| a == "--audio"));
        // А вот «эта обоина немая» — свойство обоев, и оно передаётся.
        let mute = video_native_plan(host, "/v.mp4", false);
        assert_eq!(mute[0].argv.last().unwrap(), "--no-audio");
    }

    /// В строке запуска стоит бинарь и только он: Python из неё ушёл
    /// целиком вместе со снятым запасным путём.
    #[test]
    fn video_command_is_the_host_binary_only() {
        let host = "/opt/rexon/bin/rexon-wallpaper-host";
        let plan = video_native_plan(host, "/v.mp4", true);
        assert_eq!(plan[0].argv, vec![host, "--mode", "video", "/v.mp4"]);
        let mute = video_native_plan(host, "/v.mp4", false);
        assert_eq!(mute[0].argv, vec![host, "--mode", "video", "/v.mp4", "--no-audio"]);
        // Ни один план не имеет права позвать Python: запасного хоста нет,
        // и молчаливо подменять бинарь интерпретатором больше нечем.
        for step in plan.iter().chain(mute.iter()) {
            assert!(
                !step.argv.iter().any(|a| a.contains("python")),
                "{:?}",
                step.argv
            );
        }
    }

    /// Отказ вместо тихого отката: текст обязан называть и причину, и что
    /// делать (правило #13).
    #[test]
    fn missing_host_message_names_cause_and_remedy() {
        assert!(video_host_missing_message().contains(WALLPAPER_HOST_BINARY));
        // Хост один — и веб-отказ называет тот же бинарь, а не снятый
        // `rexon-web-host`, которого у покупателя больше нет.
        assert!(web_host_missing().contains(WALLPAPER_HOST_BINARY));
        assert!(!web_host_missing().contains("rexon-web-host"));
        assert!(video_host_missing_message().contains("reinstall Rexon"));
        assert!(video_host_missing_message().contains("cargo build"));
    }

    /// Расширение оболочки получает ТУ ЖЕ строку запуска, что своё окно:
    /// голого `python3` из `PATH` в ней нет ни при каком раскладе.
    #[test]
    fn shell_ext_gets_same_host_command_as_own_window() {
        let host = "/opt/rexon/bin/rexon-wallpaper-host";
        for de in ["gnome", "cinnamon"] {
            let plan = shell_ext_plan(de, "/v.mp4", host, true)
                .expect("схема расширения известна");
            let native = video_native_plan(host, "/v.mp4", true);
            assert_eq!(
                plan.daemon_argv.last().unwrap(),
                &gvariant_string_array(&native[0].argv)
            );
            assert_eq!(plan.daemon_argv[3], SHELL_EXT_DAEMON_KEY);
            // Голого `python3` из `PATH` в строке нет ни при каком раскладе:
            // запасной путь расширения снят вместе с Python-хостом.
            assert!(!plan.daemon_argv.last().unwrap().contains("python"));
            // Ключ не записался — это отказ, а не мелочь: без него
            // расширению нечем запускать демона.
            assert!(!plan.daemon_failed_message.is_empty());
        }
        let mute = shell_ext_plan("gnome", "/v.mp4", host, false)
            .expect("схема расширения известна");
        assert!(mute.daemon_argv.last().unwrap().contains("--no-audio"));
        // Окружениям со своим окном расширение не нужно.
        assert!(shell_ext_plan("kde", "/v.mp4", host, true).is_none());
    }

    /// Flash своим окном и через расширение оболочки — одна и та же строка
    /// хоста `--mode swf`; внешнего `ruffle` в ней нет.
    #[test]
    fn swf_goes_through_our_host_in_both_ways() {
        let host = "/opt/rexon/bin/rexon-wallpaper-host";
        let native = swf_host_plan(host, "/обои/ролик.swf");
        assert_eq!(native.len(), 1);
        assert_eq!(native[0].kind, step_kind::SPAWN_SESSION);
        assert_eq!(native[0].argv, vec![host, "--mode", "swf", "/обои/ролик.swf"]);
        assert_eq!(native[0].format, "swf");
        assert!(native[0].ok_message.contains("ролик.swf"));
        assert!(!native[0].argv.iter().any(|a| a.contains("ruffle")));
        for de in ["gnome", "cinnamon"] {
            let plan = swf_shell_ext_plan(de, "/обои/ролик.swf", host).expect("схема известна");
            assert_eq!(plan.daemon_argv.last().unwrap(), &gvariant_string_array(&native[0].argv));
            assert_eq!(plan.engine_argv.last().unwrap(), "swf");
            assert!(plan.ok_message.contains("Flash"));
        }
        assert!(swf_shell_ext_plan("kde", "/r.swf", host).is_none());
        // Видео через общий построитель не изменилось.
        let video = shell_ext_plan("gnome", "/v.mp4", host, true).unwrap();
        assert_eq!(video.engine_argv.last().unwrap(), "video");
        assert!(video.ok_message.contains("Video wallpaper"));
    }

    /// Запись массива для `gsettings set` — валидный текст GVariant даже
    /// когда в пути есть кавычка или обратная косая.
    #[test]
    fn gvariant_array_escapes_quotes_and_backslashes() {
        let items = vec![
            "/usr/bin/python3".to_string(),
            "/home/w/фильм \"тест\".mp4".to_string(),
            "back\\slash".to_string(),
        ];
        assert_eq!(
            gvariant_string_array(&items),
            "[\"/usr/bin/python3\",\"/home/w/фильм \\\"тест\\\".mp4\",\"back\\\\slash\"]"
        );
        assert_eq!(gvariant_string_array(&[]), "[]");
    }

    /// Список «живых» форматов обязан покрывать ровно те, что заводят
    /// отдельный процесс, — и не считать живой статичную картинку.
    #[test]
    fn live_formats_are_the_ones_that_spawn_a_process() {
        for fmt in ["video", "youtube", "animated", "web", "swf"] {
            assert!(is_live_format(fmt), "{fmt}");
        }
        assert!(!is_live_format("static"));
        assert!(!is_live_format(""));
        // Каждый живой формат обязан иметь имя для покупателя. По-английски
        // имя «video» совпадает со служебным словом, поэтому сверка — по
        // таблице имён, а не «имя ≠ ключ».
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let names = [
            ("video", "video"),
            ("youtube", "YouTube video"),
            ("animated", "GIF animation"),
            ("web", "web page"),
            ("swf", "Flash (SWF)"),
        ];
        assert_eq!(LIVE_FORMATS.len(), names.len());
        for fmt in LIVE_FORMATS {
            let want = names.iter().find(|(k, _)| k == fmt).map(|(_, v)| *v);
            assert_eq!(Some(format_label(fmt).as_str()), want, "{fmt}");
        }
    }

    /// YouTube «стрим» на Mutter/Muffin неисполним: там нет ни своего окна,
    /// ни wlr-layer-shell для mpvpaper — режим уходит в «скачать».
    #[test]
    fn youtube_stream_routes_to_download_on_shell_ext_sessions() {
        for de in ["gnome", "cinnamon"] {
            assert_eq!(youtube_effective_mode(de, "wayland", "stream"), "download");
            assert!(!youtube_mode_downgraded_note(de, "wayland", "stream").is_empty());
            // X11 у тех же окружений обходится обычным окном — не трогаем.
            assert_eq!(youtube_effective_mode(de, "x11", "stream"), "stream");
            assert!(youtube_mode_downgraded_note(de, "x11", "stream").is_empty());
        }
        // Остальные окружения как были.
        assert_eq!(youtube_effective_mode("kde", "wayland", "stream"), "stream");
        assert_eq!(youtube_effective_mode("kde", "wayland", "download"), "download");
        // Явно выбранное «скачать» пояснения не требует.
        assert!(youtube_mode_downgraded_note("gnome", "wayland", "download").is_empty());
    }

    /// А сообщение о неудачном скачивании не смеет советовать «стрим» там,
    /// где стрима нет: покупатель переключил бы режим, снова получил бы
    /// скачивание и ходил бы по кругу.
    #[test]
    fn download_failure_never_advises_stream_where_stream_is_impossible() {
        for de in ["gnome", "cinnamon"] {
            let msg = youtube_download_failed_message(de, "wayland");
            assert!(!msg.contains("streaming mode"), "{de}: {msg}");
            // Имя оболочки — человеческое, а не идентификатор из конфига.
            assert!(msg.contains(shell_ext_de_label(de)), "{de}: {msg}");
            assert!(!msg.contains(de), "{de}: сырой идентификатор в тексте");
            // X11 у тех же окружений стрим умеет — совет остаётся прежним.
            assert_eq!(
                youtube_download_failed_message(de, "x11"),
                youtube_download_failed()
            );
        }
        assert_eq!(
            youtube_download_failed_message("kde", "wayland"),
            youtube_download_failed()
        );
    }

    /// И сам план отказывается запускать mpvpaper там, где ему не на чем
    /// рисовать, — даже если mpvpaper установлен.
    #[test]
    fn youtube_plan_refuses_mpvpaper_on_mutter() {
        let tools = t(&["mpvpaper", "mpv", "xwinwrap"]);
        for de in ["gnome", "cinnamon"] {
            let plan = youtube_plan(
                de, "wayland", "https://youtu.be/x", true, 50, "1080p", &tools,
                "{monitor}", "1920x1080", &Hints::default(),
            );
            assert_eq!(plan[0].kind, step_kind::FAIL, "{de}");
            assert!(plan[0].err_message.contains("download"), "{de}");
        }
        // KDE-Wayland — прежний путь через mpvpaper.
        let kde = youtube_plan(
            "kde", "wayland", "https://youtu.be/x", true, 50, "1080p", &tools,
            "{monitor}", "1920x1080", &Hints::default(),
        );
        assert_eq!(kde[0].backend, "mpvpaper");
    }

    /// Тот же класс дефекта у GIF-обоев: swww и mpvpaper рисуют своим окном
    /// через wlr-layer-shell, которого у Mutter и Muffin нет, — а план про
    /// окружение не спрашивал вовсе и звал их безусловно. Покупатель на
    /// GNOME и Cinnamon-Wayland получал «swww или mpvpaper не установлены»
    /// при УСТАНОВЛЕННЫХ swww и mpvpaper либо запуск без картинки.
    #[test]
    fn animated_plan_refuses_layer_shell_tools_on_mutter() {
        let tools = t(&["swww", "mpvpaper", "mpv", "xwinwrap"]);
        for de in ["gnome", "cinnamon"] {
            let plan = animated_plan(
                de, "wayland", "/w.gif", &tools, "{monitor}", "1920x1080",
                &Hints::default(),
            );
            assert_eq!(plan.len(), 1, "{de}");
            assert_eq!(plan[0].kind, step_kind::FAIL, "{de}");
            // Ни swww, ни mpvpaper на этих окружениях запускать нельзя.
            assert!(!plan.iter().any(|s| s.backend == "swww"), "{de}");
            assert!(!plan.iter().any(|s| s.backend == "mpvpaper"), "{de}");
            // Имя оболочки — человеческое, а не идентификатор из конфига.
            assert!(plan[0].err_message.contains(shell_ext_de_label(de)), "{de}");
            assert!(!plan[0].err_message.contains(de), "{de}: сырой идентификатор");
            // X11 у тех же окружений обходится своим окном — путь прежний.
            let x11 = animated_plan(
                de, "x11", "/w.gif", &tools, "{monitor}", "1920x1080",
                &Hints::default(),
            );
            assert_eq!(x11[0].backend, "xwinwrap", "{de}");
        }
        // Wayland С wlr-layer-shell (KWin и wlroots) — как было: swww
        // первым, mpvpaper следом.
        let kde = animated_plan(
            "kde", "wayland", "/w.gif", &tools, "{monitor}", "1920x1080",
            &Hints::default(),
        );
        assert_eq!(kde[0].backend, "swww");
        assert!(kde.iter().any(|s| s.backend == "mpvpaper"));
        // И без swww остаётся один mpvpaper.
        let only_mpv = animated_plan(
            "hyprland", "wayland", "/w.gif", &t(&["mpvpaper"]), "{monitor}",
            "1920x1080", &Hints::default(),
        );
        assert_eq!(only_mpv[0].backend, "mpvpaper");
    }

    /// ⚠️ Веб-обои не показывались НИ НА ОДНОМ рабочем столе с 30.07.2026:
    /// хост `rexon/web_wallpaper.py` был удалён, а план продолжал звать
    /// `python -m rexon.web_wallpaper`. Процесс умирал сразу, а покупателю
    /// рапортовали «Веб-обои запущены». Этот тест сторожит, чтобы строка
    /// запуска больше никогда не указывала на удалённый модуль.
    #[test]
    fn web_plan_never_calls_the_deleted_python_module() {
        for session in ["wayland", "x11"] {
            let plan = web_plan(
                session, "/usr/bin/python3", "/w/index.html", "/w/index.html",
                &t(&["surf", "xwinwrap"]), "1920x1080", &Hints::default(), None,
            );
            for step in &plan {
                assert!(
                    !step.argv.iter().any(|a| a.contains("web_wallpaper")),
                    "{session}: план всё ещё зовёт удалённый модуль: {:?}",
                    step.argv
                );
            }
        }
    }

    /// Первым шагом идёт наш бинарь, а не Python; заданный путь к сборке
    /// побеждает поиск по `$PATH`.
    #[test]
    fn web_plan_spawns_our_host_first() {
        let host = "/дерево/rust/target/release/rexon-wallpaper-host";
        let plan = web_plan(
            "wayland", "/usr/bin/python3", "https://пример.рф", "https://пример.рф",
            &[], "1920x1080", &Hints::default(), Some(host),
        );
        assert_eq!(plan[0].argv, vec![host, "--mode", "web", "https://пример.рф"]);
        // На Wayland свой хост — единственный путь: xwinwrap там не работает.
        assert_eq!(plan.len(), 1);
        assert!(plan[0].terminal);
        // Обои живут дольше команды, поэтому шаг сеансовый: обычный SPAWN
        // движок обоев дожидается, и запуск выглядел бы вечно висящим.
        assert_eq!(plan[0].kind, step_kind::SPAWN_SESSION);

        // ⚠️ Бинаря нет нигде — отказ ДО запуска, а не запуск ничего.
        // `None` сюда приходит после того, как ядро уже обыскало и дерево
        // сборки, и `$PATH`; спавн в такой обстановке дал бы человеку
        // мигание пустого стола и «код 127» вместо ответа.
        let missing = web_plan(
            "wayland", "/usr/bin/python3", "/w/index.html", "/w/index.html",
            &[], "1920x1080", &Hints::default(), None,
        );
        assert_eq!(missing[0].kind, step_kind::FAIL);
        assert!(missing[0].err_message.contains(WALLPAPER_HOST_BINARY));
        assert!(missing[0].argv.is_empty(), "отказ ничего не запускает");
    }

    /// На X11 запасной путь через surf остаётся — и принимает как ссылку,
    /// так и локальный файл.
    #[test]
    fn web_plan_keeps_surf_fallback_on_x11() {
        // Хоста нет — но на X11 очередь обязана дойти до surf: отказ
        // первого шага здесь НЕ терминальный.
        let plan = web_plan(
            "x11", "/usr/bin/python3", "/w/index.html", "/w/index.html",
            &t(&["surf", "xwinwrap"]), "1920x1080", &Hints::default(), None,
        );
        assert_eq!(plan.len(), 2);
        assert!(!plan[0].terminal, "отказ не должен обрывать план на X11");
        assert_eq!(plan[1].backend, "surf");
        assert!(plan[1].argv.contains(&"file:///w/index.html".to_string()));

        let link = web_plan(
            "x11", "/usr/bin/python3", "https://a", "https://a",
            &t(&["surf", "xwinwrap"]), "1920x1080", &Hints::default(), None,
        );
        assert!(link[1].argv.contains(&"https://a".to_string()));
    }

    /// Геометрия окна xwinwrap: метку исполнитель раскрывает сам (по метке
    /// на выход), а голый размер, как и прежде, получает нулевое смещение —
    /// так зовёт план мост `rexon-py`, и команда у него та же, что была.
    #[test]
    fn xwinwrap_takes_the_whole_geometry_or_the_old_zero_offset() {
        let tools = t(&["xwinwrap", "mpv"]);
        let by_mark = animated_plan(
            "generic", "x11", "/w.gif", &tools, "{monitor}", GEOMETRY_MARK,
            &Hints::default(),
        );
        assert_eq!(by_mark[0].backend, "xwinwrap");
        assert!(
            by_mark[0].argv.contains(&GEOMETRY_MARK.to_string()),
            "{:?}", by_mark[0].argv
        );
        let by_size = animated_plan(
            "generic", "x11", "/w.gif", &tools, "{monitor}", "{resolution}",
            &Hints::default(),
        );
        assert!(
            by_size[0].argv.contains(&"{resolution}+0+0".to_string()),
            "{:?}", by_size[0].argv
        );
        // Веб через surf ходит тем же xwinwrap — и геометрию берёт так же.
        let web = web_plan(
            "x11", "", "/w/index.html", "/w/index.html",
            &t(&["surf", "xwinwrap"]), GEOMETRY_MARK, &Hints::default(), None,
        );
        assert!(
            web[1].argv.contains(&GEOMETRY_MARK.to_string()),
            "{:?}", web[1].argv
        );
    }

    /// Хост обоев один: видео и веб отличаются только значением режима в
    /// строке запуска, бинарь и место пути — одни и те же.
    #[test]
    fn one_host_argv_for_every_mode() {
        let host = "/opt/rexon/bin/rexon-wallpaper-host";
        assert_eq!(
            wallpaper_host_argv(host, HostMode::Video, "/v.mp4"),
            vec![host, "--mode", "video", "/v.mp4"]
        );
        assert_eq!(
            wallpaper_host_argv(host, HostMode::Web, "https://a"),
            vec![host, "--mode", "web", "https://a"]
        );
        // Строка видео своим окном и строка веба собраны одной функцией.
        let video = video_native_plan(host, "/v.mp4", true);
        let web = web_plan(
            "wayland", "", "/w/index.html", "/w/index.html", &[], "", &Hints::default(),
            Some(host),
        );
        assert_eq!(video[0].argv[..2], web[0].argv[..2]);
        assert_eq!(video[0].argv[2], "video");
        assert_eq!(web[0].argv[2], "web");
        // Значение ключа разбирается туда и обратно; незнакомое — None.
        for mode in HostMode::ALL {
            assert_eq!(HostMode::from_arg(mode.as_arg()), Some(mode));
        }
        assert_eq!(HostMode::from_arg("swf"), Some(HostMode::Swf));
        assert_eq!(
            wallpaper_host_argv(host, HostMode::Swf, "/r.swf"),
            vec![host, "--mode", "swf", "/r.swf"]
        );
        assert_eq!(HostMode::from_arg("flash"), None);
        assert_eq!(HostMode::from_arg(""), None);
        // Без ключа — видео: так звал хост прежний контракт, и такую строку
        // уже хранит расширение оболочки в gsettings покупателя.
        assert_eq!(HostMode::DEFAULT, HostMode::Video);
        // Коды возврата режимов не пересекаются с общими.
        let codes = [
            host_exit::OK,
            host_exit::NOTHING_TO_SHOW,
            host_exit::QML_FAILED,
            host_exit::FRAMES_FAILED,
            host_exit::VIDEO_NO_AUDIO,
            host_exit::WEB_PAGE_FAILED,
            host_exit::SWF_NO_GPU,
            host_exit::SWF_MOVIE_FAILED,
        ];
        let mut uniq = codes.to_vec();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), codes.len(), "коды возврата хоста пересеклись");
        // Хвостовой слэш корня не удваивается.
        assert!(!host_candidates("/дерево/", WALLPAPER_HOST_BINARY)[0].contains("//rust"));
    }

    /// Порядок поиска бинаря: дерево (release → debug) → `$PATH`.
    /// Ничего не нашли — `None`, и это отказ, а не переход на другой хост.
    #[test]
    fn wallpaper_host_lookup_order() {
        struct Fs {
            exists: Vec<String>,
            asked: Vec<String>,
            path_hit: bool,
        }
        impl DaemonFs for Fs {
            fn is_exec_file(&mut self, path: &str) -> bool {
                self.asked.push(path.to_string());
                self.exists.iter().any(|e| e == path)
            }
            fn which(&mut self, name: &str) -> Option<String> {
                self.asked.push(format!("which:{name}"));
                self.path_hit.then(|| format!("/usr/bin/{name}"))
            }
        }
        let release = format!("/дерево/rust/target/release/{WALLPAPER_HOST_BINARY}");
        let debug = format!("/дерево/rust/target/debug/{WALLPAPER_HOST_BINARY}");

        let mut both = Fs {
            exists: vec![release.clone(), debug.clone()],
            asked: Vec::new(),
            path_hit: true,
        };
        assert_eq!(
            find_wallpaper_host_binary(&mut both, "/дерево"),
            Some(release)
        );

        let mut only_debug = Fs {
            exists: vec![debug.clone()],
            asked: Vec::new(),
            path_hit: true,
        };
        assert_eq!(
            find_wallpaper_host_binary(&mut only_debug, "/дерево"),
            Some(debug)
        );

        let mut installed = Fs {
            exists: Vec::new(),
            asked: Vec::new(),
            path_hit: true,
        };
        assert_eq!(
            find_wallpaper_host_binary(&mut installed, "/дерево"),
            Some(format!("/usr/bin/{WALLPAPER_HOST_BINARY}"))
        );

        let mut nothing = Fs {
            exists: Vec::new(),
            asked: Vec::new(),
            path_hit: false,
        };
        assert_eq!(find_wallpaper_host_binary(&mut nothing, "/дерево"), None);
        assert_eq!(nothing.asked.len(), 3);
    }

    #[test]
    fn youtube_quality_maps_to_ytdl_format() {
        assert!(youtube_format("4k").contains("height<=2160"));
        assert!(youtube_format("неизвестно").contains("height<=1080"));
        assert_eq!(
            youtube_format("best"),
            "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best"
        );
    }

    #[test]
    fn moov_position_decides_remux() {
        let mut front = Vec::new();
        front.extend_from_slice(&16u32.to_be_bytes());
        front.extend_from_slice(b"ftyp");
        front.extend_from_slice(&[0u8; 8]);
        front.extend_from_slice(&32u32.to_be_bytes());
        front.extend_from_slice(b"moov");
        assert!(moov_at_front(&front));

        let mut tail = Vec::new();
        tail.extend_from_slice(&16u32.to_be_bytes());
        tail.extend_from_slice(b"ftyp");
        tail.extend_from_slice(&[0u8; 8]);
        tail.extend_from_slice(&32u32.to_be_bytes());
        tail.extend_from_slice(b"mdat");
        assert!(!moov_at_front(&tail));

        // мусор — не трогаем файл
        assert!(moov_at_front(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn sha1_matches_hashlib() {
        // hashlib.sha1(b"abc").hexdigest()
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha1_hex(b""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(faststart_key("/a/b.mp4", 10, 5).len(), 16);
    }

    #[test]
    fn video_validation_reports_reason() {
        assert_eq!(validate_video("/v.mp4", false, 0, 1024).1, "File not found");
        assert_eq!(
            validate_video("/v.png", true, 10, 1024).1,
            "Unsupported format: .png"
        );
        let (ok, msg) = validate_video("/v.mp4", true, 2 * 1024 * 1024 * 1024, 1024);
        assert!(!ok && msg.starts_with("The file is too large: 2048.0 MB"));
        assert!(validate_video("/v.mp4", true, 1024, 1024).0);
    }

    #[test]
    fn containments_pick_desktops_only() {
        let text = "[Containments][1]\nplugin=org.kde.panel\n\
                    [Containments][128]\nplugin=org.kde.desktopcontainment\n\
                    [Containments][77]\nplugin=org.kde.plasma.folder\n";
        assert_eq!(desktop_containment_ids(text), vec!["128", "77"]);
        assert_eq!(desktop_containment_ids("[General]\nx=1\n"), vec!["128"]);
    }

    /// Английские сообщения обоев — без русских букв; имя пакета из строки
    /// «чего не хватает» снимается так же, как по-русски.
    #[test]
    fn english_wallpaper_texts_have_no_russian() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let cyrillic = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
        let mut texts = vec![
            spawn_died_message(Some(1)),
            spawn_died_message(None),
            wallpaper_set_message(),
            video_host_missing_message(),
            youtube_mode_downgraded_note("gnome", "wayland", "stream"),
            youtube_no_ytdlp_message("Install the “yt-dlp” package"),
            youtube_unsupported_message("https://example.com"),
            youtube_download_failed(),
            youtube_download_failed_message("gnome", "wayland"),
            web_host_missing(),
            web_missing_message("/x/index.html"),
            scene_unsupported(),
            swf_missing_message("/x/a.swf"),
            application_unsupported(),
            stopped_message(Some("video")),
            stopped_message(None),
            unknown_format_message("xyz"),
            file_missing_message("/x/a.mp4"),
            install_deps_unknown_message(&["mpv".to_string()]),
            validate_video("/x/a.mp4", false, 0, 500).1,
            parse_gpu("01:00.0 VGA compatible controller: NVIDIA Corporation\n\tKernel driver in use: nvidia\n").warning,
        ];
        for fmt in ["static", "animated", "video", "youtube", "web", "swf", "scene", "application"] {
            texts.push(format_label(fmt));
            texts.push(format_title(fmt));
            for session in ["wayland", "x11"] {
                for item in missing_for_format(fmt, session, &|_| false) {
                    assert!(!dep_package_name(&item).contains(' '), "{fmt}: {item}");
                    texts.push(item);
                }
            }
        }
        for text in &texts {
            assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
        }
        assert_eq!(format_title("static"), "Static image");
        assert_eq!(dep_package_name("swww (recommended) or mpvpaper"), "swww");
    }

    #[test]
    fn gpu_warns_only_on_proprietary_nvidia() {
        let g = parse_gpu(
            "01:00.0 VGA compatible controller: NVIDIA Corporation\n\
             \tKernel driver in use: nvidia\n",
        );
        assert_eq!((g.vendor.as_str(), g.driver.as_str()), ("nvidia", "nvidia"));
        assert!(g.wayland_risk && !g.warning.is_empty());

        let n = parse_gpu(
            "01:00.0 VGA compatible controller: NVIDIA Corporation\n\
             \tKernel driver in use: nouveau\n",
        );
        assert!(!n.wayland_risk && n.warning.is_empty());
        assert_eq!(parse_gpu("").vendor, "unknown");
    }

    #[test]
    fn missing_deps_are_per_session() {
        let none = |_: &str| false;
        assert_eq!(missing_for_format("video", "wayland", &none), vec!["mpvpaper"]);
        assert_eq!(
            missing_for_format("video", "x11", &none),
            vec!["xwinwrap-git (AUR)", "mpv"]
        );
        // youtube тянет за собой зависимости video
        let yt = missing_for_format("youtube", "wayland", &none);
        assert_eq!(yt.last().unwrap(), "mpvpaper");
        assert!(missing_for_format("static", "x11", &none).is_empty());
        // Веб и Flash рисует наш хост: доустанавливать нечего ни в одной сессии,
        // и прежней подсказки «gtk4-layer-shell + webkit2gtk» больше нет.
        for fmt in ["web", "swf"] {
            for session in ["wayland", "x11", ""] {
                assert!(missing_for_format(fmt, session, &none).is_empty(), "{fmt}/{session}");
            }
        }
    }

    #[test]
    fn scene_no_longer_pretends_to_start() {
        // Своего сценового рендера нет и не будет; текст обязан звать во
        // вкладку, а не рассказывать про пропавший модуль.
        assert!(scene_unsupported().contains("no longer supported"));
        assert!(scene_unsupported().contains("Wallpaper Engine"));
        assert!(!scene_unsupported().contains("parallax"));
    }

    #[test]
    fn splitext_matches_python() {
        assert_eq!(split_ext("/a/b.mp4"), ("/a/b", ".mp4"));
        assert_eq!(split_ext("/a/b"), ("/a/b", ""));
        assert_eq!(split_ext("/a/.hidden"), ("/a/.hidden", ""));
        assert_eq!(split_ext("/a.b/c"), ("/a.b/c", ""));
    }

    #[test]
    fn kde_reset_script_writes_image_before_switching_plugin() {
        let s = kde_reset_script("/usr/share/wallpapers/Next/");
        let img = s.find("writeConfig").unwrap();
        let plug = s.find("wallpaperPlugin").unwrap();
        assert!(img < plug, "картинка обязана записаться до смены плагина");
        assert!(!kde_reset_script("").contains("writeConfig"));
    }

    #[test]
    fn desktop_icons_cover_every_de_that_has_them() {
        assert_eq!(desktop_icons_argvs("gnome", false).len(), 2);
        assert_eq!(desktop_icons_argvs("xfce", true)[0].last().unwrap(), "2");
        assert_eq!(desktop_icons_argvs("xfce", false)[0].last().unwrap(), "0");
        assert!(desktop_icons_argvs("kde", false).is_empty());
        assert_eq!(desktop_icons_argvs("cinnamon-mint", false).len(), 1);
    }

    /// Отсрочка проверки живучести обязана перекрывать самый медленный
    /// наблюдавшийся отказ (0,2 с) и не разрастаться: её платит КАЖДЫЙ
    /// формат обоев и каждый возврат фона.
    #[test]
    fn spawn_alive_grace_covers_slow_death_and_stays_short() {
        assert!(SPAWN_ALIVE_GRACE_SEC >= 0.2, "медленный отказ пройдёт как успех");
        assert!(SPAWN_ALIVE_GRACE_SEC <= 1.0, "применение обоев затормозится");
    }

    /// Текст про мгновенную смерть называет код возврата и не притворяется
    /// выводом процесса.
    #[test]
    fn spawn_died_message_names_exit_code() {
        assert!(spawn_died_message(Some(1)).contains("code 1"));
        assert!(!spawn_died_message(None).contains("code"));
    }

    /// Первая весть уходит сразу, повтор того же — никогда.
    #[test]
    fn bridge_announces_first_change_and_never_repeats_itself() {
        // Не говорили ещё ни разу — говорим.
        assert!(bridge_should_announce(true, "Ночной город", false, "", None, 0.0));
        // То же самое, что уже сказано, — молчим, сколько бы ни прошло.
        assert!(!bridge_should_announce(
            true,
            "Ночной город",
            true,
            "Ночной город",
            Some(0.0),
            1_000.0
        ));
        // Остановка после запуска — новость, и порог давно прошёл.
        assert!(bridge_should_announce(false, "", true, "Ночной город", Some(0.0), 1_000.0));
    }

    /// Пачка нажатий в витрине проходит одной вестью: порог считает время
    /// от НАШЕЙ прошлой вести, а не от смены обоев.
    #[test]
    fn bridge_quiet_period_swallows_a_burst() {
        let told_at = Some(100.0);
        // Другая сцена сразу после нашей вести — молчим.
        assert!(!bridge_should_announce(true, "Лес", true, "Ночной город", told_at, 100.5));
        // И даже остановка внутри порога — молчим (признак в состоянии моста
        // остаётся верным, и читающая сторона опознает остановку по нему).
        assert!(!bridge_should_announce(false, "", true, "Ночной город", told_at, 102.0));
        // Ровно на пороге — уже говорим.
        assert!(bridge_should_announce(
            true,
            "Лес",
            true,
            "Ночной город",
            told_at,
            100.0 + BRIDGE_QUIET_SEC
        ));
    }

    /// Порог отсекает пачку, но много меньше памяти читающей стороны (три
    /// минуты) — иначе он съедал бы вести, на которые та могла бы отозваться.
    #[test]
    fn bridge_quiet_period_covers_a_burst_and_stays_short() {
        assert!(BRIDGE_QUIET_SEC >= 2.0, "перебор в витрине пройдёт пачкой");
        assert!(BRIDGE_QUIET_SEC <= 30.0, "порог съест осмысленную весть");
    }

    #[test]
    fn идентификатор_окна_и_argv_свойств_берутся_из_одного_места() {
        let geometry = "WINDOW=10485771\nX=0\nY=0\nWIDTH=1280\nHEIGHT=800\nSCREEN=0\n";
        assert_eq!(parse_xdotool_window_id(geometry), Some(10485771));
        assert_eq!(parse_xdotool_window_id("WIDTH=1\n"), None);
        assert_eq!(parse_xdotool_window_id("WINDOW=не число\n"), None);
        let a = xprop_window_info_argv(10485771);
        assert_eq!(a[0], "xprop");
        assert!(a.contains(&"_NET_WM_WINDOW_TYPE".to_string()));
        assert!(a.contains(&"_NET_WM_PID".to_string()));
        // Спрашиваем ОДНИМ запуском, а не двумя.
        assert_eq!(a.iter().filter(|p| p.as_str() == "-id").count(), 1);
    }

    #[test]
    fn отсев_своего_окна_работает_и_на_wayland() {
        // KWin: тип 1 — рабочий стол, 2 — панель/док, плюс свой процесс.
        let kwin = "type: 1\npid: 5\n";
        assert!(kwin_window_is_wallpaper_layer(kwin, &[]));
        assert!(kwin_window_is_wallpaper_layer("type: 2\n", &[]));
        assert!(kwin_window_is_wallpaper_layer("pid: 5\n", &[5]));
        assert!(!kwin_window_is_wallpaper_layer("type: 0\npid: 5\n", &[9]));
        // Непонятый ответ — поведение прежнее.
        assert!(!kwin_window_is_wallpaper_layer("", &[5]));
        assert!(!kwin_window_is_wallpaper_layer("тарабарщина", &[5]));

        // Hyprland: типа окна в ответе нет, остаётся хозяин.
        let hypr = r#"{"pid": 5, "fullscreen": 2}"#;
        assert!(hyprctl_window_is_wallpaper_layer(hypr, &[5]));
        assert!(!hyprctl_window_is_wallpaper_layer(hypr, &[9]));
        assert!(!hyprctl_window_is_wallpaper_layer("{}", &[5]));
        assert!(!hyprctl_window_is_wallpaper_layer("не json", &[5]));
    }

    /// COSMIC: статичная картинка — в родной ключ cosmic-bg, а не своим
    /// демоном поверх него; без cosmic-bg — прежний общий путь.
    #[test]
    fn cosmic_static_wallpaper_goes_to_cosmic_bg() {
        let t = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let plan = static_plan("cosmic", "wayland", "/w.png", &t(&["cosmic-bg", "swaybg"]), "");
        assert_eq!(plan[0].kind, step_kind::COSMIC_BG);
        assert!(!plan[0].terminal, "неудача записи обязана уводить на запасной путь");
        assert_eq!(plan[1].backend, "swaybg");
        let plain = static_plan("cosmic", "wayland", "/w.png", &t(&["swaybg"]), "");
        assert!(plain.iter().all(|s| s.kind != step_kind::COSMIC_BG));
        assert!(PLAN_TOOLS.contains(&"cosmic-bg"));
        // Возврат после живых обоев — тем же ключом.
        assert!(RESTORE_DES.contains(&"cosmic"));
        assert_eq!(
            restore_static_plan("cosmic", "/w.png", &t(&["cosmic-bg"]), "")[0].kind,
            step_kind::COSMIC_BG
        );
        // Другим окружениям этот шаг не достаётся.
        for de in ["sway", "hyprland", "gnome", "kde"] {
            let p = static_plan(de, "wayland", "/w.png", &t(&["cosmic-bg", "swaybg"]), "");
            assert!(p.iter().all(|s| s.kind != step_kind::COSMIC_BG), "{de}");
        }
    }

    /// Запись `Entry` — в виде `ron::ser::to_string_pretty`, с чужими полями
    /// прежней записи; незнакомое значение — умолчание cosmic-bg.
    #[test]
    fn cosmic_bg_entry_keeps_foreign_fields() {
        let shipped = "(\n    output: \"all\",\n    source: Path(\"/usr/share/backgrounds/cosmic/orion_nebula_nasa_heic0601a.jpg\"),\n    filter_by_theme: #true,\n    rotation_frequency: 3600,\n    filter_method: Lanczos,\n    scaling_mode: Zoom,\n    sampling_method: Alphanumeric,\n)\n";
        let fresh = cosmic_bg_entry(None, "/home/u/Обои/a \"b\".png").unwrap();
        assert_eq!(
            fresh,
            "(\n    output: \"all\",\n    source: Path(\"/home/u/Обои/a \\\"b\\\".png\"),\n    filter_by_theme: false,\n    rotation_frequency: 900,\n    filter_method: Lanczos,\n    scaling_mode: Zoom,\n    sampling_method: Alphanumeric,\n)"
        );
        assert_eq!(cosmic_bg_source_path(&fresh).as_deref(), Some("/home/u/Обои/a \"b\".png"));
        let kept = cosmic_bg_entry(Some(shipped), "/w.png").unwrap();
        assert!(kept.contains("filter_by_theme: true,") && kept.contains("rotation_frequency: 3600,"), "{kept}");
        let fit = shipped.replace("scaling_mode: Zoom", "scaling_mode: Fit((0.0, 0.0, 0.0))").replace("Lanczos", "Nearest");
        let kept = cosmic_bg_entry(Some(&fit), "/w.png").unwrap();
        assert!(kept.contains("scaling_mode: Fit((0.0, 0.0, 0.0)),") && kept.contains("filter_method: Nearest,"), "{kept}");
        let odd = shipped.replace("scaling_mode: Zoom", "scaling_mode: Fit((").replace("Alphanumeric", "Shuffle");
        let kept = cosmic_bg_entry(Some(&odd), "/w.png").unwrap();
        assert!(kept.contains("scaling_mode: Zoom,") && kept.contains("sampling_method: Alphanumeric,"), "{kept}");
        // Управляющий символ в пути — не пишем вовсе.
        assert!(cosmic_bg_entry(None, "/a\nb.png").is_none());
        // Цвет вместо картинки — пути нет.
        assert_eq!(cosmic_bg_source_path("(\n    source: Color(Single((0.1, 0.2, 0.3))),\n)"), None);
    }

    /// Исполнение: ключ `all` и `same-on-all = true` в `$XDG_CONFIG_HOME`;
    /// уже стоящий `true` не переписывается; не перечиталось — не успех.
    #[test]
    fn apply_cosmic_bg_writes_both_keys() {
        use std::cell::RefCell;
        use std::collections::HashMap;
        let fs: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
        let writes = RefCell::new(Vec::new());
        let ok = apply_cosmic_bg(
            "/cfg/",
            "/w.png",
            &mut |p| fs.borrow().get(p).cloned(),
            &mut |p, t| {
                writes.borrow_mut().push(p.to_string());
                fs.borrow_mut().insert(p.to_string(), t.to_string());
                true
            },
        );
        assert!(ok);
        let dir = "/cfg/cosmic/com.system76.CosmicBackground/v1";
        assert_eq!(*writes.borrow(), vec![format!("{dir}/all"), format!("{dir}/same-on-all")]);
        assert_eq!(fs.borrow()[&format!("{dir}/same-on-all")], "true");
        writes.borrow_mut().clear();
        assert!(apply_cosmic_bg(
            "/cfg",
            "/v.png",
            &mut |p| fs.borrow().get(p).cloned(),
            &mut |p, t| {
                writes.borrow_mut().push(p.to_string());
                fs.borrow_mut().insert(p.to_string(), t.to_string());
                true
            },
        ));
        assert_eq!(*writes.borrow(), vec![format!("{dir}/all")]);
        // Запись «удалась», но на диске не то — честный отказ.
        assert!(!apply_cosmic_bg("/cfg", "/x.png", &mut |p| fs.borrow().get(p).cloned(), &mut |_, _| true));
    }

    /// Разбор состояния расширения: числа `ExtensionState` gnome-shell 45–51.
    #[test]
    fn shell_ext_state_is_parsed_from_gdbus_and_gsettings() {
        use ShellExtState::*;
        let gnome = |out: &str| parse_shell_ext_state("gnome", Some(out));
        let info = |state: &str| {
            format!("({{'uuid': <'{SHELL_EXT_ID}'>, 'version': <1.0>, 'state': <{state}>}},)\n")
        };
        assert_eq!(gnome(&info("1.0")), Enabled);
        assert_eq!(gnome(&info("2.0")), Disabled);
        assert_eq!(gnome(&info("3.0")), Error);
        assert_eq!(gnome(&info("4.0")), OutOfDate);
        assert_eq!(gnome(&info("6.0")), Disabled);
        assert_eq!(gnome(&info("99.0")), NotLoaded);
        assert_eq!(gnome(&info("42.0")), Unknown);
        assert_eq!(gnome("(@a{sv} {},)\n"), NotLoaded);
        assert_eq!(gnome(""), Unknown);
        assert_eq!(parse_shell_ext_state("gnome", None), Unknown);

        let cinnamon = |out: &str| parse_shell_ext_state("cinnamon", Some(out));
        assert_eq!(cinnamon(&format!("['a@b', '{SHELL_EXT_ID}']\n")), Enabled);
        assert_eq!(cinnamon("@as []\n"), Disabled);
        assert_eq!(cinnamon(""), Unknown);

        assert!(shell_ext_state_message(Enabled, "x").is_none());
        assert_eq!(shell_ext_state_message(Disabled, "x"), Some((true, "x".to_string())));
        assert!(!shell_ext_state_message(Error, "x").unwrap().0);
        assert!(!shell_ext_state_message(OutOfDate, "x").unwrap().0);
    }

    /// Каталоги схемы расширения: своя копия раньше системных, `--schemadir`
    /// встаёт сразу за `gsettings`.
    #[test]
    fn shell_ext_schema_dir_goes_right_after_gsettings() {
        let dirs = shell_ext_schema_dirs("gnome", "/home/u", "", "");
        assert_eq!(
            dirs,
            vec![
                format!("/home/u/.local/share/gnome-shell/extensions/{SHELL_EXT_ID}/schemas"),
                format!("/usr/local/share/gnome-shell/extensions/{SHELL_EXT_ID}/schemas"),
                format!("/usr/share/gnome-shell/extensions/{SHELL_EXT_ID}/schemas"),
            ]
        );
        let dirs = shell_ext_schema_dirs("cinnamon", "/home/u", "/x/data/", "/opt/share");
        assert_eq!(
            dirs,
            vec![
                format!("/x/data/cinnamon/extensions/{SHELL_EXT_ID}/schemas"),
                format!("/opt/share/cinnamon/extensions/{SHELL_EXT_ID}/schemas"),
            ]
        );
        assert!(shell_ext_schema_dirs("kde", "/home/u", "", "").is_empty());

        let plan = shell_ext_plan("gnome", "/v.mp4", "/h", true).unwrap();
        assert_eq!(
            with_schemadir(&plan.source_argv, "/d"),
            argv(&[
                "gsettings",
                "--schemadir",
                "/d",
                "set",
                "org.gnome.shell.extensions.rexon-wallpaper",
                "source",
                "/v.mp4",
            ])
        );
        let gdbus = shell_ext_state_argv("gnome");
        assert_eq!(with_schemadir(&gdbus, "/d"), gdbus);
    }
}
