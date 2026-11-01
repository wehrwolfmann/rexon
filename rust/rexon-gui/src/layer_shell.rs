// Слой обоев из Rust: обёртка над LayerShellQt (C++-шим cpp/rexon_layer_shell.*).
//
// Зачем это первым из шимов Qt-подсистем: от wlr-layer-shell зависят и обои,
// и видеозаставка входа — флагманская фича продукта. Пока слоя не было в
// Rust, окно обоев умел поднять только QML-путь на PySide6, и ни один
// Rust-путь обоев не работал.
//
// ⚠️ Тот Python/QML-путь снят с дерева 2026-08-23 — свойства ниже уже не с
// чем сверять глазами, они и есть источник истины. Правка любого из них
// меняет поведение обоев и заставки разом:
//
//     LS.Window.layer: LS.Window.LayerBackground
//     LS.Window.anchors: AnchorTop | AnchorBottom | AnchorLeft | AnchorRight
//     LS.Window.exclusionZone: -1
//     LS.Window.keyboardInteractivity: KeyboardInteractivityNone
//
// ⚠️ Грабля из канона проекта: в QML свойство называется `exclusionZone`, а
// сеттер в C++ — `setExclusiveZone`. Разнобой в самой библиотеке KDE. Здесь
// принято имя из QML (`exclusionZone`), потому что именно оно на виду у
// вендорного QML и у нашего.
//
// ⚠️ Второе: слой ставится ДО первого показа окна. LayerShellQt подменяет
// интеграцию оболочки при создании платформенного окна; показанное окно уже
// стало обычным xdg_toplevel и в слой не переселится. Стенд ниже проверяет
// это не на слово, а по байтам протокола Wayland.

