// AnimationsActions — the object of the "Animation" section of the
// "Settings" tab.
//
// There is no logic here: snapshot and writing are done by the core
// (`rexon_core::animations`) the same way as the `de_matrix_stand` test
// bench. The object only translates QML calls into core calls and hands
// back ready strings (the snapshot JSON, the result phrase). Writing is
// short (gsettings, kwriteconfig6, the compositor file and its check), so
// it happens right away, without a worker thread.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type AnimationsActions = super::AnimationsActionsRust;
    }

    extern "RustQt" {
        /// Section snapshot (JSON `animations::AnimState`).
        #[qinvokable]
        fn state(self: &AnimationsActions) -> QString;

        /// "No animations" off/on.
        #[qinvokable]
        #[cxx_name = "setEnabled"]
        fn set_enabled(self: &AnimationsActions, on: bool) -> QString;

        /// Speed slider: index of the `SPEED_STEPS` step.
        #[qinvokable]
        #[cxx_name = "setSpeedIndex"]
        fn set_speed_index(self: &AnimationsActions, index: i32) -> QString;

        /// Event effect: the row and the chosen option (for a check box
        /// `true`/`false`).
        #[qinvokable]
        #[cxx_name = "setRow"]
        fn set_row(self: &AnimationsActions, id: &QString, value: &QString) -> QString;

        /// "Defaults".
        #[qinvokable]
        fn defaults(self: &AnimationsActions) -> QString;

        /// The "Desktop effects" list (JSON `animations::DesktopEffectsState`;
        /// outside KDE — `available: false`).
        #[qinvokable]
        #[cxx_name = "effectsState"]
        fn effects_state(self: &AnimationsActions) -> QString;

        /// Enable or disable an effect of the list.
        #[qinvokable]
        #[cxx_name = "setEffect"]
        fn set_effect(self: &AnimationsActions, id: &QString, on: bool) -> QString;

        /// "Defaults" for the effects list.
        #[qinvokable]
        #[cxx_name = "effectsDefaults"]
        fn effects_defaults(self: &AnimationsActions) -> QString;

        /// "Configure…": the settings page of a KWin effect. Empty means it was
        /// opened.
        #[qinvokable]
        fn configure(self: &AnimationsActions, id: &QString) -> QString;
    }
}

use rexon_core::animations::{self as anim, AnimChange, LookCtx, SystemIo};

#[derive(Default)]
pub struct AnimationsActionsRust;

fn apply(change: AnimChange) -> cxx_qt_lib::QString {
    cxx_qt_lib::QString::from(&anim::apply_change(&SystemIo, &LookCtx::for_look(), &change))
}

impl qobject::AnimationsActions {
    fn state(&self) -> cxx_qt_lib::QString {
        let state = anim::read_state(&SystemIo, &LookCtx::for_look());
        cxx_qt_lib::QString::from(&serde_json::to_string(&state).unwrap_or_default())
    }

    fn set_enabled(&self, on: bool) -> cxx_qt_lib::QString {
        apply(AnimChange::Enabled(on))
    }

    fn set_speed_index(&self, index: i32) -> cxx_qt_lib::QString {
        match usize::try_from(index).ok().and_then(|i| anim::SPEED_STEPS.get(i)) {
            Some(f) => apply(AnimChange::Speed(*f)),
            None => cxx_qt_lib::QString::default(),
        }
    }

    fn set_row(&self, id: &cxx_qt_lib::QString, value: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        apply(AnimChange::Row { id: id.to_string(), value: value.to_string() })
    }

    fn defaults(&self) -> cxx_qt_lib::QString {
        apply(AnimChange::Defaults)
    }

    fn effects_state(&self) -> cxx_qt_lib::QString {
        let state = anim::desktop_effects_state(&SystemIo, &LookCtx::for_look());
        cxx_qt_lib::QString::from(&serde_json::to_string(&state).unwrap_or_default())
    }

    fn set_effect(&self, id: &cxx_qt_lib::QString, on: bool) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&anim::set_desktop_effect(&SystemIo, &LookCtx::for_look(), &id.to_string(), on))
    }

    fn effects_defaults(&self) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&anim::desktop_effects_defaults(&SystemIo, &LookCtx::for_look()))
    }

    fn configure(&self, id: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&anim::configure_effect(&SystemIo, &LookCtx::for_look(), &id.to_string()))
    }
}
