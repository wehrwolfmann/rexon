// The shared part of the selection storages of the "Wallpapers" page: the
// `SelectStorage` base (qextra) plus the vendor's `WallpaperSelectStorage`
// layer.
//
// This is the same replacement for Qt inheritance as `query_base.rs` for
// queries: cxx-qt cannot make a Rust QObject inherit from a Rust QObject,
// while the vendor has TWO storages and the second inherits from the first
// — `PlaylistItemSelectStorage : WallpaperSelectStorage : SelectStorage`
// (`ui/src/model/wallpaper_select_storage.cppm` v0.3.7). The macro emits
// what the original got from two base classes, and the differences stay in
// the bridge files.
//
// The module is NOT a bridge: a `macro_rules!` in a file with
// `#[cxx_qt::bridge]` would live together with it, while both bridges need
// it at once. One engine per crate, not a copy in each (rule #12).
//
// What the bridge type must provide by itself:
//
//   * a Rust struct with the fields `state: SelectState`,
//     `model: *mut qobject::QObject` and
//     `model_watch: Vec<cxx_qt::QMetaObjectConnectionGuard>` (model
//     subscriptions, waywallen 0.4.4);
//   * in the bridge — the properties `model`/`selectionMode`/`anchorIndex`/
//     `selectedCount`/`active`/`removableSelectedCount`/
//     `hiddenSelectedCount` with their signals;
//   * `fn keep_active(&self) -> bool` — the original's
//     `keepActiveWithoutSelection`. It is virtual in C++, so here too each
//     has its own: for the wallpaper layer it is "a playlist is being
//     edited", for the contents layer — "a playlist is set";
//   * `fn after_selected_count_changed(self: Pin<&mut Self>)` — what in the
//     original hangs on `selectedCountChanged` through the subclass's own
//     connect (for the playlist contents this is recomputing the order of
//     the added items);
//   * `fn clear(...)` — the `clear()` invokable. It is virtual in the
//     original: the subclass does its own thing and calls `clear_base()`
//     from here.

