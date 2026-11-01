//! Утилиты QML-синглтона `Util` вкладки Wallpaper Engine — порт
//! `ui/src/util.cpp` waywallen (MIT, (c) hypengw; атрибуция в NOTICE).
//!
//! Синглтон отдаёт вендоренному QML то, что неудобно писать инлайном на
//! JavaScript: определение рабочего стола из `XDG_CURRENT_DESKTOP`,
//! конвертацию описаний Steam Workshop (BBCode) в подмножество HTML для
//! `Qt.StyledText` и разбор/сборку цвета в проводном формате Wallpaper
//! Engine. Qt здесь не нужен: вход и выход — строки и числа, поэтому вся
//! логика живёт в ядре, а Python-шим (`qml_ui/bridge.py`) остаётся тонким.
//!
//! ⚠️ Наш прежний Python-мост был ЗАГЛУШКОЙ и врал в трёх местах — это
//! чинится здесь (правило #16 «баги при порте чиним»):
//!   * `bbcodeToHtml` возвращал текст как есть (описания обоев в панели
//!     деталей показывались сырым BBCode);
//!   * `colorFromWire`/`colorToWire` считали строку ИМЕНЕМ цвета Qt
//!     (`#rrggbb`), тогда как проводной формат WE — «r g b [a]», числа
//!     0..1 через пробел. Из-за этого палитра пользовательских свойств
//!     обоев читала чужое значение как невалидный цвет и писала обратно
//!     hex, которого демон не понимает;
//!   * `colorHasAlpha` считал шестнадцатеричные цифры вместо чисел.
//!
//! ⚠️ Починен и баг САМОГО апстрима (кандидат в репорт): их авто-ссылки
//! (`Util::bbcodeToHtml`, последний проход) идут по всему тексту одной
//! регуляркой `(href=")|(https?://…)` и не пропускают уже собранный
//! якорь — любая ссылка из `[url]`/`[url=]` разворачивалась во вложенный
//! мусор вида `<a href="<a href="http://x">http://x">…</a></a>`
//! (проверено симуляцией их регулярки). Здесь авто-ссылка не заходит
//! внутрь разметки: `<a …>…</a>` и любые теги переносятся как есть.
//!
//! Регулярных выражений нет намеренно (в проекте нет зависимости `regex`,
//! и порядок проходов важнее краткости) — разбор посимвольный, той же
//! грамматики; порядок проходов 1-в-1 с оригиналом, иначе `\n` внутри
//! тела тега успевал превратиться в `<br>` раньше разбора самого тега.

// ── Рабочий стол сессии ──────────────────────────────────────────────

/// `Desktop::Unknown` — порт enum `Util::Desktop`.
pub const DESKTOP_UNKNOWN: i32 = 0;
/// `Desktop::Kde`.
pub const DESKTOP_KDE: i32 = 1;
/// `Desktop::Hyprland`.
pub const DESKTOP_HYPRLAND: i32 = 2;
/// `Desktop::Sway`.
pub const DESKTOP_SWAY: i32 = 3;
/// `Desktop::Niri`.
pub const DESKTOP_NIRI: i32 = 4;
/// `Desktop::Gnome`.
pub const DESKTOP_GNOME: i32 = 5;

/// Рабочий стол по `XDG_CURRENT_DESKTOP` — порт `Util::desktop`.
///
/// Побеждает ПЕРВЫЙ известный токен списка (`KDE:wayland` → Kde), пустые
/// токены пропускаются, регистр не важен — та же семантика, что у
/// `display::spawner::detect_de` на стороне демона.
pub fn detect_desktop(xdg_current_desktop: &str) -> i32 {
    for token in xdg_current_desktop.split(':') {
        match token.trim().to_ascii_lowercase().as_str() {
            "kde" => return DESKTOP_KDE,
            "hyprland" => return DESKTOP_HYPRLAND,
            "sway" => return DESKTOP_SWAY,
            "niri" => return DESKTOP_NIRI,
            "gnome" => return DESKTOP_GNOME,
            _ => {}
        }
    }
    DESKTOP_UNKNOWN
}

/// Умеет ли композитор переименовывать выходы — порт
/// `Util::supportsDisplayRename` (Hyprland/Sway/Niri).
pub fn supports_display_rename(desktop: i32) -> bool {
    matches!(desktop, DESKTOP_HYPRLAND | DESKTOP_SWAY | DESKTOP_NIRI)
}

