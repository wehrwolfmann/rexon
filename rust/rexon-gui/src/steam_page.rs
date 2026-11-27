// SteamModel — the "Steam" tab in Rust + cxx-qt + QML.
//
// Ported to QML following the common tab scheme: decisions and labels are
// the core's, here is the QObject shell, and QML holds only the markup.
//
// What is here and what is not:
//   * there IS the QObject shell: the page's fixed labels, the palette, the
//     list model of the splash showcase (the delegate's roles), the cards of
//     the current background page, the large card, the status chip, the
//     pagination strip and receiving events from the background tasks
//     (`crate::steam_workers`);
//   * there is NOT a single line of product decisions — what to show, in
//     what order, in what colour and with what words is answered by
//     `rexon_core::qt_steam`.
//
// Warning: the background grid is exposed as a LIST (`bgCards`), not as a
// second model. The reason is not laziness: a page holds `cols × BG_ROWS`
// cards, that is, 25 with five columns and no more than 40 with eight —
// exactly the volume for which there is no reason to create a second model
// with its own roles and its own reset. The splash showcase stayed a model:
// there it is the whole list (the owner has hundreds of clips), and a
// repeater over it would already be wasteful.
//
// Warning: the splash showcase is filled not by the model itself but by the
// page host (`setMovies`), and this is not unfinished work but a boundary:
// the clip scan, the config, favourites and autostart are side actions,
// and their place is in the `actions` object (the same technique as in
// "Extensions" with installing `.rxn`). The Python host puts
// `rexon/qt_steam/qml_window.py` there, the Rust host `steam_actions.rs`;
// both call the same core flow `rexon_core::session_splash` over the live
// executor `rexon_core::splash_exec` (until 08.09.2026 it was locked inside
// `rexon-py`, and the tab could not have a Rust action object).

