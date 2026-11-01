// PluginTranslations — QML-синглтон подписей плагинов демона waywallen.
//
// Появился в waywallen 0.3.7 вместе с локализуемыми подписями: плагин
// отдаёт подпись парой `PluginMessage{plugin_id, msgid}`, а перевод демон
// присылает каталогами `.po` на запрос `plugin_translation_list`. Вендорный
// `I18n.qml` (0.3.7) состоит ровно из обращения к этому типу, и без него не
// компилируется ни он, ни семь страниц, которые его зовут.
//
// ⭐ У НАС он не замена нашему русскому слою, а его продолжение. Порядок
// разрешения подписи — в ядре (`rexon_core::plugin_l10n`, там же его
// обоснование и тесты):
//
//   1. каталог самого плагина (перевод его автора);
//   2. наш `remote_l10n`;
//   3. сам `msgid`.
//
// Замер на живом демоне 0.3.7: русский каталог есть у `wallhaven`, `image`,
// `video`, а у `org.waywallen.open-wallpaper-engine` (Мастерская Steam) —
// только zh-CN. То есть без пункта 2 весь русский Мастерской пропал бы, а
// без пункта 1 мы молча перебивали бы перевод автора плагина своим.
//
// Логики здесь нет (правило крейта): разбор `.po`, метки локали и сам
// порядок — в ядре. Мосту остаётся оболочка QObject, запрос к демону и
// приём ответа.

/// Мост cxx-qt: синглтон `PluginTranslations`.
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
        /// Счётчик обновлений каталогов. Вендорный `I18n.tr()` читает его
        /// ПЕРВОЙ строкой именно ради зависимости привязки: пришёл каталог
        /// — все подписи пересчитались сами.
        /// ⚠️ `f64`, а не `i64`: 64-битное целое наружу в QML не выпускаем
        /// (разбор — `src/qvalue.rs`). Счётчик внутри остаётся `i64`.
        #[qproperty(f64, revision, READ = revision, NOTIFY = revision_changed)]
        /// Язык интерфейса (`ru`, `ru-RU`, `zh-CN`). По умолчанию берётся
        /// из окружения.
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

        /// Подпись для покупателя по `PluginMessage` из протокола.
        ///
        /// Принимает то, что кладёт в модель наш мост: карту
        /// `{pluginId, msgid}` (или `plugin_id`/`msg_id` — на случай иной
        /// раскладки ключей), голую строку и пустое значение.
        #[qinvokable]
        fn translate(self: Pin<&mut PluginTranslations>, value: &QVariant) -> QString;

        /// Запросить каталоги у демона.
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

