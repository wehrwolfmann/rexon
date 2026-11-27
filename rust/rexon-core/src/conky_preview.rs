//! A Conky widget snapshot for the tile — when the widget has no picture of its own.
//!
//! WHY. Half of the widgets come without a picture, and the tile in the section is
//! a 📊 placeholder: the person does not see what they have until they start it. Rexon
//! takes a snapshot of the widget itself and puts it into the cache
//! (`~/.cache/rexon/conky-previews/<file key>-<fingerprint>.png`, next to it
//! `<key>.json` with the fingerprint; a snapshot of the live window is `<key>.png`); the tile takes it if the widget has no picture of its own
//! ([`crate::conky_scan::decorate_item`]).
//!
//! ## The invisible screen (the main path — both Wayland and X11)
//!
//! A copy of the widget is started on its own invisible Xvfb X server (Wehrwolfmann's
//! decision of 25.10.2026): nothing flickers on the real desktop, there is a snapshot
//! before the first launch, and a Wayland session does not get in the way. The person's
//! current wallpaper is set as the screen root — a transparent widget on the tile lies on
//! it, not on black. How it works ([`Stand`]):
//! * Xvfb `-displayfd` (the server picks the number itself), `-auth` with its own
//!   cookie, `-noreset` (otherwise when the last client leaves the server
//!   resets and the wallpaper vanishes), `-nolisten tcp`; the size is the primary
//!   monitor, `-dpi` — as the person's;
//! * Rexon takes the `_NET_WM_CM_S0` selection ("there is a compositor"): conky ≥ 1.25
//!   then takes a 32-bit window with alpha, and Rexon itself lays it onto the wallpaper
//!   (premultiplied blending `out = src + dst·(255−a)/255`, as a
//!   compositor does). Transparent, semi-transparent, opaque and Lua/cairo
//!   come out right; the root pixmap and `_XROOTPMAP_ID` are set anyway —
//!   for old conky with pseudo-transparency and `own_window = false`;
//! * the copy runs with the preview overlay ([`preview_overlay`]): `background =
//!   false` (otherwise conky goes to the background with a new pid and is not stopped),
//!   output to X11 only (`output_backend = 'x11'`, for old ones
//!   `out_to_x`/`out_to_wayland`) and the environment `XDG_SESSION_TYPE=x11` without
//!   `WAYLAND_DISPLAY`: in a Wayland session conky 1.25 would otherwise choose
//!   Wayland and land on the REAL desktop;
//! * its own process group, `PR_SET_PDEATHSIG` (Rexon died — they died too),
//!   stopped with `killpg` TERM → KILL; copies that escaped the group — by pid
//!   from `/proc`. Preview processes are seen neither by "On the desktop now" nor by
//!   "Stop all" ([`is_preview_launch`]).
//!
//! Only what can be started without a question is snapshotted ([`check_widget`]):
//! the widget only draws or is already allowed in its current form, and it lacks
//! nothing. No password is asked while browsing the catalog, no packages are
//! installed. If the test bench did not come up, the tile stays a placeholder. No Xvfb —
//! one dismissable hint above the section with this distribution's package
//! and an "Install" button ([`xvfb_hint`]); the package is installed only by the button.
//!
//! Xvfb is a mandatory dependency of the package: Arch and its descendants —
//! `xorg-server-xvfb`, Debian/Ubuntu — `xvfb`, Fedora/openSUSE —
//! `xorg-x11-server-Xvfb` (`utils::TOOL_PACKAGES`). On SteamOS (`/usr`
//! read-only) the same Arch package ships inside the Rexon sysext. Rexon does not embed
//! its own copy of an X server.
//!
//! ## The live window (the X11 fallback without Xvfb)
//!
//! Without Xvfb on X11 the snapshot is taken at the widget's first launch: the window — from
//! the conky log (`drawing to created window 0x…`), the snapshot — `XGetImage`
//! through libX11, which the core loads at call time (`x11-dl`).

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::animations::LookIo;
use crate::de_apply::Ctx;

/// The Rexon cache subdirectory with snapshots.
pub const CACHE_SUBDIR: &str = "conky-previews";

/// How long to wait for the widget to draw: conky draws on the first
/// update step, for some widgets the step is seconds.
pub const WAIT_MS: u64 = 6000;
const STEP_MS: u64 = 400;

/// The snapshot format version (`<key>.json`): if it changed, everything is retaken.
pub const META_VERSION: u32 = 1;

/// Part of the test bench files path: `$XDG_RUNTIME_DIR/rexon/conky-preview/`.
pub const PREVIEW_SEGMENT: &str = "/rexon/conky-preview/";
/// The preview overlay name ending.
pub const PREVIEW_SUFFIX: &str = ".preview.conf";

/// Margins around the widget on the tile, screen pixels.
pub const CARD_MARGIN: u32 = 16;
/// The tile's long side is no larger than this.
pub const CARD_MAX: u32 = 640;
/// The test bench shuts down after this many seconds without work.
pub const STAND_IDLE: Duration = Duration::from_secs(10);
/// The backdrop color when there is no wallpaper picture (live wallpapers, not found out).
pub const FILL_RGB: [u8; 3] = [0x1e, 0x20, 0x26];

