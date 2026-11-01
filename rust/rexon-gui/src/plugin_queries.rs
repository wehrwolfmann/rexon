// Менеджер плагинов: список, установка из zip, инспекция zip, удаление,
// проверка и установка обновлений, действия плагина с формой значений.
//
// Эталон — waywallen `ui/src/query/{plugin,plugin_action,
// progress}_query.cpp` @e5626c1 (вендорный коммит v0.2.6+9), а не наш
// Python-мост. Порт чинит три расхождения (правило #16):
//
//   1. `PluginDeleteQuery.reload()` не был переопределён вовсе — свойство
//      `pluginId` этого запроса было мёртвым. У оригинала
//      `reload() == remove(pluginId)`.
//   2. Проверка и установка обновлений у оригинала наследуются от
//      `ProgressQuery`: RPC лишь ПРИНИМАЕТ задачу и отдаёт `query_id`, а
//      идёт она событиями `task_progress` демона. У нас этого класса не
//      было совсем: свойств `queryId`/`progress`/`progressing` не
//      существовало (а вендорный QML их читает —
//      `PluginManagePage.qml:499,501`, привязки индикатора были мёртвыми),
//      сигналы `checked`/`installed` летели по ПРИЁМУ задачи, и тост
//      «Обновлён "X"» появлялся раньше, чем обновление вставало.
//   3. `PluginActionQuery`: у оригинала при ответе-отказе демона
//      `completed` не летит ВООБЩЕ (лямбду `inspect_set` на не-OK не
//      зовёт), и диалог формы действия остаётся открытым навсегда. Наш
//      Python это чинил; починка сохранена и перенесена в ядро.
//
// ⚠️ Вынужденное отличие сигнатуры: у оригинала `check(const QString&
// pluginId = {})` — moc печатает обе арности. cxx-qt значений по умолчанию
// не умеет, а вендорный QML зовёт РОВНО `updateCheckQuery.check()`
// (PluginManagePage.qml:95). Поэтому `check()` объявлен без аргументов и
// означает то же, что `check({})` у оригинала: очистить `pluginId` и
// проверить ВСЕ плагины. Проверка одного — `pluginId = id; reload()`,
// как и у оригинала (`reload() { check(m_plugin_id); }`).

