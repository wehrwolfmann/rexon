//! Витрины профиля Steam — этап 1: нарезка своей картинки и расчёт макета
//! для предпросмотра. В Steam ничего не пишется: ни запросов от имени
//! аккаунта, ни загрузки. Итог — папка с готовыми частями и памяткой.
//!
//! Два вида витрины:
//! * «Витрина иллюстраций» — основная часть 506 px и боковая 100 px через
//!   зазор 9 px; боковая короче основной (под ней в профиле счётчики);
//! * «Избранная иллюстрация» — одна часть 630 px.
//!
//! Размеры взяты из разметки публичного профиля (`max-width: 506px` и
//! `max-width: 100px` у картинок витрины). Место витрины на фоне 1920 px и
//! зазор сверены 12.10.2026 по живой публичной странице профиля (снимок
//! страницы шириной 1920 px, положение картинок найдено по точкам) и по
//! таблице стилей Steam `profilev2.css`:
//! * левый край картинки — x = 495: колонка 976 px по центру фона
//!   (472) + поле колонки 12 + поле блока витрины 10 + рамка картинки 1;
//! * зазор между картинками — 9 px: рамка 1 + отступ 7 + рамка 1;
//! * верх картинки первой витрины — y = 256 (на проверенном профиле блок
//!   первой витрины начинается на 240, поле блока иллюстраций 15, рамка 1).
//!   ⚠️ Высота шапки профиля зависит от её содержимого, а каждая витрина
//!   выше сдвигает следующие — поэтому сдвиг по высоте человек подбирает сам;
//! * боковая короче основной на 70 px — из той же таблицы стилей: плашка
//!   счётчика «+ N» под боковой 57 px + отступ 11 + две рамки по 1. По
//!   точкам живьём НЕ измерено (на проверенном профиле в витрине
//!   иллюстраций боковой картинки не было).
//!
//! Модель кадрирования: «рамка» витрины — ширина макета (615 или 630 px) на
//! высоту исходника при вписывании по ширине. Масштаб ≥ 1 приближает
//! картинку внутри рамки, сдвиги 0…1 выбирают видимое окно. Пропорции не
//! искажаются никогда: окно исходника всегда той же формы, что рамка.
//!
//! Решения (геометрия, имена, шаги сжатия, тексты) — здесь, файлы читаются
//! и пишутся тоже здесь: модуль ни от Qt, ни от сети не зависит.

use std::fs;
use std::io::{BufReader, BufWriter, Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use image::imageops::FilterType;
use image::{AnimationDecoder, DynamicImage, ImageFormat, RgbaImage};
use serde::Serialize;

use crate::i18n::{tr, trf};

// ============================================================================
// Константы Steam (часть — со звёздочкой «сверить живьём»)
// ============================================================================

/// Предел веса одного файла иллюстрации. Steam пишет «5 МБ»; берём меньшее
/// из двух прочтений (5 000 000 байт), чтобы не промахнуться на десятичных
/// и двоичных мегабайтах.
pub const FILE_LIMIT_BYTES: u64 = 5_000_000;

/// Уровень Steam, с которого открываются витрины. На этапе 1 у Steam не
/// спрашивается — только напоминание человеку.
pub const MIN_STEAM_LEVEL: u32 = 10;

/// Ширина основной части витрины иллюстраций (видна в разметке профиля).
pub const ARTWORK_MAIN_WIDTH: u32 = 506;
/// Ширина боковой части (видна в разметке профиля).
pub const ARTWORK_SIDE_WIDTH: u32 = 100;
/// Рамка вокруг картинки в ячейке витрины (`.screenshot_showcase_screenshot
/// { border: 1px }`).
pub const SLOT_BORDER: u32 = 1;
/// Отступ между большой ячейкой и правой колонкой
/// (`.screenshot_showcase_primary { margin-right: 7px }`).
pub const ARTWORK_SLOT_MARGIN: u32 = 7;
/// Зазор между основной и боковой КАРТИНКАМИ: рамка + отступ + рамка.
/// Сверено 12.10.2026 по живой странице: 9 px.
pub const ARTWORK_GAP: u32 = SLOT_BORDER + ARTWORK_SLOT_MARGIN + SLOT_BORDER;
/// Плашка счётчика «+ N» под боковой картинкой
/// (`.screenshot_showcase_smallscreenshot.screenshot_count { height: 57px }`).
pub const ARTWORK_COUNT_HEIGHT: u32 = 57;
/// Отступ под маленькой ячейкой
/// (`.screenshot_showcase_smallscreenshot { margin-bottom: 11px }`).
pub const ARTWORK_SMALL_MARGIN: u32 = 11;
/// На сколько боковая часть короче основной: под ней в профиле стоит
/// плашка счётчика. ⚠️ Посчитано по таблице стилей Steam (57 + 11 + 2
/// рамки), по точкам на живой странице не измерено.
pub const ARTWORK_SIDE_TRIM: u32 = ARTWORK_COUNT_HEIGHT + ARTWORK_SMALL_MARGIN + 2 * SLOT_BORDER;
/// Ширина избранной иллюстрации.
pub const FEATURED_WIDTH: u32 = 630;

/// Ширина фона профиля, на которую считаются координаты.
pub const PROFILE_BG_WIDTH: u32 = 1920;
/// Колонка содержимого профиля: 952 px плюс поля по 12 px, по центру фона
/// (`.profile_content { width: 952px; padding: 12px; margin: 0 auto }`).
pub const PROFILE_CONTENT_WIDTH: u32 = 952;
pub const PROFILE_CONTENT_PADDING: u32 = 12;
/// Поле блока витрины слева (`.profile_customization_block`).
pub const SHOWCASE_BLOCK_PADDING: u32 = 10;
/// Левый край КАРТИНКИ первой ячейки на фоне 1920 px: 472 + 12 + 10 + 1.
/// Сверено 12.10.2026 по живой странице: 495.
pub const SHOWCASE_X: u32 = (PROFILE_BG_WIDTH - PROFILE_CONTENT_WIDTH - 2 * PROFILE_CONTENT_PADDING) / 2
    + PROFILE_CONTENT_PADDING
    + SHOWCASE_BLOCK_PADDING
    + SLOT_BORDER;
/// Верх картинки ПЕРВОЙ витрины под шапкой профиля. Каждая витрина выше
/// сдвигает следующие вниз — поэтому в предпросмотре есть сдвиг.
/// Сошлось с живой страницей 12.10.2026 (блок первой витрины на 240, поле
/// блока иллюстраций 15, рамка 1). ⚠️ Проверено на ОДНОМ профиле: высота
/// шапки зависит от её содержимого.
pub const FIRST_SHOWCASE_Y: u32 = 256;

/// Колонка профиля: 960 px по центру фона 1920.
pub const PROFILE_COLUMN_X: u32 = 480;
pub const PROFILE_COLUMN_WIDTH: u32 = 960;
/// Окно предпросмотра по ширине фона: колонка профиля с полями по 240 px
/// (весь фон 1920 в окне вкладки мелковат).
pub const PREVIEW_VIEW_X: u32 = 240;
pub const PREVIEW_VIEW_WIDTH: u32 = 1440;

/// Краски макета профиля в предпросмотре. Это подражание странице профиля
/// Steam (тёмная страница, полупрозрачные подложки), а не тема Rexon —
/// поэтому от скина они не зависят.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PreviewLook {
    /// Страница ниже фона.
    pub page: &'static str,
    /// Подложка витрины и шапки.
    pub plate: &'static str,
    /// Полоса заголовка витрины.
    pub header: &'static str,
    pub text: &'static str,
    pub muted: &'static str,
    /// Заглушка аватара и значка уровня.
    pub placeholder: &'static str,
}

pub const PREVIEW_LOOK: PreviewLook = PreviewLook {
    page: "#1b2838",
    plate: "#66000000",
    header: "#80000000",
    text: "#ffffff",
    muted: "#acb2b8",
    placeholder: "#3d4450",
};

/// Предел высоты рамки: выше — уже не витрина, а ошибка вписывания.
pub const MAX_FRAME_HEIGHT: u32 = 10_000;
/// Предел памяти под кадры GIF после нарезки (RGBA). Длиннее — отказ с
/// просьбой укоротить ролик, а не падение посреди работы.
pub const MAX_GIF_WORK_BYTES: u64 = 1_200_000_000;
/// Предел масштаба на ползунке.
pub const MAX_ZOOM: f64 = 4.0;

// ============================================================================
// Вид витрины и её части
// ============================================================================

/// Вид витрины.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ShowcaseKind {
    /// «Витрина иллюстраций»: основная 506 + боковая 100.
    Artwork,
    /// «Избранная иллюстрация»: одна часть 630.
    Featured,
}

/// Роль части — от неё имя файла и подпись.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PartRole {
    Main,
    Side,
    Featured,
}

/// Часть витрины: где она в рамке и какой ширины.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PartSpec {
    pub role: PartRole,
    /// Левый край части в рамке витрины.
    pub x: u32,
    pub width: u32,
    /// На сколько часть короче рамки снизу.
    pub bottom_trim: u32,
}

const ARTWORK_PARTS: &[PartSpec] = &[
    PartSpec { role: PartRole::Main, x: 0, width: ARTWORK_MAIN_WIDTH, bottom_trim: 0 },
    PartSpec {
        role: PartRole::Side,
        x: ARTWORK_MAIN_WIDTH + ARTWORK_GAP,
        width: ARTWORK_SIDE_WIDTH,
        bottom_trim: ARTWORK_SIDE_TRIM,
    },
];

const FEATURED_PARTS: &[PartSpec] =
    &[PartSpec { role: PartRole::Featured, x: 0, width: FEATURED_WIDTH, bottom_trim: 0 }];

/// Все виды по порядку показа.
pub const KINDS: &[ShowcaseKind] = &[ShowcaseKind::Artwork, ShowcaseKind::Featured];

impl ShowcaseKind {
    pub fn key(self) -> &'static str {
        match self {
            ShowcaseKind::Artwork => "artwork",
            ShowcaseKind::Featured => "featured",
        }
    }

    pub fn from_key(key: &str) -> Option<ShowcaseKind> {
        KINDS.iter().copied().find(|k| k.key() == key)
    }

    /// Название вида так, как его зовёт Steam в редакторе профиля.
    pub fn label(self) -> String {
        match self {
            ShowcaseKind::Artwork => tr("Artwork Showcase"),
            ShowcaseKind::Featured => tr("Featured Artwork Showcase"),
        }
    }

    /// Строка-пояснение под выбором вида.
    pub fn hint(self) -> String {
        match self {
            ShowcaseKind::Artwork => tr("Two pictures side by side: a wide one (506 px) and a narrow one (100 px)."),
            ShowcaseKind::Featured => tr("One wide picture (630 px)."),
        }
    }

    pub fn parts(self) -> &'static [PartSpec] {
        match self {
            ShowcaseKind::Artwork => ARTWORK_PARTS,
            ShowcaseKind::Featured => FEATURED_PARTS,
        }
    }

    /// Ширина всей витрины (рамки), от левого края первой части до правого
    /// края последней.
    pub fn layout_width(self) -> u32 {
        self.parts().iter().map(|p| p.x + p.width).max().unwrap_or(0)
    }
}

