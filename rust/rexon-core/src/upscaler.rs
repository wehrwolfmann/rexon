//! Апскейл обоев низкого разрешения — порт `rexon/extensions/upscaler.py`
//! (класс `WallpaperUpscaler`).
//!
//! Чистая логика (детерминированные вход→выход) вынесена в ядро:
//!   * `suggest_scale` — рекомендуемый множитель по разрешению (пороги
//!     480/800/1024/1920) и `needs_upscale`/`MIN_DESKTOP_WIDTH`;
//!   * разбор вывода `identify` (`%w %h` → (w, h), ошибка → (0, 0));
//!   * построение argv КАЖДОЙ стратегии байт-в-байт: Real-ESRGAN (одиночный
//!     `-s min(scale,4)` и двухпроходный x4→x2 при scale>4), waifu2x
//!     (`-s min(scale,4) -n 2`), center+blur (`convert (+clone …) +swap …`,
//!     цель 1920×1080), ImageMagick Lanczos (`w*scale × h*scale`);
//!   * ВЫБОР стратегии: порядок автопробы `realesrgan → waifu2x → center`,
//!     резолв бинаря Real-ESRGAN (system PATH → локальная копия → скачивание),
//!     дефолтный dest (`{base}_upscaled{ext}`), ранние выходы (identify
//!     упал / scale ≤ 1);
//!   * `process_wallpapers_in_dir` — обход папки, фильтр расширений, апскейл
//!     маленьких in-place.
//!
//! СХЕМА: апскейл интерактивен — ход зависит от результатов subprocess
//! (identify/realesrgan/waifu2x/convert) и от файлов, появившихся после них.
//! Поэтому, как в `importers.rs`, вместо статического плана — трейт [`UpIo`]:
//! КАЖДЫЙ побочный вызов оригинала (subprocess.run / shutil.which /
//! os.path.exists|isfile / os.access / os.remove / os.listdir / os.replace /
//! скачивание Real-ESRGAN) — один метод трейта, в том же порядке. Эталон
//! паритета — журнал вызовов живого Python read-only (subprocess перехвачен
//! прокси, реальный апскейл/identify/скачивание НЕ запускались;
//! `tests/golden/upscaler_cases.json`); Rust обязан сделать те же вызовы в том
//! же порядке (argv/timeout байт-в-байт) и прийти к тому же результату.
//!
//! Квирки оригинала СОХРАНЕНЫ:
//!   * `upscale` проверяет только `w == 0` (не `h`) — при identify «100 0»
//!     апскейл всё равно идёт (сохранено);
//!   * `process_wallpapers_in_dir` имеет параметр `auto=True`, который нигде
//!     не используется, — в Rust опущен как мёртвый;
//!   * при `method="auto"` `identify` для одного файла вызывается многократно
//!     (рекурсивный `upscale` перечитывает размер на каждый метод) — сохранено;
//!   * center+blur игнорирует `scale` (всегда цель 1920×1080) — сохранено;
//!   * ПОЧИНЕНО (порт, правило «баги при переводе чиним»): размер картинки
//!     ядро сначала спрашивает у исполнителя ([`UpIo::image_size`]) и только
//!     потом зовёт `identify`. Оригинал так и делает — Pillow-первым, потому
//!     что на SteamOS ImageMagick нет; без этого хука Rust-путь на такой
//!     машине всегда получал (0, 0) и апскейл молча не выполнялся. Реплей
//!     журнала в parity-тестах не задет: `image_size` по умолчанию `None`.
//!   * баг оригинала: `_ensure_realesrgan` в ветке скачивания использует
//!     `zipfile`/`_safe_zip_extract`, не импортированные в модуле → любое
//!     автоскачивание падает `NameError`, перехватывается `except` и даёт ""
//!     (Real-ESRGAN автоскачиванием никогда не появляется). Здесь скачивание
//!     делегировано методу [`UpIo::download_realesrgan`]; при паритете с
//!     оригиналом исполнитель возвращает `None` (баг воспроизведён), а живой
//!     исполнитель качает по-настоящему — это НЕ меняет ядро.
//!
//! ИСПОЛНИТЕЛЬ — не здесь: реальный запуск identify/апскейлеров, само
//!   скачивание+распаковка Real-ESRGAN (тело `download_realesrgan`), файловые
//!   операции и логи. Живой исполнитель на Rust — [`crate::imp_live::LiveUpIo`]
//!   (им пользуется хвост импорта Windows-тем); в процессе с Python его роль
//!   играет `_LiveUpIo` из `rexon/extensions/upscaler.py`. Оба переопределяют
//!   [`UpIo::image_size`]: Python — через Pillow, Rust — разбором заголовка
//!   самого файла ([`image_size_from_header`]), и ни одному из них ImageMagick
//!   для этого не нужен. Если размер не даётся ВООБЩЕ, исполнитель обязан
//!   сказать об этом словами ([`note_upscale_skipped`]), а не пропустить шаг
//!   молча.

