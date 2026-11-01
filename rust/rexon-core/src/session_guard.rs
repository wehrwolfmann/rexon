//! Сторож перед сеансом Plasma для AeroThemePlasma: «вход без Aero» на один
//! вход, когда оформление разошлось с обновлённой Plasma.
//!
//! ЗАЧЕМ. После `pacman -Syu` форки AeroThemePlasma остаются собранными под
//! прежнюю Plasma. Худшие случаи: эффект KWin с разошедшимся ABI роняет `kwin_wayland` при загрузке —
//! обёртка перезапускает его до 10 раз и выкидывает к экрану входа; виджет
//! панели роняет plasmashell — systemd сдаётся после трёх падений, и человек
//! видит чёрный стол с курсором. Безопасного режима ни у KWin, ни у
//! plasmashell нет.
//!
//! КАК. startplasma выполняет `~/.config/plasma-workspace/env/*.sh` ДО запуска
//! KWin и plasmashell (`startplasma.cpp`, `runEnvironmentScripts()`). Туда
//! Rexon кладёт тонкий сценарий ([`env_script`]): он зовёт
//! `rexon --session-guard` с потолком в 3 секунды и ничего больше; нет бинаря
//! или он упал — вход идёт как обычно (вход важнее сторожа). Сторож:
//!   1. быстро сверяет набор с текущими библиотеками — ТЕ ЖЕ проверки, что у
//!      окна Rexon ([`crate::aeroshell::check_modules_with`]: строка версии в
//!      эффектах против KWin, версия форка libplasma против plasma-workspace,
//!      `ldd -r` модулей набора), плюс `ldd -r /usr/bin/plasmashell`. Данные —
//!      из базы pacman файлами (`/var/lib/pacman/local`), без единого запуска
//!      pacman; `ldd` — параллельно. Итог — [`decide`];
//!   2. разошлось по-настоящему — [`fall_back_to_stock`]: эффекты Aero в
//!      `kwinrc [Plugins] <id>Enabled=false`, а если ломается сторона
//!      plasmashell — штатная раскладка панелей вместо раскладки с виджетами
//!      Aero. Всё прежнее — в копии в `$XDG_STATE_HOME/rexon/session-guard/`;
//!   3. после входа — уведомление «Оформление временно выключено… [Пересобрать]
//!      [Подробнее]» ([`notice_route_live`], отдельный процесс с задержкой: до
//!      входа показывать уведомления некому);
//!   4. после удачной пересборки ([`after_rebuild_live`]) эффекты возвращаются
//!      сразу, раскладка — при следующем входе (plasmashell, пока жив, держит
//!      раскладку в памяти и при выходе записал бы её поверх возвращённой).
//!      Сверка снова сходится при входе — сторож возвращает всё сам.
//!
//! ЗАЩИТА ОТ ДУРАКА.
//!   * Копия — до правки; правка — атомарной заменой файла. Повторный вход при
//!     всё ещё сломанном наборе НЕ перетирает копию: прежние значения ключей
//!     уже записаны, а в файле сейчас наши `false` — сохранять их значило бы
//!     потерять настройки человека. Добавляются только новые ключи.
//!   * Возврат идемпотентный и бережный: ключ возвращается, только если в нём
//!     всё ещё наше `false` (человек сам включил эффект — его выбор остаётся);
//!     раскладка, которую человек успел поменять во «входе без Aero», перед
//!     возвратом откладывается копией `appletsrc.during-fallback`.
//!   * AeroThemePlasma не стоит (нет пакета оболочки) — сторож молчит.
//!     Вне Plasma не трогает ничего; трогает только два файла: `kwinrc` и
//!     `plasma-org.kde.plasma.desktop-appletsrc`.
//!   * Ложная тревога → только уведомление. Подмена — лишь по однозначным
//!     уликам: строка версии эффекта не та (по ней же решает загрузчик KWin),
//!     `ldd` не находит библиотеку или символ, форк libplasma не той версии.
//!     Один лишь «приватный интерфейс прежнего Qt» ([`crate::aeroshell::Report::at_risk_only`])
//!     или беда в самом plasmashell без форка libplasma (это не Aero, менять
//!     ему раскладку бесполезно) — только уведомление.
//!   * Выключено «проверять оформление при запуске» в настройках — сторож
//!     тоже молчит, а сценарий снимается.
//!
//! ОБЩЕЕ ДЛЯ СТОРОЖА СЕАНСА. «Откатить оформление на штатное и запомнить, как
//! было» — [`fall_back_to_stock`] и обратная ей [`restore_saved`]. Их же
//! позовёт сторож сеанса (ступень 3: падения KWin/plasmashell сразу после
//! входа). Ему посреди сеанса править раскладку нельзя (plasmashell жив),
//! поэтому для него есть [`request_fallback`]: отметка «со следующего входа —
//! штатное оформление», её применяет этот сторож до KWin. Возврат после
//! такой отметки сам по сходящейся сверке НЕ делается (сверка сходилась и в
//! тот раз, когда всё упало) — только после пересборки или по
//! `rexon --session-guard-restore`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::aeroshell::{self, Marker, Report};
use crate::i18n::{tr, trf};
use crate::shell_palette::{ini_apply, ini_get, IniEdit};

// ============================================================================
// Имена и пути
// ============================================================================

/// Флаг сторожа (его зовёт сценарий в `plasma-workspace/env`).
pub const GUARD_FLAG: &str = "--session-guard";
/// Добавка к [`GUARD_FLAG`]: только сверить и напечатать решение, ничего не
/// менять (проверка в ВМ и замер времени).
pub const DRY_RUN_FLAG: &str = "--dry-run";
/// Флаг отложенного уведомления после входа.
pub const NOTICE_FLAG: &str = "--session-guard-notice";
/// Флаг ручного возврата сохранённого оформления.
pub const RESTORE_FLAG: &str = "--session-guard-restore";

/// Имя сценария в `~/.config/plasma-workspace/env/`.
pub const ENV_SCRIPT_NAME: &str = "rexon-session-guard.sh";
/// Раскладка панелей и рабочего стола plasmashell.
pub const APPLETSRC_NAME: &str = "plasma-org.kde.plasma.desktop-appletsrc";
/// Группа включения модулей KWin.
pub const KWIN_PLUGINS_GROUP: &str = "Plugins";
/// Бинарь plasmashell — его `ldd -r` ловит разошедшийся форк libplasma.
pub const PLASMASHELL_BIN: &str = "/usr/bin/plasmashell";
/// База установленных пакетов pacman.
pub const PACMAN_LOCAL: &str = "/var/lib/pacman/local";
/// Потолок времени сторожа в сценарии, секунды (вход не тормозить).
pub const ENV_TIMEOUT_SECS: u64 = 3;
/// Через сколько после входа показывать уведомление, секунды: до этого
/// уведомления показывать некому (служба уведомлений — сам plasmashell).
pub const NOTICE_DELAY_SECS: u64 = 15;
/// Сколько ещё пытаться достучаться до службы уведомлений, секунды.
pub const NOTICE_RETRY_SECS: u64 = 120;
/// Потолок одного `ldd -r`, секунды.
const LDD_TIMEOUT_SECS: u64 = 2;

/// Файлы сторожа: настройки Plasma и каталог копий.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardPaths {
    /// `$XDG_CONFIG_HOME` (обычно `~/.config`).
    pub config_home: PathBuf,
    /// `$XDG_STATE_HOME/rexon/session-guard`.
    pub state_dir: PathBuf,
}

impl GuardPaths {
    pub fn new(config_home: impl Into<PathBuf>, state_dir: impl Into<PathBuf>) -> Self {
        Self { config_home: config_home.into(), state_dir: state_dir.into() }
    }

    /// Пути живого пользователя.
    pub fn live() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let state = crate::utils::state_home(std::env::var("XDG_STATE_HOME").ok().as_deref(), &home);
        Self::new(crate::utils::config_home_env(), Path::new(&state).join("rexon/session-guard"))
    }

    pub fn kwinrc(&self) -> PathBuf {
        self.config_home.join("kwinrc")
    }
    pub fn appletsrc(&self) -> PathBuf {
        self.config_home.join(APPLETSRC_NAME)
    }
    pub fn env_script(&self) -> PathBuf {
        self.config_home.join("plasma-workspace/env").join(ENV_SCRIPT_NAME)
    }
    /// Что сохранено и почему.
    pub fn saved_json(&self) -> PathBuf {
        self.state_dir.join("saved.json")
    }
    /// Копия раскладки человека до подмены.
    pub fn saved_appletsrc(&self) -> PathBuf {
        self.state_dir.join("appletsrc")
    }
    /// Полная копия `kwinrc` до первой правки (на крайний случай, руками).
    pub fn saved_kwinrc(&self) -> PathBuf {
        self.state_dir.join("kwinrc")
    }
    /// Раскладка, которую человек менял во «входе без Aero», — отложена
    /// перед возвратом его настоящей.
    pub fn during_fallback_appletsrc(&self) -> PathBuf {
        self.state_dir.join("appletsrc.during-fallback")
    }
    /// Уведомление, ждущее показа после входа.
    pub fn notice_json(&self) -> PathBuf {
        self.state_dir.join("notice.json")
    }
    /// Отметка сторожа сеанса: со следующего входа — штатное оформление.
    pub fn request_file(&self) -> PathBuf {
        self.state_dir.join("request")
    }
    /// О каком «только риске» уже сказали (отпечаток).
    pub fn risk_stamp(&self) -> PathBuf {
        self.state_dir.join("risk-notified")
    }
}

// ============================================================================
// База pacman файлами: без запуска pacman
// ============================================================================
//
// `pacman -Qq`, `-Q`, `-Ql` — это 3–15 запусков по 10–40 мс. До KWin каждые
// десятки миллисекунд — задержка входа. Та же база лежит текстом:
// `/var/lib/pacman/local/<имя>-<версия>-<выпуск>/{desc,files}`.

/// Имя и версия пакета из имени каталога базы (`kwin-6.8.1-2.1` →
/// `("kwin", "6.8.1-2.1")`). Имя может содержать `-`, версия и выпуск — нет.
pub fn local_db_entry(dir_name: &str) -> Option<(String, String)> {
    let mut parts = dir_name.rsplitn(3, '-');
    let rel = parts.next()?;
    let ver = parts.next()?;
    let name = parts.next()?;
    (!name.is_empty() && !ver.is_empty() && !rel.is_empty()).then(|| (name.to_string(), format!("{ver}-{rel}")))
}

/// Файл `files` базы → вывод как у `pacman -Ql <pkg>` (`<pkg> /путь`), чтобы
/// дальше шёл общий разбор ([`crate::aeroshell::package_files`]).
pub fn local_files_as_ql(package: &str, files_text: &str) -> String {
    let mut out = String::new();
    let mut in_files = false;
    for line in files_text.lines() {
        if line.starts_with('%') {
            in_files = line == "%FILES%";
            continue;
        }
        if in_files && !line.is_empty() {
            out.push_str(package);
            out.push_str(" /");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Установленные пакеты: имя → (версия, каталог базы).
#[derive(Debug, Clone, Default)]
pub struct LocalDb {
    pub entries: Vec<(String, String, PathBuf)>,
}

impl LocalDb {
    /// Прочитать список каталогов базы. Нет базы (не Arch) — пусто.
    pub fn read(root: &Path) -> Self {
        let mut entries = Vec::new();
        if let Ok(rd) = std::fs::read_dir(root) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if let Some((pkg, ver)) = local_db_entry(&name) {
                    entries.push((pkg, ver, e.path()));
                }
            }
        }
        entries.sort();
        Self { entries }
    }
    pub fn names(&self) -> Vec<String> {
        self.entries.iter().map(|(n, _, _)| n.clone()).collect()
    }
    pub fn version(&self, package: &str) -> Option<String> {
        self.entries.iter().find(|(n, _, _)| n == package).map(|(_, v, _)| v.clone())
    }
    /// Вывод «как `pacman -Ql`» для пакета либо None.
    pub fn file_list(&self, package: &str) -> Option<String> {
        let (_, _, dir) = self.entries.iter().find(|(n, _, _)| n == package)?;
        let text = std::fs::read_to_string(dir.join("files")).ok()?;
        Some(local_files_as_ql(package, &text))
    }
}

// ============================================================================
// Из чего состоит набор: эффекты KWin и виджеты Plasma
// ============================================================================

/// Модуль эффекта KWin из набора.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectModule {
    /// Имя файла (`smodglow.so`) — так он назван в бедах сверки.
    pub file_name: String,
    /// Опознаватель модуля для `kwinrc [Plugins] <id>Enabled`.
    pub id: String,
}