/// The snapshot of the settings file `conf` in the cache.
pub fn cache_path(ctx: &Ctx, conf: &str) -> String {
    format!(
        "{}/{CACHE_SUBDIR}/{}.png",
        crate::appearance_cache::cache_dir(&ctx.home),
        crate::conky_overlay::key(conf)
    )
}

/// The snapshot's fingerprint next to it.
pub fn meta_path(ctx: &Ctx, conf: &str) -> String {
    let png = cache_path(ctx, conf);
    format!("{}.json", png.trim_end_matches(".png"))
}

/// An invisible screen snapshot with the fingerprint `meta`: `<key>-<8 characters
/// of the fingerprint>.png`. Each retaken snapshot gets a new name — otherwise a window
/// that remembered the picture by address would show the previous one.
pub fn shot_path(ctx: &Ctx, conf: &str, meta: &Meta) -> String {
    let json = serde_json::to_string(meta).unwrap_or_default();
    let h = <sha2::Sha256 as sha2::Digest>::digest(json.as_bytes());
    let tag: String = h.iter().take(4).map(|b| format!("{b:02x}")).collect();
    let png = cache_path(ctx, conf);
    format!("{}-{tag}.png", png.trim_end_matches(".png"))
}

/// The snapshot of the file `conf` from the cache: of the invisible screen (by the fingerprint next to it),
/// otherwise of the live window.
pub fn cached_one(ctx: &Ctx, conf: &str) -> Option<String> {
    let meta: Option<Meta> = std::fs::read_to_string(meta_path(ctx, conf)).ok().and_then(|t| serde_json::from_str(&t).ok());
    meta.map(|m| shot_path(ctx, conf, &m))
        .filter(|p| Path::new(p).is_file())
        .or_else(|| Some(cache_path(ctx, conf)).filter(|p| Path::new(p).is_file()))
}

/// The first snapshot from the cache for the files `confs` (the main one first).
pub fn cached(ctx: &Ctx, confs: &[String]) -> Option<String> {
    confs.iter().find_map(|c| cached_one(ctx, c))
}

/// The conky window id from its log — the last "drawing to created window".
pub fn window_of_log(text: &str) -> Option<u64> {
    text.lines().rev().find_map(|l| {
        let rest = l.split("drawing to created window").nth(1)?;
        let hex = rest.trim().trim_start_matches("0x");
        let hex: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        u64::from_str_radix(&hex, 16).ok().filter(|w| *w != 0)
    })
}

/// Whether conky draws right on the screen root (`own_window = false`).
pub fn draws_on_desktop(log: &str) -> bool {
    log.contains("drawing to desktop window")
}

/// Whether the LIVE window can be snapshotted in this session: X11 (there is `DISPLAY`, no
/// Wayland).
pub fn can_capture(ctx: &Ctx, display: Option<&str>) -> bool {
    !ctx.wayland && display.is_some_and(|d| !d.is_empty())
}

// ════════════════════════════════════════════════════════════════════════════
//  Pixels
// ════════════════════════════════════════════════════════════════════════════

/// A pixel of a 32-bit conky window (BGRA, the color premultiplied by alpha) to
/// straight RGBA for a PNG with transparency. conky draws outlines and bars also
/// with "color greater than alpha" pixels — a compositor ADDS them to the background, and they
/// are visible; the straight color of such a pixel cannot be expressed, so the alpha
/// is raised to the brightest channel: `a' = max(a, r, g, b)`.
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

/// 32-bit ZPixmap pixels (BGRA, least significant byte first) → RGBA. For a window with
/// depth 32 the color is premultiplied — for the PNG it is converted back to straight
/// ([`straight_rgba`]); depth 24 — opaque.
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

/// A window pixel (BGRA) over the backdrop `under` (RGB) — as a compositor does:
/// at depth 32 the source is premultiplied, `out = src + dst·(255−a)/255`;
/// at depth 24 the window is opaque.
pub fn over(p: &[u8], depth: u32, under: [u8; 3]) -> [u8; 3] {
    if depth != 32 {
        return [p[2], p[1], p[0]];
    }
    let a = p[3] as u32;
    let mix = |s: u8, d: u8| -> u8 { (s as u32 + (d as u32 * (255 - a) + 127) / 255).min(255) as u8 };
    [mix(p[2], under[0]), mix(p[1], under[1]), mix(p[0], under[2])]
}

/// Whether anything is drawn: not all pixels are the same (4 bytes per pixel).
pub fn has_content(px: &[u8]) -> bool {
    let first = px.get(..4);
    px.chunks_exact(4).any(|p| Some(p) != first)
}

/// The bounding box of how `now` (RGB) differs from `base` (RGB) of the same size:
/// `(x, y, width, height)`. `None` — it does not.
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

/// Write RGBA to a PNG.
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
//  The backdrop (wallpaper) and the tile
// ════════════════════════════════════════════════════════════════════════════

