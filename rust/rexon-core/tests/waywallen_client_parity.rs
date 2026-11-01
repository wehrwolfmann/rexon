//! Golden-сверка порта `waywallen_client.rs` с оригиналом
//! `rexon/waywallen_client.py` (`WaywallenClient` + CLI `_cli`).
//!
//! Эталоны сняты с ЖИВОГО Python: PySide6.QtDBus подменялся фейком ДО
//! импорта модуля (реальный Qt/D-Bus не загружался, живой демон не
//! трогался), каждый вызов уровня трейта [`WaywallenIo`] (is_valid /
//! property / call) записан в журнал `golden/waywallen_client_cases.json`
//! вместе с CLI-выводом (stdout/stderr/код) и результатами API-шагов.
//!
//! Здесь журнал реплеится: [`MockIo`] на каждый вызов Rust-порта сверяет
//! op/аргументы с очередной записью (порядок 1-в-1) и возвращает записанный
//! результат; в конце журнал обязан быть исчерпан. Покрыты: статус (полный /
//! пустые свойства / строковый порт), apply/next/previous/pause/resume/
//! rescan/open-ui, ошибки D-Bus, usage/неизвестное действие, коэрции
//! свойств (falsy → дефолт, bool/int-значения, знак и парсинг int).
//!
//! Починенные баги оригинала сверяются ОТ ЗАФИКСИРОВАННОГО КРАША: кейсы с
//! `python_crash`/`ok:false` в golden (ValueError на нечисловом int-значении,
//! TypeError на multi-arg ответе Rescan) в Rust дают 0 — намеренное
//! расхождение, задокументированное в `waywallen_client.rs`.

use rexon_core::waywallen_client::{
    cli, DbusReply, DbusValue, WaywallenClient, WaywallenIo,
};
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/waywallen_client_cases.json"))
        .unwrap()
}

/// JSON-значение журнала → [`DbusValue`] (как Python видел QVariant).
fn to_dbus(v: &Value) -> DbusValue {
    match v {
        Value::Null => DbusValue::None,
        Value::Bool(b) => DbusValue::Bool(*b),
        Value::Number(n) => DbusValue::Int(n.as_i64().expect("int в golden")),
        Value::String(s) => DbusValue::Str(s.clone()),
        other => panic!("неожиданный тип значения в golden: {other}"),
    }
}

/// [`DbusValue`] → JSON для сверки аргументов вызова с журналом.
fn from_dbus(v: &DbusValue) -> Value {
    match v {
        DbusValue::None => Value::Null,
        DbusValue::Bool(b) => Value::Bool(*b),
        DbusValue::Int(i) => Value::from(*i),
        DbusValue::Str(s) => Value::from(s.clone()),
    }
}

struct MockIo {
    case: String,
    journal: Vec<Value>,
    pos: usize,
}

impl MockIo {
    fn new(case: &Value) -> Self {
        MockIo {
            case: case["name"].as_str().unwrap().to_string(),
            journal: case["journal"].as_array().unwrap().clone(),
            pos: 0,
        }
    }

    fn next(&mut self, op: &str) -> Value {
        assert!(
            self.pos < self.journal.len(),
            "{}: журнал исчерпан на шаге {}, Rust просит op={op}",
            self.case,
            self.pos
        );
        let e = self.journal[self.pos].clone();
        assert_eq!(
            e["op"].as_str().unwrap_or(""),
            op,
            "{}: шаг {}: Rust просит {op}, эталон {}",
            self.case,
            self.pos,
            e["op"]
        );
        self.pos += 1;
        e
    }

    fn finish(&self) {
        assert_eq!(
            self.pos,
            self.journal.len(),
            "{}: журнал не исчерпан (осталось {} записей)",
            self.case,
            self.journal.len() - self.pos
        );
    }
}

impl WaywallenIo for MockIo {
    fn is_valid(&mut self) -> bool {
        let e = self.next("is_valid");
        e["result"].as_bool().unwrap()
    }

