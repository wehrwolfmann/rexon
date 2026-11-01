// SteamActions — объект побочных действий вкладки «Steam» на Rust.
//
// ЗАЧЕМ. `SteamPage.qml` ждёт свойством `actions` объект с восемью методами:
// снимок витрины (`scan`), окно выбора своего ролика (`pickFile`), выбор
// (`select`), звёздочка (`setFavorite`), три переключателя (`setSound`,
// `setRotation`, `setAutostart`) и полноэкранный показ (`play`). Пока такого
// объекта на Rust не было, раздел «Steam» под Rust-хозяином окна открывался
// живым НА ВИД: витрина пуста, кнопки нажимаются и ничего не делают. Здесь
// объект появляется, и раздел начинает работать.
//
// ⚠️ Девятого метода, `webapiToken()`, с 13.10.2026 нет. Он ходил в сеть из
// потока интерфейса, а с 08.09.2026 вдобавок не был виден из QML (потерян
// `#[qinvokable]`), так что страница молча жила без токена и без
// анимированных фонов. Токен добывает фоновая задача инвентаря
// (`steam_workers.rs`, `start_inventory`).
//
// Договор со страницей взят у Python-класса `SteamActions`
// (`rexon/qt_steam/qml_window.py`) БУКВА В БУКВУ — те же имена, те же типы
// ответов. Разметка страницы поэтому не изменилась ни на строку.
//
// ГДЕ ЖИВЁТ ЛОГИКА. Своей здесь нет вовсе:
//   * ЧТО делать с заставкой — поток `rexon_core::session_splash::Splash`
//     (скан роликов Steam, конфиг, избранное, ротация, гибрид KSplash,
//     автозапуск, все три пути прогона);
//   * ДЕЛАЕТ это исполнитель `rexon_core::splash_exec::LiveSplashIo` — тот
//     самый, которым работает и живой Python-фасад;
//   * ЧТО сказать человеку — тексты ядра (`rexon_core::qt_steam`), те же,
//     что показывает виджетная вкладка;
//
// ⚠️ ЧТО ПРИШЛОСЬ ПЕРЕДВИНУТЬ РАДИ ЭТОГО ОБЪЕКТА (обе правки — не побочный
// эффект, а условие того, чтобы второй копии правил не появилось):
//   1. Живой исполнитель заставки переехал из `rexon-py` в ядро
//      (`splash_exec.rs`). В `rexon-py` он был недосягаем: тот крейт
//      собирается только как `cdylib` для Python.
//   2. Обмен cookie клиента Steam на `webapi_token` дописан в ядро
//      (`steam_token.rs`). Он один оставался на `urllib` Python, хотя
//      сетевой слой ядра давно есть и вкладка ходит им же за инвентарём.
//
// ⚠️ ВСЕ МЕТОДЫ СИНХРОННЫ — как и у Python-оригинала. Каждый из них либо
// читает готовый снимок, либо пишет один файл, и ответ странице нужен
// немедленно; долгие работы (сеть, миниатюры, кадры-превью) вкладка ведёт
// сама задачами tokio (`steam_workers.rs`). Фоновость, как у «Расширений»,
// здесь была бы не улучшением, а разрывом договора: страница читает ответ
// той же строкой, которой позвала.

