//! «Удалить» у установленной темы — с плитки раздела и из обзоров диска.
//!
//! ЗАЧЕМ. Прежде удалить можно было только купленное в магазине (по журналу
//! установленных, `appearance_installer::uninstall_from_log`): тема,
//! поставленная руками, чужим установщиком или распакованная в `~/.icons`,
//! оставалась навсегда. В KDE удаляется любая тема пользователя (строки
//! паритета 0.8, 0.9 и «Удаление» каждого раздела), причём не сразу: плитка
//! помечается, внизу всплывает строка с «Вернуть», и снимается тема, когда
//! строка закрылась.
//!
//! Правила (решает [`RemovalRoots::verdict`] по пути из скана —
//! `ThemeItem::install_path`, признак «включена» — `local_kind == "active"`):
//!   * включённую сейчас тему удалить нельзя — кнопка недоступна с подсказкой
//!     ([`removal_active_reason`]); при снятии проверяется ещё раз по живой
//!     карте включённого, во всех видах, которые читают этот каталог (одна
//!     папка в `~/.themes` бывает и темой GTK, и рамкой xfwm4). Исключение —
//!     запущенный свой виджет Conky: после вопроса он гаснет и уходит в
//!     корзину ([`Verdict::RunningConky`]), как найденный на диске;
//!   * своя тема — прямой потомок каталога тем пользователя (`~/.local/share/…`,
//!     `~/.icons`, `~/.themes`, `~/.config/Kvantum` и каталоги разделов
//!     окружений) — удаляется отложенно ([`RemovalQueue`]);
//!   * экран входа и заставка загрузки, которые поставил сам Rexon (путь есть в
//!     журнале установленных), — прежним «Удалить» по журналу
//!     (`LiveInstaller::uninstall`, помощник от root; коммит a135f89). Окно
//!     пароля на истечении всплывающей строки было бы сюрпризом, поэтому
//!     здесь без отсрочки;
//!   * экран входа и заставка загрузки без пакета-владельца, положенные руками
//!     или чужим установщиком, — тем же помощником от root по пути
//!     (`LiveInstaller::remove_loose_system_theme`), тоже без отсрочки;
//!   * прочее системное (`/usr/share/…`) — кнопка недоступна с подсказкой
//!     ([`removal_system_reason`]): файлы принадлежат пакету;
//!   * у плитки без пути (Qt-стиль из `QStyleFactory`) кнопки нет вовсе.
//!
//! Отложенное удаление ничего не трогает на диске, пока строка «Вернуть»
//! открыта: [`RemovalQueue`] только помнит пути, а страница прячет их плитки
//! ([`RemovalQueue::retain_visible`]). Снимает [`commit`], когда строка
//! закрылась или истекла (и на выходе из программы). Упала программа до
//! этого — тема осталась на месте: потерять тему хуже, чем не удалить.

use std::collections::HashSet;
use std::path::Path;

use serde_json::{Map, Value};

use crate::i18n::{ntr, tr, trf};
use crate::installed_scan::{de_types, sidecar_preview_path, types_for};
use crate::models::ThemeItem;

/// Сколько всплывающая строка с «Вернуть» висит, прежде чем тема снимется.
pub const UNDO_TIMEOUT_MS: i32 = 8000;

/// Что делать с кнопкой «Удалить» у плитки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Кнопки нет: у плитки нет пути на диске.
    Hidden,
    /// Своя тема: отложенное удаление через [`RemovalQueue`], без вопроса,
    /// со строкой «Вернуть» — как у всех разделов «Оформления». `path` — то,
    /// что реально лежит на диске (у цветовой схемы — файл `.colors`). Свой
    /// невключённый виджет Conky идёт тем же путём, но в корзину ([`commit`]).
    Local { path: String },
    /// Системная тема, которую поставил Rexon: прежнее «Удалить» по журналу
    /// (`LiveInstaller::uninstall(content_id)`), сразу, с окном пароля.
    Journal { content_id: String },
    /// Системная тема без пакета-владельца и без записи журнала (положена
    /// руками): снимает помощник от root по пути, сразу, с окном пароля.
    Loose { path: String },
    /// Виджет Conky, найденный на диске вне каталога Rexon: после вопроса
    /// (`conky_scan::found_removal_questions`) гаснет и уходит в корзину
    /// (`conky_scan::live_remove_found`), сразу, без строки «Вернуть» —
    /// возвращается он из корзины.
    FoundConky { path: String },
    /// Свой виджет Conky (папка в каталоге Rexon), который сейчас запущен:
    /// после вопроса (`conky_scan::running_removal_question`) гаснет, теряет
    /// автозапуск и уходит в корзину ([`live_remove_running_conky`]), сразу —
    /// как найденный на диске.
    RunningConky { path: String },
    /// Кнопка видна, но недоступна; `reason` — подсказка на ней.
    Blocked { reason: String },
}

impl Verdict {
    /// Видна ли кнопка.
    pub fn visible(&self) -> bool {
        !matches!(self, Verdict::Hidden)
    }
    /// Можно ли её нажать.
    pub fn enabled(&self) -> bool {
        matches!(
            self,
            Verdict::Local { .. } | Verdict::Journal { .. } | Verdict::Loose { .. } | Verdict::FoundConky { .. } | Verdict::RunningConky { .. }
        )
    }
    /// Подсказка на недоступной кнопке (у доступной — пусто).
    pub fn tooltip(&self) -> String {
        match self {
            Verdict::Blocked { reason } => reason.clone(),
            _ => String::new(),
        }
    }
}

// ── Подписи ────────────────────────────────────────────────────────────

/// Подсказка на «Удалить» у включённой сейчас темы.
pub fn removal_active_reason() -> String {
    tr("You can't remove the active theme. Switch to another theme first.")
}

/// Подсказка на «Удалить» у системной темы.
pub fn removal_system_reason() -> String {
    tr("This theme came with the system (it is in /usr/share), so Rexon doesn't remove it. \
        It goes away together with its package.")
}

/// Подсказка на «Удалить» у темы вне каталогов тем пользователя.
pub fn removal_outside_reason() -> String {
    tr("This theme is outside your theme folders, so Rexon doesn't remove it.")
}

/// Текст всплывающей строки: что помечено к удалению. `names` — имена тем в
/// порядке пометки.
pub fn removal_toast_text(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => trf("Theme “%1” removed.", &[one]),
        many => ntr("%n theme removed.", "%n themes removed.", many.len() as u64),
    }
}

/// Текст строки «Вернуть», пока помеченное ещё на диске: тема БУДЕТ
/// удалена. Прошедшее время («удалена») здесь обманывало: человек решал, что
/// тема уже пропала, и «Вернуть» не искал. Срок — отдельной строкой
/// обратного отсчёта ([`removal_countdown_text`]).
pub fn removal_pending_text(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => trf("Theme “%1” will be removed.", &[one]),
        many => ntr("%n theme will be removed.", "%n themes will be removed.", many.len() as u64),
    }
}

/// Обратный отсчёт строки «Вернуть»: через сколько секунд снимется
/// помеченное (страница считает от [`UNDO_TIMEOUT_MS`] и держит паузу, пока
/// над строкой курсор).
pub fn removal_countdown_text(seconds: u64) -> String {
    // Самостоятельное предложение: строка встаёт сразу за «Тема … будет
    // удалена.», и прежнее «Через 6 секунд, если…» читалось обрывком.
    ntr(
        "%n second left — “Undo” cancels the removal.",
        "%n seconds left — “Undo” cancels the removal.",
        seconds,
    )
}

/// Кнопка всплывающей строки: отменить удаление.
pub fn removal_undo_btn() -> String {
    tr("Undo")
}

/// Тема оказалась включённой к моменту снятия.
fn removal_now_active(name: &str) -> String {
    trf("“%1” is in use now, so it wasn't removed.", &[&name])
}

// ── Каталоги тем ───────────────────────────────────────────────────────