/// Результат `subprocess.run`: `ok` = returncode == 0, плюс stdout (нужен
/// для разбора вывода `identify`).
pub struct RunResult {
    pub ok: bool,
    pub stdout: Vec<u8>,
}

/// Интерфейс исполнителя: каждый метод — ровно один побочный вызов
/// оригинала, в порядке исполнения. В parity-тестах реализуется реплеем
/// журнала; в живом мосте — реальным I/O.
pub trait UpIo {
    /// `subprocess.run(argv, capture_output=True, timeout=...)`.
    fn run(&mut self, argv: Vec<String>, timeout: u64) -> RunResult;
    /// `shutil.which(name)` → путь или None.
    fn which(&mut self, name: &str) -> Option<String>;
    /// `os.path.exists`.
    fn exists(&mut self, path: &str) -> bool;
    /// `os.path.isfile`.
    fn is_file(&mut self, path: &str) -> bool;
    /// `os.access(path, os.X_OK)`.
    fn access_x(&mut self, path: &str) -> bool;
    /// `os.remove(path)` (ошибки подавлены оригиналом).
    fn remove_file(&mut self, path: &str);
    /// Ветка автоскачивания Real-ESRGAN (`_ensure_realesrgan`, шаг 3): путь к
    /// бинарю или None. В оригинале всегда None (NameError, см. заголовок).
    fn download_realesrgan(&mut self) -> Option<String>;
    /// `os.listdir(path)`.
    fn listdir(&mut self, path: &str) -> Vec<String>;
    /// `os.replace(src, dst)`.
    fn replace(&mut self, src: &str, dst: &str);

    /// Готовый ответ `get_image_size` целиком, если исполнитель умеет мерить
    /// размер сам.
    ///
    /// Оригинал спрашивает размер СНАЧАЛА у Pillow (жёсткая зависимость
    /// Rexon, читает только заголовок) и лишь потом запускает `identify` —
    /// именно потому, что на SteamOS ImageMagick нет вовсе. Ядро Pillow
    /// вызвать не может, а без этого хука Rust-путь на такой машине всегда
    /// получал (0, 0) и апскейл молча не делался, тогда как Python-фолбэк
    /// работал. Живой мост отвечает здесь результатом
    /// `WallpaperUpscaler.get_image_size` (в нём и Pillow, и `identify`).
    ///
    /// `None` — исполнитель не умеет, ядро идёт запасным путём через
    /// `identify` (реплей журнала в parity-тестах так и остаётся).
    ///
    /// ⚠️ Ответ `Some((0, 0))` — законный и значит «померить не вышло НИЧЕМ».
    /// Живой исполнитель отвечает так только сказав об этом человеку: тихо
    /// пропущенный апскейл покупатель принял бы за нормальную работу.
    fn image_size(&mut self, _path: &str) -> Option<(i64, i64)> {
        None
    }
}

// ============================================================================
// Константы (порт полей класса)
// ============================================================================

/// `MIN_DESKTOP_WIDTH` — порог `needs_upscale` (макс. сторона меньше → апскейл).
pub const MIN_DESKTOP_WIDTH: i64 = 1024;
/// `MIN_DESKTOP_HEIGHT` (в оригинале объявлен, но в логике не используется).
pub const MIN_DESKTOP_HEIGHT: i64 = 768;

