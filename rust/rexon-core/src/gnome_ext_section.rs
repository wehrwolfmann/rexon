//! Раздел «Расширения GNOME»: установленное — со слов самой оболочки,
//! действия плитки и обновления.
//!
//! Шина и запасной путь — в [`crate::gnome_shell_ext`] (типаж
//! [`ShellExtIo`]); здесь — решения раздела поверх них:
//!
//! * список установленного: в сеансе GNOME — `ListExtensions` (диск —
//!   запасной путь, когда оболочки нет), пометки плитки («Ошибка», «Не
//!   поддерживает GNOME N», «Обновление — после перезахода», «Системное»);
//! * действия: установить (окно оболочки, без неё — zip выпуска),
//!   включить/выключить, настройки, удалить (только своё), открыть папку
//!   или скопировать путь, подробности ошибки;
//! * журнал установленного (`provider_id = ego`);
//! * обновления: `CheckForUpdates()` и строка «Скачано обновлений: N».
//!
//! Всё проверяется на подделке [`ShellExtIo`], живая оболочка не нужна.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::gnome_shell_ext::{self as gse, ExtInfo, ExtState, InstallOutcome, ShellExtError, ShellExtIo};
use crate::i18n::{tr, trf};
use crate::models::ThemeItem;

/// Ключ раздела.
pub const SECTION_KEY: &str = "gnomeext";
/// Вид применения раздела (матрица `de_customization`).
pub const KIND: &str = "gnome-shell-extension";
/// Выход из сеанса GNOME: подтверждение показывает сам GNOME.
pub const LOGOUT_ARGV: &[&str] = &["gnome-session-quit", "--logout"];
/// Сколько раз пересчитать скачанные обновления после `CheckForUpdates()`:
/// оболочка отвечает сразу, а качает в фоне.
pub const UPDATE_RECOUNTS: usize = 6;
/// Пауза между пересчётами, секунды.
pub const UPDATE_RECOUNT_SECS: u64 = 5;

/// Префикс номера плитки с диска этого раздела.
fn local_prefix() -> String {
    format!("local:{SECTION_KEY}:")
}

/// Номер плитки установленного расширения (тот же, что даёт обзор диска).
pub fn local_id(uuid: &str) -> String {
    format!("{}{uuid}", local_prefix())
}

/// uuid из номера плитки: `local:gnomeext:<uuid>` или `ego:<uuid>`.
pub fn uuid_of_id(id: &str) -> Option<&str> {
    if let Some(u) = id.strip_prefix(&local_prefix()) {
        return crate::rxn_manifest::is_extension_uuid(u).then_some(u);
    }
    crate::gnome_ego::parse_item_id(id)
}

/// Снимок оболочки: `None` — оболочки нет или она не ответила (тогда
/// раздел живёт по диску, как прежде).
pub fn shell_snapshot(io: &mut dyn ShellExtIo) -> Option<Vec<ExtInfo>> {
    if !io.shell_running() {
        return None;
    }
    match gse::list_extensions(io) {
        Ok(list) => Some(list),
        Err(e) => {
            crate::log_warn!("gnome-ext", "ListExtensions: {e:?}");
            None
        }
    }
}

/// Живой снимок (сеансовая шина).
pub fn live_snapshot() -> Option<Vec<ExtInfo>> {
    shell_snapshot(&mut gse::LiveShell::new())
}

/// uuid расширений, которые оболочка запустила (`state` = включено), —
/// «Активно» на плитке.
pub fn active_uuids(list: &[ExtInfo]) -> Vec<String> {
    list.iter().filter(|e| e.state == ExtState::Active).map(|e| e.uuid.clone()).collect()
}

/// uuid расширений, включённых в настройке оболочки (даже если запуститься
/// они не смогли) — по ним решается, «Включить» или «Выключить».
pub fn enabled_uuids(list: &[ExtInfo]) -> Vec<String> {
    list.iter().filter(|e| e.enabled || e.state == ExtState::Active).map(|e| e.uuid.clone()).collect()
}

// ── Пометки плитки ───────────────────────────────────────────────────────

/// Пометка «Ошибка».
pub fn error_label() -> String {
    tr("Error")
}

/// Пометка «Обновление — после перезахода».
pub fn update_label() -> String {
    tr("Update — after you log in again")
}

/// Пометка «Системное».
pub fn system_label() -> String {
    // TRANSLATORS: a GNOME extension installed for the whole system.
    tr("System")
}

/// Пометка расширения, которое лежит на диске, но оболочка его ещё не
/// видела (поставлено мимо неё, без перезахода).
pub fn next_login_label() -> String {
    tr("Starts after you log in again")
}

/// Пометка устаревшего: «Не поддерживает GNOME 51». `shell` пусто —
/// общая фраза.
pub fn out_of_date_label(shell: &str) -> String {
    if shell.is_empty() {
        tr("Doesn't support this GNOME version")
    } else {
        crate::gnome_ego::unsupported_label(shell)
    }
}

fn mark(key: &str, text: String) -> Value {
    serde_json::json!({ "key": key, "text": text })
}

/// Что делать с папкой расширения: своё — открыть в файловом менеджере,
/// системное — скопировать путь (менять там нечего).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderAction {
    Open(String),
    Copy(String),
}

impl FolderAction {
    pub fn key(&self) -> &'static str {
        match self {
            FolderAction::Open(_) => "open",
            FolderAction::Copy(_) => "copy",
        }
    }
    pub fn path(&self) -> &str {
        match self {
            FolderAction::Open(p) | FolderAction::Copy(p) => p,
        }
    }
}

/// Действие с папкой у расширения из оболочки.
pub fn folder_action(info: &ExtInfo) -> Option<FolderAction> {
    if info.path.is_empty() {
        return None;
    }
    Some(if info.is_system { FolderAction::Copy(info.path.clone()) } else { FolderAction::Open(info.path.clone()) })
}

/// Поля плитки по сведениям оболочки. `shell` — подпись версии GNOME
/// («51») для пометки устаревшего. Ключи читает страница (шаг QML):
/// `gnomeShellKnown`, `gnomeUuid`, `gnomeState`, `gnomeEnabled`,
/// `gnomeActive`, `gnomeCanChange`, `gnomeHasPrefs`, `gnomeSystem`,
/// `gnomeCanRemove`, `gnomeHasUpdate`, `gnomeError`, `gnomeErrorText`,
/// `gnomeUnsupported`, `gnomeUnsupportedText`, `gnomeFolderAction`,
/// `gnomePath`, `gnomeVersion`, `gnomeUrl` (домашняя страница автора),
/// `gnomeMarks` (`[{key, text}]` по порядку).
pub fn shell_fields(info: &ExtInfo, shell: &str) -> Map<String, Value> {
    let mut m = Map::new();
    let error = info.state == ExtState::Error || (!info.error.is_empty() && info.state != ExtState::Active);
    let out_of_date = info.state == ExtState::OutOfDate;
    let mut marks = Vec::new();
    if error {
        marks.push(mark("error", error_label()));
    }
    if out_of_date {
        marks.push(mark("out-of-date", out_of_date_label(shell)));
    }
    if info.has_update {
        marks.push(mark("update", update_label()));
    }
    if info.is_system {
        marks.push(mark("system", system_label()));
    }
    let folder = folder_action(info);
    m.insert("gnomeShellKnown".into(), Value::from(true));
    m.insert("gnomeUuid".into(), Value::from(info.uuid.clone()));
    m.insert("gnomeState".into(), Value::from(info.state.key()));
    m.insert("gnomeEnabled".into(), Value::from(info.enabled));
    m.insert("gnomeActive".into(), Value::from(info.state == ExtState::Active));
    m.insert("gnomeCanChange".into(), Value::from(info.can_change));
    m.insert("gnomeHasPrefs".into(), Value::from(info.has_prefs));
    m.insert("gnomeSystem".into(), Value::from(info.is_system));
    m.insert("gnomeCanRemove".into(), Value::from(!info.is_system));
    m.insert("gnomeHasUpdate".into(), Value::from(info.has_update));
    m.insert("gnomeError".into(), Value::from(error));
    m.insert("gnomeErrorText".into(), Value::from(info.error.clone()));
    m.insert("gnomeUnsupported".into(), Value::from(out_of_date));
    m.insert(
        "gnomeUnsupportedText".into(),
        Value::from(if out_of_date { out_of_date_label(shell) } else { String::new() }),
    );
    m.insert("gnomeFolderAction".into(), Value::from(folder.as_ref().map(FolderAction::key).unwrap_or("")));
    m.insert("gnomePath".into(), Value::from(info.path.clone()));
    m.insert("gnomeVersion".into(), Value::from(info.display_version().to_string()));
    m.insert("gnomeUrl".into(), Value::from(info.url.clone()));
    m.insert("gnomeMarks".into(), Value::Array(marks));
    m
}

