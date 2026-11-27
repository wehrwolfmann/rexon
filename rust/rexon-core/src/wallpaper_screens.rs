//! Per-monitor wallpapers: which monitor is which, what to show on it and
//! when to rebuild the showing after hot-plugging.
//!
//! The module is a CORE without Qt and without side effects (except two thin
//! live wrappers reading sysfs/hwdata at the end). It is called by:
//!
//!   * the `rexon-wallpaper-host` host and its supervisor — [`plan_show`]
//!     (which processes to keep and on which screens), [`plan_change`] (what
//!     to restart after the screens change), [`HotplugSettle`] (when);
//!   * the «Apply ▸ Monitor N» page — [`identify_screens`],
//!     [`ScreenAssignments::assign`] and storage in `settings.json`
//!     ([`ScreenAssignments::from_settings`] / [`ScreenAssignments::write_settings`]);
//!   * the GNOME/Cinnamon extensions and DE executors — the same [`ShowPlan`]:
//!     a group has connector names by which the DE lays out the picture.
//!
//! ## Identifying a monitor
//!
//! The connector name (`DP-1`) is no good as a memory key: the person moved
//! the cable to the neighboring port — and "the same" monitor became another
//! one. So the key is built from the EDID — the manufacturer (PNP code), the
//! product code and the serial number — and survives a port change. The
//! connector is added to the key only where it cannot be done without it:
//! there is no serial (cheap panels, laptop eDP) AND a monitor with the same
//! key stands next to it. Two identical monitors without a serial are told
//! apart by port, a single such one follows the person from port to port.
//!
//! The EDID is taken from `/sys/class/drm/card*-<connector>/edid` and parsed
//! here with our own code ([`parse_edid`], the block structure follows the
//! VESA E-EDID 1.3/1.4 standard: header, manufacturer code, product code,
//! serial, descriptors 0xFC/0xFF/0xFE). Qt knows its own about the same
//! monitor (`QScreen::name/manufacturer/model/serialNumber`), and differently
//! on different platforms: on X11 the fields come from the EDID, on Wayland
//! from `wl_output` (there is usually no serial there), some drivers have
//! their own X11 connector names (`HDMI-0`, `DisplayPort-0`). So a Qt screen
//! is matched with a DRM output by points ([`match_screens`]): a matching
//! serial weighs the most, then the connector, the model and the
//! manufacturer; contradicting serials forbid the pair altogether. Not
//! matched — the key is built from what Qt said, and if there is not even
//! that — from the connector name.
//!
//! ## Assignments and compatibility
//!
//! `settings.json`, the [`KEY_SCREENS`] key: the «on all screens» wallpaper
//! and wallpapers by monitor keys. No key (all previous installs) means "one
//! wallpaper on all screens", exactly how Rexon worked before this module. The
//! assignment of a disconnected monitor is NOT erased: connected again — its
//! wallpaper is back.
//!
//! ## Showing
//!
//! One process per group of screens with the same wallpaper — the same
//! wallpaper on two monitors must not decode the video twice; different
//! wallpapers — different processes.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

// ============================================================================
// EDID
// ============================================================================

/// Length of the EDID base block.
pub const EDID_BLOCK_LEN: usize = 128;

/// Header of the EDID base block.
const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// Offsets of the four 18-byte descriptors of the base block.
const DESCRIPTOR_OFFSETS: [usize; 4] = [54, 72, 90, 108];

/// Tags of the display text descriptors.
const TAG_SERIAL_TEXT: u8 = 0xFF;
const TAG_UNSPECIFIED_TEXT: u8 = 0xFE;
const TAG_NAME: u8 = 0xFC;

/// What the core needs from a monitor's EDID.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EdidInfo {
    /// PNP manufacturer code, three capital letters (`SAM`, `DEL`).
    pub pnp_id: String,
    /// Product code (bytes 10–11, low byte first).
    pub product_code: u16,
    /// Numeric serial (bytes 12–15); `0` — the manufacturer did not fill it in.
    pub serial_number: u32,
    /// The serial as text (descriptor 0xFF), if present.
    pub serial_text: Option<String>,
    /// The monitor name (descriptor 0xFC), if present.
    pub monitor_name: Option<String>,
    /// Other text (descriptors 0xFE) — laptop panels have the model there.
    pub extra_text: Vec<String>,
    /// Year of manufacture, if given.
    pub year: Option<u16>,
    /// Whether the checksum matched. Parsing does NOT require it: some monitors
    /// have a broken checksum while the other fields are honest.
    pub checksum_ok: bool,
}

impl EdidInfo {
    /// The serial for the key: text, otherwise the number, otherwise empty.
    pub fn serial(&self) -> String {
        if let Some(t) = self.serial_text.as_deref().filter(|t| !t.is_empty()) {
            return t.to_string();
        }
        if self.serial_number != 0 {
            return self.serial_number.to_string();
        }
        String::new()
    }
}

/// Why the EDID did not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdidError {
    /// Less than one block (a disconnected connector has an empty file).
    TooShort,
    /// No `00 FF FF FF FF FF FF 00` header.
    BadHeader,
    /// The manufacturer code is outside `A..Z`.
    BadVendor,
}

/// Descriptor text: up to 13 bytes, the end is `0x0A`, the tail is spaces.
/// Non-printable characters are dropped: no garbage goes into the key and the label.
fn descriptor_text(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0x0A || b == 0x00).unwrap_or(raw.len());
    raw[..end]
        .iter()
        .filter(|b| (0x20..0x7F).contains(*b))
        .map(|&b| b as char)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Parse the EDID base block.
pub fn parse_edid(bytes: &[u8]) -> Result<EdidInfo, EdidError> {
    if bytes.len() < EDID_BLOCK_LEN {
        return Err(EdidError::TooShort);
    }
    let block = &bytes[..EDID_BLOCK_LEN];
    if block[..8] != EDID_HEADER {
        return Err(EdidError::BadHeader);
    }
    // Manufacturer: 16 bits big-endian, the top bit is zero, then three
    // letters of 5 bits each, `1` = `A`.
    let word = u16::from_be_bytes([block[8], block[9]]);
    let mut pnp_id = String::with_capacity(3);
    for shift in [10u16, 5, 0] {
        let v = ((word >> shift) & 0x1F) as u8;
        if !(1..=26).contains(&v) {
            return Err(EdidError::BadVendor);
        }
        pnp_id.push((b'A' + v - 1) as char);
    }
    let product_code = u16::from_le_bytes([block[10], block[11]]);
    let serial_number = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);
    let year = match block[17] {
        0 | 0xFF => None,
        y => Some(1990 + u16::from(y)),
    };
    let mut info = EdidInfo {
        pnp_id,
        product_code,
        serial_number,
        year,
        checksum_ok: block.iter().fold(0u8, |acc, &b| acc.wrapping_add(b)) == 0,
        ..EdidInfo::default()
    };
    for off in DESCRIPTOR_OFFSETS {
        let d = &block[off..off + 18];
        // A display descriptor: the first two bytes are zero (otherwise it is a mode).
        if d[0] != 0 || d[1] != 0 || d[2] != 0 {
            continue;
        }
        let text = descriptor_text(&d[5..18]);
        if text.is_empty() {
            continue;
        }
        match d[3] {
            TAG_NAME if info.monitor_name.is_none() => info.monitor_name = Some(text),
            TAG_SERIAL_TEXT if info.serial_text.is_none() => info.serial_text = Some(text),
            TAG_UNSPECIFIED_TEXT => info.extra_text.push(text),
            _ => {}
        }
    }
    Ok(info)
}

