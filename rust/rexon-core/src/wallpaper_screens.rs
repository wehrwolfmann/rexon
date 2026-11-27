//! Обои по мониторам: кто какой монитор, что на нём показывать и когда
//! пересобирать показ после горячего подключения.
//!
//! Модуль — ЯДРО без Qt и без побочных действий (кроме двух тонких
//! живых обёрток чтения sysfs/hwdata в конце). Его зовут:
//!
//!   * хост `rexon-wallpaper-host` и его надсмотрщик — [`plan_show`] (какие
//!     процессы держать и на каких экранах), [`plan_change`] (что
//!     перезапустить после смены экранов), [`HotplugSettle`] (когда);
//!   * страница «Применить ▸ Монитор N» — [`identify_screens`],
//!     [`ScreenAssignments::assign`] и хранение в `settings.json`
//!     ([`ScreenAssignments::from_settings`] / [`ScreenAssignments::write_settings`]);
//!   * расширения GNOME/Cinnamon и исполнители DE — тот же [`ShowPlan`]:
//!     у группы есть имена разъёмов, по которым DE раскладывает картинку.
//!
//! ## Опознание монитора
//!
//! Имя разъёма (`DP-1`) не годится как ключ памяти: человек переткнул кабель
//! в соседний порт — и «тот же» монитор стал другим. Поэтому ключ строится
//! из EDID — производитель (код PNP), код изделия и серийный номер — и
//! переживает смену порта. Разъём добавляется к ключу только там, где без
//! него нельзя: серийника нет (дешёвые панели, ноутбучные eDP) И рядом
//! стоит монитор с тем же ключом. Два одинаковых монитора без серийника
//! различаются по порту, один такой — ходит за человеком по портам.
//!
//! EDID берётся из `/sys/class/drm/card*-<разъём>/edid` и разбирается здесь
//! своим кодом ([`parse_edid`], структура блока — по стандарту VESA E-EDID
//! 1.3/1.4: заголовок, код производителя, код изделия, серийник,
//! дескрипторы 0xFC/0xFF/0xFE). Qt о том же мониторе знает своё
//! (`QScreen::name/manufacturer/model/serialNumber`), и на разных
//! платформах по-разному: на X11 поля приходят из EDID, на Wayland — из
//! `wl_output` (серийника там, как правило, нет), имена разъёмов X11 у
//! части драйверов свои (`HDMI-0`, `DisplayPort-0`). Поэтому экран Qt
//! сопоставляется с выходом DRM по очкам ([`match_screens`]): совпавший
//! серийник весит больше всего, потом разъём, модель и производитель;
//! противоречащие серийники запрещают пару вовсе. Не сопоставился —
//! ключ строится из того, что сказал Qt, а нет и этого — из имени разъёма.
//!
//! ## Назначения и совместимость
//!
//! `settings.json`, ключ [`KEY_SCREENS`]: обои «на все экраны» и по
//! ключам мониторов. Ключа нет (все прежние установки) — значит «одни
//! обои на все экраны», ровно как работал Rexon до этого модуля. Назначение
//! отключённого монитора НЕ стирается: подключили снова — вернулись его
//! обои.
//!
//! ## Показ
//!
//! Один процесс на группу экранов с одинаковыми обоями — одинаковые обои
//! на двух мониторах не должны декодировать видео дважды; разные обои —
//! разные процессы.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

// ============================================================================
// EDID
// ============================================================================

/// Длина базового блока EDID.
pub const EDID_BLOCK_LEN: usize = 128;

/// Заголовок базового блока EDID.
const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// Смещения четырёх 18-байтовых дескрипторов базового блока.
const DESCRIPTOR_OFFSETS: [usize; 4] = [54, 72, 90, 108];

/// Метки текстовых дескрипторов дисплея.
const TAG_SERIAL_TEXT: u8 = 0xFF;
const TAG_UNSPECIFIED_TEXT: u8 = 0xFE;
const TAG_NAME: u8 = 0xFC;

