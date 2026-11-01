//! ExtensionManager — порт `rexon/extensions/manager.py` (чистая часть).
//!
//! Портируется ЛОГИКА менеджера расширений без исполнения:
//!   * `is_safe_sys_name` — allowlist имён для системных путей под root;
//!   * `find_manifest_root` + `install_rxn_check` — весь валидационный
//!     конвейер `_install_rxn` ДО копирования (v4-манифест с выводом id из
//!     title, path-traversal-проверки id, миграция v2, VALID_TYPES,
//!     обязательность install.sh/uninstall.sh, `_check_rexon_compat`,
//!     «уже установлено») — тексты ошибок байт-в-байт с Python;
//!   * `safe_install_plan` / `safe_uninstall_plan` — установка/удаление
//!     типов оформления как ЧИСТЫЙ ПЛАН шагов (`ExtStep`) по образцу
//!     `system_plan.rs`: копирования, симлинки, записи файлов (тексты
//!     сгенерированных конфигов 1-в-1), команды с таймаутами, pkexec-скрипты
//!     (тексты 1-в-1), staging под /tmp. Эталон паритета — ЗАПИСАННЫЕ
//!     побочные вызовы (shutil/os/subprocess) живого Python на тех же
//!     входах (см. `ext_manager_parity.rs`);
//!   * `detect_display_manager_plan` — политика выбора DM (сами probes —
//!     шаги плана, их результат подаётся замыканием);
//!   * `find_background` — поиск фона в data/ (по снимку с размерами);
//!   * `discover` — скан каталога расширений (дерево установленного);
//!   * `stats` / `save_state_text` / `load_state` — каталожные структуры и
//!     состояние вкл/выкл (текст JSON байт-в-байт с `json.dump(indent=2)`).
//!
//! ФАЙЛОВАЯ СИСТЕМА — ЗА ИНТЕРФЕЙСОМ (как installed_scan.rs): вход —
//! `FsSnap`, снимок дерева «как его видел Python-эталон» (порядок имён =
//! порядок os.scandir/os.listdir, включая каталоги, которые Python читал
//! ПОСЛЕ собственных копирований — снимок хранит то, что видел оригинал).
//! Поверх снимка — `PlanFs`: оверлей путей, СОЗДАВАЕМЫХ самим планом
//! (copytree/makedirs/write), чтобы проверки exists/isdir внутри плана
//! видели свои же шаги.
//!
//! НЕ портировано (исполнение — остаётся Python): threading.Lock, реальные
//! копирования/удаления, subprocess/pkexec/notify-send/pkill и чтение их
//! вывода, tempfile (пути staged/tmp детерминированно выдаёт `TmpAlloc` —
//! исполнитель подставляет реальные), импортёры Windows-форматов
//! (`importers.py`), запуск install.sh/uninstall.sh и `.desktop`-установка
//! (`extension.py`, исполнение), load/unload плагинов (importlib),
//! `_detect_display_manager` как процесс (systemctl/which — probes в плане,
//! ответы инъекцией), `_staged_under_tmp` как копирование (шаг `Stage`).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::constants::{self, is_supported_font};
use crate::de_customization;
use crate::extension::Extension;
use crate::i18n::{tr, trf};
use crate::manifest_v4::validate_v4_manifest;
use crate::system_plan::privileged_stage_cmd;

// ============================================================================
// FsSnap — снимок файловой системы (с размерами файлов и симлинками)
// ============================================================================

/// Файл снимка: размер (для `_find_background`) и текст (None — бинарный).
struct FileEntry {
    size: u64,
    text: Option<String>,
}

/// Снимок дерева в ДВУХ слоях:
///   * pre — состояние ДО прогона (по нему принимаются решения плана:
///     exists/isdir «существовало ли до нас»);
///   * observed (dirs/files целиком) — плюс то, что Python прочитал ПОСЛЕ
///     собственных копирований (листинги dest-каталогов, распакованные
///     .plasmoid) — по нему план читает содержимое своих же шагов.
///
/// Каталог → имена в порядке os.scandir эталона; файл → (размер, текст);
/// links — пути-симлинки пре-состояния (для lexists/islink).
pub struct FsSnap {
    dirs: HashMap<String, Vec<String>>,
    files: HashMap<String, FileEntry>,
    links: HashSet<String>,
    pre_dirs: HashSet<String>,
    pre_files: HashSet<String>,
    /// Одиночные файлы, которые общий журнал записал за ЭТИМ пакетом
    /// (`owned_files` снимка): их установка заменяет, удаление снимает.
    owned_files: HashSet<String>,
    /// Пары `(файл пакета, файл на месте)`, совпадающие байт в байт
    /// (`same_files` снимка): такой файл на месте считается своим.
    same_files: HashSet<(String, String)>,
}

impl FsSnap {
    /// Из JSON-фикстуры `{"dirs": {p: [names]}, "files": {p: {"size": n,
    /// "text": str|null}}, "links": [p], "pre_dirs": [p], "pre_files": [p],
    /// "owned_files": [p], "same_files": [[src, dest]]}`.
    /// Без pre-списков весь снимок считается пре-состоянием.
    pub fn from_json(v: &Value) -> Self {
        let mut dirs = HashMap::new();
        if let Some(obj) = v["dirs"].as_object() {
            for (k, names) in obj {
                let list = names
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|n| n.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                dirs.insert(k.clone(), list);
            }
        }
        let mut files = HashMap::new();
        if let Some(obj) = v["files"].as_object() {
            for (k, meta) in obj {
                files.insert(
                    k.clone(),
                    FileEntry {
                        size: meta["size"].as_u64().unwrap_or(0),
                        text: meta["text"].as_str().map(String::from),
                    },
                );
            }
        }
        let str_set = |key: &str| -> Option<HashSet<String>> {
            v[key].as_array().map(|a| {
                a.iter()
                    .filter_map(|n| n.as_str().map(String::from))
                    .collect()
            })
        };
        let links = str_set("links").unwrap_or_default();
        let pre_dirs = str_set("pre_dirs")
            .unwrap_or_else(|| dirs.keys().cloned().collect());
        let pre_files = str_set("pre_files")
            .unwrap_or_else(|| files.keys().cloned().collect());
        let owned_files = str_set("owned_files").unwrap_or_default();
        let same_files = v["same_files"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|pair| {
                        Some((pair[0].as_str()?.to_string(), pair[1].as_str()?.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        FsSnap { dirs, files, links, pre_dirs, pre_files, owned_files, same_files }
    }

    /// Файл `dest` записан общим журналом за этим пакетом.
    pub fn owns_file(&self, dest: &str) -> bool {
        self.owned_files.contains(dest)
    }

    /// Файл на месте `dest` — байт в байт файл пакета `src`: пара из снимка
    /// либо (в фикстурах с текстами) одинаковые тексты одного размера.
    pub fn same_bytes(&self, src: &str, dest: &str) -> bool {
        if self.same_files.contains(&(src.to_string(), dest.to_string())) {
            return true;
        }
        match (self.files.get(src), self.files.get(dest)) {
            (Some(a), Some(b)) => a.size == b.size && a.text.is_some() && a.text == b.text,
            _ => false,
        }
    }

    /// Каталог существовал ДО прогона (`os.path.isdir` в решениях плана).
    pub fn is_dir(&self, path: &str) -> bool {
        self.pre_dirs.contains(path)
    }

    /// Файл существовал ДО прогона (`os.path.isfile` в решениях плана).
    pub fn is_file(&self, path: &str) -> bool {
        self.pre_files.contains(path)
    }

    pub fn exists(&self, path: &str) -> bool {
        self.is_dir(path) || self.is_file(path)
    }

    /// `os.path.lexists` — существует сам путь, включая битые симлинки.
    pub fn lexists(&self, path: &str) -> bool {
        self.exists(path) || self.links.contains(path)
    }

    /// Каталог есть в наблюдениях (включая созданные самим прогоном —
    /// например, dest после copytree).
    pub fn obs_is_dir(&self, path: &str) -> bool {
        self.dirs.contains_key(path)
    }

    /// Файл есть в наблюдениях.
    pub fn obs_is_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn is_link(&self, path: &str) -> bool {
        self.links.contains(path)
    }

    pub fn read_text(&self, path: &str) -> Option<&str> {
        self.files.get(path).and_then(|f| f.text.as_deref())
    }

    pub fn size(&self, path: &str) -> u64 {
        self.files.get(path).map(|f| f.size).unwrap_or(0)
    }

    /// Имена элементов каталога в порядке снимка ([] — каталога нет).
    pub fn scandir(&self, path: &str) -> &[String] {
        self.dirs.get(path).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Эмуляция `os.walk` (topdown): (root, подкаталоги, файлы), классы —
    /// в порядке листинга снимка.
    pub fn walk(&self, root: &str) -> Vec<(String, Vec<String>, Vec<String>)> {
        let mut out = Vec::new();
        self.walk_into(root, &mut out);
        out
    }

    fn walk_into(&self, root: &str, out: &mut Vec<(String, Vec<String>, Vec<String>)>) {
        if !self.obs_is_dir(root) {
            return;
        }
        let mut subdirs = Vec::new();
        let mut files = Vec::new();
        for name in self.scandir(root) {
            if self.obs_is_dir(&join(root, name)) {
                subdirs.push(name.clone());
            } else {
                files.push(name.clone());
            }
        }
        out.push((root.to_string(), subdirs.clone(), files));
        for d in subdirs {
            self.walk_into(&join(root, &d), out);
        }
    }
}

fn join(dir: &str, rel: &str) -> String {
    format!("{dir}/{rel}")
}

/// Оверлей поверх снимка: пути, которые СОЗДАЁТ сам план (copytree/makedirs/
/// write), видны последующим проверкам exists/isdir — как их видел Python,
/// реально исполнявший копирования по ходу `_safe_install`.
struct PlanFs<'a> {
    snap: &'a FsSnap,
    made_dirs: HashSet<String>,
    made_files: HashSet<String>,
    /// Сколько основных частей план пропустил: на месте чужое (заметки
    /// [`taken_by_other_notice`]/[`taken_file_notice`]). Пропущено всё —
    /// установка отказывает целиком ([`ExtStep::Refuse`]).
    skipped: usize,
}

impl<'a> PlanFs<'a> {
    fn new(snap: &'a FsSnap) -> Self {
        PlanFs { snap, made_dirs: HashSet::new(), made_files: HashSet::new(), skipped: 0 }
    }

    fn mk_dir(&mut self, path: &str) {
        self.made_dirs.insert(path.to_string());
    }

    fn mk_file(&mut self, path: &str) {
        self.made_files.insert(path.to_string());
    }

    fn is_dir(&self, path: &str) -> bool {
        self.snap.is_dir(path) || self.made_dirs.contains(path)
    }

    fn exists(&self, path: &str) -> bool {
        self.snap.exists(path)
            || self.made_dirs.contains(path)
            || self.made_files.contains(path)
    }
}

// ============================================================================
// Шаги плана
// ============================================================================

/// Шаг плана установки/удаления расширения — побочный вызов, который сделал
/// бы Python (`shutil`/`os`/`subprocess`). Исполнение — при подключении к
/// живому Python.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op")]
pub enum ExtStep {
    /// `os.makedirs(path, exist_ok=True)`.
    MakeDirs { path: String },
    /// `shutil.copytree(src, dest)`.
    CopyTree { src: String, dest: String },
    /// `shutil.copy2(src, dest)` (dest может быть каталогом — как в Python).
    CopyFile { src: String, dest: String },
    /// Удаление дерева: `safe` — через `_safe_rmtree` (проверки границ $HOME),
    /// иначе `shutil.rmtree` (`ignore_errors` — как в вызове оригинала).
    RemoveTree { path: String, safe: bool, ignore_errors: bool },
    /// `os.remove(path)`.
    RemoveFile { path: String },
    /// `os.symlink(target, link)`.
    Symlink { target: String, link: String },
    /// Запись файла с указанным содержимым (тексты 1-в-1 с Python).
    WriteFile { path: String, content: String },
    /// Распаковка ZIP через `_safe_zip_extract` (напр. `.plasmoid`).
    ExtractZip { archive: String, dest: String },
    /// `_staged_under_tmp`: скопировать `sources` в staged-каталог под /tmp
    /// (каталоги — содержимым, файлы — как есть).
    Stage { staged: String, sources: Vec<String> },
    /// `_run_pkexec_script`: записать скрипт во временный файл
    /// `rexon_{prefix}_*.sh` и выполнить `pkexec bash <файл>` (timeout 60).
    PkexecScript { prefix: String, script: String },
    /// `subprocess.run(argv, capture_output=True, timeout=…)`.
    Run { argv: Vec<String>, timeout_secs: Option<u64> },
    /// `subprocess.Popen(argv, …)` — запуск без ожидания.
    Spawn { argv: Vec<String> },
    /// Команда, чей отказ человеку надо сказать: не удалась — `failed` идёт
    /// в заметки установки (`Installed::notes`), установка продолжается.
    RunNoted { argv: Vec<String>, timeout_secs: Option<u64>, failed: String },
    /// Сказать человеку после установки, что часть сделана не была и
    /// почему (`Installed::notes`); ничего не исполняет.
    Notice { text: String },
    /// Отказ установки целиком, ещё до первого шага: всё основное пакета
    /// пропущено (на месте чужое, см. [`put_owned_copy`]/[`put_owned_file`]).
    /// `text` — причина для человека; исполнитель отвечает ею ошибкой.
    Refuse { text: String },
}

fn run(argv: Vec<&str>, timeout: u64) -> ExtStep {
    ExtStep::Run {
        argv: argv.into_iter().map(str::to_string).collect(),
        timeout_secs: Some(timeout),
    }
}

// ============================================================================
// Окружение плана
// ============================================================================

/// Детерминированный поставщик tmp-путей (замена tempfile.mkdtemp): пути
/// вида `{base}/{prefix}{N}` — исполнитель на живой системе подставляет
/// реальные mkdtemp-каталоги в том же порядке.
pub struct TmpAlloc {
    base: String,
    n: Cell<u64>,
}

impl TmpAlloc {
    pub fn new(base: &str) -> Self {
        TmpAlloc { base: base.to_string(), n: Cell::new(0) }
    }

    pub fn mkdtemp(&self, prefix: &str) -> String {
        let n = self.n.get();
        self.n.set(n + 1);
        format!("{}/{}{}", self.base, prefix, n)
    }

    /// Постоянный путь `{base}/{name}` под каталогом плана — без счётчика
    /// (пробы, которые снимок готовит заранее: [`PLASMOID_PROBE_DIR`]).
    pub fn path(&self, name: &str) -> String {
        format!("{}/{}", self.base, name)
    }
}

/// Каталог под каталогом плана, куда снимок (`ext_exec::plan_snapshot`)
/// заранее распаковывает архив `.plasmoid` пакета — в подкаталог с именем
/// файла архива. По нему план узнаёт `KPlugin.Id` ДО запуска `kpackagetool`
/// (тот при обновлении заменил бы чужой плазмоид с тем же id).
pub const PLASMOID_PROBE_DIR: &str = "rexon_plasmoid_probe";

/// Окружение `_safe_install`/`_safe_uninstall` — всё внешнее инъекцией.
pub struct ExtEnv<'a> {
    /// $HOME (как его видел эталон; INSTALL_PATHS строятся от него).
    pub home: &'a str,
    pub fs: &'a FsSnap,
    /// `shutil.which(name)` → путь или None.
    pub which: &'a dyn Fn(&str) -> Option<String>,
    /// Успешен ли (returncode == 0) записанный прогон argv — для веток,
    /// зависящих от результата команды (kpackagetool -i, systemctl is-active).
    pub run_ok: &'a dyn Fn(&[String]) -> bool,
    /// `$XDG_CURRENT_DESKTOP` (сырой, lower() делает сама логика).
    pub xdg_current_desktop: &'a str,
    /// `$XDG_SESSION_TYPE`.
    pub xdg_session_type: &'a str,
    /// Задан ли `$WAYLAND_DISPLAY`.
    pub wayland_display: bool,
    /// Задеплоен ли rexon-stage-helper (org.rexon.stage).
    pub stage_helper_available: bool,
    /// Путь SYSTEM_HELPER (org.rexon.system-install) и есть ли он на диске.
    pub system_helper_path: &'a str,
    pub system_helper_exists: bool,
    /// Семейство для argv системного helper'а: "debian" либо "arch"
    /// (Python: DEBIAN → "debian", всё остальное → "arch").
    pub helper_family: &'a str,
    pub tmp: &'a TmpAlloc,
}

impl ExtEnv<'_> {
    /// INSTALL_PATHS[t] от инъектированного $HOME. Таблица суффиксов одна —
    /// `constants::INSTALL_PATH_SUFFIXES` (второй копии здесь больше нет).
    fn install_path(&self, t: &str) -> Option<String> {
        constants::install_path_suffix(t).map(|s| format!("{}/{}", self.home, s))
    }

    /// Доступен ли системный helper (`shutil.which("pkexec")` + файл).
    fn system_helper_ok(&self) -> bool {
        (self.which)("pkexec").is_some() && self.system_helper_exists
    }
}

/// Расширение как вход плана: каталог + разобранный манифест.
pub struct ExtInfo<'a> {
    pub dir: &'a str,
    pub manifest: &'a Map<String, Value>,
}

impl ExtInfo<'_> {
    fn get_str(&self, key: &str, default: &'static str) -> String {
        self.manifest
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(default)
            .to_string()
    }

