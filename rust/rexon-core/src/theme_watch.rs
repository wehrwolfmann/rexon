//! Сторож оформления сеанса: GTK-теме не хватает своих значков и курсора.
//!
//! ЗАЧЕМ. Тему меняют не только в Rexon: окно «Внешний вид» MATE, настройки
//! GNOME, Cinnamon, XFCE. Метатема (`index.theme`) называет набор значков
//! (`IconTheme=`) и курсор (`CursorTheme=`), окружение их записывает — а
//! самих наборов в системе нет. GTK молча откатывается на hicolor/Adwaita, и
//! человек видит «тема какая-то не такая» (живой случай 17.10.2026: MATE,
//! WhiteSur-Dark-nord просит WhiteSur-Dark и WhiteSur-cursors, обоих нет).
//!
//! ЧТО ДЕЛАЕТ. Пока Rexon запущен, сторож ждёт записи в файлы, где окружения
//! держат оформление (база dconf, xfconf, kdeglobals, `settings.ini` GTK и
//! соседи — [`watch_targets`]), — через inotify, без опроса. После затишья
//! ([`DEBOUNCE_MS`]) читает текущие тему, значки и курсор (теми же
//! командами, что карта активных тем «Оформления») и, если чего-то нет на
//! диске, действует по порядку (решение Wehrwolfmann 17.10.2026):
//! 1. набор кладётся в домашний каталог без пароля и без сборки (готовый
//!    архив магазина KDE, `themes::dep_home_source`) — скачать,
//!    распаковать, применить молча и сказать одним коротким уведомлением
//!    (настройка [`AUTO_HOME_KEY`]; выключена — вместо этого вопрос);
//! 2. набор есть только системным пакетом или в AUR — уведомление с кнопкой
//!    «Установить» (общий путь установщика `LiveInstaller::apply_dep_items`,
//!    тот же, что у «Применить» в Rexon);
//! 3. набора нет нигде — честное уведомление, «Вернуть стандартные» (если
//!    они стоят) и «Оставить как есть».
//!
//! ЧЕГО НЕ ДЕЛАЕТ.
//! * Не надоедает: об одной теме спрашивает один раз — ответ (любой, в том
//!   числе закрытое уведомление) помнится в настройках Rexon
//!   ([`ASKED_KEY`]), пока тема не сменится.
//! * Не спрашивает после «Применить» в самом Rexon: там вопрос уже задан, и
//!   установщик отмечает тему ([`note_rexon_applied`]).
//! * ⛔ Не пишет в настройки имя набора, которого на диске нет: «стандартные»
//!   предлагаются, только если они сами стоят.
//! * Без `notify-send` кнопок нет (служба уведомлений шлёт нажатие сигналом,
//!   а `dbus-send` его не ловит) — тогда текст с подсказкой, где поставить.
//!
//! Решения — чистые функции ниже; всё, что ходит наружу, — в «живой» части.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::fs_watch::{Wake, Watch, WatchTarget};
use crate::i18n::{tr, trf};
use crate::themes::{DepItem, DepRole, DepState};

/// Ключ настроек Rexon: о какой теме и о каких наборах уже спросили.
pub const ASKED_KEY: &str = "theme_deps_asked";
/// Ключ настроек Rexon: «Докачивать значки и курсоры к теме автоматически».
pub const AUTO_HOME_KEY: &str = "theme_deps_auto_download";
/// Затишье после последней записи, мс: окно «Внешний вид» пишет тему,
/// значки и курсор тремя ключами подряд — спрашивать один раз, о всех.
pub const DEBOUNCE_MS: i32 = 3000;
/// Первый взгляд после запуска, секунды: при входе в сеанс служба
/// уведомлений и окружение поднимаются не сразу.
pub const STARTUP_DELAY_SECS: u64 = 20;
/// Как часто пробовать заново поставить слежку за каталогами, которых при
/// запуске не было (dconf заводит свой при первой записи), мс. Это не опрос
/// настроек: проверяется только, появился ли каталог.
pub const RETRY_WATCH_MS: i32 = 10 * 60 * 1000;

// ── что окружение показывает сейчас ─────────────────────────────────────────

/// Тема, значки и курсор из настроек окружения (пусто — не прочитались).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Looks {
    pub gtk: String,
    pub icons: String,
    pub cursor: String,
}

/// Чего не хватает по одной роли и что с этим можно сделать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    /// Набор и его состояние: `Installable(пакет)` либо `Missing`.
    pub item: DepItem,
    /// Его просит сама тема (`index.theme`).
    pub from_theme: bool,
    /// Стоящий набор, на который можно перейти вместо него («Вернуть
    /// стандартные»). Только когда в настройках записано имя, которого нет:
    /// если записанный набор стоит, менять его незачем.
    pub fallback: Option<String>,
    /// Набор кладётся в домашний каталог без пароля и без сборки
    /// ([`crate::themes::dep_home_source`]).
    pub home: bool,
}

fn same_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Чего не хватает: просьбы темы (`theme_items`, их строит установщик —
/// `LiveInstaller::theme_deps_plan`) и наборы, записанные в настройках
/// (`setting_items` — те же [`DepItem`] для текущих имён). `standard` —
/// стоящий на диске стандартный набор роли окружения.
///
/// По каждой роли — не больше одного пункта:
/// * в настройках записано имя, которого нет, — пункт о нём (если тема
///   просит другой набор, который можно поставить, — о наборе темы);
///   запасной — набор темы, если он стоит, иначе стандартный;
/// * записанный набор стоит, а тема просит недостающий — пункт о наборе
///   темы, без запасного (текущий и так рабочий).
pub fn needs(
    theme_items: &[DepItem],
    setting_items: &[DepItem],
    standard: &dyn Fn(DepRole) -> Option<String>,
) -> Vec<Need> {
    let mut out = Vec::new();
    for role in [DepRole::Icons, DepRole::Cursor] {
        let t = theme_items.iter().find(|i| i.role == role);
        let s = setting_items.iter().find(|i| i.role == role);
        let broken = s.filter(|s| !matches!(s.state, DepState::Installed(_)));
        if let Some(s) = broken {
            let theme_installed = t.and_then(|t| match &t.state {
                DepState::Installed(name) if !same_name(&t.wanted, &s.wanted) => Some(name.clone()),
                _ => None,
            });
            let theme_other = t.filter(|t| {
                !same_name(&t.wanted, &s.wanted) && matches!(t.state, DepState::Installable(_))
            });
            let (item, from_theme) = match theme_other {
                Some(t) if !matches!(s.state, DepState::Installable(_)) => (t.clone(), true),
                _ => (s.clone(), t.is_some_and(|t| same_name(&t.wanted, &s.wanted))),
            };
            let fallback = theme_installed
                .or_else(|| standard(role))
                .filter(|f| !same_name(f, &item.wanted));
            let home = crate::themes::dep_home_source(item.role, &item.wanted).is_some();
            out.push(Need { item, from_theme, fallback, home });
            continue;
        }
        if let Some(t) = t.filter(|t| !matches!(t.state, DepState::Installed(_))) {
            let home = crate::themes::dep_home_source(t.role, &t.wanted).is_some();
            out.push(Need { item: t.clone(), from_theme: true, fallback: None, home });
        }
    }
    out
}

// ── о чём уже спросили ─────────────────────────────────────────────────────

/// Память «о чём спросили»: тема и ключи наборов ([`need_key`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Asked {
    pub theme: String,
    pub keys: Vec<String>,
}