/// Мост cxx-qt объекта действий.
///
/// Тип объявлен `#[qml_element]`, поэтому хозяин окна заводит его сам
/// (`SteamActions { }`) и подставляет странице свойством `actions`.
/// Регистрации в `main_window.rs` для этого не требуется: там карта
/// разделов, а не список побочных объектов.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type SteamActions = super::SteamActionsRust;
    }

    extern "RustQt" {
        /// Витрина заставок и состояние переключателей ОДНИМ снимком:
        /// `{movies, current, favorites, enabled, rotate, sound}`.
        ///
        /// Пустой снимок — не поломка: у машины без Steam роликов нет, и
        /// страница честно скажет это словами ядра.
        #[qinvokable]
        fn scan(self: &SteamActions) -> QVariant;

        /// Окно выбора своего видео. Пустая строка — человек передумал.
        #[qinvokable]
        #[cxx_name = "pickFile"]
        fn pick_file(self: &SteamActions) -> QString;

        /// Запомнить выбранный ролик в конфиге заставки.
        #[qinvokable]
        fn select(self: &SteamActions, path: &QString) -> bool;

        /// Переключить звёздочку. Отдаёт АКТУАЛЬНЫЙ список избранного, а не
        /// «получилось ли»: страница держит избранное у себя, и два вызова
        /// подряд («переключи», «а теперь прочитай») разошлись бы с диском.
        #[qinvokable]
        #[cxx_name = "setFavorite"]
        fn set_favorite(self: &SteamActions, path: &QString) -> QVariant;

        /// Проиграть заставку полноэкранно. «» — пошло, иначе причина.
        #[qinvokable]
        fn play(self: &SteamActions, path: &QString, sound: bool) -> QString;

        /// Где стоит Steam — тот же ответ, что модель находит себе
        /// сама. Наружу отдан ради сверки: расхождение значит, что
        /// страница осталась без корня и секция фонов будет пуста.
        #[qinvokable]
        #[cxx_name = "steamRoot"]
        fn steam_root(self: &SteamActions) -> QString;

        /// Автозапуск заставки при входе. «» — получилось, иначе причина.
        #[qinvokable]
        #[cxx_name = "setAutostart"]
        fn set_autostart(self: &SteamActions, on: bool) -> QString;

        /// Случайная заставка при каждом входе.
        #[qinvokable]
        #[cxx_name = "setRotation"]
        fn set_rotation(self: &SteamActions, on: bool) -> bool;

        /// Звук у заставки. Пишется в конфиг вместе с выбранным роликом.
        #[qinvokable]
        #[cxx_name = "setSound"]
        fn set_sound(self: &SteamActions, on: bool) -> bool;
    }
}

use cxx_qt_lib::{QString, QVariant};
use serde_json::{Map, Value};

use rexon_core::i18n::tr;
use rexon_core::qt_steam as qs;
use rexon_core::session_splash::{self as ss, Splash};
use rexon_core::splash_exec::LiveSplashIo;

/// Rust-состояние объекта: пусто, и это не упущение.
///
/// Исполнитель заставки заводится НА ОДИН вызов — так же, как это делает
/// Python-фасад (`session_splash._io()`). Держать его полем нельзя: внутри
/// лежат дескрипторы своих процессов и X11-слоёв, а объект действий живёт
/// столько же, сколько окно.
#[derive(Default)]
pub struct SteamActionsRust;

impl qobject::SteamActions {
    fn steam_root(&self) -> QString {
        let mut io = live_io();
        let home = home();
        QString::from(
            &Splash::new(&mut io, &home)
                .steam_root()
                .unwrap_or_default(),
        )
    }

    fn scan(&self) -> QVariant {
        let mut io = live_io();
        let mut splash = Splash::new(&mut io, &home());

        let cfg = splash.load_config();
        let current = cfg
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let mut movies = splash.scan_startup_movies();
        // Надо ли доклеивать СВОЙ ролик — решает ядро; отсюда приходит только
        // наблюдение «файл на диске есть». Иначе выбор покупателя пропадал бы
        // из виду, не попав в скан Steam.
        let scanned: Vec<String> = movies
            .iter()
            .map(|m| m.get("path").and_then(Value::as_str).unwrap_or("").to_string())
            .collect();
        let exists = !current.is_empty() && std::path::Path::new(&current).is_file();
        if qs::needs_custom_movie(&current, &scanned, exists) {
            let (basename, stem) = name_and_stem(&current);
            movies.push(ss::describe_custom(&current, &basename, &stem));
        }

        let favorites = splash.favorites();
        let enabled = splash.is_enabled();
        let rotate = splash.rotation_enabled();
        let sound = cfg
            .get("sound")
            .map(|v| v.as_bool().unwrap_or(true))
            .unwrap_or(true);

        crate::qvalue::json_to_variant(&snapshot(
            movies, &current, &favorites, enabled, rotate, sound,
        ))
    }

