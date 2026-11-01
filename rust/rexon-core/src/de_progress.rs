//! Порт `rexon/qt_main/de_progress.py` — показа хода операций с рабочими
//! столами (установка, переключение, удаление).
//!
//! Сам диалог остаётся за Qt: полоса, заголовок, строка текущего пакета,
//! журнал и кнопка закрытия. Здесь — всё, что решает, ЧТО в них положить:
//! названия этапов, разбор доли выполнения, куда попадает сообщение и с
//! какой пометкой, каким становится диалог по завершении. Ровно это
//! понадобится и виджетной вкладке, и будущей QML — движок решений один
//! (правило #12).
//!
//! Подписи идут через каталог перевода (`crate::i18n`): исходный язык —
//! английский, русский живёт в `po/ru.po`.

use crate::i18n::{n_, tr, trf};

/// Заголовки этапов. Этап, которого здесь нет, заголовок НЕ меняет —
/// так ведёт себя и `warning`, и любой незнакомый этап.
///
/// Это первая строка окна хода — её читает покупатель, поэтому она простым
/// русским, без служебных слов («defaults», «lock», «MIME», «Display
/// manager», «бэкап»). Названия технологий (KDE Plasma, AUR) — латиницей,
/// как везде в продукте. Прежние заголовки переписаны 13.09.2026; Python-путь
/// берёт их отсюда же, через мост.
///
/// Вторая колонка — `msgid` каталога; переводится в [`stage_title`].
pub const STAGE_TITLES: &[(&str, &str)] = &[
    ("snapshot", n_("Remembering your default apps…")),
    ("plasma_backup", n_("Saving a copy of the KDE Plasma panel…")),
    ("config_backup", n_("Saving a copy of your desktop settings…")),
    ("wait_lock", n_("Waiting for another package installation to finish…")),
    ("install", n_("Installing packages…")),
    ("done_main", n_("Main packages installed")),
    ("aur", n_("Installing packages from the AUR…")),
    ("aur_done", n_("AUR packages installed")),
    ("aur_warn", n_("Some AUR packages failed to install")),
    ("post_install", n_("Finishing setup…")),
    ("dm_enable", n_("Setting up the login screen…")),
    ("switcher", n_("Setting up default apps…")),
    ("restore", n_("Restoring your default apps…")),
    ("mime", n_("Choosing which apps open your files…")),
    ("remove", n_("Removing packages…")),
    ("done", n_("Finished")),
    ("error", n_("Error")),
];

/// Заголовок этапа на языке интерфейса, если он у этапа есть.
pub fn stage_title(stage: &str) -> Option<String> {
    STAGE_TITLES
        .iter()
        .find(|(k, _)| *k == stage)
        .map(|(_, v)| tr(v))
}

/// Наибольшее значение полосы: доля выполнения раскладывается на 0..1000.
pub const BAR_MAX: i32 = 1000;

/// Состояние диалога до первого сообщения.
pub fn initial_status() -> String {
    tr("Preparing…")
}
/// Надпись на кнопке закрытия, пока операция идёт.
pub fn close_text_busy() -> String {
    tr("Please wait…")
}
/// Надпись на кнопке закрытия, когда всё кончилось.
pub fn close_text_done() -> String {
    tr("Close")
}
/// Заголовок при удачном завершении.
pub fn status_done() -> String {
    tr("Finished ✅")
}
/// Заголовок при неудаче.
pub fn status_failed() -> String {
    tr("Failed ❌")
}
/// Итог в журнале, когда сказать по существу нечего.
pub fn log_done_plain() -> String {
    format!("\n{}", tr("✅ Done"))
}
/// Сообщение об удаче, которое считается пустым.
pub const MSG_OK: &str = "ok";

