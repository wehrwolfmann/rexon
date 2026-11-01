//! «Эффекты экрана»: строки развёртки, кинескоп, зелёный и янтарный
//! монитор, видеокассета, аналоговое ТВ, киноплёнка, чёрно-белый — одной
//! настройкой во всех окружениях, где композитор вообще даёт наложить
//! шейдер.
//!
//! Устройство:
//! * Ядро — ОДИН набор функций GLSL с точкой входа
//!   `vec4 rx_effect(vec2 uv, vec2 res, float t)` ([`core_glsl`]): `uv` —
//!   координата 0…1, `res` — размер картинки в пикселях, `t` — секунды,
//!   уже помноженные на скорость. Текстуру ядро читает только через
//!   `rx_tex(uv)` — эту функцию даёт обёртка. Ядро написано на общем
//!   подмножестве GLSL 1.10 / GLSL ES 1.00 (только плавающие литералы, без
//!   целочисленной арифметики и без `texture*` напрямую), поэтому одно и то
//!   же тело собирается под любым композитором.
//! * Обёртки под способы ([`Host`], [`generate`]): Cogl-сниппет (GNOME
//!   Shell `Shell.GLSLEffect`, Cinnamon `Cinnamon.GLSLEffect`), Hyprland
//!   `decoration:screen_shader` (GLSL ES 300), picom `window_shader()`
//!   (GLSL 330), Wayfire filters (`@builtin@`, `get_pixel`), KWin
//!   (`#version 140`, `sampler`, `texcoord0`). Сила и скорость вшиваются
//!   константами: у Hyprland, picom и Wayfire своих униформ нет. Частота
//!   кадров — забота хоста (расширение GNOME/Cinnamon решает, как часто
//!   перерисовывать; Hyprland и picom рисуют каждый кадр).
//! * На «оконных» способах (picom, KWin) шейдер ложится на каждое окно
//!   отдельно: кривизна и затемнение краёв у каждого окна смотрелись бы
//!   как дефект, поэтому в оконном режиме они выключены
//!   ([`Preset::warps`] помечает такие пресеты для интерфейса).
//! * Таблица «окружение → способ → поддержка» ([`support`]) с причинами
//!   для человека.
//! * Hyprland уже применяется ([`plan_settings`]): файл шейдера в
//!   `~/.config/rexon/screen-effects/hyprland.frag`, наш файл
//!   `rexon-screen-effects.lua|conf` рядом с конфигом и строка его
//!   подключения в управляемом блоке — тот же механизм, что у «Анимации»
//!   ([`crate::animations::wm_include`]). Выключение снимает блок, конфиг
//!   человека возвращается к прежним байтам.
//!
//! ⚖️ Происхождение: весь GLSL здесь — собственный код Rexon (Apache-2.0).
//! Приёмы общеизвестные (развёртка косинусом, бочкообразное искажение,
//! маска-решётка, перевод в YIQ по коэффициентам FCC, шум по хешу), ни
//! строки из чужих шейдеров не взято. Код расширения Shader Effects
//! (GPL-2.0) и GPL-шейдеры libretro не смотрелись.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::animations::{
    argv, parse_state_line, run_steps, state_line, without_block, wm_include, wm_targets, LookCtx, LookIo, LookStep,
};
use crate::i18n::{n_, tr};
use crate::themes::WmSyntax;

// ═══════════════════════════════════════════════════════════════════════════
// Пресеты
// ═══════════════════════════════════════════════════════════════════════════

/// Готовый эффект.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum Preset {
    /// Строки развёртки поверх картинки.
    Scanlines,
    /// Кинескоп: кривизна, виньетка, маска-решётка, свечение, сведение лучей.
    Crt,
    /// Монохромный монитор с зелёным люминофором.
    CrtGreen,
    /// Монохромный монитор с янтарным люминофором.
    CrtAmber,
    /// Видеокассета: дрожание строк, полоса трекинга, размытый цвет, шум.
    Vhs,
    /// Аналоговый сигнал (по мотивам композитного NTSC): размытый цвет,
    /// радужные края, ползущие точки.
    Ntsc,
    /// Киноплёнка: «трёхцветная» цветокоррекция, зерно, мерцание, царапины,
    /// дрожание кадра в окне.
    Film,
    /// Чёрно-белый.
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

    /// Устойчивое имя (в файлах состояния и в интерфейсе).
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

    /// Пресет меняется во времени (шум, дрожание, мерцание). Без часов у
    /// хоста или при скорости 0 он застывает на одном кадре.
    pub fn animated(self) -> bool {
        matches!(self, Preset::Vhs | Preset::Ntsc | Preset::Film)
    }

    /// Пресет гнёт картинку или темнит края (кривизна кинескопа, виньетка).
    /// На оконных способах это выключается — иначе каждое окно гнулось бы
    /// отдельно.
    pub fn warps(self) -> bool {
        matches!(self, Preset::Crt | Preset::CrtGreen | Preset::CrtAmber | Preset::Film)
    }

    /// Тело `rx_effect` этого пресета.
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

