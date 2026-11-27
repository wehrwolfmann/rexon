//! Наборы виджетов Conky: несколько виджетов и обои — одной кнопкой.
//!
//! ЗАЧЕМ. Человек собирает стол из нескольких виджетов (часы, процессор,
//! сеть) под определённые обои и хочет переключаться между такими
//! «раскладками», а не запускать каждый виджет заново. Программы-менеджеры
//! conky хранят такой набор файлом `.cmtheme` — им же поставляются наборы в
//! пакетах тем (`Green Apple Desktop.cmtheme`). Rexon:
//! * сохраняет запущенные сейчас виджеты и текущие обои своим набором
//!   ([`save`]) в `~/.config/rexon/conky-sets.json`;
//! * показывает и наборы из файлов `.cmtheme`, которые лежат в папках
//!   виджетов ([`file_sets`]) — пакет тем ставится, и его наборы сразу
//!   готовы к применению;
//! * применяет набор ([`apply`]): гасит ТОЛЬКО виджеты прошлого набора,
//!   которых нет в новом (чужие и запущенные руками не трогает — никакого
//!   «погасить все conky»), запускает недостающие и отдаёт обои вызывающему,
//!   чтобы их поставил общий исполнитель обоев Rexon (любое окружение).
//!
//! Файл `.cmtheme` — строки: путь к файлу настроек виджета, путь к картинке
//! обоев (`.png`/`.jpg`/`.jpeg`), `wallpaper-scaling:<способ>`; путь может
//! быть в кавычках и начинаться с `~`. Пути вида `~/.conky/<Имя>/…` ведут
//! туда, где такой виджет лежит у Rexon (`~/.config/conky/<Имя>/…`), если по
//! старому месту файла нет ([`resolve`]).
//!
//! Безопасность та же, что у кнопки «Запустить»: виджет с командами и
//! сценариями без разрешения не запускается ([`crate::conky_safety`]); окно
//! спрашивает один раз за весь набор ([`apply_question`]), командная строка
//! такие виджеты пропускает и говорит об этом. Программы — тоже как у
//! «Запустить» («всё или ничего», [`crate::conky_safety::prepare_start`]):
//! недостающие из репозиториев ставятся без вопроса одним окном пароля на
//! весь набор, виджет, которому их всё равно не хватает, не запускается.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::animations::LookIo;
use crate::de_apply::Ctx;
use crate::i18n::{ntr, tr, trf};

/// Файл наборов (под `$XDG_CONFIG_HOME`).
pub const STORE_REL: &str = "rexon/conky-sets.json";

/// Расширение файла набора программ-менеджеров conky.
pub const CMTHEME_EXT: &str = ".cmtheme";

/// Набор виджетов.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ConkySet {
    pub name: String,
    /// Файлы настроек виджетов (пути файлов автора).
    pub widgets: Vec<String>,
    /// Картинка обоев; `None` — обои набор не меняет.
    #[serde(default)]
    pub wallpaper: Option<String>,
    /// Файл `.cmtheme`, из которого набор прочитан (у своих — `None`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// Что лежит в файле наборов.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Store {
    #[serde(default)]
    pub sets: Vec<ConkySet>,
    /// Имя набора, применённого последним, — его виджеты гасятся при
    /// применении другого.
    #[serde(default)]
    pub applied: Option<String>,
    /// Виджеты, которые запустило последнее применение (у набора из файла
    /// состав мог поменяться — гасится то, что запускали на деле).
    #[serde(default)]
    pub applied_widgets: Vec<String>,
}

pub fn store_path(ctx: &Ctx) -> String {
    ctx.config(STORE_REL)
}

pub fn load(io: &dyn LookIo, ctx: &Ctx) -> Store {
    io.read(&store_path(ctx)).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn store(io: &dyn LookIo, ctx: &Ctx, s: &Store) -> bool {
    io.write(&store_path(ctx), &serde_json::to_string_pretty(s).unwrap_or_default())
}

// ============================================================================
// Файл .cmtheme
// ============================================================================

/// Разобранный `.cmtheme`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cmtheme {
    /// Пути виджетов как в файле, `~` раскрыт.
    pub widgets: Vec<String>,
    pub wallpaper: Option<String>,
    /// Способ размещения обоев (`zoomed`, `stretched`…) — справочно: Rexon
    /// ставит картинку способом своего модуля обоев.
    pub scaling: Option<String>,
}

fn is_image(path: &str) -> bool {
    let l = path.to_ascii_lowercase();
    l.ends_with(".png") || l.ends_with(".jpg") || l.ends_with(".jpeg")
}

