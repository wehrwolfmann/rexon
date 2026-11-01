//! Порт `rexon/waywallen_kde_plugin.py` — управление Plasma-плагином обоев
//! `org.waywallen.kde` для KDE Plasma 6 (установка/апгрейд/детект/авто-выбор).
//!
//! Здесь — ЧИСТАЯ логика (детерминированный вход→выход), исполнение НЕ здесь:
//!   * пути пакета (`bundle_dir` / `install_dir` / `version_marker`);
//!   * `is_kde` (разбор `XDG_CURRENT_DESKTOP`);
//!   * `file_sha` — версия рантайм-`.so` (sha256 → первые 16 hex), исполнитель
//!     читает файл, хэширует ЗДЕСЬ (`hashlib.sha256(...).hexdigest()[:16]`);
//!   * `resolve_installed_version` — marker-текст либо хэш установленного `.so`;
//!   * `install_or_upgrade` — весь идемпотентный конвейер kpackagetool6
//!     (no-op / `-i` / `-u` / fallback `-r`+`-i`) как ДРАЙВЕР по замыканию-`run`
//!     (rc/stderr каждого запуска подаётся исполнителем — сам subprocess здесь
//!     не крутится), с той же логикой решений, теми же argv/таймаутами и
//!     текстами сообщений байт-в-байт;
//!   * JS-скрипты Plasma scripting (`select_plugin` / `current_plugins`) и argv
//!     их доставки (`dbus-send` / `qdbus6`), парсер вывода `current_plugins`,
//!     `all_selected`;
//!   * argv рестарта plasmashell (`systemctl --user`).
//!
//! НЕ портировано (исполнение — остаётся за исполнителем/мостом): реальный
//! запуск kpackagetool6/dbus-send/qdbus6/systemctl и чтение их вывода, `which`
//! (наличие тула подаётся параметром), чтение/запись ФС (marker, `.so`,
//! metadata.json — их СОДЕРЖИМОЕ/НАЛИЧИЕ подаётся параметром), catch OSError
//! запуска subprocess (исполнитель формирует «ошибка запуска» сам),
//! оркестрация `ensure_waywallen_plugin` вокруг живых side-effect'ов —
//! соответствующие ЧИСТЫЕ решения (`should_reselect`, сборка финального
//! сообщения) вынесены сюда, сами вызовы — нет.

use sha2::{Digest, Sha256};

pub const PLUGIN_ID: &str = "org.waywallen.kde";
pub const PLUGIN_TYPE: &str = "Plasma/Wallpaper";

/// Имя рантайм-библиотеки QML-модуля внутри пакета; её хэш = версия пакета.
///
/// ⚠️ Устойчиво именно ИМЯ, а не путь: апстрим уже переносил файл внутри
/// пакета (0.3.3 — из `contents/ui/WaywallenDisplayEmbed/` в
/// `contents/ui/Plugin/WaywallenDisplayEmbed/`). Прибитый путь тогда молча
/// сломал бы детект версии — «установленной» версии не видно, обновление не
/// срабатывает, у пользователя старый плагин. Поэтому рантайм ИЩЕТСЯ по
/// имени (`pick_runtime_so_rel` ниже), а путь ниже — только запасной.
pub const RUNTIME_SO_NAME: &str = "libwaywallen-display-qml.so";

/// Раскладка рантайма в текущем забандленном пакете. Используется как
/// запасной ответ, когда поиск по имени ничего не нашёл (пакета нет, каталог
/// нечитаем) — чтобы путь в сообщениях/логах оставался определённым.
pub const RUNTIME_SO_REL: &str =
    "contents/ui/Plugin/WaywallenDisplayEmbed/libwaywallen-display-qml.so";

/// Имя marker-файла с версией установленного нами пакета.
pub const VERSION_MARKER_NAME: &str = ".rexon-installed-version";

/// Кандидаты kpackagetool в порядке приоритета — порт `_kpackagetool`
/// (`shutil.which("kpackagetool6") or shutil.which("kpackagetool5")`).
/// Исполнитель делает `which` по порядку и подаёт первый найденный.
pub const KPACKAGETOOL_CANDIDATES: &[&str] = &["kpackagetool6", "kpackagetool5"];

/// Таймаут запусков kpackagetool6 (секунды) — как `timeout=30` в Python.
pub const KPACKAGETOOL_TIMEOUT: u64 = 30;

// ── пути ────────────────────────────────────────────────────────────────

fn join(parts: &[&str]) -> String {
    parts.join("/")
}

/// Забандленный kpackage внутри Rexon — порт `_BUNDLE_DIR`
/// (`join(dirname(__file__), "kpackage", PLUGIN_ID)`). `module_dir` — каталог
/// пакета `rexon` (родитель `waywallen_kde_plugin.py`).
pub fn bundle_dir(module_dir: &str) -> String {
    join(&[module_dir, "kpackage", PLUGIN_ID])
}

/// Куда kpackagetool6 ставит Plasma/Wallpaper плагины (per-user) — порт
/// `_INSTALL_DIR` (`~/.local/share/plasma/wallpapers/{PLUGIN_ID}`).
pub fn install_dir(home: &str) -> String {
    join(&[home, ".local/share/plasma/wallpapers", PLUGIN_ID])
}

/// Marker с версией установленного пакета — порт `_VERSION_MARKER`.
pub fn version_marker(home: &str) -> String {
    join(&[&install_dir(home), VERSION_MARKER_NAME])
}

/// Запасной путь рантайм-`.so` относительно базы пакета — `join(base,
/// RUNTIME_SO_REL)`. Основной способ — поиск по имени, см.
/// `pick_runtime_so_rel`.
pub fn runtime_so_path(base: &str) -> String {
    join(&[base, RUNTIME_SO_REL])
}

/// Выбрать один рантайм-`.so` среди путей, которые исполнитель нашёл по имени
/// `RUNTIME_SO_NAME` внутри пакета (пути — ОТНОСИТЕЛЬНО базы пакета, разделены
/// `/`). Правило выбора детерминированное, чтобы оба движка на одном дереве
/// всегда сходились в одном файле:
///
/// 1. точное совпадение с `preferred_rel` (раскладка нашего забандленного
///    пакета) — так установленный экземпляр сверяется с бандлом «как с
///    собой», а не со случайно уцелевшей копией от прошлой версии;
/// 2. иначе — самый неглубокий путь;
/// 3. при равной глубине — лексикографически меньший.
///
/// Пустой список → `None` (пакета/рантайма нет).
pub fn pick_runtime_so_rel(found_rel: &[String], preferred_rel: Option<&str>) -> Option<String> {
    if let Some(pref) = preferred_rel {
        if found_rel.iter().any(|p| p == pref) {
            return Some(pref.to_string());
        }
    }
    found_rel
        .iter()
        .min_by(|a, b| {
            let depth = |s: &str| s.matches('/').count();
            depth(a).cmp(&depth(b)).then_with(|| a.cmp(b))
        })
        .cloned()
}

/// Найти рантайм-`.so` внутри пакета `base` ПО ИМЕНИ и выбрать один
/// правилом [`pick_runtime_so_rel`]. Полный путь либо `None`.
///
/// Обход живёт здесь, а не у каждого исполнителя: его зовёт Rust-вкладка
/// обоев (`rexon-gui::we_socket`) и мост `rexon-py` (`wkp_find_runtime_so`)
/// — своей копии обхода у моста больше нет.
/// Симлинки на каталоги не разворачиваются: петля в чужом пакете иначе
/// увела бы обход в бесконечность.
pub fn find_runtime_so(base: &str, preferred_rel: Option<&str>) -> Option<String> {
    let root = std::path::Path::new(base);
    let mut found: Vec<String> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
            } else if entry.file_name() == RUNTIME_SO_NAME {
                if let Some(rel) = path.strip_prefix(root).ok().and_then(|r| r.to_str()) {
                    found.push(rel.to_string());
                }
            }
        }
    }
    pick_runtime_so_rel(&found, preferred_rel)
        .map(|rel| root.join(rel).to_string_lossy().into_owned())
}

