//! Light and dark appearance on a schedule, in all environments.
//!
//! The person chooses "Light", "Dark" or "Automatic". In automatic mode the
//! switch happens at two times of day ("light from 07:00, dark from 19:00")
//! or at sunrise and sunset at their location. What gets switched is also
//! their choice: a pair of global themes (Plasma), a pair of color schemes, a
//! pair of GTK themes, a pair of icon themes, a pair of wallpaper pictures and
//! the light/dark mode of applications (the freedesktop portal's
//! `color-scheme`).
//!
//! How the environments themselves do it (checked 01.11.2026):
//! * Plasma 6.8 has its own global theme switching: `kdeglobals [KDE]
//!   AutomaticLookAndFeel`, `DefaultLightLookAndFeel`, `DefaultDarkLookAndFeel`
//!   (schema `lookandfeelsettings.kcfg`), executed by the kded module
//!   `lookandfeelautoswitcher` (it watches `kdeglobals` via `KConfigWatcher`,
//!   hence the write uses `--notify`). The schedule is shared with the night
//!   color temperature — `knighttimerc` (`[General] Source=Location|Times`,
//!   `[Location] Automatic/Latitude/Longitude`, `[Times] SunriseStart/
//!   SunsetStart`), read by the `knighttimed` service. Rexon does NOT switch
//!   the global theme in Plasma itself — it configures Plasma.
//! * COSMIC has `com.system76.CosmicTheme.Mode/v1/auto_switch` (switched by
//!   cosmic-settings-daemon at sunrise and sunset; it has no times of its own)
//!   and `is_dark`. The "by the sun" mode is left to COSMIC; for "by time",
//!   Rexon writes `is_dark`.
//! * GNOME, Cinnamon, MATE, Xfce, Budgie, LXQt and tiling WMs have no schedule
//!   of their own: Rexon switches.
//!
//! The executor has no permanently running process: the systemd user timer
//! [`TIMER_UNIT`] wakes `rexon --apply-day-night` at the switch moments (by
//! the sun, the exact moments of the next switches in UTC, with the timer
//! rewritten after each switch; by time, two daily moments), and at session
//! login the same run is triggered by the autostart entry [`AUTOSTART_FILE`].
//! The timer uses wall-clock time: a switch missed during sleep fires after
//! wake-up, one missed while the machine was off fires at startup
//! (`Persistent=true`).
//!
//! Each part is applied through THE SAME paths as the "Apply" button in
//! "Appearance" ([`crate::appearance_installer::LiveInstaller::apply`]): in
//! each environment the global theme, scheme, GTK and icons are written the
//! way that environment expects. There is no second way to apply a theme
//! here (rule #12).

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::i18n::{tr, trc, trf};

/// The flag for running without a window.
pub const CLI_FLAG: &str = "--apply-day-night";
/// The switch timer.
pub const TIMER_UNIT: &str = "rexon-day-night.timer";
/// The service the timer wakes.
pub const SERVICE_UNIT: &str = "rexon-day-night.service";
/// The autostart entry: apply the current state at session login.
pub const AUTOSTART_FILE: &str = "rexon-day-night.desktop";
/// The "Appearance" sections that show the block.
pub const SECTIONS_KDE: &[&str] = &["global", "colors"];
pub const SECTIONS_OTHER: &[&str] = &["colors", "gtk"];

// ============================================================================
// Settings
// ============================================================================

/// The person's choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Rexon does not manage light and dark (nothing is chosen).
    #[default]
    Off,
    Light,
    Dark,
    Auto,
}

/// The automatic mode schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Schedule {
    /// Light from sunrise to sunset.
    #[default]
    Sun,
    /// Two times of day.
    Times,
}

/// Light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Light,
    Dark,
}

impl Phase {
    pub fn is_dark(self) -> bool {
        self == Phase::Dark
    }
}

/// A "light / dark" pair for one part.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Pair {
    pub on: bool,
    pub light: String,
    pub dark: String,
}

impl Pair {
    fn pick(&self, phase: Phase) -> &str {
        match phase {
            Phase::Light => &self.light,
            Phase::Dark => &self.dark,
        }
    }
}

/// The settings (`~/.config/rexon/day-night.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mode: Mode,
    pub schedule: Schedule,
    /// "HH:MM" local time.
    pub light_at: String,
    pub dark_at: String,
    /// The location is determined automatically (by the time zone; in Plasma by
    /// the Plasma service, in COSMIC by COSMIC itself).
    pub auto_location: bool,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub global: Pair,
    pub colors: Pair,
    pub gtk: Pair,
    pub icons: Pair,
    pub wallpaper: Pair,
    /// The light/dark mode of applications (the freedesktop portal).
    pub portal: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: Mode::Off,
            schedule: Schedule::Sun,
            light_at: "07:00".into(),
            dark_at: "19:00".into(),
            auto_location: true,
            lat: None,
            lon: None,
            global: Pair::default(),
            colors: Pair::default(),
            gtk: Pair::default(),
            icons: Pair::default(),
            wallpaper: Pair::default(),
            portal: true,
        }
    }
}

/// The last applied state (`~/.local/state/rexon/day-night.json`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Memo {
    /// The environment of the last login: the timer wakes the service without
    /// `XDG_CURRENT_DESKTOP` in the environment (Xfce and MATE do not pass it to
    /// systemd).
    pub de: String,
    pub phase: Option<Phase>,
    /// When it was applied (unix, s).
    pub at: i64,
}

/// "HH:MM" → minutes since midnight.
pub fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Latitude/longitude from an input field: a comma or a period.
pub fn parse_coord(s: &str, limit: f64) -> Option<f64> {
    let v: f64 = s.trim().replace(',', ".").parse().ok()?;
    (v.is_finite() && v.abs() <= limit).then_some(v)
}

// ============================================================================
// Time and the sun
// ============================================================================

/// Light or dark by two times. `light_from == dark_from` is a schedule
/// without a switch: always light (such settings are not saved).
pub fn phase_by_times(now_min: u32, light_from: u32, dark_from: u32) -> Phase {
    let light = if light_from <= dark_from {
        now_min >= light_from && now_min < dark_from
    } else {
        now_min >= light_from || now_min < dark_from
    };
    if light || light_from == dark_from { Phase::Light } else { Phase::Dark }
}

/// The sun on one solar day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SunDay {
    /// Sunrise and sunset, unix seconds.
    Normal { rise: i64, set: i64 },
    /// The sun does not set.
    PolarDay,
    /// The sun does not rise.
    PolarNight,
}

const J2000: f64 = 2_451_545.0;
const UNIX_EPOCH_JD: f64 = 2_440_587.5;

fn jd_of_unix(t: i64) -> f64 {
    t as f64 / 86_400.0 + UNIX_EPOCH_JD
}

fn unix_of_jd(j: f64) -> i64 {
    ((j - UNIX_EPOCH_JD) * 86_400.0).round() as i64
}

/// The number of the solar day (since J2000) whose noon is closest to `t`.
fn day_number(t: i64, lon: f64) -> i64 {
    (jd_of_unix(t) - J2000 - 0.0009 + lon / 360.0).round() as i64
}