/// URL автоскачивания Real-ESRGAN (`_ESRGAN_URL`) — для исполнителя.
pub const ESRGAN_URL: &str =
    "https://github.com/xinntao/Real-ESRGAN/releases/download/\
     v0.2.5.0/realesrgan-ncnn-vulkan-20220424-ubuntu.zip";

/// Порядок автопробы стратегий в `method="auto"`.
pub const AUTO_METHODS: [&str; 3] = ["realesrgan", "waifu2x", "center"];

/// Модель Real-ESRGAN, передаваемая через `-n`.
const REALESRGAN_MODEL: &str = "realesrgan-x4plus";

/// Путь локальной копии Real-ESRGAN (`_ESRGAN_DIR/realesrgan-ncnn-vulkan`) от
/// домашнего каталога — исполнитель подставляет `expanduser("~")`.
pub fn esrgan_local_exe(home: &str) -> String {
    format!("{home}/.local/share/rexon/bin/realesrgan/realesrgan-ncnn-vulkan")
}

/// Расширения изображений для `process_wallpapers_in_dir` (`img_exts`).
const IMG_EXTS: [&str; 5] = [".jpg", ".jpeg", ".png", ".bmp", ".webp"];

// ============================================================================
// Хелперы семантики Python (os.path, int)
// ============================================================================

/// `os.path.splitext`: ведущие точки basename расширением не считаются.
fn splitext(p: &str) -> (&str, &str) {
    let base_start = p.rfind('/').map_or(0, |i| i + 1);
    let base = &p[base_start..];
    if let Some(dot_rel) = base.rfind('.') {
        if base[..dot_rel].chars().any(|c| c != '.') {
            let dot = base_start + dot_rel;
            return (&p[..dot], &p[dot..]);
        }
    }
    (p, "")
}

/// `os.path.join` для POSIX (двух аргументов достаточно).
fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_string()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// Дефолтный dest: `{base}_upscaled{ext}` (порт ветки `dest_path is None`).
fn default_dest(src: &str) -> String {
    let (base, ext) = splitext(src);
    format!("{base}_upscaled{ext}")
}

/// `int(token)` (десятичный, знак, ASCII-цифры). None — не число.
fn py_int(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let (neg, digits) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i64>().ok().map(|v| if neg { -v } else { v })
}

/// `int(parts[0]), int(parts[1])` от `stdout.strip().split()`; при
/// IndexError/ValueError → (0, 0).
pub fn parse_identify(stdout: &[u8]) -> (i64, i64) {
    let s = String::from_utf8_lossy(stdout);
    let mut parts = s.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some(a), Some(b)) => match (py_int(a), py_int(b)) {
            (Some(w), Some(h)) => (w, h),
            _ => (0, 0),
        },
        _ => (0, 0),
    }
}

// ============================================================================
// Чистая логика: пороги
// ============================================================================

/// `suggest_scale(w, h)` → множитель 2/4/8/1. Пороги по большей стороне.
/// Намеренное расхождение (структурное, без изменения поведения): в оригинале
/// две отдельные ветки `<800 → 4` (XP/Vista) и `<1024 → 4` (HD-ready) с
/// одинаковым результатом; здесь объединены в `< 1024 → 4` (значения
/// идентичны, golden покрывает 799/800/1023/1024).
pub fn suggest_scale(w: i64, h: i64) -> i64 {
    let longest = w.max(h);
    if longest < 480 {
        8 // мобильное
    } else if longest < 1024 {
        4 // XP/Vista / HD-ready
    } else if longest < 1920 {
        2 // HD → Full HD
    } else {
        1 // уже достаточно
    }
}

