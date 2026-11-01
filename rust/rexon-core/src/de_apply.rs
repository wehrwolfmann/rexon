//! Кнопки разделов окружений во вкладке «Оформление»: «Применить» у тем,
//! «Включить»/«Выключить» у дополнений, «Запустить»/«Остановить» у Conky.
//!
//! Решение владельца 04.10.2026: установка `.rxn` ТОЛЬКО кладёт файлы на
//! диск, включает человек — кнопкой. Прежде у типов матрицы без раздела во
//! вкладке (рамки xfwm4 и Marco, темы оболочек GNOME/Cinnamon/LXQt,
//! переключатель окон KDE, скрипты и эффекты KWin, расширения GNOME,
//! апплеты/десклеты/расширения Cinnamon, действия Nemo, Conky) включала сама
//! установка: «поставил — и у тебя уже другие рамки», без выбора.
//!
//! Второго списка команд здесь нет (правило #12). Что писать и куда,
//! описывает матрица ([`crate::de_customization`]), шаги включения и
//! выключения — те же, что делала установка и делает удаление
//! ([`crate::ext_manager::de_enable_steps`] / [`de_disable_steps`]). Этот
//! модуль добавляет только то, чего у установки не было:
//!   * ЧТЕНИЕ — что включено сейчас (плитки «Активные» и кнопка «Выключить»
//!     обязаны говорить правду, а не помнить, что нажимали);
//!   * исполнение кнопки с проверкой — после шагов значение перечитывается,
//!     и «✅» ставится только если окружение действительно его приняло;
//!   * Conky — запуск, остановка и автозапуск ОДНОГО виджета (установка
//!     гасила все conky разом по имени процесса).
//!
//! Экран входа (SDDM) и заставка загрузки (Plymouth) идут через системный
//! помощник от root — их «Применить» живёт рядом с установкой в
//! [`crate::appearance_installer`]; здесь только строка вызова
//! ([`system_apply_argv`]).
//!
//! [`de_disable_steps`]: crate::ext_manager::de_disable_steps

use crate::animations::LookIo;
use crate::de_customization::{self as dec, Activate, EntryFmt, Payload, Row};
use crate::i18n::{tr, trf};

// ============================================================================
// Вид кнопки
// ============================================================================

/// Какая кнопка у раздела.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// «🎨 Применить»: одна тема на окружение, выбор заменяет прежний.
    Apply,
    /// «▶ Включить» / «⏸ Выключить»: дополнений включено сколько угодно.
    Toggle,
    /// «▶ Запустить» / «⏹ Остановить»: отдельная программа (Conky).
    Run,
}

/// Как включается тип `.rxn` кнопкой раздела.
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    /// Тема — одно значение ключа окружения (xfwm4, Marco, оболочки GNOME,
    /// Cinnamon и LXQt, раскладка Alt+Tab).
    Theme(&'static Row),
    /// Дополнение — элемент списка включённых (скрипты и эффекты KWin,
    /// расширения GNOME, апплеты/десклеты/расширения Cinnamon, действия Nemo).
    Addon(&'static Row),
    /// Виджет Conky — отдельная программа, у неё запуск и автозапуск.
    Conky,
}

impl Mode {
    pub fn button(self) -> Button {
        match self {
            Mode::Theme(_) => Button::Apply,
            Mode::Addon(_) => Button::Toggle,
            Mode::Conky => Button::Run,
        }
    }
}

/// Тип Conky.
pub const CONKY_KIND: &str = "theme-conky";

/// Тип действий Nemo: файлы, которые Nemo подхватывает сам; выключенные он
/// держит в своём списке [`NEMO_DISABLED`] (родной ключ — так же выключает
/// их «Настройка действий» самого Nemo).
pub const NEMO_KIND: &str = "nemo-action";
pub const NEMO_DISABLED: (&str, &str) = ("org.nemo.plugins", "disabled-actions");

/// Тип рамок окон Marco (MATE).
pub const MARCO_KIND: &str = "theme-marco";

/// Тип рамок окон Openbox и labwc: `<theme><name>` в `rc.xml` менеджера
/// окон сеанса ([`session_wm`]).
pub const OPENBOX_KIND: &str = "theme-openbox";

/// Схема расширения User Themes: через её ключ `name` GNOME берёт тему
/// оболочки. Схемы нет — расширение не установлено, и записать тему некуда.
pub const USER_THEME_SCHEMA: &str = "org.gnome.shell.extensions.user-theme";

/// Способ включения типа кнопкой. `None` — у типа нет кнопки этого модуля
/// (типы общего установщика оформления, у которых свои действия; плагин
/// обоев Plasma, который выбирается только в настройках стола).
pub fn mode(kind: &str) -> Option<Mode> {
    if kind == CONKY_KIND {
        return Some(Mode::Conky);
    }
    let row = dec::de_managed_row(kind)?;
    match row.activate {
        Activate::KdeConfig { .. }
        | Activate::Gsettings { .. }
        | Activate::Xfconf { .. }
        | Activate::IniKey { .. }
        | Activate::RcXmlTheme => Some(Mode::Theme(row)),
        Activate::KwinPlugin | Activate::GnomeExtension | Activate::GsettingsList { .. } => {
            Some(Mode::Addon(row))
        }
        Activate::Auto if kind == NEMO_KIND => Some(Mode::Addon(row)),
        _ => None,
    }
}

/// Кнопка раздела по его виду применения (`apply_kind` = ключ раздела).
/// Разделы, которых этот модуль не ведёт, — обычное «Применить».
pub fn button_for_apply_kind(apply_kind: &str) -> Button {
    crate::categories::category_kind(apply_kind)
        .and_then(mode)
        .map(Mode::button)
        .unwrap_or(Button::Apply)
}

/// Вид применения из номера местной плитки `local:<вид>:<имя>`.
pub fn apply_kind_of_local_id(id: &str) -> Option<&str> {
    id.strip_prefix("local:")?.split_once(':').map(|(k, _)| k)
}

// ============================================================================
// Чтение: что включено сейчас
// ============================================================================

/// Где искать файлы пользователя.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub home: String,
    /// `$XDG_CONFIG_HOME`, если задан (иначе `~/.config`).
    pub xdg_config_home: Option<String>,
    /// Сеанс Wayland (для оговорки у Conky).
    pub wayland: bool,
    /// Запущенное окружение (`mate`, `gnome`…; пусто — не опознано) — для
    /// оговорки у Conky: на Wayland он работает не везде одинаково.
    pub de: String,
}

impl Ctx {
    /// Живой сеанс.
    pub fn live() -> Self {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        let home = {
            let h = env("HOME");
            if h.is_empty() { "/root".to_string() } else { h }
        };
        let xdg = env("XDG_CONFIG_HOME");
        let wayland = !env("WAYLAND_DISPLAY").is_empty()
            || env("XDG_SESSION_TYPE").eq_ignore_ascii_case("wayland");
        let de = crate::system::detect_current_de(
            &env("XDG_CURRENT_DESKTOP"),
            &env("DESKTOP_SESSION"),
            !env("WAYLAND_DISPLAY").is_empty(),
        )
        .unwrap_or_default();
        Ctx {
            home,
            xdg_config_home: (!xdg.is_empty()).then_some(xdg),
            wayland,
            de,
        }
    }

    pub(crate) fn config(&self, rel: &str) -> String {
        match &self.xdg_config_home {
            Some(x) => format!("{x}/{rel}"),
            None => format!("{}/.config/{rel}", self.home),
        }
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}

/// Значение ini-ключа в секции (`[группа]` → `ключ=`).
pub fn ini_value(text: &str, group: &str, key: &str) -> Option<String> {
    let mut here = false;
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            here = &s[1..s.len() - 1] == group;
            continue;
        }
        if !here {
            continue;
        }
        if let Some((k, v)) = s.split_once('=') {
            if k.trim() == key {
                let v = v.trim();
                return (!v.is_empty()).then(|| v.to_string());
            }
        }
    }
    None
}

/// Значение `gsettings get` без кавычек (`'Name'` → `Name`).
pub fn parse_gsettings_string(out: &str) -> Option<String> {
    let v = out.trim().trim_matches('\'').trim_matches('"').trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// Список строк из ответа `gsettings get` (`['a', 'b']`, `@as []`).
pub fn parse_gsettings_list(out: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in out.chars() {
        match quote {
            Some(q) if ch == q => {
                items.push(std::mem::take(&mut cur));
                quote = None;
            }
            Some(_) => cur.push(ch),
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None => {}
        }
    }
    items
}

/// Идентификатор объекта из элемента списка включённых.
pub fn entry_uuid(entry: &str, fmt: EntryFmt) -> Option<String> {
    let field = match fmt {
        EntryFmt::Uuid => Some(entry),
        // panel1:<зона>:<порядок>:<uuid>:<номер>
        EntryFmt::CinnamonApplet => entry.split(':').nth(3),
        // <uuid>:<номер>:<x>:<y>
        EntryFmt::CinnamonDesklet => entry.split(':').next(),
    }?;
    let field = field.trim();
    (!field.is_empty()).then(|| field.to_string())
}

/// Включён ли плагин KWin по тексту `kwinrc` (`[Plugins] <id>Enabled=`).
/// Ключа нет — решает `EnabledByDefault` из метаданных плагина.
pub fn kwin_plugin_enabled(kwinrc: Option<&str>, id: &str, enabled_by_default: bool) -> bool {
    match kwinrc.and_then(|t| ini_value(t, "Plugins", &format!("{id}Enabled"))) {
        Some(v) => v.eq_ignore_ascii_case("true"),
        None => enabled_by_default,
    }
}

/// `KPlugin.EnabledByDefault` из `metadata.json`.
fn kpackage_enabled_by_default(meta_json: Option<&str>) -> bool {
    meta_json
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| v.get("KPlugin")?.get("EnabledByDefault")?.as_bool())
        .unwrap_or(false)
}

/// Системный каталог объектов типа рядом с пользовательским каталогом
/// матрицы: `.local/share/X` → `/usr/share/X`, `.themes` → `/usr/share/themes`.
/// Пусто — у типа системного каталога нет (Conky).
pub fn system_dir(row: &Row, usr_root: &str) -> String {
    if let Some(rest) = row.dest.strip_prefix(".local/share/") {
        return format!("{usr_root}/share/{rest}");
    }
    if row.dest == ".themes" {
        return format!("{usr_root}/share/themes");
    }
    String::new()
}

/// Каталоги, где лежат объекты типа (пользовательский, затем системный).
fn object_dirs(ctx: &Ctx, row: &Row) -> Vec<String> {
    let mut dirs = vec![row.dest_path(&ctx.home)];
    let sys = system_dir(row, "/usr");
    if !sys.is_empty() {
        dirs.push(sys);
    }
    dirs
}

/// Текущее значение темы (`None` — не задано или прочитать нечем).
pub fn theme_value(io: &dyn LookIo, ctx: &Ctx, row: &Row) -> Option<String> {
    match row.activate {
        Activate::KdeConfig { file, group, key, .. } => {
            if io.have("kreadconfig6") {
                let (ok, out) = io.run(&argv(&[
                    "kreadconfig6", "--file", file, "--group", group, "--key", key,
                ]));
                if ok {
                    return parse_gsettings_string(&out);
                }
            }
            ini_value(&io.read(&ctx.config(file))?, group, key)
        }
        Activate::Gsettings { schema, key } => {
            let (ok, out) = io.run(&argv(&["gsettings", "get", schema, key]));
            if ok { parse_gsettings_string(&out) } else { None }
        }
        Activate::Xfconf { channel, prop } => {
            let (ok, out) = io.run(&argv(&["xfconf-query", "-c", channel, "-p", prop]));
            if ok { parse_gsettings_string(&out) } else { None }
        }
        Activate::IniKey { file, group, key } => {
            ini_value(&io.read(&format!("{}/{file}", ctx.home))?, group, key)
        }
        Activate::RcXmlTheme => {
            let wm = session_wm(io, ctx).filter(|w| w.rc_xml_path().is_some())?;
            let (_, text, _) = rc_xml(io, ctx, wm);
            crate::qt_settings::rc_xml_theme_value(&text?, &["name"]).filter(|v| !v.is_empty())
        }
        _ => None,
    }
}

// ============================================================================
// Рамки Openbox и labwc: `rc.xml` менеджера окон сеанса
// ============================================================================