fn role_key(role: DepRole) -> &'static str {
    match role {
        DepRole::Icons => "icons",
        DepRole::Cursor => "cursor",
    }
}

/// Ключ набора в памяти: роль и имя без учёта регистра.
pub fn need_key(role: DepRole, wanted: &str) -> String {
    format!("{}:{}", role_key(role), wanted.to_lowercase())
}

impl Asked {
    /// Из настроек Rexon (`None`/мусор — ни о чём не спрашивали).
    pub fn from_value(v: Option<&Value>) -> Self {
        let Some(v) = v else { return Self::default() };
        Asked {
            theme: v.get("theme").and_then(Value::as_str).unwrap_or("").to_string(),
            keys: v
                .get("asked")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|k| k.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({ "theme": self.theme, "asked": self.keys })
    }

    /// Память для темы `theme`: тема сменилась — прежние ответы забыты.
    pub fn for_theme(self, theme: &str) -> Self {
        // Пустое имя — разовая неудача чтения, а не смена темы: память та же.
        if self.theme == theme || theme.trim().is_empty() {
            self
        } else {
            Asked { theme: theme.to_string(), keys: Vec::new() }
        }
    }

    /// Пункты, о которых ещё не спрашивали.
    pub fn unasked(&self, needs: &[Need]) -> Vec<Need> {
        needs
            .iter()
            .filter(|n| !self.keys.contains(&need_key(n.item.role, &n.item.wanted)))
            .cloned()
            .collect()
    }

    /// Отметить, что о `needs` спросили.
    pub fn remember(&mut self, needs: &[Need]) {
        for n in needs {
            let k = need_key(n.item.role, &n.item.wanted);
            if !self.keys.contains(&k) {
                self.keys.push(k);
            }
        }
    }
}

// ── уведомление ─────────────────────────────────────────────────────────────

/// Кнопка «Установить».
pub const ACTION_INSTALL: &str = "install";
/// Кнопка «Вернуть стандартные».
pub const ACTION_STANDARD: &str = "standard";
/// Кнопка «Оставить как есть».
pub const ACTION_KEEP: &str = "keep";

/// Уведомление: заголовок, текст и кнопки `(ключ, подпись)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
}

/// «значки «X»» / «курсор «X»».
pub fn set_label(role: DepRole, name: &str) -> String {
    match role {
        // TRANSLATORS: part of a sentence, %1 is an icon set name.
        DepRole::Icons => trf("icons “%1”", &[&name]),
        // TRANSLATORS: part of a sentence, %1 is a cursor theme name.
        DepRole::Cursor => trf("cursor “%1”", &[&name]),
    }
}

fn join_labels(labels: &[String]) -> String {
    match labels {
        [] => String::new(),
        [one] => one.clone(),
        [first, rest @ ..] => {
            // TRANSLATORS: joins two parts of a list: “icons “X” and cursor “Y””.
            trf("%1 and %2", &[first, &join_labels(rest)])
        }
    }
}

fn labels<'a>(needs: impl Iterator<Item = &'a Need>) -> Vec<String> {
    needs.map(|n| set_label(n.item.role, &n.item.wanted)).collect()
}

/// Заголовок уведомлений сторожа.
pub fn notice_title() -> String {
    tr("Missing icons or cursor")
}

/// Заголовок уведомления о тихой докачке.
pub fn notice_title_done() -> String {
    tr("Icons and cursor downloaded")
}

/// Уведомление о `needs` у темы `theme`. `None` — говорить не о чем.
///
/// Три группы: что кладётся в домашний каталог без пароля (сюда пункты
/// попадают, только когда тихая докачка выключена или не удалась —
/// [`auto_home_enabled`]), что ставится пакетом (пароль, у AUR — сборка), и
/// чего нет нигде.
pub fn notice(theme: &str, needs: &[Need]) -> Option<Notice> {
    if needs.is_empty() {
        return None;
    }
    let home: Vec<&Need> = needs.iter().filter(|n| n.home).collect();
    let package: Vec<&Need> = needs
        .iter()
        .filter(|n| !n.home && matches!(n.item.state, DepState::Installable(_)))
        .collect();
    let missing: Vec<&Need> =
        needs.iter().filter(|n| !n.home && matches!(n.item.state, DepState::Missing)).collect();
    let of_theme = |n: &&&Need| n.from_theme && !theme.is_empty();
    let mut parts: Vec<String> = Vec::new();
    let ask = |group: &[&Need], parts: &mut Vec<String>| {
        let by_theme = labels(group.iter().filter(of_theme).copied());
        if !by_theme.is_empty() {
            // TRANSLATORS: %1 is a GTK theme name, %2 is e.g. “icons “X” and cursor “Y””.
            parts.push(trf("The theme “%1” needs %2 — install?", &[&theme, &join_labels(&by_theme)]));
        }
        let by_setting = labels(group.iter().filter(|n| !of_theme(n)).copied());
        if !by_setting.is_empty() {
            parts.push(trf(
                // TRANSLATORS: %1 is e.g. “icons “X” and cursor “Y””.
                "The desktop settings use %1, which isn't on this computer — install?",
                &[&join_labels(&by_setting)],
            ));
        }
    };
    ask(&home, &mut parts);
    if !home.is_empty() {
        parts.push(tr("It is downloaded into your home folder, no password needed."));
    }
    ask(&package, &mut parts);
    let mut packages: Vec<String> = Vec::new();
    for n in &package {
        if let DepState::Installable(p) = &n.item.state {
            if !packages.contains(p) {
                packages.push(p.clone());
            }
        }
    }
    if !packages.is_empty() {
        parts.push(trf(
            "Packages: %1 (an administrator password may be requested).",
            &[&packages.join(", ")],
        ));
    }
    if !missing.is_empty() {
        parts.push(trf(
            // TRANSLATORS: %1 is e.g. “icons “X” and cursor “Y””.
            "Not found in the repositories Rexon knows: %1.",
            &[&join_labels(&labels(missing.iter().copied()))],
        ));
    }
    let fallbacks: Vec<String> =
        needs.iter().filter_map(|n| n.fallback.as_ref().map(|f| set_label(n.item.role, f))).collect();
    if !fallbacks.is_empty() {
        parts.push(trf(
            // TRANSLATORS: %1 is e.g. “icons “Menta” and cursor “mate-black””.
            "You can switch to the standard ones (%1) or keep the current ones.",
            &[&join_labels(&fallbacks)],
        ));
    } else if !missing.is_empty() {
        parts.push(tr("Your current icons and cursor stay."));
    }
    let mut actions = Vec::new();
    if !home.is_empty() || !package.is_empty() {
        actions.push((ACTION_INSTALL.to_string(), tr("Install")));
    }
    if !fallbacks.is_empty() {
        actions.push((ACTION_STANDARD.to_string(), tr("Use standard")));
    }
    actions.push((ACTION_KEEP.to_string(), tr("Keep as is")));
    Some(Notice { summary: notice_title(), body: parts.join("\n"), actions })
}