/// `~` и `$HOME` в начале пути — домашний каталог (в середине пути `~` —
/// часть имени, его не трогаем).
fn expand_home(path: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if path == "~" || path == "$HOME" {
        return home.to_string();
    }
    for p in ["~/", "$HOME/"] {
        if let Some(rest) = path.strip_prefix(p) {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}

/// Разобрать текст `.cmtheme`.
pub fn parse_cmtheme(text: &str, home: &str) -> Cmtheme {
    let mut out = Cmtheme::default();
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix('"').unwrap_or(line);
        let line = line.strip_suffix('"').unwrap_or(line).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(v) = line.strip_prefix("wallpaper-scaling") {
            if let Some(v) = v.trim_start().strip_prefix(':') {
                let v = v.trim().to_ascii_lowercase();
                if !v.is_empty() {
                    out.scaling = Some(v);
                }
            }
            continue;
        }
        let path = expand_home(line, home);
        if is_image(&path) {
            out.wallpaper = Some(path);
        } else if !out.widgets.contains(&path) {
            out.widgets.push(path);
        }
    }
    out
}

/// Где файл на деле: по записанному пути, а если там пусто и путь ведёт в
/// `.conky/` (`.Conky/`) — в ту же папку виджета под `~/.config/conky/`.
/// `None` — файла нет ни там, ни там.
pub fn resolve(ctx: &Ctx, path: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    if exists(path) {
        return Some(path.to_string());
    }
    let root = crate::de_apply::conky_root(ctx);
    for marker in ["/.conky/", "/.Conky/", "/.config/Conky/"] {
        if let Some((_, tail)) = path.split_once(marker) {
            let moved = format!("{root}/{tail}");
            if exists(&moved) {
                return Some(moved);
            }
        }
    }
    None
}

fn live_exists(path: &str) -> bool {
    Path::new(path).is_file()
}

/// Файлы `.cmtheme` в папках виджетов: первый и второй уровень под
/// `~/.config/conky/` и местами «Найдено на диске».
pub fn cmtheme_files(io: &dyn LookIo, ctx: &Ctx, extra: &[String]) -> Vec<String> {
    let mut dirs = vec![crate::de_apply::conky_root(ctx)];
    for r in crate::conky_scan::roots(ctx, extra) {
        if !r.files_only && !dirs.contains(&r.dir) {
            dirs.push(r.dir);
        }
    }
    let mut out = Vec::new();
    for dir in dirs {
        for top in io.list_dir(&dir) {
            if top.starts_with('.') {
                continue;
            }
            let path = format!("{dir}/{top}");
            if top.ends_with(CMTHEME_EXT) {
                out.push(path);
                continue;
            }
            for inner in io.list_dir(&path) {
                if inner.ends_with(CMTHEME_EXT) {
                    out.push(format!("{path}/{inner}"));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Имя набора по файлу: имя без `.cmtheme`.
fn file_set_name(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.strip_suffix(CMTHEME_EXT).unwrap_or(base).to_string()
}

/// Набор из файла `.cmtheme`: пути — туда, где файлы лежат на деле;
/// виджеты, которых на диске нет, отбрасываются (их число — второе поле).
pub fn set_of_cmtheme(ctx: &Ctx, path: &str, text: &str, exists: &dyn Fn(&str) -> bool) -> (ConkySet, usize) {
    let c = parse_cmtheme(text, &ctx.home);
    let mut widgets = Vec::new();
    let mut missing = 0;
    for w in &c.widgets {
        match resolve(ctx, w, exists) {
            Some(p) if !widgets.contains(&p) => widgets.push(p),
            Some(_) => {}
            None => missing += 1,
        }
    }
    // Картинку проверяет не `exists` виджетов (тот смотрит текстовые
    // файлы), а тот же поиск: обои пакета лежат рядом с набором.
    let wallpaper = c.wallpaper.as_deref().and_then(|w| resolve(ctx, w, exists));
    (ConkySet { name: file_set_name(path), widgets, wallpaper, file: Some(path.to_string()) }, missing)
}

/// Наборы из файлов `.cmtheme` на диске.
pub fn file_sets(io: &dyn LookIo, ctx: &Ctx, extra: &[String], exists: &dyn Fn(&str) -> bool) -> Vec<(ConkySet, usize)> {
    cmtheme_files(io, ctx, extra)
        .into_iter()
        .filter_map(|p| io.read(&p).map(|t| set_of_cmtheme(ctx, &p, &t, exists)))
        .collect()
}

// ============================================================================
// Список для страницы
// ============================================================================

/// Строка набора для страницы.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetRow {
    pub name: String,
    /// «Gotham, Seamod · с обоями».
    pub summary: String,
    /// Набор применён последним.
    pub applied: bool,
    /// Свой набор Rexon (можно удалить); набор из файла — нет.
    pub own: bool,
    /// Применить нечего: ни одного виджета набора нет на диске.
    pub empty: bool,
    /// Подсказка к строке (откуда набор, чего не хватает).
    pub tooltip: String,
}

/// Имя виджета для сообщений и разрешений по файлу настроек: папка под
/// `~/.config/conky/` — имя плитки, иначе — найденный на диске, иначе сам
/// файл.
pub fn widget_name_of(ctx: &Ctx, extra: &[String], conf: &str) -> String {
    if let Some(id) = crate::de_apply::conky_widget_of_conf(&crate::animations::SystemIo, ctx, conf) {
        return id;
    }
    crate::conky_scan::owner_of_conf(ctx, extra, conf).unwrap_or_else(|| conf.to_string())
}

fn titles(ctx: &Ctx, extra: &[String], widgets: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in widgets {
        let t = crate::conky_scan::widget_title(&widget_name_of(ctx, extra, w));
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// Все наборы: свои, затем из файлов (одноимённый своему файл не дублируется).
pub fn all_sets(io: &dyn LookIo, ctx: &Ctx, extra: &[String], exists: &dyn Fn(&str) -> bool) -> (Store, Vec<(ConkySet, usize)>) {
    let st = load(io, ctx);
    let mut rows: Vec<(ConkySet, usize)> = st.sets.iter().cloned().map(|s| (s, 0)).collect();
    for (s, missing) in file_sets(io, ctx, extra, exists) {
        if !rows.iter().any(|(r, _)| r.name == s.name) {
            rows.push((s, missing));
        }
    }
    (st, rows)
}

pub fn rows(io: &dyn LookIo, ctx: &Ctx, extra: &[String], exists: &dyn Fn(&str) -> bool) -> Vec<SetRow> {
    let (st, sets) = all_sets(io, ctx, extra, exists);
    sets.into_iter()
        .map(|(s, missing)| {
            let names = titles(ctx, extra, &s.widgets);
            let mut summary = names.join(", ");
            if s.wallpaper.is_some() {
                summary = if summary.is_empty() { tr("wallpaper only") } else { trf("%1 · with wallpaper", &[&summary]) };
            }
            let mut tips: Vec<String> = Vec::new();
            if let Some(f) = &s.file {
                tips.push(trf("From the set file %1", &[f]));
            }
            if missing > 0 {
                tips.push(ntr(
                    "%n widget of this set isn't on this computer.",
                    "%n widgets of this set aren't on this computer.",
                    missing as u64,
                ));
            }
            SetRow {
                applied: st.applied.as_deref() == Some(s.name.as_str()),
                own: s.file.is_none(),
                empty: s.widgets.is_empty() && s.wallpaper.is_none(),
                tooltip: tips.join("\n"),
                name: s.name,
                summary,
            }
        })
        .collect()
}

/// Подписи блока «Наборы виджетов».
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Texts {
    pub title: String,
    pub hint: String,
    pub name_placeholder: String,
    pub save: String,
    pub save_tooltip: String,
    pub apply: String,
    pub remove: String,
    pub applied: String,
    pub empty: String,
    pub export: String,
    pub export_tooltip: String,
}

pub fn texts() -> Texts {
    Texts {
        title: tr("Widget sets"),
        hint: tr("A set starts several widgets at once and can change the wallpaper too."),
        name_placeholder: tr("Set name"),
        save: tr("Save running as a set"),
        save_tooltip: tr("Remembers the widgets running now and the current wallpaper."),
        apply: tr("Apply"),
        remove: tr("Delete"),
        applied: tr("✓ Applied"),
        empty: tr("No sets yet: start the widgets you like and save them as a set — then the whole set comes back with one button."),
        export: tr("Save set to file…"),
        export_tooltip: tr("Puts the widgets of the set, their places on the screen and the wallpaper into one .rxn file. On another computer open it with “Install from File…”."),
    }
}

// ============================================================================
// Сохранить, удалить
// ============================================================================

/// Годится ли имя набора: не пустое, без перевода строки.
fn clean_name(name: &str) -> Option<String> {
    let n = name.trim();
    (!n.is_empty() && !n.contains('\n') && n.chars().count() <= 80).then(|| n.to_string())
}

/// Сохранить запущенные сейчас виджеты (и обои `wallpaper`) набором
/// `name`. Свой набор с тем же именем заменяется.
pub fn save(io: &dyn LookIo, ctx: &Ctx, name: &str, wallpaper: Option<String>) -> (bool, String) {
    let Some(name) = clean_name(name) else {
        return (false, tr("Give the set a name."));
    };
    // Многооконная тема со сценарием — одна строка набора.
    let widgets = crate::conky_procs::running_widget_confs(io, ctx);
    if widgets.is_empty() {
        return (false, tr("No widget is running now: start the widgets for the set first."));
    }
    let mut st = load(io, ctx);
    let replaced = st.sets.iter().any(|s| s.name == name);
    st.sets.retain(|s| s.name != name);
    st.sets.push(ConkySet { name: name.clone(), widgets: widgets.clone(), wallpaper, file: None });
    // Сохранённое — то, что запущено: это и есть применённый набор.
    st.applied = Some(name.clone());
    st.applied_widgets = widgets;
    if !store(io, ctx, &st) {
        return (false, trf("Couldn't save the set “%1”", &[&name]));
    }
    let msg = if replaced { trf("Set “%1” updated", &[&name]) } else { trf("Set “%1” saved", &[&name]) };
    (true, msg)
}

/// Вопрос перед удалением набора: `(заголовок, текст)`.
pub fn remove_question(name: &str) -> (String, String) {
    (
        trf("Delete the set “%1”?", &[&name]),
        tr("The widgets and the wallpaper stay as they are — only the set is forgotten."),
    )
}

/// Удалить свой набор (виджеты и файлы не трогаются).
pub fn remove(io: &dyn LookIo, ctx: &Ctx, name: &str) -> (bool, String) {
    let mut st = load(io, ctx);
    let before = st.sets.len();
    st.sets.retain(|s| s.name != name);
    if st.sets.len() == before {
        return (false, trf("There's no set “%1” to delete", &[&name]));
    }
    if st.applied.as_deref() == Some(name) {
        st.applied = None;
        st.applied_widgets.clear();
    }
    if !store(io, ctx, &st) {
        return (false, trf("Couldn't delete the set “%1”", &[&name]));
    }
    (true, trf("Set “%1” deleted", &[&name]))
}

// ============================================================================
// Применить
// ============================================================================

/// Что сделает применение.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub set: ConkySet,
    /// Запущенные виджеты прошлого набора, которых в этом нет, — погасить.
    pub stop: Vec<String>,
    /// Виджеты набора, которые сейчас не запущены, — запустить.
    pub start: Vec<String>,
    /// Имена виджетов из `start`, которым нужно разрешение на команды.
    pub needs_trust: Vec<String>,
}

pub fn find(io: &dyn LookIo, ctx: &Ctx, extra: &[String], name: &str, exists: &dyn Fn(&str) -> bool) -> Option<ConkySet> {
    let (_, sets) = all_sets(io, ctx, extra, exists);
    sets.into_iter().map(|(s, _)| s).find(|s| s.name == name)
}

pub fn plan(io: &dyn LookIo, ctx: &Ctx, extra: &[String], mut set: ConkySet) -> Plan {
    let st = load(io, ctx);
    // Окна одной темы со сценарием (набор сохранён до 16.11.2026 или из
    // файла) — одна строка: тема поднимается и гасится один раз.
    set.widgets = crate::conky_theme::unit_confs(io, ctx, extra, &set.widgets);
    let running = crate::conky_procs::running_widget_confs(io, ctx);
    let prev: Vec<String> = if st.applied.is_some() {
        crate::conky_theme::unit_confs(io, ctx, extra, &st.applied_widgets)
    } else {
        Vec::new()
    };
    let stop: Vec<String> = prev.into_iter().filter(|w| running.contains(w) && !set.widgets.contains(w)).collect();
    let start: Vec<String> = set.widgets.iter().filter(|w| !running.contains(w)).cloned().collect();
    let mut needs_trust: Vec<String> = Vec::new();
    for w in &start {
        let name = widget_name_of(ctx, extra, w);
        if !needs_trust.contains(&name) && crate::conky_safety::start_refusal(io, ctx, &name).is_some() {
            needs_trust.push(name);
        }
    }
    Plan { set, stop, start, needs_trust }
}

/// Один вопрос за весь набор: какие виджеты выполняют команды. `None` —
/// спрашивать не о чем.
pub fn apply_question(io: &dyn LookIo, ctx: &Ctx, p: &Plan) -> Option<(String, String)> {
    if p.needs_trust.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    for name in &p.needs_trust {
        if let Some((_, text)) = crate::conky_safety::start_question(io, ctx, name) {
            parts.push(text);
        }
    }
    Some((
        trf("Apply the set “%1”?", &[&p.set.name]),
        format!(
            "{}\n\n{}",
            ntr(
                "%n widget of this set runs commands on your computer:",
                "%n widgets of this set run commands on your computer:",
                p.needs_trust.len() as u64,
            ),
            parts.join("\n\n")
        ),
    ))
}

/// Итог применения.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    pub ok: bool,
    pub message: String,
    /// Обои набора — поставить их общим исполнителем обоев.
    pub wallpaper: Option<String>,
}

/// Запуск одного виджета: `(имя виджета, подпись, файл настроек)` →
/// `(получилось, сообщение)`. Живьём — [`live_start`]; сверки подставляют
/// свой, чтобы не запускать conky.
pub type Starter<'a> = &'a dyn Fn(&str, &str, &str) -> (bool, String);

/// Живой запуск виджета: ремонт путей и `conky` из папки виджета с
/// журналом — как у кнопки «Запустить».
pub fn live_start(name: &str, title: &str, conf: &str) -> (bool, String) {
    let ctx = Ctx::live();
    crate::de_apply::conky_prepare(&ctx, name);
    crate::conky_procs::start_conf(&crate::animations::SystemIo, &ctx, title, conf)
}

/// Готовность виджетов набора к запуску — имена виджетов → по ответу на
/// каждое, в том же порядке. Живьём — общий путь «всё или ничего» кнопки
/// «▶️ Запустить» ([`crate::conky_safety::live_prepare_start`]: недостающие
/// программы из репозиториев ставятся без вопроса одним окном пароля);
/// сверки подставляют свой.
pub type Ready<'a> = &'a dyn Fn(&[String]) -> Vec<crate::conky_safety::Readiness>;

/// Живая готовность виджетов набора.
pub fn live_ready(names: &[String]) -> Vec<crate::conky_safety::Readiness> {
    crate::conky_safety::live_prepare_start(names, crate::conky_safety::Retry::Set)
}

/// Применить план. `trusted` — человек разрешил команды виджетов из
/// `needs_trust` (окно); `false` — такие виджеты пропускаются с пояснением
/// (командная строка, отказ в окне). Оставшиеся проходят `ready` — тот же
/// путь, что у «▶️ Запустить»: виджет без нужных программ не запускается, а
/// в итоге сказано, какой и почему; остальные виджеты набора запускаются.
pub fn apply(io: &dyn LookIo, ctx: &Ctx, extra: &[String], p: &Plan, trusted: bool, ready: Ready, start: Starter) -> Applied {
    let mut lines: Vec<String> = Vec::new();
    let mut ok = true;
    if !p.stop.is_empty() {
        // Тема со сценарием — по любому её окну, не только по главному файлу.
        let pids = crate::conky_procs::pids_of_units(io, ctx, &p.stop);
        let title = titles(ctx, extra, &p.stop).join(", ");
        let (stopped, msg) = crate::conky_procs::stop_pids(io, &title, pids);
        ok &= stopped;
        lines.push(msg);
    }
    let mut started: Vec<String> = Vec::new();
    // Сначала разрешение на команды, потом программы: виджет, которому
    // отказали в разрешении, пакеты не ставит.
    let mut queue: Vec<(&String, String)> = Vec::new();
    for conf in &p.start {
        let name = widget_name_of(ctx, extra, conf);
        if p.needs_trust.contains(&name) {
            if !trusted {
                let text = crate::conky_safety::declined_text(&name);
                if !lines.contains(&text) {
                    lines.push(text);
                }
                continue;
            }
            crate::conky_safety::remember(io, ctx, &name);
        }
        queue.push((conf, name));
    }
    let mut names: Vec<String> = Vec::new();
    for (_, name) in &queue {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    let verdicts = if names.is_empty() { Vec::new() } else { ready(&names) };
    let mut told: Vec<String> = Vec::new();
    for (conf, name) in queue {
        let verdict = names.iter().position(|n| *n == name).and_then(|i| verdicts.get(i));
        let note = match verdict {
            Some(Err(why)) => {
                ok = false;
                if !told.contains(&name) {
                    lines.push(why.clone());
                    told.push(name);
                }
                continue;
            }
            Some(Ok(note)) => note.clone(),
            None => None,
        };
        let title = crate::conky_scan::widget_title(&name);
        let (good, msg) = start(&name, &title, conf);
        if good {
            started.push(conf.clone());
        }
        ok &= good;
        lines.push(msg);
        if let Some(n) = note.filter(|_| good && !told.contains(&name)) {
            lines.push(n);
            told.push(name);
        }
    }
    let mut st = load(io, ctx);
    st.applied = Some(p.set.name.clone());
    let running = crate::conky_procs::running_widget_confs(io, ctx);
    st.applied_widgets = p.set.widgets.iter().filter(|w| running.contains(w) || started.contains(w)).cloned().collect();
    store(io, ctx, &st);
    if p.start.is_empty() && p.stop.is_empty() {
        lines.push(trf("The widgets of “%1” are already running.", &[&p.set.name]));
    }
    Applied { ok, message: lines.join("\n"), wallpaper: p.set.wallpaper.clone() }
}

// ============================================================================
// Живая машина
// ============================================================================

fn live() -> (crate::animations::SystemIo, Ctx, Vec<String>) {
    let ctx = Ctx::live();
    let extra = crate::conky_scan::extra_dirs_for(&ctx);
    (crate::animations::SystemIo, ctx, extra)
}

pub fn live_rows() -> Vec<SetRow> {
    let (io, ctx, extra) = live();
    rows(&io, &ctx, &extra, &live_exists)
}

/// Картинка обоев стола сейчас (для «Сохранить как набор»).
pub fn live_current_wallpaper() -> Option<String> {
    let slot = crate::wallpaper_exec::live_slot();
    crate::wallpaper_exec::try_with_slot(&slot, |e| e.desktop_static_image()).flatten()
}

pub fn live_save(name: &str) -> (bool, String) {
    let (io, ctx, _) = live();
    save(&io, &ctx, name, live_current_wallpaper())
}

pub fn live_remove(name: &str) -> (bool, String) {
    let (io, ctx, _) = live();
    remove(&io, &ctx, name)
}

/// План и вопрос для окна. `Err` — набора нет.
pub fn live_plan(name: &str) -> Result<(Plan, Option<(String, String)>), String> {
    let (io, ctx, extra) = live();
    let Some(set) = find(&io, &ctx, &extra, name, &live_exists) else {
        return Err(trf("There's no set “%1”", &[&name]));
    };
    // Ремонт путей правит файлы виджета — до отпечатка разрешения, а не после.
    for w in &set.widgets {
        crate::de_apply::conky_prepare(&ctx, &widget_name_of(&ctx, &extra, w));
    }
    let p = plan(&io, &ctx, &extra, set);
    let q = apply_question(&io, &ctx, &p);
    Ok((p, q))
}

/// Применить план и поставить обои набора. `(получилось, сообщение)`.
pub fn live_apply(p: &Plan, trusted: bool) -> (bool, String) {
    let (io, ctx, extra) = live();
    // Самой программы Conky нет — ставится без вопроса, как у «▶️
    // Запустить» (защита — окно пароля); не встала — набор не применяется.
    if !p.start.is_empty() && !io.have("conky") {
        if crate::de_apply::conky_install_package(false, crate::de_apply::live_conky_package()).is_none() {
            return (false, crate::de_apply::conky_missing_refusal());
        }
        if let Err(why) = crate::de_apply::install_conky_live() {
            crate::log_warn!("conky", "Conky для набора «{}» не встал: {why}", p.set.name);
            // TRANSLATORS: %1 is the reason, %2 the name of a widget set.
            return (false, trf("Couldn't install Conky: %1. The widgets of the set “%2” weren't started.", &[&why, &p.set.name]));
        }
    }
    let done = apply(&io, &ctx, &extra, p, trusted, &live_ready, &live_start);
    let mut ok = done.ok;
    let mut lines = vec![done.message];
    if let Some(wp) = done.wallpaper {
        let slot = crate::wallpaper_exec::live_slot();
        let title = crate::conky_scan::widget_title(&wp);
        let r = crate::wallpaper_exec::with_slot(&slot, |e| e.apply(&wp, "static", &title));
        ok &= r.ok;
        lines.push(r.message);
    }
    (ok, lines.into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n"))
}

/// `rexon --conky-set <имя>`: применить набор без окна. Виджеты с
/// командами без разрешения не запускаются (спросить некого).
pub fn cli_route(args: &[String], out: &mut dyn FnMut(&str)) -> i32 {
    let name = args
        .iter()
        .position(|a| a == CLI_FLAG)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .or_else(|| args.iter().find_map(|a| a.strip_prefix(&format!("{CLI_FLAG}=")).map(str::to_string)));
    let Some(name) = name.filter(|n| !n.trim().is_empty()) else {
        out(&tr("Name the set: rexon --conky-set \"<set name>\""));
        for r in live_rows() {
            out(&format!("  {}", r.name));
        }
        return 2;
    };
    match live_plan(&name) {
        Err(why) => {
            out(&why);
            1
        }
        Ok((p, _)) => {
            let (ok, msg) = live_apply(&p, false);
            for line in msg.lines() {
                out(line);
            }
            if ok { 0 } else { 1 }
        }
    }
}

/// Флаг командной строки.
pub const CLI_FLAG: &str = "--conky-set";

// ============================================================================
// Набор в файл .rxn и обратно
// ============================================================================
//
// ЗАЧЕМ. Человек собрал стол из виджетов и хочет такой же на другой машине
// (или отдать другу). «Сохранить набор в файл…» кладёт в один `.rxn` папки
// виджетов набора (конфиги, Lua, картинки, шрифты — всё, что лежит в папке
// виджета), обои набора, положение и экран каждого виджета (накладка
// Rexon), список нужных программ и шрифтов. «Установить из файла…» на
// другой машине раскладывает его обратно ([`crate::ext_manager`], ветка
// `conky_set`): каждая папка — свой виджет `~/.config/conky/<папка>`, шрифты
// — в `~/.local/share/fonts/rexon-conky-<id>`, набор — файлом `.cmtheme` в
// `~/.config/conky/<id>/` (его сразу видят «Наборы виджетов»), положение —
// накладкой ([`after_set_install`]).
//
// Раскладка архива:
//   manifest.json            — тип `theme-conky`, раздел [`SET_MANIFEST_KEY`];
//   preview.png              — снимок виджета из кеша превью (если есть);
//   data/<папка>/…           — папка каждого виджета;
//   data/<id>/<имя>.cmtheme  — сам набор; рядом `wallpaper.<расш>`.
//
// Защита от дурака (личное). Абсолютные пути домашней папки переписываются
// в `~/…` (пути в папку виджета — сразу туда, где виджет встанет у
// получателя), так что имя пользователя с путями не уезжает. Почта, пароли,
// ключи погодных служб, координаты, имена сетей Wi-Fi, имя пользователя и
// машины вне путей автоматически не чинятся (правка угадалась бы неверно):
// сохранение останавливается со списком «что и где» ([`ExportError::Private`]),
// и окно спрашивает, сохранить ли всё равно. Файлы вне папок виджетов не
// берутся вовсе — их список уходит в итог. Разрешение на команды виджета
// НЕ переносится: на другой машине Rexon спросит заново. Автозапуск
// записывается, но установка его не включает (правило 04.10.2026: установка
// только кладёт виджет) — об этом говорит итог установки.

/// Раздел манифеста с описанием набора.
pub const SET_MANIFEST_KEY: &str = "conky_set";
/// Начало id пакета набора.
pub const SET_ID_PREFIX: &str = "conky-set-";
/// Больше этого в один файл не кладётся (папки виджетов вместе).
const EXPORT_MAX_BYTES: u64 = 200 * 1024 * 1024;
/// Текстовые файлы больше этого не переписываются и не проверяются.
const EXPORT_TEXT_MAX: u64 = 1024 * 1024;
/// Картинка обоев больше этого в набор не кладётся.
const EXPORT_WALLPAPER_MAX: u64 = 64 * 1024 * 1024;

/// Виджет набора в файле.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExportedWidget {
    /// Папка в `data/` — она же папка под `~/.config/conky/` у получателя.
    pub folder: String,
    /// Файл настроек внутри папки.
    pub conf: String,
    /// Положение, экран, фон… — выбор человека поверх файла автора.
    #[serde(default, skip_serializing_if = "crate::conky_overlay::Overrides::is_empty")]
    pub settings: crate::conky_overlay::Overrides,
    /// Был включён «Запускать при входе» — с этой задержкой, секунд.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autostart_delay: Option<u32>,
}

/// Раздел [`SET_MANIFEST_KEY`] манифеста.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SetManifest {
    pub name: String,
    pub widgets: Vec<ExportedWidget>,
    /// Файл обоев в `data/<id>/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<String>,
    /// Программы, которые зовут команды виджетов (`cava`, `sensors`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<String>,
    /// Шрифты из настроек виджетов (семейства).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<String>,
}

/// Что известно о машине, с которой сохраняют.
#[derive(Debug, Clone, Default)]
pub struct ExportEnv {
    pub user: String,
    pub host: String,
    /// `$XDG_STATE_HOME` (накладки виджетов); `None` — `~/.local/state`.
    pub state_home: Option<String>,
}

impl ExportEnv {
    pub fn live(ctx: &Ctx) -> Self {
        let user = std::env::var("USER")
            .ok()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| crate::conky_scan::widget_title(&ctx.home));
        let host = std::fs::read_to_string("/etc/hostname").unwrap_or_default().trim().to_string();
        let state_home = std::env::var("XDG_STATE_HOME").ok().filter(|s| !s.is_empty());
        ExportEnv { user, host, state_home }
    }
}

/// Итог сохранения.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Exported {
    pub path: String,
    pub widgets: usize,
    /// Файлы вне папок виджетов, на которые ссылаются виджеты (`~/…`).
    pub left_out: Vec<String>,
    /// Виджеты набора, которых нет на диске.
    pub missing: usize,
    pub wallpaper: bool,
}

/// Почему набор не сохранён.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// Нашлось личное: строки «файл:строка — что». Сохранить можно только
    /// после согласия человека (`allow_private`).
    Private(Vec<String>),
    Failed(String),
}

/// Имя пакета набора: `conky-set-<имя латиницей>`.
pub fn set_package_id(name: &str) -> String {
    let slug = crate::importers::slug60(name);
    format!("{SET_ID_PREFIX}{}", if slug.is_empty() { "set".to_string() } else { slug })
}

/// Годное имя файла/папки из `name`: без `/`, не скрытое, не пустое.
fn safe_component(name: &str, fallback: &str) -> String {
    let n: String = name.trim().trim_start_matches('.').replace('/', "-").chars().take(80).collect();
    let n = n.trim().to_string();
    if n.is_empty() || n == ".." { fallback.to_string() } else { n }
}

/// Шрифты, названные в настройках виджета: `font = 'Roboto:size=10'`,
/// `xftfont`, `${font Ubuntu Mono:bold:size=9}`.
pub fn config_fonts(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |raw: &str| {
        let fam = raw.split(':').next().unwrap_or("").trim().trim_matches(['"', '\'']).trim();
        if !fam.is_empty() && !fam.starts_with('$') && !out.iter().any(|f| f == fam) {
            out.push(fam.to_string());
        }
    };
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with("--") || s.starts_with('#') {
            continue;
        }
        // `font = '…',` (1.10) и `font …` / `xftfont …` (1.9).
        for key in ["font", "xftfont"] {
            let Some(rest) = s.strip_prefix(key) else { continue };
            if !rest.starts_with(|c: char| c.is_whitespace() || c == '=') {
                continue;
            }
            let rest = rest.trim_start();
            let value = rest.strip_prefix('=').map(str::trim_start).unwrap_or(rest);
            let v = value.trim_end_matches(',').trim().trim_matches(['"', '\'']);
            if !v.is_empty() && !v.contains("${") {
                push(v);
            }
        }
        let mut from = 0;
        while let Some(rel) = s[from..].find("${font ") {
            let start = from + rel + "${font ".len();
            let end = s[start..].find('}').map(|e| start + e).unwrap_or(s.len());
            push(&s[start..end]);
            from = end;
        }
    }
    out
}

/// Переписать пути одного текста: папка виджета `src` (абсолютно и от
/// `~`) — на `~/.config/conky/<folder>`, остальная домашняя папка — на `~`.
fn relocate_text(text: &str, home: &str, src: Option<&str>, folder: &str) -> String {
    let home = home.trim_end_matches('/');
    let target = format!("~/.config/conky/{folder}");
    let mut out = text.to_string();
    if let Some(src) = src.map(|s| s.trim_end_matches('/')).filter(|s| !s.is_empty()) {
        let mut forms = vec![src.to_string()];
        if let Some(rel) = src.strip_prefix(&format!("{home}/")) {
            for head in ["~/", "$HOME/", "${HOME}/"] {
                forms.push(format!("{head}{rel}"));
            }
        }
        for f in forms {
            if out.contains(&f) {
                out = crate::rxn_manifest::replace_path_prefix(&out, &f, &target);
            }
        }
    }
    if !home.is_empty() && out.contains(home) {
        out = crate::rxn_manifest::replace_path_prefix(&out, home, "~");
    }
    out
}

/// Пути `~/…` в тексте, которые ведут в существующий файл вне
/// `~/.config/conky/<folder>`: виджет на них опирается, а в набор они не
/// попадут.
fn outside_refs(text: &str, home: &str, folder: &str) -> Vec<String> {
    let inside = format!("~/.config/conky/{folder}/");
    let mut out: Vec<String> = Vec::new();
    for (pos, _) in text.match_indices("~/") {
        let prev_ok = text[..pos].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || "_./-".contains(c)));
        if !prev_ok {
            continue;
        }
        let rest = &text[pos..];
        let end = rest.find(|c: char| c.is_whitespace() || "'\"`}{)(][,;|<>&$\\=".contains(c)).unwrap_or(rest.len());
        let p = rest[..end].trim_end_matches(['.', ':']);
        if p.starts_with(&inside) || p.len() <= 2 {
            continue;
        }
        let abs = format!("{}/{}", home.trim_end_matches('/'), &p[2..]);
        if Path::new(&abs).is_file() && !out.iter().any(|o| o == p) {
            out.push(p.to_string());
        }
    }
    out
}