/// Кто рисует рамки в этом сеансе — для LXQt по его `session.conf`, в
/// сеансе labwc — labwc (`qt_settings::session_title_wm`).
pub fn session_wm(io: &dyn LookIo, ctx: &Ctx) -> Option<crate::qt_settings::TitleWm> {
    let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
    let home = ctx.config("");
    crate::qt_settings::session_title_wm(&ctx.de, home.trim_end_matches('/'), &dirs, ctx.wayland, &|p| io.read(p))
}

/// `rc.xml` менеджера: путь своего, текст (свой либо системный образец) и
/// взят ли он из системного.
fn rc_xml(io: &dyn LookIo, ctx: &Ctx, wm: crate::qt_settings::TitleWm) -> (String, Option<String>, bool) {
    let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
    let home = ctx.config("");
    crate::qt_settings::rc_xml_source(wm, home.trim_end_matches('/'), &dirs, &|p| io.read(p))
}

/// Шаги темы рамок `name`: новый текст `rc.xml` (меняется только
/// `<theme><name>`) и `--reconfigure` менеджера. `Err` — отказ для человека
/// до первой записи: рамки рисует не Openbox и не labwc, либо `rc.xml`
/// писать некуда или он не разбирается.
pub fn rc_theme_steps(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Result<Vec<crate::ext_manager::ExtStep>, String> {
    use crate::qt_settings::TitleWm;
    let refuse = |why: String| Err(trf("Couldn't apply “%1”: %2", &[&name, &why]));
    let wm = match session_wm(io, ctx) {
        Some(wm @ (TitleWm::Openbox | TitleWm::Labwc)) => wm,
        Some(other) => {
            return refuse(trf(
                // TRANSLATORS: %1 is a window manager, e.g. “kwin” or “xfwm4”.
                "window borders in this session are drawn by %1, and Openbox themes only work in Openbox and labwc",
                &[&other.program()],
            ))
        }
        None => {
            return refuse(tr(
                "Openbox themes only work in Openbox and labwc, and this session uses another window manager \
                 (or none is chosen in LXQt Session Settings)",
            ))
        }
    };
    let (own, text, _) = rc_xml(io, ctx, wm);
    let Some(content) = crate::qt_settings::rc_xml_with_theme_value(text.as_deref(), wm, &["name"], name) else {
        return refuse(if text.is_none() {
            trf("%1 doesn't exist, and there is no system settings file to start from", &[&own])
        } else {
            trf("the settings file %1 can't be read, so Rexon leaves it as it is", &[&own])
        });
    };
    Ok(vec![
        crate::ext_manager::ExtStep::WriteFile { path: own, content },
        crate::ext_manager::ExtStep::Run { argv: argv(&[wm.program(), "--reconfigure"]), timeout_secs: Some(10) },
    ])
}

/// Включённые объекты дополнения (имена каталогов/файлов, как на диске).
pub fn enabled_addons(io: &dyn LookIo, ctx: &Ctx, kind: &str, row: &Row) -> Vec<String> {
    if kind == NEMO_KIND {
        let (schema, key) = NEMO_DISABLED;
        let (ok, out) = io.run(&argv(&["gsettings", "get", schema, key]));
        let disabled = if ok { parse_gsettings_list(&out) } else { Vec::new() };
        let suffix = match row.payload {
            Payload::Files { ext } => ext,
            _ => "",
        };
        let mut out = Vec::new();
        for dir in object_dirs(ctx, row) {
            for f in io.list_dir(&dir) {
                if f.ends_with(suffix) && !disabled.contains(&f) && !out.contains(&f) {
                    out.push(f);
                }
            }
        }
        return out;
    }
    match row.activate {
        Activate::KwinPlugin => {
            let kwinrc = io.read(&ctx.config("kwinrc"));
            let mut out = Vec::new();
            for dir in object_dirs(ctx, row) {
                for id in io.list_dir(&dir) {
                    let meta = io.read(&format!("{dir}/{id}/metadata.json"));
                    if kwin_plugin_enabled(
                        kwinrc.as_deref(),
                        &id,
                        kpackage_enabled_by_default(meta.as_deref()),
                    ) && !out.contains(&id)
                    {
                        out.push(id);
                    }
                }
            }
            out
        }
        Activate::GnomeExtension => {
            let (ok, out) = io.run(&argv(&["gsettings", "get", "org.gnome.shell", "enabled-extensions"]));
            if ok { parse_gsettings_list(&out) } else { Vec::new() }
        }
        Activate::GsettingsList { schema, key, entry, .. } => {
            let (ok, out) = io.run(&argv(&["gsettings", "get", schema, key]));
            if !ok {
                return Vec::new();
            }
            let mut v: Vec<String> = Vec::new();
            for e in parse_gsettings_list(&out) {
                if let Some(u) = entry_uuid(&e, entry) {
                    if !v.contains(&u) {
                        v.push(u);
                    }
                }
            }
            v
        }
        _ => Vec::new(),
    }
}

/// Что включено сейчас у типа: имя темы, включённые дополнения либо
/// запущенные виджеты Conky. Пусто — ничего (или прочитать нечем).
pub fn active_names(io: &dyn LookIo, ctx: &Ctx, kind: &str) -> Vec<String> {
    match mode(kind) {
        // Тему рамок, которую Marco не загрузил, он не показывает, сколько
        // бы её имя ни стояло в ключе: «Активна» у неё была бы неправдой.
        Some(Mode::Theme(row)) => theme_value(io, ctx, row)
            .filter(|name| kind != MARCO_KIND || marco_theme_problem(io, ctx, name).is_none())
            .into_iter()
            .collect(),
        Some(Mode::Addon(row)) => enabled_addons(io, ctx, kind, row),
        Some(Mode::Conky) => conky_running_ids(io, ctx),
        None => Vec::new(),
    }
}

// ============================================================================
// Исполнение кнопки
// ============================================================================

/// Шаги плана из установщика расширений — исполнить по-живому.
fn run_steps(io: &dyn LookIo, steps: &[crate::ext_manager::ExtStep]) -> Option<String> {
    let steps: Vec<crate::install_plan::Step> = steps.iter().cloned().map(Into::into).collect();
    match exec_plan(io, &steps, &mut crate::install_plan::Undo::off()) {
        Ok(err) => err,
        Err(f) => Some(crate::install_plan::failure_text(&f)),
    }
}

/// Исполнить шаги кнопки раздела ОБЩИМ исполнителем
/// ([`crate::install_plan::exec_undoable`]) под откатом `undo`; команды и
/// запуск без ожидания идут через `io` (`io.run`, `io.spawn`: в проверках —
/// подделка, ничего не запускается), так что подмена машины та же, что
/// прежде.
///
/// Исполняются ВСЕ шаги; неудача команды не обрывает прогон, а
/// возвращается первой (`Ok(Some(текст))`). Мерило успеха — не коды
/// команд, а то, что окружение отдаёт после записи ([`check`]): обновление
/// окружения (`qdbus … reconfigure`) законно падает, когда KWin не запущен,
/// а ключ при этом записан. `Err` — сбой файлового шага (записи под
/// откатом, [`crate::themes::ApplyStep::Guard`]).
pub fn exec_plan(
    io: &dyn LookIo,
    steps: &[crate::install_plan::Step],
    undo: &mut crate::install_plan::Undo,
) -> Result<Option<String>, crate::install_plan::Failure> {
    use crate::utils::{RunOutcome, RunReport};
    let first: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
    let run = |argv: &[String], _timeout: Option<u64>| -> RunReport {
        let (ok, out) = io.run(argv);
        // Чтение прежнего значения под снимок «что было» не причина неудачи:
        // у свойства xfconf, которого ещё нет, оно и должно не удаться.
        if !ok && first.borrow().is_none() && !crate::install_plan::is_value_read(argv) {
            let tool = argv.first().map(String::as_str).unwrap_or("");
            let detail = out.trim();
            *first.borrow_mut() = Some(if detail.is_empty() { tool.to_string() } else { format!("{tool}: {detail}") });
        }
        RunReport {
            outcome: if ok { RunOutcome::Ok } else { RunOutcome::Failed },
            returncode: Some(if ok { 0 } else { 1 }),
            stdout: if ok { out.into_bytes() } else { Vec::new() },
            ..Default::default()
        }
    };
    let have = |tool: &str| io.have(tool);
    // Обновление окружения (`cinnamon --replace`) запускается и не ждётся.
    let spawn = |argv: &[String]| {
        io.spawn(argv);
    };
    let ctx = crate::install_plan::Ctx {
        install: true,
        steamos: false,
        privileged: None,
        run: &run,
        have: &have,
        spawn: &spawn,
        wayland: false,
        sync_env: &crate::install_plan::no_sync_env,
    };
    crate::install_plan::exec_undoable(steps, &mut Vec::new(), &ctx, undo)?;
    Ok(first.into_inner())
}

/// Что сверить после записи: правду окружения, а не то, что писали.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// Тема: окружение отдаёт `name`.
    Theme { kind: String, name: String },
    /// Дополнение было включено (`was_on`) — после нажатия должно стать
    /// наоборот.
    Addon { kind: String, name: String, was_on: bool },
}

/// План нажатия кнопки раздела ([`press_plan`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Отказ до первого шага — ничего не записано.
    Refused(String),
    /// Шаги общего плана, затем сверка [`check`].
    Steps { steps: Vec<crate::install_plan::Step>, check: Check },
    /// Conky — запуск и остановка процессов своим путём ([`press`]).
    Own,
}

/// Файлы, которые перепишут команды кнопки раздела (`kwriteconfig6
/// --file …`, сценарий правки ini): их «что было» снимает шаг
/// [`crate::themes::ApplyStep::Guard`]. Записи `gsettings` и `xfconf-query`
/// держат службы (dconf, xfconfd) — снимать там нечего.
fn written_files(ctx: &Ctx, row: &Row) -> Vec<String> {
    match row.activate {
        Activate::KdeConfig { file, .. } => vec![ctx.config(file)],
        Activate::KwinPlugin => vec![ctx.config("kwinrc")],
        Activate::IniKey { file, .. } => vec![format!("{}/{file}", ctx.home)],
        _ => Vec::new(),
    }
}

fn plan_steps(ctx: &Ctx, row: &Row, steps: Vec<crate::ext_manager::ExtStep>, check: Check) -> Plan {
    let guard = written_files(ctx, row);
    let mut out: Vec<crate::install_plan::Step> = Vec::new();
    if !guard.is_empty() && !steps.is_empty() {
        out.push(crate::themes::ApplyStep::Guard { paths: guard }.into());
    }
    out.extend(steps.into_iter().map(Into::into));
    Plan::Steps { steps: out, check }
}

/// План нажатия кнопки раздела — без единой записи и запуска (только пробы:
/// включено ли дополнение сейчас, есть ли схема User Themes, загрузит ли
/// Marco тему). Тема — включить; дополнение — переключить по правде
/// окружения; Conky — своим путём ([`Plan::Own`]).
pub fn press_plan(io: &dyn LookIo, ctx: &Ctx, kind: &str, name: &str) -> Plan {
    match mode(kind) {
        Some(Mode::Theme(row)) => {
            if kind == MARCO_KIND {
                // Ключ не пишется вовсе: Marco такую тему не загрузит, оставит
                // прежние рамки, а вкладка показала бы «Применено».
                if let Some(problem) = marco_theme_problem(io, ctx, name) {
                    return Plan::Refused(marco_refusal(name, &problem));
                }
            }
            if let Some(reason) = missing_user_themes(io, row) {
                return Plan::Refused(trf("Couldn't apply “%1”: %2", &[&name, &reason]));
            }
            if kind == OPENBOX_KIND {
                // Свой `rc.xml` пишется шагом записи файла — под откатом
                // нажатия, как запись своими руками.
                return match rc_theme_steps(io, ctx, name) {
                    Ok(steps) => Plan::Steps {
                        steps: steps.into_iter().map(Into::into).collect(),
                        check: Check::Theme { kind: kind.to_string(), name: name.to_string() },
                    },
                    Err(why) => Plan::Refused(why),
                };
            }
            let steps = enable_steps(io, ctx, kind, name);
            plan_steps(ctx, row, steps, Check::Theme { kind: kind.to_string(), name: name.to_string() })
        }
        Some(Mode::Addon(row)) => {
            let was_on = enabled_addons(io, ctx, kind, row).iter().any(|n| n == name);
            let steps = if was_on {
                disable_steps(io, ctx, kind, name)
            } else {
                enable_steps(io, ctx, kind, name)
            };
            plan_steps(ctx, row, steps, Check::Addon { kind: kind.to_string(), name: name.to_string(), was_on })
        }
        Some(Mode::Conky) => Plan::Own,
        None => Plan::Refused(trf("“%1” can't be applied here", &[&name])),
    }
}