/// Что из набора включается в настройках: эффекты и виджеты.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AeroParts {
    pub effects: Vec<EffectModule>,
    /// Опознаватели виджетов и контейнеров Plasma из набора.
    pub applets: Vec<String>,
}

/// Опознаватель модуля KWin: `KPlugin.Id` из вшитых метаданных, иначе имя
/// файла без `.so` (так же решает и `KPluginMetaData`).
///
/// Qt 6 вшивает метаданные в CBOR: ключ `KPlugin` (`0x67`+7 букв), внутри —
/// `Id` (`0x62 'I' 'd'`) и текстовая строка. Старый вид (JSON текстом) тоже
/// понимается.
pub fn effect_plugin_id(data: &[u8], file_name: &str) -> String {
    let stem = file_name.strip_suffix(".so").unwrap_or(file_name).to_string();
    let find = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).position(|w| w == needle);
    let valid = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    if let Some(k) = find(data, b"\x67KPlugin") {
        let rest = &data[k..];
        if let Some(i) = find(rest, b"\x62Id") {
            let v = &rest[i + 3..];
            let (len, from) = match v.first() {
                Some(&b) if (0x60..0x78).contains(&b) => ((b - 0x60) as usize, 1),
                Some(&0x78) => (v.get(1).copied().unwrap_or(0) as usize, 2),
                _ => (0, 0),
            };
            if len > 0 && v.len() >= from + len {
                if let Ok(s) = std::str::from_utf8(&v[from..from + len]) {
                    if valid(s) {
                        return s.to_string();
                    }
                }
            }
        }
    }
    if let Some(k) = find(data, b"\"KPlugin\"") {
        let rest = &data[k..];
        if let Some(i) = find(rest, b"\"Id\"") {
            let tail = String::from_utf8_lossy(&rest[i + 4..rest.len().min(i + 4 + 256)]).to_string();
            if let Some(v) = tail.trim_start().strip_prefix(':').map(str::trim_start).and_then(|t| t.strip_prefix('"')) {
                if let Some((id, _)) = v.split_once('"') {
                    if valid(id) {
                        return id.to_string();
                    }
                }
            }
        }
    }
    stem
}

/// Опознаватель виджета по пути файла пакета: каталог
/// `/usr/share/plasma/plasmoids/<id>/…` или модуль `…/plasma/applets/<id>.so`.
pub fn applet_id_of(path: &str) -> Option<String> {
    if let Some((_, rest)) = path.split_once("/share/plasma/plasmoids/") {
        let id = rest.split('/').next().unwrap_or("");
        return (!id.is_empty()).then(|| id.to_string());
    }
    if let Some((_, rest)) = path.split_once("/plasma/applets/") {
        let id = rest.strip_suffix(".so").filter(|r| !r.contains('/'))?;
        return (!id.is_empty()).then(|| id.to_string());
    }
    None
}

/// Состав набора по файлам его пакетов.
pub fn aero_parts(files: &[(String, Vec<String>)], read_file: &dyn Fn(&str) -> Option<Vec<u8>>) -> AeroParts {
    let mut parts = AeroParts::default();
    for (_, list) in files {
        for path in aeroshell::effect_plugin_paths(list) {
            let file_name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let id = read_file(&path)
                .map(|d| effect_plugin_id(&d, &file_name))
                .unwrap_or_else(|| file_name.strip_suffix(".so").unwrap_or(&file_name).to_string());
            if !parts.effects.iter().any(|e| e.id == id) {
                parts.effects.push(EffectModule { file_name, id });
            }
        }
        for path in list {
            if let Some(id) = applet_id_of(path) {
                if !parts.applets.contains(&id) {
                    parts.applets.push(id);
                }
            }
        }
    }
    parts
}

// ============================================================================
// Решение «разошлось или нет»
// ============================================================================

/// Итог сверки для сторожа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Оболочки AeroThemePlasma нет — сторожу здесь делать нечего.
    NotInstalled,
    /// Всё сходится.
    Healthy,
    /// Есть только риск либо беда не в наборе — уведомить, ничего не менять.
    NoticeOnly { why: Vec<String> },
    /// Разошлось по-настоящему: что выключить на этот вход.
    Broken { effects: bool, shell: bool, why: Vec<String> },
}

/// Имя модуля из строки беды сверки (`smodglow.so: undefined symbol: …`).
fn problem_module(line: &str) -> &str {
    line.split(": ").next().unwrap_or(line)
}

/// Решить по сверке набора ([`crate::aeroshell::check_modules_with`]) и
/// `ldd -r` plasmashell ([`crate::aeroshell::ldd_problems`]).
pub fn decide(report: &Report, plasmashell_problems: &[String], parts: &AeroParts) -> Verdict {
    if !report.packages.iter().any(|p| p.package.starts_with(aeroshell::SHELL_PACKAGE_PREFIX)) {
        return Verdict::NotInstalled;
    }
    let is_effect = |module: &str| parts.effects.iter().any(|e| e.file_name == module);
    // «Приватный интерфейс прежнего Qt» — риск, а не поломка: модуль пока
    // грузится. Подменять по нему настройки — ложная тревога.
    let certain = |line: &str| !line.contains(aeroshell::QT_PRIVATE_PROBLEM);
    let differs = |p: &aeroshell::PackageVerdict| matches!((&p.built_for, &p.installed), (Some(b), Some(i)) if b != i);

    let mut why = Vec::new();
    let mut effects = false;
    let mut shell = false;
    for p in &report.packages {
        if p.marker == Marker::KwinEffect && differs(p) {
            effects = true;
            why.push(format!(
                "{}: KWin {} ≠ {}",
                p.package,
                p.built_for.as_deref().unwrap_or("?"),
                p.installed.as_deref().unwrap_or("?")
            ));
        }
        if p.marker == Marker::PlasmaLibrary && differs(p) {
            shell = true;
            why.push(format!(
                "{}: libPlasma {} ≠ Plasma {}",
                p.package,
                p.built_for.as_deref().unwrap_or("?"),
                p.installed.as_deref().unwrap_or("?")
            ));
        }
        for line in p.problems.iter().filter(|l| certain(l)) {
            if is_effect(problem_module(line)) {
                effects = true;
            } else {
                shell = true;
            }
            why.push(format!("{}: {line}", p.package));
        }
    }
    let fork = report.packages.iter().any(|p| p.package.starts_with(aeroshell::LIBPLASMA_FORK_PREFIX));
    if !plasmashell_problems.is_empty() {
        for line in plasmashell_problems {
            why.push(format!("plasmashell: {line}"));
        }
        // plasmashell не грузится при стоящем форке libplasma — это форк.
        // Без форка беда не в наборе (смесь Qt и т. п.): раскладка Aero тут
        // ни при чём, и менять её — ложная тревога.
        shell |= fork;
    }
    if effects || shell {
        return Verdict::Broken { effects, shell, why };
    }
    if report.needs_rebuild || !plasmashell_problems.is_empty() {
        for p in report.packages.iter().filter(|p| p.stale) {
            for line in &p.problems {
                why.push(format!("{}: {line}", p.package));
            }
        }
        return Verdict::NoticeOnly { why };
    }
    Verdict::Healthy
}

// ============================================================================
// kwinrc: выключить эффекты с копией и вернуть
// ============================================================================

/// Прежнее значение одного ключа (`None` — ключа не было).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedKey {
    pub key: String,
    pub previous: Option<String>,
}

/// Что сторож выключает: `false` — значение, которое он пишет.
const OFF: &str = "false";

fn enabled_key(id: &str) -> String {
    format!("{id}Enabled")
}

/// Выключить эффекты в тексте `kwinrc`. Прежние значения дописываются в
/// `saved` — только для ключей, которых там ещё нет (повторный вход не
/// перетирает копию нашими же `false`). Уже выключенный ключ не трогается и
/// не запоминается. → новый текст либо None, если менять нечего.
pub fn disable_effects(kwinrc: Option<&str>, ids: &[String], saved: &mut Vec<SavedKey>) -> Option<String> {
    let text = kwinrc.unwrap_or_default();
    let mut edits = Vec::new();
    for id in ids {
        let key = enabled_key(id);
        let current = ini_get(text, KWIN_PLUGINS_GROUP, &key);
        if current.as_deref() == Some(OFF) {
            continue;
        }
        if !saved.iter().any(|s| s.key == key) {
            saved.push(SavedKey { key: key.clone(), previous: current });
        }
        edits.push(IniEdit { group: KWIN_PLUGINS_GROUP.into(), key, value: Some(OFF.into()) });
    }
    (!edits.is_empty()).then(|| ini_apply(kwinrc, &edits))
}

/// Вернуть сохранённые ключи. Ключ, в котором уже не наше `false` (человек
/// включил эффект сам), остаётся как есть и попадает во второй список.
/// → (новый текст либо None, если менять нечего; оставленные ключи).
pub fn restore_effects(kwinrc: Option<&str>, saved: &[SavedKey]) -> (Option<String>, Vec<String>) {
    let text = kwinrc.unwrap_or_default();
    let mut edits = Vec::new();
    let mut kept = Vec::new();
    for s in saved {
        let current = ini_get(text, KWIN_PLUGINS_GROUP, &s.key);
        if current.as_deref() != Some(OFF) {
            if current != s.previous {
                kept.push(s.key.clone());
            }
            continue;
        }
        if s.previous.as_deref() == Some(OFF) {
            continue;
        }
        edits.push(IniEdit { group: KWIN_PLUGINS_GROUP.into(), key: s.key.clone(), value: s.previous.clone() });
    }
    ((!edits.is_empty()).then(|| ini_apply(kwinrc, &edits)), kept)
}

// ============================================================================
// Раскладка plasmashell: штатная вместо раскладки с виджетами Aero
// ============================================================================
//
// Группы раскладки: `[Containments][N]` — контейнер (рабочий стол, панель,
// внутренний контейнер системного лотка), `[Containments][N][Applets][M]` —
// виджет в нём, глубже — их настройки. Штатная раскладка строится из
// раскладки человека: рабочие столы остаются (обои, комнаты, экраны), из
// них убираются только виджеты Aero; все панели уходят, вместо них — одна
// штатная панель внизу, как у Plasma по умолчанию. Сама раскладка человека
// целиком лежит в копии и возвращается файлом.

struct Group {
    name: Option<String>,
    lines: Vec<String>,
}

fn split_groups(text: &str) -> Vec<Group> {
    let mut out = vec![Group { name: None, lines: Vec::new() }];
    for line in text.lines() {
        let t = line.trim();
        if t.len() >= 2 && t.starts_with('[') && t.ends_with(']') {
            out.push(Group { name: Some(t[1..t.len() - 1].to_string()), lines: Vec::new() });
        } else {
            out.last_mut().expect("преамбула есть всегда").lines.push(line.to_string());
        }
    }
    out
}

