//! Мини-карта экрана для виджетов Conky: положение из `conky.config`
//! (`alignment`, `gap_x`, `gap_y`, `xinerama_head`) ↔ прямоугольник окна
//! на мониторе — и обратно, когда человек перетащил прямоугольник.
//!
//! КАК CONKY СТАВИТ ОКНО (исходники conky 1.2x, `update_text_area`, и
//! справочник настроек; сверено 14.11.2026):
//! * рабочая область — весь монитор `xinerama_head` (по умолчанию 0; номер
//!   вне списка — весь экран X целиком). Панели НЕ вычитаются;
//! * по каждой оси выравнивание — начало, середина или конец:
//!   начало: `текст = край + gap`; конец: `текст = край − ширина текста − gap`;
//!   середина: `текст = середина − ширина текста / 2 − gap` (у середины
//!   положительный `gap` сдвигает к левому/верхнему краю);
//! * окно больше текста на рамку `b = border_inner_margin + border_outer_margin
//!   + border_width` (по умолчанию 3 + 1 + 1) с каждой стороны и стоит на
//!   `текст − b`;
//! * `gap_*` и рамка умножаются на масштаб `Xft.dpi / 96` (на X11);
//! * умолчания conky: `alignment = bottom_left`, `gap_x = 5`, `gap_y = 60`;
//!   `alignment = none` — окно ставит оконный менеджер.
//!
//! Wayland: conky 1.25 со слоями (KWin, wlroots, Hyprland, niri, COSMIC)
//! цепляет окно к краю по тому же выравниванию с отступами `gap_*`, монитор
//! выбирает сам стол (conky умеет только один вывод), поэтому на карте
//! виджеты стоят на первом экране и между экранами не переносятся. На
//! GNOME, Cinnamon и Pantheon под Wayland слоёв нет — окно ставит стол,
//! карта честно говорит, что двигать нечем.
//!
//! Размер окна: точный — у запущенного из Rexon виджета на X11 (окно из
//! журнала conky); иначе — по снимку плитки невидимого экрана
//! (`conky_preview`: поля известны), иначе — грубо по `minimum_width/height`.
//! На карте приблизительный размер помечен.
//!
//! Запись — тем же путём, что числа в настройках карточки: накладка
//! `conky_overlay` (файл автора не трогается, «Как у автора» снимает всё),
//! одно поле `position` пишет выравнивание, отступы и экран разом, чтобы
//! conky перечитал накладку один раз.

use std::collections::BTreeMap;

use crate::animations::LookIo;
use crate::conky_overlay::{self as ov, Overrides, Value};
use crate::de_apply::Ctx;
use crate::i18n::{tr, trf};

/// Умолчания conky.
pub const DEFAULT_ALIGNMENT: &str = "bottom_left";
pub const DEFAULT_GAP_X: i64 = 5;
pub const DEFAULT_GAP_Y: i64 = 60;
/// `border_inner_margin` + `border_outer_margin` + `border_width` по
/// умолчанию.
pub const DEFAULT_BORDER: i64 = 3 + 1 + 1;
/// Прилипание к краю и середине — ближе стольких точек экрана.
pub const SNAP_PX: i32 = 24;
/// Размер окна, когда о нём ничего не известно.
pub const GUESS_SIZE: (i32, i32) = (220, 160);
/// Отступ — в тех же пределах, что у поля в настройках.
const GAP_LIMIT: i64 = 10000;

// ════════════════════════════════════════════════════════════════════════════
//  Геометрия
// ════════════════════════════════════════════════════════════════════════════

/// Прямоугольник в точках экрана.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn centre(&self) -> (i64, i64) {
        (self.x as i64 * 2 + self.w as i64, self.y as i64 * 2 + self.h as i64)
    }
    fn overlap(&self, o: &Rect) -> i64 {
        let w = (self.x + self.w).min(o.x + o.w) - self.x.max(o.x);
        let h = (self.y + self.h).min(o.y + o.h) - self.y.max(o.y);
        if w > 0 && h > 0 { w as i64 * h as i64 } else { 0 }
    }
}

/// Монитор на карте.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Area {
    /// Номер для `xinerama_head`; `None` — номер conky не понимает
    /// (Wayland, весь экран).
    pub head: Option<u32>,
    pub name: String,
    pub rect: Rect,
    pub primary: bool,
}

/// Масштаб и рамка окна виджета.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// `Xft.dpi / 96` на X11, 1 на Wayland.
    pub scale: f64,
    /// Рамка в единицах файла настроек (до масштаба).
    pub border: i64,
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics { scale: 1.0, border: DEFAULT_BORDER }
    }
}

impl Metrics {
    fn px(&self, v: i64) -> i64 {
        (v as f64 * self.scale).round() as i64
    }
    fn units(&self, px: i64) -> i64 {
        let s = if self.scale > 0.0 { self.scale } else { 1.0 };
        ((px as f64 / s).round() as i64).clamp(-GAP_LIMIT, GAP_LIMIT)
    }
}

