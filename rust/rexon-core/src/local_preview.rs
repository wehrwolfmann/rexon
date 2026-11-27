//! The tile picture of a local theme that has neither its own picture nor a
//! picture from a store entry (the "Appearance" tab).
//!
//! Everything here is deterministic (input → output): what to draw on the tile —
//! a sample from the theme's own contents by the table of kinds
//! [`PREVIEW_RULES`] or the kind's placeholder — and the ready list of drawing
//! operations ([`DrawOp`]): what to fill, with what color, which picture or part
//! of the theme's image to put where. The rendering itself belongs to the window
//! (`rexon-gui`, `local_preview_render.rs`): it executes the operations in order
//! and decides nothing.
//!
//! Parsing of the characteristic colors of a KDE scheme ([`parse_scheme_colors`])
//! is checked against a reference on real schemes —
//! `tests/local_preview_parity.rs`. Color components outside 0..=255 are dropped
//! entirely: an entry with garbage is skipped rather than drawn as a black strip.

mod metacity;
pub use metacity::{marco_failure, marco_theme_problem, MarcoProblem};
mod scheme;
pub use scheme::{
    follows_scheme_badge, is_dark, item_tone, plasma_style_tone, q_gray, scheme_look, scheme_window_sample,
    tone_bar_applies, tone_chips, tone_filter, tone_of_text, tone_passes, SchemeLook, Tone, DARK_GRAY_BELOW,
    TONE_ALL, TONE_DARK, TONE_FOLLOWS, TONE_LIGHT,
};

// ── frame size and the shared background ───────────────────────────────────

/// Width of the tile picture.
pub const PREVIEW_WIDTH: i64 = 300;
/// Height of the tile picture.
pub const PREVIEW_HEIGHT: i64 = 170;
/// How many characteristic scheme colors are taken at most.
pub const MAX_SCHEME_COLORS: usize = 6;
/// Keys of the characteristic scheme colors — searched for in all groups of the
/// file in order of appearance.
pub const SCHEME_COLOR_KEYS: &[&str] = &["BackgroundNormal", "DecorationFocus", "ForegroundNormal"];
/// The base fill of the frame.
pub const BASE_BACKGROUND: &str = "#1e2d3d";
/// Top of the shared background's gradient.
pub const GRADIENT_TOP: &str = "#263d52";
/// Bottom of the shared background's gradient.
pub const GRADIENT_BOTTOM: &str = "#16202d";

// ── parsing .colors ────────────────────────────────────────────────────────

/// Characteristic colors of a KDE scheme from the text of a `.colors` file (an
/// unreadable file — empty text → `[]`).
///
/// Line by line: without `=` — skipped; a key not from [`SCHEME_COLOR_KEYS`] —
/// skipped (exact match, the value after the first `=`); fewer than three
/// comma-separated components — skipped; a non-numeric component (surrounding
/// spaces and a sign are allowed) — skipped. Duplicates are dropped, at most
/// [`MAX_SCHEME_COLORS`] colors in order of appearance. Components outside
/// 0..=255 are dropped entirely.
pub fn parse_scheme_colors(text: &str) -> Vec<(u8, u8, u8)> {
    let mut out: Vec<(u8, u8, u8)> = Vec::new();
    for line in text.lines() {
        let s = line.trim();
        let Some((k, v)) = s.split_once('=') else {
            continue;
        };
        if !SCHEME_COLOR_KEYS.contains(&k) {
            continue;
        }
        let parts: Vec<&str> = v.split(',').collect();
        if parts.len() < 3 {
            continue;
        }
        let Some(rgb) = parse_rgb(parts[0], parts[1], parts[2]) else {
            continue;
        };
        if out.contains(&rgb) {
            continue;
        }
        out.push(rgb);
        if out.len() >= MAX_SCHEME_COLORS {
            break;
        }
    }
    out
}

/// Three color components, each in 0..=255.
fn parse_rgb(r: &str, g: &str, b: &str) -> Option<(u8, u8, u8)> {
    Some((color_byte(r)?, color_byte(g)?, color_byte(b)?))
}

/// An integer in 0..=255; surrounding spaces and a sign are allowed.
fn color_byte(s: &str) -> Option<u8> {
    let v = s.trim().parse::<i64>().ok()?;
    u8::try_from(v).ok()
}

/// The theme kind glyph from the tile label: the first word (the label holds
/// "💎 Global Theme · …"). An empty label → an empty string, no glyph is drawn.
pub fn type_emoji(author: &str) -> String {
    if author.is_empty() {
        return String::new();
    }
    let stripped = author.trim();
    match stripped.split_once(' ') {
        Some((first, _)) => first.to_string(),
        None => stripped.to_string(),
    }
}

// ── sizes and colors ──────────────────────────────────────────────────────

/// A rectangle `(x, y, width, height)`.
pub type Rect = (f64, f64, f64, f64);
/// A color with transparency `(r, g, b, a)`.
pub type Rgba = (u8, u8, u8, u8);

/// Font size of the kind glyph on the placeholder.
pub const GLYPH_POINT_SIZE: i64 = 48;
/// Color of the kind glyph on the placeholder.
pub const GLYPH_COLOR: Rgba = (255, 255, 255, 255);

// ── drawing plan ──────────────────────────────────────────────────────────

/// One drawing operation of the tile picture.
///
/// The executor performs the operations one after another and decides NOTHING:
/// neither geometry, nor colors, nor what exactly to draw.
#[derive(Debug, Clone, PartialEq)]
pub enum DrawOp {
    /// Fill a rectangle with a color.
    Fill { rect: Rect, color: Rgba },
    /// Fill a rectangle with a top-to-bottom gradient: `top` → `bottom` (colors as
    /// `#rrggbb`).
    Gradient {
        rect: Rect,
        top: &'static str,
        bottom: &'static str,
    },
    /// Text in the center of a rectangle with a font of size `point_size`.
    Text {
        rect: Rect,
        text: String,
        point_size: i64,
        color: Rgba,
    },
    /// A picture from a file (PNG, JPEG, SVG) in a rectangle, keeping the aspect
    /// ratio: entirely in the center or, with `cover`, filling the whole rectangle
    /// with the excess cropped.
    Image { rect: Rect, path: String, cover: bool },
    /// A picture from a file at its own size, repeated over the whole rectangle from
    /// its top left corner (the excess is cropped). That is how an xfwm4 theme frame
    /// is assembled: its sides are narrow tile images.
    Tiled { rect: Rect, path: String },
    /// A frame of nine parts of an SVG image (`<prefix>-topleft`, `…-top`, …,
    /// `…-center`; an empty `prefix` — just `topleft`, …): corners at their own
    /// size, sides and the middle stretched. Plasma styles and Aurorae themes store
    /// their frames this way. The first of `prefixes` whose middle is in the file is
    /// taken; if there is none, the frame is not drawn.
    SvgFrame { rect: Rect, path: String, prefixes: Vec<String> },
    /// One part of an SVG image by name, stretched over a rectangle.
    SvgElement { rect: Rect, path: String, element: String },
    /// Ready RGBA8888 pixels (not premultiplied by alpha) with the top left corner
    /// at `(x, y)`.
    Pixels { x: f64, y: f64, width: u32, height: u32, rgba: Vec<u8> },
    /// Clipping: everything up to the paired [`DrawOp::Unclip`] is drawn only inside
    /// a rectangle with rounded corners (radii: top left, top right, bottom left,
    /// bottom right) and inside the already opened clips.
    Clip { rect: Rect, radii: [f64; 4] },
    /// End of the last opened clip.
    Unclip,
    /// A segment of thickness `width` between the centers of two points.
    Line { from: (f64, f64), to: (f64, f64), width: f64, color: Rgba },
    /// An arc of the ellipse inscribed into a rectangle: from angle `start` for
    /// `extent` degrees clockwise, zero — up. With `filled` — a filled sector,
    /// otherwise a one-point line.
    Arc { rect: Rect, start: f64, extent: f64, filled: bool, color: Rgba },
    /// Fill a rectangle with a gradient through several colors in equal shares.
    Shade { rect: Rect, direction: ShadeDirection, colors: Vec<Rgba> },
    /// A picture from a file stretched over a rectangle without keeping the aspect
    /// ratio or, with `tile`, repeated at its own size; `alpha` — opacity (0–1).
    /// With `colorize` the picture is recolored: its brightness goes from black
    /// through the named color to white.
    Picture { rect: Rect, path: String, tile: bool, alpha: f64, colorize: Option<Rgba> },
    /// A line of text in the center of a rectangle with a font `pixel_size` screen
    /// pixels high; a line wider than the rectangle is drawn with a smaller font —
    /// so that it fits by width.
    Label { rect: Rect, text: String, pixel_size: i64, bold: bool, color: Rgba },
}

/// Where the [`DrawOp::Shade`] gradient goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadeDirection {
    /// Left to right.
    Horizontal,
    /// Top to bottom.
    Vertical,
    /// From the top left corner to the bottom right.
    Diagonal,
}

// ── the single chain of a local theme's tile picture ───────────────────────
//
// The tile picture is obtained by one chain for ANY theme kind and any section
// ("Active", "Downloaded", "System", "Favorites", the sections by kind):
//   1. the theme's own picture — a file in its directory or one named by its
//      metadata (`installed_scan::find_preview`);
//   2. a picture from a store entry — put next to the theme at installation,
//      found through the KNewStuff registry or the store cache
//      (`installed_scan::resolve_preview`);
//   3. a sample drawn from the theme's own contents — the [`PREVIEW_RULES`]
//      table;
//   4. the kind's placeholder: the kind glyph on the shared background, without
//      a name (it stands under the picture).
// Steps 1–2 are given by the disk scan in the `preview_local` field; steps 3–4
// by [`card_picture`]. A new kind is connected with one line of
// [`PREVIEW_RULES`]: neither the window nor the executor knows about theme kinds.

/// A mark of the kind of drawn pictures: it is part of the cache key, so that
/// frames drawn by previous rules are not served instead of the current ones.
pub const PICTURE_STYLE: &str = "v6";

/// Where the tile picture came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PreviewSource {
    /// The theme's own picture.
    Own,
    /// A picture from a store entry.
    Store,
    /// A sample drawn from the theme's contents.
    Sample,
    /// The kind's placeholder.
    Placeholder,
}

impl PreviewSource {
    /// A short word for the log and the test stand.
    pub fn label(self) -> &'static str {
        match self {
            PreviewSource::Own => "своя",
            PreviewSource::Store => "магазин",
            PreviewSource::Sample => "образец",
            PreviewSource::Placeholder => "заглушка",
        }
    }
}

/// Whose picture lies in the scan's `preview_local`: the theme's own (a file
/// inside the theme directory) or from a store entry (put at installation, the
/// store cache).
pub fn file_source(preview_path: &str, install_path: &str) -> PreviewSource {
    let inside = !install_path.is_empty()
        && preview_path.starts_with(&format!("{}/", install_path.trim_end_matches('/')));
    let ours = preview_path.ends_with(crate::installed_scan::REXON_PREVIEW_NAME);
    if inside && !ours {
        PreviewSource::Own
    } else {
        PreviewSource::Store
    }
}

/// What is known about the theme when its sample is drawn.
#[derive(Debug, Clone, Copy)]
pub struct SampleInput<'a> {
    /// The apply kind (`local:<kind>:<name>`).
    pub kind: &'a str,
    /// The theme directory; for a color scheme — its file (with or without the
    /// extension).
    pub install_path: &'a str,
    /// Icon theme directories in which to look for a package icon by name
    /// ([`icon_theme_dirs`]).
    pub icon_themes: &'a [String],
}

/// A row of the sample table: the theme kind and what its sample is drawn with.
pub struct PreviewRule {
    /// The apply kind.
    pub kind: &'static str,
    /// What is drawn — for the log and the test stand.
    pub what: &'static str,
    /// The sample's operations on top of the shared background; `None` — there is
    /// nothing to take from the theme.
    pub sample: fn(&SampleInput) -> Option<Vec<DrawOp>>,
}

const fn rule(
    kind: &'static str,
    what: &'static str,
    sample: fn(&SampleInput) -> Option<Vec<DrawOp>>,
) -> PreviewRule {
    PreviewRule { kind, what, sample }
}

/// Samples by theme kind. A kind that is not here gets steps 1, 2 and 4.
pub const PREVIEW_RULES: &[PreviewRule] = &[
    rule("colors", "окно цветами схемы", scheme::colors_sample),
    rule("cursor", "указатели темы", cursor_sample),
    rule("icons", "значки темы", icons_sample),
    rule("plasma", "окно и панель из рисунков темы", plasma_sample),
    rule("aurorae", "рамка окна из рисунков темы", aurorae_sample),
    rule("kvantum", "окно в цветах темы", kvantum_sample),
    rule("gtk", "окно в цветах темы", gtk_sample),
    rule("plymouth", "кадр заставки", plymouth_sample),
    rule("sddm", "фон экрана входа", sddm_sample),
    rule("xfwm4", "рамка окна из рисунков темы", xfwm4_sample),
    rule("marco", "рамка окна по командам рисования темы", metacity::marco_sample),
    rule("openbox", "рамки окон по цветам темы", openbox_sample),
    rule("gnomeshell", "панель и меню в цветах темы", gnome_shell_sample),
    rule("cinnamontheme", "панель и меню в цветах темы", cinnamon_sample),
    rule("tabbox", "значок пакета", package_icon_sample),
    rule("kwinscripts", "значок пакета", package_icon_sample),
    rule("kwineffects", "значок пакета", package_icon_sample),
    rule("gnomeext", "значок пакета", package_icon_sample),
    rule("cinnamonapplets", "значок пакета", package_icon_sample),
    rule("cinnamondesklets", "значок пакета", package_icon_sample),
    rule("cinnamonext", "значок пакета", package_icon_sample),
    rule("nemoactions", "значок действия", package_icon_sample),
];

/// A ready tile picture: the source and the drawing operations.
#[derive(Debug, Clone, PartialEq)]
pub struct CardPicture {
    /// [`PreviewSource::Sample`] or [`PreviewSource::Placeholder`].
    pub source: PreviewSource,
    /// What is drawn (empty for a placeholder) — for the log and the test stand.
    pub what: &'static str,
    /// Frame width.
    pub width: i64,
    /// Frame height.
    pub height: i64,
    /// The base fill.
    pub base: &'static str,
    /// Operations in execution order.
    pub ops: Vec<DrawOp>,
}

/// The tile picture of a theme without its own picture and without a store
/// entry: a sample from its contents, and if there is nothing to take — the
/// kind's placeholder with glyph `glyph`.
pub fn card_picture(input: &SampleInput, glyph: &str) -> CardPicture {
    let mut ops = vec![backdrop()];
    // The theme's own thumbnails (the window frame and GTK widgets) are more
    // honest than a drawn sample — but at their own size, fitted in, not
    // stretched over the tile.
    let sampled = thumbnails_sample(input).map(|sample| ("миниатюры темы", sample)).or_else(|| {
        PREVIEW_RULES
            .iter()
            .find(|r| r.kind == input.kind)
            .and_then(|r| (r.sample)(input).map(|sample| (r.what, sample)))
    });
    let (source, what) = match sampled {
        Some((what, sample)) => {
            ops.extend(sample);
            (PreviewSource::Sample, what)
        }
        None => {
            if !glyph.is_empty() {
                ops.push(DrawOp::Text {
                    rect: (0.0, 0.0, PREVIEW_WIDTH as f64, PREVIEW_HEIGHT as f64),
                    text: glyph.to_string(),
                    point_size: GLYPH_POINT_SIZE,
                    color: GLYPH_COLOR,
                });
            }
            (PreviewSource::Placeholder, "")
        }
    };
    CardPicture {
        source,
        what,
        width: PREVIEW_WIDTH,
        height: PREVIEW_HEIGHT,
        base: BASE_BACKGROUND,
        ops,
    }
}

