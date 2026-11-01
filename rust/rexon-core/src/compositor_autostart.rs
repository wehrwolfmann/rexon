//! Автозапуск при входе там, где окружение НЕ исполняет XDG autostart.
//!
//! Записи `~/.config/autostart/*.desktop` исполняют полные окружения (KDE,
//! GNOME, Cinnamon, XFCE, MATE, Budgie, LXQt, COSMIC) и сессии под systemd,
//! в которых поднимается `xdg-desktop-autostart.target`. Голые композиторы
//! их не читают вовсе, и переключатель «Запускать при входе» горел, а после
//! входа не стартовало ничего: ни лоток, ни заставка, ни движок обоев.
//!
//! Кто что исполняет (сверено по исходникам и man-страницам, 02.10.2026):
//!
//! | композитор | XDG autostart                                  | родной способ                         |
//! |------------|------------------------------------------------|---------------------------------------|
//! | sway       | нет (только при sway-systemd / своих строках)  | `exec …` в `~/.config/sway/config`    |
//! | Hyprland   | нет; да только под uwsm                        | `hl.on("hyprland.start", …)` в `hyprland.lua`, `exec-once =` в `hyprland.conf` |
//! | niri       | да через `niri-session` (systemd), иначе нет   | `spawn-at-startup` в `config.kdl`     |
//! | river      | нет                                            | исполняемый `~/.config/river/init`    |
//! | labwc      | нет                                            | скрипт `~/.config/labwc/autostart`    |
//! | Wayfire    | нет                                            | раздел `[autostart]` в `wayfire.ini`  |
//!
//! Поэтому в этих шести Rexon пишет СВОЮ строку в родной конфиг — управляемым
//! блоком между метками — и прячет от них `.desktop`-запись ключом
//! `NotShowIn=` ([`with_not_show_in`]). Получается ровно один путь запуска на
//! окружение: где композитор исполняет и XDG, и свою строку (niri из
//! `niri-session`, Hyprland под uwsm), `.desktop` для него скрыт, и программа
//! не стартует дважды. `NotShowIn=` получает только тот композитор, в чей
//! конфиг строка РЕАЛЬНО легла: иначе под uwsm спрятали бы запись, у которой
//! не осталось замены.
//!
//! Блок обратим бит-в-бит: метка начала помнит, добавлял ли Rexon перевод
//! строки перед блоком (` +nl`) и заводил ли файл сам (` +new`); снятие
//! возвращает файл к прежним байтам, а заведённый файл — удаляет.
//!
//! ⚠️ Файлы, которых нет, заводятся только там, где это ничего не ломает:
//! * sway — свой `config` с `include` системного (без него свой файл
//!   отключил бы системный конфиг целиком, вместе с клавишами);
//! * labwc — свой `autostart`, который сперва исполняет системный;
//! * Wayfire — свой `wayfire.ini` (умолчания всё равно читаются из
//!   `defaults.ini`).
//!
//! Hyprland и niri при первом входе сами кладут конфиг по умолчанию — если
//! опередить их файлом из одной строки, человек остался бы без клавиш.
//! river без `init` бесполезен сам по себе. Там файла не заводим и честно
//! говорим, что строка не легла.
//!
//! Общий помощник для чужих строк автозапуска (например, шелл, который
//! ставит вкладка «Рабочие столы»): [`ensure_block`] / [`remove_block`] на
//! файл, [`resolve_target`] — какой файл у композитора главный,
//! [`launch_line`] — строка запуска в его синтаксисе.

use std::path::Path;

use crate::i18n::{n_, tr, trf};

// ============================================================================
// Композиторы и синтаксисы
// ============================================================================

/// Композитор, который сам XDG-записи автозапуска не исполняет.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Compositor {
    Sway,
    Hyprland,
    Niri,
    River,
    Labwc,
    Wayfire,
}

impl Compositor {
    /// Все, по порядку.
    pub const ALL: [Compositor; 6] = [
        Compositor::Sway,
        Compositor::Hyprland,
        Compositor::Niri,
        Compositor::River,
        Compositor::Labwc,
        Compositor::Wayfire,
    ];

    /// Имя для человека (имена программ не переводятся).
    pub fn display_name(self) -> &'static str {
        match self {
            Compositor::Sway => "Sway",
            Compositor::Hyprland => "Hyprland",
            Compositor::Niri => "niri",
            Compositor::River => "river",
            Compositor::Labwc => "labwc",
            Compositor::Wayfire => "Wayfire",
        }
    }

    /// Имена в `XDG_CURRENT_DESKTOP` (их же получает `NotShowIn=`):
    /// `DesktopNames=` сеансовых файлов плюс то, что композитор ставит сам.
    pub fn desktop_names(self) -> &'static [&'static str] {
        match self {
            Compositor::Sway => &["sway"],
            Compositor::Hyprland => &["Hyprland"],
            Compositor::Niri => &["niri"],
            Compositor::River => &["river"],
            Compositor::Labwc => &["labwc"],
            Compositor::Wayfire => &["Wayfire", "wayfire"],
        }
    }

    /// Бинари в `PATH`, по которым видно, что композитор стоит.
    pub fn binaries(self) -> &'static [&'static str] {
        match self {
            Compositor::Sway => &["sway"],
            Compositor::Hyprland => &["Hyprland", "hyprland"],
            Compositor::Niri => &["niri"],
            Compositor::River => &["river"],
            Compositor::Labwc => &["labwc"],
            Compositor::Wayfire => &["wayfire"],
        }
    }

    /// Переменная, которую композитор ставит своим детям, — признак того,
    /// что сеанс сейчас его. У river такой нет (только `XDG_CURRENT_DESKTOP`).
    fn session_marker(self) -> Option<&'static str> {
        match self {
            Compositor::Sway => Some("SWAYSOCK"),
            Compositor::Hyprland => Some("HYPRLAND_INSTANCE_SIGNATURE"),
            Compositor::Niri => Some("NIRI_SOCKET"),
            Compositor::River => None,
            Compositor::Labwc => Some("LABWC_PID"),
            Compositor::Wayfire => Some("WAYFIRE_CONFIG_FILE"),
        }
    }
}

/// Синтаксис файла, в который ложится блок.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syntax {
    /// `~/.config/sway/config`: `exec <команда оболочки>`.
    Sway,
    /// `hyprland.conf` (hyprlang): `exec-once = <команда>`.
    HyprConf,
    /// `hyprland.lua`: `hl.on("hyprland.start", …)`.
    HyprLua,
    /// `config.kdl`: `spawn-at-startup "argv0" "argv1" …`.
    NiriKdl,
    /// Скрипт оболочки (river `init`, labwc `autostart`): `<команда> &`.
    Shell,
    /// `wayfire.ini`: ключ в разделе `[autostart]`.
    WayfireIni,
}

impl Syntax {
    /// Начало строки-комментария.
    pub fn comment(self) -> &'static str {
        match self {
            Syntax::HyprLua => "--",
            Syntax::NiriKdl => "//",
            _ => "#",
        }
    }
}

// ============================================================================
// Строка запуска
// ============================================================================

/// Почему строку в этот конфиг не написать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Главного файла настроек нет, а заводить его самим нельзя.
    ConfigMissing,
    /// Файл устроен так, что безопасно дописать его нельзя.
    Unsupported,
    /// В команде есть знаки, которых этот синтаксис не держит.
    BadCharacters,
    /// В конфиге выключен модуль автозапуска (Wayfire).
    PluginOff,
    /// Запись на диск не удалась.
    Io(String),
}

// TRANSLATORS: Reason Rexon couldn't add its startup line to a window manager's config: the config file doesn't exist yet.
const R_MISSING: &str = n_("its settings file wasn't found");
// TRANSLATORS: Reason Rexon couldn't add its startup line to a window manager's config.
const R_UNSUPPORTED: &str = n_("its settings file is arranged in a way Rexon can't safely add to");
// TRANSLATORS: Reason Rexon couldn't add its startup line to a window manager's config.
const R_CHARS: &str = n_("the launch command has characters this settings file can't hold");
// TRANSLATORS: Reason Rexon couldn't add its startup line to the Wayfire config: its autostart plugin is disabled.
const R_PLUGIN: &str = n_("startup programs are turned off in its settings");
// TRANSLATORS: Reason Rexon couldn't add its startup line to a window manager's config; %1 is the system error.
const R_IO: &str = n_("its settings file couldn't be changed (%1)");
const MSG_NOT_COVERED: &str =
    // TRANSLATORS: %1 is a window manager name (Sway, Hyprland…), %2 is the reason.
    n_("%1 doesn't start programs from the common startup list, and Rexon couldn't add itself to its settings: %2.");
// TRANSLATORS: Manual fallback; %1 is a settings file path, %2 is a line to paste.
const MSG_MANUAL: &str = n_("Add this line to %1 yourself: %2");

impl Refusal {
    /// Причина словами человека.
    pub fn text(&self) -> String {
        match self {
            Refusal::ConfigMissing => tr(R_MISSING),
            Refusal::Unsupported => tr(R_UNSUPPORTED),
            Refusal::BadCharacters => tr(R_CHARS),
            Refusal::PluginOff => tr(R_PLUGIN),
            Refusal::Io(e) => trf(R_IO, &[e]),
        }
    }
}

/// Знак, которому в оболочке кавычки не нужны. Не-ASCII (кириллица в путях)
/// оболочка держит как часть слова.
fn shell_safe(c: char) -> bool {
    c.is_alphanumeric() || "_@%+=:,./-".contains(c)
}