/// Сверка после шагов: что окружение отдаёт — `(получилось, сообщение)`.
/// `err` — первая неудача команды ([`exec_plan`]): её называет отказ.
pub fn check(io: &dyn LookIo, ctx: &Ctx, check: &Check, err: Option<String>) -> (bool, String) {
    match check {
        Check::Theme { kind, name } => {
            let Some(Mode::Theme(row)) = mode(kind) else {
                return (false, trf("“%1” can't be applied here", &[name]));
            };
            // Правда — то, что окружение отдаёт после записи, а не то, что писали.
            match theme_value(io, ctx, row) {
                Some(v) if v == *name => (true, trf("✅ Applied: %1", &[name])),
                _ => (false, trf("Couldn't apply “%1”: %2", &[name, &not_accepted(err)])),
            }
        }
        Check::Addon { kind, name, was_on } => {
            let Some(Mode::Addon(row)) = mode(kind) else {
                return (false, trf("“%1” can't be applied here", &[name]));
            };
            let now_on = enabled_addons(io, ctx, kind, row).iter().any(|n| n == name);
            if now_on == *was_on {
                return (false, trf("Couldn't turn “%1” on or off: %2", &[name, &not_accepted(err)]));
            }
            let mut msg = if now_on {
                trf("▶️ Enabled: %1", &[name])
            } else {
                trf("⏸️ Disabled: %1", &[name])
            };
            // Когда это станет видно — то же условие, что в подписи раздела.
            if let Some(note) = refresh_note(io, row, ctx, name) {
                msg.push_str(" — ");
                msg.push_str(&note);
            }
            (true, msg)
        }
    }
}

/// Отказ «окружение не приняло» — с причиной от команды, если она была.
fn not_accepted(err: Option<String>) -> String {
    err.unwrap_or_else(|| tr("the desktop didn't accept the setting"))
}

/// Окружение планировщика для шагов включения: из него нужны только дом и
/// поиск программ (шагов копирования здесь нет).
fn with_ext_env<R>(io: &dyn LookIo, ctx: &Ctx, body: impl FnOnce(&crate::ext_manager::ExtEnv) -> R) -> R {
    use crate::ext_manager::{ExtEnv, FsSnap, TmpAlloc};
    let fs = FsSnap::from_json(&serde_json::json!({}));
    let tmp = TmpAlloc::new("/tmp/rexon-de-apply");
    let which = |name: &str| io.have(name).then(|| name.to_string());
    let run_ok = |_: &[String]| false;
    // Запущенный стол — из контекста (его и проверяют сверки); переменная
    // процесса — только если стол не опознан.
    let xdg = if ctx.de.is_empty() { std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default() } else { ctx.de.clone() };
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let env = ExtEnv {
        home: &ctx.home,
        fs: &fs,
        which: &which,
        run_ok: &run_ok,
        xdg_current_desktop: &xdg,
        xdg_session_type: &session,
        wayland_display: ctx.wayland,
        stage_helper_available: false,
        system_helper_path: "",
        system_helper_exists: false,
        helper_family: "arch",
        tmp: &tmp,
    };
    body(&env)
}

/// Шаги включения объекта кнопкой (видны сверкам).
pub fn enable_steps(io: &dyn LookIo, ctx: &Ctx, kind: &str, name: &str) -> Vec<crate::ext_manager::ExtStep> {
    if kind == NEMO_KIND {
        let (schema, key) = NEMO_DISABLED;
        return vec![sh(dec::gsettings_list_remove_script(schema, key, &format!("'{name}'")))];
    }
    if kind == OPENBOX_KIND {
        return rc_theme_steps(io, ctx, name).unwrap_or_default();
    }
    with_ext_env(io, ctx, |env| crate::ext_manager::de_enable_steps(env, kind, name))
}

/// Шаги выключения объекта кнопкой (видны сверкам).
pub fn disable_steps(io: &dyn LookIo, ctx: &Ctx, kind: &str, name: &str) -> Vec<crate::ext_manager::ExtStep> {
    if kind == NEMO_KIND {
        let (schema, key) = NEMO_DISABLED;
        return vec![sh(dec::gsettings_list_add_script(schema, key, name, &format!("'{name}'")))];
    }
    with_ext_env(io, ctx, |env| crate::ext_manager::de_disable_steps(env, kind, name))
}

fn sh(script: String) -> crate::ext_manager::ExtStep {
    crate::ext_manager::ExtStep::Run {
        argv: vec!["bash".into(), "-c".into(), script],
        timeout_secs: Some(30),
    }
}

/// Включено ли дополнение сейчас (правда окружения).
pub fn addon_enabled(io: &dyn LookIo, ctx: &Ctx, kind: &str, name: &str) -> bool {
    match mode(kind) {
        Some(Mode::Addon(row)) => enabled_addons(io, ctx, kind, row).iter().any(|n| n == name),
        _ => false,
    }
}

/// Включить или выключить дополнение (вкладка «Расширения» — пакет целиком).
pub fn set_addon(io: &dyn LookIo, ctx: &Ctx, kind: &str, name: &str, on: bool) -> Result<(), String> {
    let steps = if on {
        enable_steps(io, ctx, kind, name)
    } else {
        disable_steps(io, ctx, kind, name)
    };
    let err = run_steps(io, &steps);
    if addon_enabled(io, ctx, kind, name) == on {
        Ok(())
    } else {
        Err(format!("{name}: {}", not_accepted(err)))
    }
}

/// Нажатие кнопки раздела: применить тему, включить/выключить дополнение,
/// запустить/остановить Conky. `(получилось, сообщение для человека)`.
///
/// Направление у дополнений и Conky решает НЕ кнопка, а окружение: включено
/// сейчас — выключаем, нет — включаем. Плитка могла устареть (человек
/// успел переключить в настройках самого окружения), а кнопка обязана
/// сделать то, что на ней написано по правде.
///
/// Тема и дополнение — план ([`press_plan`]), общий исполнитель
/// ([`exec_plan`], здесь без отката: «Применить» вкладки исполняет тот же
/// план под откатом сам) и сверка ([`check`]).
pub fn press(io: &dyn LookIo, ctx: &Ctx, kind: &str, name: &str) -> (bool, String) {
    match press_plan(io, ctx, kind, name) {
        Plan::Refused(m) => (false, m),
        Plan::Steps { steps, check: c } => match exec_plan(io, &steps, &mut crate::install_plan::Undo::off()) {
            Ok(err) => check(io, ctx, &c, err),
            Err(f) => (false, trf("Couldn't apply “%1”: %2", &[&name, &crate::install_plan::failure_text(&f)])),
        },
        Plan::Own => {
            if conky_running_ids(io, ctx).iter().any(|i| i == name) {
                conky_stop(io, ctx, name)
            } else {
                conky_start(io, ctx, name)
            }
        }
    }
}

/// Тема оболочки GNOME без расширения User Themes: схемы
/// [`USER_THEME_SCHEMA`] нет, `gsettings set` отказывает с «No such schema»,
/// и человеку надо сказать, чего не хватает, а не пересказать ошибку
/// gsettings. `None` — тип не про оболочку, схема есть или спросить некого
/// (без gsettings запись откажет сама и назовёт причину).
fn missing_user_themes(io: &dyn LookIo, row: &Row) -> Option<String> {
    let Activate::Gsettings { schema, .. } = row.activate else { return None };
    if schema != USER_THEME_SCHEMA || !io.have("gsettings") {
        return None;
    }
    if io.run(&argv(&["gsettings", "list-keys", schema])).0 {
        return None;
    }
    // Расширения оболочки лежат в разделе «Оформления», а не во вкладке
    // «Расширения» — там только пакеты .rxn.
    let place = format!("{} → {}", tr("Appearance"), crate::i18n::trc("appearance section", "GNOME Extensions"));
    Some(trf(
        // TRANSLATORS: %1 is the place in Rexon, e.g. “Appearance → GNOME Extensions”.
        "GNOME Shell themes need the User Themes extension — install it and turn it on in “%1”",
        &[&place],
    ))
}

/// Каталоги `metacity-1` темы `name` в порядке поиска Marco: `~/.themes`,
/// `$XDG_DATA_HOME/themes`, затем каталоги `$XDG_DATA_DIRS`.
pub fn marco_theme_dirs(ctx: &Ctx, name: &str) -> Vec<String> {
    let data_home = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), &ctx.home);
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
    let mut roots = vec![format!("{}/.themes", ctx.home), format!("{data_home}/themes")];
    roots.extend(data_dirs.split(':').filter(|d| !d.is_empty()).map(|d| format!("{d}/themes")));
    roots.into_iter().map(|r| format!("{r}/{name}/metacity-1")).collect()
}