/// Qt layer of the `SelectStorage` + `WallpaperSelectStorage` base.
///
/// `$ty` is the type name in the bridge, `$cast` is the name of the C++
/// `qobject_cast` shim to the library model (each bridge has its own: the
/// same `extern "C++"` declaration in two bridges gives a `duplicate symbol`
/// at link time).
#[macro_export]
macro_rules! impl_select_storage_base {
    ($ty:ident, $cast:ident) => {
        impl qobject::$ty {
            // ── model access ──────────────────────────────────────────────

            /// The model as a Rust type. `None` means no model is set.
            ///
            /// Warning: the cast was already checked with `qobject_cast` when the
            /// property was written, so only a pointer cast remains here.
            fn list_model(
                &self,
            ) -> ::core::option::Option<
                &$crate::wallpaper_list_model::qobject::WallpaperListModel,
            > {
                if self.model.is_null() {
                    return ::core::option::Option::None;
                }
                ::core::option::Option::Some(unsafe {
                    &*self
                        .model
                        .cast::<$crate::wallpaper_list_model::qobject::WallpaperListModel>()
                })
            }

            /// The model as a mutable reference. A separate method rather than through
            /// `self`: the model pointer is NOT derived from `self`, so while the model
            /// is being edited, the storage can still be read and send its own signals.
            ///
            /// # Safety
            /// `self.model` is either nullptr or a live `WallpaperListModel` (the cast
            /// was checked with `qobject_cast` when the property was written).
            unsafe fn list_model_pin<'a>(
                &self,
            ) -> ::core::option::Option<
                ::core::pin::Pin<
                    &'a mut $crate::wallpaper_list_model::qobject::WallpaperListModel,
                >,
            > {
                if self.model.is_null() {
                    return ::core::option::Option::None;
                }
                ::core::option::Option::Some(unsafe {
                    ::core::pin::Pin::new_unchecked(
                        &mut *self
                            .model
                            .cast::<$crate::wallpaper_list_model::qobject::WallpaperListModel>(),
                    )
                })
            }

            pub fn model(&self) -> *mut qobject::QObject {
                self.model
            }

            /// Writing the `model` property. QML calls it — a raw `QObject*` comes from
            /// there, and there is no way to check it on the Rust side.
            ///
            /// # Safety
            /// `model` is either nullptr or a live QObject: the `qobject_cast` shim
            /// reads the metaobject at this address.
            pub unsafe fn set_model(
                mut self: ::core::pin::Pin<&mut Self>,
                model: *mut qobject::QObject,
            ) {
                // A foreign model is not accepted — like the original's qobject_cast.
                let next = unsafe { qobject::$cast(model) };
                if self.model == next {
                    return;
                }
                ::cxx_qt::CxxQtType::rust_mut(self.as_mut()).model = next;
                self.as_mut().watch_model();
                self.as_mut().model_changed();
                self.as_mut().notify_selected_count();
                self.as_mut().cancel_if_empty();
                self.notify_active_changed();
            }

            pub fn selected_count(&self) -> i32 {
                self.list_model()
                    .map(|m| m.selected_count_value())
                    .unwrap_or(0)
            }

            pub fn removable_selected_count(&self) -> i32 {
                self.list_model()
                    .map(|m| m.removable_selected_keys_vec().len() as i32)
                    .unwrap_or(0)
            }

            /// waywallen 0.4.4: how many of the selected wallpapers are hidden.
            pub fn hidden_selected_count(&self) -> i32 {
                self.list_model()
                    .map(|m| m.hidden_selected_count_value())
                    .unwrap_or(0)
            }

            /// Model subscriptions — a port of the `WallpaperSelectStorage` v0.4.4
            /// constructor lambda: when the model changes, the old connection is
            /// broken, and `dataChanged` → `selectedHiddenCountChanged` is hooked to the
            /// new one.
            ///
            /// Beyond the original — the model's `selectionChanged` → a full recompute
            /// (`onModelSelectionChanged`). In the original the "hidden" flag changes by
            /// editing a row in place (`dataChanged`), while our model is rebuilt
            /// entirely (`reset_rows`), and there is no `dataChanged` then; it reports
            /// the preserved selection precisely with `selectionChanged`. Without this
            /// connection the selection counters would freeze after the list is
            /// rebuilt.
            ///
            /// Warning: the `dataChanged` handler ONLY sends a signal and does not touch
            /// the state: it is raised by the model itself inside `set_selected`, which
            /// is called by this same storage. The model sends `selectionChanged` only
            /// from `reset_rows` (called by the list query, not by the storage); a full
            /// recompute is safe there.
            fn watch_model(mut self: ::core::pin::Pin<&mut Self>) {
                ::cxx_qt::CxxQtType::rust_mut(self.as_mut()).model_watch.clear();
                /// Storage address for the handlers. The connection lives no longer than
                /// the storage: the guards lie in its own field and break it on Drop; the
                /// connection is direct, with no deferred calls.
                struct Owner(*mut qobject::$ty);
                // SAFETY: the connections are direct (`AutoConnection` in the GUI thread);
                // the handler is not called from another thread.
                unsafe impl Send for Owner {}
                let raw: *mut qobject::$ty = unsafe { self.as_mut().get_unchecked_mut() };
                let ::core::option::Option::Some(mut model) = (unsafe { self.list_model_pin() })
                else {
                    return;
                };
                let owner = Owner(raw);
                let on_data = model.as_mut().on_data_changed(move |_, _, _, _| {
                    let owner = &owner;
                    let storage = unsafe { ::core::pin::Pin::new_unchecked(&mut *owner.0) };
                    storage.selected_hidden_count_changed();
                });
                let owner = Owner(raw);
                let on_selection = model.on_selection_changed(move |_| {
                    let owner = &owner;
                    let storage = unsafe { ::core::pin::Pin::new_unchecked(&mut *owner.0) };
                    storage.on_model_selection_changed();
                });
                let mut rust = ::cxx_qt::CxxQtType::rust_mut(self.as_mut());
                rust.model_watch.push(on_data);
                rust.model_watch.push(on_selection);
            }

            // ── base properties ───────────────────────────────────────────

            pub fn selection_mode(&self) -> bool {
                self.state.selection_mode
            }

            pub fn set_selection_mode(mut self: ::core::pin::Pin<&mut Self>, value: bool) {
                if self.state.selection_mode == value {
                    return;
                }
                ::cxx_qt::CxxQtType::rust_mut(self.as_mut()).state.selection_mode = value;
                self.as_mut().selection_mode_changed();
                self.notify_active_changed();
            }

            pub fn anchor_index(&self) -> i32 {
                self.state.anchor_index
            }

            pub fn set_anchor_index(mut self: ::core::pin::Pin<&mut Self>, value: i32) {
                if self.state.anchor_index == value {
                    return;
                }
                ::cxx_qt::CxxQtType::rust_mut(self.as_mut()).state.anchor_index = value;
                self.anchor_index_changed();
            }

            pub fn active(&self) -> bool {
                self.state.selection_mode || self.selected_count() > 0 || self.keep_active()
            }

            // ── invokables ────────────────────────────────────────────────

            pub fn begin(mut self: ::core::pin::Pin<&mut Self>, row: i32) -> bool {
                if self.model.is_null() {
                    self.as_mut().set_selection_mode(false);
                    self.set_anchor_index(-1);
                    return false;
                }
                if row >= 0 {
                    if let ::core::option::Option::Some(model) =
                        (unsafe { self.list_model_pin() })
                    {
                        model.set_selected(row, true);
                    }
                    self.as_mut().notify_selected_count();
                    self.as_mut().set_anchor_index(row);
                }
                if self.selected_count() == 0 && !self.keep_active() {
                    self.as_mut().set_selection_mode(false);
                    self.set_anchor_index(-1);
                    return false;
                }
                self.set_selection_mode(true);
                true
            }

            pub fn set_selected(
                mut self: ::core::pin::Pin<&mut Self>,
                row: i32,
                selected: bool,
            ) -> bool {
                let ::core::option::Option::Some(model) = (unsafe { self.list_model_pin() })
                else {
                    return false;
                };
                let changed = model.set_selected(row, selected);
                self.as_mut().notify_selected_count();
                self.cancel_if_empty();
                changed
            }

            pub fn toggle_selected(mut self: ::core::pin::Pin<&mut Self>, row: i32) -> bool {
                let ::core::option::Option::Some(model) = (unsafe { self.list_model_pin() })
                else {
                    return false;
                };
                let changed = model.toggle_selected(row);
                self.as_mut().notify_selected_count();
                let has_selection = self.selected_count() > 0;
                let keep = self.keep_active();
                self.as_mut().set_selection_mode(has_selection || keep);
                self.set_anchor_index(if has_selection { row } else { -1 });
                changed
            }

            pub fn select_range(
                mut self: ::core::pin::Pin<&mut Self>,
                from: i32,
                to: i32,
                selected: bool,
            ) -> bool {
                let ::core::option::Option::Some(model) = (unsafe { self.list_model_pin() })
                else {
                    return false;
                };
                let changed = model.select_range(from, to, selected);
                self.as_mut().notify_selected_count();
                if selected && self.selected_count() > 0 {
                    self.set_selection_mode(true);
                } else {
                    self.cancel_if_empty();
                }
                changed
            }

            /// The body of the original's `SelectStorage::clear()`. The `clear()`
            /// invokable is virtual in the original: the subclass does its own thing
            /// and calls this.
            pub fn clear_base(mut self: ::core::pin::Pin<&mut Self>) {
                if let ::core::option::Option::Some(model) = (unsafe { self.list_model_pin() })
                {
                    model.clear_selection();
                }
                self.as_mut().notify_selected_count();
                self.as_mut().set_selection_mode(false);
                self.as_mut().set_anchor_index(-1);
                self.notify_active_changed();
            }

            pub fn set_selected_keys(
                mut self: ::core::pin::Pin<&mut Self>,
                keys: &::cxx_qt_lib::QStringList,
            ) {
                if let ::core::option::Option::Some(model) = (unsafe { self.list_model_pin() })
                {
                    model.set_selected_keys(keys);
                }
                self.as_mut().notify_selected_count();
                self.cancel_if_empty();
            }

            pub fn selected_keys(&self) -> ::cxx_qt_lib::QStringList {
                match self.list_model() {
                    ::core::option::Option::Some(model) => ::cxx_qt_lib::QStringList::from_iter(
                        model.selected_keys_vec().iter().map(::cxx_qt_lib::QString::from),
                    ),
                    ::core::option::Option::None => ::cxx_qt_lib::QStringList::default(),
                }
            }

            pub fn selected_items(&self) -> qobject::QList_QVariant {
                match self.list_model() {
                    ::core::option::Option::Some(model) => {
                        let mut out = qobject::QList_QVariant::default();
                        for item in model.selected_items().iter() {
                            out.append(item.clone());
                        }
                        out
                    }
                    ::core::option::Option::None => qobject::QList_QVariant::default(),
                }
            }

            pub fn selected_wallpaper_ids(&self) -> qobject::QList_QVariant {
                let mut out = qobject::QList_QVariant::default();
                if let ::core::option::Option::Some(model) = self.list_model() {
                    for key in model.selected_keys_vec() {
                        out.append(::cxx_qt_lib::QVariant::from(&::cxx_qt_lib::QString::from(
                            &key,
                        )));
                    }
                }
                out
            }

            pub fn removable_selected_wallpaper_ids(&self) -> ::cxx_qt_lib::QStringList {
                match self.list_model() {
                    ::core::option::Option::Some(model) => ::cxx_qt_lib::QStringList::from_iter(
                        model
                            .removable_selected_keys_vec()
                            .iter()
                            .map(::cxx_qt_lib::QString::from),
                    ),
                    ::core::option::Option::None => ::cxx_qt_lib::QStringList::default(),
                }
            }

            pub fn on_model_selection_changed(mut self: ::core::pin::Pin<&mut Self>) {
                self.as_mut().notify_selected_count();
                self.as_mut().cancel_if_empty();
                self.notify_active_changed();
            }

            // ── internals ─────────────────────────────────────────────────

            /// The selection in the model changed: send both counters and give the
            /// subclass its own `selectedCountChanged` handler (in the original — a
            /// connect in the subclass constructor).
            ///
            /// Signals first, then the handler: in the original it is a slot, and slots
            /// run after the signal has propagated through the QML bindings.
            fn notify_selected_count(mut self: ::core::pin::Pin<&mut Self>) {
                self.as_mut().selected_count_changed();
                self.as_mut().selected_removable_count_changed();
                // waywallen 0.4.4: `selectedCountChanged` and `modelChanged` →
                // `selectedHiddenCountChanged` (the constructor's chains).
                self.as_mut().selected_hidden_count_changed();
                self.after_selected_count_changed();
            }

            /// The original's `cancelIfEmpty`: the selection became empty — leave
            /// selection mode.
            fn cancel_if_empty(mut self: ::core::pin::Pin<&mut Self>) {
                if self.selected_count() > 0 || self.keep_active() {
                    self.notify_active_changed();
                    return;
                }
                self.as_mut().set_selection_mode(false);
                self.as_mut().set_anchor_index(-1);
                self.notify_active_changed();
            }

            /// The original's `notifyActiveChanged`: the signal only on a real flip.
            fn notify_active_changed(mut self: ::core::pin::Pin<&mut Self>) {
                let next = self.active();
                if self.state.active == next {
                    return;
                }
                ::cxx_qt::CxxQtType::rust_mut(self.as_mut()).state.active = next;
                self.active_changed();
            }
        }
    };
}