/// Положение в словах conky.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Placement {
    /// Полное имя выравнивания (`top_right`).
    pub alignment: String,
    pub gap_x: i64,
    pub gap_y: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Start,
    Middle,
    End,
}

/// Оси выравнивания: (по ширине, по высоте).
fn axes(alignment: &str) -> (Axis, Axis) {
    let full = ov::normalize_alignment(alignment).unwrap_or(DEFAULT_ALIGNMENT);
    let (v, h) = full.split_once('_').unwrap_or(("bottom", "left"));
    let h = match h {
        "left" => Axis::Start,
        "right" => Axis::End,
        _ => Axis::Middle,
    };
    let v = match v {
        "top" => Axis::Start,
        "bottom" => Axis::End,
        _ => Axis::Middle,
    };
    (h, v)
}

fn alignment_name(h: Axis, v: Axis) -> String {
    let v = match v {
        Axis::Start => "top",
        Axis::Middle => "middle",
        Axis::End => "bottom",
    };
    let h = match h {
        Axis::Start => "left",
        Axis::Middle => "middle",
        Axis::End => "right",
    };
    format!("{v}_{h}")
}

/// Начало окна по одной оси: `start`/`len` — рабочая область, `size` —
/// окно целиком, `gap` и `b` — в точках.
fn window_start(axis: Axis, start: i64, len: i64, size: i64, gap: i64, b: i64) -> i64 {
    let text = (size - 2 * b).max(0);
    let text_start = match axis {
        Axis::Start => start + gap,
        Axis::End => start + len - text - gap,
        Axis::Middle => start + len / 2 - text / 2 - gap,
    };
    text_start - b
}

/// Отступ по одной оси для окна, начинающегося в `pos` (обратное
/// [`window_start`]), в точках.
fn gap_px(axis: Axis, start: i64, len: i64, size: i64, pos: i64, b: i64) -> i64 {
    let text = (size - 2 * b).max(0);
    let text_start = pos + b;
    match axis {
        Axis::Start => text_start - start,
        Axis::End => start + len - text - text_start,
        Axis::Middle => start + len / 2 - text / 2 - text_start,
    }
}

/// Где conky поставит окно размера `size` с положением `p` на мониторе
/// `area`.
pub fn rect_of(area: &Rect, p: &Placement, size: (i32, i32), m: &Metrics) -> Rect {
    let (h, v) = axes(&p.alignment);
    let b = m.px(m.border);
    let x = window_start(h, area.x as i64, area.w as i64, size.0 as i64, m.px(p.gap_x), b);
    let y = window_start(v, area.y as i64, area.h as i64, size.1 as i64, m.px(p.gap_y), b);
    Rect { x: x as i32, y: y as i32, w: size.0, h: size.1 }
}

/// Треть, в которой лежит середина окна: так выбирается угол или край,
/// ближайший к месту, куда окно положили.
fn third(start: i64, len: i64, centre2: i64) -> Axis {
    // centre2 — удвоенная середина: без дробей.
    let rel = centre2 - 2 * start;
    if rel * 3 < 2 * len {
        Axis::Start
    } else if rel * 3 >= 4 * len {
        Axis::End
    } else {
        Axis::Middle
    }
}

/// Положение conky для окна `r` на мониторе `area`: угол или край — по
/// трети экрана, где середина окна, отступы — от этого угла или края.
pub fn placement_of(area: &Rect, r: &Rect, m: &Metrics) -> Placement {
    let (cx, cy) = r.centre();
    let h = third(area.x as i64, area.w as i64, cx);
    let v = third(area.y as i64, area.h as i64, cy);
    let b = m.px(m.border);
    let gx = gap_px(h, area.x as i64, area.w as i64, r.w as i64, r.x as i64, b);
    let gy = gap_px(v, area.y as i64, area.h as i64, r.h as i64, r.y as i64, b);
    Placement { alignment: alignment_name(h, v), gap_x: m.units(gx), gap_y: m.units(gy) }
}

/// Монитор, на котором окно `r`: с наибольшим перекрытием, без перекрытия
/// — с ближайшей серединой. `None` — мониторов нет.
pub fn area_of(areas: &[Area], r: &Rect) -> Option<usize> {
    let best = areas.iter().enumerate().max_by_key(|(i, a)| (a.rect.overlap(r), -(*i as i64)))?;
    if best.1.rect.overlap(r) > 0 {
        return Some(best.0);
    }
    let (cx, cy) = r.centre();
    areas
        .iter()
        .enumerate()
        .min_by_key(|(_, a)| {
            let (ax, ay) = a.rect.centre();
            (ax - cx).pow(2) + (ay - cy).pow(2)
        })
        .map(|(i, _)| i)
}

/// Поставить окно в пределы монитора и, если `snap`, прилепить к краю или
/// середине, до которых ближе [`SNAP_PX`].
pub fn settle(area: &Rect, r: &Rect, snap: bool) -> Rect {
    fn axis(start: i32, len: i32, pos: i32, size: i32, snap: bool) -> i32 {
        let max = start + (len - size).max(0);
        let pos = pos.clamp(start, max);
        if !snap {
            return pos;
        }
        let centre = start + (len - size).max(0) / 2;
        [start, centre, max]
            .into_iter()
            .map(|c| ((c - pos).abs(), c))
            .filter(|(d, _)| *d <= SNAP_PX)
            .min()
            .map_or(pos, |(_, c)| c)
    }
    Rect {
        x: axis(area.x, area.w, r.x, r.w, snap),
        y: axis(area.y, area.h, r.y, r.h, snap),
        w: r.w,
        h: r.h,
    }
}