// ── small theme thumbnails ────────────────────────────────────────────────
//
// GTK and frame themes put tiny `thumbnail.png` files into subdirectories:
// B00merang has 120×35 (a button and checkboxes) and 100×32 (a window title
// bar), Dracula 133×32. Stretched "over the whole tile with cropping" they
// turned into a huge blurry "tton" and a check mark. Such thumbnails are no
// good as the whole tile picture: a frame is assembled from them — the window
// title bar above the widgets, each in its own proportions and no larger than
// twice its size, on the shared background.

/// A thumbnail smaller than this in width or height is "small": it cannot cover
/// the whole tile (the tile is [`PREVIEW_WIDTH`]×[`PREVIEW_HEIGHT`]).
pub const SMALL_THUMB_WIDTH: f64 = 200.0;
/// See [`SMALL_THUMB_WIDTH`].
pub const SMALL_THUMB_HEIGHT: f64 = 100.0;
/// By how many times at most a small thumbnail is enlarged: beyond that it blurs
/// into mush.
pub const SMALL_THUMB_MAX_SCALE: f64 = 2.0;

/// Window frame thumbnails in the theme's subdirectories — in order of preference.
const FRAME_THUMBS: &[&str] = &["metacity-1/thumbnail.png", "xfwm4/thumbnail.png"];
/// Widget thumbnails (buttons, checkboxes, a panel) — in order of preference.
const WIDGET_THUMBS: &[&str] = &[
    "gtk-3.0/thumbnail.png",
    "gtk-3.20/thumbnail.png",
    "gtk-4.0/thumbnail.png",
    "gtk-2.0/thumbnail.png",
    "cinnamon/thumbnail.png",
    "gnome-shell/thumbnail.png",
];
/// Theme kinds whose tile is assembled from subdirectory thumbnails.
const THUMB_KINDS: &[&str] = &["gtk", "marco", "xfwm4", "cinnamontheme", "gnomeshell"];

/// Whether a theme subdirectory's thumbnail (`…/<subdir>/thumbnail.png`) is small.
pub fn is_small_thumbnail(path: &str) -> bool {
    path.ends_with("/thumbnail.png")
        && image_size(std::path::Path::new(path))
            .is_some_and(|(w, h)| w < SMALL_THUMB_WIDTH || h < SMALL_THUMB_HEIGHT)
}

/// Remove small thumbnails from local tiles: for such a tile the window will
/// assemble a frame from the thumbnails ([`card_picture`]) rather than stretch a
/// single one over the whole size.
pub fn drop_small_thumbnails(items: &mut [crate::models::ThemeItem]) {
    for it in items.iter_mut() {
        // A local tile — by the `local:…` id: the scan does not set the kind
        // (`local_kind`) for all, the window adds it later.
        if it.id.starts_with("local:") && is_small_thumbnail(&it.preview_local) {
            it.preview_local.clear();
        }
    }
}

/// A frame from the theme's thumbnails: the window frame on top, widgets below
/// it. `None` — there are no thumbnails or the theme kind is not the right one.
fn thumbnails_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    if !THUMB_KINDS.contains(&input.kind) {
        return None;
    }
    let dir = std::path::Path::new(input.install_path);
    let first = |list: &[&str]| {
        list.iter().map(|rel| dir.join(rel)).find_map(|p| {
            let size = image_size(&p)?;
            Some((p.to_string_lossy().into_owned(), size))
        })
    };
    let pictures: Vec<(String, (f64, f64))> = [first(FRAME_THUMBS), first(WIDGET_THUMBS)].into_iter().flatten().collect();
    if pictures.is_empty() {
        return None;
    }
    const MARGIN: f64 = 20.0;
    const GAP: f64 = 12.0;
    let room_w = PREVIEW_WIDTH as f64 - 2.0 * MARGIN;
    let room_h = (PREVIEW_HEIGHT as f64 - 2.0 * MARGIN - GAP * (pictures.len() as f64 - 1.0)) / pictures.len() as f64;
    let sized: Vec<(String, f64, f64)> = pictures
        .into_iter()
        .map(|(path, (w, h))| {
            let k = SMALL_THUMB_MAX_SCALE.min(room_w / w).min(room_h / h);
            (path, w * k, h * k)
        })
        .collect();
    let total: f64 = sized.iter().map(|(_, _, h)| h).sum::<f64>() + GAP * (sized.len() as f64 - 1.0);
    let mut y = (PREVIEW_HEIGHT as f64 - total) / 2.0;
    let mut ops = Vec::new();
    for (path, w, h) in sized {
        let x = (PREVIEW_WIDTH as f64 - w) / 2.0;
        ops.push(DrawOp::Image { rect: (x.round(), y.round(), w.round(), h.round()), path, cover: false });
        y += h + GAP;
    }
    Some(ops)
}

/// The shared background of drawn pictures.
fn backdrop() -> DrawOp {
    backdrop_sized(PREVIEW_WIDTH as f64, PREVIEW_HEIGHT as f64)
}

fn backdrop_sized(w: f64, h: f64) -> DrawOp {
    DrawOp::Gradient { rect: (0.0, 0.0, w, h), top: GRADIENT_TOP, bottom: GRADIENT_BOTTOM }
}

/// A tile picture from a ready snapshot (a KDE window frame is drawn by its own
/// module — [`crate::kdecoration::take_shot`], a Qt style and a GTK theme by the
/// helper [`crate::widget_shot::take_shot`]): the snapshot with transparency on
/// top of the shared background, like the other samples. The frame is the size
/// of the snapshot, but no smaller than the tile: a large snapshot (widgets are
/// captured for the details picture) does not lose anything by being shrunk to
/// the tile and is not stretched in the details.
pub fn shot_picture(path: &str, what: &'static str) -> CardPicture {
    let (w, h) = image_size(std::path::Path::new(path))
        .filter(|(w, h)| *w >= PREVIEW_WIDTH as f64 && *h >= PREVIEW_HEIGHT as f64)
        .unwrap_or((PREVIEW_WIDTH as f64, PREVIEW_HEIGHT as f64));
    CardPicture {
        source: PreviewSource::Sample,
        what,
        width: w.round() as i64,
        height: h.round() as i64,
        base: BASE_BACKGROUND,
        ops: vec![backdrop_sized(w, h), DrawOp::Image { rect: (0.0, 0.0, w, h), path: path.to_string(), cover: false }],
    }
}

// ── reading theme files ───────────────────────────────────────────────────

/// Text of a theme file, no more than 4 MB (themes also carry huge stylesheets).
fn read_text(path: &std::path::Path) -> Option<String> {
    const LIMIT: u64 = 4 << 20;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > LIMIT {
        return None;
    }
    std::fs::read(path).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Value of `key` in group `[group]` of ini-like text (the first one found).
fn ini_value<'t>(text: &'t str, group: &str, key: &str) -> Option<&'t str> {
    let mut inside = false;
    for line in text.lines() {
        let s = line.trim();
        if let Some(name) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            inside = name == group;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((k, v)) = s.split_once('=') {
            if k.trim() == key {
                return Some(v.trim());
            }
        }
    }
    None
}

/// The `<stem>.svgz` or `<stem>.svg` file in the theme directory.
fn svg_file(dir: &std::path::Path, stem: &str) -> Option<String> {
    ["svgz", "svg"]
        .iter()
        .map(|ext| dir.join(format!("{stem}.{ext}")))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

// ── colors ───────────────────────────────────────────────────────────────

type Rgb = (u8, u8, u8);

/// A color of the form `r,g,b` (KDE color files).
fn kde_color(value: &str) -> Option<Rgb> {
    let mut parts = value.split(',');
    parse_rgb(parts.next()?, parts.next()?, parts.next()?)
}

/// A stylesheet color: `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(…)`, `rgba(…)`,
/// `white`, `black`.
fn css_color(value: &str) -> Option<Rgb> {
    let v = value.trim();
    if let Some(hex) = v.strip_prefix('#') {
        let hex: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        let byte = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
        return match hex.len() {
            3 | 4 => {
                let d = |at: usize| u8::from_str_radix(hex.get(at..at + 1)?, 16).ok().map(|n| n * 17);
                Some((d(0)?, d(1)?, d(2)?))
            }
            6 | 8 => Some((byte(0)?, byte(2)?, byte(4)?)),
            _ => None,
        };
    }
    let low = v.to_ascii_lowercase();
    if low.starts_with("rgb") {
        let inner = low.split_once('(')?.1.split(')').next()?;
        let mut nums = inner.split(',').map(|n| n.trim().parse::<f64>().ok());
        let mut next = || nums.next().flatten().map(|n| n.clamp(0.0, 255.0).round() as u8);
        return Some((next()?, next()?, next()?));
    }
    match low.as_str() {
        "white" => Some((255, 255, 255)),
        "black" => Some((0, 0, 0)),
        _ => None,
    }
}

/// A mix of two colors: `t` — the share of the second.
fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let ch = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    (ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2))
}

/// Window colors from which a sample of an application theme is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Window background.
    pub window: (u8, u8, u8),
    /// Background of a list view.
    pub view: (u8, u8, u8),
    /// Text.
    pub text: (u8, u8, u8),
    /// Selection.
    pub selection: (u8, u8, u8),
    /// Text on the selection.
    pub selection_text: (u8, u8, u8),
    /// Button.
    pub button: (u8, u8, u8),
    /// Window title bar.
    pub titlebar: (u8, u8, u8),
}

/// A sample window in the theme's colors: a title bar, a list with a selected
/// row, lines of text, a button.
pub fn window_sample(p: &Palette) -> Vec<DrawOp> {
    let fill = |x: f64, y: f64, w: f64, h: f64, c: Rgb, a: u8| DrawOp::Fill {
        rect: (x, y, w, h),
        color: (c.0, c.1, c.2, a),
    };
    let (x, y, w, h) = (36.0, 16.0, 228.0, 138.0);
    let edge = mix(p.window, p.text, 0.35);
    let title_text = if contrast(p.titlebar, p.text) { p.text } else { p.selection_text };
    vec![
        // The window's shadow and border.
        fill(x + 3.0, y + 4.0, w, h, (0, 0, 0), 70),
        fill(x - 1.0, y - 1.0, w + 2.0, h + 2.0, edge, 255),
        fill(x, y, w, h, p.window, 255),
        // A title bar with three buttons.
        fill(x, y, w, 22.0, p.titlebar, 255),
        fill(x + 10.0, y + 8.0, 70.0, 6.0, title_text, 220),
        fill(x + w - 18.0, y + 7.0, 8.0, 8.0, title_text, 220),
        fill(x + w - 32.0, y + 7.0, 8.0, 8.0, title_text, 150),
        fill(x + w - 46.0, y + 7.0, 8.0, 8.0, title_text, 150),
        // A list with a selected row.
        fill(x + 10.0, y + 32.0, 104.0, 96.0, edge, 255),
        fill(x + 11.0, y + 33.0, 102.0, 94.0, p.view, 255),
        fill(x + 18.0, y + 42.0, 70.0, 6.0, p.text, 220),
        fill(x + 11.0, y + 54.0, 102.0, 18.0, p.selection, 255),
        fill(x + 18.0, y + 60.0, 60.0, 6.0, p.selection_text, 235),
        fill(x + 18.0, y + 80.0, 78.0, 6.0, p.text, 220),
        fill(x + 18.0, y + 96.0, 52.0, 6.0, p.text, 220),
        // Lines of text on the right.
        fill(x + 126.0, y + 36.0, 88.0, 6.0, p.text, 220),
        fill(x + 126.0, y + 50.0, 64.0, 6.0, p.text, 150),
        fill(x + 126.0, y + 64.0, 76.0, 6.0, p.text, 150),
        // A progress bar in the selection color.
        fill(x + 126.0, y + 82.0, 88.0, 6.0, edge, 255),
        fill(x + 126.0, y + 82.0, 52.0, 6.0, p.selection, 255),
        // A button.
        fill(x + 148.0, y + 104.0, 66.0, 24.0, edge, 255),
        fill(x + 149.0, y + 105.0, 64.0, 22.0, p.button, 255),
        fill(x + 164.0, y + 113.0, 34.0, 6.0, p.text, 220),
    ]
}

/// Whether two colors differ enough to write one over the other.
fn contrast(a: Rgb, b: Rgb) -> bool {
    let luma = |c: Rgb| 0.299 * c.0 as f64 + 0.587 * c.1 as f64 + 0.114 * c.2 as f64;
    (luma(a) - luma(b)).abs() > 80.0
}

/// Window colors from a KDE color file (a `.colors` scheme, the `colors` file of
/// a Plasma style). `None` — the file has neither a window background nor window
/// text.
pub fn kde_palette(text: &str) -> Option<Palette> {
    let get = |group: &str, key: &str| ini_value(text, group, key).and_then(kde_color);
    let window = get("Colors:Window", "BackgroundNormal")?;
    let text_color = get("Colors:Window", "ForegroundNormal")?;
    let selection = get("Colors:Selection", "BackgroundNormal").unwrap_or(text_color);
    Some(Palette {
        window,
        view: get("Colors:View", "BackgroundNormal").unwrap_or(window),
        text: text_color,
        selection,
        selection_text: get("Colors:Selection", "ForegroundNormal").unwrap_or(window),
        button: get("Colors:Button", "BackgroundNormal").unwrap_or(window),
        titlebar: get("WM", "activeBackground")
            .or_else(|| get("Colors:Header", "BackgroundNormal"))
            .unwrap_or_else(|| mix(window, text_color, 0.15)),
    })
}

/// Window colors from the settings of a Kvantum theme (the `GeneralColors` group).
pub fn kvantum_palette(text: &str) -> Option<Palette> {
    let get = |key: &str| ini_value(text, "GeneralColors", key).and_then(css_color);
    let window = get("window.color")?;
    let text_color = get("window.text.color").or_else(|| get("text.color"))?;
    Some(Palette {
        window,
        view: get("base.color").unwrap_or(window),
        text: text_color,
        selection: get("highlight.color").unwrap_or(text_color),
        selection_text: get("highlight.text.color").unwrap_or(window),
        button: get("button.color").unwrap_or(window),
        titlebar: mix(window, text_color, 0.15),
    })
}

/// Named colors of a GTK stylesheet: `@define-color name value;`. A reference
/// value (`@other`) and a function value (`shade(@other, 0.9)`) are expanded to
/// the first color named in them.
fn gtk_defined_colors(css: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for chunk in css.split("@define-color").skip(1) {
        let Some(decl) = chunk.split(';').next() else { continue };
        let decl = decl.trim();
        let Some((name, value)) = decl.split_once(char::is_whitespace) else { continue };
        out.insert(name.trim().to_string(), value.trim().to_string());
    }
    out
}

fn gtk_resolve(colors: &std::collections::HashMap<String, String>, value: &str, depth: u8) -> Option<Rgb> {
    if let Some(rgb) = css_color(value) {
        return Some(rgb);
    }
    if depth == 0 {
        return None;
    }
    // A reference or a function over a reference: we take the first named color.
    let at = value.find(['@', '#'])?;
    let rest = &value[at..];
    if rest.starts_with('#') {
        return css_color(rest);
    }
    let name: String = rest[1..]
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
        .collect();
    gtk_resolve(colors, colors.get(&name)?, depth - 1)
}