    fn pick_file(&self) -> QString {
        // Заголовок и фильтр — из ядра: те же, что показывала виджетная
        // вкладка. Начальный каталог — корень Steam, а если его нет, дом.
        let mut io = live_io();
        let start = Splash::new(&mut io, &home())
            .steam_root()
            .unwrap_or_else(home);
        // Диалог поднимает ОБЩИЙ шим крейта (`cpp/rexon_ext_dialogs.h`):
        // синхронный ответ, вложенный цикл событий, портал рабочего стола на
        // Wayland. Второго объявления той же C++-функции в этом мосте нет
        // намеренно — cxx печатает шим по имени функции, и дубль дал бы
        // `duplicate symbol` на линковке.
        crate::extensions_actions::qobject::pick_file(
            &QString::from(&qs::sv_pick_dialog_title()),
            &QString::from(&qs::sv_pick_filter()),
            &QString::from(&start),
        )
    }

    fn select(&self, path: &QString) -> bool {
        let path = path.to_string();
        if path.is_empty() {
            return false;
        }
        let mut io = live_io();
        let mut splash = Splash::new(&mut io, &home());
        // Звук берётся из конфига, а не выдумывается: страница переключает
        // его отдельным действием, и своё умолчание тихо сбрасывало бы выбор
        // покупателя.
        let sound = splash
            .load_config()
            .get("sound")
            .map(|v| v.as_bool().unwrap_or(true))
            .unwrap_or(true);
        splash.save_config(&path, sound).is_none()
    }

    fn set_favorite(&self, path: &QString) -> QVariant {
        let path = path.to_string();
        let mut io = live_io();
        let mut splash = Splash::new(&mut io, &home());
        // ⚠️ Починка при переносе (правило #16): пустой путь отдаёт то, что
        // ЛЕЖИТ НА ДИСКЕ, а не пустой список, как делал Python. Список
        // уезжает прямо в `page.setFavorites(...)`, и пустой ответ стирал бы
        // все звёздочки разом — на экране это неотличимо от «человек их снял».
        if path.is_empty() {
            return crate::qvalue::json_to_variant(&strings_to_json(&splash.favorites()));
        }
        let current = splash.favorites();
        let on = !current.iter().any(|p| p == &path);
        let (favorites, err) = splash.set_favorite(&path, on);
        // ⚠️ Вторая починка того же рода: конфиг не записался — отдаём
        // ПРЕЖНИЙ список. Ядро в этом случае возвращает желаемый, и звёздочка
        // загоралась бы на экране, не пережив перезапуск.
        let favorites = if err.is_some() { current } else { favorites };
        crate::qvalue::json_to_variant(&strings_to_json(&favorites))
    }

    fn play(&self, path: &QString, sound: bool) -> QString {
        let path = path.to_string();
        if path.is_empty() {
            return QString::from(&tr(qs::SV_TEST_NO_ITEM));
        }
        let mut io = live_io();
        // `blocking=false` — управление возвращается сразу: это проверка из
        // интерфейса, а не вход в систему. `hybrid=false` — эстафета
        // ksplash→mpv тут ни при чём, как и у виджетной вкладки.
        let (ok, msg) = Splash::new(&mut io, &home()).play(
            Some(&path),
            Some(sound),
            false,
            Some(false),
            &repo_root(),
        );
        QString::from(&reason(ok, msg))
    }

