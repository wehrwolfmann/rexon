//! Порт `rexon/qt_wallpaper_engine/engine_update.py` — авто-обновление
//! upstream-проектов вкладки Wallpaper Engine (только dev-режим).
//!
//! Оригинал смешивает ЧИСТЫЕ решения (какой репо опросить, как разобрать
//! ответ GitHub API, кто новее, что показать в отчёте, куда положить файл,
//! какой argv у проб) с побочными эффектами (HTTP через `urllib`, чтение и
//! запись `~/.config/rexon/dev_upstream_state.json`, `subprocess.run`
//! busctl/AppImage, распаковка zip, `shutil`/`os.replace`). Здесь — только
//! чистая часть: вход → выход. Сети в ядре НЕТ вообще; наблюдения среды
//! (тело ответа API, stdout busctl, содержимое `plugin.toml`, наличие
//! файлов) подаются параметрами, а исполнение планов остаётся Python-шиму
//! `engine_update.py` (сверка обоих путей — `tests/engine_update_roundtrip.py`).
//!
//! Единственное исключение — [`validate_plugin_zip`]: это ЧТЕНИЕ локального
//! архива (парсинг, не мутация), и оно живёт в ядре так же, как разбор `.rxn`
//! в `manifest_v4`/`ext_utils`. Чистое ядро проверки при этом доступно
//! отдельно — [`validate_plugin_entries`].
//!
//! Две отслеживаемые цели (см. [`targets`]):
//!   * `waywallen` — приложение-интерфейс, управляемый артефакт
//!     `~/.local/share/waywallen/waywallen.AppImage`;
//!   * `owe` — open-wallpaper-engine, плагин-zip к waywallen ≥ 0.2.3,
//!     каталог `$XDG_DATA_HOME/waywallen/plugins/<id>/`.
//!
//! ## Починенные при порте баги оригинала (правило авто-починки)
//!
//! 1. **Потеря установленного плагина при сбое распаковки.** В
//!    `install_plugin_zip_fs` ветка `except` безусловно делала
//!    `_rmtree_quiet(target)`. Если ошибка случалась ДО переноса прежней
//!    версии в `.bak` (битый zip, нет места, отказ прав при `extract`),
//!    удалялся РАБОЧИЙ установленный плагин, и восстановить его было неоткуда
//!    (`moved_to_bak == False`). Здесь откатом управляет
//!    [`plugin_rollback_plan`]: `target` удаляется только если мы его сами
//!    создали.
//! 2. **`AttributeError` на испорченном стейте.** `load_state()` проверял
//!    только верхний уровень; `state["targets"]` или `targets[key]` не-объект
//!    (например список после ручной правки файла) роняли `_target_state` и
//!    `installed_version` необработанным `AttributeError`. Здесь любые
//!    не-объекты нормализуются в пустой объект ([`normalize_state`]).
//! 3. **`AttributeError` на `null` в ответе GitHub.** `r.get("tag_name", "")`
//!    возвращает `None`, если ключ ЕСТЬ и равен `null` (а не отсутствует), —
//!    дальше `a["name"].endswith(...)` падал. Здесь `null` любого поля
//!    трактуется как пустая строка ([`json_str`]).
//! 4. **`version.lstrip("v")`** снимает ВСЕ ведущие `v` (тег `vv1` → `1`,
//!    строка `version` → `ersion`). Здесь — [`version_tag_to_state`] с
//!    семантикой `removeprefix("v")`.
//! 5. **Exec-бит на скачанном zip.** `download_asset` ставил `0o755` любому
//!    временному файлу, включая плагин-архив. AppImage без `+x` не проходит
//!    валидацию, а zip исполнять незачем — [`download_spec`] выдаёт `0o755`
//!    только для AppImage, `0o644` для zip.
//! 6. **Мусор при сбое установки AppImage.** `install_appimage` не убирал за
//!    собой `dest + ".new-update"`, если падал между `copy2` и `os.replace`.
//!    План [`appimage_install_plan`] называет этот файл явно, а шим удаляет
//!    его в ветке отката.
//!
//! 7. **Установленная версия «помнилась», а не читалась.** Наша версия
//!    waywallen бралась у ЖИВОГО демона (D-Bus), а если он молчал — из
//!    стейта, куда её пишет `mark_installed`. Движок, обновлённый мимо
//!    Rexon (или обновлённый из Rexon, но ещё не перезапущенный), делал оба
//!    источника неверными: на диске 0.3.7, демон рапортует 0.3.5, в стейте
//!    0.2.6 от 26.07 (живой замер 27.08.2026). Здесь добавлен первый
//!    источник — `--version` у того бинаря, который реально будет запущен
//!    ([`binary_version_argv`], [`parse_binary_version`],
//!    [`current_version`]).
//! 8. **Стейт хранил две разные «наши версии».** `store_report` писал
//!    `latest`/`update_available`/`note`, посчитанные по ЖИВОЙ версии, но
//!    поле `installed_version` не трогал — и файл сообщал «установлено
//!    0.2.6» рядом с «актуально (0.3.5)». Теперь проверка кладёт в стейт ту
//!    же версию, с которой сравнивала.
//! 9. **Выбор release-ассета «первый, чьё имя оканчивается на суффикс».**
//!    Пока upstream клал в релиз ровно один файл, правило работало; как
//!    только рядом появились соседи, выбор поехал у ОБЕИХ целей (живая
//!    проверка 31.08.2026):
//!      * OWE v0.2.8 — четыре ассета, и под `-linux-x86_64.zip` первым
//!        подходит `debug-symbols-linux-x86_64.zip`, то есть вместо плагина
//!        качались отладочные символы. Установка потом честно падала на
//!        «в корне архива нет plugin.toml» — данные целы, но плагин
//!        перестал ставиться и обновляться вовсе;
//!      * waywallen v0.3.1 и новее — два AppImage, и под `.AppImage` первым
//!        идёт `waywallen-<v>-aarch64.AppImage`, то есть скачивался бинарь
//!        ЧУЖОЙ архитектуры.
//!    Починка — [`asset_matches`]: опознаём артефакт по тому, чем он
//!    является (`<asset_prefix>-<версия>…<asset_suffix>`), а не вычёркиваем
//!    известные довески по именам.
//!
//! Спорный квирк, оставленный как есть (совместимость отчёта):
//! `check_target` в ветке «релизов нет» ставит `update_available = True` на
//! ЛЮБОЙ новый коммит default-ветки — это не релиз, но у таких репо иного
//! признака свежести нет.

use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

// ============================================================================
// Константы
// ============================================================================

/// id OWE-плагина — из `plugin.toml` реального релиз-zip; каталог установки =
/// `plugins_dir()/<id>`.
pub const OWE_PLUGIN_ID: &str = "org.waywallen.open-wallpaper-engine";

/// Минимальная версия waywallen для OWE-как-плагина (release notes v0.2.3).
pub const OWE_MIN_WAYWALLEN: [u128; 3] = [0, 2, 3];

/// Сколько релизов/коммитов максимум тащить из API за раз.
pub const FETCH_LIMIT: usize = 30;

/// Сколько записей показывать, если базовая версия неизвестна.
pub const UNKNOWN_BASE_LIMIT: usize = 10;

/// Маркер usage-баннера waywallen — им подтверждается, что AppImage
/// распаковался и бинарь внутри запускается.
pub const USAGE_MARKER: &str = "usage: waywallen";

/// Флаг-пустышка, которым проверяется запуск AppImage.
pub const VALIDATE_PROBE_FLAG: &str = "--rexon-validate-probe";