/// Почему Marco тему рамок не загрузит
/// ([`crate::local_preview::marco_theme_problem`]); `None` — помехи нет.
pub fn marco_theme_problem(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<crate::local_preview::MarcoProblem> {
    crate::local_preview::marco_theme_problem(&marco_theme_dirs(ctx, name), &|path| io.read(path))
}

/// Подпись кнопки рамки для человека (точная функция кнопки — в скобках
/// рядом, поэтому парам «un…» хватает подписи основной кнопки). Подписи
/// общих кнопок — те же, что в настройке кнопок заголовка
/// ([`crate::window_buttons::MANAGED`]).
fn marco_button_label(function: &str) -> String {
    let managed = match function {
        "above" | "unabove" => "above",
        "stick" | "unstick" => "sticky",
        other => other,
    };
    if let Some((_, label)) = crate::window_buttons::MANAGED.iter().find(|(id, _)| *id == managed) {
        return tr(label);
    }
    match function {
        "appmenu" => crate::i18n::trc("window frame button", "Application menu"),
        "shade" | "unshade" => crate::i18n::trc("window frame button", "Roll up into title bar"),
        other => other.to_string(),
    }
}

/// Отказ применить тему рамок, которую Marco не загрузит, — для человека.
pub fn marco_refusal(name: &str, problem: &crate::local_preview::MarcoProblem) -> String {
    use crate::local_preview::MarcoProblem;
    match problem {
        MarcoProblem::UnknownButton(function) => trf(
            "Marco can't load “%1”: the theme has a “%2” button Marco doesn't know, so MATE would keep the old window borders",
            &[&name, function],
        ),
        MarcoProblem::MissingButton(function) => {
            let label = marco_button_label(function);
            let button = if label == *function { label } else { format!("{label} ({function})") };
            trf(
                // TRANSLATORS: %2 is a window frame button, e.g. “Roll up into title bar (shade)”.
                "Marco doesn't accept “%1”: the theme has no “%2” button, which Marco requires, so MATE would keep the old window borders",
                &[&name, &button],
            )
        }
        MarcoProblem::MissingPart(part) => trf(
            // TRANSLATORS: %2 is a part of the theme file as written there, e.g. “<author>”.
            "Marco doesn't accept “%1”: the theme file lacks the required part %2, so MATE would keep the old window borders",
            &[&name, part],
        ),
    }
}

/// Когда изменение станет видно, если не сразу.
///
/// GNOME включает и выключает расширение сразу, если уже знает его: оно
/// было на месте при входе в сеанс (проверено на gnome-shell 51.1,
/// 16.10.2026). Перезаход на Wayland нужен только расширению, которое
/// поставили после входа, — его оболочка ещё не видела.
fn refresh_note(io: &dyn LookIo, row: &Row, ctx: &Ctx, name: &str) -> Option<String> {
    use dec::Refresh;
    match row.refresh {
        Refresh::GnomeShell if ctx.wayland && !gnome_shell_knows(io, name) => {
            Some(tr("on Wayland GNOME picks this up after you log out and back in"))
        }
        Refresh::CinnamonReplace if ctx.wayland => {
            Some(tr("Cinnamon picks this up after you log out and back in"))
        }
        _ => None,
    }
}

/// Знает ли запущенная оболочка GNOME расширение `uuid`. Не ответила —
/// «не знает»: лишняя оговорка про перезаход безопаснее ложного «готово».
fn gnome_shell_knows(io: &dyn LookIo, uuid: &str) -> bool {
    let (ok, out) = io.run(&argv(&[
        "gdbus",
        "call",
        "--session",
        "--dest",
        "org.gnome.Shell",
        "--object-path",
        "/org/gnome/Shell",
        "--method",
        "org.gnome.Shell.Extensions.GetExtensionInfo",
        uuid,
    ]));
    // Неизвестный uuid — пустой словарь `(@a{sv} {},)`, известный — с `state`.
    ok && out.contains("'state': <")
}

// ============================================================================
// Conky
// ============================================================================

/// Каталог виджетов Conky (туда кладёт установка).
pub fn conky_root(ctx: &Ctx) -> String {
    dec::row(CONKY_KIND)
        .map(|r| r.dest_path(&ctx.home))
        .unwrap_or_else(|| format!("{}/.config/conky", ctx.home))
}

/// Файл настроек виджета: `conky.conf`, `.conkyrc`, `*.conkyrc`, затем
/// любой `*conky*.conf` уровнем ниже — тот же порядок, что у установки.
///
/// Виджет, найденный на диске вне каталога Rexon (имя — абсолютный путь,
/// `conky_scan`), и тема, где файл назван как угодно (`Jhalak.conf`,
/// `Gotham`), — по содержимому ([`crate::conky_scan::config_files`]).
pub fn conky_conf(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Option<String> {
    if crate::conky_scan::is_found_name(id) {
        return crate::conky_scan::widget_confs(io, ctx, id, None).into_iter().next();
    }
    let dir = format!("{}/{id}", conky_root(ctx));
    let mut names = io.list_dir(&dir);
    names.sort();
    if let Some(f) = names
        .iter()
        .find(|f| *f == "conky.conf" || *f == ".conkyrc" || f.ends_with(".conkyrc"))
    {
        return Some(format!("{dir}/{f}"));
    }
    let looks = |f: &str| {
        f.to_lowercase().contains("conky") && (f.ends_with(".conf") || f.ends_with(".conkyrc"))
    };
    if let Some(f) = names.iter().find(|f| looks(f)) {
        return Some(format!("{dir}/{f}"));
    }
    for sub in &names {
        let sub_dir = format!("{dir}/{sub}");
        let mut inner = io.list_dir(&sub_dir);
        inner.sort();
        if let Some(f) = inner.iter().find(|f| looks(f)) {
            return Some(format!("{sub_dir}/{f}"));
        }
    }
    crate::conky_scan::config_files(io, &dir).into_iter().next()
}

/// Файл настроек из командной строки процесса conky (`-c X`, `--config=X`,
/// `--config X`). `None` — это не conky либо он без своего файла. Conky,
/// запущенный с накладкой Rexon, отвечает файлом автора
/// ([`crate::conky_overlay::original_of_launch`]): для «Остановить»,
/// «Запущенные» и автозапуска это тот же виджет.
pub fn conky_conf_of_cmdline(cmdline: &str) -> Option<String> {
    let launch = conky_launch_of_cmdline(cmdline)?;
    Some(crate::conky_overlay::original_of_launch(&launch).unwrap_or(launch))
}

/// Файл из `-c` командной строки conky как есть (накладка — накладкой).
pub fn conky_launch_of_cmdline(cmdline: &str) -> Option<String> {
    let args: Vec<&str> = cmdline.split('\0').filter(|a| !a.is_empty()).collect();
    let prog = args.first()?.rsplit('/').next()?;
    if prog != "conky" {
        return None;
    }
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if *a == "-c" || *a == "--config" {
            return it.next().map(|s| s.to_string());
        }
        if let Some(v) = a.strip_prefix("--config=") {
            return Some(v.to_string());
        }
        if let Some(v) = a.strip_prefix("-c").filter(|v| !v.is_empty()) {
            return Some(v.to_string());
        }
    }
    None
}

/// Процессы conky: `(pid, файл настроек)`.
fn conky_processes(io: &dyn LookIo) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pid in io.list_dir("/proc") {
        if !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Some(cmd) = io.read(&format!("/proc/{pid}/cmdline")) else { continue };
        // Копия на невидимом экране снимков (`conky_preview`) — не виджет.
        if conky_launch_of_cmdline(&cmd).is_some_and(|l| crate::conky_preview::is_preview_launch(&l)) {
            continue;
        }
        if let Some(conf) = conky_conf_of_cmdline(&cmd) {
            out.push((pid, conf));
        }
    }
    out
}

/// Виджет, которому принадлежит файл настроек (`<корень>/<id>/…`).
fn conky_id_of_conf(ctx: &Ctx, conf: &str) -> Option<String> {
    let root = format!("{}/", conky_root(ctx));
    let rest = conf.strip_prefix(&root)?;
    let id = rest.split('/').next()?;
    (!id.is_empty() && rest.contains('/')).then(|| id.to_string())
}

/// Виджеты Conky, которые работают сейчас.
pub fn conky_running_ids(io: &dyn LookIo, ctx: &Ctx) -> Vec<String> {
    let mut ids = Vec::new();
    for (_, conf) in conky_processes(io) {
        if let Some(id) = conky_id_of_conf(ctx, &conf) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    // Найденные на диске (`~/.conky`, `~/.conkyrc`…) — по пути.
    let extra = crate::conky_scan::extra_dirs_for(ctx);
    for owner in crate::conky_scan::running_found(io, ctx, &extra) {
        if !ids.contains(&owner) {
            ids.push(owner);
        }
    }
    ids
}

// ── Пути «чужой папки» в файлах виджета ─────────────────────────────────
//
// Темы магазина писались под установку в `~/.conky/<Имя>/` (так раскладывали
// архивы программы-менеджера Conky и инструкции авторов) и прописывают туда
// абсолютные пути: `lua_load ~/.conky/<Имя>/rings.lua`, `${image
// ~/.conky/<Имя>/bg.png}`, `$HOME/.conky/<Имя>/weather.sh`, а то и
// `/home/<автор>/.conky/…`. Rexon ставит виджет в `~/.config/conky/<id>/`,
// и такие пути ведут мимо. Ремонт — в файлах самого виджета: путь, который
// никуда не ведёт, но чей «хвост» есть в папке виджета, переписывается на
// эту папку. Остальное не трогается:
//   * путь, который уже существует (у человека и правда есть
//     `~/.conky/<Имя>`), — остаётся как есть;
//   * путь, уже ведущий в папку виджета, — тоже (повторный ремонт ничего не
//     меняет, его можно звать перед каждым запуском);
//   * «хвоста» в папке виджета нет — не наш файл, правка не угадывается;
//   * подстановка дала бы пробел или кавычку там, где их не было, — строка
//     разбилась бы на два аргумента; оставляем.
// В `~/.conky` при этом ничего не создаётся — чужое там не трогается, и при
// удалении виджета убирать за собой нечего.

/// Корни, под которые писались темы (после `~/`, `$HOME/` или
/// `/home/<кто-то>/`).
const CONKY_FOREIGN_ROOTS: &[&str] = &[".conky/", ".Conky/", ".config/conky/", ".config/Conky/"];

/// Конец пути в тексте конфига, Lua или сценария.
fn conky_path_end(c: char) -> bool {
    c.is_whitespace() || "'\"`}{)(][,;|<>&$\\=".contains(c)
}

/// Начало пути `~/`, `$HOME/`, `${HOME}/` или `/home/<имя>/` в позиции `i`
/// строки `text`: `(длина префикса, абсолютный префикс-дом или None —
/// «дом этого человека»)`.
fn conky_home_prefix(text: &str, i: usize) -> Option<(usize, Option<String>)> {
    let rest = &text[i..];
    for p in ["~/", "$HOME/", "${HOME}/"] {
        if rest.starts_with(p) {
            return Some((p.len(), None));
        }
    }
    let user = rest.strip_prefix("/home/")?;
    let n = user
        .find(|c: char| !(c.is_ascii_alphanumeric() || "._-".contains(c)))
        .unwrap_or(user.len());
    if n == 0 || !user[n..].starts_with('/') {
        return None;
    }
    Some((6 + n + 1, Some(format!("/home/{}", &user[..n]))))
}

/// Починить пути одного текста. `home` — домашняя папка, `widget` —
/// папка виджета (абсолютная, без `/` в конце), `exists`/`is_dir` —
/// наблюдения диска. `None` — менять нечего.
pub fn conky_repair_text(
    text: &str,
    home: &str,
    widget: &str,
    exists: &dyn Fn(&str) -> bool,
    is_dir: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut last = 0;
    let mut i = 0;
    while i < text.len() {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let prev_ok = text[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || "_./~$-".contains(c)));
        let found = prev_ok.then(|| conky_home_prefix(text, i)).flatten();
        let Some((plen, author_home)) = found else {
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            continue;
        };
        let after = &text[i + plen..];
        let Some(root) = CONKY_FOREIGN_ROOTS.iter().find(|r| after.starts_with(**r)) else {
            i += plen;
            continue;
        };
        let tail_all = &after[root.len()..];
        // В кавычках путь идёт до закрывающей кавычки (пробелы в нём —
        // часть имени), без кавычек — до первого разделителя.
        let quote = text[..i].chars().next_back().filter(|c| *c == '"' || *c == '\'');
        let tail_len = match quote {
            Some(q) => tail_all.find(|c: char| c == q || c == '\n'),
            None => tail_all.find(conky_path_end),
        }
        .unwrap_or(tail_all.len());
        let tail = &tail_all[..tail_len];
        let end = i + plen + root.len() + tail_len;
        let original_home = author_home.clone().unwrap_or_else(|| home.to_string());
        let original = format!("{original_home}/{root}{tail}");
        let parts: Vec<&str> = tail.split('/').filter(|p| !p.is_empty()).collect();
        let inside = |p: &str| p == widget || p.starts_with(&format!("{widget}/"));
        let pick = (!parts.is_empty() && !exists(&original) && !inside(&original))
            .then(|| {
                // Сначала хвост, который есть целиком, — от самого длинного.
                (0..parts.len())
                    .map(|k| parts[k..].join("/"))
                    .find(|s| exists(&format!("{widget}/{s}")))
                    // Затем файл, который виджет пишет сам (погода, кэш):
                    // его ещё нет, но есть его папка.
                    .or_else(|| {
                        (0..parts.len().saturating_sub(1))
                            .find(|&k| is_dir(&format!("{widget}/{}", parts[k..parts.len() - 1].join("/"))))
                            .map(|k| parts[k..].join("/"))
                    })
            })
            .flatten();
        if let Some(sub) = pick {
            let slash = if tail.ends_with('/') { "/" } else { "" };
            let target = format!("{widget}/{sub}{slash}");
            // Тот же вид начала пути, что у автора: `~/…` остаётся `~/…`.
            let head = &text[i..i + plen];
            let written = match (author_home.is_some(), target.strip_prefix(&format!("{home}/"))) {
                (false, Some(rel)) => format!("{head}{rel}"),
                _ => target.clone(),
            };
            let breaks = |s: &str| s.contains(|c: char| c.is_whitespace() || "'\"".contains(c));
            let fits = match quote {
                Some(q) => !written.contains(q),
                None => !breaks(&written),
            };
            if fits {
                out.push_str(&text[last..i]);
                out.push_str(&written);
                last = end;
                changed = true;
            }
        }
        i = end.max(i + 1);
    }
    if !changed {
        return None;
    }
    out.push_str(&text[last..]);
    Some(out)
}

/// Починить пути во всех текстовых файлах папки виджета `widget` (см.
/// [`conky_repair_text`]). Возвращает изменённые файлы. Ссылки не
/// проходятся, двоичные и большие файлы не читаются.
pub fn conky_repair_paths(home: &str, widget: &str) -> Vec<String> {
    const MAX_FILES: usize = 4000;
    const MAX_SIZE: u64 = 1 << 20;
    let exists = |p: &str| std::path::Path::new(p).exists();
    let is_dir = |p: &str| std::path::Path::new(p).is_dir();
    let mut changed = Vec::new();
    let mut stack = vec![(widget.trim_end_matches('/').to_string(), 0usize)];
    let widget = widget.trim_end_matches('/');
    let mut seen = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            seen += 1;
            if seen > MAX_FILES {
                return changed;
            }
            let Ok(ft) = e.file_type() else { continue };
            let path = e.path().to_string_lossy().into_owned();
            if ft.is_dir() && depth < 8 {
                stack.push((path, depth + 1));
                continue;
            }
            if !ft.is_file() || e.metadata().map_or(true, |m| m.len() > MAX_SIZE) {
                continue;
            }
            // Установщик автора (`install.sh`) описывает, КУДА класть тему, —
            // переписанный, он копировал бы виджет сам в себя.
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.starts_with("install") || name.starts_with("uninstall") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else { continue };
            if bytes.contains(&0) {
                continue;
            }
            let Ok(text) = String::from_utf8(bytes) else { continue };
            if let Some(fixed) = conky_repair_text(&text, home, widget, &exists, &is_dir) {
                if std::fs::write(&path, fixed).is_ok() {
                    changed.push(path);
                }
            }
        }
    }
    changed
}