    fn set_autostart(&self, on: bool) -> QString {
        let mut io = live_io();
        let mut splash = Splash::new(&mut io, &home());
        let (ok, msg) = if on {
            splash.install_autostart()
        } else {
            splash.uninstall_autostart()
        };
        QString::from(&reason(ok, msg))
    }

    fn set_rotation(&self, on: bool) -> bool {
        let mut io = live_io();
        Splash::new(&mut io, &home()).set_rotation(on).is_none()
    }

    fn set_sound(&self, on: bool) -> bool {
        let mut io = live_io();
        let mut splash = Splash::new(&mut io, &home());
        let path = splash
            .load_config()
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if path.is_empty() {
            // Ролик ещё не выбран — записывать звук некуда; выбор сохранит
            // его вместе с путём.
            return false;
        }
        splash.save_config(&path, on).is_none()
    }
}

// ============================================================================
// Наблюдения, которых у ядра нет
// ============================================================================

/// Домашний каталог (`os.path.expanduser("~")` оригинала).
fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

/// Живой исполнитель заставки — на ОДИН вызов, как у Python-фасада.
///
/// ⛔ Интерпретатор НЕ передаётся и не подразумевается: с 16.09.2026 (блок
/// H) ни один путь заставки его не зовёт. Строку автозапуска и
/// `ExecStart=` юнита собирает `rexon_core::app::launch_program` — бинарь
/// окна при Rust-входе, лаунчер `rexon` полным путём при откате, и НИЧЕГО
/// (отказ вслух), если не нашлось ни того, ни другого. Второй аргумент
/// конструктора остался только ради подписи, которую держит мост `rexon-py`
/// (`splash_io::PyLiveSplashIo`); уйдёт вместе с мостом.
fn live_io() -> LiveSplashIo {
    LiveSplashIo::new(&home(), "")
}

/// Корень дерева разработки: там ядро ищет собранный `rexon-splash-overlay`.
///
/// ⚠️ Нужен ровно одному пути прогона — блокирующему Wayland-показу при входе,
/// которого из вкладки не бывает (`play` зовётся с `blocking=false`). Величина
/// не выдумывается и не выводится здесь: правило «что считать деревом
/// разработчика» одно на весь продукт и живёт в ядре (правило #12). Не
/// вывелось — пустая строка, и ядро уйдёт на поиск по `PATH`, как оно и умеет.
fn repo_root() -> String {
    rexon_core::cli::repo_root()
}

// ============================================================================
// Мелочи перевода
// ============================================================================

/// `(имя файла, имя без расширения)` — то, что Python считал
/// `os.path.basename` и `os.path.splitext` перед `describe_custom`.
fn name_and_stem(path: &str) -> (String, String) {
    let p = std::path::Path::new(path);
    let basename = p
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    (basename, stem)
}

/// Ответ действия, которое либо получилось, либо назвало причину.
///
/// Пустая строка — успех. Отказ без слов подменяется общей причиной ядра:
/// молчаливая кнопка читается как поломка (правило #13).
fn reason(ok: bool, message: String) -> String {
    if ok {
        String::new()
    } else if message.is_empty() {
        qs::sv_no_reason()
    } else {
        message
    }
}

fn strings_to_json(items: &[String]) -> Value {
    Value::Array(items.iter().map(|s| Value::from(s.as_str())).collect())
}

/// Ключи снимка, которые страница читает точкой (`snapshot.movies`,
/// `snapshot.rotate`…). Список назван отдельно не для красоты: забытый ключ
/// приезжает в QML как `undefined`, а `undefined` в переключателе — это
/// молчаливое «выключено». Такой разъезд не виден ни на сборке, ни глазом.
const SNAPSHOT_KEYS: [&str; 6] = ["movies", "current", "favorites", "enabled", "rotate", "sound"];

