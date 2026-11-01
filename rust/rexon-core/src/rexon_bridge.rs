//! Мост Rexon ↔ внешние приложения — порт `rexon/rexon_bridge.py`.
//!
//! Двусторонний обмен состоянием между Rexon и BonziBuddy (и другими)
//! через общий JSON-файл `~/.config/rexon/bridge.json` плюс файловые
//! команды/события в `~/.config/rexon/bridge_commands/`. Оригинал —
//! самодостаточный модуль (чистый Python + JSON, без Qt), поэтому
//! портируется почти целиком: данные DE-реакций, построение сообщений,
//! слияние секций, геттеры состояния, опрос и очистка команд.
//!
//! I/O-границы (остаются вызывающему/шиму):
//! - текущее время — параметрами `now_iso` (ISO-строка `datetime.now()
//!   .isoformat()`) и `epoch_ms`/`now_epoch` (`time.time()`);
//! - проверка живости процесса (`os.kill(pid, 0)`) — замыканием
//!   `pid_alive` в [`app_running`];
//! - межпроцессная блокировка `fcntl.flock` из `_update` — вне ядра
//!   (шим держит `.lock`-файл вокруг [`RexonBridge::update_section`];
//!   атомарность самой записи обеспечена tmp + rename);
//! - CLI-утилита `main()` (argparse + random) не портируется — это
//!   отладочный симулятор, не логика моста.
//!
//! Файловые операции над bridge.json/командами выполняются напрямую
//! (как в `favorites.rs`) — это локальные файлы, не сокеты и не демон.
//!
//! Формат записи: `json.dump(indent=2, ensure_ascii=False)` — сверяется
//! побайтово (golden-тест `rexon_bridge_parity.rs`).

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

// ============================================================================
// Пути
// ============================================================================

/// Каталог моста (модульная константа `BRIDGE_DIR`) — папка настроек Rexon.
///
/// ⚠️ Починка: раньше путь склеивался литералом `~/.config/rexon` от голого
/// `$HOME`, мимо `$XDG_CONFIG_HOME`. Мост — межпроцессный: BonziBuddy и
/// прочие читают `bridge.json` по своему пути, а окно настроек Rexon,
/// избранное и расширения живут в XDG-осведомлённой папке. На машине с
/// нестандартным `XDG_CONFIG_HOME` мост уезжал в ДРУГУЮ папку: Rexon писал
/// состояние туда, где его никто не ищет, файловые команды из
/// `bridge_commands/` не доходили, а удаление Rexon оставляло эту папку на
/// диске. Путь считается [`crate::utils::rexon_config_dir`] — той же
/// единой точкой, что и все остальные хранилища Rexon в конфигах.
pub fn bridge_dir() -> PathBuf {
    crate::utils::rexon_config_dir()
}

/// `~/.config/rexon/bridge.json` (`BRIDGE_FILE`).
pub fn default_bridge_file() -> PathBuf {
    bridge_dir().join("bridge.json")
}

/// `~/.config/rexon/bridge_commands` (`COMMANDS_DIR`).
pub fn default_commands_dir() -> PathBuf {
    bridge_dir().join("bridge_commands")
}

/// Лимит размера файла команды: 1 МиБ — защита от DoS (`MAX_CMD_SIZE`).
pub const MAX_CMD_SIZE: u64 = 1024 * 1024;

// ============================================================================
// DE-реакции для BonziBuddy
// ============================================================================

/// Наборы реплик для одного DE (ключи словаря `DE_REACTIONS[de]`).
pub struct DeReactions {
    pub greetings: &'static [&'static str],
    pub jokes: &'static [&'static str],
    pub on_switch_to: &'static [&'static str],
    pub on_switch_from: &'static [&'static str],
    pub idle_comments: &'static [&'static str],
}

impl DeReactions {
    /// JSON-объект с тем же порядком ключей, что у Python-словаря.
    pub fn to_value(&self) -> Value {
        let mut obj = Map::new();
        for (key, list) in [
            ("greetings", self.greetings),
            ("jokes", self.jokes),
            ("on_switch_to", self.on_switch_to),
            ("on_switch_from", self.on_switch_from),
            ("idle_comments", self.idle_comments),
        ] {
            obj.insert(
                key.to_string(),
                Value::Array(list.iter().map(|s| Value::from(*s)).collect()),
            );
        }
        Value::Object(obj)
    }
}