/// Sunrise and sunset of solar day `n` via the sunrise equation (mean
/// anomaly, equation of the center, ecliptic longitude, declination;
/// refraction and the solar disk give −0.833°). Accuracy is about a minute at
/// middle latitudes; altitude above sea level is not taken into account.
/// Latitude in degrees (north +), longitude in degrees (east +).
pub fn sun_day(n: i64, lat: f64, lon: f64) -> SunDay {
    let j_star = n as f64 + 0.0009 - lon / 360.0;
    let m = (357.5291 + 0.985_600_28 * j_star).rem_euclid(360.0);
    let mr = m.to_radians();
    let c = 1.9148 * mr.sin() + 0.0200 * (2.0 * mr).sin() + 0.0003 * (3.0 * mr).sin();
    let lambda = (m + c + 180.0 + 102.9372).rem_euclid(360.0).to_radians();
    let transit = J2000 + j_star + 0.0053 * mr.sin() - 0.0069 * (2.0 * lambda).sin();
    let sin_d = lambda.sin() * 23.4397_f64.to_radians().sin();
    let cos_d = (1.0 - sin_d * sin_d).sqrt();
    let phi = lat.to_radians();
    let cos_w = ((-0.833_f64).to_radians().sin() - phi.sin() * sin_d) / (phi.cos() * cos_d);
    if cos_w < -1.0 {
        return SunDay::PolarDay;
    }
    if cos_w > 1.0 {
        return SunDay::PolarNight;
    }
    let half = cos_w.acos().to_degrees() / 360.0;
    SunDay::Normal { rise: unix_of_jd(transit - half), set: unix_of_jd(transit + half) }
}

/// The sun on the day whose noon is closest to `t`.
pub fn sun_around(t: i64, lat: f64, lon: f64) -> SunDay {
    sun_day(day_number(t, lon), lat, lon)
}

/// Light or dark by the sun at moment `t`.
pub fn phase_by_sun(t: i64, lat: f64, lon: f64) -> Phase {
    match sun_around(t, lat, lon) {
        SunDay::Normal { rise, set } if t >= rise && t < set => Phase::Light,
        SunDay::Normal { .. } | SunDay::PolarNight => Phase::Dark,
        SunDay::PolarDay => Phase::Light,
    }
}

/// The next switches by the sun after `t`: (moment, what begins). Empty means
/// polar day or night for the coming days.
pub fn next_sun_switches(t: i64, lat: f64, lon: f64, count: usize) -> Vec<(i64, Phase)> {
    let n0 = day_number(t, lon);
    let mut out: Vec<(i64, Phase)> = (n0 - 1..=n0 + 3)
        .filter_map(|n| match sun_day(n, lat, lon) {
            SunDay::Normal { rise, set } => Some([(rise, Phase::Light), (set, Phase::Dark)]),
            _ => None,
        })
        .flatten()
        .filter(|(at, _)| *at > t)
        .collect();
    out.sort_by_key(|(at, _)| *at);
    out.truncate(count);
    out
}

/// The next switches by two times after `now_min` (minutes since local
/// midnight): (in how many minutes, what begins).
pub fn next_time_switches(now_min: u32, light_from: u32, dark_from: u32) -> Vec<(u32, Phase)> {
    if light_from == dark_from {
        return Vec::new();
    }
    let until = |at: u32| (at + 1440 - now_min - 1) % 1440 + 1;
    let mut out = vec![(until(light_from), Phase::Light), (until(dark_from), Phase::Dark)];
    out.sort_by_key(|(m, _)| *m);
    out
}

/// Days since 1970-01-01 → (year, month, day) in the Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// (year, month, day) → days since 1970-01-01.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A moment for `OnCalendar=` in UTC: "2026-11-01 16:41:00 UTC". The
/// machine's time zone is then not needed at all: the timer and the sun
/// calculation both speak UTC.
pub fn utc_calendar(t: i64) -> String {
    let (y, mo, d) = civil_from_days(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400);
    format!("{y:04}-{mo:02}-{d:02} {:02}:{:02}:{:02} UTC", s / 3600, s / 60 % 60, s % 60)
}

/// "HH:MM" of moment `t` on the local clock with offset `offset` (seconds).
pub fn local_hhmm(t: i64, offset: i64) -> String {
    let s = (t + offset).rem_euclid(86_400);
    format!("{:02}:{:02}", s / 3600, s / 60 % 60)
}

// ============================================================================
// Location by time zone
// ============================================================================

/// The zone name: `$TZ` (without a leading `:`), otherwise the target of the
/// `/etc/localtime` link after `zoneinfo/`.
pub fn tz_name(tz_env: Option<&str>, localtime_link: Option<&str>) -> Option<String> {
    if let Some(tz) = tz_env.map(|t| t.trim().trim_start_matches(':')).filter(|t| t.contains('/')) {
        return Some(tz.rsplit("zoneinfo/").next().unwrap_or(tz).to_string());
    }
    let link = localtime_link?;
    let (_, zone) = link.split_once("zoneinfo/")?;
    let zone = zone.trim_start_matches("posix/").trim_start_matches("right/");
    (!zone.is_empty()).then(|| zone.to_string())
}

/// "+5230" / "-0011532" → degrees.
fn iso6709_part(s: &str, deg_digits: usize) -> Option<f64> {
    let sign = match s.as_bytes().first()? {
        b'+' => 1.0,
        b'-' => -1.0,
        _ => return None,
    };
    let digits = &s[1..];
    if digits.len() < deg_digits + 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let num = |a: usize, b: usize| digits.get(a..b).and_then(|p| p.parse::<f64>().ok());
    let deg = num(0, deg_digits)?;
    let min = num(deg_digits, deg_digits + 2)?;
    let sec = if digits.len() >= deg_digits + 4 { num(deg_digits + 2, deg_digits + 4)? } else { 0.0 };
    Some(sign * (deg + min / 60.0 + sec / 3600.0))
}

/// The coordinates of the zone's main city from `zone1970.tab`
/// (`DE\t+5230+01322\tEurope/Berlin`). This is the place the clock already
/// follows, and the accuracy is enough for sunrise and sunset to within
/// minutes or tens of minutes.
pub fn zone_coords(tab: &str, zone: &str) -> Option<(f64, f64)> {
    tab.lines().filter(|l| !l.starts_with('#')).find_map(|line| {
        let mut cols = line.split('\t');
        let (_, coords, name) = (cols.next()?, cols.next()?, cols.next()?);
        if name.trim() != zone {
            return None;
        }
        let split = coords[1..].find(['+', '-'])? + 1;
        let (lat, lon) = coords.split_at(split);
        Some((iso6709_part(lat, 2)?, iso6709_part(lon, 3)?))
    })
}

/// The place name from the zone: "Europe/Berlin" → "Berlin".
pub fn zone_place(zone: &str) -> String {
    zone.rsplit('/').next().unwrap_or(zone).replace('_', " ")
}

// ============================================================================
// What the environment does by itself
// ============================================================================

/// Whether the block makes sense. Not in Deepin: themes there are set only by
/// the Control Center (it has its own "Automatic").
pub fn available(de: &str) -> bool {
    de != "deepin"
}

/// Only Plasma has a global theme.
fn has_global(de: &str) -> bool {
    de == "kde"
}

/// Plasma switches the global theme itself.
pub fn kde_native(cfg: &Config, de: &str) -> bool {
    cfg.mode == Mode::Auto && has_global(de) && cfg.global.on
}

/// COSMIC switches light and dark itself (only by the sun).
pub fn cosmic_native(cfg: &Config, de: &str) -> bool {
    cfg.mode == Mode::Auto && de == "cosmic" && cfg.schedule == Schedule::Sun && cfg.portal
}