/// Каталоги тем пользователя (с видами, которые в них ищут) и системные
/// каталоги — из тех же таблиц, что у сканов (`installed_scan::types_for`,
/// `de_types`, прежние `~/.icons` и `~/.themes` из `section_types`): нового
/// списка мест здесь нет (правило #12).
pub struct RemovalRoots {
    /// (каталог пользователя, вид темы).
    user: Vec<(String, String)>,
    /// Системные каталоги тем и корень usr.
    system: Vec<String>,
    /// Каталоги тем экрана входа и заставки загрузки (снимает помощник).
    helper: Vec<String>,
}

impl RemovalRoots {
    pub fn new(home: &str, usr_root: &str) -> Self {
        let home = home.trim_end_matches('/');
        let usr_root = usr_root.trim_end_matches('/');
        let mut user: Vec<(String, String)> = Vec::new();
        let mut system: Vec<String> = vec![usr_root.to_string()];
        let mut helper: Vec<String> = Vec::new();
        fn push(user: &mut Vec<(String, String)>, dir: &str, kind: &str) {
            let dir = dir.trim_end_matches('/');
            if !dir.is_empty() && !user.iter().any(|(d, k)| d == dir && k == kind) {
                user.push((dir.to_string(), kind.to_string()));
            }
        }
        let types = types_for(home, usr_root).into_iter().chain(de_types(home, usr_root, ""));
        for t in types {
            push(&mut user, &t.user_dir, t.apply_kind);
            if !t.sys_dir.is_empty() {
                let sys = t.sys_dir.trim_end_matches('/').to_string();
                if t.apply_kind == "sddm" || t.apply_kind == "plymouth" {
                    helper.push(sys.clone());
                }
                if !system.contains(&sys) {
                    system.push(sys);
                }
            }
            // Темы рамок и оболочек ищутся и в `~/.local/share/themes`
            // (`installed_scan::de_types`).
            if t.entry == "sub" {
                push(&mut user, &format!("{home}/.local/share/themes"), t.apply_kind);
                push(&mut user, &format!("{home}/.themes"), t.apply_kind);
            }
        }
        // Прежние каталоги, которые читают и KDE, и GTK.
        for kind in ["icons", "cursor"] {
            push(&mut user, &format!("{home}/.icons"), kind);
        }
        push(&mut user, &format!("{home}/.themes"), "gtk");
        // Заставка и экран блокировки KDE — части пакетов look-and-feel.
        for kind in ["splash", "lockscreen"] {
            push(&mut user, &format!("{home}/.local/share/plasma/look-and-feel"), kind);
        }
        RemovalRoots { user, system, helper }
    }

    /// Виды тем, которые ищут в каталоге `dir` (пусто — не каталог тем
    /// пользователя).
    fn kinds_at(&self, dir: &str) -> Vec<&str> {
        self.user.iter().filter(|(d, _)| d == dir).map(|(_, k)| k.as_str()).collect()
    }

    /// Решение по плитке скана.
    ///
    /// `install_path` — `ThemeItem::install_path`; у цветовой схемы скан
    /// отдаёт путь без `.colors` — файл находится здесь. `active` — плитка
    /// помечена включённой. `journal` — журнал установленных
    /// (`ocs_store::parse_installed_log`). `loose` — системные темы без
    /// пакета-владельца (`LiveScan::loose_system_themes`).
    pub fn verdict(
        &self,
        install_path: &str,
        active: bool,
        journal: &Map<String, Value>,
        loose: &HashSet<String>,
    ) -> Verdict {
        let path = install_path.trim_end_matches('/');
        if path.is_empty() {
            return Verdict::Hidden;
        }
        if active {
            return Verdict::Blocked { reason: removal_active_reason() };
        }
        let target = disk_target(path);
        let Some((parent, name)) = target.rsplit_once('/') else {
            return Verdict::Blocked { reason: removal_outside_reason() };
        };
        if (safe_name(name) && !self.kinds_at(parent).is_empty()) || !self.nested_kinds(&target).is_empty() {
            return if present(&target) {
                Verdict::Local { path: target }
            } else {
                Verdict::Hidden // темы уже нет — нечего удалять
            };
        }
        if self.helper.iter().any(|d| d == parent) {
            if let Some(content_id) = journal_owner(journal, &target) {
                return Verdict::Journal { content_id };
            }
            if safe_name(name) && loose.contains(&target) {
                return Verdict::Loose { path: target };
            }
        }
        if self.system.iter().any(|d| target == *d || target.starts_with(&format!("{d}/"))) {
            return Verdict::Blocked { reason: removal_system_reason() };
        }
        Verdict::Blocked { reason: removal_outside_reason() }
    }

    /// Виды, чья часть темы — `path` вида `<каталог тем>/<имя>/<подпапка>`
    /// (рамка `Arc/xfwm4` внутри темы GTK, [`frame_part_target`]); пусто — не
    /// такая часть.
    fn nested_kinds(&self, path: &str) -> Vec<&str> {
        let Some((theme, sub)) = path.rsplit_once('/') else { return Vec::new() };
        let Some((dir, name)) = theme.rsplit_once('/') else { return Vec::new() };
        if !safe_name(name) || !safe_name(sub) {
            return Vec::new();
        }
        self.kinds_at(dir)
            .into_iter()
            .filter(|k| part_subdir(k) == Some(sub))
            .collect()
    }

    /// Включена ли сейчас тема по пути `path` хоть в одном виде, который
    /// ищет её в этом каталоге. `active_map` — `LiveScan::active_map`.
    /// У части темы (`Arc/xfwm4`) — только в видах этой части.
    pub fn in_use(&self, path: &str, active_map: &std::collections::HashMap<String, String>) -> bool {
        let nested = self.nested_kinds(path);
        if !nested.is_empty() {
            let name = path.rsplit_once('/').and_then(|(t, _)| t.rsplit_once('/')).map(|(_, n)| n).unwrap_or("");
            return nested
                .into_iter()
                .any(|kind| crate::installed_scan::active_names(active_map, kind).iter().any(|n| *n == name));
        }
        let Some((parent, file)) = path.rsplit_once('/') else { return false };
        let name = file.strip_suffix(".colors").unwrap_or(file);
        self.kinds_at(parent).into_iter().any(|kind| {
            crate::installed_scan::active_names(active_map, kind)
                .iter()
                .any(|n| *n == name || *n == file)
        })
    }
}

/// Подпапка части темы у вида `kind` (`xfwm4`, `metacity-1`, `gnome-shell`,
/// `cinnamon`); `None` — вид не часть папки темы.
fn part_subdir(kind: &str) -> Option<&'static str> {
    match crate::appearance_installer::de_theme_marker(kind) {
        Some(crate::appearance_installer::DeThemeMarker::Subdir(sub)) => Some(sub),
        _ => None,
    }
}

/// Что снимает «Удалить» у плитки вида `kind` с папкой `install_path`.
///
/// Решение 3в Wehrwolfmann 20.10.2026: у рамки окон (и оболочки), которая
/// лежит подпапкой в папке темы GTK (`Arc/xfwm4` рядом с `Arc/gtk-3.0`), —
/// только эта подпапка: тема GTK остаётся. Папка, где кроме подпапки рамки
/// других папок нет, — отдельная рамка: снимается целиком, как раньше.
/// `subdirs` — имена папок внутри `install_path`.
pub fn frame_part_target(install_path: &str, kind: &str, subdirs: &[String]) -> String {
    let path = install_path.trim_end_matches('/');
    match part_subdir(kind) {
        Some(sub) if subdirs.iter().any(|d| d == sub) && subdirs.iter().any(|d| d != sub && !d.starts_with('.')) => {
            format!("{path}/{sub}")
        }
        _ => path.to_string(),
    }
}

/// Части, поставленные в папку `path` ОТДЕЛЬНО (другой записью журнала
/// установленных, не той, что владеет самой папкой): рамка окон, легшая
/// подпапкой в тему GTK. `own_id` — запись, которую снимают (её пути не в
/// счёт). Отдаёт `(запись, имя)`.
pub fn separate_parts_inside(journal: &Map<String, Value>, path: &str, own_id: Option<&str>) -> Vec<(String, String)> {
    let path = path.trim_end_matches('/');
    let under = format!("{path}/");
    let paths_of = |rec: &Value| -> Vec<String> {
        rec["paths"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.trim_end_matches('/').to_string())).collect())
            .unwrap_or_default()
    };
    journal
        .iter()
        .filter(|(id, _)| Some(id.as_str()) != own_id)
        .filter(|(_, rec)| {
            let paths = paths_of(rec);
            !paths.iter().any(|p| p == path) && paths.iter().any(|p| p.starts_with(&under))
        })
        .map(|(id, rec)| (id.clone(), rec["name"].as_str().unwrap_or(id).to_string()))
        .collect()
}

