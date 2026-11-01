// WallpaperEngineModel — шелл вкладки «Wallpaper Engine» на Rust + cxx-qt + QML.
//
// Перенесена на QML по общей схеме вкладок. Это ПЕРВЫЙ шаг переноса вкладки:
// Navigation Rail и пустое место страницы. Содержимое пунктов — вендорный
// QML waywallen (MIT, (c) hypengw; атрибуция в NOTICE) — приезжает
// следующими шагами, поэтому здесь его ещё нет вовсе.
//
// ⚠️ Чем эта вкладка отличается от шести уже перенесённых. У них страница
// НАША целиком: разметка в `qml/<Имя>Page.qml`, данные в соседнем мосте. У
// вкладки обоев страницы вендорные и давно на QML — на Python оставался
// только шелл ВОКРУГ них: рейл, состояния «подключение…»/«нет демона»/
// «модули не собраны» и скрытая страница «Разработчик». Переносится именно
// шелл, и переносить его есть с чего: все его решения и подписи уже лежат в
// `rexon_core::we_window` (порт сделан коммитом 8151d7f).
//
// Что здесь есть и чего нет:
//   * ЕСТЬ оболочка QObject: пункты рейла готовыми записями, выбранный
//     пункт, признаки «плагины собраны» и «режим разработчика», палитра
//     скина и подписка на его смену;
//   * НЕТ ни одной строки продуктовых решений — состав рейла, соответствие
//     «пункт → вендорная страница», подписи кнопок, пометка расхождения
//     паритета и текст заглушки считает ядро (`we_window::build_rail`,
//     `rail_button_label`, `TEXT_NO_QML_MODULES`).
//
// ⚠️ Тип зовётся `WallpaperEngineModel`, а не `WallpaperEnginePage`: .qml
// модуля сами становятся QML-типами по имени файла, и `WallpaperEnginePage`
// занят `qml/WallpaperEnginePage.qml`. Совпадение имён даёт «Ambiguous type
// detected» и страницу, которая не находит собственный источник данных.

