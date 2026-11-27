// PluginTranslations — the QML singleton of waywallen daemon plugin labels.
//
// It appeared in waywallen 0.3.7 together with localizable labels: a
// plugin gives a label as a `PluginMessage{plugin_id, msgid}` pair, and the
// daemon sends the translation as `.po` catalogues on a
// `plugin_translation_list` request. The vendored `I18n.qml` (0.3.7)
// consists exactly of a call to this type, and without it neither it nor
// the seven pages that call it compile.
//
// ⭐ For US it is not a replacement for our Russian layer but its
// continuation. The order of resolving a label is in the core
// (`rexon_core::plugin_l10n`, together with its rationale and tests):
//
//   1. the plugin's own catalogue (its author's translation);
//   2. our `remote_l10n`;
//   3. the `msgid` itself.
//
// Measured on a live 0.3.7 daemon: a Russian catalogue exists for
// `wallhaven`, `image`, `video`, while
// `org.waywallen.open-wallpaper-engine` (Steam Workshop) has only zh-CN.
// So without item 2 all the Workshop Russian would be lost, and without
// item 1 we would silently override the plugin author's translation with
// our own.
//
// There is no logic here (the crate rule): parsing `.po`, locale tags and
// the order itself are in the core. The bridge is left with the QObject
// wrapper, the request to the daemon and receiving the answer.

/// cxx-qt bridge: the `PluginTranslations` singleton.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        /// Catalogue update counter. The vendored `I18n.tr()` reads it on the FIRST
        /// line precisely for the binding dependency: a catalogue arrived — all
        /// labels recompute by themselves.
        /// Warning: `f64`, not `i64`: we do not expose a 64-bit integer to QML (the
        /// analysis is in `src/qvalue.rs`). The counter inside stays `i64`.
        #[qproperty(f64, revision, READ = revision, NOTIFY = revision_changed)]
        /// Interface language (`ru`, `ru-RU`, `zh-CN`). Taken from the environment
        /// by default.
        #[qproperty(QString, locale, READ = locale, WRITE = set_locale,
                    NOTIFY = locale_changed)]
        type PluginTranslations = super::PluginTranslationsRust;

        #[qsignal]
        #[cxx_name = "revisionChanged"]
        fn revision_changed(self: Pin<&mut PluginTranslations>);
        #[qsignal]
        #[cxx_name = "localeChanged"]
        fn locale_changed(self: Pin<&mut PluginTranslations>);

        fn revision(self: &PluginTranslations) -> f64;
        fn locale(self: &PluginTranslations) -> QString;
        #[cxx_name = "setLocale"]
        fn set_locale(self: Pin<&mut PluginTranslations>, value: &QString);

        /// A label for the customer from a protocol `PluginMessage`.
        ///
        /// Accepts what our bridge puts into the model: a map `{pluginId, msgid}`
        /// (or `plugin_id`/`msg_id` — in case of a different key layout), a bare
        /// string and an empty value.
        #[qinvokable]
        fn translate(self: Pin<&mut PluginTranslations>, value: &QVariant) -> QString;

        /// Request the catalogues from the daemon.
        #[qinvokable]
        fn reload(self: Pin<&mut PluginTranslations>);

        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PluginTranslations>, token: i64,
                            frame: &QByteArray);
    }

    impl cxx_qt::Threading for PluginTranslations {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QByteArray, QString, QVariant};
use prost::Message;
use rexon_core::plugin_l10n::{self, Catalogs};
use rexon_core::proto::control as pb;
use serde_json::Value;

use crate::qvalue::variant_to_json;

/// Singleton state.
pub struct PluginTranslationsRust {
    /// Parsed catalogues: `plugin_id` → `locale` → messages.
    catalogs: Catalogs,
    /// The daemon's catalogue generation (its own `generation`).
    generation: u64,
    /// Update counter for QML bindings.
    revision: i64,
    /// Interface language.
    locale: String,
    /// A request has already been sent on the current connection.
    requested: bool,
    /// Token of the sent request (other answers are ignored).
    pending: Option<i64>,
}

impl Default for PluginTranslationsRust {
    fn default() -> Self {
        Self {
            catalogs: Catalogs::new(),
            generation: 0,
            revision: 0,
            locale: env_locale(),
            requested: false,
            pending: None,
        }
    }
}