/// Whether Rexon's timer is needed: in automatic mode there is at least one
/// part the environment does not switch by itself.
pub fn needs_timer(cfg: &Config, de: &str) -> bool {
    if cfg.mode != Mode::Auto {
        return false;
    }
    let portal_ours = cfg.portal && de != "kde" && !cosmic_native(cfg, de);
    let global_ours = cfg.global.on && has_global(de) && !kde_native(cfg, de);
    let covered = global_covers(cfg, de);
    portal_ours || global_ours || (cfg.colors.on && !covered) || cfg.gtk.on || (cfg.icons.on && !covered) || cfg.wallpaper.on
}

/// In Plasma a global theme carries its own color scheme and icons: when a
/// pair of global themes is enabled, separate scheme and icon pairs are not
/// applied — otherwise the Plasma module would override them after switching
/// the theme (or the other way round, depending on who came later).
pub fn global_covers(cfg: &Config, de: &str) -> bool {
    has_global(de) && cfg.global.on
}

/// A single apply action.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
    /// A theme of an "Appearance" section (`kind` is its `apply_kind`).
    Theme { kind: &'static str, name: String },
    /// A wallpaper picture.
    Wallpaper { path: String },
    /// A command (`gsettings` for the application mode).
    Run { argv: Vec<String> },
    /// COSMIC: write `is_dark`.
    CosmicDark { dark: bool },
}

fn gsettings(schema: &str, key: &str, value: &str) -> Action {
    Action::Run { argv: ["gsettings", "set", schema, key, value].iter().map(|s| s.to_string()).collect() }
}

/// Application mode: how to tell the environment "light / dark". Plasma needs
/// nothing: the KDE portal derives the mode from the color scheme.
pub fn portal_actions(de: &str, phase: Phase) -> Vec<Action> {
    let dark = phase.is_dark();
    let (gs, gk) = crate::themes::COLOR_SCHEME_KEY;
    let gnome = gsettings(gs, gk, crate::themes::color_scheme_value(dark));
    match de {
        "kde" | "deepin" => Vec::new(),
        "cosmic" => vec![Action::CosmicDark { dark }, gnome],
        // The xapp portal takes the mode from its own schema.
        "cinnamon" | "cinnamon-mint" | "xfce" => {
            vec![gsettings("org.x.apps.portal", "color-scheme", if dark { "prefer-dark" } else { "prefer-light" }), gnome]
        }
        "pantheon" => {
            let (ps, pk) = crate::themes::PANTHEON_COLOR_SCHEME_KEY;
            vec![gsettings(ps, pk, crate::themes::pantheon_color_scheme_value(dark)), gnome]
        }
        _ => vec![gnome],
    }
}

/// What to apply for `phase`. `scheduled` means a run by the timer or at
/// login: then the parts the environment switches by itself are skipped. The
/// order: global theme (it carries its own scheme and icons) → scheme →
/// icons → GTK → wallpaper → application mode last (in GNOME the GTK theme
/// sets the mode by its own darkness, and the person's choice must win).
pub fn plan(cfg: &Config, de: &str, phase: Phase, scheduled: bool) -> Vec<Action> {
    if !available(de) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let named = |p: &Pair| (p.on && !p.pick(phase).trim().is_empty()).then(|| p.pick(phase).trim().to_string());
    if has_global(de) && !(scheduled && kde_native(cfg, de)) {
        if let Some(name) = named(&cfg.global) {
            out.push(Action::Theme { kind: "global", name });
        }
    }
    let covered = global_covers(cfg, de);
    for (kind, pair) in [("colors", &cfg.colors), ("icons", &cfg.icons), ("gtk", &cfg.gtk)] {
        if covered && kind != "gtk" {
            continue;
        }
        if let Some(name) = named(pair) {
            out.push(Action::Theme { kind, name });
        }
    }
    if let Some(path) = named(&cfg.wallpaper) {
        out.push(Action::Wallpaper { path });
    }
    if cfg.portal && !(scheduled && cosmic_native(cfg, de)) {
        out.extend(portal_actions(de, phase));
    }
    out
}

// ============================================================================
// Files: timer, service, autostart, Plasma and COSMIC settings
// ============================================================================

/// The paths of one user.
#[derive(Debug, Clone, PartialEq)]
pub struct Paths {
    pub config_home: String,
    pub state_home: String,
}

impl Paths {
    pub fn live() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Paths {
            config_home: crate::utils::config_home(env("XDG_CONFIG_HOME").as_deref(), &home),
            state_home: crate::utils::state_home(env("XDG_STATE_HOME").as_deref(), &home),
        }
    }
    pub fn config(&self) -> String {
        format!("{}/rexon/day-night.json", self.config_home)
    }
    /// Rexon's general settings (they contain the wallpaper playlist).
    pub fn settings(&self) -> String {
        format!("{}/rexon/settings.json", self.config_home)
    }
    pub fn memo(&self) -> String {
        format!("{}/rexon/day-night.json", self.state_home)
    }
    pub fn unit(&self, name: &str) -> String {
        format!("{}/{name}", crate::update_guard::user_unit_dir(&self.config_home))
    }
    pub fn autostart(&self) -> String {
        format!("{}/autostart/{AUTOSTART_FILE}", self.config_home)
    }
    pub fn cosmic_mode(&self, key: &str) -> String {
        format!("{}/cosmic/com.system76.CosmicTheme.Mode/v1/{key}", self.config_home)
    }
}

/// The timer text. By time: two daily moments on the local clock; by the
/// sun: the exact moments of the next switches in UTC (after each switch the
/// service rewrites the timer) plus a daily check in case of polar day or
/// night. The description is in English: `systemctl` reads it.
pub fn timer_text(cfg: &Config, sun_events: &[i64]) -> String {
    let mut calendar = String::new();
    match cfg.schedule {
        Schedule::Times => {
            for at in [&cfg.light_at, &cfg.dark_at] {
                if let Some(m) = parse_hhmm(at) {
                    calendar.push_str(&format!("OnCalendar=*-*-* {:02}:{:02}:00\n", m / 60, m % 60));
                }
            }
        }
        Schedule::Sun => {
            for t in sun_events {
                calendar.push_str(&format!("OnCalendar={}\n", utc_calendar(*t)));
            }
            calendar.push_str("OnCalendar=*-*-* 00:10:00\n");
        }
    }
    format!(
        "[Unit]\n\
         Description=Rexon: switch between light and dark appearance\n\n\
         [Timer]\n\
         {calendar}\
         AccuracySec=30s\n\
         # A switch missed while the machine was off happens at the next start.\n\
         Persistent=true\n\
         Unit={SERVICE_UNIT}\n\n\
         [Install]\n\
         WantedBy=timers.target\n"
    )
}

/// The service text.
pub fn service_text(exec_start: &str, program: Option<&str>) -> String {
    let condition = program
        .filter(|p| p.starts_with('/'))
        .map(|p| format!("ConditionPathExists={p}\n"))
        .unwrap_or_default();
    format!(
        "[Unit]\n\
         Description=Rexon: apply the light or dark appearance\n\
         {condition}\n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={exec_start}\n\
         TimeoutStartSec=120\n\
         Nice=5\n"
    )
}

/// The autostart entry: apply the current state at login.
pub fn autostart_text(exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Rexon Light and Dark\n\
         Exec={exec}\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-Delay=3\n"
    )
}

