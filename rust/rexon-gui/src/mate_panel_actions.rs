// MatePanelActions — объект раздела «Панель MATE» (готовые раскладки
// панели). Раздел самостоятельный: его можно вставить в любую вкладку
// («Настройки» сейчас, «Оформление» потом) без переделки.
//
// Логики здесь нет: снимок, подписи, применение и возврат — ядро
// (`rexon_core::mate_panel`).

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type MatePanelActions = super::MatePanelActionsRust;
    }

    impl cxx_qt::Threading for MatePanelActions {}

    extern "RustQt" {
        /// Снимок раздела (JSON `mate_panel::PanelState`).
        #[qinvokable]
        fn state(self: &MatePanelActions) -> QString;

        /// Текст подтверждения перед применением раскладки `id`, а при
        /// пустом `id` — перед возвратом прежней панели
        /// (JSON `{title, question}`).
        #[qinvokable]
        fn confirm(self: &MatePanelActions, id: &QString) -> QString;

        /// Применить раскладку `id` → сообщение для человека.
        #[qinvokable]
        fn apply(self: &MatePanelActions, id: &QString) -> QString;

        /// Вопрос «собрать недостающие части раскладки из AUR?» (JSON
        /// `{title, question}`; `{}` — спрашивать не о чем: пакеты из
        /// репозиториев ставятся без вопроса).
        #[qinvokable]
        #[cxx_name = "partsQuestion"]
        fn parts_question(self: &MatePanelActions, id: &QString) -> QString;

        /// Применить раскладку в фоне. Недостающие части из репозиториев
        /// ставятся всегда (окно пароля); `install` — ещё и собрать части из
        /// AUR (общая очередь сборок; человек согласился в вопросе). Не
        /// встало — раскладка не применяется. Итог — сигналом `applied`.
        #[qinvokable]
        #[cxx_name = "applyAsync"]
        fn apply_async(self: Pin<&mut MatePanelActions>, id: &QString, install: bool);

        /// Применение кончилось: сообщение для человека.
        #[qsignal]
        fn applied(self: Pin<&mut MatePanelActions>, text: QString);

        /// Вернуть исходную панель в фоне: возврат ждёт, пока панель снимет
        /// апплеты (до 3 с), и интерфейс на это время не замирает. Итог —
        /// сигналом `restored`.
        #[qinvokable]
        #[cxx_name = "restoreAsync"]
        fn restore_async(self: Pin<&mut MatePanelActions>);

        /// Возврат кончился: сообщение для человека.
        #[qsignal]
        fn restored(self: Pin<&mut MatePanelActions>, text: QString);
    }
}

use std::pin::Pin;

use cxx_qt::Threading;
use rexon_core::animations::SystemIo;
use rexon_core::mate_panel::{self, PanelCtx};

#[derive(Default)]
pub struct MatePanelActionsRust;

impl qobject::MatePanelActions {
    fn state(&self) -> cxx_qt_lib::QString {
        let state = mate_panel::read_state(&SystemIo, &PanelCtx::from_env());
        cxx_qt_lib::QString::from(&serde_json::to_string(&state).unwrap_or_default())
    }

    fn confirm(&self, id: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        let id = id.to_string();
        let (title, question) =
            if id.is_empty() { mate_panel::restore_confirm_texts() } else { mate_panel::confirm_texts(&id) };
        let json = serde_json::json!({ "title": title, "question": question });
        cxx_qt_lib::QString::from(&json.to_string())
    }

    fn apply(&self, id: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&mate_panel::apply(&SystemIo, &PanelCtx::from_env(), &id.to_string()))
    }

    fn parts_question(&self, id: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        let id = id.to_string();
        let offer = mate_panel::live_parts_offer(&SystemIo, &PanelCtx::from_env(), &id);
        let json = match mate_panel::parts_question(&id, &offer) {
            Some((title, question)) => serde_json::json!({ "title": title, "question": question }),
            None => serde_json::json!({}),
        };
        cxx_qt_lib::QString::from(&json.to_string())
    }

    fn apply_async(mut self: Pin<&mut Self>, id: &cxx_qt_lib::QString, install: bool) {
        let id = id.to_string();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let ctx = PanelCtx::from_env();
            let offer = mate_panel::live_parts_offer(&SystemIo, &ctx, &id);
            let offer = if install { offer } else { offer.repo_only() };
            let text = mate_panel::apply_with_parts(&SystemIo, &ctx, &id, &offer, &mut |o| {
                if !o.repo.is_empty() {
                    mate_panel::live_install_repo_parts(&o.repo)?;
                }
                for (package, _) in &o.aur {
                    crate::appearance_actions::aur_install_wait(package)?;
                }
                Ok(())
            });
            let _ = handle.queue(move |mut object| {
                object.as_mut().applied(cxx_qt_lib::QString::from(&text));
            });
        });
    }

    fn restore_async(mut self: Pin<&mut Self>) {
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let text = mate_panel::restore(&SystemIo, &PanelCtx::from_env());
            let _ = handle.queue(move |mut object| {
                object.as_mut().restored(cxx_qt_lib::QString::from(&text));
            });
        });
    }
}
