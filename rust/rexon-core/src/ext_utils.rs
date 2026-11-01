//! Утилиты работы с расширениями — порт `rexon/extensions/utils.py`.
//!
//! Безопасная распаковка ZIP (защита от ZipSlip), упаковка `.rxn`, запись
//! `manifest.json`, безопасное удаление каталога.
//!
//! ⚠️ Сверка ZIP-упаковки — СТРУКТУРНАЯ, не побайтовая: `.zip` содержит
//! таймстемпы файлов и зависит от версии/уровня deflate, поэтому байт-в-байт
//! с Python не совпадёт в принципе. Проверяем инвариант, который важен для
//! приложения: набор имён-в-архиве и содержимое каждого файла (golden-тест
//! `ext_utils_parity.rs` сверяет их с архивом, собранным самим Python).

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use zip::write::SimpleFileOptions;

/// Лексическая нормализация пути (без обращения к ФС): убирает `.`, схлопывает
/// `..`. Аналог того, что `os.path.realpath` делает с несуществующим путём
/// (символические ссылки при этом НЕ разрешаются — для свежего каталога
/// распаковки их нет; осознанное отклонение от Python-realpath).
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `os.path.realpath` для, возможно, ещё не созданного пути: канонизируется
/// самый глубокий существующий предок, несуществующий хвост добавляется
/// лексически (как в CPython posixpath.realpath, strict=False).
fn realpath_missing_ok(path: &Path) -> std::io::Result<PathBuf> {
    if let Ok(real) = std::fs::canonicalize(path) {
        return Ok(real);
    }
    let lexical = normalize_lexical(&std::path::absolute(path)?);
    let mut existing = lexical.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if existing.exists() {
            break;
        }
        let Some(name) = existing.file_name() else { break };
        tail.push(name.to_os_string());
        if !existing.pop() {
            break;
        }
    }
    let mut real = std::fs::canonicalize(&existing).unwrap_or(existing);
    for name in tail.into_iter().rev() {
        real.push(name);
    }
    Ok(real)
}

/// Куда `zipfile.extractall` реально кладёт запись — порт санитайзера
/// `ZipFile._extract_member`: имя разбивается по `/`, а пустые компоненты,
/// `.` и `..` ВЫБРАСЫВАЮТСЯ (не схлопываются). Поэтому `a/../b.txt` у Python
/// оказывается в `a/b.txt`, а не в `b.txt`, а ведущий `/` просто теряется.
///
/// Раньше здесь стоял `normalize_lexical`, который `..` схлопывал, — тот же
/// архив распаковывался Rust'ом в ДРУГОЙ каталог, чем Python (у `.rxn` это
/// значит другой корень с `manifest.json`). Проверка ZipSlip выше остаётся на
/// схлопывающей нормализации: она повторяет `os.path.realpath` из проверки
/// оригинала.
fn extract_member_name(name: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            continue;
        }
        out.push(part);
    }
    out
}

/// `repr()` строки как в CPython — для `{имя!r}` в текстах ошибок оригинала.
/// Печатаемые не-ASCII остаются как есть; экранируются `\`, `\n`, `\r`, `\t`,
/// сама кавычка и управляющие C0/DEL.
pub(crate) fn py_repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Сообщение об ошибке ZIP словами оригинала (`zipfile`).
///
/// Тексты крейта `zip` английские и внутренние («invalid Zip archive: Could
/// not find EOCD»), а доезжают они прямиком до окна покупателя: установщик
/// `.rxn` подклеивает `str(e)` к своему префиксу. `zipfile` в тех же местах
/// поднимает `BadZipFile` с короткими текстами — их и отдаём, иначе Rust-путь
/// и Python-фолбэк говорят покупателю разное про один и тот же архив.
pub fn zip_error_text(e: &zip::result::ZipError) -> String {
    use zip::result::ZipError;
    match e {
        // Не ZIP вовсе либо обрезан так, что каталога записей нет.
        ZipError::InvalidArchive(_) | ZipError::UnsupportedArchive(_) => {
            "File is not a zip file".to_string()
        }
        ZipError::FileNotFound => "There is no item named in the archive".to_string(),
        ZipError::InvalidPassword => "Bad password for file".to_string(),
        ZipError::Io(io) => io.to_string(),
        // Крейт помечен non_exhaustive: всё новое — тоже «не тот архив».
        _ => "File is not a zip file".to_string(),
    }
}