impl PartRole {
    /// Слово в имени файла (переводится: имена понятны человеку на его
    /// языке).
    pub fn file_word(self) -> String {
        match self {
            // TRANSLATORS: part of a file name: the wide picture of a Steam
            // artwork showcase ("1-main.gif"). Lowercase, no spaces.
            PartRole::Main => tr("main"),
            // TRANSLATORS: part of a file name: the narrow right picture of a
            // Steam artwork showcase ("2-side.gif"). Lowercase, no spaces.
            PartRole::Side => tr("side"),
            // TRANSLATORS: part of a file name: the picture of a Steam
            // featured artwork showcase ("1-featured.gif"). Lowercase, no spaces.
            PartRole::Featured => tr("featured"),
        }
    }

    /// Подпись части для человека.
    pub fn label(self) -> String {
        match self {
            PartRole::Main => tr("Main (506 px)"),
            PartRole::Side => tr("Side (100 px)"),
            PartRole::Featured => tr("Featured (630 px)"),
        }
    }
}

// ============================================================================
// Кадрирование
// ============================================================================

/// Как человек поставил картинку: масштаб и сдвиги.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// 1 — картинка вписана по ширине витрины; больше — приближение.
    pub zoom: f64,
    /// 0 — левый край, 1 — правый (имеет смысл при приближении).
    pub pan_x: f64,
    /// 0 — верх, 1 — низ (имеет смысл при приближении).
    pub pan_y: f64,
}

impl Default for Placement {
    fn default() -> Self {
        Placement { zoom: 1.0, pan_x: 0.5, pan_y: 0.5 }
    }
}

impl Placement {
    /// Привести к допустимому: масштаб 1…[`MAX_ZOOM`], сдвиги 0…1; мусор
    /// (NaN) — к умолчанию.
    pub fn normalized(self) -> Placement {
        let fix = |v: f64, lo: f64, hi: f64, dflt: f64| {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                dflt
            }
        };
        Placement {
            zoom: fix(self.zoom, 1.0, MAX_ZOOM, 1.0),
            pan_x: fix(self.pan_x, 0.0, 1.0, 0.5),
            pan_y: fix(self.pan_y, 0.0, 1.0, 0.5),
        }
    }
}

/// Прямоугольник в пикселях исходника (дробный — для предпросмотра).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Готовый расчёт: что вырезать из исходника и как ляжет витрина.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Layout {
    pub kind: ShowcaseKind,
    /// Рамка витрины в пикселях Steam.
    pub frame_width: u32,
    pub frame_height: u32,
    /// Окно исходника, которое растягивается на рамку.
    pub crop: CropRect,
    /// Во сколько раз пиксель исходника растянется (>1 — растяжение).
    pub upscale: f64,
    /// Масштаб после приведения (для подсказки «нужна ширина не меньше…»).
    pub zoom: f64,
    pub parts: Vec<LayoutPart>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LayoutPart {
    pub role: PartRole,
    pub label: String,
    pub x: u32,
    pub width: u32,
    pub height: u32,
}

/// Высота рамки при вписывании исходника по ширине витрины.
pub fn frame_height(kind: ShowcaseKind, src_w: u32, src_h: u32) -> u32 {
    if src_w == 0 || src_h == 0 {
        return 0;
    }
    let w = kind.layout_width() as f64;
    ((src_h as f64) * w / (src_w as f64)).round().max(1.0) as u32
}

/// Расчёт макета. `None` — у исходника нет размеров либо рамка выходит за
/// разумную высоту ([`MAX_FRAME_HEIGHT`]).
pub fn layout(kind: ShowcaseKind, src_w: u32, src_h: u32, placement: Placement) -> Option<Layout> {
    let p = placement.normalized();
    let fh = frame_height(kind, src_w, src_h);
    if fh == 0 || fh > MAX_FRAME_HEIGHT {
        return None;
    }
    let fw = kind.layout_width();
    let (sw, sh) = (src_w as f64, src_h as f64);
    let cw = sw / p.zoom;
    let ch = sh / p.zoom;
    let crop = CropRect { x: (sw - cw) * p.pan_x, y: (sh - ch) * p.pan_y, w: cw, h: ch };
    let parts = kind
        .parts()
        .iter()
        .map(|s| LayoutPart {
            role: s.role,
            label: s.role.label(),
            x: s.x,
            width: s.width,
            height: fh.saturating_sub(s.bottom_trim).max(1),
        })
        .collect();
    Some(Layout { kind, frame_width: fw, frame_height: fh, crop, upscale: fw as f64 / cw, zoom: p.zoom, parts })
}

/// Растяжение, начиная с которого человека предупреждаем (немного больше
/// единицы — чтобы не пугать из-за округления).
pub const UPSCALE_WARN: f64 = 1.05;

/// Предупреждения к расчёту (картинка мала и растянется).
pub fn layout_warnings(l: &Layout) -> Vec<String> {
    let mut out = Vec::new();
    if l.upscale > UPSCALE_WARN {
        out.push(trf(
            "The picture is smaller than the showcase and will be stretched %1 times — it may look blurry. A picture at least %2 px wide looks sharp.",
            &[&format!("{:.1}", l.upscale), &((l.frame_width as f64 * l.zoom).ceil() as u32)],
        ));
    }
    out
}

// ============================================================================
// Исходник: проверка
// ============================================================================

/// Формат исходника.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFormat {
    Png,
    Jpeg,
    Gif,
}

/// Что известно об исходнике.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceInfo {
    pub format: SourceFormat,
    pub width: u32,
    pub height: u32,
    /// Кадров (у PNG и JPG — 1).
    pub frames: u32,
    /// Длительность одного прохода GIF, мс.
    pub duration_ms: u64,
    pub bytes: u64,
    pub animated: bool,
}

/// Формат по содержимому, а не по расширению: «.jpg», который на деле PNG,
/// в Steam не редкость.
pub fn sniff_format(head: &[u8]) -> Option<SourceFormat> {
    match image::guess_format(head).ok()? {
        ImageFormat::Png => Some(SourceFormat::Png),
        ImageFormat::Jpeg => Some(SourceFormat::Jpeg),
        ImageFormat::Gif => Some(SourceFormat::Gif),
        _ => None,
    }
}

/// Проверить исходник: формат, размеры, кадры. Ошибка — готовая фраза для
/// человека.
pub fn inspect(path: &Path) -> Result<SourceInfo, String> {
    let bytes = fs::metadata(path).map(|m| m.len()).map_err(|_| tr("The file cannot be read."))?;
    let data = fs::read(path).map_err(|_| tr("The file cannot be read."))?;
    let format = sniff_format(&data).ok_or_else(|| tr("This is not a PNG, JPG or GIF picture."))?;
    match format {
        SourceFormat::Gif => {
            let mut opts = gif::DecodeOptions::new();
            opts.set_color_output(gif::ColorOutput::Indexed);
            let mut dec = opts.read_info(Cursor::new(&data)).map_err(|_| tr("The GIF file is damaged."))?;
            let (w, h) = (dec.width() as u32, dec.height() as u32);
            let mut frames = 0u32;
            let mut duration_ms = 0u64;
            loop {
                match dec.read_next_frame() {
                    Ok(Some(f)) => {
                        frames += 1;
                        duration_ms += u64::from(f.delay) * 10;
                    }
                    Ok(None) => break,
                    // Обрыв в конце файла — частая болезнь GIF из сети:
                    // кадры до обрыва читаются, их и считаем.
                    Err(_) if frames > 0 => break,
                    Err(_) => return Err(tr("The GIF file is damaged.")),
                }
            }
            if w == 0 || h == 0 || frames == 0 {
                return Err(tr("The GIF file is damaged."));
            }
            Ok(SourceInfo { format, width: w, height: h, frames, duration_ms, bytes, animated: frames > 1 })
        }
        _ => {
            let reader = image::ImageReader::new(Cursor::new(&data))
                .with_guessed_format()
                .map_err(|_| tr("The file cannot be read."))?;
            let (w, h) = reader.into_dimensions().map_err(|_| tr("The picture is damaged."))?;
            Ok(SourceInfo { format, width: w, height: h, frames: 1, duration_ms: 0, bytes, animated: false })
        }
    }
}

/// Замечания к исходнику для человека (не ошибки — работать можно).
pub fn source_notes(info: &SourceInfo) -> Vec<String> {
    let mut out = Vec::new();
    if info.animated {
        out.push(trf(
            "Animated GIF: %1 frames, %2 s per loop.",
            &[&info.frames, &format!("{:.1}", info.duration_ms as f64 / 1000.0)],
        ));
    }
    if info.bytes > FILE_LIMIT_BYTES {
        out.push(tr("The file is larger than 5 MB; Rexon will compress the parts to fit Steam's limit."));
    }
    out
}

// ============================================================================
// Куда сохранять
// ============================================================================

/// Каталог «Изображения» по XDG: переменная `XDG_PICTURES_DIR`, затем
/// `user-dirs.dirs`, затем `~/Pictures`.
pub fn pictures_dir(
    env_pictures: Option<&str>,
    user_dirs_text: Option<&str>,
    home: &str,
) -> PathBuf {
    if let Some(p) = env_pictures.map(str::trim).filter(|p| !p.is_empty()) {
        return PathBuf::from(p.replace("$HOME", home));
    }
    if let Some(text) = user_dirs_text {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("XDG_PICTURES_DIR=") {
                let v = rest.trim().trim_matches('"').replace("$HOME", home);
                // Пустое значение или сам дом — «не задано» (так пишет xdg-user-dirs).
                if !v.is_empty() && v.trim_end_matches('/') != home.trim_end_matches('/') {
                    return PathBuf::from(v);
                }
            }
        }
    }
    PathBuf::from(format!("{home}/Pictures"))
}

/// Корень наборов: `<Изображения>/Rexon/steam-showcase`.
pub fn output_root(pictures: &Path) -> PathBuf {
    pictures.join("Rexon").join("steam-showcase")
}

