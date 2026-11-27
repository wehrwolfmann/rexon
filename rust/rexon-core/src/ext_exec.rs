//! Исполнитель менеджера расширений на живой файловой системе.
//!
//! ЗАЧЕМ. Планировщик расширений давно на Rust — `ext_manager.rs` умеет всё
//! РЕШАТЬ: проверить .rxn, вывести id, собрать план установки/удаления
//! оформления шагами [`ExtStep`]. Но ДЕЛАТЬ он не умеет ничего: вход у него —
//! снимок `FsSnap`, настоящей файловой системы в нём нет вовсе. Исполнителем
//! этого плана до сих пор был Python (`_exec_plan` в
//! `rexon/extensions/manager.py`), и из-за этого объект побочных действий
//! вкладки «Расширения» приходилось держать на PySide6.
//!
//! Здесь дописан исполнитель: те же побочные вызовы, что делал Python
//! (`shutil`/`os`/`subprocess`/`pkexec`), но на Rust. Разделение прежнее и
//! оно принципиально — **планировщик решает, исполнитель делает**:
//!   * что копировать, что писать, что запускать — [`ext_manager`];
//!   * снимок ФС для планировщика — [`plan_snapshot`] (порт `_plan_snapshot`);
//!   * прогон шагов — [`exec_plan`] (порт `_exec_plan`);
//!   * конвейеры целиком — [`install_from_file`], [`uninstall`], [`toggle`].
//!
//! ⭐ ИМПОРТЁРЫ ЧУЖИХ ФОРМАТОВ ПОДКЛЮЧЕНЫ (07.09.2026). Прежняя граница —
//! «понимаю только `.rxn`» — снята: живой исполнитель трейта `ImpIo` переехал
//! из моста `rexon-py` в ядро ([`crate::imp_live`]), и [`install_from_file`]
//! снова делает то же, что делал Python: `.themepack`/`.deskthemepack` и
//! Windows-темы (`.theme`/`.msstyles`/`.wmv`/`.dream`/`.gadget`) сначала
//! конвертируются в `.rxn` во временном каталоге, а ставится уже результат.
//! Иначе вкладка обманывала бы своим же фильтром окна выбора файла
//! (`qt_extensions::FILE_DIALOG_FILTER` предлагает Windows-темы прямым
//! текстом).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::constants;
use crate::ext_manager::{self, ExtEnv, ExtInfo, ExtStep, FsSnap, TmpAlloc};
use crate::ext_utils;
use crate::extension::Extension;
use crate::i18n::{tr, trf};
use crate::qt_extensions;

// ============================================================================
// Мелочи
// ============================================================================

/// `$HOME` — тот же, что подставляет Python (`os.path.expanduser("~")`).
fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Ошибка исполнителя — уже готовая строка для человека: её показывает
/// строка состояния вкладки, как показывала `str(e)` из Python.
pub type ExecResult<T> = Result<T, String>;

fn err<T>(text: impl Into<String>) -> ExecResult<T> {
    Err(text.into())
}

// ============================================================================
// Снимки файловой системы для планировщика
// ============================================================================

/// Собиратель снимка: те же четыре слоя, что у Python (`_plan_snapshot`).
#[derive(Default)]
struct SnapBuilder {
    dirs: Map<String, Value>,
    files: Map<String, Value>,
    links: Vec<Value>,
    pre_dirs: Vec<Value>,
    pre_files: Vec<Value>,
    /// Файлы, которые общий журнал записал за пакетом (`FsSnap::owns_file`).
    owned_files: Vec<Value>,
    /// Пары `[файл пакета, файл на месте]`, равные байт в байт
    /// (`FsSnap::same_bytes`).
    same_files: Vec<Value>,
}

impl SnapBuilder {
    fn into_value(self) -> Value {
        let mut out = Map::new();
        out.insert("dirs".into(), Value::Object(self.dirs));
        out.insert("files".into(), Value::Object(self.files));
        out.insert("links".into(), Value::Array(self.links));
        out.insert("pre_dirs".into(), Value::Array(self.pre_dirs));
        out.insert("pre_files".into(), Value::Array(self.pre_files));
        out.insert("owned_files".into(), Value::Array(self.owned_files));
        out.insert("same_files".into(), Value::Array(self.same_files));
        Value::Object(out)
    }

    /// Одиночные файлы пакета, которые уже лежат в каталогах назначения
    /// под тем же именем: какие из них — байт в байт файлы пакета
    /// (правило владельца для файлов, `ext_manager::put_owned_file`).
    /// Сравниваются только обычные файлы (не ссылки) одного размера.
    fn same_files_in(&mut self, ext_dir: &Path, bases: &[PathBuf]) {
        let prefix = format!("{}/", s(ext_dir));
        let mut by_name: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for path in self.files.keys() {
            if let Some(rel) = path.strip_prefix(&prefix) {
                let name = rel.rsplit('/').next().unwrap_or(rel);
                by_name.entry(name.to_string()).or_default().push(path.clone());
            }
        }
        for base in bases {
            for name in Self::names_of(base) {
                let Some(srcs) = by_name.get(&name) else { continue };
                let dest = base.join(&name);
                let Ok(md) = std::fs::symlink_metadata(&dest) else { continue };
                if !md.file_type().is_file() {
                    continue;
                }
                let mut dest_bytes: Option<Vec<u8>> = None;
                for src in srcs {
                    if std::fs::metadata(src).map(|m| m.len()).ok() != Some(md.len()) {
                        continue;
                    }
                    if dest_bytes.is_none() {
                        dest_bytes = std::fs::read(&dest).ok();
                    }
                    if dest_bytes.is_some() && std::fs::read(src).ok() == dest_bytes {
                        self.same_files.push(Value::Array(vec![Value::String(src.clone()), Value::String(s(&dest))]));
                    }
                }
            }
        }
    }

    fn file(&mut self, path: &Path, size: u64, text: Option<String>) {
        let mut m = Map::new();
        m.insert("size".into(), Value::from(size));
        m.insert(
            "text".into(),
            match text {
                Some(t) => Value::String(t),
                None => Value::Null,
            },
        );
        self.files.insert(s(path), Value::Object(m));
    }

    /// Имена каталога в порядке чтения. `read_dir` отдаёт их в порядке ФС, а
    /// `os.scandir` Python — в своём; порядок влияет на выбор «первого
    /// подходящего» файла в плане, поэтому имена сортируются — так витрина и
    /// план ведут себя одинаково от машины к машине (тот же довод, что у
    /// [`qt_extensions::installed_in`]).
    fn names_of(dir: &Path) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .collect();
        names.sort();
        names
    }

    /// Глубокий обход (порт `deep`): дерево целиком, с текстами
    /// `metadata.json`/`metadata.desktop` — по ним планировщик достаёт
    /// plugin-id плазмоида.
    ///
    /// `observed_only = true` — записи идут только в слой наблюдений: это то,
    /// что оригинал видел ПОСЛЕ собственных шагов, и решениями «существовало
    /// ли до нас» становиться не должно.
    fn deep(&mut self, root: &Path, observed_only: bool) {
        let key = s(root);
        if self.dirs.contains_key(&key) && observed_only {
            return;
        }
        let names = Self::names_of(root);
        self.dirs.insert(
            key,
            Value::Array(names.iter().map(|n| Value::String(n.clone())).collect()),
        );
        if !observed_only {
            self.pre_dirs.push(Value::String(s(root)));
        }
        for name in names {
            let path = root.join(&name);
            let link = std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if link {
                self.links.push(Value::String(s(&path)));
            }
            let is_dir = path.is_dir();
            if is_dir {
                if link {
                    // как os.walk(followlinks=False): в классификации каталог
                    // есть, внутрь не идём
                    self.dirs.entry(s(&path)).or_insert_with(|| Value::Array(Vec::new()));
                    if !observed_only {
                        self.pre_dirs.push(Value::String(s(&path)));
                    }
                } else {
                    self.deep(&path, observed_only);
                }
                continue;
            }
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let text = if name == "metadata.json" || name == "metadata.desktop" {
                std::fs::read_to_string(&path).ok()
            } else {
                None
            };
            self.file(&path, size, text);
            if !observed_only {
                self.pre_files.push(Value::String(s(&path)));
            }
        }
    }

    /// Файл с текстом (небольшие файлы настроек: LightDM).
    fn text_file(&mut self, path: &Path) {
        if self.files.contains_key(&s(path)) || !path.is_file() {
            return;
        }
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        self.file(path, size, std::fs::read_to_string(path).ok());
        self.pre_files.push(Value::String(s(path)));
    }

    /// Каталог файлов настроек с текстами — только имена с `suffix`
    /// (`.conf`, `.desktop`); ссылки читаются по цели (у Debian описание
    /// приветствия — ссылка на настоящее).
    fn texts_of(&mut self, dir: &Path, suffix: &str) {
        if self.dirs.contains_key(&s(dir)) || !dir.is_dir() {
            return;
        }
        let names: Vec<String> = Self::names_of(dir).into_iter().filter(|n| n.ends_with(suffix)).collect();
        self.dirs.insert(s(dir), Value::Array(names.iter().map(|n| Value::String(n.clone())).collect()));
        self.pre_dirs.push(Value::String(s(dir)));
        for name in names {
            self.text_file(&dir.join(name));
        }
    }

    /// Мелкий обход целевого каталога (порт `shallow`): присутствие,
    /// классификация и симлинки детей — без рекурсии. Планировщику этого
    /// хватает для решений `exists`/`isdir`, а обходить `~/.local/share/icons`
    /// целиком значило бы читать десятки тысяч файлов на каждую установку.
    fn shallow(&mut self, base: &Path) {
        if self.dirs.contains_key(&s(base)) || !base.is_dir() {
            return;
        }
        let names = Self::names_of(base);
        self.dirs.insert(
            s(base),
            Value::Array(names.iter().map(|n| Value::String(n.clone())).collect()),
        );
        self.pre_dirs.push(Value::String(s(base)));
        for name in names {
            let path = base.join(&name);
            if std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
            {
                self.links.push(Value::String(s(&path)));
            }
            if path.is_dir() {
                self.dirs.entry(s(&path)).or_insert_with(|| Value::Array(Vec::new()));
                self.pre_dirs.push(Value::String(s(&path)));
            } else if path.is_file() {
                self.pre_files.push(Value::String(s(&path)));
            }
        }
    }

    /// Дерево `src_root`, перекорнёванное в `dest_root`, — в наблюдения
    /// (порт `project`). Так планировщик видит содержимое СВОЕГО же
    /// копирования: conky ищет конфиг уже в целевом каталоге.
    fn project(&mut self, src_root: &Path, dest_root: &Path) {
        let names = Self::names_of(src_root);
        self.dirs.insert(
            s(dest_root),
            Value::Array(names.iter().map(|n| Value::String(n.clone())).collect()),
        );
        for name in names {
            let src = src_root.join(&name);
            let dest = dest_root.join(&name);
            if src.is_dir() {
                if std::fs::symlink_metadata(&src)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    self.dirs.entry(s(&dest)).or_insert_with(|| Value::Array(Vec::new()));
                } else {
                    self.project(&src, &dest);
                }
                continue;
            }
            let size = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
            self.file(&dest, size, None);
        }
    }
}

/// Снимок дерева для валидационного конвейера `.rxn` (порт `_snap_tree`).
///
/// Распакованный архив планировщик читает слоем НАБЛЮДЕНИЙ (`obs_is_file`) —
/// он появился в ходе прогона, а не до него. Поэтому pre-слои остаются
/// пустыми: `FsSnap::from_json` без них считал бы всё пре-состоянием.
pub fn snap_tree(root: &Path) -> Value {
    let mut b = SnapBuilder::default();
    b.deep(root, true);
    b.into_value()
}

/// Каталоги, куда Rexon ставит оформление, — те же, что видит планировщик.
///
/// ⚠️ Берутся ДВУМЯ наборами намеренно. Планировщик строит путь как
/// `{HOME}/{суффикс}` (`ExtEnv::install_path`), а продуктовая таблица —
/// XDG-осведомлённо (`constants::install_path`). На машине с заданным
/// `XDG_DATA_HOME` это разные каталоги, и снимок обязан покрыть оба: иначе
/// решение «уже существует» принималось бы по пустому месту.
fn target_dirs(home: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for t in constants::valid_types() {
        if let Some(suffix) = constants::install_path_suffix(t) {
            out.push(PathBuf::from(format!("{home}/{suffix}")));
        }
        if let Some(path) = constants::install_path(t) {
            out.push(path);
        }
    }
    out.push(PathBuf::from(format!("{home}/.themes")));
    out.push(PathBuf::from(crate::utils::expand_user(
        "~/.config/autostart",
        home,
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
    )));
    out.sort();
    out.dedup();
    out
}

/// Архив `.plasmoid`, который выберет план в каталоге `dir`: первый по
/// порядку имён, если раньше него не встретился каталог с описанием
/// плазмоида (тогда ставится каталог, архив не понадобится).
fn first_plasmoid_archive(dir: &Path) -> Option<PathBuf> {
    for name in SnapBuilder::names_of(dir) {
        let full = dir.join(&name);
        if name.ends_with(".plasmoid") {
            return Some(full);
        }
        if full.is_dir() && (full.join("metadata.json").is_file() || full.join("metadata.desktop").is_file()) {
            return None;
        }
    }
    None
}