/// Одно слово для `sh -c`: как есть или в одинарных кавычках.
pub fn shell_quote(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(shell_safe) {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// argv → командная строка оболочки.
pub fn shell_command(argv: &[String]) -> String {
    argv.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ")
}

/// Строка KDL в кавычках.
fn kdl_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Ключ Wayfire для своей записи: `rexon_<id>` (дефис в ключе не нужен).
fn wayfire_key(id: &str) -> String {
    format!("rexon_{}", id.replace('-', "_"))
}

/// Строки тела блока: запуск `argv` в синтаксисе `syntax`.
///
/// `id` нужен только Wayfire — ключи раздела должны различаться.
pub fn launch_line(syntax: Syntax, id: &str, argv: &[String]) -> Result<Vec<String>, Refusal> {
    if argv.is_empty() || argv.iter().any(|a| a.contains('\n') || a.contains('\r') || a.contains('\0')) {
        return Err(Refusal::BadCharacters);
    }
    let sh = shell_command(argv);
    Ok(match syntax {
        Syntax::Sway => {
            // sway разворачивает `$имя` и снимает экранирование `\` ещё до
            // оболочки (`unescape_string` у всех аргументов `exec`).
            if sh.contains('$') || sh.contains('\\') {
                return Err(Refusal::BadCharacters);
            }
            // `,` вне кавычек sway считает границей команд (`;` оболочка и
            // так берёт в кавычки) — слово с запятой идёт в кавычках.
            let words: Vec<String> = argv
                .iter()
                .map(|a| if a.contains(',') { format!("'{}'", a.replace('\'', "'\\''")) } else { shell_quote(a) })
                .collect();
            let sh = words.join(" ");
            // Единственное слово в кавычках sway раскавычивает сам, и
            // оболочка резала бы путь по пробелам: оборачиваем ещё раз.
            if words.len() == 1 && sh.starts_with('\'') {
                if sh.contains('"') {
                    return Err(Refusal::BadCharacters);
                }
                return Ok(vec![format!("exec \"{sh}\"")]);
            }
            vec![format!("exec {sh}")]
        }
        Syntax::HyprConf => {
            // hyprlang: `#` — комментарий, `$` — переменная.
            if sh.contains('#') || sh.contains('$') {
                return Err(Refusal::BadCharacters);
            }
            vec![format!("exec-once = {sh}")]
        }
        Syntax::HyprLua => {
            let lua = sh.replace('\\', "\\\\").replace('"', "\\\"");
            vec![format!("hl.on(\"hyprland.start\", function() hl.exec_cmd(\"{lua}\") end)")]
        }
        Syntax::NiriKdl => {
            let args: Vec<String> = argv.iter().map(|a| kdl_string(a)).collect();
            vec![format!("spawn-at-startup {}", args.join(" "))]
        }
        Syntax::Shell => vec![format!("{sh} &")],
        Syntax::WayfireIni => {
            // wf-config: `#` начинает комментарий, `\` — экранирование.
            if sh.contains('#') || sh.contains('\\') {
                return Err(Refusal::BadCharacters);
            }
            vec![format!("{} = {sh}", wayfire_key(id))]
        }
    })
}

// ============================================================================
// Управляемый блок (чистые функции над текстом)
// ============================================================================

const FLAG_NL: &str = " +nl";
const FLAG_NEW: &str = " +new";

fn begin_prefix(syntax: Syntax, id: &str) -> String {
    format!("{} >>> rexon:{id}", syntax.comment())
}

fn begin_line(syntax: Syntax, id: &str, added_nl: bool, new_file: bool) -> String {
    let mut s = begin_prefix(syntax, id);
    if added_nl {
        s.push_str(FLAG_NL);
    }
    if new_file {
        s.push_str(FLAG_NEW);
    }
    s.push_str(" >>> managed by Rexon, edits are overwritten");
    s
}

fn end_line(syntax: Syntax, id: &str) -> String {
    format!("{} <<< rexon:{id} <<<", syntax.comment())
}

/// Найденный блок: байтовые границы строк начала и конца (конец — после
/// перевода строки метки конца, если он есть) и флаги метки.
struct Found {
    start: usize,
    /// Начало тела (сразу за строкой метки начала).
    body_start: usize,
    /// Начало строки метки конца.
    end_line_start: usize,
    end: usize,
    body: String,
    added_nl: bool,
    new_file: bool,
}

/// Начало и конец каждой строки (с её `\n`).
fn line_spans(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut pos = 0;
    for piece in text.split_inclusive('\n') {
        out.push((pos, pos + piece.len()));
        pos += piece.len();
    }
    out
}

fn find_block(text: &str, syntax: Syntax, id: &str) -> Option<Found> {
    let prefix = begin_prefix(syntax, id);
    let end_marker = end_line(syntax, id);
    let spans = line_spans(text);
    let mut i = 0;
    while i < spans.len() {
        let (s, e) = spans[i];
        let line = text[s..e].trim_end_matches(['\n', '\r']);
        // `rexon:tray` не должен поймать `rexon:tray-extra`.
        if let Some(rest) = line.strip_prefix(&prefix) {
            if rest.starts_with(' ') {
                let flags = rest.split(" >>>").next().unwrap_or("");
                let added_nl = flags.contains(FLAG_NL.trim());
                let new_file = flags.contains(FLAG_NEW.trim());
                for &(bs, be) in spans.iter().skip(i + 1) {
                    let l = text[bs..be].trim_end_matches(['\n', '\r']);
                    if l == end_marker {
                        let body_start = spans[i].1;
                        return Some(Found {
                            start: s,
                            body_start,
                            end_line_start: bs,
                            end: be,
                            body: text[body_start..bs].to_string(),
                            added_nl,
                            new_file,
                        });
                    }
                }
                return None; // метка начала без конца — не трогаем
            }
        }
        i += 1;
    }
    None
}

/// Есть ли в тексте блок `id`.
pub fn has_block(text: &str, syntax: Syntax, id: &str) -> bool {
    find_block(text, syntax, id).is_some()
}

/// Куда в существующем файле вставлять новый блок.
fn insertion_point(text: &str, syntax: Syntax) -> Result<Option<usize>, Refusal> {
    match syntax {
        Syntax::Shell => {
            // `exec` в начале строки заменяет оболочку — всё после него не
            // исполнится. Встаём перед первым таким.
            for (s, e) in line_spans(text) {
                let l = &text[s..e];
                if l.starts_with("exec ") || l.starts_with("exec\t") || l.trim_end() == "exec" {
                    return Ok(Some(s));
                }
            }
            Ok(None)
        }
        Syntax::HyprLua => {
            // После `return` верхнего уровня Lua не принимает операторов.
            if line_spans(text).iter().any(|&(s, e)| {
                let l = &text[s..e];
                l.starts_with("return") && l[6..].chars().next().is_none_or(|c| !c.is_alphanumeric() && c != '_')
            }) {
                return Err(Refusal::Unsupported);
            }
            Ok(None)
        }
        Syntax::WayfireIni => {
            // Внутри уже существующего раздела `[autostart]` — сразу после
            // заголовка (второй такой раздел wf-config не обязан склеивать).
            // Заголовок завёл наш же блок — встаём за ЕГО меткой конца:
            // иначе новый блок оказался бы внутри старого и ушёл бы вместе
            // с ним при снятии.
            let mut open_end: Option<String> = None;
            let mut header_inside = false;
            for (s, e) in line_spans(text) {
                let l = text[s..e].trim_end_matches(['\n', '\r']);
                let at_end = if text[s..e].ends_with('\n') { Some(e) } else { None };
                if let Some(end) = &open_end {
                    if l == end {
                        if header_inside {
                            return Ok(at_end);
                        }
                        open_end = None;
                    } else if l.trim() == "[autostart]" {
                        header_inside = true;
                    }
                    continue;
                }
                if l.trim() == "[autostart]" {
                    return Ok(at_end); // заголовок последней строкой — в конец
                }
                if let Some(id) = our_block_id(l, syntax) {
                    open_end = Some(end_line(syntax, id));
                }
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// Метка начала нашего блока (любого) — его `id`.
fn our_block_id(line: &str, syntax: Syntax) -> Option<&str> {
    let head = format!("{} >>> rexon:", syntax.comment());
    let rest = line.strip_prefix(&head)?;
    rest.find(' ').map(|i| &rest[..i])
}

/// Строка — преамбула, которую блок несёт за весь файл, а не за себя:
/// sway — `include` системного конфига в заведённом нами файле, labwc —
/// исполнение системного `autostart`, Wayfire — заголовок `[autostart]`.
/// Пока в файле есть наши блоки, она должна в нём остаться.
fn preamble_like(syntax: Syntax, line: &str) -> bool {
    match syntax {
        Syntax::Sway => line.starts_with("include "),
        Syntax::Shell => line.starts_with(". "),
        Syntax::WayfireIni => line.trim() == "[autostart]",
        _ => false,
    }
}

/// Есть ли в `wayfire.ini` раздел `[autostart]`.
fn wayfire_has_section(text: &str) -> bool {
    text.lines().any(|l| l.trim() == "[autostart]")
}

/// Итог правки текста.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextEdit {
    /// Ничего менять не надо.
    Unchanged,
    /// Новый текст файла.
    Write(String),
    /// Файл был заведён нами и теперь пуст — удалить.
    Delete,
}

/// Поставить/обновить блок `id` с телом `body` (строки без `\n`).
///
/// `existing` — текущий текст или `None`, если файла нет (тогда файл
/// заводится, и метка помнит это). `preamble` — строки, которые идут в блок
/// ПЕРЕД телом только при заведении файла (sway: `include` системного).
pub fn upsert_text(
    existing: Option<&str>,
    syntax: Syntax,
    id: &str,
    body: &[String],
    preamble: &[String],
) -> Result<TextEdit, Refusal> {
    let mut body_text = String::new();
    let new_file = existing.is_none();
    if new_file {
        for l in preamble {
            body_text.push_str(l);
            body_text.push('\n');
        }
    }
    let wf_new_section = syntax == Syntax::WayfireIni
        && !existing.is_some_and(wayfire_has_section);
    if wf_new_section {
        body_text.push_str("[autostart]\n");
    }
    for l in body {
        body_text.push_str(l);
        body_text.push('\n');
    }

    let text = existing.unwrap_or("");
    if let Some(found) = find_block(text, syntax, id) {
        // Обновление: преамбула и заголовок раздела у старого блока свои —
        // меняются только строки тела, флаги и окружение блока остаются.
        let old_body = &found.body;
        let mut kept = String::new();
        for l in old_body.split_inclusive('\n') {
            let t = l.trim_end_matches(['\n', '\r']);
            let is_body = body_like(syntax, t);
            if !is_body {
                kept.push_str(l);
            }
        }
        let mut new_body = kept;
        for l in body {
            new_body.push_str(l);
            new_body.push('\n');
        }
        if &new_body == old_body {
            return Ok(TextEdit::Unchanged);
        }
        let mut out = String::with_capacity(text.len() + new_body.len());
        out.push_str(&text[..found.body_start]);
        out.push_str(&new_body);
        out.push_str(&text[found.end_line_start..]);
        return Ok(TextEdit::Write(out));
    }

    let at = if new_file { None } else { insertion_point(text, syntax)? };
    let added_nl = at.is_none() && !text.is_empty() && !text.ends_with('\n');
    let mut block = begin_line(syntax, id, added_nl, new_file);
    block.push('\n');
    block.push_str(&body_text);
    block.push_str(&end_line(syntax, id));
    block.push('\n');

    let out = match at {
        Some(pos) => format!("{}{}{}", &text[..pos], block, &text[pos..]),
        None => {
            let mut s = text.to_string();
            if added_nl {
                s.push('\n');
            }
            s.push_str(&block);
            s
        }
    };
    Ok(TextEdit::Write(out))
}

/// Строка — наше тело запуска (а не преамбула/заголовок раздела)?
fn body_like(syntax: Syntax, line: &str) -> bool {
    match syntax {
        Syntax::Sway => line.starts_with("exec "),
        Syntax::HyprConf => line.starts_with("exec-once"),
        Syntax::HyprLua => line.starts_with("hl.on("),
        Syntax::NiriKdl => line.starts_with("spawn-at-startup"),
        Syntax::Shell => line.ends_with(" &"),
        Syntax::WayfireIni => line.starts_with("rexon_"),
    }
}

/// Снять блок `id`: текст становится байт-в-байт прежним.
pub fn remove_text(existing: &str, syntax: Syntax, id: &str) -> TextEdit {
    let Some(found) = find_block(existing, syntax, id) else {
        return TextEdit::Unchanged;
    };
    let mut start = found.start;
    let mut tail = existing[found.end..].to_string();
    let our_nl = found.added_nl && start > 0 && existing.as_bytes()[start - 1] == b'\n';
    // Файл завели мы, и до блока в нём ничего нет.
    let our_file = found.new_file && start == 0;
    if tail.is_empty() {
        if our_nl {
            start -= 1;
        }
    } else if our_nl || our_file {
        // За блоком стоит другой наш блок (его поставили позже — файл уже
        // кончался нашим `\n`)? Сними мы перевод строки — метка следующего
        // блока прилипла бы к последней строке человека, а заведённый нами
        // файл после снятия всех блоков остался бы пустым, а не удалён.
        // Перевод строки и файл переходят к следующему блоку вместе с
        // флагами: его снятие вернёт файлу прежние байты.
        // За блоком чужие строки (человек дописал их после нас) — перевод
        // строки перед ними нужен, и он остаётся.
        if let Some(next) = inherit_flags(&tail, syntax, our_nl, our_file) {
            tail = next;
        }
    }
    // Преамбула блока (sway: `include` системного конфига) принадлежит
    // файлу, а не блоку: снять её, пока за блоком что-то остаётся, — значит
    // отключить системный конфиг вместе с клавишами. Следующий наш блок
    // забирает её себе (и уйдёт с ней последним); чужие строки получают
    // её как обычные строки перед собой.
    let preamble: String = found
        .body
        .split_inclusive('\n')
        .filter(|l| preamble_like(syntax, l.trim_end_matches(['\n', '\r'])))
        .collect();
    if !preamble.is_empty() && !tail.is_empty() {
        tail = match our_block_id(tail.lines().next().unwrap_or(""), syntax) {
            Some(_) => {
                let first = tail.find('\n').map_or(tail.len(), |i| i + 1);
                let rest = &tail[first..];
                let missing: String = preamble
                    .split_inclusive('\n')
                    .filter(|l| !rest.split_inclusive('\n').any(|r| r.trim_end() == l.trim_end()))
                    .collect();
                let sep = if first == tail.len() && !tail.ends_with('\n') { "\n" } else { "" };
                format!("{}{sep}{missing}{rest}", &tail[..first])
            }
            None => format!("{preamble}{tail}"),
        };
    }
    let out = format!("{}{}", &existing[..start], tail);
    if found.new_file && out.is_empty() {
        return TextEdit::Delete;
    }
    TextEdit::Write(out)
}

/// Если `tail` начинается с метки начала нашего блока — вернуть `tail`, где
/// этой метке добавлены флаги `add_nl`/`add_new` (наследство снимаемого
/// блока); иначе `None`.
fn inherit_flags(tail: &str, syntax: Syntax, add_nl: bool, add_new: bool) -> Option<String> {
    let line_end = tail.find('\n').unwrap_or(tail.len());
    let line = tail[..line_end].trim_end_matches('\r');
    let head = format!("{} >>> rexon:", syntax.comment());
    let rest = line.strip_prefix(&head)?;
    let id = &rest[..rest.find(' ')?];
    let flags = rest[id.len()..].split(" >>>").next().unwrap_or("");
    let nl = flags.contains(FLAG_NL.trim()) || add_nl;
    let new = flags.contains(FLAG_NEW.trim()) || add_new;
    Some(format!("{}{}", begin_line(syntax, id, nl, new), &tail[line.len()..]))
}

// ============================================================================
// Окружение и выбор файла
// ============================================================================

/// Что известно о машине и сеансе. Живое — [`SessionEnv::live`]; сверки
/// собирают своё с временным домашним каталогом.
#[derive(Debug, Clone, Default)]
pub struct SessionEnv {
    pub home: String,
    /// `$XDG_CONFIG_HOME` или `~/.config`.
    pub config_home: String,
    /// `$XDG_CONFIG_DIRS` или `/etc/xdg`.
    pub config_dirs: Vec<String>,
    /// Системный каталог конфигов (`/etc`): sway и niri ищут там.
    pub sysconf: String,
    /// Компоненты `XDG_CURRENT_DESKTOP`.
    pub current_desktop: Vec<String>,
    /// Переменные, которые здесь читаются (маркеры сеанса и переопределения
    /// путей конфигов); пустые не кладутся.
    pub vars: std::collections::BTreeMap<String, String>,
    /// Какие композиторы стоят (бинарь в `PATH`).
    pub installed: Vec<Compositor>,
}

/// Переменные, которые [`SessionEnv::live`] снимает с процесса.
const LIVE_VARS: &[&str] = &[
    "SWAYSOCK",
    "HYPRLAND_INSTANCE_SIGNATURE",
    "HYPRLAND_CONFIG",
    "NIRI_SOCKET",
    "NIRI_CONFIG",
    "LABWC_PID",
    "WAYFIRE_CONFIG_FILE",
];

impl SessionEnv {
    /// Окружение живого процесса.
    pub fn live() -> Self {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let home = get("HOME").unwrap_or_default();
        let config_home = crate::utils::config_home(get("XDG_CONFIG_HOME").as_deref(), &home);
        let config_dirs = get("XDG_CONFIG_DIRS")
            .map(|v| v.split(':').filter(|s| !s.is_empty()).map(str::to_string).collect())
            .unwrap_or_else(|| vec!["/etc/xdg".to_string()]);
        let current_desktop = get("XDG_CURRENT_DESKTOP")
            .map(|v| v.split(':').filter(|s| !s.is_empty()).map(str::to_string).collect())
            .unwrap_or_default();
        let vars = LIVE_VARS
            .iter()
            .filter_map(|k| get(k).map(|v| (k.to_string(), v)))
            .collect();
        let path = std::env::var("PATH").ok();
        let installed = Compositor::ALL
            .into_iter()
            .filter(|c| c.binaries().iter().any(|b| crate::utils::which(b, path.as_deref()).is_some()))
            .collect();
        SessionEnv {
            home,
            config_home,
            config_dirs,
            sysconf: "/etc".to_string(),
            current_desktop,
            vars,
            installed,
        }
    }

    fn var(&self, k: &str) -> Option<&str> {
        self.vars.get(k).map(String::as_str).filter(|v| !v.is_empty())
    }
}

/// Полные окружения, которые XDG-записи исполняют сами (своим менеджером
/// сеанса или через systemd). Сравнение без учёта регистра.
const XDG_DESKTOPS: &[&str] = &[
    "KDE", "GNOME", "X-Cinnamon", "Cinnamon", "XFCE", "MATE", "Budgie", "LXQt", "LXDE",
    "COSMIC", "Pantheon", "Unity", "Deepin", "DDE", "Enlightenment", "Lumina", "UKUI",
];

/// Чей сеанс сейчас идёт.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentSession {
    /// Полное окружение — XDG-записи исполняются.
    FullDesktop,
    /// Один из шести композиторов.
    Compositor(Compositor),
    /// Не распознан — судим только по наличию `.desktop`, как прежде.
    Unknown,
}

/// Распознать сеанс. Полное окружение важнее маркеров композитора: Budgie
/// и Xfce на Wayland бегут поверх labwc, LXQt — поверх любого из шести, и
/// автозапуск у них исполняет свой менеджер сеанса.
pub fn current_session(env: &SessionEnv) -> CurrentSession {
    let has_name = |n: &str| env.current_desktop.iter().any(|d| d.eq_ignore_ascii_case(n));
    if XDG_DESKTOPS.iter().any(|n| has_name(n)) {
        return CurrentSession::FullDesktop;
    }
    for c in Compositor::ALL {
        if c.session_marker().is_some_and(|m| env.var(m).is_some())
            || c.desktop_names().iter().any(|n| has_name(n))
        {
            return CurrentSession::Compositor(c);
        }
    }
    CurrentSession::Unknown
}

/// Какой файл у композитора главный и как его править.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub path: String,
    pub syntax: Syntax,
    /// Файла нет. `Some(преамбула)` — заводить можно с этими строками
    /// впереди; `None` у несуществующего файла — заводить нельзя.
    pub create_with: Option<Vec<String>>,
    pub exists: bool,
}

fn is_file(p: &str) -> bool {
    Path::new(p).is_file()
}

fn join(dir: &str, rel: &str) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), rel)
}

