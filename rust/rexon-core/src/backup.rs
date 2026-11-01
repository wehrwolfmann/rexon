//! Резервные копии конфигов DE — порт детерминированных частей `rexon/backup.py`
//! (`BackupManager`, `PlasmaProtector`).
//!
//! Портированы ТОЛЬКО чистые вход→выход куски (сверяются golden-тестом
//! `backup_parity.rs` с самим Python):
//!   * константы staging/rollback-суффиксов и списки конфигов Plasma
//!     (`PANEL_CONFIGS`, `FULL_CONFIGS`);
//!   * имя бэкапа `{de}_{timestamp}` (`backup_name`);
//!   * безопасное имя файла в снапшоте — `relpath(path, home)` с заменой
//!     разделителя и срезкой ведущих `.`/`_` (`safe_config_name`, `posix_relpath`);
//!   * детект Plasma по `XDG_CURRENT_DESKTOP` (`is_plasma_desktop`);
//!   * раскрытие `~` в списке путей (`expand_tilde`);
//!   * выборки/сортировки метаданных (`de_backups_for`, `plasma_backups`,
//!     `plasma_latest_backup`, `plasma_has_backups`).
//!
//! С 2026-07-30 портирован и САМ транзакционный движок восстановления
//! (`transactional_restore`) — он принципиально не выражается «планом»:
//! staging → atomic swap → commit/rollback это конечный автомат над ФС, и
//! половина отката жила бы в шиме. Работает напрямую через `std::fs`
//! (как `ext_utils.rs`/`settings.rs`), сам чинит своё состояние при сбое и
//! НИКОГДА не паникует: любая ошибка возвращается в [`RestoreOutcome`], так
//! что для шима «исключение моста» = «до ФС дело не дошло» и фолбэк безопасен.
//! Вместе с ним портированы решения обоих менеджеров: раскладка снапшота,
//! выбор записи для восстановления, ротация снимков Plasma, argv перезапуска
//! plasmashell и все пользовательские сообщения.
//!
//! ⚠ Правило #16: в оригинале при падении ПОСЕРЕДИНЕ atomic swap элемент,
//! у которого `rename(dest → rollback)` уже прошёл, а `rename(stage → dest)`
//! ещё нет, в список `swapped` не попадал — его откат не выполнялся, и
//! пользователь оставался БЕЗ файла (rollback-копия сиротела рядом). Порт
//! ведёт такой «полёт» отдельно и откатывает его тоже; тот же фикс внесён в
//! Python-эталон `_transactional_restore` (он остаётся фолбэком).
//!
//! Остаётся Python-стороне: `glob` шаблонов конфигов, чтение/запись JSON
//! метаданных, вызов `subprocess` для plasmashell.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

use crate::system_plan::Cmd;
use crate::i18n::{ntr, tr, trf};

/// Суффикс staging-копии при транзакционном restore (`dest + _STAGE_SUFFIX`).
pub const STAGE_SUFFIX: &str = ".rexon-new";
/// Суффикс rollback-копии оригинала при atomic swap.
pub const ROLLBACK_SUFFIX: &str = ".rexon-rollback";

/// Критичные конфиги панели Plasma (`PlasmaProtector.PANEL_CONFIGS`).
pub const PANEL_CONFIGS: &[&str] = &[
    "~/.config/plasma-org.kde.plasma.desktop-appletsrc",
    "~/.config/plasmashellrc",
    "~/.config/plasmarc",
    "~/.config/plasmanotifyrc",
    "~/.config/plasma-localerc",
];

/// Расширенный набор конфигов Plasma (`FULL_CONFIGS = PANEL_CONFIGS + extra`).
/// Порядок = панель, затем дополнительные (важно для копирования/сверки).
pub const FULL_CONFIGS: &[&str] = &[
    "~/.config/plasma-org.kde.plasma.desktop-appletsrc",
    "~/.config/plasmashellrc",
    "~/.config/plasmarc",
    "~/.config/plasmanotifyrc",
    "~/.config/plasma-localerc",
    "~/.config/kdeglobals",
    "~/.config/kwinrc",
    "~/.config/kglobalshortcutsrc",
    "~/.config/kcminputrc",
    "~/.config/kscreenlockerrc",
    "~/.config/ksmserverrc",
    "~/.config/klaunchrc",
    "~/.config/khotkeysrc",
    "~/.config/lattedockrc",
    "~/.local/share/plasma",
];

/// Имя бэкапа — `f"{de_name}_{timestamp}"` (`create_backup`).
pub fn backup_name(de_name: &str, timestamp: &str) -> String {
    format!("{de_name}_{timestamp}")
}

/// Раскрытие `~` в каждом пути на `home` (порт `PlasmaProtector._expand`).
/// Как Python `str.replace("~", home)` — заменяет ВСЕ вхождения `~`.
pub fn expand_tilde(paths: &[&str], home: &str) -> Vec<String> {
    expand_tilde_xdg(paths, home, None)
}

/// То же с учётом `$XDG_CONFIG_HOME`: ведущий `~/.config` уходит в корень
/// конфигов (единый расширитель ядра — [`crate::utils::expand_user`]), а
/// остальные `~` раскрываются в `home` как раньше. Без переменной результат
/// совпадает с [`expand_tilde`] байт-в-байт.
pub fn expand_tilde_xdg(
    paths: &[&str],
    home: &str,
    xdg_config_home: Option<&str>,
) -> Vec<String> {
    paths
        .iter()
        .map(|p| {
            let head = crate::utils::expand_user(p, home, xdg_config_home);
            // хвостовые `~` внутри пути (квирк оригинала: replace ВСЕХ)
            head.replace('~', home)
        })
        .collect()
}

/// Детект работающей Plasma по `XDG_CURRENT_DESKTOP`
/// (`PlasmaProtector.is_plasma_running`): `"kde"` или `"plasma"` в нижнем
/// регистре строки.
pub fn is_plasma_desktop(xdg_current_desktop: &str) -> bool {
    let de = xdg_current_desktop.to_lowercase();
    de.contains("kde") || de.contains("plasma")
}

/// Нормализованные компоненты абсолютного POSIX-пути (аналог `os.path.normpath`
/// для абсолютного пути: отбрасываем пустые и `.`, `..` схлопываем, не поднимаясь
/// выше корня).
fn norm_abs_components(path: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|&l| l != "..") {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Относительный путь от `start` до `path` для абсолютных POSIX-путей —
/// порт `os.path.relpath` (без разрешения симлинков). `path == start` → `"."`.
pub fn posix_relpath(path: &str, start: &str) -> String {
    let sc = norm_abs_components(start);
    let pc = norm_abs_components(path);
    let i = sc.iter().zip(pc.iter()).take_while(|(a, b)| a == b).count();
    let mut rel: Vec<&str> = vec![".."; sc.len() - i];
    rel.extend_from_slice(&pc[i..]);
    if rel.is_empty() {
        ".".to_string()
    } else {
        rel.join("/")
    }
}

/// Безопасное имя файла в снапшоте, без коллизий (логика `create_backup`):
/// `relpath(path, home)` → замена `/` на `_` → срезка ведущих `.`/`_`.
/// `~/.config/gtk-3.0/settings.ini` → `config_gtk-3.0_settings.ini`.
pub fn safe_config_name(path: &str, home: &str) -> String {
    let rel = posix_relpath(path, home);
    rel.replace('/', "_")
        .trim_start_matches(['.', '_'])
        .to_string()
}

// ── Метаданные: выборки и сортировки (чистые) ──────────────────────────────

/// Сводка бэкапа для конкретного DE (`BackupManager.get_backups_for_de`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeBackupSummary {
    pub name: String,
    pub timestamp: String,
    pub files_count: usize,
}

/// Запись метаданных `BackupManager` (значение `metadata[name]`), из которой
/// берутся `de_name`, `timestamp` и число сохранённых путей.
#[derive(Debug, Clone)]
pub struct DeMetaEntry {
    pub name: String,
    pub de_name: String,
    pub timestamp: String,
    pub files_count: usize,
}

/// Бэкапы для DE, от новых к старым (`get_backups_for_de`): фильтр по `de_name`,
/// затем стабильная сортировка по `timestamp` по убыванию (как Python
/// `sorted(..., reverse=True)` — при равных ключах порядок вставки сохраняется).
pub fn de_backups_for(entries: &[DeMetaEntry], de_name: &str) -> Vec<DeBackupSummary> {
    let mut v: Vec<DeBackupSummary> = entries
        .iter()
        .filter(|e| e.de_name == de_name)
        .map(|e| DeBackupSummary {
            name: e.name.clone(),
            timestamp: e.timestamp.clone(),
            files_count: e.files_count,
        })
        .collect();
    v.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    v
}

/// Одна запись бэкапа Plasma (`PlasmaProtector._meta["backups"][i]`).
///
/// `deny_unknown_fields`: Python возвращает записи мета-файла КАК ЕСТЬ
/// (вместе с неизвестными ключами) — типизированный разбор молча потерял бы
/// их; вместо этого ошибка разбора, и шим уходит на Python-фолбэк.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlasmaBackup {
    pub timestamp: String,
    pub reason: String,
    pub path: String,
    pub files: Vec<String>,
    pub full: bool,
}

/// Список бэкапов Plasma от новых к старым (`get_backups` = `reversed(...)`).
pub fn plasma_backups(backups: &[PlasmaBackup]) -> Vec<PlasmaBackup> {
    backups.iter().rev().cloned().collect()
}

/// Последний бэкап Plasma (`get_latest_backup` = `backups[-1]` или None).
pub fn plasma_latest_backup(backups: &[PlasmaBackup]) -> Option<PlasmaBackup> {
    backups.last().cloned()
}

/// Есть ли бэкапы Plasma (`has_backups`).
pub fn plasma_has_backups(backups: &[PlasmaBackup]) -> bool {
    !backups.is_empty()
}

// ══════════════════════════════════════════════════════════════════════════
//  Транзакционное восстановление (порт `_transactional_restore`)
// ══════════════════════════════════════════════════════════════════════════

/// Итог транзакции — кортеж `(ok, message, count_restored)` оригинала.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RestoreOutcome {
    pub ok: bool,
    pub message: String,
    pub restored: usize,
}

/// `os.path.exists(p) or os.path.islink(p)` — путь занят (битая ссылка тоже).
fn exists_or_link(p: &Path) -> bool {
    p.symlink_metadata().is_ok()
}

/// Порт `_remove_path`: симлинк/файл — unlink, каталог — рекурсивно, отсутствие
/// пути ошибкой не считается (у оригинала `ignore_errors=True`).
fn remove_path(p: &Path) {
    if let Ok(md) = p.symlink_metadata() {
        if md.is_dir() {
            let _ = fs::remove_dir_all(p);
        } else {
            let _ = fs::remove_file(p);
        }
    }
}

/// Копия файла «как есть» — порт `shutil.copy2(..., follow_symlinks=False)`:
/// симлинк воспроизводится симлинком, у обычного файла сохраняются права и
/// время модификации (без времени конфиг выглядел бы только что изменённым —
/// демоны DE следят за mtime).
fn copy_file_like(src: &Path, dst: &Path) -> io::Result<()> {
    let md = fs::symlink_metadata(src)?;
    if md.file_type().is_symlink() {
        std::os::unix::fs::symlink(fs::read_link(src)?, dst)?;
        return Ok(());
    }
    fs::copy(src, dst)?;
    if let (Ok(modified), Ok(f)) = (
        md.modified(),
        fs::OpenOptions::new().write(true).open(dst),
    ) {
        let mut t = fs::FileTimes::new().set_modified(modified);
        if let Ok(accessed) = md.accessed() {
            t = t.set_accessed(accessed);
        }
        let _ = f.set_times(t);
    }
    Ok(())
}

/// Рекурсивная копия каталога — порт `shutil.copytree(..., symlinks=True)`:
/// вложенные симлинки НЕ разыменовываются (иначе снапшот раздувается и
/// теряет структуру `~/.local/share/plasma`).
fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    if let Ok(md) = fs::metadata(src) {
        let _ = fs::set_permissions(dst, md.permissions());
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            remove_path(&to);
            std::os::unix::fs::symlink(fs::read_link(&from)?, &to)?;
        } else if ft.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            copy_file_like(&from, &to)?;
        }
    }
    Ok(())
}

/// Транзакционное восстановление набора `(src, dest)`.
///
/// Гарантии оригинала сохранены: либо заменены ВСЕ dest, либо ни один;
/// staging рядом с dest (одна ФС → `rename` атомарен); на любой ошибке —
/// возврат к исходному состоянию. Функция не паникует: ошибки ФС уходят в
/// `message`, поэтому исключение на границе моста означает, что до ФС дело
/// не дошло, и фолбэк на Python безопасен.
pub fn transactional_restore(items: &[(String, String)]) -> RestoreOutcome {
    // ── Фаза 1: staging — копии рядом с целью, оригиналы не тронуты ──
    let mut staged: Vec<(String, String)> = Vec::new();
    for (src, dest) in items {
        let src_p = Path::new(src);
        if !exists_or_link(src_p) {
            continue;
        }
        let stage = format!("{dest}{STAGE_SUFFIX}");
        let stage_p = Path::new(&stage);
        let staged_one = (|| -> io::Result<()> {
            let parent = Path::new(dest).parent().unwrap_or(Path::new(""));
            let parent =
                if parent.as_os_str().is_empty() { Path::new(".") } else { parent };
            fs::create_dir_all(parent)?;
            // Убрать staging с прошлой неудачной попытки.
            remove_path(stage_p);
            if fs::symlink_metadata(src_p)?.is_dir() {
                copy_tree(src_p, stage_p)
            } else {
                copy_file_like(src_p, stage_p)
            }
        })();
        if let Err(e) = staged_one {
            for (_d, s) in &staged {
                remove_path(Path::new(s));
            }
            remove_path(stage_p);
            return RestoreOutcome {
                ok: false,
                message: format!("staging failed: {e}"),
                restored: 0,
            };
        }
        staged.push((dest.clone(), stage));
    }

    // ── Фаза 2: atomic swap — rename(dest → rollback), rename(stage → dest) ──
    let mut swapped: Vec<(String, Option<String>)> = Vec::new();
    for (dest, stage) in &staged {
        let dest_p = Path::new(dest);
        let mut rollback: Option<String> = None;
        // Прошли ли первый rename: только после него цель «уехала» и её
        // нужно возвращать. Если упал сам первый rename — цель не тронута,
        // и трогать её в откате НЕЛЬЗЯ (иначе снесём живой конфиг).
        let mut moved = false;
        let swap_one = (|| -> io::Result<()> {
            if exists_or_link(dest_p) {
                let rb = format!("{dest}{ROLLBACK_SUFFIX}");
                remove_path(Path::new(&rb));
                fs::rename(dest_p, &rb)?;
                rollback = Some(rb);
            }
            moved = true;
            fs::rename(stage, dest_p)
        })();
        if let Err(e) = swap_one {
            // ⚠ Правило #16: элемент «в полёте» (цель уехала в rollback, а
            // stage → dest ещё не прошёл) оригинал в откат НЕ включал — цель
            // оставалась удалённой. Добавляем его к откатываемым.
            let mut done = swapped.clone();
            if moved {
                done.push((dest.clone(), rollback));
            }
            for (d, rb) in &done {
                let dp = Path::new(d);
                remove_path(dp);
                if let Some(rb) = rb {
                    let rbp = Path::new(rb);
                    if exists_or_link(rbp) {
                        let _ = fs::rename(rbp, dp);
                    }
                }
            }
            // Убрать staging, не дошедший до swap: у реально обменянных он
            // уже переименован в dest, у элемента «в полёте» — на месте.
            let done_stages: Vec<String> = swapped
                .iter()
                .map(|(d, _)| format!("{d}{STAGE_SUFFIX}"))
                .collect();
            for (_d, s) in &staged {
                if !done_stages.contains(s) && exists_or_link(Path::new(s)) {
                    remove_path(Path::new(s));
                }
            }
            return RestoreOutcome {
                ok: false,
                message: format!("swap failed, rolled back: {e}"),
                restored: 0,
            };
        }
        swapped.push((dest.clone(), rollback));
    }

    // ── Commit: rollback-копии больше не нужны ──
    for (_d, rb) in &swapped {
        if let Some(rb) = rb {
            remove_path(Path::new(rb));
        }
    }
    RestoreOutcome {
        ok: true,
        message: msg_transaction_restored(swapped.len()),
        restored: swapped.len(),
    }
}

