//! Bridge between Rexon and external applications — port of
//! `rexon/rexon_bridge.py`.
//!
//! Two-way exchange of state between Rexon and BonziBuddy (and others) through
//! a shared JSON file `~/.config/rexon/bridge.json` plus file commands/events in
//! `~/.config/rexon/bridge_commands/`. The original is a self-contained module
//! (pure Python + JSON, no Qt), so it is ported almost entirely: the DE reaction
//! data, building messages, merging sections, state getters, polling and
//! cleaning up commands.
//!
//! I/O boundaries (they stay with the caller/the shim):
//! - the current time — via the parameters `now_iso` (the ISO string of
//!   `datetime.now().isoformat()`) and `epoch_ms`/`now_epoch` (`time.time()`);
//! - the process liveness check (`os.kill(pid, 0)`) — via the `pid_alive`
//!   closure in [`app_running`];
//! - the inter-process lock `fcntl.flock` from `_update` — outside the core (the
//!   shim holds a `.lock` file around [`RexonBridge::update_section`]; the
//!   atomicity of the write itself is ensured by tmp + rename);
//! - the CLI utility `main()` (argparse + random) is not ported — it is a
//!   debugging simulator, not bridge logic.
//!
//! File operations on bridge.json/commands are done directly (as in
//! `favorites.rs`) — these are local files, not sockets or a daemon.
//!
//! Write format: `json.dump(indent=2, ensure_ascii=False)` — checked byte for
//! byte (golden test `rexon_bridge_parity.rs`).

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

// ============================================================================
// Paths
// ============================================================================

/// The bridge directory (the module constant `BRIDGE_DIR`) — the Rexon settings
/// folder.
///
/// Warning — fix: the path used to be glued together as the literal
/// `~/.config/rexon` from bare `$HOME`, bypassing `$XDG_CONFIG_HOME`. The bridge
/// is inter-process: BonziBuddy and others read `bridge.json` from their path,
/// while the Rexon settings window, favorites and extensions live in the
/// XDG-aware folder. On a machine with a non-standard `XDG_CONFIG_HOME` the
/// bridge drifted into ANOTHER folder: Rexon wrote state where nobody looks for
/// it, file commands from `bridge_commands/` did not arrive, and uninstalling
/// Rexon left this folder on disk. The path is computed by
/// [`crate::utils::rexon_config_dir`] — the same single point as all other
/// Rexon stores in the configs.
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

/// Size limit of a command file: 1 MiB — protection against DoS (`MAX_CMD_SIZE`).
pub const MAX_CMD_SIZE: u64 = 1024 * 1024;

// ============================================================================
// DE reactions for BonziBuddy
// ============================================================================

/// Sets of lines for one DE (the keys of the dictionary `DE_REACTIONS[de]`).
pub struct DeReactions {
    pub greetings: &'static [&'static str],
    pub jokes: &'static [&'static str],
    pub on_switch_to: &'static [&'static str],
    pub on_switch_from: &'static [&'static str],
    pub idle_comments: &'static [&'static str],
}