/// Interface language from the environment: `LANGUAGE`, `LC_ALL`,
/// `LC_MESSAGES`, `LANG`.
///
/// There is no copy of the loop of our own here any more: the same locale
/// is needed for the human-readable theme names in the installed items
/// scanner, and two copies of one environment read drift apart silently —
/// the engine is one, in the core.
fn env_locale() -> String {
    rexon_core::utils::env_locale()
}

/// Parse a value from QML into a triple `(plugin_id, msgid, our
/// translation)`.
///
/// `rexonFallback` is OUR field in the same map: a label computed by key
/// (`rexon_core::remote_l10n::translate_source`). A `PluginMessage` has no
/// sort or filter key at all, while our tables are keyed for them — without
/// this field the Russian would be lost for all sorts and filter titles at
/// once.
///
/// A bare string is a `msgid` without a plugin: it has no catalogue, and
/// the label goes straight through our layer. `null`/empty behave the same.
fn message_parts(value: &Value) -> (String, String, String, Vec<String>) {
    let text = |v: Option<&Value>| -> String {
        v.and_then(Value::as_str).unwrap_or_default().to_string()
    };
    match value {
        Value::String(s) => (String::new(), s.clone(), String::new(), Vec::new()),
        Value::Object(map) => {
            let plugin_id = {
                let by_camel = text(map.get("pluginId"));
                if by_camel.is_empty() {
                    text(map.get("plugin_id"))
                } else {
                    by_camel
                }
            };
            let msgid = {
                let by_plain = text(map.get("msgid"));
                if by_plain.is_empty() {
                    text(map.get("msgId"))
                } else {
                    by_plain
                }
            };
            // waywallen 0.3.8: a label may carry substitutions `%1`, `%2` — they are
            // substituted AFTER translation, so that the catalogue is searched for the
            // template, not for the glued string.
            let arguments = map
                .get("arguments")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|v| v.as_str().unwrap_or_default().to_string())
                        .collect()
                })
                .unwrap_or_default();
            (plugin_id, msgid, text(map.get("rexonFallback")), arguments)
        }
        _ => (String::new(), String::new(), String::new(), Vec::new()),
    }
}

impl qobject::PluginTranslations {
    pub fn revision(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.revision)
    }

    pub fn locale(&self) -> QString {
        QString::from(&self.locale)
    }

    pub fn set_locale(mut self: Pin<&mut Self>, value: &QString) {
        let value = value.to_string();
        if value == self.locale {
            return;
        }
        self.as_mut().rust_mut().locale = value;
        self.as_mut().locale_changed();
        // The catalogues are already at hand and do not need changing — but labels
        // are chosen by language, so the bindings must recompute.
        self.bump_revision();
    }

    /// A label by `PluginMessage`.
    ///
    /// Warning: it is called for EVERY label of the page, so there is only a
    /// map lookup here: `.po` parsing is done once, when the answer is received.
    pub fn translate(mut self: Pin<&mut Self>, value: &QVariant) -> QString {
        // A cheap lazy loading guard: while there is no connection — wait, and on
        // a new connection ask for the catalogues again. The check costs one
        // atomic read, and the request goes out once per connection.
        self.as_mut().ensure_requested();

        let (plugin_id, msgid, fallback, arguments) =
            message_parts(&variant_to_json(value));
        let rust = self.rust();
        QString::from(&plugin_l10n::resolve_with_arguments(
            &rust.catalogs,
            &plugin_id,
            &msgid,
            &fallback,
            &rust.locale,
            &arguments,
        ))
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().requested = true;
        self.send_list_request();
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if self.pending != Some(token) {
            return;
        }
        self.as_mut().rust_mut().pending = None;

        // Warning: the transport hands over the RAW server frame (`ServerFrame`),
        // not the `Response` inside it — as for all queries of the tab.
        //
        // An older daemon does not know the request — the answer will come as an
        // error or will not parse at all. This is NOT a failure: the tab must come
        // up, and the labels will be taken by our layer (items 2-3 of the order).
        let Ok(server_frame) = pb::ServerFrame::decode(frame.as_slice()) else {
            return;
        };
        let Some(pb::server_frame::Kind::Response(resp)) = server_frame.kind else {
            return;
        };
        let Some(pb::response::Payload::PluginTranslationList(list)) = resp.payload else {
            return;
        };
        if list.generation == self.generation && !self.catalogs.is_empty() {
            // The catalogues are the same — there is no reason to poke the bindings.
            return;
        }
        let catalogs = plugin_l10n::build_catalogs(
            list.documents
                .iter()
                .map(|d| (d.plugin_id.as_str(), d.locale.as_str(), d.po.as_slice())),
        );
        {
            let mut rust = self.as_mut().rust_mut();
            rust.catalogs = catalogs;
            rust.generation = list.generation;
        }
        self.bump_revision();
    }

    /// Bump the update counter and tell QML about it.
    ///
    /// Warning: a single `revisionChanged` is not enough. The binding of the
    /// vendored `I18n.tr()` reads `revision` on its first line and recomputes by
    /// itself, but those who translate a label NOT in QML but on their own side
    /// (model roles) have no way to learn about the catalogue change: Qt cached
    /// their value in the delegate. So the subscribers are woken up right here
    /// — see `subscribers`.
    fn bump_revision(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().revision += 1;
        self.revision_changed();
        subscribers::dispatch();
    }

    /// Ask for the catalogues if there is a connection and no request has been
    /// made yet.
    fn ensure_requested(mut self: Pin<&mut Self>) {
        if !crate::discover_queries::connected() {
            // The connection was lost — the next connection will start over: the
            // daemon may have changed together with the set of plugins. We write only
            // when there is something to change: we get here for EVERY label.
            if self.requested {
                self.as_mut().rust_mut().requested = false;
            }
            return;
        }
        if self.requested {
            return;
        }
        self.as_mut().rust_mut().requested = true;
        self.send_list_request();
    }

    /// The `plugin_translation_list` frame into the tab's shared transport.
    fn send_list_request(mut self: Pin<&mut Self>) {
        if !crate::discover_queries::connected() {
            return;
        }
        const FIELD: &str = "plugin_translation_list";
        let token = crate::discover_queries::next_token();
        let payload = pb::PluginTranslationListRequest {}.encode_to_vec();
        let Ok(bytes) = rexon_core::we_client::request_bytes(token as u64, FIELD, &payload)
        else {
            debug_assert!(false, "поле {FIELD} неизвестно протоколу");
            return;
        };
        self.as_mut().rust_mut().pending = Some(token);
        // The token owner is remembered BEFORE sending: the answer may arrive
        // before we return (the same as in `impl_query_base`).
        let raw: *mut Self = unsafe { Pin::get_unchecked_mut(self.as_mut()) };
        unsafe { crate::we_transport::remember(token, raw.cast::<std::ffi::c_void>()) };
        crate::we_transport::send(token, FIELD, &bytes);
    }
}