/// Имя QML-обёртки элемента вывода внутри пакета: она лежит рядом с
/// каталогом модуля (`…/Plugin/PluginDisplay.qml` и
/// `…/Plugin/WaywallenDisplayEmbed/`) и импортирует его по относительному
/// пути. Модуль `Waywallen.DisplayEmbed` лежит в каталоге с другим именем,
/// поэтому по имени модуля он не импортируется — только через эту обёртку.
pub const DISPLAY_ITEM_QML_NAME: &str = "PluginDisplay.qml";

/// Обёртка элемента вывода для найденной рантайм-библиотеки
/// ([`find_runtime_so`]): каталог выше каталога модуля.
pub fn display_item_qml_of(runtime_so: &str) -> Option<String> {
    let module_dir = std::path::Path::new(runtime_so).parent()?;
    let ui_dir = module_dir.parent()?;
    Some(ui_dir.join(DISPLAY_ITEM_QML_NAME).to_string_lossy().into_owned())
}

/// Где искать пакет с элементом вывода для хоста-клиента вывода на X11:
/// сначала вшитый в Rexon (`<данные>/kpackage/…`, версия, проверенная с
/// нашим демоном), затем установленный Plasma-плагин (`~/.local/share/…`).
pub fn display_package_candidates(data_dir: Option<&str>, home: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(dir) = data_dir.filter(|d| !d.is_empty()) {
        out.push(bundle_dir(dir.trim_end_matches('/')));
    }
    if home.starts_with('/') {
        out.push(install_dir(home.trim_end_matches('/')));
    }
    out
}

/// metadata.json пакета — по нему определяется наличие (bundle/installed).
pub fn metadata_json_path(base: &str) -> String {
    join(&[base, "metadata.json"])
}

// ── окружение ───────────────────────────────────────────────────────────

/// Порт `_is_kde` — "kde"/"plasma" в `XDG_CURRENT_DESKTOP` (без регистра).
pub fn is_kde(xdg_current_desktop: &str) -> bool {
    let d = xdg_current_desktop.to_lowercase();
    d.contains("kde") || d.contains("plasma")
}

// ── версия пакета (хэш рантайм-.so) ─────────────────────────────────────

/// Порт `_file_sha` — sha256 содержимого файла, первые 16 hex-символов.
/// Исполнитель читает файл (в Python — чанками по 64 КиБ, результат тот же).
pub fn file_sha(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(16);
    for b in digest.iter() {
        if hex.len() >= 16 {
            break;
        }
        hex.push_str(&format!("{b:02x}"));
    }
    hex.truncate(16);
    hex
}

/// Порт `_installed_version`: если marker непустой — его текст (после strip),
/// иначе хэш установленного `.so` (`installed_so_sha`, `None` если файла нет).
pub fn resolve_installed_version(
    marker_text: Option<&str>,
    installed_so_sha: Option<&str>,
) -> Option<String> {
    if let Some(t) = marker_text {
        let v = t.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    installed_so_sha.map(str::to_string)
}

// ── установка / апгрейд kpackage ────────────────────────────────────────

/// Результат одного запуска kpackagetool6, поданный исполнителем.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub rc: i32,
    pub stderr: String,
    pub stdout: String,
}

/// Одна команда kpackagetool6, которую исполнитель обязан запустить.
#[derive(Debug, Clone, PartialEq)]
pub struct KpCall {
    pub argv: Vec<String>,
    pub timeout_secs: u64,
}

/// Итог `_install_or_upgrade` — план фактически выполненных вызовов + решение.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallOutcome {
    /// Запуски kpackagetool6 в порядке исполнения (argv + timeout).
    pub calls: Vec<KpCall>,
    pub ok: bool,
    pub message: String,
    /// Был ли (пере)установлен пакет в этот раз.
    pub installed_now: bool,
    /// Версия для записи в marker (только при успешной установке и наличии
    /// `bundle_ver`); `None` — marker не трогаем.
    pub marker_to_write: Option<String>,
}

fn kp_argv(tool: &str, op: &str, target: &str) -> Vec<String> {
    vec![
        tool.to_string(),
        "--type".to_string(),
        PLUGIN_TYPE.to_string(),
        op.to_string(),
        target.to_string(),
    ]
}

/// Порт `_install_or_upgrade`. Драйвер: сам subprocess здесь не крутится —
/// исполнитель через `run` запускает поданную argv и возвращает rc/вывод.
///
/// `tool` — путь kpackagetool (`None` = не найден); `bundle_valid` = каталог
/// пакета существует и содержит metadata.json; `installed` / `installed_ver` /
/// `bundle_ver` — состояние, посчитанное исполнителем из ФС.
///
/// Ветку catch-OSError запуска subprocess Python-оригинала здесь НЕ моделируем
/// (это про сам процесс, не про логику) — исполнитель при провале запуска сам
/// формирует «kpackagetool6 ошибка запуска: …».
pub fn install_or_upgrade(
    tool: Option<&str>,
    bundle_dir: &str,
    bundle_valid: bool,
    installed: bool,
    installed_ver: Option<&str>,
    bundle_ver: Option<&str>,
    run: &mut dyn FnMut(&[String]) -> RunOutput,
) -> InstallOutcome {
    let calls: Vec<KpCall> = Vec::new();
    let fail = |calls: Vec<KpCall>, msg: String| InstallOutcome {
        calls,
        ok: false,
        message: msg,
        installed_now: false,
        marker_to_write: None,
    };

    let Some(tool) = tool else {
        return fail(calls, "kpackagetool6 не найден — нужен KDE Plasma 6".to_string());
    };
    if !bundle_valid {
        return fail(calls, format!("забандленный пакет не найден: {bundle_dir}"));
    }

    // No-op: тот же плагин той же версии уже установлен.
    if installed && bundle_ver.is_some() && installed_ver == bundle_ver {
        return InstallOutcome {
            calls,
            ok: true,
            message: "плагин уже установлен (актуальная версия)".to_string(),
            installed_now: false,
            marker_to_write: None,
        };
    }

    let mut calls = calls;
    let op = if installed { "-u" } else { "-i" };
    let mut r = {
        let argv = kp_argv(tool, op, bundle_dir);
        let out = run(&argv);
        calls.push(KpCall { argv, timeout_secs: KPACKAGETOOL_TIMEOUT });
        out
    };

    if r.rc != 0 {
        // -u мог упасть (у waywallen нет версии в metadata) → remove + install.
        if installed {
            let rm = kp_argv(tool, "-r", PLUGIN_ID);
            run(&rm); // результат -r игнорируется (как в Python)
            calls.push(KpCall { argv: rm, timeout_secs: KPACKAGETOOL_TIMEOUT });
            let ins = kp_argv(tool, "-i", bundle_dir);
            r = run(&ins);
            calls.push(KpCall { argv: ins, timeout_secs: KPACKAGETOOL_TIMEOUT });
        }
        if r.rc != 0 {
            // Как в Python: op здесь остаётся исходным ("-u" при fallback);
            // (stderr если непустой, иначе stdout).strip().
            let detail = if r.stderr.is_empty() { &r.stdout } else { &r.stderr };
            return fail(
                calls,
                format!("kpackagetool6 {op} rc={}: {}", r.rc, detail.trim()),
            );
        }
    }

    let verb = if installed { "обновлён" } else { "установлен" };
    InstallOutcome {
        calls,
        ok: true,
        message: format!("плагин {verb}"),
        installed_now: true,
        marker_to_write: bundle_ver.map(str::to_string),
    }
}

// ── авто-выбор плагина на десктоп-контейнментах ─────────────────────────

/// JS для назначения плагина обоями всех десктопов — порт `select_plugin`.
/// Пробелы значимы (два пробела перед `ds[i]`) — сверено golden-эталоном;
/// НЕ использовать `\`-перенос строки (он съедает ведущие пробелы).
pub fn select_plugin_js() -> String {
    format!(
        "var ds = desktops();for (var i = 0; i < ds.length; i++) {{  ds[i].wallpaperPlugin = '{PLUGIN_ID}';}}"
    )
}

