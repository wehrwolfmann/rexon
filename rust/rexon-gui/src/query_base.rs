// The part shared by all queries of the Wallpaper Engine tab: the
// properties of the `Query` base, sending a frame through the shared
// transport seam and emitting the base's signals.
//
// This is the replacement for Qt inheritance: cxx-qt cannot make a Rust
// QObject inherit from a Rust QObject, while the whole query hierarchy of
// the tab is inheritance from one base. The macro emits what the original
// got from a shared base class.
//
// The module is NOT a bridge: a `macro_rules!` in a file with
// `#[cxx_qt::bridge]` would live together with it, while all bridges need
// it at once. One engine per crate, not a copy in every file (rule #12).
//
// Pending: `src/wallpaper_actions.rs` and `src/discover_queries.rs` keep
// their own copies of the same scaffolding — they were written earlier,
// before the shared module appeared. Switching them to `impl_query_base!`
// is a separate short edit; it is not done here so as not to overlap with
// parallel work on those files.

/// Properties of the `Query` base, sending a frame and emitting the base's
/// signals.
///
/// Requirements for the type: a Rust struct with a `base: QueryState` field,
/// and a bridge with the signals `statusChanged`/`queryingChanged`/
/// `errorChanged`/`forwardErrorChanged`/`finished`/`errorOccurred`/
/// `appErrorOccurred`/`sendRequest` and an `emit_property(&str)` method.
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

            /// Send the frame out through the shared seam of batches 2-3.
            ///
            /// Returns a token, or `None` if there is no connection: then the core has
            /// already switched the request to Error and there is nothing to send.
            ///
            /// Warning: the token owner is remembered BEFORE sending: the answer may
            /// arrive before we return from the callback.
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
                // The number is issued BEFORE the frame is built and put right into it:
                // it is also the protocol's `request_id`; there is no second counter any
                // more.
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

            /// Emit the base's signals. The properties are read live: an
            /// `onStatusChanged` handler in QML must see the new values.
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

/// The frame could not be parsed — the daemon may be newer than us.
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

/// Registry of requests waiting for the progress of a long daemon task.
///
/// A port of the original's `Notify::taskProgress` → `ProgressQuery` link.
/// For us the event arrives as a RAW frame through the transport seam, so
/// the registry holds not a Qt signal but a closure per request.
///
/// A closure rather than a `CxxQtThread<T>` of a concrete type: DIFFERENT
/// request types wait for progress (update check and installation), and
/// `CxxQtThread` cannot be made generic over the type — cxx-qt emits its
/// own alias for every QObject. The same technique as the acquisition
/// state store in `remote_list_model.rs`.
pub mod progress {
    use rexon_core::we_manage::TaskProgressSnapshot;
    use std::sync::{Mutex, OnceLock};

    /// Delivering a snapshot to one request. `Err(())` means the object died
    /// and the entry should be removed.
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

    /// Register a recipient. Returns the entry number for removal.
    pub fn register(deliver: Deliver) -> u64 {
        let mut reg = registry().lock().expect("реестр прогресса не отравлен");
        reg.next_id += 1;
        let id = reg.next_id;
        reg.entries.push((id, deliver));
        id
    }

    /// Remove an entry. Idempotent.
    pub fn unregister(id: u64) {
        let mut reg = registry().lock().expect("реестр прогресса не отравлен");
        reg.entries.retain(|(entry, _)| *entry != id);
    }

    /// How many recipients are in the registry right now (for checks).
    pub fn registered() -> usize {
        registry()
            .lock()
            .expect("реестр прогресса не отравлен")
            .entries
            .len()
    }

    /// Send the snapshot to all live recipients. Returns their number.
    pub fn dispatch(query_id: &str, snapshot: &TaskProgressSnapshot) -> usize {
        let mut delivered = 0usize;
        let mut reg = registry().lock().expect("реестр прогресса не отравлен");
        reg.entries.retain(|(_, deliver)| match deliver(query_id, snapshot) {
            Ok(()) => {
                delivered += 1;
                true
            }
            // The object died — the entry is no longer needed.
            Err(()) => false,
        });
        delivered
    }
}