/// Этап, сообщение которого идёт в журнал БЕЗ пометки и без обрезки пробелов
/// с краёв — сырой вывод пакетного менеджера.
pub const STAGE_RAW: &str = "raw";
/// Этап, сообщение которого показывается строкой текущего пакета.
pub const STAGE_PKG_INSTALL: &str = "pkg_install";
/// Этапы, сообщение которых тоже идёт в строку текущего пакета, но только
/// когда оно непустое.
pub const STAGES_CURRENT_IF_SET: &[&str] = &["download", "check"];
/// Этап, сообщение которого помечается в журнале как ошибка.
pub const STAGE_ERROR: &str = "error";
/// Этап, сообщение которого помечается в журнале как предупреждение.
pub const STAGE_WARNING: &str = "warning";

/// Пометка перед сообщением об ошибке в журнале.
pub const PREFIX_ERROR: &str = "❌ ";
/// Пометка перед предупреждением в журнале.
pub const PREFIX_WARNING: &str = "⚠️ ";

/// Что изменить в диалоге по одному сообщению о ходе дела.
///
/// `None` означает «не трогать»: у полосы это доля, которую показывать
/// нельзя, у остальных — что этот этап их не касается.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgressUpdate {
    /// Новое значение полосы, 0..=[`BAR_MAX`].
    pub bar: Option<i32>,
    /// Новый заголовок.
    pub status: Option<String>,
    /// Новая строка текущего пакета; пустая строка означает «очистить».
    pub current: Option<String>,
    /// Строка, дописываемая в журнал отдельным абзацем.
    pub log_line: Option<String>,
}

/// Значение полосы по доле выполнения.
///
/// Доля усекается к нулю, как это делает приведение к целому в оригинале.
/// Значение вне 0..=[`BAR_MAX`] полосу не двигает — так его и молча
/// отбрасывал Qt, поэтому отдаём `None`.
///
/// ⚠️ ПОЧИНЕНО (правило #16). Бесконечная доля роняла диалог: оригинал ловил
/// только `TypeError`/`ValueError`, а приведение бесконечности к целому даёт
/// `OverflowError`. Прилетело бы такое из подсчёта «сделано/всего» при нуле
/// в знаменателе — и показ установки обрывался бы посреди работы. Теперь
/// бесконечность и «не число» ведут себя одинаково: полоса не двигается.
///
/// Проверка на конечность оставлена явной, хотя сама по себе она на итог не
/// влияет: приведение к целому в Rust не переполняется, а упирается в предел
/// типа, и такое значение всё равно не попадает в полосу. Мутацией эту
/// строку не поймать именно поэтому — здесь она стоит как объяснение, а не
/// как защита.
pub fn bar_value(fraction: Option<f64>) -> Option<i32> {
    let f = fraction?;
    if !f.is_finite() {
        return None;
    }
    let value = (f * 1000.0) as i32;
    (0..=BAR_MAX).contains(&value).then_some(value)
}

/// Сообщение, у которого пометка уже стоит, идёт в журнал как есть.
fn return_log(message: &str) -> Option<String> {
    Some(message.to_string())
}

/// Разобрать сообщение о ходе дела.
pub fn on_progress(stage: &str, message: &str, fraction: Option<f64>) -> ProgressUpdate {
    let mut update = ProgressUpdate {
        bar: bar_value(fraction),
        status: stage_title(stage),
        ..Default::default()
    };

    if stage == STAGE_PKG_INSTALL {
        // Пустое сообщение здесь ЗАТИРАЕТ строку текущего пакета — оригинал
        // ставил её без проверки.
        update.current = Some(message.to_string());
    } else if STAGES_CURRENT_IF_SET.contains(&stage) && !message.is_empty() {
        update.current = Some(message.to_string());
    }

    update.log_line = if stage == STAGE_RAW {
        let text = message.trim();
        (!text.is_empty()).then(|| text.to_string())
    } else if message.is_empty() {
        None
    } else {
        let prefix = match stage {
            STAGE_ERROR => PREFIX_ERROR,
            STAGE_WARNING => PREFIX_WARNING,
            _ => "",
        };
        // ⚠️ Пометку ставим ОДИН раз. Разбор вывода пакетного менеджера
        // (`system_plan::parse_pacman_line`) собирает сообщение уже с
        // крестом/восклицанием, и слепое склеивание давало в журнале
        // «❌ ❌ failed to commit transaction» — двойной крест там, где беда
        // всего одна. Поймано показом журнала целиком при разборе установки
        // Budgie и LXQt.
        if !prefix.is_empty() && message.starts_with(prefix.trim_end()) {
            return_log(message)
        } else {
            Some(format!("{prefix}{message}"))
        }
    };

    update
}