/// JS для чтения wallpaperPlugin каждого десктопа — порт `current_plugins`.
/// Пробел перед `out.push` значим — сверено golden-эталоном.
pub fn current_plugins_js() -> String {
    "var ds = desktops(); var out = [];for (var i = 0; i < ds.length; i++) out.push(ds[i].wallpaperPlugin);print(out.join(','));".to_string()
}

/// argv доставки JS в plasmashell через D-Bus — порт `_plasma_evaluate_script`
/// (`dbus-send`, timeout 15). Строка аргумента — `string:{js}`.
pub fn evaluate_script_argv(js: &str) -> Vec<String> {
    vec![
        "dbus-send".to_string(),
        "--session".to_string(),
        "--print-reply".to_string(),
        "--dest=org.kde.plasmashell".to_string(),
        "--type=method_call".to_string(),
        "/PlasmaShell".to_string(),
        "org.kde.PlasmaShell.evaluateScript".to_string(),
        format!("string:{js}"),
    ]
}

/// argv чтения текущих плагинов — порт `current_plugins` (`qdbus6`, timeout 10).
pub fn current_plugins_argv() -> Vec<String> {
    vec![
        "qdbus6".to_string(),
        "org.kde.plasmashell".to_string(),
        "/PlasmaShell".to_string(),
        "org.kde.PlasmaShell.evaluateScript".to_string(),
        current_plugins_js(),
    ]
}

/// Разбор stdout `current_plugins` — порт `[p for p in
/// out.strip().split(",") if p]` (элементы НЕ триммятся по отдельности).
/// Вызывается только при rc==0; при rc!=0 исполнитель отдаёт пустой список.
pub fn parse_current_plugins(stdout: &str) -> Vec<String> {
    stdout
        .trim()
        .split(',')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// Порт `_all_selected` — непусто И все равны PLUGIN_ID.
pub fn all_selected(plugins: &[String]) -> bool {
    !plugins.is_empty() && plugins.iter().all(|p| p == PLUGIN_ID)
}

// ── рестарт plasmashell (только при обновлении выбранного плагина) ──────

/// argv рестарта plasmashell — порт `_restart_plasmashell`: reset-failed
/// (timeout 5), затем restart (timeout 15). Исполнитель прогоняет по порядку.
pub fn restart_plasmashell_cmds() -> Vec<KpCall> {
    vec![
        KpCall {
            argv: vec![
                "systemctl".to_string(),
                "--user".to_string(),
                "reset-failed".to_string(),
                "plasma-plasmashell.service".to_string(),
            ],
            timeout_secs: 5,
        },
        KpCall {
            argv: vec![
                "systemctl".to_string(),
                "--user".to_string(),
                "restart".to_string(),
                "plasma-plasmashell.service".to_string(),
            ],
            timeout_secs: 15,
        },
    ]
}

// ── оркестрация ensure (чистые решения) ─────────────────────────────────

/// Нужно ли (пере)выбирать плагин на десктопах — порт условия в
/// `ensure_waywallen_plugin`: если только что (пере)установили ИЛИ ещё не на
/// всех десктопах выбран (`not _all_selected()`), делаем select; иначе no-op.
pub fn should_reselect(installed_now: bool, all_selected_now: bool) -> bool {
    installed_now || !all_selected_now
}

/// Финальное сообщение `ensure_waywallen_plugin` — конкатенация через "; "
/// базового сообщения и опциональных пометок (порядок как в Python).
pub fn ensure_message(base: &str, selected: bool, restarted: bool) -> String {
    let mut parts = vec![base.to_string()];
    if selected {
        parts.push("выбран на десктопах".to_string());
    }
    if restarted {
        parts.push("plasmashell перезапущен".to_string());
    }
    parts.join("; ")
}

// ── когда назначать плагин обоями: только по применению ────────────────
//
// ⛔ Дефект 30.10.2026 (приёмка в ВМ, Plasma 6.8.2, новый пользователь):
// ПЕРВОЕ открытие вкладки Wallpaper Engine ставило плагин, назначало его
// обоями ВСЕХ рабочих столов и перезапускало plasmashell. Библиотека была
// пуста — плагин до первого кадра рисует чёрное (`main.qml` апстрима), и
// человек, просто заглянувший во вкладку, остался с чёрным столом вместо
// своих обоев (правило #13).
//
// Как устроено у апстрима: на KDE демон показ не запускает сам
// (`display_registry.rs`: `spawn = External`, «plasmashell грузит
// kpackage»), а выбирает плагин человек в настройках Plasma — сам waywallen
// `wallpaperPlugin` не трогает никогда. При этом `wallpaper_apply` с нулём
// дисплеев демон отклоняет (`apply.rs`: `require_display` →
// `NoDisplayRegistered`). Значит, плагин нужен ВЫБРАННЫМ к моменту
// применения — но не раньше.
//
// Отсюда порядок:
//   * открытие вкладки: плагин можно поставить (kpackagetool6, без root) —
//     на рабочий стол это не влияет; назначать его обоями нельзя;
//     plasmashell перезапускается, только если ОБНОВЛЁН плагин, который уже
//     стоит обоями (иначе новый `.so` не попадёт в память оболочки);
//   * применение (`wallpaper_apply`, запуск плейлиста, переход по нему):
//     дисплеев нет — кадр придерживается, плагин назначается, кадр уходит,
//     как только plasmashell зарегистрирует дисплей (или по сроку — тогда
//     демон честно ответит ошибкой, а не тишиной).

/// Зачем идёт подготовка плагина.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsurePurpose {
    /// Открыта вкладка (или нажата «Поставить версию Rexon»): только
    /// установка, рабочий стол не трогается.
    Prepare,
    /// Человек применил обои из библиотеки — плагин назначается обоями.
    Apply,
}

impl EnsurePurpose {
    /// Назначать ли плагин обоями рабочих столов.
    pub fn selects(self) -> bool {
        self == EnsurePurpose::Apply
    }
}

/// Поля запросов, после которых демон показывает обои на дисплеях.
///
/// `wallpaper_apply_via_portal` сюда НЕ входит: он ставит картинку через
/// портал рабочего стола, а не через дисплей waywallen.
pub const SHOWS_WALLPAPER_FIELDS: &[&str] =
    &["wallpaper_apply", "playlist_activate", "playlist_jump_to"];

/// Показывает ли запрос с этим полем обои на дисплеях.
pub fn request_shows_wallpaper(field: &str) -> bool {
    SHOWS_WALLPAPER_FIELDS.contains(&field)
}

/// Порт обратной стороны `_all_selected`: стоит ли плагин обоями хоть на
/// одном рабочем столе (то есть его `.so` уже в памяти оболочки).
pub fn any_selected(plugins: &[String]) -> bool {
    plugins.iter().any(|p| p == PLUGIN_ID)
}

/// Перезапускать ли plasmashell после установки.
///
/// Только когда ОБНОВИЛИ плагин, который сейчас стоит обоями: старый `.so`
/// уже загружен, и без перезапуска оболочка продолжит рисовать им. Свежая
/// установка и обновление невыбранного плагина оболочку не трогают — она
/// загрузит новый пакет сама, когда его назначат.
pub fn restart_after_install(installed_now: bool, was_present: bool, in_use: bool) -> bool {
    installed_now && was_present && in_use
}

/// Как отправить кадр запроса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyGate {
    /// Сразу, как обычно.
    Send,
    /// Придержать до появления дисплея, а дисплей тем временем поднять
    /// (KDE — назначить плагин, X11 — запустить хост-клиент вывода).
    HoldAndSelect,
}

/// Решение для кадра запроса: придерживать ли его, пока Rexon поднимает
/// дисплей демона.
///
/// Придерживается только запрос, показывающий обои, только там, где дисплей
/// поднимает сам Rexon (`rexon_brings_display`: KDE — назначением плагина,
/// X11 — хостом-клиентом вывода, `wallpaper_backend::DisplaySetup`), и
/// только когда дисплеев у демона нет: есть дисплей — вывод уже работает, и
/// решать за человека, где ещё показывать обои, незачем.
pub fn apply_gate(rexon_brings_display: bool, field: &str, displays: usize) -> ApplyGate {
    if rexon_brings_display && displays == 0 && request_shows_wallpaper(field) {
        ApplyGate::HoldAndSelect
    } else {
        ApplyGate::Send
    }
}