/// Флаг запроса версии у бинаря движка.
///
/// ⚠️ Комментарий оригинала «`--version` бинарь НЕ поддерживает — печатает
/// usage (проверено живьём 2026-07-08)» устарел: waywallen 0.3.x отвечает
/// `waywallen <версия>`. Старые сборки по-прежнему печатают usage-баннер —
/// [`parse_binary_version`] распознаёт его и отдаёт «», после чего работают
/// прежние источники (демон/busctl/стейт).
pub const VERSION_FLAG: &str = "--version";

/// Таймаут запроса версии у бинаря, секунды.
pub const BINARY_VERSION_TIMEOUT_SECS: u64 = 5;

/// Таймаут пробного запуска AppImage, секунды.
pub const PROBE_TIMEOUT_SECS: u64 = 30;

/// Таймаут запроса версии по D-Bus через busctl, секунды.
pub const BUSCTL_TIMEOUT_SECS: u64 = 5;

/// Таймаут запроса к GitHub API, секунды.
pub const API_TIMEOUT_SECS: u64 = 15;

/// Таймаут скачивания ассета, секунды.
pub const DOWNLOAD_TIMEOUT_SECS: u64 = 30;

/// Размер чанка скачивания (1 МиБ) — между чанками проверяется отмена.
pub const DOWNLOAD_CHUNK: u64 = 1 << 20;

/// Заголовок `Accept` запросов к GitHub API.
pub const API_ACCEPT: &str = "application/vnd.github+json";

/// Заголовок `User-Agent` запросов к GitHub API.
pub const API_USER_AGENT: &str = "Rexon-dev";

/// Префикс каталога-стейджинга распаковки плагина.
pub const PLUGIN_STAGING_PREFIX: &str = ".rexon-plugin-install-";

/// Подкаталог стейджинга, в который распаковывается архив.
pub const PLUGIN_EXTRACT_SUBDIR: &str = "archive";

/// Суффикс каталога/файла прежней версии (для отката).
pub const BAK_SUFFIX: &str = ".bak-update";

/// Суффикс временного файла новой версии AppImage (в каталоге назначения).
pub const NEW_SUFFIX: &str = ".new-update";

// ============================================================================
// Пути
// ============================================================================

/// Управляемый AppImage движка — порт `MANAGED_APPIMAGE`.
pub fn managed_appimage(home: &str) -> String {
    format!("{home}/.local/share/waywallen/waywallen.AppImage")
}

/// Пользовательский каталог плагинов waywallen — порт `plugins_dir()`.
/// Зеркалит `standard_plugin_roots()` демона: `$XDG_DATA_HOME/waywallen/plugins`,
/// фолбэк `~/.local/share`. Пустая переменная считается незаданной (как
/// `os.environ.get(...) or ...` в Python).
pub fn plugins_dir(xdg_data_home: Option<&str>, home: &str) -> String {
    let base = match xdg_data_home {
        Some(x) if !x.is_empty() => x.to_string(),
        _ => format!("{home}/.local/share"),
    };
    format!("{base}/waywallen/plugins")
}

/// Файл стейта dev-обновлений — порт `STATE_PATH`.
pub fn state_path(home: &str) -> String {
    state_path_xdg(None, home)
}

/// То же с учётом `$XDG_CONFIG_HOME`.
pub fn state_path_xdg(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/rexon/dev_upstream_state.json",
        crate::utils::config_home(xdg_config_home, home)
    )
}

/// Временный файл атомарной записи стейта (`tmp+rename`).
pub fn state_tmp_path(state_path: &str) -> String {
    format!("{state_path}.tmp")
}

/// Путь `plugin.toml` установленного OWE-плагина.
pub fn owe_manifest_path(plugins_dir: &str) -> String {
    format!("{plugins_dir}/{OWE_PLUGIN_ID}/plugin.toml")
}

// ============================================================================
// Цели
// ============================================================================

/// Способ установки цели.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum InstallKind {
    /// Управляемый AppImage.
    #[serde(rename = "appimage")]
    AppImage,
    /// Плагин-zip к waywallen.
    #[serde(rename = "plugin_zip")]
    PluginZip,
    /// Установка не поддерживается (report-only).
    #[serde(rename = "")]
    None,
}

impl InstallKind {
    /// Строковое имя, как в поле `UpstreamTarget.kind` оригинала.
    pub fn as_str(self) -> &'static str {
        match self {
            InstallKind::AppImage => "appimage",
            InstallKind::PluginZip => "plugin_zip",
            InstallKind::None => "",
        }
    }
}

/// Один отслеживаемый upstream-проект — порт датакласса `UpstreamTarget`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpstreamTarget {
    /// id в стейте/UI.
    pub key: String,
    /// Человекочитаемое имя.
    pub title: String,
    /// slug GitHub («owner/name»).
    pub repo: String,
    /// Начало имени release-ассета — имя самого артефакта, без версии
    /// («» = не знаем, тогда проверяется только суффикс).
    pub asset_prefix: String,
    /// Суффикс release-ассета (для installable).
    pub asset_suffix: String,
    /// Управляемый локальный артефакт («» = нет).
    pub managed_path: String,
    /// Умеем ли скачать+установить.
    pub installable: bool,
    /// Способ установки.
    pub kind: InstallKind,
    /// Пояснение режима для UI.
    pub note: String,
}

/// Обе цели в порядке оригинального кортежа `TARGETS`. `home` нужен для
/// `managed_path` waywallen (в Python это модульная константа с `expanduser`).
pub fn targets(home: &str) -> Vec<UpstreamTarget> {
    vec![
        UpstreamTarget {
            key: "waywallen".into(),
            // Терминология проекта: waywallen = ПРИЛОЖЕНИЕ с интерфейсом,
            // «движок рендеров» — это OWE ниже. Не путать в подписях.
            title: crate::i18n::tr("waywallen (interface app)"),
            repo: "waywallen/waywallen".into(),
            // имя ассета: waywallen-<v>[-<билд>]-x86_64.AppImage
            // (в v0.1.3/v0.1.4 между версией и архитектурой стоял хеш
            // коммита, в v0.1.7 — номер пересборки, поэтому середина имени
            // в правило не входит).
            asset_prefix: "waywallen".into(),
            // ⚠️ архитектура в суффиксе ОБЯЗАТЕЛЬНА: с v0.3.1 upstream
            // прикладывает ещё и aarch64-сборку, а голый «.AppImage»
            // подходил и ей (и подходил ПЕРВЫМ — см. дефект 9).
            asset_suffix: "-x86_64.AppImage".into(),
            managed_path: managed_appimage(home),
            installable: true,
            kind: InstallKind::AppImage,
            note: crate::i18n::tr("managed AppImage, full update cycle"),
        },
        UpstreamTarget {
            key: "owe".into(),
            title: crate::i18n::tr("open-wallpaper-engine (renderer engine)"),
            repo: "waywallen/open-wallpaper-engine".into(),
            // имя ассета:
            // org.waywallen.open-wallpaper-engine-<v>-linux-x86_64.zip
            asset_prefix: OWE_PLUGIN_ID.into(),
            asset_suffix: "-linux-x86_64.zip".into(),
            managed_path: String::new(),
            installable: true,
            kind: InstallKind::PluginZip,
            note: crate::i18n::tr(
                "plugin zip for waywallen (v0.2.3+), installed through the daemon \
or into ~/.local/share/waywallen/plugins/",
            ),
        },
    ]
}

/// Цель по ключу (`None` — ключ не наш).
pub fn target_by_key(home: &str, key: &str) -> Option<UpstreamTarget> {
    targets(home).into_iter().find(|t| t.key == key)
}

// ============================================================================
// Записи changelog и статусы
// ============================================================================

/// Release-ассет (имя + прямой URL скачивания).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

/// Запись changelog: релиз либо (в фолбэке) коммит default-ветки.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ChangelogEntry {
    pub tag: String,
    pub name: String,
    pub published_at: String,
    pub body: String,
    pub assets: Vec<Asset>,
}