/// Каким становится диалог, когда операция кончилась.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneUpdate {
    /// Новое значение полосы; при неудаче она остаётся как была.
    pub bar: Option<i32>,
    pub status: String,
    /// Новая строка текущего пакета; по завершении она очищается всегда.
    pub current: Option<String>,
    /// Строка в журнал; при неудаче ничего не дописывается — текст ошибки
    /// уже показан этапом `error`, воркер это обеспечивает
    /// (см. [`worker_error_emit`]).
    pub log_line: Option<String>,
    pub close_text: String,
    pub close_enabled: bool,
}

/// Разобрать завершение операции.
pub fn on_done(ok: bool, msg: &str) -> DoneUpdate {
    if ok {
        let log_line = if !msg.is_empty() && msg != MSG_OK {
            format!("\n✅ {msg}")
        } else {
            log_done_plain()
        };
        DoneUpdate {
            bar: Some(BAR_MAX),
            status: status_done(),
            // Строка текущего шага под итогом гаснет и при удаче: иначе под
            // «Завершено ✅» оставалась висеть последняя сырая строка
            // пакетного менеджера («looking for conflicting packages...»,
            // живая установка River 24.10.2026). Итог говорит статус.
            current: Some(String::new()),
            log_line: Some(log_line),
            close_text: close_text_done(),
            close_enabled: true,
        }
    } else {
        DoneUpdate {
            bar: None,
            status: status_failed(),
            current: Some(String::new()),
            log_line: None,
            close_text: close_text_done(),
            close_enabled: true,
        }
    }
}

/// Должен ли исполнитель сам объявить об ошибке, когда операция кончилась.
///
/// Правило оригинала: если операция не удалась, ошибку по ходу дела никто не
/// объявлял, а сказать есть что — объявить её самим, чтобы текст непременно
/// попал в журнал единым путём. Возвращается сообщение для этапа
/// [`STAGE_ERROR`].
pub fn worker_error_emit(ok: bool, error_seen: bool, msg: &str) -> Option<String> {
    (!ok && !error_seen && !msg.is_empty()).then(|| msg.to_string())
}

// ── Подтверждение установки рабочего стола ─────────────────────────────────
//
// `confirm_de_install` — ЕДИНСТВЕННОЕ место, где вкладки спрашивают
// «ставим?». Само решение «можно ли ставить» живёт в
// [`crate::distro_compat`]; сюда портировано то, что делалось с этим
// решением дальше и лежало второй копией в Qt-шиме: какое из трёх окон
// показать и что в нём написать.

/// Заголовок окна, которым отказывают в установке.
pub fn confirm_blocked_title() -> String {
    tr("Can't Install")
}
/// Подпись единственной кнопки в окне отказа. Не «OK»: человеку сообщают
/// новость, а не спрашивают согласия (правило #13).
pub fn confirm_blocked_button() -> String {
    tr("Got It")
}
/// Приписка, когда проверить совместимость не удалось вовсе.
///
/// Молчать тут нельзя: «неизвестно» — это не «разрешено». Установку не
/// запрещаем (иначе на неизвестном дистрибутиве Rexon бесполезен), но
/// честно предупреждаем, что предупреждений не будет.
pub fn confirm_unknown_note() -> String {
    tr("Couldn't check whether your distribution is compatible: the Rexon core \
        isn't available. You can still install, but you won't be warned about \
        your distribution's quirks.")
}