/// Рамка вокруг всех мониторов (весь экран X).
pub fn bounds(areas: &[Area]) -> Rect {
    let Some(first) = areas.first() else { return Rect::default() };
    let (mut x0, mut y0) = (first.rect.x, first.rect.y);
    let (mut x1, mut y1) = (first.rect.x + first.rect.w, first.rect.y + first.rect.h);
    for a in areas {
        x0 = x0.min(a.rect.x);
        y0 = y0.min(a.rect.y);
        x1 = x1.max(a.rect.x + a.rect.w);
        y1 = y1.max(a.rect.y + a.rect.h);
    }
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

// ════════════════════════════════════════════════════════════════════════════
//  Виджет на карте: поля автора + выбор человека
// ════════════════════════════════════════════════════════════════════════════

/// Откуда известен размер окна.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SizeFrom {
    /// Окно запущенного виджета — точно.
    Window,
    /// Снимок плитки — почти точно.
    Picture,
    /// `minimum_width/height` или ничего — примерно.
    Guess,
}

/// Положение виджета, как оно действует: `(положение, номер монитора conky,
/// неизвестно ли)`. Неизвестно — у автора выражение или `none`; тогда
/// показываются умолчания conky.
pub fn effective(cfg: &BTreeMap<String, Value>, o: &Overrides) -> (Placement, Option<i64>, bool) {
    let mut unknown = false;
    let alignment = match (&o.alignment, cfg.get("alignment")) {
        (Some(a), _) => a.clone(),
        (None, Some(Value::Str(s))) => match ov::normalize_alignment(s) {
            Some(a) => a.to_string(),
            None => {
                unknown = true; // `none`: окно ставит оконный менеджер
                DEFAULT_ALIGNMENT.to_string()
            }
        },
        (None, Some(_)) => {
            unknown = true;
            DEFAULT_ALIGNMENT.to_string()
        }
        (None, None) => DEFAULT_ALIGNMENT.to_string(),
    };
    let mut num = |own: Option<i64>, key: &str, default: i64| match (own, cfg.get(key)) {
        (Some(v), _) => v,
        (None, None) => default,
        (None, Some(v)) => v.as_i64().unwrap_or_else(|| {
            unknown = true;
            default
        }),
    };
    let gap_x = num(o.gap_x, "gap_x", DEFAULT_GAP_X);
    let gap_y = num(o.gap_y, "gap_y", DEFAULT_GAP_Y);
    let head = match (o.xinerama_head, cfg.get("xinerama_head")) {
        (Some(h), _) => Some(h as i64),
        (None, Some(v)) => v.as_i64(),
        (None, None) => None,
    };
    (Placement { alignment, gap_x, gap_y }, head, unknown)
}

/// Рамка окна из полей автора (выражение — умолчание conky).
pub fn border_of(cfg: &BTreeMap<String, Value>) -> i64 {
    [("border_inner_margin", 3), ("border_outer_margin", 1), ("border_width", 1)]
        .iter()
        .map(|(k, d)| cfg.get(*k).and_then(Value::as_i64).filter(|v| *v >= 0).unwrap_or(*d))
        .sum()
}

/// Грубый размер окна по `minimum_width/height` (в точках, с рамкой).
pub fn guess_size(cfg: &BTreeMap<String, Value>, o: &Overrides, m: &Metrics) -> (i32, i32) {
    let get = |own: Option<i64>, key: &str| own.or_else(|| cfg.get(key).and_then(Value::as_i64)).unwrap_or(0).max(0);
    let b = 2 * m.px(m.border);
    let w = m.px(get(o.min_width, "minimum_width")) + b;
    let h = m.px(get(o.min_height, "minimum_height")) + b;
    (w.max(GUESS_SIZE.0 as i64) as i32, h.max(GUESS_SIZE.1 as i64) as i32)
}

/// Размер окна по снимку плитки `w×h`: плитка — окно плюс поля
/// `conky_preview::CARD_MARGIN`, если не уменьшена. У края экрана поле
/// обрезано — размер выйдет чуть больше настоящего.
pub fn size_from_card(w: u32, h: u32) -> Option<(i32, i32)> {
    let m = crate::conky_preview::CARD_MARGIN;
    if w.max(h) >= crate::conky_preview::CARD_MAX || w <= 2 * m + 4 || h <= 2 * m + 4 {
        return None;
    }
    Some(((w - 2 * m) as i32, (h - 2 * m) as i32))
}