/// `DE_REACTIONS` — реплики BonziBuddy по DE (порядок как в оригинале).
pub static DE_REACTIONS: &[(&str, DeReactions)] = &[
    (
        "kde",
        DeReactions {
            greetings: &[
                "О, KDE! Тут так красиво, что я забыл зачем пришёл.",
                "KDE? Значит, вы любите всё настраивать. Мы поладим!",
                "Привет! Какой красивый рабочий стол у вас.",
                "KDE! Тут уютно. Прямо как дома.",
            ],
            jokes: &[
                "Тут столько настроек, что даже я запутался!",
                "Красивые окна, красивые кнопки... А я? Я тоже красивый?",
                "Говорят, тут можно настроить вообще всё. Даже меня?",
                "Я тут походил по настройкам... через три часа вернулся.",
            ],
            on_switch_to: &[
                "Ого, новый рабочий стол! Тут красиво!",
                "Переехали! Мне тут нравится, много кнопок и панелей.",
                "О, всё по-новому! Дайте осмотреться...",
            ],
            on_switch_from: &[
                "Уходим? Я только привык к этим красивым окнам...",
                "Ладно, переезжаем. Надеюсь, там тоже будет красиво!",
            ],
            idle_comments: &[
                "Знаете, мне тут нравится. Уютненько.",
                "А эти виджеты на рабочем столе — прикольная штука!",
                "Панель снизу, как в Windows. Почти как дома!",
                "Тут даже обои можно анимированные поставить. Круто!",
            ],
        },
    ),
    (
        "gnome",
        DeReactions {
            greetings: &[
                "О, тут всё так... просто и чисто!",
                "Привет! Минималистичный рабочий стол, мне нравится.",
                "Тут просторно! Никаких лишних кнопок.",
                "Привет! А где тут панель задач? А, понял, тут по-другому...",
            ],
            jokes: &[
                "Тут всё так просто, что даже настройки простые!",
                "Минимализм — это когда меньше кнопок, больше красоты.",
                "Я тут самый яркий элемент на рабочем столе!",
                "Всё серое и стильное. А я фиолетовый. Выделяюсь!",
            ],
            on_switch_to: &[
                "Новый рабочий стол! Тут так чисто и аккуратно.",
                "Переехали! Просторно тут. Есть где погулять.",
                "О, новый дом! Непривычно, но красиво.",
            ],
            on_switch_from: &[
                "Уходим? Я только привык к этой простоте!",
                "Ладно, поехали дальше. Интересно, что там будет.",
            ],
            idle_comments: &[
                "Знаете, иногда простота — это хорошо. Ничего лишнего.",
                "Тут так тихо и спокойно... Не то что на прошлом столе!",
                "А мне нравится, что тут нет миллиона кнопок.",
            ],
        },
    ),
    (
        "mate",
        DeReactions {
            greetings: &[
                "О! Классический рабочий стол! Ностальгия!",
                "Привет! Тут прямо как в старые добрые времена.",
                "Две панели! Сверху и снизу! Как раньше было!",
                "Тут так знакомо... Как будто вернулся в 2005.",
            ],
            jokes: &[
                "Ретро-стиль! Я тоже ретро, так что мы пара!",
                "Классика никогда не выходит из моды. Как и я!",
                "Две панели — значит в два раза больше места для меня!",
                "Этот рабочий стол напоминает мне молодость...",
            ],
            on_switch_to: &[
                "О, классический стол! Как в старые добрые!",
                "Переехали! Тут уютно, как дома у бабушки.",
                "Наконец-то нормальные панели! Сверху и снизу!",
            ],
            on_switch_from: &[
                "Уходим из классики? Ну ладно, но я буду скучать.",
                "Прощай, уютный стол! Было хорошо.",
            ],
            idle_comments: &[
                "Мне тут прямо как дома. Всё на своих местах.",
                "Классический вид — самый понятный. Без сюрпризов!",
                "Тут всё работает. Просто работает. И хорошо!",
            ],
        },
    ),
    (
        "xfce",
        DeReactions {
            greetings: &[
                "Xfce! Быстрый и лёгкий! Мне нравится!",
                "Привет! Тут всё так шустро работает!",
                "О, тут мышка на иконке! Симпатичная.",
                "Лёгкий рабочий стол. Компьютер даже не напрягается!",
            ],
            jokes: &[
                "Тут так быстро всё открывается, что я еле успеваю!",
                "Этот рабочий стол работает даже на слабых компьютерах. Респект!",
                "Лёгкий, быстрый, простой. Три качества, которых мне не хватает.",
                "Тут так мало ресурсов потребляется, что мне стыдно за себя.",
            ],
            on_switch_to: &[
                "Новый стол! Тут так быстро всё! Вжух!",
                "Переехали! Компьютер прямо ожил!",
            ],
            on_switch_from: &["Уходим? Надеюсь, там тоже будет быстро..."],
            idle_comments: &[
                "Быстро, просто, надёжно. Что ещё нужно?",
                "Тут всё открывается мгновенно. Красота!",
                "Простой и удобный стол. Без лишнего.",
            ],
        },
    ),
    (
        "cinnamon",
        DeReactions {
            greetings: &[
                "Привет! Тут очень знакомо... Похоже на Windows!",
                "О, удобный рабочий стол! Меню «Пуск» — то есть «Меню»!",
                "Cinnamon! Тут всё понятно с первого взгляда.",
                "Привет! Красивый и удобный стол. Мне нравится!",
            ],
            jokes: &[
                "Тут как в Windows, только лучше. И бесплатно!",
                "Панель задач снизу, меню слева... Как дома!",
                "Самый дружелюбный рабочий стол для новичков. И для горилл!",
                "Тут даже бабушка разберётся. Проверено!",
            ],
            on_switch_to: &[
                "О, новый стол! Тут уютненько, всё знакомо.",
                "Переехали! Тут прямо как в Windows, только лучше.",
            ],
            on_switch_from: &["Уходим? Но тут было так удобно!"],
            idle_comments: &[
                "Знаете, это самый удобный стол для тех, кто с Windows.",
                "Всё на своих местах. Панель, меню, часы. Красота!",
            ],
        },
    ),
    (
        "pantheon",
        DeReactions {
            greetings: &[
                "Привет! Pantheon — тихо, чисто, ничего лишнего.",
                "О, elementary! Тут даже я стараюсь вести себя прилично.",
            ],
            jokes: &[
                "Пантеон — это же храм! Можно, я буду тут божеством?",
                "Док внизу, панель сверху. Почти как у яблочных, только без яблок.",
            ],
            on_switch_to: &["Новый стол! Какой спокойный!"],
            on_switch_from: &["Уходим из храма? Было уютно..."],
            idle_comments: &[
                "Здесь так тихо, что слышно, как думает процессор.",
                "Минимализм. Мне даже нечего уронить.",
            ],
        },
    ),
    (
        "deepin",
        DeReactions {
            greetings: &[
                "Привет! Deepin — круглые углы и стекло, красота!",
                "О, Deepin! Тут даже окна прозрачные, как мои намерения.",
            ],
            jokes: &[
                "Углы такие круглые, что я боюсь укатиться.",
                "Размытие за окнами — это не я плохо вижу, это так задумано.",
            ],
            on_switch_to: &["Новый стол! Какой гладкий!"],
            on_switch_from: &["Уходим из Deepin? Стекло было красивое..."],
            idle_comments: &[
                "Если долго смотреть в прозрачную панель, кажется, что она смотрит в ответ.",
                "Тут всё такое мягкое, что даже ошибки кажутся вежливыми.",
            ],
        },
    ),
    (
        "budgie",
        DeReactions {
            greetings: &[
                "Привет! Красивый и современный стол!",
                "Budgie! Простой, стильный, без лишнего.",
                "О, тут красиво! Мне нравится этот дизайн.",
            ],
            jokes: &[
                "Budgie — это попугайчик! Привет, коллега-животное!",
                "Стильный стол! Тут и мне стыдно в пижаме ходить.",
                "Простой и красивый. Как я. Ну, красивый точно.",
            ],
            on_switch_to: &["Новый стол! Стильненько тут!"],
            on_switch_from: &["Уходим? Было красиво..."],
            idle_comments: &[
                "Тут приятно находиться. Всё аккуратно.",
                "Современный дизайн. Мне нравится!",
            ],
        },
    ),
    (
        "cosmic",
        DeReactions {
            greetings: &[
                "COSMIC! Это что-то новенькое! Интересно!",
                "О, новый рабочий стол! Выглядит футуристично!",
                "Привет! Тут всё такое... космическое!",
            ],
            jokes: &[
                "Космический рабочий стол! А я — космическая горилла!",
                "Это ещё в разработке, но уже красивее многих готовых!",
                "Будущее наступило! И оно выглядит неплохо.",
            ],
            on_switch_to: &["Переехали в будущее! Круто!", "COSMIC! Мы первопроходцы!"],
            on_switch_from: &["Уходим из будущего? Ну ладно, вернёмся в настоящее."],
            idle_comments: &["Тут ещё не всё готово, но потенциал огромный!"],
        },
    ),
    (
        "hyprland",
        DeReactions {
            greetings: &[
                "Ого, тут окна сами встают на место! Магия!",
                "Привет! Красивый стол с анимациями!",
                "Тут всё автоматически раскладывается. Удобно!",
            ],
            jokes: &[
                "Окна сами раскладываются! Мне бы так в жизни...",
                "Столько анимаций! Тут красивее, чем в кино!",
                "Тут всё на клавиатуре. Мышка отдыхает. А я нет!",
            ],
            on_switch_to: &["Новый стол! Тут окна сами встают на место!"],
            on_switch_from: &["Уходим? Но тут такие красивые анимации были!"],
            idle_comments: &["Тут всё так плавно двигается. Залипательно!"],
        },
    ),
    // ⚠️ Правило #12: список DE тут обязан совпадать с `_DE_DATABASE`
    // (rexon/system.py). LXQt, niri и sway туда добавили, а сюда забыли —
    // BonziBuddy молчал в трёх поддерживаемых окружениях (get_de_reactions
    // отдавал {}, потому что «-» в имени нет и нормализация не спасала).
    (
        "lxqt",
        DeReactions {
            greetings: &[
                "О, LXQt! Скромно и быстро — уважаю.",
                "Привет! Тут всё лёгкое, даже я себя пушинкой чувствую.",
                "LXQt? Значит, цените, когда ничего не тормозит.",
            ],
            jokes: &[
                "Тут так легко, что я чуть не улетел!",
                "Никаких лишних анимаций. Зато я подвигаюсь за всех!",
                "Старый компьютер, а летает. Мне бы так!",
            ],
            on_switch_to: &["Новый стол! И какой быстрый!"],
            on_switch_from: &["Уходим? Тут было так шустро..."],
            idle_comments: &["Тихо, быстро, ничего не мешает. Красота!"],
        },
    ),
    (
        "niri",
        DeReactions {
            greetings: &[
                "Ого, окна выстроились в ленту! Никогда такого не видел.",
                "Привет! Тут стол уезжает в сторону — я слежу за ним глазами.",
                "niri? Значит, вы любите порядок по горизонтали.",
            ],
            jokes: &[
                "Окна в ряд, как вагоны. Я тогда буду паровозом!",
                "Тут стол едет влево-вправо. У меня уже голова кружится!",
                "Прокрутил стол и потерял окно. Оно вернётся, да?",
            ],
            on_switch_to: &["Новый стол! Тут всё выстраивается в ленту!"],
            on_switch_from: &["Уходим? А я только научился прокручивать!"],
            idle_comments: &["Смотрю, как окна уезжают за край. Завораживает."],
        },
    ),
    (
        "sway",
        DeReactions {
            greetings: &[
                "О, sway! Тут окна делят экран без ссор.",
                "Привет! Мышку можно спрятать, тут всё с клавиатуры.",
                "sway? Значит, вы знаете, что делаете. Уважаю!",
            ],
            jokes: &[
                "Тут всё горячими клавишами. Я запомнил три, остальные забыл.",
                "Окна поделили экран по-честному. Мне тоже кусочек!",
                "Я нажал что-то не то и окно уехало. Верните его, пожалуйста.",
            ],
            on_switch_to: &["Новый стол! Тут окна сами делят место!"],
            on_switch_from: &["Уходим? Я только выучил половину клавиш..."],
            idle_comments: &["Ни одного лишнего пикселя. Аккуратно тут у вас."],
        },
    ),
    (
        "river",
        DeReactions {
            greetings: &[
                "О, River! Окна текут, как река, — и каждое на своём месте.",
                "Привет! Тут окна раскладывают по меткам, а не по столам.",
                "River? Тихо, быстро, всё с клавиатуры. Уважаю!",
            ],
            jokes: &[
                "Окно с меткой три, окно с меткой пять... А я с какой меткой?",
                "Река течёт, окна плывут. Я, кажется, тоже поплыл.",
                "Super и цифра — и окно уже тут. Мне бы так с бананами!",
            ],
            on_switch_to: &["Новый стол! Тут окна сами встают по течению!"],
            on_switch_from: &["Уходим? А я только разобрался с метками..."],
            idle_comments: &["Смотрю, как окна делят экран. Спокойно, как у реки."],
        },
    ),
    (
        "wayfire",
        DeReactions {
            greetings: &[
                "Ого, Wayfire! Сейчас стол как закрутится кубом!",
                "Привет! Окна тут качаются, как желе. Красота!",
                "Wayfire? Значит, вы любите эффекты. Я тоже!",
            ],
            jokes: &[
                "Потряс окно — оно качается. Потряс себя — я тоже качаюсь!",
                "Стол-куб! А можно я внутри посижу?",
                "Столько эффектов, что я забыл, зачем пришёл.",
            ],
            on_switch_to: &["Новый стол! Тут окна шевелятся, как живые!"],
            on_switch_from: &["Уходим? А куб я так и не докрутил..."],
            idle_comments: &["Окна плавают, огоньки горят. Уютно тут."],
        },
    ),
];