/// Сколько ждать дисплея после назначения плагина, мс. Дольше — кадр
/// уходит как есть, и демон отвечает своей ошибкой «нет дисплея».
pub const HOLD_TIMEOUT_MS: u64 = 15_000;

// ── «Применить» при нуле дисплеев: цель «все рабочие столы» ────────────
//
// ⛔ Дефект 30.10.2026 (ВМ, Plasma 6.8.2, свежий пользователь, r1887):
// после того как назначение плагина перенесли на момент применения,
// вендорная карточка обоев держала «Применить» серой — она включается по
// `PresentationTargetState.hasSelection`, а для этого нужен хоть один
// дисплей. Дисплей же появляется, только когда плагин УЖЕ назначен:
// замкнутый круг, применить обои на свежей системе было нечем. Страница
// «Мониторы» при этом отправляла ставить расширение руками из GitHub/KDE
// Store и выбирать его в настройках Plasma (правило #13).
//
// Выход: пока дисплеев нет, но кадр применения будет придержан и плагин
// назначен (`apply_gate` → `HoldAndSelect`), у кнопки есть цель — «все
// рабочие столы» (пустой список целей в запросе, как у вендорного «все»).
// Решение берётся из того же `apply_gate`, чтобы кнопка не разошлась с
// тем, что транспорт сделает с кадром.

/// Можно ли нажать «Применить», пока у демона нет ни одного дисплея.
///
/// Да ровно тогда, когда транспорт придержит такой кадр и сам поднимет
/// дисплей: тогда обои применятся на все столы.
pub fn apply_without_displays(rexon_brings_display: bool) -> bool {
    apply_gate(rexon_brings_display, "wallpaper_apply", 0) == ApplyGate::HoldAndSelect
}

/// Можно ли запустить плейлист, пока у демона нет ни одного дисплея.
///
/// ⛔ Тот же круг, что у «Применить» (дефект 30.10.2026, после 66a2dd5b):
/// вендорный список плейлистов выбирает цели по одной и без дисплея
/// кнопку запуска не включал, а при пустом списке целей проверка «уже
/// играет» (`every()` по пустому набору) отвечала «да» и превращала
/// запуск в паузу. Решение — тот же `apply_gate` для `playlist_activate`:
/// кадр придержится, плагин назначится, цель — все рабочие столы с
/// подключением новых дисплеев (`auto_attach`), как у вендорного «все».
pub fn play_without_displays(rexon_brings_display: bool) -> bool {
    apply_gate(rexon_brings_display, "playlist_activate", 0) == ApplyGate::HoldAndSelect
}

/// Подсказка страницы «Мониторы» при нуле дисплеев вместо вендорной
/// «поставьте расширение руками» (KDE) или голого «Мониторы не
/// зарегистрированы» (X11: MATE, XFCE, Cinnamon X11 — дефект 30.10.2026).
/// Пустая строка — подсказки нет (дисплей поднимает не Rexon), страница
/// показывает своё. Называет оба пути, что работают без дисплеев:
/// «Применить» у обоев и «Воспроизвести плейлист» ([`play_without_displays`]).
pub fn no_displays_hint(setup: crate::wallpaper_backend::DisplaySetup) -> String {
    use crate::wallpaper_backend::DisplaySetup;
    if !apply_without_displays(setup.rexon_brings_display()) {
        return String::new();
    }
    match setup {
        DisplaySetup::KdePlugin => crate::i18n::tr(
            "Wallpaper Engine wallpapers are not on the desktop yet. Pick a wallpaper or a playlist \
             and press “Apply” or “Play playlist” — Rexon will put them on the Plasma desktop by \
             itself.",
        ),
        DisplaySetup::RexonHost => crate::i18n::tr(
            "Wallpaper Engine wallpapers are not on the desktop yet. Pick a wallpaper or a playlist \
             and press “Apply” or “Play playlist” — Rexon will put them on the desktop by itself.",
        ),
        DisplaySetup::None => String::new(),
    }
}

/// Отпускать ли придержанные кадры.
///
/// `select_done` — подготовка закончилась; `select_ok` — плагин назначен.
/// Назначить не вышло — ждать нечего, ответ демона скажет человеку правду.
pub fn release_held(select_done: bool, select_ok: bool, displays: usize, waited_ms: u64) -> bool {
    displays > 0 || (select_done && !select_ok) || waited_ms >= HOLD_TIMEOUT_MS
}

/// Обновить набор дисплеев демона по его событию.
///
/// Возвращает `true`, если набор изменился. Чужие события не трогают набор.
pub fn track_displays(
    ids: &mut std::collections::BTreeSet<u64>,
    event: &crate::proto::control::Event,
) -> bool {
    use crate::proto::control::event::Payload;
    match event.payload.as_ref() {
        Some(Payload::DisplaySnapshot(snap)) => {
            let next: std::collections::BTreeSet<u64> =
                snap.displays.iter().map(|d| d.display_id).collect();
            let changed = *ids != next;
            *ids = next;
            changed
        }
        Some(Payload::DisplayChanged(changed)) => {
            changed.display.as_ref().is_some_and(|d| ids.insert(d.display_id))
        }
        Some(Payload::DisplayRemoved(removed)) => ids.remove(&removed.display_id),
        _ => false,
    }
}

/// Что писать в журнал о назначении плагина в момент, когда отпускаются
/// придержанные кадры. Прежняя строка «плагин назначен: false» вводила в
/// заблуждение: кадры чаще всего отпускаются по появлению дисплея, пока
/// поток назначения ещё не отчитался, хотя плагин уже назначен.
pub fn select_state_for_log(select_done: bool, select_ok: bool) -> &'static str {
    match (select_done, select_ok) {
        // Общие слова для обоих путей подъёма вывода: на KDE это назначение
        // плагина, на X11 — запуск хоста-клиента вывода.
        (false, _) => "подъём вывода ещё не отчитался",
        (true, true) => "вывод поднят только что",
        (true, false) => "не поднимался (уже был) или поднять не вышло",
    }
}

// ── откат: живые обои не запустились — вернуть прежние обои стола ───────
//
// ⛔ Риск, найденный при приёмке 66a2dd5b (ВМ, llvmpipe): плагин назначен
// ради применения, а рендерер waywallen не стартовал
// (`ww_bridge_pool_create failed: -22`) — человек остаётся с ЧЁРНЫМ столом
// вместо своих обоев (правило #13). То же будет на любой машине, где
// рендерер не поднимается (нет Vulkan с DMA-BUF, старая видеокарта).
//
// Правило: если придержанное применение не удалось — демон ответил ошибкой
// или рендерер этого применения пропал в первые секунды, — и плагин
// назначался ИМЕННО ради него, столам возвращается плагин, стоявший до
// назначения (обычно `org.kde.image`; его настройки Plasma хранит, так что
// возвращается и сама картинка). Стоял плагин до «Применить» — не трогаем.

/// Плагин обоев одного рабочего стола Plasma (контейнера `desktops()`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopPlugin {
    /// `id` контейнера — по нему откат находит тот же стол.
    pub desktop_id: i64,
    pub plugin: String,
}

/// JS: плагин обоев каждого стола вместе с номером контейнера —
/// `id=плагин` через `;`. Снимается перед назначением, чтобы было что
/// вернуть.
pub fn plugins_by_desktop_js() -> String {
    "var ds = desktops(); var res = '';for (var i = 0; i < ds.length; i++) \
     res += (i ? ';' : '') + ds[i].id + '=' + ds[i].wallpaperPlugin;print(res);"
        .to_string()
}

