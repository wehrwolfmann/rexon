//! Сторож ЕДИНСТВЕННОЙ двери записи в профиль покупателя.
//!
//! ЗАЧЕМ. `std::fs::write` (и `File::create`) файл сперва ОБРЕЗАЕТ и только
//! потом наполняет. Между этими двумя шагами покупатель может закрыть окно,
//! система — уйти в сон, а питание — пропасть; на диске тогда остаётся
//! обрезанный файл, который выглядит настоящим. Разные файлы профиля платят
//! за это по-разному:
//!
//!   * `settings.json` — обрезанный читается как пустой объект: ВСЕ
//!     настройки покупателя молча откатываются к значениям по умолчанию;
//!   * `favorites.json` — так же молча пустеет «⭐ Избранное»;
//!   * `bridge.json` и файлы команд моста — чужой процесс считает такой
//!     файл битым и УДАЛЯЕТ его, команда пропадает без следа;
//!   * опись копий панели (`backups.json`) и описи снимков рабочего стола —
//!     сами копии остаются на диске, но найти и восстановить их уже нечем;
//!   * `mimeapps.list`, `kdeglobals`, строка автозапуска — это файлы не наши,
//!     а покупателя: их половина означает переставшие открываться ссылки и
//!     мёртвую строку, которая оживает при входе в сеанс.
//!
//! ⭐ Милость «битое читается как ненаписанное и переснимается само» есть
//! РОВНО У ОДНОГО файла — эталона защиты приложений: битый JSON там значит
//! «эталон не снят», и следующий запуск снимает его заново. У соседей по
//! каталогу такой милости нет, поэтому дверь обязана быть общей, а не
//! «там, где не повезло».
//!
//! ЧТО СТЕРЕЖЁМ. Проверки читают ИСХОДНИКИ через `include_str!` (то есть
//! пересобираются при любой их правке) и краснеют сами, без человека:
//!
//!   1. сама дверь (`utils::atomic_write_bytes`) на месте и устроена
//!      правильно: временный файл РЯДОМ с целевым, `fsync`, `rename`;
//!   2. каждая поимённо названная запись в профиль идёт через эту дверь и
//!      не содержит сырых `fs::write` / `File::create`;
//!   3. в модулях, которые кроме профиля ничего не пишут, сырых записей нет
//!      вовсе — туда новая дверь не пролезет и мимо списка из пункта 2.
//!
//! ⚠️ Добавил новую запись в профиль — добавь её сюда же (правило #12).
//! Красный тест здесь означает не «тест устарел», а «у профиля снова две
//! двери».

/// Общая дверь.
const UTILS_RS: &str = include_str!("../src/utils.rs");

/// Исходники, в которых живут поимённо стерегомые записи.
const SETTINGS_RS: &str = include_str!("../src/settings.rs");
const FAVORITES_RS: &str = include_str!("../src/favorites.rs");
const BRIDGE_RS: &str = include_str!("../src/rexon_bridge.rs");
const BACKUP_RS: &str = include_str!("../src/backup.rs");
const INSTALL_JOURNAL_RS: &str = include_str!("../src/install_journal.rs");
const SCREENSHOTS_RS: &str = include_str!("../src/screenshots.rs");
const SYSTEM_EXEC_RS: &str = include_str!("../src/system_exec.rs");
const SYSTEM_DE_EXEC_RS: &str = include_str!("../src/system_de_exec.rs");
const APPDEFAULTS_EXEC_RS: &str = include_str!("../src/appdefaults_exec.rs");
const SPLASH_EXEC_RS: &str = include_str!("../src/splash_exec.rs");
const WALLPAPER_EXEC_RS: &str = include_str!("../src/wallpaper_exec.rs");
const APPEARANCE_INSTALLER_RS: &str = include_str!("../src/appearance_installer.rs");
const QT_APP_RS: &str = include_str!("../src/qt_app.rs");

/// Окно вкладок — второй вход в тот же профиль.
const GUI_SETTINGS_ACTIONS_RS: &str = include_str!("../../rexon-gui/src/settings_actions.rs");
const GUI_TRAY_RS: &str = include_str!("../../rexon-gui/src/tray.rs");
const GUI_AUTOSTART_RS: &str = include_str!("../../rexon-gui/src/autostart_queries.rs");
const GUI_STEAM_WORKERS_RS: &str = include_str!("../../rexon-gui/src/steam_workers.rs");
const GUI_WE_UPDATE_WORKERS_RS: &str = include_str!("../../rexon-gui/src/we_update_workers.rs");

/// Как записать целый файл МИМО общей двери. Ровно эти обороты и ищем.
const RAW_WRITES: &[&str] = &[
    "fs::write(",
    "File::create(",
    "serde_json::to_writer",
    "write_all(",
];

