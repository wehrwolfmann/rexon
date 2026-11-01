// Общая часть всех запросов вкладки Wallpaper Engine: свойства базы
// `Query`, отправка кадра общим транспортным швом и рассылка сигналов базы.
//
// Это и есть замена Qt-наследования: cxx-qt не умеет
// наследование Rust-QObject от Rust-QObject, а вся иерархия запросов вкладки
// — наследование от одной базы. Макрос печатает то, что у оригинала давал
// общий базовый класс.
//
// Модуль НЕ мост: `macro_rules!` в файле с `#[cxx_qt::bridge]` жил бы вместе
// с ним, а нужен он всем мостам сразу. Один движок на крейт, а не копия в
// каждом файле (правило #12).
//
// ⏳ `src/wallpaper_actions.rs` и `src/discover_queries.rs` держат свои копии
// этой же заготовки — они писались раньше, до появления общего модуля.
// Переключить их на `impl_query_base!` — отдельная короткая правка; здесь
// она не делается, чтобы не пересекаться с параллельной работой по тем
// файлам.

/// Свойства базы `Query`, отправка кадра и рассылка сигналов базы.
///
/// Требования к типу: Rust-структура с полем `base: QueryState`, а мост —
/// с сигналами `statusChanged`/`queryingChanged`/`errorChanged`/
/// `forwardErrorChanged`/`finished`/`errorOccurred`/`appErrorOccurred`/
/// `sendRequest` и методом `emit_property(&str)`.
#[macro_export]
macro_rules! impl_query_base {
    ($query:ident) => {
        impl qobject::$query {
            pub fn status(&self) -> i32 {
                self.base.status
            }

            pub fn error(&self) -> ::cxx_qt_lib::QString {
                ::cxx_qt_lib::QString::from(&self.base.error)
            }

            pub fn querying(&self) -> bool {
                self.base.status == ::rexon_core::we_query::STATUS_QUERYING
            }

            pub fn forward_error(&self) -> bool {
                self.base.forward_error
            }

            pub fn set_forward_error(
                mut self: ::core::pin::Pin<&mut Self>,
                value: bool,
            ) {
                let signals = ::rexon_core::we_query::set_forward_error(
                    &mut ::cxx_qt::CxxQtType::rust_mut(self.as_mut()).base,
                    value,
                );
                self.dispatch(signals);
            }

            /// Отправить кадр наружу общим швом партий 2-3.
            ///
            /// Возвращает токен или `None`, если соединения нет: тогда ядро
            /// уже перевело запрос в Error и слать нечего.
            ///
            /// ⚠️ Владельца токена запоминаем ДО отправки: ответ может
            /// прийти раньше, чем мы вернёмся из колбэка.
            pub fn send_frame(
                mut self: ::core::pin::Pin<&mut Self>,
                field: &'static str,
                payload: ::std::vec::Vec<u8>,
            ) -> ::core::option::Option<i64> {
                let connected = $crate::discover_queries::connected();
                let (send, signals) = ::rexon_core::we_query::begin_call(
                    &mut ::cxx_qt::CxxQtType::rust_mut(self.as_mut()).base,
                    connected,
                );
                self.as_mut().dispatch(signals);
                if !send {
                    return ::core::option::Option::None;
                }
                // Номер выдаётся ДО сборки кадра и кладётся прямо в него:
                // он же `request_id` протокола, второго счётчика больше нет.
                let token = $crate::discover_queries::next_token();
                let ::core::result::Result::Ok(bytes) =
                    ::rexon_core::we_client::request_bytes(token as u64, field, &payload)
                else {
                    debug_assert!(false, "поле {field} неизвестно протоколу");
                    return ::core::option::Option::None;
                };
                let raw: *mut Self =
                    unsafe { ::core::pin::Pin::get_unchecked_mut(self.as_mut()) };
                unsafe {
                    $crate::we_transport::remember(
                        token,
                        raw.cast::<::std::ffi::c_void>(),
                    )
                };
                self.as_mut().send_request(
                    token,
                    ::cxx_qt_lib::QString::from(field),
                    ::cxx_qt_lib::QByteArray::from(&bytes[..]),
                );
                $crate::we_transport::send(token, field, &bytes);
                ::core::option::Option::Some(token)
            }

            /// Разослать сигналы базы. Свойства читаются живые: обработчик
            /// `onStatusChanged` в QML обязан увидеть новые значения.
            pub fn dispatch_base(
                mut self: ::core::pin::Pin<&mut Self>,
                signals: ::std::vec::Vec<::rexon_core::we_query::QuerySignal>,
            ) {
                use ::rexon_core::we_query::QuerySignal;
                for signal in signals {
                    match signal {
                        QuerySignal::StatusChanged(v) => self.as_mut().status_changed(v),
                        QuerySignal::QueryingChanged => self.as_mut().querying_changed(),
                        QuerySignal::ErrorChanged => self.as_mut().error_changed(),
                        QuerySignal::ForwardErrorChanged => {
                            self.as_mut().forward_error_changed()
                        }
                        QuerySignal::Finished => self.as_mut().finished(),
                        QuerySignal::ErrorOccurred(text) => {
                            self.as_mut()
                                .error_occurred(::cxx_qt_lib::QString::from(&text));
                        }
                        QuerySignal::AppErrorOccurred(text) => {
                            self.as_mut()
                                .app_error_occurred(::cxx_qt_lib::QString::from(&text));
                        }
                        QuerySignal::Property(name) => self.as_mut().emit_property(name),
                    }
                }
            }
        }
    };
}