    pub fn ext_id(&self) -> String {
        self.get_str("id", "")
    }

    pub fn ext_type(&self) -> String {
        self.get_str("type", "plugin")
    }

    pub fn name(&self) -> String {
        match self.manifest.get("name").and_then(Value::as_str) {
            Some(name) => name.to_string(),
            None => crate::i18n::trc("extension name", "Untitled"),
        }
    }

    pub fn description(&self) -> String {
        self.get_str("description", "")
    }
}

// ============================================================================
// is_safe_sys_name — allowlist имён для системных путей под root
// ============================================================================

/// Порт `_is_safe_sys_name`: только `[A-Za-z0-9._-]+`, без ведущего
/// `-`/`.`/`/` и без `..` (anti path-traversal / flag-инъекция).
pub fn is_safe_sys_name(name: &str) -> bool {
    if name.is_empty()
        || name.starts_with('-')
        || name.starts_with('.')
        || name.starts_with('/')
        || name.contains("..")
    {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

// ============================================================================
// Валидационный конвейер установки (_install_rxn до копирования)
// ============================================================================

/// Корень с manifest.json: сам tmp_dir либо его подкаталог первого уровня —
/// порт `_find_manifest_root`.
pub fn find_manifest_root(fs: &FsSnap, tmp_dir: &str) -> Option<String> {
    // Распакованное дерево .rxn — «наблюдения» (создано в ходе прогона).
    if fs.obs_is_file(&join(tmp_dir, "manifest.json")) {
        return Some(tmp_dir.to_string());
    }
    for name in fs.scandir(tmp_dir) {
        let sub = join(tmp_dir, name);
        if fs.obs_is_dir(&sub) && fs.obs_is_file(&join(&sub, "manifest.json")) {
            return Some(sub);
        }
    }
    None
}

/// Ошибка установки: тип Python-исключения + текст (байт-в-байт, кроме
/// BadJson — деталь JSONDecodeError у serde другая, сверяется префикс).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallError {
    /// "FileNotFoundError" | "ValueError" | "RuntimeError" | "FileExistsError"
    pub kind: &'static str,
    pub message: String,
}

fn verr(message: String) -> InstallError {
    InstallError { kind: "ValueError", message }
}

/// Результат успешной проверки.
pub struct InstallCheck {
    pub ext_id: String,
    pub ext_type: String,
    /// Требует install.sh/uninstall.sh (типы со скриптами).
    pub is_scripted: bool,
    /// Манифест после возможных правок (v4: выведенные id/name; v2: тип).
    pub manifest: Value,
    /// Текст manifest.json, если Python его переписал (json.dump indent=2,
    /// ensure_ascii=False — байт-в-байт), иначе None.
    pub rewritten_manifest_text: Option<String>,
}

/// Миграция типов v2 → v3 — порт `V2_TYPE_MIGRATION` (пуст с 2026-06-12,
/// оставлен на случай будущих миграций — как в Python).
pub const V2_TYPE_MIGRATION: &[(&str, &str)] = &[];

/// Порт валидации `_install_rxn` (от разбора манифеста до `shutil.copytree`,
/// не включая её). `manifest` — уже разобранный JSON из `src_dir`
/// (ошибку разбора вызывающий оборачивает в текст
/// `manifest.json повреждён: …` через [`bad_manifest_json_error`]).
///
/// Инъекции:
///   * `realpath` — `os.path.realpath` (символические ссылки — за
///     исполнителем; для несуществующих путей достаточно лексической
///     нормализации);
///   * `dest_exists` — существует ли `extensions_dir/<ext_id>`.
pub fn install_rxn_check(
    fs: &FsSnap,
    src_dir: &str,
    extensions_dir: &str,
    manifest: Value,
    realpath: &dyn Fn(&str) -> String,
    dest_exists: &dyn Fn(&str) -> bool,
) -> Result<InstallCheck, InstallError> {
    let mut meta = match manifest {
        Value::Object(m) => m,
        // Патологический не-dict манифест: Python упал бы на meta.get —
        // реальные .rxn всегда объект (как в extension.rs).
        _ => return Err(verr(missing_id())),
    };
    let mut rewritten = false;

    // ── v4 adaptive wallpaper: schema-валидация + вывод id из title ──
    if v4_fill_id_and_name(&mut meta)? {
        rewritten = true;
    }

    // ── id: наличие + анти-traversal ──
    let ext_id = manifest_ext_id(&meta)?;
    let dest_dir = join(extensions_dir, &ext_id);
    let real_dest = realpath(&dest_dir);
    let real_base = realpath(extensions_dir);
    if !real_dest.starts_with(&format!("{real_base}/")) {
        return Err(verr(trf(
            "manifest.json: id '%1' points outside the extensions folder",
            &[&ext_id],
        )));
    }

    // ── тип: миграция v2 → v3 + допустимость ──
    let mut ext_type = meta
        .get("type")
        .map(py_str)
        .unwrap_or_default();
    if let Some((_, to)) = V2_TYPE_MIGRATION.iter().find(|(from, _)| *from == ext_type) {
        ext_type = to.to_string();
        meta.insert("type".to_string(), Value::String(ext_type.clone()));
        rewritten = true;
    }
    if !constants::is_valid_type(&ext_type) {
        return Err(verr(trf("Unknown extension type: '%1'", &[&ext_type])));
    }

    // ── скриптовые типы: install.sh / uninstall.sh обязательны ──
    let is_scripted = constants::is_scripted(&ext_type);
    if is_scripted {
        // src_dir — распакованное дерево (наблюдения).
        if !fs.obs_is_file(&join(src_dir, "install.sh")) {
            return Err(InstallError {
                kind: "FileNotFoundError",
                message: trf("install.sh is required for type '%1'", &[&ext_type]),
            });
        }
        if !fs.obs_is_file(&join(src_dir, "uninstall.sh")) {
            return Err(InstallError {
                kind: "FileNotFoundError",
                message: trf("uninstall.sh is required for type '%1'", &[&ext_type]),
            });
        }
    }

    // ── совместимость с версией Rexon ──
    check_rexon_compat(&meta).map_err(|message| InstallError {
        kind: "RuntimeError",
        message,
    })?;

    // ── дубликат ──
    if dest_exists(&dest_dir) {
        return Err(InstallError {
            kind: "FileExistsError",
            message: trf(
                "Extension '%1' is already installed. Remove the old version first.",
                &[&ext_id],
            ),
        });
    }

    let manifest = Value::Object(meta);
    let rewritten_manifest_text = if rewritten {
        Some(dumps_indent2(&manifest))
    } else {
        None
    };
    Ok(InstallCheck {
        ext_id,
        ext_type,
        is_scripted,
        manifest,
        rewritten_manifest_text,
    })
}

/// Манифест v4 (`wallpaper-video-adaptive`): проверка схемы и вывод `id`
/// из `title` (`v4-<slug>`) и `name`, если их нет. `Ok(true)` — манифест
/// v4 и в нём что-то дописано; `Ok(false)` — это не v4. Общая часть
/// [`install_rxn_check`] и `rxn_manifest` (перевод v4 → v5 в памяти).
pub fn v4_fill_id_and_name(meta: &mut Map<String, Value>) -> Result<bool, InstallError> {
    if meta.get("type").and_then(Value::as_str) != Some("wallpaper-video-adaptive") {
        return Ok(false);
    }
    let (ok, err) = validate_v4_manifest(&Value::Object(meta.clone()));
    if !ok {
        return Err(verr(trf("Invalid v4 manifest: %1", &[&err])));
    }
    if !is_truthy(meta.get("id")) {
        let title = meta
            .get("title")
            .map(py_str)
            .unwrap_or_default()
            .to_lowercase();
        let slug = slugify(&title);
        let id = if slug.is_empty() {
            "v4-adaptive".to_string()
        } else {
            format!("v4-{slug}")
        };
        meta.insert("id".to_string(), Value::String(id));
    }
    if !meta.contains_key("name") {
        // setdefault(name, title or id): пустой/ложный title → id.
        let name = match meta.get("title") {
            Some(t) if is_truthy(Some(t)) => t.clone(),
            _ => meta.get("id").cloned().unwrap_or(Value::Null),
        };
        meta.insert("name".to_string(), name);
    }
    Ok(true)
}

/// `id` манифеста v3/v4: есть, непустая строка, без `/` и `..`.
pub fn manifest_ext_id(meta: &Map<String, Value>) -> Result<String, InstallError> {
    let ext_id = match meta.get("id") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => return Err(verr(missing_id())),
    };
    if ext_id.contains('/') || ext_id.contains("..") {
        return Err(verr(trf(
            "manifest.json: unsafe id '%1' (it contains path characters)",
            &[&ext_id],
        )));
    }
    Ok(ext_id)
}

/// «Нет поля id» — одна подпись на обе ветки проверки.
fn missing_id() -> String {
    tr("manifest.json: the 'id' field is missing")
}

/// Текст ошибки для повреждённого manifest.json (`ValueError` Python:
/// `manifest.json повреждён: {JSONDecodeError}`; деталь после двоеточия у
/// serde своя — parity сверяет префикс).
pub fn bad_manifest_json_error(detail: &str) -> InstallError {
    verr(trf("manifest.json is damaged: %1", &[&detail]))
}

/// Порт `_check_rexon_compat`: разбор `rexon_version` (`re.match` — только
/// с начала строки) и сравнение с REXON_VERSION. Неизвестный оператор или
/// нераспознанная строка — пропуск (как `checks.get(op, True)`).
pub fn check_rexon_compat(meta: &Map<String, Value>) -> Result<(), String> {
    let req_str = meta
        .get("rexon_version")
        .map(py_str)
        .unwrap_or_default();
    if req_str.is_empty() {
        return Ok(());
    }
    // r"([><=!]+)\s*(\d+)\.(\d+)\.(\d+)" с семантикой re.match (якорь только
    // в начале строки; хвост после третьей группы допустим).
    let cs: Vec<char> = req_str.chars().collect();
    let mut i = 0;
    while i < cs.len() && matches!(cs[i], '>' | '<' | '=' | '!') {
        i += 1;
    }
    if i == 0 {
        return Ok(()); // нет матча — как `if not m: return`
    }
    let op: String = cs[..i].iter().collect();
    while i < cs.len() && cs[i].is_whitespace() {
        i += 1;
    }
    let take_num = |i: &mut usize| -> Option<u32> {
        let start = *i;
        while *i < cs.len() && cs[*i].is_ascii_digit() {
            *i += 1;
        }
        if *i == start {
            return None;
        }
        cs[start..*i].iter().collect::<String>().parse().ok()
    };
    let expect_dot = |i: &mut usize| -> bool {
        if cs.get(*i) == Some(&'.') {
            *i += 1;
            true
        } else {
            false
        }
    };
    let Some(a) = take_num(&mut i) else { return Ok(()) };
    if !expect_dot(&mut i) {
        return Ok(());
    }
    let Some(b) = take_num(&mut i) else { return Ok(()) };
    if !expect_dot(&mut i) {
        return Ok(());
    }
    let Some(c) = take_num(&mut i) else { return Ok(()) };
    let req = (a, b, c);
    let cur = constants::REXON_VERSION;
    let ok = match op.as_str() {
        ">=" => cur >= req,
        ">" => cur > req,
        "==" => cur == req,
        "<=" => cur <= req,
        "<" => cur < req,
        "!=" => cur != req,
        _ => true, // checks.get(op, True)
    };
    if ok {
        Ok(())
    } else {
        Err(trf(
            "Requires Rexon %1, but %2 is installed",
            &[&req_str, &format!("{}.{}.{}", cur.0, cur.1, cur.2)],
        ))
    }
}

/// `re.sub(r"[^a-z0-9]+", "-", s).strip("-")` (вход уже lower()).
fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

// ============================================================================
// _safe_install как план
// ============================================================================

/// Порт `_safe_install`: установка типов оформления как чистый план.
pub fn safe_install_plan(env: &ExtEnv, ext: &ExtInfo) -> Vec<ExtStep> {
    let mut steps = Vec::new();
    let mut pfs = PlanFs::new(env.fs);
    safe_install_into(env, ext, &mut pfs, &mut steps);
    // Всё основное пропущено (на месте чужое) — ставить нечего: не «успех с
    // заметками», а понятный отказ целиком, до первого шага. Частичный
    // пропуск — установка с заметками.
    if pfs.skipped > 0 && !steps.iter().any(places_something) {
        let mut lines = vec![trf(
            "“%1” was not installed: everything in it would replace something that this package \
             did not install.",
            &[&ext.name()],
        )];
        lines.extend(steps.iter().filter_map(|s| match s {
            ExtStep::Notice { text } => Some(text.clone()),
            _ => None,
        }));
        return vec![ExtStep::Refuse { text: lines.join("\n") }];
    }
    steps
}

/// Кладёт ли шаг что-то из пакета: копия, распаковка, ссылка, раскладка для
/// системной части, системный шаг или инструмент KDE. Каталоги-основы
/// (`MakeDirs`), метки и заметки — не в счёт.
fn places_something(step: &ExtStep) -> bool {
    match step {
        ExtStep::CopyTree { .. }
        | ExtStep::CopyFile { .. }
        | ExtStep::ExtractZip { .. }
        | ExtStep::Symlink { .. }
        | ExtStep::Stage { .. }
        | ExtStep::PkexecScript { .. } => true,
        ExtStep::Run { argv, .. } => argv.first().is_some_and(|a| {
            let bin = a.rsplit('/').next().unwrap_or(a);
            bin == "pkexec" || bin.starts_with("kpackagetool")
        }),
        _ => false,
    }
}

