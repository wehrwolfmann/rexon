// LinkGuard — QML-синглтон ссылок из чужого текста.
//
// Описания и комментарии товаров, сведения о пакете AUR, описания
// расширений: всё, что пишут чужие люди, показывается общим компонентом
// `RexonLinkText.qml`, а он спрашивает этот синглтон, что делать с нажатой
// ссылкой. Решение (своя карточка / вопрос / сразу), пометка ↗, тексты окна
// и список доверенных сайтов — ядро (`rexon_core::link_policy`, там же
// причина и тесты). Здесь только оболочка QObject и открытие в браузере.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        type LinkGuard = super::LinkGuardRust;

        /// Что делать со ссылкой: строка JSON `{action, site, url, card,
        /// title, text, hint, remember, open, cancel, refused, cardFailed}`;
        /// `action` — `page`, `card`, `open`, `ask` или `refuse`.
        #[qinvokable]
        fn decide(self: &LinkGuard, url: &QString) -> QString;

        /// Открыть в браузере (после вопроса или для доверенного сайта).
        /// `remember` — «Больше не спрашивать для этого сайта».
        #[qinvokable]
        fn open(self: &LinkGuard, url: &QString, remember: bool) -> bool;

        /// Разметка с ↗ у внешних ссылок.
        #[qinvokable]
        fn mark(self: &LinkGuard, html: &QString) -> QString;

        /// Строка «какие сайты открываются без вопроса» для «Настроек».
        #[qinvokable]
        #[cxx_name = "trustedSummary"]
        fn trusted_summary(self: &LinkGuard) -> QString;

        /// Снова спрашивать для всех сайтов. → итог для строки состояния.
        #[qinvokable]
        #[cxx_name = "forgetAll"]
        fn forget_all(self: &LinkGuard) -> QString;
    }
}

use cxx_qt_lib::QString;
use rexon_core::link_policy::{self as lp, LinkAction};
use serde_json::json;

#[derive(Default)]
pub struct LinkGuardRust;

impl qobject::LinkGuard {
    fn decide(&self, url: &QString) -> QString {
        let url = url.to_string();
        let action = lp::decide(&url, &lp::live_trusted());
        let (kind, site, card) = match &action {
            LinkAction::Page => ("page", String::new(), serde_json::Value::Null),
            LinkAction::Card(r) => ("card", String::new(), serde_json::to_value(r).unwrap_or_default()),
            LinkAction::Open => ("open", lp::site_of(&url).unwrap_or_default(), serde_json::Value::Null),
            LinkAction::Ask { site } => ("ask", site.clone(), serde_json::Value::Null),
            LinkAction::Refuse => ("refuse", String::new(), serde_json::Value::Null),
        };
        match &action {
            LinkAction::Refuse => rexon_core::log_warn!("link", "ссылка не открыта (схема): {url}"),
            LinkAction::Card(r) => rexon_core::log_info!("link", "ссылка на товар → карточка: {r:?}"),
            LinkAction::Ask { .. } => rexon_core::log_info!("link", "вопрос перед внешней ссылкой: {url}"),
            _ => {}
        }
        // Вопрос всегда о сайте; для карточки — тоже: если открыть её не
        // выйдет, страница спросит о том же адресе, не заводя второго окна.
        let ask_site = if matches!(action, LinkAction::Card(_)) {
            lp::site_of(&url).unwrap_or_default()
        } else {
            site.clone()
        };
        QString::from(
            &json!({
                "action": kind,
                "site": ask_site,
                "url": url,
                "card": card,
                "title": lp::ask_title(),
                "text": lp::ask_text(&ask_site, &url),
                "hint": lp::ask_hint(),
                "remember": lp::remember_label(),
                "open": lp::open_label(&ask_site),
                "cancel": lp::cancel_label(),
                "refused": lp::refused_status(&url),
                "cardFailed": lp::card_failed_status(),
            })
            .to_string(),
        )
    }

    fn open(&self, url: &QString, remember: bool) -> bool {
        let url = url.to_string();
        // Решение ещё раз: окну могли подсунуть то, что открывать нельзя.
        if matches!(lp::decide(&url, &[]), LinkAction::Refuse | LinkAction::Page) {
            rexon_core::log_warn!("link", "открытие отклонено: {url}");
            return false;
        }
        if remember {
            if let Some(site) = lp::site_of(&url) {
                match lp::live_remember(&site) {
                    Ok(()) => rexon_core::log_info!("link", "сайт {site} — больше не спрашивать"),
                    Err(e) => rexon_core::log_warn!("link", "не запомнил сайт {site}: {e}"),
                }
            }
        }
        rexon_core::log_info!("link", "открыта внешняя ссылка: {url}");
        crate::appearance_actions::qobject::open_url_external(&QString::from(&url))
    }

    fn mark(&self, html: &QString) -> QString {
        QString::from(&lp::mark_external(&html.to_string()))
    }

    fn trusted_summary(&self) -> QString {
        QString::from(&lp::trusted_summary(&lp::live_trusted()))
    }

    fn forget_all(&self) -> QString {
        let result = lp::live_forget_all();
        match &result {
            Ok(n) => rexon_core::log_info!("link", "доверенные сайты сброшены ({n})"),
            Err(e) => rexon_core::log_warn!("link", "сброс доверенных сайтов: {e}"),
        }
        QString::from(&lp::forget_status(&result))
    }
}
