//! «Screen effects»: scanlines, CRT, green and amber
//! monitor, video cassette, analog TV, film, black and white — with one
//! setting in every environment where the compositor lets a shader be applied
//! at all.
//!
//! Structure:
//! * The core is ONE set of GLSL functions with the entry point
//!   `vec4 rx_effect(vec2 uv, vec2 res, float t)` ([`core_glsl`]): `uv` is
//!   the 0…1 coordinate, `res` the picture size in pixels, `t` seconds
//!   already multiplied by the speed. The core reads the texture only through
//!   `rx_tex(uv)` — this function is provided by the wrapper. The core is written in the common
//!   subset of GLSL 1.10 / GLSL ES 1.00 (float literals only, no
//!   integer arithmetic and no direct `texture*`), so one and the
//!   same body compiles under any compositor.
//! * Wrappers per method ([`Host`], [`generate`]): a Cogl snippet (GNOME
//!   Shell `Shell.GLSLEffect`, Cinnamon `Cinnamon.GLSLEffect`), Hyprland
//!   `decoration:screen_shader` (GLSL ES 300), picom `window_shader()`
//!   (GLSL 330), Wayfire filters (`@builtin@`, `get_pixel`), KWin
//!   (`#version 140`, `sampler`, `texcoord0`). Strength and speed are baked in as
//!   constants: Hyprland, picom and Wayfire have no uniforms of their own. The frame
//!   rate is the host's business (the GNOME/Cinnamon extension decides how often
//!   to redraw; Hyprland and picom draw every frame).
//! * On «per-window» methods (picom, KWin) the shader lands on each window
//!   separately: curvature and edge darkening on each window would look
//!   like a defect, so in window mode they are turned off
//!   ([`Preset::warps`] marks such presets for the interface).
//! * The «environment → method → support» table ([`support`]) with reasons
//!   for the person.
//! * Hyprland is already applied ([`plan_settings`]): the shader file in
//!   `~/.config/rexon/screen-effects/hyprland.frag`, our file
//!   `rexon-screen-effects.lua|conf` next to the config and the line that includes it
//!   in a managed block — the same mechanism as «Animations»
//!   ([`crate::animations::wm_include`]). Turning it off removes the block, and the person's config
//!   returns to its previous bytes.
//! * Where there is no own screen shader but there is a stock black-and-white filter
//!   (KDE, Pantheon, COSMIC), the «Black and white» preset turns that filter on
//!   ([`plan_native_mono`]); the other presets are not offered there.
//!
//! ⚖️ Provenance: all GLSL here is Rexon's own code (Apache-2.0).
//! The techniques are well known (cosine scanlines, barrel distortion,
//! grille mask, YIQ conversion with FCC coefficients, hash noise), not a
//! single line is taken from other shaders. The code of the Shader Effects extension
//! (GPL-2.0) and the GPL libretro shaders were not looked at.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::animations::{
    argv, gsettings_get, gsettings_set, kconfig_value, kwrite, parse_state_line, run_steps, state_line, without_block,
    wm_include, wm_targets, LookCtx, LookIo, LookStep,
};
use crate::i18n::{n_, tr, trf};
use crate::themes::WmSyntax;

// ═══════════════════════════════════════════════════════════════════════════
// Presets
// ═══════════════════════════════════════════════════════════════════════════

/// A ready effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum Preset {
    /// Scanlines over the picture.
    Scanlines,
    /// CRT: curvature, vignette, grille mask, glow, beam convergence.
    Crt,
    /// Monochrome monitor with green phosphor.
    CrtGreen,
    /// Monochrome monitor with amber phosphor.
    CrtAmber,
    /// Video cassette: line jitter, tracking band, blurred color, noise.
    Vhs,
    /// Analog signal (inspired by composite NTSC): blurred color,
    /// rainbow edges, crawling dots.
    Ntsc,
    /// Film: «three-strip» color grading, grain, flicker, scratches,
    /// frame jitter in the window.
    Film,
    /// Black and white.
    Monochrome,
}

// TRANSLATORS: Screen effect preset name (thin dark horizontal lines over the picture, like an old monitor).
const L_SCANLINES: &str = n_("Scanlines");
// TRANSLATORS: Screen effect preset name: looks like an old cathode-ray tube TV/monitor.
const L_CRT: &str = n_("CRT screen");
// TRANSLATORS: Screen effect preset name: old monochrome monitor with green phosphor.
const L_CRT_GREEN: &str = n_("Green monitor");
// TRANSLATORS: Screen effect preset name: old monochrome monitor with amber phosphor.
const L_CRT_AMBER: &str = n_("Amber monitor");
// TRANSLATORS: Screen effect preset name: looks like a worn video cassette.
const L_VHS: &str = n_("VHS tape");
// TRANSLATORS: Screen effect preset name: looks like an analog TV signal (blurry colors, rainbow edges).
const L_NTSC: &str = n_("Analog TV");
// TRANSLATORS: Screen effect preset name: looks like an old film print (grain, flicker, scratches).
const L_FILM: &str = n_("Film");
// TRANSLATORS: Screen effect preset name.
const L_MONO: &str = n_("Black and white");

impl Preset {
    pub const ALL: [Preset; 8] = [
        Preset::Scanlines,
        Preset::Crt,
        Preset::CrtGreen,
        Preset::CrtAmber,
        Preset::Vhs,
        Preset::Ntsc,
        Preset::Film,
        Preset::Monochrome,
    ];

    /// Stable name (in state files and in the interface).
    pub fn id(self) -> &'static str {
        match self {
            Preset::Scanlines => "scanlines",
            Preset::Crt => "crt",
            Preset::CrtGreen => "crt-green",
            Preset::CrtAmber => "crt-amber",
            Preset::Vhs => "vhs",
            Preset::Ntsc => "ntsc",
            Preset::Film => "film",
            Preset::Monochrome => "monochrome",
        }
    }

    pub fn from_id(id: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.id() == id)
    }

    pub fn label(self) -> String {
        tr(match self {
            Preset::Scanlines => L_SCANLINES,
            Preset::Crt => L_CRT,
            Preset::CrtGreen => L_CRT_GREEN,
            Preset::CrtAmber => L_CRT_AMBER,
            Preset::Vhs => L_VHS,
            Preset::Ntsc => L_NTSC,
            Preset::Film => L_FILM,
            Preset::Monochrome => L_MONO,
        })
    }

    /// The preset changes over time (noise, jitter, flicker). Without a clock in
    /// the host or at speed 0 it freezes on one frame.
    pub fn animated(self) -> bool {
        matches!(self, Preset::Vhs | Preset::Ntsc | Preset::Film)
    }

    /// The preset bends the picture or darkens the edges (CRT curvature, vignette).
    /// On per-window methods this is turned off — otherwise every window would bend
    /// separately.
    pub fn warps(self) -> bool {
        matches!(self, Preset::Crt | Preset::CrtGreen | Preset::CrtAmber | Preset::Film)
    }

    /// Body of this preset's `rx_effect`.
    fn body(self) -> String {
        match self {
            Preset::Scanlines => BODY_SCANLINES.to_string(),
            Preset::Crt => BODY_CRT.to_string(),
            Preset::CrtGreen => BODY_PHOSPHOR.replace("RX_TINT_VALUE", "vec3(0.25, 1.0, 0.4)"),
            Preset::CrtAmber => BODY_PHOSPHOR.replace("RX_TINT_VALUE", "vec3(1.0, 0.65, 0.12)"),
            Preset::Vhs => BODY_VHS.to_string(),
            Preset::Ntsc => BODY_NTSC.to_string(),
            Preset::Film => BODY_FILM.to_string(),
            Preset::Monochrome => BODY_MONO.to_string(),
        }
    }
}

/// Effect strength and speed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EffectParams {
    /// 0…1: how pronounced the artifacts are (for «Black and white» — how much
    /// color is removed).
    pub strength: f64,
    /// 0…3, 1 is normal; 0 — the effect freezes (and does not load the GPU with
    /// per-frame redraws).
    pub speed: f64,
}

pub const DEFAULT_STRENGTH: f64 = 0.6;
pub const DEFAULT_SPEED: f64 = 1.0;
pub const MAX_SPEED: f64 = 3.0;

impl Default for EffectParams {
    fn default() -> Self {
        EffectParams { strength: DEFAULT_STRENGTH, speed: DEFAULT_SPEED }
    }
}