/// Итог тихой докачки: «К теме «X» докачаны значки «A» и курсор «B».»
pub fn downloaded_text(theme: &str, done: &[(DepRole, String)]) -> String {
    let list = join_labels(&done.iter().map(|(r, n)| set_label(*r, n)).collect::<Vec<_>>());
    if theme.is_empty() {
        // TRANSLATORS: %1 is e.g. “icons “X” and cursor “Y””.
        trf("Downloaded and applied %1.", &[&list])
    } else {
        // TRANSLATORS: %1 is a GTK theme name, %2 is e.g. “icons “X” and cursor “Y””.
        trf("Downloaded %2 for the theme “%1”.", &[&theme, &list])
    }
}

/// Подсказка в уведомлении без кнопок (нет `notify-send`).
pub fn no_buttons_hint() -> String {
    tr("To install it, press “Apply” on this theme in Rexon's “Appearance” tab.")
}

/// argv `notify-send`, который ждёт ответа: нажатая кнопка придёт в stdout
/// своим ключом ([`answer_from_stdout`]). Важное — не гаснет само.
pub fn notify_argv(exe: &str, notice: &Notice) -> Vec<String> {
    crate::utils::notify_ask_argv(exe, &notice.summary, &notice.body, &notice.actions)
}

/// Нажатая кнопка из вывода `notify-send --wait`. `None` — закрыли без
/// ответа (это тоже ответ: «не сейчас», второй раз не спросим).
pub fn answer_from_stdout(stdout: &str) -> Option<&'static str> {
    let last = stdout.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    [ACTION_INSTALL, ACTION_STANDARD, ACTION_KEEP].into_iter().find(|a| *a == last)
}

/// Итог после «Вернуть стандартные»: строка на набор.
pub fn standard_line(role: DepRole, name: &str, ok: bool) -> String {
    if ok {
        trf("Switched to %1.", &[&set_label(role, name)])
    } else {
        trf("⚠️ Couldn't switch to %1.", &[&set_label(role, name)])
    }
}

// ── за чем следить ──────────────────────────────────────────────────────────

/// Какие события ядра значат «оформление могли сменить»: запись и
/// появление файла (в том числе `rename` поверх — так базу dconf и
/// `settings.ini` переписывают заменой). Слежка — общая (`fs_watch`), за
/// КАТАЛОГАМИ: слежка за самим файлом после первой же замены глохнет.
const WATCH_EVENTS: u32 = libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_CREATE;

/// Куда смотреть во всех окружениях сразу (лишняя слежка ничего не стоит,
/// а окружение посреди сеанса не меняется): база dconf (GNOME, MATE,
/// Cinnamon, Budgie, COSMIC — `gsettings`), её замена `keyfile`, xfconf
/// (XFCE), kdeglobals и kcminputrc (KDE), lxqt.conf (LXQt), `settings.ini`
/// GTK и курсор по умолчанию в `~/.icons/default` (тайловые WM).
pub fn watch_targets(home: &str, config_home: &str) -> Vec<WatchTarget> {
    let c = config_home.trim_end_matches('/');
    let h = home.trim_end_matches('/');
    let t = |dir: String, names: &[&str]| WatchTarget { dir, names: names.iter().map(|n| n.to_string()).collect() };
    vec![
        t(c.to_string(), &["kdeglobals", "kcminputrc"]),
        t(format!("{c}/dconf"), &["user"]),
        t(format!("{c}/glib-2.0/settings"), &["keyfile"]),
        t(format!("{c}/gtk-3.0"), &["settings.ini"]),
        t(format!("{c}/gtk-4.0"), &["settings.ini"]),
        t(format!("{c}/xfce4/xfconf/xfce-perchannel-xml"), &["xsettings.xml"]),
        t(format!("{c}/lxqt"), &["lxqt.conf"]),
        t(format!("{h}/.icons/default"), &["index.theme"]),
    ]
}

// ════════════════════════════════════════════════════════════════════════════
//  Живая часть
// ════════════════════════════════════════════════════════════════════════════

/// Один писатель памяти за раз: сторож и «Применить» живут в одном процессе.
fn asked_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    &LOCK
}

/// Докачивать ли значки и курсоры к теме сами (настройка Rexon
/// [`AUTO_HOME_KEY`], по умолчанию — да). Выключено — вместо тихой
/// докачки уведомление с кнопкой «Установить».
pub fn auto_home_enabled() -> bool {
    crate::settings::SettingsManager::new()
        .ok()
        .and_then(|m| m.get(AUTO_HOME_KEY).and_then(Value::as_bool))
        .unwrap_or(true)
}

/// Запомненный ответ на вопрос о значках темы `theme`
/// (`themes::THEME_ICONS_CHOICE_KEY`); `None` — не спрашивали.
pub fn live_icons_choice(theme: &str) -> Option<bool> {
    let m = crate::settings::SettingsManager::new().ok()?;
    crate::themes::icons_choice_of(m.get(crate::themes::THEME_ICONS_CHOICE_KEY), theme)
}

