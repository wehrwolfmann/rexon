//! Миниатюры карточек обоев, у которых движок не дал готового превью.
//!
//! Вкладка обоев рисует готовое превью напрямую; сюда попадает только
//! остаток — обои без картинки (в первую очередь видео из своей папки:
//! у ролика превью нет, пока его кто-нибудь не снимет). Для них кадр
//! берётся из самого файла и кладётся в общий кэш миниатюр рабочего стола
//! (Thumbnail Managing Standard, freedesktop.org):
//! `<кэш>/thumbnails/x-large/<md5 адреса>.png`, с подписями `Thumb::URI`,
//! `Thumb::MTime`, `Thumb::Size` внутри PNG. Запросов к демону нет — всё
//! решается на месте, по файлу.
//!
//! Здесь всё решение: что считать источником, где лежит миниатюра, свежа ли
//! она, как звать `ffmpeg`, как собрать PNG. Объект для QML
//! (`rexon-gui/src/thumbnail_request.rs`) только исполняет: зовёт
//! [`live_lookup`] в главном потоке и [`live_generate`] в рабочем.
//!
//! Договор со страницей (читает `component/ThumbnailImage.qml`): свойства
//! `source`, `resource`, `wpType` — на вход; `state` (`Idle`/`Loading`/
//! `Ready`/`Failed`), `cachePath` (адрес готового PNG) и `error` — на выход.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

use md5::{Digest, Md5};

/// Состояния запроса — порядок тот же, что у перечисления `State` на
/// странице (`W.ThumbnailRequest.Ready` читается оттуда числом).
pub const STATE_IDLE: i32 = 0;
pub const STATE_LOADING: i32 = 1;
pub const STATE_READY: i32 = 2;
pub const STATE_FAILED: i32 = 3;

/// Длинная сторона миниатюры. Карточка просит не больше 512 (`maxSize`).
pub const MAX_EDGE: u32 = 512;

/// Сколько кадров от начала ролика просматривает `ffmpeg`, выбирая самый
/// показательный: первый кадр часто чёрный (затемнение, заставка).
pub const VIDEO_PROBE_FRAMES: u32 = 30;

/// Сколько ждём `ffmpeg` на один файл. Битый или сетевой файл не должен
/// навсегда занять одно из мест очереди миниатюр.
pub const FRAME_TIMEOUT_SECS: u64 = 20;

/// Причины отказа — строки свойства `error`. Латиницей: это поле для
/// журнала и отладки, человеку страница его не показывает.
pub const ERR_NO_SOURCE: &str = "no preview source";
pub const ERR_NOT_FOUND: &str = "source file not found";

/// Что именно превращать в миниатюру.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// Файл-источник (как пришёл со страницы, ещё не абсолютный).
    pub path: String,
    /// `true` — ролик (кадр достаёт `ffmpeg`), `false` — картинка.
    pub is_video: bool,
}

/// Выбор источника по трём свойствам запроса.
///
/// * есть `source` (готовое превью) — миниатюра делается из него;
/// * иначе, если обои — видео или картинка, — из самого файла обоев;
/// * иначе брать не из чего: сцену или веб-страницу в картинку без
///   рисовальщика не превратить.
pub fn resolve_job(source: &str, resource: &str, wp_type: &str) -> Result<Job, &'static str> {
    if !source.is_empty() {
        return Ok(Job { path: source.to_string(), is_video: false });
    }
    if !resource.is_empty() && (wp_type == "video" || wp_type == "image") {
        return Ok(Job { path: resource.to_string(), is_video: wp_type == "video" });
    }
    Err(ERR_NO_SOURCE)
}

/// Корень кэша миниатюр: `WAYWALLEN_THUMB_DIR`, иначе
/// `$XDG_CACHE_HOME/thumbnails`, иначе `~/.cache/thumbnails`. Тот же
/// каталог, куда кладёт миниатюры движок обоев и файловый менеджер, —
/// снятое однажды видно всем.
pub fn thumb_root(thumb_dir: Option<&str>, xdg_cache_home: Option<&str>, home: &str) -> String {
    match thumb_dir {
        Some(v) if !v.is_empty() => v.trim_end_matches('/').to_string(),
        _ => format!("{}/thumbnails", crate::utils::cache_home(xdg_cache_home, home)),
    }
}

/// Адрес файла, под которым миниатюра числится в кэше и в подписи
/// `Thumb::URI`. Без процентного кодирования — так же считает ключ движок
/// обоев, и только при совпадении ключа кэш у нас с ним общий.
pub fn thumb_uri(abs_path: &str) -> String {
    format!("file://{abs_path}")
}