/// Поля плитки расширения, которое лежит на диске, а запущенная оболочка
/// его не знает: заработает после перезахода.
pub fn unseen_fields(uuid: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("gnomeShellKnown".into(), Value::from(false));
    m.insert("gnomeUuid".into(), Value::from(uuid.to_string()));
    m.insert("gnomeMarks".into(), Value::Array(vec![mark("next-login", next_login_label())]));
    m
}

/// Плитка расширения, которое знает оболочка, а обзор диска не нашёл
/// (каталог вне `~/.local/share` и `/usr/share`).
pub fn item_from_info(info: &ExtInfo) -> ThemeItem {
    // Тот же вид, что у плитки с диска («значок вид · состояние»): подпись
    // состояния дописывает [`stamp_rows`] через [`author_with_state`].
    let head = format!("🧩 {}", tr("GNOME Shell extension"));
    let author = match state_badge(info) {
        Some(badge) => format!("{head} · {badge}"),
        None if info.is_system => format!("{head} · {}", system_label()),
        None => head,
    };
    ThemeItem {
        id: local_id(&info.uuid),
        name: info.name.clone(),
        description: info.description.clone(),
        author,
        version: info.display_version().to_string(),
        category_id: crate::gnome_ego::CATEGORY_ID,
        installed: true,
        install_path: info.path.clone(),
        local_kind: (info.state == ExtState::Active).then(|| "active".to_string()),
        ..Default::default()
    }
}

/// Подпись состояния установленного расширения — те же слова, что у
/// переключаемого во вкладке «Расширения» (`qt_extensions`): «✅ Активно» /
/// «⏸️ Выключено». `None` — включено, но не работает (ошибка, не та версия
/// GNOME): это скажут пометки плитки, а подпись остаётся прежней.
///
/// Прежде у выключенного здесь стояло происхождение («🗂️ Установлена
/// вручную») — то же место, где у включённого «✅ Активна», и человек не
/// видел, что расширение выключено (живая проверка 15.11).
pub fn state_badge(info: &ExtInfo) -> Option<String> {
    if info.state == ExtState::Active {
        Some(crate::qt_extensions::state_enabled_text())
    } else if !info.enabled {
        Some(crate::qt_extensions::state_disabled_text())
    } else {
        None
    }
}

/// Подпись автора плитки с состоянием со слов оболочки: вид плитки (всё до
/// первого « · ») и [`state_badge`] вместо подписи скана. `None` — менять
/// нечего.
pub fn author_with_state(author: &str, info: &ExtInfo) -> Option<String> {
    let badge = state_badge(info)?;
    let head = author.split(" · ").next().unwrap_or(author).trim_end();
    Some(if head.is_empty() { badge } else { format!("{head} · {badge}") })
}

/// Список раздела в сеансе GNOME: плитки с диска плюс то, что знает только
/// оболочка (по порядку её списка). Номера плиток с диска — `local:gnomeext:<uuid>`.
pub fn merge_installed(mut items: Vec<ThemeItem>, list: &[ExtInfo]) -> Vec<ThemeItem> {
    for info in list {
        let id = local_id(&info.uuid);
        if !items.iter().any(|it| it.id == id) {
            items.push(item_from_info(info));
        }
    }
    items
}

/// Дописать поля оболочки в строки раздела (`local:gnomeext:*` и `ego:*`).
/// Плитка с диска, которой оболочка не знает, получает «после перезахода»;
/// карточке витрины, которая уже стоит, — `installed`.
pub fn stamp_rows(rows: &mut [Value], list: &[ExtInfo], shell: &str) {
    for row in rows.iter_mut() {
        let id = row.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        let local = id.starts_with(&local_prefix());
        let Some(uuid) = uuid_of_id(&id).map(str::to_string) else { continue };
        let Some(m) = row.as_object_mut() else { continue };
        match list.iter().find(|e| e.uuid == uuid) {
            Some(info) => {
                m.extend(shell_fields(info, shell));
                if local {
                    let author = m.get("author").and_then(Value::as_str).unwrap_or("");
                    if let Some(author) = author_with_state(author, info) {
                        m.insert("author".into(), Value::from(author));
                    }
                } else {
                    m.insert("installed".into(), Value::from(true));
                }
            }
            None if local => m.extend(unseen_fields(&uuid)),
            None => {}
        }
    }
}

/// Картинка установленных плиток (`local:gnomeext:<uuid>`) без своей — та
/// же, что у карточки этого расширения в витрине. `picture_file` по uuid
/// отдаёт уже лежащий на диске файл картинки витрины (память страниц EGO
/// `gnome_ego::live_pictures` + кеш превью) или `None` — тогда плитке, как
/// прежде, рисуется заглушка. Своих запросов на сайт здесь нет.
pub fn stamp_ego_pictures(rows: &mut [Value], picture_file: &dyn Fn(&str) -> Option<String>) {
    for row in rows.iter_mut() {
        let Some(m) = row.as_object_mut() else { continue };
        let id = m.get("id").and_then(Value::as_str).unwrap_or("");
        let Some(uuid) = id.strip_prefix(&local_prefix()).map(str::to_string) else { continue };
        if m.get("preview_local").and_then(Value::as_str).is_some_and(|p| !p.is_empty()) {
            continue;
        }
        if let Some(file) = picture_file(&uuid).filter(|f| !f.is_empty()) {
            m.insert("preview_local".into(), Value::from(file));
        }
    }
}

// ── Действия ─────────────────────────────────────────────────────────────

/// Действие плитки (имя приходит из страницы).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Toggle,
    Enable,
    Disable,
    Prefs,
    Uninstall,
    Folder,
    ErrorDetails,
}

impl Action {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "toggle" => Action::Toggle,
            "enable" => Action::Enable,
            "disable" => Action::Disable,
            "prefs" => Action::Prefs,
            "uninstall" => Action::Uninstall,
            "folder" => Action::Folder,
            "errorDetails" => Action::ErrorDetails,
            _ => return None,
        })
    }
}

/// Итог установки для строки состояния: `(удалось, текст)`. «Отмена» в окне
/// оболочки — не ошибка.
pub fn install_message(name: &str, outcome: InstallOutcome) -> (bool, String) {
    match outcome {
        InstallOutcome::Installed => (true, trf("✅ Installed and turned on: %1", &[&name])),
        InstallOutcome::Cancelled => (true, trf("Installation of “%1” was cancelled.", &[&name])),
        InstallOutcome::InstalledNextLogin => {
            (true, format!("{} — {}", trf("✅ Installed: %1", &[&name]), gse::next_login_note()))
        }
    }
}

/// Установить расширение: при запущенной оболочке — через неё (её окно
/// подтверждения, запуск сразу); без неё — `fetch_zip` скачивает zip
/// выпуска и он ставится запасным путём (заработает при следующем входе).
pub fn install(
    io: &mut dyn ShellExtIo,
    uuid: &str,
    data_home: &str,
    fetch_zip: &mut dyn FnMut() -> Result<PathBuf, String>,
) -> Result<InstallOutcome, ShellExtError> {
    if io.shell_running() {
        return gse::install_remote(io, uuid);
    }
    if !crate::rxn_manifest::is_extension_uuid(uuid) {
        return Err(ShellExtError::BadUuid(uuid.to_string()));
    }
    let zip = fetch_zip().map_err(ShellExtError::Offline)?;
    let r = gse::install_zip_offline(io, &zip, uuid, data_home);
    let _ = std::fs::remove_file(&zip);
    r
}

/// Выпуск для запасного пути: под версию GNOME этой машины, иначе самый
/// новый активный. `None` — ставить нечего.
pub fn offline_release(
    releases: &[crate::gnome_ego::Release],
    shell: Option<&crate::gnome_ego::ShellVersion>,
) -> Option<i64> {
    if let Some(v) = shell {
        return crate::gnome_ego::release_for(releases, v).map(|r| r.version);
    }
    releases
        .iter()
        .filter(|r| r.status == crate::gnome_ego::STATUS_ACTIVE)
        .max_by_key(|r| r.version)
        .map(|r| r.version)
}

