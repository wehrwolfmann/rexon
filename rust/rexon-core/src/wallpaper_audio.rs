//! Звук обоев: одна модель, одна политика, один источник значений.
//!
//! До этого модуля звук обоев жил тремя несвязанными кусками:
//!
//!   * галочка «Звук видео-обоев» в наших настройках — нигде не
//!     сохранялась и при каждом открытии вкладки снова вставала в «да»;
//!   * громкость нашего хоста — константа `50` в памяти Python-объекта,
//!     контрола в интерфейсе не было вовсе;
//!   * громкость, «глушить при чужом звуке» и таблица «глушить когда…»
//!     чужого движка — отдельная карта, уезжавшая в его демон и никак не
//!     связанная с первыми двумя.
//!
//! Из-за этого человек, поставивший громкость 30 % во вкладке чужого
//! движка, у наших видео-обоев получал 50 %, а переключатель «без звука»
//! накрывал только чужой движок. Правило #12: чинить общий механизм, а не
//! обе копии по отдельности — поэтому модель настроек, значения по
//! умолчанию и решение «играть/глушить/паузить» считаются ЗДЕСЬ, а обе
//! стороны только читают отсюда:
//!
//!   * наш хост обоев `rexon-wallpaper-host` — [`AudioSettings::from_map`]
//!     поверх `~/.config/rexon/settings.json` и [`AudioSettings::decide`];
//!   * чужой движок — та же структура, разложенная в его карту глобальных
//!     настроек ([`AudioSettings::write_engine_global`]).
//!
//! Хранилище одно — `settings.json`. Отдельного файла состояния нет
//! намеренно: живое применение работает от того же файла, который пишет
//! окно настроек, поэтому «сохранил» и «услышал» не могут разойтись.

use serde_json::{Map, Value};

// ── Ключи хранилища ────────────────────────────────────────────────────────

/// Играет ли звук обоев вообще.
pub const KEY_ENABLED: &str = "wallpaper_audio_enabled";
/// Громкость обоев, проценты 0..100.
pub const KEY_VOLUME: &str = "wallpaper_audio_volume";
/// Глушить, когда звук играет кто-то ещё.
pub const KEY_MUTE_OTHER: &str = "wallpaper_audio_mute_when_other";
/// Что делать в каждом из состояний окон — объект «условие → действие».
pub const KEY_ACTIONS: &str = "wallpaper_audio_actions";
/// Плавность приглушения, миллисекунды.
pub const KEY_FADE_MS: &str = "wallpaper_audio_fade_ms";

// ── Ключи чужого движка ────────────────────────────────────────────────────

/// Те же значения в карте глобальных настроек чужого движка.
pub const ENGINE_KEY_ENABLED: &str = "renderer.enable_audio";
pub const ENGINE_KEY_VOLUME: &str = "renderer.volume";
pub const ENGINE_KEY_MUTE_OTHER: &str = "muteWhenOtherAudio";
pub const ENGINE_KEY_FADE_MS: &str = "audioFadeMs";
pub const ENGINE_KEY_ACTIONS: &str = "autoReplay";

// ── Свойства звукового потока ──────────────────────────────────────────────

/// Роль потока обоев.
///
/// ⚠️ НЕ `music`. Qt Multimedia помечает свои потоки `Music` жёстко —
/// публичного способа это изменить нет, значение зашито в саму библиотеку.
/// Обои от этого попадают в корзину музыкальных плееров: по роли строится
/// ячейка запоминания громкости (общая со ВСЕМИ музыкальными
/// программами — громкость обоев затирает громкость плеера и наоборот), и
/// по ней же работают `module-role-cork` и `module-role-ducking`.
///
/// Почему именно `Movie`, а не «что-нибудь про анимацию». Проверено
/// замером на PipeWire 1.6: словарь ролей там свой (`Movie`, `Music`,
/// `Camera`, `Screen`, `Communication`, `Game`, `Notification`, `DSP`,
/// `Production`, `Accessibility`, `Test`), а роли PulseAudio в него
/// переводятся — `animation` схлопывается в `Movie` и наружу выходит как
/// `video`. То есть «поставить обоям роль анимации» физически
/// недостижимо: выбор не между `animation` и `video`, а между `Music` и
/// всем остальным. `Movie` описывает видео-обои честно и, в отличие от
/// `Screen` (это про захват экрана), не врёт о происхождении потока.
pub const STREAM_ROLE: &str = "Movie";

/// Имя узла в звуковой системе — латиницей и без пробелов: это ключ, а не
/// подпись.
pub const STREAM_NODE: &str = "rexon-wallpaper";

/// Ключ, по которому звуковая система запоминает громкость обоев отдельно
/// от всего остального. Именно он, а не роль, отвечает за память громкости
/// на PipeWire.
pub const STREAM_APP_ID: &str = "com.rexon.wallpaper";

/// Имя, под которым обои видно в системном микшере. По-человечески и на
/// языке интерфейса: в микшере KDE строка идёт рядом с «Firefox» и
/// «Steam», и `RexonVideoWallpaper` там читается как сбой, а не как
/// программа (правило #13).
///
/// ⚠️ Это ПОДПИСЬ, а не ключ: свой поток звуковая система узнаёт по
/// [`STREAM_APP_ID`] и [`STREAM_NODE`] (см. [`OWN_STREAM_MARKERS`]), а
/// память громкости держится на `application.id`. Двойная кавычка
/// из перевода вырезается — имя стоит внутри кавычек строки свойств.
pub fn stream_description() -> String {
    // TRANSLATORS: name of the wallpaper's audio stream in the system volume mixer
    crate::i18n::tr("Rexon Wallpaper").replace('"', "")
}

/// Имя потока до перевода интерфейса. Уже не подпись, а примета: у людей
/// оно ещё живёт в ячейках памяти громкости звуковой системы.
const LEGACY_STREAM_DESCRIPTION: &str = "Обои Rexon";

/// Строка `PIPEWIRE_PROPS` для процесса обоев.
///
/// Почему именно окружение, а не вызов Qt: роль потока Qt Multimedia
/// наружу не отдаёт. Зато проигрыватель Qt 6.11 ходит в звук нативным
/// клиентом PipeWire, а тот читает `PIPEWIRE_PROPS` при создании
/// контекста, то есть уже после старта процесса: достаточно выставить
/// переменную в своём же `main()` до первой звуковой операции.
pub fn pipewire_props() -> String {
    let description = stream_description();
    format!(
        "{{ media.role = {STREAM_ROLE} media.category = Playback \
         application.id = {STREAM_APP_ID} node.name = {STREAM_NODE} \
         application.name = \"{description}\" \
         media.name = \"{description}\" }}"
    )
}

/// Та же просьба для клиента PulseAudio — на машинах без PipeWire.
///
/// Роль там пишется словарём PulseAudio (`video`), и свойства потока
/// задаёт сама программа, так что переменная перебивает не всё; вреда от
/// неё нет, а на стеках, где роль ставится на контексте, она попадает в
/// цель.
pub fn pulse_props() -> String {
    format!(
        "media.role=video media.category=Playback application.id={STREAM_APP_ID}"
    )
}

/// Та же просьба для клиента ALSA — ею в звук ходит Flash (Ruffle через cpal).
///
/// ⚠️ Без неё `PIPEWIRE_PROPS` доносит роль и `application.id`, но ИМЯ
/// переписывает плагин ALSA: в системном микшере строка выходит как
/// «PipeWire ALSA [rexon-wallpaper-host]» (живой замер 16.09.2026). Для
/// покупателя это не название программы, а сбой (правило #13), да и нам
/// самим поток удобнее узнавать по своему имени, а не по чужой обёртке.
pub fn alsa_props() -> String {
    let description = stream_description();
    format!(
        "{{ node.name = {STREAM_NODE} application.name = \"{description}\" \
         media.name = \"{description}\" media.role = {STREAM_ROLE} \
         application.id = {STREAM_APP_ID} }}"
    )
}

