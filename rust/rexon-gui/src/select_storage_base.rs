// Общая часть хранилищ выбора страницы «Обои»: база `SelectStorage`
// (qextra) плюс надстройка `WallpaperSelectStorage` вендора.
//
// Это та же замена Qt-наследования, что `query_base.rs` для запросов:
// cxx-qt не умеет наследование Rust-QObject от Rust-QObject, а у вендора
// хранилищ ДВА и второе наследует первому —
// `PlaylistItemSelectStorage : WallpaperSelectStorage : SelectStorage`
// (`ui/src/model/wallpaper_select_storage.cppm` v0.3.7). Макрос печатает
// то, что у оригинала давали два базовых класса, а различия остаются в
// файлах-мостах.
//
// Модуль НЕ мост: `macro_rules!` в файле с `#[cxx_qt::bridge]` жил бы
// вместе с ним, а нужен он обоим мостам сразу. Один движок на крейт, а не
// копия в каждом (правило #12).
//
// Что тип-мост обязан дать сам:
//
//   * Rust-структуру с полями `state: SelectState`,
//     `model: *mut qobject::QObject` и
//     `model_watch: Vec<cxx_qt::QMetaObjectConnectionGuard>` (подписки на
//     модель, waywallen 0.4.4);
//   * в мосте — свойства `model`/`selectionMode`/`anchorIndex`/
//     `selectedCount`/`active`/`removableSelectedCount`/
//     `hiddenSelectedCount` с их сигналами;
//   * `fn keep_active(&self) -> bool` — `keepActiveWithoutSelection`
//     оригинала. Виртуальная в C++, поэтому и здесь своя у каждого:
//     у надстройки обоей это «правят плейлист», у надстройки состава —
//     «плейлист задан»;
//   * `fn after_selected_count_changed(self: Pin<&mut Self>)` — то, что у
//     оригинала висит на `selectedCountChanged` собственным connect'ом
//     подкласса (у состава плейлиста это пересчёт порядка добавленных);
//   * `fn clear(...)` — инвокабл `clear()`. Он виртуальный у оригинала:
//     подкласс делает своё и зовёт `clear_base()` отсюда.

/// Qt-слой базы `SelectStorage` + `WallpaperSelectStorage`.
///
/// `$ty` — имя типа в мосте, `$cast` — имя C++-шима `qobject_cast` к
/// модели библиотеки (у каждого моста своё: одно и то же объявление
/// `extern "C++"` в двух мостах даёт `duplicate symbol` на линковке).
#[macro_export]
macro_rules! impl_select_storage_base {
    ($ty:ident, $cast:ident) => {
        impl qobject::$ty {
            // ── доступ к модели ───────────────────────────────────────────

            /// Модель как Rust-тип. `None` — модель не задана.
            ///
            /// ⚠️ Приведение уже проверено `qobject_cast` в момент записи
            /// свойства, поэтому здесь остаётся только каст указателя.
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

            /// Модель как изменяемая ссылка. Отдельным методом, а не через
            /// `self`: указатель на модель НЕ выводится из `self`, поэтому
            /// пока модель правится, хранилище можно продолжать читать и
            /// слать его собственные сигналы.
            ///
            /// # Safety
            /// `self.model` — либо nullptr, либо живая `WallpaperListModel`
            /// (приведение проверено `qobject_cast` при записи свойства).
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

            /// Запись свойства `model`. Зовёт её QML — оттуда приходит
            /// сырой `QObject*`, и проверить его на стороне Rust нечем.
            ///
            /// # Safety
            /// `model` — либо nullptr, либо живой QObject: шим
            /// `qobject_cast` читает мета-объект по этому адресу.
            pub unsafe fn set_model(
                mut self: ::core::pin::Pin<&mut Self>,
                model: *mut qobject::QObject,
            ) {
                // Чужая модель не принимается — как qobject_cast оригинала.
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

            /// waywallen 0.4.4: сколько выбранных обоев скрыто.
            pub fn hidden_selected_count(&self) -> i32 {
                self.list_model()
                    .map(|m| m.hidden_selected_count_value())
                    .unwrap_or(0)
            }

            /// Подписки на модель — порт лямбды конструктора
            /// `WallpaperSelectStorage` v0.4.4: при смене модели старая
            /// связь рвётся, к новой цепляется `dataChanged` →
            /// `selectedHiddenCountChanged`.
            ///
            /// Сверх оригинала — `selectionChanged` модели → полный
            /// пересчёт (`onModelSelectionChanged`). У оригинала признак
            /// «скрыта» меняется правкой строки на месте (`dataChanged`), а
            /// наша модель перестраивается целиком (`reset_rows`), и
            /// `dataChanged` при этом не бывает; о сохранившемся выборе
            /// она сообщает именно `selectionChanged`. Без этой связи
            /// счётчики выбора застывали бы после пересборки списка.
            ///
            /// ⚠️ Обработчик `dataChanged` ТОЛЬКО шлёт сигнал и состояния не
            /// трогает: его поднимает сама модель внутри `set_selected`,
            /// который зовёт это же хранилище. `selectionChanged` модель
            /// шлёт лишь из `reset_rows` (его зовёт запрос списка, не
            /// хранилище), там полный пересчёт безопасен.
            fn watch_model(mut self: ::core::pin::Pin<&mut Self>) {
                ::cxx_qt::CxxQtType::rust_mut(self.as_mut()).model_watch.clear();
                /// Адрес хранилища для обработчиков. Связь живёт не дольше
                /// хранилища: сторожа лежат в его же поле и при Drop её
                /// рвут; связь прямая, отложенных вызовов нет.
                struct Owner(*mut qobject::$ty);
                // SAFETY: связи прямые (`AutoConnection` в потоке GUI), из
                // другого потока обработчик не зовётся.
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

            // ── свойства базы ─────────────────────────────────────────────

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

            // ── инвокаблы ─────────────────────────────────────────────────

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

            /// Тело `SelectStorage::clear()` оригинала. Инвокабл `clear()`
            /// у оригинала виртуальный: подкласс делает своё и зовёт это.
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

            // ── внутреннее ────────────────────────────────────────────────

            /// Выбор в модели поменялся: разослать оба счётчика и дать
            /// подклассу его собственный обработчик
            /// `selectedCountChanged` (у оригинала — connect в
            /// конструкторе подкласса).
            ///
            /// Сначала сигналы, потом обработчик: у оригинала это слот, а
            /// слоты выполняются после того, как сигнал разошёлся по
            /// привязкам QML.
            fn notify_selected_count(mut self: ::core::pin::Pin<&mut Self>) {
                self.as_mut().selected_count_changed();
                self.as_mut().selected_removable_count_changed();
                // waywallen 0.4.4: `selectedCountChanged` и `modelChanged` →
                // `selectedHiddenCountChanged` (цепочки конструктора).
                self.as_mut().selected_hidden_count_changed();
                self.after_selected_count_changed();
            }

            /// `cancelIfEmpty` оригинала: выбор опустел — выходим из
            /// режима выбора.
            fn cancel_if_empty(mut self: ::core::pin::Pin<&mut Self>) {
                if self.selected_count() > 0 || self.keep_active() {
                    self.notify_active_changed();
                    return;
                }
                self.as_mut().set_selection_mode(false);
                self.as_mut().set_anchor_index(-1);
                self.notify_active_changed();
            }

            /// `notifyActiveChanged` оригинала: сигнал только на реальном
            /// перевороте.
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
