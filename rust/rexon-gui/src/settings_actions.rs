// SettingsActions — объект побочных действий вкладки «Настройки» на Rust.
//
// ЗАЧЕМ. `SettingsPage.qml` ждёт свойством `actions` объект, который умеет
// две вещи: отдать СНИМОК машины (`facts`, `state`, `ambientState`) и
// ИЗМЕНИТЬ её (сменить скин, включить автозапуск, применить файловый
// менеджер, вернуть родные приложения окружения, записать звуковую тему,
// защитить приложения по умолчанию). Пока такого объекта на Rust не было,
// раздел «Настройки» под Rust-хозяином окна открывался ПУСТЫМ: разделов у
// страницы нет до первого `facts()`, а `MainWindow.qml` кладёт в неё
// `SettingsPage {}` без объекта действий. Здесь объект появляется, и раздел
// начинает работать.
//
// Договор со страницей взят у Python-класса `SettingsActions`
// (`rexon/qt_settings/window.py`, 1100 строк вместе с хозяином окна) БУКВА В
// БУКВУ — те же сорок имён, те же типы ответов, те же два сигнала
// (`status`, `refreshed`). Разметка страницы поэтому не изменилась ни на
// строку, кроме одной: страница заводит объект СЕБЕ САМА
// (`SettingsActions { id: ownActions }`), как это уже делают «Расширения» и
// Wallpaper Engine. Хозяину окна для этого делать нечего.
//
// ГДЕ ЖИВЁТ ЛОГИКА. Своей здесь нет вовсе:
//   * ЧТО показать и какими словами — ядро (`rexon_core::qt_settings`), тот
//     же каталог `TEXTS`, которым набрана виджетная вкладка;
//   * ЧТО считать состоянием машины — ядро же: `ui_base` (скин), `tray`
//     (автозапуск), `settings::SettingsManager` (настройки), `appdefaults`
//     (родные приложения окружения и файловый менеджер), `defaults`
//     (защита приложений по умолчанию), `backup` (копии панели Plasma),
//     `system`/`screenshots` (опознание окружения), `onboarding` (ключи
//     обучения), `aeroshell` (сверка оформления);
//   * ЧЕМ это делать — общий запуск команд ядра (`utils::run`) и обычные
//     чтение-запись файлов. Второго набора правил здесь не заводится.
//
// ⚠️ ЧТО НЕ ПЕРЕНЕСЕНО И ПОЧЕМУ — ЧЕТЫРЕ ГРАНИЦЫ, НАЗВАННЫЕ ВСЛУХ.
// Молчаливого «ничего не произошло» здесь быть не должно (правило #13):
// покупатель принял бы такую кнопку за поломку. Поэтому каждый
// неперенесённый метод ОБЯЗАН ответить строкой «что нажали и почему не
// вышло». Отсылать в классическое окно этой строке больше нечем: пункта
// ярлыка с 16.09 нет, самого окна поставка не везёт, и общая формулировка
// снята вместе с ним (см. «Границы, названные вслух» ниже). Технические
// причины дальше — для разработчика, покупателю они не показываются:
//
//   1. ДВИЖОК ОБОЕВ — ПЕРЕНЕСЁН. Исполнитель планов в ядре
//      (`wallpaper_exec::WallpaperExec`), и через него здесь работают:
//      «Остановить живые обои», «Картинка…», «Видео…», YouTube, веб-обои,
//      библиотека, размытие, фоновые звуки, кнопки MATE и установка yt-dlp.
//      Исполнитель один на процесс и общий со страницей «Steam»
//      (`wallpaper_exec::live_slot`); применения идут общей дверью ядра
//      (`wallpaper_exec::run_in_slot`), и живые видео и YouTube — под
//      надзором за нагрузкой, который поднимает она. Какие форматы исполнитель
//      носит, решает `wallpaper_exec::carries_format`: картинки, видео,
//      GIF, веб и YouTube применяются, сцены и обои-приложения из
//      библиотеки получают тот же окончательный отказ, что в классическом
//      окне. В классическое окно не отсылается ничего: Flash (SWF) —
//      режим хоста обоев `--mode swf`.
//   2. ПРОВЕРКА КОМПЛЕКТНОСТИ — ПЕРЕНЕСЕНА. «Проверить и доустановить»
//      исполняет ядро тем же исполнителем, что ставит рабочие столы
//      (`system_de_exec::completeness_survey_live` / `complete_de_live`):
//      проверка без пароля → вопрос со списком недостающего в общем окне
//      операции (`DeOperationDialog.qml`) → после «Да» доустановка с ходом
//      дела, отменой пока система не тронута и одним объяснением на отказ
//      пароля.
//   2а. ОФОРМЛЕНИЕ AEROSHELL — ПЕРЕНЕСЕНО. Сверка и пересборка — тем же
//      исполнителем ядра (`system_de_exec::aeroshell_survey_live` /
//      `rebuild_aeroshell_live`) и тем же окном операции. Сверку при запуске
//      Rexon зовёт главное окно (`aeroshellStartupCheck`, свой объект
//      действий и своё окно), кнопку «Проверить сейчас» — эта страница.
//   3. (снята) КОПИИ ПАНЕЛИ PLASMA — см. «что открыто» ниже.
//   4. (снята) ОБУЧЕНИЕ. Накладка обучения есть у главного окна на Rust
//      (`onboarding_overlay`), и «Пройти обучение заново» передаёт ей
//      просьбу. Без главного окна кнопка говорит, куда идти (`ob.no_main_window`).
//
// ⭐ ЧТО ОТКРЫТО ПОСЛЕ ПЕРЕНОСА.
//   * ОКНО ВЫБОРА ШРИФТА. Границей здесь стояло «`QFontDialog` — виджет,
//     которого в `QGuiApplication` нет вовсе». Теперь его роль играет
//     `FontDialog` из `QtQuick.Dialogs`, поднятый тем же шимом с вложенным
//     циклом событий, которым «Расширения» выбирают .rxn-файл
//     (`cpp/rexon_ext_dialogs.h`) — второго шима для этого не заводилось.
//   * КОПИИ ПАНЕЛИ PLASMA. Исполнитель `PlasmaProtector` дописан — но не
//     здесь, а рядом с решениями, в `backup.rs` ядра (`plasma_backup_run`,
//     `plasma_restore_run`, `plasma_restart_shell`): те же кнопки есть и во
//     вкладке «Рабочие столы», и второй копии исполнителя в продукте быть не
//     должно (правило #12). Вкладке остаётся то, что видно только ей: где
//     лежит каталог копий, какое сейчас окружение и чем запускать команды.
//
// ⭐ ЧТО ПОЧИНЕНО ПРИ ПЕРЕНОСЕ (правило #16 — дефекты не консервируем):
//   1. Скин в списке больше не расходится с нарисованным. Python отдавал
//      странице СЫРОЕ значение `ui_skin` из настроек, а палитру страница
//      берёт у `ui_base::active_skin_colors`, который на незнакомое имя
//      откатывается к стандартному. Написали в настройки чужой скин — и
//      вкладка была выкрашена в один, а в списке стоял другой. Теперь имя
//      проходит через тот же `active_skin_id_from`, что и палитра.
//   2. Импорт тем Windows больше не «недоступен на этой сборке». Python
//      проверял `hasattr(_rexon_py, "importers_import_to_rxn")` — то есть
//      наличие моста в Python; в Rust-процессе импортёр слинкован в бинарь,
//      и проверять нечего.
//   3. Отказ автозапуска называет причину. Python возвращал общее «не
//      удалось — подробности в журнале», а журнал покупатель не читает;
//      теперь причина уходит в тот же текст ядра `status.error`.
//   4. Запись настроек звука обоев больше не рапортует об успехе вслепую:
//      Python не смотрел на итог сохранения вовсе.
//   5. Раздел «Фоновые звуки» честно отвечает про `mpv`. Python при
//      отсутствии движка отдавал `mpv: false`, не спрашивая систему, —
//      человек с установленным mpv видел «не установлен пакет mpv».

/// Мост cxx-qt объекта действий.
///
/// Тип объявлен `#[qml_element]`, поэтому страница заводит его себе сама
/// (`SettingsActions { id: ownActions }`). Регистрации в `main_window.rs`
/// для этого не требуется: там карта разделов, а не список побочных
/// объектов, — и хозяин окна остаётся нетронутым.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        /// Окно доустановки недостающего: вопрос, ход дела или итог. Тот же
        /// договор, что у `DesktopsActions.operation`, — окно одно на всех
        /// (`DeOperationDialog.qml`).
        #[qproperty(QMap_QString_QVariant, operation, READ = operation, NOTIFY = operation_changed)]
        /// Показывать ли кнопку «Установить Rexon в систему…» рядом с
        /// «Пройти обучение заново». Решение ядра БЕЗ учёта ответа «больше
        /// не спрашивать»: отказ снимает сама кнопка, а у установленного
        /// Rexon (пакет, Steam, установка для всех или для себя) кнопки нет
        /// вовсе. Пересчитывается вместе со снимком страницы.
        #[qproperty(bool, install_offered, cxx_name = "installOffered",
                    READ = install_offered, NOTIFY = refreshed)]
        /// Подпись кнопки и пояснение под ней: `{label, hint}`. Буквы — из
        /// ядра (`installer::settings_install_button` / `_hint`).
        #[qproperty(QMap_QString_QVariant, install_texts, cxx_name = "installTexts",
                    READ = install_texts, CONSTANT)]
        type SettingsActions = super::SettingsActionsRust;
    }

    unsafe extern "RustQt" {
        /// Ответ ДОЛГОЙ работы: показать всплывающим сообщением.
        #[qsignal]
        fn status(self: Pin<&mut SettingsActions>, message: QString);

        /// Долгая работа изменила состояние машины — перечитать снимок.
        #[qsignal]
        fn refreshed(self: Pin<&mut SettingsActions>);

        #[qsignal]
        #[cxx_name = "operationChanged"]
        fn operation_changed(self: Pin<&mut SettingsActions>);

        /// Строка журнала хода доустановки — дописывается окном.
        #[qsignal]
        #[cxx_name = "operationLog"]
        fn operation_log(self: Pin<&mut SettingsActions>, line: QString);
    }

    extern "RustQt" {
        fn operation(self: &SettingsActions) -> QMap_QString_QVariant;
        #[cxx_name = "installOffered"]
        fn install_offered(self: &SettingsActions) -> bool;
        #[cxx_name = "installTexts"]
        fn install_texts(self: &SettingsActions) -> QMap_QString_QVariant;
    }

    extern "RustQt" {
        /// «Да» в окне вопроса — начать доустановку.
        #[qinvokable]
        #[cxx_name = "confirmOperation"]
        fn confirm_operation(self: Pin<&mut SettingsActions>);

        /// «Нет», «Закрыть» — убрать окно (пока доустановка идёт, не действует).
        #[qinvokable]
        #[cxx_name = "dismissOperation"]
        fn dismiss_operation(self: Pin<&mut SettingsActions>);

        /// «Отменить» — только пока система не тронута.
        #[qinvokable]
        #[cxx_name = "cancelOperation"]
        fn cancel_operation(self: Pin<&mut SettingsActions>);
    }

    extern "RustQt" {
        // ── Снимки машины ────────────────────────────────────────────────

        /// Факты, от которых зависит САМ СОСТАВ разделов вкладки. Решение
        /// «показывать ли раздел» принимает ядро (`qt_settings::
        /// section_visible`), здесь только наблюдения.
        #[qinvokable]
        fn facts(self: &SettingsActions) -> QString;

        /// Полный снимок того, что вкладка показывает (JSON).
        #[qinvokable]
        fn state(self: &SettingsActions) -> QString;

        /// Только играющие фоновые звуки — снимок по таймеру. Отдельно от
        /// `state`, потому что полный снимок ходит в систему за шрифтами,
        /// звуковыми темами и приложениями по умолчанию.
        #[qinvokable]
        #[cxx_name = "ambientState"]
        fn ambient_state(self: &SettingsActions) -> QString;

        // ── Язык интерфейса ──────────────────────────────────────────────

        /// Запомнить язык интерфейса (`ui_language`): `system`, `en`, `ru`…
        /// Применяется после перезапуска — снимок (`lang.restart`) скажет,
        /// нужен ли он.
        #[qinvokable]
        #[cxx_name = "setLanguage"]
        fn set_language(self: &SettingsActions, code: &QString) -> QString;

        /// «Перезапустить Rexon»: запустить новый процесс, который дождётся
        /// выхода этого, и выйти. Пустой ответ — перезапуск пошёл.
        #[qinvokable]
        #[cxx_name = "restartRexon"]
        fn restart_rexon(self: &SettingsActions) -> QString;

        // ── Право изменять систему (Б13) ─────────────────────────────────

        /// Пароль уже введён, и Rexon может изменять систему до закрытия
        /// окна (`privilege::run::has_session`). Опрос — таймером страницы.
        #[qinvokable]
        #[cxx_name = "adminSession"]
        fn admin_session(self: &SettingsActions) -> bool;

        /// «Запретить сейчас»: закрыть сеанс прав. Ответ — строка для
        /// всплывающего сообщения.
        #[qinvokable]
        #[cxx_name = "revokeAdmin"]
        fn revoke_admin(self: &SettingsActions) -> QString;

        // ── Скин, обучение, автозапуск ───────────────────────────────────

        /// Сменить скин интерфейса и перекрасить открытые вкладки.
        #[qinvokable]
        #[cxx_name = "setSkin"]
        fn set_skin(self: &SettingsActions, skin_id: &QString) -> QString;

        /// Предлагать ли вводный показ при запуске.
        #[qinvokable]
        #[cxx_name = "setOnboarding"]
        fn set_onboarding(self: &SettingsActions, enabled: bool) -> QString;

        /// Пройти вводный показ заново. Изменяемый доступ — ради толчка
        /// «перечитать снимок» после того, как накладка включит обучение.
        #[qinvokable]
        #[cxx_name = "replayOnboarding"]
        fn replay_onboarding(self: Pin<&mut SettingsActions>) -> QString;

        /// «Установить Rexon в систему…»: снять прежний ответ «больше не
        /// спрашивать» и открыть мастер первого запуска. Отвечает строкой
        /// для подвала — молчаливого «ничего не произошло» у кнопки быть не
        /// должно (правило #13).
        #[qinvokable]
        #[cxx_name = "installRexon"]
        fn install_rexon(self: &SettingsActions) -> QString;

        /// Автозапуск Rexon при входе в систему (в лоток, без окна).
        #[qinvokable]
        #[cxx_name = "setAutostart"]
        fn set_autostart(self: &SettingsActions, enabled: bool) -> QString;

        /// Докачивать ли значки и курсоры к теме без вопроса
        /// (`rexon_core::theme_watch::AUTO_HOME_KEY`).
        #[qinvokable]
        #[cxx_name = "setThemeDepsAuto"]
        fn set_theme_deps_auto(self: &SettingsActions, enabled: bool) -> QString;

        // ── Файловый менеджер ────────────────────────────────────────────

        /// Запомнить выбор файлового менеджера (ещё не применяя).
        #[qinvokable]
        #[cxx_name = "setFileManager"]
        fn set_file_manager(self: &SettingsActions, fm_id: &QString) -> QString;

        /// Сделать выбранный файловый менеджер менеджером по умолчанию.
        #[qinvokable]
        #[cxx_name = "applyFileManager"]
        fn apply_file_manager(self: &SettingsActions) -> QString;

        /// Вернуть родные приложения окружения.
        #[qinvokable]
        #[cxx_name = "restoreNative"]
        fn restore_native(self: &SettingsActions) -> QString;

        /// Проверить комплектность рабочего стола (с доустановкой). Проверка
        /// идёт в фоне; нехватка — окно вопроса, после «Да» — окно хода.
        #[qinvokable]
        #[cxx_name = "checkCompleteness"]
        fn check_completeness(self: Pin<&mut SettingsActions>) -> QString;

        // ── Обои ─────────────────────────────────────────────────────────

        /// Остановить живые обои.
        #[qinvokable]
        #[cxx_name = "stopWallpaper"]
        fn stop_wallpaper(self: &SettingsActions) -> QString;

        /// Записать настройки звука обоев (обои НЕ перезапускаются: хост
        /// следит за файлом настроек и применяет правку сам).
        #[qinvokable]
        #[cxx_name = "setWallpaperAudio"]
        fn set_wallpaper_audio(
            self: &SettingsActions,
            enabled: bool,
            volume: i32,
            mute_other: bool,
            hide_covered: bool,
        ) -> QString;

        /// Выбрать файл обоев и применить. `kind` — `static` либо `video`.
        /// Изменяемый доступ — ради итога применения, который приходит из
        /// фонового потока сигналом.
        #[qinvokable]
        #[cxx_name = "pickWallpaper"]
        fn pick_wallpaper(self: Pin<&mut SettingsActions>, kind: &QString) -> QString;

        /// Применить YouTube-обои.
        #[qinvokable]
        #[cxx_name = "applyYoutube"]
        fn apply_youtube(
            self: Pin<&mut SettingsActions>,
            url: &QString,
            quality: &QString,
            mode: &QString,
            audio: bool,
        ) -> QString;

        /// Звук у YouTube-обоев — свойство запуска, живёт до закрытия окна.
        #[qinvokable]
        #[cxx_name = "setYoutubeAudio"]
        fn set_youtube_audio(self: &SettingsActions, on: bool) -> QString;

        /// Поставить либо обновить yt-dlp. Изменяемый доступ — ради итога,
        /// который приходит из фонового потока сигналом.
        #[qinvokable]
        fn ytdlp(self: Pin<&mut SettingsActions>, action: &QString) -> QString;

        /// Применить веб-обои.
        #[qinvokable]
        #[cxx_name = "applyWeb"]
        fn apply_web(self: Pin<&mut SettingsActions>, url: &QString) -> QString;

        // ── Библиотека обоев ─────────────────────────────────────────────

        /// Открыть файл обоев тем, чем его открывает система.
        #[qinvokable]
        #[cxx_name = "previewLibrary"]
        fn preview_library(self: &SettingsActions, path: &QString) -> QString;

        /// Применить обои из библиотеки.
        #[qinvokable]
        #[cxx_name = "applyLibrary"]
        fn apply_library(
            self: Pin<&mut SettingsActions>,
            path: &QString,
            fmt: &QString,
        ) -> QString;

        // ── Импорт тем Windows ───────────────────────────────────────────

        /// Выбрать файл темы Windows и импортировать его в фоне.
        #[qinvokable]
        #[cxx_name = "pickImport"]
        fn pick_import(self: Pin<&mut SettingsActions>) -> QString;

        // ── Размытие обоев ───────────────────────────────────────────────

        /// Запомнить силу размытия (применится следующим «Размыть»).
        #[qinvokable]
        #[cxx_name = "setBlurRadius"]
        fn set_blur_radius(self: &SettingsActions, value: i32) -> QString;

        /// Размыть обои либо снять размытие. Изменяемый доступ — ради итога,
        /// который приходит из фонового потока сигналом.
        #[qinvokable]
        fn blur(self: Pin<&mut SettingsActions>, action: &QString) -> QString;

        // ── Фоновые звуки ────────────────────────────────────────────────

        /// Выбрать аудиофайл и проигрывать его по кругу.
        #[qinvokable]
        #[cxx_name = "addAmbient"]
        fn add_ambient(self: &SettingsActions, volume: i32) -> QString;

        /// Задать громкость играющему звуку.
        #[qinvokable]
        #[cxx_name = "setAmbientVolume"]
        fn set_ambient_volume(self: &SettingsActions, sound_id: &QString, volume: i32)
            -> QString;

        /// Остановить один фоновый звук.
        #[qinvokable]
        #[cxx_name = "stopAmbient"]
        fn stop_ambient(self: &SettingsActions, sound_id: &QString) -> QString;

        /// Остановить все фоновые звуки.
        #[qinvokable]
        #[cxx_name = "stopAllAmbient"]
        fn stop_all_ambient(self: &SettingsActions) -> QString;

        // ── Звуки событий ────────────────────────────────────────────────

        /// Записать звуковую тему окружения.
        #[qinvokable]
        #[cxx_name = "setSoundTheme"]
        fn set_sound_theme(self: &SettingsActions, theme: &QString) -> QString;

        /// Включить либо выключить звуки событий.
        #[qinvokable]
        #[cxx_name = "setEventSounds"]
        fn set_event_sounds(self: &SettingsActions, enabled: bool) -> QString;

        /// Прослушать звук события темы.
        #[qinvokable]
        #[cxx_name = "playSound"]
        fn play_sound(self: &SettingsActions, theme: &QString, event_id: &QString) -> QString;

        // ── MATE ─────────────────────────────────────────────────────────

        /// Иконки рабочего стола MATE: `on` / `off` / `restart`.
        #[qinvokable]
        #[cxx_name = "mateAction"]
        fn mate_action(self: &SettingsActions, action: &QString) -> QString;

        // ── Системные шрифты ─────────────────────────────────────────────

        /// Выбрать системный шрифт: `system` / `mono` / `title`.
        #[qinvokable]
        #[cxx_name = "pickFont"]
        fn pick_font(self: &SettingsActions, target: &QString) -> QString;

        /// Записать отрисовку шрифтов: сглаживание (и диапазон кеглей без
        /// него), субпиксели, хинтинг.
        #[qinvokable]
        #[cxx_name = "setFontRendering"]
        fn set_font_rendering(
            self: &SettingsActions,
            antialias: bool,
            exclude: bool,
            exclude_from: i32,
            exclude_to: i32,
            subpixel: &QString,
            hinting: &QString,
        ) -> QString;

        /// «Изменить все шрифты»: окно выбора, затем одно семейство и/или
        /// кегль всем шрифтам окружения. `mode` — `both|family|size`.
        #[qinvokable]
        #[cxx_name = "changeAllFonts"]
        fn change_all_fonts(self: &SettingsActions, mode: &QString) -> QString;

        /// Принудительное DPI шрифтов: `enabled = false` — вернуть «по
        /// экрану».
        #[qinvokable]
        #[cxx_name = "setFontDpi"]
        fn set_font_dpi(self: &SettingsActions, enabled: bool, dpi: i32) -> QString;

        // ── Управление шрифтами ──────────────────────────────────────────

        /// Установленные шрифты по семействам (JSON): подписи, сводка,
        /// варианты фильтров, текст образца, семейства с начертаниями и
        /// файлами — под фильтр `filter` (JSON `qt_settings::FontListFilter`:
        /// место, поиск, производитель, тип файла, письменность).
        /// `full = false` — только подписи (раздел свёрнут).
        #[qinvokable]
        #[cxx_name = "fontLibrary"]
        fn font_library(self: &SettingsActions, full: bool, filter: &QString) -> QString;

        /// Таблица символов начертания (JSON: блоки Юникода с кодами
        /// символов, которые есть в файле шрифта).
        #[qinvokable]
        #[cxx_name = "fontChars"]
        fn font_chars(self: &SettingsActions, file: &QString, style: &QString) -> QString;

        /// Запомнить свой текст образца (пусто — образец по умолчанию) →
        /// текст, которым теперь рисуются образцы.
        #[qinvokable]
        #[cxx_name = "setFontSampleText"]
        fn set_font_sample_text(self: &SettingsActions, text: &QString) -> QString;

        /// Включить или выключить шрифт без удаления (`<rejectfont>` в
        /// нашем блоке `fonts.conf`): всё семейство (`file` пуст) или одно
        /// начертание → итог для человека.
        #[qinvokable]
        #[cxx_name = "setFontEnabled"]
        fn set_font_enabled(self: &SettingsActions, family: &QString, file: &QString, enabled: bool) -> QString;

        /// Свои группы шрифтов (`fontgroups.xml`, общий с KDE): `op` —
        /// `create` (`arg` — имя), `rename` (`arg` — новое имя), `remove`
        /// (с вопросом), `add` / `take` (`arg` — семейство) → JSON
        /// `{"text": итог, "origin": что выбрать в фильтре после правки}`.
        #[qinvokable]
        #[cxx_name = "fontGroupEdit"]
        fn font_group_edit(self: &SettingsActions, op: &QString, group: &QString, arg: &QString) -> QString;

        /// Включить или выключить все шрифты своей группы → итог.
        #[qinvokable]
        #[cxx_name = "setFontGroupEnabled"]
        fn set_font_group_enabled(self: &SettingsActions, group: &QString, enabled: bool) -> QString;

        /// Экспорт группы: окно сохранения, затем ZIP с файлами её шрифтов
        /// → итог (пусто — человек передумал).
        #[qinvokable]
        #[cxx_name = "exportFontGroup"]
        fn export_font_group(self: &SettingsActions, group: &QString) -> QString;

        /// Дубликаты: одно начертание в нескольких файлах (JSON: сводка и
        /// группы с копиями). Удаляются копии тем же `removeFont`.
        #[qinvokable]
        #[cxx_name = "fontDuplicates"]
        fn font_duplicates(self: &SettingsActions) -> QString;

        /// Установить шрифт из файла (окно выбора) → итог для человека.
        /// `scope` — для кого (`qt_settings::FONT_INSTALL_SCOPES`): «для
        /// всех» идёт через сеанс прав в фоне, итог — сигналом `status`.
        #[qinvokable]
        #[cxx_name = "installFontFile"]
        fn install_font_file(self: Pin<&mut SettingsActions>, scope: &QString) -> QString;

        /// Удалить шрифт: всё семейство (`file` пуст) или одно начертание
        /// (`file`). Свои — сразу, поставленные для всех — через сеанс прав
        /// в фоне (итог — сигналом `status`), шрифты пакетов — нет. С
        /// вопросом.
        #[qinvokable]
        #[cxx_name = "removeFont"]
        fn remove_font(self: Pin<&mut SettingsActions>, family: &QString, file: &QString) -> QString;

        // ── Защита приложений по умолчанию ───────────────────────────────

        /// Переключить правило защиты (`enabled` — главный выключатель).
        #[qinvokable]
        #[cxx_name = "setProtection"]
        fn set_protection(self: &SettingsActions, key: &QString, enabled: bool) -> QString;

        /// Запомнить нынешние приложения по умолчанию.
        #[qinvokable]
        fn snapshot(self: &SettingsActions) -> QString;

        /// Вернуть запомненные приложения по умолчанию.
        #[qinvokable]
        #[cxx_name = "restoreDefaults"]
        fn restore_defaults(self: &SettingsActions) -> QString;

        // ── Копии панели Plasma ──────────────────────────────────────────

        /// Снять копию панели Plasma.
        #[qinvokable]
        #[cxx_name = "plasmaBackup"]
        fn plasma_backup(self: &SettingsActions) -> QString;

        /// Тексты вопроса перед возвратом копии: `{title, question}` (JSON).
        #[qinvokable]
        #[cxx_name = "plasmaConfirm"]
        fn plasma_confirm(self: &SettingsActions, timestamp: &QString) -> QString;

        /// Вернуть копию панели Plasma.
        #[qinvokable]
        #[cxx_name = "plasmaRestore"]
        fn plasma_restore(self: &SettingsActions, timestamp: &QString) -> QString;

        // ── Оформление рабочего стола ────────────────────────────────────

        /// Проверять ли оформление при запуске Rexon.
        #[qinvokable]
        #[cxx_name = "setAeroshell"]
        fn set_aeroshell(self: &SettingsActions, enabled: bool) -> QString;

        /// Сверить оформление прямо сейчас. Сверка идёт в фоне; отстало —
        /// окно вопроса, после «Восстановить» — окно хода пересборки.
        #[qinvokable]
        #[cxx_name = "aeroshellCheckNow"]
        fn aeroshell_check_now(self: Pin<&mut SettingsActions>) -> QString;

        /// Проверка оформления при запуске Rexon (зовёт главное окно). Та же
        /// сверка, но молча, если всё в порядке или проверка выключена в
        /// настройках.
        #[qinvokable]
        #[cxx_name = "aeroshellStartupCheck"]
        fn aeroshell_startup_check(self: Pin<&mut SettingsActions>);
    }

    // Объявление трейта обязано быть В МОСТЕ: cxx-qt печатает по нему алиас
    // типа потока, без которого фоновый импорт не доедет до сигнала.
    impl cxx_qt::Threading for SettingsActions {}
}

use core::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cxx_qt::{CxxQtThread, CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QUrl, QVariant};
use serde_json::{json, Map, Value};