/// Все переменные окружения разом — то, что хост ставит себе в `main()`.
///
/// Трое, а не один: у каждого звукового клиента свой способ прочитать
/// свойства потока. Qt Multimedia (видео) ходит нативным клиентом PipeWire —
/// его слушает `PIPEWIRE_PROPS`; на машинах без PipeWire работает
/// `PULSE_PROP_OVERRIDE`; Ruffle (Flash) ходит через cpal и ALSA — там
/// решает `PIPEWIRE_ALSA`. Лишняя переменная вреда не делает: чужой клиент
/// её просто не читает.
pub fn stream_env() -> Vec<(&'static str, String)> {
    vec![
        ("PIPEWIRE_PROPS", pipewire_props()),
        ("PULSE_PROP_OVERRIDE", pulse_props()),
        ("PIPEWIRE_ALSA", alsa_props()),
    ]
}

// ── Действия ───────────────────────────────────────────────────────────────

/// Что делать с обоями, когда условие выполнено.
///
/// Числа совпадают с `AutoAction` протокола чужого движка (`NONE`, `MUTE`,
/// `PAUSE`, `STOP`) — намеренно: одна и та же настройка не должна значить
/// в двух местах разное.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Action {
    /// Ничего не делать.
    #[default]
    None = 0,
    /// Заглушить звук, картинку оставить.
    Mute = 1,
    /// Остановить воспроизведение (и звук, и картинку).
    Pause = 2,
    /// То же, что пауза, плюс вернуться в начало.
    Stop = 3,
}

impl Action {
    /// Из числа протокола; всё непонятное — «ничего не делать», а не
    /// случайная тишина.
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => Action::Mute,
            2 => Action::Pause,
            3 => Action::Stop,
            _ => Action::None,
        }
    }

    /// Число протокола.
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    /// Заглушён ли звук этим действием.
    pub fn mutes(self) -> bool {
        self != Action::None
    }

    /// Останавливается ли воспроизведение этим действием.
    pub fn pauses(self) -> bool {
        matches!(self, Action::Pause | Action::Stop)
    }
}

/// Имена условий — одни на оба хранилища.
///
/// Последние два (`gamemode`, `onBattery`) названы ровно так, как их зовёт
/// протокол чужого движка (повод паузы GameMode и питания от батареи): в его
/// карту таблица ложится одним и тем же [`AutoActions::to_map`], и своё
/// второе имя у одного и того же условия значило бы, что выбор человека до
/// демона не доезжает.
pub const CONDITIONS: [&str; 8] = [
    "anyWindow",
    "focused",
    "maximized",
    "fullscreen",
    "sessionLocked",
    "sessionInactive",
    "gamemode",
    "onBattery",
];

/// Таблица «условие → действие».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoActions {
    /// Открыто хоть какое-то окно.
    pub any_window: Action,
    /// Окно поверх обоев в фокусе.
    pub focused: Action,
    /// Окно развёрнуто во весь экран (обоев не видно).
    pub maximized: Action,
    /// Полноэкранное окно — игра или видео.
    pub fullscreen: Action,
    /// Сеанс заблокирован.
    pub session_locked: Action,
    /// Сеанс неактивен (переключились на другой).
    pub session_inactive: Action,
    /// Идёт игра: включён игровой режим (Feral GameMode) или запущен
    /// gamescope.
    pub gamemode: Action,
    /// Компьютер работает от батареи. На машине без батареи не бывает
    /// никогда.
    pub on_battery: Action,
}

impl Default for AutoActions {
    /// Умолчания продукта, а не пустая таблица.
    ///
    /// Жалоба, с которой начался этот модуль, звучала «обои орут поверх
    /// игры и видео», поэтому безопасное поведение включено сразу
    /// (правило #13: не спрашивать там, где неверный ответ ломает
    /// впечатление). Полноэкранное окно — это игра или кино: обои там
    /// не видно вовсе, значит и работать им незачем — пауза. Окно во весь
    /// экран — обоев тоже не видно, но пауза дала бы рывок при каждом
    /// сворачивании, поэтому только тишина.
    ///
    /// Батарея и игра — пауза, а не «ничего» (у чужого движка там «ничего»).
    /// Разница намеренная, решение Wehrwolfmann 15.11.2026: живые обои на
    /// ноутбуке от батареи — это заметно короче работа без розетки, а во
    /// время игры — кадры, отнятые у игры той же видеокартой. Человек,
    /// который этого не знает, чинить не пойдёт — он решит, что «ноутбук
    /// слабый» (правило #13: безопасное поведение по умолчанию). Пауза, а не
    /// «остановить»: вернулся к розетке — обои идут дальше с того же места,
    /// без рывка в начало.
    fn default() -> Self {
        Self {
            any_window: Action::None,
            focused: Action::None,
            maximized: Action::Mute,
            fullscreen: Action::Pause,
            session_locked: Action::Stop,
            session_inactive: Action::Stop,
            gamemode: Action::Pause,
            on_battery: Action::Pause,
        }
    }
}

impl AutoActions {
    /// Разложить в карту с именами условий (обе стороны — одна раскладка).
    pub fn to_map(self) -> Map<String, Value> {
        let mut m = Map::new();
        for (name, action) in CONDITIONS.iter().zip(self.list()) {
            m.insert((*name).to_string(), Value::from(action.as_i64()));
        }
        m
    }

    /// Собрать из карты; чего нет — берётся из умолчаний.
    pub fn from_map(m: &Map<String, Value>) -> Self {
        let pick = |name: &str, fallback: Action| -> Action {
            m.get(name)
                .and_then(Value::as_i64)
                .map(Action::from_i64)
                .unwrap_or(fallback)
        };
        let d = Self::default();
        Self {
            any_window: pick("anyWindow", d.any_window),
            focused: pick("focused", d.focused),
            maximized: pick("maximized", d.maximized),
            fullscreen: pick("fullscreen", d.fullscreen),
            session_locked: pick("sessionLocked", d.session_locked),
            session_inactive: pick("sessionInactive", d.session_inactive),
            gamemode: pick("gamemode", d.gamemode),
            on_battery: pick("onBattery", d.on_battery),
        }
    }

    /// Действия в порядке [`CONDITIONS`].
    pub fn list(self) -> [Action; 8] {
        [
            self.any_window,
            self.focused,
            self.maximized,
            self.fullscreen,
            self.session_locked,
            self.session_inactive,
            self.gamemode,
            self.on_battery,
        ]
    }

    /// Нужно ли вообще спрашивать про окна: хоть одно из «оконных» условий
    /// что-то делает. Опрос оконного менеджера — это процесс на каждом тике,
    /// и ради одной батареи его гонять незачем.
    pub fn windows_matter(self) -> bool {
        [self.any_window, self.focused, self.maximized, self.fullscreen]
            .iter()
            .any(|a| *a != Action::None)
    }

    /// Нужно ли спрашивать logind о сеансе (заблокирован / неактивен).
    pub fn session_matters(self) -> bool {
        self.session_locked != Action::None || self.session_inactive != Action::None
    }
}

// ── Наблюдаемое состояние рабочего стола ───────────────────────────────────

/// Что сейчас происходит вокруг обоев. Заполняет наблюдатель хоста; здесь
/// только решение по этим фактам.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Conditions {
    pub any_window: bool,
    pub focused: bool,
    pub maximized: bool,
    pub fullscreen: bool,
    pub session_locked: bool,
    pub session_inactive: bool,
    /// Идёт игра: игровой режим включён или запущен gamescope.
    pub gamemode: bool,
    /// Питание от батареи.
    pub on_battery: bool,
    /// Звук играет кто-то ещё (не мы).
    pub other_audio: bool,
}

