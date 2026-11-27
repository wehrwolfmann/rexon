//! Guard of the SINGLE door for writing into the buyer's profile.
//!
//! WHY. `std::fs::write` (and `File::create`) first TRUNCATES the file and only
//! then fills it. Between these two steps the buyer may close the window,
//! the system may go to sleep, and power may be lost; then a truncated file is left
//! on disk that looks real. Different profile files pay
//! for this in different ways:
//!
//!   * `settings.json` — a truncated one reads as an empty object: ALL of
//!     the buyer's settings silently roll back to the defaults;
//!   * `favorites.json` — «⭐ Favorites» silently empties the same way;
//!   * `bridge.json` and the bridge command files — another process considers such
//!     a file broken and DELETES it, the command vanishes without a trace;
//!   * the panel backup index (`backups.json`) and the desktop snapshot indexes —
//!     the copies themselves stay on disk, but there is nothing left to find and restore them with;
//!   * `mimeapps.list`, `kdeglobals`, the autostart line — these files are not ours
//!     but the buyer's: half of them means links that stopped opening and
//!     a dead line that comes back to life when logging into the session.
//!
//! ⭐ The mercy "broken reads as unwritten and gets recaptured by itself" exists for
//! EXACTLY ONE file — the app protection reference: there broken JSON means
//! "the reference has not been taken", and the next launch takes it anew. Its neighbours in
//! the directory have no such mercy, so the door must be shared, not
//! "wherever we were unlucky".
//!
//! WHAT WE GUARD. The checks read the SOURCES via `include_str!` (i.e. they are
//! rebuilt on any change to them) and go red by themselves, without a person:
//!
//!   1. the door itself (`utils::atomic_write_bytes`) is in place and built
//!      correctly: a temporary file NEXT TO the target one, `fsync`, `rename`;
//!   2. every write into the profile named by name goes through this door and
//!      contains no raw `fs::write` / `File::create`;
//!   3. modules that write nothing but the profile have no raw writes
//!      at all — a new door cannot sneak in there past the list from item 2.
//!
//! ⚠️ Added a new write into the profile — add it here too (rule #12).
//! A red test here does not mean "the test is outdated" but "the profile has two
//! doors again".

/// The shared door.
const UTILS_RS: &str = include_str!("../src/utils.rs");

/// Sources holding the writes guarded by name.
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

/// The tab window is the second entrance into the same profile.
const GUI_SETTINGS_ACTIONS_RS: &str = include_str!("../../rexon-gui/src/settings_actions.rs");
const GUI_TRAY_RS: &str = include_str!("../../rexon-gui/src/tray.rs");
const GUI_AUTOSTART_RS: &str = include_str!("../../rexon-gui/src/autostart_queries.rs");
const GUI_STEAM_WORKERS_RS: &str = include_str!("../../rexon-gui/src/steam_workers.rs");
const GUI_WE_UPDATE_WORKERS_RS: &str = include_str!("../../rexon-gui/src/we_update_workers.rs");

/// How to write a whole file BYPASSING the shared door. Exactly these idioms are searched for.
const RAW_WRITES: &[&str] = &[
    "fs::write(",
    "File::create(",
    "serde_json::to_writer",
    "write_all(",
];

/// Cut the tail with the test module off the source: raw writes are
/// legitimate in tests — they prepare the bench there, not write the buyer's profile.
fn without_tests(src: &str) -> &str {
    match src.find("\n#[cfg(test)]\n") {
        Some(at) => &src[..at],
        None => src,
    }
}

/// The body of the function `fn <name>` — from its first `{` to the matching `}`.
///
/// `anchor` — where in the file to start searching. It is needed where a method name
/// is declared FIRST in a contract (`trait`, a body without braces) and only then
/// implemented by the live executor: without the anchor the parser would find the declaration and
/// go red for nothing. An empty anchor — search from the start of the file.
///
/// The parsing is deliberately simple (a brace counter): in the bodies of these functions curly
/// braces occur only in `format!` templates, and those are paired. If
/// it ever stops being enough, the test will fail on the search rather than lie
/// with green.
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

/// One guarded door.
struct Door {
    /// The source text (via `include_str!`).
    src: &'static str,
    /// Its path, for a person reading the error message.
    file: &'static str,
    /// The name of the function that writes the file.
    name: &'static str,
    /// Where in the file to search for it (empty — from the start). See [`body_of`].
    anchor: &'static str,
    /// What exactly it puts on the buyer's disk.
    what: &'static str,
}

/// The list, by name, of ALL doors for writing into the buyer's profile.
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

/// Modules that have nothing to write but the profile: there must be no raw writes there
/// AT ALL, and a new door will not slip past the named list above.
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

    // The text form is the same door, not a second implementation.
    let text_door = body_of(src, "rexon-core/src/utils.rs", "atomic_write", "");
    assert!(
        text_door.contains("atomic_write_bytes(path, text.as_bytes())"),
        "atomic_write обязан звать байтовую дверь, а не писать своё"
    );

    // The crash report got it first; both windows call the name from `qt_app`.
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

/// The directory for experiments is in `$HOME/.cache`, not in `/tmp`: that is RAM (and a different
/// file system, on which `rename` would stop being a rename).
/// Unique per run.
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

/// The door really works, rather than only looking right in the source:
/// a reader pulling the file in a loop NEVER sees a half.
///
/// A raw `fs::write` right here next to it is caught by this very check — that is how
/// it was written: first the bite, then the fix.
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

    // Not a single stub, not a single forgotten temporary file.
    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != "settings.json")
        .collect();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(bad, 0, "читатель поймал недописанный файл {bad} раз");
    assert!(leftovers.is_empty(), "остались временные файлы: {leftovers:?}");
}

/// Two prices that renaming charges by default are NOT paid by the door:
/// the permissions of an existing file are kept, and a symbolic link is not replaced
/// by a plain file.
#[test]
fn the_door_keeps_file_mode_and_follows_a_symlink() {
    use std::os::unix::fs::PermissionsExt;
    let dir = home_tmp("atomic-mode");

    // A config closed to outsiders stays closed.
    let closed = dir.join("secret.json");
    std::fs::write(&closed, "старое").unwrap();
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o600)).unwrap();
    rexon_core::utils::atomic_write(&closed.to_string_lossy(), "новое").unwrap();
    let mode = std::fs::metadata(&closed).unwrap().permissions().mode() & 0o777;

    // A config from a store, placed as a link, stays a link.
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

/// The temporary file lands NEXT TO the target one — checked by contradiction: into
/// a directory that cannot be written to, the door does not write even over an existing
/// file. A raw `fs::write` would pass in this case (there is permission on the file),
/// and that is exactly how the two doors differ to the touch.
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
