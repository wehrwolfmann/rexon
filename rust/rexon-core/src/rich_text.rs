//! Store descriptions → safe markup for a Qt `RichText` label.
//!
//! WHY. Item descriptions come from other people's sites in whatever the
//! author typed: the KDE Store (OCS) sends HTML with Markdown written inside
//! it (`### ✨ Features:<br />- <strong>…</strong>`), GitHub READMEs are
//! Markdown, Cinnamon Spices and AUR give plain text, Steam-style authors use
//! BBCode. The card showed all of it as is: heading marks, list dashes, `**`
//! and even `<br>` stood in the text (27.11.2026, «Windows XP Professional -
//! Luna Blue and Classic Suite»). Now every description goes through ONE
//! function, [`store_description_html`]:
//!
//! * real HTML tags are kept, but only from a whitelist ([`ALLOWED`]) and only
//!   with the attributes that matter (`href` of a link, `color` of `font`);
//! * `<script>`, `<style>`, `<iframe>`, `<object>`… are removed WITH their
//!   content; any other unknown tag is shown as text (`<name>` in a README is
//!   a word, not markup — the README dialog used to lose such words);
//! * a link keeps only an `http`, `https` or `mailto` address; the ↗ mark and
//!   the «open the site in the browser?» question are added by the window's
//!   shared link path (`link_policy`, `RexonLinkText`);
//! * a picture from the network is NEVER loaded by itself: `<img>` becomes a
//!   «🖼 picture ↗» link to it (Qt would otherwise fetch it silently);
//! * BBCode tags are turned into the same HTML ([`crate::we_util`]);
//! * Markdown, line by line: headings `#…######`, lists `-`/`*`/`+`/`1.`,
//!   quotes `>`, rules `---`, fenced code ```` ``` ````; inline `**bold**`,
//!   `__bold__`, `*italic*`, `` `code` ``, `~~struck~~`, `[text](url)`;
//! * line breaks: `<br>` (also an escaped `&lt;br /&gt;`) and `\n` in text
//!   without HTML structure;
//! * code (`<pre>`, `<code>`, fenced blocks) is left exactly as written.
//!
//! The function is idempotent: its own output passes through unchanged, so it
//! can run both before the machine translation (the translator then sees
//! tags, not `**` and `###`, and translates only the text between them) and
//! on display.

use crate::i18n::tr;

/// Tags kept as they are (lower case). Everything else is either dropped
/// with its content ([`DROPPED_WITH_CONTENT`]) or shown as text.
pub const ALLOWED: &[&str] = &[
    "a", "b", "strong", "i", "em", "u", "s", "strike", "del", "code", "tt", "kbd", "pre", "br", "p", "div",
    "span", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "blockquote", "hr", "sub", "sup", "small",
    "big", "font", "center", "table", "thead", "tbody", "tr", "td", "th", "dl", "dt", "dd",
];

/// Tags removed together with everything inside them.
pub const DROPPED_WITH_CONTENT: &[&str] =
    &["script", "style", "iframe", "object", "embed", "applet", "noscript", "template", "svg", "math", "form", "select", "textarea", "head", "title"];

/// Tags that start a new line by themselves (a Markdown line ends there).
const BLOCK: &[&str] = &[
    "br", "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "blockquote", "hr", "pre", "table", "tr",
    "center", "dl", "dt", "dd",
];

/// Link schemes that may stay in a description.
const SAFE_SCHEMES: &[&str] = &["http://", "https://", "mailto:"];

/// A piece of the description: a tag (raw text, lower-case name, closing?) or text.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Tag { raw: String, name: String, close: bool },
    Text(String),
}

