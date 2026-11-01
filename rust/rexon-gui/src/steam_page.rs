// SteamModel — вкладка «Steam» на Rust + cxx-qt + QML.
//
// Перенесена на QML по общей схеме вкладок: решения и подписи — ядро,
// здесь оболочка QObject, в QML только разметка.
//
// Что здесь есть и чего нет:
//   * ЕСТЬ оболочка QObject: неизменные подписи страницы, палитра, модель
//     списка витрины заставок (роли делегата), карточки текущей страницы
//     фонов, крупная карточка, чип состояния, полоса пагинации и приём
//     событий от фоновых задач (`crate::steam_workers`);
//   * НЕТ ни одной строки продуктовых решений — что показать, в каком
//     порядке, каким цветом и какими словами, отвечает
//     `rexon_core::qt_steam`.
//
// ⚠️ Сетка фонов отдаётся СПИСКОМ (`bgCards`), а не второй моделью. Причина
// не в лени: на странице лежит `cols × BG_ROWS` карточек, то есть 25 при
// пяти столбцах и не больше 40 при восьми — ровно тот объём, ради которого
// заводить вторую модель со своими ролями и своим сбросом незачем. Витрина
// заставок моделью осталась: там весь список целиком (у владельца — сотни
// роликов), и повторитель по нему был бы уже расточительством.
//
// ⚠️ Витрину заставок наполняет не сама модель, а хозяин страницы
// (`setMovies`), и это не незаконченность, а граница: скан роликов, конфиг,
// избранное и автозапуск — побочные действия, их место в объекте `actions`
// (тот же приём, что у «Расширений» с установкой `.rxn`). Python-хозяин
// кладёт туда `rexon/qt_steam/qml_window.py`, Rust-хозяин — `steam_actions.rs`;
// оба зовут один и тот же поток ядра `rexon_core::session_splash` поверх
// живого исполнителя `rexon_core::splash_exec` (до 08.09.2026 тот был заперт
// в `rexon-py`, и Rust-объекта действий у вкладки быть не могло).