/// `get_image_size` — сначала спрашиваем исполнителя (у живого моста там
/// Pillow-первым, как в оригинале), иначе запуск identify + разбор. (0, 0)
/// при ошибке.
fn get_image_size(io: &mut dyn UpIo, path: &str) -> (i64, i64) {
    if let Some(size) = io.image_size(path) {
        return size;
    }
    let r = io.run(
        vec![
            "identify".into(),
            "-format".into(),
            "%w %h".into(),
            path.into(),
        ],
        10,
    );
    if r.ok {
        parse_identify(&r.stdout)
    } else {
        (0, 0)
    }
}

/// `needs_upscale` — макс. сторона меньше `MIN_DESKTOP_WIDTH`. (0,0) → false.
pub fn needs_upscale(io: &mut dyn UpIo, path: &str) -> bool {
    let (w, h) = get_image_size(io, path);
    if w == 0 || h == 0 {
        return false;
    }
    w.max(h) < MIN_DESKTOP_WIDTH
}

// ============================================================================
// Резолв Real-ESRGAN и стратегии (argv байт-в-байт)
// ============================================================================

/// `_ensure_realesrgan`: system PATH → локальная копия → скачивание.
/// None — не найден (в оригинале скачивание всегда даёт "" из-за NameError).
fn ensure_realesrgan(io: &mut dyn UpIo, local_exe: &str) -> Option<String> {
    if let Some(exe) = io.which("realesrgan-ncnn-vulkan") {
        return Some(exe);
    }
    if io.is_file(local_exe) && io.access_x(local_exe) {
        return Some(local_exe.to_string());
    }
    io.download_realesrgan()
}

/// `_upscale_realesrgan`: одиночный `-s min(scale,4)` либо двухпроходный
/// x4→x2 при scale>4 (Real-ESRGAN не умеет x8 за проход).
fn upscale_realesrgan(
    io: &mut dyn UpIo,
    src: &str,
    dest: &str,
    scale: i64,
    local_exe: &str,
) -> String {
    let exe = match ensure_realesrgan(io, local_exe) {
        Some(e) => e,
        None => return src.to_string(),
    };
    let r = if scale > 4 {
        let tmp = format!("{dest}.tmp.png");
        let r1 = io.run(
            vec![
                exe.clone(),
                "-i".into(),
                src.into(),
                "-o".into(),
                tmp.clone(),
                "-s".into(),
                "4".into(),
                "-n".into(),
                REALESRGAN_MODEL.into(),
            ],
            300,
        );
        if !r1.ok {
            return src.to_string();
        }
        let r2 = io.run(
            vec![
                exe.clone(),
                "-i".into(),
                tmp.clone(),
                "-o".into(),
                dest.into(),
                "-s".into(),
                "2".into(),
                "-n".into(),
                REALESRGAN_MODEL.into(),
            ],
            300,
        );
        io.remove_file(&tmp);
        r2
    } else {
        let s = scale.min(4);
        io.run(
            vec![
                exe,
                "-i".into(),
                src.into(),
                "-o".into(),
                dest.into(),
                "-s".into(),
                s.to_string(),
                "-n".into(),
                REALESRGAN_MODEL.into(),
            ],
            300,
        )
    };
    if r.ok && io.exists(dest) {
        dest.to_string()
    } else {
        src.to_string()
    }
}

/// `_upscale_waifu2x`: `-s min(scale,4) -n 2` (noise level 2, max x4).
fn upscale_waifu2x(io: &mut dyn UpIo, src: &str, dest: &str, scale: i64) -> String {
    let exe = match io.which("waifu2x-ncnn-vulkan") {
        Some(e) => e,
        None => return src.to_string(),
    };
    let s = scale.min(4);
    let r = io.run(
        vec![
            exe,
            "-i".into(),
            src.into(),
            "-o".into(),
            dest.into(),
            "-s".into(),
            s.to_string(),
            "-n".into(),
            "2".into(),
        ],
        300,
    );
    if r.ok && io.exists(dest) {
        dest.to_string()
    } else {
        src.to_string()
    }
}