/// Главный файл композитора — тем же порядком поиска, что у него самого.
pub fn resolve_target(c: Compositor, env: &SessionEnv) -> Target {
    let existing = |path: String, syntax: Syntax| Target { path, syntax, create_with: None, exists: true };
    let missing = |path: String, syntax: Syntax, create_with: Option<Vec<String>>| Target {
        path,
        syntax,
        create_with,
        exists: false,
    };
    match c {
        Compositor::Sway => {
            // sway/config.c `get_config_path`.
            let cands = [
                join(&env.home, ".sway/config"),
                join(&env.config_home, "sway/config"),
                join(&env.home, ".i3/config"),
                join(&env.config_home, "i3/config"),
            ];
            if let Some(p) = cands.iter().find(|p| is_file(p)) {
                return existing(p.clone(), Syntax::Sway);
            }
            let system = [join(&env.sysconf, "sway/config"), join(&env.sysconf, "i3/config")]
                .into_iter()
                .find(|p| is_file(p));
            let preamble = system.map(|s| vec![format!("include {}", crate::themes::sway_path(&s))]);
            missing(join(&env.config_home, "sway/config"), Syntax::Sway, preamble)
        }
        Compositor::Hyprland => {
            if let Some(p) = env.var("HYPRLAND_CONFIG") {
                let syntax = if p.ends_with(".lua") { Syntax::HyprLua } else { Syntax::HyprConf };
                return if is_file(p) { existing(p.to_string(), syntax) } else { missing(p.to_string(), syntax, None) };
            }
            // 0.55+: сначала `hyprland.lua`, без него — прежний `hyprland.conf`.
            let lua = join(&env.config_home, "hypr/hyprland.lua");
            let conf = join(&env.config_home, "hypr/hyprland.conf");
            if is_file(&lua) {
                existing(lua, Syntax::HyprLua)
            } else if is_file(&conf) {
                existing(conf, Syntax::HyprConf)
            } else {
                missing(lua, Syntax::HyprLua, None)
            }
        }
        Compositor::Niri => {
            let p = env
                .var("NIRI_CONFIG")
                .map(str::to_string)
                .unwrap_or_else(|| join(&env.config_home, "niri/config.kdl"));
            if is_file(&p) { existing(p, Syntax::NiriKdl) } else { missing(p, Syntax::NiriKdl, None) }
        }
        Compositor::River => {
            let p = join(&env.config_home, "river/init");
            if is_file(&p) { existing(p, Syntax::Shell) } else { missing(p, Syntax::Shell, None) }
        }
        Compositor::Labwc => {
            let p = join(&env.config_home, "labwc/autostart");
            if is_file(&p) {
                return existing(p, Syntax::Shell);
            }
            // labwc читает ПЕРВЫЙ найденный `autostart`: свой файл заслонит
            // системный, поэтому заведённый сперва исполняет его.
            let system = env.config_dirs.iter().map(|d| join(d, "labwc/autostart")).find(|p| is_file(p));
            let preamble = match system {
                Some(s) => vec![format!(". {}", shell_quote(&s))],
                None => Vec::new(),
            };
            missing(p, Syntax::Shell, Some(preamble))
        }
        Compositor::Wayfire => {
            let p = match env.var("WAYFIRE_CONFIG_FILE") {
                Some(p) => p.to_string(),
                None => {
                    let vendored = join(&env.config_home, "wayfire/wayfire.ini");
                    if is_file(&vendored) { vendored } else { join(&env.config_home, "wayfire.ini") }
                }
            };
            if is_file(&p) { existing(p, Syntax::WayfireIni) } else { missing(p, Syntax::WayfireIni, Some(Vec::new())) }
        }
    }
}