/// Window colors from the named colors of a GTK stylesheet. The names are those
/// accepted in GTK 3 and 4 themes; some themes add a suffix to the name
/// (`_breeze`).
pub fn gtk_css_palette(css: &str) -> Option<Palette> {
    let colors = gtk_defined_colors(css);
    let pick = |names: &[&str]| -> Option<Rgb> {
        names.iter().find_map(|want| {
            let exact = colors.get(*want);
            let suffixed = || {
                let prefix = format!("{want}_");
                let mut keys: Vec<&String> = colors.keys().filter(|k| k.starts_with(&prefix)).collect();
                keys.sort();
                keys.first().and_then(|k| colors.get(*k))
            };
            exact.or_else(suffixed).and_then(|v| gtk_resolve(&colors, v, 4))
        })
    };
    let window = pick(&["theme_bg_color", "bg_color", "window_bg_color"])?;
    let text = pick(&["theme_fg_color", "fg_color", "window_fg_color", "theme_text_color", "text_color"])?;
    Some(Palette {
        window,
        view: pick(&["theme_base_color", "base_color", "view_bg_color"]).unwrap_or(window),
        text,
        selection: pick(&["theme_selected_bg_color", "selected_bg_color", "accent_bg_color", "accent_color"])
            .unwrap_or(text),
        selection_text: pick(&["theme_selected_fg_color", "selected_fg_color", "accent_fg_color"])
            .unwrap_or(window),
        button: pick(&["button_bg_color", "theme_button_background_normal"]).unwrap_or(window),
        titlebar: pick(&["wm_bg", "headerbar_bg_color", "theme_titlebar_background", "titlebar_bg_color"])
            .unwrap_or_else(|| mix(window, text, 0.15)),
    })
}

/// Window colors from the settings of a GTK 2 theme: the `gtk-color-scheme` line
/// with `name:color` pairs.
pub fn gtk2_palette(gtkrc: &str) -> Option<Palette> {
    let get = |key: &str| -> Option<Rgb> {
        let marker = format!("{key}:");
        gtkrc.match_indices(&marker).find_map(|(at, _)| {
            // The whole name, not the tail of another one (`bg_color` in
            // `selected_bg_color`). The pairs in the line are separated by a line
            // break written out as letters (`\n`) — its `n` is not considered part of
            // the name.
            let head = &gtkrc[..at];
            let inside_name = head.chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_');
            if inside_name && !head.ends_with("\\n") {
                return None;
            }
            css_color(gtkrc[at + marker.len()..].trim_start())
        })
    };
    let window = get("bg_color")?;
    let text = get("fg_color").or_else(|| get("text_color"))?;
    Some(Palette {
        window,
        view: get("base_color").unwrap_or(window),
        text,
        selection: get("selected_bg_color").unwrap_or(text),
        selection_text: get("selected_fg_color").unwrap_or(window),
        button: window,
        titlebar: mix(window, text, 0.15),
    })
}

// ── samples by kind ──────────────────────────────────────────────────────

/// A Kvantum theme: a window in the colors of its settings (`<name>.kvconfig`).
fn kvantum_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    let dir = std::path::Path::new(input.install_path);
    let named = dir.file_name().map(|n| dir.join(format!("{}.kvconfig", n.to_string_lossy())));
    let config = named.filter(|p| p.is_file()).or_else(|| {
        let mut found: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "kvconfig"))
            .collect();
        found.sort();
        found.into_iter().next()
    })?;
    Some(window_sample(&kvantum_palette(&read_text(&config)?)?))
}

/// A GTK stylesheet together with those it imports (`@import`) — the theme's
/// named colors often lie in a neighboring file.
fn gtk_css_text(dir: &std::path::Path) -> Option<String> {
    let main = read_text(&dir.join("gtk.css"))?;
    let mut text = String::new();
    for chunk in main.split("@import").skip(1).take(8) {
        let Some(decl) = chunk.split(';').next() else { continue };
        let name = decl
            .trim()
            .trim_start_matches("url(")
            .trim_end_matches(')')
            .trim_matches(['"', '\'', ' ']);
        if name.contains("://") || name.starts_with('/') || name.contains("..") {
            continue;
        }
        if let Some(part) = read_text(&dir.join(name)) {
            text.push_str(&part);
            text.push('\n');
        }
    }
    // The main file's own definitions come last: they override the imported ones.
    text.push_str(&main);
    Some(text)
}

/// A GTK theme: a window in its colors — from the GTK 3/4 stylesheet, and if the
/// colors are not named there (the theme is compiled into one binary file) —
/// from the GTK 2 settings.
fn gtk_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    let dir = std::path::Path::new(input.install_path);
    let from_css = ["gtk-3.0", "gtk-4.0", "gtk-3.20"]
        .iter()
        .find_map(|sub| gtk_css_text(&dir.join(sub)).and_then(|css| gtk_css_palette(&css)));
    let palette = from_css.or_else(|| gtk2_palette(&read_text(&dir.join("gtk-2.0/gtkrc"))?))?;
    Some(window_sample(&palette))
}

/// A Plasma style: a window and a panel from its own images; for a style made of
/// colors only — a window in those colors.
fn plasma_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    let dir = std::path::Path::new(input.install_path);
    let frame = |path: String, prefixes: &[&str], rect: Rect| DrawOp::SvgFrame {
        rect,
        path,
        prefixes: prefixes.iter().map(|p| p.to_string()).collect(),
    };
    let dialog = svg_file(dir, "dialogs/background").or_else(|| svg_file(dir, "widgets/background"));
    let panel = svg_file(dir, "widgets/panel-background");
    if dialog.is_none() && panel.is_none() {
        let palette = kde_palette(&read_text(&dir.join("colors"))?)?;
        return Some(window_sample(&palette));
    }
    let mut ops = Vec::new();
    if let Some(path) = dialog {
        ops.push(frame(path, &[""], (38.0, 10.0, 224.0, 112.0)));
        // The style's text color is known only from its color file; without it —
        // a neutral gray, distinguishable on both light and dark.
        let ink = read_text(&dir.join("colors"))
            .and_then(|t| ini_value(&t, "Colors:Window", "ForegroundNormal").and_then(kde_color))
            .unwrap_or((128, 128, 128));
        for (x, y, w) in [(60.0, 34.0, 120.0), (60.0, 48.0, 86.0), (60.0, 62.0, 104.0)] {
            ops.push(DrawOp::Fill { rect: (x, y, w, 6.0), color: (ink.0, ink.1, ink.2, 200) });
        }
        if let Some(path) = svg_file(dir, "widgets/lineedit") {
            ops.push(frame(path, &["base"], (58.0, 82.0, 96.0, 26.0)));
        }
        if let Some(path) = svg_file(dir, "widgets/button") {
            ops.push(frame(path, &["normal"], (164.0, 82.0, 78.0, 26.0)));
        }
    }
    if let Some(path) = panel {
        // A panel at the bottom edge of the screen — its own frame (`south`);
        // some styles have one for all edges.
        ops.push(frame(path, &["south", ""], (8.0, 128.0, 284.0, 36.0)));
    }
    Some(ops)
}

/// An Aurorae window frame theme: a frame from its image and title bar buttons in
/// the places named by its settings (`<name>rc`).
fn aurorae_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    let dir = std::path::Path::new(input.install_path);
    let decoration = svg_file(dir, "decoration")?;
    let rc = dir
        .file_name()
        .and_then(|n| read_text(&dir.join(format!("{}rc", n.to_string_lossy()))))
        .unwrap_or_default();
    let num = |group: &str, key: &str, default: f64| {
        ini_value(&rc, group, key).and_then(|v| v.parse::<f64>().ok()).unwrap_or(default)
    };
    let (x, y, w, h) = (30.0, 20.0, 240.0, 130.0);
    let mut ops = vec![DrawOp::SvgFrame { rect: (x, y, w, h), path: decoration, prefixes: vec!["decoration".to_string()] }];
    let bw = num("Layout", "ButtonWidth", 22.0).clamp(8.0, 30.0);
    let bh = num("Layout", "ButtonHeight", 22.0).clamp(8.0, 30.0);
    let gap = num("Layout", "ButtonSpacing", 4.0).clamp(0.0, 12.0);
    let top = y + num("Layout", "PaddingTop", 0.0).clamp(0.0, 40.0) + num("Layout", "TitleEdgeTop", 4.0).clamp(0.0, 20.0);
    let mut right = x + w - num("Layout", "PaddingRight", 0.0).clamp(0.0, 40.0) - num("Layout", "TitleEdgeRight", 4.0).clamp(0.0, 20.0);
    for name in ["close", "maximize", "minimize"] {
        let Some(path) = svg_file(dir, name) else { continue };
        right -= bw;
        ops.push(DrawOp::SvgElement { rect: (right, top, bw, bh), path, element: "active-center".to_string() });
        right -= gap;
    }
    Some(ops)
}

/// The number from a frame name (`animation-0012` → 12) and the name without the
/// number.
fn frame_number(stem: &str) -> Option<(&str, u32)> {
    let digits = stem.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits == stem.len() {
        return None;
    }
    let (series, number) = stem.split_at(stem.len() - digits);
    Some((series.trim_end_matches(['-', '_']), number.parse().ok()?))
}

/// A Plymouth boot splash: a background in its color and the main image — the
/// emblem (`watermark`, `logo`), and if there is none — a frame of the longest
/// animation.
fn plymouth_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    let dir = std::path::Path::new(input.install_path);
    let conf = dir
        .file_name()
        .and_then(|n| read_text(&dir.join(format!("{}.plymouth", n.to_string_lossy()))))
        .unwrap_or_default();
    let module = ini_value(&conf, "Plymouth Theme", "ModuleName").unwrap_or("");
    let images = ini_value(&conf, module, "ImageDir")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| dir.to_path_buf());
    let mut names: Vec<String> = std::fs::read_dir(&images)
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_ascii_lowercase().ends_with(".png"))
        .collect();
    names.sort();
    let stem = |n: &str| n[..n.len() - 4].to_string();
    let named = |want: &[&str]| {
        want.iter().find_map(|w| names.iter().find(|n| stem(n).eq_ignore_ascii_case(w)).cloned())
    };
    // The longest numbered series; for an "animation" we take the last frame
    // (the image is fully assembled on it), for the others — the middle one.
    let mut series: std::collections::BTreeMap<String, Vec<(u32, String)>> = Default::default();
    for n in &names {
        if let Some((name, number)) = frame_number(&stem(n)) {
            series.entry(name.to_string()).or_default().push((number, n.clone()));
        }
    }
    let frame = series.iter().max_by_key(|(_, frames)| frames.len()).map(|(name, frames)| {
        let mut frames = frames.clone();
        frames.sort();
        let at = if name.contains("animation") { frames.len() - 1 } else { frames.len() / 2 };
        frames[at].1.clone()
    });
    let background = named(&["background", "bg", "wallpaper"]);
    let main = named(&["watermark", "logo", "bgrt-fallback"]).or(frame).or_else(|| {
        names.iter().find(|n| Some(*n) != background.as_ref()).cloned()
    });
    if main.is_none() && background.is_none() {
        return None;
    }
    let hex = |key: &str| {
        ini_value(&conf, module, key)
            .and_then(|v| css_color(&format!("#{}", v.trim_start_matches("0x"))))
            .unwrap_or((0, 0, 0))
    };
    let (start, end) = (hex("BackgroundStartColor"), hex("BackgroundEndColor"));
    let full = (0.0, 0.0, PREVIEW_WIDTH as f64, PREVIEW_HEIGHT as f64);
    let half = PREVIEW_HEIGHT as f64 / 2.0;
    let path = |n: String| images.join(n).to_string_lossy().into_owned();
    let mut ops = vec![
        DrawOp::Fill { rect: (0.0, 0.0, full.2, half), color: (start.0, start.1, start.2, 255) },
        DrawOp::Fill { rect: (0.0, half, full.2, half), color: (end.0, end.1, end.2, 255) },
    ];
    if let Some(n) = background {
        ops.push(DrawOp::Image { rect: full, path: path(n), cover: true });
    }
    if let Some(n) = main {
        ops.push(DrawOp::Image { rect: (75.0, 25.0, 150.0, 120.0), path: path(n), cover: false });
    }
    Some(ops)
}

/// An SDDM login screen theme without a snapshot: its background picture
/// (`theme.conf`).
fn sddm_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    let dir = std::path::Path::new(input.install_path);
    let conf = read_text(&dir.join("theme.conf"))?;
    let name = ini_value(&conf, "General", "background").or_else(|| ini_value(&conf, "General", "Background"))?;
    let name = name.trim_matches(['"', '\'']);
    let path = if name.starts_with('/') { std::path::PathBuf::from(name) } else { dir.join(name) };
    let low = name.to_ascii_lowercase();
    if !path.is_file() || ![".png", ".jpg", ".jpeg", ".webp", ".svg"].iter().any(|e| low.ends_with(e)) {
        return None;
    }
    Some(vec![DrawOp::Image {
        rect: (0.0, 0.0, PREVIEW_WIDTH as f64, PREVIEW_HEIGHT as f64),
        path: path.to_string_lossy().into_owned(),
        cover: true,
    }])
}

// ── xfwm4 window frame ────────────────────────────────────────────────────
//
// An XFCE frame theme is a directory of images of the frame parts (described in
// the "Xfwm4 theme how-to" in the XFCE documentation): corners `top-left`,
// `top-right`, `bottom-left`, `bottom-right`, sides `left`, `right`, `bottom`,
// five title bar parts `title-1` … `title-5` and buttons `close`, `maximize`,
// `hide`, `menu`, `stick`, `shade`; each part has states `-active`,
// `-inactive` and so on. A part's image is a bitmap `.xpm`, on top of which a
// `.png` or `.svg` of the same name is laid. Sides and title bar parts are
// tiles: they repeat over the whole length.

/// Size of a picture in pixels from the file header: XPM (the first data line),
/// PNG (the IHDR block), SVG (`width`/`height` of the root or `viewBox`).
pub fn image_size(path: &std::path::Path) -> Option<(f64, f64)> {
    use std::io::Read;
    let mut head = vec![0u8; 4096];
    let n = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
    head.truncate(n);
    let ok = |w: f64, h: f64| (w >= 1.0 && h >= 1.0 && w <= 4096.0 && h <= 4096.0).then_some((w, h));
    if head.starts_with(b"\x89PNG\r\n\x1a\n") && head.len() >= 24 {
        let be = |at: usize| u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]) as f64;
        return ok(be(16), be(20));
    }
    let text = String::from_utf8_lossy(&head);
    if text.trim_start().starts_with("/* XPM */") {
        // The first quoted line: "width height colors chars-per-pixel".
        let values = text.split('"').nth(1)?;
        let mut nums = values.split_whitespace().map(|v| v.parse::<f64>().ok());
        return ok(nums.next()??, nums.next()??);
    }
    let tag = &text[text.find("<svg")?..];
    let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
    let attr = |name: &str| -> Option<&str> {
        let at = tag.match_indices(name).find(|(i, _)| tag[..*i].ends_with(char::is_whitespace))?.0;
        let rest = tag[at + name.len()..].trim_start().strip_prefix('=')?.trim_start();
        let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        rest[1..].split(quote).next()
    };
    let number = |v: &str| v.trim().trim_end_matches("px").parse::<f64>().ok();
    if let (Some(w), Some(h)) = (attr("width").and_then(number), attr("height").and_then(number)) {
        return ok(w, h);
    }
    let mut view = attr("viewBox")?.split([' ', ',']).filter(|v| !v.is_empty()).map(|v| v.parse::<f64>().ok());
    let (_, _, w, h) = (view.next()??, view.next()??, view.next()??, view.next()??);
    ok(w, h)
}

/// An xfwm4 frame part: the layers of its image from bottom to top, and the size.
struct FramePart {
    layers: Vec<String>,
    width: f64,
    height: f64,
}