fn group_value(g: &Group, key: &str) -> Option<String> {
    g.lines.iter().rev().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

fn segments(name: &str) -> Vec<&str> {
    name.split("][").collect()
}

/// Ссылается ли раскладка на виджеты или контейнеры набора.
pub fn layout_uses(appletsrc: &str, ids: &[String]) -> bool {
    split_groups(appletsrc).iter().any(|g| {
        let Some(name) = &g.name else { return false };
        let s = segments(name);
        let is_item = (s.len() == 2 && s[0] == "Containments") || (s.len() == 4 && s[0] == "Containments" && s[2] == "Applets");
        is_item && group_value(g, "plugin").is_some_and(|p| ids.contains(&p))
    })
}

/// Виджеты штатной панели Plasma (`org.kde.plasma.desktop` `layout.js`).
pub const STOCK_PANEL_APPLETS: &[&str] = &[
    "org.kde.plasma.kickoff",
    "org.kde.plasma.icontasks",
    "org.kde.plasma.marginsseparator",
    "org.kde.plasma.systemtray",
    "org.kde.plasma.digitalclock",
    "org.kde.plasma.showdesktop",
];
const STOCK_DESKTOP: &str = "org.kde.desktopcontainment";
const SYSTRAY_APPLET: &str = "org.kde.plasma.systemtray";
const SYSTRAY_CONTAINMENT: &str = "org.kde.plasma.private.systemtray";

/// Штатная раскладка из раскладки человека. None — раскладка не ссылается
/// на набор, подменять нечего.
pub fn stock_layout(appletsrc: &str, aero: &[String]) -> Option<String> {
    if !layout_uses(appletsrc, aero) {
        return None;
    }
    let groups = split_groups(appletsrc);
    let is_aero = |g: &Group| group_value(g, "plugin").is_some_and(|p| aero.contains(&p));
    let mut max_id: u64 = 0;
    let mut kept_containments: Vec<String> = Vec::new();
    let mut dropped_applets: Vec<(String, String)> = Vec::new();
    for g in &groups {
        let Some(name) = &g.name else { continue };
        let s = segments(name);
        if s.first() != Some(&"Containments") {
            continue;
        }
        for (i, seg) in s.iter().enumerate() {
            if (i == 1 || (i == 3 && s[2] == "Applets")) && seg.bytes().all(|b| b.is_ascii_digit()) {
                max_id = max_id.max(seg.parse().unwrap_or(0));
            }
        }
        if s.len() == 2 {
            let zero = |k: &str| group_value(g, k).is_none_or(|v| v == "0");
            let plugin = group_value(g, "plugin").unwrap_or_default();
            if zero("formfactor") && zero("location") && plugin != SYSTRAY_CONTAINMENT {
                kept_containments.push(s[1].to_string());
            }
        }
        if s.len() == 4 && s[2] == "Applets" && is_aero(g) {
            dropped_applets.push((s[1].to_string(), s[3].to_string()));
        }
    }

    let mut out = String::new();
    for g in &groups {
        let keep = match &g.name {
            None => true,
            Some(name) => {
                let s = segments(name);
                if s.first() != Some(&"Containments") || s.len() < 2 {
                    true
                } else if !kept_containments.iter().any(|c| c == s[1]) {
                    false
                } else {
                    !(s.len() >= 4 && s[2] == "Applets" && dropped_applets.iter().any(|(c, a)| c == s[1] && a == s[3]))
                }
            }
        };
        if !keep {
            continue;
        }
        if let Some(name) = &g.name {
            out.push_str(&format!("[{name}]\n"));
        }
        let desktop_aero = g.name.as_deref().is_some_and(|n| segments(n).len() == 2) && is_aero(g);
        for l in &g.lines {
            if desktop_aero && l.split_once('=').is_some_and(|(k, _)| k.trim() == "plugin") {
                out.push_str(&format!("plugin={STOCK_DESKTOP}\n"));
            } else {
                out.push_str(l);
                out.push('\n');
            }
        }
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.trim().is_empty() {
        out.push('\n');
    }

    // Штатная панель. Виджет, который подменяет сам набор (тот же
    // опознаватель в его пакете), в неё не ставится: он и есть сломанный.
    let panel = max_id + 1;
    let mut next = panel;
    let mut order = Vec::new();
    let mut applets = String::new();
    let mut systray_inner = None;
    for id in STOCK_PANEL_APPLETS.iter().filter(|id| !aero.iter().any(|a| a == *id)) {
        next += 1;
        order.push(next.to_string());
        applets.push_str(&format!("[Containments][{panel}][Applets][{next}]\nimmutability=1\nplugin={id}\n\n"));
        if *id == SYSTRAY_APPLET && !aero.iter().any(|a| a == SYSTRAY_CONTAINMENT) {
            systray_inner = Some(next);
        }
    }
    out.push_str(&format!(
        "[Containments][{panel}]\nactivityId=\nformfactor=2\nimmutability=1\nlastScreen=0\nlocation=4\nplugin=org.kde.panel\nwallpaperplugin=org.kde.image\n\n"
    ));
    out.push_str(&applets);
    if let Some(tray) = systray_inner {
        next += 1;
        out.push_str(&format!("[Containments][{panel}][Applets][{tray}][Configuration]\nSystrayContainmentId={next}\n\n"));
        out.push_str(&format!(
            "[Containments][{next}]\nactivityId=\nformfactor=2\nimmutability=1\nlastScreen=0\nlocation=4\nplugin={SYSTRAY_CONTAINMENT}\nwallpaperplugin=org.kde.image\n\n"
        ));
    }
    out.push_str(&format!("[Containments][{panel}][General]\nAppletOrder={}\n", order.join(";")));
    Some(out)
}

// ============================================================================
// Состояние: что сохранено и почему
// ============================================================================

/// Почему оформление откатили.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FallbackCause {
    /// Сверка разошлась — сойдётся при входе, всё вернётся само.
    #[default]
    Mismatch,
    /// Попросил сторож сеанса (падения после входа): сверка тогда сходилась,
    /// поэтому сама по себе сходящаяся сверка — не повод возвращать.
    Requested { reason: String },
}

/// Сохранённое оформление человека.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SavedLook {
    pub cause: FallbackCause,
    /// Когда откатили (секунды эпохи).
    pub since: u64,
    /// Прежние значения ключей `kwinrc [Plugins]`.
    pub effects: Vec<SavedKey>,
    /// Раскладка человека лежит копией ([`GuardPaths::saved_appletsrc`]).
    pub layout: bool,
    /// Отпечаток записанной нами штатной раскладки — по нему видно, менял ли
    /// её человек.
    pub layout_written: Option<String>,
    /// Пересборка прошла — возвращать при входе, даже если откат просил
    /// сторож сеанса.
    pub rebuilt: bool,
    /// Технические причины (для «Подробнее»).
    pub why: Vec<String>,
}

impl SavedLook {
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty() && !self.layout
    }
}

/// Отпечаток текста (FNV-1a, 64 бита): стабилен между сборками.
pub fn fingerprint(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Записать файл атомарно: рядом и переименованием. Прерванная запись
/// оставляет прежний файл целым.
pub(crate) fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("rexon-tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Прочитать сохранённое. Нет файла или он испорчен — None.
pub fn load_saved(paths: &GuardPaths) -> Option<SavedLook> {
    serde_json::from_str(&read_text(&paths.saved_json())?).ok()
}

fn store_saved(paths: &GuardPaths, saved: &SavedLook) -> Result<(), String> {
    let text = serde_json::to_string_pretty(saved).map_err(|e| e.to_string())?;
    write_atomic(&paths.saved_json(), &text)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ============================================================================
// ОБЩЕЕ: откатить на штатное и запомнить; вернуть
// ============================================================================

/// Что откатить.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FallbackPlan {
    /// Опознаватели эффектов KWin, которые выключить.
    pub effects: Vec<String>,
    /// Опознаватели виджетов набора: раскладка, ссылающаяся на них,
    /// подменяется штатной. Пусто — раскладку не трогать.
    pub applets: Vec<String>,
    /// Технические причины (для «Подробнее»).
    pub why: Vec<String>,
}

impl FallbackPlan {
    /// Весь набор: и эффекты, и раскладка (так откатывает сторож сеанса).
    pub fn whole(parts: &AeroParts) -> Self {
        Self {
            effects: parts.effects.iter().map(|e| e.id.clone()).collect(),
            applets: parts.applets.clone(),
            why: Vec::new(),
        }
    }
}

/// Что сделал откат.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FallbackDone {
    /// Выключенные сейчас эффекты (ключи `kwinrc`).
    pub effects_off: Vec<String>,
    /// Раскладка подменена сейчас.
    pub layout_replaced: bool,
}

impl FallbackDone {
    pub fn changed(&self) -> bool {
        !self.effects_off.is_empty() || self.layout_replaced
    }
}

/// Откатить оформление на штатное и запомнить, как было.
///
/// ОБЩАЯ функция: её зовёт сторож до KWin ([`guard_with`]) и позовёт сторож
/// сеанса. Порядок для каждого файла: копия в `state_dir` (только если её
/// ещё нет) → запись «что сохранено» → правка файла атомарной заменой.
/// Прерывание в любом месте не теряет настроек: копия всегда раньше правки.
///
/// Идемпотентна: повторный вызов при уже откаченном оформлении ничего не
/// перезаписывает в копиях; доделывает только то, что снова включилось
/// (новый эффект, раскладка, которую Plasma заново собрала из пакета Aero).
///
/// ⚠️ Раскладку править можно, только пока plasmashell не запущен (он
/// держит её в памяти и записал бы поверх) — посреди сеанса зовите
/// [`request_fallback`].
pub fn fall_back_to_stock(paths: &GuardPaths, plan: &FallbackPlan, cause: FallbackCause) -> Result<FallbackDone, String> {
    let mut saved = load_saved(paths).unwrap_or_default();
    if saved.is_empty() {
        saved.cause = cause;
        saved.since = now_secs();
        saved.rebuilt = false;
    } else if matches!(cause, FallbackCause::Requested { .. }) {
        // Падение после входа сильнее расхождения: такое не возвращается
        // само по сходящейся сверке.
        saved.cause = cause;
    }
    for w in &plan.why {
        if !saved.why.contains(w) {
            saved.why.push(w.clone());
        }
    }
    let mut done = FallbackDone::default();

    // ── kwinrc ──
    if !plan.effects.is_empty() {
        let kwinrc = read_text(&paths.kwinrc());
        let before = saved.effects.len();
        if let Some(new_text) = disable_effects(kwinrc.as_deref(), &plan.effects, &mut saved.effects) {
            if let Some(original) = &kwinrc {
                if !paths.saved_kwinrc().exists() {
                    write_atomic(&paths.saved_kwinrc(), original)?;
                }
            }
            store_saved(paths, &saved)?;
            write_atomic(&paths.kwinrc(), &new_text)?;
            done.effects_off = plan
                .effects
                .iter()
                .map(|id| enabled_key(id))
                .filter(|k| ini_get(kwinrc.as_deref().unwrap_or_default(), KWIN_PLUGINS_GROUP, k).as_deref() != Some(OFF))
                .collect();
        } else if saved.effects.len() != before {
            store_saved(paths, &saved)?;
        }
    }

    // ── раскладка ──
    if !plan.applets.is_empty() {
        if let Some(current) = read_text(&paths.appletsrc()) {
            if let Some(stock) = stock_layout(&current, &plan.applets) {
                // Копия — только первая: если она уже есть, в файле сейчас
                // НЕ раскладка человека (Plasma собрала её заново из пакета).
                if !saved.layout || !paths.saved_appletsrc().exists() {
                    write_atomic(&paths.saved_appletsrc(), &current)?;
                    saved.layout = true;
                }
                saved.layout_written = Some(fingerprint(&stock));
                store_saved(paths, &saved)?;
                write_atomic(&paths.appletsrc(), &stock)?;
                done.layout_replaced = true;
            }
        }
    }
    if !saved.is_empty() {
        store_saved(paths, &saved)?;
    }
    Ok(done)
}

/// Что возвращать.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreScope {
    /// Всё сохранённое (до запуска plasmashell — при входе или вручную).
    All,
    /// Только эффекты KWin (посреди сеанса: раскладку держит plasmashell).
    EffectsOnly,
}

/// Что вернул возврат.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RestoreDone {
    /// Было что возвращать.
    pub had_saved: bool,
    /// Возвращённые ключи `kwinrc`.
    pub effects: Vec<String>,
    /// Ключи, которые человек поменял сам, — оставлены как есть.
    pub kept: Vec<String>,
    /// Раскладка человека возвращена.
    pub layout: bool,
    /// Раскладка ещё ждёт возврата (следующий вход).
    pub layout_pending: bool,
}