fn safe_install_into(
    env: &ExtEnv,
    ext: &ExtInfo,
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let ext_type = ext.ext_type();
    let Some(target_base) = env.install_path(&ext_type) else {
        // wallpaper-pack не копируется — обои применяются напрямую.
        return;
    };

    let data_dir = join(ext.dir, "data");
    if !env.fs.is_dir(&data_dir) {
        return; // предупреждение в логе — шагов нет
    }

    steps.push(ExtStep::MakeDirs { path: target_base.clone() });
    pfs.mk_dir(&target_base);

    // v4 adaptive: симлинк на каталог расширения (медиа не дублируем).
    if ext_type == "wallpaper-video-adaptive" {
        let link = join(&target_base, &ext.ext_id());
        if !env.fs.lexists(&link) {
            steps.push(ExtStep::Symlink {
                target: ext.dir.to_string(),
                link,
            });
        }
        return;
    }

    if ext_type == "font-pack" {
        install_fonts_plan(env, ext, steps);
        return;
    }

    // KDE color schemes: файлы .colors (не папки).
    if ext_type == "theme-kde-colors" {
        for f in env.fs.scandir(&data_dir) {
            let src = join(&data_dir, f);
            if env.fs.is_file(&src) && f.ends_with(".colors") {
                let dest = join(&target_base, f);
                // Только файл: схему выбирает «Применить» вкладки «Оформление»
                // (см. [`crate::appearance_installer::applied_by_button`]).
                // Схема человека или другого пакета с тем же именем файла (и
                // другим содержимым) не перезаписывается.
                if !put_owned_file(env, pfs, &src, &dest, steps) {
                    steps.push(taken_file_notice(f, &dest));
                    pfs.skipped += 1;
                }
            }
        }
        return;
    }

    // Кастомизация конкретных окружений (KWin, GNOME, Cinnamon, XFCE, MATE,
    // LXQt) — ОДИН generic-путь по матрице `de_customization`, а не новая
    // ветка на каждый тип (правило #12).
    if let Some(row) = de_customization::de_managed_row(&ext_type) {
        install_de_custom_plan(env, ext, row, &data_dir, pfs, steps);
        return;
    }

    // Остальные темы: каждая подпапка data/ — тема/набор. login-screen и
    // plymouth ставятся через pkexec — user-копия пропускается.
    let skip_user_copy = matches!(
        ext_type.as_str(),
        "theme-login-screen" | "theme-plymouth"
    );
    let mut installed_names: Vec<String> = Vec::new();
    for name in env.fs.scandir(&data_dir) {
        let entry = join(&data_dir, name);
        if !env.fs.is_dir(&entry) {
            continue;
        }
        if skip_user_copy {
            installed_names.push(name.clone());
            continue;
        }
        let dest = join(&target_base, name);
        // Копии подпапок комплекта (`~/.local/share/rexon/bundles/<подпапка>`)
        // и Conky (`~/.config/conky/<подпапка>`) лежат под ОБЩИМИ именами —
        // такая же папка может быть чужой. Кладутся с меткой владельца, чтобы
        // удаление снимало ровно своё (дефекты Д2/Д3, 25.10.2026).
        if ext_type == "theme-bundle" || ext_type == "theme-conky" {
            installed_names.push(name.clone());
            // Подпапка с именем самого виджета — это его каталог
            // `~/.config/conky/<id>`: его кладёт `install_conky_plan`.
            if !(ext_type == "theme-conky" && *name == ext.ext_id()) {
                put_owned_copy(env, ext, pfs, &entry, &dest, steps);
            }
            continue;
        }
        // Основная копия темы — по тому же правилу владельца: на месте
        // каталог чужой темы (поставленной руками, другим пакетом или
        // прежним Rexon без метки) — не трогать, а сказать человеку.
        if !put_owned_copy(env, ext, pfs, &entry, &dest, steps) {
            steps.push(taken_by_other_notice(name, &dest));
            pfs.skipped += 1;
            continue;
        }
        installed_names.push(name.clone());
    }

    // ⛔ Установка темы НЕ применяет её. Прежде здесь шли
    // plasma-apply-lookandfeel/-colorscheme/-desktoptheme, kvantummanager --set
    // и запись ~/.icons/default (курсор у X11-программ менялся сразу при
    // установке, до «Применить» — дефект 03.10.2026). Все эти типы включает
    // кнопка «Применить» вкладки «Оформление» общим планом каждого вида
    // (курсор — `themes::apply_cursor_plan_sized` во всех окружениях); сторож —
    // `rxn_install_never_applies_what_the_button_applies` в тестах.

    match ext_type.as_str() {
        "theme-window-decoration" => {
            install_window_decoration_plan(env, ext, &installed_names, pfs, steps)
        }
        "theme-login-screen" => {
            install_login_screen_plan(env, ext, &data_dir, &installed_names, steps)
        }
        "theme-plymouth" => {
            install_plymouth_plan(env, ext, &data_dir, &installed_names, steps)
        }
        "theme-conky" => install_conky_plan(env, ext, &data_dir, pfs, steps),
        "theme-bundle" => install_bundle_plan(env, ext, pfs, steps),
        "theme-plasmoid" => install_plasmoid_plan(env, ext, &data_dir, pfs, steps),
        _ => {}
    }
}

// ============================================================================
// Кастомизация окружений: один путь на все типы матрицы
// ============================================================================

/// Объект установки: что копируем и под каким именем он будет включаться.
struct DeObject {
    src: String,
    /// Имя каталога назначения; оно же — идентификатор для активации
    /// (uuid у GNOME/Cinnamon, KPlugin.Id у KDE, имя темы у ~/.themes).
    name: String,
}

/// Прочитать поле из JSON-файла снимка (`meta`), поддерживая точечный путь
/// (`KPlugin.Id`).
fn json_field(fs: &FsSnap, path: &str, field: &str) -> Option<String> {
    let text = fs.read_text(path)?;
    let v: Value = serde_json::from_str(text).ok()?;
    let mut cur = &v;
    for part in field.split('.') {
        cur = cur.get(part)?;
    }
    cur.as_str().map(String::from)
}

/// Разобрать `data/` на объекты по форме полезной нагрузки строки матрицы.
///
/// Единая грабля всех окружений: имя каталога назначения не всегда равно имени
/// папки в архиве — GNOME и Cinnamon требуют каталог ровно с именем `uuid`,
/// KDE — с именем `KPlugin.Id`. Ошибёшься именем — объект установится и молча
/// не заработает.
fn de_objects(
    env: &ExtEnv,
    ext: &ExtInfo,
    row: &de_customization::Row,
    data_dir: &str,
) -> Vec<DeObject> {
    use de_customization::Payload;

    let mut out: Vec<DeObject> = Vec::new();

    // Имя объекта по каталогу-кандидату.
    let resolve = |src: &str, fallback: &str| -> String {
        match row.payload {
            Payload::DirUuid { meta, field } => {
                json_field(env.fs, &join(src, meta), field)
                    .unwrap_or_else(|| fallback.to_string())
            }
            Payload::KPackage { id_field, .. } => {
                json_field(env.fs, &join(src, "metadata.json"), id_field)
                    .or_else(|| {
                        env.fs
                            .read_text(&join(src, "metadata.desktop"))
                            .and_then(|t| {
                                ini_get(t, "Desktop Entry", "X-KDE-PluginInfo-Name")
                            })
                    })
                    .unwrap_or_else(|| fallback.to_string())
            }
            _ => fallback.to_string(),
        }
    };

    // Признак «этот каталог и есть объект» (а не контейнер с объектами).
    let is_object = |dir: &str| -> bool {
        match row.payload {
            Payload::DirUuid { meta, .. } => env.fs.is_file(&join(dir, meta)),
            Payload::KPackage { .. } => {
                env.fs.is_file(&join(dir, "metadata.json"))
                    || env.fs.is_file(&join(dir, "metadata.desktop"))
            }
            Payload::ThemeSubdir { sub } => env.fs.is_dir(&join(dir, sub)),
            Payload::Dir => true,
            Payload::Files { .. } => false,
        }
    };

    if let Payload::Files { ext: suffix } = row.payload {
        for f in env.fs.scandir(data_dir) {
            let src = join(data_dir, f);
            if env.fs.is_file(&src) && f.ends_with(suffix) {
                out.push(DeObject { src, name: f.clone() });
            }
        }
        return out;
    }

    for name in env.fs.scandir(data_dir) {
        let entry = join(data_dir, name);
        if env.fs.is_dir(&entry) && is_object(&entry) {
            out.push(DeObject {
                name: resolve(&entry, name),
                src: entry,
            });
        }
    }

    // Пакет положили без обёртки — сам data/ и есть объект (частый случай:
    // архив с gnome-shell-расширением распакован «как есть»).
    if out.is_empty() && is_object(data_dir) {
        let fallback = ext.ext_id();
        out.push(DeObject {
            name: resolve(data_dir, &fallback),
            src: data_dir.to_string(),
        });
    }
    out
}

/// Шаг «выполнить сценарий оболочки» (изменение списков gsettings и правка
/// ini-файлов одной командой не делаются — см. `de_customization`).
fn sh(script: String) -> ExtStep {
    ExtStep::Run {
        argv: vec!["bash".into(), "-c".into(), script],
        timeout_secs: Some(30),
    }
}

/// Шаги активации объекта. Пусто — значит включать нечего либо включение
/// сознательно оставлено пользователю (плагин обоев Plasma).
fn de_activate_steps(
    env: &ExtEnv,
    row: &de_customization::Row,
    obj_name: &str,
    dest: &str,
) -> Vec<ExtStep> {
    use de_customization::{Activate, EntryFmt};

    let kw = |args: Vec<String>| ExtStep::Run { argv: args, timeout_secs: Some(10) };

    match row.activate {
        // Rexon — типы общего установщика оформления: сюда они не приходят
        // (`de_managed_row` их отсекает), включение делает установщик тем.
        Activate::Auto | Activate::Rexon | Activate::Manual(_) => Vec::new(),
        Activate::KdeConfig { file, group, key, extra } => {
            let mut v = vec![kw(vec![
                "kwriteconfig6".into(),
                "--file".into(),
                file.into(),
                "--group".into(),
                group.into(),
                "--key".into(),
                key.into(),
                obj_name.into(),
            ])];
            if let Some((k, val)) = extra {
                v.push(kw(vec![
                    "kwriteconfig6".into(),
                    "--file".into(),
                    file.into(),
                    "--group".into(),
                    group.into(),
                    "--key".into(),
                    k.into(),
                    val.into(),
                ]));
            }
            v
        }
        Activate::KwinPlugin => vec![kw(vec![
            "kwriteconfig6".into(),
            "--file".into(),
            "kwinrc".into(),
            "--group".into(),
            "Plugins".into(),
            "--key".into(),
            format!("{obj_name}Enabled"),
            "true".into(),
        ])],
        Activate::Gsettings { schema, key } => vec![kw(vec![
            "gsettings".into(),
            "set".into(),
            schema.into(),
            key.into(),
            obj_name.into(),
        ])],
        Activate::GsettingsList { schema, key, entry, .. } => match entry {
            EntryFmt::CinnamonApplet => {
                vec![sh(de_customization::cinnamon_applet_enable_script(obj_name))]
            }
            EntryFmt::CinnamonDesklet => {
                vec![sh(de_customization::cinnamon_desklet_enable_script(obj_name))]
            }
            EntryFmt::Uuid => vec![sh(de_customization::gsettings_list_add_script(
                schema, key, obj_name, obj_name,
            ))],
        },
        Activate::GnomeExtension => {
            vec![sh(de_customization::gnome_extension_enable_script(obj_name, dest))]
        }
        // `-n -t string`: на свежем профиле свойства ещё нет, и голое `-s`
        // отказывает («does not exist on channel») — стенд 04.10.2026.
        Activate::Xfconf { channel, prop } => vec![kw(vec![
            "xfconf-query".into(),
            "-c".into(),
            channel.into(),
            "-p".into(),
            prop.into(),
            "-n".into(),
            "-t".into(),
            "string".into(),
            "-s".into(),
            obj_name.into(),
        ])],
        Activate::IniKey { file, group, key } => vec![sh(de_customization::ini_set_script(
            &format!("{}/{}", env.home, file),
            group,
            key,
            obj_name,
        ))],
        // Узел `rc.xml` правится по его тексту, а читать файлы этот план не
        // умеет: тему рамок Openbox и labwc включает кнопка раздела
        // (`crate::de_apply::rc_theme_steps`), установка её не включает.
        Activate::RcXmlTheme => Vec::new(),
    }
}

/// Шаг обновления окружения — только там, где перезапуск безопасен.
///
/// Осознанно НЕ перезапускаем оболочку GNOME (на Wayland это невозможно, а на
/// X11 требует прав, которых у нас нет), панель LXQt (упавшую панель некому
/// поднять — пользователь останется без панели) и plasmashell (перерисовка
/// стола не стоит риска потерять раскладку). Там применение — по подсказке
/// [`Refresh::hint`] в интерфейсе.
fn qdbus_tool(env: &ExtEnv) -> Option<String> {
    (env.which)("qdbus6")
        .or_else(|| (env.which)("qdbus"))
        .or_else(|| (env.which)("qdbus-qt6"))
}

fn de_refresh_step(env: &ExtEnv, row: &de_customization::Row) -> Option<ExtStep> {
    use de_customization::Refresh;
    match row.refresh {
        Refresh::KwinReconfigure => {
            let tool = qdbus_tool(env)?;
            Some(ExtStep::Run {
                argv: vec![
                    tool,
                    "org.kde.KWin".into(),
                    "/KWin".into(),
                    "reconfigure".into(),
                ],
                timeout_secs: Some(10),
            })
        }
        Refresh::CinnamonReplace => {
            // На Wayland Cinnamon перезапустить нельзя — только перезаход.
            if env.wayland_display || env.xdg_session_type.eq_ignore_ascii_case("wayland") {
                return None;
            }
            // `cinnamon --replace` в чужом сеансе (MATE, XFCE…) сменил бы
            // там оконный менеджер на Cinnamon: только в запущенном Cinnamon.
            if !env.xdg_current_desktop.to_lowercase().contains("cinnamon") {
                return None;
            }
            let tool = (env.which)("cinnamon")?;
            Some(ExtStep::Spawn { argv: vec![tool, "--replace".into()] })
        }
        _ => None,
    }
}

/// Включить уже установленный объект типа матрицы — кнопка «Применить» /
/// «Включить» вкладки «Оформление» (`crate::de_apply`).
///
/// Те же шаги, что прежде делала установка ([`de_activate_steps`] плюс
/// безопасное обновление окружения): установка теперь только кладёт файлы,
/// а включает человек кнопкой (решение 04.10.2026). Второго списка команд
/// нет — правило #12. Пусто — тип не из матрицы либо включать нечего.
pub fn de_enable_steps(env: &ExtEnv, kind: &str, obj_name: &str) -> Vec<ExtStep> {
    let Some(row) = de_customization::de_managed_row(kind) else {
        return Vec::new();
    };
    let dest = join(&row.dest_path(env.home), obj_name);
    let mut steps = de_activate_steps(env, row, obj_name, &dest);
    if !steps.is_empty() {
        steps.extend(de_refresh_step(env, row));
    }
    steps
}

/// Имена объектов типа матрицы внутри пакета `.rxn` — так, как они лежат
/// на диске после установки (uuid для GNOME/Cinnamon, `KPlugin.Id` для
/// KDE, имя файла для Nemo). По ним вкладка «Расширения» включает и
/// выключает дополнение тем же путём, что кнопка «Оформления».
pub fn de_object_names(env: &ExtEnv, ext: &ExtInfo) -> Vec<String> {
    let Some(row) = de_customization::de_managed_row(&ext.ext_type()) else {
        return Vec::new();
    };
    let data_dir = join(ext.dir, "data");
    de_objects(env, ext, row, &data_dir).into_iter().map(|o| o.name).collect()
}