/// Монитор, на котором conky ставит виджет: `head` conky (нет — 0, вне
/// списка — весь экран). На Wayland — первый экран.
pub fn home_area(areas: &[Area], head: Option<i64>, wayland: bool) -> Option<Rect> {
    if wayland {
        return areas.first().map(|a| a.rect);
    }
    let head = head.unwrap_or(0);
    match areas.iter().find(|a| a.head.is_some_and(|h| h as i64 == head)) {
        Some(a) => Some(a.rect),
        None if areas.is_empty() => None,
        None => Some(bounds(areas)),
    }
}

/// Значение поля `position` для [`ov::set_value`] — выбор человека для
/// четырёх полей положения (null — как у автора).
pub fn position_value(o: &Overrides) -> serde_json::Value {
    serde_json::json!({
        "alignment": o.alignment,
        "gap_x": o.gap_x,
        "gap_y": o.gap_y,
        "xinerama_head": o.xinerama_head,
    })
}

/// Куда перетащили: монитор, окно на нём (в пределах, с прилипанием) и
/// новое значение поля `position`.
///
/// `fixed` — монитор менять нельзя (Wayland): окно остаётся на `home`.
/// Номер монитора пишется, только если мониторов X11 больше одного.
pub fn drop_to(areas: &[Area], home: Option<Rect>, fixed: bool, r: &Rect, snap: bool, m: &Metrics, o: &Overrides) -> Option<(Rect, serde_json::Value)> {
    let (area, head) = if fixed {
        (home?, None)
    } else {
        let i = area_of(areas, r)?;
        (areas[i].rect, areas[i].head)
    };
    let placed = settle(&area, r, snap);
    let p = placement_of(&area, &placed, m);
    let mut next = o.clone();
    next.alignment = Some(p.alignment);
    next.gap_x = Some(p.gap_x);
    next.gap_y = Some(p.gap_y);
    if !fixed && areas.iter().filter(|a| a.head.is_some()).count() > 1 {
        next.xinerama_head = head;
    }
    Some((placed, position_value(&next)))
}

// ════════════════════════════════════════════════════════════════════════════
//  Для окна: карта целиком
// ════════════════════════════════════════════════════════════════════════════

/// Виджет на карте.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MapWidget {
    pub item_id: String,
    pub title: String,
    pub conf: String,
    pub rect: Rect,
    pub size_from: SizeFrom,
    pub running: bool,
    /// Виджет открытой карточки.
    pub focus: bool,
    /// Где стоит, не известно (выражение у автора или `none`) — показан
    /// по умолчаниям conky; перетаскивание задаст своё.
    pub unknown: bool,
    /// Положение сейчас — для «Вернуть» ([`position_value`]).
    pub saved: serde_json::Value,
    pub tooltip: String,
}

/// Подписи мини-карты.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MapTexts {
    pub title: String,
    pub hint: String,
    pub revert: String,
    pub revert_tooltip: String,
    pub guess: String,
    pub unknown: String,
    pub empty: String,
}

pub fn map_texts() -> MapTexts {
    MapTexts {
        title: tr("Where the widgets are on the screen"),
        hint: tr(
            "Drag a widget to move it; it sticks to the edges and the middle. Arrow keys move the chosen widget \
             (with Shift — by one point).",
        ),
        revert: tr("Undo moves"),
        revert_tooltip: tr("Put the widgets moved here back where they were before."),
        guess: tr("The size is approximate: Rexon will know it exactly once the widget is running."),
        unknown: tr("The widget works out its place itself — Rexon shows a guess. Move it to set your own place."),
        empty: tr("No widgets on the screen yet. Start a widget to see it here."),
    }
}

/// Мини-карта.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MapView {
    /// Двигать можно.
    pub available: bool,
    /// Почему нет.
    pub reason: String,
    pub areas: Vec<Area>,
    pub bounds: Rect,
    pub widgets: Vec<MapWidget>,
    /// Монитор менять нельзя (Wayland).
    pub fixed_area: bool,
    /// Оговорка под картой.
    pub note: Option<String>,
    pub t: MapTexts,
}

/// Почему на этом столе двигать нечем; `None` — можно.
pub fn unavailable_reason(ctx: &Ctx) -> Option<String> {
    (ctx.wayland && ov::wayland_without_layers(&ctx.de)).then(|| {
        tr("On this desktop under Wayland the desktop itself decides where the Conky window goes, so it can't be moved here.")
    })
}

/// Экран для карты со слов окна Rexon: `[{name, x, y, width, height}]`
/// в логических точках (`Qt.application.screens`).
pub fn parse_gui_screens(json: &str) -> Vec<Area> {
    let Ok(serde_json::Value::Array(list)) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let num = |v: &serde_json::Value, k: &str| v.get(k).and_then(serde_json::Value::as_f64).map(|n| n.round() as i32);
    list.iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let (w, h) = (num(s, "width")?, num(s, "height")?);
            (w >= 100 && h >= 100).then(|| Area {
                head: None,
                name: s.get("name").and_then(serde_json::Value::as_str).unwrap_or("").to_string(),
                rect: Rect { x: num(s, "x").unwrap_or(0), y: num(s, "y").unwrap_or(0), w, h },
                primary: i == 0,
            })
        })
        .collect()
}