/// Вопрос перед удалением папки темы `theme`, в которой лежат отдельно
/// поставленные части `parts` ([`separate_parts_inside`]): (заголовок, текст).
/// `None` — таких частей нет, спрашивать не о чем.
pub fn separate_parts_question(theme: &str, parts: &[(String, String)]) -> Option<(String, String)> {
    if parts.is_empty() {
        return None;
    }
    let names: Vec<&str> = parts.iter().map(|(_, n)| n.as_str()).collect();
    let title = tr("Remove the theme together with its window borders?");
    // TRANSLATORS: %1 is the theme name, %2 the names of window border themes installed into its folder.
    let text = trf("The folder of “%1” also holds “%2”, installed separately (window borders or a shell \
                    theme). Removing the theme removes them too.",
        &[&theme, &names.join("», «")],
    );
    Some((title, text))
}

/// Имя элемента каталога, которое безопасно снимать.
fn safe_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/')
}

/// Есть ли что-то по пути (ссылку не разыменовываем).
fn present(p: &str) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

/// Что реально лежит на диске под путём скана: сам путь, а у цветовой схемы
/// (скан отдаёт путь без окончания) — файл `<путь>.colors`.
fn disk_target(path: &str) -> String {
    if !present(path) {
        let colors = format!("{path}.colors");
        if Path::new(&colors).is_file() {
            return colors;
        }
    }
    path.to_string()
}

/// Запись журнала установленных, в путях которой есть `path`.
fn journal_owner(journal: &Map<String, Value>, path: &str) -> Option<String> {
    journal.iter().find_map(|(id, rec)| {
        rec["paths"]
            .as_array()?
            .iter()
            .any(|p| p.as_str().map(|s| s.trim_end_matches('/')) == Some(path))
            .then(|| id.clone())
    })
}

// ── Отложенное удаление ────────────────────────────────────────────────

/// Тема, помеченная к удалению.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRemoval {
    /// Что снимать (`Verdict::Local::path`).
    pub path: String,
    /// Имя для человека (`ThemeItem::name`).
    pub name: String,
    /// Номер плитки (`ThemeItem::id`) — чтобы убрать сайдкар-превью.
    pub item_id: String,
}

/// Помеченные к удалению темы, пока открыта строка «Вернуть».
#[derive(Debug, Default)]
pub struct RemovalQueue {
    pending: Vec<PendingRemoval>,
}

impl RemovalQueue {
    /// Пометить тему к удалению (повторная пометка того же пути — без
    /// изменений). Отдаёт текст всплывающей строки.
    pub fn mark(&mut self, path: &str, name: &str, item_id: &str) -> String {
        if !self.pending.iter().any(|p| p.path == path) {
            self.pending.push(PendingRemoval {
                path: path.to_string(),
                name: name.to_string(),
                item_id: item_id.to_string(),
            });
        }
        self.toast_text()
    }

    /// «Вернуть»: снять все пометки. Отдаёт, сколько тем вернулось.
    pub fn undo_all(&mut self) -> usize {
        std::mem::take(&mut self.pending).len()
    }

