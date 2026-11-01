//! Установщик `.rxn` v4 adaptive-обоев — порт `rexon/rxn_v4_installer.py`
//! (ЧИСТАЯ логика; побочные эффекты — планом либо инъекцией наблюдений).
//!
//! Оригинал распаковывает `.rxn` (ZIP c `manifest.json` v4) в стандартное
//! место `~/.local/share/rexon/wallpapers/<title-slug>-<short-hash>/` и
//! раскладывает video/audio-треки. Здесь портирована вся детерминированная
//! часть:
//!   * [`slugify`]       — `title` → безопасный slug ФС (тот же алгоритм);
//!   * [`short_hash`]    — SHA-256 первых 16 МиБ файла → 8 hex-символов (тот
//!     же алгоритм: prefix-hash как уникальный идентификатор, не подпись);
//!   * [`collect_manifest_paths`] / [`verify_paths_present`] — сбор
//!     объявленных в манифесте путей и проверка их наличия в архиве (тексты
//!     ошибок байт-в-байт, включая `{p!r}`-repr);
//!   * [`plan_install_v4`] — весь конвейер `install_rxn_v4` ДО реальной
//!     записи: валидация схемы (переиспользуется [`crate::manifest_v4`]),
//!     проверка путей, вычисление slug+hash и целевого каталога, решение
//!     «уже установлено / установить» — результат [`InstallOutcome`] с ПЛАНОМ
//!     шагов ([`InstallStep`]) по образцу `ext_manager.rs`/`system_plan.rs`;
//!   * [`build_listed_entry`] — обогащение манифеста при листинге
//!     (`_videos_abs`/`_audio_abs`/`_preview_path`/… — раскладка треков в
//!     абсолютные пути, порядок ключей 1-в-1 с Python);
//!   * [`remove_slug_error`] — валидация slug для удаления (анти-traversal).
//!
//! НЕ портировано (исполнение — остаётся Python/мост): само чтение
//! `manifest.json` из ZIP (`parse_v4_manifest` — типы его исключений
//! FileNotFoundError/BadZipFile/KeyError/JSONDecodeError часть контракта,
//! см. `manifest_v4.py`), чтение `namelist()` архива, реальная распаковка
//! (`_safe_zip_extract`), создание каталогов, сканирование `INSTALL_ROOT`,
//! `os.path.realpath`/`is_file` при листинге, откат `_safe_rmtree` при
//! провале распаковки, логирование, CLI. Их выполняет исполнитель, а результат
//! (namelist, короткий хеш, существование каталога, наличие файлов) подаётся
//! сюда как вход. Эталон паритета — golden из живого Python
//! (`rxn_v4_installer_parity.rs`, снят read-only).
//!
//! Багов оригинала при разборе не найдено — код чистый (см. отчёт порта);
//! санкционированных расхождений с Python нет.

use std::collections::HashSet;

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::manifest_v4::validate_v4_manifest;

/// Длина короткого хеша в hex-символах (`_HASH_LEN`).
pub const HASH_LEN: usize = 8;
/// Сколько байт файла хешируется (первые 16 МиБ, как в `_short_hash`).
pub const HASH_INPUT_LIMIT: usize = 16 * 1024 * 1024;

// ============================================================================
// Хелперы семантики Python
// ============================================================================

/// `os.path.join`/`PurePath /` для POSIX (двух аргументов достаточно).
fn join(dir: &str, rel: &str) -> String {
    if rel.starts_with('/') || dir.is_empty() {
        rel.to_string()
    } else if dir.ends_with('/') {
        format!("{dir}{rel}")
    } else {
        format!("{dir}/{rel}")
    }
}

/// Последний компонент пути (`Path.name`).
fn basename(p: &str) -> &str {
    p.trim_end_matches('/').rsplit('/').next().unwrap_or(p)
}

/// Лексическая нормализация абсолютного POSIX-пути (аналог `Path.resolve()`
/// для дерева без символических ссылок): убирает `.`, схлопывает `..`.
fn normalize_abs(p: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for comp in p.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            c => stack.push(c),
        }
    }
    format!("/{}", stack.join("/"))
}

/// `str((sub / rel).resolve())` для уже-разрешённого абсолютного `sub` и
/// относительного `rel` (символических ссылок в свежераспакованном дереве
/// нет — осознанное отклонение от Python-realpath, как в `ext_utils.rs`).
fn resolve_join(sub: &str, rel: &str) -> String {
    normalize_abs(&join(sub, rel))
}

