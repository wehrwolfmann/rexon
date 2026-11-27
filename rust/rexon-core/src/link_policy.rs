//! Links from foreign text: descriptions, comments, package information.
//!
//! WHY. On 16.11.2026 a link from a wallpaper author in a description silently
//! took the person to the browser, to a paid ko-fi theme. Wehrwolfmann's
//! decision of the same day:
//!
//! 1. a link to a product of a store that Rexon can show as a card (KDE Store
//!    and the Pling sites, AUR, Wallhaven, Cinnamon Spices, GNOME Extensions)
//!    opens the CARD inside Rexon, without a browser;
//! 2. every other external link is marked with ↗ and, before opening, asks
//!    "Open the external site <domain> in the browser?" with a "Don't ask again
//!    for this site" checkbox (per domain, stored in the Rexon settings, reset
//!    in the "Settings" tab).
//!
//! There is deliberately no built-in browser: payments and account logins in
//! our window are a risk that the person's own browser does not have.
//!
//! This module holds the address parsing, the decision and the texts. The
//! question dialog is the shared component
//! `RexonLinkText.qml`/`RexonLinkDialog.qml`, the bridge is
//! `rexon-gui::link_guard`.

use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::i18n::{tr, trf};

/// `settings.json` key: the sites for which the person chose "Don't ask again".
pub const TRUSTED_KEY: &str = "trusted_link_sites";

/// Sites of the Pling network: they share one catalog and the same product
/// numbers (`/p/<number>`), and Rexon reads it through the KDE Store API.
const OCS_HOSTS: &[&str] = &[
    "store.kde.org",
    "pling.com",
    "opendesktop.org",
    "kde-look.org",
    "gnome-look.org",
    "xfce-look.org",
    "cinnamon-look.org",
    "box-look.org",
];

/// The store product a link points to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum StoreRef {
    /// KDE Store / Pling: product number.
    Ocs { id: String },
    /// AUR package: name.
    Aur { name: String },
    /// Wallhaven picture: id.
    Wallhaven { id: String },
    /// Cinnamon spice: type (`applet`…) and number on the site (for themes, the name).
    Spices { kind: String, spices_id: String },
    /// GNOME extension: number on the site (`/extension/<pk>/…`), not the uuid.
    Ego { pk: u64 },
}

/// What to do with a clicked link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkAction {
    /// `rexon:…` — an action of the page itself (the translation toggle).
    Page,
    /// A product card inside Rexon.
    Card(StoreRef),
    /// Open right away: the site is already trusted.
    Open,
    /// Ask. `site` — the domain for "Don't ask again"; empty — the link does not
    /// point to a site (a file, mail), and it cannot be remembered.
    Ask { site: String },
    /// Do not open at all (unknown scheme: `javascript:`, `data:`…).
    Refuse,
}

/// Address parts: (scheme, host in lower case, path, query). The host is without
/// login and port: `https://store.kde.org@evil.example/` leads to
/// `evil.example`, and that is the one that must be named to the person.
fn parts(url: &str) -> (String, String, String, String, bool) {
    let (scheme, netloc, path, query, _) = crate::ocs::urlsplit(url.trim());
    let has_login = netloc.contains('@');
    let host = netloc.rsplit('@').next().unwrap_or("");
    let host = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("").to_string()
    } else {
        host.split(':').next().unwrap_or("").to_string()
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    (scheme, host, path, query, has_login)
}

/// Domain for the question and for the trusted list: the host without `www.`.
pub fn site_of(url: &str) -> Option<String> {
    let (scheme, host, ..) = parts(url);
    if !matches!(scheme.as_str(), "http" | "https") || host.is_empty() {
        return None;
    }
    Some(host.strip_prefix("www.").unwrap_or(&host).to_string())
}

/// The host is `domain` itself or a subdomain of it.
fn host_is(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|h| h.ends_with('.'))
}

/// Path segments without empty ones.
fn segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

/// Value of a query parameter.
fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.len() <= 12 && s.bytes().all(|b| b.is_ascii_digit())
}

/// Arch package name: lower-case letters, digits and `@._+-`, not starting with
/// a hyphen or a dot.
fn valid_pkg_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && !s.starts_with(['-', '.'])
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"@._+-".contains(&b))
}

