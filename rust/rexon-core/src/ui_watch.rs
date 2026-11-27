//! Сторож главного потока окна и журнал действий человека.
//!
//! ЗАЧЕМ. Живой случай 15.11.2026: окно Rexon зависло, Cinnamon показал
//! «приложение не отвечает — подождать/закрыть», владелец нажал «Закрыть»,
//! и процесс был убит (SIGKILL). В `rexon.log` об этом не нашлось НИЧЕГО:
//! последняя запись — за две минуты до, ни что делал человек, ни где стоял
//! главный поток. SIGKILL не перехватить, поэтому улика обязана лечь в
//! журнал ЗАРАНЕЕ — пока процесс ещё жив, но уже не отвечает:
//!
//!   * журнал действий: каждое нажатие кнопки, переключатель, выбор в
//!     списке, пункт меню и смена раздела — одна строка INFO; действие,
//!     которое держало главный поток дольше [`SLOW_ACTION`], — вторая
//!     строка с длительностью;
//!   * сторож: отдельный поток смотрит на пульс цикла событий (таймер Qt
//!     на главном потоке бьёт каждые [`HEARTBEAT_MS`] мс). Пульса нет
//!     дольше [`STALL_AFTER`] — WARN «окно не отвечает» с текущим
//!     действием и стеком главного потока; дольше [`SEVERE_AFTER`] — одна
//!     строка ERROR; пульс вернулся — INFO «окно снова отвечает, висело N с».
//!     На одно зависание — не больше трёх строк.
//!
//! Строки журнала пишутся сразу в файл (`applog` пишет без буфера, вызовом
//! `write`): данные уже в ядре системы и переживают SIGKILL процесса.
//!
//! Qt здесь нет: таймер пульса и обход предков кнопки живут в `rexon-gui`
//! (`action_journal.rs`), а решения, тексты и сторож — здесь, под тестами
//! без Qt.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::applog::{self, Level};

/// Область строк журнала.
pub const AREA: &str = "ui";

/// Как часто главный поток бьёт пульс.
pub const HEARTBEAT_MS: u64 = 250;
/// Как часто сторож смотрит на пульс.
pub const POLL: Duration = Duration::from_millis(250);
/// Пульса нет дольше — окно «не отвечает» (WARN).
pub const STALL_AFTER: Duration = Duration::from_secs(2);
/// Пульса нет дольше — одна строка ERROR.
pub const SEVERE_AFTER: Duration = Duration::from_secs(10);
/// Действие держало главный поток дольше — строка с длительностью.
pub const SLOW_ACTION: Duration = Duration::from_millis(500);
/// Самая длинная подпись в строке журнала, знаков.
const LABEL_MAX: usize = 80;

// ============================================================================
// Журнал действий
// ============================================================================

#[derive(Clone)]
struct Action {
    id: u64,
    what: String,
    started: Instant,
}

#[derive(Default)]
struct Activity {
    /// Открытый раздел окна (идентификатор `qt_main::SECTIONS`).
    section: String,
    /// Действие, обработчик которого ещё не вернул управление циклу событий.
    current: Option<Action>,
    /// Последнее начатое действие — даже если уже закончилось.
    last: Option<Action>,
    /// Последнее записанное значение каждого поля ввода (место + подсказка).
    entered: std::collections::HashMap<String, String>,
}

static ACTIVITY: Mutex<Option<Activity>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn with_activity<R>(f: impl FnOnce(&mut Activity) -> R) -> R {
    let mut guard = ACTIVITY.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Activity::default))
}

/// Подпись в строку журнала: одна строка, без лишних пробелов, не длиннее
/// [`LABEL_MAX`] знаков. Пустая — «(без подписи)»: кнопка-значок тоже
/// действие, и пустые кавычки читались бы как ошибка журнала.
pub fn clean_label(label: &str) -> String {
    let one: String = label.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.is_empty() {
        return "(без подписи)".to_string();
    }
    let cut: String = one.chars().take(LABEL_MAX).collect();
    if cut.chars().count() < one.chars().count() {
        format!("{cut}…")
    } else {
        cut
    }
}

/// Место кнопки в окне по цепочке её предков.
///
/// `chain` — имена классов предков от ближнего к дальнему через `/`, как
/// их отдаёт Qt (`ScreenEffectsSection_QMLTYPE_42/QQuickColumnLayout/…`).
/// Берутся только компоненты, объявленные НАШИМИ файлами QML (у них
/// приписка `_QMLTYPE_N` либо `_QML_N`), кроме общих контролов `Rexon*` и
/// корня окна; остаются два ближних, от внешнего к внутреннему. Впереди —
/// открытый раздел окна.
///
/// Пример: раздел `appearance`, цепочка
/// `RexonButton_QMLTYPE_3/QQuickRowLayout/ScreenEffectsSection_QMLTYPE_9/AppearancePage_QMLTYPE_2/MainWindow_QMLTYPE_0`
/// → `appearance ▸ AppearancePage ▸ ScreenEffectsSection`.
pub fn area_from_chain(section: &str, chain: &str) -> String {
    let mut names: Vec<&str> = Vec::new();
    for raw in chain.split('/') {
        let Some(pos) = raw.find("_QML") else { continue };
        let name = &raw[..pos];
        if name.is_empty() || name.starts_with("Rexon") || name == "MainWindow" {
            continue;
        }
        if names.last() != Some(&name) {
            names.push(name);
        }
        if names.len() == 2 {
            break;
        }
    }
    names.reverse();
    let mut parts: Vec<&str> = Vec::new();
    if !section.trim().is_empty() {
        parts.push(section.trim());
    }
    parts.extend(names);
    if parts.is_empty() {
        "окно".to_string()
    } else {
        parts.join(" ▸ ")
    }
}