use crate::desktops_actions::OpView;
use rexon_core::de_progress as dp;
use rexon_core::qt_settings::{self as qs, SoundSetting};
use rexon_core::system_de_exec as dx;
use rexon_core::wallpaper_exec::{
    self, live_slot, try_with_slot, LiveSys, WallpaperExec, WallpaperResult, WpSys,
};
use rexon_core::{
    aeroshell, appdefaults, backup, defaults, engine_update, ext_exec, gtk_sanitizer, importers,
    onboarding, qt_app, screenshots, settings as core_settings, system, system_plan, ui_base,
    utils, wallpaper_audio, wallpaper_engine, youtube_wallpaper,
};

// ============================================================================
// Состояние объекта
// ============================================================================

/// То, что живёт РОВНО столько же, сколько открытая вкладка, и на диске не
/// хранится: свойства запуска и дорогие снимки.
///
/// Дорогие — не фигура речи: чтение трёх системных шрифтов это три запуска
/// команд, а список звуковых тем — обход дерева `sounds`. Полный снимок
/// спрашивается после КАЖДОГО действия (`root.did(...)` в разметке), и без
/// памяти вкладка держала бы машину в постоянной работе на ровном месте.
/// Точки сброса те же, что у Python-оригинала: шрифты — после выбора шрифта,
/// темы — никогда за жизнь вкладки (их состав меняется установкой пакетов).
#[derive(Default)]
struct Session {
    /// Звук YouTube-обоев: свойство ЗАПУСКА, а не настройка. `None` — ещё не
    /// трогали, по умолчанию включён.
    youtube_audio: Option<bool>,
    /// Сила размытия, выбранная ползунком в этом окне.
    blur_radius: Option<i32>,
    /// Что показывает раздел импорта до первого импорта.
    import_note: String,
    /// Что показывает раздел комплектности до первой проверки.
    completeness_note: Option<String>,
    /// Снимок системных шрифтов: `цель → готовая подпись`.
    fonts: Option<Vec<(String, String)>>,
    /// Снимок отрисовки шрифтов (сглаживание, субпиксели, хинтинг): чтение
    /// — до четырёх запусков команд, поэтому держится до записи.
    rendering: Option<qs::FontRendering>,
    /// Снимок принудительного DPI шрифтов (`Some(None)` — не задано).
    font_dpi: Option<Option<i32>>,
    /// Установленные шрифты для «Управления шрифтами» (`fc-list`):
    /// держится до установки или удаления шрифта.
    font_library: Option<Vec<qs::InstalledFont>>,
    /// Список установленных звуковых тем.
    sound_themes: Option<Vec<String>>,
    /// Версия yt-dlp, если он есть. Спрашивается запуском программы, а
    /// поставить либо обновить её эта вкладка пока не умеет — значит, за
    /// жизнь вкладки ответ не меняется.
    ytdlp_version: Option<String>,
    /// Установленные обои для библиотеки. Обход каталога расширений читает
    /// каждый манифест, поэтому список держится до события, которое его
    /// меняет, — импорта темы (тот же порядок, что у Python `_lib_cache`).
    library: Option<Vec<wallpaper_engine::LibraryItem>>,
}

/// Rust-состояние объекта действий.
#[derive(Default)]
pub struct SettingsActionsRust {
    session: Mutex<Session>,
    /// Идёт ли импорт темы Windows. Отдельным флажком, а не полем `Session`:
    /// его читает и гасит фоновый поток, у которого доступа к объекту нет.
    import_busy: Arc<AtomicBool>,
    // Своего исполнителя обоев здесь НЕТ: он один на процесс
    // (`wallpaper_exec::live_slot`), общий со страницей «Steam». Прежний
    // слот вкладки не давал «Остановить» снять видео, поставленное оттуда.
    /// Идёт ли применение обоев (тем же порядком, что и импорт).
    wallpaper_busy: Arc<AtomicBool>,
    /// Идёт ли установка либо обновление yt-dlp.
    ytdlp_busy: Arc<AtomicBool>,
    /// Занята ли кнопка «Проверить и доустановить»: от нажатия до итога,
    /// включая открытое окно вопроса.
    completeness_busy: Arc<AtomicBool>,
    /// Окно операции — доустановки или пересборки оформления (общий вид с
    /// «Рабочими столами»).
    op: OpView,
    /// Что ждёт «Да» в окне вопроса.
    pending: Option<PendingOp>,
    /// Флаг отмены идущей операции.
    cancel: Option<dx::CancelToken>,
    /// Что сторож обновлений нашёл ещё после удаления сироты: покажется,
    /// когда человек закроет окно с итогом удаления.
    update_guard_rest: Option<rexon_core::update_guard::Advice>,
}

/// Операция, которая ждёт «Да» в окне вопроса.
enum PendingOp {
    /// Экран входа сам открывал сеанс, на котором держится сирота: сделать
    /// пару сеанса на Wayland сеансом по умолчанию, потом снова предложить
    /// удалить сирот.
    WaylandDefault(&'static rexon_core::system_plan::SessionSuccessor, Vec<String>),
    /// Доустановка недостающего: ключ проверки и подпись для человека.
    Completeness(Vec<(String, String)>),
    /// Пересборка оформления AeroShell: пакеты.
    Aeroshell(Vec<String>),
    /// Возврат штатной Plasma вместо форка, оставшегося без оболочки.
    RestoreStock(Vec<String>),
    /// Удалить пакеты-сироты, найденные сторожем обновлений.
    RemoveOrphans(Vec<String>),
    /// Окно сторожа: «Вернуть прежние версии» — дальше окно подтверждения
    /// с перечнем «было → станет».
    RollbackAsk(rexon_core::update_guard::RollbackOffer),
    /// Подтверждено: вернуть прежние версии набора.
    Rollback(rexon_core::update_guard::RollbackOffer),
}

/// Идёт ли сверка либо пересборка оформления — на ВЕСЬ процесс. Объектов
/// действий два (у главного окна для проверки при запуске и у страницы
/// настроек), а сборка на машине должна идти одна.
static AEROSHELL_BUSY: AtomicBool = AtomicBool::new(false);

/// Что сейчас с обоями у исполнителя слота: идут ли, формат, путь. Замок НЕ
/// ждём (снимок спрашивается после каждого нажатия): занят применением с
/// любой страницы — ответ «не играют», следующий снимок скажет правду.
/// Исполнителя здесь не заводим: смотреть не на что.
fn wallpaper_now<S: WpSys>(
    slot: &Mutex<Option<WallpaperExec<S>>>,
) -> (bool, Option<String>, Option<String>) {
    match slot.try_lock() {
        Ok(mut guard) => match guard.as_mut() {
            Some(exec) => {
                let st = exec.status();
                (st.running, st.format, st.path)
            }
            None => (false, None, None),
        },
        Err(_) => (false, None, None),
    }
}

/// «Остановить живые обои» исполнителем слота — какой бы страницей они ни
/// были поставлены. `None` — исполнитель занят, ждать его в потоке окна
/// нельзя.
fn stop_wallpaper_in<S: WpSys + Default>(
    slot: &Mutex<Option<WallpaperExec<S>>>,
) -> Option<WallpaperResult> {
    try_with_slot(slot, |exec| exec.stop(true))
}

/// Гасит флажок занятости, когда выходит из области видимости — в том числе
/// при панике фоновой работы.
struct BusyGuard(Arc<AtomicBool>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Что сейчас с обоями — наблюдения, из которых собирается раздел «Обои».
struct WallpaperView {
    running: bool,
    format: Option<String>,
    path: Option<String>,
    audio: wallpaper_audio::AudioSettings,
    youtube_audio: bool,
    /// Версия yt-dlp; `None` — его нет в системе.
    ytdlp_version: Option<String>,
    /// Идёт ли установка либо обновление yt-dlp.
    ytdlp_busy: bool,
    busy: bool,
}

/// Раздел «Обои» снимка — те же ключи, что у Python (`_wallpaper_state`).
/// Слова и решения — ядра; здесь только раскладка по ключам страницы.
fn wallpaper_section(view: &WallpaperView) -> Value {
    let (ytdlp_label, ytdlp_button, ytdlp_action) = qs::ytdlp_row(
        view.ytdlp_version.is_some(),
        view.ytdlp_version.as_deref().unwrap_or(""),
    );
    json!({
        // Исполнитель ядра слинкован в бинарь — раздел есть всегда.
        "engine": true,
        "running": view.running,
        "status": qs::wallpaper_status_text(
            view.running,
            view.format.as_deref(),
            view.path.as_deref(),
        ),
        "audio": {
            "enabled": view.audio.enabled,
            "volume": view.audio.volume_pct,
            "muteOther": view.audio.mute_when_other_audio,
            "hideCovered": wallpaper_audio::hide_when_covered(&view.audio.actions),
        },
        "ytAudio": view.youtube_audio,
        "ytdlpLabel": ytdlp_label,
        "ytdlpButton": ytdlp_button,
        "ytdlpAction": ytdlp_action,
        "ytdlpBusy": view.ytdlp_busy,
        "busy": view.busy,
    })
}

impl SettingsActionsRust {
    /// Сессия под замком. Отравленный замок не роняет вкладку: он значит, что
    /// фоновый поток упал с паникой, а показать снимок машины человеку всё
    /// равно надо.
    fn session(&self) -> std::sync::MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }
}

// ============================================================================
// Границы, названные вслух
// ============================================================================

// Что именно не перенесено — для разработчика, покупателю НЕ показывается
// (правило #13: ни имён модулей, ни polkit, ни путей к .py в интерфейсе).
//
//   * Установка, смена и удаление стола — см. шапку `desktops_actions.rs`.

// ⭐⭐ Починка 16.09.2026. Здесь жила общая формулировка границы
// `classic_window_only` — «эта функция пока есть только в классическом окне
// Rexon, щёлкните правой кнопкой мыши по ярлыку и выберите „Rexon
// (классическое окно)“». Она СНЯТА, и вот почему:
//
//   * совет стал неправдой в тот же день: пункт `[Desktop Action Classic]`
//     из ярлыка убран (40620d2) — человек щёлкнул бы правой кнопкой и не
//     нашёл того, что ему назвали. Подсказка, ведущая в пустоту, хуже
//     отсутствия подсказки;
//   * вести стало некуда и по существу: классического окна не везёт ни
//     пакет дистрибутива, ни ручная установка, а сама Python-часть уходит
//     (решение владельца 16.09);
//   * звать её было уже некому: границ в классическое окно на страницах не
//     осталось (последним переехал Flash), функция стояла под
//     `#[allow(dead_code)]`, и трогали её только сверки.
//
// Договор «ни одна кнопка не молчит» (правило #13) при этом не отменён —
// его держит сторож `nothing_goes_to_the_classic_window` ниже: НОВАЯ граница
// не имеет права появиться молча, а отсылать в снятое окно ей больше нечем.
use rexon_core::installer as inst;

/// Почему окно выбора шрифта не открылось. Это НЕ граница переноса: шим
/// диалогов есть, но `FontDialog` из `QtQuick.Dialogs` мог не подняться —
/// например, в системе не установлен модуль `QtQuick.Dialogs` вовсе. Молчать
/// об этом нельзя: кнопка, после которой ничего не происходит, читается как
/// поломка программы (правило #13). Имя пакета оставлено — по нему человек
/// может доустановить недостающее.
fn font_dialog_failed() -> String {
    // TRANSLATORS: shown after the font kind, e.g. “System Font — …”.
    rexon_core::i18n::tr("the font chooser didn't open: the part of Qt that draws such windows is \
                          missing on the system (package qt6-declarative)")
}

// ============================================================================
// Доустановка недостающего: доставка хода из рабочего потока
// ============================================================================

/// Причина в строке «Ошибка проверки: …», когда рабочий поток упал сам.
fn completeness_internal_error() -> String {
    // TRANSLATORS: the reason shown after “Check failed:” when the check
    // itself crashed.
    rexon_core::i18n::tr("internal Rexon error, please try again")
}

/// Что написать в разделе после доустановки. Удача — итог проверки ядра
/// словами; отказ пароля, отмена и частичный итог — первая строка итога
/// исполнителя: сводка «не удалось: N» по отказанным командам читалась бы как
/// поломка пакетов, а их не трогали вовсе.
fn completeness_note_after(outcome: &dx::DeOpOutcome, results: &[(String, String)]) -> String {
    if outcome.ok && !results.is_empty() {
        return qs::compliance_summary(results);
    }
    let first = outcome.message.lines().next().unwrap_or("").trim();
    if outcome.ok {
        format!("✅ {first}")
    } else {
        format!("⚠️ {first}")
    }
}

/// Окно вопроса «оформление перестало работать»: тексты и кнопки ядра, те же,
/// что у Python-окна (`offer_aeroshell_rebuild`). Имена пакетов и версии в
/// вопрос не идут — они в журнале программы.
///
/// Журнал сеанса назвал, что именно выключено (`report.journal`), — эти
/// строки идут в текст: «эффект такой-то выключен: собран под KWin X,
/// установлен Y — пересобрать пакет».
fn aeroshell_confirm_view(report: &aeroshell::Report) -> OpView {
    OpView {
        yes_text: aeroshell::confirm_button_for(report),
        no_text: dp::aeroshell_skip_button(),
        ..OpView::confirm(&aeroshell::confirm_title_for(report), &aeroshell::confirm_text_for(report))
    }
}

/// Окно о сторонних модулях из журнала сеанса (эффекты KWin, виджеты,
/// оформление окон не из набора): есть что пересобрать из AUR — вопрос с
/// кнопкой пересборки, нет — только сказать, что делать.
fn broken_plugins_view(report: &aeroshell::Report, rebuild: bool) -> OpView {
    let title = rexon_core::plugin_journal::plugins_title();
    let text = aeroshell::plugins_text(report, rebuild);
    if rebuild {
        OpView {
            yes_text: dp::aeroshell_fix_button(),
            no_text: dp::aeroshell_skip_button(),
            ..OpView::confirm(&title, &text)
        }
    } else {
        OpView {
            phase: "blocked".to_string(),
            title,
            text,
            ..OpView::default()
        }
    }
}

/// Окно вопроса «форк остался без оболочки — вернуть штатную Plasma».
fn restore_stock_confirm_view() -> OpView {
    OpView {
        yes_text: aeroshell::restore_button(),
        no_text: dp::aeroshell_skip_button(),
        ..OpView::confirm(
            &aeroshell::restore_title(),
            &format!("{}<br><br>{}", aeroshell::restore_text(), aeroshell::restore_informative()),
        )
    }
}

/// Окно-сообщение без действия (одна кнопка «Понятно»): сказать, почему
/// Rexon ничего не предлагает. Текст — обычный, экранируется здесь.
fn aeroshell_notice_view(title: &str, text: &str) -> OpView {
    OpView {
        phase: "blocked".to_string(),
        title: title.to_string(),
        text: rexon_core::ui_base::escape_dialog_text(text).replace('\n', "<br>"),
        ..OpView::default()
    }
}

/// Приёмник хода исполнителя ядра: каждое событие уходит в поток окна.
struct SettingsSink {
    handle: CxxQtThread<qobject::SettingsActions>,
}

impl dx::DeOpSink for SettingsSink {
    fn progress(&self, stage: &str, message: &str, fraction: Option<f64>) {
        let (stage, message) = (stage.to_string(), message.to_string());
        deliver(&self.handle, move |object| {
            object.apply_op_progress(stage, message, fraction);
        });
    }

    fn cancellable(&self, allowed: bool) {
        deliver(&self.handle, move |object| object.apply_op_cancellable(allowed));
    }
}

/// Поставить работу в поток окна; вкладка уже закрыта — молча, иначе в журнал.
fn deliver(
    handle: &CxxQtThread<qobject::SettingsActions>,
    work: impl FnOnce(Pin<&mut qobject::SettingsActions>) + Send + 'static,
) {
    if let Err(e) = handle.queue(work) {
        if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
            rexon_core::log_warn!("settings", "ход доустановки не доставлен вкладке: {e:?}");
        }
    }
}

// ============================================================================
// Мелочи перевода
// ============================================================================

/// Подпись из каталога ядра.
fn t(key: &str) -> String {
    qs::text(key, &[]).unwrap_or_default()
}

/// Подпись из каталога ядра с одной подстановкой.
fn t1(key: &str, a: &str) -> String {
    qs::text(key, &[a.to_string()]).unwrap_or_default()
}

/// Домашний каталог (`os.path.expanduser("~")` оригинала).
fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

/// Корень пользовательских конфигов (`$XDG_CONFIG_HOME`, иначе `~/.config`).
fn config_home() -> String {
    let xdg = std::env::var("XDG_CONFIG_HOME").unwrap_or_default();
    utils::config_home(
        if xdg.is_empty() { None } else { Some(xdg.as_str()) },
        &home(),
    )
}

/// Корень пользовательских данных (`$XDG_DATA_HOME`, иначе `~/.local/share`).
fn data_home() -> String {
    match std::env::var("XDG_DATA_HOME") {
        Ok(v) if !v.is_empty() => v,
        _ => format!("{}/.local/share", home()),
    }
}

/// Текущее окружение рабочего стола; пусто — не опознано.
///
/// Тот же ответ, что даёт `SystemInfo.current_de` на Python и `current_de()`
/// в `desktops_actions.rs`: одна дверь, вторых таблиц окружений нет.
pub(crate) fn current_de() -> String {
    let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = std::env::var("DESKTOP_SESSION").unwrap_or_default();
    let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
    system::detect_current_de(&xdg, &session, wayland).unwrap_or_default()
}

/// Есть ли бинарь в PATH.
fn has(binary: &str) -> bool {
    utils::which(binary, None).is_some()
}

/// Найденный `kreadconfig6`/`kwriteconfig6`, либо пусто (порт `_kde_tool`).
fn kde_tool(name: &str) -> String {
    utils::which(name, None).unwrap_or_default()
}

/// Вывод команды чтения; пусто — не вышло (порт `_read_cmd`).
fn read_cmd(argv: &[String]) -> String {
    if argv.is_empty() {
        return String::new();
    }
    let report = utils::run(
        &utils::RunSpec {
            argv: argv.to_vec(),
            output: utils::RunOutput::Capture,
            timeout_secs: Some(5),
            lang_c: true,
            ..Default::default()
        },
        None,
    );
    if report.returncode != Some(0) {
        return String::new();
    }
    utils::decode_text(&report.stdout)
}

/// Запустить команду записи → удалось ли (порт `_write_cmd`).
fn write_cmd(argv: &[String]) -> bool {
    if argv.is_empty() {
        return false;
    }
    let report = utils::run(
        &utils::RunSpec {
            argv: argv.to_vec(),
            output: utils::RunOutput::Capture,
            timeout_secs: Some(10),
            ..Default::default()
        },
        None,
    );
    report.returncode == Some(0)
}

/// Прочитать файл целиком; `None` — нет файла или не читается.
fn read_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Разобранный JSON файла; пустой объект — файла нет либо он битый.
///
/// Битый файл настроек не должен ронять вкладку: человек не поймёт, что
/// случилось, а вкладка — единственное место, где это чинится.
fn read_json(path: &str) -> Value {
    read_file(path)
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| Value::Object(Map::new()))
}

/// Запись файла общей дверью ядра ([`rexon_core::utils::atomic_write`]).
fn write_file_atomic(path: &str, content: &str) -> Result<(), std::io::Error> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    rexon_core::utils::atomic_write(path, content)
}

/// Лежит ли такой `.desktop` в каталогах приложений (порт `_desktop_exists`).
fn desktop_exists(desktop_id: &str) -> bool {
    defaults::desktop_search_paths(desktop_id, &home())
        .iter()
        .any(|p| std::path::Path::new(p).is_file())
}

/// Семейство пакетного менеджера этой машины — для подсказок «установите пакет».
fn pkg_family() -> String {
    system::DistroDetector::detect_live()
        .family_live()
        .as_str()
        .to_string()
}

// ============================================================================
// Снимки машины
// ============================================================================

/// Ключи снимка `state()`, которые страница читает точкой (`root.st.wp`,
/// `root.st.prot`…).
///
/// Список назван отдельно не для красоты: забытый ключ приезжает в QML как
/// `undefined`, а `undefined` в переключателе — молчаливое «выключено».
/// Такой разъезд не виден ни на сборке, ни глазом.
const STATE_KEYS: [&str; 18] = [
    "polkit", "lang", "skin", "onboarding", "tray", "fm", "quickfix", "compl", "wp", "lib", "imp",
    "blur", "amb", "snd", "font", "prot", "plasma", "aero",
];

/// Собрать снимок по СПИСКУ ключей — единственное место, где состав ответа
/// `state()` записан.
fn state_object(values: [Value; 18]) -> Value {
    let mut out = Map::new();
    for (key, value) in STATE_KEYS.into_iter().zip(values) {
        out.insert(key.to_string(), value);
    }
    Value::Object(out)
}

impl qobject::SettingsActions {
    fn facts(&self) -> QString {
        let de = current_de();
        let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let session = std::env::var("DESKTOP_SESSION").unwrap_or_default();

        // Вводный показ: сценарий и тексты живут в ядре и слинкованы в этот
        // бинарь — значит, он доступен всегда. У Python здесь стояла проверка
        // «поднялся ли мост `rexon_py`», которой в Rust-процессе нет смысла.
        let onboarding_available = !onboarding::STEPS.is_empty();

        let plasma = backup::is_plasma_desktop(&xdg) || !plasma_backups().is_empty();
        let aeroshell = !plasma_forks().is_empty();

        QString::from(
            &json!({
                "onboarding": onboarding_available,
                "mate": de == "mate",
                "plasma": plasma,
                "aeroshell": aeroshell,
                "deDisplay": qs::de_display(&de, &xdg, &session),
            })
            .to_string(),
        )
    }

    fn state(&self) -> QString {
        let de = current_de();
        let manager = core_settings::SettingsManager::new().ok();
        let get = |key: &str| -> Option<Value> {
            manager
                .as_ref()
                .and_then(|m| m.get(key))
                .cloned()
        };

        let snapshot = state_object([
            // polkit — одна причина на всё привилегированное, сказанная один
            // раз и вверху страницы.
            Value::from(polkit_banner()),
            language_state(),
            // ⭐ Починка: скин берётся тем же путём, что и палитра страницы.
            Value::from(ui_base::active_skin_id_from(
                get("ui_skin").as_ref().and_then(Value::as_str),
            )),
            // Ключа ещё нет = первый запуск, показ будет предложен.
            Value::from(get(onboarding::KEY_ENABLED) != Some(Value::Bool(false))),
            json!({
                "autostart": crate::tray::autostart_enabled(),
                // Спрашиваем сам Qt, а не гадаем по имени оболочки.
                "available": crate::tray::available(),
                // Докачка значков и курсора к теме (по умолчанию — да).
                "themeDepsAuto": rexon_core::theme_watch::auto_home_enabled(),
            }),
            file_manager_state(&de, manager.as_ref()),
            quickfix_state(&de),
            self.completeness_state(),
            self.wallpaper_state(),
            self.library_section(),
            self.import_state(),
            self.blur_section(),
            self.ambient_section(),
            self.sound_state(&de),
            self.fonts_state(&de),
            protection_state(),
            plasma_state(),
            json!({ "enabled": get(aeroshell_key()) != Some(Value::Bool(false)) }),
        ]);
        QString::from(&snapshot.to_string())
    }

    fn ambient_state(&self) -> QString {
        QString::from(&self.ambient_section().to_string())
    }

    /// Раздел «Фоновые звуки» — те же ключи, что у Python (`_ambient_state`).
    ///
    /// Звуки играет исполнитель ядра, он и знает, что играет: доигравшие
    /// вычищаются из его списка при каждом опросе. Пока исполнитель занят
    /// фоновым применением обоев, его замок не ждём — строки на этот опрос
    /// пусты, следующий снимок их вернёт.
    ///
    /// ⭐ Починка (перенесена из прежней версии): про `mpv` система
    /// спрашивается ВСЕГДА — Python без движка отвечал «не установлен», не
    /// спрашивая.
    fn ambient_section(&self) -> Value {
        let rows: Vec<Value> = try_with_slot(&live_slot(), |exec| {
            exec.active_ambient()
                .into_iter()
                .map(|sound| {
                    let (name, volume) = qs::ambient_row_texts(
                        Some(&sound.name),
                        Some(&sound.volume.to_string()),
                    );
                    json!({ "id": sound.id, "name": name, "volume": volume })
                })
                .collect()
        })
        .unwrap_or_default();
        json!({ "manager": true, "mpv": has("mpv"), "active": rows })
    }

    /// Раздел «Комплектность рабочего стола».
    fn completeness_state(&self) -> Value {
        let busy = self.rust().completeness_busy.load(Ordering::SeqCst);
        let state = self.rust().session();
        json!({
            "text": state
                .completeness_note
                .clone()
                .unwrap_or_else(|| t("compl.idle")),
            // Кнопка гаснет от нажатия до итога — и пока открыт вопрос.
            "busy": busy,
        })
    }

    /// Раздел «Обои» — те же ключи, что у Python (`_wallpaper_state`).
    ///
    /// Исполнитель ядра слинкован в бинарь, поэтому раздел есть всегда
    /// (`engine: true`). Пока идёт применение, исполнитель занят фоновым
    /// потоком: его замок НЕ ждём — снимок спрашивается после каждого
    /// нажатия, и ожидание заморозило бы вкладку на время команды оболочки.
    /// Исполнитель общий на процесс, поэтому здесь видны и обои,
    /// поставленные со страницы «Steam».
    fn wallpaper_state(&self) -> Value {
        let state = self.rust();
        let busy = state.wallpaper_busy.load(Ordering::SeqCst);
        let (running, format, path) = wallpaper_now(&live_slot());

        // Карта целиком, со слитыми умолчаниями, — ровно то, что читал
        // Python (`self._settings.settings`), и тем же чтением ядра.
        let audio = wallpaper_audio::AudioSettings::from_map(&core_settings::load_settings(
            &core_settings::settings_file(),
        ));

        let ytdlp_version = if has("yt-dlp") {
            Some(
                state
                    .session()
                    .ytdlp_version
                    .get_or_insert_with(|| {
                        let out = read_cmd(&youtube_wallpaper::version_argv());
                        let out = out.trim();
                        // Тот же ответ, что давал Python на неудачный опрос.
                        if out.is_empty() {
                            "?".to_string()
                        } else {
                            out.to_string()
                        }
                    })
                    .clone(),
            )
        } else {
            None
        };

        wallpaper_section(&WallpaperView {
            running,
            format,
            path,
            audio,
            youtube_audio: state.session().youtube_audio.unwrap_or(true),
            ytdlp_version,
            ytdlp_busy: state.ytdlp_busy.load(Ordering::SeqCst),
            busy,
        })
    }

    /// Раздел «Размытие обоев» — те же ключи, что у Python (`_blur_state`).
    ///
    /// `pillow` здесь всегда истина: размытие делает ядро своим кодом, и
    /// сторонняя библиотека Python ему не нужна — предупреждение «нет
    /// Pillow» было бы неправдой.
    fn blur_section(&self) -> Value {
        let state = self.rust();
        json!({
            "engine": true,
            "pillow": true,
            "radius": state.session().blur_radius.unwrap_or(qs::BLUR_RANGE.3),
            "busy": state.wallpaper_busy.load(Ordering::SeqCst),
        })
    }

    /// Раздел «Библиотека обоев» — те же ключи, что у Python
    /// (`_library_state`). Список пакетов читает ядро
    /// (`wallpaper_engine::list_installed_wallpapers`).
    fn library_section(&self) -> Value {
        let items = self
            .rust()
            .session()
            .library
            .get_or_insert_with(|| {
                let dir = rexon_core::qt_extensions::extensions_dir();
                wallpaper_engine::list_installed_wallpapers(&dir.to_string_lossy())
            })
            .clone();
        let rows: Vec<Value> = items
            .iter()
            .map(|item| {
                // Картинка отдаётся адресом, а не путём: QML берёт источник по
                // URL и путь без схемы молча не открыл бы (как у Python).
                let preview = if std::path::Path::new(&item.preview).is_file() {
                    QUrl::from_local_file(&QString::from(&item.preview))
                        .to_qstring()
                        .to_string()
                } else {
                    String::new()
                };
                json!({
                    "name": if item.name.is_empty() { t("lib.unnamed") } else { item.name.clone() },
                    "path": item.path,
                    "format": item.format,
                    // Подпись для человека — из ядра; служебное `format`
                    // остаётся для логики карточки (просмотр, применение).
                    "formatLabel": wallpaper_engine::format_title(&item.format),
                    "preview": preview,
                })
            })
            .collect();
        json!({ "engine": true, "items": rows })
    }