/// Мост cxx-qt: содержимое вкладки «Steam». Оно же — модель витрины заставок.
///
/// ⚠️ Тип зовётся `SteamModel`, а не `SteamPage`: .qml-файлы модуля сами
/// становятся QML-типами по имени файла, и `SteamPage` занят
/// `qml/SteamPage.qml`.
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

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    // Окно верхнего уровня — ради ОДНОГО вопроса: показывает ли композитор
    // нашу поверхность.
    //
    // ⚠️ Свойства `visibility` у окна для этого НЕ хватает. На Wayland
    // протокол xdg-shell состояния «свёрнуто» клиенту не сообщает вовсе:
    // после сворачивания KWin окно продолжает числиться обычным
    // (`Window.Windowed`), и превью крутило бы видео в никуда. Честный
    // признак — `QWindow::isExposed()`; Q_PROPERTY у него нет, поэтому из
    // QML он достаётся только так.
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
        // ── Окно и шапка ────────────────────────────────────────────────
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
        // Подвкладки: [{key, label, checked}, …].
        #[qproperty(QList_QVariant, segments, READ = segments,
                    NOTIFY = section_changed)]
        #[qproperty(QString, section, READ = section, NOTIFY = section_changed)]

        // ── Секция «Видеозаставка» ──────────────────────────────────────
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
        // Меняющееся у витрины.
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
        // Крупная карточка: {present, title, source, labels, event,
        // starText, starOn, status, poster}.
        #[qproperty(QMap_QString_QVariant, hero, READ = hero,
                    NOTIFY = hero_changed)]
        // Что ДОЛЖНО играть в крупной карточке прямо сейчас: `file://…`
        // либо «» (не играем). Сигнал тот же, что у карточки: превью и
        // карточка показывают ОДИН ролик, и меняются они всегда вместе.
        #[qproperty(QString, preview_source, cxx_name = "previewSource",
                    READ = preview_source, NOTIFY = preview_source_changed)]
        // Чип состояния: {text, background, border, color}.
        #[qproperty(QMap_QString_QVariant, state_chip, cxx_name = "stateChip",
                    READ = state_chip, NOTIFY = hero_changed)]

        // ── Секция «Steam-фоны» ─────────────────────────────────────────
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
        // Карточки ТЕКУЩЕЙ страницы: [{id, icon, name, sub, subTooltip,
        // animated, thumbTooltip, thumb, applied}, …].
        #[qproperty(QList_QVariant, bg_cards, cxx_name = "bgCards",
                    READ = bg_cards, NOTIFY = backgrounds_changed)]
        // Все фоны профиля (не страница сетки) — выбор фона для
        // предпросмотра в подвкладке «Витрины профиля»:
        // [{id, name, url, thumb, equipped}, …], `url` — статичная картинка.
        #[qproperty(QList_QVariant, bg_choices, cxx_name = "bgChoices",
                    READ = bg_choices, NOTIFY = backgrounds_changed)]
        // Поздние адреса крупного просмотра: id фона → {source, video,
        // done} — для `RexonImageViewer.sources`.
        #[qproperty(QMap_QString_QVariant, bg_viewer_sources, cxx_name = "bgViewerSources",
                    READ = bg_viewer_sources, NOTIFY = backgrounds_changed)]
        // Переключатель вида фонов: [{key, label, checked}, …] — «Все /
        // Статичные / Анимированные» со счётчиками.
        #[qproperty(QList_QVariant, bg_kind_chips, cxx_name = "bgKindChips",
                    READ = bg_kind_chips, NOTIFY = backgrounds_changed)]
        // Почему в списке нет анимированных фонов и что с этим делать;
        // пусто — причины нет, строка с кнопкой «Повторить» не видна.
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
        // Полоса пагинации: {visible, prevEnabled, nextEnabled, text}.
        #[qproperty(QMap_QString_QVariant, pager, READ = pager,
                    NOTIFY = backgrounds_changed)]

        // ── Общее ───────────────────────────────────────────────────────
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
        // Краски и размеры оформления вкладки, которых нет в палитре скина:
        // группа-секция, карточка витрины, врезка-тезис, главная и зелёная
        // кнопки, чип-переключатель, подложки миниатюр. Всё это уже есть у
        // ядра (тем же набором красится виджетная вкладка) — разметка их
        // только берёт, своих не заводит. Скин сменили — пересчитать.
        #[qproperty(QMap_QString_QVariant, styles, READ = styles,
                    NOTIFY = palette_changed)]
        /// Через сколько гаснет плашка состояния, мс. Число одно на продукт
        /// (`ui_base::TOAST_TIMEOUT_MS`), своего у вкладки нет.
        #[qproperty(i32, toast_timeout_ms, cxx_name = "toastTimeoutMs",
                    READ = toast_timeout_ms, CONSTANT)]
        type SteamModel = super::SteamModelRust;
    }

    // ── Сигналы ─────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        /// Скин сменили на лету — палитру пересчитать.
        #[qsignal]
        #[cxx_name = "paletteChanged"]
        fn palette_changed(self: Pin<&mut SteamModel>);

        /// Всё, что меняется вместе с составом витрины заставок.
        #[qsignal]
        #[cxx_name = "showcaseChanged"]
        fn showcase_changed(self: Pin<&mut SteamModel>);

        /// Крупная карточка и чип состояния: они всегда меняются парой.
        #[qsignal]
        #[cxx_name = "heroChanged"]
        fn hero_changed(self: Pin<&mut SteamModel>);

        /// Что играет в крупной карточке. Отдельным сигналом от
        /// `heroChanged` и только по РЕАЛЬНОЙ смене ролика: по нему разметка
        /// поднимает проигрыватель при первой надобности, а он тянет за
        /// собой мультимедийный слой Qt.
        #[qsignal]
        #[cxx_name = "previewSourceChanged"]
        fn preview_source_changed(self: Pin<&mut SteamModel>);

        /// Состав страницы фонов, строка состояния и пагинация.
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
    }

    // ── Наследуемые методы базового класса ──────────────────────────────
    extern "RustQt" {
        /// # Safety
        /// Требует парного end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut SteamModel>);
        /// # Safety
        /// Парный к begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut SteamModel>);
    }

    // ── Переопределения QAbstractListModel ──────────────────────────────
    // Без #[qinvokable]: инвокабл в наследнике затеняет укороченные клоны
    // сигнатур базы, и rowCount() без аргументов из JS падает.
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

    // ── Чтение свойств ──────────────────────────────────────────────────
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

    // ── API для QML и хозяина страницы ──────────────────────────────────
    //
    // ⚠️ Ни одного инвокабла с числом в подписи: cxx-qt пишет в метаобъект
    // тип как `::std::int32_t`, метатипа с таким именем нет, и вызов через
    // `QMetaObject.invokeMethod` молча возвращает False.
    extern "RustQt" {
        /// Где нашёлся Steam (пусто — не нашёлся). Зовёт хозяин страницы.
        #[qinvokable]
        #[cxx_name = "setSteamRoot"]
        fn set_steam_root(self: Pin<&mut SteamModel>, root: &QString);

        /// Наполнить витрину заставок. `movies` — список записей скана
        /// (`session_splash.scan_startup_movies`), `favorites` — пути со
        /// звёздочкой. Побочные действия заставки пока у хозяина страницы.
        #[qinvokable]
        #[cxx_name = "setMovies"]
        fn set_movies(
            self: Pin<&mut SteamModel>,
            movies: &QVariant,
            current_path: &QString,
            favorites: &QVariant,
        );

        /// Состояние переключателей заставки одним вызовом.
        #[qinvokable]
        #[cxx_name = "setSplashState"]
        fn set_splash_state(
            self: Pin<&mut SteamModel>,
            enabled: bool,
            rotate: bool,
            sound: bool,
        );

        /// Переключить подвкладку (`splash` либо `bg`).
        #[qinvokable]
        #[cxx_name = "setSection"]
        fn set_section(self: Pin<&mut SteamModel>, key: &QString);

        /// Строка поиска витрины заставок.
        #[qinvokable]
        #[cxx_name = "setSvSearch"]
        fn set_sv_search(self: Pin<&mut SteamModel>, text: &QString);

        /// Чип фильтра «откуда заставка».
        #[qinvokable]
        #[cxx_name = "setSvGroup"]
        fn set_sv_group(self: Pin<&mut SteamModel>, key: &QString);

        /// Выбрать ролик для входа (клик по карточке).
        #[qinvokable]
        #[cxx_name = "selectMovie"]
        fn select_movie(self: Pin<&mut SteamModel>, path: &QString);

        /// Карточка под курсором («» — курсор ушёл): её показывает крупная
        /// карточка, пока не выбрали другую.
        #[qinvokable]
        #[cxx_name = "setHovered"]
        fn set_hovered(self: Pin<&mut SteamModel>, path: &QString);

        /// Отметить/снять звёздочку. `favorites` — актуальный список от
        /// хозяина (звёздочку сохраняет он).
        #[qinvokable]
        #[cxx_name = "setFavorites"]
        fn set_favorites(self: Pin<&mut SteamModel>, favorites: &QVariant);

        /// Путь ролика, который сейчас показывает крупная карточка.
        #[qinvokable]
        #[cxx_name = "shownPath"]
        fn shown_path(self: &SteamModel) -> QString;

        /// Строка-источник ролика (подсказка бейджа карточки).
        #[qinvokable]
        #[cxx_name = "sourceTextFor"]
        fn source_text_for(self: &SteamModel, path: &QString) -> QString;

        // ── Живое превью ролика ─────────────────────────────────────────

        /// Видна ли крупная карточка человеку. Решает разметка: своя
        /// подвкладка, своя вкладка окна, окно на экране. `false` —
        /// `previewSource` пустеет, и плеер останавливается.
        #[qinvokable]
        #[cxx_name = "setPreviewShown"]
        fn set_preview_shown(self: Pin<&mut SteamModel>, on: bool);

        /// Плеер не осилил ролик, который играл (битый файл, незнакомый
        /// кодек). Больше его не пробуем: иначе секундный опрос открывал бы
        /// его снова и снова. Какой именно — модель знает сама, разметке
        /// пересказывать это не надо.
        #[qinvokable]
        #[cxx_name = "markPreviewBroken"]
        fn mark_preview_broken(self: Pin<&mut SteamModel>);

        /// Ролик пошёл на весь экран — превью замирает: двух копий одного
        /// видео разом быть не должно.
        #[qinvokable]
        #[cxx_name = "pausePreview"]
        fn pause_preview(self: Pin<&mut SteamModel>);

        /// Вернулись в окно — полноэкранный показ закончился, паузу снять.
        #[qinvokable]
        #[cxx_name = "resumePreview"]
        fn resume_preview(self: Pin<&mut SteamModel>);

        /// Секундный опрос обстановки: снимает ПРОСРОЧЕННУЮ паузу. Нужен
        /// на случай, когда фокус после полноэкранного показа не вернулся
        /// вовсе, — иначе превью замерло бы навсегда.
        #[qinvokable]
        #[cxx_name = "previewTick"]
        fn preview_tick(self: Pin<&mut SteamModel>);

        /// Строка состояния страницы (итог полноэкранного показа).
        #[qinvokable]
        #[cxx_name = "setStatus"]
        fn set_status_text(self: Pin<&mut SteamModel>, text: &QString);

        /// Ответ переключателя «Показывать при входе»: `on` — что просили,
        /// `reason` — ответ `actions.setAutostart` («» — получилось). Отказ
        /// показывается строкой состояния простыми словами из ядра.
        #[qinvokable]
        #[cxx_name = "autostartAnswered"]
        fn autostart_answered(self: Pin<&mut SteamModel>, on: bool, reason: &QString);

        /// Переключатель «Каждый вход — новый ролик» нажат: если крутить
        /// нечего, страница говорит об этом (порог — в ядре).
        #[qinvokable]
        #[cxx_name = "rotationToggled"]
        fn rotation_toggled(self: Pin<&mut SteamModel>, on: bool);

        /// Показывает ли композитор окно `window` (в QML — `Window.window`).
        /// Пустой указатель — «показывает»: окна ещё нет, а замирать из-за
        /// этого превью не должно.
        ///
        /// # Safety
        /// `window` обязан быть живым QWindow.
        #[qinvokable]
        #[cxx_name = "windowExposed"]
        unsafe fn window_exposed(self: &SteamModel, window: *mut QObject) -> bool;

        // ── Фоны профиля ────────────────────────────────────────────────

        /// Ширина полосы фонов изменилась — пересчитать столбцы и страницу.
        #[qinvokable]
        #[cxx_name = "setBgViewportWidth"]
        fn set_bg_viewport_width(self: Pin<&mut SteamModel>, width: &QString);

        /// Строка поиска сетки фонов.
        #[qinvokable]
        #[cxx_name = "setBgSearch"]
        fn set_bg_search(self: Pin<&mut SteamModel>, text: &QString);

        /// Фильтр вида фонов: «» — все, `static`, `animated`.
        #[qinvokable]
        #[cxx_name = "setBgKind"]
        fn set_bg_kind(self: Pin<&mut SteamModel>, key: &QString);

        /// Заново спросить у Steam список фонов (кнопка «Повторить» у
        /// строки «анимированные не показаны»).
        #[qinvokable]
        #[cxx_name = "reloadBackgrounds"]
        fn reload_backgrounds(self: Pin<&mut SteamModel>);

        /// Звук видео-обоев.
        #[qinvokable]
        #[cxx_name = "setBgAudio"]
        fn set_bg_audio(self: Pin<&mut SteamModel>, on: bool);

        /// Предыдущая страница сетки.
        #[qinvokable]
        #[cxx_name = "bgPrevPage"]
        fn bg_prev_page(self: Pin<&mut SteamModel>);

        /// Следующая страница сетки.
        #[qinvokable]
        #[cxx_name = "bgNextPage"]
        fn bg_next_page(self: Pin<&mut SteamModel>);

        /// Список для крупного просмотра: ВСЕ отобранные фоны, а не одна
        /// страница сетки, — листание идёт по всему списку.
        #[qinvokable]
        #[cxx_name = "bgViewerItems"]
        fn bg_viewer_items(self: &SteamModel) -> QList_QVariant;

        /// Просмотру понадобился фон целиком — добыть его (один раз).
        #[qinvokable]
        #[cxx_name = "requestBgFull"]
        fn request_bg_full(self: Pin<&mut SteamModel>, item_id: &QString);

        /// Список для крупного просмотра заставок: показанные ролики.
        #[qinvokable]
        #[cxx_name = "svViewerItems"]
        fn sv_viewer_items(self: &SteamModel) -> QList_QVariant;

        /// Начать применение фона обоями. `false` — уже применяем.
        #[qinvokable]
        #[cxx_name = "applyBackground"]
        fn apply_background(self: Pin<&mut SteamModel>, item_id: &QString) -> bool;

        // ── Фоновые задачи ──────────────────────────────────────────────

        /// Поднять задачи вкладки: кэши с диска и сеть. Зовётся один раз,
        /// после того как известен корень Steam.
        #[qinvokable]
        fn start(self: Pin<&mut SteamModel>, webapi_token: &QString);

        /// Забрать всё, что накопили фоновые задачи. Зовёт таймер страницы —
        /// это шов между рантаймом задач и потоком интерфейса.
        #[qinvokable]
        fn pump(self: Pin<&mut SteamModel>);

        /// Погасить задачи и дождаться их — порт `closeEvent`.
        #[qinvokable]
        fn shutdown(self: Pin<&mut SteamModel>);

        /// Номер роли по имени (-1, если роли нет) — проверки без делегата.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &SteamModel, name: &QString) -> i32;

        /// Значение роли строки по пути ролика — проверки без делегата.
        #[qinvokable]
        #[cxx_name = "roleValueOf"]
        fn role_value_of(self: &SteamModel, path: &QString, name: &QString) -> QVariant;

        /// Ширина плашки состояния при такой ширине страницы и такой ширине
        /// текста в одну строку. Считает ядро (`ui_base::toast_width`), поля
        /// и потолок — тоже его. Разметка зовёт напрямую из привязки.
        #[qinvokable]
        #[cxx_name = "toastWidth"]
        fn toast_width(self: &SteamModel, page_width: i32, text_width: i32) -> i32;
    }

    // Объявление трейта обязано быть В МОСТЕ, иначе `impl cxx_qt::Initialize`
    // снаружи не находит конструктора: cxx-qt печатает его по этой строке.
    impl cxx_qt::Initialize for SteamModel {}
    // Подписка на смену скина идёт через `CxxQtThread::queue`, и трейт
    // потока обязан быть объявлен В МОСТЕ.
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
// Роли модели витрины заставок
// ============================================================================