/// Личное в тексте после перевода путей: строки «файл:строка — что».
fn private_lines(text: &str, file: &str, env: &ExportEnv) -> Vec<String> {
    use crate::rxn_manifest::{find_secrets, scrub_text, PrivateKind, ScrubContext};
    let mut out: Vec<String> = find_secrets(text)
        .into_iter()
        .map(|f| format!("{file}:{} — {}", f.line, f.kind.label()))
        .collect();
    let sc = ScrubContext { home: String::new(), user: env.user.clone(), host: env.host.clone() };
    for kind in scrub_text(text, &sc).replaced {
        let word = if kind == PrivateKind::UserName { &env.user } else { &env.host };
        let line = text.lines().position(|l| l.contains(word.as_str())).map(|i| i + 1).unwrap_or(1);
        out.push(format!("{file}:{line} — {}", kind.label()));
    }
    out
}

/// Скопировать папку виджета `src` в `dest`, переписав пути в текстовых
/// файлах. Не берутся: ссылки, скрытые каталоги (`.git`), метка владельца
/// Rexon, файлы, которые не выкладываются никогда
/// ([`crate::rxn_manifest::never_export`]). `budget` — сколько байт ещё можно.
#[allow(clippy::too_many_arguments)]
fn copy_widget_tree(
    src: &Path,
    dest: &Path,
    rel: &str,
    home: &str,
    src_dir: &str,
    folder: &str,
    env: &ExportEnv,
    budget: &mut u64,
    private: &mut Vec<String>,
    left_out: &mut Vec<String>,
) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut entries: Vec<_> = std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(ft) = e.file_type() else { continue };
        let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if ft.is_symlink()
            || name == crate::ext_manager::OWNER_MARK
            || name == crate::installed_scan::REXON_PREVIEW_NAME
            || crate::rxn_manifest::never_export(&child_rel)
        {
            continue;
        }
        if ft.is_dir() {
            if !name.starts_with('.') {
                copy_widget_tree(&e.path(), &dest.join(&name), &child_rel, home, src_dir, folder, env, budget, private, left_out)?;
            }
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        if size > *budget {
            return Err(trf("The widgets are too big for one file (more than %1 MB).", &[&(EXPORT_MAX_BYTES / 1024 / 1024)]));
        }
        *budget -= size;
        let bytes = std::fs::read(e.path()).map_err(|e| e.to_string())?;
        let text = (size <= EXPORT_TEXT_MAX && !bytes.contains(&0)).then(|| String::from_utf8(bytes.clone()).ok()).flatten();
        match text {
            Some(t) => {
                let fixed = relocate_text(&t, home, Some(src_dir), folder);
                private.extend(private_lines(&fixed, &format!("{folder}/{child_rel}"), env));
                for r in outside_refs(&fixed, home, folder) {
                    if !left_out.contains(&r) {
                        left_out.push(r);
                    }
                }
                std::fs::write(dest.join(&name), fixed).map_err(|e| e.to_string())?;
            }
            None => std::fs::write(dest.join(&name), &bytes).map_err(|e| e.to_string())?,
        }
    }
    Ok(())
}