/// cxx-qt bridge: the content of the "Steam" tab. It is also the splash
/// showcase model.
///
/// Warning: the type is called `SteamModel`, not `SteamPage`: the module's
/// .qml files themselves become QML types by file name, and `SteamPage` is
/// taken by `qml/SteamPage.qml`.
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

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    // The top-level window — for the sake of ONE question: whether the
    // compositor shows our surface.
    //
    // Warning: the window's `visibility` property is NOT enough for this. On
    // Wayland the xdg-shell protocol does not tell the client the "minimized"
    // state at all: after minimizing in KWin the window keeps counting as
    // normal (`Window.Windowed`), and the preview would play video into
    // nowhere. The honest flag is `QWindow::isExposed()`; it has no
    // Q_PROPERTY, so this is the only way to get it from QML.
    unsafe extern "C++" {
        include!(<QtCore/QObject>);
        type QObject;

        include!(<QtGui/QWindow>);
        type QWindow;

        #[cxx_name = "isExposed"]
        fn is_exposed(self: &QWindow) -> bool;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        // ── Window and header ───────────────────────────────────────────
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
        #[qproperty(i32, group_title_point_size, cxx_name = "groupTitlePointSize",
                    READ = group_title_point_size, CONSTANT)]
        // Sub-tabs: [{key, label, checked}, …].
        #[qproperty(QList_QVariant, segments, READ = segments,
                    NOTIFY = section_changed)]
        #[qproperty(QString, section, READ = section, NOTIFY = section_changed)]

        // ── The "Video splash" section ──────────────────────────────────
        #[qproperty(QString, splash_group_title, cxx_name = "splashGroupTitle",
                    READ = splash_group_title, CONSTANT)]
        #[qproperty(QString, splash_pitch, cxx_name = "splashPitch",
                    READ = splash_pitch, CONSTANT)]
        #[qproperty(QString, sv_search_placeholder, cxx_name = "svSearchPlaceholder",
                    READ = sv_search_placeholder, CONSTANT)]
        #[qproperty(QString, sv_pick_text, cxx_name = "svPickText",
                    READ = sv_pick_text, CONSTANT)]
        #[qproperty(QString, sv_pick_tooltip, cxx_name = "svPickTooltip",
                    READ = sv_pick_tooltip, CONSTANT)]
        #[qproperty(QString, sv_pick_dialog_title, cxx_name = "svPickDialogTitle",
                    READ = sv_pick_dialog_title, CONSTANT)]
        #[qproperty(QString, sv_pick_filter, cxx_name = "svPickFilter",
                    READ = sv_pick_filter, CONSTANT)]
        #[qproperty(QString, sv_autostart_text, cxx_name = "svAutostartText",
                    READ = sv_autostart_text, CONSTANT)]
        #[qproperty(QString, sv_autostart_tooltip, cxx_name = "svAutostartTooltip",
                    READ = sv_autostart_tooltip, CONSTANT)]
        #[qproperty(QString, sv_rotate_text, cxx_name = "svRotateText",
                    READ = sv_rotate_text, CONSTANT)]
        #[qproperty(QString, sv_rotate_tooltip, cxx_name = "svRotateTooltip",
                    READ = sv_rotate_tooltip, CONSTANT)]
        #[qproperty(QString, sv_sound_text, cxx_name = "svSoundText",
                    READ = sv_sound_text, CONSTANT)]
        #[qproperty(QString, sv_test_text, cxx_name = "svTestText",
                    READ = sv_test_text, CONSTANT)]
        #[qproperty(QString, sv_test_tooltip, cxx_name = "svTestTooltip",
                    READ = sv_test_tooltip, CONSTANT)]
        #[qproperty(QString, sv_star_tooltip, cxx_name = "svStarTooltip",
                    READ = sv_star_tooltip, CONSTANT)]
        #[qproperty(QString, sv_card_star_tooltip, cxx_name = "svCardStarTooltip",
                    READ = sv_card_star_tooltip, CONSTANT)]
        #[qproperty(QString, sv_test_playing_text, cxx_name = "svTestPlayingText",
                    READ = sv_test_playing_text, CONSTANT)]
        #[qproperty(QString, sv_test_no_item_text, cxx_name = "svTestNoItemText",
                    READ = sv_test_no_item_text, CONSTANT)]
        #[qproperty(QString, sv_thumb_placeholder, cxx_name = "svThumbPlaceholder",
                    READ = sv_thumb_placeholder, CONSTANT)]
        #[qproperty(QString, sv_import_cancel_text, cxx_name = "svImportCancelText",
                    READ = sv_import_cancel_text, CONSTANT)]
        #[qproperty(QString, sv_delete_glyph, cxx_name = "svDeleteGlyph",
                    READ = sv_delete_glyph, CONSTANT)]
        #[qproperty(QString, sv_delete_tooltip, cxx_name = "svDeleteTooltip",
                    READ = sv_delete_tooltip, CONSTANT)]
        #[qproperty(QString, sv_delete_title, cxx_name = "svDeleteTitle",
                    READ = sv_delete_title, CONSTANT)]
        #[qproperty(QString, sv_delete_confirm_text, cxx_name = "svDeleteConfirmText",
                    READ = sv_delete_confirm_text, CONSTANT)]
        #[qproperty(QString, sv_delete_keep_text, cxx_name = "svDeleteKeepText",
                    READ = sv_delete_keep_text, CONSTANT)]
        #[qproperty(QString, sv_steam_choice_glyph, cxx_name = "svSteamChoiceGlyph",
                    READ = sv_steam_choice_glyph, CONSTANT)]
        #[qproperty(QString, sv_steam_choice_tooltip, cxx_name = "svSteamChoiceTooltip",
                    READ = sv_steam_choice_tooltip, CONSTANT)]
        /// A clip is being added (checked, copied or converted) — the add button
        /// is disabled, the progress line and «Cancel» are shown.
        #[qproperty(bool, import_busy, cxx_name = "importBusy",
                    READ = import_busy, NOTIFY = import_changed)]
        #[qproperty(QString, import_text, cxx_name = "importText",
                    READ = import_text, NOTIFY = import_changed)]
        #[qproperty(i32, sv_thumb_width, cxx_name = "svThumbWidth",
                    READ = sv_thumb_width, CONSTANT)]
        #[qproperty(i32, sv_thumb_height, cxx_name = "svThumbHeight",
                    READ = sv_thumb_height, CONSTANT)]
        #[qproperty(i32, sv_card_width, cxx_name = "svCardWidth",
                    READ = sv_card_width, CONSTANT)]
        #[qproperty(i32, sv_hero_width, cxx_name = "svHeroWidth",
                    READ = sv_hero_width, CONSTANT)]
        #[qproperty(i32, sv_hero_height, cxx_name = "svHeroHeight",
                    READ = sv_hero_height, CONSTANT)]
        #[qproperty(i32, sv_min_cols, cxx_name = "svMinCols",
                    READ = sv_min_cols, CONSTANT)]
        #[qproperty(i32, sv_max_cols, cxx_name = "svMaxCols",
                    READ = sv_max_cols, CONSTANT)]
        #[qproperty(i32, sv_hover_delay_ms, cxx_name = "svHoverDelayMs",
                    READ = sv_hover_delay_ms, CONSTANT)]
        // What changes in the showcase.
        #[qproperty(QList_QVariant, sv_chips, cxx_name = "svChips",
                    READ = sv_chips, NOTIFY = showcase_changed)]
        #[qproperty(QString, sv_count_text, cxx_name = "svCountText",
                    READ = sv_count_text, NOTIFY = showcase_changed)]
        #[qproperty(QString, sv_search_text, cxx_name = "svSearchText",
                    READ = sv_search_text, NOTIFY = showcase_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = showcase_changed)]
        #[qproperty(bool, autostart_on, cxx_name = "autostartOn",
                    READ = autostart_on, NOTIFY = showcase_changed)]
        #[qproperty(bool, rotate_on, cxx_name = "rotateOn",
                    READ = rotate_on, NOTIFY = showcase_changed)]
        #[qproperty(bool, sound_on, cxx_name = "soundOn",
                    READ = sound_on, NOTIFY = showcase_changed)]
        // The large card: {present, title, source, labels, event, starText,
        // starOn, status, poster}.
        #[qproperty(QMap_QString_QVariant, hero, READ = hero,
                    NOTIFY = hero_changed)]
        // What MUST play in the large card right now: `file://…` or "" (not
        // playing). The signal is the same as the card's: the preview and the card
        // show ONE clip, and they always change together.
        #[qproperty(QString, preview_source, cxx_name = "previewSource",
                    READ = preview_source, NOTIFY = preview_source_changed)]
        // The status chip: {text, background, border, color}.
        #[qproperty(QMap_QString_QVariant, state_chip, cxx_name = "stateChip",
                    READ = state_chip, NOTIFY = hero_changed)]

        // ── The "Steam backgrounds" section ─────────────────────────────
        #[qproperty(QString, bg_group_title, cxx_name = "bgGroupTitle",
                    READ = bg_group_title, CONSTANT)]
        #[qproperty(QString, bg_group_desc, cxx_name = "bgGroupDesc",
                    READ = bg_group_desc, CONSTANT)]
        #[qproperty(QString, bg_search_placeholder, cxx_name = "bgSearchPlaceholder",
                    READ = bg_search_placeholder, CONSTANT)]
        #[qproperty(QString, bg_audio_text, cxx_name = "bgAudioText",
                    READ = bg_audio_text, CONSTANT)]
        #[qproperty(QString, bg_apply_text, cxx_name = "bgApplyText",
                    READ = bg_apply_text, CONSTANT)]
        #[qproperty(QString, bg_applied_text, cxx_name = "bgAppliedText",
                    READ = bg_applied_text, CONSTANT)]
        #[qproperty(QString, bg_prev_text, cxx_name = "bgPrevText",
                    READ = bg_prev_text, CONSTANT)]
        #[qproperty(QString, bg_next_text, cxx_name = "bgNextText",
                    READ = bg_next_text, CONSTANT)]
        #[qproperty(i32, bg_thumb_width, cxx_name = "bgThumbWidth",
                    READ = bg_thumb_width, CONSTANT)]
        #[qproperty(i32, bg_thumb_height, cxx_name = "bgThumbHeight",
                    READ = bg_thumb_height, CONSTANT)]
        #[qproperty(i32, bg_card_width, cxx_name = "bgCardWidth",
                    READ = bg_card_width, CONSTANT)]
        #[qproperty(i32, bg_min_cols, cxx_name = "bgMinCols",
                    READ = bg_min_cols, CONSTANT)]
        #[qproperty(i32, bg_max_cols, cxx_name = "bgMaxCols",
                    READ = bg_max_cols, CONSTANT)]
        // Cards of the CURRENT page: [{id, icon, name, sub, subTooltip, animated,
        // thumbTooltip, thumb, applied}, …].
        #[qproperty(QList_QVariant, bg_cards, cxx_name = "bgCards",
                    READ = bg_cards, NOTIFY = backgrounds_changed)]
        // All profile backgrounds (not a grid page) — choosing the background for
        // the preview in the "Profile showcases" sub-tab:
        // [{id, name, url, thumb, equipped}, …], `url` is a static picture.
        #[qproperty(QList_QVariant, bg_choices, cxx_name = "bgChoices",
                    READ = bg_choices, NOTIFY = backgrounds_changed)]
        // Late addresses of the large viewer: background id → {source, video,
        // done} — for `RexonImageViewer.sources`.
        #[qproperty(QMap_QString_QVariant, bg_viewer_sources, cxx_name = "bgViewerSources",
                    READ = bg_viewer_sources, NOTIFY = backgrounds_changed)]
        // Background kind switch: [{key, label, checked}, …] — "All / Static /
        // Animated" with counters.
        #[qproperty(QList_QVariant, bg_kind_chips, cxx_name = "bgKindChips",
                    READ = bg_kind_chips, NOTIFY = backgrounds_changed)]
        // Why there are no animated backgrounds in the list and what to do about
        // it; empty means there is no reason, and the line with the "Retry" button
        // is hidden.
        #[qproperty(QString, bg_animated_note, cxx_name = "bgAnimatedNote",
                    READ = bg_animated_note, NOTIFY = backgrounds_changed)]
        #[qproperty(QString, bg_retry_text, cxx_name = "bgRetryText",
                    READ = bg_retry_text, CONSTANT)]
        #[qproperty(QString, bg_status_text, cxx_name = "bgStatusText",
                    READ = bg_status_text, NOTIFY = backgrounds_changed)]
        #[qproperty(QString, bg_search_text, cxx_name = "bgSearchText",
                    READ = bg_search_text, NOTIFY = backgrounds_changed)]
        #[qproperty(bool, bg_audio_on, cxx_name = "bgAudioOn",
                    READ = bg_audio_on, NOTIFY = backgrounds_changed)]
        // Pagination strip: {visible, prevEnabled, nextEnabled, text}.
        #[qproperty(QMap_QString_QVariant, pager, READ = pager,
                    NOTIFY = backgrounds_changed)]

        // ── Shared ──────────────────────────────────────────────────────
        #[qproperty(QString, header_found_text, cxx_name = "headerFoundText",
                    READ = header_found_text, NOTIFY = header_changed)]
        #[qproperty(QString, header_found_path, cxx_name = "headerFoundPath",
                    READ = header_found_path, NOTIFY = header_changed)]
        #[qproperty(QString, status_text, cxx_name = "statusText",
                    READ = status_text, NOTIFY = status_changed)]
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        #[qproperty(QString, accent_color, cxx_name = "accentColor",
                    READ = accent_color, CONSTANT)]
        #[qproperty(QString, applied_color, cxx_name = "appliedColor",
                    READ = applied_color, CONSTANT)]
        #[qproperty(QString, gold_color, cxx_name = "goldColor",
                    READ = gold_color, CONSTANT)]
        // Colours and sizes of the tab's look that are not in the skin palette:
        // the section group, the showcase card, the thesis inset, the main and the
        // green buttons, the toggle chip, the thumbnail backgrounds. All of this
        // already exists in the core (the widget tab is painted with the same set)
        // — the markup only takes them and has none of its own. The skin changed —
        // recompute.
        #[qproperty(QMap_QString_QVariant, styles, READ = styles,
                    NOTIFY = palette_changed)]
        /// After how many ms the status banner goes away. The number is one for the
        /// product (`ui_base::TOAST_TIMEOUT_MS`), the tab has none of its own.
        #[qproperty(i32, toast_timeout_ms, cxx_name = "toastTimeoutMs",
                    READ = toast_timeout_ms, CONSTANT)]
        type SteamModel = super::SteamModelRust;
    }

    // ── Signals ─────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        /// The skin was changed on the fly — recompute the palette.
        #[qsignal]
        #[cxx_name = "paletteChanged"]
        fn palette_changed(self: Pin<&mut SteamModel>);

        /// Everything that changes together with the composition of the splash
        /// showcase.
        #[qsignal]
        #[cxx_name = "showcaseChanged"]
        fn showcase_changed(self: Pin<&mut SteamModel>);

        /// The large card and the status chip: they always change as a pair.
        #[qsignal]
        #[cxx_name = "heroChanged"]
        fn hero_changed(self: Pin<&mut SteamModel>);

        /// What plays in the large card. A signal separate from `heroChanged` and
        /// only on a REAL change of the clip: by it the markup brings up the player
        /// at the first need, and the player pulls in Qt's multimedia layer.
        #[qsignal]
        #[cxx_name = "previewSourceChanged"]
        fn preview_source_changed(self: Pin<&mut SteamModel>);

        /// The composition of the background page, the status line and the
        /// pagination.
        #[qsignal]
        #[cxx_name = "backgroundsChanged"]
        fn backgrounds_changed(self: Pin<&mut SteamModel>);

        #[qsignal]
        #[cxx_name = "sectionChanged"]
        fn section_changed(self: Pin<&mut SteamModel>);

        #[qsignal]
        #[cxx_name = "headerChanged"]
        fn header_changed(self: Pin<&mut SteamModel>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut SteamModel>);

        /// The progress of adding a clip changed.
        #[qsignal]
        #[cxx_name = "importChanged"]
        fn import_changed(self: Pin<&mut SteamModel>);

        /// A clip was added or moved: the page re-reads the showcase
        /// (`actions.scan()` — the snapshot is the page host's).
        #[qsignal]
        #[cxx_name = "moviesOnDiskChanged"]
        fn movies_on_disk_changed(self: Pin<&mut SteamModel>);
    }

    // ── Inherited methods of the base class ─────────────────────────────
    extern "RustQt" {
        /// # Safety
        /// Requires a paired end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut SteamModel>);
        /// # Safety
        /// Paired with begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut SteamModel>);
    }

    // ── QAbstractListModel overrides ────────────────────────────────────
    // Without #[qinvokable]: an invokable in a subclass shadows the shortened
    // clones of the base's signatures, and rowCount() without arguments from
    // JS fails.
    extern "RustQt" {
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &SteamModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &SteamModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &SteamModel, index: &QModelIndex, role: i32) -> QVariant;
    }

    // ── Property reading ────────────────────────────────────────────────
    extern "RustQt" {
        #[cxx_name = "windowTitle"]
        fn window_title(self: &SteamModel) -> QString;
        #[cxx_name = "windowMinWidth"]
        fn window_min_width(self: &SteamModel) -> i32;
        #[cxx_name = "windowMinHeight"]
        fn window_min_height(self: &SteamModel) -> i32;
        #[cxx_name = "headerTitle"]
        fn header_title(self: &SteamModel) -> QString;
        #[cxx_name = "headerTitlePointSize"]
        fn header_title_point_size(self: &SteamModel) -> i32;
        #[cxx_name = "groupTitlePointSize"]
        fn group_title_point_size(self: &SteamModel) -> i32;
        fn segments(self: &SteamModel) -> QList_QVariant;
        fn section(self: &SteamModel) -> QString;

        #[cxx_name = "splashGroupTitle"]
        fn splash_group_title(self: &SteamModel) -> QString;
        #[cxx_name = "splashPitch"]
        fn splash_pitch(self: &SteamModel) -> QString;
        #[cxx_name = "svSearchPlaceholder"]
        fn sv_search_placeholder(self: &SteamModel) -> QString;
        #[cxx_name = "svPickText"]
        fn sv_pick_text(self: &SteamModel) -> QString;
        #[cxx_name = "svPickTooltip"]
        fn sv_pick_tooltip(self: &SteamModel) -> QString;
        #[cxx_name = "svPickDialogTitle"]
        fn sv_pick_dialog_title(self: &SteamModel) -> QString;
        #[cxx_name = "svPickFilter"]
        fn sv_pick_filter(self: &SteamModel) -> QString;
        #[cxx_name = "svAutostartText"]
        fn sv_autostart_text(self: &SteamModel) -> QString;
        #[cxx_name = "svAutostartTooltip"]
        fn sv_autostart_tooltip(self: &SteamModel) -> QString;
        #[cxx_name = "svRotateText"]
        fn sv_rotate_text(self: &SteamModel) -> QString;
        #[cxx_name = "svRotateTooltip"]
        fn sv_rotate_tooltip(self: &SteamModel) -> QString;
        #[cxx_name = "svSoundText"]
        fn sv_sound_text(self: &SteamModel) -> QString;
        #[cxx_name = "svTestText"]
        fn sv_test_text(self: &SteamModel) -> QString;
        #[cxx_name = "svTestTooltip"]
        fn sv_test_tooltip(self: &SteamModel) -> QString;
        #[cxx_name = "svStarTooltip"]
        fn sv_star_tooltip(self: &SteamModel) -> QString;
        #[cxx_name = "svCardStarTooltip"]
        fn sv_card_star_tooltip(self: &SteamModel) -> QString;
        #[cxx_name = "svTestPlayingText"]
        fn sv_test_playing_text(self: &SteamModel) -> QString;
        #[cxx_name = "svTestNoItemText"]
        fn sv_test_no_item_text(self: &SteamModel) -> QString;
        #[cxx_name = "svThumbPlaceholder"]
        fn sv_thumb_placeholder(self: &SteamModel) -> QString;
        #[cxx_name = "svImportCancelText"]
        fn sv_import_cancel_text(self: &SteamModel) -> QString;
        #[cxx_name = "svDeleteGlyph"]
        fn sv_delete_glyph(self: &SteamModel) -> QString;
        #[cxx_name = "svDeleteTooltip"]
        fn sv_delete_tooltip(self: &SteamModel) -> QString;
        #[cxx_name = "svDeleteTitle"]
        fn sv_delete_title(self: &SteamModel) -> QString;
        #[cxx_name = "svDeleteConfirmText"]
        fn sv_delete_confirm_text(self: &SteamModel) -> QString;
        #[cxx_name = "svDeleteKeepText"]
        fn sv_delete_keep_text(self: &SteamModel) -> QString;
        #[cxx_name = "svSteamChoiceGlyph"]
        fn sv_steam_choice_glyph(self: &SteamModel) -> QString;
        #[cxx_name = "svSteamChoiceTooltip"]
        fn sv_steam_choice_tooltip(self: &SteamModel) -> QString;
        #[cxx_name = "importBusy"]
        fn import_busy(self: &SteamModel) -> bool;
        #[cxx_name = "importText"]
        fn import_text(self: &SteamModel) -> QString;
        #[cxx_name = "svThumbWidth"]
        fn sv_thumb_width(self: &SteamModel) -> i32;
        #[cxx_name = "svThumbHeight"]
        fn sv_thumb_height(self: &SteamModel) -> i32;
        #[cxx_name = "svCardWidth"]
        fn sv_card_width(self: &SteamModel) -> i32;
        #[cxx_name = "svHeroWidth"]
        fn sv_hero_width(self: &SteamModel) -> i32;
        #[cxx_name = "svHeroHeight"]
        fn sv_hero_height(self: &SteamModel) -> i32;
        #[cxx_name = "svMinCols"]
        fn sv_min_cols(self: &SteamModel) -> i32;
        #[cxx_name = "svMaxCols"]
        fn sv_max_cols(self: &SteamModel) -> i32;
        #[cxx_name = "svHoverDelayMs"]
        fn sv_hover_delay_ms(self: &SteamModel) -> i32;
        #[cxx_name = "svChips"]
        fn sv_chips(self: &SteamModel) -> QList_QVariant;
        #[cxx_name = "svCountText"]
        fn sv_count_text(self: &SteamModel) -> QString;
        #[cxx_name = "svSearchText"]
        fn sv_search_text(self: &SteamModel) -> QString;
        fn count(self: &SteamModel) -> i32;
        #[cxx_name = "autostartOn"]
        fn autostart_on(self: &SteamModel) -> bool;
        #[cxx_name = "rotateOn"]
        fn rotate_on(self: &SteamModel) -> bool;
        #[cxx_name = "soundOn"]
        fn sound_on(self: &SteamModel) -> bool;
        fn hero(self: &SteamModel) -> QMap_QString_QVariant;
        #[cxx_name = "previewSource"]
        fn preview_source(self: &SteamModel) -> QString;
        #[cxx_name = "stateChip"]
        fn state_chip(self: &SteamModel) -> QMap_QString_QVariant;

        #[cxx_name = "bgGroupTitle"]
        fn bg_group_title(self: &SteamModel) -> QString;
        #[cxx_name = "bgGroupDesc"]
        fn bg_group_desc(self: &SteamModel) -> QString;
        #[cxx_name = "bgSearchPlaceholder"]
        fn bg_search_placeholder(self: &SteamModel) -> QString;
        #[cxx_name = "bgAudioText"]
        fn bg_audio_text(self: &SteamModel) -> QString;
        #[cxx_name = "bgApplyText"]
        fn bg_apply_text(self: &SteamModel) -> QString;
        #[cxx_name = "bgAppliedText"]
        fn bg_applied_text(self: &SteamModel) -> QString;
        #[cxx_name = "bgPrevText"]
        fn bg_prev_text(self: &SteamModel) -> QString;
        #[cxx_name = "bgNextText"]
        fn bg_next_text(self: &SteamModel) -> QString;
        #[cxx_name = "bgThumbWidth"]
        fn bg_thumb_width(self: &SteamModel) -> i32;
        #[cxx_name = "bgThumbHeight"]
        fn bg_thumb_height(self: &SteamModel) -> i32;
        #[cxx_name = "bgCardWidth"]
        fn bg_card_width(self: &SteamModel) -> i32;
        #[cxx_name = "bgMinCols"]
        fn bg_min_cols(self: &SteamModel) -> i32;
        #[cxx_name = "bgMaxCols"]
        fn bg_max_cols(self: &SteamModel) -> i32;
        #[cxx_name = "bgCards"]
        fn bg_cards(self: &SteamModel) -> QList_QVariant;
        #[cxx_name = "bgChoices"]
        fn bg_choices(self: &SteamModel) -> QList_QVariant;
        #[cxx_name = "bgViewerSources"]
        fn bg_viewer_sources(self: &SteamModel) -> QMap_QString_QVariant;
        #[cxx_name = "bgKindChips"]
        fn bg_kind_chips(self: &SteamModel) -> QList_QVariant;
        #[cxx_name = "bgAnimatedNote"]
        fn bg_animated_note(self: &SteamModel) -> QString;
        #[cxx_name = "bgRetryText"]
        fn bg_retry_text(self: &SteamModel) -> QString;
        #[cxx_name = "bgStatusText"]
        fn bg_status_text(self: &SteamModel) -> QString;
        #[cxx_name = "bgSearchText"]
        fn bg_search_text(self: &SteamModel) -> QString;
        #[cxx_name = "bgAudioOn"]
        fn bg_audio_on(self: &SteamModel) -> bool;
        fn pager(self: &SteamModel) -> QMap_QString_QVariant;

        #[cxx_name = "headerFoundText"]
        fn header_found_text(self: &SteamModel) -> QString;
        #[cxx_name = "headerFoundPath"]
        fn header_found_path(self: &SteamModel) -> QString;
        #[cxx_name = "statusText"]
        fn status_text(self: &SteamModel) -> QString;
        fn palette(self: &SteamModel) -> QMap_QString_QVariant;
        #[cxx_name = "accentColor"]
        fn accent_color(self: &SteamModel) -> QString;
        #[cxx_name = "appliedColor"]
        fn applied_color(self: &SteamModel) -> QString;
        #[cxx_name = "goldColor"]
        fn gold_color(self: &SteamModel) -> QString;
        fn styles(self: &SteamModel) -> QMap_QString_QVariant;
        #[cxx_name = "toastTimeoutMs"]
        fn toast_timeout_ms(self: &SteamModel) -> i32;
    }

    // ── API for QML and the page host ───────────────────────────────────
    //
    // Warning: not a single invokable with a number in its signature: cxx-qt
    // writes the type into the metaobject as `::std::int32_t`, there is no
    // metatype with that name, and a call through `QMetaObject.invokeMethod`
    // silently returns False.
    extern "RustQt" {
        /// Where Steam was found (empty — not found). Called by the page host.
        #[qinvokable]
        #[cxx_name = "setSteamRoot"]
        fn set_steam_root(self: Pin<&mut SteamModel>, root: &QString);

        /// Fill the splash showcase. `movies` is the list of scan records
        /// (`session_splash.scan_startup_movies`), `favorites` the starred paths.
        /// The splash side actions are still at the page host for now.
        #[qinvokable]
        #[cxx_name = "setMovies"]
        fn set_movies(
            self: Pin<&mut SteamModel>,
            movies: &QVariant,
            current_path: &QString,
            favorites: &QVariant,
        );

        /// The state of the splash toggles in one call.
        #[qinvokable]
        #[cxx_name = "setSplashState"]
        fn set_splash_state(
            self: Pin<&mut SteamModel>,
            enabled: bool,
            rotate: bool,
            sound: bool,
        );

        /// Switch the sub-tab (`splash` or `bg`).
        #[qinvokable]
        #[cxx_name = "setSection"]
        fn set_section(self: Pin<&mut SteamModel>, key: &QString);

        /// Search string of the splash showcase.
        #[qinvokable]
        #[cxx_name = "setSvSearch"]
        fn set_sv_search(self: Pin<&mut SteamModel>, text: &QString);

        /// The "where the splash comes from" filter chip.
        #[qinvokable]
        #[cxx_name = "setSvGroup"]
        fn set_sv_group(self: Pin<&mut SteamModel>, key: &QString);

        /// Choose the clip for login (a click on a card).
        #[qinvokable]
        #[cxx_name = "selectMovie"]
        fn select_movie(self: Pin<&mut SteamModel>, path: &QString);

        /// The card under the cursor ("" — the cursor left): the large card shows
        /// it until another one is chosen.
        #[qinvokable]
        #[cxx_name = "setHovered"]
        fn set_hovered(self: Pin<&mut SteamModel>, path: &QString);

        /// Set/clear a star. `favorites` is the current list from the host (the
        /// host saves the star).
        #[qinvokable]
        #[cxx_name = "setFavorites"]
        fn set_favorites(self: Pin<&mut SteamModel>, favorites: &QVariant);

        /// Path of the clip the large card is showing right now.
        #[qinvokable]
        #[cxx_name = "shownPath"]
        fn shown_path(self: &SteamModel) -> QString;

        /// The clip's source string (the tooltip of the card badge).
        #[qinvokable]
        #[cxx_name = "sourceTextFor"]
        fn source_text_for(self: &SteamModel, path: &QString) -> QString;

        // ── Live clip preview ───────────────────────────────────────────

        /// Whether the large card is visible to the user. Decided by the markup:
        /// its own sub-tab, its own window tab, the window on screen. `false` —
        /// `previewSource` becomes empty, and the player stops.
        #[qinvokable]
        #[cxx_name = "setPreviewShown"]
        fn set_preview_shown(self: Pin<&mut SteamModel>, on: bool);

        /// The player could not handle the clip it was playing (a broken file, an
        /// unknown codec). We do not try it again: otherwise the once-a-second poll
        /// would open it again and again. Which one exactly the model knows itself,
        /// the markup does not need to retell it.
        #[qinvokable]
        #[cxx_name = "markPreviewBroken"]
        fn mark_preview_broken(self: Pin<&mut SteamModel>);

        /// The clip went full screen — the preview freezes: there must not be two
        /// copies of the same video at once.
        #[qinvokable]
        #[cxx_name = "pausePreview"]
        fn pause_preview(self: Pin<&mut SteamModel>);

        /// We are back in the window — the full-screen show ended, remove the
        /// pause.
        #[qinvokable]
        #[cxx_name = "resumePreview"]
        fn resume_preview(self: Pin<&mut SteamModel>);

        /// The once-a-second poll of the situation: removes an OVERDUE pause.
        /// Needed for the case when the focus did not come back at all after the
        /// full-screen show — otherwise the preview would freeze forever.
        #[qinvokable]
        #[cxx_name = "previewTick"]
        fn preview_tick(self: Pin<&mut SteamModel>);

        /// The page's status line (the outcome of the full-screen show).
        #[qinvokable]
        #[cxx_name = "setStatus"]
        fn set_status_text(self: Pin<&mut SteamModel>, text: &QString);

        /// Add the person's own clip from `path` in the background (check,
        /// copy or convert, choose). `false` — one is already being added.
        #[qinvokable]
        #[cxx_name = "importMovie"]
        fn import_movie(self: Pin<&mut SteamModel>, path: &QString) -> bool;

        /// Keep a copy of the OLD selection `path` (a path to someone else's
        /// file) in the background — without conversion and installation.
        /// `false` — not started (the tasks are not up yet, or a clip is being
        /// added): the page asks again later.
        #[qinvokable]
        #[cxx_name = "migrateMovie"]
        fn migrate_movie(self: Pin<&mut SteamModel>, path: &QString) -> bool;

        /// Stop adding the clip.
        #[qinvokable]
        #[cxx_name = "cancelImport"]
        fn cancel_import(self: Pin<&mut SteamModel>);

        /// The answer of the "Show at login" toggle: `on` is what was asked,
        /// `reason` the answer of `actions.setAutostart` ("" — it worked). A refusal
        /// is shown in the status line in simple words from the core.
        #[qinvokable]
        #[cxx_name = "autostartAnswered"]
        fn autostart_answered(self: Pin<&mut SteamModel>, on: bool, reason: &QString);

        /// The "A new clip at every login" toggle was pressed: if there is nothing
        /// to rotate, the page says so (the threshold is in the core).
        #[qinvokable]
        #[cxx_name = "rotationToggled"]
        fn rotation_toggled(self: Pin<&mut SteamModel>, on: bool);

        /// Whether the compositor shows the `window` window (in QML —
        /// `Window.window`). A null pointer means "shows": there is no window yet,
        /// and the preview must not freeze because of that.
        ///
        /// # Safety
        /// `window` must be a live QWindow.
        #[qinvokable]
        #[cxx_name = "windowExposed"]
        unsafe fn window_exposed(self: &SteamModel, window: *mut QObject) -> bool;

        // ── Profile backgrounds ─────────────────────────────────────────

        /// The width of the background strip changed — recompute the columns and
        /// the page.
        #[qinvokable]
        #[cxx_name = "setBgViewportWidth"]
        fn set_bg_viewport_width(self: Pin<&mut SteamModel>, width: &QString);

        /// Search string of the background grid.
        #[qinvokable]
        #[cxx_name = "setBgSearch"]
        fn set_bg_search(self: Pin<&mut SteamModel>, text: &QString);

        /// Background kind filter: "" — all, `static`, `animated`.
        #[qinvokable]
        #[cxx_name = "setBgKind"]
        fn set_bg_kind(self: Pin<&mut SteamModel>, key: &QString);

        /// Ask Steam for the background list again (the "Retry" button of the
        /// "animated ones are not shown" line).
        #[qinvokable]
        #[cxx_name = "reloadBackgrounds"]
        fn reload_backgrounds(self: Pin<&mut SteamModel>);

        /// Video wallpaper sound.
        #[qinvokable]
        #[cxx_name = "setBgAudio"]
        fn set_bg_audio(self: Pin<&mut SteamModel>, on: bool);

        /// The previous grid page.
        #[qinvokable]
        #[cxx_name = "bgPrevPage"]
        fn bg_prev_page(self: Pin<&mut SteamModel>);

        /// The next grid page.
        #[qinvokable]
        #[cxx_name = "bgNextPage"]
        fn bg_next_page(self: Pin<&mut SteamModel>);

        /// The list for the large viewer: ALL selected backgrounds, not one grid
        /// page — paging goes over the whole list.
        #[qinvokable]
        #[cxx_name = "bgViewerItems"]
        fn bg_viewer_items(self: &SteamModel) -> QList_QVariant;

        /// The viewer needs a whole background — obtain it (once).
        #[qinvokable]
        #[cxx_name = "requestBgFull"]
        fn request_bg_full(self: Pin<&mut SteamModel>, item_id: &QString);

        /// The list for the large splash viewer: the shown clips.
        #[qinvokable]
        #[cxx_name = "svViewerItems"]
        fn sv_viewer_items(self: &SteamModel) -> QList_QVariant;

        /// Start applying a background as a wallpaper. `false` means we are already
        /// applying.
        #[qinvokable]
        #[cxx_name = "applyBackground"]
        fn apply_background(self: Pin<&mut SteamModel>, item_id: &QString) -> bool;

        // ── Background tasks ────────────────────────────────────────────

        /// Bring up the tab's tasks: caches from disk and the network. Called once,
        /// after the Steam root is known.
        #[qinvokable]
        fn start(self: Pin<&mut SteamModel>, webapi_token: &QString);

        /// Take everything the background tasks have accumulated. Called by the
        /// page's timer — this is the seam between the task runtime and the
        /// interface thread.
        #[qinvokable]
        fn pump(self: Pin<&mut SteamModel>);

        /// Cancel the tasks and wait for them — a port of `closeEvent`.
        #[qinvokable]
        fn shutdown(self: Pin<&mut SteamModel>);

        /// Role number by name (-1 if there is no such role) — checks without a
        /// delegate.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &SteamModel, name: &QString) -> i32;

        /// A row's role value by the clip path — checks without a delegate.
        #[qinvokable]
        #[cxx_name = "roleValueOf"]
        fn role_value_of(self: &SteamModel, path: &QString, name: &QString) -> QVariant;

        /// Width of the status banner for this page width and this width of the
        /// text on one line. Computed by the core (`ui_base::toast_width`), the
        /// paddings and the cap are its too. The markup calls it directly from a
        /// binding.
        #[qinvokable]
        #[cxx_name = "toastWidth"]
        fn toast_width(self: &SteamModel, page_width: i32, text_width: i32) -> i32;
    }

    // The trait declaration must be IN THE BRIDGE, otherwise
    // `impl cxx_qt::Initialize` outside does not find the constructor: cxx-qt
    // emits it from this line.
    impl cxx_qt::Initialize for SteamModel {}
    // The skin change subscription goes through `CxxQtThread::queue`, and the
    // thread trait must be declared IN THE BRIDGE.
    impl cxx_qt::Threading for SteamModel {}
}