/// `_upscale_center_blur`: центр на размытом увеличенном фоне (цель tw×th).
/// `scale` не используется (квирк оригинала сохранён).
fn upscale_center_blur(
    io: &mut dyn UpIo,
    src: &str,
    dest: &str,
    tw: i64,
    th: i64,
) -> String {
    let r = io.run(
        vec![
            "convert".into(),
            src.into(),
            "(".into(),
            "+clone".into(),
            "-resize".into(),
            format!("{tw}x{th}!"),
            "-blur".into(),
            "0x30".into(),
            "-brightness-contrast".into(),
            "-20x-10".into(),
            ")".into(),
            "+swap".into(),
            "-gravity".into(),
            "center".into(),
            "-composite".into(),
            "-resize".into(),
            format!("{tw}x{th}"),
            dest.into(),
        ],
        30,
    );
    if r.ok && io.exists(dest) {
        dest.to_string()
    } else {
        src.to_string()
    }
}

/// `_upscale_imagemagick`: Lanczos-resize до `w*scale × h*scale` (перечитывает
/// размер оригинала через identify).
fn upscale_imagemagick(io: &mut dyn UpIo, src: &str, dest: &str, scale: i64) -> String {
    let (w, h) = get_image_size(io, src);
    let nw = w * scale;
    let nh = h * scale;
    let r = io.run(
        vec![
            "convert".into(),
            src.into(),
            "-resize".into(),
            format!("{nw}x{nh}"),
            "-filter".into(),
            "Lanczos".into(),
            dest.into(),
        ],
        30,
    );
    if r.ok && io.exists(dest) {
        dest.to_string()
    } else {
        src.to_string()
    }
}

/// `WallpaperUpscaler.upscale`. `method`: "auto" (realesrgan→waifu2x→center),
/// "realesrgan"/"waifu2x"/"center"/"imagemagick"; иначе — возврат src.
/// `dest_opt=None` → `{base}_upscaled{ext}`. `scale=0` → `suggest_scale`.
/// Возвращает путь к результату (или исходный src при неуспехе/ненужности).
pub fn upscale(
    io: &mut dyn UpIo,
    src: &str,
    dest_opt: Option<&str>,
    scale: i64,
    method: &str,
    local_exe: &str,
) -> String {
    let (w, h) = get_image_size(io, src);
    if w == 0 {
        return src.to_string();
    }
    let mut scale = scale;
    if scale == 0 {
        scale = suggest_scale(w, h);
    }
    if scale <= 1 {
        return src.to_string();
    }
    let dest = match dest_opt {
        Some(d) => d.to_string(),
        None => default_dest(src),
    };
    match method {
        "auto" => {
            for m in AUTO_METHODS {
                let result = upscale(io, src, Some(&dest), scale, m, local_exe);
                if result != src && io.exists(&result) {
                    return result;
                }
            }
            src.to_string()
        }
        "realesrgan" => upscale_realesrgan(io, src, &dest, scale, local_exe),
        "waifu2x" => upscale_waifu2x(io, src, &dest, scale),
        "center" => upscale_center_blur(io, src, &dest, 1920, 1080),
        "imagemagick" => upscale_imagemagick(io, src, &dest, scale),
        _ => src.to_string(),
    }
}

/// `process_wallpapers_in_dir`: апскейл маленьких обоев в папке in-place.
/// Возвращает число обработанных. Параметр `auto` оригинала опущен (мёртв).
pub fn process_wallpapers_in_dir(
    io: &mut dyn UpIo,
    data_dir: &str,
    local_exe: &str,
) -> i64 {
    let mut processed = 0i64;
    for f in io.listdir(data_dir) {
        let ext = splitext(&f).1.to_lowercase();
        if !IMG_EXTS.contains(&ext.as_str()) {
            continue;
        }
        let full = join(data_dir, &f);
        if !needs_upscale(io, &full) {
            continue;
        }
        let (w, h) = get_image_size(io, &full);
        let scale = suggest_scale(w, h);
        let tmp = format!("{full}.upscaled.png");
        let result = upscale(io, &full, Some(&tmp), scale, "auto", local_exe);
        if result != full && io.exists(&tmp) {
            io.replace(&tmp, &full);
            processed += 1;
        } else if io.exists(&tmp) {
            io.remove_file(&tmp);
        }
    }
    processed
}