/// Отрезать от исходника хвост с модулем тестов: в тестах сырые записи
/// законны — там ими готовят стенд, а не пишут профиль покупателя.
fn without_tests(src: &str) -> &str {
    match src.find("\n#[cfg(test)]\n") {
        Some(at) => &src[..at],
        None => src,
    }
}

/// Тело функции `fn <name>` — от её первой `{` до парной ей `}`.
///
/// `anchor` — с какого места файла искать. Он нужен там, где имя метода
/// объявлено СНАЧАЛА в договоре (`trait`, тело без скобок) и только потом
/// исполнено живым исполнителем: без якоря разбор нашёл бы объявление и
/// покраснел зря. Пустой якорь — ищем с начала файла.
///
/// Разбор нарочно простой (счётчик скобок): в телах этих функций фигурные
/// скобки встречаются только в `format!`-шаблонах, а они парные. Если
/// когда-нибудь перестанет хватать — тест упадёт на поиске, а не соврёт
/// зелёным.
fn body_of<'a>(src: &'a str, file: &str, name: &str, anchor: &str) -> &'a str {
    let from = if anchor.is_empty() {
        0
    } else {
        src.find(anchor).unwrap_or_else(|| {
            panic!("{file}: не нашлось место `{anchor}` — исполнитель переехал или переименован")
        })
    };
    let needle = format!("fn {name}(");
    let at = from
        + src[from..]
            .find(&needle)
            .unwrap_or_else(|| panic!("{file}: не нашлась функция `{name}` — её переименовали или убрали, а дверь в профиль осталась"));
    let rest = &src[at..];
    let open = rest
        .find('{')
        .unwrap_or_else(|| panic!("{file}: у `{name}` нет тела"));
    let bytes = rest.as_bytes();
    let mut depth = 0usize;
    for i in open..bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &rest[open..=i];
                }
            }
            _ => {}
        }
    }
    panic!("{file}: тело `{name}` не закрылось")
}

/// Одна стерегомая дверь.
struct Door {
    /// Текст исходника (через `include_str!`).
    src: &'static str,
    /// Путь к нему для человека в сообщении об ошибке.
    file: &'static str,
    /// Имя функции, которая пишет файл.
    name: &'static str,
    /// С какого места файла её искать (пусто — с начала). См. [`body_of`].
    anchor: &'static str,
    /// Что именно она кладёт покупателю на диск.
    what: &'static str,
}

/// Поимённый список ВСЕХ дверей записи в профиль покупателя.
fn doors() -> Vec<Door> {
    let d = |src, file, name, anchor, what| Door { src, file, name, anchor, what };
    vec![
        d(SETTINGS_RS, "rexon-core/src/settings.rs", "save_settings", "", "~/.config/rexon/settings.json — все настройки покупателя"),
        d(FAVORITES_RS, "rexon-core/src/favorites.rs", "save_raw", "", "~/.config/rexon/favorites.json — «⭐ Избранное»"),
        d(BRIDGE_RS, "rexon-core/src/rexon_bridge.rs", "write_bridge", "", "~/.config/rexon/bridge.json — состояние моста"),
        d(BRIDGE_RS, "rexon-core/src/rexon_bridge.rs", "write_message", "", "~/.config/rexon/bridge_commands/*.json — команды моста"),
        d(BACKUP_RS, "rexon-core/src/backup.rs", "plasma_write_meta", "", "опись копий панели Plasma"),
        d(INSTALL_JOURNAL_RS, "rexon-core/src/install_journal.rs", "save_at", "", "~/.config/rexon/install_journal.json — общий журнал установки"),
        d(INSTALL_JOURNAL_RS, "rexon-core/src/install_journal.rs", "mirror_legacy", "", "~/.config/rexon/ocs_installed.json и extensions_state.json — зеркало общего журнала"),
        d(SCREENSHOTS_RS, "rexon-core/src/screenshots.rs", "save_screenshot", "", "~/.config/rexon/screenshots/* — снимки столов"),
        d(SCREENSHOTS_RS, "rexon-core/src/screenshots.rs", "placeholder_on_disk_for", "", "заглушка карточки стола"),
        d(SYSTEM_EXEC_RS, "rexon-core/src/system_exec.rs", "write_text", "impl SystemIo for LiveSystemIo {", "эталон защиты приложений по умолчанию и конфиги стола"),
        d(SYSTEM_DE_EXEC_RS, "rexon-core/src/system_de_exec.rs", "de_config_backup", "impl DeOpsIo for LiveDeOpsIo {", "~/.local/share/rexon/backups/metadata.json — опись снимков настроек стола"),
        d(APPDEFAULTS_EXEC_RS, "rexon-core/src/appdefaults_exec.rs", "write_text_live", "", "mimeapps.list, kdeglobals, строка автозапуска"),
        d(SPLASH_EXEC_RS, "rexon-core/src/splash_exec.rs", "write_atomic", "impl core_ss::SplashIo for LiveSplashIo {", "конфиг и юнит заставки входа"),
        d(WALLPAPER_EXEC_RS, "rexon-core/src/wallpaper_exec.rs", "write_text", "impl WpSys for LiveSys {", "состояние обоев и конфиги оболочек"),
        d(APPEARANCE_INSTALLER_RS, "rexon-core/src/appearance_installer.rs", "add_history", "", "~/.config/rexon/theme_history.json — история тем"),
        d(GUI_SETTINGS_ACTIONS_RS, "rexon-gui/src/settings_actions.rs", "write_file_atomic", "", "настройки со вкладки «Настройки»"),
        d(GUI_TRAY_RS, "rexon-gui/src/tray.rs", "set_autostart", "", "строка автозапуска Rexon"),
        d(GUI_AUTOSTART_RS, "rexon-gui/src/autostart_queries.rs", "write_file", "", "строка автозапуска и юнит службы"),
        d(GUI_STEAM_WORKERS_RS, "rexon-gui/src/steam_workers.rs", "write_atomic", "", "какой фон Steam применён и кэш инвентаря"),
        d(GUI_WE_UPDATE_WORKERS_RS, "rexon-gui/src/we_update_workers.rs", "write_state_text", "", "состояние обновления движка обоев"),
    ]
}

