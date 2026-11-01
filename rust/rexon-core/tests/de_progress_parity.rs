//! Golden-сверка порта `de_progress.rs` с оригиналом
//! `rexon/qt_main/de_progress.py`.
//!
//! Эталон снят с ЖИВОГО диалога: PySide6 поднимался offscreen, вызывались
//! настоящие `DEProgressDialog.on_progress` / `set_done`, и после каждого
//! вызова снималось состояние всех четырёх элементов — значение полосы,
//! заголовок, строка текущего пакета и текст журнала, плюс надпись и
//! доступность кнопки закрытия. Правило воркера снималось так же вживую:
//! `_DEOperationWorker` запускался настоящим потоком, а испущенные сигналы
//! записывались.
//!
//! `golden/de_progress_cases.json` — снятые значения.
//!
//! Эталон русский: подписи теперь идут через каталог перевода, поэтому
//! сверки с ним закрепляют русский из `po/ru.po` (`i18n_pin::ru`) — это и
//! есть проверка «по-русски как было». Английский интерфейс сверяется
//! отдельно в конце файла.

mod i18n_pin;

use rexon_core::de_progress as dp;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/de_progress_cases.json")).unwrap()
}

/// Состояние диалога, как его видит эталон.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dialog {
    bar: i32,
    status: String,
    current: String,
    log: Vec<String>,
    close_text: String,
    close_enabled: bool,
}

impl Dialog {
    /// Свежий диалог — ровно то, что собирает его конструктор.
    fn new() -> Self {
        Self {
            bar: 0,
            status: dp::initial_status(),
            current: String::new(),
            log: Vec::new(),
            close_text: dp::close_text_busy(),
            close_enabled: false,
        }
    }

    fn apply(&mut self, u: &dp::ProgressUpdate) {
        if let Some(v) = u.bar {
            self.bar = v;
        }
        if let Some(s) = &u.status {
            self.status = s.clone();
        }
        if let Some(c) = &u.current {
            self.current = c.clone();
        }
        if let Some(l) = &u.log_line {
            self.log.push(l.clone());
        }
    }

    fn apply_done(&mut self, u: &dp::DoneUpdate) {
        if let Some(v) = u.bar {
            self.bar = v;
        }
        self.status = u.status.clone();
        if let Some(c) = &u.current {
            self.current = c.clone();
        }
        if let Some(l) = &u.log_line {
            self.log.push(l.clone());
        }
        self.close_text = u.close_text.clone();
        self.close_enabled = u.close_enabled;
    }

    /// Текст журнала: `appendPlainText` кладёт каждую строку отдельным
    /// абзацем, а `toPlainText` склеивает абзацы переводом строки.
    fn log_text(&self) -> String {
        self.log.join("\n")
    }