/// Выключить объект типа матрицы — кнопка «Выключить» (те же шаги, что
/// удаление делает перед сносом каталога).
pub fn de_disable_steps(env: &ExtEnv, kind: &str, obj_name: &str) -> Vec<ExtStep> {
    let Some(row) = de_customization::de_managed_row(kind) else {
        return Vec::new();
    };
    let mut steps = de_deactivate_steps(env, row, obj_name);
    if !steps.is_empty() {
        steps.extend(de_refresh_step(env, row));
    }
    steps
}

/// Установка типа кастомизации окружения: копирование + включение + обновление.
fn install_de_custom_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    row: &de_customization::Row,
    data_dir: &str,
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    use de_customization::{Activate, Payload};

    let objects = de_objects(env, ext, row, data_dir);
    if objects.is_empty() {
        return; // предупреждение в логе — шагов нет
    }

    let base = row.dest_path(env.home);
    let mut installed: Vec<String> = Vec::new();

    for obj in &objects {
        if let Payload::Files { .. } = row.payload {
            // Одиночный файл — по правилу владельца файлов: на месте чужой
            // файл с другим содержимым — пропуск с заметкой.
            let dest = join(&base, &obj.name);
            if !put_owned_file(env, pfs, &obj.src, &dest, steps) {
                steps.push(taken_file_notice(&obj.name, &dest));
                pfs.skipped += 1;
                continue;
            }
            installed.push(obj.name.clone());
            continue;
        }

        // Каталог с тем же именем уже стоит и положен не этим пакетом
        // (тема человека, другой пакет, копия прежнего Rexon без метки) —
        // объект пропускается целиком: ни копии, ни инструмента KDE, ни
        // включения чужой темы. Свою прежнюю копию (метка этого пакета)
        // ниже снимает и кладёт заново `put_owned_copy`.
        let dest = join(&base, &obj.name);
        if (pfs.exists(&dest) || env.fs.lexists(&dest)) && !owned_by(env.fs, &dest, &ext.ext_id()) {
            steps.push(taken_by_other_notice(&obj.name, &dest));
            pfs.skipped += 1;
            continue;
        }

        // KDE-пакеты ставит их собственный инструмент — он кладёт файлы в
        // правильный каталог и обновляет реестр; копирование остаётся
        // запасным путём (как уже сделано для плазмоидов).
        if let Payload::KPackage { ptype, .. } = row.payload {
            if let Some(tool) =
                (env.which)("kpackagetool6").or_else(|| (env.which)("kpackagetool5"))
            {
                steps.push(ExtStep::Run {
                    argv: vec![
                        tool.clone(),
                        "--type".into(),
                        ptype.into(),
                        "-u".into(),
                        obj.src.clone(),
                    ],
                    timeout_secs: Some(30),
                });
                let install_argv = vec![
                    tool,
                    "--type".into(),
                    ptype.into(),
                    "-i".into(),
                    obj.src.clone(),
                ];
                steps.push(ExtStep::Run {
                    argv: install_argv.clone(),
                    timeout_secs: Some(30),
                });
                installed.push(obj.name.clone());
                if (env.run_ok)(&install_argv) {
                    // Метка владельца и у пакета, поставленного инструментом
                    // KDE: по ней удаление узнаёт своё (чужое с тем же
                    // именем не снимается).
                    steps.push(ExtStep::WriteFile {
                        path: join(&dest, OWNER_MARK),
                        content: format!("{}\n", ext.ext_id()),
                    });
                    continue;
                }
            } else {
                installed.push(obj.name.clone());
            }
        } else {
            installed.push(obj.name.clone());
        }

        steps.push(ExtStep::MakeDirs { path: base.clone() });
        pfs.mk_dir(&base);
        put_owned_copy(env, ext, pfs, &obj.src, &dest, steps);
    }

    if installed.is_empty() {
        return;
    }

    // Скалярные ключи (имя темы, раскладка Alt+Tab) принимают ОДНО значение:
    // берём apply_name из манифеста, иначе первый объект. Списочные включения
    // (апплеты, расширения) применяются ко всем объектам пакета.
    let scalar_target = ext
        .manifest
        .get("apply_name")
        .map(py_str)
        .unwrap_or_else(|| installed[0].clone());

    let list_like = matches!(
        row.activate,
        Activate::GsettingsList { .. } | Activate::GnomeExtension | Activate::KwinPlugin
    );
    let targets: Vec<String> = if list_like {
        installed.clone()
    } else {
        vec![scalar_target]
    };

    // ⛔ Тип, который включает «Применить» вкладки «Оформление» (заставка и
    // экран блокировки KDE), установка только кладёт: выбор — за человеком
    // (дефект 03.10.2026). Ключ в `activate` при этом остаётся — по нему
    // удаление сбрасывает настройку, если она указывала на снимаемую тему.
    if crate::appearance_installer::applied_by_button(row.kind) {
        return;
    }

    let mut activated = false;
    for name in &targets {
        let dest = join(&base, name);
        for step in de_activate_steps(env, row, name, &dest) {
            steps.push(step);
            activated = true;
        }
    }

    if activated {
        if let Some(step) = de_refresh_step(env, row) {
            steps.push(step);
        }
    }
}

/// Шаги выключения объекта — обратное к [`de_activate_steps`].
///
/// Общие для удаления (сначала выключить, потом снести) и для кнопки
/// «Выключить» вкладки «Оформление» ([`de_disable_steps`]): второго способа
/// выключить то же самое нет (правило #12). Пусто — выключать нечего.
fn de_deactivate_steps(env: &ExtEnv, row: &de_customization::Row, obj_name: &str) -> Vec<ExtStep> {
    use de_customization::{Activate, EntryFmt, Payload};
    let mut v: Vec<ExtStep> = Vec::new();
    match row.activate {
        Activate::Auto | Activate::Rexon | Activate::Manual(_) => {}
        Activate::KdeConfig { file, group, key, .. } => {
            v.push(sh(de_customization::kde_config_reset_if_script(
                file, group, key, obj_name,
            )));
        }
        Activate::KwinPlugin => {
            v.push(ExtStep::Run {
                argv: vec![
                    "kwriteconfig6".into(),
                    "--file".into(),
                    "kwinrc".into(),
                    "--group".into(),
                    "Plugins".into(),
                    "--key".into(),
                    format!("{}Enabled", obj_name),
                    "false".into(),
                ],
                timeout_secs: Some(10),
            });
            // Проверено живьём на Plasma 6: снятого скрипта `reconfigure`
            // НЕ выгружает — он продолжает работать до конца сеанса, хотя
            // файлы уже удалены. Выгружаем явно, для этого у KWin есть
            // отдельный метод (у эффектов такой проблемы нет — им хватает
            // ключа в kwinrc).
            if matches!(
                row.payload,
                Payload::KPackage { ptype: "KWin/Script", .. }
            ) {
                if let Some(tool) = qdbus_tool(env) {
                    v.push(ExtStep::Run {
                        argv: vec![
                            tool,
                            "org.kde.KWin".into(),
                            "/Scripting".into(),
                            "unloadScript".into(),
                            obj_name.to_string(),
                        ],
                        timeout_secs: Some(10),
                    });
                }
            }
        }
        Activate::Gsettings { schema, key } => {
            v.push(sh(de_customization::gsettings_reset_if_script(
                schema, key, obj_name,
            )));
        }
        Activate::GsettingsList { schema, key, entry, .. } => {
            let marker = match entry {
                EntryFmt::CinnamonApplet => format!(":{}:", obj_name),
                EntryFmt::CinnamonDesklet | EntryFmt::Uuid => obj_name.to_string(),
            };
            v.push(sh(de_customization::gsettings_list_remove_script(
                schema, key, &marker,
            )));
        }
        Activate::GnomeExtension => {
            v.push(sh(de_customization::gnome_extension_disable_script(
                obj_name,
            )));
        }
        Activate::Xfconf { channel, prop } => {
            v.push(ExtStep::Run {
                argv: vec![
                    "xfconf-query".into(),
                    "-c".into(),
                    channel.into(),
                    "-p".into(),
                    prop.into(),
                    "-r".into(),
                ],
                timeout_secs: Some(10),
            });
        }
        Activate::IniKey { .. } => {
            // Тема LXQt: чужой конфиг не правим, панель просто вернётся к
            // системной теме при следующем входе.
        }
        Activate::RcXmlTheme => {
            // Тема рамок Openbox и labwc: при удалении `rc.xml` не трогаем —
            // менеджер окон, не найдя тему, рисует рамки своей встроенной.
        }
    }
    v
}

/// Удаление типа кастомизации окружения: сначала выключить, потом снести.
///
/// Порядок важен: если сначала удалить каталог, а ключ оставит указывать на
/// него, окружение получит ссылку в никуда (у экрана блокировки KDE это
/// означает нерабочую блокировку).
fn uninstall_de_custom_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    row: &de_customization::Row,
    steps: &mut Vec<ExtStep>,
) {
    use de_customization::Payload;

    let data_dir = join(ext.dir, "data");
    let objects = de_objects(env, ext, row, &data_dir);
    if objects.is_empty() {
        return;
    }
    let base = row.dest_path(env.home);
    let mut deactivated = false;

    for obj in &objects {
        let off = de_deactivate_steps(env, row, &obj.name);
        if !off.is_empty() {
            deactivated = true;
        }
        steps.extend(off);

        let dest = join(&base, &obj.name);
        // Каталог с этим именем стоит, но положен не этим пакетом (тема
        // человека, которую установка пропустила, или копия прежнего Rexon
        // без метки) — не снимать ни инструментом KDE, ни удалением.
        let foreign = !matches!(row.payload, Payload::Files { .. })
            && env.fs.lexists(&dest)
            && !owned_by(env.fs, &dest, &ext.ext_id());
        if foreign {
            continue;
        }

        // KDE-пакеты снимает их инструмент — иначе останется запись в реестре.
        if let Payload::KPackage { ptype, .. } = row.payload {
            if let Some(tool) =
                (env.which)("kpackagetool6").or_else(|| (env.which)("kpackagetool5"))
            {
                steps.push(ExtStep::Run {
                    argv: vec![
                        tool,
                        "--type".into(),
                        ptype.into(),
                        "-r".into(),
                        obj.name.clone(),
                    ],
                    timeout_secs: Some(30),
                });
            }
        }

        if let Payload::Files { .. } = row.payload {
            // Только свой файл (журнал этого пакета или байт в байт его):
            // одноимённый файл человека установка пропустила — удаление его
            // тоже не трогает.
            if file_is_ours(env.fs, &obj.src, &dest) {
                steps.push(ExtStep::RemoveFile { path: dest });
            }
        } else if env.fs.is_dir(&dest) {
            steps.push(ExtStep::RemoveTree {
                path: dest,
                safe: true,
                ignore_errors: true,
            });
        }
    }

    if deactivated {
        if let Some(step) = de_refresh_step(env, row) {
            steps.push(step);
        }
    }
}

/// Порт `Extension.install_fonts` (шрифты в ~/.local/share/fonts/rexon-<id>).
fn install_fonts_plan(env: &ExtEnv, ext: &ExtInfo, steps: &mut Vec<ExtStep>) {
    let fonts = font_files(env.fs, ext);
    if fonts.is_empty() {
        return; // warning в логе
    }
    let dest_dir = format!("{}/.local/share/fonts/rexon-{}", env.home, ext.ext_id());
    steps.push(ExtStep::MakeDirs { path: dest_dir.clone() });
    for font in &fonts {
        // Файл в файл под своим именем: копия «в каталог» у общего
        // исполнителя не бывает (шаг 2 слияния установщиков).
        let name = font.rsplit('/').next().unwrap_or(font);
        steps.push(ExtStep::CopyFile { src: font.clone(), dest: join(&dest_dir, name) });
    }
    steps.push(run(vec!["fc-cache", "-f", &dest_dir], 30));
}

/// Порт `Extension.font_files` по снимку: data/ (или корень) рекурсивно,
/// сопоставление формата — по суффиксу целиком (`is_supported_font`), так что
/// составной `.pcf.gz` тоже матчится.
fn font_files(fs: &FsSnap, ext: &ExtInfo) -> Vec<String> {
    let data_dir = join(ext.dir, "data");
    let root = if fs.is_dir(&data_dir) { data_dir } else { ext.dir.to_string() };
    let mut out = Vec::new();
    for (dir, _subdirs, files) in fs.walk(&root) {
        for f in &files {
            if is_supported_font(f) {
                out.push(join(&dir, f));
            }
        }
    }
    out
}

/// Метка владельца копии: файл в корне каталога, положенного пакетом под
/// ОБЩИМ именем (`~/.themes/<тема>` у рамок вне KDE, подпапки комплекта в
/// `~/.local/share/rexon/bundles`, подпапки Conky в `~/.config/conky`).
/// Текст — id пакета. Удаление и обновление трогают каталог, только если
/// метка его — тема человека или другого пакета с тем же именем остаётся
/// как была (дефекты Д1–Д3, найдены страховкой 25.10.2026).
pub const OWNER_MARK: &str = ".rexon-owner";

/// Каталог `dir` положил пакет `ext_id`: обычный каталог (не ссылка) с
/// нашей меткой.
fn owned_by(fs: &FsSnap, dir: &str, ext_id: &str) -> bool {
    fs.is_dir(dir)
        && !fs.is_link(dir)
        && fs.read_text(&join(dir, OWNER_MARK)).is_some_and(|t| t.trim() == ext_id)
}

/// Положить копию `src` в `dest` от имени пакета: места нет — копия и
/// метка; место наше — снять и положить заново (обновление пакета); место
/// чужое — не трогать (`false`: копия не положена).
///
/// Совместимость: копии, положенные Rexon до метки (25.10.2026), метки не
/// имеют и от чужих неотличимы — они тоже не трогаются. Поставить поверх
/// такой копии — удалить её каталог и установить пакет снова.
fn put_owned_copy(
    env: &ExtEnv,
    ext: &ExtInfo,
    pfs: &mut PlanFs,
    src: &str,
    dest: &str,
    steps: &mut Vec<ExtStep>,
) -> bool {
    let ext_id = ext.ext_id();
    if pfs.exists(dest) || env.fs.lexists(dest) {
        if !owned_by(env.fs, dest, &ext_id) {
            return false;
        }
        steps.push(ExtStep::RemoveTree {
            path: dest.to_string(),
            safe: true,
            ignore_errors: true,
        });
    }
    steps.push(ExtStep::CopyTree { src: src.to_string(), dest: dest.to_string() });
    pfs.mk_dir(dest);
    steps.push(ExtStep::WriteFile {
        path: join(dest, OWNER_MARK),
        content: format!("{ext_id}\n"),
    });
    true
}

/// Заметка установки: тема `name` не поставлена — на её месте `dest` стоит
/// каталог, положенный не этим пакетом (как пропуск у магазина).
fn taken_by_other_notice(name: &str, dest: &str) -> ExtStep {
    ExtStep::Notice {
        text: trf(
            "“%1” skipped: %2 already holds a theme with this name that this package did not \
             install. To replace it, remove that folder and install the package again.",
            &[&name, &dest],
        ),
    }
}

