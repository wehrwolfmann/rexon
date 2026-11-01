// AppearanceModel — вкладка «Оформление» на Rust + cxx-qt + QML.
//
// Последняя из вкладок, перенесённых на QML по общей схеме: решения и
// подписи — ядро, здесь оболочка QObject, в QML только разметка.
//
// Что здесь есть и чего нет:
//   * ЕСТЬ оболочка QObject: боковая панель разделов, шапка (чипы источника,
//     под-чипы «вида» и режима блокировки, поиск, выбор рабочего стола,
//     сортировка), строка состояния, полоса страниц и модель плиток
//     каталога с ролями делегата;
//   * НЕТ ни одной строки продуктовых решений. Что показать, в каком
//     порядке, какими словами и с какой кнопкой — отвечает
//     `rexon_core::appearance_page` (2 690 строк перенесённой логики).
//
// ⚠️ Сеть и диск остаются снаружи. У ядра есть и разбор ответов магазина
// (`ocs`), и AUR (`aur`), и обход диска (`installed_scan`), но ЖИВОГО
// исполнителя `OcsCacheIo` в продукте пока нет — SQLite-кеш и HTTP живут в
// `rexon/appearance/ocs_client.py`. Поэтому страница спрашивает у хозяина
// (`root.actions`) ровно то, что ядро само велело спросить: состав запроса
// собирает `catalogRequest()`, ответ принимает `catalogAnswer()`. Когда
// исполнитель приедет на Rust, на место хозяина встанет он — ни страница,
// ни этот мост не изменятся.