use core::pin::Pin;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QList, QMap, QMapPair_QString_QVariant, QModelIndex, QString, QVariant,
};
use serde_json::Value;

use rexon_core::i18n::tr;
use rexon_core::qt_steam as qs;
use rexon_core::session_splash::Splash;
use rexon_core::splash_exec::LiveSplashIo;
use rexon_core::steam_profile_items::ProfileBackgroundItem;
use rexon_core::ui_base;

use crate::steam_workers::{
    self, InventoryRequest, SteamEnv, SteamEvent, SteamTasks, WallpaperApply,
};

// ============================================================================
// Roles of the splash showcase model
// ============================================================================

/// Roles start at Qt::UserRole + 1 (Qt::UserRole == 256).
const ROLE_BASE: i32 = 256 + 1;

/// Role names in number order — the single source of truth for roleNames,
/// data() and roleNumber().
const ROLE_NAMES: [&str; 12] = [
    "path",
    "title",
    "badge",
    "badgeBackground",
    "badgeColor",
    "poster",
    "starGlyph",
    "starColor",
    "starOn",
    "selected",
    // The person's own clip (in our folders) — the card shows «🗑».
    "deletable",
    // Steam itself plays this clip at startup — the card shows «🎮».
    "steamChosen",
];

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

/// A path on disk → an address for `Image.source` and `MediaPlayer.source`.
/// An empty path stays empty.
///
/// Warning: special characters are percent-encoded, and this is not
/// embellishment. Clip paths come from Steam libraries and from the "Own
/// file…" window, that is, they are not written by the program: QUrl will
/// still survive a space in the name, but `#` cuts the path off at the hash
/// — the file then simply does not open, and the preview silently stays a
/// static frame. The UTF-8 byte representation is encoded, so Russian file
/// names pass just the same.
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

