//! Курсоры Windows (`.cur` / `.ani`) → Xcursor: разбор и запись своим кодом.
//!
//! ЗАЧЕМ СВОЙ КОД. Рабочий стол Linux (KWin, Mutter, Hyprland, X-сервер)
//! рисует курсор сам и понимает только Xcursor; «проигрывателя» `.cur`/`.ani`
//! у него нет, поэтому перевод обязателен. Прежний путь держался на чужих
//! программах (`win2xcur` из pip или `convert` + `xcursorgen`) и брал ТОЛЬКО
//! первый кадр (`convert file[0]`): анимированный курсор становился
//! неподвижным, а без `xcursorgen` (его нет в большинстве систем) импорт не
//! работал вовсе. Здесь всё — на Rust, без внешних программ.
//!
//! Форматы (по документации Microsoft и собственному разбору файлов):
//!   * `.cur`/`.ico` — каталог ICONDIR: 6 байт заголовка (0, тип 1/2, число
//!     записей) + записи по 16 байт; у курсора (тип 2) поля «плоскости/биты»
//!     записи — это горячая точка X/Y. Картинка записи — либо PNG целиком,
//!     либо DIB (BITMAPINFOHEADER, высота удвоена: XOR-картинка + AND-маска).
//!   * `.ani` — RIFF `ACON`: `anih` (36 байт: число кадров, число шагов,
//!     частота по умолчанию в 1/60 с, флаги), `rate` (задержка каждого шага),
//!     `seq ` (номер кадра каждого шага), `LIST fram` → чанки `icon`, каждый
//!     — целый `.cur`/`.ico` со своими горячими точками; `LIST INFO` →
//!     `INAM` (название), `IART` (автор).
//!   * Xcursor — `Xcur`, оглавление, картинки `0xfffd0002` с номинальным
//!     размером, горячей точкой, задержкой в мс и пикселями ARGB с
//!     предумноженной альфой (libXcursor берёт ВСЕ картинки ближайшего
//!     номинала по порядку — это и есть анимация).
//!
//! Размеры. Курсор Windows любого холста (32…256 px) показывается в
//! номинале 32 при 100 % масштаба, поэтому холст приводится к номиналам
//! 24/32/48 (+64/96 для HiDPI, если исходник не меньше). Уменьшение —
//! Ланцош по предумноженной альфе (без тёмной каймы); увеличение — сначала
//! точное целое повторение пикселей, и лишь остаток — Ланцошем, чтобы
//! мелкий пиксель-арт не «замыливался».
//!
//! Роли. Файл набора знает роль только по описателю: `install.inf`
//! (строки реестра `HKCU,"Control Panel\Cursors",Arrow,…` и список схемы
//! `…\Cursors\Schemes` в фиксированном порядке Windows) или `.crs`
//! (RW-Designer: `[Arrow] Path=…`). Имена файлов — только запасной путь.

use std::collections::HashMap;

/// Предел входного файла курсора (больше — не курсор, а мусор/бомба).
pub const MAX_INPUT_BYTES: usize = 16 << 20;
/// Предел стороны картинки кадра (у Windows — 256).
const MAX_SIDE: u32 = 512;
/// Пределы записей каталога, кадров и шагов анимации.
const MAX_ENTRIES: usize = 64;
const MAX_FRAMES: usize = 1024;
const MAX_STEPS: usize = 4096;
/// Бюджет одного файла Xcursor: при превышении сначала отпадают 96, потом 64.
pub const XCURSOR_BUDGET: usize = 8 << 20;

// ============================================================================
// Чтение чисел
// ============================================================================

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o.checked_add(2)?).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o.checked_add(4)?)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn i32_at(b: &[u8], o: usize) -> Option<i32> {
    u32_at(b, o).map(|v| v as i32)
}

// ============================================================================
// Картинка курсора и анимация
// ============================================================================

/// Одна картинка курсора: RGBA с прямой (не предумноженной) альфой.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurImage {
    pub width: u32,
    pub height: u32,
    pub hot_x: u32,
    pub hot_y: u32,
    pub rgba: Vec<u8>,
}

/// Шаг анимации: какой кадр и сколько его держать.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub frame: usize,
    pub delay_ms: u32,
}

/// Разобранный курсор: кадры (у каждого — все размеры из файла) и шаги.
/// У неподвижного `.cur` — один кадр и один шаг.
#[derive(Clone, Debug, Default)]
pub struct Animation {
    pub frames: Vec<Vec<CurImage>>,
    pub steps: Vec<Step>,
    /// `INAM` из `.ani`, если есть.
    pub title: Option<String>,
    /// `IART` из `.ani`, если есть.
    pub artist: Option<String>,
}

impl Animation {
    /// Анимирован ли курсор (больше одного шага).
    pub fn is_animated(&self) -> bool {
        self.steps.len() > 1
    }

    /// Наибольшая сторона картинок во всех кадрах.
    pub fn max_side(&self) -> u32 {
        self.frames
            .iter()
            .flat_map(|f| f.iter())
            .map(|i| i.width.max(i.height))
            .max()
            .unwrap_or(0)
    }
}

/// Разобрать файл курсора по содержимому (`RIFF…ACON` — `.ani`, иначе
/// каталог `.cur`/`.ico`). Расширению файла не верим: в наборах встречаются
/// `.cur`, переименованные из `.ani`.
pub fn parse_cursor_file(data: &[u8]) -> Result<Animation, String> {
    if data.len() > MAX_INPUT_BYTES {
        return Err(format!("file is too large ({} bytes)", data.len()));
    }
    if data.get(0..4) == Some(b"RIFF") {
        return parse_ani(data);
    }
    let images = parse_icondir(data)?;
    Ok(Animation {
        frames: vec![images],
        steps: vec![Step { frame: 0, delay_ms: 0 }],
        title: None,
        artist: None,
    })
}

// ============================================================================
// ICONDIR (.cur / .ico)
// ============================================================================

/// Горячая точка записи ICO-типа (тип 1). В «иконке» поля записи — это
/// плоскости и биты на пиксель; но редакторы `.ani` порой пишут тип 1 и
/// кладут туда настоящую горячую точку. Похоже на плоскости/биты — (0, 0)
/// (левый верхний угол, как у стрелки), иначе — поля как точка.
fn icon_entry_hotspot(f1: u16, f2: u16, w: u32, h: u32) -> (u32, u32) {
    let looks_like_planes_bpp =
        f1 <= 1 && matches!(f2, 0 | 1 | 2 | 4 | 8 | 16 | 24 | 32);
    if !looks_like_planes_bpp && (f1 as u32) < w && (f2 as u32) < h {
        (f1 as u32, f2 as u32)
    } else {
        (0, 0)
    }
}

/// Разобрать каталог `.cur`/`.ico`: все записи, которые удалось прочесть.
pub fn parse_icondir(data: &[u8]) -> Result<Vec<CurImage>, String> {
    let (Some(reserved), Some(kind), Some(count)) =
        (u16_at(data, 0), u16_at(data, 2), u16_at(data, 4))
    else {
        return Err("file is too short for a cursor".into());
    };
    if reserved != 0 || !(kind == 1 || kind == 2) {
        return Err("not a Windows cursor or icon".into());
    }
    if count == 0 {
        return Err("cursor has no images".into());
    }
    let mut out = Vec::new();
    let mut last_err = String::from("no readable images");
    for i in 0..(count as usize).min(MAX_ENTRIES) {
        let e = 6 + i * 16;
        let Some(entry) = data.get(e..e + 16) else { break };
        let ew = if entry[0] == 0 { 256 } else { entry[0] as u32 };
        let eh = if entry[1] == 0 { 256 } else { entry[1] as u32 };
        let f1 = u16_at(entry, 4).unwrap_or(0);
        let f2 = u16_at(entry, 6).unwrap_or(0);
        let size = u32_at(entry, 8).unwrap_or(0) as usize;
        let start = u32_at(entry, 12).unwrap_or(0) as usize;
        if start >= data.len() {
            last_err = "image offset is outside the file".into();
            continue;
        }
        let end = start.saturating_add(size).min(data.len());
        match decode_entry(&data[start..end], eh) {
            Ok((w, h, rgba)) => {
                let (mut hx, mut hy) = if kind == 2 {
                    (f1 as u32, f2 as u32)
                } else {
                    icon_entry_hotspot(f1, f2, ew, eh)
                };
                // Запись врёт о размере (бывает у самодельных наборов) —
                // точку переносим пропорционально настоящей картинке.
                if w != ew {
                    hx = hx * w / ew;
                }
                if h != eh {
                    hy = hy * h / eh;
                }
                out.push(CurImage {
                    width: w,
                    height: h,
                    hot_x: hx.min(w - 1),
                    hot_y: hy.min(h - 1),
                    rgba,
                });
            }
            Err(e) => last_err = e,
        }
    }
    if out.is_empty() {
        return Err(last_err);
    }
    Ok(out)
}