/// Все файлы, где у композитора МОЖЕТ лежать наш блок (для снятия).
fn all_candidates(c: Compositor, env: &SessionEnv) -> Vec<(String, Syntax)> {
    let mut out = vec![{
        let t = resolve_target(c, env);
        (t.path, t.syntax)
    }];
    let extra: Vec<(String, Syntax)> = match c {
        Compositor::Sway => vec![
            (join(&env.home, ".sway/config"), Syntax::Sway),
            (join(&env.config_home, "sway/config"), Syntax::Sway),
            (join(&env.home, ".i3/config"), Syntax::Sway),
            (join(&env.config_home, "i3/config"), Syntax::Sway),
        ],
        Compositor::Hyprland => vec![
            (join(&env.config_home, "hypr/hyprland.lua"), Syntax::HyprLua),
            (join(&env.config_home, "hypr/hyprland.conf"), Syntax::HyprConf),
        ],
        Compositor::Niri => vec![(join(&env.config_home, "niri/config.kdl"), Syntax::NiriKdl)],
        Compositor::River => vec![],
        Compositor::Labwc => vec![],
        Compositor::Wayfire => vec![
            (join(&env.config_home, "wayfire/wayfire.ini"), Syntax::WayfireIni),
            (join(&env.config_home, "wayfire.ini"), Syntax::WayfireIni),
        ],
    };
    for e in extra {
        if !out.contains(&e) {
            out.push(e);
        }
    }
    out
}

/// Проверки самого файла до правки: формат, который нельзя дописать.
fn precheck(c: Compositor, target: &Target, text: Option<&str>) -> Result<(), Refusal> {
    match (c, text) {
        (Compositor::River, Some(t)) => {
            // init исполняется как программа: дописывать можно только скрипт
            // оболочки, и он обязан остаться исполняемым.
            let first = t.lines().next().unwrap_or("");
            let shell = first.starts_with("#!")
                && ["sh", "bash", "dash", "zsh", "ksh", "mksh", "ash"].iter().any(|s| {
                    first.split(|ch: char| ch == '/' || ch.is_whitespace()).any(|w| w == *s)
                });
            if !shell {
                return Err(Refusal::Unsupported);
            }
            Ok(())
        }
        (Compositor::Wayfire, Some(t)) => {
            // Модуль `autostart` должен быть в `[core] plugins`, если список
            // задан (без ключа действует умолчательный — с ним).
            let mut in_core = false;
            let mut lines = t.lines().peekable();
            while let Some(l) = lines.next() {
                let s = l.trim();
                if s.starts_with('[') {
                    in_core = s == "[core]";
                    continue;
                }
                if in_core && s.starts_with("plugins") && s[7..].trim_start().starts_with('=') {
                    let mut value = s.split_once('=').map(|x| x.1).unwrap_or("").to_string();
                    while value.trim_end().ends_with('\\') {
                        match lines.next() {
                            Some(n) => {
                                value.push(' ');
                                value.push_str(n);
                            }
                            None => break,
                        }
                    }
                    if !value.split(|ch: char| ch.is_whitespace() || ch == '\\').any(|w| w == "autostart") {
                        return Err(Refusal::PluginOff);
                    }
                }
            }
            Ok(())
        }
        _ => {
            let _ = target;
            Ok(())
        }
    }
}

// ============================================================================
// Файловые помощники (общий API для чужих строк автозапуска)
// ============================================================================

/// Что случилось с файлом.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Unchanged,
    Written,
    Created,
    Removed,
    Deleted,
}

fn write_file(path: &str, text: &str) -> Result<(), Refusal> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| Refusal::Io(e.to_string()))?;
    }
    crate::utils::atomic_write(path, text).map_err(|e| Refusal::Io(e.to_string()))
}

/// Поставить/обновить управляемый блок `id` в файле `path`.
///
/// `body` — строки тела (готовые, в синтаксисе файла: см. [`launch_line`]).
/// `create_with` — `Some(преамбула)`, если файла может не быть и его можно
/// завести; `None` — отсутствие файла есть отказ [`Refusal::ConfigMissing`].
/// Повтор с тем же телом файла не трогает.
pub fn ensure_block(
    path: &str,
    syntax: Syntax,
    id: &str,
    body: &[String],
    create_with: Option<&[String]>,
) -> Result<Change, Refusal> {
    let existing = match std::fs::read_to_string(path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(Refusal::Io(e.to_string())),
    };
    if existing.is_none() && create_with.is_none() {
        return Err(Refusal::ConfigMissing);
    }
    let preamble = create_with.unwrap_or(&[]);
    match upsert_text(existing.as_deref(), syntax, id, body, preamble)? {
        TextEdit::Unchanged => Ok(Change::Unchanged),
        TextEdit::Write(t) => {
            write_file(path, &t)?;
            Ok(if existing.is_none() { Change::Created } else { Change::Written })
        }
        TextEdit::Delete => Ok(Change::Unchanged),
    }
}

/// Снять блок `id` из файла `path`; заведённый нами и опустевший файл
/// удаляется. Нет файла или блока — [`Change::Unchanged`].
pub fn remove_block(path: &str, syntax: Syntax, id: &str) -> Result<Change, Refusal> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Change::Unchanged),
        Err(e) => return Err(Refusal::Io(e.to_string())),
    };
    match remove_text(&text, syntax, id) {
        TextEdit::Unchanged => Ok(Change::Unchanged),
        TextEdit::Write(t) => {
            write_file(path, &t)?;
            Ok(Change::Removed)
        }
        TextEdit::Delete => {
            std::fs::remove_file(path).map_err(|e| Refusal::Io(e.to_string()))?;
            Ok(Change::Deleted)
        }
    }
}

// ============================================================================
// Строки автозапуска программ Rexon
// ============================================================================

/// Итог по одному композитору.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub compositor: Compositor,
    pub path: String,
    /// `Ok` — блок на месте (записан или уже был).
    pub result: Result<Change, Refusal>,
    /// Готовая строка для ручной вставки (если её удалось составить).
    pub manual_line: Option<String>,
}

/// Итог установки строк.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub outcomes: Vec<Outcome>,
}

impl SyncReport {
    /// Композиторы, в чьих конфигах блок стоит.
    pub fn covered(&self) -> Vec<Compositor> {
        self.outcomes.iter().filter(|o| o.result.is_ok()).map(|o| o.compositor).collect()
    }