/// Запомнить ответ на вопрос о значках темы `theme`.
pub fn remember_icons_choice(theme: &str, yes: bool) {
    if theme.trim().is_empty() {
        return;
    }
    let _guard = asked_lock().lock();
    match crate::settings::SettingsManager::new() {
        Ok(mut m) => {
            let value = crate::themes::icons_choice_with(m.get(crate::themes::THEME_ICONS_CHOICE_KEY), theme, yes);
            if let Err(e) = m.set(crate::themes::THEME_ICONS_CHOICE_KEY, value) {
                crate::log_warn!("theme", "ответ о значках темы не записан: {e}");
            }
        }
        Err(e) => crate::log_warn!("theme", "настройки не открылись: {e}"),
    }
}

/// Пункт — значки, которые просит сама тема (о них — вопрос с памятью,
/// решение 3а), а не набор из настроек стола.
fn is_theme_icons(n: &Need) -> bool {
    n.from_theme && n.item.role == DepRole::Icons
}

/// Учесть запомненный ответ о значках темы: «Оставить мои» — о них не
/// спрашивать вовсе; без ответа — не качать молча, а спросить кнопкой.
/// Отдаёт `(о чём спрашивать, можно ли значки темы качать молча)`.
pub fn apply_icons_choice(ask: Vec<Need>, choice: Option<bool>) -> (Vec<Need>, bool) {
    let ask = ask.into_iter().filter(|n| !(is_theme_icons(n) && choice == Some(false))).collect();
    (ask, choice == Some(true))
}

fn load_asked() -> Asked {
    crate::settings::SettingsManager::new()
        .map(|m| Asked::from_value(m.get(ASKED_KEY)))
        .unwrap_or_default()
}

fn save_asked(asked: &Asked) {
    match crate::settings::SettingsManager::new() {
        Ok(mut m) => {
            if let Err(e) = m.set(ASKED_KEY, asked.to_value()) {
                crate::log_warn!("theme", "сторож оформления: память ответов не записана: {e}");
            }
        }
        Err(e) => crate::log_warn!("theme", "сторож оформления: настройки не открылись: {e}"),
    }
}

fn run_stdout(argv: &[String], timeout: Option<u64>) -> Option<String> {
    let path = std::env::var("PATH").ok();
    let report = crate::utils::run(
        &crate::utils::RunSpec {
            argv: argv.to_vec(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs: timeout,
            ..crate::utils::RunSpec::default()
        },
        path.as_deref(),
    );
    (report.returncode == Some(0)).then(|| crate::utils::decode_text(&report.stdout))
}

fn live_de() -> String {
    let env = |n: &str| std::env::var(n).unwrap_or_default();
    crate::system::detect_current_de(
        &env("XDG_CURRENT_DESKTOP"),
        &env("DESKTOP_SESSION"),
        !env("WAYLAND_DISPLAY").is_empty(),
    )
    .unwrap_or_else(|| "kde".to_string())
}

/// Тема, значки и курсор окружения `de`: родное хранилище (gsettings,
/// xfconf, DConfig) — теми же командами, что карта активных тем; чего там
/// нет (KDE, LXQt, тайловые WM) — из самой карты.
pub fn live_looks(de: &str) -> Looks {
    let [gtk, icons, cursor] =
        crate::installed_scan::native_role_values(de, |c| run_stdout(&c.argv, c.timeout));
    let mut looks = Looks {
        gtk: gtk.unwrap_or_default(),
        icons: icons.unwrap_or_default(),
        cursor: cursor.unwrap_or_default(),
    };
    if looks.gtk.is_empty() || looks.icons.is_empty() || looks.cursor.is_empty() {
        let map = crate::installed_scan::LiveScan::new(Vec::new(), HashMap::new()).active_map();
        let fill = |slot: &mut String, key: &str| {
            if slot.is_empty() {
                *slot = map.get(key).cloned().unwrap_or_default();
            }
        };
        fill(&mut looks.gtk, "gtk");
        fill(&mut looks.icons, "icons");
        fill(&mut looks.cursor, "cursor");
    }
    looks
}

/// Значения настроек, которые просьбой о наборе не считаются (как у
/// метатемы): запасной набор и ссылка на выбранный курсор.
fn is_real_set_name(name: &str) -> bool {
    !name.is_empty() && !["hicolor", "default"].iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// В настройках записан набор, который стоит под ДРУГИМ написанием
/// (`WhiteSur-Dark`, а на диске `WhiteSur-dark`; другое имя того же набора):
/// GTK ищет каталог по точному имени и молча откатывается. Тогда — имя с
/// диска; `None` — менять нечего.
pub fn spelling_fix(setting: &str, on_disk: Option<&str>) -> Option<String> {
    on_disk.filter(|d| *d != setting).map(str::to_string)
}

/// Исправить написание наборов в настройках ([`spelling_fix`]). `true` —
/// что-то записано (окружение пришлёт запись, и сторож посмотрит снова).
fn live_fix_spelling(de: &str, looks: &Looks) -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    let data = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref().filter(|v| !v.is_empty()), &home);
    let roots = crate::themes::dep_search_roots(&home, &data, &std::env::var("XDG_DATA_DIRS").unwrap_or_default());
    let mut fixed = false;
    for (role, name) in [(DepRole::Icons, &looks.icons), (DepRole::Cursor, &looks.cursor)] {
        if !is_real_set_name(name) {
            continue;
        }
        let found = crate::appearance_installer::live_find_dep(role, name, &roots);
        let Some(right) = spelling_fix(name, found.as_deref()) else { continue };
        let log = Box::new(|line: &str| crate::log_info!("theme", "{line}"));
        let mut installer = crate::appearance_installer::LiveInstaller::new(de, log);
        let ok = installer.apply_installed_set(role, &right);
        crate::log_info!("theme", "сторож оформления: «{name}» на диске зовётся «{right}» — {}", if ok { "исправлено" } else { "исправить не вышло" });
        fixed |= ok;
    }
    fixed
}