    /// Строка закрылась или истекла: отдать помеченное на снятие.
    pub fn take_all(&mut self) -> Vec<PendingRemoval> {
        std::mem::take(&mut self.pending)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Текст всплывающей строки для текущих пометок.
    pub fn toast_text(&self) -> String {
        let names: Vec<String> = self.pending.iter().map(|p| p.name.clone()).collect();
        removal_pending_text(&names)
    }

    /// Помечена ли плитка с путём скана `install_path`.
    pub fn is_pending(&self, install_path: &str) -> bool {
        let path = install_path.trim_end_matches('/');
        let colors = format!("{path}.colors");
        self.pending.iter().any(|p| p.path == path || p.path == colors)
    }

    /// Убрать из ответа скана плитки помеченных тем.
    pub fn retain_visible(&self, items: &mut Vec<ThemeItem>) {
        if self.pending.is_empty() {
            return;
        }
        items.retain(|it| it.install_path.is_empty() || !self.is_pending(&it.install_path));
    }
}

/// Снять помеченные темы — `(всё снялось, сообщение для строки состояния)`.
///
/// Перед снятием каждая тема проверяется заново: путь всё ещё прямой потомок
/// каталога тем пользователя, и тема не включена ни в одном виде
/// (`in_use`, по живой карте включённого). Снятые пути уходят и из журнала
/// установленных (`log_path`): запись, у которой на диске ничего не
/// осталось, забывается — плитка магазина перестаёт звать тему
/// установленной. Сайдкар-превью снимается заодно.
///
/// Виджет Conky (плитка `local:conky:…`) не стирается, а уходит в корзину
/// `home_trash` ([`crate::utils::move_to_trash`]) — как у запущенного и
/// найденного на диске (`conky_scan::stop_and_trash`): у виджета бывают
/// правки человека, и вернуть его должно быть можно и после строки «Вернуть».
pub fn commit(
    pending: Vec<PendingRemoval>,
    roots: &RemovalRoots,
    in_use: &dyn Fn(&str) -> bool,
    home: &str,
    log_path: &str,
    home_trash: &Path,
) -> (bool, String) {
    let empty = Map::new();
    let mut removed_names: Vec<String> = Vec::new();
    let mut trashed_names: Vec<String> = Vec::new();
    let date = crate::utils::trash_deletion_date_now();
    let mut removed_paths: HashSet<String> = HashSet::new();
    let mut problems: Vec<String> = Vec::new();
    // Покупки магазина в общем журнале установки (шаг 3г слияния
    // установщиков): записанную тему снимает журнал — вместе с её ссылкой
    // `~/.themes`; тему, положенную не магазином, — прежним путём.
    let journal = crate::install_journal::store_view(log_path);
    for p in pending {
        match roots.verdict(&p.path, false, &empty, &HashSet::new()) {
            Verdict::Local { .. } => {}
            Verdict::Hidden => {
                // Уже нет на диске — снимать нечего, но журнал поправить.
                removed_paths.insert(p.path.clone());
                removed_names.push(p.name);
                continue;
            }
            other => {
                problems.push(format!("{}: {}", p.name, other.tooltip()));
                continue;
            }
        }
        if in_use(&p.path) {
            problems.push(removal_now_active(&p.name));
            continue;
        }
        if p.item_id.starts_with("local:conky:") {
            match crate::utils::move_to_trash(Path::new(&p.path), home_trash, &date) {
                Ok(_) => {
                    remove_sidecar(home, &p.item_id);
                    removed_paths.insert(p.path.clone());
                    trashed_names.push(p.name);
                }
                // TRANSLATORS: %1 is a Conky widget name, %2 the system error.
                Err(e) => problems.push(trf(
                    "Couldn't move “%1” to the trash: %2. The widget stays where it was.",
                    &[&p.name, &e],
                )),
            }
            continue;
        }
        let res = match journal_part(&journal, &p.path) {
            Some(steps) => remove_steps(&steps),
            None => remove_on_disk(&p.path),
        };
        match res {
            Ok(()) => {
                remove_sidecar(home, &p.item_id);
                tidy_after_removal(home, &p.path);
                removed_paths.insert(p.path.clone());
                removed_names.push(p.name);
            }
            // TRANSLATORS: %1 is a file or folder path, %2 is the system error.
            Err(e) => problems.push(trf("Couldn't remove %1: %2.", &[&p.path, &e.to_string()])),
        }
    }
    let mut message = removal_toast_text(&removed_names);
    for name in &trashed_names {
        if !message.is_empty() {
            message.push(' ');
        }
        // TRANSLATORS: %1 is a Conky widget name.
        message.push_str(&trf("🗑️ “%1” moved to the trash", &[name]));
    }
    if !removed_paths.is_empty() {
        if let Err(e) = forget_in_journal(log_path, &removed_paths) {
            if !message.is_empty() {
                message.push(' ');
            }
            message.push_str(&trf(
                "⚠️ The installed-themes log wasn't updated (%1) — the theme will stay \
                 in the list until the next successful write.",
                &[&e],
            ));
        }
    }
    for problem in &problems {
        if !message.is_empty() {
            message.push(' ');
        }
        message.push_str(problem);
    }
    (problems.is_empty(), message)
}

/// Шаги снятия пути `path` по записи покупки общего журнала, в которой он
/// записан ([`crate::install_journal::part_removal`]): сам путь и его ссылка
/// `~/.themes`. `None` — путь не из магазина.
///
/// Плитка — то, что человек видит на диске и сам велел удалить: если журнал
/// сам путь оставил бы (на месте теперь не то, что клали), снимается, как
/// прежде, то, что лежит на диске.
fn journal_part(journal: &crate::install_journal::Journal, path: &str) -> Option<Vec<crate::ext_manager::ExtStep>> {
    use crate::install_journal::{part_removal, probe_live, removal_path, Source};
    let r = journal
        .items
        .values()
        .filter(|e| e.source == Source::Store)
        .find_map(|e| part_removal(e, path, &probe_live))?;
    let path = path.trim_end_matches('/');
    let own = r.steps.iter().any(|st| removal_path(st).is_some_and(|q| q.trim_end_matches('/') == path));
    (own || std::fs::symlink_metadata(path).is_err()).then_some(r.steps)
}

/// Исполнить файловые шаги снятия; первая ошибка — отказ.
fn remove_steps(steps: &[crate::ext_manager::ExtStep]) -> std::io::Result<()> {
    use crate::ext_manager::ExtStep;
    for step in steps {
        match step {
            ExtStep::RemoveTree { path, .. } => remove_on_disk(path)?,
            ExtStep::RemoveFile { path } => remove_on_disk(path)?,
            _ => {}
        }
    }
    Ok(())
}

/// Снять то, что лежит по пути (каталог — целиком, ссылку — саму); нет — и
/// снимать нечего.
fn remove_on_disk(path: &str) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Убрать то, что тема оставляет рядом с собой, после того как снят её
/// путь `removed`. ОДНО поведение у обоих удалений: «Удалить» на плитке
/// ([`commit`]) и удаления по журналу покупки
/// (`appearance_installer::uninstall_from_log`) — прежде журнал оставлял
/// висячую ссылку `~/.themes/<тема>` (дефект Д4, 25.10.2026).
///
/// * тема из `~/.local/share/themes` — её ссылка в `~/.themes`
///   ([`crate::installed_scan::drop_theme_link`]; чужую ссылку и тему не трогает);
/// * спайс Cinnamon — переводы, настройки, у действия Nemo — его каталог.
///
/// С шага 3г слияния установщиков ссылку темы ПОКУПКИ снимает общий журнал
/// установки (она записана в покупке, [`crate::install_journal::Placed::of`]),
/// и удаление по журналу покупки зовёт отсюда только уборку спайса. Целиком
/// уборка остаётся у плитки: тема в `~/.local/share/themes`, положенная не
/// магазином (руками, другим установщиком), в журнале не записана, а ссылку
/// ей `sync_theme_links` ставит всё равно; переводы и настройки спайса в
/// журнал не пишутся вовсе.
pub fn tidy_after_removal(home: &str, removed: &str) {
    crate::installed_scan::drop_theme_link(home, removed);
    crate::cinnamon_spices::cleanup_after_removal(&crate::cinnamon_spices::SpiceEnv::for_home(home), removed);
}

/// Снять сайдкар-превью плитки `local:<вид>:<имя>` (кеш картинки темы без
/// своего превью). Нет файла — нечего делать.
fn remove_sidecar(home: &str, item_id: &str) {
    let Some((kind, name)) = item_id.strip_prefix("local:").and_then(|r| r.split_once(':')) else {
        return;
    };
    let _ = std::fs::remove_file(sidecar_preview_path(home, kind, name));
}

/// Убрать снятые пути из журнала установленных; запись без единого пути на
/// диске забыть. Записей нет или журнал не менялся — ничего не писать.
/// Пишется общий журнал установки, `ocs_installed.json` — его зеркало (шаг
/// 5б слияния установщиков).
fn forget_in_journal(log_path: &str, removed: &HashSet<String>) -> Result<(), String> {
    let mut data = crate::install_journal::store_log_view(log_path);
    let mut changed = false;
    let mut forget: Vec<String> = Vec::new();
    for (id, rec) in data.iter_mut() {
        let Some(paths) = rec.get_mut("paths").and_then(Value::as_array_mut) else { continue };
        let before = paths.len();
        // Снятая папка темы уносит и части, лёгшие в неё подпапками.
        paths.retain(|v| v.as_str().map(|s| !removed_covers(removed, s)).unwrap_or(true));
        if paths.len() != before {
            changed = true;
            let left = paths.iter().filter_map(Value::as_str).any(present);
            if !left {
                forget.push(id.clone());
            }
        }
    }
    if !changed {
        return Ok(());
    }
    for id in forget {
        crate::ocs_store::forget_installed(&mut data, &id);
    }
    crate::install_journal::put_store_log(log_path, &data).map_err(|e| e.to_string())
}

/// Снят ли путь `p` — сам или вместе с папкой, в которой лежал.
fn removed_covers(removed: &HashSet<String>, p: &str) -> bool {
    let p = p.trim_end_matches('/');
    removed.contains(p) || removed.iter().any(|r| p.starts_with(&format!("{r}/")))
}

/// Плитка встроенной рамки KDE ([`crate::kdecoration`]), которую не
/// удаляют: всё, что лежит не в доме человека.
fn builtin_decoration_without_removal(item: &ThemeItem, home: &str) -> bool {
    let builtin = item
        .id
        .strip_prefix("local:aurorae:")
        .and_then(crate::kdecoration::parse_entry)
        .is_some();
    let home = format!("{}/", home.trim_end_matches('/'));
    builtin && !item.install_path.starts_with(&home)
}

// ── Живые обёртки ──────────────────────────────────────────────────────

fn live_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
}

/// Журнал установленных этой машины.
pub fn live_log_path() -> String {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty());
    crate::ocs_store::install_log_path_xdg(xdg.as_deref(), &live_home())
}

/// Решения по плиткам одного скана: каталоги и журнал читаются один раз.
pub struct LiveRemoval {
    roots: RemovalRoots,
    journal: Map<String, Value>,
    /// Системные темы без пакета-владельца; спрашивается у пакетной базы
    /// один раз и только когда попалась плитка из их каталогов.
    loose: std::cell::OnceCell<HashSet<String>>,
}

impl LiveRemoval {
    pub fn new() -> Self {
        LiveRemoval {
            roots: RemovalRoots::new(&live_home(), "/usr"),
            journal: crate::install_journal::store_log_view(&live_log_path()),
            loose: std::cell::OnceCell::new(),
        }
    }