/// Картинка записи: PNG целиком или DIB с AND-маской.
fn decode_entry(blob: &[u8], entry_h: u32) -> Result<(u32, u32, Vec<u8>), String> {
    if blob.starts_with(b"\x89PNG\r\n\x1a\n") {
        let img = image::load_from_memory_with_format(blob, image::ImageFormat::Png)
            .map_err(|e| format!("PNG image is damaged: {e}"))?
            .to_rgba8();
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
            return Err(format!("unsupported image size {w}x{h}"));
        }
        return Ok((w, h, img.into_raw()));
    }
    decode_dib(blob, entry_h)
}

/// DIB записи иконки: 1/4/8 бит с палитрой, 16/24/32 бита; строки снизу
/// вверх; за XOR-картинкой — AND-маска 1 бит (1 = прозрачно).
fn decode_dib(b: &[u8], entry_h: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let hs = u32_at(b, 0).ok_or("bitmap header is missing")? as usize;
    if !(40..=b.len()).contains(&hs) {
        return Err("unsupported bitmap header".into());
    }
    let w = i32_at(b, 4).unwrap_or(0);
    let h2 = i32_at(b, 8).unwrap_or(0);
    if w <= 0 || w as u32 > MAX_SIDE {
        return Err(format!("unsupported image width {w}"));
    }
    let w = w as u32;
    let top_down = h2 < 0;
    let h2a = h2.unsigned_abs();
    // Обычно высота удвоена (картинка + маска); у редких писателей — нет.
    let (h, has_mask) = if h2a == entry_h && h2a != 2 * entry_h {
        (h2a, false)
    } else {
        (h2a / 2, true)
    };
    if h == 0 || h > MAX_SIDE {
        return Err(format!("unsupported image height {h}"));
    }
    let bpp = u16_at(b, 14).unwrap_or(0) as u32;
    let compression = u32_at(b, 16).unwrap_or(0);
    let clr_used = u32_at(b, 32).unwrap_or(0);
    if compression != 0 && compression != 3 {
        return Err("compressed bitmaps are not supported".into());
    }
    let mut pos = hs;
    let mut green_mask = 0u32;
    if compression == 3 {
        if hs >= 52 {
            green_mask = u32_at(b, 44).unwrap_or(0);
        } else {
            green_mask = u32_at(b, pos + 4).unwrap_or(0);
            pos += 12;
        }
    }
    let mut palette: Vec<[u8; 3]> = Vec::new();
    if bpp <= 8 {
        if !matches!(bpp, 1 | 4 | 8) {
            return Err(format!("unsupported color depth {bpp}"));
        }
        let n = if clr_used > 0 { clr_used.min(256) } else { 1 << bpp } as usize;
        for i in 0..n {
            let Some(c) = b.get(pos + i * 4..pos + i * 4 + 3) else { break };
            palette.push([c[2], c[1], c[0]]);
        }
        pos += n * 4;
    } else if !matches!(bpp, 16 | 24 | 32) {
        return Err(format!("unsupported color depth {bpp}"));
    }
    let (wu, hu) = (w as usize, h as usize);
    let stride = (wu * bpp as usize).div_ceil(32) * 4;
    let xor = b
        .get(pos..pos + stride * hu)
        .ok_or("bitmap data is truncated")?;
    pos += stride * hu;
    let mstride = wu.div_ceil(32) * 4;
    let mask = if has_mask { b.get(pos..pos + mstride * hu) } else { None };

    let mut rgba = vec![0u8; wu * hu * 4];
    let mut any_alpha = false;
    for y in 0..hu {
        let row = if top_down { y } else { hu - 1 - y };
        let src = &xor[row * stride..row * stride + stride];
        for x in 0..wu {
            let (r, g, bl, a) = match bpp {
                1 | 4 | 8 => {
                    let bit = x * bpp as usize;
                    let byte = src[bit / 8];
                    let shift = 8 - bpp as usize - (bit % 8);
                    let idx = ((byte >> shift) & ((1u16 << bpp) - 1) as u8) as usize;
                    let c = palette.get(idx).copied().unwrap_or([0, 0, 0]);
                    (c[0], c[1], c[2], 255)
                }
                16 => {
                    let v = u16::from_le_bytes([src[x * 2], src[x * 2 + 1]]) as u32;
                    if green_mask == 0x7e0 {
                        let r = ((v >> 11) & 31) * 255 / 31;
                        let g = ((v >> 5) & 63) * 255 / 63;
                        let bb = (v & 31) * 255 / 31;
                        (r as u8, g as u8, bb as u8, 255)
                    } else {
                        let r = ((v >> 10) & 31) * 255 / 31;
                        let g = ((v >> 5) & 31) * 255 / 31;
                        let bb = (v & 31) * 255 / 31;
                        (r as u8, g as u8, bb as u8, 255)
                    }
                }
                24 => (src[x * 3 + 2], src[x * 3 + 1], src[x * 3], 255),
                _ => {
                    let a = src[x * 4 + 3];
                    if a != 0 {
                        any_alpha = true;
                    }
                    (src[x * 4 + 2], src[x * 4 + 1], src[x * 4], a)
                }
            };
            let o = (y * wu + x) * 4;
            rgba[o..o + 4].copy_from_slice(&[r, g, bl, a]);
        }
    }
    // 32 бита с настоящей альфой — маска не нужна. Иначе прозрачность — из
    // AND-маски; «инвертирующие» точки (маска 1 при ненулевом цвете) Xcursor
    // не умеет — рисуем их чёрными, как это делают системы без инверсии.
    if !(bpp == 32 && any_alpha) {
        for y in 0..hu {
            let row = if top_down { y } else { hu - 1 - y };
            for x in 0..wu {
                let transparent = mask
                    .map(|m| m[row * mstride + x / 8] & (0x80 >> (x % 8)) != 0)
                    .unwrap_or(false);
                let o = (y * wu + x) * 4;
                if transparent {
                    let colored = rgba[o] | rgba[o + 1] | rgba[o + 2] != 0;
                    let px = if colored { [0, 0, 0, 255] } else { [0, 0, 0, 0] };
                    rgba[o..o + 4].copy_from_slice(&px);
                } else {
                    rgba[o + 3] = 255;
                }
            }
        }
    }
    Ok((w, h, rgba))
}

// ============================================================================
// ANI (RIFF ACON)
// ============================================================================

/// Заголовок `anih`.
#[derive(Clone, Copy, Debug)]
struct AniHeader {
    steps: u32,
    rate: u32,
}