    fn assert_matches(&self, want: &Value, case: &str) {
        assert_eq!(
            self.bar,
            want["bar"].as_i64().unwrap() as i32,
            "полоса, случай {case}"
        );
        assert_eq!(
            self.status,
            want["status"].as_str().unwrap(),
            "заголовок, случай {case}"
        );
        assert_eq!(
            self.current,
            want["current"].as_str().unwrap(),
            "текущий пакет, случай {case}"
        );
        assert_eq!(
            self.log_text(),
            want["log"].as_str().unwrap(),
            "журнал, случай {case}"
        );
        assert_eq!(
            self.close_text,
            want["close_text"].as_str().unwrap(),
            "надпись кнопки, случай {case}"
        );
        assert_eq!(
            self.close_enabled,
            want["close_enabled"].as_bool().unwrap(),
            "доступность кнопки, случай {case}"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn initial_state_matches_original() {
    let _lang = i18n_pin::ru();
    let g = fx();
    Dialog::new().assert_matches(&g["initial"], "initial");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn stage_titles_match_original() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let want = g["stage_titles"].as_object().unwrap();
    assert_eq!(
        dp::STAGE_TITLES.len(),
        want.len(),
        "число этапов с заголовком"
    );
    for (stage, title) in want {
        assert_eq!(
            dp::stage_title(stage).as_deref(),
            title.as_str(),
            "заголовок этапа {stage}"
        );
    }
    // Этапа, которого в таблице нет, заголовок не меняет.
    assert_eq!(dp::stage_title("warning"), None);
    assert_eq!(dp::stage_title("pkg_install"), None);
    assert_eq!(dp::stage_title("raw"), None);
    assert_eq!(dp::stage_title(""), None);
}

/// Каждое одиночное сообщение — на свежем диалоге, как и снимался эталон.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn single_messages_match_original() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let cases: &[(&str, &str, &str, Option<f64>)] = &[
        ("install_frac", "install", "ставим", Some(0.25)),
        ("install_no_frac", "install", "ставим", None),
        ("frac_zero", "install", "", Some(0.0)),
        ("frac_one", "install", "", Some(1.0)),
        ("frac_truncates", "install", "", Some(0.98765)),
        ("frac_negative", "install", "", Some(-0.5)),
        ("frac_over_one", "install", "", Some(2.0)),
        ("frac_nan", "install", "", Some(f64::NAN)),
        ("pkg_install", "pkg_install", "plasma-desktop", None),
        ("pkg_install_empty", "pkg_install", "", None),
        ("download", "download", "качаем 1/10", None),
        ("download_empty", "download", "", None),
        ("check", "check", "проверяем", None),
        ("raw_padded", "raw", "  строка вывода  ", None),
        ("raw_blank", "raw", "     ", None),
        ("raw_empty", "raw", "", None),
        ("error", "error", "всё сломалось", None),
        ("warning", "warning", "будь осторожен", None),
        ("unknown_stage", "чегототакое", "просто текст", None),
        ("known_stage_no_msg", "done_main", "", None),
        ("aur_warn", "aur_warn", "часть не встала", Some(0.5)),
    ];
    for (name, stage, message, fraction) in cases {
        let mut dlg = Dialog::new();
        dlg.apply(&dp::on_progress(stage, message, *fraction));
        dlg.assert_matches(&g[*name], name);
    }
}

/// ⚠️ ПОЧИНЕНО (правило #16). Бесконечная доля роняла диалог: оригинал ловил
/// `TypeError`/`ValueError`, а приведение бесконечности к целому даёт
/// `OverflowError`. Эталон это падение и зафиксировал; порт вместо падения
/// оставляет полосу на месте — как при «не числе».
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn infinite_fraction_no_longer_crashes() {
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(
        g["frac_inf"]["raised"].as_str(),
        Some("OverflowError: cannot convert float infinity to integer"),
        "оригинал падал именно так"
    );
    assert_eq!(dp::bar_value(Some(f64::INFINITY)), None);
    assert_eq!(dp::bar_value(Some(f64::NEG_INFINITY)), None);
    assert_eq!(dp::bar_value(Some(f64::NAN)), None);

    // Диалог при этом остаётся живым и обрабатывает следующее сообщение.
    let mut dlg = Dialog::new();
    dlg.apply(&dp::on_progress("install", "ставим", Some(f64::INFINITY)));
    assert_eq!(dlg.bar, 0, "полоса не двинулась");
    assert_eq!(dlg.status, "Установка пакетов...", "остальное отработало");
    dlg.apply(&dp::on_progress("install", "", Some(0.5)));
    assert_eq!(dlg.bar, 500, "следующее сообщение принято");
}

/// Доля усекается к нулю, а не округляется.
#[test]
fn fraction_truncates_towards_zero() {
    assert_eq!(dp::bar_value(Some(0.98765)), Some(987));
    assert_eq!(dp::bar_value(Some(0.9999)), Some(999));
    assert_eq!(dp::bar_value(Some(0.05)), Some(50));
    assert_eq!(dp::bar_value(Some(0.4)), Some(400));
    assert_eq!(dp::bar_value(None), None);
    // Вне полосы — не двигаем.
    assert_eq!(dp::bar_value(Some(-0.001)), None);
    assert_eq!(dp::bar_value(Some(1.1)), None);
    // А вот 1.001 в полосу ПОПАДАЕТ: 1.001 × 1000 в двоичной дроби чуть
    // меньше 1001, и усечение даёт ровно 1000. Оригинал ведёт себя так же.
    assert_eq!(dp::bar_value(Some(1.001)), Some(dp::BAR_MAX));
    assert_eq!(dp::bar_value(Some(0.0)), Some(0));
    assert_eq!(dp::bar_value(Some(1.0)), Some(dp::BAR_MAX));
}

/// Прогнать записанную эталоном последовательность, сверяя состояние ПОСЛЕ
/// КАЖДОГО сообщения, а не только в конце.
fn replay(case: &str) {
    let g = fx();
    let input = g[case]["input"].as_array().unwrap();
    let steps = g[case]["steps"].as_array().unwrap();
    assert_eq!(input.len(), steps.len());

    let mut dlg = Dialog::new();
    for (i, (msg, want)) in input.iter().zip(steps).enumerate() {
        let stage = msg[0].as_str().unwrap();
        let text = msg[1].as_str().unwrap();
        let fraction = msg[2].as_f64();
        dlg.apply(&dp::on_progress(stage, text, fraction));
        dlg.assert_matches(want, &format!("{case}[{i}] {stage}"));
    }
}

/// Настоящая последовательность установки.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn sequence_matches_original_step_by_step() {
    let _lang = i18n_pin::ru();
    replay("sequence");
}