fn kwrite(file: &str, group: &str, key: &str, value: &str) -> Vec<String> {
    ["kwriteconfig6", "--file", file, "--group", group, "--key", key, "--notify", value]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Plasma settings: global theme switching and the shared schedule
/// (`knighttimerc`, the same as "System Settings → Day-Night Cycle").
/// `currently_auto` is whether Plasma's switching is on right now: choosing
/// "Light"/"Dark" turns it off, "do not manage" leaves it alone.
pub fn kde_native_argv(cfg: &Config, currently_auto: bool) -> Vec<Vec<String>> {
    if !kde_native(cfg, "kde") {
        return if currently_auto && cfg.mode != Mode::Off {
            vec![kwrite("kdeglobals", "KDE", "AutomaticLookAndFeel", "false")]
        } else {
            Vec::new()
        };
    }
    let mut out = vec![
        kwrite("kdeglobals", "KDE", "DefaultLightLookAndFeel", cfg.global.light.trim()),
        kwrite("kdeglobals", "KDE", "DefaultDarkLookAndFeel", cfg.global.dark.trim()),
    ];
    match cfg.schedule {
        Schedule::Times => {
            out.push(kwrite("knighttimerc", "General", "Source", "Times"));
            for (key, at) in [("SunriseStart", &cfg.light_at), ("SunsetStart", &cfg.dark_at)] {
                if let Some(m) = parse_hhmm(at) {
                    out.push(kwrite("knighttimerc", "Times", key, &format!("{:02}:{:02}:00", m / 60, m % 60)));
                }
            }
        }
        Schedule::Sun => {
            out.push(kwrite("knighttimerc", "General", "Source", "Location"));
            match (cfg.auto_location, cfg.lat, cfg.lon) {
                (false, Some(lat), Some(lon)) => {
                    out.push(kwrite("knighttimerc", "Location", "Automatic", "false"));
                    out.push(kwrite("knighttimerc", "Location", "Latitude", &format!("{lat}")));
                    out.push(kwrite("knighttimerc", "Location", "Longitude", &format!("{lon}")));
                }
                _ => out.push(kwrite("knighttimerc", "Location", "Automatic", "true")),
            }
        }
    }
    // Last: the Plasma module rereads the settings on this key.
    out.push(kwrite("kdeglobals", "KDE", "AutomaticLookAndFeel", "true"));
    out
}

// ============================================================================
// Input/output
// ============================================================================

/// The machine: files, commands, clock.
pub trait DayNightIo {
    fn read(&self, path: &str) -> Option<String>;
    fn write(&self, path: &str, text: &str) -> bool;
    fn remove(&self, path: &str) -> bool;
    fn list_dir(&self, path: &str) -> Vec<String>;
    fn run(&self, argv: &[String]) -> (bool, String);
    fn have(&self, tool: &str) -> bool;
    /// Applies a theme of an "Appearance" section (`kind` is `apply_kind`).
    fn apply_theme(&self, de: &str, kind: &str, name: &str) -> (bool, String);
    /// Sets a picture as the wallpaper.
    fn apply_wallpaper(&self, path: &str) -> (bool, String);
}

/// The real machine.
pub struct SystemDayNightIo;

impl DayNightIo for SystemDayNightIo {
    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }
    fn write(&self, path: &str, text: &str) -> bool {
        crate::animations::LookIo::write(&crate::animations::SystemIo, path, text)
    }
    fn remove(&self, path: &str) -> bool {
        match std::fs::remove_file(path) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        }
    }
    fn list_dir(&self, path: &str) -> Vec<String> {
        crate::animations::LookIo::list_dir(&crate::animations::SystemIo, path)
    }
    fn run(&self, argv: &[String]) -> (bool, String) {
        crate::animations::LookIo::run(&crate::animations::SystemIo, argv)
    }
    fn have(&self, tool: &str) -> bool {
        crate::animations::LookIo::have(&crate::animations::SystemIo, tool)
    }
    fn apply_theme(&self, de: &str, kind: &str, name: &str) -> (bool, String) {
        let home = std::env::var("HOME").unwrap_or_default();
        let Some(cat) = crate::categories::category_by_kind(Path::new(&home), kind) else {
            return (false, String::new());
        };
        let item = crate::models::ThemeItem { category_id: cat.id, name: name.to_string(), ..Default::default() };
        let mut installer = crate::appearance_installer::LiveInstaller::new(
            de,
            Box::new(|line: &str| crate::log_info!("day-night", "{line}")),
        );
        installer.apply(&item, name)
    }
    fn apply_wallpaper(&self, path: &str) -> (bool, String) {
        if !Path::new(path).is_file() {
            return (false, trf("Wallpaper file not found: %1", &[&path]));
        }
        crate::wallpaper_exec::with_slot(&crate::wallpaper_exec::live_slot(), |exec| {
            let res = exec.apply(path, "static", "");
            (res.ok, res.message)
        })
    }
}

pub fn read_config(io: &dyn DayNightIo, paths: &Paths) -> Config {
    io.read(&paths.config()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn read_memo(io: &dyn DayNightIo, paths: &Paths) -> Memo {
    io.read(&paths.memo()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write_json<T: Serialize>(io: &dyn DayNightIo, path: &str, v: &T) -> bool {
    io.write(path, &(serde_json::to_string_pretty(v).unwrap_or_default() + "\n"))
}

/// The clock and the location for the decision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    /// unix, s.
    pub now: i64,
    /// Local time minus UTC, s.
    pub offset: i64,
}

impl Clock {
    pub fn live() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        // The local clock is the product's shared point (`Timestamp::now_local`);
        // the offset is the difference from UTC of the same second.
        let offset = crate::qt_app::Timestamp::now_local()
            .map(|ts| {
                let local = days_from_civil(i64::from(ts.year), ts.month, ts.day) * 86_400
                    + i64::from(ts.hour * 3600 + ts.minute * 60 + ts.second);
                // The second may have changed between the two reads: round to the minute.
                ((local - now) as f64 / 60.0).round() as i64 * 60
            })
            .unwrap_or(0);
        Clock { now, offset }
    }
    fn local_min(&self) -> u32 {
        ((self.now + self.offset).rem_euclid(86_400) / 60) as u32
    }
}

/// The location for the sun calculation: one's own or by the time zone.
pub fn location(io: &dyn DayNightIo, cfg: &Config, tz_env: Option<&str>, localtime_link: Option<&str>) -> Option<(f64, f64, String)> {
    if !cfg.auto_location {
        if let (Some(lat), Some(lon)) = (cfg.lat, cfg.lon) {
            return Some((lat, lon, String::new()));
        }
    }
    let zone = tz_name(tz_env, localtime_link)?;
    let tab = io.read("/usr/share/zoneinfo/zone1970.tab").or_else(|| io.read("/usr/share/zoneinfo/zone.tab"))?;
    let (lat, lon) = zone_coords(&tab, &zone)?;
    Some((lat, lon, zone_place(&zone)))
}

fn live_tz() -> (Option<String>, Option<String>) {
    (
        std::env::var("TZ").ok(),
        std::fs::read_link("/etc/localtime").ok().map(|p| p.to_string_lossy().to_string()),
    )
}

/// Light or dark right now. Outside automatic mode, the chosen one.
pub fn current_phase(cfg: &Config, clock: Clock, place: Option<(f64, f64)>) -> Phase {
    match cfg.mode {
        Mode::Dark => Phase::Dark,
        Mode::Light | Mode::Off => Phase::Light,
        Mode::Auto => match cfg.schedule {
            Schedule::Times => match (parse_hhmm(&cfg.light_at), parse_hhmm(&cfg.dark_at)) {
                (Some(l), Some(d)) => phase_by_times(clock.local_min(), l, d),
                _ => Phase::Light,
            },
            Schedule::Sun => place.map(|(lat, lon)| phase_by_sun(clock.now, lat, lon)).unwrap_or(Phase::Light),
        },
    }
}

/// Executes the plan. Returns the failures (one line per part).
pub fn run_plan(io: &dyn DayNightIo, paths: &Paths, de: &str, actions: &[Action]) -> Vec<String> {
    let mut failed = Vec::new();
    // One owner of the wallpaper: if the Rexon wallpaper playlist is playing,
    // the schedule's picture pair does not touch the screen (themes and schemes
    // switch as usual).
    let playlist_owns = io
        .read(&paths.settings())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().map(crate::wallpaper_playlist::owns_wallpaper))
        .unwrap_or(false);
    for action in actions {
        let (ok, detail) = match action {
            Action::Theme { kind, name } => io.apply_theme(de, kind, name),
            Action::Wallpaper { .. } if playlist_owns => {
                crate::log_info!("day-night", "обои не меняю: их ведёт плейлист обоев");
                (true, String::new())
            }
            Action::Wallpaper { path } => io.apply_wallpaper(path),
            Action::Run { argv } => io.run(argv),
            Action::CosmicDark { dark } => {
                (io.write(&paths.cosmic_mode("is_dark"), if *dark { "true" } else { "false" }), String::new())
            }
        };
        if !ok {
            let what = match action {
                Action::Theme { name, .. } => name.clone(),
                Action::Wallpaper { path } => path.clone(),
                Action::Run { argv } => argv.join(" "),
                Action::CosmicDark { .. } => "COSMIC".into(),
            };
            failed.push(if detail.is_empty() { what } else { format!("{what}: {detail}") });
        }
    }
    failed
}

/// What to do with the executor's files: (path, text) to write; paths to
/// delete; `systemctl --user` commands in order.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct UnitsPlan {
    pub write: Vec<(String, String)>,
    pub remove: Vec<String>,
    pub systemctl: Vec<Vec<String>>,
}