/// Результат проверки одной цели — порт датакласса `TargetStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct TargetStatus {
    pub key: String,
    pub title: String,
    /// Наша версия («» = неизвестна).
    pub installed: String,
    /// Последний тег upstream.
    pub latest: String,
    pub update_available: bool,
    /// URL ассета для установки (installable).
    pub asset_url: String,
    pub installable: bool,
    /// Записи новее нашей версии, от старых к новым.
    pub changelog: Vec<ChangelogEntry>,
    /// Человекочитаемый статус/ошибка.
    pub note: String,
    /// `false` — «не удалось проверить».
    pub ok: bool,
}

// ============================================================================
// Разбор ответов GitHub API
// ============================================================================

/// URL списка релизов репозитория.
pub fn releases_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/releases?per_page={FETCH_LIMIT}")
}

/// URL списка коммитов default-ветки (`since` — ISO-время или «»).
pub fn commits_url(repo: &str, since_iso: &str) -> String {
    let base = format!("https://api.github.com/repos/{repo}/commits?per_page={FETCH_LIMIT}");
    if since_iso.is_empty() {
        base
    } else {
        format!("{base}&since={since_iso}")
    }
}

/// Заголовки запроса к API — пары (имя, значение) в порядке оригинала.
pub fn api_headers() -> Vec<(String, String)> {
    vec![
        ("Accept".to_string(), API_ACCEPT.to_string()),
        ("User-Agent".to_string(), API_USER_AGENT.to_string()),
    ]
}

/// Строковое поле JSON-объекта. `null` и отсутствие ключа дают «» —
/// починка бага оригинала (`dict.get(k, "")` отдаёт `None` на явный `null`,
/// и дальше `.endswith`/`.strip` падали `AttributeError`).
fn json_str(obj: &Map<String, Value>, key: &str) -> String {
    match obj.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Python-truthiness JSON-значения (для `draft`/`prerelease`).
fn json_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Разобрать ответ `GET /repos/{repo}/releases` — порт `_fetch_releases`.
///
/// Отбрасывает draft/prerelease, порядок сохраняется (API отдаёт от новых к
/// старым). Не-массив на верхнем уровне — пустой список (как в оригинале).
/// `Err` — тело не разобралось как JSON (в Python это исключение `json.loads`,
/// которое ловит `check_target`).
pub fn parse_releases(body: &str) -> Result<Vec<ChangelogEntry>, String> {
    let data: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let Some(items) = data.as_array() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for item in items {
        let Some(obj) = item.as_object() else { continue };
        if json_truthy(obj.get("draft")) || json_truthy(obj.get("prerelease")) {
            continue;
        }
        let tag = json_str(obj, "tag_name");
        let name = json_str(obj, "name");
        let assets = obj
            .get("assets")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_object)
                    .map(|a| Asset {
                        name: json_str(a, "name"),
                        url: json_str(a, "browser_download_url"),
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(ChangelogEntry {
            name: if name.is_empty() { tag.clone() } else { name },
            tag,
            published_at: json_str(obj, "published_at"),
            body: json_str(obj, "body").trim().to_string(),
            assets,
        });
    }
    Ok(out)
}

/// Разобрать ответ `GET /repos/{repo}/commits` — порт `_fetch_commits`.
/// Коммиты приводятся к формату changelog: `tag` = первые 9 символов sha,
/// `name` = первая строка сообщения, `body` = остаток.
pub fn parse_commits(body: &str) -> Result<Vec<ChangelogEntry>, String> {
    let data: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let Some(items) = data.as_array() else {
        return Ok(Vec::new());
    };
    let empty = Map::new();
    let mut out = Vec::new();
    for item in items {
        let Some(obj) = item.as_object() else { continue };
        let commit = obj.get("commit").and_then(Value::as_object).unwrap_or(&empty);
        let message = json_str(commit, "message");
        let (head, tail) = match message.split_once('\n') {
            Some((h, t)) => (h.to_string(), t.trim().to_string()),
            None => (message.clone(), String::new()),
        };
        let sha = json_str(obj, "sha");
        let committer = commit
            .get("committer")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        out.push(ChangelogEntry {
            tag: sha.chars().take(9).collect(),
            name: head,
            published_at: json_str(committer, "date"),
            body: tail,
            assets: Vec::new(),
        });
    }
    Ok(out)
}

// ============================================================================
// Версии
// ============================================================================

/// `'v0.2.4'` / `'0.2.4-x86_64'` → `[0, 2, 4]` — порт `_ver_tuple`.
///
/// ⚠️ Осознанное сужение: Python `re.findall(r"\d+")` матчит и не-ASCII
/// цифры Unicode; теги/версии waywallen — ASCII, поэтому здесь берутся
/// только ASCII-цифры. Числа шире `u128` насыщаются его максимумом
/// (в Python целые не ограничены, но такие «версии» не встречаются).
pub fn ver_tuple(v: &str) -> Vec<u128> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in v.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            out.push(cur.parse::<u128>().unwrap_or(u128::MAX));
            cur.clear();
        }
    }
    if !cur.is_empty() {
        out.push(cur.parse::<u128>().unwrap_or(u128::MAX));
    }
    out
}

/// Тег релиза → строка для стейта. Порт `version.lstrip("v")` с починенной
/// семантикой: снимается ОДИН ведущий `v` (`removeprefix`), а не все подряд.
pub fn version_tag_to_state(version: &str) -> String {
    version.strip_prefix('v').unwrap_or(version).to_string()
}

/// argv запроса версии у бинаря движка: `<бинарь> --version`.
///
/// Бинарь берётся тем же поиском, что и для запуска демона
/// ([`crate::we_client::find_daemon_binary`]), — спрашиваем версию РОВНО у
/// того кандидата, который реально будет запущен.
pub fn binary_version_argv(bin: &str) -> Vec<String> {
    vec![bin.to_string(), VERSION_FLAG.to_string()]
}

/// Разобрать вывод `waywallen --version` (`waywallen 0.3.7`) → `0.3.7`.
///
/// Правила намеренно узкие, чтобы usage-баннер старых сборок не был принят
/// за версию:
///   * вывод, содержащий [`USAGE_MARKER`], версией не считается вовсе;
///   * годен первый токен вида `[v]N.N[.N…]` — обязательно с точкой (иначе
///     любое число из справки, вроде `--fps 60`, стало бы «версией»);
///   * хвост после чисел допускается только через `-`/`+`
///     (`0.3.7-x86_64`) и в результат не входит.
///
/// Ничего похожего на версию нет → «» (источник считается недоступным).
pub fn parse_binary_version(stdout: &str) -> String {
    let text = stdout.trim();
    if text.is_empty() || text.to_lowercase().contains(USAGE_MARKER) {
        return String::new();
    }
    for tok in text.split_whitespace() {
        let cand = tok.strip_prefix('v').unwrap_or(tok);
        let head: String = cand
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let tail = &cand[head.len()..];
        let plausible = head.contains('.')
            && head.starts_with(|c: char| c.is_ascii_digit())
            && !head.ends_with('.')
            && (tail.is_empty() || tail.starts_with('-') || tail.starts_with('+'));
        if plausible {
            return head;
        }
    }
    String::new()
}