/// Разобрать кадр не удалось — демон может быть новее нас.
pub fn decode_failed(
    st: &mut rexon_core::we_query::QueryState,
) -> Vec<rexon_core::we_query::QuerySignal> {
    let mut out = rexon_core::we_query::set_error(st, &rexon_core::i18n::tr("could not parse the daemon response"));
    out.extend(rexon_core::we_query::set_status(
        st,
        rexon_core::we_query::STATUS_ERROR,
    ));
    out
}

/// Реестр запросов, ждущих прогресса длинной задачи демона.
///
/// Порт связки `Notify::taskProgress` → `ProgressQuery` оригинала. У нас
/// событие приходит СЫРЫМ кадром через транспортный шов, поэтому реестр
/// держит не сигнал Qt, а по замыканию на запрос.
///
/// Замыкание, а не `CxxQtThread<T>` конкретного типа: прогресса ждут
/// РАЗНЫЕ типы запросов (проверка обновлений и установка), а обобщить
/// `CxxQtThread` по типу нельзя — cxx-qt печатает свой алиас на каждый
/// QObject. Тот же приём, что у склада состояний
/// приобретения в `remote_list_model.rs`.
pub mod progress {
    use rexon_core::we_manage::TaskProgressSnapshot;
    use std::sync::{Mutex, OnceLock};

    /// Доставка снимка одному запросу. `Err(())` — объект умер, запись
    /// пора снять.
    type Deliver = Box<dyn Fn(&str, &TaskProgressSnapshot) -> Result<(), ()> + Send>;

    #[derive(Default)]
    struct Registry {
        next_id: u64,
        entries: Vec<(u64, Deliver)>,
    }

    fn registry() -> &'static Mutex<Registry> {
        static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
        REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
    }

    /// Зарегистрировать получателя. Возвращает номер записи для снятия.
    pub fn register(deliver: Deliver) -> u64 {
        let mut reg = registry().lock().expect("реестр прогресса не отравлен");
        reg.next_id += 1;
        let id = reg.next_id;
        reg.entries.push((id, deliver));
        id
    }

    /// Снять запись. Идемпотентно.
    pub fn unregister(id: u64) {
        let mut reg = registry().lock().expect("реестр прогресса не отравлен");
        reg.entries.retain(|(entry, _)| *entry != id);
    }

    /// Сколько получателей сейчас в реестре (для проверок).
    pub fn registered() -> usize {
        registry()
            .lock()
            .expect("реестр прогресса не отравлен")
            .entries
            .len()
    }

    /// Разослать снимок всем живым получателям. Возвращает их число.
    pub fn dispatch(query_id: &str, snapshot: &TaskProgressSnapshot) -> usize {
        let mut delivered = 0usize;
        let mut reg = registry().lock().expect("реестр прогресса не отравлен");
        reg.entries.retain(|(_, deliver)| match deliver(query_id, snapshot) {
            Ok(()) => {
                delivered += 1;
                true
            }
            // Объект умер — запись больше не нужна.
            Err(()) => false,
        });
        delivered
    }
}