    /// Применить обои в фоновом потоке — одна дверь для «Картинки…» и
    /// библиотеки.
    fn start_wallpaper_apply(self: Pin<&mut Self>, path: String, fmt: String) {
        // Название сцены пустое: у файла его нет, а выдумывать его из имени
        // файла исполнитель запрещает.
        self.run_wallpaper_job(move |exec| {
            let res = exec.apply(&path, &fmt, "");
            qs::wallpaper_done(res.ok, &res.message)
        });
    }

    /// Долгая работа исполнителя обоев в фоновом потоке — одна дверь для
    /// применения и размытия. Команды оболочки и обработка картинки — это
    /// секунды; поток нужен ровно затем, чтобы окно в это время не стояло.
    /// `job` возвращает строку итога — она приходит сигналом `status`.
    fn run_wallpaper_job(
        mut self: Pin<&mut Self>,
        job: impl FnOnce(&mut WallpaperExec<LiveSys>) -> String + Send + 'static,
    ) {
        let flag = self.as_ref().get_ref().rust().wallpaper_busy.clone();
        flag.store(true, Ordering::SeqCst);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // Флажок гаснет и при панике исполнителя: иначе раздел навсегда
            // остался бы «занятым», и кнопки не отвечали бы до перезапуска.
            let busy = BusyGuard(flag);
            // Исполнитель процесса (общий со «Steam») и общая дверь ядра:
            // живым обоям, которым нужен надзор за нагрузкой (видео,
            // YouTube), она его поднимет; остальным — откажется сама.
            let status = wallpaper_exec::run_in_slot(&live_slot(), job);
            // Гасим ДО сигнала: страница перечитает снимок по `refreshed`, и
            // горящий флажок показал бы ей «работа идёт».
            drop(busy);
            let queued = handle.queue(move |mut object| {
                object.as_mut().refreshed();
                object.as_mut().status(QString::from(&status));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("settings", "итог применения обоев не доставлен: {e:?}");
                }
            }
        });
    }

    /// Раздел «Импорт тем Windows».
    fn import_state(&self) -> Value {
        let busy = self.rust().import_busy.load(Ordering::SeqCst);
        json!({
            // ⭐ Починка: импортёр слинкован в бинарь, проверять нечего.
            "available": true,
            "note": self.rust().session().import_note.clone(),
            "busy": busy,
        })
    }

    /// Раздел «Звуки событий».
    fn sound_state(&self, de: &str) -> Value {
        let dirs = qs::sound_theme_dirs(&data_home());
        let themes = {
            let mut state = self.rust().session();
            state
                .sound_themes
                .get_or_insert_with(|| qs::list_sound_themes(&dirs))
                .clone()
        };
        // Тема отдаётся КАК ЕСТЬ, даже пустая или незнакомая: подстановка
        // первой установленной показывала человеку чужое значение как его
        // собственное. Строку «— не задана —» рисует ядро.
        let current = qs::parse_sound_theme(&read_cmd(
            &qs::sound_read_argv_for(de, SoundSetting::Theme, &kde_tool("kreadconfig6"))
                .unwrap_or_default(),
        ));
        // Кнопки «послушать» остаются и при незаданной теме: система в этом
        // случае играет `freedesktop`, и тест звука обязан играть ровно то
        // же самое. Подстановку делает ядро (`effective_sound_theme`) — в
        // подпись списка она не попадает.
        let showcase = qs::showcase_events(
            &qs::list_theme_events(&dirs, &current),
            qs::SOUND_PREVIEW_BUTTONS,
        );
        json!({
            "themes": themes,
            "current": current,
            "events": qs::parse_event_sounds(&read_cmd(
                &qs::sound_read_argv_for(de, SoundSetting::EventSounds,
                                         &kde_tool("kreadconfig6"))
                    .unwrap_or_default(),
            )),
            "showcase": showcase,
        })
    }

    /// Раздел «Системные шрифты»: `цель → готовая подпись`.
    fn fonts_state(&self, de: &str) -> Value {
        let cached = {
            let mut state = self.rust().session();
            state
                .fonts
                .get_or_insert_with(|| {
                    qs::font_targets_for_wm(de, title_wm(de))
                        .into_iter()
                        .map(|target| {
                            (
                                (*target).to_string(),
                                qs::font_value_text(&read_font(de, target)),
                            )
                        })
                        .collect()
                })
                .clone()
        };
        let mut values = Map::new();
        for (target, text) in cached {
            values.insert(target, Value::from(text));
        }
        let rendering = self
            .rust()
            .session()
            .rendering
            .get_or_insert_with(|| read_rendering(de))
            .clone();
        let dpi = *self.rust().session().font_dpi.get_or_insert_with(|| read_font_dpi(de));
        let store = font_dpi_store(de);
        let mut scope = qs::extra_fonts_scope_note(de);
        if let Some(why) = qs::title_font_note(de, title_wm(de)) {
            scope.push(' ');
            scope.push_str(&why);
        }
        let (label, tip, _title) = qs::change_all_texts();
        let modes: Vec<Value> = qs::change_all_mode_choices()
            .into_iter()
            .map(|(id, label)| json!({ "id": id, "label": label }))
            .collect();
        json!({
            "values": values,
            "scope": scope,
            "render": rendering_state(&rendering),
            "all": { "label": label, "tip": tip, "modes": modes },
            "dpi": {
                "available": store.is_some(),
                "enabled": dpi.is_some(),
                "value": dpi.unwrap_or(qs::FONT_BASE_DPI),
                "min": qs::FONT_DPI_RANGE.0,
                "max": qs::FONT_DPI_RANGE.1,
                "note": qs::font_dpi_note_for(de, store.as_ref()),
            },
        })
    }
}

/// Где в этом сеансе держится DPI шрифтов (`None` — задать нечем).
fn font_dpi_store(de: &str) -> Option<qs::FontDpiStore> {
    qs::font_dpi_store(de, wayland_session())
}

/// Заданное DPI шрифтов (`None` — по экрану).
fn read_font_dpi(de: &str) -> Option<i32> {
    let store = font_dpi_store(de)?;
    match qs::font_dpi_read_argv(&store, &kde_tool("kreadconfig6")) {
        Some(argv) => qs::parse_font_dpi(&store, &read_cmd(&argv)),
        None if store == qs::FontDpiStore::Xresources => {
            read_file(&xresources_path()).and_then(|t| qs::xresources_dpi(&t))
        }
        None => read_file(&fontconfig_path()).and_then(|t| qs::fontconfig_rendering(&t).dpi),
    }
}

/// Идёт ли сеанс Wayland.
/// Кто рисует заголовки окон в окружении без своего менеджера окон (LXQt,
/// labwc) — `qt_settings::title_wm_for` по `lxqt/session.conf`: свой, затем
/// системные (`$XDG_CONFIG_DIRS`, как их ищет `startlxqt`).
pub(crate) fn title_wm(de: &str) -> Option<qs::TitleWm> {
    let mut confs = Vec::new();
    if de == "lxqt" {
        let mut dirs = vec![config_home()];
        let system = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
        dirs.extend(system.split(':').filter(|d| !d.is_empty()).map(str::to_string));
        dirs.extend(["/etc/xdg".to_string(), "/usr/share".to_string()]);
        confs = dirs.iter().filter_map(|d| read_file(&format!("{}/lxqt/session.conf", d.trim_end_matches('/')))).collect();
    }
    let refs: Vec<&str> = confs.iter().map(String::as_str).collect();
    qs::title_wm_for(de, &refs, wayland_session())
}

fn wayland_session() -> bool {
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    rexon_core::wallpaper_mmap::is_wayland_session(&session, &display)
}

/// Путь `~/.Xresources`.
fn xresources_path() -> String {
    format!("{}/{}", home().trim_end_matches('/'), qs::XRESOURCES_REL_PATH)
}

/// Записать `Xft.dpi` в наш блок `~/.Xresources` и, если идёт сеанс X11,
/// влить ту же строку в живую базу ресурсов — новые программы увидят DPI
/// сразу, без перевхода. Снятое DPI из живой базы не вынимается (xrdb
/// умеет снять только всё разом): оно уходит при следующем входе.
fn apply_xresources_dpi(dpi: Option<i32>) -> bool {
    let path = xresources_path();
    let existing = read_file(&path);
    if existing.is_none() && dpi.is_none() {
        // Снимать нечего, а пустой файл заводить незачем.
        return true;
    }
    let Some(text) = qs::xresources_with_dpi(existing.as_deref(), dpi) else {
        return false;
    };
    if text.trim().is_empty() {
        // В файле был только наш блок — пустой файл не оставляем.
        return std::fs::remove_file(&path).is_ok();
    }
    if write_file_atomic(&path, &text).is_err() {
        return false;
    }
    let display = std::env::var("DISPLAY").unwrap_or_default();
    if let (Some(dpi), false, true) = (dpi, display.is_empty() || wayland_session(), has("xrdb")) {
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| std::env::temp_dir().display().to_string());
        let snippet = format!("{}/rexon-xft-dpi.ad", runtime.trim_end_matches('/'));
        if write_file_atomic(&snippet, &qs::xft_dpi_line(dpi)).is_ok() {
            let _ = write_cmd(&qs::xrdb_merge_argv(&snippet));
            let _ = std::fs::remove_file(&snippet);
        }
    }
    true
}

/// Записать DPI шрифтов в хранилище окружения → получилось ли.
fn apply_font_dpi(de: &str, dpi: Option<i32>) -> bool {
    let Some(store) = font_dpi_store(de) else { return false };
    if store == qs::FontDpiStore::Xresources {
        return apply_xresources_dpi(dpi);
    }
    if store != qs::FontDpiStore::Fontconfig {
        return match qs::font_dpi_write_argv(&store, dpi, &kde_tool("kwriteconfig6")) {
            Some(argv) => write_cmd(&argv),
            None => false,
        };
    }
    // LXQt: DPI живёт в том же блоке fonts.conf, что и сглаживание, —
    // блок переписывается целиком с нынешней отрисовкой.
    let path = fontconfig_path();
    let existing = read_file(&path);
    let block = qs::fontconfig_block(&read_rendering(de), dpi);
    let ok = match qs::fontconfig_with_block(existing.as_deref(), &block) {
        Ok(text) => write_file_atomic(&path, &text).is_ok(),
        Err(_) => false,
    };
    // GTK-программы LXQt читают множитель текста GNOME.
    let gnome = qs::FontDpiStore::TextScaling { schema: "org.gnome.desktop.interface".to_string() };
    if ok && has("gsettings") {
        if let Some(argv) = qs::font_dpi_write_argv(&gnome, dpi, "") {
            let _ = write_cmd(&argv);
        }
    }
    // В сеансе X11 — ещё и Xft.dpi: LXQt его в базу ресурсов не кладёт.
    if ok && qs::font_dpi_xresources_also(de, wayland_session()) {
        let _ = apply_xresources_dpi(dpi);
    }
    ok
}

/// Раздел «Отрисовка шрифтов» снимка: нынешние значения и списки выбора.
fn rendering_state(r: &qs::FontRendering) -> Value {
    let choices = |list: Vec<(&str, String)>| -> Vec<Value> {
        list.into_iter().map(|(id, label)| json!({ "id": id, "label": label })).collect()
    };
    let (from, to) = r.exclude.unwrap_or(qs::AA_EXCLUDE_DEFAULT);
    json!({
        "antialias": r.antialias,
        "exclude": r.exclude.is_some(),
        "from": from,
        "to": to,
        "min": qs::AA_EXCLUDE_LIMITS.0,
        "max": qs::AA_EXCLUDE_LIMITS.1,
        "subpixel": r.subpixel,
        "hinting": r.hinting,
        "subpixels": choices(qs::subpixel_choices()),
        "hintings": choices(qs::hinting_choices()),
    })
}

/// Есть ли ключи отрисовки старого GNOME (`qt_settings::LEGACY_XSETTINGS_SCHEMA`).
fn legacy_render_available() -> bool {
    has("gsettings") && qs::legacy_render_available(&read_cmd(&qs::legacy_render_probe_argv()))
}

/// Установленные шрифты по `fc-list` и выключенные Rexon: их `fc-list` не
/// видит, поэтому каждый выключенный файл спрашивается у `fc-query`.
fn read_font_library() -> Vec<qs::InstalledFont> {
    let dirs = font_user_dirs_checked();
    let mut fonts = qs::parse_fc_list_fonts(&read_cmd(&qs::fc_list_fonts_argv()), &dirs);
    for file in read_disabled_fonts() {
        // fontconfig отдаёт выключенный файл ещё какое-то время после записи
        // блока (пока не обновит свой кеш): такой шрифт уже в списке — его
        // помечаем, а не пропускаем. Иначе у только что выключенного шрифта
        // оставалась кнопка «Выключить», и включить его обратно было нечем
        // (приёмка 10.10.2026).
        let mut listed = false;
        for f in fonts.iter_mut().filter(|f| f.file == file) {
            f.disabled = true;
            listed = true;
        }
        if listed || !std::path::Path::new(&file).is_file() {
            continue;
        }
        let out = read_cmd(&qs::fc_query_fonts_argv(&file));
        fonts.extend(qs::parse_fc_list_fonts(&out, &dirs).into_iter().map(|mut f| {
            f.disabled = true;
            f
        }));
    }
    // Письменности, которые fontconfig узнаёт не по языку, а по символу.
    for (script, ch) in qs::FONT_SCRIPT_PROBES {
        qs::font_add_probed_script(&mut fonts, script, &read_cmd(&qs::fc_list_char_argv(*ch)));
    }
    fonts
}

/// Свой текст образца из настроек (или образец по умолчанию).
fn font_sample_text() -> String {
    let settings = core_settings::load_settings(&core_settings::settings_file());
    qs::font_sample_text(settings.get(qs::FONT_SAMPLE_TEXT_KEY).and_then(Value::as_str))
}

/// Файлы, выключенные нашим блоком `fonts.conf`.
fn read_disabled_fonts() -> Vec<String> {
    read_file(&fontconfig_path())
        .map(|text| qs::fontconfig_disabled_files(&text))
        .unwrap_or_default()
}

/// Записать список выключенных шрифтов в наш блок `fonts.conf` → записался
/// ли. `Err` — файл не разбирается или наш блок в нём разорван руками (тогда
/// он не тронут). Блок опустел и в файле не осталось ничего, кроме
/// заведённого нами скелета, — файл удаляется: до Rexon его не было.
fn write_disabled_fonts(path: &str, existing: Option<&str>, files: &[String]) -> Result<bool, qs::FontconfigRefusal> {
    let text = qs::fontconfig_with_disabled(existing, files)?;
    Ok(if qs::fontconfig_is_bare(&text) {
        existing.is_none() || std::fs::remove_file(path).is_ok()
    } else {
        write_file_atomic(path, &text).is_ok()
    })
}

/// Каталоги шрифтов пользователя — как записаны и как они есть на диске
/// (домашний каталог бывает ссылкой: `/home` → `/var/home`).
fn font_user_dirs_checked() -> Vec<String> {
    let mut dirs = qs::font_user_dirs(&home(), &data_home());
    for d in dirs.clone() {
        if let Ok(real) = std::fs::canonicalize(&d) {
            let real = real.to_string_lossy().into_owned();
            if !dirs.contains(&real) {
                dirs.push(real);
            }
        }
    }
    dirs
}

/// Путь файла с развёрнутым каталогом (сама ссылка-файл не разворачивается:
/// удаляется ссылка, а не то, на что она ведёт).
fn real_parent_path(file: &str) -> String {
    let path = std::path::Path::new(file);
    match (path.parent().and_then(|p| std::fs::canonicalize(p).ok()), path.file_name()) {
        (Some(dir), Some(name)) => dir.join(name).to_string_lossy().into_owned(),
        _ => file.to_string(),
    }
}

/// Поставить файл шрифта в каталог шрифтов пользователя и пересобрать его
/// кэш. Дверь одна на продукт — ядро (`local_install::install_font_file`):
/// ею же ставит шрифты «Установить из файла…» вкладки «Оформление».
fn install_font(source: &str) -> qs::FontInstallOutcome {
    rexon_core::local_install::install_font_file(source, &data_home(), true)
}

/// Удалённый выключенный файл больше незачем держать в нашем блоке
/// `fonts.conf`: снять из него те из `files`, которых больше нет.
fn forget_removed_disabled(files: &[String]) {
    if files.is_empty() {
        return;
    }
    let path = fontconfig_path();
    let existing = read_file(&path);
    let disabled = existing.as_deref().map(qs::fontconfig_disabled_files).unwrap_or_default();
    let gone: Vec<String> = files.iter().filter(|f| !std::path::Path::new(f).exists()).cloned().collect();
    if disabled.iter().any(|f| gone.contains(f)) {
        let _ = write_disabled_fonts(&path, existing.as_deref(), &qs::font_disabled_after(&disabled, &gone, true));
    }
}

/// Путь файла своих групп шрифтов — того же, что у KDE.
fn font_groups_path() -> String {
    format!("{}/{}", config_home().trim_end_matches('/'), qs::FONT_GROUPS_REL_PATH)
}

/// Свои группы шрифтов. Нет файла — групп нет; `Err` — файл не разбирается
/// (его не переписываем).
fn read_font_groups() -> Result<Vec<qs::FontGroup>, ()> {
    match read_file(&font_groups_path()) {
        Some(text) => qs::parse_font_groups(&text),
        None => Ok(Vec::new()),
    }
}

/// Путь пользовательского `fonts.conf`.
fn fontconfig_path() -> String {
    format!("{}/{}", config_home().trim_end_matches('/'), qs::FONTCONFIG_REL_PATH)
}

/// Нынешняя отрисовка шрифтов: родные ключи окружения, затем `fonts.conf`
/// (`qt_settings::merge_rendering`).
fn read_rendering(de: &str) -> qs::FontRendering {
    let native = match qs::rendering_read_argv(de, &kde_tool("kreadconfig6")) {
        Some((dialect, reads)) => {
            let answers: Vec<(qs::RenderKey, String)> =
                reads.into_iter().map(|(key, argv)| (key, read_cmd(&argv))).collect();
            qs::parse_render_answers(dialect, &answers)
        }
        None => qs::RenderingParts::default(),
    };
    // Старый GNOME: новых ключей нет, отрисовка — в схеме службы настроек.
    let native = if qs::rendering_needs_legacy(&native, legacy_render_available()) {
        let (dialect, reads) = qs::legacy_rendering_read_argv();
        let answers: Vec<(qs::RenderKey, String)> =
            reads.into_iter().map(|(key, argv)| (key, read_cmd(&argv))).collect();
        qs::parse_render_answers(dialect, &answers)
    } else {
        native
    };
    let fc = read_file(&fontconfig_path())
        .map(|text| qs::fontconfig_rendering(&text))
        .unwrap_or_default();
    qs::merge_rendering(&native, &fc)
}

/// Записать отрисовку шрифтов планом ядра: свой блок в `fonts.conf` (общий
/// слой для всех программ), родные ключи окружения, ключи GNOME для GTK.
/// Принудительное DPI, если оно уже задано блоком, сохраняется.
fn apply_rendering(de: &str, r: &qs::FontRendering) -> qs::RenderingOutcome {
    let path = fontconfig_path();
    let existing = read_file(&path);
    let dpi = existing.as_deref().and_then(|t| qs::fontconfig_rendering(t).dpi);
    let text = match qs::fontconfig_with_block(existing.as_deref(), &qs::fontconfig_block(r, dpi)) {
        Ok(text) => text,
        Err(_) => return qs::RenderingOutcome::FontconfigRefused(path),
    };
    if write_file_atomic(&path, &text).is_err() {
        return qs::RenderingOutcome::FontconfigFailed(path);
    }
    let plan = qs::rendering_plan(de, r, &kde_tool("kwriteconfig6"), has("gsettings"));
    let mut native_ok = true;
    for argv in &plan.primary {
        native_ok &= write_cmd(argv);
    }
    for argv in &plan.also {
        let _ = write_cmd(argv);
    }
    // Старый GNOME держит отрисовку в схеме службы настроек — пишется,
    // только если схема эти ключи ещё знает.
    if legacy_render_available() {
        let mut legacy_ok = true;
        for argv in qs::legacy_rendering_argv(r) {
            legacy_ok &= write_cmd(&argv);
        }
        // Старый GNOME: новых ключей нет, родные — эти.
        if legacy_ok && qs::legacy_render_is_native(de) {
            native_ok = true;
        }
    }
    if native_ok {
        qs::RenderingOutcome::Saved
    } else {
        qs::RenderingOutcome::NativeFailed
    }
}

/// Полоса-предупреждение «в системе нет polkit»; пусто — есть.
fn polkit_banner() -> String {
    if has("pkexec") {
        return String::new();
    }
    let hint = utils::tool_package_hint("pkexec", &pkg_family());
    t1("polkit.banner", &system::polkit_missing_message(&hint))
}

/// Раздел «Файловый менеджер».
fn file_manager_state(de: &str, manager: Option<&core_settings::SettingsManager>) -> Value {
    let current = manager
        .and_then(|m| m.get("file_manager"))
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .to_string();
    let mut badges = Map::new();
    for (fm_id, _label) in qs::fm_choices() {
        if fm_id != "auto" {
            badges.insert(fm_id.clone(), Value::from(has(&fm_id)));
        }
    }
    json!({
        "current": current,
        "badges": Value::Object(badges),
        "currentName": resolve_file_manager(de, &current),
    })
}

/// Какой файловый менеджер стоит на самом деле (`auto` — тот, что родной для
/// этого окружения).
fn resolve_file_manager(de: &str, setting: &str) -> String {
    if setting != "auto" {
        return setting.to_string();
    }
    appdefaults::default_file_manager_for_de(Some(de), &|binary| has(binary))
}

/// Раздел «Родные приложения окружения».
///
/// ⚠️ Имя окружения тут ДРУГОЕ, не то, что в заголовках разделов: заголовки
/// называют СЕССИЮ (`qt_settings::de_display` — «KDE»), а этот раздел
/// возвращает приложения конкретного рабочего стола и обязан звать его полным
/// именем из базы столов («KDE Plasma»). Одно имя на оба случая давало
/// «Восстановить приложения KDE» против «…KDE Plasma» в одном продукте.
fn quickfix_state(de: &str) -> Value {
    let known = appdefaults::de_apps_info(de, &desktop_exists)
        .iter()
        .any(|(_, resolved)| resolved.is_some());
    json!({ "known": known, "de": screenshots::display_name(de) })
}