/// argv запроса версии демона по D-Bus через busctl — порт `current_version`.
pub fn busctl_version_argv() -> Vec<String> {
    [
        "busctl",
        "--user",
        "get-property",
        crate::waywallen_client::BUS_NAME,
        crate::waywallen_client::OBJECT_PATH,
        crate::waywallen_client::IFACE,
        "Version",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Разобрать вывод busctl (`s "0.2.2"`) — порт разбора в `current_version`.
/// Нет кавычек — версия неизвестна («»).
pub fn parse_busctl_version(stdout: &str) -> String {
    let out = stdout.trim();
    let mut parts = out.split('"');
    match (parts.next(), parts.next()) {
        (Some(_), Some(v)) => v.to_string(),
        _ => String::new(),
    }
}

/// Версия движка waywallen — порт `current_version` без побочных эффектов.
///
/// Порядок источников — от самого правдивого к самому старому:
/// * `binary_version` — вывод `--version` у ТОГО бинаря, который реально
///   будет запущен (разбор — [`parse_binary_version`]). Это единственный
///   источник, который знает, что лежит на диске ПРЯМО СЕЙЧАС;
/// * `daemon_version` — свойство `Version` живого демона по D-Bus (Qt-путь);
///   подаётся `Some(..)` только если демон отвечает и версия непустая.
///   ⚠️ Это версия РАБОТАЮЩЕГО процесса: после обновления AppImage он ещё
///   долго рапортует старое число (живой замер 27.08.2026: на диске 0.3.7,
///   демон по D-Bus — 0.3.5), поэтому бинарь идёт ПЕРЕД ним;
/// * `busctl_stdout` — stdout фолбэк-вызова busctl (`None`, если вызов
///   не делался/упал);
/// * `state_installed` — версия, записанная при последней установке. Самый
///   слабый источник: если движок обновляли не из Rexon, он врёт бессрочно
///   (живой замер: в стейте 0.2.6 от 26.07 при 0.3.7 на диске).
pub fn current_version(
    binary_version: Option<&str>,
    daemon_version: Option<&str>,
    busctl_stdout: Option<&str>,
    state_installed: &str,
) -> String {
    if let Some(v) = binary_version {
        if !v.is_empty() {
            return v.to_string();
        }
    }
    if let Some(v) = daemon_version {
        if !v.is_empty() {
            return v.to_string();
        }
    }
    if let Some(out) = busctl_stdout {
        let v = parse_busctl_version(out);
        if !v.is_empty() {
            return v;
        }
    }
    state_installed.to_string()
}

/// Версия OWE-плагина из текста `plugin.toml` — порт `owe_installed_version`.
/// Битый TOML/нет секции `[plugin]`/нет `version` → «».
pub fn owe_version_from_manifest(toml_text: &str) -> String {
    let Ok(v) = toml_text.parse::<toml::Value>() else {
        return String::new();
    };
    v.get("plugin")
        .and_then(|p| p.get("version"))
        .map(toml_scalar_str)
        .unwrap_or_default()
}

/// `str(value)` в терминах Python для скалярных значений TOML.
fn toml_scalar_str(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => format_py_float(*f),
        toml::Value::Boolean(true) => "True".to_string(),
        toml::Value::Boolean(false) => "False".to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        // Списки/таблицы в этих полях не встречаются; строковое представление
        // Python для них не воспроизводим — отдаём пусто.
        _ => String::new(),
    }
}

/// `str(float)` как в Python (`repr` кратчайшего представления).
fn format_py_float(f: f64) -> String {
    if f == f.trunc() && f.is_finite() && f.abs() < 1e16 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// Наша версия цели — порт `installed_version`.
///
/// * `waywallen` → живая версия движка (`current_version`, где первым
///   источником идёт `--version` бинаря);
/// * `owe` → правда с ФС (`plugin.toml`) → стейт установки → изученная;
/// * прочее → изученная → установленная.
pub fn installed_version(
    key: &str,
    current_version: &str,
    owe_fs_version: &str,
    state_installed: &str,
    state_seen: &str,
) -> String {
    match key {
        "waywallen" => current_version.to_string(),
        "owe" => first_non_empty(&[owe_fs_version, state_installed, state_seen]),
        _ => first_non_empty(&[state_seen, state_installed]),
    }
}

fn first_non_empty(candidates: &[&str]) -> String {
    candidates
        .iter()
        .find(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

// ============================================================================
// Стейт ~/.config/rexon/dev_upstream_state.json
// ============================================================================

/// Разобрать стейт — порт `load_state`. Отсутствие файла, битый JSON, не-объект
/// на верхнем уровне дают пустой стейт (оригинал не падал только на верхнем
/// уровне; вложенные не-объекты чинятся в [`normalize_state`]).
pub fn load_state(text: Option<&str>) -> Map<String, Value> {
    let Some(text) = text else {
        return Map::new();
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// Привести `state["targets"]` и его записи к объектам — починка бага
/// оригинала: `state.setdefault("targets", {}).setdefault(key, {})` роняло
/// `AttributeError`, если в файле лежал список/строка.
pub fn normalize_state(state: &mut Map<String, Value>) {
    if !state.get("targets").map(Value::is_object).unwrap_or(false) {
        state.insert("targets".into(), Value::Object(Map::new()));
    }
    let targets = state
        .get_mut("targets")
        .and_then(Value::as_object_mut)
        .expect("targets только что нормализован в объект");
    let broken: Vec<String> = targets
        .iter()
        .filter(|(_, v)| !v.is_object())
        .map(|(k, _)| k.clone())
        .collect();
    for k in broken {
        targets.insert(k, Value::Object(Map::new()));
    }
}

/// Запись цели в стейте (создаётся при отсутствии) — порт `_target_state`.
fn target_state<'a>(state: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    normalize_state(state);
    let targets = state
        .get_mut("targets")
        .and_then(Value::as_object_mut)
        .expect("targets нормализован");
    targets
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    targets
        .get_mut(key)
        .and_then(Value::as_object_mut)
        .expect("запись цели нормализована")
}

/// Прочитать строковое поле записи цели (не создавая запись).
pub fn state_field(state: &Map<String, Value>, key: &str, field: &str) -> String {
    state
        .get("targets")
        .and_then(Value::as_object)
        .and_then(|t| t.get(key))
        .and_then(Value::as_object)
        .map(|e| match e.get(field) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        })
        .unwrap_or_default()
}

/// Зафиксировать установленную версию цели — порт `mark_installed`.
pub fn mark_installed(state: &mut Map<String, Value>, key: &str, version: &str, now_iso: &str) {
    let ts = target_state(state, key);
    ts.insert("installed_version".into(), Value::from(version));
    ts.insert("installed_at".into(), Value::from(now_iso));
}

/// Отметить версию изученной — порт `mark_seen`.
pub fn mark_seen(state: &mut Map<String, Value>, key: &str, version: &str, now_iso: &str) {
    let ts = target_state(state, key);
    ts.insert("seen_version".into(), Value::from(version));
    ts.insert("seen_at".into(), Value::from(now_iso));
}

/// Сохранить отчёт по цели в стейт — порт `_store_report`. Ассеты в отчёт не
/// попадают (шум), тела релизов — попадают.
pub fn store_report(
    state: &mut Map<String, Value>,
    key: &str,
    st: &TargetStatus,
    now_iso: &str,
) {
    let report: Vec<Value> = st
        .changelog
        .iter()
        .map(|r| {
            let mut m = Map::new();
            m.insert("tag".into(), Value::from(r.tag.clone()));
            m.insert("name".into(), Value::from(r.name.clone()));
            m.insert("published_at".into(), Value::from(r.published_at.clone()));
            m.insert("body".into(), Value::from(r.body.clone()));
            Value::Object(m)
        })
        .collect();
    let ts = target_state(state, key);
    ts.insert("last_check".into(), Value::from(now_iso));
    // Установленная версия, с которой СРАВНИВАЛИ, — в тот же файл: иначе
    // `installed_version` остаётся с момента последней установки из Rexon и
    // расходится с `note`/`update_available`, посчитанными по живой версии
    // (замер 27.08.2026: в файле 0.2.6, а note «актуально (0.3.5)»).
    // Пустую версию не пишем — «не смогли определить» не должно затирать
    // то, что записал `mark_installed`.
    if !st.installed.is_empty() {
        ts.insert("installed_version".into(), Value::from(st.installed.clone()));
    }
    ts.insert("latest".into(), Value::from(st.latest.clone()));
    ts.insert("update_available".into(), Value::from(st.update_available));
    ts.insert("note".into(), Value::from(st.note.clone()));
    ts.insert("report".into(), Value::Array(report));
}

/// Текст файла стейта — как `json.dump(..., ensure_ascii=False, indent=1)`:
/// отступ в один пробел, кириллица не экранируется, перевода строки в конце
/// нет.
pub fn serialize_state(state: &Map<String, Value>) -> String {
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    if serde::Serialize::serialize(&Value::Object(state.clone()), &mut ser).is_err() {
        return "{}".to_string();
    }
    String::from_utf8(buf).unwrap_or_else(|_| "{}".to_string())
}

/// Метка времени стейта — порт `_now_iso` (`%Y-%m-%dT%H:%M:%SZ`, UTC).
pub fn now_iso(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let rest = unix_secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rest / 3600, (rest % 3600) / 60, rest % 60);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Дни от эпохи Unix → (год, месяц, день) по григорианскому календарю.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ============================================================================
// Проверка целей
// ============================================================================

/// Чем закончился сетевой поход за данными цели (исполняется вне ядра).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    /// Запрос релизов не удался (текст исключения Python).
    ReleasesFailed(String),
    /// Релизы получены. Пустой список = «релизов нет» и шим ДОЛЖЕН был
    /// сходить за коммитами; здесь пустой список трактуется так же, как
    /// `Commits(vec![])` (нет ни релизов, ни новых коммитов).
    Releases(Vec<ChangelogEntry>),
    /// Релизов нет, коммиты default-ветки получены.
    Commits(Vec<ChangelogEntry>),
    /// Релизов нет, запрос коммитов не удался (текст исключения Python).
    CommitsFailed(String),
}

/// Подходит ли имя release-ассета нашей цели.
///
/// ⚠️ Починка дефекта 9 ОРИГИНАЛА (см. шапку модуля). Оригинал брал первый
/// ассет, чьё имя `endswith(asset_suffix)`, и ломался, как только upstream
/// добавлял в релиз ещё файлы: у OWE выбирались `debug-symbols-…`, у
/// waywallen — сборка под aarch64.
///
/// Почему правило именно такое. Вычёркивать известные довески по именам
/// («не брать debug-symbols») бессмысленно: завтра появятся `sources.zip`,
/// `sha256sums.txt` или `-musl-`-вариант, и мы вернёмся сюда же. Поэтому
/// опознаём не «чужое», а СВОЁ: у обеих целей во ВСЕХ релизах (проверено
/// живьём по всем 18 релизам OWE и 23 waywallen, 31.08.2026) настоящий
/// артефакт назван по одной схеме
///
/// ```text
/// <имя артефакта>-<версия>[-<что угодно>]-<архитектура><расширение>
///     org.waywallen.open-wallpaper-engine-0.2.8-linux-x86_64.zip
///     waywallen-0.3.7-x86_64.AppImage
///     waywallen-0.1.4-g8233394-x86_64.AppImage
/// ```
///
/// то есть имя НАЧИНАЕТСЯ с имени артефакта, сразу за ним через дефис идёт
/// версия (цифра), а КОНЧАЕТСЯ имя архитектурой и расширением. Оба конца
/// закреплены — середина (хеш коммита, номер пересборки) свободна.
/// Побочные файлы релиза мимо этой схемы: `debug-symbols-…` не начинается
/// с имени артефакта, `…-aarch64.zip` не кончается нашим суффиксом.
///
/// Пустой `asset_prefix` = поведение оригинала (только суффикс): так
/// остаются рабочими цели, у которых схемы имён мы не знаем.
pub fn asset_matches(name: &str, prefix: &str, suffix: &str) -> bool {
    if !name.ends_with(suffix) {
        return false;
    }
    if prefix.is_empty() {
        return true;
    }
    let Some(rest) = name
        .strip_prefix(prefix)
        .and_then(|r| r.strip_prefix('-'))
    else {
        return false;
    };
    // за именем артефакта сразу начинается версия, а суффикс — отдельный
    // хвост, а не кусок этой же версии (`rest` длиннее суффикса)
    rest.starts_with(|c: char| c.is_ascii_digit()) && rest.len() > suffix.len()
}

/// Проверить одну цель — порт `check_target` без сети.
///
/// `installed` — результат [`installed_version`]; `fetch` — что вернул поход
/// в API. Исключений наружу нет: любая ошибка превращается в `ok = false`
/// с внятной `note`.
pub fn check_target(
    target: &UpstreamTarget,
    installed: &str,
    fetch: &FetchOutcome,
) -> TargetStatus {
    let mut st = TargetStatus {
        key: target.key.clone(),
        title: target.title.clone(),
        installable: target.installable,
        installed: installed.to_string(),
        ..Default::default()
    };

    let releases: &[ChangelogEntry] = match fetch {
        FetchOutcome::ReleasesFailed(e) => {
            st.note = crate::i18n::trf("could not check: %1", &[&e]);
            return st;
        }
        FetchOutcome::CommitsFailed(e) => {
            st.note = crate::i18n::trf("no releases, and commits are unavailable: %1", &[&e]);
            return st;
        }
        FetchOutcome::Commits(commits) => {
            st.ok = true;
            st.changelog = commits.iter().rev().cloned().collect();
            st.latest = commits.first().map(|c| c.tag.clone()).unwrap_or_default();
            st.update_available = !commits.is_empty();
            st.note = if commits.is_empty() {
                crate::i18n::tr("no releases and no new commits")
            } else {
                crate::i18n::tr("no releases — showing commits of the default branch")
            };
            return st;
        }
        FetchOutcome::Releases(rels) if rels.is_empty() => {
            // Шим до сюда не доводит (пустой список = поход за коммитами),
            // но защищаемся: пустота эквивалентна «коммитов нет».
            st.ok = true;
            st.update_available = false;
            st.note = crate::i18n::tr("no releases and no new commits");
            return st;
        }
        FetchOutcome::Releases(rels) => rels,
    };

    st.ok = true;
    st.latest = releases[0].tag.clone();
    let ours = ver_tuple(&st.installed);
    let newer: Vec<ChangelogEntry> = if ours.is_empty() {
        // базовая версия неизвестна — честно показываем последние релизы
        let shown: Vec<ChangelogEntry> =
            releases.iter().take(UNKNOWN_BASE_LIMIT).cloned().collect();
        st.update_available = true;
        st.note = crate::i18n::trf(
            "our version is unknown — showing the last %1 release(s), newest %2",
            &[&shown.len(), &st.latest],
        );
        shown
    } else {
        let shown: Vec<ChangelogEntry> = releases
            .iter()
            .filter(|r| ver_tuple(&r.tag) > ours)
            .cloned()
            .collect();
        st.update_available = !shown.is_empty();
        st.note = if shown.is_empty() {
            crate::i18n::trf("up to date (%1)", &[&st.installed])
        } else {
            crate::i18n::trf("%1 available (we have %2)", &[&st.latest, &st.installed])
        };
        shown
    };
    // от старых к новым — так удобнее читать/внедрять по порядку
    st.changelog = newer.into_iter().rev().collect();

    // URL ассета для установки — из САМОГО нового релиза
    if target.installable && st.update_available {
        for a in &releases[0].assets {
            if asset_matches(&a.name, &target.asset_prefix, &target.asset_suffix) {
                st.asset_url = a.url.clone();
                break;
            }
        }
        if st.asset_url.is_empty() {
            st.note = crate::i18n::trf("%1; the release has no matching asset", &[&st.note]);
        }
    }
    st
}

/// Плоский текст отчёта — порт `render_changelog_text` (кнопка «копировать
/// всё»). Формат стабильный: по цели заголовок диапазона, по релизу — тег,
/// имя, дата и полные release notes.
pub fn render_changelog_text(statuses: &[TargetStatus]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for st in statuses {
        let base = if st.installed.is_empty() {
            crate::i18n::tr("(unknown)")
        } else {
            st.installed.clone()
        };
        let latest = if st.latest.is_empty() { "?" } else { &st.latest };
        lines.push(format!("═══ {} — {base} → {latest} ═══", st.title));
        lines.push(crate::i18n::trf("Status: %1", &[&st.note]));
        for rel in &st.changelog {
            let date: String = rel.published_at.chars().take(10).collect();
            lines.push(String::new());
            lines.push(format!("— {} ({}) {date}", rel.tag, rel.name));
            let body = rel.body.trim();
            lines.push(if body.is_empty() {
                crate::i18n::tr("(no description)")
            } else {
                body.to_string()
            });
        }
        lines.push(String::new());
    }
    lines.join("\n").trim().to_string()
}

// ============================================================================
// Скачивание
// ============================================================================

/// Что качаем — определяет имя временного файла и права на него.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadKind {
    /// AppImage движка (`download_asset` с дефолтными аргументами).
    AppImage,
    /// Плагин-zip OWE (`download_plugin_zip`).
    PluginZip,
}

/// Параметры временного файла скачивания.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadSpec {
    pub prefix: String,
    pub suffix: String,
    /// Права на готовый файл.
    ///
    /// ⚠️ Починка оригинала: `download_asset` ставил `0o755` ЛЮБОМУ файлу,
    /// включая плагин-архив. AppImage без `+x` не проходит `validate_appimage`,
    /// а zip исполнять незачем — ему `0o644`.
    pub mode: u32,
    pub chunk: u64,
    pub timeout_secs: u64,
}

/// Параметры скачивания по типу артефакта — порт дефолтов `download_asset`
/// и `download_plugin_zip`.
pub fn download_spec(kind: DownloadKind) -> DownloadSpec {
    match kind {
        DownloadKind::AppImage => DownloadSpec {
            prefix: "waywallen-new-".into(),
            suffix: ".AppImage".into(),
            mode: 0o755,
            chunk: DOWNLOAD_CHUNK,
            timeout_secs: DOWNLOAD_TIMEOUT_SECS,
        },
        DownloadKind::PluginZip => DownloadSpec {
            prefix: "owe-plugin-".into(),
            suffix: ".zip".into(),
            mode: 0o644,
            chunk: DOWNLOAD_CHUNK,
            timeout_secs: DOWNLOAD_TIMEOUT_SECS,
        },
    }
}

/// Текст прогресса скачивания в МиБ — порт `done_b >> 20 / total_b >> 20`
/// воркеров окна (сдвиг, а не деление: так же округляет вниз).
pub fn progress_mib(done_bytes: u64, total_bytes: u64) -> (u64, u64) {
    (done_bytes >> 20, total_bytes >> 20)
}

// ============================================================================
// Валидация и установка AppImage
// ============================================================================

/// argv пробного запуска AppImage — порт `validate_appimage`.
pub fn appimage_probe_argv(path: &str) -> Vec<String> {
    vec![path.to_string(), VALIDATE_PROBE_FLAG.to_string()]
}

/// Вердикт валидации скачанного AppImage — порт `validate_appimage`.
///
/// * `is_file` / `is_executable` — наблюдения `os.path.isfile` / `os.access`;
/// * `spawn_error` — текст исключения, если запуск вообще не состоялся;
/// * `combined_output` — `stdout + stderr` пробного запуска.
pub fn validate_appimage_verdict(
    is_file: bool,
    is_executable: bool,
    spawn_error: Option<&str>,
    combined_output: &str,
) -> (bool, String) {
    if !(is_file && is_executable) {
        return (false, crate::i18n::tr("file not found or not executable"));
    }
    if let Some(e) = spawn_error {
        return (false, crate::i18n::trf("does not start: %1", &[&e]));
    }
    if combined_output.contains(USAGE_MARKER) {
        return (true, crate::i18n::tr("AppImage starts (usage banner received)"));
    }
    (
        false,
        crate::i18n::tr("AppImage did not print the expected usage banner — the file may be corrupt"),
    )
}

/// План подмены управляемого AppImage — порт `install_appimage`.
///
/// ⚠️ Писать поверх `dest` нельзя: демон исполняет этот файл, запись даёт
/// `[Errno 26] Text file busy`. Поэтому копия кладётся во временное имя В ТОЙ
/// ЖЕ папке и атомарно переименовывается — rename поверх работающего бинаря
/// легален (старый inode доисполняется, новый подхватится при перезапуске).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppimageInstallPlan {
    /// Каталог назначения (создать при отсутствии).
    pub dest_dir: String,
    /// Управляемый AppImage.
    pub dest: String,
    /// Копия прежней версии для отката.
    pub bak: String,
    /// Временное имя новой версии рядом с `dest`.
    pub tmp: String,
    /// Права на новый файл.
    pub mode: u32,
    /// Версия для стейта («» — не записывать).
    pub state_version: String,
}

