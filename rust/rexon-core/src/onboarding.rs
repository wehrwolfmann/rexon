//! Обучение при первом запуске — сценарий и его состояние.
//!
//! Человек, пришедший с Windows, открывает окно с шестью вкладками и не
//! понимает, с чего начать. Обучение ведёт его по цепочке: подсветить
//! нужное место, коротко объяснить, дать «Далее» и «Пропустить».
//!
//! Здесь — только данные и решения: из каких шагов состоит сценарий, к
//! чему шаг привязан, какой показывать следующим и когда обучение
//! закончено. Само затемнение, «прожектор» и кнопки рисует Qt-сторона.
//!
//! Шаги описаны ДАННЫМИ, а не кодом: добавить урок — дописать запись в
//! таблицу, не трогая ни одну вкладку.

use crate::i18n::{n_, tr, trf};

// ============================================================================
// Ключи настроек
// ============================================================================

/// Включено ли обучение. Отсутствие ключа означает «ещё не спрашивали» —
/// именно по нему первый запуск отличается от осознанного отказа.
pub const KEY_ENABLED: &str = "onboarding_enabled";

/// Идентификаторы уже показанных шагов. Каждый шаг показывается один раз.
pub const KEY_SEEN: &str = "onboarding_seen";

// ============================================================================
// Шаги
// ============================================================================

/// К чему привязана подсказка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// Заголовок вкладки в верхней полосе — подсвечивается сам «чип».
    Tab(&'static str),
    /// Содержимое вкладки целиком.
    TabContent(&'static str),
    /// Ни к чему: карточка по центру окна (приветствие, завершение).
    Window,
}

impl Anchor {
    /// Идентификатор вкладки, если шаг к ней привязан.
    pub fn tab_id(&self) -> Option<&'static str> {
        match self {
            Anchor::Tab(id) | Anchor::TabContent(id) => Some(id),
            Anchor::Window => None,
        }
    }

    /// Вид привязки строкой — Qt-стороне удобнее так.
    pub fn kind(&self) -> &'static str {
        match self {
            Anchor::Tab(_) => "tab",
            Anchor::TabContent(_) => "tab_content",
            Anchor::Window => "window",
        }
    }
}

/// Один шаг обучения.
///
/// `title` и `text` — строки каталога перевода (`msgid`, по-английски);
/// показывать их — через [`Step::title`]/[`Step::text`], не полями.
#[derive(Debug, Clone)]
pub struct Step {
    /// Устойчивый идентификатор: по нему запоминается «уже показан».
    /// Менять нельзя — иначе у людей урок покажется повторно.
    pub id: &'static str,
    pub anchor: Anchor,
    pub title: &'static str,
    pub text: &'static str,
}

impl Step {
    /// Заголовок шага на языке интерфейса.
    pub fn title(&self) -> String {
        tr(self.title)
    }

    /// Пояснение шага на языке интерфейса.
    pub fn text(&self) -> String {
        tr(self.text)
    }
}