/// Part `name` of the active window in the theme directory; `None` — no image.
fn xfwm4_part(dir: &std::path::Path, name: &str) -> Option<FramePart> {
    let file = |ext: &str| Some(dir.join(format!("{name}-active.{ext}"))).filter(|p| p.is_file());
    let base = file("xpm");
    let over = ["png", "svg"].iter().find_map(|ext| file(ext));
    // The part's size is set by its bitmap, and without it — by the overlay.
    let (width, height) = base.iter().chain(over.iter()).find_map(|p| image_size(p))?;
    let layers = base.into_iter().chain(over).map(|p| p.to_string_lossy().into_owned()).collect();
    Some(FramePart { layers, width, height })
}

/// A part's image at its own size with the top left corner at `(x, y)`.
fn part_at(ops: &mut Vec<DrawOp>, part: &FramePart, x: f64, y: f64) {
    for path in &part.layers {
        ops.push(DrawOp::Image { rect: (x, y, part.width, part.height), path: path.clone(), cover: false });
    }
}

/// A part's image as a tile over a rectangle.
fn part_tiled(ops: &mut Vec<DrawOp>, part: &FramePart, rect: Rect) {
    if rect.2 <= 0.0 || rect.3 <= 0.0 {
        return;
    }
    for path in &part.layers {
        ops.push(DrawOp::Tiled { rect, path: path.clone() });
    }
}

/// A `key=value` value in the xfwm4 theme settings (`themerc`, without groups).
fn themerc_value<'t>(text: &'t str, key: &str) -> Option<&'t str> {
    text.lines().map(str::trim).filter(|l| !l.starts_with('#')).find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then_some(v.trim())
    })
}

/// xfwm4 title bar buttons by the letters of the layout (`button_layout` in `themerc`).
const XFWM4_BUTTONS: &[(char, &str)] =
    &[('O', "menu"), ('T', "stick"), ('S', "shade"), ('H', "hide"), ('M', "maximize"), ('C', "close")];
/// The xfwm4 button layout when the theme sets none of its own: the window menu
/// on the left, "shade", "minimize", "maximize", "close" on the right.
const XFWM4_DEFAULT_LAYOUT: &str = "O|SHMC";

/// An XFCE frame theme: a window in its frame — corners, sides, title bar and
/// buttons from its own images, at their real size.
fn xfwm4_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    let dir = std::path::Path::new(input.install_path).join("xfwm4");
    let part = |name: &str| xfwm4_part(&dir, name);
    let rc = read_text(&dir.join("themerc")).unwrap_or_default();
    let number = |key: &str| themerc_value(&rc, key).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let (top_left, top_right) = (part("top-left"), part("top-right"));
    let titles: Vec<Option<FramePart>> = (1..=5).map(|n| part(&format!("title-{n}"))).collect();
    // Without a title bar there is no frame: the theme is empty or not built from
    // images.
    let title_h = titles.iter().flatten().map(|p| p.height).fold(0.0, f64::max);
    if title_h < 1.0 {
        return None;
    }
    let (x, y, w, h) = (30.0, 16.0, 240.0, 138.0);
    let title_h = title_h.min(h / 2.0);
    let (left, right, bottom) = (part("left"), part("right"), part("bottom"));
    let (bottom_left, bottom_right) = (part("bottom-left"), part("bottom-right"));
    let width_of = |p: &Option<FramePart>| p.as_ref().map(|p| p.width.min(w / 3.0)).unwrap_or(0.0);
    let height_of = |p: &Option<FramePart>| p.as_ref().map(|p| p.height.min(h / 3.0)).unwrap_or(0.0);
    let (left_w, right_w) = (width_of(&left), width_of(&right));
    let bottom_h = height_of(&bottom).max(height_of(&bottom_left)).max(height_of(&bottom_right));

    let mut ops = vec![
        // The window's shadow and contents: neutral, a frame theme has none of its own.
        DrawOp::Fill { rect: (x + 3.0, y + 4.0, w, h), color: (0, 0, 0, 70) },
        DrawOp::Fill { rect: (x, y + title_h, w, h - title_h), color: (236, 236, 236, 255) },
    ];
    for (line_y, line_w) in [(18.0, 120.0), (34.0, 86.0), (50.0, 104.0)] {
        ops.push(DrawOp::Fill { rect: (x + left_w + 14.0, y + title_h + line_y, line_w, 6.0), color: (120, 120, 120, 200) });
    }
    // The title bar: the corners, five parts between them. The first and the
    // fifth stretch as tiles to the middle, the third lies under the window
    // title, the second and fourth are transitions to it.
    let (corner_l, corner_r) = (width_of(&top_left), width_of(&top_right));
    let (span_x, span_w) = (x + corner_l, w - corner_l - corner_r);
    let (t2, t4) = (width_of(&titles[1]), width_of(&titles[3]));
    let text_w = (span_w - t2 - t4).clamp(0.0, 84.0);
    let text_x = span_x + ((span_w - text_w) / 2.0).round();
    if let Some(p) = titles[0].as_ref().or(titles[2].as_ref()) {
        part_tiled(&mut ops, p, (span_x, y, text_x - t2 - span_x, p.height.min(title_h)));
    }
    if let Some(p) = titles[4].as_ref().or(titles[2].as_ref()) {
        let from = text_x + text_w + t4;
        part_tiled(&mut ops, p, (from, y, span_x + span_w - from, p.height.min(title_h)));
    }
    if let Some(p) = titles[2].as_ref().or(titles[0].as_ref()) {
        part_tiled(&mut ops, p, (text_x, y, text_w, p.height.min(title_h)));
    }
    if let Some(p) = &titles[1] {
        part_at(&mut ops, p, text_x - t2, y);
    }
    if let Some(p) = &titles[3] {
        part_at(&mut ops, p, text_x + text_w, y);
    }
    if let Some(p) = &top_left {
        part_at(&mut ops, p, x, y);
    }
    if let Some(p) = &top_right {
        part_at(&mut ops, p, x + w - corner_r, y);
    }
    // Sides and the bottom.
    if let Some(p) = &left {
        part_tiled(&mut ops, p, (x, y + title_h, left_w, h - title_h - bottom_h));
    }
    if let Some(p) = &right {
        part_tiled(&mut ops, p, (x + w - right_w, y + title_h, right_w, h - title_h - bottom_h));
    }
    let (bl_w, br_w) = (width_of(&bottom_left), width_of(&bottom_right));
    if let Some(p) = &bottom {
        part_tiled(&mut ops, p, (x + bl_w, y + h - p.height.min(bottom_h), w - bl_w - br_w, p.height.min(bottom_h)));
    }
    if let Some(p) = &bottom_left {
        part_at(&mut ops, p, x, y + h - p.height.min(bottom_h));
    }
    if let Some(p) = &bottom_right {
        part_at(&mut ops, p, x + w - br_w, y + h - p.height.min(bottom_h));
    }
    // The window title — as a strip of the title text color, if the theme
    // named it as a number (rather than a GTK theme color name).
    if let Some(ink) = themerc_value(&rc, "active_text_color").and_then(css_color) {
        if text_w >= 40.0 {
            ops.push(DrawOp::Fill {
                rect: (text_x + (text_w - 60.0) / 2.0, y + (title_h / 2.0).round() - 3.0, 60.0, 6.0),
                color: (ink.0, ink.1, ink.2, 220),
            });
        }
    }
    // Buttons: before the layout's bar — left to right, after it — right to left.
    let layout = themerc_value(&rc, "button_layout").filter(|l| !l.is_empty()).unwrap_or(XFWM4_DEFAULT_LAYOUT);
    let (offset, spacing) = (number("button_offset").clamp(0.0, 20.0), number("button_spacing").clamp(-4.0, 12.0));
    let button = |letter: char| {
        XFWM4_BUTTONS.iter().find(|(l, _)| *l == letter).and_then(|(_, name)| part(name)).filter(|p| p.height <= h / 2.0)
    };
    let (before, after) = layout.split_once('|').unwrap_or(("", layout));
    let mut at = x + left_w + offset;
    for p in before.chars().filter_map(button) {
        part_at(&mut ops, &p, at, y + ((title_h - p.height) / 2.0).round().max(0.0));
        at += p.width + spacing;
    }
    let mut at = x + w - right_w - offset;
    for p in after.chars().rev().filter_map(button) {
        at -= p.width;
        part_at(&mut ops, &p, at, y + ((title_h - p.height) / 2.0).round().max(0.0));
        at -= spacing;
    }
    Some(ops)
}

// ── Openbox and labwc frames ──────────────────────────────────────────────
//
// An Openbox theme is a `themerc` (lines `key: value`, comments `!` and `#`) and
// `.xbm` button images next to it. labwc reads the same format and looks for its
// copy of the theme in `<theme>/labwc/` before `<theme>/openbox-3/`
// (labwc-theme(5)). Such themes almost never have a picture of their own, and
// the sample is drawn from colors: an active window over an inactive one — a
// title bar with a color gradient, the title, buttons and the frame.

/// The theme's `themerc` for the sample: `labwc/`, then `openbox-3/`.
fn openbox_themerc(theme: &std::path::Path) -> Option<(std::path::PathBuf, String)> {
    ["labwc", "openbox-3"]
        .iter()
        .map(|sub| theme.join(sub))
        .find_map(|dir| read_text(&dir.join("themerc")).map(|t| (dir, t)))
}

/// Whether a key matches a pattern from `themerc`, where `*` is one or several
/// name parts (`window.*.button.*.image.color`).
fn ob_key_matches(pattern: &[&str], key: &[&str]) -> bool {
    match (pattern.first(), key.first()) {
        (None, None) => true,
        (Some(&"*"), Some(_)) => (1..=key.len()).any(|n| ob_key_matches(&pattern[1..], &key[n..])),
        (Some(p), Some(k)) => p.eq_ignore_ascii_case(k) && ob_key_matches(&pattern[1..], &key[1..]),
        _ => false,
    }
}

/// Value of an Openbox theme key: an exact key is more important than patterns
/// with `*`; among patterns — the one with fewer `*`, on a tie — the last in the
/// file.
fn ob_value<'t>(text: &'t str, key: &str) -> Option<&'t str> {
    let key: Vec<&str> = key.split('.').collect();
    let mut best: Option<(usize, &'t str)> = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with('!') || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let pattern: Vec<&str> = k.trim().split('.').collect();
        if !ob_key_matches(&pattern, &key) {
            continue;
        }
        let stars = pattern.iter().filter(|p| **p == "*").count();
        if best.is_none_or(|(b, _)| stars <= b) {
            best = Some((stars, v.trim()));
        }
    }
    best.map(|(_, v)| v)
}

/// A color in X11 notation, as Openbox themes write it: `#rgb`, `#rrggbb`,
/// `rgb:r/g/b` (1–4 digits per channel), `grey40`/`gray40`, several names.
fn ob_color(value: &str) -> Option<Rgb> {
    let v = value.trim();
    let low = v.to_ascii_lowercase();
    if let Some(rest) = low.strip_prefix("rgb:") {
        let mut parts = rest.split('/').map(|p| {
            let p = p.trim();
            let n = u32::from_str_radix(p, 16).ok()?;
            let max = (1u32 << (4 * p.len().clamp(1, 4) as u32)) - 1;
            Some((n as f64 * 255.0 / max as f64).round() as u8)
        });
        return Some((parts.next()??, parts.next()??, parts.next()??));
    }
    for prefix in ["grey", "gray"] {
        if let Some(n) = low.strip_prefix(prefix) {
            if n.is_empty() {
                return Some((190, 190, 190));
            }
            let pct: f64 = n.parse().ok()?;
            let c = (pct.clamp(0.0, 100.0) * 255.0 / 100.0).round() as u8;
            return Some((c, c, c));
        }
    }
    match low.as_str() {
        "red" => Some((255, 0, 0)),
        "green" => Some((0, 255, 0)),
        "blue" => Some((0, 0, 255)),
        "yellow" => Some((255, 255, 0)),
        "orange" => Some((255, 165, 0)),
        _ => css_color(v),
    }
}

/// An `.xbm` image: width, height and pixels row by row (`true` — filled).
fn xbm_bits(text: &str) -> Option<(u32, u32, Vec<bool>)> {
    let define = |suffix: &str| {
        text.lines().find_map(|l| {
            let mut w = l.split_whitespace();
            (w.next()? == "#define" && w.next()?.ends_with(suffix)).then(|| w.next()?.parse::<u32>().ok()).flatten()
        })
    };
    let (width, height) = (define("_width")?, define("_height")?);
    if width == 0 || height == 0 || width > 64 || height > 64 {
        return None;
    }
    let body = text.split_once('{')?.1.split('}').next()?;
    let bytes: Vec<u8> = body
        .split(',')
        .filter_map(|b| {
            let b = b.trim();
            let hex = b.strip_prefix("0x").or_else(|| b.strip_prefix("0X"))?;
            u8::from_str_radix(hex, 16).ok()
        })
        .collect();
    let row = width.div_ceil(8) as usize;
    if bytes.len() < row * height as usize {
        return None;
    }
    let mut bits = Vec::with_capacity((width * height) as usize);
    for y in 0..height as usize {
        for x in 0..width as usize {
            bits.push(bytes[y * row + x / 8] >> (x % 8) & 1 == 1);
        }
    }
    Some((width, height, bits))
}

/// An Openbox title bar button: the theme's `.xbm` image in color `color`, twice
/// as large; no image — a simple glyph made of lines.
fn ob_button(ops: &mut Vec<DrawOp>, dir: &std::path::Path, name: &str, cx: f64, cy: f64, color: Rgb) {
    let ink = (color.0, color.1, color.2, 255);
    if let Some((w, h, bits)) = read_text(&dir.join(format!("{name}.xbm"))).and_then(|t| xbm_bits(&t)) {
        const SCALE: u32 = 2;
        let (sw, sh) = (w * SCALE, h * SCALE);
        let mut rgba = vec![0u8; (sw * sh * 4) as usize];
        for y in 0..sh {
            for x in 0..sw {
                if bits[((y / SCALE) * w + x / SCALE) as usize] {
                    let at = ((y * sw + x) * 4) as usize;
                    rgba[at..at + 4].copy_from_slice(&[ink.0, ink.1, ink.2, 255]);
                }
            }
        }
        ops.push(DrawOp::Pixels { x: (cx - sw as f64 / 2.0).round(), y: (cy - sh as f64 / 2.0).round(), width: sw, height: sh, rgba });
        return;
    }
    let r = 4.5;
    match name {
        "close" => {
            ops.push(DrawOp::Line { from: (cx - r, cy - r), to: (cx + r, cy + r), width: 2.0, color: ink });
            ops.push(DrawOp::Line { from: (cx - r, cy + r), to: (cx + r, cy - r), width: 2.0, color: ink });
        }
        "max" => {
            for (from, to) in [((-r, -r), (r, -r)), ((r, -r), (r, r)), ((r, r), (-r, r)), ((-r, r), (-r, -r))] {
                ops.push(DrawOp::Line { from: (cx + from.0, cy + from.1), to: (cx + to.0, cy + to.1), width: 1.5, color: ink });
            }
            ops.push(DrawOp::Line { from: (cx - r, cy - r + 1.5), to: (cx + r, cy - r + 1.5), width: 1.5, color: ink });
        }
        _ => ops.push(DrawOp::Line { from: (cx - r, cy + r), to: (cx + r, cy + r), width: 2.0, color: ink }),
    }
}

