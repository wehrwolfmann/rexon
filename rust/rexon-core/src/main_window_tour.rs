//! Режим обхода главного окна: `rexon-main-window --tour[=<раздел>] --tour-out <папка>`.
//!
//! ЗАЧЕМ. Магазинов в «Оформлении» десятки (раздел × KDE Store / AUR / Все /
//! Wallhaven / Razer Axon / Spices / сайт расширений GNOME), и проверять их
//! глазами — час щелчков, после которого всё равно что-то пропущено: плитки
//! без картинок, забытое слово поиска при смене магазина, поле поиска,
//! уехавшее в одном магазине раздела. Обход проходит всё сам, невидимо
//! (`QT_QPA_PLATFORM=offscreen`), снимает каждый экран в PNG и пишет
//! `report.txt` со всем подозрительным.
//!
//! КТО ЧТО РЕШАЕТ. Здесь — всё: разбор ключей, порядок обхода, какие вызовы
//! страницы разрешены, когда экран «загрузился», что считать подозрительным
//! и вид отчёта. Окно (`rexon-gui`, `qml/MainWindow.qml`) только исполняет
//! команды [`Tour::next`] и возвращает, что увидело.
//!
//! ⛔ ОБХОД НИЧЕГО НЕ МЕНЯЕТ. Только навигация, поиск и просмотр: команды
//! называют лишь методы из [`ALLOWED_CALLS`], а [`call_allowed`] дополнительно
//! отвергает всё, в чьём имени есть установка, применение, удаление и родня
//! ([`FORBIDDEN_WORDS`]). Исполнитель обязан спрашивать [`call_allowed`]
//! перед КАЖДЫМ вызовом.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ============================================================================
// Ключи командной строки
// ============================================================================

/// `--tour` (все разделы) или `--tour=<раздел>[/<подраздел>]`.
pub const TOUR_FLAG: &str = "--tour";
/// Папка снимков и отчёта: `--tour-out <папка>` или `--tour-out=<папка>`.
pub const OUT_FLAG: &str = "--tour-out";
/// Тестовое слово поиска: `--tour-query <слово>`.
pub const QUERY_FLAG: &str = "--tour-query";
/// Общий предел обхода в секундах: `--tour-timeout <с>`.
pub const TIMEOUT_FLAG: &str = "--tour-timeout";

/// Слово поиска по умолчанию: находит что-то почти в любом магазине.
pub const DEFAULT_QUERY: &str = "girl";
/// Общий предел обхода по умолчанию. Без сети каждый магазин упирается в
/// [`LOAD_TIMEOUT_MS`], и обход всего окна обязан всё равно кончиться.
pub const DEFAULT_TIMEOUT_SECS: u64 = 1200;

/// Сколько ждать загрузки выдачи одного экрана.
pub const LOAD_TIMEOUT_MS: u64 = 20_000;
/// Загрузка дольше — подозрительно (строка отчёта).
pub const SLOW_LOAD_MS: u64 = 5_000;
/// Сколько ждать картинок плиток после загрузки выдачи.
pub const PREVIEW_TIMEOUT_MS: u64 = 10_000;
/// Очередь картинок пуста и число плиток без картинки не менялось столько —
/// дальше ждать нечего (картинки вида, которые рисуются на месте, очереди
/// загрузок не видны).
pub const PREVIEW_SETTLE_MS: u64 = 1_500;
/// Меньше этого «готово» не бывает: запрос магазина уходит после паузы
/// набора (`catalogDebounceMs`), и до неё экран ещё показывает прошлое.
pub const MIN_LOAD_MS: u64 = 400;
/// Сколько ждать, пока раздел окна построит страницу.
pub const PAGE_TIMEOUT_MS: u64 = 15_000;

/// Размер невидимого окна обхода: снимки читаемы, шапка в одну строку.
pub const WINDOW_SIZE: (i32, i32) = (1366, 860);

/// Отчёт в папке обхода.
pub const REPORT_FILE: &str = "report.txt";
/// Журнал процесса обхода (поток ошибок: сообщения Qt/QML и журнал Rexon).
pub const JOURNAL_FILE: &str = "journal.log";

/// Коды возврата: всё чисто / есть подозрительное / не запустился / общий
/// тайм-аут.
pub const EXIT_CLEAN: i32 = 0;
pub const EXIT_SUSPICIOUS: i32 = 1;
pub const EXIT_BAD_ARGS: i32 = 2;
pub const EXIT_TIMEOUT: i32 = 3;

/// Что попросили обойти.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TourConfig {
    /// Раздел окна (`qt_main::SECTIONS`); `None` — все.
    pub section: Option<String>,
    /// Подраздел «Оформления» (ключ боковой панели); `None` — все.
    pub sub: Option<String>,
    pub query: String,
    pub out_dir: PathBuf,
    pub timeout_secs: u64,
}

impl TourConfig {
    /// Как обход назван в отчёте.
    pub fn target_label(&self) -> String {
        match (&self.section, &self.sub) {
            (None, _) => "всё окно".to_string(),
            (Some(s), None) => s.clone(),
            (Some(s), Some(sub)) => format!("{s}/{sub}"),
        }
    }
}

/// Просили ли обход вообще (до разбора остальных ключей).
pub fn wants_tour(args: &[String]) -> bool {
    args.iter().any(|a| a == TOUR_FLAG || a.starts_with(&format!("{TOUR_FLAG}=")))
}

/// Значение ключа `--flag v` / `--flag=v`.
fn flag_value(args: &[String], flag: &str) -> Result<Option<String>, String> {
    let eq = format!("{flag}=");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == flag {
            return match it.next() {
                Some(v) if !v.starts_with("--") => Ok(Some(v.clone())),
                _ => Err(format!("у ключа {flag} нет значения")),
            };
        }
        if let Some(v) = a.strip_prefix(&eq) {
            return Ok(Some(v.to_string()));
        }
    }
    Ok(None)
}

