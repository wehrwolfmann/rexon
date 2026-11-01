//! Чистая логика YouTube-обоев — порт `rexon/youtube_wallpaper.py`.
//!
//! Оригинальный `YouTubeWallpaperManager` рендерит видео с YouTube (и ~1800
//! других сайтов через yt-dlp) как обои в двух режимах:
//!   * **stream**   — `yt-dlp --get-url` отдаёт прямой URL потока → mpv/mpvpaper;
//!   * **download** — `yt-dlp -f … -o …` качает в `~/.cache/rexon/youtube/` →
//!     проигрывается локальный файл.
//!
//! Класс смешивает детерминированные решения (валидация/классификация URL,
//! выбор формата по качеству, построение argv yt-dlp для info/stream/download/
//! playlist, пути кеша по хешу URL, разбор stdout yt-dlp — прямой URL, JSON
//! метаданных, прогресс скачивания, список форматов/плейлиста, выбор файлов
//! под вытеснение из кеша) с побочными эффектами (реальные `subprocess`
//! yt-dlp/mpv, скачивание, `os.scandir`/`makedirs`/`unlink`, чтение/запись
//! JSON-кеша, `threading`). Здесь — ТОЛЬКО чистая часть: вход → выход, без
//! файловой системы, сети и процессов.
//!
//! Наблюдения за средой (установлен ли yt-dlp/ffmpeg, содержимое stdout
//! yt-dlp, размеры/mtime файлов кеша, `time.time()`) подаются ПАРАМЕТРАМИ.
//! Реальный запуск yt-dlp/mpv, скачивание и работа с ФС остаются в
//! Python-шиме `youtube_wallpaper.py`, который исполняет то, что решило ядро
//! (argv, пути, формат, набор файлов на удаление).
//!
//! Паритет байт-в-байт с живым Python — `tests/youtube_wallpaper_parity.rs`
//! (эталон снят read-only, реальный yt-dlp/скачивание НЕ запускались; stdout
//! синтетический, HOME/кеш нормализованы в `/HOME`).
//!
//! ⚠️ Багов-крешей в оригинале при порте не обнаружено. Отмеченные квирки
//! сохранены как есть — это осознанное поведение оригинала:
//!   * `get_stream_url` берёт только ПЕРВУЮ строку `--get-url` (видео-URL),
//!     отбрасывая аудио-URL при раздельных дорожках — так в оригинале
//!     (комментарий «Берём первый (видео)»); mpv/mpvpaper это не ломает.
//!   * пустой/пробельный stdout `--get-url` в Python валит `IndexError`,
//!     который гасится `except Exception → None`; здесь честно возвращаем
//!     `None` — наблюдаемый результат тот же.

use serde_json::{Map, Value};

// ============================================================================
// Константы (порт модульных констант youtube_wallpaper.py)
// ============================================================================

/// Лимит кеша видео (МБ) — порт `MAX_CACHE_MB`.
pub const MAX_CACHE_MB: u64 = 2048;
/// Максимальная длительность для скачивания (сек) — порт `MAX_DURATION_SEC`.
pub const MAX_DURATION_SEC: i64 = 3600;
/// Таймаут загрузки (сек) — порт `MAX_DOWNLOAD_TIMEOUT`.
pub const MAX_DOWNLOAD_TIMEOUT: u64 = 600;

/// Свежесть кеша метаданных (сек) — порт `age < 3600` в `get_video_info`.
pub const META_CACHE_FRESH_SEC: f64 = 3600.0;

/// Значение `--socket-timeout` во всех вызовах yt-dlp.
const SOCKET_TIMEOUT: &str = "15";

/// Формат yt-dlp по умолчанию для стриминга — порт `STREAM_FORMAT`
/// (совпадает с `QUALITY_MAP["1080p"]`).
pub const STREAM_FORMAT: &str =
    "bestvideo[height<=1080][ext=mp4]+bestaudio[ext=m4a]/best[height<=1080]";

/// Таблица «качество → формат yt-dlp» — порт `QUALITY_MAP`.
pub const QUALITY_MAP: &[(&str, &str)] = &[
    (
        "4k",
        "bestvideo[height<=2160][ext=mp4]+bestaudio[ext=m4a]/best[height<=2160]",
    ),
    (
        "1440p",
        "bestvideo[height<=1440][ext=mp4]+bestaudio[ext=m4a]/best[height<=1440]",
    ),
    (
        "1080p",
        "bestvideo[height<=1080][ext=mp4]+bestaudio[ext=m4a]/best[height<=1080]",
    ),
    (
        "720p",
        "bestvideo[height<=720][ext=mp4]+bestaudio[ext=m4a]/best[height<=720]",
    ),
    (
        "480p",
        "bestvideo[height<=480][ext=mp4]+bestaudio[ext=m4a]/best[height<=480]",
    ),
    ("best", "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best"),
];

/// Расширения незавершённых файлов, исключаемых из кеша — порт кортежа
/// `(".part", ".temp", ".ytdl")`.
pub const INCOMPLETE_EXTS: &[&str] = &[".part", ".temp", ".ytdl"];