/// Мост cxx-qt: настройка слоя wlr-layer-shell для существующего QWindow.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    // Шим LayerShellQt. QObject объявлять не надо — cxx-qt знает его сам.
    unsafe extern "C++" {
        include!("rexon_layer_shell.h");

        /// Платформа Qt — Wayland (на X11 слоя нет вообще).
        fn rexon_layer_shell_available() -> bool;

        /// # Safety
        /// `window` обязан быть живым QObject (обычно QWindow); вызывать до
        /// первого показа окна.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_configure(
            window: *mut QObject,
            layer: i32,
            anchors: i32,
            exclusion_zone: i32,
            keyboard: i32,
            scope: &QString,
        ) -> bool;

        /// # Safety
        /// `window` обязан быть живым QObject, уже настроенным на слой и
        /// ещё не показанным.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_pin_to_window_screen(window: *mut QObject) -> bool;

        /// # Safety
        /// `window` обязан быть живым QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_screen_name(window: *mut QObject) -> QString;

        /// # Safety
        /// `window` обязан быть живым QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_layer(window: *mut QObject) -> i32;
        /// # Safety
        /// `window` обязан быть живым QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_anchors(window: *mut QObject) -> i32;
        /// # Safety
        /// `window` обязан быть живым QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_exclusion_zone(window: *mut QObject) -> i32;
        /// # Safety
        /// `window` обязан быть живым QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_keyboard(window: *mut QObject) -> i32;
    }

    extern "RustQt" {
        /// Настройщик слоя. Экземпляр создаётся рядом с окном в QML и не
        /// владеет им: окно живёт своей жизнью, здесь только его настройка.
        #[qobject]
        #[qml_element]
        #[qproperty(i32, layer)]
        #[qproperty(i32, anchors)]
        #[qproperty(i32, exclusion_zone, cxx_name = "exclusionZone")]
        #[qproperty(i32, keyboard_interactivity, cxx_name = "keyboardInteractivity")]
        #[qproperty(QString, scope)]
        type WallpaperLayer = super::WallpaperLayerRust;
    }

    extern "RustQt" {
        /// Возможен ли слой в этой сессии (платформа Qt — Wayland).
        #[qinvokable]
        fn available(self: &WallpaperLayer) -> bool;

        /// Поставить окну КАНОНИЧЕСКИЙ слой обоев, не глядя на свойства
        /// объекта. Единственная точка, где записано, что такое «обои»
        /// (правило #12): все пути обоев обязаны звать её, а не собирать
        /// значения заново у себя.
        ///
        /// # Safety
        /// `window` — живой QWindow, ещё не показанный.
        #[qinvokable]
        #[cxx_name = "applyWallpaperLayer"]
        unsafe fn apply_wallpaper_layer(self: &WallpaperLayer, window: *mut QObject) -> bool;

        /// Поставить окну слой по текущим свойствам объекта — для случаев,
        /// которые обоями не являются (панели, оверлеи).
        ///
        /// # Safety
        /// `window` — живой QWindow, ещё не показанный.
        #[qinvokable]
        unsafe fn apply(self: &WallpaperLayer, window: *mut QObject) -> bool;

        /// Прицелить поверхность слоя на экран САМОГО окна
        /// (`Window.screen` в QML) и снять «пусть решает композитор».
        ///
        /// Нужно ровно для второго монитора: обоями заняты все экраны, по
        /// окну на каждый, и каждое обязано лечь на свой выход, а не туда,
        /// куда композитор сочтёт нужным. Звать ПОСЛЕ
        /// `applyWallpaperLayer` и ДО первого показа окна: выход уходит в
        /// запрос `get_layer_surface`.
        ///
        /// # Safety
        /// `window` — живой QWindow, уже настроенный на слой и ещё не
        /// показанный.
        #[qinvokable]
        #[cxx_name = "pinScreen"]
        unsafe fn pin_screen(self: &WallpaperLayer, window: *mut QObject) -> bool;

        /// Имя экрана, на который нацелена поверхность; пусто — экран не
        /// назван. Для отчёта самопроверки: «окно на своём мониторе» и
        /// «окно вообще поднялось» — разные утверждения.
        ///
        /// # Safety
        /// `window` — живой QObject.
        #[qinvokable]
        #[cxx_name = "screenName"]
        unsafe fn screen_name(self: &WallpaperLayer, window: *mut QObject) -> QString;

        /// # Safety
        /// `window` — живой QObject.
        #[qinvokable]
        #[cxx_name = "readLayer"]
        unsafe fn read_layer(self: &WallpaperLayer, window: *mut QObject) -> i32;
        /// # Safety
        /// `window` — живой QObject.
        #[qinvokable]
        #[cxx_name = "readAnchors"]
        unsafe fn read_anchors(self: &WallpaperLayer, window: *mut QObject) -> i32;
        /// # Safety
        /// `window` — живой QObject.
        #[qinvokable]
        #[cxx_name = "readExclusionZone"]
        unsafe fn read_exclusion_zone(self: &WallpaperLayer, window: *mut QObject) -> i32;
        /// # Safety
        /// `window` — живой QObject.
        #[qinvokable]
        #[cxx_name = "readKeyboardInteractivity"]
        unsafe fn read_keyboard_interactivity(self: &WallpaperLayer, window: *mut QObject) -> i32;
    }
}

use cxx_qt_lib::QString;

/// Слой поверхности — LayerShellQt::Window::Layer.
pub mod layer {
    pub const BACKGROUND: i32 = 0;
    pub const BOTTOM: i32 = 1;
    pub const TOP: i32 = 2;
    pub const OVERLAY: i32 = 3;
}

/// Якоря — LayerShellQt::Window::Anchor, битовая маска.
pub mod anchor {
    pub const NONE: i32 = 0;
    pub const TOP: i32 = 1;
    pub const BOTTOM: i32 = 2;
    pub const LEFT: i32 = 4;
    pub const RIGHT: i32 = 8;
    /// Все четыре стороны — растянуть на весь выход.
    pub const ALL: i32 = TOP | BOTTOM | LEFT | RIGHT;
}

/// Реакция на клавиатуру — LayerShellQt::Window::KeyboardInteractivity.
pub mod keyboard {
    pub const NONE: i32 = 0;
    pub const EXCLUSIVE: i32 = 1;
    pub const ON_DEMAND: i32 = 2;
}

/// Набор свойств поверхности слоя.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerSpec {
    pub layer: i32,
    pub anchors: i32,
    /// Зона вытеснения. -1 = поверхность не резервирует места и не
    /// вытесняется панелями — единственное правильное значение для обоев.
    pub exclusion_zone: i32,
    pub keyboard_interactivity: i32,
}

impl LayerSpec {
    /// Канонический слой обоев Rexon. Значения сняты с прежнего
    /// Python/QML-пути (четыре его файла задавали одно и то же); сам путь
    /// снят с дерева, эти константы — единственная их запись.
    pub const WALLPAPER: LayerSpec = LayerSpec {
        layer: layer::BACKGROUND,
        anchors: anchor::ALL,
        exclusion_zone: -1,
        keyboard_interactivity: keyboard::NONE,
    };
}