/// Что сделал человек — тело строки журнала.
#[derive(Debug, Clone, PartialEq)]
pub enum Deed<'a> {
    /// Нажал кнопку с подписью.
    Click { label: &'a str },
    /// Переключил флажок с подписью.
    Toggle { label: &'a str, on: bool },
    /// Выбрал значение в выпадающем списке.
    Choose { value: &'a str },
    /// Закончил ввод в поле с подсказкой `field`: `value` — введённое
    /// (поле пароля сюда не попадает вовсе, см. [`note_entered`]).
    Entered { field: &'a str, value: &'a str },
}

/// Строка журнала о действии: `действие: <место> → «<подпись>»…`.
pub fn deed_line(area: &str, deed: &Deed) -> String {
    match deed {
        Deed::Click { label } => format!("действие: {area} → «{}»", clean_label(label)),
        Deed::Toggle { label, on } => format!(
            "действие: {area} → «{}»: {}",
            clean_label(label),
            if *on { "включено" } else { "выключено" }
        ),
        Deed::Choose { value } => format!("действие: {area} → выбрано «{}»", clean_label(value)),
        Deed::Entered { field, value } => {
            if value.trim().is_empty() {
                format!("действие: {area} → поле «{}» очищено", clean_label(field))
            } else {
                format!("действие: {area} → поле «{}»: «{}»", clean_label(field), clean_label(value))
            }
        }
    }
}

/// Строка о долгом действии: `действие заняло 1234 мс: <что>`.
pub fn slow_line(what: &str, took: Duration) -> String {
    format!("действие заняло {} мс: {what}", took.as_millis())
}

/// Строка о смене раздела окна.
pub fn section_line(section: &str) -> String {
    format!("раздел: {}", clean_label(section))
}

/// Открыт раздел `section` — строка журнала и место для следующих строк.
/// Повтор того же раздела не пишется.
pub fn note_section(section: &str) {
    let changed = with_activity(|a| {
        if a.section == section {
            return false;
        }
        a.section = section.to_string();
        true
    });
    if changed {
        applog::write(Level::Info, AREA, &section_line(section));
    }
}

/// Открытый раздел окна (пусто — ещё не открывался).
pub fn current_section() -> String {
    with_activity(|a| a.section.clone())
}

/// Человек сделал `deed` в месте `area`: строка журнала, и действие
/// становится «текущим», пока его обработчик не вернёт управление
/// ([`finish_deed`]). Ответ — номер для [`finish_deed`].
pub fn begin_deed(area: &str, deed: &Deed) -> u64 {
    let what = deed_line(area, deed);
    applog::write(Level::Info, AREA, &what);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let action = Action { id, what, started: Instant::now() };
    with_activity(|a| {
        a.current = Some(action.clone());
        a.last = Some(action);
    });
    id
}

/// Обработчик действия `id` вернул управление циклу событий. Держал он
/// главный поток дольше [`SLOW_ACTION`] — строка с длительностью.
pub fn finish_deed(id: u64) {
    let done = with_activity(|a| match &a.current {
        Some(c) if c.id == id => a.current.take(),
        _ => None,
    });
    if let Some(action) = done {
        let took = action.started.elapsed();
        if took >= SLOW_ACTION {
            let what = action.what.strip_prefix("действие: ").unwrap_or(&action.what);
            applog::write(Level::Info, AREA, &slow_line(what, took));
        }
    }
}

/// Что сейчас делает окно — для строки сторожа.
pub fn activity_report() -> String {
    with_activity(|a| {
        let now = Instant::now();
        let section = if a.section.is_empty() { "—".to_string() } else { a.section.clone() };
        match (&a.current, &a.last) {
            (Some(c), _) => format!(
                "выполняется «{}» (начато {:.1} с назад); раздел {section}",
                c.what,
                now.duration_since(c.started).as_secs_f64()
            ),
            (None, Some(l)) => format!(
                "текущего действия нет; последнее — «{}» ({:.1} с назад); раздел {section}",
                l.what,
                now.duration_since(l.started).as_secs_f64()
            ),
            (None, None) => format!("действий ещё не было; раздел {section}"),
        }
    })
}

// ============================================================================
// Ввод с мыши и клавиатуры: что за действие
// ============================================================================
//
// Нажатия ловит ОДИН фильтр событий приложения (`rexon-gui`,
// `action_journal.rs` + шим `rexon_install_input_journal`): кнопки,
// флажки, пункты меню и выпадающих списков, карточки магазинов с
// `TapHandler`/`MouseArea` — всё, что взяло нажатие мыши (или Пробел/Enter
// на кнопке в фокусе). Шим ничего не решает: он отдаёт цепочку «кто взял
// нажатие → его предки» строками, а что из этого действие и как его
// назвать — решается здесь.

/// Одно звено цепочки от фильтра ввода.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InputItem {
    /// Имя класса Qt (`RexonButton_QMLTYPE_3`, `QQuickTapHandler`).
    pub class: String,
    /// Признаки (буквы), см. [`parse_input_chain`].
    pub flags: String,
    /// `objectName`.
    pub name: String,
    /// Своя подпись (`text`, у списка — `displayText`, у окна — `title`).
    /// У полей ввода шим её НЕ берёт: там может быть пароль.
    pub text: String,
    /// Первая надпись (`Text`) внутри — для карточек и кнопок-значков.
    pub inner: String,
}

impl InputItem {
    fn has(&self, flag: char) -> bool {
        self.flags.contains(flag)
    }
}

/// Разбор описания от шима: звено на строку, от взявшего нажатие к дальним
/// предкам; поля через табуляцию — класс, признаки, `objectName`, подпись,
/// надпись внутри. Признаки: `b` — кнопка (`AbstractButton`, в том числе
/// пункт меню и строка списка), `k` — переключаемая, `K` — сейчас включена,
/// `r` — радиокнопка, `x` — выпадающий список, `p` — всплывающее окно,
/// `i` — поле ввода, `h` — обработчик указателя (`TapHandler`), `m` —
/// `MouseArea`, `f` — прокручиваемая область, `s` — ползунок/полоса
/// прокрутки, `t` — вкладка `TabButton`.
pub fn parse_input_chain(desc: &str) -> Vec<InputItem> {
    desc.lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut f = line.split('\t');
            let mut next = || f.next().unwrap_or("").to_string();
            InputItem { class: next(), flags: next(), name: next(), text: next(), inner: next() }
        })
        .collect()
}

