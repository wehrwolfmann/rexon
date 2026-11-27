// AppearanceModel — the «Appearance» tab in Rust + cxx-qt + QML.
//
// The last of the tabs ported to QML following the common scheme:
// decisions and labels are the core's, here is the QObject shell, and QML
// holds only the markup.
//
// What is here and what is not:
//   * there IS the QObject shell: the section sidebar, the header (source
//     chips, the "kind" and lock mode sub-chips, the search, the desktop
//     choice, the sort), the status line, the page strip and the catalogue
//     tile model with the delegate's roles;
//   * there is NOT a single line of product decisions. What to show, in
//     what order, with what words and with which button is answered by
//     `rexon_core::appearance_page` (2,690 lines of ported logic).
//
// Warning: the network and the disk stay outside. The core has parsing of
// the store answers (`ocs`), AUR (`aur`) and the disk walk
// (`installed_scan`), but there is no LIVE `OcsCacheIo` executor in the
// product yet — the SQLite cache and HTTP live in
// `rexon/appearance/ocs_client.py`. So the page asks the host
// (`root.actions`) exactly what the core itself told it to ask: the
// request contents are assembled by `catalogRequest()`, the answer is
// accepted by `catalogAnswer()`. When the executor arrives in Rust, it
// will take the host's place — neither the page nor this bridge will
// change.