/// Чего не хватает в сеансе с `looks` — на живом диске.
fn live_needs(installer: &crate::appearance_installer::LiveInstaller, looks: &Looks) -> Vec<Need> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref().filter(|v| !v.is_empty()), &home);
    let roots = crate::themes::dep_search_roots(&home, &data, &std::env::var("XDG_DATA_DIRS").unwrap_or_default());
    let family = crate::appearance_installer::live_deps_family();
    let find = |role: DepRole, name: &str| crate::appearance_installer::live_find_dep(role, name, &roots);
    let theme_items = if looks.gtk.is_empty() { Vec::new() } else { installer.theme_deps_plan(&looks.gtk) };
    let mut setting_items = Vec::new();
    for (role, name) in [(DepRole::Icons, &looks.icons), (DepRole::Cursor, &looks.cursor)] {
        if !is_real_set_name(name) {
            continue;
        }
        let deps = match role {
            DepRole::Icons => crate::themes::ThemeDeps { icons: Some(name.clone()), cursor: None },
            DepRole::Cursor => crate::themes::ThemeDeps { icons: None, cursor: Some(name.clone()) },
        };
        setting_items.extend(crate::themes::plan_theme_deps(&deps, &family, &|r, w| find(r, w)));
    }
    let de = installer.de_id().to_string();
    let standard = |role: DepRole| {
        let name = crate::appearance_installer::live_standard_theme(&de, role_key(role))?;
        find(role, &name)
    };
    needs(&theme_items, &setting_items, &standard)
}

/// «Применить» GTK-темы в самом Rexon: вопрос о недостающем задан там, и
/// сторож о той же теме и тех же наборах второй раз не спрашивает.
pub fn note_rexon_applied(installer: &crate::appearance_installer::LiveInstaller, theme: &str) {
    let mut looks = live_looks(installer.de_id());
    looks.gtk = theme.to_string();
    let found = live_needs(installer, &looks);
    let _guard = asked_lock().lock();
    let mut asked = load_asked().for_theme(theme);
    asked.remember(&found);
    save_asked(&asked);
}

/// Показать уведомление и дождаться ответа. `None` — закрыли без ответа
/// или кнопок нет вовсе.
fn live_ask(notice: &Notice) -> Option<&'static str> {
    // Ждать ответа хоть сутки: уведомление важное и само не гаснет. Кнопок
    // нет (нет `notify-send` или он старый) — подсказка, что сделать самому.
    let key = crate::utils::notify_ask_live(&notice.summary, &notice.body, &notice.actions, &no_buttons_hint(), 24 * 3600)?;
    answer_from_stdout(&key)
}

/// Исполнить ответ человека по `asked_needs` темы `theme`.
fn live_act(answer: &str, theme: &str, asked_needs: &[Need]) {
    let now = live_looks(&live_de());
    if now.gtk != theme {
        crate::log_info!("theme", "сторож оформления: тема уже другая («{}»), ответ «{answer}» не исполняется", now.gtk);
        return;
    }
    let log = Box::new(|line: &str| crate::log_info!("theme", "{line}"));
    let mut installer = crate::appearance_installer::LiveInstaller::new("", log);
    let lines: Vec<String> = match answer {
        ACTION_INSTALL => {
            let items: Vec<DepItem> = asked_needs
                .iter()
                .filter(|n| n.home || matches!(n.item.state, DepState::Installable(_)))
                .map(|n| n.item.clone())
                .collect();
            installer.allow_deps_install(true);
            if asked_needs.iter().any(is_theme_icons) {
                remember_icons_choice(theme, true);
            }
            installer.set_icons_choice(Some(true));
            installer.apply_dep_items(&items)
        }
        ACTION_KEEP => {
            if asked_needs.iter().any(is_theme_icons) {
                remember_icons_choice(theme, false);
            }
            Vec::new()
        }
        ACTION_STANDARD => asked_needs
            .iter()
            .filter_map(|n| {
                let name = n.fallback.as_ref()?;
                let ok = installer.apply_installed_set(n.item.role, name);
                Some(standard_line(n.item.role, name, ok))
            })
            .collect(),
        _ => Vec::new(),
    };
    if lines.is_empty() {
        return;
    }
    for line in &lines {
        crate::log_info!("theme", "сторож оформления: {line}");
    }
    crate::utils::notify_live(&notice_title(), &lines.join("\n"));
}

/// Итог одного пункта тихой докачки.
#[derive(Debug)]
enum Silent {
    /// Докачан и применён — войдёт в короткое уведомление.
    Done(DepRole, String),
    /// Докачан, но не применился: стоит — спрашивать о нём нечего.
    Settled,
    /// Не докачан (сеть, сбой) — остаётся вопросом «скачать в домашний
    /// каталог», с кнопкой «Установить» для повтора, а не «нет нигде».
    Ask(Need),
}

/// `result` — `install_home_set` + удалось ли применить.
fn silent_step(n: Need, result: Result<(String, bool), String>) -> Silent {
    match result {
        Ok((name, true)) => Silent::Done(n.item.role, name),
        Ok((name, false)) => {
            crate::log_warn!("theme", "сторож оформления: «{name}» докачан, но не применился");
            Silent::Settled
        }
        Err(_) => Silent::Ask(n),
    }
}

/// Тихая докачка ([`auto_home_enabled`]): то, что кладётся в домашний
/// каталог без пароля, — скачать, распаковать и применить без вопроса, и
/// одним коротким уведомлением сказать, что сделано. Отдаёт то, о чём
/// всё-таки надо спросить: пакеты, чего нет нигде, и что докачать не вышло
/// (оно идёт дальше как обычный пункт — пакетом либо «нет нигде»).
fn live_silent_downloads(de: &str, theme: &str, ask: Vec<Need>) -> Vec<Need> {
    let (ask, icons_silent) = apply_icons_choice(ask, live_icons_choice(theme));
    if !ask.iter().any(|n| n.home) || !auto_home_enabled() {
        return ask;
    }
    let log = Box::new(|line: &str| crate::log_info!("theme", "{line}"));
    let mut installer = crate::appearance_installer::LiveInstaller::new(de, log);
    let mut rest = Vec::new();
    let mut done: Vec<(DepRole, String)> = Vec::new();
    for n in ask {
        if !n.home || (is_theme_icons(&n) && !icons_silent) {
            rest.push(n);
            continue;
        }
        let result = installer
            .install_home_set(n.item.role, &n.item.wanted)
            .map(|name| {
                let applied = installer.apply_installed_set(n.item.role, &name);
                (name, applied)
            });
        if let Err(e) = &result {
            crate::log_warn!("theme", "сторож оформления: «{}» не докачан: {e}", n.item.wanted);
        }
        match silent_step(n, result) {
            Silent::Done(role, name) => done.push((role, name)),
            Silent::Settled => {}
            Silent::Ask(n) => rest.push(n),
        }
    }
    if !done.is_empty() {
        let text = downloaded_text(theme, &done);
        crate::log_info!("theme", "сторож оформления: {text}");
        crate::utils::notify_live(&notice_title_done(), &text);
    }
    rest
}