/// Состояние синглтона.
pub struct PluginTranslationsRust {
    /// Разобранные каталоги: `plugin_id` → `locale` → сообщения.
    catalogs: Catalogs,
    /// Поколение каталогов у демона (его же `generation`).
    generation: u64,
    /// Счётчик обновлений для привязок QML.
    revision: i64,
    /// Язык интерфейса.
    locale: String,
    /// Запрос уже отправлен на нынешнем соединении.
    requested: bool,
    /// Токен отправленного запроса (чужие ответы игнорируются).
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

/// Язык интерфейса из окружения: `LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`.
///
/// Своей копии цикла здесь больше нет: та же локаль нужна и человеческим
/// именам тем в сканере установленного, а две копии одного чтения окружения
/// расходятся молча — движок один, в ядре.
fn env_locale() -> String {
    rexon_core::utils::env_locale()
}

/// Разобрать значение из QML в тройку `(plugin_id, msgid, наш перевод)`.
///
/// `rexonFallback` — НАШЕ поле в той же карте: подпись, посчитанная по
/// ключу (`rexon_core::remote_l10n::translate_source`). Ключа сортировки
/// или фильтра в `PluginMessage` нет вовсе, а наши таблицы для них
/// ключевые — без этого поля русский пропал бы у всех сортировок и
/// заголовков фильтров разом.
///
/// Голая строка — это `msgid` без плагина: каталога у неё нет, и подпись
/// пойдёт сразу нашим слоем. Так же ведёт себя и `null`/пустое.
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
            // waywallen 0.3.8: подпись может нести подстановки `%1`, `%2` —
            // их подставляют ПОСЛЕ перевода, чтобы в каталоге искался
            // шаблон, а не склеенная строка.
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
        // Каталоги уже на руках, менять их не нужно — но подписи выбираются
        // по языку, значит привязки обязаны пересчитаться.
        self.bump_revision();
    }

    /// Подпись по `PluginMessage`.
    ///
    /// ⚠️ Зовётся на КАЖДУЮ подпись страницы, поэтому здесь только поиск по
    /// карте: разбор `.po` сделан один раз, при приёме ответа.
    pub fn translate(mut self: Pin<&mut Self>, value: &QVariant) -> QString {
        // Дешёвый сторож ленивой загрузки: пока соединения нет — ждём, а на
        // новом соединении просим каталоги заново. Проверка стоит одного
        // чтения атомика, запрос уходит один раз за соединение.
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

        // ⚠️ Транспорт отдаёт СЫРОЙ кадр сервера (`ServerFrame`), а не
        // `Response` внутри него — как и всем запросам вкладки.
        //
        // Демон постарше запроса не знает — ответ придёт ошибкой или не
        // разберётся вовсе. Это НЕ авария: вкладка обязана подняться, а
        // подписи возьмёт наш слой (пункты 2-3 порядка).
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
            // Каталоги те же — привязки дёргать незачем.
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

    /// Поднять счётчик обновлений и сказать об этом QML.
    ///
    /// ⚠️ Одного `revisionChanged` мало. Привязка вендорного `I18n.tr()`
    /// читает `revision` первой строкой и пересчитывается сама, а вот те,
    /// кто переводит подпись НЕ в QML, а у себя (роли моделей), про смену
    /// каталогов узнать неоткуда: их значение Qt закэшировал в делегате.
    /// Поэтому здесь же будятся подписчики — см. `subscribers`.
    fn bump_revision(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().revision += 1;
        self.revision_changed();
        subscribers::dispatch();
    }

    /// Спросить каталоги, если соединение есть, а запроса ещё не было.
    fn ensure_requested(mut self: Pin<&mut Self>) {
        if !crate::discover_queries::connected() {
            // Соединение пропало — следующее подключение начнёт заново:
            // демон мог смениться вместе с набором плагинов. Пишем только
            // когда есть что менять: сюда заходят на КАЖДУЮ подпись.
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

    /// Кадр `plugin_translation_list` в общий транспорт вкладки.
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
        // Владельца токена запоминаем ДО отправки: ответ может прийти
        // раньше, чем мы вернёмся (так же, как в `impl_query_base`).
        let raw: *mut Self = unsafe { Pin::get_unchecked_mut(self.as_mut()) };
        unsafe { crate::we_transport::remember(token, raw.cast::<std::ffi::c_void>()) };
        crate::we_transport::send(token, FIELD, &bytes);
    }
}

/// Подписчики на смену каталогов подписей.
///
/// Зачем это есть. Каталоги плагинов грузятся ЛЕНИВО: первый же вопрос
/// «как это называется по-русски» только отправляет запрос демону и
/// возвращает запасной текст — сам `msgid`, то есть английское слово.
/// Ответ приезжает спустя доли секунды, и `revisionChanged` заставляет
/// пересчитаться привязки QML, которые зовут `I18n.tr()`.
///
/// Но подпись, посчитанную в Rust и отданную наружу РОЛЬЮ модели, никакая
/// привязка не пересчитает: Qt прочитал роль один раз, когда создавал
/// делегат, и больше не спросит, пока модель не скажет `dataChanged`.
/// Такая модель обязана подписаться здесь и разослать `dataChanged` сама.
///
/// Реестр общий (правило #12) и устроен ровно как реестр скина
/// (`crate::skin`): следующей модели с подписями из Rust хватит `watch` в
/// `initialize` и `unwatch` в `Drop`.
pub mod subscribers {
    use core::pin::Pin;
    use std::sync::{Mutex, OnceLock};

    /// Толчок одному подписчику. `Err(())` — объект умер, запись пора снять.
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

    /// Подписать живой объект на приход каталогов. Возвращает номер записи —
    /// его подписчик обязан снять в `Drop`.
    ///
    /// Доставка идёт через `CxxQtThread::queue`: на умершем объекте она
    /// возвращает `ObjectDestroyed`, а не падает, поэтому ни `unwrap()`, ни
    /// предварительной проверки живости здесь нет — между проверкой и
    /// постановкой объект успевает умереть.
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

    /// Снять подписку. Идемпотентно: `0` — объект не подписывался.
    pub fn unwatch(id: u64) {
        if id == 0 {
            return;
        }
        let mut reg = registry().lock().expect("реестр подписей не отравлен");
        reg.entries.retain(|(entry, _)| *entry != id);
    }

    /// Сколько объектов сейчас подписано (для проверок).
    pub fn watching() -> usize {
        registry()
            .lock()
            .expect("реестр подписей не отравлен")
            .entries
            .len()
    }

    /// Толкнуть всех живых подписчиков: каталоги сменились, подписи
    /// пересчитать. Возвращает число оповещённых; мёртвые записи снимаются
    /// заодно.
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

    /// Пустое значение не должно превращаться в подпись: у вендорного QML
    /// на этот случай свой запасной текст.
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

    /// waywallen 0.3.8: подстановки едут отдельным списком и подставляются
    /// уже ПОСЛЕ перевода — иначе в каталоге искалась бы склеенная строка.
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
        // Сама подстановка живёт в ядре — здесь проверяется, что мост
        // отдаёт ей ровно то, что пришло.
        assert_eq!(
            rexon_core::plugin_l10n::format_message(
                "Вход выполнен как %1",
                &["Wehrwolfmann".to_string()]
            ),
            "Вход выполнен как Wehrwolfmann"
        );
        // Процент без цифры — не подстановка.
        assert_eq!(
            rexon_core::plugin_l10n::format_message("Готово на 100%", &["x".to_string()]),
            "Готово на 100%"
        );
        // Номер вне диапазона остаётся как был.
        assert_eq!(
            rexon_core::plugin_l10n::format_message("%2 и %1", &["один".to_string()]),
            "%2 и один"
        );
    }

    /// Наш ключевой перевод приезжает в карте отдельным полем.
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
        // «C» и пустая переменная не должны становиться меткой языка.
        assert!(!env_locale().is_empty());
    }
}