/// The plan for the timer, the service and autostart. `launch` is (the
/// `ExecStart=` line, the autostart `Exec=` line, the binary path); `None`
/// means there is nothing to launch with.
pub fn units_plan(
    paths: &Paths,
    cfg: &Config,
    de: &str,
    sun_events: &[i64],
    launch: Option<(&str, &str, Option<&str>)>,
    existing: &dyn Fn(&str) -> Option<String>,
) -> UnitsPlan {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let files = [paths.unit(TIMER_UNIT), paths.unit(SERVICE_UNIT), paths.autostart()];
    let mut plan = UnitsPlan::default();
    match (needs_timer(cfg, de), launch) {
        (true, Some((exec_start, exec_desktop, program))) => {
            let texts = [timer_text(cfg, sun_events), service_text(exec_start, program), autostart_text(exec_desktop)];
            for (path, text) in files.iter().zip(texts) {
                if existing(path).as_deref() != Some(text.as_str()) {
                    plan.write.push((path.clone(), text));
                }
            }
            if plan.write.iter().any(|(p, _)| p != &files[2]) {
                // `restart` both starts a new timer and makes the previous one pick up the
                // new moments.
                plan.systemctl = vec![v(&["daemon-reload"]), v(&["enable", TIMER_UNIT]), v(&["restart", TIMER_UNIT])];
            }
        }
        _ => {
            if existing(&files[0]).is_some() {
                plan.systemctl.push(v(&["disable", "--now", TIMER_UNIT]));
            }
            for path in files {
                if existing(&path).is_some() {
                    plan.remove.push(path);
                }
            }
            if plan.remove.iter().any(|p| !p.ends_with(AUTOSTART_FILE)) {
                plan.systemctl.push(v(&["daemon-reload"]));
            }
        }
    }
    plan
}

/// `systemctl --user` when Rexon is removed.
pub fn uninstall_paths(paths: &Paths) -> Vec<String> {
    vec![paths.unit(TIMER_UNIT), paths.unit(SERVICE_UNIT), paths.autostart()]
}

fn exec_units(io: &dyn DayNightIo, plan: &UnitsPlan) -> Result<(), String> {
    // Removal: first `disable --now` (the file is still in place), then the files.
    let (first, rest): (Vec<&Vec<String>>, Vec<&Vec<String>>) =
        plan.systemctl.iter().partition(|a| a.first().map(String::as_str) == Some("disable"));
    let systemctl = |args: &Vec<String>| -> Result<(), String> {
        let mut argv = vec!["systemctl".to_string(), "--user".to_string()];
        argv.extend(args.iter().cloned());
        let (ok, out) = io.run(&argv);
        if ok { Ok(()) } else { Err(format!("{}: {}", argv.join(" "), out.trim())) }
    };
    for args in first {
        systemctl(args)?;
    }
    for (path, text) in &plan.write {
        if !io.write(path, text) {
            return Err(trf("Couldn't write %1", &[path]));
        }
    }
    for path in &plan.remove {
        io.remove(path);
    }
    if plan.systemctl.is_empty() {
        return Ok(());
    }
    if !io.have("systemctl") {
        return Err(tr("systemd isn't available for the user session — switching on a schedule needs it."));
    }
    for args in rest {
        systemctl(args)?;
    }
    Ok(())
}

/// The moments of the next switches by the sun, for the timer.
fn sun_events_for(cfg: &Config, clock: Clock, place: Option<(f64, f64)>) -> Vec<i64> {
    match (cfg.schedule, place) {
        (Schedule::Sun, Some((lat, lon))) => next_sun_switches(clock.now, lat, lon, 2).into_iter().map(|(t, _)| t).collect(),
        _ => Vec::new(),
    }
}

/// The launch environment: from the session variables, otherwise from the last login.
fn de_of(env_de: Option<String>, memo: &Memo) -> String {
    env_de.filter(|d| !d.is_empty()).unwrap_or_else(|| memo.de.clone())
}

/// Everything machine-dependent, for a single pass.
pub struct Env<'a> {
    pub io: &'a dyn DayNightIo,
    pub paths: Paths,
    pub clock: Clock,
    /// The environment from the session variables (None means there are none: a systemd service).
    pub de: Option<String>,
    pub tz: (Option<String>, Option<String>),
    /// (`ExecStart=`, the autostart `Exec=`, the binary path).
    pub launch: Option<(String, String, Option<String>)>,
}

impl Env<'_> {
    fn place(&self, cfg: &Config) -> Option<(f64, f64)> {
        location(self.io, cfg, self.tz.0.as_deref(), self.tz.1.as_deref()).map(|(a, b, _)| (a, b))
    }
    fn launch_ref(&self) -> Option<(&str, &str, Option<&str>)> {
        self.launch.as_ref().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_deref()))
    }
}

/// Validates the settings before saving. `None` means they are valid.
pub fn validate(cfg: &Config, de: &str) -> Option<String> {
    if cfg.mode == Mode::Auto && cfg.schedule == Schedule::Times {
        match (parse_hhmm(&cfg.light_at), parse_hhmm(&cfg.dark_at)) {
            (Some(l), Some(d)) if l != d => {}
            (Some(_), Some(_)) => return Some(tr("The light and dark times must differ.")),
            _ => return Some(tr("Enter the times as HH:MM, for example 07:00.")),
        }
    }
    if cfg.mode == Mode::Auto && cfg.schedule == Schedule::Sun && !cfg.auto_location && (cfg.lat.is_none() || cfg.lon.is_none()) {
        return Some(tr("Enter the latitude and longitude, or let Rexon find them by the time zone."));
    }
    let pairs = [(&cfg.global, has_global(de)), (&cfg.colors, true), (&cfg.gtk, true), (&cfg.icons, true), (&cfg.wallpaper, true)];
    if pairs.iter().any(|(p, here)| *here && p.on && (p.light.trim().is_empty() || p.dark.trim().is_empty())) {
        return Some(tr("Choose both the light and the dark variant for every part that is switched on."));
    }
    let any = pairs.iter().any(|(p, here)| *here && p.on) || (cfg.portal && de != "kde");
    if cfg.mode != Mode::Off && !any {
        return Some(tr("Switch on at least one part to change."));
    }
    None
}