/// Мост cxx-qt: шелл вкладки «Wallpaper Engine».
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // ── Размеры и подписи рейла. CONSTANT: за время жизни вкладки они
        // не меняются, это константы ядра.
        #[qproperty(i32, rail_width, cxx_name = "railWidth",
                    READ = rail_width, CONSTANT)]
        #[qproperty(QString, rail_logo, cxx_name = "railLogo",
                    READ = rail_logo, CONSTANT)]
        #[qproperty(i32, rail_button_min_height, cxx_name = "railButtonMinHeight",
                    READ = rail_button_min_height, CONSTANT)]
        // Форма и кегль кнопки пункта. У виджетной версии их ставит таблица
        // стилей из ядра (`we_window::rail_qss`), и здесь берутся ТЕ ЖЕ
        // числа: рейл обязан выглядеть одинаково из обеих версий вкладки.
        #[qproperty(i32, rail_button_radius, cxx_name = "railButtonRadius",
                    READ = rail_button_radius, CONSTANT)]
        #[qproperty(i32, rail_button_font_size, cxx_name = "railButtonFontSize",
                    READ = rail_button_font_size, CONSTANT)]
        // Подсветка ВЫБРАННОГО пункта — тоже готовой краской `#AARRGGBB`
        // из ядра, а не долей поверх акцента: доля `0.25` у виджетов
        // усекается до 63/255, а сцена QML считала по самой доле, и
        // подсветка выходила на уровень светлее (замер 04.09).
        #[qproperty(QString, rail_selected_color,
                    cxx_name = "railSelectedColor",
                    READ = rail_selected_color, NOTIFY = palette_changed)]
        // Подложка полосы рейла — готовой краской `#AARRGGBB` из ядра, а
        // не долей: доля у виджетов и у сцены QML округляется по-разному,
        // и полоса выходила на уровень светлее (замер 04.09).
        #[qproperty(QString, rail_background_color,
                    cxx_name = "railBackgroundColor",
                    READ = rail_background_color, NOTIFY = palette_changed)]
        // Кегль и насыщенность буквы-логотипа: те же числа, какими её
        // красит таблица стилей виджетной версии (`logo_qss`).
        #[qproperty(i32, logo_font_size, cxx_name = "logoFontSize",
                    READ = logo_font_size, CONSTANT)]
        #[qproperty(i32, logo_font_weight, cxx_name = "logoFontWeight",
                    READ = logo_font_weight, CONSTANT)]
        // Числа стартовых экранов («подключение…», «демон недоступен»):
        // кегли, зазор и поля колонки. У виджетной версии их ставят те же
        // константы ядра через `wewin_constants`, здесь — те же числа.
        #[qproperty(i32, state_text_font_size, cxx_name = "stateTextFontSize",
                    READ = state_text_font_size, CONSTANT)]
        #[qproperty(i32, no_daemon_icon_font_size, cxx_name = "noDaemonIconFontSize",
                    READ = no_daemon_icon_font_size, CONSTANT)]
        #[qproperty(i32, state_column_spacing, cxx_name = "stateColumnSpacing",
                    READ = state_column_spacing, CONSTANT)]
        #[qproperty(i32, state_column_margin, cxx_name = "stateColumnMargin",
                    READ = state_column_margin, CONSTANT)]
        // Прозрачность ПРИГЛУШЁННОГО текста (`on_surface_variant`
        // оригинала): текст ожидания у виджетной версии — роль `Text`
        // с этой долей, а не отдельная краска скина.
        #[qproperty(f64, variant_alpha, cxx_name = "variantAlpha",
                    READ = variant_alpha, CONSTANT)]
        // Тёмный ли действующий скин. Вопрос решает ядро тем же
        // критерием, что и весь продукт: подложка рейла считается от него,
        // а «steam_dark в названии» ответом не является — скин выбирает
        // покупатель, и «Светлый» обязан считаться светлым.
        #[qproperty(bool, skin_dark, cxx_name = "skinDark",
                    READ = skin_dark, NOTIFY = palette_changed)]
        // Чем красить вендорный интерфейс: скином Rexon (по умолчанию) или
        // схемой самого waywallen (скин «Как в waywallen»). Решает ядро
        // (`we_window::vendor_theme`); шелл только кладёт краски в Material.
        #[qproperty(bool, vendor_theme_follow, cxx_name = "vendorThemeFollow",
                    READ = vendor_theme_follow, NOTIFY = palette_changed)]
        #[qproperty(QString, vendor_accent, cxx_name = "vendorAccent",
                    READ = vendor_accent, NOTIFY = palette_changed)]
        #[qproperty(QString, vendor_background, cxx_name = "vendorBackground",
                    READ = vendor_background, NOTIFY = palette_changed)]
        #[qproperty(QString, vendor_text, cxx_name = "vendorText",
                    READ = vendor_text, NOTIFY = palette_changed)]
        // Роли схемы Material красками скина, объектом JSON (`{}` у схемы
        // самого waywallen) — для `MD.Token.color.overrides`.
        #[qproperty(QString, vendor_roles, cxx_name = "vendorRoles",
                    READ = vendor_roles, NOTIFY = palette_changed)]
        // Текст заглушки, когда плагины вендорного интерфейса не собраны.
        #[qproperty(QString, no_qml_modules_text, cxx_name = "noQmlModulesText",
                    READ = no_qml_modules_text, CONSTANT)]

        // ── Стартовые состояния вкладки: «подключение…», «нет демона»,
        // «страницы». Номера состояний, подписи, иконка и текст причины —
        // из ядра (`we_window::STATE_*`, `TEXT_*`, `no_daemon_text`). Своей
        // подписи для человека здесь нет ни одной: продукт обязан говорить
        // одними и теми же словами из виджетной вкладки и из этой.
        #[qproperty(i32, state, READ = state, NOTIFY = state_changed)]
        #[qproperty(i32, state_connecting, cxx_name = "stateConnecting",
                    READ = state_connecting, CONSTANT)]
        #[qproperty(i32, state_no_daemon, cxx_name = "stateNoDaemon",
                    READ = state_no_daemon, CONSTANT)]
        #[qproperty(i32, state_pages, cxx_name = "statePages",
                    READ = state_pages, CONSTANT)]
        #[qproperty(QString, connecting_text, cxx_name = "connectingText",
                    READ = connecting_text, CONSTANT)]
        #[qproperty(QString, no_daemon_icon, cxx_name = "noDaemonIcon",
                    READ = no_daemon_icon, CONSTANT)]
        #[qproperty(QString, retry_daemon_label, cxx_name = "retryDaemonLabel",
                    READ = retry_daemon_label, CONSTANT)]
        // Причина живёт вместе с состоянием, поэтому сигнал у них общий:
        // отдельный сигнал на текст означал бы два толчка на один переход.
        #[qproperty(QString, no_daemon_text, cxx_name = "noDaemonText",
                    READ = no_daemon_text, NOTIFY = state_changed)]
        // Сколько ждать ответа `health` — сторож заводит хозяин страницы.
        #[qproperty(i32, health_timeout_ms, cxx_name = "healthTimeoutMs",
                    READ = health_timeout_ms, CONSTANT)]
        // Общий срок ожидания движка: сторож экрана «Подключение…» заводит
        // сама разметка, а срок — из ядра (`we_window::CONNECT_TIMEOUT_MS`).
        #[qproperty(i32, connect_timeout_ms, cxx_name = "connectTimeoutMs",
                    READ = connect_timeout_ms, CONSTANT)]

        // ── Среда, в которой открылась вкладка. Отвечено один раз при
        // создании объекта: ни плагины, ни маркер dev-режима за время
        // работы Rexon не появляются и не исчезают.
        #[qproperty(bool, qml_available, cxx_name = "qmlAvailable",
                    READ = qml_available, CONSTANT)]
        #[qproperty(bool, dev_mode, cxx_name = "devMode",
                    READ = dev_mode, CONSTANT)]

        // ── Сам рейл. Пункты меняются вместе с пометкой расхождения
        // паритета: она живёт прямо в подписи кнопки «Разраб.».
        #[qproperty(QList_QVariant, rail_items, cxx_name = "railItems",
                    READ = rail_items, NOTIFY = rail_changed)]
        #[qproperty(i32, stub_index, cxx_name = "stubIndex",
                    READ = stub_index, NOTIFY = rail_changed)]
        #[qproperty(i32, dev_index, cxx_name = "devIndex",
                    READ = dev_index, NOTIFY = rail_changed)]
        #[qproperty(bool, parity_drift, cxx_name = "parityDrift",
                    READ = parity_drift, WRITE = set_parity_drift,
                    NOTIFY = rail_changed)]
        #[qproperty(i32, current_index, cxx_name = "currentIndex",
                    READ = current_index, WRITE = set_current_index,
                    NOTIFY = current_index_changed)]

        // Каталог вендоренного интерфейса waywallen (`qml_ui/` пакета).
        //
        // ⚠️ Значение приходит от хозяина страницы, а не считается здесь:
        // вендорный QML лежит ВНУТРИ пакета `rexon/`, и адрес загруженной
        // `.so` о раскладке пакета не говорит ничего — она доставляется в
        // каталог QML-модулей. Тот же довод, что у `we_env::pkg_dir`.
        #[qproperty(QString, vendor_qml_dir, cxx_name = "vendorQmlDir",
                    READ = vendor_qml_dir, WRITE = set_vendor_qml_dir,
                    NOTIFY = vendor_qml_dir_changed)]

        // Адрес шелла вокруг вендорной страницы (`PageHost.qml`) — НАШ
        // файл рядом с вендоренным интерфейсом. Пусто, пока хозяин не
        // назвал каталог.
        #[qproperty(QUrl, page_host_url, cxx_name = "pageHostUrl",
                    READ = page_host_url, NOTIFY = vendor_qml_dir_changed)]

        // Палитра действующего скина (правило #17). Не CONSTANT: скин
        // меняют на лету, и без сигнала открытая вкладка осталась бы в
        // цветах прошлого.
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]

        // ── «Чей фон»: полоса над страницей при живом чужом шелле
        // (Noctalia, DankMaterialShell). Решение, текст и подписи — ядро
        // (`rexon_core::desktop_shell`); здесь только раскладка по
        // свойствам и два действия по кнопкам.
        #[qproperty(bool, shell_banner_visible, cxx_name = "shellBannerVisible",
                    READ = shell_banner_visible, NOTIFY = shell_state_changed)]
        #[qproperty(QString, shell_banner_text, cxx_name = "shellBannerText",
                    READ = shell_banner_text, NOTIFY = shell_state_changed)]
        #[qproperty(bool, shell_can_give, cxx_name = "shellCanGive",
                    READ = shell_can_give, NOTIFY = shell_state_changed)]
        #[qproperty(bool, shell_can_return, cxx_name = "shellCanReturn",
                    READ = shell_can_return, NOTIFY = shell_state_changed)]
        #[qproperty(QString, shell_give_label, cxx_name = "shellGiveLabel",
                    READ = shell_give_label, CONSTANT)]
        #[qproperty(QString, shell_return_label, cxx_name = "shellReturnLabel",
                    READ = shell_return_label, CONSTANT)]
        // Итог последнего нажатия (успех или отказ словами ядра) и признак
        // отказа — чтобы разметка красила его иначе. Пусто — нажатий не было.
        #[qproperty(QString, shell_action_message, cxx_name = "shellActionMessage",
                    READ = shell_action_message, NOTIFY = shell_state_changed)]
        #[qproperty(bool, shell_action_failed, cxx_name = "shellActionFailed",
                    READ = shell_action_failed, NOTIFY = shell_state_changed)]
        type WallpaperEngineModel = super::WallpaperEngineModelRust;
    }

    // ── Сигналы ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        /// Скин сменили на лету — палитру пересчитать. Толчок приходит из
        /// общего реестра (`crate::skin`), а не отсюда.
        #[qsignal]
        #[cxx_name = "paletteChanged"]
        fn palette_changed(self: Pin<&mut WallpaperEngineModel>);

        /// Состав или подписи пунктов рейла изменились.
        #[qsignal]
        #[cxx_name = "railChanged"]
        fn rail_changed(self: Pin<&mut WallpaperEngineModel>);

        #[qsignal]
        #[cxx_name = "currentIndexChanged"]
        fn current_index_changed(self: Pin<&mut WallpaperEngineModel>);

        #[qsignal]
        #[cxx_name = "vendorQmlDirChanged"]
        fn vendor_qml_dir_changed(self: Pin<&mut WallpaperEngineModel>);

        /// Вкладка перешла на другой экран: «подключение…», «нет демона»
        /// (вместе с причиной) или «страницы».
        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut WallpaperEngineModel>);

        /// Пересчитано «чей фон» (или нажата одна из двух кнопок).
        #[qsignal]
        #[cxx_name = "shellStateChanged"]
        fn shell_state_changed(self: Pin<&mut WallpaperEngineModel>);
    }

    // ── Чтение свойств ────────────────────────────────────────────────────
    extern "RustQt" {
        #[cxx_name = "railWidth"]
        fn rail_width(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "railLogo"]
        fn rail_logo(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "railButtonMinHeight"]
        fn rail_button_min_height(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "railButtonRadius"]
        fn rail_button_radius(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "railButtonFontSize"]
        fn rail_button_font_size(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "railSelectedColor"]
        fn rail_selected_color(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "railBackgroundColor"]
        fn rail_background_color(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "logoFontSize"]
        fn logo_font_size(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "logoFontWeight"]
        fn logo_font_weight(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "stateTextFontSize"]
        fn state_text_font_size(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "noDaemonIconFontSize"]
        fn no_daemon_icon_font_size(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "stateColumnSpacing"]
        fn state_column_spacing(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "stateColumnMargin"]
        fn state_column_margin(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "variantAlpha"]
        fn variant_alpha(self: &WallpaperEngineModel) -> f64;
        #[cxx_name = "skinDark"]
        fn skin_dark(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "vendorThemeFollow"]
        fn vendor_theme_follow(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "vendorAccent"]
        fn vendor_accent(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "vendorBackground"]
        fn vendor_background(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "vendorText"]
        fn vendor_text(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "vendorRoles"]
        fn vendor_roles(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "noQmlModulesText"]
        fn no_qml_modules_text(self: &WallpaperEngineModel) -> QString;
        fn state(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "stateConnecting"]
        fn state_connecting(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "stateNoDaemon"]
        fn state_no_daemon(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "statePages"]
        fn state_pages(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "connectingText"]
        fn connecting_text(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "noDaemonIcon"]
        fn no_daemon_icon(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "retryDaemonLabel"]
        fn retry_daemon_label(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "noDaemonText"]
        fn no_daemon_text(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "healthTimeoutMs"]
        fn health_timeout_ms(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "connectTimeoutMs"]
        fn connect_timeout_ms(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "qmlAvailable"]
        fn qml_available(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "devMode"]
        fn dev_mode(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "railItems"]
        fn rail_items(self: &WallpaperEngineModel) -> QList_QVariant;
        #[cxx_name = "stubIndex"]
        fn stub_index(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "devIndex"]
        fn dev_index(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "parityDrift"]
        fn parity_drift(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "setParityDrift"]
        fn set_parity_drift(self: Pin<&mut WallpaperEngineModel>, value: bool);
        #[cxx_name = "currentIndex"]
        fn current_index(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "setCurrentIndex"]
        fn set_current_index(self: Pin<&mut WallpaperEngineModel>, value: i32);
        fn palette(self: &WallpaperEngineModel) -> QMap_QString_QVariant;
        #[cxx_name = "vendorQmlDir"]
        fn vendor_qml_dir(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "setVendorQmlDir"]
        fn set_vendor_qml_dir(self: Pin<&mut WallpaperEngineModel>, value: &QString);
        #[cxx_name = "pageHostUrl"]
        fn page_host_url(self: &WallpaperEngineModel) -> QUrl;
        #[cxx_name = "shellBannerVisible"]
        fn shell_banner_visible(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "shellBannerText"]
        fn shell_banner_text(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "shellCanGive"]
        fn shell_can_give(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "shellCanReturn"]
        fn shell_can_return(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "shellGiveLabel"]
        fn shell_give_label(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "shellReturnLabel"]
        fn shell_return_label(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "shellActionMessage"]
        fn shell_action_message(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "shellActionFailed"]
        fn shell_action_failed(self: &WallpaperEngineModel) -> bool;
    }

    // ── API для QML ───────────────────────────────────────────────────────
    extern "RustQt" {
        /// Имя файла вендорной страницы для пункта `idx`; пусто — пункт не
        /// вендорный (заглушка «модули не собраны» либо «Разработчик»).
        #[qinvokable]
        #[cxx_name = "qmlForIndex"]
        fn qml_for_index(self: &WallpaperEngineModel, idx: i32) -> QString;

        /// Идентификатор пункта `idx` (`wallpapers_qml`, `dev`, …); пусто —
        /// такого пункта нет. Нужен проверкам: они ведут страницу по
        /// именам пунктов, а не по номерам, которые зависят от dev-режима.
        #[qinvokable]
        #[cxx_name = "railItemId"]
        fn rail_item_id(self: &WallpaperEngineModel, idx: i32) -> QString;

        /// Показать экран ожидания движка.
        #[qinvokable]
        #[cxx_name = "showConnecting"]
        fn show_connecting(self: Pin<&mut WallpaperEngineModel>);

        /// Показать экран отказа с причиной.
        ///
        /// `reason` — сырое объяснение от клиента демона, а не готовая
        /// подпись: строку для человека из него складывает ядро.
        #[qinvokable]
        #[cxx_name = "showNoDaemon"]
        fn show_no_daemon(self: Pin<&mut WallpaperEngineModel>, reason: &QString);

        /// То же, но причина одна и известна заранее: демон подключился и
        /// молчит на проверку готовности.
        #[qinvokable]
        #[cxx_name = "showHealthTimeout"]
        fn show_health_timeout(self: Pin<&mut WallpaperEngineModel>);

        /// Истёк ОБЩИЙ срок ожидания движка ([`ww::CONNECT_TIMEOUT_MS`]):
        /// показать отказ с кнопкой повтора.
        ///
        /// Зовёт сторож разметки, потому что ждёт ИМЕННО ЭКРАН: у путей,
        /// которые на нём застревают (сокет не открылся, бесконечный
        /// реконнект), своего сторожа нет ни в транспорте, ни в ядре.
        #[qinvokable]
        #[cxx_name = "showConnectTimeout"]
        fn show_connect_timeout(self: Pin<&mut WallpaperEngineModel>);

        /// Разобрать ответ `health` подключившегося демона и перейти на
        /// нужный экран. Вердикт выносит ядро (`we_window::health_verdict`).
        #[qinvokable]
        #[cxx_name = "applyHealth"]
        fn apply_health(self: Pin<&mut WallpaperEngineModel>, state: &QString);

        /// Адрес вендорной страницы пункта `idx` для `Loader`. Пустой
        /// адрес — грузить нечего: пункт не вендорный либо каталог
        /// интерфейса хозяин ещё не назвал, и `Loader` остаётся пустым.
        #[qinvokable]
        #[cxx_name = "vendorPageUrl"]
        fn vendor_page_url(self: &WallpaperEngineModel, idx: i32) -> QUrl;

        /// Нужна ли странице пункта `idx` вендорная ШАПКА (`MD.AppBar` с
        /// её заголовком и кнопками действий). Ответ держит ядро
        /// (`we_window::RailItem::header`), потому что он один на обе
        /// версии вкладки.
        #[qinvokable]
        #[cxx_name = "vendorPageHeader"]
        fn vendor_page_header(self: &WallpaperEngineModel, idx: i32) -> bool;

        /// Пересчитать «чей фон» по живому сеансу (зовёт разметка при
        /// открытии вкладки и по таймеру: шелл могут запустить и погасить,
        /// пока окно открыто).
        #[qinvokable]
        #[cxx_name = "refreshShellState"]
        fn refresh_shell_state(self: Pin<&mut WallpaperEngineModel>);

        /// «Отдать обои Rexon»: с согласия человека (нажатие и есть
        /// согласие) выключить обои шелла, запомнив прежнее значение.
        #[qinvokable]
        #[cxx_name = "giveWallpaperToRexon"]
        fn give_wallpaper_to_rexon(self: Pin<&mut WallpaperEngineModel>);

        /// «Вернуть оболочке»: поставить на место то, что было до Rexon.
        #[qinvokable]
        #[cxx_name = "returnWallpaperToShell"]
        fn return_wallpaper_to_shell(self: Pin<&mut WallpaperEngineModel>);
    }

    // Объявление трейта обязано быть В МОСТЕ, иначе `impl cxx_qt::Initialize`
    // снаружи не находит конструктора: cxx-qt печатает его по этой строке.
    impl cxx_qt::Initialize for WallpaperEngineModel {}
    // Подписка на смену скина идёт через `CxxQtThread::queue`, и трейт
    // потока обязан быть объявлен В МОСТЕ.
    impl cxx_qt::Threading for WallpaperEngineModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QUrl, QVariant};