/// Каталог миниатюр размера `x-large` внутри корня кэша.
pub fn cache_dir(root: &str) -> String {
    format!("{}/x-large", root.trim_end_matches('/'))
}

/// Файл миниатюры: `<корень>/x-large/<md5 адреса>.png`.
pub fn cache_file(root: &str, abs_path: &str) -> String {
    let digest = Md5::digest(thumb_uri(abs_path).as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}/{hex}.png", cache_dir(root))
}

/// Вписать размер в квадрат `max_edge`, сохранив пропорции. Меньшее не
/// растягивается. `None` — размер вырожден.
pub fn fit_inside(width: u32, height: u32, max_edge: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 || max_edge == 0 {
        return None;
    }
    if width <= max_edge && height <= max_edge {
        return Some((width, height));
    }
    let (w, h, m) = (u64::from(width), u64::from(height), u64::from(max_edge));
    if width >= height {
        Some((max_edge, (h * m / w).max(1) as u32))
    } else {
        Some(((w * m / h).max(1) as u32, max_edge))
    }
}

/// Годится ли лежащая в кэше миниатюра: она должна быть не старше
/// источника. Сверяется время изменения самих файлов (секунды) — открывать
/// PNG ради подписи `Thumb::MTime` в главном потоке слишком дорого.
pub fn cache_is_fresh(source_mtime: i64, thumb_mtime: i64) -> bool {
    thumb_mtime >= source_mtime
}