/// Что ядру нужно из EDID монитора.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EdidInfo {
    /// Код производителя PNP, три заглавные буквы (`SAM`, `DEL`).
    pub pnp_id: String,
    /// Код изделия (байты 10–11, младший первым).
    pub product_code: u16,
    /// Числовой серийник (байты 12–15); `0` — производитель не заполнил.
    pub serial_number: u32,
    /// Серийник текстом (дескриптор 0xFF), если есть.
    pub serial_text: Option<String>,
    /// Имя монитора (дескриптор 0xFC), если есть.
    pub monitor_name: Option<String>,
    /// Прочий текст (дескрипторы 0xFE) — у ноутбучных панелей там модель.
    pub extra_text: Vec<String>,
    /// Год выпуска, если указан.
    pub year: Option<u16>,
    /// Сошлась ли контрольная сумма. Разбор её НЕ требует: у части
    /// мониторов сумма битая, а остальные поля честные.
    pub checksum_ok: bool,
}

impl EdidInfo {
    /// Серийник для ключа: текст, иначе число, иначе пусто.
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

/// Почему EDID не разобрался.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdidError {
    /// Меньше одного блока (у отключённого разъёма файл пустой).
    TooShort,
    /// Нет заголовка `00 FF FF FF FF FF FF 00`.
    BadHeader,
    /// Код производителя вне `A..Z`.
    BadVendor,
}

/// Текст дескриптора: до 13 байт, конец — `0x0A`, хвост — пробелы.
/// Непечатаемое отбрасывается: в ключ и в подпись мусор не идёт.
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

/// Разобрать базовый блок EDID.
pub fn parse_edid(bytes: &[u8]) -> Result<EdidInfo, EdidError> {
    if bytes.len() < EDID_BLOCK_LEN {
        return Err(EdidError::TooShort);
    }
    let block = &bytes[..EDID_BLOCK_LEN];
    if block[..8] != EDID_HEADER {
        return Err(EdidError::BadHeader);
    }
    // Производитель: 16 бит старшим первым, старший бит — ноль, дальше три
    // буквы по 5 бит, `1` = `A`.
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
        // Дескриптор дисплея: первые два байта нулевые (иначе это режим).
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

/// Название производителя по коду PNP из текста `hwdata/pnp.ids`
/// (`КОД<таб>Название`). Нужно для сверки с Qt на Wayland: композиторы
/// отдают в `wl_output` название, а не код.
pub fn pnp_vendor_name(pnp_ids_text: &str, pnp_id: &str) -> Option<String> {
    pnp_ids_text.lines().find_map(|line| {
        let (code, name) = line.split_once('\t')?;
        (code.trim().eq_ignore_ascii_case(pnp_id)).then(|| name.trim().to_string()).filter(|n| !n.is_empty())
    })
}

// ============================================================================
// Выходы DRM и экраны Qt
// ============================================================================

/// Выход видеокарты из sysfs (`/sys/class/drm/card1-DP-1`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DrmOutput {
    /// Имя разъёма без карты: `DP-1`, `HDMI-A-1`, `eDP-1`.
    pub connector: String,
    /// `status` = `connected`.
    pub connected: bool,
    /// Разобранный EDID; `None` — файла нет, пустой или битый.
    pub edid: Option<EdidInfo>,
    /// Название производителя из `pnp.ids`, если вызывающий его нашёл.
    pub vendor_name: Option<String>,
}

/// Имя разъёма из имени каталога sysfs: `card1-HDMI-A-1` → `HDMI-A-1`.
/// Не каталог выхода (`card1`, `renderD128`, `version`) — `None`.
pub fn connector_from_sysfs_name(dir_name: &str) -> Option<&str> {
    let rest = dir_name.strip_prefix("card")?;
    let (num, connector) = rest.split_once('-')?;
    if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) || connector.is_empty() {
        return None;
    }
    Some(connector)
}