    /// Решение по плитке.
    pub fn verdict(&self, item: &ThemeItem) -> Verdict {
        // Виджет Conky «Найдено на диске» (имя — путь): свои правила, и
        // запущенный снимается тоже — сначала гаснет.
        let found = item.id.strip_prefix("local:conky:").is_some_and(crate::conky_scan::is_found_name);
        if found {
            return match crate::conky_scan::live_found_removal(&item.install_path) {
                Ok(Some(r)) => Verdict::FoundConky { path: r.path },
                Ok(None) => Verdict::Hidden,
                Err(reason) => Verdict::Blocked { reason },
            };
        }
        // Встроенная рамка KDE (`local:aurorae:<модуль>/<тема>`): модуль Qt
        // (Breeze, Oxygen) и пакет темы QML из системы приходят с Plasma, у
        // KDE кнопки удаления у них нет — и у нас тоже.
        if builtin_decoration_without_removal(item, &live_home()) {
            return Verdict::Hidden;
        }
        let own_conky = item.id.starts_with("local:conky:");
        let active = item.local_kind.as_deref() == Some("active");
        if own_conky && active {
            let base = self.roots.verdict(&item.install_path, false, &self.journal, &HashSet::new());
            return running_conky_verdict(base);
        }
        let none = HashSet::new();
        let path = item.install_path.trim_end_matches('/');
        let in_helper_dir = path
            .rsplit_once('/')
            .is_some_and(|(parent, _)| self.roots.helper.iter().any(|d| d == parent));
        let loose = if in_helper_dir {
            self.loose.get_or_init(|| {
                crate::installed_scan::LiveScan::new(Vec::new(), Default::default()).loose_system_themes()
            })
        } else {
            &none
        };
        // Рамка, лёгшая подпапкой в тему GTK: снимается только подпапка.
        let kind = item.id.strip_prefix("local:").and_then(|r| r.split_once(':')).map(|(k, _)| k).unwrap_or("");
        let target = if part_subdir(kind).is_some() {
            let subdirs: Vec<String> = std::fs::read_dir(path)
                .map(|rd| {
                    rd.flatten()
                        .filter(|e| e.path().is_dir())
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            frame_part_target(path, kind, &subdirs)
        } else {
            path.to_string()
        };
        self.roots.verdict(&target, active, &self.journal, loose)
    }

    /// Вопрос перед снятием папки `path` (решение «Удалить» плитки), если в
    /// ней лежат отдельно поставленные части ([`separate_parts_question`]).
    pub fn separate_parts_question(&self, path: &str, theme: &str) -> Option<(String, String)> {
        separate_parts_question(theme, &separate_parts_inside(&self.journal, path, None))
    }
}

/// Вопрос перед «Удалить» покупки `content_id` по журналу: в её папках
/// лежат отдельно поставленные части ([`separate_parts_question`]).
pub fn live_uninstall_question(content_id: &str) -> Option<(String, String)> {
    let journal = crate::install_journal::store_log_view(&live_log_path());
    let rec = journal.get(content_id)?;
    let theme = rec["name"].as_str().unwrap_or(content_id).to_string();
    let mut parts: Vec<(String, String)> = Vec::new();
    for p in rec["paths"].as_array()?.iter().filter_map(Value::as_str) {
        for part in separate_parts_inside(&journal, p, Some(content_id)) {
            if !parts.contains(&part) {
                parts.push(part);
            }
        }
    }
    separate_parts_question(&theme, &parts)
}

/// Свой виджет Conky запущен: то, что было бы обычным «Удалить» своей темы
/// (`base` — решение как для невключённой), становится «остановить и
/// удалить»; прочие решения (кнопки нет, недоступна) — как есть.
fn running_conky_verdict(base: Verdict) -> Verdict {
    match base {
        Verdict::Local { path } => Verdict::RunningConky { path },
        other => other,
    }
}

/// Снять свой запущенный виджет Conky `path` (плитка `item_id`): его
/// процессы гаснут по номерам, запись автозапуска уходит, папка — в корзину
/// (`conky_scan::live_remove_running_own`); затем, как у [`commit`], путь
/// уходит из журнала установленных и снимается сайдкар-превью.
pub fn live_remove_running_conky(path: &str, item_id: &str) -> (bool, String) {
    let (ok, msg) = crate::conky_scan::live_remove_running_own(path);
    if ok {
        remove_sidecar(&live_home(), item_id);
        let gone: HashSet<String> = std::iter::once(path.trim_end_matches('/').to_string()).collect();
        if let Err(e) = forget_in_journal(&live_log_path(), &gone) {
            crate::log_warn!("theme", "журнал установленных не обновлён после снятия виджета: {e}");
        }
    }
    (ok, msg)
}

impl Default for LiveRemoval {
    fn default() -> Self {
        Self::new()
    }
}

/// Снять помеченное на этой машине. `de` — рабочий стол вкладки (ключ Rexon):
/// по нему карта включённого знает разделы окружения. Читает настройки
/// окружения (может звать `gsettings`) — не из потока интерфейса.
pub fn live_commit(pending: Vec<PendingRemoval>, de: &str) -> (bool, String) {
    if pending.is_empty() {
        return (true, String::new());
    }
    let home = live_home();
    let roots = RemovalRoots::new(&home, "/usr");
    let mut scan = crate::installed_scan::LiveScan::new(Vec::new(), Default::default()).with_de(de);
    // Экран входа к удалению своих тем отношения не имеет — `systemctl` не зовём.
    scan.plasma_login = Some(false);
    let active = scan.active_map();
    let paths: Vec<String> = pending.iter().map(|p| p.path.clone()).collect();
    let xdg_data = std::env::var("XDG_DATA_HOME").ok();
    let trash = crate::utils::home_trash_dir(xdg_data.as_deref(), &home);
    let outcome = commit(pending, &roots, &|p| roots.in_use(p, &active), &home, &live_log_path(), Path::new(&trash));
    // Тема из файла Windows стояла ещё и расширением — запись снимаем вместе
    // с ней, иначе она остаётся в каталоге расширений навсегда.
    let gone: Vec<String> = paths.into_iter().filter(|p| !present(p)).collect();
    // Снятый виджет Conky из каталога Rexon: его процессы гаснут (по номерам),
    // запись автозапуска уходит — иначе при входе conky звал бы файл, которого
    // нет. Не виджет — ничего не делается.
    let ctx = crate::de_apply::Ctx::live();
    for p in &gone {
        crate::de_apply::conky_forget_widget(&crate::animations::SystemIo, &ctx, p);
    }
    for id in crate::ext_exec::forget_extensions_of_removed_themes(&gone) {
        crate::log_info!("theme", "вместе с темой снята запись расширения: {id}");
    }
    outcome
}

// ── «Удалить» у товара магазина ─────────────────────────────────────────

/// Пути покупки `content_id` на диске: из журнала установленных (KDE Store,
/// Cinnamon Spices; прежние записи `github:…` — так же). У спайса Cinnamon — ещё и его каталог: тема,
/// поставленная окном настроек Cinnamon, в журнале не числится, а «Удалить»
/// в витрине у неё есть.
pub fn store_paths(journal: &Map<String, Value>, content_id: &str, home: &str) -> Vec<String> {
    let mut out: Vec<String> = journal
        .get(content_id)
        .and_then(|r| r["paths"].as_array())
        .map(|a| a.iter().filter_map(Value::as_str).map(|p| p.trim_end_matches('/').to_string()).collect())
        .unwrap_or_default();
    if let Some((t, key)) = crate::cinnamon_spices::parse_item_id(content_id) {
        let dir = format!("{}/{key}", crate::cinnamon_spices::install_root(&crate::cinnamon_spices::SpiceEnv::for_home(home), t));
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// «Удалить» у товара магазина (кнопка деталей): включённую сейчас тему
/// снять нельзя — та же проверка, что у плитки сетки установленного
/// ([`RemovalRoots::in_use`]: тема GTK, рамки xfwm4 и Marco, оболочки GNOME
/// и Cinnamon, значки, курсоры, цвета…), по путям покупки. `Some(причина)` —
/// кнопка недоступна; `None` — удалять можно.
pub fn store_blocked(paths: &[String], in_use: &dyn Fn(&str) -> bool) -> Option<String> {
    paths.iter().any(|p| in_use(p)).then(removal_active_reason)
}

/// [`store_blocked`] на этой машине для покупки `content_id`. `de` — рабочий
/// стол вкладки (как у [`live_commit`]). Читает настройки окружения (может
/// звать `gsettings`) — не из потока интерфейса.
pub fn live_store_blocked(content_id: &str, de: &str) -> Option<String> {
    let home = live_home();
    let journal = crate::install_journal::store_log_view(&live_log_path());
    let paths: Vec<String> = store_paths(&journal, content_id, &home).into_iter().filter(|p| present(p)).collect();
    if paths.is_empty() {
        return None;
    }
    let roots = RemovalRoots::new(&home, "/usr");
    let mut scan = crate::installed_scan::LiveScan::new(Vec::new(), Default::default()).with_de(de);
    scan.plasma_login = Some(false);
    let active = scan.active_map();
    store_blocked(&paths, &|p| roots.in_use(p, &active))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Временный HOME и подставной /usr — живой профиль не трогается.
    fn sandbox(tag: &str) -> (PathBuf, String, String) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir()
            .join(format!("rexon_removal_{tag}_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let home = root.join("home").to_string_lossy().into_owned();
        let usr = root.join("usr").to_string_lossy().into_owned();
        (root, home, usr)
    }

    fn mkdir(p: &str) {
        std::fs::create_dir_all(p).unwrap();
    }

    fn touch(p: &str) {
        mkdir(Path::new(p).parent().unwrap().to_str().unwrap());
        std::fs::write(p, "x").unwrap();
    }

    fn item(id: &str, name: &str, path: &str, active: bool) -> ThemeItem {
        ThemeItem {
            id: id.to_string(),
            name: name.to_string(),
            install_path: path.to_string(),
            local_kind: active.then(|| "active".to_string()),
            ..Default::default()
        }
    }

    /// Свой виджет Conky: не запущен — обычное отложенное «Удалить»;
    /// запущен — не отказ, а «остановить и удалить»; недоступное и
    /// отсутствующее остаются как были.
    #[test]
    fn running_own_conky_is_stop_and_remove_not_refusal() {
        let (_root, home, usr) = sandbox("conky-running");
        let roots = RemovalRoots::new(&home, &usr);
        let journal = Map::new();
        let w = format!("{home}/.config/conky/Jhalak");
        touch(&format!("{w}/Jhalak.conf"));
        let idle = roots.verdict(&w, false, &journal, &HashSet::new());
        assert_eq!(idle, Verdict::Local { path: w.clone() });
        let running = running_conky_verdict(roots.verdict(&w, false, &journal, &HashSet::new()));
        assert_eq!(running, Verdict::RunningConky { path: w.clone() });
        assert!(running.enabled() && running.visible() && running.tooltip().is_empty());
        let blocked = Verdict::Blocked { reason: removal_outside_reason() };
        assert_eq!(running_conky_verdict(blocked.clone()), blocked);
        assert_eq!(running_conky_verdict(Verdict::Hidden), Verdict::Hidden);
    }

    #[test]
    fn own_themes_in_every_user_folder_are_removable() {
        let (_root, home, usr) = sandbox("own");
        let roots = RemovalRoots::new(&home, &usr);
        let journal = Map::new();
        for dir in [
            format!("{home}/.local/share/icons/Papirus"),
            format!("{home}/.icons/Bibata"),
            format!("{home}/.themes/Arc"),
            format!("{home}/.local/share/themes/Arc-Dark"),
            format!("{home}/.local/share/plasma/look-and-feel/org.x.desktop"),
            format!("{home}/.local/share/plasma/desktoptheme/Sweet"),
            format!("{home}/.local/share/aurorae/themes/Win7"),
            format!("{home}/.local/share/sounds/ocean"),
            format!("{home}/.config/Kvantum/KvArc"),
        ] {
            mkdir(&dir);
            assert_eq!(roots.verdict(&dir, false, &journal, &HashSet::new()), Verdict::Local { path: dir.clone() }, "{dir}");
        }
        // Цветовая схема: скан отдаёт путь без окончания, снимается файл.
        let colors = format!("{home}/.local/share/color-schemes/Nord");
        touch(&format!("{colors}.colors"));
        assert_eq!(
            roots.verdict(&colors, false, &journal, &HashSet::new()),
            Verdict::Local { path: format!("{colors}.colors") }
        );
    }

    #[test]
    fn active_system_and_outside_are_blocked_with_reason() {
        let (root, home, usr) = sandbox("blocked");
        let roots = RemovalRoots::new(&home, &usr);
        let journal = Map::new();
        let own = format!("{home}/.local/share/icons/Papirus");
        mkdir(&own);
        let active = roots.verdict(&own, true, &journal, &HashSet::new());
        assert_eq!(active, Verdict::Blocked { reason: removal_active_reason() });
        assert!(active.visible() && !active.enabled());
        assert_eq!(active.tooltip(), removal_active_reason());

        let sys = format!("{usr}/share/icons/breeze");
        mkdir(&sys);
        assert_eq!(roots.verdict(&sys, false, &journal, &HashSet::new()), Verdict::Blocked { reason: removal_system_reason() });
        // Plymouth не из журнала — тоже системная, помощник не зовём.
        let ply = format!("{usr}/share/plymouth/themes/spinner");
        mkdir(&ply);
        assert_eq!(roots.verdict(&ply, false, &journal, &HashSet::new()), Verdict::Blocked { reason: removal_system_reason() });

        let outside = root.join("elsewhere/Theme").to_string_lossy().into_owned();
        mkdir(&outside);
        assert_eq!(roots.verdict(&outside, false, &journal, &HashSet::new()), Verdict::Blocked { reason: removal_outside_reason() });
        // Вложенный глубже каталог темы — не тема каталога тем.
        let nested = format!("{home}/.local/share/icons/Papirus/scalable");
        mkdir(&nested);
        assert_eq!(roots.verdict(&nested, false, &journal, &HashSet::new()), Verdict::Blocked { reason: removal_outside_reason() });
        // Сам каталог тем — тоже нет.
        let base = format!("{home}/.local/share/icons");
        assert!(!roots.verdict(&base, false, &journal, &HashSet::new()).enabled());
        // Плитка без пути (Qt-стиль) — без кнопки.
        assert_eq!(roots.verdict("", false, &journal, &HashSet::new()), Verdict::Hidden);
    }

    #[test]
    fn rexon_installed_plymouth_goes_through_journal() {
        let (_root, home, usr) = sandbox("journal");
        let roots = RemovalRoots::new(&home, &usr);
        let ply = format!("{usr}/share/plymouth/themes/neon");
        mkdir(&ply);
        let journal: Map<String, Value> =
            serde_json::from_value(serde_json::json!({"777": {"name": "Neon", "paths": [ply.clone()]}})).unwrap();
        assert_eq!(roots.verdict(&ply, false, &journal, &HashSet::new()), Verdict::Journal { content_id: "777".into() });
        // Тема без записи журнала и без пакета-владельца — помощником по пути;
        // включённую сейчас не снимаем и её.
        let hand = format!("{usr}/share/plymouth/themes/by-hand");
        mkdir(&hand);
        let loose: HashSet<String> = [hand.clone()].into();
        assert_eq!(roots.verdict(&hand, false, &journal, &loose), Verdict::Loose { path: hand.clone() });
        assert!(!roots.verdict(&hand, true, &journal, &loose).enabled());
        assert_eq!(
            roots.verdict(&hand, false, &journal, &HashSet::new()),
            Verdict::Blocked { reason: removal_system_reason() }
        );
        // Включённую — не трогаем и через журнал.
        assert!(!roots.verdict(&ply, true, &journal, &HashSet::new()).enabled());
    }

    #[test]
    fn queue_marks_hides_and_undoes() {
        let mut q = RemovalQueue::default();
        let mut items = vec![
            item("local:icons:A", "A", "/h/.local/share/icons/A", false),
            item("local:colors:Nord", "Nord", "/h/.local/share/color-schemes/Nord", false),
            item("local:widgetstyle:Fusion", "Fusion", "", false),
        ];
        assert_eq!(q.mark("/h/.local/share/icons/A", "A", "local:icons:A"), removal_pending_text(&["A".into()]));
        let text = q.mark("/h/.local/share/color-schemes/Nord.colors", "Nord", "local:colors:Nord");
        assert_eq!(text, removal_pending_text(&["A".into(), "Nord".into()]));
        // Повторная пометка ничего не добавляет.
        q.mark("/h/.local/share/icons/A", "A", "local:icons:A");
        q.retain_visible(&mut items);
        assert_eq!(items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["local:widgetstyle:Fusion"]);
        assert_eq!(q.undo_all(), 2);
        assert!(q.is_empty() && !q.is_pending("/h/.local/share/icons/A"));
    }

    #[test]
    fn commit_removes_only_unused_own_themes_and_cleans_journal() {
        let (_root, home, usr) = sandbox("commit");
        let roots = RemovalRoots::new(&home, &usr);
        let icons = format!("{home}/.local/share/icons/Papirus");
        touch(&format!("{icons}/index.theme"));
        let gtk = format!("{home}/.themes/Arc");
        touch(&format!("{gtk}/gtk-3.0/gtk.css"));
        let colors = format!("{home}/.local/share/color-schemes/Nord.colors");
        touch(&colors);
        // Ссылка: снимается сама ссылка, цель остаётся.
        let target = format!("{home}/keep/Bibata");
        touch(&format!("{target}/index.theme"));
        let link = format!("{home}/.icons/Bibata");
        mkdir(&format!("{home}/.icons"));
        std::os::unix::fs::symlink(&target, &link).unwrap();
        // Сайдкар-превью цветовой схемы.
        let side = sidecar_preview_path(&home, "colors", "Nord");
        touch(&side);

        let log = format!("{home}/.config/rexon/ocs_installed.json");
        touch(&log);
        std::fs::write(
            &log,
            serde_json::json!({
                "1": {"name": "Papirus", "paths": [icons.clone()]},
                "2": {"name": "Pack", "paths": [colors.clone(), format!("{home}/.local/share/plasma/desktoptheme/Keep")]},
            })
            .to_string(),
        )
        .unwrap();
        mkdir(&format!("{home}/.local/share/plasma/desktoptheme/Keep"));

        // Arc стал темой рамок xfwm4 в той же папке — она включена.
        let mut active = HashMap::new();
        active.insert("xfwm4".to_string(), "Arc".to_string());
        let kinds = roots.kinds_at(&format!("{home}/.themes"));
        assert!(kinds.contains(&"gtk"), "{kinds:?}");
        let pend = |path: &str, name: &str, id: &str| PendingRemoval {
            path: path.into(),
            name: name.into(),
            item_id: id.into(),
        };
        let in_use = |p: &str| p == gtk || roots.in_use(p, &active);
        let (ok, msg) = commit(
            vec![
                pend(&icons, "Papirus", "local:icons:Papirus"),
                pend(&gtk, "Arc", "local:gtk:Arc"),
                pend(&colors, "Nord", "local:colors:Nord"),
                pend(&link, "Bibata", "local:cursor:Bibata"),
            ],
            &roots,
            &in_use,
            &home,
            &log,
            Path::new(&format!("{home}/.local/share/Trash")),
        );
        assert!(!ok, "{msg}");
        assert!(msg.contains(&removal_now_active("Arc")), "{msg}");
        assert!(msg.starts_with(&removal_toast_text(&["Papirus".into(), "Nord".into(), "Bibata".into()])), "{msg}");
        assert!(!present(&icons) && !present(&colors) && !present(&link) && !present(&side));
        assert!(present(&gtk), "включённая тема осталась");
        assert!(present(&target), "цель ссылки не тронута");

        let data = crate::ocs_store::parse_installed_log(std::fs::read_to_string(&log).ok().as_deref());
        assert!(data.get("1").is_none(), "запись без путей забыта");
        assert_eq!(
            data["2"]["paths"],
            serde_json::json!([format!("{home}/.local/share/plasma/desktoptheme/Keep")]),
            "у набора остался живой путь"
        );
    }

    #[test]
    fn commit_refuses_path_that_left_user_folders() {
        let (_root, home, usr) = sandbox("refuse");
        let roots = RemovalRoots::new(&home, &usr);
        let sys = format!("{usr}/share/icons/breeze");
        mkdir(&sys);
        let (ok, msg) = commit(
            vec![PendingRemoval { path: sys.clone(), name: "Breeze".into(), item_id: String::new() }],
            &roots,
            &|_| false,
            &home,
            &format!("{home}/nolog.json"),
            Path::new(&format!("{home}/.local/share/Trash")),
        );
        assert!(!ok && msg.contains(&removal_system_reason()), "{msg}");
        assert!(present(&sys));
    }

    /// Решение 3в: рамка, лёгшая подпапкой в тему GTK, удаляется одна —
    /// тема GTK остаётся; тема GTK с отдельно поставленной рамкой внутри
    /// спрашивает и уносит запись рамки из журнала.
    #[test]
    fn frame_inside_gtk_theme_is_removed_alone() {
        let (_root, home, usr) = sandbox("frame-part");
        let roots = RemovalRoots::new(&home, &usr);
        let theme = format!("{home}/.local/share/themes/Arc");
        touch(&format!("{theme}/gtk-3.0/gtk.css"));
        touch(&format!("{theme}/metacity-1/metacity-theme-3.xml"));
        let subdirs = vec!["gtk-3.0".to_string(), "metacity-1".to_string()];
        let part = frame_part_target(&theme, "marco", &subdirs);
        assert_eq!(part, format!("{theme}/metacity-1"));
        // Отдельная рамка и сама тема GTK — папка целиком.
        assert_eq!(frame_part_target(&theme, "marco", &["metacity-1".to_string()]), theme);
        assert_eq!(frame_part_target(&theme, "gtk", &subdirs), theme);

        let journal: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "10": {"name": "Arc GTK", "paths": [theme.clone()]},
            "11": {"name": "Arc Marco", "paths": [part.clone()]},
        }))
        .unwrap();
        assert_eq!(roots.verdict(&part, false, &journal, &HashSet::new()), Verdict::Local { path: part.clone() });
        // Включена тема GTK, а не рамка: рамку снять можно.
        let mut active = HashMap::new();
        active.insert("gtk".to_string(), "Arc".to_string());
        assert!(!roots.in_use(&part, &active));
        assert!(roots.in_use(&theme, &active));
        active.insert("marco".to_string(), "Arc".to_string());
        assert!(roots.in_use(&part, &active));

        // Вопрос: у темы GTK внутри отдельная рамка; у самой рамки — нет.
        let inside = separate_parts_inside(&journal, &theme, None);
        assert_eq!(inside, [("11".to_string(), "Arc Marco".to_string())]);
        assert!(separate_parts_question("Arc", &inside).is_some_and(|(_, t)| t.contains("Arc Marco")));
        assert!(separate_parts_inside(&journal, &part, None).is_empty());
        assert!(separate_parts_inside(&journal, &theme, Some("11")).is_empty());

        let log = format!("{home}/.config/rexon/ocs_installed.json");
        touch(&log);
        std::fs::write(&log, Value::Object(journal).to_string()).unwrap();
        let pend = |path: &str, id: &str| PendingRemoval { path: path.into(), name: "Arc".into(), item_id: id.into() };
        let (ok, msg) = commit(vec![pend(&part, "local:marco:Arc")], &roots, &|_| false, &home, &log, Path::new(&format!("{home}/.local/share/Trash")));
        assert!(ok, "{msg}");
        assert!(!present(&part) && present(&format!("{theme}/gtk-3.0/gtk.css")), "снята только рамка");
        let data = crate::ocs_store::parse_installed_log(std::fs::read_to_string(&log).ok().as_deref());
        assert!(data.get("11").is_none() && data.get("10").is_some());

        // Тема GTK целиком уносит и запись рамки, лежавшей внутри.
        touch(&format!("{theme}/metacity-1/metacity-theme-3.xml"));
        std::fs::write(
            &log,
            serde_json::json!({"10": {"name": "Arc GTK", "paths": [theme.clone()]}, "11": {"name": "Arc Marco", "paths": [part.clone()]}})
                .to_string(),
        )
        .unwrap();
        let (ok, msg) = commit(vec![pend(&theme, "local:gtk:Arc")], &roots, &|_| false, &home, &log, Path::new(&format!("{home}/.local/share/Trash")));
        assert!(ok, "{msg}");
        assert!(!present(&theme));
        let data = crate::ocs_store::parse_installed_log(std::fs::read_to_string(&log).ok().as_deref());
        assert!(data.is_empty(), "{data:?}");
    }

    /// Шаг 3г: «Удалить» на плитке темы GTK из магазина снимает её по общему
    /// журналу установки — тему и её ссылку `~/.themes`, а цвета той же
    /// покупки остаются (и путь в журнале магазина). Тема, положенная не
    /// магазином, снимается прежним путём — её ссылку снимает уборка
    /// (`drop_theme_link`: журнал её не знает).
    #[test]
    fn store_gtk_tile_removal_goes_by_journal_with_its_link() {
        let (_root, home, usr) = sandbox("journal-tile");
        let roots = RemovalRoots::new(&home, &usr);
        let xdg = format!("{home}/.local/share/themes");
        touch(&format!("{xdg}/Nord/gtk-3.0/gtk.css"));
        touch(&format!("{xdg}/Hand/gtk-3.0/gtk.css"));
        let colors = format!("{home}/.local/share/color-schemes/Nord.colors");
        touch(&colors);
        assert_eq!(crate::installed_scan::sync_theme_links(&home).len(), 2);
        let log = format!("{home}/.config/rexon/ocs_installed.json");
        touch(&log);
        let theme = format!("{xdg}/Nord");
        std::fs::write(&log, serde_json::json!({"10": {"name": "Nord", "rexon_type": "gtk", "paths": [theme.clone(), colors.clone()]}}).to_string()).unwrap();
        let journal = crate::install_journal::store_view(&log);
        assert!(journal_part(&journal, &theme).is_some_and(|st| st.len() == 2), "тема и ссылка — по журналу");
        assert!(journal_part(&journal, &format!("{xdg}/Hand")).is_none(), "тема не из магазина — не по журналу");

        let pend = |path: &str, id: &str| PendingRemoval { path: path.into(), name: "x".into(), item_id: id.into() };
        let trash = format!("{home}/.local/share/Trash");
        let (ok, msg) = commit(
            vec![pend(&theme, "local:gtk:Nord"), pend(&format!("{xdg}/Hand"), "local:gtk:Hand")],
            &roots,
            &|_| false,
            &home,
            &log,
            Path::new(&trash),
        );
        assert!(ok, "{msg}");
        for name in ["Nord", "Hand"] {
            assert!(!present(&format!("{xdg}/{name}")), "{name}: тема осталась");
            assert!(std::fs::symlink_metadata(format!("{home}/.themes/{name}")).is_err(), "{name}: ссылка ~/.themes осталась");
        }
        assert!(present(&colors), "снята другая часть покупки");
        let data = crate::ocs_store::parse_installed_log(std::fs::read_to_string(&log).ok().as_deref());
        assert_eq!(data["10"]["paths"], serde_json::json!([colors]), "{data:?}");
    }

    /// Свой невключённый виджет Conky после строки «Вернуть» уходит в
    /// корзину freedesktop (файл и запись `.trashinfo`), а не стирается, —
    /// как запущенный и найденный на диске; тема рядом снимается как прежде.
    #[test]
    fn own_conky_widget_goes_to_trash() {
        let (_root, home, usr) = sandbox("conky-trash");
        let roots = RemovalRoots::new(&home, &usr);
        let widget = format!("{home}/.config/conky/RexonProba");
        touch(&format!("{widget}/conky.conf"));
        let icons = format!("{home}/.local/share/icons/Papirus");
        touch(&format!("{icons}/index.theme"));
        assert_eq!(
            roots.verdict(&widget, false, &Map::new(), &HashSet::new()),
            Verdict::Local { path: widget.clone() }
        );
        let trash = format!("{home}/.local/share/Trash");
        let pend = |path: &str, name: &str, id: &str| PendingRemoval { path: path.into(), name: name.into(), item_id: id.into() };
        let (ok, msg) = commit(
            vec![pend(&widget, "RexonProba", "local:conky:RexonProba"), pend(&icons, "Papirus", "local:icons:Papirus")],
            &roots,
            &|_| false,
            &home,
            &format!("{home}/nolog.json"),
            Path::new(&trash),
        );
        assert!(ok, "{msg}");
        assert!(!present(&widget) && !present(&icons));
        assert!(present(&format!("{trash}/files/RexonProba/conky.conf")), "виджет в корзине");
        let info = std::fs::read_to_string(format!("{trash}/info/RexonProba.trashinfo")).unwrap();
        assert!(info.contains("Path=") && info.contains("RexonProba"), "{info}");
        assert!(!present(&format!("{trash}/files/Papirus")), "тема стирается, как прежде");
        assert!(msg.contains(&trf("🗑️ “%1” moved to the trash", &[&"RexonProba"])), "{msg}");
        assert!(msg.contains(&removal_toast_text(&["Papirus".into()])), "{msg}");
    }

    /// «Удалить» у товара магазина: та же защита включённой темы, что у
    /// плитки, — по путям покупки из журнала, а у спайса Cinnamon и по его
    /// каталогу (поставленного окном настроек Cinnamon в журнале нет).
    #[test]
    fn store_item_removal_is_blocked_while_its_theme_is_on() {
        let roots = RemovalRoots::new("/h", "/u");
        let journal: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "1234": {"name": "Arc", "paths": ["/h/.themes/Arc/"]},
            "spices:theme:CBlack": {"name": "CBlack", "paths": ["/h/.themes/CBlack"]},
        }))
        .unwrap();
        assert_eq!(store_paths(&journal, "1234", "/h"), ["/h/.themes/Arc"]);
        assert_eq!(store_paths(&journal, "spices:theme:CBlack", "/h"), ["/h/.themes/CBlack"], "без повтора");
        assert_eq!(store_paths(&journal, "spices:theme:Mint-Y", "/h"), ["/h/.themes/Mint-Y"], "не из журнала — по каталогу");
        assert!(store_paths(&journal, "999", "/h").is_empty());
        // Каждый вид, что ищет темы в ~/.themes, держит тему: GTK, рамки
        // XFCE и MATE, оболочки GNOME и Cinnamon.
        for kind in ["gtk", "xfwm4", "marco", "gnomeshell", "cinnamontheme"] {
            let active = HashMap::from([(kind.to_string(), "CBlack".to_string())]);
            let paths = store_paths(&journal, "spices:theme:CBlack", "/h");
            assert_eq!(store_blocked(&paths, &|p| roots.in_use(p, &active)), Some(removal_active_reason()), "{kind}");
            let other = store_paths(&journal, "1234", "/h");
            assert_eq!(store_blocked(&other, &|p| roots.in_use(p, &active)), None, "{kind}: чужая тема не держит");
        }
        // Значки из KDE Store — тот же путь.
        let icons: Map<String, Value> =
            serde_json::from_value(serde_json::json!({"7": {"paths": ["/h/.local/share/icons/Papirus"]}})).unwrap();
        let active = HashMap::from([("icons".to_string(), "Papirus".to_string())]);
        assert!(store_blocked(&store_paths(&icons, "7", "/h"), &|p| roots.in_use(p, &active)).is_some());
    }

    #[test]
    fn in_use_checks_every_kind_of_the_folder() {
        let roots = RemovalRoots::new("/h", "/u");
        let mut active = HashMap::new();
        active.insert("cursor".to_string(), "Bibata".to_string());
        active.insert("colors".to_string(), "Nord".to_string());
        // Курсор лежит в каталоге значков — включён как курсор.
        assert!(roots.in_use("/h/.local/share/icons/Bibata", &active));
        assert!(roots.in_use("/h/.icons/Bibata", &active));
        assert!(roots.in_use("/h/.local/share/color-schemes/Nord.colors", &active));
        assert!(!roots.in_use("/h/.local/share/icons/Papirus", &active));
        // Заставка из глобальной темы — тот же пакет look-and-feel.
        active.insert("splash".to_string(), "org.x.desktop".to_string());
        assert!(roots.in_use("/h/.local/share/plasma/look-and-feel/org.x.desktop", &active));
    }

    #[test]
    fn builtin_kde_decorations_have_no_remove_button() {
        let tile = |id: &str, path: &str| ThemeItem { id: id.into(), install_path: path.into(), ..Default::default() };
        let home = "/h";
        let breeze = tile("local:aurorae:org.kde.breeze/Breeze", "/usr/lib/qt6/plugins/org.kde.kdecoration3/org.kde.breeze.so");
        assert!(builtin_decoration_without_removal(&breeze, home));
        let plastik = tile("local:aurorae:org.kde.kwin.aurorae/plastik", "/usr/share/kwin/decorations/plastik");
        assert!(builtin_decoration_without_removal(&plastik, home));
        // Свой пакет темы QML и тема Aurorae SVG — по общим правилам.
        let mine = tile("local:aurorae:org.kde.kwin.aurorae/mine", "/h/.local/share/kwin/decorations/mine");
        assert!(!builtin_decoration_without_removal(&mine, home));
        let svg = tile("local:aurorae:Sweet", "/usr/share/aurorae/themes/Sweet");
        assert!(!builtin_decoration_without_removal(&svg, home));
    }
}