/// Исход проверки, как его отдаёт `distro_compat.explain_de_install`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallVerdict {
    /// Разрешена ли установка.
    pub allowed: bool,
    /// `"ok"` / `"warning"` / `"blocked"`.
    pub outcome: String,
    /// Заголовок причины на языке человека.
    pub title: String,
    /// Объяснение: что мешает и что с этим делать. Переносы строк — `\n`.
    pub explanation: String,
}

/// Какое окно показать перед установкой рабочего стола.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmPlan {
    /// Отказ: окно БЕЗ кнопки «установить». Предлагать нажать кнопку, за
    /// которой заведомо ошибка, — то же самое, что не предупредить вовсе.
    Blocked {
        window_title: String,
        /// Текст-заголовок окна (простой HTML).
        text: String,
        /// Развёрнутое объяснение (обычный текст, переносы `\n`).
        informative: String,
        button: String,
    },
    /// Вопрос «ставим?». Предупреждение (если есть) вошло В ЭТО ЖЕ окно, а
    /// не вторым диалогом подряд: цепочки диалогов неудобны в режиме
    /// Big Picture (архитектурное правило #16).
    Ask {
        /// Тело вопроса, простой HTML (`<b>`, `<br>`).
        body: String,
    },
}

/// Что показать перед установкой — порт развилки `confirm_de_install`.
///
/// `verdict` = `None` означает «проверить не удалось»: вопрос задаётся, но с
/// припиской [`confirm_unknown_note`]. `question` — текст вопроса вкладки
/// (простой HTML).
pub fn confirm_install_plan(verdict: Option<&InstallVerdict>, question: &str) -> ConfirmPlan {
    match verdict {
        Some(v) if !v.allowed => ConfirmPlan::Blocked {
            window_title: confirm_blocked_title(),
            text: format!("<b>{}</b>", v.title),
            informative: v.explanation.clone(),
            button: confirm_blocked_button(),
        },
        None => ConfirmPlan::Ask {
            body: format!("{}<br><br>{question}", confirm_unknown_note()),
        },
        Some(v) if v.outcome == "warning" => ConfirmPlan::Ask {
            body: format!(
                "<b>{}</b><br><br>{}<br><br>{question}",
                v.title,
                v.explanation.replace('\n', "<br>")
            ),
        },
        Some(_) => ConfirmPlan::Ask {
            body: question.to_string(),
        },
    }
}

// ── Оформление AeroShell отстало от обновлённой системы ────────────────────
//
// Пакеты оформления собраны под ту версию рабочего стола, что стояла в
// момент сборки. Система обновилась — оформление перестаёт работать МОЛЧА:
// пропадает панель задач, меню, подсветка кнопок и прозрачность, а в тяжёлом
// случае не открывается экран блокировки и человек видит чёрный экран.
//
// Предложение восстановить показывают двое — главное окно при запуске и
// кнопка в настройках, — поэтому тексты и разбор состояния живут в одном
// месте (правило #12).