// ============================================================================
// Выбор формата — порт QUALITY_MAP.get(...)
// ============================================================================

/// Поиск формата по ключу качества — порт `QUALITY_MAP.get(quality)`.
pub fn quality_format(quality: &str) -> Option<&'static str> {
    QUALITY_MAP
        .iter()
        .find(|(k, _)| *k == quality)
        .map(|(_, v)| *v)
}

/// Формат для стриминга — порт `QUALITY_MAP.get(quality, STREAM_FORMAT)`.
pub fn stream_format(quality: &str) -> &'static str {
    quality_format(quality).unwrap_or(STREAM_FORMAT)
}

/// Формат для скачивания — порт `QUALITY_MAP.get(quality, QUALITY_MAP["1080p"])`
/// (неизвестное качество → 1080p, как в оригинале).
pub fn download_format(quality: &str) -> &'static str {
    quality_format(quality).unwrap_or_else(|| quality_format("1080p").unwrap())
}

// ============================================================================
// Пути кеша — порт _CACHE_DIR / _META_DIR / шаблон вывода / meta_path
// ============================================================================

/// Каталог кеша видео — порт `_CACHE_DIR`.
/// `xdg_cache_home` = `$XDG_CACHE_HOME` (Some — задан, None — не задан → `~/.cache`).
pub fn cache_dir(xdg_cache_home: Option<&str>, home: &str) -> String {
    let base = match xdg_cache_home {
        Some(x) => x.to_string(),
        None => format!("{home}/.cache"),
    };
    format!("{base}/rexon/youtube")
}

/// Каталог метаданных — порт `_META_DIR`.
pub fn meta_dir(cache_dir: &str) -> String {
    format!("{cache_dir}/meta")
}

/// Путь JSON-кеша метаданных по хешу — порт `os.path.join(_META_DIR, f"{h}.json")`.
pub fn meta_path(meta_dir: &str, vid_hash: &str) -> String {
    format!("{meta_dir}/{vid_hash}.json")
}

/// Шаблон `-o` для yt-dlp — порт `os.path.join(_CACHE_DIR, f"{h}.%(ext)s")`.
pub fn output_template(cache_dir: &str, vid_hash: &str) -> String {
    format!("{cache_dir}/{vid_hash}.%(ext)s")
}

/// Стабильный хеш URL для имени файла кеша — порт `_video_id_hash`:
/// `sha256(url.utf8).hexdigest()[:16]`. URL передаётся уже обрезанным (`strip`)
/// — как это делают вызывающие в оригинале.
pub fn video_id_hash(url: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(url.as_bytes());
    let mut hex = String::with_capacity(16);
    for b in digest {
        if hex.len() >= 16 {
            break;
        }
        hex.push_str(&format!("{b:02x}"));
    }
    hex.truncate(16);
    hex
}

/// Свежий ли кеш метаданных — порт `age < 3600`.
pub fn meta_cache_fresh(age_sec: f64) -> bool {
    age_sec < META_CACHE_FRESH_SEC
}

// ============================================================================
// Классификация / валидация URL — порт is_supported_url / is_youtube_url / …
// ============================================================================

/// Быстрая проверка «поддерживается ли URL» — порт `is_supported_url`:
/// после `strip` должен начинаться с `http://`/`https://`; пустой → false.
pub fn is_supported_url(url: &str) -> bool {
    let u = url.trim();
    if u.is_empty() {
        return false;
    }
    u.starts_with("http://") || u.starts_with("https://")
}

/// YouTube ли это URL — порт `is_youtube_url` (регекс `_YOUTUBE_RE.search`,
/// IGNORECASE). Регекс портирован через подстроки с сохранением `search`- и
/// `watch\?.*v=`-семантики; префиксы `https?://`/`www.|m.` необязательны и на
/// результат `search` не влияют, поэтому в проверке не участвуют.
pub fn is_youtube_url(url: &str) -> bool {
    let l = url.to_lowercase();
    if l.contains("youtu.be/") {
        return true;
    }
    for pat in [
        "youtube.com/shorts/",
        "youtube.com/live/",
        "youtube.com/embed/",
        "youtube.com/v/",
        "youtube.com/playlist?",
    ] {
        if l.contains(pat) {
            return true;
        }
    }
    // `youtube.com/watch?` … `v=` на той же строке (music.youtube.com — частный
    // случай той же подстроки). Порт `watch\?.*v=` (`.` не матчит `\n`).
    watch_then_v(&l)
}

/// `youtube.com/watch?` с последующим `v=` в пределах той же строки.
fn watch_then_v(lower: &str) -> bool {
    let needle = "youtube.com/watch?";
    let mut from = 0;
    while let Some(rel) = lower[from..].find(needle) {
        let after = from + rel + needle.len();
        // конец текущей строки (`.` в регексе не матчит `\n`)
        let line_end = lower[after..]
            .find('\n')
            .map(|n| after + n)
            .unwrap_or(lower.len());
        if lower[after..line_end].contains("v=") {
            return true;
        }
        from = after;
    }
    false
}