/// Subscribers to label catalogue changes.
///
/// Why this exists. Plugin catalogues are loaded LAZILY: the very first
/// question "what is this called in Russian" only sends a request to the
/// daemon and returns the fallback text — the `msgid` itself, that is, an
/// English word. The answer arrives a fraction of a second later, and
/// `revisionChanged` makes the QML bindings that call `I18n.tr()`
/// recompute.
///
/// But a label computed in Rust and handed out as a model ROLE will not be
/// recomputed by any binding: Qt read the role once, when it created the
/// delegate, and will not ask again until the model says `dataChanged`.
/// Such a model must subscribe here and send `dataChanged` itself.
///
/// The registry is shared (rule #12) and built exactly like the skin
/// registry (`crate::skin`): the next model with labels from Rust needs only
/// `watch` in `initialize` and `unwatch` in `Drop`.
pub mod subscribers {
    use core::pin::Pin;
    use std::sync::{Mutex, OnceLock};

    /// A push to one subscriber. `Err(())` means the object died and the entry
    /// should be removed.
    type Notify = Box<dyn Fn() -> Result<(), ()> + Send>;

    #[derive(Default)]
    struct Registry {
        next_id: u64,
        entries: Vec<(u64, Notify)>,
    }

    fn registry() -> &'static Mutex<Registry> {
        static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
        REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
    }

    /// Subscribe a live object to catalogue arrivals. Returns the entry number —
    /// the subscriber must remove it in `Drop`.
    ///
    /// Delivery goes through `CxxQtThread::queue`: on a dead object it returns
    /// `ObjectDestroyed` rather than crashing, so there is neither `unwrap()`
    /// nor a preceding liveness check here — between the check and the queueing
    /// the object may die.
    pub fn watch<T>(handle: cxx_qt::CxxQtThread<T>, notify: fn(Pin<&mut T>)) -> u64
    where
        T: cxx_qt::Threading + 'static,
    {
        let mut reg = registry().lock().expect("реестр подписей не отравлен");
        reg.next_id += 1;
        let id = reg.next_id;
        reg.entries.push((
            id,
            Box::new(move || match handle.queue(notify) {
                Ok(()) => Ok(()),
                Err(cxx_qt::ThreadingQueueError::ObjectDestroyed) => Err(()),
                Err(_) => Ok(()),
            }),
        ));
        id
    }

    /// Remove a subscription. Idempotent: `0` means the object never
    /// subscribed.
    pub fn unwatch(id: u64) {
        if id == 0 {
            return;
        }
        let mut reg = registry().lock().expect("реестр подписей не отравлен");
        reg.entries.retain(|(entry, _)| *entry != id);
    }

    /// How many objects are subscribed right now (for checks).
    pub fn watching() -> usize {
        registry()
            .lock()
            .expect("реестр подписей не отравлен")
            .entries
            .len()
    }

    /// Push all live subscribers: the catalogues changed, recompute the labels.
    /// Returns the number notified; dead entries are removed along the way.
    pub fn dispatch() -> usize {
        let mut told = 0usize;
        let mut reg = registry().lock().expect("реестр подписей не отравлен");
        reg.entries.retain(|(_, notify)| match notify() {
            Ok(()) => {
                told += 1;
                true
            }
            Err(()) => false,
        });
        told
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn карта_из_qml_разбирается_в_пару() {
        let v = json!({"pluginId": "org.waywallen.wallhaven", "msgid": "Trending"});
        assert_eq!(
            message_parts(&v),
            (
                "org.waywallen.wallhaven".to_string(),
                "Trending".to_string(),
                String::new(),
                Vec::new()
            )
        );
    }

    #[test]
    fn змеиная_раскладка_ключей_тоже_понимается() {
        let v = json!({"plugin_id": "org.waywallen.image", "msgid": "Frame rate"});
        assert_eq!(
            message_parts(&v),
            (
                "org.waywallen.image".to_string(),
                "Frame rate".to_string(),
                String::new(),
                Vec::new()
            )
        );
    }

    #[test]
    fn голая_строка_идёт_как_msgid_без_плагина() {
        assert_eq!(
            message_parts(&json!("Not signed in")),
            (String::new(), "Not signed in".to_string(), String::new(), Vec::new())
        );
    }

    /// An empty value must not turn into a label: the vendored QML has its own
    /// fallback text for that case.
    #[test]
    fn пустое_остаётся_пустым() {
        assert_eq!(
            message_parts(&Value::Null),
            (String::new(), String::new(), String::new(), Vec::new())
        );
        assert_eq!(
            message_parts(&json!({"pluginId": "x"})),
            ("x".to_string(), String::new(), String::new(), Vec::new())
        );
    }

    /// waywallen 0.3.8: substitutions travel as a separate list and are
    /// substituted AFTER translation — otherwise the catalogue would be searched
    /// for the glued string.
    #[test]
    fn подстановки_читаются_и_ставятся_после_перевода() {
        let v = json!({
            "pluginId": "steam",
            "msgid": "Signed in as %1",
            "arguments": ["Wehrwolfmann"]
        });
        assert_eq!(
            message_parts(&v),
            (
                "steam".to_string(),
                "Signed in as %1".to_string(),
                String::new(),
                vec!["Wehrwolfmann".to_string()]
            )
        );
        // The substitution itself lives in the core — here we check that the
        // bridge passes it exactly what arrived.
        assert_eq!(
            rexon_core::plugin_l10n::format_message(
                "Вход выполнен как %1",
                &["Wehrwolfmann".to_string()]
            ),
            "Вход выполнен как Wehrwolfmann"
        );
        // A percent sign without a digit is not a substitution.
        assert_eq!(
            rexon_core::plugin_l10n::format_message("Готово на 100%", &["x".to_string()]),
            "Готово на 100%"
        );
        // A number out of range stays as it was.
        assert_eq!(
            rexon_core::plugin_l10n::format_message("%2 и %1", &["один".to_string()]),
            "%2 и один"
        );
    }

    /// Our keyed translation arrives in the map as a separate field.
    #[test]
    fn ключевой_перевод_читается_из_карты() {
        let v = json!({
            "pluginId": "org.waywallen.open-wallpaper-engine",
            "msgid": "Trending today",
            "rexonFallback": "Популярные (Сегодня)"
        });
        assert_eq!(
            message_parts(&v),
            (
                "org.waywallen.open-wallpaper-engine".to_string(),
                "Trending today".to_string(),
                "Популярные (Сегодня)".to_string(),
                Vec::new()
            )
        );
    }

    #[test]
    fn локаль_из_окружения_не_пустая() {
        // "C" and an empty variable must not become a language tag.
        assert!(!env_locale().is_empty());
    }
}
