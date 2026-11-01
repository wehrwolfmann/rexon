//! Снимок виджета Conky для плитки — когда своей картинки у виджета нет.
//!
//! ЗАЧЕМ. Половина виджетов приходит без картинки, и плитка в разделе —
//! заглушка 📊: человек не видит, что у него стоит, пока не запустит. Rexon
//! снимает виджет сам и кладёт снимок в кеш
//! (`~/.cache/rexon/conky-previews/<ключ файла>-<отпечаток>.png`, рядом
//! `<ключ>.json` с отпечатком; снимок живого окна — `<ключ>.png`); плитка берёт его, если у виджета нет своей картинки
//! ([`crate::conky_scan::decorate_item`]).
//!
//! ## Невидимый экран (основной путь — и Wayland, и X11)
//!
//! Копия виджета запускается на своём невидимом X-сервере Xvfb (решение
//! Wehrwolfmann 25.10.2026): на настоящем столе ничего не мелькает, снимок
//! есть до первого запуска, и сеанс Wayland ему не мешает. Корнем экрана
//! ставятся текущие обои человека — прозрачный виджет на плитке лежит на
//! них, а не на чёрном. Как это устроено ([`Stand`]):
//! * Xvfb `-displayfd` (номер выбирает сам сервер), `-auth` со своей
//!   кукой, `-noreset` (иначе с уходом последнего клиента сервер
//!   сбрасывается и обои пропадают), `-nolisten tcp`; размер — главный
//!   монитор, `-dpi` — как у человека;
//! * Rexon занимает выбор `_NET_WM_CM_S0` («композитор есть»): conky ≥ 1.25
//!   тогда берёт 32-битное окно с альфой, и Rexon сам кладёт его на обои
//!   (предумноженное наложение `out = src + dst·(255−a)/255`, как у
//!   композитора). Прозрачные, полупрозрачные, непрозрачные и Lua/cairo
//!   выходят верно; корневой pixmap и `_XROOTPMAP_ID` ставятся всё равно —
//!   для старых conky с ложной прозрачностью и `own_window = false`;
//! * копия идёт с накладкой превью ([`preview_overlay`]): `background =
//!   false` (иначе conky уходит в фон с новым номером и не гасится),
//!   вывод только в X11 (`output_backend = 'x11'`, у старых
//!   `out_to_x`/`out_to_wayland`) и окружение `XDG_SESSION_TYPE=x11` без
//!   `WAYLAND_DISPLAY`: conky 1.25 в сеансе Wayland иначе выбрал бы
//!   Wayland и встал бы на НАСТОЯЩИЙ стол;
//! * своя группа процессов, `PR_SET_PDEATHSIG` (умер Rexon — умерли и они),
//!   гасится `killpg` TERM → KILL; копии, ушедшие мимо группы, — по номеру
//!   из `/proc`. Процессы превью не видят ни «На столе сейчас», ни
//!   «Остановить все» ([`is_preview_launch`]).
//!
//! Снимают только то, что можно запустить без вопроса ([`check_widget`]):
//! виджет только рисует или уже разрешён в нынешнем виде, и ему ничего не
//! недостаёт. Пароль при просмотре каталога не спрашивается, пакеты не
//! ставятся. Нет Xvfb или стенд не поднялся — плитка остаётся заглушкой,
//! без подсказок.
//!
//! Xvfb — обязательная зависимость пакета: Arch и его потомки —
//! `xorg-server-xvfb`, Debian/Ubuntu — `xvfb`, Fedora/openSUSE —
//! `xorg-x11-server-Xvfb` (`utils::TOOL_PACKAGES`). На SteamOS (`/usr`
//! только для чтения) тот же пакет Arch едет внутри sysext Rexon. Своей
//! копии X-сервера Rexon не вшивает.
//!
//! ## Живое окно (запасной путь X11 без Xvfb)
//!
//! Без Xvfb на X11 снимок делается при первом запуске виджета: окно — из
//! журнала conky (`drawing to created window 0x…`), снимок — `XGetImage`
//! через libX11, которую ядро грузит в момент вызова (`x11-dl`).

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::animations::LookIo;
use crate::de_apply::Ctx;

/// Подкаталог кеша Rexon со снимками.
pub const CACHE_SUBDIR: &str = "conky-previews";

/// Сколько ждать, пока виджет нарисуется: conky рисует на первом шаге
/// обновления, у части виджетов шаг — секунды.
pub const WAIT_MS: u64 = 6000;
const STEP_MS: u64 = 400;

/// Версия формата снимка (`<ключ>.json`): сменилась — всё переснимается.
pub const META_VERSION: u32 = 1;

/// Часть пути файлов стенда: `$XDG_RUNTIME_DIR/rexon/conky-preview/`.
pub const PREVIEW_SEGMENT: &str = "/rexon/conky-preview/";
/// Окончание имени накладки превью.
pub const PREVIEW_SUFFIX: &str = ".preview.conf";

/// Поля вокруг виджета на плитке, точки экрана.
pub const CARD_MARGIN: u32 = 16;
/// Длинная сторона плитки не больше.
pub const CARD_MAX: u32 = 640;
/// Стенд гаснет после стольких секунд без работы.
pub const STAND_IDLE: Duration = Duration::from_secs(10);
/// Цвет подложки, когда картинки обоев нет (живые обои, не узнали).
pub const FILL_RGB: [u8; 3] = [0x1e, 0x20, 0x26];

/// Снимок файла настроек `conf` в кеше.
pub fn cache_path(ctx: &Ctx, conf: &str) -> String {
    format!(
        "{}/{CACHE_SUBDIR}/{}.png",
        crate::appearance_cache::cache_dir(&ctx.home),
        crate::conky_overlay::key(conf)
    )
}

/// Отпечаток снимка рядом с ним.
pub fn meta_path(ctx: &Ctx, conf: &str) -> String {
    let png = cache_path(ctx, conf);
    format!("{}.json", png.trim_end_matches(".png"))
}

/// Снимок невидимого экрана с отпечатком `meta`: `<ключ>-<8 знаков
/// отпечатка>.png`. Новое имя у каждого переснятого снимка — иначе окно,
/// запомнившее картинку по адресу, показывало бы прежнюю.
pub fn shot_path(ctx: &Ctx, conf: &str, meta: &Meta) -> String {
    let json = serde_json::to_string(meta).unwrap_or_default();
    let h = <sha2::Sha256 as sha2::Digest>::digest(json.as_bytes());
    let tag: String = h.iter().take(4).map(|b| format!("{b:02x}")).collect();
    let png = cache_path(ctx, conf);
    format!("{}-{tag}.png", png.trim_end_matches(".png"))
}

/// Снимок файла `conf` из кеша: невидимого экрана (по отпечатку рядом),
/// иначе живого окна.
pub fn cached_one(ctx: &Ctx, conf: &str) -> Option<String> {
    let meta: Option<Meta> = std::fs::read_to_string(meta_path(ctx, conf)).ok().and_then(|t| serde_json::from_str(&t).ok());
    meta.map(|m| shot_path(ctx, conf, &m))
        .filter(|p| Path::new(p).is_file())
        .or_else(|| Some(cache_path(ctx, conf)).filter(|p| Path::new(p).is_file()))
}

/// Первый снимок из кеша для файлов `confs` (главный — первым).
pub fn cached(ctx: &Ctx, confs: &[String]) -> Option<String> {
    confs.iter().find_map(|c| cached_one(ctx, c))
}

/// Номер окна conky из его журнала — последнее «drawing to created window».
pub fn window_of_log(text: &str) -> Option<u64> {
    text.lines().rev().find_map(|l| {
        let rest = l.split("drawing to created window").nth(1)?;
        let hex = rest.trim().trim_start_matches("0x");
        let hex: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        u64::from_str_radix(&hex, 16).ok().filter(|w| *w != 0)
    })
}

/// Рисует ли conky прямо на корне экрана (`own_window = false`).
pub fn draws_on_desktop(log: &str) -> bool {
    log.contains("drawing to desktop window")
}

/// Можно ли снимать ЖИВОЕ окно в этом сеансе: X11 (есть `DISPLAY`, нет
/// Wayland).
pub fn can_capture(ctx: &Ctx, display: Option<&str>) -> bool {
    !ctx.wayland && display.is_some_and(|d| !d.is_empty())
}

// ════════════════════════════════════════════════════════════════════════════
//  Пиксели
// ════════════════════════════════════════════════════════════════════════════

/// Пиксель 32-битного окна conky (BGRA, цвет предумножен на альфу) в
/// прямой RGBA для PNG с прозрачностью. conky рисует обводки и полосы и
/// пикселями «цвет больше альфы» — композитор их СКЛАДЫВАЕТ с фоном, и они
/// видны; прямой цвет такого пикселя не выразить, поэтому альфа
/// поднимается до самого яркого канала: `a' = max(a, r, g, b)`.
pub fn straight_rgba(b: u8, g: u8, r: u8, a: u8) -> [u8; 4] {
    let a = a.max(r).max(g).max(b);
    let un = |c: u8| -> u8 {
        match a {
            0 => 0,
            255 => c,
            _ => ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8,
        }
    };
    [un(r), un(g), un(b), a]
}