// ══════════════════════════════════════════════════════════════════════════
//  BackupManager: раскладка снапшота, выбор источников, сообщения
// ══════════════════════════════════════════════════════════════════════════

/// Успешный итог транзакции (внутреннее сообщение `_transactional_restore`).
///
/// Починка склонения — та же, что у [`msg_restored_items`]: эта строка
/// попадает человеку на глаза внутри сообщения о неудаче восстановления.
pub fn msg_transaction_restored(n: usize) -> String {
    // TRANSLATORS: goes after a colon inside a restore failure message, hence lowercase.
    ntr("restored %n item", "restored %n items", n as u64)
}

/// Итог `BackupManager.restore_backup`.
///
/// Починка (правило #16): счёт стоял с одним окончанием на все числа —
/// покупатель читал «Восстановлено 1 элементов».
pub fn msg_restored_items(n: usize) -> String {
    ntr("Restored %n item", "Restored %n items", n as u64)
}

/// `restore_backup`: бэкапа с таким именем нет в метаданных.
pub fn msg_backup_not_found() -> String {
    tr("Backup not found")
}

/// Соединение путей как `os.path.join` для наших случаев (`base` абсолютный,
/// `name` всегда относительный — `safe_config_name`/basename).
fn join(base: &str, name: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

/// Порт `os.path.basename` для POSIX-путей.
fn basename(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// Раскладка снапшота DE (`create_backup`): куда лечь каждому конфигу.
/// Возвращает `(src, dest)` — имя в снапшоте не теряет структуру пути.
pub fn snapshot_items(
    paths: &[String],
    backup_path: &str,
    home: &str,
) -> Vec<(String, String)> {
    paths
        .iter()
        .map(|p| (p.clone(), join(backup_path, &safe_config_name(p, home))))
        .collect()
}

/// Источники для `restore_backup`: `safe_config_name` с фолбэком на basename
/// (обратная совместимость со снапшотами старых версий); чего нет — пропуск.
pub fn restore_items(
    backup_path: &str,
    orig_paths: &[String],
    home: &str,
    exists: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    let mut items = Vec::new();
    for orig in orig_paths {
        let mut src = join(backup_path, &safe_config_name(orig, home));
        if !exists(&src) {
            src = join(backup_path, basename(orig));
            if !exists(&src) {
                continue;
            }
        }
        items.push((src, orig.clone()));
    }
    items
}

/// Запись `metadata[backup_name]` (`create_backup`) — порядок ключей как у
/// Python-словаря, чтобы `metadata.json` совпадал байт-в-байт.
pub fn de_meta_entry(
    de_name: &str,
    timestamp: &str,
    paths: &[String],
    backup_path: &str,
) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    m.insert("de_name".into(), de_name.into());
    m.insert("timestamp".into(), timestamp.into());
    m.insert("paths".into(), paths.to_vec().into());
    m.insert("backup_path".into(), backup_path.into());
    serde_json::Value::Object(m)
}

// ══════════════════════════════════════════════════════════════════════════
//  PlasmaProtector: снимки панели, ротация, выбор, перезапуск оболочки
// ══════════════════════════════════════════════════════════════════════════

/// Сколько снимков панели держим (`while len(...) > 10` в `backup_panel`).
pub const MAX_PLASMA_SNAPSHOTS: usize = 10;

/// Пауза между остановкой и запуском plasmashell (`time.sleep(2)`).
pub const PLASMASHELL_SETTLE_SECS: u64 = 2;

// ⭐ Слова этих сообщений — те же, какими подписана сама секция «Защита
// панели KDE Plasma»: «Сохранить копию», «Сохранённые копии», «Папка копии».
// Оригинал (Python) говорил здесь «бэкап» и «снапшот», и покупатель читал в
// одном окне два разных названия одной вещи. Расхождение с оригиналом
// намеренное (правила #13 и #16), сторож — `tests/backup_parity.rs`.

/// `backup_panel`: копировать нечего.
///
/// Было «Нет файлов для бэкапа»: слово «бэкап» покупателю ничего не
/// объясняет, а из самой фразы не видно, каких файлов не нашлось.
pub fn msg_nothing_to_backup() -> String {
    tr("Nothing to copy: there are no panel settings files")
}
/// `restore_panel`: снимков панели нет вовсе.
///
/// Было «Нет бэкапов Plasma»; теми же словами подписан и пустой список в
/// окне («Сохранённых копий пока нет»).
pub fn msg_no_plasma_backups() -> String {
    tr("No saved copies of the panel yet")
}
/// `restore_panel`: транзакция прошла, но ни один файл не восстановился.
pub fn msg_none_restored() -> String {
    tr("Couldn't restore any files")
}
/// Хвост сообщения `restore_panel` после перезапуска оболочки.
///
/// Было «Plasmashell перезапущен» — имя служебной программы вместо того, что
/// человек видит на экране. Соседняя строка на неудачу
/// ([`msg_plasmashell_not_restarted`]) с самого начала говорила «панель».
///
/// Хвост начинается с перевода строки — он дописывается к итогу возврата.
pub fn msg_plasmashell_restarted() -> String {
    format!("\n{}", tr("Panel restarted"))
}

/// Итог `backup_panel`.
///
/// Починка (правило #16): счёт стоял с одним окончанием на все числа —
/// «Бэкап создан (1 файлов)». Склоняет каталог перевода ([`ntr`]), как и
/// остальные счётные строки продукта.
pub fn msg_backup_created(n: usize) -> String {
    ntr("Copy saved (%n file)", "Copy saved (%n files)", n as u64)
}

/// `restore_panel`: снимка с такой меткой времени нет.
///
/// Метку времени показываем в том же человеческом виде, в каком подписаны
/// строки списка копий, — машинное «20260730_010203» покупателю ни о чём не
/// говорит.
pub fn msg_snapshot_not_found(timestamp: &str) -> String {
    // Метка идёт в хвосте, а не в середине фразы: неразобранная метка
    // остаётся как есть, и «Копия от нет-такой не найдена» читалось бы
    // ломаным русским.
    // TRANSLATORS: %1 is the date and time the copy was made.
    trf("Copy not found: %1", &[&crate::qt_settings::pretty_backup_ts(timestamp)])
}

/// `restore_panel`: каталог снимка исчез с диска.
pub fn msg_snapshot_dir_missing(dir: &str) -> String {
    // TRANSLATORS: %1 is a folder path.
    trf("The copy's folder wasn't found: %1", &[&dir])
}

/// `restore_panel`: транзакция не удалась (внутренний текст — от неё).
pub fn msg_restore_failed(detail: &str) -> String {
    // TRANSLATORS: %1 explains what went wrong.
    trf("The restore failed: %1", &[&detail])
}

/// Итог `restore_panel` (до строки о перезапуске оболочки).
///
/// Те же две починки, что у [`msg_backup_created`] и
/// [`msg_snapshot_not_found`]: склонение счёта и человеческая метка времени
/// вместо машинной.
pub fn msg_restored_files(n: usize, timestamp: &str) -> String {
    ntr(
        // TRANSLATORS: %n is the number of files, %1 the date and time the copy was made.
        "Restored %n file from the copy of %1",
        "Restored %n files from the copy of %1",
        n as u64,
    )
    .replace("%1", &crate::qt_settings::pretty_backup_ts(timestamp))
}

/// Раскладка снимка панели (`backup_panel`): имя в снимке — basename, потому
/// что все PANEL/FULL-конфиги лежат в разных каталогах без коллизий имён.
/// Отсутствующие конфиги в снимок не попадают.
pub fn plasma_snapshot_items(
    configs: &[String],
    snap_dir: &str,
    exists: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    configs
        .iter()
        .filter(|p| exists(p))
        .map(|p| (p.clone(), join(snap_dir, basename(p))))
        .collect()
}

/// Запись `_meta["backups"]` для только что снятого снимка.
pub fn plasma_entry(
    timestamp: &str,
    reason: &str,
    snap_dir: &str,
    saved: &[String],
    full: bool,
) -> PlasmaBackup {
    PlasmaBackup {
        timestamp: timestamp.to_string(),
        reason: reason.to_string(),
        path: snap_dir.to_string(),
        files: saved.to_vec(),
        full,
    }
}

/// Ротация снимков: пока их больше [`MAX_PLASMA_SNAPSHOTS`], выбрасываем
/// самые старые (с начала списка). Возвращает каталоги, которые исполнителю
/// нужно удалить, — сам список уже урезан.
pub fn plasma_rotate(backups: &mut Vec<PlasmaBackup>) -> Vec<String> {
    let mut dropped = Vec::new();
    while backups.len() > MAX_PLASMA_SNAPSHOTS {
        dropped.push(backups.remove(0).path);
    }
    dropped
}

/// Выбор снимка для восстановления (`restore_panel`): по метке времени либо
/// последний. `Err` — готовое сообщение пользователю.
pub fn plasma_pick<'a>(
    backups: &'a [PlasmaBackup],
    timestamp: Option<&str>,
) -> Result<&'a PlasmaBackup, String> {
    if backups.is_empty() {
        return Err(msg_no_plasma_backups());
    }
    match timestamp.filter(|t| !t.is_empty()) {
        Some(ts) => backups
            .iter()
            .find(|b| b.timestamp == ts)
            .ok_or_else(|| msg_snapshot_not_found(ts)),
        // `backups[-1]`: последний по порядку добавления, не по метке.
        None => Ok(backups.last().expect("проверено на пустоту выше")),
    }
}

/// Источники для восстановления панели: `snap_dir/basename(orig)`, чего нет —
/// пропуск (порт цикла в `restore_panel`).
pub fn plasma_restore_items(
    entry: &PlasmaBackup,
    exists: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    entry
        .files
        .iter()
        .map(|orig| (join(&entry.path, basename(orig)), orig.clone()))
        .filter(|(src, _)| exists(src))
        .collect()
}

/// Индекс снимка с данной меткой (`delete_backup`); None — такого нет.
pub fn plasma_delete_index(backups: &[PlasmaBackup], timestamp: &str) -> Option<usize> {
    backups.iter().position(|b| b.timestamp == timestamp)
}

/// Штатный перезапуск оболочки: `systemctl --user restart
/// plasma-plasmashell.service`.
///
/// ⚠️ Раньше здесь был `kquitapp6 plasmashell`, которого нет на Ubuntu
/// 24.04 (там Plasma 5 и `kquitapp5`), — то есть штатный путь на самом
/// массовом целевом дистрибутиве всегда проваливался и оболочку добивали
/// `killall`. Плюс в двух других местах проекта та же задача уже решалась
/// правильно — юнитом (`waywallen_kde_plugin.rs::restart_plasmashell_cmds`,
/// `app.rs::plasmashell_restart_argv`), то есть это был третий, отставший
/// экземпляр одного механизма (правило #12). Юнит `plasma-plasmashell`
/// поднимает оболочку сам, поэтому при успехе ни остановка, ни запуск не
/// нужны; если systemd-юнита нет (не-systemd система, Plasma без
/// systemd-старта), [`plasma_restart_shell`] уходит на `kquitapp` + `kstart`,
/// а затем — на PID оболочки своего сеанса.
pub fn plasma_quit_cmd() -> Cmd {
    Cmd {
        argv: vec![
            "systemctl".into(),
            "--user".into(),
            "restart".into(),
            "plasma-plasmashell.service".into(),
        ],
        // Рестарт синхронный: юнит должен успеть подняться (как в app.rs).
        timeout_secs: Some(15),
        lang_c: false,
    }
}

/// Штатная остановка оболочки, когда юнита нет: `kquitapp6 plasmashell`
/// (исполнитель подменяет на `kquitapp5` на Plasma 5 — `utils::TOOL_SETS`).
///
/// ⚠️ Прежде здесь был `killall plasmashell`: убийство ПО ИМЕНИ, которое
/// задевает любой процесс с таким именем у любого сеанса этого человека и
/// не отличает оболочку текущего сеанса от чужой. `kquitapp` просит
/// оболочку выйти через D-Bus ЭТОГО сеанса — чужую он не достанет.
pub fn plasma_kill_cmd() -> Cmd {
    Cmd {
        argv: vec!["kquitapp6".into(), "plasmashell".into()],
        timeout_secs: Some(10),
        lang_c: false,
    }
}

/// Поднять оболочку заново после остановки: `kstart plasmashell` (`kstart5`
/// на Plasma 5). `kstart` запускает программу отдельно от вызывающего, так
/// что панель не умрёт вместе с Rexon; таймаута нет — запуск фоновый.
pub fn plasma_start_cmd() -> Cmd {
    Cmd {
        argv: vec!["kstart".into(), "plasmashell".into()],
        timeout_secs: None,
        lang_c: false,
    }
}

/// Запуск оболочки напрямую — только когда `kstart` нет вовсе. Отвязку от
/// нашего процесса делает исполнитель (`setsid`).
pub fn plasma_direct_start_cmd() -> Cmd {
    Cmd {
        argv: vec!["plasmashell".into()],
        timeout_secs: None,
        lang_c: false,
    }
}