/// The manufacturer name by PNP code from the `hwdata/pnp.ids` text
/// (`CODE<tab>Name`). Needed for matching with Qt on Wayland: compositors
/// put the name, not the code, into `wl_output`.
pub fn pnp_vendor_name(pnp_ids_text: &str, pnp_id: &str) -> Option<String> {
    pnp_ids_text.lines().find_map(|line| {
        let (code, name) = line.split_once('\t')?;
        (code.trim().eq_ignore_ascii_case(pnp_id)).then(|| name.trim().to_string()).filter(|n| !n.is_empty())
    })
}

// ============================================================================
// DRM outputs and Qt screens
// ============================================================================

/// A graphics card output from sysfs (`/sys/class/drm/card1-DP-1`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DrmOutput {
    /// The connector name without the card: `DP-1`, `HDMI-A-1`, `eDP-1`.
    pub connector: String,
    /// `status` = `connected`.
    pub connected: bool,
    /// The parsed EDID; `None` — no file, empty or broken.
    pub edid: Option<EdidInfo>,
    /// The manufacturer name from `pnp.ids`, if the caller found it.
    pub vendor_name: Option<String>,
}

/// The connector name from the sysfs directory name: `card1-HDMI-A-1` → `HDMI-A-1`.
/// Not an output directory (`card1`, `renderD128`, `version`) — `None`.
pub fn connector_from_sysfs_name(dir_name: &str) -> Option<&str> {
    let rest = dir_name.strip_prefix("card")?;
    let (num, connector) = rest.split_once('-')?;
    if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) || connector.is_empty() {
        return None;
    }
    Some(connector)
}

/// What Qt said about the screen (`QScreen`) — or any other query
/// (`wlr-randr`, `hyprctl monitors`) with the same four fields.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScreenFacts {
    /// `QScreen::name()` — the platform's connector name (`DP-1`, on X11 it can be `HDMI-0`).
    pub name: String,
    /// `QScreen::manufacturer()` — the PNP code or the manufacturer name.
    pub manufacturer: String,
    /// `QScreen::model()`.
    pub model: String,
    /// `QScreen::serialNumber()` — usually empty on Wayland.
    pub serial: String,
}

/// Connector type and number in a general form: `HDMI-A-1` and `HDMI-1` →
/// `("HDMI", 1)`, `DisplayPort-0` → `("DP", 0)`. Needed for X11, where drivers
/// name the same connectors their own way.
pub fn connector_family(name: &str) -> Option<(String, u32)> {
    let (kind, index) = name.rsplit_once('-')?;
    let index: u32 = index.parse().ok()?;
    let kind = kind.to_ascii_uppercase();
    let kind = match kind.as_str() {
        "HDMI-A" | "HDMI-B" | "HDMI" => "HDMI",
        "DP" | "DISPLAYPORT" => "DP",
        "EDP" => "EDP",
        "DVI-D" | "DVI-I" | "DVI-A" | "DVI" => "DVI",
        "VGA" => "VGA",
        "USB-C" | "USBC" => "USBC",
        other => other,
    }
    .to_string();
    Some((kind, index))
}

/// A "Qt screen — DRM output" pair is incompatible: both serials exist and differ.
fn serials_conflict(screen: &ScreenFacts, edid: &EdidInfo) -> bool {
    let qs = screen.serial.trim();
    if qs.is_empty() {
        return false;
    }
    let es = edid.serial();
    !es.is_empty() && !serial_equal(qs, edid)
}

fn serial_equal(qt_serial: &str, edid: &EdidInfo) -> bool {
    let q = qt_serial.trim();
    if q.is_empty() {
        return false;
    }
    if edid.serial_text.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(q)) {
        return true;
    }
    edid.serial_number != 0
        && (q == edid.serial_number.to_string()
            || q.eq_ignore_ascii_case(&format!("0x{:08X}", edid.serial_number))
            || q.eq_ignore_ascii_case(&format!("{:08X}", edid.serial_number)))
}

fn model_equal(qt_model: &str, edid: &EdidInfo) -> bool {
    let q = qt_model.trim();
    if q.is_empty() {
        return false;
    }
    let mut texts = edid.monitor_name.iter().chain(edid.extra_text.iter());
    texts.any(|t| t.eq_ignore_ascii_case(q))
        || q == edid.product_code.to_string()
        || q.eq_ignore_ascii_case(&format!("0x{:04X}", edid.product_code))
        || q.eq_ignore_ascii_case(&format!("{:04X}", edid.product_code))
}

fn manufacturer_equal(qt_manufacturer: &str, out: &DrmOutput, edid: &EdidInfo) -> bool {
    let q = qt_manufacturer.trim();
    if q.is_empty() {
        return false;
    }
    if q.eq_ignore_ascii_case(&edid.pnp_id) {
        return true;
    }
    out.vendor_name.as_deref().is_some_and(|v| {
        let (v, q) = (v.to_lowercase(), q.to_lowercase());
        v == q || v.starts_with(&q) || q.starts_with(&v)
    })
}

/// Points of a pair; `None` — the pair is forbidden.
///
/// Weights: serial 8, exact connector 6, model 4, connector type and number 2,
/// connector type 1, manufacturer 1. The match threshold is
/// [`MATCH_THRESHOLD`]: the manufacturer alone is not enough (a person may
/// have two Dells).
pub fn match_score(screen: &ScreenFacts, out: &DrmOutput) -> Option<u32> {
    let mut score = 0;
    if let Some(edid) = &out.edid {
        if serials_conflict(screen, edid) {
            return None;
        }
        if serial_equal(&screen.serial, edid) {
            score += 8;
        }
        if model_equal(&screen.model, edid) {
            score += 4;
        }
        if manufacturer_equal(&screen.manufacturer, out, edid) {
            score += 1;
        }
    }
    if !screen.name.is_empty() && screen.name.eq_ignore_ascii_case(&out.connector) {
        score += 6;
    } else if let (Some(a), Some(b)) = (connector_family(&screen.name), connector_family(&out.connector)) {
        if a == b {
            score += 2;
        } else if a.0 == b.0 {
            score += 1;
        }
    }
    Some(score)
}

/// The lowest points at which a pair counts as one monitor.
pub const MATCH_THRESHOLD: u32 = 2;

/// Match Qt screens to DRM outputs. The answer is one slot per screen (in
/// `screens` order): the output index in `outputs` or `None`. Only connected
/// outputs are taken into account; each output goes to no more than one
/// screen, the best pairs first.
pub fn match_screens(screens: &[ScreenFacts], outputs: &[DrmOutput]) -> Vec<Option<usize>> {
    let mut pairs = Vec::new();
    for (si, s) in screens.iter().enumerate() {
        for (oi, o) in outputs.iter().enumerate() {
            if !o.connected {
                continue;
            }
            if let Some(score) = match_score(s, o).filter(|&sc| sc >= MATCH_THRESHOLD) {
                pairs.push((score, si, oi));
            }
        }
    }
    // The best pairs first; on a tie — in order, so that the answer does not
    // depend on the sorting.
    pairs.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let mut result = vec![None; screens.len()];
    let mut taken = vec![false; outputs.len()];
    for (_, si, oi) in pairs {
        if result[si].is_none() && !taken[oi] {
            result[si] = Some(oi);
            taken[oi] = true;
        }
    }
    result
}

// ============================================================================
// Monitor key
// ============================================================================

/// The "key@connector" separator for identical monitors without a serial.
pub const CONNECTOR_SEP: char = '@';