/// "Save": writes the settings, configures Plasma/COSMIC, installs or removes
/// the timer and applies the current state. Returns (succeeded, message).
pub fn save(env: &Env, cfg: &Config) -> (bool, String) {
    let io = env.io;
    let memo = read_memo(io, &env.paths);
    let de = de_of(env.de.clone(), &memo);
    if !available(&de) {
        return (false, unavailable_text());
    }
    if let Some(why) = validate(cfg, &de) {
        return (false, why);
    }
    if !write_json(io, &env.paths.config(), cfg) {
        return (false, trf("Couldn't write %1", &[&env.paths.config()]));
    }
    let mut problems = Vec::new();
    if de == "kde" {
        let currently = io
            .read(&format!("{}/kdeglobals", env.paths.config_home))
            .and_then(|t| crate::shell_palette::ini_get(&t, "KDE", "AutomaticLookAndFeel"))
            .is_some_and(|v| v.trim() == "true");
        for argv in kde_native_argv(cfg, currently) {
            let (ok, out) = io.run(&argv);
            if !ok {
                problems.push(format!("{}: {}", argv.join(" "), out.trim()));
                break;
            }
        }
    }
    if de == "cosmic" && cfg.mode != Mode::Off {
        let auto = cosmic_native(cfg, &de);
        if !io.write(&env.paths.cosmic_mode("auto_switch"), if auto { "true" } else { "false" }) {
            problems.push(env.paths.cosmic_mode("auto_switch"));
        }
    }
    let place = env.place(cfg);
    if cfg.mode == Mode::Auto && cfg.schedule == Schedule::Sun && place.is_none() {
        problems.push(tr("Couldn't find your location by the time zone — enter the latitude and longitude."));
    }
    let units = units_plan(&env.paths, cfg, &de, &sun_events_for(cfg, env.clock, place), env.launch_ref(), &|p| io.read(p));
    if needs_timer(cfg, &de) && env.launch.is_none() {
        problems.push(crate::app::MSG_NOTHING_TO_LAUNCH.to_string());
    }
    if let Err(e) = exec_units(io, &units) {
        problems.push(e);
    }
    let failed = if cfg.mode == Mode::Off {
        Vec::new()
    } else {
        let phase = current_phase(cfg, env.clock, place);
        let failed = run_plan(io, &env.paths, &de, &plan(cfg, &de, phase, true));
        write_json(io, &env.paths.memo(), &Memo { de: de.clone(), phase: Some(phase), at: env.clock.now });
        failed
    };
    if !failed.is_empty() {
        problems.push(trf("Not applied: %1", &[&failed.join("; ")]));
    }
    if problems.is_empty() {
        let msg = match cfg.mode {
            Mode::Off => tr("Rexon no longer switches between light and dark."),
            Mode::Light => tr("The light appearance is applied."),
            Mode::Dark => tr("The dark appearance is applied."),
            Mode::Auto => tr("Saved. The appearance will switch by itself."),
        };
        (true, msg)
    } else {
        (false, problems.join("\n"))
    }
}

/// `rexon --apply-day-night [--login]`: applies the current state and, for
/// the sun schedule, rewrites the timer for the next switches. Exit code 0
/// means applied or nothing to do; 1 means some part was not applied.
pub fn run_scheduled(env: &Env, _login: bool, out: &mut dyn FnMut(&str)) -> i32 {
    let io = env.io;
    let cfg = read_config(io, &env.paths);
    let mut memo = read_memo(io, &env.paths);
    let de = de_of(env.de.clone(), &memo);
    if cfg.mode != Mode::Auto || !available(&de) {
        // A timer is left over from previous settings: remove it.
        let units = units_plan(&env.paths, &Config { mode: Mode::Off, ..cfg.clone() }, &de, &[], None, &|p| io.read(p));
        let _ = exec_units(io, &units);
        return 0;
    }
    let place = env.place(&cfg);
    let phase = current_phase(&cfg, env.clock, place);
    let failed = run_plan(io, &env.paths, &de, &plan(&cfg, &de, phase, true));
    memo = Memo { de: de.clone(), phase: Some(phase), at: env.clock.now };
    write_json(io, &env.paths.memo(), &memo);
    let units = units_plan(&env.paths, &cfg, &de, &sun_events_for(&cfg, env.clock, place), env.launch_ref(), &|p| io.read(p));
    if let Err(e) = exec_units(io, &units) {
        out(&e);
    }
    for f in &failed {
        out(f);
    }
    i32::from(!failed.is_empty())
}

// ============================================================================
// The block snapshot for the window
// ============================================================================

/// A drop-down list item.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

/// One part in the window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PartView {
    pub id: &'static str,
    pub label: String,
    pub on: bool,
    pub light: String,
    pub dark: String,
    pub items: Vec<Choice>,
    /// A file choice rather than a list (wallpaper).
    pub file: bool,
}

/// The block snapshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct State {
    pub available: bool,
    pub sections: Vec<String>,
    pub title: String,
    pub note: String,
    pub config: Config,
    pub modes: Vec<Choice>,
    pub schedules: Vec<Choice>,
    pub parts: Vec<PartView>,
    pub portal_label: String,
    pub light_label: String,
    pub dark_label: String,
    pub light_from_label: String,
    pub dark_from_label: String,
    pub lat_label: String,
    pub lon_label: String,
    pub auto_location_label: String,
    pub place: String,
    pub pick_label: String,
    pub save_label: String,
    /// "It is light now. The next switch is to dark at 17:41."
    pub status: String,
    /// The line of the collapsed block: what is chosen now ("Light and dark: on a
    /// schedule 07:00–19:00").
    pub summary: String,
    /// Whether the block is expanded. Collapsed by default: when expanded it took
    /// half the screen above the theme grid. Remembered between runs
    /// ([`EXPANDED_KEY`]).
    pub expanded: bool,
    /// What the environment switches by itself.
    pub native_note: String,
}

fn unavailable_text() -> String {
    tr("In Deepin, the light and dark appearance is chosen only in the Control Center (it has its own “Automatic”).")
}

fn lower_dirs(home: &str, rel: &[&str]) -> Vec<String> {
    rel.iter()
        .map(|r| r.replace('~', home))
        .collect()
}

/// The name from the global theme's `metadata.json`.
fn lnf_label(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    v.pointer("/KPlugin/Name").and_then(Value::as_str).map(str::to_string)
}