// ══════════════════════════════════════════════════════════════════════════
//  Исполнитель снимков панели (`PlasmaProtector` на живой ФС)
// ══════════════════════════════════════════════════════════════════════════
//
// ЗАЧЕМ ЗДЕСЬ, А НЕ В ШИМЕ. Решения о снимках лежат выше в этом же файле
// (`plasma_snapshot_items`, `plasma_entry`, `plasma_rotate`, `plasma_pick`,
// `plasma_restore_items`), а ДЕЛАТЬ их до сих пор умел только Python
// (`PlasmaProtector` в `rexon/backup.py`). Из-за этого обе вкладки, которые
// показывают копии панели («Настройки» и «Рабочие столы»), отвечали на
// кнопку строкой «исполнителя нет». Исполнитель дописан рядом с решениями —
// там же, где уже живёт транзакционный движок восстановления, и по тем же
// правилам: прямой `std::fs`, никаких паник, любая беда возвращается словами.
//
// ⚠️ ЗАПУСКА ПРОЦЕССОВ ЗДЕСЬ НЕТ НАМЕРЕННО. Перезапуск оболочки — это
// единственный шаг, которым снимок трогает ЖИВОЙ рабочий стол, и он вынесен
// в [`plasma_restart_shell`] с ПОДСТАВНЫМИ запускателями: сверка обязана
// уметь проверить порядок «юнит → kill+start», ни разу не тронув панель
// человека.

/// Имя файла метаданных снимков панели (`PlasmaProtector._meta_file`).
pub const PLASMA_META_NAME: &str = "plasma_meta.json";

/// Путь к файлу метаданных снимков в каталоге копий.
pub fn plasma_meta_path(dir: &str) -> String {
    join(dir, PLASMA_META_NAME)
}

/// Записи мета-файла В ПОРЯДКЕ ЗАПИСИ и КАК ЕСТЬ (сырой JSON).
///
/// ⚠️ Сырыми они читаются не из лени. У [`PlasmaBackup`] стоит
/// `deny_unknown_fields`, то есть запись, которую оставила более новая версия
/// Rexon, типизированным разбором не пройдёт. Если бы мы писали файл обратно
/// из типизированного списка, такая запись ИСЧЕЗЛА БЫ вместе со ссылкой на
/// снимок — а снимок остался бы на диске мёртвым грузом. Поэтому на диск
/// всегда уезжает то же, что с него прочитано, плюс наша новая запись.
pub fn plasma_meta_entries(dir: &str) -> Vec<serde_json::Value> {
    let Ok(text) = fs::read_to_string(plasma_meta_path(dir)) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    match value.get("backups") {
        Some(serde_json::Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    }
}

/// Разбор сырых записей в типизованные. Запись незнакомой формы пропускается:
/// показать её всё равно нечем, а ронять из-за неё весь список нельзя.
pub fn plasma_parse_entries(raw: &[serde_json::Value]) -> Vec<PlasmaBackup> {
    raw.iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect()
}

/// Записать мета-файл (`_save_meta`): те же два пробела отступа и те же живые
/// буквы (не `\uXXXX`), что у `json.dump(..., indent=2, ensure_ascii=False)`.
fn plasma_write_meta(dir: &str, raw: &[serde_json::Value]) -> io::Result<()> {
    let mut root = serde_json::Map::new();
    root.insert("backups".into(), serde_json::Value::Array(raw.to_vec()));
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(root))
        .map_err(|e| io::Error::other(e.to_string()))?;
    fs::create_dir_all(dir)?;
    // Замена целиком, а не дописывание: половина мета-файла хуже, чем его
    // отсутствие — список копий перестал бы читаться вовсе. Отсюда и общая
    // дверь: сами копии панели на диске останутся, но найти их будет нечем.
    crate::utils::atomic_write(&plasma_meta_path(dir), &text)
}

/// Копия одного конфига в снимок: файл — как есть, каталог — деревом.
pub(crate) fn copy_config_into(src: &str, dst: &str) -> io::Result<()> {
    let src_p = Path::new(src);
    if src_p.is_dir() {
        copy_tree(src_p, Path::new(dst))
    } else {
        if let Some(parent) = Path::new(dst).parent() {
            fs::create_dir_all(parent)?;
        }
        copy_file_like(src_p, Path::new(dst))
    }
}

/// Снять копию панели (`backup_panel`) → `(получилось ли, что сказать)`.
///
/// `Err` — то, что у Python было ИСКЛЮЧЕНИЕМ (каталог не создался, мета-файл
/// не записался): вкладка показывает такую беду отдельным текстом
/// («Не удалось сохранить копию: …»), а не общим итогом.
///
/// `timestamp` приходит снаружи: часы — не решение, а наблюдение, и подставив
/// метку, сверка получает воспроизводимый снимок.
pub fn plasma_backup_run(
    dir: &str,
    configs: &[String],
    reason: &str,
    full: bool,
    timestamp: &str,
) -> Result<(bool, String), String> {
    let snap_dir = join(dir, timestamp);
    fs::create_dir_all(&snap_dir).map_err(|e| e.to_string())?;

    let mut saved: Vec<String> = Vec::new();
    for (src, dst) in plasma_snapshot_items(configs, &snap_dir, |p| Path::new(p).exists()) {
        // Не скопировался один конфиг — не повод бросать остальные: у
        // оригинала здесь `try/except` вокруг КАЖДОГО файла.
        if copy_config_into(&src, &dst).is_ok() {
            saved.push(src);
        }
    }

    if saved.is_empty() {
        // Пустой каталог снимка на диске — мусор, который потом выглядит как
        // сломанная копия.
        let _ = fs::remove_dir_all(&snap_dir);
        return Ok((false, msg_nothing_to_backup()));
    }

    let entry = plasma_entry(timestamp, reason, &snap_dir, &saved, full);
    let mut raw = plasma_meta_entries(dir);
    raw.push(serde_json::to_value(&entry).map_err(|e| e.to_string())?);

    // Ротация: какие снимки выбросить — решает ядро выше, здесь только
    // `rmtree` и вычёркивание их записей из сырого списка ПО ПУТИ снимка.
    let mut parsed = plasma_parse_entries(&raw);
    let dropped = plasma_rotate(&mut parsed);
    for path in &dropped {
        let _ = fs::remove_dir_all(path);
    }
    if !dropped.is_empty() {
        raw.retain(|v| {
            let path = v.get("path").and_then(|p| p.as_str()).unwrap_or_default();
            !dropped.iter().any(|d| d == path)
        });
    }

    plasma_write_meta(dir, &raw).map_err(|e| e.to_string())?;
    Ok((true, msg_backup_created(saved.len())))
}

/// Вернуть копию панели (`restore_panel`) → `(получилось ли, что сказать)`.
///
/// Перезапуск оболочки сюда НЕ входит: решение «перезапускать ли» зависит от
/// того, работает ли Plasma прямо сейчас, и принимается там же, где видно
/// окружение сессии ([`is_plasma_desktop`]).
pub fn plasma_restore_run(dir: &str, timestamp: Option<&str>) -> (bool, String) {
    let raw = plasma_meta_entries(dir);
    let backups = plasma_parse_entries(&raw);
    let entry = match plasma_pick(&backups, timestamp) {
        Ok(entry) => entry.clone(),
        Err(message) => return (false, message),
    };

    if !Path::new(&entry.path).is_dir() {
        return (false, msg_snapshot_dir_missing(&entry.path));
    }

    let items = plasma_restore_items(&entry, |p| Path::new(p).exists());
    let outcome = transactional_restore(&items);
    if !outcome.ok {
        return (false, msg_restore_failed(&outcome.message));
    }
    if outcome.restored == 0 {
        return (false, msg_none_restored());
    }
    (true, msg_restored_files(outcome.restored, &entry.timestamp))
}

/// Чем кончился перезапуск оболочки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellRestart {
    /// Юнит systemd перезапустил оболочку сам.
    Unit,
    /// `kquitapp` остановил оболочку, `kstart` поднял заново.
    QuitAndStart,
    /// Остановлена оболочка ТЕКУЩЕГО сеанса по её PID, затем поднята заново.
    SessionPid,
    /// Ни один путь не сработал: юнита нет, `kquitapp` не помог, а оболочки
    /// своего сеанса не нашлось. Чужие процессы при этом не тронуты.
    NotRestarted,
}

/// Хвост сообщения о возврате панели, если оболочку перезапустить не вышло.
pub fn msg_plasmashell_not_restarted() -> String {
    format!(
        "\n{}",
        tr("The panel didn't restart on its own — log out and back in to see the restored panel")
    )
}

/// Хвост сообщения о возврате панели по итогу перезапуска.
pub fn plasma_restart_message(outcome: ShellRestart) -> String {
    match outcome {
        ShellRestart::NotRestarted => msg_plasmashell_not_restarted(),
        _ => msg_plasmashell_restarted(),
    }
}

/// Руки перезапуска. Подставляемые: сверка порядка шагов не должна трогать
/// живую панель человека, поэтому в тестах вместо них — запись шагов.
pub trait ShellOps {
    /// Запустить и дождаться; `true` — код возврата 0.
    fn run(&self, cmd: &Cmd) -> bool;
    /// Запустить отдельно от нашего процесса и не ждать.
    fn spawn(&self, cmd: &Cmd) -> bool;
    /// Есть ли инструмент (с учётом синонимов `utils::TOOL_SETS`).
    fn has(&self, tool: &str) -> bool;
    /// PID оболочек ТЕКУЩЕГО сеанса (см. [`is_session_shell`]).
    fn session_shell_pids(&self) -> Vec<u32>;
    /// Попросить процесс завершиться (SIGTERM); `true` — сигнал доставлен.
    fn terminate(&self, pid: u32) -> bool;
    /// Пауза между остановкой и запуском.
    fn settle(&self);
}

/// Перезапустить оболочку Plasma — решение и исполнение в ОДНОМ месте на
/// весь продукт.
///
/// Порядок — штатная последовательность KDE, от мягкого к точному:
///   1. `systemctl --user restart plasma-plasmashell.service` — юнит сам
///      останавливает и поднимает оболочку (второй запуск дал бы две панели);
///   2. `kquitapp6 plasmashell` + `kstart plasmashell` (`*5` на Plasma 5) —
///      только если есть ОБА: остановить и не поднять хуже, чем не трогать;
///   3. PID оболочки ТЕКУЩЕГО сеанса: тот же uid, `/proc/PID/exe` —
///      `plasmashell`, тот же `XDG_SESSION_ID`; сигнал по PID, затем запуск.
///
/// ⚠️ Убийства по имени (`killall plasmashell`) здесь нет и быть не должно:
/// оно задевает оболочку любого сеанса этого человека. Не нашлось своей
/// оболочки — [`ShellRestart::NotRestarted`], и ничего не тронуто.
pub fn plasma_restart_shell_with(ops: &impl ShellOps) -> ShellRestart {
    if ops.run(&plasma_quit_cmd()) {
        return ShellRestart::Unit;
    }
    let can_start = ops.has("kstart");
    if can_start && ops.has("kquitapp6") && ops.run(&plasma_kill_cmd()) {
        ops.settle();
        ops.spawn(&plasma_start_cmd());
        return ShellRestart::QuitAndStart;
    }
    let mut stopped = false;
    for pid in ops.session_shell_pids() {
        stopped |= ops.terminate(pid);
    }
    if !stopped {
        return ShellRestart::NotRestarted;
    }
    ops.settle();
    let start = if can_start {
        plasma_start_cmd()
    } else {
        plasma_direct_start_cmd()
    };
    ops.spawn(&start);
    ShellRestart::SessionPid
}

/// Перезапустить оболочку Plasma на живой машине (`_restart_plasmashell`).
pub fn plasma_restart_shell() -> ShellRestart {
    plasma_restart_shell_with(&LiveShellOps)
}

/// Оболочка ли это ТЕКУЩЕГО сеанса.
///
/// Три признака сразу, и ни один не лишний: uid отсекает чужих людей,
/// `exe` — чужие программы с похожим именем, `XDG_SESSION_ID` — оболочку
/// другого сеанса того же человека (второй вход, удалённый сеанс). Своего
/// `XDG_SESSION_ID` нет — не угадываем, а не трогаем никого.
pub fn is_session_shell(
    exe: &str,
    proc_uid: u32,
    environ: &[u8],
    my_uid: u32,
    my_session: &str,
) -> bool {
    is_session_process(exe, "plasmashell", proc_uid, environ, my_uid, my_session)
}

/// Процесс ли это программы `name` ТЕКУЩЕГО сеанса — те же три признака,
/// что у [`is_session_shell`].
pub fn is_session_process(
    exe: &str,
    name: &str,
    proc_uid: u32,
    environ: &[u8],
    my_uid: u32,
    my_session: &str,
) -> bool {
    if my_session.is_empty() || proc_uid != my_uid {
        return false;
    }
    let exe = exe.strip_suffix(" (deleted)").unwrap_or(exe);
    if Path::new(exe).file_name().and_then(|n| n.to_str()) != Some(name) {
        return false;
    }
    let wanted = format!("XDG_SESSION_ID={my_session}");
    environ.split(|b| *b == 0).any(|kv| kv == wanted.as_bytes())
}

/// Живые руки перезапуска. `PATH` берётся из окружения процесса: без него
/// резолвер смотрел бы в запасной список каталогов, а не туда, где человек
/// (или стенд проверки) держит свои инструменты.
struct LiveShellOps;

impl LiveShellOps {
    fn path_env() -> Option<String> {
        std::env::var("PATH").ok()
    }
}