/// Whether the active skin is dark — the core's shared answer (`ui_base`),
/// not our own copy of the criterion: the "Extensions" page asks the same
/// question.
fn active_is_dark() -> bool {
    ui_base::active_skin_is_dark()
}

/// A string field of a clip record.
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// A `QVariant` with a string list → `Vec<String>`. Comes from QML/the
/// host.
fn strings_from(v: &QVariant) -> Vec<String> {
    v.value::<QList<QVariant>>()
        .map(|list| {
            list.iter()
                .filter_map(|x| x.value::<QString>())
                .map(|q| q.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// A `QVariant` with a list of maps → clip records as JSON.
///
/// Scan records arrive from the page host as a QVariantList of QVariantMap;
/// here they are converted into the form in which the core reads them.
///
/// Warning: the conversion is done by the crate's SHARED converter, not by
/// type-by-type parsing in place. There used to be our own parsing here
/// that determined the type by trying `QVariant::value<T>`, starting with a
/// string — and Qt's `canConvert` is permissive, ANY number "converts" to a
/// string. `appid` arrived in the core as the string `"2861720"`,
/// `movie_appid` read it as 0, and the inventory item markers were NEVER
/// found: the card badge and the filter chip showed "Points Shop" instead of
/// the sale name, and the tooltip a generic phrase instead of "Video splash
/// from …". Caught by a pixel-by-pixel comparison with the original on
/// 01.09.2026; `qvalue::variant_to_json` warns exactly about this trap,
/// where the type is determined by Qt itself (`QJsonDocument::fromVariant`).
fn movies_from(v: &QVariant) -> Vec<Value> {
    match crate::qvalue::variant_to_json(v) {
        Value::Array(items) => items
            .into_iter()
            .filter(|item| item.is_object())
            .collect(),
        _ => Vec::new(),
    }
}

// ============================================================================
// The wallpaper engine from the page host's side
// ============================================================================

// ============================================================================
// Rust state of the page
// ============================================================================

/// State of the "Steam" tab.
#[derive(Default)]
pub struct SteamModelRust {
    /// Entry number in the shared skin change registry (`crate::skin`).
    skin_watch: u64,
    env: SteamEnv,
    tasks: Option<Arc<SteamTasks>>,

    steam_root: String,
    sid: String,
    section: String,
    status: String,

    // ── Splash showcase ──
    movies: Vec<Value>,
    favorites: Vec<String>,
    meta: qs::MetaMap,
    current_path: String,
    hovered_path: String,
    sv_search: String,
    sv_group: String,
    /// Numbers of the shown clips in `movies`.
    visible: Vec<usize>,
    /// Clip path → a ready preview frame.
    posters: BTreeMap<String, String>,
    autostart_on: bool,
    rotate_on: bool,
    sound_on: bool,
    /// A clip is being added; the current stage line.
    import_busy: bool,
    import_text: String,

    // ── Live clip preview in the large card ──
    /// The large card is in view (set by the markup).
    preview_shown: bool,
    /// Clips the player could not handle — we do not try them a second time.
    preview_broken: BTreeSet<String>,
    /// Until this moment the preview is frozen (full-screen show).
    preview_paused_until: Option<Instant>,
    /// What plays now according to the model's decision. A cache, not a second
    /// opinion: the decision is computed by `preview_path`, and we store it so
    /// that the once-a-second poll stays silent while nothing has changed.
    preview_playing: String,

    // ── Background grid ──
    bg_all: Vec<ProfileBackgroundItem>,
    bg_search: String,
    /// Kind filter: all / static / animated. Lives until the program closes:
    /// the tab has no saved settings of its own (the video wallpaper sound and
    /// the search are reset at launch in exactly the same way).
    bg_kind: String,
    bg_page: u32,
    bg_cols: u32,
    bg_audio: bool,
    /// Background id → a ready thumbnail.
    thumbs: BTreeMap<String, String>,
    /// Animated background id → the downloaded clip (for the large viewer).
    movies_full: BTreeMap<String, String>,
    /// Backgrounds the large viewer has already gone for: we do not ask a
    /// second time, and a failure does not leave the viewer in an eternal
    /// "loading".
    full_asked: BTreeSet<String>,
    full_done: BTreeSet<String>,
    applied_id: String,
    /// The background being applied (for the status line on completion).
    pending_apply: Option<ProfileBackgroundItem>,
    /// The list is already shown from the disk cache — do not overwrite it with
    /// a network error.
    cache_shown: bool,
    /// Why the background section has nothing to show (or why it is not ready
    /// yet): "Loading the inventory", "Steam not found", "no userdata",
    /// "updating from the cache", "the inventory did not load", "there are no
    /// backgrounds". Empty means there is no reason, and the label is computed
    /// from the number of backgrounds.
    ///
    /// Warning: this is the label of the section ITSELF, not a pop-up message.
    /// In the original (`rexon/qt_steam/window.py`) ALL the listed strings are
    /// written into `_bg_status` — the label standing in the same place as
    /// "Backgrounds found: 0". Here they went into the toast at the bottom left
    /// over the neighbouring "Video splash" section, where the user has no idea
    /// what they refer to, and disappeared after three seconds together with
    /// the toast, while the section lied all that time with the count
    /// "Backgrounds found: 0".
    bg_notice: String,
    /// How the request for owned items ended: whether there are animated
    /// backgrounds in the list at all depends on it. The string for the user is
    /// assembled by the core.
    bg_animated_gap: qs::AnimatedGap,
    /// The background equipped in the profile — remembered for a repeated list
    /// request.
    bg_equipped: Option<ProfileBackgroundItem>,
}

impl Drop for SteamModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
        if let Some(t) = self.tasks.take() {
            t.shutdown();
        }
    }
}

impl SteamModelRust {
    /// Recompute the shown clips by the current search and filter.
    fn recompute_showcase(&mut self) {
        self.sv_group =
            qs::normalize_group_key(&self.sv_group, &self.movies, &self.favorites, &self.meta);
        let needle = self.sv_search.trim().to_lowercase();
        self.visible = self
            .movies
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                qs::sv_matches(m, &needle, &self.sv_group, &self.favorites, &self.meta)
            })
            .map(|(i, _)| i)
            .collect();
    }

    /// Clips selected by the search and the filter.
    fn shown_movies(&self) -> Vec<&Value> {
        self.visible.iter().filter_map(|i| self.movies.get(*i)).collect()
    }

    /// What the large card shows: the hovered or the chosen one.
    fn shown_item(&self) -> Option<&Value> {
        if !self.hovered_path.is_empty() {
            if let Some(it) = self.movies.iter().find(|m| s(m, "path") == self.hovered_path) {
                return Some(it);
            }
        }
        self.current_item()
    }

    /// What MUST play in the large card right now ("" — nothing).
    ///
    /// A port of the original's `_sv_preview_target` + `_sv_preview_allowed` as
    /// one decision: the markup only has to say whether the card is visible. We
    /// play exactly the clip the card shows — that is, the hovered one, and if
    /// there is no hover, the chosen one.
    fn preview_path(&self) -> &str {
        if !self.preview_shown || self.section != qs::SEG_SPLASH {
            return "";
        }
        if matches!(self.preview_paused_until, Some(t) if Instant::now() < t) {
            return "";
        }
        let Some(item) = self.shown_item() else {
            return "";
        };
        let path = s(item, "path");
        // A broken clip and a vanished file are no reason to poke the player.
        if path.is_empty()
            || self.preview_broken.contains(path)
            || !std::path::Path::new(path).is_file()
        {
            return "";
        }
        path
    }

    /// The clip that will play at the next login.
    fn current_item(&self) -> Option<&Value> {
        self.movies.iter().find(|m| s(m, "path") == self.current_path)
    }

    /// Backgrounds selected by the search.
    fn bg_filtered(&self) -> Vec<&ProfileBackgroundItem> {
        qs::bg_filtered_by(&self.bg_all, &self.bg_search.trim().to_lowercase(), &self.bg_kind)
    }

    /// Pagination strip by the current state.
    fn pager_view(&self) -> qs::PagerView {
        qs::bg_pager(self.bg_filtered().len(), self.bg_page, self.cols())
    }

    /// Columns in the background grid right now (while the width is unknown —
    /// the minimum).
    fn cols(&self) -> u32 {
        if self.bg_cols == 0 {
            qs::BG_MIN_COLS
        } else {
            self.bg_cols
        }
    }

    /// The tab's tasks (brought up lazily: until `start` there are none).
    fn tasks(&self) -> Option<&Arc<SteamTasks>> {
        self.tasks.as_ref()
    }

    /// Order thumbnails ONLY for the cards of the current page.
    fn request_page_thumbs(&self) {
        let Some(tasks) = self.tasks() else { return };
        let p = self.pager_view();
        let filtered = self.bg_filtered();
        let jobs: Vec<(String, String)> = filtered[p.start..p.end]
            .iter()
            .filter(|it| !self.thumbs.contains_key(&it.id))
            .map(|it| {
                let card = qs::bg_card(it);
                (card.id, card.preview_url)
            })
            .collect();
        tasks.start_thumbs(jobs);
    }

    /// Order preview frames for clips that do not have them yet.
    fn request_posters(&self) {
        let Some(tasks) = self.tasks() else { return };
        let jobs: Vec<String> = self
            .shown_movies()
            .iter()
            .map(|m| s(m, "path").to_string())
            .filter(|p| !p.is_empty() && !self.posters.contains_key(p))
            .collect();
        tasks.start_posters(jobs);
    }
}

// ============================================================================
// Constructor
// ============================================================================