    fn get_property(&mut self, name: &str) -> DbusValue {
        let e = self.next("property");
        assert_eq!(
            e["name"].as_str().unwrap(),
            name,
            "{}: property: имя свойства разошлось",
            self.case
        );
        to_dbus(&e["result"])
    }

    fn call(&mut self, method: &str, args: &[DbusValue]) -> DbusReply {
        let e = self.next("call");
        assert_eq!(
            e["method"].as_str().unwrap(),
            method,
            "{}: call: метод разошёлся",
            self.case
        );
        let got: Vec<Value> = args.iter().map(from_dbus).collect();
        assert_eq!(
            Value::Array(got),
            e["args"],
            "{}: call {method}: аргументы разошлись",
            self.case
        );
        DbusReply {
            error_name: e["error_name"].as_str().unwrap().to_string(),
            error_message: e["error_message"].as_str().unwrap().to_string(),
            arguments: e["arguments"]
                .as_array()
                .unwrap()
                .iter()
                .map(to_dbus)
                .collect(),
        }
    }
}

// ── CLI-кейсы ────────────────────────────────────────────────────────────

#[test]
fn cli_parity() {
    let fx = fx();
    let mut seen = 0usize;
    for case in fx["cases"].as_array().unwrap() {
        if case["kind"] != "cli" {
            continue;
        }
        seen += 1;
        let name = case["name"].as_str().unwrap();
        let argv: Vec<&str> = case["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let mut client = WaywallenClient::new(MockIo::new(case));
        let r = cli(&mut client, "waywallen_client.py", &argv);

        if let Some(crash) = case["python_crash"].as_str() {
            // Починенный баг: Python падал необработанным исключением
            // (`int()` на мусорном ответе Rescan) — Rust отдаёт 0.
            assert!(
                crash.starts_with("ValueError"),
                "{name}: неожиданный краш Python в golden: {crash}"
            );
            assert_eq!(r.code, 0, "{name}: код после починки");
            assert_eq!(
                r.stdout, "rescan task id: 0\n",
                "{name}: stdout после починки"
            );
            assert_eq!(r.stderr, "", "{name}: stderr после починки");
            continue;
        }

        assert_eq!(
            i64::from(r.code),
            case["exit_code"].as_i64().unwrap(),
            "{name}: код возврата"
        );
        assert_eq!(r.stdout, case["stdout"].as_str().unwrap(), "{name}: stdout");
        assert_eq!(r.stderr, case["stderr"].as_str().unwrap(), "{name}: stderr");
    }
    assert!(seen >= 10, "cli-кейсы не найдены в golden");
}

// ── API-кейсы ────────────────────────────────────────────────────────────

/// Ожидаемая строка из golden-значения: str как есть, прочее — Python
/// `str()` (порт возвращает String там, где Python отдавал «что пришло»).
fn expect_str(v: &Value) -> String {
    to_dbus(v).py_str()
}

#[test]
fn api_parity() {
    let fx = fx();
    let mut seen = 0usize;
    for case in fx["cases"].as_array().unwrap() {
        if case["kind"] != "api" {
            continue;
        }
        seen += 1;
        let name = case["name"].as_str().unwrap();
        let steps = case["steps"].as_array().unwrap();
        let results = case["results"].as_array().unwrap();
        assert_eq!(steps.len(), results.len(), "{name}: steps vs results");

        let mut c = WaywallenClient::new(MockIo::new(case));
        for (step, res) in steps.iter().zip(results) {
            let step = step.as_array().unwrap();
            let attr = step[0].as_str().unwrap();
            let ok = res["ok"].as_bool().unwrap();
            let ctx = format!("{name}: шаг {attr}");
            match attr {
                "available" => {
                    assert!(ok, "{ctx}");
                    assert_eq!(
                        Value::Bool(c.available()),
                        res["value"],
                        "{ctx}"
                    );
                }
                "version" | "current_wallpaper_id" | "queue_mode" => {
                    assert!(ok, "{ctx}");
                    let got = match attr {
                        "version" => c.version(),
                        "current_wallpaper_id" => c.current_wallpaper_id(),
                        _ => c.queue_mode(),
                    };
                    assert_eq!(got, expect_str(&res["value"]), "{ctx}");
                }
                "ws_port" | "rotation_secs" => {
                    let got = if attr == "ws_port" {
                        c.ws_port()
                    } else {
                        c.rotation_secs()
                    };
                    if ok {
                        assert_eq!(
                            got,
                            res["value"].as_i64().unwrap(),
                            "{ctx}"
                        );
                    } else {
                        // Починенный баг: ValueError на нечисловой строке.
                        let err = res["error"].as_str().unwrap();
                        assert!(
                            err.starts_with("ValueError"),
                            "{ctx}: неожиданная ошибка Python: {err}"
                        );
                        assert_eq!(got, 0, "{ctx}: 0 после починки");
                    }
                }
                "apply_by_id" => {
                    // Python коэрцил str(wallpaper_id) — в Rust коэрция на
                    // вызывающем (типизированный &str), как для int-id 123.
                    let id = match &step[1] {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        other => panic!("{ctx}: неожиданный id {other}"),
                    };
                    let r = c.apply_by_id(&id);
                    assert!(ok, "{ctx}");
                    assert_eq!(
                        r.unwrap().py_str(),
                        expect_str(&res["value"]),
                        "{ctx}"
                    );
                }
                "next" => {
                    let r = c.next();
                    if ok {
                        assert_eq!(
                            r.unwrap().py_str(),
                            expect_str(&res["value"]),
                            "{ctx}"
                        );
                    } else {
                        let e = r.unwrap_err();
                        assert_eq!(
                            format!("RuntimeError: {e}"),
                            res["error"].as_str().unwrap(),
                            "{ctx}: текст ошибки"
                        );
                    }
                }
                "rescan" => {
                    let got = c.rescan().expect("rescan без ошибки D-Bus");
                    if ok {
                        assert_eq!(
                            got,
                            res["value"].as_i64().unwrap(),
                            "{ctx}"
                        );
                    } else {
                        // Починенный баг: TypeError на int(list).
                        let err = res["error"].as_str().unwrap();
                        assert!(
                            err.starts_with("TypeError"),
                            "{ctx}: неожиданная ошибка Python: {err}"
                        );
                        assert_eq!(got, 0, "{ctx}: 0 после починки");
                    }
                }
                "set_rotation_interval" => {
                    assert!(ok, "{ctx}");
                    c.set_rotation_interval(step[1].as_i64().unwrap())
                        .unwrap();
                }
                "set_shuffle" => {
                    assert!(ok, "{ctx}");
                    c.set_shuffle(step[1].as_bool().unwrap()).unwrap();
                }
                "quit" => {
                    assert!(ok, "{ctx}");
                    c.quit().unwrap();
                }
                other => panic!("{name}: неизвестный шаг {other}"),
            }
        }
    }
    assert!(seen >= 5, "api-кейсы не найдены в golden");
}

// ── исчерпание журнала ───────────────────────────────────────────────────

/// Отдельная проверка: после CLI-прогона журнал каждого кейса исчерпан —
/// Rust не делает ни лишних, ни недостающих D-Bus обращений.
#[test]
fn journal_exhausted() {
    let fx = fx();
    for case in fx["cases"].as_array().unwrap() {
        if case["kind"] != "cli" || case["python_crash"].as_str().is_some() {
            continue;
        }
        let argv: Vec<&str> = case["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let io = MockIo::new(case);
        let mut client = WaywallenClient::new(io);
        let _ = cli(&mut client, "waywallen_client.py", &argv);
        client.into_io().finish();
    }
}
