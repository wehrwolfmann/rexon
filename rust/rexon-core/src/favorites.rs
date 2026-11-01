//! Избранные темы оформления — порт `rexon/appearance/favorites.py`.
//!
//! Хранилище `~/.config/rexon/favorites.json` — формат `{id: {ThemeItem-поля}}`
//! (не только id, чтобы показывать карточки без повторного запроса к OCS).
//! Ключ — `ThemeItem.id` (OCS content_id). Пишется `json.dump(ensure_ascii=
//! False, indent=1)` → сверяется побайтово (golden-тест `favorites_parity.rs`).
//!
//! Функции ядра чистые (работают над переданным стором/путём) — как в
//! `settings.rs`; тонкие обёртки берут реальный путь через `default_path()`.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::models::{to_json_indent1, ThemeItem};

/// Путь к стору избранного: `~/.config/rexon/favorites.json`.
///
/// ⚠️ Починка: раньше путь собирался из голого `$HOME`, минуя
/// `XDG_CONFIG_HOME`. Интерфейс при этом читает XDG-осведомлённый путь
/// (`rexon/appearance/favorites.py` строит его через `expand_user`), и при
/// нестандартном `XDG_CONFIG_HOME` избранное РАЗДВАИВАЛОСЬ: вкладка
/// показывала один файл, ядро писало другой, и звёздочки пропадали на
/// ровном месте. Такое окружение — не экзотика: его задают Flatpak,
/// Steam-runtime и просто аккуратно настроенный профиль. Путь теперь
/// считается тем же способом, что и все остальные пути пользователя, —
/// единой точкой [`crate::utils::rexon_config_dir`].
pub fn default_path() -> PathBuf {
    crate::utils::rexon_config_dir().join("favorites.json")
}

/// Загрузить сырой стор `{id: {…}}`. Любая ошибка чтения/парса или не-объект
/// на верхнем уровне → пустой стор (как `except (OSError, JSONDecodeError)` и
/// проверка `isinstance(data, dict)` в Python). Значения хранятся как есть —
/// Python не нормализует их при загрузке.
pub fn load_raw(path: &Path) -> Map<String, Value> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        },
        Err(_) => Map::new(),
    }
}

/// Сохранить стор: mkdir родителя, затем общая дверь
/// [`crate::utils::atomic_write`] с `indent=1`. Ошибку каталога/записи
/// возвращаем (Python логирует warning и молча продолжает).
pub fn save_raw(path: &Path, store: &Map<String, Value>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let data = to_json_indent1(&Value::Object(store.clone()));
    crate::utils::atomic_write(&path.to_string_lossy(), &data)
}

/// В избранном ли элемент: непустой id, присутствующий в сторе.
pub fn is_favorite(store: &Map<String, Value>, item_id: &str) -> bool {
    !item_id.is_empty() && store.contains_key(item_id)
}

/// Добавить/убрать элемент в сторе (in-place). Возвращает новое состояние
/// (`true` = теперь в избранном). Пустой id — без изменений, `false`.
///
/// Для OCS-тем `preview_local` (путь кеша — может протухнуть) обнуляется,
/// превью перетянется по `preview_url`. Для ЛОКАЛЬНЫХ тем (id `local:…`)
/// `preview_local` — стабильный путь к файлу на диске, а `preview_url` пуст →
/// СОХРАНЯЕМ, иначе локальная тема осталась бы в «Избранном» без превью.
pub fn toggle(store: &mut Map<String, Value>, item: &ThemeItem) -> bool {
    if item.id.is_empty() {
        return false;
    }
    if store.contains_key(&item.id) {
        // ⚠️ Именно `shift_remove`, а НЕ `remove`: при включённой фиче
        // `preserve_order` серде-мапа — IndexMap, и `remove` == `swap_remove`,
        // т.е. на место удалённого переезжает ПОСЛЕДНИЙ элемент. Python
        // `del data[id]` порядок остальных сохраняет, и порядок ключей
        // favorites.json — это порядок карточек в «⭐ Избранном»: со
        // `swap_remove` после снятия одной звезды последняя избранная тема
        // прыгала на её место (и файл расходился с Python побайтово).
        store.shift_remove(&item.id);
        return false;
    }
    let mut entry = serde_json::to_value(item).expect("ThemeItem сериализуется");
    if !item.id.starts_with("local:") {
        if let Value::Object(ref mut obj) = entry {
            obj.insert("preview_local".to_string(), Value::from(""));
        }
    }
    store.insert(item.id.clone(), entry);
    true
}

/// Все избранные как `ThemeItem` (для сетки категории «Избранное»).
///
/// Неизвестные ключи serde отбрасывает, отсутствующие берут дефолт — аналог
/// `ThemeItem(**{k: v … if k in _FIELDS})`. Запись, которую нельзя разобрать в
/// `ThemeItem`, пропускается. Для локальных тем (id `local:…`) восстанавливаем
/// рантайм-атрибут `local_kind = "downloaded"` (он не сериализуется), чтобы
/// карточка показывалась как локальная, а не как OCS-товар.
pub fn list_items(store: &Map<String, Value>) -> Vec<ThemeItem> {
    let mut out = Vec::new();
    for value in store.values() {
        let Ok(mut item) = serde_json::from_value::<ThemeItem>(value.clone()) else {
            continue;
        };
        if item.id.starts_with("local:") {
            item.local_kind = Some("downloaded".to_string());
        }
        out.push(item);
    }
    out
}