/// Разобрать ключи обхода. `Ok(None)` — обхода не просили.
///
/// `default_out` зовётся, только если `--tour-out` не дан.
pub fn parse_args(
    args: &[String],
    default_out: impl FnOnce() -> PathBuf,
) -> Result<Option<TourConfig>, String> {
    if !wants_tour(args) {
        return Ok(None);
    }
    let target = args
        .iter()
        .find_map(|a| a.strip_prefix(&format!("{TOUR_FLAG}=")))
        .map(str::trim)
        .unwrap_or("");
    let (section, sub) = match target.split_once('/') {
        Some((s, sub)) => (s.trim(), sub.trim()),
        None => (target, ""),
    };
    if !section.is_empty() && !crate::qt_main::SECTIONS.contains(&section) {
        return Err(format!(
            "раздела «{section}» нет; есть: {}",
            crate::qt_main::SECTIONS.join(", ")
        ));
    }
    if !sub.is_empty() && section != "appearance" {
        return Err(format!(
            "подразделы обход знает только у «appearance», а не у «{section}»"
        ));
    }
    let query = flag_value(args, QUERY_FLAG)?.unwrap_or_else(|| DEFAULT_QUERY.to_string());
    if query.trim().is_empty() {
        return Err("слово поиска пустое".to_string());
    }
    let timeout_secs = match flag_value(args, TIMEOUT_FLAG)? {
        Some(v) => v
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| format!("{TIMEOUT_FLAG}: ждали число секунд, а не «{v}»"))?,
        None => DEFAULT_TIMEOUT_SECS,
    };
    let out_dir = match flag_value(args, OUT_FLAG)? {
        Some(v) if !v.trim().is_empty() => PathBuf::from(v.trim()),
        Some(_) => return Err(format!("{OUT_FLAG}: пустой путь")),
        None => default_out(),
    };
    Ok(Some(TourConfig {
        section: (!section.is_empty()).then(|| section.to_string()),
        sub: (!sub.is_empty()).then(|| sub.to_string()),
        query: query.trim().to_string(),
        out_dir,
        timeout_secs,
    }))
}

/// Папка по умолчанию: `$XDG_CACHE_HOME/rexon/tour` (или `~/.cache/…`).
pub fn default_out_dir(env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    let cache = env("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| Path::new(&h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("."));
    cache.join("rexon").join("tour")
}

/// Переменные Qt процесса обхода: невидимое окно и сообщения Qt/QML в поток
/// ошибок с типом в начале строки — по нему [`is_qml_warning`] отличает
/// предупреждение от `console.log`.
pub fn qt_env() -> Vec<(&'static str, &'static str)> {
    vec![
        ("QT_QPA_PLATFORM", "offscreen"),
        ("QT_FORCE_STDERR_LOGGING", "1"),
        ("QT_MESSAGE_PATTERN", "%{type}: %{if-category}%{category}: %{endif}%{message}"),
    ]
}

/// Файл, оставленный прошлым обходом (его можно убрать перед новым): снимки
/// `NN-….png`, отчёт и журнал. Чужие файлы в папке не трогаются.
pub fn is_tour_artifact(name: &str) -> bool {
    if name == REPORT_FILE || name == JOURNAL_FILE {
        return true;
    }
    let b = name.as_bytes();
    name.ends_with(".png") && b.len() > 3 && b[0].is_ascii_digit() && b[1].is_ascii_digit() && b[2] == b'-'
}

// ============================================================================
// Запрет на действия
// ============================================================================

/// Единственные методы страниц, которые обход вообще зовёт.
pub const ALLOWED_CALLS: &[&str] = &[
    "openSection",
    "tourSelectCategory",
    "tourSetSource",
    "tourTypeQuery",
    "tourClearQuery",
];

/// Слова, которых в имени вызова обхода быть не может ни при каких условиях:
/// за ними стоят изменения в системе (установка, применение, удаление…).
pub const FORBIDDEN_WORDS: &[&str] = &[
    "install", "apply", "uninstall", "remove", "delete", "enable", "disable",
    "toggle", "save", "write", "restore", "reset", "switch", "set_default",
    "setdefault", "play", "launch", "run", "update", "upgrade", "download",
];

/// Можно ли обходу позвать метод `method`. Исполнитель спрашивает это перед
/// каждым вызовом и на `false` вызов не делает, а пишет провал.
pub fn call_allowed(method: &str) -> bool {
    if !ALLOWED_CALLS.contains(&method) {
        return false;
    }
    // Имя без приставки `tour`: иначе «tourSetSource» прошло бы мимо
    // проверки слов по совпадению «set».
    let bare = method.strip_prefix("tour").unwrap_or(method).to_ascii_lowercase();
    !FORBIDDEN_WORDS.iter().any(|w| bare.contains(w))
}

// ============================================================================
// Команды и ответы исполнителя
// ============================================================================

/// Команда исполнителю. `method` — вызов страницы, который за ней стоит
/// (сверяется [`call_allowed`]); у снимка и конца вызова нет.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "cmd", rename_all = "camelCase")]
pub enum Command {
    /// Открыть раздел окна и дождаться его страницы.
    OpenSection { method: String, section: String, timeout_ms: u64 },
    /// Открыть раздел боковой панели «Оформления».
    SelectCategory { method: String, key: String },
    /// Выбрать магазин (чип «Магазин:»).
    SetSource { method: String, key: String },
    /// Ввести слово в поле поиска (как человек: поле + Enter).
    TypeQuery { method: String, text: String },
    /// Очистить поиск (кнопкой ✕).
    ClearQuery { method: String },
    /// Дождаться загрузки и картинок, снять окно в `path`.
    Capture {
        file: String,
        path: String,
        load_timeout_ms: u64,
        preview_timeout_ms: u64,
        settle_ms: u64,
        grid: bool,
    },
    /// Обход окончен: исполнитель выходит с `exit_code`.
    Finish { exit_code: i32, report: String },
}

impl Command {
    fn open_section(section: &str) -> Self {
        Self::OpenSection { method: "openSection".into(), section: section.into(), timeout_ms: PAGE_TIMEOUT_MS }
    }
    fn select_category(key: &str) -> Self {
        Self::SelectCategory { method: "tourSelectCategory".into(), key: key.into() }
    }
    fn set_source(key: &str) -> Self {
        Self::SetSource { method: "tourSetSource".into(), key: key.into() }
    }
    fn type_query(text: &str) -> Self {
        Self::TypeQuery { method: "tourTypeQuery".into(), text: text.into() }
    }
    fn clear_query() -> Self {
        Self::ClearQuery { method: "tourClearQuery".into() }
    }

    /// Вызов страницы за командой.
    pub fn method(&self) -> Option<&str> {
        match self {
            Self::OpenSection { method, .. }
            | Self::SelectCategory { method, .. }
            | Self::SetSource { method, .. }
            | Self::TypeQuery { method, .. }
            | Self::ClearQuery { method } => Some(method),
            Self::Capture { .. } | Self::Finish { .. } => None,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{\"cmd\":\"finish\",\"exit_code\":1}".to_string())
    }
}

/// Что исполнитель увидел, выполнив прошлую команду. Поля, не относящиеся к
/// команде, остаются по умолчанию.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Report {
    pub ok: bool,
    pub error: String,
    // ── openSection
    pub built: bool,
    /// `appearance` — страница с боковой панелью и магазинами; иначе пусто.
    pub kind: String,
    /// Доступные разделы боковой панели (ключи) в порядке панели.
    pub categories: Vec<String>,
    // ── selectCategory
    /// Магазины раздела (ключи чипов) в порядке шапки.
    pub sources: Vec<String>,
    /// Есть ли в разделе поле поиска.
    pub search: bool,
    // ── capture
    pub saved: bool,
    /// Вид страницы: `grid`, `axon`, `detail`…
    pub screen: String,
    pub load_ms: u64,
    pub load_timed_out: bool,
    pub tiles: i64,
    pub no_preview: i64,
    pub preview_timed_out: bool,
    pub search_visible: bool,
    pub search_x: i64,
    pub search_y: i64,
    pub search_text: String,
    pub model_query: String,
    pub status: String,
    pub status_warn: bool,
    /// Действующий магазин по словам страницы.
    pub source: String,
    /// Длина журнала к моменту ответа — ставит хозяин окна, не QML.
    pub log_offset: u64,
}

impl Report {
    pub fn from_json(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_else(|e| Self {
            error: format!("ответ исполнителя не разобран: {e}"),
            ..Self::default()
        })
    }
}

// ============================================================================
// Ожидание загрузки
// ============================================================================

/// Замер исполнителя, пока он ждёт экран.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct WaitSample {
    /// `load` — ждём выдачу; `preview` — ждём картинки плиток.
    pub phase: String,
    pub elapsed_ms: u64,
    pub timeout_ms: u64,
    // load
    pub busy: bool,
    pub status: String,
    // preview
    pub no_preview: i64,
    pub previews_pending: i64,
    pub stable_ms: u64,
    pub settle_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitVerdict {
    Wait,
    Ready,
    Timeout,
}

impl WaitVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wait => "wait",
            Self::Ready => "ready",
            Self::Timeout => "timeout",
        }
    }
}