/// Мост cxx-qt: семь запросов менеджера плагинов.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QVariant, plugins, READ = plugins, NOTIFY = plugins_changed)]
        #[qproperty(QStringList, inactive_system, cxx_name = "inactiveSystem",
                    READ = inactive_system, NOTIFY = plugins_changed)]
        #[qproperty(QStringList, inactive_user, cxx_name = "inactiveUser",
                    READ = inactive_user, NOTIFY = plugins_changed)]
        type PluginListQuery = super::PluginListQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, zip_path, cxx_name = "zipPath", READ = zip_path,
                    WRITE = set_zip_path, NOTIFY = zip_path_changed)]
        #[qproperty(QString, plugin_id, cxx_name = "pluginId", READ = plugin_id,
                    NOTIFY = result_changed)]
        #[qproperty(bool, needs_restart, cxx_name = "needsRestart", READ = needs_restart,
                    NOTIFY = result_changed)]
        type PluginInstallQuery = super::PluginInstallQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, zip_path, cxx_name = "zipPath", READ = zip_path,
                    WRITE = set_zip_path, NOTIFY = zip_path_changed)]
        #[qproperty(QString, plugin_id, cxx_name = "pluginId", READ = plugin_id,
                    NOTIFY = result_changed)]
        #[qproperty(QString, name, READ = name, NOTIFY = result_changed)]
        #[qproperty(QString, version, READ = version, NOTIFY = result_changed)]
        #[qproperty(QString, update, READ = update, NOTIFY = result_changed)]
        #[qproperty(bool, has_source, cxx_name = "hasSource", READ = has_source,
                    NOTIFY = result_changed)]
        #[qproperty(QStringList, renderers, READ = renderers, NOTIFY = result_changed)]
        #[qproperty(bool, overwrite, READ = overwrite, NOTIFY = result_changed)]
        #[qproperty(QString, existing_version, cxx_name = "existingVersion",
                    READ = existing_version, NOTIFY = result_changed)]
        #[qproperty(QString, existing_name, cxx_name = "existingName",
                    READ = existing_name, NOTIFY = result_changed)]
        #[qproperty(bool, existing_system, cxx_name = "existingSystem",
                    READ = existing_system, NOTIFY = result_changed)]
        type PluginInspectQuery = super::PluginInspectQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, plugin_id, cxx_name = "pluginId", READ = plugin_id,
                    NOTIFY = result_changed)]
        #[qproperty(bool, needs_restart, cxx_name = "needsRestart", READ = needs_restart,
                    NOTIFY = result_changed)]
        type PluginDeleteQuery = super::PluginDeleteQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, query_id, cxx_name = "queryId", READ = query_id,
                    NOTIFY = query_id_changed)]
        #[qproperty(f64, progress, READ = progress, NOTIFY = progress_changed)]
        #[qproperty(bool, progressing, READ = progressing, NOTIFY = progressing_changed)]
        #[qproperty(QString, plugin_id, cxx_name = "pluginId", READ = plugin_id,
                    WRITE = set_plugin_id, NOTIFY = plugin_id_changed)]
        #[qproperty(QVariant, updates, READ = updates, NOTIFY = updates_changed)]
        type PluginUpdateCheckQuery = super::PluginUpdateCheckQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, query_id, cxx_name = "queryId", READ = query_id,
                    NOTIFY = query_id_changed)]
        #[qproperty(f64, progress, READ = progress, NOTIFY = progress_changed)]
        #[qproperty(bool, progressing, READ = progressing, NOTIFY = progressing_changed)]
        #[qproperty(QString, plugin_id, cxx_name = "pluginId", READ = plugin_id,
                    NOTIFY = plugin_id_changed)]
        type PluginUpdateInstallQuery = super::PluginUpdateInstallQueryRust;

        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError", READ = forward_error,
                    WRITE = set_forward_error, NOTIFY = forward_error_changed)]
        #[qproperty(QString, plugin_id, cxx_name = "pluginId", READ = plugin_id,
                    WRITE = set_plugin_id, NOTIFY = plugin_id_changed)]
        #[qproperty(QString, action_id, cxx_name = "actionId", READ = action_id,
                    WRITE = set_action_id, NOTIFY = action_id_changed)]
        type PluginActionQuery = super::PluginActionQueryRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PluginListQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PluginListQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PluginListQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PluginListQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PluginListQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PluginListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PluginListQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PluginListQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PluginInstallQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PluginInstallQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PluginInstallQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PluginInstallQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PluginInstallQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PluginInstallQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PluginInstallQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PluginInstallQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PluginInspectQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PluginInspectQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PluginInspectQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PluginInspectQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PluginInspectQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PluginInspectQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PluginInspectQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PluginInspectQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PluginDeleteQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PluginDeleteQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PluginDeleteQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PluginDeleteQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PluginDeleteQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PluginDeleteQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PluginDeleteQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PluginDeleteQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PluginUpdateCheckQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PluginUpdateCheckQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PluginUpdateCheckQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PluginUpdateCheckQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PluginUpdateCheckQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PluginUpdateCheckQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PluginUpdateCheckQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PluginUpdateCheckQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PluginUpdateInstallQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PluginUpdateInstallQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PluginUpdateInstallQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PluginUpdateInstallQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PluginUpdateInstallQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PluginUpdateInstallQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PluginUpdateInstallQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PluginUpdateInstallQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut PluginActionQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut PluginActionQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut PluginActionQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut PluginActionQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut PluginActionQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut PluginActionQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut PluginActionQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut PluginActionQuery>, token: i64, field: QString,
                        request: QByteArray);

        #[qsignal]
        #[cxx_name = "pluginsChanged"]
        fn plugins_changed(self: Pin<&mut PluginListQuery>);

        #[qsignal]
        #[cxx_name = "zipPathChanged"]
        fn zip_path_changed(self: Pin<&mut PluginInstallQuery>);
        #[qsignal]
        #[cxx_name = "resultChanged"]
        fn result_changed(self: Pin<&mut PluginInstallQuery>);
        #[qsignal]
        fn installed(self: Pin<&mut PluginInstallQuery>, plugin_id: QString,
                     needs_restart: bool);

        #[qsignal]
        #[cxx_name = "zipPathChanged"]
        fn zip_path_changed(self: Pin<&mut PluginInspectQuery>);
        #[qsignal]
        #[cxx_name = "resultChanged"]
        fn result_changed(self: Pin<&mut PluginInspectQuery>);
        #[qsignal]
        fn inspected(self: Pin<&mut PluginInspectQuery>);

        #[qsignal]
        #[cxx_name = "resultChanged"]
        fn result_changed(self: Pin<&mut PluginDeleteQuery>);
        #[qsignal]
        fn deleted(self: Pin<&mut PluginDeleteQuery>, plugin_id: QString,
                   needs_restart: bool);

        #[qsignal]
        #[cxx_name = "queryIdChanged"]
        fn query_id_changed(self: Pin<&mut PluginUpdateCheckQuery>);
        #[qsignal]
        #[cxx_name = "progressChanged"]
        fn progress_changed(self: Pin<&mut PluginUpdateCheckQuery>);
        #[qsignal]
        #[cxx_name = "progressingChanged"]
        fn progressing_changed(self: Pin<&mut PluginUpdateCheckQuery>);
        #[qsignal]
        #[cxx_name = "progressEnded"]
        fn progress_ended(self: Pin<&mut PluginUpdateCheckQuery>, error: bool,
                          message: QString);
        #[qsignal]
        #[cxx_name = "pluginIdChanged"]
        fn plugin_id_changed(self: Pin<&mut PluginUpdateCheckQuery>);
        #[qsignal]
        #[cxx_name = "updatesChanged"]
        fn updates_changed(self: Pin<&mut PluginUpdateCheckQuery>);
        #[qsignal]
        fn checked(self: Pin<&mut PluginUpdateCheckQuery>);

        #[qsignal]
        #[cxx_name = "queryIdChanged"]
        fn query_id_changed(self: Pin<&mut PluginUpdateInstallQuery>);
        #[qsignal]
        #[cxx_name = "progressChanged"]
        fn progress_changed(self: Pin<&mut PluginUpdateInstallQuery>);
        #[qsignal]
        #[cxx_name = "progressingChanged"]
        fn progressing_changed(self: Pin<&mut PluginUpdateInstallQuery>);
        #[qsignal]
        #[cxx_name = "progressEnded"]
        fn progress_ended(self: Pin<&mut PluginUpdateInstallQuery>, error: bool,
                          message: QString);
        #[qsignal]
        #[cxx_name = "pluginIdChanged"]
        fn plugin_id_changed(self: Pin<&mut PluginUpdateInstallQuery>);
        #[qsignal]
        fn installed(self: Pin<&mut PluginUpdateInstallQuery>, plugin_id: QString);

        #[qsignal]
        #[cxx_name = "pluginIdChanged"]
        fn plugin_id_changed(self: Pin<&mut PluginActionQuery>);
        #[qsignal]
        #[cxx_name = "actionIdChanged"]
        fn action_id_changed(self: Pin<&mut PluginActionQuery>);
        #[qsignal]
        fn completed(self: Pin<&mut PluginActionQuery>, accepted: bool, error: QString,
                     session_id: QString);
    }

    extern "RustQt" {
        fn status(self: &PluginListQuery) -> i32;
        fn error(self: &PluginListQuery) -> QString;
        fn querying(self: &PluginListQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PluginListQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PluginListQuery>, value: bool);

        fn status(self: &PluginInstallQuery) -> i32;
        fn error(self: &PluginInstallQuery) -> QString;
        fn querying(self: &PluginInstallQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PluginInstallQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PluginInstallQuery>, value: bool);

        fn status(self: &PluginInspectQuery) -> i32;
        fn error(self: &PluginInspectQuery) -> QString;
        fn querying(self: &PluginInspectQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PluginInspectQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PluginInspectQuery>, value: bool);

        fn status(self: &PluginDeleteQuery) -> i32;
        fn error(self: &PluginDeleteQuery) -> QString;
        fn querying(self: &PluginDeleteQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PluginDeleteQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PluginDeleteQuery>, value: bool);

        fn status(self: &PluginUpdateCheckQuery) -> i32;
        fn error(self: &PluginUpdateCheckQuery) -> QString;
        fn querying(self: &PluginUpdateCheckQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PluginUpdateCheckQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PluginUpdateCheckQuery>, value: bool);

        fn status(self: &PluginUpdateInstallQuery) -> i32;
        fn error(self: &PluginUpdateInstallQuery) -> QString;
        fn querying(self: &PluginUpdateInstallQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PluginUpdateInstallQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PluginUpdateInstallQuery>, value: bool);

        fn status(self: &PluginActionQuery) -> i32;
        fn error(self: &PluginActionQuery) -> QString;
        fn querying(self: &PluginActionQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &PluginActionQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut PluginActionQuery>, value: bool);

        fn plugins(self: &PluginListQuery) -> QVariant;
        #[cxx_name = "inactiveSystem"]
        fn inactive_system(self: &PluginListQuery) -> QStringList;
        #[cxx_name = "inactiveUser"]
        fn inactive_user(self: &PluginListQuery) -> QStringList;

        #[cxx_name = "zipPath"]
        fn zip_path(self: &PluginInstallQuery) -> QString;
        #[cxx_name = "setZipPath"]
        fn set_zip_path(self: Pin<&mut PluginInstallQuery>, value: QString);
        #[cxx_name = "pluginId"]
        fn plugin_id(self: &PluginInstallQuery) -> QString;
        #[cxx_name = "needsRestart"]
        fn needs_restart(self: &PluginInstallQuery) -> bool;

        #[cxx_name = "zipPath"]
        fn zip_path(self: &PluginInspectQuery) -> QString;
        #[cxx_name = "setZipPath"]
        fn set_zip_path(self: Pin<&mut PluginInspectQuery>, value: QString);
        #[cxx_name = "pluginId"]
        fn plugin_id(self: &PluginInspectQuery) -> QString;
        fn name(self: &PluginInspectQuery) -> QString;
        fn version(self: &PluginInspectQuery) -> QString;
        fn update(self: &PluginInspectQuery) -> QString;
        #[cxx_name = "hasSource"]
        fn has_source(self: &PluginInspectQuery) -> bool;
        fn renderers(self: &PluginInspectQuery) -> QStringList;
        fn overwrite(self: &PluginInspectQuery) -> bool;
        #[cxx_name = "existingVersion"]
        fn existing_version(self: &PluginInspectQuery) -> QString;
        #[cxx_name = "existingName"]
        fn existing_name(self: &PluginInspectQuery) -> QString;
        #[cxx_name = "existingSystem"]
        fn existing_system(self: &PluginInspectQuery) -> bool;

        #[cxx_name = "pluginId"]
        fn plugin_id(self: &PluginDeleteQuery) -> QString;
        #[cxx_name = "needsRestart"]
        fn needs_restart(self: &PluginDeleteQuery) -> bool;

        #[cxx_name = "queryId"]
        fn query_id(self: &PluginUpdateCheckQuery) -> QString;
        fn progress(self: &PluginUpdateCheckQuery) -> f64;
        fn progressing(self: &PluginUpdateCheckQuery) -> bool;
        #[cxx_name = "pluginId"]
        fn plugin_id(self: &PluginUpdateCheckQuery) -> QString;
        #[cxx_name = "setPluginId"]
        fn set_plugin_id(self: Pin<&mut PluginUpdateCheckQuery>, value: QString);
        fn updates(self: &PluginUpdateCheckQuery) -> QVariant;

        #[cxx_name = "queryId"]
        fn query_id(self: &PluginUpdateInstallQuery) -> QString;
        fn progress(self: &PluginUpdateInstallQuery) -> f64;
        fn progressing(self: &PluginUpdateInstallQuery) -> bool;
        #[cxx_name = "pluginId"]
        fn plugin_id(self: &PluginUpdateInstallQuery) -> QString;

        #[cxx_name = "pluginId"]
        fn plugin_id(self: &PluginActionQuery) -> QString;
        #[cxx_name = "setPluginId"]
        fn set_plugin_id(self: Pin<&mut PluginActionQuery>, value: QString);
        #[cxx_name = "actionId"]
        fn action_id(self: &PluginActionQuery) -> QString;
        #[cxx_name = "setActionId"]
        fn set_action_id(self: Pin<&mut PluginActionQuery>, value: QString);
    }

    extern "RustQt" {
        #[qinvokable]
        fn reload(self: Pin<&mut PluginListQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PluginListQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut PluginInstallQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PluginInstallQuery>, token: i64, frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut PluginInspectQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PluginInspectQuery>, token: i64, frame: &QByteArray);

        /// ⚠️ Починка: у оригинала `reload() == remove(pluginId)`, а у нас
        /// его не было вовсе — свойство `pluginId` было мёртвым.
        #[qinvokable]
        fn reload(self: Pin<&mut PluginDeleteQuery>);
        #[qinvokable]
        fn remove(self: Pin<&mut PluginDeleteQuery>, plugin_id: QString);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PluginDeleteQuery>, token: i64, frame: &QByteArray);

        /// Проверить обновление ТЕКУЩЕГО `pluginId` (оригинал:
        /// `reload() { check(m_plugin_id); }`).
        #[qinvokable]
        fn reload(self: Pin<&mut PluginUpdateCheckQuery>);
        /// Проверить ВСЕ плагины — то же, что `check({})` у оригинала.
        #[qinvokable]
        fn check(self: Pin<&mut PluginUpdateCheckQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PluginUpdateCheckQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut PluginUpdateInstallQuery>);
        #[qinvokable]
        fn install(self: Pin<&mut PluginUpdateInstallQuery>, plugin_id: QString);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PluginUpdateInstallQuery>, token: i64,
                            frame: &QByteArray);

        #[qinvokable]
        fn reload(self: Pin<&mut PluginActionQuery>);
        #[qinvokable]
        fn invoke(self: Pin<&mut PluginActionQuery>, values: QVariant);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut PluginActionQuery>, token: i64, frame: &QByteArray);
    }

    impl cxx_qt::Threading for PluginUpdateCheckQuery {}
    impl cxx_qt::Threading for PluginUpdateInstallQuery {}
    impl cxx_qt::Constructor<()> for PluginUpdateCheckQuery {}
    impl cxx_qt::Constructor<()> for PluginUpdateInstallQuery {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::{QByteArray, QString, QStringList, QVariant};
use prost::Message;
use serde_json::Value;

use rexon_core::proto::control as pb;
use rexon_core::we_manage::{self, ProgressState, TaskProgressSnapshot, PROGRESS_ENDED};
use rexon_core::we_query::{QuerySignal, QueryState, STATUS_ERROR};

use crate::impl_query_base;
use crate::query_base::{decode_failed, progress};
use crate::qvalue::{json_to_variant, variant_to_json};

fn to_qstringlist(items: &[String]) -> QStringList {
    let mut list = QStringList::default();
    for item in items {
        list.append(QString::from(item));
    }
    list
}

/// Разослать сигналы «кадр не разобрался».
macro_rules! on_decode_error {
    ($self:ident) => {{
        let signals = decode_failed(&mut $self.as_mut().rust_mut().base);
        $self.dispatch(signals);
    }};
}

// ════════════════════════════════════════════════════════════════════════
// PluginListQuery — установленные плагины-пакеты
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct PluginListQueryRust {
    base: QueryState,
    data: we_manage::PluginListData,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(PluginListQuery);

impl qobject::PluginListQuery {
    pub fn plugins(&self) -> QVariant {
        json_to_variant(&Value::Array(self.data.plugins.clone()))
    }

    pub fn inactive_system(&self) -> QStringList {
        to_qstringlist(&self.data.inactive_system)
    }

    pub fn inactive_user(&self) -> QStringList {
        to_qstringlist(&self.data.inactive_user)
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let payload = pb::PluginListRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("plugin_list", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::apply_plugin_list(&mut rust.base, &mut rust.data, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            // Один сигнал на три свойства — так у оригинала.
            "plugins" => self.as_mut().plugins_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// PluginInstallQuery — установка плагина из zip
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct PluginInstallQueryRust {
    base: QueryState,
    zip_path: String,
    result: we_manage::PluginIdResult,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(PluginInstallQuery);

impl qobject::PluginInstallQuery {
    pub fn zip_path(&self) -> QString {
        QString::from(&self.zip_path)
    }

    pub fn set_zip_path(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.zip_path == next {
            return;
        }
        self.as_mut().rust_mut().zip_path = next;
        self.zip_path_changed();
    }

    pub fn plugin_id(&self) -> QString {
        QString::from(&self.result.plugin_id)
    }

    pub fn needs_restart(&self) -> bool {
        self.result.needs_restart
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        if self.zip_path.is_empty() {
            return;
        }
        let payload = pb::PluginInstallRequest {
            zip_path: self.zip_path.clone(),
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("plugin_install", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::apply_plugin_install(&mut rust.base, &mut rust.result, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "result" => self.as_mut().result_changed(),
            "installed" => {
                let id = QString::from(&self.result.plugin_id);
                let restart = self.result.needs_restart;
                self.as_mut().installed(id, restart);
            }
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// PluginInspectQuery — что внутри zip ДО установки
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct PluginInspectQueryRust {
    base: QueryState,
    zip_path: String,
    data: we_manage::PluginInspectData,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(PluginInspectQuery);

impl qobject::PluginInspectQuery {
    pub fn zip_path(&self) -> QString {
        QString::from(&self.zip_path)
    }

    pub fn set_zip_path(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.zip_path == next {
            return;
        }
        self.as_mut().rust_mut().zip_path = next;
        self.zip_path_changed();
    }

    pub fn plugin_id(&self) -> QString {
        QString::from(&self.data.plugin_id)
    }
    pub fn name(&self) -> QString {
        QString::from(&self.data.name)
    }
    pub fn version(&self) -> QString {
        QString::from(&self.data.version)
    }
    pub fn update(&self) -> QString {
        QString::from(&self.data.update)
    }
    pub fn has_source(&self) -> bool {
        self.data.has_source
    }
    pub fn renderers(&self) -> QStringList {
        to_qstringlist(&self.data.renderers)
    }
    pub fn overwrite(&self) -> bool {
        self.data.overwrite
    }
    pub fn existing_version(&self) -> QString {
        QString::from(&self.data.existing_version)
    }
    pub fn existing_name(&self) -> QString {
        QString::from(&self.data.existing_name)
    }
    pub fn existing_system(&self) -> bool {
        self.data.existing_system
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        if self.zip_path.is_empty() {
            return;
        }
        let payload = pb::PluginInspectRequest {
            zip_path: self.zip_path.clone(),
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("plugin_inspect", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::apply_plugin_inspect(&mut rust.base, &mut rust.data, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "result" => self.as_mut().result_changed(),
            "inspected" => self.as_mut().inspected(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// PluginDeleteQuery — удаление пользовательского плагина
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct PluginDeleteQueryRust {
    base: QueryState,
    result: we_manage::PluginIdResult,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(PluginDeleteQuery);

impl qobject::PluginDeleteQuery {
    pub fn plugin_id(&self) -> QString {
        QString::from(&self.result.plugin_id)
    }

    pub fn needs_restart(&self) -> bool {
        self.result.needs_restart
    }

    /// ⚠️ Починка: у оригинала `reload()` — это `remove(pluginId)`.
    pub fn reload(mut self: Pin<&mut Self>) {
        let id = QString::from(&self.result.plugin_id);
        self.as_mut().remove(id);
    }

    pub fn remove(mut self: Pin<&mut Self>, plugin_id: QString) {
        let id = plugin_id.to_string();
        if id.is_empty() {
            return;
        }
        let payload = pb::PluginDeleteRequest {
            plugin_id: id,
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("plugin_delete", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::apply_plugin_delete(&mut rust.base, &mut rust.result, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "result" => self.as_mut().result_changed(),
            "deleted" => {
                let id = QString::from(&self.result.plugin_id);
                let restart = self.result.needs_restart;
                self.as_mut().deleted(id, restart);
            }
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// Запросы с прогрессом длинной задачи (ProgressQuery оригинала)
// ════════════════════════════════════════════════════════════════════════
//
// RPC только ПРИНИМАЕТ задачу и отдаёт `query_id`; дальше её ведут события
// `task_progress` демона. Событие приходит сырым кадром в транспортный шов,
// разбирается ядром и рассылается реестром `query_base::progress`.

/// Общая часть обеих: свойства ProgressQuery и подписка на прогресс.
macro_rules! impl_progress_query {
    ($query:ident) => {
        impl qobject::$query {
            pub fn query_id(&self) -> QString {
                QString::from(&self.progress.query_id)
            }

            pub fn progress(&self) -> f64 {
                self.progress.progress
            }

            pub fn progressing(&self) -> bool {
                self.progress.progressing
            }

            /// Подписаться на события прогресса.
            ///
            /// ⚠️ `queue(...)` без `unwrap()` и без предварительного
            /// `is_destroyed()`: между проверкой и постановкой объект
            /// успевает умереть — так прямо предписывает doc-комментарий
            /// cxx-qt. Мёртвый объект даёт `ObjectDestroyed`, и запись
            /// снимается прямо в рассылке.
            fn subscribe_progress(mut self: Pin<&mut Self>) {
                let handle = self.as_ref().get_ref().qt_thread();
                let id = progress::register(Box::new(move |query_id, snapshot| {
                    let query_id = query_id.to_string();
                    let snapshot = snapshot.clone();
                    match handle.queue(move |object| {
                        object.on_task_progress(&query_id, &snapshot);
                    }) {
                        Ok(()) => Ok(()),
                        Err(ThreadingQueueError::ObjectDestroyed) => Err(()),
                        // Объект ещё жив, следующая рассылка может пройти.
                        Err(_) => Ok(()),
                    }
                }));
                self.as_mut().rust_mut().progress_id = id;
            }

            /// Снимок прогресса приехал — наш ли он.
            pub fn on_task_progress(
                mut self: Pin<&mut Self>,
                query_id: &str,
                snapshot: &TaskProgressSnapshot,
            ) {
                if !we_manage::progress_matches(&self.progress, query_id, snapshot.sequence) {
                    return;
                }
                let signals = {
                    let mut rust = self.as_mut().rust_mut();
                    let rust = &mut *rust;
                    we_manage::apply_task_progress(&mut rust.base, &mut rust.progress, snapshot)
                };
                self.dispatch(signals);
            }

            /// Сигналы, общие для обеих: три свойства прогресса и конец.
            /// `false` — имя не про прогресс, разбирает вызывающий.
            fn emit_progress_property(mut self: Pin<&mut Self>, name: &str) -> bool {
                match name {
                    "queryId" => self.as_mut().query_id_changed(),
                    "progress" => self.as_mut().progress_changed(),
                    "progressing" => self.as_mut().progressing_changed(),
                    _ => return false,
                }
                true
            }
        }
    };
}

// ── PluginUpdateCheckQuery ───────────────────────────────────────────────

#[derive(Default)]
pub struct PluginUpdateCheckQueryRust {
    base: QueryState,
    progress: ProgressState,
    plugin_id: String,
    data: we_manage::PluginUpdateCheckData,
    pending: std::collections::BTreeSet<i64>,
    /// Номер записи в реестре прогресса; 0 — не подписан.
    progress_id: u64,
}

impl Drop for PluginUpdateCheckQueryRust {
    fn drop(&mut self) {
        if self.progress_id != 0 {
            progress::unregister(self.progress_id);
        }
    }
}

impl_query_base!(PluginUpdateCheckQuery);
impl_progress_query!(PluginUpdateCheckQuery);

impl cxx_qt::Initialize for qobject::PluginUpdateCheckQuery {
    fn initialize(self: Pin<&mut Self>) {
        self.subscribe_progress();
    }
}

impl qobject::PluginUpdateCheckQuery {
    pub fn plugin_id(&self) -> QString {
        QString::from(&self.plugin_id)
    }

    pub fn set_plugin_id(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.plugin_id == next {
            return;
        }
        self.as_mut().rust_mut().plugin_id = next;
        self.plugin_id_changed();
    }

    pub fn updates(&self) -> QVariant {
        json_to_variant(&Value::Array(self.data.updates.clone()))
    }

    /// Проверить обновление текущего `pluginId`.
    pub fn reload(self: Pin<&mut Self>) {
        self.send_check();
    }

    /// Проверить ВСЕ плагины: `check({})` оригинала очищает `pluginId`.
    pub fn check(mut self: Pin<&mut Self>) {
        self.as_mut().set_plugin_id(QString::default());
        self.send_check();
    }

    fn send_check(mut self: Pin<&mut Self>) {
        // Оригинал: `if (querying() || progressing()) return;` — второй
        // заход не затирает идущую задачу.
        if self.as_ref().querying() || self.progress.progressing {
            return;
        }
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::begin_progress(&mut rust.base, &mut rust.progress)
        };
        self.as_mut().dispatch(signals);
        let payload = pb::PluginUpdateCheckRequest {
            plugin_id: self.plugin_id.clone(),
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("plugin_update_check", payload) {
            self.rust_mut().pending.insert(token);
            return;
        }
        // Кадр не ушёл (нет соединения) — у оригинала это ветка
        // `failProgressQuery`: снять «идёт» и сообщить о конце, иначе
        // индикатор крутился бы вечно.
        self.fail_progress_now();
    }

    /// Закрыть прогресс отказом с уже выставленным текстом ошибки.
    fn fail_progress_now(mut self: Pin<&mut Self>) {
        let message = self.base.error.clone();
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::fail_progress(&mut rust.base, &mut rust.progress, &message)
        };
        self.dispatch(signals);
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::apply_plugin_update_check(
                &mut rust.base,
                &mut rust.progress,
                &mut rust.data,
                frame.as_slice(),
            )
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        if self.as_mut().emit_progress_property(name) {
            return;
        }
        match name {
            "updates" => self.as_mut().updates_changed(),
            PROGRESS_ENDED => {
                let error = self.base.status == STATUS_ERROR;
                let message = QString::from(&self.progress.last_message);
                self.as_mut().progress_ended(error, message);
                // Оригинал вешает это лямбдой в конструкторе: `checked()`
                // только когда задача доиграла БЕЗ ошибки.
                if !error {
                    self.as_mut().checked();
                }
            }
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ── PluginUpdateInstallQuery ─────────────────────────────────────────────

#[derive(Default)]
pub struct PluginUpdateInstallQueryRust {
    base: QueryState,
    progress: ProgressState,
    plugin_id: String,
    pending: std::collections::BTreeSet<i64>,
    progress_id: u64,
}

impl Drop for PluginUpdateInstallQueryRust {
    fn drop(&mut self) {
        if self.progress_id != 0 {
            progress::unregister(self.progress_id);
        }
    }
}

impl_query_base!(PluginUpdateInstallQuery);
impl_progress_query!(PluginUpdateInstallQuery);

impl cxx_qt::Initialize for qobject::PluginUpdateInstallQuery {
    fn initialize(self: Pin<&mut Self>) {
        self.subscribe_progress();
    }
}

impl qobject::PluginUpdateInstallQuery {
    pub fn plugin_id(&self) -> QString {
        QString::from(&self.plugin_id)
    }

    pub fn reload(mut self: Pin<&mut Self>) {
        let id = QString::from(&self.plugin_id);
        self.as_mut().install(id);
    }

    pub fn install(mut self: Pin<&mut Self>, plugin_id: QString) {
        let id = plugin_id.to_string();
        if id.is_empty() || self.as_ref().querying() || self.progress.progressing {
            return;
        }
        if self.plugin_id != id {
            self.as_mut().rust_mut().plugin_id = id;
            self.as_mut().plugin_id_changed();
        }
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::begin_progress(&mut rust.base, &mut rust.progress)
        };
        self.as_mut().dispatch(signals);
        let payload = pb::PluginUpdateInstallRequest {
            plugin_id: self.plugin_id.clone(),
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("plugin_update_install", payload) {
            self.rust_mut().pending.insert(token);
            return;
        }
        self.fail_progress_now();
    }

    /// Закрыть прогресс отказом с уже выставленным текстом ошибки.
    fn fail_progress_now(mut self: Pin<&mut Self>) {
        let message = self.base.error.clone();
        let signals = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::fail_progress(&mut rust.base, &mut rust.progress, &message)
        };
        self.dispatch(signals);
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::apply_plugin_update_install(
                &mut rust.base,
                &mut rust.progress,
                frame.as_slice(),
            )
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        if self.as_mut().emit_progress_property(name) {
            return;
        }
        match name {
            PROGRESS_ENDED => {
                let error = self.base.status == STATUS_ERROR;
                let message = QString::from(&self.progress.last_message);
                self.as_mut().progress_ended(error, message);
                if !error {
                    let id = QString::from(&self.plugin_id);
                    self.as_mut().installed(id);
                }
            }
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

// ════════════════════════════════════════════════════════════════════════
// PluginActionQuery — действие плагина с формой значений
// ════════════════════════════════════════════════════════════════════════

#[derive(Default)]
pub struct PluginActionQueryRust {
    base: QueryState,
    plugin_id: String,
    action_id: String,
    result: we_manage::PluginActionResult,
    pending: std::collections::BTreeSet<i64>,
}

impl_query_base!(PluginActionQuery);

impl qobject::PluginActionQuery {
    pub fn plugin_id(&self) -> QString {
        QString::from(&self.plugin_id)
    }

    pub fn set_plugin_id(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.plugin_id == next {
            return;
        }
        self.as_mut().rust_mut().plugin_id = next;
        self.plugin_id_changed();
    }

    pub fn action_id(&self) -> QString {
        QString::from(&self.action_id)
    }

    pub fn set_action_id(mut self: Pin<&mut Self>, value: QString) {
        let next = value.to_string();
        if self.action_id == next {
            return;
        }
        self.as_mut().rust_mut().action_id = next;
        self.action_id_changed();
    }

    pub fn reload(self: Pin<&mut Self>) {
        self.invoke(QVariant::default());
    }

    pub fn invoke(mut self: Pin<&mut Self>, values: QVariant) {
        let payload = pb::PluginActionRequest {
            plugin_id: self.plugin_id.clone(),
            action_id: self.action_id.clone(),
            values: we_manage::values_to_wire(&variant_to_json(&values)),
        }
        .encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("plugin_action", payload) {
            self.as_mut().rust_mut().pending.insert(token);
            return;
        }
        // Соединения нет: диалог формы ждёт `completed`, иначе он висит.
        let error = self.base.error.clone();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.result = we_manage::PluginActionResult {
                accepted: false,
                error,
                session_id: String::new(),
            };
        }
        self.dispatch(vec![QuerySignal::Property("completed")]);
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_manage::apply_plugin_action(&mut rust.base, &mut rust.result, frame.as_slice())
        };
        match parsed {
            Ok(signals) => self.dispatch(signals),
            Err(_) => on_decode_error!(self),
        }
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            "completed" => {
                let accepted = self.result.accepted;
                let error = QString::from(&self.result.error);
                let session = QString::from(&self.result.session_id);
                self.as_mut().completed(accepted, error, session);
            }
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}