/// Скачать zip выпуска (живая сеть) в `work`.
pub fn live_fetch_zip(
    uuid: &str,
    shell: Option<&crate::gnome_ego::ShellVersion>,
    work: &Path,
    token: &crate::net::CancelToken,
) -> Result<PathBuf, String> {
    let releases = crate::gnome_ego::live_releases(uuid, token).map_err(|e| e.message())?;
    let version = offline_release(&releases, shell)
        .ok_or_else(|| tr("extensions.gnome.org has no release of this extension for your GNOME version"))?;
    let url = crate::gnome_ego::release_zip_url(uuid, version).ok_or_else(|| ShellExtError::BadUuid(uuid.into()).message())?;
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let dest = work.join(format!("{uuid}.shell-extension.zip"));
    let mut net = crate::net::UreqNet::new().with_cancel(token.clone());
    if crate::ocs::http_download(&mut net, &url, &dest.to_string_lossy(), false, 60, 64 * 1024 * 1024) {
        Ok(dest)
    } else {
        Err(tr("couldn't download the extension from extensions.gnome.org"))
    }
}

// ── Расширения-зависимости пакетов оформления ────────────────────────────

/// Итог установки расширения, которого требует пакет оформления
/// (`.rxn`, образ «Как в дистрибутиве»: `"source": "gnome-extensions"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepOutcome {
    /// Уже стоит — ничего не делали.
    AlreadyInstalled,
    Done(InstallOutcome),
}

/// Поставить расширение-зависимость сразу, без вопроса Rexon «Да/Нет»
/// (правило 24.10: зависимости ставятся сами, защита — окно подтверждения
/// оболочки). Уже стоит (каталог у пользователя или в системе — `on_disk`,
/// либо оболочка его знает) — пропуск. `unsupported` — сверка с сайтом до
/// установки: `Some(причина)` — под эту версию GNOME выпуска нет, и окно
/// оболочки человеку даже не показывается. Дальше — общий [`install`]:
/// при оболочке — через неё, без неё — zip выпуска запасным путём.
pub fn install_dependency(
    io: &mut dyn ShellExtIo,
    uuid: &str,
    data_home: &str,
    on_disk: &dyn Fn(&str) -> bool,
    unsupported: &mut dyn FnMut() -> Option<String>,
    fetch_zip: &mut dyn FnMut() -> Result<PathBuf, String>,
) -> Result<DepOutcome, ShellExtError> {
    if !crate::rxn_manifest::is_extension_uuid(uuid) {
        return Err(ShellExtError::BadUuid(uuid.to_string()));
    }
    if on_disk(uuid) {
        return Ok(DepOutcome::AlreadyInstalled);
    }
    if io.shell_running() && matches!(gse::extension_info(io, uuid), Ok(Some(_))) {
        return Ok(DepOutcome::AlreadyInstalled);
    }
    if let Some(why) = unsupported() {
        return Err(ShellExtError::Offline(why));
    }
    install(io, uuid, data_home, fetch_zip).map(DepOutcome::Done)
}

/// Причина «нет выпуска под эту версию GNOME» по списку выпусков сайта;
/// `None` — выпуск есть или версию GNOME не узнать (тогда решает сама
/// оболочка или запасной путь берёт самый новый выпуск).
pub fn unsupported_reason(
    releases: &[crate::gnome_ego::Release],
    shell: Option<&crate::gnome_ego::ShellVersion>,
) -> Option<String> {
    let v = shell?;
    crate::gnome_ego::release_for(releases, v)
        .is_none()
        .then(|| tr("extensions.gnome.org has no release of this extension for your GNOME version"))
}

/// Строка итога для человека; `None` — говорить нечего (уже стояло).
/// `without` — что без расширения не заработает (виды оформления).
pub fn dependency_note(uuid: &str, without: &str, r: &Result<DepOutcome, ShellExtError>) -> Option<String> {
    let lost = |why: String| {
        let why = why.trim();
        let why = if why.ends_with(['.', '!', '?']) { why.to_string() } else { format!("{why}.") };
        let mut s = trf("The GNOME extension %1 was not installed: %2", &[&uuid, &why]);
        if !without.is_empty() {
            s.push(' ');
            s.push_str(&trf("Without it, this part won't work fully: %1.", &[&without]));
        }
        s
    };
    match r {
        Ok(DepOutcome::AlreadyInstalled) => None,
        Ok(DepOutcome::Done(InstallOutcome::Installed)) => {
            Some(trf("The GNOME extension %1 was installed and turned on.", &[&uuid]))
        }
        Ok(DepOutcome::Done(InstallOutcome::InstalledNextLogin)) => Some(format!(
            "{} {}",
            trf("The GNOME extension %1 was installed.", &[&uuid]),
            gse::next_login_note()
        )),
        Ok(DepOutcome::Done(InstallOutcome::Cancelled)) => {
            Some(lost(tr("you cancelled it in the GNOME Shell window.")))
        }
        // Причина запасного пути (сеть, нет выпуска) — своими словами, без
        // второго «не удалось установить».
        Err(ShellExtError::Offline(why)) => Some(lost(why.clone())),
        Err(e) => Some(lost(e.message())),
    }
}

/// [`install_dependency`] на живой машине: оболочка с шины, версия GNOME,
/// выпуски сайта, zip во временный каталог; поставленное — в журнал
/// установленного раздела «Расширения GNOME» (как ручная установка).
/// Звать из рабочего потока: окно оболочки ждёт человека.
pub fn live_install_dependency(uuid: &str) -> Result<DepOutcome, ShellExtError> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data_home = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), &home);
    let mut io = gse::LiveShell::new();
    let shell = gse::detect_shell_version(&mut io).as_deref().and_then(crate::gnome_ego::parse_shell_version);
    let token = crate::net::CancelToken::new();
    // Список выпусков один на сверку и выбор zip; сайт не ответил —
    // сверка не мешает (решит оболочка или запасной путь скажет сам).
    let releases = crate::gnome_ego::live_releases(uuid, &token).ok();
    let mut unsupported = || releases.as_deref().and_then(|r| unsupported_reason(r, shell.as_ref()));
    let work = std::env::temp_dir().join(format!("rexon-gext-dep-{}", std::process::id()));
    let mut fetch = || live_fetch_zip(uuid, shell.as_ref(), &work, &token);
    let on_disk = |u: &str| crate::distro_looks::extension_installed_live(u);
    let r = install_dependency(&mut io, uuid, &data_home, &on_disk, &mut unsupported, &mut fetch);
    let _ = std::fs::remove_dir_all(&work);
    if let Ok(DepOutcome::Done(o)) = r {
        if o != InstallOutcome::Cancelled {
            let info = gse::extension_info(&mut io, uuid).ok().flatten();
            let (name, version) = info.map(|i| (i.name, i.version)).unwrap_or_default();
            let entry = journal_item(uuid, &name, &version, "");
            let now = crate::engine_update::now_iso(
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
            );
            if let Err(e) = live_journal_edit(&home, |log| journal_record(log, &entry, &data_home, uuid, &now)) {
                crate::log_warn!("gnome-ext", "журнал установленного: {e}");
            }
        }
    }
    r
}

/// Включить или выключить: при оболочке — D-Bus; без неё — сценарий
/// (`gnome-extensions` или ключи `enabled-extensions`/`disabled-extensions`),
/// заработает при входе.
pub fn set_enabled(
    io: &mut dyn ShellExtIo,
    uuid: &str,
    on: bool,
    data_home: &str,
) -> Result<String, ShellExtError> {
    if io.shell_running() {
        gse::set_enabled(io, uuid, on)?;
        return Ok(if on { trf("▶️ Enabled: %1", &[&uuid]) } else { trf("⏸️ Disabled: %1", &[&uuid]) });
    }
    if !crate::rxn_manifest::is_extension_uuid(uuid) {
        return Err(ShellExtError::BadUuid(uuid.to_string()));
    }
    let script = if on {
        let dest = gse::user_extension_dir(data_home, uuid);
        crate::de_customization::gnome_extension_enable_script(uuid, &dest.to_string_lossy())
    } else {
        crate::de_customization::gnome_extension_disable_script(uuid)
    };
    match io.run(&["sh".to_string(), "-c".into(), script]) {
        Ok(out) if out.success => {
            let done = if on { trf("▶️ Enabled: %1", &[&uuid]) } else { trf("⏸️ Disabled: %1", &[&uuid]) };
            Ok(format!("{done} — {}", tr("takes effect the next time you log in to GNOME")))
        }
        Ok(out) => Err(ShellExtError::Offline(out.text.trim().to_string())),
        Err(e) => Err(ShellExtError::Offline(e)),
    }
}