/// Строка состояния страницы говорит «грузится».
pub fn status_says_busy(status: &str) -> bool {
    let s = status.trim();
    s.starts_with('⏳')
        || s == crate::appearance_page::status_loading_catalog()
        || s == crate::appearance_page::status_loading_aur()
        || s == crate::appearance_page::status_loading_scan()
        || s == crate::appearance_page::status_scanning_system()
}

/// Готов ли экран к снимку.
pub fn wait_verdict(s: &WaitSample) -> WaitVerdict {
    let timed_out = s.timeout_ms > 0 && s.elapsed_ms >= s.timeout_ms;
    match s.phase.as_str() {
        "preview" => {
            if s.no_preview <= 0 || (s.previews_pending <= 0 && s.stable_ms >= s.settle_ms) {
                WaitVerdict::Ready
            } else if timed_out {
                WaitVerdict::Timeout
            } else {
                WaitVerdict::Wait
            }
        }
        _ => {
            if s.busy || status_says_busy(&s.status) || s.elapsed_ms < MIN_LOAD_MS {
                if timed_out { WaitVerdict::Timeout } else { WaitVerdict::Wait }
            } else {
                WaitVerdict::Ready
            }
        }
    }
}

// ============================================================================
// Порядок обхода
// ============================================================================

/// Шаг экрана: просмотр магазина или выдача по поиску.
pub const STEP_BROWSE: &str = "browse";
pub const STEP_SEARCH: &str = "search";

/// Чей это снимок.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotMeta {
    pub section: String,
    /// Раздел боковой панели «Оформления»; пусто у остальных разделов окна.
    pub category: String,
    /// Магазин; `-`, если переключателя нет.
    pub store: String,
    pub step: String,
    /// Какое слово обязано стоять в поле поиска.
    pub expect_query: Option<String>,
    /// Экран — сетка плиток (у прочих разделов окна плиток не считаем).
    pub grid: bool,
}

/// Строка плана: команда и, для снимка, чей он.
#[derive(Debug, Clone, PartialEq)]
pub enum Planned {
    Do(Command),
    Shot(ShotMeta),
}