/// Раздел «Защита приложений по умолчанию».
fn protection_state() -> Value {
    let rules = protection_rules();
    let protected = protected_apps();
    let current = current_defaults();
    let summary = defaults::get_status_summary(&protected, &current, &rules);

    let changed: Vec<(String, String, String)> = summary
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter(|row| row.get("changed") == Some(&Value::Bool(true)))
                .map(|row| {
                    let field = |key: &str| {
                        row.get(key).and_then(Value::as_str).unwrap_or("").to_string()
                    };
                    (
                        defaults::app_type_label(&field("type")).to_string(),
                        field("saved"),
                        field("current"),
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    // Сколько приложений реально запомнено. Пустая строка в файле эталона —
    // это «не запомнено», а не «запомнено пустое»: иначе раздел снова стал бы
    // рапортовать успех, ничего не храня.
    let remembered_count = protected
        .iter()
        .filter(|(_, v)| v.as_deref().is_some_and(|s| !s.trim().is_empty()))
        .count();

    let mut rule_map = Map::new();
    let mut remembered = Map::new();
    for key in qs::protection_types() {
        rule_map.insert(key.to_string(), Value::from(rules.protect(key)));
        remembered.insert(
            key.to_string(),
            Value::from(
                protected
                    .iter()
                    .find(|(k, _)| k == key)
                    .and_then(|(_, v)| v.clone())
                    .unwrap_or_default(),
            ),
        );
    }

    let note = defaults::terminal_status_note(has("xdg-terminal-exec"));
    json!({
        "enabled": rules.enabled(),
        "rules": Value::Object(rule_map),
        "remembered": Value::Object(remembered),
        "note": if note.is_empty() { String::new() } else { t1("prot.note", &note) },
        "noteTip": if note.is_empty() {
            String::new()
        } else {
            utils::tool_package_hint("xdg-terminal-exec", &pkg_family())
        },
        "status": qs::protection_status_text(remembered_count, &changed),
        "canRestore": !changed.is_empty(),
    })
}

/// Раздел «Копии панели Plasma». Список читается по-настоящему: это обычный
/// JSON на диске, снятый живым продуктом, и прятать его было бы враньём.
fn plasma_state() -> Value {
    let backups = plasma_backups();
    let shown: Vec<Value> = backups
        .iter()
        .take(qs::PLASMA_BACKUPS_SHOWN)
        .map(|b| {
            json!({
                "ts": b.timestamp,
                "label": qs::backup_row_label(&b.timestamp, &b.reason, b.files.len() as i64),
                "tip": t1("plasma.backup_dir", &b.timestamp),
            })
        })
        .collect();
    json!({
        "header": qs::plasma_backups_header(backups.len()),
        "backups": shown,
    })
}

// ============================================================================
// Наблюдения, которых у ядра нет
// ============================================================================

/// Каталог копий панели Plasma (`PlasmaProtector._backup_dir`).
fn plasma_backup_dir() -> String {
    format!("{}/rexon/backups/plasma-panel", data_home())
}

/// Копии панели Plasma от новых к старым; порядок задаёт ядро.
///
/// Своего чтения мета-файла здесь больше нет: читает и разбирает его тот же
/// исполнитель, который его и пишет (`backup::plasma_meta_entries`). Два
/// читателя одного файла разъезжаются молча — список показывал бы одно, а
/// кнопка «Восстановить» брала другое.
fn plasma_backups() -> Vec<backup::PlasmaBackup> {
    let raw = backup::plasma_meta_entries(&plasma_backup_dir());
    backup::plasma_backups(&backup::plasma_parse_entries(&raw))
}

/// `$XDG_CONFIG_HOME`, если он задан: с нестандартным корнем конфигов копия
/// панели сохраняла бы и возвращала НЕ те файлы, которые читает Plasma.
fn xdg_config_home_env() -> Option<String> {
    std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty())
}

/// Метка времени копии — `strftime("%Y%m%d_%H%M%S")` местного времени.
/// Часы у продукта одни (`qt_app::Timestamp::now_local`), своих здесь нет.
fn plasma_stamp() -> String {
    qt_app::Timestamp::now_local()
        .unwrap_or_default()
        .file_stamp()
}

/// Снять копию панели Plasma — ОДИН вызов на обе вкладки («Настройки» и
/// «Рабочие столы», `desktops_actions.rs`), как один `PlasmaProtector` у
/// Python. Набор конфигов панели и раскрытие `~` — у ядра; снимок делает его
/// же исполнитель. Здесь — только каталог копий, метка времени и повод.
///
/// `Err` — отказ ДИСКА (каталог не создался, мета-файл не записался); у
/// Python это было исключение.
pub(crate) fn panel_backup_now(reason: &str) -> Result<(bool, String), String> {
    let configs = backup::expand_tilde_xdg(
        backup::PANEL_CONFIGS,
        &home(),
        xdg_config_home_env().as_deref(),
    );
    let outcome =
        backup::plasma_backup_run(&plasma_backup_dir(), &configs, reason, false, &plasma_stamp());
    if matches!(outcome, Ok((true, _))) {
        notify_backup_created();
    }
    outcome
}

/// Сказать BonziBuddy о новой копии панели — как Python
/// (`rexon_bridge.notify("notify_backup_created")` в `PlasmaProtector`).
///
/// Мост ядра тот же (`rexon_bridge::RexonBridge`): событие — файл в
/// `~/.config/rexon/bridge_commands`, второго механизма нет. Мост —
/// вспомогательная связь: его отказ не роняет копию, а только пишется в
/// журнал. Уборка событий старше 5 минут — тем же, кто сорит, как у Python.
fn notify_backup_created() {
    use rexon_core::rexon_bridge::RexonBridge;

    let bridge = RexonBridge::new_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let now_iso = qt_app::Timestamp::now_local()
        .unwrap_or_default()
        .isoformat();
    let sent = bridge
        .ensure_dirs()
        .and_then(|_| bridge.notify_backup_created(&now_iso, now.as_millis() as u64));
    if let Err(e) = sent {
        rexon_core::log_warn!("bridge", "мост BonziBuddy: backup_created не отправлено — {e}");
    }
    bridge.cleanup_commands(300.0, now.as_secs_f64());
}

/// Вернуть панель Plasma из копии (`None` — из последней) — общий вызов обеих
/// вкладок, как `PlasmaProtector.restore_panel`.
///
/// Перезапуск оболочки — единственный шаг, которым возврат копии трогает
/// ЖИВОЙ рабочий стол, и он делается только на самой Plasma: на другом столе
/// перезапускать нечего. Как именно перезапускать (юнит → kquitapp/kstart →
/// PID оболочки своего сеанса) решает и исполняет ядро одной функцией.
pub(crate) fn panel_restore_now(timestamp: Option<&str>) -> (bool, String) {
    let (ok, mut message) = backup::plasma_restore_run(&plasma_backup_dir(), timestamp);
    if ok && backup::is_plasma_desktop(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default())
    {
        let outcome = backup::plasma_restart_shell();
        message.push_str(&backup::plasma_restart_message(outcome));
    }
    (ok, message)
}

/// Установленные тяжёлые форки Plasma (AeroThemePlasma/AeroShell).
///
/// Только Arch — форки живут исключительно в AUR; отбор делает ядро.
fn plasma_forks() -> Vec<String> {
    if system::DistroDetector::detect_live().family_live() != system::PkgFamily::Arch {
        return Vec::new();
    }
    if !has("pacman") {
        return Vec::new();
    }
    let stdout = read_cmd(&["pacman".to_string(), "-Qq".to_string()]);
    let installed: Vec<String> = stdout.lines().map(|l| l.trim().to_string()).collect();
    system_plan::filter_plasma_forks(&installed)
}

/// Поставить слежку за обновлениями пакетов (пользовательские юниты
/// systemd, без пароля; `rexon_core::update_guard`) — в фоне: это
/// `systemctl --user`. Нужна всем на Arch: защита от обновления, которое
/// поставит набор пакетов вразнобой, а при стоящих форках Plasma — и сверка
/// AeroThemePlasma (свою настройку она проверяет сама). Неудача не мешает
/// работе окна — только запись в журнал.
fn sync_update_watch() {
    std::thread::spawn(|| {
        if let Err(e) = dx::update_watch_live() {
            rexon_core::log_warn!("settings", "слежка за обновлениями: {e}");
        }
    });
}

/// Идёт ли уже проверка «до обновления» (при запуске и по кнопке разом).
static UPDATE_GUARD_BUSY: AtomicBool = AtomicBool::new(false);

/// Идёт установка или удаление шрифта «для всех» (окно пароля, помощник):
/// второе нажатие до итога — отказ словами, а не вторая операция.
static SHARED_FONT_BUSY: AtomicBool = AtomicBool::new(false);

fn aeroshell_key() -> &'static str {
    aeroshell::CHECK_ON_START_SETTING
}

/// Сверять ли оформление AeroShell при запуске Rexon.
///
/// AeroShell (AeroThemePlasma) — оформление Plasma: его панель задач и
/// «Пуск» живут только в сеансе Plasma. Живая проверка в MATE: при запуске
/// всплывало окно «Оформление нужно пересобрать» про панель и «Пуск»,
/// которых в этом сеансе нет и не будет, — вопрос не к месту. Вне Plasma
/// при запуске молчим (запись в журнал), а пакеты сверятся при следующем
/// входе в Plasma: там же и спросим. Нажатие кнопки уведомления
/// (`from_notice`) — просьба человека, её исполняем в любом сеансе;
/// «Проверить сейчас» сюда не ходит вовсе.
fn startup_survey_wanted(enabled: bool, from_notice: bool, plasma: bool) -> bool {
    from_notice || (enabled && plasma)
}

/// Текущий системный шрифт цели, как его читает окружение: из родного
/// хранилища (`qt_settings::font_plan`) — команды или файла (COSMIC, LXQt).
/// Начертание остаётся в строке — разбирает её [`read_font_face`].
fn read_font(de: &str, target: &str) -> String {
    let raw = match qs::font_plan_wm(de, target, title_wm(de)).primary {
        // sway: наш блок в его конфиге, иначе строка `font` конфига или
        // шрифт sway по умолчанию.
        qs::FontStore::File(qs::FontFile::SwayConf) => qs::sway_title_font_spec_live(),
        qs::FontStore::Cmd(backend) => read_cmd(
            &qs::sound_read_argv(&backend, &kde_tool("kreadconfig6")).unwrap_or_default(),
        ),
        // rc.xml Openbox/labwc: без своего менеджер читает системный.
        qs::FontStore::File(file) => read_file(&font_file_path(&file))
            .or_else(|| matches!(file, qs::FontFile::RcXml { .. }).then(|| qs::FontFileIo::read_system(&ConfigFiles, &file.rel_path())).flatten())
            .map(|text| file.read_spec(&text))
            .unwrap_or_default(),
    };
    raw.trim().trim_matches(['\'', '"']).to_string()
}

/// [`read_font`] разобранным: семейство, кегль и начертание
/// (`qt_settings::parse_font_spec_face` — описание Pango gsettings/xfconf,
/// запись QFont KDE и Qt).
fn read_font_face(de: &str, target: &str) -> (String, i32, qs::FontFace) {
    qs::parse_font_spec_face(&read_font(de, target))
}

/// Полный путь файла-хранилища шрифта.
fn font_file_path(file: &qs::FontFile) -> String {
    format!("{}/{}", config_home().trim_end_matches('/'), file.rel_path())
}

/// Записать шрифт в одно хранилище → получилось ли.
fn write_font_store(store: &qs::FontStore, family: &str, size: i32, face: qs::FontFace) -> bool {
    match store {
        // sway: наш файл, блок `include` в конфиге с проверкой самим sway и
        // `swaymsg font …` — ядро, тем же механизмом, что акцент.
        qs::FontStore::File(qs::FontFile::SwayConf) => qs::sway_title_font_apply_live(family, size, face),
        qs::FontStore::Cmd(_) => {
            // Начертание — словами Pango в gsettings/xfconf, полями QFont в
            // kdeglobals (`font_store_write_argv_face`).
            match qs::font_store_write_argv_face(store, family, size, face, &kde_tool("kwriteconfig6")) {
                Some(argv) => write_cmd(&argv),
                None => false,
            }
        }
        qs::FontStore::File(file) => {
            let base = config_home();
            let Some(writes) = file.file_writes_face(&base, &ConfigFiles, family, size, face) else {
                return false;
            };
            writes.iter().all(|w| write_config_file(&base, w))
        }
    }
}

/// Файлы настроек под `$XDG_CONFIG_HOME` — для планов ядра
/// (`qt_settings::FontFileIo`).
struct ConfigFiles;

impl qs::FontFileIo for ConfigFiles {
    fn read(&self, rel_path: &str) -> Option<String> {
        read_file(&format!("{}/{rel_path}", config_home().trim_end_matches('/')))
    }

    fn read_system(&self, rel_path: &str) -> Option<String> {
        let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
        let found = dirs
            .split(':')
            .filter(|d| !d.is_empty())
            .chain(["/etc/xdg"])
            .find_map(|d| read_file(&format!("{}/{rel_path}", d.trim_end_matches('/'))));
        found
    }

    fn link_target(&self, rel_path: &str) -> Option<String> {
        let path = format!("{}/{rel_path}", config_home().trim_end_matches('/'));
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if !meta.file_type().is_symlink() {
            return None;
        }
        std::fs::canonicalize(&path).ok().map(|p| p.to_string_lossy().into_owned())
    }
}

/// Исполнить одну запись файла плана шрифта. Ссылка на чужую тему
/// снимается, а не правится насквозь (`FontFileWrite::replace_link`).
fn write_config_file(base: &str, w: &qs::FontFileWrite) -> bool {
    let path = format!("{}/{}", base.trim_end_matches('/'), w.rel_path);
    if w.replace_link && std::fs::remove_file(&path).is_err() {
        return false;
    }
    write_file_atomic(&path, &w.text).is_ok()
}

/// Ответ шима выбора шрифта → что применять. `None` — применять нечего
/// (человек закрыл окно, ничего не выбрав).
///
/// Разбор строки «Семейство,кегль» делает ядро тем же `parse_font_spec`,
/// которым читается настройка окружения. Здесь остаётся ОДНО решение:
/// неположительный кегль значит «шрифт задан пиксельным размером, точек в нём
/// нет» — тогда берётся тот же запасной кегль 10, что у Python-оригинала
/// (`font.pointSize() if font.pointSize() > 0 else 10`). Без него в конфиг
/// уехало бы «Noto Sans,-1», и окружение показало бы шрифт нулевого размера.
fn chosen_font(spec: &str) -> Option<(String, i32)> {
    let (family, size) = qs::parse_font_spec(spec);
    if family.is_empty() {
        return None;
    }
    Some((family, if size > 0 { size } else { 10 }))
}

/// Записать шрифт цели в текущее окружение → получилось ли.
///
/// Порт `_apply_font`, расширенный на все окружения: план целиком решает ядро
/// (`qt_settings::font_plan`) — родное хранилище окружения (KDE —
/// `kwriteconfig6`, XFCE — `xfconf`, GNOME/Cinnamon/MATE/Budgie — свои ключи
/// `gsettings`, COSMIC — `cosmic-config`, LXQt — `lxqt.conf`), спутники для
/// GTK- и Qt-программ и перенос в `gtk-{3,4}.0/settings.ini`. Текст ini
/// собирает `gtk_sanitizer::write_ini_setting_in` (тот же движок, которым
/// чинится оформление GTK, — второго правила о секции `[Settings]` в проекте
/// не появляется). Успех решает только родное хранилище.
fn apply_font(de: &str, target: &str, family: &str, size: i32, face: qs::FontFace) -> bool {
    let plan = qs::font_plan_wm(de, target, title_wm(de));
    // Спутники-команды — до основной записи: снятый «заголовки системным
    // шрифтом» должен успеть до того, как оконный менеджер перечитает шрифт.
    for argv in &plan.extra {
        let _ = write_cmd(argv);
    }
    let ok = write_font_store(&plan.primary, family, size, face);
    // Вспомогательные записи не делают применение неудачным: родной шрифт
    // окружения уже записан, и врать об этом нельзя.
    for store in &plan.also {
        let _ = write_font_store(store, family, size, face);
    }
    // Оконному менеджеру — перечитать настройки (Openbox, labwc).
    if ok {
        for argv in &plan.notify {
            let _ = write_cmd(argv);
        }
    }

    // Перенос в GTK — только у того шрифта, у которого там вообще есть свой
    // ключ (у моноширинного и заголовочного его нет).
    if plan.gtk_ini {
        // `gtk-font-name` — описание Pango, начертание в нём словами.
        let spec = qs::pango_font_desc(family, size, face);
        for path in qs::gtk_font_ini_paths(&config_home()) {
            let text = gtk_sanitizer::write_ini_setting_in(
                read_file(&path).as_deref(),
                "Settings",
                qs::GTK_FONT_KEY,
                &spec,
            );
            let _ = write_file_atomic(&path, &text);
        }
        remove_stray_gtk_font_dirs();
    }
    ok
}

/// Снять каталоги `gtk-gtk-{3,4}.0`, которые заводила прежняя ошибочная
/// запись шрифта, — только если в них ровно наш файл
/// (`qt_settings::stray_gtk_font_dir_removable`).
fn remove_stray_gtk_font_dirs() {
    for (dir, ini) in qs::stray_gtk_font_dirs(&config_home()) {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        let entries: Vec<String> =
            read.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        if qs::stray_gtk_font_dir_removable(&entries, read_file(&ini).as_deref()) {
            let _ = std::fs::remove_file(&ini);
            let _ = std::fs::remove_dir(&dir);
        }
    }
}

/// Применить звуковую настройку общим планом ядра
/// (`qt_settings::sound_plan`) — тем же, что у «Оформления»: родной ключ
/// окружения, общий слой для GTK- (`org.gnome.desktop.sound`) и
/// KDE/Qt-программ (`kdeglobals [Sounds]`), затем проверка чтением.
///
/// Успех — только если РОДНОЙ механизм отдал обратно записанное (`check`):
/// `kwriteconfig6` и `gsettings` умеют завершаться нулём, ничего не записав.
fn apply_sound_setting(setting: SoundSetting, value: &str, check: impl Fn(&str) -> bool) -> bool {
    let de = current_de();
    let plan = qs::sound_plan(
        &de,
        setting,
        value,
        &kde_tool("kwriteconfig6"),
        &kde_tool("kreadconfig6"),
        has("gsettings"),
    );
    let Some(primary) = plan.write.first() else {
        return false;
    };
    let primary_ok = write_cmd(primary);
    for argv in plan.write.iter().skip(1) {
        let _ = write_cmd(argv);
    }
    match plan.verify {
        Some(verify) => check(&read_cmd(verify.as_slice())),
        // Читать нечем — остаётся код возврата записи.
        None => primary_ok,
    }
}

/// Включить или выключить звуки событий запущенного окружения — итог
/// строкой для человека. Одна дверь для выключателя «Настроек» и полосы
/// «Системные звуки выключены» во вкладке «Оформление».
pub(crate) fn switch_event_sounds(enabled: bool) -> String {
    let ok = apply_sound_setting(
        SoundSetting::EventSounds,
        qs::event_sounds_value(enabled),
        |raw| qs::parse_event_sounds(raw) == enabled,
    );
    qs::event_sounds_status(ok, enabled)
}

/// Сырой ответ выключателя звуков событий окружения `de`; пусто —
/// прочитать нечем.
pub(crate) fn event_sounds_raw(de: &str) -> String {
    read_cmd(
        &qs::sound_read_argv_for(de, SoundSetting::EventSounds, &kde_tool("kreadconfig6"))
            .unwrap_or_default(),
    )
}

/// Правила защиты приложений по умолчанию с диска.
fn protection_rules() -> defaults::Rules {
    let loaded = read_json(&format!("{}/rexon/protection_rules.json", config_home()));
    match loaded {
        Value::Object(map) => defaults::Rules::defaults().updated(&map),
        _ => defaults::Rules::defaults(),
    }
}

/// Запомненные приложения по умолчанию с диска.
fn protected_apps() -> Vec<(String, Option<String>)> {
    defaults::load_protected_apps(&read_json(&format!(
        "{}/rexon/protected_defaults.json",
        config_home()
    )))
}

/// Что стоит в системе ПРЯМО СЕЙЧАС: `[(тип, .desktop | None), …]` в том же
/// порядке, что у Python (MIME_TYPES, затем терминал).
fn current_defaults() -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for (app_type, mime) in defaults::MIME_TYPES {
        let stdout = read_cmd(&[
            "xdg-mime".to_string(),
            "query".to_string(),
            "default".to_string(),
            (*mime).to_string(),
        ]);
        let value = stdout.lines().next().unwrap_or("").trim().to_string();
        out.push((
            (*app_type).to_string(),
            if value.is_empty() { None } else { Some(value) },
        ));
    }
    out.push(("terminal".to_string(), current_terminal()));
    out
}

/// Текущий терминал по умолчанию.
///
/// Ленивость источников повторена за оригиналом дословно: непустая первая
/// строка `xdg-terminals.list` конклюзивна, и ни `kdeglobals`, ни `gsettings`
/// после неё не читаются. Решение принимает ядро.
fn current_terminal() -> Option<String> {
    let cfg = config_home();
    let xdg_list = read_file(&format!("{cfg}/xdg-terminals.list"));
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

    if let Some(content) = &xdg_list {
        if content.lines().next().is_some_and(|l| !l.trim().is_empty()) {
            return defaults::get_current_terminal(Some(content), &desktop, None, None);
        }
    }

    let de = desktop.to_lowercase();
    let mut kdeglobals = None;
    let mut gsettings_exec = None;
    if de.contains("kde") || de.contains("plasma") {
        kdeglobals = read_file(&format!("{cfg}/kdeglobals"));
    } else if let Some(schema) = defaults::terminal_exec_schema(&desktop) {
        gsettings_exec = gsettings_get(schema, "exec");
    }

    defaults::get_current_terminal(
        xdg_list.as_deref(),
        &desktop,
        kdeglobals.as_deref(),
        gsettings_exec.as_deref(),
    )
    .or_else(installed_terminal)
}

/// `gsettings get <schema> <key>`; `None` — команда не отработала.
fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    if !has("gsettings") {
        return None;
    }
    let out = read_cmd(&[
        "gsettings".to_string(),
        "get".to_string(),
        schema.to_string(),
        key.to_string(),
    ]);
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Терминал, который просто установлен в системе, — последний источник, когда
/// настройки молчат (типичная свежая система с тайловым WM). Выбор делает
/// ядро; здесь только наблюдения «бинарь в PATH» и «.desktop на диске».
fn installed_terminal() -> Option<String> {
    let mut available: Vec<String> = Vec::new();
    let mut desktops: Vec<String> = Vec::new();
    for (binary, desktop_ids) in defaults::TERMINAL_CANDIDATES {
        if !has(binary) {
            continue;
        }
        available.push((*binary).to_string());
        for id in *desktop_ids {
            if desktop_exists(id) {
                desktops.push((*id).to_string());
            }
        }
    }
    defaults::pick_terminal(&available, &desktops)
}

// ============================================================================
// Действия
// ============================================================================

impl qobject::SettingsActions {
    // ── Язык интерфейса ──────────────────────────────────────────────────

    fn set_language(&self, code: &QString) -> QString {
        let code = code.to_string();
        // Незнакомое значение не пишется: мусор в настройке молча дал бы
        // английский.
        if !qs::language_choice_valid(&code) {
            return QString::default();
        }
        if let Err(text) = save_setting(rexon_core::i18n::SETTINGS_KEY, Value::from(code.as_str())) {
            rexon_core::log_warn!("lang", "выбор «{code}» не сохранён: {text}");
            return QString::from(&text);
        }
        rexon_core::log_info!(
            "lang",
            "выбран язык «{code}» (даст {}, сейчас {}; перезапуск {})",
            rexon_core::i18n::language_for_setting(Some(&code)),
            rexon_core::i18n::current_language(),
            if rexon_core::i18n::restart_needed(&code) { "нужен" } else { "не нужен" }
        );
        QString::from(&qs::language_saved_status(&code))
    }

    fn admin_session(&self) -> bool {
        // `REXON_SHOT_ADMIN_SESSION=1` — только для снимков вкладки на
        // отдельном экране: полоса видна без живого помощника. Права это не
        // даёт — «Запретить сейчас» тогда ничего не закрывает.
        rexon_core::privilege::run::has_session()
            || std::env::var_os("REXON_SHOT_ADMIN_SESSION").is_some_and(|v| v == "1")
    }

    fn revoke_admin(&self) -> QString {
        rexon_core::privilege::run::shutdown();
        rexon_core::log_info!("privilege", "«Запретить сейчас» в «Настройках»: сеанс прав закрыт");
        QString::from(&rexon_core::privilege::revoke::Reason::RevokedByUser.message())
    }

    fn restart_rexon(&self) -> QString {
        match spawn_restart() {
            Ok(()) => {
                crate::tray::request_quit("перезапуск из «Настроек»");
                QString::default()
            }
            Err(reason) => {
                rexon_core::log_error!("restart", "отменён, Rexon остаётся открытым: {reason}");
                QString::from(&qs::language_restart_failed(&reason))
            }
        }
    }

    // ── Скин, обучение, автозапуск ───────────────────────────────────────

    fn set_skin(&self, skin_id: &QString) -> QString {
        let skin_id = skin_id.to_string();
        // Незнакомое имя не пишется вовсе: сохранённый мусор откатывался бы к
        // стандартному скину при каждом чтении, и настройка выглядела бы
        // «не запоминающейся».
        if skin_id.is_empty() || !ui_base::is_valid_skin(&skin_id) {
            return QString::default();
        }
        let saved = core_settings::SettingsManager::new()
            .and_then(|mut m| m.set("ui_skin", Value::from(skin_id.as_str())));
        if let Err(e) = saved {
            return QString::from(&t1("status.error", &e.to_string()));
        }
        // Толчок ПОСЛЕ записи: страницы читают действующий скин из настроек,
        // и рассылка до записи перекрасила бы их в прежний цвет.
        crate::skin::dispatch();
        let label = ui_base::available_skins()
            .into_iter()
            .find(|(id, _)| *id == skin_id)
            .map(|(_, label)| label.to_string())
            .unwrap_or_else(|| skin_id.clone());
        QString::from(&t1("skin.applied", &label))
    }

    fn set_onboarding(&self, enabled: bool) -> QString {
        match save_setting(onboarding::KEY_ENABLED, Value::from(enabled)) {
            Ok(()) => QString::from(&qs::onboarding_toggle_status(enabled)),
            Err(text) => QString::from(&text),
        }
    }