/// «Включить/Выключить» по правде оболочки: включено — выключить.
pub fn toggle(io: &mut dyn ShellExtIo, uuid: &str, data_home: &str) -> Result<String, ShellExtError> {
    if io.shell_running() {
        let info = gse::extension_info(io, uuid)?.ok_or(ShellExtError::NotInstalled)?;
        let on = !(info.enabled || info.state == ExtState::Active);
        return set_enabled(io, uuid, on, data_home);
    }
    // Без оболочки правда — ключи: включено, если uuid есть в
    // `enabled-extensions` и нет в `disabled-extensions` (он сильнее).
    let mut listed = |key: &str| match io.run(&["gsettings".into(), "get".into(), "org.gnome.shell".into(), key.into()]) {
        Ok(out) if out.success => crate::de_apply::parse_gsettings_list(&out.text).iter().any(|u| u == uuid),
        _ => false,
    };
    let enabled = listed("enabled-extensions") && !listed("disabled-extensions");
    set_enabled(io, uuid, !enabled, data_home)
}

/// Удалить своё: при оболочке — `UninstallExtension` (системное она не
/// удаляет — отказ словами); без неё — каталог в `~/.local/share` и только
/// он. `Ok(true)` — удалено.
pub fn uninstall_own(io: &mut dyn ShellExtIo, uuid: &str, data_home: &str) -> Result<(), ShellExtError> {
    if io.shell_running() {
        return gse::uninstall(io, uuid);
    }
    if !crate::rxn_manifest::is_extension_uuid(uuid) {
        return Err(ShellExtError::BadUuid(uuid.to_string()));
    }
    let dir = gse::user_extension_dir(data_home, uuid);
    if !dir.is_dir() {
        return Err(ShellExtError::SystemExtension);
    }
    let _ = io.run(&["sh".to_string(), "-c".into(), crate::de_customization::gnome_extension_disable_script(uuid)]);
    if crate::ext_utils::safe_rmtree(&dir) {
        Ok(())
    } else {
        Err(ShellExtError::Offline(dir.display().to_string()))
    }
}

/// Что делать с папкой: по оболочке, без неё — по месту на диске.
pub fn folder_for(io: &mut dyn ShellExtIo, uuid: &str, data_home: &str, disk_path: &str) -> Option<FolderAction> {
    if io.shell_running() {
        if let Ok(Some(info)) = gse::extension_info(io, uuid) {
            return folder_action(&info);
        }
    }
    if disk_path.is_empty() {
        return None;
    }
    let own = Path::new(disk_path).starts_with(gse::user_extension_dir(data_home, "").as_path());
    Some(if own { FolderAction::Open(disk_path.to_string()) } else { FolderAction::Copy(disk_path.to_string()) })
}

/// Текст окна «Подробнее об ошибке»: последняя ошибка из сведений и журнал
/// ошибок оболочки без повторов. Пусто — ошибок нет.
pub fn error_details(io: &mut dyn ShellExtIo, uuid: &str) -> Result<String, ShellExtError> {
    let info = gse::extension_info(io, uuid)?.ok_or(ShellExtError::NotInstalled)?;
    let mut lines: Vec<String> = Vec::new();
    let first = info.error.trim();
    if !first.is_empty() {
        lines.push(first.to_string());
    }
    for l in gse::extension_errors(io, uuid).unwrap_or_default() {
        let l = l.trim().to_string();
        if !l.is_empty() && !lines.contains(&l) {
            lines.push(l);
        }
    }
    Ok(lines.join("\n"))
}

/// Строка ответа действия для страницы: `{ok, message, action, uuid,
/// folder?, path?, details?}`.
pub fn action_answer(action: Action, uuid: &str, r: Result<Value, ShellExtError>) -> Value {
    let name = match action {
        Action::Toggle => "toggle",
        Action::Enable => "enable",
        Action::Disable => "disable",
        Action::Prefs => "prefs",
        Action::Uninstall => "uninstall",
        Action::Folder => "folder",
        Action::ErrorDetails => "errorDetails",
    };
    let mut m = Map::new();
    m.insert("action".into(), Value::from(name));
    m.insert("uuid".into(), Value::from(uuid.to_string()));
    match r {
        Ok(Value::Object(extra)) => {
            m.insert("ok".into(), Value::from(true));
            m.extend(extra);
        }
        Ok(v) => {
            m.insert("ok".into(), Value::from(true));
            m.insert("message".into(), v);
        }
        Err(e) => {
            m.insert("ok".into(), Value::from(false));
            m.insert("message".into(), Value::from(e.message()));
        }
    }
    Value::Object(m)
}

/// Исполнить действие плитки. `disk_path` — путь плитки с диска (для папки
/// без оболочки). Окно настроек и папку открывает живой слой — здесь только
/// решение (`folder`, `path`) и вызов настроек через оболочку.
pub fn run_action(io: &mut dyn ShellExtIo, action: Action, uuid: &str, data_home: &str, disk_path: &str) -> Value {
    let r: Result<Value, ShellExtError> = match action {
        Action::Toggle => toggle(io, uuid, data_home).map(Value::from),
        Action::Enable => set_enabled(io, uuid, true, data_home).map(Value::from),
        Action::Disable => set_enabled(io, uuid, false, data_home).map(Value::from),
        Action::Prefs => gse::open_prefs(io, uuid, "").map(|()| Value::from(String::new())),
        Action::Uninstall => uninstall_own(io, uuid, data_home).map(|()| Value::from(trf("🗑️ Removed: %1", &[&uuid]))),
        Action::Folder => match folder_for(io, uuid, data_home, disk_path) {
            Some(f) => {
                let msg = match &f {
                    FolderAction::Open(_) => String::new(),
                    FolderAction::Copy(p) => trf("Path copied: %1", &[&p]),
                };
                Ok(serde_json::json!({ "folder": f.key(), "path": f.path(), "message": msg }))
            }
            None => Err(ShellExtError::NotInstalled),
        },
        Action::ErrorDetails => error_details(io, uuid).map(|d| {
            let text = if d.is_empty() { tr("GNOME Shell reported no errors for this extension.") } else { d };
            // Имя для заголовка окна и домашняя страница для «Написать
            // автору» — окно открывают и с плитки, где их нет.
            let info = gse::extension_info(io, uuid).ok().flatten();
            let name = info.as_ref().map(|i| i.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| uuid.to_string());
            let url = info.map(|i| i.url).unwrap_or_default();
            serde_json::json!({ "details": text, "message": "", "name": name, "url": url })
        }),
    };
    action_answer(action, uuid, r)
}

// ── Журнал установленного ────────────────────────────────────────────────

/// Запись журнала установленного: `content_id = ego:<uuid>`,
/// `provider_id = ego`, версия выпуска и каталог расширения пользователя.
pub fn journal_item(uuid: &str, name: &str, version: &str, author: &str) -> crate::ocs_store::InstalledItem {
    crate::ocs_store::InstalledItem {
        content_id: crate::gnome_ego::item_id(uuid),
        name: if name.is_empty() { uuid.to_string() } else { name.to_string() },
        rexon_type: KIND.to_string(),
        author: author.to_string(),
        version: version.to_string(),
        web_url: format!("{}/extension-query/?uuid={uuid}", crate::gnome_ego::HOME_URL),
        provider_id: crate::gnome_ego::PROVIDER_ID.to_string(),
        category_id: crate::gnome_ego::CATEGORY_ID,
        ..Default::default()
    }
}

/// Записать установку в журнал (`log` — содержимое `ocs_installed.json`).
pub fn journal_record(log: &mut Map<String, Value>, item: &crate::ocs_store::InstalledItem, data_home: &str, uuid: &str, now: &str) {
    let path = gse::user_extension_dir(data_home, uuid).to_string_lossy().into_owned();
    crate::ocs_store::record_installed(log, item, &[path], now);
}

/// Забыть установку (после удаления).
pub fn journal_forget(log: &mut Map<String, Value>, uuid: &str) -> bool {
    crate::ocs_store::forget_installed(log, &crate::gnome_ego::item_id(uuid))
}

/// Живая правка журнала (`~/.config/rexon/ocs_installed.json` через общий
/// журнал установки).
pub fn live_journal_edit(home: &str, edit: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
    let cfg = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty());
    let path = crate::ocs_store::install_log_path_xdg(cfg.as_deref(), home);
    let mut data = crate::install_journal::store_log_view(&path);
    edit(&mut data);
    crate::install_journal::put_store_log(&path, &data).map_err(|e| e.to_string())
}

// ── Удаление с «Вернуть» ─────────────────────────────────────────────────