/// Пиксели ZPixmap 32 бит (BGRA, младший байт первым) → RGBA. У окна с
/// глубиной 32 цвет предумножен — для PNG он возвращается к прямому
/// ([`straight_rgba`]); глубина 24 — непрозрачно.
pub fn bgra_to_rgba(data: &[u8], width: usize, height: usize, stride: usize, depth: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let row = &data[y * stride..];
        for x in 0..width {
            let p = &row[x * 4..x * 4 + 4];
            if depth == 32 {
                out.extend_from_slice(&straight_rgba(p[0], p[1], p[2], p[3]));
            } else {
                out.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
    }
    out
}

/// Пиксель окна (BGRA) поверх подложки `under` (RGB) — как композитор:
/// у глубины 32 источник предумножен, `out = src + dst·(255−a)/255`;
/// у глубины 24 окно непрозрачно.
pub fn over(p: &[u8], depth: u32, under: [u8; 3]) -> [u8; 3] {
    if depth != 32 {
        return [p[2], p[1], p[0]];
    }
    let a = p[3] as u32;
    let mix = |s: u8, d: u8| -> u8 { (s as u32 + (d as u32 * (255 - a) + 127) / 255).min(255) as u8 };
    [mix(p[2], under[0]), mix(p[1], under[1]), mix(p[0], under[2])]
}

/// Нарисовано ли что-нибудь: не все пиксели одинаковы (4 байта на точку).
pub fn has_content(px: &[u8]) -> bool {
    let first = px.get(..4);
    px.chunks_exact(4).any(|p| Some(p) != first)
}

/// Рамка того, чем `now` (RGB) отличается от `base` (RGB) того же размера:
/// `(x, y, ширина, высота)`. `None` — ничем.
pub fn diff_bbox(now: &[u8], base: &[u8], width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 3) as usize;
            if now.get(i..i + 3) != base.get(i..i + 3) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    (x0 != u32::MAX).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

/// Записать RGBA в PNG.
pub fn write_png(path: &str, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if let Some(dir) = Path::new(path).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = format!("{path}.part");
    let file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(rgba).map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

// ════════════════════════════════════════════════════════════════════════════
//  Подложка (обои) и плитка
// ════════════════════════════════════════════════════════════════════════════

/// Обои экрана стенда: RGB во весь экран и метка для отпечатка.
#[derive(Debug, Clone, PartialEq)]
pub struct Backdrop {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    /// `путь|mtime|размер` картинки или `fill` — ровная заливка.
    pub tag: String,
}

/// Метка картинки обоев для отпечатка: путь, время правки и размер.
/// `fill` — картинки нет.
pub fn backdrop_tag(path: Option<&str>) -> String {
    let Some(p) = path else { return "fill".to_string() };
    let Ok(m) = std::fs::metadata(p) else { return "fill".to_string() };
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{p}|{mtime}|{}", m.len())
}

impl Backdrop {
    /// Ровная тёмная заливка.
    pub fn fill(width: u32, height: u32) -> Self {
        let rgb = FILL_RGB.iter().copied().cycle().take((width * height * 3) as usize).collect();
        Backdrop { width, height, rgb, tag: "fill".to_string() }
    }

    /// Картинка `path`, заполняющая экран (масштаб по большей стороне,
    /// обрезка по центру — как «Заполнить» у столов). Не открылась —
    /// заливка.
    pub fn from_image(path: Option<&str>, width: u32, height: u32) -> Self {
        let Some(p) = path else { return Self::fill(width, height) };
        let Ok(img) = image::open(p) else { return Self::fill(width, height) };
        let img = img.to_rgb8();
        let (iw, ih) = img.dimensions();
        if iw == 0 || ih == 0 {
            return Self::fill(width, height);
        }
        let k = (width as f64 / iw as f64).max(height as f64 / ih as f64);
        let (sw, sh) = (((iw as f64 * k).ceil() as u32).max(width), ((ih as f64 * k).ceil() as u32).max(height));
        let scaled = image::imageops::resize(&img, sw, sh, image::imageops::FilterType::Triangle);
        let (l, t) = ((sw - width) / 2, (sh - height) / 2);
        let crop = image::imageops::crop_imm(&scaled, l, t, width, height).to_image();
        Backdrop { width, height, rgb: crop.into_raw(), tag: backdrop_tag(Some(p)) }
    }

    fn at(&self, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * self.width + x) * 3) as usize;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }
}

/// Плитка: кусок экрана `piece` (RGB, `w×h` в точке `x, y`) на обоях с
/// полями [`CARD_MARGIN`], длинная сторона не больше [`CARD_MAX`]. RGBA.
pub fn card(bg: &Backdrop, piece: &[u8], x: u32, y: u32, w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    let x0 = x.saturating_sub(CARD_MARGIN);
    let y0 = y.saturating_sub(CARD_MARGIN);
    let x1 = (x + w + CARD_MARGIN).min(bg.width);
    let y1 = (y + h + CARD_MARGIN).min(bg.height);
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut out = image::RgbaImage::new(cw, ch);
    for cy in 0..ch {
        for cx in 0..cw {
            let (sx, sy) = (x0 + cx, y0 + cy);
            let c = if sx >= x && sx < x + w && sy >= y && sy < y + h {
                let i = (((sy - y) * w + (sx - x)) * 3) as usize;
                [piece[i], piece[i + 1], piece[i + 2]]
            } else {
                bg.at(sx, sy)
            };
            out.put_pixel(cx, cy, image::Rgba([c[0], c[1], c[2], 255]));
        }
    }
    let long = cw.max(ch);
    if long > CARD_MAX {
        let k = CARD_MAX as f64 / long as f64;
        let (nw, nh) = (((cw as f64 * k).round() as u32).max(1), ((ch as f64 * k).round() as u32).max(1));
        out = image::imageops::resize(&out, nw, nh, image::imageops::FilterType::Lanczos3);
    }
    let (w, h) = out.dimensions();
    (w, h, out.into_raw())
}

// ════════════════════════════════════════════════════════════════════════════
//  Накладка превью, выдержка, кука
// ════════════════════════════════════════════════════════════════════════════

/// Каталог стенда: `$XDG_RUNTIME_DIR/rexon/conky-preview` (у сеанса без
/// него — во временном каталоге под номером пользователя).
pub fn runtime_dir() -> String {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(r) if !r.is_empty() => format!("{}{}", r.trim_end_matches('/'), PREVIEW_SEGMENT.trim_end_matches('/')),
        _ => {
            // SAFETY: getuid не имеет побочных действий.
            let uid = unsafe { libc::getuid() };
            format!(
                "{}/rexon-{uid}{}",
                std::env::temp_dir().to_string_lossy().trim_end_matches('/'),
                PREVIEW_SEGMENT.trim_end_matches('/')
            )
        }
    }
}

/// Запущен ли conky с накладкой превью — копия на невидимом экране, а не
/// виджет на столе.
pub fn is_preview_launch(launch: &str) -> bool {
    launch.contains(PREVIEW_SEGMENT) && launch.ends_with(PREVIEW_SUFFIX)
}

/// Текст накладки превью: выполнить `source` (файл автора, его перевод из
/// 1.9 или накладку с правками человека) и удержать копию на невидимом
/// экране X11, на переднем плане.
pub fn preview_overlay(source: &str, version: Option<(u32, u32)>) -> String {
    let lua = crate::conky_overlay::lua_str;
    let mut out = String::new();
    out.push_str("-- Rexon: a copy of the widget on an invisible screen, for its tile picture.\n");
    out.push_str(&format!("dofile({})\n", lua(source)));
    out.push_str("conky.config = conky.config or {}\n");
    out.push_str("local c = conky.config\n");
    out.push_str("c.background = false\n");
    out.push_str("c.xinerama_head = nil\n");
    match version {
        Some(v) if v >= (1, 25) => out.push_str("c.output_backend = 'x11'\n"),
        Some(_) => out.push_str("c.out_to_x = true\nc.out_to_wayland = false\n"),
        // Версия не узнана — всё сразу: незнакомое поле conky лишь отметит.
        None => out.push_str("c.output_backend = 'x11'\nc.out_to_x = true\nc.out_to_wayland = false\n"),
    }
    out
}

/// Сколько ждать после появления окна, пока виджет дорисуется: два шага
/// обновления, но от полусекунды до трёх секунд.
pub fn settle_ms(update_interval: Option<f64>) -> u64 {
    let ui = update_interval.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0);
    ((ui * 2000.0) as u64).clamp(500, 3000)
}

/// Запись `.Xauthority` FamilyWild с кукой MIT-MAGIC-COOKIE-1. Номер
/// дисплея `display_num` пуст до запуска (`-displayfd`: номер выбирает
/// сервер, а пустой номер он принимает за любой); клиентам libX11 номер
/// нужен — после запуска файл переписывается с ним.
pub fn auth_record(display_num: &str, cookie: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut field = |b: &[u8]| {
        out.extend_from_slice(&(b.len() as u16).to_be_bytes());
        out.extend_from_slice(b);
    };
    field(b"");
    field(display_num.as_bytes());
    field(b"MIT-MAGIC-COOKIE-1");
    field(cookie);
    let mut rec = 0xFFFFu16.to_be_bytes().to_vec();
    rec.extend_from_slice(&out);
    rec
}

// ════════════════════════════════════════════════════════════════════════════
//  Отпечаток и допуск
// ════════════════════════════════════════════════════════════════════════════

/// Отпечаток снимка (`<ключ>.json`): совпал целиком — снимок свежий.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Meta {
    pub v: u32,
    /// `xvfb` — невидимый экран; `x11-live` — живое окно.
    pub source: String,
    /// Файлы кода виджета и выбор человека в его настройках.
    pub fp: String,
    /// Обои-подложка ([`backdrop_tag`]).
    pub wallpaper: String,
    /// `ширина×высота@dpi`.
    pub screen: String,
    /// Версия conky.
    pub conky: String,
}