impl cxx_qt::Initialize for qobject::SteamModel {
    fn initialize(mut self: Pin<&mut Self>) {
        let env = SteamEnv::from_process();
        {
            let mut me = self.as_mut().rust_mut();
            me.env = env;
            me.section = qs::SEG_SPLASH.to_string();
            me.sound_on = true;
            me.bg_audio = true;
            me.bg_cols = qs::BG_MIN_COLS;
        }
        // The page finds out where Steam is ITSELF rather than waiting for the
        // host.
        //
        // Warning: this is not decoration but a race fix. Under the Python host the
        // root came from outside (`qml_window.py:_tell_steam_root` →
        // `setSteamRoot`), but the page starts the background tasks itself, in
        // `Component.onCompleted` — that is, BEFORE the host manages to say
        // anything. The first run of the tasks inevitably saw an empty root and
        // stopped at "Steam not found", and the host then took the tasks down and
        // brought them up again. Under the Rust host there was nobody to say it at
        // all: the profile backgrounds section stayed empty forever.
        //
        // Here the root is known BEFORE the first task: `initialize` runs when the
        // model is created, before `Component.onCompleted`. The search rules are
        // not duplicated — the core knows them
        // (`session_splash::Splash::steam_root`: candidates, `realpath`, checking
        // `config/`), and the executor is the same live one as the action
        // object's. The `setSteamRoot` setter stays: the Python host still calls
        // it, and the value will be the same.
        let root = {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
            let mut io = LiveSplashIo::new(&home, rexon_core::installer::PYTHON_FALLBACK_EXE);
            Splash::new(&mut io, &home).steam_root().unwrap_or_default()
        };
        self.as_mut().rust_mut().steam_root = root;

        // The skin was changed on the fly — recompute the palette. The push comes
        // from the shared registry (`crate::skin`), not from here.
        let watch = crate::skin::watch(self.qt_thread(), |page| page.palette_changed());
        self.as_mut().rust_mut().skin_watch = watch;
        // Warning: the initial text is the label of the background section ITSELF,
        // not a toast. In the original this is exactly the label's text at its
        // creation (`rexon/qt_steam/window.py`,
        // `QLabel("⏳ Загружаю инвентарь Steam…")`). As a toast it popped up at the
        // bottom left over the neighbouring "Video splash" section, where the user
        // has no idea what it refers to, and went away after three seconds, while
        // the background section meanwhile lied "Backgrounds found: 0" without
        // having loaded anything yet.
        self.as_mut().set_bg_notice(&qs::bg_loading_text());
    }
}

// ============================================================================
// Properties: fixed labels
// ============================================================================

impl qobject::SteamModel {
    pub fn window_title(&self) -> QString {
        QString::from(qs::WINDOW_TITLE)
    }
    pub fn window_min_width(&self) -> i32 {
        qs::WINDOW_MIN_SIZE.0 as i32
    }
    pub fn window_min_height(&self) -> i32 {
        qs::WINDOW_MIN_SIZE.1 as i32
    }
    pub fn header_title(&self) -> QString {
        QString::from(qs::HEADER_TITLE)
    }
    pub fn header_title_point_size(&self) -> i32 {
        qs::HEADER_TITLE_POINT_SIZE
    }
    pub fn group_title_point_size(&self) -> i32 {
        qs::GROUP_TITLE_POINT_SIZE
    }

    pub fn segments(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (key, label) in qs::segments() {
            out.append(QVariant::from(&vmap(vec![
                ("key", qstr(key)),
                ("label", qstr(&label)),
                ("checked", QVariant::from(&(key == self.rust().section))),
            ])));
        }
        out
    }

    pub fn section(&self) -> QString {
        QString::from(&self.rust().section)
    }

    pub fn splash_group_title(&self) -> QString {
        QString::from(&qs::splash_group_title())
    }
    pub fn splash_pitch(&self) -> QString {
        QString::from(&qs::splash_pitch())
    }
    pub fn sv_search_placeholder(&self) -> QString {
        QString::from(&qs::sv_search_placeholder())
    }
    pub fn sv_pick_text(&self) -> QString {
        QString::from(&qs::sv_pick_text())
    }
    pub fn sv_pick_tooltip(&self) -> QString {
        QString::from(&qs::sv_pick_tooltip())
    }
    pub fn sv_pick_dialog_title(&self) -> QString {
        QString::from(&qs::sv_pick_dialog_title())
    }
    pub fn sv_pick_filter(&self) -> QString {
        QString::from(&qs::sv_pick_filter())
    }
    pub fn sv_autostart_text(&self) -> QString {
        QString::from(&qs::sv_autostart_text())
    }
    pub fn sv_autostart_tooltip(&self) -> QString {
        QString::from(&qs::sv_autostart_tooltip())
    }
    pub fn sv_rotate_text(&self) -> QString {
        QString::from(&qs::sv_rotate_text())
    }
    pub fn sv_rotate_tooltip(&self) -> QString {
        QString::from(&qs::sv_rotate_tooltip())
    }
    pub fn sv_sound_text(&self) -> QString {
        QString::from(&qs::sv_sound_text())
    }
    pub fn sv_test_text(&self) -> QString {
        QString::from(&qs::sv_test_text())
    }
    pub fn sv_test_tooltip(&self) -> QString {
        QString::from(&qs::sv_test_tooltip())
    }
    pub fn sv_star_tooltip(&self) -> QString {
        QString::from(&qs::sv_star_tooltip())
    }
    pub fn sv_card_star_tooltip(&self) -> QString {
        QString::from(&qs::sv_card_star_tooltip())
    }
    pub fn sv_test_playing_text(&self) -> QString {
        QString::from(&qs::sv_test_playing())
    }
    pub fn sv_test_no_item_text(&self) -> QString {
        QString::from(&tr(qs::SV_TEST_NO_ITEM))
    }
    pub fn sv_thumb_placeholder(&self) -> QString {
        QString::from(qs::SV_THUMB_PLACEHOLDER)
    }
    pub fn sv_import_cancel_text(&self) -> QString {
        QString::from(&qs::sv_import_cancel_text())
    }
    pub fn sv_delete_glyph(&self) -> QString {
        QString::from(qs::SV_DELETE_GLYPH)
    }
    pub fn sv_delete_tooltip(&self) -> QString {
        QString::from(&qs::sv_delete_tooltip())
    }
    pub fn sv_delete_title(&self) -> QString {
        QString::from(&qs::sv_delete_title())
    }
    pub fn sv_delete_confirm_text(&self) -> QString {
        QString::from(&qs::sv_delete_confirm_text())
    }
    pub fn sv_delete_keep_text(&self) -> QString {
        QString::from(&qs::sv_delete_keep_text())
    }
    pub fn sv_steam_choice_glyph(&self) -> QString {
        QString::from(qs::SV_STEAM_CHOICE_GLYPH)
    }
    pub fn sv_steam_choice_tooltip(&self) -> QString {
        QString::from(&qs::sv_steam_choice_tooltip())
    }
    pub fn import_busy(&self) -> bool {
        self.rust().import_busy
    }
    pub fn import_text(&self) -> QString {
        QString::from(&self.rust().import_text)
    }
    pub fn sv_thumb_width(&self) -> i32 {
        qs::SV_THUMB_W
    }
    pub fn sv_thumb_height(&self) -> i32 {
        qs::SV_THUMB_H
    }
    pub fn sv_card_width(&self) -> i32 {
        qs::SV_CARD_W
    }
    pub fn sv_hero_width(&self) -> i32 {
        qs::SV_HERO_W
    }
    pub fn sv_hero_height(&self) -> i32 {
        qs::SV_HERO_H
    }
    pub fn sv_min_cols(&self) -> i32 {
        qs::SV_MIN_COLS as i32
    }
    pub fn sv_max_cols(&self) -> i32 {
        qs::SV_MAX_COLS as i32
    }
    pub fn sv_hover_delay_ms(&self) -> i32 {
        qs::SV_HOVER_DELAY_MS as i32
    }

    pub fn bg_group_title(&self) -> QString {
        QString::from(&qs::bg_group_title())
    }
    pub fn bg_group_desc(&self) -> QString {
        QString::from(&qs::bg_group_desc())
    }
    pub fn bg_search_placeholder(&self) -> QString {
        QString::from(&qs::bg_search_placeholder())
    }
    pub fn bg_audio_text(&self) -> QString {
        QString::from(&qs::bg_audio_text())
    }
    pub fn bg_apply_text(&self) -> QString {
        QString::from(&qs::bg_apply_text())
    }
    pub fn bg_applied_text(&self) -> QString {
        QString::from(&qs::bg_applied_label())
    }
    pub fn bg_prev_text(&self) -> QString {
        QString::from(&qs::bg_prev_text())
    }
    pub fn bg_next_text(&self) -> QString {
        QString::from(&qs::bg_next_text())
    }
    pub fn bg_thumb_width(&self) -> i32 {
        qs::BG_THUMB_W
    }
    pub fn bg_thumb_height(&self) -> i32 {
        qs::BG_THUMB_H
    }
    pub fn bg_card_width(&self) -> i32 {
        qs::BG_CARD_W
    }
    pub fn bg_min_cols(&self) -> i32 {
        qs::BG_MIN_COLS as i32
    }
    pub fn bg_max_cols(&self) -> i32 {
        qs::BG_MAX_COLS as i32
    }

    pub fn accent_color(&self) -> QString {
        QString::from(&qs::hex(qs::ACCENT_RGB))
    }
    pub fn applied_color(&self) -> QString {
        QString::from(&qs::hex(qs::APPLIED_RGB))
    }
    pub fn gold_color(&self) -> QString {
        QString::from(&qs::hex(qs::GOLD_RGB))
    }

    /// The tab's look that is not in the skin palette.
    ///
    /// Warning: EVERY colour goes through [`ui_base::css_color_to_qml`]. In the
    /// core they are written the way widget QSS understands them
    /// (`rgba(r,g,b,a)`), while QML reads such notation as an invalid colour and
    /// draws it BLACK without a word. Handing them over here "as is" is not
    /// allowed.
    pub fn styles(&self) -> QMap<QMapPair_QString_QVariant> {
        let dark = active_is_dark();
        let col = |css: &str| qstr(&ui_base::css_color_to_qml(css));

        let (group_bg, group_border) = qs::group_colors(dark);
        let group = vmap(vec![
            ("background", col(&group_bg)),
            ("border", col(&group_border)),
            ("radius", QVariant::from(&qs::GROUP_RADIUS)),
            ("marginH", QVariant::from(&qs::GROUP_MARGIN_H)),
            ("marginV", QVariant::from(&qs::GROUP_MARGIN_V)),
            ("spacing", QVariant::from(&qs::GROUP_SPACING)),
        ]);

        let (card_bg, card_border, card_hover) = qs::card_colors(false, dark);
        let (sel_bg, sel_border, sel_hover) = qs::card_colors(true, dark);
        let card = vmap(vec![
            ("background", col(&card_bg)),
            ("border", col(&card_border)),
            ("hover", col(&card_hover)),
            ("selectedBackground", col(&sel_bg)),
            ("selectedBorder", col(&sel_border)),
            ("selectedHover", col(&sel_hover)),
            ("radius", QVariant::from(&qs::CARD_RADIUS)),
            ("margin", QVariant::from(&qs::CARD_MARGIN)),
            ("spacing", QVariant::from(&qs::CARD_SPACING)),
            ("gap", QVariant::from(&qs::CARD_GAP)),
        ]);

        let (pitch_bg, pitch_bar) = qs::pitch_colors();
        let pitch = vmap(vec![
            ("background", col(&pitch_bg)),
            ("bar", col(&pitch_bar)),
            ("barWidth", QVariant::from(&qs::PITCH_BAR_W)),
            ("radius", QVariant::from(&qs::PITCH_RADIUS)),
            ("marginH", QVariant::from(&qs::PITCH_MARGIN_H)),
            ("marginV", QVariant::from(&qs::PITCH_MARGIN_V)),
        ]);

        let (pb_bg, pb_hover, pb_disabled, pb_fg) = qs::primary_btn_colors();
        let primary = vmap(vec![
            ("background", col(&pb_bg)),
            ("hover", col(&pb_hover)),
            ("disabled", col(&pb_disabled)),
            ("color", col(&pb_fg)),
            ("radius", QVariant::from(&qs::BTN_RADIUS)),
            ("bold", QVariant::from(&true)),
        ]);

        let (ab_bg, ab_hover, ab_fg) = qs::applied_btn_colors();
        let applied = vmap(vec![
            ("background", col(&ab_bg)),
            ("hover", col(&ab_hover)),
            ("disabled", col(&pb_disabled)),
            ("color", col(&ab_fg)),
            ("radius", QVariant::from(&qs::BTN_RADIUS)),
            ("bold", QVariant::from(&true)),
        ]);

        let (chip_border, chip_on_bg, chip_on_border, chip_hover) = qs::switch_chip_colors();
        let chip = vmap(vec![
            ("border", col(&chip_border)),
            ("checkedBackground", col(&chip_on_bg)),
            ("checkedBorder", col(&chip_on_border)),
            ("hoverBackground", col(&chip_hover)),
            ("radius", QVariant::from(&qs::BTN_RADIUS)),
        ]);

        let thumb = vmap(vec![
            ("svBackground", col(qs::SV_THUMB_BG)),
            ("bgBackground", col(qs::BG_THUMB_BG)),
            ("heroBackground", col(qs::HERO_THUMB_BG)),
            ("radius", QVariant::from(&qs::THUMB_RADIUS)),
            ("heroRadius", QVariant::from(&qs::HERO_RADIUS)),
        ]);

        // The pop-up status banner: colours and paddings — the same door as for
        // "Settings" and the widget banner (`crate::toast`).
        let toast = crate::toast::style_map(dark);

        vmap(vec![
            ("group", QVariant::from(&group)),
            ("card", QVariant::from(&card)),
            ("pitch", QVariant::from(&pitch)),
            ("primaryButton", QVariant::from(&primary)),
            ("appliedButton", QVariant::from(&applied)),
            ("chip", QVariant::from(&chip)),
            ("thumb", QVariant::from(&thumb)),
            ("toast", QVariant::from(&toast)),
            ("svTitleElideWidth", QVariant::from(&qs::SV_TITLE_ELIDE_W)),
        ])
    }

