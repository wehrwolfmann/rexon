//! Сторож игрового режима (gamescope-session-plus, «как Steam Machine»):
//! не оставлять человека перед чёрным экраном.
//!
//! ЗАЧЕМ. gamescope 3.17 (коммит `14328a835`, 06.2026) без подключённого
//! экрана больше НЕ падает: он работает на «Virtual screen», Steam
//! запускается, сеанс жив, а экран чёрный. Счётчик коротких сеансов
//! gamescope-session-plus при этом молчит. На гибридном ноутбуке так бывает,
//! как только gamescope возьмёт не ту видеокарту: DRM-узел он открывает у
//! выбранного Vulkan-устройства, а оно — первое по порядку, если
//! `--prefer-vk-device` (`VULKAN_ADAPTER`) не задан или указывает на
//! несуществующее устройство (тогда — МОЛЧА первое). Порядок легко
//! перевернуть: `DRI_PRIME=1` в `~/.config/environment.d`, который сеанс
//! подгружает сам.
//!
//! КАК (без root). gamescope-session-plus подхватывает
//! `~/.config/gamescope-session-plus/sessions.d/steam` ПОСЛЕ системного
//! `sessions.d/steam`. Туда Rexon кладёт тонкий сценарий ([`hook_script`]):
//!   1. до запуска gamescope — `rexon --gamemode-preflight`
//!      ([`decide_preflight`]): какая карта держит встроенную панель (eDP,
//!      LVDS, DSI) по `/sys/class/drm` ([`scan_drm`]); её `vendor:device`
//!      подставляется в `VULKAN_ADAPTER`, если он пуст, указывает на карту
//!      без экрана или на устройство, которого нет. Vulkan-устройство PCI-карты
//!      несёт те же `vendorID:deviceID`, что `/sys/.../device/{vendor,device}`,
//!      а `--prefer-vk-device` понимает ровно `vendor:device` — PCI-адрес
//!      (06:00.0) нужен только человеку в журнале. Две одинаковые карты
//!      так не различить — тогда ничего не меняем ([`KeepReason::SameIdTwice`]);
//!   2. сразу после ответа gamescope (`post_gamescope_start`, системное тело
//!      сохраняется и зовётся первым) — `rexon --gamemode-watch` в фоне
//!      ([`classify_log`], [`output_step`], [`ready_step`]): журнал юнита
//!      `gamescope-session-plus@steam` (`drm: selecting connector …` ✅ /
//!      `drm: cannot find any connected connector!` ⛔), потом корневые
//!      свойства `GAMESCOPE_FOCUSED_APP` = 769 (интерфейс Steam). Отказ —
//!      запись в `$XDG_STATE_HOME/rexon/gamemode/failures` и остановка юнита:
//!      сеанс кончается, человек возвращается к экрану входа и выбирает стол;
//!   3. `short_session_recover` (5 коротких сеансов подряд): системное тело
//!      (починка Steam) + запись отказа; системный откат зовёт `steamosctl`,
//!      которого на обычном Arch нет.
//!
//! ЗАЩИТА ОТ ЛОЖНОЙ ТРЕВОГИ. Сеанс гасится сразу только по однозначным
//! строкам журнала (нет экрана, не прошёл modeset). «Steam не показался за
//! T» гасит сеанс лишь до первого удачного входа ([`should_end_session`]):
//! после него долгая загрузка (обновление Steam) только записывается.
//!
//! Сценарий в `sessions.d` чужой файл не перезаписывает ([`hook_change`]) и
//! убирает сам себя, когда бинаря Rexon больше нет.

use crate::i18n::{tr, trf};
use std::path::{Path, PathBuf};

/// До запуска gamescope: выбрать карту экрана (печатает одну машинную строку).
pub const PREFLIGHT_FLAG: &str = "--gamemode-preflight";
/// В сеансе: следить, появилась ли картинка.
pub const WATCH_FLAG: &str = "--gamemode-watch";
/// Счётчик коротких сеансов сработал (зовёт `short_session_recover`).
pub const RECOVER_FLAG: &str = "--gamemode-recover";
/// Положить сценарий сторожа в `sessions.d` (руками, для пробы).
pub const INSTALL_FLAG: &str = "--gamemode-guard-install";
/// Убрать сценарий сторожа.
pub const REMOVE_FLAG: &str = "--gamemode-guard-remove";
/// Не гасить сеанс и ничего не записывать — только сказать.
pub const DRY_RUN_FLAG: &str = "--dry-run";
/// Читать журнал gamescope из файла (проверка без живого сеанса).
pub const LOG_FLAG: &str = "--log";
/// С какого момента (секунды эпохи) читать журнал юнита.
pub const SINCE_FLAG: &str = "--since";

/// Клиент gamescope-session-plus, за которым смотрим.
pub const CLIENT: &str = "steam";
/// Системный сценарий сеанса: в нём должны быть крючки, которые мы заменяем.
pub const SESSION_SCRIPT: &str = "/usr/share/gamescope-session-plus/gamescope-session-plus";
/// Где лежит наш сценарий относительно `$XDG_CONFIG_HOME`.
pub const HOOK_REL_PATH: &str = "gamescope-session-plus/sessions.d/steam";
/// Первая строка-подпись: по ней отличаем свой файл от файла человека.
pub const HOOK_MARKER: &str = "# Rexon: game mode guard.";
/// appid интерфейса Steam в gamescope (`steamcompmgr.cpp`).
pub const STEAM_APPID: u64 = 769;
/// Потолок предполётной проверки в сценарии: вход важнее сторожа.
pub const PREFLIGHT_TIMEOUT_SECS: u64 = 5;
/// Строка `VULKAN_ADAPTER` в ответе предполётной проверки.
pub const SET_PREFIX: &str = "set VULKAN_ADAPTER=";
/// Ответ «вход бесполезен — сразу назад».
pub const FAIL_PREFIX: &str = "fail ";
/// Ответ «ничего не менять».
pub const KEEP_LINE: &str = "keep";

/// Имя юнита сеанса.
pub fn unit_name() -> String {
    format!("gamescope-session-plus@{CLIENT}.service")
}

// ============================================================================
// Видеокарты и экраны (/sys/class/drm)
// ============================================================================

/// Разъём карты: `eDP-1`, `HDMI-A-1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connector {
    pub name: String,
    pub connected: bool,
}

/// DRM-карта: `card1` → PCI `0000:06:00.0`, `1002:15bf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrmCard {
    /// `card1`. Номер между загрузками меняется — опираться на него нельзя.
    pub card: String,
    /// PCI-адрес (`0000:06:00.0`), если карта на PCI.
    pub pci: Option<String>,
    pub vendor: u16,
    pub device: u16,
    /// Драйвер (`amdgpu`, `nvidia`).
    pub driver: Option<String>,
    pub boot_vga: bool,
    pub connectors: Vec<Connector>,
}

impl DrmCard {
    /// `vendor:device` в том виде, что понимает `--prefer-vk-device`.
    pub fn vk_id(&self) -> String {
        format!("{:04x}:{:04x}", self.vendor, self.device)
    }
    /// Подключённая встроенная панель (первая).
    pub fn connected_panel(&self) -> Option<&str> {
        self.connectors.iter().find(|c| c.connected && is_internal_panel(&c.name)).map(|c| c.name.as_str())
    }
    /// Есть ли вообще подключённый экран.
    pub fn any_connected(&self) -> bool {
        self.connectors.iter().any(|c| c.connected)
    }
    /// Для журнала: `card1 (0000:06:00.0, 1002:15bf, amdgpu)`.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(p) = &self.pci {
            parts.push(p.clone());
        }
        parts.push(self.vk_id());
        if let Some(d) = &self.driver {
            parts.push(d.clone());
        }
        format!("{} ({})", self.card, parts.join(", "))
    }
}

/// Встроенная панель ноутбука/приставки: eDP, LVDS, DSI.
pub fn is_internal_panel(connector: &str) -> bool {
    ["eDP-", "LVDS-", "DSI-"].iter().any(|p| connector.starts_with(p))
}

fn parse_hex_id(text: &str) -> Option<u16> {
    let t = text.trim();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    u16::from_str_radix(t, 16).ok()
}