/// Имя набора из имени исходника: без расширения, без «/» и управляющих
/// символов, не длиннее 60 символов.
pub fn set_name_from_source(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let clean: String = stem
        .chars()
        .map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c })
        .take(60)
        .collect();
    let clean = clean.trim().trim_start_matches('.').trim().to_string();
    if clean.is_empty() {
        // TRANSLATORS: folder name for a set of showcase pictures when the
        // source file has no usable name.
        tr("showcase")
    } else {
        clean
    }
}

/// Свободный каталог набора: `<имя>`, затем `<имя> (2)`, `<имя> (3)`…
/// Чужой набор с тем же именем не затирается.
pub fn unique_set_dir(root: &Path, name: &str) -> PathBuf {
    let first = root.join(name);
    if !first.exists() {
        return first;
    }
    (2..10_000)
        .map(|n| root.join(format!("{name} ({n})")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Имя файла части: «1-основная.gif».
pub fn part_file_name(index: usize, role: PartRole, ext: &str) -> String {
    format!("{}-{}.{}", index + 1, role.file_word(), ext)
}

/// Имя памятки.
pub fn readme_file_name() -> String {
    // TRANSLATORS: file name of the instructions saved next to the showcase
    // pictures. Lowercase, words joined with "-".
    format!("{}.txt", tr("how-to-upload"))
}

// ============================================================================
// Шаги сжатия GIF
// ============================================================================

/// Ступень сжатия GIF: число цветов и прореживание кадров.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GifStep {
    /// Цветов в общей палитре (2…256).
    pub colors: u16,
    /// Брать каждый N-й кадр (1 — все). Задержки выброшенных кадров
    /// прибавляются к оставшимся — длительность ролика не меняется.
    pub keep_every: u16,
}

/// Ступени по порядку: сначала палитра, потом кадры, потом опять палитра.
pub const GIF_STEPS: &[GifStep] = &[
    GifStep { colors: 256, keep_every: 1 },
    GifStep { colors: 128, keep_every: 1 },
    GifStep { colors: 128, keep_every: 2 },
    GifStep { colors: 64, keep_every: 2 },
    GifStep { colors: 64, keep_every: 3 },
    GifStep { colors: 32, keep_every: 3 },
    GifStep { colors: 32, keep_every: 4 },
    GifStep { colors: 16, keep_every: 6 },
];

/// Предупреждение о потере качества для ступени (пусто у первой).
pub fn gif_step_warning(step: GifStep, frames_before: usize, frames_after: usize) -> String {
    if step.colors >= 256 && step.keep_every <= 1 {
        return String::new();
    }
    let mut parts = Vec::new();
    if step.colors < 256 {
        parts.push(trf("colours reduced to %1", &[&step.colors]));
    }
    if step.keep_every > 1 {
        parts.push(trf("frames reduced from %1 to %2 (motion is less smooth)", &[&frames_before, &frames_after]));
    }
    trf("To fit Steam's 5 MB limit the GIF was compressed: %1. Quality is lower than the original.", &[&parts.join(", ")])
}

/// Номера кадров, остающихся после прореживания, и их задержки (сотые доли
/// секунды; задержки выброшенных прибавлены к предыдущему оставшемуся).
pub fn thin_frames(delays_cs: &[u16], keep_every: u16) -> Vec<(usize, u16)> {
    let k = keep_every.max(1) as usize;
    let mut out: Vec<(usize, u16)> = Vec::new();
    for (i, d) in delays_cs.iter().enumerate() {
        if i % k == 0 || out.is_empty() {
            out.push((i, *d));
        } else if let Some(last) = out.last_mut() {
            last.1 = last.1.saturating_add(*d);
        }
    }
    out
}

/// Задержка кадра GIF в сотых долях секунды из длительности в мс. Ноль
/// браузеры показывают как 0,1 с — так и записываем, честно.
pub fn delay_cs_from_ms(ms: u64) -> u16 {
    let cs = ((ms + 5) / 10).min(u16::MAX as u64) as u16;
    if cs < 2 {
        10
    } else {
        cs
    }
}

// ============================================================================
// Нарезка
// ============================================================================

/// Запрос на нарезку.
#[derive(Debug, Clone)]
pub struct SliceRequest {
    pub source: PathBuf,
    pub kind: ShowcaseKind,
    pub placement: Placement,
    /// Каталог набора (создаётся; должен быть свободен — см. [`unique_set_dir`]).
    pub out_dir: PathBuf,
}

/// Готовый файл.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OutFile {
    pub path: String,
    pub file_name: String,
    pub role: PartRole,
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub bytes: u64,
}

/// Итог нарезки.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SliceReport {
    pub dir: String,
    pub files: Vec<OutFile>,
    pub readme: String,
    pub warnings: Vec<String>,
}

/// Прогресс и отмена — от исполнителя (поток окна).
pub struct SliceControl<'a> {
    pub progress: &'a dyn Fn(f32),
    pub cancel: &'a AtomicBool,
}

fn cancelled(ctl: &SliceControl) -> bool {
    ctl.cancel.load(Ordering::Relaxed)
}

/// Окно исходника → рамка витрины (целочисленное окно, ресемплинг).
fn render_frame(src: &RgbaImage, l: &Layout, filter: FilterType) -> RgbaImage {
    let x = l.crop.x.round().max(0.0) as u32;
    let y = l.crop.y.round().max(0.0) as u32;
    let w = (l.crop.w.round() as u32).clamp(1, src.width().saturating_sub(x).max(1));
    let h = (l.crop.h.round() as u32).clamp(1, src.height().saturating_sub(y).max(1));
    let view = image::imageops::crop_imm(src, x, y, w, h).to_image();
    if view.width() == l.frame_width && view.height() == l.frame_height {
        view
    } else {
        image::imageops::resize(&view, l.frame_width, l.frame_height, filter)
    }
}

/// Часть из рамки.
fn cut_part(frame: &RgbaImage, part: &LayoutPart) -> RgbaImage {
    image::imageops::crop_imm(frame, part.x, 0, part.width, part.height.min(frame.height())).to_image()
}

/// Нарезать исходник на части витрины, уложить каждую в 5 МБ и положить
/// рядом памятку. Ошибка — готовая фраза для человека; недоделанный каталог
/// набора при ошибке снимается.
pub fn slice(req: &SliceRequest, ctl: &SliceControl) -> Result<SliceReport, String> {
    let info = inspect(&req.source)?;
    let l = layout(req.kind, info.width, info.height, req.placement)
        .ok_or_else(|| tr("The picture is too tall for a showcase."))?;
    fs::create_dir_all(&req.out_dir).map_err(|e| trf("Cannot create the folder: %1", &[&e]))?;
    let res = if info.format == SourceFormat::Gif {
        slice_gif(req, &info, &l, ctl)
    } else {
        slice_still(req, &info, &l, ctl)
    };
    match res {
        Ok((files, mut warnings)) => {
            warnings.splice(0..0, layout_warnings(&l));
            let readme = req.out_dir.join(readme_file_name());
            let text = readme_text(req.kind, &files);
            fs::write(&readme, text).map_err(|e| trf("Cannot save the file: %1", &[&e]))?;
            (ctl.progress)(1.0);
            Ok(SliceReport {
                dir: req.out_dir.to_string_lossy().into_owned(),
                files,
                readme: readme.to_string_lossy().into_owned(),
                warnings,
            })
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&req.out_dir);
            Err(e)
        }
    }
}

/// Картинка (PNG/JPG). PNG остаётся PNG, пока влезает в 5 МБ; иначе — JPEG
/// со ступенями качества (и предупреждение).
fn slice_still(
    req: &SliceRequest,
    info: &SourceInfo,
    l: &Layout,
    ctl: &SliceControl,
) -> Result<(Vec<OutFile>, Vec<String>), String> {
    let img = image::open(&req.source).map_err(|_| tr("The picture is damaged."))?.to_rgba8();
    (ctl.progress)(0.2);
    let frame = render_frame(&img, l, FilterType::Lanczos3);
    let mut files = Vec::new();
    let mut warnings = Vec::new();
    let n = l.parts.len().max(1);
    for (i, part) in l.parts.iter().enumerate() {
        if cancelled(ctl) {
            return Err(tr("Cancelled."));
        }
        let piece = cut_part(&frame, part);
        let (data, ext, note) = encode_still(&piece, info.format)?;
        if data.len() as u64 > FILE_LIMIT_BYTES {
            return Err(tr("Could not fit the picture into Steam's 5 MB limit."));
        }
        if let Some(note) = note {
            warnings.push(note);
        }
        let name = part_file_name(i, part.role, ext);
        let path = req.out_dir.join(&name);
        write_file(&path, &data)?;
        files.push(OutFile {
            path: path.to_string_lossy().into_owned(),
            file_name: name,
            role: part.role,
            label: part.label.clone(),
            width: piece.width(),
            height: piece.height(),
            frames: 1,
            bytes: data.len() as u64,
        });
        (ctl.progress)(0.2 + 0.8 * (i + 1) as f32 / n as f32);
    }
    Ok((files, warnings))
}

/// Качество JPEG по ступеням, когда PNG не влез.
pub const JPEG_QUALITIES: &[u8] = &[92, 85, 75, 60, 45];

fn encode_still(piece: &RgbaImage, format: SourceFormat) -> Result<(Vec<u8>, &'static str, Option<String>), String> {
    if format == SourceFormat::Png {
        let mut buf = Vec::new();
        let enc = image::codecs::png::PngEncoder::new_with_quality(
            &mut buf,
            image::codecs::png::CompressionType::Best,
            image::codecs::png::FilterType::Adaptive,
        );
        DynamicImage::ImageRgba8(piece.clone())
            .write_with_encoder(enc)
            .map_err(|e| trf("Cannot save the file: %1", &[&e]))?;
        if buf.len() as u64 <= FILE_LIMIT_BYTES {
            return Ok((buf, "png", None));
        }
    }
    // JPEG без прозрачности: прозрачное ложится на чёрное — так же, как
    // Steam показывает пустоту витрины.
    let rgb = DynamicImage::ImageRgba8(piece.clone()).to_rgb8();
    for (i, q) in JPEG_QUALITIES.iter().enumerate() {
        let mut buf = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, *q);
        DynamicImage::ImageRgb8(rgb.clone())
            .write_with_encoder(enc)
            .map_err(|e| trf("Cannot save the file: %1", &[&e]))?;
        if buf.len() as u64 <= FILE_LIMIT_BYTES {
            let note = if format == SourceFormat::Png {
                Some(tr("A part did not fit into 5 MB as PNG and was saved as JPG."))
            } else if i > 0 {
                Some(trf("JPG quality lowered to %1 to fit into 5 MB.", &[q]))
            } else {
                None
            };
            return Ok((buf, "jpg", note));
        }
    }
    Err(tr("Could not fit the picture into Steam's 5 MB limit."))
}