impl ShellOps for LiveShellOps {
    fn run(&self, cmd: &Cmd) -> bool {
        let spec = crate::utils::RunSpec {
            argv: cmd.argv.clone(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs: cmd.timeout_secs,
            lang_c: cmd.lang_c,
            ..Default::default()
        };
        crate::utils::run(&spec, Self::path_env().as_deref()).returncode == Some(0)
    }

    fn spawn(&self, cmd: &Cmd) -> bool {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};

        let argv = crate::utils::resolved_argv(&cmd.argv, Self::path_env().as_deref());
        let Some((head, rest)) = argv.split_first() else {
            return false;
        };
        let mut command = Command::new(head);
        command
            .args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Свой сеанс: панель не должна умереть вместе с Rexon.
        // SAFETY: между fork и exec зовётся только setsid — он безопасен
        // для асинхронного сигнала.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        match command.spawn() {
            Ok(mut child) => {
                // Забрать код завершения, чтобы не оставить зомби.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                true
            }
            Err(_) => false,
        }
    }

    fn has(&self, tool: &str) -> bool {
        crate::utils::resolve_tool(tool, &[], Self::path_env().as_deref()).is_some()
    }

    fn session_shell_pids(&self) -> Vec<u32> {
        use std::os::unix::fs::MetadataExt;

        let my_session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
        if my_session.is_empty() {
            return Vec::new();
        }
        // SAFETY: getuid не имеет предусловий и не может провалиться.
        let my_uid = unsafe { libc::getuid() };
        let me = std::process::id();
        let Ok(entries) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        entries
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
            .filter(|pid| *pid != me)
            .filter(|pid| {
                let base = format!("/proc/{pid}");
                let (Ok(meta), Ok(exe), Ok(environ)) = (
                    fs::metadata(&base),
                    fs::read_link(format!("{base}/exe")),
                    fs::read(format!("{base}/environ")),
                ) else {
                    return false;
                };
                is_session_shell(&exe.to_string_lossy(), meta.uid(), &environ, my_uid, &my_session)
            })
            .collect()
    }

    fn terminate(&self, pid: u32) -> bool {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: kill с конкретным положительным PID и SIGTERM — обычный
        // системный вызов без требований к памяти.
        unsafe { libc::kill(pid, libc::SIGTERM) == 0 }
    }

    fn settle(&self) {
        std::thread::sleep(std::time::Duration::from_secs(PLASMASHELL_SETTLE_SECS));
    }
}

// ══════════════════════════════════════════════════════════════════════════
//  Сверки исполнителя
// ══════════════════════════════════════════════════════════════════════════
//
// ⚠️ ЖИВЫХ ПАНЕЛЕЙ ЗДЕСЬ НЕТ И БЫТЬ НЕ МОЖЕТ. Всё, что ниже, работает в
// подставном каталоге во временной папке: свои «конфиги», свой каталог
// копий, свой мета-файл. Ни один путь не ведёт в `~/.config`, а перезапуск
// оболочки проверяется подставными запускателями, которые ничего не
// запускают.
// ============================================================================
// Исходная панель: снимок до первого вмешательства Rexon
// ============================================================================
//
// Решение Wehrwolfmann 20.10.2026: «Вернуть прежнюю панель» возвращает
// панель, какой она была у человека ДО первого вмешательства Rexon, а не
// предыдущую раскладку Rexon. Раньше каждое применение раскладки
// перезаписывало единственную копию: после «Как Windows» → «Классическая»
// возврат давал «Как Windows».
//
// Поэтому копия панели (одна на стол, путь задаёт сам стол) — исходная: её
// снимает первое вмешательство, последующие её не трогают и кладут свой
// снимок рядом (`<копия>.last`, для будущего «шага назад»; кнопки нет).
// Вернули исходную — рядом ложится отметка `<копия>.returned`, и следующее
// вмешательство снова снимает исходную: панель после возврата и есть
// панель человека. «Сохранить панель» — выбор самого человека: копия
// заменяется (после вопроса) и отметка снимается. Копия без отметки,
// снятая прежними выпусками, считается исходной.

/// Содержимое отметки «исходную панель уже вернули».
pub const PANEL_ORIGINAL_RETURNED: &str = "returned";

/// Отметка «исходную панель вернули» рядом с копией `copy`.
pub fn panel_original_returned_mark(copy: &str) -> String {
    format!("{copy}.returned")
}

/// Последний снимок перед вмешательством Rexon, когда исходная копия уже
/// есть (кнопки «шаг назад» пока нет — файл для неё).
pub fn panel_last_snapshot_path(copy: &str) -> String {
    format!("{copy}.last")
}

/// Куда положить снимок панели перед вмешательством Rexon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSnapshotSlot {
    /// Исходная копия: её ещё нет либо её уже вернули.
    Original,
    /// Рядом ([`panel_last_snapshot_path`]): исходная копия стоит.
    Last,
}

/// Решение по копии `copy_exists` и тексту отметки `returned_mark`
/// ([`panel_original_returned_mark`], `None` — файла нет).
pub fn panel_snapshot_slot(copy_exists: bool, returned_mark: Option<&str>) -> PanelSnapshotSlot {
    let returned = returned_mark.is_some_and(|m| m.trim() == PANEL_ORIGINAL_RETURNED);
    if copy_exists && !returned {
        PanelSnapshotSlot::Last
    } else {
        PanelSnapshotSlot::Original
    }
}

// ============================================================================
// Копия панели XFCE
// ============================================================================
//
// Снимает и возвращает её `xfce4-panel-profiles` (`save <файл>` /
// `load <файл>`, CLI программы): архив канала xfconf `xfce4-panel` и
// файлов `~/.config/xfce4/panel`, при загрузке панель перезапускается им
// же. Копия одна — у «Сохранить панель» и «Вернуть панель» вкладки
// «Рабочие столы».

/// Программа копий панели XFCE.
pub const XFCE_PANEL_PROFILES: &str = "xfce4-panel-profiles";

/// Файл копии панели XFCE (`data_home` — `$XDG_DATA_HOME` либо
/// `~/.local/share`).
pub fn xfce_panel_copy_path(data_home: &str) -> String {
    format!("{}/rexon/backups/xfce4-panel.tar.bz2", data_home.trim_end_matches('/'))
}

/// Команда снятия копии.
pub fn xfce_panel_save_argv(path: &str) -> Vec<String> {
    vec![XFCE_PANEL_PROFILES.to_string(), "save".to_string(), path.to_string()]
}

/// Команда возврата из копии.
pub fn xfce_panel_load_argv(path: &str) -> Vec<String> {
    vec![XFCE_PANEL_PROFILES.to_string(), "load".to_string(), path.to_string()]
}

/// Снять копию панели XFCE → `(удалось, сообщение)`.
pub fn xfce_panel_save_live(data_home: &str) -> (bool, String) {
    let path = xfce_panel_copy_path(data_home);
    if let Some(dir) = Path::new(&path).parent() {
        if let Err(e) = fs::create_dir_all(dir) {
            return (false, e.to_string());
        }
    }
    let (ok, out) = crate::animations::LookIo::run(&crate::animations::SystemIo, &xfce_panel_save_argv(&path));
    if ok && Path::new(&path).is_file() {
        (true, path)
    } else {
        (false, if out.trim().is_empty() { XFCE_PANEL_PROFILES.to_string() } else { out.trim().to_string() })
    }
}

/// Вернуть панель XFCE из копии → `(удалось, сообщение)`.
pub fn xfce_panel_restore_live(data_home: &str) -> (bool, String) {
    let path = xfce_panel_copy_path(data_home);
    if !Path::new(&path).is_file() {
        return (false, tr("There is no saved panel copy yet."));
    }
    let (ok, out) = crate::animations::LookIo::run(&crate::animations::SystemIo, &xfce_panel_load_argv(&path));
    if ok {
        (true, path)
    } else {
        (false, if out.trim().is_empty() { XFCE_PANEL_PROFILES.to_string() } else { out.trim().to_string() })
    }
}

// ============================================================================
// Копия панели прочих окружений (Cinnamon, Budgie, LXQt, GNOME с панельными
// расширениями, Pantheon, COSMIC, бары тайловых композиторов)
// ============================================================================
//
// Одна копия на окружение: `~/.local/share/rexon/backups/panel-<стол>/`.
// Что снимать, у каждого стола своё ([`panel_copy_spec`]): ветки dconf
// (целиком или ключи корня по приставке), файлы и каталоги от HOME. В копии
// лежит `manifest.json` — что именно снято, — и «Вернуть» возвращает ровно
// это. Новая копия собирается рядом (`.new`) и встаёт на место только
// целиком: неудачное снятие прежнюю копию не портит. Как панель подхватит
// вернувшееся, тоже решает стол ([`PanelReload`]): большинство следит за
// своими настройками само.

/// Что входит в копию панели.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelPart {
    /// Ветка dconf целиком (при возврате — сперва сброс ветки).
    DconfDir(&'static str),
    /// Ключи корня ветки dconf, чьи имена начинаются с одной из приставок
    /// (при возврате — дописываются, остальные ключи ветки не трогаются).
    DconfKeys { dir: &'static str, prefixes: &'static [&'static str] },
    /// Файл или каталог от HOME (`.config/lxqt/panel.conf`).
    Home(String),
    /// Каталоги в `parent` (от HOME), чьи имена начинаются с `prefix`.
    HomeMatching { parent: &'static str, prefix: &'static str },
    /// Ресурс DConfig Deepin целиком: все записываемые ключи его схемы
    /// (`/usr/share/dsg/configs/<appid>/<resource>.json`) через `dde-dconfig`.
    /// Ключ со значением по умолчанию при возврате сбрасывается, а не
    /// записывается.
    DdeDconfig { appid: &'static str, resource: &'static str },
}

/// Как панель подхватывает вернувшиеся настройки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelReload {
    /// Сама: следит за своими настройками (dconf, файлы).
    Live,
    /// Запустить программу отдельным сеансом (`budgie-panel --replace`).
    Spawn(&'static [&'static str]),
    /// Модуль сеанса LXQt: остановить и запустить через `org.lxqt.session`.
    LxqtModule(&'static str),
    /// Послать SIGUSR2 процессам своего сеанса с этим именем (waybar
    /// перечитывает по нему настройки).
    ReloadSignal(&'static str),
    /// Перезапустить пользовательскую службу systemd, если она идёт
    /// (`systemctl --user --no-block try-restart`).
    RestartUserUnit(&'static str),
}

/// Что снимать у стола и как потом перечитать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelCopySpec {
    pub parts: Vec<PanelPart>,
    pub reload: Vec<PanelReload>,
}

/// Что машина говорит о столе — для выбора частей копии.
pub struct PanelProbe<'a> {
    /// Есть ли программа в системе.
    pub have: &'a dyn Fn(&str) -> bool,
    /// Есть ли путь от HOME.
    pub home_exists: &'a dyn Fn(&str) -> bool,
    /// Включённые расширения GNOME Shell (`enabled-extensions`).
    pub gnome_extensions: &'a [String],
    /// Есть ли файл по абсолютному пути (схемы DConfig Deepin).
    pub system_exists: &'a dyn Fn(&str) -> bool,
}

/// Приложение DConfig, под которым dde-shell (док Deepin V23/V25) хранит
/// настройки с выпуска 0.0.35 (август 2024, «correct app id used by
/// DConfig»); прежнее `org.deepin.ds.dock` осталось лишь копией схем для
/// совместимости.
pub const DDE_SHELL_DCONFIG_APPID: &str = "org.deepin.dde.shell";

/// Ресурсы дока dde-shell: сам док (размер, место, скрытие, плагины),
/// область уведомлений и панель задач (закреплённые программы).
pub const DDE_DOCK_RESOURCES: &[&str] =
    &["org.deepin.ds.dock", "org.deepin.ds.dock.tray", "org.deepin.ds.dock.taskmanager"];

/// Служба systemd дока Deepin (dde-session): `dde-shell -C DDE`.
pub const DDE_DOCK_UNIT: &str = "dde-shell@DDE.service";

/// Расширения GNOME Shell, которые и есть панель или док, и их ветка dconf.
pub const GNOME_PANEL_EXTENSIONS: &[(&str, &str)] = &[
    ("dash-to-dock@micxgx.gmail.com", "/org/gnome/shell/extensions/dash-to-dock/"),
    ("ubuntu-dock@ubuntu.com", "/org/gnome/shell/extensions/dash-to-dock/"),
    ("dash-to-panel@jderose9.github.com", "/org/gnome/shell/extensions/dash-to-panel/"),
];

/// Что снимать у стола `de` (ключ Rexon). `None` — снимать нечего: кнопок
/// копии панели у такого стола нет. KDE, MATE и XFCE снимаются своими путями
/// (`plasma_backup_run`, `mate_panel`, `xfce4-panel-profiles`).
pub fn panel_copy_spec(de: &str, probe: &PanelProbe) -> Option<PanelCopySpec> {
    match de {
        // Budgie: всё о панелях — ветка `com.solus-project.budgie-panel`
        // (панели, `instance/` апплетов, их настройки). Готовые панели
        // budgie-panel из dconf не перестраивает — `--replace` поднимает их
        // заново (так Budgie перезапускают и сами).
        // LXQt: панели и их модули — один файл `~/.config/lxqt/panel.conf`.
        // Свой файл панель сама не перечитывает; сеанс LXQt останавливает и
        // снова запускает её модуль (`org.lxqt.session`, `stopModule` /
        // `startModule`, имя модуля — файл автозапуска).
        "lxqt" if (probe.have)("lxqt-panel") => Some(PanelCopySpec {
            parts: vec![PanelPart::Home(".config/lxqt/panel.conf".to_string())],
            reload: vec![PanelReload::LxqtModule("lxqt-panel.desktop")],
        }),
        // GNOME: своей панели нет — копия имеет смысл, только когда панель
        // или док дают включённые расширения. Их ветки dconf и сам список
        // включённых (`org.gnome.shell enabled-extensions`); расширения
        // следят за своими настройками сами.
        "gnome" => {
            let mut parts: Vec<PanelPart> = Vec::new();
            for (uuid, dir) in GNOME_PANEL_EXTENSIONS {
                let part = PanelPart::DconfDir(dir);
                if probe.gnome_extensions.iter().any(|e| e == uuid) && !parts.contains(&part) {
                    parts.push(part);
                }
            }
            if parts.is_empty() {
                return None;
            }
            parts.push(PanelPart::DconfKeys { dir: "/org/gnome/shell/", prefixes: &["enabled-extensions"] });
            Some(PanelCopySpec { parts, reload: vec![PanelReload::Live] })
        }
        // Pantheon: верхняя панель Wingpanel (`io.elementary.desktop.wingpanel`)
        // и стоящий док — Plank (`net.launchpad.plank`, значки дока — файлы
        // `~/.config/plank`) или новый док elementary (`io.elementary.dock`).
        // Все следят за своими настройками сами.
        "pantheon" if (probe.have)("io.elementary.wingpanel") => {
            let mut parts = vec![PanelPart::DconfDir("/io/elementary/desktop/wingpanel/")];
            if (probe.have)("plank") {
                parts.push(PanelPart::DconfDir("/net/launchpad/plank/"));
                parts.push(PanelPart::Home(".config/plank".to_string()));
            }
            if (probe.have)("io.elementary.dock") {
                parts.push(PanelPart::DconfDir("/io/elementary/dock/"));
            }
            Some(PanelCopySpec { parts, reload: vec![PanelReload::Live] })
        }
        // Wayfire: панель wf-panel и фон wf-background (wf-shell) — один файл
        // `~/.config/wf-shell.ini`; wf-shell следит за ним сам (inotify) и
        // перечитывает при записи.
        "wayfire" if (probe.have)("wf-panel") => Some(PanelCopySpec {
            parts: vec![PanelPart::Home(".config/wf-shell.ini".to_string())],
            reload: vec![PanelReload::Live],
        }),
        // COSMIC: панель, док и их кнопки — каталоги настроек
        // `~/.config/cosmic/com.system76.CosmicPanel*` (cosmic-config); за
        // ними панель следит сама.
        "cosmic" if (probe.have)("cosmic-panel") => Some(PanelCopySpec {
            parts: vec![PanelPart::HomeMatching { parent: ".config/cosmic", prefix: "com.system76.CosmicPanel" }],
            reload: vec![PanelReload::Live],
        }),
        // Тайловые композиторы: панель — это бар. Конфиги того, что стоит:
        // waybar (`~/.config/waybar`, перечитывает его по SIGUSR2), оболочки
        // Noctalia (`~/.config/noctalia`) и DankMaterialShell
        // (`~/.config/DankMaterialShell`), которые следят за своими
        // настройками сами. River — тоже с waybar (её поднимает стартовый
        // init Rexon).
        "hyprland" | "niri" | "sway" | "river" => {
            let mut parts = Vec::new();
            let mut reload = Vec::new();
            if (probe.have)("waybar") && (probe.home_exists)(".config/waybar") {
                parts.push(PanelPart::Home(".config/waybar".to_string()));
                reload.push(PanelReload::ReloadSignal("waybar"));
            }
            for dir in [".config/noctalia", ".config/DankMaterialShell"] {
                if (probe.home_exists)(dir) {
                    parts.push(PanelPart::Home(dir.to_string()));
                }
            }
            if parts.is_empty() {
                return None;
            }
            if reload.is_empty() {
                reload.push(PanelReload::Live);
            }
            Some(PanelCopySpec { parts, reload })
        }
        // Deepin V23/V25: док — dde-shell, его настройки — три ресурса
        // DConfig приложения `org.deepin.dde.shell`. Док за своими ключами
        // следит лишь частично (область уведомлений подхватывает только
        // скрытые значки), поэтому после возврата Deepin сам перезапускает
        // службу дока. Другой выпуск (прежний dde-dock deepin 20, dde-shell
        // до 0.0.35 со своим приложением DConfig) — кнопок нет: ключей его
        // мы не знаем.
        "deepin"
            if (probe.have)("dde-dconfig")
                && (probe.have)("dde-shell")
                && DDE_DOCK_RESOURCES.iter().all(|r| {
                    (probe.system_exists)(&crate::dde::dconfig_schema_path(DDE_SHELL_DCONFIG_APPID, r))
                }) =>
        {
            Some(PanelCopySpec {
                parts: DDE_DOCK_RESOURCES
                    .iter()
                    .map(|r| PanelPart::DdeDconfig { appid: DDE_SHELL_DCONFIG_APPID, resource: r })
                    .collect(),
                reload: vec![PanelReload::RestartUserUnit(DDE_DOCK_UNIT)],
            })
        }
        "budgie" if (probe.have)("budgie-panel") => Some(PanelCopySpec {
            parts: vec![PanelPart::DconfDir("/com/solus-project/budgie-panel/")],
            reload: vec![PanelReload::Spawn(&["budgie-panel", "--replace"])],
        }),
        // Панели и апплеты Cinnamon — ключи `org.cinnamon` (`panels-*`,
        // `panel-zone-*`, `enabled-applets`, `next-applet-id`), настройки
        // апплетов — файлы `~/.config/cinnamon/spices/`. Cinnamon следит за
        // обоими сам (смена `enabled-applets` перестраивает панели).
        "cinnamon" | "cinnamon-mint" => Some(PanelCopySpec {
            parts: vec![
                PanelPart::DconfKeys {
                    dir: "/org/cinnamon/",
                    prefixes: &["panels-", "panel-zone-", "enabled-applets", "next-applet-id"],
                },
                PanelPart::Home(".config/cinnamon/spices".to_string()),
            ],
            reload: vec![PanelReload::Live],
        }),
        _ => None,
    }
}

/// Каталог копии панели стола `de`.
pub fn panel_copy_dir(data_home: &str, de: &str) -> String {
    format!("{}/rexon/backups/panel-{de}", data_home.trim_end_matches('/'))
}

/// Руки копии панели: команды, файлы, процессы. Живые — [`LivePanelIo`].
pub trait PanelIo {
    fn run(&self, argv: &[String]) -> (bool, String);
    fn read(&self, path: &str) -> Option<String>;
    fn write(&self, path: &str, text: &str) -> bool;
    fn exists(&self, path: &str) -> bool;
    fn list_dir(&self, path: &str) -> Vec<String>;
    fn copy_tree(&self, src: &str, dest: &str) -> Result<(), String>;
    fn remove_tree(&self, path: &str) -> Result<(), String>;
    fn rename(&self, from: &str, to: &str) -> Result<(), String>;
    fn spawn(&self, argv: &[String]) -> bool;
    /// Послать SIGUSR2 процессам своего сеанса с именем `exe`; сколько.
    fn reload_signal(&self, exe: &str) -> usize;
    fn pause(&self);
}

fn sv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Годится ли путь копии/манифеста: относительный, без `..`.
fn safe_rel(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

/// Ключи корня дампа dconf (группа `[/]`), чьи имена начинаются с одной из
/// `prefixes`, — готовым дампом для `dconf load`.
pub fn dconf_root_keys(dump: &str, prefixes: &[&str]) -> String {
    let mut out = String::new();
    let mut in_root = false;
    for line in dump.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_root = t == "[/]";
            continue;
        }
        if !in_root {
            continue;
        }
        let key = t.split('=').next().unwrap_or("").trim();
        if prefixes.iter().any(|p| key.starts_with(p)) {
            out.push_str(line);
            out.push('\n');
        }
    }
    if out.is_empty() {
        out
    } else {
        format!("[/]\n{out}")
    }
}