/// Rust-состояние настройщика. По умолчанию — слой обоев: безопасное
/// умолчание, при котором забытая настройка даёт обои, а не окно поверх
/// всего (правило #13, защита от дурака).
pub struct WallpaperLayerRust {
    layer: i32,
    anchors: i32,
    exclusion_zone: i32,
    keyboard_interactivity: i32,
    scope: QString,
}

impl Default for WallpaperLayerRust {
    fn default() -> Self {
        let spec = LayerSpec::WALLPAPER;
        Self {
            layer: spec.layer,
            anchors: spec.anchors,
            exclusion_zone: spec.exclusion_zone,
            keyboard_interactivity: spec.keyboard_interactivity,
            scope: QString::default(),
        }
    }
}

/// Настроить окну слой по описанию. Пустой `scope` — не трогать умолчание
/// LayerShellQt (композитор использует его для порядка внутри слоя).
///
/// # Safety
/// `window` обязан указывать на живой QWindow, ещё не показанный на экране.
pub unsafe fn apply_spec(window: *mut qobject::QObject, spec: &LayerSpec, scope: &QString) -> bool {
    if window.is_null() {
        return false;
    }
    unsafe {
        qobject::rexon_layer_shell_configure(
            window,
            spec.layer,
            spec.anchors,
            spec.exclusion_zone,
            spec.keyboard_interactivity,
            scope,
        )
    }
}

/// Прицелить поверхность слоя на экран самого окна (`QWindow::screen()`)
/// и снять «пусть выбирает композитор».
///
/// Нужно там, где окон обоев несколько — по одному на монитор: без явного
/// выхода композитор вправе положить все поверхности на один экран.
///
/// # Safety
/// `window` обязан указывать на живой QWindow, которому уже поставлен слой
/// ([`apply_spec`]) и который ещё не показан.
pub unsafe fn pin_to_window_screen(window: *mut qobject::QObject) -> bool {
    if window.is_null() {
        return false;
    }
    unsafe { qobject::rexon_layer_shell_pin_to_window_screen(window) }
}

impl qobject::WallpaperLayer {
    fn available(&self) -> bool {
        qobject::rexon_layer_shell_available()
    }

    unsafe fn apply_wallpaper_layer(&self, window: *mut qobject::QObject) -> bool {
        unsafe { apply_spec(window, &LayerSpec::WALLPAPER, &self.scope) }
    }

    unsafe fn apply(&self, window: *mut qobject::QObject) -> bool {
        let spec = LayerSpec {
            layer: self.layer,
            anchors: self.anchors,
            exclusion_zone: self.exclusion_zone,
            keyboard_interactivity: self.keyboard_interactivity,
        };
        unsafe { apply_spec(window, &spec, &self.scope) }
    }

    unsafe fn pin_screen(&self, window: *mut qobject::QObject) -> bool {
        if window.is_null() {
            return false;
        }
        unsafe { qobject::rexon_layer_shell_pin_to_window_screen(window) }
    }

    unsafe fn screen_name(&self, window: *mut qobject::QObject) -> QString {
        if window.is_null() {
            return QString::default();
        }
        unsafe { qobject::rexon_layer_shell_screen_name(window) }
    }

    unsafe fn read_layer(&self, window: *mut qobject::QObject) -> i32 {
        if window.is_null() {
            return -1;
        }
        unsafe { qobject::rexon_layer_shell_layer(window) }
    }

    unsafe fn read_anchors(&self, window: *mut qobject::QObject) -> i32 {
        if window.is_null() {
            return -1;
        }
        unsafe { qobject::rexon_layer_shell_anchors(window) }
    }

    unsafe fn read_exclusion_zone(&self, window: *mut qobject::QObject) -> i32 {
        if window.is_null() {
            return i32::MIN;
        }
        unsafe { qobject::rexon_layer_shell_exclusion_zone(window) }
    }

    unsafe fn read_keyboard_interactivity(&self, window: *mut qobject::QObject) -> i32 {
        if window.is_null() {
            return -1;
        }
        unsafe { qobject::rexon_layer_shell_keyboard(window) }
    }
}
