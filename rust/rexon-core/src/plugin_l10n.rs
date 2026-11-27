//! Labels of waywallen daemon plugins: the plugin author's catalog on top of
//! our Russian layer.
//!
//! # Why
//!
//! Since waywallen 0.3.7 source plugins return labels not as a bare string but
//! as a `PluginMessage{plugin_id, msgid}` pair (see `control.proto`), and the
//! daemon sends the translation itself separately — as `.po` catalogs in
//! response to a `plugin_translation_list` request. The vendored QML runs EVERY
//! label through `I18n.tr()`, i.e. through this mechanism.
//!
//! We already have translations of catalog labels — [`crate::remote_l10n`],
//! the Russian tables for the Steam Workshop and Wallhaven. If we simply took
//! the upstream path, our translation would be lost: the plugin
//! `org.waywallen.open-wallpaper-engine` (Steam Workshop) has NO Russian
//! catalog at all — measured on a live 0.3.7 daemon: `image`, `video` and
//! `wallhaven` have catalogs (ru + zh-CN), while open-wallpaper-engine has
//! only zh-CN. And that is exactly where most of our labels live.
//!
//! # Order of resolving a label
//!
//! 1. **the plugin catalog** — the plugin author's translation: it is updated
//!    together with the plugin and reaches every waywallen user, not just us;
//! 2. **our [`crate::remote_l10n`]** — fills the gaps;
//! 3. **the `msgid` itself** — if nothing was found anywhere.
//!
//! Warning: item 2 comes from TWO sides, and both are the same layer of ours:
//!
//!   * `fallback` — a label that [`crate::remote_l10n::translate_source`] has
//!     already computed BY KEY (`trend_day` → «Популярные (Сегодня)», "Popular
//!     (Today)"; the `mature` filter → «18+ (для взрослых)», "18+ (adults)";
//!     the `wallpaper_engine` source → «Мастерская Steam», "Steam Workshop").
//!     `PluginMessage` has no key at all, so the bridge itself brings it here —
//!     as the `rexonFallback` field in the same map;
//!   * [`crate::remote_l10n::text`] — lookup by the English string (`msgid`),
//!     which covers the plugins' buttons and status lines.
//!
//! Without the first half, Russian would disappear from all sort orders and
//! filter headings at once: our tables for them are keyed, not text-based —
//! caught by the acceptance test `scripts/test_we_plugin_translations.py` on a
//! live daemon.
//!
//! The reverse order would mean that we silently override the plugin author's
//! translation with ours — that must not be done. As upstream catalogs appear,
//! our layer shrinks by itself, and labels do not flicker.
//!
//! Warning: `msgid` is the English source string, not a key (checked on a live
//! daemon: `labelText.msgid == "Log in to Wallhaven"`). That is why the
//! `remote_l10n` tables, whose key is also the English string, fit here without
//! a single new map (rule #12: fix the shared engine instead of starting a
//! second dictionary).
//!
//! # What is not here
//!
//! Input/output: parsing `.po` and lookup are pure functions over bytes and
//! strings. The network (the `plugin_translation_list` request), Qt properties
//! and signals are behind the bridge `rexon-gui/src/plugin_translations.rs`.

use std::collections::BTreeMap;

/// A parsed catalog of one plugin in one language: `msgid` → `msgstr`.
pub type Messages = BTreeMap<String, String>;

/// Catalogs of all plugins: `plugin_id` → `locale` → messages.
pub type Catalogs = BTreeMap<String, BTreeMap<String, Messages>>;

// ============================================================================
// Parsing .po
// ============================================================================

/// Strip the quotes from one `.po` line and expand its escapes.
///
/// Returns `None` if the line has no quoted part: such lines (comments,
/// `msgid_plural` without quotes, garbage) are simply skipped.
fn unquote(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let body = &line[start + 1..];
    let end = {
        // The closing quote is the last unescaped one.
        let bytes = body.as_bytes();
        let mut found = None;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'"' => {
                    found = Some(i);
                    i += 1;
                }
                _ => i += 1,
            }
        }
        found?
    };
    let raw = &body[..end];

    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            // An unknown pair is left as is: corrupting someone else's text is worse than
            // showing an extra backslash.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Some(out)
}

/// Which part of the entry we are currently collecting.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    None,
    Id,
    Str,
}