/// Строка текущего пакета уже заполнена, приходит ПУСТОЕ сообщение. Этапы
/// ведут себя по-разному, и это не случайность: `pkg_install` ставит строку
/// без проверки и потому затирает её, а `download`/`check` — только когда
/// есть что показать. На свежем диалоге разницы не видно (там и так пусто),
/// поэтому проверяется она именно так.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn empty_message_overwrites_only_for_pkg_install() {
    let _lang = i18n_pin::ru();
    replay("current_overwrite");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn done_matches_original() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let cases: &[(&str, bool, &str)] = &[
        ("done_ok_msg", true, "поставлено 42 пакета"),
        ("done_ok_literal_ok", true, "ok"),
        ("done_ok_empty", true, ""),
        ("done_fail", false, "не вышло"),
        ("done_fail_empty", false, ""),
    ];
    for (name, ok, msg) in cases {
        let mut dlg = Dialog::new();
        dlg.apply_done(&dp::on_done(*ok, msg));
        dlg.assert_matches(&g[*name], name);
    }
}

/// При неудаче журнал НЕ дополняется: текст ошибки там уже есть, положил его
/// этап `error`. И полоса остаётся там, где стояла.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn failure_keeps_log_and_bar() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let mut dlg = Dialog::new();
    dlg.apply(&dp::on_progress("error", "первая беда", None));
    dlg.apply_done(&dp::on_done(false, "первая беда"));
    dlg.assert_matches(&g["done_fail_after_error"], "done_fail_after_error");
    assert_eq!(
        dlg.log_text(),
        "❌ первая беда",
        "ошибка в журнале ровно одна"
    );
}

/// Завершение поверх ЗАПОЛНЕННОГО диалога — только так видно, что именно оно
/// трогает: неудача очищает строку текущего пакета и оставляет полосу на
/// месте, удача доводит полосу до конца и строку тоже очищает (оригинал её
/// не трогал, и под «Завершено ✅» висела последняя сырая строка pacman —
/// живая установка River 24.10.2026). На свежем диалоге обе разницы не
/// видны — там и полоса на нуле, и строка пуста.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn done_over_populated_dialog() {
    let _lang = i18n_pin::ru();
    let g = fx();

    let mut failed = Dialog::new();
    failed.apply(&dp::on_progress("pkg_install", "plasma-desktop", Some(0.4)));
    failed.apply(&dp::on_progress("error", "пакет не найден", None));
    failed.apply_done(&dp::on_done(false, "пакет не найден"));
    failed.assert_matches(&g["done_fail_after_pkg"], "done_fail_after_pkg");
    assert_eq!(failed.bar, 400, "полоса осталась там, где стояла");
    assert_eq!(failed.current, "", "строка текущего пакета очищена");

    let mut succeeded = Dialog::new();
    succeeded.apply(&dp::on_progress("pkg_install", "plasma-desktop", Some(0.4)));
    succeeded.apply_done(&dp::on_done(true, "готово"));
    succeeded.assert_matches(&g["done_ok_after_pkg"], "done_ok_after_pkg");
    assert_eq!(
        succeeded.current, "",
        "под итогом не должна висеть последняя строка пакетного менеджера"
    );

    // Та же беда с сырой строкой вывода (этап `raw` строку шага не ставит,
    // но этап разбора pacman — ставит).
    let mut raw = Dialog::new();
    raw.apply(&dp::on_progress("pkg_install", "🔍 looking for conflicting packages...", Some(0.6)));
    raw.apply_done(&dp::on_done(true, ""));
    assert_eq!(raw.current, "", "строка шага пережила итог");
    assert_eq!(raw.status, "Завершено ✅");
}