/// Ошибка чтения записи архива словами оригинала. Крейт `zip` отдаёт
/// несовпадение контрольной суммы обычной io-ошибкой «Invalid checksum»,
/// `zipfile` — `BadZipFile("Bad CRC-32 for file 'имя'")`.
fn entry_read_error(e: std::io::Error, member: &str) -> ArchiveError {
    if e.kind() == std::io::ErrorKind::InvalidData && e.to_string().contains("Invalid checksum") {
        return ArchiveError::BadZip(format!("Bad CRC-32 for file {}", py_repr_str(member)));
    }
    ArchiveError::Io(e.to_string())
}

/// Ошибка распаковки/упаковки.
#[derive(Debug)]
pub enum ArchiveError {
    /// Обнаружен ZipSlip — опасный путь в архиве (аналог `ValueError`).
    ZipSlip(String),
    /// Повреждённый архив — аналог `zipfile.BadZipFile`. Отдельно от `Io`,
    /// потому что `BadZipFile` НЕ наследник `OSError`: вызывающие (установка
    /// `.rxn`, импортёр иконок) ловят именно его, чтобы сказать «Файл
    /// повреждён…» и уйти на запасной путь.
    BadZip(String),
    /// Ошибка ввода-вывода.
    Io(String),
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchiveError::ZipSlip(m) | ArchiveError::BadZip(m) | ArchiveError::Io(m) => {
                write!(f, "{m}")
            }
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<std::io::Error> for ArchiveError {
    fn from(e: std::io::Error) -> Self {
        ArchiveError::Io(e.to_string())
    }
}

impl From<zip::result::ZipError> for ArchiveError {
    fn from(e: zip::result::ZipError) -> Self {
        match e {
            zip::result::ZipError::Io(io) => ArchiveError::Io(io.to_string()),
            other => ArchiveError::BadZip(zip_error_text(&other)),
        }
    }
}

/// Распаковать ZIP с защитой от path traversal (ZipSlip) — порт
/// `_safe_zip_extract`. Сначала проверяет ВСЕ записи (как Python: сперва
/// цикл проверки, потом `extractall`), при опасном пути — ошибка до записи
/// хоть одного файла.
pub fn safe_zip_extract(archive_path: &Path, dest: &Path) -> Result<(), ArchiveError> {
    // Python: os.path.realpath(dest) — существования dest НЕ требует
    // (extractall создаёт каталоги сам); canonicalize ещё не созданного
    // назначения (импортёры зовут с dest внутри свежего mkdtemp) шёл бы
    // ENOENT — канонизируем ближайшего существующего предка, хвост
    // добавляем лексически.
    let dest = realpath_missing_ok(dest)?;
    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    // Проверка путей (ZipSlip): target должен быть внутри dest либо == dest.
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let target = normalize_lexical(&dest.join(entry.name()));
        if !target.starts_with(&dest) {
            return Err(ArchiveError::ZipSlip(crate::i18n::trf(
                "ZipSlip: unsafe path in the archive: %1",
                &[&py_repr_str(entry.name())],
            )));
        }
    }

    // Извлечение (пути — как их формирует zipfile.extractall).
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let member = entry.name().to_string();
        let rel = extract_member_name(&member);
        if rel.as_os_str().is_empty() {
            // Имя целиком из отброшенных компонентов ("/", "./", "..") —
            // extractall для такой записи не создаёт ничего.
            continue;
        }
        let target = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.is_symlink() {
            // Ссылка-синоним внутри архива (см. [`pack_rxn`]) восстанавливается
            // ссылкой — только относительная и без `..`, то есть не дальше
            // своего каталога вглубь; иная пропускается (за пределы `dest`
            // архив не дотянется ни записью, ни ссылкой).
            let mut link_to = String::new();
            entry
                .read_to_string(&mut link_to)
                .map_err(|e| entry_read_error(e, &member))?;
            if is_inner_link_target(Path::new(&link_to)) {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let _ = std::fs::remove_file(&target);
                std::os::unix::fs::symlink(&link_to, &target)?;
            }
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&target)?;
            // Битая полезная нагрузка при живом каталоге записей — это
            // BadZipFile («Bad CRC-32…»), а не OSError.
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| entry_read_error(e, &member))?;
        }
    }
    Ok(())
}