/// Вернуть сохранённое [`fall_back_to_stock`]. Идемпотентна: возвращать
/// нечего — `had_saved = false` и ни одного касания файлов.
pub fn restore_saved(paths: &GuardPaths, scope: RestoreScope) -> Result<RestoreDone, String> {
    let Some(mut saved) = load_saved(paths) else {
        return Ok(RestoreDone::default());
    };
    let mut done = RestoreDone { had_saved: !saved.is_empty(), ..Default::default() };

    if !saved.effects.is_empty() {
        let kwinrc = read_text(&paths.kwinrc());
        let (new_text, kept) = restore_effects(kwinrc.as_deref(), &saved.effects);
        if let Some(t) = new_text {
            write_atomic(&paths.kwinrc(), &t)?;
        }
        done.effects = saved.effects.iter().map(|s| s.key.clone()).filter(|k| !kept.contains(k)).collect();
        done.kept = kept;
        saved.effects.clear();
        store_saved(paths, &saved)?;
    }

    if saved.layout {
        if scope == RestoreScope::All {
            let copy = paths.saved_appletsrc();
            match read_text(&copy) {
                Some(original) => {
                    if let Some(current) = read_text(&paths.appletsrc()) {
                        if Some(fingerprint(&current)) != saved.layout_written && current != original {
                            write_atomic(&paths.during_fallback_appletsrc(), &current)?;
                        }
                    }
                    write_atomic(&paths.appletsrc(), &original)?;
                    done.layout = true;
                }
                // Копии нет (стёрли руками) — возвращать нечего, но и
                // помнить о ней незачем.
                None => {}
            }
            saved.layout = false;
            saved.layout_written = None;
            let _ = std::fs::remove_file(&copy);
            store_saved(paths, &saved)?;
        } else {
            done.layout_pending = true;
        }
    }

    if saved.is_empty() {
        let _ = std::fs::remove_file(paths.saved_json());
        let _ = std::fs::remove_file(paths.saved_kwinrc());
    }
    Ok(done)
}

/// Отметка сторожа сеанса: со следующего входа — штатное оформление (её
/// применит [`guard_with`] до KWin). `reason` — человеческая причина для
/// уведомления.
pub fn request_fallback(paths: &GuardPaths, reason: &str) -> Result<(), String> {
    write_atomic(&paths.request_file(), reason)
}

/// Пересборка прошла: возвращать при следующем входе, кто бы ни просил отката.
pub fn mark_rebuilt(paths: &GuardPaths) -> Result<(), String> {
    let Some(mut saved) = load_saved(paths) else { return Ok(()) };
    saved.rebuilt = true;
    store_saved(paths, &saved)
}

// ============================================================================
// Сторож при входе
// ============================================================================

/// Какое уведомление показать после входа.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PendingNotice {
    /// Оформление выключено на этот вход.
    TurnedOff { effects: bool, layout: bool, why: Vec<String>, reason: Option<String> },
    /// Сохранённое оформление возвращено.
    Restored,
    /// Только риск — ничего не меняли.
    Risk { why: Vec<String> },
    /// Сторож сеанса: plasmashell упал из-за набора, раскладка станет
    /// штатной со следующего входа ([`request_fallback`]).
    NextLogin { reason: String },
}

/// Что сделал сторож.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardOutcome {
    pub verdict: Verdict,
    pub notice: Option<PendingNotice>,
    /// Строки для журнала.
    pub log: Vec<String>,
}

/// Сторож целиком по готовой сверке: решение → откат/возврат → уведомление.
///
/// `risk_fingerprint` — отпечаток «только риска»: о том же самом второй раз
/// не говорим.
pub fn guard_with(paths: &GuardPaths, verdict: Verdict, parts: &AeroParts, risk_fingerprint: &str) -> GuardOutcome {
    let mut log = Vec::new();
    let mut notice = None;
    if verdict == Verdict::NotInstalled {
        // Набор сняли, а сохранённое осталось: возвращать раскладку с
        // виджетами, которых больше нет, нельзя; оставляем как есть.
        return GuardOutcome { verdict, notice, log: vec!["session-guard: AeroThemePlasma is not installed".into()] };
    }
    let requested = read_text(&paths.request_file());
    if let Some(reason) = &requested {
        let plan = FallbackPlan::whole(parts);
        match fall_back_to_stock(paths, &plan, FallbackCause::Requested { reason: reason.trim().to_string() }) {
            Ok(done) => {
                let _ = std::fs::remove_file(paths.request_file());
                log.push(format!("session-guard: requested fallback: {done:?}"));
                notice = Some(PendingNotice::TurnedOff {
                    effects: !done.effects_off.is_empty(),
                    layout: done.layout_replaced,
                    why: Vec::new(),
                    reason: Some(reason.trim().to_string()),
                });
            }
            Err(e) => log.push(format!("session-guard: requested fallback failed: {e}")),
        }
        return GuardOutcome { verdict, notice, log };
    }
    match &verdict {
        Verdict::NotInstalled => {}
        Verdict::Healthy => {
            let saved = load_saved(paths).filter(|s| !s.is_empty());
            if let Some(s) = saved {
                let auto = s.rebuilt || s.cause == FallbackCause::Mismatch;
                if auto {
                    match restore_saved(paths, RestoreScope::All) {
                        Ok(done) => {
                            log.push(format!("session-guard: restored {done:?}"));
                            notice = Some(PendingNotice::Restored);
                        }
                        Err(e) => log.push(format!("session-guard: restore failed: {e}")),
                    }
                } else {
                    log.push("session-guard: kept the fallback requested after a crash".into());
                }
            }
            let _ = std::fs::remove_file(paths.risk_stamp());
        }
        Verdict::NoticeOnly { why } => {
            let last = read_text(&paths.risk_stamp());
            if last.as_deref().map(str::trim) != Some(risk_fingerprint) {
                notice = Some(PendingNotice::Risk { why: why.clone() });
                let _ = write_atomic(&paths.risk_stamp(), risk_fingerprint);
            }
            log.push(format!("session-guard: notice only: {why:?}"));
        }
        Verdict::Broken { effects, shell, why } => {
            let plan = FallbackPlan {
                effects: if *effects { parts.effects.iter().map(|e| e.id.clone()).collect() } else { Vec::new() },
                applets: if *shell { parts.applets.clone() } else { Vec::new() },
                why: why.clone(),
            };
            match fall_back_to_stock(paths, &plan, FallbackCause::Mismatch) {
                Ok(done) => {
                    log.push(format!("session-guard: fallback {done:?}; why {why:?}"));
                    // Уже откачено прошлым входом — всё равно напомнить: человек
                    // снова вошёл в оформление без Aero.
                    let saved = load_saved(paths).unwrap_or_default();
                    if done.changed() || !saved.is_empty() {
                        notice = Some(PendingNotice::TurnedOff {
                            effects: !saved.effects.is_empty(),
                            layout: saved.layout,
                            why: why.clone(),
                            reason: None,
                        });
                    }
                }
                Err(e) => log.push(format!("session-guard: fallback failed: {e}")),
            }
        }
    }
    GuardOutcome { verdict, notice, log }
}

// ============================================================================
// Сценарий в plasma-workspace/env
// ============================================================================

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Текст сценария. startplasma ВЫПОЛНЯЕТ эти файлы в общей оболочке (через
/// `.`), поэтому здесь нет ни `exit`, ни `set -e`, ни переменных: выход или
/// ошибка оборвали бы чужие сценарии и переменные сеанса. Вывод — в никуда
/// (startplasma читает вывод этой оболочки). Бинаря нет (Rexon удалили) —
/// сценарий убирает сам себя.
pub fn env_script(program: &str, script_path: &str) -> String {
    let bin = sh_quote(program);
    format!(
        "# Rexon: AeroThemePlasma check before KWin starts. Written and removed by Rexon.\n\
         # It never blocks the login: at most {ENV_TIMEOUT_SECS} s, every error is ignored.\n\
         if [ -x {bin} ]; then\n\
         \x20   if command -v timeout >/dev/null 2>&1; then\n\
         \x20       timeout -k 1 {ENV_TIMEOUT_SECS} {bin} {GUARD_FLAG} </dev/null >/dev/null 2>&1 || true\n\
         \x20   else\n\
         \x20       {bin} {GUARD_FLAG} </dev/null >/dev/null 2>&1 || true\n\
         \x20   fi\n\
         else\n\
         \x20   rm -f {} 2>/dev/null || true\n\
         fi\n",
        sh_quote(script_path)
    )
}

/// Что сделать со сценарием.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptChange {
    Nothing,
    Write(String),
    Remove,
}

/// План: `wanted` — стоит AeroThemePlasma и проверка не выключена;
/// `program` — бинарь Rexon (None — звать нечего, сценарий снимается).
pub fn env_script_change(wanted: bool, program: Option<&str>, script_path: &str, existing: Option<&str>) -> ScriptChange {
    match (wanted, program) {
        (true, Some(p)) if p.starts_with('/') => {
            let text = env_script(p, script_path);
            if existing == Some(text.as_str()) {
                ScriptChange::Nothing
            } else {
                ScriptChange::Write(text)
            }
        }
        _ if existing.is_some() => ScriptChange::Remove,
        _ => ScriptChange::Nothing,
    }
}

/// Применить план сценария.
pub fn sync_env_script(paths: &GuardPaths, wanted: bool, program: Option<&str>) -> Result<ScriptChange, String> {
    let path = paths.env_script();
    let existing = read_text(&path);
    let change = env_script_change(wanted, program, &path.to_string_lossy(), existing.as_deref());
    match &change {
        ScriptChange::Nothing => {}
        ScriptChange::Write(text) => write_atomic(&path, text)?,
        ScriptChange::Remove => std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?,
    }
    Ok(change)
}

/// Проверка оформления включена в настройках ([`aeroshell::CHECK_ON_START_SETTING`]).
pub(crate) fn check_enabled_live() -> bool {
    let saved = crate::settings::SettingsManager::new()
        .ok()
        .and_then(|m| m.get(aeroshell::CHECK_ON_START_SETTING).and_then(serde_json::Value::as_bool));
    aeroshell::check_on_start(saved)
}

/// Стоит ли оболочка AeroThemePlasma (по базе pacman, без запуска pacman).
pub(crate) fn aero_installed(db: &LocalDb) -> bool {
    db.entries.iter().any(|(n, _, _)| n.starts_with(aeroshell::SHELL_PACKAGE_PREFIX))
}

/// Поставить или снять сценарий на живой машине. Зовётся вместе с
/// остальной слежкой за обновлениями (при запуске Rexon).
pub fn sync_env_script_live() -> Result<ScriptChange, String> {
    let db = LocalDb::read(Path::new(PACMAN_LOCAL));
    let entry = crate::installer::launch_entry_live();
    let program = match &entry {
        crate::installer::LaunchEntry::MainWindow(p) => Some(p.clone()),
        crate::installer::LaunchEntry::Python => None,
    };
    let wanted = aero_installed(&db) && check_enabled_live();
    let paths = GuardPaths::live();
    // Сторож сеанса — при тех же условиях и той же точкой: оба ставятся,
    // пока стоит AeroThemePlasma, и снимаются вместе с ней.
    let exec = crate::app::unit_launch_command(&entry, None, &[crate::session_watch::WATCH_FLAG]);
    let program_ref = program.as_deref();
    if let Err(e) = crate::session_watch::sync_unit_live(
        &paths.config_home.to_string_lossy(),
        wanted,
        exec.as_deref(),
        program_ref,
    ) {
        crate::log_warn!("session-watch", "юнит сторожа сеанса: {e}");
    }
    sync_env_script(&paths, wanted, program_ref)
}

// ============================================================================
// Живая сверка (быстрая)
// ============================================================================

/// Итог живой сверки.
pub struct LiveFacts {
    pub report: Report,
    pub plasmashell_problems: Vec<String>,
    pub parts: AeroParts,
}