/// Сценарий обучения по порядку показа.
///
/// Цепочка ведёт по тому, ради чего человек и ставит Rexon: сменить вид
/// рабочего стола, поставить живые обои, обезопасить себя от поломки и
/// узнать про возможности Steam.
pub const STEPS: &[Step] = &[
    Step {
        id: "welcome",
        anchor: Anchor::Window,
        title: n_("Welcome to Rexon"),
        text: n_(
            "Rexon changes how your desktop looks: themes, wallpapers, cursors, \
             fonts. I'll show you where everything is in a minute. You can skip \
             this and turn it on later in Settings."
        ),
    },
    Step {
        id: "tab_appearance",
        anchor: Anchor::Tab("appearance"),
        title: n_("Appearance"),
        text: n_(
            "The main tab. Here are the theme catalog, wallpapers, cursors and \
             fonts. A theme can be downloaded and applied in one click — no need \
             to search websites and unpack archives by hand."
        ),
    },
    Step {
        id: "appearance_catalog",
        anchor: Anchor::TabContent("appearance"),
        title: n_("Theme catalog"),
        text: n_(
            "Pick a section on the left; theme cards with previews are on the \
             right. The button on a card downloads the theme and applies it right \
             away. Rexon keeps your previous look, so you can always go back."
        ),
    },
    Step {
        id: "tab_wallpaper_engine",
        anchor: Anchor::Tab("wallpaper_engine"),
        title: n_("Live wallpapers"),
        text: n_(
            "Video and animated wallpapers — the same as Wallpaper Engine on \
             Windows. The Workshop is here too: wallpapers can be downloaded and \
             set right from it."
        ),
    },
    Step {
        id: "tab_desktops",
        anchor: Anchor::Tab("desktops"),
        title: n_("Desktops"),
        text: n_(
            "Install and switch desktops — KDE, GNOME, Cinnamon and others. The \
             current desktop can't be removed: otherwise there would be nothing \
             to log in to after a reboot."
        ),
    },
    Step {
        id: "settings_protection",
        anchor: Anchor::Tab("settings"),
        title: n_("App protection"),
        text: n_(
            "Switching desktops tends to reset your default apps — browser, \
             terminal, viewer. Protection in Settings remembers your choice and \
             brings it back. It is on from the start."
        ),
    },
    Step {
        id: "tab_steam",
        anchor: Anchor::Tab("steam"),
        title: n_("Steam features"),
        text: n_(
            "A startup movie when you log in — like on the Steam Deck — and \
             Steam profile backgrounds. You can pick a movie from the ones \
             already in your library."
        ),
    },
    Step {
        id: "finish",
        anchor: Anchor::Window,
        title: n_("Done"),
        text: n_(
            "That's all. You can take the tour again or turn it off in Settings. \
             If something goes wrong with the appearance, the Start menu has an \
             emergency recovery item."
        ),
    },
];

/// Шаг по идентификатору.
pub fn step(id: &str) -> Option<&'static Step> {
    STEPS.iter().find(|s| s.id == id)
}

/// Порядковый номер шага среди всех, начиная с единицы — для подписи
/// «шаг N из M».
pub fn step_position(id: &str) -> Option<usize> {
    STEPS.iter().position(|s| s.id == id).map(|i| i + 1)
}

/// Всего шагов в сценарии.
pub fn total_steps() -> usize {
    STEPS.len()
}

// ============================================================================
// Состояние
// ============================================================================

/// Что делать при запуске приложения.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// Первый запуск: спросить, включать ли обучение.
    Ask,
    /// Показать этот шаг.
    Show(&'static str),
    /// Ничего не делать: выключено или всё пройдено.
    Nothing,
}

/// Решение при запуске.
///
/// `enabled` — значение ключа настроек: `None` означает «ещё не
/// спрашивали». Отличать это от явного `Some(false)` важно: иначе человек,
/// однажды отказавшийся, получал бы вопрос при каждом запуске.
pub fn on_start(enabled: Option<bool>, seen: &[String]) -> Start {
    match enabled {
        None => Start::Ask,
        Some(false) => Start::Nothing,
        Some(true) => match next_step(seen) {
            Some(step) => Start::Show(step.id),
            None => Start::Nothing,
        },
    }
}

/// Следующий непоказанный шаг, либо `None`, если сценарий пройден.
pub fn next_step(seen: &[String]) -> Option<&'static Step> {
    STEPS.iter().find(|s| !seen.iter().any(|x| x == s.id))
}

/// Шаг, идущий за данным, — для кнопки «Далее».
pub fn step_after(current: &str, seen: &[String]) -> Option<&'static Step> {
    let Some(pos) = STEPS.iter().position(|s| s.id == current) else {
        return next_step(seen);
    };
    STEPS[pos + 1..]
        .iter()
        .find(|s| !seen.iter().any(|x| x == s.id))
}

/// Пометить шаг показанным. Повторов в списке не возникает, порядок
/// сохраняется — список читаемый, если открыть настройки руками.
pub fn mark_seen(seen: &mut Vec<String>, id: &str) {
    if !seen.iter().any(|x| x == id) {
        seen.push(id.to_string());
    }
}

/// Пройден ли сценарий целиком.
pub fn is_finished(seen: &[String]) -> bool {
    next_step(seen).is_none()
}

