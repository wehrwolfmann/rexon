//! Опасные директивы и здоровье виджета Conky (план паритета с Conky
//! Manager, п.42 и п.40).
//!
//! Виджет Conky — не картинка: файл настроек может выполнять команды
//! (`${exec…}`, `${execi…}`, `${texeci…}`), грузить сценарии Lua
//! (`lua_load`, а в них `os.execute`, `io.popen`) и ходить в интернет
//! (`${curl}`, `${rss}`, `${weather}`). Скачанная из магазина тема делает это
//! с правами человека. Поэтому перед ПЕРВЫМ запуском такого виджета Rexon
//! показывает, что именно он выполняет, и запускает только с разрешения.
//!
//! Разрешение помнится по отпечатку (SHA-256) файлов-кода виджета: файлов
//! настроек, сценариев Lua и оболочки, файлов, на которые ссылаются его
//! команды. Правка любого из них — вопрос снова, с пояснением «файлы
//! изменились». Данные, которые виджет пишет к себе в каталог (кэш погоды,
//! картинки), отпечаток не трогают — иначе вопрос шёл бы при каждом запуске.
//!
//! Разрешения лежат в `~/.config/rexon/conky-trust.json`.
//!
//! Тот же разбор отвечает и на «чего виджету не хватает» ([`health`]):
//! сценариев, на которые он ссылается, и программ из его команд — с пакетом
//! для семейства дистрибутива, чтобы поставить его до запуска (без вопроса:
//! защита — окно пароля polkit; не встало — виджет не запускается, см.
//! [`start_plan`]).
//!
//! Разбор — без запуска Lua: комментарии вырезаются, строки остаются как
//! есть, директивы ищутся в оставшемся тексте. Цель — честно показать
//! человеку команды, а не доказать безопасность: команда, собранная
//! сценарием из строк (`local cmd = 'cava -p ' .. CONF` → `io.popen(cmd)`),
//! видна текстом, где неизвестные при разборе части — «…»; совсем без строк
//! — вызовом `os.execute(…)` с выражением.

use sha2::{Digest, Sha256};

use crate::animations::LookIo;
use crate::de_apply::Ctx;
use crate::i18n::{ntr, tr, trf};

/// Что делает виджет помимо рисования.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskKind {
    /// Выполняет команду оболочки.
    Command,
    /// Грузит сценарий Lua.
    LuaScript,
    /// Скачивает из интернета.
    Network,
}

/// Одна опасная директива.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Risk {
    pub kind: RiskKind,
    /// Команда, сценарий или адрес — как написано в файле.
    pub what: String,
    /// Файл, где директива стоит: относительно каталога виджета (вне его —
    /// полный путь).
    pub file: String,
}

/// Директивы с командой: `(имя, первый аргумент — интервал)`.
const COMMAND_DIRECTIVES: &[(&str, bool)] = &[
    ("exec", false),
    ("execp", false),
    ("execbar", false),
    ("execgauge", false),
    ("execgraph", false),
    ("execi", true),
    ("execpi", true),
    ("execibar", true),
    ("execigauge", true),
    ("execigraph", true),
    ("texeci", true),
    ("texecpi", true),
];

/// Директивы, которые скачивают: первый аргумент — адрес.
const NETWORK_DIRECTIVES: &[&str] = &["curl", "rss", "weather", "weather_forecast"];

/// Расширения файлов-кода: их правка меняет поведение виджета.
const CODE_EXTS: &[&str] = &["lua", "sh", "bash", "zsh", "py", "pl", "rb", "js", "php", "tcl", "awk", "sed"];

/// Глубина обхода каталога виджета.
const MAX_DEPTH: usize = 6;

/// Больше файлов-кода не смотрим (виджет-архив на тысячи файлов — уже не
/// виджет; отпечаток всё равно меняется при правке первых).
const MAX_FILES: usize = 400;

// ============================================================================
// Файлы виджета
// ============================================================================

/// Каталог виджета и его файлы настроек.
struct Widget {
    /// Полный путь виджета (каталог или один файл) — ключ разрешения.
    path: String,
    /// Каталог, от которого пишутся пути в сообщениях.
    base: String,
    confs: Vec<String>,
}

fn widget(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Widget {
    let path = crate::conky_scan::widget_path(ctx, id);
    let is_dir = !io.list_dir(&path).is_empty();
    let base = if is_dir { path.clone() } else { crate::de_apply::conky_work_dir(&path).to_string() };
    let primary = if crate::conky_scan::is_found_name(id) { None } else { crate::de_apply::conky_conf(io, ctx, id) };
    let confs = crate::conky_scan::widget_confs(io, ctx, id, primary);
    Widget { path, base, confs }
}

fn rel(base: &str, path: &str) -> String {
    path.strip_prefix(&format!("{}/", base.trim_end_matches('/')))
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string())
}

fn ext_of(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// Все текстовые файлы каталога виджета (без `.git`).
fn walk(io: &dyn LookIo, dir: &str, depth: usize, out: &mut Vec<String>) {
    for name in io.list_dir(dir) {
        if out.len() >= MAX_FILES {
            return;
        }
        let path = format!("{dir}/{name}");
        if !io.list_dir(&path).is_empty() {
            if depth < MAX_DEPTH && name != ".git" {
                walk(io, &path, depth + 1, out);
            }
        } else {
            out.push(path);
        }
    }
}

/// Путь из директивы — к файлу на диске. conky и его команды работают из
/// папки файла настроек (`de_apply::conky_work_dir`), `~` — домашний
/// каталог. Голое имя без косой черты — программа из PATH, не файл.
fn resolve(ctx: &Ctx, work_dir: &str, token: &str) -> Option<String> {
    let t = token.trim_matches(|c| c == '"' || c == '\'');
    if let Some(rest) = t.strip_prefix("~/") {
        return Some(format!("{}/{rest}", ctx.home.trim_end_matches('/')));
    }
    if let Some(rest) = t.strip_prefix("$HOME/").or_else(|| t.strip_prefix("${HOME}/")) {
        return Some(format!("{}/{rest}", ctx.home.trim_end_matches('/')));
    }
    if t.starts_with('/') {
        return Some(t.to_string());
    }
    if t.contains('/') && !t.contains("://") {
        let t = t.strip_prefix("./").unwrap_or(t);
        return Some(format!("{}/{t}", work_dir.trim_end_matches('/')));
    }
    None
}

// ============================================================================
// Разбор
// ============================================================================

/// Уровень `=` длинной скобки Lua в позиции `i` (`[[` — 0, `[==[` — 2).
fn long_bracket(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') {
        j += 1;
    }
    (b.get(j) == Some(&b'[')).then_some(j - i - 1)
}

