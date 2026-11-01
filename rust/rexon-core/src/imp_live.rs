//! `LiveImpIo` — РЕАЛЬНЫЙ исполнитель трейта [`crate::importers::ImpIo`].
//!
//! ЗАЧЕМ ОТДЕЛЬНЫЙ МОДУЛЬ. Логика импортёров чужих форматов (`.themepack`,
//! `.theme`, `.msstyles`, курсоры, иконки) портирована целиком
//! (`crate::importers`), но она ИНТЕРАКТИВНА: ход разбора зависит от вывода
//! `convert`/`ffmpeg`/`wrestool`/`7z` и от файлов, появившихся после них.
//! Поэтому у неё нет статического плана — только трейт `ImpIo`, за которым
//! стоит живая ФС и живые подпроцессы.
//!
//! До 07.09.2026 единственная живая реализация трейта жила в PyO3-мосте
//! (`rust/rexon-py/src/lib.rs`), то есть импорт Windows-темы работал ТОЛЬКО
//! в процессе с Python. Из-за этого `ext_exec::install_from_file` умел
//! ставить лишь `.rxn`, а вкладка «Расширения» в процессе без Python теряла
//! половину того, что обещает своим же фильтром окна выбора файла
//! (`qt_extensions::FILE_DIALOG_FILTER`). Исполнитель переехал сюда целиком
//! и байт-в-байт: мост теперь берёт его отсюда же, второй копии нет.
//!
//! ЕДИНСТВЕННЫЙ ШОВ НАРУЖУ — апскейл обоев: `upscaler.rs` ведёт свой поток
//! через трейт `UpIo`, и его живой исполнитель может быть разным (в процессе
//! с Python — тот, что умеет Pillow и скачивание Real-ESRGAN). Поэтому
//! [`LiveImpIo`] принимает апскейл ЗАМЫКАНИЕМ: `None` — не апскейлить вовсе
//! (так ведёт себя `can_import`, которому побочные эффекты не нужны).
//!
//! ⭐ НИ ОДИН ПРОПУЩЕННЫЙ ШАГ ЗДЕСЬ НЕ МОЛЧИТ. Импорт чужой темы держится на
//! посторонних программах, и часть из них может отсутствовать. Раньше это
//! кончалось тихой потерей: обои копировались как есть, превью не строилось,
//! человек видел «✅ Установлено» и решал, что так и надо. Теперь каждый
//! пропуск кладётся в [`Notes`] и доходит до строки состояния вкладки.
//!
//! ⚠️ Таблицы синонимов инструментов ниже — те же, что у `utils::TOOL_SETS`,
//! но НЕ они: здесь сознательно сохранён набор, с которым снят эталон
//! паритета моста (`rust/rexon-py/tests/importers_roundtrip.py`). Сводить их
//! в одну — отдельная работа со своим замером, а не побочный эффект переезда.

use crate::ext_utils as core_ext;
use crate::importers as core_imp;

/// Замыкание апскейла обоев: `f(data_dir) -> сколько обработано`.
///
/// Порт `WallpaperUpscaler.process_wallpapers_in_dir`. Живёт снаружи, потому
/// что живой исполнитель апскейлера зависит от процесса (см. шапку модуля).
pub type UpscaleFn = Box<dyn FnMut(&str) -> u64 + Send>;

/// Копилка предупреждений о ПРОПУЩЕННЫХ необязательных шагах импорта.
///
/// Общая на весь конвейер и разделяемая: замыкание апскейла работает уже
/// внутри импорта, а прочитать написанное надо снаружи, после его конца.
/// Пропуск, о котором никто не узнал, — это и есть молчаливый отказ, ради
/// которого копилка и заведена.
pub type Notes = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// Пустая копилка предупреждений.
pub fn new_notes() -> Notes {
    std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))
}

/// Записать предупреждение, если такого ещё нет.
///
/// Повторы гасятся намеренно: обоев в теме бывает десяток, и одна и та же
/// строка «размер определить нечем» десять раз подряд — это не информация,
/// а стена текста (правило #13).
fn note(notes: &Notes, text: String) {
    if let Ok(mut list) = notes.lock() {
        if !list.iter().any(|t| *t == text) {
            list.push(text);
        }
    }
}