use rexon_core::desktop_shell as shell;
use rexon_core::we_window as ww;
use rexon_core::{ui_base, utils};

use crate::we_env;

// ============================================================================
// Чистый слой: среда вкладки (Qt здесь нет)
// ============================================================================

/// Включён ли скрытый режим разработчика.
///
/// Развилка целиком в ядре (`we_window::dev_mode`): здесь только чтение
/// окружения и одна проверка файла на диске — тот же порядок, что у
/// `is_dev_mode()` в `rexon/qt_wallpaper_engine/window.py`.
pub fn dev_mode_now() -> bool {
    let raw = std::env::var(ww::DEV_ENV_VAR).unwrap_or_default();
    let marker = utils::expand_user(
        ww::DEV_MARKER_PATH,
        &we_env::home(),
        we_env::xdg_config_home().as_deref(),
    );
    ww::dev_mode(&raw, std::path::Path::new(&marker).is_file())
}

/// Готовые записи пунктов рейла для QML.
///
/// Отдельной функцией — чтобы состав пунктов проверялся без Qt: подпись
/// кнопки собирает ядро, а здесь только раскладка по ключам.
pub fn rail_entries(plan: &ww::RailPlan, drift: bool) -> Vec<RailEntry> {
    plan.items
        .iter()
        .enumerate()
        .map(|(idx, item)| RailEntry {
            id: item.id.clone(),
            icon: item.icon.clone(),
            label: item.label.clone(),
            qml: item.qml.clone(),
            // Пометка расхождения паритета живёт в подписи и только у
            // пункта «Разработчик» — как в виджетной версии. Пункт узнаётся
            // по номеру из плана, а не по строке `dev`: имя пункта решает
            // ядро, и вторая его копия здесь разъехалась бы молча.
            button_label: ww::rail_button_label(
                &item.icon,
                &item.label,
                drift && plan.dev_index == Some(idx),
            ),
        })
        .collect()
}