/// Список показанных шагов после отказа в середине обучения: пометить всё,
/// чтобы «Пропустить» не превращалось в вопрос при следующем запуске.
pub fn skip_all() -> Vec<String> {
    STEPS.iter().map(|s| s.id.to_string()).collect()
}

/// Сбросить прогресс — «пройти обучение заново» в Настройках.
pub fn reset() -> Vec<String> {
    Vec::new()
}

// ============================================================================
// Тексты интерфейса
// ============================================================================
//
// Функции, а не константы: перевод выбирается при показе (`crate::i18n`),
// и функцию, в отличие от константы, нельзя показать непереведённой.

/// Заголовок вопроса при первом запуске.
pub fn ask_title() -> String {
    tr("Show a short tour?")
}
/// Пояснение к вопросу.
pub fn ask_text() -> String {
    tr("It takes about a minute: I'll show where to change themes, wallpapers \
        and appearance. You can turn this on or off later in Settings.")
}
/// Кнопка согласия.
pub fn ask_yes() -> String {
    // TRANSLATORS: button that agrees to show the first-launch tour.
    tr("Show")
}
/// Кнопка отказа.
pub fn ask_no() -> String {
    // TRANSLATORS: button that declines the first-launch tour.
    tr("No, thanks")
}

/// Кнопка перехода к следующему шагу.
pub fn btn_next() -> String {
    tr("Next")
}
/// Кнопка на последнем шаге.
pub fn btn_done() -> String {
    tr("Done")
}
/// Кнопка отказа посреди обучения.
pub fn btn_skip() -> String {
    tr("Skip")
}

/// Подпись «шаг N из M».
pub fn progress_label(position: usize, total: usize) -> String {
    // TRANSLATORS: %1 is the current step, %2 is the number of steps.
    trf("Step %1 of %2", &[&position, &total])
}

/// Надпись переключателя в Настройках.
pub fn settings_label() -> String {
    tr("Tour on first launch")
}
/// Кнопка повторного прохождения.
pub fn settings_replay() -> String {
    tr("Take the tour again")
}

// ============================================================================
// Накладка: геометрия и палитра
// ============================================================================
//
// Числа накладки живут здесь, а не в Qt-шиме: вырез, зазор и радиус связаны
// между собой (карточка отступает от выреза ровно на зазор, вырез —
// на отступ вокруг цели), и разъехавшись, они дают то криво обрезанную
// подсветку, то карточку поверх подсвеченного места.

/// Затемнение вокруг подсвеченного места, RGBA. Своего числа у обучения
/// нет: гашение фона одно на продукт и живёт в `ui_base`.
pub const DIM_COLOR: (u8, u8, u8, u8) = crate::ui_base::MODAL_DIM_RGBA;
/// Неон подсветки скина по умолчанию, RGB. Живой цвет берётся у
/// ДЕЙСТВУЮЩЕГО скина — см. [`overlay_palette`]; эта запись осталась
/// эталоном для сверок.
pub const HALO_COLOR: (u8, u8, u8) = (102, 192, 244);
/// Толщина неоновой рамки вокруг выреза, px.
pub const HALO_WIDTH: i32 = 2;
/// Отступ выреза вокруг цели, px: подсвечивается не впритык к виджету.
pub const TARGET_PAD: i32 = 8;
/// Радиус скругления выреза и рамки, px.
pub const CORNER_RADIUS: i32 = 10;
/// Наибольшая ширина карточки, px: длинная строка нечитаема.
pub const CARD_MAX_WIDTH: i32 = 420;
/// Зазор между вырезом и карточкой, px. Он же — минимальный отступ
/// карточки от края окна.
pub const CARD_GAP: i32 = 14;
/// Фон карточки у скина по умолчанию. Живой цвет — [`overlay_palette`].
pub const CARD_BACKGROUND: &str = "#1b2838";

// ── стиль карточки: числа, которых больше нет в Qt-шиме ──────────────────
//
// Поля, зазоры, радиусы, кегли и толщины начертания лежали в
// `rexon/onboarding.py` вперемешку со строками QSS, хотя тот же модуль
// обещал в шапке, что чисел у него не осталось. Теперь обещание верно:
// шим только исполняет.