/// Забрать накопленное и оставить копилку пустой.
pub fn take_notes(notes: &Notes) -> Vec<String> {
    notes
        .lock()
        .map(|mut list| std::mem::take(&mut *list))
        .unwrap_or_default()
}


/// Реальный исполнитель ImpIo: настоящие ФС-операции и подпроцессы.
pub struct LiveImpIo {
    /// Апскейл обоев (None — можно только can_import).
    upscaler: Option<UpscaleFn>,
    /// Первая ошибка side-effect операции (текст для OSError).
    pub io_err: Option<String>,
    /// Счётчик для уникальности имён mkdtemp внутри процесса.
    tmp_seq: u64,
}

impl LiveImpIo {
    /// Исполнитель с апскейлом обоев (`None` — без апскейла).
    pub fn new(upscaler: Option<UpscaleFn>) -> Self {
        LiveImpIo { upscaler, io_err: None, tmp_seq: 0 }
    }

    fn fail(&mut self, what: &str, path: &str, e: &std::io::Error) {
        if self.io_err.is_none() {
            self.io_err = Some(format!("{what} {path}: {e}"));
        }
    }
}

/// Есть ли исполняемый файл по пути (для which).
fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

// Имена внешних конвертеров в разных дистрибутивах — зеркало таблиц
// `_TOOL_SETS` / `_TOOL_SUBCOMMANDS` из `rexon/utils.py`. Ядро строит argv
// с КАНОНИЧЕСКИМ именем (`convert`, `7z`, `identify`), а знает реальные
// имена исполнитель — здесь этот модуль, а не Python (у прочих планов
// подменой занимается `utils.safe_run`). Без этого проверка «программа
// есть» на стороне Python (там `convert` находится как `magick`)
// расходилась бы с запуском на стороне моста.
const TOOL_ALIASES: &[(&str, &[&str])] = &[
    // ImageMagick 7: `magick` — drop-in замена `convert`
    ("convert", &["convert", "magick"]),
    // пакет `7zip` кладёт `7zz` (Debian/Ubuntu), старый p7zip — `7za`
    ("7z", &["7z", "7zz", "7za"]),
];

// Инструменты, у которых своего бинаря может не быть вовсе: в ImageMagick 7
// `identify` — это подкоманда `magick identify`, то есть ДВА слова.
const TOOL_SUBCOMMANDS: &[(&str, &[&str])] = &[("identify", &["magick", "identify"])];

/// Есть ли программа с таким именем в PATH.
fn in_path(name: &str) -> bool {
    if name.contains('/') {
        return is_executable(std::path::Path::new(name));
    }
    let Some(paths) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&paths).any(|d| is_executable(&d.join(name)))
}

/// Есть ли инструмент в системе — с учётом синонимов и подкоманд.
pub fn tool_available(name: &str) -> bool {
    if in_path(name) {
        return true;
    }
    for (canon, names) in TOOL_ALIASES {
        if *canon == name && names.iter().any(|n| in_path(n)) {
            return true;
        }
    }
    for (canon, prefix) in TOOL_SUBCOMMANDS {
        if *canon == name && in_path(prefix[0]) {
            return true;
        }
    }
    false
}

/// argv с реальным именем инструмента (если каноническое не нашлось).
fn resolved_tool_argv(argv: Vec<String>) -> Vec<String> {
    let Some(head) = argv.first() else { return argv };
    if in_path(head) {
        return argv;
    }
    for (canon, names) in TOOL_ALIASES {
        if canon == head {
            if let Some(found) = names.iter().find(|n| in_path(n)) {
                let mut out = argv.clone();
                out[0] = (*found).to_string();
                return out;
            }
        }
    }
    for (canon, prefix) in TOOL_SUBCOMMANDS {
        if canon == head && in_path(prefix[0]) {
            let mut out: Vec<String> = prefix.iter().map(|s| s.to_string()).collect();
            out.extend(argv.into_iter().skip(1));
            return out;
        }
    }
    argv
}