/// Рабочий каталог conky для файла настроек `conf` — его папка. Виджеты
/// пишут пути к своим файлам от неё (`lua_load ./x.lua`, `${image
/// ./bg.png}`, `${execi 60 ./weather.sh}`): conky и запущенные им команды
/// ищут их от рабочего каталога процесса. Так же запускают виджет программы,
/// под которые темы рисовались (переход в папку файла, затем conky).
pub fn conky_work_dir(conf: &str) -> &str {
    match conf.rsplit_once('/') {
        Some(("", _)) => "/",
        Some((dir, _)) => dir,
        None => ".",
    }
}

/// Строка запуска кнопки «Запустить»: `(argv, рабочий каталог)`.
pub fn conky_start_command(conf: &str) -> (Vec<String>, String) {
    (
        ["conky", "-d", "-c", conf].map(String::from).to_vec(),
        conky_work_dir(conf).to_string(),
    )
}

/// Сценарий `sh -c` автозапуска при входе: `$0` — файл настроек; переход
/// в его папку и замена оболочки на conky с тем же файлом. Путь в сценарий
/// не подставляется — он приходит отдельным аргументом, поэтому пробелы,
/// кавычки и `$` в имени папки экранировать внутри сценария не нужно. После
/// `exec` в `/proc` остаётся сам conky с `-c <файл>`, и «запущен ли виджет»
/// узнаётся как у кнопки.
///
/// Знака `%` в сценарии нет намеренно: в `Exec=` он служебный, и
/// запускальщики по-разному разбирают `%%` внутри кавычек.
pub const CONKY_AUTOSTART_SCRIPT: &str = r#"cd "$(dirname "$0")" && exec conky -c "$0""#;

/// То же для виджета с накладкой Rexon: `$0` — накладка (с ней conky
/// запускается), `$1` — файл автора (его папка — рабочий каталог).
pub const CONKY_OVERLAY_AUTOSTART_SCRIPT: &str = r#"cd "$(dirname "$1")" && exec conky -c "$0""#;

fn conky_start(io: &dyn LookIo, ctx: &Ctx, id: &str) -> (bool, String) {
    let Some(conf) = conky_conf(io, ctx, id) else {
        return (
            false,
            tr("This Conky widget has no settings file (conky.conf) — there's nothing to start."),
        );
    };
    if !io.have("conky") {
        return (false, conky_missing_refusal());
    }
    conky_prepare(ctx, id);
    // Виджет выполняет команды или сценарии, а человек этого ещё не
    // разрешил (или файлы с тех пор изменились): не запускать. Вопрос
    // задаёт интерфейс до нажатия (`conky_safety::start_question`); сюда
    // доходят запуски мимо него — «Установить и применить» из магазина.
    // Сверка — ПОСЛЕ ремонта путей: он правит файлы, и отпечаток,
    // снятый до него, при следующем запуске уже не совпал бы.
    if let Some(why) = crate::conky_safety::start_refusal(io, ctx, id) {
        return (false, why);
    }
    // Запуск из папки виджета, вывод ошибок — в журнал; не запустился —
    // причина словами conky (`conky_procs::start_conf`).
    crate::conky_procs::start_conf(io, ctx, &crate::conky_scan::widget_title(id), &conf)
}

/// Подготовка виджета к запуску: пути под `~/.conky/<Имя>/` — на папку
/// виджета (виджеты, поставленные раньше этой починки, чинятся здесь же,
/// при первом запуске). Найденный на диске виджет и так лежит там, куда
/// смотрят его пути, — его файлы не трогаются. Интерфейс зовёт её и ДО
/// вопроса о командах виджета: разрешение помнит файлы уже починенными.
pub fn conky_prepare(ctx: &Ctx, id: &str) {
    if !crate::conky_scan::is_found_name(id) {
        conky_repair_paths(&ctx.home, &format!("{}/{id}", conky_root(ctx)));
    }
}

/// Отказ «Запустить», когда программы Conky нет.
pub fn conky_missing_refusal() -> String {
    tr("The widget wasn't started: it needs the Conky program, and Conky isn't installed.")
}

/// Оговорка Conky на Wayland для окружения `de_id`. `None` — оговорка не
/// нужна.
///
/// Conky на Wayland рисует виджет на столе через wlr-layer-shell: его знают
/// KWin, композиторы на wlroots (Sway, Hyprland, labwc — на нём и Budgie
/// 10.10), niri и COSMIC. Mutter (GNOME), Muffin (Cinnamon) и Gala (Pantheon)
/// его не реализуют: там виджет идёт только через XWayland и встаёт как
/// обычное окно — или не встаёт вовсе. Прежняя оговорка «на Wayland идёт
/// через XWayland» про KDE и тайловые устарела, а GNOME ложно обнадёживала.
pub fn conky_wayland_note(de_id: &str, wayland: bool) -> Option<String> {
    conky_wayland_note_for(de_id, wayland, None)
}

/// [`conky_wayland_note`] с версией conky. С 1.25 conky сам выбирает вывод
/// (`output_backend`) и на столе без слоёв встаёт обычным окном Wayland —
/// предупреждение говорит именно это
/// ([`crate::conky_overlay::preset_note`]); у прежних выпусков и без версии
/// — прежнее «ненадёжно».
pub fn conky_wayland_note_for(de_id: &str, wayland: bool, version: Option<(u32, u32)>) -> Option<String> {
    use crate::de_customization::De;
    if !wayland {
        return None;
    }
    if version.is_some_and(|v| v >= (1, 25)) {
        return crate::conky_overlay::preset_note(de_id, wayland).map(|n| format!("⚠️ {n}"));
    }
    match crate::de_customization::de_from_id(de_id)? {
        De::Gnome | De::Cinnamon | De::Pantheon => Some(tr(
            "⚠️ In the Wayland session of this desktop Conky is unreliable: the widget may not \
             appear or may land in the wrong place. The X11 session (chosen on the login screen) \
             works best.",
        )),
        _ => None,
    }
}