/// Разобрать `.ani`: кадры, шаги (`seq`), задержки (`rate`, 1/60 с → мс).
pub fn parse_ani(data: &[u8]) -> Result<Animation, String> {
    if data.get(0..4) != Some(b"RIFF") || data.get(8..12) != Some(b"ACON") {
        return Err("not an animated cursor".into());
    }
    let riff_len = u32_at(data, 4).unwrap_or(0) as usize;
    // Длина RIFF у части файлов врёт — верим меньшему из двух.
    let end = 8usize.saturating_add(riff_len).min(data.len());
    let mut header: Option<AniHeader> = None;
    let mut rate: Option<Vec<u32>> = None;
    let mut seq: Option<Vec<u32>> = None;
    let mut frames: Vec<Option<Vec<CurImage>>> = Vec::new();
    let mut frame_err = String::new();
    let mut title = None;
    let mut artist = None;
    let mut pos = 12usize;
    while pos + 8 <= end {
        let id = &data[pos..pos + 4];
        let size = u32_at(data, pos + 4).unwrap_or(0) as usize;
        let body_start = pos + 8;
        let body_end = body_start.saturating_add(size).min(end);
        let body = &data[body_start..body_end];
        match id {
            b"anih" => {
                if body.len() < 36 {
                    return Err("animated cursor header is damaged".into());
                }
                header = Some(AniHeader {
                    steps: u32_at(body, 8).unwrap_or(0),
                    rate: u32_at(body, 28).unwrap_or(0),
                });
            }
            b"rate" => rate = Some(u32_list(body)),
            b"seq " => seq = Some(u32_list(body)),
            b"LIST" if body.len() >= 4 => match &body[0..4] {
                b"fram" => walk_chunks(&body[4..], |cid, cbody| {
                    if cid == b"icon" && frames.len() < MAX_FRAMES {
                        match parse_icondir(cbody) {
                            Ok(imgs) => frames.push(Some(imgs)),
                            Err(e) => {
                                frame_err = e;
                                frames.push(None);
                            }
                        }
                    }
                }),
                b"INFO" => walk_chunks(&body[4..], |cid, cbody| {
                    let text = || {
                        let raw = cbody.split(|c| *c == 0).next().unwrap_or(&[]);
                        let t = decode_text(raw).trim().to_string();
                        (!t.is_empty()).then_some(t)
                    };
                    match cid {
                        b"INAM" => title = text(),
                        b"IART" => artist = text(),
                        _ => {}
                    }
                }),
                _ => {}
            },
            _ => {}
        }
        pos = body_start.saturating_add(size) + (size & 1);
    }
    let header = header.ok_or("animated cursor has no header")?;
    // Битый кадр не роняет весь курсор: его место занимает соседний целый.
    let Some(first_ok) = frames.iter().position(|f| f.is_some()) else {
        return Err(if frame_err.is_empty() {
            "animated cursor has no frames".into()
        } else {
            frame_err
        });
    };
    let mut prev = frames[first_ok].clone().expect("целый кадр");
    let frames: Vec<Vec<CurImage>> = frames
        .into_iter()
        .map(|f| match f {
            Some(imgs) => {
                prev = imgs.clone();
                imgs
            }
            None => prev.clone(),
        })
        .collect();

    let n_steps = if header.steps > 0 { header.steps as usize } else { frames.len() };
    let mut steps = Vec::new();
    for i in 0..n_steps.min(MAX_STEPS) {
        let frame = match seq.as_ref().and_then(|s| s.get(i)) {
            Some(v) => *v as usize,
            None => i % frames.len(),
        };
        if frame >= frames.len() {
            continue;
        }
        let jiffies = rate
            .as_ref()
            .and_then(|r| r.get(i).copied())
            .unwrap_or(header.rate)
            .max(1);
        let delay_ms = ((jiffies as u64 * 1000 + 30) / 60).min(u32::MAX as u64) as u32;
        steps.push(Step { frame, delay_ms });
    }
    if steps.is_empty() {
        steps.push(Step { frame: 0, delay_ms: 0 });
    }
    Ok(Animation { frames, steps, title, artist })
}

fn u32_list(body: &[u8]) -> Vec<u32> {
    body.chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Обойти подчанки RIFF-списка (с выравниванием на чётный байт).
fn walk_chunks(mut b: &[u8], mut f: impl FnMut(&[u8], &[u8])) {
    while b.len() >= 8 {
        let size = u32_at(b, 4).unwrap_or(0) as usize;
        let body_end = 8usize.saturating_add(size).min(b.len());
        f(&b[0..4], &b[8..body_end]);
        let next = 8usize.saturating_add(size) + (size & 1);
        if next >= b.len() {
            break;
        }
        b = &b[next..];
    }
}

// ============================================================================
// Масштаб и Xcursor
// ============================================================================

/// Отрисованная картинка одного номинала: RGBA с ПРЕДУМНОЖЕННОЙ альфой.
#[derive(Clone, Debug)]
struct Rendered {
    width: u32,
    height: u32,
    hot_x: u32,
    hot_y: u32,
    premul: Vec<u8>,
}

/// Номиналы для анимации: 24/32/48 всегда, 64 и 96 — если исходник не меньше.
pub fn target_sizes(anim: &Animation) -> Vec<u32> {
    let m = anim.max_side();
    let mut v = vec![24, 32, 48];
    if m >= 64 {
        v.push(64);
    }
    if m >= 96 {
        v.push(96);
    }
    v
}

fn premultiply(rgba: &[u8]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        let a = px[3] as u32;
        for c in &mut px[0..3] {
            *c = ((*c as u32 * a + 127) / 255) as u8;
        }
    }
    out
}

fn unpremultiply(premul: &[u8]) -> Vec<u8> {
    let mut out = premul.to_vec();
    for px in out.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        } else {
            for c in &mut px[0..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    out
}

/// Масштаб предумноженной картинки без «мыла» на увеличении.
fn scale_premul(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    use image::imageops::{resize, FilterType};
    if (sw, sh) == (dw, dh) {
        return src.to_vec();
    }
    let img = image::RgbaImage::from_raw(sw, sh, src.to_vec()).expect("размер буфера");
    let out = if dw >= sw && dh >= sh {
        // Целое повторение пикселей, остаток — Ланцошем вниз.
        let k = dw.div_ceil(sw).max(dh.div_ceil(sh)).max(1);
        let nn = resize(&img, sw * k, sh * k, FilterType::Nearest);
        if (sw * k, sh * k) == (dw, dh) {
            nn
        } else {
            resize(&nn, dw, dh, FilterType::Lanczos3)
        }
    } else {
        resize(&img, dw, dh, FilterType::Lanczos3)
    };
    let mut raw = out.into_raw();
    // Звон Ланцоша не должен дать цвет ярче альфы (Xcursor — предумноженный).
    for px in raw.chunks_exact_mut(4) {
        let a = px[3];
        for c in &mut px[0..3] {
            if *c > a {
                *c = a;
            }
        }
    }
    raw
}

/// Картинка кадра в номинале `size`: берётся наименьшая из файла, что не
/// меньше номинала (иначе наибольшая), холст приводится к номиналу.
fn render_frame(images: &[CurImage], size: u32) -> Rendered {
    let side = |i: &CurImage| i.width.max(i.height);
    let src = images
        .iter()
        .filter(|i| side(i) >= size)
        .min_by_key(|i| side(i))
        .or_else(|| images.iter().max_by_key(|i| side(i)))
        .expect("у кадра есть картинки");
    let s = size as f64 / side(src) as f64;
    let dw = ((src.width as f64 * s).round() as u32).max(1);
    let dh = ((src.height as f64 * s).round() as u32).max(1);
    let premul = scale_premul(&premultiply(&src.rgba), src.width, src.height, dw, dh);
    let hot = |h: u32, sd: u32, dd: u32| {
        ((((h as f64) + 0.5) * dd as f64 / sd as f64).floor() as u32).min(dd - 1)
    };
    Rendered {
        width: dw,
        height: dh,
        hot_x: hot(src.hot_x, src.width, dw),
        hot_y: hot(src.hot_y, src.height, dh),
        premul,
    }
}

const XCUR_MAGIC: &[u8; 4] = b"Xcur";
const XCUR_IMAGE: u32 = 0xfffd_0002;

/// Итог перевода одного курсора.
#[derive(Clone, Debug)]
pub struct XcursorOut {
    pub bytes: Vec<u8>,
    /// Номиналы, попавшие в файл.
    pub sizes: Vec<u32>,
    /// Шагов анимации в файле (после склейки соседних одинаковых).
    pub steps: usize,
}

/// Шаги для записи: соседние шаги одного кадра склеиваются (сумма задержек),
/// у неподвижного курсора задержка 0.
pub fn merged_steps(anim: &Animation) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
    for s in &anim.steps {
        if let Some(last) = steps.last_mut() {
            if last.frame == s.frame {
                last.delay_ms = last.delay_ms.saturating_add(s.delay_ms);
                continue;
            }
        }
        steps.push(*s);
    }
    if steps.len() == 1 {
        steps[0].delay_ms = 0;
    }
    steps
}

