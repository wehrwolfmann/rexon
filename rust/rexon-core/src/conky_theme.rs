//! Тема Conky «папкой с описанием»: `theme.json` и `preview.png`.
//!
//! ЗАЧЕМ. Темы, собранные визуальным редактором Conky Studio (и
//! написанные под его менеджер руками), лежат папкой, где кроме файла
//! настроек есть описание `theme.json`: имя для показа, автор, версия,
//! описание, нужные программы (`requires`), экран (`monitor`). Формат —
//! по документации редактора (страница вики «Theme layout»). Rexon читает
//! его ТОЛЬКО на чтение и показывает человеку как свою карточку: имя,
//! автора и описание — в карточке, `preview.png` — на плитке, `requires` —
//! в общей проверке недостающих программ (`conky_safety::health`: что есть
//! в репозиториях, ставится само, защита — окно пароля), `monitor` —
//! начальный экран виджета в его настройках.
//!
//! Разбор прощающий: неизвестные поля и поля не того типа молча
//! пропускаются, битый файл — как будто его нет. Своего `theme.json` Rexon
//! не пишет.

use crate::animations::LookIo;
use crate::de_apply::Ctx;

/// Имя файла описания в корне папки темы.
pub const META_FILE: &str = "theme.json";

/// Картинка темы для плитки.
pub const PREVIEW_FILE: &str = "preview.png";

/// Больше — не описание, а что-то чужое: не читаем.
const META_MAX_BYTES: usize = 64 * 1024;

/// Длиннее — обрезается (описание в карточке, не книга).
const TEXT_MAX_CHARS: usize = 2000;

/// Описание темы. Все поля необязательны.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeMeta {
    pub name: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    /// Разрешение, под которое тема рисовалась (`1920x1080`).
    pub resolution: Option<String>,
    /// Что нужно теме: программы и возможности conky (`lua-cairo`).
    pub requires: Vec<String>,
    pub tags: Vec<String>,
    /// Чем сделана (`conky-studio`).
    pub created_with: Option<String>,
    pub store_id: Option<String>,
    pub sha256: Option<String>,
    /// Экран: `auto`, `primary` или имя выхода (`DP-1`).
    pub monitor: Option<String>,
}

/// Строковое поле: строка (или число — `"version": 1.2`), без пробелов по
/// краям, не пустая. Иной тип — `None`.
fn text_field(v: &serde_json::Value) -> Option<String> {
    let s = match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => return None,
    };
    if s.is_empty() {
        return None;
    }
    Some(s.chars().take(TEXT_MAX_CHARS).collect())
}

/// Список строк: массив (не-строки пропускаются) или одна строка.
fn list_field(v: &serde_json::Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let s = s.trim();
        if !s.is_empty() && s.len() <= 200 && !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    };
    match v {
        serde_json::Value::Array(items) => {
            for i in items.iter().take(100) {
                if let serde_json::Value::String(s) = i {
                    push(s);
                }
            }
        }
        serde_json::Value::String(s) => push(s),
        _ => {}
    }
    out
}

/// Разобрать текст `theme.json`. `None` — это не объект JSON.
pub fn parse_meta(text: &str) -> Option<ThemeMeta> {
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let map = v.as_object()?;
    let text = |k: &str| map.get(k).and_then(text_field);
    let list = |k: &str| map.get(k).map(list_field).unwrap_or_default();
    Some(ThemeMeta {
        name: text("name"),
        author: text("author"),
        version: text("version"),
        description: text("description"),
        resolution: text("resolution"),
        requires: list("requires"),
        tags: list("tags"),
        created_with: text("created_with"),
        store_id: text("store_id"),
        sha256: text("sha256"),
        monitor: text("monitor"),
    })
}

/// Описание темы в папке `dir`; нет файла, слишком большой или битый —
/// `None`.
pub fn meta(io: &dyn LookIo, dir: &str) -> Option<ThemeMeta> {
    let text = io.read(&format!("{}/{META_FILE}", dir.trim_end_matches('/')))?;
    if text.len() > META_MAX_BYTES {
        return None;
    }
    parse_meta(&text)
}

/// Папка виджета `name` (имя плитки), если виджет — папка, а не один файл.
pub fn widget_dir(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<String> {
    let path = crate::conky_scan::widget_path(ctx, name);
    (!io.list_dir(&path).is_empty()).then_some(path)
}

/// Описание темы виджета `name`.
pub fn widget_meta(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<ThemeMeta> {
    meta(io, &widget_dir(io, ctx, name)?)
}

/// `preview.png` в папке темы с описанием: картинка автора, она главнее
/// прочих снимков рядом.
pub fn preview(io: &dyn LookIo, dir: &str) -> Option<String> {
    let dir = dir.trim_end_matches('/');
    let names = io.list_dir(dir);
    if !names.iter().any(|n| n == META_FILE) {
        return None;
    }
    names.iter().any(|n| n == PREVIEW_FILE).then(|| format!("{dir}/{PREVIEW_FILE}"))
}

/// Виджет, которому принадлежит файл настроек `conf`: свой (под каталогом
/// Rexon) или найденный на диске. `None` — файл ничей.
pub fn widget_of_conf(io: &dyn LookIo, ctx: &Ctx, conf: &str) -> Option<String> {
    crate::de_apply::conky_widget_of_conf(io, ctx, conf)
        .or_else(|| crate::conky_scan::owner_of_conf(ctx, &crate::conky_scan::extra_dirs_for(ctx), conf))
}

// ════════════════════════════════════════════════════════════════════════════
//  requires[] — в проверку недостающих программ
// ════════════════════════════════════════════════════════════════════════════

/// Возможность самого conky, а не отдельная программа: `lua-cairo` (так
/// пишет редактор по умолчанию), `cairo`, `lua`, `imlib2`, `conky`, сборки
/// `conky-*`. Её ставит пакет conky, проверять в PATH нечего.
fn conky_feature(token: &str) -> bool {
    let t = token.to_ascii_lowercase();
    t == "conky"
        || t.starts_with("conky-")
        || t.starts_with("conky_")
        || t.starts_with("lua")
        || t.contains("cairo")
        || t.contains("imlib")
        || t.contains("font")
        || matches!(t.as_str(), "x11" | "xorg" | "wayland" | "xwayland")
}

/// Похоже ли на имя программы или пакета: латиница, цифры, `._+-`.
fn plain_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 64
        && token.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
        && !token.starts_with(['-', '.'])
}

/// Программы из `requires`, которые надо искать в PATH. Возможности conky
/// и записи не в виде имени («Nerd Font», «internet») пропускаются. Имя
/// пакета (`lm_sensors`) переводится в имя программы по таблице
/// `utils::TOOL_PACKAGES` (`sensors`), чтобы проверка искала то, что
/// реально лежит в PATH.
pub fn required_programs(meta: &ThemeMeta) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in &meta.requires {
        let token = raw.trim();
        if !plain_token(token) || conky_feature(token) {
            continue;
        }
        let table = crate::utils::TOOL_PACKAGES;
        let program = if table.iter().any(|(bin, _)| *bin == token) {
            token.to_string()
        } else if let Some((bin, _)) = table.iter().find(|(_, pkgs)| {
            pkgs.iter().any(|(_, p)| p.strip_suffix(crate::utils::AUR_ONLY_MARK).unwrap_or(p) == token)
        }) {
            bin.to_string()
        } else {
            token.to_string()
        };
        if !out.contains(&program) {
            out.push(program);
        }
    }
    out
}

// ════════════════════════════════════════════════════════════════════════════
//  monitor — начальный экран
// ════════════════════════════════════════════════════════════════════════════