/// Кадры части и их задержки.
struct PartFrames {
    part: LayoutPart,
    frames: Vec<RgbaImage>,
}

/// GIF: все кадры через рамку, части — отдельными GIF с общей палитрой
/// каждый. Ступень сжатия выбирается ОДНА на все части: иначе основная и
/// боковая шли бы с разной частотой кадров.
fn slice_gif(
    req: &SliceRequest,
    info: &SourceInfo,
    l: &Layout,
    ctl: &SliceControl,
) -> Result<(Vec<OutFile>, Vec<String>), String> {
    let work = u64::from(info.frames) * u64::from(l.frame_width) * u64::from(l.frame_height) * 4;
    if work > MAX_GIF_WORK_BYTES {
        return Err(tr("This GIF is too long or too large to process. Shorten it or make it smaller and try again."));
    }
    let file = fs::File::open(&req.source).map_err(|_| tr("The file cannot be read."))?;
    let dec = image::codecs::gif::GifDecoder::new(BufReader::new(file)).map_err(|_| tr("The GIF file is damaged."))?;
    let mut parts: Vec<PartFrames> =
        l.parts.iter().map(|p| PartFrames { part: p.clone(), frames: Vec::new() }).collect();
    let mut delays: Vec<u16> = Vec::new();
    // Растяжение кадров GIF — треугольным фильтром: Ланцош на сотнях кадров
    // в разы дольше, а разница на палитре 256 цветов не видна.
    let filter = FilterType::Triangle;
    for (i, frame) in dec.into_frames().enumerate() {
        if cancelled(ctl) {
            return Err(tr("Cancelled."));
        }
        let frame = match frame {
            Ok(f) => f,
            // Обрыв в конце — берём то, что успели прочитать.
            Err(_) if !delays.is_empty() => break,
            Err(_) => return Err(tr("The GIF file is damaged.")),
        };
        let (num, den) = frame.delay().numer_denom_ms();
        let ms = if den == 0 { 0 } else { u64::from(num) / u64::from(den) };
        delays.push(delay_cs_from_ms(ms));
        let full = render_frame(frame.buffer(), l, filter);
        for pf in &mut parts {
            pf.frames.push(cut_part(&full, &pf.part));
        }
        (ctl.progress)(0.4 * (i + 1) as f32 / info.frames.max(1) as f32);
    }
    if delays.is_empty() {
        return Err(tr("The GIF file is damaged."));
    }

    let mut warnings = Vec::new();
    let total_steps = GIF_STEPS.len() as f32;
    for (si, step) in GIF_STEPS.iter().enumerate() {
        let kept = thin_frames(&delays, step.keep_every);
        // Прореживать ролик из пары кадров бессмысленно — пропускаем ступень.
        if step.keep_every > 1 && kept.len() < 2 && delays.len() >= 2 {
            continue;
        }
        let mut encoded = Vec::new();
        let mut fits = true;
        for pf in &parts {
            if cancelled(ctl) {
                return Err(tr("Cancelled."));
            }
            let data = encode_gif(&pf.frames, &kept, step.colors)?;
            if data.len() as u64 > FILE_LIMIT_BYTES {
                fits = false;
                break;
            }
            encoded.push(data);
        }
        (ctl.progress)(0.4 + 0.6 * (si + 1) as f32 / total_steps);
        if !fits {
            continue;
        }
        let note = gif_step_warning(*step, delays.len(), kept.len());
        if !note.is_empty() {
            warnings.push(note);
        }
        let mut files = Vec::new();
        for (i, (pf, data)) in parts.iter().zip(encoded).enumerate() {
            let name = part_file_name(i, pf.part.role, "gif");
            let path = req.out_dir.join(&name);
            write_file(&path, &data)?;
            files.push(OutFile {
                path: path.to_string_lossy().into_owned(),
                file_name: name,
                role: pf.part.role,
                label: pf.part.label.clone(),
                width: pf.part.width,
                height: pf.part.height,
                frames: kept.len() as u32,
                bytes: data.len() as u64,
            });
        }
        if info.animated {
            warnings.push(tr("Steam does not synchronise GIFs in a showcase: the parts may drift apart. Reloading the profile page starts them together again."));
        }
        return Ok((files, warnings));
    }
    Err(tr("Even after strong compression the GIF does not fit into Steam's 5 MB limit. Shorten it or crop a smaller area and try again."))
}

/// Закодировать кадры части в GIF с одной общей палитрой на `colors` цветов
/// (прозрачность — отдельный цвет палитры).
fn encode_gif(frames: &[RgbaImage], kept: &[(usize, u16)], colors: u16) -> Result<Vec<u8>, String> {
    let first = frames.first().ok_or_else(|| tr("The GIF file is damaged."))?;
    let (w, h) = (first.width(), first.height());
    let has_alpha = kept.iter().any(|(i, _)| frames[*i].pixels().any(|p| p.0[3] < 128));
    let quant_colors = (colors as usize - usize::from(has_alpha)).clamp(2, 256);

    // Палитра учится на выборке пикселей всех оставшихся кадров — до ~1 млн
    // точек, чтобы длинный ролик не учился минутами.
    let total_px: usize = kept.len() * (w as usize) * (h as usize);
    let stride = (total_px / 1_000_000).max(1);
    let mut sample = Vec::with_capacity((total_px / stride + 1) * 4);
    let mut n = 0usize;
    for (i, _) in kept {
        for p in frames[*i].pixels() {
            if n % stride == 0 && p.0[3] >= 128 {
                sample.extend_from_slice(&[p.0[0], p.0[1], p.0[2], 255]);
            }
            n += 1;
        }
    }
    if sample.is_empty() {
        sample.extend_from_slice(&[0, 0, 0, 255]);
    }
    let nq = color_quant::NeuQuant::new(10, quant_colors, &sample);
    let mut palette = nq.color_map_rgb();
    let transparent = if has_alpha {
        let idx = (palette.len() / 3) as u8;
        palette.extend_from_slice(&[0, 0, 0]);
        Some(idx)
    } else {
        None
    };
    // Палитра GIF — степень двойки; дополняем чёрным.
    let mut size = 2usize;
    while size < palette.len() / 3 {
        size *= 2;
    }
    palette.resize(size * 3, 0);

    let mut out = Vec::new();
    {
        let mut enc = gif::Encoder::new(&mut out, w as u16, h as u16, &palette)
            .map_err(|e| trf("Cannot save the file: %1", &[&e]))?;
        enc.set_repeat(gif::Repeat::Infinite).map_err(|e| trf("Cannot save the file: %1", &[&e]))?;
        for (i, delay) in kept {
            let img = &frames[*i];
            let mut idx = Vec::with_capacity((w * h) as usize);
            for p in img.pixels() {
                if p.0[3] < 128 {
                    idx.push(transparent.unwrap_or(0));
                } else {
                    idx.push(nq.index_of(&[p.0[0], p.0[1], p.0[2], 255]) as u8);
                }
            }
            let mut f = gif::Frame::from_indexed_pixels(w as u16, h as u16, idx, transparent);
            f.delay = *delay;
            // Каждый кадр — целиком: прежний кадр стирается до фона, иначе
            // прозрачные места копили бы следы старых кадров.
            f.dispose = if transparent.is_some() { gif::DisposalMethod::Background } else { gif::DisposalMethod::Keep };
            enc.write_frame(&f).map_err(|e| trf("Cannot save the file: %1", &[&e]))?;
        }
    }
    Ok(out)
}

fn write_file(path: &Path, data: &[u8]) -> Result<(), String> {
    let f = fs::File::create(path).map_err(|e| trf("Cannot save the file: %1", &[&e]))?;
    let mut w = BufWriter::new(f);
    w.write_all(data).and_then(|_| w.flush()).map_err(|e| trf("Cannot save the file: %1", &[&e]))
}

// ============================================================================
// Памятка
// ============================================================================

/// Адрес страницы загрузки иллюстрации (открывает человек сам).
pub const UPLOAD_PAGE_URL: &str = "https://steamcommunity.com/sharedfiles/edititem/767/3/";

/// Текст памятки «что куда загружать». Без кода консоли. Те же шаги с
/// кнопками ведёт помощник в окне ([`wizard_steps`]); памятка остаётся в
/// папке на случай, когда набор открывают без Rexon.
pub fn readme_text(kind: ShowcaseKind, files: &[OutFile]) -> String {
    let mut s = String::new();
    s.push_str(&trf("Steam profile showcase: %1", &[&kind.label()]));
    s.push_str("\n\n");
    s.push_str(&tr("Files in this folder:"));
    s.push('\n');
    for f in files {
        s.push_str(&format!(
            "  • {} — {}, {}×{} px, {}\n",
            f.file_name,
            f.label,
            f.width,
            f.height,
            human_size(f.bytes)
        ));
    }
    s.push('\n');
    let steps: Vec<String> = match kind {
        ShowcaseKind::Artwork => vec![
            trf("Showcases open at Steam level %1. Check your level on your profile page.", &[&MIN_STEAM_LEVEL]),
            trf("Open the artwork upload page in Steam: %1 (Profile → Artwork → Upload).", &[&UPLOAD_PAGE_URL]),
            tr("Upload the files one by one, starting with number 1. Give each a clear title, for example \"Showcase — main\" and \"Showcase — side\": in Steam's picker the long pictures are hard to recognise by thumbnail."),
            tr("Tick \"I certify that I created this piece of art\" only if the picture is really yours. For someone else's art Steam may remove the upload when the author complains."),
            tr("Open Edit Profile → Featured Showcase, add \"Artwork Showcase\" and pick the main file for the large slot and the side file for the right slot."),
        ],
        ShowcaseKind::Featured => vec![
            trf("Showcases open at Steam level %1. Check your level on your profile page.", &[&MIN_STEAM_LEVEL]),
            trf("Open the artwork upload page in Steam: %1 (Profile → Artwork → Upload).", &[&UPLOAD_PAGE_URL]),
            tr("Upload the file and give it a clear title."),
            tr("Tick \"I certify that I created this piece of art\" only if the picture is really yours. For someone else's art Steam may remove the upload when the author complains."),
            tr("Open Edit Profile → Featured Showcase, add \"Featured Artwork Showcase\" and pick the uploaded file."),
        ],
    };
    for (i, step) in steps.iter().enumerate() {
        s.push_str(&format!("{}. {}\n", i + 1, step));
    }
    s.push('\n');
    s.push_str(&tr("Note: by default Steam may show a tall picture shrunk or with black bars. The same steps, with buttons that open the right Steam pages and a check of the result, are in Rexon: Steam → Profile Showcases."));
    s.push('\n');
    s
}