impl EffectParams {
    pub fn clamped(self) -> Self {
        let fix = |v: f64, lo: f64, hi: f64, def: f64| if v.is_finite() { v.clamp(lo, hi) } else { def };
        EffectParams {
            strength: fix(self.strength, 0.0, 1.0, DEFAULT_STRENGTH),
            speed: fix(self.speed, 0.0, MAX_SPEED, DEFAULT_SPEED),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// GLSL core
// ═══════════════════════════════════════════════════════════════════════════

/// Shared helpers. Requirement for the wrapper: before this text it declares
/// `vec4 rx_tex(vec2 uv)`, the constants `RX_S` and `RX_WINDOW`, and the precision.
const CORE_HELPERS: &str = r#"
const float RX_PI = 3.14159265;

float rx_hash(vec2 p) {
    p = fract(p * vec2(0.1373, 0.2719) + vec2(0.31, 0.57));
    p += dot(p, p.yx + 19.19);
    return fract(p.x * p.y * 47.3);
}

float rx_vnoise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = rx_hash(i);
    float b = rx_hash(i + vec2(1.0, 0.0));
    float c = rx_hash(i + vec2(0.0, 1.0));
    float d = rx_hash(i + vec2(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

float rx_luma(vec3 c) {
    return dot(c, vec3(0.2126, 0.7152, 0.0722));
}

vec3 rx_yiq(vec3 c) {
    return vec3(dot(c, vec3(0.299, 0.587, 0.114)),
                dot(c, vec3(0.596, -0.274, -0.322)),
                dot(c, vec3(0.211, -0.523, 0.312)));
}

vec3 rx_rgb(vec3 y) {
    return vec3(dot(y, vec3(1.0, 0.956, 0.621)),
                dot(y, vec3(1.0, -0.272, -0.647)),
                dot(y, vec3(1.0, -1.106, 1.703)));
}

vec2 rx_barrel(vec2 uv, float k) {
    vec2 c = uv * 2.0 - 1.0;
    c *= 1.0 + k * dot(c, c) * 0.25;
    return c * 0.5 + 0.5;
}

float rx_inside(vec2 uv, vec2 res) {
    vec2 e = 1.5 / res;
    vec2 a = smoothstep(vec2(0.0), e, uv) * smoothstep(vec2(0.0), e, vec2(1.0) - uv);
    return a.x * a.y;
}

float rx_vignette(vec2 uv, float k) {
    vec2 c = uv - 0.5;
    return clamp(1.0 - k * dot(c, c) * 2.0, 0.0, 1.0);
}

float rx_scan(vec2 uv, vec2 res, float k, float period) {
    float s = 0.5 + 0.5 * cos(uv.y * res.y / period * 2.0 * RX_PI);
    return 1.0 - k * (1.0 - s);
}

vec3 rx_mask(vec2 uv, vec2 res, float k) {
    float m = mod(floor(uv.x * res.x), 3.0);
    vec3 w = vec3(1.0 - k);
    if (m < 0.5) {
        w.r = 1.0;
    } else if (m < 1.5) {
        w.g = 1.0;
    } else {
        w.b = 1.0;
    }
    return w * (3.0 / (3.0 - 2.0 * k));
}

vec3 rx_soft(vec2 uv, vec2 px) {
    return (rx_tex(uv + vec2(px.x, 0.0)).rgb + rx_tex(uv - vec2(px.x, 0.0)).rgb
          + rx_tex(uv + vec2(0.0, px.y)).rgb + rx_tex(uv - vec2(0.0, px.y)).rgb) * 0.25;
}

vec4 rx_out(vec3 rgb, float a) {
    return vec4(clamp(rgb, 0.0, a), a);
}
"#;

const BODY_SCANLINES: &str = r#"
vec4 rx_effect(vec2 uv, vec2 res, float t) {
    vec4 c = rx_tex(uv);
    vec3 col = c.rgb * rx_scan(uv, res, 0.6 * RX_S, 3.0) * (1.0 + 0.3 * RX_S);
    return rx_out(col, c.a);
}
"#;

const BODY_CRT: &str = r#"
vec4 rx_effect(vec2 uv, vec2 res, float t) {
    vec2 px = 1.0 / res;
    vec2 q = uv;
    if (RX_WINDOW < 0.5) {
        q = rx_barrel(uv, 0.24 * RX_S);
    }
    vec4 base = rx_tex(q);
    float conv = 0.9 * RX_S;
    vec3 col = vec3(rx_tex(q + vec2(px.x * conv, 0.0)).r, base.g, rx_tex(q - vec2(px.x * conv, 0.0)).b);
    col = mix(col, max(col, rx_soft(q, px * 1.5)), 0.4 * RX_S);
    col *= rx_scan(q, res, 0.45 * RX_S, 3.0);
    col *= rx_mask(uv, res, 0.3 * RX_S);
    col *= 1.0 + 0.25 * RX_S;
    if (RX_WINDOW < 0.5) {
        col *= rx_vignette(q, 0.8 * RX_S) * rx_inside(q, res);
    }
    return rx_out(col, base.a);
}
"#;

/// Monochrome phosphor: `RX_TINT_VALUE` is substituted by [`Preset::body`].
const BODY_PHOSPHOR: &str = r#"
vec4 rx_effect(vec2 uv, vec2 res, float t) {
    vec3 tint = RX_TINT_VALUE;
    vec2 px = 1.0 / res;
    vec2 q = uv;
    if (RX_WINDOW < 0.5) {
        q = rx_barrel(uv, 0.24 * RX_S);
    }
    vec4 base = rx_tex(q);
    float l = rx_luma(base.rgb);
    float glow = rx_luma(rx_soft(q, px * 2.0));
    l = max(l, mix(l, glow, 0.6 * RX_S));
    vec3 col = tint * (l * (1.0 + 0.3 * RX_S) + 0.025 * base.a);
    col *= rx_scan(q, res, 0.5 * RX_S, 3.0);
    if (RX_WINDOW < 0.5) {
        col *= rx_vignette(q, 0.8 * RX_S) * rx_inside(q, res);
    }
    return rx_out(col, base.a);
}
"#;

const BODY_VHS: &str = r#"
vec4 rx_effect(vec2 uv, vec2 res, float t) {
    vec2 px = 1.0 / res;
    float line = floor(uv.y * res.y * 0.5);
    float frame = floor(t * 30.0);
    float jit = rx_vnoise(vec2(line * 0.07, t * 5.0)) - 0.5;
    float band = fract(t * 0.06);
    float near = 1.0 - smoothstep(0.0, 0.05, abs(uv.y - band));
    vec2 q = uv;
    q.x += (jit * 0.003 + near * 0.02 * (rx_hash(vec2(line, frame)) - 0.5)) * RX_S;
    q.x += sin(uv.y * 7.0 + t * 1.3) * 0.0012 * RX_S;
    vec4 base = rx_tex(q);
    float off = (0.5 + 3.5 * RX_S) * px.x;
    vec3 shifted = vec3(rx_tex(q + vec2(off, 0.0)).r, base.g, rx_tex(q - vec2(off, 0.0)).b);
    vec3 wide = (rx_tex(q + vec2(off * 2.0, 0.0)).rgb + rx_tex(q - vec2(off * 2.0, 0.0)).rgb + shifted * 2.0) * 0.25;
    vec3 sharp = rx_yiq(base.rgb);
    vec3 soft = rx_yiq(wide);
    vec3 col = rx_rgb(vec3(sharp.x, soft.yz * (1.0 - 0.3 * RX_S)));
    col += (rx_hash(floor(uv * res * 0.5) + frame * 1.37) - 0.5) * 0.12 * RX_S;
    col += near * rx_hash(vec2(floor(uv.x * res.x * 0.25), frame + line)) * 0.3 * RX_S;
    col *= rx_scan(uv, res, 0.15 * RX_S, 2.0);
    return rx_out(col, base.a);
}
"#;

const BODY_NTSC: &str = r#"
vec4 rx_effect(vec2 uv, vec2 res, float t) {
    vec2 px = 1.0 / res;
    vec2 p = floor(uv * res);
    float w = (1.0 + 2.0 * RX_S) * px.x;
    vec4 base = rx_tex(uv);
    vec3 c0 = rx_yiq(base.rgb);
    vec3 l1 = rx_yiq(rx_tex(uv - vec2(w, 0.0)).rgb);
    vec3 r1 = rx_yiq(rx_tex(uv + vec2(w, 0.0)).rgb);
    vec3 l2 = rx_yiq(rx_tex(uv - vec2(w * 2.0, 0.0)).rgb);
    vec3 r2 = rx_yiq(rx_tex(uv + vec2(w * 2.0, 0.0)).rgb);
    float y = mix(c0.x, c0.x * 0.5 + (l1.x + r1.x) * 0.25, 0.6 * RX_S);
    vec2 iq = (c0.yz * 2.0 + (l1.yz + r1.yz) * 1.5 + (l2.yz + r2.yz)) / 7.0;
    float edge = c0.x - 0.5 * (l1.x + r1.x);
    float phase = (p.x + p.y) * RX_PI * 0.5 + t * RX_PI * 4.0;
    iq += edge * 0.6 * RX_S * vec2(cos(phase), sin(phase));
    y += 0.04 * RX_S * sin(phase) * length(iq);
    vec3 col = rx_rgb(vec3(y, iq));
    col *= rx_scan(uv, res, 0.2 * RX_S, 2.0);
    return rx_out(col, base.a);
}
"#;

const BODY_FILM: &str = r#"
vec4 rx_effect(vec2 uv, vec2 res, float t) {
    vec2 q = uv;
    if (RX_WINDOW < 0.5) {
        q += (vec2(rx_vnoise(vec2(t * 3.1, 0.0)), rx_vnoise(vec2(0.0, t * 2.7))) - 0.5) * 0.0025 * RX_S;
    }
    vec4 base = rx_tex(q);
    vec3 c = base.rgb;
    vec3 others = vec3(c.g + c.b, c.r + c.b, c.r + c.g) * 0.5;
    vec3 tc = clamp(c + (c - others) * 0.6, 0.0, 1.0);
    tc = mix(tc, tc * tc * (3.0 - 2.0 * tc), 0.35);
    tc *= vec3(1.04, 1.0, 0.92);
    c = mix(c, tc * base.a, RX_S);
    float frame = floor(t * 24.0);
    c += (rx_hash(floor(uv * res) + frame * vec2(3.11, 1.73)) - 0.5) * 0.08 * RX_S;
    c *= 1.0 + (rx_vnoise(vec2(t * 9.0, 1.7)) - 0.5) * 0.08 * RX_S;
    float sx = rx_hash(vec2(frame, 3.7));
    float on = step(0.6, rx_hash(vec2(frame, 9.1)));
    float d = abs(uv.x - sx) * res.x;
    c += on * (1.0 - smoothstep(0.0, 1.5, d)) * 0.25 * RX_S;
    if (RX_WINDOW < 0.5) {
        c *= rx_vignette(uv, 0.6 * RX_S);
    }
    return rx_out(c, base.a);
}
"#;

const BODY_MONO: &str = r#"
vec4 rx_effect(vec2 uv, vec2 res, float t) {
    vec4 c = rx_tex(uv);
    return rx_out(mix(c.rgb, vec3(rx_luma(c.rgb)), RX_S), c.a);
}
"#;

/// A GLSL number: always with a dot (GLSL 1.10 and ES 1.00 do not convert integers).
fn glsl_float(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') { format!("{s}0") } else { s.to_string() }
}

/// The preset core: constants, helpers and `rx_effect`. Before it the wrapper
/// declares the precision and `vec4 rx_tex(vec2 uv)`.
pub fn core_glsl(preset: Preset, params: EffectParams, window_mode: bool) -> String {
    let p = params.clamped();
    let mut out = String::new();
    out.push_str(&format!("// Rexon screen effect \"{}\" (Rexon, Apache-2.0)\n", preset.id()));
    out.push_str(&format!("const float RX_S = {};\n", glsl_float(p.strength)));
    out.push_str(&format!("const float RX_WINDOW = {};\n", if window_mode { "1.0" } else { "0.0" }));
    out.push_str(CORE_HELPERS);
    out.push_str(&preset.body());
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// Wrappers per method
// ═══════════════════════════════════════════════════════════════════════════

/// A method of applying a shader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum Host {
    /// A Cogl snippet for `Shell.GLSLEffect` (GNOME) and `Cinnamon.GLSLEffect`
    /// on `Main.uiGroup` / the stage: the whole screen. The uniforms `rx_res` (actor
    /// size, px) and `rx_time` (seconds) are set by the extension via
    /// `set_uniform_float`; the sampler is layer 0 (the default).
    Cogl,
    /// Hyprland `decoration:screen_shader`: the whole screen, GLSL ES 3.00,
    /// `tex`, `v_texcoord`, `time` (seconds).
    Hyprland,
    /// picom `--window-shader-fg`: each window, GLSL 3.30, `texcoord` in
    /// pixels, `time` in milliseconds, `default_post_processing`.
    Picom,
    /// Wayfire filters (soreau/filters) `wf/filters/set-fs-shader`: the whole
    /// output, GLSL ES 3.00 with `@builtin@`, `get_pixel(uvpos)`; no clock.
    Wayfire,
    /// KWin (a JS effect `addFragmentShader(MapTexture | Modulate, …)`):
    /// each window, `#version 140`, `sampler`, `texcoord0`, `modulation`;
    /// a JS effect has no timer of its own.
    KWin,
}

impl Host {
    pub const ALL: [Host; 5] = [Host::Cogl, Host::Hyprland, Host::Picom, Host::Wayfire, Host::KWin];

    pub fn id(self) -> &'static str {
        match self {
            Host::Cogl => "cogl",
            Host::Hyprland => "hyprland",
            Host::Picom => "picom",
            Host::Wayfire => "wayfire-filters",
            Host::KWin => "kwin",
        }
    }

    /// The host gives the shader time — animated presets come alive.
    pub fn has_time(self) -> bool {
        matches!(self, Host::Cogl | Host::Hyprland | Host::Picom)
    }

    /// The shader lands on each window rather than on the whole screen.
    pub fn per_window(self) -> bool {
        matches!(self, Host::Picom | Host::KWin)
    }

    /// Shader file extension.
    pub fn file_ext(self) -> &'static str {
        match self {
            Host::Cogl => "glsl",
            _ => "frag",
        }
    }
}

/// A ready shader for a method.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Shader {
    pub host: Host,
    pub preset: Preset,
    /// Shader file text. For Cogl — the snippet declarations (`declarations`).
    pub source: String,
    /// Cogl only: the snippet code (`code`, the `FRAGMENT` hook, with replacement).
    pub cogl_code: Option<String>,
    /// Uniforms the host must set (besides the default sampler).
    pub uniforms: Vec<&'static str>,
    /// The shader reads time — the host must redraw every frame.
    pub animated: bool,
    /// Window mode: no curvature and no vignette.
    pub window_mode: bool,
}

/// The preset's shader for a method.
pub fn generate(host: Host, preset: Preset, params: EffectParams) -> Shader {
    let p = params.clamped();
    let animated = preset.animated() && p.speed > 0.0 && host.has_time();
    let window_mode = host.per_window();
    let core = core_glsl(preset, p, window_mode);
    let speed = glsl_float(p.speed);
    let time_expr = |base: &str| if animated { format!("{base} * {speed}") } else { "0.0".to_string() };
    let mut uniforms: Vec<&'static str> = Vec::new();
    let mut s = String::new();
    let mut cogl_code = None;
    match host {
        Host::Cogl => {
            s.push_str("uniform sampler2D rx_sampler;\nuniform vec2 rx_res;\n");
            uniforms.push("rx_res");
            if animated {
                s.push_str("uniform float rx_time;\n");
                uniforms.push("rx_time");
            }
            s.push_str("vec4 rx_tex(vec2 uv) {\n    return texture2D(rx_sampler, uv);\n}\n");
            s.push_str(&core);
            cogl_code = Some(format!(
                "cogl_color_out = rx_effect(cogl_tex_coord_in[0].xy, rx_res, {});\n",
                time_expr("rx_time")
            ));
        }
        Host::Hyprland => {
            s.push_str("#version 300 es\nprecision highp float;\n");
            s.push_str("in vec2 v_texcoord;\nuniform sampler2D tex;\nout vec4 fragColor;\n");
            if animated {
                s.push_str("uniform float time;\n");
                uniforms.push("time");
            }
            s.push_str("vec4 rx_tex(vec2 uv) {\n    return texture(tex, uv);\n}\n");
            s.push_str(&core);
            s.push_str(&format!(
                "\nvoid main() {{\n    vec2 res = vec2(textureSize(tex, 0));\n    fragColor = rx_effect(v_texcoord, res, {});\n}}\n",
                time_expr("time")
            ));
        }
        Host::Picom => {
            s.push_str("#version 330\n");
            s.push_str("in vec2 texcoord;\nuniform sampler2D tex;\n");
            if animated {
                s.push_str("uniform float time;\n");
                uniforms.push("time");
            }
            s.push_str("vec4 default_post_processing(vec4 c);\n");
            s.push_str("vec4 rx_tex(vec2 uv) {\n    return texture(tex, uv);\n}\n");
            s.push_str(&core);
            s.push_str(&format!(
                "\nvec4 window_shader() {{\n    vec2 res = vec2(textureSize(tex, 0));\n    return default_post_processing(rx_effect(texcoord / res, res, {}));\n}}\n",
                time_expr("time / 1000.0")
            ));
        }
        Host::Wayfire => {
            s.push_str("#version 300 es\n@builtin_ext@\n@builtin@\n\nprecision highp float;\n");
            s.push_str("uniform sampler2D in_tex;\nout vec4 out_color;\nin mediump vec2 uvpos;\nuniform float progress;\n");
            s.push_str("vec4 rx_tex(vec2 uv) {\n    return get_pixel(uv);\n}\n");
            s.push_str(&core);
            s.push_str(
                "\nvoid main() {\n    vec2 res = vec2(textureSize(in_tex, 0));\n    out_color = mix(get_pixel(uvpos), rx_effect(uvpos, res, 0.0), progress);\n}\n",
            );
        }
        Host::KWin => {
            s.push_str("#version 140\n");
            s.push_str("uniform sampler2D sampler;\nuniform vec4 modulation;\nin vec2 texcoord0;\nout vec4 fragColor;\n");
            s.push_str("vec4 rx_tex(vec2 uv) {\n    return texture(sampler, uv);\n}\n");
            s.push_str(&core);
            s.push_str(
                "\nvoid main() {\n    vec2 res = vec2(textureSize(sampler, 0));\n    fragColor = rx_effect(texcoord0, res, 0.0) * modulation;\n}\n",
            );
        }
    }
    Shader { host, preset, source: s, cogl_code, uniforms, animated, window_mode }
}

// ═══════════════════════════════════════════════════════════════════════════
// Where it is possible: environment → method → support
// ═══════════════════════════════════════════════════════════════════════════

/// What the effect covers in an environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Coverage {
    /// The whole screen.
    Screen,
    /// Each window separately (curvature and vignette turned off).
    Windows,
    /// Only the environment's stock black-and-white filter.
    MonochromeOnly,
    /// Not verified.
    Unknown,
    /// Not possible.
    None,
}

/// A row of the support table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Support {
    /// The environment as the core calls it (`system::detect_current_de`).
    pub de: String,
    pub coverage: Coverage,
    pub host: Option<Host>,
    /// Animated presets come alive (otherwise they freeze on a frame).
    pub animated: bool,
    /// There is a stock black-and-white filter (KDE, Pantheon, COSMIC).
    pub native_monochrome: bool,
    /// Rexon can already turn on effects here.
    pub ready: bool,
    /// What needs to be installed or replaced for this (empty — nothing).
    pub needs: String,
    /// An honest explanation for the person.
    pub note: String,
}

// TRANSLATORS: Screen effects support note for GNOME.
const N_GNOME: &str = n_("Works on the whole screen through Rexon's own GNOME Shell extension.");
// TRANSLATORS: Screen effects support note for Cinnamon.
const N_CINNAMON: &str = n_("Works on the whole screen through Rexon's own Cinnamon extension.");
const N_HYPRLAND: &str = n_(
    // TRANSLATORS: Screen effects support note for Hyprland. "Speed 0" refers to the speed slider.
    "Works on the whole screen through the Hyprland screen shader. Moving effects make Hyprland redraw the whole screen every frame, which noticeably loads the graphics card; at speed 0 the effect stays still and costs almost nothing.",
);
const N_KDE: &str = n_(
    // TRANSLATORS: Screen effects support note for KDE Plasma.
    "KWin applies the effect to each window separately, not to the whole screen: screen curvature and darkened corners are left out, and moving effects stay still. Black and white is also available through Plasma's own color filter.",
);
const N_PICOM: &str = n_(
    // TRANSLATORS: Screen effects support note for MATE, Xfce, LXQt on X11. picom is a program name.
    "Works on each window separately through picom, which replaces the desktop's built-in compositor: screen curvature and darkened corners are left out.",
);
// TRANSLATORS: What must be installed for screen effects on MATE/Xfce/LXQt. picom is a program name.
const NEED_PICOM: &str = n_("picom instead of the built-in compositor");
const N_WAYFIRE: &str = n_(
    // TRANSLATORS: Screen effects support note for Wayfire. "filters" is a plugin name.
    "Works on the whole screen through the filters plugin. Wayfire gives effects no clock, so moving effects stay still.",
);
// TRANSLATORS: What must be installed for screen effects on Wayfire. "filters" is a plugin name.
const NEED_WAYFIRE: &str = n_("the filters plugin for Wayfire (from the AUR)");
// TRANSLATORS: Screen effects support note for Pantheon.
const N_PANTHEON: &str = n_("Pantheon can't add effects to the screen; only its own black and white filter is available.");
// TRANSLATORS: Screen effects support note for COSMIC.
const N_COSMIC: &str = n_("COSMIC can't add effects to the screen; only its own black and white filter is available.");
// TRANSLATORS: Screen effects support note for sway, river, labwc.
const N_WLROOTS: &str = n_("This compositor has no way to apply effects to the screen.");
// TRANSLATORS: Screen effects support note for niri.
const N_NIRI: &str = n_("niri doesn't support screen effects yet (requested from its developers).");
// TRANSLATORS: Screen effects support note for Budgie.
const N_BUDGIE: &str = n_("Budgie has no way to add effects to the screen.");
// TRANSLATORS: Screen effects note for any Deepin session not on Treeland (X11 and others).
const N_DEEPIN: &str = n_("Rexon supports Deepin only on Treeland");
// TRANSLATORS: Screen effects support note for Deepin's Treeland (Wayland session).
const N_TREELAND: &str = n_("Treeland has no way to apply effects to the screen.");
// TRANSLATORS: Screen effects support note for an X11-only desktop started in a Wayland session.
const N_NOT_WAYLAND: &str = n_("Screen effects here work only in an X11 session.");
// TRANSLATORS: Screen effects support note for a desktop Rexon doesn't know.
const N_UNKNOWN: &str = n_("Rexon doesn't know how to add screen effects on this desktop.");

/// The support table for an environment (`wayland` — a Wayland session).
pub fn support(de: &str, wayland: bool) -> Support {
    use Coverage::*;
    let row = |coverage: Coverage, host: Option<Host>, mono: bool, ready: bool, needs: &str, note: &str| Support {
        de: de.to_string(),
        coverage,
        host,
        animated: host.is_some_and(Host::has_time),
        native_monochrome: mono,
        ready,
        needs: if needs.is_empty() { String::new() } else { tr(needs) },
        note: tr(note),
    };
    match de {
        "gnome" => row(Screen, Some(Host::Cogl), false, true, "", N_GNOME),
        "cinnamon-mint" => row(Screen, Some(Host::Cogl), false, true, "", N_CINNAMON),
        "hyprland" => row(Screen, Some(Host::Hyprland), false, true, "", N_HYPRLAND),
        "kde" => row(Windows, Some(Host::KWin), true, false, "", N_KDE),
        "mate" | "xfce" | "lxqt" if !wayland => row(Windows, Some(Host::Picom), false, false, NEED_PICOM, N_PICOM),
        "mate" | "xfce" | "lxqt" => row(None, Option::None, false, false, "", N_NOT_WAYLAND),
        "wayfire" => row(Screen, Some(Host::Wayfire), false, false, NEED_WAYFIRE, N_WAYFIRE),
        "pantheon" => row(MonochromeOnly, Option::None, true, false, "", N_PANTHEON),
        "cosmic" => row(MonochromeOnly, Option::None, true, false, "", N_COSMIC),
        "sway" | "river" | "labwc" => row(None, Option::None, false, false, "", N_WLROOTS),
        "niri" => row(None, Option::None, false, false, "", N_NIRI),
        "budgie" => row(None, Option::None, false, false, "", N_BUDGIE),
        // Deepin — Treeland only (Rexon does not support the X11 session).
        "deepin" if wayland => row(None, Option::None, false, false, "", N_TREELAND),
        "deepin" => row(None, Option::None, false, false, "", N_DEEPIN),
        _ => row(None, Option::None, false, false, "", N_UNKNOWN),
    }
}