/// A link to a store product that Rexon shows as a card.
pub fn store_ref(url: &str) -> Option<StoreRef> {
    let (scheme, host, path, query, has_login) = parts(url);
    if !matches!(scheme.as_str(), "http" | "https") || has_login {
        return None;
    }
    let seg = segments(&path);
    if OCS_HOSTS.iter().any(|d| host_is(&host, d)) {
        // `/p/1234567`, `/p/1234567/`, `/p/1234567#files-panel`.
        if let ["p", id, ..] = seg.as_slice() {
            return digits(id).then(|| StoreRef::Ocs { id: id.to_string() });
        }
        // Old KDE-Look addresses: `/content/show.php?content=12345`.
        if seg.first() == Some(&"content") && seg.get(1).is_some_and(|s| s.starts_with("show.php")) {
            return query_param(&query, "content")
                .filter(|id| digits(id))
                .map(|id| StoreRef::Ocs { id: id.to_string() });
        }
        return None;
    }
    if host == "aur.archlinux.org" {
        if let ["packages", name] = seg.as_slice() {
            return valid_pkg_name(name).then(|| StoreRef::Aur { name: name.to_string() });
        }
        return None;
    }
    if host_is(&host, "wallhaven.cc") {
        if let ["w", id] = seg.as_slice() {
            return crate::wallhaven::valid_id(id).then(|| StoreRef::Wallhaven { id: id.to_string() });
        }
        return None;
    }
    if host == "whvn.cc" {
        if let [id] = seg.as_slice() {
            return crate::wallhaven::valid_id(id).then(|| StoreRef::Wallhaven { id: id.to_string() });
        }
        return None;
    }
    if host == "cinnamon-spices.linuxmint.com" {
        if let [json, "view", id] = seg.as_slice() {
            let t = crate::cinnamon_spices::TYPES.iter().find(|t| t.json == *json)?;
            let ok = if t.is_theme() {
                id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            } else {
                digits(id)
            };
            return ok.then(|| StoreRef::Spices { kind: t.key.to_string(), spices_id: id.to_string() });
        }
        return None;
    }
    if host == "extensions.gnome.org" {
        if let ["extension", pk, ..] = seg.as_slice() {
            return pk.parse::<u64>().ok().filter(|&n| n > 0 && digits(pk)).map(|pk| StoreRef::Ego { pk });
        }
    }
    None
}

/// Decision for a clicked link.
pub fn decide(url: &str, trusted: &[String]) -> LinkAction {
    let url = url.trim();
    let (scheme, host, ..) = parts(url);
    match scheme.as_str() {
        "rexon" => LinkAction::Page,
        "http" | "https" => {
            if host.is_empty() {
                return LinkAction::Refuse;
            }
            if let Some(r) = store_ref(url) {
                return LinkAction::Card(r);
            }
            let site = site_of(url).unwrap_or_default();
            if trusted.iter().any(|t| t == &site) {
                LinkAction::Open
            } else {
                LinkAction::Ask { site }
            }
        }
        // Files and mail also go outside, but they cannot be remembered "per site".
        "file" | "mailto" => LinkAction::Ask { site: String::new() },
        _ => LinkAction::Refuse,
    }
}

/// Whether a link leads outside (such links are marked with ↗).
pub fn is_external(url: &str) -> bool {
    matches!(decide(url, &[]), LinkAction::Ask { .. } | LinkAction::Open)
}

// ── The ↗ mark ─────────────────────────────────────────────────────────────

/// The external-link sign.
pub const EXTERNAL_MARK: &str = "↗";

