//! Отрисовка картинки плитки местной темы — исполняющий слой.
//!
//! Раскладка та же, что у вкладок: решения в ядре
//! (`rexon_core::local_preview::card_picture` — что нарисовать, вся
//! геометрия, цвета, какие файлы темы взять), исполнение здесь. Qt-примитивы
//! даёт `cpp/rexon_local_preview.cpp` — в cxx-qt-lib 0.9 нет ни QPainter
//! поверх устройства, ни drawText с прямоугольником, ни QSvgRenderer.
//!
//! Кадр рисуется в QImage, а не в QPixmap: так его можно рисовать вне потока
//! интерфейса. Первый показ раздела — это сотни кадров с чтением файлов тем,
//! и в потоке интерфейса окно на это время замирало.

use std::ffi::{c_char, c_void, CString};

use rexon_core::local_preview as lp;

unsafe extern "C" {
    fn rexon_lp_canvas_new(width: i32, height: i32, base_hex: *const c_char) -> *mut c_void;
    fn rexon_lp_fill(
        canvas: *mut c_void,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    );
    fn rexon_lp_gradient(
        canvas: *mut c_void,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        top_hex: *const c_char,
        bottom_hex: *const c_char,
    );
    #[allow(clippy::too_many_arguments)]
    fn rexon_lp_text(
        canvas: *mut c_void,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        text: *const c_char,
        point_size: i32,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    );
    fn rexon_lp_image_size(path: *const c_char, out_width: *mut i32, out_height: *mut i32) -> bool;
    #[allow(clippy::too_many_arguments)]
    fn rexon_lp_image(
        canvas: *mut c_void,
        path: *const c_char,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        cx: f64,
        cy: f64,
        cw: f64,
        ch: f64,
    );
    fn rexon_lp_image_tiled(canvas: *mut c_void, path: *const c_char, x: f64, y: f64, w: f64, h: f64);
    fn rexon_lp_svg_size(
        canvas: *mut c_void,
        path: *const c_char,
        element: *const c_char,
        out_width: *mut f64,
        out_height: *mut f64,
    ) -> bool;
    fn rexon_lp_svg(
        canvas: *mut c_void,
        path: *const c_char,
        element: *const c_char,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    );
    fn rexon_lp_pixels(canvas: *mut c_void, rgba: *const u8, width: i32, height: i32, x: f64, y: f64);
    #[allow(clippy::too_many_arguments)]
    fn rexon_lp_clip_push(
        canvas: *mut c_void,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        top_left: f64,
        top_right: f64,
        bottom_left: f64,
        bottom_right: f64,
    );
    fn rexon_lp_clip_pop(canvas: *mut c_void);
    #[allow(clippy::too_many_arguments)]
    fn rexon_lp_line(canvas: *mut c_void, x1: f64, y1: f64, x2: f64, y2: f64, width: f64, r: u8, g: u8, b: u8, a: u8);
    #[allow(clippy::too_many_arguments)]
    fn rexon_lp_arc(
        canvas: *mut c_void,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        start: f64,
        extent: f64,
        filled: bool,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    );
    #[allow(clippy::too_many_arguments)]
    fn rexon_lp_shade(canvas: *mut c_void, x: f64, y: f64, w: f64, h: f64, direction: i32, rgba: *const u8, count: i32);
    #[allow(clippy::too_many_arguments)]
    fn rexon_lp_picture(
        canvas: *mut c_void,
        path: *const c_char,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        tile: bool,
        alpha: f64,
        colorize: bool,
        r: u8,
        g: u8,
        b: u8,
    );
    #[allow(clippy::too_many_arguments)]
    fn rexon_lp_label(
        canvas: *mut c_void,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        text: *const c_char,
        pixel_size: i32,
        bold: bool,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    );
    fn rexon_lp_finish_png(canvas: *mut c_void, path: *const c_char) -> bool;
}

/// Картинка плитки (образец темы или заглушка вида, `lp::card_picture`)
/// файлом PNG. `false` — записать не вышло.
///
/// Можно звать из любого потока: кадр рисуется в QImage. Каталог назначения
/// создаётся здесь же — иначе первая же плитка молча не нарисовалась бы на
/// чистой машине. Файл появляется по пути `dest` сразу целым (запись идёт в
/// соседний файл, затем переименование): плитка не подхватит половину кадра,
/// и два потока, рисующие одну и ту же плитку, друг другу не мешают.
pub fn render_picture_to_png(picture: &lp::CardPicture, dest: &str) -> bool {
    let dest_path = std::path::Path::new(dest);
    if let Some(dir) = dest_path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return false;
        }
    }
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let part = format!("{dest}.{}-{seq}.part", std::process::id());
    let Ok(part_c) = CString::new(part.as_str()) else { return false };
    let canvas = paint_ops(picture.width, picture.height, picture.base, &picture.ops);
    let written = unsafe { rexon_lp_finish_png(canvas, part_c.as_ptr()) };
    if written && std::fs::rename(&part, dest_path).is_ok() {
        return true;
    }
    let _ = std::fs::remove_file(&part);
    false
}