/// Роли начинаются с Qt::UserRole + 1 (Qt::UserRole == 256).
const ROLE_BASE: i32 = 256 + 1;

/// Имена ролей в порядке номеров — единственный источник истины и для
/// roleNames, и для data(), и для roleNumber().
const ROLE_NAMES: [&str; 10] = [
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
];

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

/// Путь на диске → адрес для `Image.source` и `MediaPlayer.source`. Пустой
/// путь остаётся пустым.
///
/// ⚠️ Спецсимволы кодируются процентами, и это не украшательство. Пути
/// роликов приходят из библиотек Steam и из окна «Свой файл…», то есть их
/// пишет не программа: пробел в имени QUrl ещё переживёт, а `#` обрубает
/// путь на месте решётки — файл тогда просто не открывается, и превью
/// молча остаётся статичным кадром. Кодируется байтовое представление
/// UTF-8, поэтому русские имена файлов проходят так же.
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

/// Тёмный ли действующий скин — общий ответ ядра (`ui_base`), а не своя
/// копия критерия: тот же вопрос задаёт страница «Расширений».
fn active_is_dark() -> bool {
    ui_base::active_skin_is_dark()
}

/// Строковое поле записи ролика.
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `QVariant` со списком строк → `Vec<String>`. Приходит из QML/хозяина.
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