/// Who is who among the connected screens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScreenIdentity {
    /// The assignment memory key. For identical monitors — with the connector.
    pub key: String,
    /// The key without the connector (what the monitor is by itself).
    pub base: String,
    /// The screen name in Qt — the host finds the `QScreen` by it.
    pub screen_name: String,
    /// The DRM connector name if the screen is matched; otherwise the Qt name.
    pub connector: String,
    /// The model for the label (`DELL U2723QE`); empty — label with the connector.
    pub label: String,
    /// The key was built from the EDID (not from Qt's words or the connector name).
    pub from_edid: bool,
}

/// Clean a key part: no separators and no surrounding spaces.
fn key_part(s: &str) -> String {
    s.trim().chars().map(|c| if c == ':' || c == CONNECTOR_SEP { '_' } else { c }).collect()
}

/// A key from the EDID: `edid:SAM:7140:H4ZR123456` (the product code is 4 hex digits).
pub fn edid_base_key(edid: &EdidInfo) -> String {
    format!("edid:{}:{:04X}:{}", edid.pnp_id, edid.product_code, key_part(&edid.serial()))
}

/// A key from Qt's words: `qt:Manufacturer:Model:Serial`; all three empty —
/// `port:<name>`.
pub fn screen_base_key(screen: &ScreenFacts) -> String {
    let (m, md, s) = (key_part(&screen.manufacturer), key_part(&screen.model), key_part(&screen.serial));
    if md.is_empty() && s.is_empty() {
        // The manufacturer alone is not enough for a key — that is a brand, not a monitor.
        return format!("port:{}", key_part(&screen.name));
    }
    format!("qt:{m}:{md}:{s}")
}

/// Identify the connected screens: the key, connector and label of each.
///
/// Identical keys on two screens (two identical monitors without a serial or
/// with the same serial) get the connector into the key — their wallpapers
/// differ if the person assigned it so.
pub fn identify_screens(screens: &[ScreenFacts], outputs: &[DrmOutput]) -> Vec<ScreenIdentity> {
    let matched = match_screens(screens, outputs);
    let mut ids: Vec<ScreenIdentity> = screens
        .iter()
        .zip(matched)
        .map(|(s, m)| {
            let out = m.map(|i| &outputs[i]);
            match out.and_then(|o| o.edid.as_ref().map(|e| (o, e))) {
                Some((o, e)) => ScreenIdentity {
                    key: String::new(),
                    base: edid_base_key(e),
                    screen_name: s.name.clone(),
                    connector: o.connector.clone(),
                    label: e
                        .monitor_name
                        .clone()
                        .or_else(|| e.extra_text.last().cloned())
                        .unwrap_or_else(|| s.model.trim().to_string()),
                    from_edid: true,
                },
                None => ScreenIdentity {
                    key: String::new(),
                    base: screen_base_key(s),
                    screen_name: s.name.clone(),
                    connector: out.map(|o| o.connector.clone()).unwrap_or_else(|| s.name.clone()),
                    label: s.model.trim().to_string(),
                    from_edid: false,
                },
            }
        })
        .collect();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for id in &ids {
        *counts.entry(id.base.clone()).or_default() += 1;
    }
    for id in &mut ids {
        id.key = if counts[&id.base] > 1 {
            format!("{}{CONNECTOR_SEP}{}", id.base, id.connector)
        } else {
            id.base.clone()
        };
    }
    ids
}

// ============================================================================
// Assignments
// ============================================================================

/// The key in `settings.json`.
pub const KEY_SCREENS: &str = "wallpaper_screens";

/// How many disconnected monitors to remember. More — we forget the ones not
/// seen for a long time: nobody needs the meeting room projector a year later.
pub const REMEMBER_MONITORS: usize = 32;

/// What to show: the same three values as `WallpaperExec::apply(path,
/// format, name)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WallpaperSource {
    /// A path to a file/directory or a link.
    pub path: String,
    /// The wallpaper format (`video`, `web`, `swf`, `static`, …) — as in the engine.
    pub format: String,
    /// A name for the person (journal, tray).
    pub name: String,
}

impl WallpaperSource {
    /// Whether this is the same wallpaper — for grouping. The name does not count:
    /// one video under two labels is still one video.
    pub fn same_show(&self, other: &WallpaperSource) -> bool {
        self.path == other.path && self.format == other.format
    }

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("path".into(), Value::from(self.path.clone()));
        m.insert("format".into(), Value::from(self.format.clone()));
        m.insert("name".into(), Value::from(self.name.clone()));
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Option<WallpaperSource> {
        let o = v.as_object()?;
        let s = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let src = WallpaperSource { path: s("path"), format: s("format"), name: s("name") };
        (!src.path.is_empty()).then_some(src)
    }
}

/// The assignment of one monitor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MonitorEntry {
    /// Its wallpaper.
    pub source: WallpaperSource,
    /// The monitor label at the time of assignment — to show in the list when the
    /// monitor is disconnected.
    pub label: String,
    /// The connector it was last seen on.
    pub connector: String,
    /// When it was last seen (seconds since the epoch; 0 — unknown).
    pub last_seen: u64,
}

/// Where to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyTarget {
    /// On all screens: resets the assignments of individual monitors.
    AllScreens,
    /// On this monitor.
    Monitor(ScreenIdentity),
}

/// All per-screen wallpaper assignments.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScreenAssignments {
    /// The «on all screens» wallpaper — for everyone without their own.
    pub all: Option<WallpaperSource>,
    /// Monitors' own wallpapers by key ([`ScreenIdentity::key`]).
    pub monitors: BTreeMap<String, MonitorEntry>,
}

impl ScreenAssignments {
    /// Read from the `settings.json` map. No key or a corrupted one — empty
    /// assignments, that is, the former "one wallpaper on all screens" behavior
    /// (the wallpaper itself is then known by the caller, as before).
    pub fn from_settings(settings: &Map<String, Value>) -> ScreenAssignments {
        let Some(obj) = settings.get(KEY_SCREENS).and_then(Value::as_object) else {
            return ScreenAssignments::default();
        };
        let all = obj.get("all").and_then(WallpaperSource::from_value);
        let mut monitors = BTreeMap::new();
        if let Some(mons) = obj.get("monitors").and_then(Value::as_object) {
            for (key, v) in mons {
                let Some(o) = v.as_object() else { continue };
                let Some(source) = o.get("source").and_then(WallpaperSource::from_value) else { continue };
                let s = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                monitors.insert(
                    key.clone(),
                    MonitorEntry {
                        source,
                        label: s("label"),
                        connector: s("connector"),
                        last_seen: o.get("last_seen").and_then(Value::as_u64).unwrap_or(0),
                    },
                );
            }
        }
        ScreenAssignments { all, monitors }
    }

    /// The value for `settings.json`.
    pub fn to_value(&self) -> Value {
        let mut mons = Map::new();
        for (key, e) in &self.monitors {
            let mut m = Map::new();
            m.insert("source".into(), e.source.to_value());
            m.insert("label".into(), Value::from(e.label.clone()));
            m.insert("connector".into(), Value::from(e.connector.clone()));
            m.insert("last_seen".into(), Value::from(e.last_seen));
            mons.insert(key.clone(), Value::Object(m));
        }
        let mut obj = Map::new();
        obj.insert("all".into(), self.all.as_ref().map_or(Value::Null, WallpaperSource::to_value));
        obj.insert("monitors".into(), Value::Object(mons));
        Value::Object(obj)
    }

    /// Write into the `settings.json` map (other keys are left alone).
    pub fn write_settings(&self, settings: &mut Map<String, Value>) {
        settings.insert(KEY_SCREENS.into(), self.to_value());
    }