/// Рекурсивный обход для walk (topdown, симлинки-каталоги не раскрываются).
fn walk_rec(dir: &std::path::Path, out: &mut Vec<core_imp::WalkEntry>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut descend = Vec::new();
    for n in names {
        let p = dir.join(&n);
        if p.is_dir() {
            let is_link = p
                .symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if !is_link {
                descend.push(p);
            }
            dirs.push(n);
        } else {
            files.push(n);
        }
    }
    out.push((dir.to_string_lossy().into_owned(), dirs, files));
    for p in descend {
        walk_rec(&p, out);
    }
}

/// Рекурсивное копирование каталога (shutil.copytree, symlinks=False).
fn copy_tree_rec(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dest.join(entry.file_name());
        if s.is_dir() {
            copy_tree_rec(&s, &d)?;
        } else {
            std::fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

/// Запустить внешний конвертер: `(получилось ли, stdout)`.
///
/// Общее тело для обоих живых исполнителей модуля (`ImpIo::run` импортёров и
/// `UpIo::run` апскейлера): у них один и тот же оригинал — `utils.run_tool`,
/// и две копии этого цикла разошлись бы молча (правило #12).
fn run_tool(argv: Vec<String>, timeout: u64) -> (bool, Vec<u8>) {
        use std::io::Read;
        use std::process::{Command, Stdio};
        let none = (false, Vec::new());
        // Имя конвертера — через общий резолвер: на ImageMagick 7 вместо
        // `convert` запустится `magick`, вместо `identify` — `magick
        // identify`. Программы нет вовсе — ниже честный неуспех команды.
        let argv = resolved_tool_argv(argv);
        let Some((bin, rest)) = argv.split_first() else { return none };
        let child = Command::new(bin)
            .args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            // бинарника нет — допущение исполнителя: как неуспех команды
            Err(_) => return none,
        };
        let reader = |pipe: Option<std::process::ChildStdout>,
                      errpipe: Option<std::process::ChildStderr>| {
            let mut out = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut out);
            }
            if let Some(mut p) = errpipe {
                let mut sink = Vec::new();
                let _ = p.read_to_end(&mut sink);
            }
            out
        };
        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let t = std::thread::spawn(move || reader(stdout_pipe, stderr_pipe));
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(timeout);
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Some(st),
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        // таймаут — допущение исполнителя: неуспех команды
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_) => break None,
            }
        };
        let stdout = t.join().unwrap_or_default();
        (status.map(|s| s.success()).unwrap_or(false), stdout)
}