/// Имя события моста: живые обои пошли. Название сцены — полем `name`, если
/// приславший его знает.
pub const WALLPAPER_STARTED_EVENT: &str = "video_wallpaper_started";
/// Имя события моста: живые обои встали.
pub const WALLPAPER_STOPPED_EVENT: &str = "video_wallpaper_stopped";

/// `EVENT_REACTIONS` — шаблоны реакций на события.
pub static EVENT_REACTIONS: &[(&str, &[&str])] = &[
    (
        "de_installed",
        &[
            "Ого, новый рабочий стол установлен! Хочу посмотреть!",
            "Новый стол? Интересно, как там будет!",
            "Установили новый стол! Можем переключиться и глянуть.",
        ],
    ),
    (
        "de_removed",
        &[
            "Удалили рабочий стол? Прощай, старый друг...",
            "Одним столом меньше. Ну и ладно!",
        ],
    ),
    (
        "theme_changed",
        &[
            "О, новая тема! Красиво!",
            "Сменили оформление? Мне нравится!",
            "Новые цвета! Надеюсь, я в них вписываюсь.",
        ],
    ),
    (
        "extension_installed",
        &[
            "Новое расширение! Что оно делает?",
            "Установили что-то новое? Круто!",
        ],
    ),
    (
        "backup_created",
        &[
            "Бэкап создан! Отлично, подстраховались.",
            "Хороший человек — тот, кто делает бэкапы. Как вы!",
        ],
    ),
    (
        "video_wallpaper_started",
        &[
            "Ооо, живые обои! Красиво двигается!",
            "Живые обои? А мне тоже можно подвигаться?",
        ],
    ),
    (
        "video_wallpaper_stopped",
        &[
            "Обои встали. Опять на стол смотреть скучно.",
            "Всё, картинка больше не двигается. Ну и ладно.",
        ],
    ),
];