// ── Подпись дисплея ──────────────────────────────────────────────────

/// Подпись дисплея — порт `Display::displayLabel` (`ui/src/objmodel/
/// display.cppm`, waywallen 0.4.4): псевдоним, а без него имя выхода, и
/// ВСЕГДА номер дисплея в скобках — «HEADLESS-2 (#1)». Номер стоит не для
/// различения тёзок: у автора он приписывается безусловно. Без имени и
/// псевдонима — «Display #N»; эту строку автор собирает в C++ без `tr()`,
/// поэтому и у нас она не переводится.
pub fn display_label(alias: &str, name: &str, id: u64) -> String {
    let base = if alias.is_empty() { name } else { alias };
    if base.is_empty() {
        format!("Display #{id}")
    } else {
        format!("{base} (#{id})")
    }
}

// ── BBCode → подмножество HTML для Qt.StyledText ─────────────────────

/// Экранирование `&`, `<`, `>` — порт `escapeHtml`.
fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Позиция ASCII-подстроки без учёта регистра, начиная с `from`
/// (байтовый индекс; шаблоны ASCII → индекс всегда на границе символа).
fn find_ci(hay: &str, needle: &str, from: usize) -> Option<usize> {
    if needle.is_empty() || from > hay.len() {
        return None;
    }
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if h.len() < n.len() {
        return None;
    }
    let eq = |a: u8, b: u8| a.eq_ignore_ascii_case(&b);
    for i in from..=(h.len() - n.len()) {
        if n.iter().zip(&h[i..]).all(|(a, b)| eq(*a, *b)) {
            return Some(i);
        }
    }
    None
}

/// `[tag]…[/tag]` → `open…close` для всех непересекающихся пар слева
/// направо — порт `replaceTag` (нежадное `(.*?)` = ближайший закрывающий).
fn replace_tag(input: &str, tag: &str, open: &str, close: &str) -> String {
    let open_pat = format!("[{tag}]");
    let close_pat = format!("[/{tag}]");
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(start) = find_ci(input, &open_pat, cursor) {
        let body_at = start + open_pat.len();
        let Some(end) = find_ci(input, &close_pat, body_at) else {
            break;
        };
        out.push_str(&input[cursor..start]);
        out.push_str(open);
        out.push_str(&input[body_at..end]);
        out.push_str(close);
        cursor = end + close_pat.len();
    }
    out.push_str(&input[cursor..]);
    out
}

/// Замена литерала без учёта регистра (`[hr]`, `[list]`, `[/list]`).
fn replace_ci(input: &str, pat: &str, repl: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(start) = find_ci(input, pat, cursor) {
        out.push_str(&input[cursor..start]);
        out.push_str(repl);
        cursor = start + pat.len();
    }
    out.push_str(&input[cursor..]);
    out
}

/// `[quote]`/`[quote=кто]` … `[/quote]` → `<i>“…”</i>` (автор не
/// показывается — в StyledText его некуда деть, порт оригинала).
fn replace_quote(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(start) = find_ci(input, "[quote", cursor) {
        // после «[quote» допустимы либо «]», либо «=что-угодно-без-]».
        let rest = &input[start + 6..];
        let Some(close_bracket) = rest.find(']') else {
            break;
        };
        let attr = &rest[..close_bracket];
        if !attr.is_empty() && !attr.starts_with('=') {
            // «[quoted…» — не наш тег, идём дальше.
            cursor = start + 6;
            continue;
        }
        let body_at = start + 6 + close_bracket + 1;
        let Some(end) = find_ci(input, "[/quote]", body_at) else {
            break;
        };
        out.push_str(&input[cursor..start]);
        out.push_str("<i>“");
        out.push_str(&input[body_at..end]);
        out.push_str("”</i>");
        cursor = end + "[/quote]".len();
    }
    out.push_str(&input[cursor..]);
    out
}

/// `[*]` + следующие пробельные символы → `<li>` (закрытие пункта берёт
/// на себя `</ul>`, как в оригинале).
fn replace_list_items(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(start) = find_ci(input, "[*]", cursor) {
        out.push_str(&input[cursor..start]);
        out.push_str("<li>");
        let mut after = start + 3;
        while let Some(ch) = input[after..].chars().next() {
            if ch.is_whitespace() {
                after += ch.len_utf8();
            } else {
                break;
            }
        }
        cursor = after;
    }
    out.push_str(&input[cursor..]);
    out
}