/// Плейлист ли это YouTube — порт `is_playlist_url` (`"playlist?list=" in url`).
pub fn is_playlist_url(url: &str) -> bool {
    url.contains("playlist?list=")
}

// ============================================================================
// argv yt-dlp — порт cmd в get_video_info / get_stream_url / download / playlist
// ============================================================================

/// argv `yt-dlp --dump-json` (метаданные) — порт cmd из `get_video_info`.
/// `url` передаётся уже обрезанным (`strip`), как в оригинале.
pub fn info_argv(url: &str) -> Vec<String> {
    vec![
        "yt-dlp".into(),
        "--no-warnings".into(),
        "--no-playlist".into(),
        "--dump-json".into(),
        "--no-download".into(),
        "--socket-timeout".into(),
        SOCKET_TIMEOUT.into(),
        url.into(),
    ]
}

/// argv `yt-dlp --get-url` (прямой URL потока) — порт cmd из `get_stream_url`.
/// `fmt` — результат [`stream_format`]; `url` уже обрезан.
pub fn stream_argv(url: &str, fmt: &str) -> Vec<String> {
    vec![
        "yt-dlp".into(),
        "--no-warnings".into(),
        "--no-playlist".into(),
        "--get-url".into(),
        "-f".into(),
        fmt.into(),
        "--socket-timeout".into(),
        SOCKET_TIMEOUT.into(),
        url.into(),
    ]
}

/// argv `yt-dlp` для скачивания в кеш — порт cmd из `download`.
/// `fmt` — результат [`download_format`]; `output_tmpl` — [`output_template`];
/// `url` уже обрезан.
pub fn download_argv(url: &str, fmt: &str, output_tmpl: &str) -> Vec<String> {
    vec![
        "yt-dlp".into(),
        "--no-warnings".into(),
        "--no-playlist".into(),
        "-f".into(),
        fmt.into(),
        "--merge-output-format".into(),
        "mp4".into(),
        "--socket-timeout".into(),
        SOCKET_TIMEOUT.into(),
        "--no-part".into(),
        "--newline".into(),
        "-o".into(),
        output_tmpl.into(),
        url.into(),
    ]
}

/// argv `yt-dlp --flat-playlist --dump-json` — порт cmd из `get_playlist_entries`.
/// `url` уже обрезан.
pub fn playlist_argv(url: &str, max_entries: u64) -> Vec<String> {
    vec![
        "yt-dlp".into(),
        "--no-warnings".into(),
        "--flat-playlist".into(),
        "--dump-json".into(),
        "--playlist-end".into(),
        max_entries.to_string(),
        "--socket-timeout".into(),
        SOCKET_TIMEOUT.into(),
        url.into(),
    ]
}

// ── argv установки / обновления / версии yt-dlp ────────────────────────────
//
// Порт «сырых» fallback-argv из install_ytdlp / update_ytdlp / ytdlp_version.
// Именованный privileged-вариант (`privileged_pkg_cmd`) решает system-слой —
// это наблюдение/side-effect исполнителя; здесь только детерминированные argv.

/// Имя пакета yt-dlp — одинаково во всех семействах.
pub const YTDLP_PACKAGE: &str = "yt-dlp";

/// Кандидаты бинаря pip в порядке приоритета.
///
/// На Debian/Ubuntu пакет `python3-pip` кладёт `pip3`, а `pip` появляется
/// только если поставлен `python-is-python3`; на Arch/Fedora/openSUSE есть
/// оба. Поэтому сначала `pip3`.
pub const PIP_BINARIES: [&str; 2] = ["pip3", "pip"];

/// argv установки yt-dlp через pip.
///
/// `--break-system-packages` НЕ ставится по умолчанию: он нужен только там,
/// где окружение помечено PEP 668 (Arch, Debian/Ubuntu), а на Fedora и
/// openSUSE такой пометки нет и лишний флаг только шумит. Правильный порядок
/// — сначала без флага, и лишь если pip отказал по PEP 668
/// ([`pip_break_system_needed`]), повторить с ним.
pub fn pip_argv(pip_bin: &str, upgrade: bool, break_system: bool) -> Vec<String> {
    let mut v = vec![pip_bin.into(), "install".into(), "--user".into()];
    if break_system {
        v.push("--break-system-packages".into());
    }
    if upgrade {
        v.push("--upgrade".into());
    }
    v.push(YTDLP_PACKAGE.into());
    v
}

/// Отказ pip из-за PEP 668 («externally-managed-environment») — значит,
/// стоит повторить с `--break-system-packages`.
pub fn pip_break_system_needed(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("externally-managed-environment") || s.contains("externally managed")
}

/// Итог установки через пакетный менеджер дистрибутива.
pub fn msg_installed_via(pkg_manager: &str) -> String {
    crate::i18n::trf("yt-dlp installed with %1", &[&pkg_manager])
}