fn ldd_live(path: &str) -> Option<String> {
    let r = crate::utils::run(
        &crate::utils::RunSpec {
            argv: vec!["ldd".into(), "-r".into(), path.into()],
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(LDD_TIMEOUT_SECS),
            lang_c: true,
            stderr_to_stdout: true,
            ..Default::default()
        },
        None,
    );
    matches!(r.outcome, crate::utils::RunOutcome::Ok | crate::utils::RunOutcome::Failed)
        .then(|| String::from_utf8_lossy(&r.stdout).to_string())
}

/// `ldd -r` по всем путям разом (по потоку на ядро): один `ldd` — десятки
/// миллисекунд, по очереди на два десятка модулей вышла бы секунда.
fn ldd_parallel(paths: &[String]) -> std::collections::HashMap<String, String> {
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out = std::sync::Mutex::new(std::collections::HashMap::new());
    std::thread::scope(|s| {
        for _ in 0..workers.min(paths.len().max(1)) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(p) = paths.get(i) else { break };
                if let Some(text) = ldd_live(p) {
                    out.lock().expect("замок результатов").insert(p.clone(), text);
                }
            });
        }
    });
    out.into_inner().unwrap_or_default()
}

/// Сверка на живой машине. None — AeroThemePlasma не стоит (дальше не идём:
/// это почти весь путь сторожа у всех, кроме владельцев набора).
pub fn facts_live() -> Option<LiveFacts> {
    let db = LocalDb::read(Path::new(PACMAN_LOCAL));
    if !aero_installed(&db) {
        return None;
    }
    let forks = crate::system_plan::filter_plasma_forks(&db.names());
    let files: Vec<(String, Vec<String>)> = forks
        .iter()
        .map(|p| (p.clone(), db.file_list(p).map(|o| aeroshell::package_files(&o)).unwrap_or_default()))
        .collect();
    let read_file = |p: &str| std::fs::read(p).ok();
    let parts = aero_parts(&files, &read_file);
    let has_ldd = crate::utils::which("ldd", None).is_some();
    let mut targets: Vec<String> = files.iter().flat_map(|(_, f)| aeroshell::qt_module_paths(f)).collect();
    targets.push(PLASMASHELL_BIN.to_string());
    let ldd = if has_ldd { ldd_parallel(&targets) } else { Default::default() };
    let file_list = |p: &str| db.file_list(p);
    let kwin = db.version("kwin");
    let plasma = db.version("plasma-workspace");
    let qt = db.version("qt6-base");
    let report = aeroshell::check_modules_with(
        &forks,
        kwin.as_deref(),
        plasma.as_deref(),
        qt.as_deref(),
        &file_list,
        &read_file,
        &|p| ldd.get(p).cloned(),
    );
    let plasmashell_problems = ldd.get(PLASMASHELL_BIN).map(|o| aeroshell::ldd_problems(o, &[])).unwrap_or_default();
    Some(LiveFacts { report, plasmashell_problems, parts })
}

/// Сеанс — Plasma (или о столе ещё ничего не сказано: startplasma может
/// выставить имя стола позже сценариев).
fn plasma_session(xdg_current_desktop: Option<&str>) -> bool {
    xdg_current_desktop.is_none_or(|d| d.trim().is_empty() || d.split(':').any(|p| p.eq_ignore_ascii_case("KDE")))
}

/// `rexon --session-guard [--dry-run]` — его зовёт сценарий до KWin.
/// Всегда 0: вход важнее сторожа.
pub fn guard_route_live(dry_run: bool, out: &mut dyn FnMut(&str)) -> i32 {
    let started = std::time::Instant::now();
    if !plasma_session(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref()) {
        out("session-guard: not a Plasma session");
        return 0;
    }
    if !check_enabled_live() {
        out("session-guard: disabled in settings");
        return 0;
    }
    let Some(facts) = facts_live() else {
        out("session-guard: AeroThemePlasma is not installed");
        return 0;
    };
    let verdict = decide(&facts.report, &facts.plasmashell_problems, &facts.parts);
    out(&format!("session-guard: {verdict:?} in {} ms", started.elapsed().as_millis()));
    if dry_run {
        out(&format!("session-guard: effects {:?}", facts.parts.effects));
        out(&format!("session-guard: applets {:?}", facts.parts.applets));
        return 0;
    }
    let advice = aeroshell::Advice::Rebuild { packages: facts.report.rebuild.clone() };
    let risk_fp = aeroshell::notify_fingerprint(&facts.report, &advice);
    let paths = GuardPaths::live();
    let outcome = guard_with(&paths, verdict, &facts.parts, &risk_fp);
    for l in &outcome.log {
        crate::log_info!("session-guard", "{l}");
        out(l);
    }
    if let Some(notice) = &outcome.notice {
        queue_notice_live(&paths, notice, out);
    }
    0
}

/// Уведомление в очередь и отдельный процесс, который его покажет, когда
/// служба уведомлений поднимется (зовёт и сторож сеанса).
pub(crate) fn queue_notice_live(paths: &GuardPaths, notice: &PendingNotice, out: &mut dyn FnMut(&str)) {
    if let Ok(text) = serde_json::to_string(notice) {
        if write_atomic(&paths.notice_json(), &text).is_ok() {
            spawn_notice_live(out);
        }
    }
}

/// Отложенное уведомление — отдельным процессом в своей группе: сценарий
/// (и `timeout` вокруг сторожа) его не ждёт и не гасит.
fn spawn_notice_live(out: &mut dyn FnMut(&str)) {
    use std::os::unix::process::CommandExt;
    let Ok(exe) = std::env::current_exe() else { return };
    let r = std::process::Command::new(exe)
        .arg(NOTICE_FLAG)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    if let Err(e) = r {
        out(&format!("session-guard: notice process: {e}"));
    }
}

// ============================================================================
// Уведомление после входа
// ============================================================================

/// Заголовок: оформление выключено на этот вход.
pub fn turned_off_title() -> String {
    tr("AeroThemePlasma Is Turned Off for Now")
}

/// Текст: что случилось и что делать.
pub fn turned_off_body(reason: Option<&str>) -> String {
    match reason {
        Some(r) if !r.is_empty() => trf(
            "Plasma failed to start with AeroThemePlasma (%1), so Rexon switched to the standard \
             Plasma look for now. Your settings are saved and come back after a rebuild.",
            &[&r],
        ),
        _ => tr("Plasma was updated, and AeroThemePlasma no longer fits it. To keep the desktop \
                 working, Rexon switched to the standard Plasma look for now. Your settings are \
                 saved and come back after a rebuild."),
    }
}

/// Подробности: что именно выключено.
pub fn turned_off_details(effects: bool, layout: bool, why: &[String]) -> String {
    let mut lines = Vec::new();
    if effects {
        lines.push(tr("The AeroThemePlasma effects are turned off in KWin."));
    }
    if layout {
        lines.push(tr("The panels are replaced with the standard Plasma panel; your layout is saved."));
    }
    lines.push(trf("To bring everything back without rebuilding: rexon %1", &[&RESTORE_FLAG]));
    let mut text = lines.join("\n");
    if !why.is_empty() {
        text.push_str("\n\n");
        text.push_str(&why.iter().take(6).map(|w| format!("• {w}")).collect::<Vec<_>>().join("\n"));
    }
    text
}

/// Заголовок: раскладка станет штатной со следующего входа.
pub fn next_login_title() -> String {
    tr("AeroThemePlasma Turns Off at Next Login")
}

/// Текст: plasmashell упал из-за набора, раскладку сейчас не тронуть.
pub fn next_login_body(reason: &str) -> String {
    trf(
        // TRANSLATORS: %1 says which AeroThemePlasma module crashed.
        "The Plasma desktop crashed because of AeroThemePlasma (%1). From the next login, \
         Rexon uses the standard Plasma panels until AeroThemePlasma is rebuilt. Your layout \
         is saved and comes back after a rebuild.",
        &[&reason],
    )
}

pub fn restored_title() -> String {
    tr("AeroThemePlasma Is Back")
}

pub fn restored_body() -> String {
    tr("AeroThemePlasma fits Plasma again, so Rexon brought back your saved look and panels.")
}

pub fn rebuild_button() -> String {
    tr("Rebuild")
}

pub fn details_button() -> String {
    tr("Details")
}

/// После пересборки: эффекты вернулись; раскладка — при следующем входе.
pub fn after_rebuild_message(layout_pending: bool) -> String {
    if layout_pending {
        tr("Your AeroThemePlasma effects are back on. The panels come back at the next login.")
    } else {
        tr("Your AeroThemePlasma effects are back on.")
    }
}

/// Итог `rexon --session-guard-restore` для печати.
pub fn restore_report(done: &RestoreDone) -> String {
    if !done.had_saved {
        return tr("Nothing to restore: Rexon hasn't changed your Plasma look.");
    }
    let mut lines = vec![tr("Restored the saved Plasma look.")];
    if !done.kept.is_empty() {
        lines.push(trf("Left as you changed them: %1", &[&done.kept.join(", ")]));
    }
    if done.layout_pending {
        lines.push(tr("The panels come back at the next login."));
    }
    lines.join("\n")
}