impl Conditions {
    /// Флаги в порядке [`CONDITIONS`].
    pub fn list(self) -> [bool; 8] {
        [
            self.any_window,
            self.focused,
            self.maximized,
            self.fullscreen,
            self.session_locked,
            self.session_inactive,
            self.gamemode,
            self.on_battery,
        ]
    }
}

// ── Настройки ──────────────────────────────────────────────────────────────

/// Громкость по умолчанию, проценты.
///
/// У оригинала 100, у нас 50: обои — фон, и первое включение не должно
/// оглушать. Поднять до 100 человек может одним движением, а вот
/// «испугался и выключил навсегда» назад не отыгрывается.
pub const DEFAULT_VOLUME_PCT: i64 = 50;
/// Плавность приглушения по умолчанию, мс (как у чужого движка).
pub const DEFAULT_FADE_MS: i64 = 500;

/// Настройки звука обоев целиком.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioSettings {
    pub enabled: bool,
    pub volume_pct: i64,
    pub mute_when_other_audio: bool,
    pub fade_ms: i64,
    pub actions: AutoActions,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            volume_pct: DEFAULT_VOLUME_PCT,
            // У чужого движка это выключено по умолчанию, у нас включено.
            // Разница намеренная и та же по смыслу, что в `AutoActions`:
            // обои, перекрикивающие фильм, человек воспринимает как
            // поломку продукта, а не как настройку, которую он забыл
            // включить.
            mute_when_other_audio: true,
            fade_ms: DEFAULT_FADE_MS,
            actions: AutoActions::default(),
        }
    }
}

/// Зажать громкость в 0..100.
pub fn clamp_volume_pct(v: i64) -> i64 {
    v.clamp(0, 100)
}

impl AudioSettings {
    /// Прочитать из карты настроек Rexon (`settings.json`).
    pub fn from_map(m: &Map<String, Value>) -> Self {
        let d = Self::default();
        let actions = match m.get(KEY_ACTIONS) {
            Some(Value::Object(o)) => AutoActions::from_map(o),
            _ => d.actions,
        };
        Self {
            enabled: m.get(KEY_ENABLED).and_then(Value::as_bool).unwrap_or(d.enabled),
            volume_pct: clamp_volume_pct(
                m.get(KEY_VOLUME)
                    .and_then(Value::as_i64)
                    .unwrap_or(d.volume_pct),
            ),
            mute_when_other_audio: m
                .get(KEY_MUTE_OTHER)
                .and_then(Value::as_bool)
                .unwrap_or(d.mute_when_other_audio),
            fade_ms: m
                .get(KEY_FADE_MS)
                .and_then(Value::as_i64)
                .unwrap_or(d.fade_ms)
                .clamp(0, 5000),
            actions,
        }
    }

    /// Разложить в карту настроек Rexon.
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert(KEY_ENABLED.into(), Value::Bool(self.enabled));
        m.insert(KEY_VOLUME.into(), Value::from(self.volume_pct));
        m.insert(
            KEY_MUTE_OTHER.into(),
            Value::Bool(self.mute_when_other_audio),
        );
        m.insert(KEY_FADE_MS.into(), Value::from(self.fade_ms));
        m.insert(KEY_ACTIONS.into(), Value::Object(self.actions.to_map()));
        m
    }

    /// Прочитать из карты глобальных настроек ЧУЖОГО движка.
    ///
    /// Ровно те же значения под его именами — чтобы «громкость обоев»
    /// значила одно и то же по обе стороны и её нельзя было развести
    /// правкой одной из копий.
    pub fn from_engine_global(g: &Map<String, Value>) -> Self {
        let d = Self::default();
        let actions = match g.get(ENGINE_KEY_ACTIONS) {
            Some(Value::Object(o)) => AutoActions::from_map(o),
            _ => d.actions,
        };
        Self {
            enabled: g
                .get(ENGINE_KEY_ENABLED)
                .and_then(Value::as_bool)
                .unwrap_or(d.enabled),
            volume_pct: clamp_volume_pct(
                g.get(ENGINE_KEY_VOLUME)
                    .and_then(Value::as_i64)
                    .unwrap_or(d.volume_pct),
            ),
            mute_when_other_audio: g
                .get(ENGINE_KEY_MUTE_OTHER)
                .and_then(Value::as_bool)
                .unwrap_or(d.mute_when_other_audio),
            fade_ms: g
                .get(ENGINE_KEY_FADE_MS)
                .and_then(Value::as_i64)
                .unwrap_or(d.fade_ms),
            actions,
        }
    }

    /// Вписать значения в карту глобальных настроек чужого движка.
    ///
    /// ⚠️ Именно вписать в существующую, а не собрать новую: у того движка
    /// сохранение настроек — полная замена карты, и потерянный по дороге
    /// ключ стирается у человека насовсем.
    pub fn write_engine_global(&self, g: &mut Map<String, Value>) {
        g.insert(ENGINE_KEY_ENABLED.into(), Value::Bool(self.enabled));
        g.insert(ENGINE_KEY_VOLUME.into(), Value::from(self.volume_pct));
        g.insert(
            ENGINE_KEY_MUTE_OTHER.into(),
            Value::Bool(self.mute_when_other_audio),
        );
        g.insert(ENGINE_KEY_FADE_MS.into(), Value::from(self.fade_ms));
        g.insert(
            ENGINE_KEY_ACTIONS.into(),
            Value::Object(self.actions.to_map()),
        );
    }

    /// Громкость в долях единицы — в таком виде её ждёт проигрыватель.
    pub fn volume01(&self) -> f64 {
        clamp_volume_pct(self.volume_pct) as f64 / 100.0
    }

    /// Решение по текущей обстановке.
    pub fn decide(&self, c: &Conditions) -> Decision {
        // Самое строгое из сработавших действий и побеждает: «остановить»
        // сильнее «паузы», пауза сильнее тишины. Порядок задан порядком
        // вариантов `Action`, поэтому новая строка таблицы попадает в
        // решение сама, без правки этой функции.
        let mut worst = Action::None;
        for (action, active) in self.actions.list().iter().zip(c.list()) {
            if active && *action > worst {
                worst = *action;
            }
        }
        let muted_by_other = self.mute_when_other_audio && c.other_audio;
        Decision {
            // Звук отдаём, только когда его просили И ничто не велит молчать.
            audio_on: self.enabled && !worst.mutes() && !muted_by_other,
            paused: worst.pauses(),
            rewind: worst == Action::Stop,
            volume01: self.volume01(),
        }
    }
}

/// Простой вид настройки «останавливать обои, когда их не видно».
///
/// В интерфейсе продукта таблица «шесть условий × четыре действия» — это
/// экран для того, кто уже знает, чего хочет (правило #13: покупателю
/// нужно «легко и просто»). Поэтому наружу выходит одна галочка, а
/// разворачивает её в таблицу ядро — чтобы значение галочки и значения
/// таблицы не могли разойтись.
pub fn actions_for_hide_when_covered(on: bool) -> AutoActions {
    actions_for_choices(SimpleChoices {
        hide_when_covered: on,
        ..SimpleChoices::default()
    })
}

/// Обратный вопрос: включена ли эта галочка сейчас.
pub fn hide_when_covered(actions: &AutoActions) -> bool {
    actions.fullscreen != Action::None || actions.maximized != Action::None
}

/// Три галочки раздела «Обои» вместо таблицы «условие × действие».
///
/// Каждая галочка разворачивается в строки таблицы здесь, в ядре, — чтобы
/// окно настроек и хост обоев не могли понять одну и ту же галочку
/// по-разному (правило #12). Умолчание — все три включены: то же, что
/// [`AutoActions::default`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimpleChoices {
    /// «Останавливать обои, когда их не видно за окном».
    pub hide_when_covered: bool,
    /// «Ставить обои на паузу при работе от батареи».
    pub pause_on_battery: bool,
    /// «Ставить обои на паузу во время игры».
    pub pause_in_game: bool,
}

impl Default for SimpleChoices {
    fn default() -> Self {
        choices_of(&AutoActions::default())
    }
}

