//! Язык интерфейса для сверок: закрепить и не дать соседям его сменить.
//!
//! ЗАЧЕМ. Подписи ядра проходят через `rexon_core::i18n::tr`, а язык там —
//! состояние ВСЕГО процесса. Без закрепления сверка зависела бы от локали
//! машины и от того, стоит ли пакет с каталогом в `/usr/share/locale`; а
//! сверки одного файла идут параллельно, и соседняя, переключив язык,
//! перекрасила бы чужие ожидания. Поэтому язык выдаётся под замком:
//! `let _lang = i18n_pin::en();` держит английский до конца сверки.
//!
//! Русский берётся из НАСТОЯЩЕГО `po/ru.po`, собранного `msgfmt` (пакет
//! gettext) во временный каталог, — тем же читателем, что и продукт. Так
//! сверка «по-русски как было» доказывает ровно то, что увидит человек.
//!
//! Подключение: `mod i18n_pin;` в файле сверки (каталог, а не файл верхнего
//! уровня — чтобы cargo не счёл его отдельной сверкой).

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use rexon_core::i18n;

fn lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    // Упавшая сверка не должна валить все остальные «отравленным» замком.
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// `po/ru.po` дерева; `REXON_I18N_PIN_PO` подменяет его черновиком —
/// проверить свои переводы до того, как они влиты в общий каталог.
fn po_file() -> PathBuf {
    match std::env::var_os("REXON_I18N_PIN_PO") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../po/ru.po"),
    }
}

/// Каталог с собранным `ru/LC_MESSAGES/rexon.mo` — один на процесс.
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

/// Английский (исходный язык) до конца сверки.
pub fn en() -> MutexGuard<'static, ()> {
    let guard = lock();
    assert_eq!(i18n::set_language(Some(i18n::SOURCE_LANGUAGE)), "en");
    guard
}

/// Русский из `po/ru.po` до конца сверки.
pub fn ru() -> MutexGuard<'static, ()> {
    let guard = lock();
    locale_dir();
    assert_eq!(i18n::set_language(Some("ru")), "ru", "русский каталог не подхватился");
    guard
}

/// Русский каталог для поштучных справок (`msgid` → перевод) без смены
/// языка процесса.
pub fn ru_catalog() -> &'static i18n::Catalog {
    static CAT: OnceLock<i18n::Catalog> = OnceLock::new();
    CAT.get_or_init(|| {
        let mo = i18n::catalog_file(locale_dir(), "ru", "mo");
        i18n::Catalog::load(&mo).expect("каталог читается")
    })
}

/// Текст файла для системы (ярлык, автозапуск, политика polkit, помощник,
/// метаданные темы) без строк языков, кроме исходного и русского.
///
/// ЗАЧЕМ. Такие файлы несут СРАЗУ все языки каталога (`system_l10n`):
/// `Name[de]=`, `<message xml:lang="de">`, `"Description[de]":`, строки
/// таблицы помощника `de) rx_lang=de ;;` и `de:ключ) rx_t=…`. Эталоны сняты
/// с английским и русским; новый язык в `po/LINGUAS` не должен перекрашивать
/// их все разом (так и случилось с немецким 01.10). Сами строки новых языков
/// проверяет `system_l10n` — здесь сверяется прежнее.
pub fn ru_only(text: &str) -> String {
    text.split_inclusive('\n')
        .filter(|line| match line_language(line) {
            Some(code) => code == "ru",
            None => true,
        })
        .collect()
}

/// Код языка строки системного файла или `None`, если строка не языковая.
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
    // "Description[de]": …  и  Name[de]=…
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
    // Таблица помощника: `de) rx_lang=de ;;` и `de:ключ) rx_t=…`.
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