/// Мост cxx-qt: содержимое вкладки «Оформление». Оно же — модель плиток.
///
/// ⚠️ Тип зовётся `AppearanceModel`, а не `AppearancePage`: .qml-файлы
/// модуля сами становятся QML-типами по имени файла, и `AppearancePage`
/// занят `qml/AppearancePage.qml` (грабля 1 схемы переноса).
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// Базовый Qt-тип. БЕЗ `#[qobject]`: на каждый `#[qobject]`-тип в
        /// `extern "C++Qt"` cxx-qt печатает шимы приведения с именем по
        /// ТИПУ, и второй мост крейта с той же базой даёт `duplicate symbol`.
        type QAbstractListModel;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvector.h");
        /// Набор ролей для dataChanged (в Qt6 QVector<int> == QList<int>).
        type QVector_i32 = cxx_qt_lib::QVector<i32>;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        // ── Окно и шапка: константы ядра ────────────────────────────────
        #[qproperty(QString, window_title, cxx_name = "windowTitle",
                    READ = window_title, CONSTANT)]
        #[qproperty(i32, window_min_width, cxx_name = "windowMinWidth",
                    READ = window_min_width, CONSTANT)]
        #[qproperty(i32, window_min_height, cxx_name = "windowMinHeight",
                    READ = window_min_height, CONSTANT)]
        #[qproperty(QString, header_title, cxx_name = "headerTitle",
                    READ = header_title, CONSTANT)]
        #[qproperty(i32, header_title_point_size, cxx_name = "headerTitlePointSize",
                    READ = header_title_point_size, CONSTANT)]
        // Геометрия каталога: боковая панель, строка поиска, плитка.
        #[qproperty(i32, sidebar_width, cxx_name = "sidebarWidth",
                    READ = sidebar_width, CONSTANT)]
        #[qproperty(i32, sidebar_row_height, cxx_name = "sidebarRowHeight",
                    READ = sidebar_row_height, CONSTANT)]
        #[qproperty(i32, search_width, cxx_name = "searchWidth",
                    READ = search_width, CONSTANT)]
        #[qproperty(i32, card_width, cxx_name = "cardWidth",
                    READ = card_width, CONSTANT)]
        #[qproperty(i32, card_height, cxx_name = "cardHeight",
                    READ = card_height, CONSTANT)]
        #[qproperty(i32, card_preview_height, cxx_name = "cardPreviewHeight",
                    READ = card_preview_height, CONSTANT)]
        #[qproperty(i32, card_text_width, cxx_name = "cardTextWidth",
                    READ = card_text_width, CONSTANT)]
        #[qproperty(i32, card_body_margin, cxx_name = "cardBodyMargin",
                    READ = card_body_margin, CONSTANT)]
        #[qproperty(i32, card_body_top_margin, cxx_name = "cardBodyTopMargin",
                    READ = card_body_top_margin, CONSTANT)]
        #[qproperty(i32, card_body_bottom_margin, cxx_name = "cardBodyBottomMargin",
                    READ = card_body_bottom_margin, CONSTANT)]
        #[qproperty(i32, card_body_spacing, cxx_name = "cardBodySpacing",
                    READ = card_body_spacing, CONSTANT)]
        #[qproperty(i32, card_button_height, cxx_name = "cardButtonHeight",
                    READ = card_button_height, CONSTANT)]
        #[qproperty(i32, card_button_top_margin, cxx_name = "cardButtonTopMargin",
                    READ = card_button_top_margin, CONSTANT)]
        #[qproperty(i32, card_name_point_size, cxx_name = "cardNamePointSize",
                    READ = card_name_point_size, CONSTANT)]
        #[qproperty(i32, card_meta_point_size, cxx_name = "cardMetaPointSize",
                    READ = card_meta_point_size, CONSTANT)]
        #[qproperty(i32, grid_spacing, cxx_name = "gridSpacing",
                    READ = grid_spacing, CONSTANT)]
        #[qproperty(i32, grid_margin_h, cxx_name = "gridMarginH",
                    READ = grid_margin_h, CONSTANT)]
        #[qproperty(i32, grid_margin_v, cxx_name = "gridMarginV",
                    READ = grid_margin_v, CONSTANT)]
        #[qproperty(i32, header_margin_h, cxx_name = "headerMarginH",
                    READ = header_margin_h, CONSTANT)]
        #[qproperty(i32, header_margin_v, cxx_name = "headerMarginV",
                    READ = header_margin_v, CONSTANT)]
        #[qproperty(i32, catalog_debounce_ms, cxx_name = "catalogDebounceMs",
                    READ = catalog_debounce_ms, CONSTANT)]
        // ── Подсказки поиска ────────────────────────────────────────────
        #[qproperty(i32, suggest_debounce_ms, cxx_name = "suggestDebounceMs",
                    READ = suggest_debounce_ms, CONSTANT)]
        #[qproperty(i32, suggest_row_height, cxx_name = "suggestRowHeight",
                    READ = suggest_row_height, CONSTANT)]
        #[qproperty(i32, suggest_thumb_width, cxx_name = "suggestThumbWidth",
                    READ = suggest_thumb_width, CONSTANT)]
        #[qproperty(i32, suggest_thumb_height, cxx_name = "suggestThumbHeight",
                    READ = suggest_thumb_height, CONSTANT)]
        #[qproperty(QList_QVariant, suggest_rows, cxx_name = "suggestRows",
                    READ = suggest_rows, NOTIFY = suggest_changed)]
        #[qproperty(bool, suggest_visible, cxx_name = "suggestVisible",
                    READ = suggest_visible, NOTIFY = suggest_changed)]
        #[qproperty(i32, suggest_index, cxx_name = "suggestIndex",
                    READ = suggest_index, NOTIFY = suggest_changed)]
        #[qproperty(i32, first_load_ms, cxx_name = "firstLoadMs",
                    READ = first_load_ms, CONSTANT)]
        // Неизменные подписи шапки.
        #[qproperty(QString, source_label, cxx_name = "sourceLabel",
                    READ = source_label, CONSTANT)]
        #[qproperty(QString, submode_label, cxx_name = "submodeLabel",
                    READ = submode_label, CONSTANT)]
        #[qproperty(QString, cursor_size_label, cxx_name = "cursorSizeLabel",
                    READ = cursor_size_label, CONSTANT)]
        #[qproperty(QString, de_label, cxx_name = "deLabel",
                    READ = de_label, CONSTANT)]
        #[qproperty(QString, de_tooltip, cxx_name = "deTooltip",
                    READ = de_tooltip, CONSTANT)]
        #[qproperty(QString, refresh_text, cxx_name = "refreshText",
                    READ = refresh_text, CONSTANT)]
        #[qproperty(QString, refresh_tooltip, cxx_name = "refreshTooltip",
                    READ = refresh_tooltip, CONSTANT)]
        #[qproperty(QString, page_prev_text, cxx_name = "pagePrevText",
                    READ = page_prev_text, CONSTANT)]
        #[qproperty(QString, page_next_text, cxx_name = "pageNextText",
                    READ = page_next_text, CONSTANT)]
        // Сортировки: [{label, key}, …] — таблица ядра, за время жизни
        // вкладки не меняется.
        #[qproperty(QList_QVariant, sorts, READ = sorts, CONSTANT)]

        // ── Палитра действующего скина (правило #17) ────────────────────
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        // Раскладка разделов-групп (`sty` у раздела «Панель MATE»).
        #[qproperty(QMap_QString_QVariant, section_styles, cxx_name = "sectionStyles",
                    READ = section_styles, NOTIFY = palette_changed)]

        // ── Боковая панель разделов ─────────────────────────────────────
        // [{key, icon, name, text, available, current}, …]
        #[qproperty(QList_QVariant, sidebar_rows, cxx_name = "sidebarRows",
                    READ = sidebar_rows, NOTIFY = sidebar_changed)]
        #[qproperty(QString, section_title, cxx_name = "sectionTitle",
                    READ = section_title, NOTIFY = sidebar_changed)]

        // ── Шапка: что видно при текущем разделе ────────────────────────
        #[qproperty(bool, search_visible, cxx_name = "searchVisible",
                    READ = search_visible, NOTIFY = chrome_changed)]
        #[qproperty(QString, search_placeholder, cxx_name = "searchPlaceholder",
                    READ = search_placeholder, NOTIFY = chrome_changed)]
        #[qproperty(i32, search_delay_ms, cxx_name = "searchDelayMs",
                    READ = search_delay_ms, NOTIFY = chrome_changed)]
        #[qproperty(QString, search_clear_tooltip, cxx_name = "searchClearTooltip",
                    READ = search_clear_tooltip, CONSTANT)]
        #[qproperty(QString, caveat_text, cxx_name = "caveatText",
                    READ = caveat_text, NOTIFY = chrome_changed)]
        #[qproperty(bool, source_bar_visible, cxx_name = "sourceBarVisible",
                    READ = source_bar_visible, NOTIFY = chrome_changed)]
        #[qproperty(bool, style_bar_visible, cxx_name = "styleBarVisible",
                    READ = style_bar_visible, NOTIFY = chrome_changed)]
        #[qproperty(bool, lock_bar_visible, cxx_name = "lockBarVisible",
                    READ = lock_bar_visible, NOTIFY = chrome_changed)]
        // Фильтр «Все / Светлые / Тёмные» у цветовых схем и стилей Plasma.
        #[qproperty(bool, tone_bar_visible, cxx_name = "toneBarVisible",
                    READ = tone_bar_visible, NOTIFY = chrome_changed)]
        #[qproperty(bool, cursor_size_bar_visible, cxx_name = "cursorSizeBarVisible",
                    READ = cursor_size_bar_visible, NOTIFY = chrome_changed)]
        #[qproperty(bool, sort_visible, cxx_name = "sortVisible",
                    READ = sort_visible, NOTIFY = chrome_changed)]
        #[qproperty(bool, pagination_visible, cxx_name = "paginationVisible",
                    READ = pagination_visible, NOTIFY = chrome_changed)]
        // «Установить из файла…» в шапке раздела: видимость, подписи кнопки
        // и меню, заголовок и фильтры окна выбора, ключ раздела — всё ядро
        // (`appearance_page`, `local_install`).
        #[qproperty(bool, install_file_visible, cxx_name = "installFileVisible",
                    READ = install_file_visible, NOTIFY = chrome_changed)]
        #[qproperty(QString, install_file_text, cxx_name = "installFileText",
                    READ = install_file_text, CONSTANT)]
        #[qproperty(QString, install_file_tooltip, cxx_name = "installFileTooltip",
                    READ = install_file_tooltip, CONSTANT)]
        #[qproperty(QString, install_file_files_text, cxx_name = "installFileFilesText",
                    READ = install_file_files_text, CONSTANT)]
        #[qproperty(QString, install_file_folder_text, cxx_name = "installFileFolderText",
                    READ = install_file_folder_text, CONSTANT)]
        #[qproperty(QString, install_file_title, cxx_name = "installFileTitle",
                    READ = install_file_title, NOTIFY = chrome_changed)]
        #[qproperty(QList_QVariant, install_file_filters, cxx_name = "installFileFilters",
                    READ = install_file_filters, NOTIFY = chrome_changed)]
        #[qproperty(QString, install_file_section, cxx_name = "installFileSection",
                    READ = install_file_section, NOTIFY = chrome_changed)]
        // Установленное сверху над магазином раздела, выделять ли текущую
        // тему (ядро: `CatalogView::installed_view`, `marks_current`).
        #[qproperty(bool, installed_view, cxx_name = "installedView",
                    READ = installed_view, NOTIFY = chrome_changed)]
        #[qproperty(bool, mark_current, cxx_name = "markCurrent",
                    READ = mark_current, NOTIFY = chrome_changed)]
        // Чипы: [{key, label, tooltip, checked}, …].
        #[qproperty(QList_QVariant, source_chips, cxx_name = "sourceChips",
                    READ = source_chips, NOTIFY = chrome_changed)]
        #[qproperty(QList_QVariant, style_chips, cxx_name = "styleChips",
                    READ = style_chips, NOTIFY = chrome_changed)]
        #[qproperty(QList_QVariant, lock_chips, cxx_name = "lockChips",
                    READ = lock_chips, NOTIFY = chrome_changed)]
        #[qproperty(QList_QVariant, tone_chips, cxx_name = "toneChips",
                    READ = tone_chips, NOTIFY = chrome_changed)]
        // Чипы размера курсора: [{key, label, tooltip}, …]; какой отмечен,
        // решает страница по ответу `actions.cursorSize()`.
        #[qproperty(QList_QVariant, cursor_size_chips, cxx_name = "cursorSizeChips",
                    READ = cursor_size_chips, CONSTANT)]
        #[qproperty(QString, sort_key, cxx_name = "sortKey",
                    READ = sort_key, NOTIFY = chrome_changed)]
        #[qproperty(QString, query, READ = query, NOTIFY = chrome_changed)]
        // Выбор рабочего стола: [{id, name}, …] и текущий.
        #[qproperty(QList_QVariant, de_choices, cxx_name = "deChoices",
                    READ = de_choices, NOTIFY = de_changed)]
        #[qproperty(QString, de, READ = de, NOTIFY = de_changed)]

        // ── Что показано: сетка, деталь или объяснение ──────────────────
        #[qproperty(QString, screen, READ = screen, NOTIFY = screen_changed)]
        #[qproperty(QString, note_text, cxx_name = "noteText",
                    READ = note_text, NOTIFY = screen_changed)]
        // Панель экрана входа при Plasma Login Manager (`screen ==
        // "plasmalogin"`): подписи ядра и видна ли кнопка настроек.
        #[qproperty(QString, plm_title, cxx_name = "plmTitle",
                    READ = plm_title, NOTIFY = screen_changed)]
        #[qproperty(QString, plm_body, cxx_name = "plmBody",
                    READ = plm_body, NOTIFY = screen_changed)]
        #[qproperty(QString, plm_open_label, cxx_name = "plmOpenLabel",
                    READ = plm_open_label, NOTIFY = screen_changed)]
        #[qproperty(bool, plm_open_visible, cxx_name = "plmOpenVisible",
                    READ = plm_open_visible, NOTIFY = screen_changed)]
        #[qproperty(QString, plm_store_hint, cxx_name = "plmStoreHint",
                    READ = plm_store_hint, NOTIFY = screen_changed)]
        // Раздел экрана входа: над полкой и панелью — блок «Менеджер
        // входа» (данные — проба `loginScreenReady`).
        #[qproperty(bool, login_section, cxx_name = "loginSection",
                    READ = login_section, NOTIFY = screen_changed)]

        // ── Строка состояния и полоса страниц ───────────────────────────
        #[qproperty(QString, status_text, cxx_name = "statusText",
                    READ = status_text, NOTIFY = status_changed)]
        #[qproperty(QString, page_label, cxx_name = "pageLabel",
                    READ = page_label, NOTIFY = page_changed)]
        #[qproperty(bool, prev_enabled, cxx_name = "prevEnabled",
                    READ = prev_enabled, NOTIFY = page_changed)]
        #[qproperty(bool, next_enabled, cxx_name = "nextEnabled",
                    READ = next_enabled, NOTIFY = page_changed)]
        // ── Панель деталей ──────────────────────────────────────────────
        #[qproperty(QString, detail_back_text, cxx_name = "detailBackText",
                    READ = detail_back_text, CONSTANT)]
        #[qproperty(QString, detail_uninstall_text, cxx_name = "detailUninstallText",
                    READ = detail_uninstall_text, CONSTANT)]
        #[qproperty(QString, detail_site_text, cxx_name = "detailSiteText",
                    READ = detail_site_text, CONSTANT)]
        #[qproperty(QString, detail_site_tooltip, cxx_name = "detailSiteTooltip",
                    READ = detail_site_tooltip, CONSTANT)]
        #[qproperty(QString, detail_copy_link_text, cxx_name = "detailCopyLinkText",
                    READ = detail_copy_link_text, CONSTANT)]
        #[qproperty(QString, detail_copy_link_tooltip, cxx_name = "detailCopyLinkTooltip",
                    READ = detail_copy_link_tooltip, CONSTANT)]
        #[qproperty(QString, detail_apply_local_text, cxx_name = "detailApplyLocalText",
                    READ = detail_apply_local_text, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_preview_tooltip, cxx_name = "detailPreviewTooltip",
                    READ = detail_preview_tooltip, CONSTANT)]
        // Выбор файла у товара магазина с несколькими файлами: строки —
        // решение ядра (`ap::store_file_choices_json`), подписи — его же.
        #[qproperty(QString, detail_files, cxx_name = "detailFiles",
                    READ = detail_files, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_files_title, cxx_name = "detailFilesTitle",
                    READ = detail_files_title, CONSTANT)]
        #[qproperty(QString, detail_files_hint, cxx_name = "detailFilesHint",
                    READ = detail_files_hint, CONSTANT)]
        #[qproperty(QString, detail_file_will_install, cxx_name = "detailFileWillInstall",
                    READ = detail_file_will_install, CONSTANT)]
        #[qproperty(QString, detail_file_install_text, cxx_name = "detailFileInstallText",
                    READ = detail_file_install_text, CONSTANT)]
        #[qproperty(QString, aur_open_text, cxx_name = "aurOpenText",
                    READ = aur_open_text, CONSTANT)]
        #[qproperty(QString, aur_open_tooltip, cxx_name = "aurOpenTooltip",
                    READ = aur_open_tooltip, CONSTANT)]
        // ── Прослушивание тем звуков (паритет с KDE «Системные звуки») ──
        #[qproperty(QString, sound_preview_label, cxx_name = "soundPreviewLabel",
                    READ = sound_preview_label, CONSTANT)]
        #[qproperty(QString, sound_play_all_tooltip, cxx_name = "soundPlayAllTooltip",
                    READ = sound_play_all_tooltip, CONSTANT)]
        #[qproperty(QString, sound_missing_tooltip, cxx_name = "soundMissingTooltip",
                    READ = sound_missing_tooltip, CONSTANT)]
        #[qproperty(QString, sound_store_preview_text, cxx_name = "soundStorePreviewText",
                    READ = sound_store_preview_text, CONSTANT)]
        #[qproperty(QString, sound_store_preview_tooltip, cxx_name = "soundStorePreviewTooltip",
                    READ = sound_store_preview_tooltip, CONSTANT)]
        // ── «Продемонстрировать» у заставки KDE (паритет с модулем KDE) ──
        #[qproperty(QString, splash_demo_label, cxx_name = "splashDemoLabel",
                    READ = splash_demo_label, CONSTANT)]
        #[qproperty(QString, splash_demo_tooltip, cxx_name = "splashDemoTooltip",
                    READ = splash_demo_tooltip, CONSTANT)]
        /// Тема звуков на диске, которую показывает панель деталей (пусто —
        /// не тема звуков).
        #[qproperty(QString, detail_sound_theme, cxx_name = "detailSoundTheme",
                    READ = detail_sound_theme, NOTIFY = detail_changed)]
        /// Открыт товар Магазина из раздела «Звуки» — его можно послушать
        /// до установки.
        #[qproperty(bool, detail_sound_store, cxx_name = "detailSoundStore",
                    READ = detail_sound_store, NOTIFY = detail_changed)]
        #[qproperty(i32, detail_preview_width, cxx_name = "detailPreviewWidth",
                    READ = detail_preview_width, CONSTANT)]
        #[qproperty(i32, detail_preview_height, cxx_name = "detailPreviewHeight",
                    READ = detail_preview_height, CONSTANT)]
        #[qproperty(bool, detail_is_local, cxx_name = "detailIsLocal",
                    READ = detail_is_local, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_name, cxx_name = "detailName",
                    READ = detail_name, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_meta, cxx_name = "detailMeta",
                    READ = detail_meta, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_stars, cxx_name = "detailStars",
                    READ = detail_stars, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_downloads, cxx_name = "detailDownloads",
                    READ = detail_downloads, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_path, cxx_name = "detailPath",
                    READ = detail_path, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_description, cxx_name = "detailDescription",
                    READ = detail_description, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_install_text, cxx_name = "detailInstallText",
                    READ = detail_install_text, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_install_tooltip, cxx_name = "detailInstallTooltip",
                    READ = detail_install_tooltip, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_install_state, cxx_name = "detailInstallState",
                    READ = detail_install_state, NOTIFY = detail_changed)]
        #[qproperty(bool, detail_uninstall_visible, cxx_name = "detailUninstallVisible",
                    READ = detail_uninstall_visible, NOTIFY = detail_changed)]
        // «Удалить» у темы с диска: недоступна у включённой и системной,
        // причина — подсказкой (`theme_removal`).
        #[qproperty(bool, detail_uninstall_enabled, cxx_name = "detailUninstallEnabled",
                    READ = detail_uninstall_enabled, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_uninstall_tooltip, cxx_name = "detailUninstallTooltip",
                    READ = detail_uninstall_tooltip, NOTIFY = detail_changed)]
        // Всплывающая строка отложенного удаления: «Вернуть» и сколько
        // она висит.
        #[qproperty(QString, removal_undo_text, cxx_name = "removalUndoText",
                    READ = removal_undo_text, CONSTANT)]
        #[qproperty(i32, removal_undo_ms, cxx_name = "removalUndoMs",
                    READ = removal_undo_ms, CONSTANT)]
        #[qproperty(QString, detail_favorite_text, cxx_name = "detailFavoriteText",
                    READ = detail_favorite_text, NOTIFY = detail_changed)]
        #[qproperty(bool, detail_apply_local_enabled, cxx_name = "detailApplyLocalEnabled",
                    READ = detail_apply_local_enabled, NOTIFY = detail_changed)]
        // «Настроить…» у стиля приложений и темы Kvantum (JSON
        // `themes::StyleConfigButton`, без `argv`).
        #[qproperty(QString, detail_style_config, cxx_name = "detailStyleConfig",
                    READ = detail_style_config, NOTIFY = detail_changed)]
        // «Применять к программам GTK 4/libadwaita» у темы GTK (JSON
        // `gtk4_theme::Toggle`).
        #[qproperty(QString, detail_gtk4, cxx_name = "detailGtk4",
                    READ = detail_gtk4, NOTIFY = detail_changed)]
        // «Настроить…» у рамки окна KDE (JSON `kdecoration::ConfigButton`,
        // без `argv`).
        #[qproperty(QString, detail_deco_config, cxx_name = "detailDecoConfig",
                    READ = detail_deco_config, NOTIFY = detail_changed)]
        // «Изменить…» / «Создать на основе…» у цветовой схемы (JSON:
        // `visible` + подписи `color_scheme_edit::EditorTexts`).
        #[qproperty(QString, detail_scheme_edit, cxx_name = "detailSchemeEdit",
                    READ = detail_scheme_edit, NOTIFY = detail_changed)]
        // Переключатель «Запускать при входе» — только у виджетов Conky.
        #[qproperty(bool, detail_autostart_visible, cxx_name = "detailAutostartVisible",
                    READ = detail_autostart_visible, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_autostart_text, cxx_name = "detailAutostartText",
                    READ = detail_autostart_text, CONSTANT)]
        #[qproperty(QString, detail_preview, cxx_name = "detailPreview",
                    READ = detail_preview, NOTIFY = detail_frame_changed)]
        #[qproperty(QString, detail_preview_text, cxx_name = "detailPreviewText",
                    READ = detail_preview_text, NOTIFY = detail_frame_changed)]
        #[qproperty(QString, detail_item_id, cxx_name = "detailItemId",
                    READ = detail_item_id, NOTIFY = detail_changed)]
        // ── Галерея скриншотов ──────────────────────────────────────────
        #[qproperty(QString, gallery_counter, cxx_name = "galleryCounter",
                    READ = gallery_counter, NOTIFY = detail_frame_changed)]
        #[qproperty(bool, gallery_nav_visible, cxx_name = "galleryNavVisible",
                    READ = gallery_nav_visible, NOTIFY = detail_frame_changed)]
        #[qproperty(QString, overlay_zoom_text, cxx_name = "overlayZoomText",
                    READ = overlay_zoom_text, CONSTANT)]
        #[qproperty(QString, overlay_zoom_tooltip, cxx_name = "overlayZoomTooltip",
                    READ = overlay_zoom_tooltip, CONSTANT)]
        #[qproperty(QString, overlay_prev_text, cxx_name = "overlayPrevText",
                    READ = overlay_prev_text, CONSTANT)]
        #[qproperty(QString, overlay_prev_tooltip, cxx_name = "overlayPrevTooltip",
                    READ = overlay_prev_tooltip, CONSTANT)]
        #[qproperty(QString, overlay_next_text, cxx_name = "overlayNextText",
                    READ = overlay_next_text, CONSTANT)]
        #[qproperty(QString, overlay_next_tooltip, cxx_name = "overlayNextTooltip",
                    READ = overlay_next_tooltip, CONSTANT)]
        #[qproperty(i32, overlay_button_size, cxx_name = "overlayButtonSize",
                    READ = overlay_button_size, CONSTANT)]
        #[qproperty(i32, overlay_margin, cxx_name = "overlayMargin",
                    READ = overlay_margin, CONSTANT)]
        // ── Комментарии ─────────────────────────────────────────────────
        #[qproperty(QString, comments_header, cxx_name = "commentsHeader",
                    READ = comments_header, NOTIFY = comments_changed)]
        #[qproperty(QList_QVariant, comment_rows, cxx_name = "commentRows",
                    READ = comment_rows, NOTIFY = comments_changed)]
        #[qproperty(bool, comments_more_visible, cxx_name = "commentsMoreVisible",
                    READ = comments_more_visible, NOTIFY = comments_changed)]
        #[qproperty(bool, comments_empty, cxx_name = "commentsEmpty",
                    READ = comments_empty, NOTIFY = comments_changed)]
        #[qproperty(QString, comments_empty_text, cxx_name = "commentsEmptyText",
                    READ = comments_empty_text, CONSTANT)]
        #[qproperty(QString, comments_more_text, cxx_name = "commentsMoreText",
                    READ = comments_more_text, CONSTANT)]
        #[qproperty(QString, detail_status, cxx_name = "detailStatus",
                    READ = detail_status, NOTIFY = detail_status_changed)]
        /// Идёт долгое действие (установка, сборка, применение): кнопки
        /// запуска заперты.
        #[qproperty(bool, action_busy, cxx_name = "actionBusy",
                    READ = action_busy, NOTIFY = install_changed)]
        /// Видна ли полоса хода: у сборки помощником AUR и у применения темы
        /// доли выполненного нет вовсе (решение ядра).
        #[qproperty(bool, progress_visible, cxx_name = "progressVisible",
                    READ = progress_visible, NOTIFY = install_changed)]
        /// Доля выполненного, 0–100 (значение обрезает ядро).
        #[qproperty(i32, install_percent, cxx_name = "installPercent",
                    READ = install_percent, NOTIFY = install_changed)]
        // ── Загрузчик превью ────────────────────────────────────────────
        /// Есть ли готовое превью, которое ещё не поставлено на плитку.
        /// Пока `true`, страница крутит таймер выдачи: шесть JPEG,
        /// раскодированных за один проход цикла событий, дают заметный рывок.
        #[qproperty(bool, preview_ready_pending, cxx_name = "previewReadyPending",
                    READ = preview_ready_pending, NOTIFY = preview_changed)]
        /// Пауза перед фоновым прогревом галереи показанной сетки.
        #[qproperty(i32, gallery_warm_delay_ms, cxx_name = "galleryWarmDelayMs",
                    READ = gallery_warm_delay_ms, CONSTANT)]
        /// Через сколько после показа вкладки начинается фоновый прогрев
        /// остальных разделов: спорить за сеть с открытым он не должен.
        #[qproperty(i32, prefetch_start_ms, cxx_name = "prefetchStartMs",
                    READ = prefetch_start_ms, CONSTANT)]
        /// Пауза между фоновыми прогревами — не долбить магазин.
        #[qproperty(i32, prefetch_pause_ms, cxx_name = "prefetchPauseMs",
                    READ = prefetch_pause_ms, CONSTANT)]
        /// Через сколько после показа вкладки разово чистятся протухшие
        /// переводы: работа не горячая и ждать её незачем.
        #[qproperty(i32, purge_translations_ms, cxx_name = "purgeTranslationsMs",
                    READ = purge_translations_ms, CONSTANT)]
        /// Сколько ждать, пока осядет смена стиля Qt после применения темы.
        #[qproperty(i32, style_storm_ms, cxx_name = "styleStormMs",
                    READ = style_storm_ms, CONSTANT)]
        /// Сетку пора пересобрать — но только когда шторм стиля осядет.
        #[qproperty(bool, reload_pending, cxx_name = "reloadPending",
                    READ = reload_pending, NOTIFY = install_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        // ── Карточка пакета AUR ──
        // Откуда открытый товар: `store` | `local` | `aur`.
        #[qproperty(QString, detail_source, cxx_name = "detailSource",
                    READ = detail_source, NOTIFY = detail_changed)]
        // Сведения о пакете: [{label, value, html, warn}].
        #[qproperty(QList_QVariant, aur_facts, cxx_name = "aurFacts",
                    READ = aur_facts, NOTIFY = detail_changed)]
        // Страницу пакета не дали — комментарии только на сайте.
        #[qproperty(bool, comments_blocked, cxx_name = "commentsBlocked",
                    READ = comments_blocked, NOTIFY = comments_changed)]
        #[qproperty(QString, comments_blocked_text, cxx_name = "commentsBlockedText",
                    READ = comments_blocked_text, CONSTANT)]
        // «📄 Подробнее» на плитке пакета.
        #[qproperty(QString, aur_details_text, cxx_name = "aurDetailsText",
                    READ = aur_details_text, CONSTANT)]
        #[qproperty(QString, aur_details_tooltip, cxx_name = "aurDetailsTooltip",
                    READ = aur_details_tooltip, CONSTANT)]
        // Проверка источника пакета AUR (`aur_trust`): признаки риска
        // [{text, why, danger}], итог словами, значок у «Установить».
        #[qproperty(QList_QVariant, aur_trust_rows, cxx_name = "aurTrustRows",
                    READ = aur_trust_rows, NOTIFY = detail_changed)]
        #[qproperty(QString, aur_trust_status, cxx_name = "aurTrustStatus",
                    READ = aur_trust_status, NOTIFY = detail_changed)]
        #[qproperty(QString, aur_trust_level, cxx_name = "aurTrustLevel",
                    READ = aur_trust_level, NOTIFY = detail_changed)]
        #[qproperty(QString, aur_trust_badge, cxx_name = "aurTrustBadge",
                    READ = aur_trust_badge, NOTIFY = detail_changed)]
        #[qproperty(QString, aur_trust_tooltip, cxx_name = "aurTrustTooltip",
                    READ = aur_trust_tooltip, NOTIFY = detail_changed)]
        #[qproperty(QString, aur_pkgbuild_title, cxx_name = "aurPkgbuildTitle",
                    READ = aur_pkgbuild_title, NOTIFY = detail_changed)]
        #[qproperty(QString, aur_package_base, cxx_name = "aurPackageBase",
                    READ = aur_package_base, NOTIFY = detail_changed)]
        #[qproperty(QString, aur_trust_title, cxx_name = "aurTrustTitle",
                    READ = aur_trust_title, CONSTANT)]
        #[qproperty(QString, aur_trust_note, cxx_name = "aurTrustNote",
                    READ = aur_trust_note, CONSTANT)]
        #[qproperty(QString, aur_trust_danger_color, cxx_name = "aurTrustDangerColor",
                    READ = aur_trust_danger_color, CONSTANT)]
        #[qproperty(QString, aur_pkgbuild_btn, cxx_name = "aurPkgbuildBtn",
                    READ = aur_pkgbuild_btn, CONSTANT)]
        #[qproperty(QString, aur_pkgbuild_btn_tooltip, cxx_name = "aurPkgbuildBtnTooltip",
                    READ = aur_pkgbuild_btn_tooltip, CONSTANT)]
        #[qproperty(QString, aur_pkgbuild_loading, cxx_name = "aurPkgbuildLoading",
                    READ = aur_pkgbuild_loading, CONSTANT)]
        type AppearanceModel = super::AppearanceModelRust;
    }

    // ── Сигналы ─────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        /// Скин сменили на лету — палитру пересчитать. Толчок приходит из
        /// общего реестра (`crate::skin`), а не отсюда.
        #[qsignal]
        #[cxx_name = "paletteChanged"]
        fn palette_changed(self: Pin<&mut AppearanceModel>);

        #[qsignal]
        #[cxx_name = "sidebarChanged"]
        fn sidebar_changed(self: Pin<&mut AppearanceModel>);

        #[qsignal]
        #[cxx_name = "chromeChanged"]
        fn chrome_changed(self: Pin<&mut AppearanceModel>);

        #[qsignal]
        #[cxx_name = "deChanged"]
        fn de_changed(self: Pin<&mut AppearanceModel>);

        #[qsignal]
        #[cxx_name = "screenChanged"]
        fn screen_changed(self: Pin<&mut AppearanceModel>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut AppearanceModel>);

        #[qsignal]
        #[cxx_name = "pageChanged"]
        fn page_changed(self: Pin<&mut AppearanceModel>);

        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut AppearanceModel>);

        /// Состав запроса изменился — странице пора спросить хозяина.
        /// Отдельный сигнал, а не «сходи сам»: решение о том, ЧТО просить,
        /// принимает ядро, а ходить в сеть умеет только хозяин.
        #[qsignal]
        #[cxx_name = "catalogWanted"]
        fn catalog_wanted(self: Pin<&mut AppearanceModel>);

        /// Панель деталей показывает другой товар.
        #[qsignal]
        #[cxx_name = "detailChanged"]
        fn detail_changed(self: Pin<&mut AppearanceModel>);

        /// Сменился кадр на месте картинки деталей.
        #[qsignal]
        #[cxx_name = "detailFrameChanged"]
        fn detail_frame_changed(self: Pin<&mut AppearanceModel>);

        /// Итог установки/применения под кнопками деталей.
        #[qsignal]
        #[cxx_name = "detailStatusChanged"]
        fn detail_status_changed(self: Pin<&mut AppearanceModel>);

        /// Ход долгого действия изменился (началось, подвинулось, кончилось).
        #[qsignal]
        #[cxx_name = "installChanged"]
        fn install_changed(self: Pin<&mut AppearanceModel>);

        /// Товару нужны полные детали (описание, все скриншоты).
        #[qsignal]
        #[cxx_name = "detailWanted"]
        fn detail_wanted(self: Pin<&mut AppearanceModel>, item_id: QString);

        /// Нужен кадр галереи: номер и адрес картинки (обе строкой —
        /// число в подписи метаобъекта cxx-qt снаружи не разбирается).
        #[qsignal]
        #[cxx_name = "galleryWanted"]
        fn gallery_wanted(self: Pin<&mut AppearanceModel>, index: QString, url: QString);

        /// Нужна страница комментариев товара.
        #[qsignal]
        #[cxx_name = "commentsWanted"]
        fn comments_wanted(self: Pin<&mut AppearanceModel>, item_id: QString, page: QString);

        #[qsignal]
        #[cxx_name = "commentsChanged"]
        fn comments_changed(self: Pin<&mut AppearanceModel>);

        #[qsignal]
        #[cxx_name = "suggestChanged"]
        fn suggest_changed(self: Pin<&mut AppearanceModel>);

        /// Подсказкам нужен свежий ответ магазина (JSON-строка запроса).
        #[qsignal]
        #[cxx_name = "suggestWanted"]
        fn suggest_wanted(self: Pin<&mut AppearanceModel>, request: QString);

        /// Нужна картинка плитки либо фоновый прогрев кадра галереи.
        ///
        /// `job` — номер заявки: его хозяин ничего не разбирает, а лишь
        /// возвращает обратно вместе с ответом. Иначе две плитки, делящие
        /// один адрес, и живая заявка поверх фоновой были бы неразличимы.
        /// `low` — фоновая заявка: показывать её нечему, она лишь наполняет
        /// кеш и уступает живым плиткам.
        #[qsignal]
        #[cxx_name = "previewWanted"]
        fn preview_wanted(
            self: Pin<&mut AppearanceModel>,
            job: QString,
            item_id: QString,
            url: QString,
            low: bool,
        );

        /// Сбой был временным — повторить заявку через `delay_ms`.
        /// `generation` несёт поколение очереди: смена раздела его меняет, и
        /// повтор прошлого раздела молча выбрасывается.
        #[qsignal]
        #[cxx_name = "previewRetryWanted"]
        fn preview_retry_wanted(
            self: Pin<&mut AppearanceModel>,
            generation: QString,
            job: QString,
            delay_ms: QString,
        );

        /// Нужен фоновый прогрев: состав запроса строкой JSON. Прогрев не
        /// показывается никому — он лишь наполняет кеш, чтобы соседний
        /// раздел и следующая страница открылись без сети.
        #[qsignal]
        #[cxx_name = "prefetchWanted"]
        fn prefetch_wanted(self: Pin<&mut AppearanceModel>, request: QString);

        /// Раздел сменили — всё летящее за превью можно обрывать.
        #[qsignal]
        #[cxx_name = "previewsCanceled"]
        fn previews_canceled(self: Pin<&mut AppearanceModel>);

        /// Состав готовых превью изменился (таймер выдачи держат по нему).
        #[qsignal]
        #[cxx_name = "previewChanged"]
        fn preview_changed(self: Pin<&mut AppearanceModel>);

        /// dataChanged базового класса — точечное обновление строки.
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut AppearanceModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );
    }

    // index() безопасен, но cxx-qt требует держать #[inherit] в unsafe-блоке
    // (unsafe тут про соответствие сигнатуре C++, а не про опасность).
    unsafe extern "RustQt" {
        #[inherit]
        fn index(
            self: &AppearanceModel,
            row: i32,
            column: i32,
            parent: &QModelIndex,
        ) -> QModelIndex;
    }

    // ── Наследуемые методы базового класса ──────────────────────────────
    extern "RustQt" {
        /// # Safety
        /// Требует парного end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut AppearanceModel>);
        /// # Safety
        /// Парный к begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut AppearanceModel>);
    }

    // ── Переопределения QAbstractListModel ──────────────────────────────
    // Без #[qinvokable]: инвокабл в наследнике затеняет укороченные клоны
    // сигнатур базы, и rowCount() без аргументов из JS падает.
    extern "RustQt" {
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &AppearanceModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &AppearanceModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &AppearanceModel, index: &QModelIndex, role: i32) -> QVariant;
    }

    // ── Чтение свойств ──────────────────────────────────────────────────
    extern "RustQt" {
        #[cxx_name = "windowTitle"]
        fn window_title(self: &AppearanceModel) -> QString;
        #[cxx_name = "windowMinWidth"]
        fn window_min_width(self: &AppearanceModel) -> i32;
        #[cxx_name = "windowMinHeight"]
        fn window_min_height(self: &AppearanceModel) -> i32;
        #[cxx_name = "headerTitle"]
        fn header_title(self: &AppearanceModel) -> QString;
        #[cxx_name = "headerTitlePointSize"]
        fn header_title_point_size(self: &AppearanceModel) -> i32;
        #[cxx_name = "sidebarWidth"]
        fn sidebar_width(self: &AppearanceModel) -> i32;
        #[cxx_name = "sidebarRowHeight"]
        fn sidebar_row_height(self: &AppearanceModel) -> i32;
        #[cxx_name = "searchWidth"]
        fn search_width(self: &AppearanceModel) -> i32;
        #[cxx_name = "cardWidth"]
        fn card_width(self: &AppearanceModel) -> i32;
        #[cxx_name = "cardHeight"]
        fn card_height(self: &AppearanceModel) -> i32;
        #[cxx_name = "cardPreviewHeight"]
        fn card_preview_height(self: &AppearanceModel) -> i32;
        #[cxx_name = "cardTextWidth"]
        fn card_text_width(self: &AppearanceModel) -> i32;
        #[cxx_name = "cardBodyMargin"]
        fn card_body_margin(self: &AppearanceModel) -> i32;
        #[cxx_name = "cardBodyTopMargin"]
        fn card_body_top_margin(self: &AppearanceModel) -> i32;
        #[cxx_name = "cardBodyBottomMargin"]
        fn card_body_bottom_margin(self: &AppearanceModel) -> i32;
        #[cxx_name = "cardBodySpacing"]
        fn card_body_spacing(self: &AppearanceModel) -> i32;
        #[cxx_name = "cardButtonHeight"]
        fn card_button_height(self: &AppearanceModel) -> i32;
        #[cxx_name = "cardButtonTopMargin"]
        fn card_button_top_margin(self: &AppearanceModel) -> i32;
        #[cxx_name = "cardNamePointSize"]
        fn card_name_point_size(self: &AppearanceModel) -> i32;
        #[cxx_name = "cardMetaPointSize"]
        fn card_meta_point_size(self: &AppearanceModel) -> i32;
        #[cxx_name = "gridSpacing"]
        fn grid_spacing(self: &AppearanceModel) -> i32;
        #[cxx_name = "gridMarginH"]
        fn grid_margin_h(self: &AppearanceModel) -> i32;
        #[cxx_name = "gridMarginV"]
        fn grid_margin_v(self: &AppearanceModel) -> i32;
        #[cxx_name = "headerMarginH"]
        fn header_margin_h(self: &AppearanceModel) -> i32;
        #[cxx_name = "headerMarginV"]
        fn header_margin_v(self: &AppearanceModel) -> i32;
        #[cxx_name = "previewReadyPending"]
        fn preview_ready_pending(self: &AppearanceModel) -> bool;
        #[cxx_name = "galleryWarmDelayMs"]
        fn gallery_warm_delay_ms(self: &AppearanceModel) -> i32;
        #[cxx_name = "prefetchStartMs"]
        fn prefetch_start_ms(self: &AppearanceModel) -> i32;
        #[cxx_name = "prefetchPauseMs"]
        fn prefetch_pause_ms(self: &AppearanceModel) -> i32;
        #[cxx_name = "purgeTranslationsMs"]
        fn purge_translations_ms(self: &AppearanceModel) -> i32;
        #[cxx_name = "styleStormMs"]
        fn style_storm_ms(self: &AppearanceModel) -> i32;
        #[cxx_name = "reloadPending"]
        fn reload_pending(self: &AppearanceModel) -> bool;
        #[cxx_name = "catalogDebounceMs"]
        fn catalog_debounce_ms(self: &AppearanceModel) -> i32;
        #[cxx_name = "suggestDebounceMs"]
        fn suggest_debounce_ms(self: &AppearanceModel) -> i32;
        #[cxx_name = "suggestRowHeight"]
        fn suggest_row_height(self: &AppearanceModel) -> i32;
        #[cxx_name = "suggestThumbWidth"]
        fn suggest_thumb_width(self: &AppearanceModel) -> i32;
        #[cxx_name = "suggestThumbHeight"]
        fn suggest_thumb_height(self: &AppearanceModel) -> i32;
        #[cxx_name = "suggestRows"]
        fn suggest_rows(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "suggestVisible"]
        fn suggest_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "suggestIndex"]
        fn suggest_index(self: &AppearanceModel) -> i32;
        #[cxx_name = "firstLoadMs"]
        fn first_load_ms(self: &AppearanceModel) -> i32;
        #[cxx_name = "sourceLabel"]
        fn source_label(self: &AppearanceModel) -> QString;
        #[cxx_name = "submodeLabel"]
        fn submode_label(self: &AppearanceModel) -> QString;
        #[cxx_name = "cursorSizeLabel"]
        fn cursor_size_label(self: &AppearanceModel) -> QString;
        #[cxx_name = "deLabel"]
        fn de_label(self: &AppearanceModel) -> QString;
        #[cxx_name = "deTooltip"]
        fn de_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "refreshText"]
        fn refresh_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "refreshTooltip"]
        fn refresh_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "pagePrevText"]
        fn page_prev_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "pageNextText"]
        fn page_next_text(self: &AppearanceModel) -> QString;
        fn sorts(self: &AppearanceModel) -> QList_QVariant;
        fn palette(self: &AppearanceModel) -> QMap_QString_QVariant;
        #[cxx_name = "sectionStyles"]
        fn section_styles(self: &AppearanceModel) -> QMap_QString_QVariant;
        #[cxx_name = "sidebarRows"]
        fn sidebar_rows(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "sectionTitle"]
        fn section_title(self: &AppearanceModel) -> QString;
        #[cxx_name = "searchVisible"]
        fn search_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "searchPlaceholder"]
        fn search_placeholder(self: &AppearanceModel) -> QString;
        #[cxx_name = "searchDelayMs"]
        fn search_delay_ms(self: &AppearanceModel) -> i32;
        #[cxx_name = "searchClearTooltip"]
        fn search_clear_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "caveatText"]
        fn caveat_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "sourceBarVisible"]
        fn source_bar_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "styleBarVisible"]
        fn style_bar_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "lockBarVisible"]
        fn lock_bar_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "toneBarVisible"]
        fn tone_bar_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "cursorSizeBarVisible"]
        fn cursor_size_bar_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "sortVisible"]
        fn sort_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "paginationVisible"]
        fn pagination_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "installFileVisible"]
        fn install_file_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "installFileText"]
        fn install_file_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "installFileTooltip"]
        fn install_file_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "installFileFilesText"]
        fn install_file_files_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "installFileFolderText"]
        fn install_file_folder_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "installFileTitle"]
        fn install_file_title(self: &AppearanceModel) -> QString;
        #[cxx_name = "installedView"]
        fn installed_view(self: &AppearanceModel) -> bool;
        #[cxx_name = "markCurrent"]
        fn mark_current(self: &AppearanceModel) -> bool;

        #[cxx_name = "installFileFilters"]
        fn install_file_filters(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "installFileSection"]
        fn install_file_section(self: &AppearanceModel) -> QString;
        #[cxx_name = "sourceChips"]
        fn source_chips(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "styleChips"]
        fn style_chips(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "lockChips"]
        fn lock_chips(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "toneChips"]
        fn tone_chips(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "cursorSizeChips"]
        fn cursor_size_chips(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "sortKey"]
        fn sort_key(self: &AppearanceModel) -> QString;
        fn query(self: &AppearanceModel) -> QString;
        #[cxx_name = "deChoices"]
        fn de_choices(self: &AppearanceModel) -> QList_QVariant;
        fn de(self: &AppearanceModel) -> QString;
        fn screen(self: &AppearanceModel) -> QString;
        #[cxx_name = "noteText"]
        fn note_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "plmTitle"]
        fn plm_title(self: &AppearanceModel) -> QString;
        #[cxx_name = "plmBody"]
        fn plm_body(self: &AppearanceModel) -> QString;
        #[cxx_name = "plmOpenLabel"]
        fn plm_open_label(self: &AppearanceModel) -> QString;
        #[cxx_name = "plmOpenVisible"]
        fn plm_open_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "plmStoreHint"]
        fn plm_store_hint(self: &AppearanceModel) -> QString;
        #[cxx_name = "loginSection"]
        fn login_section(self: &AppearanceModel) -> bool;
        #[cxx_name = "statusText"]
        fn status_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "pageLabel"]
        fn page_label(self: &AppearanceModel) -> QString;
        #[cxx_name = "prevEnabled"]
        fn prev_enabled(self: &AppearanceModel) -> bool;
        #[cxx_name = "nextEnabled"]
        fn next_enabled(self: &AppearanceModel) -> bool;
        fn count(self: &AppearanceModel) -> i32;
        #[cxx_name = "detailBackText"]
        fn detail_back_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailUninstallText"]
        fn detail_uninstall_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailSiteText"]
        fn detail_site_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailSiteTooltip"]
        fn detail_site_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailCopyLinkText"]
        fn detail_copy_link_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailCopyLinkTooltip"]
        fn detail_copy_link_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailApplyLocalText"]
        fn detail_apply_local_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailPreviewTooltip"]
        fn detail_preview_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailFiles"]
        fn detail_files(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailFilesTitle"]
        fn detail_files_title(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailFilesHint"]
        fn detail_files_hint(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailFileWillInstall"]
        fn detail_file_will_install(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailFileInstallText"]
        fn detail_file_install_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurOpenText"]
        fn aur_open_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurOpenTooltip"]
        fn aur_open_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "soundPreviewLabel"]
        fn sound_preview_label(self: &AppearanceModel) -> QString;
        #[cxx_name = "soundPlayAllTooltip"]
        fn sound_play_all_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "soundMissingTooltip"]
        fn sound_missing_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "soundStorePreviewText"]
        fn sound_store_preview_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "soundStorePreviewTooltip"]
        fn sound_store_preview_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "splashDemoLabel"]
        fn splash_demo_label(self: &AppearanceModel) -> QString;
        #[cxx_name = "splashDemoTooltip"]
        fn splash_demo_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailSoundTheme"]
        fn detail_sound_theme(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailSoundStore"]
        fn detail_sound_store(self: &AppearanceModel) -> bool;
        #[cxx_name = "detailPreviewWidth"]
        fn detail_preview_width(self: &AppearanceModel) -> i32;
        #[cxx_name = "detailPreviewHeight"]
        fn detail_preview_height(self: &AppearanceModel) -> i32;
        #[cxx_name = "detailIsLocal"]
        fn detail_is_local(self: &AppearanceModel) -> bool;
        #[cxx_name = "detailName"]
        fn detail_name(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailMeta"]
        fn detail_meta(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailStars"]
        fn detail_stars(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailDownloads"]
        fn detail_downloads(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailPath"]
        fn detail_path(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailDescription"]
        fn detail_description(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailInstallText"]
        fn detail_install_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailInstallTooltip"]
        fn detail_install_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailInstallState"]
        fn detail_install_state(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailUninstallVisible"]
        fn detail_uninstall_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "detailUninstallEnabled"]
        fn detail_uninstall_enabled(self: &AppearanceModel) -> bool;
        #[cxx_name = "detailUninstallTooltip"]
        fn detail_uninstall_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "removalUndoText"]
        fn removal_undo_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "removalUndoMs"]
        fn removal_undo_ms(self: &AppearanceModel) -> i32;
        #[cxx_name = "detailFavoriteText"]
        fn detail_favorite_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailApplyLocalEnabled"]
        fn detail_apply_local_enabled(self: &AppearanceModel) -> bool;
        #[cxx_name = "detailStyleConfig"]
        fn detail_style_config(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailGtk4"]
        fn detail_gtk4(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailDecoConfig"]
        fn detail_deco_config(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailSchemeEdit"]
        fn detail_scheme_edit(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailAutostartVisible"]
        fn detail_autostart_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "detailAutostartText"]
        fn detail_autostart_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailPreview"]
        fn detail_preview(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailPreviewText"]
        fn detail_preview_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailItemId"]
        fn detail_item_id(self: &AppearanceModel) -> QString;
        #[cxx_name = "galleryCounter"]
        fn gallery_counter(self: &AppearanceModel) -> QString;
        #[cxx_name = "galleryNavVisible"]
        fn gallery_nav_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "overlayZoomText"]
        fn overlay_zoom_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "overlayZoomTooltip"]
        fn overlay_zoom_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "overlayPrevText"]
        fn overlay_prev_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "overlayPrevTooltip"]
        fn overlay_prev_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "overlayNextText"]
        fn overlay_next_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "overlayNextTooltip"]
        fn overlay_next_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "overlayButtonSize"]
        fn overlay_button_size(self: &AppearanceModel) -> i32;
        #[cxx_name = "overlayMargin"]
        fn overlay_margin(self: &AppearanceModel) -> i32;
        #[cxx_name = "commentsHeader"]
        fn comments_header(self: &AppearanceModel) -> QString;
        #[cxx_name = "commentRows"]
        fn comment_rows(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "commentsMoreVisible"]
        fn comments_more_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "commentsEmpty"]
        fn comments_empty(self: &AppearanceModel) -> bool;
        #[cxx_name = "commentsEmptyText"]
        fn comments_empty_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "commentsMoreText"]
        fn comments_more_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailStatus"]
        fn detail_status(self: &AppearanceModel) -> QString;
        #[cxx_name = "actionBusy"]
        fn action_busy(self: &AppearanceModel) -> bool;
        #[cxx_name = "progressVisible"]
        fn progress_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "installPercent"]
        fn install_percent(self: &AppearanceModel) -> i32;
        #[cxx_name = "detailSource"]
        fn detail_source(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurFacts"]
        fn aur_facts(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "commentsBlocked"]
        fn comments_blocked(self: &AppearanceModel) -> bool;
        #[cxx_name = "commentsBlockedText"]
        fn comments_blocked_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurDetailsText"]
        fn aur_details_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurDetailsTooltip"]
        fn aur_details_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurTrustRows"]
        fn aur_trust_rows(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "aurTrustStatus"]
        fn aur_trust_status(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurTrustLevel"]
        fn aur_trust_level(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurTrustBadge"]
        fn aur_trust_badge(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurTrustTooltip"]
        fn aur_trust_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurPkgbuildTitle"]
        fn aur_pkgbuild_title(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurPackageBase"]
        fn aur_package_base(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurTrustTitle"]
        fn aur_trust_title(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurTrustNote"]
        fn aur_trust_note(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurTrustDangerColor"]
        fn aur_trust_danger_color(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurPkgbuildBtn"]
        fn aur_pkgbuild_btn(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurPkgbuildBtnTooltip"]
        fn aur_pkgbuild_btn_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "aurPkgbuildLoading"]
        fn aur_pkgbuild_loading(self: &AppearanceModel) -> QString;
    }

    // ── API для QML и хозяина страницы ──────────────────────────────────
    //
    // ⚠️ Ни одного инвокабла с числом в подписи: cxx-qt пишет в метаобъект
    // тип как `::std::int32_t`, метатипа с таким именем нет, и вызов через
    // `QMetaObject.invokeMethod` молча возвращает False (грабля 12 схемы).
    // Там, где нужно число, оно едет строкой.
    extern "RustQt" {
        /// Факты о машине, которых ядро само не добудет: список Qt-стилей,
        /// действующий стиль виджетов, запомненный выбор рабочего стола.
        /// Зовёт хозяин страницы один раз при старте (JSON-строка).
        #[qinvokable]
        #[cxx_name = "setEnvironment"]
        fn set_environment(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Что рисует экран входа и не SteamOS ли это — ответ фоновой пробы
        /// объекта действий (`loginScreenReady`, JSON `{plasmaLogin, steamos,
        /// canOpen}`). Пересобирает боковую панель: при Plasma Login Manager
        /// на месте SDDM-полки встаёт его раздел.
        #[qinvokable]
        #[cxx_name = "setLoginScreen"]
        fn set_login_screen(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Открыть настройки экрана входа не вышло — сказать это строкой
        /// состояния и назвать, куда идти руками.
        #[qinvokable]
        #[cxx_name = "loginSettingsFailed"]
        fn login_settings_failed(self: Pin<&mut AppearanceModel>);

        /// Полоса «Системные звуки выключены» раздела «Звуки»: JSON
        /// `ap::EventSoundsBanner`. Читает выключатель запущенного
        /// окружения; выбран в каталоге другой рабочий стол — полосы нет
        /// (его ключ отсюда не включить). Синхронно: одна короткая команда.
        #[qinvokable]
        #[cxx_name = "eventSoundsBanner"]
        fn event_sounds_banner(self: &AppearanceModel) -> QString;

        /// «Включить» на полосе: тот же путь записи, что у выключателя во
        /// вкладке «Настройки». Итог — строкой для человека.
        #[qinvokable]
        #[cxx_name = "enableEventSounds"]
        fn enable_event_sounds(self: &AppearanceModel) -> QString;

        /// Полоса «Вернуть как было» открытого раздела: JSON
        /// `ap::AppliedRevertBanner` (запись «Применить» общего журнала, у
        /// цветов — ещё памятка слоя и кнопка полосы шелла). Синхронно: два
        /// файла и проба шелла, как у `shellPalette`.
        #[qinvokable]
        #[cxx_name = "appliedRevertBanner"]
        fn applied_revert_banner(self: &AppearanceModel) -> QString;

        /// Открыть раздел боковой панели.
        #[qinvokable]
        #[cxx_name = "selectCategory"]
        fn select_category(self: Pin<&mut AppearanceModel>, key: &QString);

        /// Магазин: `ocs` | `aur` | `both`.
        #[qinvokable]
        #[cxx_name = "setSource"]
        fn set_source(self: Pin<&mut AppearanceModel>, key: &QString);

        /// Режим раздела «Экран блокировки».
        #[qinvokable]
        #[cxx_name = "setLockMode"]
        fn set_lock_mode(self: Pin<&mut AppearanceModel>, key: &QString);

        /// Под-выбор раздела «Стиль приложений» (`kvantum` либо `qt:<Имя>`).
        #[qinvokable]
        #[cxx_name = "setStyleSub"]
        fn set_style_sub(self: Pin<&mut AppearanceModel>, key: &QString);

        /// Фильтр тона у цветовых схем и стилей Plasma (`lp::TONE_*`).
        #[qinvokable]
        #[cxx_name = "setToneFilter"]
        fn set_tone_filter(self: Pin<&mut AppearanceModel>, key: &QString);

        /// Сортировка каталога.
        #[qinvokable]
        #[cxx_name = "setSort"]
        fn set_sort(self: Pin<&mut AppearanceModel>, key: &QString);

        /// Рабочий стол, под который отфильтрован каталог.
        #[qinvokable]
        #[cxx_name = "setDe"]
        fn set_de(self: Pin<&mut AppearanceModel>, de_id: &QString);

        /// Текст строки поиска. Пустая строка СРАЗУ сбрасывает запрос —
        /// починка оригинала жила в `_on_search_text_changed`.
        #[qinvokable]
        #[cxx_name = "setQuery"]
        fn set_query(self: Pin<&mut AppearanceModel>, text: &QString);

        /// Применить набранный запрос (Enter в строке поиска).
        #[qinvokable]
        #[cxx_name = "submitQuery"]
        fn submit_query(self: Pin<&mut AppearanceModel>);

        /// Перечитать каталог заново.
        #[qinvokable]
        fn refresh(self: Pin<&mut AppearanceModel>);

        /// Чип «Запущенные» раздела «Виджеты Conky»: в сетке только
        /// запущенные виджеты (магазин под ними прячется).
        #[qinvokable]
        #[cxx_name = "setRunningOnly"]
        fn set_running_only(self: Pin<&mut AppearanceModel>, on: bool);

        /// После «Установить из файла…»: перечитать установленное мимо кеша.
        #[qinvokable]
        #[cxx_name = "showInstalledFresh"]
        fn show_installed_fresh(self: Pin<&mut AppearanceModel>);

        /// Подпись обратного отсчёта строки «Вернуть»: через сколько секунд
        /// помеченное будет удалено (`theme_removal`).
        #[qinvokable]
        #[cxx_name = "removalCountdownText"]
        fn removal_countdown_text(self: &AppearanceModel, seconds: i32) -> QString;

        /// Листание страниц.
        #[qinvokable]
        #[cxx_name = "prevPage"]
        fn prev_page(self: Pin<&mut AppearanceModel>);
        #[qinvokable]
        #[cxx_name = "nextPage"]
        fn next_page(self: Pin<&mut AppearanceModel>);

        /// Что именно надо принести хозяину: JSON-строка запроса. Пустая
        /// строка — просить нечего (раздел не для этого стола либо запрос
        /// уже летит).
        #[qinvokable]
        #[cxx_name = "catalogRequest"]
        fn catalog_request(self: Pin<&mut AppearanceModel>) -> QString;

        /// Ответ на запрос каталога (JSON-строка). Ответ с чужим поколением
        /// молча отбрасывается — человек уже ушёл в другой раздел.
        #[qinvokable]
        #[cxx_name = "catalogAnswer"]
        fn catalog_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Готовое превью плитки: `id` товара и путь на диске.
        #[qinvokable]
        #[cxx_name = "setPreview"]
        fn set_preview(self: Pin<&mut AppearanceModel>, item_id: &QString, path: &QString);

        /// Картинка заявки `job` записана хозяином в кеш по пути `path`.
        #[qinvokable]
        #[cxx_name = "previewDone"]
        fn preview_done(self: Pin<&mut AppearanceModel>, job: &QString, path: &QString);

        /// Заявка `job` не удалась. `status` — ответ сервера числом строкой
        /// (пусто, если ответа не было вовсе; `canceled` — заявку оборвали мы
        /// сами, такую не повторяют). Решение «повторить или сдаться»
        /// принимает ядро.
        #[qinvokable]
        #[cxx_name = "previewFailed"]
        fn preview_failed(self: Pin<&mut AppearanceModel>, job: &QString, status: &QString);

        /// Пауза перед повтором вышла — вернуть заявку в очередь, если
        /// раздел с тех пор не сменили.
        #[qinvokable]
        #[cxx_name = "previewRetry"]
        fn preview_retry(self: Pin<&mut AppearanceModel>, generation: &QString, job: &QString);

        /// Поставить на плитку ОДНО готовое превью. По одному за проход
        /// цикла событий: раскодировать шесть JPEG разом — заметный рывок.
        #[qinvokable]
        #[cxx_name = "drainPreview"]
        fn drain_preview(self: Pin<&mut AppearanceModel>);

        /// Фоновый прогрев первого кадра галереи показанных плиток. Ушли с
        /// раздела (поколение сменилось) — прогрев просто не начинается.
        #[qinvokable]
        #[cxx_name = "warmGallery"]
        fn warm_gallery(self: Pin<&mut AppearanceModel>, generation: &QString);

        /// Завести очередь фонового прогрева: первые страницы всех сетевых
        /// разделов сверху вниз (состав выбирает ядро).
        #[qinvokable]
        #[cxx_name = "startCategoryPrefetch"]
        fn start_category_prefetch(self: Pin<&mut AppearanceModel>);

        /// Запустить следующий прогрев очереди — по одному за раз.
        #[qinvokable]
        #[cxx_name = "pumpPrefetch"]
        fn pump_prefetch(self: Pin<&mut AppearanceModel>);

        /// Ответ на фоновый прогрев (JSON-строка): отметить цель сделанной и
        /// низким приоритетом дотянуть картинки её карточек.
        #[qinvokable]
        #[cxx_name = "prefetchAnswer"]
        fn prefetch_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Шторм стиля осел — пересобрать сетку. Обход кеша сканов диска
        /// уходит вместе со следующим запросом: тема применена только что, и
        /// старый скан показал бы «✅ Активна» на прежней.
        #[qinvokable]
        #[cxx_name = "reloadAfterStyleStorm"]
        fn reload_after_style_storm(self: Pin<&mut AppearanceModel>);

        /// Вкладку закрывают: новых прогревов больше не выдаём.
        #[qinvokable]
        #[cxx_name = "closePage"]
        fn close_page(self: Pin<&mut AppearanceModel>);

        /// Поколение показанного каталога — им страница метит отложенные
        /// работы, чтобы не разбудить их уже в чужом разделе.
        #[qinvokable]
        #[cxx_name = "catalogGeneration"]
        fn catalog_generation(self: &AppearanceModel) -> QString;

        /// Состав избранного (список идентификаторов) — приходит от хозяина.
        #[qinvokable]
        #[cxx_name = "setFavorites"]
        fn set_favorites(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Строка состояния (сообщения побочных действий).
        #[qinvokable]
        #[cxx_name = "setStatus"]
        fn set_status_text(self: Pin<&mut AppearanceModel>, text: &QString);

        /// Открыть панель деталей товара сетки.
        #[qinvokable]
        #[cxx_name = "openDetail"]
        fn open_detail(self: Pin<&mut AppearanceModel>, item_id: &QString);

        /// Вернуться к сетке.
        #[qinvokable]
        #[cxx_name = "closeDetail"]
        fn close_detail(self: Pin<&mut AppearanceModel>);

        /// Полные детали товара (JSON-строка): описание, все скриншоты.
        #[qinvokable]
        #[cxx_name = "detailAnswer"]
        fn detail_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Переведённое описание вдогонку уже показанной карточке (JSON-строка:
        /// `itemId` и `translated`). Второй проход `loadDetail`: карточка к
        /// этому времени на экране, страница только дополняет её и ставит
        /// ссылку «показать оригинал».
        #[qinvokable]
        #[cxx_name = "descriptionAnswer"]
        fn description_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Переключить «перевод ⇄ оригинал» под описанием.
        #[qinvokable]
        #[cxx_name = "toggleDescription"]
        fn toggle_description(self: Pin<&mut AppearanceModel>);

        /// Долгое действие над товаром началось: запереть кнопки запуска,
        /// написать первую строку журнала и — если у этого вида действия
        /// есть доля выполненного — показать полосу хода. `kind` берётся из
        /// ядра: `install` | `aur` | `apply`.
        #[qinvokable]
        #[cxx_name = "actionBegin"]
        fn action_begin(self: Pin<&mut AppearanceModel>, item_id: &QString, kind: &QString);

        /// «Установить из файла…» началась: то же, что [`action_begin`],
        /// но имя — выбранного (`paths_json` — массив адресов окна выбора),
        /// плитки у него нет.
        #[qinvokable]
        #[cxx_name = "installFileBegin"]
        fn install_file_begin(self: Pin<&mut AppearanceModel>, paths_json: &QString);

        /// Очередь установок AUR изменилась (строка JSON
        /// `aur_install::AurQueue::snapshot_json`): стадии на плитках AUR
        /// всех разделов и строка состояния с именем пакета.
        #[qinvokable]
        #[cxx_name = "setAurJobs"]
        fn set_aur_jobs(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Ход действия подвинулся: доля выполненного, 0–100.
        #[qinvokable]
        #[cxx_name = "setInstallPercent"]
        fn set_install_percent(self: Pin<&mut AppearanceModel>, pct: i32);

        /// Очередная строка журнала действия: что установщик делает прямо
        /// сейчас. Видна и на панели деталей, и в строке состояния.
        #[qinvokable]
        #[cxx_name = "installLog"]
        fn install_log(self: Pin<&mut AppearanceModel>, text: &QString);

        /// Итог установки/применения под кнопками деталей: `ok` решает,
        /// каким значком начнётся строка (решение ядра). Он же гасит полосу
        /// хода и отпирает кнопки — действие кончилось.
        #[qinvokable]
        #[cxx_name = "setDetailStatus"]
        fn set_detail_status(self: Pin<&mut AppearanceModel>, text: &QString, ok: bool);

        /// Ввод в строке поиска изменился: решить, нужны ли подсказки и
        /// откуда их брать (решение ядра — `suggest_action`).
        #[qinvokable]
        #[cxx_name = "updateSuggestions"]
        fn update_suggestions(self: Pin<&mut AppearanceModel>, text: &QString);

        /// Фокус ушёл из строки поиска — выпадашку убрать.
        #[qinvokable]
        #[cxx_name = "setSearchFocused"]
        fn set_search_focused(self: Pin<&mut AppearanceModel>, on: bool);

        /// Ответ магазина для подсказок (JSON-строка).
        #[qinvokable]
        #[cxx_name = "suggestAnswer"]
        fn suggest_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Ходьба по выпадашке с клавиатуры: `up` либо `down`.
        #[qinvokable]
        #[cxx_name = "suggestMove"]
        fn suggest_move(self: Pin<&mut AppearanceModel>, direction: &QString);

        /// Открыть выбранную подсказку. `false` — выбирать нечего, и тогда
        /// Enter означает обычный поиск.
        #[qinvokable]
        #[cxx_name = "suggestActivate"]
        fn suggest_activate(self: Pin<&mut AppearanceModel>) -> bool;

        /// Открыть подсказку под этим номером (клик мышью).
        #[qinvokable]
        #[cxx_name = "suggestPick"]
        fn suggest_pick(self: Pin<&mut AppearanceModel>, index: &QString);

        /// Размер и место выпадашки под строкой поиска: `{x, width, height}`.
        /// Числа считает ядро (`suggest_box`).
        #[qinvokable]
        #[cxx_name = "suggestGeometry"]
        fn suggest_geometry(
            self: &AppearanceModel,
            anchor_x: &QString,
            anchor_width: &QString,
            page_width: &QString,
        ) -> QMap_QString_QVariant;

        /// Листание галереи.
        #[qinvokable]
        #[cxx_name = "galleryPrev"]
        fn gallery_prev(self: Pin<&mut AppearanceModel>);
        #[qinvokable]
        #[cxx_name = "galleryNext"]
        fn gallery_next(self: Pin<&mut AppearanceModel>);

        /// Готовый кадр галереи: номер строкой и путь на диске.
        #[qinvokable]
        #[cxx_name = "setGalleryImage"]
        fn set_gallery_image(self: Pin<&mut AppearanceModel>, index: &QString, path: &QString);

        /// Кадр не доехал.
        #[qinvokable]
        #[cxx_name = "setGalleryFailed"]
        fn set_gallery_failed(self: Pin<&mut AppearanceModel>, index: &QString);

        /// Обход ссылок закончен: сколько кадров есть на самом деле.
        #[qinvokable]
        #[cxx_name = "setGalleryFinished"]
        fn set_gallery_finished(self: Pin<&mut AppearanceModel>, count: &QString);

        /// Кадры для полноэкранного просмотра: только те, что реально
        /// лежат на диске. Пустой список — показывать нечего.
        #[qinvokable]
        #[cxx_name = "zoomFrames"]
        fn zoom_frames(self: &AppearanceModel) -> QList_QVariant;

        /// Дочитать ветку комментариев.
        #[qinvokable]
        #[cxx_name = "loadMoreComments"]
        fn load_more_comments(self: Pin<&mut AppearanceModel>);

        /// Страница комментариев (JSON-строка).
        #[qinvokable]
        #[cxx_name = "commentsAnswer"]
        fn comments_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Переводы комментариев вдогонку уже показанному списку (JSON-строка:
        /// `itemId`, `page` и `rows` — пары «оригинал → перевод»). Второй
        /// проход `loadComments`: список к этому времени на экране, страница
        /// только правит его строки и зажигает у них «показать оригинал».
        /// Комментариев, которых в `rows` нет (русские, пустые), правка не
        /// касается вовсе.
        #[qinvokable]
        #[cxx_name = "commentTranslationsAnswer"]
        fn comment_translations_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Переключить «перевод ⇄ оригинал» у одного комментария.
        #[qinvokable]
        #[cxx_name = "toggleComment"]
        fn toggle_comment(self: Pin<&mut AppearanceModel>, index: &QString);

        /// Паспорт пакета AUR для открытой карточки (JSON-строка
        /// `{itemId, info}` | `{itemId, missing}` | `{itemId, error}`).
        #[qinvokable]
        #[cxx_name = "aurDetailAnswer"]
        fn aur_detail_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Итог проверки источника пакета AUR (JSON-строка
        /// `{itemId, entry}` | `{itemId, failed}`): запись кеша
        /// `aur_trust::CacheEntry`, признаки собирает ядро.
        #[qinvokable]
        #[cxx_name = "aurTrustAnswer"]
        fn aur_trust_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Кнопка установки в карточке пакета AUR — ровно та, что на его
        /// плитке (с очередью установок): `{text, enabled, tooltip, state}`.
        #[qinvokable]
        #[cxx_name = "detailAurButton"]
        fn detail_aur_button(self: &AppearanceModel) -> QMap_QString_QVariant;

        /// Адрес страницы плитки: у товара магазина — pling, у пакета —
        /// aur.archlinux.org. Адрес пакета собирает ядро (`AurPackage`),
        /// своей копии правил в разметке быть не должно.
        #[qinvokable]
        #[cxx_name = "siteUrlOf"]
        fn site_url_of(self: &AppearanceModel, item_id: &QString) -> QString;

        /// Адрес страницы товара в магазине (пусто — открывать нечего).
        #[qinvokable]
        #[cxx_name = "detailSiteUrl"]
        fn detail_site_url(self: &AppearanceModel) -> QString;

        /// «Копировать ссылку»: адрес страницы товара в магазине — в буфер
        /// обмена, подтверждение — в строку состояния.
        #[qinvokable]
        #[cxx_name = "copyDetailLink"]
        fn copy_detail_link(self: Pin<&mut AppearanceModel>);

        /// Картинка по этому адресу (`file://…` из кеша) с движением? Тогда
        /// разметка показывает её `AnimatedImage`, иначе — обычным `Image`.
        #[qinvokable]
        #[cxx_name = "isAnimatedPreview"]
        fn is_animated_preview(self: &AppearanceModel, url: &QString) -> bool;

        /// Номер открытого товара — хозяину для установки и применения.
        #[qinvokable]
        #[cxx_name = "detailItemJson"]
        fn detail_item_json(self: &AppearanceModel) -> QString;

        /// Ссылка на архив открытого товара (прослушивание до установки).
        #[qinvokable]
        #[cxx_name = "detailDownloadUrl"]
        fn detail_download_url(self: &AppearanceModel) -> QString;

        /// Подсказка кнопки одного звука («Прослушать звук «battery-caution»»).
        #[qinvokable]
        #[cxx_name = "soundTooltip"]
        fn sound_tooltip(self: &AppearanceModel, event: &QString) -> QString;

        /// Ход скачивания темы для прослушивания — подпись из ядра.
        /// Числа — `f64`: 64-битное целое QML не принимает (сторож
        /// `qml_int64` в `src/lib.rs`).
        #[qinvokable]
        #[cxx_name = "soundStoreProgress"]
        fn sound_store_progress(self: &AppearanceModel, done: f64, total: f64) -> QString;

        /// Перечитать открытый раздел с диска, обойдя кеш сканов: активная
        /// тема сменилась не через плитку (полоса «Палитра оболочки»).
        /// Только для разделов с диска и только на сетке — магазин и
        /// открытую панель деталей не трогает.
        #[qinvokable]
        #[cxx_name = "reloadLocalFresh"]
        fn reload_local_fresh(self: Pin<&mut AppearanceModel>);

        /// «Настроить…» стиля открытой панели деталей: запустить его
        /// программу настроек (`themes::style_config_button_live`). Отдаёт
        /// ошибку для человека (пусто — запущено).
        #[qinvokable]
        #[cxx_name = "configureDetailStyle"]
        fn configure_detail_style(self: Pin<&mut AppearanceModel>) -> QString;

        /// Переключатель «Применять к программам GTK 4/libadwaita»:
        /// записать в настройки (`gtk4_theme::set_enabled_live`). Отдаёт
        /// ошибку для человека (пусто — записано).
        #[qinvokable]
        #[cxx_name = "setGtk4Apply"]
        fn set_gtk4_apply(self: Pin<&mut AppearanceModel>, on: bool) -> QString;

        /// «Настроить…» рамки окна KDE открытой панели деталей: окно
        /// настроек её модуля (`kdecoration::config_button_live`). Окно
        /// закрыли — снимок рамки переснимается. Отдаёт ошибку для человека
        /// (пусто — запущено).
        #[qinvokable]
        #[cxx_name = "configureDetailDecoration"]
        fn configure_detail_decoration(self: Pin<&mut AppearanceModel>) -> QString;

        /// «Изменить…» (`mode` = `edit`) или «Создать на основе…» (`new`)
        /// у цветовой схемы панели деталей. Есть редактор KDE — он
        /// запускается; нет — отдаёт состояние своего окна (JSON
        /// `color_scheme_edit::EditOpen`; `error` — не вышло).
        #[qinvokable]
        #[cxx_name = "editDetailScheme"]
        fn edit_detail_scheme(self: Pin<&mut AppearanceModel>, mode: &QString) -> QString;

        /// Сохранить схему своего окна: имя, роли (JSON-массив
        /// `SchemeRole`), файл-основа и имя своей схемы, которую можно
        /// переписать. Ответ — JSON `color_scheme_edit::SaveResult`; при
        /// успехе сетка перечитает диск.
        #[qinvokable]
        #[cxx_name = "saveSchemeCopy"]
        fn save_scheme_copy(
            self: Pin<&mut AppearanceModel>,
            name: &QString,
            roles: &QString,
            base: &QString,
            overwrite: &QString,
        ) -> QString;

        /// Цвета главной кнопки деталей: `{bg, fg, border}`.
        #[qinvokable]
        #[cxx_name = "detailButtonColors"]
        fn detail_button_colors(self: &AppearanceModel, state: &QString)
            -> QMap_QString_QVariant;

        /// Цвета кнопки плитки по её состоянию: `{bg, fg, border}`.
        /// Пустая строка — цвета нет вовсе (прозрачная заливка / без рамки).
        /// Решение принимает ядро, здесь только подстановка токена скина.
        #[qinvokable]
        #[cxx_name = "buttonColors"]
        fn button_colors(self: &AppearanceModel, state: &QString) -> QMap_QString_QVariant;

        /// Краски контрола, перекрытого таблицей стилей вкладки:
        /// `{background, hover, border, hoverBorder, color, radius,
        /// paddingH, paddingV}` — готовый набор для `tone` кнопки и списка.
        /// Виды: `combo`, `nav`, `secondary` (см. `control_colors` ядра).
        #[qinvokable]
        #[cxx_name = "controlColors"]
        fn control_colors(self: &AppearanceModel, kind: &QString)
            -> QMap_QString_QVariant;

        /// Номер роли по имени (-1, если роли нет) — проверки без делегата.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &AppearanceModel, name: &QString) -> i32;

        /// Значение роли строки по идентификатору товара — проверки без
        /// делегата.
        #[qinvokable]
        #[cxx_name = "roleValueOf"]
        fn role_value_of(self: &AppearanceModel, item_id: &QString, name: &QString) -> QVariant;
    }

    // Объявление трейта обязано быть В МОСТЕ, иначе `impl cxx_qt::Initialize`
    // снаружи не находит конструктора.
    impl cxx_qt::Initialize for AppearanceModel {}
    // Подписка на смену скина идёт через `CxxQtThread::queue`.
    impl cxx_qt::Threading for AppearanceModel {}
}

use core::pin::Pin;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QList, QMap, QMapPair_QString_QVariant, QModelIndex, QString, QVariant,
};
use serde_json::{json, Value};

use rexon_core::appearance_page as ap;
use rexon_core::local_preview as lp;
use rexon_core::aur::AurPackage;
use rexon_core::aur_install;
use rexon_core::categories;
use rexon_core::cinnamon_spices as cs;
use rexon_core::models::{ThemeCategory, ThemeItem};
use rexon_core::ocs;
use rexon_core::ui_base;

// ============================================================================
// Роли модели плиток
// ============================================================================

/// Роли начинаются с Qt::UserRole + 1 (Qt::UserRole == 256).
const ROLE_BASE: i32 = 256 + 1;

/// Имена ролей в порядке номеров — единственный источник истины и для
/// roleNames, и для data(), и для roleNumber().
const ROLE_NAMES: [&str; 28] = [
    // Откуда плитка: `ocs` (магазин), `aur` (пакет), `local` (диск).
    "kind",
    "itemId",
    "name",
    "author",
    "stats",
    "statsTooltip",
    "starGlyph",
    "preview",
    "metaLine",
    "metaTooltip",
    "metaWarn",
    "buttonText",
    "buttonState",
    "buttonEnabled",
    "buttonTooltip",
    "favoriteText",
    "favoriteState",
    // Чем занять место картинки, когда её нет вовсе. У пакета AUR это его
    // значок: без ссылки на GitHub OpenGraph-карточки не существует, и
    // оригинал в этом случае показывал ровно значок, а не пустоту.
    "previewText",
    // Тема звуков на диске, которую показывает плитка (пусто — не тема
    // звуков): по ней плитка рисует ряд «Прослушать:».
    "soundTheme",
    // Заставка KDE на диске, которую можно показать (`ksplashqml --test`);
    // пусто — кнопки «Продемонстрировать» на плитке нет.
    "splashDemo",
    // Установка пакета AUR из очереди этой сессии: строка под кнопками
    // (причина отказа), её подсказка (полный итог) и подпись ссылки
    // «Журнал» (пусто — ссылки нет). У прочих плиток пусто.
    "aurNote",
    "aurNoteTooltip",
    "aurLog",
    // Признаки риска пакета AUR на плитке: `` | `warn` | `danger`
    // (`aur_trust`; только данные RPC и уже проверенное раньше).
    "riskLevel",
    // «Удалить» на плитке темы с диска (`theme_removal`): видна ли кнопка,
    // можно ли нажать, подсказка (у недоступной — причина). У товаров
    // магазина и пакетов кнопки нет.
    "removeVisible",
    "removeEnabled",
    "removeTooltip",
    // Текущая тема раздела (`local_kind = "active"`): рамка и жирная
    // подпись, как у KDE, — там, где ядро велит выделять (`markCurrent`).
    "current",
];

const ROLE_PREVIEW: usize = 7;
const ROLE_BUTTON_TEXT: usize = 11;
const ROLE_BUTTON_STATE: usize = 12;
const ROLE_BUTTON_ENABLED: usize = 13;
const ROLE_FAVORITE_TEXT: usize = 15;
const ROLE_FAVORITE_STATE: usize = 16;

// ============================================================================
// Чистый слой: строка сетки
// ============================================================================

/// «Удалить» у плитки темы с диска — как его прислал хозяин (поля строки
/// `removeVisible`, `removeEnabled`, `removeTooltip`; решение —
/// `theme_removal::Verdict`).
#[derive(Debug, Clone, Default)]
pub struct RemovalUi {
    pub visible: bool,
    pub enabled: bool,
    pub tooltip: String,
}

impl RemovalUi {
    /// Из строки ответа хозяина; `None` — полей нет (товар магазина).
    fn from_row(raw: &Value) -> Option<Self> {
        let visible = raw.get("removeVisible")?.as_bool()?;
        Some(RemovalUi {
            visible,
            enabled: raw.get("removeEnabled").and_then(Value::as_bool).unwrap_or(false),
            tooltip: raw.get("removeTooltip").and_then(Value::as_str).unwrap_or("").to_string(),
        })
    }
}

/// Сведения сайта Cinnamon Spices у плитки, которым в `ThemeItem` места
/// нет: «лайки», дата правки, есть ли обновление, страница на сайте и
/// пометка «может подвешивать оболочку» (у установленного).
#[derive(Debug, Clone, Default)]
pub struct SpiceExtra {
    pub score: i64,
    pub edited: i64,
    pub update: bool,
    pub page: String,
    pub danger: bool,
    /// Поставленную тему включает сама карточка («Применить») — так в
    /// разделах GTK и рамок окон (`cinnamon_spices::PART_SECTIONS`).
    pub apply: bool,
}

impl SpiceExtra {
    /// Из строки ответа хозяина; `None` — плитка не из Cinnamon Spices и
    /// без пометки.
    fn from_row(raw: &Value) -> Option<Self> {
        if raw.get("spicesScore").is_none() && raw.get("spicesDanger").is_none() {
            return None;
        }
        Some(SpiceExtra {
            score: raw.get("spicesScore").and_then(Value::as_i64).unwrap_or(0),
            edited: raw.get("spicesEdited").and_then(Value::as_i64).unwrap_or(0),
            update: raw.get("spicesUpdate").and_then(Value::as_bool).unwrap_or(false),
            page: raw.get("spicesPage").and_then(Value::as_str).unwrap_or("").to_string(),
            danger: raw.get("spicesDanger").and_then(Value::as_bool).unwrap_or(false),
            apply: raw.get("spicesApply").and_then(Value::as_bool).unwrap_or(false),
        })
    }
}

/// Одна плитка каталога. Держит ровно то, что пришло с ответом; подписи и
/// кнопки считает ядро на лету — это дешёвые строковые операции, и вторая
/// копия решений в состоянии разошлась бы с первой молча.
#[derive(Debug, Clone)]
pub enum Card {
    /// Товар магазина либо местная тема с диска (`local_kind` заполнен).
    Theme(Box<ThemeItem>),
    /// Пакет AUR.
    Aur(Box<AurPackage>),
}

/// Установка AUR из снимка очереди (`aur_install::AurQueue::snapshot_json`).
#[derive(Debug, Clone)]
pub struct AurJobView {
    pub name: String,
    /// Весь состав (у комплекта — все его пакеты).
    pub packages: Vec<String>,
    pub stage: aur_install::AurStage,
    pub message: String,
    pub short: String,
}

impl AurJobView {
    fn from_json(v: &Value) -> Option<Self> {
        let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Some(Self {
            name: text("name"),
            packages: v
                .get("packages")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default(),
            stage: aur_install::AurStage::parse(v.get("stage")?.as_str()?)?,
            message: text("message"),
            short: text("short"),
        })
    }
}

impl Card {
    fn id(&self) -> &str {
        match self {
            Card::Theme(it) => &it.id,
            Card::Aur(p) => &p.name,
        }
    }

    /// Адрес картинки плитки.
    fn preview_url(&self) -> String {
        match self {
            Card::Theme(it) => it.preview_url.clone(),
            Card::Aur(p) => p.preview_url(),
        }
    }

    /// Картинки плитки по очереди: не загрузилась одна — берём следующую
    /// (решение ядра, `preview_candidates`).
    fn preview_candidates(&self) -> Vec<String> {
        match self {
            Card::Theme(it) => ap::preview_candidates(&it.preview_url, &it.preview_urls),
            Card::Aur(p) => vec![p.preview_url()],
        }
    }

    /// Адреса кадров карусели — их греют фоном после самих плиток.
    fn gallery_urls(&self) -> Vec<String> {
        match self {
            Card::Theme(it) => ap::gallery_urls(&it.preview_urls, &it.preview_url),
            // У пакета AUR карусели нет вовсе: одна карточка OpenGraph.
            Card::Aur(_) => Vec::new(),
        }
    }
}

// ============================================================================
// Мелочи перевода в Qt
// ============================================================================

fn vmap(pairs: Vec<(&str, QVariant)>) -> QMap<QMapPair_QString_QVariant> {
    let mut out = QMap::<QMapPair_QString_QVariant>::default();
    for (key, value) in pairs {
        out.insert(QString::from(key), value);
    }
    out
}

fn qstr(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

/// Путь на диске → адрес для `Image.source`. Пустой путь остаётся пустым.
/// Спецсимволы кодируются процентами: имена файлов кеша пишет магазин, и
/// решётка в имени обрубила бы путь на месте (тот же приём во вкладке
/// «Steam»).
fn file_url(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let mut out = String::from("file://");
    for b in path.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Адрес `file://…` → путь на диске (обратное к [`file_url`]). Не файл — `None`.
fn path_of_file_url(url: &str) -> Option<String> {
    let encoded = url.strip_prefix("file://")?;
    // Обратное к `file_url`: разворачиваем %XX.
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// Календарный день отметки времени AUR в поясе машины.
///
/// Пояс берётся у Qt (`QDateTime`), а не считается руками: перевод отметки
/// в день зависит от базы часовых поясов, и своей копии этой базы в ядре
/// быть не должно (потому ядро и просит готовую строку).
fn aur_updated_day(last_modified: i64) -> String {
    if last_modified <= 0 {
        return String::new();
    }
    let tz = cxx_qt_lib::QTimeZone::system_time_zone();
    let dt = cxx_qt_lib::QDateTime::from_secs_since_epoch(last_modified, &tz);
    let date = dt.date();
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Строковое поле записи.
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

// ============================================================================
// Rust-состояние страницы
// ============================================================================

/// Ответ хозяина о том, что заявку оборвали мы сами. Такой «сбой» не
/// повторяют: он значит лишь, что человек ушёл из раздела.
const PREVIEW_CANCELED: &str = "canceled";

/// Вид фонового прогрева: первая страница ЧУЖОГО раздела.
const PREFETCH_CATEGORY: &str = "category";
/// Вид фонового прогрева: следующая страница ОТКРЫТОГО раздела.
const PREFETCH_PAGE: &str = "page";

/// Одна заявка на картинку в том виде, в каком она ушла хозяину.
#[derive(Debug, Clone)]
pub struct PreviewReq {
    /// Чья карточка.
    cid: String,
    /// Адрес картинки — он же ключ дедупа и ключ дискового кеша.
    url: String,
    /// Фоновая заявка: показывать нечего, только наполняет кеш.
    low: bool,
}

/// Заказ фоновому потоку на картинку плитки местной темы.
/// Ключ нарисованной картинки местной плитки в `previews`: номер плитки И
/// каталог темы. По одному номеру картинка, нарисованная для плитки в другом
/// обзоре (тот же `local:gtk:<имя>`, но без своей картинки — например, пока
/// тема была активной), подменяла потом настоящую миниатюру темы.
fn drawn_key(id: &str, install_path: &str) -> String {
    format!("{id}\u{0}{install_path}")
}

/// Картинка плитки темы: своя картинка темы (скан) важнее нарисованной;
/// нарисованная — только у темы без своей; картинка магазина — по номеру.
fn tile_picture(previews: &BTreeMap<String, String>, item: &ThemeItem) -> String {
    if !item.preview_local.is_empty() {
        return item.preview_local.clone();
    }
    previews
        .get(&drawn_key(&item.id, &item.install_path))
        .or_else(|| previews.get(&item.id))
        .cloned()
        .unwrap_or_default()
}

struct PictureJob {
    /// Номер плитки (`local:<вид>:<имя>`).
    id: String,
    /// Вид применения темы.
    kind: String,
    /// Каталог темы (у цветовой схемы — её файл).
    install_path: String,
    /// Знак вида для заглушки.
    glyph: String,
    /// Куда положить готовый кадр.
    dest: String,
    /// Рамка окна KDE: снять её модулем (`kdecoration::take_shot`).
    shot: Option<rexon_core::kdecoration::ShotRequest>,
    /// Стиль Qt, тема Kvantum или GTK 3: снять форму из настоящих виджетов
    /// (`widget_shot::take_shot`).
    widget_shot: Option<rexon_core::widget_shot::ShotRequest>,
}

pub struct AppearanceModelRust {
    /// Номер записи в общем реестре смены скина (`crate::skin`).
    skin_watch: u64,
    home: PathBuf,

    /// Разделы боковой панели — порядок и состав задаёт ядро.
    sidebar: Vec<ThemeCategory>,
    /// Состояние каталога целиком: раздел, страница, сортировка, источник.
    view: ap::CatalogView,
    /// Учёт летящих запросов и прогрева — конечный автомат ядра.
    requests: ap::CatalogRequests,

    /// Плитки текущей страницы.
    cards: Vec<Card>,
    /// Идентификатор товара → путь к готовому превью.
    previews: BTreeMap<String, String>,
    /// Плитки местных тем, чья картинка сейчас рисуется в фоне (номер
    /// плитки): повторный заход в раздел не заказывает их второй раз.
    drawing: std::collections::HashSet<String>,
    /// Сколько раз закрывали окно настроек рамки KDE (номер плитки): номер
    /// входит в ключ кеша снимка, чтобы плитка показала новый кадр, а не
    /// тот же файл из кеша картинок Qt.
    deco_config_gen: BTreeMap<String, u32>,
    /// Очередь загрузчика превью: сколько картинок тянуть разом, кого
    /// пропускать вперёд, что считать дублем и когда повторять — решает она.
    preview: ap::PreviewQueue,
    /// Номер заявки → что именно по ней просили. Номер живёт до ответа и
    /// возвращается хозяином обратно: две плитки изредка делят один адрес, и
    /// по одному адресу их было бы не различить.
    preview_jobs: BTreeMap<u64, PreviewReq>,
    /// Счётчик номеров заявок.
    preview_seq: u64,
    /// Идентификаторы избранного.
    favorites: BTreeSet<String>,
    /// «Удалить» у тем с диска: номер плитки → решение, присланное
    /// хозяином вместе со сканом (`theme_removal::Verdict`).
    removal: BTreeMap<String, RemovalUi>,
    /// Плитки Cinnamon Spices: номер плитки → сведения сайта ([`SpiceExtra`]).
    spices: BTreeMap<String, SpiceExtra>,
    /// Всего работ у раздела по ответу магазина.
    total: i64,
    /// Есть ли что за страницей — у потока магазина с добором
    /// (`ap::stream_page`) это знает сам ответ; `None` — ответ без потока
    /// (AUR, диск): страницы там режутся на месте и считаются по `total`.
    has_more: Option<bool>,

    /// Есть ли помощник AUR (yay/paru).
    aur_helper: bool,
    /// Выбор рабочего стола: [(id, имя)].
    de_choices: Vec<(String, String)>,
    /// Рабочий стол запущенного сеанса (пусто — не опознан): по нему ручной
    /// выбор подписывается как ручной (`ap::de_picker_choices`).
    detected_de: String,

    status: String,
    /// Что показано: `grid` | `detail` | `note` | `plasmalogin`.
    screen: String,
    /// Экран входа рисует Plasma Login Manager (ответ фоновой пробы; до
    /// ответа — нет, и панель стоит как при SDDM).
    plasma_login: bool,
    /// Есть чем открыть настройки экрана входа Plasma Login Manager.
    plm_can_open: bool,

    /// Установки AUR этой сессии (очередь живёт в объекте действий и
    /// переживает страницу; здесь — последний её снимок).
    aur_jobs: Vec<AurJobView>,
    /// Последняя строка о ходе установки AUR, которую мы поставили в строку
    /// состояния: чужие сообщения ею не перебиваются по второму разу.
    aur_status: String,

    // ── Панель деталей ──
    /// Открытый товар (местная тема отличается заполненным `local_kind`).
    detail: Option<Box<ThemeItem>>,
    /// Показывать оригинал описания вместо перевода.
    detail_show_original: bool,
    /// Итог последнего действия под кнопками.
    detail_status: String,
    /// Идёт долгое действие над товаром.
    action_busy: bool,
    /// Вид идущего действия — от него зависит, есть ли полоса хода.
    action_kind: String,
    /// Номер товара, над которым идёт действие: по итогу установки его
    /// плитке и панели деталей ставится «✓ Установлено»/«✓ Применено».
    action_item: String,
    /// Сетку надо пересобрать, как только осядет смена стиля Qt.
    reload_pending: bool,
    /// Следующий запрос каталога обязан обойти кеш сканов диска.
    catalog_fresh: bool,
    /// Следующий запрос Cinnamon Spices — мимо кеша («Обновить»).
    site_force: bool,
    /// Доля выполненного для полосы хода, 0–100.
    install_percent: i64,
    /// Путь к картинке, показываемой сейчас.
    detail_frame: String,
    /// Надпись вместо картинки, когда показывать нечего.
    detail_frame_text: String,

    // ── Галерея скриншотов ──
    /// Адреса кадров карусели (первый слот — «лицо товара», его в карусели
    /// нет: решение ядра, `gallery_urls`).
    gallery_src: Vec<String>,
    /// Номер кадра → путь на диске.
    gallery_loaded: BTreeMap<i64, String>,
    /// Кадры, которые не доедут.
    gallery_failed: BTreeSet<i64>,
    /// Показываемый кадр.
    gallery_index: i64,
    /// Сколько кадров у товара по ответу магазина.
    gallery_known: i64,
    /// Обход ссылок закончен.
    gallery_finished: bool,
    /// Человек листал руками — тогда кадр из кеша его не сдвинет.
    gallery_user_nav: bool,

    // ── Комментарии ──
    comments: Vec<CommentRow>,
    /// Следующая страница комментариев.
    comments_page: i64,
    /// Сколько комментариев у товара всего (`None` — магазин не сказал).
    comments_total: Option<i64>,

    // ── Подсказки поиска ──
    /// Что набрано в строке поиска прямо сейчас.
    suggest_query: String,
    /// Поколение запроса подсказок: ответ на прошлый ввод отбрасывается.
    suggest_generation: u64,
    /// Показываемые подсказки.
    suggest_items: Vec<ThemeItem>,
    /// Фокус в строке поиска.
    suggest_focused: bool,
    /// Выбранная строка выпадашки (-1 — ни одной).
    suggest_index: i64,

    // ── Карточка пакета AUR ──
    /// Открытый пакет AUR (у товара магазина и темы с диска — `None`).
    aur_detail: Option<Box<AurDetail>>,
}

/// Открытая карточка пакета AUR.
#[derive(Debug, Clone, Default)]
pub struct AurDetail {
    /// Плитка, с которой открыли (до ответа RPC — всё, что известно).
    pkg: AurPackage,
    /// Паспорт RPC `info`.
    info: Option<rexon_core::aur::AurInfo>,
    /// Соисполнители со страницы пакета.
    comaintainers: Vec<String>,
    /// Почему паспорта нет (сеть, пакет удалён) — строкой для показа.
    note: String,
    /// Страницу пакета не дали — комментарии только на сайте.
    comments_blocked: bool,
    /// У сайта есть следующая страница комментариев.
    comments_more: bool,
    /// Проверка источника: запись кеша `aur_trust` (глубокая проверка и
    /// снимки сопровождающего).
    trust: Option<rexon_core::aur_trust::CacheEntry>,
    /// Проверка кончилась (с ответом или без).
    trust_done: bool,
}

impl Default for AppearanceModelRust {
    /// Стартовое состояние собирается здесь, а не в `initialize`, потому что
    /// `rowCount` зовётся из QML раньше любого нашего кода. Раздел по
    /// умолчанию — «Популярные» (первый в боковой панели ядра).
    fn default() -> Self {
        let home = home_dir();
        let sidebar = categories::sidebar(&home);
        let first = sidebar
            .first()
            .cloned()
            .unwrap_or_else(categories::popular);
        Self {
            skin_watch: 0,
            home,
            view: ap::CatalogView::new(first, "kde"),
            sidebar,
            requests: ap::CatalogRequests::new(),
            cards: Vec::new(),
            previews: BTreeMap::new(),
            drawing: Default::default(),
            deco_config_gen: BTreeMap::new(),
            preview: ap::PreviewQueue::new(),
            preview_jobs: BTreeMap::new(),
            preview_seq: 0,
            favorites: BTreeSet::new(),
            removal: BTreeMap::new(),
            spices: BTreeMap::new(),
            total: 0,
            has_more: None,
            aur_helper: false,
            aur_jobs: Vec::new(),
            aur_status: String::new(),
            de_choices: Vec::new(),
            detected_de: String::new(),
            status: String::new(),
            screen: "grid".to_string(),
            plasma_login: false,
            plm_can_open: false,
            detail: None,
            detail_show_original: false,
            detail_status: String::new(),
            action_busy: false,
            action_kind: String::new(),
            action_item: String::new(),
            reload_pending: false,
            catalog_fresh: false,
            site_force: false,
            install_percent: 0,
            detail_frame: String::new(),
            detail_frame_text: ap::detail_preview_loading(),
            gallery_src: Vec::new(),
            gallery_loaded: BTreeMap::new(),
            gallery_failed: BTreeSet::new(),
            gallery_index: 0,
            gallery_known: 0,
            gallery_finished: false,
            gallery_user_nav: false,
            comments: Vec::new(),
            comments_page: 0,
            comments_total: None,
            suggest_query: String::new(),
            suggest_generation: 0,
            suggest_items: Vec::new(),
            suggest_focused: false,
            suggest_index: -1,
            aur_detail: None,
        }
    }
}

/// Снять подписку на смену скина: реестр иначе держал бы запись закрытой
/// вкладки до первой рассылки.
impl Drop for AppearanceModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

impl AppearanceModelRust {
    /// Раздел по ключу.
    fn category(&self, key: &str) -> Option<ThemeCategory> {
        self.sidebar.iter().find(|c| c.key == key).cloned()
    }
}

impl qobject::AppearanceModel {
    // ── Неизменные подписи и числа ──────────────────────────────────────
    fn window_title(&self) -> QString {
        QString::from(&ap::page_title())
    }
    fn window_min_width(&self) -> i32 {
        ap::WINDOW_MIN_SIZE.0 as i32
    }
    fn window_min_height(&self) -> i32 {
        ap::WINDOW_MIN_SIZE.1 as i32
    }
    fn header_title(&self) -> QString {
        QString::from(&ap::page_title())
    }
    fn header_title_point_size(&self) -> i32 {
        ap::PAGE_TITLE_POINT_SIZE
    }
    fn sidebar_width(&self) -> i32 {
        ap::SIDEBAR_W
    }
    fn sidebar_row_height(&self) -> i32 {
        ap::SIDEBAR_ROW_H
    }
    fn search_width(&self) -> i32 {
        ap::SEARCH_W
    }
    fn card_width(&self) -> i32 {
        ap::CARD_W
    }
    fn card_height(&self) -> i32 {
        ap::CARD_H
    }
    fn card_preview_height(&self) -> i32 {
        ap::PREVIEW_H
    }
    fn card_text_width(&self) -> i32 {
        ap::card_text_width()
    }
    fn card_body_margin(&self) -> i32 {
        ap::CARD_BODY_MARGIN
    }
    fn card_body_top_margin(&self) -> i32 {
        ap::CARD_BODY_TOP_MARGIN
    }
    fn card_body_bottom_margin(&self) -> i32 {
        ap::CARD_BODY_BOTTOM_MARGIN
    }
    fn card_body_spacing(&self) -> i32 {
        ap::CARD_BODY_SPACING
    }
    fn card_button_height(&self) -> i32 {
        ap::CARD_BUTTON_HEIGHT
    }
    fn card_button_top_margin(&self) -> i32 {
        ap::CARD_BUTTON_TOP_MARGIN
    }
    fn card_name_point_size(&self) -> i32 {
        ap::CARD_NAME_POINT_SIZE
    }
    fn card_meta_point_size(&self) -> i32 {
        ap::CARD_META_POINT_SIZE
    }
    fn grid_spacing(&self) -> i32 {
        ap::GRID_SPACING
    }
    fn grid_margin_h(&self) -> i32 {
        ap::GRID_MARGIN_H
    }
    fn grid_margin_v(&self) -> i32 {
        ap::GRID_MARGIN_V
    }
    fn header_margin_h(&self) -> i32 {
        ap::HEADER_MARGIN_H
    }
    fn header_margin_v(&self) -> i32 {
        ap::HEADER_MARGIN_V
    }
    fn preview_ready_pending(&self) -> bool {
        self.rust().preview.has_ready()
    }
    fn gallery_warm_delay_ms(&self) -> i32 {
        ap::GALLERY_WARM_DELAY_MS as i32
    }
    fn prefetch_start_ms(&self) -> i32 {
        ap::PREFETCH_START_MS as i32
    }
    fn prefetch_pause_ms(&self) -> i32 {
        ap::PREFETCH_PAUSE_MS as i32
    }
    fn purge_translations_ms(&self) -> i32 {
        ap::PURGE_TRANSLATIONS_MS as i32
    }
    fn style_storm_ms(&self) -> i32 {
        ap::STYLE_STORM_MS as i32
    }
    fn reload_pending(&self) -> bool {
        self.rust().reload_pending
    }
    fn catalog_debounce_ms(&self) -> i32 {
        ap::CATALOG_DEBOUNCE_MS as i32
    }
    fn first_load_ms(&self) -> i32 {
        ap::FIRST_LOAD_MS as i32
    }
    fn source_label(&self) -> QString {
        QString::from(&ap::source_label())
    }
    fn submode_label(&self) -> QString {
        QString::from(&ap::submode_label())
    }
    fn cursor_size_label(&self) -> QString {
        QString::from(&ap::cursor_size_label())
    }
    fn de_label(&self) -> QString {
        QString::from(&ap::de_label())
    }
    fn de_tooltip(&self) -> QString {
        QString::from(&ap::de_tooltip())
    }
    fn refresh_text(&self) -> QString {
        QString::from(ap::REFRESH_BTN)
    }
    fn refresh_tooltip(&self) -> QString {
        QString::from(&ap::refresh_tooltip())
    }
    fn page_prev_text(&self) -> QString {
        QString::from(&ap::page_prev_btn())
    }
    fn page_next_text(&self) -> QString {
        QString::from(&ap::page_next_btn())
    }

    fn sorts(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (label, key) in ap::SORTS {
            out.append(QVariant::from(&vmap(vec![
                ("label", qstr(&ap::sort_label(label))),
                ("key", qstr(key)),
            ])));
        }
        out
    }

    /// `sty` раздела-группы — те же метрики и краски, что у разделов
    /// «Настроек» (`settings_page::group_style`).
    fn section_styles(&self) -> QMap<QMapPair_QString_QVariant> {
        vmap(vec![("group", QVariant::from(&crate::settings_page::group_style()))])
    }

    // ── Палитра ─────────────────────────────────────────────────────────
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        // Цвет необратимого действия («🗑 Удалить») — одно решение на
        // продукт, живёт в ядре вкладки «Расширения». Своей копии красного
        // здесь не заводим (правило #12).
        out.insert(
            QString::from("danger"),
            qstr(rexon_core::qt_extensions::DANGER_COLOR),
        );
        // Цвет предупреждения — тот же, что во вкладке «Настройки»: им
        // раздел «Панель MATE» помечает недостающие апплеты раскладки.
        out.insert(QString::from("state_warn"), qstr(rexon_core::qt_settings::COLOR_WARN));
        out.insert(
            QString::from("danger_fill_alpha"),
            QVariant::from(&rexon_core::qt_extensions::DANGER_FILL_ALPHA),
        );
        out.insert(
            QString::from("danger_fill_hover_alpha"),
            QVariant::from(&rexon_core::qt_extensions::DANGER_FILL_HOVER_ALPHA),
        );
        // Затемнение фона под модальным окном — одно на продукт.
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        out
    }

    // ── Боковая панель ──────────────────────────────────────────────────
    fn sidebar_rows(&self) -> QList<QVariant> {
        let me = self.rust();
        let de = me.view.de.as_str();
        let mut out = QList::<QVariant>::default();
        // Заголовок группы — у первой ВИДИМОЙ строки группы: скрытые в
        // окружении разделы заголовка не уносят, а пустая группа не
        // показывает его вовсе.
        let mut last_group: Option<categories::SidebarGroup> = None;
        // Раскладки mate-panel — только там, где панель на самом деле есть
        // (сеанс MATE и сама программа).
        let mate_panel = rexon_core::mate_panel::is_available(
            &rexon_core::animations::SystemIo,
            &rexon_core::mate_panel::PanelCtx::from_env(),
        );
        for cat in categories::grouped_sidebar(&me.sidebar, de) {
            // Раздел настроек действует на ЗАПУЩЕННЫЙ стол, а не на
            // выбранный в шапке: спрятанный в этом сеансе раздел (эффекты
            // KWin вне KDE) не показываем и при выборе другого стола.
            let available = categories::category_available(&cat.key, de)
                && (cat.key != categories::MATE_PANEL_KEY || mate_panel)
                && (!categories::is_settings_panel(&cat.key)
                    || categories::category_available(&cat.key, &me.detected_de));
            let group = categories::category_group(&cat.key, de);
            let opens_group = available && last_group != Some(group);
            if opens_group {
                last_group = Some(group);
            }
            let name = categories::display_name(&cat, de);
            out.append(QVariant::from(&vmap(vec![
                ("key", qstr(&cat.key)),
                ("icon", qstr(&cat.icon)),
                ("name", qstr(&name)),
                ("text", qstr(&ap::sidebar_row_text(&cat.icon, &name))),
                ("available", QVariant::from(&available)),
                (
                    "current",
                    QVariant::from(&(cat.key == me.view.category.key)),
                ),
                ("group", qstr(group.key())),
                (
                    "groupTitle",
                    qstr(&if opens_group { categories::group_title(group, de) } else { String::new() }),
                ),
                (
                    "groupNote",
                    qstr(&if opens_group { categories::group_note(group, de) } else { String::new() }),
                ),
            ])));
        }
        out
    }

    fn section_title(&self) -> QString {
        let view = &self.rust().view;
        let cat = &view.category;
        QString::from(&ap::section_title(&cat.icon, &categories::display_name(cat, &view.de)))
    }

    // ── Шапка ───────────────────────────────────────────────────────────
    fn chrome(&self) -> ap::Chrome {
        let me = self.rust();
        me.view.chrome()
    }

    fn search_visible(&self) -> bool {
        self.chrome().search
    }
    fn search_delay_ms(&self) -> i32 {
        self.rust().view.search_delay_ms() as i32
    }
    fn search_clear_tooltip(&self) -> QString {
        QString::from(&ap::search_clear_tooltip())
    }
    fn search_placeholder(&self) -> QString {
        if !self.chrome().search {
            return QString::default();
        }
        QString::from(&self.rust().view.search_placeholder())
    }
    fn event_sounds_banner(&self) -> QString {
        let me = self.rust();
        let running = crate::settings_actions::current_de();
        let raw = if me.view.category.key == "sounds" && me.view.de == running {
            crate::settings_actions::event_sounds_raw(&running)
        } else {
            String::new()
        };
        let banner = ap::event_sounds_banner(&me.view.category.key, &raw);
        QString::from(&serde_json::to_string(&banner).unwrap_or_default())
    }
    fn enable_event_sounds(&self) -> QString {
        QString::from(&crate::settings_actions::switch_event_sounds(true))
    }
    fn applied_revert_banner(&self) -> QString {
        use rexon_core::install_journal as journal;
        use rexon_core::shell_palette as sp;
        let key = self.rust().view.category.key.clone();
        let rexon = rexon_core::utils::rexon_config_dir();
        let applied = journal::view_at(&rexon).applied.remove(&key);
        let colors = key == "colors";
        let home = sp::PaletteHome::live();
        let layer_memo = colors && sp::read_layer_memo(&home).has_changes();
        let shell_revert = colors && {
            let b = sp::live_banner();
            b.visible && b.can_revert
        };
        let banner = ap::applied_revert_banner(&key, applied.as_ref(), layer_memo, shell_revert);
        QString::from(&serde_json::to_string(&banner).unwrap_or_default())
    }
    fn caveat_text(&self) -> QString {
        // «ℹ️ …» — тот же вид подписи, что в оригинале (`_sync_de_caveat`).
        match self.chrome().caveat {
            Some(note) => QString::from(&format!("ℹ️ {note}")),
            None => QString::default(),
        }
    }
    fn source_bar_visible(&self) -> bool {
        self.chrome().source_bar
    }
    fn style_bar_visible(&self) -> bool {
        self.chrome().style_bar
    }
    fn lock_bar_visible(&self) -> bool {
        self.chrome().lock_bar
    }
    fn cursor_size_bar_visible(&self) -> bool {
        self.chrome().cursor_size_bar
    }
    fn install_file_visible(&self) -> bool {
        self.chrome().install_file
    }
    fn install_file_text(&self) -> QString {
        QString::from(&ap::install_file_btn())
    }
    fn install_file_tooltip(&self) -> QString {
        QString::from(&ap::install_file_tooltip())
    }
    fn install_file_files_text(&self) -> QString {
        QString::from(&ap::install_file_files_item())
    }
    fn install_file_folder_text(&self) -> QString {
        QString::from(&ap::install_file_folder_item())
    }
    fn install_file_title(&self) -> QString {
        QString::from(&ap::install_file_dialog_title(&self.rust().view.category.name))
    }
    fn install_file_filters(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for f in rexon_core::local_install::dialog_filters(&self.rust().view.category.key) {
            out.append(qstr(&f));
        }
        out
    }
    fn install_file_section(&self) -> QString {
        QString::from(&self.rust().view.category.key)
    }
    fn installed_view(&self) -> bool {
        self.rust().view.installed_view()
    }
    fn mark_current(&self) -> bool {
        self.rust().view.marks_current()
    }
    fn sort_visible(&self) -> bool {
        self.chrome().sort_combo
    }
    fn pagination_visible(&self) -> bool {
        self.chrome().pagination
    }

    fn source_chips(&self) -> QList<QVariant> {
        let view = &self.rust().view;
        // «VSThemes» есть только в «Курсорах»; вне их выбранным рисуется
        // источник, который действует на самом деле.
        let active = view.effective_source().to_string();
        let mut out = QList::<QVariant>::default();
        for chip in ap::source_chips_for(&view.category.key) {
            out.append(QVariant::from(&vmap(vec![
                ("key", qstr(chip.key)),
                ("label", qstr(&chip.label)),
                ("tooltip", qstr(&chip.tooltip)),
                ("checked", QVariant::from(&(chip.key == active))),
            ])));
        }
        out
    }

    fn lock_chips(&self) -> QList<QVariant> {
        let view = &self.rust().view;
        let active = view.lock_mode_effective().to_string();
        let mut out = QList::<QVariant>::default();
        for chip in ap::lock_chips_for(&view.de) {
            out.append(QVariant::from(&vmap(vec![
                ("key", qstr(chip.key)),
                ("label", qstr(&chip.label)),
                ("tooltip", qstr(&chip.tooltip)),
                ("checked", QVariant::from(&(chip.key == active))),
            ])));
        }
        out
    }

    fn cursor_size_chips(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for chip in ap::cursor_size_chips() {
            out.append(QVariant::from(&vmap(vec![
                ("key", qstr(chip.key)),
                ("label", qstr(&chip.label)),
                ("tooltip", qstr(&chip.tooltip)),
            ])));
        }
        out
    }

    /// Действующий фильтр тона раздела (`CatalogView::active_tone_filter`).
    fn active_tone_filter(&self) -> &'static str {
        self.rust().view.active_tone_filter()
    }

    fn tone_bar_visible(&self) -> bool {
        self.rust().view.tone_bar()
    }

    /// Сужает ли выбранный фильтр сетку до установленного (тон, «Qt-стили /
    /// Kvantum», «Запущенные» — правило одно, `CatalogView::installed_only`).
    /// Под ним витрины магазина нет вовсе: ни плиток (магазина и AUR), ни
    /// запроса за ней, ни её листалки, сортировки и выбора магазина.
    fn installed_only(&self) -> bool {
        self.rust().view.installed_only()
    }

    /// Чипы фильтра тона (`lp::tone_chips`).
    fn tone_chips(&self) -> QList<QVariant> {
        let active = self.active_tone_filter();
        let mut out = QList::<QVariant>::default();
        for chip in lp::tone_chips(&self.rust().view.category.key) {
            out.append(QVariant::from(&vmap(vec![
                ("key", qstr(chip.key)),
                ("label", qstr(&chip.label)),
                ("tooltip", qstr(&chip.tooltip)),
                ("checked", QVariant::from(&(chip.key == active))),
            ])));
        }
        out
    }

    /// Чипы фильтра сетки «Стиля приложений» (`ap::style_filter_chips`).
    fn style_chips(&self) -> QList<QVariant> {
        let active = self.rust().view.style_filter();
        let mut out = QList::<QVariant>::default();
        for chip in ap::style_filter_chips() {
            out.append(QVariant::from(&vmap(vec![
                ("key", qstr(chip.key)),
                ("label", qstr(&chip.label)),
                ("tooltip", qstr(&chip.tooltip)),
                ("checked", QVariant::from(&(chip.key == active))),
            ])));
        }
        out
    }

    fn sort_key(&self) -> QString {
        QString::from(&self.rust().view.active_sort())
    }

    fn query(&self) -> QString {
        QString::from(&self.rust().view.query)
    }

    fn de_choices(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (id, name) in &self.rust().de_choices {
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(id)),
                ("name", qstr(name)),
            ])));
        }
        out
    }

    fn de(&self) -> QString {
        QString::from(&self.rust().view.de)
    }

    // ── Экран, состояние, страницы ──────────────────────────────────────
    fn screen(&self) -> QString {
        QString::from(&self.rust().screen)
    }

    fn note_text(&self) -> QString {
        let me = self.rust();
        match me.view.note(&me.home) {
            Some(text) => QString::from(&text),
            None => QString::default(),
        }
    }

    fn plm_panel(&self) -> ap::PlasmaLoginPanel {
        ap::plasma_login_panel(self.rust().plm_can_open)
    }
    fn plm_title(&self) -> QString {
        QString::from(&self.plm_panel().title)
    }
    fn plm_body(&self) -> QString {
        QString::from(&self.plm_panel().body)
    }
    fn plm_open_label(&self) -> QString {
        QString::from(&self.plm_panel().open_label)
    }
    fn plm_open_visible(&self) -> bool {
        self.rust().plm_can_open
    }
    fn plm_store_hint(&self) -> QString {
        QString::from(&self.plm_panel().store_hint)
    }
    fn login_section(&self) -> bool {
        let me = self.rust();
        me.screen != "detail" && me.view.is_login_screen()
    }

    fn status_text(&self) -> QString {
        QString::from(&self.rust().status)
    }

    fn pagination(&self) -> ap::Pagination {
        let me = self.rust();
        match me.has_more {
            Some(more) => ap::stream_pagination(me.view.page, me.total, more),
            None => ap::pagination(me.view.page, me.total, me.cards.len()),
        }
    }

    fn page_label(&self) -> QString {
        QString::from(&self.pagination().label)
    }
    fn prev_enabled(&self) -> bool {
        self.pagination().prev_enabled
    }
    fn next_enabled(&self) -> bool {
        self.pagination().next_enabled
    }
    fn count(&self) -> i32 {
        self.rust().cards.len() as i32
    }
}

// ============================================================================
// Модель плиток
// ============================================================================

impl qobject::AppearanceModel {
    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.rust().cards.len() as i32
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut names = qobject::QHash_i32_QByteArray::default();
        for (i, name) in ROLE_NAMES.iter().enumerate() {
            names.insert(ROLE_BASE + i as i32, QByteArray::from(*name));
        }
        names
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let row = index.row();
        if !index.is_valid() || row < 0 || row as usize >= self.rust().cards.len() {
            return QVariant::default();
        }
        let Ok(slot) = usize::try_from(role - ROLE_BASE) else {
            return QVariant::default();
        };
        let card = self.rust().cards[row as usize].clone();
        self.role_of(&card, ROLE_NAMES.get(slot).copied())
    }

    /// Значение одной роли плитки. Отдельно от `data`, чтобы тем же путём
    /// отвечал `roleValueOf` — иначе проверки смотрели бы не туда, куда
    /// смотрит делегат.
    fn role_of(&self, card: &Card, name: Option<&str>) -> QVariant {
        let Some(name) = name else {
            return QVariant::default();
        };
        match card {
            Card::Theme(item) => self.theme_role(item, name),
            Card::Aur(pkg) => self.aur_role(pkg, name),
        }
    }

    fn theme_role(&self, item: &ThemeItem, name: &str) -> QVariant {
        let local_kind = item.local_kind.clone().unwrap_or_default();
        let local = !local_kind.is_empty();
        if let Some(v) = self.spice_role(item, local, name) {
            return v;
        }
        match name {
            "kind" => qstr(if local { "local" } else { "ocs" }),
            "itemId" => qstr(&item.id),
            "name" => qstr(ap::card_name(&item.name)),
            // В витринах «Новые»/«Популярные» — ещё и вид вещи.
            "author" => {
                let showcase = !local
                    && matches!(self.rust().view.category.store, rexon_core::models::CatalogSource::OcsShowcase);
                qstr(&ap::showcase_author(&self.rust().home, item, showcase))
            }
            // Оценка и число загрузок бывают только у товара магазина: у темы
            // с диска «★ 0.0 ⬇ 0» — бессмыслица, строку не показываем.
            // У темы с диска строка оценки свободна: в ней отметка глобальной
            // темы со своей раскладкой виджетов (как подпись плитки в KDE).
            // У стиля Plasma без своих цветов — «Поддерживает цветовые схемы».
            "stats" if local => qstr(
                &lp::follows_scheme_badge(&item.id, &item.install_path, self.active_tone_filter())
                    .unwrap_or_else(|| rexon_core::themes::lnf_tile_badge(&item.id, &item.install_path)),
            ),
            "stats" => qstr(&ap::card_stats_line(
                item.rating_stars(),
                item.downloads,
                &item.updated,
            )),
            "statsTooltip" => match ap::card_updated_tooltip(&item.updated) {
                Some(tip) => qstr(&tip),
                None => qstr(""),
            },
            // Значок оценки — единственное цветное пятно стат-строки
            // (#CardStar таблицы стилей); цвет берётся из палитры ключом
            // `rating_star`, здесь только сам знак.
            "starGlyph" if local => qstr(""),
            "starGlyph" => qstr("⭐"),
            "preview" => qstr(&file_url(&tile_picture(&self.rust().previews, item))),
            // У темы запасного знака нет: оригинал оставлял место картинки
            // пустым, и выдумывать сюда своё — отсебятина.
            "previewText" => qstr(""),
            // Местная тема источника не имеет — строка пустая.
            "metaLine" | "metaTooltip" => qstr(""),
            "soundTheme" => qstr(&ap::local_sound_theme(&item.id).unwrap_or_default()),
            "splashDemo" => qstr(&ap::local_splash_demo(&item.id).unwrap_or_default()),
            "aurNote" | "aurNoteTooltip" | "aurLog" | "riskLevel" => qstr(""),
            "metaWarn" => QVariant::from(&false),
            "buttonText" | "buttonState" | "buttonEnabled" => {
                let btn = if local {
                    ap::card_local_button_for(&local_kind, &item.id)
                } else {
                    let can_apply = ap::store_item_can_apply(&self.rust().home, item);
                    ap::card_install_button(item.installed, item.applied, can_apply)
                };
                match name {
                    "buttonText" => qstr(&btn.text),
                    "buttonState" => qstr(btn.state),
                    _ => QVariant::from(&btn.enabled),
                }
            }
            "buttonTooltip" => {
                if local && ap::sddm_blocked_by_plasma_login(&item.id, self.rust().plasma_login) {
                    qstr(&ap::sddm_under_plasma_login_reason())
                } else {
                    qstr("")
                }
            }
            "removeVisible" | "removeEnabled" | "removeTooltip" => {
                let ui = self.rust().removal.get(&item.id).cloned().unwrap_or_default();
                match name {
                    "removeVisible" => QVariant::from(&(local && ui.visible)),
                    "removeEnabled" => QVariant::from(&(local && ui.enabled)),
                    _ => qstr(&ui.tooltip),
                }
            }
            "current" => QVariant::from(&(local_kind == "active")),
            "favoriteText" | "favoriteState" => {
                // «★ Избранное» есть и у местной темы: её тоже можно
                // отложить (так в оригинале).
                let btn = ap::favorite_button(self.rust().favorites.contains(&item.id));
                if name == "favoriteText" {
                    qstr(&btn.text)
                } else {
                    qstr(btn.state)
                }
            }
            _ => QVariant::default(),
        }
    }

    /// Роли, в которых плитка Cinnamon Spices отличается от товара KDE
    /// Store: «лайки» и дата правки вместо оценки и скачиваний, «Обновить»
    /// при новой версии на сайте, предупреждение «может подвешивать
    /// оболочку» (и у плитки установленного). `None` — как у всех.
    fn spice_role(&self, item: &ThemeItem, local: bool, name: &str) -> Option<QVariant> {
        let x = self.rust().spices.get(&item.id)?;
        Some(match name {
            "metaLine" if x.danger => qstr(&cs::danger_line()),
            "metaTooltip" if x.danger => qstr(&cs::danger_tooltip()),
            "metaWarn" if x.danger => QVariant::from(&true),
            "stats" if !local => qstr(&cs::stats_line(x.score, x.edited)),
            "statsTooltip" if !local => qstr(&cs::stats_tooltip()),
            "buttonText" | "buttonState" | "buttonEnabled" if !local => {
                let btn = cs::card_button_for(item.installed, x.update, x.apply);
                match name {
                    "buttonText" => qstr(&btn.text),
                    "buttonState" => qstr(btn.state),
                    _ => QVariant::from(&btn.enabled),
                }
            }
            _ => return None,
        })
    }

    /// Страница плитки Cinnamon Spices на сайте (пусто — плитка не оттуда).
    fn spice_page(&self, item_id: &str) -> Option<String> {
        self.rust().spices.get(item_id).map(|x| x.page.clone()).filter(|p| !p.is_empty())
    }

    /// Что показать на плитке пакета `name` по очереди установок AUR.
    ///
    /// Своя установка — по её стадии; пакет, который ставится в составе
    /// идущего комплекта, — стадией комплекта (пока тот идёт). Установка,
    /// удавшаяся до этой сессии, очередь не трогает: плитка как раньше.
    fn aur_job_state(&self, name: &str) -> Option<aur_install::CardJobState> {
        let jobs = &self.rust().aur_jobs;
        let job = jobs
            .iter()
            .find(|j| j.name == name)
            .or_else(|| jobs.iter().find(|j| j.stage.active() && j.packages.iter().any(|p| p == name)))?;
        Some(aur_install::card_job_state(job.stage, &job.short, &job.message))
    }

    fn aur_role(&self, pkg: &AurPackage, name: &str) -> QVariant {
        // Установка из очереди этой сессии перекрывает обычную кнопку:
        // «В очереди», «Сборка…», «Повторить» (решение ядра).
        const JOB_ROLES: &[&str] = &[
            "buttonText",
            "buttonEnabled",
            "buttonTooltip",
            "buttonState",
            "aurNote",
            "aurNoteTooltip",
            "aurLog",
        ];
        if JOB_ROLES.contains(&name) {
            if let Some(st) = self.aur_job_state(&pkg.name) {
                return match name {
                    "buttonText" => qstr(&st.button_text),
                    "buttonEnabled" => QVariant::from(&st.button_enabled),
                    "buttonTooltip" => qstr(&st.button_tooltip),
                    "buttonState" => qstr(st.button_state),
                    "aurNote" => qstr(&st.note),
                    "aurNoteTooltip" => qstr(&st.note_tooltip),
                    _ => qstr(&st.log_text),
                };
            }
        }
        let day = aur_updated_day(pkg.last_modified);
        match name {
            "kind" => qstr("aur"),
            "itemId" => qstr(&pkg.name),
            "name" => qstr(ap::card_name(&pkg.name)),
            "author" => qstr(ap::card_author(&pkg.maintainer)),
            "stats" => qstr(&ap::aur_stats_line(pkg.votes, pkg.popularity, &day)),
            "statsTooltip" => qstr(&ap::aur_stats_tooltip(pkg.votes, pkg.popularity, &day)),
            // У пакета AUR оценка — голоса, и знак у неё свой.
            "starGlyph" => qstr("👍"),
            "preview" => {
                let path = self
                    .rust()
                    .previews
                    .get(&pkg.name)
                    .cloned()
                    .unwrap_or_default();
                qstr(&file_url(&path))
            }
            "previewText" => qstr(pkg.icon()),
            // Признаки риска: значок перед строкой источника, список — в
            // подсказке. «Устарел» строка уже говорит словами — значком
            // его не дублируем.
            "metaLine" => {
                let risks = aur_tile_signals(pkg);
                let mark = if risks.iter().any(|s| s.key != "out_of_date") {
                    rexon_core::aur_trust::tile_mark(&risks)
                } else {
                    ""
                };
                qstr(&format!("{mark}{}", ap::aur_meta_line(&pkg.version, pkg.out_of_date)))
            }
            "metaTooltip" => {
                let risks = aur_tile_signals(pkg);
                let deep = aur_trust_entry(&pkg.name).is_some_and(|e| e.facts.is_some());
                let tip = rexon_core::aur_trust::tile_tooltip(&risks, deep);
                let ver = ap::aur_version_tooltip(&pkg.version);
                qstr(&if tip.is_empty() { ver } else { format!("{ver}\n\n{tip}") })
            }
            "metaWarn" => QVariant::from(&(pkg.out_of_date || !aur_tile_signals(pkg).is_empty())),
            "riskLevel" => qstr(
                rexon_core::aur_trust::level(&aur_tile_signals(pkg)).map(|l| l.key()).unwrap_or(""),
            ),
            "buttonText" | "buttonEnabled" | "buttonTooltip" => {
                let btn = ap::aur_install_button(pkg.installed, self.rust().aur_helper);
                match name {
                    "buttonText" => qstr(&btn.text),
                    "buttonEnabled" => QVariant::from(&btn.enabled),
                    _ => qstr(&btn.tooltip),
                }
            }
            "buttonState" => qstr(if pkg.installed { "true" } else { "false" }),
            "aurNote" | "aurNoteTooltip" | "aurLog" => qstr(""),
            // Пакет AUR в избранное не откладывается: избранное — это
            // товары магазина и темы с диска.
            "favoriteText" | "favoriteState" => qstr(""),
            "soundTheme" | "splashDemo" => qstr(""),
            "removeVisible" | "removeEnabled" | "current" => QVariant::from(&false),
            "removeTooltip" => qstr(""),
            _ => QVariant::default(),
        }
    }

    fn button_colors(&self, state: &QString) -> QMap<QMapPair_QString_QVariant> {
        let (bg, fg, border) = ap::card_button_colors(&state.to_string());
        let skin = ui_base::active_skin_colors();
        // Значение с решёткой — готовый цвет; без решётки — имя токена
        // действующего скина (контракт `card_button_colors`).
        let resolve = |v: &str| -> String {
            if v.is_empty() || v.starts_with('#') {
                v.to_string()
            } else {
                skin.get(v).cloned().unwrap_or_default()
            }
        };
        vmap(vec![
            ("bg", qstr(&resolve(bg))),
            ("fg", qstr(&resolve(fg))),
            ("border", qstr(&resolve(border))),
        ])
    }

    fn control_colors(&self, kind: &QString) -> QMap<QMapPair_QString_QVariant> {
        let skin = ui_base::active_skin_colors();
        let c = ap::control_colors(&kind.to_string());
        // Значения таблицы — ИМЕНА токенов действующего скина: своих красок
        // ни ядро отсюда, ни разметка не заводят.
        let token = |name: &str| -> String {
            if name.is_empty() {
                String::new()
            } else {
                skin.get(name).cloned().unwrap_or_default()
            }
        };
        vmap(vec![
            ("background", qstr(&token(c.bg))),
            ("hover", qstr(&token(c.hover))),
            ("border", qstr(&token(c.border))),
            ("hoverBorder", qstr(&token(c.hover_border))),
            ("color", qstr(&token(c.fg))),
            ("radius", QVariant::from(&c.radius)),
            ("paddingH", QVariant::from(&c.pad_h)),
            ("paddingV", QVariant::from(&c.pad_v)),
        ])
    }

    fn role_number(&self, name: &QString) -> i32 {
        let wanted = name.to_string();
        ROLE_NAMES
            .iter()
            .position(|n| *n == wanted)
            .map(|i| ROLE_BASE + i as i32)
            .unwrap_or(-1)
    }

    fn role_value_of(&self, item_id: &QString, name: &QString) -> QVariant {
        let id = item_id.to_string();
        let Some(card) = self.rust().cards.iter().find(|c| c.id() == id).cloned() else {
            return QVariant::default();
        };
        self.role_of(&card, Some(name.to_string().as_str()))
    }

}

// ============================================================================
// Действия страницы
// ============================================================================

impl qobject::AppearanceModel {
    /// Пересобрать всё, что зависит от состояния каталога, и позвать за
    /// данными. Один вход — чтобы ни один переключатель не забыл про
    /// половину страницы (это и был `_update_chrome` + `_reload` оригинала).
    fn resync(mut self: Pin<&mut Self>) {
        // Есть ли программа Conky — для подписи её раздела: после установки
        // виджета (и программы вместе с ним) подпись обязана смениться.
        if self.rust().view.category.key == "conky" {
            let installed = rexon_core::de_apply::have_conky();
            let installable = rexon_core::de_apply::live_conky_package().is_some();
            let mut me = self.as_mut().rust_mut();
            me.view.conky_installed = installed;
            me.view.conky_installable = installable;
            me.view.wayland = rexon_core::de_apply::Ctx::live().wayland;
        }
        // Витрины нет у объяснения («раздела нет в этом столе», «закрыто на
        // SteamOS») и у панели Plasma Login Manager: в магазин не ходим.
        let screen = {
            let me = self.rust();
            me.view.screen(&me.home)
        };
        let blocked = screen != ap::SCREEN_GRID;
        {
            let mut me = self.as_mut().rust_mut();
            me.screen = screen.to_string();
            if blocked {
                me.status.clear();
            }
        }
        if blocked {
            self.as_mut().clear_cards();
        }
        self.as_mut().sidebar_changed();
        self.as_mut().chrome_changed();
        self.as_mut().screen_changed();
        self.as_mut().status_changed();
        self.as_mut().page_changed();
        if !blocked {
            self.as_mut().start_request();
        }
    }

    /// Опустошить сетку.
    fn clear_cards(mut self: Pin<&mut Self>) {
        unsafe {
            self.as_mut().begin_reset_model();
            self.as_mut().rust_mut().cards.clear();
            self.as_mut().end_reset_model();
        }
        self.as_mut().count_changed();
    }

    /// Завести новый запрос каталога и сказать странице, что пора идти к
    /// хозяину. Ключ и поколение считает ядро.
    fn start_request(mut self: Pin<&mut Self>) {
        let (key, busy_status) = {
            let me = self.rust();
            let key = request_key(&me.view);
            let status = if me.view.is_local() {
                ap::scan_busy_status(&me.view.local_scan_which())
            } else if me.view.aur_mode() {
                ap::status_loading_aur()
            } else {
                ap::status_loading_catalog()
            };
            (key, status)
        };
        {
            let mut me = self.as_mut().rust_mut();
            let generation = me.requests.bump();
            me.requests.schedule(generation, &key);
            me.status = busy_status;
        }
        self.as_mut().status_changed();
        // Картинки прежнего раздела человеку больше не нужны, а канал нужен
        // новому: очередь меняет поколение, хозяин обрывает летящее.
        self.as_mut().cancel_previews();
        self.as_mut().catalog_wanted();
    }

    fn set_environment(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        {
            let mut me = self.as_mut().rust_mut();
            // Стили Qt и действующий стиль страница больше не держит: сетку
            // «Стиля приложений» собирает скан хозяина (`scan_disk`), а
            // текущий стиль отмечает сам скан.
            me.aur_helper = v
                .get("aurHelper")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }
        // Рабочий стол: фактически запущенный побеждает запомненный выбор —
        // решение принимает ядро (`initial_de`).
        let choice = ap::initial_de(s(&v, "detectedDe"), s(&v, "savedDe"), s(&v, "seenDe"));
        {
            let mut me = self.as_mut().rust_mut();
            me.view.de = choice.de_id.clone();
            me.detected_de = s(&v, "detectedDe").to_string();
            me.de_choices = ap::de_picker_choices(&choice.de_id, &me.detected_de);
        }
        self.as_mut().de_changed();
        self.resync();
    }

    fn set_login_screen(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
        let (plasma_login, steamos, can_open) =
            (flag("plasmaLogin"), flag("steamos"), flag("canOpen"));
        // Задет ли открытый раздел: он сменил вид (SDDM ↔ Plasma Login
        // Manager) или закрылся/открылся на SteamOS. Незадетый раздел не
        // перечитываем — витрина осталась бы та же, а запрос ушёл бы зря.
        let touched = {
            let mut me = self.as_mut().rust_mut();
            let key = me.view.category.key.clone();
            let was_blocked = me.view.steamos_blocked();
            me.plasma_login = plasma_login;
            me.plm_can_open = can_open;
            me.view.login_manager = v
                .get("loginManager")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            me.view.steamos = steamos;
            me.sidebar = categories::sidebar_for(&me.home, plasma_login);
            let mut touched = me.view.steamos_blocked() != was_blocked;
            // Открытую деталь товара не выдёргиваем из-под человека: раздел
            // пересоберётся, когда он из неё выйдет в другой.
            let in_detail = me.screen == "detail";
            if !in_detail && !me.sidebar.iter().any(|c| c.key == key) {
                // Открыт был раздел экрана входа прежнего вида — встаём в
                // новый на том же месте панели.
                let login = [categories::PLASMA_LOGIN_KEY, "sddm"];
                if let Some(cat) = me
                    .sidebar
                    .iter()
                    .find(|c| login.contains(&c.key.as_str()))
                    .cloned()
                {
                    me.view.category = cat;
                    me.view.page = 0;
                    me.view.total = 0;
                    me.total = 0;
                    me.view.query.clear();
                    touched = true;
                }
            }
            touched && !in_detail
        };
        if touched {
            // Раздел сменил вид — плитки прежнего вида уходят сразу, как и
            // при выборе раздела (`select_category`).
            self.as_mut().clear_cards();
            self.as_mut().resync();
        } else {
            self.as_mut().sidebar_changed();
            self.as_mut().screen_changed();
            // Пояснение «сейчас его показывает …» у открытого раздела входа.
            self.as_mut().chrome_changed();
        }
    }

    fn login_settings_failed(mut self: Pin<&mut Self>) {
        let text = self.plm_panel().open_failed;
        self.as_mut().rust_mut().status = text;
        self.as_mut().status_changed();
    }

    fn select_category(mut self: Pin<&mut Self>, key: &QString) {
        let key = key.to_string();
        {
            let me = self.rust();
            if me.view.category.key == key {
                return;
            }
        }
        let Some(cat) = self.rust().category(&key) else {
            return;
        };
        {
            let mut me = self.as_mut().rust_mut();
            me.view.category = cat;
            me.view.page = 0;
            me.view.total = 0;
            me.total = 0;
            // Запрос принадлежит разделу, из которого ушли: тащить его
            // дальше значит открыть соседний раздел уже отфильтрованным по
            // забытому слову. Оригинал на виджетах чистит и поле, и запрос
            // (`_select_category`); строку поиска подтягивает разметка по
            // `chromeChanged`.
            me.view.query.clear();
            // Под-выбор «вида» принадлежит разделу «Стиль приложений»:
            // уходя из него, забываем, чтобы он не тянулся в чужой раздел.
            me.view.style_sub = None;
            // Чип «Запущенные» принадлежит разделу Conky.
            me.view.running_only = false;
            // Фильтр тона — своему разделу (у Plasma он тоже не помнится).
            me.view.tone_filter.clear();
            me.requests.forget_page_prefetches();
        }
        // Открытая карточка темы принадлежит разделу, из которого ушли: под
        // шапкой нового раздела она оставалась на экране (приёмка
        // 10.10.2026: шапка «Обои-плагины», внутри — тема экрана входа).
        if self.rust().screen == "detail" {
            self.as_mut().close_detail();
        }
        // Плитки тоже принадлежат разделу, из которого ушли: пока едет
        // каталог нового («⏳ Загрузка каталога…»), под его шапкой стояли
        // значки и курсоры «Активных» (приёмка 24.10.2026, «GTK-темы»).
        // Сетку опустошаем сразу; из кеша новый раздел заполнит её мгновенно.
        self.as_mut().clear_cards();
        self.as_mut().resync();
        // Открыл раздел — сразу греем следующий: при переключении он
        // откроется из кеша мгновенно (прогрессивность оригинала).
        self.prioritize_prefetch();
    }

    fn set_source(mut self: Pin<&mut Self>, key: &QString) {
        let key = key.to_string();
        {
            let mut me = self.as_mut().rust_mut();
            if me.view.source == key {
                return;
            }
            me.view.source = key;
            me.view.page = 0;
        }
        self.resync();
    }

    fn set_lock_mode(mut self: Pin<&mut Self>, key: &QString) {
        let key = key.to_string();
        {
            let mut me = self.as_mut().rust_mut();
            if me.view.lock_mode == key {
                return;
            }
            me.view.lock_mode = key;
            me.view.page = 0;
        }
        self.resync();
    }

    fn set_style_sub(mut self: Pin<&mut Self>, key: &QString) {
        let key = key.to_string();
        {
            let mut me = self.as_mut().rust_mut();
            if me.view.style_sub.as_deref() == Some(key.as_str()) {
                return;
            }
            me.view.style_sub = Some(key);
            me.view.page = 0;
        }
        self.resync();
    }

    fn set_tone_filter(mut self: Pin<&mut Self>, key: &QString) {
        let key = key.to_string();
        {
            let mut me = self.as_mut().rust_mut();
            if me.view.tone_filter == key {
                return;
            }
            me.view.tone_filter = key;
            // Установленное приходит только с первой страницей магазина:
            // под фильтром со второй страницы сетка осталась бы пустой.
            me.view.page = 0;
        }
        self.resync();
    }

    fn set_sort(mut self: Pin<&mut Self>, key: &QString) {
        let key = key.to_string();
        {
            let mut me = self.as_mut().rust_mut();
            if me.view.sort == key {
                return;
            }
            me.view.sort = key;
            me.view.page = 0;
        }
        self.resync();
    }

    fn set_de(mut self: Pin<&mut Self>, de_id: &QString) {
        let picked = de_id.to_string();
        {
            let mut me = self.as_mut().rust_mut();
            // «Определять автоматически» — стол сеанса; подпись ручного
            // выбора снимается и тогда, когда стол каталога тот же.
            let (de_id, _) = ap::resolve_de_pick(&picked, &me.detected_de);
            let choices = ap::de_picker_choices(&de_id, &me.detected_de);
            if me.view.de == de_id && me.de_choices == choices {
                return;
            }
            me.view.de = de_id.clone();
            me.de_choices = choices;
            me.view.page = 0;
        }
        self.as_mut().de_changed();
        self.resync();
    }

    fn set_query(mut self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        // Что делать с набранным, решает ядро (`CatalogView::edit_query`):
        // поиск применяется по Enter, а ОЧИСТКА поля сбрасывает запрос
        // сразу — иначе запрос залипал на прошлом слове при визуально
        // пустом поле, и каталог оставался пустым навсегда.
        let outcome = self.as_mut().rust_mut().view.edit_query(&text);
        match outcome {
            // Набранное записано, ждём Enter. Шапку НЕ трогаем: свойство
            // `query` держит текст самого поля, и перезапись его на ходу
            // прыгала бы кареткой.
            ap::QueryEdit::Idle => {}
            // Обзоры диска ищут по уже прочитанному списку — без сети и
            // мгновенно, поэтому отбирают сразу при наборе, как страницы
            // KDE. Магазин посимвольно не дёргаем: страница применит слово
            // `submitQuery` после паузы ядра (`searchDelayMs`) или по Enter.
            //
            // Шапка оповещается и тогда: свойство `query` обязано идти за
            // ядром, иначе после смены раздела (там запрос сбрасывается)
            // поле не замечало сброса — для него запрос и раньше был пуст,
            // — и забытое слово оставалось в поле (19.10.2026, «MishMty»).
            ap::QueryEdit::Kept => {
                if self.rust().view.search_delay_ms() == 0 {
                    self.as_mut().rust_mut().view.page = 0;
                    self.resync();
                } else {
                    self.as_mut().chrome_changed();
                }
            }
            ap::QueryEdit::Cleared => {
                self.as_mut().chrome_changed();
                self.resync();
            }
        }
    }

    fn submit_query(mut self: Pin<&mut Self>) {
        {
            let mut me = self.as_mut().rust_mut();
            me.view.page = 0;
        }
        self.as_mut().chrome_changed();
        self.resync();
    }

    fn refresh(mut self: Pin<&mut Self>) {
        {
            let mut me = self.as_mut().rust_mut();
            me.requests.forget_page_prefetches();
            // «Обновить» у Cinnamon Spices — заново спросить сайт, а не кеш.
            me.site_force = me.view.spices_mode();
            // «Обновить» у сетки установленного — заново прочитать диск и
            // журнал установленных, а не отдать прежний скан из кеша: иначе
            // тема, положенная мимо Rexon, появлялась только после перезапуска.
            if me.view.is_local() || me.view.installed_view() {
                me.catalog_fresh = true;
            }
        }
        self.resync();
    }

    fn show_installed_fresh(self: Pin<&mut Self>) {
        self.reload_local_fresh();
    }

    fn set_running_only(mut self: Pin<&mut Self>, on: bool) {
        if self.rust().view.running_only == on {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            me.view.running_only = on;
            // Под «Запущенными» магазина нет (`installed_only`), а
            // установленное приходит с первой страницей.
            me.view.page = 0;
        }
        self.refresh();
    }

    fn prev_page(mut self: Pin<&mut Self>) {
        if !self.prev_enabled() {
            return;
        }
        self.as_mut().rust_mut().view.page -= 1;
        self.resync();
    }

    fn next_page(mut self: Pin<&mut Self>) {
        if !self.next_enabled() {
            return;
        }
        self.as_mut().rust_mut().view.page += 1;
        self.resync();
    }

    fn catalog_request(mut self: Pin<&mut Self>) -> QString {
        let outcome = self.as_mut().rust_mut().requests.fire();
        let ap::FireOutcome::Send { generation, key } = outcome else {
            return QString::default();
        };
        let me = self.rust();
        let body = json!({
            "generation": generation,
            "key": key,
            "kind": request_kind(&me.view),
            "storeKind": store_kind(&me.view),
            "categoryId": me.view.query_category_id(),
            "categoryKey": me.view.category.key,
            "page": me.view.page,
            "pageSize": ap::PAGE_SIZE,
            "sort": me.view.active_sort(),
            "query": me.view.query,
            "which": me.view.local_scan_which(),
            "aurKey": me.view.aur_cache_key(),
            "lockMode": me.view.lock_mode_effective(),
            "de": me.view.de,
            // Обойти кеш сканов диска: тему только что применили.
            "fresh": me.catalog_fresh,
            "force": me.site_force,
        });
        self.as_mut().rust_mut().catalog_fresh = false;
        self.as_mut().rust_mut().site_force = false;
        QString::from(&body.to_string())
    }

    fn catalog_answer(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        let key = s(&v, "key").to_string();
        // Ответ раздела, с которого человек уже ушёл, молча отбрасывается —
        // ключ и поколение сверяет ядро. ⚠️ `answer` отвечает «УСТАРЕЛ ЛИ»,
        // а не «принять ли»: с отрицанием сюда не проходил ни один свежий
        // ответ, и витрина оставалась пустой при живом хозяине.
        if self.as_mut().rust_mut().requests.answer(&key) {
            return;
        }
        // У раздела без витрины (объяснение, панели экрана входа и MATE,
        // разделы настроек) сетки нет: запоздавший ответ прежнего раздела
        // (из кеша он приходит без отправителя и считается свежим) писал
        // над разделом «Кнопки заголовка окна: 0».
        if !matches!(self.rust().screen.as_str(), ap::SCREEN_GRID | "detail") {
            return;
        }
        let error = s(&v, "error").to_string();
        let mut cards: Vec<Card> = Vec::new();
        if let Some(list) = v.get("items").and_then(Value::as_array) {
            for raw in list {
                if let Ok(item) = serde_json::from_value::<ThemeItem>(raw.clone()) {
                    let mut item = item;
                    // Местные темы приезжают со своим видом (`localKind`):
                    // у них другая кнопка и другой бейдж.
                    if let Some(kind) = raw.get("localKind").and_then(Value::as_str) {
                        if !kind.is_empty() {
                            item.local_kind = Some(kind.to_string());
                        }
                    }
                    // Имя без перевода — для поиска по оригинальному названию.
                    if let Some(src) = raw.get("sourceName").and_then(Value::as_str) {
                        item.source_name = src.to_string();
                    }
                    if let Some(ui) = RemovalUi::from_row(raw) {
                        self.as_mut().rust_mut().removal.insert(item.id.clone(), ui);
                    }
                    match SpiceExtra::from_row(raw) {
                        Some(x) => {
                            self.as_mut().rust_mut().spices.insert(item.id.clone(), x);
                        }
                        None => {
                            self.as_mut().rust_mut().spices.remove(&item.id);
                        }
                    }
                    cards.push(Card::Theme(Box::new(item)));
                }
            }
        }
        if let Some(list) = v.get("packages").and_then(Value::as_array) {
            for raw in list {
                if let Ok(pkg) = serde_json::from_value::<AurPackage>(raw.clone()) {
                    cards.push(Card::Aur(Box::new(pkg)));
                }
            }
        }
        // Действующий запрос обязателен ДЛЯ ВСЕХ тем с диска, откуда бы они
        // в сетку ни пришли: и для раздела-обзора целиком, и для
        // установленных тем, доклеенных сверху магазина в «Сплэш-экране» и
        // «Экране блокировки». Раньше поиск замечал только витрину магазина,
        // и рядом с одной найденной темой стояли все установленные.
        // Карточки магазина этой меркой не трогаем: там отбор сделан на
        // стороне магазина и по своим полям (описание, метки).
        {
            let query = self.rust().view.query.clone();
            if !query.is_empty() {
                cards.retain(|c| match c {
                    Card::Theme(it) if it.local_kind.is_some() => {
                        ap::local_item_matches(it, &query)
                    }
                    _ => true,
                });
            }
            // Фильтр тона цветовых схем и стилей Plasma (решение —
            // `lp::tone_passes`). Тон знаем только у тем с диска: витрина
            // магазина под сеткой при выбранном фильтре уходит, иначе под
            // «Тёмными» стояли бы светлые схемы магазина.
            if self.tone_bar_visible() {
                let filter = self.active_tone_filter();
                if filter != lp::TONE_ALL {
                    cards.retain(|c| match c {
                        Card::Theme(it) => lp::tone_passes(&it.id, &it.install_path, filter),
                        _ => false,
                    });
                }
            }
            // «Запущенные»: виджеты Conky с процессом сейчас (у них вид
            // плитки «active»), без магазина.
            if self.rust().view.running_only {
                cards.retain(|c| matches!(c, Card::Theme(it) if it.local_kind.as_deref() == Some("active")));
            }
            // Любой фильтр «только установленное» (`installed_only`):
            // плиток магазина в сетке нет, даже если ответ их принёс.
            if self.installed_only() {
                cards.retain(|c| matches!(c, Card::Theme(it) if it.local_kind.is_some()));
            }
        }
        let total = v.get("total").and_then(Value::as_i64).unwrap_or(0);
        let has_more = v.get("hasMore").and_then(Value::as_bool);
        {
            let mut me = self.as_mut().rust_mut();
            me.has_more = has_more;
            // Карточки экрана блокировки приезжают из чужих каталогов —
            // ядро переписывает им категорию, чтобы установщик тронул
            // именно компонент блокировки, а не всю тему.
            let lock_mode = me.view.lock_mode_effective().to_string();
            if me.view.category.key == "lockscreen" {
                let mut items: Vec<ThemeItem> = cards
                    .iter()
                    .filter_map(|c| match c {
                        Card::Theme(it) => Some((**it).clone()),
                        _ => None,
                    })
                    .collect();
                ap::stamp_lock_kind(&mut items, &lock_mode);
                let mut fixed = items.into_iter();
                for card in cards.iter_mut() {
                    if let Card::Theme(_) = card {
                        if let Some(it) = fixed.next() {
                            *card = Card::Theme(Box::new(it));
                        }
                    }
                }
            }
            me.total = total;
            me.view.total = total;
        }
        let shown = cards.len();
        unsafe {
            self.as_mut().begin_reset_model();
            self.as_mut().rust_mut().cards = cards;
            self.as_mut().end_reset_model();
        }
        // Подпись состояния зависит от вида раздела — решение у ядра.
        let status = {
            let me = self.rust();
            let query = me.view.query.trim();
            if !error.is_empty() {
                error
            } else if !query.is_empty() {
                // Действует поиск — так и сказать: обычное «Найдено» над
                // одной плиткой выглядело как пропавший магазин.
                let installed = me
                    .cards
                    .iter()
                    .filter(|c| matches!(c, Card::Theme(it) if it.local_kind.is_some()))
                    .count() as i64;
                // Под фильтром «только установленное» магазина нет —
                // нашлось ровно показанное.
                let found = if me.view.is_local() || me.view.installed_only() {
                    shown as i64
                } else if me.view.installed_view() {
                    installed + total.max(shown as i64 - installed)
                } else {
                    total.max(shown as i64)
                };
                ap::search_status(query, found)
            } else if shown == 0 && me.view.is_local() && !me.view.query.trim().is_empty() {
                // Поиск по установленному ничего не нашёл — сказать это
                // словами, а не «Установлено: 0» над пустой сеткой.
                ap::catalog_status(0, 0)
            } else if me.view.installed_view() {
                // Установленное сверху, магазин под ним: обе части.
                let installed = me
                    .cards
                    .iter()
                    .filter(|c| matches!(c, Card::Theme(it) if it.local_kind.is_some()))
                    .count();
                // Под фильтром «только установленное» витрины магазина нет
                // (`installed_only`) — «Ничего не найдено» про неё было бы
                // неправдой.
                if me.view.installed_only() {
                    ap::installed_status(installed)
                } else {
                    ap::combined_status(installed, total, shown - installed)
                }
            } else if me.view.is_local() {
                let which = me.view.local_scan_which();
                ap::scan_status(&which, shown).unwrap_or_else(|| {
                    if me.view.category.key == "favorites" {
                        ap::favorites_status(shown)
                    } else {
                        ap::local_category_status(&me.view.category.name, shown)
                    }
                })
            } else {
                ap::catalog_status(total, shown)
            }
        };
        self.as_mut().rust_mut().status = status;
        self.as_mut().count_changed();
        self.as_mut().status_changed();
        self.as_mut().page_changed();
        // Местным темам без своей картинки рисуем кадр — до сети, а не после:
        // сети им и не нужно, а плитка не должна секунду стоять пустой.
        self.as_mut().draw_local_placeholders();
        // Картинки показанной страницы — все разом, а не по одной на плитку:
        // сколько их лететь одновременно и в каком порядке, решает очередь
        // ядра, и живая плитка всегда обгоняет фоновый прогрев.
        self.as_mut().enqueue_page_previews();
        // …и следом кеш следующей страницы, чтобы «Вперёд» открылось без сети.
        self.as_mut().prefetch_next_page();
    }

    fn set_preview(mut self: Pin<&mut Self>, item_id: &QString, path: &QString) {
        let id = item_id.to_string();
        let path = path.to_string();
        let row = {
            let mut me = self.as_mut().rust_mut();
            me.previews.insert(id.clone(), path);
            me.cards.iter().position(|c| c.id() == id)
        };
        // Тот же файл — картинка открытой панели деталей: качать её вторично
        // незачем, а показать надо сразу.
        let for_detail = {
            let me = self.rust();
            me.detail.as_ref().is_some_and(|it| it.id == id) && me.detail_frame.is_empty()
        };
        if for_detail {
            {
                let mut me = self.as_mut().rust_mut();
                let path = me.previews.get(&id).cloned().unwrap_or_default();
                me.detail_frame = path;
                me.detail_frame_text = String::new();
            }
            self.as_mut().detail_frame_changed();
        }
        let Some(row) = row else { return };
        let index = self.as_ref().index(row as i32, 0, &QModelIndex::default());
        let mut roles = cxx_qt_lib::QVector::<i32>::default();
        roles.append(ROLE_BASE + ROLE_PREVIEW as i32);
        self.as_mut().data_changed(&index, &index, &roles);
    }

    fn set_favorites(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        let Some(list) = v.as_array() else { return };
        let ids: BTreeSet<String> = list
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        self.as_mut().rust_mut().favorites = ids;
        self.as_mut().detail_changed();
        // Отметка «в избранном» живёт и на плитках: перерисовываем их
        // ровно по двум ролям, а не сбросом всей модели.
        let rows = self.rust().cards.len();
        if rows > 0 {
            let first = self.as_ref().index(0, 0, &QModelIndex::default());
            let last = self
                .as_ref()
                .index(rows as i32 - 1, 0, &QModelIndex::default());
            let mut roles = cxx_qt_lib::QVector::<i32>::default();
            roles.append(ROLE_BASE + ROLE_FAVORITE_TEXT as i32);
            roles.append(ROLE_BASE + ROLE_FAVORITE_STATE as i32);
            self.as_mut().data_changed(&first, &last, &roles);
        }
    }

    fn set_status_text(mut self: Pin<&mut Self>, text: &QString) {
        self.as_mut().rust_mut().status = text.to_string();
        self.as_mut().status_changed();
    }
}

/// Ключ запроса: тем же ключом ядро сверяет ответ и отбрасывает чужой.
///
/// Своего правила здесь нет — ключ собирает ядро (`CatalogView::request_key`),
/// одинаково для магазина, AUR и обзоров диска.
fn request_key(view: &ap::CatalogView) -> String {
    view.request_key()
}

/// Откуда брать данные раздела.
/// Отметка времени файла темы в секундах эпохи. `0` — файла нет.
///
/// Ею ключ кеша плейсхолдера отличает «ту же тему» от «той же темы после
/// правки»: у цветовой схемы кадр рисуется по её содержимому.
fn file_stamp(path: &str) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn request_kind(view: &ap::CatalogView) -> &'static str {
    // Под фильтром «только установленное» магазин не спрашиваем — только
    // диск (`CatalogView::installed_only`).
    if view.installed_only() {
        return "local";
    }
    // Установленное и страница магазина одним запросом; магазин — тот же,
    // что без установленного (`store_kind`).
    if view.installed_view() {
        "combined"
    } else {
        store_kind(view)
    }
}

/// Источник магазина раздела (или обзор диска).
fn store_kind(view: &ap::CatalogView) -> &'static str {
    if view.is_local() {
        "local"
    } else if view.aur_mode() {
        "aur"
    } else if view.both_mode() {
        "both"
    } else if view.spices_mode() {
        "spices"
    } else {
        "ocs"
    }
}

impl cxx_qt::Initialize for qobject::AppearanceModel {
    fn initialize(mut self: Pin<&mut Self>) {
        // Подписка на смену скина: одна строка на страницу, рассылка общая
        // (`crate::skin`). Снимается в `Drop` состояния.
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::AppearanceModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
        let de = self.rust().view.de.clone();
        let detected = self.rust().detected_de.clone();
        self.as_mut().rust_mut().de_choices = ap::de_picker_choices(&de, &detected);
    }
}

// ============================================================================
// Панель деталей
// ============================================================================

impl qobject::AppearanceModel {
    // ── Неизменные подписи ──────────────────────────────────────────────
    fn detail_back_text(&self) -> QString {
        QString::from(&ap::detail_back_btn())
    }
    fn detail_uninstall_text(&self) -> QString {
        QString::from(&ap::detail_uninstall_btn())
    }
    fn detail_site_text(&self) -> QString {
        QString::from(&ap::detail_open_site_btn())
    }
    fn detail_site_tooltip(&self) -> QString {
        QString::from(&ap::detail_open_site_tooltip())
    }
    fn detail_copy_link_text(&self) -> QString {
        QString::from(&ap::detail_copy_link_btn())
    }
    fn detail_files(&self) -> QString {
        let me = self.rust();
        match me.detail.as_ref() {
            // Блок выбора — только у товара магазина: у темы с диска и пакета
            // AUR файлов магазина нет.
            Some(it) if it.local_kind.is_none() && me.aur_detail.is_none() => {
                QString::from(&ap::store_file_choices_json(it))
            }
            _ => QString::from("[]"),
        }
    }
    fn detail_files_title(&self) -> QString {
        QString::from(&ap::detail_files_title())
    }
    fn detail_files_hint(&self) -> QString {
        QString::from(&ap::detail_files_hint())
    }
    fn detail_file_will_install(&self) -> QString {
        QString::from(&ap::detail_file_will_install())
    }
    fn detail_file_install_text(&self) -> QString {
        QString::from(&ap::detail_file_install_btn())
    }
    fn detail_copy_link_tooltip(&self) -> QString {
        QString::from(&ap::detail_copy_link_tooltip())
    }
    fn detail_apply_local_text(&self) -> QString {
        // Активную тему применять повторно незачем — тогда на кнопке бейдж;
        // у дополнения и виджета — обратное действие (решение ядра).
        QString::from(&self.detail_local_button().0)
    }

    /// Кнопка действия местной темы на панели деталей: (подпись, можно ли).
    fn detail_local_button(&self) -> (String, bool) {
        let detail = self.rust().detail.as_ref();
        let kind = detail.and_then(|it| it.local_kind.clone()).unwrap_or_default();
        let id = detail.map(|it| it.id.clone()).unwrap_or_default();
        ap::detail_local_button(&kind, &id)
    }
    fn detail_preview_tooltip(&self) -> QString {
        QString::from(&ap::detail_preview_tooltip())
    }
    /// Кнопка плитки пакета AUR рядом с «Установить»: страница пакета на
    /// сайте (так в оригинале; щелчок по самой плитке ведёт туда же).
    fn aur_open_text(&self) -> QString {
        QString::from(&ap::aur_open_btn())
    }
    fn aur_open_tooltip(&self) -> QString {
        QString::from(&ap::aur_open_tooltip())
    }
    fn sound_preview_label(&self) -> QString {
        QString::from(&ap::sound_preview_label())
    }
    fn sound_play_all_tooltip(&self) -> QString {
        QString::from(&ap::sound_play_all_tooltip())
    }
    fn sound_missing_tooltip(&self) -> QString {
        QString::from(&ap::sound_missing_tooltip())
    }
    fn sound_store_preview_text(&self) -> QString {
        QString::from(&ap::sound_store_preview_btn())
    }
    fn sound_store_preview_tooltip(&self) -> QString {
        QString::from(&ap::sound_store_preview_tooltip())
    }
    fn splash_demo_label(&self) -> QString {
        QString::from(&rexon_core::appearance_installer::splash_demo_label())
    }
    fn splash_demo_tooltip(&self) -> QString {
        QString::from(&rexon_core::appearance_installer::splash_demo_tooltip())
    }
    fn detail_sound_theme(&self) -> QString {
        let theme = self.rust().detail.as_ref().and_then(|it| ap::local_sound_theme(&it.id));
        QString::from(&theme.unwrap_or_default())
    }
    fn detail_sound_store(&self) -> bool {
        self.rust().detail.as_ref().is_some_and(|it| ap::is_store_sound_item(it))
    }
    fn detail_download_url(&self) -> QString {
        match self.rust().detail.as_ref() {
            Some(it) => QString::from(&it.download_url),
            None => QString::default(),
        }
    }
    fn sound_tooltip(&self, event: &QString) -> QString {
        QString::from(&ap::sound_preview_tooltip(&event.to_string()))
    }
    fn sound_store_progress(&self, done: f64, total: f64) -> QString {
        QString::from(&ap::sound_store_progress(
            crate::qvalue::qml_to_i64(done),
            crate::qvalue::qml_to_i64(total),
        ))
    }
    fn detail_preview_width(&self) -> i32 {
        ap::DETAIL_PREVIEW_W
    }
    fn detail_preview_height(&self) -> i32 {
        ap::DETAIL_PREVIEW_H
    }

    // ── Содержимое открытого товара ─────────────────────────────────────
    fn detail_is_local(&self) -> bool {
        self.rust()
            .detail
            .as_ref()
            .and_then(|it| it.local_kind.as_ref())
            .is_some()
    }

    fn detail_name(&self) -> QString {
        match &self.rust().detail {
            Some(it) => QString::from(ap::card_name(&it.name)),
            None => QString::default(),
        }
    }

    fn detail_meta(&self) -> QString {
        let Some(item) = self.rust().detail.as_ref() else {
            return QString::default();
        };
        // У местной темы поле автора уже несёт «<значок> <Тип> · <бейдж>»:
        // лицензии, даты и версии у неё нет вовсе.
        if item.local_kind.is_some() {
            return QString::from(ap::card_author(&item.author));
        }
        // У пакета AUR — сопровождающий, лицензия, дата и версия из паспорта
        // (до ответа RPC — с плитки).
        if let Some(d) = self.rust().aur_detail.as_ref() {
            let (license, updated) = match d.info.as_ref() {
                Some(i) => (i.license.join(", "), rexon_core::aur::epoch_day(i.last_modified)),
                None => (String::new(), rexon_core::aur::epoch_day(d.pkg.last_modified)),
            };
            let line = ap::detail_meta_line(&item.author, &license, &updated, &item.version);
            return QString::from(&format!("🛠️ AUR • {line}"));
        }
        let line = ap::detail_meta_line(&item.author, &item.license, &item.updated, &item.version);
        QString::from(&ap::detail_meta_with_size(&line, item.download_size))
    }

    fn detail_stars(&self) -> QString {
        // У пакета AUR звёзд нет: голоса и популярность — в его сведениях.
        if self.rust().aur_detail.is_some() {
            return QString::default();
        }
        match self.rust().detail.as_ref() {
            Some(it) if it.local_kind.is_none() && it.id.starts_with(cs::ID_PREFIX) => {
                let score = self.rust().spices.get(&it.id).map_or(0, |x| x.score);
                QString::from(&cs::detail_stars(score))
            }
            Some(it) if it.local_kind.is_none() => {
                QString::from(&ap::detail_stars_label(it.rating_stars()))
            }
            _ => QString::default(),
        }
    }

    fn detail_downloads(&self) -> QString {
        if self.rust().aur_detail.is_some() {
            return QString::default();
        }
        match self.rust().detail.as_ref() {
            // Чисел скачиваний у Cinnamon Spices нет — строку не показываем.
            Some(it) if it.id.starts_with(cs::ID_PREFIX) => QString::default(),
            Some(it) if it.local_kind.is_none() => {
                QString::from(&ap::detail_downloads_label(it.downloads))
            }
            _ => QString::default(),
        }
    }

    fn detail_path(&self) -> QString {
        let me = self.rust();
        let Some(item) = me.detail.as_ref() else {
            return QString::default();
        };
        // Пакет ставит менеджер пакетов — каталога установки у раздела нет.
        if me.aur_detail.is_some() {
            return QString::default();
        }
        let cat = categories::category_by_id(&me.home, item.category_id);
        let cat_name = cat
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("cat {}", item.category_id));
        // У местной темы путь известен с диска; таблица категорий его не
        // знает (виртуальные разделы своего каталога установки не имеют).
        let path = if item.local_kind.is_some() && !item.install_path.is_empty() {
            item.install_path.clone()
        } else {
            cat.map(|c| c.install_dir).unwrap_or_else(|| "?".to_string())
        };
        QString::from(&ap::detail_path_label(&cat_name, &path))
    }

    fn detail_description(&self) -> QString {
        let me = self.rust();
        let Some(item) = me.detail.as_ref() else {
            return QString::default();
        };
        let was_translated = !item.description_translated.is_empty();
        let accent = ui_base::active_skin_colors()
            .get("accent")
            .cloned()
            .unwrap_or_default();
        QString::from(&ap::description_html(
            &item.description,
            // ⚠️ ВИДИМОЕ описание (`display_description` — перевод если есть,
            // иначе оригинал), а не поле перевода. Здесь стояло
            // `description_translated`, и товар без перевода показывал «Без
            // описания» вместо своего английского текста: «показать перевод»
            // это ВТОРОЕ положение переключателя, а первое — просто «что есть».
            // У Python ровно так и было (`_desc_trans = ... if was_translated
            // else _desc_orig`), и живой прогон 21.09 нашёл расхождение сразу,
            // как только перевод стало можно выключить (нет модели — нет
            // перевода, а описание есть).
            item.display_description(),
            was_translated,
            me.detail_show_original,
            &accent,
        ))
    }

    fn detail_button(&self) -> ap::InstallButton {
        match self.rust().detail.as_ref() {
            Some(it) if it.local_kind.is_none() && it.id.starts_with(cs::ID_PREFIX) => {
                let x = self.rust().spices.get(&it.id);
                cs::detail_button_for(it.installed, x.is_some_and(|x| x.update), x.is_some_and(|x| x.apply))
            }
            Some(it) => {
                let can_apply = ap::store_item_can_apply(&self.rust().home, it);
                ap::install_button(it.installed, it.applied, can_apply)
            }
            None => ap::install_button(false, false, false),
        }
    }

    fn detail_install_text(&self) -> QString {
        QString::from(&self.detail_button().text)
    }
    fn detail_install_tooltip(&self) -> QString {
        QString::from(&self.detail_button().tooltip)
    }
    fn detail_install_state(&self) -> QString {
        QString::from(self.detail_button().state)
    }

    fn detail_uninstall_visible(&self) -> bool {
        // Пакет AUR — нет: удаление товара магазина его не снимет, а пакет
        // снимает менеджер пакетов. Тема с диска — по решению ядра
        // (`theme_removal`): у плитки без пути кнопки нет.
        if self.rust().aur_detail.is_some() {
            return false;
        }
        match self.rust().detail.as_ref() {
            Some(it) if it.local_kind.is_some() => self.detail_removal().visible,
            Some(it) => it.installed,
            None => false,
        }
    }

    fn detail_uninstall_enabled(&self) -> bool {
        match self.rust().detail.as_ref() {
            Some(it) if it.local_kind.is_some() => self.detail_removal().enabled,
            // Товар магазина: решение хозяина пришло с деталями (включённую
            // тему снять нельзя); пока не пришло — можно, хозяин проверит
            // ещё раз при снятии.
            Some(it) => self.rust().removal.get(&it.id).is_none_or(|r| r.enabled),
            None => false,
        }
    }

    fn detail_uninstall_tooltip(&self) -> QString {
        match self.rust().detail.as_ref() {
            Some(it) if it.local_kind.is_some() => QString::from(&self.detail_removal().tooltip),
            Some(it) => QString::from(&self.rust().removal.get(&it.id).map(|r| r.tooltip.clone()).unwrap_or_default()),
            _ => QString::default(),
        }
    }

    /// Решение «Удалить» для открытой темы с диска.
    fn detail_removal(&self) -> RemovalUi {
        let me = self.rust();
        me.detail
            .as_ref()
            .and_then(|it| me.removal.get(&it.id).cloned())
            .unwrap_or_default()
    }

    fn removal_undo_text(&self) -> QString {
        QString::from(&rexon_core::theme_removal::removal_undo_btn())
    }

    fn removal_undo_ms(&self) -> i32 {
        rexon_core::theme_removal::UNDO_TIMEOUT_MS
    }

    fn removal_countdown_text(&self, seconds: i32) -> QString {
        QString::from(&rexon_core::theme_removal::removal_countdown_text(
            u64::try_from(seconds).unwrap_or(0),
        ))
    }

    fn detail_favorite_text(&self) -> QString {
        let me = self.rust();
        let on = me
            .detail
            .as_ref()
            .is_some_and(|it| me.favorites.contains(&it.id));
        QString::from(&ap::favorite_button(on).text)
    }

    fn detail_apply_local_enabled(&self) -> bool {
        self.detail_local_button().1
    }

    fn detail_autostart_visible(&self) -> bool {
        self.rust()
            .detail
            .as_ref()
            .is_some_and(|it| ap::detail_autostart_visible(&it.id))
    }

    /// Вид местной плитки и её имя по номеру `local:<вид>:<имя>`.
    fn detail_local_parts(&self) -> Option<(String, String)> {
        let it = self.rust().detail.as_ref()?;
        let rest = it.id.strip_prefix("local:")?;
        let (kind, name) = rest.split_once(':')?;
        Some((kind.to_string(), name.to_string()))
    }

    fn detail_style_config(&self) -> QString {
        let Some((kind, name)) = self.detail_local_parts() else {
            return QString::from("{}");
        };
        if kind != "widgetstyle" && kind != "kvantum" {
            return QString::from("{}");
        }
        let b = rexon_core::themes::style_config_button_live(&kind, &name);
        QString::from(&json!({"visible": b.visible, "enabled": b.enabled,
                              "text": b.text, "tooltip": b.tooltip}).to_string())
    }

    fn detail_gtk4(&self) -> QString {
        use rexon_core::gtk4_theme as g4;
        let Some((_, name)) = self.detail_local_parts().filter(|(k, _)| k == "gtk") else {
            return QString::from("{}");
        };
        let home = std::env::var("HOME").unwrap_or_default();
        let de = self.rust().view.de.clone();
        let t = g4::toggle(&name, &g4::theme_gtk4_live(&name, &home), g4::enabled_live(), g4::mode_follows(&de));
        QString::from(&serde_json::to_string(&t).unwrap_or_else(|_| "{}".into()))
    }

    fn detail_deco_config(&self) -> QString {
        let Some(it) = self.rust().detail.as_ref().filter(|it| it.id.starts_with("local:aurorae:")) else {
            return QString::from("{}");
        };
        let b = rexon_core::kdecoration::config_button_live(&it.id, &it.install_path, &it.name);
        QString::from(&serde_json::to_string(&b).unwrap_or_else(|_| "{}".into()))
    }

    fn detail_scheme_edit(&self) -> QString {
        use rexon_core::color_scheme_edit as cse;
        let me = self.rust();
        let visible = me
            .detail
            .as_ref()
            .is_some_and(|it| cse::scheme_of_item(&it.id, &it.install_path).is_some());
        let mut v = serde_json::to_value(cse::editor_texts(kde_scheme_editor_present())).unwrap_or_default();
        if let Some(obj) = v.as_object_mut() {
            obj.insert("visible".into(), visible.into());
        }
        QString::from(&v.to_string())
    }

    fn detail_autostart_text(&self) -> QString {
        QString::from(&ap::detail_autostart_text())
    }

    fn detail_preview(&self) -> QString {
        QString::from(&file_url(&self.rust().detail_frame))
    }

    fn detail_preview_text(&self) -> QString {
        QString::from(&self.rust().detail_frame_text)
    }

    fn detail_item_id(&self) -> QString {
        match self.rust().detail.as_ref() {
            Some(it) => QString::from(&it.id),
            None => QString::default(),
        }
    }

    fn action_busy(&self) -> bool {
        self.rust().action_busy
    }
    fn progress_visible(&self) -> bool {
        let me = self.rust();
        me.action_busy && ap::action_shows_progress(&me.action_kind)
    }
    fn install_percent(&self) -> i32 {
        self.rust().install_percent as i32
    }
    fn detail_status(&self) -> QString {
        QString::from(&self.rust().detail_status)
    }

    fn detail_site_url(&self) -> QString {
        if let Some(d) = self.rust().aur_detail.as_ref() {
            return QString::from(&d.pkg.aur_url());
        }
        match self.rust().detail.as_ref() {
            Some(it) if it.local_kind.is_none() && self.spice_page(&it.id).is_some() => {
                QString::from(&self.spice_page(&it.id).unwrap_or_default())
            }
            Some(it) if it.local_kind.is_none() && !it.id.is_empty() => {
                QString::from(&format!("https://www.pling.com/p/{}/", it.id))
            }
            _ => QString::default(),
        }
    }

    fn copy_detail_link(mut self: Pin<&mut Self>) {
        let url = match (self.rust().aur_detail.as_ref(), self.rust().detail.as_ref()) {
            (Some(d), _) => d.pkg.aur_url(),
            (None, Some(it)) if it.local_kind.is_none() => {
                self.spice_page(&it.id).unwrap_or_else(|| ap::store_page_url(&it.id))
            }
            _ => String::new(),
        };
        if url.is_empty() {
            return;
        }
        // Функция шима зовётся через мост синглтона `Action` (второе
        // объявление дало бы дубль символа на линковке, см. `we_dev_page`).
        crate::we_action::qobject::rexon_we_copy_to_clipboard(&QString::from(&url));
        self.as_mut().rust_mut().status = ap::link_copied_status(&url);
        self.as_mut().status_changed();
    }

    fn is_animated_preview(&self, url: &QString) -> bool {
        let Some(path) = path_of_file_url(&url.to_string()) else {
            return false;
        };
        let mut head = [0u8; 32];
        let n = std::fs::File::open(&path)
            .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
            .unwrap_or(0);
        ap::is_animated_image_header(&head[..n])
    }

    fn site_url_of(&self, item_id: &QString) -> QString {
        let id = item_id.to_string();
        match self.rust().cards.iter().find(|c| c.id() == id) {
            Some(Card::Aur(pkg)) => QString::from(&pkg.aur_url()),
            Some(Card::Theme(it)) if it.local_kind.is_none() && self.spice_page(&it.id).is_some() => {
                QString::from(&self.spice_page(&it.id).unwrap_or_default())
            }
            Some(Card::Theme(it)) if it.local_kind.is_none() && !it.id.is_empty() => {
                QString::from(&format!("https://www.pling.com/p/{}/", it.id))
            }
            _ => QString::default(),
        }
    }

    fn detail_item_json(&self) -> QString {
        match self.rust().detail.as_ref() {
            Some(it) => match serde_json::to_string(&**it) {
                Ok(text) => QString::from(&text),
                Err(_) => QString::default(),
            },
            None => QString::default(),
        }
    }

    fn detail_button_colors(&self, state: &QString) -> QMap<QMapPair_QString_QVariant> {
        let (bg, fg, border) = ap::detail_button_colors(&state.to_string());
        let skin = ui_base::active_skin_colors();
        let resolve = |v: &str| -> String {
            if v.is_empty() || v.starts_with('#') {
                v.to_string()
            } else {
                skin.get(v).cloned().unwrap_or_default()
            }
        };
        vmap(vec![
            ("bg", qstr(&resolve(bg))),
            ("fg", qstr(&resolve(fg))),
            ("border", qstr(&resolve(border))),
        ])
    }

    // ── Действия ────────────────────────────────────────────────────────
    fn open_detail(mut self: Pin<&mut Self>, item_id: &QString) {
        let id = item_id.to_string();
        let found = self
            .rust()
            .cards
            .iter()
            .find(|c| c.id() == id)
            .and_then(|c| match c {
                Card::Theme(it) => Some(((**it).clone(), None)),
                // Пакет AUR — своя карточка внутри программы, как у товара
                // магазина: паспорт RPC, зависимости, комментарии со страницы
                // пакета. Общая часть панели (имя, описание, картинка,
                // комментарии) берёт товар, собранный из плитки.
                Card::Aur(pkg) => Some((aur_detail_item(pkg), Some((**pkg).clone()))),
            });
        let Some((mut item, aur_pkg)) = found else { return };
        // Готовое превью плитки становится первым кадром — иначе панель
        // деталей на мгновение показывала бы пустоту там, где картинка уже
        // есть на диске.
        if item.preview_local.is_empty() {
            item.preview_local = tile_picture(&self.rust().previews, &item);
        }
        let frame = item.preview_local.clone();
        let local = item.local_kind.is_some();
        let id = item.id.clone();
        let is_aur = aur_pkg.is_some();
        {
            let mut me = self.as_mut().rust_mut();
            // Карточка пакета: картинка с плитки — нулевой кадр; если взять
            // картинку негде (проект не на GitHub/GitLab/Codeberg), на её
            // месте тот же значок, что на плитке.
            me.aur_detail = aur_pkg.map(|pkg| {
                // Проверка источника, сделанная раньше, видна сразу; свежая
                // придёт вслед за паспортом.
                let trust = aur_trust_entry(&pkg.name);
                Box::new(AurDetail {
                    pkg,
                    trust,
                    ..AurDetail::default()
                })
            });
            me.detail_show_original = false;
            // Итог прошлого товара к новому не относится — гасим (дефект
            // оригинала: сообщение висело на чужой панели). Полоса хода и
            // запертые кнопки — оттуда же.
            me.detail_status.clear();
            me.action_busy = false;
            me.action_kind.clear();
            me.install_percent = 0;
            me.detail_frame = frame.clone();
            let aur_icon = me
                .aur_detail
                .as_ref()
                .filter(|d| rexon_core::aur::detail_image_sources(&d.pkg.upstream_url).is_empty())
                .map(|d| d.pkg.icon().to_string());
            // Местная тема в магазин за картинкой не ходит: нет файла на
            // диске — значит, превью не будет вовсе, и ждать его незачем.
            me.detail_frame_text = if !frame.is_empty() {
                String::new()
            } else if let Some(icon) = aur_icon {
                icon
            } else if local {
                ap::detail_preview_unavailable()
            } else {
                ap::detail_preview_loading()
            };
            // Галерея и комментарии ПРЕДЫДУЩЕГО товара — чужие; их номер,
            // кадры и счётчик к новому не относятся.
            me.gallery_src.clear();
            me.gallery_loaded.clear();
            me.gallery_failed.clear();
            me.gallery_index = 0;
            me.gallery_known = 0;
            me.gallery_finished = false;
            me.gallery_user_nav = false;
            me.comments.clear();
            me.comments_page = 0;
            me.comments_total = None;
            // Картинка плитки пакета — сразу нулевой кадр: карточка
            // досылает остальные (README) кадрами по номерам.
            if is_aur && !frame.is_empty() {
                me.gallery_loaded.insert(0, frame.clone());
            }
            me.detail = Some(Box::new(item));
            me.screen = "detail".to_string();
        }
        self.as_mut().comments_changed();
        self.as_mut().detail_changed();
        self.as_mut().detail_frame_changed();
        self.as_mut().detail_status_changed();
        self.as_mut().install_changed();
        self.as_mut().screen_changed();
        // Местная тема лежит на диске целиком — в магазин за ней не ходим.
        if !local {
            self.as_mut().detail_wanted(QString::from(&id));
        }
    }

    fn close_detail(mut self: Pin<&mut Self>) {
        {
            let mut me = self.as_mut().rust_mut();
            me.detail = None;
            me.aur_detail = None;
            me.screen = "grid".to_string();
        }
        self.as_mut().detail_changed();
        self.as_mut().screen_changed();
    }

    fn detail_answer(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        let Ok(item) = serde_json::from_value::<ThemeItem>(v.clone()) else {
            return;
        };
        // Ответ про товар, который уже закрыли, молча отбрасываем.
        {
            let me = self.rust();
            match me.detail.as_ref() {
                Some(open) if open.id == item.id => {}
                _ => return,
            }
        }
        // «Удалить» поставленного товара: можно ли (включённую тему — нет).
        if item.local_kind.is_none() {
            match RemovalUi::from_row(&v) {
                Some(ui) => {
                    self.as_mut().rust_mut().removal.insert(item.id.clone(), ui);
                }
                None => {
                    self.as_mut().rust_mut().removal.remove(&item.id);
                }
            }
        }
        let frame = {
            let mut me = self.as_mut().rust_mut();
            // Картинку, которая уже показана, не теряем: полные детали её
            // не несут (там только ссылки).
            let shown = me.detail_frame.clone();
            let mut item = item;
            if item.preview_local.is_empty() {
                item.preview_local = shown.clone();
            }
            me.detail = Some(Box::new(item));
            shown
        };
        // Карусель — РОВНО как на сайте: без первого слота (он «лицо
        // товара» и остаётся превью плитки). Список собирает ядро, оно же
        // гонит уменьшенные копии в полное разрешение.
        let (id, local) = {
            let me = self.rust();
            match me.detail.as_ref() {
                Some(it) => (it.id.clone(), it.local_kind.is_some()),
                None => (String::new(), true),
            }
        };
        {
            let mut me = self.as_mut().rust_mut();
            let (urls, known) = match me.detail.as_ref() {
                Some(it) => (
                    ap::gallery_urls(&it.preview_urls, &it.preview_url),
                    it.preview_urls.len() as i64,
                ),
                None => (Vec::new(), 0),
            };
            me.gallery_known = known;
            me.gallery_src = urls;
            // Готовое превью плитки — нулевой кадр карусели: качать его
            // второй раз незачем.
            if !frame.is_empty() {
                me.gallery_loaded.insert(0, frame.clone());
            }
        }
        self.as_mut().detail_changed();
        self.as_mut().sync_frame();
        // У Cinnamon Spices комментариев для программ нет.
        if !local && !id.is_empty() && !id.starts_with(cs::ID_PREFIX) {
            self.as_mut()
                .comments_wanted(QString::from(&id), QString::from("0"));
        }
    }

    fn description_answer(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        let id = v.get("itemId").and_then(Value::as_str).unwrap_or_default();
        let translated = v.get("translated").and_then(Value::as_str).unwrap_or_default();
        // Пустой перевод — не перевод: он бы только зажёг ссылку «показать
        // оригинал» над пустым местом (`description_html` держится за
        // `was_translated`).
        if id.is_empty() || translated.trim().is_empty() {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            // Ответ про товар, который уже закрыли или сменили, молча
            // отбрасываем — ровно как в `detail_answer`. Сторож у ядра свой, но
            // между его проверкой и этой строкой очередь окна успевает
            // повернуться, и последнее слово о том, что открыто, — за страницей.
            match me.detail.as_mut() {
                Some(open) if open.id == id => open.description_translated = translated.to_string(),
                _ => return,
            }
            // Переключатель «перевод ⇄ оригинал» остаётся в том положении, в
            // каком его оставил человек: он мог нажать «показать оригинал» до
            // прихода перевода — сбросить его значило бы дёрнуть текст под
            // рукой.
        }
        self.as_mut().detail_changed();
    }

    fn toggle_description(mut self: Pin<&mut Self>) {
        let now = self.rust().detail_show_original;
        self.as_mut().rust_mut().detail_show_original = !now;
        self.as_mut().detail_changed();
    }

    fn action_begin(self: Pin<&mut Self>, item_id: &QString, kind: &QString) {
        let id = item_id.to_string();
        let kind = kind.to_string();
        let name = {
            let me = self.rust();
            me.cards
                .iter()
                .find(|c| c.id() == id)
                .map(|c| match c {
                    // У местной темы имя зашито в номер (`local:<вид>:<имя>`)
                    // — берём его оттуда, как это делает оригинал.
                    Card::Theme(it) => ap::local_theme_name(&it.id, &it.name),
                    Card::Aur(p) => p.name.clone(),
                })
                .or_else(|| {
                    me.detail
                        .as_ref()
                        .filter(|it| it.id == id)
                        .map(|it| ap::local_theme_name(&it.id, &it.name))
                })
                // Действие без плитки называется своим номером.
                .unwrap_or_else(|| id.clone())
        };
        self.begin_action(&id, &kind, &name);
    }

    fn install_file_begin(self: Pin<&mut Self>, paths_json: &QString) {
        let paths: Vec<String> = serde_json::from_str(&paths_json.to_string()).unwrap_or_default();
        let label = rexon_core::local_install::busy_label(&paths);
        self.begin_action("", ap::ACTION_LOCAL, &label);
    }

    /// Общее начало долгого действия: кнопки заперты, первая строка хода.
    fn begin_action(mut self: Pin<&mut Self>, id: &str, kind: &str, name: &str) {
        let (id, kind) = (id.to_string(), kind.to_string());
        {
            let mut me = self.as_mut().rust_mut();
            me.action_busy = true;
            me.action_kind = kind.clone();
            me.action_item = id.clone();
            me.install_percent = 0;
            // Ход виден в двух местах сразу: в строке состояния сетки и на
            // панели деталей. На деталях строка состояния сетки СКРЫТА, и
            // без своей подписи установка там выглядела как «кнопка не
            // реагирует».
            me.detail_status = ap::install_preparing();
            me.status = ap::action_busy_status(&kind, &name);
        }
        self.as_mut().install_changed();
        self.as_mut().detail_status_changed();
        self.as_mut().status_changed();
    }

    fn set_aur_jobs(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        let jobs: Vec<AurJobView> = v
            .get("jobs")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(AurJobView::from_json).collect())
            .unwrap_or_default();
        // Только что кончившиеся: их итог — в строку состояния, а удавшиеся
        // пакеты (с составом комплекта) плитки отмечают «установлен».
        let finished: Vec<AurJobView> = {
            let me = self.rust();
            jobs.iter()
                .filter(|j| !j.stage.active())
                .filter(|j| {
                    me.aur_jobs.iter().any(|old| old.name == j.name && old.stage.active())
                })
                .cloned()
                .collect()
        };
        let status = v.get("status").and_then(Value::as_str).unwrap_or("").to_string();
        let mut status_changed = false;
        {
            let mut me = self.as_mut().rust_mut();
            me.aur_jobs = jobs;
            for job in finished.iter().filter(|j| j.stage == aur_install::AurStage::Done) {
                for card in me.cards.iter_mut() {
                    if let Card::Aur(pkg) = card {
                        if pkg.name == job.name || job.packages.contains(&pkg.name) {
                            pkg.installed = true;
                        }
                    }
                }
            }
            // Строка сверху: идущая установка с именем пакета; кончилась —
            // её итог. Одна и та же строка второй раз не ставится, чтобы не
            // перебивать сообщения, пришедшие после неё.
            let line = match finished.last() {
                Some(job) if !job.message.is_empty() => {
                    ap::action_result_status(job.stage == aur_install::AurStage::Done, &job.message)
                }
                _ => status,
            };
            if !line.is_empty() && line != me.aur_status {
                me.aur_status = line.clone();
                me.status = line;
                status_changed = true;
            }
        }
        if status_changed {
            self.as_mut().status_changed();
        }
        let rows = self.rust().cards.len();
        if rows > 0 {
            let first = self.as_ref().index(0, 0, &QModelIndex::default());
            let last = self.as_ref().index(rows as i32 - 1, 0, &QModelIndex::default());
            self.as_mut().data_changed(&first, &last, &cxx_qt_lib::QVector::<i32>::default());
        }
    }

    fn set_install_percent(mut self: Pin<&mut Self>, pct: i32) {
        {
            let mut me = self.as_mut().rust_mut();
            me.install_percent = ap::install_percent(pct as i64);
            // Ход пришёл — значит, действие ещё идёт: полосу держим даже
            // если о начале нам сказать забыли.
            me.action_busy = true;
            if me.action_kind.is_empty() {
                me.action_kind = ap::ACTION_INSTALL.to_string();
            }
        }
        self.as_mut().install_changed();
    }

    fn install_log(mut self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        {
            let mut me = self.as_mut().rust_mut();
            me.detail_status = text.clone();
            me.status = text;
        }
        self.as_mut().detail_status_changed();
        self.as_mut().status_changed();
    }

    fn set_detail_status(mut self: Pin<&mut Self>, text: &QString, ok: bool) {
        let text = text.to_string();
        let message = if text.is_empty() {
            String::new()
        } else {
            // Значок ставит ядро и только если его ещё нет: установщик
            // отдаёт готовые «✅ Установлено: …».
            ap::action_result_status(ok, &text)
        };
        // Итог установки из магазина — на плитку и панель деталей этого
        // товара: «установлен» сразу, «применён» — по факту из журнала и
        // карты активных тем (решение ядра). Без этого кнопка оставалась на
        // «Установить» до перезахода в раздел.
        let (kind, item_id) = {
            let me = self.rust();
            (me.action_kind.clone(), me.action_item.clone())
        };
        if ok && kind == ap::ACTION_INSTALL && !item_id.is_empty() {
            self.as_mut().mark_action_item_installed(&item_id);
        }
        {
            let mut me = self.as_mut().rust_mut();
            me.action_item.clear();
            me.detail_status = message.clone();
            // Действие кончилось: полоса хода уходит, кнопки отпираются.
            // Удавшееся применение темы просит пересобрать сетку — но не
            // сейчас, а когда осядет смена стиля Qt (решение ядра).
            if ap::reload_after_action(&me.action_kind, ok) {
                me.reload_pending = true;
            }
            me.action_busy = false;
            me.action_kind.clear();
            me.install_percent = 0;
            if !message.is_empty() {
                me.status = message;
            }
        }
        self.as_mut().install_changed();
        self.as_mut().status_changed();
        self.as_mut().detail_status_changed();
    }

    /// Поставить итог удачной установки плитке и панели деталей товара.
    ///
    /// Состояние считает ядро (`mark_installed_live`): журнал читается один
    /// раз, карта активных тем — только если у вида есть «применена».
    fn mark_action_item_installed(mut self: Pin<&mut Self>, item_id: &str) {
        let home = self.rust().home.to_string_lossy().to_string();
        let (row, mut items) = {
            let me = self.rust();
            let row = me.cards.iter().position(|c| c.id() == item_id);
            let mut items: Vec<ThemeItem> = Vec::new();
            if let Some(Card::Theme(it)) = row.map(|r| &me.cards[r]) {
                if it.local_kind.is_none() {
                    items.push((**it).clone());
                }
            }
            if let Some(it) = me.detail.as_ref() {
                if it.id == item_id && it.local_kind.is_none() {
                    items.push((**it).clone());
                }
            }
            (row, items)
        };
        if items.is_empty() {
            return;
        }
        ap::mark_installed_live(&home, &mut items, true);
        let (installed, applied) = (items[0].installed, items[0].applied);
        let (card_touched, detail_touched) = {
            let mut me = self.as_mut().rust_mut();
            let mut card_touched = false;
            if let Some(r) = row {
                if let Some(Card::Theme(it)) = me.cards.get_mut(r) {
                    if it.local_kind.is_none() {
                        it.installed = installed;
                        it.applied = applied;
                        card_touched = true;
                    }
                }
            }
            let mut detail_touched = false;
            if let Some(it) = me.detail.as_mut() {
                if it.id == item_id && it.local_kind.is_none() {
                    it.installed = installed;
                    it.applied = applied;
                    detail_touched = true;
                }
            }
            (card_touched, detail_touched)
        };
        if card_touched {
            if let Some(r) = row {
                let index = self.as_ref().index(r as i32, 0, &QModelIndex::default());
                let mut roles = cxx_qt_lib::QVector::<i32>::default();
                for role in [ROLE_BUTTON_TEXT, ROLE_BUTTON_STATE, ROLE_BUTTON_ENABLED] {
                    roles.append(ROLE_BASE + role as i32);
                }
                self.as_mut().data_changed(&index, &index, &roles);
            }
        }
        if detail_touched {
            self.as_mut().detail_changed();
        }
    }
}

// ============================================================================
// Загрузчик превью
// ============================================================================
//
// Сеть живёт у хозяина, правила — у ядра (`ap::PreviewQueue`). Мост между
// ними ровно один: очередь говорит, ЧТО запускать, хозяин приносит ответ,
// очередь решает, повторить ли сбой и когда отдать готовое наверх.
//
// Один запрос на адрес, шесть картинок разом, из них не больше двух под
// фоновый прогрев, две добавочные попытки на временный сбой и своё поколение
// у каждого раздела — всё это цифры и правила ядра, здесь их нет.

impl qobject::AppearanceModel {
    /// Картинка уже лежит в кеше? Путь считает ядро — тот же, по которому её
    /// пишет хозяин.
    fn cached_preview(&self, url: &str) -> Option<String> {
        let home = self.rust().home.to_string_lossy().to_string();
        let path = ap::preview_cache_path(&home, url);
        if std::path::Path::new(&path).is_file() {
            Some(path)
        } else {
            None
        }
    }

    /// Заказать картинки всех плиток показанной страницы.
    ///
    /// Оригинал заказывал их так же — всей страницей сразу (`_start_preview_load`),
    /// а не по одной на появившуюся плитку: очередь и так держит канал в
    /// шести запросах, а порядок в ней важнее ленивости.
    fn enqueue_page_previews(mut self: Pin<&mut Self>) {
        let wants: Vec<(String, String)> = self
            .rust()
            .cards
            .iter()
            .map(|c| (c.id().to_string(), c.preview_url()))
            .collect();
        for (cid, url) in wants {
            let cached = self.cached_preview(&url);
            self.as_mut()
                .rust_mut()
                .preview
                .want(&cid, &url, false, cached.as_deref(), true);
        }
        self.as_mut().pump_previews();
    }

    /// Нарисовать плитки местных тем, у которых нет ни своей картинки, ни
    /// картинки из записи магазина.
    ///
    /// Третий и четвёртый шаги единой цепочки превью
    /// (`rexon_core::local_preview`): образец из содержимого темы, а если
    /// взять из неё нечего — заглушка вида. Что рисовать, решает ядро по
    /// таблице видов; здесь нет ни одной развилки по виду темы — только кеш
    /// кадров и исполнение (`crate::local_preview_render`).
    ///
    /// Кадр кладётся файлом и берётся из кеша при следующем заходе: плитке
    /// нужен АДРЕС картинки, отдать ей байты в память не через что. Готовые
    /// кадры ставятся сразу; недостающие рисует фоновый поток по порядку
    /// плиток и отдаёт по одному ([`Self::local_picture_ready`]). В потоке
    /// интерфейса их рисовать нельзя: первый показ «Системных» — около двух
    /// сотен кадров с чтением файлов тем, и окно на это время замирало.
    fn draw_local_placeholders(mut self: Pin<&mut Self>) {
        use rexon_core::local_preview as lp;
        let home = self.rust().home.to_string_lossy().to_string();
        let wants: Vec<(String, i64, String, String, String)> = {
            let me = self.rust();
            me.cards
                .iter()
                .filter_map(|c| match c {
                    Card::Theme(it)
                        if it.local_kind.is_some()
                            && it.preview_local.is_empty()
                            && !me.previews.contains_key(&drawn_key(&it.id, &it.install_path))
                            && !me.drawing.contains(&it.id) =>
                    {
                        Some((
                            it.id.clone(),
                            it.category_id,
                            it.install_path.clone(),
                            it.name.clone(),
                            it.author.clone(),
                        ))
                    }
                    _ => None,
                })
                .collect()
        };
        if wants.is_empty() {
            return;
        }
        let mut ready: Vec<(String, String)> = Vec::new();
        let mut todo: Vec<PictureJob> = Vec::new();
        // Рамки окон KDE снимаются их модулями; что есть на машине — один раз
        // на пачку плиток.
        let shots = wants
            .iter()
            .any(|w| w.0.starts_with("local:aurorae:"))
            .then(|| rexon_core::kdecoration::ShotSetup::live(&home));
        // Стили Qt, темы Kvantum и GTK снимаются настоящими виджетами.
        let widget_shots = wants
            .iter()
            .any(|w| ["local:widgetstyle:", "local:kvantum:", "local:gtk:"].iter().any(|p| w.0.starts_with(p)))
            .then(|| rexon_core::widget_shot::ShotSetup::live(&home));
        for (id, category_id, install_path, name, author) in wants {
            let kind = rexon_core::de_apply::apply_kind_of_local_id(&id).unwrap_or("").to_string();
            let glyph = lp::type_emoji(&author);
            let shot = shots.as_ref().and_then(|s| s.request(&id, &name));
            // Снимок рамки зависит ещё от размера рамки и цветов KDE.
            let shot_tag = match (&shots, kind.as_str()) {
                (Some(s), "aurorae") => s.tag.clone(),
                _ => String::new(),
            };
            // …и от настроек самой рамки: после окна «Настроить…» — новый ключ.
            let shot_tag = match self.rust().deco_config_gen.get(&id) {
                Some(n) if !shot_tag.is_empty() => format!("{shot_tag}\u{0}cfg{n}"),
                _ => shot_tag,
            };
            let widget_shot = widget_shots.as_ref().and_then(|s| s.request(&id, &install_path));
            let shot_tag = match widget_shots.as_ref().map(|s| s.cache_tag(&id, &install_path)) {
                Some(tag) if !tag.is_empty() => format!("{shot_tag}\u{0}{tag}"),
                _ => shot_tag,
            };
            // У цветовой схемы скан отдаёт путь без окончания; отметка
            // времени нужна её файла — кадр рисуется по его содержимому.
            let colors_file = format!("{install_path}.colors");
            let stamped = if !std::path::Path::new(&install_path).exists()
                && std::path::Path::new(&colors_file).is_file()
            {
                colors_file
            } else {
                install_path.clone()
            };
            // В ключе — всё, от чего зависит кадр: вид, путь, знак вида,
            // отметка времени и отметка правил отрисовки.
            let dest = ap::placeholder_cache_path(
                &home,
                category_id,
                &install_path,
                &name,
                &format!("{}\u{0}{kind}\u{0}{glyph}\u{0}{shot_tag}", lp::PICTURE_STYLE),
                file_stamp(&stamped),
            );
            if std::path::Path::new(&dest).is_file() {
                ready.push((drawn_key(&id, &install_path), dest));
            } else {
                todo.push(PictureJob { id, kind, install_path, glyph, dest, shot, widget_shot });
            }
        }
        if !ready.is_empty() {
            {
                let mut me = self.as_mut().rust_mut();
                for (key, path) in &ready {
                    me.previews.insert(key.clone(), path.clone());
                }
            }
            // Кадры появились после сброса модели — перерисовываем одну
            // роль, а не всю витрину.
            let rows = self.rust().cards.len();
            if rows > 0 {
                let first = self.as_ref().index(0, 0, &QModelIndex::default());
                let last = self
                    .as_ref()
                    .index(rows as i32 - 1, 0, &QModelIndex::default());
                let mut roles = cxx_qt_lib::QVector::<i32>::default();
                roles.append(ROLE_BASE + ROLE_PREVIEW as i32);
                self.as_mut().data_changed(&first, &last, &roles);
            }
        }
        if todo.is_empty() {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            for job in &todo {
                me.drawing.insert(job.id.clone());
            }
        }
        let handle = self.as_ref().get_ref().qt_thread();
        let shots_runner = shots.and_then(|s| s.runner);
        let widget_helper = widget_shots.and_then(|s| s.helper);
        let spawned = std::thread::Builder::new().name("rexon-tile-pictures".into()).spawn(move || {
            let icon_themes = lp::icon_theme_dirs(&home, "/usr");
            for job in todo {
                // Рамку KDE рисует её модуль в отдельном процессе; не вышло —
                // прежняя картинка (образец из SVG либо заглушка).
                let shot_png = format!("{}.shot.png", job.dest);
                let shot_dir = std::path::Path::new(&job.dest)
                    .parent()
                    .map(|d| d.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let shot_runner = shots_runner.as_deref();
                let shot_taken = match (&job.shot, shot_runner) {
                    (Some(req), Some(runner)) => rexon_core::kdecoration::take_shot(runner, &shot_dir, req, &shot_png),
                    _ => false,
                };
                // Стиль Qt и тему GTK снимает помощник настоящими виджетами;
                // не вышло — нарисованный образец по цветам темы.
                let widget_taken = !shot_taken
                    && match (&job.widget_shot, widget_helper.as_deref()) {
                        (Some(req), Some(helper)) => rexon_core::widget_shot::take_shot(helper, &shot_dir, req, &shot_png),
                        _ => false,
                    };
                let picture = if shot_taken {
                    lp::shot_picture(&shot_png, "рамка окна, нарисованная её модулем")
                } else if widget_taken {
                    lp::shot_picture(&shot_png, "форма из настоящих виджетов в стиле темы")
                } else {
                    lp::card_picture(
                        &lp::SampleInput {
                            kind: &job.kind,
                            install_path: &job.install_path,
                            icon_themes: &icon_themes,
                        },
                        &job.glyph,
                    )
                };
                let drawn = crate::local_preview_render::render_picture_to_png(&picture, &job.dest);
                if shot_taken || widget_taken {
                    let _ = std::fs::remove_file(&shot_png);
                }
                let key = drawn_key(&job.id, &job.install_path);
                let (id, dest) = (job.id, job.dest);
                // Окно закрыли — очередь не принимает: дорисовывать некому.
                if handle
                    .queue(move |object| object.local_picture_ready(&id, &key, drawn.then_some(dest)))
                    .is_err()
                {
                    return;
                }
            }
        });
        if spawned.is_err() {
            // Поток не завёлся — плитки остаются без картинки до следующего
            // захода в раздел, но заказ не считается висящим.
            self.as_mut().rust_mut().drawing.clear();
        }
    }

    /// Фоновый поток нарисовал картинку плитки `id` (`None` — не вышло).
    fn local_picture_ready(mut self: Pin<&mut Self>, id: &str, key: &str, path: Option<String>) {
        let row = {
            let mut me = self.as_mut().rust_mut();
            me.drawing.remove(id);
            let Some(path) = path else { return };
            me.previews.insert(key.to_string(), path.clone());
            // Открытая панель деталей ждёт этот кадр (снимок рамки после
            // окна «Настроить…»).
            let for_detail = me.detail.as_ref().is_some_and(|it| it.id == id) && me.detail_frame.is_empty();
            if for_detail {
                me.detail_frame = path;
                me.detail_frame_text = String::new();
            }
            let row = me.cards.iter().position(|c| c.id() == id);
            (row, for_detail)
        };
        let (row, for_detail) = row;
        if for_detail {
            self.as_mut().detail_frame_changed();
        }
        // Раздел с тех пор сменили — картинка осталась в кеше и встанет при
        // следующем заходе; перерисовывать нечего.
        let Some(row) = row else { return };
        let index = self.as_ref().index(row as i32, 0, &QModelIndex::default());
        let mut roles = cxx_qt_lib::QVector::<i32>::default();
        roles.append(ROLE_BASE + ROLE_PREVIEW as i32);
        self.as_mut().data_changed(&index, &index, &roles);
    }

    /// Фоновый прогрев первого кадра карусели показанных плиток.
    ///
    /// Плитка и карусель — РАЗНЫЕ картинки: в плитке миниатюра, в карусели
    /// полный размер со второго скриншота. Годами грелась только плитка,
    /// поэтому щелчок по уже показанной карточке всё равно упирался в сеть.
    fn warm_gallery(mut self: Pin<&mut Self>, generation: &QString) {
        let asked: u64 = generation.to_string().parse().unwrap_or(u64::MAX);
        if asked != self.rust().requests.generation() {
            return; // раздел с тех пор сменили — греть уже нечего
        }
        let wants: Vec<(String, String)> = self
            .rust()
            .cards
            .iter()
            .filter_map(|c| {
                c.gallery_urls()
                    .into_iter()
                    .next()
                    .map(|u| (c.id().to_string(), u))
            })
            .collect();
        for (cid, url) in wants {
            let cached = self.cached_preview(&url);
            self.as_mut()
                .rust_mut()
                .preview
                .want(&cid, &url, true, cached.as_deref(), false);
        }
        self.as_mut().pump_previews();
    }

    /// Фоновый прогрев картинок ЧУЖОЙ страницы (её плиток на экране нет —
    /// наверх ничего не отдаётся, кеш просто наполняется).
    fn enqueue_prefetched_previews(mut self: Pin<&mut Self>, urls: Vec<(String, String)>) {
        for (cid, url) in urls {
            let cached = self.cached_preview(&url);
            self.as_mut()
                .rust_mut()
                .preview
                .want(&cid, &url, true, cached.as_deref(), false);
        }
        self.as_mut().pump_previews();
    }

    /// Раздать хозяину то, что очередь разрешила запустить, и разбудить
    /// выдачу готовых.
    fn pump_previews(mut self: Pin<&mut Self>) {
        let jobs = self.as_mut().rust_mut().preview.pump();
        for job in jobs {
            let id = {
                let mut me = self.as_mut().rust_mut();
                me.preview_seq += 1;
                let id = me.preview_seq;
                me.preview_jobs.insert(
                    id,
                    PreviewReq {
                        cid: job.content_id.clone(),
                        url: job.url.clone(),
                        low: job.low,
                    },
                );
                id
            };
            self.as_mut().preview_wanted(
                QString::from(&id.to_string()),
                QString::from(&job.content_id),
                QString::from(&job.url),
                job.low,
            );
        }
        self.as_mut().preview_changed();
    }

    /// Ответ по заявке пришёл — снять её с учёта и вернуть, о чём просили.
    fn take_preview_job(mut self: Pin<&mut Self>, job: &QString) -> Option<PreviewReq> {
        let id: u64 = job.to_string().parse().ok()?;
        self.as_mut().rust_mut().preview_jobs.remove(&id)
    }

    fn preview_done(mut self: Pin<&mut Self>, job: &QString, path: &QString) {
        let Some(req) = self.as_mut().take_preview_job(job) else {
            return;
        };
        let path = path.to_string();
        {
            let mut me = self.as_mut().rust_mut();
            me.preview.finished(&req.url, req.low);
            me.preview.succeeded(&req.cid, &req.url, req.low, &path);
        }
        self.as_mut().pump_previews();
    }

    fn preview_failed(mut self: Pin<&mut Self>, job: &QString, status: &QString) {
        let Some(req) = self.as_mut().take_preview_job(job) else {
            return;
        };
        self.as_mut().rust_mut().preview.finished(&req.url, req.low);
        let status = status.to_string();
        // Заявку оборвали мы сами (ушли из раздела) — это не сбой сети, и
        // повторять её незачем.
        if status == PREVIEW_CANCELED {
            self.as_mut().pump_previews();
            return;
        }
        // У живой плитки есть запасные картинки товара: первая не
        // загрузилась — сразу следующая. Повтор той же ссылки здесь лишний:
        // временные сбои сети скачивание уже повторило само.
        if !req.low {
            let next = {
                let me = self.rust();
                me.cards
                    .iter()
                    .find(|c| c.id() == req.cid)
                    .and_then(|c| ap::next_preview_candidate(&c.preview_candidates(), &req.url))
            };
            if let Some(url) = next {
                let cached = self.cached_preview(&url);
                self.as_mut()
                    .rust_mut()
                    .preview
                    .want(&req.cid, &url, false, cached.as_deref(), true);
                self.as_mut().pump_previews();
                return;
            }
        }
        let code: Option<u16> = status.parse().ok();
        let verdict = self.as_mut().rust_mut().preview.failed(&req.url, code);
        if let ap::PreviewRetry::Again { delay_ms, .. } = verdict {
            let (generation, id) = {
                let mut me = self.as_mut().rust_mut();
                me.preview_seq += 1;
                let id = me.preview_seq;
                me.preview_jobs.insert(id, req);
                (me.preview.generation(), id)
            };
            self.as_mut().preview_retry_wanted(
                QString::from(&generation.to_string()),
                QString::from(&id.to_string()),
                QString::from(&delay_ms.to_string()),
            );
        }
        self.as_mut().pump_previews();
    }

    fn preview_retry(mut self: Pin<&mut Self>, generation: &QString, job: &QString) {
        let Some(req) = self.as_mut().take_preview_job(job) else {
            return;
        };
        let asked: u64 = generation.to_string().parse().unwrap_or(u64::MAX);
        let back = self
            .as_mut()
            .rust_mut()
            .preview
            .requeue(asked, &req.cid, &req.url, req.low);
        if back {
            self.as_mut().pump_previews();
        }
    }

    fn drain_preview(mut self: Pin<&mut Self>) {
        let Some((cid, path)) = self.as_mut().rust_mut().preview.take_ready() else {
            self.as_mut().preview_changed();
            return;
        };
        self.as_mut()
            .set_preview(&QString::from(&cid), &QString::from(&path));
        self.as_mut().preview_changed();
    }

    /// Бросить всё, что летит и стоит за превью: раздел сменили.
    fn cancel_previews(mut self: Pin<&mut Self>) {
        {
            let mut me = self.as_mut().rust_mut();
            me.preview.cancel_all();
            me.preview_jobs.clear();
        }
        self.as_mut().previews_canceled();
        self.as_mut().preview_changed();
    }

    fn catalog_generation(&self) -> QString {
        QString::from(&self.rust().requests.generation().to_string())
    }
}

// ============================================================================
// Фоновый прогрев
// ============================================================================
//
// Две разные бесшовности одним механизмом (правило #12):
//   * СОСЕДНИЙ РАЗДЕЛ — открыл N, фоном греется N+1, N+2…: переключение
//     открывается из кеша мгновенно;
//   * СЛЕДУЮЩАЯ СТРАНИЦА открытого раздела — «Вперёд» тоже без сети.
//
// Очередь, порядок, дедуп и «по одной за раз» держит ядро
// (`ap::CatalogRequests`), сеть — хозяин. Живой запрос человека всегда
// важнее: пока раздел, на который он смотрит, грузится, канал ему не
// занимаем, а очередь догонит после ответа.

impl qobject::AppearanceModel {
    fn start_category_prefetch(mut self: Pin<&mut Self>) {
        let targets = {
            let me = self.rust();
            ap::prefetch_targets(&me.view, &me.sidebar)
        };
        self.as_mut().rust_mut().requests.enqueue_prefetch(&targets);
        self.as_mut().pump_prefetch();
    }

    /// Поставить соседей открытого раздела в начало очереди.
    fn prioritize_prefetch(mut self: Pin<&mut Self>) {
        let front = {
            let me = self.rust();
            ap::prefetch_neighbours(&me.view, &me.sidebar, me.requests.prefetch_done())
        };
        self.as_mut().rust_mut().requests.prioritize_prefetch(&front);
        self.as_mut().pump_prefetch();
    }

    fn pump_prefetch(mut self: Pin<&mut Self>) {
        let Some((cat_id, sort)) = self.as_mut().rust_mut().requests.next_prefetch() else {
            return;
        };
        let body = json!({
            "kind": PREFETCH_CATEGORY,
            "categoryId": cat_id,
            "page": 0,
            "pageSize": ap::PAGE_SIZE,
            "sort": sort,
            "query": "",
        });
        self.as_mut()
            .prefetch_wanted(QString::from(&body.to_string()));
    }

    /// Прогреть кеш СЛЕДУЮЩЕЙ страницы открытого раздела.
    ///
    /// Один прогрев на страницу за сессию: без этого каждый повторный заход в
    /// раздел слал тот же запрос заново и топил канал. Живой запрос человека
    /// за ту же страницу тоже считается прогревом.
    fn prefetch_next_page(mut self: Pin<&mut Self>) {
        let request = {
            let me = self.rust();
            // Местные разделы и AUR лежат на диске, Cinnamon Spices
            // листается по своему индексу — греть у них нечего.
            if me.view.is_local()
                || me.view.aur_mode()
                || me.view.spices_mode()
                || me.total <= 0
                || self.installed_only()
            {
                None
            } else {
                // Есть ли следующая — по факту потока, если ответ его знает.
                let more = me.has_more.unwrap_or_else(|| {
                    ap::has_page_to_prefetch(me.view.page, me.total, me.cards.len())
                });
                if !more {
                    None
                } else {
                    // Сортировка обязана совпасть с той, по которой пойдёт
                    // живой запрос, иначе ключ кеша страницы N+1 не сойдётся
                    // при нажатии «Вперёд».
                    Some(json!({
                        "kind": PREFETCH_PAGE,
                        "categoryId": me.view.query_category_id(),
                        "page": me.view.page + 1,
                        "pageSize": ap::PAGE_SIZE,
                        "sort": me.view.active_sort(),
                        "query": me.view.query,
                    }))
                }
            }
        };
        let Some(body) = request else { return };
        let key = ocs::list_key(
            body["categoryId"].as_str().unwrap_or_default(),
            body["page"].as_i64().unwrap_or_default(),
            body["sort"].as_str().unwrap_or_default(),
            body["query"].as_str().unwrap_or_default(),
        );
        if !self.as_mut().rust_mut().requests.claim_page_prefetch(&key) {
            return;
        }
        self.as_mut()
            .prefetch_wanted(QString::from(&body.to_string()));
    }

    fn prefetch_answer(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        // Цель раздела отмечается сделанной независимо от улова: кеш просто
        // не наполнится, а второй раз ту же цель тянуть незачем.
        if s(&v, "kind") == PREFETCH_CATEGORY {
            let cat_id = s(&v, "categoryId").to_string();
            let sort = s(&v, "sort").to_string();
            self.as_mut()
                .rust_mut()
                .requests
                .prefetch_finished((cat_id, sort));
        }
        // Картинки чужой страницы — низким приоритетом: показывать их
        // некуда, они лишь наполняют кеш к моменту, когда её откроют.
        let mut urls: Vec<(String, String)> = Vec::new();
        if let Some(list) = v.get("items").and_then(Value::as_array) {
            for raw in list {
                if let Ok(item) = serde_json::from_value::<ThemeItem>(raw.clone()) {
                    if !item.preview_url.is_empty() {
                        urls.push((item.id, item.preview_url));
                    }
                }
            }
        }
        if !urls.is_empty() {
            self.as_mut().enqueue_prefetched_previews(urls);
        }
    }

    fn reload_after_style_storm(mut self: Pin<&mut Self>) {
        {
            let mut me = self.as_mut().rust_mut();
            if !me.reload_pending {
                return;
            }
            me.reload_pending = false;
            // Тема применена только что: кешированный скан диска показал бы
            // «✅ Активна» на прежней.
            me.catalog_fresh = true;
        }
        self.as_mut().install_changed();
        self.resync();
    }

    fn reload_local_fresh(mut self: Pin<&mut Self>) {
        {
            let mut me = self.as_mut().rust_mut();
            if !(me.view.is_local() || me.view.installed_view()) || me.screen != ap::SCREEN_GRID {
                return;
            }
            // Кеш сканов хозяин уже сбросил, но запрос без обхода мог бы
            // встретить заново заполненный соседним прогревом.
            me.catalog_fresh = true;
        }
        self.resync();
    }

    fn set_gtk4_apply(mut self: Pin<&mut Self>, on: bool) -> QString {
        let err = match rexon_core::gtk4_theme::set_enabled_live(on) {
            Ok(()) => String::new(),
            Err(e) => {
                rexon_core::log_warn!("theme", "переключатель GTK 4: {e}");
                e.to_string()
            }
        };
        self.as_mut().detail_changed();
        QString::from(&err)
    }

    fn configure_detail_style(self: Pin<&mut Self>) -> QString {
        let Some((kind, name)) = self.detail_local_parts() else {
            return QString::default();
        };
        let b = rexon_core::themes::style_config_button_live(&kind, &name);
        if !b.enabled || b.argv.is_empty() {
            return QString::from(&b.tooltip);
        }
        match spawn_detached(&b.argv) {
            Ok(mut child) => {
                // Дождаться в стороне — иначе закрытое окно висело бы зомби.
                std::thread::spawn(move || child.wait());
                QString::default()
            }
            Err(e) => {
                rexon_core::log_warn!("theme", "настройки стиля {name}: {e}");
                QString::from(&e)
            }
        }
    }

    fn configure_detail_decoration(self: Pin<&mut Self>) -> QString {
        use rexon_core::kdecoration as kd;
        let Some(it) = self.rust().detail.clone().filter(|it| it.id.starts_with("local:aurorae:")) else {
            return QString::default();
        };
        let b = kd::config_button_live(&it.id, &it.install_path, &it.name);
        if !b.enabled || b.argv.is_empty() {
            return QString::from(&b.tooltip);
        }
        if !kd::ensure_config_script(&b.argv) {
            rexon_core::log_warn!("theme", "сценарий окна настроек рамки не записан");
        }
        match spawn_detached(&b.argv) {
            Ok(mut child) => {
                // Как у KDE: окно настроек закрыли — образец рамки рисуется
                // заново (настройки могли поменять её вид).
                let handle = self.as_ref().get_ref().qt_thread();
                let id = it.id.clone();
                std::thread::spawn(move || {
                    let _ = child.wait();
                    let _ = handle.queue(move |mut o| o.as_mut().decoration_settings_closed(&id));
                });
                QString::default()
            }
            Err(e) => {
                rexon_core::log_warn!("theme", "настройки рамки {}: {e}", it.id);
                QString::from(&e)
            }
        }
    }

    /// Окно настроек рамки `id` закрыли: прежний снимок — вон из кеша, новый
    /// кладётся под другим ключом (`deco_config_gen`) и встаёт в плитку и в
    /// открытую панель деталей.
    fn decoration_settings_closed(mut self: Pin<&mut Self>, id: &str) {
        {
            let mut me = self.as_mut().rust_mut();
            *me.deco_config_gen.entry(id.to_string()).or_insert(0) += 1;
            let keys: Vec<String> = me
                .previews
                .keys()
                .filter(|k| k.split_once('\u{0}').is_some_and(|(kid, _)| kid == id))
                .cloned()
                .collect();
            for key in keys {
                if let Some(old) = me.previews.remove(&key) {
                    let _ = std::fs::remove_file(old);
                }
            }
            if me.detail.as_ref().is_some_and(|it| it.id == id) {
                me.detail_frame.clear();
            }
        }
        self.as_mut().draw_local_placeholders();
    }

    fn edit_detail_scheme(self: Pin<&mut Self>, mode: &QString) -> QString {
        use rexon_core::color_scheme_edit as cse;
        let Some(it) = self.rust().detail.clone() else {
            return QString::from("{}");
        };
        let home = std::env::var("HOME").unwrap_or_default();
        let file = cse::scheme_of_item(&it.id, &it.install_path).map(|(_, f)| f).unwrap_or_default();
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let Some(open) = cse::edit_open(
            &it.id,
            &it.install_path,
            &it.name,
            &home,
            cse::EditMode::from_key(&mode.to_string()),
            kde_scheme_editor_present(),
            &text,
        ) else {
            return QString::from("{}");
        };
        if open.mode == "kde" {
            match spawn_detached(&open.argv) {
                Ok(child) => {
                    // Как у модуля KDE: редактор закрыли — список схем
                    // перечитывается (новая схема, правка своей).
                    let handle = self.as_ref().get_ref().qt_thread();
                    std::thread::spawn(move || {
                        let mut child = child;
                        let _ = child.wait();
                        let _ = handle.queue(|mut o| o.as_mut().schemes_changed_on_disk());
                    });
                }
                Err(e) => {
                    rexon_core::log_warn!("theme", "редактор цветовых схем: {e}");
                    return QString::from(&json!({"error": e}).to_string());
                }
            }
        }
        QString::from(&serde_json::to_string(&open).unwrap_or_else(|_| "{}".into()))
    }

    fn save_scheme_copy(
        mut self: Pin<&mut Self>,
        name: &QString,
        roles: &QString,
        base: &QString,
        overwrite: &QString,
    ) -> QString {
        use rexon_core::color_scheme_edit as cse;
        let roles: Vec<cse::SchemeRole> = serde_json::from_str(&roles.to_string()).unwrap_or_default();
        let home = std::env::var("HOME").unwrap_or_default();
        let overwrite = overwrite.to_string();
        let result = cse::save_scheme(
            &home,
            &base.to_string(),
            &name.to_string(),
            &roles,
            (!overwrite.is_empty()).then_some(overwrite.as_str()),
            &cse::system_scheme_dirs(std::env::var("XDG_DATA_DIRS").ok().as_deref()),
        );
        if result.ok {
            rexon_core::log_info!("theme", "цветовая схема сохранена: {}", result.path);
            self.as_mut().schemes_changed_on_disk();
        }
        QString::from(&serde_json::to_string(&result).unwrap_or_else(|_| "{}".into()))
    }

    /// Схемы на диске поменялись (свой редактор или редактор KDE): сетка
    /// перечитывает диск мимо кеша — сразу, если открыта, иначе при
    /// следующем запросе.
    fn schemes_changed_on_disk(mut self: Pin<&mut Self>) {
        let on_grid = {
            let mut me = self.as_mut().rust_mut();
            me.catalog_fresh = true;
            me.screen == ap::SCREEN_GRID
        };
        if on_grid {
            self.resync();
        }
    }

    fn close_page(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().requests.close();
        self.as_mut().cancel_previews();
    }
}

// ============================================================================
// Галерея скриншотов и комментарии
// ============================================================================

/// Один комментарий в том виде, в каком его показывает панель деталей.
#[derive(Debug, Clone, Default)]
pub struct CommentRow {
    /// Закреплён сопровождающим пакета AUR (значок перед ником).
    pinned: bool,
    author: String,
    date: String,
    original: String,
    translated: String,
    was_translated: bool,
    show_original: bool,
    depth: i64,
}

impl qobject::AppearanceModel {
    // ── Неизменные подписи накладных контролов ──────────────────────────
    fn overlay_zoom_text(&self) -> QString {
        QString::from(ap::OVERLAY_ZOOM_BTN)
    }
    fn overlay_zoom_tooltip(&self) -> QString {
        QString::from(&ap::overlay_zoom_tooltip())
    }
    fn overlay_prev_text(&self) -> QString {
        QString::from(ap::OVERLAY_PREV_BTN)
    }
    fn overlay_prev_tooltip(&self) -> QString {
        QString::from(&ap::overlay_prev_tooltip())
    }
    fn overlay_next_text(&self) -> QString {
        QString::from(ap::OVERLAY_NEXT_BTN)
    }
    fn overlay_next_tooltip(&self) -> QString {
        QString::from(&ap::overlay_next_tooltip())
    }
    fn overlay_button_size(&self) -> i32 {
        ap::OVERLAY_BTN
    }
    fn overlay_margin(&self) -> i32 {
        ap::OVERLAY_MARGIN
    }
    fn comments_empty_text(&self) -> QString {
        QString::from(&ap::comments_empty_text())
    }
    fn comments_more_text(&self) -> QString {
        QString::from(&ap::comments_more_btn())
    }

    // ── Галерея ─────────────────────────────────────────────────────────
    fn gallery_total(&self) -> i64 {
        let me = self.rust();
        ap::gallery_total(
            me.gallery_known,
            me.gallery_src.len(),
            me.gallery_loaded.len(),
        )
    }

    /// Сколько кадров галереи ещё живы (битые при листании пропускаются).
    fn gallery_alive(&self) -> i64 {
        let total = self.gallery_total();
        let dead = self
            .rust()
            .gallery_failed
            .iter()
            .filter(|&&i| i >= 0 && i < total)
            .count() as i64;
        total - dead
    }

    fn gallery_counter(&self) -> QString {
        let me = self.rust();
        match ap::gallery_counter_skipping(me.gallery_index, self.gallery_total(), &me.gallery_failed) {
            Some(text) => QString::from(&text),
            None => QString::default(),
        }
    }

    fn gallery_nav_visible(&self) -> bool {
        self.gallery_alive() > 1
    }

    /// Пересчитать кадр на месте картинки и попросить у хозяина недостающее.
    fn sync_frame(mut self: Pin<&mut Self>) {
        let total = self.gallery_total();
        let alive = self.gallery_alive();
        let frame = {
            let me = self.rust();
            if total > 0 && alive == 0 && !me.gallery_loaded.contains_key(&me.gallery_index) {
                // Битые все кадры — картинки не будет, а не «листайте стрелками».
                ap::GalleryFrame::Unavailable
            } else {
                ap::gallery_frame(
                    me.gallery_index,
                    &me.gallery_loaded,
                    &me.gallery_failed,
                    me.gallery_finished,
                    total,
                )
            }
        };
        {
            let mut me = self.as_mut().rust_mut();
            match frame {
                ap::GalleryFrame::Image(path) => {
                    me.detail_frame = path;
                    me.detail_frame_text = String::new();
                }
                ap::GalleryFrame::Failed => {
                    me.detail_frame = String::new();
                    me.detail_frame_text = ap::gallery_failed_msg();
                }
                ap::GalleryFrame::Loading => {
                    me.detail_frame = String::new();
                    me.detail_frame_text = ap::gallery_loading_msg();
                }
                // Одиночная картинка, которой пока нет: панель остаётся
                // как была — мигать пустотой на ровном месте незачем.
                ap::GalleryFrame::Unchanged => {}
                // Картинки не будет. Превью плитки, если оно уже стоит на
                // месте кадра, остаётся — заглушка только вместо пустоты.
                ap::GalleryFrame::Unavailable => {
                    if me.detail_frame.is_empty() {
                        me.detail_frame_text = ap::detail_preview_unavailable();
                    }
                }
            }
        }
        self.as_mut().detail_frame_changed();
        self.request_frame();
    }

    /// Попросить кадр, который показан сейчас, если его ещё нет.
    fn request_frame(mut self: Pin<&mut Self>) {
        let want = {
            let me = self.rust();
            let index = me.gallery_index;
            if me.gallery_loaded.contains_key(&index) || me.gallery_failed.contains(&index) {
                None
            } else {
                me.gallery_src
                    .get(index as usize)
                    .cloned()
                    .map(|url| (index, url))
            }
        };
        let Some((index, url)) = want else { return };
        self.as_mut()
            .gallery_wanted(QString::from(&index.to_string()), QString::from(&url));
    }

    fn gallery_step(mut self: Pin<&mut Self>, delta: i64) {
        let total = self.gallery_total();
        let next = {
            let me = self.rust();
            ap::gallery_step_skipping(me.gallery_index, total, delta, &me.gallery_failed)
        };
        {
            let mut me = self.as_mut().rust_mut();
            if me.gallery_index == next {
                return;
            }
            me.gallery_index = next;
            // Дальше кадр из кеша человека уже не сдвинет: он выбрал сам.
            me.gallery_user_nav = true;
        }
        self.sync_frame();
    }

    fn gallery_prev(self: Pin<&mut Self>) {
        self.gallery_step(-1);
    }

    fn gallery_next(self: Pin<&mut Self>) {
        self.gallery_step(1);
    }

    fn set_gallery_image(mut self: Pin<&mut Self>, index: &QString, path: &QString) {
        let Ok(index) = index.to_string().parse::<i64>() else {
            return;
        };
        {
            let mut me = self.as_mut().rust_mut();
            me.gallery_loaded.insert(index, path.to_string());
            me.gallery_failed.remove(&index);
        }
        // Кадр подхватился из кеша, а человек ещё не листал — встаём на
        // самый ранний готовый (решение ядра).
        let adopt = {
            let me = self.rust();
            ap::adopt_gallery_index(&me.gallery_loaded, me.gallery_index, me.gallery_user_nav)
        };
        if let Some(index) = adopt {
            self.as_mut().rust_mut().gallery_index = index;
        }
        self.sync_frame();
    }

    fn set_gallery_failed(mut self: Pin<&mut Self>, index: &QString) {
        let Ok(index) = index.to_string().parse::<i64>() else {
            return;
        };
        let total = self.gallery_total();
        {
            let mut me = self.as_mut().rust_mut();
            me.gallery_failed.insert(index);
            // Не загрузился кадр, на котором стоим, — уходим на следующий
            // живой: битый кадр в галерее не показываем.
            if me.gallery_index == index {
                me.gallery_index =
                    ap::gallery_step_skipping(index, total, 1, &me.gallery_failed);
            }
        }
        self.sync_frame();
    }

    fn set_gallery_finished(mut self: Pin<&mut Self>, count: &QString) {
        let count = count.to_string().parse::<i64>().unwrap_or(0);
        {
            let mut me = self.as_mut().rust_mut();
            me.gallery_finished = true;
            if count > 0 {
                me.gallery_known = count;
            }
        }
        self.sync_frame();
    }

    fn zoom_frames(&self) -> QList<QVariant> {
        let me = self.rust();
        let mut out = QList::<QVariant>::default();
        // Показываем ТОЛЬКО то, что действительно лежит на диске: решение
        // и проверку файла делает ядро, здесь лишь доступ к файловой
        // системе, которого у ядра нет.
        let exists = |p: &str| std::path::Path::new(p).is_file();
        let current = me.detail_frame.clone();
        let Some((paths, index)) = ap::zoom_frames(&current, &me.gallery_loaded, exists) else {
            return out;
        };
        for path in paths {
            out.append(qstr(&file_url(&path)));
        }
        // Последним слотом — номер кадра, с которого открывать просмотр.
        out.append(QVariant::from(&QString::from(&index.to_string())));
        out
    }

    // ── Комментарии ─────────────────────────────────────────────────────
    fn comments_header(&self) -> QString {
        let me = self.rust();
        if me.comments.is_empty() && me.comments_total.unwrap_or(0) == 0 {
            return QString::from(&ap::comments_empty_header());
        }
        let depths: Vec<i64> = me.comments.iter().map(|c| c.depth).collect();
        let has_more = match me.aur_detail.as_ref() {
            Some(d) => d.comments_more,
            None => ap::comments_has_more(&depths),
        };
        QString::from(&ap::comments_header_text(
            me.comments_total,
            me.comments.len(),
            has_more,
        ))
    }

    fn comment_rows(&self) -> QList<QVariant> {
        let me = self.rust();
        let accent = ui_base::active_skin_colors()
            .get("accent")
            .cloned()
            .unwrap_or_default();
        let product_author = me
            .detail
            .as_ref()
            .map(|it| it.author.clone())
            .unwrap_or_default();
        let mut out = QList::<QVariant>::default();
        for (i, row) in me.comments.iter().enumerate() {
            let body = ap::comment_body_html(
                &row.original,
                &row.translated,
                row.was_translated,
                row.show_original,
                &accent,
            );
            out.append(QVariant::from(&vmap(vec![
                ("index", qstr(&i.to_string())),
                ("author", qstr(&ap::comment_author_display(&row.author))),
                ("date", qstr(&ap::comment_date(&row.date))),
                ("body", qstr(&body)),
                (
                    "indent",
                    QVariant::from(&(ap::comment_indent(row.depth) as i32)),
                ),
                (
                    "replyPrefix",
                    qstr(&format!(
                        "{}{}",
                        if row.pinned { rexon_core::aur::PINNED_MARK } else { "" },
                        ap::comment_reply_prefix(row.depth)
                    )),
                ),
                (
                    "isProductAuthor",
                    // У пакета AUR «свои» — и соисполнители тоже.
                    QVariant::from(
                        &(ap::comment_is_product_author(&row.author, &product_author)
                            || me.aur_detail.as_ref().is_some_and(|d| {
                                d.comaintainers
                                    .iter()
                                    .any(|c| ap::comment_is_product_author(&row.author, c))
                            })),
                    ),
                ),
            ])));
        }
        out
    }

    fn comments_more_visible(&self) -> bool {
        let me = self.rust();
        // У пакета AUR «есть ли ещё» знает сам сайт (ссылка «Next»): страница
        // бывает неполной из-за удалённых комментариев.
        if let Some(d) = me.aur_detail.as_ref() {
            return d.comments_more && !me.comments.is_empty();
        }
        // Кнопка «Показать ещё» имеет смысл, пока страница пришла полной.
        me.detail.as_ref().is_some_and(|it| it.local_kind.is_none())
            && !me.comments.is_empty()
            && me.comments.len() % ap::COMMENTS_PAGESIZE == 0
    }

    fn comments_empty(&self) -> bool {
        self.rust().comments.is_empty()
    }

    fn load_more_comments(mut self: Pin<&mut Self>) {
        let (id, page) = {
            let me = self.rust();
            let Some(item) = me.detail.as_ref() else {
                return;
            };
            (item.id.clone(), me.comments_page)
        };
        self.as_mut()
            .comments_wanted(QString::from(&id), QString::from(&page.to_string()));
    }

    fn comments_answer(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        // Ответ про закрытый товар молча отбрасываем.
        {
            let me = self.rust();
            let open = me.detail.as_ref().map(|it| it.id.clone()).unwrap_or_default();
            if open.is_empty() || open != s(&v, "itemId") {
                return;
            }
        }
        let page = v.get("page").and_then(Value::as_i64).unwrap_or(0);
        let total = v.get("total").and_then(Value::as_i64);
        let mut rows: Vec<CommentRow> = Vec::new();
        if let Some(list) = v.get("comments").and_then(Value::as_array) {
            for raw in list {
                let original = ap::clean_comment_text(s(raw, "text"));
                let translated = raw
                    .get("translated")
                    .and_then(Value::as_str)
                    .map(ap::clean_comment_text)
                    .unwrap_or_default();
                rows.push(CommentRow {
                    pinned: raw.get("pinned").and_then(Value::as_bool).unwrap_or(false),
                    author: s(raw, "author").to_string(),
                    date: s(raw, "date").to_string(),
                    was_translated: !translated.is_empty() && translated != original,
                    translated: if translated.is_empty() {
                        original.clone()
                    } else {
                        translated
                    },
                    original,
                    show_original: false,
                    depth: ap::comment_depth(
                        raw.get("depth").and_then(Value::as_i64).unwrap_or(0),
                    ),
                });
            }
        }
        {
            let mut me = self.as_mut().rust_mut();
            // Первая страница заменяет список, следующие дописываются.
            if page == 0 {
                me.comments.clear();
            }
            me.comments.extend(rows);
            me.comments_page = page + 1;
            if total.is_some() {
                me.comments_total = total;
            }
        }
        // Страница пакета AUR несёт и то, чего нет у магазина: есть ли ещё
        // страница, не закрыли ли нам сайт, соисполнители.
        let comaintainers: Option<Vec<String>> = v.get("comaintainers").and_then(Value::as_array).map(|a| {
            a.iter().filter_map(Value::as_str).map(str::to_string).collect()
        });
        let had_names = {
            let mut me = self.as_mut().rust_mut();
            match me.aur_detail.as_mut() {
                Some(d) => {
                    d.comments_blocked = v.get("blocked").and_then(Value::as_bool).unwrap_or(false);
                    d.comments_more = v.get("hasMore").and_then(Value::as_bool).unwrap_or(false);
                    match comaintainers {
                        Some(names) if names != d.comaintainers => {
                            d.comaintainers = names;
                            true
                        }
                        _ => false,
                    }
                }
                None => false,
            }
        };
        if had_names {
            self.as_mut().detail_changed();
        }
        self.as_mut().comments_changed();
    }

    fn comment_translations_answer(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        let page = v.get("page").and_then(Value::as_i64).unwrap_or(-1);
        {
            let me = self.rust();
            // Ответ про закрытый или уже сменившийся товар молча отбрасываем —
            // как в `comments_answer`. Сторож у ядра свой, но между его
            // проверкой и этой строкой очередь окна успевает повернуться, и
            // последнее слово о том, что открыто, — за страницей.
            let open = me.detail.as_ref().map(|it| it.id.clone()).unwrap_or_default();
            if open.is_empty() || open != s(&v, "itemId") {
                return;
            }
            // Страница комментариев обязана быть ПОКАЗАННОЙ: показаны страницы
            // с нулевой по `comments_page - 1`. Перевод страницы, которую
            // список успел выбросить (человек закрыл и открыл комментарии
            // заново), не про эти строки.
            if page < 0 || page >= me.comments_page {
                return;
            }
        }
        // Пара «оригинал → перевод». Чистка та же, что у самого списка:
        // сравнивать надо с тем, что в строке лежит, а не с сырым ответом
        // магазина.
        let mut done: Vec<(String, String)> = Vec::new();
        if let Some(list) = v.get("rows").and_then(Value::as_array) {
            for raw in list {
                let source = ap::clean_comment_text(s(raw, "text"));
                let translated = ap::clean_comment_text(s(raw, "translated"));
                // Перевод, равный оригиналу, переводом не считается: он бы
                // только зажёг ссылку «показать оригинал» над той же самой
                // строкой.
                if source.is_empty() || translated.is_empty() || translated == source {
                    continue;
                }
                done.push((source, translated));
            }
        }
        if done.is_empty() {
            return;
        }
        let mut changed = false;
        {
            let mut me = self.as_mut().rust_mut();
            for row in me.comments.iter_mut() {
                if row.was_translated {
                    continue;
                }
                let Some((_, translated)) = done.iter().find(|(src, _)| *src == row.original) else {
                    continue;
                };
                row.translated = translated.clone();
                row.was_translated = true;
                // Переключатель «перевод ⇄ оригинал» остаётся в том положении,
                // в каком его оставил человек: он мог нажать «показать
                // оригинал» до прихода перевода — сбросить его значило бы
                // дёрнуть текст под рукой.
                changed = true;
            }
        }
        if changed {
            self.as_mut().comments_changed();
        }
    }

    fn toggle_comment(mut self: Pin<&mut Self>, index: &QString) {
        let Ok(index) = index.to_string().parse::<usize>() else {
            return;
        };
        {
            let mut me = self.as_mut().rust_mut();
            let Some(row) = me.comments.get_mut(index) else {
                return;
            };
            row.show_original = !row.show_original;
        }
        self.as_mut().comments_changed();
    }
}

// ============================================================================
// Подсказки поиска
// ============================================================================

impl qobject::AppearanceModel {
    fn suggest_debounce_ms(&self) -> i32 {
        ap::SUGGEST_DEBOUNCE_MS as i32
    }
    fn suggest_row_height(&self) -> i32 {
        ap::SUGGEST_ROW_H
    }
    fn suggest_thumb_width(&self) -> i32 {
        ap::SUGGEST_THUMB_W
    }
    fn suggest_thumb_height(&self) -> i32 {
        ap::SUGGEST_THUMB_H
    }

    fn suggest_rows(&self) -> QList<QVariant> {
        let me = self.rust();
        let mut out = QList::<QVariant>::default();
        // Строк не больше, чем помещается в выпадашку: число из ядра.
        for (i, item) in me
            .suggest_items
            .iter()
            .take(ap::SUGGEST_MAX_ROWS as usize)
            .enumerate()
        {
            let thumb = me
                .previews
                .get(&item.id)
                .cloned()
                .unwrap_or_else(|| item.preview_local.clone());
            out.append(QVariant::from(&vmap(vec![
                ("index", qstr(&i.to_string())),
                ("itemId", qstr(&item.id)),
                ("name", qstr(ap::card_name(&item.name))),
                ("author", qstr(ap::card_author(&item.author))),
                ("thumb", qstr(&file_url(&thumb))),
                ("current", QVariant::from(&(i as i64 == me.suggest_index))),
            ])));
        }
        out
    }

    fn suggest_visible(&self) -> bool {
        let me = self.rust();
        me.screen == "grid" && me.suggest_focused && !me.suggest_items.is_empty()
    }

    fn suggest_index(&self) -> i32 {
        self.rust().suggest_index as i32
    }

    fn suggest_geometry(
        &self,
        anchor_x: &QString,
        anchor_width: &QString,
        page_width: &QString,
    ) -> QMap<QMapPair_QString_QVariant> {
        let anchor_x = anchor_x.to_string().parse::<i32>().unwrap_or(0);
        let anchor_w = anchor_width.to_string().parse::<i32>().unwrap_or(0);
        let page_w = page_width.to_string().parse::<i32>().unwrap_or(0);
        let rows = self
            .rust()
            .suggest_items
            .len()
            .min(ap::SUGGEST_MAX_ROWS as usize) as i32;
        let width = ap::suggest_width(anchor_w);
        vmap(vec![
            (
                "x",
                QVariant::from(&ap::suggest_x(anchor_x, width, page_w)),
            ),
            ("width", QVariant::from(&width)),
            ("height", QVariant::from(&ap::suggest_height(rows))),
        ])
    }

    fn update_suggestions(mut self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        self.as_mut().rust_mut().suggest_query = text.clone();
        let action = {
            let me = self.rust();
            ap::suggest_action(&text, me.screen == "grid", &me.view)
        };
        match action {
            ap::SuggestAction::Hide => {
                {
                    let mut me = self.as_mut().rust_mut();
                    me.suggest_items.clear();
                    me.suggest_index = -1;
                }
                self.as_mut().suggest_changed();
            }
            // Местный раздел и AUR сети не знают: фильтруем то, что уже
            // показано, — мгновенно и без запроса (решение ядра).
            ap::SuggestAction::Local => {
                let items: Vec<ThemeItem> = {
                    let me = self.rust();
                    let loaded: Vec<ThemeItem> = me
                        .cards
                        .iter()
                        .filter_map(|c| match c {
                            Card::Theme(it) => Some((**it).clone()),
                            Card::Aur(_) => None,
                        })
                        .collect();
                    ap::local_filter(&loaded, &text)
                };
                {
                    let mut me = self.as_mut().rust_mut();
                    me.suggest_items = items;
                    me.suggest_index = -1;
                }
                self.as_mut().suggest_changed();
            }
            ap::SuggestAction::Network => {
                let request = {
                    let mut me = self.as_mut().rust_mut();
                    me.suggest_generation += 1;
                    let generation = me.suggest_generation;
                    json!({
                        "generation": generation,
                        "categoryId": me.view.query_category_id(),
                        "page": 0,
                        "sort": me.view.active_sort(),
                        "query": text,
                    })
                    .to_string()
                };
                self.as_mut().suggest_wanted(QString::from(&request));
            }
        }
    }

    fn set_search_focused(mut self: Pin<&mut Self>, on: bool) {
        {
            let mut me = self.as_mut().rust_mut();
            if me.suggest_focused == on {
                return;
            }
            me.suggest_focused = on;
            if !on {
                me.suggest_index = -1;
            }
        }
        self.as_mut().suggest_changed();
    }

    fn suggest_answer(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        let generation = v.get("generation").and_then(Value::as_u64).unwrap_or(0);
        // Человек печатал дальше, ушёл из строки или стёр введённое — ответ
        // уже не про то, что он видит (решение ядра).
        let fresh = {
            let me = self.rust();
            ap::suggest_answer_fresh(
                generation,
                me.suggest_generation,
                me.suggest_focused,
                &me.suggest_query,
            )
        };
        if !fresh {
            return;
        }
        let mut items: Vec<ThemeItem> = Vec::new();
        if let Some(list) = v.get("items").and_then(Value::as_array) {
            for raw in list {
                if let Ok(item) = serde_json::from_value::<ThemeItem>(raw.clone()) {
                    items.push(item);
                }
            }
        }
        {
            let mut me = self.as_mut().rust_mut();
            me.suggest_items = items;
            me.suggest_index = -1;
        }
        self.as_mut().suggest_changed();
    }

    fn suggest_move(mut self: Pin<&mut Self>, direction: &QString) {
        let delta: i64 = if direction.to_string() == "up" { -1 } else { 1 };
        {
            let mut me = self.as_mut().rust_mut();
            let shown = me.suggest_items.len().min(ap::SUGGEST_MAX_ROWS as usize) as i64;
            if shown == 0 {
                return;
            }
            // По кругу: с последней строки вниз — снова на первую.
            let next = me.suggest_index + delta;
            me.suggest_index = if next < 0 {
                shown - 1
            } else if next >= shown {
                0
            } else {
                next
            };
        }
        self.as_mut().suggest_changed();
    }

    fn suggest_activate(self: Pin<&mut Self>) -> bool {
        let index = self.rust().suggest_index;
        if index < 0 {
            return false;
        }
        self.suggest_open(index)
    }

    fn suggest_pick(self: Pin<&mut Self>, index: &QString) {
        let Ok(index) = index.to_string().parse::<i64>() else {
            return;
        };
        self.suggest_open(index);
    }

    /// Открыть подсказку под этим номером — как клик по плитке.
    fn suggest_open(mut self: Pin<&mut Self>, index: i64) -> bool {
        let item = self
            .rust()
            .suggest_items
            .get(index as usize)
            .cloned();
        let Some(item) = item else { return false };
        {
            let mut me = self.as_mut().rust_mut();
            me.suggest_items.clear();
            me.suggest_index = -1;
            me.suggest_focused = false;
        }
        self.as_mut().suggest_changed();
        // Подсказка ведёт прямо в панель деталей: товар уже найден, второй
        // раз просить его в сетке незачем.
        let id = item.id.clone();
        let known = self.rust().cards.iter().any(|c| c.id() == id);
        if !known {
            // Товар из выпадашки может не значиться в открытой сетке —
            // кладём его туда, иначе панели деталей неоткуда его взять.
            // Через сброс модели: менять состав строк молча нельзя.
            unsafe {
                self.as_mut().begin_reset_model();
                self.as_mut().rust_mut().cards.push(Card::Theme(Box::new(item)));
                self.as_mut().end_reset_model();
            }
            self.as_mut().count_changed();
        }
        self.as_mut().open_detail(&QString::from(&id));
        true
    }
}

// ============================================================================
// Карточка пакета AUR
// ============================================================================
//
// Общая часть панели деталей (имя, описание, картинка, комментарии, полоса
// хода) работает над товаром, собранным из плитки пакета; своё у пакета —
// сведения из паспорта RPC `info` и кнопка установки, та же, что на плитке
// (с очередью установок). Что показывать — решает ядро (`aur::info_facts`).

/// Запись проверки источника пакета из кеша (память процесса, при первом
/// обращении — `~/.cache/rexon/aur-trust.json`). Сети не касается.
fn aur_trust_entry(name: &str) -> Option<rexon_core::aur_trust::CacheEntry> {
    static PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let path = PATH.get_or_init(rexon_core::aur_trust::live_cache_path);
    rexon_core::aur_trust::cached_entry(path, name)
}

/// Признаки риска для плитки: данные RPC плитки и проверка, сделанная
/// раньше (если карточку уже открывали).
fn aur_tile_signals(pkg: &AurPackage) -> Vec<rexon_core::aur_trust::Signal> {
    let entry = aur_trust_entry(&pkg.name);
    rexon_core::aur_trust::signals(
        &rexon_core::aur_trust::RpcView::from(pkg),
        entry.as_ref(),
        rexon_core::aur_trust::now_secs(),
    )
}

/// Товар панели деталей из плитки пакета.
fn aur_detail_item(pkg: &AurPackage) -> ThemeItem {
    ThemeItem {
        id: pkg.name.clone(),
        name: pkg.name.clone(),
        description: pkg.description.clone(),
        author: pkg.maintainer.clone(),
        version: pkg.version.clone(),
        preview_url: pkg.preview_url(),
        installed: pkg.installed,
        ..ThemeItem::default()
    }
}

impl qobject::AppearanceModel {
    fn detail_source(&self) -> QString {
        let me = self.rust();
        let source = if me.aur_detail.is_some() {
            "aur"
        } else if me.detail.as_ref().is_some_and(|it| it.local_kind.is_some()) {
            "local"
        } else if me.detail.as_ref().is_some_and(|it| it.id.starts_with(cs::ID_PREFIX)) {
            "spices"
        } else {
            "store"
        };
        QString::from(source)
    }

    fn aur_facts(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        let Some(d) = self.rust().aur_detail.as_ref() else {
            return out;
        };
        // До ответа RPC — то, что известно с плитки; ответа не будет —
        // причина первой строкой.
        let info = d.info.clone().unwrap_or_else(|| rexon_core::aur::AurInfo {
            name: d.pkg.name.clone(),
            version: d.pkg.version.clone(),
            description: d.pkg.description.clone(),
            url: d.pkg.upstream_url.clone(),
            maintainer: d.pkg.maintainer.clone(),
            votes: d.pkg.votes,
            popularity: d.pkg.popularity,
            last_modified: d.pkg.last_modified,
            ..Default::default()
        });
        if !d.note.is_empty() {
            out.append(QVariant::from(&vmap(vec![
                ("label", qstr("")),
                ("value", qstr(&d.note)),
                ("html", qstr("")),
                ("warn", QVariant::from(&true)),
            ])));
        }
        for f in rexon_core::aur::info_facts(&info, &d.comaintainers) {
            let html = if f.link {
                let url = rexon_core::translator::html_escape(&f.value);
                // Цвет ссылки — в самом тексте: таблица стилей фрагмент
                // внутри подписи не красит (как у переключателя перевода).
                let accent = ui_base::active_skin_colors()
                    .get("accent")
                    .cloned()
                    .unwrap_or_default();
                format!("<a href=\"{url}\" style=\"color:{accent};text-decoration:none\">{url}</a>")
            } else {
                String::new()
            };
            out.append(QVariant::from(&vmap(vec![
                ("label", qstr(&f.label)),
                ("value", qstr(&f.value)),
                ("html", qstr(&html)),
                ("warn", QVariant::from(&f.warn)),
            ])));
        }
        out
    }

    fn comments_blocked(&self) -> bool {
        self.rust().aur_detail.as_ref().is_some_and(|d| d.comments_blocked)
    }

    fn comments_blocked_text(&self) -> QString {
        QString::from(&rexon_core::aur::comments_on_site_text())
    }

    fn aur_details_text(&self) -> QString {
        QString::from(&rexon_core::aur::details_btn())
    }

    fn aur_details_tooltip(&self) -> QString {
        QString::from(&rexon_core::aur::details_tooltip())
    }

    fn aur_detail_answer(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        let id = v.get("itemId").and_then(Value::as_str).unwrap_or_default().to_string();
        let info = v
            .get("info")
            .and_then(|i| serde_json::from_value::<rexon_core::aur::AurInfo>(i.clone()).ok());
        {
            let mut me = self.as_mut().rust_mut();
            // Ответ про закрытую или сменившуюся карточку молча отбрасываем.
            let me = &mut *me;
            let Some(d) = me.aur_detail.as_mut().filter(|d| d.pkg.name == id) else {
                return;
            };
            match info {
                Some(info) => {
                    // Описание и версия — свежие из паспорта (плитка могла
                    // прийти из кеша раздела).
                    if let Some(item) = me.detail.as_mut() {
                        if item.description != info.description {
                            item.description = info.description.clone();
                            item.description_translated.clear();
                        }
                        item.version = info.version.clone();
                        item.author = info.maintainer.clone();
                    }
                    d.note.clear();
                    d.info = Some(info);
                }
                None => {
                    d.note = if v.get("missing").and_then(Value::as_bool).unwrap_or(false) {
                        rexon_core::aur::info_missing_text()
                    } else {
                        let why = v.get("error").and_then(Value::as_str).unwrap_or_default();
                        rexon_core::i18n::trf("Couldn't load the package details: %1", &[&why])
                    };
                }
            }
        }
        self.as_mut().detail_changed();
        // Комментарии — со страницы пакета, после паспорта: два обращения к
        // сайту подряд не уходят разом.
        self.as_mut()
            .comments_wanted(QString::from(&id), QString::from("0"));
    }

    /// Признаки риска открытой карточки: глубокая проверка (если есть) и
    /// паспорт RPC (до ответа — данные плитки).
    fn aur_trust_signals(&self) -> Vec<rexon_core::aur_trust::Signal> {
        let Some(d) = self.rust().aur_detail.as_ref() else {
            return Vec::new();
        };
        let rpc = match d.info.as_ref() {
            Some(i) => rexon_core::aur_trust::RpcView::from(i),
            None => rexon_core::aur_trust::RpcView::from(&d.pkg),
        };
        rexon_core::aur_trust::signals(&rpc, d.trust.as_ref(), rexon_core::aur_trust::now_secs())
    }

    fn aur_trust_rows(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for s in self.aur_trust_signals() {
            out.append(QVariant::from(&vmap(vec![
                ("text", qstr(&s.text)),
                ("why", qstr(&s.why)),
                ("danger", QVariant::from(&(s.level == rexon_core::aur_trust::Level::Danger))),
            ])));
        }
        out
    }

    fn aur_trust_status(&self) -> QString {
        let Some(d) = self.rust().aur_detail.as_ref() else {
            return QString::default();
        };
        if !d.trust_done && d.trust.as_ref().and_then(|e| e.facts.as_ref()).is_none() {
            return QString::from(&rexon_core::aur_trust::status_checking());
        }
        let signals = self.aur_trust_signals();
        let facts = d.trust.as_ref().and_then(|e| e.facts.as_ref());
        QString::from(&rexon_core::aur_trust::status_text(facts, &signals))
    }

    fn aur_trust_level(&self) -> QString {
        let level = rexon_core::aur_trust::level(&self.aur_trust_signals());
        QString::from(level.map(|l| l.key()).unwrap_or(""))
    }

    fn aur_trust_badge(&self) -> QString {
        QString::from(&rexon_core::aur_trust::badge_text(&self.aur_trust_signals()))
    }

    fn aur_trust_tooltip(&self) -> QString {
        QString::from(&rexon_core::aur_trust::badge_tooltip(&self.aur_trust_signals()))
    }

    fn aur_pkgbuild_title(&self) -> QString {
        let name = self.rust().aur_detail.as_ref().map(|d| d.pkg.name.clone()).unwrap_or_default();
        QString::from(&rexon_core::aur_trust::pkgbuild_title(&name))
    }

    /// Ветка git пакета в AUR (PKGBUILD лежит под ней, а не под именем
    /// пакета, если база общая).
    fn aur_package_base(&self) -> QString {
        let Some(d) = self.rust().aur_detail.as_ref() else {
            return QString::default();
        };
        let base = d
            .info
            .as_ref()
            .map(|i| i.package_base.clone())
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| d.pkg.name.clone());
        QString::from(&base)
    }

    fn aur_trust_title(&self) -> QString {
        QString::from(&rexon_core::aur_trust::section_title())
    }

    fn aur_trust_note(&self) -> QString {
        QString::from(&rexon_core::aur_trust::section_note())
    }

    fn aur_trust_danger_color(&self) -> QString {
        QString::from(rexon_core::aur_trust::DANGER_COLOR)
    }

    fn aur_pkgbuild_btn(&self) -> QString {
        QString::from(&rexon_core::aur_trust::pkgbuild_btn())
    }

    fn aur_pkgbuild_btn_tooltip(&self) -> QString {
        QString::from(&rexon_core::aur_trust::pkgbuild_tooltip())
    }

    fn aur_pkgbuild_loading(&self) -> QString {
        QString::from(&rexon_core::aur_trust::pkgbuild_loading())
    }

    fn aur_trust_answer(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        let id = v.get("itemId").and_then(Value::as_str).unwrap_or_default().to_string();
        let entry = v
            .get("entry")
            .and_then(|e| serde_json::from_value::<rexon_core::aur_trust::CacheEntry>(e.clone()).ok());
        {
            let mut me = self.as_mut().rust_mut();
            let Some(d) = me.aur_detail.as_mut().filter(|d| d.pkg.name == id) else {
                return;
            };
            d.trust_done = true;
            if entry.is_some() {
                d.trust = entry;
            }
        }
        self.as_mut().detail_changed();
        // Плитка того же пакета теперь знает больше — перерисовать её.
        let row = self
            .rust()
            .cards
            .iter()
            .position(|c| matches!(c, Card::Aur(p) if p.name == id));
        if let Some(r) = row {
            let index = self.as_ref().index(r as i32, 0, &QModelIndex::default());
            self.as_mut().data_changed(&index, &index, &cxx_qt_lib::QVector::<i32>::default());
        }
    }

    fn detail_aur_button(&self) -> QMap<QMapPair_QString_QVariant> {
        let me = self.rust();
        let Some(d) = me.aur_detail.as_ref() else {
            return vmap(Vec::new());
        };
        // Плитка в сетке — первоисточник состояния (её отмечает очередь
        // установок); нет её там (сетку пересобрали) — пакет карточки.
        let pkg = me
            .cards
            .iter()
            .find_map(|c| match c {
                Card::Aur(p) if p.name == d.pkg.name => Some((**p).clone()),
                _ => None,
            })
            .unwrap_or_else(|| d.pkg.clone());
        let role = |name: &str| self.aur_role(&pkg, name);
        vmap(vec![
            ("text", role("buttonText")),
            ("enabled", role("buttonEnabled")),
            ("tooltip", role("buttonTooltip")),
            ("state", role("buttonState")),
        ])
    }
}

/// Есть ли редактор цветовых схем KDE (`kcolorschemeeditor`) в PATH.
fn kde_scheme_editor_present() -> bool {
    let path = std::env::var("PATH").ok();
    rexon_core::utils::which(rexon_core::color_scheme_edit::KDE_EDITOR, path.as_deref()).is_some()
}

/// Запустить окно сторонней программы (настройки стиля, редактор схем):
/// без ввода и вывода, своей группой процессов — закрытие Rexon его не
/// гасит. Ошибка — для человека.
fn spawn_detached(argv: &[String]) -> Result<std::process::Child, String> {
    use std::os::unix::process::CommandExt;
    let Some((program, args)) = argv.split_first() else {
        return Err(String::new());
    };
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| rexon_core::i18n::trf("Couldn't start %1: %2", &[&program, &e]))
}
