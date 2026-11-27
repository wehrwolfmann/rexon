// The wallpaper layer from Rust: a wrapper over LayerShellQt (C++ shim
// cpp/rexon_layer_shell.*).
//
// Why this is the first of the Qt subsystem shims: both wallpapers and the
// login video splash — the product's flagship feature — depend on
// wlr-layer-shell. While the layer was not in Rust, only the QML path on
// PySide6 could bring up a wallpaper window, and not a single Rust
// wallpaper path worked.
//
// Warning: that Python/QML path was removed from the tree on 2026-08-23 —
// there is nothing left to compare the properties below with by eye; they
// are the source of truth. Changing any of them changes the behaviour of
// both wallpapers and the splash at once:
//
//     LS.Window.layer: LS.Window.LayerBackground
//     LS.Window.anchors: AnchorTop | AnchorBottom | AnchorLeft | AnchorRight
//     LS.Window.exclusionZone: -1
//     LS.Window.keyboardInteractivity: KeyboardInteractivityNone
//
// Warning: a trap from the project canon: in QML the property is called
// `exclusionZone`, while the C++ setter is `setExclusiveZone`. The
// inconsistency is in the KDE library itself. Here the QML name
// (`exclusionZone`) is used, because that is what the vendored QML and our
// own see.
//
// Warning: second: the layer is set BEFORE the window is first shown.
// LayerShellQt replaces the shell integration when the platform window is
// created; a shown window has already become an ordinary xdg_toplevel and
// will not move into the layer. The test bench below checks this not on
// trust but by the bytes of the Wayland protocol.

/// cxx-qt bridge: setting up a wlr-layer-shell layer for an existing
/// QWindow.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    // The LayerShellQt shim. QObject does not need to be declared — cxx-qt
    // knows it by itself.
    unsafe extern "C++" {
        include!("rexon_layer_shell.h");

        /// The Qt platform is Wayland (on X11 there is no layer at all).
        fn rexon_layer_shell_available() -> bool;

        /// # Safety
        /// `window` must be a live QObject (usually a QWindow); call before the
        /// window is first shown.
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
        /// `window` must be a live QObject, already set up for the layer and not
        /// yet shown.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_pin_to_window_screen(window: *mut QObject) -> bool;

        /// # Safety
        /// `window` must be a live QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_screen_name(window: *mut QObject) -> QString;

        /// # Safety
        /// `window` must be a live QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_layer(window: *mut QObject) -> i32;
        /// # Safety
        /// `window` must be a live QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_anchors(window: *mut QObject) -> i32;
        /// # Safety
        /// `window` must be a live QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_exclusion_zone(window: *mut QObject) -> i32;
        /// # Safety
        /// `window` must be a live QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_layer_shell_keyboard(window: *mut QObject) -> i32;
    }

    extern "RustQt" {
        /// The layer configurator. An instance is created next to the window in QML
        /// and does not own it: the window lives its own life, here there is only
        /// its configuration.
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
        /// Whether a layer is possible in this session (the Qt platform is
        /// Wayland).
        #[qinvokable]
        fn available(self: &WallpaperLayer) -> bool;

        /// Give the window the CANONICAL wallpaper layer regardless of the object's
        /// properties. The only point where it is written what a "wallpaper" is
        /// (rule #12): all wallpaper paths must call it rather than assemble the
        /// values anew themselves.
        ///
        /// # Safety
        /// `window` is a live QWindow, not yet shown.
        #[qinvokable]
        #[cxx_name = "applyWallpaperLayer"]
        unsafe fn apply_wallpaper_layer(self: &WallpaperLayer, window: *mut QObject) -> bool;

        /// Give the window a layer according to the object's current properties —
        /// for cases that are not wallpapers (panels, overlays).
        ///
        /// # Safety
        /// `window` is a live QWindow, not yet shown.
        #[qinvokable]
        unsafe fn apply(self: &WallpaperLayer, window: *mut QObject) -> bool;

        /// Aim the layer surface at the screen of the window ITSELF (`Window.screen`
        /// in QML) and clear "let the compositor decide".
        ///
        /// Needed precisely for the second monitor: wallpapers occupy all screens,
        /// one window each, and each must land on its own output, not wherever the
        /// compositor sees fit. Call AFTER `applyWallpaperLayer` and BEFORE the
        /// window is first shown: the output goes into the `get_layer_surface`
        /// request.
        ///
        /// # Safety
        /// `window` is a live QWindow, already set up for the layer and not yet
        /// shown.
        #[qinvokable]
        #[cxx_name = "pinScreen"]
        unsafe fn pin_screen(self: &WallpaperLayer, window: *mut QObject) -> bool;

        /// Name of the screen the surface is aimed at; empty means no screen is
        /// named. For the self-test report: "the window is on its own monitor" and
        /// "the window came up at all" are different statements.
        ///
        /// # Safety
        /// `window` is a live QObject.
        #[qinvokable]
        #[cxx_name = "screenName"]
        unsafe fn screen_name(self: &WallpaperLayer, window: *mut QObject) -> QString;

        /// # Safety
        /// `window` is a live QObject.
        #[qinvokable]
        #[cxx_name = "readLayer"]
        unsafe fn read_layer(self: &WallpaperLayer, window: *mut QObject) -> i32;
        /// # Safety
        /// `window` is a live QObject.
        #[qinvokable]
        #[cxx_name = "readAnchors"]
        unsafe fn read_anchors(self: &WallpaperLayer, window: *mut QObject) -> i32;
        /// # Safety
        /// `window` is a live QObject.
        #[qinvokable]
        #[cxx_name = "readExclusionZone"]
        unsafe fn read_exclusion_zone(self: &WallpaperLayer, window: *mut QObject) -> i32;
        /// # Safety
        /// `window` is a live QObject.
        #[qinvokable]
        #[cxx_name = "readKeyboardInteractivity"]
        unsafe fn read_keyboard_interactivity(self: &WallpaperLayer, window: *mut QObject) -> i32;
    }
}