/// Одна запись пункта рейла в том виде, в каком её читает QML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailEntry {
    /// Идентификатор пункта (`wallpapers_qml`, `dev`, …).
    pub id: String,
    /// Эмодзи-иконка.
    pub icon: String,
    /// Подпись под иконкой.
    pub label: String,
    /// Имя файла вендорной страницы; пусто для не-вендорных пунктов.
    pub qml: String,
    /// Готовая подпись кнопки (иконка, перевод строки, подпись, пометка).
    pub button_label: String,
}

// ============================================================================
// Мелочи перевода в Qt
// ============================================================================

/// Собрать QVariantMap из пар. QML читает такую карту точкой (`item.label`).
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

/// Rust-состояние шелла вкладки.
pub struct WallpaperEngineModelRust {
    /// Номер записи в общем реестре смены скина (`crate::skin`).
    /// 0 — страница ещё не подписана.
    skin_watch: u64,
    /// План рейла от ядра: пункты, индекс заглушки, индекс dev-страницы.
    plan: ww::RailPlan,
    /// Собраны ли плагины вендорного интерфейса.
    qml_available: bool,
    /// Включён ли скрытый режим разработчика.
    dev_mode: bool,
    /// Найдено ли расхождение паритета (пометка на кнопке «Разраб.»).
    parity_drift: bool,
    /// Выбранный пункт рейла.
    current_index: i32,
    /// Каталог вендоренного интерфейса waywallen; называет хозяин страницы.
    vendor_qml_dir: String,
    /// Показанный экран: «подключение…», «нет демона» или «страницы».
    state: i32,
    /// Причина отказа для экрана «нет демона»; пусто — причины нет и
    /// показывается короткая строка ядра без двоеточия.
    no_daemon_reason: String,
    /// Полоса «чей фон» — готовым решением ядра.
    shell_banner: shell::ShellBanner,
    /// Итог последнего нажатия и признак отказа.
    shell_action_message: String,
    shell_action_failed: bool,
}