/// Поля карточки `(слева, сверху, справа, снизу)`, px.
pub const CARD_MARGINS: (i32, i32, i32, i32) = (18, 16, 18, 14);
/// Зазор между строками карточки, px.
pub const CARD_SPACING: i32 = 8;
/// Зазор между кнопками в нижнем ряду карточки, px.
pub const CARD_BUTTON_SPACING: i32 = 8;
/// Радиус скругления карточки, px (у выреза свой — [`CORNER_RADIUS`]).
pub const CARD_RADIUS: i32 = 12;
/// Насколько кегль подписи прогресса меньше кегля приложения и ниже какого
/// значения не опускается.
pub const PROGRESS_POINT_DELTA: i32 = -1;
/// Наименьший кегль подписи прогресса, pt.
pub const PROGRESS_POINT_MIN: i32 = 8;
/// Насколько кегль заголовка карточки больше кегля приложения.
pub const TITLE_POINT_DELTA: i32 = 3;
/// Наименьший кегль заголовка карточки, pt.
pub const TITLE_POINT_MIN: i32 = 14;
/// Начертание пояснения: тонкое на тёмном фоне нечитаемо.
pub const BODY_FONT_WEIGHT: i32 = 600;
/// Начертание надписи на кнопке «Далее».
pub const NEXT_FONT_WEIGHT: i32 = 700;
/// Радиус скругления кнопки «Далее», px.
pub const NEXT_RADIUS: i32 = 6;
/// Отступы внутри кнопки «Далее» `(сверху-снизу, слева-справа)`, px.
pub const NEXT_PADDING: (i32, i32) = (8, 20);
/// Отступы внутри кнопки «Пропустить» `(сверху-снизу, слева-справа)`, px.
pub const SKIP_PADDING: (i32, i32) = (6, 10);
/// Доля белого, подмешиваемая к акценту при наведении на «Далее».
pub const HOVER_LIGHTEN: f64 = 0.25;
/// Надпись на акцентной кнопке, когда сам акцент СВЕТЛЫЙ.
pub const ON_ACCENT_DARK: &str = "#0b1722";
/// Надпись на акцентной кнопке, когда сам акцент ТЁМНЫЙ.
pub const ON_ACCENT_LIGHT: &str = "#ffffff";

/// Палитра накладки: цвета берутся у скина, а не запекаются в обучение.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayPalette {
    /// Неон подсветки и заливка кнопки «Далее», RGB.
    pub halo: (u8, u8, u8),
    /// Тот же неон строкой `#rrggbb`.
    pub halo_hex: String,
    /// Заливка кнопки «Далее» при наведении.
    pub halo_hover: String,
    /// Фон карточки.
    pub card_background: String,
    /// Основной текст карточки (заголовок и пояснение).
    pub card_text: String,
    /// Приглушённый текст («шаг N из M», «Пропустить»).
    pub card_muted: String,
    /// Надпись на акцентной кнопке — контрастная к самому акценту.
    pub on_accent: &'static str,
}

/// Палитра накладки для НАЗВАННОГО скина.
///
/// ⚠️ Раньше все три цвета были запечены под `steam_dark`: покупатель,
/// выбравший «Светлый» или «Synthwave», получал тёмно-синюю карточку со
/// steam-голубой рамкой посреди своего окна. Тот же класс дефекта, что и
/// у страниц на QML, поэтому чинится тем же способом — цвета приезжают из
/// таблицы скинов (правило #12).
///
/// Приглушённый текст берётся из токена скина (`text_dim`), а не из
/// прежнего серо-голубого `#9fb4c6`: на тёмном фоне продукт держит только
/// белый, серые оттенки с него сливались.
pub fn overlay_palette_for(skin_id: &str) -> OverlayPalette {
    let colors = crate::ui_base::skin_colors(skin_id);
    let pick = |key: &str, fallback: &str| -> String {
        colors.get(key).cloned().unwrap_or_else(|| fallback.to_string())
    };
    let halo_hex = pick("accent", "#66c0f4");
    let halo = crate::ui_base::parse_hex(&halo_hex).unwrap_or(HALO_COLOR);
    let on_accent = if crate::ui_base::is_dark_rgb(halo.0, halo.1, halo.2) {
        ON_ACCENT_LIGHT
    } else {
        ON_ACCENT_DARK
    };
    OverlayPalette {
        halo,
        halo_hover: crate::ui_base::lighten_hex(&halo_hex, HOVER_LIGHTEN),
        halo_hex,
        card_background: pick("bg_primary", CARD_BACKGROUND),
        card_text: pick("text_primary", "#ffffff"),
        card_muted: pick("text_dim", "#ffffff"),
        on_accent,
    }
}