/// «1,2 МБ» / «340 КБ» — вес файла для человека.
pub fn human_size(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        trf("%1 MB", &[&format!("{:.1}", bytes as f64 / 1_000_000.0)])
    } else {
        trf("%1 KB", &[&((bytes + 999) / 1000)])
    }
}

// ============================================================================
// Помощник: загрузить части и выставить их в витрине
// ============================================================================
//
// ⛔ Граница. Rexon ничего не делает от имени учётной записи Steam: не
// загружает иллюстрации, не меняет профиль, не трогает cookie и токены
// сеанса ради записи. Помощник только открывает человеку нужные страницы,
// кладёт текст в буфер обмена, показывает папку с частями и читает
// ПУБЛИЧНУЮ страницу профиля без входа. Всё, что нажимается на страницах
// Steam, нажимает сам человек.

/// Корень сообщества Steam.
pub const STEAM_COMMUNITY_URL: &str = "https://steamcommunity.com";

/// Публичная страница профиля по SteamID64. Язык — английский: по нему
/// страница разбирается одинаково у всех.
pub fn profile_page_url(sid64: &str) -> String {
    format!("{STEAM_COMMUNITY_URL}/profiles/{sid64}/?l=english")
}

/// Страница редактора профиля на разделе витрин. Без SteamID64 — адрес
/// «мой профиль», который Steam сам переводит на вошедшего человека.
/// ⚠️ Адрес раздела взят из памятки и руководств; под входом не сверен.
pub fn edit_showcases_url(sid64: &str) -> String {
    if sid64.is_empty() {
        format!("{STEAM_COMMUNITY_URL}/my/edit/showcases")
    } else {
        format!("{STEAM_COMMUNITY_URL}/profiles/{sid64}/edit/showcases")
    }
}

/// Тот же адрес для клиента Steam: там человек уже вошёл, а в браузере —
/// не обязательно.
pub fn steam_client_url(url: &str) -> String {
    format!("steam://openurl/{url}")
}

/// «Пустое» название иллюстрации: один невидимый знак (U+2800). С ним под
/// картинкой в витрине не остаётся строки с названием. Вписывает его сам
/// человек — помощник только кладёт знак в буфер обмена.
/// ⚠️ Приём из руководств; что Steam принимает именно этот знак, живьём не
/// сверено — поэтому рядом всегда есть обычное понятное название.
pub const BLANK_TITLE: &str = "\u{2800}";

/// Понятное название части для поля «Название» на странице загрузки: по
/// нему часть легко узнать в окне выбора иллюстрации.
pub fn upload_title(set_name: &str, role: PartRole) -> String {
    let word = match role {
        // TRANSLATORS: word in the title of an uploaded Steam artwork: the
        // wide picture of the showcase.
        PartRole::Main => tr("main"),
        // TRANSLATORS: word in the title of an uploaded Steam artwork: the
        // narrow right picture of the showcase.
        PartRole::Side => tr("side"),
        // TRANSLATORS: word in the title of an uploaded Steam artwork: the
        // single picture of a featured artwork showcase.
        PartRole::Featured => tr("featured"),
    };
    format!("{set_name} — {word}")
}

/// Имя файла состояния помощника в папке набора. С точки — чтобы не
/// мешался в окне выбора файла на странице загрузки.
pub const WIZARD_STATE_FILE: &str = ".rexon-showcase.json";

/// Часть набора в состоянии помощника.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct WizardPart {
    pub file_name: String,
    /// `main` / `side` / `featured`.
    pub role: String,
    pub width: u32,
    pub height: u32,
    /// Человек отметил: часть загружена в Steam.
    #[serde(default)]
    pub uploaded: bool,
}

/// Состояние помощника: что за набор и докуда человек дошёл. Лежит рядом
/// с частями, поэтому к набору можно вернуться позже.
#[derive(Debug, Clone, Default, PartialEq, Serialize, serde::Deserialize)]
pub struct WizardState {
    /// Ключ вида витрины ([`ShowcaseKind::key`]).
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub parts: Vec<WizardPart>,
    /// Человек отметил: части выставлены в витрине.
    #[serde(default)]
    pub placed: bool,
    /// Последняя проверка публичного профиля сошлась.
    #[serde(default)]
    pub verified: bool,
}

fn role_key(role: PartRole) -> &'static str {
    match role {
        PartRole::Main => "main",
        PartRole::Side => "side",
        PartRole::Featured => "featured",
    }
}

fn role_from_key(key: &str) -> Option<PartRole> {
    match key {
        "main" => Some(PartRole::Main),
        "side" => Some(PartRole::Side),
        "featured" => Some(PartRole::Featured),
        _ => None,
    }
}

impl WizardState {
    /// Свежее состояние для только что нарезанного набора.
    pub fn for_files(kind: ShowcaseKind, files: &[OutFile]) -> WizardState {
        WizardState {
            kind: kind.key().to_string(),
            parts: files
                .iter()
                .map(|f| WizardPart {
                    file_name: f.file_name.clone(),
                    role: role_key(f.role).to_string(),
                    width: f.width,
                    height: f.height,
                    uploaded: false,
                })
                .collect(),
            placed: false,
            verified: false,
        }
    }

    pub fn showcase_kind(&self) -> ShowcaseKind {
        ShowcaseKind::from_key(&self.kind).unwrap_or(ShowcaseKind::Artwork)
    }

    pub fn all_uploaded(&self) -> bool {
        !self.parts.is_empty() && self.parts.iter().all(|p| p.uploaded)
    }

    /// Отметить часть загруженной (или снять отметку). Снятая отметка
    /// откатывает и следующие шаги: выставить и проверить можно только
    /// загруженное.
    pub fn set_uploaded(&mut self, file_name: &str, on: bool) {
        for p in &mut self.parts {
            if p.file_name == file_name {
                p.uploaded = on;
            }
        }
        if !self.all_uploaded() {
            self.placed = false;
            self.verified = false;
        }
    }

    /// Отметить «выставлено в витрине». Без загруженных частей не ставится.
    pub fn set_placed(&mut self, on: bool) {
        self.placed = on && self.all_uploaded();
        if !self.placed {
            self.verified = false;
        }
    }

    /// Ключ шага, на котором человек сейчас: `upload`, `place`, `check`
    /// либо `done`.
    pub fn current_step(&self) -> &'static str {
        if !self.all_uploaded() {
            "upload"
        } else if !self.placed {
            "place"
        } else if !self.verified {
            "check"
        } else {
            "done"
        }
    }
}

/// Прочитать состояние помощника из папки набора. Нет файла или он битый —
/// `None`: помощник начнёт сначала.
pub fn load_wizard_state(dir: &Path) -> Option<WizardState> {
    let text = fs::read_to_string(dir.join(WIZARD_STATE_FILE)).ok()?;
    let state: WizardState = serde_json::from_str(&text).ok()?;
    if state.parts.is_empty() {
        return None;
    }
    Some(state)
}

/// Записать состояние помощника в папку набора (через временный файл).
pub fn save_wizard_state(dir: &Path, state: &WizardState) -> Result<(), String> {
    let text = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{WIZARD_STATE_FILE}.part"));
    fs::write(&tmp, text).map_err(|e| trf("Cannot save the file: %1", &[&e]))?;
    fs::rename(&tmp, dir.join(WIZARD_STATE_FILE)).map_err(|e| trf("Cannot save the file: %1", &[&e]))
}

/// Шаг помощника для показа.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WizardStep {
    /// `upload` / `place` / `check`.
    pub key: &'static str,
    pub title: String,
    /// Короткие подписи «что нажать», по порядку.
    pub lines: Vec<String>,
    pub done: bool,
    /// Шаг, на котором человек сейчас.
    pub current: bool,
}

/// Три шага помощника с подписями под вид витрины и состояние набора.
pub fn wizard_steps(state: &WizardState) -> Vec<WizardStep> {
    let kind = state.showcase_kind();
    let current = state.current_step();
    let many = state.parts.len() > 1;
    let upload_lines = vec![
        tr("Press \"Open the upload page\": Steam opens the artwork upload page, and the folder with the parts opens next to it."),
        if many {
            tr("Upload the parts one by one, the main one first. For each: \"Choose file\" on the page, paste the title, press \"Save and Continue\".")
        } else {
            tr("\"Choose file\" on the page, paste the title, press \"Save and Continue\".")
        },
        tr("Tick \"I certify that I created this piece of art\" only if the picture is really yours."),
        tr("Leave visibility \"Public\": a hidden picture cannot be put into a showcase."),
        tr("After each upload tick the part here — Rexon remembers where you stopped."),
    ];
    let place_lines = match kind {
        ShowcaseKind::Artwork => vec![
            tr("Press \"Open the profile editor\": Steam opens Edit Profile on the showcases section."),
            trf("Choose \"%1\" in the showcase list.", &[&kind.label()]),
            tr("Click the large slot and pick the main part; click the first slot on the right and pick the side part."),
            tr("Press \"Save\" at the bottom of the page, then tick \"Done\" here."),
        ],
        ShowcaseKind::Featured => vec![
            tr("Press \"Open the profile editor\": Steam opens Edit Profile on the showcases section."),
            trf("Choose \"%1\" in the showcase list.", &[&kind.label()]),
            tr("Click the slot and pick the uploaded picture."),
            tr("Press \"Save\" at the bottom of the page, then tick \"Done\" here."),
        ],
    };
    let check_lines = vec![
        tr("Press \"Check my profile\": Rexon reads your public profile page (without signing in) and compares what Steam shows with your parts."),
        tr("The profile must be public, otherwise there is nothing to read."),
    ];
    vec![
        WizardStep {
            key: "upload",
            title: tr("Step 1. Upload the parts"),
            lines: upload_lines,
            done: state.all_uploaded(),
            current: current == "upload",
        },
        WizardStep {
            key: "place",
            title: tr("Step 2. Put them into the showcase"),
            lines: place_lines,
            done: state.placed,
            current: current == "place",
        },
        WizardStep {
            key: "check",
            title: tr("Step 3. Check"),
            lines: check_lines,
            done: state.verified,
            current: current == "check",
        },
    ]
}

// ============================================================================
// Публичная страница профиля: что Steam показывает в витрине
// ============================================================================