/// План строится СРАЗУ, а не при первом обращении: `railItems` QML читает
/// раньше любого нашего кода, и пустой рейл на первом кадре человек прочёл
/// бы как поломку вкладки.
impl Default for WallpaperEngineModelRust {
    fn default() -> Self {
        let qml_available = we_env::qml_plugins_ready();
        let dev_mode = dev_mode_now();
        Self {
            skin_watch: 0,
            plan: ww::build_rail(qml_available, dev_mode),
            qml_available,
            dev_mode,
            parity_drift: false,
            current_index: 0,
            vendor_qml_dir: String::new(),
            // Вкладка открывается на «подключение…» — как виджетная, у
            // которой стек состояний начинается с того же экрана. Демона
            // ещё не спрашивали, и рисовать страницы поверх мёртвого
            // сокета значило бы показать покупателю пустоту без объяснения.
            state: ww::STATE_CONNECTING as i32,
            no_daemon_reason: String::new(),
            // Полоса считается при первом `refreshShellState` из разметки,
            // а не здесь: сбор читает каталог сеанса и `/proc`, и конструктор
            // объекта — не место для этого.
            shell_banner: shell::ShellBanner::default(),
            shell_action_message: String::new(),
            shell_action_failed: false,
        }
    }
}

/// Снять подписку на смену скина: реестр иначе держал бы запись закрытой
/// вкладки до первой рассылки.
impl Drop for WallpaperEngineModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

impl qobject::WallpaperEngineModel {
    // ── Размеры и подписи рейла ───────────────────────────────────────────
    fn rail_width(&self) -> i32 {
        ww::RAIL_WIDTH
    }

    fn rail_logo(&self) -> QString {
        QString::from(ww::RAIL_LOGO)
    }

    fn rail_button_min_height(&self) -> i32 {
        ww::RAIL_BUTTON_MIN_HEIGHT
    }

    fn rail_button_radius(&self) -> i32 {
        ww::RAIL_BUTTON_RADIUS
    }

    fn rail_button_font_size(&self) -> i32 {
        ww::RAIL_BUTTON_FONT_SIZE
    }

    /// Подложка полосы рейла краской действующего скина.
    fn rail_background_color(&self) -> QString {
        QString::from(ww::rail_background_argb_hex(self.skin_dark()).as_str())
    }

    fn logo_font_size(&self) -> i32 {
        ww::LOGO_FONT_SIZE
    }

    fn logo_font_weight(&self) -> i32 {
        ww::LOGO_FONT_WEIGHT
    }

    /// Подсветка выбранного пункта краской действующего скина.
    ///
    /// Цвет берётся из той же палитры, что уходит в QML (`accent`), а
    /// прозрачность накладывает ядро — восьмибитной, как её читает
    /// таблица стилей виджетной версии.
    fn rail_selected_color(&self) -> QString {
        let (r, g, b) = ui_base::active_skin_colors()
            .get("accent")
            .and_then(|c| ui_base::parse_hex(c))
            .unwrap_or((102, 192, 244));
        QString::from(ww::rail_selected_argb_hex(r, g, b).as_str())
    }

    fn state_text_font_size(&self) -> i32 {
        ww::STATE_TEXT_FONT_SIZE
    }

    fn no_daemon_icon_font_size(&self) -> i32 {
        ww::NO_DAEMON_ICON_FONT_SIZE
    }

    fn state_column_spacing(&self) -> i32 {
        ww::STATE_COLUMN_SPACING
    }

    fn state_column_margin(&self) -> i32 {
        ww::STATE_COLUMN_MARGIN
    }

    fn variant_alpha(&self) -> f64 {
        ww::VARIANT_ALPHA as f64
    }

    /// Тёмный ли действующий скин — тем же критерием, что у соседних
    /// QML-вкладок (`ui_base::is_dark_rgb` по цвету фона).
    fn skin_dark(&self) -> bool {
        ui_base::active_skin_colors()
            .get("bg_primary")
            .and_then(|c| ui_base::parse_hex(c))
            .map(|(r, g, b)| ui_base::is_dark_rgb(r, g, b))
            .unwrap_or(true)
    }

    /// Красить ли вендорный интерфейс схемой самого waywallen.
    fn vendor_theme_follow(&self) -> bool {
        ww::active_vendor_theme().follow_vendor
    }

    /// Затравка схемы Material — акцент действующего скина.
    fn vendor_accent(&self) -> QString {
        QString::from(&ww::active_vendor_theme().accent)
    }

    /// Фон шелла вендорного интерфейса — фон страницы скина.
    fn vendor_background(&self) -> QString {
        QString::from(&ww::active_vendor_theme().background)
    }

    /// Основной текст шелла — текст страницы скина.
    fn vendor_text(&self) -> QString {
        QString::from(&ww::active_vendor_theme().text)
    }

    /// Роли схемы Material красками скина, объектом JSON.
    fn vendor_roles(&self) -> QString {
        QString::from(&ww::vendor_roles_json(&ww::active_vendor_theme()))
    }

    fn no_qml_modules_text(&self) -> QString {
        // Сборка есть, но под другую версию Qt — объяснить это, а не
        // посылать собирать то, что уже собрано (09.10.2026, падение
        // «Настроек» после обновления Qt 6.11 → 6.12).
        if !self.qml_available {
            if let Some((built, running)) = we_env::qml_modules_wrong_qt() {
                return QString::from(&ww::text_qml_modules_wrong_qt(built, running));
            }
        }
        QString::from(&ww::text_no_qml_modules())
    }