/// Итог установки через pip.
pub fn msg_installed_via_pip(pip_bin: &str) -> String {
    crate::i18n::trf("yt-dlp installed with %1 (--user)", &[&pip_bin])
}

/// Итог обновления через pip.
pub fn msg_updated_via_pip(pip_bin: &str) -> String {
    crate::i18n::trf("yt-dlp updated with %1", &[&pip_bin])
}

/// Полный провал установки: что человеку делать дальше.
pub fn msg_install_failed() -> String {
    crate::i18n::tr("Could not install yt-dlp automatically. Install it manually: pip install yt-dlp")
}

/// yt-dlp нет — обновлять нечего.
pub fn msg_not_installed() -> String {
    crate::i18n::tr("yt-dlp is not installed")
}

// ── живое скачивание в кеш ─────────────────────────────────────────────────

/// Скачанный файл ролика среди имён каталога кеша — порт `_find_cached`.
/// Порядок имён сортированный: у Python он был порядком файловой системы.
pub fn cached_file_name(names: &[String], vid_hash: &str) -> Option<String> {
    let mut hits: Vec<&String> = names.iter().filter(|n| is_cached_file(n, vid_hash)).collect();
    hits.sort();
    hits.first().map(|n| (*n).clone())
}

/// Файлы каталога кеша: (путь, имя, mtime, размер).
fn cache_files_live(dir: &str) -> Vec<(String, String, f64, u64)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read.filter_map(Result::ok)
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            Some((
                e.path().to_string_lossy().into_owned(),
                e.file_name().to_string_lossy().into_owned(),
                mtime,
                meta.len(),
            ))
        })
        .collect()
}

fn find_cached_live(dir: &str, vid_hash: &str) -> Option<String> {
    let names: Vec<String> = cache_files_live(dir).into_iter().map(|f| f.1).collect();
    cached_file_name(&names, vid_hash).map(|name| format!("{dir}/{name}"))
}

/// Метаданные ролика — порт `get_video_info`: свежий кеш карточки, иначе
/// `yt-dlp --dump-json` и запись карточки в кеш.
fn video_info_live(url: &str, cache: &str, vid_hash: &str) -> Option<Value> {
    let meta_dir = meta_dir(cache);
    let meta = meta_path(&meta_dir, vid_hash);
    if let Ok(m) = std::fs::metadata(&meta) {
        let age = m
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|d| d.as_secs_f64())
            .unwrap_or(f64::MAX);
        if meta_cache_fresh(age) {
            if let Some(v) = std::fs::read_to_string(&meta)
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            {
                return Some(v);
            }
        }
    }
    let (rc, stdout, stderr) = run_live(&info_argv(url), Some(30))?;
    if rc != 0 {
        crate::log_warn!("yt-dlp", "сведения о ролике не получены: {}", stderr.trim());
        return None;
    }
    let data: Value = serde_json::from_str(&stdout).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let info = build_video_info(&data, url, vid_hash, now);
    if std::fs::create_dir_all(&meta_dir).is_ok() {
        if let Ok(text) = serde_json::to_string_pretty(&info) {
            let _ = crate::utils::atomic_write(&meta, &text);
        }
    }
    Some(info)
}

/// Скачать ролик в кеш — порт `YouTubeWallpaperManager.download`. Путь к
/// файлу либо `None` (причина — в журнале). Уже скачанный берётся из кеша;
/// слишком длинный ролик и прямой эфир не качаются; перед загрузкой кеш
/// ужимается в предел.
pub fn download_live(url: &str, quality: &str) -> Option<String> {
    crate::utils::which("yt-dlp", None)?;
    let url = url.trim();
    let vid_hash = video_id_hash(url);
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let xdg = std::env::var("XDG_CACHE_HOME").ok().filter(|v| !v.is_empty());
    let cache = cache_dir(xdg.as_deref(), &home);
    std::fs::create_dir_all(&cache).ok()?;

    if let Some(path) = find_cached_live(&cache, &vid_hash) {
        crate::log_info!("yt-dlp", "ролик найден в кеше: {path}");
        return Some(path);
    }
    if let Some(info) = video_info_live(url, &cache, &vid_hash) {
        let duration = info
            .get("duration")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let live = info.get("is_live").and_then(Value::as_bool).unwrap_or(false);
        if let Some(reason) = download_block_reason(duration as i64, live) {
            crate::log_info!("yt-dlp", "ролик не скачивается: {reason}");
            return None;
        }
    }
    // Освободить место в кеше.
    let files = cache_files_live(&cache);
    let sizes: Vec<u64> = files.iter().map(|f| f.3).collect();
    let triples: Vec<(String, f64, u64)> =
        files.iter().map(|f| (f.0.clone(), f.2, f.3)).collect();
    for path in select_evictions(dir_size_mb(&sizes), &triples) {
        let _ = std::fs::remove_file(path);
    }

    let fmt = download_format(quality);
    let argv = download_argv(url, fmt, &output_template(&cache, &vid_hash));
    let (rc, _, stderr) = run_live(&argv, Some(MAX_DOWNLOAD_TIMEOUT))?;
    if rc != 0 {
        crate::log_warn!("yt-dlp", "загрузка не удалась (код {rc}): {}", stderr.trim());
        return None;
    }
    find_cached_live(&cache, &vid_hash)
}