/// cxx-qt bridge: the content of the «Appearance» tab. It is also the tile
/// model.
///
/// Warning: the type is called `AppearanceModel`, not `AppearancePage`: the
/// module's .qml files themselves become QML types by file name, and
/// `AppearancePage` is taken by `qml/AppearancePage.qml` (pitfall 1 of the
/// porting scheme).
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// The base Qt type. WITHOUT `#[qobject]`: for every `#[qobject]` type in
        /// `extern "C++Qt"` cxx-qt emits cast shims named by TYPE, and a second
        /// bridge of the crate with the same base gives a `duplicate symbol`.
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
        /// Set of roles for dataChanged (in Qt6 QVector<int> == QList<int>).
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
        // ── Window and header: constants of the core ────────────────────
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
        // Catalogue geometry: the sidebar, the search line, the tile.
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
        // ── Search suggestions ──────────────────────────────────────────
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
        // Fixed labels of the header.
        #[qproperty(QString, source_label, cxx_name = "sourceLabel",
                    READ = source_label, CONSTANT)]
        #[qproperty(QString, submode_label, cxx_name = "submodeLabel",
                    READ = submode_label, CONSTANT)]
        #[qproperty(QString, orientation_bar_label, cxx_name = "orientationBarLabel",
                    READ = orientation_bar_label, CONSTANT)]
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
        // Sorts: [{label, key}, …] — the core's table, it does not change during
        // the tab's lifetime.
        #[qproperty(QList_QVariant, sorts, READ = sorts, NOTIFY = chrome_changed)]

        // ── Palette of the active skin (rule #17) ───────────────────────
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        // Layout of the group sections (`sty` for the "MATE panel" section).
        #[qproperty(QMap_QString_QVariant, section_styles, cxx_name = "sectionStyles",
                    READ = section_styles, NOTIFY = palette_changed)]

        // ── Section sidebar ─────────────────────────────────────────────
        // [{key, icon, name, text, available, current}, …]
        #[qproperty(QList_QVariant, sidebar_rows, cxx_name = "sidebarRows",
                    READ = sidebar_rows, NOTIFY = sidebar_changed)]
        #[qproperty(QString, section_title, cxx_name = "sectionTitle",
                    READ = section_title, NOTIFY = sidebar_changed)]

        // ── Header: what is visible for the current section ─────────────
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
        // The "All / Light / Dark" filter for colour schemes and Plasma styles.
        #[qproperty(bool, tone_bar_visible, cxx_name = "toneBarVisible",
                    READ = tone_bar_visible, NOTIFY = chrome_changed)]
        // "All / Horizontal / Vertical" in a store that knows the picture size.
        #[qproperty(bool, orientation_bar_visible, cxx_name = "orientationBarVisible",
                    READ = orientation_bar_visible, NOTIFY = chrome_changed)]
        #[qproperty(bool, cursor_size_bar_visible, cxx_name = "cursorSizeBarVisible",
                    READ = cursor_size_bar_visible, NOTIFY = chrome_changed)]
        #[qproperty(bool, sort_visible, cxx_name = "sortVisible",
                    READ = sort_visible, NOTIFY = chrome_changed)]
        #[qproperty(bool, pagination_visible, cxx_name = "paginationVisible",
                    READ = pagination_visible, NOTIFY = chrome_changed)]
        // "Install from file…" in the section header: visibility, the labels of
        // the button and the menu, the title and filters of the choice window, the
        // section key — all the core's (`appearance_page`, `local_install`).
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
        // The state of "Only for your GNOME version" of the extensions.gnome.org
        // showcase — the model's truth (`setEgoCompatibleOnly`).
        #[qproperty(bool, ego_compatible_only, cxx_name = "egoCompatibleOnly",
                    READ = ego_compatible_only, NOTIFY = chrome_changed)]
        // The section of the "Settings" tab on the `settings` screen (the core:
        // `CatalogView::settings_panel_key`).
        #[qproperty(QString, settings_panel, cxx_name = "settingsPanel",
                    READ = settings_panel, NOTIFY = chrome_changed)]
        // The «⚙️ Settings» / "← Back to catalogue" button in the header of
        // "Fonts", "Icons", "Cursors", window borders and "GTK themes" (the core:
        // `CatalogView::section_settings_panels`, `section_settings_open`).
        #[qproperty(bool, section_settings_visible, cxx_name = "sectionSettingsVisible",
                    READ = section_settings_visible, NOTIFY = chrome_changed)]
        #[qproperty(bool, section_settings_open, cxx_name = "sectionSettingsOpen",
                    READ = section_settings_open, NOTIFY = chrome_changed)]
        #[qproperty(QString, section_settings_text, cxx_name = "sectionSettingsText",
                    READ = section_settings_text, NOTIFY = chrome_changed)]
        #[qproperty(QString, section_settings_tooltip, cxx_name = "sectionSettingsTooltip",
                    READ = section_settings_tooltip, NOTIFY = chrome_changed)]
        // Installed items on top above the section's store, whether to highlight
        // the current theme (the core: `CatalogView::installed_view`,
        // `marks_current`).
        #[qproperty(bool, installed_view, cxx_name = "installedView",
                    READ = installed_view, NOTIFY = chrome_changed)]
        #[qproperty(bool, mark_current, cxx_name = "markCurrent",
                    READ = mark_current, NOTIFY = chrome_changed)]
        // Chips: [{key, label, tooltip, checked}, …].
        #[qproperty(QList_QVariant, source_chips, cxx_name = "sourceChips",
                    READ = source_chips, NOTIFY = chrome_changed)]
        #[qproperty(QList_QVariant, style_chips, cxx_name = "styleChips",
                    READ = style_chips, NOTIFY = chrome_changed)]
        #[qproperty(QList_QVariant, lock_chips, cxx_name = "lockChips",
                    READ = lock_chips, NOTIFY = chrome_changed)]
        #[qproperty(QList_QVariant, tone_chips, cxx_name = "toneChips",
                    READ = tone_chips, NOTIFY = chrome_changed)]
        #[qproperty(QList_QVariant, orientation_chips, cxx_name = "orientationChips",
                    READ = orientation_chips, NOTIFY = chrome_changed)]
        // Cursor size chips: [{key, label, tooltip}, …]; which one is checked is
        // decided by the page by the answer of `actions.cursorSize()`.
        #[qproperty(QList_QVariant, cursor_size_chips, cxx_name = "cursorSizeChips",
                    READ = cursor_size_chips, CONSTANT)]
        #[qproperty(QString, sort_key, cxx_name = "sortKey",
                    READ = sort_key, NOTIFY = chrome_changed)]
        #[qproperty(QString, query, READ = query, NOTIFY = chrome_changed)]
        // The desktop choice: [{id, name}, …] and the current one.
        #[qproperty(QList_QVariant, de_choices, cxx_name = "deChoices",
                    READ = de_choices, NOTIFY = de_changed)]
        #[qproperty(QString, de, READ = de, NOTIFY = de_changed)]

        // ── What is shown: the grid, a detail or an explanation ─────────
        #[qproperty(QString, screen, READ = screen, NOTIFY = screen_changed)]
        #[qproperty(QString, note_text, cxx_name = "noteText",
                    READ = note_text, NOTIFY = screen_changed)]
        // The login screen panel with Plasma Login Manager
        // (`screen == "plasmalogin"`): the core's labels and whether the settings
        // button is visible.
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
        // The login screen section: above the shelf and the panel — the "Login
        // manager" block (the data is the `loginScreenReady` probe).
        #[qproperty(bool, login_section, cxx_name = "loginSection",
                    READ = login_section, NOTIFY = screen_changed)]

        // ── Status line and page strip ──────────────────────────────────
        #[qproperty(QString, status_text, cxx_name = "statusText",
                    READ = status_text, NOTIFY = status_changed)]
        // The status line warns (⚠: the wallpaper was set with a caveat, or an
        // error) — QML paints it with the `state_warn` warning colour.
        #[qproperty(bool, status_warn, cxx_name = "statusWarn",
                    READ = status_warn, NOTIFY = status_changed)]
        #[qproperty(QString, page_label, cxx_name = "pageLabel",
                    READ = page_label, NOTIFY = page_changed)]
        #[qproperty(bool, prev_enabled, cxx_name = "prevEnabled",
                    READ = prev_enabled, NOTIFY = page_changed)]
        #[qproperty(bool, next_enabled, cxx_name = "nextEnabled",
                    READ = next_enabled, NOTIFY = page_changed)]
        // ── Details panel ───────────────────────────────────────────────
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
        // File choice for a store item with several files: the rows are the
        // core's decision (`ap::store_file_choices_json`), and the labels are its
        // too.
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
        // ── Listening to sound themes (parity with KDE "System sounds") ─
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
        // ── "Demonstrate" for a KDE splash (parity with the KDE module) ─
        #[qproperty(QString, splash_demo_label, cxx_name = "splashDemoLabel",
                    READ = splash_demo_label, CONSTANT)]
        #[qproperty(QString, splash_demo_tooltip, cxx_name = "splashDemoTooltip",
                    READ = splash_demo_tooltip, CONSTANT)]
        /// The sound theme on disk shown by the details panel (empty — not a sound
        /// theme).
        #[qproperty(QString, detail_sound_theme, cxx_name = "detailSoundTheme",
                    READ = detail_sound_theme, NOTIFY = detail_changed)]
        /// A Store item from the "Sounds" section is open — it can be listened to
        /// before installation.
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
        // "Remove" for a theme from disk: unavailable for an enabled and a system
        // one, the reason in a tooltip (`theme_removal`).
        #[qproperty(bool, detail_uninstall_enabled, cxx_name = "detailUninstallEnabled",
                    READ = detail_uninstall_enabled, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_uninstall_tooltip, cxx_name = "detailUninstallTooltip",
                    READ = detail_uninstall_tooltip, NOTIFY = detail_changed)]
        // The pop-up line of a deferred removal: "Undo" and how long it hangs.
        #[qproperty(QString, removal_undo_text, cxx_name = "removalUndoText",
                    READ = removal_undo_text, CONSTANT)]
        #[qproperty(i32, removal_undo_ms, cxx_name = "removalUndoMs",
                    READ = removal_undo_ms, CONSTANT)]
        #[qproperty(QString, detail_favorite_text, cxx_name = "detailFavoriteText",
                    READ = detail_favorite_text, NOTIFY = detail_changed)]
        #[qproperty(bool, detail_apply_local_enabled, cxx_name = "detailApplyLocalEnabled",
                    READ = detail_apply_local_enabled, NOTIFY = detail_changed)]
        // "Configure…" for an application style and a Kvantum theme (JSON
        // `themes::StyleConfigButton`, without `argv`).
        #[qproperty(QString, detail_style_config, cxx_name = "detailStyleConfig",
                    READ = detail_style_config, NOTIFY = detail_changed)]
        // Buttons of a GNOME extension in the details (JSON: `visible`, `toggle`
        // {text, state, enabled}, `tooltip`, `prefs`, `folder`, `remove`,
        // `error`, `url`, `name`; `GnomeExtra`).
        #[qproperty(QString, detail_gnome, cxx_name = "detailGnome",
                    READ = detail_gnome, NOTIFY = detail_changed)]
        // "Apply to GTK 4/libadwaita programs" for a GTK theme (JSON
        // `gtk4_theme::Toggle`).
        #[qproperty(QString, detail_gtk4, cxx_name = "detailGtk4",
                    READ = detail_gtk4, NOTIFY = detail_changed)]
        // "Configure…" for a KDE window border (JSON `kdecoration::ConfigButton`,
        // without `argv`).
        #[qproperty(QString, detail_deco_config, cxx_name = "detailDecoConfig",
                    READ = detail_deco_config, NOTIFY = detail_changed)]
        // "Edit…" / "Create based on…" for a colour scheme (JSON: `visible` +
        // the `color_scheme_edit::EditorTexts` labels).
        #[qproperty(QString, detail_scheme_edit, cxx_name = "detailSchemeEdit",
                    READ = detail_scheme_edit, NOTIFY = detail_changed)]
        // The «Start at login» toggle — only for Conky widgets.
        #[qproperty(bool, detail_autostart_visible, cxx_name = "detailAutostartVisible",
                    READ = detail_autostart_visible, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_conky_id, cxx_name = "detailConkyId",
                    READ = detail_conky_id, NOTIFY = detail_changed)]
        #[qproperty(QString, detail_autostart_text, cxx_name = "detailAutostartText",
                    READ = detail_autostart_text, CONSTANT)]
        #[qproperty(QString, detail_preview, cxx_name = "detailPreview",
                    READ = detail_preview, NOTIFY = detail_frame_changed)]
        #[qproperty(QString, detail_preview_text, cxx_name = "detailPreviewText",
                    READ = detail_preview_text, NOTIFY = detail_frame_changed)]
        #[qproperty(QString, detail_item_id, cxx_name = "detailItemId",
                    READ = detail_item_id, NOTIFY = detail_changed)]
        // ── Screenshot gallery ──────────────────────────────────────────
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
        // ── Comments ────────────────────────────────────────────────────
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
        /// A long action is in progress (installation, building, applying): the
        /// start buttons are locked.
        #[qproperty(bool, action_busy, cxx_name = "actionBusy",
                    READ = action_busy, NOTIFY = install_changed)]
        /// Whether the progress bar is visible: building with the AUR helper and
        /// applying a theme have no progress share at all (the core's decision).
        #[qproperty(bool, progress_visible, cxx_name = "progressVisible",
                    READ = progress_visible, NOTIFY = install_changed)]
        /// The progress share, 0–100 (the core clamps the value).
        #[qproperty(i32, install_percent, cxx_name = "installPercent",
                    READ = install_percent, NOTIFY = install_changed)]
        // ── Preview loader ──────────────────────────────────────────────
        /// Whether there is a ready preview not yet put on a tile. While `true`,
        /// the page spins the delivery timer: six JPEGs decoded in one pass of the
        /// event loop give a noticeable jerk.
        #[qproperty(bool, preview_ready_pending, cxx_name = "previewReadyPending",
                    READ = preview_ready_pending, NOTIFY = preview_changed)]
        /// The pause before the background warm-up of the shown grid's gallery.
        #[qproperty(i32, gallery_warm_delay_ms, cxx_name = "galleryWarmDelayMs",
                    READ = gallery_warm_delay_ms, CONSTANT)]
        /// How long after the tab is shown the background warm-up of the other
        /// sections begins: it must not compete with the open one for the network.
        #[qproperty(i32, prefetch_start_ms, cxx_name = "prefetchStartMs",
                    READ = prefetch_start_ms, CONSTANT)]
        /// The pause between background warm-ups — do not hammer the store.
        #[qproperty(i32, prefetch_pause_ms, cxx_name = "prefetchPauseMs",
                    READ = prefetch_pause_ms, CONSTANT)]
        /// How long after the tab is shown the stale translations are cleaned up
        /// once: the work is not urgent and there is no reason to wait for it.
        #[qproperty(i32, purge_translations_ms, cxx_name = "purgeTranslationsMs",
                    READ = purge_translations_ms, CONSTANT)]
        /// How long to wait for the Qt style change to settle after a theme is
        /// applied.
        #[qproperty(i32, style_storm_ms, cxx_name = "styleStormMs",
                    READ = style_storm_ms, CONSTANT)]
        /// It is time to rebuild the grid — but only once the style storm settles.
        #[qproperty(bool, reload_pending, cxx_name = "reloadPending",
                    READ = reload_pending, NOTIFY = install_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        // ── AUR package card ──
        // Where the opened item comes from: `store` | `local` | `aur`.
        #[qproperty(QString, detail_source, cxx_name = "detailSource",
                    READ = detail_source, NOTIFY = detail_changed)]
        // Package information: [{label, value, html, warn}].
        #[qproperty(QList_QVariant, aur_facts, cxx_name = "aurFacts",
                    READ = aur_facts, NOTIFY = detail_changed)]
        // The package page was not given — the comments are only on the site.
        #[qproperty(bool, comments_blocked, cxx_name = "commentsBlocked",
                    READ = comments_blocked, NOTIFY = comments_changed)]
        #[qproperty(QString, comments_blocked_text, cxx_name = "commentsBlockedText",
                    READ = comments_blocked_text, CONSTANT)]
        // "📄 More" on the package tile.
        #[qproperty(QString, aur_details_text, cxx_name = "aurDetailsText",
                    READ = aur_details_text, CONSTANT)]
        #[qproperty(QString, aur_details_tooltip, cxx_name = "aurDetailsTooltip",
                    READ = aur_details_tooltip, CONSTANT)]
        // The AUR package source check (`aur_trust`): risk signals
        // [{text, why, danger}], the outcome in words, the badge at «Install».
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

    // ── Signals ─────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        /// The skin was changed on the fly — recompute the palette. The push comes
        /// from the shared registry (`crate::skin`), not from here.
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

        /// The request contents changed — it is time for the page to ask the host.
        /// A separate signal rather than "go yourself": the decision about WHAT to
        /// ask is made by the core, and only the host can go to the network.
        #[qsignal]
        #[cxx_name = "catalogWanted"]
        fn catalog_wanted(self: Pin<&mut AppearanceModel>);

        /// The details panel shows another item.
        #[qsignal]
        #[cxx_name = "detailChanged"]
        fn detail_changed(self: Pin<&mut AppearanceModel>);

        /// The frame in place of the details picture changed.
        #[qsignal]
        #[cxx_name = "detailFrameChanged"]
        fn detail_frame_changed(self: Pin<&mut AppearanceModel>);

        /// The outcome of installing/applying under the details buttons.
        #[qsignal]
        #[cxx_name = "detailStatusChanged"]
        fn detail_status_changed(self: Pin<&mut AppearanceModel>);

        /// The progress of a long action changed (started, moved, ended).
        #[qsignal]
        #[cxx_name = "installChanged"]
        fn install_changed(self: Pin<&mut AppearanceModel>);

        /// The item needs its full details (description, all screenshots).
        #[qsignal]
        #[cxx_name = "detailWanted"]
        fn detail_wanted(self: Pin<&mut AppearanceModel>, item_id: QString);

        /// A gallery frame is needed: the number and the picture address (both as
        /// strings — a number in a cxx-qt metaobject signature is not parsed from
        /// outside).
        #[qsignal]
        #[cxx_name = "galleryWanted"]
        fn gallery_wanted(self: Pin<&mut AppearanceModel>, index: QString, url: QString);

        /// A page of item comments is needed.
        #[qsignal]
        #[cxx_name = "commentsWanted"]
        fn comments_wanted(self: Pin<&mut AppearanceModel>, item_id: QString, page: QString);

        #[qsignal]
        #[cxx_name = "commentsChanged"]
        fn comments_changed(self: Pin<&mut AppearanceModel>);

        #[qsignal]
        #[cxx_name = "suggestChanged"]
        fn suggest_changed(self: Pin<&mut AppearanceModel>);

        /// The suggestions need a fresh store answer (the request as a JSON
        /// string).
        #[qsignal]
        #[cxx_name = "suggestWanted"]
        fn suggest_wanted(self: Pin<&mut AppearanceModel>, request: QString);

        /// A tile picture or a background warm-up of a gallery frame is needed.
        ///
        /// `job` is the request number: the host parses nothing in it but only
        /// returns it back together with the answer. Otherwise two tiles sharing one
        /// address, and a live request over a background one, would be
        /// indistinguishable. `low` is a background request: there is nothing to
        /// show it on, it only fills the cache and yields to live tiles.
        #[qsignal]
        #[cxx_name = "previewWanted"]
        fn preview_wanted(
            self: Pin<&mut AppearanceModel>,
            job: QString,
            item_id: QString,
            url: QString,
            low: bool,
        );

        /// The failure was temporary — repeat the request after `delay_ms`.
        /// `generation` carries the queue generation: a section change changes it,
        /// and a repeat of the previous section is silently thrown away.
        #[qsignal]
        #[cxx_name = "previewRetryWanted"]
        fn preview_retry_wanted(
            self: Pin<&mut AppearanceModel>,
            generation: QString,
            job: QString,
            delay_ms: QString,
        );

        /// A background warm-up is needed: the request contents as a JSON string.
        /// The warm-up is shown to nobody — it only fills the cache so that the
        /// neighbouring section and the next page open without the network.
        #[qsignal]
        #[cxx_name = "prefetchWanted"]
        fn prefetch_wanted(self: Pin<&mut AppearanceModel>, request: QString);

        /// The section changed — everything flying for previews can be aborted.
        #[qsignal]
        #[cxx_name = "previewsCanceled"]
        fn previews_canceled(self: Pin<&mut AppearanceModel>);

        /// The set of ready previews changed (the delivery timer is kept by it).
        #[qsignal]
        #[cxx_name = "previewChanged"]
        fn preview_changed(self: Pin<&mut AppearanceModel>);

        /// dataChanged of the base class — a targeted row update.
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

    // index() is safe, but cxx-qt requires keeping #[inherit] in an unsafe
    // block (unsafe here is about matching the C++ signature, not about
    // danger).
    unsafe extern "RustQt" {
        #[inherit]
        fn index(
            self: &AppearanceModel,
            row: i32,
            column: i32,
            parent: &QModelIndex,
        ) -> QModelIndex;
    }

    // ── Inherited methods of the base class ─────────────────────────────
    extern "RustQt" {
        /// # Safety
        /// Requires a paired end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut AppearanceModel>);
        /// # Safety
        /// Paired with begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut AppearanceModel>);
    }

    // ── QAbstractListModel overrides ────────────────────────────────────
    // Without #[qinvokable]: an invokable in a subclass shadows the shortened
    // clones of the base's signatures, and rowCount() without arguments from
    // JS fails.
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

    // ── Property reading ────────────────────────────────────────────────
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
        #[cxx_name = "orientationBarLabel"]
        fn orientation_bar_label(self: &AppearanceModel) -> QString;
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
        #[cxx_name = "orientationBarVisible"]
        fn orientation_bar_visible(self: &AppearanceModel) -> bool;
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
        #[cxx_name = "egoCompatibleOnly"]
        fn ego_compatible_only(self: &AppearanceModel) -> bool;
        #[cxx_name = "settingsPanel"]
        fn settings_panel(self: &AppearanceModel) -> QString;
        #[cxx_name = "sectionSettingsVisible"]
        fn section_settings_visible(self: &AppearanceModel) -> bool;
        #[cxx_name = "sectionSettingsOpen"]
        fn section_settings_open(self: &AppearanceModel) -> bool;
        #[cxx_name = "sectionSettingsText"]
        fn section_settings_text(self: &AppearanceModel) -> QString;
        #[cxx_name = "sectionSettingsTooltip"]
        fn section_settings_tooltip(self: &AppearanceModel) -> QString;
        #[cxx_name = "sourceChips"]
        fn source_chips(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "styleChips"]
        fn style_chips(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "lockChips"]
        fn lock_chips(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "toneChips"]
        fn tone_chips(self: &AppearanceModel) -> QList_QVariant;
        #[cxx_name = "orientationChips"]
        fn orientation_chips(self: &AppearanceModel) -> QList_QVariant;
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
        #[cxx_name = "statusWarn"]
        fn status_warn(self: &AppearanceModel) -> bool;
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
        #[cxx_name = "detailGnome"]
        fn detail_gnome(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailGtk4"]
        fn detail_gtk4(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailDecoConfig"]
        fn detail_deco_config(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailSchemeEdit"]
        fn detail_scheme_edit(self: &AppearanceModel) -> QString;
        #[cxx_name = "detailAutostartVisible"]
        fn detail_autostart_visible(self: &AppearanceModel) -> bool;
        fn detail_conky_id(self: &AppearanceModel) -> QString;
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

    // ── API for QML and the page host ───────────────────────────────────
    //
    // Warning: not a single invokable with a number in its signature: cxx-qt
    // writes the type into the metaobject as `::std::int32_t`, there is no
    // metatype with that name, and a call through `QMetaObject.invokeMethod`
    // silently returns False (pitfall 12 of the scheme). Where a number is
    // needed, it travels as a string.
    extern "RustQt" {
        /// Columns of the tile grid at width `usable` (`ap::grid_columns`).
        #[qinvokable]
        #[cxx_name = "gridColumns"]
        fn grid_columns(self: &AppearanceModel, usable: i32) -> i32;

        /// Width of a grid cell at width `usable` (`ap::grid_cell_width`).
        #[qinvokable]
        #[cxx_name = "gridCellWidth"]
        fn grid_cell_width(self: &AppearanceModel, usable: i32) -> i32;

        /// Facts about the machine that the core will not get by itself: the list of
        /// Qt styles, the active widget style, the remembered desktop choice. The
        /// page host calls it once at start-up (a JSON string).
        #[qinvokable]
        #[cxx_name = "setEnvironment"]
        fn set_environment(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// What draws the login screen and whether this is SteamOS — the answer of
        /// the action object's background probe (`loginScreenReady`, JSON
        /// `{plasmaLogin, steamos, canOpen}`). Rebuilds the sidebar: with Plasma
        /// Login Manager its section takes the place of the SDDM shelf.
        #[qinvokable]
        #[cxx_name = "setLoginScreen"]
        fn set_login_screen(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Opening the login screen settings failed — say so in the status line and
        /// name where to go by hand.
        #[qinvokable]
        #[cxx_name = "loginSettingsFailed"]
        fn login_settings_failed(self: Pin<&mut AppearanceModel>);

        /// The "System sounds are off" strip of the "Sounds" section: JSON
        /// `ap::EventSoundsBanner`. Reads the running environment's toggle; if
        /// another desktop is chosen in the catalogue, there is no strip (its key
        /// cannot be enabled from here). Synchronous: one short command.
        #[qinvokable]
        #[cxx_name = "eventSoundsBanner"]
        fn event_sounds_banner(self: &AppearanceModel) -> QString;

        /// "Enable" on the strip: the same write path as the toggle in the
        /// "Settings" tab. The outcome is a line for the user.
        #[qinvokable]
        #[cxx_name = "enableEventSounds"]
        fn enable_event_sounds(self: &AppearanceModel) -> QString;

        /// The "Revert to what it was" strip of the open section: JSON
        /// `ap::AppliedRevertBanner` (the "Apply" record of the shared journal, for
        /// colours also the layer's memo and the shell strip button). Synchronous:
        /// two files and a shell probe, like `shellPalette`.
        #[qinvokable]
        #[cxx_name = "appliedRevertBanner"]
        fn applied_revert_banner(self: &AppearanceModel) -> QString;

        /// Open a sidebar section.
        #[qinvokable]
        #[cxx_name = "selectCategory"]
        fn select_category(self: Pin<&mut AppearanceModel>, key: &QString);

        /// Sections with a «⚙️ Settings» view: toggle the "catalogue ↔ ⚙️ Settings"
        /// view.
        #[qinvokable]
        #[cxx_name = "toggleSectionSettings"]
        fn toggle_section_settings(self: Pin<&mut AppearanceModel>);

        /// The store: `ocs` | `aur` | `both`.
        #[qinvokable]
        #[cxx_name = "setSource"]
        fn set_source(self: Pin<&mut AppearanceModel>, key: &QString);

        /// The mode of the "Lock screen" section.
        #[qinvokable]
        #[cxx_name = "setLockMode"]
        fn set_lock_mode(self: Pin<&mut AppearanceModel>, key: &QString);

        /// Sub-choice of the "Application style" section (`kvantum` or
        /// `qt:<Name>`).
        #[qinvokable]
        #[cxx_name = "setStyleSub"]
        fn set_style_sub(self: Pin<&mut AppearanceModel>, key: &QString);

        /// The tone filter for colour schemes and Plasma styles (`lp::TONE_*`).
        #[qinvokable]
        #[cxx_name = "setToneFilter"]
        fn set_tone_filter(self: Pin<&mut AppearanceModel>, key: &QString);

        /// The orientation filter of a store that knows the picture size
        /// (`WallpaperOrientation` key, empty — "All").
        #[qinvokable]
        #[cxx_name = "setOrientationFilter"]
        fn set_orientation_filter(self: Pin<&mut AppearanceModel>, key: &QString);

        /// The catalogue sort.
        #[qinvokable]
        #[cxx_name = "setSort"]
        fn set_sort(self: Pin<&mut AppearanceModel>, key: &QString);

        /// The desktop the catalogue is filtered for.
        #[qinvokable]
        #[cxx_name = "setDe"]
        fn set_de(self: Pin<&mut AppearanceModel>, de_id: &QString);

        /// The search line text. An empty string resets the query AT ONCE — the
        /// original's fix lived in `_on_search_text_changed`.
        #[qinvokable]
        #[cxx_name = "setQuery"]
        fn set_query(self: Pin<&mut AppearanceModel>, text: &QString);

        /// Apply the typed query (Enter in the search line).
        #[qinvokable]
        #[cxx_name = "submitQuery"]
        fn submit_query(self: Pin<&mut AppearanceModel>);

        /// Re-read the catalogue anew.
        #[qinvokable]
        fn refresh(self: Pin<&mut AppearanceModel>);

        /// The "Running" chip of the "Conky widgets" section: only running widgets
        /// in the grid (the store under them is hidden).
        #[qinvokable]
        #[cxx_name = "setRunningOnly"]
        fn set_running_only(self: Pin<&mut AppearanceModel>, on: bool);

        /// The "Only for your GNOME version" switch of the extensions.gnome.org
        /// showcase: when off, all extensions are visible, and incompatible ones
        /// are marked.
        #[qinvokable]
        #[cxx_name = "setEgoCompatibleOnly"]
        fn set_ego_compatible_only(self: Pin<&mut AppearanceModel>, on: bool);

        /// After "Install from file…": re-read the installed items bypassing the
        /// cache.
        #[qinvokable]
        #[cxx_name = "showInstalledFresh"]
        fn show_installed_fresh(self: Pin<&mut AppearanceModel>);

        /// The countdown label of the "Undo" line: in how many seconds what is
        /// marked will be removed (`theme_removal`).
        #[qinvokable]
        #[cxx_name = "removalCountdownText"]
        fn removal_countdown_text(self: &AppearanceModel, seconds: i32) -> QString;

        /// The grid laid out into `cols` columns: a catalogue page is whole rows
        /// (`ap::grid_page_size`). If the size changed, the page is recomputed so
        /// that the first tile stays on screen, and the showcase is re-requested.
        #[qinvokable]
        #[cxx_name = "setGridColumns"]
        fn set_grid_columns(self: Pin<&mut AppearanceModel>, cols: i32);

        /// Paging.
        #[qinvokable]
        #[cxx_name = "prevPage"]
        fn prev_page(self: Pin<&mut AppearanceModel>);
        #[qinvokable]
        #[cxx_name = "nextPage"]
        fn next_page(self: Pin<&mut AppearanceModel>);

        /// What exactly to bring to the host: the request as a JSON string. An
        /// empty string means there is nothing to ask (the section is not for this
        /// desktop or a request is already in flight).
        #[qinvokable]
        #[cxx_name = "catalogRequest"]
        fn catalog_request(self: Pin<&mut AppearanceModel>) -> QString;

        /// The answer to the catalogue request (a JSON string). An answer with
        /// another generation is silently discarded — the user has already gone to
        /// another section.
        #[qinvokable]
        #[cxx_name = "catalogAnswer"]
        fn catalog_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// A ready tile preview: the item `id` and the path on disk.
        #[qinvokable]
        #[cxx_name = "setPreview"]
        fn set_preview(self: Pin<&mut AppearanceModel>, item_id: &QString, path: &QString);

        /// The picture of request `job` was written by the host into the cache at
        /// path `path`.
        #[qinvokable]
        #[cxx_name = "previewDone"]
        fn preview_done(self: Pin<&mut AppearanceModel>, job: &QString, path: &QString);

        /// Request `job` failed. `status` is the server's answer as a numeric string
        /// (empty if there was no answer at all; `canceled` — we aborted the request
        /// ourselves, such ones are not repeated). The decision "repeat or give up"
        /// is made by the core.
        #[qinvokable]
        #[cxx_name = "previewFailed"]
        fn preview_failed(self: Pin<&mut AppearanceModel>, job: &QString, status: &QString);

        /// The pause before a repeat is over — return the request to the queue if
        /// the section has not changed since.
        #[qinvokable]
        #[cxx_name = "previewRetry"]
        fn preview_retry(self: Pin<&mut AppearanceModel>, generation: &QString, job: &QString);

        /// Put ONE ready preview on a tile. One per pass of the event loop: decoding
        /// six JPEGs at once gives a noticeable jerk.
        #[qinvokable]
        #[cxx_name = "drainPreview"]
        fn drain_preview(self: Pin<&mut AppearanceModel>);

        /// A background warm-up of the first gallery frame of the shown tiles. If
        /// we left the section (the generation changed), the warm-up simply does not
        /// start.
        #[qinvokable]
        #[cxx_name = "warmGallery"]
        fn warm_gallery(self: Pin<&mut AppearanceModel>, generation: &QString);

        /// Set up the background warm-up queue: the first pages of all network
        /// sections from top to bottom (the core chooses the set).
        #[qinvokable]
        #[cxx_name = "startCategoryPrefetch"]
        fn start_category_prefetch(self: Pin<&mut AppearanceModel>);

        /// Start the next warm-up of the queue — one at a time.
        #[qinvokable]
        #[cxx_name = "pumpPrefetch"]
        fn pump_prefetch(self: Pin<&mut AppearanceModel>);

        /// The answer to a background warm-up (a JSON string): mark the target as
        /// done and pull the pictures of its cards with a low priority.
        #[qinvokable]
        #[cxx_name = "prefetchAnswer"]
        fn prefetch_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// The style storm settled — rebuild the grid. Bypassing the disk scan
        /// cache goes with the next request: the theme was just applied, and an old
        /// scan would show «✅ Active» on the previous one.
        #[qinvokable]
        #[cxx_name = "reloadAfterStyleStorm"]
        fn reload_after_style_storm(self: Pin<&mut AppearanceModel>);

        /// The tab is being closed: we no longer hand out new warm-ups.
        #[qinvokable]
        #[cxx_name = "closePage"]
        fn close_page(self: Pin<&mut AppearanceModel>);

        /// The generation of the shown catalogue — the page marks deferred work with
        /// it so as not to wake it up in another section.
        #[qinvokable]
        #[cxx_name = "catalogGeneration"]
        fn catalog_generation(self: &AppearanceModel) -> QString;

        /// The set of favourites (a list of identifiers) — comes from the host.
        #[qinvokable]
        #[cxx_name = "setFavorites"]
        fn set_favorites(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// The status line (messages of side actions).
        #[qinvokable]
        #[cxx_name = "setStatus"]
        fn set_status_text(self: Pin<&mut AppearanceModel>, text: &QString);

        /// Open the details panel of a grid item.
        #[qinvokable]
        #[cxx_name = "openDetail"]
        fn open_detail(self: Pin<&mut AppearanceModel>, item_id: &QString);

        /// Open the details panel of an item that is not in the grid: a link to a
        /// store item from third-party text (the `linkedCardReady` answer: `{item}`
        /// or `{aur}`).
        #[qinvokable]
        #[cxx_name = "openLinkedItem"]
        fn open_linked_item(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// A Conky widget card is open — take its picture anew (the snapshot was
        /// re-taken after a change of colours or settings).
        #[qinvokable]
        #[cxx_name = "refreshConkyDetailPicture"]
        fn refresh_conky_detail_picture(self: Pin<&mut AppearanceModel>);

        /// Go back to the grid.
        #[qinvokable]
        #[cxx_name = "closeDetail"]
        fn close_detail(self: Pin<&mut AppearanceModel>);

        /// Full item details (a JSON string): the description, all screenshots.
        #[qinvokable]
        #[cxx_name = "detailAnswer"]
        fn detail_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// The translated description following the card already shown (a JSON
        /// string: `itemId` and `translated`). The second pass of `loadDetail`: the
        /// card is on screen by this time, and the page only complements it and
        /// sets the "show original" link.
        #[qinvokable]
        #[cxx_name = "descriptionAnswer"]
        fn description_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Toggle "translation ⇄ original" under the description.
        #[qinvokable]
        #[cxx_name = "toggleDescription"]
        fn toggle_description(self: Pin<&mut AppearanceModel>);

        /// A long action on an item started: lock the start buttons, write the first
        /// line of the log and — if this kind of action has a progress share — show
        /// the progress bar. `kind` comes from the core: `install` | `aur` |
        /// `apply`.
        #[qinvokable]
        #[cxx_name = "actionBegin"]
        fn action_begin(self: Pin<&mut AppearanceModel>, item_id: &QString, kind: &QString);

        /// "Install from file…" started: the same as [`action_begin`], but the name
        /// is that of what was chosen (`paths_json` is an array of addresses of the
        /// choice window), and it has no tile.
        #[qinvokable]
        #[cxx_name = "installFileBegin"]
        fn install_file_begin(self: Pin<&mut AppearanceModel>, paths_json: &QString);

        /// The AUR installation queue changed (a JSON string
        /// `aur_install::AurQueue::snapshot_json`): the stages on the AUR tiles of
        /// all sections and the status line with the package name.
        #[qinvokable]
        #[cxx_name = "setAurJobs"]
        fn set_aur_jobs(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// The action progress moved: the progress share, 0–100.
        #[qinvokable]
        #[cxx_name = "setInstallPercent"]
        fn set_install_percent(self: Pin<&mut AppearanceModel>, pct: i32);

        /// The next line of the action log: what the installer is doing right now.
        /// Visible both on the details panel and in the status line.
        #[qinvokable]
        #[cxx_name = "installLog"]
        fn install_log(self: Pin<&mut AppearanceModel>, text: &QString);

        /// The outcome of installing/applying under the details buttons: `ok`
        /// decides which icon the line starts with (the core's decision). It also
        /// hides the progress bar and unlocks the buttons — the action is over.
        #[qinvokable]
        #[cxx_name = "setDetailStatus"]
        fn set_detail_status(self: Pin<&mut AppearanceModel>, text: &QString, ok: bool);

        /// The input in the search line changed: decide whether suggestions are
        /// needed and where to take them from (the core's decision —
        /// `suggest_action`).
        #[qinvokable]
        #[cxx_name = "updateSuggestions"]
        fn update_suggestions(self: Pin<&mut AppearanceModel>, text: &QString);

        /// The focus left the search line — remove the drop-down.
        #[qinvokable]
        #[cxx_name = "setSearchFocused"]
        fn set_search_focused(self: Pin<&mut AppearanceModel>, on: bool);

        /// The store answer for the suggestions (a JSON string).
        #[qinvokable]
        #[cxx_name = "suggestAnswer"]
        fn suggest_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Walking the drop-down with the keyboard: `up` or `down`.
        #[qinvokable]
        #[cxx_name = "suggestMove"]
        fn suggest_move(self: Pin<&mut AppearanceModel>, direction: &QString);

        /// Open the selected suggestion. `false` means there is nothing to select,
        /// and then Enter means an ordinary search.
        #[qinvokable]
        #[cxx_name = "suggestActivate"]
        fn suggest_activate(self: Pin<&mut AppearanceModel>) -> bool;

        /// Open the suggestion with this number (a mouse click).
        #[qinvokable]
        #[cxx_name = "suggestPick"]
        fn suggest_pick(self: Pin<&mut AppearanceModel>, index: &QString);

        /// The size and place of the drop-down under the search line:
        /// `{x, width, height}`. The numbers are computed by the core
        /// (`suggest_box`).
        #[qinvokable]
        #[cxx_name = "suggestGeometry"]
        fn suggest_geometry(
            self: &AppearanceModel,
            anchor_x: &QString,
            anchor_width: &QString,
            page_width: &QString,
        ) -> QMap_QString_QVariant;

        /// Paging the gallery.
        #[qinvokable]
        #[cxx_name = "galleryPrev"]
        fn gallery_prev(self: Pin<&mut AppearanceModel>);
        #[qinvokable]
        #[cxx_name = "galleryNext"]
        fn gallery_next(self: Pin<&mut AppearanceModel>);

        /// A ready gallery frame: the number as a string and the path on disk.
        #[qinvokable]
        #[cxx_name = "setGalleryImage"]
        fn set_gallery_image(self: Pin<&mut AppearanceModel>, index: &QString, path: &QString);

        /// The frame did not arrive.
        #[qinvokable]
        #[cxx_name = "setGalleryFailed"]
        fn set_gallery_failed(self: Pin<&mut AppearanceModel>, index: &QString);

        /// Walking the links is finished: how many frames there really are.
        #[qinvokable]
        #[cxx_name = "setGalleryFinished"]
        fn set_gallery_finished(self: Pin<&mut AppearanceModel>, count: &QString);

        /// Frames for the full-screen viewer: only those that really lie on disk.
        /// An empty list means there is nothing to show.
        #[qinvokable]
        #[cxx_name = "zoomFrames"]
        fn zoom_frames(self: &AppearanceModel) -> QList_QVariant;

        /// Read the comments thread further.
        #[qinvokable]
        #[cxx_name = "loadMoreComments"]
        fn load_more_comments(self: Pin<&mut AppearanceModel>);

        /// A page of comments (a JSON string).
        #[qinvokable]
        #[cxx_name = "commentsAnswer"]
        fn comments_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Comment translations following the already shown list (a JSON string:
        /// `itemId`, `page` and `rows` — "original → translation" pairs). The second
        /// pass of `loadComments`: the list is on screen by this time, and the page
        /// only fixes its rows and lights "show original" on them. The edit does not
        /// touch the comments that are not in `rows` (Russian, empty) at all.
        #[qinvokable]
        #[cxx_name = "commentTranslationsAnswer"]
        fn comment_translations_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// Toggle "translation ⇄ original" for one comment.
        #[qinvokable]
        #[cxx_name = "toggleComment"]
        fn toggle_comment(self: Pin<&mut AppearanceModel>, index: &QString);

        /// The AUR package passport for the open card (a JSON string
        /// `{itemId, info}` | `{itemId, missing}` | `{itemId, error}`).
        #[qinvokable]
        #[cxx_name = "aurDetailAnswer"]
        fn aur_detail_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// The outcome of the AUR package source check (a JSON string
        /// `{itemId, entry}` | `{itemId, failed}`): an `aur_trust::CacheEntry` cache
        /// record, the signals are assembled by the core.
        #[qinvokable]
        #[cxx_name = "aurTrustAnswer"]
        fn aur_trust_answer(self: Pin<&mut AppearanceModel>, payload: &QString);

        /// The install button in the AUR package card — exactly the one on its tile
        /// (with the installation queue): `{text, enabled, tooltip, state}`.
        #[qinvokable]
        #[cxx_name = "detailAurButton"]
        fn detail_aur_button(self: &AppearanceModel) -> QMap_QString_QVariant;

        /// The tile's page address: for a store item — pling, for a package —
        /// aur.archlinux.org. The package address is assembled by the core
        /// (`AurPackage`), there must not be a copy of the rules in the markup.
        #[qinvokable]
        #[cxx_name = "siteUrlOf"]
        fn site_url_of(self: &AppearanceModel, item_id: &QString) -> QString;

        /// The address of the item page in the store (empty — there is nothing to
        /// open).
        #[qinvokable]
        #[cxx_name = "detailSiteUrl"]
        fn detail_site_url(self: &AppearanceModel) -> QString;

        /// "Copy link": the address of the item page in the store goes to the
        /// clipboard, the confirmation to the status line.
        #[qinvokable]
        #[cxx_name = "copyDetailLink"]
        fn copy_detail_link(self: Pin<&mut AppearanceModel>);

        /// Is the picture at this address (`file://…` from the cache) animated?
        /// Then the markup shows it with `AnimatedImage`, otherwise with an
        /// ordinary `Image`.
        #[qinvokable]
        #[cxx_name = "isAnimatedPreview"]
        fn is_animated_preview(self: &AppearanceModel, url: &QString) -> bool;

        /// The number of the opened item — for the host, for installing and
        /// applying.
        #[qinvokable]
        #[cxx_name = "detailItemJson"]
        fn detail_item_json(self: &AppearanceModel) -> QString;

        /// The archive link of the opened item (listening before installation).
        #[qinvokable]
        #[cxx_name = "detailDownloadUrl"]
        fn detail_download_url(self: &AppearanceModel) -> QString;

        /// The tooltip of a single sound's button ("Listen to the
        /// "battery-caution" sound").
        #[qinvokable]
        #[cxx_name = "soundTooltip"]
        fn sound_tooltip(self: &AppearanceModel, event: &QString) -> QString;

        /// The download progress of a theme for listening — the label from the
        /// core. The numbers are `f64`: QML does not accept a 64-bit integer (the
        /// `qml_int64` guard in `src/lib.rs`).
        #[qinvokable]
        #[cxx_name = "soundStoreProgress"]
        fn sound_store_progress(self: &AppearanceModel, done: f64, total: f64) -> QString;

        /// Re-read the open section from disk bypassing the scan cache: the active
        /// theme changed not through a tile (the "Shell palette" strip). Only for
        /// disk sections and only on the grid — it does not touch the store or an
        /// open details panel.
        #[qinvokable]
        #[cxx_name = "reloadLocalFresh"]
        fn reload_local_fresh(self: Pin<&mut AppearanceModel>);

        /// "Configure…" of the style of the open details panel: launch its settings
        /// program (`themes::style_config_button_live`). Returns an error for the
        /// user (empty — launched).
        #[qinvokable]
        #[cxx_name = "configureDetailStyle"]
        fn configure_detail_style(self: Pin<&mut AppearanceModel>) -> QString;

        /// The "Apply to GTK 4/libadwaita programs" toggle: write it into the
        /// settings (`gtk4_theme::set_enabled_live`). Returns an error for the user
        /// (empty — written).
        #[qinvokable]
        #[cxx_name = "setGtk4Apply"]
        fn set_gtk4_apply(self: Pin<&mut AppearanceModel>, on: bool) -> QString;

        /// "Configure…" of the KDE window border of the open details panel: the
        /// settings window of its module (`kdecoration::config_button_live`). When
        /// the window is closed, the border snapshot is re-taken. Returns an error
        /// for the user (empty — launched).
        #[qinvokable]
        #[cxx_name = "configureDetailDecoration"]
        fn configure_detail_decoration(self: Pin<&mut AppearanceModel>) -> QString;

        /// "Edit…" (`mode` = `edit`) or "Create based on…" (`new`) for the colour
        /// scheme of the details panel. If there is a KDE editor, it is launched; if
        /// not, returns the state of our own window (JSON
        /// `color_scheme_edit::EditOpen`; `error` — it failed).
        #[qinvokable]
        #[cxx_name = "editDetailScheme"]
        fn edit_detail_scheme(self: Pin<&mut AppearanceModel>, mode: &QString) -> QString;

        /// Save the scheme of our own window: the name, the roles (a JSON array of
        /// `SchemeRole`), the base file and the name of our own scheme that may be
        /// overwritten. The answer is JSON `color_scheme_edit::SaveResult`; on
        /// success the grid re-reads the disk.
        #[qinvokable]
        #[cxx_name = "saveSchemeCopy"]
        fn save_scheme_copy(
            self: Pin<&mut AppearanceModel>,
            name: &QString,
            roles: &QString,
            base: &QString,
            overwrite: &QString,
        ) -> QString;

        /// Colours of the main details button: `{bg, fg, border}`.
        #[qinvokable]
        #[cxx_name = "detailButtonColors"]
        fn detail_button_colors(self: &AppearanceModel, state: &QString)
            -> QMap_QString_QVariant;

        /// Colours of a tile button by its state: `{bg, fg, border}`. An empty
        /// string means no colour at all (a transparent fill / no border). The
        /// decision is made by the core, here there is only substituting the skin
        /// token.
        #[qinvokable]
        #[cxx_name = "buttonColors"]
        fn button_colors(self: &AppearanceModel, state: &QString) -> QMap_QString_QVariant;

        /// Colours of a control overridden by the tab's style sheet:
        /// `{background, hover, border, hoverBorder, color, radius, paddingH,
        /// paddingV}` — a ready set for the `tone` of a button and a list. Kinds:
        /// `combo`, `nav`, `secondary` (see the core's `control_colors`).
        #[qinvokable]
        #[cxx_name = "controlColors"]
        fn control_colors(self: &AppearanceModel, kind: &QString)
            -> QMap_QString_QVariant;

        /// Role number by name (-1 if there is no such role) — checks without a
        /// delegate.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &AppearanceModel, name: &QString) -> i32;

        /// A row's role value by the item identifier — checks without a delegate.
        #[qinvokable]
        #[cxx_name = "roleValueOf"]
        fn role_value_of(self: &AppearanceModel, item_id: &QString, name: &QString) -> QVariant;

        /// Tour mode (`--tour`): the catalogue of the visible section is still
        /// loading.
        #[qinvokable]
        #[cxx_name = "tourBusy"]
        fn tour_busy(self: &AppearanceModel) -> bool;

        /// Tour mode: {total, noPreview, pending} — tiles, tiles without a picture
        /// and pictures in the load queue.
        #[qinvokable]
        #[cxx_name = "tourTiles"]
        fn tour_tiles(self: &AppearanceModel) -> QMap_QString_QVariant;
    }

    // The trait declaration must be IN THE BRIDGE, otherwise
    // `impl cxx_qt::Initialize` outside does not find the constructor.
    impl cxx_qt::Initialize for AppearanceModel {}
    // The skin change subscription goes through `CxxQtThread::queue`.
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
use rexon_core::gnome_ext_section as gext;
use rexon_core::wallhaven as wh;
use rexon_core::wallpaper_screens::WallpaperOrientation;
use rexon_core::models::{ThemeCategory, ThemeItem};
use rexon_core::ocs;
use rexon_core::ui_base;

// ============================================================================
// Roles of the tile model
// ============================================================================

/// Roles start at Qt::UserRole + 1 (Qt::UserRole == 256).
const ROLE_BASE: i32 = 256 + 1;

/// Role names in number order — the single source of truth for roleNames,
/// data() and roleNumber().
const ROLE_NAMES: [&str; 37] = [
    // Where the tile comes from: `ocs` (the store), `aur` (a package), `local`
    // (the disk).
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
    // What to put in place of the picture when there is none at all. For an
    // AUR package it is its icon: without a GitHub link there is no OpenGraph
    // card, and in this case the original showed exactly the icon, not
    // emptiness.
    "previewText",
    // The sound theme on disk shown by the tile (empty — not a sound theme):
    // by it the tile draws the "Listen:" row.
    "soundTheme",
    // A KDE splash on disk that can be shown (`ksplashqml --test`); empty
    // means there is no "Demonstrate" button on the tile.
    "splashDemo",
    // An AUR package installation from this session's queue: the line under
    // the buttons (the refusal reason), its tooltip (the full outcome) and the
    // label of the "Log" link (empty — no link). Empty for other tiles.
    "aurNote",
    "aurNoteTooltip",
    "aurLog",
    // Risk signals of an AUR package on the tile: `` | `warn` | `danger`
    // (`aur_trust`; only RPC data and what was already checked before).
    "riskLevel",
    // "Remove" on a theme-from-disk tile (`theme_removal`): whether the button
    // is visible, whether it can be pressed, the tooltip (for an unavailable
    // one — the reason). Store items and packages have no button.
    "removeVisible",
    "removeEnabled",
    "removeTooltip",
    // The section's current theme (`local_kind = "active"`): a frame and a bold
    // label, as in KDE — where the core tells to highlight (`markCurrent`).
    "current",
    // A GNOME extension known to the running shell: its uuid (empty — not
    // one; then the tile buttons are the shared ones), whether it has its own
    // settings and an error whose details can be asked about
    // (`gnome_ext_section`).
    "gnomeUuid",
    "gnomePrefs",
    "gnomeErrorInfo",
    // The tile picture is a font file (an AUR font package): the file
    // address, with which the tile draws the `previewSample` text sample.
    // Empty means not a font.
    "previewFont",
    "previewSample",
    // The label in place of the picture when there is none
    // (`aur::no_preview_note`): honestly what the item is and why there is no
    // screenshot.
    "previewNote",
    // The full-size picture of a theme on disk for the full-screen view
    // (`ThemeItem::preview_big`); empty — the viewer shows `preview`.
    "previewBig",
    // The AUR package mark in mixed results ("All"): a set, a system package,
    // a password (`ap::aur_mix_badge`), and its tooltip.
    "mixBadge",
    "mixBadgeTooltip",
];

const ROLE_PREVIEW: usize = 7;
const ROLE_PREVIEW_FONT: usize = 31;
const ROLE_BUTTON_TEXT: usize = 11;
const ROLE_BUTTON_STATE: usize = 12;
const ROLE_BUTTON_ENABLED: usize = 13;
const ROLE_FAVORITE_TEXT: usize = 15;
const ROLE_FAVORITE_STATE: usize = 16;

// ============================================================================
// Pure layer: a grid row
// ============================================================================

/// "Remove" of a theme-from-disk tile — as the host sent it (the row fields
/// `removeVisible`, `removeEnabled`, `removeTooltip`; the decision is
/// `theme_removal::Verdict`).
#[derive(Debug, Clone, Default)]
pub struct RemovalUi {
    pub visible: bool,
    pub enabled: bool,
    pub tooltip: String,
}

impl RemovalUi {
    /// From the host's answer row; `None` means there are no fields (a store
    /// item).
    fn from_row(raw: &Value) -> Option<Self> {
        let visible = raw.get("removeVisible")?.as_bool()?;
        Some(RemovalUi {
            visible,
            enabled: raw.get("removeEnabled").and_then(Value::as_bool).unwrap_or(false),
            tooltip: raw.get("removeTooltip").and_then(Value::as_str).unwrap_or("").to_string(),
        })
    }
}

/// Cinnamon Spices site information of a tile that has no place in
/// `ThemeItem`: "likes", the edit date, whether there is an update, the page
/// on the site and the "may freeze the shell" mark (for an installed one).
#[derive(Debug, Clone, Default)]
pub struct SpiceExtra {
    pub score: i64,
    pub edited: i64,
    pub update: bool,
    pub page: String,
    pub danger: bool,
    /// The card itself enables an installed theme ("Apply") — so in the GTK and
    /// window borders sections (`cinnamon_spices::PART_SECTIONS`).
    pub apply: bool,
}

/// What a third-party store with its own website (currently Wallhaven)
/// knows about a tile: the item page on the site and its own rating line
/// instead of the KDE Store's "★ rating ⬇ downloads". The answer row keys
/// are `site*`.
#[derive(Debug, Clone, Default)]
pub struct SiteExtra {
    pub page: String,
    pub stats: String,
    pub stats_tooltip: String,
}

impl SiteExtra {
    fn from_row(raw: &Value) -> Option<Self> {
        let page = raw.get("sitePage").and_then(Value::as_str).unwrap_or("");
        if page.is_empty() {
            return None;
        }
        let text = |k: &str| raw.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Some(SiteExtra { page: page.to_string(), stats: text("siteStats"), stats_tooltip: text("siteStatsTooltip") })
    }
}

/// A GNOME extension according to the shell — the `gnome*` fields of the
/// answer row (`gnome_ext_section::shell_fields`, `unseen_fields`): the
/// tile's marks and what can be done with it.
#[derive(Debug, Clone, Default)]
pub struct GnomeExtra {
    /// The shell knows it (otherwise it lies on disk and will work after
    /// re-login: only a mark, the buttons are the shared ones).
    pub shell_known: bool,
    pub uuid: String,
    pub enabled: bool,
    pub active: bool,
    pub can_change: bool,
    pub has_prefs: bool,
    pub can_remove: bool,
    pub error: bool,
    pub unsupported: bool,
    /// The tooltip of the marks: the error text or "Does not support GNOME N".
    pub tooltip: String,
    /// `open` (our own) | `copy` (a system one) | empty.
    pub folder: String,
    pub url: String,
    /// Labels of the marks in order.
    pub marks: Vec<String>,
}

impl GnomeExtra {
    /// From the host's answer row; `None` means the tile is not a GNOME
    /// extension.
    fn from_row(raw: &Value) -> Option<Self> {
        let uuid = raw.get("gnomeUuid").and_then(Value::as_str)?.to_string();
        let flag = |k: &str| raw.get(k).and_then(Value::as_bool).unwrap_or(false);
        let text = |k: &str| raw.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let marks = raw
            .get("gnomeMarks")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|m| m.get("text").and_then(Value::as_str)).map(str::to_string).collect())
            .unwrap_or_default();
        let error_text = text("gnomeErrorText");
        let tooltip = if flag("gnomeError") && !error_text.is_empty() { error_text } else { text("gnomeUnsupportedText") };
        Some(GnomeExtra {
            shell_known: flag("gnomeShellKnown"),
            uuid,
            enabled: flag("gnomeEnabled"),
            active: flag("gnomeActive"),
            can_change: flag("gnomeCanChange"),
            has_prefs: flag("gnomeHasPrefs"),
            can_remove: flag("gnomeCanRemove"),
            error: flag("gnomeError"),
            unsupported: flag("gnomeUnsupported"),
            tooltip,
            folder: text("gnomeFolderAction"),
            url: text("gnomeUrl"),
            marks,
        })
    }

    /// The tile's marks line: "Error · Update — after re-login".
    fn marks_line(&self) -> String {
        self.marks.join(" · ")
    }
}

impl SpiceExtra {
    /// From the host's answer row; `None` means the tile is not from Cinnamon
    /// Spices and without a mark.
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

/// One catalogue tile. It holds exactly what came with the answer; the
/// labels and buttons are computed by the core on the fly — these are cheap
/// string operations, and a second copy of the decisions in the state would
/// silently diverge from the first.
#[derive(Debug, Clone)]
pub enum Card {
    /// A store item or a local theme from disk (`local_kind` filled in).
    Theme(Box<ThemeItem>),
    /// An AUR package.
    Aur(Box<AurPackage>),
}

/// An AUR installation from the queue snapshot
/// (`aur_install::AurQueue::snapshot_json`).
#[derive(Debug, Clone)]
pub struct AurJobView {
    pub name: String,
    /// The whole composition (for a set — all its packages).
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

    /// The tile picture address.
    fn preview_url(&self) -> String {
        match self {
            Card::Theme(it) => it.preview_url.clone(),
            Card::Aur(p) => p.preview_url(),
        }
    }

    /// The tile pictures in turn: if one did not load, take the next one (the
    /// core's decision, `preview_candidates`).
    fn preview_candidates(&self) -> Vec<String> {
        match self {
            Card::Theme(it) => ap::preview_candidates(&it.preview_url, &it.preview_urls),
            Card::Aur(p) => vec![p.preview_url()],
        }
    }

    /// Addresses of the carousel frames — they are warmed up in the background
    /// after the tiles themselves.
    fn gallery_urls(&self) -> Vec<String> {
        match self {
            Card::Theme(it) => ap::gallery_urls(&it.preview_urls, &it.preview_url),
            // An AUR package has no carousel at all: one OpenGraph card.
            Card::Aur(_) => Vec::new(),
        }
    }
}

// ============================================================================
// Small conversions into Qt
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

/// A path on disk → an address for `Image.source`. An empty path stays
/// empty. Special characters are percent-encoded: the cache file names are
/// written by the store, and a hash in a name would cut the path off at
/// that place (the same technique in the "Steam" tab).
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

/// A `file://…` address → a path on disk (the inverse of [`file_url`]). Not
/// a file — `None`.
fn path_of_file_url(url: &str) -> Option<String> {
    let encoded = url.strip_prefix("file://")?;
    // The inverse of `file_url`: we expand %XX.
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

/// The calendar day of an AUR timestamp in the machine's time zone.
///
/// The zone is taken from Qt (`QDateTime`), not computed by hand:
/// converting a timestamp into a day depends on the time zone database, and
/// there must not be a copy of this database in the core (that is why the
/// core asks for a ready string).
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

/// A string field of a record.
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

// ============================================================================
// Rust state of the page
// ============================================================================

/// The host's answer saying that we aborted the request ourselves. Such a
/// "failure" is not repeated: it only means the user left the section.
const PREVIEW_CANCELED: &str = "canceled";

/// Kind of background warm-up: the first page of ANOTHER section.
const PREFETCH_CATEGORY: &str = "category";
/// Kind of background warm-up: the next page of the OPEN section.
const PREFETCH_PAGE: &str = "page";

/// One picture request in the form it went to the host.
#[derive(Debug, Clone)]
pub struct PreviewReq {
    /// Whose card it is.
    cid: String,
    /// The picture address — it is also the dedup key and the disk cache key.
    url: String,
    /// A background request: there is nothing to show, it only fills the cache.
    low: bool,
}

/// An order to a background thread for a local theme tile picture.
/// The key of a drawn local tile picture in `previews`: the tile number AND
/// the theme directory. By the number alone, a picture drawn for a tile in
/// another overview (the same `local:gtk:<name>`, but without its own
/// picture — for example, while the theme was active) later replaced the
/// real thumbnail of the theme.
fn drawn_key(id: &str, install_path: &str) -> String {
    format!("{id}\u{0}{install_path}")
}

/// A theme tile picture: the theme's own picture (the scan) matters more
/// than a drawn one; a drawn one — only for a theme without its own; a store
/// picture — by number.
/// The note of a «Global Themes» disk tile from the scan row (`stamp_note` of
/// the host); `None` — the row has none.
fn note_from_row(raw: &Value) -> Option<rexon_core::native_global::CardNote> {
    let line = raw.get("noteLine").and_then(Value::as_str).filter(|l| !l.is_empty())?;
    Some(rexon_core::native_global::CardNote {
        line: line.to_string(),
        tooltip: raw.get("noteTooltip").and_then(Value::as_str).unwrap_or("").to_string(),
        warn: raw.get("noteWarn").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// Tiles of the whole themes of the desktop («Global Themes» outside Plasma).
const NATIVE_TILE_PREFIX: &str = "local:nativeglobal:";

/// The full-screen copy of a drawn tile picture lies next to it under this suffix.
const BIG_PICTURE_SUFFIX: &str = ".big.png";

/// The full-screen copy of drawn picture `path`, when it is on disk.
fn big_picture_path(path: &str) -> Option<String> {
    let big = format!("{path}{BIG_PICTURE_SUFFIX}");
    std::path::Path::new(&big).is_file().then_some(big)
}

/// Give tile `id` its full-screen picture (the magnifier and the viewer take it).
fn set_card_big_picture(cards: &mut [Card], id: &str, big: &str) {
    for c in cards.iter_mut() {
        if let Card::Theme(it) = c {
            if it.id == id {
                it.preview_big = big.to_string();
            }
        }
    }
}

/// Draw the picture of a whole theme of the desktop: the tile into `dest`, the
/// same frame twice as large into `big`. The widgets are shot from its controls
/// theme by `helper` (no helper or display — the window in the theme colors).
/// `false` — the tile picture could not be written.
fn draw_native_picture(
    theme: Option<&rexon_core::native_global::NativeTheme>,
    home: &str,
    de: &str,
    helper: Option<&str>,
    gtk_dark: bool,
    glyph: &str,
    dest: &str,
    big: &str,
) -> bool {
    use rexon_core::local_preview as lp;
    use rexon_core::widget_shot as ws;
    let Some(theme) = theme else {
        let stub = lp::card_picture(&lp::SampleInput { kind: "", install_path: "", icon_themes: &[] }, glyph);
        return crate::local_preview_render::render_picture_to_png(&stub, dest);
    };
    let mut whole = rexon_core::native_global::whole_theme_live(theme, home);
    // The variant the programs will have after «Apply» (MATE: by the theme itself).
    let gtk_dark = rexon_core::native_global::picture_dark_live(de, theme, home, gtk_dark);
    let shot_png = format!("{dest}.shot.png");
    let work = std::path::Path::new(dest).parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    // The same GTK shot as a tile of the «GTK Themes» section, only larger.
    let kind = whole.gtk_dir.as_deref().and_then(|dir| ws::shot_kind(&format!("local:gtk:{}", theme.gtk), dir, true, gtk_dark));
    let shot = match (helper, kind) {
        (Some(helper), Some(kind)) => {
            let req = ws::ShotRequest { kind, palette: Vec::new() };
            ws::take_shot_sized(helper, &work, &req, &shot_png, lp::WHOLE_SHOT_SIZE)
        }
        _ => false,
    };
    if shot {
        whole.widgets_shot = Some(shot_png.clone());
    }
    let picture = lp::whole_theme_picture(&whole, glyph);
    let drawn = crate::local_preview_render::render_picture_to_png(&picture, dest);
    // The full-screen copy only for a real picture: a placeholder has nothing larger.
    if drawn && picture.source == lp::PreviewSource::Sample {
        let _ = crate::local_preview_render::render_scaled_to_png(&picture, big, lp::WHOLE_BIG_SCALE);
    } else {
        let _ = std::fs::remove_file(big);
    }
    let _ = std::fs::remove_file(&shot_png);
    rexon_core::log_info!(
        "appearance",
        "whole theme picture {}: {} (widgets shot: {shot})",
        theme.key,
        if picture.what.is_empty() { "placeholder" } else { picture.what }
    );
    drawn
}

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
    /// The tile number (`local:<kind>:<name>`).
    id: String,
    /// The theme applying kind.
    kind: String,
    /// The theme directory (for a colour scheme — its file).
    install_path: String,
    /// The kind sign for the placeholder.
    glyph: String,
    /// Where to put the ready frame.
    dest: String,
    /// A KDE window border: take it with its module (`kdecoration::take_shot`).
    shot: Option<rexon_core::kdecoration::ShotRequest>,
    /// A Qt style, a Kvantum or GTK 3 theme: take a form from real widgets
    /// (`widget_shot::take_shot`).
    widget_shot: Option<rexon_core::widget_shot::ShotRequest>,
}

pub struct AppearanceModelRust {
    /// Entry number in the shared skin change registry (`crate::skin`).
    skin_watch: u64,
    home: PathBuf,

    /// Sidebar sections — the order and composition are set by the core.
    sidebar: Vec<ThemeCategory>,
    /// The whole catalogue state: section, page, sort, source.
    view: ap::CatalogView,
    /// Accounting of requests in flight and of the warm-up — the core's finite
    /// state machine.
    requests: ap::CatalogRequests,

    /// Tiles of the current page.
    cards: Vec<Card>,
    /// Item identifier → path to the ready preview.
    previews: BTreeMap<String, String>,
    /// Local theme tiles whose picture is being drawn in the background right
    /// now (the tile number): re-entering the section does not order them a
    /// second time.
    drawing: std::collections::HashSet<String>,
    /// How many times the KDE border settings window was closed (the tile
    /// number): the number is part of the snapshot cache key, so that the tile
    /// shows a new frame rather than the same file from Qt's image cache.
    deco_config_gen: BTreeMap<String, u32>,
    /// The preview loader queue: how many pictures to pull at once, whom to let
    /// ahead, what to count as a duplicate and when to repeat — it decides.
    preview: ap::PreviewQueue,
    /// Request number → what exactly was asked for with it. The number lives
    /// until the answer and is returned back by the host: two tiles
    /// occasionally share one address, and by the address alone they could not
    /// be told apart.
    preview_jobs: BTreeMap<u64, PreviewReq>,
    /// The request number counter.
    preview_seq: u64,
    /// Identifiers of favourites.
    favorites: BTreeSet<String>,
    /// "Remove" for themes from disk: tile number → the decision sent by the
    /// host together with the scan (`theme_removal::Verdict`).
    removal: BTreeMap<String, RemovalUi>,
    /// Cinnamon Spices tiles: tile number → site information ([`SpiceExtra`]).
    spices: BTreeMap<String, SpiceExtra>,
    /// Tiles of third-party stores with their own site (Wallhaven) →
    /// [`SiteExtra`].
    site: BTreeMap<String, SiteExtra>,
    /// GNOME extension tiles: tile number → shell information
    /// ([`GnomeExtra`]).
    gnome: BTreeMap<String, GnomeExtra>,
    /// «Global Themes» outside Plasma: tile number → the note under its name
    /// (what a native theme sets, what of a Plasma package carries over).
    notes: BTreeMap<String, rexon_core::native_global::CardNote>,
    /// How many works the section has in total by the store's answer.
    total: i64,
    /// Whether there is anything after the page — for a topped-up store stream
    /// (`ap::stream_page`) the answer itself knows it; `None` means an answer
    /// without a stream (AUR, the disk): pages there are cut on the spot and
    /// counted by `total`.
    has_more: Option<bool>,
    /// Columns in the grid right now (0 — the grid has not been laid out yet).
    grid_cols: u32,

    /// Whether there is an AUR helper (yay/paru).
    aur_helper: bool,
    /// The desktop choice: [(id, name)].
    de_choices: Vec<(String, String)>,
    /// The desktop of the running session (empty — not recognized): by it a
    /// manual choice is labelled as manual (`ap::de_picker_choices`).
    detected_de: String,

    status: String,
    /// The outcome of the background re-conversion of cursors from Windows: it
    /// will replace the status line of the re-read section's answer (otherwise
    /// that would overwrite it).
    pending_notice: Option<String>,
    /// What is shown: `grid` | `detail` | `note` | `plasmalogin`.
    screen: String,
    /// The login screen is drawn by Plasma Login Manager (the background
    /// probe's answer; before the answer — no, and the panel stands as with
    /// SDDM).
    plasma_login: bool,
    /// There is something to open the Plasma Login Manager login screen
    /// settings with.
    plm_can_open: bool,

    /// AUR installations of this session (the queue lives in the action object
    /// and outlives the page; here is its last snapshot).
    aur_jobs: Vec<AurJobView>,
    /// The last line about the AUR installation progress that we put into the
    /// status line: other messages are not overridden by it a second time.
    aur_status: String,

    // ── Details panel ──
    /// The opened item (a local theme is distinguished by a filled
    /// `local_kind`).
    detail: Option<Box<ThemeItem>>,
    /// Show the original of the description instead of the translation.
    detail_show_original: bool,
    /// The description translation takes longer than the threshold — under the
    /// text «Translating the description…». Set and removed by the
    /// `descriptionReady` signal (the `pending` field), reset by opening another
    /// item.
    detail_translating: bool,
    /// The outcome of the last action under the buttons.
    detail_status: String,
    /// A long action on an item is in progress.
    action_busy: bool,
    /// The kind of the running action — whether there is a progress bar depends
    /// on it.
    action_kind: String,
    /// The number of the item the action is on: by the installation outcome its
    /// tile and the details panel get "✓ Installed"/"✓ Applied".
    action_item: String,
    /// The grid has to be rebuilt as soon as the Qt style change settles.
    reload_pending: bool,
    /// The next catalogue request must bypass the disk scan cache.
    catalog_fresh: bool,
    /// The next Cinnamon Spices request — bypassing the cache ("Refresh").
    site_force: bool,
    /// The progress share for the progress bar, 0–100.
    install_percent: i64,
    /// The path of the picture shown right now.
    detail_frame: String,
    /// The label instead of the picture when there is nothing to show.
    detail_frame_text: String,

    // ── Screenshot gallery ──
    /// Addresses of the carousel frames (the first slot is "the face of the
    /// item", it is not in the carousel: the core's decision, `gallery_urls`).
    gallery_src: Vec<String>,
    /// Frame number → path on disk.
    gallery_loaded: BTreeMap<i64, String>,
    /// Frames that will not arrive.
    gallery_failed: BTreeSet<i64>,
    /// The shown frame.
    gallery_index: i64,
    /// How many frames the item has by the store's answer.
    gallery_known: i64,
    /// Walking the links is finished.
    gallery_finished: bool,
    /// The user paged by hand — then a frame from the cache will not move them.
    gallery_user_nav: bool,

    // ── Comments ──
    comments: Vec<CommentRow>,
    /// The next comments page.
    comments_page: i64,
    /// How many comments the item has in total (`None` — the store did not
    /// say).
    comments_total: Option<i64>,

    // ── Search suggestions ──
    /// What is typed in the search line right now.
    suggest_query: String,
    /// Generation of the suggestions request: an answer to the previous input
    /// is discarded.
    suggest_generation: u64,
    /// The shown suggestions.
    suggest_items: Vec<ThemeItem>,
    /// The focus is in the search line.
    suggest_focused: bool,
    /// The selected row of the drop-down (-1 — none).
    suggest_index: i64,

    // ── AUR package card ──
    /// The open AUR package (`None` for a store item and a theme from disk).
    aur_detail: Option<Box<AurDetail>>,
}

/// The open AUR package card.
#[derive(Debug, Clone, Default)]
pub struct AurDetail {
    /// The tile it was opened from (before the RPC answer — everything that is
    /// known).
    pkg: AurPackage,
    /// The RPC `info` passport.
    info: Option<rexon_core::aur::AurInfo>,
    /// Co-maintainers from the package page.
    comaintainers: Vec<String>,
    /// Why there is no passport (network, the package was removed) — as a line
    /// for display.
    note: String,
    /// The package page was not given — the comments are only on the site.
    comments_blocked: bool,
    /// The site has a next page of comments.
    comments_more: bool,
    /// The source check: the `aur_trust` cache record (the deep check and the
    /// maintainer's snapshots).
    trust: Option<rexon_core::aur_trust::CacheEntry>,
    /// The check finished (with an answer or without).
    trust_done: bool,
}

impl Default for AppearanceModelRust {
    /// The initial state is assembled here, not in `initialize`, because
    /// `rowCount` is called from QML before any of our code. The default section
    /// is "Popular" (the first in the core's sidebar).
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
            site: BTreeMap::new(),
            gnome: BTreeMap::new(),
            notes: BTreeMap::new(),
            total: 0,
            has_more: None,
            grid_cols: 0,
            aur_helper: false,
            aur_jobs: Vec::new(),
            aur_status: String::new(),
            de_choices: Vec::new(),
            detected_de: String::new(),
            status: String::new(),
            pending_notice: None,
            screen: "grid".to_string(),
            plasma_login: false,
            plm_can_open: false,
            detail: None,
            detail_show_original: false,
            detail_translating: false,
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

/// Remove the skin change subscription: otherwise the registry would keep
/// the entry of a closed tab until the first broadcast.
impl Drop for AppearanceModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

impl AppearanceModelRust {
    /// Catalogue page size: whole grid rows (`ap::grid_page_size`); until the
    /// grid is laid out — the server's twenty.
    fn page_size(&self) -> u32 {
        if self.grid_cols == 0 {
            ap::PAGE_SIZE
        } else {
            ap::grid_page_size(self.grid_cols)
        }
    }

    /// Section by key.
    fn category(&self, key: &str) -> Option<ThemeCategory> {
        self.sidebar.iter().find(|c| c.key == key).cloned()
    }
}

impl qobject::AppearanceModel {
    // ── Fixed labels and numbers ────────────────────────────────────────
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
    fn grid_columns(&self, usable: i32) -> i32 {
        ap::grid_columns(usable)
    }
    fn grid_cell_width(&self, usable: i32) -> i32 {
        ap::grid_cell_width(usable)
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
    fn orientation_bar_label(&self) -> QString {
        QString::from(&ap::orientation_bar_label())
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
        for (label, key) in self.rust().view.sorts() {
            out.append(QVariant::from(&vmap(vec![
                ("label", qstr(&ap::sort_label(label))),
                ("key", qstr(key)),
            ])));
        }
        out
    }

    /// `sty` of a group section — the same metrics and colours as the
    /// "Settings" sections (`settings_page::group_style`).
    fn section_styles(&self) -> QMap<QMapPair_QString_QVariant> {
        vmap(vec![("group", QVariant::from(&crate::settings_page::group_style()))])
    }

    // ── Palette ─────────────────────────────────────────────────────────
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        // The colour of an irreversible action ("🗑 Remove") — one decision for
        // the product, it lives in the core of the "Extensions" tab. We do not
        // create our own copy of the red here (rule #12).
        out.insert(
            QString::from("danger"),
            qstr(rexon_core::qt_extensions::DANGER_COLOR),
        );
        // The warning colour — the same as in the "Settings" tab: the "MATE panel"
        // section marks the layout's missing applets with it.
        out.insert(QString::from("state_warn"), qstr(rexon_core::qt_settings::COLOR_WARN));
        out.insert(
            QString::from("danger_fill_alpha"),
            QVariant::from(&rexon_core::qt_extensions::DANGER_FILL_ALPHA),
        );
        out.insert(
            QString::from("danger_fill_hover_alpha"),
            QVariant::from(&rexon_core::qt_extensions::DANGER_FILL_HOVER_ALPHA),
        );
        // Background dimming under a modal window — one for the product.
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        out
    }

    // ── Sidebar ─────────────────────────────────────────────────────────
    fn sidebar_rows(&self) -> QList<QVariant> {
        let me = self.rust();
        let de = me.view.de.as_str();
        let mut out = QList::<QVariant>::default();
        // A group header goes with the first VISIBLE row of the group: sections
        // hidden in the environment do not take the header away, and an empty
        // group does not show it at all.
        let mut last_group: Option<categories::SidebarGroup> = None;
        // mate-panel layouts — only where the panel actually exists (a MATE
        // session and the program itself).
        let mate_panel = rexon_core::mate_panel::is_available(
            &rexon_core::animations::SystemIo,
            &rexon_core::mate_panel::PanelCtx::for_look(),
        );
        for cat in categories::grouped_sidebar(&me.sidebar, de) {
            // The settings sections act on the desktop CHOSEN in the header
            // (`animations::LookCtx::for_look`) — and are visible by it too, not by the
            // running one: previously they wrote into the session, and with another
            // desktop chosen they talked about someone else's (08.11.2026).
            let available = categories::category_available_in(&cat.key, de, me.view.session)
                && (cat.key != categories::MATE_PANEL_KEY || mate_panel);
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

    // ── Header ──────────────────────────────────────────────────────────
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
        // "ℹ️ …" — the same label form as in the original (`_sync_de_caveat`).
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
    fn ego_compatible_only(&self) -> bool {
        self.rust().view.ego_compatible_only
    }
    fn settings_panel(&self) -> QString {
        QString::from(&self.rust().view.settings_panel_key())
    }
    fn section_settings_visible(&self) -> bool {
        self.rust().view.has_section_settings()
    }
    fn section_settings_open(&self) -> bool {
        self.rust().view.section_settings_open()
    }
    fn section_settings_text(&self) -> QString {
        if self.rust().view.section_settings_open() {
            QString::from(&ap::detail_back_btn())
        } else {
            QString::from(&ap::section_settings_btn())
        }
    }
    fn section_settings_tooltip(&self) -> QString {
        let view = &self.rust().view;
        if view.section_settings_open() {
            return QString::default();
        }
        QString::from(&ap::section_settings_tooltip(&view.section_settings_panels()))
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
        // "VSThemes" exists only in "Cursors"; outside them the source that
        // actually applies is drawn as selected.
        let active = view.effective_source().to_string();
        let mut out = QList::<QVariant>::default();
        for chip in ap::source_chips_in(&view.category.key, view.axon) {
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

    /// The section's active tone filter (`CatalogView::active_tone_filter`).
    fn active_tone_filter(&self) -> &'static str {
        self.rust().view.active_tone_filter()
    }

    fn tone_bar_visible(&self) -> bool {
        self.rust().view.tone_bar()
    }

    fn orientation_bar_visible(&self) -> bool {
        self.rust().view.orientation_bar()
    }

    /// Orientation chips (`ap::orientation_chips`).
    fn orientation_chips(&self) -> QList<QVariant> {
        let active = self.rust().view.active_orientation().map_or("", WallpaperOrientation::key);
        let mut out = QList::<QVariant>::default();
        for chip in ap::orientation_chips() {
            out.append(QVariant::from(&vmap(vec![
                ("key", qstr(chip.key)),
                ("label", qstr(&chip.label)),
                ("tooltip", qstr(&chip.tooltip)),
                ("checked", QVariant::from(&(chip.key == active))),
            ])));
        }
        out
    }

    /// Whether the chosen filter narrows the grid down to the installed items
    /// (the tone, "Qt styles / Kvantum", "Running" — the rule is one,
    /// `CatalogView::installed_only`). Under it there is no store showcase at
    /// all: neither tiles (of the store and AUR), nor a request for it, nor its
    /// pager, sort and store choice.
    fn installed_only(&self) -> bool {
        self.rust().view.installed_only()
    }

    /// Tone filter chips (`lp::tone_chips`).
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

    /// Grid filter chips of "Application style" (`ap::style_filter_chips`).
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

    // ── Screen, state, pages ────────────────────────────────────────────
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

    fn status_warn(&self) -> bool {
        rexon_core::qt_settings::status_is_warning(&self.rust().status)
    }

    fn pagination(&self) -> ap::Pagination {
        let me = self.rust();
        match me.has_more {
            Some(more) => ap::stream_pagination(me.view.page, me.total, more, me.page_size()),
            None => ap::pagination(me.view.page, me.total, me.cards.len(), me.page_size()),
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
// Tile model
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

    /// The value of one tile role. Separate from `data` so that `roleValueOf`
    /// answers by the same path — otherwise the checks would look somewhere
    /// other than the delegate does.
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
        if let Some(v) = self.gnome_role(item, name) {
            return v;
        }
        if let Some(v) = self.note_role(item, local, name) {
            return v;
        }
        if !local {
            if let Some(x) = self.rust().site.get(&item.id) {
                match name {
                    "stats" => return qstr(&x.stats),
                    "statsTooltip" => return qstr(&x.stats_tooltip),
                    "starGlyph" => return qstr(""),
                    _ => {}
                }
            }
        }
        match name {
            "kind" => qstr(if local { "local" } else { "ocs" }),
            "itemId" => qstr(&item.id),
            "name" => qstr(ap::card_name(&item.name)),
            // In the "New"/"Popular" showcases — also the kind of thing.
            "author" => {
                let showcase = !local
                    && matches!(self.rust().view.category.store, rexon_core::models::CatalogSource::OcsShowcase);
                qstr(&ap::showcase_author(&self.rust().home, item, showcase))
            }
            // A rating and a download count exist only for a store item: for a theme
            // from disk "★ 0.0 ⬇ 0" is nonsense, and the line is not shown.
            // For a theme from disk the rating line is free: it holds the mark of a
            // global theme with its own widget layout (like the tile label in KDE).
            // For a Plasma style without its own colours — "Supports colour schemes".
            "stats" if local => qstr(
                &lp::follows_scheme_badge(&item.id, &item.install_path, self.active_tone_filter())
                    .unwrap_or_else(|| rexon_core::themes::lnf_tile_badge(&item.id, &item.install_path)),
            ),
            "stats" => qstr(&ap::card_stats_line(
                item.rated_stars(),
                item.downloads,
                &item.updated,
            )),
            "statsTooltip" => match ap::card_updated_tooltip(&item.updated) {
                Some(tip) => qstr(&tip),
                None => qstr(""),
            },
            // The rating icon is the only coloured spot of the stats line (#CardStar of
            // the style sheet); the colour is taken from the palette by the
            // `rating_star` key, here there is only the sign itself.
            "starGlyph" if local => qstr(""),
            "starGlyph" => qstr(ap::card_star_glyph(item.rated_stars())),
            "preview" => qstr(&file_url(&tile_picture(&self.rust().previews, item))),
            "previewBig" => qstr(&if local && std::path::Path::new(&item.preview_big).is_file() {
                file_url(&item.preview_big)
            } else {
                String::new()
            }),
            // A theme has no fallback sign: the original left the picture place empty,
            // and inventing something of our own here would be making things up.
            "previewText" => qstr(""),
            // A local theme has no source — the line is empty.
            "metaLine" | "metaTooltip" => qstr(""),
            "soundTheme" => qstr(&ap::local_sound_theme(&item.id).unwrap_or_default()),
            "splashDemo" => qstr(&ap::local_splash_demo(&item.id).unwrap_or_default()),
            "aurNote" | "aurNoteTooltip" | "aurLog" | "riskLevel" => qstr(""),
            "previewFont" | "previewSample" | "previewNote" | "mixBadge" | "mixBadgeTooltip" => qstr(""),
            "gnomeUuid" => qstr(""),
            "gnomePrefs" | "gnomeErrorInfo" => QVariant::from(&false),
            "metaWarn" => QVariant::from(&false),
            "buttonText" | "buttonState" | "buttonEnabled" => {
                let btn = if local {
                    ap::card_local_button_for(&local_kind, &item.id)
                } else {
                    let home = &self.rust().home;
                    let can_apply = ap::store_item_can_apply(home, item);
                    ap::card_store_button(item.installed, item.applied, can_apply, ap::store_item_switch(home, item))
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
                // "★ Favourites" exists for a local theme too: it can also be set aside
                // (as in the original).
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

    /// The note line of «Global Themes» outside Plasma: the one sent with a disk
    /// tile, and for a Plasma package from the store — what can carry over at
    /// most (its contents are not known before download). `None` — no note.
    fn note_role(&self, item: &ThemeItem, local: bool, name: &str) -> Option<QVariant> {
        if !matches!(name, "metaLine" | "metaTooltip" | "metaWarn") {
            return None;
        }
        let me = self.rust();
        let store_note;
        let note = match me.notes.get(&item.id) {
            Some(n) => n,
            None if !local
                && me.view.category.key == "global"
                && categories::section_id_for(item.category_id) == 722 =>
            {
                store_note = rexon_core::native_global::store_plasma_note(&me.view.de)?;
                &store_note
            }
            None => return None,
        };
        Some(match name {
            "metaLine" => qstr(&note.line),
            "metaTooltip" => qstr(&note.tooltip),
            _ => QVariant::from(&note.warn),
        })
    }

    /// Roles in which a Cinnamon Spices tile differs from a KDE Store item:
    /// "likes" and the edit date instead of the rating and downloads, "Update"
    /// on a new version on the site, the "may freeze the shell" warning (for an
    /// installed tile too). `None` — as for everyone.
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

    /// Roles of a GNOME extension tile (both from disk and from the showcase):
    /// the shell's marks instead of the author line, "Enable/Disable" by its
    /// truth, "🗑" only for our own, "⚙️" and "⚠️" — for those with settings
    /// and an error. An extension the shell does not know gets only the "after
    /// re-login" mark, the buttons are the shared ones. `None` — as for
    /// everyone.
    fn gnome_role(&self, item: &ThemeItem, name: &str) -> Option<QVariant> {
        let x = self.rust().gnome.get(&item.id)?;
        let known = x.shell_known;
        Some(match name {
            "metaLine" if !x.marks.is_empty() => qstr(&x.marks_line()),
            "metaTooltip" if !x.marks.is_empty() => qstr(&x.tooltip),
            "metaWarn" if !x.marks.is_empty() => QVariant::from(&(x.error || x.unsupported)),
            "buttonText" | "buttonState" | "buttonEnabled" if known => {
                let btn = gext::toggle_button(x.enabled, x.active, x.can_change);
                match name {
                    "buttonText" => qstr(&btn.text),
                    "buttonState" => qstr(btn.state),
                    _ => QVariant::from(&btn.enabled),
                }
            }
            "buttonTooltip" if known => qstr(&if x.can_change { String::new() } else { gext::locked_tooltip() }),
            "removeVisible" if known => QVariant::from(&x.can_remove),
            "removeEnabled" if known => QVariant::from(&true),
            "removeTooltip" if known => qstr(""),
            "gnomeUuid" if known => qstr(&x.uuid),
            "gnomePrefs" => QVariant::from(&(known && x.has_prefs)),
            "gnomeErrorInfo" => QVariant::from(&(known && x.error)),
            _ => return None,
        })
    }

    /// The tile's page on its store's site — Cinnamon Spices or Wallhaven
    /// (empty — the tile is not from there, the KDE Store has its own address).
    fn spice_page(&self, item_id: &str) -> Option<String> {
        let me = self.rust();
        me.spices
            .get(item_id)
            .map(|x| x.page.clone())
            .or_else(|| me.site.get(item_id).map(|x| x.page.clone()))
            // A Wallhaven card from favourites arrives without an answer row — the
            // address is assembled from the id itself.
            .or_else(|| wh::parse_item_id(item_id).map(wh::page_url))
            .filter(|p| !p.is_empty())
    }

    /// What to show on the tile of package `name` by the AUR installation
    /// queue.
    ///
    /// Its own installation — by its stage; a package installed as part of a
    /// running set — by the set's stage (while it runs). An installation that
    /// succeeded before this session does not touch the queue: the tile is as
    /// before.
    fn aur_job_state(&self, name: &str) -> Option<aur_install::CardJobState> {
        let jobs = &self.rust().aur_jobs;
        let job = jobs
            .iter()
            .find(|j| j.name == name)
            .or_else(|| jobs.iter().find(|j| j.stage.active() && j.packages.iter().any(|p| p == name)))?;
        Some(aur_install::card_job_state(job.stage, &job.short, &job.message))
    }

    fn aur_role(&self, pkg: &AurPackage, name: &str) -> QVariant {
        // An installation from this session's queue overrides the ordinary button:
        // "Queued", "Building…", "Retry" (the core's decision).
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
            // For an AUR package the rating is votes, and it has its own sign.
            "starGlyph" => qstr("👍"),
            // The tile picture; a font file is not shown as a picture — a text sample
            // is drawn with it (`previewFont`).
            "preview" | "previewFont" => {
                let path = self
                    .rust()
                    .previews
                    .get(&pkg.name)
                    .cloned()
                    .unwrap_or_default();
                let font = ap::is_font_file(&path);
                let wanted = if name == "preview" { !font } else { font };
                qstr(&if wanted { file_url(&path) } else { String::new() })
            }
            "previewSample" => qstr(&if pkg.is_font() { rexon_core::aur::font_sample_text() } else { String::new() }),
            "previewNote" => qstr(&rexon_core::aur::no_preview_note(pkg)),
            "previewBig" => qstr(""),
            "mixBadge" => qstr(&ap::aur_mix_badge(&self.rust().view, &pkg.category)),
            "mixBadgeTooltip" => qstr(&ap::aur_mix_badge_tooltip()),
            "previewText" => qstr(pkg.icon()),
            // Risk signals: an icon before the source line, the list in the tooltip.
            // The line already says "Outdated" in words — we do not duplicate it with
            // an icon.
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
            // An AUR package is not set aside into favourites: favourites are store
            // items and themes from disk.
            "favoriteText" | "favoriteState" => qstr(""),
            "soundTheme" | "splashDemo" => qstr(""),
            "removeVisible" | "removeEnabled" | "current" => QVariant::from(&false),
            "removeTooltip" | "gnomeUuid" => qstr(""),
            "gnomePrefs" | "gnomeErrorInfo" => QVariant::from(&false),
            _ => QVariant::default(),
        }
    }

    fn button_colors(&self, state: &QString) -> QMap<QMapPair_QString_QVariant> {
        let (bg, fg, border) = ap::card_button_colors(&state.to_string());
        let skin = ui_base::active_skin_colors();
        // A value with a hash is a ready colour; without a hash — the name of a
        // token of the active skin (the `card_button_colors` contract).
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
        // The table values are NAMES of tokens of the active skin: neither the
        // core from here nor the markup creates colours of their own.
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

    fn tour_busy(&self) -> bool {
        self.rust().requests.busy()
    }

    /// A tile "without a preview" is one whose `preview` and `previewFont`
    /// roles are empty: exactly that one the delegate draws without a
    /// screenshot (a sign and an honest label). A font sample (`previewFont`)
    /// is a preview.
    fn tour_tiles(&self) -> QMap<QMapPair_QString_QVariant> {
        let me = self.rust();
        let empty = |c: &Card, role: &str| self.role_of(c, Some(role)).value::<QString>().is_none_or(|v| v.is_empty());
        let no_preview = me.cards.iter().filter(|c| empty(c, "preview") && empty(c, "previewFont")).count();
        let pending = me.preview.inflight() + me.preview.queued() + usize::from(me.preview.has_ready());
        vmap(vec![
            ("total", QVariant::from(&(me.cards.len() as i32))),
            ("noPreview", QVariant::from(&(no_preview as i32))),
            ("pending", QVariant::from(&(pending as i32))),
        ])
    }
}

// ============================================================================
// Page actions
// ============================================================================

impl qobject::AppearanceModel {
    /// Rebuild everything that depends on the catalogue state, and call for
    /// data. One entry — so that no switch forgets half of the page (this was
    /// the original's `_update_chrome` + `_reload`).
    fn resync(mut self: Pin<&mut Self>) {
        // The session kind of the chosen desktop — for sections possible only on
        // X11 or only on Wayland: the session desktop as is, a manually chosen one
        // — its kind (`LookCtx::for_chosen`).
        {
            let de = self.rust().view.de.clone();
            let wayland = rexon_core::animations::LookCtx::from_env().for_chosen(&de).wayland;
            self.as_mut().rust_mut().view.session = categories::Session::from_wayland(wayland);
        }
        // Whether Plasma, LXQt or Deepin is installed — for the Qt style caveat
        // on sway/river/Wayfire: the variables file is elsewhere then. Only in
        // the sections that carry this caveat.
        if matches!(self.rust().view.category.key.as_str(), "kvantum" | "widgetstyle" | "colors") {
            let path = std::env::var("PATH").ok();
            let installed = rexon_core::themes::own_qt_desktop_installed(&|b| {
                rexon_core::utils::which(b, path.as_deref()).is_some()
            });
            self.as_mut().rust_mut().view.qt_desktop_installed = installed;
        }
        // Whether Razer Axon is found — for its chip and the settings block in the
        // wallpaper section: Axon may have been installed (or its folder
        // specified) with the window open. The probe walks the known Wine
        // prefixes, only in the section with the source.
        if rexon_core::axon_window::has_section(&self.rust().view.category.key) {
            let axon = rexon_core::axon_window::show_source_live();
            self.as_mut().rust_mut().view.axon = axon;
        }
        // Whether the Conky program is present — for its section's label: after
        // installing a widget (and the program with it) the label must change.
        if self.rust().view.category.key == "conky" {
            let installed = rexon_core::de_apply::have_conky();
            let installable = rexon_core::de_apply::live_conky_package().is_some();
            let mut me = self.as_mut().rust_mut();
            me.view.conky_installed = installed;
            me.view.conky_installable = installable;
            // The session through conky's eyes: through XWayland the caveat about
            // Wayland is not needed.
            me.view.wayland = rexon_core::conky_overlay::conky_ctx(
                &rexon_core::animations::SystemIo,
                &rexon_core::de_apply::Ctx::live(),
            )
            .wayland;
        }
        // An explanation ("this section is not in this desktop", "closed on
        // SteamOS") and the Plasma Login Manager panel have no showcase: we do not
        // go to the store.
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
        if self.rust().view.category.key == "cursor" {
            self.as_mut().refresh_windows_cursors_once();
        }
    }

    /// The "Cursors" section is open: in the background re-convert the
    /// installed themes "from Windows" affected by the converter fix
    /// (`ext_exec::refresh_windows_cursors_live`; Wehrwolfmann's decision of
    /// 14.11.2026). Once per window life: the converted ones get the current
    /// version, and a repeated pass would find nothing.
    fn refresh_windows_cursors_once(self: Pin<&mut Self>) {
        use std::sync::atomic::{AtomicBool, Ordering};
        static STARTED: AtomicBool = AtomicBool::new(false);
        if STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        let handle = self.as_ref().get_ref().qt_thread();
        let spawned = std::thread::Builder::new().name("rexon-wincur-again".into()).spawn(move || {
            let lines = rexon_core::ext_exec::refresh_windows_cursors_live();
            if lines.is_empty() {
                return;
            }
            let text = lines.join("\n");
            let _ = handle.queue(move |object| object.windows_cursors_refreshed(&text));
        });
        if spawned.is_err() {
            rexon_core::log_warn!("cursors", "поток перевода заново курсоров не завёлся");
        }
    }

    /// The outcome of the re-conversion: re-read the section with installed
    /// items from disk, show the outcome line over the answer.
    fn windows_cursors_refreshed(mut self: Pin<&mut Self>, text: &str) {
        let reload = {
            let me = self.rust();
            (me.view.is_local() || me.view.installed_view()) && me.screen == ap::SCREEN_GRID
        };
        if reload {
            self.as_mut().rust_mut().pending_notice = Some(text.to_string());
            self.as_mut().reload_local_fresh();
        } else {
            self.as_mut().rust_mut().status = text.to_string();
            self.as_mut().status_changed();
        }
    }

    /// Empty the grid.
    fn clear_cards(mut self: Pin<&mut Self>) {
        unsafe {
            self.as_mut().begin_reset_model();
            self.as_mut().rust_mut().cards.clear();
            self.as_mut().end_reset_model();
        }
        self.as_mut().count_changed();
    }

    /// Start a new catalogue request and tell the page it is time to go to the
    /// host. The key and the generation are computed by the core.
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
        // The user no longer needs the previous section's pictures, while the new
        // one needs the channel: the queue changes the generation, and the host
        // aborts what is in flight.
        self.as_mut().cancel_previews();
        self.as_mut().catalog_wanted();
    }

    fn set_environment(mut self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        {
            let mut me = self.as_mut().rust_mut();
            // The page no longer keeps the Qt styles and the active style: the
            // "Application style" grid is assembled by the host's scan (`scan_disk`),
            // and the scan itself marks the current style.
            me.aur_helper = v
                .get("aurHelper")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        }
        // The desktop: the actually running one wins over the remembered choice —
        // the decision is made by the core (`initial_de`).
        let choice = ap::initial_de(s(&v, "detectedDe"), s(&v, "savedDe"), s(&v, "seenDe"));
        {
            let mut me = self.as_mut().rust_mut();
            me.view.de = choice.de_id.clone();
            rexon_core::animations::set_chosen_desktop(&choice.de_id);
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
        // Whether the open section is affected: it changed its kind (SDDM ↔ Plasma
        // Login Manager) or closed/opened on SteamOS. An unaffected section is not
        // re-read — the showcase would stay the same, and the request would go out
        // for nothing.
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
            // We do not pull the open item detail out from under the user: the section
            // will be rebuilt when they leave it for another one.
            let in_detail = me.screen == "detail";
            if !in_detail && !me.sidebar.iter().any(|c| c.key == key) {
                // The login screen section of the previous kind was open — we move into
                // the new one at the same place of the panel.
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
            // The section changed its kind — the tiles of the previous kind go away at
            // once, as when choosing a section (`select_category`).
            self.as_mut().clear_cards();
            self.as_mut().resync();
        } else {
            self.as_mut().sidebar_changed();
            self.as_mut().screen_changed();
            // The note "it is now shown by …" for the open login section.
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
            // "Whether there is a next page" is the answer of the PREVIOUS section.
            // Without a reset the new section's pager, until its catalogue arrived,
            // counted pages by someone else's answer: "Page 1 / 2" over an empty grid
            // (live check of 15.11.2026, "GTK themes" after "HD Wallpapers").
            me.has_more = None;
            // The query belongs to the section that was left: carrying it further
            // means opening the neighbouring section already filtered by a forgotten
            // word. The original on widgets clears both the field and the query
            // (`_select_category`); the markup pulls in the search line by
            // `chromeChanged`.
            me.view.query.clear();
            // The "kind" sub-choice belongs to the "Application style" section:
            // leaving it, we forget it so that it does not drag into another section.
            me.view.style_sub = None;
            // The "Running" chip belongs to the Conky section.
            me.view.running_only = false;
            // The tone filter belongs to its section (Plasma does not remember it
            // either).
            me.view.tone_filter.clear();
            // The «⚙️ Settings» view belongs to the section ("Fonts", "Icons",
            // "Cursors"): coming back, the user sees the catalogue, as on the first
            // visit.
            me.view.section_settings = false;
            me.requests.forget_page_prefetches();
        }
        // The open theme card belongs to the section that was left: under the new
        // section's header it stayed on screen (acceptance of 10.10.2026: the
        // header "Plugin wallpapers", inside — a login screen theme).
        if self.rust().screen == "detail" {
            self.as_mut().close_detail();
        }
        // The tiles also belong to the section that was left: while the new one's
        // catalogue is on its way ("⏳ Loading the catalogue…"), the icons and
        // cursors of "Active" stood under its header (acceptance of 24.10.2026,
        // "GTK themes"). We empty the grid at once; the new section will fill it
        // instantly from the cache.
        self.as_mut().clear_cards();
        self.as_mut().resync();
        // A section was opened — we immediately warm up the next one: on switching
        // it will open instantly from the cache (the original's progressiveness).
        self.prioritize_prefetch();
    }

    fn toggle_section_settings(mut self: Pin<&mut Self>) {
        if !self.rust().view.has_section_settings() {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            me.view.section_settings = !me.view.section_settings;
            // The search belongs to the catalogue: the settings have no search line,
            // and a forgotten word must not greet the user on coming back.
            me.view.query.clear();
        }
        if self.rust().screen == "detail" {
            self.as_mut().close_detail();
        }
        // The tiles go away at once (the settings screen), and come back from the
        // cache.
        self.as_mut().clear_cards();
        self.as_mut().resync();
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
            // The installed items come only with the first store page: under a filter
            // from the second page on the grid would stay empty.
            me.view.page = 0;
        }
        self.resync();
    }

    fn set_orientation_filter(mut self: Pin<&mut Self>, key: &QString) {
        let key = key.to_string();
        {
            let mut me = self.as_mut().rust_mut();
            if me.view.orientation_filter == key {
                return;
            }
            me.view.orientation_filter = key;
            // Another selection on the server: page 1 of it.
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
            // "Detect automatically" is the session desktop; the manual choice label
            // is removed even when the catalogue desktop is the same.
            let (de_id, _) = ap::resolve_de_pick(&picked, &me.detected_de);
            let choices = ap::de_picker_choices(&de_id, &me.detected_de);
            if me.view.de == de_id && me.de_choices == choices {
                return;
            }
            me.view.de = de_id.clone();
            rexon_core::animations::set_chosen_desktop(&de_id);
            me.de_choices = choices;
            me.view.page = 0;
        }
        self.as_mut().de_changed();
        self.resync();
    }

    fn set_query(mut self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        // What to do with what was typed is decided by the core
        // (`CatalogView::edit_query`): the search is applied on Enter, while
        // CLEARING the field resets the query at once — otherwise the query stuck
        // on the previous word with a visually empty field, and the catalogue
        // stayed empty forever.
        let outcome = self.as_mut().rust_mut().view.edit_query(&text);
        match outcome {
            // What was typed is recorded, we wait for Enter. We do NOT touch the
            // header: the `query` property holds the text of the field itself, and
            // overwriting it on the go would make the caret jump.
            ap::QueryEdit::Idle => {}
            // Disk overviews search over the already read list — without the network
            // and instantly, so they filter right while typing, like the KDE pages. We
            // do not poke the store per character: the page will apply the word with
            // `submitQuery` after the core's pause (`searchDelayMs`) or on Enter.
            //
            // The header is notified then too: the `query` property must follow the
            // core, otherwise after a section change (where the query is reset) the
            // field did not notice the reset — for it the query was empty before as
            // well — and the forgotten word stayed in the field (19.10.2026, "MishMty").
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
            // "Refresh" for Cinnamon Spices and Wallhaven — ask the site again, not
            // the cache.
            me.site_force = me.view.own_site_store();
            // "Refresh" for the installed grid — read the disk and the installed
            // journal again rather than giving the previous scan from the cache:
            // otherwise a theme put bypassing Rexon appeared only after a restart.
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
            // Under "Running" there is no store (`installed_only`), and the installed
            // items come with the first page.
            me.view.page = 0;
        }
        self.refresh();
    }

    fn set_ego_compatible_only(mut self: Pin<&mut Self>, on: bool) {
        if self.rust().view.ego_compatible_only == on {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            me.view.ego_compatible_only = on;
            // A different filter means different results: page them from the start.
            me.view.page = 0;
        }
        self.as_mut().chrome_changed();
        self.resync();
    }

    fn set_grid_columns(mut self: Pin<&mut Self>, cols: i32) {
        let cols = cols.max(1) as u32;
        let old = self.rust().page_size();
        if self.rust().grid_cols == cols {
            return;
        }
        self.as_mut().rust_mut().grid_cols = cols;
        let new = self.rust().page_size();
        if new == old {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            me.view.page = ap::page_after_resize(me.view.page, old, new);
        }
        // Only the store showcase is paged; local sections do not need the page
        // size, and a re-request there would be a needless disk scan.
        if self.pagination_visible() {
            self.resync();
        }
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
        // A request that did NOT go to the network goes into the log: otherwise
        // "⏳ Loading the catalogue…" forever with a silent network (15.11.2026)
        // is indistinguishable from a request that was never asked for.
        match &outcome {
            ap::FireOutcome::Deduplicated { key } => {
                rexon_core::log_info!("appearance", "каталог {key}: уже летит, второй запрос не шлю");
            }
            ap::FireOutcome::Stale => {
                rexon_core::log_info!("appearance", "каталог: запрос устарел до отправки");
            }
            _ => {}
        }
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
            "pageSize": me.page_size(),
            "sort": me.view.active_sort(),
            "query": me.view.query,
            "orientation": me.view.active_orientation().map_or("", WallpaperOrientation::key),
            "which": me.view.local_scan_which(),
            "aurKey": me.view.aur_cache_key(),
            "lockMode": me.view.lock_mode_effective(),
            "egoCompatibleOnly": me.view.ego_compatible_only,
            "de": me.view.de,
            // Bypass the disk scan cache: a theme was just applied.
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
        // An answer of a section the user has already left is silently discarded
        // — the key and the generation are compared by the core. Warning: `answer`
        // answers "IS IT STALE", not "should it be accepted": with the negation not
        // a single fresh answer got through here, and the showcase stayed empty
        // with a live host.
        if self.as_mut().rust_mut().requests.answer(&key) {
            rexon_core::log_info!("appearance", "каталог {key}: ответ устарел, отброшен");
            return;
        }
        // A section without a showcase (an explanation, the login screen and MATE
        // panels, settings sections) has no grid: a late answer of the previous
        // section (from the cache it comes without a sender and counts as fresh)
        // wrote "Window title buttons: 0" over the section.
        if !matches!(self.rust().screen.as_str(), ap::SCREEN_GRID | "detail") {
            return;
        }
        let error = s(&v, "error").to_string();
        let mut cards: Vec<Card> = Vec::new();
        if let Some(list) = v.get("items").and_then(Value::as_array) {
            for raw in list {
                if let Ok(item) = serde_json::from_value::<ThemeItem>(raw.clone()) {
                    let mut item = item;
                    // Local themes arrive with their own kind (`localKind`): they have a
                    // different button and a different badge.
                    if let Some(kind) = raw.get("localKind").and_then(Value::as_str) {
                        if !kind.is_empty() {
                            item.local_kind = Some(kind.to_string());
                        }
                    }
                    // The name without translation — for searching by the original name.
                    if let Some(src) = raw.get("sourceName").and_then(Value::as_str) {
                        item.source_name = src.to_string();
                    }
                    if let Some(big) = raw.get("previewBig").and_then(Value::as_str) {
                        item.preview_big = big.to_string();
                    }
                    if let Some(ui) = RemovalUi::from_row(raw) {
                        self.as_mut().rust_mut().removal.insert(item.id.clone(), ui);
                    }
                    match SiteExtra::from_row(raw) {
                        Some(x) => {
                            self.as_mut().rust_mut().site.insert(item.id.clone(), x);
                        }
                        None => {
                            self.as_mut().rust_mut().site.remove(&item.id);
                        }
                    }
                    match SpiceExtra::from_row(raw) {
                        Some(x) => {
                            self.as_mut().rust_mut().spices.insert(item.id.clone(), x);
                        }
                        None => {
                            self.as_mut().rust_mut().spices.remove(&item.id);
                        }
                    }
                    match GnomeExtra::from_row(raw) {
                        Some(x) => {
                            self.as_mut().rust_mut().gnome.insert(item.id.clone(), x);
                        }
                        None => {
                            self.as_mut().rust_mut().gnome.remove(&item.id);
                        }
                    }
                    match note_from_row(raw) {
                        Some(n) => {
                            self.as_mut().rust_mut().notes.insert(item.id.clone(), n);
                        }
                        None => {
                            self.as_mut().rust_mut().notes.remove(&item.id);
                        }
                    }
                    cards.push(Card::Theme(Box::new(item)));
                }
            }
        }
        // The open GNOME extension was enabled, disabled or crashed: the details
        // buttons follow the shell's fresh word.
        let gnome_detail = {
            let me = self.rust();
            me.detail.as_ref().is_some_and(|d| me.gnome.contains_key(&d.id))
        };
        if gnome_detail {
            self.as_mut().detail_changed();
        }
        if let Some(list) = v.get("packages").and_then(Value::as_array) {
            for raw in list {
                if let Ok(pkg) = serde_json::from_value::<AurPackage>(raw.clone()) {
                    cards.push(Card::Aur(Box::new(pkg)));
                }
            }
        }
        // The active query is mandatory FOR ALL themes from disk, wherever they
        // came into the grid from: both for a whole overview section and for the
        // installed themes glued on top of the store in "Splash screen" and "Lock
        // screen". Previously the search noticed only the store showcase, and next
        // to one found theme stood all the installed ones.
        // We do not touch the store cards with this yardstick: there the selection
        // is done on the store's side and by its own fields (description, tags).
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
            // The tone filter of colour schemes and Plasma styles (the decision is
            // `lp::tone_passes`). We know the tone only for themes from disk: the store
            // showcase under the grid goes away with a filter chosen, otherwise light
            // store schemes would stand under "Dark".
            if self.tone_bar_visible() {
                let filter = self.active_tone_filter();
                if filter != lp::TONE_ALL {
                    cards.retain(|c| match c {
                        Card::Theme(it) => lp::tone_passes(&it.id, &it.install_path, filter),
                        _ => false,
                    });
                }
            }
            // "Running": Conky widgets with a process right now (they have the
            // "active" tile kind), without the store.
            if self.rust().view.running_only {
                cards.retain(|c| matches!(c, Card::Theme(it) if it.local_kind.as_deref() == Some("active")));
            }
            // Any "installed only" filter (`installed_only`): there are no store tiles
            // in the grid, even if the answer brought them.
            if self.installed_only() {
                cards.retain(|c| matches!(c, Card::Theme(it) if it.local_kind.is_some()));
            }
        }
        let total = v.get("total").and_then(Value::as_i64).unwrap_or(0);
        let has_more = v.get("hasMore").and_then(Value::as_bool);
        {
            let mut me = self.as_mut().rust_mut();
            me.has_more = has_more;
            // Lock screen cards arrive from other catalogues — the core rewrites their
            // category so that the installer touches exactly the lock component, not
            // the whole theme.
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
        // The state label depends on the section kind — the decision is the
        // core's.
        let status = {
            let me = self.rust();
            let query = me.view.query.trim();
            if !error.is_empty() {
                error
            } else if !query.is_empty() {
                // A search is active — say so: an ordinary "Found" over one tile looked
                // like a missing store.
                let installed = me
                    .cards
                    .iter()
                    .filter(|c| matches!(c, Card::Theme(it) if it.local_kind.is_some()))
                    .count() as i64;
                // Under the "installed only" filter there is no store — exactly what is
                // shown was found.
                let found = if me.view.is_local() || me.view.installed_only() {
                    shown as i64
                } else if me.view.installed_view() {
                    installed + total.max(shown as i64 - installed)
                } else {
                    total.max(shown as i64)
                };
                ap::search_status(query, found)
            } else if shown == 0 && me.view.is_local() && !me.view.query.trim().is_empty() {
                // The search over the installed ones found nothing — say so in words, not
                // "Installed: 0" over an empty grid.
                ap::catalog_status(0, 0)
            } else if me.view.installed_view() {
                // Installed ones on top, the store under them: both parts.
                let installed = me
                    .cards
                    .iter()
                    .filter(|c| matches!(c, Card::Theme(it) if it.local_kind.is_some()))
                    .count();
                // Under the "installed only" filter there is no store showcase
                // (`installed_only`) — "Nothing found" about it would be untrue.
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
        let status = self.as_mut().rust_mut().pending_notice.take().unwrap_or(status);
        self.as_mut().rust_mut().status = status;
        self.as_mut().count_changed();
        self.as_mut().status_changed();
        self.as_mut().page_changed();
        // For local themes without their own picture we draw a frame — before the
        // network, not after: they do not need the network, and a tile must not
        // stand empty for a second.
        self.as_mut().draw_local_placeholders();
        // Pictures of the shown page — all at once, not one per tile: how many fly
        // at once and in what order is decided by the core's queue, and a live tile
        // always overtakes the background warm-up.
        self.as_mut().enqueue_page_previews();
        // …and then the cache of the next page, so that "Forward" opens without
        // the network.
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
        // The same file is the picture of the open details panel: there is no
        // reason to download it a second time, and it has to be shown at once.
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
        roles.append(ROLE_BASE + ROLE_PREVIEW_FONT as i32);
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
        // The "in favourites" mark lives on the tiles too: we repaint them by
        // exactly two roles, not by resetting the whole model.
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

/// The request key: with the same key the core checks the answer and
/// discards someone else's.
///
/// There is no rule of our own here — the key is assembled by the core
/// (`CatalogView::request_key`), the same way for the store, AUR and the
/// disk overviews.
fn request_key(view: &ap::CatalogView) -> String {
    view.request_key()
}

/// Where to take the section's data from.
/// The theme file's timestamp in epoch seconds. `0` means there is no file.
///
/// By it the placeholder cache key tells "the same theme" from "the same
/// theme after an edit": for a colour scheme the frame is drawn from its
/// contents.
fn file_stamp(path: &str) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn request_kind(view: &ap::CatalogView) -> &'static str {
    // Under the "installed only" filter we do not ask the store — only the
    // disk (`CatalogView::installed_only`).
    if view.installed_only() {
        return "local";
    }
    // The installed items and a store page in one request; the store is the
    // same as without the installed ones (`store_kind`).
    if view.installed_view() {
        "combined"
    } else {
        store_kind(view)
    }
}

/// The section's store source (or the disk overview).
fn store_kind(view: &ap::CatalogView) -> &'static str {
    if view.is_local() {
        "local"
    } else if view.aur_mode() {
        "aur"
    } else if view.both_mode() {
        "both"
    } else if view.spices_mode() {
        "spices"
    } else if view.wallhaven_mode() {
        "wallhaven"
    } else if view.ego_mode() {
        "ego"
    } else {
        "ocs"
    }
}

impl cxx_qt::Initialize for qobject::AppearanceModel {
    fn initialize(mut self: Pin<&mut Self>) {
        // The skin change subscription: one line per page, the broadcast is shared
        // (`crate::skin`). Removed in `Drop` of the state.
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
// Details panel
// ============================================================================

impl qobject::AppearanceModel {
    // ── Fixed labels ────────────────────────────────────────────────────
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
            // The choice block — only for a store item: a theme from disk and an AUR
            // package have no store files.
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
        // There is no reason to re-apply the active theme — then the button has a
        // badge; an add-on and a widget get the reverse action (the core's
        // decision).
        QString::from(&self.detail_local_button().0)
    }

    /// The action button of a local theme on the details panel: (label,
    /// whether it can be pressed).
    fn detail_local_button(&self) -> (String, bool) {
        let detail = self.rust().detail.as_ref();
        let kind = detail.and_then(|it| it.local_kind.clone()).unwrap_or_default();
        let id = detail.map(|it| it.id.clone()).unwrap_or_default();
        ap::detail_local_button(&kind, &id)
    }
    fn detail_preview_tooltip(&self) -> QString {
        QString::from(&ap::detail_preview_tooltip())
    }
    /// The AUR package tile button next to «Install»: the package page on the
    /// site (as in the original; a click on the tile itself leads there too).
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

    // ── Contents of the opened item ─────────────────────────────────────
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
        // For a local theme the author field already carries
        // "<icon> <Type> · <badge>": it has no licence, date or version at all.
        if item.local_kind.is_some() {
            return QString::from(ap::card_author(&item.author));
        }
        // For an AUR package — the maintainer, the licence, the date and the
        // version from the passport (before the RPC answer — from the tile).
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
        // An AUR package has no stars: the votes and popularity are in its
        // information.
        if self.rust().aur_detail.is_some() {
            return QString::default();
        }
        match self.rust().detail.as_ref() {
            Some(it) if it.local_kind.is_none() && it.id.starts_with(cs::ID_PREFIX) => {
                let score = self.rust().spices.get(&it.id).map_or(0, |x| x.score);
                QString::from(&cs::detail_stars(score))
            }
            // Wallhaven has no rating — only "favourites" and views.
            Some(it) if wh::is_item_id(&it.id) => QString::default(),
            Some(it) if it.local_kind.is_none() => {
                QString::from(&ap::detail_stars_label(it.rated_stars()))
            }
            _ => QString::default(),
        }
    }

    fn detail_downloads(&self) -> QString {
        if self.rust().aur_detail.is_some() {
            return QString::default();
        }
        match self.rust().detail.as_ref() {
            // Cinnamon Spices and Wallhaven have no download counts — we do not show
            // the line.
            Some(it) if it.id.starts_with(cs::ID_PREFIX) || wh::is_item_id(&it.id) => QString::default(),
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
        // A package is installed by the package manager — the section has no
        // installation directory.
        if me.aur_detail.is_some() {
            return QString::default();
        }
        let cat = categories::category_by_id(&me.home, item.category_id);
        let cat_name = cat
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("cat {}", item.category_id));
        // For a local theme the path is known from the disk; the category table
        // does not know it (virtual sections have no installation directory of
        // their own).
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
        // While the translation is on its way (and longer than the threshold),
        // under the original there is a «Translating the description…» note:
        // otherwise the English text looked as if nobody was going to translate
        // it (06.11, theme 2380824).
        let note = if me.detail_translating && !was_translated {
            ap::description_translating_html()
        } else {
            String::new()
        };
        QString::from(&(ap::description_html(
            &item.description,
            // Warning: the VISIBLE description (`display_description` — the
            // translation if there is one, otherwise the original), not the
            // translation field. `description_translated` stood here, and an item
            // without a translation showed «No description» instead of its English
            // text: "show translation" is the SECOND position of the toggle, and the
            // first is simply "what there is". Python had exactly this
            // (`_desc_trans = ... if was_translated else _desc_orig`), and the live run
            // of 21.09 found the divergence as soon as the translation could be turned
            // off (no model — no translation, but there is a description).
            item.display_description(),
            was_translated,
            me.detail_show_original,
            &accent,
        ) + &note))
    }

    fn detail_button(&self) -> ap::InstallButton {
        match self.rust().detail.as_ref() {
            Some(it) if it.local_kind.is_none() && it.id.starts_with(cs::ID_PREFIX) => {
                let x = self.rust().spices.get(&it.id);
                cs::detail_button_for(it.installed, x.is_some_and(|x| x.update), x.is_some_and(|x| x.apply))
            }
            Some(it) => {
                let home = &self.rust().home;
                let can_apply = ap::store_item_can_apply(home, it);
                ap::store_detail_button(it.installed, it.applied, can_apply, ap::store_item_switch(home, it))
            }
            None => ap::install_button(false, false, false),
        }
    }

    fn detail_install_text(&self) -> QString {
        QString::from(&self.detail_button().text)
    }
    fn detail_install_tooltip(&self) -> QString {
        // A GNOME extension is installed by the shell, and it asks for the
        // confirmation itself with its own window — so that it does not catch the
        // user unawares.
        if let Some(it) = self.rust().detail.as_ref() {
            if !it.installed && rexon_core::gnome_ego::is_item_id(&it.id) {
                return QString::from(&gext::install_tooltip());
            }
        }
        QString::from(&self.detail_button().tooltip)
    }
    fn detail_install_state(&self) -> QString {
        QString::from(self.detail_button().state)
    }

    fn detail_uninstall_visible(&self) -> bool {
        // An AUR package — no: removing a store item will not remove it, a package
        // is removed by the package manager. A theme from disk — by the core's
        // decision (`theme_removal`): a tile without a path has no button.
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
            // A store item: the host's decision came with the details (an enabled
            // theme cannot be removed); until it came — allowed, the host will check
            // once more on removal.
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

    /// The "Remove" decision for the open theme from disk.
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

    /// The Conky widget number for «Start at login», the parts and the settings
    /// on the details panel: for an installed tile — its number, for an
    /// installed store item — the number of its installed tile (the same
    /// actions as in "Downloaded"); empty means not a Conky widget or not
    /// installed yet.
    fn detail_conky_id(&self) -> QString {
        let Some(it) = self.rust().detail.as_ref() else { return QString::default() };
        if it.local_kind.is_some() {
            return if ap::detail_autostart_visible(&it.id) { QString::from(&it.id) } else { QString::default() };
        }
        if !it.installed || !rexon_core::de_apply::is_conky_item(it) {
            return QString::default();
        }
        let home = &self.rust().home;
        let local = ap::live_logged_theme_name(&home.to_string_lossy(), &it.id)
            .and_then(|name| ap::store_local_id(home, it, &name))
            .filter(|id| ap::detail_autostart_visible(id));
        local.map(|id| QString::from(&id)).unwrap_or_default()
    }

    /// The kind of a local tile and its name by the number
    /// `local:<kind>:<name>`.
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

    fn detail_gnome(&self) -> QString {
        let me = self.rust();
        let Some(it) = me.detail.as_ref() else {
            return QString::from("{}");
        };
        let Some(x) = me.gnome.get(&it.id).filter(|x| x.shell_known) else {
            return QString::from("{}");
        };
        let btn = gext::toggle_button(x.enabled, x.active, x.can_change);
        QString::from(
            &json!({
                "visible": true,
                "toggle": {"text": btn.text, "state": btn.state, "enabled": btn.enabled},
                "tooltip": if x.can_change { String::new() } else { gext::locked_tooltip() },
                "prefs": x.has_prefs,
                "folder": x.folder,
                "remove": x.can_remove,
                "error": x.error,
                "url": x.url,
                "name": ap::card_name(&it.name),
            })
            .to_string(),
        )
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
        // The shim function is called through the bridge of the `Action` singleton
        // (a second declaration would give a duplicate symbol at link time, see
        // `we_dev_page`).
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

    // ── Actions ─────────────────────────────────────────────────────────
    fn open_detail(self: Pin<&mut Self>, item_id: &QString) {
        let id = item_id.to_string();
        let found = self
            .rust()
            .cards
            .iter()
            .find(|c| c.id() == id)
            .and_then(|c| match c {
                Card::Theme(it) => Some(((**it).clone(), None)),
                // An AUR package has its own card inside the program, like a store item:
                // the RPC passport, dependencies, comments from the package page. The
                // shared part of the panel (name, description, picture, comments) takes
                // the item assembled from the tile.
                Card::Aur(pkg) => Some((aur_detail_item(pkg), Some((**pkg).clone()))),
            });
        let Some((item, aur_pkg)) = found else { return };
        self.open_detail_with(item, aur_pkg);
    }

    fn open_linked_item(self: Pin<&mut Self>, payload: &QString) {
        let Ok(v) = serde_json::from_str::<Value>(&payload.to_string()) else {
            return;
        };
        if let Some(name) = v.get("aur").and_then(Value::as_str).filter(|n| !n.is_empty()) {
            // A package by name: the card will request the passport, pictures and
            // comments itself — by the same path as for a tile.
            let pkg = AurPackage { name: name.to_string(), ..AurPackage::default() };
            self.open_detail_with(aur_detail_item(&pkg), Some(pkg));
            return;
        }
        let Some(item) = v.get("item").and_then(|i| serde_json::from_value::<ThemeItem>(i.clone()).ok()) else {
            return;
        };
        if item.id.is_empty() {
            return;
        }
        self.open_detail_with(item, None);
    }

    /// The item details panel: from the grid or by a link.
    fn open_detail_with(mut self: Pin<&mut Self>, mut item: ThemeItem, aur_pkg: Option<AurPackage>) {
        // The tile's ready preview becomes the first frame — otherwise the details
        // panel would for a moment show emptiness where the picture is already on
        // disk.
        if item.preview_local.is_empty() {
            item.preview_local = tile_picture(&self.rust().previews, &item);
        }
        // For a font tile the "picture" is the font file (the sample is drawn with
        // it): it is not fit as a card frame.
        if ap::is_font_file(&item.preview_local) {
            item.preview_local.clear();
        }
        let frame = item.preview_local.clone();
        let local = item.local_kind.is_some();
        let id = item.id.clone();
        let is_aur = aur_pkg.is_some();
        {
            let mut me = self.as_mut().rust_mut();
            // A package card: the picture from the tile is frame zero; if there is
            // nowhere to take a picture from (the project is not on
            // GitHub/GitLab/Codeberg), the same icon as on the tile stands in its
            // place.
            me.aur_detail = aur_pkg.map(|pkg| {
                // A source check done earlier is visible at once; a fresh one will come
                // after the passport.
                let trust = aur_trust_entry(&pkg.name);
                Box::new(AurDetail {
                    pkg,
                    trust,
                    ..AurDetail::default()
                })
            });
            me.detail_show_original = false;
            me.detail_translating = false;
            // The outcome of the previous item does not belong to the new one — we
            // clear it (a defect of the original: the message hung on someone else's
            // panel). The progress bar and the locked buttons — likewise.
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
            // A local theme does not go to the store for a picture: if there is no
            // file on disk, there will be no preview at all, and there is no reason to
            // wait for it.
            me.detail_frame_text = if !frame.is_empty() {
                String::new()
            } else if let Some(icon) = aur_icon {
                icon
            } else if local {
                ap::detail_preview_unavailable()
            } else {
                ap::detail_preview_loading()
            };
            // The gallery and comments of the PREVIOUS item belong to someone else;
            // their number, frames and counter do not apply to the new one.
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
            // The package tile's picture is frame zero at once: the card sends the
            // rest (README) as frames by number.
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
        // A local theme lies on disk whole — we do not go to the store for it.
        if !local {
            self.as_mut().detail_wanted(QString::from(&id));
        }
    }

    fn refresh_conky_detail_picture(mut self: Pin<&mut Self>) {
        let Some(name) = self
            .rust()
            .detail
            .as_ref()
            .and_then(|it| it.id.strip_prefix("local:conky:").map(str::to_string))
        else {
            return;
        };
        let ctx = rexon_core::de_apply::Ctx::live();
        let Some(path) = rexon_core::conky_scan::tile_picture(&rexon_core::animations::SystemIo, &ctx, &name) else {
            return;
        };
        if self.rust().detail_frame == path {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            me.detail_frame = path;
            me.detail_frame_text = String::new();
        }
        self.as_mut().detail_frame_changed();
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
        // An answer about an item that has already been closed is silently
        // discarded.
        {
            let me = self.rust();
            match me.detail.as_ref() {
                Some(open) if open.id == item.id => {}
                _ => return,
            }
        }
        // "Remove" of an installed item: whether it is allowed (an enabled theme —
        // no).
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
            // We do not lose a picture already shown: the full details do not carry it
            // (there are only links there).
            let shown = me.detail_frame.clone();
            let mut item = item;
            if item.preview_local.is_empty() {
                item.preview_local = shown.clone();
            }
            // The tile's description translation may have come BEFORE the details (it
            // runs in its own thread, without waiting for the store). If the
            // description is the same, the translation is kept, otherwise the text
            // would flash English until the second (cached) answer.
            if let Some(old) = me.detail.as_ref() {
                if item.description_translated.is_empty() && old.description == item.description {
                    item.description_translated = old.description_translated.clone();
                }
            }
            me.detail = Some(Box::new(item));
            shown
        };
        // The carousel — EXACTLY as on the site: without the first slot (it is the
        // "face of the item" and stays the tile preview). The list is assembled by
        // the core, and it also upgrades reduced copies to full resolution.
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
            // The tile's ready preview is the carousel's frame zero: there is no
            // reason to download it a second time.
            if !frame.is_empty() {
                me.gallery_loaded.insert(0, frame.clone());
            }
        }
        self.as_mut().detail_changed();
        self.as_mut().sync_frame();
        // Cinnamon Spices has no comments for programs, Wallhaven has no API of
        // its own for them, extensions.gnome.org neither (asking the KDE Store for
        // comments by an `ego:` number is pointless).
        if !local
            && !id.is_empty()
            && !id.starts_with(cs::ID_PREFIX)
            && !wh::is_item_id(&id)
            && !rexon_core::gnome_ego::is_item_id(&id)
        {
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
        // The source the translation was computed from: the panel may have got
        // another description in the meantime (fresh details, the AUR passport),
        // and it does not need a translation of someone else's text.
        let source = v.get("source").and_then(Value::as_str);
        // `pending` is only the "translating" note: `true` — the translation is
        // taking longer than the threshold, `false` — the work ended without a
        // translation (the core decided "do not translate" or there is no model).
        let pending = v.get("pending").and_then(Value::as_bool);
        if id.is_empty() {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            // An answer about an item that has already been closed or changed is
            // silently discarded — exactly as in `detail_answer`. The core has its own
            // watcher, but between its check and this line the window's queue manages
            // to turn, and the last word about what is open belongs to the page.
            let me = &mut *me;
            let Some(open) = me.detail.as_mut().filter(|o| o.id == id) else {
                return;
            };
            if source.is_some_and(|s| s != open.description) {
                return;
            }
            if let Some(pending) = pending {
                if me.detail_translating == pending {
                    return;
                }
                me.detail_translating = pending;
            } else {
                // An empty translation is not a translation: it would only light the
                // "show original" link over an empty place (`description_html` holds on to
                // `was_translated`).
                if translated.trim().is_empty() {
                    return;
                }
                open.description_translated = translated.to_string();
                me.detail_translating = false;
            }
            // The "translation ⇄ original" toggle stays in the position the user left
            // it in: they may have pressed "show original" before the translation
            // arrived — resetting it would jerk the text under their hand.
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
                    // For a local theme the name is baked into the number
                    // (`local:<kind>:<name>`) — we take it from there, as the original does.
                    Card::Theme(it) => ap::local_theme_name(&it.id, &it.name),
                    Card::Aur(p) => p.name.clone(),
                })
                .or_else(|| {
                    me.detail
                        .as_ref()
                        .filter(|it| it.id == id)
                        .map(|it| ap::local_theme_name(&it.id, &it.name))
                })
                // An action without a tile is named by its own number.
                .unwrap_or_else(|| id.clone())
        };
        self.begin_action(&id, &kind, &name);
    }

    fn install_file_begin(self: Pin<&mut Self>, paths_json: &QString) {
        let paths: Vec<String> = serde_json::from_str(&paths_json.to_string()).unwrap_or_default();
        let label = rexon_core::local_install::busy_label(&paths);
        self.begin_action("", ap::ACTION_LOCAL, &label);
    }

    /// The shared start of a long action: the buttons are locked, the first
    /// progress line.
    fn begin_action(mut self: Pin<&mut Self>, id: &str, kind: &str, name: &str) {
        let (id, kind) = (id.to_string(), kind.to_string());
        {
            let mut me = self.as_mut().rust_mut();
            me.action_busy = true;
            me.action_kind = kind.clone();
            me.action_item = id.clone();
            me.install_percent = 0;
            // The progress is visible in two places at once: in the grid's status line
            // and on the details panel. On the details the grid's status line is
            // HIDDEN, and without its own label the installation there looked like
            // "the button does not respond".
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
        // Just finished ones: their outcome goes to the status line, and the tiles
        // mark the succeeded packages (with the set's composition) as "installed".
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
            // The line on top: the running installation with the package name; when it
            // ends — its outcome. The same line is not set a second time, so as not to
            // override messages that came after it.
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
            // Progress arrived — so the action is still running: we keep the bar even
            // if we were not told about the start.
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
            // The core sets the icon, and only if there is none yet: the installer
            // gives ready "✅ Installed: …".
            ap::action_result_status(ok, &text)
        };
        // The outcome of an installation from the store goes to this item's tile
        // and details panel: "installed" at once, "applied" — by the fact from the
        // journal and the map of active themes (the core's decision). Without this
        // the button stayed at «Install» until the section was re-entered.
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
            // The action ended: the progress bar goes away, the buttons are unlocked.
            // A successful theme apply asks to rebuild the grid — but not now, rather
            // when the Qt style change settles (the core's decision).
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

    /// Put the outcome of a successful installation onto the item's tile and
    /// details panel.
    ///
    /// The state is computed by the core (`mark_installed_live`): the journal is
    /// read once, the map of active themes — only if the kind has "applied".
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
// Preview loader
// ============================================================================
//
// The network lives at the host, the rules at the core (`ap::PreviewQueue`).
// There is exactly one bridge between them: the queue says WHAT to launch,
// the host brings the answer, and the queue decides whether to repeat a
// failure and when to hand the ready result up.
//
// One request per address, six pictures at once, of them no more than two
// for the background warm-up, two extra attempts for a temporary failure
// and a generation of its own for each section — all of these are numbers
// and rules of the core, there are none of them here.

impl qobject::AppearanceModel {
    /// Is the picture already in the cache? The path is computed by the core —
    /// the same one by which the host writes it.
    fn cached_preview(&self, url: &str) -> Option<String> {
        let home = self.rust().home.to_string_lossy().to_string();
        let path = ap::preview_cache_path(&home, url);
        if std::path::Path::new(&path).is_file() {
            Some(path)
        } else {
            None
        }
    }

    /// Order the pictures of all tiles of the shown page.
    ///
    /// The original ordered them the same way — the whole page at once
    /// (`_start_preview_load`), not one per appearing tile: the queue keeps the
    /// channel at six requests anyway, and the order in it matters more than
    /// laziness.
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

    /// Draw the tiles of local themes that have neither their own picture nor a
    /// picture from the store record.
    ///
    /// The third and fourth steps of the single preview chain
    /// (`rexon_core::local_preview`): a sample from the theme's contents, and if
    /// there is nothing to take from it — a kind placeholder. What to draw is
    /// decided by the core by the kinds table; there is not a single branching
    /// by theme kind here — only the frame cache and execution
    /// (`crate::local_preview_render`).
    ///
    /// The frame is put as a file and taken from the cache on the next visit:
    /// the tile needs a picture ADDRESS, and there is no way to give it bytes in
    /// memory. Ready frames are set at once; the missing ones are drawn by a
    /// background thread in tile order and handed over one by one
    /// ([`Self::local_picture_ready`]). They must not be drawn in the interface
    /// thread: the first showing of "System" is about two hundred frames with
    /// reading theme files, and the window froze for that time.
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
        // Full-screen pictures of ready tiles (tile number → file).
        let mut bigs: Vec<(String, String)> = Vec::new();
        let mut todo: Vec<PictureJob> = Vec::new();
        // KDE window borders are captured by their modules; what is on the machine
        // — once per batch of tiles.
        let shots = wants
            .iter()
            .any(|w| w.0.starts_with("local:aurorae:"))
            .then(|| rexon_core::kdecoration::ShotSetup::live(&home));
        // Qt styles, Kvantum and GTK themes are captured with real widgets.
        let widget_shots = wants
            .iter()
            .any(|w| {
                ["local:widgetstyle:", "local:kvantum:", "local:gtk:", NATIVE_TILE_PREFIX].iter().any(|p| w.0.starts_with(p))
            })
            .then(|| rexon_core::widget_shot::ShotSetup::live(&home));
        let de_now = self.rust().view.de.clone();
        for (id, category_id, install_path, name, author) in wants {
            let kind = rexon_core::de_apply::apply_kind_of_local_id(&id).unwrap_or("").to_string();
            let glyph = lp::type_emoji(&author);
            let shot = shots.as_ref().and_then(|s| s.request(&id, &name));
            // A border snapshot also depends on the border size and the KDE colours.
            let shot_tag = match (&shots, kind.as_str()) {
                (Some(s), "aurorae") => s.tag.clone(),
                _ => String::new(),
            };
            // …and on the border's own settings: after the "Configure…" window — a new
            // key.
            let shot_tag = match self.rust().deco_config_gen.get(&id) {
                Some(n) if !shot_tag.is_empty() => format!("{shot_tag}\u{0}cfg{n}"),
                _ => shot_tag,
            };
            let widget_shot = widget_shots.as_ref().and_then(|s| s.request(&id, &install_path));
            let shot_tag = match widget_shots.as_ref().map(|s| s.cache_tag(&id, &install_path)) {
                Some(tag) if !tag.is_empty() => format!("{shot_tag}\u{0}{tag}"),
                _ => shot_tag,
            };
            // A whole theme of the desktop: its picture holds a widget shot of its
            // controls theme when there is something to shoot with.
            // Where the theme sets the dark variant itself (MATE), the picture does
            // not follow the GTK settings of the moment.
            let shot_tag = match widget_shots.as_ref() {
                Some(s) if id.starts_with(NATIVE_TILE_PREFIX) => format!(
                    "{shot_tag}\u{0}{}{}\u{0}{}",
                    rexon_core::widget_shot::style_tag(),
                    s.helper.is_some() && s.gtk_display,
                    if rexon_core::native_global::sets_dark_preference(&de_now) {
                        "own".to_string()
                    } else {
                        s.gtk_dark.to_string()
                    }
                ),
                _ => shot_tag,
            };
            // For a colour scheme the scan gives the path without the extension; the
            // timestamp needed is that of its file — the frame is drawn from its
            // contents.
            let colors_file = format!("{install_path}.colors");
            let stamped = if !std::path::Path::new(&install_path).exists()
                && std::path::Path::new(&colors_file).is_file()
            {
                colors_file
            } else {
                install_path.clone()
            };
            // The key holds everything the frame depends on: the kind, the path, the
            // kind sign, the timestamp and the rendering rules stamp.
            let dest = ap::placeholder_cache_path(
                &home,
                category_id,
                &install_path,
                &name,
                &format!("{}\u{0}{kind}\u{0}{glyph}\u{0}{shot_tag}", lp::PICTURE_STYLE),
                file_stamp(&stamped),
            );
            if std::path::Path::new(&dest).is_file() {
                if let Some(big) = big_picture_path(&dest) {
                    bigs.push((id.clone(), big));
                }
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
                for (id, big) in &bigs {
                    set_card_big_picture(&mut me.cards, id, big);
                }
            }
            // The frames appeared after a model reset — we repaint one role, not the
            // whole showcase.
            let rows = self.rust().cards.len();
            if rows > 0 {
                let first = self.as_ref().index(0, 0, &QModelIndex::default());
                let last = self
                    .as_ref()
                    .index(rows as i32 - 1, 0, &QModelIndex::default());
                let mut roles = cxx_qt_lib::QVector::<i32>::default();
                roles.append(ROLE_BASE + ROLE_PREVIEW as i32);
                roles.append(ROLE_BASE + ROLE_PREVIEW_FONT as i32);
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
        let gtk_shots = widget_shots.as_ref().is_some_and(|s| s.gtk_display);
        let gtk_dark = widget_shots.as_ref().is_some_and(|s| s.gtk_dark);
        let widget_helper = widget_shots.and_then(|s| s.helper);
        let de = self.rust().view.de.clone();
        let spawned = std::thread::Builder::new().name("rexon-tile-pictures".into()).spawn(move || {
            let icon_themes = lp::icon_theme_dirs(&home, "/usr");
            // The whole themes of the desktop on disk — once per batch.
            let natives = todo
                .iter()
                .any(|j| j.id.starts_with(NATIVE_TILE_PREFIX))
                .then(|| rexon_core::native_global::scan_live(&de, &home))
                .unwrap_or_default();
            for job in todo {
                if let Some(key) = job.id.strip_prefix(NATIVE_TILE_PREFIX) {
                    let theme = natives.iter().find(|t| t.key == key);
                    let helper = widget_helper.as_deref().filter(|_| gtk_shots);
                    let (dest, big) = (job.dest.clone(), format!("{}{BIG_PICTURE_SUFFIX}", job.dest));
                    let drawn = draw_native_picture(theme, &home, &de, helper, gtk_dark, &job.glyph, &dest, &big);
                    let key = drawn_key(&job.id, &job.install_path);
                    let id = job.id;
                    if handle
                        .queue(move |object| object.local_picture_ready(&id, &key, drawn.then_some(dest)))
                        .is_err()
                    {
                        return;
                    }
                    continue;
                }
                // A KDE border is drawn by its module in a separate process; if it failed
                // — the previous picture (a sample from SVG or a placeholder).
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
                // A Qt style and a GTK theme are captured by the helper with real widgets;
                // if it failed — a drawn sample by the theme's colours.
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
                // The window was closed — the queue does not accept: there is nobody to
                // finish drawing for.
                if handle
                    .queue(move |object| object.local_picture_ready(&id, &key, drawn.then_some(dest)))
                    .is_err()
                {
                    return;
                }
            }
        });
        if spawned.is_err() {
            // The thread did not start — the tiles stay without a picture until the
            // next visit to the section, but the order is not considered hanging.
            self.as_mut().rust_mut().drawing.clear();
        }
    }

    /// The background thread drew the picture of tile `id` (`None` — it
    /// failed).
    fn local_picture_ready(mut self: Pin<&mut Self>, id: &str, key: &str, path: Option<String>) {
        let row = {
            let mut me = self.as_mut().rust_mut();
            me.drawing.remove(id);
            let Some(path) = path else { return };
            me.previews.insert(key.to_string(), path.clone());
            if let Some(big) = big_picture_path(&path) {
                set_card_big_picture(&mut me.cards, id, &big);
            }
            // The open details panel is waiting for this frame (the border snapshot
            // after the "Configure…" window).
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
        // The section has changed since — the picture stayed in the cache and will
        // be set on the next visit; there is nothing to repaint.
        let Some(row) = row else { return };
        let index = self.as_ref().index(row as i32, 0, &QModelIndex::default());
        let mut roles = cxx_qt_lib::QVector::<i32>::default();
        roles.append(ROLE_BASE + ROLE_PREVIEW as i32);
        roles.append(ROLE_BASE + ROLE_PREVIEW_FONT as i32);
        self.as_mut().data_changed(&index, &index, &roles);
    }

    /// A background warm-up of the first carousel frame of the shown tiles.
    ///
    /// A tile and the carousel are DIFFERENT pictures: the tile has a
    /// thumbnail, the carousel the full size from the second screenshot. For
    /// years only the tile was warmed up, so a click on an already shown card
    /// still ran into the network.
    fn warm_gallery(mut self: Pin<&mut Self>, generation: &QString) {
        let asked: u64 = generation.to_string().parse().unwrap_or(u64::MAX);
        if asked != self.rust().requests.generation() {
            return; // the section has changed since — there is nothing left to warm up
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

    /// A background warm-up of the pictures of ANOTHER page (its tiles are not
    /// on screen — nothing is handed up, the cache simply fills).
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

    /// Hand the host what the queue allowed to launch, and wake up the delivery
    /// of the ready ones.
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

    /// An answer for the request arrived — remove it from accounting and return
    /// what was asked for.
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
        // We aborted the request ourselves (left the section) — this is not a
        // network failure, and there is no reason to repeat it.
        if status == PREVIEW_CANCELED {
            self.as_mut().pump_previews();
            return;
        }
        // A live tile has spare pictures of the item: if the first did not load —
        // the next one at once. Repeating the same link is superfluous here: the
        // download has already repeated temporary network failures itself.
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

    /// Drop everything flying and queued for previews: the section changed.
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
// Background warm-up
// ============================================================================
//
// Two different kinds of seamlessness with one mechanism (rule #12):
//   * THE NEIGHBOURING SECTION — N was opened, N+1, N+2… are warmed up in
//     the background: a switch opens instantly from the cache;
//   * THE NEXT PAGE of the open section — "Forward" also without the
//     network.
//
// The queue, the order, dedup and "one at a time" are held by the core
// (`ap::CatalogRequests`), the network by the host. The user's live request
// always matters more: while the section they are looking at is loading,
// we do not occupy the channel, and the queue will catch up after the
// answer.

impl qobject::AppearanceModel {
    fn start_category_prefetch(mut self: Pin<&mut Self>) {
        let targets = {
            let me = self.rust();
            ap::prefetch_targets(&me.view, &me.sidebar)
        };
        self.as_mut().rust_mut().requests.enqueue_prefetch(&targets);
        self.as_mut().pump_prefetch();
    }

    /// Put the neighbours of the open section at the front of the queue.
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
            "pageSize": self.rust().page_size(),
            "sort": sort,
            "query": "",
        });
        self.as_mut()
            .prefetch_wanted(QString::from(&body.to_string()));
    }

    /// Warm up the cache of the NEXT page of the open section.
    ///
    /// One warm-up per page per session: without this every repeated visit to
    /// the section sent the same request again and flooded the channel. The
    /// user's live request for the same page also counts as a warm-up.
    fn prefetch_next_page(mut self: Pin<&mut Self>) {
        let request = {
            let me = self.rust();
            // Local sections and AUR lie on disk, Cinnamon Spices is paged by its own
            // index — there is nothing to warm up for them.
            if me.view.is_local()
                || me.view.aur_mode()
                || me.view.own_site_store()
                || me.total <= 0
                || self.installed_only()
            {
                None
            } else {
                // Whether there is a next one — by the fact of the stream, if the answer
                // knows it.
                let more = me.has_more.unwrap_or_else(|| {
                    ap::has_page_to_prefetch(me.view.page, me.total, me.page_size())
                });
                if !more {
                    None
                } else {
                    // The sort must match the one the live request will go by, otherwise the
                    // cache key of page N+1 will not match when "Forward" is pressed.
                    Some(json!({
                        "kind": PREFETCH_PAGE,
                        "categoryId": me.view.query_category_id(),
                        "page": me.view.page + 1,
                        "pageSize": me.page_size(),
                        "sort": me.view.active_sort(),
                        "query": me.view.query,
                    }))
                }
            }
        };
        let Some(body) = request else { return };
        // The page size is in the key: at another window width page N holds other
        // works, and the previous warm-up does not cover it.
        let key = format!(
            "{}:n{}",
            ocs::list_key(
                body["categoryId"].as_str().unwrap_or_default(),
                body["page"].as_i64().unwrap_or_default(),
                body["sort"].as_str().unwrap_or_default(),
                body["query"].as_str().unwrap_or_default(),
            ),
            body["pageSize"].as_u64().unwrap_or_default(),
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
        // The section's target is marked done regardless of the catch: the cache
        // simply will not fill, and there is no reason to pull the same target a
        // second time.
        if s(&v, "kind") == PREFETCH_CATEGORY {
            let cat_id = s(&v, "categoryId").to_string();
            let sort = s(&v, "sort").to_string();
            self.as_mut()
                .rust_mut()
                .requests
                .prefetch_finished((cat_id, sort));
        }
        // Pictures of another page — with a low priority: there is nowhere to show
        // them, they only fill the cache by the time it is opened.
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
            // A theme was just applied: a cached disk scan would show «✅ Active» on
            // the previous one.
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
            // The host has already reset the scan cache, but a request without
            // bypassing could meet it refilled by a neighbouring warm-up.
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
                // Wait on the side — otherwise a closed window would hang as a zombie.
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
                // As in KDE: the settings window was closed — the border sample is drawn
                // anew (the settings may have changed its look).
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

    /// The settings window of border `id` was closed: the previous snapshot goes
    /// out of the cache, a new one is put under another key (`deco_config_gen`)
    /// and lands on the tile and in the open details panel.
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
                    // As in the KDE module: the editor was closed — the list of schemes is
                    // re-read (a new scheme, an edit of our own).
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

    /// The schemes on disk changed (our own editor or the KDE editor): the grid
    /// re-reads the disk bypassing the cache — at once if it is open, otherwise
    /// on the next request.
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
// Screenshot gallery and comments
// ============================================================================

/// One comment in the form the details panel shows it.
#[derive(Debug, Clone, Default)]
pub struct CommentRow {
    /// Pinned by the AUR package maintainer (an icon before the nickname).
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
    // ── Fixed labels of the overlay controls ────────────────────────────
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

    // ── Gallery ─────────────────────────────────────────────────────────
    fn gallery_total(&self) -> i64 {
        let me = self.rust();
        ap::gallery_total(
            me.gallery_known,
            me.gallery_src.len(),
            me.gallery_loaded.len(),
        )
    }

    /// How many gallery frames are still alive (broken ones are skipped when
    /// paging).
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

    /// Recompute the frame in place of the picture and ask the host for what is
    /// missing.
    fn sync_frame(mut self: Pin<&mut Self>) {
        let total = self.gallery_total();
        let alive = self.gallery_alive();
        let frame = {
            let me = self.rust();
            if total > 0 && alive == 0 && !me.gallery_loaded.contains_key(&me.gallery_index) {
                // All frames are broken — there will be no picture, rather than "page with
                // the arrows".
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
                // A single picture that is not there yet: the panel stays as it was —
                // there is no reason to flash emptiness for nothing.
                ap::GalleryFrame::Unchanged => {}
                // There will be no picture. The tile preview, if it already stands in the
                // frame's place, stays — the placeholder is only instead of emptiness.
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

    /// Ask for the frame shown right now if it is not there yet.
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
            // From now on a frame from the cache will not move the user: they chose
            // themselves.
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
        // A frame was picked up from the cache, and the user has not paged yet —
        // we move to the earliest ready one (the core's decision).
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
            // The frame we stand on did not load — we move to the next live one: a
            // broken frame is not shown in the gallery.
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
        // We show ONLY what really lies on disk: the decision and the file check
        // are done by the core, here there is only file system access, which the
        // core does not have.
        let exists = |p: &str| std::path::Path::new(p).is_file();
        let current = me.detail_frame.clone();
        let Some((paths, index)) = ap::zoom_frames(&current, &me.gallery_loaded, exists) else {
            return out;
        };
        // A theme on disk: the card frame is the small tile picture, the
        // full-screen view gets the theme's full-size one.
        let paths = match me.detail.as_ref() {
            Some(it) => ap::zoom_full_size(paths, &it.preview_local, &it.preview_big, exists),
            None => paths,
        };
        for path in paths {
            out.append(qstr(&file_url(&path)));
        }
        // The last slot is the number of the frame to open the viewer from.
        out.append(QVariant::from(&QString::from(&index.to_string())));
        out
    }

    // ── Comments ────────────────────────────────────────────────────────
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
                    // For an AUR package "our own" includes the co-maintainers too.
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
        // For an AUR package the site itself knows "whether there are more" (the
        // "Next" link): a page may be incomplete because of deleted comments.
        if let Some(d) = me.aur_detail.as_ref() {
            return d.comments_more && !me.comments.is_empty();
        }
        // The "Show more" button makes sense while the page arrived full.
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
        // An answer about a closed item is silently discarded.
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
            // The first page replaces the list, the following ones are appended.
            if page == 0 {
                me.comments.clear();
            }
            me.comments.extend(rows);
            me.comments_page = page + 1;
            if total.is_some() {
                me.comments_total = total;
            }
        }
        // An AUR package page also carries what the store does not have: whether
        // there is one more page, whether the site closed itself to us, the
        // co-maintainers.
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
            // An answer about a closed or already changed item is silently discarded —
            // as in `comments_answer`. The core has its own watcher, but between its
            // check and this line the window's queue manages to turn, and the last word
            // about what is open belongs to the page.
            let open = me.detail.as_ref().map(|it| it.id.clone()).unwrap_or_default();
            if open.is_empty() || open != s(&v, "itemId") {
                return;
            }
            // The comments page must be a SHOWN one: pages from zero to
            // `comments_page - 1` are shown. A translation of a page the list has
            // already thrown away (the user closed and reopened the comments) is not
            // about these rows.
            if page < 0 || page >= me.comments_page {
                return;
            }
        }
        // An "original → translation" pair. The cleanup is the same as for the
        // list itself: the comparison must be with what lies in the row, not with
        // the store's raw answer.
        let mut done: Vec<(String, String)> = Vec::new();
        if let Some(list) = v.get("rows").and_then(Value::as_array) {
            for raw in list {
                let source = ap::clean_comment_text(s(raw, "text"));
                let translated = ap::clean_comment_text(s(raw, "translated"));
                // A translation equal to the original does not count as a translation: it
                // would only light the "show original" link over the very same line.
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
                // The "translation ⇄ original" toggle stays in the position the user left
                // it in: they may have pressed "show original" before the translation
                // arrived — resetting it would jerk the text under their hand.
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
// Search suggestions
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
        // No more rows than fit into the drop-down: the number comes from the
        // core.
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
            // A local section and AUR do not know the network: we filter what is
            // already shown — instantly and without a request (the core's decision).
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
        // The user typed further, left the line or erased what was typed — the
        // answer is no longer about what they see (the core's decision).
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
            // In a loop: from the last row down — back to the first.
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

    /// Open the suggestion with this number — like a click on a tile.
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
        // A suggestion leads straight into the details panel: the item is already
        // found, there is no reason to ask for it in the grid a second time.
        let id = item.id.clone();
        let known = self.rust().cards.iter().any(|c| c.id() == id);
        if !known {
            // An item from the drop-down may not be listed in the open grid — we put
            // it there, otherwise the details panel has nowhere to take it from.
            // Through a model reset: the row composition must not be changed silently.
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
// AUR package card
// ============================================================================
//
// The shared part of the details panel (name, description, picture,
// comments, progress bar) works over the item assembled from the package
// tile; the package's own parts are the information from the RPC `info`
// passport and the install button, the same as on the tile (with the
// installation queue). What to show is decided by the core
// (`aur::info_facts`).

/// The package source check record from the cache (process memory, on first
/// access — `~/.cache/rexon/aur-trust.json`). Does not touch the network.
fn aur_trust_entry(name: &str) -> Option<rexon_core::aur_trust::CacheEntry> {
    static PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let path = PATH.get_or_init(rexon_core::aur_trust::live_cache_path);
    rexon_core::aur_trust::cached_entry(path, name)
}

/// Risk signals for the tile: the tile's RPC data and a check done earlier
/// (if the card was already opened).
fn aur_tile_signals(pkg: &AurPackage) -> Vec<rexon_core::aur_trust::Signal> {
    let entry = aur_trust_entry(&pkg.name);
    rexon_core::aur_trust::signals(
        &rexon_core::aur_trust::RpcView::from(pkg),
        entry.as_ref(),
        rexon_core::aur_trust::now_secs(),
    )
}

/// The details panel item from a package tile.
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
        } else if me.detail.as_ref().is_some_and(|it| wh::is_item_id(&it.id)) {
            "wallhaven"
        } else if me.detail.as_ref().is_some_and(|it| rexon_core::gnome_ego::is_item_id(&it.id)) {
            "ego"
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
        // Before the RPC answer — what is known from the tile; if there will be no
        // answer — the reason as the first line.
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
                // The link colour is in the text itself: the style sheet does not paint a
                // fragment inside a label (like the translation toggle).
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
            // An answer about a closed or changed card is silently discarded.
            let me = &mut *me;
            let Some(d) = me.aur_detail.as_mut().filter(|d| d.pkg.name == id) else {
                return;
            };
            match info {
                Some(info) => {
                    // The description and the version are fresh from the passport (the tile
                    // may have come from the section's cache).
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
        // The comments come from the package page, after the passport: two
        // requests to the site in a row do not go out at once.
        self.as_mut()
            .comments_wanted(QString::from(&id), QString::from("0"));
    }

    /// Risk signals of the open card: the deep check (if any) and the RPC
    /// passport (before the answer — the tile's data).
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

    /// The package's git branch in AUR (the PKGBUILD lies under it, not under
    /// the package name, if the base is shared).
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
        // The tile of the same package now knows more — repaint it.
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
        // The tile in the grid is the primary source of the state (the
        // installation queue marks it); if it is not there (the grid was rebuilt)
        // — the card's package.
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

/// Whether the KDE colour scheme editor (`kcolorschemeeditor`) is in PATH.
fn kde_scheme_editor_present() -> bool {
    let path = std::env::var("PATH").ok();
    rexon_core::utils::which(rexon_core::color_scheme_edit::KDE_EDITOR, path.as_deref()).is_some()
}

/// Launch a window of a third-party program (style settings, the scheme
/// editor): without input and output, in its own process group — closing
/// Rexon does not stop it. The error is for the user.
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