/// Мониторы карты: на X11 — `xrandr` (номера conky), иначе — окно Rexon.
pub fn areas(io: &dyn LookIo, ctx: &Ctx, gui: &[Area]) -> Vec<Area> {
    if !ctx.wayland {
        let mons = ov::monitors(io, false);
        if !mons.is_empty() {
            return mons
                .into_iter()
                .map(|m| Area {
                    head: Some(m.index),
                    name: m.name,
                    rect: Rect { x: m.x, y: m.y, w: m.width as i32, h: m.height as i32 },
                    primary: m.primary,
                })
                .collect();
        }
    }
    gui.to_vec()
}

/// Масштаб conky на X11: `Xft.dpi / 96`; не узнать — 1.
pub fn scale(io: &dyn LookIo, ctx: &Ctx) -> f64 {
    if ctx.wayland || !io.have("xrdb") {
        return 1.0;
    }
    let (ok, out) = io.run(&["xrdb".to_string(), "-query".to_string()]);
    ok.then(|| crate::conky_preview::parse_xft_dpi(&out)).flatten().map_or(1.0, |d| d as f64 / 96.0)
}

/// Чем узнать размер окна — подставляется (живой X11 / подделка в тестах).
pub type SizeProbe<'a> = &'a dyn Fn(&str, bool) -> Option<((i32, i32), SizeFrom)>;

/// Один файл настроек на карте. `None` — поля автора не прочитать.
#[allow(clippy::too_many_arguments)]
pub fn map_widget(
    io: &dyn LookIo,
    ctx: &Ctx,
    areas: &[Area],
    scale: f64,
    item_id: &str,
    title: &str,
    conf: &str,
    running: bool,
    focus: bool,
    probe: SizeProbe,
) -> Option<MapWidget> {
    let text = io.read(conf)?;
    let (_, cfg) = ov::author_config(&text);
    let cfg = cfg?;
    let o = ov::load(io, &ov::live_paths(ctx, conf));
    let m = Metrics { scale, border: border_of(&cfg) };
    let (p, head, unknown) = effective(&cfg, &o);
    let (size, size_from) = probe(conf, running).unwrap_or_else(|| (guess_size(&cfg, &o, &m), SizeFrom::Guess));
    let area = home_area(areas, head, ctx.wayland)?;
    let rect = rect_of(&area, &p, size, &m);
    let tooltip = trf("“%1”: %2, %3 × %4 from the edge", &[&title, &align_label(&p.alignment), &p.gap_x, &p.gap_y]);
    Some(MapWidget {
        item_id: item_id.to_string(),
        title: title.to_string(),
        conf: conf.to_string(),
        rect,
        size_from,
        running,
        focus,
        unknown,
        saved: position_value(&o),
        tooltip,
    })
}

/// Подпись угла или края — та же, что у сетки в настройках.
fn align_label(alignment: &str) -> String {
    ov::align_label(alignment).unwrap_or_else(|| alignment.to_string())
}

/// Карта: запущенные виджеты `running` (`(id плитки, название, файл)`) и
/// файл открытой карточки `focus`.
#[allow(clippy::too_many_arguments)]
pub fn map_view(
    io: &dyn LookIo,
    ctx: &Ctx,
    areas: Vec<Area>,
    scale: f64,
    running: &[(String, String, String)],
    focus: Option<(String, String, String)>,
    probe: SizeProbe,
) -> MapView {
    let mut v = MapView {
        available: false,
        reason: String::new(),
        bounds: bounds(&areas),
        areas,
        widgets: Vec::new(),
        fixed_area: ctx.wayland,
        note: ctx.wayland.then(|| tr("Under Wayland the desktop chooses the screen for Conky, so widgets stay on one screen.")),
        t: map_texts(),
    };
    if let Some(why) = unavailable_reason(ctx) {
        v.reason = why;
        return v;
    }
    if v.areas.is_empty() {
        v.reason = tr("Rexon couldn't find out the size of the screen.");
        return v;
    }
    let mut seen: Vec<String> = Vec::new();
    let focus_conf = focus.as_ref().map(|f| f.2.clone());
    for (id, title, conf) in running.iter().cloned().chain(focus.into_iter()) {
        if seen.contains(&conf) {
            continue;
        }
        let is_running = running.iter().any(|r| r.2 == conf);
        let is_focus = focus_conf.as_deref() == Some(conf.as_str());
        if let Some(w) = map_widget(io, ctx, &v.areas, scale, &id, &title, &conf, is_running, is_focus, probe) {
            v.widgets.push(w);
        }
        seen.push(conf);
    }
    v.available = true;
    v
}

// ════════════════════════════════════════════════════════════════════════════
//  Живое
// ════════════════════════════════════════════════════════════════════════════