/// Обновление прошло.
pub fn msg_updated() -> String {
    crate::i18n::tr("yt-dlp updated")
}

/// Обновить не удалось ни самим yt-dlp, ни через pip.
pub fn msg_update_failed() -> String {
    crate::i18n::tr("Could not update yt-dlp")
}

/// Итог запуска команды: (код возврата, stdout, stderr); `None` — команда не
/// дошла до кода (нет программы, вышло время).
pub type RunResult = Option<(i32, String, String)>;

/// Запустить pip; при отказе PEP 668 повторить с `--break-system-packages` —
/// порт `YouTubeWallpaperManager._pip_run`. Запуск — снаружи (`run`), чтобы
/// порядок попыток сверялся без единого процесса.
///
/// Флаг не ставится заранее: он нужен только там, где окружение помечено
/// PEP 668 (Arch, Debian/Ubuntu); решаем по факту отказа.
pub fn pip_run_with(run: &mut dyn FnMut(&[String]) -> RunResult, pip_bin: &str, upgrade: bool) -> bool {
    for break_system in [false, true] {
        let argv = pip_argv(pip_bin, upgrade, break_system);
        let Some((rc, stdout, stderr)) = run(&argv) else {
            return false;
        };
        if rc == 0 {
            return true;
        }
        if break_system {
            return false;
        }
        let said = if stderr.is_empty() { stdout } else { stderr };
        if !pip_break_system_needed(&said) {
            return false;
        }
    }
    false
}