/// Подпись-условие раздела «Виджеты Conky»: нет программы — сказать
/// заранее, а не после кнопки «Запустить»; ненадёжный Wayland — тоже.
/// `installable` — Rexon может сам поставить пакет (не SteamOS, не
/// неизменяемая система, знакомый менеджер пакетов). `None` — всё в порядке.
pub fn conky_section_note(installed: bool, installable: bool, de_id: &str, wayland: bool) -> Option<String> {
    let mut parts = Vec::new();
    if !installed {
        parts.push(if installable {
            tr("The Conky program isn't installed yet — Rexon will install it when you start a widget.")
        } else {
            tr("The Conky program isn't installed, and Rexon can't install programs on this system by itself: \
                install the conky package with your system's tools.")
        });
    }
    parts.extend(conky_wayland_note(de_id, wayland));
    // Cinnamon на Wayland: свои виджеты стола у него есть и держатся на
    // столе в любом сеансе — показать дорогу к ним (24.10.2026).
    if wayland && crate::de_customization::de_from_id(de_id) == Some(crate::de_customization::De::Cinnamon) {
        parts.push(crate::i18n::trf(
            // TRANSLATORS: %1 is the name of the neighbouring section (“Cinnamon Desklets”).
            "Desklets stay on the desktop in any session — they're in the neighbouring “%1” section.",
            &[&crate::i18n::trc("appearance section", "Cinnamon Desklets")],
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Пакет программы Conky, который Rexon может поставить на ЭТОЙ системе.
/// `None` — не может: SteamOS, неизменяемая система (системный пакет там не
/// встанет) или незнакомый менеджер пакетов — тогда установку не
/// предлагаем, только предупреждаем.
pub fn live_conky_package() -> Option<&'static str> {
    let detector = crate::system::DistroDetector::detect_live();
    if detector.is_steamos() || detector.is_immutable() {
        return None;
    }
    conky_package(detector.family_live().as_str())
}

/// Виджет ли это Conky — карточка магазина (раздел 124) или местная плитка
/// `local:conky:<имя>`.
pub fn is_conky_item(item: &crate::models::ThemeItem) -> bool {
    apply_kind_of_local_id(&item.id) == Some("conky")
        || crate::categories::section_id_for(item.category_id) == crate::categories::CONKY_SECTION_ID
}

/// Товар раздела Conky — программа, а не виджет: КАЖДЫЙ его файл — пакет
/// дистрибутива или установщик ([`crate::ocs::DISTRO_PACKAGE_EXTS`]: `.deb`,
/// `.rpm`, `.pkg.tar.zst`, `.AppImage`…). Так в категории «Conky» магазина
/// лежат «Conky Manager GTK» и «Conky Manager Qt»: ставить их как виджет
/// нечего, а наверху витрины они шли первыми. У виджета есть хоть один
/// архив или файл настроек — он остаётся, даже если рядом лежит пакет.
pub fn is_conky_program(item: &crate::models::ThemeItem) -> bool {
    if !is_conky_item(item) {
        return false;
    }
    let mut names: Vec<String> = item
        .download_links
        .iter()
        .filter_map(|l| l.get("name").and_then(serde_json::Value::as_str))
        .map(str::to_ascii_lowercase)
        .collect();
    if names.is_empty() && !item.download_name.is_empty() {
        names.push(item.download_name.to_ascii_lowercase());
    }
    !names.is_empty()
        && names
            .iter()
            .all(|n| crate::ocs::DISTRO_PACKAGE_EXTS.iter().any(|e| n.ends_with(e)))
}

/// Пакет Conky, который Rexon ставит перед запуском виджета САМ, без
/// вопроса «Да/Нет» (решение владельца 24.10: в вопросе легко по ошибке
/// нажать «Нет»; защита — окно пароля polkit). `None` — ставить нечего:
/// Conky стоит или поставить его Rexon здесь не может (тогда раздел
/// предупредил заранее, а «Запустить» скажет [`conky_missing_refusal`]).
///
/// `have_conky` — есть ли `conky` в PATH, `package` — [`live_conky_package`].
pub fn conky_install_package(have_conky: bool, package: Option<&str>) -> Option<String> {
    if have_conky {
        return None;
    }
    package.map(str::to_string)
}

/// Поставить Conky тем же путём, что вся установка пакетов Rexon (pkexec +
/// `rexon-pkg-helper`), только из официальных репозиториев
/// (`appearance_installer::live_install_repo_packages`: AUR без вопроса не
/// собирается), и проверить по факту — бинарь `conky` в PATH. `Err` —
/// причина для человека (виджет после неё не запускается).
pub fn install_conky_live() -> Result<(), String> {
    let Some(package) = live_conky_package() else {
        return Err(tr("Rexon can't install programs on this system by itself."));
    };
    crate::appearance_installer::live_install_repo_packages(
        &[package.to_string()],
        crate::privilege::op::PkgSource::Conky,
    )?;
    if !have_conky() {
        return Err(trf("the %1 package was installed, but the conky program didn't appear", &[&package]));
    }
    Ok(())
}

/// Есть ли программа `conky` в PATH процесса. `utils::which(_, None)` ищет
/// по запасному списку каталогов, а не по `$PATH`: Conky, поставленный не в
/// `/usr/bin` (или подложенный стендом), он бы не увидел.
pub fn have_conky() -> bool {
    crate::utils::which("conky", std::env::var("PATH").ok().as_deref()).is_some()
}

/// Итог неудачной установки Conky: виджет не запущен, и сказано почему и
/// что сделать. Окно пароля закрыто — «передумал», а не поломка: совет
/// повторить «▶️ Запустить» и ввести пароль.
pub fn conky_install_failed(why: &str) -> String {
    if why == crate::appearance_installer::password_not_confirmed() {
        return trf(
            // TRANSLATORS: %1 is the label of the widget's start button.
            "The widget wasn't started: it needs the Conky program. Press “%1” again and enter the \
             password to install it.",
            &[&tr("▶️ Start")],
        );
    }
    trf("Couldn't install Conky: %1. The widget wasn't started.", &[&why])
}

/// Пакет программы Conky для семейства пакетов (`system::PkgFamily::as_str`).
/// `None` — Rexon его здесь не ставит. Имена сверены с репозиториями:
/// Arch (extra), Debian/Ubuntu, Fedora и openSUSE — везде `conky`.
pub fn conky_package(family: &str) -> Option<&'static str> {
    match family {
        "arch" | "debian" | "fedora" | "suse" => Some("conky"),
        _ => None,
    }
}

fn conky_stop(io: &dyn LookIo, ctx: &Ctx, id: &str) -> (bool, String) {
    // Гасим ТОЛЬКО процессы этого виджета — по номеру процесса, найденному по
    // его файлу настроек. Чужие conky (свои у человека) не трогаем.
    // В сообщении — имя виджета, а не путь найденного на диске.
    let title = crate::conky_scan::widget_title(id);
    let pids: Vec<String> = if crate::conky_scan::is_found_name(id) {
        crate::conky_procs::pids_where(io, ctx, &|conf| crate::conky_scan::widget_owns(ctx, id, conf))
    } else {
        conky_processes(io)
            .into_iter()
            .filter(|(_, conf)| conky_id_of_conf(ctx, conf).as_deref() == Some(id))
            .map(|(pid, _)| pid)
            .collect()
    };
    if pids.is_empty() {
        return (true, trf("⏹️ Stopped: %1", &[&title]));
    }
    let mut cmd = vec!["kill".to_string()];
    cmd.extend(pids);
    let (ok, out) = io.run(&cmd);
    if !ok {
        return (false, trf("Couldn't stop “%1”: %2", &[&title, &out.trim().to_string()]));
    }
    (true, trf("⏹️ Stopped: %1", &[&title]))
}

/// Перед «Удалить»: погасить процессы виджета (по номерам, как
/// «Остановить») и снять его запись автозапуска — иначе удалённый виджет
/// продолжал бы висеть на столе, а при входе автозапуск звал бы conky с
/// файлом, которого нет. `widget_dir` — путь из журнала установки; не
/// папка виджета (`<корень Conky>/<id>`) — ничего не делается. `Some(id)` —
/// виджет опознан.
pub fn conky_forget_widget(io: &dyn LookIo, ctx: &Ctx, widget_dir: &str) -> Option<String> {
    let root = format!("{}/", conky_root(ctx));
    let id = widget_dir.trim_end_matches('/').strip_prefix(&root)?;
    if id.is_empty() || id.contains('/') || id == "." || id == ".." {
        return None;
    }
    let _ = conky_stop(io, ctx, id);
    conky_autostart_forget(ctx, id);
    crate::conky_overlay::forget_under(ctx, widget_dir);
    Some(id.to_string())
}

/// Файл автозапуска виджета (имя то же, что писала прежняя установка, —
/// уже включённые раньше виджеты остаются включёнными).
pub fn conky_autostart_path(ctx: &Ctx, id: &str) -> String {
    ctx.config(&format!("autostart/rexon-conky-{}.desktop", crate::conky_scan::autostart_key(id)))
}

/// Запись автозапуска виджета `id` для файла `conf` с настройками `opts`
/// (строки композиторов ставятся тут же, [`crate::conky_autostart::build`]).
fn conky_autostart_built(io: &dyn LookIo, ctx: &Ctx, id: &str, conf: &str, opts: &crate::conky_autostart::Options) -> crate::conky_autostart::Built {
    let p = crate::conky_overlay::live_paths(ctx, conf);
    let launch = crate::conky_overlay::autostart_launch(io, &p);
    let env = crate::conky_autostart::session_env(ctx);
    let key = crate::conky_scan::autostart_key(id);
    crate::conky_autostart::build(&env, &key, &crate::conky_scan::widget_title(id), conf, launch.as_deref(), opts)
}

/// Убрать автозапуск виджета: запись и строки композиторов.
pub fn conky_autostart_forget(ctx: &Ctx, id: &str) {
    let _ = std::fs::remove_file(conky_autostart_path(ctx, id));
    crate::conky_autostart::remove(&crate::conky_autostart::session_env(ctx), &crate::conky_scan::autostart_key(id));
}

/// Включён ли автозапуск виджета.
///
/// Запись прежнего вида (без перехода в папку виджета, без
/// `X-KDE-autostart-after`) заодно переписывается на нынешнюю с теми же
/// задержкой и окружением: включённый раньше виджет иначе так и запускался
/// бы при входе по-старому. Нынешняя запись не трогается.
pub fn conky_autostart(io: &dyn LookIo, ctx: &Ctx, id: &str) -> bool {
    let path = conky_autostart_path(ctx, id);
    let Some(text) = io.read(&path) else { return false };
    if text.contains("\nExec=conky -c ") || !text.contains("\nX-KDE-autostart-after=") {
        if let Some(conf) = conky_conf(io, ctx, id) {
            let opts = crate::conky_autostart::options_of(&text);
            io.write(&path, &conky_autostart_built(io, ctx, id, &conf, &opts).desktop);
        }
    }
    true
}

/// Переключатель «Запускать при входе». `(получилось, сообщение)`. Новая
/// запись — с задержкой [`crate::conky_autostart::DEFAULT_DELAY`].
pub fn set_conky_autostart(io: &dyn LookIo, ctx: &Ctx, id: &str, on: bool) -> (bool, String) {
    let path = conky_autostart_path(ctx, id);
    if !on {
        let ok = match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        };
        crate::conky_autostart::remove(&crate::conky_autostart::session_env(ctx), &crate::conky_scan::autostart_key(id));
        return if ok {
            (true, trf("Conky widget “%1” won't start at login", &[&id]))
        } else {
            (false, trf("Couldn't change autostart of “%1”", &[&id]))
        };
    }
    let opts = io.read(&path).map(|t| crate::conky_autostart::options_of(&t)).unwrap_or(crate::conky_autostart::Options {
        delay: crate::conky_autostart::DEFAULT_DELAY,
        only_in: Vec::new(),
    });
    set_conky_autostart_options(io, ctx, id, &opts)
}

/// Записать автозапуск виджета с настройками `opts` (включает его).
/// `(получилось, сообщение)`; в сеансе композитора, куда строка запуска не
/// легла, сообщение говорит, что вставить руками.
pub fn set_conky_autostart_options(io: &dyn LookIo, ctx: &Ctx, id: &str, opts: &crate::conky_autostart::Options) -> (bool, String) {
    let path = conky_autostart_path(ctx, id);
    let Some(conf) = conky_conf(io, ctx, id) else {
        return (
            false,
            tr("This Conky widget has no settings file (conky.conf) — there's nothing to start."),
        );
    };
    // При входе виджет запустится без Rexon — пути чинятся заранее.
    if !crate::conky_scan::is_found_name(id) {
        conky_repair_paths(&ctx.home, &format!("{}/{id}", conky_root(ctx)));
    }
    let built = conky_autostart_built(io, ctx, id, &conf, opts);
    if !io.write(&path, &built.desktop) {
        return (false, trf("Couldn't change autostart of “%1”", &[&id]));
    }
    let title = crate::conky_scan::widget_title(id);
    match built.refusal {
        Some(why) => (false, trf("Conky widget “%1” is set to start at login, but not in this session: %2", &[&title, &why])),
        None if built.configs.is_empty() => (true, trf("✅ Conky widget “%1” will start when you log in", &[&title])),
        // TRANSLATORS: %1 is a Conky widget name, %2 a settings file of a window manager (sway, Hyprland…).
        None => (true, trf("✅ Conky widget “%1” will start when you log in — the start line is in %2", &[&title, &built.configs.join(", ")])),
    }
}

// ============================================================================
// Экран входа и заставка загрузки — строка вызова помощника
// ============================================================================

/// `pkexec <помощник> [--lang=…] apply-<вид> <тема> - <семейство>`: режим
/// «Применить» системного помощника — тема уже стоит в `/usr/share/<вид>/
/// themes`, её только выбирают. Тот же помощник и то же действие polkit,
/// что у установки, — пароль спрашивается тем же «одним паролем»
/// (`auth_admin_keep`), новых запросов не появляется.
pub fn system_apply_argv(
    helper: &str,
    lang_args: &[String],
    kind: &str,
    theme: &str,
    family: &str,
) -> Vec<String> {
    system_mode_argv(helper, lang_args, "apply", kind, theme, family)
}

/// `pkexec <помощник> [--lang=…] remove-<вид> <тема> - <семейство>`: режим
/// «Удалить» того же помощника — снять `/usr/share/<вид>/themes/<тема>`.
/// Активную тему и файлы пакета системы помощник не трогает сам; проверка
/// до окна пароля — у вызывающего
/// ([`crate::appearance_installer::system_theme_remove_with`]).
pub fn system_remove_argv(
    helper: &str,
    lang_args: &[String],
    kind: &str,
    theme: &str,
    family: &str,
) -> Vec<String> {
    system_mode_argv(helper, lang_args, "remove", kind, theme, family)
}

/// Общая строка вызова режимов помощника, которым каталог не нужен.
fn system_mode_argv(
    helper: &str,
    lang_args: &[String],
    mode: &str,
    kind: &str,
    theme: &str,
    family: &str,
) -> Vec<String> {
    let mut v = vec!["pkexec".to_string(), helper.to_string()];
    v.extend(lang_args.iter().cloned());
    v.extend([format!("{mode}-{kind}"), theme.to_string(), "-".to_string(), family.to_string()]);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_follow_the_matrix() {
        for k in ["theme-xfwm4", "theme-marco", "theme-gnome-shell", "theme-cinnamon", "theme-lxqt", "kde-window-switcher"] {
            assert_eq!(mode(k).map(Mode::button), Some(Button::Apply), "{k}");
        }
        for k in [
            "kde-kwin-script", "kde-kwin-effect", "gnome-shell-extension", "cinnamon-applet",
            "cinnamon-desklet", "cinnamon-extension", "nemo-action",
        ] {
            assert_eq!(mode(k).map(Mode::button), Some(Button::Toggle), "{k}");
        }
        assert_eq!(mode("theme-conky").map(Mode::button), Some(Button::Run));
        for k in ["kde-wallpaper-plugin", "theme-gtk", "font-pack", "theme-plasmoid"] {
            assert!(mode(k).is_none(), "{k}");
        }
    }

    #[test]
    fn conky_programs_are_told_from_widgets() {
        use crate::models::ThemeItem;
        let item = |names: &[&str]| ThemeItem {
            id: "1".into(),
            category_id: crate::categories::CONKY_SECTION_ID,
            download_links: names.iter().map(|n| serde_json::json!({"url": "u", "name": n})).collect(),
            ..Default::default()
        };
        // «Conky Manager Qt» с магазина: одни пакеты и AppImage.
        assert!(is_conky_program(&item(&[
            "conky-manager-1.0-2-x86-64.AppImage",
            "conky-manager-q-1.0-2-x86-64.deb",
            "conky-manager-q-1.0-2-x86-64.pkg.tar.zst",
            "conky-manager-q-1.0-2.x86-64.rpm",
        ])));
        // Виджеты остаются: архив, голый файл настроек, архив рядом с пакетом.
        assert!(!is_conky_program(&item(&["Mimosa-AmethystC-v4.0.zip"])));
        assert!(!is_conky_program(&item(&["conky.conf", "readme.txt"])));
        assert!(!is_conky_program(&item(&["3D-Plus.tar.xz"])));
        assert!(!is_conky_program(&item(&["w.zip", "w-1.0.deb"])));
        // Файлов нет — признака нет.
        assert!(!is_conky_program(&item(&[])));
        // Не раздел Conky — не наше дело.
        let mut other = item(&["x.deb"]);
        other.category_id = 107;
        assert!(!is_conky_program(&other));
    }

    #[test]
    fn gsettings_lists_parse() {
        assert_eq!(parse_gsettings_list("@as []"), Vec::<String>::new());
        assert_eq!(parse_gsettings_list("['a@b', 'c']"), vec!["a@b", "c"]);
        assert_eq!(
            entry_uuid("panel1:right:3:weather@mockturtl:12", EntryFmt::CinnamonApplet).as_deref(),
            Some("weather@mockturtl")
        );
        assert_eq!(entry_uuid("clock@cinnamon.org:0:100:100", EntryFmt::CinnamonDesklet).as_deref(), Some("clock@cinnamon.org"));
    }

    /// Файл настроек с любым именем узнаётся по содержимому (у Jhalak —
    /// `Jhalak.conf`, рядом `cava.conf` визуализатора), а плитка «Найдено на
    /// диске» — путь — ведёт в свой каталог; её автозапуск — файл без косых
    /// черт в имени.
    #[test]
    fn conky_conf_by_content_and_for_found_widgets() {
        use crate::animations::SystemIo;
        let base = std::env::temp_dir().join(format!("rexon-conky-conf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let put = |rel: &str, text: &str| {
            let p = base.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        put(".config/conky/Jhalak/cava.conf", "[general]\nbars = 128\n");
        put(".config/conky/Jhalak/Jhalak.conf", "-- JHALAK\nconky.config = {}\nconky.text = [[x]]\n");
        put(".conky/Gotham/Gotham", "TEXT\n${time}\n");
        put(".conkyrc", "TEXT\n");
        let h = base.to_string_lossy().into_owned();
        let ctx = Ctx { home: h.clone(), xdg_config_home: None, wayland: false, de: String::new() };
        assert_eq!(conky_conf(&SystemIo, &ctx, "Jhalak"), Some(format!("{h}/.config/conky/Jhalak/Jhalak.conf")));
        assert_eq!(conky_conf(&SystemIo, &ctx, &format!("{h}/.conky/Gotham")), Some(format!("{h}/.conky/Gotham/Gotham")));
        assert_eq!(conky_conf(&SystemIo, &ctx, &format!("{h}/.conkyrc")), Some(format!("{h}/.conkyrc")));
        let auto = conky_autostart_path(&ctx, &format!("{h}/.conky/Gotham"));
        let file = auto.rsplit('/').next().unwrap();
        assert!(auto.starts_with(&format!("{h}/.config/autostart/rexon-conky-found-")) && file.ends_with(".desktop"), "{auto}");
        assert_eq!(conky_autostart_path(&ctx, "Jhalak"), format!("{h}/.config/autostart/rexon-conky-Jhalak.desktop"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn conky_cmdline() {
        assert_eq!(conky_conf_of_cmdline("conky\0-d\0-c\0/h/.config/conky/w/conky.conf\0").as_deref(), Some("/h/.config/conky/w/conky.conf"));
        assert_eq!(conky_conf_of_cmdline("/usr/bin/conky\0--config=/x\0").as_deref(), Some("/x"));
        assert_eq!(conky_conf_of_cmdline("bash\0-c\0conky\0"), None);
        let ctx = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        assert_eq!(conky_id_of_conf(&ctx, "/h/.config/conky/w/sub/conky.conf").as_deref(), Some("w"));
        assert_eq!(conky_id_of_conf(&ctx, "/h/.config/conky/conky.conf"), None);
    }

    /// Кнопка «Запустить» пускает conky из папки файла настроек: виджеты с
    /// `lua_load ./x.lua`, `${image ./bg.png}` и `${execi … ./s.sh}` ищут
    /// свои файлы от рабочего каталога процесса.
    #[test]
    fn conky_starts_in_widget_folder() {
        let (argv, dir) = conky_start_command("/h/.config/conky/My w/sub/conky.conf");
        assert_eq!(argv, ["conky", "-d", "-c", "/h/.config/conky/My w/sub/conky.conf"]);
        assert_eq!(dir, "/h/.config/conky/My w/sub");
        // Файл в корне — корень, а не пустая строка.
        assert_eq!(conky_work_dir("/conky.conf"), "/");
    }

    /// Автозапуск при входе: та же папка, что у кнопки, — и строка запуска
    /// переживает пробелы, кавычки и `$` в пути (прежде путь шёл в
    /// одинарных кавычках, и апостроф в имени виджета ломал строку).
    #[test]
    fn conky_autostart_entry_runs_in_widget_folder() {
        let conf = "/h/.config/conky/It's \"my\" $w/conky.conf";
        let entry = crate::ext_manager::conky_desktop_entry("w", conf);
        let value = |key: &str| {
            entry
                .lines()
                .find_map(|l| l.strip_prefix(key))
                .unwrap_or_else(|| panic!("нет {key} в\n{entry}"))
                .to_string()
        };
        let argv = crate::compositor_autostart::exec_argv(&value("Exec=")).expect("строка Exec разбирается");
        assert_eq!(argv, ["sh", "-c", CONKY_AUTOSTART_SCRIPT, conf]);
        assert_eq!(value("Path="), "/h/.config/conky/It's \"my\" $w");
    }

    /// Строка автозапуска на деле: `sh` из записи заходит в папку виджета и
    /// заменяется conky с тем же файлом (подставной conky пишет, где его
    /// запустили и с чем) — значит, «запущен ли виджет» по `/proc` его
    /// по-прежнему узнаёт.
    #[test]
    fn conky_autostart_script_really_changes_directory() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!("rexon-conky-cwd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let bin = tmp.join("bin");
        let widget = tmp.join("It's \"my\" $w");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&widget).unwrap();
        let log = tmp.join("log");
        let fake = bin.join("conky");
        std::fs::write(&fake, format!("#!/bin/sh\npwd > '{}'\nprintf '%s\\n' \"$@\" >> '{}'\n", log.display(), log.display())).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let conf = widget.join("conky.conf").to_string_lossy().into_owned();
        std::fs::write(&conf, "conky.text = [[x]]\n").unwrap();
        let entry = crate::ext_manager::conky_desktop_entry("w", &conf);
        let exec = entry.lines().find_map(|l| l.strip_prefix("Exec=")).unwrap();
        let argv = crate::compositor_autostart::exec_argv(exec).unwrap();
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let st = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir("/")
            .env("PATH", path)
            .status()
            .unwrap();
        assert!(st.success());
        let got = std::fs::read_to_string(&log).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(got, format!("{}\n-c\n{conf}\n", widget.display()));
    }

    /// Автозапуск, включённый до перехода в папку виджета, переписывается
    /// при первом же взгляде на переключатель; нынешний — не трогается.
    #[test]
    fn old_conky_autostart_entry_is_upgraded() {
        use std::cell::RefCell;
        struct Fake {
            entry: RefCell<String>,
            writes: RefCell<usize>,
        }
        impl LookIo for Fake {
            fn run(&self, _a: &[String]) -> (bool, String) { (true, String::new()) }
            fn have(&self, _t: &str) -> bool { true }
            fn read(&self, p: &str) -> Option<String> {
                (p == "/h/.config/autostart/rexon-conky-w.desktop").then(|| self.entry.borrow().clone())
            }
            fn write(&self, _p: &str, t: &str) -> bool {
                *self.writes.borrow_mut() += 1;
                *self.entry.borrow_mut() = t.to_string();
                true
            }
            fn list_dir(&self, p: &str) -> Vec<String> {
                if p == "/h/.config/conky/w" { vec!["conky.conf".into()] } else { Vec::new() }
            }
        }
        let ctx = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        let old = "[Desktop Entry]\nType=Application\nName=Conky — w\nExec=conky -c '/h/.config/conky/w/conky.conf'\n";
        let io = Fake { entry: RefCell::new(old.into()), writes: RefCell::new(0) };
        assert!(conky_autostart(&io, &ctx, "w"));
        assert_eq!(*io.entry.borrow(), crate::ext_manager::conky_desktop_entry("w", "/h/.config/conky/w/conky.conf"));
        assert!(conky_autostart(&io, &ctx, "w"));
        assert_eq!(*io.writes.borrow(), 1, "нынешняя запись не переписывается");
    }

    /// Пути под установку в `~/.conky/<Имя>/` — строки из настоящих
    /// архивов раздела Conky магазина (victorConky, conky_orange, Conky
    /// Nord, conky_grey_redux): переписываются на папку виджета, если там
    /// есть их «хвост»; существующие и чужие пути не трогаются.
    #[test]
    fn conky_foreign_paths_point_into_widget() {
        let home = "/h";
        let w = "/h/.config/conky/victorConky1.1";
        let files = [
            "/h/.config/conky/victorConky1.1/victorConky/ArchLarge.png",
            "/h/.config/conky/victorConky1.1/conky_orange.lua",
            "/h/.config/conky/victorConky1.1/nordcore/start_conky.sh",
            "/h/.config/conky/victorConky1.1/lib/rings.lua",
            "/h/.conky/Mine/clock.lua",
        ];
        let dirs = ["/h/.config/conky/victorConky1.1/nordcore", "/h/.config/conky/victorConky1.1/lib"];
        let exists = |p: &str| files.contains(&p) || dirs.contains(&p);
        let is_dir = |p: &str| dirs.contains(&p);
        let fix = |t: &str| conky_repair_text(t, home, w, &exists, &is_dir);

        // Обёртка архива: хвост целиком есть в папке виджета.
        assert_eq!(
            fix("${image ~/.conky/victorConky/ArchLarge.png -p 10,0}").as_deref(),
            Some("${image ~/.config/conky/victorConky1.1/victorConky/ArchLarge.png -p 10,0}")
        );
        // Файл прямо в `~/.conky` (архив с папкой, а путь — без неё).
        assert_eq!(
            fix("lua_load ~/.conky/conky_orange.lua").as_deref(),
            Some("lua_load ~/.config/conky/victorConky1.1/conky_orange.lua")
        );
        // Lua-строка и `${HOME}`: файл, который виджет пишет сам, — его ещё
        // нет, но есть его папка.
        assert_eq!(
            fix("x = '${HOME}/.conky/nordcore/weather_raw'").as_deref(),
            Some("x = '${HOME}/.config/conky/victorConky1.1/nordcore/weather_raw'")
        );
        // Дом автора — абсолютный путь в дом этого человека.
        assert_eq!(
            fix("Exec=/home/linuxscoop/.conky/nordcore/start_conky.sh").as_deref(),
            Some("Exec=/h/.config/conky/victorConky1.1/nordcore/start_conky.sh")
        );
        // Чужое имя в `~/.config/conky/<Имя>/` — тоже мимо, тоже чинится.
        assert_eq!(
            fix("lua_load = '~/.config/conky/Regulus/lib/rings.lua'").as_deref(),
            Some("lua_load = '~/.config/conky/victorConky1.1/lib/rings.lua'")
        );
        // Существующий путь, уже наш путь, просто слово и чужой файл — без правки.
        assert_eq!(fix("lua_load ~/.conky/Mine/clock.lua"), None);
        assert_eq!(fix("${image ~/.config/conky/victorConky1.1/victorConky/ArchLarge.png}"), None);
        assert_eq!(fix("Copy the files to ~/.conky/ and run"), None);
        assert_eq!(fix("${image ~/.conky/Other/nothing.png}"), None);
        assert_eq!(fix("x=~/.conky.bak/a"), None);
        // Повторный ремонт ничего не меняет.
        let once = fix("${image ~/.conky/victorConky/ArchLarge.png}").unwrap();
        assert_eq!(fix(&once), None);
    }

    /// Папка виджета с пробелом: подстановка разбила бы аргумент на два —
    /// путь остаётся, как был; в кавычках автора — чинится.
    #[test]
    fn conky_path_repair_keeps_arguments_whole() {
        let w = "/h/.config/conky/Rock and Roll";
        let exists = |p: &str| p == "/h/.config/conky/Rock and Roll/bg.png";
        let no = |_: &str| false;
        assert_eq!(conky_repair_text("${image ~/.conky/bg.png}", "/h", w, &exists, &no), None);
        assert_eq!(
            conky_repair_text("${image \"~/.conky/Rock and Roll/bg.png\"}", "/h", w, &exists, &no).as_deref(),
            Some("${image \"~/.config/conky/Rock and Roll/bg.png\"}")
        );
    }

    /// Ремонт на диске: меняются только текстовые файлы с такими путями,
    /// права файла (сценарий остаётся исполняемым) сохраняются, `~/.conky`
    /// не появляется.
    #[test]
    fn conky_repair_paths_on_disk() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!("rexon-conky-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let home = tmp.to_string_lossy().into_owned();
        let w = tmp.join(".config/conky/nord");
        std::fs::create_dir_all(w.join("nordcore")).unwrap();
        std::fs::write(w.join("nordcore/clock01_rings.lua"), "-- lua\n").unwrap();
        std::fs::write(w.join("nordcore/conkyrc2core"), "lua_load = '~/.conky/nordcore/clock01_rings.lua',\n").unwrap();
        let sh = w.join("nordcore/start_conky.sh");
        std::fs::write(&sh, "#!/bin/sh\nconky -c ~/.conky/nordcore/conkyrc2core &\n").unwrap();
        std::fs::set_permissions(&sh, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(w.join("bg.png"), b"\x89PNG\0~/.conky/nordcore/x").unwrap();
        std::fs::write(w.join("install.sh"), "cp -r nordcore ~/.conky/nordcore\n").unwrap();
        let wd = w.to_string_lossy().into_owned();
        let mut changed = conky_repair_paths(&home, &wd);
        changed.sort();
        let rc = std::fs::read_to_string(w.join("nordcore/conkyrc2core")).unwrap();
        let start = std::fs::read_to_string(&sh).unwrap();
        let mode = std::fs::metadata(&sh).unwrap().permissions().mode() & 0o777;
        let conky_dir = tmp.join(".conky").exists();
        let again = conky_repair_paths(&home, &wd);
        let png = std::fs::read(w.join("bg.png")).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(changed.len(), 2, "{changed:?}");
        assert_eq!(rc, format!("lua_load = '~/.config/conky/nord/nordcore/clock01_rings.lua',\n"));
        assert_eq!(start, "#!/bin/sh\nconky -c ~/.config/conky/nord/nordcore/conkyrc2core &\n");
        assert_eq!(mode, 0o755);
        assert!(!conky_dir);
        assert!(again.is_empty());
        assert_eq!(png, b"\x89PNG\0~/.conky/nordcore/x");
        assert!(!changed.iter().any(|p| p.ends_with("install.sh")), "установщик автора не трогается");
    }

    /// «Удалить» виджета: гаснут только его процессы (по номерам), запись
    /// автозапуска снимается; путь не из папки виджетов — ничего не делается.
    #[test]
    fn forgetting_a_widget_stops_it_and_drops_autostart() {
        use std::cell::RefCell;
        struct Fake {
            kills: RefCell<Vec<Vec<String>>>,
        }
        impl LookIo for Fake {
            fn run(&self, a: &[String]) -> (bool, String) {
                self.kills.borrow_mut().push(a.to_vec());
                (true, String::new())
            }
            fn have(&self, _t: &str) -> bool { true }
            fn read(&self, p: &str) -> Option<String> {
                match p {
                    "/proc/42/cmdline" => Some("conky\0-c\0/h/.config/conky/w/conky.conf\0".into()),
                    "/proc/43/cmdline" => Some("conky\0-c\0/h/.config/conky/other/conky.conf\0".into()),
                    _ => None,
                }
            }
            fn write(&self, _p: &str, _t: &str) -> bool { true }
            fn list_dir(&self, p: &str) -> Vec<String> {
                if p == "/proc" { vec!["42".into(), "43".into(), "self".into()] } else { Vec::new() }
            }
        }
        let tmp = std::env::temp_dir().join(format!("rexon-conky-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let cfg = tmp.to_string_lossy().into_owned();
        std::fs::create_dir_all(tmp.join("autostart")).unwrap();
        std::fs::write(tmp.join("autostart/rexon-conky-w.desktop"), "x").unwrap();
        std::fs::write(tmp.join("autostart/rexon-conky-other.desktop"), "x").unwrap();
        let ctx = Ctx { home: "/h".into(), xdg_config_home: Some(cfg), wayland: false, de: String::new() };
        let io = Fake { kills: RefCell::new(Vec::new()) };
        assert_eq!(conky_forget_widget(&io, &ctx, "/h/.config/conky/w/").as_deref(), Some("w"));
        assert_eq!(*io.kills.borrow(), vec![vec!["kill".to_string(), "42".to_string()]]);
        let gone = !tmp.join("autostart/rexon-conky-w.desktop").exists();
        let kept = tmp.join("autostart/rexon-conky-other.desktop").exists();
        assert_eq!(conky_forget_widget(&io, &ctx, "/h/.local/share/fonts/rexon-conky-w"), None);
        assert_eq!(conky_forget_widget(&io, &ctx, "/h/.config/conky/w/sub"), None);
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(gone && kept);
        assert_eq!(io.kills.borrow().len(), 1);
    }

    #[test]
    fn kwin_default_state() {
        assert!(kwin_plugin_enabled(Some("[Plugins]\nfooEnabled=true\n"), "foo", false));
        assert!(!kwin_plugin_enabled(Some("[Plugins]\nfooEnabled=false\n"), "foo", true));
        assert!(kwin_plugin_enabled(None, "foo", true));
        assert!(!kwin_plugin_enabled(Some("[Other]\nfooEnabled=true\n"), "foo", false));
    }

    #[test]
    fn helper_apply_line() {
        assert_eq!(
            system_apply_argv("/usr/lib/rexon-system-helper", &["--lang=ru".into()], "sddm", "Sugar", "arch"),
            vec!["pkexec", "/usr/lib/rexon-system-helper", "--lang=ru", "apply-sddm", "Sugar", "-", "arch"]
        );
    }

    #[test]
    fn helper_remove_line() {
        assert_eq!(
            system_remove_argv("/usr/lib/rexon-system-helper", &[], "plymouth", "neon", "debian"),
            vec!["pkexec", "/usr/lib/rexon-system-helper", "remove-plymouth", "neon", "-", "debian"]
        );
    }

    /// Тема рамок, которую Marco не загрузит: применение отказывает с
    /// причиной и ключ не трогает; уже записанная в ключ — не «Активна».
    #[test]
    fn marco_theme_marco_cannot_load_is_refused() {
        use std::cell::RefCell;
        struct Fake {
            key: RefCell<String>,
            sets: RefCell<usize>,
        }
        impl LookIo for Fake {
            fn run(&self, argv: &[String]) -> (bool, String) {
                if argv.get(1).map(String::as_str) == Some("set") {
                    *self.sets.borrow_mut() += 1;
                    *self.key.borrow_mut() = argv[4].clone();
                    return (true, String::new());
                }
                (true, format!("'{}'\n", self.key.borrow()))
            }
            fn have(&self, _t: &str) -> bool { true }
            fn read(&self, p: &str) -> Option<String> {
                // Тема, которую Marco принимает, — с порчей по имени темы.
                let (extra, skip) = match p {
                    "/h/.themes/Shine/metacity-1/metacity-theme-3.xml" => (r#"<button function="unmaximize" state="normal"/>"#, ""),
                    "/h/.themes/Win8/metacity-1/metacity-theme-3.xml" => ("", "shade"),
                    "/h/.themes/Plain/metacity-1/metacity-theme-3.xml" => ("", ""),
                    _ => return None,
                };
                let buttons: String = ["close", "maximize", "minimize", "menu", "shade", "above", "stick", "unshade", "unabove", "unstick"]
                    .iter()
                    .filter(|f| **f != skip)
                    .flat_map(|f| ["normal", "pressed"].map(|st| format!(r#"<button function="{f}" state="{st}"/>"#)))
                    .collect();
                let windows: String = ["normal", "dialog", "modal_dialog", "utility", "menu", "border"]
                    .iter()
                    .map(|t| format!(r#"<window type="{t}" style_set="s"/>"#))
                    .collect();
                Some(format!(
                    "<metacity_theme><info><name>n</name><author>a</author><copyright>c</copyright>\
                     <date>d</date><description>t</description></info>\
                     <frame_style name=\"s\">{buttons}{extra}</frame_style>{windows}</metacity_theme>"
                ))
            }
            fn write(&self, _p: &str, _t: &str) -> bool { false }
            fn list_dir(&self, _p: &str) -> Vec<String> { Vec::new() }
        }
        let ctx = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        let io = Fake { key: RefCell::new("Plain".into()), sets: RefCell::new(0) };

        let (ok, msg) = press(&io, &ctx, MARCO_KIND, "Shine");
        assert!(!ok);
        assert!(msg.contains("Shine") && msg.contains("unmaximize"), "{msg}");
        assert_eq!(*io.sets.borrow(), 0, "ключ не записан");
        // Темы Win8 из магазина без кнопки shade: отказ называет кнопку.
        let (ok, msg) = press(&io, &ctx, MARCO_KIND, "Win8");
        assert!(!ok);
        assert!(msg.contains("Win8") && msg.contains("(shade)"), "{msg}");
        assert_eq!(*io.sets.borrow(), 0, "ключ не записан");
        assert_eq!(active_names(&io, &ctx, MARCO_KIND), vec!["Plain".to_string()]);

        // Записанная в обход Rexon — Marco её не показывает, и мы не говорим «Активна».
        *io.key.borrow_mut() = "Shine".into();
        assert!(active_names(&io, &ctx, MARCO_KIND).is_empty());

        let (ok, _) = press(&io, &ctx, MARCO_KIND, "Plain");
        assert!(ok);
        assert_eq!(*io.sets.borrow(), 1);
    }

    /// Тема оболочки GNOME без расширения User Themes: отказ называет
    /// расширение, ключ не пишется; со схемой — применяется как прежде.
    #[test]
    fn gnome_shell_theme_without_user_themes_says_so() {
        use std::cell::RefCell;
        struct Fake {
            schema: bool,
            key: RefCell<String>,
            sets: RefCell<usize>,
        }
        impl LookIo for Fake {
            fn run(&self, argv: &[String]) -> (bool, String) {
                let has = self.schema;
                match argv.get(1).map(String::as_str) {
                    Some("list-keys") => (has, if has { "name\n".into() } else { "No such schema\n".into() }),
                    Some("set") if has => {
                        *self.sets.borrow_mut() += 1;
                        *self.key.borrow_mut() = argv[4].clone();
                        (true, String::new())
                    }
                    Some("get") if has => (true, format!("'{}'\n", self.key.borrow())),
                    _ => (false, "No such schema\n".into()),
                }
            }
            fn have(&self, _t: &str) -> bool { true }
            fn read(&self, _p: &str) -> Option<String> { None }
            fn write(&self, _p: &str, _t: &str) -> bool { false }
            fn list_dir(&self, _p: &str) -> Vec<String> { Vec::new() }
        }
        let ctx = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        let io = Fake { schema: false, key: RefCell::new(String::new()), sets: RefCell::new(0) };
        let (ok, msg) = press(&io, &ctx, "theme-gnome-shell", "Dracula");
        assert!(!ok);
        assert!(msg.contains("Dracula") && msg.contains("User Themes"), "{msg}");
        assert_eq!(*io.sets.borrow(), 0);

        let io = Fake { schema: true, key: RefCell::new(String::new()), sets: RefCell::new(0) };
        let (ok, msg) = press(&io, &ctx, "theme-gnome-shell", "Dracula");
        assert!(ok, "{msg}");
        assert_eq!(*io.sets.borrow(), 1);
    }

    /// Включение расширения, которое оболочка уже знает, видно сразу —
    /// оговорки про перезаход нет; не знает или не ответила — оговорка
    /// остаётся (живая проверка на gnome-shell 51.1, 16.10.2026: Rexon
    /// обещал перезаход, а выключение срабатывало тут же).
    #[test]
    fn relogin_note_only_for_extension_the_shell_hasnt_seen() {
        struct Fake {
            answer: Option<&'static str>,
        }
        impl LookIo for Fake {
            fn run(&self, argv: &[String]) -> (bool, String) {
                assert_eq!(argv[0], "gdbus");
                assert_eq!(argv.last().map(String::as_str), Some("x@y"));
                match self.answer {
                    Some(out) => (true, out.to_string()),
                    None => (false, String::new()),
                }
            }
            fn have(&self, _t: &str) -> bool { true }
            fn read(&self, _p: &str) -> Option<String> { None }
            fn write(&self, _p: &str, _t: &str) -> bool { false }
            fn list_dir(&self, _p: &str) -> Vec<String> { Vec::new() }
        }
        let row = dec::row("gnome-shell-extension").expect("строка GNOME");
        let wl = Ctx { home: "/h".into(), xdg_config_home: None, wayland: true, de: String::new() };
        let known = Fake { answer: Some("({'uuid': <'x@y'>, 'state': <2.0>},)\n") };
        assert_eq!(refresh_note(&known, row, &wl, "x@y"), None);
        let unseen = Fake { answer: Some("(@a{sv} {},)\n") };
        assert!(refresh_note(&unseen, row, &wl, "x@y").is_some());
        let silent = Fake { answer: None };
        assert!(refresh_note(&silent, row, &wl, "x@y").is_some());
        let x11 = Ctx { wayland: false, ..wl };
        assert_eq!(refresh_note(&unseen, row, &x11, "x@y"), None);
    }
}