impl DeReactions {
    /// A JSON object with the same key order as the Python dictionary.
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

/// `DE_REACTIONS` — BonziBuddy's lines per DE (order as in the original).
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
    // Warning — rule #12: the list of DEs here must match `_DE_DATABASE`
    // (rexon/system.py). LXQt, niri and sway were added there but forgotten here —
    // BonziBuddy was silent in three supported environments (get_de_reactions
    // returned {}, because there is no "-" in the name and normalization did not
    // help).
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

/// Bridge event name: the live wallpaper started. The scene name goes in the
/// `name` field, if the sender knows it.
pub const WALLPAPER_STARTED_EVENT: &str = "video_wallpaper_started";
/// Bridge event name: the live wallpaper stopped.
pub const WALLPAPER_STOPPED_EVENT: &str = "video_wallpaper_stopped";

/// `EVENT_REACTIONS` — reaction templates for events.
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

/// Reactions for a DE with the normalization `"cinnamon-mint"` → `"cinnamon"` —
/// port of `get_de_reactions` (without the branch reading current_de from the
/// file: the caller does that via [`get_current_de`]).
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

/// The same, but as a JSON object (Python returns `{}` for an unknown DE).
pub fn de_reactions_value(de: &str) -> Value {
    match lookup_de_reactions(de) {
        Some(r) => r.to_value(),
        None => Value::Object(Map::new()),
    }
}

/// All reactions of all DEs — port of `get_all_de_reactions` (for preloading).
pub fn all_de_reactions_value() -> Value {
    let mut obj = Map::new();
    for (de, r) in DE_REACTIONS {
        obj.insert((*de).to_string(), r.to_value());
    }
    Value::Object(obj)
}

/// Reaction templates for an event — port of `get_event_reactions` (an empty
/// slice if the type is unknown).
pub fn event_reactions(event_type: &str) -> &'static [&'static str] {
    EVENT_REACTIONS
        .iter()
        .find(|(k, _)| *k == event_type)
        .map(|(_, v)| *v)
        .unwrap_or(&[])
}

// ============================================================================
// Python semantics helpers
// ============================================================================

/// Truthiness of a value per Python's rules (`bool(x)`).
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

/// `json.dumps(obj, ensure_ascii=False, indent=2)` — the format of bridge.json
/// and of the command files. A local analogue of `models::to_json_indent1`, but
/// with an indent of 2 (the serde_json `PrettyFormatter` default).
fn to_json_indent2(value: &Value) -> String {
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::pretty(&mut buf);
    value
        .serialize(&mut ser)
        .expect("serde_json::Value всегда сериализуется");
    String::from_utf8(buf).expect("serde_json пишет валидный UTF-8")
}

// ============================================================================
// Reading / writing / merging bridge.json (pure logic over data)
// ============================================================================

/// Read bridge.json — port of `_read`. Any read/parse error or a non-object at
/// the top level → an empty dictionary (Python logs a warning and returns `{}`).
pub fn read_bridge(path: &Path) -> Map<String, Value> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        },
        Err(_) => Map::new(),
    }
}

/// Write bridge.json through the shared door ([`crate::utils::atomic_write`]) —
/// port of `_write`. Format `json.dump(indent=2, ensure_ascii=False)`, without a
/// trailing newline. The error is returned (Python logs it and keeps quiet).
pub fn write_bridge(path: &Path, data: &Map<String, Value>) -> std::io::Result<()> {
    let text = to_json_indent2(&Value::Object(data.clone()));
    crate::utils::atomic_write(&path.to_string_lossy(), &text)
}

/// Merge updates into a section + set `_updated_at` — the core of `_update`
/// (without the file and flock). Key order: existing keys keep their position,
/// new ones are appended at the end — as with a Python dict.
///
/// A deliberate divergence: in the original a section that is NOT a dictionary
/// (garbage from a third-party application) raised `AttributeError` both in the
/// main branch and in the fallback without a lock — a crash of any notify. Here
/// such a section is silently replaced with a fresh dictionary.
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
// State getters (pure, over the dictionary that was read)
// ============================================================================

/// A section of the dictionary (the analogue of `data.get(name, {})`); a
/// non-object → None.
fn section<'a>(data: &'a Map<String, Value>, name: &str) -> Option<&'a Map<String, Value>> {
    data.get(name).and_then(|v| v.as_object())
}

/// A string field of a section with a default. A non-string value → the default
/// (Python returned the value as is, contrary to the `-> str` annotation).
fn section_str(data: &Map<String, Value>, sect: &str, key: &str, default: &str) -> String {
    section(data, sect)
        .and_then(|s| s.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or(default)
        .to_string()
}

/// Whether Rexon is installed — port of `is_rexon_installed` (a truthy check).
pub fn is_rexon_installed(data: &Map<String, Value>) -> bool {
    truthy(section(data, "rexon").and_then(|s| s.get("rexon_installed")))
}

/// Rexon version — port of `get_rexon_version`.
pub fn get_rexon_version(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "rexon_version", "")
}