    /// Имена для `NotShowIn=`.
    pub fn not_show_in(&self) -> Vec<&'static str> {
        self.covered().into_iter().flat_map(|c| c.desktop_names().iter().copied()).collect()
    }

    /// Отказ по композитору текущего сеанса — фраза для человека, или
    /// `None`, если в этом сеансе всё сработает.
    pub fn current_refusal(&self, env: &SessionEnv) -> Option<String> {
        let CurrentSession::Compositor(c) = current_session(env) else {
            return None;
        };
        let o = self.outcomes.iter().find(|o| o.compositor == c)?;
        let Err(r) = &o.result else {
            return None;
        };
        let mut msg = trf(MSG_NOT_COVERED, &[&c.display_name(), &r.text()]);
        if let Some(line) = &o.manual_line {
            msg.push(' ');
            msg.push_str(&trf(MSG_MANUAL, &[&o.path, line]));
        }
        Some(msg)
    }
}

/// Для каких композиторов писать строку: стоящие на машине плюс тот, чей
/// сеанс идёт сейчас.
fn targets_for(env: &SessionEnv) -> Vec<Compositor> {
    let mut out: Vec<Compositor> = env.installed.clone();
    if let CurrentSession::Compositor(c) = current_session(env) {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Какие конфиги композиторов правит [`sync_command_in`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Все композиторы, стоящие на машине, и тот, чей сеанс идёт (строки
    /// программ Rexon: лоток и прочие).
    Installed,
    /// Только композитор ТЕКУЩЕГО сеанса; в полном окружении и в
    /// нераспознанном сеансе — ни один. В конфигах прочих композиторов
    /// подновляется лишь строка, которая там уже стоит (от прежних
    /// включений), — новой не появляется. Так пишутся строки виджетов
    /// Conky (решение Wehrwolfmann 23.10.2026).
    Current,
    /// Ни одной новой строки: подновить только уже стоящие (виджет, который
    /// текущему сеансу не нужен — «только в другом окружении»).
    Present,
}

/// Поставить строку запуска `argv` под меткой `id` во все применимые
/// конфиги композиторов (`None` — снять отовсюду).
pub fn sync_command(env: &SessionEnv, id: &str, argv: Option<&[String]>) -> SyncReport {
    sync_command_in(env, id, argv, Scope::Installed)
}

/// [`sync_command`] с выбором, какие конфиги трогать ([`Scope`]). `None`
/// снимает строку отовсюду при любом `scope`.
pub fn sync_command_in(env: &SessionEnv, id: &str, argv: Option<&[String]>, scope: Scope) -> SyncReport {
    let mut report = SyncReport::default();
    let Some(argv) = argv else {
        remove_everywhere(env, id);
        return report;
    };
    match scope {
        Scope::Installed => {
            let wanted = targets_for(env);
            for c in Compositor::ALL {
                if !wanted.contains(&c) {
                    // Не стоит и не текущий — блока там быть не должно.
                    let target = resolve_target(c, env);
                    let _ = remove_block(&target.path, target.syntax, id);
                    continue;
                }
                report.outcomes.push(sync_one(env, c, id, argv));
            }
        }
        Scope::Current | Scope::Present => {
            let current = match (scope, current_session(env)) {
                (Scope::Current, CurrentSession::Compositor(c)) => Some(c),
                _ => None,
            };
            let present = compositors_with_block(env, id);
            for c in Compositor::ALL {
                if Some(c) == current || present.contains(&c) {
                    report.outcomes.push(sync_one(env, c, id, argv));
                }
            }
        }
    }
    report
}

/// Поставить строку `argv` под меткой `id` в конфиг ОДНОГО композитора
/// `c` (и снять её из его прочих файлов-кандидатов).
fn sync_one(env: &SessionEnv, c: Compositor, id: &str, argv: &[String]) -> Outcome {
    let target = resolve_target(c, env);
    let line = launch_line(target.syntax, id, argv);
    let manual_line = line.as_ref().ok().map(|l| l.join("\n"));
    let result = (|| {
        let body = line.clone()?;
        let text = std::fs::read_to_string(&target.path).ok();
        precheck(c, &target, text.as_deref())?;
        ensure_block(&target.path, target.syntax, id, &body, target.create_with.as_deref())
    })();
    // Строка, которая не легла в главный файл, могла остаться в другом
    // кандидате (сменился формат Hyprland) — не держать двух.
    for (p, s) in all_candidates(c, env) {
        if p != target.path {
            let _ = remove_block(&p, s, id);
        }
    }
    Outcome { compositor: c, path: target.path, result, manual_line }
}

/// Композиторы, в чьих конфигах (любом из файлов-кандидатов) стоит блок `id`.
pub fn compositors_with_block(env: &SessionEnv, id: &str) -> Vec<Compositor> {
    Compositor::ALL
        .into_iter()
        .filter(|&c| {
            all_candidates(c, env)
                .iter()
                .any(|(p, s)| std::fs::read_to_string(p).is_ok_and(|t| has_block(&t, *s, id)))
        })
        .collect()
}

/// Снять блок `id` из всех файлов-кандидатов композитора `c`.
pub fn remove_from(env: &SessionEnv, c: Compositor, id: &str) {
    for (p, s) in all_candidates(c, env) {
        let _ = remove_block(&p, s, id);
    }
}

/// Снять блок `id` из всех возможных конфигов всех шести композиторов.
pub fn remove_everywhere(env: &SessionEnv, id: &str) -> Vec<(String, Result<Change, Refusal>)> {
    let mut out = Vec::new();
    for c in Compositor::ALL {
        for (p, s) in all_candidates(c, env) {
            let r = remove_block(&p, s, id);
            if r != Ok(Change::Unchanged) {
                out.push((p, r));
            }
        }
    }
    out
}

/// Сработает ли автозапуск `id` в ТЕКУЩЕМ сеансе, если `.desktop`-запись
/// (или юнит) на месте. Полные окружения и нераспознанные сеансы — да;
/// у композитора — если в его главном конфиге стоит наш блок, либо
/// `session_runs_it()` говорит, что этот сеанс всё же исполнит запись сам
/// (для `.desktop` — поднят `xdg-desktop-autostart.target`: uwsm,
/// `niri-session`; для юнита — `graphical-session.target`), и запись от
/// него не скрыта `NotShowIn=`. `desktop_text` — `None` у юнита.
pub fn works_in_current_session(
    env: &SessionEnv,
    id: &str,
    desktop_text: Option<&str>,
    session_runs_it: impl FnOnce() -> bool,
) -> bool {
    let CurrentSession::Compositor(c) = current_session(env) else {
        return true;
    };
    let target = resolve_target(c, env);
    if let Ok(text) = std::fs::read_to_string(&target.path) {
        if has_block(&text, target.syntax, id) {
            return true;
        }
    }
    let hidden = desktop_text
        .map(|t| not_show_in_of(t).iter().any(|n| c.desktop_names().contains(&n.as_str())))
        .unwrap_or(false);
    !hidden && session_runs_it()
}

/// Живая проверка: поднят ли в этом сеансе `xdg-desktop-autostart.target`.
pub fn xdg_autostart_target_active() -> bool {
    user_unit_active("xdg-desktop-autostart.target")
}

/// Живая проверка: поднят ли `graphical-session.target` (его ждут юниты
/// с `WantedBy=graphical-session.target`).
pub fn graphical_session_target_active() -> bool {
    user_unit_active("graphical-session.target")
}

fn user_unit_active(unit: &str) -> bool {
    std::process::Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", unit])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// ============================================================================
// .desktop: NotShowIn и разбор Exec
// ============================================================================

/// Значения `NotShowIn=` записи.
fn not_show_in_of(desktop: &str) -> Vec<String> {
    desktop
        .lines()
        .find_map(|l| l.strip_prefix("NotShowIn="))
        .map(|v| v.split(';').filter(|s| !s.is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Запись `desktop` с ключом `NotShowIn=<names>` (пустой список — без
/// ключа). Прежний ключ заменяется; новый встаёт сразу после `Type=`.
pub fn with_not_show_in(desktop: &str, names: &[&str]) -> String {
    let mut out = String::with_capacity(desktop.len() + 64);
    let mut placed = names.is_empty();
    for line in desktop.split_inclusive('\n') {
        if line.starts_with("NotShowIn=") {
            continue;
        }
        out.push_str(line);
        if !placed && line.starts_with("Type=") {
            if !line.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!("NotShowIn={};\n", names.join(";")));
            placed = true;
        }
    }
    if !placed {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!("NotShowIn={};\n", names.join(";")));
    }
    out
}

/// argv из значения `Exec=` по спецификации Desktop Entry: сперва
/// разэкранирование значения ключа (`\s \n \t \r \\`), затем разбор
/// кавычек (`\"`, `` \` ``, `\$`, `\\` внутри) и `%%`; коды полей (`%f` …)
/// отбрасываются. `None` — строка разбита (незакрытая кавычка).
pub fn exec_argv(exec_value: &str) -> Option<Vec<String>> {
    let mut value = String::with_capacity(exec_value.len());
    let mut chars = exec_value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('s') => value.push(' '),
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                Some('r') => value.push('\r'),
                Some('\\') => value.push('\\'),
                Some(o) => {
                    value.push('\\');
                    value.push(o);
                }
                None => value.push('\\'),
            }
        } else {
            value.push(c);
        }
    }
    let mut argv = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut it = value.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            ' ' | '\t' => {
                if in_word {
                    argv.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match it.next()? {
                        '"' => break,
                        '\\' => match it.next()? {
                            e @ ('"' | '`' | '$' | '\\') => cur.push(e),
                            o => {
                                cur.push('\\');
                                cur.push(o);
                            }
                        },
                        o => cur.push(o),
                    }
                }
            }
            '%' => match it.next() {
                Some('%') => {
                    in_word = true;
                    cur.push('%');
                }
                Some(_) | None => {} // код поля — не аргумент
            },
            o => {
                in_word = true;
                cur.push(o);
            }
        }
    }
    if in_word {
        argv.push(cur);
    }
    Some(argv)
}

/// argv из `Exec=` записи `[Desktop Entry]`.
pub fn desktop_argv(desktop: &str) -> Option<Vec<String>> {
    let mut in_main = false;
    for line in desktop.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_main = t == "[Desktop Entry]";
            continue;
        }
        if in_main {
            if let Some(v) = t.strip_prefix("Exec=") {
                return exec_argv(v).filter(|a| !a.is_empty());
            }
        }
    }
    None
}