/// Section context for the environment `chosen` selected in «Current
/// environment» of the «Appearance» tab: empty, «detect automatically» or
/// the same desktop — the session context as is; another one — its name and ITS session kind
/// ([`KNOWN_DESKTOPS`]; an unknown one keeps the previous session kind). The explanation and the
/// section's writes follow the chosen desktop, not the running one.
pub fn ctx_for(session: LookCtx, chosen: &str) -> LookCtx {
    session.for_chosen(chosen)
}

/// All known environments: (core name, Wayland session).
pub const KNOWN_DESKTOPS: &[(&str, bool)] = &[
    ("gnome", true),
    ("cinnamon-mint", false),
    ("hyprland", true),
    ("kde", true),
    ("mate", false),
    ("xfce", false),
    ("lxqt", false),
    ("wayfire", true),
    ("pantheon", true),
    ("cosmic", true),
    ("sway", true),
    ("river", true),
    ("labwc", true),
    ("niri", true),
    ("budgie", false),
    ("deepin", true),
];

/// The whole table (for the interface and the help).
pub fn support_table() -> Vec<Support> {
    KNOWN_DESKTOPS.iter().map(|(de, w)| support(de, *w)).collect()
}

// ═══════════════════════════════════════════════════════════════════════════
// Settings and writing (Hyprland for now)
// ═══════════════════════════════════════════════════════════════════════════

/// State of «Screen effects».
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EffectSettings {
    /// `None` — effects are turned off.
    pub preset: Option<Preset>,
    pub params: EffectParams,
}

impl Default for EffectSettings {
    fn default() -> Self {
        EffectSettings { preset: None, params: EffectParams::default() }
    }
}

pub const SE_TAG: &str = "screen-effects";
pub const SE_STEM: &str = "rexon-screen-effects";

/// Hyprland shader file (absolute path — Hyprland reads it itself).
pub fn hypr_shader_path(ctx: &LookCtx) -> String {
    ctx.cfg("rexon/screen-effects/hyprland.frag")
}

/// FNV-1a: the shader version tag in our file. If the shader changed (a new
/// Rexon version), our file changes, and Hyprland rereads the config.
fn short_hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn fmt_num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A quoted Lua string.
fn lua_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Our Hyprland file: the state line, the shader path and — for moving
/// effects — disabled damage tracking (without it Hyprland does not
/// redraw the screen every frame and shows an error about `time`).
pub fn hypr_content(syntax: WmSyntax, preset: Preset, params: EffectParams, shader: &Shader, shader_path: &str) -> String {
    let p = params.clamped();
    let c = syntax.comment();
    let mut state = BTreeMap::new();
    state.insert("preset".to_string(), preset.id().to_string());
    state.insert("strength".to_string(), fmt_num(p.strength));
    state.insert("speed".to_string(), fmt_num(p.speed));
    state.insert("shader".to_string(), short_hash(&shader.source));
    let mut out = state_line(c, &state);
    out.push_str(&format!(
        "{c} Rexon: screen effects. This file is rewritten by Rexon; change screen effects in Rexon instead.\n"
    ));
    if syntax == WmSyntax::HyprLua {
        out.push_str(&format!("hl.config({{ decoration = {{ screen_shader = {} }} }})\n", lua_string(shader_path)));
        if shader.animated {
            out.push_str("hl.config({ debug = { damage_tracking = 0 } })\n");
        }
    } else {
        out.push_str(&format!("decoration {{\n    screen_shader = {shader_path}\n}}\n"));
        if shader.animated {
            out.push_str("debug {\n    damage_tracking = 0\n}\n");
        }
    }
    out
}

fn params_from_state(map: &BTreeMap<String, String>) -> EffectParams {
    let num = |k: &str, def: f64| map.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(def);
    EffectParams { strength: num("strength", DEFAULT_STRENGTH), speed: num("speed", DEFAULT_SPEED) }.clamped()
}

/// Snapshot: what is enabled now. Strength and speed are remembered even for a disabled
/// effect (from our file) so the sliders do not reset.
pub fn read_settings(io: &dyn LookIo, ctx: &LookCtx) -> EffectSettings {
    match ctx.de.as_str() {
        "hyprland" => read_hyprland(io, ctx),
        "gnome" | "cinnamon-mint" => read_gnome_cinnamon(io, ctx),
        "kde" | "pantheon" | "cosmic" => read_native_mono(io, ctx),
        _ => EffectSettings::default(),
    }
}

fn read_hyprland(io: &dyn LookIo, ctx: &LookCtx) -> EffectSettings {
    let targets = wm_targets(ctx, "hyprland", SE_STEM, &|_| String::new());
    let Some(target) = targets.iter().find(|t| io.read(&t.config).is_some()) else {
        return EffectSettings::default();
    };
    let Some(own) = io.read(&target.own_path) else {
        return EffectSettings::default();
    };
    let state = parse_state_line(&own);
    let params = params_from_state(&state);
    let included = io.read(&target.config).is_some_and(|c| c.contains(&target.include_line));
    let preset = if included { state.get("preset").and_then(|id| Preset::from_id(id)) } else { None };
    EffectSettings { preset, params }
}

// ── GNOME and Cinnamon: through Rexon's own extensions ──────────────────────
//
// The `rexon-wallpaper@rexon.app` extension (it is always installed together with
// the program) puts the `Host::Cogl` shader on `Main.uiGroup` — the whole screen — and
// runs the clock of moving effects itself. The contract:
// * the extension schema key [`SHELL_EFFECT_KEY`] is the path to the shader file,
//   empty — the effect is off (the default);
// * the file holds the snippet declarations, the line [`COGL_CODE_MARKER`], the snippet code
//   ([`cogl_file_content`]);
// * there are two files, used in turn ([`shell_shader_slots`]): the new shader is written into
//   the one the key does NOT point to right now, and only then is the key
//   switched. Switching is a single write, so rolling back «as it was» comes for free:
//   if the write fails, the key points to the previous intact file; turning off means
//   the key is empty, and the extension removes the effect, returning the screen to its previous look;
// * strength, speed and preset are remembered in our state file
//   ([`shell_state_path`]) even for a disabled effect — the sliders do not
//   reset.
// No shell restart is needed: the extension watches the key.

/// Extension schema key with the path to the shader file (empty — off).
pub const SHELL_EFFECT_KEY: &str = "effect-shader";
/// The line between the declarations and the snippet code in the shader file.
pub const COGL_CODE_MARKER: &str = "//@rexon-cogl-code";

/// Shader file text for the GNOME/Cinnamon extension.
pub fn cogl_file_content(shader: &Shader) -> String {
    let code = shader.cogl_code.as_deref().unwrap_or_default();
    format!("{}\n{COGL_CODE_MARKER}\n{code}", shader.source.trim_end())
}

/// Two slots for the shader file: the new one is written where the key is not pointing.
pub fn shell_shader_slots(ctx: &LookCtx) -> [String; 2] {
    [ctx.cfg("rexon/screen-effects/shell-a.glsl"), ctx.cfg("rexon/screen-effects/shell-b.glsl")]
}

/// Our state file (preset, strength, speed).
pub fn shell_state_path(ctx: &LookCtx) -> String {
    ctx.cfg("rexon/screen-effects/shell.state")
}

/// A GVariant string `'…'` for `gsettings set`.
fn gvariant_str(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The `gsettings get` reply for a string key → the string itself.
fn parse_gvariant_str(out: &str) -> String {
    let t = out.trim();
    let t = t.strip_prefix("@s ").unwrap_or(t);
    let quoted = (t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"'));
    if t.len() < 2 || !quoted {
        return t.to_string();
    }
    let mut res = String::new();
    let mut chars = t[1..t.len() - 1].chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                res.push(n);
            }
        } else {
            res.push(c);
        }
    }
    res
}

/// `gsettings` with the schema from the extension's own directory, if it is
/// compiled there (the shared schema directory may lack it — the same as for wallpapers,
/// [`crate::wallpaper_engine::shell_ext_schema_dirs`]).
///
/// The directory of an orphaned copy ([`orphan_user_copy`]) is skipped: its schema is from
/// an older version, and the copy itself will be removed.
fn shell_gsettings(io: &dyn LookIo, ctx: &LookCtx) -> Option<(Vec<String>, &'static str)> {
    let schema = crate::wallpaper_engine::shell_ext_schema(&ctx.de)?;
    let dirs = crate::wallpaper_engine::shell_ext_schema_dirs(&ctx.de, &ctx.home, &ctx.data_home, &ctx.data_dirs.join(":"));
    let orphan = orphan_user_copy(io, ctx);
    let mut cmd = vec!["gsettings".to_string()];
    if let Some(dir) = dirs
        .into_iter()
        .filter(|d| orphan.as_deref().is_none_or(|o| !d.starts_with(&format!("{o}/"))))
        .find(|d| io.list_dir(d).iter().any(|n| n == "gschemas.compiled"))
    {
        cmd.push("--schemadir".to_string());
        cmd.push(dir);
    }
    Some((cmd, schema))
}

/// Where the effect key points now. `None` — there is no key (the extension is from
/// an older version or not installed) or `gsettings` did not answer.
fn shell_current_path(io: &dyn LookIo, ctx: &LookCtx) -> Option<String> {
    let (mut cmd, schema) = shell_gsettings(io, ctx)?;
    if !io.have("gsettings") {
        return None;
    }
    cmd.extend(argv(&["get", schema, SHELL_EFFECT_KEY]));
    let (ok, out) = io.run(&cmd);
    ok.then(|| parse_gvariant_str(&out))
}

fn shell_state_content(preset: Option<Preset>, params: EffectParams) -> String {
    let p = params.clamped();
    let mut state = BTreeMap::new();
    if let Some(preset) = preset {
        state.insert("preset".to_string(), preset.id().to_string());
    }
    state.insert("strength".to_string(), fmt_num(p.strength));
    state.insert("speed".to_string(), fmt_num(p.speed));
    let mut out = state_line("#", &state);
    out.push_str("# Rexon: screen effects on GNOME/Cinnamon. Rewritten by Rexon.\n");
    out
}

/// What is enabled now in GNOME or Cinnamon.
pub fn read_gnome_cinnamon(io: &dyn LookIo, ctx: &LookCtx) -> EffectSettings {
    let state = io.read(&shell_state_path(ctx)).map(|t| parse_state_line(&t)).unwrap_or_default();
    let params = params_from_state(&state);
    let on = shell_current_path(io, ctx).is_some_and(|p| !p.is_empty() && io.read(&p).is_some());
    let preset = if on { state.get("preset").and_then(|id| Preset::from_id(id)) } else { None };
    EffectSettings { preset, params }
}

// TRANSLATORS: Screen effects on GNOME/Cinnamon. %1 is the place in Rexon, e.g. “Appearance → GNOME Extensions”.
const MSG_SHELL_EXT_OLD: &str = n_(
    "The Rexon shell extension is left over from an older version and doesn't know screen effects yet. \
     Reinstall it in “%1” and try again.",
);
// TRANSLATORS: Screen effects on GNOME/Cinnamon. %1 is the place in Rexon, e.g. “Appearance → GNOME Extensions”.
const MSG_SHELL_EXT_NOT_RUNNING: &str = n_(
    "The Rexon shell extension isn't running in this session, so screen effects can't be shown. \
     Log out and back in; if that doesn't help, reinstall it in “%1”.",
);

/// The «enable our extension» step if it is disabled. Enabled and
/// unanswered — no steps; not loaded, with an error or of the wrong version —
/// a refusal with an explanation: the effect will not show there, and «done» would be a lie.
fn shell_enable_steps(io: &dyn LookIo, ctx: &LookCtx) -> Result<Vec<LookStep>, String> {
    use crate::wallpaper_engine::{self as wpe, ShellExtState};
    let probe = wpe::shell_ext_state_argv(&ctx.de);
    let out = if io.have(&probe[0]) {
        let (ok, out) = io.run(&probe);
        ok.then_some(out)
    } else {
        None
    };
    match wpe::parse_shell_ext_state(&ctx.de, out.as_deref()) {
        ShellExtState::Enabled if ctx.de != "gnome" => Ok(cinnamon_repair_steps(io, ctx)),
        ShellExtState::Enabled | ShellExtState::Unknown => Ok(Vec::new()),
        // GNOME — `gnome-extensions enable`; Cinnamon — append ourselves to the
        // list of enabled ones (the shell picks it up immediately), a decision shared with
        // wallpapers (`shell_ext_enable_argv`).
        ShellExtState::Disabled => {
            Ok(wpe::shell_ext_enable_argv(&ctx.de, out.as_deref()).map(LookStep::Cmd).into_iter().collect())
        }
        ShellExtState::NotLoaded | ShellExtState::Error | ShellExtState::OutOfDate => {
            Err(trf(MSG_SHELL_EXT_NOT_RUNNING, &[&wpe::shell_ext_place(&ctx.de)]))
        }
    }
}

// ── Cinnamon: the extension is in the enabled list but does not work ─────────
//
// Cinnamon considers an extension «enabled» by the `enabled-extensions` list,
// even if its `enable()` failed. That happened live on 15.11.2026 (Cinnamon 6.8
// Wayland, package r2290): `~/.local/share/cinnamon/extensions` still held
// a copy of the extension from an older Rexon version (Cinnamon takes the copy from
// the home directory before the system one), its `enable()` failed with
// `Meta.BackgroundGroup is not a constructor`, and the screen effect never
// showed even once, while the window said «applied». So for Cinnamon the
// enabled list is not an answer: we ask the shell itself what is actually
// RUNNING (`GetRunningXletUUIDs`), and if ours is not there, we remove the
// orphaned copy and ask the shell to reload the extension (`ReloadXlet` —
// the same as Cinnamon's «Extensions» does on an update; the shell is not
// restarted).

fn cinnamon_dbus_argv(method: &str, args: &[&str]) -> Vec<String> {
    let mut cmd = argv(&["gdbus", "call", "--session", "--dest", "org.Cinnamon", "--object-path", "/org/Cinnamon", "--method"]);
    cmd.push(format!("org.Cinnamon.{method}"));
    cmd.extend(args.iter().map(|a| a.to_string()));
    cmd
}

/// Ask Cinnamon which extensions are running.
pub fn cinnamon_running_argv() -> Vec<String> {
    cinnamon_dbus_argv("GetRunningXletUUIDs", &["extension"])
}

/// Ask Cinnamon to reload our extension from disk.
pub fn cinnamon_reload_argv() -> Vec<String> {
    cinnamon_dbus_argv("ReloadXlet", &[crate::wallpaper_engine::SHELL_EXT_ID, "EXTENSION"])
}

/// The `GetRunningXletUUIDs` reply: `Some(true)` — ours is running, `Some(false)` —
/// it is not, `None` — the reply is unclear (wrong shell or no answer).
pub fn parse_cinnamon_running(out: &str) -> Option<bool> {
    let t = out.trim();
    if !t.starts_with('(') {
        return None;
    }
    Some(t.contains(&format!("'{}'", crate::wallpaper_engine::SHELL_EXT_ID)))
}

/// An orphaned copy of our Cinnamon extension in the home directory: it
/// shadows the system one from the package, while Rexon itself is not installed into the home directory
/// (there is no `~/.local/lib/rexon`), i.e. nobody will ever update the copy again.
/// Only when it differs from the system one — an identical one bothers nobody.
pub fn orphan_user_copy(io: &dyn LookIo, ctx: &LookCtx) -> Option<String> {
    if ctx.de == "gnome" || crate::wallpaper_engine::shell_ext_schema(&ctx.de).is_none() {
        return None;
    }
    let id = crate::wallpaper_engine::SHELL_EXT_ID;
    let user = format!("{}/cinnamon/extensions/{id}", ctx.data_home.trim_end_matches('/'));
    let user_js = io.read(&format!("{user}/extension.js"))?;
    if !io.list_dir(&format!("{}/.local/lib/rexon", ctx.home.trim_end_matches('/'))).is_empty() {
        return None;
    }
    let dirs = if ctx.data_dirs.is_empty() { crate::plugin_journal::data_dirs("") } else { ctx.data_dirs.clone() };
    let system_js = dirs
        .iter()
        .map(|d| format!("{}/cinnamon/extensions/{id}/extension.js", d.trim_end_matches('/')))
        .find_map(|p| io.read(&p))?;
    (system_js != user_js).then_some(user)
}

/// The log line about removing the orphaned copy — one for both paths (screen
/// effects and wallpapers).
fn log_orphan_removal(path: &str) {
    crate::log_info!("screen-effects", "снимаю брошенную копию расширения, заслонявшую системную: {path}");
}

