// AnimationsActions — объект раздела «Анимация» вкладки «Настройки».
//
// Логики здесь нет: снимок и запись делает ядро (`rexon_core::animations`)
// тем же путём, что стенд `de_matrix_stand`. Объект только переводит вызовы
// QML в вызовы ядра и отдаёт готовые строки (JSON снимка, фраза итога).
// Запись короткая (gsettings, kwriteconfig6, файл композитора и его
// проверка), поэтому идёт сразу, без рабочего потока.

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
        /// Снимок раздела (JSON `animations::AnimState`).
        #[qinvokable]
        fn state(self: &AnimationsActions) -> QString;

        /// «Без анимаций» выкл/вкл.
        #[qinvokable]
        #[cxx_name = "setEnabled"]
        fn set_enabled(self: &AnimationsActions, on: bool) -> QString;

        /// Ползунок скорости: индекс ступени `SPEED_STEPS`.
        #[qinvokable]
        #[cxx_name = "setSpeedIndex"]
        fn set_speed_index(self: &AnimationsActions, index: i32) -> QString;

        /// Эффект события: строка и выбранный вариант (у флажка `true`/`false`).
        #[qinvokable]
        #[cxx_name = "setRow"]
        fn set_row(self: &AnimationsActions, id: &QString, value: &QString) -> QString;

        /// «По умолчанию».
        #[qinvokable]
        fn defaults(self: &AnimationsActions) -> QString;

        /// Список «Эффекты рабочего стола» (JSON
        /// `animations::DesktopEffectsState`; вне KDE — `available: false`).
        #[qinvokable]
        #[cxx_name = "effectsState"]
        fn effects_state(self: &AnimationsActions) -> QString;

        /// Включить или выключить эффект списка.
        #[qinvokable]
        #[cxx_name = "setEffect"]
        fn set_effect(self: &AnimationsActions, id: &QString, on: bool) -> QString;

        /// «По умолчанию» для списка эффектов.
        #[qinvokable]
        #[cxx_name = "effectsDefaults"]
        fn effects_defaults(self: &AnimationsActions) -> QString;

        /// «Настроить…»: страница настроек эффекта KWin. Пусто — открыта.
        #[qinvokable]
        fn configure(self: &AnimationsActions, id: &QString) -> QString;
    }
}

use rexon_core::animations::{self as anim, AnimChange, LookCtx, SystemIo};

#[derive(Default)]
pub struct AnimationsActionsRust;

fn apply(change: AnimChange) -> cxx_qt_lib::QString {
    cxx_qt_lib::QString::from(&anim::apply_change(&SystemIo, &LookCtx::from_env(), &change))
}

impl qobject::AnimationsActions {
    fn state(&self) -> cxx_qt_lib::QString {
        let state = anim::read_state(&SystemIo, &LookCtx::from_env());
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
        let state = anim::desktop_effects_state(&SystemIo, &LookCtx::from_env());
        cxx_qt_lib::QString::from(&serde_json::to_string(&state).unwrap_or_default())
    }

    fn set_effect(&self, id: &cxx_qt_lib::QString, on: bool) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&anim::set_desktop_effect(&SystemIo, &LookCtx::from_env(), &id.to_string(), on))
    }

    fn effects_defaults(&self) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&anim::desktop_effects_defaults(&SystemIo, &LookCtx::from_env()))
    }

    fn configure(&self, id: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&anim::configure_effect(&SystemIo, &LookCtx::from_env(), &id.to_string()))
    }
}