/// Разбор ответа `plugins_by_desktop_js`. Мусор пропускается.
pub fn parse_plugins_by_desktop(stdout: &str) -> Vec<DesktopPlugin> {
    stdout
        .trim()
        .split(';')
        .filter_map(|item| {
            let (id, plugin) = item.trim().split_once('=')?;
            let desktop_id = id.trim().parse().ok()?;
            let plugin = plugin.trim();
            (!plugin.is_empty()).then(|| DesktopPlugin { desktop_id, plugin: plugin.to_string() })
        })
        .collect()
}

/// Имя плагина, которое безопасно вставить в JS (id KPackage: латиница,
/// цифры, `.`, `_`, `-`). Иное в откат не попадает.
fn plugin_id_is_safe(plugin: &str) -> bool {
    !plugin.is_empty()
        && plugin.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// JS отката: столам, где сейчас стоит наш плагин, вернуть прежний.
///
/// `None` — откатывать нечего: плагин был назначен не нами (`selected_now`
/// ложно), снимка прежнего нет или все столы и до того стояли на нашем
/// плагине. Столы, появившиеся после снимка, и столы, где человек уже
/// выбрал другое, не трогаются.
pub fn revert_plugins_js(selected_now: bool, previous: Option<&[DesktopPlugin]>) -> Option<String> {
    if !selected_now {
        return None;
    }
    let entries: Vec<String> = previous?
        .iter()
        .filter(|d| d.plugin != PLUGIN_ID && plugin_id_is_safe(&d.plugin))
        .map(|d| format!("'{}': '{}'", d.desktop_id, d.plugin))
        .collect();
    if entries.is_empty() {
        return None;
    }
    Some(format!(
        "var prev = {{{}}}; var ds = desktops();for (var i = 0; i < ds.length; i++) {{  \
         var p = prev[String(ds[i].id)];  if (p && ds[i].wallpaperPlugin == '{PLUGIN_ID}') \
         ds[i].wallpaperPlugin = p;}}",
        entries.join(", ")
    ))
}

/// Сколько после отправки придержанного применения следить, не пропал ли
/// его рендерер (упал на старте, на первом кадре), мс.
pub const FIRST_FRAME_WATCH_MS: u64 = 10_000;

/// Что делать по очередному событию сторожа применения.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchVerdict {
    /// Следить дальше.
    Keep,
    /// Применение состоялось (или за ним больше незачем следить).
    Done,
    /// Применение не удалось — вернуть прежние обои.
    Revert,
}

/// Сторож придержанного применения: следит за ответом демона на ПОСЛЕДНИЙ
/// отпущенный кадр (он и определяет, что окажется на столе) и за его
/// рендерером в первые `FIRST_FRAME_WATCH_MS`.
///
/// ⛔ Плейлист (`playlist_activate`) и отложенное применение рендерер в
/// ответе не называют — раньше сторож на таком ответе сдавался, и падение
/// рендерера после успешного ответа (ВМ, llvmpipe:
/// `ww_bridge_pool_create failed: -22`) оставляло стол чёрным. Теперь
/// рендерер узнаётся по событиям демона: какие рендереры показаны на
/// дисплеях (`DisplayInfo.links`) с момента отправки кадра. До придержки
/// дисплеев не было вовсе, так что всё, что появилось на них после, — от
/// этого применения. Демон, снимая упавший рендерер, шлёт
/// `RendererRemoved` раньше обновлённого списка дисплеев
/// (`on_renderer_process_exit`), но порядок не важен: сторож помнит всех,
/// кто хоть раз был показан.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyWatch {
    request_id: u64,
    answered: bool,
    /// Рендерер из ответа `wallpaper_apply` — следим ровно за ним.
    renderer_id: Option<String>,
    /// Текущие связи «дисплей → рендереры» по событиям демона.
    links: std::collections::BTreeMap<u64, std::collections::BTreeSet<String>>,
    /// Рендереры, хоть раз показанные на дисплеях с отправки кадра.
    shown: std::collections::BTreeSet<String>,
    /// Рендереры, о которых демон сообщил (`RendererChanged`) с отправки:
    /// живые кандидаты, например следующий рендерер плейлиста.
    announced: std::collections::BTreeSet<String>,
    /// Показанные рендереры, которых демон снял.
    lost: std::collections::BTreeSet<String>,
}

impl ApplyWatch {
    pub fn new(request_id: u64) -> ApplyWatch {
        ApplyWatch {
            request_id,
            answered: false,
            renderer_id: None,
            links: Default::default(),
            shown: Default::default(),
            announced: Default::default(),
            lost: Default::default(),
        }
    }

    /// Ответ ли это на наблюдаемый кадр.
    pub fn watches(&self, request_id: u64) -> bool {
        request_id != 0 && request_id == self.request_id
    }

    /// Ответ демона. `ok` — статус `OK`; `renderer_id` — из ответа
    /// `wallpaper_apply` (пусто у плейлистов и отложенного применения —
    /// тогда рендерер узнаётся по событиям дисплеев, `on_event`).
    pub fn on_response(&mut self, request_id: u64, ok: bool, renderer_id: &str) -> WatchVerdict {
        if !self.watches(request_id) {
            return WatchVerdict::Keep;
        }
        if !ok {
            return WatchVerdict::Revert;
        }
        self.answered = true;
        if !renderer_id.is_empty() {
            self.renderer_id = Some(renderer_id.to_string());
            return WatchVerdict::Keep;
        }
        // Рендерер мог упасть ещё до ответа — события уже пришли.
        self.verdict_by_displays()
    }

    /// Событие демона: связи дисплеев, появление и снятие рендереров.
    pub fn on_event(&mut self, event: &crate::proto::control::Event) -> WatchVerdict {
        use crate::proto::control::event::Payload;
        match event.payload.as_ref() {
            Some(Payload::DisplaySnapshot(snap)) => {
                self.links.clear();
                for display in &snap.displays {
                    self.set_links(display);
                }
                WatchVerdict::Keep
            }
            Some(Payload::DisplayChanged(changed)) => {
                if let Some(display) = &changed.display {
                    self.set_links(display);
                }
                WatchVerdict::Keep
            }
            Some(Payload::DisplayRemoved(removed)) => {
                self.links.remove(&removed.display_id);
                WatchVerdict::Keep
            }
            Some(Payload::RendererChanged(changed)) => {
                if let Some(renderer) = &changed.renderer {
                    self.lost.remove(&renderer.renderer_id);
                    self.announced.insert(renderer.renderer_id.clone());
                }
                WatchVerdict::Keep
            }
            Some(Payload::RendererRemoved(removed)) => self.on_renderer_removed(&removed.renderer_id),
            _ => WatchVerdict::Keep,
        }
    }

    fn set_links(&mut self, display: &crate::proto::control::DisplayInfo) {
        let ids: std::collections::BTreeSet<String> = display
            .links
            .iter()
            .map(|l| l.renderer_id.clone())
            .filter(|id| !id.is_empty())
            .collect();
        self.shown.extend(ids.iter().cloned());
        self.links.insert(display.display_id, ids);
    }

    /// Демон снял рендерер. Наш — значит, он упал, не начав показ.
    pub fn on_renderer_removed(&mut self, renderer_id: &str) -> WatchVerdict {
        if let Some(ours) = &self.renderer_id {
            return if ours == renderer_id { WatchVerdict::Revert } else { WatchVerdict::Keep };
        }
        self.announced.remove(renderer_id);
        if !self.shown.contains(renderer_id) {
            // Не показывался на дисплеях после отправки — не наш.
            return WatchVerdict::Keep;
        }
        self.lost.insert(renderer_id.to_string());
        if self.answered {
            self.verdict_by_displays()
        } else {
            // Ответа ещё нет: ошибку скажет он, а успех — с учётом потери.
            WatchVerdict::Keep
        }
    }

    /// Рендерер не назван: применение не удалось, если показанный рендерер
    /// снят и на смену ему не пришёл ни один живой (смена обоев плейлистом
    /// снимает прежний, но новый к тому времени уже объявлен).
    fn verdict_by_displays(&self) -> WatchVerdict {
        if self.lost.is_empty() {
            return WatchVerdict::Keep;
        }
        let alive = self
            .links
            .values()
            .flatten()
            .chain(self.announced.iter())
            .any(|id| !self.lost.contains(id));
        if alive {
            WatchVerdict::Keep
        } else {
            WatchVerdict::Revert
        }
    }