    /// Apply a wallpaper. «On all» resets the monitors' own wallpapers — the
    /// person asked for the same one everywhere, and a returning monitor will
    /// show it too.
    pub fn assign(&mut self, target: &ApplyTarget, source: WallpaperSource, now: u64) {
        match target {
            ApplyTarget::AllScreens => {
                self.all = Some(source);
                self.monitors.clear();
            }
            ApplyTarget::Monitor(id) => {
                // The same monitor may have been recorded under the neighboring
                // form of the key (with the connector and without) — the old record
                // must not pop up instead of the new one.
                if let Some(old) = self.lookup_key(id) {
                    if old != id.key {
                        self.monitors.remove(&old);
                    }
                }
                self.monitors.insert(
                    id.key.clone(),
                    MonitorEntry { source, label: id.label.clone(), connector: id.connector.clone(), last_seen: now },
                );
            }
        }
    }

    /// Remove the monitor's own wallpaper: from now on it shows «on all».
    pub fn clear_monitor(&mut self, id: &ScreenIdentity) -> bool {
        match self.lookup_key(id) {
            Some(k) => self.monitors.remove(&k).is_some(),
            None => false,
        }
    }

    /// Under which key this monitor's wallpaper is stored.
    ///
    /// Order: the exact key → the key with the current connector → the key
    /// without the connector → the only record "this monitor@some connector". The
    /// last three are about identical monitors without a serial: while there is
    /// no second one, the first is recognized by the old record with a
    /// connector too, and vice versa.
    pub fn lookup_key(&self, id: &ScreenIdentity) -> Option<String> {
        let with_port = format!("{}{CONNECTOR_SEP}{}", id.base, id.connector);
        for k in [&id.key, &with_port, &id.base] {
            if self.monitors.contains_key(k.as_str()) {
                return Some(k.clone());
            }
        }
        if id.key == id.base {
            let prefix = format!("{}{CONNECTOR_SEP}", id.base);
            let mut found = self.monitors.keys().filter(|k| k.starts_with(&prefix));
            if let (Some(one), None) = (found.next(), found.next()) {
                return Some(one.clone());
            }
        }
        None
    }

    /// The monitor's own wallpaper, if any.
    pub fn monitor_source(&self, id: &ScreenIdentity) -> Option<&WallpaperSource> {
        self.lookup_key(id).and_then(|k| self.monitors.get(&k)).map(|e| &e.source)
    }

    /// What to show on the screen: its own wallpaper, otherwise «on all»,
    /// otherwise nothing from Rexon (the desktop shows its own background).
    pub fn source_for(&self, id: &ScreenIdentity) -> Option<&WallpaperSource> {
        self.monitor_source(id).or(self.all.as_ref())
    }

    /// Mark these monitors as connected now and forget the extra disconnected
    /// ones beyond [`REMEMBER_MONITORS`] (the longest unseen first).
    pub fn mark_seen(&mut self, ids: &[ScreenIdentity], now: u64) {
        for id in ids {
            if let Some(k) = self.lookup_key(id) {
                if let Some(e) = self.monitors.get_mut(&k) {
                    e.last_seen = now;
                    e.connector = id.connector.clone();
                }
            }
        }
        if self.monitors.len() > REMEMBER_MONITORS {
            let mut by_age: Vec<(u64, String)> =
                self.monitors.iter().map(|(k, e)| (e.last_seen, k.clone())).collect();
            by_age.sort();
            let extra = self.monitors.len() - REMEMBER_MONITORS;
            for (_, k) in by_age.into_iter().take(extra) {
                self.monitors.remove(&k);
            }
        }
    }

    /// Whether anyone has their own wallpaper (otherwise — the former "one on all" mode).
    pub fn per_monitor(&self) -> bool {
        !self.monitors.is_empty()
    }
}

// ============================================================================
// Showing plan
// ============================================================================

/// One showing group: one wallpaper on several screens — one process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowGroup {
    /// What to show.
    pub source: WallpaperSource,
    /// Qt screen names (`ScreenIdentity::screen_name`) in screen order.
    pub screens: Vec<String>,
    /// Monitor keys of the same screens.
    pub keys: Vec<String>,
}

/// The plan: showing groups and the screens on which Rexon shows nothing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShowPlan {
    /// Groups in the order of their first screen.
    pub groups: Vec<ShowGroup>,
    /// Screens without a Rexon wallpaper (Qt names).
    pub uncovered: Vec<String>,
}

/// Lay the connected screens out over showing processes.
pub fn plan_show(screens: &[ScreenIdentity], assignments: &ScreenAssignments) -> ShowPlan {
    let mut plan = ShowPlan::default();
    for id in screens {
        let Some(src) = assignments.source_for(id) else {
            plan.uncovered.push(id.screen_name.clone());
            continue;
        };
        match plan.groups.iter_mut().find(|g| g.source.same_show(src)) {
            Some(g) => {
                g.screens.push(id.screen_name.clone());
                g.keys.push(id.key.clone());
            }
            None => plan.groups.push(ShowGroup {
                source: src.clone(),
                screens: vec![id.screen_name.clone()],
                keys: vec![id.key.clone()],
            }),
        }
    }
    plan
}

/// What to do with the processes to go from plan `old` to plan `new`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanChange {
    /// Groups that stay as they were (leave the process alone).
    pub keep: Vec<ShowGroup>,
    /// The same wallpaper but a different set of screens — restart (or give the
    /// process the new screen list).
    pub reshape: Vec<ShowGroup>,
    /// A new wallpaper — start.
    pub start: Vec<ShowGroup>,
    /// The wallpaper is no longer anywhere — stop.
    pub stop: Vec<ShowGroup>,
}

impl PlanChange {
    /// Nothing needs to be done.
    pub fn is_noop(&self) -> bool {
        self.reshape.is_empty() && self.start.is_empty() && self.stop.is_empty()
    }
}

/// Compare two plans. Groups are matched by wallpaper: the same wallpaper —
/// the same process.
pub fn plan_change(old: &ShowPlan, new: &ShowPlan) -> PlanChange {
    let mut change = PlanChange::default();
    for g in &new.groups {
        match old.groups.iter().find(|o| o.source.same_show(&g.source)) {
            Some(o) if o.screens == g.screens => change.keep.push(g.clone()),
            Some(_) => change.reshape.push(g.clone()),
            None => change.start.push(g.clone()),
        }
    }
    for o in &old.groups {
        if !new.groups.iter().any(|g| g.source.same_show(&o.source)) {
            change.stop.push(o.clone());
        }
    }
    change
}

/// The host command line switch with the screen list.
pub const SCREENS_FLAG: &str = "--screens";

/// The screen list for the command line: names separated by commas.
pub fn format_screens_arg(screens: &[String]) -> String {
    screens.join(",")
}

/// Parsing the [`SCREENS_FLAG`] value; empty — "all screens" (the former
/// behavior of the host without the switch).
pub fn parse_screens_arg(raw: &str) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

// ============================================================================
// Hot-plugging: settling
// ============================================================================

/// The silence after the last event after which the screens count as
/// settled. Connecting a monitor is several events in a row (the output
/// appeared, changed mode, moved), and laying out after each one would
/// rebuild the showing several times.
pub const HOTPLUG_QUIET_MS: u64 = 2000;

/// We do not wait longer than this from the first event, even if events keep
/// coming: a loose connector must not leave the person without a wallpaper forever.
pub const HOTPLUG_MAX_WAIT_MS: u64 = 10_000;

/// Settling over "screens changed" events. Time is milliseconds of any
/// monotonic clock, supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotplugSettle {
    quiet_ms: u64,
    max_wait_ms: u64,
    first: Option<u64>,
    last: Option<u64>,
}

impl Default for HotplugSettle {
    fn default() -> Self {
        HotplugSettle::new(HOTPLUG_QUIET_MS, HOTPLUG_MAX_WAIT_MS)
    }
}