/// Parse a `.po` catalog into a `msgid` → `msgstr` map.
///
/// Understands line continuations, escapes and `msgstr[0]`. Skips the header
/// (the entry with an empty `msgid`), empty translations and `#` comments. A
/// broken file is not an error: everything that parses is parsed — otherwise a
/// single malformed entry would eat the plugin's whole catalog.
pub fn parse_po(po: &[u8]) -> Messages {
    let text = String::from_utf8_lossy(po);
    let mut out = Messages::new();
    let mut id = String::new();
    let mut msg = String::new();
    let mut field = Field::None;

    // Record the collected pair and start the next one.
    fn flush(out: &mut Messages, id: &mut String, msg: &mut String) {
        if !id.is_empty() && !msg.is_empty() {
            out.insert(std::mem::take(id), std::mem::take(msg));
        } else {
            id.clear();
            msg.clear();
        }
    }

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgid_plural") {
            // The daemon does not send plural forms; continuations of such an entry must
            // not end up in the msgid.
            let _ = rest;
            field = Field::None;
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgid") {
            flush(&mut out, &mut id, &mut msg);
            field = Field::Id;
            if let Some(part) = unquote(rest) {
                id.push_str(&part);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgstr") {
            // `msgstr[0]` — the first plural form, good enough as the translation; the
            // other forms are skipped.
            if rest.starts_with('[') && !rest.starts_with("[0]") {
                field = Field::None;
                continue;
            }
            field = Field::Str;
            if let Some(part) = unquote(rest) {
                msg.push_str(&part);
            }
            continue;
        }
        if line.starts_with("msgctxt") {
            // We do not distinguish contexts: the daemon has none.
            field = Field::None;
            continue;
        }
        if line.starts_with('"') {
            // Continuation of the previous field.
            if let Some(part) = unquote(line) {
                match field {
                    Field::Id => id.push_str(&part),
                    Field::Str => msg.push_str(&part),
                    Field::None => {}
                }
            }
        }
    }
    flush(&mut out, &mut id, &mut msg);
    out
}

// ============================================================================
// Locale
// ============================================================================

/// Language tags from the most specific to the most general: `ru_RU.UTF-8` →
/// `["ru-RU", "ru"]`.
///
/// The daemon labels catalogs as `ru` and `zh-CN` (with a hyphen), so the
/// underscore is normalized into a hyphen. An empty locale gives an empty list —
/// there is nothing to look a catalog up by.
pub fn locale_candidates(locale: &str) -> Vec<String> {
    let cleaned = locale
        .split(['.', '@'])
        .next()
        .unwrap_or("")
        .replace('_', "-");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return Vec::new();
    }
    let mut out = vec![cleaned.to_string()];
    if let Some((lang, _region)) = cleaned.split_once('-') {
        if !lang.is_empty() {
            out.push(lang.to_string());
        }
    }
    out
}

// ============================================================================
// Resolving a label
// ============================================================================

/// A label from the plugin catalog — item 1 of the order. `None` — not in the catalog.
pub fn catalog_lookup(
    catalogs: &Catalogs,
    plugin_id: &str,
    msgid: &str,
    locale: &str,
) -> Option<String> {
    if plugin_id.is_empty() || msgid.is_empty() {
        return None;
    }
    let by_locale = catalogs.get(plugin_id)?;
    for tag in locale_candidates(locale) {
        if let Some(found) = by_locale.get(&tag).and_then(|m| m.get(msgid)) {
            return Some(found.clone());
        }
    }
    None
}

/// A label for the buyer: plugin catalog → our `remote_l10n` → the `msgid` itself.
///
/// `fallback` is our label computed by key (see the module header); an empty
/// string means "there is no keyed translation".
///
/// An empty `msgid` is returned empty: in the protocol it means "no label", and
/// nothing may be substituted there — the vendored QML picks the fallback text
/// itself (`I18n.tr(x) || modelData.name`).
pub fn resolve(
    catalogs: &Catalogs,
    plugin_id: &str,
    msgid: &str,
    fallback: &str,
    locale: &str,
) -> String {
    resolve_with_arguments(catalogs, plugin_id, msgid, fallback, locale, &[])
}