/// Положить одиночный файл `src` в `dest` от имени пакета (правило
/// владельца для файлов: метку в имя файла не положить, владелец — общий
/// журнал). Места нет — копия. На месте файл, который общий журнал записал
/// за этим пакетом, либо байт в байт тот же (тогда он считается своим и
/// попадает в журнал этой установки) — копия поверх. Иначе — чужой файл
/// (человека, другого пакета, ссылка, каталог): не трогать (`false`).
fn put_owned_file(env: &ExtEnv, pfs: &mut PlanFs, src: &str, dest: &str, steps: &mut Vec<ExtStep>) -> bool {
    if !pfs.made_files.contains(dest) && (pfs.exists(dest) || env.fs.lexists(dest)) && !file_is_ours(env.fs, src, dest) {
        return false;
    }
    steps.push(ExtStep::CopyFile { src: src.to_string(), dest: dest.to_string() });
    pfs.mk_file(dest);
    true
}

/// Файл на месте `dest` — свой для пакета с файлом `src`: обычный файл (не
/// ссылка), записанный за пакетом общим журналом или байт в байт тот же.
fn file_is_ours(fs: &FsSnap, src: &str, dest: &str) -> bool {
    fs.is_file(dest) && !fs.is_link(dest) && (fs.owns_file(dest) || fs.same_bytes(src, dest))
}

/// Заметка установки: файл `name` не положен — на его месте `dest` лежит
/// другой файл, положенный не этим пакетом.
fn taken_file_notice(name: &str, dest: &str) -> ExtStep {
    ExtStep::Notice {
        text: trf(
            "“%1” skipped: %2 already exists, differs from the file in this package and was not \
             installed by it. To replace it, remove that file and install the package again.",
            &[&name, &dest],
        ),
    }
}

/// Снять копию `dest`, если её положил этот пакет ([`put_owned_copy`]).
fn remove_owned_copy(env: &ExtEnv, ext: &ExtInfo, dest: &str, steps: &mut Vec<ExtStep>) {
    if owned_by(env.fs, dest, &ext.ext_id()) {
        steps.push(ExtStep::RemoveTree {
            path: dest.to_string(),
            safe: true,
            ignore_errors: true,
        });
    }
}

/// Рамки вне KDE: тема из каталога Aurorae — ещё и в `~/.themes/<тема>`
/// (там её ищут GNOME, Cinnamon, Budgie, Pantheon и XFCE).
fn copy_decoration_to_gtk_themes(
    env: &ExtEnv,
    ext: &ExtInfo,
    twd_base: &str,
    installed_names: &[String],
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let gtk_themes = format!("{}/.themes", env.home);
    for name in installed_names {
        let src = join(twd_base, name);
        if pfs.is_dir(&src) {
            put_owned_copy(env, ext, pfs, &src, &join(&gtk_themes, name), steps);
        }
    }
}

/// Порт `_install_window_decoration` (DE-зависимая установка декораций).
fn install_window_decoration_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    installed_names: &[String],
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let xdg = env.xdg_current_desktop.to_lowercase();
    let twd_base = env
        .install_path("theme-window-decoration")
        .expect("theme-window-decoration в INSTALL_PATHS");

    if xdg.contains("kde") || xdg.contains("plasma") {
        // Рамки (Aurorae) включает «Применить» вкладки «Оформление»
        // (`aurorae_plan`), установка только кладёт тему в каталог Aurorae.
        // Прежде здесь писались kwinrc org.kde.kdecoration2 и посылался
        // reloadConfig — рамки менялись при установке (дефект 03.10.2026).
    } else if xdg.contains("gnome")
        || xdg.contains("budgie")
        || xdg.contains("pantheon")
        || xdg.contains("cinnamon")
    {
        copy_decoration_to_gtk_themes(env, ext, &twd_base, installed_names, pfs, steps);
    } else if xdg.contains("xfce") {
        copy_decoration_to_gtk_themes(env, ext, &twd_base, installed_names, pfs, steps);
        // Рамки XFCE включает «Применить» раздела «Рамки окон (XFCE)» (тип
        // `theme-xfwm4`, решение 04.10.2026): установка только кладёт тему в
        // ~/.themes. Ветка с записью xfconf осталась на случай, если у типа
        // кнопки не станет, — иначе тему было бы нечем включить.
        if crate::appearance_installer::applied_by_button("theme-xfwm4") {
            return;
        }
        if let Some(first) = installed_names.first() {
            steps.push(run(
                vec!["xfconf-query", "-c", "xfwm4", "-p", "/general/theme", "-s", first],
                10,
            ));
        }
    }
    // прочие DE — только лог
}

/// Порт `_detect_display_manager` как план: probes systemctl — шаги, их
/// результат — через `env.run_ok`; фолбэк по `shutil.which` — без шагов.
pub fn detect_display_manager_plan(env: &ExtEnv, steps: &mut Vec<ExtStep>) -> String {
    const DM_SERVICES: &[(&str, &str)] = &[
        ("sddm", "sddm"),
        ("gdm", "gdm"),
        ("lightdm", "lightdm"),
        ("cosmic-greeter", "cosmic-greeter"),
        ("lxdm", "lxdm"),
    ];
    for (service, name) in DM_SERVICES {
        let argv: Vec<String> = vec![
            "systemctl".into(),
            "is-active".into(),
            format!("{service}.service"),
        ];
        steps.push(ExtStep::Run { argv: argv.clone(), timeout_secs: Some(5) });
        if (env.run_ok)(&argv) {
            return name.to_string();
        }
    }
    const DM_EXES: &[(&str, &str)] = &[
        ("sddm", "sddm"),
        ("gdm", "gdm"),
        ("gdm3", "gdm"),
        ("lightdm", "lightdm"),
        ("cosmic-greeter", "cosmic-greeter"),
    ];
    for (exe, name) in DM_EXES {
        if (env.which)(exe).is_some() {
            return name.to_string();
        }
    }
    String::new()
}

/// Порт `_find_background`: `background*`/`bg*` в корне, иначе самый большой
/// файл-изображение > 10 000 байт (по снимку с размерами).
pub fn find_background(fs: &FsSnap, data_dir: &str) -> String {
    const BG_EXTS: &[&str] = &[".png", ".jpg", ".jpeg", ".webp", ".bmp"];
    let ext_of = |f: &str| -> String {
        f.rfind('.').map(|i| f[i..].to_lowercase()).unwrap_or_default()
    };
    for f in fs.scandir(data_dir) {
        let lower = f.to_lowercase();
        if (lower.starts_with("background") || lower.starts_with("bg"))
            && BG_EXTS.contains(&ext_of(f).as_str())
        {
            return join(data_dir, f);
        }
    }
    let mut biggest = (String::new(), 0u64);
    for (root, _dirs, files) in fs.walk(data_dir) {
        for f in &files {
            if BG_EXTS.contains(&ext_of(f).as_str()) {
                let full = join(&root, f);
                let sz = fs.size(&full);
                if sz > biggest.1 {
                    biggest = (full, sz);
                }
            }
        }
    }
    if biggest.1 > 10_000 {
        biggest.0
    } else {
        String::new()
    }
}

/// Порт `_install_login_screen` (SDDM/GDM/LightDM/COSMIC).
fn install_login_screen_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    data_dir: &str,
    installed_names: &[String],
    steps: &mut Vec<ExtStep>,
) {
    let dm = detect_display_manager_plan(env, steps);
    if dm.is_empty() {
        return; // warning
    }

    if dm == "sddm" && !installed_names.is_empty() {
        let mut theme_name = installed_names[0].clone();
        let mut src = join(data_dir, &theme_name);
        if !env.fs.is_dir(&src) {
            // нет подпапки — data/ сама является темой
            src = data_dir.to_string();
            theme_name = ext.ext_id();
        }
        if is_safe_sys_name(&theme_name) {
            stage_sddm_plan(env, &theme_name, &src, steps);
        }
        // небезопасное имя — только лог
    } else if dm == "gdm" {
        let bg = find_background(env.fs, data_dir);
        if !bg.is_empty() {
            stage_login_bg_plan(env, "gdm-bg", &bg, steps);
        }
    } else if dm == "lightdm" {
        let bg = find_background(env.fs, data_dir);
        if !bg.is_empty() {
            let greeter = lightdm_greeter(env.fs);
            match lightdm_background_way(&greeter) {
                LightdmBackground::GreeterConfig => stage_login_bg_plan(env, "lightdm-bg", &bg, steps),
                LightdmBackground::DeepinAccount => {
                    // Картинку — в общий каталог (экран входа читает её от
                    // своего пользователя), затем — в учётную запись.
                    stage_login_bg_plan(env, "login-bg", &bg, steps);
                    steps.push(deepin_greeter_background_step());
                }
                LightdmBackground::UserWallpaper => steps.push(ExtStep::Notice {
                    text: tr("The Pantheon login screen (io.elementary.greeter) shows each user's own desktop \
                              wallpaper and has no background of its own: set the picture as your wallpaper, \
                              and it will show up on the login screen too."),
                }),
                LightdmBackground::Unknown => steps.push(ExtStep::Notice {
                    text: trf(
                        "The login screen “%1” keeps its background in settings Rexon doesn't know, so the \
                         login screen background was not changed.",
                        &[&greeter],
                    ),
                }),
            }
        }
        for name in installed_names {
            let theme_dir = join(data_dir, name);
            if env.fs.is_dir(&theme_dir)
                && env.fs.is_file(&join(&theme_dir, "index.html"))
            {
                if is_safe_sys_name(name) {
                    stage_dir_plan(
                        env,
                        "lightdm-webkit",
                        name,
                        &theme_dir,
                        "/usr/share/lightdm-webkit2-greeter/themes",
                        "rexon_lightdm_web",
                        steps,
                    );
                }
                break; // и при небезопасном имени — break, как в Python
            }
        }
    } else if dm == "cosmic-greeter" {
        let bg = find_background(env.fs, data_dir);
        if !bg.is_empty() {
            stage_login_bg_plan(env, "cosmic-greeter", &bg, steps);
        }
    }
    // прочие DM — только лог
}

/// Где LightDM-приветствие берёт фон.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightdmBackground {
    /// `background=` в своём файле настроек: slick-greeter
    /// (`/etc/lightdm/slick-greeter.conf`), lightdm-gtk-greeter
    /// (`/etc/lightdm/lightdm-gtk-greeter.conf`). Так же — когда приветствие
    /// узнать не удалось: прежнее поведение.
    GreeterConfig,
    /// lightdm-deepin-greeter: фон учётной записи — `GreeterBackground`
    /// службы `org.deepin.dde.Accounts1` (пользователь меняет свой без пароля).
    DeepinAccount,
    /// io.elementary.greeter: своего фона нет — каждому пользователю он
    /// показывает его обои рабочего стола (копию кладёт
    /// io.elementary.settings-daemon при смене обоев).
    UserWallpaper,
    /// Другое приветствие: куда оно смотрит, Rexon не знает.
    Unknown,
}

/// Файлы настроек LightDM в порядке чтения самим LightDM: каталоги
/// `lightdm.conf.d` (системный, затем свой), затем `lightdm.conf`; поздний
/// перекрывает ранний.
pub const LIGHTDM_CONF_DIRS: &[&str] = &["/usr/share/lightdm/lightdm.conf.d", "/etc/lightdm/lightdm.conf.d"];
pub const LIGHTDM_CONF: &str = "/etc/lightdm/lightdm.conf";
/// Описания приветствий: `<greeter-session>.desktop`, строка `Exec=`.
pub const XGREETERS_DIR: &str = "/usr/share/xgreeters";

/// `greeter-session` из текста настроек LightDM (секции `[Seat:*]` и
/// прежняя `[SeatDefaults]`).
fn lightdm_greeter_session_in(text: &str) -> Option<String> {
    let mut in_seat = false;
    let mut found = None;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_seat = l == "[Seat:*]" || l == "[SeatDefaults]";
            continue;
        }
        if !in_seat {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            if k.trim() == "greeter-session" && !v.trim().is_empty() {
                found = Some(v.trim().to_string());
            }
        }
    }
    found
}

/// Какое приветствие покажет LightDM: имя программы из `Exec=` описания
/// `/usr/share/xgreeters/<greeter-session>.desktop` (у Debian имя —
/// общий `lightdm-greeter`, а описание — ссылка на настоящее), иначе само
/// имя сеанса. Пусто — не задано нигде.
pub fn lightdm_greeter(fs: &FsSnap) -> String {
    let mut files: Vec<String> = Vec::new();
    for dir in LIGHTDM_CONF_DIRS {
        let mut names: Vec<&String> = fs.scandir(dir).iter().filter(|n| n.ends_with(".conf")).collect();
        names.sort();
        files.extend(names.into_iter().map(|n| join(dir, n)));
    }
    files.push(LIGHTDM_CONF.to_string());
    let session = files
        .iter()
        .filter_map(|f| fs.read_text(f).and_then(lightdm_greeter_session_in))
        .next_back()
        .unwrap_or_default();
    if session.is_empty() {
        return session;
    }
    let exec = fs.read_text(&format!("{XGREETERS_DIR}/{session}.desktop")).and_then(|d| {
        d.lines()
            .find_map(|l| l.trim().strip_prefix("Exec="))
            .and_then(|e| e.split_whitespace().next())
            .map(|e| e.rsplit('/').next().unwrap_or(e).to_string())
    });
    exec.filter(|e| !e.is_empty()).unwrap_or(session)
}

/// Куда писать фон для приветствия `greeter` ([`lightdm_greeter`]).
pub fn lightdm_background_way(greeter: &str) -> LightdmBackground {
    match greeter {
        "" | "slick-greeter" | "lightdm-slick-greeter" | "lightdm-gtk-greeter" => LightdmBackground::GreeterConfig,
        "lightdm-deepin-greeter" => LightdmBackground::DeepinAccount,
        "io.elementary.greeter" | "pantheon-greeter" => LightdmBackground::UserWallpaper,
        _ => LightdmBackground::Unknown,
    }
}

/// Фон экрана входа Deepin для своей учётной записи: `SetGreeterBackground`
/// службы `org.deepin.dde.Accounts1` (системная шина; свою запись служба
/// меняет без пароля). Картинка — та, что помощник только что положил в
/// `/usr/share/backgrounds/rexon-login.*` (прежние он убирает, так что она
/// там одна; расширение решает он). Путь объекта — `User<uid>`.
pub fn deepin_greeter_background_step() -> ExtStep {
    ExtStep::RunNoted {
        argv: vec![
            "sh".into(),
            "-c".into(),
            DEEPIN_GREETER_BG_SCRIPT.into(),
        ],
        timeout_secs: Some(30),
        failed: tr("Deepin's account service didn't take the login screen background — set it in Control \
                    Center → Personalization → Wallpaper."),
    }
}

const DEEPIN_GREETER_BG_SCRIPT: &str = "set -- /usr/share/backgrounds/rexon-login.*; [ -f \"$1\" ] || exit 1; \
     exec busctl --system call org.deepin.dde.Accounts1 \"/org/deepin/dde/Accounts1/User$(id -u)\" \
     org.deepin.dde.Accounts1.User SetGreeterBackground s \"file://$1\"";

/// Порт `_stage_login_bg`: staging фона + named helper либо legacy pkexec.
fn stage_login_bg_plan(env: &ExtEnv, category: &str, bg: &str, steps: &mut Vec<ExtStep>) {
    if !env.fs.is_file(bg) {
        return;
    }
    let staged = env.tmp.mkdtemp("rexon_stage_");
    steps.push(ExtStep::Stage {
        staged: staged.clone(),
        sources: vec![bg.to_string()],
    });
    let named = privileged_stage_cmd(
        category,
        "rexon-login",
        &staged,
        true,
        Some(&staged),
        env.stage_helper_available,
    );
    if let Some(argv) = named {
        steps.push(ExtStep::Run { argv, timeout_secs: Some(120) });
    } else {
        steps.push(ExtStep::PkexecScript {
            prefix: format!("rexon_{category}"),
            script: login_bg_legacy_script(category, bg),
        });
    }
    steps.push(ExtStep::RemoveTree {
        path: staged,
        safe: false,
        ignore_errors: true,
    });
}