/// `card12` → 12; `card1-eDP-1`, `renderD128` → None.
fn card_number(name: &str) -> Option<u32> {
    name.strip_prefix("card").filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))?.parse().ok()
}

/// PCI-адрес по имени каталога устройства (`0000:06:00.0`).
fn looks_like_pci(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 12 && b[4] == b':' && b[7] == b':' && b[10] == b'.'
}

/// Прочитать карты из `/sys/class/drm` (в тестах — из подставного каталога).
/// Только чтение; карты без `device/vendor` (виртуальные) пропускаются.
pub fn scan_drm(class_drm: &Path) -> Vec<DrmCard> {
    let Ok(rd) = std::fs::read_dir(class_drm) else { return Vec::new() };
    let names: Vec<String> = rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    let mut cards: Vec<(u32, DrmCard)> = Vec::new();
    for name in &names {
        let Some(num) = card_number(name) else { continue };
        let dev = class_drm.join(name).join("device");
        let read = |f: &str| std::fs::read_to_string(dev.join(f)).ok();
        let (Some(vendor), Some(device)) =
            (read("vendor").as_deref().and_then(parse_hex_id), read("device").as_deref().and_then(parse_hex_id))
        else {
            continue;
        };
        let pci = std::fs::canonicalize(&dev)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .filter(|n| looks_like_pci(n));
        let driver = std::fs::read_link(dev.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        let boot_vga = read("boot_vga").map(|s| s.trim() == "1").unwrap_or(false);
        let prefix = format!("{name}-");
        let mut connectors: Vec<Connector> = names
            .iter()
            .filter_map(|n| n.strip_prefix(&prefix))
            .filter(|c| !c.starts_with("Writeback"))
            .map(|c| Connector {
                name: c.to_string(),
                connected: std::fs::read_to_string(class_drm.join(format!("{prefix}{c}")).join("status"))
                    .map(|s| s.trim() == "connected")
                    .unwrap_or(false),
            })
            .collect();
        connectors.sort_by(|a, b| a.name.cmp(&b.name));
        cards.push((num, DrmCard { card: name.clone(), pci, vendor, device, driver, boot_vga, connectors }));
    }
    cards.sort_by_key(|(n, _)| *n);
    cards.into_iter().map(|(_, c)| c).collect()
}

/// Разобрать `VULKAN_ADAPTER` так же, как gamescope (`%X:%X`): `1002:15bf`,
/// `0x1002:0x15BF`. Не разобралось — None (gamescope тогда берёт первое).
pub fn parse_adapter(text: &str) -> Option<(u16, u16)> {
    let (v, d) = text.trim().trim_matches('"').trim_matches('\'').split_once(':')?;
    Some((parse_hex_id(v)?, parse_hex_id(d)?))
}

// ============================================================================
// Переменные, которые сеанс подгрузит ПОСЛЕ нас
// ============================================================================

/// Строки `KEY=VALUE` файла `environment.d` (как их понимает `.` под
/// `set -a`: необязательный `export`, кавычки снимаются, `#` — комментарий).
pub fn parse_env_lines(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let l = l.strip_prefix("export ").map(str::trim_start).unwrap_or(l);
            let (k, v) = l.split_once('=')?;
            if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return None;
            }
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                .unwrap_or(v);
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// Переменные, которые ПРЯЧУТ Vulkan-устройства (а не только меняют
/// порядок): с ними `--prefer-vk-device` может не найти карту экрана.
/// `DRI_PRIME`/`__NV_PRIME_RENDER_OFFLOAD` порядок меняют, но
/// `VULKAN_ADAPTER` их перебивает — о них не предупреждаем.
pub fn device_hiding_vars(env: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = env
        .iter()
        .filter(|(k, v)| match k.as_str() {
            "__VK_LAYER_NV_optimus" => v == "NVIDIA_only",
            "MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE" => v == "1",
            "VK_ICD_FILENAMES" | "VK_DRIVER_FILES" | "VK_LOADER_DRIVERS_SELECT" => !v.is_empty(),
            _ => false,
        })
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    out.sort();
    out.dedup();
    out
}

// ============================================================================
// Предполётная проверка (чистое решение)
// ============================================================================

/// Почему ничего не меняем.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeepReason {
    /// Одна карта: выбирать не из чего.
    SingleCard,
    /// `VULKAN_ADAPTER` уже указывает на карту экрана.
    AlreadyScreenCard,
    /// Человек (или device-quirks) выбрал другую карту, и у неё есть экран.
    ChosenCardHasScreen(String),
    /// У карты экрана такой же `vendor:device`, как у другой карты.
    SameIdTwice(String),
    /// Карт с `vendor` нет вовсе (gamescope упадёт сам — это заметит счётчик).
    NoCards,
}

/// Почему вход бесполезен: картинки не будет, лучше сразу к экрану входа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailReason {
    /// Ни у одной карты нет подключённого экрана.
    NoConnectedOutput,
    /// Экран на NVIDIA, а `nvidia_drm.modeset` не `Y`.
    NvidiaModesetOff,
    /// `VULKAN_ADAPTER` из `environment.d` (подгрузится после нас и
    /// перебьёт нашу правку) указывает на карту без экрана.
    EnvironmentAdapterWithoutScreen(String),
}

impl FailReason {
    /// Машинный код (журнал отказов, ответ сценарию).
    pub fn code(&self) -> &'static str {
        match self {
            FailReason::NoConnectedOutput => "no-connected-output",
            FailReason::NvidiaModesetOff => "nvidia-modeset-off",
            FailReason::EnvironmentAdapterWithoutScreen(_) => "environment-adapter-without-screen",
        }
    }
}

/// Итог предполётной проверки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preflight {
    Keep(KeepReason),
    /// Подставить `VULKAN_ADAPTER` (`replaced` — что было, если было).
    Set { adapter: String, card: String, replaced: Option<String> },
    Fail(FailReason),
}

impl Preflight {
    /// Одна строка для сценария в `sessions.d`.
    pub fn machine_line(&self) -> String {
        match self {
            Preflight::Keep(_) => KEEP_LINE.to_string(),
            Preflight::Set { adapter, .. } => format!("{SET_PREFIX}{adapter}"),
            Preflight::Fail(r) => format!("{FAIL_PREFIX}{}", r.code()),
        }
    }
}

/// Что известно до запуска gamescope.
#[derive(Debug, Clone, Default)]
pub struct PreflightInput<'a> {
    pub cards: &'a [DrmCard],
    /// `VULKAN_ADAPTER` сейчас (системный `sessions.d`, device-quirks).
    pub adapter: Option<&'a str>,
    /// `VULKAN_ADAPTER` из `environment.d`: сеанс подгрузит его ПОСЛЕ нас.
    pub environment_adapter: Option<&'a str>,
    /// `/sys/module/nvidia_drm/parameters/modeset` = `Y` (None — модуля нет).
    pub nvidia_modeset: Option<bool>,
}

/// Карта, на которой должна быть картинка: со встроенной панелью, иначе —
/// с любым подключённым экраном (сначала `boot_vga`).
pub fn screen_card(cards: &[DrmCard]) -> Option<&DrmCard> {
    cards
        .iter()
        .find(|c| c.connected_panel().is_some())
        .or_else(|| cards.iter().find(|c| c.boot_vga && c.any_connected()))
        .or_else(|| cards.iter().find(|c| c.any_connected()))
}

fn card_by_adapter<'a>(cards: &'a [DrmCard], adapter: &str) -> Option<&'a DrmCard> {
    let (v, d) = parse_adapter(adapter)?;
    cards.iter().find(|c| c.vendor == v && c.device == d)
}