/// Картинка в ячейке витрины на странице профиля.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProfileSlot {
    /// Большая ячейка (иначе — маленькая справа).
    pub primary: bool,
    /// Номер иллюстрации в Steam.
    pub file_id: String,
    /// Потолок ширины из разметки (`max-width`): 506, 100 или 630.
    pub max_width: u32,
    /// Размер, в котором Steam отдаёт картинку (`imw`×`imh` в её адресе).
    pub shown_width: u32,
    pub shown_height: u32,
    pub image_url: String,
    /// Название под большой картинкой (у маленьких его нет).
    pub title: String,
}

/// Витрина с иллюстрациями на странице профиля.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProfileArtShowcase {
    /// Какой по счёту витриной сверху она стоит (с нуля).
    pub position: usize,
    /// Одна картинка во всю ширину — «Избранная иллюстрация».
    pub single: bool,
    pub slots: Vec<ProfileSlot>,
}

/// Что удалось прочитать с публичной страницы профиля.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProfilePage {
    /// Профиль закрыт — витрин на странице нет.
    pub private: bool,
    /// Заголовки всех витрин по порядку.
    pub headers: Vec<String>,
    /// Витрины иллюстраций (обычно одна).
    pub art: Vec<ProfileArtShowcase>,
}

/// Текст между `start` и ближайшим `end` после него.
fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    let len = text[from..].find(end)?;
    Some(&text[from..from + len])
}

/// Число сразу после `key` (до первого нецифрового знака).
fn number_after(text: &str, key: &str) -> u32 {
    text.find(key)
        .map(|i| &text[i + key.len()..])
        .map(|rest| rest.trim_start())
        .and_then(|rest| {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse().ok()
        })
        .unwrap_or(0)
}

/// Снять HTML-сущности, которые встречаются в названиях.
fn unescape_html(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// Одна ячейка: кусок разметки от её открывающего `div` до следующей.
fn parse_slot(chunk: &str, primary: bool) -> Option<ProfileSlot> {
    let img = between(chunk, "<img", ">")?;
    let image_url = unescape_html(between(img, "src=\"", "\"")?);
    let title = if primary {
        between(chunk, "screenshot_showcase_itemname\">", "</div>")
            .map(|t| unescape_html(t.trim()))
            .unwrap_or_default()
    } else {
        String::new()
    };
    Some(ProfileSlot {
        primary,
        file_id: between(chunk, "data-publishedfileid=\"", "\"").unwrap_or("").to_string(),
        max_width: number_after(img, "max-width:"),
        shown_width: number_after(&image_url, "imw="),
        shown_height: number_after(&image_url, "imh="),
        image_url,
        title,
    })
}

/// Разобрать публичную страницу профиля: закрыт ли он, какие витрины стоят
/// и что Steam показывает в витринах иллюстраций.
///
/// Разметка снята с живой страницы 12.10.2026: витрина иллюстраций — блок
/// `profile_customization myart`, внутри большая ячейка
/// `screenshot_showcase_primary` и маленькие `screenshot_showcase_smallscreenshot
/// showcase_slot`; размер показа стоит в адресе картинки (`imw`, `imh`).
/// ⚠️ У «Избранной иллюстрации» живого образца не было: она опознаётся по
/// классу `single` у большой ячейки (он есть в таблице стилей Steam).
pub fn parse_profile_page(html: &str) -> ProfilePage {
    let mut page = ProfilePage {
        private: html.contains("profile_private_info")
            || between(html, "<body", ">").map(|b| b.contains("private_profile")).unwrap_or(false),
        ..ProfilePage::default()
    };
    // Витрины идут подряд блоками `<div ... class="profile_customization…">`.
    const BLOCK: &str = "class=\"profile_customization";
    let mut starts: Vec<usize> = Vec::new();
    let mut at = 0;
    while let Some(i) = html[at..].find(BLOCK) {
        let pos = at + i;
        let after = &html[pos + BLOCK.len()..];
        // Только сам блок витрины: `profile_customization"` или с добавочным
        // классом через пробел, а не `profile_customization_header` и т. п.
        if after.starts_with('"') || after.starts_with(' ') {
            starts.push(pos);
        }
        at = pos + BLOCK.len();
    }
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(html.len());
        let block = &html[start..end];
        let classes = between(block, "class=\"", "\"").unwrap_or("");
        let header = between(block, "profile_customization_header\">", "</div>")
            .map(|t| unescape_html(t.trim()))
            .unwrap_or_default();
        page.headers.push(header);
        if !classes.split_whitespace().any(|c| c == "myart") {
            continue;
        }
        let mut showcase = ProfileArtShowcase { position: n, ..ProfileArtShowcase::default() };
        const PRIMARY: &str = "class=\"screenshot_showcase_primary";
        const SMALL: &str = "class=\"screenshot_showcase_smallscreenshot showcase_slot";
        if let Some(i) = block.find(PRIMARY) {
            let rest = &block[i..];
            let cut = rest.find("screenshot_showcase_rightcol").unwrap_or(rest.len());
            let chunk = &rest[..cut];
            showcase.single = between(chunk, "class=\"", "\"")
                .map(|c| c.split_whitespace().any(|w| w == "single"))
                .unwrap_or(false);
            // Пустая ячейка (`openslot`) картинки не несёт.
            if let Some(slot) = parse_slot(chunk, true) {
                showcase.slots.push(slot);
            }
        }
        let mut from = 0;
        while let Some(i) = block[from..].find(SMALL) {
            let pos = from + i;
            let rest = &block[pos + SMALL.len()..];
            let cut = rest.find("class=\"screenshot_showcase_smallscreenshot").unwrap_or(rest.len());
            if let Some(slot) = parse_slot(&rest[..cut], false) {
                showcase.slots.push(slot);
            }
            from = pos + SMALL.len();
        }
        page.art.push(showcase);
    }
    page
}

/// Итог сверки одной части с тем, что показывает Steam.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckPart {
    pub role: String,
    pub label: String,
    pub expected_width: u32,
    pub expected_height: u32,
    /// В нужной ячейке витрины есть картинка.
    pub found: bool,
    pub shown_width: u32,
    pub shown_height: u32,
    /// Размер показа совпал с размером части.
    pub ok: bool,
    pub image_url: String,
    pub title: String,
}

/// Итог проверки профиля.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckReport {
    /// `private` / `no_showcase` / `empty` / `mismatch` / `ok`.
    pub status: &'static str,
    pub ok: bool,
    /// Главная строка для человека.
    pub headline: String,
    /// Что именно не так и что делать.
    pub lines: Vec<String>,
    pub parts: Vec<CheckPart>,
    /// Сколько витрин стоит в профиле выше этой (от них зависит её место
    /// на фоне); `-1` — витрина не найдена.
    pub above: i32,
}

/// На сколько точек размер показа может отличаться от размера части:
/// Steam округляет высоту при пересчёте.
pub const CHECK_TOLERANCE: u32 = 1;

/// Сверить части набора с тем, что стоит в витрине на странице профиля.
pub fn check_profile(state: &WizardState, page: &ProfilePage) -> CheckReport {
    let kind = state.showcase_kind();
    let fail = |status: &'static str, headline: String, lines: Vec<String>| CheckReport {
        status,
        ok: false,
        headline,
        lines,
        parts: Vec::new(),
        above: -1,
    };
    if page.private {
        return fail(
            "private",
            tr("The profile is private — there is nothing to read."),
            vec![tr("Open Edit Profile → Privacy Settings in Steam and set \"My profile\" to \"Public\", then check again.")],
        );
    }
    let wanted_single = kind == ShowcaseKind::Featured;
    let Some(showcase) = page
        .art
        .iter()
        .find(|s| s.single == wanted_single)
        .or_else(|| page.art.first())
    else {
        return fail(
            "no_showcase",
            trf("There is no \"%1\" on your profile page yet.", &[&kind.label()]),
            vec![tr("Do step 2: add the showcase in the profile editor and press \"Save\".")],
        );
    };
    let mut parts = Vec::new();
    let mut small = showcase.slots.iter().filter(|s| !s.primary);
    let primary = showcase.slots.iter().find(|s| s.primary);
    for p in &state.parts {
        let role = role_from_key(&p.role).unwrap_or(PartRole::Main);
        let slot = if role == PartRole::Side { small.next() } else { primary };
        let (found, w, h, url, title) = match slot {
            Some(s) => (true, s.shown_width, s.shown_height, s.image_url.clone(), s.title.clone()),
            None => (false, 0, 0, String::new(), String::new()),
        };
        parts.push(CheckPart {
            role: p.role.clone(),
            label: role.label(),
            expected_width: p.width,
            expected_height: p.height,
            found,
            shown_width: w,
            shown_height: h,
            ok: found && w == p.width && h.abs_diff(p.height) <= CHECK_TOLERANCE,
            image_url: url,
            title,
        });
    }
    let above = showcase.position as i32;
    if parts.iter().all(|p| !p.found) {
        return CheckReport {
            status: "empty",
            ok: false,
            headline: tr("The showcase is on the profile, but its slots are empty."),
            lines: vec![tr("Do step 2: click each slot in the profile editor, pick the uploaded part and press \"Save\".")],
            parts,
            above,
        };
    }
    let mut lines = Vec::new();
    for p in &parts {
        if !p.found {
            lines.push(trf("%1: the slot is empty — pick the part in the profile editor.", &[&p.label]));
        } else if !p.ok {
            lines.push(trf(
                "%1: Steam shows it as %2×%3 px, the part is %4×%5 px.",
                &[&p.label, &p.shown_width, &p.shown_height, &p.expected_width, &p.expected_height],
            ));
        }
    }
    if lines.is_empty() {
        return CheckReport {
            status: "ok",
            ok: true,
            headline: tr("✅ The parts are in the showcase and their sizes match."),
            lines: Vec::new(),
            parts,
            above,
        };
    }
    lines.push(tr("If the size differs, another picture may be in the slot, or Steam is shrinking a tall picture. Check which picture is chosen in the profile editor."));
    CheckReport {
        status: "mismatch",
        ok: false,
        headline: tr("⚠️ The showcase does not match the parts yet."),
        lines,
        parts,
        above,
    }
}