/// Галочки → таблица.
pub fn actions_for_choices(c: SimpleChoices) -> AutoActions {
    let d = AutoActions::default();
    let covered = |on: bool, action: Action| if on { action } else { Action::None };
    AutoActions {
        any_window: covered(c.hide_when_covered, d.any_window),
        focused: covered(c.hide_when_covered, d.focused),
        maximized: covered(c.hide_when_covered, d.maximized),
        fullscreen: covered(c.hide_when_covered, d.fullscreen),
        // Заблокированный и неактивный сеанс галочками не выключаются:
        // играть обоями в чужом сеансе или на замке — это не «настройка
        // на любителя», а разряженная батарея и звук из ниоткуда.
        session_locked: d.session_locked,
        session_inactive: d.session_inactive,
        gamemode: if c.pause_in_game { Action::Pause } else { Action::None },
        on_battery: if c.pause_on_battery { Action::Pause } else { Action::None },
    }
}

/// Таблица → галочки (что показать в окне настроек).
pub fn choices_of(actions: &AutoActions) -> SimpleChoices {
    SimpleChoices {
        hide_when_covered: hide_when_covered(actions),
        pause_on_battery: actions.on_battery != Action::None,
        pause_in_game: actions.gamemode != Action::None,
    }
}

/// Что хост должен сделать прямо сейчас.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decision {
    /// Держать звуковой поток открытым.
    ///
    /// ⚠️ Именно «поток есть / потока нет», а не «громкость 0». Открытый
    /// молчащий поток весь сеанс висит в микшере отдельным ползунком,
    /// который ничего не играет, и не даёт звуковой карте уснуть; кроме
    /// того, по нему невозможно отличить «работает тихо» от «не работает
    /// вовсе» — а ровно на этой неразличимости самопроверка обоев уже
    /// однажды зеленела там, где до звуковой карты не доходило ничего.
    pub audio_on: bool,
    /// Держать воспроизведение на паузе.
    pub paused: bool,
    /// Перемотать в начало (действие «остановить»).
    pub rewind: bool,
    /// Громкость в долях единицы.
    pub volume01: f64,
}

// ── Чужой звук ─────────────────────────────────────────────────────────────

/// По каким свойствам потока его вообще можно опознать.
///
/// ⚠️ `application.id` в списке ПЕРВЫМ не случайно: это единственное
/// свойство, которое доезжает до звуковой системы неизменным по ОБОИМ
/// дорогам — и у видео (клиент PipeWire от Qt Multimedia), и у Flash
/// (клиент ALSA от cpal). Остальные плагин ALSA переписывает по-своему.
pub const STREAM_ID_KEYS: [&str; 5] = [
    "application.id",
    "node.name",
    "application.name",
    "media.name",
    "application.process.binary",
];

/// Приметы НАШЕГО потока обоев — одни на все виды обоев и на оба режима
/// хоста.
///
/// ⭐ Починка 16.09.2026, найдена замером. Список жил в хосте и сверялся
/// ТОЧНЫМ равенством, поэтому узнавал только видео. Flash ходит в звук
/// клиентом ALSA (cpal), а плагин ALSA у PipeWire подписывает поток
/// по-своему: `node.name = "alsa_playback.rexon-wallpaper-host"`,
/// `application.name = "PipeWire ALSA [rexon-wallpaper-host]"`. Точного
/// совпадения там нет ни с одним прежним именем — и Flash-обои при
/// включённой по умолчанию настройке «приглушать, когда звук идёт из
/// другой программы» принимали СЕБЯ за чужую программу и глушили сами
/// себя. В журнале это выглядело как «звук играет другая программа» при
/// пустом рабочем столе, а для человека — просто «у Flash нет звука».
///
/// Поэтому: приметы сверяются вхождением, а не равенством, и живут в
/// ядре, а не в хосте, — чтобы третий вид обоев не завёл себе третью
/// копию списка (правило #12).
pub const OWN_STREAM_MARKERS: [&str; 4] = [
    STREAM_APP_ID,
    STREAM_NODE,
    LEGACY_STREAM_DESCRIPTION,
    // Прежнее имя потока видео-обоев: у людей оно ещё живёт в ячейках
    // памяти громкости звуковой системы.
    "RexonVideoWallpaper",
];