/// Команда, достающая один кадр и отдающая его PNG-ом в стандартный вывод.
///
/// Для ролика кадр выбирает фильтр `thumbnail` из первых
/// [`VIDEO_PROBE_FRAMES`] кадров; для картинки (запасной путь, когда формат
/// не читается своими средствами) берётся единственный кадр. Масштаб —
/// вписать в [`MAX_EDGE`], не растягивая.
pub fn frame_argv(path: &str, is_video: bool) -> Vec<String> {
    let scale = format!(
        "scale=w='min({e},iw)':h='min({e},ih)':force_original_aspect_ratio=decrease",
        e = MAX_EDGE
    );
    let filter = if is_video {
        format!("thumbnail=n={VIDEO_PROBE_FRAMES},{scale}")
    } else {
        scale
    };
    [
        crate::wallpaper_engine::FFMPEG_BIN,
        "-v", "error", "-nostdin",
        "-i", path,
        "-map", "0:v:0", "-an", "-sn",
        "-vf", &filter,
        "-frames:v", "1",
        "-f", "image2pipe", "-c:v", "png", "-",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Собрать PNG миниатюры из пикселей RGBA с тремя подписями стандарта.
pub fn encode_thumb_png(
    rgba: &[u8],
    width: u32,
    height: u32,
    uri: &str,
    source_mtime: i64,
    source_size: u64,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || rgba.len() != (width as usize) * (height as usize) * 4 {
        return Err(format!("bad frame: {width}x{height}, {} bytes", rgba.len()));
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // Стандарт называет `tEXt`, но он вмещает только Latin-1: адрес с
        // не-латиницей уходит в `iTXt` (UTF-8 без потерь).
        if uri.is_ascii() {
            encoder.add_text_chunk("Thumb::URI".to_string(), uri.to_string())
        } else {
            encoder.add_itxt_chunk("Thumb::URI".to_string(), uri.to_string())
        }
        .map_err(|e| e.to_string())?;
        encoder
            .add_text_chunk("Thumb::MTime".to_string(), source_mtime.to_string())
            .map_err(|e| e.to_string())?;
        encoder
            .add_text_chunk("Thumb::Size".to_string(), source_size.to_string())
            .map_err(|e| e.to_string())?;
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Декодировать картинку из памяти и ужать до [`MAX_EDGE`].
pub fn decode_and_fit(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let (w, h) = fit_inside(img.width(), img.height(), MAX_EDGE)
        .ok_or_else(|| "empty image".to_string())?;
    let img = if (w, h) == (img.width(), img.height()) {
        img
    } else {
        img.resize_exact(w, h, image::imageops::FilterType::Triangle)
    };
    Ok((img.to_rgba8().into_raw(), w, h))
}

// ── живая часть: диск и ffmpeg ─────────────────────────────────────────────

/// Миниатюры нет — её надо сделать. Всё, что нужно рабочему потоку.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// Абсолютный путь источника; он же ключ, по которому одинаковые
    /// запросы разных карточек сливаются в одну работу.
    pub path: String,
    pub is_video: bool,
    /// Куда писать готовый PNG.
    pub cache_path: String,
    pub source_mtime: i64,
    pub source_size: u64,
}

/// Итог быстрой проверки в главном потоке.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// Миниатюра уже лежит в кэше — путь к PNG.
    Ready(String),
    /// Делать не из чего — причина.
    Failed(String),
    /// Нужна работа в фоне.
    Miss(Pending),
}

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// [`lookup_in`] с корнем кэша из окружения процесса.
pub fn live_lookup(source: &str, resource: &str, wp_type: &str) -> Lookup {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    let root = thumb_root(
        std::env::var("WAYWALLEN_THUMB_DIR").ok().as_deref(),
        std::env::var("XDG_CACHE_HOME").ok().as_deref(),
        &home,
    );
    lookup_in(&root, source, resource, wp_type)
}

/// Быстрая проверка без декодирования: источник есть? миниатюра в кэше
/// есть и не старше него? Только `stat` — годится для главного потока.
pub fn lookup_in(root: &str, source: &str, resource: &str, wp_type: &str) -> Lookup {
    let job = match resolve_job(source, resource, wp_type) {
        Ok(job) => job,
        Err(e) => return Lookup::Failed(e.to_string()),
    };
    let abs = std::path::absolute(&job.path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(job.path);
    let Ok(meta) = std::fs::metadata(&abs) else {
        return Lookup::Failed(ERR_NOT_FOUND.to_string());
    };
    if !meta.is_file() {
        return Lookup::Failed(ERR_NOT_FOUND.to_string());
    }
    let source_mtime = mtime_secs(&meta);
    let cache_path = cache_file(root, &abs);
    if let Ok(thumb) = std::fs::metadata(&cache_path) {
        if thumb.len() > 0 && cache_is_fresh(source_mtime, mtime_secs(&thumb)) {
            return Lookup::Ready(cache_path);
        }
    }
    Lookup::Miss(Pending {
        path: abs,
        is_video: job.is_video,
        cache_path,
        source_mtime,
        source_size: meta.len(),
    })
}

/// Запустить `ffmpeg` и забрать PNG кадра из его вывода, не дольше
/// [`FRAME_TIMEOUT_SECS`].
fn live_ffmpeg_frame(path: &str, is_video: bool) -> Result<Vec<u8>, String> {
    let argv = frame_argv(path, is_video);
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", argv[0]))?;
    // Вывод читают отдельные потоки: иначе полный канал остановит ffmpeg,
    // а мы будем ждать его завершения.
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let mut stderr = child.stderr.take().ok_or("no stderr")?;
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        buf
    });
    let deadline = Instant::now() + Duration::from_secs(FRAME_TIMEOUT_SECS);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let png = out_reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();
    match status {
        None => Err(format!("{}: timeout", argv[0])),
        Some(s) if s.success() && !png.is_empty() => Ok(png),
        Some(_) => {
            let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no frame");
            Err(format!("{}: {}", argv[0], line.trim()))
        }
    }
}

/// Записать файл так, чтобы читатель никогда не увидел половину: во
/// временный рядом (доступ только владельцу) и переименованием на место.
fn live_write_atomic(cache_path: &str, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let dir = Path::new(cache_path).parent().ok_or("no cache dir")?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let nonce = UNIX_EPOCH.elapsed().map_or(0, |d| d.subsec_nanos());
    let tmp = format!("{cache_path}.tmp.{}.{nonce:x}", std::process::id());
    let written = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_data()?;
        std::fs::rename(&tmp, cache_path)
    })();
    written.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{cache_path}: {e}")
    })
}

/// Сделать миниатюру и положить в кэш. Долго (декодирование, `ffmpeg`) —
/// только из рабочего потока. Возвращает путь к PNG.
pub fn live_generate(job: &Pending) -> Result<String, String> {
    // Картинку читаем сами; не вышло (формат, которого у нас нет) — тем же
    // путём, что и ролик.
    let own = if job.is_video {
        None
    } else {
        std::fs::read(&job.path).ok().and_then(|bytes| decode_and_fit(&bytes).ok())
    };
    let (rgba, width, height) = match own {
        Some(frame) => frame,
        None => decode_and_fit(&live_ffmpeg_frame(&job.path, job.is_video)?)?,
    };
    let png = encode_thumb_png(
        &rgba,
        width,
        height,
        &thumb_uri(&job.path),
        job.source_mtime,
        job.source_size,
    )?;
    live_write_atomic(&job.cache_path, &png)?;
    Ok(job.cache_path.clone())
}