/// Сила и скорость эффекта.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EffectParams {
    /// 0…1: насколько выражены артефакты (у «Чёрно-белого» — насколько
    /// погашен цвет).
    pub strength: f64,
    /// 0…3, 1 — обычная; 0 — эффект застывает (и не грузит видеокарту
    /// покадровой перерисовкой).
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
// Ядро GLSL
// ═══════════════════════════════════════════════════════════════════════════

/// Общие помощники. Требование к обёртке: перед этим текстом объявлена
/// `vec4 rx_tex(vec2 uv)`, константы `RX_S` и `RX_WINDOW` и точность.
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

/// Монохромный люминофор: `RX_TINT_VALUE` подставляет [`Preset::body`].
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

/// Число GLSL: всегда с точкой (GLSL 1.10 и ES 1.00 не приводят целые).
fn glsl_float(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') { format!("{s}0") } else { s.to_string() }
}

/// Ядро пресета: константы, помощники и `rx_effect`. Перед ним обёртка
/// объявляет точность и `vec4 rx_tex(vec2 uv)`.
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
// Обёртки под способы
// ═══════════════════════════════════════════════════════════════════════════

/// Способ наложить шейдер.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum Host {
    /// Cogl-сниппет для `Shell.GLSLEffect` (GNOME) и `Cinnamon.GLSLEffect`
    /// на `Main.uiGroup` / сцене: весь экран. Униформы `rx_res` (размер
    /// актёра, px) и `rx_time` (секунды) ставит расширение через
    /// `set_uniform_float`; сэмплер — слой 0 (значение по умолчанию).
    Cogl,
    /// Hyprland `decoration:screen_shader`: весь экран, GLSL ES 3.00,
    /// `tex`, `v_texcoord`, `time` (секунды).
    Hyprland,
    /// picom `--window-shader-fg`: каждое окно, GLSL 3.30, `texcoord` в
    /// пикселях, `time` в миллисекундах, `default_post_processing`.
    Picom,
    /// Wayfire filters (soreau/filters) `wf/filters/set-fs-shader`: весь
    /// выход, GLSL ES 3.00 с `@builtin@`, `get_pixel(uvpos)`; часов нет.
    Wayfire,
    /// KWin (JS-эффект `addFragmentShader(MapTexture | Modulate, …)`):
    /// каждое окно, `#version 140`, `sampler`, `texcoord0`, `modulation`;
    /// своего таймера у JS-эффекта нет.
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

    /// Хост даёт шейдеру время — анимированные пресеты живут.
    pub fn has_time(self) -> bool {
        matches!(self, Host::Cogl | Host::Hyprland | Host::Picom)
    }

    /// Шейдер ложится на каждое окно, а не на весь экран.
    pub fn per_window(self) -> bool {
        matches!(self, Host::Picom | Host::KWin)
    }

    /// Расширение файла шейдера.
    pub fn file_ext(self) -> &'static str {
        match self {
            Host::Cogl => "glsl",
            _ => "frag",
        }
    }
}

/// Готовый шейдер под способ.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Shader {
    pub host: Host,
    pub preset: Preset,
    /// Текст файла шейдера. У Cogl — объявления сниппета (`declarations`).
    pub source: String,
    /// Только у Cogl: код сниппета (`code`, хук `FRAGMENT`, с заменой).
    pub cogl_code: Option<String>,
    /// Униформы, которые обязан задать хост (кроме сэмплера по умолчанию).
    pub uniforms: Vec<&'static str>,
    /// Шейдер читает время — хост должен перерисовывать каждый кадр.
    pub animated: bool,
    /// Оконный режим: без кривизны и виньетки.
    pub window_mode: bool,
}

/// Шейдер пресета под способ.
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
// Где можно: окружение → способ → поддержка
// ═══════════════════════════════════════════════════════════════════════════

/// Что покрывает эффект в окружении.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Coverage {
    /// Весь экран.
    Screen,
    /// Каждое окно отдельно (кривизна и виньетка выключены).
    Windows,
    /// Только штатный чёрно-белый фильтр окружения.
    MonochromeOnly,
    /// Не проверено.
    Unknown,
    /// Нельзя.
    None,
}