/// Собрать файл Xcursor со ВСЕМИ шагами анимации во всех номиналах.
pub fn to_xcursor(anim: &Animation, budget: usize) -> XcursorOut {
    let steps = merged_steps(anim);
    let mut sizes = target_sizes(anim);
    let estimate = |sizes: &[u32]| -> usize {
        sizes
            .iter()
            .map(|s| (12 + 36 + (*s as usize * *s as usize * 4)) * steps.len())
            .sum()
    };
    while sizes.len() > 2 && estimate(&sizes) > budget {
        sizes.pop();
    }
    let mut cache: HashMap<(usize, u32), Rendered> = HashMap::new();
    let mut chunks: Vec<(u32, Vec<u8>)> = Vec::new();
    for &size in &sizes {
        for st in &steps {
            let r = cache
                .entry((st.frame, size))
                .or_insert_with(|| render_frame(&anim.frames[st.frame], size));
            let mut c = Vec::with_capacity(36 + r.premul.len());
            for v in [
                36,
                XCUR_IMAGE,
                size,
                1,
                r.width,
                r.height,
                r.hot_x,
                r.hot_y,
                st.delay_ms,
            ] {
                c.extend_from_slice(&v.to_le_bytes());
            }
            for px in r.premul.chunks_exact(4) {
                let argb = (px[3] as u32) << 24
                    | (px[0] as u32) << 16
                    | (px[1] as u32) << 8
                    | px[2] as u32;
                c.extend_from_slice(&argb.to_le_bytes());
            }
            chunks.push((size, c));
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(XCUR_MAGIC);
    for v in [16u32, 0x1_0000, chunks.len() as u32] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let mut offset = 16 + chunks.len() * 12;
    for (size, c) in &chunks {
        for v in [XCUR_IMAGE, *size, offset as u32] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        offset += c.len();
    }
    for (_, c) in &chunks {
        out.extend_from_slice(c);
    }
    XcursorOut { bytes: out, sizes, steps: steps.len() }
}

/// Сведения о картинке файла Xcursor (для сверок и проверки результата).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XcurImageInfo {
    pub nominal: u32,
    pub width: u32,
    pub height: u32,
    pub hot_x: u32,
    pub hot_y: u32,
    pub delay: u32,
}

/// Прочесть оглавление и заголовки картинок файла Xcursor.
pub fn read_xcursor(data: &[u8]) -> Result<Vec<XcurImageInfo>, String> {
    if data.get(0..4) != Some(XCUR_MAGIC) {
        return Err("not an Xcursor file".into());
    }
    let header = u32_at(data, 4).ok_or("short")? as usize;
    let ntoc = u32_at(data, 12).ok_or("short")? as usize;
    let mut out = Vec::new();
    for i in 0..ntoc {
        let t = header + i * 12;
        let kind = u32_at(data, t).ok_or("toc is truncated")?;
        if kind != XCUR_IMAGE {
            continue;
        }
        let p = u32_at(data, t + 8).ok_or("toc is truncated")? as usize;
        let f = |k: usize| u32_at(data, p + k * 4).ok_or("image is truncated");
        let info = XcurImageInfo {
            nominal: f(2)?,
            width: f(4)?,
            height: f(5)?,
            hot_x: f(6)?,
            hot_y: f(7)?,
            delay: f(8)?,
        };
        let need = p + 36 + info.width as usize * info.height as usize * 4;
        if need > data.len() {
            return Err("pixels are truncated".into());
        }
        out.push(info);
    }
    Ok(out)
}

/// Обложка набора: первые кадры курсоров в сетке на прозрачном фоне (PNG).
pub fn preview_png(cursors: &[&Animation], width: u32, height: u32, cell: u32) -> Option<Vec<u8>> {
    if cursors.is_empty() {
        return None;
    }
    let n = cursors.len() as u32;
    let gap = cell / 3;
    let per_row = ((width - gap) / (cell + gap)).max(1).min(n);
    let rows = n.div_ceil(per_row);
    let mut canvas = image::RgbaImage::new(width, height);
    let total_h = rows * cell + (rows - 1) * gap;
    let y0 = height.saturating_sub(total_h) / 2;
    for (i, anim) in cursors.iter().enumerate() {
        let (i, row) = (i as u32 % per_row, i as u32 / per_row);
        let in_row = if row + 1 == rows { n - row * per_row } else { per_row };
        let row_w = in_row * cell + (in_row - 1) * gap;
        let x0 = width.saturating_sub(row_w) / 2 + i * (cell + gap);
        let y = y0 + row * (cell + gap);
        let first = anim.steps.first().map(|s| s.frame).unwrap_or(0);
        let r = render_frame(&anim.frames[first], cell);
        let tile = image::RgbaImage::from_raw(r.width, r.height, unpremultiply(&r.premul))?;
        image::imageops::overlay(&mut canvas, &tile, x0 as i64, y as i64);
    }
    let mut out = Vec::new();
    use image::ImageEncoder;
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(canvas.as_raw(), width, height, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(out)
}

// ============================================================================
// Роли курсоров Windows и имена Xcursor
// ============================================================================

/// Роль курсора в схеме Windows (имя значения в `HKCU\Control Panel\Cursors`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    Arrow,
    Help,
    AppStarting,
    Wait,
    Crosshair,
    IBeam,
    NWPen,
    No,
    SizeNS,
    SizeWE,
    SizeNWSE,
    SizeNESW,
    SizeAll,
    UpArrow,
    Hand,
    Pin,
    Person,
    Pan,
    Grab,
    Grabbing,
    ZoomIn,
    ZoomOut,
}

/// Порядок путей в значении схемы `…\Cursors\Schemes` (так его читает Windows).
pub const SCHEME_ORDER: [Role; 17] = [
    Role::Arrow,
    Role::Help,
    Role::AppStarting,
    Role::Wait,
    Role::Crosshair,
    Role::IBeam,
    Role::NWPen,
    Role::No,
    Role::SizeNS,
    Role::SizeWE,
    Role::SizeNWSE,
    Role::SizeNESW,
    Role::SizeAll,
    Role::UpArrow,
    Role::Hand,
    Role::Pin,
    Role::Person,
];

/// Все роли (для обхода).
pub const ALL_ROLES: [Role; 22] = [
    Role::Arrow,
    Role::Help,
    Role::AppStarting,
    Role::Wait,
    Role::Crosshair,
    Role::IBeam,
    Role::NWPen,
    Role::No,
    Role::SizeNS,
    Role::SizeWE,
    Role::SizeNWSE,
    Role::SizeNESW,
    Role::SizeAll,
    Role::UpArrow,
    Role::Hand,
    Role::Pin,
    Role::Person,
    Role::Pan,
    Role::Grab,
    Role::Grabbing,
    Role::ZoomIn,
    Role::ZoomOut,
];

impl Role {
    /// Имя значения реестра Windows.
    pub fn reg_name(self) -> &'static str {
        match self {
            Role::Arrow => "Arrow",
            Role::Help => "Help",
            Role::AppStarting => "AppStarting",
            Role::Wait => "Wait",
            Role::Crosshair => "Crosshair",
            Role::IBeam => "IBeam",
            Role::NWPen => "NWPen",
            Role::No => "No",
            Role::SizeNS => "SizeNS",
            Role::SizeWE => "SizeWE",
            Role::SizeNWSE => "SizeNWSE",
            Role::SizeNESW => "SizeNESW",
            Role::SizeAll => "SizeAll",
            Role::UpArrow => "UpArrow",
            Role::Hand => "Hand",
            Role::Pin => "Pin",
            Role::Person => "Person",
            Role::Pan => "Pan",
            Role::Grab => "Grab",
            Role::Grabbing => "Grabbing",
            Role::ZoomIn => "Zoom-in",
            Role::ZoomOut => "Zoom-out",
        }
    }

    /// Роль по имени значения реестра / секции `.crs` (регистр не важен).
    pub fn from_reg_name(name: &str) -> Option<Role> {
        let n = name.trim().trim_matches('"').to_ascii_lowercase();
        if n == "precisionhair" {
            return Some(Role::Crosshair);
        }
        ALL_ROLES.iter().copied().find(|r| r.reg_name().eq_ignore_ascii_case(&n))
    }

    /// Имена Xcursor роли; первое — файл, остальные — ссылки на него.
    /// Набор: имена CSS, старые имена X11 и хеши, которые спрашивают Qt/GTK.
    pub fn xcursor_names(self) -> &'static [&'static str] {
        match self {
            Role::Arrow => &[
                "default", "left_ptr", "arrow", "top_left_arrow", "left-arrow",
                "context-menu", "copy", "dnd-copy", "alias", "dnd-link", "link",
                "1081e37283d90000800003c07f3ef6bf", "6407b0e94181790501fd1e167b474872",
                "b66166c04f8c3109214a4fbd64a50fc8", "3085a0e285430894940527032f8b26df",
                "640fb0e74195791501fd1ed57b41487f", "a2a266d0498c3104214a47bd64ab0fc8",
            ],
            Role::Help => &[
                "help", "question_arrow", "whats_this", "left_ptr_help", "dnd-ask",
                "5c6cd98b3f3ebcb1f9c7f1c204630408", "d9ce0ab605698f320427677b458ad60b",
            ],
            Role::AppStarting => &[
                "progress", "left_ptr_watch", "half-busy",
                "3ecb610c1bf2410f44200f48c40d3599", "08e8e1c95fe2fc01f976f1e063a24ccd",
                "00000000000000020006000e7e9ffc3f",
            ],
            Role::Wait => &["wait", "watch"],
            Role::Crosshair => &[
                "crosshair", "cross", "tcross", "cross_reverse", "diamond_cross", "cell", "plus",
            ],
            Role::IBeam => &["text", "xterm", "ibeam", "vertical-text"],
            Role::NWPen => &["pencil", "draft"],
            Role::No => &[
                "not-allowed", "crossed_circle", "circle", "forbidden", "no-drop",
                "dnd-no-drop", "X_cursor", "pirate", "03b6e0fcb3499374a867c041f52298f0",
            ],
            Role::SizeNS => &[
                "ns-resize", "n-resize", "s-resize", "size_ver", "sb_v_double_arrow",
                "v_double_arrow", "double_arrow", "row-resize", "split_v", "top_side",
                "bottom_side", "00008160000006810000408080010102",
                "2870a09082c103050810ffdffffe0204",
            ],
            Role::SizeWE => &[
                "ew-resize", "e-resize", "w-resize", "size_hor", "sb_h_double_arrow",
                "h_double_arrow", "col-resize", "split_h", "left_side", "right_side",
                "028006030e0e7ebffc7f7070c0600140", "14fef782d02440884392942c11205230",
            ],
            Role::SizeNWSE => &[
                "nwse-resize", "nw-resize", "se-resize", "size_fdiag", "bd_double_arrow",
                "top_left_corner", "bottom_right_corner", "ul_angle", "lr_angle",
                "c7088f0f3e6c8088236ef8e1e3e70000",
            ],
            Role::SizeNESW => &[
                "nesw-resize", "ne-resize", "sw-resize", "size_bdiag", "fd_double_arrow",
                "top_right_corner", "bottom_left_corner", "ur_angle", "ll_angle",
                "fcf1c3c7cd4491d801f1e1c78f100000",
            ],
            Role::SizeAll => &[
                "move", "fleur", "size_all", "dnd-move",
                "4498f0e0c1937ffe01fd06f973665830", "9081237383d90e509aa00f00170e968f",
            ],
            Role::UpArrow => &["up-arrow", "up_arrow", "sb_up_arrow", "center_ptr"],
            Role::Hand => &[
                "pointer", "hand", "hand1", "hand2", "pointing_hand",
                "9d800788f1b08800ae810202380a0822", "e29285e634086352946a0e7090d73106",
            ],
            // У Linux своих курсоров «место»/«человек» нет — файл остаётся,
            // чтобы набор был полным, но никто его не спросит.
            Role::Pin => &["pin"],
            Role::Person => &["person"],
            Role::Pan => &["all-scroll"],
            Role::Grab => &["grab", "openhand"],
            Role::Grabbing => &[
                "grabbing", "closedhand", "dnd-none", "fcf21c00b30f7e3f83fe0dfd12e71cff",
            ],
            Role::ZoomIn => &["zoom-in"],
            Role::ZoomOut => &["zoom-out"],
        }
    }

    /// Чем заменить роль, если в наборе её нет (ближайший по смыслу курсор
    /// ТОГО ЖЕ набора лучше курсора чужой темы). Пусто — наследуется.
    pub fn fallbacks(self) -> &'static [Role] {
        match self {
            Role::AppStarting => &[Role::Wait],
            Role::Wait => &[Role::AppStarting],
            Role::UpArrow => &[Role::Arrow],
            Role::Pan => &[Role::SizeAll],
            Role::Grab => &[Role::Hand],
            Role::Grabbing => &[Role::Grab, Role::SizeAll],
            _ => &[],
        }
    }
}