/// `QVariant` со списком карт → записи роликов в виде JSON.
///
/// Записи скана приезжают от хозяина страницы как QVariantList из
/// QVariantMap; здесь они переводятся в тот вид, в котором их читает ядро.
///
/// ⚠️ Перевод делает ОБЩИЙ конвертер крейта, а не разбор по типам на месте.
/// Своя разборка тут стояла и определяла тип перебором `QVariant::value<T>`,
/// начиная со строки, — а `canConvert` у Qt разрешающий, и ЛЮБОЕ число
/// «конвертируется» в строку. `appid` приезжал в ядро строкой `"2861720"`,
/// `movie_appid` читал его как 0, и метки предмета из инвентаря не
/// находились НИКОГДА: бейдж карточки и чип фильтра показывали «Points
/// Shop» вместо названия распродажи, а подсказка — общую фразу вместо
/// «Видеозаставка из …». Поймано попиксельной сверкой с оригиналом
/// 01.09.2026; ровно об этой ловушке предупреждает `qvalue::variant_to_json`,
/// где тип определяет сам Qt (`QJsonDocument::fromVariant`).
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
// Движок обоев со стороны хозяина страницы
// ============================================================================

// ============================================================================
// Rust-состояние страницы
// ============================================================================

/// Состояние вкладки «Steam».
#[derive(Default)]
pub struct SteamModelRust {
    /// Номер записи в общем реестре смены скина (`crate::skin`).
    skin_watch: u64,
    env: SteamEnv,
    tasks: Option<Arc<SteamTasks>>,