/// Снять копию панели по `spec` в `copy_dir` → путь копии либо причина.
pub fn panel_copy_save(io: &dyn PanelIo, spec: &PanelCopySpec, home: &str, copy_dir: &str) -> Result<String, String> {
    let home = home.trim_end_matches('/');
    let tmp = format!("{copy_dir}.new");
    io.remove_tree(&tmp)?;
    let mut manifest: Vec<serde_json::Value> = Vec::new();
    for (i, part) in spec.parts.iter().enumerate() {
        match part {
            PanelPart::DconfDir(dir) | PanelPart::DconfKeys { dir, .. } => {
                let (ok, dump) = io.run(&sv(&["dconf", "dump", dir]));
                if !ok {
                    return Err(dump);
                }
                let (kind, text) = match part {
                    PanelPart::DconfKeys { prefixes, .. } => ("dconf_keys", dconf_root_keys(&dump, prefixes)),
                    _ => ("dconf_dir", dump),
                };
                let file = format!("dconf-{i}.ini");
                if !io.write(&format!("{tmp}/{file}"), &text) {
                    return Err(format!("{tmp}/{file}"));
                }
                manifest.push(serde_json::json!({ "kind": kind, "dir": dir, "file": file }));
            }
            PanelPart::Home(rel) => {
                if io.exists(&format!("{home}/{rel}")) {
                    io.copy_tree(&format!("{home}/{rel}"), &format!("{tmp}/home/{rel}"))?;
                    manifest.push(serde_json::json!({ "kind": "home", "path": rel }));
                }
            }
            PanelPart::HomeMatching { parent, prefix } => {
                let names: Vec<String> =
                    io.list_dir(&format!("{home}/{parent}")).into_iter().filter(|n| n.starts_with(prefix)).collect();
                for n in &names {
                    io.copy_tree(&format!("{home}/{parent}/{n}"), &format!("{tmp}/home/{parent}/{n}"))?;
                }
                manifest.push(serde_json::json!({ "kind": "home_matching", "parent": parent, "prefix": prefix, "names": names }));
            }
            PanelPart::DdeDconfig { appid, resource } => {
                let schema_path = crate::dde::dconfig_schema_path(appid, resource);
                let Some(schema) = io.read(&schema_path) else {
                    return Err(schema_path);
                };
                let mut values = serde_json::Map::new();
                for key in crate::dde::dconfig_schema_writable_keys(&schema) {
                    let (ok, raw) = io.run(&crate::dde::dconfig_get_key_argv(appid, resource, &key));
                    if !ok {
                        return Err(raw);
                    }
                    let (_, default) = io.run(&crate::dde::dconfig_is_default_argv(appid, resource, &key));
                    values.insert(
                        key,
                        serde_json::json!({
                            "value": crate::dde::dconfig_cli_value(&raw),
                            "default": default.trim() == "true",
                        }),
                    );
                }
                let file = format!("dde-{i}.json");
                let body = serde_json::json!({ "appid": appid, "resource": resource, "values": values });
                if !io.write(&format!("{tmp}/{file}"), &body.to_string()) {
                    return Err(format!("{tmp}/{file}"));
                }
                manifest.push(serde_json::json!({ "kind": "dde_dconfig", "appid": appid, "resource": resource, "file": file }));
            }
        }
    }
    if !io.write(&format!("{tmp}/manifest.json"), &serde_json::Value::Array(manifest).to_string()) {
        return Err(format!("{tmp}/manifest.json"));
    }
    io.remove_tree(copy_dir)?;
    io.rename(&tmp, copy_dir)?;
    Ok(copy_dir.to_string())
}

/// Вернуть панель из копии `copy_dir` и дать столу её перечитать.
pub fn panel_copy_restore(io: &dyn PanelIo, spec: &PanelCopySpec, home: &str, copy_dir: &str) -> Result<String, String> {
    let home = home.trim_end_matches('/');
    let Some(text) = io.read(&format!("{copy_dir}/manifest.json")) else {
        return Err(tr("There is no saved panel copy yet."));
    };
    let entries: Vec<serde_json::Value> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
    for e in &entries {
        match s(e, "kind").as_str() {
            kind @ ("dconf_dir" | "dconf_keys") => {
                let (dir, file) = (s(e, "dir"), s(e, "file"));
                // Только ветки, которые этот стол и снимает: манифест — файл
                // на диске, чужую ветку из него не сбрасываем.
                let known = spec.parts.iter().any(|p| matches!(p, PanelPart::DconfDir(d) | PanelPart::DconfKeys { dir: d, .. } if *d == dir));
                if !known || !safe_rel(&file) {
                    continue;
                }
                if kind == "dconf_dir" {
                    let (ok, out) = io.run(&sv(&["dconf", "reset", "-f", &dir]));
                    if !ok {
                        return Err(out);
                    }
                }
                let path = format!("{copy_dir}/{file}");
                let (ok, out) = io.run(&sv(&["sh", "-c", "dconf load \"$1\" < \"$2\"", "sh", &dir, &path]));
                if !ok {
                    return Err(out);
                }
            }
            "home" => {
                let rel = s(e, "path");
                if !safe_rel(&rel) {
                    continue;
                }
                io.remove_tree(&format!("{home}/{rel}"))?;
                io.copy_tree(&format!("{copy_dir}/home/{rel}"), &format!("{home}/{rel}"))?;
            }
            "home_matching" => {
                let (parent, prefix) = (s(e, "parent"), s(e, "prefix"));
                if !safe_rel(&parent) || prefix.is_empty() || prefix.contains('/') {
                    continue;
                }
                for n in io.list_dir(&format!("{home}/{parent}")).into_iter().filter(|n| n.starts_with(&prefix)) {
                    io.remove_tree(&format!("{home}/{parent}/{n}"))?;
                }
                let names: Vec<String> = e
                    .get("names")
                    .and_then(serde_json::Value::as_array)
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                for n in names.iter().filter(|n| n.starts_with(&prefix) && safe_rel(n) && !n.contains('/')) {
                    io.copy_tree(&format!("{copy_dir}/home/{parent}/{n}"), &format!("{home}/{parent}/{n}"))?;
                }
            }
            "dde_dconfig" => {
                let (appid, resource, file) = (s(e, "appid"), s(e, "resource"), s(e, "file"));
                // Только ресурсы, которые этот стол и снимает.
                let known = spec.parts.iter().any(
                    |p| matches!(p, PanelPart::DdeDconfig { appid: a, resource: r } if *a == appid && *r == resource),
                );
                if !known || !safe_rel(&file) {
                    continue;
                }
                let Some(text) = io.read(&format!("{copy_dir}/{file}")) else {
                    return Err(format!("{copy_dir}/{file}"));
                };
                let body: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                let Some(values) = body.get("values").and_then(serde_json::Value::as_object) else {
                    continue;
                };
                for (key, v) in values {
                    if !crate::dde::dconfig_key_ok(key) {
                        continue;
                    }
                    let argv = if v.get("default").and_then(serde_json::Value::as_bool) == Some(true) {
                        crate::dde::dconfig_reset_key_argv(&appid, &resource, key)
                    } else {
                        crate::dde::dconfig_set_key_argv(&appid, &resource, key, &s(v, "value"))
                    };
                    let (ok, out) = io.run(&argv);
                    if !ok {
                        return Err(out);
                    }
                }
            }
            _ => {}
        }
    }
    for r in &spec.reload {
        match r {
            PanelReload::Live => {}
            PanelReload::Spawn(argv) => {
                io.spawn(&sv(argv));
            }
            PanelReload::LxqtModule(module) => {
                let call = |method: &str| {
                    sv(&[
                        "dbus-send",
                        "--session",
                        "--type=method_call",
                        "--dest=org.lxqt.session",
                        "/LXQtSession",
                        &format!("org.lxqt.session.{method}"),
                        &format!("string:{module}"),
                    ])
                };
                io.run(&call("stopModule"));
                io.pause();
                io.run(&call("startModule"));
            }
            PanelReload::ReloadSignal(exe) => {
                io.reload_signal(exe);
            }
            PanelReload::RestartUserUnit(unit) => {
                io.run(&sv(&["systemctl", "--user", "--no-block", "try-restart", unit]));
            }
        }
    }
    Ok(copy_dir.to_string())
}

/// Живые руки копии панели.
pub struct LivePanelIo;

fn copy_tree_keep_links(src: &Path, dest: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        if let Some(p) = dest.parent() {
            fs::create_dir_all(p)?;
        }
        let target = fs::read_link(src)?;
        let _ = fs::remove_file(dest);
        return std::os::unix::fs::symlink(target, dest);
    }
    if meta.is_dir() {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_tree_keep_links(&entry.path(), &dest.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p)?;
    }
    fs::copy(src, dest).map(|_| ())
}