/// Конец длинной скобки уровня `level`, начатой в `i`: позиция после `]=*]`.
fn long_end(b: &[u8], i: usize, level: usize) -> usize {
    let close: Vec<u8> = std::iter::once(b']').chain(std::iter::repeat_n(b'=', level)).chain(std::iter::once(b']')).collect();
    let start = i + level + 2;
    b[start.min(b.len())..]
        .windows(close.len())
        .position(|w| w == close.as_slice())
        .map(|p| start + p + close.len())
        .unwrap_or(b.len())
}

/// Текст Lua без комментариев: комментарии заменены пробелами (переводы
/// строк остаются), строки — как были.
fn strip_lua_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    let blank = |out: &mut Vec<u8>, part: &[u8]| {
        out.extend(part.iter().map(|&c| if c == b'\n' { b'\n' } else { b' ' }));
    };
    while i < b.len() {
        let c = b[i];
        if c == b'-' && b.get(i + 1) == Some(&b'-') {
            let end = match long_bracket(b, i + 2) {
                Some(level) => long_end(b, i + 2, level),
                None => b[i..].iter().position(|&c| c == b'\n').map(|p| i + p).unwrap_or(b.len()),
            };
            blank(&mut out, &b[i..end]);
            i = end;
        } else if let Some(level) = long_bracket(b, i) {
            let end = long_end(b, i, level);
            out.extend_from_slice(&b[i..end]);
            i = end;
        } else if c == b'"' || c == b'\'' {
            let mut j = i + 1;
            while j < b.len() && b[j] != c && b[j] != b'\n' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(b.len());
            out.extend_from_slice(&b[i..end]);
            i = end;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Строковый литерал Lua с позиции `i` (пробелы до него пропускаются):
/// `(содержимое, позиция после)`.
fn lua_string_at(s: &str, i: usize) -> Option<(String, usize)> {
    let b = s.as_bytes();
    let mut i = i;
    while b.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
        i += 1;
    }
    let q = *b.get(i)?;
    if q == b'"' || q == b'\'' {
        let mut j = i + 1;
        let mut val: Vec<u8> = Vec::new();
        while j < b.len() && b[j] != q && b[j] != b'\n' {
            if b[j] == b'\\' && j + 1 < b.len() {
                val.push(b[j + 1]);
                j += 2;
            } else {
                val.push(b[j]);
                j += 1;
            }
        }
        return Some((String::from_utf8_lossy(&val).into_owned(), (j + 1).min(b.len())));
    }
    let level = long_bracket(b, i)?;
    let end = long_end(b, i, level);
    let inner_start = (i + level + 2).min(end);
    let inner_end = end.saturating_sub(level + 2).max(inner_start);
    Some((s[inner_start..inner_end].to_string(), end))
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Позиции слова `word` целиком (не часть другого имени).
fn word_positions<'a>(s: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    let b = s.as_bytes();
    s.match_indices(word).map(|(p, _)| p).filter(move |&p| {
        (p == 0 || !is_ident(b[p - 1]) && b[p - 1] != b'.') && b.get(p + word.len()).is_none_or(|&c| !is_ident(c))
    })
}

/// Директивы `${…}` в тексте: `(вид, что)`.
fn directives(text: &str) -> Vec<(RiskKind, String)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = text[i..].find("${") {
        let start = i + p + 2;
        let mut j = start;
        while j < b.len() && is_ident(b[j]) {
            j += 1;
        }
        let name = &text[start..j];
        // Конец директивы — парная `}` (внутри бывают вложенные `${…}`).
        let mut depth = 1;
        let mut k = j;
        while k < b.len() {
            match b[k] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        let args = text[j..k.min(b.len())].trim();
        if let Some((_, interval)) = COMMAND_DIRECTIVES.iter().find(|(n, _)| *n == name) {
            let cmd = if *interval {
                args.split_once(char::is_whitespace).map(|(_, rest)| rest.trim()).unwrap_or("")
            } else {
                args
            };
            if !cmd.is_empty() {
                out.push((RiskKind::Command, cmd.to_string()));
            }
        } else if NETWORK_DIRECTIVES.contains(&name) {
            if let Some(url) = args.split_whitespace().next() {
                out.push((RiskKind::Network, url.to_string()));
            }
        }
        // Вложенные директивы разбираются следующим шагом поиска.
        i = j.max(start);
    }
    out
}

/// Старый синтаксис (1.9): настройки до строки `TEXT`, после — текст.
fn is_old_syntax(text: &str) -> bool {
    !text.contains("conky.config") && !text.contains("conky.text") && text.lines().any(|l| l.trim() == "TEXT")
}

/// Строка 1.9 без комментария `#` (`\#` — знак, а не комментарий).
fn strip_hash_comment(line: &str) -> &str {
    let b = line.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'#' && (i == 0 || b[i - 1] != b'\\') {
            return &line[..i];
        }
    }
    line
}

/// Директивы файла настроек: команды, адреса и сценарии `lua_load`.
fn scan_conf(text: &str) -> Vec<(RiskKind, String)> {
    let mut out = Vec::new();
    if is_old_syntax(text) {
        let mut in_text = false;
        let mut body = String::new();
        for line in text.lines() {
            if in_text {
                body.push_str(line);
                body.push('\n');
                continue;
            }
            if line.trim() == "TEXT" {
                in_text = true;
                continue;
            }
            let line = strip_hash_comment(line).trim();
            if let Some(rest) = line.strip_prefix("lua_load").filter(|r| r.starts_with(char::is_whitespace)) {
                out.extend(rest.split_whitespace().map(|s| (RiskKind::LuaScript, s.to_string())));
            }
        }
        out.extend(directives(&body));
        return out;
    }
    let code = strip_lua_comments(text);
    for p in word_positions(&code, "lua_load").collect::<Vec<_>>() {
        let after = &code[p + "lua_load".len()..];
        let Some(eq) = after.find(|c: char| !c.is_whitespace()).filter(|&e| after[e..].starts_with('=')) else {
            continue;
        };
        let at = p + "lua_load".len() + eq + 1;
        if let Some((value, _)) = lua_string_at(&code, at) {
            out.extend(value.split_whitespace().map(|s| (RiskKind::LuaScript, s.to_string())));
        }
    }
    out.extend(directives(&code));
    out
}