/// Номер экрана (`xinerama_head`) для поля `monitor` при списке экранов
/// `monitors`. `None` — выбирать нечего: экран один, `auto`, или выхода с
/// таким именем нет (монитор отключили).
pub fn monitor_head(meta: &ThemeMeta, monitors: &[crate::conky_overlay::Monitor]) -> Option<u32> {
    let want = meta.monitor.as_deref()?.trim();
    if monitors.len() < 2 || want.is_empty() || want.eq_ignore_ascii_case("auto") {
        return None;
    }
    if want.eq_ignore_ascii_case("primary") {
        return monitors.iter().find(|m| m.primary).map(|m| m.index);
    }
    monitors.iter().find(|m| m.name == want).map(|m| m.index)
}

/// Перед запуском файла `conf`: если человек экран виджета ещё не
/// выбирал (у файла нет своих настроек Rexon), а тема называет экран, —
/// записать его начальным выбором. Дальше это обычная настройка «Экран»:
/// человек меняет её в карточке, а «Как у автора» снимает её вместе с
/// прочими (и при следующем запуске экран темы ставится снова — это и есть
/// выбор автора). На Wayland conky экран не выбирает — ничего не делается.
pub fn seed_monitor(io: &dyn LookIo, ctx: &Ctx, conf: &str) {
    if ctx.wayland {
        return;
    }
    let p = crate::conky_overlay::live_paths(ctx, conf);
    if io.read(&p.settings).is_some() {
        return;
    }
    let Some(name) = widget_of_conf(io, ctx, conf) else { return };
    let Some(meta) = widget_meta(io, ctx, &name) else { return };
    if meta.monitor.is_none() {
        return;
    }
    let Some(head) = monitor_head(&meta, &crate::conky_overlay::monitors(io, ctx.wayland)) else { return };
    let ov = crate::conky_overlay::Overrides { xinerama_head: Some(head), ..Default::default() };
    let env = crate::conky_overlay::Env { version: crate::conky_overlay::conky_version(io), de: ctx.de.clone() };
    if let Err(why) = crate::conky_overlay::save(io, &p, conf, &ov, &env) {
        crate::log_warn!("conky", "экран темы для «{conf}» не записан: {why}");
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Карточка
// ════════════════════════════════════════════════════════════════════════════

/// Дополнить плитку виджета описанием темы: имя для показа (если плитка
/// зовётся просто именем папки), автор, версия, описание — где своих нет.
/// Имя папки остаётся для поиска (`source_name`).
pub fn decorate(meta: &ThemeMeta, folder_title: &str, item: &mut crate::models::ThemeItem) {
    if let Some(name) = &meta.name {
        if (item.name.is_empty() || item.name == folder_title) && name != folder_title {
            if item.source_name.is_empty() {
                item.source_name = folder_title.to_string();
            }
            item.name = name.clone();
        }
    }
    if item.author.is_empty() {
        if let Some(a) = &meta.author {
            item.author = a.clone();
        }
    }
    if item.version.is_empty() {
        if let Some(v) = &meta.version {
            item.version = v.clone();
        }
    }
    if item.description.is_empty() {
        if let Some(d) = &meta.description {
            item.description = crate::translator::html_escape(d).replace('\n', "<br>");
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  start.sh — запуск темы её сценарием
// ════════════════════════════════════════════════════════════════════════════
//
// У тем такого вида данные (датчики, погода, обложка плеера) собирают
// фоновые циклы из `start.sh`, которые пишут файлы кэша, а сценарий Lua
// темы их читает. Запущенная как `conky -c conky.conf`, такая тема
// рисуется с пустыми полями. Поэтому тема со `start.sh` запускается им:
// рабочий каталог — папка темы, своя группа процессов, метка
// [`MARKER_ENV`] в окружении (её наследуют все процессы темы, и после
// `setsid` внутри сценария тоже). Остановка гасит по учёту — группу из
// файла запуска и процессы с меткой этой темы, — а не по имени программы.
// Сценарий — код: до запуска его и `scripts/*.sh` показывает проверка
// команд (`conky_safety`), и без «Да» он не запускается.

/// Сценарий запуска в корне папки темы.
pub const START_SCRIPT: &str = "start.sh";

/// Папка вспомогательных сценариев темы.
pub const SCRIPTS_DIR: &str = "scripts";

/// Метка в окружении процессов темы, запущенной сценарием: путь папки
/// виджета. По ней «Остановить» находит циклы сценария, а «Запущенные» —
/// все окна conky темы.
pub const MARKER_ENV: &str = "REXON_CONKY_WIDGET";

/// Больше — не сценарий запуска.
const SCRIPT_MAX_BYTES: usize = 256 * 1024;

/// Сценарий запуска виджета `name`: `<папка>/start.sh`, не пустой. У
/// виджета одним файлом сценария нет.
pub fn start_script(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<String> {
    let dir = widget_dir(io, ctx, name)?;
    let path = format!("{dir}/{START_SCRIPT}");
    io.read(&path).filter(|t| !t.trim().is_empty() && t.len() <= SCRIPT_MAX_BYTES)?;
    Some(path)
}

/// Сценарии оболочки, которые исполняет запуск темы из папки `dir`:
/// `start.sh` первым, затем `scripts/*.sh` по имени. Пусто — у темы нет
/// `start.sh`.
pub fn shell_scripts(io: &dyn LookIo, dir: &str) -> Vec<String> {
    let dir = dir.trim_end_matches('/');
    let start = format!("{dir}/{START_SCRIPT}");
    if io.read(&start).is_none_or(|t| t.trim().is_empty()) {
        return Vec::new();
    }
    let mut out = vec![start];
    let sub = format!("{dir}/{SCRIPTS_DIR}");
    let mut names = io.list_dir(&sub);
    names.sort();
    for n in names {
        let lower = n.to_ascii_lowercase();
        if lower.ends_with(".sh") || lower.ends_with(".bash") {
            out.push(format!("{sub}/{n}"));
        }
    }
    out
}

/// Оболочки, которыми можно исполнить сценарий.
const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh", "ksh", "mksh"];

/// Чем исполнять сценарий: оболочка из строки `#!` (`#!/bin/bash`,
/// `#!/usr/bin/env bash`), иначе `sh`. Сценарий запускается через неё, а
/// не сам, — распаковщики архивов теряют бит исполнения.
pub fn interpreter(text: &str) -> String {
    let Some(line) = text.lines().next().and_then(|l| l.strip_prefix("#!")) else {
        return "sh".to_string();
    };
    let mut words = line.split_whitespace();
    let mut prog = words.next().unwrap_or("").rsplit('/').next().unwrap_or("");
    if prog == "env" {
        prog = words.find(|w| !w.starts_with('-')).unwrap_or("");
    }
    if SHELLS.contains(&prog) { prog.to_string() } else { "sh".to_string() }
}

/// Как запустить тему сценарием.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptLaunch {
    /// Папка виджета — рабочий каталог и значение метки [`MARKER_ENV`].
    pub dir: String,
    pub script: String,
    /// `[оболочка, сценарий]`.
    pub argv: Vec<String>,
}

/// План запуска виджета `name` сценарием; `None` — у виджета нет
/// `start.sh`, он идёт прежним путём `conky -c`.
pub fn script_launch(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<ScriptLaunch> {
    let script = start_script(io, ctx, name)?;
    let text = io.read(&script)?;
    let dir = widget_dir(io, ctx, name)?;
    Some(ScriptLaunch { argv: vec![interpreter(&text), script.clone()], dir, script })
}

/// Виджет со сценарием, которому принадлежит файл настроек `conf`.
pub fn script_widget_of_conf(io: &dyn LookIo, ctx: &Ctx, conf: &str) -> Option<String> {
    widget_of_conf(io, ctx, conf).filter(|n| start_script(io, ctx, n).is_some())
}

/// Метка темы в окружении процесса (`/proc/<pid>/environ`: строки
/// `КЛЮЧ=значение` через NUL).
pub fn marker_of_environ(environ: &[u8]) -> Option<String> {
    let key = format!("{MARKER_ENV}=");
    environ
        .split(|b| *b == 0)
        .find_map(|e| e.strip_prefix(key.as_bytes()))
        .map(|v| String::from_utf8_lossy(v).into_owned())
        .filter(|v| !v.is_empty())
}

/// Метка процесса `pid`; `None` — процесса нет или он без метки.
pub fn marker_of_pid(io: &dyn LookIo, pid: &str) -> Option<String> {
    marker_of_environ(&io.read_bytes(&format!("/proc/{pid}/environ"))?)
}

/// Все процессы с меткой темы `widget` (путь папки), по возрастанию номера.
pub fn marked_pids(io: &dyn LookIo, widget: &str) -> Vec<String> {
    let widget = widget.trim_end_matches('/');
    let mut pids: Vec<u64> = io.list_dir("/proc").iter().filter_map(|p| p.parse().ok()).collect();
    pids.sort_unstable();
    pids.into_iter()
        .map(|p| p.to_string())
        .filter(|p| marker_of_pid(io, p).is_some_and(|m| m.trim_end_matches('/') == widget))
        .collect()
}

/// Каталог файлов запуска тем: `$XDG_RUNTIME_DIR/rexon/conky-start`.
fn run_dir() -> Option<String> {
    let rt = std::env::var("XDG_RUNTIME_DIR").ok().filter(|d| !d.is_empty())?;
    Some(format!("{}/rexon/conky-start", rt.trim_end_matches('/')))
}

/// Файл запуска темы (номер группы процессов): по ключу автозапуска
/// виджета — у него нет косых черт.
fn group_file(widget: &str) -> Option<String> {
    let key = crate::conky_scan::autostart_key(widget);
    Some(format!("{}/{key}.pgid", run_dir()?))
}

/// Номер группы темы из файла запуска — только если её глава жив и
/// несёт метку этой темы (номер мог достаться другой программе).
pub fn recorded_group(io: &dyn LookIo, widget: &str) -> Option<String> {
    let text = io.read(&group_file(widget)?)?;
    let pgid = text.trim().to_string();
    if pgid.is_empty() || !pgid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    marker_of_pid(io, &pgid).is_some_and(|m| m.trim_end_matches('/') == widget.trim_end_matches('/')).then_some(pgid)
}

/// Что погасить вместе с процессами `pids`: у процесса с меткой темы — все
/// процессы этой темы (циклы сценария, остальные окна conky) и её группа
/// (`-<номер>` для `kill`). Без меток — `pids` как есть.
pub fn with_companions(io: &dyn LookIo, pids: Vec<String>) -> Vec<String> {
    let mut widgets: Vec<String> = Vec::new();
    for p in &pids {
        if let Some(m) = marker_of_pid(io, p) {
            if !widgets.contains(&m) {
                widgets.push(m);
            }
        }
    }
    let mut out = pids;
    for w in &widgets {
        for p in marked_pids(io, w) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
        if let Some(g) = recorded_group(io, w) {
            let g = format!("-{g}");
            if !out.contains(&g) {
                out.push(g);
            }
        }
    }
    out
}

/// Всё, что гасить у темы `widget` (путь папки): процессы с её меткой и
/// её группа. Пусто — тема сценарием не запущена.
pub fn widget_companions(io: &dyn LookIo, widget: &str) -> Vec<String> {
    let mut out = marked_pids(io, widget);
    if let Some(g) = recorded_group(io, widget) {
        out.push(format!("-{g}"));
    }
    out
}

/// Номера процессов conky виджета `name` сейчас: по метке темы или по
/// файлу настроек в его папке.
pub fn widget_conky_pids(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Vec<String> {
    let path = crate::conky_scan::widget_path(ctx, name);
    crate::conky_procs::all(io)
        .into_iter()
        .filter(|p| {
            p.widget.as_deref().is_some_and(|w| w.trim_end_matches('/') == path)
                || p.conf.as_deref().is_some_and(|c| crate::conky_scan::widget_owns(ctx, name, c))
        })
        .map(|p| p.pid)
        .collect()
}

/// Сколько тема должна продержаться с живым conky, чтобы считаться
/// запущенной (как у `conky_procs`: вывод Wayland падает не сразу).
const SURVIVE_MS: u64 = 3000;

/// Сколько ждать первого conky от сценария: перед ним сценарий запускает
/// свои опросчики.
const APPEAR_MS: u64 = 6000;

/// Запустить виджет `name` его `start.sh`. Прежний запуск темы сначала
/// гасится (тема одна на столе). `(получилось, сообщение)`.
pub fn start_widget_script(io: &dyn LookIo, ctx: &Ctx, name: &str) -> (bool, String) {
    let title = crate::conky_scan::widget_title(name);
    if !io.have("conky") {
        return (false, crate::de_apply::conky_missing_refusal());
    }
    let Some(plan) = script_launch(io, ctx, name) else {
        return (false, crate::i18n::trf("Couldn't start “%1”: its start script is gone.", &[&title]));
    };
    // Тема уже стоит (кнопкой, при входе, руками) — погасить её целиком.
    let mut old = widget_conky_pids(io, ctx, name);
    old.extend(widget_companions(io, &plan.dir));
    if !old.is_empty() {
        stop_and_wait(io, with_companions(io, old));
    }
    let main_conf = main_conf(io, ctx, name);
    let log = crate::conky_log::live_log_path(ctx, main_conf.as_deref().unwrap_or(&plan.script));
    let needs_x11 = ctx.wayland && main_conf.as_deref().is_some_and(crate::conky_overlay::widget_needs_x11);
    let mut xwayland = crate::conky_overlay::live_via_xwayland(io, ctx, needs_x11);
    let mut result = run_script(io, ctx, name, &plan, &log, xwayland);
    if result.is_err() && !xwayland && ctx.wayland && crate::conky_overlay::live_display() {
        crate::log_warn!("conky", "«{title}»: вывод Wayland не поднялся, второй запуск сценария через XWayland");
        xwayland = true;
        result = run_script(io, ctx, name, &plan, &log, true);
    }
    if let Err(why) = result {
        return (false, why);
    }
    let mut msg = crate::i18n::trf("▶️ Started: %1", &[&title]);
    if !xwayland {
        let version = if ctx.wayland { crate::conky_overlay::conky_version(io) } else { None };
        if let Some(note) = crate::de_apply::conky_wayland_note_for(&ctx.de, ctx.wayland, version) {
            msg.push_str(" — ");
            msg.push_str(&note);
        }
    }
    (true, msg)
}

/// `kill -- <targets>` (номера и `-группы`). `kill` отвечает ошибкой, если
/// хоть один номер уже вышел сам (короткий `sleep` цикла сценария), — это
/// не неудача: `(true, _)`, если из названных номеров никого не осталось.
/// `(получилось, вывод kill)`.
pub fn kill_targets(io: &dyn LookIo, targets: &[String]) -> (bool, String) {
    let mut cmd = vec!["kill".to_string(), "--".to_string()];
    cmd.extend(targets.iter().cloned());
    let (ok, out) = io.run(&cmd);
    if ok {
        return (true, out);
    }
    let alive = targets
        .iter()
        .filter(|t| !t.starts_with('-'))
        .any(|p| io.read(&format!("/proc/{p}/cmdline")).is_some_and(|c| !c.is_empty()));
    (!alive, out)
}

/// Погасить `targets` (номера и `-группы`) и дождаться выхода — иначе
/// новый запуск принял бы старый conky за свой.
fn stop_and_wait(io: &dyn LookIo, targets: Vec<String>) {
    let _ = kill_targets(io, &targets);
    let pids: Vec<&String> = targets.iter().filter(|t| !t.starts_with('-')).collect();
    for _ in 0..30 {
        if pids.iter().all(|p| io.read(&format!("/proc/{p}/cmdline")).is_none_or(|c| c.is_empty())) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Один запуск сценария: `Ok` — conky темы появился и продержался
/// [`SURVIVE_MS`]; иначе всё, что сценарий успел поднять, гасится, а
/// причина — из журнала.
fn run_script(io: &dyn LookIo, ctx: &Ctx, name: &str, plan: &ScriptLaunch, log: &str, xwayland: bool) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let title = crate::conky_scan::widget_title(name);
    let out = crate::conky_log::open_for_start(log);
    let err = out.as_ref().and_then(|f| f.try_clone().ok());
    let stdio = |f: Option<std::fs::File>| f.map(std::process::Stdio::from).unwrap_or_else(std::process::Stdio::null);
    let mut cmd = std::process::Command::new(&plan.argv[0]);
    cmd.args(&plan.argv[1..])
        .current_dir(&plan.dir)
        .env(MARKER_ENV, &plan.dir)
        .stdin(std::process::Stdio::null())
        .stdout(stdio(out))
        .stderr(stdio(err))
        .process_group(0);
    if xwayland {
        crate::conky_overlay::apply_xwayland(&mut cmd);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| crate::i18n::trf("Couldn't start “%1”: %2", &[&title, &e.to_string()]))?;
    let leader = child.id().to_string();
    if let Some(file) = group_file(&plan.dir) {
        io.write(&file, &format!("{leader}\n"));
    }
    // Сценарий может выйти сразу (`exec setsid "$0"`) или стать conky —
    // ждать его конца в отдельном потоке, чтобы не осталось зомби.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    let step = std::time::Duration::from_millis(100);
    let mut seen_ms: u64 = 0;
    let mut waited: u64 = 0;
    let ok = loop {
        std::thread::sleep(step);
        waited += 100;
        if widget_conky_pids(io, ctx, name).is_empty() {
            // Появился и пропал — упал при старте.
            if seen_ms > 0 || waited >= APPEAR_MS {
                break false;
            }
        } else {
            seen_ms += 100;
            if seen_ms >= SURVIVE_MS {
                break true;
            }
        }
    };
    if ok {
        return Ok(());
    }
    // Не поднялся — циклы сценария на столе не оставлять.
    let mut rest = widget_companions(io, &plan.dir);
    rest.push(format!("-{leader}"));
    let _ = kill_targets(io, &rest);
    let text = std::fs::read(log).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let why = match crate::conky_log::failure_reason(&text) {
        Some(reason) => crate::i18n::trf("conky closed right after starting: %1", &[&reason]),
        None => crate::i18n::tr("the widget's start script didn't bring up Conky — see what it reported in the card"),
    };
    Err(crate::i18n::trf("Couldn't start “%1”: %2", &[&title, &why]))
}

/// Строка автозапуска темы сценарием: `sh -c` с телом и аргументами
/// (`$0` — сценарий, `$1` — папка-метка, `$2` — оболочка). Пути в тело не
/// подставляются.
pub const AUTOSTART_SCRIPT: &str = r#"cd "$(dirname "$0")" && export REXON_CONKY_WIDGET="$1" && exec "$2" "$0""#;

/// Строка запуска для конфига композитора (там `$` разворачивает сам
/// композитор — пути вписаны в кавычках; путь с `"`, `$`, `` ` ``, `\` —
/// `None`).
pub fn compositor_argv(plan: &ScriptLaunch, delay: u32) -> Option<Vec<String>> {
    let bad = |s: &str| s.contains(['"', '$', '`', '\\', '\n']);
    if bad(&plan.dir) || bad(&plan.script) {
        return None;
    }
    let sleep = if delay > 0 { format!("sleep {delay} && ") } else { String::new() };
    Some(vec![
        "sh".into(),
        "-c".into(),
        format!(
            "{sleep}cd \"{dir}\" && export {MARKER_ENV}=\"{dir}\" && exec {sh} \"{script}\"",
            dir = plan.dir,
            sh = plan.argv[0],
            script = plan.script
        ),
    ])
}

// ════════════════════════════════════════════════════════════════════════════
//  Многооконная тема — один виджет
// ════════════════════════════════════════════════════════════════════════════
//
// Сценарий запуска темы бывает поднимает несколько conky — по окну на
// экран или на часть рисунка (`conky_window_*.conf`, копии с
// `xinerama_head`). Для человека это ОДИН виджет: в «Запущенных», в ленте,
// в наборах и в вопросе «Остановить все» он один, а остановка гасит всю
// группу процессов темы ([`with_companions`]).

/// Имя виджета (как у плитки) по папке темы `dir` — значению метки
/// [`MARKER_ENV`]: под каталогом Rexon — путь от него, иначе найденный на
/// диске (путь), иначе сама папка.
pub fn widget_of_dir(ctx: &Ctx, extra: &[String], dir: &str) -> String {
    let dir = dir.trim_end_matches('/');
    let root = format!("{}/", crate::de_apply::conky_root(ctx).trim_end_matches('/'));
    if let Some(rest) = dir.strip_prefix(&root).filter(|r| !r.is_empty()) {
        return rest.to_string();
    }
    crate::conky_scan::owner_of_conf(ctx, extra, &format!("{dir}/{START_SCRIPT}"))
        .filter(|o| o.trim_end_matches('/') == dir)
        .unwrap_or_else(|| dir.to_string())
}

/// Главный файл настроек виджета `name`: им тема со сценарием значится в
/// наборах и в списке запущенных.
pub fn main_conf(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<String> {
    crate::conky_scan::widget_confs(io, ctx, name, crate::de_apply::conky_conf(io, ctx, name)).into_iter().next()
}

/// Каким файлом значится запущенный conky с файлом `conf` и меткой
/// `marker`: окно темы со сценарием — главным файлом темы (все окна темы —
/// одна строка), прочие — своим файлом.
pub fn unit_conf(io: &dyn LookIo, ctx: &Ctx, extra: &[String], conf: &str, marker: Option<&str>) -> String {
    let name = match marker {
        Some(dir) => Some(widget_of_dir(ctx, extra, dir)),
        None => script_widget_of_conf(io, ctx, conf),
    };
    name.and_then(|n| main_conf(io, ctx, &n)).unwrap_or_else(|| conf.to_string())
}

/// Файлы виджетов `confs` (из набора) без повторов одной темы: окна темы
/// со сценарием сводятся к её главному файлу — иначе набор поднимал бы
/// тему столько раз, сколько у неё окон.
pub fn unit_confs(io: &dyn LookIo, ctx: &Ctx, extra: &[String], confs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in confs {
        let u = unit_conf(io, ctx, extra, c, None);
        if !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

// ════════════════════════════════════════════════════════════════════════════
//  Conky Studio — редактор, которым тема сделана
// ════════════════════════════════════════════════════════════════════════════
//
// Rexon сам редактор не пишет: тему, сделанную в Conky Studio (отметка
// `created_with` в `theme.json` или файл проекта `.cstudio`), карточка
// предлагает открыть в нём — только если он есть на этой машине. Ставится
// Conky Studio одним файлом AppImage, поэтому ищется и в PATH, и в
// обычных местах, куда такие файлы кладут.

/// Расширение файла проекта Conky Studio.
pub const STUDIO_PROJECT_EXT: &str = "cstudio";

/// Имена программы в PATH (обёртки, ссылки на AppImage).
const STUDIO_PROGRAMS: &[&str] = &["conky-studio", "conkystudio", "ConkyStudio", "conky_studio"];

/// Отметка `created_with` — Conky Studio (`conky-studio`, `Conky Studio`,
/// `conkystudio 1.1`).
fn studio_mark(created_with: &str) -> bool {
    let s: String = created_with.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    s.starts_with("conkystudio")
}

/// Файл проекта Conky Studio темы в папке `dir`: внутри папки (первый по
/// имени) или рядом с ней (`<папка>.cstudio`).
pub fn studio_project(io: &dyn LookIo, dir: &str) -> Option<String> {
    let dir = dir.trim_end_matches('/');
    let ext = format!(".{STUDIO_PROJECT_EXT}");
    let mut names = io.list_dir(dir);
    names.sort();
    if let Some(n) = names.iter().find(|n| n.to_ascii_lowercase().ends_with(&ext) && n.len() > ext.len()) {
        return Some(format!("{dir}/{n}"));
    }
    let (parent, base) = dir.rsplit_once('/')?;
    io.list_dir(if parent.is_empty() { "/" } else { parent })
        .into_iter()
        .find(|n| n.strip_suffix(&ext).is_some_and(|stem| stem == base))
        .map(|n| format!("{parent}/{n}"))
}

/// Тема в папке `dir` сделана в Conky Studio.
pub fn is_studio_theme(io: &dyn LookIo, dir: &str) -> bool {
    meta(io, dir).and_then(|m| m.created_with).is_some_and(|c| studio_mark(&c)) || studio_project(io, dir).is_some()
}

/// Можно ли файл запустить: бит исполнения. Файла нет на настоящем диске
/// (подделка в тестах) — можно.
fn executable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(true)
}

/// Похоже ли имя файла на AppImage Conky Studio
/// (`ConkyStudio-1.1.1-x86_64.AppImage`, `conky-studio.appimage`).
fn studio_appimage_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let flat: String = lower.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    flat.starts_with("conkystudio") && lower.ends_with(".appimage")
}

/// Каталог «Загрузки» из `user-dirs.dirs` (у русского стола —
/// `~/Загрузки`); `None` — не задан.
fn download_dir(io: &dyn LookIo, ctx: &Ctx) -> Option<String> {
    let home = ctx.home.trim_end_matches('/');
    let cfg = ctx.xdg_config_home.clone().unwrap_or_else(|| format!("{home}/.config"));
    let text = io.read(&format!("{}/user-dirs.dirs", cfg.trim_end_matches('/')))?;
    text.lines().find_map(|l| {
        let v = l.trim().strip_prefix("XDG_DOWNLOAD_DIR=")?.trim().trim_matches('"').replace("$HOME", home);
        (!v.is_empty() && v.trim_end_matches('/') != home).then(|| v.trim_end_matches('/').to_string())
    })
}

/// Где ищется AppImage Conky Studio.
fn studio_dirs(io: &dyn LookIo, ctx: &Ctx) -> Vec<String> {
    let home = ctx.home.trim_end_matches('/');
    let mut dirs: Vec<String> = ["Applications", "applications", "AppImages", "Appimages", ".local/bin", "bin", "Downloads"]
        .iter()
        .map(|d| format!("{home}/{d}"))
        .collect();
    if let Some(d) = download_dir(io, ctx) {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs.push("/opt".into());
    dirs.push("/opt/conky-studio".into());
    dirs
}

/// Как запустить Conky Studio на этой машине: программа из PATH или путь
/// к AppImage (последний по имени — обычно самый новый выпуск). `None` —
/// Conky Studio не найден.
pub fn find_studio(io: &dyn LookIo, ctx: &Ctx) -> Option<String> {
    if let Some(p) = STUDIO_PROGRAMS.iter().find(|p| io.have(p)) {
        return Some(p.to_string());
    }
    for dir in studio_dirs(io, ctx) {
        let mut names: Vec<String> = io.list_dir(&dir).into_iter().filter(|n| studio_appimage_name(n)).collect();
        names.sort();
        if let Some(n) = names.into_iter().rev().find(|n| executable(&format!("{dir}/{n}"))) {
            return Some(format!("{dir}/{n}"));
        }
    }
    None
}

/// Чем открыть виджет `name` в Conky Studio: строка запуска (с файлом
/// проекта, если он есть). `None` — тема не из Conky Studio или его нет
/// на машине: кнопки в карточке нет.
pub fn studio_edit_argv(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<Vec<String>> {
    let dir = widget_dir(io, ctx, name)?;
    if !is_studio_theme(io, &dir) {
        return None;
    }
    let mut argv = vec![find_studio(io, ctx)?];
    if let Some(project) = studio_project(io, &dir) {
        argv.push(project);
    }
    Some(argv)
}

/// «Редактировать в Conky Studio»: запустить редактор. `(получилось,
/// сообщение)`.
pub fn edit_in_studio(io: &dyn LookIo, ctx: &Ctx, name: &str) -> (bool, String) {
    let title = crate::conky_scan::widget_title(name);
    let Some(argv) = studio_edit_argv(io, ctx, name) else {
        return (false, crate::i18n::tr("Conky Studio isn't found on this computer."));
    };
    if !io.spawn(&argv) {
        return (false, crate::i18n::trf("Couldn't open Conky Studio: %1", &[&argv[0]]));
    }
    crate::log_info!("conky", "«{title}»: открыт в Conky Studio ({})", argv[0]);
    (true, crate::i18n::trf("✏️ Opening “%1” in Conky Studio. Restart the widget to see your changes.", &[&title]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_reads_known_fields() {
        let m = parse_meta(
            r#"{
              "name": "Night Ops", "author": "you", "version": "1.0.0",
              "description": "Compact strip", "resolution": "1920x1080",
              "requires": ["lua-cairo", "curl", "jq"], "tags": ["dark", "minimal"],
              "created_with": "conky-studio", "store_id": "abc", "sha256": "00ff",
              "monitor": "DP-1"
            }"#,
        )
        .unwrap();
        assert_eq!(m.name.as_deref(), Some("Night Ops"));
        assert_eq!(m.author.as_deref(), Some("you"));
        assert_eq!(m.version.as_deref(), Some("1.0.0"));
        assert_eq!(m.description.as_deref(), Some("Compact strip"));
        assert_eq!(m.resolution.as_deref(), Some("1920x1080"));
        assert_eq!(m.requires, ["lua-cairo", "curl", "jq"]);
        assert_eq!(m.tags, ["dark", "minimal"]);
        assert_eq!(m.created_with.as_deref(), Some("conky-studio"));
        assert_eq!(m.store_id.as_deref(), Some("abc"));
        assert_eq!(m.sha256.as_deref(), Some("00ff"));
        assert_eq!(m.monitor.as_deref(), Some("DP-1"));
    }

    /// Неизвестные поля и поля не того типа молча пропускаются; битый файл
    /// и не-объект — как будто описания нет.
    #[test]
    fn meta_skips_unknown_and_broken() {
        let m = parse_meta(
            r#"{"name": 5, "author": {"x": 1}, "version": 1.5, "description": "  ",
                "requires": ["curl", 3, null, "", "curl"], "tags": "solo",
                "monitor": ["DP-1"], "future_field": true}"#,
        )
        .unwrap();
        assert_eq!(m.name.as_deref(), Some("5"));
        assert_eq!(m.author, None);
        assert_eq!(m.version.as_deref(), Some("1.5"));
        assert_eq!(m.description, None);
        assert_eq!(m.requires, ["curl"]);
        assert_eq!(m.tags, ["solo"]);
        assert_eq!(m.monitor, None);
        assert_eq!(parse_meta("{ not json"), None);
        assert_eq!(parse_meta("[1, 2]"), None);
        assert_eq!(parse_meta("\u{feff}{}"), Some(ThemeMeta::default()));
    }

    #[test]
    fn requires_become_programs() {
        let m = ThemeMeta {
            requires: ["lua-cairo", "conky", "curl", "lm_sensors", "Nerd Font", "jq", "cairo", "playerctl", "curl"]
                .map(String::from)
                .to_vec(),
            ..Default::default()
        };
        assert_eq!(required_programs(&m), ["curl", "sensors", "jq", "playerctl"]);
    }

    fn mon(index: u32, name: &str, primary: bool) -> crate::conky_overlay::Monitor {
        crate::conky_overlay::Monitor { index, name: name.into(), width: 1920, height: 1080, x: 0, y: 0, primary }
    }

    #[test]
    fn monitor_field_picks_head() {
        let two = [mon(0, "eDP-1", false), mon(1, "DP-1", true)];
        let with = |m: &str| ThemeMeta { monitor: Some(m.into()), ..Default::default() };
        assert_eq!(monitor_head(&with("DP-1"), &two), Some(1));
        assert_eq!(monitor_head(&with("primary"), &two), Some(1));
        assert_eq!(monitor_head(&with("auto"), &two), None);
        assert_eq!(monitor_head(&with("HDMI-9"), &two), None, "выхода нет — экран не трогаем");
        assert_eq!(monitor_head(&with("DP-1"), &two[..1]), None, "экран один — выбирать нечего");
        assert_eq!(monitor_head(&ThemeMeta::default(), &two), None);
    }

    #[test]
    fn card_takes_name_author_description() {
        let m = parse_meta(r#"{"name": "Night Ops", "author": "you", "version": "2", "description": "a <b>\nb"}"#).unwrap();
        let mut it = crate::models::ThemeItem { name: "night-ops".into(), ..Default::default() };
        decorate(&m, "night-ops", &mut it);
        assert_eq!(it.name, "Night Ops");
        assert_eq!(it.source_name, "night-ops");
        assert_eq!(it.author, "you");
        assert_eq!(it.version, "2");
        assert_eq!(it.description, "a &lt;b&gt;<br>b");
        // Своё имя (из магазина) и своё описание не перебиваются.
        let mut own = crate::models::ThemeItem { name: "Store title".into(), description: "x".into(), ..Default::default() };
        decorate(&m, "night-ops", &mut own);
        assert_eq!(own.name, "Store title");
        assert_eq!(own.description, "x");
    }

    /// `requires` темы входит в общую проверку недостающих программ.
    #[test]
    fn requires_join_health_check() {
        let d = std::env::temp_dir().join(format!("rexon-conky-theme-req-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("T")).unwrap();
        std::fs::write(d.join("T/conky.conf"), "conky.config = {}\nconky.text = [[x]]\n").unwrap();
        std::fs::write(d.join("T/theme.json"), r#"{"requires": ["lua-cairo", "rexon-no-such-program-42"]}"#).unwrap();
        let ctx = Ctx { home: d.to_string_lossy().into_owned(), xdg_config_home: None, wayland: false, de: String::new() };
        let name = d.join("T").to_string_lossy().into_owned();
        let h = crate::conky_safety::health(&crate::animations::SystemIo, &ctx, &name, "arch");
        let missing: Vec<&str> = h.missing_programs.iter().map(|m| m.program.as_str()).collect();
        assert_eq!(missing, ["rexon-no-such-program-42"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-conky-theme-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(root: &std::path::Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn ctx_at(home: &std::path::Path) -> Ctx {
        Ctx { home: home.to_string_lossy().into_owned(), xdg_config_home: None, wayland: false, de: String::new() }
    }

    #[test]
    fn interpreter_from_shebang() {
        assert_eq!(interpreter("#!/bin/bash\necho"), "bash");
        assert_eq!(interpreter("#!/usr/bin/env bash\n"), "bash");
        assert_eq!(interpreter("#!/usr/bin/env -S zsh -f\n"), "zsh");
        assert_eq!(interpreter("#!/usr/bin/python3\n"), "sh", "не оболочка — sh");
        assert_eq!(interpreter("echo hi\n"), "sh");
        // Метка в сценарии автозапуска — та же, что ищет остановка.
        assert!(AUTOSTART_SCRIPT.contains(&format!("export {MARKER_ENV}=")));
    }

    /// План запуска: тема со `start.sh` — им, из папки темы, оболочкой из
    /// `#!`; без него (или виджет одним файлом) — прежний путь.
    #[test]
    fn launch_plan_goes_through_start_script() {
        let d = tmp("plan");
        put(&d, "T/conky.conf", "conky.config = {}\nconky.text = [[x]]\n");
        put(&d, "T/start.sh", "#!/bin/bash\nexec conky -c conky.conf\n");
        put(&d, "Plain/conky.conf", "conky.config = {}\nconky.text = [[x]]\n");
        put(&d, "Empty/conky.conf", "conky.text = [[x]]\n");
        put(&d, "Empty/start.sh", "  \n");
        let io = crate::animations::SystemIo;
        let ctx = ctx_at(&d);
        let s = |p: &str| d.join(p).to_string_lossy().into_owned();
        let plan = script_launch(&io, &ctx, &s("T")).unwrap();
        assert_eq!(plan.dir, s("T"));
        assert_eq!(plan.argv, ["bash".to_string(), s("T/start.sh")]);
        assert_eq!(script_launch(&io, &ctx, &s("Plain")), None);
        assert_eq!(script_launch(&io, &ctx, &s("Empty")), None, "пустой start.sh — не сценарий");
        assert_eq!(script_launch(&io, &ctx, &s("T/conky.conf")), None, "виджет-файл — без сценария");
        let a = compositor_argv(&plan, 5).unwrap();
        assert_eq!(a[2], format!("sleep 5 && cd \"{0}\" && export {MARKER_ENV}=\"{0}\" && exec bash \"{0}/start.sh\"", s("T")));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Сканер опасных команд видит `start.sh` и `scripts/*.sh`: без
    /// разрешения тема не запускается; разрешение помнится, пока файлы не
    /// изменились. Свои функции сценария недостающими программами не
    /// считаются.
    #[test]
    fn safety_scanner_sees_start_script() {
        crate::i18n::set_language(Some("en"));
        let d = tmp("scan");
        put(&d, "T/conky.conf", "conky.config = {}\nconky.text = [[${time}]]\n");
        put(
            &d,
            "T/start.sh",
            "#!/bin/bash\n# комментарий\nDIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\ncd \"$DIR\"\n\
             poll_weather() {\n  curl -s https://wttr.in/?format=3 > .runtime-cache/weather.cache\n}\n\
             poll_weather &\nwhile true; do sleep 60; scripts/gpu.sh; done &\nexec conky -c \"$DIR/conky.conf\"\n",
        );
        put(&d, "T/scripts/gpu.sh", "#!/bin/sh\nnvidia-smi --query-gpu=temperature.gpu --format=csv > .runtime-cache/gpu.cache\n");
        let io = crate::animations::SystemIo;
        let ctx = ctx_at(&d);
        let name = d.join("T").to_string_lossy().into_owned();
        let risks = crate::conky_safety::scan_widget(&io, &ctx, &name);
        let what: Vec<&str> = risks.iter().map(|r| r.what.as_str()).collect();
        assert!(what.contains(&"start.sh"), "{what:?}");
        assert!(what.contains(&"scripts/gpu.sh"), "{what:?}");
        assert!(what.iter().any(|w| w.starts_with("curl -s https://wttr.in")), "{what:?}");
        assert!(what.iter().any(|w| w.starts_with("nvidia-smi")), "{what:?}");
        assert!(what.contains(&"while true; do sleep 60; scripts/gpu.sh; done &"), "{what:?}");
        assert!(!what.iter().any(|w| w.starts_with("cd ") || w.starts_with("exec conky")), "обвязка не показывается: {what:?}");
        assert!(risks.iter().any(|r| r.kind == crate::conky_safety::RiskKind::Network));
        let (title, text) = crate::conky_safety::start_question(&io, &ctx, &name).expect("без разрешения — вопрос");
        assert_eq!(title, "Allow the widget to run commands?");
        assert!(text.contains("• start.sh\n"), "{text}");
        assert!(crate::conky_safety::start_refusal(&io, &ctx, &name).is_some());
        // Функции сценария и программы его строк — не «нет программы».
        let h = crate::conky_safety::health(&io, &ctx, &name, "arch");
        assert!(h.missing_programs.is_empty(), "{:?}", h.missing_programs);
        assert!(crate::conky_safety::remember(&io, &ctx, &name));
        assert!(crate::conky_safety::start_question(&io, &ctx, &name).is_none());
        put(&d, "T/scripts/gpu.sh", "#!/bin/sh\nrm -rf ~/x\n");
        let (_, text) = crate::conky_safety::start_question(&io, &ctx, &name).expect("файлы изменились — вопрос снова");
        assert!(text.contains("rm -rf ~/x"), "{text}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Подделка `/proc`: окружение процессов с меткой темы.
    struct FakeProcs {
        files: std::collections::HashMap<String, String>,
        ran: std::cell::RefCell<Vec<Vec<String>>>,
    }

    impl LookIo for FakeProcs {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(argv.to_vec());
            (true, String::new())
        }
        fn have(&self, _: &str) -> bool {
            true
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.get(path).cloned()
        }
        fn write(&self, _: &str, _: &str) -> bool {
            false
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            if path != "/proc" {
                return Vec::new();
            }
            let mut v: Vec<String> =
                self.files.keys().filter_map(|k| k.strip_prefix("/proc/")?.split_once('/').map(|(p, _)| p.to_string())).collect();
            v.sort();
            v.dedup();
            v
        }
    }

    fn fake_proc(f: &mut std::collections::HashMap<String, String>, pid: u32, cmd: &[&str], comm: &str, marker: Option<&str>) {
        f.insert(format!("/proc/{pid}/cmdline"), cmd.join("\0") + "\0");
        f.insert(format!("/proc/{pid}/comm"), format!("{comm}\n"));
        let mut env = "HOME=/h\0LANG=C\0".to_string();
        if let Some(m) = marker {
            env.push_str(&format!("{MARKER_ENV}={m}\0"));
        }
        f.insert(format!("/proc/{pid}/environ"), env);
    }

    /// Остановка темы со сценарием: вместе с её conky гаснут циклы
    /// сценария (по метке), чужие процессы — нет.
    #[test]
    fn stop_takes_the_whole_theme() {
        let mut f = std::collections::HashMap::new();
        let t = "/h/.config/conky/Night";
        fake_proc(&mut f, 200, &["bash", "/h/.config/conky/Night/start.sh"], "bash", Some(t));
        fake_proc(&mut f, 201, &["sleep", "60"], "sleep", Some(t));
        fake_proc(&mut f, 202, &["conky", "-c", "/h/.config/conky/Night/conky.conf"], "conky", Some(t));
        fake_proc(&mut f, 203, &["conky", "-c", "/h/.config/conky/Night/conky_w2.conf"], "conky", Some(t));
        fake_proc(&mut f, 300, &["conky", "-c", "/h/.config/conky/Other/conky.conf"], "conky", None);
        fake_proc(&mut f, 301, &["sleep", "60"], "sleep", Some("/h/.config/conky/Night2"));
        let io = FakeProcs { files: f, ran: Default::default() };
        assert_eq!(marker_of_environ(b"A=1\0REXON_CONKY_WIDGET=/x/y\0B=2\0").as_deref(), Some("/x/y"));
        assert_eq!(marker_of_environ(b"A=1\0"), None);
        assert_eq!(marked_pids(&io, t), ["200", "201", "202", "203"]);
        assert_eq!(with_companions(&io, vec!["202".into()]), ["202", "200", "201", "203"]);
        assert_eq!(with_companions(&io, vec!["300".into()]), ["300"], "без метки — только сам процесс");
        let ctx = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        let (ok, _) = crate::conky_procs::stop_pids(&io, "Night", vec!["203".into()]);
        assert!(ok);
        assert_eq!(*io.ran.borrow(), [["kill", "--", "203", "200", "201", "202"].map(String::from).to_vec()]);
        let procs = crate::conky_procs::all(&io);
        assert_eq!(procs.iter().filter(|p| p.widget.as_deref() == Some(t)).count(), 2);
        let _ = ctx;
    }

    /// Живой запуск сценарием на подставном «conky» (копия `tail` под этим
    /// именем — настоящий conky не запускается): метка в окружении, рабочий
    /// каталог — папка темы, своя группа; остановка гасит всю группу.
    #[test]
    fn start_script_runs_and_stops_as_group() {
        struct Io;
        impl LookIo for Io {
            fn run(&self, argv: &[String]) -> (bool, String) {
                crate::animations::SystemIo.run(argv)
            }
            fn have(&self, _: &str) -> bool {
                true
            }
            fn read(&self, path: &str) -> Option<String> {
                crate::animations::SystemIo.read(path)
            }
            fn read_bytes(&self, path: &str) -> Option<Vec<u8>> {
                crate::animations::SystemIo.read_bytes(path)
            }
            fn write(&self, path: &str, text: &str) -> bool {
                crate::animations::SystemIo.write(path, text)
            }
            fn list_dir(&self, path: &str) -> Vec<String> {
                crate::animations::SystemIo.list_dir(path)
            }
        }
        let Some(tail) = crate::utils::which("tail", std::env::var("PATH").ok().as_deref()) else { return };
        let d = tmp("run");
        put(&d, "T/conky.conf", "conky.config = {}\nconky.text = [[x]]\n");
        put(&d, "T/start.sh", "#!/bin/sh\nwhile true; do sleep 1; done &\nexec ./conky -c +1 -f /dev/null\n");
        std::fs::copy(tail, d.join("T/conky")).unwrap();
        let ctx = ctx_at(&d);
        let name = d.join("T").to_string_lossy().into_owned();
        let (ok, msg) = start_widget_script(&Io, &ctx, &name);
        assert!(ok, "{msg}");
        let conky = widget_conky_pids(&Io, &ctx, &name);
        assert_eq!(conky.len(), 1, "{conky:?}");
        let all = marked_pids(&Io, &name);
        assert!(all.len() >= 2, "цикл сценария тоже с меткой: {all:?}");
        let cwd = std::fs::read_link(format!("/proc/{}/cwd", conky[0])).unwrap();
        assert_eq!(cwd, d.join("T"));
        let (ok, _) = crate::conky_procs::stop_pids(&Io, "T", conky);
        assert!(ok);
        for _ in 0..30 {
            if marked_pids(&Io, &name).is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(marked_pids(&Io, &name).is_empty(), "после остановки от темы ничего не осталось");
        if let Some(g) = group_file(&name) {
            let _ = std::fs::remove_file(g);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn preview_png_only_next_to_theme_json() {
        let d = std::env::temp_dir().join(format!("rexon-conky-theme-pv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("a")).unwrap();
        std::fs::create_dir_all(d.join("b")).unwrap();
        for dir in ["a", "b"] {
            std::fs::write(d.join(dir).join("preview.png"), b"p").unwrap();
        }
        std::fs::write(d.join("a/theme.json"), "{}").unwrap();
        let io = crate::animations::SystemIo;
        let s = |p: &str| d.join(p).to_string_lossy().into_owned();
        assert_eq!(preview(&io, &s("a")), Some(s("a/preview.png")));
        assert_eq!(preview(&io, &s("b")), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Тема из Conky Studio: отметка `created_with` или файл `.cstudio`;
    /// кнопка — только если Conky Studio найден (AppImage с битом
    /// исполнения в обычных местах или программа в PATH).
    #[test]
    fn studio_theme_and_editor_lookup() {
        use std::os::unix::fs::PermissionsExt;
        struct NoPath;
        impl LookIo for NoPath {
            fn run(&self, _: &[String]) -> (bool, String) {
                (false, String::new())
            }
            fn have(&self, _: &str) -> bool {
                false
            }
            fn read(&self, path: &str) -> Option<String> {
                crate::animations::SystemIo.read(path)
            }
            fn write(&self, _: &str, _: &str) -> bool {
                false
            }
            fn list_dir(&self, path: &str) -> Vec<String> {
                crate::animations::SystemIo.list_dir(path)
            }
        }
        struct InPath;
        impl LookIo for InPath {
            fn run(&self, _: &[String]) -> (bool, String) {
                (false, String::new())
            }
            fn have(&self, p: &str) -> bool {
                p == "conky-studio"
            }
            fn read(&self, path: &str) -> Option<String> {
                crate::animations::SystemIo.read(path)
            }
            fn write(&self, _: &str, _: &str) -> bool {
                false
            }
            fn list_dir(&self, path: &str) -> Vec<String> {
                crate::animations::SystemIo.list_dir(path)
            }
        }
        let d = tmp("studio");
        let conf = "conky.config = {}\nconky.text = [[x]]\n";
        put(&d, ".config/conky/Night/conky.conf", conf);
        put(&d, ".config/conky/Night/theme.json", r#"{"name": "Night", "created_with": "Conky Studio"}"#);
        put(&d, ".config/conky/Proj/conky.conf", conf);
        put(&d, ".config/conky/Proj/Proj.cstudio", "{}");
        put(&d, ".config/conky/Side/conky.conf", conf);
        put(&d, ".config/conky/Side.cstudio", "{}");
        put(&d, ".config/conky/Plain/conky.conf", conf);
        put(&d, ".config/conky/Plain/theme.json", r#"{"created_with": "hand"}"#);
        let ctx = ctx_at(&d);
        let s = |p: &str| d.join(p).to_string_lossy().into_owned();
        assert!(is_studio_theme(&NoPath, &s(".config/conky/Night")));
        assert!(is_studio_theme(&NoPath, &s(".config/conky/Proj")));
        assert!(is_studio_theme(&NoPath, &s(".config/conky/Side")), ".cstudio рядом с папкой");
        assert!(!is_studio_theme(&NoPath, &s(".config/conky/Plain")));
        assert_eq!(studio_edit_argv(&NoPath, &ctx, "Night"), None, "Conky Studio не найден — кнопки нет");
        // AppImage без бита исполнения не годится; с битом — последний по имени.
        put(&d, "Applications/ConkyStudio-1.0.0-x86_64.AppImage", "x");
        assert_eq!(find_studio(&NoPath, &ctx), None);
        put(&d, "Загрузки/ConkyStudio-1.1.0-x86_64.AppImage", "x");
        put(&d, "Загрузки/ConkyStudio-1.1.1-x86_64.AppImage", "x");
        put(&d, "Загрузки/notes.txt", "x");
        put(&d, ".config/user-dirs.dirs", "XDG_DOWNLOAD_DIR=\"$HOME/Загрузки\"\n");
        for f in ["Загрузки/ConkyStudio-1.1.0-x86_64.AppImage", "Загрузки/ConkyStudio-1.1.1-x86_64.AppImage"] {
            std::fs::set_permissions(d.join(f), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let app = s("Загрузки/ConkyStudio-1.1.1-x86_64.AppImage");
        assert_eq!(find_studio(&NoPath, &ctx).as_deref(), Some(app.as_str()));
        assert_eq!(studio_edit_argv(&NoPath, &ctx, "Night"), Some(vec![app.clone()]));
        assert_eq!(studio_edit_argv(&NoPath, &ctx, "Proj"), Some(vec![app.clone(), s(".config/conky/Proj/Proj.cstudio")]));
        assert_eq!(studio_edit_argv(&NoPath, &ctx, "Side"), Some(vec![app, s(".config/conky/Side.cstudio")]));
        assert_eq!(studio_edit_argv(&NoPath, &ctx, "Plain"), None, "тема не из Conky Studio — кнопки нет");
        assert_eq!(studio_edit_argv(&InPath, &ctx, "Night"), Some(vec!["conky-studio".to_string()]), "PATH — первым");
        // Подделка ничего не запускает — честный отказ.
        assert!(!edit_in_studio(&NoPath, &ctx, "Night").0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Многооконная тема (сценарий поднял несколько conky, одно окно — из
    /// копии файла вне папки темы): в ленте, в наборах, в вопросе и в
    /// счёте «Остановить все» — один виджет; ленточный крестик и набор
    /// гасят все окна темы.
    #[test]
    fn multi_window_theme_is_one_widget() {
        crate::i18n::set_language(Some("en"));
        struct Mixed {
            procs: std::collections::HashMap<String, String>,
            ran: std::cell::RefCell<Vec<Vec<String>>>,
        }
        impl LookIo for Mixed {
            fn run(&self, argv: &[String]) -> (bool, String) {
                self.ran.borrow_mut().push(argv.to_vec());
                (true, String::new())
            }
            fn have(&self, _: &str) -> bool {
                true
            }
            fn read(&self, path: &str) -> Option<String> {
                if path.starts_with("/proc/") {
                    return self.procs.get(path).cloned();
                }
                crate::animations::SystemIo.read(path)
            }
            fn write(&self, _: &str, _: &str) -> bool {
                false
            }
            fn list_dir(&self, path: &str) -> Vec<String> {
                if path == "/proc" {
                    let mut v: Vec<String> = self
                        .procs
                        .keys()
                        .filter_map(|k| k.strip_prefix("/proc/")?.split_once('/').map(|(p, _)| p.to_string()))
                        .collect();
                    v.sort();
                    v.dedup();
                    return v;
                }
                crate::animations::SystemIo.list_dir(path)
            }
        }
        let d = tmp("multi");
        let conf = "conky.config = {}\nconky.text = [[x]]\n";
        put(&d, ".config/conky/Night/conky.conf", conf);
        put(&d, ".config/conky/Night/conky_window_2.conf", conf);
        put(&d, ".config/conky/Night/start.sh", "#!/bin/sh\nconky -c conky.conf &\nexec conky -c conky_window_2.conf\n");
        put(&d, ".config/conky/Other/conky.conf", conf);
        let ctx = ctx_at(&d);
        let s = |p: &str| d.join(p).to_string_lossy().into_owned();
        let night = s(".config/conky/Night");
        let (main, w2, other) = (s(".config/conky/Night/conky.conf"), s(".config/conky/Night/conky_window_2.conf"), s(".config/conky/Other/conky.conf"));
        let mut f = std::collections::HashMap::new();
        fake_proc(&mut f, 200, &["sh", &s(".config/conky/Night/start.sh")], "sh", Some(&night));
        fake_proc(&mut f, 201, &["conky", "-c", &main], "conky", Some(&night));
        fake_proc(&mut f, 202, &["conky", "-c", &w2], "conky", Some(&night));
        fake_proc(&mut f, 203, &["conky", "-c", "/run/user/1000/Night-DP-1.conf"], "conky", Some(&night));
        fake_proc(&mut f, 300, &["conky", "-c", &other], "conky", None);
        let io = Mixed { procs: f, ran: Default::default() };
        assert_eq!(widget_of_dir(&ctx, &[], &night), "Night");
        assert_eq!(crate::conky_procs::running_widget_confs(&io, &ctx), [main.clone(), other.clone()]);
        assert_eq!(unit_confs(&io, &ctx, &[], &[w2.clone(), main.clone(), other.clone()]), [main.clone(), other.clone()]);
        let ribbon = crate::conky_procs::ribbon(&io, &ctx, &[]);
        assert_eq!(ribbon.len(), 2, "{ribbon:?}");
        assert_eq!(ribbon[0].item_id, "local:conky:Night");
        assert_eq!(ribbon[0].confs, [main.clone(), w2.clone(), "/run/user/1000/Night-DP-1.conf".to_string()]);
        let ids = crate::de_apply::conky_running_ids(&io, &ctx);
        assert_eq!(ids.iter().filter(|i| *i == "Night").count(), 1, "{ids:?}");
        let procs = crate::conky_procs::all(&io);
        let (_, text) = crate::conky_procs::stop_all_question(&ctx, &[], &procs).unwrap();
        assert_eq!(text.matches("• Night").count(), 1, "{text}");
        // Набор гасит тему по главному файлу — со всеми окнами.
        let pids = crate::conky_procs::pids_of_units(&io, &ctx, std::slice::from_ref(&main));
        assert_eq!(pids, ["201", "202", "203"]);
        let pids: Vec<String> = procs.iter().map(|p| p.pid.clone()).collect();
        let (ok, msg) = crate::conky_procs::stop_all(&io, &pids);
        assert!(ok);
        assert_eq!(msg, "⏹️ Stopped 2 Conky widgets");
        let _ = std::fs::remove_dir_all(&d);
    }

}