/// Что о экране сказал Qt (`QScreen`) — или любой другой опрос
/// (`wlr-randr`, `hyprctl monitors`), где те же четыре поля.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScreenFacts {
    /// `QScreen::name()` — имя разъёма у платформы (`DP-1`, на X11 бывает `HDMI-0`).
    pub name: String,
    /// `QScreen::manufacturer()` — код PNP или название производителя.
    pub manufacturer: String,
    /// `QScreen::model()`.
    pub model: String,
    /// `QScreen::serialNumber()` — на Wayland обычно пусто.
    pub serial: String,
}

/// Тип и номер разъёма в общем виде: `HDMI-A-1` и `HDMI-1` → `("HDMI", 1)`,
/// `DisplayPort-0` → `("DP", 0)`. Нужно для X11, где драйверы зовут те же
/// разъёмы по-своему.
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

/// Пара «экран Qt — выход DRM» несовместима: оба серийника есть и разные.
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

/// Очки пары; `None` — пара запрещена.
///
/// Веса: серийник 8, точный разъём 6, модель 4, тип и номер разъёма 2,
/// тип разъёма 1, производитель 1. Порог совпадения — [`MATCH_THRESHOLD`]:
/// одного производителя мало (у человека бывает два Dell).
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

/// Наименьшие очки, при которых пара считается одним монитором.
pub const MATCH_THRESHOLD: u32 = 2;

/// Сопоставить экраны Qt выходам DRM. Ответ — по одному месту на экран (в
/// порядке `screens`): индекс выхода в `outputs` или `None`. Учитываются
/// только подключённые выходы; каждый выход достаётся не больше чем одному
/// экрану, лучшие пары — первыми.
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
    // Лучшие пары первыми; при равенстве — по порядку, чтобы ответ не
    // зависел от сортировки.
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
// Ключ монитора
// ============================================================================

/// Разделитель «ключ@разъём» у одинаковых мониторов без серийника.
pub const CONNECTOR_SEP: char = '@';

/// Кто есть кто среди подключённых экранов.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScreenIdentity {
    /// Ключ памяти назначений. Для одинаковых мониторов — с разъёмом.
    pub key: String,
    /// Ключ без разъёма (что монитор есть сам по себе).
    pub base: String,
    /// Имя экрана у Qt — по нему хост находит `QScreen`.
    pub screen_name: String,
    /// Имя разъёма DRM, если экран сопоставлен; иначе имя у Qt.
    pub connector: String,
    /// Модель для подписи (`DELL U2723QE`); пусто — подписывать разъёмом.
    pub label: String,
    /// Ключ построен из EDID (а не из слов Qt или имени разъёма).
    pub from_edid: bool,
}

/// Очистить часть ключа: без разделителей и пробелов по краям.
fn key_part(s: &str) -> String {
    s.trim().chars().map(|c| if c == ':' || c == CONNECTOR_SEP { '_' } else { c }).collect()
}

/// Ключ из EDID: `edid:SAM:7140:H4ZR123456` (код изделия — 4 hex-цифры).
pub fn edid_base_key(edid: &EdidInfo) -> String {
    format!("edid:{}:{:04X}:{}", edid.pnp_id, edid.product_code, key_part(&edid.serial()))
}

/// Ключ из слов Qt: `qt:Производитель:Модель:Серийник`; все три пусты —
/// `port:<имя>`.
pub fn screen_base_key(screen: &ScreenFacts) -> String {
    let (m, md, s) = (key_part(&screen.manufacturer), key_part(&screen.model), key_part(&screen.serial));
    if md.is_empty() && s.is_empty() {
        // Одного производителя для ключа мало — это не монитор, а марка.
        return format!("port:{}", key_part(&screen.name));
    }
    format!("qt:{m}:{md}:{s}")
}