/// The test bench screen wallpaper: full-screen RGB and a tag for the fingerprint.
#[derive(Debug, Clone, PartialEq)]
pub struct Backdrop {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    /// `path|mtime|size` of the picture or `fill` — a flat fill.
    pub tag: String,
}

/// The wallpaper picture tag for the fingerprint: path, modification time and size.
/// `fill` — there is no picture.
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
    /// A flat dark fill.
    pub fn fill(width: u32, height: u32) -> Self {
        let rgb = FILL_RGB.iter().copied().cycle().take((width * height * 3) as usize).collect();
        Backdrop { width, height, rgb, tag: "fill".to_string() }
    }

    /// The picture `path` filling the screen (scaled by the larger side,
    /// cropped at the center — like desktops' "Fill"). If it does not open —
    /// a fill.
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

/// The tile: a piece of the screen `piece` (RGB, `w×h` at the point `x, y`) on the wallpaper with
/// [`CARD_MARGIN`] margins, the long side no larger than [`CARD_MAX`]. RGBA.
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
//  The preview overlay, the wait, the cookie
// ════════════════════════════════════════════════════════════════════════════

/// The test bench directory: `$XDG_RUNTIME_DIR/rexon/conky-preview` (for a session without
/// it — in the temporary directory under the user's number).
pub fn runtime_dir() -> String {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(r) if !r.is_empty() => format!("{}{}", r.trim_end_matches('/'), PREVIEW_SEGMENT.trim_end_matches('/')),
        _ => {
            // SAFETY: getuid has no side effects.
            let uid = unsafe { libc::getuid() };
            format!(
                "{}/rexon-{uid}{}",
                std::env::temp_dir().to_string_lossy().trim_end_matches('/'),
                PREVIEW_SEGMENT.trim_end_matches('/')
            )
        }
    }
}

/// Whether conky is started with the preview overlay — a copy on the invisible screen, not
/// a widget on the desktop.
pub fn is_preview_launch(launch: &str) -> bool {
    launch.contains(PREVIEW_SEGMENT) && launch.ends_with(PREVIEW_SUFFIX)
}

/// The preview overlay text: execute `source` (the author's file, its conversion from
/// 1.9 or the overlay with the person's edits) and hold the copy on the invisible
/// X11 screen, in the foreground.
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
        // The version was not found out — everything at once: conky will merely note an unfamiliar field.
        None => out.push_str("c.output_backend = 'x11'\nc.out_to_x = true\nc.out_to_wayland = false\n"),
    }
    out
}

/// How long to wait after the window appears for the widget to finish drawing: two update
/// steps, but from half a second to three seconds.
pub fn settle_ms(update_interval: Option<f64>) -> u64 {
    let ui = update_interval.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0);
    ((ui * 2000.0) as u64).clamp(500, 3000)
}

/// A FamilyWild `.Xauthority` record with a MIT-MAGIC-COOKIE-1 cookie. The display
/// number `display_num` is empty before the start (`-displayfd`: the server picks the number,
/// and it treats an empty number as any); libX11 clients need the number —
/// after the start the file is rewritten with it.
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
//  Fingerprint and admission
// ════════════════════════════════════════════════════════════════════════════

/// The snapshot fingerprint (`<key>.json`): if it matches entirely, the snapshot is fresh.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Meta {
    pub v: u32,
    /// `xvfb` — the invisible screen; `x11-live` — the live window.
    pub source: String,
    /// The widget's code files and the person's choice in its settings.
    pub fp: String,
    /// The backdrop wallpaper ([`backdrop_tag`]).
    pub wallpaper: String,
    /// `width×height@dpi`.
    pub screen: String,
    /// The conky version.
    pub conky: String,
}

/// Whether the snapshot is fresh: the read fingerprint matched the current one.
pub fn is_fresh(saved: Option<&Meta>, now: &Meta) -> bool {
    saved == Some(now)
}

/// Why the widget is not snapshotted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// No Xvfb.
    NoXvfb,
    /// No conky.
    NoConky,
    /// The widget has its own picture — no snapshot needed.
    OwnPicture,
    /// The widget runs commands and there is no permission (or the files have
    /// changed since): it cannot be started without asking the person.
    NeedsPermission,
    /// Programs or files are missing — the snapshot would come out incomplete.
    Missing,
}

/// Admission to a snapshot from ready answers — the matrix separate from the machine.
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

/// The main settings file of the widget `id` and all its files.
fn widget_files(io: &dyn LookIo, ctx: &Ctx, id: &str) -> Vec<String> {
    let primary = if crate::conky_scan::is_found_name(id) { None } else { crate::de_apply::conky_conf(io, ctx, id) };
    crate::conky_scan::widget_confs(io, ctx, id, primary)
}

/// Admission of the widget `id` to a snapshot on this machine: the package family
/// `family` — for checking missing programs. Installs nothing and
/// asks nothing.
pub fn check_widget(io: &dyn LookIo, ctx: &Ctx, id: &str, family: &str) -> Result<(), Skip> {
    let confs = widget_files(io, ctx, id);
    let own = crate::conky_scan::widget_picture(io, &crate::conky_scan::widget_path(ctx, id), &confs).is_some();
    let allowed = || crate::conky_safety::start_question(io, ctx, id).is_none();
    let healthy = || crate::conky_safety::health(io, ctx, id, family).is_fine();
    eligibility(io.have("Xvfb"), io.have("conky"), own, true, true)?;
    eligibility(true, true, false, allowed(), healthy())
}

