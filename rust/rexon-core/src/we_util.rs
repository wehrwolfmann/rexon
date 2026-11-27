//! Utilities of the `Util` QML singleton of the Wallpaper Engine tab — port of
//! waywallen's `ui/src/util.cpp` (MIT, (c) hypengw; attribution in NOTICE).
//!
//! The singleton gives the vendored QML what is awkward to write inline in
//! JavaScript: detecting the desktop from `XDG_CURRENT_DESKTOP`, converting
//! Steam Workshop descriptions (BBCode) into an HTML subset for
//! `Qt.StyledText` and parsing/building a color in the Wallpaper Engine wire
//! format. Qt is not needed here: input and output are strings and numbers,
//! so all the logic lives in the core, and the Python shim
//! (`qml_ui/bridge.py`) stays thin.
//!
//! ⚠️ Our former Python bridge was a STUB and lied in three places — fixed
//! here (rule #16 «fix bugs during the port»):
//!   * `bbcodeToHtml` returned the text as is (wallpaper descriptions in the
//!     details panel were shown as raw BBCode);
//!   * `colorFromWire`/`colorToWire` treated the string as a Qt color NAME
//!     (`#rrggbb`), whereas the WE wire format is «r g b [a]», numbers 0..1
//!     separated by spaces. Because of that the wallpaper user property
//!     palette read the foreign value as an invalid color and wrote back a hex
//!     the daemon does not understand;
//!   * `colorHasAlpha` counted hex digits instead of numbers.
//!
//! ⚠️ A bug of upstream ITSELF is fixed too (a report candidate): their
//! auto-links (`Util::bbcodeToHtml`, the last pass) go over the whole text with
//! one regex `(href=")|(https?://…)` and do not skip an already built anchor —
//! any link from `[url]`/`[url=]` expanded into nested garbage like
//! `<a href="<a href="http://x">http://x">…</a></a>` (checked by simulating
//! their regex). Here auto-linking does not go inside markup: `<a …>…</a>`
//! and any tags are carried over as is.
//!
//! There are no regular expressions on purpose (the project has no `regex`
//! dependency, and the order of passes matters more than brevity) — parsing is
//! character by character, with the same grammar; the order of passes is 1:1
//! with the original, otherwise a `\n` inside a tag body would have turned
//! into `<br>` before the tag itself was parsed.

// ── Session desktop ──────────────────────────────────────────────────

/// `Desktop::Unknown` — port of the `Util::Desktop` enum.
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

/// The desktop from `XDG_CURRENT_DESKTOP` — port of `Util::desktop`.
///
/// The FIRST known token of the list wins (`KDE:wayland` → Kde), empty tokens
/// are skipped, case does not matter — the same semantics as
/// `display::spawner::detect_de` on the daemon side.
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

/// Whether the compositor can rename outputs — port of
/// `Util::supportsDisplayRename` (Hyprland/Sway/Niri).
pub fn supports_display_rename(desktop: i32) -> bool {
    matches!(desktop, DESKTOP_HYPRLAND | DESKTOP_SWAY | DESKTOP_NIRI)
}

// ── Display label ────────────────────────────────────────────────────

/// Display label — port of `Display::displayLabel` (`ui/src/objmodel/
/// display.cppm`, waywallen 0.4.4): the alias, without it the output name,
/// and ALWAYS the display number in parentheses — «HEADLESS-2 (#1)». The
/// number is not there to tell namesakes apart: the author appends it
/// unconditionally. Without a name and an alias — «Display #N»; the author
/// builds this string in C++ without `tr()`, so ours is not translated either.
pub fn display_label(alias: &str, name: &str, id: u64) -> String {
    let base = if alias.is_empty() { name } else { alias };
    if base.is_empty() {
        format!("Display #{id}")
    } else {
        format!("{base} (#{id})")
    }
}

// ── BBCode → HTML subset for Qt.StyledText ───────────────────────────

/// Escaping `&`, `<`, `>` — port of `escapeHtml`.
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

/// Position of an ASCII substring, case-insensitive, starting at `from` (a
/// byte index; the patterns are ASCII → the index is always on a char boundary).
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

/// `[tag]…[/tag]` → `open…close` for all non-overlapping pairs left to right —
/// port of `replaceTag` (non-greedy `(.*?)` = the nearest closing tag).
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

/// Case-insensitive literal replacement (`[hr]`, `[list]`, `[/list]`).
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