impl core_imp::ImpIo for LiveImpIo {
    fn mkdtemp(&mut self, prefix: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::var_os("TMPDIR")
            .filter(|v| !v.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
        let mut last = base.join(prefix);
        for _ in 0..100 {
            self.tmp_seq += 1;
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            let name = format!(
                "{prefix}{:x}{:x}{:x}",
                std::process::id(),
                nanos,
                self.tmp_seq
            );
            last = base.join(&name);
            match std::fs::create_dir(&last) {
                Ok(()) => {
                    let _ = std::fs::set_permissions(
                        &last,
                        std::fs::Permissions::from_mode(0o700),
                    );
                    return last.to_string_lossy().into_owned();
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => {
                    self.fail("mkdtemp", &last.to_string_lossy(), &e);
                    break;
                }
            }
        }
        last.to_string_lossy().into_owned()
    }

    fn makedirs(&mut self, path: &str) {
        if let Err(e) = std::fs::create_dir_all(path) {
            self.fail("makedirs", path, &e);
        }
    }

    fn run(&mut self, argv: Vec<String>, timeout: u64) -> core_imp::RunResult {
        let (ok, stdout) = run_tool(argv, timeout);
        core_imp::RunResult { ok, stdout }
    }

    fn walk(&mut self, root: &str) -> Vec<core_imp::WalkEntry> {
        let mut out = Vec::new();
        walk_rec(std::path::Path::new(root), &mut out);
        out
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(path) else { return Vec::new() };
        let mut names: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn is_file(&mut self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }

    fn is_dir(&mut self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }

    fn exists(&mut self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn getsize(&mut self, path: &str) -> u64 {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        // errors="replace": U+FFFD на битых байтах — как from_utf8_lossy
        std::fs::read(path)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    fn read_head_text(&mut self, path: &str, n: usize) -> Option<String> {
        // .read(n) в текстовом режиме — n СИМВОЛОВ
        self.read_text(path).map(|s| s.chars().take(n).collect())
    }

    fn read_head_bytes(&mut self, path: &str, n: usize) -> Option<Vec<u8>> {
        std::fs::read(path).ok().map(|mut b| {
            b.truncate(n);
            b
        })
    }

    fn read_bytes(&mut self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn is_zipfile(&mut self, path: &str) -> Option<bool> {
        match std::fs::File::open(path) {
            Err(_) => None,
            Ok(f) => Some(zip::ZipArchive::new(f).is_ok()),
        }
    }

    fn zip_open(&mut self, path: &str) -> bool {
        // ошибка открытия файла приравнена к BadZipFile (допущение)
        std::fs::File::open(path)
            .ok()
            .map(|f| zip::ZipArchive::new(f).is_ok())
            .unwrap_or(false)
    }

    fn which(&mut self, name: &str) -> bool {
        // С учётом синонимов — иначе ядро сочло бы `7z` отсутствующим там,
        // где пакет положил только `7zz` (см. `TOOL_ALIASES`).
        tool_available(name)
    }

    fn copy_file(&mut self, src: &str, dest: &str) {
        if let Err(e) = std::fs::copy(src, dest) {
            self.fail("copy", src, &e);
        }
    }

    fn copy_tree(&mut self, src: &str, dest: &str) {
        if let Err(e) = copy_tree_rec(
            std::path::Path::new(src),
            std::path::Path::new(dest),
        ) {
            self.fail("copytree", src, &e);
        }
    }

    fn symlink(&mut self, target: &str, link: &str) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    fn remove_file(&mut self, path: &str) {
        if let Err(e) = std::fs::remove_file(path) {
            self.fail("remove", path, &e);
        }
    }

    fn remove_tree(&mut self, path: &str) {
        // shutil.rmtree(..., ignore_errors=True)
        let _ = std::fs::remove_dir_all(path);
    }

    fn write_file(&mut self, path: &str, content: &str) {
        if let Err(e) = std::fs::write(path, content) {
            self.fail("write", path, &e);
        }
    }

    fn extract_zip(&mut self, archive: &str, dest: &str) {
        if let Err(e) = core_ext::safe_zip_extract(
            std::path::Path::new(archive),
            std::path::Path::new(dest),
        ) {
            let msg = match e {
                core_ext::ArchiveError::ZipSlip(m)
                | core_ext::ArchiveError::BadZip(m)
                | core_ext::ArchiveError::Io(m) => m,
            };
            if self.io_err.is_none() {
                self.io_err = Some(format!("extract_zip {archive}: {msg}"));
            }
        }
    }

    fn pack_rxn(&mut self, src: &str, dest: &str) {
        if let Err(e) = core_ext::pack_rxn(
            std::path::Path::new(src),
            std::path::Path::new(dest),
        ) {
            let msg = match e {
                core_ext::ArchiveError::ZipSlip(m)
                | core_ext::ArchiveError::BadZip(m)
                | core_ext::ArchiveError::Io(m) => m,
            };
            if self.io_err.is_none() {
                self.io_err = Some(format!("pack_rxn {dest}: {msg}"));
            }
        }
    }

    fn write_bytes(&mut self, path: &str, data: &[u8]) {
        if let Err(e) = std::fs::write(path, data) {
            self.fail("write", path, &e);
        }
    }

    fn zip_names(&mut self, path: &str) -> Option<Vec<String>> {
        let f = std::fs::File::open(path).ok()?;
        let zip = zip::ZipArchive::new(f).ok()?;
        Some(zip.file_names().map(str::to_string).collect())
    }

    fn is_symlink(&mut self, path: &str) -> bool {
        std::fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }

    fn upscale_wallpapers(&mut self, data_dir: &str) -> u64 {
        match &mut self.upscaler {
            Some(cb) => cb(data_dir),
            None => 0,
        }
    }
}

// ============================================================================
// Курсоры Windows: стабильный вход для витрин и «перевести заново»
// ============================================================================

/// Семейство пакетного менеджера этой машины (для имён пакетов в отказах).
fn live_family() -> String {
    crate::system::DistroDetector::detect_live()
        .family_live()
        .as_str()
        .to_string()
}

/// ⭐ СТАБИЛЬНЫЙ ВХОД: набор курсоров Windows → `.rxn` в `out_dir`.
///
/// `path` — архив (`.zip`/`.rar`/`.7z`), папка, `.inf`/`.crs` или один
/// `.cur`/`.ani`. В `.rxn` — тема Xcursor (все кадры анимации), оригиналы
/// набора и manifest с `source: "windows"` и сведениями из `source`
/// (сайт, адрес страницы, автор, лицензия). Ставится потом как обычный
/// `.rxn` (`ext_exec::install_from_file`). Ошибка — готовый текст для
/// человека на языке интерфейса.
///
/// Нужны ли чужие программы: только для `.rar`/`.7z` (`bsdtar` или `7z`);
/// их нет — отказ с именем пакета этой системы ДО любых действий.
pub fn import_windows_cursor_archive(
    path: &std::path::Path,
    out_dir: &std::path::Path,
    source: &core_imp::CursorSource,
) -> Result<std::path::PathBuf, String> {
    let p = path.to_string_lossy().into_owned();
    let family = live_family();
    core_imp::require_tools(core_imp::import_kind(&p, "cursor"), &family, &|t| {
        tool_available(t)
    })?;
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let out = out_dir.to_string_lossy().into_owned();
    let mut io = LiveImpIo::new(None);
    let result = core_imp::cursor_import_with_source(&mut io, &p, source, Some(&out));
    if let Some(message) = io.io_err {
        return Err(message);
    }
    result
        .map(std::path::PathBuf::from)
        .map_err(|e| core_imp::friendly_error_text(&e.message, &family).unwrap_or(e.message))
}

/// Перевести тему курсоров Windows заново из её оригиналов — без перекачки.
///
/// `src` — `.rxn` или каталог (распакованный `.rxn` либо установленное
/// расширение `~/.config/rexon/extensions/wincur-…`). Новый `.rxn` с тем же
/// id кладётся в `out_dir`; ставится как обычно и заменяет прежний.
pub fn retranslate_windows_cursors(
    src: &std::path::Path,
    out_dir: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let out = out_dir.to_string_lossy().into_owned();
    let mut io = LiveImpIo::new(None);
    let mut unpacked: Option<std::path::PathBuf> = None;
    let dir = if src.is_dir() {
        src.to_path_buf()
    } else {
        let tmp = std::path::PathBuf::from(core_imp::ImpIo::mkdtemp(&mut io, "rexon_wincur_rxn_"));
        unpacked = Some(tmp.clone());
        if let Err(e) = core_ext::safe_zip_extract(src, &tmp) {
            let _ = std::fs::remove_dir_all(&tmp);
            let msg = match e {
                core_ext::ArchiveError::ZipSlip(m)
                | core_ext::ArchiveError::BadZip(m)
                | core_ext::ArchiveError::Io(m) => m,
            };
            return Err(crate::i18n::trf("Couldn't unpack the archive: %1", &[&msg]));
        }
        tmp
    };
    let result =
        core_imp::cursor_retranslate_dir(&mut io, &dir.to_string_lossy(), &out);
    if let Some(tmp) = unpacked {
        let _ = std::fs::remove_dir_all(tmp);
    }
    if let Some(message) = io.io_err {
        return Err(message);
    }
    result.map(std::path::PathBuf::from).map_err(|e| e.message)
}

// ============================================================================
// Живой исполнитель апскейлера обоев (`upscaler::UpIo`)
// ============================================================================
//
// ЗАЧЕМ ЗДЕСЬ. Импорт `.themepack`/`.theme` заканчивается апскейлом маленьких
// обоев (`ImpIo::upscale_wallpapers`), а сам апскейл — это второй трейт со
// своим живым исполнителем. Без него переезд импортёров в процесс без Python
// был бы разменом: импорт заработал бы, а обои из тем XP/Vista остались бы
// в своём 800×600. Поэтому исполнитель апскейлера лежит рядом с исполнителем
// импортёров и подставляется одной функцией [`upscale_hook`].
//
// РАЗЛИЧИЕ С PYTHON-ИСПОЛНИТЕЛЕМ ровно одно и названо вслух: `image_size`.
// Оригинал спрашивает размер сначала у Pillow и лишь потом у `identify`;
// Pillow здесь нет, поэтому метод не переопределяется и ядро идёт запасным
// путём через `identify`. Для этого пути это не потеря: весь конвейер импорта
// Windows-темы и без того держится на ImageMagick (`convert` для превью и
// битмапов), то есть на машине, где импорт вообще возможен, `identify` есть.

/// Живой исполнитель [`crate::upscaler::UpIo`]: настоящие ФС и подпроцессы.
pub struct LiveUpIo {
    /// Куда сказать, что апскейл пропущен и почему.
    notes: Notes,
    /// Семейство пакетного менеджера — чтобы назвать пакет ИМЕНЕМ этой
    /// системы, а не арчевым (правило #13).
    family: String,
}

impl LiveUpIo {
    /// Исполнитель, который умеет пожаловаться.
    pub fn new(notes: Notes, family: &str) -> Self {
        LiveUpIo { notes, family: family.to_string() }
    }
}

/// Домашний каталог (`$HOME`) либо текущий — как у прочих точек ядра.
fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
}

/// Путь исполняемого файла в `$PATH` — семантика `shutil.which`.
fn which_path(name: &str) -> Option<String> {
    if name.contains('/') {
        let p = std::path::Path::new(name);
        return is_executable(p).then(|| name.to_string());
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|d| d.join(name))
        .find(|p| is_executable(p))
        .map(|p| p.to_string_lossy().into_owned())
}

/// `os.access(path, os.X_OK)`.
fn access_x(path: &str) -> bool {
    is_executable(std::path::Path::new(path))
}

/// Выставить права `0o755` (порт `os.chmod(..., 0o755)`).
fn chmod_755(path: &str) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
}

impl crate::upscaler::UpIo for LiveUpIo {
    /// Размер картинки — СВОИМИ силами, без посторонних программ.
    ///
    /// Порядок ровно как у оригинала, только вместо Pillow — разбор
    /// заголовка самого файла: он покрывает все пять форматов, до которых
    /// апскейлер вообще добирается. Не опознали — пробуем `identify`. Не
    /// вышло и это — говорим об этом словами и отвечаем «0×0»: тот же
    /// ответ, что ядро получило бы само, но теперь человек о нём узнает.
    ///
    /// `identify` зовётся здесь, а не оставляется ядру, именно ради
    /// последней ветки: иначе исполнителю неоткуда узнать, что запасной ход
    /// тоже не сработал, и пропуск снова стал бы молчаливым.
    fn image_size(&mut self, path: &str) -> Option<(i64, i64)> {
        let head = read_head_bytes(path, crate::upscaler::HEADER_PROBE_BYTES);
        if let Some(size) = head
            .as_deref()
            .and_then(crate::upscaler::image_size_from_header)
        {
            return Some(size);
        }
        let r = run_tool(
            vec![
                "identify".into(),
                "-format".into(),
                "%w %h".into(),
                path.into(),
            ],
            10,
        );
        if r.0 {
            let size = crate::upscaler::parse_identify(&r.1);
            if size.0 > 0 && size.1 > 0 {
                return Some(size);
            }
        }
        note(&self.notes, crate::upscaler::note_upscale_skipped(&self.family));
        Some((0, 0))
    }

    fn run(&mut self, argv: Vec<String>, timeout: u64) -> crate::upscaler::RunResult {
        let (ok, stdout) = run_tool(argv, timeout);
        crate::upscaler::RunResult { ok, stdout }
    }

    fn which(&mut self, name: &str) -> Option<String> {
        which_path(name)
    }

    fn exists(&mut self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn is_file(&mut self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }

    fn access_x(&mut self, path: &str) -> bool {
        access_x(path)
    }

    fn remove_file(&mut self, path: &str) {
        // ошибка подавлена оригиналом (`except OSError: log.debug`)
        let _ = std::fs::remove_file(path);
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(path) else { return Vec::new() };
        let mut names: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn replace(&mut self, src: &str, dst: &str) {
        let _ = std::fs::rename(src, dst);
    }

    fn download_realesrgan(&mut self) -> Option<String> {
        download_realesrgan_impl()
    }
}

/// Порт `WallpaperUpscaler._download_realesrgan_impl`: скачать и распаковать
/// Real-ESRGAN, вернуть путь к бинарю.
///
/// Оригинал (Python) в этом месте всегда падал `NameError` — `zipfile` и
/// `_safe_zip_extract` в модуле не импортированы, — то есть автоскачивания у
/// покупателя не было ни разу. Здесь оно работает: правило #16 (баг оригинала
/// чинится в порте). Любой сбой — тихий `None`: апскейл просто уйдёт на
/// следующую стратегию (waifu2x, затем ImageMagick).
fn download_realesrgan_impl() -> Option<String> {
    use crate::net::NetIo;

    let home = home_dir();
    let local_exe = crate::upscaler::esrgan_local_exe(&home);
    let esrgan_dir = std::path::Path::new(&local_exe).parent()?.to_path_buf();
    std::fs::create_dir_all(&esrgan_dir).ok()?;
    let zip_path = esrgan_dir.join("realesrgan.zip");

    // Скачивание потоком в файл, а не тело целиком в память: архив
    // Real-ESRGAN — десятки мегабайт, и `urlretrieve` оригинала тоже писал
    // его на диск по кускам.
    let mut net = crate::net::UreqNet::new();
    let zip_str = zip_path.to_string_lossy().into_owned();
    let ok = (|| -> bool {
        if net.open(crate::upscaler::ESRGAN_URL, "Rexon", 60).is_err() {
            return false;
        }
        if net.create(&zip_str).is_err() {
            net.close();
            return false;
        }
        loop {
            match net.read(64 * 1024) {
                Ok(chunk) if chunk.is_empty() => break,
                Ok(chunk) => {
                    if net.write(&chunk).is_err() {
                        net.close_file();
                        net.close();
                        return false;
                    }
                }
                Err(_) => {
                    net.close_file();
                    net.close();
                    return false;
                }
            }
        }
        net.close_file();
        net.close();
        true
    })();
    if !ok {
        let _ = std::fs::remove_file(&zip_path);
        return None;
    }

    if core_ext::safe_zip_extract(&zip_path, &esrgan_dir).is_err() {
        let _ = std::fs::remove_file(&zip_path);
        return None;
    }

    if std::path::Path::new(&local_exe).is_file() {
        chmod_755(&local_exe);
    } else {
        // Бинарь может лежать в подпапке архива — как и у оригинала, первый
        // найденный переносится на штатное место вместе со своими моделями.
        let mut walked = Vec::new();
        walk_rec(&esrgan_dir, &mut walked);
        'outer: for (root, _dirs, files) in &walked {
            for f in files {
                if f != "realesrgan-ncnn-vulkan" {
                    continue;
                }
                let found = std::path::Path::new(root).join(f);
                let found_s = found.to_string_lossy().into_owned();
                chmod_755(&found_s);
                if found_s != local_exe {
                    let _ = std::fs::rename(&found, &local_exe);
                }
                for mf in std::fs::read_dir(root).into_iter().flatten().flatten() {
                    let name = mf.file_name().to_string_lossy().into_owned();
                    if !name.ends_with(".bin") && !name.ends_with(".param") {
                        continue;
                    }
                    let dst = esrgan_dir.join(&name);
                    if !dst.exists() {
                        let _ = std::fs::copy(mf.path(), dst);
                    }
                }
                break 'outer;
            }
        }
    }

    let _ = std::fs::remove_file(&zip_path);

    if std::path::Path::new(&local_exe).is_file() && access_x(&local_exe) {
        return Some(local_exe);
    }
    None
}

/// Прочитать начало файла (до `limit` байт). `None` — файл не открылся.
fn read_head_bytes(path: &str, limit: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let f = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.take(limit as u64).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Апскейл обоев живым исполнителем — готовое замыкание для [`LiveImpIo`].
///
/// Порт `WallpaperUpscaler.process_wallpapers_in_dir`: в процессе с Python его
/// подставлял мост, здесь то же делает само ядро. Пропущенные шаги попадают в
/// `notes`, а не в тишину.
pub fn upscale_hook(notes: Notes, family: &str) -> UpscaleFn {
    let family = family.to_string();
    Box::new(move |data_dir: &str| {
        let local_exe = crate::upscaler::esrgan_local_exe(&home_dir());
        let mut io = LiveUpIo::new(notes.clone(), &family);
        crate::upscaler::process_wallpapers_in_dir(&mut io, data_dir, &local_exe)
            .max(0) as u64
    })
}