/// Модули, которым кроме профиля писать нечего: там сырых записей не должно
/// быть ВООБЩЕ, и новая дверь не проскочит мимо поимённого списка выше.
const PROFILE_ONLY: &[(&str, &str)] = &[
    (SETTINGS_RS, "rexon-core/src/settings.rs"),
    (FAVORITES_RS, "rexon-core/src/favorites.rs"),
    (BRIDGE_RS, "rexon-core/src/rexon_bridge.rs"),
];

#[test]
fn the_single_door_writes_next_to_the_target_syncs_and_renames() {
    let src = without_tests(UTILS_RS);
    let body = body_of(src, "rexon-core/src/utils.rs", "atomic_write_bytes", "");

    assert!(
        body.contains("with_file_name("),
        "временный файл обязан ложиться РЯДОМ с целевым: `rename` атомарен \
         только в пределах одной файловой системы, а /tmp здесь — ОЗУ"
    );
    assert!(
        !body.contains("temp_dir") && !body.contains("\"/tmp"),
        "через /tmp переименования не выйдет — выйдет копирование, и \
         обрезанный файл вернётся туда, откуда его убирали"
    );
    assert!(
        body.contains("sync_all()"),
        "без fsync `rename` может попасть на диск раньше содержимого: после \
         сбоя питания на месте целого старого файла окажется целый новый — \
         из нулей"
    );
    assert!(body.contains("rename("), "подмена имени — это и есть атомарность");
    assert!(
        body.contains("remove_file(&tmp)"),
        "не получилось — временный файл обязан убраться за собой"
    );

    // Текстовая форма — та же дверь, а не вторая реализация.
    let text_door = body_of(src, "rexon-core/src/utils.rs", "atomic_write", "");
    assert!(
        text_door.contains("atomic_write_bytes(path, text.as_bytes())"),
        "atomic_write обязан звать байтовую дверь, а не писать своё"
    );

    // Отчёт о крахе получил её первым; имя из `qt_app` зовут оба окна.
    assert!(
        QT_APP_RS.contains("pub use crate::utils::atomic_write;"),
        "qt_app::atomic_write обязан быть ПЕРЕОБЪЯВЛЕНИЕМ общей двери, а не \
         второй её копией"
    );
}

#[test]
fn every_profile_write_goes_through_the_single_door() {
    let mut broken: Vec<String> = Vec::new();
    for door in doors() {
        let Door { src, file, name, anchor, what } = door;
        let body = body_of(without_tests(src), file, name, anchor);
        if !body.contains("atomic_write") {
            broken.push(format!("{file}::{name} ({what}) — не зовёт общую дверь"));
            continue;
        }
        for raw in RAW_WRITES {
            if body.contains(raw) {
                broken.push(format!(
                    "{file}::{name} ({what}) — пишет сырым `{raw}` мимо общей двери"
                ));
            }
        }
    }
    assert!(
        broken.is_empty(),
        "запись в профиль покупателя пошла мимо единственной атомарной двери \
         (`rexon_core::utils::atomic_write`):\n  {}\n\
         Обычная запись сперва ОБРЕЗАЕТ файл: закрытое в эту миллисекунду \
         окно оставляет покупателя без настроек.",
        broken.join("\n  ")
    );
}