/// Remove the orphaned copy of our Cinnamon extension ([`orphan_user_copy`]),
/// if there is one. A shared point for screen effects (a repair step) and wallpapers
/// (Cinnamon with layer-shell, `wallpaper_exec`): the copy from an older version
/// started a second video host there. `Some` — the path of the removed copy; after
/// removal the running extension must be reloaded ([`cinnamon_reload_argv`]).
pub fn remove_orphan_user_copy(io: &dyn LookIo, ctx: &LookCtx) -> Option<String> {
    let path = orphan_user_copy(io, ctx)?;
    log_orphan_removal(&path);
    io.remove_tree(&path).then_some(path)
}

/// Repair steps if the shell did not start our extension (otherwise empty).
fn cinnamon_repair_steps(io: &dyn LookIo, ctx: &LookCtx) -> Vec<LookStep> {
    let probe = cinnamon_running_argv();
    if !io.have(&probe[0]) {
        return Vec::new();
    }
    let (ok, out) = io.run(&probe);
    if !ok || parse_cinnamon_running(&out) != Some(false) {
        return Vec::new();
    }
    let mut steps = Vec::new();
    if let Some(path) = orphan_user_copy(io, ctx) {
        steps.push(LookStep::RemoveTree { path });
    }
    steps.push(LookStep::Cmd(cinnamon_reload_argv()));
    steps
}

// ── Confirmation from the extension ────────────────────────────────────────
//
// We say «applied» only if the extension itself said the effect
// landed on the screen. The contract with the shared `[rexon-wp:screen-effect]` block: on
// each application the extension writes the file [`shell_status_path`] with lines
// `state=on|off|error`, `path=<shader file>`, `detail=<reason>`. Before
// switching the key Rexon writes `state=pending` with the new path there — so
// the previous reply cannot pass for the new one.

/// The extension's reply file.
pub fn shell_status_path(ctx: &LookCtx) -> String {
    ctx.cfg("rexon/screen-effects/shell.status")
}

/// How long to wait for the extension's reply: it applies 150 ms after the key
/// changes, compiling the shader takes fractions of a second.
pub const CONFIRM_WAIT: std::time::Duration = std::time::Duration::from_secs(3);
/// How often to look into the reply file.
pub const CONFIRM_POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// The extension's reply to turning the effect on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    /// The effect is on the screen.
    Shown,
    /// The extension could not do it (the reason in its own words).
    Failed(String),
}

fn status_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
}

/// What the extension replied about the `expected` shader. `None` — no reply yet.
pub fn shell_confirmation(io: &dyn LookIo, ctx: &LookCtx, expected: &str) -> Option<Confirm> {
    let text = io.read(&shell_status_path(ctx))?;
    if status_field(&text, "path") != Some(expected) {
        return None;
    }
    match status_field(&text, "state")? {
        "on" => Some(Confirm::Shown),
        "error" => Some(Confirm::Failed(status_field(&text, "detail").unwrap_or_default().to_string())),
        _ => None,
    }
}

/// The step «effect key = `value`» through the extension schema.
fn shell_set_key_step(io: &dyn LookIo, ctx: &LookCtx, value: &str) -> Option<LookStep> {
    let (mut cmd, schema) = shell_gsettings(io, ctx)?;
    cmd.extend(argv(&["set", schema, SHELL_EFFECT_KEY]));
    cmd.push(gvariant_str(value));
    Some(LookStep::Cmd(cmd))
}

/// Write plan for GNOME or Cinnamon. `Err` — a ready phrase explaining why not.
pub fn plan_gnome_cinnamon(io: &dyn LookIo, ctx: &LookCtx, new: &EffectSettings) -> Result<Vec<LookStep>, String> {
    if shell_gsettings(io, ctx).is_none() {
        return Err(support(&ctx.de, ctx.wayland).note);
    }
    let set_key = |value: &str| shell_set_key_step(io, ctx, value).expect("схема проверена выше");
    let params = new.params.clamped();
    let state_path = shell_state_path(ctx);
    let current = shell_current_path(io, ctx);
    let Some(preset) = new.preset else {
        // Turning off: the key is empty — the extension removes the effect. No key —
        // nothing to turn off. Strength and speed are remembered.
        let mut steps = Vec::new();
        if current.as_deref().is_some_and(|p| !p.is_empty()) {
            steps.push(set_key(""));
        }
        let state = shell_state_content(None, params);
        if io.read(&state_path).as_deref() != Some(state.as_str()) {
            steps.push(LookStep::WriteFile { path: state_path, content: state });
        }
        return Ok(steps);
    };
    let Some(current) = current else {
        if !io.have("gsettings") {
            return Err(trf("The program %1 is not installed.", &[&"gsettings"]));
        }
        return Err(trf(MSG_SHELL_EXT_OLD, &[&crate::wallpaper_engine::shell_ext_place(&ctx.de)]));
    };
    let enable = shell_enable_steps(io, ctx)?;
    let content = cogl_file_content(&generate(Host::Cogl, preset, params));
    let state = shell_state_content(Some(preset), params);
    let state_same = io.read(&state_path).as_deref() == Some(state.as_str());
    let mut steps = Vec::new();
    // The same shader is already shown (and the extension confirmed it) — the file and
    // the key are left alone. Without confirmation the key is moved to the other file:
    // the extension notices only a CHANGE of the key, and last time it may
    // not have applied the effect at all (it was not running, the shader did not compile).
    let shown = !current.is_empty()
        && io.read(&current).as_deref() == Some(content.as_str())
        && shell_confirmation(io, ctx, &current) == Some(Confirm::Shown);
    if !shown {
        let [a, b] = shell_shader_slots(ctx);
        let slot = if current == a { b } else { a };
        steps.push(LookStep::WriteFile { path: slot.clone(), content });
        steps.push(LookStep::WriteFile {
            path: shell_status_path(ctx),
            content: format!("state=pending\npath={slot}\n"),
        });
        steps.push(set_key(&slot));
    }
    if !state_same {
        steps.push(LookStep::WriteFile { path: state_path, content: state });
    }
    steps.extend(enable);
    Ok(steps)
}

// ── Stock black-and-white filter: KDE, Pantheon, COSMIC ─────────────────────
//
// Rexon does not put its own full-screen shader here, but the environment has
// its own black-and-white filter — that is what the «Black and white» preset turns on:
// * KDE: the built-in KWin effect «Color Blindness Correction»
//   (`colorblindnesscorrection`), mode «All colors (grayscale)».
//   The keys are the same ones the KDE «Accessibility» module writes
//   (`kcmaccessibilitycolorblindnesscorrection.kcfg`):
//   `kwinrc [Plugins] colorblindnesscorrectionEnabled`,
//   `[Effect-colorblindnesscorrection] Mode` (3 — grayscale) and
//   `Intensity` (0…1); then, like the KDE module, `loadEffect`/`unloadEffect` and
//   `reconfigureEffect` on `org.kde.KWin /Effects`. A filter turned on by
//   the person for color blindness (a different mode) is not touched by Rexon: it is
//   their accessibility setting, not our effect.
// * Pantheon: Gala, schema `io.elementary.desktop.wm.accessibility`, keys
//   `enable-monochrome-filter` and `monochrome-filter-strength` (0…1). Gala
//   watches the schema itself.
// * COSMIC: cosmic-comp keeps the screen filter in the state file
//   `$XDG_STATE_HOME/cosmic-comp/a11y_screen_filter.ron` and reads it
//   only at startup (live, the filter is changed only via the shell's own Wayland
//   protocol), so enabling takes effect from the next login.
//   The COSMIC filter has no strength. The person's `inverted` flag is kept.
// A 15-second trial display is not needed here: a stock filter cannot
// leave a black screen like a shader that failed to compile.

/// The KWin effect with a black-and-white mode.
pub const KWIN_CB_EFFECT: &str = "colorblindnesscorrection";
const KWIN_CB_GROUP: &str = "Effect-colorblindnesscorrection";
const KWIN_CB_ENABLED: &str = "colorblindnesscorrectionEnabled";
/// The «grayscale» mode (fourth in the KWin effect's list of modes).
const KWIN_CB_MONO: &str = "3";
/// The Gala schema with the black-and-white filter.
pub const GALA_A11Y_SCHEMA: &str = "io.elementary.desktop.wm.accessibility";

const MSG_KDE_FILTER_BUSY: &str = n_(
    // TRANSLATORS: Screen effects on KDE Plasma: the person already uses Plasma's color filter for color blindness. "System Settings → Accessibility" is the KDE place, keep it as KDE translates it; arrows ← → point the way they should in your language: in right-to-left languages swap them.
    "Plasma's color filter is already in use for color blindness correction. Rexon won't replace it: \
     turn it off in System Settings → Accessibility first.",
);
// TRANSLATORS: Toast after screen effects were changed on COSMIC, which reads the setting only at login.
const MSG_COSMIC_NEXT_LOGIN: &str = n_("COSMIC will apply this the next time you log in.");

/// An environment with a stock filter but without Rexon's own shader: only
/// the «Black and white» preset is available.
pub fn native_mono_only(de: &str, wayland: bool) -> bool {
    let s = support(de, wayland);
    !s.ready && s.native_monochrome
}

/// The COSMIC screen filter file (`state_home` — `$XDG_STATE_HOME`, empty —
/// `~/.local/state`).
pub fn cosmic_filter_path(state_home: &str, home: &str) -> String {
    let base = if state_home.starts_with('/') {
        state_home.trim_end_matches('/').to_string()
    } else {
        format!("{}/.local/state", home.trim_end_matches('/'))
    };
    format!("{base}/cosmic-comp/a11y_screen_filter.ron")
}

fn cosmic_filter_path_env(ctx: &LookCtx) -> String {
    cosmic_filter_path(&std::env::var("XDG_STATE_HOME").unwrap_or_default(), &ctx.home)
}

/// COSMIC file text (RON, in the same form cosmic-comp writes).
fn cosmic_filter_content(inverted: bool, greyscale: bool) -> String {
    let mut out = format!("(\n    inverted: {inverted},\n");
    if greyscale {
        out.push_str("    color_filter: Some(Greyscale),\n");
    }
    out.push(')');
    out
}

/// From the COSMIC file: (inversion, grayscale).
fn parse_cosmic_filter(text: &str) -> (bool, bool) {
    let flat: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    (flat.contains("inverted:true"), flat.contains("color_filter:Some(Greyscale)"))
}

/// A call to `org.kde.kwin.Effects` with the effect name (outside KDE — silently skipped).
fn kwin_effects_call(method: &str) -> LookStep {
    let method = format!("org.kde.kwin.Effects.{method}");
    let arg = [crate::utils::DbusArg::Str(KWIN_CB_EFFECT.to_string())];
    LookStep::Notify(vec![
        crate::utils::dbus_call_argv("dbus-send", "dbus-send", "org.kde.KWin", "/Effects", &method, &arg),
        crate::utils::dbus_call_argv("qdbus", "qdbus6", "org.kde.KWin", "/Effects", &method, &arg),
    ])
}

/// Strength as a number with a dot: gsettings would take `1` for an integer and reject it for `d`.
fn strength_value(strength: f64) -> String {
    format!("{:.2}", strength.clamp(0.0, 1.0))
}

/// What is enabled now via the stock filter.
pub fn read_native_mono(io: &dyn LookIo, ctx: &LookCtx) -> EffectSettings {
    // Stock filters default to full strength (as in KDE and Gala).
    let mut s = EffectSettings { preset: None, params: EffectParams { strength: 1.0, ..EffectParams::default() } };
    let strength = |v: Option<String>| v.and_then(|v| v.parse::<f64>().ok()).map(|v| v.clamp(0.0, 1.0));
    match ctx.de.as_str() {
        "kde" => {
            let on = kconfig_value(io, ctx, "kwinrc", "Plugins", KWIN_CB_ENABLED).as_deref() == Some("true")
                && kconfig_value(io, ctx, "kwinrc", KWIN_CB_GROUP, "Mode").as_deref() == Some(KWIN_CB_MONO);
            if on {
                s.preset = Some(Preset::Monochrome);
                s.params.strength = strength(kconfig_value(io, ctx, "kwinrc", KWIN_CB_GROUP, "Intensity")).unwrap_or(1.0);
            }
        }
        "pantheon" => {
            if gsettings_get(io, GALA_A11Y_SCHEMA, "enable-monochrome-filter").as_deref() == Some("true") {
                s.preset = Some(Preset::Monochrome);
            }
            if let Some(v) = strength(gsettings_get(io, GALA_A11Y_SCHEMA, "monochrome-filter-strength")) {
                s.params.strength = v;
            }
        }
        "cosmic" => {
            if io.read(&cosmic_filter_path_env(ctx)).is_some_and(|t| parse_cosmic_filter(&t).1) {
                s.preset = Some(Preset::Monochrome);
            }
        }
        _ => {}
    }
    s
}

/// Plan for the stock filter. `Err` — a ready phrase explaining why not.
pub fn plan_native_mono(io: &dyn LookIo, ctx: &LookCtx, new: &EffectSettings) -> Result<Vec<LookStep>, String> {
    let want = match new.preset {
        None => false,
        Some(Preset::Monochrome) => true,
        Some(_) => return Err(support(&ctx.de, ctx.wayland).note),
    };
    let cur = read_native_mono(io, ctx);
    let on_now = cur.preset.is_some();
    let strength = new.params.clamped().strength;
    let same_strength = (cur.params.strength - strength).abs() < 0.005;
    let mut steps = Vec::new();
    match ctx.de.as_str() {
        "kde" => {
            let enabled = kconfig_value(io, ctx, "kwinrc", "Plugins", KWIN_CB_ENABLED).as_deref() == Some("true");
            if !want {
                if on_now {
                    // The KDE module's defaults — by deleting the keys: kwinrc
                    // returns to its pre-Rexon form.
                    steps.push(kwrite("kwinrc", "Plugins", KWIN_CB_ENABLED, None, true));
                    steps.push(kwrite("kwinrc", KWIN_CB_GROUP, "Mode", None, false));
                    steps.push(kwrite("kwinrc", KWIN_CB_GROUP, "Intensity", None, false));
                    steps.push(kwin_effects_call("unloadEffect"));
                }
                return Ok(steps);
            }
            if enabled && !on_now {
                return Err(tr(MSG_KDE_FILTER_BUSY));
            }
            if on_now && same_strength {
                return Ok(steps);
            }
            steps.push(kwrite("kwinrc", "Plugins", KWIN_CB_ENABLED, Some("true"), true));
            steps.push(kwrite("kwinrc", KWIN_CB_GROUP, "Mode", Some(KWIN_CB_MONO), false));
            steps.push(kwrite("kwinrc", KWIN_CB_GROUP, "Intensity", Some(&strength_value(strength)), false));
            steps.push(kwin_effects_call("loadEffect"));
            steps.push(kwin_effects_call("reconfigureEffect"));
        }
        "pantheon" => {
            if want && !(on_now && same_strength) {
                steps.push(gsettings_set(GALA_A11Y_SCHEMA, "monochrome-filter-strength", &strength_value(strength)));
            }
            if want != on_now {
                steps.push(gsettings_set(GALA_A11Y_SCHEMA, "enable-monochrome-filter", if want { "true" } else { "false" }));
            }
        }
        "cosmic" => {
            if want != on_now {
                let path = cosmic_filter_path_env(ctx);
                let inverted = io.read(&path).is_some_and(|t| parse_cosmic_filter(&t).0);
                steps.push(LookStep::WriteFile { path, content: cosmic_filter_content(inverted, want) });
            }
        }
        _ => return Err(support(&ctx.de, ctx.wayland).note),
    }
    Ok(steps)
}

fn hypr_reload() -> LookStep {
    LookStep::Notify(vec![argv(&["hyprctl", "reload"])])
}

/// Write plan. `Err` — a ready phrase explaining why it is not possible here.
pub fn plan_settings(io: &dyn LookIo, ctx: &LookCtx, new: &EffectSettings) -> Result<Vec<LookStep>, String> {
    match ctx.de.as_str() {
        "hyprland" => plan_hyprland(io, ctx, new),
        "gnome" | "cinnamon-mint" => plan_gnome_cinnamon(io, ctx, new),
        "kde" | "pantheon" | "cosmic" => plan_native_mono(io, ctx, new),
        _ => Err(support(&ctx.de, ctx.wayland).note),
    }
}