/// Решение: что делать с `VULKAN_ADAPTER` перед запуском gamescope.
pub fn decide_preflight(input: &PreflightInput) -> Preflight {
    let cards = input.cards;
    if cards.is_empty() {
        return Preflight::Keep(KeepReason::NoCards);
    }
    let Some(target) = screen_card(cards) else {
        return Preflight::Fail(FailReason::NoConnectedOutput);
    };
    if target.vendor == 0x10de && input.nvidia_modeset == Some(false) {
        return Preflight::Fail(FailReason::NvidiaModesetOff);
    }
    // environment.d перебьёт всё, что мы поставим: можно только понять,
    // будет ли картинка.
    if let Some(env) = input.environment_adapter.filter(|s| !s.trim().is_empty()) {
        return match card_by_adapter(cards, env) {
            Some(c) if !c.any_connected() => Preflight::Fail(FailReason::EnvironmentAdapterWithoutScreen(env.to_string())),
            // Есть такая карта с экраном — выбор человека. Нет такой —
            // gamescope возьмёт первую; это решит сторож по журналу.
            _ => Preflight::Keep(KeepReason::ChosenCardHasScreen(env.to_string())),
        };
    }
    if cards.len() == 1 {
        return Preflight::Keep(KeepReason::SingleCard);
    }
    let target_id = target.vk_id();
    if cards.iter().filter(|c| c.vk_id() == target_id).count() > 1 {
        return Preflight::Keep(KeepReason::SameIdTwice(target_id));
    }
    let current = input.adapter.map(str::trim).filter(|s| !s.is_empty());
    match current.map(|a| (a, card_by_adapter(cards, a))) {
        None => Preflight::Set { adapter: target_id, card: target.card.clone(), replaced: None },
        Some((_, Some(c))) if c.vk_id() == target_id => Preflight::Keep(KeepReason::AlreadyScreenCard),
        Some((a, Some(c))) if c.any_connected() => Preflight::Keep(KeepReason::ChosenCardHasScreen(a.to_string())),
        // Карта без экрана или устройства нет (опечатка, MUX выключил):
        // gamescope остался бы без экрана.
        Some((a, _)) => Preflight::Set { adapter: target_id, card: target.card.clone(), replaced: Some(a.to_string()) },
    }
}

/// Что сказать человеку (журнал юнита / консоль).
pub fn preflight_lines(input: &PreflightInput, decision: &Preflight, hiding: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for c in input.cards {
        let connected: Vec<&str> = c.connectors.iter().filter(|x| x.connected).map(|x| x.name.as_str()).collect();
        let screens = if connected.is_empty() { "-".to_string() } else { connected.join(", ") };
        // TRANSLATORS: game mode check, one line per video card. %1 = "card1 (0000:06:00.0, 1002:15bf, amdgpu)", %2 = connected screens like "eDP-1".
        out.push(trf("Video card %1: connected screens: %2", &[&c.describe(), &screens]));
    }
    out.push(match decision {
        // TRANSLATORS: game mode check result. %1 = "1002:15bf", %2 = "card1".
        Preflight::Set { adapter, card, replaced: None } => trf("Game mode will draw on the card with the screen: VULKAN_ADAPTER=%1 (%2).", &[adapter, card]),
        // TRANSLATORS: game mode check result. %1 = old value, %2 = new "1002:15bf", %3 = "card1".
        Preflight::Set { adapter, card, replaced: Some(old) } => trf(
            "VULKAN_ADAPTER=%1 points to a card without a screen or to no card at all; using %2 (%3) instead.",
            &[old, adapter, card],
        ),
        Preflight::Keep(KeepReason::SingleCard) => tr("One video card: nothing to choose."),
        Preflight::Keep(KeepReason::AlreadyScreenCard) => tr("VULKAN_ADAPTER already points to the card with the screen."),
        // TRANSLATORS: game mode check result. %1 = the VULKAN_ADAPTER value the user set.
        Preflight::Keep(KeepReason::ChosenCardHasScreen(a)) => trf("VULKAN_ADAPTER=%1 is set by you; left as it is.", &[a]),
        // TRANSLATORS: game mode check result. %1 = "1002:15bf".
        Preflight::Keep(KeepReason::SameIdTwice(id)) => trf("Two video cards have the same id %1, they can't be told apart; nothing changed.", &[id]),
        Preflight::Keep(KeepReason::NoCards) => tr("No video cards found."),
        Preflight::Fail(r) => fail_text(r),
    });
    if !hiding.is_empty() {
        // TRANSLATORS: game mode check warning. %1 = variables like "__VK_LAYER_NV_optimus=NVIDIA_only".
        out.push(trf("These settings hide video cards from games and gamescope, the screen may stay black: %1", &[&hiding.join(" ")]));
    }
    out
}

fn fail_text(r: &FailReason) -> String {
    match r {
        FailReason::NoConnectedOutput => tr("No screen is connected to any video card: game mode would show a black screen."),
        FailReason::NvidiaModesetOff => tr("The screen is on the NVIDIA card, but nvidia_drm.modeset is off: game mode can't show a picture."),
        // TRANSLATORS: %1 = the VULKAN_ADAPTER value from ~/.config/environment.d.
        FailReason::EnvironmentAdapterWithoutScreen(a) => {
            trf("VULKAN_ADAPTER=%1 in environment.d points to a card without a screen: game mode would show a black screen.", &[a])
        }
    }
}

// ============================================================================
// Сторож в сеансе (чистые решения)
// ============================================================================

/// Что журнал gamescope говорит о выводе.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputSignal {
    /// Решения ещё нет.
    Pending,
    /// ✅ выбран настоящий разъём.
    Selected(String),
    /// ⛔ безэкранный запуск (`node` — какой DRM-узел открыт).
    NoConnector { node: Option<String> },
    /// ⛔ вывод не поднялся.
    ModesetFailed(String),
}

fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Разобрать журнал gamescope (строки по порядку). Безэкранный запуск
/// решает сразу; сбой modeset после выбора разъёма — тоже отказ.
pub fn classify_log(text: &str) -> OutputSignal {
    let mut node = None;
    let mut selected = None;
    for raw in text.lines() {
        let line = strip_ansi(raw);
        if let Some(rest) = line.split("opening DRM node ").nth(1) {
            node = Some(rest.trim().trim_matches(|c| c == '\'' || c == '"').to_string());
        }
        if line.contains("cannot find any connected connector") {
            return OutputSignal::NoConnector { node };
        }
        if line.contains("We failed our modeset") || line.contains("fatal flip error") {
            return OutputSignal::ModesetFailed(line.trim().to_string());
        }
        if selected.is_none() {
            if let Some(rest) = line.split("drm: selecting connector").nth(1) {
                selected = Some(rest.trim().to_string());
            }
        }
    }
    selected.map(OutputSignal::Selected).unwrap_or(OutputSignal::Pending)
}

/// Сроки сторожа, секунды.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// gamescope должен выбрать разъём.
    pub output: u64,
    /// Ни одного окна в фокусе столько времени — это не загрузка.
    pub steam: u64,
    /// Потолок, если хоть что-то на экране было (обновление Steam).
    pub slow: u64,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts { output: 20, steam: 180, slow: 600 }
    }
}

/// Отказ сторожа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchFailure {
    NoConnectedOutput { node: Option<String> },
    ModesetFailed(String),
    NoOutputDecision,
    SteamNotShown,
    SteamNotReady,
}

impl WatchFailure {
    pub fn code(&self) -> &'static str {
        match self {
            WatchFailure::NoConnectedOutput { .. } => "no-connected-output",
            WatchFailure::ModesetFailed(_) => "modeset-failed",
            WatchFailure::NoOutputDecision => "no-output-decision",
            WatchFailure::SteamNotShown => "steam-not-shown",
            WatchFailure::SteamNotReady => "steam-not-ready",
        }
    }
    /// Однозначный ли это чёрный экран (по журналу), а не «долго грузится».
    pub fn is_certain(&self) -> bool {
        matches!(self, WatchFailure::NoConnectedOutput { .. } | WatchFailure::ModesetFailed(_))
    }
    /// Для человека.
    pub fn text(&self) -> String {
        match self {
            WatchFailure::NoConnectedOutput { node } => {
                let node = node.clone().unwrap_or_else(|| "?".to_string());
                // TRANSLATORS: game mode watcher. %1 = DRM node like "/dev/dri/card2".
                trf("gamescope opened the video card %1, which has no screen connected: the screen is black.", &[&node])
            }
            // TRANSLATORS: game mode watcher. %1 = the gamescope log line.
            WatchFailure::ModesetFailed(line) => trf("gamescope could not turn the screen on: %1", &[line]),
            WatchFailure::NoOutputDecision => tr("gamescope did not pick a screen in time."),
            WatchFailure::SteamNotShown => tr("Steam did not show any window in time."),
            WatchFailure::SteamNotReady => tr("Steam did not finish starting in time."),
        }
    }
}