/// Строка таблицы поддержки.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Support {
    /// Окружение как его зовёт ядро (`system::detect_current_de`).
    pub de: String,
    pub coverage: Coverage,
    pub host: Option<Host>,
    /// Анимированные пресеты живут (иначе застывают на кадре).
    pub animated: bool,
    /// Штатный чёрно-белый фильтр есть (KDE, Pantheon, COSMIC).
    pub native_monochrome: bool,
    /// Rexon уже умеет включать здесь эффекты.
    pub ready: bool,
    /// Что для этого нужно поставить или заменить (пусто — ничего).
    pub needs: String,
    /// Честное пояснение для человека.
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

/// Таблица поддержки для окружения (`wayland` — сеанс Wayland).
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
        "gnome" => row(Screen, Some(Host::Cogl), false, false, "", N_GNOME),
        "cinnamon-mint" => row(Screen, Some(Host::Cogl), false, false, "", N_CINNAMON),
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
        // Deepin — только Treeland (сеанс X11 Rexon не поддерживает).
        "deepin" if wayland => row(None, Option::None, false, false, "", N_TREELAND),
        "deepin" => row(None, Option::None, false, false, "", N_DEEPIN),
        _ => row(None, Option::None, false, false, "", N_UNKNOWN),
    }
}

/// Все известные окружения: (имя ядра, сеанс Wayland).
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

/// Вся таблица (для интерфейса и справки).
pub fn support_table() -> Vec<Support> {
    KNOWN_DESKTOPS.iter().map(|(de, w)| support(de, *w)).collect()
}

// ═══════════════════════════════════════════════════════════════════════════
// Настройка и запись (пока — Hyprland)
// ═══════════════════════════════════════════════════════════════════════════