/// Правило воркера: неудача без объявленной ошибки объявляется им самим.
#[test]
fn worker_error_emit_matches_original() {
    let g = fx();

    let cases: &[(&str, bool, bool, &str)] = &[
        // (случай, удача, ошибка уже объявлена, сообщение)
        ("worker_fail_silent", false, false, "тихий отказ"),
        ("worker_fail_after_error", false, true, "громкий отказ"),
        ("worker_ok", true, false, "ок"),
        ("worker_fail_no_msg", false, false, ""),
        ("worker_raises", false, false, "бум"),
    ];

    for (name, ok, error_seen, msg) in cases {
        // Эталон записывал ВСЕ сигналы прогресса. Когда операция объявила
        // ошибку сама, записанный сигнал — её, и воркеру добавлять нечего;
        // иначе записанный сигнал (если он есть) как раз воркером и испущен.
        let signals = g[*name]["progress"].as_array().unwrap().clone();
        for s in &signals {
            assert_eq!(s[0].as_str(), Some("error"), "случай {name}: этап");
        }
        let want_own = if *error_seen {
            None
        } else {
            signals.first().map(|s| s[1].as_str().unwrap().to_string())
        };
        assert_eq!(
            dp::worker_error_emit(*ok, *error_seen, msg),
            want_own,
            "случай {name}"
        );
    }

    // Исключение внутри операции превращается в неудачу с его текстом —
    // и текст доходит до журнала.
    assert_eq!(
        g["worker_raises"]["finished"][0][1].as_str(),
        Some("бум"),
        "текст исключения становится сообщением"
    );
    assert_eq!(dp::worker_error_emit(false, false, "бум"), Some("бум".into()));
}

// ── Подтверждение установки и окно «оформление отстало» ────────────────────
//
// Эталон — код `confirm_de_install` и `offer_aeroshell_rebuild` на день
// переноса: развилка по вердикту, сборка тела вопроса, тексты окон и блок
// «Подробнее». ⛔ Ничего не устанавливается, Qt не стартует.