    /// Человек сам применил что-то ещё — его новое действие главнее.
    pub fn on_new_apply(&self) -> WatchVerdict {
        WatchVerdict::Done
    }

    /// Ход часов: `elapsed_ms` — с отправки кадра.
    pub fn on_tick(&self, elapsed_ms: u64) -> WatchVerdict {
        let limit = if self.answered { FIRST_FRAME_WATCH_MS } else { HOLD_TIMEOUT_MS + FIRST_FRAME_WATCH_MS };
        if elapsed_ms >= limit {
            WatchVerdict::Done
        } else {
            WatchVerdict::Keep
        }
    }
}

/// Сообщение человеку: живые обои не запустились, прежние обои вернули.
pub fn revert_done_text() -> String {
    crate::i18n::tr(
        "The live wallpaper could not start on this computer, so Rexon put your previous \
         desktop wallpaper back.",
    )
}

/// Сообщение человеку: живые обои не запустились, а вернуть прежние не
/// вышло — что сделать самому.
pub fn revert_failed_text() -> String {
    crate::i18n::tr(
        "The live wallpaper could not start, and Rexon could not put your previous wallpaper \
         back. Right-click the desktop, choose “Configure Desktop and Wallpaper…” and pick \
         your wallpaper again.",
    )
}

// ── сторож версии: не ставить старое поверх нового ─────────────────────
//
// ⛔ Дефект 02.10.2026 (живьём у Wehrwolfmann). Прежнее правило было «версия
// = хэш `.so`, при ЛЮБОМ несовпадении с бандлом — `kpackagetool6 -u`».
// Направления оно не знало: Rexon r1062 с вложенным плагином 0.3.3
// (протокол вывода v8) при открытии вкладки обоев переставил его ПОВЕРХ
// более нового 0.4.0, а демон waywallen 0.4.3 принимает только v9 —
// «client protocol v8 not supported; daemon accepts [9..=9]», мониторов нет,
// обои пропали. Теперь сравниваются НАСТОЯЩИЕ версии, старое поверх нового не
// ставится никогда, чужой плагин неизвестной версии без согласия человека не
// трогается, а несовместимость с демоном называется вслух.
//
// Откуда версии:
//   * у плагина — `vendor.toml` (`version`), он едет ВНУТРИ нашего пакета и
//     kpackagetool6 копирует его в установленный каталог: есть он — плагин
//     ставили мы, и версия точная;
//   * чужой установленный (релиз апстрима) `vendor.toml` не несёт,
//     `metadata.json` апстрима без `Version`, а строки версии в `.so` нет
//     (`waywallen_display_version()` — функция, а не текст: узнать её можно
//     только загрузив библиотеку в процесс, чего мы не делаем). Поэтому для
//     чужого — таблица известных хэшей рантайма [`KNOWN_RUNTIME_SHA`];
//   * маркер `.rexon-installed-version` — запасной признак «ставили мы»
//     (так помечали установки до того, как `vendor.toml` поехал в пакете).
//
// Протокол вывода и демон. Демон спросить о протоколе можно только через
// сокет вывода (`display.sock`, запрос `hello`): при отказе он рассылает всем
// клиентам управления событие `DisplayConnectionFailed`, у человека в окне
// waywallen всплывает предупреждение. Это вмешательство в живой демон, и
// поэтому протокол берётся по ВЕРСИИ демона (свойство `Version` на шине, либо
// `--version` бинаря) из таблицы [`DAEMON_DISPLAY_PROTOCOLS`], снятой с тегов
// апстрима. Версия вне таблицы — вердикта нет, остаётся правило «не старее».

/// Имя файла с описанием вендорного куска (внутри пакета плагина).
pub const VENDOR_TOML_NAME: &str = "vendor.toml";

/// `vendor.toml` пакета `base` (бандл или установленный экземпляр).
pub fn vendor_toml_path(base: &str) -> String {
    join(&[base, VENDOR_TOML_NAME])
}

/// Версия из текста `vendor.toml` (поле `version`); нет поля, пусто или файл
/// не разбирается — `None`.
pub fn parse_vendor_version(text: &str) -> Option<String> {
    let table: toml::Table = toml::from_str(text).ok()?;
    let v = table.get("version")?.as_str()?.trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// Известные рантаймы `libwaywallen-display-qml.so` → версия плагина (по
/// [`file_sha`]). Нужна для ЧУЖОЙ установки, у которой нет `vendor.toml`.
///
/// ⚠️ Хэш меняется не только с версией: пакеты Rexon до r1278 включительно
/// снимали с `.so` отладочные символы, и у установленного из них другой хэш,
/// чем в выпуске. С 09.10.2026 плазмоид едет из архива выпуска бит в бит
/// (PKGBUILD исключает его из `_strip_own_binaries`), но стрипнутые формы
/// остаются у тех, кто ставил прежние пакеты. Незнакомый хэш — версия
/// неизвестна, а не «старая».
pub const KNOWN_RUNTIME_SHA: &[(&str, &str)] = &[
    // 0.4.3 — релиз апстрима `waywallen-kde-0.4.3-x86_64-embed.zip` (с
    // 23.10.2026 едет в пакете; посчитано по архиву выпуска).
    ("5981949590228ee7", "0.4.3"),
    // 0.4.2 — релиз апстрима `waywallen-kde-0.4.2-x86_64-embed.zip` (с
    // 19.10.2026 именно он и едет в пакете; та же сборка стоит у тех, кто
    // ставил релиз сам). Стрипнутой формы нет: с 09.10.2026 пакет плазмоид
    // не стрипает.
    ("19c1613463252bae", "0.4.2"),
    // 0.4.1 — релиз апстрима `waywallen-kde-0.4.1-x86_64-embed.zip` (ехал в
    // пакете с 09.10 по 19.10.2026; та же сборка стоит у тех, кто ставил
    // релиз сам).
    ("684c8ff280549b10", "0.4.1"),
    // 0.4.1 — тот же рантайм после `strip --strip-unneeded` (посчитано тем
    // же strip; на случай пакета, собранного до 09.10.2026).
    ("b70e45b8c5d7dbe3", "0.4.1"),
    // 0.4.0 — релиз апстрима `waywallen-kde-0.4.0-x86_64-embed.zip` (дерево).
    ("bc9e0fbd268e448a", "0.4.0"),
    // 0.4.0 — тот же рантайм после strip в пакете Rexon r1278.
    ("cbfc755c8c3ad9fd", "0.4.0"),
    // 0.3.3 — релиз апстрима (дерево до 26.09.2026).
    ("e9c240a1da4c9327", "0.3.3"),
    // 0.3.3 — тот же рантайм после strip в пакете Rexon r1062.
    ("da1b8e3007238009", "0.3.3"),
];

/// Версия плагина по хэшу его рантайма (см. [`KNOWN_RUNTIME_SHA`]).
pub fn version_from_runtime_sha(sha: &str) -> Option<&'static str> {
    KNOWN_RUNTIME_SHA.iter().find(|(s, _)| *s == sha).map(|(_, v)| *v)
}

/// Протокол вывода, на котором говорит плагин данной версии: строки «с этой
/// версии — этот протокол», по убыванию. Снято с тегов апстрима
/// `waywallen-display` (`WAYWALLEN_DISPLAY_PROTOCOL_VERSION`, 02.10.2026;
/// v0.4.1 сверен 09.10.2026 — тот же v9, добавлены только необязательные
/// «tagged»-расширения, строка «с 0.4.0 — v9» его покрывает; v0.4.2 сверен
/// 19.10.2026 — константа та же, провод не менялся, правки только внутри
/// библиотеки: синхронизация кадров EGL, #76; v0.4.3 сверен 23.10.2026 —
/// константа та же, добавлено только необязательное «tagged»-поле роли
/// дисплея).
pub const PLUGIN_DISPLAY_PROTOCOLS: &[(&str, u32)] = &[
    ("0.4.0", 9),
    ("0.3.0", 8),
    ("0.2.1", 7),
    ("0.2.0", 6),
    ("0.1.3", 5),
    ("0.1.2", 4),
    ("0.1.1", 3),
];