/// Размер видимого окна X11 `window` на дисплее сеанса.
pub fn live_window_size(window: u64) -> Option<(i32, i32)> {
    use x11_dl::xlib;
    let lib = xlib::Xlib::open().ok()?;
    // SAFETY: дисплей открывается и закрывается здесь же; ошибка «нет
    // такого окна» гасится своим обработчиком.
    unsafe {
        let dpy = (lib.XOpenDisplay)(std::ptr::null());
        if dpy.is_null() {
            return None;
        }
        unsafe extern "C" fn quiet(_: *mut xlib::Display, _: *mut xlib::XErrorEvent) -> i32 {
            0
        }
        let prev = (lib.XSetErrorHandler)(Some(quiet));
        let mut a: xlib::XWindowAttributes = std::mem::zeroed();
        let ok = (lib.XGetWindowAttributes)(dpy, window as xlib::Window, &mut a) != 0;
        (lib.XSync)(dpy, 0);
        (lib.XSetErrorHandler)(prev);
        (lib.XCloseDisplay)(dpy);
        (ok && a.map_state == xlib::IsViewable && a.width > 0 && a.height > 0).then_some((a.width, a.height))
    }
}

/// Размер окна виджета вживую: окно из журнала (X11, запущен из Rexon),
/// иначе снимок плитки.
pub fn live_size(ctx: &Ctx, conf: &str, running: bool) -> Option<((i32, i32), SizeFrom)> {
    if running && !ctx.wayland {
        let log = std::fs::read(crate::conky_log::live_log_path(ctx, conf)).ok();
        let win = log.and_then(|b| crate::conky_preview::window_of_log(&String::from_utf8_lossy(&b)));
        if let Some(size) = win.and_then(live_window_size) {
            return Some((size, SizeFrom::Window));
        }
    }
    let png = crate::conky_preview::cached_one(ctx, conf)?;
    let (w, h) = image::image_dimensions(&png).ok()?;
    size_from_card(w, h).map(|s| (s, SizeFrom::Picture))
}

/// Запущенные виджеты для карты: `(id плитки, название, файл)`.
fn live_running(io: &dyn LookIo, ctx: &Ctx) -> Vec<(String, String, String)> {
    let extra = crate::conky_scan::extra_dirs_for(ctx);
    crate::conky_procs::running_confs(io, ctx)
        .into_iter()
        .map(|conf| {
            let name = crate::conky_sets::widget_name_of(ctx, &extra, &conf);
            (format!("local:conky:{name}"), crate::conky_scan::widget_title(&name), conf)
        })
        .collect()
}

/// Живая карта: запущенные виджеты и файл `focus_conf` виджета
/// `focus_name` (пусто — без карточки). `gui_screens` — экраны со слов
/// окна Rexon ([`parse_gui_screens`]).
pub fn live_map(focus_name: &str, focus_conf: &str, gui_screens: &str) -> MapView {
    let io = crate::animations::SystemIo;
    let ctx = Ctx::live();
    let areas = areas(&io, &ctx, &parse_gui_screens(gui_screens));
    let s = scale(&io, &ctx);
    let running = live_running(&io, &ctx);
    let focus = (!focus_name.is_empty() && !focus_conf.is_empty()).then(|| {
        (format!("local:conky:{focus_name}"), crate::conky_scan::widget_title(focus_name), focus_conf.to_string())
    });
    let probe = |conf: &str, running: bool| live_size(&ctx, conf, running);
    map_view(&io, &ctx, areas, s, &running, focus, &probe)
}

/// Человек положил виджет (файл `conf` виджета `name`) в `r` (точки
/// экрана): выбрать угол или край, пересчитать отступы, записать накладку
/// и применить — как числа в настройках.
pub fn live_move(name: &str, conf: &str, r: Rect, snap: bool, gui_screens: &str) -> (bool, String) {
    let io = crate::animations::SystemIo;
    let ctx = Ctx::live();
    if let Some(why) = unavailable_reason(&ctx) {
        return (false, why);
    }
    let areas = areas(&io, &ctx, &parse_gui_screens(gui_screens));
    let Some(cfg) = io.read(conf).and_then(|t| ov::author_config(&t).1) else {
        return (false, tr("Rexon can't read this widget's settings file, so it can't change them."));
    };
    let o = ov::load(&io, &ov::live_paths(&ctx, conf));
    let m = Metrics { scale: scale(&io, &ctx), border: border_of(&cfg) };
    let (_, head, _) = effective(&cfg, &o);
    let home = home_area(&areas, head, ctx.wayland);
    let Some((_, value)) = drop_to(&areas, home, ctx.wayland, &r, snap, &m, &o) else {
        return (false, tr("Rexon couldn't find out the size of the screen."));
    };
    ov::live_set(name, conf, "position", &value)
}