    steam_root: String,
    sid: String,
    section: String,
    status: String,

    // ── Витрина заставок ──
    movies: Vec<Value>,
    favorites: Vec<String>,
    meta: qs::MetaMap,
    current_path: String,
    hovered_path: String,
    sv_search: String,
    sv_group: String,
    /// Номера показанных роликов в `movies`.
    visible: Vec<usize>,
    /// Путь ролика → готовый кадр-превью.
    posters: BTreeMap<String, String>,
    autostart_on: bool,
    rotate_on: bool,
    sound_on: bool,

    // ── Живое превью ролика в крупной карточке ──
    /// Крупная карточка на виду (ставит разметка).
    preview_shown: bool,
    /// Ролики, которых плеер не осилил, — второй раз не пробуем.
    preview_broken: BTreeSet<String>,
    /// До этого мгновения превью замерло (полноэкранный показ).
    preview_paused_until: Option<Instant>,
    /// Что играет сейчас по решению модели. Кэш, а не второе мнение:
    /// решение считает `preview_path`, а храним его затем, чтобы секундный
    /// опрос молчал, пока ничего не изменилось.
    preview_playing: String,

    // ── Сетка фонов ──
    bg_all: Vec<ProfileBackgroundItem>,
    bg_search: String,
    /// Фильтр вида: все / статичные / анимированные. Живёт до закрытия
    /// программы: своих сохраняемых настроек у вкладки нет (звук видео-обоев
    /// и поиск точно так же сбрасываются при запуске).
    bg_kind: String,
    bg_page: u32,
    bg_cols: u32,
    bg_audio: bool,
    /// id фона → готовая миниатюра.
    thumbs: BTreeMap<String, String>,
    /// id анимированного фона → скачанный ролик (для крупного просмотра).
    movies_full: BTreeMap<String, String>,
    /// Фоны, за которыми крупный просмотр уже ходил: второй раз не просим,
    /// а неудача не оставляет просмотр в вечном «загружается».
    full_asked: BTreeSet<String>,
    full_done: BTreeSet<String>,
    applied_id: String,
    /// Фон, применение которого идёт (для строки состояния по завершении).
    pending_apply: Option<ProfileBackgroundItem>,
    /// Список уже показан из дискового кэша — не затирать его ошибкой сети.
    cache_shown: bool,
    /// Почему секции фонов нечего показать (или почему она ещё не готова):
    /// «Загружаю инвентарь», «Steam не найден», «нет userdata», «обновляю из
    /// кэша», «инвентарь не загрузился», «фонов нет». Пусто — причины нет,
    /// подпись считается по числу фонов.
    ///
    /// ⚠️ Это подпись САМОЙ секции, а не всплывающее сообщение. В оригинале
    /// (`rexon/qt_steam/window.py`) ВСЕ перечисленные строки пишутся в
    /// `_bg_status` — метку, стоящую там же, где «Найдено фонов: 0». Здесь
    /// они уезжали в тост внизу слева поверх соседней секции
    /// «Видеозаставки», где человеку и непонятно, к чему они относятся, и
    /// пропадали через три секунды вместе с тостом, а секция всё это время
    /// врала счётом «Найдено фонов: 0».
    bg_notice: String,
    /// Чем кончился запрос владеемых предметов: от него зависит, есть ли в
    /// списке анимированные фоны вовсе. Строку человеку собирает ядро.
    bg_animated_gap: qs::AnimatedGap,
    /// Надетый в профиле фон — запоминается для повторного запроса списка.
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
    /// Пересчитать показанные ролики по текущим поиску и фильтру.
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

    /// Ролики, отобранные поиском и фильтром.
    fn shown_movies(&self) -> Vec<&Value> {
        self.visible.iter().filter_map(|i| self.movies.get(*i)).collect()
    }

    /// Что показывает крупная карточка: наведённое либо выбранное.
    fn shown_item(&self) -> Option<&Value> {
        if !self.hovered_path.is_empty() {
            if let Some(it) = self.movies.iter().find(|m| s(m, "path") == self.hovered_path) {
                return Some(it);
            }
        }
        self.current_item()
    }