/// Нормализовать имя ключа/файла: строчные латиница и цифры, без ведущих
/// цифр («01 - Normal Select» → «normalselect»).
fn norm_key(s: &str) -> String {
    let k: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    k.trim_start_matches(|c: char| c.is_ascii_digit()).to_string()
}

/// Роль по имени ключа `[Strings]` или файла. `hand` здесь не решается:
/// в шаблоне `install.inf` это перо (NWPen), в именах файлов — смотря по
/// набору (см. [`roles_from_filenames`]).
fn role_from_key(key: &str) -> Option<Role> {
    let k = norm_key(key);
    let r = match k.as_str() {
        "pointer" | "normal" | "normalselect" | "arrow" | "default" | "leftptr" => Role::Arrow,
        "help" | "helpselect" | "question" => Role::Help,
        "work" | "working" | "workinginbackground" | "appstarting" | "background"
        | "progress" | "workinprogress" => Role::AppStarting,
        "busy" | "wait" | "loading" => Role::Wait,
        "cross" | "crosshair" | "precision" | "precisionselect" => Role::Crosshair,
        "text" | "textselect" | "ibeam" | "beam" => Role::IBeam,
        "handwriting" | "pen" | "nwpen" | "pencil" => Role::NWPen,
        "unavailable" | "unavailiable" | "unavaliable" | "unavalible" | "no"
        | "notallowed" | "forbidden" => Role::No,
        "vert" | "vertical" | "verticalresize" | "sizens" | "dragvertical" => Role::SizeNS,
        "horz" | "horiz" | "horizontal" | "horizontalresize" | "sizewe"
        | "draghorizontal" => Role::SizeWE,
        "dgn1" | "dng1" | "diag1" | "diagonal1" | "diagonalresize1" | "sizenwse"
        | "dragdiagonal1" => Role::SizeNWSE,
        "dgn2" | "dng2" | "diag2" | "diagonal2" | "diagonalresize2" | "sizenesw"
        | "dragdiagonal2" => Role::SizeNESW,
        "move" | "sizeall" | "dragany" => Role::SizeAll,
        "alternate" | "alt" | "alternateselect" | "uparrow" => Role::UpArrow,
        "link" | "linkselect" => Role::Hand,
        "pin" | "location" | "locationselect" | "linklocation" => Role::Pin,
        "person" | "personselect" | "linkperson" => Role::Person,
        "pan" => Role::Pan,
        "grab" => Role::Grab,
        "grabbing" => Role::Grabbing,
        "zoomin" => Role::ZoomIn,
        "zoomout" => Role::ZoomOut,
        _ => return None,
    };
    Some(r)
}

/// Имя файла из пути Windows (`%10%\Cursors\X\Normal.ani` → `Normal.ani`).
pub fn win_basename(p: &str) -> String {
    let p = p.trim().trim_matches('"').trim();
    p.rsplit(['\\', '/']).next().unwrap_or(p).trim().to_string()
}

fn is_cursor_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".cur") || l.ends_with(".ani")
}

/// Роли из описателя: (роль, имя файла как в описателе) + имя схемы.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemeInfo {
    pub roles: Vec<(Role, String)>,
    pub name: Option<String>,
}