/// «Вернуть»: положение файла `conf` — как было (`saved` из карты).
pub fn live_restore(name: &str, conf: &str, saved: &serde_json::Value) -> (bool, String) {
    ov::live_set(name, conf, "position", saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { x: 0, y: 0, w: 1920, h: 1080 };

    fn p(a: &str, x: i64, y: i64) -> Placement {
        Placement { alignment: a.to_string(), gap_x: x, gap_y: y }
    }

    #[test]
    fn rect_follows_conky_formula() {
        let m = Metrics::default(); // рамка 5
        // Левый верх: окно = gap − рамка.
        assert_eq!(rect_of(&SCREEN, &p("top_left", 20, 30), (200, 100), &m), Rect { x: 15, y: 25, w: 200, h: 100 });
        // Правый низ: правый край окна = край − gap + рамка.
        let r = rect_of(&SCREEN, &p("br", 20, 30), (200, 100), &m);
        assert_eq!((r.x + r.w, r.y + r.h), (1920 - 20 + 5, 1080 - 30 + 5));
        // Середина: положительный gap — влево/вверх, рамка не влияет.
        let r = rect_of(&SCREEN, &p("middle_middle", 10, 0), (200, 100), &m);
        assert_eq!((r.x, r.y), (960 - 100 - 10, 540 - 50));
        // Масштаб 2: gap и рамка вдвое.
        let m2 = Metrics { scale: 2.0, border: 5 };
        assert_eq!(rect_of(&SCREEN, &p("top_left", 20, 30), (200, 100), &m2), Rect { x: 30, y: 50, w: 200, h: 100 });
    }

    #[test]
    fn placement_round_trips_on_every_alignment() {
        let mons = [SCREEN, Rect { x: 1920, y: -200, w: 2560, h: 1440 }];
        for area in mons {
            for a in ov::ALIGNMENTS {
                for m in [Metrics::default(), Metrics { scale: 1.0, border: 0 }] {
                    let r = rect_of(&area, &p(a, 40, 70), (300, 180), &m);
                    let back = placement_of(&area, &r, &m);
                    assert_eq!(rect_of(&area, &back, (300, 180), &m), r, "{a}");
                    assert_eq!(back.alignment, a, "окно у своего угла — тот же угол");
                    assert_eq!((back.gap_x, back.gap_y), (40, 70), "{a}");
                }
            }
        }
    }

    #[test]
    fn placement_picks_nearest_corner_and_rounds_with_scale() {
        let m = Metrics { scale: 1.5, border: 0 };
        // Правая треть, низ: правый нижний угол, отступы от него.
        let r = Rect { x: 1600, y: 900, w: 200, h: 100 };
        let pl = placement_of(&SCREEN, &r, &m);
        assert_eq!(pl.alignment, "bottom_right");
        assert_eq!((pl.gap_x, pl.gap_y), ((120.0f64 / 1.5).round() as i64, (80.0f64 / 1.5).round() as i64));
        // Середина экрана — центр, отступы ноль.
        let r = Rect { x: 860, y: 490, w: 200, h: 100 };
        assert_eq!(placement_of(&SCREEN, &r, &Metrics { scale: 1.0, border: 0 }), p("middle_middle", 0, 0));
        // Левая треть посередине по высоте.
        let r = Rect { x: 10, y: 500, w: 100, h: 50 };
        assert_eq!(placement_of(&SCREEN, &r, &m).alignment, "middle_left");
    }

    #[test]
    fn settle_clamps_and_snaps() {
        let r = Rect { x: -50, y: 2000, w: 200, h: 100 };
        assert_eq!(settle(&SCREEN, &r, false), Rect { x: 0, y: 980, w: 200, h: 100 });
        // Близко к краю — прилип; далеко — нет; близко к середине — в середину.
        assert_eq!(settle(&SCREEN, &Rect { x: 15, y: 300, w: 200, h: 100 }, true).x, 0);
        assert_eq!(settle(&SCREEN, &Rect { x: 1700, y: 300, w: 200, h: 100 }, true).x, 1720);
        assert_eq!(settle(&SCREEN, &Rect { x: 100, y: 300, w: 200, h: 100 }, true).x, 100);
        assert_eq!(settle(&SCREEN, &Rect { x: 870, y: 300, w: 200, h: 100 }, true).x, 860);
        // Без прилипания (клавиатура) — точно куда сказали.
        assert_eq!(settle(&SCREEN, &Rect { x: 15, y: 300, w: 200, h: 100 }, false).x, 15);
        // Окно больше экрана — к началу.
        assert_eq!(settle(&SCREEN, &Rect { x: 100, y: 0, w: 3000, h: 100 }, true).x, 0);
    }

    fn two_heads() -> Vec<Area> {
        vec![
            Area { head: Some(0), name: "eDP-1".into(), rect: SCREEN, primary: true },
            Area { head: Some(1), name: "HDMI-A-1".into(), rect: Rect { x: 1920, y: -200, w: 2560, h: 1440 }, primary: false },
        ]
    }

    #[test]
    fn areas_overlap_head_and_bounds() {
        let a = two_heads();
        assert_eq!(area_of(&a, &Rect { x: 1800, y: 0, w: 300, h: 100 }), Some(1), "больше на втором");
        assert_eq!(area_of(&a, &Rect { x: 5000, y: 0, w: 100, h: 100 }), Some(1), "вне всех — ближайший");
        assert_eq!(area_of(&[], &SCREEN), None);
        assert_eq!(home_area(&a, None, false), Some(SCREEN), "по умолчанию — номер 0");
        assert_eq!(home_area(&a, Some(1), false), Some(a[1].rect));
        assert_eq!(home_area(&a, Some(7), false), Some(Rect { x: 0, y: -200, w: 4480, h: 1440 }), "вне списка — весь экран");
        assert_eq!(home_area(&a, Some(1), true), Some(SCREEN), "Wayland — первый экран");
    }

    #[test]
    fn drop_writes_head_only_with_several_monitors() {
        let a = two_heads();
        let m = Metrics::default();
        let o = Overrides { bg_none: Some(true), ..Default::default() };
        let r = Rect { x: 1920 + 2560 - 210, y: -200 + 10, w: 200, h: 100 };
        let (placed, v) = drop_to(&a, None, false, &r, true, &m, &o).unwrap();
        assert_eq!(placed.x, 1920 + 2560 - 200, "прилип к правому краю второго");
        assert_eq!(placed.y, -200, "и к верхнему");
        assert_eq!(v["alignment"], "top_right");
        assert_eq!(v["xinerama_head"], 1);
        assert_eq!((v["gap_x"].as_i64(), v["gap_y"].as_i64()), (Some(5), Some(5)), "край окна = край экрана: gap = рамка");
        // Один монитор — номер не пишется; Wayland — экран не меняется.
        let (_, v) = drop_to(&a[..1], None, false, &Rect { x: 0, y: 0, w: 10, h: 10 }, false, &m, &o).unwrap();
        assert!(v["xinerama_head"].is_null());
        let (placed, v) = drop_to(&a, Some(SCREEN), true, &r, false, &m, &o).unwrap();
        assert!(placed.x + placed.w <= 1920 && v["xinerama_head"].is_null());
    }

    #[test]
    fn effective_uses_conky_defaults_and_marks_unknown() {
        let cfg = ov::parse_config("conky.config = { gap_y = 30 }").unwrap();
        let (pl, head, unknown) = effective(&cfg, &Overrides::default());
        assert_eq!(pl, p("bottom_left", 5, 30));
        assert_eq!((head, unknown), (None, false));
        let cfg = ov::parse_config("local s=2\nconky.config = { alignment = 'none', gap_x = 10 * s, xinerama_head = 1 }").unwrap();
        let (_, head, unknown) = effective(&cfg, &Overrides::default());
        assert!(unknown);
        assert_eq!(head, Some(1));
        let own = Overrides { alignment: Some("top_right".into()), gap_x: Some(1), ..Default::default() };
        let (pl, _, unknown) = effective(&cfg, &own);
        assert_eq!((pl.alignment.as_str(), pl.gap_x, unknown), ("top_right", 1, false));
    }

    #[test]
    fn border_and_sizes() {
        let cfg = ov::parse_config("conky.config = { border_inner_margin = 10, border_width = x }").unwrap();
        assert_eq!(border_of(&cfg), 10 + 1 + 1);
        let cfg = ov::parse_config("conky.config = { minimum_width = 300, minimum_height = 40 }").unwrap();
        let m = Metrics::default();
        assert_eq!(guess_size(&cfg, &Overrides::default(), &m), (310, GUESS_SIZE.1));
        assert_eq!(size_from_card(232, 132), Some((200, 100)));
        assert_eq!(size_from_card(640, 200), None, "уменьшенная плитка");
        assert_eq!(size_from_card(30, 30), None);
    }

    #[test]
    fn gui_screens_parse() {
        let a = parse_gui_screens(r#"[{"name":"DP-1","x":0,"y":0,"width":2560,"height":1440},{"name":"x","x":2560,"y":0,"width":50,"height":50}]"#);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].rect, Rect { x: 0, y: 0, w: 2560, h: 1440 });
        assert!(a[0].primary && a[0].head.is_none());
        assert!(parse_gui_screens("nope").is_empty());
    }

    #[test]
    fn position_value_round_trips_through_set_value() {
        let mut o = Overrides { gap_x: Some(3), bg_none: Some(true), ..Default::default() };
        let v = serde_json::json!({"alignment": "tr", "gap_x": 40, "gap_y": null, "xinerama_head": 1});
        ov::set_value(&mut o, "position", &v).unwrap();
        assert_eq!(o.alignment.as_deref(), Some("top_right"));
        assert_eq!((o.gap_x, o.gap_y, o.xinerama_head, o.bg_none), (Some(40), None, Some(1), Some(true)));
        // «Вернуть»: прежние значения, null — как у автора.
        let before = position_value(&Overrides::default());
        ov::set_value(&mut o, "position", &before).unwrap();
        assert_eq!((o.alignment.clone(), o.gap_x, o.xinerama_head), (None, None, None));
        assert_eq!(o.bg_none, Some(true), "прочие поля не тронуты");
        assert!(ov::set_value(&mut o, "position", &serde_json::json!({"alignment": "sideways"})).is_err());
        assert!(ov::set_value(&mut o, "position", &serde_json::json!(5)).is_err());
    }

    #[test]
    fn unavailable_on_wayland_without_layers() {
        let c = |wayland: bool, de: &str| Ctx { home: "/h".into(), xdg_config_home: None, wayland, de: de.into() };
        assert!(unavailable_reason(&c(true, "gnome")).is_some());
        assert!(unavailable_reason(&c(true, "kde")).is_none());
        assert!(unavailable_reason(&c(false, "gnome")).is_none());
    }
}
