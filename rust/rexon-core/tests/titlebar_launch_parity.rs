//! Сверка «Кнопок заголовка окна» (все кнопки KDE, флажки KWin) и «Отклика
//! при запуске» на подставном вводе-выводе: какие команды уходят в систему
//! и что видит раздел. Ожидания — `tests/golden/titlebar_launch_cases.json`
//! (ключи и умолчания сверены с `kwindecorationsettings.kcfg` и
//! `launchfeedbacksettings.kcfg` KDE 6.8).
//!
//! Подставная машина: файлы — словарь, `gsettings get` / `xfconf-query`
//! отвечают из словаря, записи (`kwriteconfig6`, `gsettings set|reset`,
//! `xfconf-query -s|-r`, уведомления D-Bus) только записываются в журнал —
//! ничего не исполняется.

use std::cell::RefCell;
use std::collections::BTreeMap;

use rexon_core::animations::{LookCtx, LookIo};
use rexon_core::{launch_feedback as lf, window_buttons as wb};
use serde_json::Value;

struct FakeIo {
    files: RefCell<BTreeMap<String, String>>,
    keys: BTreeMap<String, String>,
    log: RefCell<Vec<Vec<String>>>,
}

impl LookIo for FakeIo {
    fn run(&self, a: &[String]) -> (bool, String) {
        let reading = match a[0].as_str() {
            "gsettings" => a.get(1).map(String::as_str) == Some("get"),
            "xfconf-query" => !a.iter().any(|x| x == "-s" || x == "-r"),
            _ => false,
        };
        if reading {
            let key = match a[0].as_str() {
                "gsettings" => format!("{} {}", a[2], a[3]),
                _ => format!("{} {}", a[2], a[4]),
            };
            return match self.keys.get(&key) {
                Some(v) if a[0] == "gsettings" => (true, format!("'{v}'\n")),
                Some(v) => (true, format!("{v}\n")),
                None => (false, String::new()),
            };
        }
        self.log.borrow_mut().push(a.to_vec());
        (true, String::new())
    }
    fn have(&self, _: &str) -> bool {
        true
    }
    fn read(&self, p: &str) -> Option<String> {
        self.files.borrow().get(p).cloned()
    }
    fn write(&self, p: &str, t: &str) -> bool {
        self.files.borrow_mut().insert(p.to_string(), t.to_string());
        true
    }
    fn list_dir(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(|x| x.as_str().unwrap_or_default().to_string()).collect()).unwrap_or_default()
}

/// Сравнить только поля, названные в эталоне.
fn check_subset(name: &str, want: &Value, got: &Value) {
    for (k, w) in want.as_object().expect("state — объект") {
        let g = &got[k];
        if let (Some(wa), Some(ga)) = (w.as_array(), g.as_array()) {
            // Список объектов: в каждом — только названные поля.
            if wa.first().map(Value::is_object).unwrap_or(false) {
                assert_eq!(wa.len(), ga.len(), "{name}: {k} — число элементов: {g}");
                for (wi, gi) in wa.iter().zip(ga) {
                    check_subset(&format!("{name}.{k}"), wi, gi);
                }
                continue;
            }
        }
        assert_eq!(w, g, "{name}: поле {k}");
    }
}

#[test]
fn titlebar_and_launch_feedback_cases() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let golden: Value = serde_json::from_str(include_str!("golden/titlebar_launch_cases.json")).unwrap();
    let cases = golden["cases"].as_array().unwrap();
    assert!(cases.len() >= 15, "эталон обеднел: {}", cases.len());
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let ctx = LookCtx::new(case["de"].as_str().unwrap(), "/h", Some("/c"));
        let mut files = BTreeMap::new();
        for (rel, text) in case["files"].as_object().into_iter().flatten() {
            files.insert(ctx.cfg(rel), text.as_str().unwrap().to_string());
        }
        let keys = case["keys"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();
        let io = FakeIo { files: RefCell::new(files), keys, log: RefCell::new(Vec::new()) };
        let action = strings(&case["action"]);
        let arg = |i: usize| action.get(i).cloned().unwrap_or_default();
        let on = |i: usize| arg(i) == "true";
        let state = match action[0].as_str() {
            "state" => Some(serde_json::to_value(wb::read_state(&io, &ctx)).unwrap()),
            "lf_state" => Some(serde_json::to_value(lf::read_state(&io, &ctx)).unwrap()),
            "toggle" => {
                wb::apply_toggle(&io, &ctx, &arg(1), &arg(2), on(3));
                None
            }
            // Перетаскивание: id, откуда, позиция, куда, позиция.
            "move" => {
                let n = |i: usize| arg(i).parse().unwrap_or(0);
                let msg = wb::apply_move(&io, &ctx, &arg(1), &arg(2), n(3), &arg(4), n(5));
                Some(serde_json::json!({ "message": msg }))
            }
            // Шаг кнопкой: сторона, позиция, направление.
            "nudge" => {
                let (msg, at) = wb::apply_nudge(&io, &ctx, &arg(1), arg(2).parse().unwrap(), &arg(3));
                Some(serde_json::json!({ "message": msg, "at": at }))
            }
            "add" => {
                let msg = wb::apply_add(&io, &ctx, &arg(1), &arg(2));
                Some(serde_json::json!({ "message": msg }))
            }
            "preset" => {
                let ids: Vec<String> = arg(2).split(',').filter(|s| !s.is_empty()).map(String::from).collect();
                wb::apply_preset(&io, &ctx, &arg(1), &ids);
                None
            }
            "flag" => {
                wb::apply_flag(&io, &ctx, &arg(1), on(2));
                None
            }
            "defaults" => {
                wb::apply_defaults(&io, &ctx);
                None
            }
            "lf_cursor" => {
                lf::apply_cursor(&io, &ctx, &arg(1));
                None
            }
            "lf_taskbar" => {
                lf::apply_taskbar(&io, &ctx, on(1));
                None
            }
            "lf_timeout" => {
                lf::apply_timeout(&io, &ctx, arg(1).parse().unwrap());
                None
            }
            "lf_defaults" => {
                lf::apply_defaults(&io, &ctx);
                None
            }
            other => panic!("{name}: неизвестное действие {other}"),
        };
        if let Some(got) = state {
            check_subset(name, &case["state"], &got);
        }
        let want: Vec<Vec<String>> = case["commands"].as_array().map(|a| a.iter().map(strings).collect()).unwrap_or_default();
        assert_eq!(*io.log.borrow(), want, "{name}: команды");
    }
}