/// `<[a-zA-Z/!][^>]*>` — the same rule the translator uses for tags.
fn tokenize(src: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let b = src.as_bytes();
    let mut text_from = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' && i + 1 < b.len() && (b[i + 1].is_ascii_alphabetic() || b[i + 1] == b'/' || b[i + 1] == b'!') {
            if let Some(rel) = src[i..].find('>') {
                if i > text_from {
                    out.push(Tok::Text(src[text_from..i].to_string()));
                }
                let raw = &src[i..i + rel + 1];
                let inner = raw[1..raw.len() - 1].trim();
                let close = inner.starts_with('/');
                let name: String = inner
                    .trim_start_matches('/')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '!' || *c == '-')
                    .collect::<String>()
                    .to_ascii_lowercase();
                out.push(Tok::Tag { raw: raw.to_string(), name, close });
                i += rel + 1;
                text_from = i;
                continue;
            }
        }
        i += 1;
    }
    if text_from < src.len() {
        out.push(Tok::Text(src[text_from..].to_string()));
    }
    out
}

/// The value of attribute `name` in a raw tag (quoted or bare).
fn attr(raw: &str, name: &str) -> Option<String> {
    let low = raw.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = low[from..].find(name) {
        let at = from + rel;
        from = at + name.len();
        let before_ok = at > 0 && low.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = low[from..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let vstart = raw.len() - rest.len() + 1;
        let v = raw[vstart..].trim_start();
        let value = match v.chars().next() {
            Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or(""),
            _ => v.split(|c: char| c.is_ascii_whitespace() || c == '>').next().unwrap_or(""),
        };
        return Some(crate::translator::html_unescape(value.trim()));
    }
    None
}

/// Escape for a text node (`&` of existing entities stays).
fn escape_text(s: &str) -> String {
    s.replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A link address that may stay, or `None`.
pub fn safe_href(href: &str) -> Option<String> {
    let h = href.trim();
    let low = h.to_ascii_lowercase();
    SAFE_SCHEMES.iter().any(|s| low.starts_with(s)).then(|| h.to_string())
}

/// «🖼 picture ↗»-style link instead of a network picture.
fn picture_link(src: &str, alt: &str) -> String {
    let label = if alt.trim().is_empty() {
        // TRANSLATORS: a link in an item description in place of a picture that
        // Rexon doesn't download by itself.
        tr("picture")
    } else {
        alt.trim().to_string()
    };
    match safe_href(src) {
        Some(h) => format!("🖼 <a href=\"{}\">{}</a>", escape_attr(&h), escape_text(&label)),
        None => String::new(),
    }
}

/// Whitelist pass: tags as described in the module header.
fn sanitize(tokens: Vec<Tok>) -> Vec<Tok> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut skip: Option<String> = None;
    for t in tokens {
        if let Some(name) = &skip {
            if matches!(&t, Tok::Tag { name: n, close: true, .. } if n == name) {
                skip = None;
            }
            continue;
        }
        match t {
            // A lone `<`/`>` in text (`a < b`, `<3`) must not start markup.
            Tok::Text(s) => out.push(Tok::Text(escape_text(&s))),
            Tok::Tag { raw, name, close } => {
                if name.starts_with('!') {
                    continue; // comments, doctype
                }
                if DROPPED_WITH_CONTENT.contains(&name.as_str()) {
                    if !close && !raw.ends_with("/>") {
                        skip = Some(name);
                    }
                    continue;
                }
                if name == "img" {
                    let src = attr(&raw, "src").unwrap_or_default();
                    let alt = attr(&raw, "alt").unwrap_or_default();
                    out.push(Tok::Text(picture_link(&src, &alt)));
                    continue;
                }
                if !ALLOWED.contains(&name.as_str()) {
                    out.push(Tok::Text(escape_text(&raw)));
                    continue;
                }
                let clean = if close {
                    format!("</{name}>")
                } else {
                    match name.as_str() {
                        "a" => match attr(&raw, "href").and_then(|h| safe_href(&h)) {
                            Some(h) => format!("<a href=\"{}\">", escape_attr(&h)),
                            None => "<a>".to_string(),
                        },
                        "font" => match attr(&raw, "color").filter(|c| c.chars().all(|c| c.is_ascii_alphanumeric() || c == '#')) {
                            Some(c) => format!("<font color=\"{c}\">"),
                            None => "<font>".to_string(),
                        },
                        _ => format!("<{name}>"),
                    }
                };
                out.push(Tok::Tag { raw: clean, name, close });
            }
        }
    }
    out
}

// ── Markdown ───────────────────────────────────────────────────────────────

/// Inline Markdown in one text node (outside code): `` `code` `` first, so
/// nothing inside it is touched, then links, bold, struck and italics.
fn inline_md(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let code = rest.find('`').and_then(|a| rest[a + 1..].find('`').map(|rel| (a, a + 1 + rel)));
        match code {
            Some((a, b)) if b > a + 1 => {
                out.push_str(&inline_plain(&rest[..a]));
                out.push_str("<code>");
                out.push_str(&rest[a + 1..b]);
                out.push_str("</code>");
                rest = &rest[b + 1..];
            }
            _ => {
                out.push_str(&inline_plain(rest));
                return out;
            }
        }
    }
}