    /// Что ДОЛЖНО играть в крупной карточке прямо сейчас («» — ничего).
    ///
    /// Порт `_sv_preview_target` + `_sv_preview_allowed` оригинала одним
    /// решением: разметке остаётся только сказать, видно ли карточку.
    /// Играем ровно тот ролик, что показывает карточка, — то есть
    /// наведённый, а если наведения нет, выбранный.
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
        // Битый ролик и исчезнувший файл — не повод дёргать плеер.
        if path.is_empty()
            || self.preview_broken.contains(path)
            || !std::path::Path::new(path).is_file()
        {
            return "";
        }
        path
    }

    /// Ролик, который пойдёт при следующем входе.
    fn current_item(&self) -> Option<&Value> {
        self.movies.iter().find(|m| s(m, "path") == self.current_path)
    }

    /// Отобранные поиском фоны.
    fn bg_filtered(&self) -> Vec<&ProfileBackgroundItem> {
        qs::bg_filtered_by(&self.bg_all, &self.bg_search.trim().to_lowercase(), &self.bg_kind)
    }

    /// Полоса пагинации по текущему состоянию.
    fn pager_view(&self) -> qs::PagerView {
        qs::bg_pager(self.bg_filtered().len(), self.bg_page, self.cols())
    }

    /// Столбцов в сетке фонов сейчас (пока ширина неизвестна — минимум).
    fn cols(&self) -> u32 {
        if self.bg_cols == 0 {
            qs::BG_MIN_COLS
        } else {
            self.bg_cols
        }
    }

    /// Задачи вкладки (поднимаются лениво: до `start` их нет).
    fn tasks(&self) -> Option<&Arc<SteamTasks>> {
        self.tasks.as_ref()
    }

    /// Заказать миниатюры ТОЛЬКО для карточек текущей страницы.
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

    /// Заказать кадры-превью для роликов, у которых их ещё нет.
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
// Конструктор
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
        // Где стоит Steam, страница выясняет САМА, а не ждёт хозяина.
        //
        // ⚠️ Это не украшение, а починка гонки. Под Python-хозяином корень
        // приходил снаружи (`qml_window.py:_tell_steam_root` →
        // `setSteamRoot`), но страница берётся за фоновые задачи сама, в
        // `Component.onCompleted`, — то есть ДО того, как хозяин успевает
        // что-либо сказать. Первый заход задач неизбежно видел пустой
        // корень и останавливался на «Steam не найден», а хозяин потом
        // снимал задачи и поднимал заново. Под Rust-хозяином говорить было
        // и вовсе некому: секция фонов профиля оставалась пустой навсегда.
        //
        // Здесь корень известен РАНЬШЕ первой задачи: `initialize`
        // выполняется при создании модели, до `Component.onCompleted`.
        // Правила поиска не дублируются — их знает ядро
        // (`session_splash::Splash::steam_root`: кандидаты, `realpath`,
        // проверка `config/`), исполнитель тот же живой, что у объекта
        // действий. Сеттер `setSteamRoot` остаётся: Python-хозяин зовёт его
        // по-прежнему, и значение придёт то же самое.
        let root = {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
            let mut io = LiveSplashIo::new(&home, rexon_core::installer::PYTHON_FALLBACK_EXE);
            Splash::new(&mut io, &home).steam_root().unwrap_or_default()
        };
        self.as_mut().rust_mut().steam_root = root;

        // Скин сменили на лету — палитру пересчитать. Толчок приходит из
        // общего реестра (`crate::skin`), а не отсюда.
        let watch = crate::skin::watch(self.qt_thread(), |page| page.palette_changed());
        self.as_mut().rust_mut().skin_watch = watch;
        // ⚠️ Начальный текст — подпись САМОЙ секции фонов, а не тост. В
        // оригинале это и есть текст метки при её создании
        // (`rexon/qt_steam/window.py`, `QLabel("⏳ Загружаю инвентарь Steam…")`).
        // Тостом он всплывал внизу слева поверх соседней секции
        // «Видеозаставки», где человеку непонятно, к чему относится, и гас
        // через три секунды, а секция фонов тем временем врала «Найдено
        // фонов: 0», ещё ничего не загрузив.
        self.as_mut().set_bg_notice(&qs::bg_loading_text());
    }
}

// ============================================================================
// Свойства: неизменные подписи
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

    /// Оформление вкладки, которого нет в палитре скина.
    ///
    /// ⚠️ КАЖДАЯ краска проходит через [`ui_base::css_color_to_qml`]. У ядра
    /// они набраны так, как их понимает QSS виджетов (`rgba(r,g,b,a)`), а
    /// QML такую запись читает как недопустимый цвет и рисует ЧЁРНЫМ, не
    /// сказав ни слова. Отдавать их сюда «как есть» нельзя.
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

        // Всплывающая плашка состояния: краски и поля — та же дверь, что у
        // «Настроек» и у виджетной плашки (`crate::toast`).
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

    /// Через сколько гаснет плашка состояния, мс.
    pub fn toast_timeout_ms(&self) -> i32 {
        crate::toast::timeout_ms()
    }

    /// Ширина плашки состояния под текст такой ширины.
    pub fn toast_width(&self, page_width: i32, text_width: i32) -> i32 {
        crate::toast::width(page_width, text_width)
    }

    /// Палитра действующего скина (по умолчанию steam_dark, правило #17).
    /// Оформление QML-вкладки берётся отсюда, а не пишется заново.
    pub fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        // Затемнение фона под модальным окном — одно на продукт.
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        // Подложка неактивной части ползунка (подвкладка «Витрины профиля») —
        // тот же цвет, что у вкладки «Настройки». Без ключа `RexonSlider`
        // рисует полосу чёрной.
        out.insert(
            QString::from("track"),
            qstr(&ui_base::css_color_to_qml(&ui_base::neutral_rgba(0.18))),
        );
        out
    }
}