/// Палитра накладки для ДЕЙСТВУЮЩЕГО скина.
pub fn overlay_palette() -> OverlayPalette {
    overlay_palette_for(crate::ui_base::active_skin_id())
}

/// QSS карточки: фон, рамка неоном, скругление.
pub fn card_qss(pal: &OverlayPalette) -> String {
    format!(
        "QWidget#obCard {{ background: {}; border: {}px solid {}; \
border-radius: {}px; }}",
        pal.card_background, HALO_WIDTH, pal.halo_hex, CARD_RADIUS
    )
}

/// QSS подписи прогресса и любого другого приглушённого текста карточки.
pub fn muted_text_qss(pal: &OverlayPalette) -> String {
    format!("color: {};", pal.card_muted)
}

/// QSS заголовка карточки.
pub fn title_qss(pal: &OverlayPalette) -> String {
    format!("color: {};", pal.card_text)
}

/// QSS пояснения карточки.
pub fn body_qss(pal: &OverlayPalette) -> String {
    format!("color: {}; font-weight: {};", pal.card_text, BODY_FONT_WEIGHT)
}

/// QSS кнопки «Пропустить» — плоская надпись, акцентная под курсором.
///
/// ⚠️ Прежний переход «приглушённый → основной» на скине по умолчанию был
/// НЕВИДИМ: продукт держит на тёмном фоне только белый (правило владельца
/// от 11.06.2026), поэтому оба цвета там совпадают и наведение ничем не
/// отзывалось. Акцент отличается от текста на любом скине.
pub fn skip_button_qss(pal: &OverlayPalette) -> String {
    format!(
        "QPushButton {{ background: transparent; color: {}; border: none; \
padding: {}px {}px; }} QPushButton:hover {{ color: {}; }}",
        pal.card_muted, SKIP_PADDING.0, SKIP_PADDING.1, pal.halo_hex
    )
}

/// QSS кнопки «Далее» — заливка акцентом, контрастная надпись.
pub fn next_button_qss(pal: &OverlayPalette) -> String {
    format!(
        "QPushButton {{ background: {}; color: {}; font-weight: {}; \
border: none; border-radius: {}px; padding: {}px {}px; }} \
QPushButton:hover {{ background: {}; }}",
        pal.halo_hex,
        pal.on_accent,
        NEXT_FONT_WEIGHT,
        NEXT_RADIUS,
        NEXT_PADDING.0,
        NEXT_PADDING.1,
        pal.halo_hover
    )
}

/// Кегли карточки от кегля приложения: `(подпись прогресса, заголовок)`.
pub fn card_point_sizes(base: i32) -> (i32, i32) {
    (
        (base + PROGRESS_POINT_DELTA).max(PROGRESS_POINT_MIN),
        (base + TITLE_POINT_DELTA).max(TITLE_POINT_MIN),
    )
}

/// Где поставить карточку подсказки, в координатах накладки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardPlacement {
    pub x: i32,
    pub y: i32,
}

/// Прямоугольник выреза вокруг цели — порт `set_target`/`set_target_rect`:
/// цель расширяется на [`TARGET_PAD`] во все стороны.
///
/// `None` на входе (цели нет либо она невидима) даёт `None` и на выходе:
/// затемняется всё окно, карточка встаёт по центру.
pub fn spotlight_hole(target: Option<(i32, i32, i32, i32)>) -> Option<(i32, i32, i32, i32)> {
    let (x, y, w, h) = target?;
    if w <= 0 || h <= 0 {
        return None;
    }
    Some((
        x - TARGET_PAD,
        y - TARGET_PAD,
        w + TARGET_PAD * 2,
        h + TARGET_PAD * 2,
    ))
}