/// notify-send с действиями; ждёт службу уведомлений. → нажатое действие.
fn notify_live(summary: &str, body: &str, actions: &[(&str, String)], wait: bool) -> Option<String> {
    crate::utils::which("notify-send", None)?;
    let mut argv: Vec<String> = vec![
        "notify-send".into(),
        format!("--app-name={}", crate::utils::NOTIFY_APP_NAME),
        "--icon=rexon".into(),
    ];
    if wait {
        argv.push("--urgency=critical".into());
        argv.push("--wait".into());
    }
    for (id, label) in actions {
        argv.push(format!("--action={id}={label}"));
    }
    argv.push(summary.into());
    argv.push(body.into());
    let started = std::time::Instant::now();
    loop {
        let r = crate::utils::run(
            &crate::utils::RunSpec {
                argv: argv.clone(),
                output: crate::utils::RunOutput::Capture,
                timeout_secs: Some(if wait { aeroshell::NOTIFY_WAIT_SECS } else { 10 }),
                ..Default::default()
            },
            None,
        );
        if r.returncode == Some(0) {
            return Some(String::from_utf8_lossy(&r.stdout).trim().to_string());
        }
        // Службы уведомлений ещё нет (plasmashell поднимается) — подождать.
        if started.elapsed().as_secs() > NOTICE_RETRY_SECS {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
}

/// `rexon --session-guard-notice` — уведомление после входа.
pub fn notice_route_live(out: &mut dyn FnMut(&str)) -> i32 {
    let paths = GuardPaths::live();
    let Some(text) = read_text(&paths.notice_json()) else { return 0 };
    let _ = std::fs::remove_file(paths.notice_json());
    let Ok(notice) = serde_json::from_str::<PendingNotice>(&text) else { return 0 };
    std::thread::sleep(std::time::Duration::from_secs(NOTICE_DELAY_SECS));
    let actions = [("rebuild", rebuild_button()), ("details", details_button())];
    let (summary, body, details) = match &notice {
        PendingNotice::TurnedOff { effects, layout, why, reason } => {
            (turned_off_title(), turned_off_body(reason.as_deref()), turned_off_details(*effects, *layout, why))
        }
        PendingNotice::Restored => {
            notify_live(&restored_title(), &restored_body(), &[], false);
            return 0;
        }
        PendingNotice::NextLogin { reason } => (
            next_login_title(),
            next_login_body(reason),
            trf("To bring everything back without rebuilding: rexon %1", &[&RESTORE_FLAG]),
        ),
        PendingNotice::Risk { why } => {
            let report_text = why.iter().take(6).map(|w| format!("• {w}")).collect::<Vec<_>>().join("\n");
            (
                crate::de_progress::aeroshell_risk_title(),
                tr("After the system update, AeroThemePlasma is still built for the previous version \
                    of the system. It works for now, but it can fail at any moment. Rexon can rebuild it."),
                report_text,
            )
        }
    };
    match notify_live(&summary, &body, &actions, true).as_deref() {
        Some("rebuild") => {
            // Пересборка — тем же путём, что у окна: Rexon открывается на
            // проверке, предлагает пересборку, пароль — окном polkit.
            if let Err(e) = crate::installer::open_rexon_live(&[aeroshell::OPEN_FLAG]) {
                out(&format!("session-guard: {e}"));
            }
        }
        Some("details") => {
            if let Some(a) = notify_live(&summary, &details, &actions[..1], true) {
                if a == "rebuild" {
                    let _ = crate::installer::open_rexon_live(&[aeroshell::OPEN_FLAG]);
                }
            }
        }
        _ => {}
    }
    0
}

/// После удачной пересборки ([`crate::system_de_exec::rebuild_aeroshell_live`]):
/// сверка сходится — вернуть эффекты сейчас (и попросить KWin перечитать
/// настройки), раскладку — при следующем входе. → строка для итога окна.
pub fn after_rebuild_live() -> Option<String> {
    let paths = GuardPaths::live();
    load_saved(&paths).filter(|s| !s.is_empty())?;
    let facts = facts_live()?;
    let verdict = decide(&facts.report, &facts.plasmashell_problems, &facts.parts);
    if matches!(verdict, Verdict::Broken { .. }) {
        return None;
    }
    let _ = mark_rebuilt(&paths);
    let done = restore_saved(&paths, RestoreScope::EffectsOnly).ok()?;
    if !done.effects.is_empty() {
        reconfigure_kwin_live();
    }
    Some(after_rebuild_message(done.layout_pending))
}

/// Попросить KWin перечитать `kwinrc` (как это делает `kwriteconfig` +
/// `qdbus org.kde.KWin /KWin reconfigure`).
pub(crate) fn reconfigure_kwin_live() {
    if crate::utils::which("dbus-send", None).is_none() {
        return;
    }
    let _ = crate::utils::run(
        &crate::utils::RunSpec {
            argv: reconfigure_kwin_argv(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(5),
            ..Default::default()
        },
        None,
    );
}

/// Команда «KWin, перечитай настройки».
pub fn reconfigure_kwin_argv() -> Vec<String> {
    ["dbus-send", "--session", "--type=method_call", "--dest=org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

// ============================================================================
// Удаление Rexon во время отката
// ============================================================================
//
// Rexon удаляют, пока оформление откачено: возвращать его потом будет
// некому (сценарий до KWin без бинаря снимает сам себя). Поэтому при
// удалении: эффекты — сразу (это `kwinrc`, KWin перечитает его), а
// раскладку посреди сеанса не вернуть (plasmashell держит её в памяти и при
// выходе записал бы поверх) — её вернёт ОДНОРАЗОВЫЙ сценарий на чистом sh
// при следующем входе, до plasmashell, и снимет сам себя. Копии лежат в
// `$XDG_STATE_HOME/rexon/session-guard/` — удаление Rexon их не трогает.

/// Однократный возврат раскладки при следующем входе (без Rexon). Как и
/// [`env_script`]: ни `exit`, ни `set -e` — файл выполняется в общей оболочке
/// startplasma.
pub fn restore_once_script(paths: &GuardPaths) -> String {
    let q = |p: PathBuf| sh_quote(&p.to_string_lossy());
    let saved = q(paths.saved_appletsrc());
    let current = q(paths.appletsrc());
    format!(
        "# Rexon was removed while AeroThemePlasma was turned off: put the saved panel\n\
         # layout back once, before plasmashell starts, then remove this file.\n\
         if [ -f {saved} ]; then\n\
         \x20   if [ -f {current} ]; then cp -f {current} {} 2>/dev/null || true; fi\n\
         \x20   cp -f {saved} {current} 2>/dev/null && rm -f {saved} {} {} 2>/dev/null\n\
         fi\n\
         rm -f {} 2>/dev/null || true\n",
        q(paths.during_fallback_appletsrc()),
        q(paths.saved_json()),
        q(paths.saved_kwinrc()),
        q(paths.env_script()),
    )
}

/// Что сделано при удалении Rexon.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UninstallRelease {
    pub restored: RestoreDone,
    /// Раскладку вернёт однократный сценарий при следующем входе.
    pub layout_next_login: bool,
}

/// Удаление Rexon: вернуть сохранённое и снять сценарий до KWin.
/// Ничего не сохранено — только снять сценарий и отметки.
pub fn release_for_uninstall(paths: &GuardPaths) -> Result<UninstallRelease, String> {
    let restored = restore_saved(paths, RestoreScope::EffectsOnly)?;
    let _ = std::fs::remove_file(paths.request_file());
    let _ = std::fs::remove_file(paths.notice_json());
    let script = paths.env_script();
    let layout_next_login = restored.layout_pending && paths.saved_appletsrc().exists();
    if layout_next_login {
        write_atomic(&script, &restore_once_script(paths))?;
    } else if script.exists() {
        std::fs::remove_file(&script).map_err(|e| format!("{}: {e}", script.display()))?;
    }
    Ok(UninstallRelease { restored, layout_next_login })
}

/// Маршруты командной строки сторожа. None — это не его флаги.
pub fn cli_route(args: &[String], out: &mut dyn FnMut(&str)) -> Option<i32> {
    let has = |f: &str| args.iter().any(|a| a == f);
    if has(NOTICE_FLAG) {
        return Some(notice_route_live(out));
    }
    if has(RESTORE_FLAG) {
        // Вручную, посреди сеанса: раскладку держит plasmashell — эффекты
        // сейчас, раскладка при следующем входе (её вернёт сторож).
        let paths = GuardPaths::live();
        let _ = mark_rebuilt(&paths);
        return Some(match restore_saved(&paths, RestoreScope::EffectsOnly) {
            Ok(done) => {
                if !done.effects.is_empty() {
                    reconfigure_kwin_live();
                }
                for l in restore_report(&done).lines() {
                    out(l);
                }
                0
            }
            Err(e) => {
                out(&e);
                1
            }
        });
    }
    if has(GUARD_FLAG) {
        return Some(guard_route_live(has(DRY_RUN_FLAG), out));
    }
    if has(crate::session_watch::WATCH_FLAG) {
        return Some(crate::session_watch::watch_route_live(out));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aeroshell::PackageVerdict;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Self {
            let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let p = std::env::temp_dir().join(format!("rexon-session-guard-{tag}-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
        fn paths(&self) -> GuardPaths {
            GuardPaths::new(self.0.join("config"), self.0.join("state/rexon/session-guard"))
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

    fn pkg(name: &str, marker: Marker, built: Option<&str>, inst: Option<&str>, problems: &[&str]) -> PackageVerdict {
        let stale = matches!((built, inst), (Some(b), Some(i)) if b != i) || !problems.is_empty();
        PackageVerdict {
            package: name.into(),
            marker,
            built_for: built.map(Into::into),
            installed: inst.map(Into::into),
            compiled: true,
            stale,
            problems: problems.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn report(pkgs: Vec<PackageVerdict>) -> Report {
        let stale: Vec<String> = pkgs.iter().filter(|p| p.stale).map(|p| p.package.clone()).collect();
        Report { needs_rebuild: !stale.is_empty(), stale, packages: pkgs, ..Report::empty() }
    }

    fn parts() -> AeroParts {
        AeroParts {
            effects: vec![
                EffectModule { file_name: "smodglow.so".into(), id: "smodglow".into() },
                EffectModule { file_name: "aeroblur.so".into(), id: "aeroblur".into() },
            ],
            applets: vec!["io.gitgud.wackyideas.SevenStart".into(), "io.gitgud.wackyideas.seventasks".into()],
        }
    }

    const SHELL: &str = "aerothemeplasma-desktop-git";
    const KWINC: &str = "aeroshell-kwin-components-git";
    const LIBP: &str = "aeroshell-libplasma-git";

    // ── база pacman ──

    #[test]
    fn local_db_entry_splits_name_version_release() {
        assert_eq!(local_db_entry("kwin-6.8.1-2.1"), Some(("kwin".into(), "6.8.1-2.1".into())));
        assert_eq!(
            local_db_entry("aerothemeplasma-desktop-git-r123.abc-1"),
            Some(("aerothemeplasma-desktop-git".into(), "r123.abc-1".into()))
        );
        assert_eq!(local_db_entry("ALPM_DB_VERSION"), None);
        assert_eq!(local_db_entry("a-b"), None);
    }

    #[test]
    fn local_files_become_ql_lines_and_parse_like_pacman() {
        let files = "%FILES%\nusr/\nusr/lib/qt6/plugins/kwin/effects/plugins/smodglow.so\n\n%BACKUP%\netc/x\tabc\n";
        let ql = local_files_as_ql(KWINC, files);
        assert_eq!(ql, format!("{KWINC} /usr/\n{KWINC} /usr/lib/qt6/plugins/kwin/effects/plugins/smodglow.so\n"));
        assert_eq!(aeroshell::package_files(&ql), vec!["/usr/lib/qt6/plugins/kwin/effects/plugins/smodglow.so"]);
    }

    #[test]
    fn local_db_reads_versions_and_files_from_disk() {
        let t = Tmp::new("db");
        put(&t.0.join("local/kwin-6.8.1-2.1/desc"), "%NAME%\nkwin\n");
        put(&t.0.join("local/kwin-6.8.1-2.1/files"), "%FILES%\nusr/bin/kwin_wayland\n");
        let db = LocalDb::read(&t.0.join("local"));
        assert_eq!(db.version("kwin").as_deref(), Some("6.8.1-2.1"));
        assert_eq!(db.file_list("kwin").as_deref(), Some("kwin /usr/bin/kwin_wayland\n"));
        assert!(db.file_list("plasma-workspace").is_none());
        assert!(!aero_installed(&db));
    }

    // ── состав набора ──

    #[test]
    fn effect_id_comes_from_cbor_metadata_else_file_name() {
        let mut data = b"\x00junk\xa1\x67KPlugin\xa2\x64Name\x64Glow\x62Id\x68smodglow".to_vec();
        data.extend_from_slice(b"\x00tail");
        assert_eq!(effect_plugin_id(&data, "libsmodglow.so"), "smodglow");
        let json = br#"{"KPlugin": {"Name": "x", "Id": "aeroblur"}}"#;
        assert_eq!(effect_plugin_id(json, "x.so"), "aeroblur");
        assert_eq!(effect_plugin_id(b"nothing here", "smodsnap.so"), "smodsnap");
    }

    #[test]
    fn applet_ids_from_package_paths() {
        assert_eq!(
            applet_id_of("/usr/share/plasma/plasmoids/io.gitgud.wackyideas.SevenStart/contents/ui/main.qml").as_deref(),
            Some("io.gitgud.wackyideas.SevenStart")
        );
        assert_eq!(applet_id_of("/usr/lib/qt6/plugins/plasma/applets/org.kde.plasma.systemtray.so").as_deref(), Some("org.kde.plasma.systemtray"));
        assert_eq!(applet_id_of("/usr/lib/qt6/plugins/kwin/effects/plugins/smodglow.so"), None);
        let files = vec![(
            SHELL.to_string(),
            vec![
                "/usr/share/plasma/plasmoids/a.b/metadata.json".to_string(),
                "/usr/share/plasma/plasmoids/a.b/contents/ui/main.qml".to_string(),
                "/usr/lib/qt6/plugins/kwin/effects/plugins/smodglow.so".to_string(),
            ],
        )];
        let p = aero_parts(&files, &|_| None);
        assert_eq!(p.applets, vec!["a.b"]);
        assert_eq!(p.effects, vec![EffectModule { file_name: "smodglow.so".into(), id: "smodglow".into() }]);
    }

    // ── решение ──

    #[test]
    fn not_installed_without_the_shell_package() {
        let r = report(vec![pkg(KWINC, Marker::KwinEffect, Some("6.7.4"), Some("6.8.1"), &[])]);
        assert_eq!(decide(&r, &[], &parts()), Verdict::NotInstalled);
        assert_eq!(decide(&Report::empty(), &[], &parts()), Verdict::NotInstalled);
    }

    #[test]
    fn healthy_when_everything_fits() {
        let r = report(vec![
            pkg(SHELL, Marker::None, None, None, &[]),
            pkg(KWINC, Marker::KwinEffect, Some("6.8.1"), Some("6.8.1"), &[]),
        ]);
        assert_eq!(decide(&r, &[], &parts()), Verdict::Healthy);
    }

    #[test]
    fn effect_version_mismatch_turns_off_effects_only() {
        let r = report(vec![
            pkg(SHELL, Marker::None, None, None, &[]),
            pkg(KWINC, Marker::KwinEffect, Some("6.7.4"), Some("6.8.1"), &[]),
        ]);
        match decide(&r, &[], &parts()) {
            Verdict::Broken { effects: true, shell: false, why } => assert!(why[0].contains("6.7.4"), "{why:?}"),
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn unresolved_symbol_in_plasmoid_is_shell_side() {
        let r = report(vec![pkg(SHELL, Marker::None, None, None, &["libseventasks.so: undefined symbol: _ZN6Plasma3FooEv"])]);
        assert!(matches!(decide(&r, &[], &parts()), Verdict::Broken { effects: false, shell: true, .. }));
        let r = report(vec![
            pkg(SHELL, Marker::None, None, None, &[]),
            pkg(KWINC, Marker::KwinEffect, Some("6.8.1"), Some("6.8.1"), &["smodglow.so: libkwin.so.6: not found"]),
        ]);
        assert!(matches!(decide(&r, &[], &parts()), Verdict::Broken { effects: true, shell: false, .. }));
    }

    #[test]
    fn stale_libplasma_fork_is_shell_side() {
        let r = report(vec![
            pkg(SHELL, Marker::None, None, None, &[]),
            pkg(LIBP, Marker::PlasmaLibrary, Some("6.7.4"), Some("6.8.1"), &[]),
        ]);
        assert!(matches!(decide(&r, &[], &parts()), Verdict::Broken { effects: false, shell: true, .. }));
    }

    #[test]
    fn plasmashell_ldd_counts_only_with_the_fork() {
        let shell_bad = vec!["undefined symbol: _ZN6Plasma6Applet3fooEv".to_string()];
        let with_fork = report(vec![pkg(SHELL, Marker::None, None, None, &[]), pkg(LIBP, Marker::PlasmaLibrary, Some("6.8.1"), Some("6.8.1"), &[])]);
        assert!(matches!(decide(&with_fork, &shell_bad, &parts()), Verdict::Broken { shell: true, .. }));
        // Без форка беда не в наборе: только уведомление.
        let no_fork = report(vec![pkg(SHELL, Marker::None, None, None, &[])]);
        assert!(matches!(decide(&no_fork, &shell_bad, &parts()), Verdict::NoticeOnly { .. }));
    }

    #[test]
    fn private_qt_risk_alone_is_notice_only() {
        let r = report(vec![pkg(
            SHELL,
            Marker::QtVersion,
            Some("6.11"),
            Some("6.12"),
            &["libnotifications.so: Qt 6.11 private API, installed Qt 6.12"],
        )]);
        assert!(r.at_risk_only());
        assert!(matches!(decide(&r, &[], &parts()), Verdict::NoticeOnly { .. }));
    }

    // ── kwinrc ──

    #[test]
    fn disable_effects_saves_previous_values_and_skips_already_off() {
        let kwinrc = "[Compositing]\nBackend=OpenGL\n\n[Plugins]\nsmodglowEnabled=true\nfooEnabled=false\naeroblurEnabled=false\n";
        let mut saved = Vec::new();
        let ids = vec!["smodglow".to_string(), "aeroblur".to_string(), "smodsnap".to_string()];
        let out = disable_effects(Some(kwinrc), &ids, &mut saved).unwrap();
        assert_eq!(ini_get(&out, "Plugins", "smodglowEnabled").as_deref(), Some("false"));
        assert_eq!(ini_get(&out, "Plugins", "smodsnapEnabled").as_deref(), Some("false"));
        assert_eq!(ini_get(&out, "Plugins", "fooEnabled").as_deref(), Some("false"));
        assert!(out.contains("Backend=OpenGL"));
        assert_eq!(
            saved,
            vec![
                SavedKey { key: "smodglowEnabled".into(), previous: Some("true".into()) },
                SavedKey { key: "smodsnapEnabled".into(), previous: None },
            ]
        );
        // Повтор по уже выключенному: менять нечего, копия не тронута.
        let before = saved.clone();
        assert_eq!(disable_effects(Some(&out), &ids, &mut saved), None);
        assert_eq!(saved, before);
    }

    #[test]
    fn restore_effects_returns_only_our_false() {
        let kwinrc = "[Plugins]\nsmodglowEnabled=false\nsmodsnapEnabled=false\naeroblurEnabled=true\n";
        let saved = vec![
            SavedKey { key: "smodglowEnabled".into(), previous: Some("true".into()) },
            SavedKey { key: "smodsnapEnabled".into(), previous: None },
            SavedKey { key: "aeroblurEnabled".into(), previous: Some("false".into()) },
        ];
        let (out, kept) = restore_effects(Some(kwinrc), &saved);
        let out = out.unwrap();
        assert_eq!(ini_get(&out, "Plugins", "smodglowEnabled").as_deref(), Some("true"));
        assert_eq!(ini_get(&out, "Plugins", "smodsnapEnabled"), None);
        // Человек включил сам — оставлено, и об этом сказано.
        assert_eq!(ini_get(&out, "Plugins", "aeroblurEnabled").as_deref(), Some("true"));
        assert_eq!(kept, vec!["aeroblurEnabled"]);
    }

    // ── раскладка ──

    const APPLETSRC: &str = "[ActionPlugins][0]\nRightButton;NoModifier=org.kde.contextmenu\n\n\
[Containments][1]\nactivityId=abc-123\nformfactor=0\nimmutability=1\nlastScreen=0\nlocation=0\nplugin=org.kde.desktopcontainment\nwallpaperplugin=org.kde.image\n\n\
[Containments][1][Applets][9]\nplugin=io.gitgud.wackyideas.SevenStart\n\n\
[Containments][1][Applets][10]\nplugin=org.kde.plasma.notes\n\n\
[Containments][1][Wallpaper][org.kde.image][General]\nImage=file:///home/u/win7.jpg\n\n\
[Containments][2]\nactivityId=\nformfactor=2\nlocation=4\nplugin=org.kde.panel\n\n\
[Containments][2][Applets][3]\nplugin=io.gitgud.wackyideas.SevenStart\n\n\
[Containments][2][Applets][4]\nplugin=io.gitgud.wackyideas.seventasks\n\n\
[Containments][2][General]\nAppletOrder=3;4\n\n\
[ScreenMapping]\nitemsOnDisabledScreens=\n";

    #[test]
    fn stock_layout_keeps_desktop_and_wallpaper_and_replaces_panels() {
        let a = parts().applets;
        let out = stock_layout(APPLETSRC, &a).unwrap();
        assert!(out.contains("[Containments][1]\nactivityId=abc-123"), "{out}");
        assert!(out.contains("Image=file:///home/u/win7.jpg"));
        assert!(out.contains("[Containments][1][Applets][10]\nplugin=org.kde.plasma.notes"));
        assert!(!out.contains("wackyideas"), "{out}");
        assert!(!out.contains("[Containments][2]"));
        assert!(out.contains("[ActionPlugins][0]") && out.contains("[ScreenMapping]"));
        // Новая панель — после самого старшего номера (10).
        assert!(out.contains("[Containments][11]\nactivityId=\nformfactor=2"), "{out}");
        assert!(out.contains("plugin=org.kde.panel"));
        assert!(out.contains("[Containments][11][Applets][12]\nimmutability=1\nplugin=org.kde.plasma.kickoff"));
        assert!(out.contains("AppletOrder=12;13;14;15;16;17"));
        assert!(out.contains("SystrayContainmentId=18") && out.contains("plugin=org.kde.plasma.private.systemtray"));
        // Без виджетов набора менять нечего.
        assert_eq!(stock_layout(&out, &a), None);
    }

    #[test]
    fn stock_panel_skips_applets_the_set_overrides() {
        let mut a = parts().applets;
        a.push("org.kde.plasma.systemtray".into());
        let out = stock_layout(APPLETSRC, &a).unwrap();
        assert!(!out.contains("plugin=org.kde.plasma.systemtray"));
        assert!(!out.contains("SystrayContainmentId"));
        assert!(out.contains("AppletOrder=12;13;14;15;16"));
    }

    // ── откат и возврат на временном каталоге ──

    fn plan() -> FallbackPlan {
        FallbackPlan::whole(&parts())
    }

    #[test]
    fn fallback_then_restore_round_trips_files_exactly() {
        let t = Tmp::new("roundtrip");
        let p = t.paths();
        let kwinrc = "[Plugins]\nsmodglowEnabled=true\nblurEnabled=false\n";
        put(&p.kwinrc(), kwinrc);
        put(&p.appletsrc(), APPLETSRC);

        let done = fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        assert_eq!(done.effects_off, vec!["smodglowEnabled", "aeroblurEnabled"]);
        assert!(done.layout_replaced);
        assert_eq!(read_text(&p.saved_appletsrc()).as_deref(), Some(APPLETSRC));
        assert_eq!(read_text(&p.saved_kwinrc()).as_deref(), Some(kwinrc));
        let now = read_text(&p.kwinrc()).unwrap();
        assert_eq!(ini_get(&now, "Plugins", "smodglowEnabled").as_deref(), Some("false"));
        assert!(!read_text(&p.appletsrc()).unwrap().contains("wackyideas"));

        let back = restore_saved(&p, RestoreScope::All).unwrap();
        assert!(back.had_saved && back.layout && back.kept.is_empty());
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        let k = read_text(&p.kwinrc()).unwrap();
        assert_eq!(ini_get(&k, "Plugins", "smodglowEnabled").as_deref(), Some("true"));
        assert_eq!(ini_get(&k, "Plugins", "aeroblurEnabled"), None);
        assert_eq!(ini_get(&k, "Plugins", "blurEnabled").as_deref(), Some("false"));
        assert!(!p.saved_json().exists() && !p.saved_appletsrc().exists() && !p.saved_kwinrc().exists());

        // Второй возврат — ничего не трогает.
        let again = restore_saved(&p, RestoreScope::All).unwrap();
        assert!(!again.had_saved);
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
    }

    #[test]
    fn repeated_fallback_never_overwrites_the_copy() {
        let t = Tmp::new("repeat");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), APPLETSRC);
        fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        let saved1 = load_saved(&p).unwrap();

        // Второй вход, набор всё ещё сломан; Plasma тем временем собрала
        // раскладку заново из пакета (снова с виджетами Aero) — копия
        // раскладки человека остаётся прежней.
        let regenerated = APPLETSRC.replace("abc-123", "regenerated");
        put(&p.appletsrc(), &regenerated);
        let done = fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        assert!(done.effects_off.is_empty());
        assert!(done.layout_replaced);
        assert_eq!(read_text(&p.saved_appletsrc()).as_deref(), Some(APPLETSRC));
        let saved2 = load_saved(&p).unwrap();
        assert_eq!(saved2.effects, saved1.effects);

        // Третий вход без перемен — файлы не трогаются вовсе.
        let stock = read_text(&p.appletsrc()).unwrap();
        let done = fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        assert!(!done.changed());
        assert_eq!(read_text(&p.appletsrc()).unwrap(), stock);

        restore_saved(&p, RestoreScope::All).unwrap();
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        assert_eq!(ini_get(&read_text(&p.kwinrc()).unwrap(), "Plugins", "smodglowEnabled").as_deref(), Some("true"));
    }

    #[test]
    fn layout_changed_during_fallback_is_kept_aside() {
        let t = Tmp::new("aside");
        let p = t.paths();
        put(&p.appletsrc(), APPLETSRC);
        fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        let tweaked = format!("{}\n[Containments][99]\nplugin=org.kde.panel\n", read_text(&p.appletsrc()).unwrap());
        put(&p.appletsrc(), &tweaked);
        restore_saved(&p, RestoreScope::All).unwrap();
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        assert_eq!(read_text(&p.during_fallback_appletsrc()).as_deref(), Some(tweaked.as_str()));
    }

    #[test]
    fn effects_only_restore_leaves_layout_for_next_login() {
        let t = Tmp::new("effects-only");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), APPLETSRC);
        fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        let done = restore_saved(&p, RestoreScope::EffectsOnly).unwrap();
        assert!(done.layout_pending && !done.layout);
        assert_eq!(ini_get(&read_text(&p.kwinrc()).unwrap(), "Plugins", "smodglowEnabled").as_deref(), Some("true"));
        assert!(p.saved_appletsrc().exists());
        let done = restore_saved(&p, RestoreScope::All).unwrap();
        assert!(done.layout);
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        assert!(!p.saved_json().exists());
    }

    #[test]
    fn nothing_to_change_leaves_no_state() {
        let t = Tmp::new("nothing");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=false\naeroblurEnabled=false\n");
        put(&p.appletsrc(), "[Containments][1]\nplugin=org.kde.desktopcontainment\n");
        let done = fall_back_to_stock(&p, &plan(), FallbackCause::Mismatch).unwrap();
        assert!(!done.changed());
        assert!(!p.saved_json().exists());
        assert!(!restore_saved(&p, RestoreScope::All).unwrap().had_saved);
    }

    // ── сторож целиком ──

    fn broken() -> Verdict {
        Verdict::Broken { effects: true, shell: true, why: vec!["x".into()] }
    }

    #[test]
    fn guard_falls_back_then_restores_when_check_fits_again() {
        let t = Tmp::new("guard");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), APPLETSRC);
        let o = guard_with(&p, broken(), &parts(), "fp");
        assert!(matches!(o.notice, Some(PendingNotice::TurnedOff { effects: true, layout: true, .. })), "{o:?}");
        // Повторный вход всё ещё сломан — снова напомнить, копию не трогать.
        let o = guard_with(&p, broken(), &parts(), "fp");
        assert!(matches!(o.notice, Some(PendingNotice::TurnedOff { .. })));
        assert_eq!(read_text(&p.saved_appletsrc()).as_deref(), Some(APPLETSRC));
        let o = guard_with(&p, Verdict::Healthy, &parts(), "fp");
        assert_eq!(o.notice, Some(PendingNotice::Restored));
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        // И дальше тишина.
        assert_eq!(guard_with(&p, Verdict::Healthy, &parts(), "fp").notice, None);
    }

    #[test]
    fn guard_does_nothing_when_not_installed() {
        let t = Tmp::new("absent");
        let p = t.paths();
        put(&p.appletsrc(), APPLETSRC);
        request_fallback(&p, "crash").unwrap();
        let o = guard_with(&p, Verdict::NotInstalled, &parts(), "fp");
        assert_eq!(o.notice, None);
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
    }

    #[test]
    fn notice_only_does_not_touch_files_and_speaks_once() {
        let t = Tmp::new("risk");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), APPLETSRC);
        let v = Verdict::NoticeOnly { why: vec!["risk".into()] };
        assert!(matches!(guard_with(&p, v.clone(), &parts(), "fp1").notice, Some(PendingNotice::Risk { .. })));
        assert_eq!(guard_with(&p, v.clone(), &parts(), "fp1").notice, None);
        assert!(guard_with(&p, v, &parts(), "fp2").notice.is_some());
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        assert_eq!(read_text(&p.kwinrc()).as_deref(), Some("[Plugins]\nsmodglowEnabled=true\n"));
        assert!(!p.saved_json().exists());
    }

    #[test]
    fn requested_fallback_is_not_undone_by_a_fitting_check_until_rebuilt() {
        let t = Tmp::new("requested");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), APPLETSRC);
        request_fallback(&p, "KWin crashed 3 times").unwrap();
        let o = guard_with(&p, Verdict::Healthy, &parts(), "fp");
        match o.notice {
            Some(PendingNotice::TurnedOff { reason: Some(r), .. }) => assert_eq!(r, "KWin crashed 3 times"),
            n => panic!("{n:?}"),
        }
        assert!(!p.request_file().exists());
        assert_eq!(guard_with(&p, Verdict::Healthy, &parts(), "fp").notice, None);
        assert!(p.saved_appletsrc().exists());
        mark_rebuilt(&p).unwrap();
        assert_eq!(guard_with(&p, Verdict::Healthy, &parts(), "fp").notice, Some(PendingNotice::Restored));
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
    }

    #[test]
    fn effects_only_breakage_keeps_the_layout() {
        let t = Tmp::new("effects-broken");
        let p = t.paths();
        put(&p.appletsrc(), APPLETSRC);
        let v = Verdict::Broken { effects: true, shell: false, why: vec![] };
        guard_with(&p, v, &parts(), "fp");
        assert_eq!(read_text(&p.appletsrc()).as_deref(), Some(APPLETSRC));
        let k = read_text(&p.kwinrc()).unwrap();
        assert_eq!(ini_get(&k, "Plugins", "smodglowEnabled").as_deref(), Some("false"));
    }

    // ── сценарий env ──

    #[test]
    fn env_script_text_is_thin_and_safe_to_source() {
        let s = env_script("/usr/bin/rexon", "/home/u/.config/plasma-workspace/env/rexon-session-guard.sh");
        assert!(s.contains("if [ -x '/usr/bin/rexon' ]; then"));
        assert!(s.contains("timeout -k 1 3 '/usr/bin/rexon' --session-guard </dev/null >/dev/null 2>&1 || true"));
        assert!(s.contains("rm -f '/home/u/.config/plasma-workspace/env/rexon-session-guard.sh'"));
        // Выполняется в общей оболочке startplasma: ни выхода, ни set -e,
        // ни переменных окружения.
        assert!(!s.contains("exit") && !s.contains("set -") && !s.contains("export"));
        let q = env_script("/opt/it's/rexon", "/x");
        assert!(q.contains(r"'/opt/it'\''s/rexon'"));
    }

    #[test]
    fn env_script_runs_sourced_without_breaking_the_shell() {
        if crate::utils::which("sh", None).is_none() {
            return;
        }
        let t = Tmp::new("script");
        let bin = t.0.join("rexon");
        let mark = t.0.join("ran");
        put(&bin, &format!("#!/bin/sh\necho \"$@\" > '{}'\n", mark.display()));
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let script = t.0.join("env/rexon-session-guard.sh");
        put(&script, &env_script(&bin.to_string_lossy(), &script.to_string_lossy()));
        let run = |s: &Path| {
            std::process::Command::new("sh")
                .arg("-c")
                .arg(format!(". '{}'; echo alive", s.display()))
                .output()
                .unwrap()
        };
        let o = run(&script);
        assert_eq!(String::from_utf8_lossy(&o.stdout), "alive\n");
        assert_eq!(read_text(&mark).as_deref(), Some("--session-guard\n"));
        // Бинарь пропал — сценарий убирает себя и всё равно не рвёт оболочку.
        std::fs::remove_file(&bin).unwrap();
        let o = run(&script);
        assert_eq!(String::from_utf8_lossy(&o.stdout), "alive\n");
        assert!(!script.exists());
    }

    #[test]
    fn env_script_sync_writes_keeps_and_removes() {
        let t = Tmp::new("sync");
        let p = t.paths();
        assert!(matches!(sync_env_script(&p, true, Some("/usr/bin/rexon")).unwrap(), ScriptChange::Write(_)));
        assert!(p.env_script().exists());
        assert_eq!(sync_env_script(&p, true, Some("/usr/bin/rexon")).unwrap(), ScriptChange::Nothing);
        assert_eq!(sync_env_script(&p, false, Some("/usr/bin/rexon")).unwrap(), ScriptChange::Remove);
        assert!(!p.env_script().exists());
        assert_eq!(sync_env_script(&p, false, None).unwrap(), ScriptChange::Nothing);
        // Звать нечего — сценарий не ставится.
        assert_eq!(sync_env_script(&p, true, None).unwrap(), ScriptChange::Nothing);
        assert_eq!(sync_env_script(&p, true, Some("rexon")).unwrap(), ScriptChange::Nothing);
    }

    #[test]
    fn plasma_session_detection() {
        assert!(plasma_session(None));
        assert!(plasma_session(Some("KDE")));
        assert!(plasma_session(Some("")));
        assert!(!plasma_session(Some("MATE")));
        assert!(!plasma_session(Some("GNOME:Unity")));
    }

    #[test]
    fn cli_route_ignores_foreign_flags() {
        assert_eq!(cli_route(&["--section=settings".to_string()], &mut |_| {}), None);
    }

    #[test]
    fn texts_are_plain() {
        assert!(turned_off_body(None).contains("standard Plasma look"));
        assert!(turned_off_body(Some("KWin crashed")).contains("KWin crashed"));
        let d = turned_off_details(true, true, &["a".into()]);
        assert!(d.contains(RESTORE_FLAG) && d.contains("• a"));
        assert!(restore_report(&RestoreDone::default()).starts_with("Nothing to restore"));
        assert!(after_rebuild_message(true).contains("next login"));
        assert!(next_login_body("“x” made the Plasma desktop crash").contains("“x” made the Plasma desktop crash"));
        let n = PendingNotice::NextLogin { reason: "r".into() };
        assert_eq!(serde_json::from_str::<PendingNotice>(&serde_json::to_string(&n).unwrap()).unwrap(), n);
    }

    // ── удаление Rexon во время отката ──

    const LAYOUT: &str = "[Containments][1]\nformfactor=0\nlocation=0\nplugin=org.kde.desktopcontainment\n\n\
[Containments][2]\nformfactor=2\nlocation=4\nplugin=org.kde.panel\n\n[Containments][2][Applets][3]\nplugin=io.gitgud.wackyideas.SevenStart\n";

    #[test]
    fn uninstall_without_fallback_only_removes_the_script() {
        let t = Tmp::new("un-none");
        let p = t.paths();
        put(&p.env_script(), "guard");
        put(&p.request_file(), "KWin crashed");
        let r = release_for_uninstall(&p).unwrap();
        assert!(!r.restored.had_saved && !r.layout_next_login);
        assert!(!p.env_script().exists() && !p.request_file().exists());
        // Ни сценария, ни копий — повтор тоже ничего не ломает.
        assert_eq!(release_for_uninstall(&p).unwrap(), UninstallRelease::default());
    }

    #[test]
    fn uninstall_during_fallback_restores_effects_now_and_layout_at_next_login() {
        let t = Tmp::new("un-active");
        let p = t.paths();
        put(&p.kwinrc(), "[Plugins]\nsmodglowEnabled=true\n");
        put(&p.appletsrc(), LAYOUT);
        put(&p.env_script(), "guard");
        let plan = FallbackPlan::whole(&parts());
        let done = fall_back_to_stock(&p, &plan, FallbackCause::Requested { reason: "x".into() }).unwrap();
        assert!(done.layout_replaced);
        let stock = std::fs::read_to_string(p.appletsrc()).unwrap();

        let r = release_for_uninstall(&p).unwrap();
        assert!(r.restored.had_saved && r.layout_next_login, "{r:?}");
        let kwinrc = std::fs::read_to_string(p.kwinrc()).unwrap();
        assert!(kwinrc.contains("smodglowEnabled=true") && !kwinrc.contains("aeroblurEnabled"), "{kwinrc}");
        // Посреди сеанса раскладка не тронута; её вернёт сценарий без Rexon.
        assert_eq!(std::fs::read_to_string(p.appletsrc()).unwrap(), stock);
        let script = std::fs::read_to_string(p.env_script()).unwrap();
        assert_eq!(script, restore_once_script(&p));
        assert!(!script.contains("exit") && !script.contains("set -e") && !script.contains("rexon --"));

        // Следующий вход: startplasma выполняет файл через `.`.
        let st = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(". {} ; echo still-running", sh_quote(&p.env_script().to_string_lossy())))
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&st.stdout).trim(), "still-running", "сценарий не обрывает оболочку");
        assert_eq!(std::fs::read_to_string(p.appletsrc()).unwrap(), LAYOUT, "раскладка человека вернулась");
        assert_eq!(std::fs::read_to_string(p.during_fallback_appletsrc()).unwrap(), stock);
        assert!(!p.env_script().exists() && !p.saved_appletsrc().exists() && !p.saved_json().exists());
    }
}