impl HotplugSettle {
    /// Settling with the given deadlines.
    pub fn new(quiet_ms: u64, max_wait_ms: u64) -> HotplugSettle {
        HotplugSettle { quiet_ms, max_wait_ms: max_wait_ms.max(quiet_ms), first: None, last: None }
    }

    /// An event arrived.
    pub fn on_change(&mut self, now_ms: u64) {
        self.first.get_or_insert(now_ms);
        self.last = Some(now_ms);
    }

    /// When to rebuild (for the timer); `None` — nothing to wait for.
    pub fn due_at(&self) -> Option<u64> {
        let (first, last) = (self.first?, self.last?);
        Some((last + self.quiet_ms).min(first + self.max_wait_ms))
    }

    /// Whether it is time to rebuild. `true` — once per burst of events: the
    /// settling resets and waits for the next one.
    pub fn poll(&mut self, now_ms: u64) -> bool {
        match self.due_at() {
            Some(due) if now_ms >= due => {
                self.first = None;
                self.last = None;
                true
            }
            _ => false,
        }
    }

    /// Whether there are unprocessed events.
    pub fn pending(&self) -> bool {
        self.first.is_some()
    }
}

/// Rebuild moments for a series of events (time ascending) — the same
/// settling run over a list. For checks and the journal.
pub fn settle_times(events_ms: &[u64], quiet_ms: u64, max_wait_ms: u64) -> Vec<u64> {
    let mut s = HotplugSettle::new(quiet_ms, max_wait_ms);
    let mut out = Vec::new();
    for &t in events_ms {
        if let Some(due) = s.due_at() {
            if t >= due {
                s.poll(due);
                out.push(due);
            }
        }
        s.on_change(t);
    }
    if let Some(due) = s.due_at() {
        out.push(due);
    }
    out
}

// ============================================================================
// Live reading (thin wrappers)
// ============================================================================

/// The DRM outputs directory.
pub const DRM_SYSFS_DIR: &str = "/sys/class/drm";

/// The PNP manufacturer table from the hwdata package.
pub const PNP_IDS_PATHS: &[&str] = &["/usr/share/hwdata/pnp.ids", "/usr/share/misc/pnp.ids"];

/// Read outputs from a directory in the `/sys/class/drm` format (in the checks
/// — a temporary directory). Order — by connector name.
pub fn read_drm_outputs(root: &Path, pnp_ids_text: Option<&str>) -> Vec<DrmOutput> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<DrmOutput> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let connector = connector_from_sysfs_name(&name)?.to_string();
            let dir = e.path();
            let connected = std::fs::read_to_string(dir.join("status")).is_ok_and(|s| s.trim() == "connected");
            let edid = std::fs::read(dir.join("edid")).ok().and_then(|b| parse_edid(&b).ok());
            let vendor_name = edid.as_ref().and_then(|e| pnp_ids_text.and_then(|t| pnp_vendor_name(t, &e.pnp_id)));
            Some(DrmOutput { connector, connected, edid, vendor_name })
        })
        .collect();
    out.sort_by(|a, b| a.connector.cmp(&b.connector));
    out
}

/// This machine's outputs.
pub fn live_drm_outputs() -> Vec<DrmOutput> {
    let pnp = PNP_IDS_PATHS.iter().find_map(|p| std::fs::read_to_string(p).ok());
    read_drm_outputs(Path::new(DRM_SYSFS_DIR), pnp.as_deref())
}