/// The same as [`resolve`], but with `%1`, `%2`… substitutions from the protocol.
///
/// waywallen 0.3.8 added `PluginMessage.arguments`: the plugin returns a
/// template ("Signed in as %1") and the values separately, so that the
/// translator gets the template rather than a glued-together string.
/// Substitution happens AFTER translation — otherwise the catalog would be
/// searched for a string with the name already substituted.
pub fn resolve_with_arguments(
    catalogs: &Catalogs,
    plugin_id: &str,
    msgid: &str,
    fallback: &str,
    locale: &str,
    arguments: &[String],
) -> String {
    if msgid.is_empty() {
        // No label at all — the vendored QML chooses the fallback text.
        return String::new();
    }
    let text = if let Some(found) = catalog_lookup(catalogs, plugin_id, msgid, locale) {
        found
    } else if !fallback.is_empty() {
        fallback.to_string()
    } else {
        crate::remote_l10n::text(msgid).to_string()
    };
    format_message(&text, arguments)
}

/// Substitute `arguments` for `%1`, `%2`… .
///
/// The number is any run of consecutive digits after `%`; out of range or
/// without digits the place stays as it was (a plugin may have written "100%").
/// An empty list of values does not change the string at all.
pub fn format_message(text: &str, arguments: &[String]) -> String {
    if arguments.is_empty() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
        let digits: String = chars[i + 1..end].iter().collect();
        match digits.parse::<usize>() {
            Ok(n) if n >= 1 && n <= arguments.len() => out.push_str(&arguments[n - 1]),
            _ => out.extend(&chars[i..end]),
        }
        i = end;
    }
    out
}