    /// After how many ms the status banner goes away.
    pub fn toast_timeout_ms(&self) -> i32 {
        crate::toast::timeout_ms()
    }

    /// Width of the status banner for text of this width.
    pub fn toast_width(&self, page_width: i32, text_width: i32) -> i32 {
        crate::toast::width(page_width, text_width)
    }

    /// Palette of the active skin (steam_dark by default, rule #17). The QML
    /// tab's look is taken from here rather than written anew.
    pub fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        // Background dimming under a modal window — one for the product.
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        // The slider track (`track`) comes with the shared palette
        // (`ui_base::qml_palette`).
        out
    }
}

// ============================================================================
// Properties: what changes
// ============================================================================

impl qobject::SteamModel {
    pub fn header_found_text(&self) -> QString {
        let root = self.rust().steam_root.clone();
        let (text, _) = qs::header_found_text(if root.is_empty() { None } else { Some(&root) });
        QString::from(&text)
    }

    pub fn header_found_path(&self) -> QString {
        let root = self.rust().steam_root.clone();
        let (_, path) = qs::header_found_text(if root.is_empty() { None } else { Some(&root) });
        QString::from(&path)
    }

    pub fn status_text(&self) -> QString {
        QString::from(&self.rust().status)
    }

    pub fn sv_chips(&self) -> QList<QVariant> {
        let me = self.rust();
        let mut out = QList::<QVariant>::default();
        for chip in qs::group_chips(&me.movies, &me.favorites, &me.meta, &me.sv_group) {
            out.append(QVariant::from(&vmap(vec![
                ("key", qstr(&chip.key)),
                ("label", qstr(&chip.label)),
                ("checked", QVariant::from(&chip.checked)),
            ])));
        }
        out
    }

    pub fn sv_count_text(&self) -> QString {
        let me = self.rust();
        let filtered = !me.sv_search.trim().is_empty() || !me.sv_group.is_empty();
        QString::from(&qs::sv_count_text(
            me.visible.len(),
            me.movies.len(),
            filtered,
        ))
    }

    pub fn sv_search_text(&self) -> QString {
        QString::from(&self.rust().sv_search)
    }

    pub fn count(&self) -> i32 {
        self.rust().visible.len() as i32
    }

    pub fn autostart_on(&self) -> bool {
        self.rust().autostart_on
    }
    pub fn rotate_on(&self) -> bool {
        self.rust().rotate_on
    }
    pub fn sound_on(&self) -> bool {
        self.rust().sound_on
    }

    pub fn hero(&self) -> QMap<QMapPair_QString_QVariant> {
        let me = self.rust();
        let shown = me.shown_item();
        let view = qs::hero(shown, me.current_item(), &me.favorites, &me.meta);
        let poster = shown
            .map(|it| s(it, "path"))
            .and_then(|p| me.posters.get(p))
            .cloned()
            .unwrap_or_default();
        vmap(vec![
            ("present", QVariant::from(&view.present)),
            ("title", qstr(&view.title)),
            ("source", qstr(&view.source)),
            ("labels", qstr(&view.labels)),
            ("event", qstr(&view.event)),
            ("starText", qstr(&view.star_text)),
            ("starOn", QVariant::from(&view.star_on)),
            ("status", qstr(&view.status)),
            ("poster", qstr(&file_url(&poster))),
        ])
    }

    pub fn preview_source(&self) -> QString {
        QString::from(&file_url(&self.rust().preview_playing))
    }

    pub fn state_chip(&self) -> QMap<QMapPair_QString_QVariant> {
        let me = self.rust();
        let (text, tone) = qs::state_chip(
            me.current_item(),
            me.autostart_on,
            me.rotate_on,
            me.favorites.len(),
            me.movies.len(),
        );
        // Dark skin by default (rule #17): it is decided not by the markup but by
        // the brightness of the palette's background.
        let dark = active_is_dark();
        let (bg, border, fg) = qs::chip_colors(tone, dark);
        vmap(vec![
            ("text", qstr(&text)),
            ("background", qstr(&ui_base::css_color_to_qml(&bg))),
            ("border", qstr(&ui_base::css_color_to_qml(&border))),
            ("color", qstr(&ui_base::css_color_to_qml(&fg))),
        ])
    }

    pub fn bg_cards(&self) -> QList<QVariant> {
        let me = self.rust();
        let p = me.pager_view();
        let filtered = me.bg_filtered();
        let mut out = QList::<QVariant>::default();
        for it in &filtered[p.start..p.end] {
            let card = qs::bg_card(it);
            let thumb = me.thumbs.get(&card.id).cloned().unwrap_or_default();
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(&card.id)),
                ("icon", qstr(card.icon)),
                ("name", qstr(&card.name)),
                ("sub", qstr(&card.sub)),
                ("subTooltip", qstr(&card.sub_tooltip)),
                ("animated", QVariant::from(&card.animated)),
                ("thumbTooltip", qstr(&card.thumb_tooltip)),
                ("thumb", qstr(&file_url(&thumb))),
                ("mark", qstr(card.mark)),
                ("applied", QVariant::from(&(card.id == me.applied_id))),
            ])));
        }
        out
    }

    /// All profile backgrounds for choosing the showcase preview background.
    /// For an animated background `url` leads to a clip, so `preview` is taken —
    /// a static picture of the same size.
    pub fn bg_choices(&self) -> QList<QVariant> {
        let me = self.rust();
        let mut out = QList::<QVariant>::default();
        for it in &me.bg_all {
            let still = if it.preview.is_empty() { it.url.clone() } else { it.preview.clone() };
            let thumb = me.thumbs.get(&it.id).cloned().unwrap_or_default();
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(&it.id)),
                ("name", qstr(if it.name.is_empty() { "?" } else { &it.name })),
                // The animated marker — the same as on a grid card.
                (
                    "mark",
                    qstr(if qs::bg_is_animated(it) { qs::BG_ANIMATED_MARK } else { "" }),
                ),
                ("url", qstr(&still)),
                ("thumb", qstr(&file_url(&thumb))),
                ("equipped", QVariant::from(&it.equipped)),
            ])));
        }
        out
    }

    /// Cards of the large viewer — the whole selected list.
    ///
    /// Steam has one background picture and it is full-size right away: the file
    /// downloaded for the thumbnail is the full size. So for a static background
    /// with an already downloaded thumbnail there is nothing to wait for; the
    /// viewer will ask for the rest itself (`requestBgFull`) and get it through
    /// `bgViewerSources`.
    pub fn bg_viewer_items(&self) -> QList<QVariant> {
        let me = self.rust();
        let mut out = QList::<QVariant>::default();
        for it in me.bg_filtered() {
            let card = qs::bg_card(it);
            let still = file_url(me.thumbs.get(&card.id).map(String::as_str).unwrap_or(""));
            let ready = !still.is_empty() && !card.animated;
            out.append(QVariant::from(&vmap(vec![
                ("key", qstr(&card.id)),
                ("title", qstr(&card.name)),
                ("subtitle", qstr(&card.sub)),
                (
                    "badge",
                    qstr(&if card.animated { qs::bg_animated_badge() } else { String::new() }),
                ),
                ("thumb", qstr(&still)),
                ("source", qstr(&still)),
                ("pending", QVariant::from(&!ready)),
            ])));
        }
        out
    }

    pub fn bg_viewer_sources(&self) -> QMap<QMapPair_QString_QVariant> {
        let me = self.rust();
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for it in &me.bg_all {
            let still = me.thumbs.get(&it.id);
            let movie = me.movies_full.get(&it.id);
            let asked_done = me.full_done.contains(&it.id);
            if still.is_none() && movie.is_none() && !asked_done {
                continue;
            }
            // A static one needs only the picture; an animated one also needs the
            // clip.
            let done = asked_done || (still.is_some() && !qs::bg_is_animated(it));
            out.insert(
                QString::from(&it.id),
                QVariant::from(&vmap(vec![
                    ("source", qstr(&file_url(still.map(String::as_str).unwrap_or("")))),
                    ("video", qstr(&file_url(movie.map(String::as_str).unwrap_or("")))),
                    ("done", QVariant::from(&done)),
                ])),
            );
        }
        out
    }

    pub fn request_bg_full(mut self: Pin<&mut Self>, item_id: &QString) {
        let wanted = item_id.to_string();
        let Some(item) = self.rust().bg_all.iter().find(|it| it.id == wanted).cloned() else {
            return;
        };
        let animated = qs::bg_is_animated(&item);
        let have_still = self.rust().thumbs.contains_key(&wanted);
        if (have_still && !animated) || self.rust().full_asked.contains(&wanted) {
            return;
        }
        let Some(tasks) = self.rust().tasks.clone() else {
            return;
        };
        self.as_mut().rust_mut().full_asked.insert(wanted.clone());
        let still_url = qs::bg_card(&item).preview_url;
        // For an animated one `url` is the clip; for a static one it is the
        // picture.
        let movie_url = if animated { item.url.clone() } else { String::new() };
        tasks.start_full(wanted, if have_still { String::new() } else { still_url }, movie_url);
    }

    /// Cards of the large splash viewer: the shown clips. The preview frame is
    /// visible at once, the clip itself plays from the file on disk.
    pub fn sv_viewer_items(&self) -> QList<QVariant> {
        let me = self.rust();
        let mut out = QList::<QVariant>::default();
        for item in me.shown_movies() {
            let path = s(item, "path");
            let title = {
                let t = s(item, "title");
                let n = s(item, "name");
                if !t.is_empty() { t } else if !n.is_empty() { n } else { "?" }
            };
            let poster = file_url(me.posters.get(path).map(String::as_str).unwrap_or(""));
            let playable = !me.preview_broken.contains(path)
                && std::path::Path::new(path).is_file();
            out.append(QVariant::from(&vmap(vec![
                ("key", qstr(path)),
                ("title", qstr(title)),
                ("subtitle", qstr(&qs::source_text(item, &me.meta))),
                ("thumb", qstr(&poster)),
                ("video", qstr(&if playable { file_url(path) } else { String::new() })),
            ])));
        }
        out
    }

    pub fn bg_kind_chips(&self) -> QList<QVariant> {
        let me = self.rust();
        let mut out = QList::<QVariant>::default();
        for chip in
            qs::bg_kind_chips(&me.bg_all, &me.bg_search.trim().to_lowercase(), &me.bg_kind)
        {
            out.append(QVariant::from(&vmap(vec![
                ("key", qstr(&chip.key)),
                ("label", qstr(&chip.label)),
                ("checked", QVariant::from(&chip.checked)),
            ])));
        }
        out
    }

    pub fn bg_animated_note(&self) -> QString {
        QString::from(&qs::bg_animated_gap_text(&self.rust().bg_animated_gap))
    }

    pub fn bg_retry_text(&self) -> QString {
        QString::from(&qs::bg_retry_text())
    }

    pub fn bg_status_text(&self) -> QString {
        let me = self.rust();
        // The core decides: the section label is the reason if there is one, and
        // the background count if there is none.
        QString::from(&qs::bg_section_text_filtered(
            &me.bg_notice,
            me.bg_all.len(),
            me.bg_filtered().len(),
            !me.bg_search.trim().is_empty() || !me.bg_kind.is_empty(),
        ))
    }

    pub fn bg_search_text(&self) -> QString {
        QString::from(&self.rust().bg_search)
    }

    pub fn bg_audio_on(&self) -> bool {
        self.rust().bg_audio
    }

    pub fn pager(&self) -> QMap<QMapPair_QString_QVariant> {
        let p = self.rust().pager_view();
        vmap(vec![
            ("visible", QVariant::from(&p.visible)),
            ("prevEnabled", QVariant::from(&p.prev_enabled)),
            ("nextEnabled", QVariant::from(&p.next_enabled)),
            ("text", qstr(&p.text)),
            // The grid columns are the same ones the page is cut by: the rows are
            // always whole (`qs::bg_columns`).
            ("cols", QVariant::from(&(self.rust().cols() as i32))),
        ])
    }
}

// ============================================================================
// Splash showcase model
// ============================================================================