impl PanelIo for LivePanelIo {
    fn run(&self, argv: &[String]) -> (bool, String) {
        let spec = crate::utils::RunSpec {
            argv: argv.to_vec(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(30),
            stderr_to_stdout: true,
            ..Default::default()
        };
        let r = crate::utils::run(&spec, std::env::var("PATH").ok().as_deref());
        let mut out = String::from_utf8_lossy(&r.stdout).into_owned();
        if out.trim().is_empty() {
            out = String::from_utf8_lossy(&r.stderr).into_owned();
        }
        if out.trim().is_empty() && !r.error.is_empty() {
            out = r.error.clone();
        }
        (r.returncode == Some(0), out)
    }
    fn read(&self, path: &str) -> Option<String> {
        fs::read_to_string(path).ok()
    }
    fn write(&self, path: &str, text: &str) -> bool {
        if let Some(p) = Path::new(path).parent() {
            if fs::create_dir_all(p).is_err() {
                return false;
            }
        }
        fs::write(path, text).is_ok()
    }
    fn exists(&self, path: &str) -> bool {
        fs::symlink_metadata(path).is_ok()
    }
    fn list_dir(&self, path: &str) -> Vec<String> {
        let mut out: Vec<String> = fs::read_dir(path)
            .map(|rd| rd.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect())
            .unwrap_or_default();
        out.sort();
        out
    }
    fn copy_tree(&self, src: &str, dest: &str) -> Result<(), String> {
        copy_tree_keep_links(Path::new(src), Path::new(dest)).map_err(|e| format!("{src}: {e}"))
    }
    fn remove_tree(&self, path: &str) -> Result<(), String> {
        let p = Path::new(path);
        let res = match fs::symlink_metadata(p) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
            Ok(m) if m.is_dir() => fs::remove_dir_all(p),
            Ok(_) => fs::remove_file(p),
        };
        res.map_err(|e| format!("{path}: {e}"))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        fs::rename(from, to).map_err(|e| format!("{to}: {e}"))
    }
    fn spawn(&self, argv: &[String]) -> bool {
        LiveShellOps.spawn(&Cmd { argv: argv.to_vec(), timeout_secs: None, lang_c: false })
    }
    fn reload_signal(&self, exe: &str) -> usize {
        let mut sent = 0;
        for pid in session_pids_named(exe) {
            let Ok(pid) = libc::pid_t::try_from(pid) else { continue };
            // SAFETY: kill с конкретным положительным PID — обычный вызов.
            if unsafe { libc::kill(pid, libc::SIGUSR2) } == 0 {
                sent += 1;
            }
        }
        sent
    }
    fn pause(&self) {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// Живая проба стола для [`panel_copy_spec`].
pub fn live_panel_copy_spec(de: &str) -> Option<PanelCopySpec> {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = std::env::var("PATH").ok();
    let have = |tool: &str| crate::utils::which(tool, path.as_deref()).is_some();
    let home_exists = |rel: &str| Path::new(&format!("{}/{rel}", home.trim_end_matches('/'))).exists();
    let gnome_extensions = if de == "gnome" {
        let (ok, out) = LivePanelIo.run(&sv(&["gsettings", "get", "org.gnome.shell", "enabled-extensions"]));
        if ok { crate::de_apply::parse_gsettings_list(&out) } else { Vec::new() }
    } else {
        Vec::new()
    };
    let system_exists = |p: &str| Path::new(p).is_file();
    panel_copy_spec(
        de,
        &PanelProbe { have: &have, home_exists: &home_exists, gnome_extensions: &gnome_extensions, system_exists: &system_exists },
    )
}

fn live_copy_dir(de: &str) -> (String, String) {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_DATA_HOME").ok();
    let data = crate::utils::data_home(xdg.as_deref(), &home);
    (home, panel_copy_dir(&data, de))
}

/// Есть ли уже копия панели стола `de` (новая её заменит).
pub fn panel_copy_exists_live(de: &str) -> bool {
    let (_, dir) = live_copy_dir(de);
    Path::new(&format!("{dir}/manifest.json")).is_file()
}

/// «Сохранить панель» у стола `de` → (удалось, путь либо причина).
pub fn panel_copy_save_live(de: &str) -> (bool, String) {
    let Some(spec) = live_panel_copy_spec(de) else {
        return (false, tr("There is no saved panel copy yet."));
    };
    let (home, dir) = live_copy_dir(de);
    match panel_copy_save(&LivePanelIo, &spec, &home, &dir) {
        Ok(p) => (true, p),
        Err(e) => (false, e),
    }
}

/// «Вернуть панель» у стола `de` → (удалось, путь либо причина).
pub fn panel_copy_restore_live(de: &str) -> (bool, String) {
    let Some(spec) = live_panel_copy_spec(de) else {
        return (false, tr("There is no saved panel copy yet."));
    };
    let (home, dir) = live_copy_dir(de);
    match panel_copy_restore(&LivePanelIo, &spec, &home, &dir) {
        Ok(p) => (true, p),
        Err(e) => (false, e),
    }
}

/// Процессы своего сеанса (uid и `XDG_SESSION_ID`) с исполняемым файлом
/// `name` — по PID, не по имени в `pkill`.
fn session_pids_named(name: &str) -> Vec<u32> {
    use std::os::unix::fs::MetadataExt;
    let my_session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
    if my_session.is_empty() {
        return Vec::new();
    }
    // SAFETY: getuid не имеет предусловий.
    let my_uid = unsafe { libc::getuid() };
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            let base = format!("/proc/{pid}");
            let (Ok(meta), Ok(exe), Ok(environ)) =
                (fs::metadata(&base), fs::read_link(format!("{base}/exe")), fs::read(format!("{base}/environ")))
            else {
                return false;
            };
            is_session_process(&exe.to_string_lossy(), name, meta.uid(), &environ, my_uid, &my_session)
        })
        .collect()
}

#[cfg(test)]
mod original_panel_tests {
    use super::*;

    #[test]
    fn original_copy_is_kept_until_it_is_returned() {
        // Копии нет — снимается исходная.
        assert_eq!(panel_snapshot_slot(false, None), PanelSnapshotSlot::Original);
        // Исходная стоит (в том числе снятая прежним выпуском, без отметки) —
        // следующие снимки ложатся рядом.
        assert_eq!(panel_snapshot_slot(true, None), PanelSnapshotSlot::Last);
        assert_eq!(panel_snapshot_slot(true, Some("")), PanelSnapshotSlot::Last);
        // Исходную вернули — следующее вмешательство снимает её заново.
        assert_eq!(panel_snapshot_slot(true, Some("returned\n")), PanelSnapshotSlot::Original);
        let copy = "/h/.local/share/rexon/backups/mate-panel.dconf";
        assert_eq!(panel_original_returned_mark(copy), format!("{copy}.returned"));
        assert_eq!(panel_last_snapshot_path(copy), format!("{copy}.last"));
    }
}

#[cfg(test)]
mod xfce_panel_tests {
    use super::*;

    #[test]
    fn xfce_panel_copy_goes_through_panel_profiles() {
        let path = xfce_panel_copy_path("/home/w/.local/share/");
        assert_eq!(path, "/home/w/.local/share/rexon/backups/xfce4-panel.tar.bz2");
        assert_eq!(xfce_panel_save_argv(&path), ["xfce4-panel-profiles", "save", path.as_str()]);
        assert_eq!(xfce_panel_load_argv(&path), ["xfce4-panel-profiles", "load", path.as_str()]);
        // Копии нет — возвращать нечего, программа не зовётся.
        let (ok, msg) = xfce_panel_restore_live("/nonexistent-rexon-test");
        assert!(!ok && !msg.is_empty());
    }
}

#[cfg(test)]
mod exec_tests {
    use super::*;