    fn replay_onboarding(mut self: Pin<&mut Self>) -> QString {
        // Обучение живёт накладкой главного окна; просьбу ей передаёт общий
        // реестр. Нет окна с накладкой (вкладка открыта одиночным окном) —
        // человеку говорится, куда идти.
        if !crate::onboarding_overlay::request_replay() {
            return QString::from(&t("ob.no_main_window"));
        }
        // Повтор включает обучение в настройках (`onboarding::session_replay`),
        // но делает это накладка — сигналом из очереди, уже ПОСЛЕ того, как
        // страница перечитала снимок по ответу этой кнопки. Галочка «Показывать
        // обучение» осталась бы прежней. Толчок в ту же очередь встаёт за
        // просьбой к накладке и перечитывает снимок после её записи.
        let queued = self
            .as_mut()
            .qt_thread()
            .queue(|mut object| object.as_mut().refreshed());
        if let Err(e) = queued {
            if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                rexon_core::log_warn!("settings", "перечитать настройки после повтора обучения не вышло: {e:?}");
            }
        }
        QString::from(&t("ob.started"))
    }

    // ── «Установить Rexon в систему…» ────────────────────────────────────
    //
    // ⭐ Решение Wehrwolfmann 15.09.2026: кнопка стоит рядом с «Пройти
    // обучение заново». Признак «показывать ли», подписи и сама установка —
    // в ядре; здесь только дверь к мастеру (`first_run_wizard`), как у
    // «Пройти обучение заново» дверь к накладке обучения.

    fn install_offered(&self) -> bool {
        crate::first_run_wizard::offer_ignoring_decline().is_offer()
    }

    fn install_texts(&self) -> QMap<QMapPair_QString_QVariant> {
        let lang = ui_lang();
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        let mut put = |key: &str, value: String| {
            out.insert(QString::from(key), QVariant::from(&QString::from(&value)));
        };
        put("label", inst::settings_install_button(lang));
        put("hint", inst::settings_install_hint(lang));
        out
    }

    fn install_rexon(&self) -> QString {
        let lang = ui_lang();
        // Кнопка и есть ответ «всё-таки установить»: прежний отказ снимается
        // тем же ключом настроек, который читает решение ядра.
        if let Err(text) = save_setting(inst::KEY_FIRST_RUN_DECLINED, Value::from(false)) {
            return QString::from(&text);
        }
        // Мастер живёт в главном окне; прямой ссылки у страницы на него нет.
        if !crate::first_run_wizard::request_show() {
            return QString::from(&inst::settings_install_no_window(lang));
        }
        QString::from(&inst::settings_install_started(lang))
    }

    fn set_autostart(&self, enabled: bool) -> QString {
        match crate::tray::set_autostart(enabled) {
            Ok(()) => QString::from(&t(if enabled {
                "tray.autostart_on"
            } else {
                "tray.autostart_off"
            })),
            // ⭐ Починка: причина называется человеку, а не только журналу.
            Err(e) => QString::from(&format!(
                "{} {}",
                t("tray.autostart_failed"),
                t1("status.error", &e)
            )),
        }
    }

    fn set_theme_deps_auto(&self, enabled: bool) -> QString {
        match save_setting(rexon_core::theme_watch::AUTO_HOME_KEY, Value::from(enabled)) {
            Ok(()) => QString::from(&t(if enabled { "tray.theme_deps_on" } else { "tray.theme_deps_off" })),
            Err(text) => QString::from(&text),
        }
    }

    // ── Файловый менеджер ────────────────────────────────────────────────

    fn set_file_manager(&self, fm_id: &QString) -> QString {
        match save_setting("file_manager", Value::from(fm_id.to_string())) {
            // Пусто — сказать нечего: выбор только записан, применяет его
            // отдельная кнопка.
            Ok(()) => QString::default(),
            Err(text) => QString::from(&text),
        }
    }

    fn apply_file_manager(&self) -> QString {
        let de = current_de();
        let setting = core_settings::SettingsManager::new()
            .ok()
            .and_then(|m| m.get("file_manager").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| "auto".to_string());
        let fm_id = resolve_file_manager(&de, &setting);
        QString::from(&qs::fm_apply_status(set_file_manager_default(&fm_id)))
    }

    fn restore_native(&self) -> QString {
        let de = current_de();
        let de_display = screenshots::display_name(&de);
        let apps = appdefaults::de_apps_info(&de, &desktop_exists);

        let mut protected = protected_apps();
        let mut restored = 0usize;
        let mut missing: Vec<String> = Vec::new();

        for (app_type, desktop) in &apps {
            let Some(desktop) = desktop else {
                missing.push(defaults::app_type_label(app_type).to_string());
                continue;
            };
            if !apply_default(app_type, desktop) {
                missing.push(defaults::app_type_label(app_type).to_string());
                continue;
            }
            remember(&mut protected, app_type, desktop);
            restored += 1;
            if app_type == "file_manager" {
                // mimeapps мало: «показать в папке» идёт через D-Bus
                // FileManager1 → нужен user-override.
                set_file_manager_default(desktop);
            }
        }
        // Итог записи снимка здесь не показывается: человеку важен
        // отчёт о ВОССТАНОВЛЕНИИ, а не о памяти под него, и вторая строка
        // об ошибке перекрыла бы первую.
        let _ = save_protected(&protected);

        let message = qs::restore_native_message(restored, &missing, &de_display);
        QString::from(&qs::restore_native_status(&message, !missing.is_empty()))
    }

    // ── Комплектность рабочего стола ─────────────────────────────────────
    //
    // Порт `checkCompleteness` Python-вкладки. Исполнитель — ядро
    // (`system_de_exec::completeness_survey_live` / `complete_de_live`), тот
    // же, что ставит рабочие столы; окно — общее `DeOperationDialog.qml`.
    // Порядок: проверка без пароля → нехватка словами в окне вопроса → после
    // «Да» доустановка с ходом дела → итог в разделе и всплывающей строкой.
    // Всё на месте — окна нет вовсе, только итог (как у Python).

    fn check_completeness(mut self: Pin<&mut Self>) -> QString {
        let flag = self.as_ref().get_ref().rust().completeness_busy.clone();
        if flag.swap(true, Ordering::SeqCst) {
            return QString::from(&t("compl.busy"));
        }
        self.as_ref().get_ref().rust().session().completeness_note = Some(t("compl.running"));
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // Паника проверки не должна оставить кнопку погашенной навсегда.
            let survey = std::panic::catch_unwind(dx::completeness_survey_live).ok();
            deliver(&handle, move |object| object.survey_done(survey));
        });
        // Пустой ответ: страница перечитает снимок и покажет «Проверяю…».
        QString::default()
    }

    fn survey_done(mut self: Pin<&mut Self>, survey: Option<dx::CompletenessSurvey>) {
        let flag = self.as_ref().get_ref().rust().completeness_busy.clone();
        let Some(survey) = survey else {
            let msg = t1("compl.failed_log", &completeness_internal_error());
            self.as_ref().get_ref().rust().session().completeness_note = Some(msg.clone());
            flag.store(false, Ordering::SeqCst);
            self.as_mut().refreshed();
            self.as_mut().status(QString::from(&msg));
            return;
        };
        for line in &survey.logs {
            rexon_core::log_info!("completeness", "{line}");
        }
        let msg = qs::compliance_summary(&survey.results);
        self.as_ref().get_ref().rust().session().completeness_note = Some(msg.clone());
        if survey.fixes.is_empty() {
            flag.store(false, Ordering::SeqCst);
            self.as_mut().refreshed();
            self.as_mut().status(QString::from(&qs::compliance_status(
                &msg,
                !survey.results.is_empty(),
            )));
            return;
        }
        // Нехватка: кнопка остаётся погашенной, пока открыт вопрос.
        let labels: Vec<String> = survey.fixes.iter().map(|(_, label)| label.clone()).collect();
        self.as_mut().rust_mut().pending = Some(PendingOp::Completeness(survey.fixes));
        self.as_mut().set_op(OpView::confirm(
            &dx::completeness_confirm_title(),
            &dx::completeness_confirm_text(&labels),
        ));
        self.as_mut().refreshed();
        self.as_mut().status(QString::from(&dx::awaiting_answer(
            &dx::completeness_operation_title(),
        )));
    }

    fn operation(&self) -> QMap<QMapPair_QString_QVariant> {
        self.rust().op.to_map()
    }

    fn set_op(mut self: Pin<&mut Self>, op: OpView) {
        self.as_mut().rust_mut().op = op;
        self.as_mut().operation_changed();
    }

    fn confirm_operation(mut self: Pin<&mut Self>) {
        if self.rust().op.phase == "running" {
            return;
        }
        let Some(pending) = self.as_mut().rust_mut().pending.take() else {
            return;
        };
        let token = dx::CancelToken::new();
        self.as_mut().rust_mut().cancel = Some(token.clone());
        let handle = self.as_mut().qt_thread();
        match pending {
            PendingOp::WaylandDefault(session, names) => {
                use rexon_core::system_plan as sp;
                self.as_mut().set_op(OpView::running(&sp::wayland_default_button(session)));
                std::thread::spawn(move || {
                    let done = std::panic::catch_unwind(|| sp::make_wayland_default_live(session))
                        .unwrap_or_else(|_| Err(sp::wayland_default_failed()));
                    deliver(&handle, move |object| object.finish_wayland_default(session, names, done));
                });
            }
            PendingOp::Completeness(fixes) => {
                self.as_mut()
                    .set_op(OpView::running(&dx::completeness_operation_title()));
                std::thread::spawn(move || {
                    let sink: Arc<dyn dx::DeOpSink> = Arc::new(SettingsSink {
                        handle: handle.clone(),
                    });
                    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        dx::complete_de_live(sink, token, &fixes)
                    }))
                    .ok();
                    deliver(&handle, move |object| object.finish_completeness(done));
                });
            }
            PendingOp::RestoreStock(forks) => {
                self.as_mut().set_op(OpView::running(&aeroshell::restore_run_title()));
                std::thread::spawn(move || {
                    let sink: Arc<dyn dx::DeOpSink> = Arc::new(SettingsSink {
                        handle: handle.clone(),
                    });
                    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        dx::restore_stock_plasma_live(sink, token, &forks)
                    }))
                    .ok();
                    deliver(&handle, move |object| object.finish_restore_stock(done));
                });
            }
            PendingOp::RollbackAsk(offer) => {
                // Защита от дурака: перед откатом — что именно поменяется.
                use rexon_core::update_guard as ug;
                self.as_mut().rust_mut().cancel = None;
                let view = OpView {
                    yes_text: ug::rollback_confirm_button(),
                    no_text: dx::dialog_cancel(),
                    danger: true,
                    ..OpView::confirm(
                        &ug::rollback_confirm_title(&offer),
                        &rexon_core::ui_base::escape_dialog_text(&ug::rollback_confirm_text(&offer)),
                    )
                };
                self.as_mut().rust_mut().pending = Some(PendingOp::Rollback(offer));
                self.as_mut().set_op(view);
            }
            PendingOp::Rollback(offer) => {
                use rexon_core::update_guard as ug;
                self.as_mut().set_op(OpView::running(&ug::rollback_title(&offer)));
                std::thread::spawn(move || {
                    let lines = handle.clone();
                    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let back = ug::rollback_live(&offer, &mut |l: &str| {
                            let l = l.to_string();
                            deliver(&lines, move |object| {
                                object.apply_op_progress(dp::STAGE_RAW.to_string(), l, None)
                            });
                        });
                        // Повторная сверка — после отката: итог словами, что
                        // осталось — окном, когда итог закроют.
                        let rest = back.is_ok().then(ug::advise_live);
                        (back, rest)
                    }))
                    .ok();
                    deliver(&handle, move |object| object.finish_rollback(offer, done));
                });
            }
            PendingOp::RemoveOrphans(names) => {
                use rexon_core::update_guard as ug;
                self.as_mut().set_op(OpView::running(&ug::orphan_remove_title(&names)));
                std::thread::spawn(move || {
                    let lines = handle.clone();
                    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let removed = ug::remove_orphans_live(&names, &mut |l: &str| {
                            let l = l.to_string();
                            deliver(&lines, move |object| {
                                object.apply_op_progress(dp::STAGE_RAW.to_string(), l, None)
                            });
                        });
                        // Повторная сверка — только после удаления: что
                        // осталось, покажется, когда окно итога закроют.
                        let rest = removed.is_ok().then(ug::advise_live);
                        (removed, rest)
                    }))
                    .ok();
                    deliver(&handle, move |object| object.finish_remove_orphans(names, done));
                });
            }
            PendingOp::Aeroshell(packages) => {
                self.as_mut().set_op(OpView::running(&dp::aeroshell_run_title()));
                std::thread::spawn(move || {
                    let sink: Arc<dyn dx::DeOpSink> = Arc::new(SettingsSink {
                        handle: handle.clone(),
                    });
                    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        dx::rebuild_aeroshell_live(sink, token, &packages)
                    }))
                    .ok();
                    deliver(&handle, move |object| object.finish_aeroshell(done));
                });
            }
        }
    }

    fn finish_completeness(mut self: Pin<&mut Self>, done: Option<dx::CompletenessOutcome>) {
        let panicked = done.is_none();
        let done = done.unwrap_or_else(|| dx::CompletenessOutcome {
            outcome: dx::DeOpOutcome {
                ok: false,
                message: t1("compl.failed_log", &completeness_internal_error()),
                cancelled: false,
                restart_needed: false,
            },
            results: Vec::new(),
            logs: Vec::new(),
        });
        for line in &done.logs {
            rexon_core::log_info!("completeness", "{line}");
        }
        let mut op = self.rust().op.clone();
        if let Some(line) = op.apply_done(&done.outcome, String::new()) {
            self.as_mut().operation_log(QString::from(&line));
        }
        if panicked {
            // Ошибку по ходу дела никто не объявил — объявляем здесь.
            self.as_mut().operation_log(QString::from(&format!(
                "{}{}",
                rexon_core::de_progress::PREFIX_ERROR,
                done.outcome.message
            )));
        }
        let note = completeness_note_after(&done.outcome, &done.results);
        let footer = dx::operation_footer(&dx::completeness_operation_title(), &op.status);
        self.as_mut().rust_mut().cancel = None;
        self.as_ref().get_ref().rust().session().completeness_note = Some(note);
        self.as_ref()
            .get_ref()
            .rust()
            .completeness_busy
            .store(false, Ordering::SeqCst);
        self.as_mut().set_op(op);
        self.as_mut().refreshed();
        self.as_mut().status(QString::from(&footer));
    }

    fn apply_op_progress(
        mut self: Pin<&mut Self>,
        stage: String,
        message: String,
        fraction: Option<f64>,
    ) {
        let mut op = self.rust().op.clone();
        let Some(log_line) = op.apply_progress(&stage, &message, fraction) else {
            return;
        };
        if let Some(line) = log_line {
            self.as_mut().operation_log(QString::from(&line));
        }
        self.set_op(op);
    }

    fn apply_op_cancellable(self: Pin<&mut Self>, allowed: bool) {
        let mut op = self.rust().op.clone();
        if op.phase != "running" {
            return;
        }
        op.cancellable = allowed;
        self.set_op(op);
    }

    fn dismiss_operation(mut self: Pin<&mut Self>) {
        let phase = self.rust().op.phase.clone();
        if phase == "running" {
            return;
        }
        if phase == "confirm" {
            match self.as_mut().rust_mut().pending.take() {
                // «Не сейчас»: экран входа не меняется; удалить сироту окно
                // снова предложит по кнопке или при следующей сверке.
                Some(PendingOp::WaylandDefault(..)) => {}
                // «Нет»: доустановку не начинаем, кнопка снова доступна, в
                // разделе остаётся итог проверки («осталось доустановить»).
                Some(PendingOp::Completeness(_)) => {
                    self.as_ref()
                        .get_ref()
                        .rust()
                        .completeness_busy
                        .store(false, Ordering::SeqCst);
                    self.as_mut().refreshed();
                    self.as_mut().status(QString::default());
                }
                // «Не сейчас»: пересборка сама не начнётся никогда, спросим
                // снова при следующем запуске или по кнопке.
                Some(PendingOp::Aeroshell(_) | PendingOp::RestoreStock(_)) => {
                    AEROSHELL_BUSY.store(false, Ordering::SeqCst);
                    self.as_mut().status(QString::default());
                }
                // «Не сейчас»: снова — при следующем запуске, если пакет
                // ещё на месте и состояние сменилось, либо по кнопке.
                // То же с откатом: ни на «Не сейчас», ни на «Отмена» ничего
                // не меняется.
                Some(PendingOp::RemoveOrphans(_) | PendingOp::RollbackAsk(_) | PendingOp::Rollback(_)) | None => {}
            }
        }
        self.as_mut().set_op(OpView::default());
        // Закрыт итог удаления сироты, а сверка нашла ещё что-то — сказать.
        if let Some(rest) = self.as_mut().rust_mut().update_guard_rest.take() {
            self.as_mut().show_update_guard(rest);
        }
    }

    fn cancel_operation(mut self: Pin<&mut Self>) {
        if self.rust().op.phase != "running" || !self.rust().op.cancellable {
            return;
        }
        if let Some(token) = &self.rust().cancel {
            token.cancel();
        }
        // Кнопка гаснет сразу; исполнитель остановится на безопасной точке.
        let mut op = self.rust().op.clone();
        op.cancellable = false;
        self.as_mut().set_op(op);
    }

    // ── Обои ─────────────────────────────────────────────────────────────

    fn stop_wallpaper(&self) -> QString {
        let state = self.rust();
        // Идёт применение — исполнитель занят фоновым потоком. Ждать его
        // замок в потоке окна значило бы заморозить вкладку на время команд
        // оболочки; честнее сказать, что обои уже применяются.
        if state.wallpaper_busy.load(Ordering::SeqCst) {
            return QString::from(&t("wp.busy"));
        }
        // Что снять, что вернуть и в каком порядке на KDE, GNOME, Xfce и
        // прочих — решает исполнитель ядра (`WallpaperExec::stop`), здесь
        // только вызов и готовая строка ответа.
        //
        // ⚠️ Python следом звал `cleanup_orphans` — подбор процессов обоев
        // ПРОШЛОГО запуска по PID-файлу и по имени. Сюда он не перенесён:
        // PID-файл общий у всех окон Rexon, и остановка в этом окне снимала
        // бы обои, поставленные классическим окном. Кнопка же включается
        // только при СВОИХ живых обоях этого окна (`running`) — с любой его
        // страницы: исполнитель один на процесс, и видео из «Steam» здесь
        // тоже снимается. Исполнитель занят применением с другой страницы —
        // ждать его замок в потоке окна нельзя, ответ тот же «уже применяю».
        match stop_wallpaper_in(&live_slot()) {
            None => QString::from(&t("wp.busy")),
            Some(res) => QString::from(&qs::wallpaper_stop_status(res.ok, &res.message)),
        }
    }

    fn set_wallpaper_audio(
        &self,
        enabled: bool,
        volume: i32,
        mute_other: bool,
        hide_covered: bool,
    ) -> QString {
        // Что и какими ключами записать — решает ядро; обои не
        // перезапускаются, хост следит за файлом настроек сам.
        let patch = rexon_core::wallpaper_audio::AudioSettings {
            enabled,
            volume_pct: rexon_core::wallpaper_audio::clamp_volume_pct(volume as i64),
            mute_when_other_audio: mute_other,
            fade_ms: rexon_core::wallpaper_audio::DEFAULT_FADE_MS,
            actions: rexon_core::wallpaper_audio::actions_for_hide_when_covered(hide_covered),
        }
        .to_map();

        let Ok(mut manager) = core_settings::SettingsManager::new() else {
            return QString::from(&t1("status.error", &rexon_core::i18n::tr("settings are unavailable")));
        };
        for (key, value) in patch {
            // ⭐ Починка: Python рапортовал об успехе, не глядя на запись.
            if let Err(e) = manager.set(&key, value) {
                return QString::from(&t1("status.error", &e.to_string()));
            }
        }
        QString::from(&qs::wallpaper_audio_live_status(enabled))
    }

    fn pick_wallpaper(mut self: Pin<&mut Self>, kind: &QString) -> QString {
        let kind = kind.to_string();
        // Все форматы обоев исполнитель ядра носит сам (`carries_format`):
        // картинка, видео и Flash применяются без классического окна.
        if self.as_ref().get_ref().rust().wallpaper_busy.load(Ordering::SeqCst) {
            return QString::from(&t("wp.busy"));
        }
        let path = crate::extensions_actions::qobject::pick_file(
            &QString::from(&qs::wallpaper_dialog_title(&kind)),
            &QString::from(&qs::wallpaper_dialog_filter(&kind)),
            &QString::from(&home()),
        )
        .to_string();
        if path.is_empty() {
            // Человек передумал — сказать нечего.
            return QString::default();
        }

        // Формат — по расширению ВЫБРАННОГО файла, а не по нажатой кнопке:
        // в окне выбора есть и другие группы, и .mp4 из-под «Картинка…»
        // применился бы картинкой.
        let fmt = qs::detect_wallpaper_format(&path);
        let basename = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        self.as_mut().start_wallpaper_apply(path, fmt.to_string());
        // ⭐ Починка: Python подставлял служебное слово формата
        // («Применяю: закат.png (static)…»); покупателю — имя из ядра.
        QString::from(&qs::wallpaper_applying_status(
            &basename,
            &wallpaper_engine::format_label(fmt),
        ))
    }

    fn apply_youtube(
        mut self: Pin<&mut Self>,
        url: &QString,
        quality: &QString,
        mode: &QString,
        audio: bool,
    ) -> QString {
        let url = url.to_string().trim().to_string();
        if url.is_empty() {
            return QString::from(&t("yt.no_url"));
        }
        if !has("yt-dlp") {
            return QString::from(&t("yt.no_ytdlp"));
        }
        if self.as_ref().get_ref().rust().wallpaper_busy.load(Ordering::SeqCst) {
            return QString::from(&t("wp.busy"));
        }
        let quality = quality.to_string();
        let mode = mode.to_string();
        // Какой режим получится на самом деле (на GNOME поток становится
        // скачиванием) — решает ядро по окружению исполнителя.
        let effective = try_with_slot(&live_slot(), |exec| {
            wallpaper_engine::youtube_effective_mode(exec.de(), exec.session_type(), &mode)
                .to_string()
        })
        .unwrap_or_else(|| mode.clone());
        let (job_url, job_quality, job_mode) = (url, quality.clone(), mode);
        // Поток, скачивание и постановку делает исполнитель ядра; надзор за
        // нагрузкой поднимет общая фоновая дверь.
        self.as_mut().run_wallpaper_job(move |exec| {
            let res = exec.apply_youtube(&job_url, &job_quality, &job_mode, audio);
            qs::youtube_done(res.ok, &res.message)
        });
        QString::from(&qs::youtube_starting_status(&quality, &effective, audio))
    }

    fn set_youtube_audio(&self, on: bool) -> QString {
        self.rust().session().youtube_audio = Some(on);
        QString::from(&qs::youtube_audio_status(on))
    }

    fn ytdlp(mut self: Pin<&mut Self>, action: &QString) -> QString {
        let action = action.to_string();
        let (_button, status) = qs::ytdlp_working(&action);
        let flag = self.as_ref().get_ref().rust().ytdlp_busy.clone();
        // Уже идёт — отвечаем тем же ходом, а не молчанием (Python молчал:
        // человек жал ещё раз и вводил пароль ради повторной установки).
        if flag.swap(true, Ordering::SeqCst) {
            return QString::from(&status);
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // Флажок гаснет и при панике работы.
            let busy = BusyGuard(flag);
            // Установка — пакетным менеджером дистрибутива (пароль спрашивает
            // polkit), затем pip; обновление — самим yt-dlp, затем pip.
            // Команды и порядок решает ядро.
            let (ok, message) = if action == "update" {
                youtube_wallpaper::update_ytdlp_live()
            } else {
                youtube_wallpaper::install_ytdlp_live()
            };
            drop(busy);
            let done = qs::ytdlp_done(ok, &message);
            let queued = handle.queue(move |mut object| {
                // Спросить систему заново: после установки кнопка обязана
                // стать «Обновить», а надпись — показать версию.
                object.as_ref().get_ref().rust().session().ytdlp_version = None;
                object.as_mut().refreshed();
                object.as_mut().status(QString::from(&done));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("settings", "итог установки yt-dlp не доставлен: {e:?}");
                }
            }
        });
        QString::from(&status)
    }

    fn apply_web(mut self: Pin<&mut Self>, url: &QString) -> QString {
        let url = url.to_string().trim().to_string();
        if url.is_empty() {
            return QString::from(&t("web.no_url"));
        }
        if self.as_ref().get_ref().rust().wallpaper_busy.load(Ordering::SeqCst) {
            return QString::from(&t("wp.busy"));
        }
        // Локальный путь превращается в `file://` — решает ядро (как у Python).
        let exists = url.starts_with('/') && std::path::Path::new(&url).exists();
        let target = qs::web_wallpaper_url(&url, exists);
        let job_target = target.clone();
        // Хост веб-обоев поднимает исполнитель ядра (`apply` → `web_plan`).
        self.as_mut().run_wallpaper_job(move |exec| {
            let res = exec.apply(&job_target, "web", "");
            qs::web_done(res.ok, &res.message)
        });
        QString::from(&qs::web_loading_status(&target))
    }

    // ── Библиотека обоев ─────────────────────────────────────────────────

    fn preview_library(&self, path: &QString) -> QString {
        let path = path.to_string();
        if !path.is_empty() && std::path::Path::new(&path).is_file() {
            // Тот же шим, которым «Расширения» открывают каталог: одна дверь
            // в `QDesktopServices` на весь крейт.
            crate::extensions_actions::qobject::open_folder_url(&QString::from(&path));
        }
        QString::default()
    }

    fn apply_library(mut self: Pin<&mut Self>, path: &QString, fmt: &QString) -> QString {
        let path = path.to_string();
        let fmt = fmt.to_string();
        if path.is_empty() || !std::path::Path::new(&path).exists() {
            return QString::from(&t("lib.no_file"));
        }
        if self.as_ref().get_ref().rust().wallpaper_busy.load(Ordering::SeqCst) {
            return QString::from(&t("wp.busy"));
        }
        // Плитка любого формата идёт в исполнитель ядра: картинки, видео и
        // Flash (под надзором за нагрузкой), GIF и веб применяются; сцены,
        // обои-приложения и незнакомый формат пакета исполнитель отклоняет
        // теми же словами, что классическое окно.
        let basename = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.as_mut().start_wallpaper_apply(path, fmt.clone());
        // ⭐ Починка: Python подставлял служебное слово формата («(static)»).
        QString::from(&qs::library_applying_status(
            &basename,
            &wallpaper_engine::format_label(&fmt),
        ))
    }

    // ── Импорт тем Windows ───────────────────────────────────────────────

    fn pick_import(mut self: Pin<&mut Self>) -> QString {
        if self.as_ref().get_ref().rust().import_busy.load(Ordering::SeqCst) {
            return QString::from(&t("imp.busy"));
        }
        let path = crate::extensions_actions::qobject::pick_file(
            &QString::from(&t("imp.dialog_title")),
            &QString::from(&importers::import_dialog_filter()),
            &QString::from(&home()),
        )
        .to_string();
        if path.is_empty() {
            // Человек передумал — сказать нечего.
            return QString::default();
        }

        let basename = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        {
            let state = self.as_ref().get_ref().rust();
            state.session().import_note = t1("imp.working_file", &basename);
            state.import_busy.store(true, Ordering::SeqCst);
        }

        let flag = self.as_ref().get_ref().rust().import_busy.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // Распаковка CAB, конвертация картинок и копирование файлов темы
            // — секунды и минуты. Работу делает тот же исполнитель, которым
            // ставит .rxn вкладка «Расширения»: второй копии правил импорта
            // не появляется.
            let missing = !std::path::Path::new(&path).exists();
            let outcome = ext_exec::install_from_file(&path);
            flag.store(false, Ordering::SeqCst);

            let (note, status) = match outcome {
                Ok(done) => qs::import_success_texts(&done.name, &done.notes),
                Err(text) => {
                    let reason = qs::import_failure_reason(missing, &text);
                    qs::import_failure_texts(&reason)
                }
            };
            let queued = handle.queue(move |mut object| {
                {
                    let rust = object.as_ref().get_ref().rust();
                    let mut session = rust.session();
                    session.import_note = note;
                    // Импорт мог поставить пакет обоев — библиотеку перечитать.
                    session.library = None;
                }
                object.as_mut().refreshed();
                object.as_mut().status(QString::from(&status));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    // Молчать нельзя: итог импорта пропал бы, и раздел остался
                    // бы со строкой «Импортируем тему…» навсегда.
                    rexon_core::log_warn!("settings", "итог импорта темы не доставлен: {e:?}");
                }
            }
        });
        QString::from(&t("imp.working_status"))
    }

    // ── Размытие обоев ───────────────────────────────────────────────────

    fn set_blur_radius(&self, value: i32) -> QString {
        self.rust().session().blur_radius = Some(value);
        QString::default()
    }

    fn blur(mut self: Pin<&mut Self>, action: &QString) -> QString {
        let blurring = action.to_string() == "blur";
        if self.as_ref().get_ref().rust().wallpaper_busy.load(Ordering::SeqCst) {
            return QString::from(&t("blur.busy"));
        }
        let radius = self
            .as_ref()
            .get_ref()
            .rust()
            .session()
            .blur_radius
            .unwrap_or(qs::BLUR_RANGE.3);
        // Размытие картинки и постановку копии делает исполнитель ядра (без
        // Pillow — размытие своё); слова итога — ядро.
        self.as_mut().run_wallpaper_job(move |exec| {
            let ok = if blurring {
                exec.blur_current_wallpaper(radius as i64)
            } else {
                exec.unblur_wallpaper()
            };
            qs::blur_done(ok, &qs::blur_result(if blurring { "blur" } else { "unblur" }, ok))
        });
        QString::from(&if blurring {
            t1("blur.applying", &radius.to_string())
        } else {
            t("blur.removing")
        })
    }

    // ── Фоновые звуки ────────────────────────────────────────────────────

    fn add_ambient(&self, volume: i32) -> QString {
        let path = crate::extensions_actions::qobject::pick_file(
            &QString::from(&t("amb.dialog_title")),
            &QString::from(&qs::ambient_file_filter()),
            &QString::from(&home()),
        )
        .to_string();
        if path.is_empty() {
            // Человек передумал — сказать нечего.
            return QString::default();
        }
        let basename = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let volume = wallpaper_audio::clamp_volume_pct(volume as i64);
        // Запуск и проверка «зазвучал ли» — исполнитель ядра (та же отсрочка,
        // что у Python, доли секунды): здесь только вызов и строка ответа.
        let started = try_with_slot(&live_slot(), |exec| {
            exec.play_ambient(&qs::ambient_sound_id(&path), &path, &basename, volume, true)
        });
        match started {
            None => QString::from(&t("wp.busy")),
            Some(true) => QString::from(&qs::ambient_start_status(true, &basename, volume as i32)),
            // ⭐ Починка: Python на любой отказ писал «нужен mpv» — и при
            // установленном mpv, когда просто не играет сам файл.
            Some(false) => QString::from(&qs::ambient_start_failed_status(has("mpv"))),
        }
    }

    fn set_ambient_volume(&self, sound_id: &QString, volume: i32) -> QString {
        let id = sound_id.to_string();
        let volume = wallpaper_audio::clamp_volume_pct(volume as i64);
        // Громкость у mpv меняется только перезапуском — это делает
        // исполнитель ядра, в том же режиме, в каком звук играл. Имя берётся
        // ДО перезапуска: неудачный перезапуск снимает звук из списка.
        let outcome = try_with_slot(&live_slot(), |exec| {
            let name = exec
                .active_ambient()
                .into_iter()
                .find(|sound| sound.id == id)
                .map(|sound| sound.name);
            let ok = name.is_some() && exec.set_ambient_volume(&id, volume);
            (name, ok)
        });
        match outcome {
            None => QString::from(&t("wp.busy")),
            Some((name, ok)) => QString::from(&qs::ambient_volume_status(
                name.as_deref(),
                volume,
                ok,
                has("mpv"),
            )),
        }
    }

    fn stop_ambient(&self, sound_id: &QString) -> QString {
        let id = sound_id.to_string();
        // Снимает звук исполнитель ядра (только свой процесс, по дескриптору).
        // Имя берётся ДО остановки: после неё звука в списке уже нет.
        let name = try_with_slot(&live_slot(), |exec| {
            let name = exec
                .active_ambient()
                .into_iter()
                .find(|sound| sound.id == id)
                .map(|sound| sound.name);
            exec.stop_ambient(&id);
            name
        });
        match name {
            None => QString::from(&t("wp.busy")),
            Some(name) => QString::from(&qs::ambient_stopped_status(name.as_deref())),
        }
    }

    fn stop_all_ambient(&self) -> QString {
        // Снимает все свои звуки исполнитель ядра — те же процессы, что он
        // сам поднял; чужие mpv в системе не трогаются.
        match try_with_slot(&live_slot(), |exec| exec.stop_all_ambient()) {
            None => QString::from(&t("wp.busy")),
            Some(()) => QString::from(&t("amb.stopped_all")),
        }
    }

    // ── Звуки событий ────────────────────────────────────────────────────

    fn set_sound_theme(&self, theme: &QString) -> QString {
        let theme = theme.to_string();
        if theme.is_empty() {
            return QString::default();
        }
        // Имя, под которым на диске нет темы, не пишем: см.
        // `qt_settings::sound_theme_exists`.
        if !qs::sound_theme_exists(&qs::live_xdg_sound_theme_dirs(), &theme) {
            return QString::from(&qs::sound_theme_missing_message(&theme));
        }
        let ok = apply_sound_setting(SoundSetting::Theme, &theme, |raw| {
            qs::parse_sound_theme(raw) == theme
        });
        QString::from(&qs::sound_theme_status(ok, &theme))
    }

    fn set_event_sounds(&self, enabled: bool) -> QString {
        QString::from(&switch_event_sounds(enabled))
    }

    /// Прослушать звук; играть нечем — сказать об этом.
    ///
    /// Кнопка при отсутствии проигрывателя обязана говорить, а не молчать:
    /// иначе человеку не отличить «тихий звук» от «нечем играть».
    fn play_sound(&self, theme: &QString, event_id: &QString) -> QString {
        let theme = theme.to_string();
        // Тема не задана — играем ту, что играет система, и её же называем в
        // ответе: пустое имя означало бы «любой звук из любой темы».
        let theme = qs::effective_sound_theme(&theme).to_string();
        let event_id = event_id.to_string();
        let dirs = qs::sound_theme_dirs(&data_home());
        let file = qs::find_sound_file(&dirs, &theme, &event_id);

        // Какой проигрыватель годится для ЭТОГО файла — решает ядро: прежний
        // хвост «paplay → aplay» обманывал на темах freedesktop (.oga).
        let available: Vec<String> = utils::SOUND_PLAYERS
            .iter()
            .filter(|(binary, _, _)| has(binary))
            .map(|(binary, _, _)| (*binary).to_string())
            .collect();

        if let Some(path) = &file {
            if let Some(argv) = utils::sound_play_argv(&available, path) {
                if spawn_detached(&argv) {
                    return QString::default();
                }
            }
        }
        // Файла в теме нет (или играть его нечем) — событие libcanberra.
        if let Some(argv) = utils::sound_play_event_argv(&available, &event_id) {
            if spawn_detached(&argv) {
                return QString::default();
            }
        }
        let problem = match &file {
            None => qs::msg_no_event_in_theme(&theme),
            Some(path) => utils::msg_no_sound_player(&utils::tool_package_hint(
                utils::sound_player_suggestion(path),
                &pkg_family(),
            )),
        };
        QString::from(&t1("snd.cannot_play", &problem))
    }

    // ── MATE ─────────────────────────────────────────────────────────────

    fn mate_action(&self, action: &QString) -> QString {
        let action = action.to_string();
        // Команды MATE и проверка их итога — исполнитель ядра; слова ответа —
        // ядро. Незнакомое действие отвечает внятно, а не молчит.
        let reply = try_with_slot(&live_slot(), |exec| match action.as_str() {
            "restart" => qs::caja_restart_result(exec.mate_restart_caja_desktop()),
            "on" | "off" => {
                let visible = action == "on";
                qs::mate_icons_result(visible, exec.mate_set_desktop_icons(visible))
            }
            _ => t("mate.no_api"),
        });
        QString::from(&reply.unwrap_or_else(|| t("wp.busy")))
    }

    // ── Системные шрифты ─────────────────────────────────────────────────

    fn pick_font(&self, target: &QString) -> QString {
        let target = target.to_string();
        let (_label, _tip, title) = qs::font_target_texts(&target);
        let de = current_de();
        // Шрифта заголовков, который в этом окружении некуда записать, не
        // пишем никуда: без этой заслонки план увёл бы его в чужой ключ.
        if !qs::font_target_available_wm(&de, &target, title_wm(&de)) {
            return QString::from(&qs::font_apply_status(false, &title, "", 0));
        }
        // Deepin: окно выбора не открываем — говорим, где менять (ядро).
        if let Some(why) = qs::font_refusal_live(&de) {
            return QString::from(&format!("{title} — {why}"));
        }
        // Что показать выбранным при открытии — нынешний шрифт окружения
        // вместе с начертанием.
        let (current_family, current_size, current_face) = read_font_face(&de, &target);

        let mut outcome = 0i32;
        let chosen = crate::extensions_actions::qobject::pick_font(
            &QString::from(&title),
            &QString::from(&current_family),
            current_size,
            current_face.weight,
            current_face.italic,
            &mut outcome,
        )
        .to_string();

        if outcome < 0 {
            return QString::from(&format!("{title} — {}", font_dialog_failed()));
        }
        // Человек передумал — сказать нечего (как у Python-оригинала).
        let Some((family, size)) = chosen_font(&chosen) else {
            return QString::default();
        };

        // Жирность и наклон из окна выбора уходят во все хранилища, где
        // они есть (`apply_font`).
        let face = qs::parse_font_face(&chosen);
        let applied = apply_font(&de, &target, &family, size, face);
        // Снимок шрифтов устарел ровно здесь — иначе список показывал бы
        // старое значение до закрытия вкладки.
        self.rust().session().fonts = None;
        // В итоге — семейство с начертанием («Sans Bold»), кегль отдельно.
        let shown = qs::font_face_spec(&family, 0, face);
        QString::from(&qs::font_apply_status(applied, &title, &shown, size))
    }

    fn set_font_rendering(
        &self,
        antialias: bool,
        exclude: bool,
        exclude_from: i32,
        exclude_to: i32,
        subpixel: &QString,
        hinting: &QString,
    ) -> QString {
        let r = qs::FontRendering {
            antialias,
            exclude: exclude.then_some((exclude_from, exclude_to)),
            subpixel: subpixel.to_string(),
            hinting: hinting.to_string(),
        }
        .normalized();
        let outcome = apply_rendering(&current_de(), &r);
        self.rust().session().rendering = None;
        QString::from(&qs::rendering_status(&outcome))
    }

    fn change_all_fonts(&self, mode: &QString) -> QString {
        let mode = mode.to_string();
        let de = current_de();
        let (_label, _tip, title) = qs::change_all_texts();
        if let Some(why) = qs::font_refusal_live(&de) {
            return QString::from(&format!("{title} — {why}"));
        }
        let (current_family, current_size, current_face) = read_font_face(&de, "system");

        let mut outcome = 0i32;
        let chosen = crate::extensions_actions::qobject::pick_font(
            &QString::from(&title),
            &QString::from(&current_family),
            current_size,
            current_face.weight,
            current_face.italic,
            &mut outcome,
        )
        .to_string();
        if outcome < 0 {
            return QString::from(&format!("{title} — {}", font_dialog_failed()));
        }
        let Some((family, size)) = chosen_font(&chosen) else {
            return QString::default();
        };

        // Начертание у каждого шрифта остаётся своим: «изменить все» меняет
        // семейство и кегль, а жирный заголовок остаётся жирным.
        let mut faces = std::collections::HashMap::new();
        let now: Vec<qs::FontNow> = qs::font_targets_for_wm(&de, title_wm(&de))
            .into_iter()
            .map(|target| {
                let (family, size, face) = read_font_face(&de, target);
                faces.insert(target.to_string(), face);
                qs::FontNow { target: target.to_string(), family, size }
            })
            .collect();
        let is_mono = qs::parse_fc_spacing(&read_cmd(&qs::fc_spacing_argv(&family)));
        let (plan, mono_kept) = qs::change_all_fonts(&now, &family, size, &mode, is_mono);
        let mut failed = Vec::new();
        for font in &plan {
            let face = faces.get(&font.target).copied().unwrap_or_default();
            if !apply_font(&de, &font.target, &font.family, font.size, face) {
                failed.push(qs::font_target_texts(&font.target).2);
            }
        }
        self.rust().session().fonts = None;
        let kept = now.iter().find(|n| n.target == "mono").map(|n| n.family.clone());
        QString::from(&qs::change_all_status(
            &failed,
            &family,
            size,
            &mode,
            if mono_kept { kept.as_deref() } else { None },
        ))
    }

    fn set_font_dpi(&self, enabled: bool, dpi: i32) -> QString {
        let (lo, hi) = qs::FONT_DPI_RANGE;
        let value = enabled.then(|| dpi.clamp(lo, hi));
        let ok = apply_font_dpi(&current_de(), value);
        self.rust().session().font_dpi = None;
        QString::from(&qs::font_dpi_status(ok, value))
    }

    // ── Управление шрифтами ──────────────────────────────────────────────

    fn font_library(&self, full: bool, filter: &QString) -> QString {
        // Свёрнутому разделу нужны только подписи — fc-list не спрашивается.
        let fonts = if full {
            self.rust().session().font_library.get_or_insert_with(read_font_library).clone()
        } else {
            Vec::new()
        };
        let groups = qs::group_font_families(&fonts);
        let user_families = groups.iter().filter(|g| g.has_user()).count();
        let mut texts = Map::new();
        for (key, text) in qs::font_manager_texts() {
            texts.insert(key.to_string(), Value::from(text));
        }
        let filter = qs::FontListFilter::from_json(&filter.to_string());
        // Файл групп, который не разбирается, не мешает списку: групп просто
        // не видно, а правка честно отказывает (`fontGroupEdit`).
        let custom = read_font_groups();
        let groups_broken = custom.is_err();
        let custom = custom.unwrap_or_default();
        let families: Vec<Value> = qs::font_group_view_in(&groups, &filter, &custom)
            .iter()
            .map(|g| {
                let styles: Vec<Value> = g
                    .styles
                    .iter()
                    .map(|f| {
                        json!({ "style": f.style, "file": f.file, "user": f.user, "shared": f.shared, "disabled": f.disabled })
                    })
                    .collect();
                json!({
                    "family": g.family,
                    "user": g.has_user(),
                    "shared": g.has_shared(),
                    "packaged": g.has_packaged(),
                    "system": g.has_system(),
                    "disabled": g.all_disabled(),
                    "partly": g.any_disabled() && !g.all_disabled(),
                    "groups": qs::font_groups_of_family(&custom, &g.family),
                    "styles": styles,
                })
            })
            .collect();
        let origins: Vec<Value> = qs::font_origin_choices_with_groups(&custom)
            .into_iter()
            .map(|(id, label, own)| json!({ "id": id, "label": label, "custom": own }))
            .collect();
        let group_list: Vec<Value> = qs::font_origin_choices_with_groups(&custom)
            .into_iter()
            .filter(|(_, _, own)| *own)
            .map(|(_, name, _)| Value::from(name))
            .collect();
        let choices = qs::font_filter_choices(&fonts);
        let pairs = |v: &[(String, String)]| -> Vec<Value> {
            v.iter().map(|(id, label)| json!({ "id": id, "label": label })).collect()
        };
        let (min, max, start) = qs::FONT_SAMPLE_SIZE;
        let system = rexon_core::system::DistroDetector::detect_live();
        let shared_ok = qs::font_shared_supported(system.family_live(), system.is_steamos());
        let scopes: Vec<Value> = qs::font_install_scope_choices(shared_ok)
            .into_iter()
            .map(|(id, label)| json!({ "id": id, "label": label }))
            .collect();
        let json = json!({
            "available": has("fc-list"),
            "scopes": scopes,
            "texts": texts,
            "summary": qs::font_manager_summary(groups.len(), user_families),
            "origins": origins,
            "groups": group_list,
            "groupsBroken": groups_broken,
            "foundries": pairs(&choices.foundries),
            "fileTypes": pairs(&choices.file_types),
            "scripts": pairs(&choices.scripts),
            "sample": {
                "min": min,
                "max": max,
                "start": start,
                "text": font_sample_text(),
                "default": qs::font_sample_default(),
                "maxLength": qs::FONT_SAMPLE_TEXT_MAX,
            },
            "families": families,
        });
        QString::from(&json.to_string())
    }

    fn font_chars(&self, file: &QString, style: &QString) -> QString {
        let out = read_cmd(&qs::fc_query_charset_argv(&file.to_string()));
        let blocks: Vec<Value> = qs::font_char_blocks(&qs::fc_charset_for_style(&out, &style.to_string()))
            .into_iter()
            .map(|b| {
                json!({
                    // У «Других символов» начала нет; пустым id не быть:
                    // пустой — это «блок ещё не выбран» у страницы.
                    "id": b.start.map(|s| format!("{s:04X}")).unwrap_or_else(|| "other".to_string()),
                    "label": format!("{} ({})", b.name, b.chars.len()),
                    "chars": b.chars,
                })
            })
            .collect();
        QString::from(&json!({ "blocks": blocks }).to_string())
    }

    fn set_font_sample_text(&self, text: &QString) -> QString {
        let stored = qs::font_sample_text_to_store(&text.to_string());
        // Не записалось — образец всё равно показывается тем, что ввели:
        // человек видит свой текст, а ошибка записи не повод его отнимать.
        let _ = save_setting(qs::FONT_SAMPLE_TEXT_KEY, Value::from(stored.as_str()));
        QString::from(&qs::font_sample_text(Some(&stored)))
    }

    fn set_font_enabled(&self, family: &QString, file: &QString, enabled: bool) -> QString {
        let family = family.to_string();
        let file = file.to_string();
        let fonts = self
            .rust()
            .session()
            .font_library
            .get_or_insert_with(read_font_library)
            .clone();
        let files = qs::font_toggle_files(&fonts, &family, &file);
        if files.is_empty() {
            self.rust().session().font_library = None;
            return QString::from(&qs::font_toggle_status(&qs::FontToggleOutcome::NotFound(family)));
        }
        if let Some(bad) = files.iter().find(|f| !qs::font_glob_safe(f)) {
            let name = std::path::Path::new(bad)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| bad.clone());
            return QString::from(&qs::font_toggle_status(&qs::FontToggleOutcome::UnsafeName(name)));
        }
        let path = fontconfig_path();
        let existing = read_file(&path);
        let current = existing.as_deref().map(qs::fontconfig_disabled_files).unwrap_or_default();
        let after = qs::font_disabled_after(&current, &files, enabled);
        if !enabled {
            let de = current_de();
            let in_use: Vec<String> = qs::font_targets_for_wm(&de, title_wm(&de))
                .into_iter()
                .map(|target| read_font_face(&de, target).0)
                .collect();
            if let Some(refusal) = qs::font_disable_refusal(&fonts, &after, &in_use) {
                return QString::from(&qs::font_toggle_status(&refusal));
            }
            let mut families: Vec<String> =
                fonts.iter().filter(|f| files.contains(&f.file)).map(|f| f.family.clone()).collect();
            families.sort();
            families.dedup();
            if let Some((title, question)) = qs::font_disable_confirm(&family, &families) {
                if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&question)) {
                    return QString::default();
                }
            }
        }
        let ok = match write_disabled_fonts(&path, existing.as_deref(), &after) {
            Ok(ok) => ok,
            Err(_) => return QString::from(&qs::font_toggle_status(&qs::FontToggleOutcome::Refused(path))),
        };
        self.rust().session().font_library = None;
        let outcome = match (ok, enabled) {
            (false, _) => qs::FontToggleOutcome::Failed(path),
            (true, true) => qs::FontToggleOutcome::Enabled(family),
            (true, false) => qs::FontToggleOutcome::Disabled(family),
        };
        QString::from(&qs::font_toggle_status(&outcome))
    }

    fn font_group_edit(&self, op: &QString, group: &QString, arg: &QString) -> QString {
        let (op, group, arg) = (op.to_string(), group.to_string(), arg.to_string());
        let path = font_groups_path();
        let reply = |outcome: &qs::FontGroupOutcome, origin: String| {
            QString::from(&json!({ "text": qs::font_group_status(outcome), "origin": origin }).to_string())
        };
        // Перечитать перед правкой: группы могли поменять в KDE.
        let Ok(groups) = read_font_groups() else {
            return reply(&qs::FontGroupOutcome::Refused(path), String::new());
        };
        let edit = match op.as_str() {
            "create" => qs::FontGroupEdit::Create(arg),
            "rename" => qs::FontGroupEdit::Rename { from: group, to: arg },
            "remove" => qs::FontGroupEdit::Remove(group),
            "add" => qs::FontGroupEdit::Add { group, families: vec![arg] },
            "take" => qs::FontGroupEdit::Take { group, families: vec![arg] },
            _ => return QString::default(),
        };
        if let qs::FontGroupEdit::Remove(name) = &edit {
            if groups.iter().any(|g| &g.name == name) {
                let (title, question) = qs::font_group_remove_confirm(name);
                if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&question)) {
                    return QString::default();
                }
            }
        }
        let (next, outcome) = qs::font_groups_apply(&groups, &edit);
        if outcome.changed() && write_file_atomic(&path, &qs::font_groups_xml(&next)).is_err() {
            return reply(&qs::FontGroupOutcome::Failed(path), String::new());
        }
        // Что выбрать в фильтре: новую или переименованную группу; после
        // удаления — «Все». Иначе страница оставляет прежний выбор.
        let origin = match &outcome {
            qs::FontGroupOutcome::Created(name) | qs::FontGroupOutcome::Renamed { to: name, .. } => {
                format!("{}{name}", qs::FONT_GROUP_ORIGIN_PREFIX)
            }
            qs::FontGroupOutcome::Removed(_) | qs::FontGroupOutcome::NotFound(_) => "all".to_string(),
            _ => String::new(),
        };
        reply(&outcome, origin)
    }

    fn set_font_group_enabled(&self, group: &QString, enabled: bool) -> QString {
        let name = group.to_string();
        let groups = match read_font_groups() {
            Ok(g) => g,
            Err(_) => return QString::from(&qs::font_group_status(&qs::FontGroupOutcome::Refused(font_groups_path()))),
        };
        let Some(group) = groups.into_iter().find(|g| g.name == name) else {
            return QString::from(&qs::font_group_status(&qs::FontGroupOutcome::NotFound(name)));
        };
        let fonts = self
            .rust()
            .session()
            .font_library
            .get_or_insert_with(read_font_library)
            .clone();
        let files = qs::font_group_files(&fonts, &group);
        if files.is_empty() {
            return QString::from(&qs::font_group_empty_status(&name));
        }
        if let Some(bad) = files.iter().find(|f| !qs::font_glob_safe(f)) {
            let file_name = std::path::Path::new(bad)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| bad.clone());
            return QString::from(&qs::font_toggle_status(&qs::FontToggleOutcome::UnsafeName(file_name)));
        }
        let path = fontconfig_path();
        let existing = read_file(&path);
        let current = existing.as_deref().map(qs::fontconfig_disabled_files).unwrap_or_default();
        let after = qs::font_disabled_after(&current, &files, enabled);
        if !enabled {
            // Те же запреты, что у одного семейства: шрифт системы и
            // «выключить вообще всё».
            let de = current_de();
            let in_use: Vec<String> = qs::font_targets_for_wm(&de, title_wm(&de))
                .into_iter()
                .map(|target| read_font_face(&de, target).0)
                .collect();
            if let Some(refusal) = qs::font_disable_refusal(&fonts, &after, &in_use) {
                return QString::from(&qs::font_toggle_status(&refusal));
            }
            let mut others: Vec<String> = fonts
                .iter()
                .filter(|f| files.contains(&f.file) && !group.families.contains(&f.family))
                .map(|f| f.family.clone())
                .collect();
            others.sort();
            others.dedup();
            if let Some((title, question)) = qs::font_group_disable_confirm(&name, &others) {
                if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&question)) {
                    return QString::default();
                }
            }
        }
        let ok = match write_disabled_fonts(&path, existing.as_deref(), &after) {
            Ok(ok) => ok,
            Err(_) => return QString::from(&qs::font_toggle_status(&qs::FontToggleOutcome::Refused(path))),
        };
        self.rust().session().font_library = None;
        if !ok {
            return QString::from(&qs::font_toggle_status(&qs::FontToggleOutcome::Failed(path)));
        }
        let touched = fonts
            .iter()
            .filter(|f| group.families.contains(&f.family))
            .map(|f| f.family.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        QString::from(&qs::font_group_toggle_status(&name, touched, enabled))
    }

    fn export_font_group(&self, group: &QString) -> QString {
        let name = group.to_string();
        let groups = match read_font_groups() {
            Ok(g) => g,
            Err(_) => return QString::from(&qs::font_group_status(&qs::FontGroupOutcome::Refused(font_groups_path()))),
        };
        let Some(group) = groups.into_iter().find(|g| g.name == name) else {
            return QString::from(&qs::font_group_status(&qs::FontGroupOutcome::NotFound(name)));
        };
        let fonts = self
            .rust()
            .session()
            .font_library
            .get_or_insert_with(read_font_library)
            .clone();
        let entries =
            qs::font_group_export_entries(&qs::font_group_files(&fonts, &group), &|p| std::path::Path::new(p).is_file());
        if entries.is_empty() {
            return QString::from(&qs::font_group_empty_status(&name));
        }
        let (title, filter) = qs::font_group_export_dialog();
        let suggested = format!("{}/{}", home().trim_end_matches('/'), qs::font_group_export_name(&name));
        let chosen = crate::extensions_actions::qobject::save_file(
            &QString::from(&title),
            &QString::from(&filter),
            &QString::from(&suggested),
        )
        .to_string();
        if chosen.is_empty() {
            return QString::default();
        }
        let dest = qs::font_group_export_path(&chosen);
        let result = qs::write_font_group_zip(&entries, std::path::Path::new(&dest)).map_err(|e| e.to_string());
        QString::from(&qs::font_group_export_status(&name, &dest, &result))
    }

    fn font_duplicates(&self) -> QString {
        let fonts = qs::parse_fc_list_fonts(&read_cmd(&qs::fc_list_scalable_argv()), &font_user_dirs_checked());
        let real = |file: &str| {
            std::fs::canonicalize(file)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| file.to_string())
        };
        let found = qs::find_font_duplicates(&fonts, &real);
        let groups: Vec<Value> = found
            .iter()
            .map(|d| {
                let copies: Vec<Value> =
                    d.copies.iter().map(|c| json!({ "file": c.file, "user": c.user, "shared": c.shared })).collect();
                json!({ "family": d.family, "style": d.style, "copies": copies })
            })
            .collect();
        let json = json!({
            "summary": qs::font_duplicates_summary(found.len()),
            "groups": groups,
        });
        QString::from(&json.to_string())
    }

    fn install_font_file(mut self: Pin<&mut Self>, scope: &QString) -> QString {
        let for_all = qs::font_install_scope_is_all(&scope.to_string());
        if for_all && SHARED_FONT_BUSY.load(Ordering::SeqCst) {
            return QString::from(&rexon_core::privilege::client::ClientError::Busy.human_text());
        }
        let path = crate::extensions_actions::qobject::pick_file(
            &QString::from(&qs::font_install_dialog_title()),
            &QString::from(&qs::font_install_filter()),
            &QString::from(&home()),
        )
        .to_string();
        if path.is_empty() {
            return QString::default();
        }
        if !for_all {
            let outcome = install_font(&path);
            self.rust().session().font_library = None;
            return QString::from(&qs::font_install_status(&outcome));
        }
        if SHARED_FONT_BUSY.swap(true, Ordering::SeqCst) {
            return QString::from(&rexon_core::privilege::client::ClientError::Busy.human_text());
        }
        // Окно пароля и помощник — не в потоке окна: иначе оно «не
        // отвечает», пока человек вводит пароль.
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(|| rexon_core::local_install::install_font_file_shared(&path))
                .unwrap_or_else(|_| qs::FontInstallOutcome::CopyFailed(path.clone()));
            let text = qs::font_install_status(&outcome);
            deliver(&handle, move |object| object.shared_font_done(text, Vec::new()));
        });
        QString::default()
    }

    fn remove_font(mut self: Pin<&mut Self>, family: &QString, file: &QString) -> QString {
        let family = family.to_string();
        let file = file.to_string();
        let fonts = self
            .rust()
            .session()
            .font_library
            .get_or_insert_with(read_font_library)
            .clone();
        let dirs = font_user_dirs_checked();
        // Удаляются свои файлы — если путь и вправду ведёт в каталог шрифтов
        // пользователя (проверка заново, по настоящему пути: снимок мог
        // устареть), — и поставленные для всех (их проверяет помощник от
        // root). Шрифты пакетов — нет.
        let chosen = |f: &&qs::InstalledFont| f.family == family && (file.is_empty() || f.file == file);
        let mut files: Vec<String> = fonts
            .iter()
            .filter(|f| f.user && chosen(f))
            .map(|f| f.file.clone())
            .filter(|f| qs::font_file_is_user(&real_parent_path(f), &dirs))
            .collect();
        let mut shared: Vec<String> = fonts
            .iter()
            .filter(|f| f.shared && chosen(f))
            .map(|f| f.file.clone())
            .filter(|f| !files.contains(f))
            .collect();
        files.sort();
        files.dedup();
        shared.sort();
        shared.dedup();
        if files.is_empty() && shared.is_empty() {
            return QString::from(&qs::font_remove_status(&family, 0, 0));
        }
        if !shared.is_empty() && SHARED_FONT_BUSY.load(Ordering::SeqCst) {
            return QString::from(&rexon_core::privilege::client::ClientError::Busy.human_text());
        }
        let mut families: Vec<String> = fonts
            .iter()
            .filter(|f| files.contains(&f.file) || shared.contains(&f.file))
            .map(|f| f.family.clone())
            .collect();
        families.sort();
        families.dedup();
        let (title, mut question) = qs::font_remove_confirm(&family, files.len() + shared.len(), &families);
        if !shared.is_empty() {
            question.push(' ');
            question.push_str(&qs::font_remove_shared_note());
        }
        if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&question)) {
            return QString::default();
        }
        if !shared.is_empty() {
            if SHARED_FONT_BUSY.swap(true, Ordering::SeqCst) {
                return QString::from(&rexon_core::privilege::client::ClientError::Busy.human_text());
            }
            let handle = self.as_mut().qt_thread();
            let family = family.clone();
            let shared = shared.clone();
            std::thread::spawn(move || {
                let done = std::panic::catch_unwind(|| rexon_core::local_install::remove_shared_fonts(&shared));
                let text = match done {
                    Ok(Ok(())) => qs::font_remove_status(&family, shared.len(), 0),
                    Ok(Err(why)) => format!("⚠️ {why}"),
                    Err(_) => qs::font_remove_status(&family, 0, shared.len()),
                };
                deliver(&handle, move |object| object.shared_font_done(text, shared));
            });
            if files.is_empty() {
                return QString::default();
            }
        }
        let mut removed = 0;
        let mut failed = 0;
        let mut touched_dirs: Vec<String> = Vec::new();
        for f in &files {
            match std::fs::remove_file(f) {
                Ok(()) => {
                    removed += 1;
                    if let Some(dir) = std::path::Path::new(f).parent() {
                        let dir = dir.to_string_lossy().into_owned();
                        if !touched_dirs.contains(&dir) {
                            touched_dirs.push(dir);
                        }
                    }
                }
                Err(_) => failed += 1,
            }
        }
        for dir in &touched_dirs {
            let _ = write_cmd(&qs::fc_cache_argv(dir));
        }
        forget_removed_disabled(&files);
        self.rust().session().font_library = None;
        QString::from(&qs::font_remove_status(&family, removed, failed))
    }

    /// Итог установки или удаления шрифта «для всех» (из фонового потока):
    /// `removed` — какие файлы удалялись (их снять и из выключенных).
    fn shared_font_done(mut self: Pin<&mut Self>, text: String, removed: Vec<String>) {
        SHARED_FONT_BUSY.store(false, Ordering::SeqCst);
        forget_removed_disabled(&removed);
        self.rust().session().font_library = None;
        self.as_mut().refreshed();
        self.as_mut().status(QString::from(&text));
    }

    // ── Защита приложений по умолчанию ───────────────────────────────────

    fn set_protection(&self, key: &QString, enabled: bool) -> QString {
        let key = key.to_string();
        let mut rules = protection_rules().0;
        match rules.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = enabled,
            None => rules.push((key, enabled)),
        }
        if let Err(e) = write_file_atomic(
            &format!("{}/rexon/protection_rules.json", config_home()),
            &defaults::Rules(rules).serialize(),
        ) {
            return QString::from(&t1("status.error", &e.to_string()));
        }
        QString::default()
    }

    fn snapshot(&self) -> QString {
        let updated =
            defaults::compute_snapshot(&current_defaults(), &protection_rules(), &protected_apps());
        match save_protected(&updated) {
            Ok(()) => QString::from(&t("prot.snapshot_ok")),
            Err(e) => QString::from(&t1("prot.snapshot_error", &e)),
        }
    }

    fn restore_defaults(&self) -> QString {
        let protected = protected_apps();
        let outcome = defaults::restore_defaults(
            &protected,
            &protection_rules(),
            &current_defaults(),
            false,
            &[],
            &desktop_exists,
        );
        for (app_type, desktop) in &outcome.set_calls {
            apply_default(app_type, desktop);
        }
        // Отложенный выбор «оставить новое или вернуть старое» записывается
        // тем же файлом, что читает виджетная вкладка: разъехаться этим двум
        // нельзя, решение о них принимает ядро.
        if let Err(e) = write_file_atomic(
            &format!("{}/rexon/pending_choices.json", config_home()),
            &defaults::serialize_pending(&outcome.pending),
        ) {
            return QString::from(&t1("status.error", &e.to_string()));
        }
        QString::from(&t("prot.restore_ok"))
    }

    // ── Копии панели Plasma ──────────────────────────────────────────────

    fn plasma_backup(&self) -> QString {
        // «manual» — человек нажал кнопку сам.
        match panel_backup_now("manual") {
            Ok((ok, message)) => QString::from(&qs::plasma_status(ok, &message)),
            // То, что у Python было исключением: каталог не создался, мета-файл
            // не записался. У такой беды свой текст — общий итог «⚠️ …» назвал
            // бы её отказом снимка, а это отказ ДИСКА.
            Err(detail) => QString::from(&t1("plasma.backup_error", &detail)),
        }
    }

    fn plasma_confirm(&self, timestamp: &QString) -> QString {
        let (title, question) = qs::plasma_restore_confirm(&timestamp.to_string());
        QString::from(&json!({ "title": title, "question": question }).to_string())
    }

    fn plasma_restore(&self, timestamp: &QString) -> QString {
        let timestamp = timestamp.to_string();
        let (ok, message) =
            panel_restore_now(Some(timestamp.as_str()).filter(|ts| !ts.is_empty()));
        QString::from(&qs::plasma_status(ok, &message))
    }

    // ── Оформление рабочего стола ────────────────────────────────────────

    fn set_aeroshell(&self, enabled: bool) -> QString {
        match save_setting(aeroshell_key(), Value::from(enabled)) {
            Ok(()) => {
                QString::from(&qs::aeroshell_toggle_status(enabled))
            }
            Err(text) => QString::from(&text),
        }
    }

    // ── Оформление AeroShell отстало от системы ──────────────────────────
    //
    // Порт `offer_aeroshell_rebuild` (`rexon/qt_main/de_progress.py`) и
    // сверки при запуске (`rexon/qt_main/window.py`). Исполнитель — ядро
    // (`system_de_exec::aeroshell_survey_live` / `rebuild_aeroshell_live`);
    // окно — общее `DeOperationDialog.qml`. Порядок: сверка в фоне → отстало —
    // окно вопроса («🛠 Восстановить» / «Не сейчас») → после «Восстановить»
    // пересборка с ходом дела → итог в окне и в подвале.
    // ⛔ Пересборка сама не начинается никогда: только нажатием в окне.

    /// «Проверить сейчас».
    fn aeroshell_check_now(mut self: Pin<&mut Self>) -> QString {
        self.as_mut().start_update_guard(true);
        QString::from(&self.start_aeroshell_survey(true))
    }

    /// Проверка при запуске Rexon: только если не выключена в настройках
    /// (ключа нет — включена).
    fn aeroshell_startup_check(mut self: Pin<&mut Self>) {
        let saved = core_settings::SettingsManager::new()
            .ok()
            .and_then(|m| m.get(aeroshell_key()).and_then(Value::as_bool));
        let enabled = aeroshell::check_on_start(saved);
        sync_update_watch();
        // Защита от обновления вразнобой — всем и всегда, не только при
        // AeroThemePlasma: молча, если всё в порядке.
        self.as_mut().start_update_guard(false);
        // Открыты кнопкой уведомления о поломке — проверить сразу и сказать
        // итог вслух, даже если проверка при запуске выключена: человек
        // сам попросил.
        let from_notice = std::env::args().any(|a| a == aeroshell::OPEN_FLAG);
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let plasma = rexon_core::app::is_plasma_desktop(&desktop);
        if enabled && !from_notice && !plasma {
            // Только в журнал: окно вопроса в чужом сеансе не всплывает.
            rexon_core::log_info!(
                "aeroshell",
                "сверка оформления при запуске пропущена: сеанс «{desktop}» — не Plasma"
            );
        }
        if !startup_survey_wanted(enabled, from_notice, plasma) {
            return;
        }
        let _ = self.start_aeroshell_survey(from_notice);
    }

    // ── Обновление поставит набор пакетов вразнобой ─────────────────────
    //
    // Решения и тексты — ядро (`rexon_core::update_guard`); здесь — сверка
    // в фоне и окно-сообщение. Об одном состоянии — один раз (отпечаток
    // общий с уведомлением фоновой проверки); по кнопке — всегда.

    /// Сверка «до обновления» в фоне. `manual` — по кнопке «Проверить сейчас».
    fn start_update_guard(self: Pin<&mut Self>, manual: bool) {
        if UPDATE_GUARD_BUSY.swap(true, Ordering::SeqCst) {
            return;
        }
        let handle = self.qt_thread();
        std::thread::spawn(move || {
            let found = std::panic::catch_unwind(rexon_core::update_guard::advise_live).ok();
            deliver(&handle, move |object| object.update_guard_done(found, manual));
        });
    }

    fn update_guard_done(self: Pin<&mut Self>, found: Option<rexon_core::update_guard::Advice>, manual: bool) {
        use rexon_core::update_guard as ug;
        UPDATE_GUARD_BUSY.store(false, Ordering::SeqCst);
        let Some(advice) = found.filter(|a| !a.findings.is_empty()) else {
            return;
        };
        let fingerprint = advice.fingerprint();
        let actionable = ug::orphan_offer_live(&advice.findings) != ug::OrphanOffer::Nothing
            || ug::rollback_offer_live(&advice).is_some();
        // Уведомление звало «откройте Rexon, чтобы удалить / вернуть» — окно
        // с кнопкой показывается и после него.
        if !ug::window_should_show(
            manual,
            actionable,
            ug::notice_seen_live(&fingerprint),
            ug::window_seen_live(&fingerprint),
        ) {
            return;
        }
        if !self.rust().op.phase.is_empty() {
            // Открыто другое окно — поверх него не говорим и не отмечаем:
            // скажем при следующем запуске или уведомлением.
            return;
        }
        self.show_update_guard(advice);
    }

    /// Окно сторожа обновлений: набор уже разошёлся и прежние версии есть
    /// в кэше — вопрос с кнопкой «Вернуть прежние версии»; есть сироты,
    /// которые можно удалить, — с кнопкой «Удалить …»; иначе сообщение без
    /// действия.
    fn show_update_guard(mut self: Pin<&mut Self>, advice: rexon_core::update_guard::Advice) {
        use rexon_core::update_guard as ug;
        let fingerprint = advice.fingerprint();
        rexon_core::log_warn!("update", "обновление вразнобой — {fingerprint}");
        ug::window_mark_live(&fingerprint);
        // Вернуть прежние версии — первым: разошедшийся набор грозит чёрным
        // экраном после перезагрузки. Сирота, если есть, — окном после итога
        // отката (повторная сверка).
        if let Some(offer) = ug::rollback_offer_live(&advice) {
            let text = advice.text_for(ug::Audience::Window { removable: false, rollback: true });
            let view = OpView {
                yes_text: ug::rollback_button(),
                no_text: dp::aeroshell_skip_button(),
                ..OpView::confirm(&advice.title(), &rexon_core::ui_base::escape_dialog_text(&text))
            };
            self.as_mut().rust_mut().pending = Some(PendingOp::RollbackAsk(offer));
            self.as_mut().set_op(view);
            return;
        }
        // Удалять ли и можно ли отсюда — решает ядро (на SteamOS — нечем, в
        // сеансе, который держится на пакете, — нельзя: тогда вместо кнопки
        // сказать, откуда удалить).
        let removable = match ug::orphan_offer_live(&advice.findings) {
            ug::OrphanOffer::Remove(names) => names,
            ug::OrphanOffer::Blocked(why) => {
                let text = format!("{}\n\n{why}", advice.text_for(ug::Audience::Window { removable: false, rollback: false }));
                self.as_mut().set_op(aeroshell_notice_view(&advice.title(), &text));
                return;
            }
            ug::OrphanOffer::Nothing => {
                let text = advice.text_for(ug::Audience::Window { removable: false, rollback: false });
                self.as_mut().set_op(aeroshell_notice_view(&advice.title(), &text));
                return;
            }
        };
        let text = advice.text_for(ug::Audience::Window { removable: true, rollback: false });
        let view = OpView {
            yes_text: ug::orphan_remove_button(&removable),
            no_text: dp::aeroshell_skip_button(),
            danger: true,
            ..OpView::confirm(&advice.title(), &rexon_core::ui_base::escape_dialog_text(&text))
        };
        self.as_mut().rust_mut().pending = Some(PendingOp::RemoveOrphans(removable));
        self.as_mut().set_op(view);
    }

    /// Итог «Сделать … сеансом по умолчанию» из окна сторожа: удалось —
    /// снова вопрос «Удалить …» (теперь помощник не откажет); нет — почему.
    fn finish_wayland_default(
        mut self: Pin<&mut Self>,
        session: &'static rexon_core::system_plan::SessionSuccessor,
        names: Vec<String>,
        done: Result<String, String>,
    ) {
        use rexon_core::update_guard as ug;
        let title = rexon_core::system_plan::wayland_default_button(session);
        self.as_mut().rust_mut().cancel = None;
        match done {
            Ok(text) => {
                rexon_core::log_info!("update", "сеанс по умолчанию: {} → {}", session.x11, session.wayland);
                let text = format!("{text}\n\n{}", ug::orphan_now_removable());
                let view = OpView {
                    yes_text: ug::orphan_remove_button(&names),
                    no_text: dp::aeroshell_skip_button(),
                    danger: true,
                    ..OpView::confirm(&title, &rexon_core::ui_base::escape_dialog_text(&text))
                };
                self.as_mut().rust_mut().pending = Some(PendingOp::RemoveOrphans(names));
                self.as_mut().set_op(view);
            }
            Err(why) => {
                rexon_core::log_warn!("update", "сеанс по умолчанию не сменён ({}): {why}", session.x11);
                self.as_mut().operation_log(QString::from(&format!("{}{why}", dp::PREFIX_ERROR)));
                self.as_mut().set_op(aeroshell_notice_view(&title, &why));
            }
        }
    }

    /// Итог удаления сироты: удалось — «Удалено: …» (и «остальное в
    /// порядке», если повторная сверка чиста); нет — почему, словами.
    fn finish_remove_orphans(
        mut self: Pin<&mut Self>,
        names: Vec<String>,
        done: Option<(Result<(), String>, Option<rexon_core::update_guard::Advice>)>,
    ) {
        use rexon_core::update_guard as ug;
        let (removed, rest) = done.unwrap_or_else(|| (Err(ug::orphan_remove_failed(&names)), None));
        let rest = rest.filter(|a| !a.findings.is_empty());
        let outcome = dx::DeOpOutcome {
            ok: removed.is_ok(),
            message: removed.as_ref().err().cloned().unwrap_or_default(),
            cancelled: false,
            restart_needed: false,
        };
        let mut op = self.rust().op.clone();
        let result = ug::orphan_removed_text(&names, rest.is_none());
        if let Some(line) = op.apply_done(&outcome, rexon_core::ui_base::escape_dialog_text(&result)) {
            self.as_mut().operation_log(QString::from(&line));
        }
        // Отказ «экран входа сам открывает сеанс на этом пакете» — не тупик:
        // вместо голого отказа кнопка «Сделать … сеансом по умолчанию».
        if let Some(session) = removed.as_ref().err().and_then(|why| ug::login_default_successor(&names, why)) {
            let why = removed.as_ref().err().cloned().unwrap_or_default();
            rexon_core::log_warn!("update", "сирота не удалена ({}): экран входа открывает {}", names.join(" "), session.x11);
            let confirm = rexon_core::system_plan::wayland_default_confirm(session);
            let button = rexon_core::system_plan::wayland_default_button(session);
            let view = OpView {
                yes_text: button.clone(),
                no_text: dp::aeroshell_skip_button(),
                ..OpView::confirm(&button, &rexon_core::ui_base::escape_dialog_text(&format!("{why}\n\n{confirm}")))
            };
            self.as_mut().rust_mut().cancel = None;
            self.as_mut().rust_mut().pending = Some(PendingOp::WaylandDefault(session, names));
            self.as_mut().set_op(view);
            return;
        }
        if let Err(why) = &removed {
            rexon_core::log_warn!("update", "сирота не удалена ({}): {why}", names.join(" "));
            self.as_mut().operation_log(QString::from(&format!("{}{why}", dp::PREFIX_ERROR)));
            op.result_text = rexon_core::ui_base::escape_dialog_text(why);
        }
        let footer = dx::operation_footer(&ug::orphan_remove_title(&names), &op.status);
        self.as_mut().rust_mut().cancel = None;
        self.as_mut().rust_mut().update_guard_rest = rest;
        self.as_mut().set_op(op);
        self.as_mut().status(QString::from(&footer));
    }

    /// Итог отката: удалось — «прежние версии вернулись» (набор сошёлся;
    /// «остальное в порядке», если повторная сверка чиста) либо «вернулись,
    /// но всё ещё не сходится»; нет — почему, словами.
    fn finish_rollback(
        mut self: Pin<&mut Self>,
        offer: rexon_core::update_guard::RollbackOffer,
        done: Option<(Result<(), String>, Option<rexon_core::update_guard::Advice>)>,
    ) {
        use rexon_core::update_guard as ug;
        let (back, rest) = done.unwrap_or_else(|| (Err(ug::rollback_failed(&offer)), None));
        let rest = rest.filter(|a| !a.findings.is_empty());
        let still_mixed = rest.as_ref().is_some_and(|a| {
            a.findings.iter().any(|f| f.set.name == offer.set_name && f.stage != ug::Stage::Orphaned)
        });
        let outcome = dx::DeOpOutcome {
            ok: back.is_ok(),
            message: back.as_ref().err().cloned().unwrap_or_default(),
            cancelled: false,
            restart_needed: false,
        };
        let mut op = self.rust().op.clone();
        let result = if still_mixed {
            ug::rollback_still_mixed_text(&offer)
        } else {
            ug::rollback_done_text(&offer, rest.is_none())
        };
        if let Some(line) = op.apply_done(&outcome, rexon_core::ui_base::escape_dialog_text(&result)) {
            self.as_mut().operation_log(QString::from(&line));
        }
        if let Err(why) = &back {
            rexon_core::log_warn!("update", "прежние версии {} не вернулись: {why}", offer.set_name);
            self.as_mut().operation_log(QString::from(&format!("{}{why}", dp::PREFIX_ERROR)));
            op.result_text = rexon_core::ui_base::escape_dialog_text(why);
        }
        let footer = dx::operation_footer(&ug::rollback_title(&offer), &op.status);
        self.as_mut().rust_mut().cancel = None;
        self.as_mut().rust_mut().update_guard_rest = rest;
        self.as_mut().set_op(op);
        self.as_mut().status(QString::from(&footer));
    }

    /// Сверка в фоне. `manual` — нажата кнопка: итог «всё в порядке» сказать
    /// вслух; при запуске Rexon молчать.
    fn start_aeroshell_survey(mut self: Pin<&mut Self>, manual: bool) -> String {
        if AEROSHELL_BUSY.swap(true, Ordering::SeqCst) {
            return if manual {
                aeroshell::rebuild_busy_message()
            } else {
                String::new()
            };
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let found = std::panic::catch_unwind(dx::aeroshell_advice_live).ok();
            deliver(&handle, move |object| object.aeroshell_survey_done(found, manual));
        });
        if manual {
            aeroshell::survey_running_message()
        } else {
            String::new()
        }
    }

    fn aeroshell_survey_done(
        mut self: Pin<&mut Self>,
        found: Option<(aeroshell::Report, aeroshell::Advice)>,
        manual: bool,
    ) {
        let Some((report, advice)) = found else {
            // Сверка упала сама. При запуске молчим (она забота, а не условие
            // работы), по кнопке — говорим.
            AEROSHELL_BUSY.store(false, Ordering::SeqCst);
            if manual {
                let msg = t1("status.error", &completeness_internal_error());
                self.as_mut().status(QString::from(&msg));
            }
            return;
        };
        if advice == aeroshell::Advice::Nothing {
            AEROSHELL_BUSY.store(false, Ordering::SeqCst);
            if manual {
                self.as_mut().status(QString::from(&t("aero.up_to_date")));
            }
            return;
        }
        // Предупреждение ДО обновления при запуске — один раз на одно
        // состояние, общее с уведомлением фоновой проверки: окно, которое
        // каждый запуск твердит одно и то же, перестают читать. По кнопке
        // (и из уведомления) — всегда.
        let fingerprint = aeroshell::notify_fingerprint(&report, &advice);
        // Сторонние модули из журнала — так же один раз на состояние: это не
        // поломка всего стола, и каждый запуск твердить о них незачем.
        let before_update = matches!(
            advice,
            aeroshell::Advice::HoldUpdate { .. }
                | aeroshell::Advice::RebuildAfterUpdate { .. }
                | aeroshell::Advice::BrokenPlugins { .. }
        );
        // То же — пока оформление работает и только может сломаться
        // (`at_risk_only`): о риске говорим один раз на состояние.
        let risk_only =
            matches!(advice, aeroshell::Advice::Rebuild { .. }) && report.at_risk_only();
        if (before_update || risk_only) && !manual && dx::aeroshell_notice_seen_live(&fingerprint) {
            AEROSHELL_BUSY.store(false, Ordering::SeqCst);
            return;
        }
        if !self.rust().op.phase.is_empty() {
            // Открыто другое окно операции — поверх него не спрашиваем.
            AEROSHELL_BUSY.store(false, Ordering::SeqCst);
            if manual {
                self.as_mut()
                    .status(QString::from(&aeroshell::rebuild_busy_message()));
            }
            return;
        }
        rexon_core::log_info!(
            "aeroshell",
            "сверка оформления — {advice:?}:\n{}",
            aeroshell::report_details(&report)
        );
        match advice {
            aeroshell::Advice::Nothing => {}
            aeroshell::Advice::Rebuild { packages } => {
                if risk_only {
                    dx::aeroshell_notice_mark_live(&fingerprint);
                }
                self.as_mut().rust_mut().pending = Some(PendingOp::Aeroshell(packages));
                self.as_mut().set_op(aeroshell_confirm_view(&report));
                self.as_mut()
                    .status(QString::from(&dx::awaiting_answer(&dp::aeroshell_run_title())));
            }
            aeroshell::Advice::RestoreStock { forks } => {
                self.as_mut().rust_mut().pending = Some(PendingOp::RestoreStock(forks));
                self.as_mut().set_op(restore_stock_confirm_view());
                self.as_mut()
                    .status(QString::from(&dx::awaiting_answer(&aeroshell::restore_run_title())));
            }
            aeroshell::Advice::WaitForUpstream { plasma, fork } => {
                // Пересборка сейчас дала бы то же сломанное: честно сказать и
                // ничего не запускать. Спросим снова при следующем запуске.
                AEROSHELL_BUSY.store(false, Ordering::SeqCst);
                self.as_mut().set_op(aeroshell_notice_view(
                    &aeroshell::wait_title(),
                    &aeroshell::wait_text(&plasma, &fork),
                ));
            }
            aeroshell::Advice::RestoreBlocked { message } => {
                // Только сказать: запускать нечего, ждать ответа тоже.
                AEROSHELL_BUSY.store(false, Ordering::SeqCst);
                self.as_mut()
                    .set_op(aeroshell_notice_view(&aeroshell::restore_blocked_title(), &message));
            }
            aeroshell::Advice::HoldUpdate { plasma, fork } => {
                // Сейчас всё работает, но обновление сломает: только сказать.
                AEROSHELL_BUSY.store(false, Ordering::SeqCst);
                dx::aeroshell_notice_mark_live(&fingerprint);
                self.as_mut().set_op(aeroshell_notice_view(
                    &aeroshell::hold_title(),
                    &aeroshell::hold_text(&plasma, &fork),
                ));
            }
            aeroshell::Advice::RebuildAfterUpdate { to, .. } => {
                AEROSHELL_BUSY.store(false, Ordering::SeqCst);
                dx::aeroshell_notice_mark_live(&fingerprint);
                self.as_mut().set_op(aeroshell_notice_view(
                    &aeroshell::rebuild_after_title(),
                    &aeroshell::rebuild_after_text(&to),
                ));
            }
            aeroshell::Advice::BrokenPlugins { rebuild } => {
                dx::aeroshell_notice_mark_live(&fingerprint);
                if rebuild.is_empty() {
                    AEROSHELL_BUSY.store(false, Ordering::SeqCst);
                    self.as_mut().set_op(broken_plugins_view(&report, false));
                } else {
                    // Та же пересборка, что у набора: `paru/yay -S --rebuild`
                    // только после «Восстановить».
                    self.as_mut().rust_mut().pending = Some(PendingOp::Aeroshell(rebuild));
                    self.as_mut().set_op(broken_plugins_view(&report, true));
                    self.as_mut()
                        .status(QString::from(&dx::awaiting_answer(&dp::aeroshell_run_title())));
                }
            }
        }
    }

    fn finish_aeroshell(self: Pin<&mut Self>, done: Option<dx::DeOpOutcome>) {
        self.finish_aeroshell_op(done, &dp::aeroshell_run_title(), &dp::aeroshell_ok_text());
    }

    fn finish_restore_stock(self: Pin<&mut Self>, done: Option<dx::DeOpOutcome>) {
        self.finish_aeroshell_op(done, &aeroshell::restore_run_title(), &aeroshell::restore_ok_text());
    }

    /// Итог пересборки либо возврата штатной Plasma: одна дорожка на обе.
    fn finish_aeroshell_op(
        mut self: Pin<&mut Self>,
        done: Option<dx::DeOpOutcome>,
        title: &str,
        ok_text: &str,
    ) {
        let panicked = done.is_none();
        let outcome = done.unwrap_or_else(|| dx::DeOpOutcome {
            ok: false,
            message: format!("{}: {}", dp::aeroshell_fail_title(), completeness_internal_error()),
            cancelled: false,
            restart_needed: false,
        });
        let mut op = self.rust().op.clone();
        let result = if outcome.ok { ok_text.to_string() } else { String::new() };
        if let Some(line) = op.apply_done(&outcome, result) {
            self.as_mut().operation_log(QString::from(&line));
        }
        if panicked {
            // Ошибку по ходу дела никто не объявил — объявляем здесь.
            self.as_mut().operation_log(QString::from(&format!(
                "{}{}",
                dp::PREFIX_ERROR,
                outcome.message
            )));
        }
        let footer = dx::operation_footer(title, &op.status);
        self.as_mut().rust_mut().cancel = None;
        AEROSHELL_BUSY.store(false, Ordering::SeqCst);
        self.as_mut().set_op(op);
        self.as_mut().status(QString::from(&footer));
    }
}