/// Итог установки XDG-записи вместе со строками композиторов.
#[derive(Debug, Clone)]
pub struct DesktopMirror {
    /// Окончательный текст `.desktop` (с `NotShowIn=` для покрытых).
    pub desktop: String,
    pub report: SyncReport,
}

/// Строки композиторов для XDG-записи `desktop` под меткой `id`. Вызвать
/// ПЕРЕД записью `.desktop` и писать на диск уже [`DesktopMirror::desktop`].
pub fn mirror_desktop(env: &SessionEnv, id: &str, desktop: &str) -> DesktopMirror {
    let Some(argv) = desktop_argv(desktop) else {
        return DesktopMirror { desktop: desktop.to_string(), report: SyncReport::default() };
    };
    let report = sync_command(env, id, Some(&argv));
    let names = report.not_show_in();
    DesktopMirror { desktop: with_not_show_in(desktop, &names), report }
}

// ============================================================================
// Метки программ Rexon и живая дверь для писателей автозапуска
// ============================================================================

/// Лоток Rexon (`~/.config/autostart/rexon.desktop`).
pub const ID_TRAY: &str = "tray";
/// Заставка входа (`rexon-session-splash.desktop`).
pub const ID_SPLASH: &str = "session-splash";
/// Применение приложений по умолчанию (`rexon-appdefaults.desktop`).
pub const ID_APPDEFAULTS: &str = "appdefaults";
/// Движок обоев (юнит systemd или `rexon-wallpaper-engine.desktop`).
pub const ID_ENGINE: &str = "wallpaper-engine";

/// Все метки блоков Rexon — для снятия при удалении Rexon: строки запуска
/// программ и переменные сеанса ([`ID_SESSION_ENV`]).
pub const REXON_IDS: &[&str] = &[ID_TRAY, ID_SPLASH, ID_APPDEFAULTS, ID_ENGINE, ID_SESSION_ENV];

/// [`mirror_desktop`] на живой машине. Возвращает окончательный текст
/// записи и отказ для текущего сеанса (`None` — здесь сработает).
pub fn mirror_desktop_live(id: &str, desktop: &str) -> (String, Option<String>) {
    let env = SessionEnv::live();
    let m = mirror_desktop(&env, id, desktop);
    let refusal = m.report.current_refusal(&env);
    (m.desktop, refusal)
}

/// Снять строки `id` из конфигов всех композиторов на живой машине.
pub fn remove_live(id: &str) {
    remove_everywhere(&SessionEnv::live(), id);
}

/// [`works_in_current_session`] на живой машине.
pub fn works_live(id: &str, desktop_text: Option<&str>) -> bool {
    works_in_current_session(&SessionEnv::live(), id, desktop_text, xdg_autostart_target_active)
}

/// Освежить строки композиторов по уже лежащим XDG-записям программ Rexon.
///
/// Строки пишутся при включении переключателя — для композиторов, которые
/// стояли в ту минуту. Поставленный позже композитор (или сменившийся путь
/// к Rexon) иначе остался бы без строки до следующего щелчка. Зовётся при
/// старте лотка: записи без изменений не трогаются ни на байт. Юнит движка
/// здесь не смотрится (для этого нужен `systemctl`) — его строки ставит
/// переключатель движка.
pub fn resync_rexon_live() {
    let env = SessionEnv::live();
    for (id, path) in rexon_autostart_entries(&env) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let m = mirror_desktop(&env, id, &text);
        if m.desktop != text {
            let _ = crate::utils::atomic_write(&path, &m.desktop);
        }
    }
}

/// XDG-записи автозапуска программ Rexon: метка и путь. ОДИН список на
/// освежение строк композиторов и на починку устаревших путей.
pub fn rexon_autostart_entries(env: &SessionEnv) -> Vec<(&'static str, String)> {
    let cfg = &env.config_home;
    vec![
        (ID_TRAY, crate::tray::autostart_path(Some(cfg), &env.home)),
        (ID_SPLASH, crate::session_splash::autostart_path(Some(cfg), &env.home)),
        (ID_APPDEFAULTS, format!("{cfg}/autostart/rexon-appdefaults.desktop")),
        (ID_ENGINE, crate::we_client::autostart_desktop_path(Some(cfg), &env.home)),
    ]
}

/// Починка записи, чья программа исчезла. `Some((старая программа, новый
/// текст))` — запись устарела и есть чем заменить; `None` — чинить нечего.
///
/// Живой случай 08.10.2026: `rexon.desktop` и `rexon-appdefaults.desktop`
/// у владельца звали `/usr/local/bin/rexon` — путь давно снятой установки
/// разработчика. systemd-xdg-autostart-generator такие записи молча
/// пропускает («executable specified in Exec= does not exist»), и Rexon
/// при входе не стартовал, а галочка автозапуска в окне горела.
///
/// Меняется ТОЛЬКО программа (`argv[0]`); флаги записи (`--tray`,
/// `--apply-defaults`…) и прочие строки — как были.
pub fn repaired_exec(
    desktop: &str,
    new_program: &str,
    program_exists: &dyn Fn(&str) -> bool,
) -> Option<(String, String)> {
    let argv = desktop_argv(desktop)?;
    let old = argv.first()?.clone();
    if program_exists(&old) {
        return None;
    }
    let new_argv = desktop_argv(&format!("[Desktop Entry]\nExec={new_program}\n"))?;
    if new_argv.first() == Some(&old) || !program_exists(new_argv.first()?) {
        return None;
    }
    let mut exec = new_program.to_string();
    for a in &argv[1..] {
        exec.push(' ');
        exec.push_str(&crate::installer::desktop_exec_arg(a));
    }
    let mut in_main = false;
    let mut done = false;
    let mut out = String::with_capacity(desktop.len() + exec.len());
    for line in desktop.split_inclusive('\n') {
        let t = line.trim();
        if t.starts_with('[') {
            in_main = t == "[Desktop Entry]";
        }
        if in_main && !done && t.starts_with("Exec=") {
            out.push_str(&format!("Exec={exec}\n"));
            done = true;
            continue;
        }
        if in_main && t.starts_with("TryExec=") {
            // TryExec со старым путём спрятал бы запись так же, как Exec.
            out.push_str(&format!("TryExec={}\n", new_argv[0]));
            continue;
        }
        out.push_str(line);
    }
    Some((old, out))
}

/// Есть ли программа: абсолютный путь — исполняемый файл, имя — в `PATH`.
pub fn program_exists_live(program: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if program.contains('/') {
        return std::fs::metadata(program)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    }
    crate::utils::which(program, std::env::var("PATH").ok().as_deref()).is_some()
}

/// Починить устаревшие записи автозапуска Rexon на живой машине.
/// `program_for(метка)` — чем запускать сейчас (в формате `Exec=`); `None`
/// — заменить нечем, запись остаётся и об этом пишется в журнал.
pub fn repair_stale_rexon_live(program_for: &dyn Fn(&str) -> Option<String>) {
    let env = SessionEnv::live();
    for (id, path) in rexon_autostart_entries(&env) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(argv) = desktop_argv(&text) else {
            continue;
        };
        if program_exists_live(&argv[0]) {
            continue;
        }
        let Some(program) = program_for(id) else {
            crate::log_warn!(
                "autostart",
                "{path}: программы {} нет, а заменить нечем — запись оставлена как есть",
                argv[0]
            );
            continue;
        };
        match repaired_exec(&text, &program, &program_exists_live) {
            Some((old, fixed)) => match crate::utils::atomic_write(&path, &fixed) {
                Ok(()) => crate::log_info!(
                    "autostart",
                    "{path}: программы {old} нет — запись исправлена на {program}"
                ),
                Err(e) => crate::log_warn!("autostart", "{path}: исправить не удалось: {e}"),
            },
            None => crate::log_warn!(
                "autostart",
                "{path}: программы {} нет, замена {program} тоже не годится — запись оставлена",
                argv[0]
            ),
        }
    }
}

/// Снять ВСЕ блоки Rexon (удаление Rexon): строки запуска из конфигов
/// композиторов и переменные сеанса — ещё и из `~/.xprofile` и
/// `labwc/environment`, которых в списке конфигов композиторов нет.
pub fn remove_all_rexon(env: &SessionEnv) -> Vec<(String, Result<Change, Refusal>)> {
    let mut out = Vec::new();
    for id in REXON_IDS {
        out.extend(remove_everywhere(env, id));
    }
    for (p, syntax) in session_env_candidates(env) {
        let r = remove_block(&p, syntax, ID_SESSION_ENV);
        if r != Ok(Change::Unchanged) && !out.iter().any(|(q, _)| *q == p) {
            out.push((p, r));
        }
    }
    out
}

/// [`remove_all_rexon`] на живой машине.
pub fn remove_all_rexon_live() {
    remove_all_rexon(&SessionEnv::live());
}