/// Неизвестная при разборе часть команды, собранной сценарием Lua.
const UNKNOWN_PART: &str = "…";

/// Куски выражения `expr`, разделённые `sep` вне строк и скобок.
fn top_level_split<'a>(expr: &'a str, sep: &[u8]) -> Vec<&'a str> {
    let b = expr.as_bytes();
    let mut out = Vec::new();
    let (mut depth, mut start, mut i) = (0i32, 0, 0);
    while i < b.len() {
        let c = b[i];
        if c == b'"' || c == b'\'' || long_bracket(b, i).is_some() {
            i = lua_string_at(expr, i).map(|(_, end)| end).unwrap_or(b.len()).max(i + 1);
            continue;
        }
        match c {
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => depth -= 1,
            _ => {}
        }
        if depth == 0 && b[i..].starts_with(sep) {
            out.push(&expr[start..i]);
            i += sep.len();
            start = i;
            continue;
        }
        i += 1;
    }
    out.push(&expr[start..]);
    out
}

/// Последнее до позиции `before` присваивание `name = …` (и `local name =
/// …`): `(выражение, позиция присваивания)`. Выражение — до конца строки, а
/// строка, оборванная на `..`, продолжается следующей.
fn lua_assignment<'a>(code: &'a str, name: &str, before: usize) -> Option<(&'a str, usize)> {
    let p = word_positions(code, name)
        .filter(|&p| p < before)
        .filter(|&p| {
            let after = code[p + name.len()..].trim_start();
            after.starts_with('=') && !after.starts_with("==")
        })
        .last()?;
    let eq = p + name.len() + code[p + name.len()..].find('=')? + 1;
    let mut end = eq;
    loop {
        end += code[end..].find('\n').unwrap_or(code.len() - end);
        if end >= code.len() || !code[eq..end].trim_end().ends_with("..") {
            break;
        }
        end += 1;
    }
    Some((&code[eq..end], p))
}

/// Текст команды из выражения Lua `expr` (у вызова в позиции `at`):
/// строки как есть, имена — по их присваиванию выше по тексту,
/// `string.format("…", …)` — по шаблону, остальное — [`UNKNOWN_PART`].
/// `None` — в выражении нет ни одной известной строки.
fn lua_command_text(code: &str, expr: &str, at: usize, depth: usize) -> Option<String> {
    if depth > 4 {
        return None;
    }
    let mut text = String::new();
    let mut known = false;
    for part in top_level_split(expr, b"..") {
        let part = part.trim();
        let piece = if let Some((s, _)) = lua_string_at(part, 0).filter(|(_, end)| *end == part.len()) {
            Some(s)
        } else if let Some(args) = part.strip_prefix("string.format").map(str::trim_start).and_then(|a| a.strip_prefix('(')) {
            lua_string_at(args, 0).map(|(f, _)| format_template(&f))
        } else if !part.is_empty() && part.bytes().all(|c| is_ident(c) || c == b'.') && !part.as_bytes()[0].is_ascii_digit() {
            // `cmd = cmd .. ' -x'` берёт прежнее значение — ищется выше
            // самого присваивания.
            lua_assignment(code, part, at).and_then(|(e, p)| lua_command_text(code, e, p, depth + 1))
        } else {
            None
        };
        match piece {
            Some(s) => {
                known = true;
                text.push_str(&s);
            }
            None => text.push_str(UNKNOWN_PART),
        }
    }
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (known && text != UNKNOWN_PART).then_some(text)
}

/// Шаблон `string.format`: подстановки `%s`, `%d`… — [`UNKNOWN_PART`],
/// `%%` — знак процента.
fn format_template(f: &str) -> String {
    let mut out = String::new();
    let mut chars = f.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            out.push('%');
            continue;
        }
        while chars.peek().is_some_and(|c| !c.is_ascii_alphabetic()) {
            chars.next();
        }
        chars.next();
        out.push_str(UNKNOWN_PART);
    }
    out
}

/// Вызовы команд из сценария Lua: `os.execute(…)`, `io.popen(…)` и
/// директивы в его строках (`conky_parse("${exec …}")`).
fn scan_lua(text: &str) -> Vec<(RiskKind, String)> {
    let code = strip_lua_comments(text);
    let b = code.as_bytes();
    let mut out = Vec::new();
    for call in ["os.execute", "io.popen"] {
        for p in code.match_indices(call).map(|(p, _)| p) {
            if p > 0 && (is_ident(b[p - 1]) || b[p - 1] == b'.') {
                continue;
            }
            let mut j = p + call.len();
            while b.get(j).is_some_and(|c| c.is_ascii_whitespace()) {
                j += 1;
            }
            let paren = b.get(j) == Some(&b'(');
            let at = if paren { j + 1 } else { j };
            // Литерал целиком — сама команда; `"x" .. var` — выражение.
            let literal = lua_string_at(&code, at).filter(|(_, end)| {
                let rest = code[*end..].trim_start();
                !paren || rest.starts_with(')') || rest.starts_with(',')
            });
            let what = match literal {
                Some((s, _)) => s,
                None if paren => {
                    let mut depth = 1;
                    let mut k = at;
                    while k < b.len() && depth > 0 {
                        match b[k] {
                            b'(' => depth += 1,
                            b')' => depth -= 1,
                            _ => {}
                        }
                        k += 1;
                    }
                    let args = &code[at..k.saturating_sub(1).max(at)];
                    // Команда, собранная из строк (`local cmd = 'cava -p ' ..
                    // CONFIG .. ' 2>/dev/null'` → `io.popen(cmd, 'r')`), —
                    // текстом, неизвестные части — «…»: так видны и человеку,
                    // и проверке недостающих программ ([`health`]).
                    match lua_command_text(&code, top_level_split(args, b",").first().copied().unwrap_or(""), p, 0) {
                        Some(text) => text,
                        None => {
                            let expr = args.split_whitespace().collect::<Vec<_>>().join(" ");
                            format!("{call}({})", expr.chars().take(120).collect::<String>())
                        }
                    }
                }
                None => continue,
            };
            if !what.trim().is_empty() {
                out.push((RiskKind::Command, what.trim().to_string()));
            }
        }
    }
    out.extend(directives(&code));
    out
}