// ============================================================================
// Исполнение действий
// ============================================================================

/// Записать одну настройку; ошибка приходит ГОТОВЫМ текстом ядра.
/// Раздел «Язык» снимка: сохранённый выбор и нужен ли перезапуск.
fn language_state() -> Value {
    let saved = rexon_core::i18n::saved_setting();
    // Сохранён язык, которого в списке нет (каталог не поставлен, код с
    // ошибкой): говорит Rexon тогда на том, во что это вылилось (обычно
    // английский), — его и показываем, а не молчаливый первый пункт списка.
    let choice = if qs::language_choice_valid(&saved) {
        saved
    } else {
        rexon_core::i18n::language_for_setting(Some(&saved))
    };
    json!({
        "choice": choice,
        "restart": rexon_core::i18n::restart_needed(&choice),
    })
}

/// Язык интерфейса парой установщика (`Lang`): тот же, что у каталога.
/// Прежде здесь стоял `Lang::from_env`, читавший локаль мимо `ui_language`.
fn ui_lang() -> rexon_core::distro_compat::Lang {
    rexon_core::distro_compat::Lang::from_code(&rexon_core::i18n::current_language())
}

/// Сторож перезапуска: сколько тиков по 0,2 с следить, не умер ли новый
/// процесс сразу после запуска (60 с — с запасом на ожидание замка).
const RESTART_WATCH_TICKS: u32 = 300;