// ============================================================================
// Переменные окружения сеанса там, где ~/.config/environment.d не читают
// ============================================================================
//
// Переменные Rexon (стиль Qt — `QT_QPA_PLATFORMTHEME`/`QT_STYLE_OVERRIDE`,
// курсор — `XCURSOR_*`, терминал — `TERMINAL`) живут в
// `~/.config/environment.d/rexon-*.conf`. Этот каталог читает только сеанс,
// который берёт окружение у systemd: Plasma, GNOME, COSMIC, niri через
// `niri-session`, Hyprland под uwsm. Остальные его не видят (проверка
// окружений 02.10.2026, Д1; сверено по скриптам запуска на машине):
//
// | сеанс                              | что он читает                          | что делает Rexon                     |
// |------------------------------------|----------------------------------------|--------------------------------------|
// | Cinnamon, MATE, XFCE, Budgie на X11| `~/.xprofile` (Xsession SDDM, LightDM, GDM) | блок в `~/.xprofile` только для этих окружений (`case` по `XDG_CURRENT_DESKTOP`) |
// | XFCE на Wayland (`startxfce4 --wayland`) | `~/.config/xfce4/labwc/environment` | блок `KEY=value` в этот файл         |
// | Budgie на labwc (`startbudgielabwc`)| `~/.config/budgie-desktop/labwc/environment` | то же                          |
// | labwc сам по себе                  | `~/.config/labwc/environment`          | то же                                |
// | Hyprland без uwsm                  | `env =` в своём конфиге                | блок `env =` / `hl.env(…)` в главный конфиг (курсор — уже в своём блоке) |
// | sway (`Exec=sway`), river, Wayfire | ничего: в их конфигах переменных нет   | НЕ берёт — честная оговорка в матрице (`de_customization::condition_note_id`) |
//
// Источник правды один — файлы `environment.d`: эти блоки их зеркалят и
// пересобираются после каждой записи туда ([`sync_session_env`]).
//
// ⚠️ Файлы окружения labwc у XFCE и Budgie их скрипты запуска копируют из
// системного образца, только если файла ещё нет. Заведи Rexon файл из одного
// своего блока — образец (раскладка клавиатуры) больше не приехал бы, поэтому
// отсутствующий файл сперва засевается образцом.

/// Метка блока переменных сеанса.
pub const ID_SESSION_ENV: &str = "session-env";

/// Окружения X11, которые `environment.d` не читают (`XDG_CURRENT_DESKTOP`).
const X11_ENV_DESKTOPS: &str = "*X-Cinnamon*|*MATE*|*XFCE*|*Budgie*";
/// Их сеансовые бинари: есть хоть один — `~/.xprofile` нужен.
const X11_ENV_SESSION_BINS: &[&str] = &["cinnamon-session", "mate-session", "xfce4-session", "budgie-desktop"];

/// Переменные из `~/.config/environment.d/rexon-*.conf` (файлы — по имени,
/// внутри — по порядку; повтор ключа — последний побеждает).
pub fn rexon_environment_vars(config_home: &str) -> Vec<(String, String)> {
    let dir = join(config_home, "environment.d");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("rexon-") && n.ends_with(".conf"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut out: Vec<(String, String)> = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(join(&dir, &f)) else { continue };
        for (k, v) in parse_environment_d(&text) {
            out.retain(|(ek, _)| *ek != k);
            out.push((k, v));
        }
    }
    out
}

/// Разбор файла `environment.d`: строки `KEY=value`, комментарии `#`.
/// Ключ — только `[A-Za-z0-9_]`, значение без переводов строки.
pub fn parse_environment_d(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            let k = k.trim();
            if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            Some((k.to_string(), v.trim().trim_matches('"').to_string()))
        })
        .collect()
}

/// Куда и что положить блоком переменных.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvTarget {
    pub path: String,
    pub syntax: Syntax,
    pub body: Vec<String>,
    /// Файла нет — сперва скопировать этот образец (labwc у XFCE и Budgie).
    pub seed: Option<String>,
    /// Файла нет (и образца нет) — можно ли заводить.
    pub create: bool,
}

fn lua_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Тело блока для Hyprland: `XCURSOR_*`/`HYPRCURSOR_*` уже ставит свой блок
/// курсора — второй раз их не пишем.
fn hypr_env_body(syntax: Syntax, vars: &[(String, String)]) -> Vec<String> {
    vars.iter()
        .filter(|(k, _)| !k.starts_with("XCURSOR_") && !k.starts_with("HYPRCURSOR_"))
        .map(|(k, v)| match syntax {
            Syntax::HyprLua => format!("hl.env({}, {})", lua_string(k), lua_string(v)),
            _ => format!("env = {k},{v}"),
        })
        .collect()
}

/// Где этой машине нужны переменные Rexon помимо `environment.d`.
/// `has_bin` — есть ли программа в `PATH`; `is_file` — есть ли файл.
pub fn session_env_targets(
    env: &SessionEnv,
    vars: &[(String, String)],
    has_bin: &dyn Fn(&str) -> bool,
    is_file: &dyn Fn(&str) -> bool,
) -> Vec<EnvTarget> {
    let mut out = Vec::new();
    if vars.is_empty() {
        return out;
    }
    // X11-сеансы через экран входа: `~/.xprofile`, только для своих окружений
    // (Plasma и GNOME на X11 его тоже читают — им переменные не нужны).
    if X11_ENV_SESSION_BINS.iter().any(|b| has_bin(b)) {
        let mut body = vec![format!("case \"$XDG_CURRENT_DESKTOP\" in"), format!("    {X11_ENV_DESKTOPS})")];
        body.extend(vars.iter().map(|(k, v)| format!("        export {k}={}", shell_quote(v))));
        body.push("        ;;".into());
        body.push("esac".into());
        out.push(EnvTarget {
            path: join(&env.home, ".xprofile"),
            syntax: Syntax::Shell,
            body,
            seed: None,
            create: true,
        });
    }
    // labwc-сеансы XFCE и Budgie: свой файл окружения у каждого.
    for (rel, sample) in [
        ("xfce4/labwc/environment", "/usr/share/xfce4/labwc/labwc-environment"),
        ("budgie-desktop/labwc/environment", "/usr/share/budgie-desktop/labwc/environment"),
    ] {
        if is_file(sample) {
            out.push(EnvTarget {
                path: join(&env.config_home, rel),
                syntax: Syntax::Shell,
                body: vars.iter().map(|(k, v)| format!("{k}={v}")).collect(),
                seed: Some(sample.to_string()),
                create: true,
            });
        }
    }
    // labwc сам по себе: `~/.config/labwc/environment` (у XFCE и Budgie
    // свой каталог настроек labwc — выше).
    if env.installed.contains(&Compositor::Labwc) {
        out.push(EnvTarget {
            path: join(&env.config_home, "labwc/environment"),
            syntax: Syntax::Shell,
            body: vars.iter().map(|(k, v)| format!("{k}={v}")).collect(),
            seed: None,
            create: true,
        });
    }
    // Hyprland без uwsm: `env =` в главном конфиге (под uwsm — повтор того,
    // что он и так взял из environment.d, вреда нет).
    if env.installed.contains(&Compositor::Hyprland) {
        let t = resolve_target(Compositor::Hyprland, env);
        let body = hypr_env_body(t.syntax, vars);
        if !body.is_empty() {
            out.push(EnvTarget { path: t.path, syntax: t.syntax, body, seed: None, create: false });
        }
    }
    out
}

/// Все файлы, где может лежать блок переменных (для снятия).
fn session_env_candidates(env: &SessionEnv) -> Vec<(String, Syntax)> {
    let mut out = vec![
        (join(&env.home, ".xprofile"), Syntax::Shell),
        (join(&env.config_home, "xfce4/labwc/environment"), Syntax::Shell),
        (join(&env.config_home, "budgie-desktop/labwc/environment"), Syntax::Shell),
        (join(&env.config_home, "labwc/environment"), Syntax::Shell),
    ];
    out.extend(all_candidates(Compositor::Hyprland, env));
    out
}

/// Поставить блок `id` с телом `body`, заменив прежний ЦЕЛИКОМ (обычная
/// правка блока оставляет строки, не похожие на строку запуска, — для
/// переменных это были бы повторы).
fn replace_block(path: &str, syntax: Syntax, id: &str, body: &[String], create: bool) -> Result<Change, Refusal> {
    let existing = match std::fs::read_to_string(path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(Refusal::Io(e.to_string())),
    };
    let base = match existing.as_deref() {
        Some(t) => match remove_text(t, syntax, id) {
            TextEdit::Unchanged => Some(t.to_string()),
            TextEdit::Write(x) => Some(x),
            TextEdit::Delete => None,
        },
        None => None,
    };
    if base.is_none() && !create {
        return Err(Refusal::ConfigMissing);
    }
    match upsert_text(base.as_deref(), syntax, id, body, &[])? {
        TextEdit::Write(t) if existing.as_deref() != Some(t.as_str()) => {
            write_file(path, &t)?;
            Ok(if existing.is_none() { Change::Created } else { Change::Written })
        }
        _ => Ok(Change::Unchanged),
    }
}

/// Зеркалить переменные Rexon из `environment.d` туда, где их читают сеансы
/// без systemd (см. таблицу выше). Нет переменных — блоки снимаются.
/// Итог по каждому файлу, где что-то делалось.
pub fn sync_session_env(env: &SessionEnv, has_bin: &dyn Fn(&str) -> bool) -> Vec<(String, Result<Change, Refusal>)> {
    let vars = rexon_environment_vars(&env.config_home);
    let targets = session_env_targets(env, &vars, has_bin, &|p| is_file(p));
    let mut out = Vec::new();
    for t in &targets {
        if !is_file(&t.path) {
            if let Some(sample) = &t.seed {
                if let Ok(text) = std::fs::read_to_string(sample) {
                    if let Err(e) = write_file(&t.path, &text) {
                        out.push((t.path.clone(), Err(e)));
                        continue;
                    }
                }
            }
        }
        out.push((t.path.clone(), replace_block(&t.path, t.syntax, ID_SESSION_ENV, &t.body, t.create)));
    }
    for (p, syntax) in session_env_candidates(env) {
        if targets.iter().any(|t| t.path == p) {
            continue;
        }
        let r = remove_block(&p, syntax, ID_SESSION_ENV);
        if r != Ok(Change::Unchanged) {
            out.push((p, r));
        }
    }
    out
}

/// [`sync_session_env`] на живой машине.
pub fn sync_session_env_live() -> Vec<(String, Result<Change, Refusal>)> {
    let path = std::env::var("PATH").ok();
    sync_session_env(&SessionEnv::live(), &|b| crate::utils::which(b, path.as_deref()).is_some())
}

