//! Настройки приложения — порт `rexon/settings.py`.
//!
//! JSON-стор `~/.config/rexon/settings.json`: дефолты, слияние с файлом,
//! get/set, детект файлового менеджера. Сериализация побайтово совпадает
//! с Python `json.dump(..., indent=2, ensure_ascii=False)` — см.
//! golden-тест `tests/settings_parity.rs`.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// Дефолты в ТОМ ЖЕ порядке, что `_DEFAULTS` в settings.py — порядок
/// ключей виден в сохранённом файле, поэтому важен для сверки.
pub fn defaults() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("file_manager".into(), Value::from("auto"));
    m.insert("theme".into(), Value::from("auto"));
    m.insert("auto_backup".into(), Value::from(true));
    m.insert("wizard_done".into(), Value::from(false));
    m.insert("show_console_window".into(), Value::from(true));
    m.insert("ui_skin".into(), Value::from("steam_dark"));
    m
}

/// Файловые менеджеры в порядке приоритета (как в settings.py).
const FILE_MANAGERS: [&str; 6] = ["nemo", "nautilus", "thunar", "caja", "dolphin", "pcmanfm"];

/// Слить дефолты с содержимым файла: `dict(_DEFAULTS)` затем `.update(json)`.
/// insert по существующему ключу сохраняет его позицию (как dict.update),
/// новые ключи дописываются в конец. Любая ошибка чтения/парса → дефолты
/// (Python логирует и возвращает дефолты — здесь просто дефолты).
pub fn load_settings(config_file: &Path) -> Map<String, Value> {
    let mut result = defaults();
    if let Ok(text) = std::fs::read_to_string(config_file) {
        if let Ok(Value::Object(loaded)) = serde_json::from_str::<Value>(&text) {
            for (k, v) in loaded {
                result.insert(k, v);
            }
        }
    }
    result
}

/// Сериализация настроек в строку — байт-в-байт как `json.dump(indent=2,
/// ensure_ascii=False)`: 2 пробела отступа, UTF-8 без экранирования,
/// без хвостового перевода строки.
pub fn serialize_settings(settings: &Map<String, Value>) -> String {
    serde_json::to_string_pretty(&Value::Object(settings.clone()))
        .expect("serde_json Map всегда сериализуется")
}

/// Менеджер настроек — аналог `SettingsManager`.
pub struct SettingsManager {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub settings: Map<String, Value>,
}

impl SettingsManager {
    /// Создать: `$XDG_CONFIG_HOME/rexon/settings.json` (по умолчанию
    /// `~/.config/rexon`), создать каталог, загрузить.
    pub fn new() -> std::io::Result<Self> {
        let config_dir = crate::utils::rexon_config_dir();
        let config_file = config_dir.join("settings.json");
        std::fs::create_dir_all(&config_dir)?;
        let settings = load_settings(&config_file);
        Ok(Self { config_dir, config_file, settings })
    }

    /// Сохранение через общую дверь — [`crate::utils::atomic_write`].
    ///
    /// Здесь лежат ВСЕ настройки покупателя, и второго шанса у них нет:
    /// обрезанный `settings.json` читается как пустой, то есть всё, что
    /// человек когда-либо настроил, откатывается к значениям по умолчанию.
    pub fn save_settings(&self) -> std::io::Result<()> {
        let data = serialize_settings(&self.settings);
        crate::utils::atomic_write(&self.config_file.to_string_lossy(), &data)
    }

    /// Получить значение (None → отсутствует).
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.settings.get(key)
    }

    /// Установить значение и сохранить.
    pub fn set(&mut self, key: &str, value: Value) -> std::io::Result<()> {
        self.settings.insert(key.to_string(), value);
        self.save_settings()
    }

    /// Текущий файловый менеджер (автодетект если "auto" или отсутствует).
    pub fn get_file_manager(&self) -> String {
        match self.settings.get("file_manager").and_then(Value::as_str) {
            Some("auto") | None => detect_file_manager(&command_exists),
            Some(fm) => fm.to_string(),
        }
    }
}

/// Первый установленный файловый менеджер, иначе "nautilus". Предикат
/// вынесен параметром — для тестов без зависимости от системного PATH.
pub fn detect_file_manager(exists: &dyn Fn(&str) -> bool) -> String {
    for fm in FILE_MANAGERS {
        if exists(fm) {
            return fm.to_string();
        }
    }
    "nautilus".to_string()
}

/// Есть ли исполняемый файл `cmd` в одном из каталогов PATH.
fn command_exists(cmd: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|dir| dir.join(cmd).is_file())
}

/// Путь к `settings.json` без создания менеджера.
///
/// Нужен тем, кто настройки только ЧИТАЕТ и не вправе трогать каталог: хост
/// обоев следит за этим файлом, чтобы подхватывать правки живьём, и создавать
/// конфиг за окно настроек ему нечего.
pub fn settings_file() -> PathBuf {
    crate::utils::rexon_config_dir().join("settings.json")
}