impl qobject::SteamModel {
    pub fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.rust().visible.len() as i32
    }

    pub fn role_names(&self) -> cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray> {
        let mut out = cxx_qt_lib::QHash::<cxx_qt_lib::QHashPair_i32_QByteArray>::default();
        for (i, name) in ROLE_NAMES.iter().enumerate() {
            out.insert(ROLE_BASE + i as i32, QByteArray::from(*name));
        }
        out
    }

    pub fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let me = self.rust();
        let row = index.row();
        if row < 0 || row as usize >= me.visible.len() {
            return QVariant::default();
        }
        let Some(item) = me.movies.get(me.visible[row as usize]) else {
            return QVariant::default();
        };
        let Some(name) = ROLE_NAMES.get((role - ROLE_BASE) as usize) else {
            return QVariant::default();
        };
        self.role_value(item, name)
    }

    /// The value of one role for a clip record. A shared place for `data`,
    /// `roleValueOf` and the checks — we do not create a second copy of the
    /// rules.
    fn role_value(&self, item: &Value, name: &str) -> QVariant {
        let me = self.rust();
        let path = s(item, "path");
        let dark = active_is_dark();
        match name {
            "path" => qstr(path),
            "title" => {
                let t = s(item, "title");
                let n = s(item, "name");
                qstr(if !t.is_empty() {
                    t
                } else if !n.is_empty() {
                    n
                } else {
                    "?"
                })
            }
            "badge" => qstr(&qs::badge_text(item, &me.meta)),
            "badgeBackground" => {
                let (bg, _) = qs::badge_colors(qs::badge_tone(item), dark);
                qstr(&ui_base::css_color_to_qml(&bg))
            }
            "badgeColor" => {
                let (_, fg) = qs::badge_colors(qs::badge_tone(item), dark);
                qstr(&ui_base::css_color_to_qml(&fg))
            }
            "poster" => qstr(&file_url(
                me.posters.get(path).map(String::as_str).unwrap_or(""),
            )),
            "starGlyph" => {
                let on = me.favorites.iter().any(|f| f == path);
                qstr(if on {
                    qs::SV_STAR_GLYPH_ON
                } else {
                    qs::SV_STAR_GLYPH_OFF
                })
            }
            "starColor" => {
                let on = me.favorites.iter().any(|f| f == path);
                qstr(&ui_base::css_color_to_qml(&qs::star_color(on)))
            }
            "starOn" => QVariant::from(&me.favorites.iter().any(|f| f == path)),
            "selected" => QVariant::from(&(path == me.current_path)),
            "deletable" => QVariant::from(&item.get("deletable").and_then(Value::as_bool).unwrap_or(false)),
            "steamChosen" => QVariant::from(&item.get("steam_selected").and_then(Value::as_bool).unwrap_or(false)),
            _ => QVariant::default(),
        }
    }

    pub fn role_number(&self, name: &QString) -> i32 {
        let wanted = name.to_string();
        ROLE_NAMES
            .iter()
            .position(|n| *n == wanted)
            .map(|i| ROLE_BASE + i as i32)
            .unwrap_or(-1)
    }

    pub fn role_value_of(&self, path: &QString, name: &QString) -> QVariant {
        let wanted = path.to_string();
        let Some(item) = self.rust().movies.iter().find(|m| s(m, "path") == wanted) else {
            return QVariant::default();
        };
        // The borrow of `item` lives from `self.rust()`, so we take a copy:
        // `role_value` asks `rust()` again.
        let copy = item.clone();
        self.role_value(&copy, &name.to_string())
    }
}

// ============================================================================
// Actions
// ============================================================================

impl qobject::SteamModel {
    pub fn set_steam_root(mut self: Pin<&mut Self>, root: &QString) {
        self.as_mut().rust_mut().steam_root = root.to_string();
        self.as_mut().header_changed();
    }

    pub fn set_movies(
        mut self: Pin<&mut Self>,
        movies: &QVariant,
        current_path: &QString,
        favorites: &QVariant,
    ) {
        {
            let mut me = self.as_mut().rust_mut();
            me.movies = movies_from(movies);
            me.favorites = strings_from(favorites);
            me.current_path = current_path.to_string();
            // Nothing is chosen — take the first one found, like the original.
            if me.current_path.is_empty() {
                if let Some(first) = me.movies.first() {
                    me.current_path = s(first, "path").to_string();
                }
            }
        }
        self.as_mut().reset_showcase();
    }

    pub fn set_splash_state(mut self: Pin<&mut Self>, enabled: bool, rotate: bool, sound: bool) {
        {
            let mut me = self.as_mut().rust_mut();
            me.autostart_on = enabled;
            me.rotate_on = rotate;
            me.sound_on = sound;
        }
        self.as_mut().showcase_changed();
        self.as_mut().hero_and_preview();
    }

    pub fn set_section(mut self: Pin<&mut Self>, key: &QString) {
        let key = key.to_string();
        if !qs::is_segment(&key) {
            return;
        }
        if self.rust().section == key {
            return;
        }
        self.as_mut().rust_mut().section = key;
        // Left the showcase — the hover is removed: the preview plays only on its
        // own sub-tab (as in the original).
        self.as_mut().rust_mut().hovered_path.clear();
        self.as_mut().section_changed();
        self.as_mut().hero_and_preview();
    }

    pub fn set_sv_search(mut self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        if self.rust().sv_search == text {
            return;
        }
        self.as_mut().rust_mut().sv_search = text;
        self.as_mut().reset_showcase();
    }

    pub fn set_sv_group(mut self: Pin<&mut Self>, key: &QString) {
        let key = key.to_string();
        if self.rust().sv_group == key {
            return;
        }
        self.as_mut().rust_mut().sv_group = key;
        self.as_mut().reset_showcase();
    }

    pub fn select_movie(mut self: Pin<&mut Self>, path: &QString) {
        let path = path.to_string();
        if path.is_empty() {
            return;
        }
        self.as_mut().rust_mut().current_path = path;
        // The choice changes the highlighting of the cards — we reset the whole
        // model.
        unsafe {
            self.as_mut().begin_reset_model();
            self.as_mut().end_reset_model();
        }
        self.as_mut().hero_and_preview();
    }

    pub fn set_hovered(mut self: Pin<&mut Self>, path: &QString) {
        let path = path.to_string();
        if self.rust().hovered_path == path {
            return;
        }
        self.as_mut().rust_mut().hovered_path = path;
        self.as_mut().hero_and_preview();
    }

    pub fn set_favorites(mut self: Pin<&mut Self>, favorites: &QVariant) {
        self.as_mut().rust_mut().favorites = strings_from(favorites);
        // Favourites change the stars, the set of chips and, if favourites are
        // exactly what is being viewed, the composition of the showcase.
        self.as_mut().reset_showcase();
    }

    // ── Live clip preview ───────────────────────────────────────────────

    pub fn set_preview_shown(mut self: Pin<&mut Self>, on: bool) {
        if self.rust().preview_shown == on {
            return;
        }
        self.as_mut().rust_mut().preview_shown = on;
        self.hero_and_preview();
    }

    pub fn mark_preview_broken(mut self: Pin<&mut Self>) {
        let path = self.rust().preview_playing.clone();
        if path.is_empty() {
            return;
        }
        if !self.as_mut().rust_mut().preview_broken.insert(path) {
            return;
        }
        self.hero_and_preview();
    }

    pub fn pause_preview(mut self: Pin<&mut Self>) {
        let until = Instant::now() + Duration::from_secs_f64(qs::SV_TEST_PAUSE_S);
        self.as_mut().rust_mut().preview_paused_until = Some(until);
        self.hero_and_preview();
    }

    pub fn resume_preview(mut self: Pin<&mut Self>) {
        if self.rust().preview_paused_until.is_none() {
            return;
        }
        self.as_mut().rust_mut().preview_paused_until = None;
        self.hero_and_preview();
    }

    pub fn preview_tick(mut self: Pin<&mut Self>) {
        if matches!(self.rust().preview_paused_until, Some(t) if Instant::now() >= t) {
            self.as_mut().rust_mut().preview_paused_until = None;
        }
        // We stay silent while the decision is the same: otherwise the large card
        // would be rebuilt once a second simply because nobody did anything outside
        // the window.
        if self.rust().preview_path() != self.rust().preview_playing.as_str() {
            self.hero_and_preview();
        }
    }

    pub fn set_status_text(mut self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        self.as_mut().set_status(&text);
    }

    pub fn import_movie(mut self: Pin<&mut Self>, path: &QString) -> bool {
        self.as_mut().start_import(path.to_string(), false)
    }

    pub fn migrate_movie(mut self: Pin<&mut Self>, path: &QString) -> bool {
        self.as_mut().start_import(path.to_string(), true)
    }

    pub fn cancel_import(self: Pin<&mut Self>) {
        if let Some(tasks) = self.rust().tasks.clone() {
            tasks.cancel_movie_import();
        }
    }

    /// The shared door of adding and moving a clip: the background task and
    /// the progress line. Without the task runtime nothing can be done in the
    /// background — saying so instead of freezing the window.
    fn start_import(mut self: Pin<&mut Self>, path: String, migrate: bool) -> bool {
        if path.is_empty() {
            return false;
        }
        let Some(tasks) = self.rust().tasks.clone() else {
            // Moving an old selection simply waits for the tasks; an addition the
            // person asked for must not stay silent.
            if !migrate {
                self.as_mut().set_status(&rexon_core::session_splash::msg_import_failed(&qs::sv_no_reason()));
            }
            return false;
        };
        if !tasks.start_movie_import(path, migrate) {
            return false;
        }
        {
            let mut me = self.as_mut().rust_mut();
            me.import_busy = true;
            me.import_text = if migrate {
                rexon_core::session_splash::import_stage_text(rexon_core::session_splash::ImportStage::Copying)
            } else {
                rexon_core::session_splash::import_stage_text(rexon_core::session_splash::ImportStage::Checking)
            };
        }
        self.as_mut().import_changed();
        true
    }

    pub fn autostart_answered(mut self: Pin<&mut Self>, on: bool, reason: &QString) {
        let reason = reason.to_string();
        let Some(text) = qs::autostart_refusal_text(on, &reason) else {
            return;
        };
        // The raw reason goes to the log: it tells the customer nothing, but
        // someone analysing the refusal needs it verbatim.
        rexon_core::log_warn!("splash", "заставка при входе не переключена ({on}): {reason}");
        self.as_mut().set_status(&text);
    }

    pub fn rotation_toggled(mut self: Pin<&mut Self>, on: bool) {
        let refusal = {
            let me = self.rust();
            qs::rotation_refusal(on, me.favorites.len(), me.movies.len())
        };
        if let Some(text) = refusal {
            self.as_mut().set_status(&text);
        }
    }

    pub fn shown_path(&self) -> QString {
        QString::from(self.rust().shown_item().map(|it| s(it, "path")).unwrap_or(""))
    }

    /// A port of `QWindow.isExposed()` from the original
    /// (`_sv_preview_allowed`).
    ///
    /// The pointer cast is the same as `static_cast<QWindow*>` would do:
    /// `QObject` is the FIRST base of `QWindow`
    /// (`class QWindow : public QObject, public QSurface`), the first base has
    /// no offset, the address matches.
    ///
    /// # Safety
    /// `window` is a live QWindow or null.
    pub unsafe fn window_exposed(&self, window: *mut qobject::QObject) -> bool {
        if window.is_null() {
            return true;
        }
        let win: &qobject::QWindow = unsafe { &*(window as *const qobject::QWindow) };
        win.is_exposed()
    }

    pub fn source_text_for(&self, path: &QString) -> QString {
        let wanted = path.to_string();
        let me = self.rust();
        match me.movies.iter().find(|m| s(m, "path") == wanted) {
            Some(it) => QString::from(&qs::source_text(it, &me.meta)),
            None => QString::default(),
        }
    }

    // ── Profile backgrounds ─────────────────────────────────────────────

    /// The width comes as a STRING: an invokable with a number in its
    /// signature cannot be called from outside (`::std::int32_t` in the
    /// metaobject has no metatype). This is the width of the grid ITSELF
    /// (without the group's margins): the QML grid divides a row into
    /// `pager.cols` columns by the same width.
    pub fn set_bg_viewport_width(mut self: Pin<&mut Self>, width: &QString) {
        let px: i32 = width.to_string().trim().parse().unwrap_or(0);
        let cols = qs::bg_columns(px);
        let old = self.rust().cols();
        if cols == old {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            // What was first on the screen stays first.
            me.bg_page = qs::bg_page_after_resize(me.bg_page, old, cols);
            me.bg_cols = cols;
        }
        self.as_mut().refresh_backgrounds();
    }

    pub fn set_bg_search(mut self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        if self.rust().bg_search == text {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            me.bg_search = text;
            // A search over the whole inventory → reset to page 0.
            me.bg_page = 0;
        }
        self.as_mut().refresh_backgrounds();
    }