/// Действие по нажатию — с владеющими строками (строка журнала — через [`Deed`]).
#[derive(Debug, Clone, PartialEq)]
pub enum InputDeed {
    Click(String),
    Toggle(String, bool),
    Choose(String),
}

/// Что сделал человек этим нажатием; `None` — не действие (прокрутка,
/// щелчок в поле ввода, открытие выпадающего списка — выбор в нём будет
/// отдельной строкой, вкладка верхней полосы — её пишет строка «раздел»).
/// `via_key` — Пробел/Enter на элементе в фокусе: действие только у кнопок.
/// Ответ — место (см. [`area_from_chain`]) и действие.
pub fn deed_from_input(section: &str, items: &[InputItem], via_key: bool) -> Option<(String, InputDeed)> {
    let first = items.first()?;
    if first.has('i') || first.has('f') || first.has('s') || first.has('x') {
        return None;
    }
    if via_key && !first.has('b') {
        return None;
    }
    if !(first.has('b') || first.has('m') || first.has('h')) {
        return None;
    }
    let chain: Vec<&str> = items.iter().map(|i| i.class.as_str()).collect();
    let chain = chain.join("/");
    if first.has('t') && area_from_chain("", &chain) == "окно" {
        // Вкладка полосы разделов: смену раздела пишет `note_section`.
        return None;
    }
    let area = area_from_chain(section, &chain);
    let label = std::iter::once(first.text.as_str())
        .chain(items.iter().take(3).map(|i| i.inner.as_str()))
        .chain(std::iter::once(first.name.as_str()))
        .find(|t| !t.trim().is_empty())
        .unwrap_or("")
        .to_string();
    // Строка всплывающего списка `ComboBox`: выше неё — окно-всплывашка,
    // ещё выше — сам список.
    if first.has('b') {
        if let Some(popup) = items.iter().skip(1).position(|i| i.has('p')) {
            if items.iter().skip(2 + popup).any(|i| i.has('x')) {
                return Some((area, InputDeed::Choose(label)));
            }
        }
    }
    if first.has('k') {
        let on = first.has('r') || !first.has('K');
        return Some((area, InputDeed::Toggle(label, on)));
    }
    Some((area, InputDeed::Click(label)))
}

/// Нажатие от фильтра ввода: строка журнала и «текущее действие», если это
/// действие. → номер для [`finish_deed`], 0 — не действие.
pub fn begin_input(desc: &str, via_key: bool) -> u64 {
    let items = parse_input_chain(desc);
    let Some((area, deed)) = deed_from_input(&current_section(), &items, via_key) else {
        return 0;
    };
    match &deed {
        InputDeed::Click(label) => begin_deed(&area, &Deed::Click { label }),
        InputDeed::Toggle(label, on) => begin_deed(&area, &Deed::Toggle { label, on: *on }),
        InputDeed::Choose(value) => begin_deed(&area, &Deed::Choose { value }),
    }
}

// ============================================================================
// Поля ввода
// ============================================================================
//
// Поле ввода — общий контрол `RexonTextField`: каждое нажатие клавиши
// журналу не нужно (шум), а последнее введённое — нужно (поиск зависнет —
// видно, что искали). Поэтому набор только отмечается как «последнее
// действие» без строки, а строка пишется, когда ввод закончен (Enter или
// уход фокуса) и значение отличается от прежде записанного.
//
// ⛔ Поле пароля (`echoMode` не `Normal`) не пишется НИКАК — ни строки, ни
// отметки с текстом: QML его текст сюда и не передаёт, а ядро на всякий
// случай ещё и не смотрит на него.

/// Человек набирает текст в поле `field` (подсказка поля) места `area`:
/// отметка для сторожа, без строки журнала.
pub fn note_typing(area: &str, field: &str, value: &str, secret: bool) {
    let what = if secret {
        format!("ввод в поле «{}» ({area}), текст скрыт", clean_label(field))
    } else {
        format!("ввод в поле «{}» ({area}): «{}»", clean_label(field), clean_label(value))
    };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    with_activity(|a| a.last = Some(Action { id, what, started: Instant::now() }));
}

/// Ввод в поле закончен. Строка журнала — если значение отличается от
/// записанного прежде для этого поля (и не пустое при первом разе). Пароль
/// не пишется. → номер для [`finish_deed`], 0 — строки не было.
pub fn note_entered(area: &str, field: &str, value: &str, secret: bool) -> u64 {
    if secret {
        return 0;
    }
    let key = format!("{area}\u{1f}{field}");
    let value_one = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let fresh = with_activity(|a| {
        let prev = a.entered.get(&key);
        let changed = match prev {
            Some(p) => *p != value_one,
            None => !value_one.is_empty(),
        };
        if changed {
            a.entered.insert(key.clone(), value_one.clone());
        }
        changed
    });
    if !fresh {
        return 0;
    }
    begin_deed(area, &Deed::Entered { field, value: &value_one })
}

// ============================================================================
// Сторож: решение
// ============================================================================

/// Пороги сторожа.
#[derive(Debug, Clone, Copy)]
pub struct WatchConfig {
    pub stall_after: Duration,
    pub severe_after: Duration,
    pub poll: Duration,
}

impl Default for WatchConfig {
    fn default() -> Self {
        WatchConfig { stall_after: STALL_AFTER, severe_after: SEVERE_AFTER, poll: POLL }
    }
}