/// One window of an Openbox sample: `state` — `active` or `inactive`.
fn ob_window(ops: &mut Vec<DrawOp>, dir: &std::path::Path, rc: &str, state: &str, rect: Rect) {
    let (x, y, w, h) = rect;
    let get = |key: &str| ob_value(rc, &format!("window.{state}.{key}"));
    let color = |key: &str| get(key).and_then(ob_color);
    let title = color("title.bg.color").unwrap_or(if state == "active" { (70, 110, 160) } else { (200, 200, 200) });
    let title_to = color("title.bg.colorTo").unwrap_or(title);
    let gradient = get("title.bg").is_some_and(|t| t.to_ascii_lowercase().contains("gradient"));
    let label = color("label.text.color").unwrap_or(if state == "active" { (255, 255, 255) } else { (80, 80, 80) });
    let button = get("button.unpressed.image.color").and_then(ob_color).unwrap_or(label);
    let border = color("border.color")
        .or_else(|| ob_value(rc, "border.color").and_then(ob_color))
        .unwrap_or((0, 0, 0));
    let client = color("client.color").unwrap_or((236, 236, 236));
    let bw = ob_value(rc, "border.width").and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0).clamp(0.0, 4.0);
    let th = 22.0;
    ops.push(DrawOp::Fill { rect: (x + 3.0, y + 4.0, w, h), color: (0, 0, 0, 60) });
    ops.push(DrawOp::Fill { rect: (x, y, w, h), color: (border.0, border.1, border.2, 255) });
    let (ix, iy, iw) = (x + bw, y + bw, w - 2.0 * bw);
    let title_rect = (ix, iy, iw, th);
    if gradient && title != title_to {
        ops.push(DrawOp::Shade {
            rect: title_rect,
            direction: ShadeDirection::Vertical,
            colors: vec![(title.0, title.1, title.2, 255), (title_to.0, title_to.1, title_to.2, 255)],
        });
    } else {
        ops.push(DrawOp::Fill { rect: title_rect, color: (title.0, title.1, title.2, 255) });
    }
    ops.push(DrawOp::Fill { rect: (ix, iy + th, iw, h - th - 2.0 * bw), color: (client.0, client.1, client.2, 255) });
    // The window title — as a strip of the title text color, in the middle.
    ops.push(DrawOp::Fill { rect: (ix + (iw - 64.0) / 2.0, iy + th / 2.0 - 3.0, 64.0, 6.0), color: (label.0, label.1, label.2, 230) });
    // Lines of "contents" — in a color distinguishable from the window background.
    let ink = mix(client, (128, 128, 128), 0.7);
    for (dy, lw) in [(14.0, 0.5), (28.0, 0.36), (42.0, 0.44)] {
        if dy + 8.0 < h - th {
            ops.push(DrawOp::Fill { rect: (ix + 12.0, iy + th + dy, (iw * lw).round(), 6.0), color: (ink.0, ink.1, ink.2, 200) });
        }
    }
    // The default Openbox button layout (`NLIMC`): "minimize", "maximize", "close"
    // on the right.
    let cy = iy + th / 2.0;
    let mut cx = ix + iw - 12.0;
    for name in ["close", "max", "iconify"] {
        ob_button(ops, dir, name, cx, cy, button);
        cx -= 20.0;
    }
}

/// An Openbox and labwc frame theme: an active window over an inactive one in
/// its colors.
fn openbox_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    let (dir, rc) = openbox_themerc(std::path::Path::new(input.install_path))?;
    // A theme without a single title bar color is not a frame theme (an empty file).
    ob_value(&rc, "window.active.title.bg.color")?;
    let mut ops = Vec::new();
    ob_window(&mut ops, &dir, &rc, "inactive", (14.0, 12.0, 196.0, 96.0));
    ob_window(&mut ops, &dir, &rc, "active", (66.0, 46.0, 220.0, 112.0));
    Some(ops)
}

// ── GNOME and Cinnamon shells ─────────────────────────────────────────────
//
// A shell theme is a stylesheet of its parts. It has no named colors like GTK:
// the colors stand right in the rules. The sample needs the two places a person
// sees first — the panel and an open menu; their rules are named the same in
// both shells (`#panel`, `.popup-menu…`, Cinnamon also has `.menu`).

/// A color with transparency from a stylesheet value ([`css_color`] plus the
/// fourth number of `rgba(…)` and the last two digits of `#rrggbbaa`).
fn css_color_alpha(value: &str) -> Option<Rgba> {
    let (r, g, b) = css_color(value)?;
    let v = value.trim();
    let alpha = if let Some(hex) = v.strip_prefix('#') {
        let hex: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        match hex.len() {
            8 => u8::from_str_radix(&hex[6..8], 16).ok()?,
            4 => u8::from_str_radix(&hex[3..4], 16).ok()? * 17,
            _ => 255,
        }
    } else {
        let inner = v.split_once('(').and_then(|(_, rest)| rest.split(')').next()).unwrap_or("");
        match inner.split(',').nth(3).and_then(|a| a.trim().parse::<f64>().ok()) {
            Some(a) => (a.clamp(0.0, 1.0) * 255.0).round() as u8,
            None => 255,
        }
    };
    Some((r, g, b, alpha))
}

/// Stylesheet rules: the list of selectors and the declarations of each. Shell
/// stylesheets have no nested blocks; comments are dropped.
fn css_rules(css: &str) -> Vec<(Vec<String>, Vec<(&str, &str)>)> {
    let mut rules = Vec::new();
    for chunk in css.split('}') {
        let Some((selectors, body)) = chunk.split_once('{') else { continue };
        // A comment before a rule stays in its "selector" — we cut it off.
        let selectors = selectors.rsplit("*/").next().unwrap_or(selectors);
        let selectors: Vec<String> =
            selectors.split(',').map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        let declarations = body
            .split(';')
            .filter_map(|d| d.split_once(':'))
            .map(|(k, v)| (k.trim(), v.trim()))
            .collect();
        rules.push((selectors, declarations));
    }
    rules
}

/// The color of a property from `properties` in the first rule whose selector is
/// one of `selectors`: first an exact match, then a descendant rule (`… .name`).
/// An almost transparent color (less than a third) does not count as the place's
/// color.
fn css_rule_color(rules: &[(Vec<String>, Vec<(&str, &str)>)], selectors: &[&str], properties: &[&str]) -> Option<Rgba> {
    selectors.iter().find_map(|want| {
        let nested = format!(" {want}");
        [false, true].iter().find_map(|descendant| {
            rules
                .iter()
                .filter(|(names, _)| names.iter().any(|n| if *descendant { n.ends_with(&nested) } else { n == want }))
                .find_map(|(_, declarations)| {
                    properties.iter().find_map(|p| {
                        declarations.iter().filter(|(k, _)| k == p).find_map(|(_, v)| css_color_alpha(v))
                    })
                })
                .filter(|c| c.3 >= 85)
        })
    })
}

/// Shell colors from which its sample is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellPalette {
    /// Panel background.
    pub panel: Rgba,
    /// Text on the panel.
    pub panel_text: (u8, u8, u8),
    /// Menu background.
    pub menu: Rgba,
    /// Menu text.
    pub menu_text: (u8, u8, u8),
    /// Background of the selected menu row.
    pub selected: Rgba,
}

/// Panel and menu colors from the stylesheet of a GNOME or Cinnamon shell.
/// `None` — the stylesheet has a background for neither the panel nor the menu.
pub fn shell_palette(css: &str) -> Option<ShellPalette> {
    let rules = css_rules(css);
    const BACKGROUND: &[&str] = &["background-color", "background-gradient-start", "background"];
    let find = |selectors: &[&str], properties: &[&str]| css_rule_color(&rules, selectors, properties);
    let panel = find(&["#panel"], BACKGROUND);
    let menu = find(
        &[".popup-menu-content", ".menu", ".popup-menu", ".popup-combo-menu", ".popup-menu-boxpointer"],
        &["background-color", "-arrow-background-color", "background-gradient-start", "background"],
    );
    if panel.is_none() && menu.is_none() {
        return None;
    }
    let solid = |c: Rgba| (c.0, c.1, c.2);
    // What the stylesheet does not name is taken from the neighbor; text
    // indistinguishable on its background (taken from another rule) is replaced
    // with white or black.
    let menu = menu.or(panel)?;
    let panel = panel.unwrap_or(menu);
    let readable = |text: Option<Rgba>, on: Rgba| {
        let luma = 0.299 * on.0 as f64 + 0.587 * on.1 as f64 + 0.114 * on.2 as f64;
        let dark = luma < 128.0;
        let fallback = if dark { (255, 255, 255) } else { (0, 0, 0) };
        text.map(solid).filter(|t| contrast(*t, solid(on))).unwrap_or(fallback)
    };
    let panel_text = readable(find(&["#panel", ".panel-button", ".panel-top", "stage"], &["color"]), panel);
    let menu_text = readable(find(&[".popup-menu-content", ".menu", ".popup-menu", ".popup-menu-item", "stage"], &["color"]), menu);
    let selected = find(
        &[".popup-menu-item:selected", ".popup-menu-item:active", ".popup-menu-item:focus", ".popup-menu-item:hover", ".menu-application-button-selected"],
        BACKGROUND,
    )
    .unwrap_or_else(|| {
        let m = mix(solid(menu), menu_text, 0.16);
        (m.0, m.1, m.2, 255)
    });
    Some(ShellPalette { panel, panel_text, menu, menu_text, selected })
}

/// A shell sample: a panel at the edge of the screen and an open menu next to it.
pub fn shell_sample(p: &ShellPalette, panel_at_bottom: bool) -> Vec<DrawOp> {
    let fill = |x: f64, y: f64, w: f64, h: f64, c: Rgba| DrawOp::Fill { rect: (x, y, w, h), color: c };
    let ink = |c: (u8, u8, u8), a: u8| (c.0, c.1, c.2, a);
    let (width, height) = (PREVIEW_WIDTH as f64, PREVIEW_HEIGHT as f64);
    let panel_h = 24.0;
    let panel_y = if panel_at_bottom { height - panel_h } else { 0.0 };
    let (menu_w, menu_h) = (132.0, 116.0);
    // The menu opens from the panel: for a top one — on the right under it
    // (the GNOME status menu), for a bottom one — on the left above it (the
    // Cinnamon main menu).
    let (menu_x, menu_y) = if panel_at_bottom { (10.0, panel_y - menu_h - 6.0) } else { (width - menu_w - 10.0, panel_h + 6.0) };
    let text_y = panel_y + 9.0;
    let mut ops = vec![
        fill(0.0, panel_y, width, panel_h, p.panel),
        fill(12.0, text_y, 54.0, 6.0, ink(p.panel_text, 230)),
        fill(width / 2.0 - 22.0, text_y, 44.0, 6.0, ink(p.panel_text, 230)),
    ];
    for i in 0..3 {
        ops.push(fill(width - 22.0 - i as f64 * 16.0, text_y - 1.0, 8.0, 8.0, ink(p.panel_text, 200)));
    }
    ops.push(fill(menu_x + 3.0, menu_y + 4.0, menu_w, menu_h, (0, 0, 0, 70)));
    ops.push(fill(menu_x, menu_y, menu_w, menu_h, p.menu));
    for (row, line_w) in [70.0, 88.0, 56.0, 78.0].into_iter().enumerate() {
        let row_y = menu_y + 10.0 + row as f64 * 25.0;
        if row == 1 {
            ops.push(fill(menu_x, row_y, menu_w, 22.0, p.selected));
        }
        ops.push(fill(menu_x + 14.0, row_y + 8.0, line_w, 6.0, ink(p.menu_text, 220)));
    }
    ops
}

/// A shell sample from its stylesheet `<theme>/<rel>`.
fn shell_theme_sample(input: &SampleInput, rel: &str, panel_at_bottom: bool) -> Option<Vec<DrawOp>> {
    let css = read_text(&std::path::Path::new(input.install_path).join(rel))?;
    Some(shell_sample(&shell_palette(&css)?, panel_at_bottom))
}

/// A GNOME shell theme: the top panel and a menu in its colors.
fn gnome_shell_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    shell_theme_sample(input, "gnome-shell/gnome-shell.css", false)
}

/// A Cinnamon theme: the bottom panel and a menu in its colors.
fn cinnamon_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    shell_theme_sample(input, "cinnamon/cinnamon.css", true)
}

// ── icons ────────────────────────────────────────────────────────────────

/// Icon directories of a theme from its `index.theme`, from the most suitable
/// size to the least suitable (closer to `want` pixels, on a tie — larger).
/// Double-scale directories are skipped: the image there is the same.
fn icon_dirs(theme_dir: &std::path::Path, want: i64) -> Vec<String> {
    let Some(index) = read_text(&theme_dir.join("index.theme")) else {
        return Vec::new();
    };
    let listed: std::collections::HashSet<&str> = ini_value(&index, "Icon Theme", "Directories")
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .collect();
    let mut dirs: Vec<(i64, i64, String)> = Vec::new();
    let mut current: Option<(String, i64, i64)> = None;
    let flush = |cur: Option<(String, i64, i64)>, dirs: &mut Vec<(i64, i64, String)>| {
        if let Some((name, size, scale)) = cur {
            if scale <= 1 && listed.contains(name.as_str()) {
                dirs.push(((size - want).abs(), -size, name));
            }
        }
    };
    for line in index.lines() {
        let s = line.trim();
        if let Some(name) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            flush(current.take(), &mut dirs);
            current = Some((name.to_string(), want, 1));
        } else if let (Some((k, v)), Some(cur)) = (s.split_once('='), current.as_mut()) {
            match k.trim() {
                "Size" => cur.1 = v.trim().parse().unwrap_or(want),
                "Scale" => cur.2 = v.trim().parse().unwrap_or(1),
                _ => {}
            }
        }
    }
    flush(current.take(), &mut dirs);
    dirs.sort();
    dirs.into_iter().map(|(_, _, name)| name).collect()
}

/// The file of icon `name` in the theme's directories `dirs` (the first one found).
fn icon_file(theme_dir: &std::path::Path, dirs: &[String], name: &str) -> Option<String> {
    dirs.iter().find_map(|d| {
        ["svg", "png", "svgz"]
            .iter()
            .map(|ext| theme_dir.join(d).join(format!("{name}.{ext}")))
            .find(|p| p.is_file())
            .map(|p| p.to_string_lossy().into_owned())
    })
}

/// Which icons to show on a theme sample: each row holds names of the same icon
/// from the currently accepted one to the older ones.
const ICON_SAMPLE_NAMES: &[&[&str]] = &[
    &["folder", "inode-directory"],
    &["user-home", "folder-home"],
    &["folder-documents", "folder-document"],
    &["folder-download", "folder-downloads"],
    &["user-trash", "user-trash-full"],
    &["internet-web-browser", "web-browser", "firefox"],
    &["system-file-manager", "file-manager"],
    &["preferences-system", "systemsettings", "preferences-desktop"],
    &["utilities-terminal", "terminal"],
    &["text-x-generic", "text-plain"],
    &["image-x-generic"],
    &["audio-x-generic"],
    &["computer"],
    &["applications-multimedia", "multimedia-player"],
];

const ICON_SAMPLE_COLUMNS: usize = 4;
const ICON_SAMPLE_MAX: usize = 8;
const ICON_SAMPLE_SIZE: f64 = 56.0;