/// Шаг сторожа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchStep {
    Wait,
    /// Первая стадия пройдена (разъём выбран).
    Proceed(String),
    Ready,
    Fail(WatchFailure),
}

/// Стадия 1: выбран ли настоящий разъём.
pub fn output_step(signal: &OutputSignal, elapsed: u64, t: &Timeouts) -> WatchStep {
    match signal {
        OutputSignal::NoConnector { node } => WatchStep::Fail(WatchFailure::NoConnectedOutput { node: node.clone() }),
        OutputSignal::ModesetFailed(l) => WatchStep::Fail(WatchFailure::ModesetFailed(l.clone())),
        OutputSignal::Selected(c) => WatchStep::Proceed(c.clone()),
        OutputSignal::Pending if elapsed > t.output => WatchStep::Fail(WatchFailure::NoOutputDecision),
        OutputSignal::Pending => WatchStep::Wait,
    }
}

/// Стадия 2: показан ли Steam. `focused_app`/`focused_window` — корневые
/// свойства gamescope; `seen_window` — окно в фокусе уже бывало.
pub fn ready_step(elapsed: u64, focused_app: Option<u64>, focused_window: Option<u64>, seen_window: bool, t: &Timeouts) -> WatchStep {
    if focused_app == Some(STEAM_APPID) {
        return WatchStep::Ready;
    }
    let seen = seen_window || focused_window.is_some_and(|w| w != 0);
    if !seen && elapsed > t.steam {
        return WatchStep::Fail(WatchFailure::SteamNotShown);
    }
    if elapsed > t.slow {
        return WatchStep::Fail(WatchFailure::SteamNotReady);
    }
    WatchStep::Wait
}

/// Гасить ли сеанс. Однозначный чёрный экран — всегда; «не дождались
/// Steam» — только пока игровой режим ни разу не входил удачно.
pub fn should_end_session(failure: &WatchFailure, had_success_before: bool) -> bool {
    failure.is_certain() || !had_success_before
}

/// Значение CARDINAL из вывода `xprop -root NAME`:
/// `GAMESCOPE_FOCUSED_APP(CARDINAL) = 769` → 769; «not found» → None.
pub fn parse_xprop_cardinal(text: &str) -> Option<u64> {
    let (_, v) = text.lines().next()?.split_once('=')?;
    v.split(',').next()?.trim().parse().ok()
}

// ============================================================================
// Сценарий в sessions.d
// ============================================================================

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Текст сценария. Его подгружает (`.`) сам gamescope-session-plus под
/// `set -a`, после системного `sessions.d/steam`. Поэтому: ни `set -e`, ни
/// чужих имён (`CLIENT` в сеансе readonly), всё своё — с `_rexon_`;
/// системные `post_gamescope_start` и `short_session_recover` сохраняются
/// под своими именами и зовутся первыми (OGC может поменять их тело — мы
/// его не копируем). `exit 1` — только на однозначном «картинки не будет»:
/// сеанс кончается, человек у экрана входа, счётчик коротких сеансов не растёт.
pub fn hook_script(program: &str, hook_path: &str) -> String {
    let bin = sh_quote(program);
    format!(
        "{HOOK_MARKER} Written and removed by Rexon ({program} {REMOVE_FLAG}).\n\
         # gamescope-session-plus sources this after the system sessions.d/steam.\n\
         _rexon_bin={bin}\n\
         if [ -x \"$_rexon_bin\" ]; then\n\
         \x20   # 1) before gamescope: the video card that drives the built-in screen\n\
         \x20   _rexon_pf=$(timeout -k 1 {PREFLIGHT_TIMEOUT_SECS} \"$_rexon_bin\" {PREFLIGHT_FLAG} </dev/null) || true\n\
         \x20   case \"$_rexon_pf\" in\n\
         \x20       '{SET_PREFIX}'*) VULKAN_ADAPTER=\"${{_rexon_pf#{SET_PREFIX}}}\" ;;\n\
         \x20       '{FAIL_PREFIX}'*) echo \"rexon: game mode would show a black screen (${{_rexon_pf#{FAIL_PREFIX}}}), ending the session\" >&2; exit 1 ;;\n\
         \x20   esac\n\
         \x20   unset _rexon_pf\n\
         \x20   # 2) right after gamescope answered: watch that a picture really appears\n\
         \x20   if declare -F post_gamescope_start >/dev/null; then\n\
         \x20       eval \"_rexon_system_$(declare -f post_gamescope_start)\"\n\
         \x20   else\n\
         \x20       _rexon_system_post_gamescope_start() {{ :; }}\n\
         \x20   fi\n\
         \x20   post_gamescope_start() {{\n\
         \x20       _rexon_system_post_gamescope_start \"$@\"\n\
         \x20       \"$_rexon_bin\" {WATCH_FLAG} {SINCE_FLAG} \"$(( EPOCHSECONDS - SECONDS - 5 ))\" </dev/null &\n\
         \x20   }}\n\
         \x20   # 3) five short sessions in a row: keep the system repair, then tell Rexon\n\
         \x20   if declare -F short_session_recover >/dev/null; then\n\
         \x20       eval \"_rexon_system_$(declare -f short_session_recover)\"\n\
         \x20   else\n\
         \x20       _rexon_system_short_session_recover() {{ :; }}\n\
         \x20   fi\n\
         \x20   short_session_recover() {{\n\
         \x20       _rexon_system_short_session_recover \"$@\" || true\n\
         \x20       \"$_rexon_bin\" {RECOVER_FLAG} </dev/null || true\n\
         \x20   }}\n\
         else\n\
         \x20   rm -f {} 2>/dev/null || true\n\
         fi\n",
        sh_quote(hook_path)
    )
}

/// Есть ли в системном сценарии крючки, на которые мы опираемся.
pub fn session_script_supported(text: &str) -> bool {
    ["post_gamescope_start", "short_session_recover", "CLIENT_CONFIG_DIR_LOCAL", "--prefer-vk-device $VULKAN_ADAPTER"]
        .iter()
        .all(|n| text.contains(n))
}

/// Что сделать со сценарием.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookChange {
    Nothing,
    Write(String),
    Remove,
    /// На этом месте файл человека — не трогаем.
    ForeignFile,
}

/// Свой ли это файл.
pub fn is_our_hook(text: &str) -> bool {
    text.starts_with(HOOK_MARKER)
}

/// План: `wanted` — сторож нужен; `program` — абсолютный путь бинаря.
pub fn hook_change(wanted: bool, program: Option<&str>, hook_path: &str, existing: Option<&str>) -> HookChange {
    if existing.is_some_and(|e| !is_our_hook(e)) {
        return HookChange::ForeignFile;
    }
    match (wanted, program) {
        (true, Some(p)) if p.starts_with('/') => {
            let text = hook_script(p, hook_path);
            if existing == Some(text.as_str()) {
                HookChange::Nothing
            } else {
                HookChange::Write(text)
            }
        }
        _ if existing.is_some() => HookChange::Remove,
        _ => HookChange::Nothing,
    }
}

/// Пути сторожа.
#[derive(Debug, Clone)]
pub struct GamemodePaths {
    pub config_home: PathBuf,
    /// `$XDG_STATE_HOME/rexon/gamemode`.
    pub state_dir: PathBuf,
}

impl GamemodePaths {
    pub fn new(config_home: impl Into<PathBuf>, state_dir: impl Into<PathBuf>) -> Self {
        Self { config_home: config_home.into(), state_dir: state_dir.into() }
    }
    pub fn live() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let state = crate::utils::state_home(std::env::var("XDG_STATE_HOME").ok().as_deref(), &home);
        Self::new(crate::utils::config_home_env(), Path::new(&state).join("rexon/gamemode"))
    }
    pub fn hook(&self) -> PathBuf {
        self.config_home.join(HOOK_REL_PATH)
    }
    /// Журнал отказов: `<время> <код> <текст>`.
    pub fn failures(&self) -> PathBuf {
        self.state_dir.join("failures")
    }
    /// Когда игровой режим последний раз показал Steam.
    pub fn last_ok(&self) -> PathBuf {
        self.state_dir.join("last-ok")
    }
}

