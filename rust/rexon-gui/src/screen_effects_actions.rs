// ScreenEffectsActions — объект раздела «Эффекты экрана» вкладки «Оформление».
//
// Логики здесь нет: снимок, таблицу поддержки и запись делает ядро
// (`rexon_core::screen_effects`). Объект только переводит вызовы QML в
// вызовы ядра и отдаёт готовые строки (JSON снимка, фраза итога).
//
// ⚠️ Запись — В РАБОЧЕМ ПОТОКЕ. 15.11.2026 (Cinnamon 6.8 Wayland) окно
// зависло, пока человек быстро жал «Применить»/«Вернуть как было» и
// переключал эффекты: каждое действие синхронно запускало несколько
// `gsettings`/`gdbus` прямо в главном потоке, а теперь к ним добавилось
// ожидание ответа расширения оболочки (до `CONFIRM_WAIT`). Поэтому
// `request` только кладёт действие в очередь «последний выигрывает»
// (`LatestQueue`: ждущее действие заменяется новым, хвоста не копится) и
// сразу возвращается; итог приходит сигналом `finished`, пока идёт
// действие — `busy` (кнопки раздела неактивны).

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, busy)]
        type ScreenEffectsActions = super::ScreenEffectsActionsRust;
    }

    extern "RustQt" {
        /// Снимок раздела (JSON `screen_effects::ScreenEffectsState`) для
        /// окружения `de` (выбор «Текущее окружение»; пусто — запущенное).
        #[qinvokable]
        fn state(self: &ScreenEffectsActions, de: &QString) -> QString;

        /// Действие раздела для окружения `de`: пресет по имени (пусто —
        /// выключить), сила 0…1, скорость 0…`max_speed`. `tag` — что это
        /// («apply», «restore», «revert»), возвращается в `finished`.
        /// Не ждёт: итог — сигналом `finished`.
        #[qinvokable]
        fn request(self: Pin<&mut ScreenEffectsActions>, tag: &QString, de: &QString, preset: &QString, strength: f64, speed: f64);

        /// Итог действия: `tag` запроса, фраза для плашки, записано ли
        /// что-то, включён ли эффект (подтверждён расширением), новый
        /// снимок раздела (JSON, как у `state`).
        #[qsignal]
        fn finished(self: Pin<&mut ScreenEffectsActions>, tag: QString, text: QString, changed: bool, shown: bool, state: QString);
    }

    impl cxx_qt::Threading for ScreenEffectsActions {}
}

use core::pin::Pin;
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use rexon_core::animations::{LookCtx, SystemIo};
use rexon_core::screen_effects as se;

/// Одно действие раздела.
struct Request {
    tag: String,
    de: String,
    preset: String,
    strength: f64,
    speed: f64,
}

#[derive(Default)]
pub struct ScreenEffectsActionsRust {
    busy: bool,
    queue: Arc<se::LatestQueue<Request>>,
}

fn snapshot(de: &str) -> String {
    let state = se::read_state(&SystemIo, &se::ctx_for(LookCtx::from_env(), de));
    serde_json::to_string(&state).unwrap_or_default()
}

impl qobject::ScreenEffectsActions {
    fn state(&self, de: &QString) -> QString {
        QString::from(&snapshot(&de.to_string()))
    }

    fn request(mut self: Pin<&mut Self>, tag: &QString, de: &QString, preset: &QString, strength: f64, speed: f64) {
        let req = Request { tag: tag.to_string(), de: de.to_string(), preset: preset.to_string(), strength, speed };
        let queue = self.rust().queue.clone();
        let start = queue.push(req);
        self.as_mut().set_busy(true);
        if !start {
            return;
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            while let Some(req) = queue.take() {
                let ctx = se::ctx_for(LookCtx::from_env(), &req.de);
                let new = se::settings_from_ui(&req.preset, req.strength, req.speed);
                let out = se::apply_confirmed(&SystemIo, &ctx, &new, &std::thread::sleep);
                let state = snapshot(&req.de);
                // Окно закрыто — сообщать некому, но действие (например,
                // откат пробного показа) всё равно доделано.
                let _ = handle.queue(move |mut obj| {
                    // Занято, пока в очереди что-то есть: новое нажатие могло
                    // прийти между концом этого действия и этой строкой.
                    let busy = obj.rust().queue.busy();
                    obj.as_mut().set_busy(busy);
                    obj.as_mut().finished(
                        QString::from(&req.tag),
                        QString::from(&out.text),
                        out.changed,
                        out.shown,
                        QString::from(&state),
                    );
                });
            }
            // Последнее «занято» могло уйти до того, как поток освободил
            // очередь — снимем его ещё раз.
            let _ = handle.queue(|mut obj| {
                let busy = obj.rust().queue.busy();
                obj.as_mut().set_busy(busy);
            });
        });
    }
}