/// Куда встаёт картинка `iw`×`ih` в прямоугольнике с сохранением сторон:
/// целиком по центру либо, при `cover`, во весь прямоугольник.
fn fitted(rect: lp::Rect, iw: f64, ih: f64, cover: bool) -> lp::Rect {
    let (x, y, w, h) = rect;
    let (sx, sy) = (w / iw, h / ih);
    let scale = if cover { sx.max(sy) } else { sx.min(sy) };
    let (dw, dh) = (iw * scale, ih * scale);
    (x + (w - dw) / 2.0, y + (h - dh) / 2.0, dw, dh)
}

/// Рамка из девяти частей рисунка SVG: углы в своём размере, стороны и
/// середина растянуты. Берётся первый из `prefixes`, чья середина есть в
/// файле; нет ни одного — рамка не рисуется.
///
/// # Safety
/// `canvas` — живой холст от `rexon_lp_canvas_new`.
unsafe fn svg_frame(canvas: *mut c_void, rect: lp::Rect, path: &CString, prefixes: &[String]) {
    let id_of = |prefix: &str, part: &str| {
        let name = if prefix.is_empty() { part.to_string() } else { format!("{prefix}-{part}") };
        CString::new(name).unwrap_or_default()
    };
    let size_of = |prefix: &str, part: &str| -> Option<(f64, f64)> {
        let (mut w, mut h) = (0.0, 0.0);
        unsafe { rexon_lp_svg_size(canvas, path.as_ptr(), id_of(prefix, part).as_ptr(), &mut w, &mut h) }
            .then_some((w, h))
    };
    let Some(prefix) = prefixes.iter().find(|p| size_of(p, "center").is_some()) else {
        return;
    };
    let id = |part: &str| id_of(prefix, part);
    let size = |part: &str| size_of(prefix, part);
    let (x, y, w, h) = rect;
    // Толщина каймы — по сторонам, а где стороны нет — по углу; не шире
    // половины рамки, иначе углы наехали бы друг на друга.
    let edge = |side: &str, corner: &str, pick: fn((f64, f64)) -> f64, limit: f64| {
        size(side).or_else(|| size(corner)).map(pick).unwrap_or(0.0).clamp(0.0, limit)
    };
    let left = edge("left", "topleft", |s| s.0, w / 2.0);
    let right = edge("right", "topright", |s| s.0, w / 2.0);
    let top = edge("top", "topleft", |s| s.1, h / 2.0);
    let bottom = edge("bottom", "bottomleft", |s| s.1, h / 2.0);
    let (mid_w, mid_h) = (w - left - right, h - top - bottom);
    let parts: [(&str, f64, f64, f64, f64); 9] = [
        ("center", x + left, y + top, mid_w, mid_h),
        ("top", x + left, y, mid_w, top),
        ("bottom", x + left, y + h - bottom, mid_w, bottom),
        ("left", x, y + top, left, mid_h),
        ("right", x + w - right, y + top, right, mid_h),
        ("topleft", x, y, left, top),
        ("topright", x + w - right, y, right, top),
        ("bottomleft", x, y + h - bottom, left, bottom),
        ("bottomright", x + w - right, y + h - bottom, right, bottom),
    ];
    for (part, px, py, pw, ph) in parts {
        if pw > 0.0 && ph > 0.0 && size(part).is_some() {
            unsafe { rexon_lp_svg(canvas, path.as_ptr(), id(part).as_ptr(), px, py, pw, ph) };
        }
    }
}