/// Что сторож увидел на очередном взгляде.
#[derive(Debug, Clone, PartialEq)]
pub enum WatchEvent {
    /// Пульса нет `silent` — окно не отвечает.
    Stalled { silent: Duration },
    /// Пульса нет дольше тяжёлого порога.
    Severe { silent: Duration },
    /// Пульс вернулся; окно висело `hung`.
    Recovered { hung: Duration },
}

/// Состояние сторожа между взглядами.
#[derive(Debug, Default, Clone)]
pub struct WatchState {
    /// Отметка последнего пульса, при которой окно повисло.
    stalled_at: Option<u64>,
    severe_said: bool,
}

/// Один взгляд сторожа. `now_ms` и `beat_ms` — миллисекунды от общей
/// точки отсчёта; `beat_ms == 0` — пульса ещё не было (цикл событий не
/// запущен), и судить не о чем.
pub fn watch_step(state: &mut WatchState, now_ms: u64, beat_ms: u64, cfg: &WatchConfig) -> Option<WatchEvent> {
    if beat_ms == 0 {
        return None;
    }
    let silent = Duration::from_millis(now_ms.saturating_sub(beat_ms));
    match state.stalled_at {
        None => {
            if silent > cfg.stall_after {
                state.stalled_at = Some(beat_ms);
                state.severe_said = false;
                return Some(WatchEvent::Stalled { silent });
            }
            None
        }
        Some(at) if beat_ms != at => {
            // Новый пульс: зависание кончилось. Висело — от прежнего пульса
            // до нового (сторож смотрит раз в `poll`, его опоздание не в счёт).
            state.stalled_at = None;
            state.severe_said = false;
            Some(WatchEvent::Recovered { hung: Duration::from_millis(beat_ms.saturating_sub(at)) })
        }
        Some(_) => {
            if !state.severe_said && silent > cfg.severe_after {
                state.severe_said = true;
                return Some(WatchEvent::Severe { silent });
            }
            None
        }
    }
}

/// Строка WARN о зависании.
pub fn stall_line(silent: Duration, activity: &str, stack: Option<&str>) -> String {
    let mut s = format!("окно не отвечает {:.1} с; {activity}", silent.as_secs_f64());
    match stack {
        Some(st) if !st.trim().is_empty() => {
            s.push_str("\nстек главного потока:\n");
            s.push_str(st.trim_end());
        }
        _ => s.push_str("\nстек главного потока снять не удалось"),
    }
    s
}

/// Строка ERROR о долгом зависании.
pub fn severe_line(silent: Duration, activity: &str) -> String {
    format!(
        "окно не отвечает уже {:.0} с (дольше {} с); {activity}",
        silent.as_secs_f64(),
        SEVERE_AFTER.as_secs()
    )
}

/// Строка INFO о возвращении.
pub fn recovered_line(hung: Duration) -> String {
    format!("окно снова отвечает, висело {:.1} с", hung.as_secs_f64())
}

// ============================================================================
// Сторож: поток
// ============================================================================

/// Куда сторож пишет строки (в продукте — `applog::write`).
pub type Sink = Arc<dyn Fn(Level, &str) + Send + Sync>;
/// Снять стек наблюдаемого потока (в продукте — [`stack::capture`]).
pub type StackFn = Arc<dyn Fn() -> Option<String> + Send + Sync>;
/// Что сейчас делается (в продукте — [`activity_report`]).
pub type ActivityFn = Arc<dyn Fn() -> String + Send + Sync>;

/// Пульс одного потока и сторож над ним.
pub struct Watchdog {
    origin: Instant,
    beat: AtomicU64,
    stop: AtomicBool,
}

impl Watchdog {
    pub fn new() -> Arc<Watchdog> {
        Arc::new(Watchdog { origin: Instant::now(), beat: AtomicU64::new(0), stop: AtomicBool::new(false) })
    }

    fn now_ms(&self) -> u64 {
        // +1: ноль занят под «пульса ещё не было».
        self.origin.elapsed().as_millis() as u64 + 1
    }

    /// Удар пульса — с наблюдаемого потока.
    pub fn beat(&self) {
        self.beat.store(self.now_ms(), Ordering::Relaxed);
    }

    /// Остановить поток сторожа (стенды; в продукте он живёт до выхода).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Завести поток сторожа.
    pub fn spawn(
        self: &Arc<Self>,
        cfg: WatchConfig,
        sink: Sink,
        activity: ActivityFn,
        stack: StackFn,
    ) -> Option<std::thread::JoinHandle<()>> {
        let me = Arc::clone(self);
        std::thread::Builder::new()
            .name("rexon-ui-watch".into())
            .spawn(move || {
                let mut state = WatchState::default();
                while !me.stop.load(Ordering::Relaxed) {
                    std::thread::sleep(cfg.poll);
                    let beat = me.beat.load(Ordering::Relaxed);
                    match watch_step(&mut state, me.now_ms(), beat, &cfg) {
                        Some(WatchEvent::Stalled { silent }) => {
                            let what = activity();
                            let st = stack();
                            sink(Level::Warn, &stall_line(silent, &what, st.as_deref()));
                        }
                        Some(WatchEvent::Severe { silent }) => sink(Level::Error, &severe_line(silent, &activity())),
                        Some(WatchEvent::Recovered { hung }) => sink(Level::Info, &recovered_line(hung)),
                        None => {}
                    }
                }
            })
            .ok()
    }
}

static MAIN: OnceLock<Arc<Watchdog>> = OnceLock::new();

/// Удар пульса главного потока (таймер Qt в `rexon-gui`).
pub extern "C" fn main_heartbeat() {
    if let Some(w) = MAIN.get() {
        w.beat();
    }
}