/// `[url=X]Y[/url]` → `<a href="X">Y</a>` (кавычки вокруг X необязательны).
fn replace_url_with_target(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(start) = find_ci(input, "[url=", cursor) {
        let attr_at = start + 5;
        let Some(rel) = input[attr_at..].find(']') else {
            break;
        };
        let raw = &input[attr_at..attr_at + rel];
        let body_at = attr_at + rel + 1;
        let Some(end) = find_ci(input, "[/url]", body_at) else {
            break;
        };
        // Кавычка вокруг адреса — необязательная парная (как в оригинале).
        let target = match raw.chars().next() {
            Some(q @ ('"' | '\'')) if raw.len() >= 2 && raw.ends_with(q) => {
                &raw[1..raw.len() - 1]
            }
            _ => raw,
        };
        out.push_str(&input[cursor..start]);
        out.push_str("<a href=\"");
        out.push_str(target);
        out.push_str("\">");
        out.push_str(&input[body_at..end]);
        out.push_str("</a>");
        cursor = end + "[/url]".len();
    }
    out.push_str(&input[cursor..]);
    out
}

/// Обёртка голых `http(s)://…` в ссылки.
///
/// ⚠️ Отличие от оригинала (починка его бага): проход НЕ заходит внутрь
/// уже готовой разметки. Любой тег `<…>` переносится как есть, а после
/// `<a …>` текст до `</a>` не трогается — иначе адрес внутри `href="…"`
/// и подпись ссылки заворачивались повторно.
fn autolink(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < input.len() {
        if bytes[i] == b'<' {
            let anchor = find_ci(input, "<a ", i) == Some(i)
                || find_ci(input, "<a>", i) == Some(i);
            let tag_end = match input[i..].find('>') {
                Some(rel) => i + rel + 1,
                None => input.len(),
            };
            out.push_str(&input[i..tag_end]);
            i = tag_end;
            if anchor {
                // тело якоря и его закрывающий тег — без авто-ссылок
                let close = find_ci(input, "</a>", i).unwrap_or(input.len());
                let to = (close + 4).min(input.len());
                out.push_str(&input[i..to]);
                i = to;
            }
            continue;
        }
        let is_url = find_ci(input, "http://", i) == Some(i)
            || find_ci(input, "https://", i) == Some(i);
        if is_url {
            let mut end = i;
            while let Some(ch) = input[end..].chars().next() {
                if ch.is_whitespace() || matches!(ch, '<' | '[' | ']') {
                    break;
                }
                end += ch.len_utf8();
            }
            let url = &input[i..end];
            out.push_str("<a href=\"");
            out.push_str(url);
            out.push_str("\">");
            out.push_str(url);
            out.push_str("</a>");
            i = end;
            continue;
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Описание Steam Workshop (BBCode) → подмножество HTML `Qt.StyledText`
/// — порт `Util::bbcodeToHtml` (порядок проходов сохранён).
pub fn bbcode_to_html(src: &str) -> String {
    if src.is_empty() {
        return String::new();
    }

    // 1. Тела [noparse] прячем за непрозрачными метками, чтобы дальнейшие
    //    проходы их не трогали; вернём в самом конце экранированными.
    let mut stash: Vec<String> = Vec::new();
    let mut stage = {
        let mut out = String::with_capacity(src.len());
        let mut cursor = 0usize;
        while let Some(start) = find_ci(src, "[noparse]", cursor) {
            let body_at = start + "[noparse]".len();
            let Some(end) = find_ci(src, "[/noparse]", body_at) else {
                break;
            };
            out.push_str(&src[cursor..start]);
            out.push_str(&format!("\u{1}NP{}\u{1}", stash.len()));
            stash.push(src[body_at..end].to_string());
            cursor = end + "[/noparse]".len();
        }
        out.push_str(&src[cursor..]);
        out
    };

    // 2. Экранирование: автор описания не должен протащить сырой HTML.
    stage = escape_html(&stage);

    // 3. Строчная разметка.
    for (tag, open, close) in [
        ("b", "<b>", "</b>"),
        ("i", "<i>", "</i>"),
        ("u", "<u>", "</u>"),
        ("s", "<s>", "</s>"),
        ("strike", "<s>", "</s>"),
        ("code", "<tt>", "</tt>"),
        ("spoiler", "<font color=\"#888888\">", "</font>"),
    ] {
        stage = replace_tag(&stage, tag, open, close);
    }

    // 4. Цитаты.
    stage = replace_quote(&stage);

    // 5. Заголовки — на ступень мельче, чтобы [h1] не давил панель.
    stage = replace_tag(&stage, "h1", "<h2>", "</h2>");
    stage = replace_tag(&stage, "h2", "<h3>", "</h3>");
    stage = replace_tag(&stage, "h3", "<h4>", "</h4>");

    // 6. Линейки и списки.
    stage = replace_ci(&stage, "[hr]", "<hr>");
    stage = replace_ci(&stage, "[list]", "<ul>");
    stage = replace_ci(&stage, "[/list]", "</ul>");
    stage = replace_list_items(&stage);

    // 7. Ссылки и картинки.
    stage = replace_url_with_target(&stage);
    stage = replace_bare_url(&stage);
    stage = replace_tag(&stage, "img", "<img src=\"", "\">");

    // 8. Голые адреса (с починкой: внутрь разметки не заходим).
    stage = autolink(&stage);

    // 9. Переводы строк — последними, чтобы `\n` внутри тел тегов дожил
    //    до их разбора.
    stage = stage.replace("\r\n", "\n").replace('\n', "<br>");

    // 10. Возврат тел [noparse] — экранированным текстом.
    restore_noparse(&stage, &stash)
}

/// `[url]X[/url]` → `<a href="X">X</a>` (адрес идёт и в href, и в подпись
/// — общий `replace_tag` вставить тело дважды не умеет).
fn replace_bare_url(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(start) = find_ci(input, "[url]", cursor) {
        let body_at = start + "[url]".len();
        let Some(end) = find_ci(input, "[/url]", body_at) else {
            break;
        };
        let url = &input[body_at..end];
        out.push_str(&input[cursor..start]);
        out.push_str("<a href=\"");
        out.push_str(url);
        out.push_str("\">");
        out.push_str(url);
        out.push_str("</a>");
        cursor = end + "[/url]".len();
    }
    out.push_str(&input[cursor..]);
    out
}

/// Возврат тел `[noparse]` вместо меток `\x01NP<n>\x01`.
fn restore_noparse(input: &str, stash: &[String]) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(start) = input[cursor..].find('\u{1}').map(|r| cursor + r) {
        let rest = &input[start + 1..];
        let Some(rel) = rest.find('\u{1}') else {
            break;
        };
        let token = &rest[..rel];
        let after = start + 1 + rel + 1;
        out.push_str(&input[cursor..start]);
        if let Some(idx) = token.strip_prefix("NP").and_then(|d| d.parse::<usize>().ok()) {
            if let Some(body) = stash.get(idx) {
                out.push_str(&escape_html(body));
            }
        }
        cursor = after;
    }
    out.push_str(&input[cursor..]);
    out
}

// ── Проводной цвет Wallpaper Engine ──────────────────────────────────

/// Разбор строки «r g b [a]» (числа 0..1 через пробелы) — порт
/// `parseWireColor`. Пустой результат = строка не разобралась.
pub fn parse_wire_color(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    for part in s.split_whitespace() {
        match part.parse::<f64>() {
            Ok(v) => out.push(v),
            Err(_) => return Vec::new(),
        }
    }
    out
}

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

/// Цвет из проводного формата — порт `Util::colorFromWire`.
/// Меньше трёх чисел (в т.ч. мусор) → непрозрачный чёрный.
pub fn color_from_wire(s: &str) -> [f64; 4] {
    let nums = parse_wire_color(s);
    if nums.len() < 3 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let a = if nums.len() >= 4 { clamp01(nums[3]) } else { 1.0 };
    [clamp01(nums[0]), clamp01(nums[1]), clamp01(nums[2]), a]
}

/// Цвет в проводной формат — порт `Util::colorToWire` (4 знака после
/// запятой, как `QString::number(v, 'f', 4)`).
pub fn color_to_wire(r: f64, g: f64, b: f64, a: f64, include_alpha: bool) -> String {
    if include_alpha {
        format!("{r:.4} {g:.4} {b:.4} {a:.4}")
    } else {
        format!("{r:.4} {g:.4} {b:.4}")
    }
}

/// Есть ли в проводной строке альфа — порт `Util::colorHasAlpha`.
pub fn color_has_alpha(s: &str) -> bool {
    parse_wire_color(s).len() >= 4
}