fn plan_hyprland(io: &dyn LookIo, ctx: &LookCtx, new: &EffectSettings) -> Result<Vec<LookStep>, String> {
    let mut steps = Vec::new();
    let Some(preset) = new.preset else {
        // Turning off: remove our block from every config that has it.
        // Our file and shader stay (they remember strength and speed), but are no longer
        // included.
        let targets = wm_targets(ctx, "hyprland", SE_STEM, &|_| String::new());
        for t in &targets {
            if let Some(text) = io.read(&t.config) {
                let back = without_block(&text, t.syntax, SE_TAG);
                if back != text {
                    steps.push(LookStep::WriteFile { path: t.config.clone(), content: back });
                }
            }
        }
        if !steps.is_empty() {
            steps.push(hypr_reload());
        }
        return Ok(steps);
    };
    let params = new.params.clamped();
    let shader = generate(Host::Hyprland, preset, params);
    let path = hypr_shader_path(ctx);
    let content = {
        let shader = shader.clone();
        let path = path.clone();
        move |syntax: WmSyntax| hypr_content(syntax, preset, params, &shader, &path)
    };
    let inc = wm_include(ctx, "hyprland", SE_TAG, SE_STEM, &content);
    let current = inc.targets.iter().find(|t| io.read(&t.config).is_some());
    let unchanged = io.read(&path).as_deref() == Some(shader.source.as_str())
        && current.is_some_and(|t| {
            io.read(&t.own_path).as_deref() == Some(t.own_content.as_str())
                && io.read(&t.config).is_some_and(|c| c.contains(&t.include_line))
        });
    if unchanged {
        return Ok(steps);
    }
    steps.push(LookStep::WriteFile { path, content: shader.source });
    steps.push(LookStep::Wm(inc));
    steps.push(hypr_reload());
    Ok(steps)
}

// TRANSLATORS: Toast after screen effects were changed.
const MSG_APPLIED: &str = n_("Screen effects applied.");

/// Apply: a ready phrase for the popup message (empty — there was nothing
/// to change).
pub fn apply(io: &dyn LookIo, ctx: &LookCtx, new: &EffectSettings) -> String {
    let steps = match plan_settings(io, ctx, new) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if steps.is_empty() {
        return String::new();
    }
    match run_steps(io, ctx, &steps) {
        // COSMIC reads the filter only at login — «applied» would be untrue.
        Ok(()) if ctx.de == "cosmic" => tr(MSG_COSMIC_NEXT_LOGIN),
        Ok(()) => tr(MSG_APPLIED),
        Err(e) => e,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// The «Appearance → Screen effects» window section
// ═══════════════════════════════════════════════════════════════════════════

/// A preset tile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PresetTile {
    pub id: &'static str,
    pub label: String,
    /// Number in the preview shader ([`preview_glsl`], the `rx_preset` uniform).
    pub index: usize,
    pub animated: bool,
    pub warps: bool,
}

/// Section snapshot for the window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScreenEffectsState {
    pub de: String,
    pub coverage: Coverage,
    /// Rexon can turn on effects here: there is «Apply».
    pub can_apply: bool,
    /// Only the environment's stock black-and-white filter ([`plan_native_mono`]):
    /// the tiles are «Off» and «Black and white».
    pub monochrome_only: bool,
    /// The strength slider changes something (the COSMIC filter has no strength).
    pub has_strength: bool,
    /// What is enabled is shown on trial (15 s without an answer — rollback). Needed
    /// only for our own shader: a stock filter will not leave a black screen, and
    /// COSMIC applies it only at login anyway.
    pub trial: bool,
    /// An honest explanation from the [`support`] table.
    pub note: String,
    /// What needs to be installed (empty — nothing).
    pub needs: String,
    /// The effect will land on each window: preview without curvature and vignette.
    pub per_window: bool,
    /// Moving effects come alive here (otherwise they freeze).
    pub moving: bool,
    /// The enabled preset (`""` — off).
    pub preset: String,
    pub strength: f64,
    pub speed: f64,
    pub max_speed: f64,
    pub presets: Vec<PresetTile>,
}

/// Section snapshot: the environment's support table and what is enabled now.
pub fn read_state(io: &dyn LookIo, ctx: &LookCtx) -> ScreenEffectsState {
    let sup = support(&ctx.de, ctx.wayland);
    let cur = read_settings(io, ctx);
    let p = cur.params.clamped();
    let mono_only = native_mono_only(&ctx.de, ctx.wayland);
    ScreenEffectsState {
        de: ctx.de.clone(),
        coverage: sup.coverage,
        can_apply: sup.ready || mono_only,
        monochrome_only: mono_only,
        has_strength: ctx.de != "cosmic",
        trial: sup.ready,
        note: sup.note,
        needs: sup.needs,
        per_window: sup.host.is_some_and(Host::per_window),
        moving: sup.animated,
        preset: cur.preset.map(|p| p.id().to_string()).unwrap_or_default(),
        strength: p.strength,
        speed: p.speed,
        max_speed: MAX_SPEED,
        presets: Preset::ALL
            .iter()
            .enumerate()
            .filter(|(_, p)| !mono_only || **p == Preset::Monochrome)
            .map(|(index, p)| PresetTile {
                id: p.id(),
                label: p.label(),
                index,
                animated: p.animated(),
                warps: p.warps(),
            })
            .collect(),
    }
}

/// Result of «Apply» for the window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApplyOutcome {
    /// Phrase for the popup message (empty — there was nothing to change).
    pub text: String,
    /// Something was written (otherwise the trial display is not restarted).
    pub changed: bool,
    /// The effect is enabled and (for our own GNOME/Cinnamon extensions) confirmed
    /// by the extension itself: only then does the window start the trial display.
    pub shown: bool,
}

const MSG_SHELL_NO_ANSWER: &str = n_(
    // TRANSLATORS: Screen effects on GNOME/Cinnamon: the shell extension never answered after "Apply". %1 is the place in Rexon, e.g. “Appearance → Cinnamon Extensions”.
    "Rexon's shell extension didn't confirm the effect, so the screen was left as it was. \
     Log out and back in; if that doesn't help, reinstall the extension in “%1”.",
);
// TRANSLATORS: Screen effects on GNOME/Cinnamon: the shell couldn't build the shader. %1 is the reason as the shell reported it (may be in English).
const MSG_SHELL_EFFECT_FAILED: &str = n_("The desktop couldn't show this effect (%1), so the screen was left as it was.");

/// «Apply» with the extension's reply: on GNOME and Cinnamon «applied» — only
/// if the extension confirmed that the effect is on screen; if it did not confirm within
/// [`CONFIRM_WAIT`] or could not — the key is reset (the effect will not pop up
/// by itself at the next login without a trial display), and the phrase says why.
/// `sleep` — the pause between looks into the reply file (empty in tests).
///
/// ⚠️ Waits up to [`CONFIRM_WAIT`]: call ONLY from a worker thread, not from
/// the main one (window hang 15.11.2026).
pub fn apply_confirmed(io: &dyn LookIo, ctx: &LookCtx, new: &EffectSettings, sleep: &dyn Fn(std::time::Duration)) -> ApplyOutcome {
    let fail = |text: String| ApplyOutcome { text, changed: false, shown: false };
    let steps = match plan_settings(io, ctx, new) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    if steps.is_empty() {
        return ApplyOutcome { text: String::new(), changed: false, shown: new.preset.is_some() };
    }
    if let Some(LookStep::RemoveTree { path }) = steps.iter().find(|s| matches!(s, LookStep::RemoveTree { .. })) {
        log_orphan_removal(path);
    }
    if let Err(e) = run_steps(io, ctx, &steps) {
        return fail(e);
    }
    let shell = matches!(ctx.de.as_str(), "gnome" | "cinnamon-mint");
    let applied = || match ctx.de.as_str() {
        // COSMIC reads the filter only at login — «applied» would be untrue.
        "cosmic" => tr(MSG_COSMIC_NEXT_LOGIN),
        _ => tr(MSG_APPLIED),
    };
    if !shell || new.preset.is_none() {
        return ApplyOutcome { text: applied(), changed: true, shown: new.preset.is_some() };
    }
    let expected = shell_current_path(io, ctx).unwrap_or_default();
    let mut answer = None;
    let mut waited = std::time::Duration::ZERO;
    while !expected.is_empty() {
        answer = shell_confirmation(io, ctx, &expected);
        if answer.is_some() || waited >= CONFIRM_WAIT {
            break;
        }
        sleep(CONFIRM_POLL);
        waited += CONFIRM_POLL;
    }
    let text = match answer {
        Some(Confirm::Shown) => return ApplyOutcome { text: applied(), changed: true, shown: true },
        Some(Confirm::Failed(detail)) => trf(MSG_SHELL_EFFECT_FAILED, &[&detail]),
        None => trf(MSG_SHELL_NO_ANSWER, &[&crate::wallpaper_engine::shell_ext_place(&ctx.de)]),
    };
    crate::log_info!("screen-effects", "эффект не подтверждён расширением ({expected}): {text}");
    if let Some(step) = shell_set_key_step(io, ctx, "") {
        let _ = run_steps(io, ctx, &[step]);
    }
    ApplyOutcome { text, changed: true, shown: false }
}

/// The window's «last one wins» action queue: while one action is running,
/// new ones do not pile up — each next one REPLACES the waiting one, and after the current one
/// only the freshest is executed. Needed by the section's worker thread
/// (`rexon-gui::screen_effects_actions`): fast clicks on «Apply»,
/// «Revert» and the trial rollback do not queue up at the tail.
pub struct LatestQueue<T> {
    inner: std::sync::Mutex<(bool, Option<T>)>,
}

impl<T> Default for LatestQueue<T> {
    fn default() -> Self {
        LatestQueue { inner: std::sync::Mutex::new((false, None)) }
    }
}

impl<T> LatestQueue<T> {
    /// Put an action. `true` — there is no worker thread, one must be started
    /// (and it will take this action with [`LatestQueue::take`]); `false` — the thread
    /// is already running and will take the action itself when it finishes the current one.
    pub fn push(&self, item: T) -> bool {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.1 = Some(item);
        if g.0 {
            false
        } else {
            g.0 = true;
            true
        }
    }

    /// For the worker thread: the next action or `None` — no work, the
    /// thread ends (and the queue remembers that).
    pub fn take(&self) -> Option<T> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let next = g.1.take();
        if next.is_none() {
            g.0 = false;
        }
        next
    }

    /// Whether an action is running (or waiting).
    pub fn busy(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).0
    }
}

/// Preset by name (`""` — turn off), strength and speed → settings.
pub fn settings_from_ui(preset: &str, strength: f64, speed: f64) -> EffectSettings {
    EffectSettings { preset: Preset::from_id(preset), params: EffectParams { strength, speed } }
}

/// «Apply» from the window: preset by name (`""` — turn off), strength and speed.
/// The reply is a ready phrase for the popup message.
pub fn apply_from_ui(io: &dyn LookIo, ctx: &LookCtx, preset: &str, strength: f64, speed: f64) -> String {
    apply(io, ctx, &settings_from_ui(preset, strength, speed))
}

/// Preview shader for the window (QML `ShaderEffect`): all presets in one
/// file, selected by the `rx_preset` uniform (the number in [`Preset::ALL`]); strength
/// `rx_s`, window mode `rx_window`, time `rx_time` (already multiplied by the
/// speed) and size `rx_res` are set by the tile. The dialect is GLSL 4.40 for Qt RHI;
/// the window gets it compiled by `qsb` (`rexon-gui/build.rs`). The file
/// [`PREVIEW_SHADER_REL`] is checked against this generator by a test.
pub fn preview_glsl() -> String {
    let mut s = String::new();
    s.push_str("#version 440\n");
    s.push_str("// Rexon screen effects preview: generated by rexon_core::screen_effects::preview_glsl (Rexon, Apache-2.0).\n");
    s.push_str("// Do not edit by hand: REXON_WRITE_PREVIEW_SHADER=1 cargo test -p rexon-core preview_shader_is_current\n");
    s.push_str("layout(location = 0) in vec2 qt_TexCoord0;\n");
    s.push_str("layout(location = 0) out vec4 fragColor;\n");
    s.push_str("layout(std140, binding = 0) uniform buf {\n");
    // Uniform names are ShaderEffect properties, and a QML property cannot
    // start with a capital letter: the core calls them RX_S/RX_WINDOW — we substitute.
    s.push_str("    mat4 qt_Matrix;\n    float qt_Opacity;\n    float rx_preset;\n    float rx_s;\n    float rx_window;\n    float rx_time;\n    vec2 rx_res;\n};\n");
    s.push_str("#define RX_S rx_s\n#define RX_WINDOW rx_window\n");
    s.push_str("layout(binding = 1) uniform sampler2D source;\n");
    s.push_str("vec4 rx_tex(vec2 uv) {\n    return texture(source, uv);\n}\n");
    s.push_str(CORE_HELPERS);
    for (i, p) in Preset::ALL.iter().enumerate() {
        s.push_str(&p.body().replace("vec4 rx_effect(", &format!("vec4 rx_effect_{i}(")));
    }
    s.push_str("\nvoid main() {\n    vec2 uv = qt_TexCoord0;\n    vec4 c;\n");
    for i in 0..Preset::ALL.len() {
        let call = format!("c = rx_effect_{i}(uv, rx_res, rx_time);");
        if i == 0 {
            s.push_str(&format!("    if (rx_preset < 0.5) {{\n        {call}\n"));
        } else if i + 1 < Preset::ALL.len() {
            s.push_str(&format!("    }} else if (rx_preset < {}.5) {{\n        {call}\n", i));
        } else {
            s.push_str(&format!("    }} else {{\n        {call}\n    }}\n"));
        }
    }
    s.push_str("    fragColor = c * qt_Opacity;\n}\n");
    s
}