/// Живой запуск команды со сроком.
fn run_live(argv: &[String], timeout_secs: Option<u64>) -> RunResult {
    let report = crate::utils::run(
        &crate::utils::RunSpec {
            argv: argv.to_vec(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs,
            ..Default::default()
        },
        None,
    );
    let rc = report.returncode?;
    Some((
        rc,
        crate::utils::decode_text(&report.stdout),
        crate::utils::decode_text(&report.stderr),
    ))
}

/// Бинарь pip в системе — порядок предпочтения [`PIP_BINARIES`].
fn pip_binary_live() -> Option<String> {
    PIP_BINARIES
        .iter()
        .find(|name| crate::utils::which(name, None).is_some())
        .map(|name| (*name).to_string())
}

/// Установить yt-dlp — порт `YouTubeWallpaperManager.install_ytdlp`.
///
/// Сначала общий механизм установки пакетов (`system_plan::install_plan` —
/// тот же, что у установки рабочих столов: знает все семейства и сам
/// выбирает polkit-помощник либо сырой pkexec), затем pip. Живой запуск:
/// спрашивает пароль через polkit.
pub fn install_ytdlp_live() -> (bool, String) {
    let sys = crate::system_exec::live();
    let family = sys.family();
    if family != crate::system::PkgFamily::Unknown {
        let packages = vec![YTDLP_PACKAGE.to_string()];
        if let Ok(plan) = crate::system_plan::install_plan(
            family,
            &packages,
            false,
            None,
            sys.pkg_helper_available(),
        ) {
            if plan.early_result.is_none() {
                let cmd = plan.steps.iter().find_map(|step| match step {
                    crate::system_plan::PlanStep::Run { cmd, .. } => Some(cmd.clone()),
                    _ => None,
                });
                if let Some(cmd) = cmd {
                    // Сеанс прав окна (один пароль); его нет — прежняя команда
                    // плана. Не вышло ни так, ни так — дальше pip от
                    // пользователя (не сырой pkexec).
                    let op = crate::privilege::op::PrivOp::PkgInstall {
                        family,
                        packages: packages.clone(),
                        source: crate::privilege::op::PkgSource::YtDlp,
                    };
                    let session = if cmd.argv.first().map(String::as_str) == Some("pkexec") {
                        crate::privilege::run::run_live(&op, &[], None, &mut |_| {})
                    } else {
                        None
                    };
                    let rc = match session {
                        Some(out) => out.returncode,
                        None => run_live(&cmd.argv, cmd.timeout_secs).map(|(rc, _, _)| rc),
                    };
                    if rc == Some(0) {
                        return (
                            true,
                            msg_installed_via(&crate::system_plan::pkg_manager_name(family, None)),
                        );
                    }
                }
            }
        }
    }
    if let Some(pip) = pip_binary_live() {
        if pip_run_with(&mut |argv| run_live(argv, Some(120)), &pip, false) {
            return (true, msg_installed_via_pip(&pip));
        }
    }
    (false, msg_install_failed())
}

/// Обновить yt-dlp — порт `YouTubeWallpaperManager.update_ytdlp`.
///
/// ⭐ Починка: при удаче Python показывал человеку сырой английский вывод
/// `yt-dlp --update` («Updated yt-dlp to stable@…»). Теперь ответ — русская
/// фраза, а вывод остаётся журналу.
pub fn update_ytdlp_live() -> (bool, String) {
    if crate::utils::which("yt-dlp", None).is_none() {
        return (false, msg_not_installed());
    }
    if let Some((0, stdout, _)) = run_live(&update_self_argv(), Some(60)) {
        let said = stdout.trim();
        if !said.is_empty() {
            crate::log_info!("yt-dlp", "обновление: {said}");
        }
        return (true, msg_updated());
    }
    if let Some(pip) = pip_binary_live() {
        if pip_run_with(&mut |argv| run_live(argv, Some(120)), &pip, true) {
            return (true, msg_updated_via_pip(&pip));
        }
    }
    (false, msg_update_failed())
}

// ── legacy-argv ────────────────────────────────────────────────────────────
//
// Три функции ниже — «свой перебор pacman/apt/dnf», из-за которого на
// openSUSE (приоритет 3 в поддержке) кнопка отвечала «Неизвестный пакетный
// менеджер». Шим их больше НЕ зовёт: установка идёт общим механизмом
// `system.PackageManager.install` (правило #12), который знает все семейства.
// Оставлены только потому, что на них ещё смотрят экспорты моста
// (`rust/rexon-py/src/lib.rs`); удалять их надо вместе с экспортами.

/// `pkexec pacman -S --noconfirm yt-dlp` (Arch/CachyOS).
pub fn install_pacman_argv() -> Vec<String> {
    vec![
        "pkexec".into(),
        "pacman".into(),
        "-S".into(),
        "--noconfirm".into(),
        "yt-dlp".into(),
    ]
}

/// `pkexec apt install -y yt-dlp` (Ubuntu/Debian).
pub fn install_apt_argv() -> Vec<String> {
    vec![
        "pkexec".into(),
        "apt".into(),
        "install".into(),
        "-y".into(),
        "yt-dlp".into(),
    ]
}

/// `pkexec dnf install -y yt-dlp` (Fedora).
pub fn install_dnf_argv() -> Vec<String> {
    vec![
        "pkexec".into(),
        "dnf".into(),
        "install".into(),
        "-y".into(),
        "yt-dlp".into(),
    ]
}

/// `pip install --user --break-system-packages yt-dlp` (fallback).
pub fn install_pip_argv() -> Vec<String> {
    vec![
        "pip".into(),
        "install".into(),
        "--user".into(),
        "--break-system-packages".into(),
        "yt-dlp".into(),
    ]
}

/// `yt-dlp --update` — самообновление.
pub fn update_self_argv() -> Vec<String> {
    vec!["yt-dlp".into(), "--update".into()]
}

/// `pip install --user --break-system-packages --upgrade yt-dlp` (fallback).
pub fn update_pip_argv() -> Vec<String> {
    vec![
        "pip".into(),
        "install".into(),
        "--user".into(),
        "--break-system-packages".into(),
        "--upgrade".into(),
        "yt-dlp".into(),
    ]
}

/// `yt-dlp --version`.
pub fn version_argv() -> Vec<String> {
    vec!["yt-dlp".into(), "--version".into()]
}

// ============================================================================
// Разбор stdout yt-dlp
// ============================================================================

/// Разбор stdout `yt-dlp --get-url` — порт хвоста `get_stream_url`:
/// берём ПЕРВУЮ строку обрезанного вывода, обрезаем её и возвращаем, если
/// начинается с `http`. Пустой/непохожий на URL вывод → None.
pub fn parse_stream_url(stdout: &str) -> Option<String> {
    let first = stdout.trim().lines().next()?;
    let stream = first.trim();
    if stream.starts_with("http") {
        Some(stream.to_string())
    } else {
        None
    }
}

/// Разбор строки прогресса — порт `_parse_progress`:
///   * `[download]<пробелы><число>%` → число (float);
///   * строка содержит `[Merger]`/`[ExtractAudio]` → 99.0;
///   * иначе None.
///
/// Нечисловой захват (`float(...)` → ValueError) в Python гасится → падает в
/// проверку мержинга; здесь при неудачном парсе делаем то же.
pub fn parse_progress(line: &str) -> Option<f64> {
    if let Some(pct) = download_pct(line) {
        return Some(pct);
    }
    if line.contains("[Merger]") || line.contains("[ExtractAudio]") {
        return Some(99.0);
    }
    None
}

/// Реализация регекса `\[download\]\s+([\d.]+)%` c семантикой `re.search`.
fn download_pct(line: &str) -> Option<f64> {
    let needle = "[download]";
    let chars: Vec<char> = line.chars().collect();
    let mut from = 0;
    while let Some(rel) = line[from..].find(needle) {
        let byte_pos = from + rel;
        let char_pos = line[..byte_pos].chars().count();
        let mut i = char_pos + needle.chars().count();
        // \s+ — минимум один пробельный символ
        let ws_start = i;
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i > ws_start {
            // [\d.]+ — минимум один символ из цифр/точки
            let num_start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i > num_start && i < chars.len() && chars[i] == '%' {
                let num: String = chars[num_start..i].iter().collect();
                if let Ok(v) = num.parse::<f64>() {
                    return Some(v);
                }
                // ValueError в Python → падаем дальше (return None ниже)
            }
        }
        from = byte_pos + needle.len();
    }
    None
}

// ============================================================================
// Разбор JSON yt-dlp: список форматов / карточка видео / плейлист
// ============================================================================

/// Истинность значения по правилам Python `bool(x)`
/// (для портов выражений вида `A or B`, `if not h`).
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `dict.get(key, default)` — значение если ключ присутствует (даже `null`),
/// иначе `default`.
fn get_or(obj: &Value, key: &str, default: Value) -> Value {
    obj.get(key).cloned().unwrap_or(default)
}

/// Извлечь список видео-форматов — порт `_extract_format_list`.
/// Пропускаем без высоты / `<240` / повторную высоту; сортируем по height ↓.
pub fn extract_format_list(data: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let empty = Vec::new();
    let formats = data
        .get("formats")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    for fmt in formats {
        let h = fmt.get("height").cloned().unwrap_or(Value::Null);
        // `if not h or h < 240`
        if !truthy(&h) {
            continue;
        }
        let hi = match h.as_i64() {
            Some(n) => n,
            None => continue, // не число → как «нет высоты»
        };
        if hi < 240 {
            continue;
        }
        let res_key = format!("{hi}p");
        if seen.contains(&res_key) {
            continue;
        }
        seen.push(res_key.clone());

        // filesize: `fmt.get("filesize") or fmt.get("filesize_approx", 0)`
        let fs = fmt.get("filesize").cloned().unwrap_or(Value::Null);
        let filesize = if truthy(&fs) {
            fs
        } else {
            get_or(fmt, "filesize_approx", Value::from(0))
        };

        let mut m = Map::new();
        m.insert("resolution".into(), Value::from(res_key));
        m.insert("height".into(), Value::from(hi));
        m.insert("fps".into(), get_or(fmt, "fps", Value::from(30)));
        m.insert("vcodec".into(), get_or(fmt, "vcodec", Value::from("")));
        m.insert("filesize".into(), filesize);
        out.push(Value::Object(m));
    }
    // сортировка по height ↓ (stable, как list.sort в Python)
    out.sort_by(|a, b| {
        let hb = b["height"].as_i64().unwrap_or(0);
        let ha = a["height"].as_i64().unwrap_or(0);
        hb.cmp(&ha)
    });
    out
}

/// Собрать кешируемую карточку видео из JSON yt-dlp — порт формирования `info`
/// в `get_video_info`. `vid_hash` — [`video_id_hash`], `fetched_at` —
/// `time.time()` (side-effect исполнителя), `url` — исходный (fallback
/// `webpage_url`).
pub fn build_video_info(data: &Value, url: &str, vid_hash: &str, fetched_at: f64) -> Value {
    // description: (data.get("description","") or "")[:500]
    let desc_raw = get_or(data, "description", Value::from(""));
    let desc = if desc_raw.is_string() && truthy(&desc_raw) {
        desc_raw.as_str().unwrap().to_string()
    } else {
        String::new()
    };
    let description = truncate_chars(&desc, 500);

    let mut m = Map::new();
    m.insert("id".into(), get_or(data, "id", Value::from("")));
    m.insert(
        "title".into(),
        get_or(data, "title", Value::from("Без названия")),
    );
    m.insert("duration".into(), get_or(data, "duration", Value::from(0)));
    m.insert("thumbnail".into(), get_or(data, "thumbnail", Value::from("")));
    m.insert("uploader".into(), get_or(data, "uploader", Value::from("")));
    m.insert(
        "view_count".into(),
        get_or(data, "view_count", Value::from(0)),
    );
    m.insert(
        "webpage_url".into(),
        get_or(data, "webpage_url", Value::from(url)),
    );
    m.insert("is_live".into(), get_or(data, "is_live", Value::from(false)));
    m.insert("description".into(), Value::from(description));
    m.insert("formats".into(), Value::from(extract_format_list(data)));
    m.insert("_hash".into(), Value::from(vid_hash));
    m.insert("_fetched_at".into(), Value::from(fetched_at));
    Value::Object(m)
}

/// Разобрать stdout `yt-dlp --flat-playlist --dump-json` — порт разбора в
/// `get_playlist_entries`: по строке на видео, невалидный JSON пропускаем.
pub fn parse_playlist(stdout: &str) -> Vec<Value> {
    let mut entries = Vec::new();
    for line in stdout.trim().lines() {
        let d: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // JSONDecodeError → continue
        };
        // url: d.get("url", d.get("webpage_url", ""))
        let url = if d.get("url").is_some() {
            d["url"].clone()
        } else {
            get_or(&d, "webpage_url", Value::from(""))
        };
        // thumbnail: d.get("thumbnails",[{}])[-1].get("url","") if d.get("thumbnails") else ""
        let thumbnail = match d.get("thumbnails") {
            Some(t) if truthy(t) => t
                .as_array()
                .and_then(|a| a.last())
                .map(|last| get_or(last, "url", Value::from("")))
                .unwrap_or_else(|| Value::from("")),
            _ => Value::from(""),
        };

        let mut m = Map::new();
        m.insert("id".into(), get_or(&d, "id", Value::from("")));
        m.insert(
            "title".into(),
            get_or(&d, "title", Value::from("Без названия")),
        );
        m.insert("duration".into(), get_or(&d, "duration", Value::from(0)));
        m.insert("url".into(), url);
        m.insert("thumbnail".into(), thumbnail);
        entries.push(Value::Object(m));
    }
    entries
}

