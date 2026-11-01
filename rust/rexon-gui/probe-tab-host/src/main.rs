// Прогон макета каркаса. Печатает одну машинно-читаемую строку
// `REXON-TABHOST-PROBE ...` — в том же духе, что `REXON-WS-PROBE` у макета
// транспорта: проверка обязана быть воспроизводимой, а не «посмотрел глазами».
//
// Выход 0 — Qt повёл себя так, как обещает документация: QML встаёт внутрь
// вкладки виджетов и держит цепочку фокуса, а виджет в сцену Qt Quick НЕ
// встаёт. Любое расхождение — выход 1 и перечень провалившихся проверок.
//
// Заведомо неверные условия задаются переменной среды
// `REXON_TABHOST_MUTATE` (через запятую): `blank-qml`, `no-focus`,
// `fake-embed`, `skip-push`. Каждое обязано ронять прогон — стенд, который
// зеленеет всегда, ничего не проверяет.
//
// Каталог для снимков — первым аргументом.

use std::ffi::CString;
use std::os::raw::{c_char, c_int};

#[repr(C)]
struct TabHostReport {
    qt_major: c_int,
    qt_minor: c_int,
    qml_host_ready: c_int,
    qml_pixels_pct: c_int,
    focus_by_setfocus: c_int,
    focus_by_tab: c_int,
    focus_back_to_widget: c_int,
    widget_is_quickitem: c_int,
    qml_data_before: c_int,
    qml_data_after: c_int,
    qml_child_before: c_int,
    qml_child_after: c_int,
    widget_pixels_pct: c_int,
    parent_undefined: c_int,
    parent_assign_errors: c_int,
    widget_parent_hijacked: c_int,
    widget_became_toplevel: c_int,
    focus_detail: [c_char; 96],
    error: [c_char; 256],
}

extern "C" {
    fn rexon_tab_host_probe(mutate: c_int, out_dir: *const c_char) -> TabHostReport;
}

const MUTATE_BLANK_QML: c_int = 1;
const MUTATE_NO_FOCUS: c_int = 2;
const MUTATE_FAKE_EMBED: c_int = 4;
const MUTATE_SKIP_PUSH: c_int = 8;
const MUTATE_PARENT_OF_ITEM: c_int = 16;

fn parse_mutate(raw: &str) -> Result<c_int, String> {
    let mut flags = 0;
    for part in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        flags |= match part {
            "blank-qml" => MUTATE_BLANK_QML,
            "no-focus" => MUTATE_NO_FOCUS,
            "fake-embed" => MUTATE_FAKE_EMBED,
            "skip-push" => MUTATE_SKIP_PUSH,
            "parent-of-item" => MUTATE_PARENT_OF_ITEM,
            other => return Err(format!("неизвестная поломка: {other}")),
        };
    }
    Ok(flags)
}

fn c_str(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Приговор. Каждая строка — отдельная проверка; в отчёт уходят имена
/// провалившихся, чтобы по выводу было видно, ЧТО именно не сошлось.
fn failures(r: &TabHostReport) -> Vec<&'static str> {
    let mut bad = Vec::new();

    // ── Сторона, которая обязана работать: QML внутрь виджетов ───────
    if r.qml_host_ready != 1 {
        bad.push("qml-хост не дошёл до Ready");
    }
    // Половина площади хоста — с запасом: сцена красит его целиком, но по
    // краям есть сглаживание, а по центру лежит поле ввода.
    if r.qml_pixels_pct < 50 {
        bad.push("сцена QML не нарисовалась внутри вкладки");
    }
    if r.focus_by_setfocus != 1 {
        bad.push("фокус не дошёл из виджета в элемент QML");
    }
    if r.focus_by_tab != 1 {
        bad.push("Tab не переводит фокус в QML");
    }
    if r.focus_back_to_widget != 1 {
        bad.push("из QML фокус не возвращается в виджеты");
    }

    // ── Сторона, которая обязана НЕ работать: виджет внутрь QML ──────
    if r.widget_is_quickitem != 0 {
        bad.push("QWidget внезапно приводится к QQuickItem");
    }
    // Страховка от пустой проверки: если сцена виджет вообще не приняла,
    // «нет визуальных детей» ничего не доказывает.
    if r.qml_data_before < 0 || r.qml_data_after != r.qml_data_before + 1 {
        bad.push("сцена не приняла виджет даже ресурсом — проверка пуста");
    }
    if r.qml_child_before < 0 || r.qml_child_after != r.qml_child_before {
        bad.push("виджет стал визуальным ребёнком сцены");
    }
    if r.widget_pixels_pct != 0 {
        bad.push("виджет нарисовался внутри сцены QML");
    }
    // У настоящего Item той же сцены parent есть (положительный контроль —
    // поломка `parent-of-item`); у виджета его нет вовсе.
    if r.parent_undefined != 1 {
        bad.push("QML знает у виджета родителя-элемента");
    }
    if r.widget_parent_hijacked != 1 {
        bad.push("сцена не перевесила виджету QObject-родителя — push не сработал");
    }
    if r.widget_became_toplevel != 1 {
        bad.push("показанный виджет не стал отдельным окном");
    }

    bad
}

fn main() {
    let out_dir = std::env::args().nth(1).unwrap_or_default();
    let raw = std::env::var("REXON_TABHOST_MUTATE").unwrap_or_default();
    let mutate = match parse_mutate(&raw) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("REXON-TABHOST-PROBE error={e}");
            std::process::exit(2);
        }
    };

    let out = CString::new(out_dir).unwrap_or_default();
    let r = unsafe { rexon_tab_host_probe(mutate, out.as_ptr()) };

    let bad = failures(&r);
    let err = c_str(&r.error);
    let focus_detail = c_str(&r.focus_detail);

    println!(
        "REXON-TABHOST-PROBE qt={}.{} mutate={} qmlReady={} qmlPixels={}% \
focusSetFocus={} focusTab={} focusBack={} widgetIsItem={} \
sceneData={}->{} sceneChildren={}->{} widgetPixels={}% parentUndefined={} \
parentErrors={} parentHijacked={} widgetToplevel={} \
verdict={} focusDetail=[{}] failed={} error={}",
        r.qt_major,
        r.qt_minor,
        if raw.is_empty() { "-" } else { raw.as_str() },
        r.qml_host_ready,
        r.qml_pixels_pct,
        r.focus_by_setfocus,
        r.focus_by_tab,
        r.focus_back_to_widget,
        r.widget_is_quickitem,
        r.qml_data_before,
        r.qml_data_after,
        r.qml_child_before,
        r.qml_child_after,
        r.widget_pixels_pct,
        r.parent_undefined,
        r.parent_assign_errors,
        r.widget_parent_hijacked,
        r.widget_became_toplevel,
        if bad.is_empty() { "ok" } else { "fail" },
        focus_detail,
        if bad.is_empty() { "-".to_string() } else { bad.join("; ") },
        if err.is_empty() { "-" } else { err.as_str() },
    );

    std::process::exit(if bad.is_empty() { 0 } else { 1 });
}