/// Where the preview shader lives (relative to the `rust/` root).
pub const PREVIEW_SHADER_REL: &str = "rexon-gui/qml/shaders/screen-effects.frag";

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// The selection in «Current environment» beats the running one: in a KDE session
    /// with Wayfire selected the explanation is about Wayfire, not KWin.
    #[test]
    fn chosen_desktop_drives_the_note() {
        let mut kde = LookCtx::new("kde", "/h", None);
        kde.wayland = true;
        let wf = ctx_for(kde.clone(), "wayfire");
        assert_eq!((wf.de.as_str(), wf.wayland), ("wayfire", true));
        assert_eq!(support(&wf.de, wf.wayland).note, support("wayfire", true).note);
        assert_ne!(support(&wf.de, wf.wayland).note, support("kde", true).note);
        // MATE is an X11 session: advice about picom, not «no Wayland».
        let mate = ctx_for(kde.clone(), "mate");
        assert!(!mate.wayland);
        for same in ["", "auto", "kde", "  "] {
            let c = ctx_for(kde.clone(), same);
            assert_eq!((c.de.as_str(), c.wayland), ("kde", true), "{same:?}");
        }
    }

    // ── generator ──────────────────────────────────────────────────────────

    fn params(strength: f64, speed: f64) -> EffectParams {
        EffectParams { strength, speed }
    }

    fn count(hay: &str, needle: &str) -> usize {
        hay.matches(needle).count()
    }

    /// Every wrapper of every preset: the version on the first line, the entry point,
    /// uniforms, a single `rx_effect`, the needed texture call, balanced
    /// brackets, time — only for moving ones.
    #[test]
    fn every_host_every_preset() {
        for host in Host::ALL {
            for preset in Preset::ALL {
                for speed in [0.0, 1.0] {
                    let sh = generate(host, preset, params(0.7, speed));
                    let src = &sh.source;
                    let all = format!("{src}{}", sh.cogl_code.clone().unwrap_or_default());
                    let tag = format!("{host:?}/{preset:?}/{speed}");
                    assert_eq!(count(src, "vec4 rx_effect("), 1, "{tag}");
                    assert_eq!(count(src, "vec4 rx_tex("), 1, "{tag}");
                    assert_eq!(count(&all, "{"), count(&all, "}"), "{tag}");
                    assert_eq!(count(&all, "("), count(&all, ")"), "{tag}");
                    assert_eq!(count(src, "#version"), usize::from(host != Host::Cogl), "{tag}");
                    if host != Host::Cogl {
                        assert!(src.starts_with("#version"), "{tag}");
                    }
                    let want_anim = preset.animated() && speed > 0.0 && host.has_time();
                    assert_eq!(sh.animated, want_anim, "{tag}");
                    assert_eq!(sh.window_mode, host.per_window(), "{tag}");
                    assert!(src.contains(&format!("RX_WINDOW = {}", if host.per_window() { "1.0" } else { "0.0" })));
                    assert!(src.contains("const float RX_S = 0.7;"), "{tag}");
                    match host {
                        Host::Cogl => {
                            let code = sh.cogl_code.as_deref().unwrap();
                            assert!(code.starts_with("cogl_color_out = rx_effect(cogl_tex_coord_in[0].xy, rx_res,"));
                            assert!(src.contains("uniform vec2 rx_res;") && src.contains("texture2D(rx_sampler"));
                            assert!(!src.contains("texture("), "{tag}");
                            assert_eq!(src.contains("uniform float rx_time;"), want_anim, "{tag}");
                            assert_eq!(code.contains("rx_time"), want_anim, "{tag}");
                        }
                        Host::Hyprland => {
                            assert!(src.starts_with("#version 300 es\n"));
                            assert!(src.contains("in vec2 v_texcoord;") && src.contains("uniform sampler2D tex;"));
                            assert!(src.contains("void main()") && src.contains("fragColor = rx_effect(v_texcoord"));
                            assert_eq!(src.contains("uniform float time;"), want_anim, "{tag}");
                        }
                        Host::Picom => {
                            assert!(src.starts_with("#version 330\n"));
                            assert!(src.contains("vec4 window_shader()") && src.contains("default_post_processing("));
                            assert!(src.contains("texcoord / res"), "{tag}");
                            assert!(!src.contains("void main"), "{tag}");
                            assert_eq!(src.contains("uniform float time;"), want_anim, "{tag}");
                            if want_anim {
                                assert!(src.contains("time / 1000.0"), "{tag}");
                            }
                        }
                        Host::Wayfire => {
                            assert!(src.starts_with("#version 300 es\n@builtin_ext@\n@builtin@\n"));
                            assert!(src.contains("get_pixel(uv)") && src.contains("uniform float progress;"));
                            assert!(src.contains("out_color = mix(get_pixel(uvpos)"));
                            assert!(!src.contains("time"), "{tag}");
                        }
                        Host::KWin => {
                            assert!(src.starts_with("#version 140\n"));
                            assert!(src.contains("uniform sampler2D sampler;") && src.contains("in vec2 texcoord0;"));
                            assert!(src.contains("* modulation;"));
                            assert!(!src.contains("time"), "{tag}");
                        }
                    }
                    if host != Host::Cogl {
                        assert!(!src.contains("texture2D"), "{tag}");
                    }
                    assert_eq!(sh.uniforms.iter().any(|u| u.contains("time")), want_anim, "{tag}");
                }
            }
        }
    }

    /// The core does not touch the texture bypassing `rx_tex` and does not write integer literals
    /// where GLSL 1.10 / ES 1.00 does not convert them to float.
    #[test]
    fn core_is_portable() {
        for preset in Preset::ALL {
            let core = core_glsl(preset, EffectParams::default(), false);
            for bad in ["texture(", "texture2D(", "textureSize(", "uniform ", "#version", "precision "] {
                assert!(!core.contains(bad), "{preset:?}: {bad}");
            }
            assert!(!core.contains("RX_TINT_VALUE"), "{preset:?}");
            // Numbers in expressions — only with a dot (apart from the index there is not a single integer).
            for tok in core.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_')) {
                if tok.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    assert!(tok.contains('.'), "{preset:?}: целый литерал «{tok}»");
                }
            }
        }
    }

    #[test]
    fn params_are_clamped() {
        let p = params(7.0, -1.0).clamped();
        assert_eq!((p.strength, p.speed), (1.0, 0.0));
        let p = params(f64::NAN, f64::INFINITY).clamped();
        assert_eq!((p.strength, p.speed), (DEFAULT_STRENGTH, DEFAULT_SPEED));
        assert_eq!(glsl_float(1.0), "1.0");
        assert_eq!(glsl_float(0.25), "0.25");
        assert_eq!(glsl_float(0.0), "0.0");
        for p in Preset::ALL {
            assert_eq!(Preset::from_id(p.id()), Some(p));
        }
    }

    /// Compilation with a real glslangValidator (if installed): each
    /// environment with its own stubs for what the host substitutes.
    #[test]
    fn compiles_with_glslang() {
        let Some(tool) = crate::utils::which("glslangValidator", std::env::var("PATH").ok().as_deref()) else {
            eprintln!("glslangValidator нет — пропуск");
            return;
        };
        let dir = std::env::temp_dir().join(format!("rexon-se-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut failures = Vec::new();
        for host in Host::ALL {
            for preset in Preset::ALL {
                for speed in [0.0, 1.0] {
                    let sh = generate(host, preset, params(0.7, speed));
                    let text = match host {
                        // Cogl assembles the snippet inside its own shader: we give
                        // it GLSL 1.10 and the Cogl template variables.
                        Host::Cogl => format!(
                            "#version 110\nvarying vec4 cogl_tex_coord_in[1];\nvec4 cogl_color_out;\n{}\nvoid main() {{\n{}gl_FragColor = cogl_color_out;\n}}\n",
                            sh.source,
                            sh.cogl_code.unwrap()
                        ),
                        Host::Wayfire => sh.source.replace("@builtin_ext@\n", "").replace(
                            "@builtin@\n",
                            "uniform sampler2D _wf_tex;\nmediump vec4 get_pixel(mediump vec2 uv) { return texture(_wf_tex, uv); }\n",
                        ),
                        _ => sh.source.clone(),
                    };
                    let path = dir.join(format!("{}-{}-{speed}.frag", host.id(), preset.id()));
                    std::fs::write(&path, &text).unwrap();
                    let out = std::process::Command::new(&tool).arg(&path).output().unwrap();
                    if !out.status.success() {
                        failures.push(format!("{}: {}", path.display(), String::from_utf8_lossy(&out.stdout)));
                    }
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    // ── support table ──────────────────────────────────────────────────────

    #[test]
    fn support_table_is_honest() {
        crate::i18n::set_language(Some("en"));
        let cov = |de: &str, w: bool| support(de, w).coverage;
        assert_eq!(cov("gnome", true), Coverage::Screen);
        assert_eq!(cov("cinnamon-mint", false), Coverage::Screen);
        assert_eq!(cov("hyprland", true), Coverage::Screen);
        assert_eq!(cov("wayfire", true), Coverage::Screen);
        assert_eq!(cov("kde", true), Coverage::Windows);
        for de in ["mate", "xfce", "lxqt"] {
            assert_eq!(cov(de, false), Coverage::Windows);
            assert_eq!(support(de, false).host, Some(Host::Picom));
            assert!(!support(de, false).needs.is_empty());
            assert_eq!(cov(de, true), Coverage::None);
        }
        assert_eq!(cov("pantheon", true), Coverage::MonochromeOnly);
        assert_eq!(cov("cosmic", true), Coverage::MonochromeOnly);
        assert!(support("kde", true).native_monochrome);
        for de in ["sway", "river", "niri", "budgie", "labwc", "who-knows"] {
            assert_eq!(cov(de, true), Coverage::None, "{de}");
            assert!(support(de, true).host.is_none());
        }
        assert_eq!(cov("deepin", true), Coverage::None);
        assert_eq!(cov("deepin", false), Coverage::None);
        assert!(!KNOWN_DESKTOPS.contains(&("deepin", false)), "Deepin X11 Rexon не поддерживает");
        // Time exists only where the host provides it.
        assert!(support("hyprland", true).animated && support("gnome", true).animated);
        assert!(!support("kde", true).animated && !support("wayfire", true).animated);
        // Applied: GNOME and Cinnamon (our own extensions), Hyprland.
        let ready: Vec<_> = support_table().into_iter().filter(|s| s.ready).map(|s| s.de).collect();
        assert_eq!(ready, vec!["gnome".to_string(), "cinnamon-mint".to_string(), "hyprland".to_string()]);
        for s in support_table() {
            assert!(!s.note.is_empty(), "{}", s.de);
            // On per-window methods curvature is off — and the explanation says so.
            if s.coverage == Coverage::Windows {
                assert!(s.host.unwrap().per_window());
                assert!(s.note.contains("curvature"), "{}", s.note);
            }
        }
        assert!(Preset::Crt.warps() && Preset::CrtGreen.warps() && !Preset::Scanlines.warps());
    }

    // ── Hyprland ───────────────────────────────────────────────────────────

    #[derive(Default)]
    struct FakeIo {
        files: RefCell<HashMap<String, String>>,
        ran: RefCell<Vec<Vec<String>>>,
    }

    impl LookIo for FakeIo {
        fn run(&self, a: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(a.to_vec());
            (true, String::new())
        }
        fn have(&self, _: &str) -> bool {
            true
        }
        fn read(&self, p: &str) -> Option<String> {
            self.files.borrow().get(p).cloned()
        }
        fn write(&self, p: &str, t: &str) -> bool {
            self.files.borrow_mut().insert(p.into(), t.into());
            true
        }
        fn list_dir(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }

    fn hypr() -> LookCtx {
        LookCtx::new("hyprland", "/h", Some("/h/.config"))
    }

    fn on(preset: Preset, strength: f64, speed: f64) -> EffectSettings {
        EffectSettings { preset: Some(preset), params: params(strength, speed) }
    }

    fn run(io: &FakeIo, c: &LookCtx, s: &EffectSettings) {
        let steps = plan_settings(io, c, s).unwrap();
        run_steps(io, c, &steps).unwrap();
    }

    #[test]
    fn hyprland_lua_on_change_off() {
        crate::i18n::set_language(Some("en"));
        let io = FakeIo::default();
        let c = hypr();
        let mine = "-- mine\nhl.config({ general = { gaps_in = 5 } })\n";
        io.files.borrow_mut().insert("/h/.config/hypr/hyprland.lua".into(), mine.into());

        // Static CRT: a shader without time, damage tracking is left alone.
        run(&io, &c, &on(Preset::Crt, 0.5, 1.0));
        let shader = io.files.borrow()["/h/.config/rexon/screen-effects/hyprland.frag"].clone();
        assert!(shader.starts_with("#version 300 es") && !shader.contains("uniform float time"), "{shader}");
        let own = io.files.borrow()["/h/.config/hypr/rexon-screen-effects.lua"].clone();
        assert!(
            own.contains("hl.config({ decoration = { screen_shader = \"/h/.config/rexon/screen-effects/hyprland.frag\" } })"),
            "{own}"
        );
        assert!(!own.contains("damage_tracking"), "{own}");
        let cfg = io.files.borrow()["/h/.config/hypr/hyprland.lua"].clone();
        assert!(cfg.starts_with(mine) && cfg.contains("pcall(require, \"rexon-screen-effects\")"), "{cfg}");
        assert!(io.ran.borrow().iter().any(|a| a == &argv(&["hyprctl", "reload"])));
        let s = read_settings(&io, &c);
        assert_eq!(s.preset, Some(Preset::Crt));
        assert_eq!(s.params.strength, 0.5);

        // A repeat — an empty plan.
        assert!(plan_settings(&io, &c, &on(Preset::Crt, 0.5, 1.0)).unwrap().is_empty());

        // Moving cassette: time in the shader, damage tracking off.
        run(&io, &c, &on(Preset::Vhs, 0.8, 2.0));
        let shader = io.files.borrow()["/h/.config/rexon/screen-effects/hyprland.frag"].clone();
        assert!(shader.contains("uniform float time;") && shader.contains("time * 2.0"), "{shader}");
        let own = io.files.borrow()["/h/.config/hypr/rexon-screen-effects.lua"].clone();
        assert!(own.contains("hl.config({ debug = { damage_tracking = 0 } })"), "{own}");
        assert!(own.contains("preset=vhs") && own.contains("speed=2") && own.contains("strength=0.8"), "{own}");
        // One block, it does not multiply.
        let cfg = io.files.borrow()["/h/.config/hypr/hyprland.lua"].clone();
        assert_eq!(cfg.matches(">>> Rexon: screen-effects").count(), 1, "{cfg}");

        // Speed 0 — again without time and without load.
        run(&io, &c, &on(Preset::Vhs, 0.8, 0.0));
        let own = io.files.borrow()["/h/.config/hypr/rexon-screen-effects.lua"].clone();
        assert!(!own.contains("damage_tracking"), "{own}");

        // Turning off: the person's config is byte for byte the same as before.
        run(&io, &c, &EffectSettings { preset: None, params: params(0.8, 0.0) });
        assert_eq!(io.files.borrow()["/h/.config/hypr/hyprland.lua"], mine);
        let s = read_settings(&io, &c);
        assert_eq!(s.preset, None);
        assert_eq!(s.params.strength, 0.8, "ползунки помнятся");
        // Turning off again — nothing.
        assert!(plan_settings(&io, &c, &EffectSettings::default()).unwrap().is_empty());
    }

    #[test]
    fn hyprland_conf_and_refusals() {
        crate::i18n::set_language(Some("en"));
        let io = FakeIo::default();
        let c = hypr();
        let mine = "monitor = , preferred, auto, 1\n";
        io.files.borrow_mut().insert("/h/.config/hypr/hyprland.conf".into(), mine.into());
        run(&io, &c, &on(Preset::Film, 0.6, 1.0));
        let own = io.files.borrow()["/h/.config/hypr/rexon-screen-effects.conf"].clone();
        assert!(own.contains("decoration {\n    screen_shader = /h/.config/rexon/screen-effects/hyprland.frag\n}"), "{own}");
        assert!(own.contains("debug {\n    damage_tracking = 0\n}"), "{own}");
        let cfg = io.files.borrow()["/h/.config/hypr/hyprland.conf"].clone();
        assert!(cfg.contains("source = /h/.config/hypr/rexon-screen-effects.conf"), "{cfg}");
        run(&io, &c, &EffectSettings::default());
        assert_eq!(io.files.borrow()["/h/.config/hypr/hyprland.conf"], mine);

        // There is no Hyprland config — an honest phrase, nothing is created.
        let io = FakeIo::default();
        let steps = plan_settings(&io, &c, &on(Preset::Crt, 0.5, 1.0)).unwrap();
        let err = run_steps(&io, &c, &steps).unwrap_err();
        assert!(err.contains("Hyprland"), "{err}");
        assert!(!io.files.borrow().contains_key("/h/.config/hypr/hyprland.conf"));

        // An environment where it is not possible yet — the explanation from the table.
        let k = LookCtx::new("sway", "/h", Some("/h/.config"));
        assert_eq!(plan_settings(&io, &k, &on(Preset::Crt, 0.5, 1.0)).unwrap_err(), support("sway", true).note);
        assert_eq!(apply(&io, &k, &on(Preset::Crt, 0.5, 1.0)), support("sway", true).note);
    }

    // ── window ─────────────────────────────────────────────────────────────

    /// The preview shader file in the tree matches the generator.
    /// Regenerate: `REXON_WRITE_PREVIEW_SHADER=1 cargo test -p rexon-core preview_shader_is_current`.
    #[test]
    fn preview_shader_is_current() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(PREVIEW_SHADER_REL);
        let want = preview_glsl();
        if std::env::var_os("REXON_WRITE_PREVIEW_SHADER").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(have == want, "{} устарел: REXON_WRITE_PREVIEW_SHADER=1 cargo test -p rexon-core preview_shader_is_current", path.display());
        assert_eq!(count(&want, "rx_effect_"), 2 * Preset::ALL.len());
        assert_eq!(count(&want, "{"), count(&want, "}"));
        assert!(!want.contains("RX_TINT_VALUE") && !want.contains("const float RX_S"));
    }

    /// The preview shader is compiled by glslangValidator (if installed) as
    /// Vulkan GLSL — the same dialect qsb accepts.
    #[test]
    fn preview_shader_compiles() {
        let Some(tool) = crate::utils::which("glslangValidator", std::env::var("PATH").ok().as_deref()) else {
            eprintln!("glslangValidator нет — пропуск");
            return;
        };
        let dir = std::env::temp_dir().join(format!("rexon-se-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("preview.frag");
        std::fs::write(&path, preview_glsl()).unwrap();
        let out = std::process::Command::new(&tool).arg("-G").arg(&path).arg("-o").arg(dir.join("p.spv")).output().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    }

    /// Window snapshot: Hyprland — with «Apply» and the enabled preset; sway —
    /// without the button, with an explanation; tiles — all presets in order.
    #[test]
    fn window_state_and_apply() {
        crate::i18n::set_language(Some("en"));
        let io = FakeIo::default();
        let c = hypr();
        io.files.borrow_mut().insert("/h/.config/hypr/hyprland.conf".into(), "# mine\n".into());
        let st = read_state(&io, &c);
        assert!(st.can_apply && st.preset.is_empty() && st.moving && !st.per_window);
        assert_eq!(st.presets.iter().map(|t| t.id).collect::<Vec<_>>(), Preset::ALL.map(Preset::id));
        assert!(st.presets.iter().enumerate().all(|(i, t)| t.index == i));
        assert_eq!(apply_from_ui(&io, &c, "crt-amber", 0.4, 0.0), "Screen effects applied.");
        let st = read_state(&io, &c);
        assert_eq!((st.preset.as_str(), st.strength, st.speed), ("crt-amber", 0.4, 0.0));
        assert_eq!(apply_from_ui(&io, &c, "", 0.4, 0.0), "Screen effects applied.");
        assert_eq!(io.files.borrow()["/h/.config/hypr/hyprland.conf"], "# mine\n");
        assert!(read_state(&io, &c).preset.is_empty());

        let k = LookCtx::new("kde", "/h", Some("/h/.config"));
        let st = read_state(&io, &k);
        // KDE: no own shader yet — only the stock black-and-white filter.
        assert!(st.can_apply && st.monochrome_only && !st.trial && st.has_strength);
        assert!(st.per_window && !st.moving && st.note.contains("KWin"));
        assert_eq!(st.presets.iter().map(|t| (t.id, t.index)).collect::<Vec<_>>(), vec![("monochrome", 7)]);
        assert!(read_state(&io, &c).trial, "свой шейдер — на пробу");
        let sway = read_state(&io, &LookCtx::new("sway", "/h", Some("/h/.config")));
        assert_eq!(sway.coverage, Coverage::None);
        assert!(!sway.can_apply);
    }

    /// A shader path with a quote and a backslash does not break the Lua string.
    #[test]
    fn lua_path_is_escaped() {
        assert_eq!(lua_string(r#"/home/a"b\c"#), r#""/home/a\"b\\c""#);
    }
}

#[cfg(test)]
mod shell_ext_tests {
    //! GNOME and Cinnamon: the write plan for the extension and the contract with its JS.
    use super::*;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};

    const GNOME_JS: &str = include_str!("../../../data/gnome-extension/rexon-wallpaper@rexon.app/extension.js");
    const CINNAMON_JS: &str = include_str!("../../../data/cinnamon-extension/rexon-wallpaper@rexon.app/extension.js");
    const GNOME_SCHEMA: &str = include_str!(
        "../../../data/gnome-extension/rexon-wallpaper@rexon.app/schemas/org.gnome.shell.extensions.rexon-wallpaper.gschema.xml"
    );
    const CINNAMON_SCHEMA: &str = include_str!(
        "../../../data/cinnamon-extension/rexon-wallpaper@rexon.app/schemas/org.cinnamon.extensions.rexon-wallpaper.gschema.xml"
    );

    /// Fake machine: files and gsettings in memory, the shell's reply about the
    /// extension state is configurable.
    #[derive(Default)]
    struct ShellIo {
        files: RefCell<HashMap<String, String>>,
        gs: RefCell<HashMap<String, String>>,
        dirs: HashMap<String, Vec<String>>,
        tools: HashSet<&'static str>,
        /// Reply of `gdbus call … GetExtensionInfo` (GNOME).
        gdbus: Option<String>,
        /// Cinnamon's `GetRunningXletUUIDs` reply (`None` — not answering).
        running: Option<String>,
        /// How the extension answers a change of the effect key: `state=` in the reply
        /// file (`None` — silent, like a non-working one).
        answer: Option<&'static str>,
        removed: RefCell<Vec<String>>,
        ran: RefCell<Vec<Vec<String>>>,
    }

    impl LookIo for ShellIo {
        fn run(&self, a: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(a.to_vec());
            match a[0].as_str() {
                "gsettings" => {
                    let a: Vec<&String> = if a[1] == "--schemadir" { a[3..].iter().collect() } else { a[1..].iter().collect() };
                    let k = format!("{} {}", a[1], a[2]);
                    let mut gs = self.gs.borrow_mut();
                    match (a[0].as_str(), gs.get(&k).cloned()) {
                        ("get", Some(v)) => (true, v),
                        ("set", Some(_)) => {
                            gs.insert(k.clone(), a[3].clone());
                            // The extension answers a change of the effect key.
                            let path = parse_gvariant_str(a[3]);
                            if let (true, Some(state), false) = (k.ends_with(SHELL_EFFECT_KEY), self.answer, path.is_empty()) {
                                let ctx = LookCtx::new("gnome", "/home/u", None);
                                self.files
                                    .borrow_mut()
                                    .insert(shell_status_path(&ctx), format!("state={state}\npath={path}\ndetail=compile\n"));
                            }
                            (true, String::new())
                        }
                        _ => (false, "No such key".to_string()),
                    }
                }
                "gdbus" if a.iter().any(|x| x == "org.Cinnamon.ReloadXlet") => (true, "()".to_string()),
                "gdbus" if a.iter().any(|x| x == "org.Cinnamon.GetRunningXletUUIDs") => match &self.running {
                    Some(out) => (true, out.clone()),
                    None => (false, String::new()),
                },
                "gdbus" => match &self.gdbus {
                    Some(out) => (true, out.clone()),
                    None => (false, String::new()),
                },
                _ => (true, String::new()),
            }
        }
        fn have(&self, tool: &str) -> bool {
            self.tools.contains(tool)
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            self.files.borrow_mut().insert(path.to_string(), text.to_string());
            true
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            self.dirs.get(path).cloned().unwrap_or_default()
        }
        fn remove_tree(&self, path: &str) -> bool {
            self.removed.borrow_mut().push(path.to_string());
            self.files.borrow_mut().retain(|k, _| !k.starts_with(&format!("{path}/")));
            true
        }
    }

    const GNOME_SCHEMA_ID: &str = "org.gnome.shell.extensions.rexon-wallpaper";
    const CINNAMON_SCHEMA_ID: &str = "org.cinnamon.extensions.rexon-wallpaper";
    const GNOME_ON: &str = "({'uuid': <'rexon-wallpaper@rexon.app'>, 'state': <1.0>},)";
    const GNOME_OFF: &str = "({'uuid': <'rexon-wallpaper@rexon.app'>, 'state': <2.0>},)";
    const GNOME_NOT_LOADED: &str = "(@a{sv} {},)";

    fn ctx(de: &str) -> LookCtx {
        LookCtx::new(de, "/home/u", None)
    }

    /// A machine with the extension installed (the schema compiled in the user's
    /// extension directory) and an empty effect key.
    fn machine(de: &str, gdbus: Option<&str>) -> ShellIo {
        let c = ctx(de);
        let sub = if de == "gnome" { "gnome-shell" } else { "cinnamon" };
        let dir = format!("{}/{sub}/extensions/rexon-wallpaper@rexon.app/schemas", c.data_home);
        let schema = if de == "gnome" { GNOME_SCHEMA_ID } else { CINNAMON_SCHEMA_ID };
        let io = ShellIo {
            dirs: HashMap::from([(dir, vec!["gschemas.compiled".to_string()])]),
            tools: HashSet::from(["gsettings", "gdbus", "gnome-extensions"]),
            gdbus: gdbus.map(str::to_string),
            answer: Some("on"),
            ..Default::default()
        };
        io.gs.borrow_mut().insert(format!("{schema} {SHELL_EFFECT_KEY}"), "''".to_string());
        io.gs.borrow_mut().insert("org.cinnamon enabled-extensions".to_string(), "['rexon-wallpaper@rexon.app']".to_string());
        io
    }

    fn on(preset: Preset) -> EffectSettings {
        EffectSettings { preset: Some(preset), params: EffectParams { strength: 0.5, speed: 1.0 } }
    }

    fn key(io: &ShellIo, schema: &str) -> String {
        parse_gvariant_str(&io.gs.borrow()[&format!("{schema} {SHELL_EFFECT_KEY}")])
    }

    fn apply_ok(io: &ShellIo, c: &LookCtx, s: &EffectSettings) {
        let steps = plan_settings(io, c, s).expect("план");
        run_steps(io, c, &steps).expect("исполнение");
    }

    #[test]
    fn cogl_file_splits_at_marker() {
        for preset in Preset::ALL {
            let shader = generate(Host::Cogl, preset, EffectParams::default());
            let text = cogl_file_content(&shader);
            assert_eq!(text.matches(COGL_CODE_MARKER).count(), 1, "{preset:?}");
            let (decl, code) = text.split_once(COGL_CODE_MARKER).unwrap();
            assert!(decl.contains("vec4 rx_effect(") && decl.contains("uniform vec2 rx_res;"));
            assert!(code.contains("cogl_color_out = rx_effect("));
            // The extension starts the clock by `rx_time` in the code — exactly for moving ones.
            assert_eq!(code.contains("rx_time"), shader.animated, "{preset:?}");
        }
    }

    /// Contract with JS: the key name and marker are the same, the shared effect block in both
    /// extensions is identical character for character, the schemas carry the key with an empty
    /// default value (effect off), changing the effect key does not
    /// restart the wallpaper.
    #[test]
    fn shell_extensions_share_screen_effect() {
        let block = |src: &str, de: &str| {
            let begin = "[rexon-wp:screen-effect:begin]";
            let from = src.find(begin).unwrap_or_else(|| panic!("{de}: нет начала блока эффекта")) + begin.len();
            let to = src[from..].find("[rexon-wp:screen-effect:end]").unwrap_or_else(|| panic!("{de}: нет конца блока")) + from;
            src[from..to].to_string()
        };
        let g = block(GNOME_JS, "GNOME");
        let c = block(CINNAMON_JS, "Cinnamon");
        assert_eq!(g, c, "блоки эффекта экрана в расширениях GNOME и Cinnamon разошлись");
        assert!(g.contains(&format!("const EFFECT_KEY = '{SHELL_EFFECT_KEY}';")));
        assert!(g.contains(&format!("const EFFECT_CODE_MARKER = '{COGL_CODE_MARKER}';")));
        for (de, src, schema) in [("GNOME", GNOME_JS, GNOME_SCHEMA), ("Cinnamon", CINNAMON_JS, CINNAMON_SCHEMA)] {
            assert!(src.contains("key === EFFECT_KEY"), "{de}: смена эффекта перезапускала бы обои");
            assert!(src.contains("new ScreenEffect(") && src.contains("target: Main.uiGroup"), "{de}");
            assert!(src.contains("effectShaderPath("), "{de}: ключ читается мимо проверки схемы");
            let at = schema.find(&format!("<key name=\"{SHELL_EFFECT_KEY}\" type=\"s\">")).unwrap_or_else(|| panic!("{de}: в схеме нет ключа"));
            assert!(schema[at..].contains("<default>\"\"</default>"), "{de}: эффект обязан быть выключен по умолчанию");
        }
    }

    #[test]
    fn gvariant_strings_round_trip() {
        for s in ["", "/home/u/.config/rexon/screen-effects/shell-a.glsl", "/дом/it's \\ x"] {
            assert_eq!(parse_gvariant_str(&gvariant_str(s)), s);
        }
        assert_eq!(parse_gvariant_str("@s ''\n"), "");
    }

    #[test]
    fn gnome_on_change_same_off() {
        let c = ctx("gnome");
        let io = machine("gnome", Some(GNOME_ON));
        let [a, b] = shell_shader_slots(&c);
        assert_eq!(read_settings(&io, &c).preset, None, "по умолчанию выключено");

        apply_ok(&io, &c, &on(Preset::Crt));
        assert_eq!(key(&io, GNOME_SCHEMA_ID), a);
        assert!(io.read(&a).unwrap().contains(COGL_CODE_MARKER));
        // Written through the schema from the extension directory.
        assert!(io.ran.borrow().iter().any(|r| r[0] == "gsettings" && r[1] == "--schemadir" && r.contains(&"set".to_string())));
        let now = read_settings(&io, &c);
        assert_eq!((now.preset, now.params.strength), (Some(Preset::Crt), 0.5));

        // Change: the new shader into the other file, the previous one stays intact until
        // the key is switched.
        let before_a = io.read(&a).unwrap();
        apply_ok(&io, &c, &on(Preset::Vhs));
        assert_eq!(key(&io, GNOME_SCHEMA_ID), b);
        assert_eq!(io.read(&a).unwrap(), before_a);
        assert!(io.read(&b).unwrap().contains("rx_time"));

        // The same once more — nothing to do.
        assert!(plan_settings(&io, &c, &on(Preset::Vhs)).unwrap().is_empty());

        // Turning off: the key is empty, strength and speed are remembered.
        let off = EffectSettings { preset: None, params: EffectParams { strength: 0.5, speed: 1.0 } };
        apply_ok(&io, &c, &off);
        assert_eq!(key(&io, GNOME_SCHEMA_ID), "");
        let now = read_settings(&io, &c);
        assert_eq!((now.preset, now.params.strength), (None, 0.5));
        assert!(plan_settings(&io, &c, &off).unwrap().is_empty());
    }

    #[test]
    fn turned_off_extension_is_turned_on() {
        let c = ctx("gnome");
        let io = machine("gnome", Some(GNOME_OFF));
        let steps = plan_settings(&io, &c, &on(Preset::Scanlines)).unwrap();
        assert!(steps.iter().any(|s| matches!(s, LookStep::Cmd(a) if a[..2] == ["gnome-extensions", "enable"])));

        let c = ctx("cinnamon-mint");
        let io = machine("cinnamon-mint", None);
        io.gs.borrow_mut().insert("org.cinnamon enabled-extensions".to_string(), "['other@x']".to_string());
        apply_ok(&io, &c, &on(Preset::Film));
        assert_eq!(io.gs.borrow()["org.cinnamon enabled-extensions"], "[\"other@x\",\"rexon-wallpaper@rexon.app\"]");
        assert_eq!(key(&io, CINNAMON_SCHEMA_ID), shell_shader_slots(&c)[0]);
        assert_eq!(read_settings(&io, &c).preset, Some(Preset::Film));
    }

    /// 15.11.2026, Cinnamon 6.8 Wayland: the extension is in the enabled list but
    /// does not work (shadowed by an orphaned copy from the home directory) —
    /// the copy is removed, the shell reloads the extension, the key is written
    /// through the schema of the SYSTEM copy.
    #[test]
    fn cinnamon_stale_user_copy_is_removed_and_reloaded() {
        let c = ctx("cinnamon-mint");
        let mut io = machine("cinnamon-mint", None);
        let id = crate::wallpaper_engine::SHELL_EXT_ID;
        let user = format!("{}/cinnamon/extensions/{id}", c.data_home);
        let system_schemas = format!("/usr/share/cinnamon/extensions/{id}/schemas");
        io.dirs.insert(system_schemas.clone(), vec!["gschemas.compiled".to_string()]);
        io.files.borrow_mut().insert(format!("{user}/extension.js"), "old".into());
        io.files.borrow_mut().insert(format!("/usr/share/cinnamon/extensions/{id}/extension.js"), "new".into());
        io.running = Some("(@as [],)".to_string());

        assert_eq!(orphan_user_copy(&io, &c).as_deref(), Some(user.as_str()));
        let steps = plan_settings(&io, &c, &on(Preset::Crt)).unwrap();
        assert!(steps.contains(&LookStep::RemoveTree { path: user.clone() }), "{steps:?}");
        assert!(steps.contains(&LookStep::Cmd(cinnamon_reload_argv())), "{steps:?}");
        let set = steps
            .iter()
            .find_map(|s| match s {
                LookStep::Cmd(a) if a.contains(&"set".to_string()) && a.contains(&SHELL_EFFECT_KEY.to_string()) => Some(a.clone()),
                _ => None,
            })
            .expect("ключ эффекта пишется");
        assert_eq!(set[2], system_schemas, "схема брошенной копии не годится: {set:?}");

        let out = apply_confirmed(&io, &c, &on(Preset::Crt), &|_| {});
        assert!(out.shown && out.changed, "{out:?}");
        assert_eq!(io.removed.borrow().as_slice(), [user.clone()]);

        // A running extension is left alone, even if the copy differs.
        let mut io = machine("cinnamon-mint", None);
        io.files.borrow_mut().insert(format!("{user}/extension.js"), "old".into());
        io.files.borrow_mut().insert(format!("/usr/share/cinnamon/extensions/{id}/extension.js"), "new".into());
        io.running = Some(format!("(['{id}'],)"));
        let steps = plan_settings(&io, &c, &on(Preset::Crt)).unwrap();
        assert!(!steps.iter().any(|s| matches!(s, LookStep::RemoveTree { .. }) || *s == LookStep::Cmd(cinnamon_reload_argv())));

        // Rexon is installed into the home directory — the copy is its own, not orphaned.
        let mut io = machine("cinnamon-mint", None);
        io.files.borrow_mut().insert(format!("{user}/extension.js"), "old".into());
        io.files.borrow_mut().insert(format!("/usr/share/cinnamon/extensions/{id}/extension.js"), "new".into());
        io.dirs.insert("/home/u/.local/lib/rexon".into(), vec!["rexon".into()]);
        assert_eq!(orphan_user_copy(&io, &c), None);
        assert_eq!(remove_orphan_user_copy(&io, &c), None);
        assert!(io.removed.borrow().is_empty(), "не брошенную не снимаем");

        // Shared point for wallpapers (Cinnamon with layer-shell): the same copy
        // is removed without an effect plan.
        let io = machine("cinnamon-mint", None);
        io.files.borrow_mut().insert(format!("{user}/extension.js"), "old".into());
        io.files.borrow_mut().insert(format!("/usr/share/cinnamon/extensions/{id}/extension.js"), "new".into());
        assert_eq!(remove_orphan_user_copy(&io, &c).as_deref(), Some(user.as_str()));
        assert_eq!(io.removed.borrow().as_slice(), [user.clone()]);
    }

    #[test]
    fn cinnamon_running_answer_parses() {
        let id = crate::wallpaper_engine::SHELL_EXT_ID;
        assert_eq!(parse_cinnamon_running("(@as [],)\n"), Some(false));
        assert_eq!(parse_cinnamon_running(&format!("(['a@b', '{id}'],)")), Some(true));
        assert_eq!(parse_cinnamon_running("Error: no such method"), None);
    }

    /// «Applied» — only on the extension's reply; if it is silent or could not do it —
    /// the key is reset (the effect will not pop up by itself at the next login), the phrase
    /// says why.
    #[test]
    fn apply_needs_extension_confirmation() {
        crate::i18n::set_language(Some("en"));
        let c = ctx("cinnamon-mint");
        let io = machine("cinnamon-mint", None);
        let out = apply_confirmed(&io, &c, &on(Preset::Crt), &|_| {});
        assert_eq!((out.text.as_str(), out.shown), ("Screen effects applied.", true));
        assert_eq!(key(&io, CINNAMON_SCHEMA_ID), shell_shader_slots(&c)[0]);

        // Silent: we waited no longer than CONFIRM_WAIT and reset the key.
        let mut io = machine("cinnamon-mint", None);
        io.answer = None;
        let slept = std::cell::Cell::new(std::time::Duration::ZERO);
        let out = apply_confirmed(&io, &c, &on(Preset::Crt), &|d| slept.set(slept.get() + d));
        assert!(!out.shown && out.text.contains("didn't confirm"), "{out:?}");
        assert!(slept.get() >= CONFIRM_WAIT && slept.get() <= CONFIRM_WAIT + CONFIRM_POLL, "{:?}", slept.get());
        assert_eq!(key(&io, CINNAMON_SCHEMA_ID), "");
        assert_eq!(read_settings(&io, &c).preset, None, "окно не покажет эффект «текущим»");

        // Could not apply — the reason in its own words.
        let mut io = machine("gnome", Some(GNOME_ON));
        io.answer = Some("error");
        let g = ctx("gnome");
        let out = apply_confirmed(&io, &g, &on(Preset::Vhs), &|_| {});
        assert!(!out.shown && out.text.contains("(compile)"), "{out:?}");
        assert_eq!(key(&io, GNOME_SCHEMA_ID), "");

        // A previous «enabled» in the reply file does not pass for the new one: before switching
        // the key it holds «pending» with the new path.
        let mut io = machine("cinnamon-mint", None);
        let [a, b] = shell_shader_slots(&c);
        io.files.borrow_mut().insert(shell_status_path(&c), format!("state=on\npath={a}\n"));
        io.answer = None;
        let out = apply_confirmed(&io, &c, &on(Preset::Film), &|_| {});
        assert!(!out.shown, "{out:?}");

        // The same shader, but the extension did not confirm it — the key is switched
        // anyway so the extension notices the change.
        let io = machine("cinnamon-mint", None);
        apply_ok(&io, &c, &on(Preset::Crt));
        io.files.borrow_mut().insert(shell_status_path(&c), "state=error\npath=x\n".into());
        let steps = plan_settings(&io, &c, &on(Preset::Crt)).unwrap();
        assert!(steps.iter().any(|s| matches!(s, LookStep::WriteFile { path, .. } if *path == b)), "{steps:?}");
    }

    /// The window queue: putting an action is instant while the current one runs;
    /// five fast presses during an action give ONE next action — the
    /// freshest, not five at the tail.
    #[test]
    fn latest_queue_never_blocks_and_keeps_only_last() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        let q: Arc<LatestQueue<u32>> = Arc::default();
        let done: Arc<Mutex<Vec<u32>>> = Arc::default();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let worker = |q: Arc<LatestQueue<u32>>, done: Arc<Mutex<Vec<u32>>>| {
            std::thread::spawn(move || {
                while let Some(n) = q.take() {
                    let _ = started_tx.send(n);
                    std::thread::sleep(Duration::from_millis(200));
                    done.lock().unwrap().push(n);
                }
            })
        };
        assert!(q.push(1), "первое действие заводит поток");
        let h = worker(q.clone(), done.clone());
        // Presses happen while «1» is already running (otherwise «6» would honestly replace
        // the not yet taken «1»).
        assert_eq!(started_rx.recv_timeout(Duration::from_secs(5)), Ok(1));
        let t = Instant::now();
        for n in 2..=6 {
            assert!(!q.push(n), "поток уже идёт");
        }
        assert!(t.elapsed() < Duration::from_millis(50), "нажатие ждало действие: {:?}", t.elapsed());
        assert!(q.busy());
        h.join().unwrap();
        assert_eq!(*done.lock().unwrap(), vec![1, 6]);
        assert!(!q.busy());
        assert!(q.push(7), "после конца — снова новый поток");
    }

    /// Window markup: «Apply», «Revert» and the trial rollback
    /// go through the worker thread's queue, not a synchronous call
    /// (the window hung on 15.11.2026), and the buttons are inactive during an action.
    #[test]
    fn window_actions_go_through_worker() {
        const QML: &str = include_str!("../../rexon-gui/qml/ScreenEffectsSection.qml");
        const ACTIONS: &str = include_str!("../../rexon-gui/src/screen_effects_actions.rs");
        assert!(!QML.contains("actions.apply("), "синхронный вызов записи из QML");
        assert!(QML.matches("actions.request(").count() >= 3, "Применить, Вернуть как было, откат пробы");
        for button in ["screenEffectsApply", "screenEffectsRestore", "screenEffectsKeep", "screenEffectsRevert"] {
            let at = QML.find(&format!("objectName: \"{button}\"")).unwrap_or_else(|| panic!("нет кнопки {button}"));
            let body = &QML[at..at + QML[at..].find("onClicked").unwrap()];
            assert!(body.contains("!section.busy"), "{button} активна во время действия");
        }
        assert!(ACTIONS.contains("std::thread::spawn") && ACTIONS.contains("apply_confirmed"));
        assert!(ACTIONS.contains("LatestQueue"));
    }

    #[test]
    fn refusals_write_nothing() {
        // The extension is not loaded (GNOME before re-login) — an honest refusal.
        let c = ctx("gnome");
        let io = machine("gnome", Some(GNOME_NOT_LOADED));
        let err = plan_settings(&io, &c, &on(Preset::Crt)).unwrap_err();
        assert!(err.contains("Log out"), "{err}");
        assert!(io.files.borrow().is_empty());

        // Extension from an older version: the schema has no effect key.
        let io = machine("gnome", Some(GNOME_ON));
        io.gs.borrow_mut().clear();
        let err = plan_settings(&io, &c, &on(Preset::Crt)).unwrap_err();
        assert!(err.contains("older version"), "{err}");
        // And there is nothing to turn off there — no error.
        assert!(plan_settings(&io, &c, &EffectSettings::default()).unwrap().iter().all(|s| matches!(s, LookStep::WriteFile { .. })));
        assert_eq!(read_settings(&io, &c).preset, None);
    }
}

#[cfg(test)]
mod native_mono_tests {
    //! KDE, Pantheon, COSMIC: the stock black-and-white filter on a fake machine.
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// Files and gsettings in memory; `kwriteconfig6` really writes to kwinrc
    /// (by parsing arguments), so the snapshot after writing sees the result.
    #[derive(Default)]
    struct NativeIo {
        files: RefCell<HashMap<String, String>>,
        gs: RefCell<HashMap<String, String>>,
        ran: RefCell<Vec<Vec<String>>>,
    }

    const KWINRC: &str = "/h/.config/kwinrc";

    impl NativeIo {
        fn kwrite(&self, a: &[String]) {
            let at = |flag: &str| a.iter().position(|x| x == flag).map(|i| a[i + 1].clone()).unwrap();
            let (group, key) = (at("--group"), at("--key"));
            let value = (!a.contains(&"--delete".to_string())).then(|| a.last().unwrap().clone());
            let mut cfg: Vec<(String, Vec<(String, String)>)> = Vec::new();
            let text = self.files.borrow().get(KWINRC).cloned().unwrap_or_default();
            for line in text.lines() {
                if let Some(g) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                    cfg.push((g.to_string(), Vec::new()));
                } else if let Some((k, v)) = line.split_once('=') {
                    cfg.last_mut().unwrap().1.push((k.to_string(), v.to_string()));
                }
            }
            if !cfg.iter().any(|(g, _)| *g == group) {
                cfg.push((group.clone(), Vec::new()));
            }
            let g = cfg.iter_mut().find(|(g, _)| *g == group).unwrap();
            g.1.retain(|(k, _)| *k != key);
            if let Some(v) = value {
                g.1.push((key, v));
            }
            let mut out = String::new();
            for (g, kv) in cfg.iter().filter(|(_, kv)| !kv.is_empty()) {
                out.push_str(&format!("[{g}]\n"));
                for (k, v) in kv {
                    out.push_str(&format!("{k}={v}\n"));
                }
            }
            self.files.borrow_mut().insert(KWINRC.to_string(), out);
        }
    }

    impl LookIo for NativeIo {
        fn run(&self, a: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(a.to_vec());
            match a[0].as_str() {
                "kwriteconfig6" => self.kwrite(a),
                "gsettings" => {
                    let k = format!("{} {}", a[2], a[3]);
                    if a[1] == "get" {
                        return self.gs.borrow().get(&k).map_or((false, String::new()), |v| (true, v.clone()));
                    }
                    self.gs.borrow_mut().insert(k, a[4].clone());
                }
                _ => {}
            }
            (true, String::new())
        }
        fn have(&self, _: &str) -> bool {
            true
        }
        fn read(&self, p: &str) -> Option<String> {
            self.files.borrow().get(p).cloned()
        }
        fn write(&self, p: &str, t: &str) -> bool {
            self.files.borrow_mut().insert(p.into(), t.into());
            true
        }
        fn list_dir(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }

    fn ctx(de: &str) -> LookCtx {
        let mut c = LookCtx::new(de, "/h", Some("/h/.config"));
        c.wayland = true;
        c
    }

    fn mono(strength: f64) -> EffectSettings {
        EffectSettings { preset: Some(Preset::Monochrome), params: EffectParams { strength, speed: 1.0 } }
    }

    fn calls(io: &NativeIo, method: &str) -> usize {
        io.ran.borrow().iter().filter(|r| r.iter().any(|x| x.ends_with(method))).count()
    }

    #[test]
    fn kde_on_change_off_restores_kwinrc() {
        crate::i18n::set_language(Some("en"));
        let c = ctx("kde");
        let io = NativeIo::default();
        let before = "[Plugins]\nblurEnabled=false\n".to_string();
        io.files.borrow_mut().insert(KWINRC.into(), before.clone());
        assert_eq!(read_settings(&io, &c).preset, None);

        assert_eq!(apply(&io, &c, &mono(0.7)), "Screen effects applied.");
        let rc = io.read(KWINRC).unwrap();
        assert!(rc.contains("colorblindnesscorrectionEnabled=true"), "{rc}");
        assert!(rc.contains("[Effect-colorblindnesscorrection]\nMode=3\nIntensity=0.70\n"), "{rc}");
        assert_eq!(calls(&io, "loadEffect"), 1);
        assert_eq!(calls(&io, "reconfigureEffect"), 1);
        let now = read_settings(&io, &c);
        assert_eq!((now.preset, now.params.strength), (Some(Preset::Monochrome), 0.7));

        // The same again — nothing to do; a different strength — rewrite.
        assert!(plan_settings(&io, &c, &mono(0.7)).unwrap().is_empty());
        assert!(!plan_settings(&io, &c, &mono(0.3)).unwrap().is_empty());

        // Turning off — kwinrc as it was before Rexon, the effect unloaded.
        apply(&io, &c, &EffectSettings::default());
        assert_eq!(io.read(KWINRC).unwrap(), before);
        assert_eq!(calls(&io, "unloadEffect"), 1);
        assert!(plan_settings(&io, &c, &EffectSettings::default()).unwrap().is_empty());

        // Our own shaders are not applied here yet — an honest refusal.
        let err = plan_settings(&io, &c, &EffectSettings { preset: Some(Preset::Crt), ..mono(0.5) }).unwrap_err();
        assert!(err.contains("KWin"), "{err}");
    }

    /// The person's color blindness filter (a different mode) is touched neither
    /// by turning on nor by turning off.
    #[test]
    fn kde_colorblind_filter_of_the_person_is_left_alone() {
        crate::i18n::set_language(Some("en"));
        let c = ctx("kde");
        let io = NativeIo::default();
        let mine = "[Effect-colorblindnesscorrection]\nMode=1\n[Plugins]\ncolorblindnesscorrectionEnabled=true\n";
        io.files.borrow_mut().insert(KWINRC.into(), mine.into());
        assert_eq!(read_settings(&io, &c).preset, None);
        let err = plan_settings(&io, &c, &mono(1.0)).unwrap_err();
        assert!(err.contains("Accessibility"), "{err}");
        assert!(plan_settings(&io, &c, &EffectSettings::default()).unwrap().is_empty());
        assert_eq!(io.read(KWINRC).unwrap(), mine);
    }

    #[test]
    fn pantheon_gala_keys() {
        crate::i18n::set_language(Some("en"));
        let c = ctx("pantheon");
        let io = NativeIo::default();
        io.gs.borrow_mut().insert(format!("{GALA_A11Y_SCHEMA} enable-monochrome-filter"), "false".into());
        io.gs.borrow_mut().insert(format!("{GALA_A11Y_SCHEMA} monochrome-filter-strength"), "1.0".into());
        let st = read_state(&io, &c);
        assert!(st.can_apply && st.monochrome_only && st.has_strength && !st.trial);
        assert_eq!(st.presets.len(), 1);
        assert_eq!(st.strength, 1.0);

        assert_eq!(apply(&io, &c, &mono(1.0)), "Screen effects applied.");
        // Strength as a number with a dot — the `d` type of the Gala schema.
        assert_eq!(io.gs.borrow()[&format!("{GALA_A11Y_SCHEMA} monochrome-filter-strength")], "1.00");
        assert_eq!(read_settings(&io, &c).preset, Some(Preset::Monochrome));
        assert!(plan_settings(&io, &c, &mono(1.0)).unwrap().is_empty());

        apply(&io, &c, &EffectSettings::default());
        assert_eq!(io.gs.borrow()[&format!("{GALA_A11Y_SCHEMA} enable-monochrome-filter")], "false");
        // Strength is remembered even for a disabled filter.
        let now = read_settings(&io, &c);
        assert_eq!((now.preset, now.params.strength), (None, 1.0));
    }

    #[test]
    fn cosmic_state_file_keeps_inversion() {
        crate::i18n::set_language(Some("en"));
        let c = ctx("cosmic");
        let io = NativeIo::default();
        let path = cosmic_filter_path(&std::env::var("XDG_STATE_HOME").unwrap_or_default(), &c.home);
        assert_eq!(cosmic_filter_path("", "/h"), "/h/.local/state/cosmic-comp/a11y_screen_filter.ron");
        assert_eq!(cosmic_filter_path("/s/", "/h"), "/s/cosmic-comp/a11y_screen_filter.ron");
        let st = read_state(&io, &c);
        assert!(st.can_apply && st.monochrome_only && !st.has_strength && !st.trial);

        // Turning on says honestly: it will work from the next login.
        assert_eq!(apply(&io, &c, &mono(0.5)), "COSMIC will apply this the next time you log in.");
        assert_eq!(io.read(&path).unwrap(), "(\n    inverted: false,\n    color_filter: Some(Greyscale),\n)");
        assert_eq!(read_settings(&io, &c).preset, Some(Preset::Monochrome));
        assert!(plan_settings(&io, &c, &mono(0.9)).unwrap().is_empty(), "силы у COSMIC нет");

        // The person's inversion is kept.
        io.files.borrow_mut().insert(path.clone(), "(\n    inverted: true,\n    color_filter: Some(Greyscale),\n)".into());
        apply(&io, &c, &EffectSettings::default());
        assert_eq!(io.read(&path).unwrap(), "(\n    inverted: true,\n)");
        assert_eq!(read_settings(&io, &c).preset, None);
    }
}