use cxx_qt_lib::QString;

/// Surface layer — LayerShellQt::Window::Layer.
pub mod layer {
    pub const BACKGROUND: i32 = 0;
    pub const BOTTOM: i32 = 1;
    pub const TOP: i32 = 2;
    pub const OVERLAY: i32 = 3;
}

/// Anchors — LayerShellQt::Window::Anchor, a bit mask.
pub mod anchor {
    pub const NONE: i32 = 0;
    pub const TOP: i32 = 1;
    pub const BOTTOM: i32 = 2;
    pub const LEFT: i32 = 4;
    pub const RIGHT: i32 = 8;
    /// All four sides — stretch over the whole output.
    pub const ALL: i32 = TOP | BOTTOM | LEFT | RIGHT;
}

/// Keyboard behaviour — LayerShellQt::Window::KeyboardInteractivity.
pub mod keyboard {
    pub const NONE: i32 = 0;
    pub const EXCLUSIVE: i32 = 1;
    pub const ON_DEMAND: i32 = 2;
}

/// Set of layer surface properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerSpec {
    pub layer: i32,
    pub anchors: i32,
    /// Exclusion zone. -1 = the surface reserves no space and is not pushed
    /// aside by panels — the only correct value for wallpapers.
    pub exclusion_zone: i32,
    pub keyboard_interactivity: i32,
}

impl LayerSpec {
    /// The canonical Rexon wallpaper layer. The values were taken from the
    /// former Python/QML path (its four files set the same thing); the path
    /// itself was removed from the tree, and these constants are their only
    /// record.
    pub const WALLPAPER: LayerSpec = LayerSpec {
        layer: layer::BACKGROUND,
        anchors: anchor::ALL,
        exclusion_zone: -1,
        keyboard_interactivity: keyboard::NONE,
    };
}

/// Rust state of the configurator. The default is the wallpaper layer: a
/// safe default under which a forgotten setting gives a wallpaper rather
/// than a window on top of everything (rule #13, foolproofing).
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

/// Give the window a layer by the description. An empty `scope` means leave
/// the LayerShellQt default alone (the compositor uses it for ordering
/// within the layer).
///
/// # Safety
/// `window` must point to a live QWindow not yet shown on screen.
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

/// Aim the layer surface at the window's own screen (`QWindow::screen()`)
/// and clear "let the compositor choose".
///
/// Needed where there are several wallpaper windows — one per monitor:
/// without an explicit output the compositor is free to put all surfaces on
/// one screen.
///
/// # Safety
/// `window` must point to a live QWindow that already has a layer
/// ([`apply_spec`]) and is not yet shown.
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
        let ok = unsafe { apply_spec(window, &LayerSpec::WALLPAPER, &self.scope) };
        if ok {
            // Cinnamon with layer-shell: the shell's background service is older than
            // our window and would cover it — put it under us. Here this is only a
            // fallback order; the immediate one goes on the "first frame on screen"
            // notice (`wallpaper_host::first_frame_shown`).
            crate::wallpaper_host::cinnamon_layer::surface_up("окно обоев слоем стола");
        }
        ok
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