/// Построить план установки AppImage. `version` — тег релиза («» = не писать
/// в стейт).
pub fn appimage_install_plan(home: &str, version: &str) -> AppimageInstallPlan {
    appimage_install_plan_for_dest(&managed_appimage(home), version)
}

/// То же от явного пути управляемого артефакта — нужно шиму, где
/// `MANAGED_APPIMAGE` может быть переопределён (dry-run/тесты).
pub fn appimage_install_plan_for_dest(dest: &str, version: &str) -> AppimageInstallPlan {
    let dest_dir = match dest.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => dest[..i].to_string(),
        None => String::new(),
    };
    AppimageInstallPlan {
        dest_dir,
        bak: format!("{dest}{BAK_SUFFIX}"),
        tmp: format!("{dest}{NEW_SUFFIX}"),
        mode: 0o755,
        state_version: if version.is_empty() {
            String::new()
        } else {
            version_tag_to_state(version)
        },
        dest: dest.to_string(),
    }
}

/// Сообщение об успешной установке AppImage.
pub fn appimage_install_ok_message(dest: &str) -> String {
    crate::i18n::trf(
        "installed to %1; the running daemon keeps the old version until it stops — \
restart the wallpaper engine to pick up the new one",
        &[&dest],
    )
}

/// Как подменить AppImage движка, не уронив работающего демона.
///
/// ⛔ Подмена под живым демоном — известная грабля (27.08.2026): интерфейс
/// новой версии цепляется к старому демону, а при остановке AppImage-обёртка
/// размонтирует образ из-под ещё живого процесса — SIGBUS. Порядок поэтому
/// один: остановить → подменить → поднять.
///
/// Останавливаем только ЧЕСТНЫМИ путями:
///   * служба автозапуска [`crate::we_client::AUTOSTART_UNIT`] активна —
///     `systemctl --user stop`, после подмены `start`. Юнит несёт
///     `KillMode=mixed`, и systemd гасит цепочку в правильном порядке;
///   * демон жив, но не под нашей службой (его поднял Rexon при открытии
///     вкладки) — просим его выйти его же методом `Quit` по D-Bus: он
///     завершается сам, и обёртка снимает образ уже после него. Поднимет его
///     снова вкладка — своим обычным автозапуском при следующем обращении.
/// ⛔ Ни `kill`, ни поиска процессов по имени здесь нет и быть не должно
/// (обёртка и демон — два звена одной цепочки, убийство обёртки = SIGBUS).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineSwapPlan {
    /// argv остановки службы ДО подмены.
    pub stop_argv: Option<Vec<String>>,
    /// Попросить демона выйти методом `Quit` (служба не наша или её нет).
    pub quit_daemon: bool,
    /// argv подъёма службы ПОСЛЕ подмены (и после отката — тоже: без
    /// обоев человек не должен остаться ни в каком исходе).
    pub start_argv: Option<Vec<String>>,
}