/// What is installed for a part: (name for applying, label).
pub fn list_items(io: &dyn DayNightIo, home: &str, kind: &str) -> Vec<Choice> {
    let mut out: Vec<Choice> = Vec::new();
    let mut push = |id: String, label: String| {
        if !id.is_empty() && !out.iter().any(|c| c.id == id) {
            out.push(Choice { id, label });
        }
    };
    match kind {
        "global" => {
            for dir in lower_dirs(home, &["~/.local/share/plasma/look-and-feel", "/usr/local/share/plasma/look-and-feel", "/usr/share/plasma/look-and-feel"]) {
                for name in io.list_dir(&dir) {
                    if let Some(meta) = io.read(&format!("{dir}/{name}/metadata.json")) {
                        let label = lnf_label(&meta).unwrap_or_else(|| name.clone());
                        push(name, label);
                    }
                }
            }
        }
        "colors" => {
            for dir in lower_dirs(home, &["~/.local/share/color-schemes", "/usr/local/share/color-schemes", "/usr/share/color-schemes"]) {
                for name in io.list_dir(&dir) {
                    let Some(stem) = name.strip_suffix(".colors") else { continue };
                    let label = io
                        .read(&format!("{dir}/{name}"))
                        .and_then(|t| crate::shell_palette::ini_get(&t, "General", "Name"))
                        .filter(|n| !n.trim().is_empty())
                        .unwrap_or_else(|| stem.to_string());
                    push(stem.to_string(), label);
                }
            }
        }
        "gtk" => {
            for dir in lower_dirs(home, &["~/.themes", "~/.local/share/themes", "/usr/local/share/themes", "/usr/share/themes"]) {
                for name in io.list_dir(&dir) {
                    if !io.list_dir(&format!("{dir}/{name}/gtk-3.0")).is_empty() {
                        push(name.clone(), name);
                    }
                }
            }
        }
        "icons" => {
            for dir in lower_dirs(home, &["~/.icons", "~/.local/share/icons", "/usr/local/share/icons", "/usr/share/icons"]) {
                for name in io.list_dir(&dir) {
                    if name == "default" || name == "hicolor" || name == "locolor" {
                        continue;
                    }
                    let Some(index) = io.read(&format!("{dir}/{name}/index.theme")) else { continue };
                    // A cursor theme without icons does not belong here.
                    let dirs = crate::shell_palette::ini_get(&index, "Icon Theme", "Directories").unwrap_or_default();
                    if dirs.trim().is_empty() {
                        continue;
                    }
                    let label = crate::shell_palette::ini_get(&index, "Icon Theme", "Name").unwrap_or_else(|| name.clone());
                    push(name, label);
                }
            }
        }
        _ => {}
    }
    out.sort_by_key(|c| c.label.to_lowercase());
    out
}