/// Вопрос перед удалением своего расширения: `(заголовок, текст)`. После
/// «Да» расширение, как тема в соседних разделах, помечается к удалению и
/// висит строкой «Вернуть»; снимает его оболочка, когда строка закрылась.
pub fn removal_question(name: &str) -> (String, String) {
    (
        tr("Remove the extension?"),
        // TRANSLATORS: %1 is the name of a GNOME Shell extension.
        trf("“%1” will be removed from your home folder. You can install it again from extensions.gnome.org.", &[&name]),
    )
}

/// Помеченное к удалению — на расширения GNOME (их снимает оболочка, по
/// uuid из номера плитки) и прочее (папки тем — `theme_removal::live_commit`).
pub fn split_pending(
    pending: Vec<crate::theme_removal::PendingRemoval>,
) -> (Vec<(String, crate::theme_removal::PendingRemoval)>, Vec<crate::theme_removal::PendingRemoval>) {
    let mut gnome = Vec::new();
    let mut rest = Vec::new();
    for p in pending {
        match uuid_of_id(&p.item_id).map(str::to_string) {
            Some(uuid) => gnome.push((uuid, p)),
            None => rest.push(p),
        }
    }
    (gnome, rest)
}

// ── Подписи страницы ─────────────────────────────────────────────────────

/// Кнопка «Включить/Выключить» плитки и подробностей — по той же правде,
/// что [`toggle`]: включено в настройке или запущено — «Выключить». Слова —
/// те же, что у плитки дополнения (`appearance_page::card_local_button_for`).
/// `can_change = false` (администратор, режим сеанса) — кнопка выключена.
pub fn toggle_button(enabled: bool, active: bool, can_change: bool) -> crate::appearance_page::CardButton {
    let on = enabled || active;
    crate::appearance_page::CardButton {
        text: if on { tr("⏸️ Disable") } else { tr("▶️ Enable") },
        state: if on { "true" } else { "false" },
        enabled: can_change,
    }
}

/// Подсказка на выключенной кнопке переключения.
pub fn locked_tooltip() -> String {
    // Те же слова, что у отказа оболочки (`gnome_shell_ext`).
    tr("This extension can't be changed: it is locked by the administrator or the session.")
}

/// Подсказка «Установить»: подтверждение спросит сама оболочка.
pub fn install_tooltip() -> String {
    tr("GNOME Shell will ask you to confirm the installation in its own window")
}

/// Подписи раздела «Расширения GNOME» для страницы (JSON по образцу
/// `conky_procs::section_texts`). `error_title` — с `%1` (имя расширения).
pub fn page_texts() -> Value {
    // Пары, а не `json!`: строки внутри макроса извлечение перевода не видит.
    let pairs: Vec<(&str, String)> = vec![
        ("prefs", tr("⚙️ Settings")),
        ("prefs_tooltip", tr("Open the extension's own settings window")),
        ("folder_open", tr("📂 Open folder")),
        ("folder_open_tooltip", tr("Open the extension's folder in the file manager")),
        ("folder_copy", tr("📋 Copy path")),
        ("folder_copy_tooltip", tr("A system extension can't be changed here — copy the path to its folder")),
        ("error_details", tr("⚠️ Error details")),
        ("error_details_tooltip", tr("What GNOME Shell reported when the extension failed")),
        ("uninstall", tr("🗑️ Remove")),
        ("uninstall_tooltip", tr("Remove the extension from your home folder")),
        ("locked_tooltip", locked_tooltip()),
        ("only_compatible", crate::gnome_ego::only_compatible_label()),
        ("only_compatible_tooltip", tr("Off — every extension is shown, and the ones that don't support your GNOME are marked")),
        ("install_tooltip", install_tooltip()),
        // TRANSLATORS: %1 is the name of a GNOME Shell extension.
        ("error_title", tr("Errors of “%1”")),
        ("copy", tr("📋 Copy")),
        ("write_author", tr("✉️ Write to the author")),
        ("write_author_tooltip", tr("Open the extension's home page: there you can report the error to its author")),
    ];
    Value::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), Value::from(v))).collect())
}

// ── Обновления ───────────────────────────────────────────────────────────

/// Строка состояния раздела: «Скачано обновлений: N. Применятся после
/// выхода из сеанса». Пусто — обновлений нет.
pub fn updates_status(count: usize) -> String {
    if count == 0 {
        return String::new();
    }
    // TRANSLATORS: %1 is a number of GNOME extension updates.
    trf("Updates downloaded: %1. They will apply after you log out.", &[&count])
}

/// Подпись кнопки «Выйти из сеанса».
pub fn logout_label() -> String {
    tr("Log Out")
}

/// Ответ о скачанных обновлениях для страницы: `{count, text, logoutLabel}`.
pub fn updates_answer(count: usize) -> Value {
    serde_json::json!({ "count": count, "text": updates_status(count), "logoutLabel": logout_label() })
}

/// Попросить оболочку проверить обновления и пересчитать скачанные:
/// `wait` спит между пересчётами (`false` — хватит, страница закрылась);
/// `report` получает каждое новое число. Возвращает последнее.
pub fn check_updates(
    io: &mut dyn ShellExtIo,
    rounds: usize,
    wait: &mut dyn FnMut() -> bool,
    report: &mut dyn FnMut(usize),
) -> Result<usize, ShellExtError> {
    fn count(io: &mut dyn ShellExtIo) -> Result<usize, ShellExtError> {
        gse::list_extensions(io).map(|l| gse::pending_updates(&l))
    }
    gse::check_for_updates(io)?;
    let mut last = count(io)?;
    report(last);
    for _ in 0..rounds {
        if !wait() {
            break;
        }
        let now = count(io)?;
        if now != last {
            last = now;
            report(last);
        }
    }
    Ok(last)
}