    pub fn set_bg_kind(mut self: Pin<&mut Self>, key: &QString) {
        let key = qs::normalize_bg_kind(&key.to_string());
        if self.rust().bg_kind == key {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            me.bg_kind = key.to_string();
            // A selection over the whole inventory → reset to page 0.
            me.bg_page = 0;
        }
        self.as_mut().refresh_backgrounds();
    }

    pub fn reload_backgrounds(mut self: Pin<&mut Self>) {
        let Some(tasks) = self.rust().tasks.clone() else {
            return;
        };
        let sid = self.rust().sid.clone();
        if sid.is_empty() {
            return;
        }
        let equipped = self.rust().bg_equipped.clone();
        // The reason is removed at once: the button was pressed, the answer has
        // not come yet — the previous verdict is no longer about this request.
        self.as_mut().rust_mut().bg_animated_gap = qs::AnimatedGap::None;
        let shown = self.rust().bg_all.len();
        let text = if shown == 0 { qs::bg_loading_text() } else { qs::bg_refreshing_text(shown) };
        if shown > 0 {
            self.as_mut().rust_mut().cache_shown = true;
        }
        self.as_mut().set_bg_notice(&text);
        self.as_mut().backgrounds_changed();
        tasks.start_inventory(InventoryRequest {
            sid,
            equipped,
            webapi_token: String::new(),
        });
    }

    pub fn set_bg_audio(mut self: Pin<&mut Self>, on: bool) {
        self.as_mut().rust_mut().bg_audio = on;
        self.as_mut().backgrounds_changed();
    }

    pub fn bg_prev_page(mut self: Pin<&mut Self>) {
        let p = self.rust().pager_view();
        if !p.prev_enabled {
            return;
        }
        self.as_mut().rust_mut().bg_page = p.page.saturating_sub(1);
        self.as_mut().refresh_backgrounds();
    }

    pub fn bg_next_page(mut self: Pin<&mut Self>) {
        let p = self.rust().pager_view();
        if !p.next_enabled {
            return;
        }
        self.as_mut().rust_mut().bg_page = p.page + 1;
        self.as_mut().refresh_backgrounds();
    }

    pub fn apply_background(mut self: Pin<&mut Self>, item_id: &QString) -> bool {
        let wanted = item_id.to_string();
        let Some(item) = self
            .rust()
            .bg_all
            .iter()
            .find(|it| it.id == wanted)
            .cloned()
        else {
            return false;
        };
        let Some(tasks) = self.rust().tasks.clone() else {
            self.as_mut().set_status(&qs::bg_no_engine());
            return false;
        };
        let audio = self.rust().bg_audio;
        // ⭐ A real engine: the core's shared wallpaper executor (the same as in
        // "Settings"). Until 15.09.2026 a stub stood here, and "Apply" answered
        // "the wallpaper engine is unavailable" under both page hosts.
        let applier: std::sync::Arc<dyn WallpaperApply> =
            std::sync::Arc::new(steam_workers::ExecApplier::live());
        if !tasks.apply_background(item.clone(), audio, applier) {
            self.as_mut().set_status(&qs::bg_busy());
            return false;
        }
        let text = qs::bg_applying_text(&item.name);
        self.as_mut().rust_mut().pending_apply = Some(item);
        self.as_mut().set_status(&text);
        true
    }

    // ── Background tasks ────────────────────────────────────────────────

    pub fn start(mut self: Pin<&mut Self>, webapi_token: &QString) {
        if self.rust().tasks.is_some() {
            return;
        }
        let env = self.rust().env.clone();
        let Some(tasks) = SteamTasks::new(env.clone()) else {
            return;
        };
        self.as_mut().rust_mut().tasks = Some(Arc::new(tasks));

        // The tile frames are ordered already here: the page hands over the clips
        // (`setMovies`) BEFORE `start`, when there are no tasks yet, and the order
        // then goes into the void. After that only `reset_showcase` at the end of
        // this function repeated it — and without Steam or without an account it
        // is never reached, and all tiles (own clips, bench clips) stayed with a
        // clapperboard forever. A frame needs neither Steam nor an account — only
        // ffmpeg.
        let root = self.rust().steam_root.clone();
        if root.is_empty() {
            self.rust().request_posters();
            self.as_mut().set_bg_notice(&qs::bg_no_steam());
            return;
        }
        let Some(sid) = steam_workers::detect_account(&root) else {
            self.rust().request_posters();
            self.as_mut().set_bg_notice(&qs::bg_no_account());
            return;
        };
        // We do not clear the reason: until the network answers, the label
        // honestly says "⏳ Loading the Steam inventory…", as in the original.
        let equipped = steam_workers::read_equipped_background(&root);

        {
            let mut me = self.as_mut().rust_mut();
            me.sid = sid.clone();
            me.bg_equipped = equipped.clone();
            me.applied_id = steam_workers::read_applied_id(&env);
            me.meta = steam_workers::read_meta_cache(&env, &sid);
        }
        // Disk cache of the list: the showcase fills INSTANTLY, the network will
        // quietly update it afterwards (the original's stale-while-revalidate).
        if let Some(cached) = steam_workers::read_inventory_cache(&env, &sid) {
            let shown = cached.len();
            {
                let mut me = self.as_mut().rust_mut();
                me.bg_all = rexon_core::steam_profile_items::finalize_items(cached, equipped.clone());
                me.cache_shown = true;
                me.bg_page = 0;
            }
            self.as_mut().refresh_backgrounds();
            // The order is as in the original: first show the cache, then label it
            // "updating". The label is the section's, not a toast.
            let text = qs::bg_refreshing_text(shown);
            self.as_mut().set_bg_notice(&text);
        }
        // Markers from the cache change the showcase badges and chips.
        self.as_mut().reset_showcase();

        if let Some(tasks) = self.rust().tasks.clone() {
            tasks.start_inventory(InventoryRequest {
                sid,
                equipped,
                webapi_token: webapi_token.to_string(),
            });
        }
    }

    pub fn pump(mut self: Pin<&mut Self>) {
        loop {
            let Some(tasks) = self.rust().tasks.clone() else {
                return;
            };
            let Some(event) = tasks.poll_event() else {
                return;
            };
            self.as_mut().handle_event(event);
        }
    }

    pub fn shutdown(mut self: Pin<&mut Self>) {
        if let Some(tasks) = self.as_mut().rust_mut().tasks.take() {
            tasks.shutdown();
        }
    }
}

// ============================================================================
// Internals: receiving events and recomputations
// ============================================================================

impl qobject::SteamModel {
    /// Lay out one background task event into the page state.
    fn handle_event(mut self: Pin<&mut Self>, event: SteamEvent) {
        match event {
            SteamEvent::InventoryDone { items, error } => {
                self.as_mut().on_inventory(items, error);
            }
            SteamEvent::OwnedAnswer(gap) => {
                if self.rust().bg_animated_gap != gap {
                    self.as_mut().rust_mut().bg_animated_gap = gap;
                    self.as_mut().backgrounds_changed();
                }
            }
            SteamEvent::SplashMeta(meta) => {
                self.as_mut().rust_mut().meta = meta;
                // The markers arrived — some clips got a sale event, so the filter groups
                // became different too.
                self.as_mut().reset_showcase();
            }
            SteamEvent::Poster { id, png } => {
                self.as_mut().rust_mut().posters.insert(id, png);
                // A frame changes ONE role of a shown row; a full model reset here would
                // jerk the whole grid on every frame.
                unsafe {
                    self.as_mut().begin_reset_model();
                    self.as_mut().end_reset_model();
                }
                self.as_mut().hero_and_preview();
            }
            SteamEvent::Thumb { id, path } => {
                self.as_mut().rust_mut().thumbs.insert(id, path);
                self.as_mut().backgrounds_changed();
            }
            SteamEvent::ApplyDone { ok, message } => {
                self.as_mut().on_apply_done(ok, message);
            }
            SteamEvent::Full { id, still, movie } => {
                {
                    let mut me = self.as_mut().rust_mut();
                    if !still.is_empty() {
                        me.thumbs.insert(id.clone(), still);
                    }
                    if !movie.is_empty() {
                        me.movies_full.insert(id.clone(), movie);
                    }
                    me.full_done.insert(id);
                }
                self.as_mut().backgrounds_changed();
            }
            SteamEvent::MovieImportStage(text) => {
                if self.rust().import_text != text {
                    self.as_mut().rust_mut().import_text = text;
                    self.as_mut().import_changed();
                }
            }
            SteamEvent::MovieImportDone { ok, message } => {
                {
                    let mut me = self.as_mut().rust_mut();
                    me.import_busy = false;
                    me.import_text.clear();
                }
                self.as_mut().import_changed();
                self.as_mut().set_status(&message);
                if ok {
                    self.as_mut().movies_on_disk_changed();
                }
            }
        }
    }

    /// A port of `_on_inventory_done`.
    fn on_inventory(mut self: Pin<&mut Self>, items: Vec<ProfileBackgroundItem>, error: String) {
        let cache_shown = self.rust().cache_shown && !self.rust().bg_all.is_empty();
        if items.is_empty() {
            // The network/privacy let us down or there are no backgrounds. We do NOT
            // erase the list shown from the cache — we keep the previous one.
            if cache_shown {
                // No reason — the label returns to the count of shown backgrounds, exactly
                // as in the original.
                self.as_mut().set_bg_notice("");
                return;
            }
            let text = if error.is_empty() {
                qs::bg_nothing_found()
            } else {
                qs::bg_error_text(&error)
            };
            self.as_mut().set_bg_notice(&text);
            return;
        }
        {
            let env = self.rust().env.clone();
            let sid = self.rust().sid.clone();
            if !sid.is_empty() {
                steam_workers::write_inventory_cache(&env, &sid, &items);
            }
            let mut me = self.as_mut().rust_mut();
            me.bg_all = items;
            me.applied_id = steam_workers::read_applied_id(&env);
            me.bg_page = 0;
        }
        self.as_mut().set_bg_notice("");
        self.as_mut().refresh_backgrounds();
        self.as_mut().set_status("");
    }

    /// A port of `_on_apply_done`.
    fn on_apply_done(mut self: Pin<&mut Self>, ok: bool, message: String) {
        let item = self.as_mut().rust_mut().pending_apply.take();
        match (ok, item) {
            (true, Some(item)) => {
                let env = self.rust().env.clone();
                steam_workers::write_applied(&env, &item.id, &item.name);
                let text = qs::bg_applied_text(&item.name);
                self.as_mut().rust_mut().applied_id = item.id;
                self.as_mut().set_status(&text);
                self.as_mut().backgrounds_changed();
            }
            _ => {
                let text = qs::bg_apply_error_text(&message);
                self.as_mut().set_status(&text);
            }
        }
    }

    /// Rebuild the whole splash showcase: the composition of rows and their
    /// order change at once — this is exactly the case for which Qt has reset.
    fn reset_showcase(mut self: Pin<&mut Self>) {
        unsafe {
            self.as_mut().begin_reset_model();
            self.as_mut().rust_mut().recompute_showcase();
            self.as_mut().end_reset_model();
        }
        self.as_mut().showcase_changed();
        self.as_mut().hero_and_preview();
        self.rust().request_posters();
    }

    /// Rebuild the background page and order its thumbnails.
    fn refresh_backgrounds(mut self: Pin<&mut Self>) {
        // The page may have gone beyond the list — we normalize it with the same
        // calculation that draws the pagination strip.
        let page = self.rust().pager_view().page;
        self.as_mut().rust_mut().bg_page = page;
        self.as_mut().backgrounds_changed();
        self.rust().request_page_thumbs();
    }

    /// The large card changed — and with it the clip that plays in it: it is
    /// ONE and the same clip. A port of the original's `_sv_update_hero` +
    /// `_sv_sync_preview` pair — there it was also called as a pair.
    fn hero_and_preview(mut self: Pin<&mut Self>) {
        let want = self.rust().preview_path().to_string();
        let changed = self.rust().preview_playing != want;
        self.as_mut().rust_mut().preview_playing = want;
        // The clip signal — only on a real change: it brings up the player, and
        // there is no reason to poke it on every mouse movement.
        if changed {
            self.as_mut().preview_source_changed();
        }
        self.as_mut().hero_changed();
    }

    /// Status line (the original's shared pop-up toast).
    fn set_status(mut self: Pin<&mut Self>, text: &str) {
        self.as_mut().rust_mut().status = text.to_string();
        self.as_mut().status_changed();
    }

    /// The reason the background section has nothing to show. An empty string
    /// means there is no reason, and the label returns to the background count.
    fn set_bg_notice(mut self: Pin<&mut Self>, text: &str) {
        if self.rust().bg_notice == text {
            return;
        }
        self.as_mut().rust_mut().bg_notice = text.to_string();
        self.as_mut().backgrounds_changed();
    }
}