/// Текст legacy-скрипта `_stage_login_bg_legacy` (байт-в-байт).
pub fn login_bg_legacy_script(category: &str, bg: &str) -> String {
    // os.path.splitext(bg)[1].lstrip(".").lower(): расширение basename;
    // ведущие точки имени не считаются разделителем (как в CPython).
    let base = bg.rsplit('/').next().unwrap_or(bg);
    let raw_ext = match base.rfind('.') {
        Some(i) if base[..i].chars().any(|c| c != '.') => &base[i..],
        _ => "",
    };
    let ext = raw_ext.trim_start_matches('.').to_lowercase();
    // ^[a-z0-9]+$ иначе png (как в Python).
    let ext = if !ext.is_empty()
        && ext.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        ext
    } else {
        "png".to_string()
    };
    let dest_bg = format!("/usr/share/backgrounds/rexon-login.{ext}");
    let qbg = shlex_quote(bg);
    let qdest = shlex_quote(&dest_bg);
    let mut lines: Vec<String> = vec![
        "#!/bin/bash".into(),
        "set -e".into(),
        "install -d /usr/share/backgrounds".into(),
        "rm -f /usr/share/backgrounds/rexon-login.* 2>/dev/null || true".into(),
        format!("cp -f {qbg} {qdest}"),
        format!("chmod 644 {qdest}"),
    ];
    if category == "gdm-bg" {
        lines.extend([
            "install -d /etc/dconf/db/gdm.d".to_string(),
            "cat > /etc/dconf/db/gdm.d/02-rexon-login <<REXON_EOF".to_string(),
            "[org/gnome/desktop/background]".to_string(),
            format!("picture-uri='file://{dest_bg}'"),
            format!("picture-uri-dark='file://{dest_bg}'"),
            "picture-options='zoom'".to_string(),
            "REXON_EOF".to_string(),
            "dconf update 2>/dev/null || true".to_string(),
        ]);
    } else if category == "lightdm-bg" {
        lines.extend([
            "if [ -f /etc/lightdm/slick-greeter.conf ]; then".to_string(),
            format!(
                "  sed -i 's|^background=.*|background={dest_bg}|' \
                 /etc/lightdm/slick-greeter.conf"
            ),
            "elif [ -d /etc/lightdm ]; then".to_string(),
            "  cat >> /etc/lightdm/slick-greeter.conf <<REXON_EOF".to_string(),
            "[Greeter]".to_string(),
            format!("background={dest_bg}"),
            "REXON_EOF".to_string(),
            "fi".to_string(),
            "if [ -f /etc/lightdm/lightdm-gtk-greeter.conf ]; then".to_string(),
            format!(
                "  sed -i 's|^background=.*|background={dest_bg}|' \
                 /etc/lightdm/lightdm-gtk-greeter.conf"
            ),
            "fi".to_string(),
        ]);
    }
    lines.join("\n") + "\n"
}

/// Порт `_stage_dir`: каталог-тема → <dest_root>/<name> под root.
#[allow(clippy::too_many_arguments)]
fn stage_dir_plan(
    env: &ExtEnv,
    category: &str,
    name: &str,
    src_dir: &str,
    dest_root: &str,
    prefix: &str,
    steps: &mut Vec<ExtStep>,
) {
    if !is_safe_sys_name(name) || !env.fs.is_dir(src_dir) {
        return;
    }
    let staged = env.tmp.mkdtemp("rexon_stage_");
    steps.push(ExtStep::Stage {
        staged: staged.clone(),
        sources: vec![src_dir.to_string()],
    });
    let named = privileged_stage_cmd(
        category,
        name,
        &staged,
        true,
        Some(&staged),
        env.stage_helper_available,
    );
    if let Some(argv) = named {
        steps.push(ExtStep::Run { argv, timeout_secs: Some(300) });
    } else {
        steps.push(ExtStep::PkexecScript {
            prefix: prefix.to_string(),
            script: stage_dir_fallback_script(name, src_dir, dest_root),
        });
    }
    steps.push(ExtStep::RemoveTree {
        path: staged,
        safe: false,
        ignore_errors: true,
    });
}

/// Текст fallback-скрипта `_stage_dir` (байт-в-байт).
pub fn stage_dir_fallback_script(name: &str, src_dir: &str, dest_root: &str) -> String {
    let dest = format!("{dest_root}/{name}");
    let qsrc = shlex_quote(src_dir);
    let qdest = shlex_quote(&dest);
    [
        "#!/bin/bash".to_string(),
        "set -e".to_string(),
        format!("rm -rf {qdest}"),
        format!("install -d {qdest}"),
        format!("cp -rT {qsrc} {qdest}"),
        format!("chmod -R a+rX {qdest}"),
    ]
    .join("\n")
        + "\n"
}

/// Порт `_stage_sddm`: системный helper org.rexon.system-install либо
/// hardened сырой pkexec.
fn stage_sddm_plan(env: &ExtEnv, theme_name: &str, src: &str, steps: &mut Vec<ExtStep>) {
    if !is_safe_sys_name(theme_name) || !env.fs.is_dir(src) {
        return;
    }
    let staged = env.tmp.mkdtemp("rexon_stage_");
    steps.push(ExtStep::Stage {
        staged: staged.clone(),
        sources: vec![src.to_string()],
    });
    if env.system_helper_ok() {
        steps.push(ExtStep::Run {
            argv: [
                vec!["pkexec".to_string(), env.system_helper_path.to_string()],
                // Язык сообщений помощника (решение 30.09.2026).
                crate::system_l10n::lang_args(env.system_helper_path),
                vec![
                    "sddm".to_string(),
                    theme_name.to_string(),
                    staged.clone(),
                    env.helper_family.to_string(),
                ],
            ]
            .concat(),
            timeout_secs: Some(300),
        });
    } else {
        steps.push(ExtStep::PkexecScript {
            prefix: "rexon_sddm".to_string(),
            script: sddm_fallback_script(theme_name, src),
        });
    }
    steps.push(ExtStep::RemoveTree {
        path: staged,
        safe: false,
        ignore_errors: true,
    });
}

/// Текст fallback-скрипта `_stage_sddm`: только копия темы, без выбора.
pub fn sddm_fallback_script(theme_name: &str, src: &str) -> String {
    let dest = format!("/usr/share/sddm/themes/{theme_name}");
    let qsrc = shlex_quote(src);
    let qdest = shlex_quote(&dest);
    let qtheme = shlex_quote(theme_name);
    [
        "#!/bin/bash".to_string(),
        "set -e".to_string(),
        format!("rm -rf {qdest}"),
        format!("install -d {qdest}"),
        format!("cp -rT {qsrc} {qdest}"),
        format!("chmod -R a+rX {qdest}"),
        // Выбор темы (`zz-rexon-theme.conf`) — кнопка «Применить» раздела
        // экрана входа (решение 04.10.2026), установка только кладёт файлы.
        format!("echo \"SDDM theme installed: \"{qtheme}"),
    ]
    .join("\n")
        + "\n"
}

/// Порт `_install_plymouth` + `_stage_plymouth`.
fn install_plymouth_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    data_dir: &str,
    installed_names: &[String],
    steps: &mut Vec<ExtStep>,
) {
    let names: Vec<String> = if installed_names.is_empty() {
        vec![ext.ext_id()]
    } else {
        installed_names.to_vec()
    };
    let theme_name = names[0].clone();
    let mut src = join(data_dir, &theme_name);
    if !env.fs.is_dir(&src) {
        src = data_dir.to_string();
    }

    let has_plymouth_conf = env.fs.scandir(&src).iter().any(|f| {
        f.ends_with(".plymouth") && env.fs.is_file(&join(&src, f))
    });

    if !has_plymouth_conf {
        // Автогенерация .plymouth (+ простой .script при наличии картинки).
        let logo = env
            .fs
            .scandir(&src)
            .iter()
            .find(|f| {
                let l = f.to_lowercase();
                l.ends_with(".png") || l.ends_with(".svg")
            })
            .cloned()
            .unwrap_or_default();
        steps.push(ExtStep::WriteFile {
            path: join(&src, &format!("{theme_name}.plymouth")),
            content: plymouth_autogen_conf(&ext.name(), &ext.description(), &theme_name),
        });
        if !logo.is_empty() {
            steps.push(ExtStep::WriteFile {
                path: join(&src, &format!("{theme_name}.script")),
                content: plymouth_autogen_script(&logo),
            });
        }
    }

    // initramfs здесь не пересобирается: тема только кладётся, а выбирает её
    // (и пересобирает образ) кнопка «Применить» (решение 04.10.2026).

    if !is_safe_sys_name(&theme_name) {
        return; // error в логе
    }
    // _stage_plymouth (guard уже пройден; src — каталог по построению).
    let staged = env.tmp.mkdtemp("rexon_stage_");
    steps.push(ExtStep::Stage {
        staged: staged.clone(),
        sources: vec![src.clone()],
    });
    if env.system_helper_ok() {
        steps.push(ExtStep::Run {
            argv: [
                vec!["pkexec".to_string(), env.system_helper_path.to_string()],
                // Язык сообщений помощника (решение 30.09.2026).
                crate::system_l10n::lang_args(env.system_helper_path),
                vec![
                    "plymouth".to_string(),
                    theme_name.clone(),
                    staged.clone(),
                    env.helper_family.to_string(),
                ],
            ]
            .concat(),
            timeout_secs: Some(900),
        });
    } else {
        steps.push(ExtStep::PkexecScript {
            prefix: "rexon_plymouth".to_string(),
            script: plymouth_fallback_script(&theme_name, &src),
        });
    }
    steps.push(ExtStep::RemoveTree {
        path: staged,
        safe: false,
        ignore_errors: true,
    });
}

/// Автогенерированный `.plymouth`-конфиг (байт-в-байт с Python).
pub fn plymouth_autogen_conf(name: &str, description: &str, theme_name: &str) -> String {
    let desc = if description.is_empty() {
        "Rexon Plymouth Theme"
    } else {
        description
    };
    format!(
        "[Plymouth Theme]\n\
         Name={name}\n\
         Description={desc}\n\
         ModuleName=script\n\n\
         [script]\n\
         ImageDir=/usr/share/plymouth/themes/{theme_name}\n\
         ScriptFile=/usr/share/plymouth/themes/{theme_name}/{theme_name}.script\n"
    )
}

/// Автогенерированный Plymouth-скрипт (центрированный логотип, байт-в-байт).
pub fn plymouth_autogen_script(logo: &str) -> String {
    format!(
        "logo = Image(\"{logo}\");\n\
         logo_sprite = Sprite(logo);\n\
         logo_sprite.SetX(Window.GetWidth() / 2 - logo.GetWidth() / 2);\n\
         logo_sprite.SetY(Window.GetHeight() / 2 - logo.GetHeight() / 2);\n\
         \n\
         fun refresh_callback() {{\n\
         \x20   logo_sprite.SetOpacity(1);\n\
         }}\n\
         Plymouth.SetRefreshFunction(refresh_callback);\n"
    )
}

/// Текст fallback-скрипта `_stage_plymouth`: только копия темы.
///
/// Выбор темы (`plymouth-set-default-theme`) и пересборка образа ядра —
/// кнопка «Применить» раздела заставки загрузки через системный помощник
/// (решение 04.10.2026); прежде установка делала их сразу.
pub fn plymouth_fallback_script(theme_name: &str, src: &str) -> String {
    let dest = format!("/usr/share/plymouth/themes/{theme_name}");
    let qsrc = shlex_quote(src);
    let qdest = shlex_quote(&dest);
    [
        "#!/bin/bash".to_string(),
        "set -e".to_string(),
        format!("rm -rf {qdest}"),
        format!("install -d {qdest}"),
        format!("cp -rT {qsrc} {qdest}"),
        format!("chmod -R a+rX {qdest}"),
    ]
    .join("\n")
        + "\n"
}

/// Порт `_install_conky` (без запуска — Popen как шаг Spawn).
fn install_conky_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    data_dir: &str,
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let theme_id = ext.ext_id();
    let dest_dir = format!("{}/.config/conky/{}", env.home, theme_id);

    if pfs.exists(&dest_dir) {
        steps.push(ExtStep::RemoveTree {
            path: dest_dir.clone(),
            safe: true,
            ignore_errors: true,
        });
    }
    steps.push(ExtStep::CopyTree {
        src: data_dir.to_string(),
        dest: dest_dir.clone(),
    });
    pfs.mk_dir(&dest_dir);

    // Найти конфиг Conky. Листинг dest_dir — из снимка: там записано то,
    // что Python видел ПОСЛЕ своего copytree.
    let mut conky_conf = String::new();
    for f in env.fs.scandir(&dest_dir) {
        if f == "conky.conf" || f == ".conkyrc" || f.ends_with(".conkyrc") {
            conky_conf = join(&dest_dir, f);
            break;
        }
    }
    if conky_conf.is_empty() {
        'outer: for (root, _dirs, files) in env.fs.walk(&dest_dir) {
            for f in &files {
                if f.to_lowercase().contains("conky")
                    && (f.ends_with(".conf") || f.ends_with(".conkyrc"))
                {
                    conky_conf = join(&root, f);
                    break 'outer;
                }
            }
        }
    }
    if conky_conf.is_empty() {
        return; // warning
    }

    // Проверка опасных директив — только лог, шагов нет.

    // Шрифты (fonts/ внутри dest — скопирован нашим же шагом → наблюдения).
    let fonts_dir = join(&dest_dir, "fonts");
    if env.fs.obs_is_dir(&fonts_dir) {
        let user_fonts = format!("{}/.local/share/fonts", env.home);
        steps.push(ExtStep::MakeDirs { path: user_fonts.clone() });
        pfs.mk_dir(&user_fonts);
        for f in env.fs.scandir(&fonts_dir) {
            if is_supported_font(f) {
                let dst = join(&user_fonts, f);
                if !pfs.exists(&dst) {
                    steps.push(ExtStep::CopyFile {
                        src: join(&fonts_dir, f),
                        dest: dst.clone(),
                    });
                    pfs.mk_file(&dst);
                }
            }
        }
        steps.push(run(vec!["fc-cache", "-f"], 30));
    }

    // ⛔ Запуск и автозапуск — кнопкой «▶ Запустить» и переключателем
    // «Запускать при входе» вкладки «Оформление» (`crate::de_apply`, решение
    // 04.10.2026): установка только кладёт виджет и его шрифты. Прежняя
    // ветка (автозапуск при установке, `pkill -f conky` по ВСЕМ conky, в том
    // числе чужим, и запуск сразу) после того решения стала недостижимой и
    // снята; оговорка про Wayland приходит в итоге кнопки «Запустить».
}

/// Текст autostart-`.desktop` для Conky.
///
/// Виджет запускается из папки своего файла настроек — так же, как кнопкой
/// «Запустить» ([`crate::de_apply::conky_work_dir`]). Ключ `Path=` знают не
/// все запускальщики автозапуска, поэтому переход в папку делает сама
/// строка запуска: `sh -c` с постоянным сценарием
/// ([`crate::de_apply::CONKY_AUTOSTART_SCRIPT`]), путь — отдельным
/// аргументом по правилам `Exec=` ([`crate::installer::desktop_exec_arg`]).
/// `Path=` остаётся подсказкой для тех, кто его читает. Прежняя строка
/// `Exec=conky -c '<путь>'` запускала виджет из домашней папки (его
/// относительные пути уходили мимо), а апостроф в имени папки ломал её.
pub fn conky_desktop_entry(name: &str, conky_conf: &str) -> String {
    conky_desktop_entry_via(name, conky_conf, None)
}