// ============================================================================
// Checks
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic EDID: our own generator, not captured from other people's monitors.
    struct EdidSpec<'a> {
        pnp: &'a str,
        product: u16,
        serial: u32,
        name: Option<&'a str>,
        serial_text: Option<&'a str>,
        extra: Option<&'a str>,
    }

    fn make_edid(spec: &EdidSpec) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..8].copy_from_slice(&EDID_HEADER);
        let c: Vec<u16> = spec.pnp.bytes().map(|ch| u16::from(ch - b'A' + 1)).collect();
        let word = (c[0] << 10) | (c[1] << 5) | c[2];
        b[8..10].copy_from_slice(&word.to_be_bytes());
        b[10..12].copy_from_slice(&spec.product.to_le_bytes());
        b[12..16].copy_from_slice(&spec.serial.to_le_bytes());
        b[16] = 10;
        b[17] = 34; // 2024
        b[18] = 1;
        b[19] = 4;
        // The first descriptor is a mode (a non-zero frequency), parsing must
        // skip it.
        b[54] = 0x01;
        b[55] = 0x1D;
        let mut put = |slot: usize, tag: u8, text: &str| {
            let off = DESCRIPTOR_OFFSETS[slot];
            b[off + 3] = tag;
            let mut t = [0x20u8; 13];
            let bytes = text.as_bytes();
            t[..bytes.len()].copy_from_slice(bytes);
            if bytes.len() < 13 {
                t[bytes.len()] = 0x0A;
            }
            b[off + 5..off + 18].copy_from_slice(&t);
        };
        if let Some(n) = spec.name {
            put(1, TAG_NAME, n);
        }
        if let Some(s) = spec.serial_text {
            put(2, TAG_SERIAL_TEXT, s);
        }
        if let Some(x) = spec.extra {
            put(3, TAG_UNSPECIFIED_TEXT, x);
        }
        let sum = b[..127].iter().fold(0u8, |a, &x| a.wrapping_add(x));
        b[127] = 0u8.wrapping_sub(sum);
        b
    }

    fn spec(pnp: &str, product: u16, serial: u32) -> EdidSpec<'_> {
        EdidSpec { pnp, product, serial, name: None, serial_text: None, extra: None }
    }

    fn out(connector: &str, edid: Option<&EdidSpec>) -> DrmOutput {
        DrmOutput {
            connector: connector.into(),
            connected: true,
            edid: edid.map(|s| parse_edid(&make_edid(s)).unwrap()),
            vendor_name: None,
        }
    }

    fn qt(name: &str, manufacturer: &str, model: &str, serial: &str) -> ScreenFacts {
        ScreenFacts { name: name.into(), manufacturer: manufacturer.into(), model: model.into(), serial: serial.into() }
    }

    fn src(path: &str, format: &str) -> WallpaperSource {
        WallpaperSource { path: path.into(), format: format.into(), name: String::new() }
    }

    // ── EDID ──────────────────────────────────────────────────────────

    #[test]
    fn edid_full_monitor() {
        let s = EdidSpec { name: Some("DELL U2723QE"), serial_text: Some("7XK2R93"), ..spec("DEL", 0x41B2, 0x3031_4C42) };
        let e = parse_edid(&make_edid(&s)).unwrap();
        assert_eq!(e.pnp_id, "DEL");
        assert_eq!(e.product_code, 0x41B2);
        assert_eq!(e.serial_number, 0x3031_4C42);
        assert_eq!(e.monitor_name.as_deref(), Some("DELL U2723QE"));
        assert_eq!(e.serial_text.as_deref(), Some("7XK2R93"));
        assert_eq!(e.serial(), "7XK2R93");
        assert_eq!(e.year, Some(2024));
        assert!(e.checksum_ok);
        assert_eq!(edid_base_key(&e), "edid:DEL:41B2:7XK2R93");
    }

    #[test]
    fn edid_numeric_serial_and_empty_serial() {
        let e = parse_edid(&make_edid(&spec("SAM", 0x7140, 12345))).unwrap();
        assert_eq!(e.serial(), "12345");
        let e = parse_edid(&make_edid(&EdidSpec { extra: Some("LP140WF1"), ..spec("CMN", 0x1629, 0) })).unwrap();
        assert_eq!(e.serial(), "");
        assert_eq!(e.extra_text, vec!["LP140WF1".to_string()]);
        assert_eq!(edid_base_key(&e), "edid:CMN:1629:");
    }

    #[test]
    fn edid_thirteen_char_name_without_terminator() {
        let e = parse_edid(&make_edid(&EdidSpec { name: Some("ABCDEFGHIJKLM"), ..spec("ACR", 1, 1) })).unwrap();
        assert_eq!(e.monitor_name.as_deref(), Some("ABCDEFGHIJKLM"));
    }

    #[test]
    fn edid_errors_and_bad_checksum() {
        assert_eq!(parse_edid(&[]), Err(EdidError::TooShort));
        assert_eq!(parse_edid(&[0u8; 127]), Err(EdidError::TooShort));
        let mut b = make_edid(&spec("DEL", 1, 1));
        b[0] = 1;
        assert_eq!(parse_edid(&b), Err(EdidError::BadHeader));
        let mut b = make_edid(&spec("DEL", 1, 1));
        b[8] = 0;
        b[9] = 0; // the letters «0» are not letters
        assert_eq!(parse_edid(&b), Err(EdidError::BadVendor));
        let mut b = make_edid(&spec("DEL", 1, 1));
        b[127] ^= 0xFF;
        let e = parse_edid(&b).unwrap();
        assert!(!e.checksum_ok);
        assert_eq!(e.pnp_id, "DEL");
        // An extension block at the tail does not get in the way.
        let mut b = make_edid(&spec("DEL", 1, 1));
        b.extend_from_slice(&[0x02; 128]);
        assert!(parse_edid(&b).is_ok());
    }

    #[test]
    fn edid_garbage_in_text_is_dropped() {
        let mut b = make_edid(&EdidSpec { name: Some("AB"), ..spec("DEL", 1, 1) });
        let off = DESCRIPTOR_OFFSETS[1] + 5;
        b[off + 1] = 0x07; // non-printable inside the name
        let e = parse_edid(&b).unwrap();
        assert_eq!(e.monitor_name.as_deref(), Some("A"));
    }

    #[test]
    fn pnp_table_lookup() {
        let t = "AAA\tAvolites Ltd\nDEL\tDell Inc.\n";
        assert_eq!(pnp_vendor_name(t, "DEL").as_deref(), Some("Dell Inc."));
        assert_eq!(pnp_vendor_name(t, "del").as_deref(), Some("Dell Inc."));
        assert_eq!(pnp_vendor_name(t, "SAM"), None);
    }

    #[test]
    fn sysfs_names() {
        assert_eq!(connector_from_sysfs_name("card1-eDP-1"), Some("eDP-1"));
        assert_eq!(connector_from_sysfs_name("card2-HDMI-A-1"), Some("HDMI-A-1"));
        assert_eq!(connector_from_sysfs_name("card1"), None);
        assert_eq!(connector_from_sysfs_name("renderD128"), None);
        assert_eq!(connector_from_sysfs_name("version"), None);
        assert_eq!(connector_from_sysfs_name("cardX-DP-1"), None);
    }

    #[test]
    fn connector_families() {
        assert_eq!(connector_family("HDMI-A-1"), Some(("HDMI".into(), 1)));
        assert_eq!(connector_family("HDMI-1"), Some(("HDMI".into(), 1)));
        assert_eq!(connector_family("DisplayPort-0"), Some(("DP".into(), 0)));
        assert_eq!(connector_family("eDP-1"), Some(("EDP".into(), 1)));
        assert_eq!(connector_family("DVI-D-2"), Some(("DVI".into(), 2)));
        assert_eq!(connector_family("Virtual1"), None);
    }

    // ── matching with Qt ──────────────────────────────────────────────

    #[test]
    fn wayland_match_by_connector_with_no_serial_from_qt() {
        let dell = EdidSpec { name: Some("DELL U2723QE"), serial_text: Some("7XK2R93"), ..spec("DEL", 0x41B2, 1) };
        let sam = EdidSpec { name: Some("LS27A600"), ..spec("SAM", 0x7140, 777) };
        let outs = vec![out("DP-1", Some(&dell)), out("HDMI-A-1", Some(&sam))];
        // Wayland: no serial, the manufacturer as a name.
        let screens = vec![qt("HDMI-A-1", "Samsung Electric Company", "LS27A600", ""), qt("DP-1", "Dell Inc.", "DELL U2723QE", "")];
        assert_eq!(match_screens(&screens, &outs), vec![Some(1), Some(0)]);
        let ids = identify_screens(&screens, &outs);
        assert_eq!(ids[0].key, "edid:SAM:7140:777");
        assert_eq!(ids[1].key, "edid:DEL:41B2:7XK2R93");
        assert_eq!(ids[1].label, "DELL U2723QE");
        assert!(ids.iter().all(|i| i.from_edid));
    }

    #[test]
    fn x11_nvidia_names_differ_but_serial_matches() {
        let a = EdidSpec { name: Some("XG27AQ"), serial_text: Some("AAA111"), ..spec("AUS", 0x27A1, 0) };
        let b = EdidSpec { name: Some("XG27AQ"), serial_text: Some("BBB222"), ..spec("AUS", 0x27A1, 0) };
        let outs = vec![out("DP-1", Some(&a)), out("DP-2", Some(&b))];
        // The nvidia driver on X11 numbers them from zero: DP-0, DP-1. By name the
        // pairs get crossed — the serial decides.
        let screens = vec![qt("DP-0", "AUS", "XG27AQ", "BBB222"), qt("DP-1", "AUS", "XG27AQ", "AAA111")];
        assert_eq!(match_screens(&screens, &outs), vec![Some(1), Some(0)]);
    }

    #[test]
    fn conflicting_serial_forbids_pair_even_on_same_port() {
        let a = EdidSpec { serial_text: Some("AAA111"), ..spec("AUS", 1, 0) };
        let o = out("DP-1", Some(&a));
        assert_eq!(match_score(&qt("DP-1", "AUS", "", "ZZZ999"), &o), None);
    }

    #[test]
    fn manufacturer_alone_is_not_enough() {
        let a = spec("DEL", 1, 5);
        let o = DrmOutput { vendor_name: Some("Dell Inc.".into()), ..out("DP-3", Some(&a)) };
        assert_eq!(match_score(&qt("Virtual-1", "Dell", "", ""), &o), Some(1));
        assert_eq!(match_screens(&[qt("Virtual-1", "Dell", "", "")], &[o]), vec![None]);
    }

    #[test]
    fn disconnected_outputs_are_ignored() {
        let mut o = out("DP-1", Some(&spec("DEL", 1, 5)));
        o.connected = false;
        assert_eq!(match_screens(&[qt("DP-1", "", "", "")], &[o]), vec![None]);
    }

    #[test]
    fn two_identical_monitors_without_serial_get_port_in_key() {
        let s = EdidSpec { name: Some("HP 24f"), ..spec("HPN", 0x3456, 0) };
        let outs = vec![out("DP-1", Some(&s)), out("HDMI-A-1", Some(&s))];
        let screens = vec![qt("DP-1", "", "HP 24f", ""), qt("HDMI-A-1", "", "HP 24f", "")];
        let ids = identify_screens(&screens, &outs);
        assert_eq!(ids[0].key, "edid:HPN:3456:@DP-1");
        assert_eq!(ids[1].key, "edid:HPN:3456:@HDMI-A-1");
        assert_eq!(ids[0].base, ids[1].base);
        // A single such one — without the connector: it follows the person from port to port.
        let ids = identify_screens(&screens[..1], &outs[..1]);
        assert_eq!(ids[0].key, "edid:HPN:3456:");
    }

    #[test]
    fn no_edid_falls_back_to_qt_then_port() {
        let outs = vec![DrmOutput { connector: "Virtual-1".into(), connected: true, edid: None, vendor_name: None }];
        let ids = identify_screens(&[qt("Virtual-1", "", "", "")], &outs);
        assert_eq!(ids[0].key, "port:Virtual-1");
        assert!(!ids[0].from_edid);
        let ids = identify_screens(&[qt("X-1", "Acme", "Pro 27", "S1")], &[]);
        assert_eq!(ids[0].key, "qt:Acme:Pro 27:S1");
        assert_eq!(ids[0].connector, "X-1");
        assert_eq!(screen_base_key(&qt("X-1", "Acme", "", "")), "port:X-1");
    }

    #[test]
    fn laptop_panel_label_from_unspecified_text() {
        let s = EdidSpec { extra: Some("N140HCA-EAC"), ..spec("CMN", 0x1629, 0) };
        let ids = identify_screens(&[qt("eDP-1", "", "", "")], &[out("eDP-1", Some(&s))]);
        assert_eq!(ids[0].label, "N140HCA-EAC");
        assert_eq!(ids[0].key, "edid:CMN:1629:");
    }

    #[test]
    fn qt_model_as_product_code() {
        let e = parse_edid(&make_edid(&spec("SAM", 0x7140, 0))).unwrap();
        assert!(model_equal("0x7140", &e));
        assert!(model_equal("28992", &e));
        assert!(!model_equal("", &e));
    }

    // ── assignments ───────────────────────────────────────────────────

    fn id(key: &str, base: &str, conn: &str) -> ScreenIdentity {
        ScreenIdentity {
            key: key.into(),
            base: base.into(),
            screen_name: conn.into(),
            connector: conn.into(),
            label: String::new(),
            from_edid: true,
        }
    }

    #[test]
    fn old_settings_mean_all_screens() {
        let mut settings = Map::new();
        settings.insert("theme".into(), Value::from("auto"));
        let a = ScreenAssignments::from_settings(&settings);
        assert_eq!(a, ScreenAssignments::default());
        assert!(!a.per_monitor());
        // A corrupted value — also the former behavior.
        settings.insert(KEY_SCREENS.into(), Value::from("мусор"));
        assert_eq!(ScreenAssignments::from_settings(&settings), ScreenAssignments::default());
    }

    #[test]
    fn settings_round_trip_keeps_other_keys() {
        let mut a = ScreenAssignments::default();
        a.assign(&ApplyTarget::AllScreens, src("/v/a.mp4", "video"), 1);
        a.assign(&ApplyTarget::Monitor(id("edid:DEL:41B2:X", "edid:DEL:41B2:X", "DP-1")), src("https://x", "web"), 7);
        let mut settings = Map::new();
        settings.insert("theme".into(), Value::from("auto"));
        a.write_settings(&mut settings);
        assert_eq!(settings["theme"], "auto");
        let back = ScreenAssignments::from_settings(&settings);
        assert_eq!(back, a);
        assert_eq!(back.monitors["edid:DEL:41B2:X"].last_seen, 7);
        // A record without a path is dropped, not turned into an empty wallpaper.
        let text = r#"{"wallpaper_screens":{"all":{"path":""},"monitors":{"k":{"source":{"format":"video"}}}}}"#;
        let m: Map<String, Value> = serde_json::from_str(text).unwrap();
        assert_eq!(ScreenAssignments::from_settings(&m), ScreenAssignments::default());
    }

    #[test]
    fn all_screens_clears_per_monitor() {
        let mut a = ScreenAssignments::default();
        a.assign(&ApplyTarget::Monitor(id("k1", "k1", "DP-1")), src("/a", "video"), 1);
        a.assign(&ApplyTarget::AllScreens, src("/b", "video"), 2);
        assert!(a.monitors.is_empty());
        assert_eq!(a.source_for(&id("k1", "k1", "DP-1")), Some(&src("/b", "video")));
    }

    #[test]
    fn disconnected_monitor_is_remembered() {
        let mut a = ScreenAssignments::default();
        a.assign(&ApplyTarget::AllScreens, src("/all.mp4", "video"), 1);
        let ext = id("edid:SAM:7140:777", "edid:SAM:7140:777", "HDMI-A-1");
        a.assign(&ApplyTarget::Monitor(ext.clone()), src("/sam.mp4", "video"), 2);
        let lap = id("edid:CMN:1629:", "edid:CMN:1629:", "eDP-1");
        // Disconnected — only the laptop is in the plan.
        let plan = plan_show(std::slice::from_ref(&lap), &a);
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].source.path, "/all.mp4");
        assert!(a.monitors.contains_key(&ext.key));
        // Connected to ANOTHER port — the key is the same, the wallpaper is back.
        let ext_moved = id("edid:SAM:7140:777", "edid:SAM:7140:777", "DP-2");
        assert_eq!(a.source_for(&ext_moved), Some(&src("/sam.mp4", "video")));
        a.mark_seen(&[ext_moved], 9);
        assert_eq!(a.monitors[&ext.key].connector, "DP-2");
        assert_eq!(a.monitors[&ext.key].last_seen, 9);
    }

    #[test]
    fn twin_monitors_lookup_between_key_forms() {
        let mut a = ScreenAssignments::default();
        // While there is no second one — a record without the connector.
        a.assign(&ApplyTarget::Monitor(id("hp:", "hp:", "DP-1")), src("/one", "video"), 1);
        // A twin arrived: both have keys with the connector; the first is recognized by the base.
        assert_eq!(a.source_for(&id("hp:@DP-1", "hp:", "DP-1")), Some(&src("/one", "video")));
        // Assigned by the forms with the connector — the old record without the connector is gone.
        a.assign(&ApplyTarget::Monitor(id("hp:@DP-1", "hp:", "DP-1")), src("/one", "video"), 2);
        a.assign(&ApplyTarget::Monitor(id("hp:@HDMI-A-1", "hp:", "HDMI-A-1")), src("/two", "video"), 2);
        assert_eq!(a.monitors.len(), 2);
        // The twin was taken away: the remaining one alone on DP-1 — its record with the connector.
        assert_eq!(a.source_for(&id("hp:", "hp:", "DP-1")), Some(&src("/one", "video")));
        // Two records with a connector, but one monitor and on a third port — we
        // do not guess which of the two it is.
        assert_eq!(a.monitor_source(&id("hp:", "hp:", "DP-3")), None);
        // One of the recorded ones — we guess.
        a.monitors.remove("hp:@HDMI-A-1");
        assert_eq!(a.monitor_source(&id("hp:", "hp:", "DP-3")), Some(&src("/one", "video")));
    }

    #[test]
    fn clear_monitor_returns_to_all() {
        let mut a = ScreenAssignments::default();
        a.assign(&ApplyTarget::AllScreens, src("/all", "video"), 1);
        let m = id("k", "k", "DP-1");
        a.assign(&ApplyTarget::Monitor(m.clone()), src("/own", "web"), 1);
        assert!(a.clear_monitor(&m));
        assert!(!a.clear_monitor(&m));
        assert_eq!(a.source_for(&m), Some(&src("/all", "video")));
    }

    #[test]
    fn forgetting_beyond_limit_drops_oldest() {
        let mut a = ScreenAssignments::default();
        for i in 0..(REMEMBER_MONITORS as u64 + 3) {
            let k = format!("k{i}");
            a.assign(&ApplyTarget::Monitor(id(&k, &k, "DP-1")), src("/x", "video"), 100 + i);
        }
        a.mark_seen(&[], 1000);
        assert_eq!(a.monitors.len(), REMEMBER_MONITORS);
        assert!(!a.monitors.contains_key("k0") && !a.monitors.contains_key("k2"));
        assert!(a.monitors.contains_key("k3"));
    }

    // ── showing plan ──────────────────────────────────────────────────

    #[test]
    fn same_wallpaper_is_one_process() {
        let mut a = ScreenAssignments::default();
        a.assign(&ApplyTarget::AllScreens, src("/a.mp4", "video"), 1);
        let screens = vec![id("k1", "k1", "DP-1"), id("k2", "k2", "HDMI-A-1"), id("k3", "k3", "eDP-1")];
        let plan = plan_show(&screens, &a);
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].screens, vec!["DP-1", "HDMI-A-1", "eDP-1"]);
        assert!(plan.uncovered.is_empty());
        // The second monitor has its own wallpaper — a second process; the third has
        // the same as «on all» (a different label) — it is in the first group.
        a.assign(&ApplyTarget::Monitor(screens[1].clone()), src("https://w", "web"), 1);
        let mut same = src("/a.mp4", "video");
        same.name = "другая подпись".into();
        a.assign(&ApplyTarget::Monitor(screens[2].clone()), same, 1);
        let plan = plan_show(&screens, &a);
        assert_eq!(plan.groups.len(), 2);
        assert_eq!(plan.groups[0].screens, vec!["DP-1", "eDP-1"]);
        assert_eq!(plan.groups[0].keys, vec!["k1", "k3"]);
        assert_eq!(plan.groups[1].screens, vec!["HDMI-A-1"]);
        assert_eq!(plan.groups[1].source.format, "web");
    }

    #[test]
    fn same_path_other_format_is_other_process() {
        let mut a = ScreenAssignments::default();
        a.assign(&ApplyTarget::Monitor(id("k1", "k1", "DP-1")), src("/dir", "video"), 1);
        a.assign(&ApplyTarget::Monitor(id("k2", "k2", "DP-2")), src("/dir", "web"), 1);
        let plan = plan_show(&[id("k1", "k1", "DP-1"), id("k2", "k2", "DP-2")], &a);
        assert_eq!(plan.groups.len(), 2);
    }

    #[test]
    fn no_all_and_no_own_means_uncovered() {
        let mut a = ScreenAssignments::default();
        a.assign(&ApplyTarget::Monitor(id("k1", "k1", "DP-1")), src("/a", "video"), 1);
        let plan = plan_show(&[id("k1", "k1", "DP-1"), id("k2", "k2", "DP-2")], &a);
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.uncovered, vec!["DP-2"]);
    }

    #[test]
    fn plan_change_after_hotplug() {
        let mut a = ScreenAssignments::default();
        a.assign(&ApplyTarget::AllScreens, src("/all", "video"), 1);
        let lap = id("lap", "lap", "eDP-1");
        let ext = id("ext", "ext", "HDMI-A-1");
        a.assign(&ApplyTarget::Monitor(ext.clone()), src("/ext", "swf"), 1);
        let before = plan_show(std::slice::from_ref(&lap), &a);
        let after = plan_show(&[lap.clone(), ext.clone()], &a);
        let ch = plan_change(&before, &after);
        assert_eq!(ch.keep.len(), 1);
        assert_eq!(ch.start.len(), 1);
        assert_eq!(ch.start[0].source.path, "/ext");
        assert!(ch.stop.is_empty() && ch.reshape.is_empty());
        // An external one without its own wallpaper — the same process grows onto the second screen.
        a.clear_monitor(&ext);
        let after = plan_show(&[lap, ext], &a);
        let ch = plan_change(&before, &after);
        assert_eq!(ch.reshape.len(), 1);
        assert_eq!(ch.reshape[0].screens, vec!["eDP-1", "HDMI-A-1"]);
        // Back again — the same process shrinks, nothing stops.
        let back = plan_change(&after, &before);
        assert_eq!(back.reshape.len(), 1);
        assert!(back.stop.is_empty());
        assert!(plan_change(&before, &before).is_noop());
        assert_eq!(plan_change(&before, &ShowPlan::default()).stop.len(), 1);
    }

    #[test]
    fn screens_arg_round_trip() {
        let s = vec!["DP-1".to_string(), "HDMI-A-1".to_string()];
        assert_eq!(format_screens_arg(&s), "DP-1,HDMI-A-1");
        assert_eq!(parse_screens_arg("DP-1, HDMI-A-1,,"), s);
        assert!(parse_screens_arg("").is_empty());
    }

    // ── settling ──────────────────────────────────────────────────────

    #[test]
    fn hotplug_burst_settles_once() {
        // Connecting: four events within 600 ms — one rebuild 2 s after the
        // last one.
        assert_eq!(settle_times(&[0, 150, 400, 600], 2000, 10_000), vec![2600]);
        let mut s = HotplugSettle::default();
        assert_eq!(s.due_at(), None);
        assert!(!s.poll(5_000));
        s.on_change(1000);
        s.on_change(1300);
        assert_eq!(s.due_at(), Some(3300));
        assert!(!s.poll(3299));
        assert!(s.poll(3300));
        assert!(!s.pending());
        assert!(!s.poll(9000), "второй раз на ту же пачку не срабатывает");
    }

    #[test]
    fn two_separate_hotplugs_settle_twice() {
        assert_eq!(settle_times(&[0, 100, 5000, 5200], 2000, 10_000), vec![2100, 7200]);
    }

    #[test]
    fn flapping_connector_is_capped() {
        // An event every 500 ms without end — a rebuild no later than 10 s from the
        // first, then the next burst.
        let events: Vec<u64> = (0..30).map(|i| i * 500).collect();
        assert_eq!(settle_times(&events, 2000, 10_000), vec![10_000, 16_500]);
    }

    #[test]
    fn max_wait_never_below_quiet() {
        let s = {
            let mut s = HotplugSettle::new(2000, 500);
            s.on_change(0);
            s
        };
        assert_eq!(s.due_at(), Some(2000));
    }

    // ── live reading on a fake directory ──────────────────────────────

    #[test]
    fn read_drm_outputs_from_fake_sysfs() {
        let root = std::env::temp_dir().join(format!("rexon-drm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mk = |dir: &str, status: &str, edid: &[u8]| {
            let d = root.join(dir);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("status"), format!("{status}\n")).unwrap();
            std::fs::write(d.join("edid"), edid).unwrap();
        };
        mk("card1-eDP-1", "connected", &make_edid(&spec("CMN", 0x1629, 0)));
        mk("card1-DP-1", "disconnected", &[]);
        mk("card2-HDMI-A-1", "connected", &make_edid(&EdidSpec { name: Some("LS27A600"), ..spec("SAM", 0x7140, 9) }));
        std::fs::create_dir_all(root.join("renderD128")).unwrap();
        std::fs::write(root.join("version"), "x").unwrap();
        let outs = read_drm_outputs(&root, Some("SAM\tSamsung Electric Company\n"));
        let _ = std::fs::remove_dir_all(&root);
        let names: Vec<&str> = outs.iter().map(|o| o.connector.as_str()).collect();
        assert_eq!(names, vec!["DP-1", "HDMI-A-1", "eDP-1"]);
        assert!(!outs[0].connected && outs[0].edid.is_none());
        assert_eq!(outs[1].vendor_name.as_deref(), Some("Samsung Electric Company"));
        assert_eq!(outs[2].edid.as_ref().unwrap().pnp_id, "CMN");
        // A Wayland screen with a manufacturer name matches by connector + name.
        let ids = identify_screens(&[qt("HDMI-A-1", "Samsung Electric Company", "LS27A600", "")], &outs);
        assert_eq!(ids[0].key, "edid:SAM:7140:9");
    }
}