/// Найденное в одном файле: опасности и файлы, на которые они ссылаются.
struct Scan {
    risks: Vec<Risk>,
    refs: Vec<String>,
}

fn scan(io: &dyn LookIo, ctx: &Ctx, w: &Widget) -> Scan {
    let mut risks: Vec<Risk> = Vec::new();
    let mut refs: Vec<String> = Vec::new();
    let push = |risks: &mut Vec<Risk>, kind: RiskKind, what: String, file: &str| {
        let r = Risk { kind, what, file: rel(&w.base, file) };
        if !risks.contains(&r) {
            risks.push(r);
        }
    };
    // Сценарии Lua: всё `.lua` виджета и то, что грузит `lua_load`.
    let mut lua_files: Vec<String> = Vec::new();
    if w.path == w.base {
        let mut all = Vec::new();
        walk(io, &w.base, 1, &mut all);
        lua_files.extend(all.into_iter().filter(|p| ext_of(p) == "lua"));
    }
    for conf in &w.confs {
        let Some(text) = io.read(conf) else { continue };
        let work_dir = crate::de_apply::conky_work_dir(conf).to_string();
        for (kind, what) in scan_conf(&text) {
            match kind {
                RiskKind::LuaScript => {
                    if let Some(p) = resolve(ctx, &work_dir, &what).or_else(|| Some(format!("{work_dir}/{what}"))) {
                        if !lua_files.contains(&p) {
                            lua_files.push(p.clone());
                        }
                        refs.push(p);
                    }
                }
                RiskKind::Command => {
                    refs.extend(what.split_whitespace().filter_map(|t| resolve(ctx, &work_dir, t)));
                }
                RiskKind::Network => {}
            }
            push(&mut risks, kind, what, conf);
        }
    }
    for lua in &lua_files {
        let Some(text) = io.read(lua) else { continue };
        let work_dir = w.confs.first().map(|c| crate::de_apply::conky_work_dir(c).to_string()).unwrap_or_else(|| w.base.clone());
        for (kind, what) in scan_lua(&text) {
            if kind == RiskKind::Command {
                refs.extend(
                    what.split_whitespace().filter(|t| !t.contains(UNKNOWN_PART)).filter_map(|t| resolve(ctx, &work_dir, t)),
                );
            }
            push(&mut risks, kind, what, lua);
        }
    }
    Scan { risks, refs }
}