/// Опознать подключённые экраны: ключ, разъём и подпись каждого.
///
/// Одинаковые ключи у двух экранов (два одинаковых монитора без серийника
/// или с одним и тем же серийником) получают разъём в ключ — обои у них
/// разные, если человек так назначил.
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
// Назначения
// ============================================================================

/// Ключ в `settings.json`.
pub const KEY_SCREENS: &str = "wallpaper_screens";

/// Сколько отключённых мониторов помнить. Больше — забываем давно не
/// виденные: проектор из переговорной через год никому не нужен.
pub const REMEMBER_MONITORS: usize = 32;

/// Что показывать: те же три величины, что `WallpaperExec::apply(путь,
/// формат, имя)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WallpaperSource {
    /// Путь к файлу/каталогу или ссылка.
    pub path: String,
    /// Формат обоев (`video`, `web`, `swf`, `static`, …) — как у движка.
    pub format: String,
    /// Имя для человека (журнал, трей).
    pub name: String,
}

impl WallpaperSource {
    /// Одни ли это обои — для группировки. Имя не в счёт: одно видео под
    /// двумя подписями всё равно одно видео.
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

/// Назначение одного монитора.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MonitorEntry {
    /// Его обои.
    pub source: WallpaperSource,
    /// Подпись монитора на момент назначения — показать в списке, когда
    /// монитор отключён.
    pub label: String,
    /// Разъём, на котором его видели последним.
    pub connector: String,
    /// Когда его видели последним (секунды эпохи; 0 — не знаем).
    pub last_seen: u64,
}

/// Куда применять.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyTarget {
    /// На все экраны: сбрасывает назначения отдельных мониторов.
    AllScreens,
    /// На этот монитор.
    Monitor(ScreenIdentity),
}

/// Все назначения обоев по экранам.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScreenAssignments {
    /// Обои «на все экраны» — для всех, у кого нет своих.
    pub all: Option<WallpaperSource>,
    /// Свои обои мониторов по ключу ([`ScreenIdentity::key`]).
    pub monitors: BTreeMap<String, MonitorEntry>,
}

impl ScreenAssignments {
    /// Прочитать из карты `settings.json`. Ключа нет или он испорчен —
    /// пустые назначения, то есть прежнее поведение «одни обои на все
    /// экраны» (сами обои тогда знает вызывающий, как и раньше).
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

    /// Значение для `settings.json`.
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

    /// Записать в карту `settings.json` (остальные ключи не трогаются).
    pub fn write_settings(&self, settings: &mut Map<String, Value>) {
        settings.insert(KEY_SCREENS.into(), self.to_value());
    }