fn verdict(allowed: bool, outcome: &str, title: &str, explanation: &str) -> dp::InstallVerdict {
    dp::InstallVerdict {
        allowed,
        outcome: outcome.to_string(),
        title: title.to_string(),
        explanation: explanation.to_string(),
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn blocked_verdict_gets_window_without_install_button() {
    let _lang = i18n_pin::ru();
    let v = verdict(false, "blocked", "Astra Linux SE: ЗПС включена", "Что мешает\nЧто делать");
    match dp::confirm_install_plan(Some(&v), "Установить KDE?") {
        dp::ConfirmPlan::Blocked {
            window_title,
            text,
            informative,
            button,
        } => {
            assert_eq!(window_title, "Установка невозможна");
            assert_eq!(text, "<b>Astra Linux SE: ЗПС включена</b>");
            // Объяснение уходит как есть: у окна отдельное поле, переносы
            // там и так переносы.
            assert_eq!(informative, "Что мешает\nЧто делать");
            assert_eq!(button, "Понятно");
        }
        other => panic!("отказ должен быть отдельным окном: {other:?}"),
    }
}

#[test]
fn warning_verdict_shares_the_question_window() {
    // Цепочки диалогов подряд в Big Picture неудобны (правило #16):
    // предупреждение попадает В ТО ЖЕ окно вопроса.
    let v = verdict(true, "warning", "ROSA: нет метапакета", "Строка один\nСтрока два");
    match dp::confirm_install_plan(Some(&v), "Установить XFCE?") {
        dp::ConfirmPlan::Ask { body } => {
            assert_eq!(
                body,
                "<b>ROSA: нет метапакета</b><br><br>Строка один<br>Строка два<br><br>\
Установить XFCE?"
            );
        }
        other => panic!("предупреждение не должно отказывать: {other:?}"),
    }
}

#[test]
fn ok_verdict_asks_the_plain_question() {
    let v = verdict(true, "ok", "OK", "");
    match dp::confirm_install_plan(Some(&v), "Установить GNOME?") {
        dp::ConfirmPlan::Ask { body } => assert_eq!(body, "Установить GNOME?"),
        other => panic!("{other:?}"),
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn unknown_verdict_warns_but_does_not_block() {
    let _lang = i18n_pin::ru();
    // «Неизвестно» — это не «запрещено»: иначе на неизвестном дистрибутиве
    // Rexon бесполезен. Но и молчать нельзя.
    match dp::confirm_install_plan(None, "Установить MATE?") {
        dp::ConfirmPlan::Ask { body } => {
            assert!(body.starts_with(&dp::confirm_unknown_note()), "{body}");
            assert!(body.ends_with("Установить MATE?"), "{body}");
        }
        other => panic!("проверка не удалась — но запрещать нельзя: {other:?}"),
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn aeroshell_details_describe_every_package() {
    let _lang = i18n_pin::ru();
    let packages = vec![
        dp::AeroshellPackage {
            package: "aeroshell-plasma".into(),
            built_for: "6.4.3".into(),
            installed: "6.5.0".into(),
            stale: true,
        },
        dp::AeroshellPackage {
            package: "aeroshell-decoration".into(),
            built_for: "6.5.0".into(),
            installed: "6.5.0".into(),
            stale: false,
        },
        // Версия сборки не указана — обе половины считаются неизвестными.
        dp::AeroshellPackage {
            package: "aeroshell-icons".into(),
            built_for: String::new(),
            installed: "6.5.0".into(),
            stale: false,
        },
    ];
    assert_eq!(
        dp::aeroshell_details(&packages),
        "aeroshell-plasma: собрано под 6.4.3, установлено 6.5.0 — устарело\n\
aeroshell-decoration: собрано под 6.5.0, установлено 6.5.0 — в порядке\n\
aeroshell-icons: версия сборки не указана"
    );
    assert_eq!(dp::aeroshell_details(&[]), "");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn aeroshell_result_keeps_the_builder_output() {
    let _lang = i18n_pin::ru();
    let (title, body) = dp::aeroshell_result(true, "неважно");
    assert_eq!(title, "Оформление восстановлено");
    assert!(!body.contains("неважно"), "при удаче вывод сборщика не нужен");

    let (title, body) = dp::aeroshell_result(false, "makepkg: ошибка &lt;3&gt;");
    assert_eq!(title, "Восстановить не удалось");
    assert!(body.starts_with("makepkg: ошибка &lt;3&gt;<br><br>"), "{body}");
    // Главное в окне неудачи — что система цела.
    assert!(body.contains("система не пострадала"), "{body}");
}

/// Кнопка согласия называет то же, что заголовок окна: при одном только
/// риске («нужно пересобрать») — «Пересобрать», при поломке («перестало
/// работать») — «Восстановить».
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn aeroshell_confirm_button_matches_the_title() {
    use rexon_core::aeroshell::{self, Marker, PackageVerdict, Report};
    let _lang = i18n_pin::ru();
    let report = |built: &str| Report {
        stale: vec!["aeroshell-libplasma-git".to_string()],
        rebuild: vec!["aeroshell-libplasma-git".to_string()],
        needs_rebuild: true,
        packages: vec![PackageVerdict {
            package: "aeroshell-libplasma-git".to_string(),
            marker: Marker::PlasmaLibrary,
            built_for: Some(built.to_string()),
            installed: Some("6.7.5".to_string()),
            compiled: true,
            stale: true,
            problems: vec!["libcorebindingsplugin.so: Qt 6.11 private API, installed Qt 6.12".to_string()],
        }],
        ..Report::empty()
    };

    // Собрано под ту же Plasma, разошёлся только Qt — риск, стол работает.
    let risk = report("6.7.5");
    assert!(risk.at_risk_only());
    assert_eq!(aeroshell::confirm_title_for(&risk), "Оформление нужно пересобрать");
    assert_eq!(aeroshell::confirm_button_for(&risk), "🛠️ Пересобрать");

    // Версия Plasma разошлась — поломка, подпись прежняя.
    let broken = report("6.7.4");
    assert!(!broken.at_risk_only());
    assert_eq!(aeroshell::confirm_title_for(&broken), "Оформление перестало работать");
    assert_eq!(aeroshell::confirm_button_for(&broken), "🛠️ Восстановить");
}

// ── Английский интерфейс ───────────────────────────────────────────────────

fn cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Все подписи окна хода и окон подтверждения по-английски — ни одной
/// русской буквы: забытая строка видна здесь, а не покупателю.
#[test]
fn english_progress_has_no_russian() {
    let _lang = i18n_pin::en();
    let mut all: Vec<String> = vec![
        dp::initial_status(),
        dp::close_text_busy(),
        dp::close_text_done(),
        dp::status_done(),
        dp::status_failed(),
        dp::log_done_plain(),
        dp::confirm_blocked_title(),
        dp::confirm_blocked_button(),
        dp::confirm_unknown_note(),
        dp::aeroshell_title(),
        dp::aeroshell_text(),
        dp::aeroshell_informative(),
        dp::aeroshell_fix_button(),
        dp::aeroshell_rebuild_button(),
        dp::aeroshell_skip_button(),
        dp::aeroshell_run_title(),
        dp::aeroshell_ok_title(),
        dp::aeroshell_ok_text(),
        dp::aeroshell_fail_title(),
        dp::aeroshell_fail_note(),
    ];
    for (stage, _) in dp::STAGE_TITLES {
        all.push(dp::stage_title(stage).expect("заголовок этапа"));
    }
    let done = dp::on_done(true, "");
    all.extend([done.status, done.close_text, done.log_line.unwrap()]);
    all.push(dp::on_done(false, "x").status);
    all.push(dp::aeroshell_details(&[
        dp::AeroshellPackage {
            package: "aeroshell-plasma".into(),
            built_for: "6.4.3".into(),
            installed: "6.5.0".into(),
            stale: true,
        },
        dp::AeroshellPackage {
            package: "aeroshell-icons".into(),
            built_for: String::new(),
            installed: String::new(),
            stale: false,
        },
    ]));
    let (t, b) = dp::aeroshell_result(false, "out");
    all.extend([t, b]);
    let (t, b) = dp::aeroshell_result(true, "out");
    all.extend([t, b]);
    assert!(all.len() > 40, "сверять было нечего: {}", all.len());
    for text in &all {
        assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
    }
}

/// Английские фразы читаются как фразы.
#[test]
fn english_progress_reads_naturally() {
    let _lang = i18n_pin::en();
    assert_eq!(dp::stage_title("install").as_deref(), Some("Installing packages…"));
    assert_eq!(dp::stage_title("done").as_deref(), Some("Finished"));
    assert_eq!(dp::initial_status(), "Preparing…");
    assert_eq!(dp::on_done(true, "").log_line.as_deref(), Some("\n✅ Done"));
    assert_eq!(dp::on_done(false, "").status, "Failed ❌");
    let v = verdict(false, "blocked", "Blocked", "Why");
    match dp::confirm_install_plan(Some(&v), "Install KDE?") {
        dp::ConfirmPlan::Blocked { window_title, button, .. } => {
            assert_eq!(window_title, "Can't Install");
            assert_eq!(button, "Got It");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        dp::aeroshell_details(&[dp::AeroshellPackage {
            package: "aeroshell-plasma".into(),
            built_for: "6.4.3".into(),
            installed: "6.5.0".into(),
            stale: true,
        }]),
        "aeroshell-plasma: built for 6.4.3, installed 6.5.0 — outdated"
    );
    let (title, body) = dp::aeroshell_result(false, "makepkg: error");
    assert_eq!(title, "Couldn't Repair");
    assert_eq!(
        body,
        "makepkg: error<br><br>Your appearance was left as it was — your system is fine. \
Try again later."
    );
}