/// План раздела «Оформления»: каждый магазин по очереди, затем — если
/// поиск есть — слово в первом магазине и снова каждый магазин, с проверкой,
/// что слово осталось в поле. В конце поиск очищается.
pub fn plan_category(section: &str, category: &str, sources: &[String], search: bool, query: &str) -> Vec<Planned> {
    let shot = |store: &str, step: &str, expect: Option<&str>| {
        Planned::Shot(ShotMeta {
            section: section.to_string(),
            category: category.to_string(),
            store: store.to_string(),
            step: step.to_string(),
            expect_query: expect.map(str::to_string),
            grid: true,
        })
    };
    let mut out = Vec::new();
    if sources.is_empty() {
        out.push(shot("-", STEP_BROWSE, None));
        if search {
            out.push(Planned::Do(Command::type_query(query)));
            out.push(shot("-", STEP_SEARCH, Some(query)));
            out.push(Planned::Do(Command::clear_query()));
        }
        return out;
    }
    for src in sources {
        out.push(Planned::Do(Command::set_source(src)));
        out.push(shot(src, STEP_BROWSE, None));
    }
    if search {
        for (i, src) in sources.iter().enumerate() {
            out.push(Planned::Do(Command::set_source(src)));
            if i == 0 {
                out.push(Planned::Do(Command::type_query(query)));
            }
            out.push(shot(src, STEP_SEARCH, Some(query)));
        }
        out.push(Planned::Do(Command::clear_query()));
    }
    // Вернуть раздел на первый магазин: следующий раздел начнёт с чистого.
    if sources.len() > 1 {
        out.push(Planned::Do(Command::set_source(&sources[0])));
    }
    out
}

/// Имя файла снимка: `NN-<раздел>-<магазин>-<шаг>.png`.
pub fn shot_file_name(n: u32, place: &str, store: &str, step: &str) -> String {
    format!("{n:02}-{}-{}-{}.png", slug(place), slug(store), slug(step))
}

fn slug(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    let out = out.trim_matches('_').to_string();
    if out.is_empty() { "main".to_string() } else { out }
}

/// Снятый экран: чей он и что на нём увидели.
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    pub file: String,
    pub meta: ShotMeta,
    pub seen: Report,
}

/// Строка подозрительного: к какому снимку и что.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Имя снимка или место (`запуск`, `раздел appearance`…).
    pub place: String,
    pub text: String,
}

/// Состояние обхода.
#[derive(Debug)]
pub struct Tour {
    pub cfg: TourConfig,
    queue: VecDeque<Planned>,
    /// Последняя выданная команда и, для снимка, чей он.
    current: Option<(Command, Option<ShotMeta>)>,
    screens: Vec<Screen>,
    notes: Vec<Finding>,
    shots: u32,
    section: String,
    category: String,
    finished: bool,
}

impl Tour {
    pub fn new(cfg: TourConfig) -> Self {
        let sections: Vec<String> = match &cfg.section {
            Some(s) => vec![s.clone()],
            None => crate::qt_main::SECTIONS.iter().map(|s| s.to_string()).collect(),
        };
        let queue = sections.iter().map(|s| Planned::Do(Command::open_section(s))).collect();
        Self {
            cfg,
            queue,
            current: None,
            screens: Vec::new(),
            notes: Vec::new(),
            shots: 0,
            section: String::new(),
            category: String::new(),
            finished: false,
        }
    }