    /// Применить обои. «На все» сбрасывает свои обои мониторов — человек
    /// просил, чтобы везде было одно, и вернувшийся монитор тоже это
    /// покажет.
    pub fn assign(&mut self, target: &ApplyTarget, source: WallpaperSource, now: u64) {
        match target {
            ApplyTarget::AllScreens => {
                self.all = Some(source);
                self.monitors.clear();
            }
            ApplyTarget::Monitor(id) => {
                // Тот же монитор мог быть записан под соседней формой
                // ключа (с разъёмом и без) — старая запись не должна
                // всплыть вместо новой.
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

    /// Снять свои обои монитора: дальше он показывает «на все».
    pub fn clear_monitor(&mut self, id: &ScreenIdentity) -> bool {
        match self.lookup_key(id) {
            Some(k) => self.monitors.remove(&k).is_some(),
            None => false,
        }
    }

    /// Под каким ключом хранятся обои этого монитора.
    ///
    /// Порядок: точный ключ → ключ с текущим разъёмом → ключ без разъёма →
    /// единственная запись «этот монитор@какой-то разъём». Последние три —
    /// про одинаковые мониторы без серийника: пока второго нет, первый
    /// узнаётся и по старой записи с разъёмом, и наоборот.
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

    /// Свои обои монитора, если есть.
    pub fn monitor_source(&self, id: &ScreenIdentity) -> Option<&WallpaperSource> {
        self.lookup_key(id).and_then(|k| self.monitors.get(&k)).map(|e| &e.source)
    }

    /// Что показывать на экране: свои обои, иначе «на все», иначе ничего
    /// от Rexon (стол показывает свой фон).
    pub fn source_for(&self, id: &ScreenIdentity) -> Option<&WallpaperSource> {
        self.monitor_source(id).or(self.all.as_ref())
    }

    /// Отметить, что эти мониторы сейчас подключены, и забыть лишние
    /// отключённые сверх [`REMEMBER_MONITORS`] (давно не виденные первыми).
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

    /// Есть ли у кого-то свои обои (иначе — прежний режим «одни на все»).
    pub fn per_monitor(&self) -> bool {
        !self.monitors.is_empty()
    }
}

// ============================================================================
// План показа
// ============================================================================

/// Одна группа показа: одни обои на нескольких экранах — один процесс.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowGroup {
    /// Что показывать.
    pub source: WallpaperSource,
    /// Имена экранов у Qt (`ScreenIdentity::screen_name`) в порядке экранов.
    pub screens: Vec<String>,
    /// Ключи мониторов тех же экранов.
    pub keys: Vec<String>,
}

/// План: группы показа и экраны, на которых Rexon ничего не показывает.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShowPlan {
    /// Группы в порядке первого экрана.
    pub groups: Vec<ShowGroup>,
    /// Экраны без обоев Rexon (имена у Qt).
    pub uncovered: Vec<String>,
}

/// Разложить подключённые экраны по процессам показа.
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

/// Что сделать с процессами, чтобы перейти от плана `old` к плану `new`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanChange {
    /// Группы, которые остаются как были (процесс не трогать).
    pub keep: Vec<ShowGroup>,
    /// Те же обои, но набор экранов другой — перезапустить (или передать
    /// процессу новый список экранов).
    pub reshape: Vec<ShowGroup>,
    /// Новые обои — запустить.
    pub start: Vec<ShowGroup>,
    /// Обоев больше нигде нет — остановить.
    pub stop: Vec<ShowGroup>,
}

impl PlanChange {
    /// Ничего делать не надо.
    pub fn is_noop(&self) -> bool {
        self.reshape.is_empty() && self.start.is_empty() && self.stop.is_empty()
    }
}

/// Сравнить два плана. Группы сопоставляются по обоям: одинаковые обои —
/// тот же процесс.
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

/// Ключ строки запуска хоста со списком экранов.
pub const SCREENS_FLAG: &str = "--screens";

/// Список экранов для строки запуска: имена через запятую.
pub fn format_screens_arg(screens: &[String]) -> String {
    screens.join(",")
}

/// Разбор значения [`SCREENS_FLAG`]; пусто — «все экраны» (прежнее
/// поведение хоста без ключа).
pub fn parse_screens_arg(raw: &str) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

// ============================================================================
// Горячее подключение: выдержка
// ============================================================================

/// Тишина после последнего события, после которой экраны считаются
/// устоявшимися. Подключение монитора — это несколько событий подряд
/// (выход появился, сменил режим, сдвинулся), и раскладка по ним по одному
/// пересобирала бы показ несколько раз.
pub const HOTPLUG_QUIET_MS: u64 = 2000;

/// Дольше этого от первого события не ждём, даже если события идут:
/// разболтанный разъём не должен оставить человека без обоев навсегда.
pub const HOTPLUG_MAX_WAIT_MS: u64 = 10_000;