/// Куда поставить карточку, чтобы она НЕ накрыла подсвеченное место —
/// порт `_Spotlight._place_card`.
///
/// Порядок предпочтений сохранён: под вырезом, если карточка туда влезает;
/// иначе над ним; иначе по центру окна. По горизонтали карточка
/// центрируется по вырезу и прижимается к краям окна с зазором
/// [`CARD_GAP`].
///
/// `hole` = `None` — карточка ровно по центру окна.
pub fn card_placement(
    overlay: (i32, i32),
    card: (i32, i32),
    hole: Option<(i32, i32, i32, i32)>,
) -> CardPlacement {
    let (ow, oh) = overlay;
    let (cw, ch) = card;
    let Some((hx, hy, hw, hh)) = hole else {
        return CardPlacement {
            x: (ow - cw) / 2,
            y: (oh - ch) / 2,
        };
    };
    // QRect::bottom()/right() в Qt — последний пиксель внутри, а не за ним.
    let below = hy + hh - 1 + CARD_GAP;
    let above = hy - CARD_GAP - ch;
    let y = if below + ch <= oh {
        below
    } else if above >= 0 {
        above
    } else {
        ((oh - ch) / 2).max(0)
    };
    let center_x = hx + hw / 2;
    let x = (center_x - cw / 2).clamp(CARD_GAP, (ow - cw - CARD_GAP).max(CARD_GAP));
    CardPlacement { x, y }
}

// ============================================================================
// Содержимое карточки
// ============================================================================

/// Что написать в карточке шага.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardContent {
    /// Подпись «шаг N из M».
    pub progress: String,
    pub title: String,
    pub text: String,
    /// Подпись кнопки перехода: на последнем шаге — «Готово».
    pub next_text: String,
    /// Подпись кнопки отказа; на последнем шаге пустая.
    pub skip_text: String,
    /// Показывать ли кнопку отказа: на последнем шаге отказываться не от
    /// чего, а лишняя кнопка сбивает с толку (правило #13).
    pub skip_visible: bool,
}

/// Содержимое карточки для шага — порт развилки `Tour._show`.
///
/// Шаг, которого нет в сценарии, даёт `None`: показывать пустую карточку
/// хуже, чем не показывать ничего.
pub fn card_content(step_id: &str) -> Option<CardContent> {
    let step = step(step_id)?;
    let position = step_position(step_id)?;
    let total = total_steps();
    let last = position >= total;
    Some(CardContent {
        progress: progress_label(position, total),
        title: step.title(),
        text: step.text(),
        next_text: if last { btn_done() } else { btn_next() },
        skip_text: if last { String::new() } else { btn_skip() },
        skip_visible: !last,
    })
}

// ============================================================================
// Наведение «прожектора»
// ============================================================================