    pub fn screens(&self) -> &[Screen] {
        &self.screens
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn note(&mut self, place: impl Into<String>, text: impl Into<String>) {
        self.notes.push(Finding { place: place.into(), text: text.into() });
    }

    /// Принять ответ на прошлую команду и выдать следующую. Первый вызов —
    /// с `None`. После [`Command::Finish`] отвечает им же.
    ///
    /// `journal` нужен только для отчёта в конце (см. [`Tour::report`]).
    pub fn next(&mut self, report: Option<Report>, journal: &str) -> Command {
        if let (Some((cmd, meta)), Some(rep)) = (self.current.take(), report) {
            self.absorb(cmd, meta, rep);
        }
        loop {
            let Some(planned) = self.queue.pop_front() else {
                self.finished = true;
                let (text, code) = self.report(journal, None);
                return Command::Finish { exit_code: code, report: text };
            };
            let (cmd, meta) = match planned {
                Planned::Do(cmd) => (cmd, None),
                Planned::Shot(meta) => {
                    self.shots += 1;
                    let place = if meta.category.is_empty() { &meta.section } else { &meta.category };
                    let file = shot_file_name(self.shots, place, &meta.store, &meta.step);
                    let path = self.cfg.out_dir.join(&file).to_string_lossy().into_owned();
                    let cmd = Command::Capture {
                        file,
                        path,
                        load_timeout_ms: LOAD_TIMEOUT_MS,
                        preview_timeout_ms: PREVIEW_TIMEOUT_MS,
                        settle_ms: PREVIEW_SETTLE_MS,
                        grid: meta.grid,
                    };
                    (cmd, Some(meta))
                }
            };
            // Последняя линия запрета: команда с недозволенным вызовом не
            // уходит исполнителю вовсе.
            if let Some(m) = cmd.method() {
                if !call_allowed(m) {
                    self.note("обход", format!("вызов «{m}» запрещён — пропущен"));
                    continue;
                }
            }
            self.current = Some((cmd.clone(), meta));
            return cmd;
        }
    }

    fn absorb(&mut self, cmd: Command, meta: Option<ShotMeta>, rep: Report) {
        if !rep.error.is_empty() {
            let place = match &cmd {
                Command::Capture { file, .. } => file.clone(),
                _ => self.place(),
            };
            self.note(place, format!("исполнитель: {}", rep.error));
        }
        match cmd {
            Command::OpenSection { section, .. } => {
                self.section = section.clone();
                self.category.clear();
                if !rep.built {
                    self.note(format!("раздел {section}"), "страница раздела не построилась");
                    return;
                }
                if rep.kind == "appearance" {
                    let cats: Vec<String> = match &self.cfg.sub {
                        Some(sub) if rep.categories.contains(sub) => vec![sub.clone()],
                        Some(sub) => {
                            self.note(
                                format!("раздел {section}"),
                                format!(
                                    "подраздела «{sub}» нет среди доступных: {}",
                                    rep.categories.join(", ")
                                ),
                            );
                            Vec::new()
                        }
                        None => rep.categories.clone(),
                    };
                    if cats.is_empty() && self.cfg.sub.is_none() {
                        self.note(format!("раздел {section}"), "боковая панель пуста");
                    }
                    for key in cats.iter().rev() {
                        self.queue.push_front(Planned::Do(Command::select_category(key)));
                    }
                } else {
                    self.queue.push_front(Planned::Shot(ShotMeta {
                        section: section.clone(),
                        category: String::new(),
                        store: "-".to_string(),
                        step: STEP_BROWSE.to_string(),
                        expect_query: None,
                        grid: false,
                    }));
                }
            }
            Command::SelectCategory { key, .. } => {
                self.category = key.clone();
                if !rep.ok {
                    self.note(self.place(), "раздел боковой панели не открылся");
                    return;
                }
                let plan = plan_category(&self.section, &key, &rep.sources, rep.search, &self.cfg.query);
                for p in plan.into_iter().rev() {
                    self.queue.push_front(p);
                }
            }
            Command::SetSource { key, .. } => {
                if !rep.ok {
                    self.note(self.place(), format!("магазин «{key}» не выбрался"));
                } else if !rep.source.is_empty() && rep.source != key {
                    self.note(self.place(), format!("просили магазин «{key}», а действует «{}»", rep.source));
                }
            }
            Command::TypeQuery { .. } | Command::ClearQuery { .. } => {
                if !rep.ok {
                    self.note(self.place(), "поле поиска не нашлось");
                }
            }
            Command::Capture { file, .. } => {
                if let Some(meta) = meta {
                    self.screens.push(Screen { file, meta, seen: rep });
                }
            }
            Command::Finish { .. } => {}
        }
    }

    fn place(&self) -> String {
        if self.category.is_empty() {
            format!("раздел {}", self.section)
        } else {
            format!("{}/{}", self.section, self.category)
        }
    }

    /// Всё подозрительное: заметки хода, разбор экранов и журнала.
    pub fn findings(&self, journal: &str) -> Vec<Finding> {
        let mut out = self.notes.clone();
        // Журнал до первого экрана — «запуск», дальше — отрезок до ответа экрана.
        let mut from = 0usize;
        for scr in &self.screens {
            let to = line_end(journal, scr.seen.log_offset as usize).max(from);
            let piece = journal.get(from..to).unwrap_or("");
            let place = if from == 0 { "запуск и " .to_string() + &scr.file } else { scr.file.clone() };
            journal_findings(&place, piece, &mut out);
            from = to;
            screen_findings(scr, &mut out);
        }
        if from < journal.len() {
            journal_findings("после последнего снимка", journal.get(from..).unwrap_or(""), &mut out);
        }
        layout_findings(&self.screens, &mut out);
        out
    }

    /// Текст `report.txt` и код возврата. `interrupted` — причина обрыва
    /// (общий тайм-аут).
    pub fn report(&self, journal: &str, interrupted: Option<&str>) -> (String, i32) {
        let mut findings = self.findings(journal);
        if let Some(why) = interrupted {
            let at = match &self.current {
                Some((cmd, _)) => format!("на команде {}", cmd_label(cmd)),
                None => "между командами".to_string(),
            };
            findings.insert(0, Finding { place: "обход".into(), text: format!("ПРЕРВАН: {why} ({at})") });
        }
        let code = if interrupted.is_some() {
            EXIT_TIMEOUT
        } else if findings.is_empty() {
            EXIT_CLEAN
        } else {
            EXIT_SUSPICIOUS
        };
        (format_report(&self.cfg, &self.screens, &findings), code)
    }
}

fn cmd_label(cmd: &Command) -> String {
    match cmd {
        Command::OpenSection { section, .. } => format!("«открыть раздел {section}»"),
        Command::SelectCategory { key, .. } => format!("«раздел панели {key}»"),
        Command::SetSource { key, .. } => format!("«магазин {key}»"),
        Command::TypeQuery { text, .. } => format!("«поиск {text}»"),
        Command::ClearQuery { .. } => "«очистить поиск»".to_string(),
        Command::Capture { file, .. } => format!("«снимок {file}»"),
        Command::Finish { .. } => "«конец»".to_string(),
    }
}

// ============================================================================
// Что считать подозрительным
// ============================================================================

fn secs(ms: u64) -> String {
    format!("{},{} с", ms / 1000, (ms % 1000) / 100)
}

fn screen_findings(scr: &Screen, out: &mut Vec<Finding>) {
    let s = &scr.seen;
    let mut add = |text: String| out.push(Finding { place: scr.file.clone(), text });
    if !s.saved {
        add("снимок не сохранился".into());
    }
    if s.load_timed_out {
        add(format!("выдача не загрузилась за {}", secs(LOAD_TIMEOUT_MS)));
    } else if s.load_ms > SLOW_LOAD_MS {
        add(format!("загрузка {} (дольше {})", secs(s.load_ms), secs(SLOW_LOAD_MS)));
    }
    if scr.meta.grid && s.screen == "grid" {
        if s.tiles == 0 && !s.load_timed_out {
            add("пустая выдача (0 плиток)".into());
        } else if s.no_preview > 0 {
            add(format!("плитки без превью: {} из {}", s.no_preview, s.tiles));
        }
    }
    if let Some(q) = &scr.meta.expect_query {
        if !s.search_visible {
            add(format!("поля поиска нет — слово «{q}» проверить негде"));
        } else if s.search_text.trim() != q {
            add(format!("текст поиска пропал: в поле «{}», ждали «{q}»", s.search_text));
        } else if s.model_query.trim() != q {
            add(format!("поле показывает «{q}», а страница ищет «{}»", s.model_query));
        }
    }
    if s.status_warn && !s.status.trim().is_empty() {
        add(format!("строка состояния: «{}»", s.status.trim()));
    } else if is_network_error(&s.status) {
        add(format!("ошибка сети в строке состояния: «{}»", s.status.trim()));
    }
}

/// Отличия шапки между магазинами одного раздела: есть ли поле поиска и где.
fn layout_findings(screens: &[Screen], out: &mut Vec<Finding>) {
    let mut by_cat: BTreeMap<(String, String), Vec<&Screen>> = BTreeMap::new();
    for s in screens.iter().filter(|s| !s.meta.category.is_empty() && s.seen.screen == "grid") {
        by_cat.entry((s.meta.section.clone(), s.meta.category.clone())).or_default().push(s);
    }
    for ((section, cat), list) in by_cat {
        let header = |s: &Screen| {
            if s.seen.search_visible {
                format!("поиск ({}, {})", s.seen.search_x, s.seen.search_y)
            } else {
                "без поиска".to_string()
            }
        };
        let mut variants: Vec<(String, Vec<String>)> = Vec::new();
        for s in &list {
            let h = header(s);
            let store = s.meta.store.clone();
            match variants.iter_mut().find(|(v, _)| *v == h) {
                Some((_, stores)) => {
                    if !stores.contains(&store) {
                        stores.push(store)
                    }
                }
                None => variants.push((h, vec![store])),
            }
        }
        if variants.len() > 1 {
            let desc: Vec<String> = variants.iter().map(|(h, st)| format!("{}: {h}", st.join(", "))).collect();
            out.push(Finding {
                place: format!("{section}/{cat}"),
                text: format!("шапка разная между магазинами — {}", desc.join("; ")),
            });
        }
    }
}

/// Отрезок журнала кончается на конце строки: строка, которую Qt дописывал
/// в момент замера, целиком уходит в этот отрезок, а не рвётся на два.
fn line_end(journal: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let offset = offset.min(journal.len());
    if journal.as_bytes()[offset - 1] == b'\n' {
        return offset;
    }
    journal[offset..].find('\n').map_or(journal.len(), |i| offset + i + 1)
}

/// Строка журнала — ошибка сети.
pub fn is_network_error(line: &str) -> bool {
    let l = line.to_lowercase();
    const MARKS: &[&str] = &[
        "urlopen error", "http error", "error sending request", "dns error", "failed to lookup",
        "connection refused", "connection reset", "timed out", "network is unreachable",
        "temporary failure in name resolution", "certificate", "ssl", "tls handshake",
        "ошибка сети", "нет сети", "нет соединения", "сервер не ответил", "http 4", "http 5",
    ];
    MARKS.iter().any(|m| l.contains(m))
}

/// Строка журнала — предупреждение QML (тип строки ставит [`qt_env`]).
pub fn is_qml_warning(line: &str) -> bool {
    let l = line.trim_start();
    let typed = l.starts_with("warning: ") || l.starts_with("critical: ") || l.starts_with("fatal: ");
    typed && (l.contains(".qml:") || l.contains(": qml:") || l.contains(": js:") || l.contains("qt.qml"))
}

/// Сколько строк одного рода показывать на место; остальное — счётом.
const LINES_PER_PLACE: usize = 5;

fn journal_findings(place: &str, piece: &str, out: &mut Vec<Finding>) {
    let mut net: Vec<&str> = Vec::new();
    let mut qml: Vec<&str> = Vec::new();
    for line in piece.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if is_qml_warning(line) {
            if !qml.contains(&line) {
                qml.push(line);
            }
        } else if is_network_error(line) && !net.contains(&line) {
            net.push(line);
        }
    }
    for (kind, list) in [("ошибка сети", net), ("предупреждение QML", qml)] {
        for line in list.iter().take(LINES_PER_PLACE) {
            out.push(Finding { place: place.to_string(), text: format!("{kind}: {line}") });
        }
        if list.len() > LINES_PER_PLACE {
            out.push(Finding {
                place: place.to_string(),
                text: format!("{kind}: и ещё {} разных строк (journal.log)", list.len() - LINES_PER_PLACE),
            });
        }
    }
}