/// Один взгляд на сеанс: спросить, если есть о чём и ещё не спрашивали.
fn live_check(last: &mut Option<Looks>) {
    let de = live_de();
    let looks = live_looks(&de);
    if last.as_ref() == Some(&looks) {
        return;
    }
    // Тема не прочиталась (сеанс ещё не ответил) — судить не о чем, и в
    // `last` такое чтение не идёт, чтобы следующее не сочли сменой.
    if looks.gtk.trim().is_empty() {
        return;
    }
    *last = Some(looks.clone());
    if live_fix_spelling(&de, &looks) {
        return;
    }
    let installer = crate::appearance_installer::LiveInstaller::new(&de, Box::new(|_: &str| {}));
    let found = live_needs(&installer, &looks);
    let ask = {
        let _guard = asked_lock().lock();
        let before = load_asked();
        let mut asked = before.clone().for_theme(&looks.gtk);
        let ask = asked.unasked(&found);
        // Запомнить ДО показа: упади Rexon, пока уведомление висит, второй
        // раз при следующем запуске о том же не спросим.
        asked.remember(&ask);
        if asked != before {
            save_asked(&asked);
        }
        ask
    };
    let ask = live_silent_downloads(&de, &looks.gtk, ask);
    let Some(notice) = notice(&looks.gtk, &ask) else { return };
    crate::log_info!("theme", "сторож оформления: тема «{}», не хватает: {:?}", looks.gtk, ask);
    if let Some(answer) = live_ask(&notice) {
        crate::log_info!("theme", "сторож оформления: ответ «{answer}»");
        live_act(answer, &looks.gtk, &ask);
    }
}

/// Поток сторожа: живёт, пока жив Rexon.
pub fn run_forever() {
    watch_loop(STARTUP_DELAY_SECS, RETRY_WATCH_MS);
}

/// Цикл сторожа: первый взгляд через `delay_secs`, потом — по записям в
/// файлы оформления; без записей раз в `idle_ms` проверить, не появились ли
/// каталоги без слежки.
fn watch_loop(delay_secs: u64, idle_ms: i32) {
    std::thread::sleep(std::time::Duration::from_secs(delay_secs));
    let home = std::env::var("HOME").unwrap_or_default();
    let config = crate::utils::config_home(std::env::var("XDG_CONFIG_HOME").ok().as_deref().filter(|v| !v.is_empty()), &home);
    let Some(mut watch) = Watch::with_events(watch_targets(&home, &config), WATCH_EVENTS) else {
        crate::log_warn!("theme", "сторож оформления не запущен: inotify недоступен");
        return;
    };
    let mut last: Option<Looks> = None;
    live_check(&mut last);
    let mut deadline: Option<std::time::Instant> = None;
    loop {
        let timeout = match deadline {
            Some(d) => d.saturating_duration_since(std::time::Instant::now()).as_millis().min(i32::MAX as u128) as i32,
            None => idle_ms,
        };
        let wake = watch.wait_wake(timeout);
        let (next, act) = loop_step(deadline, wake, std::time::Instant::now());
        deadline = next;
        match act {
            LoopDo::Wait => {}
            LoopDo::Check => live_check(&mut last),
            LoopDo::Rescan => watch.rescan(),
        }
    }
}

/// Что делать циклу сторожа после пробуждения.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopDo {
    Wait,
    Check,
    Rescan,
}

/// Шаг цикла: `deadline` — конец паузы затишья после записи (её нет — ждём
/// записей). Запись в файл оформления продлевает паузу; постороннее
/// событие её не обрывает — раньше проверка уходила сразу, на половинной
/// записи темы.
fn loop_step(
    deadline: Option<std::time::Instant>,
    wake: Wake,
    now: std::time::Instant,
) -> (Option<std::time::Instant>, LoopDo) {
    match (wake, deadline) {
        (Wake::Relevant, _) => (Some(now + std::time::Duration::from_millis(DEBOUNCE_MS as u64)), LoopDo::Wait),
        (_, Some(d)) if now >= d => (None, LoopDo::Check),
        (_, Some(d)) => (Some(d), LoopDo::Wait),
        (Wake::Other, None) => (None, LoopDo::Wait),
        (Wake::Timeout, None) => (None, LoopDo::Rescan),
    }
}