/// Current DE (lowercase) — port of `get_current_de`.
pub fn get_current_de(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "current_de", "unknown")
}

/// Human-readable DE name — port of `get_de_display`.
pub fn get_de_display(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "current_de_display", "")
}

/// List of installed DEs — port of `get_installed_des`. Non-string items are
/// skipped (Python returned the list as is, contrary to the `list[str]`
/// annotation).
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

/// Current GTK theme — port of `get_current_theme`.
pub fn get_current_theme(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "current_theme", "")
}

/// Current icon set — port of `get_current_icons`.
pub fn get_current_icons(data: &Map<String, Value>) -> String {
    section_str(data, "rexon", "current_icons", "")
}

/// Number of extensions — port of `get_extensions_count` (a non-integer → 0).
pub fn get_extensions_count(data: &Map<String, Value>) -> i64 {
    section(data, "rexon")
        .and_then(|s| s.get("extensions_count"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
}

/// Whether the live wallpaper is running — port of `is_video_wallpaper_active`.
pub fn is_video_wallpaper_active(data: &Map<String, Value>) -> bool {
    truthy(section(data, "rexon").and_then(|s| s.get("video_wallpaper_active")))
}

/// The last DE switch — port of `get_last_switch`:
/// `{"from": …, "to": …, "time": …}` or None if there are no
/// `switched_from`/`switched_to` keys. The values are returned as is.
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

/// Application state — port of `get_app_state` (`data["app_<id>"]`).
pub fn get_app_state(data: &Map<String, Value>, app_id: &str) -> Map<String, Value> {
    section(data, &format!("app_{app_id}")).cloned().unwrap_or_default()
}

/// Whether an application is running — port of `is_app_running` without
/// `os.kill`: pid liveness is decided by the `pid_alive` closure (the I/O
/// boundary).
///
/// A deliberate divergence: in the original a non-integer pid (a string/float)
/// raised `TypeError` from `os.kill` past the `except`, and a pid ≤ 0 sent a
/// signal to a process group (kill's special semantics). Here such a pid is
/// treated as missing — the return follows truthy(`running`), as in the branch
/// without a pid.
pub fn app_running(
    state: &Map<String, Value>,
    pid_alive: impl FnOnce(i64) -> bool,
) -> bool {
    if !truthy(state.get("running")) {
        return false;
    }
    match state.get("pid").and_then(|v| v.as_i64()) {
        Some(pid) if pid > 0 => pid_alive(pid),
        _ => true, // running is already truthy
    }
}

/// BonziBuddy's mood — port of `get_bonzi_mood` (default `"neutral"`).
pub fn get_bonzi_mood(data: &Map<String, Value>) -> String {
    get_app_state(data, "bonzi")
        .get("mood")
        .and_then(|v| v.as_str())
        .unwrap_or("neutral")
        .to_string()
}

/// BonziBuddy's position — port of `get_bonzi_position`
/// (default `{"x": 0, "y": 0}`).
pub fn get_bonzi_position(data: &Map<String, Value>) -> Value {
    get_app_state(data, "bonzi")
        .get("position")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({"x": 0, "y": 0}))
}

// ============================================================================
// Building commands and events (pure logic)
// ============================================================================

/// A ready bridge message: id, file name and JSON content.
pub struct BridgeMessage {
    pub id: String,
    pub filename: String,
    pub value: Value,
}

/// Command/event id: `{prefix}_{epoch_ms}` + an anti-collision suffix.
///
/// A deliberate divergence: in the original the id was built from the
/// milliseconds only — two commands within one ms got the same file name, and
/// the second silently overwrote the first (a command was lost). Here with
/// `seq > 0`, `_{seq}` is appended (the writer picks the first free number).
fn message_id(prefix: &str, epoch_ms: u64, seq: u32) -> String {
    if seq == 0 {
        format!("{prefix}_{epoch_ms}")
    } else {
        format!("{prefix}_{epoch_ms}_{seq}")
    }
}

/// Build a command — port of the dictionary from `send_command`.
/// Key order: target, command, params, timestamp, id.
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

/// Build a broadcast event — port of the dictionary from `send_event`:
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
        params.insert(k, v); // a repeated "type" overwrites the value, the position is kept
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

/// Updates of the `rexon` section on a DE switch — port of the dictionary from
/// `notify_de_switch` (`last_de_switch` = the same `now_iso`).
pub fn de_switch_updates(from_de: &str, to_de: &str, now_iso: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("current_de".to_string(), Value::from(to_de));
    m.insert("last_de_switch".to_string(), Value::from(now_iso));
    m.insert("switched_from".to_string(), Value::from(from_de));
    m.insert("switched_to".to_string(), Value::from(to_de));
    m
}

// ============================================================================
// RexonBridge — the file plumbing (bridge.json + bridge_commands/)
// ============================================================================

/// A bridge with concrete paths. Unlike the Python original (where
/// `commands_dir` was always taken from the global constant even with a custom
/// `bridge_file`), the command directory is given explicitly — otherwise tests
/// and sandboxes would write into the real `~/.config/rexon`.
pub struct RexonBridge {
    pub bridge_file: PathBuf,
    pub commands_dir: PathBuf,
}

impl RexonBridge {
    /// A bridge on the standard paths (`~/.config/rexon/…`).
    pub fn new_default() -> Self {
        Self {
            bridge_file: default_bridge_file(),
            commands_dir: default_commands_dir(),
        }
    }

    /// A bridge on explicit paths (tests, sandboxes).
    pub fn new(bridge_file: PathBuf, commands_dir: PathBuf) -> Self {
        Self { bridge_file, commands_dir }
    }

    /// Create the directories (like `os.makedirs(..., exist_ok=True)` in `__init__`).
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        if let Some(parent) = self.bridge_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir_all(&self.commands_dir)
    }

    /// Port of `_read`.
    pub fn read(&self) -> Map<String, Value> {
        read_bridge(&self.bridge_file)
    }

    /// Port of `_write`.
    pub fn write(&self, data: &Map<String, Value>) -> std::io::Result<()> {
        write_bridge(&self.bridge_file, data)
    }

    /// Port of `_update`: read → merge → write. The inter-process flock is an I/O
    /// boundary (the shim); write atomicity — tmp + rename.
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

    /// Port of `update_rexon_state`: forces `rexon_installed = true` (the position
    /// of an already passed key is kept — dict semantics).
    pub fn update_rexon_state(
        &self,
        mut updates: Map<String, Value>,
        now_iso: &str,
    ) -> std::io::Result<()> {
        updates.insert("rexon_installed".to_string(), Value::Bool(true));
        self.update_section("rexon", updates, now_iso)
    }

    /// Port of `notify_de_switch`: section update + the `de_switched` event.
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

    /// Port of `notify_de_installed`.
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

    /// Port of `notify_de_removed`.
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

    /// Port of `notify_theme_changed`: theme update + an event.
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

    /// Port of `notify_extension_installed`.
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

    /// Port of `notify_backup_created`.
    pub fn notify_backup_created(&self, now_iso: &str, epoch_ms: u64) -> std::io::Result<()> {
        self.send_event("backup_created", Map::new(), now_iso, epoch_ms).map(|_| ())
    }

    /// Port of `notify_video_wallpaper`: the flag in the section and the news that
    /// the live wallpaper started or stopped.
    ///
    /// The `video_wallpaper_active` flag is ALWAYS written, whatever the caller
    /// decides about the news: the reading side looks at it when events did not
    /// reach it, and it has no right to lie.
    ///
    /// Warning — fix: stopping sent NOTHING — it only cleared the flag. A listener
    /// learned about the start through an event, but about the stop only if it
    /// thought of re-reading the flag itself. Now stopping has its own event
    /// [`WALLPAPER_STOPPED_EVENT`], paired with [`WALLPAPER_STARTED_EVENT`].
    ///
    /// `name` — the scene name; an empty one (or only spaces) gives no field in the
    /// event at all, there is nothing to invent on the engine's behalf. Stopping has
    /// no name: it stopped, that is all — the listener remembers which wallpaper
    /// stopped from the start.
    ///
    /// `announce` — whether to announce the change with an event. False keeps the
    /// flag correct and stays silent: this is how REPLACING the wallpaper with
    /// another one goes (the engine removes the old one before starting the new
    /// one), which is not a stop, and a "stopped — started" pair would only mislead
    /// the listener.
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

    /// Port of `update_app_state` (the `app_<id>` section).
    pub fn update_app_state(
        &self,
        app_id: &str,
        updates: Map<String, Value>,
        now_iso: &str,
    ) -> std::io::Result<()> {
        self.update_section(&format!("app_{app_id}"), updates, now_iso)
    }

    /// Port of `get_app_state`.
    pub fn get_app_state(&self, app_id: &str) -> Map<String, Value> {
        get_app_state(&self.read(), app_id)
    }

    /// Port of `is_app_running` (pid liveness via a closure, see [`app_running`]).
    pub fn is_app_running(
        &self,
        app_id: &str,
        pid_alive: impl FnOnce(i64) -> bool,
    ) -> bool {
        app_running(&self.get_app_state(app_id), pid_alive)
    }

    /// Port of `send_command`: write the command as a JSON file into the command
    /// directory. Returns the name of the written file.
    ///
    /// Deliberate divergences: the write is atomic (tmp + rename) — in the original
    /// the receiver could read a half-written file, consider it broken and delete
    /// it (the command was lost); on a name collision within one ms a free suffix
    /// `_{N}` is picked instead of a silent overwrite.
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

    /// Port of `send_event`: a broadcast event (`target="*"`). Returns the name of
    /// the written file. The same atomicity/anti-collision as for commands.
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

    /// The first free suffix number for a message file name.
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

    /// Writing a message file through the shared door ([`crate::utils::atomic_write`]).
    ///
    /// The message reader is a foreign process, it takes the file as soon as it
    /// sees it. Half a message is "broken JSON" for it, and it DELETES such a file:
    /// the command is lost silently, without a trace.
    fn write_message(&self, msg: &BridgeMessage) -> std::io::Result<()> {
        let path = self.commands_dir.join(&msg.filename);
        let text = to_json_indent2(&msg.value);
        crate::utils::atomic_write(&path.to_string_lossy(), &text)
    }

    /// Port of `poll_commands`: commands for `app_id` in name-sorted order.
    /// `delete` — delete the ones returned. Broken files (non-JSON, JSON that is not
    /// an object) and those exceeding [`MAX_CMD_SIZE`] are deleted regardless of
    /// `delete` (as in the original); files without `.json` and commands for other
    /// targets are not touched. A missing directory → empty.
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
                    // The file disappeared between listing and stat — as with the general
                    // except: attempt to delete, suppress the error.
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
                // A broken file — delete it (suppress a deletion error).
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

    /// Port of `poll_events`: all broadcast events (`target="*"`).
    pub fn poll_events(&self, delete: bool) -> Vec<Value> {
        self.poll_commands("*", delete)
    }

    /// Port of `cleanup_commands`: delete command files older than
    /// `max_age_seconds` (by mtime). `now_epoch` — the current time
    /// (`time.time()`), as a parameter.
    ///
    /// A deliberate divergence: in the original an error on one file (it
    /// disappeared between listing and stat) aborted the whole pass — here it is
    /// suppressed per file, and the cleanup continues.
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

    /// Port of `reset`: an empty bridge.json + cleaning up the command directory.
    ///
    /// A deliberate divergence: in the original the first `os.remove` failure (for
    /// example, a subdirectory in the command directory) aborted the cleanup — here
    /// errors are suppressed per file, directories are skipped.
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