/// Упаковать каталог в `.rxn` (ZIP, DEFLATE) — порт `_pack_rxn`. Обходит
/// дерево, кладёт файлы под относительными к `source_dir` именами.
///
/// Таймстемпы намеренно НЕ фиксируются — как и в Python; сверять архив
/// побайтово нельзя (см. заголовок модуля), только структурно.
pub fn pack_rxn(source_dir: &Path, rxn_path: &Path) -> Result<(), ArchiveError> {
    let file = std::fs::File::create(rxn_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut files = Vec::new();
    collect_files(source_dir, &mut files)?;
    for full in files {
        let arcname = full
            .strip_prefix(source_dir)
            .expect("файл внутри source_dir")
            .to_string_lossy()
            .replace('\\', "/");
        // Ссылка-синоним на файл того же дерева уходит ССЫЛКОЙ, а не копией:
        // тема курсоров держит десятки имён одного курсора (`left_ptr`,
        // `default`, `arrow`…), и копии анимаций раздули бы `.rxn` в разы.
        // Ссылка наружу или на несуществующее — по-прежнему содержимым.
        if let Ok(link_to) = std::fs::read_link(&full) {
            let resolved = full.parent().map(|p| p.join(&link_to));
            if is_inner_link_target(&link_to) && resolved.is_some_and(|r| r.is_file()) {
                zip.add_symlink(arcname, link_to.to_string_lossy(), options)?;
                continue;
            }
        }
        zip.start_file(arcname, options)?;
        let mut f = std::fs::File::open(&full)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        zip.write_all(&buf)?;
    }
    zip.finish()?;
    Ok(())
}

/// Ссылка «внутрь»: относительная, непустая, без `..` и корня — значит, не
/// выходит за каталог, где лежит.
fn is_inner_link_target(target: &Path) -> bool {
    !target.as_os_str().is_empty()
        && target
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Рекурсивно собрать пути ФАЙЛОВ (не каталогов) под `dir` — аналог
/// `os.walk` + сбор `files`.
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Записать `manifest.json` в каталог — порт `_write_manifest`
/// (`json.dump(indent=2, ensure_ascii=False)` — 2 пробела, UTF-8 без
/// экранирования). Совпадает побайтово с Python.
pub fn write_manifest(manifest: &Value, directory: &Path) -> std::io::Result<()> {
    let path = directory.join("manifest.json");
    let data = serde_json::to_string_pretty(manifest).expect("serde_json::Value сериализуется");
    std::fs::write(path, data)
}

/// Безопасное удаление каталога — порт `_safe_rmtree`.
///
/// * Несуществующий путь (и не симлинк) → `false`.
/// * Симлинк → удаляется САМА ссылка (не цель), `true`.
/// * Реальный путь ВНЕ `$HOME` и вне временного каталога → отказ, `false`;
///   сам `$HOME` (как и сам временный каталог) — тоже отказ, удаляем только
///   то, что лежит ВНУТРИ них.
/// * Иначе — рекурсивное удаление (ошибки игнорируются, как
///   `shutil.rmtree(ignore_errors=True)`), `true`.
///
/// Как и в Python, обычный ФАЙЛ внутри `$HOME` не удаляется (rmtree работает
/// только над каталогом), но функция всё равно возвращает `true`.
pub fn safe_rmtree(path: &Path) -> bool {
    let link_meta = std::fs::symlink_metadata(path);
    let is_symlink = link_meta.as_ref().map(|m| m.file_type().is_symlink()).unwrap_or(false);
    if !path.exists() && !is_symlink {
        return false;
    }
    if is_symlink {
        let _ = std::fs::remove_file(path);
        return true;
    }
    let real = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !rmtree_allowed(&real, &home_dir(), &std::env::temp_dir()) {
        return false;
    }
    let _ = std::fs::remove_dir_all(path);
    true
}

/// Снимет ли [`safe_rmtree`] что-нибудь по пути `path`: ссылку — да,
/// каталог — только под `$HOME` или временным каталогом (не сами они), файл
/// — нет (rmtree над файлом не работает). Откат установки
/// (`install_plan::Undo`) по этому ответу откладывает путь в сторону вместо
/// удаления — с теми же границами.
pub(crate) fn safe_rmtree_removes(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => true,
        Ok(m) if m.is_dir() => std::fs::canonicalize(path)
            .is_ok_and(|real| rmtree_allowed(&real, &home_dir(), &std::env::temp_dir())),
        _ => false,
    }
}

/// Разрешено ли удалять уже разрешённый (`realpath`) путь — вынесено из
/// [`safe_rmtree`] отдельной чистой функцией, чтобы проверку можно было
/// тестировать без настоящего `$HOME` на диске.
///
/// Эталон (`rexon/extensions/utils.py`) сравнивает с `home + os.sep`, то есть
/// требует строгого ПОТОМКА: `_py_safe_rmtree($HOME)` отказывает. У
/// `Path::starts_with` равенство истинно, поэтому корень отсекается явно —
/// без этого `safe_rmtree($HOME)` снёс бы домашний каталог целиком.
pub fn rmtree_allowed(real: &Path, home: &Path, tmp: &Path) -> bool {
    (real.starts_with(home) && real != home) || (real.starts_with(tmp) && real != tmp)
}

/// Домашний каталог из `$HOME` (fallback — текущий каталог).
fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}