// ============================================================================
// Размер картинки по её собственному заголовку
// ============================================================================
//
// ЗАЧЕМ. Оригинал спрашивал размер у Pillow, а `identify` держал запасным
// путём. В процессе без Python Pillow нет, и на машине без ImageMagick
// исполнителю становилось нечем мерить: размер выходил (0, 0), `needs_upscale`
// отвечал «не надо», и обои из импортированной темы Windows оставались в своём
// 800×600 — МОЛЧА. Покупатель решал бы, что программа так и работает.
//
// Мерить, однако, чаще всего нечего и не надо: ширина и высота лежат в первых
// байтах самого файла, и разобрать их дешевле, чем запускать чужую программу.
// Здесь разобраны все пять форматов, которые вообще попадают апскейлеру
// (`IMG_EXTS`), плюс GIF — он стоит трёх строк. `identify` остаётся запасным
// ходом для экзотики (TIFF, HEIF, битые файлы), а если и его нет — исполнитель
// обязан сказать об этом словами, а не промолчать (см. [`note_upscale_skipped`]).

/// Сколько байт от начала файла хватает, чтобы узнать размер.
///
/// У PNG/BMP/GIF/WebP размер лежит в первых трёх десятках байт, а вот JPEG
/// кладёт кадр (маркер SOF) ПОСЛЕ метаданных, и EXIF с миниатюрой занимает
/// десятки килобайт. Запас щедрый намеренно: читается всё равно только начало
/// файла, а не файл целиком (обои бывают по 20 МБ).
pub const HEADER_PROBE_BYTES: usize = 256 * 1024;

fn be16(b: &[u8], at: usize) -> i64 {
    ((b[at] as i64) << 8) | b[at + 1] as i64
}

fn le16(b: &[u8], at: usize) -> i64 {
    (b[at] as i64) | ((b[at + 1] as i64) << 8)
}

fn le24(b: &[u8], at: usize) -> i64 {
    (b[at] as i64) | ((b[at + 1] as i64) << 8) | ((b[at + 2] as i64) << 16)
}

fn be32(b: &[u8], at: usize) -> i64 {
    ((b[at] as i64) << 24)
        | ((b[at + 1] as i64) << 16)
        | ((b[at + 2] as i64) << 8)
        | b[at + 3] as i64
}

fn le32(b: &[u8], at: usize) -> i64 {
    (b[at] as i64)
        | ((b[at + 1] as i64) << 8)
        | ((b[at + 2] as i64) << 16)
        | ((b[at + 3] as i64) << 24)
}

/// Размер картинки по началу файла. `None` — формат не опознан либо заголовок
/// оборван (тогда зовущая сторона идёт запасным путём через `identify`).
///
/// Нулевые и отрицательные стороны считаются неопознанными: ответ «0×0» у
/// нас уже значит «не знаю», и второго значения с тем же смыслом быть не
/// должно.
pub fn image_size_from_header(head: &[u8]) -> Option<(i64, i64)> {
    let size = png_size(head)
        .or_else(|| gif_size(head))
        .or_else(|| bmp_size(head))
        .or_else(|| webp_size(head))
        .or_else(|| jpeg_size(head))?;
    if size.0 > 0 && size.1 > 0 {
        Some(size)
    } else {
        None
    }
}

/// PNG: подпись, затем блок IHDR — ширина и высота по 4 байта со смещения 16.
fn png_size(b: &[u8]) -> Option<(i64, i64)> {
    const SIGN: &[u8] = b"\x89PNG\r\n\x1a\n";
    if b.len() < 24 || &b[..8] != SIGN {
        return None;
    }
    Some((be32(b, 16), be32(b, 20)))
}

/// GIF: «логический экран» сразу за подписью, обе стороны по 2 байта.
fn gif_size(b: &[u8]) -> Option<(i64, i64)> {
    if b.len() < 10 || (&b[..6] != b"GIF87a" && &b[..6] != b"GIF89a") {
        return None;
    }
    Some((le16(b, 6), le16(b, 8)))
}