    /// Свой каталог во временной папке (tempfile в зависимостях ядра нет —
    /// тот же приём, что в `appearance_cache.rs`).
    fn scratch(tag: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("rexon-plasma-{tag}-{nanos}"));
        fs::create_dir_all(&path).expect("временный каталог обязан создаться");
        path.to_string_lossy().into_owned()
    }

    /// Подставной «рабочий стол»: каталог конфигов с двумя файлами и одним
    /// деревом — как `~/.local/share/plasma` у настоящей панели.
    fn fake_desktop(root: &str) -> Vec<String> {
        let cfg = format!("{root}/config");
        fs::create_dir_all(format!("{cfg}/plasma/look")).unwrap();
        fs::write(format!("{cfg}/appletsrc"), "панель=слева\n").unwrap();
        fs::write(format!("{cfg}/plasmashellrc"), "тема=breeze\n").unwrap();
        fs::write(format!("{cfg}/plasma/look/theme"), "обои\n").unwrap();
        vec![
            format!("{cfg}/appletsrc"),
            format!("{cfg}/plasmashellrc"),
            format!("{cfg}/plasma"),
            // Заведомо отсутствующий конфиг: его в снимке быть не должно.
            format!("{cfg}/нет-такого-файла"),
        ]
    }

    /// Снимок обязан лечь на диск ЦЕЛИКОМ — вместе с деревом, — а мета-файл
    /// назвать ровно то, что скопировалось. Без этого «Восстановить» вернуло
    /// бы половину панели и человек не понял бы, почему.
    #[test]
    fn снимок_копирует_файлы_и_дерево() {
        let root = scratch("snap");
        let configs = fake_desktop(&root);
        let dir = format!("{root}/backups");

        let (ok, msg) = plasma_backup_run(&dir, &configs, "manual", false, "20260908_120000")
            .expect("подставной каталог обязан записаться");
        assert!(ok, "снимок не снялся: {msg}");

        let snap = format!("{dir}/20260908_120000");
        assert_eq!(
            fs::read_to_string(format!("{snap}/appletsrc")).unwrap(),
            "панель=слева\n"
        );
        assert_eq!(
            fs::read_to_string(format!("{snap}/plasma/look/theme")).unwrap(),
            "обои\n",
            "вложенное дерево конфигов в снимок не попало"
        );
        assert!(
            !Path::new(&format!("{snap}/нет-такого-файла")).exists(),
            "в снимок попал конфиг, которого на диске нет"
        );

        let entries = plasma_parse_entries(&plasma_meta_entries(&dir));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].files.len(), 3, "мета-файл врёт о составе снимка");
        assert_eq!(entries[0].reason, "manual");

        let _ = fs::remove_dir_all(&root);
    }

    /// Копировать нечего — на диске не должно остаться пустого каталога,
    /// который выглядит как сломанная копия, а человеку надо сказать словами.
    #[test]
    fn пустой_снимок_не_остаётся_на_диске() {
        let root = scratch("empty");
        let dir = format!("{root}/backups");
        let configs = vec![format!("{root}/ничего-нет")];

        let (ok, msg) = plasma_backup_run(&dir, &configs, "manual", false, "20260908_130000")
            .expect("каталог копий обязан создаться");
        assert!(!ok);
        assert_eq!(msg, msg_nothing_to_backup());
        assert!(
            !Path::new(&format!("{dir}/20260908_130000")).exists(),
            "пустой каталог снимка остался на диске"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// ⚠️ Запись НЕЗНАКОМОЙ формы (её оставила бы более новая версия Rexon)
    /// обязана пережить нашу запись мета-файла. Потерять её значит потерять
    /// ссылку на чужой снимок: сам снимок остался бы на диске мёртвым грузом,
    /// а в списке его бы не было.
    #[test]
    fn чужая_запись_переживает_новую_копию() {
        let root = scratch("alien");
        let configs = fake_desktop(&root);
        let dir = format!("{root}/backups");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            plasma_meta_path(&dir),
            r#"{"backups":[{"timestamp":"20250101_000000","reason":"manual",
                "path":"/tmp/чужой","files":[],"full":false,"новое_поле":42}]}"#,
        )
        .unwrap();

        plasma_backup_run(&dir, &configs, "manual", false, "20260908_140000").unwrap();

        let raw = plasma_meta_entries(&dir);
        assert_eq!(raw.len(), 2, "чужая запись пропала из мета-файла");
        assert_eq!(
            raw[0].get("новое_поле").and_then(|v| v.as_i64()),
            Some(42),
            "незнакомый ключ чужой записи потерян"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// Ротация: больше предела снимков не копится, а лишние каталоги
    /// действительно удаляются с диска — иначе копии панели тихо съели бы
    /// место, и об этом никто бы не узнал.
    #[test]
    fn ротация_удаляет_старые_каталоги() {
        let root = scratch("rotate");
        let configs = fake_desktop(&root);
        let dir = format!("{root}/backups");

        for i in 0..(MAX_PLASMA_SNAPSHOTS + 2) {
            let stamp = format!("20260908_1500{i:02}");
            plasma_backup_run(&dir, &configs, "manual", false, &stamp).unwrap();
        }

        let entries = plasma_parse_entries(&plasma_meta_entries(&dir));
        assert_eq!(entries.len(), MAX_PLASMA_SNAPSHOTS);
        assert_eq!(entries[0].timestamp, "20260908_150002", "выброшены не самые старые");
        assert!(
            !Path::new(&format!("{dir}/20260908_150000")).exists(),
            "каталог выброшенного снимка остался на диске"
        );
        assert!(Path::new(&format!("{dir}/20260908_150002")).is_dir());

        let _ = fs::remove_dir_all(&root);
    }

    /// Возврат копии: файлы обязаны вернуться к прежнему содержимому, а
    /// метка времени — попасть в ответ (человек по ней узнаёт, что вернул).
    #[test]
    fn возврат_копии_восстанавливает_файлы() {
        let root = scratch("restore");
        let configs = fake_desktop(&root);
        let dir = format!("{root}/backups");
        plasma_backup_run(&dir, &configs, "manual", false, "20260908_160000").unwrap();

        // Панель «сломали» после снятия копии.
        fs::write(format!("{root}/config/appletsrc"), "панель=пусто\n").unwrap();

        let (ok, msg) = plasma_restore_run(&dir, Some("20260908_160000"));
        assert!(ok, "возврат не удался: {msg}");
        // Метка попадает в ответ в ЧЕЛОВЕЧЕСКОМ виде — тем же, каким
        // подписана строка списка копий в окне (правило #13, 18.09).
        assert!(
            msg.contains(&crate::qt_settings::pretty_backup_ts("20260908_160000"))
                && msg.contains("08.09.2026"),
            "в ответе нет метки копии: {msg}"
        );
        assert_eq!(
            fs::read_to_string(format!("{root}/config/appletsrc")).unwrap(),
            "панель=слева\n",
            "файл не вернулся к сохранённому виду"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// Пустая метка = «последняя копия», как у оригинала (`backups[-1]`), а
    /// чужая метка — отказ словами, а не молчание.
    #[test]
    fn выбор_копии_по_метке_и_без_неё() {
        let root = scratch("pick");
        let configs = fake_desktop(&root);
        let dir = format!("{root}/backups");
        plasma_backup_run(&dir, &configs, "manual", false, "20260908_170000").unwrap();
        fs::write(format!("{root}/config/appletsrc"), "панель=вторая\n").unwrap();
        plasma_backup_run(&dir, &configs, "manual", false, "20260908_170500").unwrap();
        fs::write(format!("{root}/config/appletsrc"), "панель=сломана\n").unwrap();

        let (ok, _) = plasma_restore_run(&dir, None);
        assert!(ok);
        assert_eq!(
            fs::read_to_string(format!("{root}/config/appletsrc")).unwrap(),
            "панель=вторая\n",
            "без метки вернулась не последняя копия"
        );

        let (ok, msg) = plasma_restore_run(&dir, Some("нет-такой-метки"));
        assert!(!ok);
        assert_eq!(msg, msg_snapshot_not_found("нет-такой-метки"));

        // Копий нет вовсе — тоже словами.
        let empty = format!("{root}/пусто");
        let (ok, msg) = plasma_restore_run(&empty, None);
        assert!(!ok);
        assert_eq!(msg, msg_no_plasma_backups());

        let _ = fs::remove_dir_all(&root);
    }

    /// Каталог снимка исчез с диска (человек почистил `~/.local/share`) —
    /// сказать об этом, а не пытаться восстанавливать из пустоты.
    #[test]
    fn исчезнувший_каталог_снимка_назван() {
        let root = scratch("gone");
        let configs = fake_desktop(&root);
        let dir = format!("{root}/backups");
        plasma_backup_run(&dir, &configs, "manual", false, "20260908_180000").unwrap();
        fs::remove_dir_all(format!("{dir}/20260908_180000")).unwrap();

        let (ok, msg) = plasma_restore_run(&dir, Some("20260908_180000"));
        assert!(!ok);
        assert!(msg.contains("20260908_180000"), "в отказе нет пути снимка: {msg}");

        let _ = fs::remove_dir_all(&root);
    }

    /// ⚠️ Порядок перезапуска оболочки. Юнит systemd поднимает панель САМ:
    /// второй запуск после успешного юнита дал бы человеку ДВЕ панели. А если
    /// юнита нет — обязаны пройти оба запасных шага, иначе панель останется
    /// лежать. Живая панель при этом не трогается: запускатели подставные.
    #[test]
    fn перезапуск_оболочки_не_поднимает_панель_дважды() {
        use std::cell::RefCell;

        /// Подставные руки: что «есть» в системе, что «удаётся», и журнал.
        struct Fake {
            unit_ok: bool,
            tools: &'static [&'static str],
            quit_ok: bool,
            pids: Vec<u32>,
            log: RefCell<Vec<String>>,
        }
        impl ShellOps for Fake {
            fn run(&self, cmd: &Cmd) -> bool {
                self.log.borrow_mut().push(cmd.argv.join(" "));
                if cmd.argv[0] == "systemctl" {
                    self.unit_ok
                } else {
                    self.quit_ok
                }
            }
            fn spawn(&self, cmd: &Cmd) -> bool {
                self.log.borrow_mut().push(format!("spawn: {}", cmd.argv.join(" ")));
                true
            }
            fn has(&self, tool: &str) -> bool {
                self.tools.contains(&tool)
            }
            fn session_shell_pids(&self) -> Vec<u32> {
                self.pids.clone()
            }
            fn terminate(&self, pid: u32) -> bool {
                self.log.borrow_mut().push(format!("term: {pid}"));
                true
            }
            fn settle(&self) {}
        }
        let fake = |unit_ok, tools, quit_ok, pids: Vec<u32>| Fake {
            unit_ok,
            tools,
            quit_ok,
            pids,
            log: RefCell::new(Vec::new()),
        };
        let both: &[&str] = &["kquitapp6", "kstart"];

        // Юнит отработал — больше ничего: второй запуск дал бы две панели.
        let f = fake(true, both, true, vec![7]);
        assert_eq!(plasma_restart_shell_with(&f), ShellRestart::Unit);
        assert_eq!(f.log.borrow().len(), 1, "после юнита сделано лишнее: {:?}", f.log.borrow());

        // Юнита нет — kquitapp останавливает, kstart поднимает отдельно.
        let f = fake(false, both, true, vec![7]);
        assert_eq!(plasma_restart_shell_with(&f), ShellRestart::QuitAndStart);
        assert_eq!(
            *f.log.borrow(),
            [
                "systemctl --user restart plasma-plasmashell.service",
                "kquitapp6 plasmashell",
                "spawn: kstart plasmashell",
            ]
        );

        // kquitapp не помог — оболочка СВОЕГО сеанса по PID, потом запуск.
        let f = fake(false, both, false, vec![4242]);
        assert_eq!(plasma_restart_shell_with(&f), ShellRestart::SessionPid);
        assert_eq!(f.log.borrow()[2..], ["term: 4242", "spawn: kstart plasmashell"]);

        // Без kstart остановка через kquitapp не начинается вовсе (остановить
        // и не поднять хуже, чем не трогать), а запуск идёт напрямую.
        let f = fake(false, &["kquitapp6"], true, vec![4242]);
        assert_eq!(plasma_restart_shell_with(&f), ShellRestart::SessionPid);
        assert_eq!(f.log.borrow()[1..], ["term: 4242", "spawn: plasmashell"]);

        // Своей оболочки не нашлось — не тронуто НИЧЕГО, и никакого `killall`.
        let f = fake(false, both, false, vec![]);
        assert_eq!(plasma_restart_shell_with(&f), ShellRestart::NotRestarted);
        assert!(f.log.borrow().iter().all(|s| !s.contains("killall")), "{:?}", f.log.borrow());
        assert!(f.log.borrow().iter().all(|s| !s.starts_with("spawn")), "{:?}", f.log.borrow());
        assert_eq!(
            plasma_restart_message(ShellRestart::NotRestarted),
            msg_plasmashell_not_restarted()
        );
    }

    /// Оболочка своего сеанса опознаётся по ТРЁМ признакам сразу; любого
    /// одного мало, чтобы не задеть чужую панель.
    #[test]
    fn оболочка_своего_сеанса_опознаётся_тремя_признаками() {
        let env = b"HOME=/h\0XDG_SESSION_ID=2\0LANG=ru_RU.UTF-8\0";
        assert!(is_session_shell("/usr/bin/plasmashell", 1000, env, 1000, "2"));
        assert!(is_session_shell("/usr/bin/plasmashell (deleted)", 1000, env, 1000, "2"));
        // чужой человек
        assert!(!is_session_shell("/usr/bin/plasmashell", 1001, env, 1000, "2"));
        // другой сеанс того же человека
        assert!(!is_session_shell("/usr/bin/plasmashell", 1000, env, 1000, "3"));
        // похожее имя — не оболочка
        assert!(!is_session_shell("/usr/bin/plasmashell-helper", 1000, env, 1000, "2"));
        // XDG_SESSION_ID=22 не равен «2»
        let env22 = b"XDG_SESSION_ID=22\0";
        assert!(!is_session_shell("/usr/bin/plasmashell", 1000, env22, 1000, "2"));
        // своего сеанса не знаем — не трогаем никого
        assert!(!is_session_shell("/usr/bin/plasmashell", 1000, env, 1000, ""));
    }
}

#[cfg(test)]
mod panel_copy_tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// Машина в памяти: файлы (каталог — ключ с `/` на конце не нужен:
    /// каталог есть, если есть файл под ним), dconf — словарь веток.
    #[derive(Default)]
    pub(super) struct Mem {
        pub files: RefCell<BTreeMap<String, String>>,
        pub dconf: RefCell<BTreeMap<String, String>>,
        pub runs: RefCell<Vec<Vec<String>>>,
        pub spawned: RefCell<Vec<Vec<String>>>,
        pub signals: RefCell<Vec<String>>,
        /// DConfig Deepin: `приложение|ресурс|ключ` → ответ `dde-dconfig get`.
        pub dde: RefCell<BTreeMap<String, String>>,
        /// Ключи DConfig со значением по умолчанию.
        pub dde_default: RefCell<std::collections::BTreeSet<String>>,
    }

    impl PanelIo for Mem {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.runs.borrow_mut().push(argv.to_vec());
            match argv.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                ["dconf", "dump", dir] => (true, self.dconf.borrow().get(*dir).cloned().unwrap_or_default()),
                ["dconf", "reset", "-f", dir] => {
                    self.dconf.borrow_mut().remove(*dir);
                    (true, String::new())
                }
                ["sh", "-c", _, "sh", dir, file] => {
                    let text = self.files.borrow().get(*file).cloned().unwrap_or_default();
                    self.dconf.borrow_mut().insert(dir.to_string(), text);
                    (true, String::new())
                }
                ["dde-dconfig", "get", "-a", a, "-r", r, "-k", k, "-m", "isDefaultValue"] => {
                    (true, self.dde_default.borrow().contains(&format!("{a}|{r}|{k}")).to_string())
                }
                ["dde-dconfig", "get", "-a", a, "-r", r, "-k", k] => {
                    (true, self.dde.borrow().get(&format!("{a}|{r}|{k}")).cloned().unwrap_or_default())
                }
                // Как печатает `get` после `set`: логические — как есть, прочее — в кавычках.
                ["dde-dconfig", "set", "-a", a, "-r", r, "-k", k, "-v", v] => {
                    let id = format!("{a}|{r}|{k}");
                    let shown = if matches!(*v, "true" | "false") { v.to_string() } else { format!("\"{v}\"") };
                    self.dde.borrow_mut().insert(id.clone(), shown);
                    self.dde_default.borrow_mut().remove(&id);
                    (true, String::new())
                }
                ["dde-dconfig", "reset", "-a", a, "-r", r, "-k", k] => {
                    let id = format!("{a}|{r}|{k}");
                    self.dde.borrow_mut().insert(id.clone(), "default".into());
                    self.dde_default.borrow_mut().insert(id);
                    (true, String::new())
                }
                _ => (true, String::new()),
            }
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            self.files.borrow_mut().insert(path.to_string(), text.to_string());
            true
        }
        fn exists(&self, path: &str) -> bool {
            let pre = format!("{path}/");
            self.files.borrow().keys().any(|k| k == path || k.starts_with(&pre))
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            let pre = format!("{}/", path.trim_end_matches('/'));
            let mut out: Vec<String> = self
                .files
                .borrow()
                .keys()
                .filter_map(|k| k.strip_prefix(&pre).map(|r| r.split('/').next().unwrap_or("").to_string()))
                .collect();
            out.dedup();
            out
        }
        fn copy_tree(&self, src: &str, dest: &str) -> Result<(), String> {
            let pre = format!("{src}/");
            let items: Vec<(String, String)> = self
                .files
                .borrow()
                .iter()
                .filter_map(|(k, v)| {
                    if k == src {
                        Some((dest.to_string(), v.clone()))
                    } else {
                        k.strip_prefix(&pre).map(|r| (format!("{dest}/{r}"), v.clone()))
                    }
                })
                .collect();
            self.files.borrow_mut().extend(items);
            Ok(())
        }
        fn remove_tree(&self, path: &str) -> Result<(), String> {
            let pre = format!("{path}/");
            self.files.borrow_mut().retain(|k, _| k != path && !k.starts_with(&pre));
            Ok(())
        }
        fn rename(&self, from: &str, to: &str) -> Result<(), String> {
            self.copy_tree(from, to)?;
            self.remove_tree(from)
        }
        fn spawn(&self, argv: &[String]) -> bool {
            self.spawned.borrow_mut().push(argv.to_vec());
            true
        }
        fn reload_signal(&self, exe: &str) -> usize {
            self.signals.borrow_mut().push(exe.to_string());
            1
        }
        fn pause(&self) {}
    }

    pub(super) fn spec_for(de: &str, tools: &[&str], home_paths: &[&str], exts: &[&str]) -> Option<PanelCopySpec> {
        let tools: Vec<String> = tools.iter().map(|s| s.to_string()).collect();
        let paths: Vec<String> = home_paths.iter().map(|s| s.to_string()).collect();
        let exts: Vec<String> = exts.iter().map(|s| s.to_string()).collect();
        let have = |t: &str| tools.iter().any(|x| x == t);
        let home_exists = |p: &str| paths.iter().any(|x| x == p);
        let system_exists = |p: &str| paths.iter().any(|x| x == p);
        panel_copy_spec(
            de,
            &PanelProbe { have: &have, home_exists: &home_exists, gnome_extensions: &exts, system_exists: &system_exists },
        )
    }

    const HOME: &str = "/home/w";
    const COPY: &str = "/home/w/.local/share/rexon/backups/panel-x";

    #[test]
    fn only_panel_keys_of_a_dconf_root_are_taken() {
        let dump = "[/]\nenabled-applets=['panel1:left:0:menu@cinnamon.org:0']\ngtk-theme='Mint-Y'\npanels-height=['1:40']\n\n[theme]\nname='x'\n";
        assert_eq!(
            dconf_root_keys(dump, &["panels-", "enabled-applets"]),
            "[/]\nenabled-applets=['panel1:left:0:menu@cinnamon.org:0']\npanels-height=['1:40']\n"
        );
        assert_eq!(dconf_root_keys("[sub]\npanels-x=1\n", &["panels-"]), "");
    }

    /// Cinnamon: панели и апплеты — ключи `org.cinnamon`, настройки
    /// апплетов — `~/.config/cinnamon/spices`; тема и прочее из той же ветки
    /// при возврате не трогаются.
    #[test]
    fn cinnamon_panel_round_trip() {
        let spec = spec_for("cinnamon-mint", &[], &[], &[]).expect("Cinnamon снимается");
        assert_eq!(spec, spec_for("cinnamon", &[], &[], &[]).unwrap());
        let m = Mem::default();
        m.dconf.borrow_mut().insert(
            "/org/cinnamon/".into(),
            "[/]\nenabled-applets=['a']\ngtk-theme='Mint-Y'\npanels-enabled=['1:0:bottom']\n".into(),
        );
        m.write(&format!("{HOME}/.config/cinnamon/spices/menu@cinnamon.org/0.json"), "{\"a\":1}");
        assert_eq!(panel_copy_save(&m, &spec, HOME, COPY).as_deref(), Ok(COPY));
        assert!(!m.exists(&format!("{COPY}.new")), "сборочный каталог убран");
        assert_eq!(
            m.read(&format!("{COPY}/dconf-0.ini")).as_deref(),
            Some("[/]\nenabled-applets=['a']\npanels-enabled=['1:0:bottom']\n")
        );
        // Человек поменял панель и настройки апплета.
        m.write(&format!("{HOME}/.config/cinnamon/spices/menu@cinnamon.org/0.json"), "{\"a\":2}");
        m.write(&format!("{HOME}/.config/cinnamon/spices/new@x/0.json"), "{}");
        assert!(panel_copy_restore(&m, &spec, HOME, COPY).is_ok());
        assert_eq!(m.read(&format!("{HOME}/.config/cinnamon/spices/menu@cinnamon.org/0.json")).as_deref(), Some("{\"a\":1}"));
        assert!(!m.exists(&format!("{HOME}/.config/cinnamon/spices/new@x")));
        // Ветку целиком не сбрасывали: только дописали ключи панели.
        assert!(!m.runs.borrow().iter().any(|r| r.get(1).map(String::as_str) == Some("reset")));
    }

    #[test]
    fn restore_without_copy_and_hostile_manifest() {
        let spec = spec_for("cinnamon", &[], &[], &[]).unwrap();
        let m = Mem::default();
        assert!(panel_copy_restore(&m, &spec, HOME, COPY).is_err(), "копии нет");
        // Манифест — файл на диске: чужая ветка и путь с «..» не исполняются.
        m.write(
            &format!("{COPY}/manifest.json"),
            r#"[{"kind":"dconf_dir","dir":"/org/gnome/","file":"x.ini"},{"kind":"home","path":"../../etc"}]"#,
        );
        assert!(panel_copy_restore(&m, &spec, HOME, COPY).is_ok());
        assert!(m.runs.borrow().is_empty(), "{:?}", m.runs.borrow());
    }

    /// Budgie: вся ветка `com.solus-project.budgie-panel` (панели,
    /// апплеты, их настройки); при возврате ветка сбрасывается целиком, и
    /// `budgie-panel --replace` строит панели заново.
    #[test]
    fn budgie_panel_round_trip() {
        assert!(spec_for("budgie", &[], &[], &[]).is_none(), "без budgie-panel снимать нечего");
        let spec = spec_for("budgie", &["budgie-panel"], &[], &[]).expect("Budgie снимается");
        let m = Mem::default();
        m.dconf.borrow_mut().insert("/com/solus-project/budgie-panel/".into(), "[/]\npanels=['a']\n".into());
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        m.dconf.borrow_mut().insert("/com/solus-project/budgie-panel/".into(), "[/]\npanels=['b']\n".into());
        panel_copy_restore(&m, &spec, HOME, COPY).unwrap();
        assert_eq!(m.dconf.borrow()["/com/solus-project/budgie-panel/"], "[/]\npanels=['a']\n");
        assert!(m.runs.borrow().iter().any(|r| r == &sv(&["dconf", "reset", "-f", "/com/solus-project/budgie-panel/"])));
        assert_eq!(*m.spawned.borrow(), [sv(&["budgie-panel", "--replace"])]);
    }

    /// LXQt: файл `~/.config/lxqt/panel.conf`; панель перечитывает его,
    /// когда сеанс LXQt останавливает и снова запускает её модуль.
    #[test]
    fn lxqt_panel_round_trip() {
        let spec = spec_for("lxqt", &["lxqt-panel"], &[], &[]).expect("LXQt снимается");
        let m = Mem::default();
        let conf = format!("{HOME}/.config/lxqt/panel.conf");
        m.write(&conf, "[panel1]\nposition=Bottom\n");
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        m.write(&conf, "[panel1]\nposition=Top\n");
        panel_copy_restore(&m, &spec, HOME, COPY).unwrap();
        assert_eq!(m.read(&conf).as_deref(), Some("[panel1]\nposition=Bottom\n"));
        let runs = m.runs.borrow();
        let calls: Vec<&String> = runs.iter().filter(|r| r[0] == "dbus-send").map(|r| &r[5]).collect();
        assert_eq!(calls, ["org.lxqt.session.stopModule", "org.lxqt.session.startModule"]);
        assert!(runs.iter().any(|r| r.last().map(String::as_str) == Some("string:lxqt-panel.desktop")));
    }

    /// GNOME: панели нет — снимаются только включённые панельные
    /// расширения (Dash to Dock, Dash to Panel) и список включённых.
    #[test]
    fn gnome_copies_only_panel_extensions() {
        assert!(spec_for("gnome", &[], &[], &["appindicatorsupport@rgcjonas.gmail.com"]).is_none());
        let spec = spec_for("gnome", &[], &[], &["dash-to-dock@micxgx.gmail.com"]).expect("Dash to Dock");
        assert!(spec.parts.contains(&PanelPart::DconfDir("/org/gnome/shell/extensions/dash-to-dock/")));
        assert!(spec.parts.contains(&PanelPart::DconfKeys { dir: "/org/gnome/shell/", prefixes: &["enabled-extensions"] }));
        let both = spec_for("gnome", &[], &[], &["ubuntu-dock@ubuntu.com", "dash-to-dock@micxgx.gmail.com", "dash-to-panel@jderose9.github.com"]).unwrap();
        let dirs = both.parts.iter().filter(|p| matches!(p, PanelPart::DconfDir(_))).count();
        assert_eq!(dirs, 2, "одна ветка Dash to Dock на два расширения: {:?}", both.parts);
    }

    /// Pantheon: ветка Wingpanel и стоящего дока (Plank или новый док
    /// elementary).
    #[test]
    fn pantheon_copies_wingpanel_and_dock() {
        assert!(spec_for("pantheon", &[], &[], &[]).is_none());
        let spec = spec_for("pantheon", &["io.elementary.wingpanel", "plank"], &[], &[]).unwrap();
        // Значки дока Plank — файлы `~/.config/plank/<док>/launchers`.
        assert_eq!(
            spec.parts,
            [
                PanelPart::DconfDir("/io/elementary/desktop/wingpanel/"),
                PanelPart::DconfDir("/net/launchpad/plank/"),
                PanelPart::Home(".config/plank".into()),
            ]
        );
        let spec = spec_for("pantheon", &["io.elementary.wingpanel", "io.elementary.dock"], &[], &[]).unwrap();
        assert!(spec.parts.contains(&PanelPart::DconfDir("/io/elementary/dock/")));
    }

    /// COSMIC: каталоги `com.system76.CosmicPanel*` в `~/.config/cosmic`
    /// (панель, док, их кнопки); лишний каталог, появившийся после
    /// снятия копии, при возврате убирается.
    #[test]
    fn cosmic_panel_round_trip() {
        let spec = spec_for("cosmic", &["cosmic-panel"], &[], &[]).expect("COSMIC");
        let m = Mem::default();
        let base = format!("{HOME}/.config/cosmic");
        m.write(&format!("{base}/com.system76.CosmicPanel/v1/entries"), "[\"Panel\"]");
        m.write(&format!("{base}/com.system76.CosmicPanel.Dock/v1/anchor"), "Bottom");
        m.write(&format!("{base}/com.system76.CosmicTheme.Dark/v1/x"), "keep");
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        m.write(&format!("{base}/com.system76.CosmicPanel.Dock/v1/anchor"), "Left");
        m.write(&format!("{base}/com.system76.CosmicPanel.Extra/v1/x"), "new");
        panel_copy_restore(&m, &spec, HOME, COPY).unwrap();
        assert_eq!(m.read(&format!("{base}/com.system76.CosmicPanel.Dock/v1/anchor")).as_deref(), Some("Bottom"));
        assert!(!m.exists(&format!("{base}/com.system76.CosmicPanel.Extra")));
        assert_eq!(m.read(&format!("{base}/com.system76.CosmicTheme.Dark/v1/x")).as_deref(), Some("keep"));
    }

    /// Тайловые композиторы: панель — это бар. Снимается конфиг того, что
    /// стоит: waybar (перечитывает по SIGUSR2), Noctalia, DankMaterialShell.
    #[test]
    fn tiling_bars_are_copied() {
        for de in ["hyprland", "niri", "sway", "river"] {
            assert!(spec_for(de, &[], &[], &[]).is_none(), "{de}: бара нет — кнопок нет");
            let spec = spec_for(de, &["waybar"], &[".config/waybar"], &[]).expect(de);
            assert_eq!(spec.parts, [PanelPart::Home(".config/waybar".into())]);
            assert_eq!(spec.reload, [PanelReload::ReloadSignal("waybar")]);
            let spec = spec_for(de, &[], &[".config/noctalia", ".config/DankMaterialShell"], &[]).unwrap();
            assert_eq!(
                spec.parts,
                [PanelPart::Home(".config/noctalia".into()), PanelPart::Home(".config/DankMaterialShell".into())]
            );
        }
        let m = Mem::default();
        let spec = spec_for("sway", &["waybar"], &[".config/waybar"], &[]).unwrap();
        m.write(&format!("{HOME}/.config/waybar/config.jsonc"), "{}");
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        panel_copy_restore(&m, &spec, HOME, COPY).unwrap();
        assert_eq!(*m.signals.borrow(), ["waybar"]);
    }

    /// Deepin: три ресурса DConfig дока dde-shell — все записываемые ключи
    /// схемы; при возврате обычные ключи пишутся, «по умолчанию» —
    /// сбрасываются, ключи только для чтения не трогаются, и док
    /// перезапускается своей службой.
    #[test]
    fn deepin_dock_round_trip() {
        let schema = |r: &str| crate::dde::dconfig_schema_path(DDE_SHELL_DCONFIG_APPID, r);
        let schemas: Vec<String> = DDE_DOCK_RESOURCES.iter().map(|r| schema(r)).collect();
        let refs: Vec<&str> = schemas.iter().map(String::as_str).collect();
        let spec = spec_for("deepin", &["dde-dconfig", "dde-shell"], &refs, &[]).expect("Deepin V23/V25");
        assert_eq!(spec.reload, [PanelReload::RestartUserUnit("dde-shell@DDE.service")]);
        let m = Mem::default();
        let rw = |v: &str| format!("{{\"value\": {v}, \"permissions\": \"readwrite\"}}");
        m.write(
            &schema("org.deepin.ds.dock"),
            &format!(
                "{{\"contents\": {{\"Dock_Size\": {}, \"Position\": {}, \"Plugins_Visible\": {}, \"Locked\": {}}}}}",
                rw("48"), rw("\"bottom\""), rw("{}"), rw("false")
            ),
        );
        m.write(
            &schema("org.deepin.ds.dock.taskmanager"),
            &format!(
                "{{\"contents\": {{\"dockedElements\": {}, \"cgroupsBasedGrouping\": {{\"value\": true, \"permissions\": \"readonly\"}}}}}}",
                rw("[]")
            ),
        );
        m.write(&schema("org.deepin.ds.dock.tray"), "{\"contents\": {}}");
        let id = |r: &str, k: &str| format!("{DDE_SHELL_DCONFIG_APPID}|{r}|{k}");
        let dock = "org.deepin.ds.dock";
        let tm = "org.deepin.ds.dock.taskmanager";
        m.dde.borrow_mut().extend([
            (id(dock, "Dock_Size"), "\"48\"".to_string()),
            (id(dock, "Position"), "\"bottom\"".to_string()),
            (id(dock, "Plugins_Visible"), "\"{\n    \"datetime\": true\n}\n\"".to_string()),
            (id(dock, "Locked"), "false".to_string()),
            (id(tm, "dockedElements"), "\"[\n    \"desktop/dde-file-manager\"\n]\n\"".to_string()),
        ]);
        m.dde_default.borrow_mut().insert(id(dock, "Locked"));
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        // Человек поменял док после копии.
        for (k, v) in [("Position", "left"), ("Dock_Size", "70"), ("Locked", "true")] {
            m.run(&crate::dde::dconfig_set_key_argv(DDE_SHELL_DCONFIG_APPID, dock, k, v));
        }
        m.runs.borrow_mut().clear();
        panel_copy_restore(&m, &spec, HOME, COPY).unwrap();
        let runs = m.runs.borrow().clone();
        let set = |r: &str, k: &str, v: &str| crate::dde::dconfig_set_key_argv(DDE_SHELL_DCONFIG_APPID, r, k, v);
        assert!(runs.contains(&set(dock, "Position", "bottom")), "{runs:?}");
        assert!(runs.contains(&set(dock, "Dock_Size", "48")));
        assert!(runs.contains(&set(dock, "Plugins_Visible", "{\"datetime\":true}")), "словарь — компактным JSON");
        assert!(runs.contains(&set(tm, "dockedElements", "[\"desktop/dde-file-manager\"]")));
        assert!(runs.contains(&crate::dde::dconfig_reset_key_argv(DDE_SHELL_DCONFIG_APPID, dock, "Locked")));
        assert!(!runs.iter().any(|a| a.contains(&"cgroupsBasedGrouping".to_string())), "ключ только для чтения не трогаем");
        assert_eq!(runs.last().unwrap(), &sv(&["systemctl", "--user", "--no-block", "try-restart", "dde-shell@DDE.service"]));
        assert!(m.dde_default.borrow().contains(&id(dock, "Locked")));
    }

    /// Док другого выпуска (схем `org.deepin.dde.shell` нет — dde-shell до
    /// 0.0.35 или старый dde-dock) — кнопок нет, а не копия чужих ключей.
    #[test]
    fn deepin_dock_of_unknown_version_has_no_copy() {
        assert!(spec_for("deepin", &["dde-dconfig", "dde-shell"], &[], &[]).is_none());
        let old = crate::dde::dconfig_schema_path("org.deepin.ds.dock", "org.deepin.ds.dock");
        assert!(spec_for("deepin", &["dde-dconfig", "dde-shell"], &[&old], &[]).is_none());
        let schemas: Vec<String> =
            DDE_DOCK_RESOURCES.iter().map(|r| crate::dde::dconfig_schema_path(DDE_SHELL_DCONFIG_APPID, r)).collect();
        let refs: Vec<&str> = schemas.iter().map(String::as_str).collect();
        assert!(spec_for("deepin", &["dde-shell"], &refs, &[]).is_none(), "без dde-dconfig писать нечем");
    }

    /// Wayfire: панель — wf-shell, один файл, перечитывается сама.
    #[test]
    fn wayfire_panel_is_wf_shell_ini() {
        assert!(spec_for("wayfire", &[], &[], &[]).is_none(), "без wf-shell кнопок нет");
        let spec = spec_for("wayfire", &["wf-panel"], &[], &[]).expect("Wayfire");
        assert_eq!(spec.parts, [PanelPart::Home(".config/wf-shell.ini".into())]);
        assert_eq!(spec.reload, [PanelReload::Live]);
    }

    #[test]
    fn unknown_desktop_has_nothing_to_copy() {
        assert!(spec_for("labwc", &[], &[], &[]).is_none());
        assert!(spec_for("kde", &[], &[], &[]).is_none(), "KDE снимается своим путём");
    }
}