/// Заголовок окна с предложением восстановить оформление.
pub fn aeroshell_title() -> String {
    tr("Desktop Appearance Stopped Working")
}
/// Главный текст предложения (простой HTML).
pub fn aeroshell_text() -> String {
    tr("<b>After a system update, your desktop appearance no longer fits the \
        new version of the desktop.</b>")
}
/// Заголовок того же окна, когда оформление ещё работает, но собрано под
/// прежнюю версию системы (`aeroshell::Report::at_risk_only`).
pub fn aeroshell_risk_title() -> String {
    tr("Desktop Appearance Needs Rebuilding")
}
/// Главный текст окна при одном только риске (простой HTML).
pub fn aeroshell_risk_text() -> String {
    tr("<b>After a system update, your desktop appearance is still built for \
        the previous version of the system. It works for now, but it can fail \
        at any moment.</b>")
}
/// Развёрнутое объяснение: чем это грозит и что сделает Rexon.
pub fn aeroshell_informative() -> String {
    tr("Because of this, the taskbar, the Start menu, button highlights and \
        window transparency may disappear, and the lock screen may stop \
        opening.<br><br>Rexon will rebuild the appearance for the new version. \
        This takes a few minutes; you'll see the progress in a window, and at \
        the end you'll need to log out and log back in.")
}
/// Подпись кнопки согласия.
pub fn aeroshell_fix_button() -> String {
    tr("🛠️ Repair")
}
/// Подпись той же кнопки при одном только риске: окно говорит «нужно
/// пересобрать», и кнопка называет то же действие теми же словами.
pub fn aeroshell_rebuild_button() -> String {
    tr("🛠️ Rebuild")
}
/// Подпись кнопки отказа. ⛔ Восстановление НЕ запускается само: сборка
/// занимает минуты и грузит машину, посреди работы это недопустимо.
pub fn aeroshell_skip_button() -> String {
    tr("Not Now")
}
/// Заголовок окна хода восстановления.
pub fn aeroshell_run_title() -> String {
    tr("Repairing Desktop Appearance")
}
/// Заголовок окна удачи.
pub fn aeroshell_ok_title() -> String {
    tr("Desktop Appearance Repaired")
}
/// Текст окна удачи.
pub fn aeroshell_ok_text() -> String {
    tr("The appearance has been rebuilt for the current system version.\
        <br><br><b>Log out and log back in</b> to get the panel, menu and \
        effects back.")
}
/// Заголовок окна неудачи.
pub fn aeroshell_fail_title() -> String {
    tr("Couldn't Repair")
}
/// Приписка к выводу сборщика при неудаче: главное — что система цела.
pub fn aeroshell_fail_note() -> String {
    tr("Your appearance was left as it was — your system is fine. Try again later.")
}

/// Состояние одного пакета оформления в сверке.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AeroshellPackage {
    pub package: String,
    /// Версия рабочего стола, под которую пакет собран (пусто — не указана).
    pub built_for: String,
    /// Версия, установленная сейчас (пусто — не определилась).
    pub installed: String,
    /// Отстал ли пакет.
    pub stale: bool,
}

/// Подробности сверки для раскрывающегося блока диалога — порт
/// `_aeroshell_details`.
///
/// Имена пакетов и версии покупателю ни о чём не говорят, поэтому они не в
/// главном тексте, а в «Подробнее».
pub fn aeroshell_details(packages: &[AeroshellPackage]) -> String {
    packages
        .iter()
        .map(|item| {
            if !item.built_for.is_empty() && !item.installed.is_empty() {
                let state = if item.stale { tr("outdated") } else { tr("fine") };
                trf(
                    // TRANSLATORS: %1 is a package name, %2 and %3 are desktop
                    // versions, %4 is “outdated” or “fine”.
                    "%1: built for %2, installed %3 — %4",
                    &[&item.package, &item.built_for, &item.installed, &state],
                )
            } else {
                trf("%1: build version not specified", &[&item.package])
            }
        })
        .collect::<Vec<String>>()
        .join("\n")
}

/// Итог восстановления оформления: заголовок окна и текст (простой HTML).
///
/// `escaped_output` — вывод сборщика, УЖЕ экранированный шимом: это сырой
/// текст чужой программы, и без экранирования он ломает разметку окна.
pub fn aeroshell_result(ok: bool, escaped_output: &str) -> (String, String) {
    if ok {
        (aeroshell_ok_title(), aeroshell_ok_text())
    } else {
        (
            aeroshell_fail_title(),
            format!("{escaped_output}<br><br>{}", aeroshell_fail_note()),
        )
    }
}