/// Опасные директивы виджета `id` (имя в каталоге Rexon или путь
/// найденного на диске). Пусто — виджет только рисует.
pub fn scan_widget(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Vec<Risk> {
    let w = widget(io, ctx, id);
    let mut risks = scan(io, ctx, &w).risks;
    risks.sort_by(|a, b| a.kind.cmp(&b.kind));
    risks
}

// ============================================================================
// Отпечаток и разрешения
// ============================================================================

/// Отпечаток файлов-кода виджета: файлы настроек, сценарии (по расширению
/// или `#!` в первой строке) и файлы, на которые ссылаются директивы.
/// Двоичные и прочие данные (кэш, картинки) не входят.
fn fingerprint(io: &dyn LookIo, w: &Widget, refs: &[String]) -> String {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut add = |path: &str, text: String| {
        if !files.iter().any(|(p, _)| p == path) {
            files.push((path.to_string(), text));
        }
    };
    for conf in &w.confs {
        if let Some(t) = io.read(conf) {
            add(conf, t);
        }
    }
    if w.path == w.base {
        let mut all = Vec::new();
        walk(io, &w.base, 1, &mut all);
        for p in all {
            let Some(t) = io.read(&p) else { continue };
            let code = CODE_EXTS.contains(&ext_of(&p).as_str())
                || t.starts_with("#!")
                || crate::conky_scan::looks_like_config(&t);
            if code {
                add(&p, t);
            }
        }
    }
    for r in refs {
        if let Some(t) = io.read(r) {
            add(r, t);
        }
    }
    files.sort();
    let mut h = Sha256::new();
    for (p, t) in &files {
        h.update(rel(&w.base, p).as_bytes());
        h.update([0u8]);
        h.update((t.len() as u64).to_le_bytes());
        h.update(t.as_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Отпечаток файлов-кода виджета `id` в нынешнем виде — тот же, что у
/// разрешения. Снимок виджета для плитки (`conky_preview`) по нему узнаёт,
/// что виджет поменялся и снимок пора переснять.
pub fn code_fingerprint(io: &dyn LookIo, ctx: &Ctx, id: &str) -> String {
    let w = widget(io, ctx, id);
    let refs = scan(io, ctx, &w).refs;
    fingerprint(io, &w, &refs)
}

/// Файл разрешений: `~/.config/rexon/conky-trust.json`.
pub fn trust_path(ctx: &Ctx) -> String {
    ctx.config("rexon/conky-trust.json")
}

fn trusted_hash(io: &dyn LookIo, ctx: &Ctx, key: &str) -> Option<String> {
    let text = io.read(&trust_path(ctx))?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("widgets")?.get(key)?.get("sha256")?.as_str().map(str::to_string)
}

/// Состояние виджета перед запуском.
enum Verdict {
    /// Спрашивать не о чем: только рисует либо уже разрешён.
    Free,
    /// Нужен вопрос; `changed` — разрешали, но файлы с тех пор изменились.
    Ask { risks: Vec<Risk>, changed: bool },
}

fn verdict(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Verdict {
    let w = widget(io, ctx, id);
    let Scan { mut risks, refs } = scan(io, ctx, &w);
    if risks.is_empty() {
        return Verdict::Free;
    }
    risks.sort_by(|a, b| a.kind.cmp(&b.kind));
    match trusted_hash(io, ctx, &w.path) {
        Some(h) if h == fingerprint(io, &w, &refs) => Verdict::Free,
        Some(_) => Verdict::Ask { risks, changed: true },
        None => Verdict::Ask { risks, changed: false },
    }
}

/// Запомнить разрешение для виджета в его нынешнем виде. `false` — файл
/// разрешений не записался.
pub fn remember(io: &dyn LookIo, ctx: &Ctx, id: &str) -> bool {
    let w = widget(io, ctx, id);
    let refs = scan(io, ctx, &w).refs;
    let hash = fingerprint(io, &w, &refs);
    let path = trust_path(ctx);
    let mut v: serde_json::Value = io
        .read(&path)
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    v["version"] = serde_json::json!(1);
    if !v.get("widgets").is_some_and(serde_json::Value::is_object) {
        v["widgets"] = serde_json::json!({});
    }
    v["widgets"][w.path.as_str()] = serde_json::json!({ "sha256": hash });
    let text = serde_json::to_string_pretty(&v).unwrap_or_default();
    let ok = io.write(&path, &format!("{text}\n"));
    // Разрешённый виджет можно снять для плитки без вопроса.
    if ok {
        crate::conky_preview::want(ctx, id);
    }
    ok
}

// ============================================================================
// Слова для человека
// ============================================================================

/// Не больше стольких строк каждого вида в вопросе.
const LIST_LIMIT: usize = 8;

fn list_block(out: &mut String, header: String, items: &[&Risk], main_file: &str) {
    if items.is_empty() {
        return;
    }
    out.push_str("\n\n");
    out.push_str(&header);
    for r in items.iter().take(LIST_LIMIT) {
        out.push_str("\n• ");
        out.push_str(&r.what);
        if r.file != main_file {
            out.push_str(&format!(" ({})", r.file));
        }
    }
    if items.len() > LIST_LIMIT {
        let more = (items.len() - LIST_LIMIT) as u64;
        out.push_str("\n• ");
        out.push_str(&ntr("…and %n more.", "…and %n more.", more));
    }
}

/// Вопрос перед запуском: `(заголовок, текст)`. `None` — спрашивать не о
/// чем: виджет только рисует либо уже разрешён в нынешнем виде.
pub fn start_question(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Option<(String, String)> {
    let Verdict::Ask { risks, changed } = verdict(io, ctx, id) else { return None };
    let title = crate::conky_scan::widget_title(id);
    let w = widget(io, ctx, id);
    let main_file = w.confs.first().map(|c| rel(&w.base, c)).unwrap_or_default();
    let mut text = String::new();
    if changed {
        text.push_str(&changed_since_allowed_text());
        text.push_str("\n\n");
    }
    text.push_str(&trf(
        // TRANSLATORS: %1 is the widget name.
        "The Conky widget “%1” doesn't just draw: it runs programs on your computer with your rights. \
         Check what it does before starting it.",
        &[&title],
    ));
    let of = |k: RiskKind| risks.iter().filter(|r| r.kind == k).collect::<Vec<_>>();
    list_block(&mut text, tr("Runs commands:"), &of(RiskKind::Command), &main_file);
    list_block(&mut text, tr("Loads Lua scripts:"), &of(RiskKind::LuaScript), &main_file);
    list_block(&mut text, tr("Downloads from the internet:"), &of(RiskKind::Network), &main_file);
    text.push_str("\n\n");
    text.push_str(&tr(
        "Allow it only if you trust where the widget came from. Rexon will remember the answer \
         until the widget's files change.",
    ));
    Some((tr("Allow the widget to run commands?"), text))
}

/// Отказ кнопки «Запустить», пока виджет не разрешён (вопрос задаёт
/// интерфейс до запуска; сюда приходят запуски мимо вопроса — «Установить
/// и применить» из магазина). `None` — запускать можно.
pub fn start_refusal(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Option<String> {
    let Verdict::Ask { .. } = verdict(io, ctx, id) else { return None };
    Some(trf(
        // TRANSLATORS: %1 is the widget name; “▶️ Start” is the button on its tile.
        "The widget “%1” wasn't started: it runs commands or scripts, and you haven't allowed that yet. \
         Press “▶️ Start” on its tile to see what it runs.",
        &[&crate::conky_scan::widget_title(id)],
    ))
}

/// Пояснение в вопросе, когда виджет разрешали, но его файлы с тех пор
/// изменились.
pub fn changed_since_allowed_text() -> String {
    tr("⚠️ The widget's files have changed since you allowed it.")
}

/// Итог, когда человек не разрешил.
pub fn declined_text(id: &str) -> String {
    trf(
        // TRANSLATORS: %1 is the widget name.
        "The widget “%1” wasn't started: you didn't allow it to run commands.",
        &[&crate::conky_scan::widget_title(id)],
    )
}

// ============================================================================
// Здоровье: чего виджету не хватает (план паритета, п.40)
// ============================================================================

/// Слова оболочки, которые не программы: встроенные команды и ключевые
/// слова. Их отсутствие в PATH — не беда.
const SHELL_WORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in",
    "function", "echo", "printf", "cd", "test", "[", "[[", "]", "]]", "export", "local", "read",
    "set", "unset", "eval", "source", ".", "true", "false", "exit", "return", "shift", "let",
    "declare", "type", "wait", "trap", "!", "{", "}", "(", ")",
];

/// Обёртки, после которых идёт настоящая программа.
const WRAPPERS: &[&str] = &["env", "sudo", "nice", "nohup", "exec", "command", "stdbuf", "ionice"];

/// Программа, которой виджету не хватает.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingProgram {
    pub program: String,
    /// Пакет на этом семействе дистрибутива; `None` — Rexon его не знает
    /// или ставить не должен (nvidia-smi приходит с драйвером).
    pub package: Option<String>,
    /// Пакет из AUR, когда в репозиториях программы нет: Rexon его не
    /// ставит, но называет — человек поставит сам.
    pub aur: Option<String>,
}

/// Чего не хватает виджету для работы.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Health {
    /// Сценарии и файлы, на которые ссылается виджет, а их нет — как
    /// написано в виджете.
    pub missing_files: Vec<String>,
    pub missing_programs: Vec<MissingProgram>,
}

impl Health {
    pub fn is_fine(&self) -> bool {
        self.missing_files.is_empty() && self.missing_programs.is_empty()
    }

    /// Пакеты, которые можно поставить (без повторов).
    pub fn packages(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in self.missing_programs.iter().filter_map(|m| m.package.clone()) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }
}

/// Пакет программы `program` для семейства `family` — только точная запись
/// таблицы `utils::TOOL_PACKAGES` (без «как в Debian» для незнакомых
/// семейств и без пакетов только из AUR: их обычная установка не ставит).
pub fn program_package(program: &str, family: &str) -> Option<String> {
    crate::utils::TOOL_PACKAGES
        .iter()
        .find(|(bin, _)| *bin == program)?
        .1
        .iter()
        .find(|(f, _)| *f == family)
        .map(|(_, pkg)| *pkg)
        .filter(|pkg| !pkg.is_empty() && *pkg != "—" && !pkg.contains('('))
        .map(str::to_string)
}

/// Пакет программы `program` из AUR, когда в репозиториях семейства
/// `family` её нет (пометка «из AUR» таблицы `utils::TOOL_PACKAGES`).
pub fn program_aur_package(program: &str, family: &str) -> Option<String> {
    crate::utils::TOOL_PACKAGES
        .iter()
        .find(|(bin, _)| *bin == program)?
        .1
        .iter()
        .find(|(f, _)| *f == family)?
        .1
        .strip_suffix(crate::utils::AUR_ONLY_MARK)
        .map(str::to_string)
}

/// Программы команды оболочки: первое слово каждого звена (`a | b; c &&
/// d`), без присваиваний `X=1`, обёрток (`env`, `sudo`) и встроенных слов.
/// Слова с `$`, путями и кавычками-выражениями пропускаются: путь — это
/// файл, а `$X` известен только при запуске.
fn command_programs(cmd: &str) -> Vec<String> {
    let mut text = cmd.to_string();
    for sep in ["&&", "||", "$(", "`", "|", ";", "&", "(", ")"] {
        text = text.replace(sep, "\n");
    }
    let mut out: Vec<String> = Vec::new();
    for seg in text.lines() {
        let mut words = seg.split_whitespace().peekable();
        while let Some(w) = words.peek() {
            let assignment = w.split_once('=').is_some_and(|(k, _)| !k.is_empty() && k.bytes().all(is_ident));
            if assignment || WRAPPERS.contains(w) || w.starts_with('-') {
                words.next();
            } else {
                break;
            }
        }
        let Some(first) = words.next() else { continue };
        let prog = first.trim_matches(|c| c == '"' || c == '\'');
        let plain = !prog.is_empty()
            && !prog.contains(['/', '$', '{', '}', '<', '>', '=', '*', '"', '\'', '…'])
            && !prog.chars().all(|c| c.is_ascii_digit());
        if plain && !SHELL_WORDS.contains(&prog) && !out.iter().any(|p| p == prog) {
            out.push(prog.to_string());
        }
    }
    out
}

/// Проверка здоровья виджета `id`: недостающие сценарии (`lua_load`,
/// сценарии оболочки в командах) и программы из команд — с пакетом для
/// семейства `family` (`system::PkgFamily::as_str`).
pub fn health(io: &dyn LookIo, ctx: &Ctx, id: &str, family: &str) -> Health {
    let w = widget(io, ctx, id);
    let risks = scan(io, ctx, &w).risks;
    let work_dir = w.confs.first().map(|c| crate::de_apply::conky_work_dir(c).to_string()).unwrap_or_else(|| w.base.clone());
    let mut h = Health::default();
    let missing_file = |h: &mut Health, what: &str, path: String| {
        if io.read(&path).is_none() && io.list_dir(&path).is_empty() && !h.missing_files.iter().any(|f| f == what) {
            h.missing_files.push(what.to_string());
        }
    };
    for r in &risks {
        match r.kind {
            RiskKind::LuaScript => {
                let path = resolve(ctx, &work_dir, &r.what).unwrap_or_else(|| format!("{work_dir}/{}", r.what));
                missing_file(&mut h, &r.what, path);
            }
            RiskKind::Command => {
                // Вызов из Lua с выражением — команда известна только при
                // запуске.
                if r.what.starts_with("os.execute(") || r.what.starts_with("io.popen(") {
                    continue;
                }
                for t in r.what.split_whitespace() {
                    let t = t.trim_matches(|c| c == '"' || c == '\'');
                    if !CODE_EXTS.contains(&ext_of(t).as_str()) || t.contains('$') || t.contains(UNKNOWN_PART) {
                        continue;
                    }
                    if let Some(path) = resolve(ctx, &work_dir, t) {
                        missing_file(&mut h, t, path);
                    }
                }
                for prog in command_programs(&r.what) {
                    if io.have(&prog) || h.missing_programs.iter().any(|m| m.program == prog) {
                        continue;
                    }
                    let package = program_package(&prog, family);
                    let aur = program_aur_package(&prog, family);
                    h.missing_programs.push(MissingProgram { program: prog, package, aur });
                }
            }
            RiskKind::Network => {}
        }
    }
    h
}

/// Что делать с запуском виджета, которому чего-то не хватает. Правило
/// владельца 24.10 — «всё или ничего»: полуработающих виджетов не бывает.
/// Программы из официальных репозиториев Rexon ставит сам, без вопроса
/// «Да/Нет» (защита — окно пароля polkit); не поставилось, или программа
/// есть только в AUR либо нигде — виджет не запускается, а в итоге сказано
/// почему и что сделать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartPlan {
    /// Программ хватает — запускать.
    Go,
    /// Сначала поставить эти пакеты (только репозитории), потом
    /// [`after_install`].
    Install(Vec<String>),
    /// Не запускать: причина и что сделать.
    Refuse(String),
}

/// Кнопка, которой человек повторит попытку: «▶️ Запустить» плитки или
/// кнопка части виджета (часть пакеты не ставит — ставит запуск плитки).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    Tile,
    Part,
    /// «Применить» набора виджетов (окно или `rexon --conky-set`).
    Set,
    /// Переключатель «Запускать при входе»: при входе виджет запустится
    /// без Rexon, поэтому программы ставятся при включении.
    Autostart,
}

/// Отказ в запуске: какие программы не дают виджету работать и что
/// сделать с каждой. `installable` — Rexon может ставить пакеты здесь;
/// `failed` — причина, по которой не встало то, что ставили.
fn refusal(id: &str, h: &Health, installable: bool, failed: Option<&str>, retry: Retry) -> String {
    let all: Vec<&str> = h.missing_programs.iter().map(|m| m.program.as_str()).collect();
    let repo: Vec<&MissingProgram> = h.missing_programs.iter().filter(|m| installable && m.package.is_some()).collect();
    let aur: Vec<&MissingProgram> = h.missing_programs.iter().filter(|m| m.package.is_none() && m.aur.is_some()).collect();
    let none: Vec<&str> = h
        .missing_programs
        .iter()
        .filter(|m| m.aur.is_none() && !(installable && m.package.is_some()))
        .map(|m| m.program.as_str())
        .collect();
    let n = all.len() as u64;
    let mut parts = vec![crate::i18n::fill(
        // TRANSLATORS: %1 is the widget name, %2 a list of programs.
        &ntr(
            "The widget “%1” wasn't started: without the program %2 it doesn't fully work.",
            "The widget “%1” wasn't started: without the programs %2 it doesn't fully work.",
            n,
        ),
        &[&crate::conky_scan::widget_title(id), &all.join(", ")],
    )];
    // Окно пароля закрыто — это «передумал», а не поломка: причину не
    // повторяем, совет ниже и так про пароль.
    if let Some(why) = failed.filter(|w| *w != crate::appearance_installer::password_not_confirmed()) {
        parts.push(trf("Installation failed: %1.", &[&why]));
    }
    if !aur.is_empty() {
        let names: Vec<&str> = aur.iter().map(|m| m.program.as_str()).collect();
        let pkgs: Vec<&str> = aur.iter().filter_map(|m| m.aur.as_deref()).collect();
        parts.push(crate::i18n::fill(
            // TRANSLATORS: %1 is a list of programs, %2 an example command
            // that installs them from the AUR (yay -S …).
            &ntr(
                "%1 is only in the AUR — install it (for example, %2) and start the widget again.",
                "%1 are only in the AUR — install them (for example, %2) and start the widget again.",
                names.len() as u64,
            ),
            &[&names.join(", "), &format!("yay -S {}", pkgs.join(" "))],
        ));
    }
    if !none.is_empty() {
        // TRANSLATORS: %1 is a list of programs.
        parts.push(trf("Install %1 yourself and start the widget again.", &[&none.join(", ")]));
    }
    // Ставить самому нечего — повтор «Запустить» поставит всё сам.
    if !repo.is_empty() && aur.is_empty() && none.is_empty() {
        let n = repo.len() as u64;
        let start = tr("▶️ Start");
        parts.push(match retry {
            Retry::Tile => crate::i18n::fill(
                // TRANSLATORS: %1 is the label of the widget's start button.
                &ntr(
                    "Press “%1” again and enter the password to install it.",
                    "Press “%1” again and enter the password to install them.",
                    n,
                ),
                &[&start],
            ),
            Retry::Part => crate::i18n::fill(
                // TRANSLATORS: %1 is the label of the start button on the
                // widget's tile.
                &ntr(
                    "Start the widget with “%1” on its tile and enter the password to install it.",
                    "Start the widget with “%1” on its tile and enter the password to install them.",
                    n,
                ),
                &[&start],
            ),
            Retry::Set => ntr(
                "Apply the set again and enter the password to install it.",
                "Apply the set again and enter the password to install them.",
                n,
            ),
            Retry::Autostart => crate::i18n::fill(
                // TRANSLATORS: %1 is the label of the widget's autostart switch.
                &ntr(
                    "Turn on “%1” again and enter the password to install it.",
                    "Turn on “%1” again and enter the password to install them.",
                    n,
                ),
                &[&crate::appearance_page::detail_autostart_text()],
            ),
        });
    }
    parts.join(" ")
}

/// План запуска виджета `id` по его проверке (`installable` — не SteamOS,
/// не неизменяемая система). Ставится только то, что есть в официальных
/// репозиториях, и только когда этого хватит: если что-то придётся ставить
/// человеку (AUR, нет пакета), пароль не спрашивается вовсе.
pub fn start_plan(id: &str, h: &Health, installable: bool) -> StartPlan {
    plan_for(id, h, installable, Retry::Tile)
}

fn plan_for(id: &str, h: &Health, installable: bool, retry: Retry) -> StartPlan {
    if h.missing_programs.is_empty() {
        return StartPlan::Go;
    }
    let self_installable = installable && h.missing_programs.iter().all(|m| m.package.is_some());
    if self_installable {
        StartPlan::Install(h.packages())
    } else {
        StartPlan::Refuse(refusal(id, h, installable, None, retry))
    }
}

/// План после установки пакетов [`StartPlan::Install`]: `fresh` — проверка
/// виджета ЗАНОВО (что встало, решает диск, а не код возврата), `result` —
/// исход установки. Не встало — виджет не запускается.
pub fn after_install(id: &str, before: &Health, fresh: &Health, result: &Result<(), String>) -> StartPlan {
    after_install_for(id, before, fresh, result, Retry::Tile)
}

fn after_install_for(id: &str, before: &Health, fresh: &Health, result: &Result<(), String>, retry: Retry) -> StartPlan {
    match result {
        Err(why) => StartPlan::Refuse(refusal(id, before, true, Some(why), retry)),
        Ok(()) if fresh.missing_programs.is_empty() => StartPlan::Go,
        // Пакет встал, а программы всё нет — ставить её больше нечем.
        Ok(()) => StartPlan::Refuse(refusal(id, fresh, false, None, retry)),
    }
}

/// Готов ли виджет к запуску: `Ok(примечание)` — запускать (примечание —
/// [`files_note`], дописать к итогу), `Err(отказ)` — не запускать.
pub type Readiness = Result<Option<String>, String>;

/// Общий путь «всё или ничего» перед запуском виджетов `ids` — у «▶️
/// Запустить» плитки, «Применить» набора и `rexon --conky-set`: проверка
/// каждого ([`health`]), недостающие пакеты из официальных репозиториев
/// ВСЕХ виджетов — одной установкой `install` (одно окно пароля на весь
/// набор, без вопроса «Да/Нет»), затем проверка заново: что не встало —
/// этот виджет не запускается, и в отказе сказано почему и что сделать
/// (`retry` — какой кнопкой повторить). Ответ — по виджету на каждый `id`,
/// в том же порядке.
pub fn prepare_start(
    io: &dyn LookIo,
    ctx: &Ctx,
    ids: &[String],
    family: &str,
    installable: bool,
    retry: Retry,
    install: &dyn Fn(&[String]) -> Result<(), String>,
) -> Vec<Readiness> {
    let checks: Vec<(Health, StartPlan)> = ids
        .iter()
        .map(|id| {
            let h = health(io, ctx, id, family);
            let plan = plan_for(id, &h, installable, retry);
            (h, plan)
        })
        .collect();
    let mut packages: Vec<String> = Vec::new();
    for (_, plan) in &checks {
        if let StartPlan::Install(list) = plan {
            for p in list {
                if !packages.contains(p) {
                    packages.push(p.clone());
                }
            }
        }
    }
    let result = if packages.is_empty() { Ok(()) } else { install(&packages) };
    ids.iter()
        .zip(checks)
        .map(|(id, (h, plan))| {
            let plan = match plan {
                StartPlan::Install(_) => after_install_for(id, &h, &health(io, ctx, id, family), &result, retry),
                other => other,
            };
            match plan {
                StartPlan::Refuse(text) => Err(text),
                _ => Ok(files_note(&h)),
            }
        })
        .collect()
}

/// [`prepare_start`] на этой машине: пакеты — [`install_packages_live`]
/// (pkexec + помощник, только репозитории).
pub fn live_prepare_start(ids: &[String], retry: Retry) -> Vec<Readiness> {
    let (family, installable) = live_family();
    let install = |packages: &[String]| {
        let result = install_packages_live(packages);
        if let Err(why) = &result {
            crate::log_warn!("conky", "пакеты виджетов Conky не встали ({}): {why}", packages.join(", "));
        }
        result
    };
    prepare_start(&crate::animations::SystemIo, &Ctx::live(), ids, &family, installable, retry, &install)
}

/// Отказ кнопке части виджета: она запускается в потоке интерфейса и
/// пакетов не ставит. `None` — программ хватает.
pub fn part_refusal(id: &str, h: &Health, installable: bool) -> Option<String> {
    (!h.missing_programs.is_empty()).then(|| refusal(id, h, installable, None, Retry::Part))
}

/// Недостающие файлы виджета — дописываются к итогу запуска (их пакетом
/// не поставить). `None` — всё на месте.
pub fn files_note(h: &Health) -> Option<String> {
    (!h.missing_files.is_empty()).then(|| trf("⚠️ The widget's files are missing: %1.", &[&h.missing_files.join(", ")]))
}

/// Семейство пакетов ЭТОЙ машины и может ли Rexon ставить на ней пакеты
/// (на SteamOS и неизменяемых системах системный пакет не встанет).
pub fn live_family() -> (String, bool) {
    let detector = crate::system::DistroDetector::detect_live();
    let installable = !detector.is_steamos() && !detector.is_immutable();
    (detector.family_live().as_str().to_string(), installable)
}

/// Поставить недостающие пакеты виджета тем же путём, что вся установка
/// пакетов Rexon (pkexec + `rexon-pkg-helper`), но только из официальных
/// репозиториев (`appearance_installer::live_install_repo_packages`): AUR
/// без вопроса не собирается.
/// `Err` — причина для человека.
pub fn install_packages_live(packages: &[String]) -> Result<(), String> {
    if !live_family().1 {
        return Err(tr("Rexon can't install programs on this system by itself."));
    }
    crate::appearance_installer::live_install_repo_packages(packages, crate::privilege::op::PkgSource::Tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_comments_are_cut_but_strings_kept() {
        let s = strip_lua_comments("a = '--x' -- y\n--[[ z\n]] b = [[--w]]\n");
        assert!(s.contains("'--x'") && !s.contains('y') && !s.contains('z') && s.contains("[[--w]]"), "{s}");
        assert_eq!(s.lines().count(), 3);
    }

    #[test]
    fn directives_with_interval_and_nesting() {
        let d = directives("${execi 10 echo ${time}} ${exec  ls -l } ${curl http://a/b 5} ${time %H}");
        assert_eq!(
            d,
            vec![
                (RiskKind::Command, "echo ${time}".to_string()),
                (RiskKind::Command, "ls -l".to_string()),
                (RiskKind::Network, "http://a/b".to_string()),
            ]
        );
    }

    #[test]
    fn lua_calls_with_expression() {
        let d = scan_lua("os.execute('a b')\nlocal h = io.popen(\"x \" .. y)\nmyos.execute('no')\n");
        assert_eq!(d[0], (RiskKind::Command, "a b".to_string()));
        // Строка с неизвестной частью — текстом, неизвестное — «…».
        assert_eq!(d[1], (RiskKind::Command, "x …".to_string()));
        assert_eq!(d.len(), 2);
        // Совсем без строк — вызовом с выражением.
        let d = scan_lua("io.popen(make_cmd(a), 'r')\n");
        assert_eq!(d, vec![(RiskKind::Command, "io.popen(make_cmd(a), 'r')".to_string())]);
    }

    #[test]
    fn lua_command_assembled_from_variables() {
        let lua = "local CONF = './cava.conf'\n\
                   local cmd = 'cava -p ' .. CONF ..\n    ' 2>/dev/null'\n\
                   p = io.popen(cmd, 'r')\n\
                   local n = string.format(\"notify-send %s '%d%%'\", title, v)\n\
                   os.execute(n)\n\
                   local s = 'ls'\n\
                   s = s .. ' -l ' .. dir\n\
                   os.execute(s)\n";
        let d: Vec<String> = scan_lua(lua).into_iter().map(|(_, w)| w).collect();
        assert_eq!(d, vec!["notify-send … '…%'", "ls -l …", "cava -p ./cava.conf 2>/dev/null"], "{d:?}");
        assert_eq!(command_programs("… -x | grep y"), vec!["grep".to_string()]);
    }
}