/// Человекочитаемая строка о видео — порт `format_info_text`.
pub fn format_info_text(info: &Value) -> String {
    // dur = info.get("duration", 0) or 0
    let dur_val = get_or(info, "duration", Value::from(0));
    let dur = if truthy(&dur_val) {
        dur_val.as_f64().unwrap_or(0.0)
    } else {
        0.0
    };

    let dur_str = if dur > 0.0 {
        let total = dur.trunc() as i64; // int(dur)
        let (mins, secs) = (total / 60, total % 60);
        let (hrs, mins) = (mins / 60, mins % 60);
        if hrs != 0 {
            format!("{hrs}:{mins:02}:{secs:02}")
        } else {
            format!("{mins}:{secs:02}")
        }
    } else if truthy(&get_or(info, "is_live", Value::Null)) {
        "♾️ live".to_string()
    } else {
        "?".to_string()
    };

    // title = info.get("title", ""); усечение до 57 + "…" при len > 60
    let title_s = value_as_str(&get_or(info, "title", Value::from("")));
    let title = if title_s.chars().count() > 60 {
        format!("{}…", truncate_chars(&title_s, 57))
    } else {
        title_s
    };

    let uploader = value_as_str(&get_or(info, "uploader", Value::from("")));

    // max_res = formats[0]["resolution"] if formats else "?"
    let max_res = info
        .get("formats")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .and_then(|a| a[0].get("resolution"))
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string();

    format!("{title}  •  {uploader}  •  {dur_str}  •  до {max_res}")
}