fn inline_plain(text: &str) -> String {
    let mut s = md_links(text);
    for (mark, open, close) in [("**", "<b>", "</b>"), ("__", "<b>", "</b>"), ("~~", "<s>", "</s>")] {
        s = md_pair(&s, mark, open, close);
    }
    md_pair(&s, "*", "<i>", "</i>")
}

/// `mark…mark` → `open…close`; the body is not empty, does not start or end
/// with a space, and stays on the line.
fn md_pair(s: &str, mark: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find(mark) {
        let body_from = a + mark.len();
        let Some(rel) = rest[body_from..].find(mark) else { break };
        let body = &rest[body_from..body_from + rel];
        let ok = !body.is_empty()
            && !body.starts_with(char::is_whitespace)
            && !body.ends_with(char::is_whitespace)
            && !body.contains('\n')
            // `2 * 3 * 4` is not italics.
            && !(mark == "*" && body.trim().chars().all(|c| c.is_ascii_digit() || c.is_whitespace()));
        if !ok {
            out.push_str(&rest[..body_from]);
            rest = &rest[body_from..];
            continue;
        }
        out.push_str(&rest[..a]);
        out.push_str(open);
        out.push_str(body);
        out.push_str(close);
        rest = &rest[body_from + rel + mark.len()..];
    }
    out.push_str(rest);
    out
}

/// `[text](url)` → a link (only a safe address; otherwise the text alone),
/// `![alt](url)` → a picture link.
fn md_links(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find('[') {
        let Some(mid_rel) = rest[a..].find("](") else { break };
        let mid = a + mid_rel;
        let label = &rest[a + 1..mid];
        let Some(end_rel) = rest[mid + 2..].find(')') else { break };
        let url = &rest[mid + 2..mid + 2 + end_rel];
        if label.contains('[') || url.contains(char::is_whitespace) || url.is_empty() {
            out.push_str(&rest[..a + 1]);
            rest = &rest[a + 1..];
            continue;
        }
        let image = a > 0 && rest.as_bytes()[a - 1] == b'!';
        out.push_str(&rest[..if image { a - 1 } else { a }]);
        if image {
            out.push_str(&picture_link(&crate::translator::html_unescape(url), label));
        } else {
            match safe_href(&crate::translator::html_unescape(url)) {
                Some(h) => out.push_str(&format!("<a href=\"{}\">{label}</a>", escape_attr(&h))),
                None => out.push_str(label),
            }
        }
        rest = &rest[mid + 2 + end_rel + 1..];
    }
    out.push_str(rest);
    out
}

/// What a Markdown line starts with.
#[derive(Debug, Clone, Copy, PartialEq)]
enum LineKind {
    Plain,
    Heading(usize),
    Bullet,
    Numbered,
    Quote,
    Rule,
    Fence,
}