// ============================================================================
// Сверки
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Delay, Frame, Rgba};
    use std::sync::Mutex;

    /// Язык сверок — исходный (английский): имена файлов и фразы сверяются
    /// по `msgid`.
    fn en() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    fn tmpdir(tag: &str) -> PathBuf {
        en();
        let d = std::env::temp_dir().join(format!("rexon-showcase-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn ctl_run<F: FnOnce(&SliceControl) -> R, R>(f: F) -> (R, Vec<f32>) {
        let seen = Mutex::new(Vec::new());
        let cancel = AtomicBool::new(false);
        let progress = |p: f32| seen.lock().unwrap().push(p);
        let ctl = SliceControl { progress: &progress, cancel: &cancel };
        let r = f(&ctl);
        (r, seen.into_inner().unwrap())
    }

    /// Пёстрая картинка (шум) — сжимается плохо, как настоящая.
    fn noisy(w: u32, h: u32, seed: u32) -> RgbaImage {
        let mut s = seed.wrapping_mul(2_654_435_761).max(1);
        RgbaImage::from_fn(w, h, |x, y| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            Rgba([(x * 255 / w.max(1)) as u8, (y * 255 / h.max(1)) as u8, (s & 0xff) as u8, 255])
        })
    }

    fn write_gif(path: &Path, frames: &[(RgbaImage, u32)]) {
        let f = fs::File::create(path).unwrap();
        let mut enc = image::codecs::gif::GifEncoder::new_with_speed(f, 30);
        enc.set_repeat(image::codecs::gif::Repeat::Infinite).unwrap();
        for (img, ms) in frames {
            enc.encode_frame(Frame::from_parts(img.clone(), 0, 0, Delay::from_numer_denom_ms(*ms, 1)))
                .unwrap();
        }
    }

    #[test]
    fn geometry_of_kinds() {
        assert_eq!(ShowcaseKind::Artwork.layout_width(), 615);
        assert_eq!(ShowcaseKind::Featured.layout_width(), 630);
        let side = ShowcaseKind::Artwork.parts()[1];
        assert_eq!((side.x, side.width, side.bottom_trim), (515, 100, 70));
        assert_eq!(ShowcaseKind::from_key("featured"), Some(ShowcaseKind::Featured));
        assert_eq!(ShowcaseKind::from_key("workshop"), None);
    }

    #[test]
    fn layout_keeps_proportions() {
        // 1230×2000 → рамка 615×1000; окно исходника той же формы при любом масштабе.
        let l = layout(ShowcaseKind::Artwork, 1230, 2000, Placement::default()).unwrap();
        assert_eq!((l.frame_width, l.frame_height), (615, 1000));
        assert_eq!(l.parts[0].height, 1000);
        assert_eq!(l.parts[1].height, 930);
        assert!((l.upscale - 0.5).abs() < 1e-9);
        let z = layout(ShowcaseKind::Artwork, 1230, 2000, Placement { zoom: 2.0, pan_x: 1.0, pan_y: 0.0 }).unwrap();
        assert!((z.crop.w / z.crop.h - 615.0 / 1000.0).abs() < 1e-3);
        assert!((z.crop.x - 615.0).abs() < 1e-9 && z.crop.y.abs() < 1e-9);
        assert!((z.upscale - 1.0).abs() < 1e-9);
        // Мусор в сдвигах — к умолчанию, масштаб — в пределы.
        let n = Placement { zoom: 99.0, pan_x: f64::NAN, pan_y: -3.0 }.normalized();
        assert_eq!((n.zoom, n.pan_x, n.pan_y), (MAX_ZOOM, 0.5, 0.0));
    }

    #[test]
    fn small_picture_warns_about_stretching() {
        en();
        let l = layout(ShowcaseKind::Featured, 300, 300, Placement::default()).unwrap();
        assert!(l.upscale > 2.0);
        assert_eq!(layout_warnings(&l).len(), 1);
        let big = layout(ShowcaseKind::Featured, 1260, 300, Placement::default()).unwrap();
        assert!(layout_warnings(&big).is_empty());
    }

    #[test]
    fn thinning_keeps_duration() {
        let d = [10u16, 10, 10, 10, 10];
        let k = thin_frames(&d, 2);
        assert_eq!(k, vec![(0, 20), (2, 20), (4, 10)]);
        let total: u32 = k.iter().map(|(_, d)| u32::from(*d)).sum();
        assert_eq!(total, 50);
        assert_eq!(thin_frames(&d, 1).len(), 5);
        assert_eq!(delay_cs_from_ms(0), 10);
        assert_eq!(delay_cs_from_ms(40), 4);
    }

    #[test]
    fn pictures_dir_follows_xdg() {
        assert_eq!(pictures_dir(Some("/x/P"), None, "/h"), PathBuf::from("/x/P"));
        let ud = "# c\nXDG_PICTURES_DIR=\"$HOME/Изображения\"\n";
        assert_eq!(pictures_dir(None, Some(ud), "/h"), PathBuf::from("/h/Изображения"));
        assert_eq!(pictures_dir(None, Some("XDG_PICTURES_DIR=\"$HOME/\""), "/h"), PathBuf::from("/h/Pictures"));
        assert_eq!(output_root(Path::new("/h/P")), PathBuf::from("/h/P/Rexon/steam-showcase"));
    }

    #[test]
    fn set_names_are_safe_and_unique() {
        assert_eq!(set_name_from_source(Path::new("/a/Ехидна.gif")), "Ехидна");
        assert_eq!(set_name_from_source(Path::new("/a/.gif")), "gif");
        let d = tmpdir("uniq");
        fs::create_dir_all(d.join("x")).unwrap();
        assert_eq!(unique_set_dir(&d, "x"), d.join("x (2)"));
        assert_eq!(unique_set_dir(&d, "y"), d.join("y"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn png_is_sliced_to_steam_widths() {
        let d = tmpdir("png");
        let src = d.join("pic.png");
        noisy(1230, 1800, 7).save(&src).unwrap();
        let info = inspect(&src).unwrap();
        assert_eq!((info.format, info.width, info.height, info.frames), (SourceFormat::Png, 1230, 1800, 1));
        let req = SliceRequest {
            source: src,
            kind: ShowcaseKind::Artwork,
            placement: Placement::default(),
            out_dir: d.join("out"),
        };
        let (r, progress) = ctl_run(|c| slice(&req, c));
        let r = r.unwrap();
        assert_eq!(r.files.len(), 2);
        let names: Vec<_> = r.files.iter().map(|f| f.file_name.clone()).collect();
        assert_eq!(names, vec!["1-main.png", "2-side.png"]);
        for f in &r.files {
            let img = image::open(&f.path).unwrap();
            assert_eq!((img.width(), img.height()), (f.width, f.height));
            assert!(f.bytes <= FILE_LIMIT_BYTES);
        }
        assert_eq!((r.files[0].width, r.files[0].height), (506, 900));
        assert_eq!((r.files[1].width, r.files[1].height), (100, 830));
        assert!(Path::new(&r.readme).exists());
        assert_eq!(progress.last().copied(), Some(1.0));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn heavy_png_falls_back_to_jpeg_under_limit() {
        // Шумная картинка 630×6000: PNG за 5 МБ, JPEG влезает.
        let d = tmpdir("heavy");
        let src = d.join("heavy.png");
        noisy(630, 6000, 3).save(&src).unwrap();
        let req = SliceRequest {
            source: src,
            kind: ShowcaseKind::Featured,
            placement: Placement::default(),
            out_dir: d.join("out"),
        };
        let (r, _) = ctl_run(|c| slice(&req, c));
        let r = r.unwrap();
        assert_eq!(r.files[0].file_name, "1-featured.jpg");
        assert!(r.files[0].bytes <= FILE_LIMIT_BYTES);
        assert!(!r.warnings.is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn gif_keeps_frames_and_delays() {
        let d = tmpdir("gif");
        let src = d.join("anim.gif");
        let frames: Vec<(RgbaImage, u32)> =
            (0..6).map(|i| (noisy(400, 300, i + 1), if i == 0 { 200 } else { 50 })).collect();
        write_gif(&src, &frames);
        let info = inspect(&src).unwrap();
        assert_eq!((info.frames, info.duration_ms, info.animated), (6, 450, true));
        let req = SliceRequest {
            source: src,
            kind: ShowcaseKind::Artwork,
            placement: Placement::default(),
            out_dir: d.join("out"),
        };
        let (r, _) = ctl_run(|c| slice(&req, c));
        let r = r.unwrap();
        let names: Vec<_> = r.files.iter().map(|f| f.file_name.clone()).collect();
        assert_eq!(names, vec!["1-main.gif", "2-side.gif"]);
        for f in &r.files {
            let mut o = gif::DecodeOptions::new();
            o.set_color_output(gif::ColorOutput::Indexed);
            let mut dec = o.read_info(fs::File::open(&f.path).unwrap()).unwrap();
            assert_eq!((dec.width() as u32, dec.height() as u32), (f.width, f.height));
            let mut delays = Vec::new();
            while let Some(fr) = dec.read_next_frame().unwrap() {
                delays.push(fr.delay);
            }
            assert_eq!(delays, vec![20, 5, 5, 5, 5, 5], "{}", f.file_name);
            assert!(f.bytes <= FILE_LIMIT_BYTES);
        }
        // 400×300 → рамка 615×461: картинка растянута — предупреждение есть.
        assert_eq!((r.files[0].width, r.files[0].height), (506, 461));
        assert_eq!((r.files[1].width, r.files[1].height), (100, 391));
        assert!(r.warnings.iter().any(|w| w.contains("stretched")));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn heavy_gif_is_compressed_under_limit_with_warning() {
        // Шумный GIF 630×800 на 24 кадра: без сжатия ~12 МБ.
        let d = tmpdir("heavygif");
        let src = d.join("heavy.gif");
        let frames: Vec<(RgbaImage, u32)> = (0..24).map(|i| (noisy(630, 800, i + 11), 40)).collect();
        write_gif(&src, &frames);
        let req = SliceRequest {
            source: src,
            kind: ShowcaseKind::Featured,
            placement: Placement::default(),
            out_dir: d.join("out"),
        };
        let (r, _) = ctl_run(|c| slice(&req, c));
        let r = r.unwrap();
        let f = &r.files[0];
        assert!(f.bytes <= FILE_LIMIT_BYTES, "{} байт", f.bytes);
        assert!(f.frames < 24, "кадров {}", f.frames);
        assert!(r.warnings.iter().any(|w| w.contains("5 MB")));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn not_a_picture_is_refused_and_leaves_nothing() {
        let d = tmpdir("bad");
        let src = d.join("x.gif");
        fs::write(&src, b"hello").unwrap();
        assert!(inspect(&src).is_err());
        let req = SliceRequest {
            source: src,
            kind: ShowcaseKind::Artwork,
            placement: Placement::default(),
            out_dir: d.join("out"),
        };
        let (r, _) = ctl_run(|c| slice(&req, c));
        assert!(r.is_err());
        assert!(!d.join("out").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn readme_names_files_and_has_no_console_code() {
        en();
        let files = vec![OutFile {
            path: "/x/1-main.gif".into(),
            file_name: "1-main.gif".into(),
            role: PartRole::Main,
            label: PartRole::Main.label(),
            width: 506,
            height: 900,
            frames: 10,
            bytes: 1_234_567,
        }];
        let t = readme_text(ShowcaseKind::Artwork, &files);
        assert!(t.contains("1-main.gif") && t.contains("1.2 MB") && t.contains(UPLOAD_PAGE_URL));
        for bad in ["$J(", "jQuery", "attr(", "image_width", "javascript"] {
            assert!(!t.contains(bad), "в памятке код консоли: {bad}");
        }
    }

    // ── Константы места витрины ──

    /// Числа, сверенные с живой страницей профиля 12.10.2026, не должны
    /// уехать при правке составляющих.
    #[test]
    fn showcase_geometry_matches_the_live_page() {
        assert_eq!(ARTWORK_GAP, 9);
        assert_eq!(ARTWORK_SIDE_TRIM, 70);
        assert_eq!(SHOWCASE_X, 495);
        assert_eq!(FIRST_SHOWCASE_Y, 256);
        // Боковая картинка начинается через зазор после основной: на живой
        // странице 495 + 506 + 9 = 1010.
        let side = ShowcaseKind::Artwork.parts()[1];
        assert_eq!(SHOWCASE_X + side.x, 1010);
        assert_eq!(ShowcaseKind::Artwork.layout_width(), 615);
    }

    // ── Помощник ──

    fn out_file(name: &str, role: PartRole, w: u32, h: u32) -> OutFile {
        OutFile {
            path: format!("/set/{name}"),
            file_name: name.to_string(),
            role,
            label: role.label(),
            width: w,
            height: h,
            frames: 1,
            bytes: 1000,
        }
    }

    fn artwork_state() -> WizardState {
        WizardState::for_files(
            ShowcaseKind::Artwork,
            &[out_file("1-main.png", PartRole::Main, 506, 900), out_file("2-side.png", PartRole::Side, 100, 830)],
        )
    }

    #[test]
    fn wizard_walks_through_three_steps() {
        en();
        let mut st = artwork_state();
        assert_eq!(st.current_step(), "upload");
        let steps = wizard_steps(&st);
        assert_eq!(steps.iter().map(|s| s.key).collect::<Vec<_>>(), ["upload", "place", "check"]);
        assert_eq!(steps.iter().map(|s| s.current).collect::<Vec<_>>(), [true, false, false]);
        assert!(steps.iter().all(|s| !s.done && !s.lines.is_empty()));

        // Выставить нельзя, пока не загружено всё.
        st.set_placed(true);
        assert!(!st.placed);
        st.set_uploaded("1-main.png", true);
        assert_eq!(st.current_step(), "upload");
        st.set_uploaded("2-side.png", true);
        assert_eq!(st.current_step(), "place");
        st.set_placed(true);
        assert_eq!(st.current_step(), "check");
        st.verified = true;
        assert_eq!(st.current_step(), "done");
        assert!(wizard_steps(&st).iter().all(|s| s.done && !s.current));

        // Сняли отметку загрузки — следующие шаги откатываются.
        st.set_uploaded("2-side.png", false);
        assert!(!st.placed && !st.verified);
        assert_eq!(st.current_step(), "upload");
    }

    #[test]
    fn wizard_texts_follow_the_kind() {
        en();
        let art = wizard_steps(&artwork_state());
        assert!(art[1].lines.iter().any(|l| l.contains("Artwork Showcase")));
        assert!(art[1].lines.iter().any(|l| l.contains("side part")));
        let featured = WizardState::for_files(
            ShowcaseKind::Featured,
            &[out_file("1-featured.png", PartRole::Featured, 630, 900)],
        );
        let steps = wizard_steps(&featured);
        assert!(steps[1].lines.iter().any(|l| l.contains("Featured Artwork Showcase")));
        assert!(!steps[1].lines.iter().any(|l| l.contains("side part")));
        // Ни один шаг не предлагает вставлять код в консоль браузера.
        for step in art.iter().chain(steps.iter()) {
            for line in &step.lines {
                let low = line.to_lowercase();
                assert!(!low.contains("console") && !low.contains("javascript"), "{line}");
            }
        }
    }

    #[test]
    fn wizard_state_survives_on_disk() {
        let dir = tmpdir("wizard");
        assert_eq!(load_wizard_state(&dir), None);
        let mut st = artwork_state();
        st.set_uploaded("1-main.png", true);
        save_wizard_state(&dir, &st).unwrap();
        assert_eq!(load_wizard_state(&dir), Some(st.clone()));
        assert!(dir.join(WIZARD_STATE_FILE).is_file());
        // Битый файл — не падение, а «начать сначала».
        fs::write(dir.join(WIZARD_STATE_FILE), "{не json").unwrap();
        assert_eq!(load_wizard_state(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wizard_addresses_and_titles() {
        en();
        assert_eq!(
            edit_showcases_url("76561190000000000"),
            "https://steamcommunity.com/profiles/76561190000000000/edit/showcases"
        );
        assert_eq!(edit_showcases_url(""), "https://steamcommunity.com/my/edit/showcases");
        assert_eq!(
            steam_client_url(UPLOAD_PAGE_URL),
            "steam://openurl/https://steamcommunity.com/sharedfiles/edititem/767/3/"
        );
        assert_eq!(
            profile_page_url("76561190000000000"),
            "https://steamcommunity.com/profiles/76561190000000000/?l=english"
        );
        assert_eq!(upload_title("Wolf", PartRole::Main), "Wolf — main");
        assert_eq!(upload_title("Wolf", PartRole::Side), "Wolf — side");
        // Невидимое название — один знак, и он не пробел, который Steam обрежет.
        assert_eq!(BLANK_TITLE.chars().count(), 1);
        assert!(!BLANK_TITLE.trim().is_empty());
    }

    // ── Публичная страница профиля ──

    const PROFILE_SAMPLE: &str = include_str!("../tests/golden/steam_profile_showcases_sample.html");

    #[test]
    fn profile_page_is_parsed() {
        let page = parse_profile_page(PROFILE_SAMPLE);
        assert!(!page.private);
        assert_eq!(
            page.headers,
            ["Rarest Achievement Showcase", "Screenshot Showcase", "Artwork Showcase", "Review Showcase"]
        );
        // Витрина снимков экрана устроена так же, но иллюстрациями не считается.
        assert_eq!(page.art.len(), 1);
        let art = &page.art[0];
        assert_eq!(art.position, 2);
        assert!(!art.single);
        assert_eq!(art.slots.len(), 2);
        let main = &art.slots[0];
        assert!(main.primary);
        assert_eq!((main.max_width, main.shown_width, main.shown_height), (506, 506, 900));
        assert_eq!(main.file_id, "200000001");
        assert_eq!(main.title, "Showcase & main");
        assert!(main.image_url.starts_with("https://images.steamusercontent.com/ugc/200000000000000001/"));
        assert!(main.image_url.contains("?imw=506&imh=900&"), "{}", main.image_url);
        let side = &art.slots[1];
        assert!(!side.primary);
        assert_eq!((side.max_width, side.shown_width, side.shown_height), (100, 100, 830));
        assert_eq!(side.file_id, "200000002");
        assert_eq!(side.title, "");
    }

    #[test]
    fn profile_page_private_and_empty() {
        let private = "<html><body class=\"flat_page profile_page private_profile\">\
            <div class=\"profile_private_info\">This profile is private.</div></body></html>";
        let page = parse_profile_page(private);
        assert!(page.private);
        assert!(page.art.is_empty());
        let none = parse_profile_page("<html><body class=\"profile_page\"></body></html>");
        assert!(!none.private && none.headers.is_empty() && none.art.is_empty());
        // Избранная иллюстрация: одна большая ячейка с классом `single`.
        let featured = "<body class=\"profile_page\"><div class=\"profile_customization myart\">\
            <div class=\"profile_customization_header\">Featured Artwork Showcase</div>\
            <div class=\"screenshot_showcase\"><div class=\"screenshot_showcase_primary single showcase_slot \" >\
            <a data-publishedfileid=\"7\" href=\"x\"><img width=\"100%\" style=\"max-width: 630px;\" \
            src=\"https://images.steamusercontent.com/ugc/1/A/?imw=630&amp;imh=400&amp;ima=fit\"></a>\
            <div class=\"screenshot_showcase_itemname\">One</div></div></div></div></body>";
        let page = parse_profile_page(featured);
        assert_eq!(page.art.len(), 1);
        assert!(page.art[0].single);
        assert_eq!(page.art[0].slots.len(), 1);
        assert_eq!((page.art[0].slots[0].shown_width, page.art[0].slots[0].shown_height), (630, 400));
    }

    #[test]
    fn profile_check_verdicts() {
        en();
        let page = parse_profile_page(PROFILE_SAMPLE);
        // Части того же размера, что показывает Steam, — сошлось.
        let st = artwork_state();
        let report = check_profile(&st, &page);
        assert_eq!(report.status, "ok");
        assert!(report.ok && report.lines.is_empty());
        assert_eq!(report.above, 2);
        assert_eq!(report.parts.len(), 2);
        assert!(report.parts.iter().all(|p| p.found && p.ok));
        assert_eq!(report.parts[0].title, "Showcase & main");

        // Высота расходится на точку — округление Steam, это не ошибка.
        let mut near = artwork_state();
        near.parts[0].height = 901;
        assert_eq!(check_profile(&near, &page).status, "ok");

        // Другая высота — расхождение с числами в подписи.
        let mut other = artwork_state();
        other.parts[0].height = 1200;
        let report = check_profile(&other, &page);
        assert_eq!(report.status, "mismatch");
        assert!(!report.ok);
        assert!(report.lines[0].contains("506×900") && report.lines[0].contains("506×1200"), "{:?}", report.lines);
        assert!(!report.parts[0].ok && report.parts[1].ok);

        // Закрытый профиль, нет витрины, пустые ячейки.
        let private = ProfilePage { private: true, ..ProfilePage::default() };
        assert_eq!(check_profile(&st, &private).status, "private");
        assert_eq!(check_profile(&st, &ProfilePage::default()).status, "no_showcase");
        let empty = ProfilePage {
            art: vec![ProfileArtShowcase { position: 0, single: false, slots: Vec::new() }],
            ..ProfilePage::default()
        };
        let report = check_profile(&st, &empty);
        assert_eq!(report.status, "empty");
        assert_eq!(report.above, 0);

        // Боковая ячейка пуста — названа именно она.
        let mut half = page.clone();
        half.art[0].slots.truncate(1);
        let report = check_profile(&st, &half);
        assert_eq!(report.status, "mismatch");
        assert!(report.parts[0].ok && !report.parts[1].found);
        assert!(report.lines[0].starts_with("Side (100 px)"), "{:?}", report.lines);
    }
}