/// Свежий ли снимок: прочитанный отпечаток совпал с нынешним.
pub fn is_fresh(saved: Option<&Meta>, now: &Meta) -> bool {
    saved == Some(now)
}

/// Почему виджет не снимается.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// Нет Xvfb.
    NoXvfb,
    /// Нет conky.
    NoConky,
    /// У виджета своя картинка — снимок не нужен.
    OwnPicture,
    /// Виджет выполняет команды, а разрешения нет (или файлы с тех пор
    /// поменялись): без вопроса человеку его не запустить.
    NeedsPermission,
    /// Не хватает программ или файлов — снимок вышел бы неполным.
    Missing,
}

/// Допуск к снимку по готовым ответам — матрица отдельно от машины.
pub fn eligibility(have_xvfb: bool, have_conky: bool, own_picture: bool, allowed: bool, healthy: bool) -> Result<(), Skip> {
    if !have_xvfb {
        return Err(Skip::NoXvfb);
    }
    if !have_conky {
        return Err(Skip::NoConky);
    }
    if own_picture {
        return Err(Skip::OwnPicture);
    }
    if !allowed {
        return Err(Skip::NeedsPermission);
    }
    if !healthy {
        return Err(Skip::Missing);
    }
    Ok(())
}

/// Главный файл настроек виджета `id` и все его файлы.
fn widget_files(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Vec<String> {
    let primary = if crate::conky_scan::is_found_name(id) { None } else { crate::de_apply::conky_conf(io, ctx, id) };
    crate::conky_scan::widget_confs(io, ctx, id, primary)
}

/// Допуск виджета `id` к снимку на этой машине: семейство пакетов
/// `family` — для проверки недостающих программ. Ничего не ставит и
/// ни о чём не спрашивает.
pub fn check_widget(io: &dyn LookIo, ctx: &Ctx, id: &str, family: &str) -> Result<(), Skip> {
    let confs = widget_files(io, ctx, id);
    let own = crate::conky_scan::widget_picture(io, &crate::conky_scan::widget_path(ctx, id), &confs).is_some();
    let allowed = || crate::conky_safety::start_question(io, ctx, id).is_none();
    let healthy = || crate::conky_safety::health(io, ctx, id, family).is_fine();
    eligibility(io.have("Xvfb"), io.have("conky"), own, true, true)?;
    eligibility(true, true, false, allowed(), healthy())
}

/// Отпечаток кода виджета `id` и выбора человека для файла `conf`.
pub fn fingerprint(io: &dyn LookIo, ctx: &Ctx, id: &str, conf: &str) -> String {
    let code = crate::conky_safety::code_fingerprint(io, ctx, id);
    let settings = io.read(&crate::conky_overlay::live_paths(ctx, conf).settings).unwrap_or_default();
    let mut h = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut h, code.as_bytes());
    sha2::Digest::update(&mut h, [0u8]);
    sha2::Digest::update(&mut h, settings.as_bytes());
    sha2::Digest::finalize(h).iter().map(|b| format!("{b:02x}")).collect()
}

// ════════════════════════════════════════════════════════════════════════════
//  libX11
// ════════════════════════════════════════════════════════════════════════════

/// Все обращения Rexon к libX11 — под одним замком: обработчик ошибок и
/// `XSetAuthorization` у библиотеки общие на процесс.
fn x_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

/// Ошибки X во время снимка (окно закрылось, ещё не показано) не роняют
/// процесс: обработчик libX11 по умолчанию на них завершает программу.
unsafe extern "C" fn quiet_x_error(_: *mut x11_dl::xlib::Display, _: *mut x11_dl::xlib::XErrorEvent) -> std::os::raw::c_int {
    0
}

/// Сырой снимок окна: положение на корне, глубина и BGRA.
struct RawShot {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    depth: u32,
    /// BGRA без хвостов строк.
    bgra: Vec<u8>,
}

/// Снять окно `window` открытого дисплея. Вызывать под [`x_lock`].
///
/// SAFETY: `dpy` — живое подключение `lib`.
unsafe fn grab_raw(lib: &x11_dl::xlib::Xlib, dpy: *mut x11_dl::xlib::Display, window: u64) -> Result<RawShot, String> {
    use x11_dl::xlib;
    let mut attrs: xlib::XWindowAttributes = std::mem::zeroed();
    if (lib.XGetWindowAttributes)(dpy, window, &mut attrs) == 0 {
        return Err("окна нет".to_string());
    }
    if attrs.map_state != xlib::IsViewable || attrs.width <= 0 || attrs.height <= 0 {
        return Err("окно не показано".to_string());
    }
    let (w, h) = (attrs.width as u32, attrs.height as u32);
    let root = (lib.XDefaultRootWindow)(dpy);
    let (mut x, mut y, mut child) = (0, 0, 0);
    if window != root {
        (lib.XTranslateCoordinates)(dpy, window, root, 0, 0, &mut x, &mut y, &mut child);
    }
    let img = (lib.XGetImage)(dpy, window, 0, 0, w, h, !0, xlib::ZPixmap);
    if img.is_null() {
        return Err("снимок не получен".to_string());
    }
    let im = &*img;
    let out = if im.bits_per_pixel == 32 && im.byte_order == xlib::LSBFirst && !im.data.is_null() {
        let stride = im.bytes_per_line as usize;
        let data = std::slice::from_raw_parts(im.data as *const u8, stride * h as usize);
        let mut bgra = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h as usize {
            bgra.extend_from_slice(&data[row * stride..row * stride + w as usize * 4]);
        }
        Ok(RawShot { x, y, width: w, height: h, depth: im.depth as u32, bgra })
    } else {
        Err(format!("неподдержанный формат окна: {} бит", im.bits_per_pixel))
    };
    (lib.XDestroyImage)(img);
    out
}

/// Снять окно `window` дисплея `display`: `(ширина, высота, RGBA)` — для
/// живого пути (PNG с прозрачностью).
pub fn grab_window(display: &str, window: u64) -> Result<(u32, u32, Vec<u8>), String> {
    use x11_dl::xlib;
    let lib = xlib::Xlib::open().map_err(|e| e.to_string())?;
    let name = std::ffi::CString::new(display).map_err(|e| e.to_string())?;
    let _g = x_lock();
    // SAFETY: дисплей открывается и закрывается здесь же; картинка
    // освобождается в `grab_raw`; указатели из функции не уходят.
    unsafe {
        let dpy = (lib.XOpenDisplay)(name.as_ptr());
        if dpy.is_null() {
            return Err(format!("не открыть дисплей {display}"));
        }
        let prev = (lib.XSetErrorHandler)(Some(quiet_x_error));
        let result = grab_raw(&lib, dpy, window).map(|s| {
            let rgba = bgra_to_rgba(&s.bgra, s.width as usize, s.height as usize, s.width as usize * 4, s.depth);
            (s.width, s.height, rgba)
        });
        (lib.XSync)(dpy, 0);
        (lib.XSetErrorHandler)(prev);
        (lib.XCloseDisplay)(dpy);
        result
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Стенд: Xvfb + обои + «композитор»
// ════════════════════════════════════════════════════════════════════════════

/// Размер и плотность экрана стенда.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
}

impl Default for Screen {
    fn default() -> Self {
        Screen { width: 1920, height: 1080, dpi: 96 }
    }
}

impl Screen {
    pub fn label(&self) -> String {
        format!("{}x{}@{}", self.width, self.height, self.dpi)
    }
}

/// `Xft.dpi` из вывода `xrdb -query`.
pub fn parse_xft_dpi(out: &str) -> Option<u32> {
    out.lines()
        .find_map(|l| l.trim().strip_prefix("Xft.dpi:"))
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| *v >= 48.0 && *v <= 480.0)
        .map(|v| v.round() as u32)
}

static GUI_SCREEN: Mutex<Option<Screen>> = Mutex::new(None);

/// Главный экран по словам окна Rexon (физические точки и логическая
/// плотность): на Wayland иначе его не узнать.
pub fn set_screen(width: u32, height: u32, dpi: u32) {
    if width >= 320 && height >= 200 {
        let dpi = if (48..=480).contains(&dpi) { dpi } else { 96 };
        *GUI_SCREEN.lock().unwrap_or_else(|p| p.into_inner()) = Some(Screen { width, height, dpi });
    }
}

/// Экран стенда: на X11 — главный монитор `xrandr` и `Xft.dpi`, иначе —
/// что сказало окно Rexon, иначе 1920×1080 при 96 dpi.
pub fn live_screen(io: &dyn LookIo, ctx: &Ctx) -> Screen {
    let gui = *GUI_SCREEN.lock().unwrap_or_else(|p| p.into_inner());
    let mut s = gui.unwrap_or_default();
    if !ctx.wayland {
        let mons = crate::conky_overlay::monitors(io, false);
        if let Some(m) = mons.iter().find(|m| m.primary).or_else(|| mons.first()) {
            if m.width >= 320 && m.height >= 200 {
                s.width = m.width;
                s.height = m.height;
            }
        }
        if io.have("xrdb") {
            let (ok, out) = io.run(&["xrdb".to_string(), "-query".to_string()]);
            if let Some(d) = ok.then(|| parse_xft_dpi(&out)).flatten() {
                s.dpi = d;
            }
        }
    }
    s
}

fn random_cookie() -> Vec<u8> {
    use std::io::Read;
    let mut buf = vec![0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(&mut buf).is_ok() {
            return buf;
        }
    }
    // Без /dev/urandom — хоть что-то непредсказуемое для чужих.
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mix = t ^ ((std::process::id() as u128) << 64);
    mix.to_le_bytes().to_vec()
}