/// Применить план сценария.
pub fn sync_hook(paths: &GamemodePaths, wanted: bool, program: Option<&str>) -> Result<HookChange, String> {
    let path = paths.hook();
    let existing = std::fs::read_to_string(&path).ok();
    let change = hook_change(wanted, program, &path.to_string_lossy(), existing.as_deref());
    match &change {
        HookChange::Nothing | HookChange::ForeignFile => {}
        HookChange::Write(text) => crate::session_guard::write_atomic(&path, text)?,
        HookChange::Remove => std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?,
    }
    Ok(change)
}

/// Дописать отказ в журнал.
pub fn record_failure(paths: &GamemodePaths, when: &str, code: &str, text: &str) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(&paths.state_dir).map_err(|e| format!("{}: {e}", paths.state_dir.display()))?;
    let path = paths.failures();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let one_line = text.replace('\n', " ");
    writeln!(f, "{when} {code} {one_line}").map_err(|e| format!("{}: {e}", path.display()))
}

// ============================================================================
// Живые маршруты
// ============================================================================

fn now_epoch() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn now_stamp() -> String {
    crate::engine_update::now_iso(now_epoch() as i64)
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let pos = args.iter().position(|a| a == flag)?;
    args.get(pos + 1).map(String::as_str)
}

/// `VULKAN_ADAPTER` и прочее из `environment.d` — в том порядке, в каком
/// их подгрузит сеанс (`/etc`, потом `~/.config`; последний побеждает).
fn environment_d_live() -> Vec<(String, String)> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in [PathBuf::from("/etc/environment.d"), Path::new(&home).join(".config/environment.d")] {
        let mut part: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "conf")).collect())
            .unwrap_or_default();
        part.sort();
        files.extend(part);
    }
    files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).flat_map(|t| parse_env_lines(&t)).collect()
}

fn nvidia_modeset_live() -> Option<bool> {
    std::fs::read_to_string("/sys/module/nvidia_drm/parameters/modeset").ok().map(|s| s.trim() == "Y")
}

/// `rexon --gamemode-preflight`: строки для человека — в stderr (журнал
/// юнита), одна машинная строка — в `out` (stdout, её читает сценарий).
pub fn preflight_route_live(dry_run: bool, out: &mut dyn FnMut(&str)) -> i32 {
    let cards = scan_drm(Path::new("/sys/class/drm"));
    let env_d = environment_d_live();
    let env_adapter = env_d.iter().rev().find(|(k, _)| k == "VULKAN_ADAPTER").map(|(_, v)| v.clone());
    let mut all_env: Vec<(String, String)> = std::env::vars().collect();
    all_env.extend(env_d.iter().cloned());
    let adapter = std::env::var("VULKAN_ADAPTER").ok();
    let input = PreflightInput {
        cards: &cards,
        adapter: adapter.as_deref(),
        environment_adapter: env_adapter.as_deref(),
        nvidia_modeset: nvidia_modeset_live(),
    };
    let decision = decide_preflight(&input);
    for l in preflight_lines(&input, &decision, &device_hiding_vars(&all_env)) {
        eprintln!("rexon: {l}");
    }
    if let Preflight::Fail(r) = &decision {
        if !dry_run {
            let _ = record_failure(&GamemodePaths::live(), &now_stamp(), r.code(), &fail_text(r));
        }
    }
    out(&decision.machine_line());
    0
}