/// Каталоги, где план ищет плазмоид пакета: `data/` у типа плазмоида,
/// подкаталог части `theme-plasmoid` у комплекта.
fn plasmoid_dirs(ext: &Extension) -> Vec<PathBuf> {
    match ext.ext_type() {
        "theme-plasmoid" => vec![ext.dir.join("data")],
        "theme-bundle" => ext
            .manifest
            .get("bundle")
            .and_then(Value::as_object)
            .and_then(|m| m.get("theme-plasmoid"))
            .and_then(Value::as_str)
            .map(|rel| vec![ext.dir.join(rel)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Снимок ФС для генерации плана установки/удаления (порт `_plan_snapshot`).
pub fn plan_snapshot(ext: &Extension, tmp_base: &Path, install: bool) -> Value {
    let home = home();
    let mut b = SnapBuilder::default();
    b.deep(&ext.dir, false);
    for base in target_dirs(&home) {
        b.shallow(&base);
    }
    // Существующий каталог темы (основная копия, `~/.themes/<тема>` у
    // рамок, подпапки комплекта и Conky) план трогает только по метке
    // владельца — её текст нужен и установке (обновить своё, не тронуть
    // чужое), и удалению. Имя каталога у части типов берётся из метаданных
    // (uuid, KPlugin.Id), поэтому метка читается у каждого каталога базы.
    for base in target_dirs(&home) {
        for name in SnapBuilder::names_of(&base) {
            b.text_file(&base.join(&name).join(ext_manager::OWNER_MARK));
        }
    }
    // Одиночные файлы (цветовые схемы, действия Nemo, шрифты комплекта)
    // метки не несут — свой ли файл на месте, план узнаёт по общему журналу
    // (записан за этим пакетом) либо по совпадению байт в байт.
    let journal = crate::install_journal::view_at(&rexon_config_dir());
    let key = crate::install_journal::key_of(crate::install_journal::Source::Rxn, ext.ext_id());
    if let Some(entry) = journal.items.get(&key) {
        for p in &entry.placed {
            if matches!(p.what, Some(crate::install_journal::What::File) | None) {
                b.owned_files.push(Value::String(p.path.clone()));
            }
        }
    }
    b.same_files_in(&ext.dir, &target_dirs(&home));
    // .plasmoid: id плазмоида — внутри архива. Снимок распаковывает его
    // заранее (`ext_manager::PLASMOID_PROBE_DIR`), чтобы план решил «на месте
    // чужой плазмоид с тем же id — не трогать» ДО запуска kpackagetool, а
    // удаление снимало каталог настоящего id.
    for dir in plasmoid_dirs(ext) {
        if let Some(file) = first_plasmoid_archive(&dir) {
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let probe = tmp_base.join(ext_manager::PLASMOID_PROBE_DIR).join(name);
            if std::fs::create_dir_all(&probe).is_ok() && ext_utils::safe_zip_extract(&file, &probe).is_ok() {
                b.deep(&probe, true);
            }
        }
    }
    if !install {
        // Удаление темы экрана входа или заставки: план смотрит, лежит ли
        // она в системном каталоге (снимает её помощник от root).
        for base in ["/usr/share/sddm/themes", crate::system_plan::PLYMOUTH_THEMES_DIR] {
            b.shallow(Path::new(base));
        }
    }

    if install {
        // Экран входа LightDM: какое приветствие стоит, план решает по
        // файлам настроек LightDM и описаниям приветствий — тексты их нужны.
        for dir in ext_manager::LIGHTDM_CONF_DIRS {
            b.texts_of(Path::new(dir), ".conf");
        }
        b.texts_of(Path::new(ext_manager::XGREETERS_DIR), ".desktop");
        b.text_file(Path::new(ext_manager::LIGHTDM_CONF));

        // conky: планировщик ищет конфиг в целевом каталоге ПОСЛЕ копирования
        // — проецируем источник (dest после rmtree+copytree равен ему 1-в-1).
        let conky_dest = PathBuf::from(crate::utils::expand_user(
            "~/.config/conky",
            &home,
            std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        ))
        .join(ext.ext_id());
        match ext.ext_type() {
            "theme-conky" => b.project(&ext.dir.join("data"), &conky_dest),
            "theme-bundle" => {
                if let Some(bundle) = ext.manifest.get("bundle").and_then(Value::as_object) {
                    for (sub_type, sub_rel) in bundle {
                        if sub_type == "theme-conky" {
                            if let Some(rel) = sub_rel.as_str() {
                                let sub_dir = ext.dir.join(rel);
                                if sub_dir.is_dir() {
                                    b.project(&sub_dir, &conky_dest);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // .plasmoid: ручная ветка планировщика (kpackagetool нет либо не
        // сработал) читает дерево РАСПАКОВАННОГО архива. Распаковываем заранее
        // в тот путь, который выдаст первый mkdtemp плана, — иначе ветка
        // увидела бы пустоту.
        if ext.ext_type() == "theme-plasmoid" {
            if let Some(file) = first_plasmoid_archive(&ext.dir.join("data")) {
                let tmp0 = tmp_base.join("rexon_plasmoid_0");
                if std::fs::create_dir_all(&tmp0).is_ok()
                    && ext_utils::safe_zip_extract(&file, &tmp0).is_ok()
                {
                    b.deep(&tmp0, true);
                }
            }
        }
    }

    b.into_value()
}

// ============================================================================
// План: планировщик решает
// ============================================================================

/// Готовый план вместе с тем, что уже успело выполниться при его сборке.
pub struct Plan {
    pub steps: Vec<ExtStep>,
    /// Пробы, ИСПОЛНЕННЫЕ колбэком `run_ok` прямо во время генерации, — их
    /// одноимённые шаги исполнитель пропустит по очереди.
    pub pre_ran: Vec<Vec<String>>,
    /// Рабочий каталог плана под `/tmp`; сносится в конце прогона.
    pub tmp_base: PathBuf,
    /// План установки (`true`) или удаления: от этого зависит, что делать с
    /// системной частью на SteamOS — см. [`exec_plan`].
    pub install: bool,
}

/// `tempfile.mkdtemp(prefix=…, dir=…)` без внешних крейтов: имя из времени и
/// счётчика, создание — `create_dir` (падает, если имя занято, — значит
/// каталог наш и только наш).
pub(crate) fn mkdtemp(dir: &Path, prefix: &str) -> ExecResult<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for attempt in 0..64u64 {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("{prefix}{}_{}_{}", std::process::id(), stamp, n + attempt));
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return err(trf("couldn't create a temporary folder: %1", &[&e])),
        }
    }
    err(tr("couldn't create a temporary folder: all names are taken"))
}

/// Собрать план установки либо удаления (порт `_plan_steps`).
///
/// ⚠️ Колбэк `run_ok` ЗАПУСКАЕТ пробы уже здесь, в том же порядке, в каком их
/// звал оригинал (`kpackagetool -u`/`-i`, `systemctl is-active`), — иначе
/// планировщик не узнает, какую ветку выбрать. Их шаги исполнитель потом
/// пропустит по списку `pre_ran`.
pub fn make_plan(ext: &Extension, install: bool) -> ExecResult<Plan> {
    // dir="/tmp" обязателен: staged-пути плана обязаны пройти realpath-проверку
    // stage-helper'а (как `mkdtemp(dir="/tmp")` у оригинала).
    let tmp_base = mkdtemp(Path::new("/tmp"), "rexon_plan_")?;
    let built = build_plan(ext, install, &tmp_base);
    match built {
        Ok((steps, pre_ran)) => Ok(Plan { steps, pre_ran, tmp_base, install }),
        Err(e) => {
            // Временный каталог за собой убираем, а ошибку отдаём наверх:
            // план либо есть, либо установку начинать нельзя.
            let _ = std::fs::remove_dir_all(&tmp_base);
            Err(e)
        }
    }
}

fn build_plan(
    ext: &Extension,
    install: bool,
    tmp_base: &Path,
) -> ExecResult<(Vec<ExtStep>, Vec<Vec<String>>)> {
    let home = home();
    let snap_value = plan_snapshot(ext, tmp_base, install);
    let fs = FsSnap::from_json(&snap_value);
    let tmp = TmpAlloc::new(&s(tmp_base));

    let pre_ran = std::cell::RefCell::new(Vec::<Vec<String>>::new());
    let which = |name: &str| crate::utils::which(name, std::env::var("PATH").ok().as_deref());
    let run_ok = |argv: &[String]| -> bool {
        let argv = argv.to_vec();
        if argv.is_empty() {
            return false;
        }
        // kpackagetool -i: оригинал запускает -u непосредственно перед -i —
        // исполняем пару в исходном порядке.
        if argv.len() == 3 && argv[1] == "-i" {
            let u_argv = vec![argv[0].clone(), "-u".to_string(), argv[2].clone()];
            pre_ran.borrow_mut().push(u_argv.clone());
            let _ = run_capture(&u_argv, 30);
        }
        let timeout = if Path::new(&argv[0])
            .file_name()
            .map(|n| n == "systemctl")
            .unwrap_or(false)
        {
            5
        } else {
            30
        };
        pre_ran.borrow_mut().push(argv.clone());
        run_capture(&argv, timeout) == Some(0)
    };

    // Помощник ищется в обоих законных каталогах (пакет — `/usr/lib`,
    // ручная установка — `/usr/local/lib`), как и остальные помощники.
    let system_helper_path = crate::installer::system_helper_path();
    let system_helper_exists = crate::system_plan::is_exec_file(system_helper_path);
    let family = if crate::system::DistroDetector::detect_live().family_live()
        == crate::system::PkgFamily::Debian
    {
        "debian"
    } else {
        "arch"
    };
    let env = ExtEnv {
        home: &home,
        fs: &fs,
        which: &which,
        run_ok: &run_ok,
        xdg_current_desktop: &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        xdg_session_type: &std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
        wayland_display: std::env::var_os("WAYLAND_DISPLAY").is_some(),
        stage_helper_available: helper_available(
            crate::system_plan::stage_helper_path(),
            crate::system_plan::STAGE_POLICY_PATH,
        ),
        system_helper_path,
        system_helper_exists,
        helper_family: family,
        tmp: &tmp,
    };
    let info = ext_manager::ExtInfo { dir: &s(&ext.dir), manifest: &ext.manifest };
    let steps = if install {
        ext_manager::safe_install_plan(&env, &info)
    } else {
        ext_manager::safe_uninstall_plan(&env, &info)
    };
    Ok((steps, pre_ran.into_inner()))
}

/// Задеплоен ли helper (порт `_helper_available`): бинарь и его polkit-правило
/// оба на месте.
fn helper_available(helper_path: &str, policy_path: &str) -> bool {
    Path::new(helper_path).exists() && Path::new(policy_path).exists()
}

// ============================================================================
// Исполнитель: исполнитель делает
// ============================================================================

/// `subprocess.run(argv, capture_output=True, timeout=…)` — код возврата либо
/// `None` (не запустилось / таймаут / убито сигналом).
fn run_capture(argv: &[String], timeout_secs: u64) -> Option<i32> {
    if argv.is_empty() {
        return None;
    }
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    // Таймаут у Python убивает процесс и поднимает исключение;
                    // здесь тот же итог одним значением: «успеха не было».
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
}

// Копирование — общее с прочими путями установки (`install_plan`).
use crate::install_plan::copy_file;

// ============================================================================
// Системная часть плана (всё, что идёт через pkexec)
// ============================================================================
//
// ⭐ Починка 28.09.2026 — молчаливый отказ. Прежде исполнитель ЗВАЛ pkexec и
// ВЫБРАСЫВАЛ ответ: и у скрипта-обёртки, и у помощника Rexon. Отказ в
// пароле, «файловая система только для чтения» (SteamOS), любая ошибка
// записи в `/usr` — человек нажимал «Установить», видел «установлено», а
// тема экрана входа или загрузки так и не появлялась. Теперь любой
// ненулевой ответ становится ошибкой шага с текстом для человека, и
// конвейер установки откатывается так же, как при любой другой ошибке.

/// Чем кончился привилегированный шаг.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivOutcome {
    /// Код 0.
    Done,
    /// pkexec вернул код возврата ≠ 0; `stderr` — что сказала команда.
    Failed { code: i32, stderr: String },
    /// Не уложились во время — процесс остановлен.
    TimedOut,
    /// pkexec не запустился вовсе (его нет в системе).
    NotStarted,
}

// Какие шаги системные и что они пишут (SteamOS, проверка темы до пароля) —
// решает общий исполнитель (`install_plan`); здесь — только сам запуск.

/// Текст для человека по исходу привилегированного шага; `None` — успех.
///
/// Отказ в пароле узнаётся так же, как во всех прочих системных действиях
/// Rexon ([`crate::system_plan::pkexec_auth_denied`]). У прочих отказов в
/// текст идёт последняя строка ответа команды — там и «только для чтения»,
/// и «нет места», и что угодно ещё, чего мы заранее не знаем.
pub fn privileged_failure_text(outcome: &PrivOutcome) -> Option<String> {
    match outcome {
        PrivOutcome::Done => None,
        PrivOutcome::NotStarted => Some(crate::system_plan::pkexec_missing_message()),
        PrivOutcome::TimedOut => Some(tr(
            "Rexon could not change the system part of the theme: it took too long and was stopped.",
        )),
        PrivOutcome::Failed { code, stderr } => {
            if crate::system_plan::pkexec_auth_denied(*code, stderr) {
                return Some(tr(
                    "The administrator password was not confirmed, so Rexon could not change the \
                     system part of the theme.",
                ));
            }
            let detail = stderr
                .lines()
                .map(str::trim)
                .rfind(|l| !l.is_empty())
                .map(|l| l.chars().take(300).collect::<String>());
            Some(match detail {
                // TRANSLATORS: %1 is the last line printed by the failed system command, as is (often English).
                Some(line) => trf("Rexon could not change the system part of the theme: %1", &[&line]),
                None => trf(
                    "Rexon could not change the system part of the theme (error code %1).",
                    &[code],
                ),
            })
        }
    }
}

/// Запустить привилегированную команду, сохранив её ответ об ошибке.
///
/// Поток ошибок уходит в файл, а не в канал: канал переполнился бы на
/// многословной пересборке загрузочного образа, и процесс встал бы.
fn run_privileged(argv: &[String], timeout_secs: u64, work: &Path) -> PrivOutcome {
    crate::log_info!("privileged", "запуск: {}", crate::applog::redact_argv(argv));
    let started = Instant::now();
    let outcome = run_privileged_inner(argv, timeout_secs, work);
    let what = match &outcome {
        PrivOutcome::Done => "готово".to_string(),
        PrivOutcome::NotStarted => "не запустилось".to_string(),
        PrivOutcome::TimedOut => format!("не уложилось в {timeout_secs} с"),
        PrivOutcome::Failed { code, stderr } => format!(
            "код {code}: {}",
            stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("")
        ),
    };
    crate::log_info!(
        "privileged",
        "{}: {what} за {:.1} с",
        argv.first().map(String::as_str).unwrap_or(""),
        started.elapsed().as_secs_f64()
    );
    outcome
}

/// Прежний путь системного шага — общим исполнителем команд
/// ([`crate::utils::run`]: читатели каналов в своих потоках, многословная
/// пересборка образа ядра процесс не подвесит). Своего `Command::new` для
/// `pkexec` здесь нет (сторож `no_raw_pkexec_spawn_outside_privilege`).
fn run_privileged_inner(argv: &[String], timeout_secs: u64, _work: &Path) -> PrivOutcome {
    let report = crate::utils::run(
        &crate::utils::RunSpec {
            argv: argv.to_vec(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(timeout_secs),
            ..crate::utils::RunSpec::default()
        },
        None,
    );
    use crate::utils::RunOutcome as Ro;
    match report.outcome {
        Ro::Ok => PrivOutcome::Done,
        Ro::Timeout => PrivOutcome::TimedOut,
        Ro::Error => PrivOutcome::NotStarted,
        // Убит сигналом — кода нет; для человека это такой же отказ.
        Ro::Failed => PrivOutcome::Failed {
            code: report.returncode.unwrap_or(-1),
            stderr: crate::utils::decode_text(&report.stderr),
        },
    }
}

/// Операция сеанса прав для системного шага плана `.rxn` и её файлы
/// (каталоги раскладки). argv строит ядро (`ext_manager`: помощники
/// раскладки и системных тем); операция принимается, ТОЛЬКО если её путь
/// «как сейчас» ([`PrivOp::pkexec_argv`]) даёт ровно этот argv — второй
/// правды о флагах и путях помощников нет. `None` — шаг идёт прежним путём
/// (скрипт-обёртка, иной помощник).
pub(crate) fn step_priv_op(step: &ExtStep) -> Option<(crate::privilege::op::PrivOp, Vec<String>)> {
    use crate::privilege::op::{Helpers, LocalFile, PrivOp, SystemThemeKind, SystemThemeOp, ThemeFamily};
    let ExtStep::Run { argv, .. } = step else { return None };
    if argv.first().map(String::as_str) != Some("pkexec") {
        return None;
    }
    let helper = argv.get(1)?.as_str();
    let rest = crate::install_plan::helper_args(argv);
    let file = |label: &str| LocalFile { slot: 0, label: label.to_string() };
    let (op, files) = if helper == crate::system_plan::stage_helper_path() {
        let [category, name, staged] = rest else { return None };
        (PrivOp::Stage { category: category.clone(), name: name.clone(), source: file(staged) }, vec![staged.clone()])
    } else if helper == crate::installer::system_helper_path() {
        let [word, theme, src, fam] = rest else { return None };
        let family = match fam.as_str() {
            "arch" => ThemeFamily::Arch,
            "debian" => ThemeFamily::Debian,
            _ => return None,
        };
        let kind_of = |k: &str| match k {
            "sddm" => Some(SystemThemeKind::Sddm),
            "plymouth" => Some(SystemThemeKind::Plymouth),
            _ => None,
        };
        let theme = theme.clone();
        if let Some(k) = word.strip_prefix("apply-").and_then(kind_of) {
            (PrivOp::SystemTheme(SystemThemeOp::Apply { kind: k, theme, family }), Vec::new())
        } else if let Some(k) = word.strip_prefix("remove-").and_then(kind_of) {
            (PrivOp::SystemTheme(SystemThemeOp::Remove { kind: k, theme, family }), Vec::new())
        } else {
            let kind = kind_of(word)?;
            (PrivOp::SystemTheme(SystemThemeOp::Install { kind, theme, source: file(src), family }), vec![src.clone()])
        }
    } else {
        return None;
    };
    let helpers = Helpers { stage: true, system: true, ..Helpers::default() };
    (op.pkexec_argv(&helpers).as_ref() == Some(argv)).then_some((op, files))
}

/// Системная команда помощника тем и раскладок (argv ядра) через сеанс
/// прав окна — для исполнителей, у которых нет плана `.rxn`, а есть argv
/// (магазин оформления: установка, «Применить», «Удалить» системной темы).
/// `None` — команда не из этих или сеанса нет: исполнитель запускает её
/// прежним путём. Ответ помощника — в виде отчёта обычного запуска.
pub(crate) fn run_system_argv_session(argv: &[String], timeout_secs: Option<u64>) -> Option<crate::utils::RunReport> {
    let (op, files) = step_priv_op(&ExtStep::Run { argv: argv.to_vec(), timeout_secs })?;
    let out = crate::privilege::run::run_live(&op, &files, None, &mut |_| {})?;
    crate::log_info!("privileged", "сеанс прав: код {:?}", out.returncode);
    Some(crate::utils::RunReport {
        outcome: out.outcome,
        returncode: out.returncode,
        stdout: out.stdout.into_bytes(),
        stderr: out.stderr.into_bytes(),
        timeout_secs: timeout_secs.unwrap_or(0),
        error: String::new(),
    })
}

/// Итог сеанса прав в виде итога системного шага.
fn priv_outcome_of(out: &crate::system_exec::RunOut) -> PrivOutcome {
    use crate::utils::RunOutcome as Ro;
    match (out.outcome, out.returncode) {
        (_, Some(0)) => PrivOutcome::Done,
        (Ro::Timeout, _) => PrivOutcome::TimedOut,
        (Ro::Error, _) => PrivOutcome::NotStarted,
        (_, code) => PrivOutcome::Failed {
            code: code.unwrap_or(-1),
            // Пояснение помощника — строками хода (`stdout`), отказы клиента — в `stderr`.
            stderr: if out.stderr.trim().is_empty() { out.stdout.clone() } else { out.stderr.clone() },
        },
    }
}

/// Живой исполнитель привилегированного шага: скрипт-обёртка —
/// `pkexec bash <файл>` (таймаут 60 с, как у оригинала), помощник — его argv.
fn run_privileged_step_live(step: &ExtStep) -> PrivOutcome {
    use std::os::unix::fs::PermissionsExt;
    // Сеанс прав окна (один пароль); его нет или шаг ему не по силам —
    // прежний путь. Отказ помощника прежним путём не повторяется (З5).
    if let Some((op, files)) = step_priv_op(step) {
        if let Some(out) = crate::privilege::run::run_live(&op, &files, None, &mut |_| {}) {
            let outcome = priv_outcome_of(&out);
            crate::log_info!("privileged", "сеанс прав: {outcome:?}");
            return outcome;
        }
    }
    let prefix = match step {
        ExtStep::PkexecScript { prefix, .. } => prefix.as_str(),
        _ => "rexon_priv",
    };
    let Ok(holder) = mkdtemp(&std::env::temp_dir(), &format!("rexon_{prefix}_")) else {
        return PrivOutcome::NotStarted;
    };
    let outcome = match step {
        ExtStep::PkexecScript { script, .. } => {
            let path = holder.join("script.sh");
            let written = (|| -> std::io::Result<()> {
                let mut f = std::fs::File::create(&path)?;
                f.write_all(script.as_bytes())?;
                f.flush()?;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            })();
            match written {
                Ok(()) => run_privileged(
                    &["pkexec".to_string(), "bash".to_string(), s(&path)],
                    60,
                    &holder,
                ),
                Err(e) => PrivOutcome::Failed { code: -1, stderr: e.to_string() },
            }
        }
        ExtStep::Run { argv, timeout_secs } => {
            run_privileged(argv, timeout_secs.unwrap_or(30), &holder)
        }
        _ => PrivOutcome::Done,
    };
    let _ = std::fs::remove_dir_all(&holder);
    outcome
}

/// Исполнить план ядра (порт `_exec_plan`): те же побочные вызовы, что делал
/// Python. Ошибка шага пробрасывается — семантика оригинала: сбой установки
/// приводит к откату в [`install_from_file`].
///
/// Системная часть (шаги через pkexec) на SteamOS не исполняется вовсе:
/// системный раздел там закрыт, а обновление стёрло бы записанное. Установка
/// с такой частью ОТКАЗЫВАЕТ целиком ещё до первого шага — с причиной для
/// человека; удаление системную часть пропускает (снести её там нечем, а
/// пользовательскую часть человек убрать вправе).
pub fn exec_plan(plan: Plan) -> ExecResult<()> {
    exec_plan_noted(plan).map(|_| ())
}

/// То же, с заметками для человека: что из плана сделано не было и почему
/// ([`ExtStep::Notice`], неудавшийся [`ExtStep::RunNoted`]).
///
/// План установки при сбое откатывается целиком (`install_plan::exec`).
pub fn exec_plan_noted(plan: Plan) -> ExecResult<Vec<String>> {
    let mut undo = if plan.install { crate::install_plan::Undo::new() } else { crate::install_plan::Undo::off() };
    let outcome = exec_plan_noted_undo(plan, &mut undo);
    undo.finish(outcome.is_ok());
    outcome
}

/// То же, но откат копится в `undo` вызывающего и не исполняется: так
/// установка `.rxn` снимает при сбое и план, и копию пакета, и то, что
/// сорвалось уже после плана (запись состояния).
fn exec_plan_noted_undo(plan: Plan, undo: &mut crate::install_plan::Undo) -> ExecResult<Vec<String>> {
    let Plan { steps, mut pre_ran, tmp_base, install } = plan;
    let ctx = ExecCtx {
        steamos: crate::distro_compat::running_on_steamos(),
        install,
        privileged: &run_privileged_step_live,
    };
    let mut notes = Vec::new();
    let outcome = exec_steps_noted(&steps, &mut pre_ran, &ctx, &mut notes, undo);
    let _ = std::fs::remove_dir_all(&tmp_base);
    outcome.map(|()| notes)
}

/// Внешнее для прогона плана: где мы и чем исполнять системные шаги.
struct ExecCtx<'a> {
    steamos: bool,
    install: bool,
    privileged: &'a dyn Fn(&ExtStep) -> PrivOutcome,
}

fn exec_steps_noted(
    steps: &[ExtStep],
    pre_ran: &mut Vec<Vec<String>>,
    ctx: &ExecCtx,
    notes: &mut Vec<String>,
    undo: &mut crate::install_plan::Undo,
) -> ExecResult<()> {
    let plan: Vec<crate::install_plan::Step> = steps.iter().cloned().map(Into::into).collect();
    let common = crate::install_plan::Ctx::live(ctx.install, ctx.steamos, Some(ctx.privileged), false);
    match crate::install_plan::exec_undoable(&plan, pre_ran, &common, undo) {
        Ok(report) => {
            notes.extend(report.notes);
            Ok(())
        }
        Err(f) => err(crate::install_plan::failure_text(&f)),
    }
}

// ============================================================================
// Скриптовые расширения и .desktop
// ============================================================================

/// Запустить `install.sh`/`uninstall.sh` расширения (порт `_run_script`):
/// `bash <скрипт>` из каталога расширения, таймаут 120 с.
fn run_script(ext: &Extension, name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let script = ext.dir.join(name);
    if !script.is_file() {
        return false;
    }
    let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755));
    let Ok(mut child) = Command::new("bash")
        .arg(&script)
        .current_dir(&ext.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return false,
        }
    }
}

/// Путь `.desktop`-файла расширения в меню пользователя.
fn desktop_entry_path(ext_id: &str) -> PathBuf {
    PathBuf::from(crate::utils::expand_user_xdg(
        "~/.local/share/applications",
        &home(),
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
    ))
    .join(format!("rexon-{ext_id}.desktop"))
}

/// Порт `install_desktop_entry`: копия `.desktop` в меню плюс дописанная
/// категория, если её в файле не было.
fn install_desktop_entry(ext: &Extension) -> bool {
    // Имя файла фиксировано (`Extension.desktop_file` — это `<каталог>/app.desktop`),
    // манифест его не задаёт.
    let src = ext.dir.join("app.desktop");
    if !src.is_file() {
        return false;
    }
    let dest = desktop_entry_path(ext.ext_id());
    let Some(parent) = dest.parent() else { return false };
    if std::fs::create_dir_all(parent).is_err() || copy_file(&src, &dest).is_err() {
        return false;
    }
    match std::fs::read_to_string(&dest) {
        Ok(content) if !content.contains("Categories=") => {
            let patched = format!("{}\nCategories=Utility;Rexon;\n", content.trim_end());
            std::fs::write(&dest, patched).is_ok()
        }
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Порт `remove_desktop_entry`.
fn remove_desktop_entry(ext_id: &str) {
    let dest = desktop_entry_path(ext_id);
    if dest.exists() {
        let _ = std::fs::remove_file(&dest);
    }
}

// ============================================================================
// Состояние вкл/выкл
// ============================================================================

/// Отметить расширение `ext_id` включённым/выключенным (`Some`) или забыть
/// его (`None` — снято). Пишется только общий журнал установки (шаг 5б
/// слияния установщиков); карта состояний `extensions_state.json` — его
/// зеркало тем же текстом, что писал прежний `_save_state`
/// (`install_journal::set_rxn_enabled_at`).
fn save_state_with(ext_id: &str, set: Option<bool>) -> ExecResult<()> {
    crate::install_journal::set_rxn_enabled_at(&rexon_config_dir(), ext_id, set)
        .map_err(|e| trf("couldn't save the state: %1", &[&e]))
}

/// Каталог конфигов Rexon, где лежат карта состояний и общий журнал.
fn rexon_config_dir() -> PathBuf {
    qt_extensions::state_file().parent().map(Path::to_path_buf).unwrap_or_default()
}

// ============================================================================
// Конвейеры вкладки
// ============================================================================

/// Итог установки: имя расширения плюс то, о чём человек обязан узнать.
pub struct Installed {
    /// ИМЯ установленного расширения — ровно то, что показывала строка
    /// состояния вкладки (`ext.name` из Python-воркера).
    pub name: String,
    /// Предупреждения о ПРОПУЩЕННЫХ необязательных шагах: установка прошла,
    /// но что-то сделано упрощённо, и молчать об этом нельзя.
    pub notes: Vec<String>,
}

/// Установить расширение из файла: `.rxn` напрямую, чужой формат — через
/// конвертацию.
pub fn install_from_file(rxn_path: &str) -> ExecResult<Installed> {
    let path = Path::new(rxn_path);
    if !path.is_file() {
        return err(trf("File not found: %1", &[&rxn_path]));
    }

    // Чужой формат — сначала конвертация в .rxn, потом обычная установка.
    // Порядок проверок и имена временных каталогов — как у оригинала
    // (`_install_from_file_unlocked`): сперва .themepack, затем остальные
    // форматы Windows.
    if let Some(converted) = import_foreign(rxn_path)? {
        let outcome = install_rxn(Path::new(&converted.rxn_path));
        let _ = std::fs::remove_dir_all(&converted.tmp_dir);
        return outcome.map(|(name, notes)| Installed { name, notes: [converted.notes, notes].concat() });
    }

    install_rxn(path).map(|(name, notes)| Installed { name, notes })
}

/// Установка готового `.rxn` (порт `_install_rxn`).
///
/// ⚠️ Весть мосту живёт ЗДЕСЬ, а не у вызывающих: через эту функцию идут и
/// `.rxn`, и сконвертированные темы Windows, и оба окна, которые ставят
/// расширения («Расширения» и импорт тем в «Настройках»). Оригинал слал
/// `extension_installed` из того же места (`_install_rxn`), а Rust-окно не
/// слало вовсе — внешнее приложение моста об установке не узнавало.
fn install_rxn(path: &Path) -> ExecResult<(String, Vec<String>)> {
    let tmp = mkdtemp(&std::env::temp_dir(), "rexon_rxn_")?;
    let mut outcome = install_unpacked(path, &tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    // Любой пакет мог положить значки или курсоры: заодно проверить общий
    // набор `hicolor` профиля — урезанный `index.theme` в нём прячет значки
    // всего стола ([`crate::icon_hygiene`]). Проверка — одно чтение файла.
    if let Ok((_, notes)) = &mut outcome {
        if let Some(line) = crate::icon_hygiene::heal_live() {
            notes.push(line);
        }
    }
    if let Ok((name, _)) = &outcome {
        let bridge = crate::rexon_bridge::RexonBridge::new_default();
        if let Err(e) = announce_installed(&bridge, name, std::time::SystemTime::now()) {
            // Мост вспомогательный: его отказ установку не отменяет.
            crate::log_warn!("bridge", "весть об установке «{name}» не отправлена — {e}");
        }
    }
    outcome
}

/// Сказать мосту, что расширение установлено (порт
/// `rexon_bridge.notify("notify_extension_installed", …)`).
///
/// Уборка старше 5 минут — как у `notify` оригинала: кто сорит событиями,
/// тот и подметает. Подметается и при сбое отправки: старые файлы от этого
/// нужнее не становятся. Удаление расширения вести не шлёт — у оригинала
/// такого события нет, и у слушателя на него нет реплик.
pub fn announce_installed(
    bridge: &crate::rexon_bridge::RexonBridge,
    name: &str,
    now: std::time::SystemTime,
) -> std::io::Result<()> {
    let since = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let iso = crate::engine_update::now_iso(since.as_secs() as i64);
    let epoch_ms = since.as_millis() as u64;
    let sent = bridge
        .ensure_dirs()
        .and_then(|_| bridge.notify_extension_installed(name, &iso, epoch_ms));
    bridge.cleanup_commands(300.0, epoch_ms as f64 / 1000.0);
    sent
}

/// Готовый `.rxn` из чужого формата, временный каталог и что пропущено.
struct Converted {
    rxn_path: String,
    tmp_dir: String,
    notes: Vec<String>,
}

/// Семейство пакетного менеджера этой машины — чтобы называть пакеты
/// ЕЁ именами, а не арчевыми.
fn pkg_family() -> String {
    crate::system::DistroDetector::detect_live()
        .family_live()
        .as_str()
        .to_string()
}

/// Конвертировать чужой формат в `.rxn`. `Ok(None)` — формат не чужой,
/// файл ставится как есть.
///
/// ⭐ ДВА МОЛЧАНИЯ ЗАКРЫТЫ ЗДЕСЬ.
///
/// Первое — конвертеры. Импорт держится на посторонних программах
/// (`cabextract`, `convert`, `7z`, `wrestool`…). Обязательной нет — отказ
/// по-русски с именем пакета ЭТОГО дистрибутива, ещё до первого побочного
/// действия. Необязательной нет — импорт идёт упрощённо, и это записывается
/// в предупреждения. Оригинал (`_require_tools` в `extensions/importers.py`)
/// такие предупреждения только клал в журнал: покупатель их не видел никогда,
/// и это тот же молчаливый отказ, только с записью в файл (правило #16 —
/// баг оригинала чиним в порте).
///
/// Второе — апскейл маленьких обоев (хвост импорта тем XP/Vista). Он
/// подключён живым исполнителем [`crate::imp_live::upscale_hook`] — тем
/// самым, что в процессе с Python подставлял мост. Если размер картинки
/// определить нечем, шаг пропускается — но опять же вслух.
fn import_foreign(src: &str) -> ExecResult<Option<Converted>> {
    use crate::importers as imp;

    let mut probe = crate::imp_live::LiveImpIo::new(None);
    let (prefix, themepack) = if imp::themepack_can_import(&mut probe, src) {
        ("rexon_tp_", true)
    } else if imp::windows_theme_can_import(&mut probe, src) {
        ("rexon_win_", false)
    } else {
        return Ok(None);
    };

    // Проверка конвертеров ДО первого побочного действия — как в оригинале.
    let family = pkg_family();
    let tools_kind = if themepack {
        "themepack"
    } else {
        imp::import_kind(src, "")
    };
    let mut notes = match imp::require_tools(tools_kind, &family, &|tool| {
        crate::imp_live::tool_available(tool)
    }) {
        Ok(warnings) => warnings,
        Err(refusal) => return err(refusal),
    };

    let tmp = mkdtemp(&std::env::temp_dir(), prefix)?;
    let tmp_s = s(&tmp);
    let skipped = crate::imp_live::new_notes();
    let mut io = crate::imp_live::LiveImpIo::new(Some(
        crate::imp_live::upscale_hook(skipped.clone(), &family),
    ));
    let outcome = if themepack {
        imp::themepack_import_to_rxn(&mut io, src, Some(&tmp_s))
    } else {
        imp::windows_theme_import_to_rxn(&mut io, src, Some(&tmp_s))
    };
    notes.extend(crate::imp_live::take_notes(&skipped));
    // Сбой РЕАЛЬНОЙ файловой операции важнее семантического исхода: в Python
    // на этом месте улетал бы OSError и установка прекращалась.
    if let Some(message) = io.io_err {
        let _ = std::fs::remove_dir_all(&tmp);
        return err(message);
    }
    match outcome {
        Ok(rxn_path) => Ok(Some(Converted { rxn_path, tmp_dir: tmp_s, notes })),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            // Текст ошибки — с именем пакета ЭТОЙ системы, а не арчевым.
            err(imp::friendly_error_text(&e.message, &family)
                .unwrap_or(e.message))
        }
    }
}

fn install_unpacked(rxn_path: &Path, tmp: &Path) -> ExecResult<(String, Vec<String>)> {
    if ext_utils::safe_zip_extract(rxn_path, tmp).is_err() {
        return err(tr("The file is damaged or is not an .rxn archive"));
    }

    let snap_value = snap_tree(tmp);
    let fs = FsSnap::from_json(&snap_value);
    let Some(src_dir) = ext_manager::find_manifest_root(&fs, &s(tmp)) else {
        return err(tr("manifest.json not found in the .rxn archive"));
    };
    let manifest_path = Path::new(&src_dir).join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| trf("manifest.json can't be read: %1", &[&e]))?;
    let manifest: Value = serde_json::from_str(&text)
        .map_err(|e| ext_manager::bad_manifest_json_error(&e.to_string()).message)?;

    let extensions_dir = qt_extensions::extensions_dir();
    let realpath = |p: &str| {
        std::fs::canonicalize(p)
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_else(|_| p.to_string())
    };
    let dest_exists = |p: &str| Path::new(p).exists();
    let check = ext_manager::install_rxn_check(
        &fs,
        &src_dir,
        &s(&extensions_dir),
        manifest,
        &realpath,
        &dest_exists,
    )
    .map_err(|e| e.message)?;

    // Планировщик мог переписать манифест (вывод id у v4, миграция типов) —
    // кладём его текст обратно ДО копирования, как делал оригинал.
    if let Some(rewritten) = &check.rewritten_manifest_text {
        std::fs::write(&manifest_path, rewritten)
            .map_err(|e| trf("couldn't write manifest.json: %1", &[&e]))?;
    }

    let dest_dir = extensions_dir.join(&check.ext_id);
    std::fs::create_dir_all(&extensions_dir)
        .map_err(|e| trf("couldn't create the extensions folder: %1", &[&e]))?;
    // Один откат на всю установку — копию пакета, план и запись состояния
    // (шаг 4 слияния установщиков): сбой в любом месте снимает положенное
    // этой установкой и возвращает замещённое, профиль — как до неё.
    let mut undo = crate::install_plan::Undo::new();
    if let Err(e) = crate::install_plan::copy_tree(Path::new(&src_dir), &dest_dir, &mut undo) {
        undo.finish(false);
        return err(trf("couldn't copy the extension: %1", &[&e]));
    }

    let state = qt_extensions::load_state();
    type Done = (String, Vec<String>, Extension, Option<Vec<crate::install_journal::Placed>>);
    let mut finish = || -> ExecResult<Done> {
        let ext = Extension::load_from_dir(&dest_dir, &state)
            .map_err(|e| trf("the extension manifest was not accepted: %1", &[&e]))?;
        let mut notes = Vec::new();
        // Что положит план — для общего журнала, до исполнения (у скриптовых
        // пакетов пути знает только их скрипт).
        let mut placed = None;
        if check.is_scripted {
            run_script(&ext, "install.sh");
            if ext.ext_type() == "desktop-tool" {
                install_desktop_entry(&ext);
            }
        } else {
            let plan = make_plan(&ext, true)?;
            placed = Some(crate::install_journal::placed_from_steps(&plan.steps, &|p| {
                std::fs::symlink_metadata(p).is_ok()
            }));
            notes = exec_plan_noted_undo(plan, &mut undo)?;
            // Набор виджетов из файла: положение виджетов — накладкой,
            // заметки «где набор» и «каких шрифтов нет».
            if let Some(set) = crate::conky_sets::set_of_manifest(&ext.manifest) {
                let ctx = crate::de_apply::Ctx::live();
                let state = std::env::var("XDG_STATE_HOME").ok().filter(|v| !v.is_empty());
                notes.extend(crate::conky_sets::after_set_install(
                    &crate::animations::SystemIo,
                    &ctx,
                    state.as_deref(),
                    &check.ext_id,
                    &set,
                    &crate::conky_sets::live_have_font,
                ));
            }
        }
        save_state_with(&check.ext_id, Some(true))?;
        let name = ext.manifest.get("name").and_then(Value::as_str).unwrap_or(&check.ext_id).to_string();
        Ok((name, notes, ext, placed))
    };
    let outcome = finish();
    // Откат: снять положенное этой установкой (копию пакета и файлы плана)
    // и вернуть замещённое. Общий журнал пишется только после успеха.
    undo.finish(outcome.is_ok());
    let (name, notes, ext, placed) = outcome?;
    record_in_journal(&ext, placed);
    Ok((name, notes))
}

/// Записать установку в общий журнал установки (после её успеха). Сбой
/// журнала установку не отменяет: «включено» пакета уже записано
/// ([`save_state_with`]), а без путей пакет снимается прежним путём, по плану.
fn record_in_journal(ext: &Extension, placed: Option<Vec<crate::install_journal::Placed>>) {
    use crate::install_journal as journal;
    let known = placed.is_some();
    let info = journal::LegacyExt { enabled: true, ..journal::legacy_ext(ext) };
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let at = crate::engine_update::now_iso(since.as_secs() as i64);
    let entry = journal::rxn_entry(&info, placed.unwrap_or_default(), known, &at);
    if let Err(e) = journal::record_rxn_at(&rexon_config_dir(), entry) {
        crate::log_warn!("install", "установка «{}» не записана в общий журнал — {e}", ext.ext_id());
    }
}

// ============================================================================
// Курсоры из Windows: перевод заново из оригиналов — только по исправлению
// ============================================================================
//
// Решение Wehrwolfmann 14.11.2026: при открытии раздела «Курсоры» установленные
// темы «из Windows», переведённые версией переводчика ниже текущей, в фоне
// переводятся заново из оригиналов, сохранённых в пакете, — но только если
// хотя бы одно исправление между их версией и текущей касается их
// ([`imp::CURSOR_CONVERTER_CHANGES`]). Вышло побайтно то же — файлы не
// заменяются, в manifest пишется только версия. Без оригиналов — пропуск без
// шума. Ошибки — в журнал; человеку — только «Курсоры X обновлены».

use crate::importers as imp;

/// Итог прохода по установленным темам курсоров «из Windows».
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CursorRefresh {
    /// Заменены файлы: (имя темы, каталог темы в `data/` — имя для применения).
    pub replaced: Vec<(String, String)>,
    /// Перевод вышел тем же — записана только версия (id пакетов).
    pub version_only: Vec<String>,
    /// Что не вышло (уже в журнале).
    pub errors: Vec<String>,
}

/// Одинаковы ли два дерева: те же имена, файлы байт в байт, ссылки с теми же
/// целями.
fn same_tree(a: &Path, b: &Path) -> bool {
    let names = |d: &Path| -> Option<Vec<std::ffi::OsString>> {
        let mut v: Vec<_> = std::fs::read_dir(d).ok()?.flatten().map(|e| e.file_name()).collect();
        v.sort();
        Some(v)
    };
    let (Some(na), Some(nb)) = (names(a), names(b)) else {
        return false;
    };
    if na != nb {
        return false;
    }
    na.iter().all(|n| {
        let (pa, pb) = (a.join(n), b.join(n));
        let (Ok(ma), Ok(mb)) = (std::fs::symlink_metadata(&pa), std::fs::symlink_metadata(&pb)) else {
            return false;
        };
        if ma.file_type().is_symlink() || mb.file_type().is_symlink() {
            ma.file_type().is_symlink()
                && mb.file_type().is_symlink()
                && std::fs::read_link(&pa).ok() == std::fs::read_link(&pb).ok()
        } else if ma.is_dir() || mb.is_dir() {
            ma.is_dir() && mb.is_dir() && same_tree(&pa, &pb)
        } else {
            ma.len() == mb.len() && std::fs::read(&pa).ok() == std::fs::read(&pb).ok()
        }
    })
}

/// Байты курсоров-оригиналов пакета (обычные файлы `.cur`/`.ani`).
fn original_cursor_bytes(originals: &Path) -> Vec<Vec<u8>> {
    let Ok(rd) = std::fs::read_dir(originals) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| {
            let l = p.to_string_lossy().to_lowercase();
            l.ends_with(".cur") || l.ends_with(".ani")
        })
        .collect();
    paths.sort();
    paths.into_iter().filter_map(|p| std::fs::read(p).ok()).collect()
}

fn write_manifest(path: &Path, m: &Value) -> std::io::Result<()> {
    std::fs::write(path, imp::dumps_indent2(m))
}

/// Пройти установленные пакеты в `extensions_dir` и перевести заново темы
/// курсоров «из Windows», которых касается исправление переводчика.
///
/// `current`/`changes` — версия и таблица переводчика (в живом пути —
/// [`imp::CURSOR_CONVERTER_VERSION`] и [`imp::CURSOR_CONVERTER_CHANGES`]);
/// `reinstall(id, новое дерево)` — поставить новое дерево пакета на место
/// прежнего (в живом пути — [`replace_installed`]). Манифест нового дерева
/// уже несёт версию `current`.
pub fn refresh_windows_cursors_in(
    extensions_dir: &Path,
    current: u32,
    changes: &[(u32, imp::CursorFixScope)],
    reinstall: &mut dyn FnMut(&str, &Path) -> Result<(), String>,
) -> CursorRefresh {
    let mut out = CursorRefresh::default();
    let Ok(rd) = std::fs::read_dir(extensions_dir) else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let manifest_path = dir.join("manifest.json");
        let Some(mut manifest) = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            continue;
        };
        let Some(from) = imp::cursor_manifest_converter(&manifest) else {
            continue;
        };
        if from >= current {
            continue;
        }
        // Старые темы без оригиналов перевести нечем — молча мимо.
        let originals = dir.join("originals");
        if !originals.is_dir() {
            continue;
        }
        let traits = imp::cursor_set_traits(&original_cursor_bytes(&originals));
        if !imp::cursor_retranslate_due(from, current, changes, &traits) {
            continue;
        }
        let id = manifest.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        let name = manifest.get("name").and_then(Value::as_str).unwrap_or(&id).to_string();
        let fail = |out: &mut CursorRefresh, what: String| {
            crate::log_warn!("cursors", "перевод заново «{name}»: {what}");
            out.errors.push(format!("{name}: {what}"));
        };
        let tmp = match mkdtemp(&std::env::temp_dir(), "rexon_wincur_again_") {
            Ok(t) => t,
            Err(e) => {
                fail(&mut out, e);
                continue;
            }
        };
        let step = (|| -> Result<Option<String>, String> {
            let rxn = crate::imp_live::retranslate_windows_cursors(&dir, &tmp.join("out"))?;
            let fresh = tmp.join("new");
            ext_utils::safe_zip_extract(&rxn, &fresh)
                .map_err(|_| tr("The file is damaged or is not an .rxn archive"))?;
            let fresh_manifest = fresh.join("manifest.json");
            let mut m: Value = std::fs::read_to_string(&fresh_manifest)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .ok_or_else(|| tr("manifest.json not found in the .rxn archive"))?;
            imp::set_cursor_manifest_converter(&mut m, current);
            write_manifest(&fresh_manifest, &m).map_err(|e| e.to_string())?;
            if same_tree(&dir.join("data"), &fresh.join("data")) {
                // Побайтно то же: файлы на месте, только версия.
                imp::set_cursor_manifest_converter(&mut manifest, current);
                write_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
                return Ok(None);
            }
            let theme_dir = m
                .get("windows_cursors")
                .and_then(|w| w.get("theme_dir"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            reinstall(&id, &fresh)?;
            Ok(Some(theme_dir))
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        match step {
            Ok(None) => out.version_only.push(id),
            Ok(Some(theme_dir)) => out.replaced.push((name.clone(), theme_dir)),
            Err(e) => fail(&mut out, e),
        }
    }
    out
}

/// Поставить новое дерево `fresh` пакета `ext_id` на место установленного —
/// тем же планом, что обычная установка (копия темы в каталог курсоров
/// заменяется по метке владельца). Сбой — всё как было.
pub fn replace_installed(ext_id: &str, fresh: &Path) -> ExecResult<()> {
    let extensions_dir = qt_extensions::extensions_dir();
    let dest = extensions_dir.join(ext_id);
    if !dest.is_dir() {
        return err(trf("Extension '%1' not found", &[&ext_id]));
    }
    // Прежний пакет — в сторону на той же ФС (переименование), вернуть при сбое.
    let backup = rexon_config_dir().join(format!("extensions-old-{ext_id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&backup);
    std::fs::rename(&dest, &backup).map_err(|e| trf("couldn't copy the extension: %1", &[&e]))?;
    let restore = |dest: &Path, backup: &Path| {
        let _ = std::fs::remove_dir_all(dest);
        let _ = std::fs::rename(backup, dest);
    };
    let mut undo = crate::install_plan::Undo::new();
    if let Err(e) = crate::install_plan::copy_tree(fresh, &dest, &mut undo) {
        undo.finish(false);
        restore(&dest, &backup);
        return err(trf("couldn't copy the extension: %1", &[&e]));
    }
    let state = qt_extensions::load_state();
    let outcome = (|| -> ExecResult<(Extension, Option<Vec<crate::install_journal::Placed>>, Vec<String>)> {
        let ext = Extension::load_from_dir(&dest, &state)
            .map_err(|e| trf("the extension manifest was not accepted: %1", &[&e]))?;
        let plan = make_plan(&ext, true)?;
        let placed = Some(crate::install_journal::placed_from_steps(&plan.steps, &|p| {
            std::fs::symlink_metadata(p).is_ok()
        }));
        let notes = exec_plan_noted_undo(plan, &mut undo)?;
        Ok((ext, placed, notes))
    })();
    undo.finish(outcome.is_ok());
    match outcome {
        Ok((ext, placed, notes)) => {
            let _ = std::fs::remove_dir_all(&backup);
            for n in notes {
                crate::log_warn!("cursors", "перевод заново «{ext_id}»: {n}");
            }
            record_in_journal(&ext, placed);
            Ok(())
        }
        Err(e) => {
            restore(&dest, &backup);
            Err(e)
        }
    }
}

/// Живой проход «Курсоров» ([`refresh_windows_cursors_in`] по каталогу
/// расширений). Отдаёт строки для человека — только про реально
/// заменённое; пусто — сказать нечего. Активная тема применяется заново
/// тем же планом, что «Применить», и подпись честно говорит, где новый вид
/// виден сразу, а где — после повторного входа.
pub fn refresh_windows_cursors_live() -> Vec<String> {
    let report = refresh_windows_cursors_in(
        &qt_extensions::extensions_dir(),
        imp::CURSOR_CONVERTER_VERSION,
        imp::CURSOR_CONVERTER_CHANGES,
        &mut |id, fresh| replace_installed(id, fresh),
    );
    if report.replaced.is_empty() {
        return Vec::new();
    }
    let mut installer =
        crate::appearance_installer::LiveInstaller::new("", Box::new(|_: &str| {}));
    let active = installer.active_cursor_theme();
    report
        .replaced
        .iter()
        .map(|(name, theme_dir)| {
            if !theme_dir.is_empty() && active.as_deref() == Some(theme_dir.as_str()) {
                if !installer.refresh_cursor(theme_dir) {
                    crate::log_warn!("cursors", "«{name}»: тема курсора не применилась заново");
                }
                trf(
                    // TRANSLATORS: %1 is a cursor theme name; it is the active one.
                    "Cursors “%1” updated. Apps opened from now on show the new look; everywhere — after you log in again.",
                    &[name],
                )
            } else {
                // TRANSLATORS: %1 is a cursor theme name.
                trf("Cursors “%1” updated", &[name])
            }
        })
        .collect()
}

// ── Второй виджет обычного пакета Conky (дефект 14.11.2026) ─────────────
//
// Прежняя установка обычного виджета Conky (`theme-conky`, не набор) клала
// всю `data/` в `<корень Conky>/<id>`, а каждую подпапку `data/<подпапка>`
// ещё и в `<корень Conky>/<подпапка>` (с меткой владельца). Вторая копия —
// второй виджет с тем же файлом настроек: в списке он виден дважды и
// запускается дважды. Новая установка вторую копию не кладёт
// (`ext_manager::safe_install_into`); уже лежащие убирает проход ниже — и
// только те, что Rexon положил сам и что с тех пор никто не менял.

/// Копия `dup` — то, что положил Rexon из `src`: только обычные файлы и
/// папки (без ссылок), каждый файл есть в `src` с теми же байтами либо в
/// том виде, в каком его оставила починка путей перед запуском
/// ([`crate::de_apply::conky_repair_text`] для папки `widget`). Метка
/// владельца в корне не в счёт. Лишний файл (виджет что-то записал сам,
/// человек поправил) — не наша копия, она остаётся.
fn conky_copy_is_pristine(src: &Path, dup: &Path, home: &str, widget: &str, top: bool) -> bool {
    let Ok(entries) = std::fs::read_dir(dup) else { return false };
    let exists = |p: &str| Path::new(p).exists();
    let is_dir = |p: &str| Path::new(p).is_dir();
    for e in entries.flatten() {
        let name = e.file_name();
        if top && name == ext_manager::OWNER_MARK {
            continue;
        }
        let (Ok(ft), Ok(sm)) = (e.file_type(), std::fs::symlink_metadata(src.join(&name))) else {
            return false;
        };
        if ft.is_symlink() || sm.file_type().is_symlink() {
            return false;
        }
        if ft.is_dir() {
            if !sm.is_dir() || !conky_copy_is_pristine(&src.join(&name), &e.path(), home, widget, false) {
                return false;
            }
            continue;
        }
        if !ft.is_file() || !sm.is_file() {
            return false;
        }
        let (Ok(mine), Ok(orig)) = (std::fs::read(e.path()), std::fs::read(src.join(&name))) else {
            return false;
        };
        if mine == orig {
            continue;
        }
        let repaired = std::str::from_utf8(&orig)
            .ok()
            .and_then(|t| crate::de_apply::conky_repair_text(t, home, widget, &exists, &is_dir));
        if repaired.as_deref().map(str::as_bytes) != Some(mine.as_slice()) {
            return false;
        }
    }
    true
}

/// Убрать вторые копии обычных виджетов Conky у пакетов из
/// `extensions_dir` (см. заметку выше). Копия `<корень>/<подпапка>`
/// снимается, только если: пакет — обычный виджет (не набор: у набора
/// подпапки и есть виджеты), его каталог `<корень>/<id>/<подпапка>` на
/// месте (виджет никуда не пропадает), метка владельца — id этого пакета,
/// содержимое — нетронутая копия ([`conky_copy_is_pristine`]). Работающая
/// вторая копия при неработающей основной не снимается (виджет не исчезнет
/// со стола посреди сеанса — снимется в следующий раз). Перед снятием
/// процессы копии гасятся, её автозапуск переходит на тот же виджет в
/// основном каталоге (вложенный `<id>/<подпапка>` у пакета из нескольких
/// виджетов, иначе `<id>`), если у того своего нет. Возвращает снятые пути.
pub fn heal_conky_duplicates(io: &dyn crate::animations::LookIo, ctx: &crate::de_apply::Ctx, extensions_dir: &Path) -> Vec<String> {
    let mut removed = Vec::new();
    let Ok(rd) = std::fs::read_dir(extensions_dir) else { return removed };
    let mut dirs: Vec<PathBuf> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
    dirs.sort();
    let root = crate::de_apply::conky_root(ctx);
    let valid = |n: &str| !n.is_empty() && n != "." && n != ".." && !n.contains('/');
    let mut running: Option<Vec<String>> = None;
    for dir in dirs {
        let Some(manifest) = std::fs::read_to_string(dir.join("manifest.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Map<String, Value>>(&t).ok())
        else {
            continue;
        };
        if manifest.get("type").and_then(Value::as_str) != Some(crate::de_apply::CONKY_KIND)
            || manifest.get(crate::conky_sets::SET_MANIFEST_KEY).is_some_and(Value::is_object)
        {
            continue;
        }
        let id = manifest.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        let main = Path::new(&root).join(&id);
        if !valid(&id) || !main.is_dir() {
            continue;
        }
        let Ok(subs) = std::fs::read_dir(dir.join("data")) else { continue };
        let mut names: Vec<String> = subs
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            if name == id || !valid(&name) {
                continue;
            }
            let dup = format!("{root}/{name}");
            let dup_p = Path::new(&dup);
            let real_dir = std::fs::symlink_metadata(dup_p).is_ok_and(|m| m.is_dir());
            let mark = std::fs::read_to_string(dup_p.join(ext_manager::OWNER_MARK)).unwrap_or_default();
            if !real_dir || mark.trim() != id || !main.join(&name).is_dir() {
                continue;
            }
            if !conky_copy_is_pristine(&dir.join("data").join(&name), dup_p, &ctx.home, &dup, true) {
                crate::log_info!("conky", "вторая копия «{dup}» пакета «{id}» изменена — оставлена");
                continue;
            }
            // Куда переходит этот виджет: вложенный `<id>/<подпапка>` у
            // пакета из нескольких виджетов, иначе сам `<id>`.
            let nested = format!("{id}/{name}");
            let target = if crate::de_apply::conky_nested_widgets(io, ctx, &id).contains(&nested) { nested } else { id.clone() };
            let running = running.get_or_insert_with(|| crate::de_apply::conky_running_ids(io, ctx));
            if running.contains(&name) && !running.contains(&target) {
                continue;
            }
            let autostart = |w: &str| Path::new(&crate::de_apply::conky_autostart_path(ctx, w)).is_file();
            if autostart(&name) && !autostart(&target) {
                let _ = crate::de_apply::set_conky_autostart(io, ctx, &target, true);
            }
            crate::de_apply::conky_forget_widget(io, ctx, &dup);
            match std::fs::remove_dir_all(dup_p) {
                Ok(()) => {
                    crate::log_info!("conky", "снята вторая копия виджета «{id}»: {dup}");
                    removed.push(dup);
                }
                Err(e) => crate::log_warn!("conky", "вторая копия «{dup}» не снялась: {e}"),
            }
        }
    }
    removed
}

/// Живой проход [`heal_conky_duplicates`] (старт Rexon). Проверки модулей
/// живой профиль не трогают.
pub fn heal_conky_duplicates_live() {
    if cfg!(test) {
        return;
    }
    let ctx = crate::de_apply::Ctx::live();
    heal_conky_duplicates(&crate::animations::SystemIo, &ctx, &qt_extensions::extensions_dir());
}

/// Удалить расширение (порт `uninstall`).
pub fn uninstall(ext_id: &str) -> ExecResult<()> {
    let state = qt_extensions::load_state();
    let Some(ext) = qt_extensions::installed_in(&qt_extensions::extensions_dir(), &state)
        .into_iter()
        .find(|e| e.ext_id() == ext_id)
    else {
        return err(trf("Extension '%1' not found", &[&ext_id]));
    };

    if constants::is_scripted(ext.ext_type()) {
        run_script(&ext, "uninstall.sh");
        if ext.ext_type() == "desktop-tool" {
            remove_desktop_entry(ext_id);
        }
    } else {
        if ext.ext_type() == "theme-conky" {
            // Погасить виджет (по номерам процессов) и снять его автозапуск.
            let ctx = crate::de_apply::Ctx::live();
            let dir = format!("{}/{ext_id}", crate::de_apply::conky_root(&ctx));
            crate::de_apply::conky_forget_widget(&crate::animations::SystemIo, &ctx, &dir);
            // Набор виджетов из файла: виджеты — свои папки (только с меткой
            // этого пакета; чужую одноимённую не трогаем).
            if let Some(set) = crate::conky_sets::set_of_manifest(&ext.manifest) {
                for w in &set.widgets {
                    let wdir = format!("{}/{}", crate::de_apply::conky_root(&ctx), w.folder);
                    let mark = std::fs::read_to_string(format!("{wdir}/{}", ext_manager::OWNER_MARK)).unwrap_or_default();
                    if mark.trim() == ext_id {
                        crate::de_apply::conky_forget_widget(&crate::animations::SystemIo, &ctx, &wdir);
                    }
                }
            }
        }
        let plan = make_plan(&ext, false)?;
        // Файлы, которые положила установка, снимает общий журнал (шаг 3г
        // слияния установщиков): только записанное и только своё. Выключение
        // и перезагрузка оболочки — шаги плана вокруг него; пакет, поставленный
        // до журнала (путей нет), — прежним планом целиком.
        let journal = crate::install_journal::view_at(&rexon_config_dir());
        let key = crate::install_journal::key_of(crate::install_journal::Source::Rxn, ext_id);
        let by_journal = journal
            .items
            .get(&key)
            .and_then(|e| crate::install_journal::rxn_removal(&plan.steps, e, &crate::install_journal::probe_live));
        match by_journal {
            Some(removal) => {
                exec_plan(Plan { steps: removal.steps.clone(), ..plan })?;
                crate::install_journal::prune(&removal);
            }
            None => exec_plan(plan)?,
        }
    }
    ext_utils::safe_rmtree(&ext.dir);
    save_state_with(ext_id, None)
}

/// Забыть расширения, чьи темы человек снял с плитки «Оформления».
///
/// Тема курсоров или значков из файла Windows встаёт расширением: копия
/// пакета лежит в каталоге расширений, а сама тема — в каталоге тем.
/// «Удалить» на плитке снимает только каталог темы, и запись расширения
/// оставалась навсегда — с исходными файлами и пометкой «включено»
/// (приёмка 10.10.2026). `removed` — снятые пути. Расширение забывается,
/// когда снята хоть одна его тема и на диске не осталось ни одной.
/// Возвращает номера забытых расширений.
pub fn forget_extensions_of_removed_themes(removed: &[String]) -> Vec<String> {
    if removed.is_empty() {
        return Vec::new();
    }
    let removed: Vec<&str> = removed.iter().map(|p| p.trim_end_matches('/')).collect();
    let state = qt_extensions::load_state();
    let mut forgotten = Vec::new();
    for ext in qt_extensions::installed_in(&qt_extensions::extensions_dir(), &state) {
        let kind = ext.ext_type().to_string();
        if constants::is_scripted(&kind) {
            continue;
        }
        let Some(base) = constants::install_path(&kind) else { continue };
        let Ok(entries) = std::fs::read_dir(ext.dir.join("data")) else { continue };
        let themes: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| base.join(e.file_name()))
            .collect();
        let hit = themes.iter().any(|t| removed.contains(&t.to_string_lossy().as_ref()));
        if !hit || themes.iter().any(|t| std::fs::symlink_metadata(t).is_ok()) {
            continue;
        }
        let id = ext.ext_id().to_string();
        match uninstall(&id) {
            Ok(()) => forgotten.push(id),
            Err(e) => crate::log_warn!("theme", "запись расширения «{id}» не снята — {e}"),
        }
    }
    forgotten
}

/// Объекты дополнения окружения в пакете (имена, как на диске после
/// установки). Пусто — пакет не дополнение окружения.
pub fn de_addon_objects(ext: &Extension) -> Vec<String> {
    let kind = ext.ext_type().to_string();
    if !matches!(crate::de_apply::mode(&kind), Some(crate::de_apply::Mode::Addon(_))) {
        return Vec::new();
    }
    let mut b = SnapBuilder::default();
    b.deep(&ext.dir, false);
    let fs = FsSnap::from_json(&b.into_value());
    let tmp = TmpAlloc::new("/tmp/rexon-de-objects");
    let which = |_: &str| None;
    let run_ok = |_: &[String]| false;
    let home = home();
    let env = ExtEnv {
        home: &home,
        fs: &fs,
        which: &which,
        run_ok: &run_ok,
        xdg_current_desktop: "",
        xdg_session_type: "",
        wayland_display: false,
        stage_helper_available: false,
        system_helper_path: "",
        system_helper_exists: false,
        helper_family: "arch",
        tmp: &tmp,
    };
    let dir = s(&ext.dir);
    crate::ext_manager::de_object_names(&env, &ExtInfo { dir: &dir, manifest: &ext.manifest })
}

/// Включено ли дополнение окружения сейчас — по правде окружения, а не по
/// отметке Rexon (её человек мог обойти настройками самого стола).
pub fn de_addon_enabled(ext: &Extension) -> bool {
    let io = crate::animations::SystemIo;
    let ctx = crate::de_apply::Ctx::live();
    de_addon_objects(ext)
        .iter()
        .any(|n| crate::de_apply::addon_enabled(&io, &ctx, ext.ext_type(), n))
}

/// Переключить расширение (порт `toggle`). Возвращает НОВОЕ состояние.
///
/// Дополнение окружения (скрипт KWin, расширение GNOME, апплет Cinnamon…)
/// включается и выключается по-настоящему — тем же путём, что кнопка
/// «Включить»/«Выключить» вкладки «Оформление» (`de_apply`); установка его
/// больше не включает (решение 04.10.2026).
pub fn toggle(ext_id: &str) -> ExecResult<bool> {
    let state = qt_extensions::load_state();
    if let Some(ext) = qt_extensions::installed_in(&qt_extensions::extensions_dir(), &state)
        .into_iter()
        .find(|e| e.ext_id() == ext_id)
    {
        let objects = de_addon_objects(&ext);
        if !objects.is_empty() {
            let io = crate::animations::SystemIo;
            let ctx = crate::de_apply::Ctx::live();
            let next = !de_addon_enabled(&ext);
            for name in &objects {
                crate::de_apply::set_addon(&io, &ctx, ext.ext_type(), name, next)?;
            }
            save_state_with(ext_id, Some(next))?;
            return Ok(next);
        }
    }
    toggle_flag(ext_id)
}

/// Переключить отметку «включено» скриптового расширения.
fn toggle_flag(ext_id: &str) -> ExecResult<bool> {
    let state = qt_extensions::load_state();
    let Some(ext) = qt_extensions::installed_in(&qt_extensions::extensions_dir(), &state)
        .into_iter()
        .find(|e| e.ext_id() == ext_id)
    else {
        // TRANSLATORS: shown in the status line after the "couldn't switch
        // the extension" prefix; %1 is the extension id.
        return err(trf("extension “%1” not found", &[&ext_id]));
    };
    let next = !ext.enabled;
    save_state_with(ext_id, Some(next))?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_plan::steamos_block_of;

    // ── Системная часть плана: отказ доходит до человека ─────────────────

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rexon-ext-exec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// План «пользовательская часть + системная часть» — как у темы экрана
    /// входа: файл в домашней папке, затем помощник через pkexec.
    fn plan_with_system_part(dir: &Path, kind: &str) -> Vec<ExtStep> {
        vec![
            ExtStep::WriteFile { path: s(&dir.join("user-part.txt")), content: "x".into() },
            ExtStep::Run {
                argv: vec!["pkexec".into(), "/usr/lib/rexon-system-helper".into(), kind.into()],
                timeout_secs: Some(300),
            },
        ]
    }

    fn exec_steps(steps: &[ExtStep], pre_ran: &mut Vec<Vec<String>>, ctx: &ExecCtx) -> ExecResult<()> {
        // Как у `exec_plan_noted`: план установки при сбое откатывается.
        let mut undo = if ctx.install { crate::install_plan::Undo::new() } else { crate::install_plan::Undo::off() };
        let outcome = exec_steps_noted(steps, pre_ran, ctx, &mut Vec::new(), &mut undo);
        undo.finish(outcome.is_ok());
        outcome
    }

    fn run(steps: &[ExtStep], steamos: bool, install: bool, answer: PrivOutcome) -> (ExecResult<()>, usize) {
        let calls = std::cell::Cell::new(0usize);
        let fake = |_: &ExtStep| {
            calls.set(calls.get() + 1);
            answer.clone()
        };
        let ctx = ExecCtx { steamos, install, privileged: &fake };
        let r = exec_steps(steps, &mut Vec::new(), &ctx);
        (r, calls.get())
    }

    /// Тема Plymouth с модулем из самой темы (код от root при загрузке)
    /// отвергается ДО окна пароля — и через помощника, и на запасном
    /// сыром `pkexec bash`, где других проверок нет вовсе.
    #[test]
    fn boot_splash_with_own_code_is_refused_before_password() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let src = scratch("ply-src");
        std::fs::write(src.join("neon.plymouth"), "[Plymouth Theme]\nModuleName=../../../share/plymouth/themes/neon/evil\n").unwrap();
        std::fs::write(src.join("evil.so"), b"\x7fELF").unwrap();
        let staged = scratch("ply-staged");
        let _ = std::fs::remove_dir_all(&staged);
        let stage = ExtStep::Stage { staged: s(&staged), sources: vec![s(&src)] };
        let helper = ExtStep::Run {
            argv: vec![
                "pkexec".into(), "/usr/lib/rexon-system-helper".into(), "plymouth".into(),
                "neon".into(), s(&staged), "arch".into(),
            ],
            timeout_secs: Some(900),
        };
        let fallback = ExtStep::PkexecScript { prefix: "rexon_plymouth".into(), script: String::new() };
        for privileged in [helper, fallback] {
            let steps = vec![stage.clone(), privileged];
            let (r, calls) = run(&steps, false, true, PrivOutcome::Done);
            let msg = r.expect_err("тема с кодом прошла");
            assert!(msg.contains("evil.so") || msg.contains("neon.plymouth"), "{msg}");
            assert_eq!(calls, 0, "окно пароля показано для отвергнутой темы");
            let _ = std::fs::remove_dir_all(&staged);
        }
        // Штатная тема идёт дальше, к помощнику.
        std::fs::remove_file(src.join("evil.so")).unwrap();
        std::fs::write(src.join("neon.plymouth"), "[Plymouth Theme]\nModuleName=script\n").unwrap();
        let steps = vec![stage.clone(), ExtStep::PkexecScript { prefix: "rexon_plymouth".into(), script: String::new() }];
        let (r, calls) = run(&steps, false, true, PrivOutcome::Done);
        assert!(r.is_ok() && calls == 1);
        let _ = std::fs::remove_dir_all(&staged);
        let _ = std::fs::remove_dir_all(&src);
    }

    /// Отказ в пароле больше не глушится: установка получает ошибку с
    /// человеческим текстом, а не «установлено».
    #[test]
    fn denied_password_reaches_the_message() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let dir = scratch("denied");
        let steps = plan_with_system_part(&dir, "sddm");
        let (r, calls) = run(&steps, false, true, PrivOutcome::Failed { code: 126, stderr: String::new() });
        assert_eq!(calls, 1);
        let text = r.expect_err("отказ в пароле — ошибка установки");
        assert!(text.contains("administrator password was not confirmed"), "{text}");
        // Всё или ничего: пользовательская часть, легшая до окна пароля,
        // снята откатом.
        assert!(!dir.join("user-part.txt").exists(), "отказ в пароле оставил пользовательскую часть");
        // То же — по тексту агента polkit при любом коде.
        let (r, _) = run(&steps, false, true, PrivOutcome::Failed {
            code: 127,
            stderr: "Error executing command as another user: Not authorized\n".into(),
        });
        assert!(r.unwrap_err().contains("password"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ошибка записи в системный раздел (как на SteamOS или при полном
    /// диске) доходит до сообщения дословно — последней строкой ответа.
    #[test]
    fn write_error_in_usr_reaches_the_message() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let dir = scratch("readonly");
        let steps = plan_with_system_part(&dir, "plymouth");
        let stderr = "install: creating directory '/usr/share/plymouth/themes/x'\n\
                      cp: cannot create regular file '/usr/share/plymouth/themes/x/x.plymouth': \
                      Read-only file system\n\n";
        let (r, _) = run(&steps, false, true, PrivOutcome::Failed { code: 1, stderr: stderr.into() });
        let text = r.unwrap_err();
        assert!(text.starts_with("Rexon could not change the system part of the theme: cp:"), "{text}");
        assert!(text.ends_with("Read-only file system"), "{text}");
        // Молчаливая команда — хотя бы код, но не пустота.
        let (r, _) = run(&steps, false, true, PrivOutcome::Failed { code: 3, stderr: String::new() });
        assert!(r.unwrap_err().contains("error code 3"));
        // Таймаут и отсутствие pkexec — тоже ошибки, а не успех.
        assert!(run(&steps, false, true, PrivOutcome::TimedOut).0.is_err());
        assert_eq!(
            run(&steps, false, true, PrivOutcome::NotStarted).0.unwrap_err(),
            crate::system_plan::pkexec_missing_message()
        );
        // Успех — успех.
        assert_eq!(run(&steps, false, true, PrivOutcome::Done).0, Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Обычные (не системные) команды плана по-прежнему не роняют установку:
    /// обновить кэш меню не вышло — не повод откатывать тему.
    #[test]
    fn plain_command_failure_is_still_not_fatal() {
        let steps = vec![ExtStep::Run { argv: vec!["false".into()], timeout_secs: Some(5) }];
        let (r, calls) = run(&steps, false, true, PrivOutcome::Done);
        assert_eq!((r, calls), (Ok(()), 0));
    }

    /// SteamOS (Steam Deck и Steam Machine): установка с системной частью
    /// отказывает ДО первого шага и называет причину по тому, что ставится;
    /// pkexec не зовётся вовсе.
    #[test]
    fn steamos_refuses_system_part_before_any_step() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        use crate::distro_compat::SteamOsBlock;
        for (kind, block) in [
            ("sddm", SteamOsBlock::LoginScreen),
            ("gdm-bg", SteamOsBlock::LoginScreen),
            ("plymouth", SteamOsBlock::BootSplash),
            ("mate-layouts", SteamOsBlock::SystemPart),
        ] {
            let dir = scratch(&format!("steamos-{kind}"));
            let steps = plan_with_system_part(&dir, kind);
            let (r, calls) = run(&steps, true, true, PrivOutcome::Done);
            assert_eq!(r, Err(block.reason()), "{kind}");
            assert_eq!(calls, 0, "{kind}: pkexec на SteamOS не зовётся");
            assert!(!dir.join("user-part.txt").exists(), "{kind}: ни одного шага до отказа");
            let _ = std::fs::remove_dir_all(&dir);
        }
        // Скрипт-обёртка опознаётся по префиксу.
        let script = ExtStep::PkexecScript { prefix: "rexon_plymouth".into(), script: String::new() };
        assert_eq!(steamos_block_of(&script), SteamOsBlock::BootSplash);
    }

    /// Удаление на SteamOS системную часть пропускает, а свою доводит до
    /// конца — иначе расширение нельзя было бы убрать вовсе.
    #[test]
    fn steamos_uninstall_skips_system_part() {
        let dir = scratch("steamos-uninstall");
        let steps = plan_with_system_part(&dir, "sddm");
        let (r, calls) = run(&steps, true, false, PrivOutcome::Failed { code: 1, stderr: "x".into() });
        assert_eq!((r, calls), (Ok(()), 0));
        assert!(dir.join("user-part.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Снимок распакованного архива обязан лечь в слой НАБЛЮДЕНИЙ: по
    /// пре-слою планировщик решал бы, что дерево существовало до нас, и
    /// `find_manifest_root` искал бы манифест не там.
    #[test]
    fn snap_tree_is_observations_only() {
        let value = snap_tree(Path::new("/nonexistent-rexon-snap"));
        assert_eq!(value["pre_dirs"].as_array().map(Vec::len), Some(0));
        assert_eq!(value["pre_files"].as_array().map(Vec::len), Some(0));
    }

    /// Каталоги установки в снимке — ОБА набора: и тот, что строит
    /// планировщик от голого `$HOME`, и продуктовый XDG-осведомлённый.
    /// Разъезд этих двух наборов уже стоил вкладке «Оформление» пустой
    /// витрины (правило #12).
    #[test]
    fn target_dirs_cover_both_path_tables() {
        let dirs = target_dirs("/home/tester");
        let suffix = constants::install_path_suffix("theme-icons").expect("тип есть в матрице");
        let plain = PathBuf::from(format!("/home/tester/{suffix}"));
        assert!(dirs.contains(&plain), "нет пути планировщика: {plain:?}");
        assert!(dirs.iter().any(|d| d.ends_with(".themes")), "нет ~/.themes");
    }

    /// Ответы исполнителя идут в строку состояния вкладки как есть — в
    /// английском интерфейсе в них ни одной русской буквы. Русский слово в
    /// слово сверяет `installer_parity::ext_exec_errors_in_russian_as_before`.
    #[test]
    fn english_install_errors_have_no_russian() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let missing = install_from_file("/nonexistent/rexon-test.rxn").err().unwrap_or_default();
        assert_eq!(missing, "File not found: /nonexistent/rexon-test.rxn");
        let dir = mkdtemp(&std::env::temp_dir(), "rexon_test_broken_rxn_").expect("каталог создан");
        let file = dir.join("broken.rxn");
        std::fs::write(&file, b"not a zip").expect("файл записан");
        let broken = install_from_file(&s(&file)).err().unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(broken, "The file is damaged or is not an .rxn archive");
        let texts = [
            missing,
            broken,
            privileged_failure_text(&PrivOutcome::NotStarted).unwrap_or_default(),
            privileged_failure_text(&PrivOutcome::TimedOut).unwrap_or_default(),
        ];
        for t in texts {
            assert!(!t.is_empty());
            assert!(!t.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')), "{t}");
        }
    }

    /// Чужой формат больше не отвергается по расширению имени.
    ///
    /// Сторож переезда импортёров в ядро: пока живой `ImpIo` жил только в
    /// мосте, `install_from_file` отвечала «умею только .rxn» на любой
    /// `.themepack`. Возврат этой ветки означал бы, что вкладка снова
    /// обманывает своим же фильтром окна выбора файла, — поэтому проверяется
    /// не «получилось», а «ответ уже не про расширение имени».
    #[test]
    fn foreign_format_is_no_longer_refused_by_extension() {
        let dir = mkdtemp(&std::env::temp_dir(), "rexon_test_foreign_")
            .expect("каталог создан");
        let file = Path::new(&dir).join("тема.themepack");
        // Не настоящий CAB: до конвертации дело не дойдёт, но и отказа по
        // расширению имени быть уже не должно.
        std::fs::write(&file, b"not a real themepack").expect("файл записан");
        let answer = install_from_file(&s(&file));
        let _ = std::fs::remove_dir_all(&dir);
        let text = answer.err().unwrap_or_default();
        assert!(
            !text.contains("умею ставить только .rxn"),
            "отказ по расширению имени вернулся: {text}"
        );
    }

    /// Весть об установке: файл события с именем расширения — ровно тот, что
    /// читает слушатель моста, — и уборка устаревших за 5 минут.
    #[test]
    fn announce_installed_writes_event_and_sweeps_stale() {
        use crate::rexon_bridge::RexonBridge;
        let dir = mkdtemp(&std::env::temp_dir(), "rexon_test_bridge_").expect("каталог создан");
        let bridge = RexonBridge::new(dir.join("bridge.json"), dir.join("bridge_commands"));
        bridge.ensure_dirs().expect("каталоги моста");
        let stale = dir.join("bridge_commands/event_old_1.json");
        std::fs::write(&stale, b"{}").expect("старое событие");
        let old = std::time::SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .and_then(|f| f.set_modified(old))
            .expect("mtime в прошлом");

        announce_installed(&bridge, "Часы", std::time::SystemTime::now()).expect("весть ушла");

        let names: Vec<String> = SnapBuilder::names_of(&dir.join("bridge_commands"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!names.contains(&"event_old_1.json".to_string()), "старьё не подметено: {names:?}");
        assert_eq!(names.len(), 1, "ровно одно событие: {names:?}");
        assert!(names[0].starts_with("event_extension_installed_"), "{names:?}");
    }

    /// Переменная, по которой дочерний прогон узнаёт свой временный каталог.
    const E2E_BASE_ENV: &str = "REXON_EXT_BRIDGE_E2E_BASE";

    /// Сквозной сторож: установка шлёт мосту `extension_installed` с ИМЕНЕМ,
    /// удаление и неудача молчат (как `_install_rxn` оригинала), а витрина,
    /// которую перечитывает страница «Расширений», видит установку и
    /// удаление без перезапуска.
    ///
    /// ⚠️ Конвейер читает `HOME`/`XDG_CONFIG_HOME` процесса, а соседние тесты
    /// этого бинаря бегут параллельно. Поэтому сама проверка идёт в ДОЧЕРНЕМ
    /// процессе того же бинаря с подменённым домом — живой профиль и соседи
    /// её не видят.
    #[test]
    fn install_announces_to_bridge_and_list_follows_without_restart() {
        let base = mkdtemp(&std::env::temp_dir(), "rexon_test_ext_bridge_").expect("каталог");
        let home = base.join("home");
        std::fs::create_dir_all(&home).expect("дом");
        let child = "ext_exec::tests::bridge_e2e_child";
        let out = Command::new(std::env::current_exe().expect("путь бинаря"))
            .args(["--exact", child, "--ignored", "--test-threads=1"])
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env(E2E_BASE_ENV, &base)
            .output()
            .expect("дочерний прогон");
        let _ = std::fs::remove_dir_all(&base);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "дочерний прогон не прошёл:\n{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn e2e_rxn(dir: &Path, id: &str, name: &str) -> PathBuf {
        use std::io::Write as _;
        let path = dir.join(format!("{id}.rxn"));
        let mut zw = zip::ZipWriter::new(std::fs::File::create(&path).expect("архив"));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let manifest = serde_json::json!({"id": id, "name": name, "type": "plugin", "version": "1.0"});
        for (entry, data) in [
            ("manifest.json", manifest.to_string()),
            ("install.sh", "exit 0\n".to_string()),
            ("uninstall.sh", "exit 0\n".to_string()),
        ] {
            zw.start_file(entry, opts).expect("запись");
            zw.write_all(data.as_bytes()).expect("запись");
        }
        zw.finish().expect("архив закрыт");
        path
    }

    fn e2e_events(_home: &Path) -> Vec<(String, Value)> {
        // Каталог спрашиваем у ядра, а не склеиваем литералом: сторож обязан
        // читать РОВНО ту папку, куда пишет продукт (в т.ч. при заданном
        // `$XDG_CONFIG_HOME` подставного окружения).
        let dir = crate::rexon_bridge::default_commands_dir();
        let mut out: Vec<(String, Value)> = SnapBuilder::names_of(&dir)
            .into_iter()
            .map(|n| {
                let text = std::fs::read_to_string(dir.join(&n)).unwrap_or_default();
                (n, serde_json::from_str(&text).unwrap_or(Value::Null))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Тело сквозного сторожа — только в дочернем процессе (см. выше).
    #[test]
    #[ignore = "запускается дочерним процессом install_announces_to_bridge_and_list_follows_without_restart"]
    fn bridge_e2e_child() {
        let base = PathBuf::from(std::env::var_os(E2E_BASE_ENV).expect("только дочерний прогон"));
        let home = PathBuf::from(home());
        assert!(home.starts_with(&base), "дом не подменён: {home:?}");

        let rxn = e2e_rxn(&base, "rexon-test-clock", "Тестовые часы");
        let done = install_from_file(&s(&rxn)).expect("установка прошла");
        assert_eq!(done.name, "Тестовые часы");
        let events = e2e_events(&home);
        assert_eq!(events.len(), 1, "ровно одно событие: {events:?}");
        let (file, msg) = &events[0];
        assert!(file.starts_with("event_extension_installed_"), "{file}");
        assert_eq!(msg["target"], "*");
        assert_eq!(msg["command"], "event");
        assert_eq!(msg["params"]["type"], "extension_installed");
        assert_eq!(msg["params"]["ext_name"], "Тестовые часы");
        let listed = |id: &str| qt_extensions::installed().iter().any(|v| v.ext_id == id);
        assert!(listed("rexon-test-clock"), "витрина не видит установленное");

        // Повтор («уже установлено») и битый архив — отказ без вести.
        assert!(install_from_file(&s(&rxn)).is_err());
        let broken = base.join("broken.rxn");
        std::fs::write(&broken, b"not a zip").expect("файл");
        assert!(install_from_file(&s(&broken)).is_err());
        assert_eq!(e2e_events(&home).len(), 1, "неудача не шлёт весть");

        // Удаление: витрина пустеет, вести нет — у оригинала её нет.
        uninstall("rexon-test-clock").expect("удаление прошло");
        assert!(!listed("rexon-test-clock"), "витрина видит удалённое");
        assert_eq!(e2e_events(&home).len(), 1, "удаление не шлёт весть");
    }

    /// Временные каталоги плана уникальны: два плана подряд не имеют права
    /// делить staged-каталог — второй снёс бы файлы первого.
    #[test]
    fn mkdtemp_gives_unique_dirs() {
        let base = std::env::temp_dir();
        let a = mkdtemp(&base, "rexon_test_").expect("каталог создан");
        let b = mkdtemp(&base, "rexon_test_").expect("каталог создан");
        assert_ne!(a, b);
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    // ── Сеанс прав окна (Б9) ─────────────────────────────────────────────

    /// Шаги помощников раскладки и системных тем узнаются операцией, и её
    /// путь «как сейчас» — ровно тот же argv; прочее — прежним путём.
    #[test]
    fn step_priv_op_recognizes_only_exact_helper_argv() {
        use crate::privilege::op::{PrivOp, SystemThemeKind, SystemThemeOp, ThemeFamily};
        let run = |argv: Vec<String>| ExtStep::Run { argv, timeout_secs: Some(60) };
        // Раскладка (фон экрана входа) — Stage с каталогом раскладки.
        let stage = crate::system_plan::privileged_stage_cmd("gdm-bg", "rexon-login", "/tmp/st", true, Some("/tmp/st"), true)
            .unwrap();
        let (op, files) = step_priv_op(&run(stage.clone())).expect("раскладка");
        assert!(matches!(&op, PrivOp::Stage { category, name, source } if category == "gdm-bg" && name == "rexon-login" && source.label == "/tmp/st"));
        assert_eq!(files, vec!["/tmp/st".to_string()]);
        // Системные темы: установка, «Применить», «Удалить».
        let helper = crate::installer::system_helper_path();
        let lang = crate::system_l10n::lang_args(helper);
        let mut install = vec!["pkexec".to_string(), helper.to_string()];
        install.extend(lang.clone());
        install.extend(["plymouth", "neon", "/tmp/th", "debian"].map(String::from));
        let (op, files) = step_priv_op(&run(install.clone())).expect("установка темы");
        assert!(matches!(
            &op,
            PrivOp::SystemTheme(SystemThemeOp::Install { kind: SystemThemeKind::Plymouth, theme, family: ThemeFamily::Debian, .. })
                if theme == "neon"
        ));
        assert_eq!(files, vec!["/tmp/th".to_string()]);
        let apply = crate::de_apply::system_apply_argv(helper, &lang, "sddm", "sugar", "arch");
        assert!(matches!(
            step_priv_op(&run(apply)),
            Some((PrivOp::SystemTheme(SystemThemeOp::Apply { kind: SystemThemeKind::Sddm, .. }), f)) if f.is_empty()
        ));
        let remove = crate::de_apply::system_remove_argv(helper, &lang, "plymouth", "neon", "arch");
        assert!(matches!(
            step_priv_op(&run(remove)),
            Some((PrivOp::SystemTheme(SystemThemeOp::Remove { kind: SystemThemeKind::Plymouth, .. }), _))
        ));
        // Не узнаётся: сырой скрипт, лишнее слово, чужой помощник, чужое
        // семейство, не pkexec, скрипт-обёртка.
        let mut extra = stage.clone();
        extra.push("--force".into());
        let mut other = install.clone();
        other[1] = "/tmp/evil-helper".into();
        let mut fam = install.clone();
        *fam.last_mut().unwrap() = "fedora".into();
        for argv in [
            vec!["pkexec".to_string(), "bash".into(), "/tmp/x.sh".into()],
            extra,
            other,
            fam,
            stage[1..].to_vec(),
        ] {
            assert_eq!(step_priv_op(&run(argv.clone())), None, "{argv:?}");
        }
        let script = ExtStep::PkexecScript { prefix: "rexon_sddm".into(), script: "true".into() };
        assert_eq!(step_priv_op(&script), None);
    }

    /// ⛔ Сторож Б9: никто, кроме сеанса прав (`privilege/`), не запускает
    /// `pkexec` своим `Command::new` — все остальные зовут общий исполнитель
    /// или `privilege::run`. Тексты и argv-литералы (`"pkexec"` в векторе)
    /// сторож не трогает.
    #[test]
    fn no_raw_pkexec_spawn_outside_privilege() {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let rust = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files = Vec::new();
        for krate in std::fs::read_dir(&rust).unwrap().flatten() {
            let src = krate.path().join("src");
            if src.is_dir() {
                walk(&src, &mut files);
            }
        }
        assert!(files.len() > 50, "исходники не найдены: {}", rust.display());
        let mut bad = Vec::new();
        for f in files {
            let shown = f.to_string_lossy().replace('\\', "/");
            if shown.contains("/src/privilege/") {
                continue;
            }
            let text = std::fs::read_to_string(&f).unwrap_or_default();
            let flat: String = text.split_whitespace().collect();
            for needle in ["Command::new(\"pkexec\")", "Command::new(\"/usr/bin/pkexec\")"] {
                if flat.contains(needle) {
                    bad.push(format!("{shown}: {needle}"));
                }
            }
        }
        assert!(bad.is_empty(), "pkexec запускается мимо сеанса прав: {bad:#?}");
    }


    // ── Курсоры из Windows: перевод заново только по исправлению ─────────

    /// Поставить тему «из Windows» в подставной каталог расширений так, как её
    /// кладёт установка: распакованный `.rxn` под каталогом с id пакета.
    fn put_windows_theme(root: &Path, ext_dir: &Path, name: &str, files: &[(&str, Vec<u8>)]) -> PathBuf {
        let set = root.join(format!("set-{name}"));
        std::fs::create_dir_all(&set).unwrap();
        for (f, b) in files {
            std::fs::write(set.join(f), b).unwrap();
        }
        let out = root.join(format!("rxn-{name}"));
        std::fs::create_dir_all(&out).unwrap();
        let mut io = crate::imp_live::LiveImpIo::new(None);
        let rxn = imp::cursor_import_to_rxn(&mut io, &s(&set), name, Some(&s(&out))).unwrap();
        let tmp = root.join(format!("unpacked-{name}"));
        ext_utils::safe_zip_extract(Path::new(&rxn), &tmp).unwrap();
        let m: Value = serde_json::from_str(&std::fs::read_to_string(tmp.join("manifest.json")).unwrap()).unwrap();
        let dest = ext_dir.join(m["id"].as_str().unwrap());
        std::fs::rename(&tmp, &dest).unwrap();
        dest
    }

    fn converter_of(dir: &Path) -> u64 {
        let m: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
        m["windows_cursors"]["converter"].as_u64().unwrap()
    }

    #[test]
    fn windows_cursors_are_converted_again_only_where_the_fix_applies() {
        use crate::wincursor::testbytes::{ani, cur32};
        let root = scratch("wincur-again");
        let ext_dir = root.join("extensions");
        std::fs::create_dir_all(&ext_dir).unwrap();
        let busy = ani(&[cur32(32, 1, 1, 10), cur32(32, 2, 2, 20)], 10, None, None, 2);
        let anim = put_windows_theme(&root, &ext_dir, "Anim", &[("busy.ani", busy.clone()), ("pointer.cur", cur32(32, 0, 0, 1))]);
        let still = put_windows_theme(&root, &ext_dir, "Still", &[("pointer.cur", cur32(32, 0, 0, 1))]);
        let changed = put_windows_theme(&root, &ext_dir, "Changed", &[("busy.ani", busy.clone())]);
        let bare = put_windows_theme(&root, &ext_dir, "Bare", &[("busy.ani", busy)]);
        // Тема старого образца — без оригиналов: пропуск без шума.
        std::fs::remove_dir_all(bare.join("originals")).unwrap();
        // «Прежний переводчик» дал другие байты у одной темы.
        let wait = changed.join("data/changed/cursors/wait");
        assert!(wait.is_file(), "нет {wait:?}");
        std::fs::write(&wait, b"old output").unwrap();
        let anim_data = std::fs::read(anim.join("data/anim/cursors/wait")).unwrap();

        // Подделка: версия 3 исправляет только анимированные (реальная — 2).
        let changes = [(2, imp::CursorFixScope::All), (3, imp::CursorFixScope::Animated)];
        let mut calls: Vec<String> = Vec::new();
        let mut reinstall = |id: &str, fresh: &Path| -> Result<(), String> {
            calls.push(id.to_string());
            let dest = ext_dir.join(id);
            std::fs::remove_dir_all(&dest).unwrap();
            let mut undo = crate::install_plan::Undo::new();
            crate::install_plan::copy_tree(fresh, &dest, &mut undo).map_err(|e| e.to_string())?;
            undo.finish(true);
            Ok(())
        };
        let report = refresh_windows_cursors_in(&ext_dir, 3, &changes, &mut reinstall);
        assert_eq!(report.errors, Vec::<String>::new());
        assert_eq!(report.replaced, vec![("Changed".to_string(), "changed".to_string())]);
        assert_eq!(report.version_only, vec!["wincur-anim".to_string()]);
        assert_eq!(calls, vec!["wincur-changed".to_string()]);
        // Побайтно то же — файлы не тронуты, только версия.
        assert_eq!(std::fs::read(anim.join("data/anim/cursors/wait")).unwrap(), anim_data);
        assert_eq!(converter_of(&anim), 3);
        // Заменённая тема — новые байты и новая версия.
        assert_ne!(std::fs::read(&wait).unwrap(), b"old output");
        assert_eq!(converter_of(&changed), 3);
        // Неподвижную исправление не касается, без оригиналов — мимо.
        assert_eq!(converter_of(&still), 2);
        assert_eq!(converter_of(&bare), 2);

        // Второй заход: всё уже на текущей версии — ни замены, ни шума.
        let again = refresh_windows_cursors_in(&ext_dir, 3, &changes, &mut |_, _| panic!("второй раз не переводится"));
        assert_eq!(again, CursorRefresh::default());
        // С настоящей таблицей (версия 2) не трогается ничего.
        let real = refresh_windows_cursors_in(
            &ext_dir,
            imp::CURSOR_CONVERTER_VERSION,
            imp::CURSOR_CONVERTER_CHANGES,
            &mut |_, _| panic!("при версии 2 переводить нечего"),
        );
        assert_eq!(real, CursorRefresh::default());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn same_tree_compares_names_bytes_and_link_targets() {
        let root = scratch("same-tree");
        for side in ["a", "b"] {
            let d = root.join(side).join("cursors");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("default"), b"xcur").unwrap();
            std::os::unix::fs::symlink("default", d.join("left_ptr")).unwrap();
        }
        assert!(same_tree(&root.join("a"), &root.join("b")));
        std::fs::write(root.join("b/cursors/default"), b"xcuR").unwrap();
        assert!(!same_tree(&root.join("a"), &root.join("b")));
        std::fs::write(root.join("b/cursors/default"), b"xcur").unwrap();
        std::fs::remove_file(root.join("b/cursors/left_ptr")).unwrap();
        std::os::unix::fs::symlink("other", root.join("b/cursors/left_ptr")).unwrap();
        assert!(!same_tree(&root.join("a"), &root.join("b")));
        assert!(!same_tree(&root.join("a"), &root.join("missing")));
        let _ = std::fs::remove_dir_all(&root);
    }

    // ── Вторая копия обычного виджета Conky ────────────────────────────

    /// Машина для прохода уборки: файлы — настоящие (во временной папке),
    /// процессов conky нет, команды не запускаются, а записываются.
    struct NoProcIo {
        ran: std::cell::RefCell<Vec<Vec<String>>>,
    }

    impl crate::animations::LookIo for NoProcIo {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(argv.to_vec());
            (true, String::new())
        }
        fn have(&self, _: &str) -> bool {
            false
        }
        fn read(&self, path: &str) -> Option<String> {
            std::fs::read_to_string(path).ok()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            let p = Path::new(path);
            p.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(p, text).is_ok()
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            if path == "/proc" {
                return Vec::new();
            }
            std::fs::read_dir(path)
                .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
                .unwrap_or_default()
        }
    }

    fn put(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn conky_ctx(home: &Path) -> crate::de_apply::Ctx {
        crate::de_apply::Ctx { home: s(home), xdg_config_home: None, wayland: false, de: String::new() }
    }

    /// Пакет `my-theme` с подпапками `data/<имя>` (у каждой — `conky.conf`
    /// и `lua/rings.lua`).
    fn conky_package(exts: &Path, subs: &[&str]) -> PathBuf {
        let pkg = exts.join("my-theme");
        put(&pkg.join("manifest.json"), r#"{"id": "my-theme", "name": "My Theme", "type": "theme-conky", "version": "1.0"}"#);
        for sub in subs {
            put(&pkg.join(format!("data/{sub}/conky.conf")), &format!("conky.text = [[{sub}]]\n"));
            put(&pkg.join(format!("data/{sub}/lua/rings.lua")), "function conky_main() end\n");
        }
        pkg
    }

    /// Копия подпапки пакета в `dest` (как клал прежний Rexon), с меткой
    /// `mark` (пусто — без метки).
    fn copy_sub(pkg: &Path, sub: &str, dest: &Path, mark: &str) {
        let mut undo = crate::install_plan::Undo::off();
        crate::install_plan::copy_tree(&pkg.join("data").join(sub), dest, &mut undo).unwrap();
        if !mark.is_empty() {
            std::fs::write(dest.join(ext_manager::OWNER_MARK), format!("{mark}\n")).unwrap();
        }
    }

    /// Уборка снимает только свою нетронутую вторую копию: копию без
    /// метки (папка человека), с меткой другого пакета, изменённую и с
    /// лишним файлом — оставляет; основной каталог виджета не трогает;
    /// автозапуск второй копии переходит на основной.
    #[test]
    fn conky_heal_removes_only_own_untouched_duplicate() {
        let home = scratch("conky-heal");
        let exts = home.join(".config/rexon/extensions");
        let subs = ["MyTheme", "Human", "Other", "Edited", "Extra"];
        let pkg = conky_package(&exts, &subs);
        let root = home.join(".config/conky");
        let main = root.join("my-theme");
        let mut undo = crate::install_plan::Undo::off();
        crate::install_plan::copy_tree(&pkg.join("data"), &main, &mut undo).unwrap();
        copy_sub(&pkg, "MyTheme", &root.join("MyTheme"), "my-theme");
        copy_sub(&pkg, "Human", &root.join("Human"), "");
        copy_sub(&pkg, "Other", &root.join("Other"), "other-pack");
        copy_sub(&pkg, "Edited", &root.join("Edited"), "my-theme");
        put(&root.join("Edited/conky.conf"), "conky.text = [[mine]]\n");
        copy_sub(&pkg, "Extra", &root.join("Extra"), "my-theme");
        put(&root.join("Extra/notes.txt"), "человек\n");
        let ctx = conky_ctx(&home);
        let old_autostart = crate::de_apply::conky_autostart_path(&ctx, "MyTheme");
        put(Path::new(&old_autostart), "[Desktop Entry]\nExec=conky -c x\n");

        let io = NoProcIo { ran: Default::default() };
        let removed = heal_conky_duplicates(&io, &ctx, &exts);
        assert_eq!(removed, vec![s(&root.join("MyTheme"))]);
        assert!(!root.join("MyTheme").exists());
        for keep in ["Human", "Other", "Edited", "Extra"] {
            assert!(root.join(keep).join("conky.conf").is_file(), "{keep} снят");
        }
        assert_eq!(std::fs::read_to_string(root.join("Edited/conky.conf")).unwrap(), "conky.text = [[mine]]\n");
        for sub in subs {
            assert!(main.join(sub).join("conky.conf").is_file(), "основной каталог потерял {sub}");
        }
        assert!(!Path::new(&old_autostart).exists(), "автозапуск второй копии остался");
        // Пакет из нескольких виджетов: автозапуск — у вложенного `my-theme/MyTheme`.
        let new_autostart = std::fs::read_to_string(crate::de_apply::conky_autostart_path(&ctx, "my-theme/MyTheme")).unwrap();
        assert!(new_autostart.contains("/my-theme/MyTheme/conky.conf"), "{new_autostart}");
        // conky не запускается; проба версии (`conky -v`, файл запуска через
        // XWayland) — не запуск.
        assert!(io.ran.borrow().iter().all(|a| a.first().map(String::as_str) != Some("conky") || a[1..] == ["-v"]));

        // Повтор ничего не делает.
        assert!(heal_conky_duplicates(&io, &ctx, &exts).is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Копия, чьи пути уже поправила починка перед запуском
    /// (`de_apply::conky_repair_paths`), — всё ещё своя; у набора виджетов
    /// подпапки — сами виджеты, их уборка не трогает; без основного
    /// каталога вторая копия остаётся.
    #[test]
    fn conky_heal_repaired_copy_sets_and_missing_main() {
        let home = scratch("conky-heal-2");
        let exts = home.join(".config/rexon/extensions");
        let pkg = conky_package(&exts, &["MyTheme"]);
        put(&pkg.join("data/MyTheme/conky.conf"), "lua_load = '~/.conky/MyTheme/lua/rings.lua'\n");
        let root = home.join(".config/conky");
        let dup = root.join("MyTheme");
        copy_sub(&pkg, "MyTheme", &dup, "my-theme");
        let ctx = conky_ctx(&home);
        let io = NoProcIo { ran: Default::default() };

        // Основного каталога нет — копия единственная, остаётся.
        assert!(heal_conky_duplicates(&io, &ctx, &exts).is_empty());
        assert!(dup.is_dir());

        let mut undo = crate::install_plan::Undo::off();
        crate::install_plan::copy_tree(&pkg.join("data"), &root.join("my-theme"), &mut undo).unwrap();
        let changed = crate::de_apply::conky_repair_paths(&ctx.home, &s(&dup));
        assert_eq!(changed.len(), 1, "починка ничего не поправила");

        // Набор виджетов: подпапки — его виджеты.
        let manifest = pkg.join("manifest.json");
        put(&manifest, r#"{"id": "my-theme", "type": "theme-conky", "conky_set": {"widgets": []}}"#);
        assert!(heal_conky_duplicates(&io, &ctx, &exts).is_empty());
        assert!(dup.is_dir());

        put(&manifest, r#"{"id": "my-theme", "type": "theme-conky"}"#);
        assert_eq!(heal_conky_duplicates(&io, &ctx, &exts), vec![s(&dup)]);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Обычный пакет с двумя вложенными виджетами (`data/A`, `data/B`,
    /// своего файла настроек в `data/` нет): файлы — одним каталогом
    /// `<корень>/my-theme`, но виджетов два — две плитки `my-theme/A` и
    /// `my-theme/B`, у каждой свой файл настроек, автозапуск, удаление;
    /// уборка переносит автозапуск прежней копии `<корень>/A` на `my-theme/A`.
    #[test]
    fn conky_package_with_two_widgets_is_two_tiles() {
        use crate::de_apply as da;
        let home = scratch("conky-two");
        let exts = home.join(".config/rexon/extensions");
        let pkg = conky_package(&exts, &["A", "B"]);
        put(&pkg.join("data/fonts/x.ttf"), "font");
        let root = home.join(".config/conky");
        let main = root.join("my-theme");
        let mut undo = crate::install_plan::Undo::off();
        crate::install_plan::copy_tree(&pkg.join("data"), &main, &mut undo).unwrap();
        let ctx = conky_ctx(&home);
        let io = NoProcIo { ran: Default::default() };

        assert_eq!(da::conky_nested_widgets(&io, &ctx, "my-theme"), vec!["my-theme/A", "my-theme/B"]);
        let top = crate::models::ThemeItem {
            id: "local:conky:my-theme".into(),
            name: "my-theme".into(),
            install_path: s(&main),
            ..Default::default()
        };
        let tiles = crate::conky_scan::split_package_items(&io, &ctx, vec![top], &["my-theme/B".to_string()]);
        let ids: Vec<&str> = tiles.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["local:conky:my-theme/A", "local:conky:my-theme/B"]);
        assert_eq!(tiles[0].name, "A");
        assert_eq!(tiles[0].install_path, s(&main.join("A")));
        assert_eq!((tiles[0].local_kind.as_deref(), tiles[1].local_kind.as_deref()), (None, Some("active")));

        let conf_a = s(&main.join("A/conky.conf"));
        assert_eq!(da::conky_conf(&io, &ctx, "my-theme/A").as_deref(), Some(conf_a.as_str()));
        assert_eq!(da::conky_widget_of_conf(&io, &ctx, &conf_a).as_deref(), Some("my-theme/A"));
        assert_eq!(da::conky_widget_of_conf(&io, &ctx, &s(&main.join("B/lua/rings.lua"))).as_deref(), Some("my-theme/B"));
        assert_ne!(da::conky_autostart_path(&ctx, "my-theme/A"), da::conky_autostart_path(&ctx, "my-theme/B"));

        // Прежняя вторая копия `<корень>/A` с автозапуском — уходит, автозапуск
        // переходит на `my-theme/A`.
        copy_sub(&pkg, "A", &root.join("A"), "my-theme");
        let old = da::conky_autostart_path(&ctx, "A");
        put(Path::new(&old), "[Desktop Entry]\nExec=conky -c x\n");
        assert_eq!(heal_conky_duplicates(&io, &ctx, &exts), vec![s(&root.join("A"))]);
        assert!(!Path::new(&old).exists());
        assert!(da::conky_autostart(&io, &ctx, "my-theme/A"));
        assert!(!da::conky_autostart(&io, &ctx, "my-theme/B"));
        let entry = std::fs::read_to_string(da::conky_autostart_path(&ctx, "my-theme/A")).unwrap();
        assert!(entry.contains(&conf_a), "{entry}");

        // Свой автозапуск у B, затем «Удалить» плитки A: снимается только A.
        assert!(da::set_conky_autostart(&io, &ctx, "my-theme/B", true).0);
        let roots = crate::theme_removal::RemovalRoots::new(&s(&home), "/usr");
        let a_dir = s(&main.join("A"));
        assert_eq!(
            roots.verdict(&a_dir, false, &Map::new(), &std::collections::HashSet::new()),
            crate::theme_removal::Verdict::Local { path: a_dir.clone() }
        );
        assert_eq!(crate::conky_scan::own_widget_name(&ctx, &a_dir).as_deref(), Some("my-theme/A"));
        std::fs::remove_dir_all(&a_dir).unwrap();
        assert_eq!(da::conky_forget_widget(&io, &ctx, &a_dir).as_deref(), Some("my-theme/A"));
        assert!(!da::conky_autostart(&io, &ctx, "my-theme/A"));
        assert!(da::conky_autostart(&io, &ctx, "my-theme/B"));
        let left = crate::conky_scan::split_package_items(
            &io,
            &ctx,
            vec![crate::models::ThemeItem { id: "local:conky:my-theme".into(), ..Default::default() }],
            &[],
        );
        assert_eq!(left.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["local:conky:my-theme/B"]);

        // Удаление пакета снимает автозапуск и вложенного виджета.
        assert_eq!(da::conky_forget_widget(&io, &ctx, &s(&main)).as_deref(), Some("my-theme"));
        assert!(!da::conky_autostart(&io, &ctx, "my-theme/B"));
        assert!(!root.join("A").exists() && !root.join("B").exists(), "копии в корне Conky");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Пакет с файлом настроек прямо в `data/` (и подпапками рядом) — один
    /// виджет `<id>`; с одной подпапкой-виджетом — тоже один.
    #[test]
    fn conky_package_with_own_conf_is_one_tile() {
        let home = scratch("conky-one");
        let exts = home.join(".config/rexon/extensions");
        let ctx = conky_ctx(&home);
        let io = NoProcIo { ran: Default::default() };
        let pkg = conky_package(&exts, &["A", "B"]);
        put(&pkg.join("data/conky.conf"), "conky.text = [[top]]\n");
        assert!(crate::de_apply::conky_nested_widgets(&io, &ctx, "my-theme").is_empty());
        std::fs::remove_dir_all(&pkg).unwrap();
        conky_package(&exts, &["A"]);
        assert!(crate::de_apply::conky_nested_widgets(&io, &ctx, "my-theme").is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }
}