/// Каталог для опытов — в `$HOME/.cache`, а не в `/tmp`: там ОЗУ (и другая
/// файловая система, на которой `rename` перестал бы быть переименованием).
/// Уникален на прогон.
fn home_tmp(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap())
        .join(".cache")
        .join(format!("rexon-test-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Дверь и вправду работает, а не только выглядит правильно в исходнике:
/// читатель, дёргающий файл в цикле, НИ РАЗУ не видит половины.
///
/// Сырой `fs::write` здесь же, рядом, ловится этой самой проверкой — так она
/// и писалась: сперва укус, потом починка.
#[test]
fn a_reader_never_catches_half_a_file() {
    let dir = home_tmp("atomic-reader");
    let path = dir.join("settings.json");
    let short = "{}".to_string();
    let long = format!("{{\"ключ\": \"{}\"}}", "я".repeat(40_000));
    rexon_core::utils::atomic_write(&path.to_string_lossy(), &short).unwrap();

    let reader_path = path.clone();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_reader = stop.clone();
    let seen_half = std::thread::spawn(move || {
        let mut bad = 0usize;
        while !stop_reader.load(std::sync::atomic::Ordering::Relaxed) {
            if let Ok(text) = std::fs::read_to_string(&reader_path) {
                if !text.is_empty() && !text.ends_with('}') {
                    bad += 1;
                }
            }
        }
        bad
    });

    for i in 0..60 {
        let text = if i % 2 == 0 { &long } else { &short };
        rexon_core::utils::atomic_write(&path.to_string_lossy(), text).unwrap();
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let bad = seen_half.join().unwrap();

    // Ни одного огрызка, ни одного забытого временного файла.
    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != "settings.json")
        .collect();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(bad, 0, "читатель поймал недописанный файл {bad} раз");
    assert!(leftovers.is_empty(), "остались временные файлы: {leftovers:?}");
}

/// Две цены, которые подмена имени берёт по умолчанию, дверью НЕ платятся:
/// права уже лежащего файла сохраняются, а символьная ссылка не подменяется
/// простым файлом.
#[test]
fn the_door_keeps_file_mode_and_follows_a_symlink() {
    use std::os::unix::fs::PermissionsExt;
    let dir = home_tmp("atomic-mode");

    // Закрытый от посторонних конфиг остаётся закрытым.
    let closed = dir.join("secret.json");
    std::fs::write(&closed, "старое").unwrap();
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o600)).unwrap();
    rexon_core::utils::atomic_write(&closed.to_string_lossy(), "новое").unwrap();
    let mode = std::fs::metadata(&closed).unwrap().permissions().mode() & 0o777;

    // Конфиг из хранилища, расставленный ссылкой, остаётся ссылкой.
    let store = dir.join("хранилище.json");
    std::fs::write(&store, "из хранилища").unwrap();
    let link = dir.join("mimeapps.list");
    std::os::unix::fs::symlink(&store, &link).unwrap();
    rexon_core::utils::atomic_write(&link.to_string_lossy(), "правка").unwrap();
    let still_link = std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink();
    let store_text = std::fs::read_to_string(&store).unwrap();

    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(mode, 0o600, "права закрытого файла обязаны пережить запись");
    assert!(still_link, "ссылка не смеет превратиться в простой файл");
    assert_eq!(store_text, "правка", "писать надо в то, на что ссылка смотрит");
}

/// Временный файл ложится РЯДОМ с целевым — проверено от противного: в
/// каталог, куда писать нельзя, дверь не пишет даже поверх существующего
/// файла. Сырой `fs::write` в этом случае как раз прошёл бы (право на файл
/// есть), и именно этим две двери и различаются на ощупь.
#[test]
fn the_tmp_file_lands_next_to_the_target_not_in_tmpfs() {
    use std::os::unix::fs::PermissionsExt;
    let dir = home_tmp("atomic-next-to");
    let path = dir.join("settings.json");
    std::fs::write(&path, "прежнее").unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();

    let result = rexon_core::utils::atomic_write(&path.to_string_lossy(), "новое");

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let kept = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        result.is_err(),
        "дверь обязана заводить временный файл в каталоге цели — иначе \
         `rename` перестал бы быть переименованием"
    );
    assert_eq!(kept, "прежнее", "неудача не смеет трогать прежний файл");
}

#[test]
fn modules_that_only_write_the_profile_have_no_raw_writes_at_all() {
    let mut broken: Vec<String> = Vec::new();
    for (src, file) in PROFILE_ONLY {
        let live = without_tests(src);
        for raw in RAW_WRITES {
            if live.contains(raw) {
                broken.push(format!("{file} — сырой `{raw}`"));
            }
        }
    }
    assert!(
        broken.is_empty(),
        "в этих модулях кроме профиля покупателя писать нечего, значит сырых \
         записей быть не должно вовсе:\n  {}",
        broken.join("\n  ")
    );
}