    // ── Среда вкладки ─────────────────────────────────────────────────────
    fn qml_available(&self) -> bool {
        self.rust().qml_available
    }

    fn dev_mode(&self) -> bool {
        self.rust().dev_mode
    }

    // ── Рейл ──────────────────────────────────────────────────────────────
    fn rail_items(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for entry in rail_entries(&self.rust().plan, self.rust().parity_drift) {
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(&entry.id)),
                ("icon", qstr(&entry.icon)),
                ("label", qstr(&entry.label)),
                ("qml", qstr(&entry.qml)),
                ("buttonLabel", qstr(&entry.button_label)),
            ])));
        }
        out
    }

    /// Индекс заглушки «модули не собраны»; -1 — заглушки в рейле нет.
    fn stub_index(&self) -> i32 {
        idx_or_none(self.rust().plan.stub_index)
    }

    /// Индекс страницы «Разработчик»; -1 — dev-режим выключен.
    fn dev_index(&self) -> i32 {
        idx_or_none(self.rust().plan.dev_index)
    }

    fn parity_drift(&self) -> bool {
        self.rust().parity_drift
    }

    /// Пометка расхождения паритета: меняет подпись кнопки «Разраб.».
    fn set_parity_drift(mut self: Pin<&mut Self>, value: bool) {
        if self.rust().parity_drift == value {
            return;
        }
        self.as_mut().rust_mut().parity_drift = value;
        self.as_mut().rail_changed();
    }

    fn current_index(&self) -> i32 {
        self.rust().current_index
    }

    /// Выбрать пункт рейла. Номер вне рейла игнорируется: рейл короткий и
    /// его состав зависит от dev-режима, а прыжок на несуществующую
    /// страницу оставил бы вкладку пустой.
    fn set_current_index(mut self: Pin<&mut Self>, value: i32) {
        let count = self.rust().plan.items.len() as i32;
        if value < 0 || value >= count || value == self.rust().current_index {
            return;
        }
        self.as_mut().rust_mut().current_index = value;
        self.as_mut().current_index_changed();
    }

    /// Палитра ДЕЙСТВУЮЩЕГО скина (сохранённый `ui_skin`, откат к
    /// стандартному по правилу #17). Своих цветов шелл не заводит.
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        // Затемнение фона под модальным окном — одно на продукт, той же
        // строкой, что у соседних QML-вкладок.
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        out
    }

    // ── API для QML ───────────────────────────────────────────────────────
    fn qml_for_index(&self, idx: i32) -> QString {
        if idx < 0 {
            return QString::default();
        }
        QString::from(self.rust().plan.qml_for_index(idx as usize).unwrap_or(""))
    }

    fn vendor_qml_dir(&self) -> QString {
        QString::from(&self.rust().vendor_qml_dir)
    }

    /// Сказать странице, где лежит вендоренный интерфейс waywallen.
    fn set_vendor_qml_dir(mut self: Pin<&mut Self>, value: &QString) {
        let value = value.to_string();
        if self.rust().vendor_qml_dir == value {
            return;
        }
        self.as_mut().rust_mut().vendor_qml_dir = value;
        self.as_mut().vendor_qml_dir_changed();
    }

    /// Адрес шелла вокруг вендорной страницы (`PageHost.qml`).
    ///
    /// ⚠️ Вендорная страница грузится НЕ напрямую, а внутрь этого шелла —
    /// так же, как её грузил питоновский хост. Он играет роль их
    /// `Window.qml`: держит режим Material по скину Rexon, размерный класс
    /// (`MProp.size`) и тосты. Без него страница «Обои» открывается пустой:
    /// её вёрстка спрашивает размерный класс, а без хозяина Material он
    /// нулевой. Проверено живьём 30.08 — пять страниц из шести рисовались
    /// и без шелла, а «Обои» нет.
    ///
    /// Имя файла держит ядро, каталог называет хозяин страницы.
    fn page_host_url(&self) -> QUrl {
        let dir = &self.rust().vendor_qml_dir;
        if dir.is_empty() {
            return QUrl::default();
        }
        local_file_url(&rexon_core::qml_modules::qml_path(
            dir,
            rexon_core::qml_modules::PAGE_HOST_QML,
        ))
    }

    /// Адрес вендорной страницы для `Loader`.
    ///
    /// Путь складывает ядро (`qml_modules::vendor_page_path`) — той же
    /// склейкой, которой пользовался питоновский хост. Своей второй копии
    /// раскладки вендорного каталога здесь нет.
    fn vendor_page_url(&self, idx: i32) -> QUrl {
        let dir = &self.rust().vendor_qml_dir;
        if dir.is_empty() || idx < 0 {
            return QUrl::default();
        }
        // ⚠️ Спрашиваем ВЕНДОРНОЕ имя, а не любое: у пункта «Разраб.»
        // имя QML-файла есть, но это НАША страница из модуля
        // `org.rexon.gui`, и в каталоге waywallen её нет. С общим
        // `qml_for_index` сюда уезжало бы имя нашей страницы, а `Loader`
        // получал бы адрес несуществующего файла.
        let Some(page) = self.rust().plan.vendor_qml_for_index(idx as usize) else {
            return QUrl::default();
        };
        let path = rexon_core::qml_modules::vendor_page_path(dir, page);
        if path.is_empty() {
            return QUrl::default();
        }
        local_file_url(&path)
    }

    /// Нужна ли странице пункта `idx` шапка от хозяина.
    ///
    /// ⚠️ Свойство ставится на шелл `PageHost.qml`, а не на саму страницу:
    /// шапку `MD.Page` рисует по `MD.MProp.page.showHeader` — по контексту
    /// ХОЗЯИНА. Страница, у waywallen живущая во всплывающем окне
    /// (`PluginManagePage.qml`), своего `showHeader` не ставит вовсе и в
    /// нашем рейле открывалась без заголовка и без всех кнопок действий.
    fn vendor_page_header(&self, idx: i32) -> bool {
        idx >= 0 && self.rust().plan.header_for_index(idx as usize)
    }

    fn rail_item_id(&self, idx: i32) -> QString {
        if idx < 0 {
            return QString::default();
        }
        let items = &self.rust().plan.items;
        QString::from(items.get(idx as usize).map(|i| i.id.as_str()).unwrap_or(""))
    }

    // ── Стартовые состояния вкладки ───────────────────────────────────────
    //
    // Своего смысла у номеров экранов здесь нет: и номера, и подписи держит
    // ядро, а разметка сравнивает свойство `state` с именованными
    // константами, а не с числами.

    fn state(&self) -> i32 {
        self.rust().state
    }

    fn state_connecting(&self) -> i32 {
        ww::STATE_CONNECTING as i32
    }

    fn state_no_daemon(&self) -> i32 {
        ww::STATE_NO_DAEMON as i32
    }

    fn state_pages(&self) -> i32 {
        ww::STATE_PAGES as i32
    }

    fn connecting_text(&self) -> QString {
        QString::from(&ww::text_connecting())
    }

    fn no_daemon_icon(&self) -> QString {
        QString::from(ww::ICON_NO_DAEMON)
    }

    fn retry_daemon_label(&self) -> QString {
        QString::from(&ww::text_retry_daemon())
    }

    fn health_timeout_ms(&self) -> i32 {
        ww::HEALTH_TIMEOUT_MS
    }

    fn connect_timeout_ms(&self) -> i32 {
        ww::CONNECT_TIMEOUT_MS
    }

    /// Текст экрана отказа.
    ///
    /// ⚠️ Хранится СЫРАЯ причина, а показывается складённая ядром подпись
    /// для человека: сама причина — машинная (пути, `PATH`, имя запроса
    /// `health`) и её место в журнале, а не на экране (правило #13).
    /// Пустая причина отдельным случаем: экран показывается и до первого
    /// ответа движка.
    fn no_daemon_text(&self) -> QString {
        let reason = &self.rust().no_daemon_reason;
        if reason.is_empty() {
            return QString::from(&ww::text_no_daemon());
        }
        QString::from(&ww::no_daemon_text_in(reason, crate::we_env::rexon_packaged()))
    }

    /// Перейти на экран. Один вход на все переходы: сигнал шлётся ровно
    /// один раз и только когда что-то действительно изменилось.
    fn switch_state(mut self: Pin<&mut Self>, state: i32, reason: String) {
        if self.rust().state == state && self.rust().no_daemon_reason == reason {
            return;
        }
        self.as_mut().rust_mut().state = state;
        self.as_mut().rust_mut().no_daemon_reason = reason;
        self.as_mut().state_changed();
    }

    fn show_connecting(self: Pin<&mut Self>) {
        self.switch_state(ww::STATE_CONNECTING as i32, String::new());
    }

    fn show_no_daemon(self: Pin<&mut Self>, reason: &QString) {
        self.fail(reason.to_string());
    }

    fn show_health_timeout(self: Pin<&mut Self>) {
        self.fail(ww::TEXT_HEALTH_TIMEOUT.to_string());
    }

    fn show_connect_timeout(self: Pin<&mut Self>) {
        self.fail(ww::REASON_CONNECT_TIMEOUT.to_string());
    }

    /// Один вход на все отказы: причина уходит в журнал, экран — покупателю.
    ///
    /// Записывать её приходится здесь, а не в транспорте: до экрана доходят
    /// и те отказы, которых транспорт не видел вовсе (истёкший срок
    /// ожидания, неготовый ответ `health`), а журнал обязан знать ВСЕ —
    /// иначе с экрана техническая подробность пропала, а в журнале не
    /// появилась.
    fn fail(self: Pin<&mut Self>, reason: String) {
        if let Some(line) = ww::daemon_failure_log_line(&reason) {
            // Общий журнал Rexon (`rexon_core::applog`).
            rexon_core::log_warn!("wallpaper-tab", "{line}");
        }
        self.switch_state(ww::STATE_NO_DAEMON as i32, reason);
    }

    /// Ответ `health` подключившегося демона.
    ///
    /// ⚠️ Открытый сокет ещё НЕ значит, что движок рабочий: он мог
    /// подняться и упасть на миграции своей базы. Без этой проверки
    /// покупатель получил бы страницы, которые молча ничего не показывают.
    /// Вердикт по ответу выносит ядро; пустой ответ — старый демон, ему
    /// верим.
    fn apply_health(self: Pin<&mut Self>, state: &QString) {
        match ww::health_verdict(&state.to_string()) {
            ww::HealthVerdict::Ready => {
                self.switch_state(ww::STATE_PAGES as i32, String::new())
            }
            ww::HealthVerdict::NotReady(reason) => self.fail(reason),
        }
    }
}