/// Завести сторож главного потока. Звать С ГЛАВНОГО потока (его стек и
/// снимается), до цикла событий; повторный вызов ничего не делает.
pub fn install_main_watchdog() {
    if MAIN.get().is_some() {
        return;
    }
    let dog = Watchdog::new();
    if MAIN.set(Arc::clone(&dog)).is_err() {
        return;
    }
    let stack_ok = stack::watch_current_thread();
    let sink: Sink = Arc::new(|level, msg| applog::write(level, AREA, msg));
    let stack_fn: StackFn = Arc::new(move || if stack_ok { stack::capture(Duration::from_millis(300)) } else { None });
    if dog.spawn(WatchConfig::default(), sink, Arc::new(activity_report), stack_fn).is_none() {
        applog::write(Level::Warn, AREA, "сторож окна не запустился: поток не создан");
    }
}

// ============================================================================
// Стек главного потока
// ============================================================================

/// Стек наблюдаемого потока: сторож шлёт ему сигнал, обработчик (на самом
/// потоке) собирает адреса возврата `backtrace()` glibc в общий буфер, а
/// имена по адресам ищет уже сторож (`dladdr`) — в обработчике сигнала
/// допустим только безопасный минимум.
///
/// Имена есть у функций библиотек (Qt, glibc) — по ним и видно, где стоит
/// поток («QProcess::waitForFinished», «QQmlComponent::create»). Свои
/// функции Rexon в динамической таблице не значатся: для них — имя файла и
/// смещение, его разворачивает `addr2line -e <бинарь> <смещение>`.
pub mod stack {
    use std::os::raw::{c_int, c_void};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    const MAX_FRAMES: usize = 48;
    /// Сколько кадров показывать в журнале.
    const SHOW_FRAMES: usize = 28;

    extern "C" {
        fn backtrace(buffer: *mut *mut c_void, size: c_int) -> c_int;
    }

    static TARGET: AtomicUsize = AtomicUsize::new(0);
    static READY: AtomicBool = AtomicBool::new(false);
    static FRAMES: [AtomicUsize; MAX_FRAMES] = [const { AtomicUsize::new(0) }; MAX_FRAMES];
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    static REQUEST: AtomicU32 = AtomicU32::new(0);
    static DONE: AtomicU32 = AtomicU32::new(0);

    /// Сигнал снятия стека — из пользовательских реального времени: у
    /// Qt и glibc своих на них нет, а SIGPROF/SIGUSR* бывают заняты.
    fn stack_signal() -> c_int {
        libc::SIGRTMIN() + 6
    }

    extern "C" fn on_stack_signal(_sig: c_int) {
        // errno прерванного кода не портим.
        let saved = unsafe { *libc::__errno_location() };
        let mut buf = [std::ptr::null_mut::<c_void>(); MAX_FRAMES];
        let n = unsafe { backtrace(buf.as_mut_ptr(), MAX_FRAMES as c_int) }.max(0) as usize;
        for (slot, addr) in FRAMES.iter().zip(buf.iter().take(n)) {
            slot.store(*addr as usize, Ordering::Relaxed);
        }
        COUNT.store(n, Ordering::Relaxed);
        DONE.store(REQUEST.load(Ordering::Acquire), Ordering::Release);
        unsafe { *libc::__errno_location() = saved };
    }