/// The hint of the "Conky widgets" section: tile previews are not drawn because
/// there is no Xvfb (Wehrwolfmann's decision of 14.11.2026). One line per section,
/// closed with a cross; an "Install" button — when Rexon can install
/// the package itself (the shared path for widgets' missing programs, pkexec + helper).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct XvfbHint {
    pub visible: bool,
    pub text: String,
    /// The package for the "Install" button; empty — no button.
    pub package: String,
    pub button: String,
    pub close_tooltip: String,
}

/// What is known about the machine for [`xvfb_hint`] — the matrix separate from the machine.
#[derive(Debug, Clone, Default)]
pub struct XvfbHintFacts {
    pub have_xvfb: bool,
    pub have_conky: bool,
    /// The "live window" fallback works (X11) and there are already snapshots in the cache.
    pub live_shots: bool,
    /// The package family (`system::PkgFamily`).
    pub family: String,
    pub steamos: bool,
    /// Rexon can install packages on this system (not SteamOS and not
    /// immutable).
    pub installable: bool,
    /// The person closed the hint.
    pub closed: bool,
}

/// Whether to show the Xvfb hint, with which text and package. Without conky
/// there will be no previews even with Xvfb — the section caption says so, not this hint.
pub fn xvfb_hint(f: &XvfbHintFacts) -> XvfbHint {
    if f.have_xvfb || !f.have_conky || f.live_shots || f.closed {
        return XvfbHint::default();
    }
    let close_tooltip = crate::i18n::tr("Don't show this hint again");
    if f.steamos {
        return XvfbHint {
            visible: true,
            text: crate::i18n::tr(
                "To see widget previews, Rexon needs the Xvfb program. On SteamOS it comes inside Rexon's \
                 system extension, which isn't active now — reinstall Rexon to bring it back.",
            ),
            close_tooltip,
            ..XvfbHint::default()
        };
    }
    let exact = crate::conky_safety::program_package("Xvfb", &f.family);
    let (text, package) = match (exact, f.installable) {
        (Some(pkg), true) => (
            // TRANSLATORS: %1 is a package name; an “Install” button follows.
            crate::i18n::trf("To see widget previews, the %1 package is needed.", &[&pkg]),
            pkg,
        ),
        _ => {
            let pkg = crate::utils::tool_package_name("Xvfb", &f.family);
            let text = if pkg.is_empty() {
                crate::i18n::tr("To see widget previews, install the Xvfb program with your system's tools.")
            } else {
                // TRANSLATORS: %1 is a package name.
                crate::i18n::trf("To see widget previews, install the %1 package with your system's tools.", &[&pkg])
            };
            (text, String::new())
        }
    };
    let button = if package.is_empty() { String::new() } else { crate::i18n::tr("📦 Install") };
    XvfbHint { visible: true, text, package, button, close_tooltip }
}

/// Whether the cache has at least one widget snapshot (any — of the live window or
/// the invisible screen).
pub fn has_any_shot(ctx: &Ctx) -> bool {
    let dir = format!("{}/{CACHE_SUBDIR}", crate::appearance_cache::cache_dir(&ctx.home));
    std::fs::read_dir(dir)
        .map(|it| it.flatten().any(|e| e.path().extension().is_some_and(|x| x == "png")))
        .unwrap_or(false)
}

/// The `settings.json` key: the person closed the Xvfb hint.
pub const XVFB_HINT_CLOSED_KEY: &str = "conky_xvfb_hint_closed";

/// [`xvfb_hint`] on this machine.
pub fn live_xvfb_hint() -> XvfbHint {
    let io = crate::animations::SystemIo;
    let ctx = Ctx::live();
    let have_xvfb = io.have("Xvfb");
    let closed = crate::settings::SettingsManager::new()
        .ok()
        .and_then(|s| s.get(XVFB_HINT_CLOSED_KEY).and_then(serde_json::Value::as_bool))
        .unwrap_or(false);
    if have_xvfb || closed {
        return XvfbHint::default();
    }
    let detector = crate::system::DistroDetector::detect_live();
    let display = std::env::var("DISPLAY").ok();
    let facts = XvfbHintFacts {
        have_xvfb,
        have_conky: io.have("conky"),
        live_shots: can_capture(&ctx, display.as_deref()) && has_any_shot(&ctx),
        family: detector.family_live().as_str().to_string(),
        steamos: detector.is_steamos(),
        installable: !detector.is_steamos() && !detector.is_immutable(),
        closed,
    };
    xvfb_hint(&facts)
}

/// Close the Xvfb hint for good.
pub fn close_xvfb_hint() {
    if let Ok(mut s) = crate::settings::SettingsManager::new() {
        if let Err(e) = s.set(XVFB_HINT_CLOSED_KEY, serde_json::Value::Bool(true)) {
            crate::log_warn!("conky", "подсказка про Xvfb не запомнена закрытой: {e}");
        }
    }
}