/// Адрес локального файла (`QUrl::fromLocalFile`) — своей сборкой из схемы
/// и пути.
///
/// ⚠️ Готового `from_local_file` в cxx-qt-lib 0.9 нет вовсе, а склеивать
/// строку «file://» + путь нельзя: путь пользователя может быть каким
/// угодно (у владельца он кириллический), и в строковом адресе такие байты
/// пришлось бы кодировать вручную. `set_path` кладёт путь как есть, а
/// кодированием занимается сам QUrl — ровно как в `QUrl::fromLocalFile`.
pub(crate) fn local_file_url(path: &str) -> QUrl {
    let mut url = QUrl::default();
    url.set_scheme(&QString::from("file"));
    url.set_path(&QString::from(path));
    url
}

/// `Option<usize>` в номер для QML: «нет такого пункта» — это -1, а не 0.
fn idx_or_none(value: Option<usize>) -> i32 {
    value.map(|v| v as i32).unwrap_or(-1)
}

// ============================================================================
// Инициализация состояния
// ============================================================================

// ── «Чей фон» ──────────────────────────────────────────────────────────────
impl qobject::WallpaperEngineModel {
    fn shell_banner_visible(&self) -> bool {
        self.rust().shell_banner.visible
    }

    fn shell_banner_text(&self) -> QString {
        QString::from(&self.rust().shell_banner.text)
    }

    fn shell_can_give(&self) -> bool {
        self.rust().shell_banner.can_give
    }

    fn shell_can_return(&self) -> bool {
        self.rust().shell_banner.can_return
    }

    fn shell_give_label(&self) -> QString {
        QString::from(&shell::give_label())
    }

    fn shell_return_label(&self) -> QString {
        QString::from(&shell::return_label())
    }

    fn shell_action_message(&self) -> QString {
        QString::from(&self.rust().shell_action_message)
    }

    fn shell_action_failed(&self) -> bool {
        self.rust().shell_action_failed
    }

    fn refresh_shell_state(mut self: Pin<&mut Self>) {
        let banner = shell::live_shell_wallpaper_state().banner;
        if banner != self.rust().shell_banner {
            self.as_mut().rust_mut().shell_banner = banner;
            self.as_mut().shell_state_changed();
        }
    }