/// Холст с исполненными операциями ядра.
///
/// Возвращает ЖИВОЙ холст — закрыть его обязан вызывающий
/// (`rexon_lp_finish_png`).
fn paint_ops(width: i64, height: i64, base: &str, ops: &[lp::DrawOp]) -> *mut c_void {
    let base = CString::new(base).unwrap_or_default();
    unsafe {
        let canvas = rexon_lp_canvas_new(width as i32, height as i32, base.as_ptr());
        for op in ops {
            match op {
                lp::DrawOp::Fill {
                    rect: (x, y, w, h),
                    color: (r, g, b, a),
                } => rexon_lp_fill(canvas, *x, *y, *w, *h, *r, *g, *b, *a),
                lp::DrawOp::Gradient {
                    rect: (x, y, w, h),
                    top,
                    bottom,
                } => {
                    let top_c = CString::new(*top).unwrap_or_default();
                    let bottom_c = CString::new(*bottom).unwrap_or_default();
                    rexon_lp_gradient(
                        canvas,
                        *x,
                        *y,
                        *w,
                        *h,
                        top_c.as_ptr(),
                        bottom_c.as_ptr(),
                    );
                }
                lp::DrawOp::Text {
                    rect: (x, y, w, h),
                    text,
                    point_size,
                    color: (r, g, b, a),
                } => {
                    let text_c = CString::new(text.as_str()).unwrap_or_default();
                    rexon_lp_text(canvas, *x, *y, *w, *h, text_c.as_ptr(), *point_size as i32, *r, *g, *b, *a);
                }
                lp::DrawOp::Image { rect, path, cover } => {
                    let Ok(path_c) = CString::new(path.as_str()) else { continue };
                    let (mut iw, mut ih) = (0, 0);
                    if !rexon_lp_image_size(path_c.as_ptr(), &mut iw, &mut ih) {
                        continue;
                    }
                    let (dx, dy, dw, dh) = fitted(*rect, iw as f64, ih as f64, *cover);
                    rexon_lp_image(canvas, path_c.as_ptr(), dx, dy, dw, dh, rect.0, rect.1, rect.2, rect.3);
                }
                lp::DrawOp::Tiled { rect: (x, y, w, h), path } => {
                    let Ok(path_c) = CString::new(path.as_str()) else { continue };
                    rexon_lp_image_tiled(canvas, path_c.as_ptr(), *x, *y, *w, *h);
                }
                lp::DrawOp::SvgFrame { rect, path, prefixes } => {
                    let Ok(path_c) = CString::new(path.as_str()) else { continue };
                    svg_frame(canvas, *rect, &path_c, prefixes);
                }
                lp::DrawOp::SvgElement { rect: (x, y, w, h), path, element } => {
                    let (Ok(path_c), Ok(element_c)) =
                        (CString::new(path.as_str()), CString::new(element.as_str()))
                    else {
                        continue;
                    };
                    rexon_lp_svg(canvas, path_c.as_ptr(), element_c.as_ptr(), *x, *y, *w, *h);
                }
                lp::DrawOp::Pixels { x, y, width, height, rgba } => {
                    if rgba.len() == (*width as usize) * (*height as usize) * 4 {
                        rexon_lp_pixels(canvas, rgba.as_ptr(), *width as i32, *height as i32, *x, *y);
                    }
                }
                lp::DrawOp::Clip { rect: (x, y, w, h), radii } => {
                    rexon_lp_clip_push(canvas, *x, *y, *w, *h, radii[0], radii[1], radii[2], radii[3]);
                }
                lp::DrawOp::Unclip => rexon_lp_clip_pop(canvas),
                lp::DrawOp::Line { from, to, width, color: (r, g, b, a) } => {
                    rexon_lp_line(canvas, from.0, from.1, to.0, to.1, *width, *r, *g, *b, *a);
                }
                lp::DrawOp::Arc { rect: (x, y, w, h), start, extent, filled, color: (r, g, b, a) } => {
                    rexon_lp_arc(canvas, *x, *y, *w, *h, *start, *extent, *filled, *r, *g, *b, *a);
                }
                lp::DrawOp::Shade { rect: (x, y, w, h), direction, colors } => {
                    let bytes: Vec<u8> = colors.iter().flat_map(|c| [c.0, c.1, c.2, c.3]).collect();
                    let direction = match direction {
                        lp::ShadeDirection::Horizontal => 0,
                        lp::ShadeDirection::Vertical => 1,
                        lp::ShadeDirection::Diagonal => 2,
                    };
                    rexon_lp_shade(canvas, *x, *y, *w, *h, direction, bytes.as_ptr(), colors.len() as i32);
                }
                lp::DrawOp::Picture { rect: (x, y, w, h), path, tile, alpha, colorize } => {
                    let Ok(path_c) = CString::new(path.as_str()) else { continue };
                    let (r, g, b, _) = colorize.unwrap_or((0, 0, 0, 0));
                    rexon_lp_picture(canvas, path_c.as_ptr(), *x, *y, *w, *h, *tile, *alpha, colorize.is_some(), r, g, b);
                }
                lp::DrawOp::Label { rect: (x, y, w, h), text, pixel_size, bold, color: (r, g, b, a) } => {
                    let text_c = CString::new(text.as_str()).unwrap_or_default();
                    rexon_lp_label(canvas, *x, *y, *w, *h, text_c.as_ptr(), *pixel_size as i32, *bold, *r, *g, *b, *a);
                }
            }
        }
        canvas
    }
}