/// argv `systemctl --user is-active --quiet <служба автозапуска>`: код 0 —
/// служба работает.
pub fn engine_unit_active_argv() -> Vec<String> {
    crate::we_client::systemctl_user_argv(&[
        "is-active",
        "--quiet",
        crate::we_client::AUTOSTART_UNIT,
    ])
}

/// argv вызова `Quit` у демона по D-Bus.
pub fn busctl_quit_argv() -> Vec<String> {
    [
        "busctl",
        "--user",
        "call",
        crate::waywallen_client::BUS_NAME,
        crate::waywallen_client::OBJECT_PATH,
        crate::waywallen_client::IFACE,
        "Quit",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Сколько раз и с каким шагом ждать ухода демона после `Quit`, мс.
pub const QUIT_WAIT_STEPS: u32 = 20;
/// Шаг ожидания ухода демона после `Quit`, мс.
pub const QUIT_WAIT_STEP_MS: u64 = 250;

/// Построить [`EngineSwapPlan`]. `unit_active` — ответ
/// [`engine_unit_active_argv`], `daemon_running` — демон отвечает на шине.
pub fn engine_swap_plan(unit_active: bool, daemon_running: bool) -> EngineSwapPlan {
    let unit = crate::we_client::AUTOSTART_UNIT;
    if unit_active {
        return EngineSwapPlan {
            stop_argv: Some(crate::we_client::systemctl_user_argv(&["stop", unit])),
            quit_daemon: false,
            start_argv: Some(crate::we_client::systemctl_user_argv(&["start", unit])),
        };
    }
    EngineSwapPlan {
        stop_argv: None,
        quit_daemon: daemon_running,
        start_argv: None,
    }
}

/// Сообщение об успешной установке, когда движок перезапущен службой.
pub fn appimage_install_restarted_message(dest: &str) -> String {
    crate::i18n::trf(
        "Installed to %1; the wallpaper engine was restarted with the new version.",
        &[&dest],
    )
}

/// Сообщение об успешной установке, когда движок остановлен и поднимется
/// при следующем обращении вкладки.
pub fn appimage_install_quit_message(dest: &str) -> String {
    crate::i18n::trf(
        "Installed to %1; the wallpaper engine was stopped and will start with \
the new version the next time the Wallpaper Engine tab needs it.",
        &[&dest],
    )
}

/// Сообщение: демон не ушёл по просьбе — подмену НЕ делали.
pub fn appimage_install_busy_message() -> String {
    crate::i18n::tr(
        "The wallpaper engine did not stop, so its file was not replaced. \
Close Rexon, stop the engine and try again.",
    )
}

/// Сообщение об ошибке установки AppImage (после отката).
pub fn appimage_install_fail_message(err: &str) -> String {
    crate::i18n::trf("installation failed, rolled back: %1", &[&err])
}

// ============================================================================
// Движок из пакетов дистрибутива: обновляет система, а не Rexon
// ============================================================================

/// Отказ ставить вторую копию цели, которую поставил пакетный менеджер.
///
/// С 30.10.2026 пакет Rexon берёт движок зависимостями (`waywallen`,
/// `open-wallpaper-engine`). Скачанная этой страницей копия легла бы рядом
/// (`~/.local/share/waywallen/…`), и, будучи новее, перекрыла бы пакетную:
/// демон берёт новейшую копию плагина, Rexon — новейший движок
/// ([`crate::we_client::prefer_managed_over_bundled`]). Дальше человек
/// застревает на этой копии — обновления системы её не трогают, а базу
/// движка она уже переписала под себя. Поэтому для целей, стоящих пакетом
/// дистрибутива, страница не качает ничего и говорит, кто их обновляет.
///
/// `key` — ключ цели ([`targets`]); `system_engine` — на диске демон пакета
/// дистрибутива ([`crate::we_client::SYSTEM_DAEMON`]), `system_owe` — его
/// плагин OWE (`/usr/share/waywallen/plugins/<OWE_PLUGIN_ID>`). `None` —
/// ставить можно.
pub fn system_update_refusal(key: &str, system_engine: bool, system_owe: bool) -> Option<String> {
    match key {
        "waywallen" if system_engine => Some(crate::i18n::tr(
            "The waywallen wallpaper engine on this computer is installed as a system \
package (waywallen). It is updated together with the rest of the system, so Rexon \
does not download a second copy of it. Install system updates, and the engine will \
be updated too.",
        )),
        "owe" if system_owe => Some(crate::i18n::tr(
            "Open Wallpaper Engine on this computer is installed as a system package \
(open-wallpaper-engine). It is updated together with the rest of the system, so \
Rexon does not download a second copy of it. Install system updates, and the plugin \
will be updated too.",
        )),
        _ => None,
    }
}

// ============================================================================
// OWE-плагин: гейт совместимости, валидация, установка
// ============================================================================

/// Совместим ли установленный waywallen с OWE-как-плагином — порт
/// `waywallen_meets_owe_min`. Пустая версия → `false` с пояснением.
pub fn waywallen_meets_owe_min(version: &str) -> (bool, String) {
    if version.is_empty() {
        return (
            false,
            crate::i18n::tr(
                "waywallen version unknown (the daemon is not running) — \
cannot check plugin compatibility",
            ),
        );
    }
    if ver_tuple(version).as_slice() < OWE_MIN_WAYWALLEN.as_slice() {
        let need = OWE_MIN_WAYWALLEN
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(".");
        return (
            false,
            crate::i18n::trf(
                "waywallen %1 is too old for the OWE plugin (needs ≥ %2: before \
that OWE is built into the AppImage, and a user copy would duplicate it) — \
update waywallen first",
                &[&version, &need],
            ),
        );
    }
    (true, crate::i18n::trf("waywallen %1 is compatible", &[&version]))
}

/// Разобранный манифест плагина — порт словаря `info` из `validate_plugin_zip`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// Итог проверки плагин-архива — порт кортежа `(ok, msg, info)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginValidation {
    pub ok: bool,
    pub msg: String,
    pub info: PluginInfo,
}

impl PluginValidation {
    fn fail(msg: impl Into<String>) -> Self {
        PluginValidation {
            ok: false,
            msg: msg.into(),
            info: PluginInfo::default(),
        }
    }
}

/// `repr()` строки как в CPython: одинарные кавычки, но двойные, если внутри
/// есть `'` и нет `"` (нужно для сообщения «небезопасный plugin id: …»).
fn py_repr(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

/// Чистое ядро проверки плагин-архива — порт тела `validate_plugin_zip`.
///
/// Требования зеркалят `inspect_zip()`/`install_zip()` демона
/// (`src/plugin/installer.rs`): `plugin.toml` В КОРНЕ, `[plugin].id` непустой
/// и без разделителей пути, пути внутри архива безопасны. `files.txt` демон
/// лишь warn'ит — здесь тоже не блокер, попадает в сообщение.
///
/// `names` — `ZipFile.namelist()`; `manifest_toml` — содержимое `plugin.toml`
/// (`None`, если файла в архиве нет).
pub fn validate_plugin_entries(
    names: &[String],
    manifest_toml: Option<&str>,
) -> PluginValidation {
    for n in names {
        let p = n.replace('\\', "/");
        if p.starts_with('/') || p.split('/').any(|seg| seg == "..") {
            return PluginValidation::fail(crate::i18n::trf("unsafe path in the archive: %1", &[n]));
        }
    }
    if !names.iter().any(|n| n == "plugin.toml") {
        return PluginValidation::fail(crate::i18n::tr("no plugin.toml at the archive root"));
    }
    let Some(text) = manifest_toml else {
        return PluginValidation::fail(crate::i18n::tr("no plugin.toml at the archive root"));
    };
    let manifest = match text.parse::<toml::Value>() {
        Ok(v) => v,
        Err(e) => return PluginValidation::fail(crate::i18n::trf("the archive cannot be read: %1", &[&e])),
    };
    let get = |k: &str| {
        manifest
            .get("plugin")
            .and_then(|p| p.get(k))
            .map(toml_scalar_str)
            .unwrap_or_default()
    };
    let info = PluginInfo {
        id: get("id"),
        name: get("name"),
        version: get("version"),
    };
    let pid = info.id.clone();
    if pid.is_empty() || pid.contains('/') || pid.contains('\\') || pid == "." || pid == ".." {
        return PluginValidation {
            ok: false,
            msg: crate::i18n::trf("unsafe plugin id: %1", &[&py_repr(&pid)]),
            info,
        };
    }
    let args: [&dyn std::fmt::Display; 4] = [&info.name, &info.version, &pid, &names.len()];
    let msg = if names.iter().any(|n| n == "files.txt") {
        crate::i18n::trf("plugin “%1” %2 (id %3), files: %4", &args)
    } else {
        crate::i18n::trf(
            "plugin “%1” %2 (id %3), files: %4; ⚠️ no files.txt (the daemon will warn)",
            &args,
        )
    };
    PluginValidation {
        ok: true,
        msg,
        info,
    }
}

/// Проверить скачанный плагин-zip ДО установки — порт `validate_plugin_zip`
/// вместе с чтением архива (read-only, как разбор `.rxn` в `manifest_v4`).
pub fn validate_plugin_zip(path: &Path) -> PluginValidation {
    if !path.is_file() {
        return PluginValidation::fail(crate::i18n::tr("file not found"));
    }
    let Ok(file) = std::fs::File::open(path) else {
        return PluginValidation::fail(crate::i18n::tr("not a zip archive"));
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return PluginValidation::fail(crate::i18n::tr("not a zip archive")),
    };
    // Порядок и дубликаты — как в `ZipFile.namelist()` (порядок записей в
    // архиве): от него зависят и «файлов: N», и то, о каком именно пути
    // сообщит проверка безопасности.
    let mut names: Vec<String> = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        match archive.by_index_raw(i) {
            Ok(f) => names.push(f.name().to_string()),
            Err(e) => return PluginValidation::fail(crate::i18n::trf("the archive cannot be read: %1", &[&e])),
        }
    }
    let manifest = archive.by_name("plugin.toml").ok().and_then(|mut f| {
        use std::io::Read;
        let mut buf = String::new();
        f.read_to_string(&mut buf).ok().map(|_| buf)
    });
    validate_plugin_entries(&names, manifest.as_deref())
}

/// План установки плагина распаковкой в каталог плагинов — порт
/// `install_plugin_zip_fs` (сами файловые операции исполняет шим).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInstallPlan {
    /// Валидация архива пройдена.
    pub ok: bool,
    /// При `ok == false` — готовое сообщение об отказе.
    pub message: String,
    pub info: PluginInfo,
    /// Корень пользовательских плагинов.
    pub root: String,
    /// Каталог плагина `<root>/<id>`.
    pub target: String,
    /// Каталог прежней версии для отката.
    pub bak: String,
    /// Префикс временного каталога-стейджинга.
    pub staging_prefix: String,
    /// Подкаталог стейджинга, куда распаковывается архив.
    pub extract_subdir: String,
    /// Версия для стейта («» — не записывать).
    pub state_version: String,
}