/// Запустить сторожа один раз на процесс.
pub fn spawn_once() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        if let Err(e) = std::thread::Builder::new().name("theme-watch".into()).spawn(run_forever) {
            crate::log_warn!("theme", "сторож оформления не запущен: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(role: DepRole, wanted: &str, state: DepState) -> DepItem {
        DepItem { role, wanted: wanted.to_string(), state }
    }
    fn pkg(p: &str) -> DepState {
        DepState::Installable(p.to_string())
    }
    fn installed(n: &str) -> DepState {
        DepState::Installed(n.to_string())
    }
    fn no_standard(_: DepRole) -> Option<String> {
        None
    }

    /// Сторож сеанса и значки темы (решение 3а): «Оставить мои» — о них не
    /// спрашивать; без ответа — не качать молча; «Скачать и включить» —
    /// можно молча. Набор из настроек стола (не темы) ответ не трогает.
    #[test]
    fn theme_icons_follow_the_remembered_answer() {
        let n = |from_theme: bool, role: DepRole| Need {
            item: item(role, "Windows XP", pkg("xp")),
            from_theme,
            fallback: None,
            home: true,
        };
        let all = || vec![n(true, DepRole::Icons), n(false, DepRole::Icons), n(true, DepRole::Cursor)];
        let (ask, silent) = apply_icons_choice(all(), Some(false));
        assert_eq!(ask.len(), 2);
        assert!(!ask.iter().any(|x| x.from_theme && x.item.role == DepRole::Icons));
        assert!(!silent);
        let (ask, silent) = apply_icons_choice(all(), None);
        assert_eq!(ask.len(), 3);
        assert!(!silent);
        assert!(apply_icons_choice(all(), Some(true)).1);
    }

    /// «Оставить мои»: значки темы не качаются и не включаются, итог
    /// говорит, что свои остались.
    #[test]
    fn keep_mine_leaves_icons_untouched() {
        let mut installer = crate::appearance_installer::LiveInstaller::new("kde", Box::new(|_: &str| {}));
        installer.set_icons_choice(Some(false));
        let lines = installer.apply_dep_items(&[item(DepRole::Icons, "Windows XP", installed("Windows XP"))]);
        assert_eq!(lines, [crate::themes::dep_icons_kept_line("Windows XP")]);
        // Не решено — тоже не трогаются, но и не сказано.
        installer.set_icons_choice(None);
        assert!(installer.apply_dep_items(&[item(DepRole::Icons, "Windows XP", installed("Windows XP"))]).is_empty());
    }

    /// Разовое пустое чтение темы (gsettings не ответил при входе) не
    /// стирает память ответов: иначе о тех же наборах спросили бы снова.
    #[test]
    fn empty_theme_reading_keeps_the_answers() {
        let a = Asked { theme: "X".into(), keys: vec!["icons:y".into()] };
        assert_eq!(a.clone().for_theme(""), a);
        assert!(a.for_theme("Z").keys.is_empty(), "смена темы — память с нуля");
    }

    /// Тихая докачка не удалась (сеть ещё не поднялась) — пункт остаётся
    /// «скачать в домашний каталог» с кнопкой «Установить», а не «нет
    /// нигде» без кнопки.
    #[test]
    fn failed_silent_download_still_offers_install() {
        let n = Need {
            item: item(DepRole::Icons, "WhiteSur-Dark", DepState::Missing),
            from_theme: true,
            fallback: None,
            home: true,
        };
        let Silent::Ask(left) = silent_step(n, Err("network".into())) else {
            panic!("неудача докачки должна остаться вопросом");
        };
        let note = notice("WhiteSur-Dark", &[left]).expect("есть о чём сказать");
        assert!(note.actions.iter().any(|(id, _)| id == ACTION_INSTALL), "{note:?}");
        assert!(!note.body.contains("Not found"), "{}", note.body);
        // Докачан и применён / докачан, но не применился.
        let n2 = Need { item: item(DepRole::Cursor, "c", DepState::Missing), from_theme: true, fallback: None, home: true };
        assert!(matches!(silent_step(n2.clone(), Ok(("c".into(), true))), Silent::Done(DepRole::Cursor, _)));
        assert!(matches!(silent_step(n2, Ok(("c".into(), false))), Silent::Settled));
    }

    /// Посторонняя запись в следимый каталог не обрывает паузу затишья: до
    /// её конца проверка не идёт.
    #[test]
    fn unrelated_write_does_not_cut_the_pause_short() {
        let now = std::time::Instant::now();
        let later = now + std::time::Duration::from_millis(DEBOUNCE_MS as u64);
        assert_eq!(loop_step(Some(later), Wake::Other, now), (Some(later), LoopDo::Wait));
        assert_eq!(loop_step(Some(now), Wake::Timeout, now), (None, LoopDo::Check));
        assert_eq!(loop_step(None, Wake::Timeout, now), (None, LoopDo::Rescan));
        let (d, act) = loop_step(None, Wake::Relevant, now);
        assert!(d.is_some() && act == LoopDo::Wait);
        // Пауза кончилась, а посторонние записи идут — проверить всё равно.
        assert_eq!(loop_step(Some(now), Wake::Other, now), (None, LoopDo::Check));
    }

    /// Живой случай владельца: MATE, тема просит WhiteSur-Dark и
    /// WhiteSur-cursors, окружение записало те же имена, обоих нет.
    #[test]
    fn whitesur_case_asks_once_about_both() {
        let theme = [
            item(DepRole::Icons, "WhiteSur-Dark", pkg("whitesur-icon-theme")),
            item(DepRole::Cursor, "WhiteSur-cursors", pkg("whitesur-cursor-theme-git")),
        ];
        let setting = theme.clone();
        let std_ = |r: DepRole| match r {
            DepRole::Icons => Some("menta".to_string()),
            DepRole::Cursor => Some("mate-black".to_string()),
        };
        let n = needs(&theme, &setting, &std_);
        assert_eq!(n.len(), 2);
        assert!(n.iter().all(|n| n.from_theme));
        assert_eq!(n[0].fallback.as_deref(), Some("menta"));
        let note = notice("WhiteSur-Dark-nord", &n).unwrap();
        let keys: Vec<&str> = note.actions.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, [ACTION_INSTALL, ACTION_STANDARD, ACTION_KEEP]);
        assert!(note.body.contains("WhiteSur-Dark-nord"), "{}", note.body);
        // Оба набора кладутся в дом без пароля: о пакетах речи нет.
        assert!(n.iter().all(|n| n.home));
        assert!(!note.body.contains("whitesur-icon-theme"), "{}", note.body);

        let mut asked = Asked::default().for_theme("WhiteSur-Dark-nord");
        assert_eq!(asked.unasked(&n).len(), 2);
        asked.remember(&n);
        assert!(asked.unasked(&n).is_empty(), "второй раз о той же теме не спрашиваем");
        // Память переживает настройки.
        let back = Asked::from_value(Some(&asked.to_value())).for_theme("WhiteSur-Dark-nord");
        assert!(back.unasked(&n).is_empty());
        // Тема сменилась — ответ забыт.
        assert_eq!(back.for_theme("Adwaita").unasked(&n).len(), 2);
    }

    /// Набор есть только пакетом (пароль, у AUR — сборка): уведомление с
    /// кнопкой и списком пакетов, о домашнем каталоге — ни слова.
    #[test]
    fn package_only_set_is_asked_with_packages() {
        let theme = [item(DepRole::Icons, "Papirus-Dark", pkg("papirus-icon-theme"))];
        let n = needs(&theme, &[], &no_standard);
        assert_eq!(n.len(), 1);
        assert!(!n[0].home);
        let note = notice("Arc-Dark", &n).unwrap();
        assert!(note.body.contains("papirus-icon-theme"), "{}", note.body);
        let keys: Vec<&str> = note.actions.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, [ACTION_INSTALL, ACTION_KEEP]);
    }

    /// Домашние источники WhiteSur — в таблице, у прочих их нет.
    #[test]
    fn home_sources_of_known_sets() {
        use crate::themes::{dep_home_source, HomeSource};
        assert!(matches!(
            dep_home_source(DepRole::Icons, "WhiteSur-Dark"),
            Some(HomeSource::Ocs { content_id: "1405756", .. })
        ));
        assert!(matches!(
            dep_home_source(DepRole::Cursor, "WhiteSur-cursors"),
            Some(HomeSource::Ocs { content_id: "1411743", .. })
        ));
        assert_eq!(dep_home_source(DepRole::Icons, "Papirus"), None);
        assert_eq!(dep_home_source(DepRole::Icons, "Unknown-Set"), None);
        let done = [(DepRole::Icons, "WhiteSur-dark".to_string()), (DepRole::Cursor, "WhiteSur-Cursors".to_string())];
        let text = downloaded_text("WhiteSur-Dark-nord", &done);
        assert!(text.contains("WhiteSur-Dark-nord") && text.contains("WhiteSur-Cursors"), "{text}");
    }

    #[test]
    fn nothing_to_ask_when_all_installed() {
        let theme = [item(DepRole::Icons, "Papirus", installed("Papirus"))];
        let setting = [item(DepRole::Icons, "Papirus", installed("Papirus"))];
        assert!(needs(&theme, &setting, &no_standard).is_empty());
        assert!(notice("Arc", &[]).is_none());
    }

    /// Человек сам выбрал рабочие значки, тема просит недостающие: спросить
    /// о наборе темы, но «стандартные» не предлагать — текущие и так живые.
    #[test]
    fn working_setting_gets_no_fallback() {
        let theme = [item(DepRole::Icons, "WhiteSur-Dark", pkg("whitesur-icon-theme"))];
        let setting = [item(DepRole::Icons, "Papirus", installed("Papirus"))];
        let n = needs(&theme, &setting, &|_| Some("menta".into()));
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].fallback, None);
        let note = notice("WhiteSur-Dark-nord", &n).unwrap();
        assert!(!note.actions.iter().any(|(k, _)| k == ACTION_STANDARD));
    }

    /// Набора нет нигде: честно сказать и предложить стандартный, только
    /// если он стоит; без стандартного — только «Оставить».
    #[test]
    fn missing_everywhere_offers_standard_or_keep() {
        let setting = [item(DepRole::Cursor, "Foo-cursors", DepState::Missing)];
        let n = needs(&[], &setting, &|_| Some("Adwaita".into()));
        assert_eq!(n[0].fallback.as_deref(), Some("Adwaita"));
        let note = notice("", &n).unwrap();
        let keys: Vec<&str> = note.actions.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, [ACTION_STANDARD, ACTION_KEEP]);
        assert!(note.body.contains("Foo-cursors"));

        let n = needs(&[], &setting, &no_standard);
        let note = notice("", &n).unwrap();
        let keys: Vec<&str> = note.actions.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, [ACTION_KEEP]);
    }

    /// В настройках битое имя, а набор самой темы стоит — запасной вариант
    /// именно он, а не стандартный окружения.
    #[test]
    fn theme_set_installed_is_preferred_fallback() {
        let theme = [item(DepRole::Icons, "Papirus-Dark", installed("Papirus-Dark"))];
        let setting = [item(DepRole::Icons, "Gone", DepState::Missing)];
        let n = needs(&theme, &setting, &|_| Some("menta".into()));
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].item.wanted, "Gone");
        assert_eq!(n[0].fallback.as_deref(), Some("Papirus-Dark"));
    }

    /// Стандартный набор и есть недостающий — не предлагать его же.
    #[test]
    fn fallback_never_equals_the_missing_set() {
        let setting = [item(DepRole::Cursor, "Adwaita", DepState::Missing)];
        let n = needs(&[], &setting, &|_| Some("adwaita".into()));
        assert_eq!(n[0].fallback, None);
    }

    #[test]
    fn answers_and_argv() {
        assert_eq!(answer_from_stdout("install\n"), Some(ACTION_INSTALL));
        assert_eq!(answer_from_stdout("12\nstandard\n"), Some(ACTION_STANDARD));
        assert_eq!(answer_from_stdout(""), None);
        assert_eq!(answer_from_stdout("rm -rf\n"), None);
        let note = Notice {
            summary: "S".into(),
            body: "B".into(),
            actions: vec![(ACTION_INSTALL.into(), "Установить".into())],
        };
        let argv = notify_argv("/usr/bin/notify-send", &note);
        assert!(argv.contains(&"--wait".to_string()));
        assert!(argv.contains(&"--action=install=Установить".to_string()));
        assert_eq!(&argv[argv.len() - 2..], ["S", "B"]);
    }

    #[test]
    fn watch_targets_cover_every_store() {
        let t = watch_targets("/h", "/h/.config/");
        let has = |dir: &str, name: &str| t.iter().any(|w| w.dir == dir && w.names.iter().any(|n| n == name));
        assert!(has("/h/.config/dconf", "user"));
        assert!(has("/h/.config/glib-2.0/settings", "keyfile"));
        assert!(has("/h/.config", "kdeglobals"));
        assert!(has("/h/.config/xfce4/xfconf/xfce-perchannel-xml", "xsettings.xml"));
        assert!(has("/h/.config/gtk-3.0", "settings.ini"));
        assert!(has("/h/.config/lxqt", "lxqt.conf"));
    }

    #[test]
    fn spelling_is_fixed_only_when_disk_differs() {
        assert_eq!(spelling_fix("WhiteSur-Dark", Some("WhiteSur-dark")).as_deref(), Some("WhiteSur-dark"));
        assert_eq!(spelling_fix("DMZ-White", Some("Vanilla-DMZ")).as_deref(), Some("Vanilla-DMZ"));
        assert_eq!(spelling_fix("Papirus", Some("Papirus")), None);
        assert_eq!(spelling_fix("Gone", None), None);
    }

    #[test]
    fn not_a_set_name() {
        assert!(!is_real_set_name(""));
        assert!(!is_real_set_name("hicolor"));
        assert!(!is_real_set_name("default"));
        assert!(is_real_set_name("WhiteSur-cursors"));
    }

    /// Стенд сторожа — НЕ для обычного прогона: живые gsettings и
    /// уведомление. Только под `dbus-run-session` с
    /// `GSETTINGS_BACKEND=keyfile`, временными `HOME`/`XDG_*` (в пути `HOME`
    /// обязано быть `deps-stand`) и подменёнными в `PATH` `notify-send`,
    /// `pkexec`, `paru`/`yay`, `pacman`:
    /// `REXON_WATCH_STAND_SECS=<сколько ждать> cargo test -p rexon-core --lib
    /// theme_watch_stand -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn theme_watch_stand() {
        let home = std::env::var("HOME").unwrap_or_default();
        let Ok(secs) = std::env::var("REXON_WATCH_STAND_SECS") else { return };
        assert!(home.contains("deps-stand"), "стенд только во временном HOME, а не в {home}");
        let secs: u64 = secs.parse().unwrap_or(30);
        std::thread::spawn(|| watch_loop(0, 1000));
        std::thread::sleep(std::time::Duration::from_secs(secs));
        println!("память: {:?}", load_asked());
    }
}