fn norm_name(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// A pair by names: the dark one has "dark" in its name, the light one is the
/// same name without it (`Adwaita` / `Adwaita-dark`, `org.kde.breeze.desktop` /
/// `org.kde.breezedark.desktop`, `BreezeLight` / `BreezeDark`).
pub fn suggest_pair(items: &[Choice]) -> Option<(String, String)> {
    for dark in items.iter().filter(|c| c.id.to_lowercase().contains("dark")) {
        let base = norm_name(&dark.id.to_lowercase().replacen("dark", "", 1));
        let light = items.iter().find(|c| {
            c.id != dark.id && {
                let n = norm_name(&c.id);
                n == base || n == format!("{base}light") || n.replace("light", "") == base
            }
        });
        if let Some(light) = light {
            return Some((light.id.clone(), dark.id.clone()));
        }
    }
    None
}

fn status_text(cfg: &Config, clock: Clock, place: Option<(f64, f64)>) -> String {
    let name = |p: Phase| match p {
        Phase::Light => trc("light and dark", "light"),
        Phase::Dark => trc("light and dark", "dark"),
    };
    if cfg.mode != Mode::Auto {
        return String::new();
    }
    let now = current_phase(cfg, clock, place);
    let next: Option<(String, Phase)> = match cfg.schedule {
        Schedule::Times => match (parse_hhmm(&cfg.light_at), parse_hhmm(&cfg.dark_at)) {
            (Some(l), Some(d)) => next_time_switches(clock.local_min(), l, d)
                .first()
                .map(|(m, p)| (local_hhmm(clock.now + i64::from(*m) * 60, clock.offset), *p)),
            _ => None,
        },
        Schedule::Sun => place.and_then(|(lat, lon)| {
            next_sun_switches(clock.now, lat, lon, 1).first().map(|(t, p)| (local_hhmm(*t, clock.offset), *p))
        }),
    };
    match next {
        Some((at, p)) => trf("Now: %1. Next switch: %2 at %3.", &[&name(now), &name(p), &at]),
        None => trf("Now: %1.", &[&name(now)]),
    }
}

/// The `settings.json` key: whether the "Light and dark" block is expanded.
pub const EXPANDED_KEY: &str = "day_night_expanded";

/// The line of the collapsed block: what is chosen.
fn summary_text(cfg: &Config) -> String {
    match (cfg.mode, cfg.schedule) {
        (Mode::Off, _) => tr("Light and dark: off"),
        (Mode::Light, _) => trf("Light and dark: manually, %1", &[&trc("light and dark", "light")]),
        (Mode::Dark, _) => trf("Light and dark: manually, %1", &[&trc("light and dark", "dark")]),
        (Mode::Auto, Schedule::Times) => trf("Light and dark: on schedule %1–%2", &[&cfg.light_at, &cfg.dark_at]),
        (Mode::Auto, Schedule::Sun) => tr("Light and dark: sunset to sunrise"),
    }
}

fn native_note(cfg: &Config, de: &str) -> String {
    match de {
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        "kde" => tr("In Plasma, Rexon hands the global theme switch to Plasma itself (System Settings → Global Theme, “Automatically”) and sets its schedule — the same “Day-night cycle” schedule that Night Light uses. The color scheme and icons then come from the global theme; Rexon switches the other parts itself."),
        "cosmic" if cfg.schedule == Schedule::Sun => tr("In COSMIC, the light and dark mode by sunrise and sunset is switched by COSMIC itself; Rexon only turns it on. Times of day COSMIC can't do — then Rexon switches it."),
        "cosmic" => tr("COSMIC switches only by sunrise and sunset; for times of day Rexon switches the mode itself."),
        _ => tr("This desktop has no schedule of its own: Rexon switches the parts itself at the right moment (a systemd user timer, no program stays running)."),
    }
}

/// The block snapshot for the window.
pub fn state(env: &Env, home: &str) -> State {
    let io = env.io;
    let memo = read_memo(io, &env.paths);
    let de = de_of(env.de.clone(), &memo);
    let mut cfg = read_config(io, &env.paths);
    let loc = location(io, &cfg, env.tz.0.as_deref(), env.tz.1.as_deref());
    if cfg.lat.is_none() || cfg.lon.is_none() {
        if let Some((lat, lon, _)) = &loc {
            cfg.lat = Some((lat * 100.0).round() / 100.0);
            cfg.lon = Some((lon * 100.0).round() / 100.0);
        }
    }
    let mut parts = Vec::new();
    let defs: [(&'static str, String, bool); 5] = [
        ("global", trc("light and dark", "Global theme"), has_global(&de)),
        ("colors", tr("Color scheme"), true),
        ("gtk", trc("light and dark", "GTK theme"), true),
        ("icons", trc("light and dark", "Icons"), true),
        ("wallpaper", tr("Wallpaper"), true),
    ];
    for (id, label, here) in defs {
        if !here {
            continue;
        }
        let pair = match id {
            "global" => &cfg.global,
            "colors" => &cfg.colors,
            "gtk" => &cfg.gtk,
            "icons" => &cfg.icons,
            _ => &cfg.wallpaper,
        };
        let items = if id == "wallpaper" { Vec::new() } else { list_items(io, home, id) };
        let (mut light, mut dark) = (pair.light.clone(), pair.dark.clone());
        if light.is_empty() && dark.is_empty() {
            if let Some((l, d)) = suggest_pair(&items) {
                light = l;
                dark = d;
            }
        }
        parts.push(PartView { id, label, on: pair.on, light, dark, items, file: id == "wallpaper" });
    }
    let sections = if de == "kde" { SECTIONS_KDE } else { SECTIONS_OTHER };
    State {
        available: available(&de),
        sections: sections.iter().map(|s| s.to_string()).collect(),
        title: tr("Light and dark"),
        note: if available(&de) { String::new() } else { unavailable_text() },
        status: status_text(&cfg, env.clock, loc.as_ref().map(|(a, b, _)| (*a, *b))),
        summary: summary_text(&cfg),
        expanded: false,
        native_note: native_note(&cfg, &de),
        place: loc.map(|(_, _, p)| p).filter(|p| !p.is_empty()).map(|p| trf("By the time zone: %1", &[&p])).unwrap_or_default(),
        config: cfg,
        modes: vec![
            Choice { id: "light".into(), label: trc("light and dark", "Light") },
            Choice { id: "dark".into(), label: trc("light and dark", "Dark") },
            Choice { id: "auto".into(), label: tr("Automatically") },
        ],
        schedules: vec![
            Choice { id: "sun".into(), label: tr("Sunset to sunrise") },
            Choice { id: "times".into(), label: tr("At set times") },
        ],
        parts,
        portal_label: tr("Light or dark mode of applications"),
        light_label: trc("light and dark", "Light"),
        dark_label: trc("light and dark", "Dark"),
        light_from_label: tr("Light from"),
        dark_from_label: tr("Dark from"),
        lat_label: tr("Latitude"),
        lon_label: tr("Longitude"),
        auto_location_label: tr("Find the location by the time zone"),
        pick_label: tr("Choose…"),
        save_label: tr("Save"),
    }
}

// ============================================================================
// Live entry points
// ============================================================================

fn live_env(io: &dyn DayNightIo) -> Env<'_> {
    let env_str = |k: &str| std::env::var(k).unwrap_or_default();
    let de = crate::system::detect_current_de(
        &env_str("XDG_CURRENT_DESKTOP"),
        &env_str("DESKTOP_SESSION"),
        !env_str("WAYLAND_DISPLAY").is_empty(),
    );
    // The section of the "Appearance" tab follows the "Current environment"
    // choice; a run by the timer (a separate process) does not know that choice
    // and takes the session.
    let de = match crate::animations::LookCtx::from_env().for_chosen(&crate::animations::chosen_desktop()).de {
        chosen if !chosen.is_empty() && de.as_deref() != Some(chosen.as_str()) => Some(chosen),
        _ => de,
    };
    let entry = crate::installer::launch_entry_live();
    let program = match &entry {
        crate::installer::LaunchEntry::MainWindow(p) => Some(p.clone()),
        crate::installer::LaunchEntry::Python => None,
    };
    let rexon_path = crate::utils::which("rexon", std::env::var("PATH").ok().as_deref());
    let exec_start = crate::app::unit_launch_command(&entry, rexon_path.as_deref(), &[CLI_FLAG]);
    let exec_desktop = crate::app::launch_command(&entry, rexon_path.as_deref(), &[CLI_FLAG, crate::aeroshell::LOGIN_FLAG]);
    Env {
        io,
        paths: Paths::live(),
        clock: Clock::live(),
        de,
        tz: live_tz(),
        launch: exec_start.zip(exec_desktop).map(|(a, b)| (a, b, program)),
    }
}

/// The snapshot for the window (JSON).
pub fn live_state() -> State {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut st = state(&live_env(&SystemDayNightIo), &home);
    st.expanded = crate::settings::load_settings(&crate::settings::settings_file())
        .get(EXPANDED_KEY)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    st
}

/// Remembers whether the block is expanded. `false` means it could not be written.
pub fn live_set_expanded(on: bool) -> bool {
    crate::settings::SettingsManager::new()
        .and_then(|mut m| m.set(EXPANDED_KEY, serde_json::Value::from(on)))
        .is_ok()
}

/// "Save" from the window: `json` is a [`Config`].
pub fn live_save(json: &str) -> (bool, String) {
    match serde_json::from_str::<Config>(json) {
        Ok(cfg) => save(&live_env(&SystemDayNightIo), &cfg),
        Err(e) => (false, e.to_string()),
    }
}

/// The `--apply-day-night` route.
pub fn cli_route(args: &[String], out: &mut dyn FnMut(&str)) -> i32 {
    let login = args.iter().any(|a| a == crate::aeroshell::LOGIN_FLAG);
    run_scheduled(&live_env(&SystemDayNightIo), login, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line of the collapsed block names what is chosen.
    #[test]
    fn строка_свёрнутого_блока() {
        let mut cfg = Config::default();
        assert_eq!(summary_text(&cfg), "Light and dark: off");
        cfg.mode = Mode::Dark;
        assert_eq!(summary_text(&cfg), "Light and dark: manually, dark");
        cfg.mode = Mode::Auto;
        cfg.schedule = Schedule::Times;
        assert_eq!(summary_text(&cfg), "Light and dark: on schedule 07:00–19:00");
        cfg.schedule = Schedule::Sun;
        assert_eq!(summary_text(&cfg), "Light and dark: sunset to sunrise");
    }

    #[test]
    fn времена_и_переход_через_полночь() {
        assert_eq!(parse_hhmm("07:30"), Some(450));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(phase_by_times(8 * 60, 7 * 60, 19 * 60), Phase::Light);
        assert_eq!(phase_by_times(20 * 60, 7 * 60, 19 * 60), Phase::Dark);
        // Light from 22:00 (a night shift) to 06:00.
        assert_eq!(phase_by_times(23 * 60, 22 * 60, 6 * 60), Phase::Light);
        assert_eq!(phase_by_times(12 * 60, 22 * 60, 6 * 60), Phase::Dark);
        assert_eq!(next_time_switches(18 * 60, 7 * 60, 19 * 60), vec![(60, Phase::Dark), (13 * 60, Phase::Light)]);
        // Exactly at the switch moment, the next one is a day away, not 0 minutes.
        assert_eq!(next_time_switches(19 * 60, 7 * 60, 19 * 60)[1], (1440, Phase::Dark));
    }

    #[test]
    fn календарь_utc() {
        assert_eq!(utc_calendar(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc_calendar(1_793_550_060), "2026-11-01 16:21:00 UTC");
        assert_eq!(days_from_civil(2026, 11, 1) * 86_400, 1_793_491_200);
        assert_eq!(local_hhmm(1_793_550_060, 3600), "17:21");
    }

    #[test]
    fn пояс_и_координаты() {
        let tab = "# comment\nDE,DK,NO,SE,SJ\t+5230+01322\tEurope/Berlin\nRU\t+554521+0373704\tEurope/Moscow\tMSK+00\n";
        assert_eq!(tz_name(None, Some("/usr/share/zoneinfo/Europe/Berlin")).as_deref(), Some("Europe/Berlin"));
        assert_eq!(tz_name(Some(":Europe/Moscow"), None).as_deref(), Some("Europe/Moscow"));
        let (lat, lon) = zone_coords(tab, "Europe/Berlin").unwrap();
        assert!((lat - 52.5).abs() < 1e-9 && (lon - (13.0 + 22.0 / 60.0)).abs() < 1e-9);
        let (lat, lon) = zone_coords(tab, "Europe/Moscow").unwrap();
        assert!((lat - 55.7558).abs() < 1e-3 && (lon - 37.6178).abs() < 1e-3, "{lat} {lon}");
        assert_eq!(zone_place("America/New_York"), "New York");
    }
}