/// Собрать снимок витрины и переключателей — единственное место, где состав
/// ответа `scan()` записан.
fn snapshot(
    movies: Vec<Value>,
    current: &str,
    favorites: &[String],
    enabled: bool,
    rotate: bool,
    sound: bool,
) -> Value {
    // Карта собирается ПО СПИСКУ ключей, а не россыпью вставок: так состав
    // ответа записан ровно один раз, и сторож ниже проверяет тот самый
    // список, которым страница и наполняется.
    let values = [
        Value::Array(movies),
        Value::from(current),
        strings_to_json(favorites),
        Value::from(enabled),
        Value::from(rotate),
        Value::from(sound),
    ];
    let mut out = Map::new();
    for (key, value) in SNAPSHOT_KEYS.into_iter().zip(values) {
        out.insert(key.to_string(), value);
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Договор со страницей: отказ ВСЕГДА со словами. Пустую строку она
    /// понимает как «получилось» и пишет в состояние «▶ Проигрываю…» —
    /// то есть безмолвный отказ показал бы человеку успех.
    #[test]
    fn refusal_is_never_wordless() {
        assert_eq!(reason(true, String::new()), "");
        assert_eq!(reason(true, "лишнее".into()), "");
        assert_eq!(reason(false, String::new()), qs::sv_no_reason());
        assert_eq!(reason(false, "нет файла".into()), "нет файла");
    }

    /// Своё имя ролика для витрины считается по ФАЙЛУ, а не по всему пути:
    /// карточка подписана `title`, и путь целиком в неё не влезал бы.
    #[test]
    fn custom_movie_is_named_by_its_file() {
        let (basename, stem) = name_and_stem("/дом/видео/Мой ролик.webm");
        assert_eq!(basename, "Мой ролик.webm");
        assert_eq!(stem, "Мой ролик");
        let (basename, stem) = name_and_stem("");
        assert_eq!(basename, "");
        assert_eq!(stem, "");
    }

    /// Снимок обязан назвать ВСЕ шесть ключей — и на пустой машине тоже.
    /// Страница читает их точкой, и забытый ключ приехал бы в неё как
    /// `undefined`, то есть переключатель молча встал бы в «выключено».
    ///
    /// ⚠️ Имена здесь выписаны ЗАНОВО, а не взяты из `SNAPSHOT_KEYS`. Сверка
    /// со списком, который сама же и проверяет, зеленела бы на любом его
    /// переименовании — то есть не проверяла бы ничего. Настоящий источник
    /// этих шести слов — `qml/SteamPage.qml` (`snapshot.movies`,
    /// `snapshot.rotate`…), и переписать их можно только вместе с ним.
    #[test]
    fn snapshot_names_every_key_the_page_reads() {
        let wanted = ["movies", "current", "favorites", "enabled", "rotate", "sound"];
        let empty = snapshot(Vec::new(), "", &[], false, false, true);
        let map = empty.as_object().expect("снимок обязан быть объектом");
        for key in wanted {
            assert!(map.contains_key(key), "в снимке нет ключа «{key}»");
        }
        assert_eq!(map.len(), wanted.len(), "в снимке появился лишний ключ");
    }

    /// Типы полей снимка держат разметку: `movies` и `favorites` — списки,
    /// три переключателя — истина/ложь. Приедь сюда строка «false», и QML
    /// принял бы её за истину (непустая строка), а переключатель встал бы во
    /// «включено» сам собой.
    #[test]
    fn snapshot_field_types_are_what_qml_expects() {
        let full = snapshot(
            vec![Value::Object(Map::new())],
            "/дом/ролик.webm",
            &["/дом/ролик.webm".to_string()],
            true,
            false,
            true,
        );
        assert!(full["movies"].is_array());
        assert!(full["favorites"].is_array());
        assert_eq!(full["current"], Value::from("/дом/ролик.webm"));
        assert!(full["enabled"].is_boolean());
        assert!(full["rotate"].is_boolean());
        assert!(full["sound"].is_boolean());
        assert_eq!(full["favorites"][0], Value::from("/дом/ролик.webm"));
    }
}