/// Value of `href` from the opening `<a …>` tag.
fn href_of(tag: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find("href") {
        let at = from + rel;
        from = at + 4;
        // `href` is a separate attribute name, not the tail of `data-href`.
        if at > 0 && !lower.as_bytes()[at - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = tag[at + 4..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
            _ => rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        };
        return Some(crate::translator::html_unescape(value));
    }
    None
}

/// Put ↗ at the end of every external link (inside `<a>`, so that the sign has
/// the same color and is clickable together with the link). Links to our own
/// cards and `rexon:` are not marked; a repeated pass adds nothing.
pub fn mark_external(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() + 16);
    let mut pos = 0;
    while let Some(rel) = lower[pos..].find("<a") {
        let start = pos + rel;
        let after = lower.as_bytes().get(start + 2).copied();
        if !matches!(after, Some(b) if b.is_ascii_whitespace()) {
            out.push_str(&html[pos..start + 2]);
            pos = start + 2;
            continue;
        }
        let Some(open_end) = lower[start..].find('>').map(|i| start + i + 1) else { break };
        let Some(close) = lower[open_end..].find("</a").map(|i| open_end + i) else { break };
        out.push_str(&html[pos..close]);
        let external = href_of(&html[start..open_end]).is_some_and(|h| is_external(&h));
        let inner = html[open_end..close].trim_end();
        if external && !inner.ends_with(EXTERNAL_MARK) {
            out.push_str("&nbsp;");
            out.push_str(EXTERNAL_MARK);
        }
        pos = close;
    }
    out.push_str(&html[pos..]);
    out
}

// ── Trusted sites ──────────────────────────────────────────────────────────

/// Sites "without a question" from the settings.
pub fn trusted_from(settings: &Map<String, Value>) -> Vec<String> {
    let mut out: Vec<String> = settings
        .get(TRUSTED_KEY)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    out.sort();
    out.dedup();
    out
}

/// The list with the site added (alphabetical, without duplicates).
pub fn with_site(list: &[String], site: &str) -> Vec<String> {
    let mut out = list.to_vec();
    let site = site.trim().to_ascii_lowercase();
    if !site.is_empty() {
        out.push(site);
    }
    out.sort();
    out.dedup();
    out
}

/// Trusted sites from the settings file.
pub fn trusted_in(config_file: &Path) -> Vec<String> {
    trusted_from(&crate::settings::load_settings(config_file))
}

/// Trusted sites of the live machine.
pub fn live_trusted() -> Vec<String> {
    trusted_in(&crate::settings::settings_file())
}

/// Remember a site as "without a question".
pub fn live_remember(site: &str) -> std::io::Result<()> {
    let mut m = crate::settings::SettingsManager::new()?;
    let list = with_site(&trusted_from(&m.settings), site);
    m.set(TRUSTED_KEY, Value::from(list))
}

/// Ask again for all sites. → how many there were.
pub fn live_forget_all() -> std::io::Result<usize> {
    let mut m = crate::settings::SettingsManager::new()?;
    let n = trusted_from(&m.settings).len();
    m.set(TRUSTED_KEY, Value::Array(Vec::new()))?;
    Ok(n)
}

// ── Texts ──────────────────────────────────────────────────────────────────

/// Short form of the address in the question: without the scheme and no longer
/// than 90 characters.
pub fn short_url(url: &str) -> String {
    let u = url.trim();
    let u = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://")).unwrap_or(u);
    if u.chars().count() > 90 {
        format!("{}…", u.chars().take(89).collect::<String>())
    } else {
        u.to_string()
    }
}

/// Title of the question dialog.
pub fn ask_title() -> String {
    // TRANSLATORS: title of the question shown before a link from a store description opens in the browser.
    tr("External website")
}

/// The question before opening. `site` empty — the link does not point to a site.
pub fn ask_text(site: &str, url: &str) -> String {
    if site.is_empty() {
        // TRANSLATORS: %1 is a link that is not a website (a file or an e-mail address).
        trf("Open “%1” in another program?", &[&short_url(url)])
    } else {
        // TRANSLATORS: %1 is a domain name, e.g. ko-fi.com.
        trf("Open the external website %1 in your browser?", &[&site])
    }
}

/// Explanation under the question.
pub fn ask_hint() -> String {
    tr("The link leads outside Rexon. The page will open in your browser, not in Rexon.")
}

/// The "Don't ask again" checkbox.
pub fn remember_label() -> String {
    tr("Don't ask again for this website")
}

/// The "Open" button.
pub fn open_label(site: &str) -> String {
    if site.is_empty() {
        tr("Open")
    } else {
        tr("Open in Browser")
    }
}

/// The "Cancel" button.
pub fn cancel_label() -> String {
    tr("Cancel")
}

