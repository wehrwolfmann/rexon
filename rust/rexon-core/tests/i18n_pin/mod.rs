//! Interface language for the checks: pin it and keep neighbours from changing it.
//!
//! WHY. The core's labels go through `rexon_core::i18n::tr`, and the language there is
//! state of the WHOLE process. Without pinning, a check would depend on the machine's locale
//! and on whether the package with the catalog is installed in `/usr/share/locale`; and
//! the checks of one file run in parallel, and a neighbour switching the language
//! would repaint someone else's expectations. So the language is handed out under a lock:
//! `let _lang = i18n_pin::en();` holds English until the end of the check.
//!
//! Russian is taken from the REAL `po/ru.po`, compiled by `msgfmt` (the gettext
//! package) into a temporary directory — by the same reader as the product. This way
//! a "Russian as before" check proves exactly what a person will see.
//!
//! Wiring: `mod i18n_pin;` in the check file (a directory, not a top-level
//! file — so that cargo does not take it for a separate check).

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use rexon_core::i18n;

fn lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    // A failed check must not bring down all the others with a "poisoned" lock.
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The tree's `po/ru.po`; `REXON_I18N_PIN_PO` replaces it with a draft —
/// to check your translations before they are merged into the shared catalog.
fn po_file() -> PathBuf {
    match std::env::var_os("REXON_I18N_PIN_PO") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../po/ru.po"),
    }
}

/// Directory with the compiled `ru/LC_MESSAGES/rexon.mo` — one per process.
fn locale_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("rexon-i18n-pin-{}", std::process::id()));
        let mo = i18n::catalog_file(&dir, "ru", "mo");
        std::fs::create_dir_all(mo.parent().unwrap()).expect("каталог для .mo создаётся");
        let status = std::process::Command::new("msgfmt")
            .arg("--check")
            .arg("-o")
            .arg(&mo)
            .arg(po_file())
            .status()
            .expect("msgfmt (пакет gettext) запускается");
        assert!(status.success(), "msgfmt отверг po/ru.po");
        std::env::set_var(i18n::ENV_LOCALEDIR, &dir);
        dir
    })
}

/// English (the source language) until the end of the check.
pub fn en() -> MutexGuard<'static, ()> {
    let guard = lock();
    assert_eq!(i18n::set_language(Some(i18n::SOURCE_LANGUAGE)), "en");
    guard
}

/// Russian from `po/ru.po` until the end of the check.
pub fn ru() -> MutexGuard<'static, ()> {
    let guard = lock();
    locale_dir();
    assert_eq!(i18n::set_language(Some("ru")), "ru", "русский каталог не подхватился");
    guard
}

/// The Russian catalog for one-off lookups (`msgid` → translation) without changing
/// the process language.
pub fn ru_catalog() -> &'static i18n::Catalog {
    static CAT: OnceLock<i18n::Catalog> = OnceLock::new();
    CAT.get_or_init(|| {
        let mo = i18n::catalog_file(locale_dir(), "ru", "mo");
        i18n::Catalog::load(&mo).expect("каталог читается")
    })
}

/// Text of a system file (launcher, autostart, polkit policy, helper,
/// theme metadata) without the language lines other than the source and Russian.
///
/// WHY. Such files carry ALL catalog languages at once (`system_l10n`):
/// `Name[de]=`, `<message xml:lang="de">`, `"Description[de]":`, rows of the
/// helper table `de) rx_lang=de ;;` and `de:key) rx_t=…`. The goldens were captured
/// with English and Russian; a new language in `po/LINGUAS` must not repaint
/// all of them at once (as happened with German on 01.10). The lines of new languages themselves
/// are checked by `system_l10n` — here the old ones are compared.
pub fn ru_only(text: &str) -> String {
    text.split_inclusive('\n')
        .filter(|line| match line_language(line) {
            Some(code) => code == "ru",
            None => true,
        })
        .collect()
}

/// The language code of a system file line, or `None` if the line is not a language one.
fn line_language(line: &str) -> Option<&str> {
    let code_ok = |c: &str| {
        !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '@')
    };
    let t = line.trim_start();
    // <message xml:lang="de">…
    if let Some(at) = t.find(" xml:lang=\"") {
        let rest = &t[at + " xml:lang=\"".len()..];
        return rest.split('"').next().filter(|c| code_ok(c));
    }
    // "Description[de]": …  and  Name[de]=…
    let key_part = t.strip_prefix('"').unwrap_or(t);
    if let Some(open) = key_part.find('[') {
        let key = &key_part[..open];
        if !key.is_empty() && key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
            if let Some(close) = key_part[open..].find(']') {
                let code = &key_part[open + 1..open + close];
                let after = &key_part[open + close + 1..];
                if code_ok(code) && (after.starts_with('=') || after.starts_with("\":")) {
                    return Some(code);
                }
            }
        }
    }
    // Helper table: `de) rx_lang=de ;;` and `de:key) rx_t=…`.
    if let Some((head, rest)) = t.split_once(") ") {
        if rest.starts_with("rx_lang=") || rest.starts_with("rx_t=") {
            let code = head.split(':').next().unwrap_or("");
            if code != "*" && code_ok(code) {
                return Some(code);
            }
        }
    }
    None
}