/// Реакции для DE с нормализацией `"cinnamon-mint"` → `"cinnamon"` —
/// порт `get_de_reactions` (без ветки чтения current_de из файла: её
/// делает вызывающий через [`get_current_de`]).
pub fn lookup_de_reactions(de: &str) -> Option<&'static DeReactions> {
    if let Some((_, r)) = DE_REACTIONS.iter().find(|(k, _)| *k == de) {
        return Some(r);
    }
    if de.contains('-') {
        let base = de.split('-').next().unwrap_or(de);
        return DE_REACTIONS.iter().find(|(k, _)| *k == base).map(|(_, r)| r);
    }
    None
}

/// То же, но JSON-объектом (Python возвращает `{}` для неизвестного DE).
pub fn de_reactions_value(de: &str) -> Value {
    match lookup_de_reactions(de) {
        Some(r) => r.to_value(),
        None => Value::Object(Map::new()),
    }
}

/// Все реакции всех DE — порт `get_all_de_reactions` (для предзагрузки).
pub fn all_de_reactions_value() -> Value {
    let mut obj = Map::new();
    for (de, r) in DE_REACTIONS {
        obj.insert((*de).to_string(), r.to_value());
    }
    Value::Object(obj)
}

/// Шаблоны реакций на событие — порт `get_event_reactions`
/// (пустой срез, если тип неизвестен).
pub fn event_reactions(event_type: &str) -> &'static [&'static str] {
    EVENT_REACTIONS
        .iter()
        .find(|(k, _)| *k == event_type)
        .map(|(_, v)| *v)
        .unwrap_or(&[])
}

// ============================================================================
// Хелперы семантики Python
// ============================================================================

/// Истинность значения по правилам Python (`bool(x)`).
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `json.dumps(obj, ensure_ascii=False, indent=2)` — формат bridge.json
/// и файлов команд. Локальный аналог `models::to_json_indent1`, но с
/// отступом 2 (дефолт `PrettyFormatter` serde_json).
fn to_json_indent2(value: &Value) -> String {
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::pretty(&mut buf);
    value
        .serialize(&mut ser)
        .expect("serde_json::Value всегда сериализуется");
    String::from_utf8(buf).expect("serde_json пишет валидный UTF-8")
}

// ============================================================================
// Чтение / запись / слияние bridge.json (чистая логика над данными)
// ============================================================================

/// Прочитать bridge.json — порт `_read`. Любая ошибка чтения/парса или
/// не-объект на верхнем уровне → пустой словарь (Python логирует warning
/// и возвращает `{}`).
pub fn read_bridge(path: &Path) -> Map<String, Value> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        },
        Err(_) => Map::new(),
    }
}