/// Команда, которой спрашивают у звуковой системы список потоков.
///
/// ⚠️ `LANG=C` обязателен: под русской локалью `pactl` переводит имена
/// полей, и разбор ниже перестаёт находить `Corked`.
pub fn sink_inputs_argv() -> Vec<String> {
    ["pactl", "list", "sink-inputs"]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

/// Играет ли звук кто-то, кроме нас.
///
/// Разбирается вывод [`sink_inputs_argv`]. Свои потоки узнаются по
/// приметам [`OWN_STREAM_MARKERS`] в свойствах [`STREAM_ID_KEYS`];
/// остановленные (`Corked: yes`) не в счёт — иначе за «чужой звук» сходил
/// бы любой поставленный на паузу проигрыватель, и обои молчали бы всегда.
///
/// ⚠️ Примета ищется ВХОЖДЕНИЕМ. Точное равенство здесь уже стоило нам
/// самозаглушающихся Flash-обоев: см. [`OWN_STREAM_MARKERS`].
pub fn other_audio_playing(stdout: &str, own_names: &[&str]) -> bool {
    let mut corked = false;
    let mut ours = false;
    let mut any = false;
    // Разбор поблочный: блок начинается со строки «Sink Input #N».
    let finish = |any: bool, ours: bool, corked: bool| any && !ours && !corked;
    for line in stdout.lines() {
        let t = line.trim();
        if t.starts_with("Sink Input #") {
            if finish(any, ours, corked) {
                return true;
            }
            corked = false;
            ours = false;
            any = true;
            continue;
        }
        if let Some(v) = t.strip_prefix("Corked:") {
            corked = v.trim().eq_ignore_ascii_case("yes");
        }
        if STREAM_ID_KEYS.iter().any(|k| t.starts_with(k)) {
            if let Some((_, value)) = t.split_once('=') {
                let value = value.trim().trim_matches('"');
                if own_names.iter().any(|n| value.contains(*n)) {
                    ours = true;
                }
            }
        }
    }
    finish(any, ours, corked)
}

// ── Опрос обстановки ───────────────────────────────────────────────────────

/// Заблокирован ли сеанс — один вопрос хранителю экрана.
pub fn screensaver_argv() -> Vec<String> {
    [
        "qdbus6",
        "org.freedesktop.ScreenSaver",
        "/ScreenSaver",
        "org.freedesktop.ScreenSaver.GetActive",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// Разбор ответа хранителя экрана.
pub fn parse_screensaver_active(stdout: &str) -> bool {
    stdout.trim().eq_ignore_ascii_case("true")
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}

/// Свойство D-Bus — вызовом `Properties.Get`, а не `busctl get-property`.
///
/// ⚠️ `--auto-start=no` обязателен, и именно поэтому `call`: служба
/// игрового режима запускается шиной по первому обращению. Опрос «идёт ли
/// игра» раз в пару секунд сам поднимал бы демон игрового режима на каждой
/// машине, где его поставили, но не пользуются. Ключ `--auto-start` у
/// `busctl` действует только на `call`.
fn dbus_property_argv(bus: &str, service: &str, path: &str, iface: &str, prop: &str) -> Vec<String> {
    argv(&[
        "busctl",
        bus,
        "--auto-start=no",
        "call",
        service,
        path,
        "org.freedesktop.DBus.Properties",
        "Get",
        "ss",
        iface,
        prop,
    ])
}

/// Работает ли машина от батареи — свойство `OnBattery` службы UPower
/// (системная шина). На компьютере без батареи UPower отвечает `false`.
pub fn upower_on_battery_argv() -> Vec<String> {
    dbus_property_argv(
        "--system",
        "org.freedesktop.UPower",
        "/org/freedesktop/UPower",
        "org.freedesktop.UPower",
        "OnBattery",
    )
}

/// Сколько программ попросили игровой режим — свойство `ClientCount`
/// службы Feral GameMode (шина сеанса). Больше нуля — идёт игра.
pub fn gamemode_client_count_argv() -> Vec<String> {
    dbus_property_argv(
        "--user",
        "com.feralinteractive.GameMode",
        "/com/feralinteractive/GameMode",
        "com.feralinteractive.GameMode",
        "ClientCount",
    )
}

/// Последнее слово ответа `busctl call … Properties.Get`: `v b true`,
/// `v i 2`. Пусто — ответа нет.
fn dbus_value(stdout: &str) -> Option<&str> {
    let mut words = stdout.split_whitespace();
    // Ответ начинается с подписи варианта `v` и типа значения.
    if words.next()? != "v" {
        return None;
    }
    words.nth(1)
}

/// Разбор логического свойства (`v b true`). Непонятный ответ — `None`.
pub fn parse_dbus_bool(stdout: &str) -> Option<bool> {
    match dbus_value(stdout)? {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Разбор целого свойства (`v i 2`, `v u 2`). Непонятный ответ — `None`.
pub fn parse_dbus_int(stdout: &str) -> Option<i64> {
    dbus_value(stdout)?.parse().ok()
}

/// Запасной путь для батареи, когда UPower нет или он молчит: состояние
/// батарей из `/sys/class/power_supply/BAT*/status`. От батареи — когда
/// хоть одна разряжается. Батарей нет — никогда.
pub fn sysfs_on_battery(statuses: &[String]) -> bool {
    statuses
        .iter()
        .any(|s| s.trim().eq_ignore_ascii_case("Discharging"))
}

/// Имена процесса gamescope в `/proc/PID/comm`. Отдельный gamescope на
/// рабочем столе — это игра, запущенная в нём (Steam так запускает игры с
/// параметром `gamescope %command%`), даже когда игровой режим не включён.
pub const GAMESCOPE_COMMS: [&str; 2] = ["gamescope", "gamescope-wl"];

/// Процесс с таким `comm` — gamescope.
pub fn is_gamescope_comm(comm: &str) -> bool {
    let comm = comm.trim();
    GAMESCOPE_COMMS.iter().any(|g| *g == comm)
}

/// Состояние сеанса — один вопрос logind: активен ли он и заблокирован ли.
///
/// `session_id` — `XDG_SESSION_ID` процесса. Если его нет (хост запущен
/// службой пользователя, вне сеанса) или он негоден для командной строки,
/// спрашиваем про `auto` — так logind называет сеанс вызывающего, а без
/// него — графический сеанс того же пользователя.
pub fn logind_session_argv(session_id: &str) -> Vec<String> {
    let id = if crate::privilege::revoke::valid_session_id(session_id) {
        session_id
    } else {
        "auto"
    };
    argv(&["loginctl", "show-session", id, "-p", "Active", "-p", "LockedHint"])
}

/// Разбор ответа logind в `(неактивен, заблокирован)`.
///
/// ⚠️ Непонятный ответ (нет logind, сеанс не нашёлся) — `(false, false)`:
/// «сеанс неактивен» по ошибке опроса оставил бы человека с вечно стоящими
/// обоями, и чинить это он не пошёл бы — решил бы, что обои сломаны.
pub fn parse_logind_session(stdout: &str) -> (bool, bool) {
    use crate::privilege::revoke::{parse_active_locked, SessionAnswer};
    match parse_active_locked(stdout) {
        SessionAnswer::Seen { active, locked } => (!active, locked),
        _ => (false, false),
    }
}

/// Разбор геометрии активного окна X11 в наблюдаемые условия.
///
/// Близнец [`kwin_window_conditions`] для X11, и отсев у них ОБЩИЙ
/// ([`crate::wallpaper_engine::window_is_wallpaper_layer`]), поэтому «окно
/// поверх обоев» по обе стороны сеанса значит одно и то же (правило #12).
///
/// ⚠️ Замер 16.09.2026 на стенде X11 с оконным менеджером: активным окном
/// `xdotool getactivewindow` называл САМИ обои — окно во весь экран с
/// `_NET_WM_WINDOW_TYPE_DESKTOP`. Правило «поверх играет полноэкранное
/// окно» срабатывало всегда, и Flash-обои давали 4 кадра вместо 240 за тот
/// же прогон.
///
/// `geometry` — вывод `xdotool … --shell`, `screen` — мерка экрана,
/// `own_pids` — процессы, чьи окна и есть обои. `window_props` зовётся
/// ЛЕНИВО и только когда окно вообще назвали; `None` (нет `xprop`, не
/// ответил) и непонятный ответ оставляют ПРЕЖНЕЕ поведение — окно
/// считается обычным. Ни «глушим всегда», ни «не глушим никогда».
pub fn x11_window_conditions(
    geometry: &str,
    screen: &str,
    own_pids: &[i64],
    window_props: impl FnOnce(i64) -> Option<String>,
) -> Conditions {
    use crate::wallpaper_engine as we;
    if geometry.trim().is_empty() {
        return Conditions::default();
    }
    if let Some(id) = we::parse_xdotool_window_id(geometry) {
        if let Some(props) = window_props(id) {
            if we::window_is_wallpaper_layer(&props, own_pids) {
                return Conditions::default();
            }
        }
    }
    // Отдельного признака «во весь экран» на X11 без лишних расспросов
    // нет: считаем по геометрии тем же правилом, что и на Wayland.
    let covers = we::parse_xdotool_fullscreen(geometry, screen);
    Conditions {
        any_window: true,
        focused: true,
        maximized: covers,
        fullscreen: covers,
        ..Conditions::default()
    }
}

/// Разбор словаря свойств окна KWin в наблюдаемые условия.
///
/// `fullscreen` KWin называет сам; «во весь экран» и «есть окно поверх»
/// считаются по геометрии — тем же способом, что и на X11, поэтому
/// поведение по обе стороны сессии одинаковое (правило #12). Размер экрана
/// приходит от окна обоев: оно ровно во весь экран и есть.
///
/// `own_pids` — процессы, чьи окна и есть обои: своё окно «окном поверх
/// обоев» не считается. Кого отсеиваем по типу окна, решает ядро движка
/// ([`crate::wallpaper_engine::kwin_window_is_wallpaper_layer`]) — список
/// один на продукт, и он же работает на X11.
pub fn kwin_window_conditions(
    info: &str,
    screen_w: i64,
    screen_h: i64,
    own_pids: &[i64],
) -> Conditions {
    let mut c = Conditions::default();
    let mut w = 0i64;
    let mut h = 0i64;
    let mut minimized = false;
    let mut have_window = false;
    for line in info.lines() {
        let t = line.trim();
        let Some((key, value)) = t.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "fullscreen" => {
                have_window = true;
                c.fullscreen = value == "true";
            }
            "minimized" => minimized = value == "true",
            "width" => w = value.parse().unwrap_or(0),
            "height" => h = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    if !have_window
        || minimized
        || crate::wallpaper_engine::kwin_window_is_wallpaper_layer(info, own_pids)
    {
        return Conditions::default();
    }
    c.any_window = true;
    c.focused = true;
    c.maximized = screen_w > 0 && screen_h > 0 && w >= screen_w && h >= screen_h;
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(json: &str) -> Map<String, Value> {
        match serde_json::from_str::<Value>(json).unwrap() {
            Value::Object(o) => o,
            _ => panic!("нужен объект"),
        }
    }

    #[test]
    fn роль_потока_не_музыкальная() {
        // Ровно та подмена, ради которой модуль и написан: обои с ролью
        // `music` попадают в общую ячейку восстановления громкости со
        // всеми музыкальными плеерами.
        assert_ne!(STREAM_ROLE.to_lowercase(), "music");
        let pw = pipewire_props();
        assert!(pw.contains("media.role = Movie"));
        assert!(pw.contains(&stream_description()), "в микшере — по-человечески");
        assert!(pw.contains(STREAM_APP_ID), "ключ памяти громкости обязателен");
        assert!(pw.contains(STREAM_NODE));
        // Роль в словаре PulseAudio называется иначе, и подмена обязана
        // ехать в обоих видах — иначе на машине без PipeWire снова музыка.
        assert!(pulse_props().contains("media.role=video"));
        // И для клиента ALSA — им в звук ходит Flash; без этой просьбы имя
        // потока переписывает плагин ALSA.
        assert!(alsa_props().contains(&stream_description()));
        assert!(alsa_props().contains(STREAM_NODE));
        assert_eq!(stream_env().len(), 3);
    }

    #[test]
    fn умолчания_не_дают_обоям_перекрикивать_игру() {
        let s = AudioSettings::default();
        assert!(s.enabled);
        assert_eq!(s.volume_pct, 50);
        assert!(s.mute_when_other_audio);
        assert_eq!(s.actions.fullscreen, Action::Pause);
        assert_eq!(s.actions.maximized, Action::Mute);
        // Спокойный стол — звук идёт.
        let d = s.decide(&Conditions::default());
        assert!(d.audio_on && !d.paused);
        assert_eq!(d.volume01, 0.5);
    }

    #[test]
    fn полноэкранное_окно_ставит_на_паузу_и_гасит_звук() {
        let s = AudioSettings::default();
        let c = Conditions {
            any_window: true,
            focused: true,
            maximized: true,
            fullscreen: true,
            ..Conditions::default()
        };
        let d = s.decide(&c);
        assert!(d.paused, "полноэкранное окно — обоев не видно");
        assert!(!d.audio_on, "пауза обязана снимать и звук");
        assert!(!d.rewind, "перематывает только «остановить»");
    }

    #[test]
    fn развёрнутое_окно_только_глушит() {
        let s = AudioSettings::default();
        let c = Conditions {
            any_window: true,
            focused: true,
            maximized: true,
            ..Conditions::default()
        };
        let d = s.decide(&c);
        assert!(!d.audio_on);
        assert!(!d.paused, "рывок при каждом разворачивании окна не нужен");
    }

    #[test]
    fn чужой_звук_глушит_обои() {
        let s = AudioSettings::default();
        let c = Conditions {
            other_audio: true,
            ..Conditions::default()
        };
        assert!(!s.decide(&c).audio_on);
        // Выключенная настройка — обои играют поверх, как их и просили.
        let s = AudioSettings {
            mute_when_other_audio: false,
            ..AudioSettings::default()
        };
        assert!(s.decide(&c).audio_on);
    }

    #[test]
    fn строгое_действие_побеждает_мягкое() {
        let s = AudioSettings {
            actions: AutoActions {
                maximized: Action::Mute,
                fullscreen: Action::Stop,
                ..AutoActions::default()
            },
            ..AudioSettings::default()
        };
        let c = Conditions {
            maximized: true,
            fullscreen: true,
            ..Conditions::default()
        };
        let d = s.decide(&c);
        assert!(d.paused && d.rewind && !d.audio_on);
    }

    #[test]
    fn выключенный_звук_не_включается_ничем() {
        let s = AudioSettings {
            enabled: false,
            ..AudioSettings::default()
        };
        assert!(!s.decide(&Conditions::default()).audio_on);
    }

    #[test]
    fn обе_стороны_читают_одни_значения() {
        // Значение, заданное в наших настройках, разложенное для чужого
        // движка и прочитанное обратно, обязано совпасть само с собой —
        // иначе «громкость обоев» опять значит разное в двух местах.
        let s = AudioSettings {
            enabled: false,
            volume_pct: 30,
            mute_when_other_audio: false,
            fade_ms: 250,
            actions: AutoActions {
                focused: Action::Mute,
                ..AutoActions::default()
            },
        };
        let mut engine = map(r#"{"queueMode":"sequential"}"#);
        s.write_engine_global(&mut engine);
        assert_eq!(AudioSettings::from_engine_global(&engine), s);
        // И чужие ключи на месте: сохранение настроек там — полная замена
        // карты, потерянный ключ стирается у человека.
        assert_eq!(engine.get("queueMode"), Some(&Value::from("sequential")));
        // Наше хранилище — тот же круг.
        let ours = s.to_map();
        assert_eq!(AudioSettings::from_map(&ours), s);
    }

    #[test]
    fn мусор_в_настройках_не_оставляет_без_звука() {
        let m = map(r#"{"wallpaper_audio_volume": 4000, "wallpaper_audio_enabled": "да"}"#);
        let s = AudioSettings::from_map(&m);
        assert_eq!(s.volume_pct, 100, "громкость зажимается, а не улетает");
        assert!(s.enabled, "нечисловое значение — умолчание, не тишина");
    }

    #[test]
    fn чужой_звук_узнаётся_по_выводу_pactl() {
        let ours = "\
Sink Input #353
\tMute: no
\tCorked: no
\tProperties:
\t\tmedia.role = \"animation\"
\t\tnode.name = \"RexonVideoWallpaper\"
";
        assert!(
            !other_audio_playing(ours, &["RexonVideoWallpaper"]),
            "свой поток чужим звуком не считается"
        );
        let firefox = format!(
            "{ours}Sink Input #400\n\tCorked: no\n\t\tapplication.name = \"Firefox\"\n"
        );
        assert!(other_audio_playing(&firefox, &["RexonVideoWallpaper"]));
        let paused = format!(
            "{ours}Sink Input #400\n\tCorked: yes\n\t\tapplication.name = \"Firefox\"\n"
        );
        assert!(
            !other_audio_playing(&paused, &["RexonVideoWallpaper"]),
            "остановленный проигрыватель — не играющий звук"
        );
        assert!(!other_audio_playing("", &["RexonVideoWallpaper"]));
    }

    #[test]
    fn поток_flash_обоев_узнаётся_своим() {
        // ⭐ Живой замер 16.09.2026 (`pactl list sink-inputs`, строки ниже
        // сняты дословно). Flash ходит в звук клиентом ALSA, и плагин ALSA
        // у PipeWire подписывает поток своими словами — точного совпадения
        // ни с одним нашим именем там нет. Пока приметы сверялись
        // равенством, Flash-обои при включённой по умолчанию настройке
        // «приглушать, когда звук идёт из другой программы» принимали СЕБЯ
        // за чужую программу и глушили сами себя.
        let flash = "\
Sink Input #72
\tCorked: no
\tProperties:
\t\tapplication.name = \"PipeWire ALSA [rexon-wallpaper-host]\"
\t\tnode.name = \"alsa_playback.rexon-wallpaper-host\"
\t\tmedia.name = \"ALSA Playback\"
\t\tapplication.id = \"com.rexon.wallpaper\"
";
        assert!(
            !other_audio_playing(flash, &OWN_STREAM_MARKERS),
            "поток Flash-обоев обязан быть узнан своим"
        );
        // Сам по себе `application.id` уже обязан хватать: у видео и у Flash
        // он один и до звуковой системы доезжает по обеим дорогам.
        let only_id = "Sink Input #1\n\tCorked: no\n\t\tapplication.id = \"com.rexon.wallpaper\"\n";
        assert!(!other_audio_playing(only_id, &OWN_STREAM_MARKERS));

        // Чужой проигрыватель, подписанный тем же плагином ALSA, остаётся
        // чужим: вхождение ищется по НАШИМ приметам, а не по слову «alsa».
        let alien = "Sink Input #1\n\tCorked: no\n\t\t                     application.name = \"PipeWire ALSA [mpv]\"\n\t\t                     node.name = \"alsa_playback.mpv\"\n";
        assert!(other_audio_playing(alien, &OWN_STREAM_MARKERS));
        assert_eq!(OWN_STREAM_MARKERS[0], STREAM_APP_ID);
    }

    #[test]
    fn состояние_окна_kwin_разбирается() {
        let maximized = "\
caption: ~ — Konsole
fullscreen: false
height: 1080
minimized: false
type: 0
width: 1920
";
        let c = kwin_window_conditions(maximized, 1920, 1080, &[]);
        assert!(c.any_window && c.focused && c.maximized && !c.fullscreen);

        let full = maximized.replace("fullscreen: false", "fullscreen: true");
        assert!(kwin_window_conditions(&full, 1920, 1080, &[]).fullscreen);

        // Сам рабочий стол — не окно поверх обоев.
        let desktop = maximized.replace("type: 0", "type: 1");
        assert_eq!(
            kwin_window_conditions(&desktop, 1920, 1080, &[]),
            Conditions::default()
        );

        // Панель/док — тоже не «окно поверх обоев»: полоска у края экрана
        // обои не закрывает, а прежний отсев знал только рабочий стол.
        let dock = maximized.replace("type: 0", "type: 2");
        assert_eq!(
            kwin_window_conditions(&dock, 1920, 1080, &[]),
            Conditions::default()
        );

        // Свёрнутое окно обоев не закрывает.
        let mini = maximized.replace("minimized: false", "minimized: true");
        assert_eq!(
            kwin_window_conditions(&mini, 1920, 1080, &[]),
            Conditions::default()
        );

        // Нет активного окна — пустой ответ, а не «полноэкранное».
        assert_eq!(
            kwin_window_conditions("", 1920, 1080, &[]),
            Conditions::default()
        );

        // Окно меньше экрана — обои видно.
        let small = maximized.replace("width: 1920", "width: 800");
        assert!(!kwin_window_conditions(&small, 1920, 1080, &[]).maximized);
    }

    /// Ответ `xdotool … --shell` для окна во весь экран 1280×800.
    fn окно_во_весь_экран(id: i64) -> String {
        format!("WINDOW={id}\nX=0\nY=0\nWIDTH=1280\nHEIGHT=800\nSCREEN=0\n")
    }

    #[test]
    fn окно_рабочего_стола_не_ставит_обои_на_паузу() {
        // ⭐ Замер 16.09.2026 на стенде X11 с оконным менеджером: активным
        // окном назывались САМИ обои — окно во весь экран с типом DESKTOP.
        // Flash-обои давали 4 кадра вместо 240 за тот же прогон.
        let g = окно_во_весь_экран(10485771);
        let desktop = "_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_DESKTOP\n\
                       _NET_WM_PID(CARDINAL) = 2742555\n";
        assert_eq!(
            x11_window_conditions(&g, "1280x800", &[], |_| Some(desktop.to_string())),
            Conditions::default(),
            "рабочий стол"
        );

        // Панель/док — тоже не «окно поверх обоев».
        let dock = "_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_DOCK\n";
        assert_eq!(
            x11_window_conditions(&g, "1280x800", &[], |_| Some(dock.to_string())),
            Conditions::default(),
            "панель"
        );

        // Своё окно обоев — тем более, даже если тип проставить не удалось.
        let no_type = "_NET_WM_WINDOW_TYPE:  not found.\n\
                       _NET_WM_PID(CARDINAL) = 2742555\n";
        assert_eq!(
            x11_window_conditions(&g, "1280x800", &[2742555], |_| Some(
                no_type.to_string()
            )),
            Conditions::default(),
            "свои обои"
        );

        // Настоящая игра во весь экран обои по-прежнему гасит.
        let game = "_NET_WM_WINDOW_TYPE(ATOM) = _NET_WM_WINDOW_TYPE_NORMAL\n\
                    _NET_WM_PID(CARDINAL) = 999\n";
        let c = x11_window_conditions(&g, "1280x800", &[2742555], |_| {
            Some(game.to_string())
        });
        assert!(c.fullscreen && c.maximized && c.any_window && c.focused, "игра");
    }

    #[test]
    fn отсев_окон_рабочего_стола_защищён_от_дурака() {
        let g = окно_во_весь_экран(10485771);

        // `xprop` не установлен или не ответил — поведение ПРЕЖНЕЕ: окно во
        // весь экран считается окном во весь экран. Ни «глушим всегда», ни
        // «не глушим никогда».
        assert!(x11_window_conditions(&g, "1280x800", &[], |_| None).fullscreen);

        // Ответ есть, но непонятный — тоже прежнее поведение.
        assert!(
            x11_window_conditions(&g, "1280x800", &[], |_| Some(
                "тарабарщина".to_string()
            ))
            .fullscreen
        );
        assert!(
            x11_window_conditions(&g, "1280x800", &[], |_| Some(String::new())).fullscreen
        );

        // `WINDOW=` в ответе нет — спрашивать не о чем, прежнее поведение,
        // и лишний процесс не запускается.
        let без_id = "X=0\nY=0\nWIDTH=1280\nHEIGHT=800\n";
        assert!(
            x11_window_conditions(без_id, "1280x800", &[], |_| panic!(
                "свойства спросили, хотя окно не назвали"
            ))
            .fullscreen
        );

        // Активного окна нет вовсе — обои играют.
        assert_eq!(
            x11_window_conditions("", "1280x800", &[], |_| panic!(
                "свойства спросили, хотя окна нет"
            )),
            Conditions::default()
        );

        // Окно МЕНЬШЕ экрана: окно есть, но обоев оно не закрывает.
        let маленькое = "WINDOW=7\nWIDTH=800\nHEIGHT=600\n";
        let c = x11_window_conditions(маленькое, "1280x800", &[], |_| Some(String::new()));
        assert!(c.any_window && c.focused && !c.maximized && !c.fullscreen);

        // Мерка экрана не разобралась — как и было, «не полноэкранное».
        assert!(
            !x11_window_conditions(&g, "мусор", &[], |_| Some(String::new())).fullscreen
        );
    }

    #[test]
    fn своё_окно_обоев_не_считается_окном_поверх_обоев() {
        // Тип окна KWin может не назвать вовсе (`type` в ответе нет) — тогда
        // остаётся хозяин окна. Процесс, который рисует обои, закрывать их
        // сам себе не может.
        let own = "\
caption: Rexon Wallpaper
fullscreen: true
height: 1080
minimized: false
pid: 4242
width: 1920
";
        assert_eq!(
            kwin_window_conditions(own, 1920, 1080, &[4242]),
            Conditions::default()
        );
        // Чужой процесс с тем же ответом обои закрывает, как и раньше.
        assert!(kwin_window_conditions(own, 1920, 1080, &[7]).fullscreen);
        // Про своих не знаем ничего — поведение прежнее.
        assert!(kwin_window_conditions(own, 1920, 1080, &[]).fullscreen);
    }

    #[test]
    fn галочка_разворачивается_в_таблицу_и_обратно() {
        let on = actions_for_hide_when_covered(true);
        assert!(hide_when_covered(&on));
        assert_eq!(on, AutoActions::default());
        let off = actions_for_hide_when_covered(false);
        assert!(!hide_when_covered(&off));
        // Замок и чужой сеанс галочкой не отключаются.
        assert_eq!(off.session_locked, Action::Stop);
        assert_eq!(off.session_inactive, Action::Stop);
    }

    #[test]
    fn умолчания_новых_условий_берегут_батарею_и_игру() {
        let a = AutoActions::default();
        assert_eq!(a.on_battery, Action::Pause, "от батареи — пауза");
        assert_eq!(a.gamemode, Action::Pause, "игра — пауза");
        assert!(a.session_inactive.pauses(), "чужой сеанс — обои стоят");
        assert_eq!(CONDITIONS.len(), a.list().len());
        assert_eq!(CONDITIONS.len(), Conditions::default().list().len());
        // Имена — ровно как у протокола чужого движка.
        assert_eq!(&CONDITIONS[6..], &["gamemode", "onBattery"]);
    }

    /// Каждое новое условие × каждое действие: решение то, что обещает
    /// действие, и только когда условие выполнено.
    #[test]
    fn новые_условия_на_все_действия() {
        type Set = fn(&mut AutoActions, Action);
        type Flag = fn(&mut Conditions);
        let rows: [(&str, Set, Flag); 3] = [
            ("батарея", |a, x| a.on_battery = x, |c| c.on_battery = true),
            ("игра", |a, x| a.gamemode = x, |c| c.gamemode = true),
            ("сеанс неактивен", |a, x| a.session_inactive = x, |c| c.session_inactive = true),
        ];
        for (name, set, flag) in rows {
            for action in [Action::None, Action::Mute, Action::Pause, Action::Stop] {
                // Остальные условия выключены — проверяется одна строка.
                let mut actions = actions_for_choices(SimpleChoices {
                    hide_when_covered: false,
                    pause_on_battery: false,
                    pause_in_game: false,
                });
                actions.session_locked = Action::None;
                actions.session_inactive = Action::None;
                set(&mut actions, action);
                let s = AudioSettings { actions, ..AudioSettings::default() };
                let mut c = Conditions::default();
                flag(&mut c);
                let d = s.decide(&c);
                assert_eq!(d.audio_on, !action.mutes(), "{name} × {action:?}: звук");
                assert_eq!(d.paused, action.pauses(), "{name} × {action:?}: пауза");
                assert_eq!(d.rewind, action == Action::Stop, "{name} × {action:?}: в начало");
                // Условие не выполнено — действие не срабатывает.
                let calm = s.decide(&Conditions::default());
                assert!(calm.audio_on && !calm.paused, "{name} × {action:?}: спокойно");
            }
        }
    }

    #[test]
    fn батарея_и_игра_по_умолчанию_ставят_на_паузу() {
        let s = AudioSettings::default();
        for c in [
            Conditions { on_battery: true, ..Conditions::default() },
            Conditions { gamemode: true, ..Conditions::default() },
            Conditions { session_inactive: true, ..Conditions::default() },
        ] {
            let d = s.decide(&c);
            assert!(d.paused && !d.audio_on, "{c:?}");
        }
        // Неактивный сеанс строже батареи: «остановить» побеждает «паузу».
        let both = Conditions { on_battery: true, session_inactive: true, ..Conditions::default() };
        assert!(s.decide(&both).rewind);
    }

    #[test]
    fn галочки_разворачиваются_в_таблицу_и_обратно() {
        for bits in 0..8u8 {
            let c = SimpleChoices {
                hide_when_covered: bits & 1 != 0,
                pause_on_battery: bits & 2 != 0,
                pause_in_game: bits & 4 != 0,
            };
            let a = actions_for_choices(c);
            assert_eq!(choices_of(&a), c, "{c:?}");
            // Замок и чужой сеанс галочками не выключаются никогда.
            assert_eq!(a.session_locked, Action::Stop);
            assert_eq!(a.session_inactive, Action::Stop);
        }
        assert_eq!(SimpleChoices::default(), choices_of(&AutoActions::default()));
        assert_eq!(actions_for_choices(SimpleChoices::default()), AutoActions::default());
        // Старая галочка «когда не видно» новых строк не трогает.
        let off = actions_for_hide_when_covered(false);
        assert_eq!(off.on_battery, Action::Pause);
        assert_eq!(off.gamemode, Action::Pause);
    }

    #[test]
    fn новые_условия_доезжают_до_протокола_чужого_движка() {
        let s = AudioSettings {
            actions: AutoActions {
                gamemode: Action::Stop,
                on_battery: Action::Mute,
                ..AutoActions::default()
            },
            ..AudioSettings::default()
        };
        let mut g = Map::new();
        s.write_engine_global(&mut g);
        let Some(Value::Object(policy)) = g.get(ENGINE_KEY_ACTIONS) else {
            panic!("нет таблицы")
        };
        // Та же карта, которую разбирает раскладка протокола демона.
        let pb = crate::we_settings::map_to_auto_replay(policy);
        assert_eq!(pb.gamemode, Action::Stop.as_i64() as i32);
        assert_eq!(pb.on_battery, Action::Mute.as_i64() as i32);
        assert_eq!(pb.session_inactive, Action::Stop.as_i64() as i32);
        assert_eq!(AudioSettings::from_engine_global(&g), s);
        // Прежняя запись без новых ключей — умолчания, а не «ничего».
        let old = map(r#"{"wallpaper_audio_actions": {"fullscreen": 0}}"#);
        let a = AudioSettings::from_map(&old).actions;
        assert_eq!((a.on_battery, a.gamemode), (Action::Pause, Action::Pause));
    }

    #[test]
    fn ответы_upower_и_gamemode_разбираются() {
        // Сняты с `busctl call … Properties.Get` дословно.
        assert_eq!(parse_dbus_bool("v b false\n"), Some(false));
        assert_eq!(parse_dbus_bool("v b true\n"), Some(true));
        assert_eq!(parse_dbus_int("v i 0\n"), Some(0));
        assert_eq!(parse_dbus_int("v i 2\n"), Some(2));
        // Служба не запущена: busctl пишет ошибку в stderr, stdout пуст.
        assert_eq!(parse_dbus_bool(""), None);
        assert_eq!(parse_dbus_int(""), None);
        assert_eq!(parse_dbus_bool("b true"), None, "без подписи варианта — не наш ответ");
        assert_eq!(parse_dbus_int("v i много"), None);
        // Шиной службу не поднимаем: игровой режим не стартует от опроса.
        for argv in [upower_on_battery_argv(), gamemode_client_count_argv()] {
            assert_eq!(argv[0], "busctl");
            assert!(argv.iter().any(|a| a == "--auto-start=no"), "{argv:?}");
            assert!(argv.iter().any(|a| a == "call"));
        }
        assert!(upower_on_battery_argv().contains(&"--system".to_string()));
        assert!(gamemode_client_count_argv().contains(&"--user".to_string()));
        assert_eq!(upower_on_battery_argv().last().map(String::as_str), Some("OnBattery"));
        assert_eq!(gamemode_client_count_argv().last().map(String::as_str), Some("ClientCount"));
    }

    #[test]
    fn батарея_из_sys_и_gamescope() {
        let st = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(sysfs_on_battery(&st(&["Discharging\n"])));
        assert!(!sysfs_on_battery(&st(&["Charging\n"])));
        assert!(!sysfs_on_battery(&st(&["Full\n", "Not charging\n"])));
        assert!(!sysfs_on_battery(&[]), "ПК без батареи — никогда");
        assert!(is_gamescope_comm("gamescope\n"));
        assert!(is_gamescope_comm("gamescope-wl\n"));
        assert!(!is_gamescope_comm("gamescope-session"));
        assert!(!is_gamescope_comm("steam"));
    }

    #[test]
    fn сеанс_logind_разбирается() {
        assert_eq!(parse_logind_session("Active=yes\nLockedHint=no\n"), (false, false));
        assert_eq!(parse_logind_session("Active=no\nLockedHint=no\n"), (true, false));
        assert_eq!(parse_logind_session("Active=yes\nLockedHint=yes\n"), (false, true));
        // logind молчит или сеанса нет — обои не встают навсегда.
        assert_eq!(parse_logind_session(""), (false, false));
        assert_eq!(parse_logind_session("мусор"), (false, false));
        assert_eq!(logind_session_argv("2")[2], "2");
        assert_eq!(logind_session_argv("")[2], "auto", "вне сеанса — свой графический сеанс");
        assert_eq!(logind_session_argv("--help")[2], "auto", "ключ под видом номера");
    }

    #[test]
    fn хранитель_экрана_разбирается() {
        assert!(parse_screensaver_active("true\n"));
        assert!(!parse_screensaver_active("false\n"));
        assert!(!parse_screensaver_active(""));
    }
}