/// The kind of a line by its first text, and how many bytes of the marker to cut.
fn line_kind(first: &str) -> (LineKind, usize) {
    let lead = first.len() - first.trim_start().len();
    let t = &first[lead..];
    if t.starts_with("```") || t.starts_with("~~~") {
        return (LineKind::Fence, first.len());
    }
    let hashes = t.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
        return (LineKind::Heading(hashes), lead + hashes + 1);
    }
    let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() >= 3 && (compact.chars().all(|c| c == '-') || compact.chars().all(|c| c == '*') || compact.chars().all(|c| c == '_')) {
        return (LineKind::Rule, first.len());
    }
    for m in ["- ", "* ", "+ ", "• "] {
        if t.starts_with(m) {
            return (LineKind::Bullet, lead + m.len());
        }
    }
    let digits = t.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && digits <= 3 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) {
        return (LineKind::Numbered, lead + digits + 2);
    }
    if t.starts_with("&gt; ") {
        return (LineKind::Quote, lead + 5);
    }
    if t.starts_with("> ") {
        return (LineKind::Quote, lead + 2);
    }
    (LineKind::Plain, 0)
}

/// A line: its tokens and how it ended (`<br>`/`\n` — a soft end that the
/// Markdown structure may swallow; a block tag — kept).
struct Line {
    toks: Vec<Tok>,
    /// The tag that ended the line (kept in the output), if any.
    end: Option<Tok>,
    /// Ended by `<br>` or `\n`.
    soft: bool,
}

/// Split into lines at `<br>`, block tags and (when `newlines`) `\n`.
fn split_lines(tokens: Vec<Tok>, newlines: bool) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut cur: Vec<Tok> = Vec::new();
    let mut pre = 0usize;
    for t in tokens {
        match &t {
            Tok::Tag { name, close, .. } if name == "pre" => {
                pre = if *close { pre.saturating_sub(1) } else { pre + 1 };
                lines.push(Line { toks: std::mem::take(&mut cur), end: Some(t), soft: false });
            }
            Tok::Tag { name, .. } if pre == 0 && name == "br" => {
                lines.push(Line { toks: std::mem::take(&mut cur), end: Some(t), soft: true });
            }
            Tok::Tag { name, .. } if pre == 0 && BLOCK.contains(&name.as_str()) => {
                lines.push(Line { toks: std::mem::take(&mut cur), end: Some(t), soft: false });
            }
            Tok::Text(s) if pre == 0 && newlines && s.contains('\n') => {
                let parts: Vec<&str> = s.split('\n').collect();
                for (k, part) in parts.iter().enumerate() {
                    let part = part.strip_suffix('\r').unwrap_or(part);
                    if !part.is_empty() {
                        cur.push(Tok::Text(part.to_string()));
                    }
                    if k + 1 < parts.len() {
                        let br = Tok::Tag { raw: "<br>".into(), name: "br".into(), close: false };
                        lines.push(Line { toks: std::mem::take(&mut cur), end: Some(br), soft: true });
                    }
                }
            }
            _ => cur.push(t),
        }
    }
    lines.push(Line { toks: cur, end: None, soft: false });
    lines
}

fn render(toks: &[Tok], code: &mut usize, md: bool) -> String {
    let mut out = String::new();
    for t in toks {
        match t {
            Tok::Tag { raw, name, close } => {
                if name == "code" || name == "tt" || name == "kbd" {
                    *code = if *close { code.saturating_sub(1) } else { *code + 1 };
                }
                out.push_str(raw);
            }
            Tok::Text(s) => {
                if md && *code == 0 {
                    out.push_str(&inline_md(s));
                } else {
                    out.push_str(s);
                }
            }
        }
    }
    out
}