/// Оболочка-сторож нового процесса. Запускает его, следит минуту и, если
/// он умер с ошибкой (не хватило библиотек, упал до окна), пишет строку в
/// журнал и показывает уведомление — человек иначе увидел бы только, что
/// Rexon «не вернулся». Ушёл с нулём (постучался в живую копию) — молчит.
/// `$1` — журнал, `$2`/`$3` — заголовок и текст уведомления, дальше — команда.
const RESTART_WATCHER: &str = r#"log=$1; summary=$2; body=$3; shift 3
"$@" &
c=$!
i=0
while [ $i -lt $REXON_RESTART_WATCH_TICKS ]; do
  st=X
  [ -r /proc/$c/stat ] && read -r _ _ st _ < /proc/$c/stat
  case $st in Z|X) break ;; esac
  sleep 0.2; i=$((i+1))
done
case $st in Z|X) ;; *) exit 0 ;; esac
wait $c; rc=$?
[ $rc -eq 0 ] && exit 0
printf '%s [%s] ERROR restart: новый процесс %s завершился с кодом %s сразу после запуска
' "$(date '+%Y-%m-%d %H:%M:%S.%3N')" "$$" "$c" "$rc" >> "$log"
if command -v dbus-send >/dev/null 2>&1; then
  dbus-send --session --dest=org.freedesktop.Notifications --type=method_call /org/freedesktop/Notifications org.freedesktop.Notifications.Notify string:Rexon uint32:0 string:rexon "string:$summary" "string:$body" array:string: dict:string:string: int32:0 >/dev/null 2>&1
elif command -v notify-send >/dev/null 2>&1; then
  notify-send --app-name=Rexon --icon=rexon "$summary" "$body"
fi
exit 0
"#;

/// СТЕНД, не продукт: сколько ещё перезапусков сделать цепочкой. Ставит
/// прогон «перезапуск N раз подряд» в песочнице; у покупателя переменной
/// нет, и ветка спит.
pub const STAND_RESTARTS_ENV: &str = "REXON_STAND_RESTARTS";
/// Остаток цепочки этого процесса (читает `main_window` при старте).
pub static STAND_RESTARTS_LEFT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Перезапуск по стенду — тот же путь, что у кнопки «Перезапустить».
pub fn stand_restart_now() {
    match spawn_restart() {
        Ok(()) => crate::tray::request_quit("стенд перезапуска"),
        Err(reason) => rexon_core::log_error!("restart", "стенд: перезапуск отменён: {reason}"),
    }
}

/// Путь исполняемого файла для перезапуска. После обновления пакета поверх
/// работающего Rexon ядро отдаёт `…/rexon-main-window (deleted)` — такого
/// файла нет, а новый лежит по прежнему пути без приписки.
pub(crate) fn restart_exe_path(raw: &str) -> String {
    raw.strip_suffix(" (deleted)").unwrap_or(raw).to_string()
}

/// Пробный запуск новой копии с немедленной привязкой ВСЕХ символов
/// (`LD_BIND_NOW=1`) и выходом в первой строке `main`. Ловит ровно живой
/// случай 08.10.2026: Qt обновилась наполовину, и новая копия падала с
/// «symbol lookup error» уже после того, как старая закрылась.
/// `Err` — первая строка ответа динамического загрузчика.
fn probe_new_copy(exe: &str) -> Result<(), String> {
    let spec = rexon_core::utils::RunSpec {
        argv: vec![exe.to_string()],
        output: rexon_core::utils::RunOutput::Capture,
        timeout_secs: Some(15),
        env: vec![
            ("LD_BIND_NOW".to_string(), "1".to_string()),
            (rexon_core::cli::LINK_PROBE_ENV.to_string(), "1".to_string()),
        ],
        lang_c: false,
        stderr_to_stdout: true,
    };
    let report = rexon_core::utils::run(&spec, None);
    if report.outcome == rexon_core::utils::RunOutcome::Ok {
        return Ok(());
    }
    let text = rexon_core::utils::decode_text(&report.stdout);
    // Строка с ошибкой загрузчика («symbol lookup error», «error while
    // loading shared libraries»), а не предупреждения перед ней.
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let first = lines
        .iter()
        .find(|l| l.contains("error"))
        .or(lines.last())
        .copied()
        .unwrap_or("")
        .to_string();
    Err(if first.is_empty() {
        format!("код {:?} {}", report.returncode, report.error)
    } else {
        first
    })
}

/// Запустить новую копию Rexon (тем же исполняемым файлом, с аргументами
/// `qt_settings::restart_args`) под сторожем [`RESTART_WATCHER`].
///
/// Новый процесс знает, что он перезапуск (`single_instance::RESTART_OF_ENV`),
/// и сам ждёт, пока этот уйдёт и отпустит замок; прежнее ожидание снаружи
/// (`kill -0` до 30 с и запуск вслепую) снято. Окружение языка — исходное
/// (`i18n::restore_original_locale_env`): подменённый этим окном `LANGUAGE`
/// новому процессу только мешал бы выбрать «как в системе».
fn spawn_restart() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    spawn_restart_with(&qs::restart_args(&args))
}

/// Перезапуск ради обновления пакета (кнопка уведомления «Rexon обновлён»):
/// тот же путь, что у кнопки «Перезапустить» в «Настройках», но новая копия
/// открывает окно там, где человек был, а не раздел «Настройки»
/// (`single_instance::update_restart_args`). `Err` — текст для человека,
/// Rexon остаётся открытым. Звать можно из любого потока.
pub(crate) fn restart_for_update() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match spawn_restart_with(&rexon_core::single_instance::update_restart_args(&args)) {
        Ok(()) => {
            crate::tray::request_quit("перезапуск после обновления Rexon");
            Ok(())
        }
        Err(reason) => {
            rexon_core::log_error!("restart", "после обновления отменён, Rexon остаётся открытым: {reason}");
            Err(qs::language_restart_failed(&reason))
        }
    }
}