/// Состояние «Эффектов экрана».
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EffectSettings {
    /// `None` — эффекты выключены.
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

/// Файл шейдера Hyprland (абсолютный путь — Hyprland читает его сам).
pub fn hypr_shader_path(ctx: &LookCtx) -> String {
    ctx.cfg("rexon/screen-effects/hyprland.frag")
}

/// FNV-1a: метка версии шейдера в нашем файле. Шейдер поменялся (новая
/// версия Rexon) — меняется наш файл, и Hyprland перечитывает конфиг.
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

/// Строка Lua в кавычках.
fn lua_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Наш файл Hyprland: строка состояния, путь шейдера и — у движущихся
/// эффектов — выключенный учёт повреждений (без него Hyprland не
/// перерисовывает экран каждый кадр и показывает ошибку о `time`).
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

/// Снимок: что сейчас включено. Сила и скорость помнятся и у выключенного
/// эффекта (из нашего файла), чтобы ползунки не сбрасывались.
pub fn read_settings(io: &dyn LookIo, ctx: &LookCtx) -> EffectSettings {
    if ctx.de != "hyprland" {
        return EffectSettings::default();
    }
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

fn hypr_reload() -> LookStep {
    LookStep::Notify(vec![argv(&["hyprctl", "reload"])])
}

/// План записи. `Err` — готовая фраза, почему здесь нельзя.
pub fn plan_settings(io: &dyn LookIo, ctx: &LookCtx, new: &EffectSettings) -> Result<Vec<LookStep>, String> {
    let sup = support(&ctx.de, std::env::var("WAYLAND_DISPLAY").is_ok());
    if ctx.de != "hyprland" {
        return Err(sup.note);
    }
    let mut steps = Vec::new();
    let Some(preset) = new.preset else {
        // Выключение: снять наш блок из каждого конфига, где он есть.
        // Наш файл и шейдер остаются (помнят силу и скорость), но больше
        // не подключены.
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

/// Применить: готовая фраза для всплывающего сообщения (пусто — менять
/// было нечего).
pub fn apply(io: &dyn LookIo, ctx: &LookCtx, new: &EffectSettings) -> String {
    let steps = match plan_settings(io, ctx, new) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if steps.is_empty() {
        return String::new();
    }
    match run_steps(io, ctx, &steps) {
        Ok(()) => tr(MSG_APPLIED),
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    // ── генератор ──────────────────────────────────────────────────────────

    fn params(strength: f64, speed: f64) -> EffectParams {
        EffectParams { strength, speed }
    }

    fn count(hay: &str, needle: &str) -> usize {
        hay.matches(needle).count()
    }

    /// Каждая обёртка каждого пресета: версия первой строкой, точка входа,
    /// униформы, единственный `rx_effect`, нужный вызов текстуры, парные
    /// скобки, время — только у движущихся.
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

    /// Ядро не трогает текстуру мимо `rx_tex` и не пишет целых литералов
    /// там, где GLSL 1.10 / ES 1.00 не приводит их к float.
    #[test]
    fn core_is_portable() {
        for preset in Preset::ALL {
            let core = core_glsl(preset, EffectParams::default(), false);
            for bad in ["texture(", "texture2D(", "textureSize(", "uniform ", "#version", "precision "] {
                assert!(!core.contains(bad), "{preset:?}: {bad}");
            }
            assert!(!core.contains("RX_TINT_VALUE"), "{preset:?}");
            // Числа в выражениях — только с точкой (кроме индекса нет ни одного целого).
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

    /// Компиляция настоящим glslangValidator (если он стоит): каждое
    /// окружение со своими заглушками того, что подставляет хост.
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
                        // Cogl собирает сниппет внутрь своего шейдера: даём
                        // ему GLSL 1.10 и переменные шаблона Cogl.
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

    // ── таблица поддержки ──────────────────────────────────────────────────

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
        // Время есть только там, где хост его даёт.
        assert!(support("hyprland", true).animated && support("gnome", true).animated);
        assert!(!support("kde", true).animated && !support("wayfire", true).animated);
        // Пока применяется только Hyprland.
        let ready: Vec<_> = support_table().into_iter().filter(|s| s.ready).map(|s| s.de).collect();
        assert_eq!(ready, vec!["hyprland".to_string()]);
        for s in support_table() {
            assert!(!s.note.is_empty(), "{}", s.de);
            // На оконных способах кривизна выключена — и пояснение об этом говорит.
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

        // Статичный кинескоп: шейдер без времени, учёт повреждений не трогаем.
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

        // Повтор — пустой план.
        assert!(plan_settings(&io, &c, &on(Preset::Crt, 0.5, 1.0)).unwrap().is_empty());

        // Движущаяся кассета: время в шейдере, учёт повреждений выключен.
        run(&io, &c, &on(Preset::Vhs, 0.8, 2.0));
        let shader = io.files.borrow()["/h/.config/rexon/screen-effects/hyprland.frag"].clone();
        assert!(shader.contains("uniform float time;") && shader.contains("time * 2.0"), "{shader}");
        let own = io.files.borrow()["/h/.config/hypr/rexon-screen-effects.lua"].clone();
        assert!(own.contains("hl.config({ debug = { damage_tracking = 0 } })"), "{own}");
        assert!(own.contains("preset=vhs") && own.contains("speed=2") && own.contains("strength=0.8"), "{own}");
        // Блок один, не плодится.
        let cfg = io.files.borrow()["/h/.config/hypr/hyprland.lua"].clone();
        assert_eq!(cfg.matches(">>> Rexon: screen-effects").count(), 1, "{cfg}");

        // Скорость 0 — снова без времени и без нагрузки.
        run(&io, &c, &on(Preset::Vhs, 0.8, 0.0));
        let own = io.files.borrow()["/h/.config/hypr/rexon-screen-effects.lua"].clone();
        assert!(!own.contains("damage_tracking"), "{own}");

        // Выключение: конфиг человека — байт в байт прежний.
        run(&io, &c, &EffectSettings { preset: None, params: params(0.8, 0.0) });
        assert_eq!(io.files.borrow()["/h/.config/hypr/hyprland.lua"], mine);
        let s = read_settings(&io, &c);
        assert_eq!(s.preset, None);
        assert_eq!(s.params.strength, 0.8, "ползунки помнятся");
        // Повторное выключение — ничего.
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

        // Конфига Hyprland нет — честная фраза, ничего не заведено.
        let io = FakeIo::default();
        let steps = plan_settings(&io, &c, &on(Preset::Crt, 0.5, 1.0)).unwrap();
        let err = run_steps(&io, &c, &steps).unwrap_err();
        assert!(err.contains("Hyprland"), "{err}");
        assert!(!io.files.borrow().contains_key("/h/.config/hypr/hyprland.conf"));

        // Окружение, где пока нельзя, — пояснение из таблицы.
        let k = LookCtx::new("sway", "/h", Some("/h/.config"));
        assert_eq!(plan_settings(&io, &k, &on(Preset::Crt, 0.5, 1.0)).unwrap_err(), support("sway", true).note);
        assert_eq!(apply(&io, &k, &on(Preset::Crt, 0.5, 1.0)), support("sway", true).note);
    }

    /// Путь шейдера с кавычкой и обратной косой не ломает строку Lua.
    #[test]
    fn lua_path_is_escaped() {
        assert_eq!(lua_string(r#"/home/a"b\c"#), r#""/home/a\"b\\c""#);
    }
}