/// [`conky_desktop_entry`] с запуском через накладку Rexon `launch`
/// ([`crate::conky_overlay`]): conky получает накладку, а рабочий каталог —
/// по-прежнему папка файла автора
/// ([`crate::de_apply::CONKY_OVERLAY_AUTOSTART_SCRIPT`]).
pub fn conky_desktop_entry_via(name: &str, conky_conf: &str, launch: Option<&str>) -> String {
    conky_desktop_entry_with(name, conky_conf, launch, 0, &[])
}

/// [`conky_desktop_entry_via`] с задержкой при входе и окружениями.
///
/// Задержка — `sleep` в самой строке запуска, а не `X-GNOME-Autostart-Delay`:
/// тот ключ исполняют только GNOME, Cinnamon, MATE и Budgie, а KDE, XFCE,
/// LXQt и `systemd-xdg-autostart-generator` его молча пропускают. Число —
/// ещё и в ключе [`crate::conky_autostart::DELAY_KEY`], чтобы перезапись
/// записи его не теряла. `only_in` — `OnlyShowIn=` («только в этом
/// окружении»). `X-KDE-autostart-after=panel` — Plasma запускает виджет
/// после панели, и он не встаёт под неё.
pub fn conky_desktop_entry_with(
    name: &str,
    conky_conf: &str,
    launch: Option<&str>,
    delay: u32,
    only_in: &[String],
) -> String {
    use crate::installer::desktop_exec_arg;
    let script = |base: &str| {
        if delay > 0 { format!("sleep {delay} && {base}") } else { base.to_string() }
    };
    let exec = match launch {
        None => format!(
            "sh -c {} {}",
            desktop_exec_arg(&script(crate::de_apply::CONKY_AUTOSTART_SCRIPT)),
            desktop_exec_arg(conky_conf)
        ),
        Some(overlay) => format!(
            "sh -c {} {} {}",
            desktop_exec_arg(&script(crate::de_apply::CONKY_OVERLAY_AUTOSTART_SCRIPT)),
            desktop_exec_arg(overlay),
            desktop_exec_arg(conky_conf)
        ),
    };
    // Значение-строка ключа: обратная косая удваивается, перевод строки — `\n`.
    let dir = crate::de_apply::conky_work_dir(conky_conf).replace('\\', "\\\\").replace('\n', "\\n");
    let mut extra = String::new();
    if !only_in.is_empty() {
        extra.push_str(&format!("OnlyShowIn={};\n", only_in.join(";")));
    }
    if delay > 0 {
        extra.push_str(&format!("{}={delay}\n", crate::conky_autostart::DELAY_KEY));
    }
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Conky — {name}\n\
         Comment=Rexon Conky widget: {name}\n\
         Exec={exec}\n\
         Path={dir}\n\
         {extra}\
         X-GNOME-Autostart-enabled=true\n\
         X-KDE-autostart-after=panel\n\
         StartupNotify=false\n\
         Terminal=false\n"
    )
}

/// Что ставить из каталога `dir` пакета: (архив `.plasmoid`, каталог
/// плазмоида) — первое по порядку имён; пустая строка — нет такого. Сам
/// `dir` с описанием — тоже каталог плазмоида.
fn plasmoid_source(env: &ExtEnv, dir: &str) -> (String, String) {
    let has_meta = |d: &str| env.fs.is_file(&join(d, "metadata.json")) || env.fs.is_file(&join(d, "metadata.desktop"));
    for f in env.fs.scandir(dir) {
        let full = join(dir, f);
        if f.ends_with(".plasmoid") {
            return (full, String::new());
        }
        if env.fs.is_dir(&full) && has_meta(&full) {
            return (String::new(), full);
        }
    }
    if has_meta(dir) {
        return (String::new(), dir.to_string());
    }
    (String::new(), String::new())
}

/// Id плазмоида-каталога `dir`: `KPlugin.Id` из metadata.json, иначе
/// `X-KDE-PluginInfo-Name` из metadata.desktop.
fn plasmoid_dir_id(env: &ExtEnv, dir: &str) -> Option<String> {
    let meta_json = join(dir, "metadata.json");
    if env.fs.is_file(&meta_json) {
        return env
            .fs
            .read_text(&meta_json)
            .and_then(|t| serde_json::from_str::<Value>(t).ok())
            .and_then(|v| v["KPlugin"]["Id"].as_str().map(String::from));
    }
    env.fs
        .read_text(&join(dir, "metadata.desktop"))
        .and_then(|t| ini_get(t, "Desktop Entry", "X-KDE-PluginInfo-Name"))
}

/// Каталог плазмоида `id` в профиле.
fn plasmoid_dest(env: &ExtEnv, id: &str) -> String {
    format!("{}/.local/share/plasma/plasmoids/{}", env.home, id)
}

/// `KPlugin.Id` (у старых — `X-KDE-PluginInfo-Name`) первого описания в
/// дереве `root` снимка.
fn plasmoid_tree_id(fs: &FsSnap, root: &str) -> Option<String> {
    for (dir, _dirs, files) in fs.walk(root) {
        if files.iter().any(|f| f == "metadata.json") {
            return json_field(fs, &join(&dir, "metadata.json"), "KPlugin.Id");
        }
        if files.iter().any(|f| f == "metadata.desktop") {
            return fs
                .read_text(&join(&dir, "metadata.desktop"))
                .and_then(|t| ini_get(t, "Desktop Entry", "X-KDE-PluginInfo-Name"));
        }
    }
    None
}

/// Id плазмоида в архиве `plasmoid_file` — по распаковке, которую снимок
/// сделал заранее ([`PLASMOID_PROBE_DIR`]). `None` — пробы нет (архив
/// битый) либо описания в нём нет.
fn plasmoid_archive_id(env: &ExtEnv, plasmoid_file: &str) -> Option<String> {
    let name = plasmoid_file.rsplit('/').next().unwrap_or(plasmoid_file);
    plasmoid_tree_id(env.fs, &env.tmp.path(&format!("{PLASMOID_PROBE_DIR}/{name}")))
}

/// На месте `dest` стоит плазмоид, положенный не этим пакетом (руками,
/// магазином, другим пакетом, прежним Rexon без метки). Каталог, который
/// положил этот же план (общая копия подпапки `data/` с именем, равным
/// id), — свой.
fn plasmoid_taken(env: &ExtEnv, ext: &ExtInfo, pfs: &PlanFs, dest: &str) -> bool {
    !pfs.made_dirs.contains(dest) && env.fs.lexists(dest) && !owned_by(env.fs, dest, &ext.ext_id())
}

/// Пропуск плазмоида `id`: заметка с причиной.
fn skip_plasmoid(id: &str, dest: &str, pfs: &mut PlanFs, steps: &mut Vec<ExtStep>) {
    // Подпапка `data/` с именем, равным id, уже пропущена общим циклом с
    // той же причиной — второй раз её не повторять.
    let notice = taken_by_other_notice(id, dest);
    if !steps.contains(&notice) {
        steps.push(notice);
    }
    pfs.skipped += 1;
}

/// Метка владельца в каталоге плазмоида `dest`.
fn plasmoid_mark(ext: &ExtInfo, dest: &str) -> ExtStep {
    ExtStep::WriteFile { path: join(dest, OWNER_MARK), content: format!("{}\n", ext.ext_id()) }
}

/// Порт `_install_plasmoid` (+ `_install_plasmoid_manual`).
///
/// Правило владельца (как у основной копии темы): каталог
/// `~/.local/share/plasma/plasmoids/<KPlugin.Id>` чужого плазмоида с тем же
/// id не снимается и не обновляется `kpackagetool -u` — плазмоид
/// пропускается с заметкой. У архива `.plasmoid` id известен только после
/// распаковки: его заранее распаковывает снимок плана, так что решение
/// принимается ДО первого запуска `kpackagetool` (пробы `run_ok` исполняют
/// его уже при сборке плана).
fn install_plasmoid_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    data_dir: &str,
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let (plasmoid_file, plasmoid_dir) = plasmoid_source(env, data_dir);

    if !plasmoid_file.is_empty() {
        let probe_id = plasmoid_archive_id(env, &plasmoid_file);
        if let Some(id) = &probe_id {
            let dest = plasmoid_dest(env, id);
            if plasmoid_taken(env, ext, pfs, &dest) {
                skip_plasmoid(id, &dest, pfs, steps);
                return;
            }
        }
        let tool = (env.which)("kpackagetool6").or_else(|| (env.which)("kpackagetool5"));
        if let Some(tool) = tool {
            // Сначала обновить, потом установить.
            steps.push(ExtStep::Run {
                argv: vec![tool.clone(), "-u".into(), plasmoid_file.clone()],
                timeout_secs: Some(30),
            });
            let install_argv = vec![tool.clone(), "-i".into(), plasmoid_file.clone()];
            steps.push(ExtStep::Run {
                argv: install_argv.clone(),
                timeout_secs: Some(30),
            });
            if (env.run_ok)(&install_argv) {
                // Метка и у плазмоида, поставленного инструментом: по ней
                // удаление и повтор узнают своё.
                if let Some(id) = &probe_id {
                    steps.push(plasmoid_mark(ext, &plasmoid_dest(env, id)));
                }
            } else {
                install_plasmoid_manual_plan(env, ext, &plasmoid_file, pfs, steps);
            }
        } else {
            install_plasmoid_manual_plan(env, ext, &plasmoid_file, pfs, steps);
        }
    } else if !plasmoid_dir.is_empty() {
        let plugin_id = plasmoid_dir_id(env, &plasmoid_dir).unwrap_or_else(|| ext.ext_id());
        let dest = plasmoid_dest(env, &plugin_id);
        if plasmoid_taken(env, ext, pfs, &dest) {
            skip_plasmoid(&plugin_id, &dest, pfs, steps);
            return;
        }
        if pfs.exists(&dest) {
            steps.push(ExtStep::RemoveTree {
                path: dest.clone(),
                safe: false,
                ignore_errors: false,
            });
        }
        steps.push(ExtStep::CopyTree { src: plasmoid_dir, dest: dest.clone() });
        pfs.mk_dir(&dest);
        steps.push(plasmoid_mark(ext, &dest));
    }
    // ничего не найдено — warning
}

/// Порт `_install_plasmoid_manual`: распаковать .plasmoid и скопировать.
/// Листинги распакованного дерева — из снимка (Python реально распаковывал).
/// Чужой плазмоид с тем же id — пропуск с заметкой (ничего не распаковано).
fn install_plasmoid_manual_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    plasmoid_file: &str,
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let tmp = env.tmp.mkdtemp("rexon_plasmoid_");

    let mut plugin_id = ext.ext_id();
    for (root, _dirs, files) in env.fs.walk(&tmp) {
        if files.iter().any(|f| f == "metadata.json") {
            if let Some(text) = env.fs.read_text(&join(&root, "metadata.json")) {
                if let Ok(v) = serde_json::from_str::<Value>(text) {
                    if let Some(id) = v["KPlugin"]["Id"].as_str() {
                        plugin_id = id.to_string();
                    }
                }
            }
            break;
        }
    }

    let dest = plasmoid_dest(env, &plugin_id);
    if plasmoid_taken(env, ext, pfs, &dest) {
        skip_plasmoid(&plugin_id, &dest, pfs, steps);
        return;
    }
    steps.push(ExtStep::ExtractZip {
        archive: plasmoid_file.to_string(),
        dest: tmp.clone(),
    });
    if env.fs.exists(&dest) {
        steps.push(ExtStep::RemoveTree {
            path: dest.clone(),
            safe: false,
            ignore_errors: false,
        });
    }

    let mut src = tmp.clone();
    for (root, dirs, files) in env.fs.walk(&tmp) {
        if files.iter().any(|f| f == "metadata.json")
            || dirs.iter().any(|d| d == "contents")
        {
            src = root;
            break;
        }
    }
    steps.push(ExtStep::CopyTree { src, dest: dest.clone() });
    pfs.mk_dir(&dest);
    steps.push(plasmoid_mark(ext, &dest));
    steps.push(ExtStep::RemoveTree {
        path: tmp,
        safe: false,
        ignore_errors: true,
    });
}

/// Порт `_install_bundle`: полный тематический пакет — по типам из manifest
/// `bundle` (порядок ключей манифеста).
fn install_bundle_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let Some(bundle_map) = ext.manifest.get("bundle").and_then(Value::as_object) else {
        return; // warning: поле 'bundle' отсутствует
    };

    for (sub_type, sub_dir_rel) in bundle_map {
        if !constants::is_valid_type(sub_type) || sub_type == "theme-bundle" {
            continue;
        }
        let sub_dir = join(ext.dir, &py_str(sub_dir_rel));
        if !env.fs.is_dir(&sub_dir) {
            continue;
        }
        let Some(target_base) = env.install_path(sub_type) else {
            continue; // нет пути установки (wallpaper-pack)
        };
        steps.push(ExtStep::MakeDirs { path: target_base.clone() });
        pfs.mk_dir(&target_base);

        match sub_type.as_str() {
            "theme-conky" => install_conky_plan(env, ext, &sub_dir, pfs, steps),
            "theme-plymouth" => {
                let names = subdir_names_or_id(env.fs, &sub_dir, &ext.ext_id());
                install_plymouth_plan(env, ext, &sub_dir, &names, steps);
            }
            "theme-login-screen" => {
                let names = subdir_names_or_id(env.fs, &sub_dir, &ext.ext_id());
                install_login_screen_plan(env, ext, &sub_dir, &names, steps);
            }
            "theme-window-decoration" => {
                let names = subdir_names(env.fs, &sub_dir);
                // Грабля оригинала сохранена: bundle НЕ копирует декорации в
                // INSTALL_PATHS перед вызовом — ветки GNOME/XFCE не найдут
                // src и ничего не скопируют (KDE-ветка всё равно пишет
                // kwinrc).
                install_window_decoration_plan(env, ext, &names, pfs, steps);
            }
            "theme-plasmoid" => install_plasmoid_plan(env, ext, &sub_dir, pfs, steps),
            "font-pack" => {
                for f in env.fs.scandir(&sub_dir) {
                    if is_supported_font(f) {
                        // Общий каталог шрифтов: одноимённый шрифт человека
                        // с другим содержимым не перезаписывается.
                        let dest = join(&target_base, f);
                        if !put_owned_file(env, pfs, &join(&sub_dir, f), &dest, steps) {
                            steps.push(taken_file_notice(f, &dest));
                            pfs.skipped += 1;
                        }
                    }
                }
                steps.push(run(vec!["fc-cache", "-f"], 30));
            }
            _ => {
                // Стандартная установка: подпапки и файлы → целевой путь.
                // Тема части комплекта ложится под своим именем в общий
                // каталог вида — по правилу владельца, как основная копия
                // отдельной установки: чужая тема (файл) с тем же именем не
                // снимается и не перезаписывается, причина — заметкой.
                for name in env.fs.scandir(&sub_dir) {
                    let entry = join(&sub_dir, name);
                    let dest = join(&target_base, name);
                    if env.fs.is_dir(&entry) {
                        if !put_owned_copy(env, ext, pfs, &entry, &dest, steps) {
                            steps.push(taken_by_other_notice(name, &dest));
                            pfs.skipped += 1;
                        }
                    } else if env.fs.is_file(&entry) && !put_owned_file(env, pfs, &entry, &dest, steps) {
                        steps.push(taken_file_notice(name, &dest));
                        pfs.skipped += 1;
                    }
                }
                // Тему из комплекта тоже не применяем: её выбирает
                // «Применить» своего раздела, как у отдельной установки.
            }
        }
    }
}