/// Пути, которые может тронуть [`sync_session_env_live`] (снимаются до
/// записи — под откат «Применить»): цели и все кандидаты, где блок
/// переменных снимается.
pub fn session_env_paths_live() -> Vec<String> {
    let env = SessionEnv::live();
    let path = std::env::var("PATH").ok();
    let vars = rexon_environment_vars(&env.config_home);
    let has = |b: &str| crate::utils::which(b, path.as_deref()).is_some();
    let mut out: Vec<String> = session_env_targets(&env, &vars, &has, &|p| is_file(p)).into_iter().map(|t| t.path).collect();
    for (p, _) in session_env_candidates(&env) {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Касается ли запись по этому пути переменных сеанса (`environment.d`).
pub fn is_environment_d_path(path: &str) -> bool {
    path.contains("/environment.d/")
}

#[cfg(test)]
mod tests {
    #[test]
    fn устаревший_путь_в_записи_автозапуска_меняется_на_нынешний() {
        let old = "[Desktop Entry]\nType=Application\nNotShowIn=sway;labwc;\nName=Rexon\n\
                   Exec=/usr/local/bin/rexon --tray\nIcon=rexon\n";
        let have = |p: &str| p == "/usr/lib/rexon/rexon-main-window";
        let (was, fixed) =
            super::repaired_exec(old, "/usr/lib/rexon/rexon-main-window", &have).expect("чинится");
        assert_eq!(was, "/usr/local/bin/rexon");
        assert!(fixed.contains("\nExec=/usr/lib/rexon/rexon-main-window --tray\n"), "{fixed}");
        assert!(fixed.contains("NotShowIn=sway;labwc;"), "прочие строки не трогаются");
        // Рабочая запись не трогается.
        assert!(super::repaired_exec(&fixed, "/x/other", &|_| true).is_none());
        // Замена, которой тоже нет, не пишется.
        assert!(super::repaired_exec(old, "/nope/rexon", &|_| false).is_none());
        // Путь с пробелом — в кавычках, флаги сохраняются.
        let ad = "[Desktop Entry]\nExec=/usr/local/bin/rexon --apply-defaults\nTryExec=/usr/local/bin/rexon\n";
        let (_, f) = super::repaired_exec(ad, "\"/opt/My Rexon/rexon-main-window\"", &|p| p.starts_with("/opt/")).unwrap();
        assert!(f.contains("Exec=\"/opt/My Rexon/rexon-main-window\" --apply-defaults\n"), "{f}");
        assert!(f.contains("TryExec=/opt/My Rexon/rexon-main-window\n"), "{f}");
    }

    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn exec_roundtrip_with_desktop_quoting() {
        let path = "/home/u/My Apps/rexon \"x\" $y";
        let exec = format!("{} --tray", crate::installer::desktop_exec_arg(path));
        // В файле ключ ещё раз экранирован? desktop_exec_arg уже отдаёт
        // значение ключа — разбор обязан вернуть путь как был.
        assert_eq!(exec_argv(&exec), Some(s(&[path, "--tray"])));
        assert_eq!(exec_argv("rexon %U --x 100%%"), Some(s(&["rexon", "--x", "100%"])));
        assert_eq!(exec_argv("\"unterminated"), None);
    }

    #[test]
    fn shell_quoting() {
        assert_eq!(shell_quote("/usr/bin/rexon"), "/usr/bin/rexon");
        assert_eq!(shell_quote("/home/а/Загрузки/x"), "/home/а/Загрузки/x");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn not_show_in_placed_and_replaced() {
        let d = "[Desktop Entry]\nType=Application\nName=X\nExec=x\n";
        let one = with_not_show_in(d, &["sway", "niri"]);
        assert_eq!(one, "[Desktop Entry]\nType=Application\nNotShowIn=sway;niri;\nName=X\nExec=x\n");
        assert_eq!(with_not_show_in(&one, &[]), d);
        assert_eq!(with_not_show_in(&one, &["niri"]), with_not_show_in(d, &["niri"]));
    }

    #[test]
    fn hyprlua_return_refused() {
        let r = upsert_text(Some("hl.config({})\nreturn cfg\n"), Syntax::HyprLua, "t", &s(&["x"]), &[]);
        assert_eq!(r, Err(Refusal::Unsupported));
        // `returns_x = 1` — не `return`.
        assert!(upsert_text(Some("returns_x = 1\n"), Syntax::HyprLua, "t", &s(&["x"]), &[]).is_ok());
    }

    /// Д1 (проверка окружений 02.10.2026): переменные Rexon доходят до
    /// сеансов, которые `environment.d` не читают, — и только до них.
    #[test]
    fn session_env_reaches_sessions_without_systemd() {
        let dir = std::env::temp_dir().join(format!("rexon-senv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.to_string_lossy().into_owned();
        let cfg = format!("{home}/.config");
        std::fs::create_dir_all(format!("{cfg}/environment.d")).unwrap();
        std::fs::create_dir_all(format!("{cfg}/hypr")).unwrap();
        std::fs::write(format!("{cfg}/environment.d/rexon-qt-style.conf"), "QT_QPA_PLATFORMTHEME=qt6ct\n").unwrap();
        std::fs::write(format!("{cfg}/environment.d/rexon-cursor.conf"), "XCURSOR_THEME=Bibata\nXCURSOR_SIZE=32\n").unwrap();
        std::fs::write(format!("{cfg}/environment.d/other.conf"), "FOO=bar\n").unwrap();
        std::fs::write(format!("{cfg}/hypr/hyprland.conf"), "monitor=,preferred,auto,1\n").unwrap();
        let sample = format!("{home}/sample-env");
        std::fs::write(&sample, "XKB_DEFAULT_LAYOUT=us\n").unwrap();
        let env = SessionEnv {
            home: home.clone(),
            config_home: cfg.clone(),
            sysconf: "/nonexistent".into(),
            installed: vec![Compositor::Hyprland],
            ..Default::default()
        };
        assert_eq!(
            rexon_environment_vars(&cfg),
            [
                ("XCURSOR_THEME".to_string(), "Bibata".to_string()),
                ("XCURSOR_SIZE".to_string(), "32".to_string()),
                ("QT_QPA_PLATFORMTHEME".to_string(), "qt6ct".to_string()),
            ]
        );
        let vars = rexon_environment_vars(&cfg);
        let only_xfce = |b: &str| b == "xfce4-session";
        let targets = session_env_targets(&env, &vars, &only_xfce, &|p| p == "/usr/share/xfce4/labwc/labwc-environment");
        let paths: Vec<&str> = targets.iter().map(|t| t.path.as_str()).collect();
        assert_eq!(
            paths,
            [format!("{home}/.xprofile").as_str(), format!("{cfg}/xfce4/labwc/environment").as_str(), format!("{cfg}/hypr/hyprland.conf").as_str()]
        );
        assert_eq!(
            targets[0].body,
            s(&[
                "case \"$XDG_CURRENT_DESKTOP\" in",
                "    *X-Cinnamon*|*MATE*|*XFCE*|*Budgie*)",
                "        export XCURSOR_THEME=Bibata",
                "        export XCURSOR_SIZE=32",
                "        export QT_QPA_PLATFORMTHEME=qt6ct",
                "        ;;",
                "esac",
            ])
        );
        assert_eq!(targets[1].body, s(&["XCURSOR_THEME=Bibata", "XCURSOR_SIZE=32", "QT_QPA_PLATFORMTHEME=qt6ct"]));
        // Курсор у Hyprland — в своём блоке; здесь только остальное.
        assert_eq!(targets[2].body, s(&["env = QT_QPA_PLATFORMTHEME,qt6ct"]));
        // Ни одного окружения без environment.d — некуда и незачем.
        assert!(session_env_targets(&SessionEnv { installed: vec![], ..env.clone() }, &vars, &|_| false, &|_| false).is_empty());

        // Запись на диск: файл .xprofile заводится, конфиг Hyprland дописывается.
        let r = sync_session_env(&env, &|b| b == "mate-session");
        assert!(r.iter().all(|(_, x)| x.is_ok()), "{r:?}");
        let xp = std::fs::read_to_string(format!("{home}/.xprofile")).unwrap();
        assert!(xp.contains("export QT_QPA_PLATFORMTHEME=qt6ct\n"), "{xp}");
        let hy = std::fs::read_to_string(format!("{cfg}/hypr/hyprland.conf")).unwrap();
        assert!(hy.starts_with("monitor=,preferred,auto,1\n") && hy.contains("\nenv = QT_QPA_PLATFORMTHEME,qt6ct\n"), "{hy}");
        // Повтор — ни байта; смена значения — без повторов строк.
        let again = sync_session_env(&env, &|b| b == "mate-session");
        assert!(again.iter().all(|(_, x)| *x == Ok(Change::Unchanged)), "{again:?}");
        std::fs::write(format!("{cfg}/environment.d/rexon-qt-style.conf"), "QT_STYLE_OVERRIDE=kvantum\n").unwrap();
        sync_session_env(&env, &|b| b == "mate-session");
        let hy = std::fs::read_to_string(format!("{cfg}/hypr/hyprland.conf")).unwrap();
        assert!(!hy.contains("QT_QPA_PLATFORMTHEME") && hy.matches("QT_STYLE_OVERRIDE").count() == 1, "{hy}");
        // Переменных не осталось — блоки сняты, заведённый .xprofile удалён,
        // конфиг Hyprland вернулся байт-в-байт.
        for f in ["rexon-qt-style.conf", "rexon-cursor.conf"] {
            std::fs::remove_file(format!("{cfg}/environment.d/{f}")).unwrap();
        }
        sync_session_env(&env, &|b| b == "mate-session");
        assert!(!std::path::Path::new(&format!("{home}/.xprofile")).exists());
        assert_eq!(std::fs::read_to_string(format!("{cfg}/hypr/hyprland.conf")).unwrap(), "monitor=,preferred,auto,1\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Hyprland на Lua: `hl.env("K", "v")`.
    #[test]
    fn session_env_hyprland_lua_syntax() {
        let vars = vec![("QT_QPA_PLATFORMTHEME".to_string(), "qt6ct".to_string())];
        assert_eq!(hypr_env_body(Syntax::HyprLua, &vars), s(&["hl.env(\"QT_QPA_PLATFORMTHEME\", \"qt6ct\")"]));
    }
}