/// Поля строки реестра INF через запятую, с учётом кавычек.
fn split_inf_fields(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// Подставить `%имя%` из `[Strings]` (регистр не важен; `%%` → `%`).
fn expand_inf(s: &str, strings: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) => {
                let name = &after[..j];
                if name.is_empty() {
                    out.push('%');
                } else if let Some(v) = strings.get(&name.to_ascii_lowercase()) {
                    out.push_str(v);
                } else {
                    out.push('%');
                    out.push_str(name);
                    out.push('%');
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Роли из `install.inf`: явные строки `…\Cursors",<Роль>,…` важнее списка
/// схемы `…\Cursors\Schemes` (в нём порядок Windows, см. [`SCHEME_ORDER`]);
/// нет ни того, ни другого — ключи `[Strings]` (`pointer`, `work`, `hand`…).
pub fn roles_from_inf(text: &str) -> SchemeInfo {
    let mut section = String::new();
    let mut strings: HashMap<String, String> = HashMap::new();
    let mut string_order: Vec<(String, String)> = Vec::new();
    let mut reg_lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            if let Some(end) = line.find(']') {
                section = line[1..end].trim().to_ascii_lowercase();
            }
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("hkcu") || lower.starts_with("hklm") {
            reg_lines.push(line.to_string());
            continue;
        }
        if section.starts_with("strings") {
            if let Some((k, v)) = line.split_once('=') {
                let k = k.trim().to_ascii_lowercase();
                let v = v.trim().trim_matches('"').to_string();
                strings.entry(k.clone()).or_insert_with(|| v.clone());
                string_order.push((k, v));
            }
        }
    }

    let mut explicit: Vec<(Role, String)> = Vec::new();
    let mut scheme: Vec<(Role, String)> = Vec::new();
    let mut name: Option<String> = None;
    for line in &reg_lines {
        let f = split_inf_fields(line);
        if f.len() < 5 {
            continue;
        }
        let path = expand_inf(&f[1], &strings).to_ascii_lowercase().replace('/', "\\");
        let path = path.trim_end_matches('\\');
        let value_name = expand_inf(&f[2], &strings);
        // Значение могло разойтись по полям, если его не взяли в кавычки.
        let value = expand_inf(&f[4..].join(","), &strings);
        if path.ends_with("control panel\\cursors\\schemes") {
            if name.is_none() && !value_name.trim().is_empty() {
                name = Some(value_name.trim().to_string());
            }
            if scheme.is_empty() {
                for (role, p) in SCHEME_ORDER.iter().zip(value.split(',')) {
                    let file = win_basename(p);
                    if is_cursor_name(&file) {
                        scheme.push((*role, file));
                    }
                }
            }
        } else if path.ends_with("control panel\\cursors") && !value_name.trim().is_empty() {
            if let Some(role) = Role::from_reg_name(&value_name) {
                let file = win_basename(&value);
                if is_cursor_name(&file) && !explicit.iter().any(|(r, _)| *r == role) {
                    explicit.push((role, file));
                }
            }
        }
    }
    // Список схемы верен, только если сходится с явными строками: у части
    // наборов (macOS-Windows 2.0.1) он сложен не в порядке Windows, и тогда
    // «Недоступно» досталось бы курсору вертикального размера.
    let scheme_trusted = explicit.iter().all(|(role, file)| {
        scheme
            .iter()
            .find(|(r, _)| r == role)
            .is_none_or(|(_, f)| f.eq_ignore_ascii_case(file))
    });
    let mut roles = explicit;
    if scheme_trusted {
        for (role, file) in scheme {
            if !roles.iter().any(|(r, _)| *r == role) {
                roles.push((role, file));
            }
        }
    }
    // Остальные роли — по ключам `[Strings]` (`pointer`, `unavailiable`,
    // `dgn1`…): у шаблона install.inf имя ключа и есть роль.
    let used: Vec<String> = roles.iter().map(|(_, f)| f.to_ascii_lowercase()).collect();
    for (k, v) in &string_order {
        if !is_cursor_name(v) {
            continue;
        }
        let file = win_basename(v);
        // Файл уже стоит на роли по реестру — ключ его не переназначает.
        if !roles.is_empty() && used.contains(&file.to_ascii_lowercase()) {
            continue;
        }
        // В шаблоне install.inf ключ `hand` — это перо (Handwriting).
        let role = if norm_key(k) == "hand" { Some(Role::NWPen) } else { role_from_key(k) };
        if let Some(role) = role {
            if !roles.iter().any(|(r, _)| *r == role) {
                roles.push((role, file));
            }
        }
    }
    if name.is_none() {
        name = strings
            .get("scheme_name")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
    }
    SchemeInfo { roles, name }
}

/// Роли из `.crs` (RW-Designer): `[Arrow]` / `Path=pointer.cur`.
pub fn roles_from_crs(text: &str) -> SchemeInfo {
    let mut roles: Vec<(Role, String)> = Vec::new();
    let mut current: Option<Role> = None;
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.starts_with('[') {
            current = line
                .find(']')
                .and_then(|end| Role::from_reg_name(&line[1..end]));
            continue;
        }
        let Some(role) = current else { continue };
        if let Some((k, v)) = line.split_once('=') {
            if k.trim().eq_ignore_ascii_case("path") {
                let file = win_basename(v);
                if is_cursor_name(&file) && !roles.iter().any(|(r, _)| *r == role) {
                    roles.push((role, file));
                }
            }
        }
    }
    SchemeInfo { roles, name: None }
}

/// Запасной путь: роли по именам файлов. `hand` — перо, если в наборе есть
/// отдельный курсор ссылки (`link`), иначе — рука-ссылка.
pub fn roles_from_filenames(files: &[String]) -> Vec<(Role, String)> {
    let stem = |f: &str| f.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_default();
    let has_link = files.iter().any(|f| role_from_key(&stem(f)) == Some(Role::Hand));
    let mut roles: Vec<(Role, String)> = Vec::new();
    for f in files {
        if !is_cursor_name(f) {
            continue;
        }
        let s = stem(f);
        let role = if norm_key(&s) == "hand" {
            Some(if has_link { Role::NWPen } else { Role::Hand })
        } else {
            role_from_key(&s)
        };
        if let Some(role) = role {
            if !roles.iter().any(|(r, _)| *r == role) {
                roles.push((role, f.clone()));
            }
        }
    }
    roles
}

// ============================================================================
// Текст описателей: кодировки
// ============================================================================

/// Windows-1251 (0x80..0xBF); 0xC0..0xFF — «А»…«я» подряд.
const CP1251_HIGH: [u16; 64] = [
    0x0402, 0x0403, 0x201A, 0x0453, 0x201E, 0x2026, 0x2020, 0x2021, 0x20AC, 0x2030, 0x0409,
    0x2039, 0x040A, 0x040C, 0x040B, 0x040F, 0x0452, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022,
    0x2013, 0x2014, 0xFFFD, 0x2122, 0x0459, 0x203A, 0x045A, 0x045C, 0x045B, 0x045F, 0x00A0,
    0x040E, 0x045E, 0x0408, 0x00A4, 0x0490, 0x00A6, 0x00A7, 0x0401, 0x00A9, 0x0404, 0x00AB,
    0x00AC, 0x00AD, 0x00AE, 0x0407, 0x00B0, 0x00B1, 0x0406, 0x0456, 0x0491, 0x00B5, 0x00B6,
    0x00B7, 0x0451, 0x2116, 0x0454, 0x00BB, 0x0458, 0x0405, 0x0455, 0x0457,
];

fn decode_utf16(b: &[u8], le: bool) -> String {
    let units: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
        .collect();
    String::from_utf16_lossy(&units)
}

/// Текст описателя набора: UTF-16 (с BOM или без), UTF-8, иначе
/// Windows-1251 (наборы с русских сайтов пишутся в ней).
pub fn decode_text(b: &[u8]) -> String {
    if let Some(rest) = b.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(rest, true);
    }
    if let Some(rest) = b.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(rest, false);
    }
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
    // UTF-16LE без BOM: латиница даёт нулевой каждый второй байт.
    let head = &b[..b.len().min(64) & !1];
    if head.len() >= 8 && head.iter().skip(1).step_by(2).all(|c| *c == 0)
        && head.iter().step_by(2).all(|c| *c != 0)
    {
        return decode_utf16(b, true);
    }
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b
            .iter()
            .map(|&c| match c {
                0..=0x7F => c as char,
                0x80..=0xBF => char::from_u32(CP1251_HIGH[(c - 0x80) as usize] as u32).unwrap_or('\u{FFFD}'),
                _ => char::from_u32(0x0410 + (c - 0xC0) as u32).unwrap_or('\u{FFFD}'),
            })
            .collect(),
    }
}

// ============================================================================
// Сверки на подставных байтах
// ============================================================================

#[cfg(test)]
pub(crate) mod testbytes {
    //! Сборщики подставных `.cur`/`.ani` для сверок (своих и импортёра).