/// Построить план установки плагина. `plugins_root` — переопределение каталога
/// (для dry-run/тестов), «» = каталог по умолчанию из `default_root`.
pub fn plugin_install_plan(
    validation: &PluginValidation,
    default_root: &str,
    plugins_root: &str,
    version: &str,
) -> PluginInstallPlan {
    let root = if plugins_root.is_empty() {
        default_root.to_string()
    } else {
        plugins_root.to_string()
    };
    if !validation.ok {
        return PluginInstallPlan {
            ok: false,
            message: crate::i18n::trf("validation failed: %1", &[&validation.msg]),
            info: validation.info.clone(),
            root,
            target: String::new(),
            bak: String::new(),
            staging_prefix: PLUGIN_STAGING_PREFIX.to_string(),
            extract_subdir: PLUGIN_EXTRACT_SUBDIR.to_string(),
            state_version: String::new(),
        };
    }
    let target = format!("{root}/{}", validation.info.id);
    PluginInstallPlan {
        ok: true,
        message: String::new(),
        info: validation.info.clone(),
        root,
        bak: format!("{target}{BAK_SUFFIX}"),
        target,
        staging_prefix: PLUGIN_STAGING_PREFIX.to_string(),
        extract_subdir: PLUGIN_EXTRACT_SUBDIR.to_string(),
        state_version: if version.is_empty() {
            String::new()
        } else {
            version_tag_to_state(version)
        },
    }
}