fn journal_since_live(since: u64) -> String {
    std::process::Command::new("journalctl")
        .args(["--user", "--no-pager", "-o", "cat", "-u", &unit_name(), "--since", &format!("@{since}")])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// `None` — xprop нет или дисплея нет.
fn root_prop_live(name: &str) -> Option<Option<u64>> {
    let display = std::env::var("DISPLAY").ok().filter(|d| !d.is_empty())?;
    let o = std::process::Command::new("xprop").args(["-display", &display, "-root", name]).output().ok()?;
    Some(parse_xprop_cardinal(&String::from_utf8_lossy(&o.stdout)))
}

/// `rexon --gamemode-watch [--since EPOCH] [--log FILE] [--dry-run]`.
pub fn watch_route_live(args: &[String], out: &mut dyn FnMut(&str)) -> i32 {
    let dry_run = args.iter().any(|a| a == DRY_RUN_FLAG);
    let log_file = arg_value(args, LOG_FLAG).map(PathBuf::from);
    let start = now_epoch();
    let since = arg_value(args, SINCE_FLAG).and_then(|s| s.parse().ok()).unwrap_or(start.saturating_sub(30));
    let t = Timeouts::default();
    let paths = GamemodePaths::live();
    let read_log = || match &log_file {
        Some(f) => std::fs::read_to_string(f).unwrap_or_default(),
        None => journal_since_live(since),
    };
    let fail = |f: WatchFailure, out: &mut dyn FnMut(&str)| -> i32 {
        let had_ok = paths.last_ok().exists();
        let end = should_end_session(&f, had_ok);
        out(&format!("rexon: {} ({})", f.text(), f.code()));
        if dry_run {
            // TRANSLATORS: game mode watcher, check run without a real session.
            out(&tr("Check only: the session is left running."));
            return 1;
        }
        let _ = record_failure(&paths, &now_stamp(), f.code(), &f.text());
        if end {
            // TRANSLATORS: game mode watcher, the session is being ended.
            out(&tr("Ending the game mode session: you will get back to the login screen, choose your desktop there."));
            let _ = std::process::Command::new("systemctl").args(["--user", "stop", "--no-block", &unit_name()]).status();
        }
        1
    };
    // Стадия 1: разъём.
    let connector = loop {
        let elapsed = now_epoch().saturating_sub(start);
        match output_step(&classify_log(&read_log()), elapsed, &t) {
            WatchStep::Proceed(c) => break c,
            WatchStep::Fail(f) => return fail(f, out),
            _ => std::thread::sleep(std::time::Duration::from_secs(1)),
        }
    };
    // TRANSLATORS: game mode watcher. %1 = connector like "eDP-1".
    out(&trf("gamescope picked the screen %1.", &[&connector]));
    // Стадия 2: Steam в фокусе. Без xprop/дисплея (проверка по файлу) — дальше не смотрим.
    if log_file.is_some() || root_prop_live("GAMESCOPE_FOCUSED_APP").is_none() {
        return 0;
    }
    let mut seen_window = false;
    loop {
        let elapsed = now_epoch().saturating_sub(start);
        let app = root_prop_live("GAMESCOPE_FOCUSED_APP").flatten();
        let win = root_prop_live("GAMESCOPE_FOCUSED_WINDOW").flatten();
        match ready_step(elapsed, app, win, seen_window, &t) {
            WatchStep::Ready => {
                if !dry_run {
                    let _ = std::fs::create_dir_all(&paths.state_dir);
                    let _ = std::fs::write(paths.last_ok(), format!("{}\n", now_stamp()));
                }
                // TRANSLATORS: game mode watcher. %1 = seconds.
                out(&trf("Steam is on the screen after %1 s.", &[&elapsed]));
                return 0;
            }
            WatchStep::Fail(f) => return fail(f, out),
            _ => {}
        }
        seen_window |= win.is_some_and(|w| w != 0);
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// `rexon --gamemode-recover`: счётчик коротких сеансов сработал.
pub fn recover_route_live(out: &mut dyn FnMut(&str)) -> i32 {
    // TRANSLATORS: game mode, five short sessions in a row.
    let text = tr("Game mode stopped right after the start five times in a row.");
    let _ = record_failure(&GamemodePaths::live(), &now_stamp(), "short-sessions", &text);
    out(&format!("rexon: {text}"));
    0
}

/// `rexon --gamemode-guard-install|--gamemode-guard-remove`.
pub fn hook_route_live(install: bool, out: &mut dyn FnMut(&str)) -> i32 {
    let paths = GamemodePaths::live();
    let program = std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned());
    if install {
        let supported = std::fs::read_to_string(SESSION_SCRIPT).map(|t| session_script_supported(&t)).unwrap_or(false);
        if !supported {
            // TRANSLATORS: %1 = path of the gamescope-session-plus script.
            out(&trf("gamescope-session-plus is not installed or has changed (%1): the game mode guard can't be set up.", &[&SESSION_SCRIPT]));
            return 1;
        }
    }
    match sync_hook(&paths, install, program.as_deref()) {
        Ok(HookChange::ForeignFile) => {
            // TRANSLATORS: %1 = path of the user's own sessions.d/steam file.
            out(&trf("%1 is your own file: Rexon doesn't touch it.", &[&paths.hook().display()]));
            1
        }
        Ok(HookChange::Write(_)) | Ok(HookChange::Nothing) if install => {
            // TRANSLATORS: %1 = path of the file Rexon wrote.
            out(&trf("Game mode guard is set up: %1", &[&paths.hook().display()]));
            0
        }
        Ok(_) => {
            // TRANSLATORS: game mode guard file was removed (or there was none).
            out(&tr("Game mode guard is removed."));
            0
        }
        Err(e) => {
            out(&e);
            1
        }
    }
}

/// Маршрут без окна. `None` — не наш флаг.
pub fn cli_route(args: &[String], out: &mut dyn FnMut(&str)) -> Option<i32> {
    let has = |f: &str| args.iter().any(|a| a == f);
    if has(PREFLIGHT_FLAG) {
        return Some(preflight_route_live(has(DRY_RUN_FLAG), out));
    }
    if has(WATCH_FLAG) {
        return Some(watch_route_live(args, out));
    }
    if has(RECOVER_FLAG) {
        return Some(recover_route_live(out));
    }
    if has(INSTALL_FLAG) {
        return Some(hook_route_live(true, out));
    }
    if has(REMOVE_FLAG) {
        return Some(hook_route_live(false, out));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Self {
            let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let p = std::env::temp_dir().join(format!("rexon-gamemode-{tag}-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Раскладка как у владельца 14.11: eDP-1 на AMD Phoenix (06:00.0) =
    /// card1, NVIDIA (01:00.0) = card2 с отключённым HDMI. Каталоги устроены
    /// как в sysfs: cardN/device — ссылка на каталог PCI-устройства.
    fn owner_sysfs(t: &Tmp) -> PathBuf {
        let devices = t.0.join("devices/pci0000:00");
        let class = t.0.join("class/drm");
        std::fs::create_dir_all(&class).unwrap();
        for (card, pci, vendor, device, driver, boot, conns) in [
            ("card1", "0000:00:08.1/0000:06:00.0", "0x1002", "0x15bf", "amdgpu", "1", &[("eDP-1", "connected"), ("DP-1", "disconnected"), ("Writeback-1", "unknown")][..]),
            ("card2", "0000:00:01.1/0000:01:00.0", "0x10de", "0x2820", "nvidia", "0", &[("HDMI-A-1", "disconnected")][..]),
        ] {
            let dev = devices.join(pci);
            put(&dev.join("vendor"), &format!("{vendor}\n"));
            put(&dev.join("device"), &format!("{device}\n"));
            put(&dev.join("boot_vga"), &format!("{boot}\n"));
            let drv = t.0.join("bus/pci/drivers").join(driver);
            std::fs::create_dir_all(&drv).unwrap();
            std::os::unix::fs::symlink(&drv, dev.join("driver")).unwrap();
            std::fs::create_dir_all(class.join(card)).unwrap();
            std::os::unix::fs::symlink(&dev, class.join(card).join("device")).unwrap();
            for (c, st) in conns {
                put(&class.join(format!("{card}-{c}")).join("status"), &format!("{st}\n"));
            }
        }
        // мусор, который не карта
        std::fs::create_dir_all(class.join("renderD128")).unwrap();
        put(&class.join("version"), "drm 1.1.0\n");
        class
    }

    fn card(name: &str, vendor: u16, device: u16, conns: &[(&str, bool)]) -> DrmCard {
        DrmCard {
            card: name.into(),
            pci: None,
            vendor,
            device,
            driver: None,
            boot_vga: false,
            connectors: conns.iter().map(|(n, c)| Connector { name: n.to_string(), connected: *c }).collect(),
        }
    }

    fn hybrid() -> Vec<DrmCard> {
        vec![card("card1", 0x1002, 0x15bf, &[("eDP-1", true)]), card("card2", 0x10de, 0x2820, &[("HDMI-A-1", false)])]
    }

    fn input<'a>(cards: &'a [DrmCard], adapter: Option<&'a str>) -> PreflightInput<'a> {
        PreflightInput { cards, adapter, ..Default::default() }
    }

    #[test]
    fn scan_reads_owner_layout_by_pci_not_card_number() {
        let t = Tmp::new("scan");
        let cards = scan_drm(&owner_sysfs(&t));
        assert_eq!(cards.len(), 2);
        let amd = &cards[0];
        assert_eq!((amd.card.as_str(), amd.pci.as_deref(), amd.vk_id().as_str()), ("card1", Some("0000:06:00.0"), "1002:15bf"));
        assert_eq!(amd.driver.as_deref(), Some("amdgpu"));
        assert!(amd.boot_vga);
        assert_eq!(amd.connected_panel(), Some("eDP-1"));
        assert!(amd.connectors.iter().all(|c| !c.name.starts_with("Writeback")));
        let nv = &cards[1];
        assert_eq!((nv.pci.as_deref(), nv.vk_id().as_str()), (Some("0000:01:00.0"), "10de:2820"));
        assert!(!nv.any_connected());
        assert_eq!(screen_card(&cards).map(|c| c.card.as_str()), Some("card1"));
    }

    #[test]
    fn scan_of_missing_dir_is_empty() {
        assert!(scan_drm(Path::new("/nonexistent/rexon/drm")).is_empty());
    }

    #[test]
    fn hybrid_without_adapter_gets_panel_card() {
        let cards = hybrid();
        let d = decide_preflight(&input(&cards, None));
        assert_eq!(d, Preflight::Set { adapter: "1002:15bf".into(), card: "card1".into(), replaced: None });
        assert_eq!(d.machine_line(), "set VULKAN_ADAPTER=1002:15bf");
        // пустое значение = не задано
        assert_eq!(decide_preflight(&input(&cards, Some("  "))).machine_line(), "set VULKAN_ADAPTER=1002:15bf");
    }

    #[test]
    fn adapter_on_card_without_screen_or_typo_is_replaced() {
        let cards = hybrid();
        for bad in ["10de:2820", "dead:beef", "ffff:0001", "не-адрес"] {
            assert_eq!(
                decide_preflight(&input(&cards, Some(bad))),
                Preflight::Set { adapter: "1002:15bf".into(), card: "card1".into(), replaced: Some(bad.into()) },
                "{bad}"
            );
        }
    }

    #[test]
    fn adapter_already_right_or_users_card_with_screen_is_kept() {
        let cards = hybrid();
        assert_eq!(decide_preflight(&input(&cards, Some("0x1002:0x15BF"))), Preflight::Keep(KeepReason::AlreadyScreenCard));
        // MUX: внешний монитор на NVIDIA, человек сам выбрал её
        let mut cards = hybrid();
        cards[1].connectors[0].connected = true;
        assert_eq!(
            decide_preflight(&input(&cards, Some("10de:2820"))),
            Preflight::Keep(KeepReason::ChosenCardHasScreen("10de:2820".into()))
        );
    }

    #[test]
    fn single_card_and_no_cards_change_nothing() {
        let one = vec![card("card0", 0x1002, 0x1435, &[("eDP-1", true)])];
        assert_eq!(decide_preflight(&input(&one, None)), Preflight::Keep(KeepReason::SingleCard));
        assert_eq!(decide_preflight(&input(&[], None)), Preflight::Keep(KeepReason::NoCards));
        assert_eq!(decide_preflight(&input(&[], None)).machine_line(), "keep");
    }

    #[test]
    fn no_connected_screen_anywhere_fails() {
        let cards = vec![card("card0", 0x1002, 0x15bf, &[("eDP-1", false)]), card("card1", 0x10de, 0x2820, &[])];
        let d = decide_preflight(&input(&cards, None));
        assert_eq!(d, Preflight::Fail(FailReason::NoConnectedOutput));
        assert_eq!(d.machine_line(), "fail no-connected-output");
    }

    #[test]
    fn desktop_without_panel_prefers_boot_vga_with_screen() {
        let mut a = card("card0", 0x10de, 0x2684, &[("DP-1", true)]);
        let mut b = card("card1", 0x1002, 0x744c, &[("HDMI-A-1", true)]);
        b.boot_vga = true;
        a.boot_vga = false;
        let cards = vec![a, b];
        assert_eq!(decide_preflight(&input(&cards, None)).machine_line(), "set VULKAN_ADAPTER=1002:744c");
    }

    #[test]
    fn two_identical_cards_cannot_be_told_apart() {
        let cards = vec![card("card0", 0x1002, 0x73bf, &[("eDP-1", true)]), card("card1", 0x1002, 0x73bf, &[])];
        assert_eq!(decide_preflight(&input(&cards, None)), Preflight::Keep(KeepReason::SameIdTwice("1002:73bf".into())));
    }

    #[test]
    fn panel_on_nvidia_needs_modeset() {
        // MUX «только дискретная»: панель на NVIDIA
        let cards = vec![card("card0", 0x10de, 0x2820, &[("eDP-1", true)]), card("card1", 0x1002, 0x15bf, &[])];
        let mut i = input(&cards, None);
        i.nvidia_modeset = Some(false);
        assert_eq!(decide_preflight(&i), Preflight::Fail(FailReason::NvidiaModesetOff));
        i.nvidia_modeset = Some(true);
        assert_eq!(decide_preflight(&i).machine_line(), "set VULKAN_ADAPTER=10de:2820");
    }

    #[test]
    fn environment_d_adapter_wins_so_only_judge_it() {
        let cards = hybrid();
        let mut i = input(&cards, None);
        i.environment_adapter = Some("10de:2820");
        assert_eq!(
            decide_preflight(&i),
            Preflight::Fail(FailReason::EnvironmentAdapterWithoutScreen("10de:2820".into()))
        );
        i.environment_adapter = Some("1002:15bf");
        assert_eq!(decide_preflight(&i), Preflight::Keep(KeepReason::ChosenCardHasScreen("1002:15bf".into())));
    }

    #[test]
    fn env_lines_and_hiding_vars() {
        let env = parse_env_lines(
            "# comment\nDRI_PRIME=1\nexport __VK_LAYER_NV_optimus=\"NVIDIA_only\"\nVULKAN_ADAPTER='10de:2820'\nnot a line\nBAD-KEY=1\n",
        );
        assert_eq!(
            env,
            vec![
                ("DRI_PRIME".to_string(), "1".to_string()),
                ("__VK_LAYER_NV_optimus".to_string(), "NVIDIA_only".to_string()),
                ("VULKAN_ADAPTER".to_string(), "10de:2820".to_string()),
            ]
        );
        assert_eq!(device_hiding_vars(&env), vec!["__VK_LAYER_NV_optimus=NVIDIA_only".to_string()]);
        assert!(device_hiding_vars(&parse_env_lines("DRI_PRIME=1\n__NV_PRIME_RENDER_OFFLOAD=1\n")).is_empty());
    }

    #[test]
    fn parse_adapter_like_gamescope() {
        assert_eq!(parse_adapter("1002:15bf"), Some((0x1002, 0x15bf)));
        assert_eq!(parse_adapter("0x10DE:0x2820"), Some((0x10de, 0x2820)));
        assert_eq!(parse_adapter("\"1002:15bf\""), Some((0x1002, 0x15bf)));
        assert_eq!(parse_adapter("1002"), None);
        assert_eq!(parse_adapter("zz:15bf"), None);
    }

    #[test]
    fn preflight_lines_name_every_card_and_result() {
        let cards = hybrid();
        let i = input(&cards, None);
        let lines = preflight_lines(&i, &decide_preflight(&i), &["VK_ICD_FILENAMES=x".into()]);
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("card1") && lines[0].contains("eDP-1"));
        assert!(lines[1].contains("card2") && lines[1].ends_with('-'));
        assert!(lines[2].contains("1002:15bf"));
        assert!(lines[3].contains("VK_ICD_FILENAMES=x"));
    }

    const LOG_OK: &str = "\
+ gamescope --prefer-output '*,eDP-1' --steam -R /run/user/1000/gamescope.X/startup.socket\n\
[gamescope] [\u{1b}[0;34mInfo\u{1b}[0m]  vulkan: selecting physical device 'AMD Radeon 780M Graphics (RADV PHOENIX)': queue family 1\n\
[gamescope] [Info]  drm: opening DRM node '/dev/dri/card1'\n\
[gamescope] [Info]  drm: Connectors:\n\
[gamescope] [Info]  drm:   eDP-1 (connected)\n\
[gamescope] [Info]  drm: selecting connector eDP-1\n";

    const LOG_BLACK: &str = "\
[gamescope] [Info]  vulkan: selecting physical device 'NVIDIA GeForce RTX 4070 Laptop GPU': queue family 0\n\
[gamescope] [Info]  drm: opening DRM node '/dev/dri/card2'\n\
[gamescope] [Info]  drm:   HDMI-A-1 (disconnected)\n\
[gamescope] [\u{1b}[0;31mError\u{1b}[0m] drm: cannot find any connected connector!\n\
[gamescope] [Info]  drm: Starting in headless mode\n";

    #[test]
    fn log_signals() {
        assert_eq!(classify_log(LOG_OK), OutputSignal::Selected("eDP-1".into()));
        assert_eq!(classify_log(LOG_BLACK), OutputSignal::NoConnector { node: Some("/dev/dri/card2".into()) });
        assert_eq!(classify_log("+ gamescope ...\n"), OutputSignal::Pending);
        assert_eq!(classify_log(""), OutputSignal::Pending);
        let modeset = format!("{LOG_OK}[gamescope] [Error] drm: We failed our modeset and have no mode to fall back to!\n");
        assert!(matches!(classify_log(&modeset), OutputSignal::ModesetFailed(l) if l.contains("We failed our modeset")));
    }

    #[test]
    fn output_stage_decisions() {
        let t = Timeouts::default();
        assert_eq!(output_step(&classify_log(LOG_OK), 3, &t), WatchStep::Proceed("eDP-1".into()));
        let black = output_step(&classify_log(LOG_BLACK), 1, &t);
        assert!(matches!(&black, WatchStep::Fail(f) if f.code() == "no-connected-output" && f.is_certain()));
        assert_eq!(output_step(&OutputSignal::Pending, 5, &t), WatchStep::Wait);
        assert_eq!(output_step(&OutputSignal::Pending, 21, &t), WatchStep::Fail(WatchFailure::NoOutputDecision));
    }

    #[test]
    fn ready_stage_decisions() {
        let t = Timeouts::default();
        assert_eq!(ready_step(10, Some(STEAM_APPID), Some(0x400001), false, &t), WatchStep::Ready);
        assert_eq!(ready_step(10, None, None, false, &t), WatchStep::Wait);
        assert_eq!(ready_step(181, None, Some(0), false, &t), WatchStep::Fail(WatchFailure::SteamNotShown));
        // окно было (обновление Steam) — ждём до потолка
        assert_eq!(ready_step(300, Some(0), Some(0x400001), false, &t), WatchStep::Wait);
        assert_eq!(ready_step(300, Some(0), Some(0), true, &t), WatchStep::Wait);
        assert_eq!(ready_step(601, Some(0), Some(0x400001), true, &t), WatchStep::Fail(WatchFailure::SteamNotReady));
    }

    #[test]
    fn slow_steam_ends_session_only_before_first_success() {
        assert!(should_end_session(&WatchFailure::NoConnectedOutput { node: None }, true));
        assert!(should_end_session(&WatchFailure::ModesetFailed(String::new()), true));
        assert!(should_end_session(&WatchFailure::SteamNotShown, false));
        assert!(!should_end_session(&WatchFailure::SteamNotShown, true));
        assert!(!should_end_session(&WatchFailure::SteamNotReady, true));
    }

    #[test]
    fn xprop_values() {
        assert_eq!(parse_xprop_cardinal("GAMESCOPE_FOCUSED_APP(CARDINAL) = 769\n"), Some(769));
        assert_eq!(parse_xprop_cardinal("GAMESCOPE_FOCUSED_WINDOW(CARDINAL) = 4194305, 0\n"), Some(4194305));
        assert_eq!(parse_xprop_cardinal("GAMESCOPE_FOCUSED_APP:  not found.\n"), None);
        assert_eq!(parse_xprop_cardinal(""), None);
    }

    #[test]
    fn hook_text_keeps_system_hooks_and_cleans_up() {
        let s = hook_script("/usr/bin/rexon-main-window", "/home/u/.config/gamescope-session-plus/sessions.d/steam");
        assert!(is_our_hook(&s));
        assert_eq!(s.matches(PREFLIGHT_FLAG).count(), 1, "{s}");
        assert!(s.contains("eval \"_rexon_system_$(declare -f post_gamescope_start)\""));
        assert!(s.contains("eval \"_rexon_system_$(declare -f short_session_recover)\""));
        assert!(s.contains("'set VULKAN_ADAPTER='*) VULKAN_ADAPTER=\"${_rexon_pf#set VULKAN_ADAPTER=}\""));
        assert!(s.contains("rm -f '/home/u/.config/gamescope-session-plus/sessions.d/steam'"));
        // CLIENT в сеансе readonly — его не трогаем
        assert!(!s.contains("CLIENT="));
        assert!(!s.contains("set -e"));
    }

    /// Сценарий в настоящем bash, как его подключает сеанс: под `set -a`,
    /// после системного файла и с `declare -r CLIENT`. Подставной «rexon»
    /// отвечает строкой, крючки зовутся — системное тело выполняется первым.
    #[test]
    fn hook_runs_in_bash_like_the_session() {
        if std::process::Command::new("bash").arg("--version").output().is_err() {
            return;
        }
        let t = Tmp::new("bash");
        let fake = t.0.join("rexon");
        let calls = t.0.join("calls");
        put(
            &fake,
            &format!(
                "#!/bin/bash\necho \"$*\" >> {calls}\ncase \"$1\" in {PREFLIGHT_FLAG}) echo 'set VULKAN_ADAPTER=1002:15bf' ;; esac\n",
                calls = sh_quote(&calls.to_string_lossy())
            ),
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let hook = t.0.join("steam");
        put(&hook, &hook_script(&fake.to_string_lossy(), &hook.to_string_lossy()));
        let driver = format!(
            "declare -r CLIENT=steam\n\
             post_gamescope_start() {{ :; }}\nshort_session_recover() {{ :; }}\n\
             function post_gamescope_start {{ echo system-post >> {calls}; }}\n\
             function short_session_recover {{ echo system-recover >> {calls}; }}\n\
             SECONDS=0\nset -a\n. {hook}\nset +a\n\
             echo \"adapter=$VULKAN_ADAPTER\"\n\
             post_gamescope_start\nwait\nshort_session_recover\n",
            calls = sh_quote(&calls.to_string_lossy()),
            hook = sh_quote(&hook.to_string_lossy())
        );
        let o = std::process::Command::new("bash").arg("-c").arg(&driver).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "adapter=1002:15bf");
        let log = std::fs::read_to_string(&calls).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines[0], PREFLIGHT_FLAG);
        assert_eq!(lines[1], "system-post");
        assert!(lines[2].starts_with(&format!("{WATCH_FLAG} {SINCE_FLAG} ")), "{log}");
        assert_eq!(&lines[3..], ["system-recover", RECOVER_FLAG]);
    }

    /// Ответ `fail …` обрывает сеанс до gamescope; нет бинаря — файл убирается сам.
    #[test]
    fn hook_fail_exits_and_missing_binary_removes_hook() {
        if std::process::Command::new("bash").arg("--version").output().is_err() {
            return;
        }
        let t = Tmp::new("bashfail");
        let fake = t.0.join("rexon");
        put(&fake, &format!("#!/bin/bash\necho '{FAIL_PREFIX}no-connected-output'\n"));
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let hook = t.0.join("steam");
        put(&hook, &hook_script(&fake.to_string_lossy(), &hook.to_string_lossy()));
        let run = |h: &Path| {
            std::process::Command::new("bash")
                .arg("-c")
                .arg(format!("set -a; . {}; set +a; echo reached-gamescope", sh_quote(&h.to_string_lossy())))
                .output()
                .unwrap()
        };
        let o = run(&hook);
        assert_eq!(o.status.code(), Some(1));
        assert!(!String::from_utf8_lossy(&o.stdout).contains("reached-gamescope"));
        assert!(String::from_utf8_lossy(&o.stderr).contains("no-connected-output"));

        let gone = t.0.join("steam-gone");
        put(&gone, &hook_script(&t.0.join("нет-такого").to_string_lossy(), &gone.to_string_lossy()));
        let o = run(&gone);
        assert!(String::from_utf8_lossy(&o.stdout).contains("reached-gamescope"));
        assert!(!gone.exists());
    }

    #[test]
    fn hook_plan_never_touches_foreign_file() {
        let p = "/h/.config/gamescope-session-plus/sessions.d/steam";
        assert_eq!(hook_change(true, Some("/usr/bin/rexon"), p, Some("export VULKAN_ADAPTER=1002:15bf\n")), HookChange::ForeignFile);
        assert_eq!(hook_change(false, None, p, Some("# my own\n")), HookChange::ForeignFile);
        assert!(matches!(hook_change(true, Some("/usr/bin/rexon"), p, None), HookChange::Write(_)));
        let ours = hook_script("/usr/bin/rexon", p);
        assert_eq!(hook_change(true, Some("/usr/bin/rexon"), p, Some(&ours)), HookChange::Nothing);
        assert_eq!(hook_change(false, Some("/usr/bin/rexon"), p, Some(&ours)), HookChange::Remove);
        assert_eq!(hook_change(true, Some("rexon"), p, Some(&ours)), HookChange::Remove);
        assert_eq!(hook_change(false, None, p, None), HookChange::Nothing);
    }

    #[test]
    fn sync_hook_writes_and_removes() {
        let t = Tmp::new("sync");
        let paths = GamemodePaths::new(t.0.join("config"), t.0.join("state/rexon/gamemode"));
        assert!(matches!(sync_hook(&paths, true, Some("/usr/bin/rexon")).unwrap(), HookChange::Write(_)));
        assert!(is_our_hook(&std::fs::read_to_string(paths.hook()).unwrap()));
        assert_eq!(sync_hook(&paths, true, Some("/usr/bin/rexon")).unwrap(), HookChange::Nothing);
        assert_eq!(sync_hook(&paths, false, None).unwrap(), HookChange::Remove);
        assert!(!paths.hook().exists());
        put(&paths.hook(), "# мой файл\n");
        assert_eq!(sync_hook(&paths, true, Some("/usr/bin/rexon")).unwrap(), HookChange::ForeignFile);
        assert_eq!(std::fs::read_to_string(paths.hook()).unwrap(), "# мой файл\n");
    }

    #[test]
    fn failures_are_appended_one_per_line() {
        let t = Tmp::new("fail");
        let paths = GamemodePaths::new(t.0.join("config"), t.0.join("state/rexon/gamemode"));
        record_failure(&paths, "2026-11-14T03:00:00+01:00", "no-connected-output", "a\nb").unwrap();
        record_failure(&paths, "2026-11-14T03:01:00+01:00", "steam-not-shown", "c").unwrap();
        let text = std::fs::read_to_string(paths.failures()).unwrap();
        assert_eq!(text, "2026-11-14T03:00:00+01:00 no-connected-output a b\n2026-11-14T03:01:00+01:00 steam-not-shown c\n");
    }

    /// Живая машина, только чтение /sys: `cargo test-core gamemode_guard::tests::live_ -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_preflight_on_this_machine() {
        let cards = scan_drm(Path::new("/sys/class/drm"));
        let i = PreflightInput { cards: &cards, nvidia_modeset: nvidia_modeset_live(), ..Default::default() };
        let d = decide_preflight(&i);
        for l in preflight_lines(&i, &d, &[]) {
            println!("{l}");
        }
        println!("{}", d.machine_line());
    }

    #[test]
    fn session_script_support_check() {
        let installed = "post_gamescope_start() {\n:\n}\nshort_session_recover\nCLIENT_CONFIG_DIR_LOCAL=x\nGAMESCOPECMD+=\" --prefer-vk-device $VULKAN_ADAPTER\"\n";
        assert!(session_script_supported(installed));
        assert!(!session_script_supported("post_gamescope_start\n"));
    }
}