/// Markdown line structure over sanitized tokens.
fn markdown(tokens: Vec<Tok>, newlines: bool) -> String {
    let lines = split_lines(tokens, newlines);
    let mut out = String::new();
    let mut list: Option<&'static str> = None; // "ul" | "ol"
    let mut fence = false;
    // The last thing written ends a block (a heading, a list, a paragraph…):
    // an empty soft line right after it is only spacing, not a line break.
    let mut after_block = true;
    let mut pre_depth = 0usize;
    let mut code = 0usize;
    let close_list = |out: &mut String, list: &mut Option<&'static str>| {
        if let Some(l) = list.take() {
            out.push_str(&format!("</{l}>"));
        }
    };
    for line in lines {
        let first_text = line.toks.iter().find_map(|t| match t {
            Tok::Text(s) if !s.trim().is_empty() => Some(s.clone()),
            Tok::Text(_) => None,
            Tok::Tag { .. } => Some(String::new()),
        });
        let in_pre = pre_depth > 0;
        let (kind, cut) = if in_pre {
            (LineKind::Plain, 0)
        } else {
            first_text.as_deref().map(line_kind).unwrap_or((LineKind::Plain, 0))
        };
        let end_raw = line.end.as_ref().map(|t| match t {
            Tok::Tag { raw, .. } => raw.clone(),
            Tok::Text(s) => s.clone(),
        });
        if let Some(Tok::Tag { name, close, .. }) = &line.end {
            if name == "pre" {
                pre_depth = if *close { pre_depth.saturating_sub(1) } else { pre_depth + 1 };
            }
        }
        if kind == LineKind::Fence {
            close_list(&mut out, &mut list);
            out.push_str(if fence { "</pre>" } else { "<pre>" });
            fence = !fence;
            after_block = true;
            continue;
        }
        if fence {
            out.push_str(&render(&line.toks, &mut code, false));
            if line.end.is_some() {
                out.push('\n');
            }
            continue;
        }
        // Cut the marker off the first text token.
        let mut toks = line.toks;
        if cut > 0 {
            if let Some(pos) = toks.iter().position(|t| matches!(t, Tok::Text(s) if !s.trim().is_empty())) {
                if let Tok::Text(s) = &toks[pos] {
                    let rest = s[cut.min(s.len())..].to_string();
                    toks[pos] = Tok::Text(rest);
                }
            }
        }
        let body = render(&toks, &mut code, !in_pre);
        match kind {
            LineKind::Heading(level) => {
                close_list(&mut out, &mut list);
                // One step smaller than written, as for BBCode: `#` must not crush the panel.
                let h = (level + 2).min(6);
                out.push_str(&format!("<h{h}>{}</h{h}>", body.trim()));
                if !line.soft {
                    out.push_str(&end_raw.unwrap_or_default());
                }
            }
            LineKind::Bullet | LineKind::Numbered => {
                let want = if kind == LineKind::Bullet { "ul" } else { "ol" };
                if list != Some(want) {
                    close_list(&mut out, &mut list);
                    out.push_str(&format!("<{want}>"));
                    list = Some(want);
                }
                out.push_str(&format!("<li>{}</li>", body.trim()));
                if !line.soft {
                    close_list(&mut out, &mut list);
                    out.push_str(&end_raw.unwrap_or_default());
                }
            }
            LineKind::Quote => {
                close_list(&mut out, &mut list);
                out.push_str(&format!("<blockquote>{}</blockquote>", body.trim()));
                if !line.soft {
                    out.push_str(&end_raw.unwrap_or_default());
                }
            }
            LineKind::Rule => {
                close_list(&mut out, &mut list);
                out.push_str("<hr>");
                if !line.soft {
                    out.push_str(&end_raw.unwrap_or_default());
                }
            }
            LineKind::Fence => unreachable!(),
            LineKind::Plain => {
                if list.is_some() {
                    close_list(&mut out, &mut list);
                    after_block = true;
                }
                let empty = body.trim().is_empty();
                if empty && line.soft && after_block {
                    continue;
                }
                out.push_str(&body);
                out.push_str(&end_raw.unwrap_or_default());
                after_block = (empty && !line.soft) || (!line.soft && line.end.is_some());
                continue;
            }
        }
        after_block = true;
    }
    close_list(&mut out, &mut list);
    if fence {
        out.push_str("</pre>");
    }
    out
}

/// Whether the text breaks its lines with `<br>` (then `\n` is just
/// whitespace, as a browser reads it).
fn has_breaks(tokens: &[Tok]) -> bool {
    tokens.iter().any(|t| matches!(t, Tok::Tag { name, .. } if name == "br"))
}

