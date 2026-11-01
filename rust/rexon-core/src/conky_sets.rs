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
    let root = format!("{}/", crate::de_apply::conky_root(ctx));
    if let Some(rest) = conf.strip_prefix(&root) {
        if let Some((top, _)) = rest.split_once('/') {
            if !top.is_empty() {
                return top.to_string();
            }
        }
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
    let widgets = crate::conky_procs::running_confs(io, ctx);
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

pub fn plan(io: &dyn LookIo, ctx: &Ctx, extra: &[String], set: ConkySet) -> Plan {
    let st = load(io, ctx);
    let running = crate::conky_procs::running_confs(io, ctx);
    let prev: Vec<String> = if st.applied.is_some() { st.applied_widgets.clone() } else { Vec::new() };
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
        let pids = crate::conky_procs::pids_where(io, ctx, &|c| p.stop.iter().any(|s| s == c));
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
    let running = crate::conky_procs::running_confs(io, ctx);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

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
        assert_eq!(kills, [vec!["kill".to_string(), "11".to_string()]], "{}", done.message);
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
}