/// An icon theme: its own icons in a grid. Only the theme's own files are taken
/// — without the themes it inherits from: the sample shows it, not them.
fn icons_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    let theme = std::path::Path::new(input.install_path);
    let dirs = icon_dirs(theme, 64);
    let mut files: Vec<String> = Vec::new();
    for names in ICON_SAMPLE_NAMES {
        if files.len() >= ICON_SAMPLE_MAX {
            break;
        }
        if let Some(file) = names.iter().find_map(|n| icon_file(theme, &dirs, n)) {
            files.push(file);
        }
    }
    // A theme without the usual names (only application icons): we fill
    // up with the first files of its best directories in order.
    for d in dirs.iter().take(12) {
        if files.len() >= ICON_SAMPLE_MAX {
            break;
        }
        let Ok(entries) = std::fs::read_dir(theme.join(d)) else { continue };
        let mut found: Vec<String> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "svg" || e == "png"))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        found.sort();
        for file in found {
            let stem = |f: &str| std::path::Path::new(f).file_stem().map(|s| s.to_os_string());
            if files.len() < ICON_SAMPLE_MAX && !files.iter().any(|have| stem(have) == stem(&file)) {
                files.push(file);
            }
        }
    }
    if files.len() < 2 {
        return None;
    }
    let rows = files.len().div_ceil(ICON_SAMPLE_COLUMNS);
    let (cell_w, cell_h) = (
        PREVIEW_WIDTH as f64 / ICON_SAMPLE_COLUMNS as f64,
        PREVIEW_HEIGHT as f64 / rows as f64,
    );
    let total = files.len();
    Some(
        files
            .into_iter()
            .enumerate()
            .map(|(i, path)| {
                let (col, row) = (i % ICON_SAMPLE_COLUMNS, i / ICON_SAMPLE_COLUMNS);
                // The last incomplete row — centered.
                let in_row = if row == rows - 1 { total - row * ICON_SAMPLE_COLUMNS } else { ICON_SAMPLE_COLUMNS };
                let shift = (PREVIEW_WIDTH as f64 - in_row as f64 * cell_w) / 2.0;
                DrawOp::Image {
                    rect: (
                        shift + col as f64 * cell_w + (cell_w - ICON_SAMPLE_SIZE) / 2.0,
                        row as f64 * cell_h + (cell_h - ICON_SAMPLE_SIZE) / 2.0,
                        ICON_SAMPLE_SIZE,
                        ICON_SAMPLE_SIZE,
                    ),
                    path,
                    cover: false,
                }
            })
            .collect(),
    )
}

/// Icon theme directories in which a package icon is looked for by name: themes
/// shipped with the environments, in the user and system directories.
pub fn icon_theme_dirs(home: &str, usr_root: &str) -> Vec<String> {
    let roots = [
        format!("{home}/.local/share/icons"),
        format!("{home}/.icons"),
        format!("{usr_root}/share/icons"),
    ];
    let mut out = Vec::new();
    for theme in ["breeze", "breeze-dark", "Adwaita", "oxygen", "hicolor"] {
        for root in &roots {
            let dir = format!("{root}/{theme}");
            if std::path::Path::new(&dir).join("index.theme").is_file() {
                out.push(dir);
            }
        }
    }
    out
}

/// A package (a window switcher, a KWin effect and script, an extension, an
/// applet) or a Nemo action: its icon is the `icon.png` file in the package or an
/// icon named by the metadata (`metadata.json`, `metadata.desktop`, for an action
/// — the `Icon-Name` key of the `.nemo_action` file itself).
fn package_icon_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    let dir = std::path::Path::new(input.install_path);
    let own = dir.join("icon.png");
    let named = || -> Option<String> {
        let from_json = read_text(&dir.join("metadata.json")).and_then(|t| {
            let data: serde_json::Value = serde_json::from_str(&t).ok()?;
            data["KPlugin"]["Icon"].as_str().or(data["icon"].as_str()).map(str::to_string)
        });
        let name = from_json
            .or_else(|| {
                let text = read_text(&dir.join("metadata.desktop"))?;
                ini_value(&text, "Desktop Entry", "Icon").map(str::to_string)
            })
            .or_else(|| {
                // A Nemo action is a single file: the metadata lies in it.
                let text = read_text(dir)?;
                ini_value(&text, "Nemo Action", "Icon-Name").map(str::to_string)
            })
            .filter(|n| !n.is_empty())?;
        if name.starts_with('/') {
            return std::path::Path::new(&name).is_file().then_some(name);
        }
        input.icon_themes.iter().find_map(|theme| {
            let theme = std::path::Path::new(theme);
            icon_file(theme, &icon_dirs(theme, 96), &name)
        })
    };
    let path = if own.is_file() { own.to_string_lossy().into_owned() } else { named()? };
    let mut ops = Vec::new();
    // A monochrome icon (`…-symbolic`) is drawn dark on transparent: it cannot be
    // seen on a dark background — a light plate is put under it.
    if path.rsplit('/').next().is_some_and(|file| file.contains("-symbolic.")) {
        ops.push(DrawOp::Fill { rect: (94.0, 29.0, 112.0, 112.0), color: (236, 236, 236, 255) });
    }
    ops.push(DrawOp::Image { rect: (102.0, 37.0, 96.0, 96.0), path, cover: false });
    Some(ops)
}

// ── a whole theme of the desktop (MATE metatheme, Cinnamon style) ─────────
//
// A metatheme ships no picture of its own: MATE builds its thumbnail itself
// (`theme-thumbnail.c` of mate-control-center: the Marco frame of the theme
// around real GTK widgets in its controls theme, and the folder icon of its
// icon set in the corner). The tile does the same from the theme's parts: the
// window in the frame of its window border theme, inside it the snapshot of
// real widgets of its controls theme (or the window colors when there is no
// snapshot), the icons of its icon set beside it and its pointer on top.

/// The frame of a whole-theme picture in logical points; the full-screen view
/// gets the same frame drawn [`WHOLE_BIG_SCALE`] times larger.
pub const WHOLE_WIDTH: i64 = 600;
/// See [`WHOLE_WIDTH`].
pub const WHOLE_HEIGHT: i64 = 340;
/// How many times larger the full-screen picture is drawn.
pub const WHOLE_BIG_SCALE: f64 = 2.0;
/// The window contents: exactly the size of the widget form of a shot
/// ([`crate::widget_shot::FORM_WIDTH`]), so the widgets stand at their own size.
pub const WHOLE_CLIENT: (f64, f64) = (crate::widget_shot::FORM_WIDTH as f64, crate::widget_shot::FORM_HEIGHT as f64);
/// The size of the widget shot for a whole theme in screen pixels: the window
/// contents of the full-screen picture.
pub const WHOLE_SHOT_SIZE: (i32, i32) =
    ((WHOLE_CLIENT.0 * WHOLE_BIG_SCALE) as i32, (WHOLE_CLIENT.1 * WHOLE_BIG_SCALE) as i32);

/// What a whole-theme picture is drawn from: the directories of its parts on
/// disk (`None` — the part is not named or not installed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WholeTheme {
    /// The window title.
    pub name: String,
    /// The controls (GTK) theme directory.
    pub gtk_dir: Option<String>,
    /// The window border theme directory (with `metacity-1`).
    pub borders_dir: Option<String>,
    /// The icon set directory first, then the sets it inherits from.
    pub icon_dirs: Vec<String>,
    /// The pointer theme directory (with `cursors`).
    pub cursor_dir: Option<String>,
    /// A snapshot of real widgets in the controls theme
    /// ([`WHOLE_SHOT_SIZE`]), when one was taken.
    pub widgets_shot: Option<String>,
}

/// Icons beside the window: the folder (the names MATE looks up for its
/// thumbnail), the home folder, the trash; the first name found of each.
const WHOLE_ICON_NAMES: &[&[&str]] = &[
    &["x-directory-normal", "folder", "inode-directory"],
    &["user-home", "folder-home"],
    &["user-trash", "user-trash-full"],
];
const WHOLE_ICON_SIZE: f64 = 56.0;
/// The pointer size looked for in the theme files.
const WHOLE_POINTER_NOMINAL: u32 = 32;

/// The file of icon `names` (the first one found) in the set and the sets it
/// inherits from, in the given order.
fn whole_icon(sets: &[String], names: &[&str], want: i64) -> Option<String> {
    sets.iter().find_map(|dir| {
        let dir = std::path::Path::new(dir);
        let dirs = icon_dirs(dir, want);
        names.iter().find_map(|n| icon_file(dir, &dirs, n))
    })
}

/// The picture of a whole theme ([`WHOLE_WIDTH`]×[`WHOLE_HEIGHT`]). It is a
/// placeholder only when there is neither a controls theme nor anything else to
/// draw.
pub fn whole_theme_picture(theme: &WholeTheme, glyph: &str) -> CardPicture {
    let (w, h) = (WHOLE_WIDTH as f64, WHOLE_HEIGHT as f64);
    let mut ops = vec![backdrop_sized(w, h)];
    let gtk = theme
        .gtk_dir
        .as_deref()
        .map(|d| metacity::GtkColors::of_theme(std::path::Path::new(d)))
        .unwrap_or_else(metacity::GtkColors::fallback);
    let p = gtk.palette;
    let solid = |c: Rgb| (c.0, c.1, c.2, 255u8);
    const MARGIN: f64 = 24.0;
    // The frame of the theme's own window borders, or a plain one in the
    // controls colors when it can't be drawn.
    let framed = theme.borders_dir.as_deref().and_then(|d| {
        // A title bar too tall for the picture — the plain frame.
        let probe = metacity::window_frame(std::path::Path::new(d), &gtk, &theme.name, (0.0, 0.0), WHOLE_CLIENT)?;
        let outer_h = probe.1 .1 + WHOLE_CLIENT.1 + 12.0;
        let y = ((h - outer_h) / 2.0).round().max(6.0);
        metacity::window_frame(std::path::Path::new(d), &gtk, &theme.name, (MARGIN, y), WHOLE_CLIENT)
    });
    let drew_frame = framed.is_some();
    let client = match framed {
        Some((frame, client)) => {
            ops.extend(frame);
            client
        }
        None => {
            const TITLE: f64 = 28.0;
            let y = ((h - WHOLE_CLIENT.1 - TITLE) / 2.0).round();
            let outer = (MARGIN, y, WHOLE_CLIENT.0, WHOLE_CLIENT.1 + TITLE);
            ops.push(DrawOp::Fill { rect: (outer.0 + 3.0, outer.1 + 4.0, outer.2, outer.3), color: (0, 0, 0, 70) });
            ops.push(DrawOp::Clip { rect: outer, radii: [6.0, 6.0, 0.0, 0.0] });
            ops.push(DrawOp::Fill { rect: (outer.0, outer.1, outer.2, TITLE), color: solid(p.titlebar) });
            ops.push(DrawOp::Label {
                rect: (outer.0 + 12.0, outer.1, outer.2 - 24.0, TITLE),
                text: theme.name.clone(),
                pixel_size: 13,
                bold: true,
                color: solid(p.text),
            });
            ops.push(DrawOp::Unclip);
            (outer.0, outer.1 + TITLE, WHOLE_CLIENT.0, WHOLE_CLIENT.1)
        }
    };
    // The window contents: the real widgets, or a form in the theme colors.
    ops.push(DrawOp::Fill { rect: client, color: solid(p.window) });
    match &theme.widgets_shot {
        Some(shot) => ops.push(DrawOp::Image { rect: client, path: shot.clone(), cover: false }),
        None => {
            let (x, y, cw, _) = client;
            let ink = mix(p.window, p.text, 0.55);
            ops.push(DrawOp::Fill { rect: (x + 20.0, y + 20.0, 110.0, 30.0), color: solid(p.button) });
            ops.push(DrawOp::Fill { rect: (x + 40.0, y + 33.0, 70.0, 5.0), color: solid(p.text) });
            ops.push(DrawOp::Fill { rect: (x + 20.0, y + 66.0, cw - 40.0, 150.0), color: solid(p.view) });
            ops.push(DrawOp::Fill { rect: (x + 20.0, y + 96.0, cw - 40.0, 26.0), color: solid(p.selection) });
            for (i, line_w) in [160.0, 120.0, 190.0, 140.0, 100.0].iter().enumerate() {
                let line_y = y + 76.0 + i as f64 * 28.0;
                let color = if i == 1 { solid(p.selection_text) } else { (ink.0, ink.1, ink.2, 255) };
                ops.push(DrawOp::Fill { rect: (x + 34.0, line_y + 6.0, *line_w, 6.0), color });
            }
        }
    }
    // The icons of its set in a column to the right of the window.
    let column_x = client.0 + client.2 + (w - client.0 - client.2 - WHOLE_ICON_SIZE) / 2.0;
    let icons: Vec<String> = WHOLE_ICON_NAMES
        .iter()
        .filter_map(|names| whole_icon(&theme.icon_dirs, names, 96))
        .collect();
    let step = h / (icons.len().max(1) as f64);
    for (i, path) in icons.iter().enumerate() {
        let y = step * i as f64 + (step - WHOLE_ICON_SIZE) / 2.0;
        ops.push(DrawOp::Image {
            rect: (column_x.round(), y.round(), WHOLE_ICON_SIZE, WHOLE_ICON_SIZE),
            path: path.clone(),
            cover: false,
        });
    }
    // Its pointer over the window, as if the mouse were there.
    let pointer = theme.cursor_dir.as_deref().and_then(|d| {
        let cursors = std::path::Path::new(d).join("cursors");
        ["left_ptr", "default", "arrow"].iter().find_map(|n| {
            let data = std::fs::read(cursors.join(n)).ok()?;
            xcursor_first_frame(&data, WHOLE_POINTER_NOMINAL)
        })
    });
    let drew_pointer = pointer.is_some();
    if let Some((pw, ph, rgba)) = pointer {
        let at = (client.0 + client.2 * 0.62, client.1 + client.3 * 0.58);
        ops.push(DrawOp::Pixels { x: at.0.round(), y: at.1.round(), width: pw, height: ph, rgba });
    }
    let has_anything = theme.gtk_dir.is_some() || drew_frame || !icons.is_empty() || drew_pointer;
    if !has_anything {
        let mut stub = card_picture(&SampleInput { kind: "", install_path: "", icon_themes: &[] }, glyph);
        stub.what = "";
        return stub;
    }
    CardPicture {
        source: PreviewSource::Sample,
        what: if theme.widgets_shot.is_some() { "theme window with real widgets" } else { "theme window in its colors" },
        width: WHOLE_WIDTH,
        height: WHOLE_HEIGHT,
        base: BASE_BACKGROUND,
        ops,
    }
}

// ── cursor theme sample ───────────────────────────────────────────────────
//
// A cursor theme usually has no picture of its own, and the tile stood with the
// generic mouse glyph — the themes in the grid did not differ from each other
// (acceptance check 10.10.2026). The sample is assembled from the theme's own
// cursors: several main pointers on the placeholder background. The Xcursor
// format per Xcursor(3): the `Xcur` signature, a table of contents, image chunks
// of type 0xfffd0002 with ARGB pixels (premultiplied by alpha).

/// Which pointers to show on the sample: the first found name of each pair (the
/// new name and the old one), no more than six.
const CURSOR_SHEET_NAMES: &[&[&str]] = &[
    &["default", "left_ptr", "arrow"],
    &["pointer", "hand2", "hand1"],
    &["text", "xterm", "ibeam"],
    &["wait", "watch"],
    &["crosshair", "cross"],
    &["move", "fleur", "all-scroll"],
    &["help", "question_arrow"],
    &["not-allowed", "crossed_circle"],
];

/// How many pointers are on the sample and how they stand.
const CURSOR_SHEET_COLUMNS: u32 = 3;
const CURSOR_SHEET_MAX: usize = 6;
/// The pointer size we look for in the file (the nearest of those recorded).
const CURSOR_SHEET_NOMINAL: u32 = 48;