/// Права файла из zip-записи — порт `(zi.external_attr >> 16) & 0o7777`.
/// `ZipFile.extract` НЕ восстанавливает unix-права, а бинарям рендереров
/// нужен exec-бит (демон делает то же в `apply_zip_mode`). `0` = не трогать.
pub fn zip_entry_mode(external_attr: u32) -> u32 {
    (external_attr >> 16) & 0o7777
}

/// Что делать при сбое установки плагина.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginRollbackPlan {
    /// Удалить `target` (только если он появился по нашей вине).
    pub remove_target: bool,
    /// Вернуть `.bak` на место `target`.
    pub restore_bak: bool,
}

/// План отката установки плагина.
///
/// ⚠️ Починка бага оригинала: `except`-ветка `install_plugin_zip_fs`
/// безусловно делала `_rmtree_quiet(target)`. При сбое ДО переноса прежней
/// версии в `.bak` (битый архив, нет места, отказ прав) это стирало РАБОЧИЙ
/// установленный плагин без возможности восстановления. Здесь `target`
/// удаляется только если мы сами его создали.
///
/// * `moved_to_bak` — прежняя версия уже уехала в `.bak`;
/// * `target_created` — мы уже успели создать новый `target`.
pub fn plugin_rollback_plan(moved_to_bak: bool, target_created: bool) -> PluginRollbackPlan {
    PluginRollbackPlan {
        remove_target: target_created,
        restore_bak: moved_to_bak,
    }
}

/// Сообщение об успешной ФС-установке плагина.
pub fn plugin_install_ok_message(plugin_id: &str, target: &str) -> String {
    crate::i18n::trf(
        "plugin %1 installed to %2 (previous version in .bak-update); restart \
the waywallen daemon so the renderers pick it up",
        &[&plugin_id, &target],
    )
}

/// Сообщение об ошибке ФС-установки плагина (после отката).
pub fn plugin_install_fail_message(err: &str) -> String {
    crate::i18n::trf("plugin installation failed, rolled back: %1", &[&err])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ver_tuple_basics() {
        assert_eq!(ver_tuple("v0.2.4"), vec![0, 2, 4]);
        assert_eq!(ver_tuple("0.2.4-x86_64"), vec![0, 2, 4, 86, 64]);
        assert!(ver_tuple("").is_empty());
    }

    #[test]
    fn now_iso_epoch() {
        assert_eq!(now_iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(now_iso(1_752_969_600), "2025-07-20T00:00:00Z");
    }

    #[test]
    fn version_tag_removes_single_v() {
        assert_eq!(version_tag_to_state("v0.2.5"), "0.2.5");
        assert_eq!(version_tag_to_state("vv1"), "v1");
        assert_eq!(version_tag_to_state("version"), "ersion");
    }
}