/// Collect the documents of the daemon's response into catalogs.
///
/// The input is what came in `PluginTranslationListResponse.documents`:
/// `(plugin_id, locale, po)` triples. Parsing happens here once per response,
/// not per label: `resolve` must be a map lookup — it is called for EVERY label
/// of the page.
pub fn build_catalogs<'a, I>(documents: I) -> Catalogs
where
    I: IntoIterator<Item = (&'a str, &'a str, &'a [u8])>,
{
    let mut out = Catalogs::new();
    for (plugin_id, locale, po) in documents {
        if plugin_id.is_empty() || locale.is_empty() {
            continue;
        }
        let messages = parse_po(po);
        if messages.is_empty() {
            continue;
        }
        out.entry(plugin_id.to_string())
            .or_default()
            .insert(locale.to_string(), messages);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PO: &str = r#"
msgid ""
msgstr "Project-Id-Version: test\nLanguage: ru\n"

#: main.lua:59
msgid "Trending"
msgstr "В тренде"

#: main.lua:60
msgid "Long line"
msgstr ""
"первая часть "
"и вторая"

msgid "Escaped \"quote\""
msgstr "Кавычка \"тут\""

msgid "Empty translation"
msgstr ""
"#;

    #[test]
    fn po_разбирается_с_продолжениями_и_экранированием() {
        let m = parse_po(PO.as_bytes());
        assert_eq!(m.get("Trending").map(String::as_str), Some("В тренде"));
        assert_eq!(
            m.get("Long line").map(String::as_str),
            Some("первая часть и вторая")
        );
        assert_eq!(
            m.get("Escaped \"quote\"").map(String::as_str),
            Some("Кавычка \"тут\"")
        );
        // The header (empty msgid) and empty translations do not get into the map.
        assert!(!m.contains_key(""));
        assert!(!m.contains_key("Empty translation"));
    }

    #[test]
    fn метки_локали_от_точной_к_общей() {
        assert_eq!(locale_candidates("ru_RU.UTF-8"), vec!["ru-RU", "ru"]);
        assert_eq!(locale_candidates("zh-CN"), vec!["zh-CN", "zh"]);
        assert_eq!(locale_candidates("ru"), vec!["ru"]);
        assert!(locale_candidates("").is_empty());
    }

    /// Item 1: the plugin author's translation is stronger than ours.
    #[test]
    fn каталог_плагина_бьёт_наш_слой() {
        // Our layer speaks the language of the Rexon interface; we pin the source
        // (English) one, the Russian one is pinned by the `remote_l10n_parity` check.
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let catalogs = build_catalogs([(
            "org.waywallen.wallhaven",
            "ru",
            format!("msgid \"{API_KEY_HINT}\"\nmsgstr \"Скопируйте ключ\"\n").as_bytes(),
        )]);
        // Our tables have their own label for this string; the plugin catalog wins.
        assert_eq!(
            resolve(&catalogs, "org.waywallen.wallhaven", API_KEY_HINT, "", "ru_RU"),
            "Скопируйте ключ"
        );
        assert_eq!(crate::remote_l10n::text(API_KEY_HINT), OUR_API_KEY_HINT);
    }

    /// A plugin string that has ITS OWN label in our tables (not the same English
    /// one) — on it you can see that it was our layer that worked.
    const API_KEY_HINT: &str = "Copy the API key from Wallhaven Account Settings.";
    const OUR_API_KEY_HINT: &str = "The API key is in your Wallhaven account settings.";

    /// Item 2: without a catalog our layer holds — this is the Steam Workshop case.
    #[test]
    fn без_каталога_держит_наш_русский() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let catalogs = Catalogs::new();
        assert_eq!(
            resolve(
                &catalogs,
                "org.waywallen.wallhaven",
                API_KEY_HINT,
                "",
                "ru_RU"
            ),
            OUR_API_KEY_HINT
        );
    }

    /// Item 2 also applies when the plugin catalog exists but lacks the string.
    #[test]
    fn дыра_в_каталоге_закрывается_нашим_слоем() {
        let catalogs = build_catalogs([(
            "org.waywallen.wallhaven",
            "ru",
            "msgid \"Trending\"\nmsgstr \"В тренде\"\n".as_bytes(),
        )]);
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        assert_eq!(
            resolve(&catalogs, "org.waywallen.wallhaven", API_KEY_HINT, "", "ru"),
            OUR_API_KEY_HINT
        );
    }

    /// Item 3: an unknown string is returned as is, not empty.
    #[test]
    fn незнакомое_остаётся_собой() {
        let catalogs = Catalogs::new();
        assert_eq!(
            resolve(&catalogs, "org.waywallen.image", "Brand new label", "", "ru"),
            "Brand new label"
        );
        assert_eq!(resolve(&catalogs, "", "", "", "ru"), "");
    }

    /// A foreign language is not substituted: the Workshop has only zh-CN — and it
    /// must NOT surface on a Russian machine.
    #[test]
    fn чужой_язык_не_подставляется() {
        let catalogs = build_catalogs([(
            "org.waywallen.open-wallpaper-engine",
            "zh-CN",
            "msgid \"Content rating\"\nmsgstr \"内容分级\"\n".as_bytes(),
        )]);
        assert_eq!(
            resolve(
                &catalogs,
                "org.waywallen.open-wallpaper-engine",
                "Content rating",
                "",
                "ru_RU"
            ),
            "Content rating"
        );
    }

    /// The keyed translation of our layer (sort orders, filter headings) is item
    /// 2, not item 1: the plugin catalog overrides it.
    #[test]
    fn ключевой_перевод_ниже_каталога_но_выше_msgid() {
        let catalogs = build_catalogs([(
            "org.waywallen.wallhaven",
            "ru",
            "msgid \"Trending\"\nmsgstr \"В тренде\"\n".as_bytes(),
        )]);
        // There is a catalog — it wins, our keyed translation gives way.
        assert_eq!(
            resolve(
                &catalogs,
                "org.waywallen.wallhaven",
                "Trending",
                "Наш вариант",
                "ru"
            ),
            "В тренде"
        );
        // No catalog — the keyed translation holds.
        assert_eq!(
            resolve(
                &catalogs,
                "org.waywallen.open-wallpaper-engine",
                "Trending today",
                "Популярные (Сегодня)",
                "ru"
            ),
            "Популярные (Сегодня)"
        );
    }

    /// A broken entry does not eat the rest of the catalog.
    #[test]
    fn битая_запись_не_рушит_каталог() {
        let po = "msgid \"good\"\nmsgstr \"ок\"\nmsgid broken\nmsgstr also broken\nmsgid \"tail\"\nmsgstr \"хвост\"\n"
            .as_bytes();
        let m = parse_po(po);
        assert_eq!(m.get("good").map(String::as_str), Some("ок"));
        assert_eq!(m.get("tail").map(String::as_str), Some("хвост"));
    }
}