/// Install Xvfb via the shared path for Conky widgets' missing programs
/// ([`crate::conky_safety::install_packages_live`]: the password window, repositories
/// only). The result is a line for the person.
pub fn install_xvfb_live() -> (bool, String) {
    let hint = live_xvfb_hint();
    if hint.package.is_empty() {
        return (false, crate::i18n::tr("Rexon can't install programs on this system by itself."));
    }
    match crate::conky_safety::install_packages_live(std::slice::from_ref(&hint.package)) {
        Ok(()) => (true, crate::i18n::tr("✅ Xvfb is installed — widget previews will appear on the tiles.")),
        Err(why) => {
            crate::log_warn!("conky", "Xvfb для превью не встал ({}): {why}", hint.package);
            (false, why)
        }
    }
}

/// The fingerprint of the widget `id` code and the person's choice for the file `conf`.
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

/// All of Rexon's calls into libX11 are under one lock: the error handler and
/// `XSetAuthorization` of the library are shared per process.
fn x_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

/// X errors during a snapshot (the window closed, not shown yet) do not bring down
/// the process: libX11's default handler terminates the program on them.
unsafe extern "C" fn quiet_x_error(_: *mut x11_dl::xlib::Display, _: *mut x11_dl::xlib::XErrorEvent) -> std::os::raw::c_int {
    0
}

/// A raw window snapshot: position on the root, depth and BGRA.
struct RawShot {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    depth: u32,
    /// BGRA without row padding.
    bgra: Vec<u8>,
}

/// Snapshot the window `window` of an open display. Call under [`x_lock`].
///
/// SAFETY: `dpy` is a live connection of `lib`.
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

/// Snapshot the window `window` of the display `display`: `(width, height, RGBA)` — for
/// the live path (a PNG with transparency).
pub fn grab_window(display: &str, window: u64) -> Result<(u32, u32, Vec<u8>), String> {
    use x11_dl::xlib;
    let lib = xlib::Xlib::open().map_err(|e| e.to_string())?;
    let name = std::ffi::CString::new(display).map_err(|e| e.to_string())?;
    let _g = x_lock();
    // SAFETY: the display is opened and closed right here; the image
    // is freed in `grab_raw`; pointers do not leave the function.
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
//  The test bench: Xvfb + wallpaper + "compositor"
// ════════════════════════════════════════════════════════════════════════════

/// The test bench screen size and density.
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

/// `Xft.dpi` from the `xrdb -query` output.
pub fn parse_xft_dpi(out: &str) -> Option<u32> {
    out.lines()
        .find_map(|l| l.trim().strip_prefix("Xft.dpi:"))
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| *v >= 48.0 && *v <= 480.0)
        .map(|v| v.round() as u32)
}

static GUI_SCREEN: Mutex<Option<Screen>> = Mutex::new(None);

/// The primary screen as the Rexon window reports it (physical pixels and logical
/// density): on Wayland there is no other way to find it out.
pub fn set_screen(width: u32, height: u32, dpi: u32) {
    if width >= 320 && height >= 200 {
        let dpi = if (48..=480).contains(&dpi) { dpi } else { 96 };
        *GUI_SCREEN.lock().unwrap_or_else(|p| p.into_inner()) = Some(Screen { width, height, dpi });
    }
}

/// The test bench screen: on X11 — the `xrandr` primary monitor and `Xft.dpi`, otherwise —
/// what the Rexon window said, otherwise 1920×1080 at 96 dpi.
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
    // Without /dev/urandom — at least something unpredictable for outsiders.
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mix = t ^ ((std::process::id() as u128) << 64);
    mix.to_le_bytes().to_vec()
}