// ============================================================================
// Отчёт
// ============================================================================

/// Текст `report.txt`: шапка, подозрительное, затем все экраны построчно.
pub fn format_report(cfg: &TourConfig, screens: &[Screen], findings: &[Finding]) -> String {
    let mut out = String::new();
    out.push_str("Обход главного окна Rexon (rexon-main-window --tour)\n");
    out.push_str(&format!(
        "Что обходили: {} | слово поиска: «{}» | снимков: {} | подозрительного: {}\n",
        cfg.target_label(),
        cfg.query,
        screens.len(),
        findings.len()
    ));
    out.push_str(&format!("Папка: {}\n\n", cfg.out_dir.display()));
    out.push_str("== Подозрительное ==\n");
    if findings.is_empty() {
        out.push_str("ничего\n");
    }
    for f in findings {
        out.push_str(&format!("{}: {}\n", f.place, f.text));
    }
    out.push_str("\n== Экраны ==\n");
    for s in screens {
        let r = &s.seen;
        let mut parts = vec![
            s.file.clone(),
            format!("вид {}", if r.screen.is_empty() { "—" } else { &r.screen }),
            if r.load_timed_out { "загрузка: НЕ дождались".to_string() } else { format!("загрузка {}", secs(r.load_ms)) },
        ];
        if s.meta.grid {
            parts.push(format!("плиток {}, без превью {}", r.tiles, r.no_preview));
            parts.push(if r.search_visible {
                format!("поиск ({}, {}) «{}»", r.search_x, r.search_y, r.search_text)
            } else {
                "поиска нет".to_string()
            });
        }
        if !r.status.trim().is_empty() {
            parts.push(format!("состояние «{}»", r.status.trim()));
        }
        out.push_str(&parts.join(" | "));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn cfg(section: Option<&str>, sub: Option<&str>) -> TourConfig {
        TourConfig {
            section: section.map(str::to_string),
            sub: sub.map(str::to_string),
            query: "girl".into(),
            out_dir: PathBuf::from("/tmp/t"),
            timeout_secs: 60,
        }
    }

    #[test]
    fn parses_flags() {
        assert_eq!(parse_args(&args(&["--section=steam"]), || PathBuf::from("x")), Ok(None));
        let c = parse_args(
            &args(&["--tour=appearance/wallpapers", "--tour-out", "/o", "--tour-query=cat", "--tour-timeout", "30"]),
            || unreachable!(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.section.as_deref(), Some("appearance"));
        assert_eq!(c.sub.as_deref(), Some("wallpapers"));
        assert_eq!(c.query, "cat");
        assert_eq!(c.out_dir, PathBuf::from("/o"));
        assert_eq!(c.timeout_secs, 30);
        let all = parse_args(&args(&["--tour"]), || PathBuf::from("/d")).unwrap().unwrap();
        assert_eq!((all.section, all.sub, all.query.as_str()), (None, None, DEFAULT_QUERY));
        assert_eq!(all.out_dir, PathBuf::from("/d"));
    }

    #[test]
    fn rejects_bad_flags() {
        let d = || PathBuf::from("/d");
        assert!(parse_args(&args(&["--tour=nope"]), d).is_err());
        assert!(parse_args(&args(&["--tour=steam/x"]), d).is_err());
        assert!(parse_args(&args(&["--tour", "--tour-out"]), d).is_err());
        assert!(parse_args(&args(&["--tour", "--tour-timeout", "abc"]), d).is_err());
        assert!(parse_args(&args(&["--tour", "--tour-query="]), d).is_err());
    }

    #[test]
    fn default_out_follows_xdg_cache() {
        let env = |k: &str| match k {
            "XDG_CACHE_HOME" => Some("/c".to_string()),
            _ => None,
        };
        assert_eq!(default_out_dir(&env), PathBuf::from("/c/rexon/tour"));
        let home = |k: &str| (k == "HOME").then(|| "/h".to_string());
        assert_eq!(default_out_dir(&home), PathBuf::from("/h/.cache/rexon/tour"));
    }

    #[test]
    fn forbidden_calls_are_refused() {
        for m in ALLOWED_CALLS {
            assert!(call_allowed(m), "{m}");
        }
        for m in ["install", "applyTheme", "tourApply", "uninstall", "removeItem", "openSectionAndInstall", "setSource"] {
            assert!(!call_allowed(m), "{m} обязан быть запрещён");
        }
    }

    #[test]
    fn every_planned_command_is_allowed() {
        let plan = plan_category("appearance", "wallpapers", &args(&["ocs", "wallhaven", "axon"]), true, "girl");
        for p in plan {
            if let Planned::Do(cmd) = p {
                let m = cmd.method().expect("у команды хода есть вызов");
                assert!(call_allowed(m), "{m}");
            }
        }
    }

    #[test]
    fn category_plan_browses_then_searches_each_store() {
        let plan = plan_category("appearance", "icons", &args(&["ocs", "aur", "both"]), true, "girl");
        let shots: Vec<(String, String, Option<String>)> = plan
            .iter()
            .filter_map(|p| match p {
                Planned::Shot(m) => Some((m.store.clone(), m.step.clone(), m.expect_query.clone())),
                _ => None,
            })
            .collect();
        let q = Some("girl".to_string());
        assert_eq!(
            shots,
            vec![
                ("ocs".into(), "browse".into(), None),
                ("aur".into(), "browse".into(), None),
                ("both".into(), "browse".into(), None),
                ("ocs".into(), "search".into(), q.clone()),
                ("aur".into(), "search".into(), q.clone()),
                ("both".into(), "search".into(), q),
            ]
        );
        // Слово вводится один раз — в первом магазине; дальше только смена.
        let typed = plan.iter().filter(|p| matches!(p, Planned::Do(Command::TypeQuery { .. }))).count();
        assert_eq!(typed, 1);
        assert!(plan.iter().any(|p| matches!(p, Planned::Do(Command::ClearQuery { .. }))));
    }

    #[test]
    fn category_without_stores_or_search() {
        let plan = plan_category("appearance", "active", &[], false, "girl");
        assert_eq!(plan.len(), 1);
        assert!(matches!(&plan[0], Planned::Shot(m) if m.store == "-" && m.step == "browse"));
    }

    #[test]
    fn file_names() {
        assert_eq!(shot_file_name(3, "wallpapers", "ocs", "browse"), "03-wallpapers-ocs-browse.png");
        assert_eq!(shot_file_name(12, "wallpaper_engine", "-", "browse"), "12-wallpaper_engine-main-browse.png");
        assert!(is_tour_artifact("03-wallpapers-ocs-browse.png"));
        assert!(is_tour_artifact(REPORT_FILE));
        assert!(!is_tour_artifact("photo.png"));
    }

    #[test]
    fn wait_rules() {
        let load = |elapsed, busy| WaitSample { phase: "load".into(), elapsed_ms: elapsed, timeout_ms: 1000, busy, ..Default::default() };
        assert_eq!(wait_verdict(&load(100, false)), WaitVerdict::Wait, "до паузы набора — рано");
        assert_eq!(wait_verdict(&load(500, false)), WaitVerdict::Ready);
        assert_eq!(wait_verdict(&load(500, true)), WaitVerdict::Wait);
        assert_eq!(wait_verdict(&load(1000, true)), WaitVerdict::Timeout);
        let mut busy_status = load(500, false);
        busy_status.status = "⏳ что-то".into();
        assert_eq!(wait_verdict(&busy_status), WaitVerdict::Wait);
        let pv = |no, pending, stable| WaitSample {
            phase: "preview".into(), elapsed_ms: 100, timeout_ms: 1000, no_preview: no,
            previews_pending: pending, stable_ms: stable, settle_ms: 500, ..Default::default()
        };
        assert_eq!(wait_verdict(&pv(0, 3, 0)), WaitVerdict::Ready);
        assert_eq!(wait_verdict(&pv(2, 3, 900)), WaitVerdict::Wait);
        assert_eq!(wait_verdict(&pv(2, 0, 600)), WaitVerdict::Ready);
    }

    fn capture_report(tiles: i64, no_preview: i64, search_x: i64, text: &str, offset: u64) -> Report {
        Report {
            ok: true,
            saved: true,
            screen: "grid".into(),
            load_ms: 800,
            tiles,
            no_preview,
            search_visible: true,
            search_x,
            search_y: 10,
            search_text: text.into(),
            model_query: text.into(),
            log_offset: offset,
            ..Default::default()
        }
    }

    /// Обход одного подраздела от начала до конца, с подставленным
    /// исполнителем: порядок команд, имена снимков и строки отчёта.
    #[test]
    fn walk_one_category_and_report() {
        let mut t = Tour::new(cfg(Some("appearance"), Some("wallpapers")));
        let journal = "debug: qml: REXON-TOUR\nwarning: qrc:/x/AppearancePage.qml:10:5: TypeError: oops\n\
                       info: rexon: <urlopen error timed out>\n";
        let first = t.next(None, journal);
        assert!(matches!(&first, Command::OpenSection { section, .. } if section == "appearance"));
        let open = Report {
            ok: true, built: true, kind: "appearance".into(),
            categories: args(&["active", "wallpapers"]), ..Default::default()
        };
        let cmd = t.next(Some(open), journal);
        assert!(matches!(&cmd, Command::SelectCategory { key, .. } if key == "wallpapers"));
        let sel = Report { ok: true, sources: args(&["ocs", "wallhaven"]), search: true, ..Default::default() };
        let mut cmd = t.next(Some(sel), journal);
        let mut files = Vec::new();
        let mut n = 0;
        loop {
            let rep = match &cmd {
                Command::Finish { .. } => break,
                Command::Capture { file, .. } => {
                    files.push(file.clone());
                    n += 1;
                    match n {
                        1 => capture_report(24, 3, 700, "", 40),
                        2 => capture_report(24, 0, 700, "", journal.len() as u64),
                        3 => capture_report(10, 0, 700, "girl", journal.len() as u64),
                        // Во втором магазине слово пропало, а поле уехало.
                        _ => capture_report(10, 0, 650, "", journal.len() as u64),
                    }
                }
                Command::SetSource { key, .. } => Report { ok: true, source: key.clone(), ..Default::default() },
                _ => Report { ok: true, ..Default::default() },
            };
            cmd = t.next(Some(rep), journal);
        }
        assert_eq!(
            files,
            vec![
                "01-wallpapers-ocs-browse.png",
                "02-wallpapers-wallhaven-browse.png",
                "03-wallpapers-ocs-search.png",
                "04-wallpapers-wallhaven-search.png",
            ]
        );
        let Command::Finish { exit_code, report } = cmd else { unreachable!() };
        assert_eq!(exit_code, EXIT_SUSPICIOUS);
        assert!(report.contains("01-wallpapers-ocs-browse.png: плитки без превью: 3 из 24"), "{report}");
        assert!(report.contains("04-wallpapers-wallhaven-search.png: текст поиска пропал"), "{report}");
        assert!(report.contains("шапка разная между магазинами"), "{report}");
        assert!(report.contains("предупреждение QML: warning: qrc:/x/AppearancePage.qml"), "{report}");
        assert!(report.contains("ошибка сети: info: rexon: <urlopen error timed out>"), "{report}");
        assert!(!report.contains("REXON-TOUR"), "console.log — не предупреждение: {report}");
    }

    #[test]
    fn missing_sub_and_unbuilt_page_are_reported() {
        let mut t = Tour::new(cfg(Some("appearance"), Some("nope")));
        t.next(None, "");
        let open = Report { ok: true, built: true, kind: "appearance".into(), categories: args(&["icons"]), ..Default::default() };
        let Command::Finish { report, exit_code } = t.next(Some(open), "") else { panic!("обход обязан кончиться") };
        assert_eq!(exit_code, EXIT_SUSPICIOUS);
        assert!(report.contains("подраздела «nope» нет"), "{report}");

        let mut t = Tour::new(cfg(Some("steam"), None));
        t.next(None, "");
        let Command::Finish { report, .. } = t.next(Some(Report::default()), "") else { panic!() };
        assert!(report.contains("страница раздела не построилась"), "{report}");
    }

    #[test]
    fn whole_window_visits_every_section_and_slow_empty_screens() {
        let mut t = Tour::new(cfg(None, None));
        let mut opened = Vec::new();
        let mut cmd = t.next(None, "");
        loop {
            let rep = match &cmd {
                Command::Finish { .. } => break,
                Command::OpenSection { section, .. } => {
                    opened.push(section.clone());
                    Report { built: true, ..Default::default() }
                }
                Command::Capture { .. } => Report { saved: true, load_ms: 7000, ..Default::default() },
                _ => Report { ok: true, ..Default::default() },
            };
            cmd = t.next(Some(rep), "");
        }
        assert_eq!(opened, crate::qt_main::SECTIONS.to_vec());
        let Command::Finish { report, .. } = cmd else { unreachable!() };
        assert!(report.contains("загрузка 7,0 с (дольше 5,0 с)"), "{report}");
        // У прочих разделов окна плиток нет — «пустой выдачи» там не бывает.
        assert!(!report.contains("пустая выдача"), "{report}");
    }

    #[test]
    fn empty_grid_and_interrupt() {
        let mut t = Tour::new(cfg(Some("appearance"), Some("icons")));
        t.next(None, "");
        t.next(Some(Report { built: true, kind: "appearance".into(), categories: args(&["icons"]), ..Default::default() }), "");
        let cmd = t.next(Some(Report { ok: true, ..Default::default() }), "");
        assert!(matches!(cmd, Command::Capture { .. }));
        let (report, code) = t.report("", Some("общий тайм-аут 60 с"));
        assert_eq!(code, EXIT_TIMEOUT);
        assert!(report.contains("ПРЕРВАН: общий тайм-аут 60 с (на команде «снимок 01-icons-main-browse.png»)"), "{report}");
        t.next(Some(Report { saved: true, screen: "grid".into(), ..Default::default() }), "");
        let (report, _) = t.report("", None);
        assert!(report.contains("пустая выдача"), "{report}");
    }

    #[test]
    fn commands_serialize_for_qml() {
        let j = Command::set_source("aur").to_json();
        assert_eq!(j, r#"{"cmd":"setSource","method":"tourSetSource","key":"aur"}"#);
        let r = Report::from_json(r#"{"ok":true,"sources":["ocs"],"search":true,"unknown":1}"#);
        assert!(r.ok && r.search && r.sources == vec!["ocs".to_string()]);
        assert!(!Report::from_json("не json").error.is_empty());
    }
}