/// Что искать Qt-стороне, чтобы навести подсветку.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AimTarget {
    /// Ничего не искать: затемнить окно целиком, карточка по центру.
    WholeWindow,
    /// Заголовок вкладки в верхней полосе.
    TabHeader(&'static str),
    /// Содержимое вкладки; на неё же надо переключиться.
    TabContent(&'static str),
}

/// На что наводить подсветку для шага — порт `Tour._aim`.
///
/// Шаг без привязки к вкладке и незнакомый шаг дают
/// [`AimTarget::WholeWindow`]: пустая рамка мешает человеку сильнее, чем
/// её отсутствие.
pub fn aim_target(step_id: &str) -> AimTarget {
    match step(step_id).map(|s| s.anchor) {
        Some(Anchor::Tab(id)) => AimTarget::TabHeader(id),
        Some(Anchor::TabContent(id)) => AimTarget::TabContent(id),
        _ => AimTarget::WholeWindow,
    }
}

// ============================================================================
// Состояние в настройках
// ============================================================================

/// Значение ключа «обучение включено» из настроек.
///
/// Не булево (файл правили руками, там строка или число) читается как
/// «ещё не спрашивали»: вопрос повторится один раз и запишет честное
/// значение. Python-мост в этом месте падал на разборе аргумента и молча
/// не показывал ничего — починено здесь, для обоих входов.
pub fn enabled_from(settings: &serde_json::Map<String, serde_json::Value>) -> Option<bool> {
    settings.get(KEY_ENABLED).and_then(serde_json::Value::as_bool)
}

/// Показанные шаги из настроек, приведённые к списку строк.
pub fn seen_from(settings: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    normalize_seen(settings.get(KEY_SEEN))
}

/// Решение при запуске прямо по содержимому настроек.
pub fn start_from(settings: &serde_json::Map<String, serde_json::Value>) -> Start {
    on_start(enabled_from(settings), &seen_from(settings))
}

// ============================================================================
// Сеанс обучения поверх файла настроек
// ============================================================================
//
// Состояние живёт в ОБЩЕМ `settings.json` (`crate::settings`) под теми же
// ключами, что пишет Python-окно: человек, прошедший обучение в одном входе,
// не увидит его второй раз в другом. Каждый шаг сохраняется сразу, а не в
// конце: закрытое на середине окно не начинает обучение сначала.
//
// Функции принимают менеджер настроек снаружи, а не открывают файл сами:
// сверки гоняют их на временном файле, не трогая профиль человека. Живой
// вход — [`crate::settings::SettingsManager::new`].
//
// Ошибка записи возвращается ВТОРЫМ значением, а не вместо ответа: как и у
// Python-окна, несохранившийся прогресс — не повод обрывать обучение на
// полуслове, но причина обязана дойти до журнала.

use crate::settings::SettingsManager;

/// Что решено при запуске окна.
pub fn session_start(manager: &SettingsManager) -> Start {
    start_from(&manager.settings)
}

/// Ответ на вопрос первого запуска. Отказ запоминается — второй раз не
/// спрашиваем; согласие сразу даёт первый непоказанный шаг.
pub fn session_answer(manager: &mut SettingsManager, show: bool) -> (Start, std::io::Result<()>) {
    let saved = manager.set(KEY_ENABLED, serde_json::Value::Bool(show));
    let start = if show {
        session_start(manager)
    } else {
        Start::Nothing
    };
    (start, saved)
}

/// «Далее»: пометить текущий шаг показанным, сохранить и отдать следующий.
/// `None` — сценарий пройден.
pub fn session_advance(
    manager: &mut SettingsManager,
    current: &str,
) -> (Option<&'static Step>, std::io::Result<()>) {
    let mut seen = seen_from(&manager.settings);
    mark_seen(&mut seen, current);
    let next = step_after(current, &seen);
    let saved = manager.set(KEY_SEEN, seen_value(&seen));
    (next, saved)
}

/// «Пропустить»: пометить пройденным весь сценарий, иначе при следующем
/// запуске обучение полезло бы снова.
pub fn session_skip(manager: &mut SettingsManager) -> std::io::Result<()> {
    manager.set(KEY_SEEN, seen_value(&skip_all()))
}

/// «Пройти обучение заново» из Настроек: сбросить прогресс, включить
/// обучение и сразу решить, что показывать.
pub fn session_replay(manager: &mut SettingsManager) -> (Start, std::io::Result<()>) {
    let seen = manager.set(KEY_SEEN, seen_value(&reset()));
    let enabled = manager.set(KEY_ENABLED, serde_json::Value::Bool(true));
    (session_start(manager), seen.and(enabled))
}

fn seen_value(seen: &[String]) -> serde_json::Value {
    serde_json::Value::Array(
        seen.iter()
            .map(|s| serde_json::Value::String(s.clone()))
            .collect(),
    )
}

/// Привести значение ключа «показанные шаги» из настроек к списку строк —
/// порт `Tour._seen`.
///
/// Настройки — файл на диске, и там может оказаться что угодно (число,
/// строка, `null`): не список — начинаем с чистого листа, а не падаем.
pub fn normalize_seen(raw: Option<&serde_json::Value>) -> Vec<String> {
    let Some(serde_json::Value::Array(items)) = raw else {
        return Vec::new();
    };
    items
        .iter()
        .map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect()
}