// ============================================================================
// Свойства: меняющееся
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
        // Тёмный скин по умолчанию (правило #17): решает не разметка, а
        // яркость подложки палитры.
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

    /// Все фоны профиля для выбора фона предпросмотра витрины. У
    /// анимированного фона `url` ведёт на ролик, поэтому берётся `preview` —
    /// статичная картинка того же размера.
    pub fn bg_choices(&self) -> QList<QVariant> {
        let me = self.rust();
        let mut out = QList::<QVariant>::default();
        for it in &me.bg_all {
            let still = if it.preview.is_empty() { it.url.clone() } else { it.preview.clone() };
            let thumb = me.thumbs.get(&it.id).cloned().unwrap_or_default();
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(&it.id)),
                ("name", qstr(if it.name.is_empty() { "?" } else { &it.name })),
                // Отметка анимированного — та же, что на карточке сетки.
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

    /// Карточки крупного просмотра — весь отобранный список.
    ///
    /// Картинка фона у Steam одна и сразу полноразмерная: файл, скачанный
    /// для миниатюры, и есть полный размер. Поэтому у статичного фона с уже
    /// скачанной миниатюрой ждать нечего; остальное просмотр попросит сам
    /// (`requestBgFull`) и получит через `bgViewerSources`.
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
            // Статичному хватает картинки; анимированному нужен ещё ролик.
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
        // У анимированного `url` — ролик; у статичного он же картинка.
        let movie_url = if animated { item.url.clone() } else { String::new() };
        tasks.start_full(wanted, if have_still { String::new() } else { still_url }, movie_url);
    }

    /// Карточки крупного просмотра заставок: показанные ролики. Кадр-превью
    /// виден сразу, сам ролик играет из файла на диске.
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
        // Решает ядро: подпись секции — это причина, если она есть, и счёт
        // фонов, если её нет.
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
        ])
    }
}

// ============================================================================
// Модель витрины заставок
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

    /// Значение одной роли для записи ролика. Общее место для `data`,
    /// `roleValueOf` и проверок — второй копии правил не заводим.
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
        // Заимствование `item` живёт от `self.rust()`, поэтому берём копию:
        // `role_value` снова спрашивает `rust()`.
        let copy = item.clone();
        self.role_value(&copy, &name.to_string())
    }
}