/// Записать bridge.json общей дверью ([`crate::utils::atomic_write`]) — порт
/// `_write`. Формат `json.dump(indent=2, ensure_ascii=False)`, без
/// завершающего перевода строки. Ошибка возвращается (Python логирует и
/// молчит).
pub fn write_bridge(path: &Path, data: &Map<String, Value>) -> std::io::Result<()> {
    let text = to_json_indent2(&Value::Object(data.clone()));
    crate::utils::atomic_write(&path.to_string_lossy(), &text)
}

/// Слить обновления в секцию + проставить `_updated_at` — ядро `_update`
/// (без файла и flock). Порядок ключей: существующие сохраняют позицию,
/// новые добавляются в конец — как у Python dict.
///
/// Намеренное расхождение: в оригинале секция-НЕ-словарь (мусор от
/// стороннего приложения) роняла `AttributeError` и в основной ветке,
/// и в fallback без блокировки — крэш любого notify. Здесь такая секция
/// молча заменяется свежим словарём.
pub fn merge_update(
    data: &mut Map<String, Value>,
    section: &str,
    updates: Map<String, Value>,
    now_iso: &str,
) {
    let entry = data
        .entry(section.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    let obj = entry.as_object_mut().expect("секция только что стала объектом");
    for (k, v) in updates {
        obj.insert(k, v);
    }
    obj.insert("_updated_at".to_string(), Value::from(now_iso));
}

// ============================================================================
// Геттеры состояния (чистые, над прочитанным словарём)
// ============================================================================

/// Секция словаря (аналог `data.get(name, {})`); не-объект → None.
fn section<'a>(data: &'a Map<String, Value>, name: &str) -> Option<&'a Map<String, Value>> {
    data.get(name).and_then(|v| v.as_object())
}

/// Строковое поле секции с дефолтом. Нестроковое значение → дефолт
/// (Python возвращал значение как есть вопреки аннотации `-> str`).
fn section_str(data: &Map<String, Value>, sect: &str, key: &str, default: &str) -> String {
    section(data, sect)
        .and_then(|s| s.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

/// Установлен ли Rexon — порт `is_rexon_installed` (truthy-проверка).
pub fn is_rexon_installed(data: &Map<String, Value>) -> bool {
    truthy(section(data, "rexon").and_then(|s| s.get("rexon_installed")))
}

/// Версия Rexon — порт `get_rexon_version`.
pub fn get_rexon_version(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "rexon_version", "")
}

/// Текущий DE (lowercase) — порт `get_current_de`.
pub fn get_current_de(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "current_de", "unknown")
}

/// Человекочитаемое имя DE — порт `get_de_display`.
pub fn get_de_display(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "current_de_display", "")
}