    /// `.cur` с одной 32-битной картинкой `w`×`w`: пиксель (x, y) =
    /// (x, y, 7, 255), остальное по `alpha_at`; горячая точка (hx, hy).
    pub fn cur32(w: u32, hx: u16, hy: u16, tint: u8) -> Vec<u8> {
        let mut dib = Vec::new();
        for v in [40u32, w, w * 2] {
            dib.extend_from_slice(&v.to_le_bytes());
        }
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&[0u8; 24]);
        // строки снизу вверх
        for y in (0..w).rev() {
            for x in 0..w {
                dib.extend_from_slice(&[tint, y as u8, x as u8, 255]);
            }
        }
        dib.extend(std::iter::repeat_n(0u8, (w as usize).div_ceil(32) * 4 * w as usize));
        icondir(2, &[(w, hx, hy, dib)])
    }

    /// Каталог: (сторона, поле1, поле2, тело записи).
    pub fn icondir(kind: u16, entries: &[(u32, u16, u16, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        let mut offset = 6 + 16 * entries.len();
        for (w, f1, f2, body) in entries {
            let side = if *w >= 256 { 0 } else { *w as u8 };
            out.extend_from_slice(&[side, side, 0, 0]);
            out.extend_from_slice(&f1.to_le_bytes());
            out.extend_from_slice(&f2.to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += body.len();
        }
        for (_, _, _, body) in entries {
            out.extend_from_slice(body);
        }
        out
    }

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut c = id.to_vec();
        c.extend_from_slice(&(body.len() as u32).to_le_bytes());
        c.extend_from_slice(body);
        if body.len() % 2 == 1 {
            c.push(0);
        }
        c
    }

    /// `.ani` из кадров-`.cur`, с `rate`/`seq` (None — чанк не пишется).
    pub fn ani(frames: &[Vec<u8>], disp_rate: u32, rate: Option<&[u32]>, seq: Option<&[u32]>, steps: u32) -> Vec<u8> {
        let mut anih = Vec::new();
        for v in [36u32, frames.len() as u32, steps, 0, 0, 0, 0, disp_rate, 1 | if seq.is_some() { 2 } else { 0 }] {
            anih.extend_from_slice(&v.to_le_bytes());
        }
        let mut body = b"ACON".to_vec();
        let mut info = b"INFO".to_vec();
        info.extend(chunk(b"INAM", b"Test cursor\0"));
        info.extend(chunk(b"IART", b"Wehrwolfmann\0"));
        body.extend(chunk(b"LIST", &info));
        body.extend(chunk(b"anih", &anih));
        let list = |v: &[u32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        if let Some(r) = rate {
            body.extend(chunk(b"rate", &list(r)));
        }
        if let Some(s) = seq {
            body.extend(chunk(b"seq ", &list(s)));
        }
        let mut fram = b"fram".to_vec();
        for f in frames {
            fram.extend(chunk(b"icon", f));
        }
        body.extend(chunk(b"LIST", &fram));
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::testbytes::*;
    use super::*;

    #[test]
    fn cur_keeps_hotspot_and_pixels() {
        let a = parse_cursor_file(&cur32(32, 5, 9, 3)).unwrap();
        assert!(!a.is_animated());
        let img = &a.frames[0][0];
        assert_eq!((img.width, img.height, img.hot_x, img.hot_y), (32, 32, 5, 9));
        // (x=4, y=2) сверху: R=x, G=y, B=tint (BGRA в файле → RGBA здесь)
        let o = (2 * 32 + 4) * 4;
        assert_eq!(&img.rgba[o..o + 4], &[4, 2, 3, 255]);
    }

    #[test]
    fn ani_keeps_every_step_delay_and_hotspot() {
        let f0 = cur32(32, 1, 2, 10);
        let f1 = cur32(32, 3, 4, 20);
        let f2 = cur32(32, 5, 6, 30);
        // seq: 0,1,2,1 ; rate в 1/60 с: 6,12,3,60
        let data = ani(&[f0, f1, f2], 10, Some(&[6, 12, 3, 60]), Some(&[0, 1, 2, 1]), 4);
        let a = parse_cursor_file(&data).unwrap();
        assert_eq!(a.frames.len(), 3);
        assert_eq!(
            a.steps,
            vec![
                Step { frame: 0, delay_ms: 100 },
                Step { frame: 1, delay_ms: 200 },
                Step { frame: 2, delay_ms: 50 },
                Step { frame: 1, delay_ms: 1000 },
            ]
        );
        assert_eq!(a.title.as_deref(), Some("Test cursor"));
        assert_eq!(a.artist.as_deref(), Some("Wehrwolfmann"));
        let out = to_xcursor(&a, XCURSOR_BUDGET);
        assert_eq!(out.sizes, vec![24, 32, 48]);
        let imgs = read_xcursor(&out.bytes).unwrap();
        assert_eq!(imgs.len(), 3 * 4);
        let at32: Vec<_> = imgs.iter().filter(|i| i.nominal == 32).collect();
        assert_eq!(
            at32.iter().map(|i| (i.hot_x, i.hot_y, i.delay)).collect::<Vec<_>>(),
            vec![(1, 2, 100), (3, 4, 200), (5, 6, 50), (3, 4, 1000)]
        );
        // 48 = ×1.5: точка (5,6) → floor(5.5·1.5)=8, floor(6.5·1.5)=9
        let at48: Vec<_> = imgs.iter().filter(|i| i.nominal == 48).collect();
        assert_eq!((at48[2].width, at48[2].hot_x, at48[2].hot_y), (48, 8, 9));
    }

    #[test]
    fn ani_without_seq_and_rate_uses_header_rate_in_order() {
        let fr: Vec<Vec<u8>> = (0..3).map(|i| cur32(32, i, i, i as u8)).collect();
        let a = parse_ani(&ani(&fr, 3, None, None, 3)).unwrap();
        assert_eq!(a.steps.iter().map(|s| (s.frame, s.delay_ms)).collect::<Vec<_>>(),
                   vec![(0, 50), (1, 50), (2, 50)]);
    }

    #[test]
    fn repeated_neighbour_steps_merge_and_static_has_zero_delay() {
        let fr: Vec<Vec<u8>> = (0..2).map(|i| cur32(32, 0, 0, i as u8)).collect();
        let a = parse_ani(&ani(&fr, 6, None, Some(&[0, 0, 1]), 3)).unwrap();
        let m = merged_steps(&a);
        assert_eq!(m, vec![Step { frame: 0, delay_ms: 200 }, Step { frame: 1, delay_ms: 100 }]);
        let s = parse_cursor_file(&cur32(32, 0, 0, 0)).unwrap();
        assert_eq!(merged_steps(&s), vec![Step { frame: 0, delay_ms: 0 }]);
    }

    #[test]
    fn big_canvas_is_scaled_to_nominals_with_hidpi() {
        let a = parse_cursor_file(&cur32(128, 64, 32, 0)).unwrap();
        let out = to_xcursor(&a, XCURSOR_BUDGET);
        assert_eq!(out.sizes, vec![24, 32, 48, 64, 96]);
        let imgs = read_xcursor(&out.bytes).unwrap();
        let i32_ = imgs.iter().find(|i| i.nominal == 32).unwrap();
        assert_eq!((i32_.width, i32_.hot_x, i32_.hot_y), (32, 16, 8));
    }

    #[test]
    fn budget_drops_hidpi_sizes_first() {
        let fr: Vec<Vec<u8>> = (0..40).map(|i| cur32(128, 0, 0, i as u8)).collect();
        let a = parse_ani(&ani(&fr, 1, None, None, 40)).unwrap();
        let out = to_xcursor(&a, 1 << 20);
        assert_eq!(out.sizes, vec![24, 32, 48]);
        assert_eq!(out.steps, 40);
    }

    #[test]
    fn integer_upscale_is_pixel_exact() {
        // 24 → 48: каждый пиксель ровно квадратом 2×2, без смешивания.
        let a = parse_cursor_file(&cur32(24, 0, 0, 9)).unwrap();
        let r = render_frame(&a.frames[0], 48);
        assert_eq!((r.width, r.height), (48, 48));
        let px = |x: usize, y: usize| r.premul[(y * 48 + x) * 4..(y * 48 + x) * 4 + 4].to_vec();
        assert_eq!(px(10, 6), vec![5, 3, 9, 255]);
        assert_eq!(px(11, 7), vec![5, 3, 9, 255]);
    }

    #[test]
    fn monochrome_cursor_uses_and_mask() {
        // 1 бит, 32×32: палитра чёрный/белый; XOR = 1 в левой половине,
        // AND = 1 в верхней половине → верх-лево «инверсия» (чёрный),
        // верх-право прозрачно, низ-лево белый, низ-право чёрный.
        let w = 32u32;
        let mut dib = Vec::new();
        for v in [40u32, w, w * 2] {
            dib.extend_from_slice(&v.to_le_bytes());
        }
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&[0u8; 24]);
        dib.extend_from_slice(&[0, 0, 0, 0, 255, 255, 255, 0]);
        for _ in 0..w {
            dib.extend_from_slice(&[0xFF, 0xFF, 0, 0]);
        }
        for y in (0..w).rev() {
            let bit = if y < 16 { 0xFF } else { 0 };
            dib.extend_from_slice(&[bit; 4]);
        }
        let a = parse_cursor_file(&icondir(2, &[(32, 0, 0, dib)])).unwrap();
        let img = &a.frames[0][0];
        let px = |x: usize, y: usize| img.rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4].to_vec();
        assert_eq!(px(3, 3), vec![0, 0, 0, 255]);
        assert_eq!(px(20, 3), vec![0, 0, 0, 0]);
        assert_eq!(px(3, 20), vec![255, 255, 255, 255]);
        assert_eq!(px(20, 20), vec![0, 0, 0, 255]);
    }

    #[test]
    fn png_entry_is_read() {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_pixel(48, 48, image::Rgba([1, 2, 3, 200]));
        use image::ImageEncoder;
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(img.as_raw(), 48, 48, image::ExtendedColorType::Rgba8)
            .unwrap();
        let a = parse_cursor_file(&icondir(2, &[(48, 7, 8, png)])).unwrap();
        let i = &a.frames[0][0];
        assert_eq!((i.width, i.hot_x, i.hot_y), (48, 7, 8));
        assert_eq!(&i.rgba[0..4], &[1, 2, 3, 200]);
    }

    #[test]
    fn garbage_is_refused_without_panic() {
        for data in [&b""[..], b"RIFF\x04\0\0\0ACON", b"\0\0\x02\0\x01\0", b"MZ\x90\0"] {
            assert!(parse_cursor_file(data).is_err(), "{data:?}");
        }
        // Битая запись с огромным смещением — ошибка, а не паника.
        let mut bad = cur32(32, 0, 0, 0);
        bad[18..22].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_cursor_file(&bad).is_err());
    }

    #[test]
    fn inf_registry_lines_beat_file_names() {
        // Как у BlueTransCursor: «Pointer.ani» стоит на месте прицела.
        let inf = r#"
[DefaultInstall]
AddReg    = Scheme.Reg
[Scheme.Reg]
HKCU,"Control Panel\Cursors\Schemes","%SCHEME_NAME%",,"%10%\%CUR_DIR%\%pointer%,%10%\%CUR_DIR%\%help%,%10%\%CUR_DIR%\%work%,%10%\%CUR_DIR%\%busy%,%10%\%CUR_DIR%\%cross%,%10%\%CUR_DIR%\%Text%,%10%\%CUR_DIR%\%Hand%,%10%\%CUR_DIR%\%unavailiable%"
[Strings]
CUR_DIR       = "Cursors\Blue"
SCHEME_NAME   = "Blue Trans"
pointer       = "Standard.ani"
help          = "Help.ani"
work          = "Work in Progress.ani"
busy          = "Loading.ani"
text          = "Text Select.cur"
unavailiable  = "No.cur"
cross         = "Pointer.ani"
hand          = "Pen.cur"
"#;
        let s = roles_from_inf(inf);
        assert_eq!(s.name.as_deref(), Some("Blue Trans"));
        let get = |r: Role| s.roles.iter().find(|(x, _)| *x == r).map(|(_, f)| f.as_str());
        assert_eq!(get(Role::Arrow), Some("Standard.ani"));
        assert_eq!(get(Role::Crosshair), Some("Pointer.ani"));
        assert_eq!(get(Role::NWPen), Some("Pen.cur"));
        assert_eq!(get(Role::No), Some("No.cur"));
        assert_eq!(get(Role::Hand), None);
    }

    #[test]
    fn inf_explicit_role_lines_win_and_cover_extras() {
        let inf = "[Wreg]\r\nHKCU,\"Control Panel\\Cursors\",Arrow,0x00020000,\"%10%\\%CUR_DIR%\\%pointer%\"\r\n\
HKCU,\"Control Panel\\Cursors\",Zoom-in,0x00020000,\"%10%\\%CUR_DIR%\\%zin%\"\r\n\
HKCU,\"Control Panel\\Cursors\",precisionhair,0x00020000,\"%10%\\%CUR_DIR%\\x.cur\"\r\n\
HKCU,\"Control Panel\\Cursors\\Schemes\",\"S\",,\"%10%\\a\\p.CUR,%10%\\a\\h.cur\"\r\n\
[Strings]\r\npointer = \"P.cur\"\r\nzin=\"Zoom-in.cur\"\r\n";
        let s = roles_from_inf(inf);
        assert_eq!(
            s.roles,
            vec![
                (Role::Arrow, "P.cur".into()),
                (Role::ZoomIn, "Zoom-in.cur".into()),
                (Role::Crosshair, "x.cur".into()),
                (Role::Help, "h.cur".into()),
            ]
        );
    }

    #[test]
    fn misordered_scheme_list_is_not_trusted() {
        // Как у macOS-Windows 2.0.1: в списке схемы после пера идут vert,
        // horz… (без «Недоступно» и диагоналей) — порядок Windows нарушен.
        let inf = r#"
[Scheme.Reg]
HKCU,"Control Panel\Cursors\Schemes","S",,"%10%\%pointer%,%10%\%help%,%10%\%work%,%10%\%busy%,%10%\%cross%,%10%\%text%,%10%\%handwriting%,%10%\%vert%,%10%\%horz%"
[Wreg]
HKCU,"Control Panel\Cursors",Arrow,0x00020000,"%10%\%pointer%"
HKCU,"Control Panel\Cursors",SizeNS,0x00020000,"%10%\%vert%"
[Strings]
pointer = "Pointer.cur"
help = "Help.cur"
vert = "Vert.cur"
horz = "Horz.cur"
unavailiable = "Unavailiable.cur"
dng1 = "Dng1.cur"
"#;
        let s = roles_from_inf(inf);
        let get = |r: Role| s.roles.iter().find(|(x, _)| *x == r).map(|(_, f)| f.as_str());
        assert_eq!(get(Role::No), Some("Unavailiable.cur"));
        assert_eq!(get(Role::SizeNWSE), Some("Dng1.cur"));
        assert_eq!(get(Role::SizeWE), Some("Horz.cur"));
        assert_eq!(get(Role::Help), Some("Help.cur"));
        assert_eq!(s.roles.iter().filter(|(_, f)| f == "Vert.cur").count(), 1);
    }

    #[test]
    fn inf_strings_only_template_maps_hand_to_pen() {
        let inf = "[Strings]\npointer=\"a.ani\"\nhand=\"b.cur\"\nlink=\"c.cur\"\nvert=\"v.cur\"\ndgn2=\"d.cur\"\n";
        let s = roles_from_inf(inf);
        assert_eq!(
            s.roles,
            vec![
                (Role::Arrow, "a.ani".into()),
                (Role::NWPen, "b.cur".into()),
                (Role::Hand, "c.cur".into()),
                (Role::SizeNS, "v.cur".into()),
                (Role::SizeNESW, "d.cur".into()),
            ]
        );
    }

    #[test]
    fn crs_sections_give_roles() {
        let crs = "\u{feff}[UpArrow]\r\nPath=alternate.cur\r\n\r\n[NWPen]\r\nPath=hand.cur\r\n[Hand]\r\nPath=link.cur\r\n[Bogus]\r\nPath=x.cur\r\n";
        assert_eq!(
            roles_from_crs(crs).roles,
            vec![
                (Role::UpArrow, "alternate.cur".into()),
                (Role::NWPen, "hand.cur".into()),
                (Role::Hand, "link.cur".into()),
            ]
        );
    }

    #[test]
    fn filenames_fallback_understands_rw_and_win11_names() {
        let files: Vec<String> = [
            "pointer.cur", "work.ani", "busy.ani", "vert.cur", "horz.cur", "dgn1.cur",
            "dgn2.cur", "unavailiable.cur", "hand.cur", "link.cur", "01 - Help Select.ani",
            "readme.txt",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let r = roles_from_filenames(&files);
        let get = |role: Role| r.iter().find(|(x, _)| *x == role).map(|(_, f)| f.as_str());
        assert_eq!(get(Role::Arrow), Some("pointer.cur"));
        assert_eq!(get(Role::AppStarting), Some("work.ani"));
        assert_eq!(get(Role::Wait), Some("busy.ani"));
        assert_eq!(get(Role::SizeNWSE), Some("dgn1.cur"));
        assert_eq!(get(Role::No), Some("unavailiable.cur"));
        assert_eq!(get(Role::NWPen), Some("hand.cur"));
        assert_eq!(get(Role::Hand), Some("link.cur"));
        assert_eq!(get(Role::Help), Some("01 - Help Select.ani"));
        // Без отдельного link «hand» — рука-ссылка.
        let r = roles_from_filenames(&["hand.cur".to_string()]);
        assert_eq!(r, vec![(Role::Hand, "hand.cur".to_string())]);
    }

    #[test]
    fn xcursor_names_do_not_overlap_between_roles() {
        let mut seen = std::collections::HashSet::new();
        for r in ALL_ROLES {
            for n in r.xcursor_names() {
                assert!(seen.insert(*n), "имя {n} у двух ролей");
            }
        }
    }

    #[test]
    fn text_encodings_are_decoded() {
        assert_eq!(decode_text(&[0xFF, 0xFE, b'a', 0, 0x1F, 0x04]), "aП");
        assert_eq!(decode_text(b"a\0b\0c\0d\0"), "abcd");
        assert_eq!(decode_text(&[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2, 0xA8]), "ПриветЁ");
        assert_eq!(decode_text("Привет".as_bytes()), "Привет");
    }

    #[test]
    fn preview_is_a_png_of_requested_size() {
        let a = parse_cursor_file(&cur32(32, 0, 0, 0)).unwrap();
        let png = preview_png(&[&a, &a, &a], 620, 348, 64).unwrap();
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!((img.width(), img.height()), (620, 348));
    }
}