/// BMP: за файловым заголовком идёт DIB, и его РАЗМЕР говорит, какой это
/// вариант. У самого старого (BITMAPCOREHEADER, 12 байт) стороны по 2 байта,
/// у всех остальных — по 4. Высота бывает отрицательной (строки сверху вниз),
/// поэтому берётся по модулю.
fn bmp_size(b: &[u8]) -> Option<(i64, i64)> {
    if b.len() < 26 || &b[..2] != b"BM" {
        return None;
    }
    if le32(b, 14) == 12 {
        return Some((le16(b, 18), le16(b, 20)));
    }
    let w = le32(b, 18) as i32 as i64;
    let h = le32(b, 22) as i32 as i64;
    Some((w.abs(), h.abs()))
}

/// WebP: контейнер RIFF, а размер лежит в первом блоке — своём для каждой из
/// трёх разновидностей (с потерями, без потерь, расширенный).
fn webp_size(b: &[u8]) -> Option<(i64, i64)> {
    if b.len() < 30 || &b[..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return None;
    }
    match &b[12..16] {
        // VP8 (с потерями): за трёхбайтовой меткой кадра — сигнатура и стороны
        // по 14 бит каждая (старшие два бита — масштаб, он нам не нужен).
        b"VP8 " => {
            if b[23..26] != [0x9D, 0x01, 0x2A] {
                return None;
            }
            Some((le16(b, 26) & 0x3FFF, le16(b, 28) & 0x3FFF))
        }
        // VP8L (без потерь): 14 бит ширины и 14 бит высоты подряд, обе на
        // единицу меньше настоящих.
        b"VP8L" => {
            if b[20] != 0x2F {
                return None;
            }
            let bits = le32(b, 21);
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        // VP8X (расширенный): размер холста тремя байтами, тоже на единицу
        // меньше настоящего.
        b"VP8X" => Some((le24(b, 24) + 1, le24(b, 27) + 1)),
        _ => None,
    }
}

/// JPEG: обход маркеров от начала до первого кадра (SOF). Кадр лежит ПОСЛЕ
/// метаданных, поэтому идти приходится по цепочке длин, а не по смещению.
fn jpeg_size(b: &[u8]) -> Option<(i64, i64)> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 1 < b.len() {
        if b[i] != 0xFF {
            // Мусор между сегментами: ищем следующий маркер, а не сдаёмся.
            i += 1;
            continue;
        }
        // Подряд идущие 0xFF — заполнитель, маркер это следующий байт.
        let mut j = i + 1;
        while j < b.len() && b[j] == 0xFF {
            j += 1;
        }
        if j >= b.len() {
            return None;
        }
        let marker = b[j];
        i = j + 1;
        match marker {
            // Маркеры без сегмента: подпись, перезапуск, «пустой».
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            // Конец картинки либо начало сжатых данных — кадра дальше нет.
            0xD9 | 0xDA => return None,
            _ => {}
        }
        if i + 1 >= b.len() {
            return None;
        }
        let seg = be16(b, i) as usize;
        if seg < 2 {
            return None;
        }
        // SOF0…SOF15, кроме таблиц Хаффмана (C4), арифметики (C8) и
        // ограничителя (CC): у них тот же диапазон, но кадра они не несут.
        let is_frame =
            (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC;
        if is_frame {
            if i + 6 >= b.len() {
                return None;
            }
            // [длина][точность][высота][ширина]
            return Some((be16(b, i + 5), be16(b, i + 3)));
        }
        i += seg;
    }
    None
}

/// Предупреждение «апскейл пропущен, потому что размер узнать нечем».
///
/// Не исключение: тема ставится, обои на месте, пропущен только необязательный
/// шаг. Но и не молчание — человек обязан узнать, что произошло и что поставить
/// (правило #13). Имя пакета берётся для ЭТОГО дистрибутива.
pub fn note_upscale_skipped(family: &str) -> String {
    let hint = crate::utils::tool_package_hint("identify", family);
    let tail = if hint.is_empty() {
        "Поставьте ImageMagick, чтобы Rexon смог их увеличить.".to_string()
    } else {
        format!("{hint}, чтобы Rexon смог их увеличить.")
    };
    format!(
        "Размер части обоев определить нечем — они оставлены как есть, \
         без увеличения. {tail}"
    )
}

#[cfg(test)]
mod header_tests {
    use super::*;

    #[test]
    fn png_header_is_read_without_any_external_program() {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        b.extend_from_slice(&[0, 0, 0, 13]);
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&800u32.to_be_bytes());
        b.extend_from_slice(&600u32.to_be_bytes());
        assert_eq!(image_size_from_header(&b), Some((800, 600)));
    }

    #[test]
    fn gif_and_bmp_headers_are_read() {
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&320u16.to_le_bytes());
        gif.extend_from_slice(&240u16.to_le_bytes());
        assert_eq!(image_size_from_header(&gif), Some((320, 240)));

        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0; 12]);
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&1024i32.to_le_bytes());
        // Отрицательная высота — законный BMP «сверху вниз».
        bmp.extend_from_slice(&(-768i32).to_le_bytes());
        assert_eq!(image_size_from_header(&bmp), Some((1024, 768)));
    }

    #[test]
    fn jpeg_frame_is_found_behind_metadata() {
        let mut b = vec![0xFF, 0xD8];
        // APP0 длиной 16 байт — кадр лежит за ним, как в жизни.
        b.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
        b.extend_from_slice(&[0u8; 14]);
        // SOF0: длина, точность, высота, ширина.
        b.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        b.extend_from_slice(&480u16.to_be_bytes());
        b.extend_from_slice(&640u16.to_be_bytes());
        b.extend_from_slice(&[0u8; 6]);
        assert_eq!(image_size_from_header(&b), Some((640, 480)));
    }

    #[test]
    fn webp_lossy_and_lossless_are_read() {
        let mut lossy = b"RIFF".to_vec();
        lossy.extend_from_slice(&[0; 4]);
        lossy.extend_from_slice(b"WEBPVP8 ");
        lossy.extend_from_slice(&[0; 4]); // длина блока
        lossy.extend_from_slice(&[0, 0, 0]); // метка кадра
        lossy.extend_from_slice(&[0x9D, 0x01, 0x2A]);
        lossy.extend_from_slice(&1280u16.to_le_bytes());
        lossy.extend_from_slice(&720u16.to_le_bytes());
        assert_eq!(image_size_from_header(&lossy), Some((1280, 720)));

        let mut ll = b"RIFF".to_vec();
        ll.extend_from_slice(&[0; 4]);
        ll.extend_from_slice(b"WEBPVP8L");
        ll.extend_from_slice(&[0; 4]);
        ll.push(0x2F);
        // 14 бит ширины и 14 бит высоты, обе на единицу меньше настоящих.
        let bits: u32 = (100 - 1) | ((50 - 1) << 14);
        ll.extend_from_slice(&bits.to_le_bytes());
        ll.extend_from_slice(&[0; 8]);
        assert_eq!(image_size_from_header(&ll), Some((100, 50)));
    }

    /// Чужой формат и обрывок заголовка обязаны честно ответить «не знаю», а
    /// не выдумать размер: выдуманный увёл бы апскейл на пустое место.
    #[test]
    fn unknown_and_truncated_headers_answer_nothing() {
        assert_eq!(image_size_from_header(b""), None);
        assert_eq!(image_size_from_header(b"II*\x00 tiff"), None);
        // Оборванный PNG: подпись есть, блока IHDR ещё нет.
        assert_eq!(image_size_from_header(b"\x89PNG\r\n\x1a\n\x00\x00"), None);
        // PNG с нулевой стороной — тоже «не знаю», а не (0, N).
        let mut zero = b"\x89PNG\r\n\x1a\n".to_vec();
        zero.extend_from_slice(&[0; 8]);
        zero.extend_from_slice(&0u32.to_be_bytes());
        zero.extend_from_slice(&600u32.to_be_bytes());
        assert_eq!(image_size_from_header(&zero), None);
    }
}