fn subdir_names(fs: &FsSnap, dir: &str) -> Vec<String> {
    fs.scandir(dir)
        .iter()
        .filter(|n| fs.is_dir(&join(dir, n)))
        .cloned()
        .collect()
}

fn subdir_names_or_id(fs: &FsSnap, dir: &str, ext_id: &str) -> Vec<String> {
    let names = subdir_names(fs, dir);
    if names.is_empty() {
        vec![ext_id.to_string()]
    } else {
        names
    }
}

// ============================================================================
// _safe_uninstall как план
// ============================================================================

/// Шаг снятия системной темы (`theme-plymouth` / `theme-login-screen`),
/// поставленной из `.rxn`: имя — то же, что выбрала установка (первая
/// подпапка данных либо id расширения). Только если помощник есть, а тема
/// лежит в `/usr/share/<вид>/themes` обычным каталогом.
fn system_theme_remove_steps(
    env: &ExtEnv,
    ext: &ExtInfo,
    ext_type: &str,
    data_dir: &str,
    steps: &mut Vec<ExtStep>,
) {
    let kind = if ext_type == "theme-plymouth" { "plymouth" } else { "sddm" };
    let Some(name) = subdir_names_or_id(env.fs, data_dir, &ext.ext_id()).into_iter().next() else {
        return;
    };
    if !is_safe_sys_name(&name) || !env.system_helper_ok() {
        return;
    }
    let dest = format!("/usr/share/{kind}/themes/{name}");
    if !env.fs.is_dir(&dest) || env.fs.is_link(&dest) {
        return;
    }
    steps.push(ExtStep::Run {
        argv: crate::de_apply::system_remove_argv(
            env.system_helper_path,
            &crate::system_l10n::lang_args(env.system_helper_path),
            kind,
            &name,
            env.helper_family,
        ),
        timeout_secs: Some(crate::installer::SYSTEM_INSTALL_TIMEOUT_SECS),
    });
}

/// Порт `_safe_uninstall`: удаление файлов оформления из системных путей.
/// (Финальный `_safe_rmtree(ext.dir)` делает `uninstall()` — вне этого плана.)
pub fn safe_uninstall_plan(env: &ExtEnv, ext: &ExtInfo) -> Vec<ExtStep> {
    let mut steps = Vec::new();
    let ext_type = ext.ext_type();
    let target_base = env.install_path(&ext_type);

    // v4 adaptive: убрать симлинк из галереи.
    if ext_type == "wallpaper-video-adaptive" {
        if let Some(base) = target_base {
            let link = join(&base, &ext.ext_id());
            if env.fs.is_link(&link) {
                steps.push(ExtStep::RemoveFile { path: link });
            }
        }
        return steps;
    }

    if ext_type == "font-pack" {
        // Extension.remove_fonts.
        let dest_dir = format!(
            "{}/.local/share/fonts/rexon-{}",
            env.home,
            ext.ext_id()
        );
        if env.fs.is_dir(&dest_dir) {
            steps.push(ExtStep::RemoveTree {
                path: dest_dir,
                safe: false,
                ignore_errors: true,
            });
            steps.push(run(vec!["fc-cache", "-f"], 30));
        }
        return steps;
    }

    if ext_type == "theme-conky" {
        let conky_dir = format!("{}/.config/conky/{}", env.home, ext.ext_id());
        if env.fs.is_dir(&conky_dir) {
            steps.push(ExtStep::RemoveTree {
                path: conky_dir,
                safe: true,
                ignore_errors: true,
            });
        }
        let autostart = format!(
            "{}/.config/autostart/rexon-conky-{}.desktop",
            env.home,
            ext.ext_id()
        );
        if env.fs.is_file(&autostart) {
            steps.push(ExtStep::RemoveFile { path: autostart });
        }
        // Копии подпапок `data/` в `~/.config/conky/<подпапка>` — только
        // свои, по метке (общая `fonts` бывает у другого виджета).
        if let Some(base) = &target_base {
            for name in subdir_names(env.fs, &join(ext.dir, "data")) {
                if name != ext.ext_id() {
                    remove_owned_copy(env, ext, &join(base, &name), &mut steps);
                }
            }
        }
        // Процессы виджета гасит исполнитель ДО плана — по номерам, найденным
        // по файлу настроек (`de_apply::conky_forget_widget`). Прежний
        // `pkill -f conky.*<id>` бил по образцу командной строки и задевал
        // любой процесс, где рядом стоят «conky» и имя виджета.
        return steps;
    }

    if ext_type == "theme-plasmoid" {
        let data_dir = join(ext.dir, "data");
        // Определить plugin id из metadata. Порядок оригинала сохранён:
        // внешний цикл по (data/, корень) НЕ прерывается после первой
        // находки — второй кандидат может перезаписать plugin_id (в т.ч.
        // на None при metadata.json без KPlugin.Id). Но None больше не
        // означает «id пакета»: тогда id берётся тем же выбором, что у
        // установки (ниже), — удаление снимает тот каталог, который она
        // положила, а не одноимённый пакету (25.10.2026).
        let mut plugin_id: Option<String> = None;
        for candidate in [data_dir, ext.dir.to_string()] {
            for meta_name in ["metadata.json", "metadata.desktop"] {
                let meta_path = join(&candidate, meta_name);
                if env.fs.is_file(&meta_path) {
                    if meta_name.ends_with(".json") {
                        plugin_id = env
                            .fs
                            .read_text(&meta_path)
                            .and_then(|t| serde_json::from_str::<Value>(t).ok())
                            .and_then(|v| {
                                v["KPlugin"]["Id"].as_str().map(String::from)
                            });
                    }
                    // metadata.desktop — fallback на ext_id (не парсится).
                    break;
                }
            }
        }
        // Id не нашёлся — тот же выбор, что у установки: подпапка data/ с
        // описанием либо архив `.plasmoid` (id — из распаковки, которую
        // снимок сделал заранее). Иначе снимался бы каталог с именем id
        // пакета, а не плазмоида.
        let plugin_id = plugin_id
            .or_else(|| {
                let (file, dir) = plasmoid_source(env, &join(ext.dir, "data"));
                if !file.is_empty() {
                    plasmoid_archive_id(env, &file)
                } else if !dir.is_empty() {
                    plasmoid_dir_id(env, &dir)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| ext.ext_id());
        let plasmoid_dir = plasmoid_dest(env, &plugin_id);
        // Плазмоид с этим id стоит, но положен не этим пакетом (установка
        // его пропустила; либо копия прежнего Rexon без метки — её снимает
        // общий журнал, если путь в нём записан): ни `kpackagetool -r`, ни
        // удаления.
        if env.fs.lexists(&plasmoid_dir) && !owned_by(env.fs, &plasmoid_dir, &ext.ext_id()) {
            return steps;
        }
        if let Some(tool) =
            (env.which)("kpackagetool6").or_else(|| (env.which)("kpackagetool5"))
        {
            steps.push(ExtStep::Run {
                argv: vec![tool, "-r".into(), plugin_id.clone()],
                timeout_secs: Some(30),
            });
        }
        if env.fs.is_dir(&plasmoid_dir) {
            steps.push(ExtStep::RemoveTree {
                path: plasmoid_dir,
                safe: true,
                ignore_errors: true,
            });
        }
        return steps;
    }

    // Plymouth / экран входа: каталог в /usr/share снимает системный
    // помощник (режим remove-<вид>). Выбранную сейчас тему и файлы пакета
    // системы он не трогает и отвечает причиной.
    if ext_type == "theme-plymouth" || ext_type == "theme-login-screen" {
        system_theme_remove_steps(env, ext, &ext_type, &join(ext.dir, "data"), &mut steps);
        return steps;
    }

    // Кастомизация окружений — тот же generic-путь, что и при установке.
    if let Some(row) = de_customization::de_managed_row(&ext_type) {
        uninstall_de_custom_plan(env, ext, row, &mut steps);
        return steps;
    }

    if ext_type == "theme-bundle" {
        let bundle = ext
            .manifest
            .get("bundle")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let data_dir = join(ext.dir, "data");
        for (sub_type, sub_folder) in &bundle {
            if sub_type == "theme-plymouth" || sub_type == "theme-login-screen" {
                // Тот же каталог, что берёт установка комплекта.
                let sub_dir = join(ext.dir, &py_str(sub_folder));
                if env.fs.is_dir(&sub_dir) {
                    system_theme_remove_steps(env, ext, sub_type, &sub_dir, &mut steps);
                }
            }
            let Some(sub_path) = env.install_path(sub_type) else {
                continue;
            };
            let folder = py_str(sub_folder);
            let base_name = folder.rsplit('/').next().unwrap_or(&folder);
            let sub_data = join(&data_dir, base_name);
            if !env.fs.is_dir(&sub_data) {
                continue;
            }
            // Только своё: каталог с меткой этого пакета, файл из его
            // журнала или байт в байт его (чужое установка пропустила).
            for name in env.fs.scandir(&sub_data) {
                let entry = join(&sub_data, name);
                let dest = join(&sub_path, name);
                if env.fs.is_dir(&entry) {
                    remove_owned_copy(env, ext, &dest, &mut steps);
                } else if env.fs.is_file(&entry) && file_is_ours(env.fs, &entry, &dest) {
                    steps.push(ExtStep::RemoveFile { path: dest });
                }
            }
        }
        // Копии подпапок `data/` в `~/.local/share/rexon/bundles` — свои.
        if let Some(base) = &target_base {
            for name in subdir_names(env.fs, &data_dir) {
                remove_owned_copy(env, ext, &join(base, &name), &mut steps);
            }
        }
        return steps;
    }

    let Some(target_base) = target_base else {
        return steps;
    };
    let data_dir = join(ext.dir, "data");
    if !env.fs.is_dir(&data_dir) {
        return steps;
    }
    for name in env.fs.scandir(&data_dir) {
        let entry = join(&data_dir, name);
        let dest = join(&target_base, name);
        if env.fs.is_dir(&entry) {
            // Только свою копию (метка этого пакета): тему человека с тем же
            // именем установка пропустила — удаление её тоже не трогает.
            // Копия прежнего Rexon без метки остаётся на месте, если путь не
            // записан в общем журнале (тогда её снимает журнал).
            if owned_by(env.fs, &dest, &ext.ext_id()) {
                steps.push(ExtStep::RemoveTree {
                    path: dest,
                    safe: false,
                    ignore_errors: true,
                });
            }
            // Рамки вне KDE: копия в `~/.themes/<тема>` — если она наша
            // (стол мог смениться после установки, поэтому без оглядки на
            // нынешний).
            if ext_type == "theme-window-decoration" {
                remove_owned_copy(env, ext, &format!("{}/.themes/{name}", env.home), &mut steps);
            }
        } else if env.fs.is_file(&entry) && file_is_ours(env.fs, &entry, &dest) {
            // KDE color schemes (.colors files) — только свой файл: схему
            // человека с тем же именем установка пропустила.
            steps.push(ExtStep::RemoveFile { path: dest });
        }
    }
    steps
}

// ============================================================================
// Обнаружение, каталог, состояние
// ============================================================================

/// Порт `_discover`: скан каталога расширений. Возвращает список в порядке
/// вставки Python-dict; каталог с дублирующимся id ПЕРЕЗАПИСЫВАЕТ более
/// ранний на месте (семантика dict[ext_id] = ext — грабля сохранена).
/// Битые расширения пропускаются (как `except Exception`).
pub fn discover(
    fs: &FsSnap,
    extensions_dir: &str,
    state: &Map<String, Value>,
) -> Vec<Extension> {
    let mut order: Vec<String> = Vec::new();
    let mut by_id: HashMap<String, Extension> = HashMap::new();
    for name in fs.scandir(extensions_dir) {
        let dir = join(extensions_dir, name);
        if !fs.is_dir(&dir) {
            continue;
        }
        let manifest_path = join(&dir, "manifest.json");
        if !fs.is_file(&manifest_path) {
            continue;
        }
        let Some(text) = fs.read_text(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<Value>(text) else {
            continue; // JSONDecodeError → log.exception + skip
        };
        let Ok(ext) = Extension::from_manifest(manifest, &dir, state) else {
            continue; // ValueError → log.exception + skip
        };
        let id = ext.ext_id().to_string();
        if !by_id.contains_key(&id) {
            order.push(id.clone());
        }
        by_id.insert(id, ext);
    }
    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

/// Порт `get_stats`: количество по типам и категориям.
/// Порядок ключей by_type — порядок `VALID_TYPES` (в Python — set, порядок
/// его итерации; сверка JSON-объектов нечувствительна к порядку).
pub fn stats(exts: &[(String, bool)]) -> Map<String, Value> {
    let total = exts.len();
    let enabled = exts.iter().filter(|(_, e)| *e).count();
    let mut by_type = Map::new();
    for t in constants::valid_types() {
        let n = exts.iter().filter(|(ty, _)| ty == t).count();
        by_type.insert(t.to_string(), Value::from(n));
    }
    let appearance = exts
        .iter()
        .filter(|(ty, _)| constants::is_appearance(ty))
        .count();
    let extensions = exts
        .iter()
        .filter(|(ty, _)| constants::is_extension(ty))
        .count();
    let mut by_category = Map::new();
    by_category.insert("appearance".to_string(), Value::from(appearance));
    by_category.insert("extensions".to_string(), Value::from(extensions));

    let mut out = Map::new();
    out.insert("total".to_string(), Value::from(total));
    out.insert("enabled".to_string(), Value::from(enabled));
    out.insert("by_type".to_string(), Value::Object(by_type));
    out.insert("by_category".to_string(), Value::Object(by_category));
    out
}

/// Порт `_save_state` (текстовая часть): `json.dump(state, indent=2,
/// ensure_ascii=False)` — байт-в-байт. Порядок пар — порядок словаря
/// `self.extensions`.
pub fn save_state_text(state: &[(String, bool)]) -> String {
    let mut m = Map::new();
    for (id, enabled) in state {
        m.insert(id.clone(), Value::Bool(*enabled));
    }
    dumps_indent2(&Value::Object(m))
}

/// Порт `_load_state`: разбор JSON-состояния; любые проблемы → пустой dict.
pub fn load_state(text: Option<&str>) -> Map<String, Value> {
    let Some(text) = text else {
        return Map::new();
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

// ============================================================================
// Хелперы
// ============================================================================

/// `json.dumps(v, indent=2, ensure_ascii=False)` — serde `to_string_pretty`
/// с `preserve_order` совпадает побайтово (как в ext_utils::write_manifest).
fn dumps_indent2(v: &Value) -> String {
    serde_json::to_string_pretty(v).expect("Value сериализуется")
}

/// Python `str(value)` для строковых значений манифеста: строки как есть
/// (нестроковые в реальных манифестах не встречаются — берём как JSON-строку
/// либо Display).
fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Python-истинность Option<&Value> (для `meta.get("id")` и т.п.).
fn is_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Порт `shlex.quote`: пусто → `''`; безопасные символы — как есть; иначе
/// одинарные кавычки с заменой `'` → `'"'"'`.
pub fn shlex_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let safe = s.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '-' | '_')
    });
    if safe {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
}

/// Минимальный разбор `.desktop`/INI для `configparser.get(section, key)`
/// (strict=False, ключи регистронезависимы — optionxform=str.lower).
fn ini_get(text: &str, section: &str, key: &str) -> Option<String> {
    let want_key = key.to_lowercase();
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_section = &line[1..line.len() - 1] == section;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = line.split_once('=').or_else(|| line.split_once(':')) {
            if k.trim().to_lowercase() == want_key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}