/// Истинность значения в смысле Python (`if v:`).
fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `repr()` строки: кавычки и экранирование как в CPython (для `{p!r}`,
/// `{slug!r}` в текстах ошибок). Печатаемые не-ASCII символы остаются как
/// есть; экранируются `\`, `\n`, `\r`, `\t`, кавычка и C0/DEL.
fn py_repr_str(s: &str) -> String {
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

// ============================================================================
// slugify + short_hash
// ============================================================================

/// `_slugify`: `title` → безопасный slug ФС.
///
/// `«The Witcher 3 — Geralt Theme»` → `«the-witcher-3-geralt-theme»`.
/// Соответствует Python `re.sub(r"[^a-z0-9]+", "-", text.lower().strip())
/// .strip("-") or "wallpaper"`: выживают только ASCII `a-z`/`0-9` (регистр
/// ASCII складывается одинаково в Python и Rust), всё прочее — прогон `-`.
pub fn slugify(text: &str) -> String {
    let lowered = text.to_lowercase();
    let mut out = String::new();
    let mut dash = false;
    for c in lowered.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let stripped = out.trim_matches('-');
    if stripped.is_empty() {
        "wallpaper".to_string()
    } else {
        stripped.to_string()
    }
}

/// `_short_hash`: SHA-256 первых `HASH_INPUT_LIMIT` байт `data` → первые
/// `HASH_LEN` hex-символов. `data` — содержимое `.rxn` (исполнитель читает
/// файл; лишнее за 16 МиБ здесь усекается, как в Python-цикле по `remaining`).
pub fn short_hash(data: &[u8]) -> String {
    let slice = if data.len() > HASH_INPUT_LIMIT {
        &data[..HASH_INPUT_LIMIT]
    } else {
        data
    };
    let digest = Sha256::digest(slice);
    let mut hex = String::with_capacity(HASH_LEN);
    for b in digest.iter() {
        if hex.len() >= HASH_LEN {
            break;
        }
        hex.push_str(&format!("{b:02x}"));
    }
    hex.truncate(HASH_LEN);
    hex
}

// ============================================================================
// Пути манифеста + проверка их наличия в архиве
// ============================================================================

/// `_collect_manifest_paths`: собрать все zip-внутренние пути, которые
/// обещает манифест, в порядке Python — значения `videos`, значения
/// `audio_tracks`, затем `fallback_video`/`fallback_audio`/`preview` (только
/// если истинны). `videos`/`audio_tracks` берутся, лишь если это словарь
/// (`(meta.get(k) or {}).values()`); значения предполагаются строками
/// (гарантируется валидацией перед вызовом).
pub fn collect_manifest_paths(meta: &Map<String, Value>) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for key in ["videos", "audio_tracks"] {
        if let Some(Value::Object(m)) = meta.get(key) {
            for (_k, v) in m {
                paths.push(v.as_str().unwrap_or_default().to_string());
            }
        }
    }
    for key in ["fallback_video", "fallback_audio", "preview"] {
        if let Some(v) = meta.get(key) {
            if is_truthy(v) {
                paths.push(v.as_str().unwrap_or_default().to_string());
            }
        }
    }
    paths
}

/// Чистая часть `_verify_paths_inside_zip`: `names` — множество имён внутри
/// архива (`ZipFile.namelist()`, читает исполнитель). Для каждого истинного
/// (непустого) объявленного пути проверяет наличие; при отсутствии —
/// `Err` с текстом байт-в-байт (`{p!r}`-repr). BadZipFile при открытии
/// архива — на стороне исполнителя (текст `Повреждённый ZIP: {e}`).
pub fn verify_paths_present(
    names: &HashSet<String>,
    paths: &[String],
) -> Result<(), String> {
    for p in paths {
        if !p.is_empty() && !names.contains(p) {
            return Err(format!(
                "Объявленный в manifest файл отсутствует в архиве: {}",
                py_repr_str(p)
            ));
        }
    }
    Ok(())
}

// ============================================================================
// План установки
// ============================================================================

/// Шаг плана установки — побочный вызов, который сделал бы `install_rxn_v4`
/// после всех проверок. Исполнение — при подключении к живому Python.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op")]
pub enum InstallStep {
    /// `Path.mkdir(parents=True, exist_ok=<exist_ok>)`.
    /// `INSTALL_ROOT` — `exist_ok=true`; целевой каталог — `exist_ok=false`.
    MakeDirs { path: String, exist_ok: bool },
    /// `_safe_zip_extract(ZipFile(archive), dest)`. При исключении исполнитель
    /// обязан выполнить откат `_safe_rmtree(dest)` и вернуть ошибку
    /// [`extract_error_message`].
    ExtractZip { archive: String, dest: String },
}

/// Итог `install_rxn_v4` (до реальной записи).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result")]
pub enum InstallOutcome {
    /// `(ok=False, message, install_dir=None)`.
    Error { message: String },
    /// `(ok=True, "Уже установлено: {install_dir}", Some(install_dir))` —
    /// каталог уже существовал, шагов нет.
    AlreadyInstalled { install_dir: String },
    /// Выполнить `steps`; при успехе — `(ok=True, "Установлено: {install_dir}",
    /// Some(install_dir))`.
    Plan {
        install_dir: String,
        steps: Vec<InstallStep>,
    },
}

/// Сообщение успешной установки (`install_rxn_v4` happy-path).
pub fn installed_message(install_dir: &str) -> String {
    format!("Установлено: {install_dir}")
}

/// Сообщение «уже установлено».
pub fn already_installed_message(install_dir: &str) -> String {
    format!("Уже установлено: {install_dir}")
}

/// Сообщение провала распаковки (исполнитель подставляет текст исключения).
pub fn extract_error_message(detail: &str) -> String {
    format!("Ошибка распаковки: {detail}")
}

/// Порт `install_rxn_v4` от шага 2 (валидация схемы) до вычисления плана
/// распаковки. Ранние побочные шаги — на исполнителе:
///   * `rxn_path.is_file()` → `Файл не найден: {rxn_path}`;
///   * `parse_v4_manifest` → `Не удалось прочитать manifest: {e}` (типы
///     исключений — контракт Python-моста).
///
/// Инъекции:
///   * `manifest` — уже разобранный `manifest.json`;
///   * `names` — `ZipFile(rxn_path).namelist()`;
///   * `short_hash` — [`short_hash`] от первых 16 МиБ файла;
///   * `install_root` — абсолютный `INSTALL_ROOT`
///     (`~/.local/share/rexon/wallpapers`);
///   * `dir_exists` — существует ли уже целевой каталог;
///   * `archive_path` — путь к `.rxn` (для шага распаковки).
pub fn plan_install_v4(
    manifest: &Value,
    names: &HashSet<String>,
    short_hash: &str,
    install_root: &str,
    dir_exists: bool,
    archive_path: &str,
) -> InstallOutcome {
    // 2. Валидация схемы.
    let (ok, err) = validate_v4_manifest(manifest);
    if !ok {
        return InstallOutcome::Error {
            message: format!("Неверный manifest: {err}"),
        };
    }
    // Валидный v4-манифест — всегда объект.
    let meta = match manifest.as_object() {
        Some(m) => m,
        None => {
            return InstallOutcome::Error {
                message: format!("Неверный manifest: {err}"),
            }
        }
    };

    // 3. Проверка наличия объявленных путей в архиве.
    let paths = collect_manifest_paths(meta);
    if let Err(message) = verify_paths_present(names, &paths) {
        return InstallOutcome::Error { message };
    }

    // 4. Целевой каталог: slug + короткий хеш.
    let title = meta.get("title").and_then(Value::as_str).unwrap_or_default();
    let slug = slugify(title);
    let install_dir = join(install_root, &format!("{slug}-{short_hash}"));

    if dir_exists {
        return InstallOutcome::AlreadyInstalled { install_dir };
    }

    // 5. План распаковки: mkdir INSTALL_ROOT (exist_ok) → mkdir целевой
    //    (без exist_ok) → безопасная распаковка.
    let steps = vec![
        InstallStep::MakeDirs {
            path: install_root.to_string(),
            exist_ok: true,
        },
        InstallStep::MakeDirs {
            path: install_dir.clone(),
            exist_ok: false,
        },
        InstallStep::ExtractZip {
            archive: archive_path.to_string(),
            dest: install_dir.clone(),
        },
    ];
    InstallOutcome::Plan { install_dir, steps }
}

// ============================================================================
// Листинг: обогащение манифеста абсолютными путями
// ============================================================================

/// Чистая часть `list_rxn_v4_wallpapers` (обогащение одной записи) — порт
/// сборки `meta_out`. `sub_dir` — уже-разрешённый абсолютный каталог
/// установки (`str(sub.resolve())`); флаги существования preview/fallback
/// вычисляет исполнитель (`Path.is_file()`).
///
/// Возвращает копию манифеста + служебные ключи В ТОМ ЖЕ ПОРЯДКЕ, что Python:
/// `_install_dir`, `_manifest_path`, `_preview_path`, `_videos_abs`,
/// `_audio_abs`, `_fallback_video_abs`, `_fallback_audio_abs`, `_slug`.
/// `_videos_abs`/`_audio_abs` строятся всегда (для КАЖДОГО трека), а
/// `_preview_path`/`_fallback_*_abs` — только если файл истинен И существует
/// (иначе `null`).
pub fn build_listed_entry(
    sub_dir: &str,
    manifest: &Map<String, Value>,
    preview_exists: bool,
    fallback_video_exists: bool,
    fallback_audio_exists: bool,
) -> Value {
    // Абсолютные пути треков (всегда, независимо от существования файла).
    let abs_map = |key: &str| -> Value {
        let mut m = Map::new();
        if let Some(Value::Object(tracks)) = manifest.get(key) {
            for (k, rel) in tracks {
                let rel = rel.as_str().unwrap_or_default();
                m.insert(k.clone(), Value::String(resolve_join(sub_dir, rel)));
            }
        }
        Value::Object(m)
    };

    // `_preview_path`/`_fallback_*_abs`: путь, только если файл существует.
    let gated = |key: &str, exists: bool| -> Value {
        if exists {
            let rel = manifest.get(key).and_then(Value::as_str).unwrap_or_default();
            Value::String(resolve_join(sub_dir, rel))
        } else {
            Value::Null
        }
    };

    let mut out: Map<String, Value> = manifest.clone();
    out.insert("_install_dir".into(), Value::String(sub_dir.to_string()));
    out.insert(
        "_manifest_path".into(),
        Value::String(resolve_join(sub_dir, "manifest.json")),
    );
    out.insert("_preview_path".into(), gated("preview", preview_exists));
    out.insert("_videos_abs".into(), abs_map("videos"));
    out.insert("_audio_abs".into(), abs_map("audio_tracks"));
    out.insert(
        "_fallback_video_abs".into(),
        gated("fallback_video", fallback_video_exists),
    );
    out.insert(
        "_fallback_audio_abs".into(),
        gated("fallback_audio", fallback_audio_exists),
    );
    out.insert(
        "_slug".into(),
        Value::String(basename(sub_dir).to_string()),
    );
    Value::Object(out)
}

// ============================================================================
// Удаление: валидация slug
// ============================================================================

/// Чистая часть `remove_rxn_v4_wallpaper`: проверка slug перед обращением к
/// ФС. `Some(msg)` — некорректный slug (пустой / содержит `/` или `\` /
/// начинается с точки), текст байт-в-байт (`{slug!r}`); `None` — slug
/// допустим (дальше исполнитель проверяет `INSTALL_ROOT/slug` и удаляет через
/// `_safe_rmtree`).
pub fn remove_slug_error(slug: &str) -> Option<String> {
    if slug.is_empty()
        || slug.contains('/')
        || slug.contains('\\')
        || slug.starts_with('.')
    {
        Some(format!("Некорректный slug: {}", py_repr_str(slug)))
    } else {
        None
    }
}

/// Сообщение «не установлено» при удалении (каталога нет).
pub fn not_installed_message(slug: &str) -> String {
    format!("Не установлено: {slug}")
}

/// Сообщение успешного удаления.
pub fn removed_message(target: &str) -> String {
    format!("Удалено: {target}")
}

/// Сообщение неудачного удаления (`_safe_rmtree` вернул false).
pub fn remove_failed_message(target: &str) -> String {
    format!("Не удалось удалить {target}")
}

/// Корень установки adaptive-обоев — `~/.local/share/rexon/wallpapers`.
///
/// ⚠ Правило #16: оригинал строил его как `Path.home()/".local/share"/…`,
/// в обход общего расширителя проекта — при нестандартном `$XDG_DATA_HOME`
/// установщик писал обои НЕ туда, где остальной Rexon держит свои данные
/// (тот же класс, что починенный `PlasmaProtector._expand`). Порт уводит
/// путь в XDG-корень данных; при стандартном окружении результат совпадает
/// с оригиналом байт-в-байт.
pub fn install_root(home: &str, xdg_data_home: Option<&str>) -> String {
    crate::utils::expand_user_xdg(
        "~/.local/share/rexon/wallpapers",
        home,
        None,
        xdg_data_home,
    )
}

/// `install_rxn_v4`: указанного `.rxn` нет на диске.
pub const MSG_FILE_NOT_FOUND_PREFIX: &str = "Файл не найден: ";
/// `install_rxn_v4`: manifest.json не читается (дальше — текст исключения).
pub const MSG_MANIFEST_READ_FAILED_PREFIX: &str =
    "Не удалось прочитать manifest: ";