// ── Тесты ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gnome_shell_ext::{BusFailure, CmdOutput, Field, RawInfo};
    use std::collections::BTreeMap;
    use std::time::Duration;

    const U: &str = "sample@example.org";
    const SYS: &str = "system@example.org";

    /// Подделка оболочки с состоянием: включение и удаление меняют словарь.
    #[derive(Default)]
    struct Shell {
        running: bool,
        exts: BTreeMap<String, RawInfo>,
        install_reply: Option<Result<String, BusFailure>>,
        updates_after_check: Vec<String>,
        calls: Vec<String>,
        runs: Vec<String>,
        /// Ответ `gsettings get … disabled-extensions` без оболочки.
        disabled: Vec<String>,
    }

    fn raw(uuid: &str, state: f64, system: bool) -> RawInfo {
        let mut m = RawInfo::new();
        m.insert("uuid".into(), Field::Str(uuid.into()));
        m.insert("name".into(), Field::Str(format!("Name {uuid}")));
        m.insert("state".into(), Field::Num(state));
        m.insert("type".into(), Field::Num(if system { 1.0 } else { 2.0 }));
        m.insert("enabled".into(), Field::Bool(state == 1.0 || state == 3.0));
        let base = if system { "/usr/share" } else { "/h/.local/share" };
        m.insert("path".into(), Field::Str(format!("{base}/gnome-shell/extensions/{uuid}")));
        m.insert("hasPrefs".into(), Field::Bool(true));
        m.insert("canChange".into(), Field::Bool(true));
        m.insert("version".into(), Field::Num(7.0));
        m
    }

    impl Shell {
        fn up() -> Self {
            let mut s = Shell { running: true, ..Default::default() };
            s.exts.insert(U.into(), raw(U, 2.0, false));
            s.exts.insert(SYS.into(), raw(SYS, 1.0, true));
            s
        }
        fn down() -> Self {
            Shell { running: false, ..Default::default() }
        }
        fn set_state(&mut self, uuid: &str, state: f64) -> bool {
            match self.exts.get_mut(uuid) {
                Some(m) => {
                    m.insert("state".into(), Field::Num(state));
                    m.insert("enabled".into(), Field::Bool(state == 1.0));
                    true
                }
                None => false,
            }
        }
    }

    impl ShellExtIo for Shell {
        fn shell_running(&mut self) -> bool {
            self.running
        }
        fn list(&mut self) -> Result<BTreeMap<String, RawInfo>, BusFailure> {
            Ok(self.exts.clone())
        }
        fn info(&mut self, uuid: &str) -> Result<RawInfo, BusFailure> {
            Ok(self.exts.get(uuid).cloned().unwrap_or_default())
        }
        fn errors(&mut self, _uuid: &str) -> Result<Vec<String>, BusFailure> {
            Ok(vec!["boom".into(), "TypeError: x is undefined".into()])
        }
        fn install_remote(&mut self, uuid: &str, timeout: Duration) -> Result<String, BusFailure> {
            self.calls.push(format!("install {uuid}"));
            assert!(timeout >= Duration::from_secs(60), "окно оболочки ждёт человека");
            let r = self.install_reply.clone().unwrap_or(Ok(gse::INSTALL_SUCCESSFUL.into()));
            if r.as_deref() == Ok(gse::INSTALL_SUCCESSFUL) {
                self.exts.insert(uuid.into(), raw(uuid, 1.0, false));
            }
            r
        }
        fn uninstall(&mut self, uuid: &str) -> Result<bool, BusFailure> {
            self.calls.push(format!("uninstall {uuid}"));
            Ok(self.exts.remove(uuid).is_some())
        }
        fn enable(&mut self, uuid: &str) -> Result<bool, BusFailure> {
            self.calls.push(format!("enable {uuid}"));
            Ok(self.set_state(uuid, 1.0))
        }
        fn disable(&mut self, uuid: &str) -> Result<bool, BusFailure> {
            self.calls.push(format!("disable {uuid}"));
            Ok(self.set_state(uuid, 2.0))
        }
        fn open_prefs(&mut self, uuid: &str, _p: &str, _m: bool) -> Result<(), BusFailure> {
            self.calls.push(format!("prefs {uuid}"));
            Ok(())
        }
        fn launch_prefs(&mut self, _uuid: &str) -> Result<(), BusFailure> {
            Ok(())
        }
        fn check_for_updates(&mut self) -> Result<(), BusFailure> {
            self.calls.push("check".into());
            for u in std::mem::take(&mut self.updates_after_check) {
                if let Some(m) = self.exts.get_mut(&u) {
                    m.insert("hasUpdate".into(), Field::Bool(true));
                }
            }
            Ok(())
        }
        fn shell_version(&mut self) -> Result<String, BusFailure> {
            Ok("51.1".into())
        }
        fn user_extensions_enabled(&mut self) -> Result<bool, BusFailure> {
            Ok(true)
        }
        fn set_user_extensions_enabled(&mut self, _on: bool) -> Result<(), BusFailure> {
            Ok(())
        }
        fn shell_setting_bool(&mut self, _key: &str) -> Option<bool> {
            None
        }
        fn which(&mut self, _cmd: &str) -> bool {
            false
        }
        fn run(&mut self, argv: &[String]) -> Result<CmdOutput, String> {
            self.runs.push(argv.join(" "));
            if argv.first().map(String::as_str) == Some("gsettings") {
                if argv.last().map(String::as_str) == Some("disabled-extensions") {
                    let items: Vec<String> = self.disabled.iter().map(|u| format!("'{u}'")).collect();
                    return Ok(CmdOutput { success: true, text: format!("[{}]", items.join(", ")) });
                }
                return Ok(CmdOutput { success: true, text: format!("['{U}']") });
            }
            Ok(CmdOutput { success: true, text: String::new() })
        }
    }

    fn info(uuid: &str, state: f64, system: bool) -> ExtInfo {
        ExtInfo::from_raw(&raw(uuid, state, system)).unwrap()
    }

    #[test]
    fn ids_and_uuids() {
        assert_eq!(local_id(U), format!("local:gnomeext:{U}"));
        assert_eq!(uuid_of_id(&local_id(U)), Some(U));
        assert_eq!(uuid_of_id(&format!("ego:{U}")), Some(U));
        assert_eq!(uuid_of_id("local:gtk:Adwaita"), None);
        assert_eq!(uuid_of_id("local:gnomeext:../x"), None);
    }

    /// Кнопка переключения — по той же правде, что `toggle`: включено или
    /// запущено — «Выключить»; запертое администратором не нажимается. В
    /// подписях страницы — все ключи, которые читает QML.
    #[test]
    fn toggle_button_and_page_texts() {
        assert_eq!(toggle_button(true, false, true).text, tr("⏸️ Disable"));
        assert_eq!(toggle_button(false, true, true).state, "true");
        let off = toggle_button(false, false, true);
        assert_eq!((off.text, off.state, off.enabled), (tr("▶️ Enable"), "false", true));
        assert!(!toggle_button(true, true, false).enabled);
        let t = page_texts();
        for key in [
            "prefs", "folder_open", "folder_copy", "error_details", "uninstall", "locked_tooltip",
            "only_compatible", "only_compatible_tooltip", "install_tooltip", "error_title", "copy", "write_author",
        ] {
            assert!(t[key].as_str().is_some_and(|v| !v.is_empty()), "нет подписи {key}");
        }
        assert!(t["error_title"].as_str().unwrap().contains("%1"));
        let f = shell_fields(&info(U, 1.0, false), "51");
        assert!(f.contains_key("gnomeUrl"));
    }

    /// Помеченное к удалению делится по номеру плитки: расширение (и с
    /// диска, и с витрины) — оболочке, тема — общему снятию папок; в вопросе
    /// перед удалением — имя расширения.
    #[test]
    fn pending_removals_split_by_tile_id() {
        use crate::theme_removal::PendingRemoval;
        let p = |id: &str| PendingRemoval { path: format!("/p/{id}"), name: id.into(), item_id: id.into() };
        let (gnome, rest) = split_pending(vec![p(&local_id(U)), p("local:gtk:Arc"), p(&format!("ego:{SYS}"))]);
        assert_eq!(gnome.iter().map(|(u, _)| u.as_str()).collect::<Vec<_>>(), [U, SYS]);
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].item_id, "local:gtk:Arc");
        let (_, text) = removal_question("Dash to Dock");
        assert!(text.contains("Dash to Dock"));
    }

    #[test]
    fn snapshot_needs_running_shell() {
        assert!(shell_snapshot(&mut Shell::down()).is_none());
        let list = shell_snapshot(&mut Shell::up()).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(active_uuids(&list), vec![SYS.to_string()]);
        // Включено, но упало — «Выключить» остаётся правдой кнопки, а
        // «Активно» на плитке — нет.
        let broken = vec![info(U, 3.0, false)];
        assert!(active_uuids(&broken).is_empty());
        assert_eq!(enabled_uuids(&broken), vec![U.to_string()]);
    }

    #[test]
    fn tile_marks() {
        let mut e = info(U, 3.0, false);
        e.error = "TypeError".into();
        let f = shell_fields(&e, "51");
        assert_eq!(f["gnomeError"], true);
        assert_eq!(f["gnomeErrorText"], "TypeError");
        assert_eq!(f["gnomeCanRemove"], true);
        assert_eq!(f["gnomeFolderAction"], "open");
        assert_eq!(f["gnomeMarks"][0]["key"], "error");

        let mut old = info(U, 4.0, false);
        old.has_update = true;
        let f = shell_fields(&old, "51");
        assert_eq!(f["gnomeUnsupported"], true);
        assert_eq!(f["gnomeUnsupportedText"], crate::gnome_ego::unsupported_label("51"));
        let keys: Vec<&str> = f["gnomeMarks"].as_array().unwrap().iter().map(|m| m["key"].as_str().unwrap()).collect();
        assert_eq!(keys, ["out-of-date", "update"]);

        let sys = shell_fields(&info(SYS, 1.0, true), "51");
        assert_eq!(sys["gnomeSystem"], true);
        assert_eq!(sys["gnomeCanRemove"], false);
        assert_eq!(sys["gnomeFolderAction"], "copy");
        assert_eq!(sys["gnomeActive"], true);
        assert_eq!(sys["gnomeMarks"][0]["key"], "system");
        assert_eq!(out_of_date_label(""), tr("Doesn't support this GNOME version"));
    }

    #[test]
    fn list_from_shell_with_disk_fallback() {
        let disk = vec![ThemeItem { id: local_id(U), name: "С диска".into(), ..Default::default() }];
        let list = vec![info(U, 2.0, false), info(SYS, 1.0, true)];
        let merged = merge_installed(disk, &list);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].name, "С диска", "плитка с диска остаётся своей");
        assert_eq!(merged[1].id, local_id(SYS));
        assert_eq!(merged[1].install_path, format!("/usr/share/gnome-shell/extensions/{SYS}"));
        assert_eq!(merged[1].local_kind.as_deref(), Some("active"));

        let unseen = "new@example.org";
        let mut rows = vec![
            serde_json::json!({ "id": local_id(U) }),
            serde_json::json!({ "id": local_id(unseen) }),
            serde_json::json!({ "id": format!("ego:{SYS}"), "installed": false }),
            serde_json::json!({ "id": "ego:other@example.org", "installed": false }),
            serde_json::json!({ "id": "local:gtk:Adwaita" }),
        ];
        stamp_rows(&mut rows, &list, "51");
        assert_eq!(rows[0]["gnomeShellKnown"], true);
        assert_eq!(rows[1]["gnomeShellKnown"], false);
        assert_eq!(rows[1]["gnomeMarks"][0]["key"], "next-login");
        assert_eq!(rows[2]["installed"], true);
        assert_eq!(rows[3]["installed"], false);
        assert!(rows[3].get("gnomeShellKnown").is_none());
        assert!(rows[4].get("gnomeShellKnown").is_none());
    }

    /// Выключенное расширение подписано «Выключено», а не происхождением;
    /// включённое — «Активно»; вид плитки остаётся.
    #[test]
    fn installed_tile_says_disabled() {
        let off = info(U, 2.0, false);
        let on = info(SYS, 1.0, true);
        let disabled = crate::qt_extensions::state_disabled_text();
        let enabled = crate::qt_extensions::state_enabled_text();
        assert_eq!(state_badge(&off), Some(disabled.clone()));
        assert_eq!(state_badge(&on), Some(enabled.clone()));
        // Включено, но с ошибкой — подпись не трогаем (скажут пометки).
        let broken = info("broken@example.org", 3.0, false);
        assert_eq!(state_badge(&broken), None);
        // Подпись скана у плитки с диска (происхождение) — любая, её место
        // занимает состояние.
        let manual = format!("🧩 {} · 🗂️ manual", tr("GNOME Shell extension"));
        assert_eq!(
            author_with_state(&manual, &off).unwrap(),
            format!("🧩 {} · {disabled}", tr("GNOME Shell extension"))
        );
        assert_eq!(author_with_state("", &on).unwrap(), enabled);
        let mut rows = vec![
            serde_json::json!({ "id": local_id(U), "author": manual }),
            serde_json::json!({ "id": format!("ego:{U}"), "author": "Автор витрины" }),
        ];
        stamp_rows(&mut rows, &[off.clone()], "51");
        assert!(rows[0]["author"].as_str().unwrap().ends_with(&disabled), "{}", rows[0]["author"]);
        assert_eq!(rows[1]["author"], "Автор витрины", "у карточки витрины подпись — автор");
        // Плитка только со слов оболочки — тот же вид.
        assert_eq!(item_from_info(&off).author, format!("🧩 {} · {disabled}", tr("GNOME Shell extension")));
        assert_eq!(item_from_info(&on).author, format!("🧩 {} · {enabled}", tr("GNOME Shell extension")));
    }

    /// Установленному — картинка витрины, если она уже есть; своя и чужие
    /// плитки не трогаются; нет картинки — остаётся заглушка.
    #[test]
    fn installed_tile_takes_showcase_picture() {
        let mut rows = vec![
            serde_json::json!({ "id": local_id(U), "preview_local": "" }),
            serde_json::json!({ "id": local_id(SYS), "preview_local": "/own.png" }),
            serde_json::json!({ "id": local_id("none@example.org") }),
            serde_json::json!({ "id": format!("ego:{U}") }),
            serde_json::json!({ "id": "local:gtk:Adwaita" }),
        ];
        let files = |uuid: &str| (uuid != "none@example.org").then(|| format!("/cache/{uuid}.png"));
        stamp_ego_pictures(&mut rows, &files);
        assert_eq!(rows[0]["preview_local"], format!("/cache/{U}.png"));
        assert_eq!(rows[1]["preview_local"], "/own.png", "своя картинка важнее");
        assert!(rows[2].get("preview_local").is_none(), "нет картинки — заглушка");
        assert!(rows[3].get("preview_local").is_none(), "карточку витрины не трогаем");
        assert!(rows[4].get("preview_local").is_none());
    }

    #[test]
    fn install_through_shell() {
        let mut io = Shell::up();
        let mut never = || -> Result<PathBuf, String> { panic!("при оболочке zip не качается") };
        let new = "fresh@example.org";
        let r = install(&mut io, new, "/h/.local/share", &mut never).unwrap();
        assert_eq!(r, InstallOutcome::Installed);
        assert_eq!(io.calls, [format!("install {new}")]);
        let (ok, msg) = install_message("Fresh", r);
        assert!(ok);
        assert!(!msg.contains(&gse::next_login_note()), "после установки через оболочку перезаход не нужен: {msg}");
    }

    #[test]
    fn install_cancelled_and_refused() {
        let mut io = Shell { install_reply: Some(Ok(gse::INSTALL_CANCELLED.into())), ..Shell::up() };
        let mut never = || -> Result<PathBuf, String> { unreachable!() };
        let r = install(&mut io, "c@example.org", "/h", &mut never).unwrap();
        assert_eq!(r, InstallOutcome::Cancelled);
        let (ok, msg) = install_message("C", r);
        assert!(ok, "отмена — не ошибка");
        assert!(msg.contains('C'));
        let mut io = Shell {
            install_reply: Some(Err(BusFailure::Method { name: "org.gnome.Shell.Error".into(), message: "nope".into() })),
            ..Shell::up()
        };
        let e = install(&mut io, "c@example.org", "/h", &mut never).unwrap_err();
        assert_eq!(e, ShellExtError::Bus("nope".into()));
    }

    #[test]
    fn install_without_shell_uses_zip() {
        let mut io = Shell::down();
        let mut asked = 0;
        let mut fail = || -> Result<PathBuf, String> {
            asked += 1;
            Err("нет сети".into())
        };
        let e = install(&mut io, U, "/h", &mut fail).unwrap_err();
        assert_eq!(asked, 1, "без оболочки — запасной путь");
        assert_eq!(e, ShellExtError::Offline("нет сети".into()));
        assert!(io.calls.is_empty(), "оболочку не зовём");
        let mut never = || -> Result<PathBuf, String> { unreachable!() };
        assert!(matches!(install(&mut io, "../x", "/h", &mut never), Err(ShellExtError::BadUuid(_))));
        let (_, msg) = install_message("X", InstallOutcome::InstalledNextLogin);
        assert!(msg.contains(&gse::next_login_note()));
    }

    #[test]
    fn offline_release_choice() {
        use crate::gnome_ego::{parse_shell_version, Release, ReleaseShell};
        let rel = |version, status, major| Release {
            version,
            status,
            shell_versions: vec![ReleaseShell { major, minor: -1, point: -1 }],
            ..Default::default()
        };
        let list = vec![rel(10, 3, 50), rel(11, 3, 51), rel(12, 1, 51), rel(13, 3, 52)];
        let v51 = parse_shell_version("51.1").unwrap();
        assert_eq!(offline_release(&list, Some(&v51)), Some(11));
        assert_eq!(offline_release(&list, None), Some(13));
        let v40 = parse_shell_version("40").unwrap();
        assert_eq!(offline_release(&list, Some(&v40)), None);
    }

    /// Зависимость пакета оформления: стоит — пропуск без единого вызова;
    /// нет — сразу через оболочку; отказ в окне — итог, а не ошибка; нет
    /// выпуска под версию GNOME — окна нет, причина названа; без оболочки —
    /// запасной путь.
    #[test]
    fn dependency_install_paths() {
        const NEW: &str = "dash-to-panel@jderose9.github.com";
        let no = |_: &str| false;
        let mut fine = || None;
        let mut never = || -> Result<PathBuf, String> { unreachable!("zip при оболочке") };

        let mut io = Shell::up();
        let disk = |u: &str| u == NEW;
        assert_eq!(install_dependency(&mut io, NEW, "/h", &disk, &mut fine, &mut never), Ok(DepOutcome::AlreadyInstalled));
        assert_eq!(install_dependency(&mut io, U, "/h", &no, &mut fine, &mut never), Ok(DepOutcome::AlreadyInstalled));
        assert!(io.calls.is_empty(), "{:?}", io.calls);

        let r = install_dependency(&mut io, NEW, "/h", &no, &mut fine, &mut never);
        assert_eq!(r, Ok(DepOutcome::Done(InstallOutcome::Installed)));
        assert_eq!(io.calls, [format!("install {NEW}")]);
        assert!(dependency_note(NEW, "Panel", &r).unwrap().contains(NEW));
        assert_eq!(dependency_note(NEW, "Panel", &Ok(DepOutcome::AlreadyInstalled)), None);

        let mut io = Shell::up();
        io.install_reply = Some(Ok(gse::INSTALL_CANCELLED.into()));
        let r = install_dependency(&mut io, NEW, "/h", &no, &mut fine, &mut never);
        assert_eq!(r, Ok(DepOutcome::Done(InstallOutcome::Cancelled)));
        let note = dependency_note(NEW, "Panel", &r).unwrap();
        assert!(note.contains("cancelled") && note.contains("Panel"), "{note}");

        let mut io = Shell::up();
        let mut old = || Some("no release for your GNOME".to_string());
        let r = install_dependency(&mut io, NEW, "/h", &no, &mut old, &mut never);
        assert_eq!(r, Err(ShellExtError::Offline("no release for your GNOME".into())));
        assert!(io.calls.is_empty(), "окно оболочки не показывается: {:?}", io.calls);
        assert!(dependency_note(NEW, "", &r).unwrap().contains("no release for your GNOME"));

        let mut io = Shell::down();
        let mut fetched = 0;
        let mut offline = || -> Result<PathBuf, String> {
            fetched += 1;
            Err("сеть недоступна".into())
        };
        let r = install_dependency(&mut io, NEW, "/h", &no, &mut fine, &mut offline);
        assert_eq!(r, Err(ShellExtError::Offline("сеть недоступна".into())));
        assert_eq!(fetched, 1, "без оболочки — zip выпуска");

        let mut io = Shell::up();
        assert!(matches!(
            install_dependency(&mut io, "../x", "/h", &no, &mut fine, &mut never),
            Err(ShellExtError::BadUuid(_))
        ));
    }

    #[test]
    fn dependency_unsupported_reason() {
        use crate::gnome_ego::{parse_shell_version, Release, ReleaseShell};
        let rel = |version, major| Release {
            version,
            status: crate::gnome_ego::STATUS_ACTIVE,
            shell_versions: vec![ReleaseShell { major, minor: -1, point: -1 }],
            ..Default::default()
        };
        let list = vec![rel(10, 50), rel(11, 51)];
        assert_eq!(unsupported_reason(&list, Some(&parse_shell_version("51.2").unwrap())), None);
        assert!(unsupported_reason(&list, Some(&parse_shell_version("49").unwrap())).is_some());
        assert_eq!(unsupported_reason(&list, None), None, "версию не узнать — решает оболочка");
    }

    #[test]
    fn enable_disable_via_bus_and_script() {
        let mut io = Shell::up();
        let msg = toggle(&mut io, U, "/h").unwrap();
        assert!(msg.contains(U));
        assert_eq!(io.calls, [format!("enable {U}")]);
        toggle(&mut io, U, "/h").unwrap();
        assert_eq!(io.calls.last().unwrap(), &format!("disable {U}"));
        assert!(io.runs.is_empty(), "при оболочке сценарий не нужен");
        assert_eq!(toggle(&mut io, "missing@example.org", "/h"), Err(ShellExtError::NotInstalled));

        // Без оболочки — сценарий; в `enabled-extensions` уже есть U → выключить.
        let mut down = Shell::down();
        let msg = toggle(&mut down, U, "/h").unwrap();
        assert!(msg.contains(&tr("takes effect the next time you log in to GNOME")));
        assert!(down.runs[0].starts_with("gsettings get"));
        assert!(down.runs[1].ends_with("disabled-extensions"));
        assert!(down.runs[2].starts_with("sh -c"));
        assert!(down.runs[2].contains("disabled-extensions"), "выключение ставит uuid в disabled-extensions");
        set_enabled(&mut down, "b@example.org", true, "/h").unwrap();
        assert!(down.runs.last().unwrap().contains("b@example.org"));

        // Есть и в `disabled-extensions` (он сильнее) — значит выключено:
        // переключатель включает.
        let mut down = Shell { disabled: vec![U.to_string()], ..Shell::down() };
        toggle(&mut down, U, "/h").unwrap();
        assert!(down.runs.last().unwrap().contains("gnome-extensions enable"), "{:?}", down.runs);
    }

    #[test]
    fn uninstall_only_own() {
        let mut io = Shell::up();
        assert_eq!(uninstall_own(&mut io, SYS, "/h"), Err(ShellExtError::SystemExtension));
        assert!(!io.calls.iter().any(|c| c.starts_with("uninstall")));
        uninstall_own(&mut io, U, "/h").unwrap();
        assert_eq!(io.calls, [format!("uninstall {U}")]);
        assert_eq!(uninstall_own(&mut io, U, "/h"), Err(ShellExtError::NotInstalled));

        // Без оболочки: своё — каталог в data_home, чужого там нет.
        let data = std::env::temp_dir().join(format!("rexon-gext-{}", std::process::id()));
        let dir = gse::user_extension_dir(&data.to_string_lossy(), U);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("metadata.json"), "{}").unwrap();
        let mut down = Shell::down();
        assert_eq!(uninstall_own(&mut down, SYS, &data.to_string_lossy()), Err(ShellExtError::SystemExtension));
        uninstall_own(&mut down, U, &data.to_string_lossy()).unwrap();
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn folder_and_error_details() {
        let mut io = Shell::up();
        assert_eq!(
            folder_for(&mut io, SYS, "/h/.local/share", ""),
            Some(FolderAction::Copy(format!("/usr/share/gnome-shell/extensions/{SYS}")))
        );
        assert_eq!(folder_for(&mut io, U, "/h/.local/share", "").unwrap().key(), "open");
        let mut down = Shell::down();
        let own = format!("/h/.local/share/gnome-shell/extensions/{U}");
        assert_eq!(folder_for(&mut down, U, "/h/.local/share", &own), Some(FolderAction::Open(own)));
        let sys = format!("/usr/share/gnome-shell/extensions/{SYS}");
        assert_eq!(folder_for(&mut down, SYS, "/h/.local/share", &sys), Some(FolderAction::Copy(sys)));

        if let Some(m) = io.exts.get_mut(U) {
            m.insert("error".into(), Field::Str("boom".into()));
        }
        assert_eq!(error_details(&mut io, U).unwrap(), "boom\nTypeError: x is undefined");
        let a = run_action(&mut io, Action::ErrorDetails, U, "/h", "");
        assert_eq!(a["ok"], true);
        assert!(a["details"].as_str().unwrap().contains("boom"));
        assert_eq!(a["name"], format!("Name {U}"), "заголовок окна — имя расширения");
        let a = run_action(&mut down, Action::ErrorDetails, U, "/h", "");
        assert_eq!(a["ok"], false);
        assert_eq!(a["message"], ShellExtError::ShellNotRunning.message());
    }

    #[test]
    fn run_action_dispatch() {
        let mut io = Shell::up();
        assert_eq!(Action::parse("toggle"), Some(Action::Toggle));
        assert_eq!(Action::parse("nonsense"), None);
        let a = run_action(&mut io, Action::Prefs, U, "/h", "");
        assert_eq!(a["ok"], true);
        assert_eq!(io.calls.last().unwrap(), &format!("prefs {U}"));
        let a = run_action(&mut io, Action::Folder, SYS, "/h", "");
        assert_eq!(a["folder"], "copy");
        assert!(a["message"].as_str().unwrap().contains(SYS));
        let a = run_action(&mut io, Action::Uninstall, SYS, "/h", "");
        assert_eq!(a["ok"], false);
        assert_eq!(a["action"], "uninstall");
    }

    #[test]
    fn journal_entry_is_ego() {
        let item = journal_item(U, "Sample", "7", "author");
        assert_eq!(item.content_id, format!("ego:{U}"));
        assert_eq!(item.provider_id, "ego");
        assert_eq!(item.rexon_type, KIND);
        let mut log = Map::new();
        journal_record(&mut log, &item, "/h/.local/share", U, "2026-11-14T00:00:00");
        let rec = &log[&format!("ego:{U}")];
        assert_eq!(rec["provider_id"], "ego");
        assert_eq!(rec["version"], "7");
        assert_eq!(rec["paths"][0], format!("/h/.local/share/gnome-shell/extensions/{U}"));
        assert!(journal_forget(&mut log, U));
        assert!(!journal_forget(&mut log, U));
    }

    #[test]
    fn updates_counter() {
        assert_eq!(updates_status(0), "");
        assert!(updates_status(3).contains('3'));
        let mut io = Shell { updates_after_check: vec![U.into()], ..Shell::up() };
        let mut seen = Vec::new();
        let mut rounds = 0;
        let n = check_updates(&mut io, 2, &mut || {
            rounds += 1;
            true
        }, &mut |n| seen.push(n))
        .unwrap();
        assert_eq!(n, 1);
        assert_eq!(seen, [1], "одно и то же число не повторяется");
        assert_eq!(rounds, 2);
        assert_eq!(io.calls[0], "check");
        let a = updates_answer(1);
        assert_eq!(a["count"], 1);
        assert_eq!(a["logoutLabel"], logout_label());
        assert_eq!(check_updates(&mut Shell::down(), 1, &mut || true, &mut |_| {}), Err(ShellExtError::ShellNotRunning));
        assert_eq!(LOGOUT_ARGV, ["gnome-session-quit", "--logout"]);
    }
}