fn spawn_restart_with(new_args: &[String]) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    use rexon_core::{log_info, log_warn};
    let pid = std::process::id();
    let raw = std::env::current_exe().map_err(|e| e.to_string())?;
    let raw = raw.to_string_lossy().into_owned();
    let exe = restart_exe_path(&raw);
    if exe != raw {
        log_warn!("restart", "исполняемый файл заменён обновлением, беру путь {exe}");
    }
    log_info!(
        "restart",
        "шаг 1: проверка новой копии {exe} (язык: выбор {}, станет {})",
        rexon_core::i18n::saved_setting(),
        rexon_core::i18n::language_for_setting(Some(&rexon_core::i18n::saved_setting()))
    );
    if let Err(why) = probe_new_copy(&exe) {
        rexon_core::log_error!("restart", "новая копия не запускается: {why}");
        return Err(rexon_core::i18n::trf(
            // TRANSLATORS: %1 is the technical error from the system loader (usually English). Rexon stays open.
            "the new copy of Rexon can't start (%1). Rexon stays open. This usually means a system update hasn't finished — finish it or restart the computer.",
            &[&why],
        ));
    }
    let log_path = rexon_core::applog::log_path();
    // TRANSLATORS: Title of a desktop notification when Rexon failed to come back after a restart.
    let summary = rexon_core::i18n::tr("Rexon didn't restart");
    let body = rexon_core::i18n::trf(
        // TRANSLATORS: Body of a desktop notification; %1 is the path of the Rexon log file.
        "The new copy of Rexon exited with an error right after starting. Open Rexon from the application menu; details are in the log: %1",
        &[&log_path.to_string_lossy()],
    );
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c")
        .arg(RESTART_WATCHER)
        .arg("rexon-restart")
        .arg(&log_path)
        .arg(&summary)
        .arg(&body)
        .arg(&exe)
        .args(new_args)
        .env(rexon_core::single_instance::RESTART_OF_ENV, pid.to_string())
        .env("REXON_RESTART_WATCH_TICKS", RESTART_WATCH_TICKS.to_string())
        .stdin(std::process::Stdio::null())
        .process_group(0);
    rexon_core::i18n::restore_original_locale_env(&mut cmd);
    // Стенд цепочки перезапусков: следующей копии — на один меньше.
    let left = STAND_RESTARTS_LEFT.load(std::sync::atomic::Ordering::SeqCst);
    if left > 0 {
        cmd.env(STAND_RESTARTS_ENV, (left - 1).to_string());
    }
    if let Some(stderr) = rexon_core::applog::original_stderr_for_child() {
        cmd.stderr(stderr);
    }
    let child = cmd.spawn().map_err(|e| e.to_string())?;
    log_info!(
        "restart",
        "шаг 2: сторож {} запустил новую копию: {exe} {}; прежний процесс {pid} уходит",
        child.id(),
        new_args.join(" ")
    );
    Ok(())
}

fn save_setting(key: &str, value: Value) -> Result<(), String> {
    core_settings::SettingsManager::new()
        .and_then(|mut m| m.set(key, value))
        .map_err(|e| t1("status.error", &e.to_string()))
}

/// Сохранить запомненные приложения по умолчанию.
fn save_protected(protected: &[(String, Option<String>)]) -> Result<(), String> {
    // Метку времени печатает ядро (`engine_update::now_iso`) — второй
    // записи формата в продукте нет.
    let stamp = engine_update::now_iso(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    );
    write_file_atomic(
        &format!("{}/rexon/protected_defaults.json", config_home()),
        &defaults::serialize_snapshot(protected, &stamp),
    )
    .map_err(|e| e.to_string())
}

/// Запомнить приложение в списке (порядок сохраняется, новые дописываются).
fn remember(protected: &mut Vec<(String, Option<String>)>, app_type: &str, desktop: &str) {
    match protected.iter_mut().find(|(k, _)| k == app_type) {
        Some(entry) => entry.1 = Some(desktop.to_string()),
        None => protected.push((app_type.to_string(), Some(desktop.to_string()))),
    }
}

/// Применить приложение по умолчанию. ЧТО именно запустить, решает ядро
/// (`defaults::set_default`), здесь только запуск.
fn apply_default(app_type: &str, desktop: &str) -> bool {
    match defaults::set_default(app_type, desktop) {
        defaults::SetDefault::Terminal(terminal) => {
            // Терминал ставится не через xdg-mime: у него свой план из трёх
            // мест записи (список freedesktop, gsettings, kdeglobals).
            run_terminal_plan(&defaults::set_terminal_plan(
                &terminal,
                &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
                &home(),
            ));
            true
        }
        defaults::SetDefault::Xdg(cmds) => {
            if cmds.is_empty() {
                return false;
            }
            cmds.iter().all(|argv| write_cmd(argv))
        }
    }
}

/// Исполнить план настройки терминала. Код возврата шагов не смотрится — как
/// и у оригинала: часть мест записи есть не в каждом окружении.
fn run_terminal_plan(plan: &[defaults::TermStep]) {
    let mut env_written = false;
    for step in plan {
        match step {
            defaults::TermStep::WriteFile { path, content } => {
                if write_file_atomic(path, content).is_ok() {
                    env_written |= rexon_core::compositor_autostart::is_environment_d_path(path);
                }
            }
            defaults::TermStep::MakeDirs { path } => {
                let _ = std::fs::create_dir_all(path);
            }
            defaults::TermStep::Run { argv } => {
                write_cmd(argv);
            }
            defaults::TermStep::SetIni {
                path,
                section,
                key,
                value,
            } => {
                let existing = read_file(path);
                let text = defaults::set_ini_value(existing.as_deref(), section, key, value);
                let _ = write_file_atomic(path, &text);
            }
        }
    }
    // `TERMINAL` — и в сеансы, которые `environment.d` не читают (Д1).
    if env_written {
        let _ = rexon_core::compositor_autostart::sync_session_env_live();
    }
}

/// Сделать `fm_id` файловым менеджером по умолчанию → применилось ли.
///
/// Порт `appdefaults.set_file_manager_default`: выбор целевого `.desktop`,
/// запись через `xdg-mime`, per-user override активации D-Bus FileManager1
/// («показать в папке» идёт НЕ через mimeapps) и проверка того, что реально
/// записалось. Все решения — у ядра.
fn set_file_manager_default(fm_id: &str) -> bool {
    let Some(desktop) = appdefaults::fm_default_target(fm_id, &desktop_exists) else {
        return false;
    };
    if !write_cmd(&appdefaults::xdg_mime_default_cmd(&desktop)) {
        return false;
    }
    override_filemanager1(&appdefaults::fm_short_name(fm_id, &desktop));
    // Родное место окружения сверх mimeapps: в XFCE — `helpers.rc
    // FileManager=` (его читают `exo-open`, панель и рабочий стол; Д7,
    // проверка окружений 02.10.2026). Шаги решает ядро.
    run_terminal_plan(&appdefaults::file_manager_home_steps(
        &desktop,
        &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        &config_home(),
        &defaults::xfce_system_helper_exists,
    ));
    // Верификация: что реально записалось. Без неё кнопка рапортует об
    // успехе там, где `xdg-mime` отработал вхолостую.
    read_cmd(&appdefaults::xdg_mime_query_cmd()).contains(&desktop)
}

/// Per-user override активации `org.freedesktop.FileManager1`.
///
/// Системным service-файлом на Arch владеет пакет nautilus: при сосуществовании
/// столов GNOME Files перехватывает «показать в папке», что бы ни стояло в
/// mimeapps. Файл в `~/.local/share/dbus-1/services/` перекрывает системный.
fn override_filemanager1(binary: &str) {
    let Ok(entries) = std::fs::read_dir("/usr/share/dbus-1/services") else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("service") {
            continue;
        }
        let Some(text) = path.to_str().and_then(read_file) else {
            continue;
        };
        // Тот ли это файловый менеджер — решает ядро (`fm1_matches`).
        if !appdefaults::fm1_matches(&text, binary) {
            continue;
        }
        let target = format!(
            "{}/dbus-1/services/{}.service",
            data_home(),
            appdefaults::FM1_NAME
        );
        let _ = write_file_atomic(&target, &text);
        return;
    }
}

/// Запустить и забыть: проигрыватель звука живёт своей жизнью, ждать его
/// нельзя — кнопка «прослушать» подвисла бы на всю длину звука.
fn spawn_detached(argv: &[String]) -> bool {
    let Some((binary, args)) = argv.split_first() else {
        return false;
    };
    std::process::Command::new(binary)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Имя пункта ярлыка, который открывал классическое окно. Пункта в
    /// ярлыке нет с 16.09, и формулировка, звавшая туда, снята — имя
    /// осталось только здесь, сторожем: ни один ответ покупателю не
    /// имеет права снова отослать его в снятое окно.
    use rexon_core::installer::CLASSIC_WINDOW_ENTRY;

    // ── Общий исполнитель обоев процесса ─────────────────────────────────

    /// Подменная машина исполнителя: ничего не запускает и не пишет, только
    /// помнит, что подняли и что сняли. Живые обои владельца не трогаются.
    #[derive(Default)]
    struct QuietSys {
        spawned: Vec<Vec<String>>,
        killed: Vec<i64>,
        next: i64,
        clock: f64,
    }

    impl WpSys for QuietSys {
        fn run(&mut self, _argv: &[String], _timeout: f64) -> Option<rexon_core::session_splash::RunOut> {
            Some(rexon_core::session_splash::RunOut {
                rc: 0,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
        fn spawn(&mut self, argv: &[String], _new_session: bool) -> Result<i64, String> {
            self.spawned.push(argv.to_vec());
            self.next += 1;
            Ok(self.next)
        }
        fn poll(&mut self, proc: i64) -> Option<i64> {
            self.killed.contains(&proc).then_some(-15)
        }
        fn pid(&mut self, proc: i64) -> Option<i64> {
            Some(4000 + proc)
        }
        fn kill_process(&mut self, proc: i64) {
            self.killed.push(proc);
        }
        fn has_tool(&mut self, _tool: &str) -> bool {
            false
        }
        fn package_hint(&mut self, _tool: &str) -> String {
            String::new()
        }
        fn exists(&mut self, _path: &str) -> bool {
            true
        }
        fn is_dir(&mut self, _path: &str) -> bool {
            false
        }
        fn listdir(&mut self, _path: &str) -> Vec<String> {
            Vec::new()
        }
        fn read_text(&mut self, _path: &str) -> Option<String> {
            None
        }
        fn write_text(&mut self, _path: &str, _content: &str) {}
        fn remove_file(&mut self, _path: &str) {}
        fn env(&mut self, name: &str) -> String {
            if name == "HOME" { "/fake-home".to_string() } else { String::new() }
        }
        fn monotonic(&mut self) -> f64 {
            self.clock += 0.01;
            self.clock
        }
        fn sleep(&mut self, seconds: f64) {
            self.clock += seconds;
        }
        fn bridge_notify(&mut self, _live: bool, _name: &str, _announce: bool) {}
        fn video_host(&mut self) -> Option<String> {
            Some("/opt/rexon/rexon-wallpaper-host".to_string())
        }
        fn file_size(&mut self, _path: &str) -> Option<u64> {
            Some(4096)
        }
    }

    /// Повторять дверь, пока замок не освободится: после применения видео
    /// общая дверь ядра поднимает надзор за нагрузкой, и его первый замер на
    /// мгновение берёт тот же замок. Окно в этот миг отвечает «уже
    /// применяю» — здесь же нужен итог.
    fn when_free<R>(mut door: impl FnMut() -> Option<R>) -> R {
        for _ in 0..200 {
            if let Some(out) = door() {
                return out;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("исполнитель так и не освободился");
    }

    /// ⭐ Один исполнитель на процесс: видео, поставленное страницей «Steam»
    /// (её живой движок `ExecApplier`, галочка звука выключена), «Настройки»
    /// видят своим снимком и снимают своей «Остановить». Слот здесь с
    /// подменным швом, а двери — те же, что у живых страниц.
    #[test]
    fn steam_applied_video_is_seen_and_stopped_by_settings() {
        let slot: wallpaper_exec::ExecSlot<QuietSys> = Arc::new(Mutex::new(Some(
            WallpaperExec::with_env(QuietSys::default(), "sway", "wayland"),
        )));

        let steam = crate::steam_workers::ExecApplier::on(slot.clone());
        let (ok, message) = crate::steam_workers::WallpaperApply::apply(
            &steam,
            "/fake-home/.cache/rexon/steam-bg/7.webm",
            "video",
            false,
            "Ночной город",
        );
        assert!(ok, "{message}");

        let (running, format, path) = when_free(|| {
            let now = wallpaper_now(&slot);
            now.0.then_some(now)
        });
        assert!(running, "«Настройки» не видят видео из «Steam»");
        assert_eq!(format.as_deref(), Some("video"));
        assert_eq!(path.as_deref(), Some("/fake-home/.cache/rexon/steam-bg/7.webm"));
        let host_argv = with_slot_sys(&slot, |sys| sys.spawned.last().cloned())
            .expect("хост видео не поднят");
        assert!(host_argv.iter().any(|a| a == "--no-audio"), "галочка звука потерялась: {host_argv:?}");

        let res = when_free(|| stop_wallpaper_in(&slot));
        assert!(res.ok, "{}", res.message);
        assert!(!wallpaper_now(&slot).0, "видео из «Steam» не снято «Остановить»");
        assert!(!with_slot_sys(&slot, |sys| sys.killed.is_empty()), "процесс хоста не снят");
    }

    fn with_slot_sys<R>(
        slot: &wallpaper_exec::ExecSlot<QuietSys>,
        look: impl FnOnce(&mut QuietSys) -> R,
    ) -> R {
        wallpaper_exec::with_slot(slot, |exec| look(exec.sys_mut()))
    }

    /// Снимок обязан назвать ВСЕ восемнадцать ключей — и на пустой машине тоже.
    /// Страница читает их точкой (`root.st.prot.enabled`), и забытый ключ
    /// приехал бы в неё как `undefined`, то есть переключатель молча встал бы
    /// в «выключено», а раздел показал бы пустоту вместо содержимого.
    ///
    /// ⚠️ Имена здесь выписаны ЗАНОВО, а не взяты из `STATE_KEYS`. Сверка со
    /// списком, который сама же и проверяет, зеленела бы на любом его
    /// переименовании — то есть не проверяла бы ничего. Настоящий источник
    /// этих семнадцати слов — `qml/SettingsPage.qml` (`root.st.polkit`,
    /// `root.st.quickfix`, `root.st.aero`…), и переписать их можно только
    /// вместе с ним.
    #[test]
    fn state_names_every_key_the_page_reads() {
        let wanted = [
            "polkit", "lang", "skin", "onboarding", "tray", "fm", "quickfix", "compl", "wp", "lib",
            "imp", "blur", "amb", "snd", "font", "prot", "plasma", "aero",
        ];
        let empty = state_object(std::array::from_fn(|_| Value::Null));
        let map = empty.as_object().expect("снимок обязан быть объектом");
        for key in wanted {
            assert!(map.contains_key(key), "в снимке нет ключа «{key}»");
        }
        assert_eq!(map.len(), wanted.len(), "в снимке появился лишний ключ");
    }

    /// Раздел «Обои» обязан назвать все ключи, которые страница читает
    /// точкой (`wpBody.wp.running`, `wpBody.audio.muteOther`…): забытый
    /// приехал бы `undefined`, и кнопка «Остановить» молча осталась бы
    /// выключенной, а подпись кнопки yt-dlp — пустой. Имена выписаны заново
    /// по `qml/SettingsPage.qml`, а не взяты из кода, который проверяется.
    #[test]
    fn wallpaper_section_names_every_key_the_page_reads() {
        let view = WallpaperView {
            running: false,
            format: None,
            path: None,
            audio: wallpaper_audio::AudioSettings::default(),
            youtube_audio: true,
            ytdlp_version: None,
            ytdlp_busy: false,
            busy: false,
        };
        let section = wallpaper_section(&view);
        for key in [
            "engine", "running", "status", "audio", "ytAudio", "ytdlpLabel", "ytdlpButton",
            "ytdlpAction", "ytdlpBusy", "busy",
        ] {
            assert!(section.get(key).is_some(), "в разделе обоев нет ключа «{key}»");
        }
        for key in ["enabled", "volume", "muteOther", "hideCovered"] {
            assert!(section["audio"].get(key).is_some(), "в звуке обоев нет ключа «{key}»");
        }
        // Раздел есть: без этого страница прячет обе перенесённые кнопки.
        assert_eq!(section["engine"], Value::Bool(true));
        // Кнопка yt-dlp не остаётся без подписи ни при каком ответе системы.
        assert!(!section["ytdlpButton"].as_str().unwrap_or("").is_empty());
        let with_ytdlp = wallpaper_section(&WallpaperView {
            ytdlp_version: Some("2026.09.01".to_string()),
            ..view
        });
        assert_eq!(with_ytdlp["ytdlpAction"], "update");
    }

    /// Какие файлы обоев применяются — решает ядро: картинка, видео и
    /// Flash применяются без классического окна.
    #[test]
    fn picture_video_and_flash_buttons_are_carried() {
        assert!(wallpaper_exec::carries_format("static"));
        assert!(wallpaper_exec::carries_format("video"));
        assert!(wallpaper_exec::carries_format("swf"));
    }

    /// Плитки библиотеки любого формата, кроме Flash, идут в исполнитель: GIF
    /// применяется его планом, сцена и обои-приложение получают те же слова,
    /// что в классическом окне. Слот — с подменным швом, дверь та же, что у
    /// кнопки «Применить» (`run_in_slot` → `apply` → `wallpaper_done`).
    #[test]
    fn library_tiles_except_flash_are_answered_here() {
        // Тексты — из каталога перевода; язык закреплён исходный, чтобы
        // сверка не зависела от локали машины (русский сверяет ядро).
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        for (fmt, want) in [
            ("scene", wallpaper_engine::scene_unsupported()),
            ("application", wallpaper_engine::application_unsupported()),
            ("parallax", wallpaper_engine::unknown_format_message("parallax")),
            // На sway без swww и mpvpaper — отказ плана движка «что поставить».
            (
                "animated",
                rexon_core::i18n::tr("Neither swww nor mpvpaper is installed for GIF wallpapers on Wayland"),
            ),
        ] {
            assert!(wallpaper_exec::carries_format(fmt), "«{fmt}» отослан бы в классическое окно");
            let slot: wallpaper_exec::ExecSlot<QuietSys> = Arc::new(Mutex::new(Some(
                WallpaperExec::with_env(QuietSys::default(), "sway", "wayland"),
            )));
            let path = format!("/fake-home/.local/share/rexon/extensions/пак/data/обои.{fmt}");
            let line = wallpaper_exec::run_in_slot(&slot, move |exec| {
                let res = exec.apply(&path, fmt, "");
                assert!(!res.ok, "{fmt}: {}", res.message);
                qs::wallpaper_done(res.ok, &res.message)
            });
            assert!(line.contains(&want), "{fmt}: {line}");
            assert!(!line.contains(CLASSIC_WINDOW_ENTRY), "{fmt}: {line}");
            assert!(with_slot_sys(&slot, |sys| sys.spawned.is_empty()), "{fmt}: что-то поднято");
        }
    }

    /// Сторож границ: рабочий код страницы НИКУДА не отсылает в классическое
    /// окно — последним переехал Flash, а 16.09 снята и сама формулировка
    /// (`classic_window_only`) вместе с пунктом ярлыка, который она называла.
    /// Кнопка, отсылающая туда снова, покраснеет здесь: и по имени снятой
    /// функции, и по имени пункта ярлыка, которого в ярлыке больше нет.
    #[test]
    fn nothing_goes_to_the_classic_window() {
        const SRC: &str = include_str!("settings_actions.rs");
        let code = &SRC[..SRC.find("#[cfg(test)]").expect("нет модуля тестов")];
        for (i, line) in code.lines().enumerate() {
            let text = line.trim_start();
            if text.starts_with("//") {
                continue;
            }
            assert!(
                !text.contains("classic_window_only(")
                    && !text.contains("blocked(")
                    && !text.contains("CLASSIC_WINDOW_ENTRY"),
                "строка {}: отсылает в классическое окно: {line}",
                i + 1
            );
        }
        let known = ["static", "video", "animated", "web", "youtube", "swf", "scene", "application"];
        for fmt in known {
            assert!(wallpaper_exec::carries_format(fmt), "«{fmt}» снова граница");
        }
    }

    /// Отказ окна шрифтов — НЕ «функция в классическом окне»: там окно есть,
    /// просто оно не открылось, и отсылать человека в другое окно было бы
    /// неправдой.
    #[test]
    fn font_dialog_failure_is_not_a_porting_border() {
        assert!(!font_dialog_failed().contains(CLASSIC_WINDOW_ENTRY));
        assert!(!font_dialog_failed().contains("FontDialog"));
    }

    /// ⭐ Дверь открыта: ответ шима выбора шрифта обязан читаться ЯДРОМ, а
    /// не вторым разбором. Строку «Семейство,кегль» склеивает QML
    /// (`cpp/rexon_ext_dialogs.cpp`, диалог FontPick), и она нарочно сделана
    /// в записи KDE — той самой, которую понимает `parse_font_spec`.
    ///
    /// Имена шрифтов здесь настоящие: у «DejaVu Sans Mono» в имени пробелы,
    /// и наивный разбор по пробелу оторвал бы от него «Mono».
    #[test]
    fn chosen_font_is_read_by_the_core() {
        assert_eq!(
            chosen_font("DejaVu Sans Mono,11"),
            Some(("DejaVu Sans Mono".to_string(), 11))
        );
        // Отказ окна и «ничего не выбрано» — пустая строка, применять нечего.
        assert_eq!(chosen_font(""), None);
    }

    /// ⚠️ Кегль `-1` приходит от Qt всегда, когда шрифт задан ПИКСЕЛЬНЫМ
    /// размером (`QFont::pointSize()` в этом случае возвращает -1). Пропустить
    /// его в конфиг окружения значит записать «Noto Sans,-1» — то есть шрифт
    /// нулевого размера во всём рабочем столе. Запасной кегль тот же, что у
    /// Python-оригинала.
    #[test]
    fn pixel_sized_font_never_reaches_the_config() {
        assert_eq!(chosen_font("Noto Sans,-1"), Some(("Noto Sans".to_string(), 10)));
        assert_eq!(chosen_font("Noto Sans,0"), Some(("Noto Sans".to_string(), 10)));
        for (_, size) in [chosen_font("Noto Sans,-1"), chosen_font("Noto Sans,0")]
            .into_iter()
            .flatten()
        {
            assert!(size > 0, "в конфиг уехал непригодный кегль {size}");
        }
    }

    /// Перенос шрифта интерфейса в GTK: ключ есть только у `system`, и текст
    /// ini собирает общий движок оформления. Проверяется РОВНО та пара
    /// вызовов, которую делает `apply_font`, — иначе сверка зеленела бы на
    /// собственной копии правил.
    #[test]
    fn only_the_interface_font_goes_to_gtk() {
        assert!(qs::font_syncs_to_gtk("system"));
        assert!(!qs::font_syncs_to_gtk("mono"));
        assert!(!qs::font_syncs_to_gtk("title"));

        let text = gtk_sanitizer::write_ini_setting_in(
            Some("[Settings]\ngtk-theme-name=Breeze\n"),
            "Settings",
            qs::GTK_FONT_KEY,
            &qs::gnome_font_spec("Noto Sans", 11),
        );
        assert!(
            text.contains("gtk-font-name=Noto Sans 11"),
            "шрифт не доехал до settings.ini: {text}"
        );
        assert!(
            text.contains("gtk-theme-name=Breeze"),
            "перенос шрифта затёр чужую настройку: {text}"
        );
    }

    /// ⭐ Починка при переносе: имя скина, которое видит список, обязано
    /// совпадать с тем, которым НАРИСОВАНА страница. Python отдавал сырое
    /// значение настройки, а палитра откатывалась к стандартному скину — и
    /// вкладка была выкрашена в один скин, а в списке стоял другой.
    #[test]
    fn skin_shown_is_the_skin_painted() {
        // Чужое имя в настройках: палитра берёт стандартный скин…
        let painted = ui_base::active_skin_id_from(Some("нет-такого-скина"));
        assert_eq!(painted, ui_base::DEFAULT_SKIN);
        // …и снимок обязан назвать его же, а не сохранённый мусор.
        assert!(ui_base::is_valid_skin(painted));
        // Знакомое имя проходит как есть.
        assert_eq!(
            ui_base::active_skin_id_from(Some(ui_base::DEFAULT_SKIN)),
            ui_base::DEFAULT_SKIN
        );
    }

    /// Итог доустановки в разделе говорит, что было на самом деле. Удача —
    /// сводка проверки ядра; отказ пароля и отмена — первая строка итога
    /// исполнителя, а не «не удалось: N» по командам, которых не запускали.
    #[test]
    fn completeness_note_says_what_happened() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let ok = dx::DeOpOutcome {
            ok: true,
            message: dx::completeness_done_message(),
            cancelled: false,
            restart_needed: false,
        };
        let fixed = vec![
            ("cosmic_wallpapers".to_string(), "fixed".to_string()),
            ("cosmic_keyring".to_string(), "ok".to_string()),
        ];
        assert_eq!(completeness_note_after(&ok, &fixed), qs::compliance_summary(&fixed));

        let denied = dx::DeOpOutcome {
            ok: false,
            message: dx::completeness_denied_message(),
            cancelled: false,
            restart_needed: false,
        };
        let refused = vec![("cosmic_wallpapers".to_string(), "failed".to_string())];
        let note = completeness_note_after(&denied, &refused);
        let head = dx::completeness_denied_message();
        let head = head.lines().next().unwrap();
        assert_eq!(note, format!("⚠️ {head}"), "{note}");
        assert!(!note.contains('\n'), "{note}");
        assert!(!note.contains(&qs::compliance_summary(&refused)), "{note}");

        let cancelled = dx::DeOpOutcome {
            ok: false,
            message: dx::cancelled_message(),
            cancelled: true,
            restart_needed: false,
        };
        assert_eq!(
            completeness_note_after(&cancelled, &refused),
            format!("⚠️ {}", dx::cancelled_message())
        );
        // Сама кнопка больше не отсылает в классическое окно.
        assert!(!completeness_internal_error().contains(CLASSIC_WINDOW_ENTRY));
    }

    /// Окно вопроса о восстановлении оформления говорит словами ядра, как
    /// Python-окно: «🛠 Восстановить» / «Не сейчас», синяя (не красная) кнопка
    /// согласия, ни имён пакетов, ни сборщиков в тексте.
    #[test]
    fn aeroshell_confirm_window_speaks_core_texts() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let view = aeroshell_confirm_view(&aeroshell::Report::empty());
        assert_eq!(view.phase, "confirm");
        assert_eq!(view.title, dp::aeroshell_title());
        assert_eq!(view.yes_text, dp::aeroshell_fix_button());
        assert_eq!(view.no_text, dp::aeroshell_skip_button());
        assert!(view.text.contains(&dp::aeroshell_text()), "{}", view.text);
        assert!(view.text.contains(&dp::aeroshell_informative()), "{}", view.text);
        assert!(view.text.contains("log out and log back in"), "{}", view.text);
        assert!(!view.danger);
        for jargon in ["aeroshell-", "pkexec", "paru", "yay", "AUR"] {
            assert!(!view.text.contains(jargon), "жаргон «{jargon}»: {}", view.text);
        }
    }

    /// Вне Plasma при запуске окно про AeroShell не всплывает; по просьбе
    /// человека (кнопка уведомления) — сверяем в любом сеансе.
    #[test]
    fn aeroshell_startup_survey_only_in_plasma() {
        assert!(startup_survey_wanted(true, false, true));
        assert!(!startup_survey_wanted(true, false, false), "MATE: молчим");
        assert!(!startup_survey_wanted(false, false, true), "выключено в настройках");
        assert!(startup_survey_wanted(false, true, false), "из уведомления — всегда");
        assert!(startup_survey_wanted(true, true, false));
    }

    /// «Проверить сейчас» и проверка при запуске больше не отсылают в
    /// классическое окно: ни одна из их функций не зовёт `blocked`.
    #[test]
    fn aeroshell_is_no_longer_a_border() {
        const SRC: &str = include_str!("settings_actions.rs");
        for head in [
            "fn aeroshell_check_now(mut self: Pin<&mut Self>)",
            "fn aeroshell_startup_check(mut self: Pin<&mut Self>)",
            "fn start_update_guard(self",
            "fn update_guard_done(",
            "fn start_aeroshell_survey(mut self",
            "fn aeroshell_survey_done(",
            "fn finish_aeroshell(self",
        ] {
            let start = SRC.find(head).unwrap_or_else(|| panic!("нет «{head}»"));
            let body = &SRC[start..];
            let end = body.find("\n    }\n").expect("конец функции не найден");
            assert!(!body[..end].contains("blocked("), "«{head}» отсылает в классическое окно");
        }
    }

    /// Отказ автозапуска обязан назвать причину, а не отправить человека в
    /// журнал: журнал покупатель не читает.
    #[test]
    fn autostart_failure_names_the_reason() {
        let line = format!(
            "{} {}",
            t("tray.autostart_failed"),
            t1("status.error", "нет прав на ~/.config/autostart")
        );
        assert!(line.contains("нет прав"), "причина потерялась: {line}");
        assert!(!t("tray.autostart_failed").is_empty());
    }
}