/// The first frame of a pointer from an Xcursor file: width, height and RGBA
/// pixels (already not premultiplied by alpha). The size nearest to `nominal` is
/// taken. `None` — the file is not Xcursor or is truncated.
pub fn xcursor_first_frame(data: &[u8], nominal: u32) -> Option<(u32, u32, Vec<u8>)> {
    const IMAGE: u32 = 0xfffd_0002;
    let u32_at = |at: usize| -> Option<u32> {
        data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if data.get(..4)? != b"Xcur" {
        return None;
    }
    let header = u32_at(4)? as usize;
    let count = u32_at(12)? as usize;
    let mut best: Option<(u32, usize)> = None;
    for i in 0..count.min(4096) {
        let at = header + i * 12;
        if u32_at(at)? != IMAGE {
            continue;
        }
        let (size, pos) = (u32_at(at + 4)?, u32_at(at + 8)? as usize);
        let closer = best.is_none_or(|(b, _)| size.abs_diff(nominal) < b.abs_diff(nominal));
        if closer {
            best = Some((size, pos));
        }
    }
    let (_, pos) = best?;
    let chunk_header = u32_at(pos)? as usize;
    let (width, height) = (u32_at(pos + 16)?, u32_at(pos + 20)?);
    if width == 0 || height == 0 || width > 512 || height > 512 {
        return None;
    }
    let pixels = data.get(pos + chunk_header..pos + chunk_header + (width * height * 4) as usize)?;
    let mut rgba = Vec::with_capacity(pixels.len());
    for p in pixels.chunks_exact(4) {
        let (b, g, r, a) = (p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32);
        let un = |c: u32| if a == 0 { 0 } else { (c * 255 / a).min(255) as u8 };
        rgba.extend_from_slice(&[un(r), un(g), un(b), a as u8]);
    }
    Some((width, height, rgba))
}

/// A cursor theme: its main pointers in a grid. `None` — not a single readable
/// pointer was found in the theme.
fn cursor_sample(input: &SampleInput) -> Option<Vec<DrawOp>> {
    let cursors = std::path::Path::new(input.install_path).join("cursors");
    let mut frames: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    for names in CURSOR_SHEET_NAMES {
        if frames.len() >= CURSOR_SHEET_MAX {
            break;
        }
        let frame = names.iter().find_map(|name| {
            let data = std::fs::read(cursors.join(name)).ok()?;
            xcursor_first_frame(&data, CURSOR_SHEET_NOMINAL)
        });
        if let Some(frame) = frame {
            frames.push(frame);
        }
    }
    if frames.is_empty() {
        return None;
    }
    let (w, h) = (PREVIEW_WIDTH as u32, PREVIEW_HEIGHT as u32);
    let total = frames.len() as u32;
    let rows = total.div_ceil(CURSOR_SHEET_COLUMNS);
    let (cell_w, cell_h) = (w / CURSOR_SHEET_COLUMNS, h / rows);
    Some(
        frames
            .into_iter()
            .enumerate()
            .map(|(i, (fw, fh, rgba))| {
                let (col, row) = (i as u32 % CURSOR_SHEET_COLUMNS, i as u32 / CURSOR_SHEET_COLUMNS);
                // The last incomplete row — centered.
                let in_row = if row == rows - 1 { total - row * CURSOR_SHEET_COLUMNS } else { CURSOR_SHEET_COLUMNS };
                let shift = (w - in_row * cell_w) / 2;
                DrawOp::Pixels {
                    x: ((shift + col * cell_w + cell_w / 2) as i64 - fw as i64 / 2) as f64,
                    y: ((row * cell_h + cell_h / 2) as i64 - fh as i64 / 2) as f64,
                    width: fw,
                    height: fh,
                    rgba,
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod cursor_sheet_tests {
    use super::*;

    /// An Xcursor file with one `size`×`size` pointer of a given ARGB color.
    fn xcursor(size: u32, argb: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"Xcur");
        for v in [16u32, 0x1_0000, 1] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in [0xfffd_0002u32, size, 28] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for v in [36u32, 0xfffd_0002, size, 1, size, size, 0, 0, 0] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for _ in 0..size * size {
            out.extend_from_slice(&[argb[3], argb[2], argb[1], argb[0]]);
        }
        out
    }

    #[test]
    fn xcursor_frame_is_read_and_unpremultiplied() {
        // Semi-transparent red, premultiplied by alpha: A=128, R=128.
        let data = xcursor(4, [128, 128, 0, 0]);
        let (w, h, rgba) = xcursor_first_frame(&data, 48).expect("кадр читается");
        assert_eq!((w, h), (4, 4));
        assert_eq!(&rgba[..4], &[255, 0, 0, 128]);
        assert!(xcursor_first_frame(b"not a cursor", 48).is_none());
        assert!(xcursor_first_frame(&data[..40], 48).is_none(), "обрезанный файл");
    }

    #[test]
    fn sample_is_drawn_from_theme_cursors() {
        let root = std::env::temp_dir().join(format!("rexon-cursor-sheet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("theme/cursors")).unwrap();
        std::fs::write(root.join("theme/cursors/left_ptr"), xcursor(24, [255, 255, 255, 255])).unwrap();
        let theme = root.join("theme").to_string_lossy().into_owned();
        let picture = card_picture(&SampleInput { kind: "cursor", install_path: &theme, icon_themes: &[] }, "🖱️");
        assert_eq!(picture.source, PreviewSource::Sample);
        // The background and the single pointer — centered, white.
        assert_eq!(picture.ops.len(), 2);
        let DrawOp::Pixels { x, y, width, height, rgba } = &picture.ops[1] else {
            panic!("указатель рисуется точками: {:?}", picture.ops[1]);
        };
        assert_eq!((*width, *height), (24, 24));
        assert_eq!((*x, *y), (150.0 - 12.0, 85.0 - 12.0));
        assert_eq!(&rgba[..4], &[255, 255, 255, 255]);
        // A theme without cursors gives no sample — the kind's placeholder remains.
        std::fs::create_dir_all(root.join("empty")).unwrap();
        let empty = root.join("empty").to_string_lossy().into_owned();
        let stub = card_picture(&SampleInput { kind: "cursor", install_path: &empty, icon_themes: &[] }, "🖱️");
        assert_eq!(stub.source, PreviewSource::Placeholder);
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod card_picture_tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Our own check directory.
    fn tmp(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("rexon-card-picture-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn picture(kind: &str, dir: &Path) -> CardPicture {
        let path = dir.to_string_lossy().into_owned();
        card_picture(&SampleInput { kind, install_path: &path, icon_themes: &[] }, "🎨")
    }

    /// Kinds of local themes for which a sample is deliberately not drawn: there is
    /// nothing to take from the theme, or it is separate work. They keep their own
    /// picture, the store entry and the kind's placeholder.
    const NO_SAMPLE: &[(&str, &str)] = &[
        ("global", "снимок несёт сама тема"),
        ("splash", "снимок несёт сама тема"),
        ("lockscreen", "снимок несёт сама тема"),
        ("sounds", "на плитке ряд «Прослушать», картинки нет"),
        ("widgetstyle", "стиль — движок Qt, на диске у него ничего нет"),
        ("lxqttheme", "снимок несёт сама тема"),
        ("conky", "картинку виджета несёт только его набор"),
    ];

    /// Every kind of local theme either has a row in the sample table or is named in
    /// the "no sample" list with a reason. A new kind without a decision fails this
    /// check — it cannot silently stay with a placeholder.
    #[test]
    fn every_local_kind_is_decided() {
        let mut kinds: Vec<&str> = crate::installed_scan::types_for("/h", "/usr")
            .iter()
            .map(|t| t.apply_kind)
            .collect();
        kinds.extend(crate::categories::DE_SECTIONS.iter().map(|(_, k)| *k));
        kinds.extend(["kvantum", "splash", "lockscreen", "widgetstyle"]);
        for kind in kinds {
            let sampled = PREVIEW_RULES.iter().any(|r| r.kind == kind);
            let waived = NO_SAMPLE.iter().any(|(k, _)| *k == kind);
            assert!(sampled != waived, "вид «{kind}»: строка образца {sampled}, в списке «без образца» {waived}");
        }
        for (i, r) in PREVIEW_RULES.iter().enumerate() {
            assert!(!PREVIEW_RULES[..i].iter().any(|o| o.kind == r.kind), "вид «{}» назван дважды", r.kind);
        }
    }

    /// The placeholder is the kind glyph on the background and nothing else: there
    /// is no theme name on it (it stands under the picture), and it looks the same
    /// for any theme kind.
    #[test]
    fn placeholder_is_glyph_without_name() {
        let root = tmp("stub");
        for kind in ["widgetstyle", "kwineffects", "gtk", "нет-такого"] {
            let got = picture(kind, &root.join("missing"));
            assert_eq!(got.source, PreviewSource::Placeholder, "{kind}");
            assert_eq!(got.ops.len(), 2, "{kind}: подложка и знак");
            assert!(matches!(got.ops[0], DrawOp::Gradient { .. }));
            let DrawOp::Text { text, .. } = &got.ops[1] else { panic!("знак вида") };
            assert_eq!(text.as_str(), "🎨");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn file_source_tells_own_from_store() {
        let dir = "/usr/share/sddm/themes/breeze";
        assert_eq!(file_source("/usr/share/sddm/themes/breeze/preview.png", dir), PreviewSource::Own);
        assert_eq!(file_source("/usr/share/sddm/themes/breeze/.rexon-preview.png", dir), PreviewSource::Store);
        assert_eq!(file_source("/home/u/.cache/rexon/ocs_previews/ab.png", dir), PreviewSource::Store);
        assert_eq!(file_source("/usr/share/sddm/themes/breeze-2/preview.png", dir), PreviewSource::Store);
    }

    #[test]
    fn colors_sample_is_a_window_in_scheme_colors() {
        let root = tmp("colors");
        put(
            &root.join("Mine.colors"),
            "[Colors:Window]\nBackgroundNormal=10,20,30\nForegroundNormal=200,210,220\n\
             [Colors:Selection]\nBackgroundNormal=1,2,3\n[WM]\nactiveBackground=40,50,60\n",
        );
        // The scan gives the scheme path without the extension.
        let got = picture("colors", &root.join("Mine"));
        assert_eq!(got.source, PreviewSource::Sample);
        // The background, then the window background over the whole frame, the title
        // bar, the selection.
        assert_eq!(got.ops[1], DrawOp::Fill { rect: (0.0, 0.0, 300.0, 170.0), color: (10, 20, 30, 255) });
        assert_eq!(got.ops[2], DrawOp::Fill { rect: (0.0, 0.0, 300.0, 26.0), color: (40, 50, 60, 255) });
        assert!(got.ops.iter().any(|op| matches!(op, DrawOp::Fill { color: (1, 2, 3, 255), .. })));
        assert!(!got.ops.iter().any(|op| matches!(op, DrawOp::Text { .. })), "названия на картинке нет");
        // A file without color groups — a placeholder, not a window in Breeze colors.
        put(&root.join("Empty.colors"), "[General]\nName=Empty\n");
        assert_eq!(picture("colors", &root.join("Empty")).source, PreviewSource::Placeholder);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn palettes_are_read_from_theme_files() {
        let kde = kde_palette(
            "[Colors:Selection]\nBackgroundNormal=61,174,233\n[Colors:Window]\nBackgroundNormal=239,240,241\nForegroundNormal=35,38,41\n[WM]\nactiveBackground=71,80,87\n",
        )
        .unwrap();
        assert_eq!((kde.window, kde.text, kde.selection, kde.titlebar), ((239, 240, 241), (35, 38, 41), (61, 174, 233), (71, 80, 87)));
        assert_eq!(kde.view, kde.window, "чего в файле нет — берётся от окна");
        assert!(kde_palette("[Colors:View]\nBackgroundNormal=1,2,3\n").is_none());

        let kv = kvantum_palette("[GeneralColors]\nwindow.color=#fafafa\nbase.color=#ffffff\nhighlight.color=#4285f420\ntext.color=#444444\n").unwrap();
        assert_eq!((kv.window, kv.view, kv.selection, kv.text), ((250, 250, 250), (255, 255, 255), (66, 133, 244), (68, 68, 68)));

        // Names with the theme's suffix, a reference to another color and a function
        // over a reference.
        let gtk = gtk_css_palette(
            "@define-color theme_bg_color_breeze #eff0f1;\n@define-color theme_fg_color_breeze rgb(35, 38, 41);\n\
             @define-color theme_selected_bg_color_breeze @accent;\n@define-color accent #3daee9;\n\
             @define-color theme_base_color_breeze shade(@theme_bg_color_breeze, 1.1);\n@define-color theme_selected_fg_color_breeze white;\n",
        )
        .unwrap();
        assert_eq!((gtk.window, gtk.text, gtk.selection, gtk.selection_text), ((239, 240, 241), (35, 38, 41), (61, 174, 233), (255, 255, 255)));
        assert_eq!(gtk.view, (239, 240, 241));
        assert!(gtk_css_palette("button { color: red; }").is_none());

        let gtk2 = gtk2_palette("gtk-color-scheme = \"base_color:#ffffff\\nfg_color:#101010\\nselected_bg_color:#86abd9\\nbg_color:#edeceb\"\n").unwrap();
        assert_eq!((gtk2.window, gtk2.text, gtk2.selection), ((237, 236, 235), (16, 16, 16), (134, 171, 217)));
    }

    #[test]
    fn themed_window_samples_come_from_theme_colors() {
        let root = tmp("window");
        put(&root.join("KvX/KvX.kvconfig"), "[GeneralColors]\nwindow.color=#101010\ntext.color=#f0f0f0\n");
        put(&root.join("Gtk/gtk-3.0/gtk.css"), "@import url(\"colors.css\");\n");
        put(&root.join("Gtk/gtk-3.0/colors.css"), "@define-color theme_bg_color #202020;\n@define-color theme_fg_color #e0e0e0;\n");
        put(&root.join("Old/gtk-2.0/gtkrc"), "gtk-color-scheme = \"bg_color:#ededed\\nfg_color:#1a1a1a\"\n");
        put(&root.join("Binary/gtk-3.0/gtk.css"), "@import url(\"resource:///org/x/gtk.css\");\n");
        put(&root.join("Colored/colors"), "[Colors:Window]\nBackgroundNormal=1,2,3\nForegroundNormal=250,250,250\n");
        for (kind, dir, window) in [
            ("kvantum", "KvX", (16, 16, 16, 255)),
            ("gtk", "Gtk", (32, 32, 32, 255)),
            ("gtk", "Old", (237, 237, 237, 255)),
            ("plasma", "Colored", (1, 2, 3, 255)),
        ] {
            let got = picture(kind, &root.join(dir));
            assert_eq!(got.source, PreviewSource::Sample, "{dir}");
            assert!(
                got.ops.iter().any(|op| matches!(op, DrawOp::Fill { color, .. } if *color == window)),
                "{dir}: окно залито цветом темы"
            );
        }
        // A theme compiled into a binary file names no colors — a placeholder.
        assert_eq!(picture("gtk", &root.join("Binary")).source, PreviewSource::Placeholder);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn icon_sample_takes_the_themes_own_icons() {
        let root = tmp("icons");
        let theme = root.join("Mine");
        put(
            &theme.join("index.theme"),
            "[Icon Theme]\nName=Mine\nDirectories=places/16,places/64,places/64@2x,apps/48\n\n\
             [places/16]\nSize=16\n\n[places/64]\nSize=64\n\n[places/64@2x]\nSize=64\nScale=2\n\n[apps/48]\nSize=48\n",
        );
        for file in ["places/16/folder.svg", "places/64/folder.svg", "places/64@2x/folder.svg", "places/64/user-home.png", "apps/48/zeta.png", "apps/48/alpha.svg"] {
            put(&theme.join(file), "");
        }
        let got = picture("icons", &theme);
        assert_eq!(got.source, PreviewSource::Sample);
        let files: Vec<String> = got
            .ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Image { path, cover: false, .. } => Some(path.strip_prefix(&format!("{}/", theme.display())).unwrap().to_string()),
                _ => None,
            })
            .collect();
        // The usual names — from the directory of the nearest size, the rest is
        // filled up with the theme's files in order; double scale is not taken.
        assert_eq!(files, ["places/64/folder.svg", "places/64/user-home.png", "apps/48/alpha.svg", "apps/48/zeta.png"]);
        // A theme without icons (only links to other themes) gives no sample.
        put(&root.join("Empty/index.theme"), "[Icon Theme]\nName=Empty\nInherits=Mine\n");
        assert_eq!(picture("icons", &root.join("Empty")).source, PreviewSource::Placeholder);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plasma_and_aurorae_samples_use_theme_drawings() {
        let root = tmp("svg");
        let plasma = root.join("Mine");
        for file in ["dialogs/background.svgz", "widgets/panel-background.svg", "widgets/button.svgz"] {
            put(&plasma.join(file), "");
        }
        let got = picture("plasma", &plasma);
        assert_eq!(got.source, PreviewSource::Sample);
        let frames: Vec<(&str, String)> = got
            .ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::SvgFrame { path, prefixes, .. } => Some((path.rsplit('/').next().unwrap(), prefixes.join("|"))),
                _ => None,
            })
            .collect();
        let frames: Vec<(&str, &str)> = frames.iter().map(|(f, p)| (*f, p.as_str())).collect();
        assert_eq!(frames, [("background.svgz", ""), ("button.svgz", "normal"), ("panel-background.svg", "south|")]);
        assert_eq!(picture("plasma", &root.join("Nothing")).source, PreviewSource::Placeholder);

        let aurorae = root.join("Frame");
        put(&aurorae.join("decoration.svg"), "");
        put(&aurorae.join("close.svg"), "");
        put(&aurorae.join("minimize.svg"), "");
        put(&aurorae.join("Framerc"), "[Layout]\nButtonWidth=20\nButtonHeight=18\nButtonSpacing=2\nTitleEdgeTop=3\nTitleEdgeRight=6\nPaddingRight=10\nPaddingTop=8\n");
        let got = picture("aurorae", &aurorae);
        assert_eq!(got.source, PreviewSource::Sample);
        assert!(matches!(&got.ops[1], DrawOp::SvgFrame { prefixes, .. } if prefixes == &["decoration"]));
        // Buttons right to left from the frame's right edge: the margin, the
        // title bar edge, the button width and the gap — from the theme settings.
        let buttons: Vec<(Rect, &str)> = got
            .ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::SvgElement { rect, path, element } if element == "active-center" => {
                    Some((*rect, path.rsplit('/').next().unwrap()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(buttons, [((234.0, 31.0, 20.0, 18.0), "close.svg"), ((212.0, 31.0, 20.0, 18.0), "minimize.svg")]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plymouth_sample_shows_the_final_animation_frame() {
        let root = tmp("plymouth");
        let theme = root.join("boot");
        put(
            &theme.join("boot.plymouth"),
            "[Plymouth Theme]\nName=boot\nModuleName=two-step\n\n[two-step]\nBackgroundStartColor=0x102030\nBackgroundEndColor=0x000000\n",
        );
        for n in 0..12 {
            put(&theme.join(format!("animation-{n:02}.png")), "");
        }
        for n in 0..3 {
            put(&theme.join(format!("throbber-{n:02}.png")), "");
        }
        let got = picture("plymouth", &theme);
        assert_eq!(got.source, PreviewSource::Sample);
        assert_eq!(got.ops[1], DrawOp::Fill { rect: (0.0, 0.0, 300.0, 85.0), color: (16, 32, 48, 255) });
        let DrawOp::Image { path, cover, .. } = got.ops.last().unwrap() else { panic!("кадр заставки") };
        assert!(path.ends_with("/animation-11.png") && !cover, "{path}");
        // The theme's emblem is more important than an animation frame.
        put(&theme.join("watermark.png"), "");
        let DrawOp::Image { path, .. } = picture("plymouth", &theme).ops.last().unwrap().clone() else { panic!() };
        assert!(path.ends_with("/watermark.png"), "{path}");
        // A theme without pictures (a text one) gives no sample.
        put(&root.join("text/text.plymouth"), "[Plymouth Theme]\nModuleName=text\n");
        assert_eq!(picture("plymouth", &root.join("text")).source, PreviewSource::Placeholder);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A bitmap XPM image of a given size (one color).
    fn xpm(width: u32, height: u32) -> String {
        let row = format!("\"{}\",\n", ".".repeat(width as usize));
        format!("/* XPM */\nstatic char * part_xpm[] = {{\n\"{width} {height} 1 1\",\n\".\tc #C0C0FF s active_color_1\",\n{}}};\n", row.repeat(height as usize))
    }

    #[test]
    fn image_size_is_read_from_file_headers() {
        let root = tmp("size");
        put(&root.join("a.xpm"), &xpm(24, 19));
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec();
        png.extend_from_slice(&[0, 0, 0, 7, 0, 0, 0, 30, 8, 6, 0, 0, 0]);
        std::fs::write(root.join("b.png"), png).unwrap();
        put(&root.join("c.svg"), "<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"2\" height=\"32px\" viewBox=\"0 0 2 32\">");
        put(&root.join("d.svg"), "<svg viewBox='0 0 16 12' stroke-width=\"3\">");
        put(&root.join("e.txt"), "не картинка");
        assert_eq!(image_size(&root.join("a.xpm")), Some((24.0, 19.0)));
        assert_eq!(image_size(&root.join("b.png")), Some((7.0, 30.0)));
        assert_eq!(image_size(&root.join("c.svg")), Some((2.0, 32.0)));
        assert_eq!(image_size(&root.join("d.svg")), Some((16.0, 12.0)), "размер из viewBox; stroke-width — не width");
        assert_eq!(image_size(&root.join("e.txt")), None);
        assert_eq!(image_size(&root.join("missing.png")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn xfwm4_frame_is_built_from_theme_parts() {
        let root = tmp("xfwm4");
        let dir = root.join("Frames/xfwm4");
        for (name, w, h) in [
            ("top-left", 6, 20), ("top-right", 6, 20), ("title-1", 2, 20), ("title-3", 4, 20), ("title-5", 2, 20),
            ("left", 3, 8), ("right", 3, 8), ("bottom", 8, 4), ("bottom-left", 6, 4), ("bottom-right", 6, 4),
            ("close", 16, 16), ("hide", 16, 16), ("menu", 16, 16),
        ] {
            put(&dir.join(format!("{name}-active.xpm")), &xpm(w, h));
        }
        // An overlay on top of the bitmap, and states that are not taken.
        put(&dir.join("close-active.png"), "");
        put(&dir.join("close-inactive.xpm"), &xpm(16, 16));
        put(&dir.join("themerc"), "# рамка\nbutton_layout=O|HC\nbutton_offset=2\nbutton_spacing=1\nactive_text_color=#ffffff\n");
        let got = picture("xfwm4", &root.join("Frames"));
        assert_eq!(got.source, PreviewSource::Sample);
        let file = |path: &str| path.rsplit('/').next().unwrap().to_string();
        let placed: Vec<(String, Rect)> = got
            .ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Image { path, rect, cover: false } => Some((file(path), *rect)),
                _ => None,
            })
            .collect();
        // Corners — at their own size at the frame's corners (30, 16, 240×138).
        assert!(placed.contains(&("top-left-active.xpm".into(), (30.0, 16.0, 6.0, 20.0))), "{placed:?}");
        assert!(placed.contains(&("top-right-active.xpm".into(), (264.0, 16.0, 6.0, 20.0))), "{placed:?}");
        assert!(placed.contains(&("bottom-right-active.xpm".into(), (264.0, 150.0, 6.0, 4.0))), "{placed:?}");
        // Buttons: the menu on the left after the side and the padding; on the
        // right — "close" at the edge, "minimize" to its left; vertically — in
        // the middle of the title bar.
        assert!(placed.contains(&("menu-active.xpm".into(), (35.0, 18.0, 16.0, 16.0))), "{placed:?}");
        assert!(placed.contains(&("close-active.xpm".into(), (249.0, 18.0, 16.0, 16.0))), "{placed:?}");
        assert!(placed.contains(&("close-active.png".into(), (249.0, 18.0, 16.0, 16.0))), "накладка поверх: {placed:?}");
        assert!(placed.contains(&("hide-active.xpm".into(), (232.0, 18.0, 16.0, 16.0))), "{placed:?}");
        assert!(!placed.iter().any(|(f, _)| f.contains("inactive")));
        // Sides and the title bar — as tiles over the whole length.
        let tiled: Vec<(String, Rect)> = got
            .ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::Tiled { path, rect } => Some((file(path), *rect)),
                _ => None,
            })
            .collect();
        assert!(tiled.contains(&("left-active.xpm".into(), (30.0, 36.0, 3.0, 114.0))), "{tiled:?}");
        assert!(tiled.contains(&("bottom-active.xpm".into(), (36.0, 150.0, 228.0, 4.0))), "{tiled:?}");
        assert!(tiled.iter().any(|(f, r)| f == "title-3-active.xpm" && r.1 == 16.0 && r.3 == 20.0), "{tiled:?}");
        assert!(tiled.iter().any(|(f, r)| f == "title-1-active.xpm" && r.0 == 36.0), "{tiled:?}");
        // A directory without title bar images gives no frame.
        put(&root.join("Empty/xfwm4/themerc"), "button_layout=O|HC\n");
        assert_eq!(picture("xfwm4", &root.join("Empty")).source, PreviewSource::Placeholder);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn shell_samples_take_panel_and_menu_colors_from_the_stylesheet() {
        let css = "/* тема */\nstage { color: #eeeeee; }\n\
                   #panel {\n  height: 28px;\n  background-color: rgba(26, 27, 35, 0.95);\n}\n\
                   #panel .panel-button { color: rgba(255, 255, 255, 0.6); }\n\
                   .popup-menu-content, .candidate-popup-content {\n  background-color: #fff;\n  color: rgba(0, 0, 0, 0.87);\n}\n\
                   .popup-menu .popup-menu-item:active { background-color: transparent; }\n\
                   .popup-menu-item:selected { background-color: #3daee9; }\n";
        let p = shell_palette(css).unwrap();
        assert_eq!(p.panel, (26, 27, 35, 242));
        assert_eq!(p.panel_text, (255, 255, 255), "правило потомка панели");
        assert_eq!((p.menu, p.menu_text), ((255, 255, 255, 255), (0, 0, 0)));
        assert_eq!(p.selected, (61, 174, 233, 255), "прозрачный фон выбранной строки пропущен");
        // A stylesheet with only a panel: the menu takes its color, the selected
        // row — a mix of the background with the text, and text indistinguishable
        // on its background is replaced.
        let bare = shell_palette("#panel { background-color: #202020; color: #212121; }").unwrap();
        assert_eq!((bare.menu, bare.panel_text), ((32, 32, 32, 255), (255, 255, 255)));
        assert_ne!(bare.selected, bare.menu);
        assert!(shell_palette(".button { color: red; }").is_none());

        let root = tmp("shell");
        put(&root.join("G/gnome-shell/gnome-shell.css"), css);
        put(&root.join("C/cinnamon/cinnamon.css"), css);
        put(&root.join("Bare/gnome-shell/gnome-shell.css"), "@import url(\"resource:///org/gnome/shell/theme/gnome-shell.css\");\n");
        let panel_rect = |kind: &str, dir: &str| {
            let got = picture(kind, &root.join(dir));
            assert_eq!(got.source, PreviewSource::Sample, "{dir}");
            match got.ops[1] {
                DrawOp::Fill { rect, color } => (rect, color),
                ref other => panic!("панель: {other:?}"),
            }
        };
        // GNOME — the panel on top, Cinnamon — at the bottom; the color comes from the
        // stylesheet.
        assert_eq!(panel_rect("gnomeshell", "G"), ((0.0, 0.0, 300.0, 24.0), (26, 27, 35, 242)));
        assert_eq!(panel_rect("cinnamontheme", "C"), ((0.0, 146.0, 300.0, 24.0), (26, 27, 35, 242)));
        assert_eq!(picture("gnomeshell", &root.join("Bare")).source, PreviewSource::Placeholder);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nemo_action_shows_the_icon_it_names() {
        let root = tmp("nemo");
        let theme = root.join("icons/hicolor");
        put(&theme.join("index.theme"), "[Icon Theme]\nDirectories=scalable/actions,48x48/places\n\n[scalable/actions]\nSize=64\n\n[48x48/places]\nSize=48\n");
        put(&theme.join("scalable/actions/xsi-list-add-symbolic.svg"), "");
        put(&theme.join("48x48/places/folder.png"), "");
        put(&root.join("add.nemo_action"), "[Nemo Action]\nName=Add\nIcon-Name=xsi-list-add-symbolic\nExec=x\n");
        put(&root.join("dir.nemo_action"), "[Nemo Action]\nName=Dir\nIcon-Name=folder\n");
        put(&root.join("bare.nemo_action"), "[Nemo Action]\nName=Bare\nExec=x\n");
        let themes = vec![theme.to_string_lossy().into_owned()];
        let of = |file: &str| {
            let path = root.join(file).to_string_lossy().into_owned();
            card_picture(&SampleInput { kind: "nemoactions", install_path: &path, icon_themes: &themes }, "⚡")
        };
        // A monochrome icon — on a light plate, a colored one — without it.
        let symbolic = of("add.nemo_action");
        assert_eq!(symbolic.ops.len(), 3);
        assert!(matches!(symbolic.ops[1], DrawOp::Fill { color: (236, 236, 236, 255), .. }));
        assert!(matches!(&symbolic.ops[2], DrawOp::Image { path, .. } if path.ends_with("/xsi-list-add-symbolic.svg")));
        let colored = of("dir.nemo_action");
        assert_eq!(colored.ops.len(), 2);
        assert!(matches!(&colored.ops[1], DrawOp::Image { path, .. } if path.ends_with("/48x48/places/folder.png")));
        assert_eq!(of("bare.nemo_action").source, PreviewSource::Placeholder, "значок не назван");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn package_icon_and_sddm_background() {
        let root = tmp("package");
        put(&root.join("applet@x/icon.png"), "");
        let got = picture("cinnamonapplets", &root.join("applet@x"));
        assert!(matches!(got.ops.last(), Some(DrawOp::Image { path, .. }) if path.ends_with("/applet@x/icon.png")));

        // An icon named by the metadata is looked for in icon themes by name.
        let theme = root.join("icons/breeze");
        put(&theme.join("index.theme"), "[Icon Theme]\nDirectories=applets/64\n\n[applets/64]\nSize=64\n");
        put(&theme.join("applets/64/effect-x.svg"), "");
        put(&root.join("fx/metadata.json"), r#"{"KPlugin": {"Icon": "effect-x", "Id": "fx"}}"#);
        put(&root.join("nofx/metadata.json"), r#"{"KPlugin": {"Icon": "effect-missing"}}"#);
        let themes = vec![theme.to_string_lossy().into_owned()];
        let with_themes = |dir: &str| {
            let path = root.join(dir).to_string_lossy().into_owned();
            card_picture(&SampleInput { kind: "kwineffects", install_path: &path, icon_themes: &themes }, "✨")
        };
        assert!(matches!(with_themes("fx").ops.last(), Some(DrawOp::Image { path, .. }) if path.ends_with("/applets/64/effect-x.svg")));
        assert_eq!(with_themes("nofx").source, PreviewSource::Placeholder, "значка нет ни в одной теме");

        put(&root.join("login/theme.conf"), "[General]\nbackground=bg.jpg\n");
        assert_eq!(picture("sddm", &root.join("login")).source, PreviewSource::Placeholder, "файла фона нет");
        put(&root.join("login/bg.jpg"), "");
        assert!(matches!(picture("sddm", &root.join("login")).ops.last(), Some(DrawOp::Image { cover: true, .. })));
        let _ = std::fs::remove_dir_all(&root);
    }
}