/// `[quote]`/`[quote=who]` … `[/quote]` → `<i>“…”</i>` (the author is not
/// shown — StyledText has nowhere to put it, port of the original).
fn replace_quote(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(start) = find_ci(input, "[quote", cursor) {
        // after «[quote» either «]» or «=anything-without-]» is allowed.
        let rest = &input[start + 6..];
        let Some(close_bracket) = rest.find(']') else {
            break;
        };
        let attr = &rest[..close_bracket];
        if !attr.is_empty() && !attr.starts_with('=') {
            // «[quoted…» is not our tag, move on.
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

/// `[*]` + the following whitespace → `<li>` (`</ul>` takes care of closing
/// the item, as in the original).
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

/// `[url=X]Y[/url]` → `<a href="X">Y</a>` (quotes around X are optional).
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
        // A quote around the address is an optional pair (as in the original).
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

/// Wrapping bare `http(s)://…` into links.
///
/// ⚠️ Differs from the original (a fix of its bug): the pass does NOT go inside
/// already built markup. Any `<…>` tag is carried over as is, and after
/// `<a …>` the text up to `</a>` is not touched — otherwise the address inside
/// `href="…"` and the link label were wrapped again.
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
                // the anchor body and its closing tag — without auto-links
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

/// Steam Workshop description (BBCode) → `Qt.StyledText` HTML subset — port
/// of `Util::bbcodeToHtml` (the order of passes is kept).
pub fn bbcode_to_html(src: &str) -> String {
    if src.is_empty() {
        return String::new();
    }

    // 1. [noparse] bodies are hidden behind opaque markers so the following
    //    passes do not touch them; they come back escaped at the very end.
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

    // 2. Escaping: the description author must not sneak in raw HTML.
    stage = escape_html(&stage);

    // 3. Inline markup.
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

    // 4. Quotes.
    stage = replace_quote(&stage);

    // 5. Headings — one step smaller, so that [h1] does not crush the panel.
    stage = replace_tag(&stage, "h1", "<h2>", "</h2>");
    stage = replace_tag(&stage, "h2", "<h3>", "</h3>");
    stage = replace_tag(&stage, "h3", "<h4>", "</h4>");

    // 6. Rules and lists.
    stage = replace_ci(&stage, "[hr]", "<hr>");
    stage = replace_ci(&stage, "[list]", "<ul>");
    stage = replace_ci(&stage, "[/list]", "</ul>");
    stage = replace_list_items(&stage);

    // 7. Links and images.
    stage = replace_url_with_target(&stage);
    stage = replace_bare_url(&stage);
    stage = replace_tag(&stage, "img", "<img src=\"", "\">");

    // 8. Bare addresses (with the fix: we do not go inside markup).
    stage = autolink(&stage);

    // 9. Line breaks — last, so that a `\n` inside tag bodies survives until
    //    they are parsed.
    stage = stage.replace("\r\n", "\n").replace('\n', "<br>");

    // 10. Returning [noparse] bodies — as escaped text.
    restore_noparse(&stage, &stash)
}

/// `[url]X[/url]` → `<a href="X">X</a>` (the address goes both into href and
/// the label — the shared `replace_tag` cannot insert the body twice).
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

/// Returning `[noparse]` bodies in place of the `\x01NP<n>\x01` markers.
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

// ── Wallpaper Engine wire color ──────────────────────────────────────

/// Parsing the «r g b [a]» string (numbers 0..1 separated by spaces) — port
/// of `parseWireColor`. An empty result = the string did not parse.
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

/// A color from the wire format — port of `Util::colorFromWire`.
/// Fewer than three numbers (including garbage) → opaque black.
pub fn color_from_wire(s: &str) -> [f64; 4] {
    let nums = parse_wire_color(s);
    if nums.len() < 3 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let a = if nums.len() >= 4 { clamp01(nums[3]) } else { 1.0 };
    [clamp01(nums[0]), clamp01(nums[1]), clamp01(nums[2]), a]
}

/// A color into the wire format — port of `Util::colorToWire` (4 digits after
/// the decimal point, like `QString::number(v, 'f', 4)`).
pub fn color_to_wire(r: f64, g: f64, b: f64, a: f64, include_alpha: bool) -> String {
    if include_alpha {
        format!("{r:.4} {g:.4} {b:.4} {a:.4}")
    } else {
        format!("{r:.4} {g:.4} {b:.4}")
    }
}

/// Whether the wire string has alpha — port of `Util::colorHasAlpha`.
pub fn color_has_alpha(s: &str) -> bool {
    parse_wire_color(s).len() >= 4
}