    /// Наблюдать текущий поток. `false` — обработчик не встал.
    pub fn watch_current_thread() -> bool {
        // Первый вызов `backtrace` подгружает libgcc_s (`dlopen`) — это
        // недопустимо в обработчике сигнала, поэтому прогрев здесь.
        let mut warm = [std::ptr::null_mut::<c_void>(); 4];
        unsafe { backtrace(warm.as_mut_ptr(), warm.len() as c_int) };
        unsafe {
            let mut act: libc::sigaction = std::mem::zeroed();
            act.sa_sigaction = on_stack_signal as extern "C" fn(c_int) as usize;
            // SA_RESTART: прерванный вызов (read, waitpid) продолжится сам.
            // БЕЗ SA_ONSTACK: запасной стек сигналов у Rust мал для разбора.
            act.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut act.sa_mask);
            if libc::sigaction(stack_signal(), &act, std::ptr::null_mut()) != 0 {
                return false;
            }
            TARGET.store(libc::pthread_self() as usize, Ordering::SeqCst);
        }
        READY.store(true, Ordering::SeqCst);
        true
    }

    /// Снять стек наблюдаемого потока; `None` — поток не принял сигнал за
    /// `timeout` (например, стоит в непрерываемом ожидании диска).
    pub fn capture(timeout: Duration) -> Option<String> {
        capture_frames(timeout).map(|f| describe(&f))
    }

    /// Сырые адреса кадров.
    pub fn capture_frames(timeout: Duration) -> Option<Vec<usize>> {
        if !READY.load(Ordering::SeqCst) {
            return None;
        }
        let n = REQUEST.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        let target = TARGET.load(Ordering::SeqCst) as libc::pthread_t;
        if unsafe { libc::pthread_kill(target, stack_signal()) } != 0 {
            return None;
        }
        let deadline = Instant::now() + timeout;
        while DONE.load(Ordering::Acquire) != n {
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let count = COUNT.load(Ordering::Relaxed).min(MAX_FRAMES);
        Some(FRAMES.iter().take(count).map(|a| a.load(Ordering::Relaxed)).collect())
    }

    type Demangle =
        unsafe extern "C" fn(*const libc::c_char, *mut libc::c_char, *mut usize, *mut c_int) -> *mut libc::c_char;

    /// `__cxa_demangle` из libstdc++, если он есть в процессе (в окне Qt —
    /// есть всегда; в тестах ядра — может не быть, тогда имя как есть).
    fn demangle(name: &str) -> String {
        static SYM: &[u8] = b"__cxa_demangle\0";
        let f = unsafe { libc::dlsym(libc::RTLD_DEFAULT, SYM.as_ptr().cast()) };
        if f.is_null() || !name.starts_with("_Z") {
            return name.to_string();
        }
        let Ok(c) = std::ffi::CString::new(name) else { return name.to_string() };
        let f: Demangle = unsafe { std::mem::transmute(f) };
        let mut status: c_int = -1;
        let out = unsafe { f(c.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), &mut status) };
        if out.is_null() || status != 0 {
            return name.to_string();
        }
        let s = unsafe { std::ffi::CStr::from_ptr(out) }.to_string_lossy().into_owned();
        unsafe { libc::free(out.cast()) };
        s
    }

    fn frame_text(addr: usize) -> (String, Option<String>) {
        let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
        if unsafe { libc::dladdr(addr as *const c_void, &mut info) } == 0 {
            return (format!("0x{addr:x}"), None);
        }
        let file = if info.dli_fname.is_null() {
            "?".to_string()
        } else {
            let full = unsafe { std::ffi::CStr::from_ptr(info.dli_fname) }.to_string_lossy().into_owned();
            full.rsplit('/').next().unwrap_or(&full).to_string()
        };
        if !info.dli_sname.is_null() {
            let raw = unsafe { std::ffi::CStr::from_ptr(info.dli_sname) }.to_string_lossy().into_owned();
            let off = addr.saturating_sub(info.dli_saddr as usize);
            let mut name = demangle(&raw);
            if name.chars().count() > 160 {
                name = name.chars().take(160).collect::<String>() + "…";
            }
            return (format!("{file}: {name}+0x{off:x}"), Some(raw));
        }
        let off = addr.saturating_sub(info.dli_fbase as usize);
        (format!("{file}+0x{off:x}"), None)
    }

    /// Кадры текстом, по одному на строку, без кадров самого обработчика.
    pub fn describe(frames: &[usize]) -> String {
        let texts: Vec<(String, Option<String>)> = frames.iter().map(|a| frame_text(*a)).collect();
        // Кадры до трамплина сигнала (`__restore_rt`) — это обработчик.
        let start = texts
            .iter()
            .position(|(_, raw)| raw.as_deref() == Some("__restore_rt"))
            .map(|i| i + 1)
            .unwrap_or(1.min(texts.len()));
        let shown: Vec<String> = texts[start..]
            .iter()
            .take(SHOW_FRAMES)
            .enumerate()
            .map(|(i, (t, _))| format!("#{i} {t}"))
            .collect();
        let mut out = shown.join("\n");
        if texts.len() - start > SHOW_FRAMES {
            out.push_str(&format!("\n… ещё {} кадров", texts.len() - start - SHOW_FRAMES));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Стенды, которые читают общее «текущее действие», идут по одному.
    static SHARED_ACTIVITY: Mutex<()> = Mutex::new(());

    #[test]
    fn подпись_одной_строкой_и_с_пределом() {
        assert_eq!(clean_label("  ✓  Применить\n«Сепия» "), "✓ Применить «Сепия»");
        assert_eq!(clean_label(""), "(без подписи)");
        let long = "я".repeat(100);
        let c = clean_label(&long);
        assert_eq!(c.chars().count(), LABEL_MAX + 1);
        assert!(c.ends_with('…'));
    }

    #[test]
    fn место_по_цепочке_предков() {
        let chain = "RexonButton_QMLTYPE_3/QQuickRowLayout/ScreenEffectsSection_QMLTYPE_9/QQuickColumnLayout/AppearancePage_QMLTYPE_2/QQuickLoader/MainWindow_QMLTYPE_0";
        assert_eq!(area_from_chain("appearance", chain), "appearance ▸ AppearancePage ▸ ScreenEffectsSection");
        // Только два ближних компонента.
        let deep = "RexonButton_QMLTYPE_1/ConkySets_QMLTYPE_7/ConkySettings_QMLTYPE_5/AppearancePage_QMLTYPE_2";
        assert_eq!(area_from_chain("appearance", deep), "appearance ▸ ConkySettings ▸ ConkySets");
        // Встроенный компонент (`_QML_N`) и повтор имени.
        assert_eq!(area_from_chain("", "Card_QML_12/Card_QML_12/QQuickItem"), "Card");
        assert_eq!(area_from_chain("", "QQuickItem/QQuickPopupItem"), "окно");
        assert_eq!(area_from_chain("steam", ""), "steam");
    }

    #[test]
    fn строки_действий() {
        let area = "appearance ▸ ScreenEffectsSection";
        assert_eq!(
            deed_line(area, &Deed::Click { label: "Применить" }),
            "действие: appearance ▸ ScreenEffectsSection → «Применить»"
        );
        assert_eq!(
            deed_line(area, &Deed::Toggle { label: "Включить", on: true }),
            "действие: appearance ▸ ScreenEffectsSection → «Включить»: включено"
        );
        assert_eq!(
            deed_line(area, &Deed::Toggle { label: "Включить", on: false }),
            "действие: appearance ▸ ScreenEffectsSection → «Включить»: выключено"
        );
        assert_eq!(
            deed_line(area, &Deed::Choose { value: "Сепия" }),
            "действие: appearance ▸ ScreenEffectsSection → выбрано «Сепия»"
        );
        assert_eq!(
            deed_line("extensions", &Deed::Entered { field: "Поиск", value: "dash to dock" }),
            "действие: extensions → поле «Поиск»: «dash to dock»"
        );
        assert_eq!(
            deed_line("extensions", &Deed::Entered { field: "Поиск", value: "" }),
            "действие: extensions → поле «Поиск» очищено"
        );
        assert_eq!(section_line("steam"), "раздел: steam");
        assert_eq!(
            slow_line("appearance → «Применить»", Duration::from_millis(1234)),
            "действие заняло 1234 мс: appearance → «Применить»"
        );
    }

    #[test]
    fn текущее_действие_видно_сторожу_до_конца_обработчика() {
        let _serial = SHARED_ACTIVITY.lock().unwrap_or_else(|e| e.into_inner());
        let id = begin_deed("тест ▸ Стенд", &Deed::Click { label: "Висеть" });
        let during = activity_report();
        assert!(during.contains("выполняется «действие: тест ▸ Стенд → «Висеть»»"), "{during}");
        finish_deed(id);
        let after = activity_report();
        assert!(after.contains("текущего действия нет; последнее — «действие: тест ▸ Стенд → «Висеть»»"), "{after}");
        // Чужой номер текущее действие не закрывает.
        let id2 = begin_deed("тест", &Deed::Click { label: "Ещё" });
        finish_deed(id2 + 1000);
        assert!(activity_report().starts_with("выполняется"));
        finish_deed(id2);
    }

    fn chain(lines: &[&str]) -> Vec<InputItem> {
        parse_input_chain(&lines.join("\n"))
    }

    #[test]
    fn нажатие_кнопки_флажка_и_строки_списка() {
        // Кнопка общего контрола: подпись своя.
        let b = chain(&[
            "RexonButton_QMLTYPE_3\tb\t\tПрименить\tПрименить",
            "QQuickRowLayout\t\t\t\t",
            "ScreenEffectsSection_QMLTYPE_9\t\t\t\t",
            "AppearancePage_QMLTYPE_2\t\t\t\t",
            "MainWindow_QMLTYPE_0\t\t\t\t",
        ]);
        assert_eq!(
            deed_from_input("appearance", &b, false),
            Some(("appearance ▸ AppearancePage ▸ ScreenEffectsSection".into(), InputDeed::Click("Применить".into())))
        );
        // Флажок: шим видит состояние ДО щелчка — пишется новое.
        let c = chain(&["RexonCheckBox_QMLTYPE_4\tbkK\t\tВключить\t", "SettingsPage_QMLTYPE_1\t\t\t\t"]);
        assert_eq!(
            deed_from_input("settings", &c, false),
            Some(("settings ▸ SettingsPage".into(), InputDeed::Toggle("Включить".into(), false)))
        );
        let r = chain(&["QQuickRadioButton\tbkKr\t\tТёмная\t", "SettingsPage_QMLTYPE_1\t\t\t\t"]);
        assert_eq!(deed_from_input("settings", &r, false).unwrap().1, InputDeed::Toggle("Тёмная".into(), true));
        // Строка выпадающего списка: строка → всплывашка → список.
        let d = chain(&[
            "QQuickItemDelegate\tb\t\tСепия\tСепия",
            "QQuickListView\tf\t\t\t",
            "QQuickPopupItem\tp\t\t\t",
            "QQuickPopup\t\t\t\t",
            "RexonComboBox_QMLTYPE_7\tx\t\tОттенки серого\t",
            "ScreenEffectsSection_QMLTYPE_9\t\t\t\t",
        ]);
        assert_eq!(
            deed_from_input("appearance", &d, false),
            Some(("appearance ▸ ScreenEffectsSection".into(), InputDeed::Choose("Сепия".into())))
        );
        // Пункт меню окна — всплывашка без списка: обычное нажатие.
        let m = chain(&[
            "QQuickMenuItem\tb\t\tПерезапустить\t",
            "QQuickPopupItem\tp\t\t\t",
            "QQuickMenu\t\t\t\t",
            "WindowMenu_QMLTYPE_5\t\twindowMenu\t\t",
        ]);
        assert_eq!(deed_from_input("", &m, false).unwrap(), ("WindowMenu".into(), InputDeed::Click("Перезапустить".into())));
    }

    #[test]
    fn карточка_магазина_и_что_не_действие() {
        // TapHandler карточки: подпись — первая надпись карточки.
        let t = chain(&[
            "QQuickTapHandler\th\t\t\t",
            "QQuickRectangle\t\t\t\tGruvbox Plasma",
            "AppearancePage_QMLTYPE_2\t\t\t\t",
        ]);
        assert_eq!(
            deed_from_input("appearance", &t, false),
            Some(("appearance ▸ AppearancePage".into(), InputDeed::Click("Gruvbox Plasma".into())))
        );
        // Кнопка-значок без надписей — по objectName, а нет и его — без подписи.
        let i = chain(&["QQuickButton\tb\tcloseButton\t\t"]);
        assert_eq!(deed_from_input("", &i, false).unwrap().1, InputDeed::Click("closeButton".into()));
        // Не действия.
        for first in [
            "RexonTextField_QMLTYPE_1\ti\t\t\t",
            "QQuickListView\tf\t\t\t",
            "QQuickScrollBar\ts\t\t\t",
            "RexonComboBox_QMLTYPE_7\tx\t\tСепия\t",
            "QQuickRectangle\t\t\t\tТекст",
        ] {
            assert_eq!(deed_from_input("x", &chain(&[first]), false), None, "{first}");
        }
        // Вкладка полосы разделов пишется строкой «раздел», а вкладка
        // внутри страницы — действие.
        let top = chain(&["QQuickTabButton\tbt\t\tОформление\t", "QQuickTabBar\t\t\t\t", "MainWindow_QMLTYPE_0\t\t\t\t"]);
        assert_eq!(deed_from_input("steam", &top, false), None);
        let inner = chain(&["QQuickTabButton\tbt\t\tСцены\t", "QQuickTabBar\t\t\t\t", "WePage_QMLTYPE_3\t\t\t\t"]);
        assert_eq!(deed_from_input("we", &inner, false).unwrap().1, InputDeed::Click("Сцены".into()));
        // Клавиша — только на кнопке.
        let ma = chain(&["QQuickMouseArea\tm\t\t\tКарточка"]);
        assert!(deed_from_input("", &ma, false).is_some());
        assert_eq!(deed_from_input("", &ma, true), None);
    }

    #[test]
    fn поле_ввода_одна_строка_и_никаких_паролей() {
        let _serial = SHARED_ACTIVITY.lock().unwrap_or_else(|e| e.into_inner());
        let area = "стенд-поле ▸ ExtensionsPage";
        // Пусто при первом разе (фокус ушёл, ничего не вводили) — молчим.
        assert_eq!(note_entered(area, "Поиск", "", false), 0);
        let id = note_entered(area, "Поиск", "dash  to dock", false);
        assert!(id > 0);
        finish_deed(id);
        // Повтор того же (Enter, потом уход фокуса) — молчим.
        assert_eq!(note_entered(area, "Поиск", "dash to dock", false), 0);
        // Очистили — строка.
        let id = note_entered(area, "Поиск", "", false);
        assert!(id > 0);
        finish_deed(id);
        // Пароль — никогда.
        assert_eq!(note_entered(area, "Пароль", "secret", true), 0);
        note_typing(area, "Пароль", "secret", true);
        let rep = activity_report();
        assert!(!rep.contains("secret"), "{rep}");
        assert!(rep.contains("текст скрыт"), "{rep}");
        note_typing(area, "Поиск", "gruv", false);
        assert!(activity_report().contains("«Поиск» (стенд-поле ▸ ExtensionsPage): «gruv»"));
    }

    #[test]
    fn сторож_решает_один_раз_на_зависание() {
        let cfg = WatchConfig {
            stall_after: Duration::from_secs(2),
            severe_after: Duration::from_secs(10),
            poll: Duration::from_millis(250),
        };
        let mut st = WatchState::default();
        // Пульса ещё не было — молчим, сколько бы ни прошло.
        assert_eq!(watch_step(&mut st, 60_000, 0, &cfg), None);
        // Пульс бьётся — молчим.
        assert_eq!(watch_step(&mut st, 1_250, 1_000, &cfg), None);
        // 2 с ровно — ещё не зависание.
        assert_eq!(watch_step(&mut st, 3_000, 1_000, &cfg), None);
        assert_eq!(
            watch_step(&mut st, 3_250, 1_000, &cfg),
            Some(WatchEvent::Stalled { silent: Duration::from_millis(2_250) })
        );
        // Дальше без повторов до тяжёлого порога.
        for t in (3_500..=11_000).step_by(250) {
            assert_eq!(watch_step(&mut st, t, 1_000, &cfg), None, "t={t}");
        }
        assert_eq!(
            watch_step(&mut st, 11_250, 1_000, &cfg),
            Some(WatchEvent::Severe { silent: Duration::from_millis(10_250) })
        );
        assert_eq!(watch_step(&mut st, 30_000, 1_000, &cfg), None);
        // Пульс вернулся: висело от прежнего пульса до нового.
        assert_eq!(
            watch_step(&mut st, 30_300, 30_100, &cfg),
            Some(WatchEvent::Recovered { hung: Duration::from_millis(29_100) })
        );
        assert_eq!(watch_step(&mut st, 30_600, 30_350, &cfg), None);
    }

    #[test]
    fn строки_сторожа() {
        let s = stall_line(Duration::from_millis(2_250), "раздел steam", Some("#0 libc.so.6: poll+0x10\n"));
        assert_eq!(s, "окно не отвечает 2.2 с; раздел steam\nстек главного потока:\n#0 libc.so.6: poll+0x10");
        let s = stall_line(Duration::from_millis(3_000), "раздел steam", None);
        assert!(s.ends_with("стек главного потока снять не удалось"));
        assert_eq!(severe_line(Duration::from_millis(10_400), "x"), "окно не отвечает уже 10 с (дольше 10 с); x");
        assert_eq!(recovered_line(Duration::from_millis(3_460)), "окно снова отвечает, висело 3.5 с");
    }

    /// Живой прогон: поток с пульсом искусственно «виснет» во сне — сторож
    /// пишет WARN со стеком, одну строку ERROR и INFO по возвращении.
    #[test]
    fn сторож_ловит_зависший_поток_и_снимает_его_стек() {
        let lines: Arc<Mutex<Vec<(Level, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_lines = Arc::clone(&lines);
        let sink: Sink = Arc::new(move |l, m| sink_lines.lock().unwrap().push((l, m.to_string())));
        let cfg = WatchConfig {
            stall_after: Duration::from_millis(600),
            severe_after: Duration::from_millis(1_500),
            poll: Duration::from_millis(50),
        };
        let dog = Watchdog::new();
        // Наблюдаемый поток — этот, как главный поток окна в продукте.
        assert!(stack::watch_current_thread());
        let handle = dog
            .spawn(
                cfg,
                sink,
                Arc::new(|| "выполняется «действие: стенд → «Висеть»»".to_string()),
                Arc::new(|| stack::capture(Duration::from_millis(500))),
            )
            .expect("поток сторожа");
        for _ in 0..6 {
            dog.beat();
            std::thread::sleep(Duration::from_millis(50));
        }
        // Зависание: поток не бьёт пульс 2 с.
        std::thread::sleep(Duration::from_millis(2_000));
        for _ in 0..8 {
            dog.beat();
            std::thread::sleep(Duration::from_millis(50));
        }
        dog.stop();
        handle.join().unwrap();

        let got = lines.lock().unwrap().clone();
        let warns: Vec<_> = got.iter().filter(|(l, _)| *l == Level::Warn).collect();
        let errors: Vec<_> = got.iter().filter(|(l, _)| *l == Level::Error).collect();
        let infos: Vec<_> = got.iter().filter(|(l, _)| *l == Level::Info).collect();
        assert_eq!((warns.len(), errors.len(), infos.len()), (1, 1, 1), "{got:#?}");
        let warn = &warns[0].1;
        assert!(warn.starts_with("окно не отвечает 0."), "{warn}");
        assert!(warn.contains("«Висеть»"), "{warn}");
        // Поток стоял во сне — в стеке есть кадры glibc.
        assert!(warn.contains("стек главного потока:\n#0 "), "{warn}");
        assert!(warn.contains("libc.so"), "{warn}");
        assert!(errors[0].1.starts_with("окно не отвечает уже "), "{}", errors[0].1);
        let info = &infos[0].1;
        assert!(info.starts_with("окно снова отвечает, висело 2."), "{info}");
    }
}