/// Status line: the link was not opened.
pub fn refused_status(url: &str) -> String {
    // TRANSLATORS: %1 is a link with an unsupported scheme (javascript:, data:…).
    trf("⚠️ Rexon does not open links like this: %1", &[&short_url(url)])
}

/// Status line: the card was not found.
pub fn card_failed_status() -> String {
    tr("⚠️ Could not open this item in Rexon")
}

// ── The "Settings" section ─────────────────────────────────────────────────

/// Line with the list of trusted sites for "Settings".
pub fn trusted_summary(list: &[String]) -> String {
    if list.is_empty() {
        tr("No websites yet: Rexon asks before every external link.")
    } else {
        // TRANSLATORS: %1 is a comma-separated list of domain names.
        trf("Opened without asking: %1", &[&list.join(", ")])
    }
}

/// Result of the reset.
pub fn forget_status(result: &std::io::Result<usize>) -> String {
    match result {
        Ok(_) => tr("🔗 Rexon will ask again before opening any external website"),
        // TRANSLATORS: %1 is the reason the settings file could not be written.
        Err(e) => trf("⚠️ Could not save the settings: %1", &[&e]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn store_links_become_cards() {
        let ocs = |id: &str| Some(StoreRef::Ocs { id: id.into() });
        assert_eq!(store_ref("https://store.kde.org/p/1294174"), ocs("1294174"));
        assert_eq!(store_ref("https://store.kde.org/p/1294174/"), ocs("1294174"));
        assert_eq!(store_ref("https://www.pling.com/p/2145021#files-panel"), ocs("2145021"));
        assert_eq!(store_ref("http://www.gnome-look.org/p/1253385"), ocs("1253385"));
        assert_eq!(store_ref("https://www.opendesktop.org/p/1132420/"), ocs("1132420"));
        assert_eq!(store_ref("https://kde-look.org/content/show.php/?content=12345"), ocs("12345"));
        assert_eq!(store_ref("https://STORE.KDE.ORG/p/5"), ocs("5"));
        assert_eq!(
            store_ref("https://aur.archlinux.org/packages/sweet-kde-git"),
            Some(StoreRef::Aur { name: "sweet-kde-git".into() })
        );
        assert_eq!(
            store_ref("https://wallhaven.cc/w/94x38z"),
            Some(StoreRef::Wallhaven { id: "94x38z".into() })
        );
        assert_eq!(store_ref("https://whvn.cc/94x38z"), Some(StoreRef::Wallhaven { id: "94x38z".into() }));
        assert_eq!(
            store_ref("https://cinnamon-spices.linuxmint.com/applets/view/17"),
            Some(StoreRef::Spices { kind: "applet".into(), spices_id: "17".into() })
        );
        assert_eq!(
            store_ref("https://cinnamon-spices.linuxmint.com/themes/view/Adapta"),
            Some(StoreRef::Spices { kind: "theme".into(), spices_id: "Adapta".into() })
        );
        assert_eq!(
            store_ref("https://extensions.gnome.org/extension/615/appindicator-support/"),
            Some(StoreRef::Ego { pk: 615 })
        );
    }

    #[test]
    fn lookalikes_are_not_store_links() {
        for url in [
            // A login in the address: in reality this is evil.example.
            "https://store.kde.org@evil.example/p/1",
            "https://store.kde.org.evil.example/p/1",
            "https://evilstore.kde.org.example/p/1",
            "https://notpling.com/p/1",
            "https://store.kde.org/browse/",
            "https://store.kde.org/p/abc",
            "https://aur.archlinux.org/packages/",
            "https://aur.archlinux.org/packages/Bad%20Name",
            "https://aur.archlinux.org/packages/x/git",
            "https://wallhaven.cc/search?q=x",
            "https://cinnamon-spices.linuxmint.com/applets",
            "https://cinnamon-spices.linuxmint.com/unknown/view/1",
            "https://extensions.gnome.org/extension/0/x/",
            "https://extensions.gnome.org/about/",
            "ftp://store.kde.org/p/1",
        ] {
            assert_eq!(store_ref(url), None, "{url}");
        }
        // A Pling subdomain is one of ours.
        assert!(store_ref("https://www.pling.com/p/1").is_some());
    }

    #[test]
    fn decision_per_kind_of_link() {
        assert_eq!(decide("rexon:toggle", &[]), LinkAction::Page);
        assert!(matches!(decide("https://store.kde.org/p/1", &[]), LinkAction::Card(_)));
        assert_eq!(
            decide("https://ko-fi.com/s/abc", &[]),
            LinkAction::Ask { site: "ko-fi.com".into() }
        );
        assert_eq!(
            decide("https://www.GitHub.com/x/y", &[]),
            LinkAction::Ask { site: "github.com".into() }
        );
        assert_eq!(decide("https://github.com/x", &s(&["github.com"])), LinkAction::Open);
        // Trust in a site does not extend to a subdomain or a neighbor.
        assert_eq!(
            decide("https://gist.github.com/x", &s(&["github.com"])),
            LinkAction::Ask { site: "gist.github.com".into() }
        );
        // A login in the address does not fool the question: the real host is named.
        assert_eq!(
            decide("https://github.com@evil.example/", &s(&["github.com"])),
            LinkAction::Ask { site: "evil.example".into() }
        );
        assert_eq!(decide("file:///home/x/a.txt", &[]), LinkAction::Ask { site: String::new() });
        assert_eq!(decide("mailto:a@b.c", &[]), LinkAction::Ask { site: String::new() });
        assert_eq!(decide("javascript:alert(1)", &[]), LinkAction::Refuse);
        assert_eq!(decide("data:text/html,x", &[]), LinkAction::Refuse);
        assert_eq!(decide("https://", &[]), LinkAction::Refuse);
        assert_eq!(decide("steam://run/1", &[]), LinkAction::Refuse);
    }

    #[test]
    fn external_links_get_the_mark_once() {
        let html = "see <a href=\"https://ko-fi.com/x\">shop</a>, \
                    <a href='https://store.kde.org/p/1'>theme</a> and \
                    <a href='rexon:toggle' style='x'>orig</a> <abbr>x</abbr>";
        let marked = mark_external(html);
        assert_eq!(
            marked,
            "see <a href=\"https://ko-fi.com/x\">shop&nbsp;↗</a>, \
             <a href='https://store.kde.org/p/1'>theme</a> and \
             <a href='rexon:toggle' style='x'>orig</a> <abbr>x</abbr>"
        );
        assert_eq!(mark_external(&marked), marked);
        // Upper-case tags, the attribute not first, `data-href` is not an address.
        assert_eq!(
            mark_external("<A data-href='rexon:x' class=l HREF=https://a.example/>a</A>"),
            "<A data-href='rexon:x' class=l HREF=https://a.example/>a&nbsp;↗</A>"
        );
        // Broken markup does not lose text.
        assert_eq!(mark_external("x <a href='https://a.example'>y"), "x <a href='https://a.example'>y");
        assert_eq!(mark_external("без ссылок"), "без ссылок");
    }

    #[test]
    fn trusted_sites_are_sorted_and_unique() {
        let list = with_site(&s(&["github.com"]), " Ko-Fi.com ");
        assert_eq!(list, s(&["github.com", "ko-fi.com"]));
        assert_eq!(with_site(&list, "github.com"), list);
        let mut m = Map::new();
        m.insert(TRUSTED_KEY.into(), serde_json::json!(["b.example", "a.example", "b.example", 5]));
        assert_eq!(trusted_from(&m), s(&["a.example", "b.example"]));
        assert!(trusted_from(&Map::new()).is_empty());
    }

    #[test]
    fn trusted_sites_survive_a_settings_file() {
        let dir = std::env::temp_dir().join(format!("rexon-link-policy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        std::fs::write(&file, r#"{"theme":"auto","trusted_link_sites":["github.com"]}"#).unwrap();
        assert_eq!(trusted_in(&file), s(&["github.com"]));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn question_names_the_site() {
        assert!(ask_text("ko-fi.com", "https://ko-fi.com/x").contains("ko-fi.com"));
        assert!(ask_text("", "file:///tmp/a").contains("file:///tmp/a"));
        assert_eq!(short_url("https://a.example/x"), "a.example/x");
        assert!(short_url(&format!("https://a.example/{}", "x".repeat(200))).ends_with('…'));
        assert!(trusted_summary(&s(&["a.example", "b.example"])).contains("a.example, b.example"));
    }
}