/// Строковое представление значения для строковых полей (`title`/`uploader`).
fn value_as_str(v: &Value) -> String {
    match v.as_str() {
        Some(s) => s.to_string(),
        None => String::new(),
    }
}

/// Усечение строки до `n` СИМВОЛОВ (Python-слайс `s[:n]` работает по кодпойнтам).
fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

// ============================================================================
// Кеш: определение файла / вытеснение / precheck скачивания
// ============================================================================

/// Наш ли это скачанный файл — порт предиката `_find_cached`:
/// имя начинается с хеша и не оканчивается на `.part`/`.temp`/`.ytdl`.
pub fn is_cached_file(name: &str, vid_hash: &str) -> bool {
    if !name.starts_with(vid_hash) {
        return false;
    }
    !INCOMPLETE_EXTS.iter().any(|ext| name.ends_with(ext))
}

/// Файлы под вытеснение из кеша — порт `_enforce_cache_limit`.
/// Вход — суммарный размер кеша (МБ) и список `(path, mtime, size_bytes)`;
/// выход — пути в порядке удаления. Если кеш в пределах лимита — пусто.
/// Удаляем старейшие (по mtime ↑), пока не уложимся в 80% лимита.
pub fn select_evictions(size_mb: f64, files: &[(String, f64, u64)]) -> Vec<String> {
    if size_mb <= MAX_CACHE_MB as f64 {
        return Vec::new();
    }
    let mut sorted: Vec<&(String, f64, u64)> = files.iter().collect();
    // старые первыми (stable сортировка по mtime, как в Python)
    sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let target = MAX_CACHE_MB as f64 * 0.8;
    let mut size = size_mb;
    let mut deleted = Vec::new();
    for (path, _, fsize) in sorted {
        if size <= target {
            break;
        }
        deleted.push(path.clone());
        size -= *fsize as f64 / (1024.0 * 1024.0);
    }
    deleted
}

/// Размер набора файлов в МБ — порт `_dir_size_mb` (сумма байт / 1 МиБ).
pub fn dir_size_mb(sizes: &[u64]) -> f64 {
    let total: u64 = sizes.iter().sum();
    total as f64 / (1024.0 * 1024.0)
}

/// Причина отказа скачивать (статус для progress_cb) — порт проверок в
/// `download`: сначала длительность, затем live. `None` — можно качать.
/// Длина проверяется ПЕРВОЙ (как в оригинале).
pub fn download_block_reason(duration: i64, is_live: bool) -> Option<String> {
    if duration > MAX_DURATION_SEC {
        return Some(format!("Видео > {} мин", MAX_DURATION_SEC / 60));
    }
    if is_live {
        return Some("Live — используйте стриминг".to_string());
    }
    None
}