// ============================================================================
// Действия
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
            // Ничего не выбрано — берём первый найденный, как оригинал.
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
        // Ушли с витрины — наведение снимается: превью крутится только на
        // своей подвкладке (как в оригинале).
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
        // Выбор меняет подсветку карточек — сбрасываем модель целиком.
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
        // Избранное меняет и звёздочки, и состав чипов, и, если смотрят
        // именно избранное, — состав витрины.
        self.as_mut().reset_showcase();
    }

    // ── Живое превью ролика ─────────────────────────────────────────────

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
        // Молчим, пока решение то же: иначе крупная карточка перестраивалась
        // бы раз в секунду просто оттого, что за окном никто ничего не делал.
        if self.rust().preview_path() != self.rust().preview_playing.as_str() {
            self.hero_and_preview();
        }
    }

    pub fn set_status_text(mut self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        self.as_mut().set_status(&text);
    }

    pub fn autostart_answered(mut self: Pin<&mut Self>, on: bool, reason: &QString) {
        let reason = reason.to_string();
        let Some(text) = qs::autostart_refusal_text(on, &reason) else {
            return;
        };
        // Сырая причина — журналу: покупателю она ничего не скажет, а
        // разбирающемуся в отказе нужна дословно.
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

    /// Порт `QWindow.isExposed()` из оригинала (`_sv_preview_allowed`).
    ///
    /// Приведение указателя — то же, что сделал бы `static_cast<QWindow*>`:
    /// `QObject` — ПЕРВАЯ база `QWindow` (`class QWindow : public QObject,
    /// public QSurface`), смещения у первой базы нет, адрес совпадает.
    ///
    /// # Safety
    /// `window` — живой QWindow либо ноль.
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

    // ── Фоны профиля ────────────────────────────────────────────────────

    /// Ширина полосы приходит СТРОКОЙ: инвокабл с числом в подписи снаружи
    /// не зовётся (`::std::int32_t` в метаобъекте не имеет метатипа).
    pub fn set_bg_viewport_width(mut self: Pin<&mut Self>, width: &QString) {
        let px: i32 = width.to_string().trim().parse().unwrap_or(0);
        let cols = ui_base::grid_columns(px, qs::BG_CARD_W, 10, 0, qs::BG_MIN_COLS, qs::BG_MAX_COLS);
        let old = self.rust().cols();
        if cols == old {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            // Что было первым на экране, тем и остаётся.
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
            // Поиск по всему инвентарю → сброс на страницу 0.
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
            // Отбор по всему инвентарю → сброс на страницу 0.
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
        // Причина снимается сразу: кнопка нажата, ответ ещё не пришёл —
        // прежний приговор уже не про этот запрос.
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
        // ⭐ Настоящий движок: общий исполнитель обоев ядра (тот же, что у
        // «Настроек»). До 15.09.2026 здесь стояла заглушка, и «Применить»
        // отвечал «движок обоев недоступен» в обоих хозяевах страницы.
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

    // ── Фоновые задачи ──────────────────────────────────────────────────

    pub fn start(mut self: Pin<&mut Self>, webapi_token: &QString) {
        if self.rust().tasks.is_some() {
            return;
        }
        let env = self.rust().env.clone();
        let Some(tasks) = SteamTasks::new(env.clone()) else {
            return;
        };
        self.as_mut().rust_mut().tasks = Some(Arc::new(tasks));

        let root = self.rust().steam_root.clone();
        if root.is_empty() {
            self.as_mut().set_bg_notice(&qs::bg_no_steam());
            return;
        }
        let Some(sid) = steam_workers::detect_account(&root) else {
            self.as_mut().set_bg_notice(&qs::bg_no_account());
            return;
        };
        // Причину не стираем: до ответа сети в подписи честно стоит
        // «⏳ Загружаю инвентарь Steam…», как в оригинале.
        let equipped = steam_workers::read_equipped_background(&root);

        {
            let mut me = self.as_mut().rust_mut();
            me.sid = sid.clone();
            me.bg_equipped = equipped.clone();
            me.applied_id = steam_workers::read_applied_id(&env);
            me.meta = steam_workers::read_meta_cache(&env, &sid);
        }
        // Дисковый кэш списка: витрина наполняется МГНОВЕННО, сеть тихо
        // обновит её следом (stale-while-revalidate оригинала).
        if let Some(cached) = steam_workers::read_inventory_cache(&env, &sid) {
            let shown = cached.len();
            {
                let mut me = self.as_mut().rust_mut();
                me.bg_all = rexon_core::steam_profile_items::finalize_items(cached, equipped.clone());
                me.cache_shown = true;
                me.bg_page = 0;
            }
            self.as_mut().refresh_backgrounds();
            // Порядок как в оригинале: сперва показать кэш, потом подписать
            // его «обновляю». Подпись — секции, не тост.
            let text = qs::bg_refreshing_text(shown);
            self.as_mut().set_bg_notice(&text);
        }
        // Метки из кэша меняют бейджи и чипы витрины.
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
// Внутреннее: приём событий и пересчёты
// ============================================================================

impl qobject::SteamModel {
    /// Разложить одно событие фоновой задачи по состоянию страницы.
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
                // Метки пришли — у части роликов появилось событие
                // распродажи, значит и группы фильтра стали другими.
                self.as_mut().reset_showcase();
            }
            SteamEvent::Poster { id, png } => {
                self.as_mut().rust_mut().posters.insert(id, png);
                // Кадр меняет ОДНУ роль показанной строки; полный сброс
                // модели тут был бы дёрганьем всей сетки на каждый кадр.
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
        }
    }

    /// Порт `_on_inventory_done`.
    fn on_inventory(mut self: Pin<&mut Self>, items: Vec<ProfileBackgroundItem>, error: String) {
        let cache_shown = self.rust().cache_shown && !self.rust().bg_all.is_empty();
        if items.is_empty() {
            // Сеть/приватность подвели либо фонов нет. Показанный из кэша
            // список НЕ стираем — оставляем прошлый.
            if cache_shown {
                // Причины нет — подпись возвращается к счёту показанных
                // фонов, ровно как в оригинале.
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

    /// Порт `_on_apply_done`.
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

    /// Пересобрать витрину заставок целиком: состав строк и их порядок
    /// меняются разом — это и есть тот случай, ради которого в Qt есть reset.
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

    /// Пересобрать страницу фонов и заказать её миниатюры.
    fn refresh_backgrounds(mut self: Pin<&mut Self>) {
        // Страница могла выйти за пределы списка — нормализуем её тем же
        // расчётом, что рисует полосу пагинации.
        let page = self.rust().pager_view().page;
        self.as_mut().rust_mut().bg_page = page;
        self.as_mut().backgrounds_changed();
        self.rust().request_page_thumbs();
    }

    /// Крупная карточка изменилась — а вместе с ней и ролик, который в ней
    /// играет: это ОДИН и тот же ролик. Порт связки `_sv_update_hero` +
    /// `_sv_sync_preview` оригинала — там она тоже звалась парой.
    fn hero_and_preview(mut self: Pin<&mut Self>) {
        let want = self.rust().preview_path().to_string();
        let changed = self.rust().preview_playing != want;
        self.as_mut().rust_mut().preview_playing = want;
        // Сигнал ролика — только по настоящей смене: он поднимает
        // проигрыватель, и дёргать его на каждое движение мыши незачем.
        if changed {
            self.as_mut().preview_source_changed();
        }
        self.as_mut().hero_changed();
    }

    /// Строка состояния (общий всплывающий тост оригинала).
    fn set_status(mut self: Pin<&mut Self>, text: &str) {
        self.as_mut().rust_mut().status = text.to_string();
        self.as_mut().status_changed();
    }

    /// Причина, по которой секции фонов нечего показать. Пустая строка —
    /// причины нет, подпись возвращается к счёту фонов.
    fn set_bg_notice(mut self: Pin<&mut Self>, text: &str) {
        if self.rust().bg_notice == text {
            return;
        }
        self.as_mut().rust_mut().bg_notice = text.to_string();
        self.as_mut().backgrounds_changed();
    }
}