/// Выдержка над событиями «экраны изменились». Время — миллисекунды
/// любых монотонных часов, их даёт вызывающий.
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
    /// Выдержка с заданными сроками.
    pub fn new(quiet_ms: u64, max_wait_ms: u64) -> HotplugSettle {
        HotplugSettle { quiet_ms, max_wait_ms: max_wait_ms.max(quiet_ms), first: None, last: None }
    }

    /// Пришло событие.
    pub fn on_change(&mut self, now_ms: u64) {
        self.first.get_or_insert(now_ms);
        self.last = Some(now_ms);
    }

    /// Когда пересобирать (для таймера); `None` — ждать нечего.
    pub fn due_at(&self) -> Option<u64> {
        let (first, last) = (self.first?, self.last?);
        Some((last + self.quiet_ms).min(first + self.max_wait_ms))
    }

    /// Пора ли пересобирать. `true` — один раз на пачку событий: выдержка
    /// сбрасывается и ждёт следующую.
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

    /// Есть ли неотработанные события.
    pub fn pending(&self) -> bool {
        self.first.is_some()
    }
}

/// Моменты пересборки для ряда событий (время по возрастанию) — та же
/// выдержка, прогнанная по списку. Для проверок и журнала.
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
// Живое чтение (тонкие обёртки)
// ============================================================================

/// Каталог выходов DRM.
pub const DRM_SYSFS_DIR: &str = "/sys/class/drm";

/// Таблица производителей PNP из пакета hwdata.
pub const PNP_IDS_PATHS: &[&str] = &["/usr/share/hwdata/pnp.ids", "/usr/share/misc/pnp.ids"];

/// Прочитать выходы из каталога в формате `/sys/class/drm` (в проверках —
/// временный каталог). Порядок — по имени разъёма.
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

/// Выходы этой машины.
pub fn live_drm_outputs() -> Vec<DrmOutput> {
    let pnp = PNP_IDS_PATHS.iter().find_map(|p| std::fs::read_to_string(p).ok());
    read_drm_outputs(Path::new(DRM_SYSFS_DIR), pnp.as_deref())
}