/// Погасить группу процессов `child`: TERM, секунда, KILL.
fn kill_group(child: &mut std::process::Child) {
    let pid = child.id() as i32;
    // SAFETY: сигнал группе своего ребёнка (группа = его номер).
    unsafe {
        libc::killpg(pid, libc::SIGTERM);
    }
    let until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < until {
        if matches!(child.try_wait(), Ok(Some(_))) {
            // Ребёнок вышел, но в группе могли остаться его дети.
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    unsafe {
        libc::killpg(pid, libc::SIGKILL);
    }
    let _ = child.wait();
}

/// Погасить по номеру все conky с накладкой `launch` — копии, ушедшие из
/// группы процессов.
fn kill_strays(launch: &str) {
    let Ok(rd) = std::fs::read_dir("/proc") else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(pid) = name.parse::<i32>() else { continue };
        let Ok(cmd) = std::fs::read(format!("/proc/{pid}/cmdline")) else { continue };
        let cmd = String::from_utf8_lossy(&cmd);
        if crate::de_apply::conky_launch_of_cmdline(&cmd).as_deref() == Some(launch) {
            // SAFETY: номер только что прочитан, и это наш conky превью.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

/// Невидимый экран со своими обоями. Живёт в одном потоке — потоке
/// очереди; гаснет в `Drop`.
pub struct Stand {
    xvfb: std::process::Child,
    display: String,
    auth: String,
    dir: String,
    lib: x11_dl::xlib::Xlib,
    dpy: *mut x11_dl::xlib::Display,
    backdrop: Backdrop,
    version: Option<(u32, u32)>,
}

impl Stand {
    /// Поднять Xvfb размером `screen`, корнем — `backdrop`, с признаком
    /// композитора.
    pub fn start(screen: Screen, backdrop: Backdrop, version: Option<(u32, u32)>) -> Result<Self, String> {
        use std::os::unix::process::CommandExt;
        use x11_dl::xlib;
        if backdrop.width != screen.width || backdrop.height != screen.height {
            return Err("подложка другого размера".to_string());
        }
        let lib = xlib::Xlib::open().map_err(|e| e.to_string())?;
        let dir = runtime_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        let auth = format!("{dir}/xvfb-{}.auth", std::process::id());
        let cookie = random_cookie();
        write_private(&auth, &auth_record("", &cookie))?;
        let mut fds = [0i32; 2];
        // SAFETY: обычная труба; концы закрываются ниже.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
            return Err("не создать трубу для -displayfd".to_string());
        }
        let (rfd, wfd) = (fds[0], fds[1]);
        let log = std::fs::File::create(format!("{dir}/xvfb.log")).map_err(|e| e.to_string())?;
        let mut cmd = std::process::Command::new("Xvfb");
        cmd.args(["-displayfd", &wfd.to_string(), "-auth", &auth, "-noreset", "-nolisten", "tcp"])
            .args(["-screen", "0", &format!("{}x{}x24", screen.width, screen.height)])
            .args(["-dpi", &screen.dpi.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(log)
            .process_group(0);
        // SAFETY: в ребёнке после fork — только async-signal-safe вызовы.
        unsafe {
            cmd.pre_exec(move || {
                libc::fcntl(wfd, libc::F_SETFD, 0);
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        let spawned = cmd.spawn();
        // SAFETY: свой конец трубы больше не нужен.
        unsafe {
            libc::close(wfd);
        }
        let mut xvfb = match spawned {
            Ok(c) => c,
            Err(e) => {
                unsafe {
                    libc::close(rfd);
                }
                let _ = std::fs::remove_file(&auth);
                return Err(format!("Xvfb не запустился: {e}"));
            }
        };
        let num = read_display_number(rfd, 5000);
        unsafe {
            libc::close(rfd);
        }
        let Some(num) = num else {
            kill_group(&mut xvfb);
            let _ = std::fs::remove_file(&auth);
            return Err("Xvfb не назвал номер дисплея".to_string());
        };
        let display = format!(":{num}");
        if let Err(e) = write_private(&auth, &auth_record(&num, &cookie)) {
            kill_group(&mut xvfb);
            let _ = std::fs::remove_file(&auth);
            return Err(e);
        }
        let mut stand = Stand { xvfb, display, auth, dir, lib, dpy: std::ptr::null_mut(), backdrop, version };
        stand.connect(&cookie)?;
        stand.prepare_root()?;
        Ok(stand)
    }

    /// Номер дисплея стенда (`:N`).
    pub fn display(&self) -> &str {
        &self.display
    }

    /// Файл куки стенда.
    pub fn auth_file(&self) -> &str {
        &self.auth
    }

    fn connect(&mut self, cookie: &[u8]) -> Result<(), String> {
        let name = std::ffi::CString::new(self.display.clone()).map_err(|e| e.to_string())?;
        let mech = b"MIT-MAGIC-COOKIE-1";
        for _ in 0..20 {
            let _g = x_lock();
            // SAFETY: XSetAuthorization копирует данные; сразу после
            // подключения возвращается обычный порядок (XAUTHORITY).
            let dpy = unsafe {
                (self.lib.XSetAuthorization)(
                    mech.as_ptr() as *mut _,
                    mech.len() as i32,
                    cookie.as_ptr() as *mut _,
                    cookie.len() as i32,
                );
                let d = (self.lib.XOpenDisplay)(name.as_ptr());
                (self.lib.XSetAuthorization)(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0);
                d
            };
            if !dpy.is_null() {
                self.dpy = dpy;
                return Ok(());
            }
            drop(_g);
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(format!("не подключиться к {}", self.display))
    }

    /// Обои корнем (+ `_XROOTPMAP_ID`/`ESETROOT_PMAP_ID`) и окно-владелец
    /// `_NET_WM_CM_S0`.
    fn prepare_root(&mut self) -> Result<(), String> {
        use x11_dl::xlib;
        let (w, h) = (self.backdrop.width, self.backdrop.height);
        let mut bgrx: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
        for p in self.backdrop.rgb.chunks_exact(3) {
            bgrx.extend_from_slice(&[p[2], p[1], p[0], 0]);
        }
        let _g = x_lock();
        let lib = &self.lib;
        let dpy = self.dpy;
        // SAFETY: подключение живое; буфер картинки принадлежит Rust и
        // отвязывается от XImage до XDestroyImage.
        unsafe {
            let prev = (lib.XSetErrorHandler)(Some(quiet_x_error));
            let scr = (lib.XDefaultScreen)(dpy);
            let root = (lib.XDefaultRootWindow)(dpy);
            let depth = (lib.XDefaultDepth)(dpy, scr);
            if depth != 24 {
                (lib.XSetErrorHandler)(prev);
                return Err(format!("глубина экрана {depth}, а нужна 24"));
            }
            let visual = (lib.XDefaultVisual)(dpy, scr);
            let pm = (lib.XCreatePixmap)(dpy, root, w, h, 24);
            let gc = (lib.XCreateGC)(dpy, pm, 0, std::ptr::null_mut());
            let img = (lib.XCreateImage)(dpy, visual, 24, xlib::ZPixmap, 0, bgrx.as_mut_ptr() as *mut _, w, h, 32, (w * 4) as i32);
            if img.is_null() {
                (lib.XSetErrorHandler)(prev);
                return Err("картинка обоев не создана".to_string());
            }
            (*img).byte_order = xlib::LSBFirst;
            (lib.XPutImage)(dpy, pm, gc, img, 0, 0, 0, 0, w, h);
            (*img).data = std::ptr::null_mut();
            (lib.XDestroyImage)(img);
            (lib.XFreeGC)(dpy, gc);
            for atom in ["_XROOTPMAP_ID", "ESETROOT_PMAP_ID"] {
                let name = std::ffi::CString::new(atom).unwrap_or_default();
                let a = (lib.XInternAtom)(dpy, name.as_ptr(), 0);
                let id: std::os::raw::c_ulong = pm;
                (lib.XChangeProperty)(dpy, root, a, xlib::XA_PIXMAP, 32, xlib::PropModeReplace, &id as *const _ as *const u8, 1);
            }
            (lib.XSetWindowBackgroundPixmap)(dpy, root, pm);
            (lib.XClearWindow)(dpy, root);
            // «Композитор есть»: conky берёт 32-битное окно с альфой.
            let cm = (lib.XCreateSimpleWindow)(dpy, root, 0, 0, 1, 1, 0, 0, 0);
            let sel = std::ffi::CString::new(format!("_NET_WM_CM_S{scr}")).unwrap_or_default();
            let a = (lib.XInternAtom)(dpy, sel.as_ptr(), 0);
            (lib.XSetSelectionOwner)(dpy, a, cm, 0);
            (lib.XSync)(dpy, 0);
            (lib.XSetErrorHandler)(prev);
        }
        Ok(())
    }

    /// Снять копию виджета: `launch_source` — файл для `dofile` (автор,
    /// перевод 1.9 или правки человека), `work_dir` — папка виджета,
    /// `key` — ключ файла настроек. Готово — плитка RGBA.
    pub fn shoot(&self, launch_source: &str, work_dir: &str, key: &str, update_interval: Option<f64>) -> Result<(u32, u32, Vec<u8>), String> {
        use std::os::unix::process::CommandExt;
        let ov = format!("{}/{key}{PREVIEW_SUFFIX}", self.dir);
        std::fs::write(&ov, preview_overlay(launch_source, self.version)).map_err(|e| e.to_string())?;
        let log = format!("{}/{key}.log", self.dir);
        let log_file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
        let mut cmd = std::process::Command::new("conky");
        cmd.args(["-c", &ov])
            .current_dir(work_dir)
            .env("DISPLAY", &self.display)
            .env("XAUTHORITY", &self.auth)
            .env("XDG_SESSION_TYPE", "x11")
            .env_remove("WAYLAND_DISPLAY")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(log_file)
            .process_group(0);
        // Язык и шрифты — как у человека: исходные LANG/LC_*/LANGUAGE (Rexon
        // мог подменить LANGUAGE под свой интерфейс), остальное окружение
        // (HOME, XDG_CONFIG_HOME, FONTCONFIG_*) наследуется как есть — иначе
        // дни недели и кириллица в виджете выходят квадратами.
        crate::i18n::restore_original_locale_env(&mut cmd);
        // SAFETY: в ребёнке после fork — только prctl.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        let mut child = cmd.spawn().map_err(|e| format!("conky не запустился: {e}"))?;
        let result = self.wait_and_grab(&mut child, &log, settle_ms(update_interval));
        kill_group(&mut child);
        kill_strays(&ov);
        result
    }

    fn wait_and_grab(&self, child: &mut std::process::Child, log: &str, settle: u64) -> Result<(u32, u32, Vec<u8>), String> {
        let started = Instant::now();
        let mut seen: Option<Instant> = None;
        let mut last = String::from("окно conky не появилось");
        loop {
            let text = std::fs::read(log).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
            if let Ok(Some(_)) = child.try_wait() {
                return Err(crate::conky_log::failure_reason(&text).unwrap_or_else(|| "conky закрылся".to_string()));
            }
            let desktop = draws_on_desktop(&text);
            let win = window_of_log(&text);
            if seen.is_none() && (desktop || win.is_some()) {
                seen = Some(Instant::now());
            }
            if seen.is_some_and(|t| t.elapsed() >= Duration::from_millis(settle)) {
                // Ровное окно (одна заливка без текста) принимается под
                // конец ожидания: так бывает и у настоящих виджетов.
                let flat_ok = started.elapsed() + Duration::from_millis(600) >= Duration::from_millis(WAIT_MS);
                let got = if desktop { self.grab_desktop() } else { self.grab_widget(win.unwrap_or(0), flat_ok) };
                match got {
                    Ok(card) => return Ok(card),
                    Err(e) => last = e,
                }
            }
            if started.elapsed() >= Duration::from_millis(WAIT_MS) {
                return Err(last);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Окно виджета поверх обоев.
    fn grab_widget(&self, window: u64, flat_ok: bool) -> Result<(u32, u32, Vec<u8>), String> {
        let shot = {
            let _g = x_lock();
            // SAFETY: подключение стенда живое.
            unsafe {
                let prev = (self.lib.XSetErrorHandler)(Some(quiet_x_error));
                let r = grab_raw(&self.lib, self.dpy, window);
                (self.lib.XSync)(self.dpy, 0);
                (self.lib.XSetErrorHandler)(prev);
                r
            }
        }?;
        // «Не одного цвета» — по сырому окну, до наложения на обои.
        let visible = shot.depth != 32 || shot.bgra.get(..4).is_some_and(|p| p.iter().any(|c| *c != 0));
        if !has_content(&shot.bgra) && !(flat_ok && visible) {
            return Err("виджет ещё ничего не нарисовал".to_string());
        }
        let bg = &self.backdrop;
        // Окно частью за краем экрана — только видимая часть.
        let x0 = shot.x.max(0) as u32;
        let y0 = shot.y.max(0) as u32;
        let x1 = ((shot.x + shot.width as i32).max(0) as u32).min(bg.width);
        let y1 = ((shot.y + shot.height as i32).max(0) as u32).min(bg.height);
        if x1 <= x0 || y1 <= y0 {
            return Err("окно за краем экрана".to_string());
        }
        let (w, h) = (x1 - x0, y1 - y0);
        let mut piece = Vec::with_capacity((w * h * 3) as usize);
        for sy in y0..y1 {
            for sx in x0..x1 {
                let (wx, wy) = ((sx as i32 - shot.x) as u32, (sy as i32 - shot.y) as u32);
                let i = ((wy * shot.width + wx) * 4) as usize;
                piece.extend_from_slice(&over(&shot.bgra[i..i + 4], shot.depth, bg.at(sx, sy)));
            }
        }
        Ok(card(bg, &piece, x0, y0, w, h))
    }

    /// Виджет без своего окна (`own_window = false`) рисует на корне:
    /// рамка — разница корня и обоев.
    fn grab_desktop(&self) -> Result<(u32, u32, Vec<u8>), String> {
        let shot = {
            let _g = x_lock();
            // SAFETY: подключение стенда живое.
            unsafe {
                let prev = (self.lib.XSetErrorHandler)(Some(quiet_x_error));
                let root = (self.lib.XDefaultRootWindow)(self.dpy);
                let r = grab_raw(&self.lib, self.dpy, root);
                (self.lib.XSync)(self.dpy, 0);
                (self.lib.XSetErrorHandler)(prev);
                r
            }
        }?;
        let bg = &self.backdrop;
        if shot.width != bg.width || shot.height != bg.height {
            return Err("корень другого размера".to_string());
        }
        let rgb: Vec<u8> = shot.bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0]]).collect();
        let Some((x, y, w, h)) = diff_bbox(&rgb, &bg.rgb, bg.width, bg.height) else {
            return Err("виджет ещё ничего не нарисовал".to_string());
        };
        let mut piece = Vec::with_capacity((w * h * 3) as usize);
        for sy in y..y + h {
            let i = ((sy * bg.width + x) * 3) as usize;
            piece.extend_from_slice(&rgb[i..i + (w * 3) as usize]);
        }
        Ok(card(bg, &piece, x, y, w, h))
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        if !self.dpy.is_null() {
            let _g = x_lock();
            // SAFETY: подключение закрывается один раз.
            unsafe {
                (self.lib.XCloseDisplay)(self.dpy);
            }
            self.dpy = std::ptr::null_mut();
        }
        kill_group(&mut self.xvfb);
        let _ = std::fs::remove_file(&self.auth);
    }
}

fn write_private(path: &str, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())
}

/// Номер дисплея из `-displayfd`: строка с числом. `None` — не дождались.
fn read_display_number(fd: i32, timeout_ms: i32) -> Option<String> {
    let mut buf = Vec::new();
    let until = Instant::now() + Duration::from_millis(timeout_ms as u64);
    loop {
        let left = until.saturating_duration_since(Instant::now()).as_millis() as i32;
        if left <= 0 {
            return None;
        }
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: один дескриптор, свой буфер.
        let r = unsafe { libc::poll(&mut pfd, 1, left) };
        if r <= 0 {
            return None;
        }
        let mut chunk = [0u8; 16];
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr() as *mut _, chunk.len()) };
        if n <= 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n as usize]);
        if buf.contains(&b'\n') {
            let s = String::from_utf8_lossy(&buf).trim().to_string();
            return (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).then_some(s);
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Снимок одного виджета
// ════════════════════════════════════════════════════════════════════════════

/// Что выполняет копия виджета: накладка с правками человека (если он
/// что-то менял), перевод из 1.9 или сам файл автора — всё в каталоге
/// стенда, накладки Rexon человека не трогаются. `(файл, шаг обновления)`.
pub fn preview_source(io: &dyn LookIo, ctx: &Ctx, conf: &str, dir: &str, version: Option<(u32, u32)>) -> Option<(String, Option<f64>)> {
    use crate::conky_overlay as co;
    let k = co::key(conf);
    let local = co::Paths {
        overlay: format!("{dir}/{k}.settings.lua"),
        settings: String::new(),
        converted: format!("{dir}/{k}{}", co::CONVERTED_SUFFIX),
    };
    let (source, cfg) = co::source_of(io, conf, &local)?;
    let mut interval = cfg.get("update_interval").and_then(|v| v.as_f64());
    let ov = co::load(io, &co::live_paths(ctx, conf));
    if ov.is_empty() {
        return Some((source, interval));
    }
    if let Some(v) = ov.update_interval.filter(|v| *v > 0.0) {
        interval = Some(v);
    }
    let env = co::Env { version, de: ctx.de.clone() };
    io.write(&local.overlay, &co::render(&source, &cfg, &ov, &env)).then_some((local.overlay, interval))
}

/// Снять главный файл виджета `id` на стенде и положить снимок с
/// отпечатком `meta` в кеш. Готово — путь снимка.
pub fn shoot_widget(io: &dyn LookIo, ctx: &Ctx, stand: &Stand, conf: &str, meta: &Meta) -> Result<String, String> {
    let (source, interval) =
        preview_source(io, ctx, conf, &stand.dir, stand.version).ok_or_else(|| "файл настроек не прочитать".to_string())?;
    let key = crate::conky_overlay::key(conf);
    let (w, h, rgba) = stand.shoot(&source, crate::de_apply::conky_work_dir(conf), &key, interval)?;
    let before = cached_one(ctx, conf);
    let out = shot_path(ctx, conf, meta);
    write_png(&out, w, h, &rgba)?;
    let json = serde_json::to_string_pretty(meta).unwrap_or_default();
    if !io.write(&meta_path(ctx, conf), &json) {
        let _ = std::fs::remove_file(&out);
        return Err("отпечаток снимка не записан".to_string());
    }
    // Прежний снимок (и живого окна) больше не нужен.
    for old in [before, Some(cache_path(ctx, conf))].into_iter().flatten() {
        if old != out {
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(out)
}

// ════════════════════════════════════════════════════════════════════════════
//  Очередь
// ════════════════════════════════════════════════════════════════════════════

#[derive(Default)]
struct Queue {
    jobs: VecDeque<(Ctx, String)>,
    running: bool,
    /// Не вышло в этом виде (`файл|отпечаток|обои`) — не пробовать снова.
    failed: HashSet<String>,
}

fn queue() -> &'static Mutex<Queue> {
    static Q: OnceLock<Mutex<Queue>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Queue::default()))
}

static ENABLED: AtomicBool = AtomicBool::new(false);

type Hook = Box<dyn Fn() -> bool + Send>;

fn hooks() -> &'static Mutex<Vec<Hook>> {
    static H: OnceLock<Mutex<Vec<Hook>>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(Vec::new()))
}

/// Включить снимки в фоне (окно Rexon открыло раздел «Виджеты Conky»).
/// До этого [`want`] ничего не делает — проверки и командная строка стенд
/// не поднимают.
pub fn enable() {
    ENABLED.store(true, Ordering::SeqCst);
}

/// Подписаться на «снимки готовы»: `notify` зовётся из потока очереди и
/// возвращает `false`, когда подписчика больше нет.
pub fn watch(notify: Box<dyn Fn() -> bool + Send>) {
    hooks().lock().unwrap_or_else(|p| p.into_inner()).push(notify);
}

fn dispatch() {
    hooks().lock().unwrap_or_else(|p| p.into_inner()).retain(|n| n());
}

/// Поставить виджет `id` в очередь снимков: снимка нет или он устарел —
/// переснять (плитка пока показывает старый). Только для своего дома и
/// только когда снимки включены ([`enable`]).
pub fn want(ctx: &Ctx, id: &str) {
    if !ENABLED.load(Ordering::SeqCst) {
        return;
    }
    let live = std::env::var("HOME").unwrap_or_default();
    if live.is_empty() || live.trim_end_matches('/') != ctx.home.trim_end_matches('/') {
        return;
    }
    let mut q = queue().lock().unwrap_or_else(|p| p.into_inner());
    if q.jobs.iter().any(|(_, j)| j == id) {
        return;
    }
    q.jobs.push_back((ctx.clone(), id.to_string()));
    if !q.running {
        q.running = true;
        drop(q);
        std::thread::Builder::new()
            .name("rexon-conky-preview".into())
            .spawn(worker)
            .map(|_| ())
            .unwrap_or_else(|_| queue().lock().unwrap_or_else(|p| p.into_inner()).running = false);
    }
}

/// Поток очереди: один стенд на всю очередь, виджеты по одному; стенд
/// гаснет после [`STAND_IDLE`] простоя.
fn worker() {
    let io = crate::animations::SystemIo;
    let have = io.have("Xvfb") && io.have("conky");
    let family = crate::conky_safety::live_family().0;
    let version = if have { crate::conky_overlay::conky_version(&io) } else { None };
    let conky = version.map(|(a, b)| format!("{a}.{b}")).unwrap_or_default();
    let mut stand: Option<Stand> = None;
    let mut broken = false;
    // Обои и экран — одни на пачку: узнаются при первой нужде.
    let mut scene: Option<(Screen, Option<String>, String)> = None;
    let mut idle_since = Instant::now();
    let mut fresh_shots = 0u32;
    loop {
        let job = {
            let mut q = queue().lock().unwrap_or_else(|p| p.into_inner());
            match q.jobs.pop_front() {
                Some(j) => Some(j),
                None if stand.is_some() && idle_since.elapsed() < STAND_IDLE => None,
                None => {
                    q.running = false;
                    break;
                }
            }
        };
        let Some((ctx, id)) = job else {
            if fresh_shots > 0 {
                fresh_shots = 0;
                dispatch();
            }
            std::thread::sleep(Duration::from_millis(300));
            continue;
        };
        idle_since = Instant::now();
        if !have || broken || check_widget(&io, &ctx, &id, &family).is_err() {
            continue;
        }
        let Some(conf) = widget_files(&io, &ctx, &id).into_iter().next() else { continue };
        let (screen, wall, tag) = scene.get_or_insert_with(|| {
            let screen = live_screen(&io, &ctx);
            let wall = live_backdrop_image();
            let tag = backdrop_tag(wall.as_deref());
            (screen, wall, tag)
        });
        let meta = Meta {
            v: META_VERSION,
            source: "xvfb".to_string(),
            fp: fingerprint(&io, &ctx, &id, &conf),
            wallpaper: tag.clone(),
            screen: screen.label(),
            conky: conky.clone(),
        };
        let saved: Option<Meta> = io.read(&meta_path(&ctx, &conf)).and_then(|t| serde_json::from_str(&t).ok());
        if is_fresh(saved.as_ref(), &meta) && Path::new(&shot_path(&ctx, &conf, &meta)).is_file() {
            continue;
        }
        let memo = format!("{conf}|{}|{}", meta.fp, meta.wallpaper);
        if queue().lock().unwrap_or_else(|p| p.into_inner()).failed.contains(&memo) {
            continue;
        }
        if stand.is_none() {
            let bd = Backdrop::from_image(wall.as_deref(), screen.width, screen.height);
            match Stand::start(*screen, bd, version) {
                Ok(s) => stand = Some(s),
                Err(e) => {
                    crate::log_warn!("conky", "невидимый экран для снимков виджетов не поднялся: {e}");
                    broken = true;
                    continue;
                }
            }
        }
        let Some(st) = stand.as_ref() else { continue };
        let t = Instant::now();
        match shoot_widget(&io, &ctx, st, &conf, &meta) {
            Ok(p) => {
                crate::log_info!("conky", "снимок виджета {id}: {p} за {} мс", t.elapsed().as_millis());
                fresh_shots += 1;
                if fresh_shots >= 5 {
                    fresh_shots = 0;
                    dispatch();
                }
            }
            Err(why) => {
                crate::log_info!("conky", "снимок виджета {id} не сделан: {why}");
                queue().lock().unwrap_or_else(|p| p.into_inner()).failed.insert(memo);
            }
        }
        idle_since = Instant::now();
    }
    drop(stand);
    if fresh_shots > 0 {
        dispatch();
    }
}

/// Картинка обоев стола для подложки (исполнитель обоев Rexon).
fn live_backdrop_image() -> Option<String> {
    let slot = crate::wallpaper_exec::live_slot();
    crate::wallpaper_exec::with_slot(&slot, |e| e.desktop_backdrop_image())
}

// ════════════════════════════════════════════════════════════════════════════
//  Живое окно (X11 без Xvfb)
// ════════════════════════════════════════════════════════════════════════════

/// Снять виджет файла `conf`, запущенный из Rexon: окно — из журнала,
/// ждать, пока нарисуется, не дольше `wait_ms`. Готово — путь снимка.
pub fn capture(ctx: &Ctx, display: &str, conf: &str, log: &str, wait_ms: u64) -> Result<String, String> {
    let out = cache_path(ctx, conf);
    let mut waited = 0;
    let mut last = String::from("окно conky не найдено в журнале");
    loop {
        let text = std::fs::read(log).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        if let Some(win) = window_of_log(&text) {
            match grab_window(display, win) {
                Ok((w, h, rgba)) if has_content(&rgba) => {
                    write_png(&out, w, h, &rgba)?;
                    return Ok(out);
                }
                Ok(_) => last = "виджет ещё ничего не нарисовал".to_string(),
                Err(e) => last = e,
            }
        }
        if waited >= wait_ms {
            return Err(last);
        }
        std::thread::sleep(Duration::from_millis(STEP_MS));
        waited += STEP_MS;
    }
}

/// После удачного «Запустить»: снять живое окно в фоне — только запасной
/// путь X11 без Xvfb (с ним снимает невидимый экран), и только если
/// снимка ещё нет. Окно Rexon не ждёт.
pub fn capture_later(ctx: &Ctx, conf: &str, log: &str) {
    let display = std::env::var("DISPLAY").ok();
    if !can_capture(ctx, display.as_deref())
        || crate::utils::which("Xvfb", std::env::var("PATH").ok().as_deref()).is_some()
        || cached_one(ctx, conf).is_some()
    {
        return;
    }
    let (ctx, conf, log, display) = (ctx.clone(), conf.to_string(), log.to_string(), display.unwrap_or_default());
    std::thread::spawn(move || match capture(&ctx, &display, &conf, &log, WAIT_MS) {
        Ok(p) => crate::log_info!("conky", "снимок виджета: {p}"),
        Err(why) => crate::log_info!("conky", "снимок виджета {conf} не сделан: {why}"),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_read_from_the_log() {
        let log = "[2026-10-24 10:00:00.1] [info ] [src/output/x11.cc:1016] drawing to created window 0x1a00002\n\
                   [2026-10-24 10:00:05.1] [info ] [src/output/x11.cc:1016] drawing to created window 0x1c00002\n";
        assert_eq!(window_of_log(log), Some(0x1c00002));
        assert_eq!(window_of_log("drawing to desktop window\n"), None);
        assert!(draws_on_desktop("[info] drawing to desktop window\n"));
        assert_eq!(window_of_log(""), None);
    }

    #[test]
    fn pixels_become_straight_rgba() {
        // BGRA: синий непрозрачный и предумноженный белый с альфой 128.
        let data = [255u8, 0, 0, 255, 128, 128, 128, 128];
        assert_eq!(bgra_to_rgba(&data, 2, 1, 8, 32), [0, 0, 255, 255, 255, 255, 255, 128]);
        assert_eq!(bgra_to_rgba(&data, 2, 1, 8, 24)[7], 255);
        assert!(has_content(&[0, 0, 0, 0, 1, 0, 0, 0]));
        assert!(!has_content(&[7, 7, 7, 7, 7, 7, 7, 7]));
    }

    /// Дефект a3d6898c: пиксель «цвет больше альфы» (обводка `${cpubar}`)
    /// выходил прозрачным при альфе 0 и пересвеченным при малой.
    #[test]
    fn colour_above_alpha_is_kept() {
        // Красный 255 при альфе 0 — композитор показывает красный.
        assert_eq!(straight_rgba(0, 0, 255, 0), [255, 0, 0, 255]);
        // Серый 100 при альфе 40 — альфа поднимается до 100, цвет белый.
        assert_eq!(straight_rgba(100, 100, 100, 40), [255, 255, 255, 100]);
        // Обычный предумноженный — как раньше.
        assert_eq!(straight_rgba(0, 0, 0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn over_composes_like_a_compositor() {
        let under = [200, 100, 50];
        // Альфа 0, цвет 0 — подложка.
        assert_eq!(over(&[0, 0, 0, 0], 32, under), under);
        // Альфа 0, цвет 255 — сложение, обводка видна.
        assert_eq!(over(&[255, 255, 255, 0], 32, under), [255, 255, 255]);
        // Полупрозрачный синий #0000ff80 на красном ≈ (127, 0, 128).
        assert_eq!(over(&[128, 0, 0, 128], 32, [255, 0, 0]), [127, 0, 128]);
        // Непрозрачный — сам цвет; глубина 24 — альфа не смотрится.
        assert_eq!(over(&[1, 2, 3, 255], 32, under), [3, 2, 1]);
        assert_eq!(over(&[1, 2, 3, 0], 24, under), [3, 2, 1]);
    }

    #[test]
    fn frame_is_the_difference_with_the_wallpaper() {
        let base = vec![10u8; 4 * 3 * 3];
        let mut now = base.clone();
        // Точки (1,1) и (2,2) изменились.
        for (x, y) in [(1u32, 1u32), (2, 2)] {
            let i = ((y * 4 + x) * 3) as usize;
            now[i] = 99;
        }
        assert_eq!(diff_bbox(&now, &base, 4, 3), Some((1, 1, 2, 2)));
        assert_eq!(diff_bbox(&base, &base, 4, 3), None);
    }

    #[test]
    fn card_has_margins_and_fits_the_tile() {
        let bg = Backdrop::fill(1000, 800);
        let piece = vec![255u8; 100 * 50 * 3];
        let (w, h, px) = card(&bg, &piece, 5, 300, 100, 50);
        // Слева поле упирается в край экрана: 5 + 100 + 16.
        assert_eq!((w, h), (121, 82));
        assert_eq!(&px[..3], &FILL_RGB);
        let big = vec![0u8; 900 * 300 * 3];
        let (w, h, _) = card(&bg, &big, 50, 50, 900, 300);
        assert_eq!(w, CARD_MAX);
        assert!(h < 300, "{h}");
    }

    #[test]
    fn preview_overlay_keeps_the_copy_off_the_real_desktop() {
        let t = preview_overlay("/h/w/conky.conf", Some((1, 25)));
        assert!(t.contains("dofile(\"/h/w/conky.conf\")"), "{t}");
        assert!(t.contains("c.background = false") && t.contains("c.xinerama_head = nil"));
        assert!(t.contains("c.output_backend = 'x11'") && !t.contains("out_to_wayland"));
        let old = preview_overlay("/h/w/x.v110.lua", Some((1, 19)));
        assert!(old.contains("c.out_to_x = true") && old.contains("c.out_to_wayland = false"));
        assert!(!old.contains("output_backend"));
        let unknown = preview_overlay("/x", None);
        assert!(unknown.contains("output_backend") && unknown.contains("out_to_wayland = false"));
    }

    #[test]
    fn settle_follows_update_interval() {
        assert_eq!(settle_ms(None), 2000);
        assert_eq!(settle_ms(Some(0.1)), 500);
        assert_eq!(settle_ms(Some(1.0)), 2000);
        assert_eq!(settle_ms(Some(10.0)), 3000);
        assert_eq!(settle_ms(Some(-1.0)), 2000);
    }

    #[test]
    fn auth_record_bytes() {
        let r = auth_record("7", &[0xAB, 0xCD]);
        let want: Vec<u8> = [
            &[0xFF, 0xFF][..],
            &[0, 0],
            &[0, 1, b'7'],
            &[0, 18],
            b"MIT-MAGIC-COOKIE-1",
            &[0, 2, 0xAB, 0xCD],
        ]
        .concat();
        assert_eq!(r, want);
        assert_eq!(&auth_record("", &[1])[4..6], &[0, 0], "пустой номер — любой дисплей");
    }

    #[test]
    fn preview_processes_are_recognised() {
        assert!(is_preview_launch("/run/user/1000/rexon/conky-preview/h+sw+sconky.conf.preview.conf"));
        assert!(!is_preview_launch("/h/.local/state/rexon/conky/overlay/h+sw.rexon.conf"));
        assert!(!is_preview_launch("/h/.config/conky/w/conky.conf"));
        assert!(runtime_dir().ends_with("/rexon/conky-preview"));
    }

    #[test]
    fn freshness_compares_every_field() {
        let m = Meta {
            v: META_VERSION,
            source: "xvfb".into(),
            fp: "a".into(),
            wallpaper: "fill".into(),
            screen: "1920x1080@96".into(),
            conky: "1.25".into(),
        };
        assert!(is_fresh(Some(&m), &m));
        assert!(!is_fresh(None, &m));
        for change in [
            Meta { fp: "b".into(), ..m.clone() },
            Meta { wallpaper: "/w.jpg|1|2".into(), ..m.clone() },
            Meta { screen: "2560x1440@96".into(), ..m.clone() },
            Meta { source: "x11-live".into(), ..m.clone() },
        ] {
            assert!(!is_fresh(Some(&change), &m), "{change:?}");
        }
    }

    #[test]
    fn eligibility_matrix() {
        assert_eq!(eligibility(true, true, false, true, true), Ok(()));
        assert_eq!(eligibility(false, true, false, true, true), Err(Skip::NoXvfb));
        assert_eq!(eligibility(true, false, false, true, true), Err(Skip::NoConky));
        assert_eq!(eligibility(true, true, true, true, true), Err(Skip::OwnPicture));
        assert_eq!(eligibility(true, true, false, false, true), Err(Skip::NeedsPermission));
        assert_eq!(eligibility(true, true, false, true, false), Err(Skip::Missing));
    }

    /// Машина с подставным PATH: файлы настоящие, программы — по списку.
    struct Io {
        have: Vec<&'static str>,
    }

    impl LookIo for Io {
        fn run(&self, _: &[String]) -> (bool, String) {
            (false, String::new())
        }
        fn have(&self, tool: &str) -> bool {
            self.have.contains(&tool)
        }
        fn read(&self, path: &str) -> Option<String> {
            crate::animations::SystemIo.read(path)
        }
        fn write(&self, path: &str, text: &str) -> bool {
            crate::animations::SystemIo.write(path, text)
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            crate::animations::SystemIo.list_dir(path)
        }
    }

    /// Виджет как Jhalak: Lua с `io.popen('cava …')` через переменную.
    /// Без разрешения — не снимается; разрешён, но cava нет — тоже (health
    /// видит программу и в собранной команде); всё есть — снимается.
    #[test]
    fn check_widget_needs_permission_and_programs() {
        let d = stand_dir("check");
        let home = d.join("home");
        let w = home.join(".config/conky/eq");
        std::fs::create_dir_all(&w).unwrap();
        std::fs::write(w.join("eq.conf"), "conky.config = { lua_load = 'visualizer.lua' }\nconky.text = [[x]]\n").unwrap();
        std::fs::write(
            w.join("visualizer.lua"),
            "local CONFIG_FILE = './cava.conf'\nlocal cmd = 'cava -p ' .. CONFIG_FILE .. ' 2>/dev/null'\np = io.popen(cmd, 'r')\n",
        )
        .unwrap();
        std::fs::write(w.join("cava.conf"), "[general]\n").unwrap();
        let ctx = Ctx { home: home.to_string_lossy().into_owned(), xdg_config_home: None, wayland: true, de: String::new() };
        let without = Io { have: vec!["Xvfb", "conky"] };
        assert_eq!(check_widget(&without, &ctx, "eq", "arch"), Err(Skip::NeedsPermission));
        assert!(crate::conky_safety::remember(&without, &ctx, "eq"));
        assert_eq!(check_widget(&without, &ctx, "eq", "arch"), Err(Skip::Missing));
        let with = Io { have: vec!["Xvfb", "conky", "cava"] };
        assert_eq!(check_widget(&with, &ctx, "eq", "arch"), Ok(()));
        assert_eq!(check_widget(&Io { have: vec!["conky", "cava"] }, &ctx, "eq", "arch"), Err(Skip::NoXvfb));
        // Своя картинка — снимок не нужен.
        std::fs::write(w.join("eq.png"), b"p").unwrap();
        assert_eq!(check_widget(&with, &ctx, "eq", "arch"), Err(Skip::OwnPicture));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn xft_dpi_is_read() {
        assert_eq!(parse_xft_dpi("Xft.antialias:\t1\nXft.dpi:\t120\n"), Some(120));
        assert_eq!(parse_xft_dpi("Xft.dpi: 96.0"), Some(96));
        assert_eq!(parse_xft_dpi("Xft.hinting: 1"), None);
    }

    #[test]
    fn wayland_and_no_display_are_not_captured_live() {
        let mut ctx = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        assert!(can_capture(&ctx, Some(":0")));
        assert!(!can_capture(&ctx, None));
        ctx.wayland = true;
        assert!(!can_capture(&ctx, Some(":0")));
    }

    // ── Стенды: настоящие Xvfb и conky, экран человека не трогается ──────

    fn have_stand() -> bool {
        let have = |p: &str| crate::utils::which(p, None).is_some();
        if !have("Xvfb") || !have("conky") {
            eprintln!("НЕ ПРОВЕРЕНО: нет Xvfb или conky");
            return false;
        }
        true
    }

    fn stand_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-conky-preview-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn red_backdrop(w: u32, h: u32) -> Backdrop {
        Backdrop { width: w, height: h, rgb: [255u8, 0, 0].iter().copied().cycle().take((w * h * 3) as usize).collect(), tag: "red".into() }
    }

    fn widget_conf(dir: &Path, name: &str, body: &str) -> String {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().into_owned()
    }

    fn version() -> Option<(u32, u32)> {
        crate::conky_overlay::conky_version(&crate::animations::SystemIo)
    }

    /// Пиксель плитки в точке (x, y) — RGB.
    fn px(card: &(u32, u32, Vec<u8>), x: u32, y: u32) -> [u8; 3] {
        let i = ((y * card.0 + x) * 4) as usize;
        [card.2[i], card.2[i + 1], card.2[i + 2]]
    }

    fn keep(name: &str, c: &(u32, u32, Vec<u8>)) {
        if let Ok(dir) = std::env::var("REXON_STAND_KEEP") {
            let _ = write_png(&format!("{dir}/{name}.png"), c.0, c.1, &c.2);
        }
    }

    /// Нет ли живых процессов с `needle` в командной строке.
    fn alive_with(needle: &str) -> Vec<String> {
        let mut out = Vec::new();
        for e in std::fs::read_dir("/proc").unwrap().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.parse::<u32>().is_err() {
                continue;
            }
            if let Ok(c) = std::fs::read(format!("/proc/{n}/cmdline")) {
                if String::from_utf8_lossy(&c).contains(needle) {
                    out.push(n);
                }
            }
        }
        out
    }

    /// Полупрозрачный `#0000ff80` на красной подложке ≈ (127, 0, 128);
    /// прозрачный — подложка; `own_window = false` — рамка по разнице;
    /// `background = true` не убегает; после стенда — ни Xvfb, ни conky.
    /// Запуск: `cargo test -p rexon-core --lib conky_preview -- --ignored`.
    #[test]
    #[ignore = "стенд: нужны Xvfb и conky"]
    fn stand_shoots_widgets_on_an_invisible_screen() {
        if !have_stand() {
            return;
        }
        let d = stand_dir("stand");
        let common = "use_xft = true, font = 'DejaVu Sans:size=14', update_interval = 1, \
                      alignment = 'top_left', gap_x = 40, gap_y = 40, minimum_width = 200, minimum_height = 80, \
                      default_color = 'white', own_window_type = 'normal'";
        let half = widget_conf(
            &d,
            "half.conf",
            &format!("conky.config = {{ {common}, own_window = true, own_window_color = '#800000ff' }}\nconky.text = [[${{color white}}ПОЛУПРОЗРАЧНЫЙ]]\n"),
        );
        let transp = widget_conf(
            &d,
            "transp.conf",
            &format!("conky.config = {{ {common}, own_window = true, own_window_color = '#00000000' }}\nconky.text = [[${{color white}}ПРОЗРАЧНЫЙ]]\n"),
        );
        let root = widget_conf(
            &d,
            "root.conf",
            &format!("conky.config = {{ {common}, own_window = false, background = true }}\nconky.text = [[НА КОРНЕ ${{time %A}}]]\n"),
        );
        let stand = Stand::start(Screen { width: 800, height: 600, dpi: 96 }, red_backdrop(800, 600), version()).unwrap();
        let display = stand.display().to_string();
        let work = d.to_string_lossy().into_owned();

        let c = stand.shoot(&half, &work, "half", Some(1.0)).unwrap_or_else(|e| panic!("полупрозрачный: {e}"));
        keep("half", &c);
        // Середина окна: синий 50 % на красном.
        let mid = px(&c, c.0 / 2, c.1 / 2);
        assert!(mid[0].abs_diff(127) <= 2 && mid[1] <= 2 && mid[2].abs_diff(128) <= 2, "{mid:?}");
        // Поле вокруг — красные обои.
        assert_eq!(px(&c, 2, 2), [255, 0, 0]);

        let c = stand.shoot(&transp, &work, "transp", Some(1.0)).unwrap_or_else(|e| panic!("прозрачный: {e}"));
        keep("transp", &c);
        assert_eq!(px(&c, 2, 2), [255, 0, 0]);
        assert_eq!(px(&c, c.0 - 3, c.1 - 3), [255, 0, 0], "прозрачный фон — подложка");

        let c = stand.shoot(&root, &work, "root", Some(1.0)).unwrap_or_else(|e| panic!("на корне: {e}"));
        keep("root", &c);
        assert!(c.0 < 600 && c.1 < 200, "рамка по разнице, не весь экран: {}×{}", c.0, c.1);

        let auth = stand.auth_file().to_string();
        drop(stand);
        std::thread::sleep(Duration::from_millis(300));
        for key in ["half", "transp", "root"] {
            let ov = format!("{}/{key}{PREVIEW_SUFFIX}", runtime_dir());
            assert!(alive_with(&ov).is_empty(), "conky {key} не погашен");
        }
        assert!(alive_with(&auth).is_empty(), "Xvfb {display} не погашен");
        assert!(!Path::new(&auth).exists(), "кука не убрана");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Очередь целиком: виджет «найден на диске» (имя — путь папки) без
    /// картинки → снимок в кеше с отпечатком → оповещение; второй раз —
    /// свежий, не переснимается. Кеш — настоящий `~/.cache/rexon`, файлы
    /// проверки убираются в конце.
    #[test]
    #[ignore = "стенд: нужны Xvfb и conky; пишет в ~/.cache/rexon/conky-previews"]
    fn stand_queue_shoots_and_notifies() {
        if !have_stand() {
            return;
        }
        let d = stand_dir("queue");
        let w = d.join("QueueStand");
        std::fs::create_dir_all(&w).unwrap();
        let conf = widget_conf(
            &w,
            "conky.conf",
            "conky.config = { own_window = true, own_window_type = 'normal', own_window_color = '#C0203040', \
             minimum_width = 220, minimum_height = 60, use_xft = true, font = 'DejaVu Sans:size=12', \
             default_color = 'white', update_interval = 1 }\nconky.text = [[ОЧЕРЕДЬ ${time %A}]]\n",
        );
        let id = w.to_string_lossy().into_owned();
        let ctx = Ctx::live();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let tx = Mutex::new(tx);
        watch(Box::new(move || tx.lock().map(|t| t.send(()).is_ok()).unwrap_or(false)));
        enable();
        want(&ctx, &id);
        rx.recv_timeout(Duration::from_secs(40)).expect("оповещения о снимке нет");
        let shot = cached(&ctx, std::slice::from_ref(&conf)).expect("снимка нет в кеше");
        eprintln!("снимок очереди: {shot}");
        if let Ok(dir) = std::env::var("REXON_STAND_KEEP") {
            let _ = std::fs::copy(&shot, format!("{dir}/queue.png"));
        }
        let meta: Meta = serde_json::from_str(&std::fs::read_to_string(meta_path(&ctx, &conf)).unwrap()).unwrap();
        assert_eq!(meta.source, "xvfb");
        // Второй раз — свежий: оповещения нет.
        want(&ctx, &id);
        assert!(rx.recv_timeout(Duration::from_secs(4)).is_err(), "свежий снимок переснят");
        let _ = std::fs::remove_file(&shot);
        let _ = std::fs::remove_file(meta_path(&ctx, &conf));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Настоящий виджет человека (`REXON_STAND_WIDGET` — его файл настроек,
    /// например ~/.config/conky/Jhalak/Jhalak.conf) на его обоях
    /// (`REXON_STAND_WALL`): глазами смотреть кириллицу и дни недели.
    #[test]
    #[ignore = "стенд: нужны Xvfb, conky и REXON_STAND_WIDGET"]
    fn stand_shoots_a_real_widget() {
        if !have_stand() {
            return;
        }
        let Ok(conf) = std::env::var("REXON_STAND_WIDGET") else {
            eprintln!("НЕ ПРОВЕРЕНО: не задан REXON_STAND_WIDGET");
            return;
        };
        let wall = std::env::var("REXON_STAND_WALL").ok();
        let screen = Screen::default();
        let bd = Backdrop::from_image(wall.as_deref(), screen.width, screen.height);
        let stand = Stand::start(screen, bd, version()).unwrap();
        let io = crate::animations::SystemIo;
        let ctx = Ctx::live();
        let (source, interval) = preview_source(&io, &ctx, &conf, &stand.dir, stand.version).expect("файл настроек");
        let t = Instant::now();
        let c = stand
            .shoot(&source, crate::de_apply::conky_work_dir(&conf), &crate::conky_overlay::key(&conf), interval)
            .unwrap_or_else(|e| panic!("снимок не сделан: {e}"));
        eprintln!("снимок {}×{} за {} мс", c.0, c.1, t.elapsed().as_millis());
        keep("real", &c);
        assert!(c.0 > 2 * CARD_MARGIN && c.1 > 2 * CARD_MARGIN);
    }
}