/// Stop the process group of `child`: TERM, a second, KILL.
fn kill_group(child: &mut std::process::Child) {
    let pid = child.id() as i32;
    // SAFETY: a signal to our own child's group (the group = its pid).
    unsafe {
        libc::killpg(pid, libc::SIGTERM);
    }
    let until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < until {
        if matches!(child.try_wait(), Ok(Some(_))) {
            // The child exited, but its children may have remained in the group.
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

/// Stop by pid all conky with the overlay `launch` — copies that left the
/// process group.
fn kill_strays(launch: &str) {
    let Ok(rd) = std::fs::read_dir("/proc") else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(pid) = name.parse::<i32>() else { continue };
        let Ok(cmd) = std::fs::read(format!("/proc/{pid}/cmdline")) else { continue };
        let cmd = String::from_utf8_lossy(&cmd);
        if crate::de_apply::conky_launch_of_cmdline(&cmd).as_deref() == Some(launch) {
            // SAFETY: the pid was just read, and it is our preview conky.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

/// The invisible screen with its own wallpaper. Lives in one thread — the queue
/// thread; shuts down in `Drop`.
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
    /// Bring up Xvfb of size `screen`, with `backdrop` as the root, with the compositor
    /// sign.
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
        // SAFETY: an ordinary pipe; the ends are closed below.
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
        // SAFETY: in the child after fork — only async-signal-safe calls.
        unsafe {
            cmd.pre_exec(move || {
                libc::fcntl(wfd, libc::F_SETFD, 0);
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        let spawned = cmd.spawn();
        // SAFETY: our end of the pipe is no longer needed.
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

    /// The test bench display number (`:N`).
    pub fn display(&self) -> &str {
        &self.display
    }

    /// The test bench cookie file.
    pub fn auth_file(&self) -> &str {
        &self.auth
    }

    fn connect(&mut self, cookie: &[u8]) -> Result<(), String> {
        let name = std::ffi::CString::new(self.display.clone()).map_err(|e| e.to_string())?;
        let mech = b"MIT-MAGIC-COOKIE-1";
        for _ in 0..20 {
            let _g = x_lock();
            // SAFETY: XSetAuthorization copies the data; right after
            // connecting the normal order (XAUTHORITY) is restored.
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

    /// The wallpaper as the root (+ `_XROOTPMAP_ID`/`ESETROOT_PMAP_ID`) and the window owning
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
        // SAFETY: the connection is live; the image buffer belongs to Rust and
        // is detached from the XImage before XDestroyImage.
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
            // "There is a compositor": conky takes a 32-bit window with alpha.
            let cm = (lib.XCreateSimpleWindow)(dpy, root, 0, 0, 1, 1, 0, 0, 0);
            let sel = std::ffi::CString::new(format!("_NET_WM_CM_S{scr}")).unwrap_or_default();
            let a = (lib.XInternAtom)(dpy, sel.as_ptr(), 0);
            (lib.XSetSelectionOwner)(dpy, a, cm, 0);
            (lib.XSync)(dpy, 0);
            (lib.XSetErrorHandler)(prev);
        }
        Ok(())
    }

    /// Snapshot a copy of the widget: `launch_source` — the file for `dofile` (the author's,
    /// the 1.9 conversion or the person's edits), `work_dir` — the widget folder,
    /// `key` — the settings file key. Done — an RGBA tile.
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
        // Language and fonts — as the person's: the original LANG/LC_*/LANGUAGE (Rexon
        // may have replaced LANGUAGE for its own interface), the rest of the environment
        // (HOME, XDG_CONFIG_HOME, FONTCONFIG_*) is inherited as is — otherwise
        // weekdays and Cyrillic in the widget come out as boxes.
        crate::i18n::restore_original_locale_env(&mut cmd);
        // SAFETY: in the child after fork — only prctl.
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
                // A flat window (one fill without text) is accepted toward
                // the end of the wait: real widgets can be like that too.
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

    /// The widget window over the wallpaper.
    fn grab_widget(&self, window: u64, flat_ok: bool) -> Result<(u32, u32, Vec<u8>), String> {
        let shot = {
            let _g = x_lock();
            // SAFETY: the test bench connection is live.
            unsafe {
                let prev = (self.lib.XSetErrorHandler)(Some(quiet_x_error));
                let r = grab_raw(&self.lib, self.dpy, window);
                (self.lib.XSync)(self.dpy, 0);
                (self.lib.XSetErrorHandler)(prev);
                r
            }
        }?;
        // "Not a single color" — by the raw window, before blending onto the wallpaper.
        let visible = shot.depth != 32 || shot.bgra.get(..4).is_some_and(|p| p.iter().any(|c| *c != 0));
        if !has_content(&shot.bgra) && !(flat_ok && visible) {
            return Err("виджет ещё ничего не нарисовал".to_string());
        }
        let bg = &self.backdrop;
        // A window partly beyond the screen edge — only the visible part.
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

    /// A widget without its own window (`own_window = false`) draws on the root:
    /// the bounding box is the difference between the root and the wallpaper.
    fn grab_desktop(&self) -> Result<(u32, u32, Vec<u8>), String> {
        let shot = {
            let _g = x_lock();
            // SAFETY: the test bench connection is live.
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
            // SAFETY: the connection is closed once.
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

/// The display number from `-displayfd`: a line with a number. `None` — did not wait long enough.
fn read_display_number(fd: i32, timeout_ms: i32) -> Option<String> {
    let mut buf = Vec::new();
    let until = Instant::now() + Duration::from_millis(timeout_ms as u64);
    loop {
        let left = until.saturating_duration_since(Instant::now()).as_millis() as i32;
        if left <= 0 {
            return None;
        }
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: one descriptor, our own buffer.
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
//  A snapshot of one widget
// ════════════════════════════════════════════════════════════════════════════

/// What the widget copy executes: the overlay with the person's edits (if they
/// changed something), the 1.9 conversion or the author's file itself — all in the test bench
/// directory, the person's Rexon overlays are not touched. `(file, update step)`.
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

/// Snapshot the main file of the widget `id` on the test bench and put the snapshot with
/// the fingerprint `meta` into the cache. Done — the snapshot path.
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
    // The previous snapshot (of the live window too) is no longer needed.
    for old in [before, Some(cache_path(ctx, conf))].into_iter().flatten() {
        if old != out {
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(out)
}

// ════════════════════════════════════════════════════════════════════════════
//  The queue
// ════════════════════════════════════════════════════════════════════════════

#[derive(Default)]
struct Queue {
    jobs: VecDeque<(Ctx, String)>,
    running: bool,
    /// Did not work in this form (`file|fingerprint|wallpaper`) — do not try again.
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

/// Enable background snapshots (the Rexon window opened the "Conky widgets" section).
/// Until then [`want`] does nothing — checks and the command line do not bring up
/// the test bench.
pub fn enable() {
    ENABLED.store(true, Ordering::SeqCst);
}

/// Subscribe to "snapshots ready": `notify` is called from the queue thread and
/// returns `false` when the subscriber is gone.
pub fn watch(notify: Box<dyn Fn() -> bool + Send>) {
    hooks().lock().unwrap_or_else(|p| p.into_inner()).push(notify);
}

fn dispatch() {
    hooks().lock().unwrap_or_else(|p| p.into_inner()).retain(|n| n());
}

/// Queue the widget `id` for snapshots: if there is no snapshot or it is outdated —
/// retake it (the tile shows the old one meanwhile). Only for our own home and
/// only when snapshots are enabled ([`enable`]).
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

/// The queue thread: one test bench for the whole queue, widgets one at a time; the test bench
/// shuts down after [`STAND_IDLE`] of idleness.
fn worker() {
    let io = crate::animations::SystemIo;
    let have = io.have("Xvfb") && io.have("conky");
    let family = crate::conky_safety::live_family().0;
    let version = if have { crate::conky_overlay::conky_version(&io) } else { None };
    let conky = version.map(|(a, b)| format!("{a}.{b}")).unwrap_or_default();
    let mut stand: Option<Stand> = None;
    let mut broken = false;
    // The wallpaper and the screen are one per batch: found out at first need.
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

/// The desktop wallpaper picture for the backdrop (the Rexon wallpaper executor).
fn live_backdrop_image() -> Option<String> {
    let slot = crate::wallpaper_exec::live_slot();
    crate::wallpaper_exec::with_slot(&slot, |e| e.desktop_backdrop_image())
}

// ════════════════════════════════════════════════════════════════════════════
//  The live window (X11 without Xvfb)
// ════════════════════════════════════════════════════════════════════════════

/// Snapshot the widget of the file `conf` started from Rexon: the window — from the log,
/// wait for it to draw, no longer than `wait_ms`. Done — the snapshot path.
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

/// After a successful "Start": snapshot the live window in the background — only the X11
/// fallback without Xvfb (with it the invisible screen takes snapshots), and only if
/// there is no snapshot yet. The Rexon window does not wait.
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
        // BGRA: opaque blue and premultiplied white with alpha 128.
        let data = [255u8, 0, 0, 255, 128, 128, 128, 128];
        assert_eq!(bgra_to_rgba(&data, 2, 1, 8, 32), [0, 0, 255, 255, 255, 255, 255, 128]);
        assert_eq!(bgra_to_rgba(&data, 2, 1, 8, 24)[7], 255);
        assert!(has_content(&[0, 0, 0, 0, 1, 0, 0, 0]));
        assert!(!has_content(&[7, 7, 7, 7, 7, 7, 7, 7]));
    }

    /// Defect a3d6898c: a "color greater than alpha" pixel (the `${cpubar}` outline)
    /// came out transparent with alpha 0 and overexposed with a small one.
    #[test]
    fn colour_above_alpha_is_kept() {
        // Red 255 with alpha 0 — the compositor shows red.
        assert_eq!(straight_rgba(0, 0, 255, 0), [255, 0, 0, 255]);
        // Gray 100 with alpha 40 — the alpha is raised to 100, the color is white.
        assert_eq!(straight_rgba(100, 100, 100, 40), [255, 255, 255, 100]);
        // A regular premultiplied one — as before.
        assert_eq!(straight_rgba(0, 0, 0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn over_composes_like_a_compositor() {
        let under = [200, 100, 50];
        // Alpha 0, color 0 — the backdrop.
        assert_eq!(over(&[0, 0, 0, 0], 32, under), under);
        // Alpha 0, color 255 — addition, the outline is visible.
        assert_eq!(over(&[255, 255, 255, 0], 32, under), [255, 255, 255]);
        // Semi-transparent blue #0000ff80 on red ≈ (127, 0, 128).
        assert_eq!(over(&[128, 0, 0, 128], 32, [255, 0, 0]), [127, 0, 128]);
        // Opaque — the color itself; depth 24 — the alpha is not looked at.
        assert_eq!(over(&[1, 2, 3, 255], 32, under), [3, 2, 1]);
        assert_eq!(over(&[1, 2, 3, 0], 24, under), [3, 2, 1]);
    }

    #[test]
    fn frame_is_the_difference_with_the_wallpaper() {
        let base = vec![10u8; 4 * 3 * 3];
        let mut now = base.clone();
        // Points (1,1) and (2,2) changed.
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
        // On the left the margin hits the screen edge: 5 + 100 + 16.
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
    fn xvfb_hint_matrix() {
        let base = XvfbHintFacts {
            have_xvfb: false,
            have_conky: true,
            family: "arch".into(),
            installable: true,
            ..XvfbHintFacts::default()
        };
        // Arch: the table package and the "Install" button.
        let h = xvfb_hint(&base);
        assert!(h.visible && h.package == "xorg-server-xvfb" && !h.button.is_empty(), "{h:?}");
        assert!(h.text.contains("xorg-server-xvfb") && !h.close_tooltip.is_empty());
        // Debian and Fedora — their own names.
        assert_eq!(xvfb_hint(&XvfbHintFacts { family: "debian".into(), ..base.clone() }).package, "xvfb");
        assert_eq!(xvfb_hint(&XvfbHintFacts { family: "fedora".into(), ..base.clone() }).package, "xorg-x11-server-Xvfb");
        // An immutable system: the package name in the text, no button.
        let ro = xvfb_hint(&XvfbHintFacts { installable: false, ..base.clone() });
        assert!(ro.visible && ro.package.is_empty() && ro.button.is_empty() && ro.text.contains("xorg-server-xvfb"));
        // An unfamiliar family: "as in Debian" in words only, without installing.
        let unknown = xvfb_hint(&XvfbHintFacts { family: "gentoo".into(), ..base.clone() });
        assert!(unknown.visible && unknown.package.is_empty() && unknown.text.contains("xvfb"), "{unknown:?}");
        // SteamOS: about the Rexon sysext, not about pacman and not with a button.
        let deck = xvfb_hint(&XvfbHintFacts { steamos: true, installable: false, ..base.clone() });
        assert!(deck.visible && deck.package.is_empty() && deck.button.is_empty(), "{deck:?}");
        assert!(!deck.text.contains("xorg-server-xvfb") && !deck.text.contains("pacman"));
        // Not shown: Xvfb is present, conky is missing, the live window gives previews,
        // the person closed it.
        for quiet in [
            XvfbHintFacts { have_xvfb: true, ..base.clone() },
            XvfbHintFacts { have_conky: false, ..base.clone() },
            XvfbHintFacts { live_shots: true, ..base.clone() },
            XvfbHintFacts { closed: true, ..base.clone() },
        ] {
            assert_eq!(xvfb_hint(&quiet), XvfbHint::default(), "{quiet:?}");
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

    /// A machine with a fake PATH: the files are real, the programs — per the list.
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

    /// A widget like Jhalak: Lua with `io.popen('cava …')` through a variable.
    /// Without permission — not snapshotted; allowed but no cava — neither (health
    /// sees the program in the assembled command too); everything present — snapshotted.
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
        // Its own picture — no snapshot needed.
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

    // ── Test benches: real Xvfb and conky, the person's screen is not touched ─

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

    /// A tile pixel at the point (x, y) — RGB.
    fn px(card: &(u32, u32, Vec<u8>), x: u32, y: u32) -> [u8; 3] {
        let i = ((y * card.0 + x) * 4) as usize;
        [card.2[i], card.2[i + 1], card.2[i + 2]]
    }

    fn keep(name: &str, c: &(u32, u32, Vec<u8>)) {
        if let Ok(dir) = std::env::var("REXON_STAND_KEEP") {
            let _ = write_png(&format!("{dir}/{name}.png"), c.0, c.1, &c.2);
        }
    }

    /// Whether there are no live processes with `needle` in the command line.
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

    /// Semi-transparent `#0000ff80` on a red backdrop ≈ (127, 0, 128);
    /// transparent — the backdrop; `own_window = false` — the bounding box by difference;
    /// `background = true` does not run away; after the test bench — neither Xvfb nor conky.
    /// Run: `cargo test -p rexon-core --lib conky_preview -- --ignored`.
    #[test]
    #[ignore = "stand: needs Xvfb and conky"]
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
        // The middle of the window: 50 % blue on red.
        let mid = px(&c, c.0 / 2, c.1 / 2);
        assert!(mid[0].abs_diff(127) <= 2 && mid[1] <= 2 && mid[2].abs_diff(128) <= 2, "{mid:?}");
        // The margin around — the red wallpaper.
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

    /// The whole queue: a widget "found on disk" (the name is the folder path) without
    /// a picture → a snapshot in the cache with a fingerprint → a notification; the second time —
    /// fresh, not retaken. The cache is the real `~/.cache/rexon`, the check's files
    /// are removed at the end.
    #[test]
    #[ignore = "stand: needs Xvfb and conky; writes to ~/.cache/rexon/conky-previews"]
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
        // The second time — fresh: no notification.
        want(&ctx, &id);
        assert!(rx.recv_timeout(Duration::from_secs(4)).is_err(), "свежий снимок переснят");
        let _ = std::fs::remove_file(&shot);
        let _ = std::fs::remove_file(meta_path(&ctx, &conf));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A real widget of the person's (`REXON_STAND_WIDGET` — its settings file,
    /// for example ~/.config/conky/Jhalak/Jhalak.conf) on their wallpaper
    /// (`REXON_STAND_WALL`): to check Cyrillic and weekdays by eye.
    #[test]
    #[ignore = "stand: needs Xvfb, conky and REXON_STAND_WIDGET"]
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