/// Whether the text uses BBCode tags.
fn looks_like_bbcode(s: &str) -> bool {
    let low = s.to_ascii_lowercase();
    ["[b]", "[i]", "[u]", "[url", "[list]", "[*]", "[h1]", "[h2]", "[h3]", "[img]", "[code]", "[quote", "[hr]", "[s]"]
        .iter()
        .any(|t| low.contains(t))
}

/// An escaped line break written as text (`&lt;br /&gt;`) → a real one: the
/// store sometimes double-escapes it, and the reader saw `<br>` in the text.
fn unescape_breaks(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(a) = rest.find("&lt;") {
        let tail = &rest[a + 4..];
        let low: String = tail.chars().take(12).collect::<String>().to_ascii_lowercase();
        let len = ["br&gt;", "br/&gt;", "br /&gt;"].iter().find(|m| low.starts_with(*m)).map(|m| m.len());
        out.push_str(&rest[..a]);
        match len {
            Some(n) => {
                out.push_str("<br>");
                rest = &tail[n..];
            }
            None => {
                out.push_str("&lt;");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A store item description (HTML, Markdown, BBCode, plain text or a mix)
/// → safe markup for a `RichText` label. See the module header.
pub fn store_description_html(src: &str) -> String {
    if src.trim().is_empty() {
        return String::new();
    }
    let mut s = unescape_breaks(src);
    if looks_like_bbcode(&s) {
        s = crate::we_util::bbcode_markup(&s);
    }
    let tokens = sanitize(tokenize(&s));
    // Who breaks lines with `<br>` does not mean anything by `\n`; otherwise
    // (Markdown, plain text, a README with some HTML at the top) `\n` is a line.
    let newlines = !has_breaks(&tokens);
    let html = markdown(tokens, newlines);
    crate::we_util::autolink(&html).trim().to_string()
}

/// Plain text (an extension description, a package summary) → the same safe
/// markup: escaped, with Markdown and line breaks.
pub fn plain_text_html(text: &str) -> String {
    store_description_html(&crate::translator::html_escape(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin() {
        crate::i18n::set_language(Some("en"));
    }

    /// The KDE Store sample of 27.11.2026: HTML with Markdown inside.
    #[test]
    fn ocs_html_with_markdown_renders_headings_lists_and_bold() {
        pin();
        let src = "<p>Authentic <strong>Windows XP</strong> suite!<br /><br />### ✨ Features:<br />- <strong>Start Menu (<code>nl.steelhull.xpstart</code>)</strong>: Green button.<br />- <strong>Tray</strong>: blue.<br /><br />### 🚀 1-Command Install:<br />Run it.</p>";
        let out = store_description_html(src);
        assert!(out.contains("<h5>✨ Features:</h5>"), "{out}");
        assert!(out.contains("<h5>🚀 1-Command Install:</h5>"), "{out}");
        assert!(out.contains("<ul><li><strong>Start Menu (<code>nl.steelhull.xpstart</code>)</strong>: Green button.</li><li><strong>Tray</strong>: blue.</li></ul>"), "{out}");
        assert!(!out.contains("###") && !out.contains("- <strong>"), "{out}");
        assert!(out.contains("Run it."), "{out}");
    }

    #[test]
    fn markdown_inline_bold_italic_code_links() {
        pin();
        let out = store_description_html("Some **bold**, *italic*, `x = 1`, ~~old~~ and [site](https://example.org/a).\n2 * 3 * 4");
        assert!(out.contains("<b>bold</b>") && out.contains("<i>italic</i>"), "{out}");
        assert!(out.contains("<code>x = 1</code>") && out.contains("<s>old</s>"), "{out}");
        assert!(out.contains("<a href=\"https://example.org/a\">site</a>"), "{out}");
        assert!(out.contains("<br>2 * 3 * 4"), "{out}");
    }

    #[test]
    fn plain_markdown_with_newlines_lists_and_fences() {
        pin();
        let out = store_description_html("# Title\n\n1. one\n2. two\n\nText\n```\n**not bold**\n```\n> quoted\n---");
        assert!(out.starts_with("<h3>Title</h3>"), "{out}");
        assert!(out.contains("<ol><li>one</li><li>two</li></ol>"), "{out}");
        assert!(out.contains("<pre>**not bold**\n</pre>"), "{out}");
        assert!(out.contains("<blockquote>quoted</blockquote>"), "{out}");
        assert!(out.contains("<hr>"), "{out}");
    }

    #[test]
    fn escaped_breaks_become_line_breaks() {
        pin();
        assert_eq!(store_description_html("a&lt;br&gt;b&lt;br /&gt;c &lt;b&gt;"), "a<br>b<br>c &lt;b&gt;");
    }

    /// Security: no scripts, no styles, no event attributes, no dangerous link
    /// schemes, and no network pictures loaded by themselves.
    #[test]
    fn unsafe_markup_is_removed_and_pictures_become_links() {
        pin();
        let out = store_description_html(
            "<script>alert(1)</script><style>p{}</style><p onclick=\"x()\" style=\"color:red\">Hi</p>\
             <a href=\"javascript:alert(1)\">bad</a> <a href='https://ok.example/'>ok</a>\
             <img src=\"https://cdn.example/p.png\" onerror=\"x()\"><iframe src=\"https://e\">in</iframe>",
        );
        assert!(!out.contains("script") && !out.contains("alert") && !out.contains("style"), "{out}");
        assert!(!out.contains("onclick") && !out.contains("onerror") && !out.contains("iframe"), "{out}");
        assert!(out.contains("<p>Hi</p>"), "{out}");
        assert!(out.contains("<a>bad</a>"), "{out}");
        assert!(out.contains("<a href=\"https://ok.example/\">ok</a>"), "{out}");
        assert!(!out.contains("<img"), "{out}");
        assert!(out.contains("🖼 <a href=\"https://cdn.example/p.png\">picture</a>"), "{out}");
        let md = store_description_html("![shot](https://cdn.example/s.png) [x](javascript:y)");
        assert!(md.contains("🖼 <a href=\"https://cdn.example/s.png\">shot</a>") && md.ends_with(" x"), "{md}");
    }

    /// An unknown tag is a word, not markup: `<name>` in a README stays visible.
    #[test]
    fn unknown_tags_are_shown_as_text() {
        pin();
        assert_eq!(store_description_html("Run <name> --help"), "Run &lt;name&gt; --help");
    }

    #[test]
    fn bbcode_descriptions_are_rendered_through_the_shared_passes() {
        pin();
        let out = store_description_html("[b]Big[/b] [url=https://x.example]site[/url]\n[list][*]one[*]two[/list]");
        assert!(out.contains("<b>Big</b>"), "{out}");
        assert!(out.contains("<a href=\"https://x.example\">site</a>"), "{out}");
        assert!(out.contains("<li>one"), "{out}");
    }

    #[test]
    fn code_is_left_as_written() {
        pin();
        let out = store_description_html("<pre>### not a heading\n- not a list **x**</pre><code>**y**</code>");
        assert!(out.contains("<pre>### not a heading\n- not a list **x**</pre>"), "{out}");
        assert!(out.contains("<code>**y**</code>"), "{out}");
    }

    /// The output passes through unchanged: the function runs before the
    /// translation and again on display.
    #[test]
    fn rendering_is_idempotent() {
        pin();
        for src in [
            "<p>Hi<br />### H<br />- <strong>a</strong><br />- b<br /><br />End</p>",
            "# T\n\n- a\n- b\n\nText **x** [l](https://e.example)\n```\ncode\n```",
            "Plain https://example.org text",
            "<img src=\"https://e.example/p.png\"> ok",
        ] {
            let once = store_description_html(src);
            assert_eq!(store_description_html(&once), once, "{src}");
        }
    }

    #[test]
    fn bare_addresses_become_links_and_plain_text_is_escaped() {
        pin();
        let out = store_description_html("See https://example.org/x for more");
        assert_eq!(out, "See <a href=\"https://example.org/x\">https://example.org/x</a> for more");
        assert_eq!(plain_text_html("a < b\n**c**"), "a &lt; b<br><b>c</b>");
        assert_eq!(store_description_html("  "), "");
    }
}