/// До какой версии плагина таблица выше проверена. Новее — протокол не
/// угадываем (у нового выпуска он мог смениться).
pub const PLUGIN_PROTOCOLS_CHECKED_UP_TO: &str = "0.4.3";

/// Какие протоколы вывода принимает демон waywallen данной версии
/// (`MIN/MAX_SUPPORTED_CLIENT_VERSION`), по убыванию версий. Снято с тегов
/// апстрима `waywallen` 02.10.2026, v0.4.4 сверен 09.10.2026, v0.5.0 —
/// 23.10.2026 (по-прежнему `[9..=9]`). Ниже 0.3.0 не записано (там диапазон шире и для нас
/// неинтересен) — вердикта нет.
pub const DAEMON_DISPLAY_PROTOCOLS: &[(&str, u32, u32)] = &[("0.4.0", 9, 9), ("0.3.0", 8, 8)];

/// До какой версии демона таблица выше проверена.
pub const DAEMON_PROTOCOLS_CHECKED_UP_TO: &str = "0.5.0";

fn version_key(v: &str) -> Vec<u128> {
    let mut t = crate::engine_update::ver_tuple(v);
    while t.last() == Some(&0) {
        t.pop();
    }
    t
}

/// Сравнить версии по числам (`0.4.0` = `v0.4`). Пустая — `None`.
pub fn compare_versions(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let (ka, kb) = (version_key(a), version_key(b));
    if ka.is_empty() || kb.is_empty() {
        return None;
    }
    Some(ka.cmp(&kb))
}

fn within_checked(version: &str, checked_up_to: &str) -> bool {
    compare_versions(version, checked_up_to).is_some_and(|o| o != std::cmp::Ordering::Greater)
}

/// Протокол вывода плагина данной версии; вне проверенной таблицы — `None`.
pub fn plugin_protocol(version: &str) -> Option<u32> {
    if !within_checked(version, PLUGIN_PROTOCOLS_CHECKED_UP_TO) {
        return None;
    }
    PLUGIN_DISPLAY_PROTOCOLS
        .iter()
        .find(|(from, _)| compare_versions(version, from).is_some_and(|o| o.is_ge()))
        .map(|(_, p)| *p)
}

/// Диапазон протоколов, который принимает демон данной версии; вне
/// проверенной таблицы — `None`.
pub fn daemon_protocols(version: &str) -> Option<(u32, u32)> {
    if !within_checked(version, DAEMON_PROTOCOLS_CHECKED_UP_TO) {
        return None;
    }
    DAEMON_DISPLAY_PROTOCOLS
        .iter()
        .find(|(from, _, _)| compare_versions(version, from).is_some_and(|o| o.is_ge()))
        .map(|(_, lo, hi)| (*lo, *hi))
}

/// Подходит ли плагин версии `plugin` к демону версии `daemon`: `None` —
/// не знаем (одной из версий нет либо она вне таблиц).
pub fn compatible_with_daemon(plugin: Option<&str>, daemon: Option<&str>) -> Option<bool> {
    let p = plugin_protocol(plugin?)?;
    let (lo, hi) = daemon_protocols(daemon?)?;
    Some((lo..=hi).contains(&p))
}

/// Что видно об установленном экземпляре (всё читает исполнитель).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstalledPlugin {
    /// В `~/.local/share/plasma/wallpapers/` есть `metadata.json`.
    pub present: bool,
    /// `version` из его `vendor.toml` (есть только у поставленного нами).
    pub vendor_version: Option<String>,
    /// Непустой маркер `.rexon-installed-version` — ставили мы.
    pub marker: bool,
    /// [`file_sha`] его рантайма.
    pub runtime_sha: Option<String>,
}

/// Вход решения «ставить ли плагин».
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginCheck {
    /// `version` из `vendor.toml` нашего бандла.
    pub bundle_version: Option<String>,
    /// [`file_sha`] рантайма бандла.
    pub bundle_sha: Option<String>,
    pub installed: InstalledPlugin,
    /// Версия демона waywallen (шина или `--version`); `None` — не узнать.
    pub daemon_version: Option<String>,
    /// Человек сам нажал «Поставить версию Rexon».
    pub consent: bool,
}

/// Что делать с плагином.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginAction {
    /// Плагина нет — `kpackagetool6 -i`.
    Install,
    /// Установленный старее (или наш без версии, или согласие) — `-u`.
    Upgrade,
    /// Тот же — ничего не делать.
    Keep,
    /// Установленный НОВЕЕ бандла — не трогать.
    KeepNewer,
    /// Чужой, версию не определить — не трогать, спросить человека.
    AskForeign,
    /// Бандл не подходит к демону — не ставить (не испортить рабочее).
    Blocked,
}

impl PluginAction {
    /// Нужен ли запуск kpackagetool6.
    pub fn installs(self) -> bool {
        matches!(self, PluginAction::Install | PluginAction::Upgrade)
    }

    /// Имя для журнала и сверок.
    pub fn as_str(self) -> &'static str {
        match self {
            PluginAction::Install => "install",
            PluginAction::Upgrade => "upgrade",
            PluginAction::Keep => "keep",
            PluginAction::KeepNewer => "keep_newer",
            PluginAction::AskForeign => "ask_foreign",
            PluginAction::Blocked => "blocked",
        }
    }
}

/// Сообщение человеку о плагине обоев (решение — здесь, показ — вкладка).
#[derive(Debug, Clone, PartialEq)]
pub enum PluginNotice {
    /// Плагин стоит не от Rexon, версия неизвестна — предложить нашу.
    ForeignUnknown,
    /// Установленный (оставленный) плагин не подходит к демону.
    InstalledIncompatible {
        plugin_version: String,
        daemon_version: String,
        /// Наш бандл к демону подходит — можно предложить его поставить.
        offer_install: bool,
        /// Протокол плагина старее того, что принимает демон.
        plugin_older: bool,
    },
    /// Наш бандл не подходит к демону, а рабочего плагина нет.
    BundleIncompatible {
        bundle_version: String,
        daemon_version: String,
        /// Бандл старее демона — обновить Rexon; иначе — обновить waywallen.
        bundle_older: bool,
    },
}

impl PluginNotice {
    /// Предлагать ли кнопку «Поставить версию Rexon».
    pub fn offers_install(&self) -> bool {
        match self {
            PluginNotice::ForeignUnknown => true,
            PluginNotice::InstalledIncompatible { offer_install, .. } => *offer_install,
            PluginNotice::BundleIncompatible { .. } => false,
        }
    }

    /// Признак для журнала и сверок.
    pub fn kind(&self) -> &'static str {
        match self {
            PluginNotice::ForeignUnknown => "foreign_unknown",
            PluginNotice::InstalledIncompatible { .. } => "installed_incompatible",
            PluginNotice::BundleIncompatible { .. } => "bundle_incompatible",
        }
    }

    /// Текст для человека (через каталог перевода).
    pub fn text(&self) -> String {
        use crate::i18n::{tr, trf};
        match self {
            PluginNotice::ForeignUnknown => tr(
                "The Plasma wallpaper plugin for waywallen was installed by someone \
                 other than Rexon, and its version could not be determined. Rexon left \
                 it as is. If live wallpapers do not appear, install the version that \
                 comes with Rexon.",
            ),
            PluginNotice::InstalledIncompatible {
                plugin_version,
                daemon_version,
                offer_install,
                plugin_older,
            } => {
                let head = trf(
                    "The installed Plasma wallpaper plugin (version %1) does not work \
                     with the waywallen engine %2, so live wallpapers are not shown.",
                    &[plugin_version, daemon_version],
                );
                let tail = if *offer_install {
                    tr("Rexon can install a version that fits.")
                } else if *plugin_older {
                    tr("Update Rexon to get a newer plugin.")
                } else {
                    tr("Update waywallen.")
                };
                format!("{head} {tail}")
            }
            PluginNotice::BundleIncompatible { bundle_version, daemon_version, bundle_older } => {
                if *bundle_older {
                    trf(
                        "The waywallen engine %1 is newer than the Plasma wallpaper \
                         plugin in this version of Rexon (%2). Update Rexon, otherwise \
                         live wallpapers will not appear on KDE.",
                        &[daemon_version, bundle_version],
                    )
                } else {
                    trf(
                        "The Plasma wallpaper plugin in Rexon (%2) needs a newer waywallen \
                         engine than the installed one (%1). Update waywallen.",
                        &[daemon_version, bundle_version],
                    )
                }
            }
        }
    }
}