    fn give_wallpaper_to_rexon(self: Pin<&mut Self>) {
        // Какой шелл — из того же ответа, по которому показана кнопка, а не
        // из разметки: кнопка без живого шелла нажаться не может.
        let target = shell_from_id(&self.rust().shell_banner.shell);
        let result = match target {
            Some(s) => shell::hand_over(s, &shell::ShellHome::live()),
            None => return,
        };
        self.finish_shell_action(result);
    }

    fn return_wallpaper_to_shell(self: Pin<&mut Self>) {
        let target = shell_from_id(&self.rust().shell_banner.shell);
        let result = match target {
            Some(s) => shell::give_back(s, &shell::ShellHome::live()),
            None => return,
        };
        self.finish_shell_action(result);
    }

    fn finish_shell_action(mut self: Pin<&mut Self>, result: Result<String, String>) {
        let (message, failed) = match result {
            Ok(m) => (m, false),
            Err(m) => (m, true),
        };
        {
            let mut r = self.as_mut().rust_mut();
            r.shell_action_message = message;
            r.shell_action_failed = failed;
            r.shell_banner = shell::live_shell_wallpaper_state().banner;
        }
        self.as_mut().shell_state_changed();
    }
}

/// Шелл по идентификатору полосы (`noctalia` | `dms`).
fn shell_from_id(id: &str) -> Option<rexon_core::system::DesktopShell> {
    rexon_core::system::DesktopShell::all().iter().copied().find(|s| s.as_str() == id)
}

impl cxx_qt::Initialize for qobject::WallpaperEngineModel {
    fn initialize(mut self: Pin<&mut Self>) {
        // Подписка на смену скина: одна строка на страницу, рассылка —
        // общая (`crate::skin`). Снимается в `Drop` того же состояния.
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::WallpaperEngineModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Полный рейл: семь вендорных пунктов, пометки паритета нет.
    #[test]
    fn rail_entries_full() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let plan = ww::build_rail(true, false);
        let entries = rail_entries(&plan, false);
        assert_eq!(entries.len(), 7);
        assert_eq!(entries[0].id, "wallpapers_qml");
        assert_eq!(entries[0].qml, "WallpaperPage.qml");
        assert_eq!(entries[0].button_label, "🖼️\nWallpapers");
    }

    /// Плагины не собраны: пункт один, он же заглушка, вендорной страницы у
    /// него нет.
    #[test]
    fn rail_entries_without_plugins() {
        let plan = ww::build_rail(false, false);
        let entries = rail_entries(&plan, false);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "no_qml");
        assert!(entries[0].qml.is_empty());
        assert_eq!(plan.stub_index, Some(0));
    }

    /// Пометка расхождения паритета висит ТОЛЬКО на пункте «Разработчик».
    #[test]
    fn drift_mark_only_on_dev_item() {
        let plan = ww::build_rail(true, true);
        let entries = rail_entries(&plan, true);
        assert_eq!(entries.len(), 8);
        assert_eq!(plan.dev_index, Some(7));
        assert_eq!(entries[7].id, "dev");
        // У «Разработчика» страница СВОЯ: имя файла есть, вендорного —
        // нет. Иначе шелл искал бы её в каталоге waywallen.
        assert_eq!(entries[7].qml, ww::DEV_PAGE_QML);
        assert_eq!(plan.vendor_qml_for_index(7), None);
        assert!(entries[7].button_label.ends_with(ww::RAIL_DRIFT_MARK));
        for entry in &entries[..7] {
            assert!(!entry.button_label.ends_with(ww::RAIL_DRIFT_MARK));
        }
    }

    /// Без расхождения пометки нет и у «Разработчика».
    #[test]
    fn no_drift_no_mark() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let plan = ww::build_rail(true, true);
        let entries = rail_entries(&plan, false);
        assert_eq!(entries[7].button_label, "🛠️\nDev");
    }

    /// Сторож ожидания обязан стоять в разметке и быть привязан к ЭКРАНУ.
    ///
    /// Стоит он не для красоты: пропади этот `Timer` — и вкладка снова
    /// зависнет на «Подключение…» навсегда, а выяснится это только у
    /// покупателя, потому что ни сборка, ни `cargo test` разметку не
    /// исполняют. Привязка к состоянию (а не к попытке) — вторая половина
    /// договора: только она даёт повтор сколько угодно раз и гасит отсчёт
    /// при удачном подключении.
    #[test]
    fn page_guards_the_waiting_screen() {
        const PAGE_QML: &str = include_str!("../qml/WallpaperEnginePage.qml");
        for part in [
            "interval: page.connectTimeoutMs",
            "running: page.state === page.stateConnecting",
            "onTriggered: page.showConnectTimeout()",
        ] {
            assert!(
                PAGE_QML.contains(part),
                "у страницы «Wallpaper Engine» пропал сторож ожидания \
                 («{part}»): экран «Подключение…» снова станет бесконечным"
            );
        }
    }

    /// Полоса «чей фон» стоит в разметке, её кнопки зовут действия модели,
    /// а пересчёт идёт и при открытии, и по таймеру — иначе запущенный
    /// после открытия окна шелл остался бы незамеченным.
    #[test]
    fn page_shows_whose_background_it_is() {
        const PAGE_QML: &str = include_str!("../qml/WallpaperEnginePage.qml");
        for part in [
            "objectName: \"shellBanner\"",
            "visible: page.shellBannerVisible",
            "onClicked: page.giveWallpaperToRexon()",
            "onClicked: page.returnWallpaperToShell()",
            "visible: page.shellCanGive",
            "visible: page.shellCanReturn",
            "triggeredOnStart: true",
            "onTriggered: page.refreshShellState()",
            // Баннер плагина обоев Plasma — на своём объекте действий.
            "objectName: \"kdePluginBanner\"",
            "visible: ownActions.kdePluginNotice !== \"\"",
            "onClicked: ownActions.installKdePlugin()",
        ] {
            assert!(PAGE_QML.contains(part), "у страницы пропало «{part}»");
        }
    }

    /// «Нет такого пункта» уезжает в QML как -1: ноль там означал бы
    /// первый пункт рейла, то есть существующую страницу.
    #[test]
    fn missing_index_is_minus_one() {
        assert_eq!(idx_or_none(None), -1);
        assert_eq!(idx_or_none(Some(0)), 0);
        assert_eq!(idx_or_none(Some(6)), 6);
    }
}