/// Где лежит виджет файла `conf`: `(папка-источник или None — виджет одним
/// файлом, имя папки в наборе, файл настроек внутри неё)`.
fn widget_source(ctx: &Ctx, extra: &[String], conf: &str) -> (Option<String>, String, String) {
    let name = widget_name_of(ctx, extra, conf);
    let dir = if crate::conky_scan::is_found_name(&name) { name.clone() } else { crate::conky_scan::widget_path(ctx, &name) };
    if dir != conf && Path::new(&dir).is_dir() {
        if let Some(rel) = conf.strip_prefix(&format!("{}/", dir.trim_end_matches('/'))) {
            let folder = safe_component(&crate::conky_scan::widget_title(&dir), "conky");
            return (Some(dir), folder, rel.to_string());
        }
    }
    // Виджет одним файлом (`~/.conkyrc`, `~/.config/conky/conky.conf`):
    // берётся только он сам — соседи в его папке чужие.
    let base = crate::conky_scan::widget_title(conf);
    let stem = base.trim_start_matches('.');
    let folder = safe_component(stem.split('.').next().unwrap_or(stem), "conky");
    let file = if base.starts_with('.') || base.is_empty() { "conky.conf".to_string() } else { base };
    (None, folder, file)
}

/// Сохранить набор `set` в файл `dest` (`.rxn`). `allow_private` — человек
/// видел список личного и согласен.
pub fn export(
    io: &dyn LookIo,
    ctx: &Ctx,
    extra: &[String],
    env: &ExportEnv,
    set: &ConkySet,
    dest: &str,
    allow_private: bool,
) -> Result<Exported, ExportError> {
    let fail = |e: String| ExportError::Failed(e);
    let id = set_package_id(&set.name);
    let base = std::env::temp_dir();
    let stage = crate::ext_exec::mkdtemp(&base, "rexon_conky_set_").map_err(fail)?;
    let result = export_into(io, ctx, extra, env, set, &stage, &id, allow_private).and_then(|mut done| {
        crate::ext_utils::pack_rxn(&stage, Path::new(dest)).map_err(|e| fail(e.to_string()))?;
        done.path = dest.to_string();
        Ok(done)
    });
    let _ = std::fs::remove_dir_all(&stage);
    if result.is_err() && !matches!(result, Err(ExportError::Private(_))) {
        let _ = std::fs::remove_file(dest);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn export_into(
    io: &dyn LookIo,
    ctx: &Ctx,
    extra: &[String],
    env: &ExportEnv,
    set: &ConkySet,
    stage: &Path,
    id: &str,
    allow_private: bool,
) -> Result<Exported, ExportError> {
    let fail = |e: String| ExportError::Failed(e);
    let data = stage.join("data");
    let home = ctx.home.trim_end_matches('/').to_string();
    let mut budget = EXPORT_MAX_BYTES;
    let mut private: Vec<String> = Vec::new();
    let mut done = Exported::default();
    let mut manifest = SetManifest { name: set.name.clone(), ..Default::default() };
    // Папка виджета — один раз, даже если в наборе несколько его частей.
    let mut copied: Vec<(String, String)> = Vec::new();
    let mut confs_alive: Vec<String> = Vec::new();
    for conf in &set.widgets {
        if !Path::new(conf).is_file() {
            done.missing += 1;
            continue;
        }
        let (src, folder0, rel) = widget_source(ctx, extra, conf);
        let key = src.clone().unwrap_or_else(|| conf.clone());
        let folder = match copied.iter().find(|(k, _)| *k == key) {
            Some((_, f)) => f.clone(),
            None => {
                let mut folder = folder0.clone();
                let mut n = 2;
                while folder == id || folder == "fonts" || copied.iter().any(|(_, f)| *f == folder) {
                    folder = format!("{folder0}-{n}");
                    n += 1;
                }
                let dest = data.join(&folder);
                match &src {
                    Some(dir) => copy_widget_tree(
                        Path::new(dir),
                        &dest,
                        "",
                        &home,
                        dir,
                        &folder,
                        env,
                        &mut budget,
                        &mut private,
                        &mut done.left_out,
                    )
                    .map_err(fail)?,
                    None => {
                        let text = std::fs::read_to_string(conf).map_err(|e| fail(e.to_string()))?;
                        let fixed = relocate_text(&text, &home, None, &folder);
                        private.extend(private_lines(&fixed, &format!("{folder}/{rel}"), env));
                        for r in outside_refs(&fixed, &home, &folder) {
                            if !done.left_out.contains(&r) {
                                done.left_out.push(r);
                            }
                        }
                        std::fs::create_dir_all(&dest).map_err(|e| fail(e.to_string()))?;
                        std::fs::write(dest.join(&rel), fixed).map_err(|e| fail(e.to_string()))?;
                    }
                }
                copied.push((key, folder.clone()));
                done.widgets += 1;
                // Программы и разрешение считаются по имени плитки виджета.
                let name = widget_name_of(ctx, extra, conf);
                for p in crate::conky_safety::widget_programs(io, ctx, &name) {
                    if !manifest.programs.contains(&p) {
                        manifest.programs.push(p);
                    }
                }
                folder
            }
        };
        let text = std::fs::read_to_string(conf).unwrap_or_default();
        for f in config_fonts(&text) {
            if !manifest.fonts.contains(&f) {
                manifest.fonts.push(f);
            }
        }
        let mut settings = crate::conky_overlay::load(io, &crate::conky_overlay::paths(ctx, env.state_home.as_deref(), conf));
        // Имена сетевых карт у другой машины свои.
        settings.net_ifaces.clear();
        let name = widget_name_of(ctx, extra, conf);
        let auto = crate::conky_autostart::widget_view(io, ctx, &name);
        manifest.widgets.push(ExportedWidget { folder, conf: rel, settings, autostart_delay: auto.on.then_some(auto.delay) });
        confs_alive.push(conf.clone());
    }
    if manifest.widgets.is_empty() && set.wallpaper.is_none() {
        return Err(fail(trf("None of the widgets of “%1” is on this computer — there's nothing to save.", &[&set.name])));
    }
    if !private.is_empty() && !allow_private {
        return Err(ExportError::Private(private));
    }

    // Сам набор: `.cmtheme` с путями туда, где виджеты встанут у получателя.
    let set_dir = data.join(id);
    std::fs::create_dir_all(&set_dir).map_err(|e| fail(e.to_string()))?;
    let mut lines: Vec<String> = manifest.widgets.iter().map(|w| format!("~/.config/conky/{}/{}", w.folder, w.conf)).collect();
    if let Some(wp) = set.wallpaper.as_deref().filter(|w| is_image(w)) {
        let size = std::fs::metadata(wp).map(|m| m.len()).unwrap_or(u64::MAX);
        if size <= EXPORT_WALLPAPER_MAX && size <= budget {
            let ext = wp.rsplit('.').next().unwrap_or("png").to_ascii_lowercase();
            let file = format!("wallpaper.{ext}");
            std::fs::copy(wp, set_dir.join(&file)).map_err(|e| fail(e.to_string()))?;
            lines.push(format!("~/.config/conky/{id}/{file}"));
            manifest.wallpaper = Some(file);
            done.wallpaper = true;
        }
    }
    let cmtheme = format!("{}{CMTHEME_EXT}", safe_component(&set.name, "set"));
    std::fs::write(set_dir.join(cmtheme), lines.join("\n") + "\n").map_err(|e| fail(e.to_string()))?;

    // Обложка — снимок виджета из кеша превью, если он уже снят.
    if let Some(shot) = crate::conky_preview::cached(ctx, &confs_alive) {
        let _ = std::fs::copy(shot, stage.join("preview.png"));
    }

    let titles: Vec<String> = manifest.widgets.iter().map(|w| w.folder.clone()).collect();
    // Пути домашней папки переписаны всегда; найденное личное человек
    // разрешил оставить — тогда «вычищено» не говорим.
    let removed = vec![crate::rxn_manifest::PrivateKind::HomePath.label()];
    // TRANSLATORS: description of a saved Conky widget set; %1 lists the widgets.
    let description = trf("Conky widget set: %1", &[&titles.join(", ")]);
    let json = serde_json::json!({
        "format_version": 3,
        "id": id,
        "name": set.name,
        "version": "1.0",
        "author": "Rexon",
        "description": description,
        "type": crate::de_apply::CONKY_KIND,
        "tags": ["conky", "widget-set"],
        "de_support": ["any"],
        SET_MANIFEST_KEY: manifest,
        "privacy": {"exported_by": "rexon", "scrubbed": private.is_empty(), "removed": removed},
    });
    let text = serde_json::to_string_pretty(&json).map_err(|e| fail(e.to_string()))?;
    std::fs::write(stage.join("manifest.json"), text).map_err(|e| fail(e.to_string()))?;
    Ok(done)
}

/// Подписи окна сохранения: `(заголовок, фильтр)`.
pub fn export_dialog() -> (String, String) {
    (tr("Save the widget set to a file"), format!("{} (*.rxn)", tr("Rexon package")))
}

/// Предложенный путь файла набора в папке `home`.
pub fn export_suggested(home: &str, name: &str) -> String {
    format!("{}/{}.rxn", home.trim_end_matches('/'), safe_component(name, "conky-set"))
}

/// Путь из окна сохранения — с окончанием `.rxn`.
pub fn export_path(chosen: &str) -> String {
    if chosen.to_ascii_lowercase().ends_with(".rxn") { chosen.to_string() } else { format!("{chosen}.rxn") }
}

/// Вопрос, когда в виджетах нашлось личное: `(заголовок, текст)`.
pub fn export_private_question(name: &str, found: &[String]) -> (String, String) {
    const SHOWN: usize = 12;
    let mut list: Vec<String> = found.iter().take(SHOWN).map(|l| format!("• {l}")).collect();
    if found.len() > SHOWN {
        list.push(ntr("…and %n more", "…and %n more", (found.len() - SHOWN) as u64));
    }
    (
        trf("Save “%1” with personal data?", &[&name]),
        format!(
            "{}\n\n{}\n\n{}",
            tr("The widgets of this set contain what looks like personal data:"),
            list.join("\n"),
            tr("Anyone who gets the file will see it. Better remove it from the widget files first. Save anyway?")
        ),
    )
}

/// Итог для человека.
pub fn export_message(done: &Exported) -> String {
    let mut lines = vec![trf("Saved: %1", &[&done.path])];
    if done.missing > 0 {
        lines.push(ntr(
            "%n widget of the set isn't on this computer and wasn't saved.",
            "%n widgets of the set aren't on this computer and weren't saved.",
            done.missing as u64,
        ));
    }
    if !done.left_out.is_empty() {
        // TRANSLATORS: %1 lists files like ~/scripts/weather.sh.
        lines.push(trf("Not included (outside the widget folders): %1", &[&done.left_out.join(", ")]));
    }
    lines.join("\n")
}

/// Сохранить набор `name` в `dest` на живой машине.
pub fn live_export(name: &str, dest: &str, allow_private: bool) -> Result<Exported, ExportError> {
    let (io, ctx, extra) = live();
    let Some(set) = find(&io, &ctx, &extra, name, &live_exists) else {
        return Err(ExportError::Failed(trf("There's no set “%1”", &[&name])));
    };
    export(&io, &ctx, &extra, &ExportEnv::live(&ctx), &set, dest, allow_private)
}

/// Набор из манифеста пакета (`None` — пакет не набор).
pub fn set_of_manifest(manifest: &serde_json::Map<String, serde_json::Value>) -> Option<SetManifest> {
    serde_json::from_value(manifest.get(SET_MANIFEST_KEY)?.clone()).ok()
}

/// После установки набора из файла: положение и прочий выбор каждого
/// виджета — накладкой (своей у человека не было), заметки для итога
/// (автозапуск не включён, каких шрифтов нет). Виджет, чью папку занял
/// чужой (метки владельца `id` нет), не трогается.
pub fn after_set_install(io: &dyn LookIo, ctx: &Ctx, state_home: Option<&str>, id: &str, set: &SetManifest, have_font: &dyn Fn(&str) -> bool) -> Vec<String> {
    let root = crate::de_apply::conky_root(ctx);
    let mut notes = Vec::new();
    let mut auto = false;
    for w in &set.widgets {
        let dir = format!("{root}/{}", w.folder);
        let mark = io.read(&format!("{dir}/{}", crate::ext_manager::OWNER_MARK));
        if mark.as_deref().map(str::trim) != Some(id) {
            continue;
        }
        auto |= w.autostart_delay.is_some();
        if w.settings.is_empty() {
            continue;
        }
        let conf = format!("{dir}/{}", w.conf);
        let p = crate::conky_overlay::paths(ctx, state_home, &conf);
        if io.read(&p.settings).is_none() {
            io.write(&p.settings, &serde_json::to_string_pretty(&w.settings).unwrap_or_default());
        }
    }
    notes.push(trf("The set “%1” is in “Widget sets” — apply it there.", &[&set.name]));
    if auto {
        notes.push(tr("Some widgets of the set started at login on the other computer: turn on “Start at login” on their cards if you want that here too."));
    }
    let missing: Vec<&str> = set.fonts.iter().map(String::as_str).filter(|f| !have_font(f)).collect();
    if !missing.is_empty() {
        // TRANSLATORS: %1 lists font names.
        notes.push(trf("Fonts the widgets use that aren't installed: %1", &[&missing.join(", ")]));
    }
    notes
}

/// Есть ли шрифт в системе (`fc-list`).
pub fn live_have_font(family: &str) -> bool {
    let out = std::process::Command::new("fc-list").arg(family).arg("family").output();
    out.map(|o| !o.stdout.is_empty()).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// Подставная машина: файлы в памяти, процессы — `/proc/<pid>/cmdline`.
    #[derive(Default)]
    struct Io {
        files: RefCell<BTreeMap<String, String>>,
        runs: RefCell<Vec<Vec<String>>>,
    }

    impl Io {
        fn put(&self, p: &str, t: &str) {
            self.files.borrow_mut().insert(p.to_string(), t.to_string());
        }
        fn proc(&self, pid: u32, conf: &str) {
            self.put(&format!("/proc/{pid}/cmdline"), &format!("conky\0-c\0{conf}\0"));
            self.put(&format!("/proc/{pid}/comm"), "conky\n");
        }
    }

    impl LookIo for Io {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.runs.borrow_mut().push(argv.to_vec());
            (true, String::new())
        }
        fn have(&self, _: &str) -> bool {
            true
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            self.put(path, text);
            true
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            let pre = format!("{}/", path.trim_end_matches('/'));
            let mut out: Vec<String> = Vec::new();
            for k in self.files.borrow().keys() {
                if let Some(rest) = k.strip_prefix(&pre) {
                    let top = rest.split('/').next().unwrap_or("").to_string();
                    if !top.is_empty() && !out.contains(&top) {
                        out.push(top);
                    }
                }
            }
            out
        }
    }

    fn ctx() -> Ctx {
        Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: "mate".into() }
    }

    #[test]
    fn cli_flag_is_its_own_route() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(crate::app::route(&a(&[CLI_FLAG, "Работа"])), crate::app::Route::ConkySet);
        assert_eq!(crate::app::route(&a(&["--conky-set=Работа"])), crate::app::Route::ConkySet);
    }

    #[test]
    fn cmtheme_is_parsed_like_the_manager_writes_it() {
        let t = "\"~/.conky/Green Apple Desktop/conky_seamod\"\n~/.conky/Green Apple Desktop/Gotham\n\n\
                 ~/.conky/Green Apple Desktop/wallpaper.jpg\nwallpaper-scaling:stretched\n/opt/w/a~b\n";
        let c = parse_cmtheme(t, "/h");
        assert_eq!(
            c.widgets,
            ["/h/.conky/Green Apple Desktop/conky_seamod", "/h/.conky/Green Apple Desktop/Gotham", "/opt/w/a~b"]
        );
        assert_eq!(c.wallpaper.as_deref(), Some("/h/.conky/Green Apple Desktop/wallpaper.jpg"));
        assert_eq!(c.scaling.as_deref(), Some("stretched"));
    }

    #[test]
    fn old_conky_paths_lead_to_the_rexon_folder() {
        let c = ctx();
        let have = |p: &str| p == "/h/.config/conky/Green Apple Desktop/Gotham" || p == "/h/.conky/Seamod/x";
        assert_eq!(
            resolve(&c, "/h/.conky/Green Apple Desktop/Gotham", &have).as_deref(),
            Some("/h/.config/conky/Green Apple Desktop/Gotham")
        );
        assert_eq!(resolve(&c, "/h/.conky/Seamod/x", &have).as_deref(), Some("/h/.conky/Seamod/x"));
        assert_eq!(resolve(&c, "/h/.conky/Nope/x", &have), None);
    }

    #[test]
    fn sets_from_files_are_listed_and_own_sets_win() {
        let io = Io::default();
        let c = ctx();
        io.put("/h/.config/conky/Green/Green.cmtheme", "~/.conky/Green/Gotham\n~/.conky/Green/Gone\n~/.conky/Green/wallpaper.jpg\n");
        io.put("/h/.config/conky/Green/Gotham", "conky.text = [[x]]\n");
        let have = |p: &str| p == "/h/.config/conky/Green/Gotham" || p == "/h/.config/conky/Green/wallpaper.jpg";
        let r = rows(&io, &c, &[], &have);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "Green");
        assert!(!r[0].own && !r[0].empty);
        assert!(r[0].summary.contains("Green"), "{:?}", r[0]);
        assert!(r[0].tooltip.contains('1'), "{:?}", r[0]);
        let set = find(&io, &c, &[], "Green", &have).expect("набор");
        assert_eq!(set.widgets, ["/h/.config/conky/Green/Gotham"]);
        assert_eq!(set.wallpaper.as_deref(), Some("/h/.config/conky/Green/wallpaper.jpg"));
    }

    #[test]
    fn save_needs_a_name_and_running_widgets() {
        let io = Io::default();
        let c = ctx();
        assert!(!save(&io, &c, "  ", None).0);
        assert!(!save(&io, &c, "Work", None).0);
        io.proc(10, "/h/.config/conky/Clock/clock.conf");
        let (ok, msg) = save(&io, &c, "Work", Some("/w.png".into()));
        assert!(ok, "{msg}");
        let st = load(&io, &c);
        assert_eq!(st.sets[0].widgets, ["/h/.config/conky/Clock/clock.conf"]);
        assert_eq!(st.applied.as_deref(), Some("Work"));
        assert!(save(&io, &c, "Work", None).0);
        assert_eq!(load(&io, &c).sets.len(), 1);
        assert!(remove(&io, &c, "Work").0);
        assert!(!remove(&io, &c, "Work").0);
        assert_eq!(load(&io, &c).applied, None);
    }

    /// Применение гасит только виджеты прошлого набора, которых нет в новом;
    /// чужой conky человека остаётся.
    #[test]
    fn apply_stops_only_the_previous_set() {
        let io = Io::default();
        let c = ctx();
        let clock = "/h/.config/conky/Clock/clock.conf";
        let cpu = "/h/.config/conky/Cpu/cpu.conf";
        let net = "/h/.config/conky/Net/net.conf";
        for f in [clock, cpu, net] {
            io.put(f, "conky.text = [[x]]\n");
        }
        io.proc(10, clock);
        io.proc(11, cpu);
        assert!(save(&io, &c, "Work", None).0);
        // Запущен руками уже после сохранения набора — не его виджет.
        io.proc(12, "/home/other/my.conf");
        let mut st = load(&io, &c);
        st.sets.push(ConkySet { name: "Home".into(), widgets: vec![clock.into(), net.into()], ..Default::default() });
        store(&io, &c, &st);
        let set = find(&io, &c, &[], "Home", &|p| io.read(p).is_some()).unwrap();
        let p = plan(&io, &c, &[], set);
        assert_eq!(p.stop, [cpu]);
        assert_eq!(p.start, [net]);
        assert!(p.needs_trust.is_empty());
        let started = RefCell::new(Vec::<String>::new());
        let starter = |_: &str, _: &str, conf: &str| {
            started.borrow_mut().push(conf.to_string());
            (true, String::new())
        };
        let done = apply(&io, &c, &[], &p, false, &all_ready, &starter);
        assert_eq!(*started.borrow(), [net]);
        let kills: Vec<Vec<String>> = io.runs.borrow().iter().filter(|a| a[0] == "kill").cloned().collect();
        assert_eq!(kills, [vec!["kill".to_string(), "--".to_string(), "11".to_string()]], "{}", done.message);
        assert_eq!(load(&io, &c).applied.as_deref(), Some("Home"));
    }

    /// Виджет с командами без разрешения: окно спрашивает один раз,
    /// командная строка пропускает.
    #[test]
    fn untrusted_widget_is_asked_once_or_skipped() {
        let io = Io::default();
        let c = ctx();
        let w = "/h/.config/conky/Weather/w.conf";
        io.put(w, "conky.text = [[${execi 600 curl -s wttr.in}]]\n");
        let mut st = load(&io, &c);
        st.sets.push(ConkySet { name: "S".into(), widgets: vec![w.into()], ..Default::default() });
        store(&io, &c, &st);
        let set = find(&io, &c, &[], "S", &|p| io.read(p).is_some()).unwrap();
        let p = plan(&io, &c, &[], set);
        assert_eq!(p.needs_trust, ["Weather"]);
        let (title, text) = apply_question(&io, &c, &p).expect("вопрос");
        assert!(title.contains('S') && text.contains("curl"), "{title}\n{text}");
        let started = RefCell::new(0);
        let starter = |_: &str, _: &str, _: &str| {
            *started.borrow_mut() += 1;
            (true, String::new())
        };
        let asked = RefCell::new(Vec::<String>::new());
        let ready = |names: &[String]| {
            asked.borrow_mut().extend(names.iter().cloned());
            names.iter().map(|_| Ok(None)).collect()
        };
        let done = apply(&io, &c, &[], &p, false, &ready, &starter);
        assert_eq!(*started.borrow(), 0, "{}", done.message);
        assert!(asked.borrow().is_empty(), "без разрешения пакеты не ставятся");
        let done = apply(&io, &c, &[], &p, true, &ready, &starter);
        assert_eq!(*started.borrow(), 1, "{}", done.message);
        assert_eq!(*asked.borrow(), ["Weather"]);
    }

    fn all_ready(names: &[String]) -> Vec<crate::conky_safety::Readiness> {
        names.iter().map(|_| Ok(None)).collect()
    }

    /// «Всё или ничего», как у «▶️ Запустить»: виджет набора, которому не
    /// хватает программ, не запускается, и итог говорит какой и почему;
    /// остальные виджеты набора запускаются, примечание о недостающих
    /// файлах дописывается к итогу. Готовность спрашивается один раз за
    /// весь набор.
    #[test]
    fn widget_without_programs_is_not_started_by_the_set() {
        let io = Io::default();
        let c = ctx();
        let jhalak = "/h/.config/conky/Jhalak/Jhalak.conf";
        let clock = "/h/.config/conky/Clock/clock.conf";
        for f in [jhalak, clock] {
            io.put(f, "conky.text = [[x]]\n");
        }
        let mut st = load(&io, &c);
        st.sets.push(ConkySet { name: "S".into(), widgets: vec![jhalak.into(), clock.into()], ..Default::default() });
        store(&io, &c, &st);
        let set = find(&io, &c, &[], "S", &|p| io.read(p).is_some()).unwrap();
        let p = plan(&io, &c, &[], set);
        let calls = RefCell::new(0);
        let ready = |names: &[String]| {
            *calls.borrow_mut() += 1;
            assert_eq!(names, ["Jhalak", "Clock"]);
            vec![Err("Jhalak: нет cava".to_string()), Ok(Some("нет ./a.lua".to_string()))]
        };
        let started = RefCell::new(Vec::<String>::new());
        let starter = |_: &str, _: &str, conf: &str| {
            started.borrow_mut().push(conf.to_string());
            (true, "запущен".to_string())
        };
        let done = apply(&io, &c, &[], &p, false, &ready, &starter);
        assert_eq!(*calls.borrow(), 1);
        assert_eq!(*started.borrow(), [clock], "{}", done.message);
        assert!(!done.ok);
        assert!(done.message.contains("Jhalak: нет cava") && done.message.contains("нет ./a.lua"), "{}", done.message);
        // Незапущенный виджет не числится в применённом наборе.
        assert_eq!(load(&io, &c).applied_widgets, [clock]);
    }

    // ── Набор в файл .rxn ──────────────────────────────────────────────

    #[test]
    fn export_paths_go_where_the_widget_lands() {
        let t = "lua_load = '/home/al/.conky/Weather/w.lua'\n${execi 60 $HOME/.conky/Weather/a.sh}\n\
                 ${image /home/al/Pictures/x.png} ~/.conky/WeatherX/y\n";
        let out = relocate_text(t, "/home/al", Some("/home/al/.conky/Weather"), "Weather");
        assert_eq!(
            out,
            "lua_load = '~/.config/conky/Weather/w.lua'\n${execi 60 ~/.config/conky/Weather/a.sh}\n\
             ${image ~/Pictures/x.png} ~/.conky/WeatherX/y\n"
        );
        // `/home/al` не задевает `/home/alex`.
        assert_eq!(relocate_text("/home/alex/x", "/home/al", None, "W"), "/home/alex/x");
    }

    #[test]
    fn export_finds_fonts_of_both_syntaxes() {
        let t = "conky.config = {\n  font = 'Roboto Mono:size=10',\n  -- font = 'Old',\n}\n\
                 xftfont DejaVu Sans:size=8\nfonts_dir x\nconky.text = [[${font Ubuntu:bold:size=9}a${font}]]\n";
        assert_eq!(config_fonts(t), ["Roboto Mono", "DejaVu Sans", "Ubuntu"]);
    }

    #[test]
    fn export_names_are_safe() {
        assert_eq!(set_package_id("Мой стол"), "conky-set-moy-stol");
        assert_eq!(safe_component(".conkyrc", "x"), "conkyrc");
        assert_eq!(safe_component("a/b", "x"), "a-b");
        assert_eq!(safe_component("..", "x"), "x");
        assert_eq!(export_path("/h/set"), "/h/set.rxn");
        assert_eq!(export_path("/h/set.RXN"), "/h/set.RXN");
    }

    #[test]
    fn export_lists_personal_data() {
        let env = ExportEnv { user: "wehrwolf".into(), host: "machina".into(), state_home: None };
        let t = "${execi 600 curl 'https://api.openweathermap.org/x?appid=0123456789abcdef0123'}\n\
                 # mail me: a.b@example.org\n${exec echo machina}\n";
        let found = private_lines(t, "W/w.conf", &env);
        assert!(found.iter().any(|l| l.starts_with("W/w.conf:1")), "{found:?}");
        assert!(found.iter().any(|l| l.starts_with("W/w.conf:2")), "{found:?}");
        assert!(found.iter().any(|l| l.starts_with("W/w.conf:3")), "{found:?}");
        assert!(private_lines("conky.text = [[${time}]]\n", "W/w.conf", &env).is_empty());
    }

    const ROUNDTRIP_BASE_ENV: &str = "REXON_TEST_CONKY_SET_BASE";

    /// Сквозная проверка: набор, сохранённый в одном доме, ставится тем же
    /// кодом «Установить из файла…» в другой (подменённый HOME дочернего
    /// прогона) и даёт ту же раскладку; удаление снимает своё.
    #[test]
    fn exported_set_installs_into_another_home() {
        let base = std::env::temp_dir().join(format!("rexon_test_conky_set_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("b");
        std::fs::create_dir_all(&home).unwrap();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "conky_sets::tests::roundtrip_child", "--ignored", "--test-threads=1", "--nocapture"])
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("LANG", "C.UTF-8")
            .env(ROUNDTRIP_BASE_ENV, &base)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&base);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "дочерний прогон не прошёл:\n{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn put(p: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    /// Все файлы дерева: относительный путь → содержимое.
    fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    out.insert(p.strip_prefix(root).unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(root, root, &mut out);
        out
    }

    #[test]
    #[ignore = "запускается дочерним процессом exported_set_installs_into_another_home"]
    fn roundtrip_child() {
        let base = PathBuf::from(std::env::var_os(ROUNDTRIP_BASE_ENV).expect("только дочерний прогон"));
        let io = crate::animations::SystemIo;
        // ── Дом A: откуда сохраняют ──
        let a = base.join("a");
        let a_s = a.to_string_lossy().into_owned();
        let ctx_a = Ctx { home: a_s.clone(), xdg_config_home: Some(format!("{a_s}/.config")), wayland: false, de: String::new() };
        let state_a = format!("{a_s}/.local/state");
        let clock = a.join(".config/conky/Clock");
        let clock_conf = clock.join("clock.conf");
        put(
            &clock_conf,
            format!(
                "conky.config = {{\n  alignment = 'top_right',\n  font = 'Roboto:size=10',\n  lua_load = '{a_s}/.config/conky/Clock/rings.lua',\n}}\n\
                 conky.text = [[${{execi 60 cava -h}}${{image {a_s}/.config/conky/Clock/bg.png}}]]\n"
            )
            .as_bytes(),
        );
        put(&clock.join("rings.lua"), b"function conky_main() end\n");
        put(&clock.join("bg.png"), b"\x89PNG\0\0binary");
        put(&clock.join("fonts/Fake.ttf"), b"ttf\0");
        put(&clock.join(".git/config"), b"secret history");
        put(&clock.join(crate::ext_manager::OWNER_MARK), b"other\n");
        let weather = a.join(".conky/Weather");
        let weather_conf = weather.join("weather.conf");
        put(&weather_conf, b"conky.config = { lua_load = '~/.conky/Weather/w.lua' }\nconky.text = [[${execi 600 ~/scripts/w.sh}]]\n");
        put(&weather.join("w.lua"), b"-- w\n");
        put(&a.join("scripts/w.sh"), b"#!/bin/sh\necho 1\n");
        let wp = a.join("Pictures/my.png");
        put(&wp, b"\x89PNG\r\n\x1a\nwp");
        let clock_s = clock_conf.to_string_lossy().into_owned();
        let weather_s = weather_conf.to_string_lossy().into_owned();
        // Положение и экран часов, автозапуск часов.
        let ov = crate::conky_overlay::Overrides { gap_x: Some(40), xinerama_head: Some(1), ..Default::default() };
        let pa = crate::conky_overlay::paths(&ctx_a, Some(&state_a), &clock_s);
        put(Path::new(&pa.settings), serde_json::to_string(&ov).unwrap().as_bytes());
        put(
            Path::new(&crate::de_apply::conky_autostart_path(&ctx_a, "Clock")),
            crate::ext_manager::conky_desktop_entry_with("Clock", &clock_s, None, None, 25, &[]).as_bytes(),
        );
        let env = ExportEnv { user: "nobody-special".into(), host: String::new(), state_home: Some(state_a.clone()) };
        let set = ConkySet {
            name: "Мой стол".into(),
            widgets: vec![clock_s.clone(), weather_s.clone(), format!("{a_s}/.config/conky/Gone/x.conf")],
            wallpaper: Some(wp.to_string_lossy().into_owned()),
            file: None,
        };
        let rxn = base.join("out/Мой стол.rxn");
        std::fs::create_dir_all(rxn.parent().unwrap()).unwrap();
        let rxn_s = rxn.to_string_lossy().into_owned();
        let done = export(&io, &ctx_a, &[], &env, &set, &rxn_s, false).expect("экспорт");
        assert_eq!((done.widgets, done.missing, done.wallpaper), (2, 1, true), "{done:?}");
        assert_eq!(done.left_out, ["~/scripts/w.sh"], "{done:?}");
        assert!(export_message(&done).contains(&rxn_s));

        // Личное: ключ погоды — отказ со списком, с согласием — сохраняется.
        let secret = base.join("a/.config/conky/Secret/s.conf");
        put(&secret, b"conky.text = [[${execi 600 curl 'https://x.org/?appid=0123456789abcdef0123'}]]\n");
        let mut leaky = set.clone();
        leaky.widgets.push(secret.to_string_lossy().into_owned());
        let leak_rxn = base.join("out/leak.rxn").to_string_lossy().into_owned();
        match export(&io, &ctx_a, &[], &env, &leaky, &leak_rxn, false) {
            Err(ExportError::Private(found)) => assert!(found.iter().any(|l| l.starts_with("Secret/s.conf:1")), "{found:?}"),
            other => panic!("личное не замечено: {other:?}"),
        }
        assert!(!Path::new(&leak_rxn).exists());
        assert!(export(&io, &ctx_a, &[], &env, &leaky, &leak_rxn, true).is_ok());

        // ── Дом B: куда ставят (HOME процесса) ──
        let b = PathBuf::from(std::env::var("HOME").unwrap());
        let b_s = b.to_string_lossy().into_owned();
        let ctx_b = Ctx { home: b_s.clone(), xdg_config_home: Some(format!("{b_s}/.config")), wayland: false, de: String::new() };
        let installed = crate::ext_exec::install_from_file(&rxn_s).expect("установка");
        eprintln!("установлено: {} {:?}", installed.name, installed.notes);
        let id = set_package_id("Мой стол");
        let conky = b.join(".config/conky");
        let got_clock = tree(&conky.join("Clock"));
        let got_weather = tree(&conky.join("Weather"));
        let names: Vec<&str> = got_clock.keys().map(String::as_str).collect();
        assert_eq!(names, [crate::ext_manager::OWNER_MARK, "bg.png", "clock.conf", "fonts/Fake.ttf", "rings.lua"], "{names:?}");
        assert_eq!(got_clock["bg.png"], b"\x89PNG\0\0binary");
        let conf_b = String::from_utf8(got_clock["clock.conf"].clone()).unwrap();
        assert!(conf_b.contains("lua_load = '~/.config/conky/Clock/rings.lua'"), "{conf_b}");
        assert!(conf_b.contains("${image ~/.config/conky/Clock/bg.png}"), "{conf_b}");
        let weather_b = String::from_utf8(got_weather["weather.conf"].clone()).unwrap();
        assert!(weather_b.contains("lua_load = '~/.config/conky/Weather/w.lua'"), "{weather_b}");
        assert!(got_weather.contains_key("w.lua"));
        // Ни одного пути дома A ни в одном файле.
        for (f, bytes) in got_clock.iter().chain(got_weather.iter()) {
            assert!(!String::from_utf8_lossy(bytes).contains(&a_s), "путь дома A в {f}");
        }
        // Набор — своей папкой, без копий виджетов внутри.
        let set_dir: Vec<String> = tree(&conky.join(&id)).into_keys().collect();
        assert_eq!(set_dir, ["wallpaper.png", "Мой стол.cmtheme"], "{set_dir:?}");
        assert!(!conky.join("Secret").exists() && !conky.join("fonts").exists());
        assert!(b.join(format!(".local/share/fonts/rexon-conky-{id}/Fake.ttf")).is_file());
        // «Наборы виджетов» дома B видят набор с путями дома B.
        let sets = file_sets(&io, &ctx_b, &[], &live_exists);
        let (got, missing) = sets.iter().find(|(s, _)| s.name == "Мой стол").expect("набор не виден");
        assert_eq!(*missing, 0);
        assert_eq!(got.widgets, [format!("{b_s}/.config/conky/Clock/clock.conf"), format!("{b_s}/.config/conky/Weather/weather.conf")]);
        assert_eq!(got.wallpaper.as_deref(), Some(format!("{b_s}/.config/conky/{id}/wallpaper.png").as_str()));
        // Положение часов — накладкой дома B; автозапуск не включён, но сказано.
        let pb = crate::conky_overlay::paths(&ctx_b, Some(&format!("{b_s}/.local/state")), &got.widgets[0]);
        assert_eq!(crate::conky_overlay::load(&io, &pb), ov);
        assert!(!Path::new(&crate::de_apply::conky_autostart_path(&ctx_b, "Clock")).exists());
        assert!(installed.notes.iter().any(|n| n.contains("Widget sets")), "{:?}", installed.notes);
        assert!(installed.notes.iter().any(|n| n.contains("Start at login")), "{:?}", installed.notes);

        // Удаление пакета снимает своё.
        crate::ext_exec::uninstall(&id).expect("удаление");
        assert!(!conky.join("Clock").exists() && !conky.join("Weather").exists() && !conky.join(&id).exists());
        assert!(!b.join(format!(".local/share/fonts/rexon-conky-{id}")).exists());
        assert!(!Path::new(&pb.settings).exists());
    }
}