// ============================================================================
// Проверки
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Синтетический EDID: свой генератор, а не снятый с чужих мониторов.
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
        // Первый дескриптор — режим (ненулевая частота), его разбор обязан
        // пропустить.
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
        b[9] = 0; // буквы «0» — не буквы
        assert_eq!(parse_edid(&b), Err(EdidError::BadVendor));
        let mut b = make_edid(&spec("DEL", 1, 1));
        b[127] ^= 0xFF;
        let e = parse_edid(&b).unwrap();
        assert!(!e.checksum_ok);
        assert_eq!(e.pnp_id, "DEL");
        // Блок расширения в хвосте не мешает.
        let mut b = make_edid(&spec("DEL", 1, 1));
        b.extend_from_slice(&[0x02; 128]);
        assert!(parse_edid(&b).is_ok());
    }

    #[test]
    fn edid_garbage_in_text_is_dropped() {
        let mut b = make_edid(&EdidSpec { name: Some("AB"), ..spec("DEL", 1, 1) });
        let off = DESCRIPTOR_OFFSETS[1] + 5;
        b[off + 1] = 0x07; // непечатаемое внутри имени
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

    // ── сопоставление с Qt ────────────────────────────────────────────

    #[test]
    fn wayland_match_by_connector_with_no_serial_from_qt() {
        let dell = EdidSpec { name: Some("DELL U2723QE"), serial_text: Some("7XK2R93"), ..spec("DEL", 0x41B2, 1) };
        let sam = EdidSpec { name: Some("LS27A600"), ..spec("SAM", 0x7140, 777) };
        let outs = vec![out("DP-1", Some(&dell)), out("HDMI-A-1", Some(&sam))];
        // Wayland: серийника нет, производитель названием.
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
        // Драйвер nvidia на X11 зовёт их с нуля: DP-0, DP-1. По имени пары
        // перекрёстно путаются — решает серийник.
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
        // Один такой — без разъёма: ходит за человеком по портам.
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

    // ── назначения ────────────────────────────────────────────────────

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
        // Испорченное значение — тоже прежнее поведение.
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
        // Запись без пути отбрасывается, а не превращается в пустые обои.
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
        // Отключили — в плане только ноутбук.
        let plan = plan_show(std::slice::from_ref(&lap), &a);
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].source.path, "/all.mp4");
        assert!(a.monitors.contains_key(&ext.key));
        // Подключили в ДРУГОЙ порт — ключ тот же, обои вернулись.
        let ext_moved = id("edid:SAM:7140:777", "edid:SAM:7140:777", "DP-2");
        assert_eq!(a.source_for(&ext_moved), Some(&src("/sam.mp4", "video")));
        a.mark_seen(&[ext_moved], 9);
        assert_eq!(a.monitors[&ext.key].connector, "DP-2");
        assert_eq!(a.monitors[&ext.key].last_seen, 9);
    }

    #[test]
    fn twin_monitors_lookup_between_key_forms() {
        let mut a = ScreenAssignments::default();
        // Пока второго нет — запись без разъёма.
        a.assign(&ApplyTarget::Monitor(id("hp:", "hp:", "DP-1")), src("/one", "video"), 1);
        // Пришёл близнец: у обоих ключи с разъёмом; первый узнаётся по базе.
        assert_eq!(a.source_for(&id("hp:@DP-1", "hp:", "DP-1")), Some(&src("/one", "video")));
        // Назначили по формам с разъёмом — старая запись без разъёма ушла.
        a.assign(&ApplyTarget::Monitor(id("hp:@DP-1", "hp:", "DP-1")), src("/one", "video"), 2);
        a.assign(&ApplyTarget::Monitor(id("hp:@HDMI-A-1", "hp:", "HDMI-A-1")), src("/two", "video"), 2);
        assert_eq!(a.monitors.len(), 2);
        // Близнеца унесли: оставшийся один на DP-1 — его запись с разъёмом.
        assert_eq!(a.source_for(&id("hp:", "hp:", "DP-1")), Some(&src("/one", "video")));
        // Две записи с разъёмом, а монитор один и на третьем порту — не
        // угадываем, какой это из двух.
        assert_eq!(a.monitor_source(&id("hp:", "hp:", "DP-3")), None);
        // Один из записанных — угадываем.
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

    // ── план показа ───────────────────────────────────────────────────

    #[test]
    fn same_wallpaper_is_one_process() {
        let mut a = ScreenAssignments::default();
        a.assign(&ApplyTarget::AllScreens, src("/a.mp4", "video"), 1);
        let screens = vec![id("k1", "k1", "DP-1"), id("k2", "k2", "HDMI-A-1"), id("k3", "k3", "eDP-1")];
        let plan = plan_show(&screens, &a);
        assert_eq!(plan.groups.len(), 1);
        assert_eq!(plan.groups[0].screens, vec!["DP-1", "HDMI-A-1", "eDP-1"]);
        assert!(plan.uncovered.is_empty());
        // Свои обои у второго монитора — второй процесс; у третьего те же,
        // что «на все» (подпись другая) — он в первой группе.
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
        // Внешний без своих обоев — тот же процесс растёт на второй экран.
        a.clear_monitor(&ext);
        let after = plan_show(&[lap, ext], &a);
        let ch = plan_change(&before, &after);
        assert_eq!(ch.reshape.len(), 1);
        assert_eq!(ch.reshape[0].screens, vec!["eDP-1", "HDMI-A-1"]);
        // Обратно — снова тот же процесс сжимается, ничего не стопорится.
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

    // ── выдержка ──────────────────────────────────────────────────────

    #[test]
    fn hotplug_burst_settles_once() {
        // Подключение: четыре события за 600 мс — одна пересборка через 2 с
        // после последнего.
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
        // Событие каждые 500 мс без конца — пересборка не позже 10 с от
        // первого, потом следующая пачка.
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

    // ── живое чтение на подставном каталоге ───────────────────────────

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
        // Wayland-экран с названием производителя сходится по разъёму + имени.
        let ids = identify_screens(&[qt("HDMI-A-1", "Samsung Electric Company", "LS27A600", "")], &outs);
        assert_eq!(ids[0].key, "edid:SAM:7140:9");
    }
}