/// Список установленных DE — порт `get_installed_des`. Нестроковые
/// элементы пропускаются (Python возвращал список как есть вопреки
/// аннотации `list[str]`).
pub fn get_installed_des(data: &Map<String, Value>) -> Vec<String> {
    section(data, "rexon")
        .and_then(|s| s.get("installed_des"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Текущая GTK-тема — порт `get_current_theme`.
pub fn get_current_theme(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "current_theme", "")
}

/// Текущий набор иконок — порт `get_current_icons`.
pub fn get_current_icons(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "current_icons", "")
}

/// Количество расширений — порт `get_extensions_count` (не-целое → 0).
pub fn get_extensions_count(data: &Map<String, Value>) -> i64 {
    section(data, "rexon")
        .and_then(|s| s.get("extensions_count"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
}

/// Запущены ли живые обои — порт `is_video_wallpaper_active`.
pub fn is_video_wallpaper_active(data: &Map<String, Value>) -> bool {
    truthy(section(data, "rexon").and_then(|s| s.get("video_wallpaper_active")))
}

/// Последнее переключение DE — порт `get_last_switch`:
/// `{"from": …, "to": …, "time": …}` либо None, если ключей
/// `switched_from`/`switched_to` нет. Значения возвращаются как есть.
pub fn get_last_switch(data: &Map<String, Value>) -> Option<Value> {
    let rexon = section(data, "rexon")?;
    let from = rexon.get("switched_from")?;
    let to = rexon.get("switched_to")?;
    let mut obj = Map::new();
    obj.insert("from".to_string(), from.clone());
    obj.insert("to".to_string(), to.clone());
    obj.insert(
        "time".to_string(),
        rexon.get("last_de_switch").cloned().unwrap_or_else(|| Value::from("")),
    );
    Some(Value::Object(obj))
}

/// Состояние приложения — порт `get_app_state` (`data["app_<id>"]`).
pub fn get_app_state(data: &Map<String, Value>, app_id: &str) -> Map<String, Value> {
    section(data, &format!("app_{app_id}")).cloned().unwrap_or_default()
}

/// Запущено ли приложение — порт `is_app_running` без `os.kill`:
/// живость pid решает замыкание `pid_alive` (I/O-граница).
///
/// Намеренное расхождение: в оригинале нецелый pid (строка/float) ронял
/// `TypeError` из `os.kill` мимо `except`, а pid ≤ 0 отправлял сигнал
/// группе процессов (спецсемантика kill). Здесь такой pid трактуется как
/// отсутствующий — возврат по truthy(`running`), как в ветке без pid.
pub fn app_running(
    state: &Map<String, Value>,
    pid_alive: impl FnOnce(i64) -> bool,
) -> bool {
    if !truthy(state.get("running")) {
        return false;
    }
    match state.get("pid").and_then(|v| v.as_i64()) {
        Some(pid) if pid > 0 => pid_alive(pid),
        _ => true, // running уже truthy
    }
}

/// Настроение BonziBuddy — порт `get_bonzi_mood` (default `"neutral"`).
pub fn get_bonzi_mood(data: &Map<String, Value>) -> String {
    get_app_state(data, "bonzi")
        .get("mood")
        .and_then(|v| v.as_str())
        .unwrap_or("neutral")
        .to_string()
}

/// Позиция BonziBuddy — порт `get_bonzi_position`
/// (default `{"x": 0, "y": 0}`).
pub fn get_bonzi_position(data: &Map<String, Value>) -> Value {
    get_app_state(data, "bonzi")
        .get("position")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({"x": 0, "y": 0}))
}

// ============================================================================
// Построение команд и событий (чистая логика)
// ============================================================================

/// Готовое сообщение моста: id, имя файла и JSON-содержимое.
pub struct BridgeMessage {
    pub id: String,
    pub filename: String,
    pub value: Value,
}

/// id команды/события: `{prefix}_{epoch_ms}` + антиколлизионный суффикс.
///
/// Намеренное расхождение: в оригинале id строился только из миллисекунд —
/// две команды в одну мс получали одно имя файла, и вторая молча затирала
/// первую (команда терялась). Здесь при `seq > 0` добавляется `_{seq}`
/// (записывающий подбирает первый свободный номер).
fn message_id(prefix: &str, epoch_ms: u64, seq: u32) -> String {
    if seq == 0 {
        format!("{prefix}_{epoch_ms}")
    } else {
        format!("{prefix}_{epoch_ms}_{seq}")
    }
}

/// Построить команду — порт словаря из `send_command`.
/// Порядок ключей: target, command, params, timestamp, id.
pub fn build_command(
    target: &str,
    command: &str,
    params: Option<Map<String, Value>>,
    timestamp_iso: &str,
    epoch_ms: u64,
    seq: u32,
) -> BridgeMessage {
    let id = message_id(&format!("{target}_{command}"), epoch_ms, seq);
    let mut obj = Map::new();
    obj.insert("target".to_string(), Value::from(target));
    obj.insert("command".to_string(), Value::from(command));
    obj.insert(
        "params".to_string(),
        Value::Object(params.unwrap_or_default()),
    );
    obj.insert("timestamp".to_string(), Value::from(timestamp_iso));
    obj.insert("id".to_string(), Value::from(id.clone()));
    BridgeMessage {
        filename: format!("{id}.json"),
        id,
        value: Value::Object(obj),
    }
}

/// Построить broadcast-событие — порт словаря из `send_event`:
/// `target="*"`, `command="event"`, `params={"type": …, **data}`.
pub fn build_event(
    event_type: &str,
    data: Map<String, Value>,
    timestamp_iso: &str,
    epoch_ms: u64,
    seq: u32,
) -> BridgeMessage {
    let id = message_id(&format!("event_{event_type}"), epoch_ms, seq);
    let mut params = Map::new();
    params.insert("type".to_string(), Value::from(event_type));
    for (k, v) in data {
        params.insert(k, v); // повторный "type" перезапишет значение, позиция сохранится
    }
    let mut obj = Map::new();
    obj.insert("target".to_string(), Value::from("*"));
    obj.insert("command".to_string(), Value::from("event"));
    obj.insert("params".to_string(), Value::Object(params));
    obj.insert("timestamp".to_string(), Value::from(timestamp_iso));
    obj.insert("id".to_string(), Value::from(id.clone()));
    BridgeMessage {
        filename: format!("{id}.json"),
        id,
        value: Value::Object(obj),
    }
}

/// Обновления секции `rexon` при переключении DE — порт словаря из
/// `notify_de_switch` (`last_de_switch` = тот же `now_iso`).
pub fn de_switch_updates(from_de: &str, to_de: &str, now_iso: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("current_de".to_string(), Value::from(to_de));
    m.insert("last_de_switch".to_string(), Value::from(now_iso));
    m.insert("switched_from".to_string(), Value::from(from_de));
    m.insert("switched_to".to_string(), Value::from(to_de));
    m
}

// ============================================================================
// RexonBridge — файловая обвязка (bridge.json + bridge_commands/)
// ============================================================================

/// Мост с конкретными путями. В отличие от Python-оригинала (где
/// `commands_dir` всегда брался из глобальной константы даже при
/// кастомном `bridge_file`) каталог команд задаётся явно — иначе
/// тесты/песочницы писали бы в реальный `~/.config/rexon`.
pub struct RexonBridge {
    pub bridge_file: PathBuf,
    pub commands_dir: PathBuf,
}

impl RexonBridge {
    /// Мост на стандартных путях (`~/.config/rexon/…`).
    pub fn new_default() -> Self {
        Self {
            bridge_file: default_bridge_file(),
            commands_dir: default_commands_dir(),
        }
    }

    /// Мост на явных путях (тесты, песочницы).
    pub fn new(bridge_file: PathBuf, commands_dir: PathBuf) -> Self {
        Self { bridge_file, commands_dir }
    }

    /// Создать каталоги (как `os.makedirs(..., exist_ok=True)` в `__init__`).
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        if let Some(parent) = self.bridge_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir_all(&self.commands_dir)
    }

    /// Порт `_read`.
    pub fn read(&self) -> Map<String, Value> {
        read_bridge(&self.bridge_file)
    }

    /// Порт `_write`.
    pub fn write(&self, data: &Map<String, Value>) -> std::io::Result<()> {
        write_bridge(&self.bridge_file, data)
    }

    /// Порт `_update`: read → merge → write. Межпроцессный flock —
    /// I/O-граница (шим); атомарность записи — tmp + rename.
    pub fn update_section(
        &self,
        section: &str,
        updates: Map<String, Value>,
        now_iso: &str,
    ) -> std::io::Result<()> {
        let mut data = self.read();
        merge_update(&mut data, section, updates, now_iso);
        self.write(&data)
    }

    /// Порт `update_rexon_state`: форсирует `rexon_installed = true`
    /// (позиция уже переданного ключа сохраняется — семантика dict).
    pub fn update_rexon_state(
        &self,
        mut updates: Map<String, Value>,
        now_iso: &str,
    ) -> std::io::Result<()> {
        updates.insert("rexon_installed".to_string(), Value::Bool(true));
        self.update_section("rexon", updates, now_iso)
    }

    /// Порт `notify_de_switch`: обновление секции + событие `de_switched`.
    pub fn notify_de_switch(
        &self,
        from_de: &str,
        to_de: &str,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<()> {
        self.update_section("rexon", de_switch_updates(from_de, to_de, now_iso), now_iso)?;
        let mut data = Map::new();
        data.insert("from".to_string(), Value::from(from_de));
        data.insert("to".to_string(), Value::from(to_de));
        self.send_event("de_switched", data, now_iso, epoch_ms)?;
        Ok(())
    }

    /// Порт `notify_de_installed`.
    pub fn notify_de_installed(
        &self,
        de_name: &str,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<()> {
        let mut data = Map::new();
        data.insert("de_name".to_string(), Value::from(de_name));
        self.send_event("de_installed", data, now_iso, epoch_ms).map(|_| ())
    }

    /// Порт `notify_de_removed`.
    pub fn notify_de_removed(
        &self,
        de_name: &str,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<()> {
        let mut data = Map::new();
        data.insert("de_name".to_string(), Value::from(de_name));
        self.send_event("de_removed", data, now_iso, epoch_ms).map(|_| ())
    }

    /// Порт `notify_theme_changed`: обновление темы + событие.
    pub fn notify_theme_changed(
        &self,
        theme_name: &str,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<()> {
        let mut updates = Map::new();
        updates.insert("current_theme".to_string(), Value::from(theme_name));
        self.update_section("rexon", updates, now_iso)?;
        let mut data = Map::new();
        data.insert("theme_name".to_string(), Value::from(theme_name));
        self.send_event("theme_changed", data, now_iso, epoch_ms).map(|_| ())
    }

    /// Порт `notify_extension_installed`.
    pub fn notify_extension_installed(
        &self,
        ext_name: &str,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<()> {
        let mut data = Map::new();
        data.insert("ext_name".to_string(), Value::from(ext_name));
        self.send_event("extension_installed", data, now_iso, epoch_ms).map(|_| ())
    }

    /// Порт `notify_backup_created`.
    pub fn notify_backup_created(&self, now_iso: &str, epoch_ms: u64) -> std::io::Result<()> {
        self.send_event("backup_created", Map::new(), now_iso, epoch_ms).map(|_| ())
    }

    /// Порт `notify_video_wallpaper`: признак в секцию и весть о том, что
    /// живые обои пошли или встали.
    ///
    /// Признак `video_wallpaper_active` пишется ВСЕГДА, что бы ни решил
    /// вызывающий про весть: на него смотрит читающая сторона, когда события
    /// до неё не дошли, и лгать он не имеет права.
    ///
    /// ⚠️ Починка: остановка не слала НИЧЕГО — только гасила признак. Слушатель
    /// узнавал о запуске событием, а об остановке — лишь если сам догадается
    /// перечитать признак. Теперь у остановки есть своё событие
    /// [`WALLPAPER_STOPPED_EVENT`], парное к [`WALLPAPER_STARTED_EVENT`].
    ///
    /// `name` — название сцены; пустое (или из одних пробелов) поля при
    /// событии не даёт вовсе, выдумывать за движок нечего. У остановки
    /// названия нет: встали — и всё, какие именно обои встали, слушатель
    /// помнит с запуска.
    ///
    /// `announce` — рассказывать ли о смене событием. Ложь оставляет признак
    /// верным и молчит: так проходит ПОДМЕНА обоев другими (движок снимает
    /// прежние перед запуском новых), которая остановкой не является и парой
    /// «встали — пошли» слушателя только обманывала бы.
    pub fn notify_video_wallpaper(
        &self,
        active: bool,
        name: &str,
        announce: bool,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<()> {
        let mut updates = Map::new();
        updates.insert("video_wallpaper_active".to_string(), Value::Bool(active));
        self.update_section("rexon", updates, now_iso)?;
        if !announce {
            return Ok(());
        }
        let mut data = Map::new();
        let event = if active {
            let name = name.trim();
            if !name.is_empty() {
                data.insert("name".to_string(), Value::from(name));
            }
            WALLPAPER_STARTED_EVENT
        } else {
            WALLPAPER_STOPPED_EVENT
        };
        self.send_event(event, data, now_iso, epoch_ms)?;
        Ok(())
    }

    /// Порт `update_app_state` (секция `app_<id>`).
    pub fn update_app_state(
        &self,
        app_id: &str,
        updates: Map<String, Value>,
        now_iso: &str,
    ) -> std::io::Result<()> {
        self.update_section(&format!("app_{app_id}"), updates, now_iso)
    }

    /// Порт `get_app_state`.
    pub fn get_app_state(&self, app_id: &str) -> Map<String, Value> {
        get_app_state(&self.read(), app_id)
    }

    /// Порт `is_app_running` (живость pid — замыканием, см. [`app_running`]).
    pub fn is_app_running(
        &self,
        app_id: &str,
        pid_alive: impl FnOnce(i64) -> bool,
    ) -> bool {
        app_running(&self.get_app_state(app_id), pid_alive)
    }

    /// Порт `send_command`: записать команду JSON-файлом в каталог команд.
    /// Возвращает имя записанного файла.
    ///
    /// Намеренные расхождения: запись атомарная (tmp + rename) — в
    /// оригинале получатель мог прочитать недописанный файл, счесть его
    /// битым и удалить (команда терялась); при коллизии имени в одну мс
    /// подбирается свободный суффикс `_{N}` вместо молчаливой перезаписи.
    pub fn send_command(
        &self,
        target: &str,
        command: &str,
        params: Option<Map<String, Value>>,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<String> {
        let prefix = format!("{target}_{command}");
        let msg = self.next_free_message(&prefix, epoch_ms, |seq| {
            build_command(target, command, params.clone(), now_iso, epoch_ms, seq)
        });
        self.write_message(&msg)?;
        Ok(msg.filename)
    }

    /// Порт `send_event`: broadcast-событие (`target="*"`). Возвращает имя
    /// записанного файла. Та же атомарность/антиколлизия, что у команд.
    pub fn send_event(
        &self,
        event_type: &str,
        data: Map<String, Value>,
        now_iso: &str,
        epoch_ms: u64,
    ) -> std::io::Result<String> {
        let prefix = format!("event_{event_type}");
        let msg = self.next_free_message(&prefix, epoch_ms, |seq| {
            build_event(event_type, data.clone(), now_iso, epoch_ms, seq)
        });
        self.write_message(&msg)?;
        Ok(msg.filename)
    }

    /// Первый свободный номер суффикса для имени файла сообщения.
    fn next_free_message(
        &self,
        prefix: &str,
        epoch_ms: u64,
        build: impl Fn(u32) -> BridgeMessage,
    ) -> BridgeMessage {
        let mut seq = 0u32;
        loop {
            let candidate = format!("{}.json", message_id(prefix, epoch_ms, seq));
            if !self.commands_dir.join(&candidate).exists() {
                return build(seq);
            }
            seq += 1;
        }
    }

    /// Запись файла сообщения общей дверью ([`crate::utils::atomic_write`]).
    ///
    /// Читатель сообщений — чужой процесс, он берёт файл сразу, как увидел.
    /// Половина сообщения для него — «битый JSON», и такой файл он УДАЛЯЕТ:
    /// команда теряется молча, без единого следа.
    fn write_message(&self, msg: &BridgeMessage) -> std::io::Result<()> {
        let path = self.commands_dir.join(&msg.filename);
        let text = to_json_indent2(&msg.value);
        crate::utils::atomic_write(&path.to_string_lossy(), &text)
    }

    /// Порт `poll_commands`: команды для `app_id` в отсортированном по
    /// имени порядке. `delete` — удалять отданные. Битые (не-JSON,
    /// JSON-не-объект) и превышающие [`MAX_CMD_SIZE`] файлы удаляются
    /// независимо от `delete` (как в оригинале); файлы без `.json` и
    /// команды чужих target не трогаются. Отсутствие каталога → пусто.
    pub fn poll_commands(&self, app_id: &str, delete: bool) -> Vec<Value> {
        let mut commands = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.commands_dir) else {
            return commands;
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        for filename in names {
            if !filename.ends_with(".json") {
                continue;
            }
            let filepath = self.commands_dir.join(&filename);
            let size = match std::fs::metadata(&filepath) {
                Ok(m) => m.len(),
                Err(_) => {
                    // Файл исчез между листингом и stat — как общий except:
                    // попытка удаления, подавление ошибки.
                    let _ = std::fs::remove_file(&filepath);
                    continue;
                }
            };
            if size > MAX_CMD_SIZE {
                let _ = std::fs::remove_file(&filepath);
                continue;
            }
            let parsed = std::fs::read_to_string(&filepath)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok());
            let Some(cmd @ Value::Object(_)) = parsed else {
                // Битый файл — удаляем (ошибку удаления подавляем).
                let _ = std::fs::remove_file(&filepath);
                continue;
            };
            if cmd.get("target").and_then(|v| v.as_str()) == Some(app_id) {
                commands.push(cmd);
                if delete {
                    let _ = std::fs::remove_file(&filepath);
                }
            }
        }
        commands
    }

    /// Порт `poll_events`: все broadcast-события (`target="*"`).
    pub fn poll_events(&self, delete: bool) -> Vec<Value> {
        self.poll_commands("*", delete)
    }

    /// Порт `cleanup_commands`: удалить файлы команд старше
    /// `max_age_seconds` (по mtime). `now_epoch` — текущее время
    /// (`time.time()`), параметром.
    ///
    /// Намеренное расхождение: в оригинале ошибка на одном файле
    /// (исчез между листингом и stat) прерывала весь проход — здесь
    /// подавляется по-файлово, очистка продолжается.
    pub fn cleanup_commands(&self, max_age_seconds: f64, now_epoch: f64) {
        let Ok(entries) = std::fs::read_dir(&self.commands_dir) else {
            return;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let filepath = entry.path();
            if !filepath.is_file() {
                continue;
            }
            let Ok(meta) = std::fs::metadata(&filepath) else {
                continue;
            };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64());
            let Some(mtime) = mtime else { continue };
            if now_epoch - mtime > max_age_seconds {
                let _ = std::fs::remove_file(&filepath);
            }
        }
    }

    /// Порт `reset`: пустой bridge.json + очистка каталога команд.
    ///
    /// Намеренное расхождение: в оригинале первый сбой `os.remove`
    /// (например, подкаталог в каталоге команд) прерывал очистку —
    /// здесь ошибки подавляются по-файлово, каталоги пропускаются.
    pub fn reset(&self) -> std::io::Result<()> {
        self.write(&Map::new())?;
        if let Ok(entries) = std::fs::read_dir(&self.commands_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        Ok(())
    }
}