/// Подпись кнопки согласия.
pub fn install_rexon_version_label() -> String {
    crate::i18n::tr("Install Rexon's Version")
}

/// Установка по согласию не удалась; `reason` — сырой ответ kpackagetool6.
pub fn install_failed_text(reason: &str) -> String {
    crate::i18n::trf("Could not install the Plasma wallpaper plugin: %1", &[&reason])
}

/// Решение о плагине.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginPlan {
    pub action: PluginAction,
    /// Распознанная версия установленного (`vendor.toml` либо хэш).
    pub installed_version: Option<String>,
    pub notice: Option<PluginNotice>,
    /// Назначать ли плагин обоями десктопов. Нет, если оставленный плагин
    /// заведомо не подходит к демону (иначе вместо обоев человека — чёрный
    /// экран) или ставить нечего.
    pub select: bool,
    /// Почему так — для журнала (не для человека, не переводится).
    pub reason: String,
}

/// Решить, что делать с плагином. Чистая функция: всё состояние подано.
///
/// Правила:
///   * установленный НОВЕЕ бандла не трогается никогда (даже по согласию —
///     кнопку в этом случае предлагают только ради совместимости с демоном);
///   * тот же рантайм — ничего не делать; та же версия у нашего с другим
///     содержимым — освежить (наши правки поверх апстрима могли смениться);
///   * чужой неизвестной версии — только по согласию;
///   * бандл, который демон заведомо не примет, не ставится: рабочий
///     плагин не портим, а если его нет — называем, что обновить.
pub fn plan_plugin(c: &PluginCheck) -> PluginPlan {
    use std::cmp::Ordering;

    let inst = &c.installed;
    let installed_version = inst.vendor_version.clone().or_else(|| {
        inst.runtime_sha.as_deref().and_then(version_from_runtime_sha).map(str::to_string)
    });
    let ours = inst.vendor_version.is_some() || inst.marker;
    let daemon = c.daemon_version.as_deref();
    let bundle_ok = compatible_with_daemon(c.bundle_version.as_deref(), daemon);
    let installed_ok = compatible_with_daemon(installed_version.as_deref(), daemon);

    let plan = |action: PluginAction, notice, reason: &str| PluginPlan {
        action,
        installed_version: installed_version.clone(),
        notice,
        select: match action {
            PluginAction::Install | PluginAction::Upgrade => true,
            PluginAction::Blocked => inst.present && installed_ok == Some(true),
            _ => installed_ok != Some(false),
        },
        reason: reason.to_string(),
    };
    // Бандл демону не подходит: если ставить нечего лучше — объяснить.
    let blocked = |reason: &str| {
        let notice = (installed_ok != Some(true)).then(|| PluginNotice::BundleIncompatible {
            bundle_version: c.bundle_version.clone().unwrap_or_default(),
            daemon_version: c.daemon_version.clone().unwrap_or_default(),
            bundle_older: bundle_protocol_older(c),
        });
        plan(PluginAction::Blocked, notice, reason)
    };
    // Оставляем установленный: не подходит к демону — сказать.
    let keep = |action, reason: &str| {
        let notice = (installed_ok == Some(false)).then(|| {
            let iv = installed_version.clone().unwrap_or_default();
            let older = plugin_protocol(&iv)
                .zip(daemon.and_then(daemon_protocols))
                .is_some_and(|(p, (lo, _))| p < lo);
            PluginNotice::InstalledIncompatible {
                plugin_version: iv,
                daemon_version: c.daemon_version.clone().unwrap_or_default(),
                offer_install: bundle_ok == Some(true),
                plugin_older: older,
            }
        });
        plan(action, notice, reason)
    };
    let upgrade = |reason: &str| {
        if bundle_ok == Some(false) {
            blocked("бандл не подходит к демону — не ставим")
        } else {
            plan(PluginAction::Upgrade, None, reason)
        }
    };

    if !inst.present {
        if bundle_ok == Some(false) {
            return blocked("плагина нет, а бандл не подходит к демону");
        }
        return plan(PluginAction::Install, None, "плагина нет — ставим");
    }

    let same_runtime = c.bundle_sha.is_some() && inst.runtime_sha == c.bundle_sha;
    let cmp = installed_version
        .as_deref()
        .zip(c.bundle_version.as_deref())
        .and_then(|(i, b)| compare_versions(i, b));

    match cmp {
        Some(Ordering::Greater) => {
            if c.consent && installed_ok == Some(false) && bundle_ok == Some(true) {
                return plan(
                    PluginAction::Upgrade,
                    None,
                    "по согласию: установленный новее, но не подходит к демону",
                );
            }
            keep(PluginAction::KeepNewer, "установленный новее бандла — не трогаем")
        }
        Some(Ordering::Equal) if same_runtime || !ours => {
            keep(PluginAction::Keep, "та же версия — ничего не делаем")
        }
        Some(Ordering::Equal) => upgrade("та же версия, другое содержимое (наши правки) — освежаем"),
        Some(Ordering::Less) => upgrade("установленный старее — обновляем"),
        None if same_runtime => keep(PluginAction::Keep, "тот же рантайм — ничего не делаем"),
        None if c.bundle_version.is_none() => {
            keep(PluginAction::Keep, "версия бандла неизвестна — не трогаем")
        }
        None if c.consent => upgrade("по согласию человека — ставим версию Rexon"),
        None if ours => upgrade("наш прежний без vendor.toml — обновляем"),
        None => {
            let mut p = keep(PluginAction::AskForeign, "чужой, версия неизвестна — спрашиваем");
            if p.notice.is_none() {
                p.notice = Some(PluginNotice::ForeignUnknown);
            }
            p
        }
    }
}

fn bundle_protocol_older(c: &PluginCheck) -> bool {
    c.bundle_version
        .as_deref()
        .and_then(plugin_protocol)
        .zip(c.daemon_version.as_deref().and_then(daemon_protocols))
        .is_some_and(|(p, (lo, _))| p < lo)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Обход находит рантайм по имени на любой глубине и отдаёт ПОЛНЫЙ путь;
    /// из двух находок без предпочтения выигрывает самая неглубокая.
    #[test]
    fn find_runtime_so_walks_package_by_name() {
        let base = std::env::temp_dir().join(format!("rexon-wkp-find-{}", std::process::id()));
        let deep = base.join("contents/ui/Plugin/WaywallenDisplayEmbed");
        let shallow = base.join("contents/ui/WaywallenDisplayEmbed");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(&shallow).unwrap();
        std::fs::write(deep.join(RUNTIME_SO_NAME), b"a").unwrap();
        std::fs::write(shallow.join(RUNTIME_SO_NAME), b"b").unwrap();
        let base_s = base.to_str().unwrap();

        let got = find_runtime_so(base_s, None).expect("рантайм найден");
        assert_eq!(got, shallow.join(RUNTIME_SO_NAME).to_string_lossy());

        let pref = "contents/ui/Plugin/WaywallenDisplayEmbed/libwaywallen-display-qml.so";
        let got = find_runtime_so(base_s, Some(pref)).expect("рантайм найден");
        assert_eq!(got, deep.join(RUNTIME_SO_NAME).to_string_lossy());

        std::fs::remove_dir_all(&base).unwrap();
        assert_eq!(find_runtime_so(base_s, None), None, "нет пакета — нет рантайма");
    }
}
