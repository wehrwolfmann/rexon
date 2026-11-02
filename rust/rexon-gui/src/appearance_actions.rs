// AppearanceActions — объект побочных действий вкладки «Оформление» на Rust.
//
// ЗАЧЕМ. `AppearancePage.qml` ждёт свойством `actions` объект из семнадцати
// методов: снимок машины, состав раздела, картинки плиток и их отмена,
// фоновый прогрев, детали товара, кадры карусели, комментарии, подсказки
// поиска, установка из магазина и из AUR, применение темы с диска, удаление,
// звёздочка, страница в браузере и разовая чистка переводов. Пока такого
// объекта на Rust не было, раздел «Оформление» под Rust-хозяином окна
// открывался живым НА ВИД: боковая панель на месте, а витрина пуста и
// звёздочки не горят — спросить сеть и диск было не у кого.
//
// Договор со страницей взят у Python-класса `AppearanceActions`
// (`rexon/appearance/qml_window.py`) БУКВА В БУКВУ — те же имена методов, те
// же четырнадцать сигналов, те же строки JSON. Разметка страницы поэтому не
// изменилась ни на строку (кроме двух строк, которыми она заводит этот объект
// себе сама, — см. `qml/AppearancePage.qml`).
//
// ГДЕ ЖИВЁТ ЛОГИКА. Своей здесь нет вовсе:
//   * ЧТО показать и какими словами — `rexon_core::appearance_page`;
//   * МАГАЗИН — `rexon_core::ocs::OcsClient` поверх живого исполнителя
//     `rexon_core::ocs::LiveOcsIo` (сеть `net::UreqNet` + SQLite-кеш
//     `appearance_cache::LiveCache`); обе половины дописаны вместе с этим
//     объектом, потому что живого исполнителя `OcsCacheIo` в продукте не
//     было вовсе;
//   * AUR — `rexon_core::aur` (запросы к RPC уже были на Rust);
//   * ДИСК — `rexon_core::installed_scan::LiveScan`;
//   * УСТАНОВКА, ПРИМЕНЕНИЕ, УДАЛЕНИЕ — `rexon_core::appearance_installer::
//     LiveInstaller`;
//   * ИЗБРАННОЕ — `rexon_core::favorites`; НАСТРОЙКИ — `rexon_core::settings`.
//
// ⚠️ ЧТО СИНХРОННО, А ЧТО НЕТ, И ПОЧЕМУ ЭТО ВАЖНО. Синхронно отвечают только
// те пять методов, ответ которых страница читает ТОЙ ЖЕ строкой, которой
// позвала: `environment`, `remember`, `toggleFavorite`, `favorites`,
// `openUrl`. Всё остальное ходит в сеть или на диск и потому отвечает
// СИГНАЛОМ из своего потока — синхронный поход в сеть заморозил бы окно на
// время ответа магазина (замер оригинала: детальный ответ до 20 с).
//
// ⚠️ ОТМЕНА НАСТОЯЩАЯ, А НЕ «ВЫБРОСИТЬ ОТВЕТ». Раздел сменили — летящие
// запросы каталога и картинок РВУТСЯ по `net::CancelToken`, как рвал их
// `QNetworkReply::abort` у Python. Без этого шесть картинок прежнего раздела
// продолжали бы занимать канал, которого ждёт открытый (замер оригинала
// 31.07.2026: ответ с 0,95 с до 12,3 с).

/// Мост cxx-qt объекта действий.
///
/// Тип объявлен `#[qml_element]`, поэтому страница заводит его себе САМА
/// (`AppearanceActions { id: ownActions }`), а хозяин-Python по-прежнему
/// подставляет свой начальным свойством. Регистрации в `main_window.rs` для
/// этого не требуется: там карта разделов, а не список побочных объектов.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    // Доступные стили Qt: статический `QStyleFactory::keys()` зовётся
    // напрямую (cxx умеет статические методы), шима нет. QtWidgets крейт уже
    // линкует (лоток), процесс — QApplication.
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        include!(<QtWidgets/QStyleFactory>);
        type QStyleFactory;

        #[Self = "QStyleFactory"]
        fn keys() -> QStringList;
    }

    // Страница товара в браузере — общий шим крейта (`cpp/rexon_ext_dialogs.h`).
    // Второго объявления `rexon_ext_pick_file`/`rexon_ext_confirm` здесь нет
    // намеренно: cxx печатает шим по имени функции, и дубль дал бы
    // `duplicate symbol` на линковке.
    unsafe extern "C++" {
        include!("rexon_ext_dialogs.h");

        #[rust_name = "open_url_external"]
        fn rexon_ext_open_url(url: &QString) -> bool;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type AppearanceActions = super::AppearanceActionsRust;
    }

    unsafe extern "RustQt" {
        /// Состав раздела (строка JSON). Ответ с чужим ключом страница
        /// отбросит сама — ключ едет обратно нетронутым.
        /// На диске появился или пропал виджет Conky (`~/.conky`,
        /// `~/.conkyrc`, свои папки): раздел перечитывает установленное.
        #[qsignal]
        #[cxx_name = "conkyDiskChanged"]
        fn conky_disk_changed(self: Pin<&mut AppearanceActions>);

        /// Готовы новые снимки виджетов Conky для плиток
        /// (`conky_preview`): раздел перечитывает установленное.
        #[qsignal]
        #[cxx_name = "conkyPreviewsReady"]
        fn conky_previews_ready(self: Pin<&mut AppearanceActions>);

        #[qsignal]
        #[cxx_name = "catalogReady"]
        fn catalog_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Ответ фонового прогрева: поля запроса плюс `items`.
        #[qsignal]
        #[cxx_name = "prefetchReady"]
        fn prefetch_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Полные детали товара (строка JSON).
        #[qsignal]
        #[cxx_name = "detailReady"]
        fn detail_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// ВТОРОЙ проход панели деталей: переведённое описание (строка JSON —
        /// `itemId` и `translated`) вдогонку уже показанной карточке. Приходит
        /// только когда перевод действительно случился: нет модели — сигнала
        /// нет вовсе, и человек видит оригинал без единой жалобы.
        #[qsignal]
        #[cxx_name = "descriptionReady"]
        fn description_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Страница комментариев товара.
        #[qsignal]
        #[cxx_name = "commentsReady"]
        fn comments_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Карточка пакета AUR (строка JSON): паспорт RPC `info` —
        /// `{itemId, info}` (или `{itemId, missing}` / `{itemId, error}`).
        /// Картинки карточки приходят кадрами (`galleryReady`), комментарии —
        /// `commentsReady`, перевод описания — `descriptionReady`.
        #[qsignal]
        #[cxx_name = "aurDetailReady"]
        fn aur_detail_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Проверка источника пакета AUR (строка JSON): `{itemId, entry}` —
        /// запись `aur_trust::CacheEntry`, или `{itemId, failed}`.
        #[qsignal]
        #[cxx_name = "aurTrustReady"]
        fn aur_trust_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// PKGBUILD пакета (строка JSON): `{itemId, text}` или
        /// `{itemId, error}`.
        #[qsignal]
        #[cxx_name = "aurPkgbuildReady"]
        fn aur_pkgbuild_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// ВТОРОЙ проход страницы комментариев: переводы (строка JSON —
        /// `itemId`, `page` и `rows` с парами «оригинал → перевод») вдогонку
        /// уже показанному списку. В `rows` попадают ТОЛЬКО те комментарии,
        /// перевод которых действительно случился: русские и нечего-переводить
        /// в ответе не упоминаются вовсе, поэтому ссылка «показать оригинал»
        /// над ними и не появляется. Нет модели — нет и сигнала.
        #[qsignal]
        #[cxx_name = "commentTranslationsReady"]
        fn comment_translations_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Подсказки строки поиска.
        #[qsignal]
        #[cxx_name = "suggestReady"]
        fn suggest_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Картинка заявки записана в кеш по пути `path`.
        #[qsignal]
        #[cxx_name = "previewDone"]
        fn preview_done(self: Pin<&mut AppearanceActions>, job: QString, path: QString);

        /// Заявка не удалась: `status` — ответ сервера числом строкой, пусто
        /// (ответа не было) либо `canceled` (оборвали мы сами).
        #[qsignal]
        #[cxx_name = "previewFailed"]
        fn preview_failed(self: Pin<&mut AppearanceActions>, job: QString, status: QString);

        /// Готовый кадр карусели: номер строкой и путь на диске.
        #[qsignal]
        #[cxx_name = "galleryReady"]
        fn gallery_ready(self: Pin<&mut AppearanceActions>, index: QString, path: QString);

        /// Кадр не доехал.
        #[qsignal]
        #[cxx_name = "galleryFailed"]
        fn gallery_failed(self: Pin<&mut AppearanceActions>, index: QString);

        /// Обход ссылок карусели закончен: сколько кадров есть на самом деле.
        #[qsignal]
        #[cxx_name = "galleryFinished"]
        fn gallery_finished(self: Pin<&mut AppearanceActions>, count: QString);

        /// Доля выполненного установки, 0–100.
        #[qsignal]
        #[cxx_name = "installProgress"]
        fn install_progress(self: Pin<&mut AppearanceActions>, pct: i32);

        /// Очередная строка журнала установки.
        #[qsignal]
        #[cxx_name = "installLog"]
        fn install_log(self: Pin<&mut AppearanceActions>, text: QString);

        /// Сообщение в строку состояния.
        #[qsignal]
        #[cxx_name = "statusMessage"]
        fn status_message(self: Pin<&mut AppearanceActions>, text: QString);

        /// Итог установки/применения/удаления.
        #[qsignal]
        #[cxx_name = "actionDone"]
        fn action_done(self: Pin<&mut AppearanceActions>, text: QString, ok: bool);

        /// Тема с диска помечена к удалению (`removeLocalById`): текст
        /// всплывающей строки с «Вернуть». Плитку страница прячет пересканом.
        #[qsignal]
        #[cxx_name = "removalPending"]
        fn removal_pending(self: Pin<&mut AppearanceActions>, text: QString);

        /// Помеченные темы сняты (или снята системная тема по журналу):
        /// итог словами для строки состояния; страница перечитывает раздел.
        #[qsignal]
        #[cxx_name = "removalDone"]
        fn removal_done(self: Pin<&mut AppearanceActions>, text: QString);

        /// Итог «Установить из файла…» сверх `actionDone` (строка JSON
        /// `local_install::LocalOutcome`): какой раздел предложить открыть
        /// (`openKey`, `openText`) и перечитать ли раздел с диска (`reload`).
        #[qsignal]
        #[cxx_name = "localInstallDone"]
        fn local_install_done(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Очередь установок AUR изменилась: строка JSON
        /// `aur_install::AurQueue::snapshot_json` (`{jobs, status}`).
        #[qsignal]
        #[cxx_name = "aurJobsChanged"]
        fn aur_jobs_changed(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Ответ пробы экрана входа (строка JSON `{plasmaLogin, loginManager,
        /// steamos, canOpen}` и раздел «Менеджер входа»: `managers`,
        /// `backend`, `switchable`, `note`, `pending`, `rollback` —
        /// `system_exec::LoginScreenView`) — см. `probeLoginScreen`.
        #[qsignal]
        #[cxx_name = "loginScreenReady"]
        fn login_screen_ready(self: Pin<&mut AppearanceActions>, payload: QString);

        /// Итог «Сделать основным» (`makeDefaultLoginManager`): удалось ли и
        /// текст для человека — «вступит после перезагрузки» и как вернуть
        /// прежний, либо причина отказа. Следом приходит свежая проба
        /// (`loginScreenReady`).
        #[qsignal]
        #[cxx_name = "loginManagerSwitched"]
        fn login_manager_switched(self: Pin<&mut AppearanceActions>, ok: bool, message: QString);

        /// Итог действия блока «Фон и оформление экрана входа» (SDDM и
        /// Plasma Login Manager): фон, перенос оформления стола или
        /// «Сбросить». Следом приходит
        /// свежая проба (`loginScreenReady`).
        #[qsignal]
        #[cxx_name = "loginLookDone"]
        fn login_look_done(self: Pin<&mut AppearanceActions>, ok: bool, message: QString);

        /// Превью экрана входа открылось (`running`) или закрылось;
        /// `message` — почему не показалось (пусто — всё в порядке).
        #[qsignal]
        #[cxx_name = "loginPreviewChanged"]
        fn login_preview_changed(self: Pin<&mut AppearanceActions>, running: bool, message: QString);

        /// Итог действия полосы «Палитра оболочки» (передать, взять,
        /// вернуть): текст для человека и удалось ли.
        #[qsignal]
        #[cxx_name = "shellPaletteDone"]
        fn shell_palette_done(self: Pin<&mut AppearanceActions>, text: QString, ok: bool);
    }

    extern "RustQt" {
        /// Снимок машины, которого странице неоткуда взять: список Qt-стилей,
        /// действующий стиль приложений, есть ли помощник AUR, запущенный и
        /// запомненный рабочий стол. Строка JSON.
        #[qinvokable]
        fn environment(self: &AppearanceActions) -> QString;

        /// Запомнить выбранный рабочий стол между запусками. Пустой `seen` —
        /// «какой стол запущен сейчас, спроси у машины сам».
        #[qinvokable]
        fn remember(self: &AppearanceActions, de_id: &QString, seen: &QString) -> bool;

        /// Взяться за состав раздела. Ответ придёт сигналом `catalogReady`.
        #[qinvokable]
        #[cxx_name = "loadCatalog"]
        fn load_catalog(self: Pin<&mut AppearanceActions>, request: &QString);

        /// Прогреть кеш чужой страницы. Показывать её нечему.
        #[qinvokable]
        #[cxx_name = "loadPrefetch"]
        fn load_prefetch(self: Pin<&mut AppearanceActions>, request: &QString);

        /// Одна картинка по заявке очереди ядра. Ответ возвращается НОМЕРОМ
        /// заявки, а не адресом: живую и фоновую заявку по одному адресу
        /// иначе не различить.
        #[qinvokable]
        #[cxx_name = "loadPreview"]
        fn load_preview(
            self: Pin<&mut AppearanceActions>,
            job: &QString,
            item_id: &QString,
            url: &QString,
            low: bool,
        );

        /// Раздел сменили: летящее за картинками прежнего к делу не относится.
        #[qinvokable]
        #[cxx_name = "cancelPreviews"]
        fn cancel_previews(self: &AppearanceActions);

        /// Полные детали товара и кадры его карусели.
        #[qinvokable]
        #[cxx_name = "loadDetail"]
        fn load_detail(self: Pin<&mut AppearanceActions>, item_id: &QString);

        /// Отдельный кадр карусели — тот, что человек листнул сам.
        #[qinvokable]
        #[cxx_name = "loadGalleryFrame"]
        fn load_gallery_frame(
            self: Pin<&mut AppearanceActions>,
            index: &QString,
            url: &QString,
        );

        /// Страница комментариев товара с сайта магазина.
        #[qinvokable]
        #[cxx_name = "loadComments"]
        fn load_comments(self: Pin<&mut AppearanceActions>, item_id: &QString, page: &QString);

        /// Карточка пакета AUR: паспорт, картинки, перевод описания.
        #[qinvokable]
        #[cxx_name = "loadAurDetail"]
        fn load_aur_detail(self: Pin<&mut AppearanceActions>, pkg_name: &QString);

        /// Страница комментариев пакета AUR (разбор страницы сайта; ответ —
        /// `commentsReady` с полями `pinned`, `hasMore`, `blocked`,
        /// `comaintainers`).
        #[qinvokable]
        #[cxx_name = "loadAurComments"]
        fn load_aur_comments(self: Pin<&mut AppearanceActions>, pkg_name: &QString, page: &QString);

        /// PKGBUILD пакета AUR — показать внутри программы. `pkg_base` —
        /// ветка git пакета в AUR (база), `pkg_name` — для ответа.
        #[qinvokable]
        #[cxx_name = "loadAurPkgbuild"]
        fn load_aur_pkgbuild(self: Pin<&mut AppearanceActions>, pkg_name: &QString, pkg_base: &QString);

        /// Подсказки строки поиска: тот же поиск по магазину, но коротко.
        #[qinvokable]
        #[cxx_name = "loadSuggestions"]
        fn load_suggestions(self: Pin<&mut AppearanceActions>, request: &QString);

        /// Поставить тему из магазина (и применить, если просят).
        /// `file_name` — файл товара, выбранный руками в панели деталей
        /// (имя из `detailFiles`); пусто — автоматический выбор, тот же,
        /// что у плитки.
        #[qinvokable]
        #[cxx_name = "installById"]
        fn install_by_id(
            self: Pin<&mut AppearanceActions>,
            item_id: &QString,
            apply_after: bool,
            file_name: &QString,
        );

        /// Кнопка товара магазина (плитка и главная кнопка деталей), как в
        /// KDE: не стоит — только поставить; стоит — применить поставленное.
        /// Решает ядро (`ap::store_card_action`); итог — `actionDone`.
        #[qinvokable]
        #[cxx_name = "actOnStoreItem"]
        fn act_on_store_item(self: Pin<&mut AppearanceActions>, item_id: &QString);

        /// Собрать пакет AUR помощником — тем же, каким ставятся пакеты DE.
        /// Установка встаёт в ОБЩУЮ очередь приложения (по одной за раз);
        /// ход и итог — сигналом `aurJobsChanged`. Тот же вызов — «Повторить».
        #[qinvokable]
        #[cxx_name = "installAur"]
        fn install_aur(self: Pin<&mut AppearanceActions>, pkg_name: &QString);

        /// Очередь установок AUR сейчас (та же строка, что у
        /// `aurJobsChanged`). Заодно подписывает объект на её изменения:
        /// очередь живёт дольше страницы, и открытая заново страница
        /// подхватывает идущую установку.
        #[qinvokable]
        #[cxx_name = "aurJobs"]
        fn aur_jobs(self: Pin<&mut AppearanceActions>) -> QString;

        /// Открыть полный журнал установки пакета AUR.
        #[qinvokable]
        #[cxx_name = "openAurLog"]
        fn open_aur_log(self: &AppearanceActions, pkg_name: &QString) -> bool;

        /// «Установить из файла…»: `paths_json` — массив адресов из окна
        /// выбора (`file:///…`) или путей, `section` — ключ раздела, из
        /// которого ставят. Что это и куда — ядро (`local_install`); итог —
        /// `actionDone` и `localInstallDone`.
        #[qinvokable]
        #[cxx_name = "installFiles"]
        fn install_files(self: Pin<&mut AppearanceActions>, paths_json: &QString, section: &QString);

        /// Применить тему, которая уже лежит на диске.
        #[qinvokable]
        #[cxx_name = "applyLocalById"]
        fn apply_local_by_id(self: Pin<&mut AppearanceActions>, item_id: &QString);

        /// Окно «Применить «X»?» глобальной темы Plasma (строка JSON —
        /// `themes::GlobalQuestion` плюс `itemId`). Пусто — у этой плитки
        /// окна нет: не глобальная тема, ещё не установлена или старого
        /// формата, — кнопка ведёт себя как прежде.
        #[qinvokable]
        #[cxx_name = "globalApplyQuestion"]
        fn global_apply_question(self: &AppearanceActions, item_id: &QString) -> QString;

        /// Применить глобальную тему с выбором окна: `appearance` —
        /// оформление, `layout` — расположение рабочего стола и виджетов;
        /// `parts` — маска частей подробного вида («Выбрать, что
        /// применить…»), `-1` — простой вид. Итог — `actionDone`, как у
        /// `applyLocalById`.
        #[qinvokable]
        #[cxx_name = "applyGlobalById"]
        fn apply_global_by_id(
            self: Pin<&mut AppearanceActions>,
            item_id: &QString,
            appearance: bool,
            layout: bool,
            parts: i32,
        );

        /// Текущий размер курсора в этом окружении, 0 — никто не задавал.
        /// Быстро (файл или один `gsettings get`) — ответ синхронный.
        #[qinvokable]
        #[cxx_name = "cursorSize"]
        fn cursor_size(self: &AppearanceActions) -> i32;

        /// Сменить размер курсора у текущей темы. Итог — сигналом
        /// `actionDone`, как у применения темы.
        #[qinvokable]
        #[cxx_name = "applyCursorSize"]
        fn apply_cursor_size(self: Pin<&mut AppearanceActions>, size: i32);

        /// Удалить установленную тему. Товар магазина — по журналу
        /// установленных; тема с диска (`local:…`) — как `removeLocalById`.
        #[qinvokable]
        fn uninstall(self: Pin<&mut AppearanceActions>, item_json: &QString);

        /// «Удалить» у темы с диска (`theme_removal`): своя — пометить к
        /// удалению (сигнал `removalPending`, снимет `commitRemovals`);
        /// системная, поставленная Rexon, — сразу помощником; включённая и
        /// прочая системная — отказ с причиной (`actionDone`).
        #[qinvokable]
        #[cxx_name = "removeLocalById"]
        fn remove_local_by_id(self: Pin<&mut AppearanceActions>, item_id: &QString);

        /// Всплывающая строка закрылась или истекла: снять помеченное.
        /// Итог — `actionDone`, затем `removalDone`.
        #[qinvokable]
        #[cxx_name = "commitRemovals"]
        fn commit_removals(self: Pin<&mut AppearanceActions>);

        /// «Вернуть»: снять все пометки. Отдаёт, сколько тем вернулось.
        #[qinvokable]
        #[cxx_name = "undoRemovals"]
        fn undo_removals(self: &AppearanceActions) -> i32;

        /// Переключить звёздочку. Отдаёт АКТУАЛЬНЫЙ состав избранного, а не
        /// «получилось ли»: страница держит избранное у себя, и два вызова
        /// подряд разошлись бы с тем, что на диске.
        #[qinvokable]
        #[cxx_name = "toggleFavorite"]
        fn toggle_favorite(self: &AppearanceActions, item_id: &QString) -> QString;

        /// Состав избранного на открытии вкладки (звёздочки на плитках).
        #[qinvokable]
        fn favorites(self: &AppearanceActions) -> QString;

        /// Страница товара в браузере.
        #[qinvokable]
        #[cxx_name = "openUrl"]
        fn open_url(self: &AppearanceActions, url: &QString) -> bool;

        /// «Продемонстрировать» заставку KDE: `ksplashqml <тема> --test`, как
        /// у модуля KDE. Отдаёт ошибку для человека (пусто — показ пошёл);
        /// неудачу самой программы сообщает `statusMessage`.
        #[qinvokable]
        #[cxx_name = "demoSplash"]
        fn demo_splash(self: Pin<&mut AppearanceActions>, theme: &QString) -> QString;

        /// Включён ли автозапуск виджета Conky местной плитки `itemId`.
        #[qinvokable]
        #[cxx_name = "conkyAutostart"]
        fn conky_autostart(self: &AppearanceActions, item_id: &QString) -> bool;

        /// «Запускать при входе» у виджета Conky. Итог — сигналом
        /// `actionDone`; отдаёт состояние после переключения.
        #[qinvokable]
        #[cxx_name = "setConkyAutostart"]
        fn set_conky_autostart(self: Pin<&mut AppearanceActions>, item_id: &QString, on: bool) -> bool;

        /// Настройки автозапуска виджета Conky (JSON
        /// `conky_autostart::View`): задержка, «только здесь».
        #[qinvokable]
        #[cxx_name = "conkyAutostartOptions"]
        fn conky_autostart_options(self: &AppearanceActions, item_id: &QString) -> QString;

        /// Записать задержку и «только здесь». Итог — сигналом
        /// `actionDone`; отдаёт настройки заново.
        #[qinvokable]
        #[cxx_name = "setConkyAutostartOptions"]
        fn set_conky_autostart_options(
            self: Pin<&mut AppearanceActions>,
            item_id: &QString,
            delay: i32,
            only_here: bool,
        ) -> QString;

        /// Части виджета Conky плитки `itemId` — JSON `[{conf, label,
        /// running}]` (`conky_procs::parts`).
        #[qinvokable]
        #[cxx_name = "conkyParts"]
        fn conky_parts(self: &AppearanceActions, item_id: &QString) -> QString;

        /// Запустить или остановить одну часть виджета. Итог — сигналом
        /// `actionDone`; отдаёт части заново (JSON, как `conkyParts`).
        #[qinvokable]
        #[cxx_name = "pressConkyPart"]
        fn press_conky_part(self: Pin<&mut AppearanceActions>, item_id: &QString, conf: &QString) -> QString;

        /// Последние строки журнала виджета; пусто — Rexon его не запускал.
        #[qinvokable]
        #[cxx_name = "conkyLog"]
        fn conky_log(self: &AppearanceActions, item_id: &QString) -> QString;

        /// Настройки виджета Conky (файл `conf` плитки `itemId`) — JSON
        /// `conky_overlay::SettingsView`: значения, выбор, подписи.
        #[qinvokable]
        #[cxx_name = "conkySettings"]
        fn conky_settings(self: &AppearanceActions, item_id: &QString, conf: &QString) -> QString;

        /// Поменять одну настройку (`key` — поле выбора, `net:<имя>` —
        /// сеть; `value` — JSON, `null` — как у автора). Итог — сигналом
        /// `actionDone`; отдаёт настройки заново.
        #[qinvokable]
        #[cxx_name = "setConkySetting"]
        fn set_conky_setting(
            self: Pin<&mut AppearanceActions>,
            item_id: &QString,
            conf: &QString,
            key: &QString,
            value: &QString,
        ) -> QString;

        /// «Как у автора»: убрать накладку. Отдаёт настройки заново.
        #[qinvokable]
        #[cxx_name = "resetConkySettings"]
        fn reset_conky_settings(self: Pin<&mut AppearanceActions>, item_id: &QString, conf: &QString) -> QString;

        /// Подписи раздела «Виджеты Conky» (JSON `conky_procs::SectionTexts`).
        #[qinvokable]
        #[cxx_name = "conkyTexts"]
        fn conky_texts(self: &AppearanceActions) -> QString;

        /// Лента «На столе сейчас»: запущенные виджеты Conky — JSON
        /// `[{item_id, title, picture, confs}]` (`conky_procs::ribbon`).
        #[qinvokable]
        #[cxx_name = "conkyRibbon"]
        fn conky_ribbon(self: &AppearanceActions) -> QString;

        /// Крестик ленты: погасить виджет `itemId` (его запущенные файлы).
        /// Итог — сигналом `actionDone`.
        #[qinvokable]
        #[cxx_name = "stopConkyRibbonItem"]
        fn stop_conky_ribbon_item(self: Pin<&mut AppearanceActions>, item_id: &QString) -> bool;

        /// Наборы виджетов Conky — JSON `{texts, rows}`
        /// (`conky_sets::texts`, `conky_sets::rows`).
        #[qinvokable]
        #[cxx_name = "conkySets"]
        fn conky_sets(self: &AppearanceActions) -> QString;

        /// «Сохранить запущенные как набор» с именем `name`. Итог —
        /// сигналом `actionDone`; `true` — сохранён.
        #[qinvokable]
        #[cxx_name = "saveConkySet"]
        fn save_conky_set(self: Pin<&mut AppearanceActions>, name: &QString) -> bool;

        /// Применить набор: вопрос о командах виджетов (один на набор),
        /// затем запуск в фоне. Итог — сигналом `actionDone`.
        #[qinvokable]
        #[cxx_name = "applyConkySet"]
        fn apply_conky_set(self: Pin<&mut AppearanceActions>, name: &QString);

        /// Удалить свой набор после вопроса. Итог — сигналом `actionDone`.
        #[qinvokable]
        #[cxx_name = "removeConkySet"]
        fn remove_conky_set(self: Pin<&mut AppearanceActions>, name: &QString) -> bool;

        /// «Остановить все»: вопрос со списком, затем остановка по номерам
        /// процессов. Итог — сигналом `actionDone`; `false` — отказались или
        /// гасить нечего.
        #[qinvokable]
        #[cxx_name = "stopAllConky"]
        fn stop_all_conky(self: Pin<&mut AppearanceActions>) -> bool;

        /// Начать слежку за местами виджетов Conky (один раз на объект):
        /// перемена — сигналом `conkyDiskChanged`.
        #[qinvokable]
        #[cxx_name = "watchConkyDisk"]
        fn watch_conky_disk(self: Pin<&mut AppearanceActions>);

        /// Главный экран для невидимого экрана снимков виджетов Conky:
        /// физические точки и логическая плотность (dpi). На Wayland иначе
        /// его не узнать.
        #[qinvokable]
        #[cxx_name = "setConkyPreviewScreen"]
        fn set_conky_preview_screen(self: &AppearanceActions, width: i32, height: i32, dpi: i32);

        /// Разовая чистка протухших переводов: место в кеше, не более.
        #[qinvokable]
        #[cxx_name = "purgeTranslations"]
        fn purge_translations(self: Pin<&mut AppearanceActions>);

        /// Что рисует экран входа (Plasma Login Manager или нет), SteamOS ли
        /// это, есть ли чем открыть настройки экрана входа и какие экраны
        /// входа установлены (основной, работающий, можно ли сделать
        /// основным и почему нет). Только чтение. Ответ —
        /// сигналом `loginScreenReady`: проба может звать `systemctl`, и в
        /// потоке интерфейса ей не место.
        #[qinvokable]
        #[cxx_name = "probeLoginScreen"]
        fn probe_login_screen(self: Pin<&mut AppearanceActions>);

        /// Последний ответ пробы экрана входа (та же строка JSON, что у
        /// `loginScreenReady`), пусто — проба ещё не ответила. Страница
        /// грузится асинхронно, и сигнал успевал прийти раньше, чем её
        /// `Connections` подключался к объекту (живой стенд 28.09: ответ
        /// пробы был, до страницы не дошёл). Поэтому страница, подключившись,
        /// спрашивает ещё и так.
        #[qinvokable]
        #[cxx_name = "loginScreen"]
        fn login_screen(self: &AppearanceActions) -> QString;

        /// Открыть настройки экрана входа Plasma Login Manager. `false` —
        /// запустить было нечем.
        #[qinvokable]
        #[cxx_name = "openLoginScreenSettings"]
        fn open_login_screen_settings(self: &AppearanceActions) -> bool;

        /// «Сделать основным» экран входа `unit` (юнит без `.service`) с
        /// приветствием `greeter` (только LightDM; пусто — без выбора).
        /// В рабочем потоке: ядро заново проверяет всё по живой системе и
        /// отказывает до пароля, иначе один `pkexec` помощника `switch-dm`.
        /// Ответ — `loginManagerSwitched`. Пока идёт прежний вызов, новый
        /// отвергается тем же сигналом.
        #[qinvokable]
        #[cxx_name = "makeDefaultLoginManager"]
        fn make_default_login_manager(self: Pin<&mut AppearanceActions>, unit: &QString, greeter: &QString);

        /// «Сделать Plasma (Wayland) сеансом по умолчанию»: экран входа сам
        /// открывает или предлагает X11-сеанс `x11` (из таблицы ядра
        /// `system_plan::X11_SESSION_SUCCESSORS`) — перевести только ключ
        /// сеанса на его пару. В рабочем потоке, через сеанс прав окна;
        /// помощник от root проверяет всё сам. Ответ — `loginManagerSwitched`,
        /// следом свежая проба.
        #[qinvokable]
        #[cxx_name = "makeWaylandDefaultSession"]
        fn make_wayland_default_session(self: Pin<&mut AppearanceActions>, x11: &QString);

        /// Фон текущей темы SDDM — картинка `path` (пусто — вернуть фон самой
        /// темы), как «Изменить фон» модуля KDE; при Plasma Login Manager —
        /// фон его экрана входа через его помощник
        /// (`plasma_login::background_live`). В рабочем потоке, через
        /// сеанс прав окна (`system_plan::sddm_background_live`). Ответ —
        /// `loginLookDone`, следом свежая проба.
        #[qinvokable]
        #[cxx_name = "setLoginBackground"]
        fn set_login_background(self: Pin<&mut AppearanceActions>, path: &QString);

        /// Перенести оформление этого стола на экран входа SDDM или Plasma
        /// Login Manager (`reset` — «Сбросить»), как «Применить параметры
        /// Plasma» модулей KDE. Ответ —
        /// `loginLookDone`, следом свежая проба.
        #[qinvokable]
        #[cxx_name = "applyLoginLook"]
        fn apply_login_look(self: Pin<&mut AppearanceActions>, reset: bool);

        /// «Показывать часы» на экране входа (SDDM — у текущей темы, Plasma
        /// Login Manager — свой ключ), как галочка модулей KDE. Ответ —
        /// `loginLookDone`, следом свежая проба.
        #[qinvokable]
        #[cxx_name = "setLoginClock"]
        fn set_login_clock(self: Pin<&mut AppearanceActions>, show: bool);

        /// Живое превью экрана входа (тестовый режим приветствия, без
        /// выхода из сеанса): `token` — `sddm:<тема>` с плитки или пусто
        /// (то, что экран входа покажет сейчас). Отдаёт отказ для человека
        /// (пусто — показ пошёл); ход — `loginPreviewChanged`.
        #[qinvokable]
        #[cxx_name = "previewLoginScreen"]
        fn preview_login_screen(self: Pin<&mut AppearanceActions>, token: &QString) -> QString;

        /// Закрыть открытое превью экрана входа.
        #[qinvokable]
        #[cxx_name = "closeLoginPreview"]
        fn close_login_preview(self: &AppearanceActions);

        /// Подпись кнопки показа на плитке (`token` — знак `splashDemo`):
        /// «Продемонстрировать заставку» или «Предпросмотр» темы SDDM.
        #[qinvokable]
        #[cxx_name = "tileDemoLabel"]
        fn tile_demo_label(self: &AppearanceActions, token: &QString) -> QString;

        /// Подсказка той же кнопки.
        #[qinvokable]
        #[cxx_name = "tileDemoTooltip"]
        fn tile_demo_tooltip(self: &AppearanceActions, token: &QString) -> QString;

        /// Полоса «Палитра оболочки»: строка JSON готового решения ядра
        /// (`shell_palette::PaletteBanner`); `visible: false` — живого шелла
        /// нет. Синхронно: сбор — каталог сеанса, `/proc` и пара файлов.
        #[qinvokable]
        #[cxx_name = "shellPalette"]
        fn shell_palette(self: &AppearanceActions) -> QString;

        /// Запомнить, кто ведёт палитру (`rexon` / `shell`).
        #[qinvokable]
        #[cxx_name = "setPaletteLead"]
        fn set_palette_lead(self: &AppearanceActions, lead: &QString) -> bool;

        /// «Вернуть как было» раздела `section` по снимку «Применить»
        /// (`ap::revert_applied_section`; у цветов первой — памятка слоя).
        /// Ответ — строка JSON `{ok, text}`. Синхронно: только файлы
        /// профиля, без команд. Кеш сканов сбрасывается — бейдж «Активна»
        /// переезжает при перечитывании.
        #[qinvokable]
        #[cxx_name = "revertApplied"]
        fn revert_applied(self: &AppearanceActions, section: &QString) -> QString;

        /// «Передать палитру оболочке». Итог — сигналом `shellPaletteDone`.
        #[qinvokable]
        #[cxx_name = "pushShellPalette"]
        fn push_shell_palette(self: Pin<&mut AppearanceActions>);

        /// «Взять цвета оболочки». Итог — сигналом `shellPaletteDone`.
        #[qinvokable]
        #[cxx_name = "pullShellPalette"]
        fn pull_shell_palette(self: Pin<&mut AppearanceActions>);

        /// «Вернуть как было». Итог — сигналом `shellPaletteDone`.
        #[qinvokable]
        #[cxx_name = "revertShellPalette"]
        fn revert_shell_palette(self: Pin<&mut AppearanceActions>);
    }

    // Объявление трейта обязано быть В МОСТЕ: cxx-qt печатает по нему алиас
    // типа потока, без которого фоновая работа не доедет до сигнала.
    impl cxx_qt::Threading for AppearanceActions {}
}

use core::pin::Pin;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use cxx_qt::{CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::QString;
use serde_json::{json, Map, Value};

use rexon_core::appearance_installer::{self as ainst, LiveInstaller};
use rexon_core::appearance_page as ap;
use rexon_core::aur::{self, AurPackage};
use rexon_core::aur_install;
use rexon_core::categories;
use rexon_core::cinnamon_spices as cs;
use rexon_core::de_apply;
use rexon_core::favorites;
use rexon_core::i18n;
use rexon_core::installed_scan::{self as scan, LiveScan};
use rexon_core::models::ThemeItem;
use rexon_core::net::{CancelToken, UreqNet};
use rexon_core::ocs::{LiveOcsIo, OcsClient};
use rexon_core::session_splash::{self as ss, Splash};
use rexon_core::settings::SettingsManager;
use rexon_core::shell_palette as palette;
use rexon_core::splash_exec::LiveSplashIo;
use rexon_core::system;
use rexon_core::system_plan;
use rexon_core::theme_removal::{self as removal, LiveRemoval, RemovalQueue, Verdict};
use rexon_core::translator_engine as te;
use rexon_core::ui_base;
use rexon_core::utils;

/// Ответ объекта странице.
type Handle = CxxQtThread<qobject::AppearanceActions>;

/// Rust-состояние объекта действий.
///
/// Всё лежит за `Arc<Mutex<…>>`, и это не перестраховка: страница просит
/// действие ПО НОМЕРУ товара, а установщику нужен сам товар — значит то, что
/// объект однажды отдал наверх, читают потом рабочие потоки.
#[derive(Default)]
pub struct AppearanceActionsRust {
    state: Arc<State>,
}

#[derive(Default)]
struct State {
    /// Слежка за местами виджетов Conky уже идёт.
    conky_watch: std::sync::atomic::AtomicBool,
    /// Всё, что объект когда-либо отдал наверх (номер → товар).
    items: Mutex<HashMap<String, ThemeItem>>,
    /// То же для пакетов AUR: у них своя кнопка и свой установщик.
    packages: Mutex<HashMap<String, AurPackage>>,
    /// Кеш сканов диска на сессию: повторный вход в раздел мгновенный.
    /// Сбрасывается после удавшейся установки — иначе бейдж «✅ Активна»
    /// остался бы на прежней теме.
    scan_cache: Mutex<HashMap<String, Vec<ThemeItem>>>,
    /// Отметка каталогов тем, при которой снят [`State::scan_cache`]
    /// (`installed_scan::roots_stamp`): другая — кеш выбрасывается.
    scan_stamp: Mutex<String>,
    /// Список AUR приезжает целиком (страниц у RPC нет) — режем сами.
    aur_cache: Mutex<HashMap<String, Vec<AurPackage>>>,
    /// Помощник AUR, спрошенный один раз за сессию вкладки.
    aur_helper: Mutex<Option<Option<String>>>,
    /// Признаки отмены живых запросов каталога (ключ запроса → токен).
    catalog_cancels: Mutex<HashMap<String, CancelToken>>,
    /// То же для картинок (номер заявки → токен).
    preview_cancels: Mutex<HashMap<String, CancelToken>>,
    /// Сколько у товара комментариев — по ответу магазина.
    ///
    /// Отдельной картой, а не полем товара: `comment_count` живёт на
    /// `ocs::OcsContent`, а страница держит у себя `ThemeItem`, и через кеш
    /// каталога это число не проходит вовсе (колонки в базе нет). Без него
    /// ветка комментариев не знала бы, есть ли ещё страницы.
    comment_counts: Mutex<HashMap<String, i64>>,
    /// Открытый товар: кадры карусели закрытого не нужны никому.
    detail_id: Mutex<String>,
    /// Показываемый СПИСОК комментариев: товар и номер обхода списка.
    ///
    /// Номер растёт каждый раз, когда список строится заново (запрос ПЕРВОЙ
    /// страницы или другой товар), и не меняется от «Показать ещё»: страницы
    /// 1, 2, 3 дописываются к тому же списку и остаются на экране. Сторожу
    /// второго прохода (перевод комментариев) нужно именно это — «те же
    /// комментарии всё ещё показываются», — а не «последняя запрошенная
    /// страница». Считай иначе, и щелчок по «Показать ещё» отменял бы перевод
    /// страницы, которая никуда не делась.
    comments_view: Mutex<(String, u64)>,
    /// Последний ответ пробы экрана входа (`probeLoginScreen`).
    login_screen: Mutex<String>,
    /// Идёт «Сделать основным»: второй вызов до конца первого не пускаем.
    login_switching: std::sync::atomic::AtomicBool,
    /// Доступные стили Qt, снятые в потоке интерфейса (`environment`):
    /// скан «Стиля приложений» идёт в рабочем потоке и берёт их отсюда.
    qt_style_keys: Mutex<Vec<String>>,
    /// Карточки пакетов AUR на сессию: паспорт (`None` — AUR пакета не
    /// знает) с моментом ответа; старше `aur::DETAIL_CACHE_SECS` — спросить
    /// заново.
    aur_info: Mutex<HashMap<String, (std::time::Instant, Option<aur::AurInfo>)>>,
    /// Страницы пакетов (имя, номер страницы комментариев) → разбор.
    aur_pages: Mutex<HashMap<(String, i64), (std::time::Instant, aur::AurPage)>>,
    /// Найденные картинки карточек: метка (`github-readme:`/`og-image:`) →
    /// адрес (`None` — картинки нет).
    aur_images: Mutex<HashMap<String, Option<String>>>,
    /// Темы с диска, помеченные к удалению, пока открыта строка «Вернуть»
    /// (`theme_removal`): их плитки не попадают в ответы сканов.
    removals: Mutex<RemovalQueue>,
    /// Рабочий стол последнего скана диска: по нему снятие проверяет,
    /// не включена ли тема в разделе окружения.
    last_de: Mutex<String>,
}

/// Страница закрылась (или программа вышла), а строка «Вернуть» ещё висела:
/// снять помеченное, как сделала бы истёкшая строка, — человек видел
/// «удалена». Не дошло сюда (падение) — темы остались на месте.
impl Drop for State {
    fn drop(&mut self) {
        let pending = self.removals.get_mut().map(|q| q.take_all()).unwrap_or_default();
        if pending.is_empty() {
            return;
        }
        let de = self.last_de.get_mut().map(|d| d.clone()).unwrap_or_default();
        let (ok, msg) = removal::live_commit(pending, &de);
        rexon_core::log_info!("theme", "удаление на закрытии: {} — {msg}", if ok { "удалось" } else { "не всё" });
    }
}

// ============================================================================
// Синхронные ответы
// ============================================================================

impl qobject::AppearanceActions {
    fn environment(&self) -> QString {
        let state = self.rust().state.clone();
        // Список Qt-стилей — живой `QStyleFactory::keys()`, как у оригинала:
        // встроенные стили плюс установленные плагины. Пустой ответ ядро
        // заменяет запасом (`available_widget_styles`).
        let live_keys: Vec<String> = qobject::QStyleFactory::keys()
            .iter()
            .map(|k| k.to_string())
            .collect();
        let styles = scan::available_widget_styles(&live_keys);
        if let Ok(mut keys) = state.qt_style_keys.lock() {
            *keys = styles.clone();
        }

        let mut live = LiveScan::new(Vec::new(), HashMap::new());
        // Отсюда нужен только стиль приложений, а это поток интерфейса:
        // ответ про экран входа ставим заранее, чтобы карта активных тем не
        // пошла спрашивать `systemctl` (SDDM-тема отсюда не читается).
        live.plasma_login = Some(false);
        let widget_style = live
            .active_map()
            .get("widgetstyle")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let detected = system::detect_current_de(
            &env_var("XDG_CURRENT_DESKTOP"),
            &env_var("DESKTOP_SESSION"),
            !env_var("WAYLAND_DISPLAY").is_empty(),
        )
        .unwrap_or_default();

        let (saved, seen) = match SettingsManager::new() {
            Ok(s) => (
                s.get("appearance_de")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                s.get("appearance_de_seen")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
            Err(_) => (String::new(), String::new()),
        };

        let payload = json!({
            "qtStyles": styles,
            "widgetStyle": widget_style,
            "aurHelper": aur_helper(&state).is_some(),
            "detectedDe": detected,
            "savedDe": saved,
            "seenDe": seen,
        });
        // ⚠️ Решение «какой стол запомнить» принимает ядро, а исполняет его
        // объект — и ПОСЛЕ снятия снимка, чтобы ядро на стороне страницы
        // увидело прежние значения и приняло то же решение.
        let choice = ap::initial_de(&detected, &saved, &seen);
        if let Some((de_id, seen)) = choice.remember {
            remember_de(&de_id, &seen);
        }
        QString::from(&payload.to_string())
    }

    fn remember(&self, de_id: &QString, seen: &QString) -> bool {
        remember_de(&de_id.to_string(), &seen.to_string())
    }

    fn toggle_favorite(&self, item_id: &QString) -> QString {
        let id = item_id.to_string();
        let item = self
            .rust()
            .state
            .items
            .lock()
            .ok()
            .and_then(|m| m.get(&id).cloned());
        if let Some(item) = item {
            let path = favorites::default_path();
            let mut store = favorites::load_raw(&path);
            favorites::toggle(&mut store, &item);
            if let Err(e) = favorites::save_raw(&path, &store) {
                // Молчать нельзя: звёздочка на экране загорелась бы, не
                // пережив перезапуск.
                rexon_core::log_warn!("appearance", "избранное не сохранено: {e}");
            }
        }
        QString::from(&favorite_ids())
    }

    fn favorites(&self) -> QString {
        QString::from(&favorite_ids())
    }

    fn conky_autostart(&self, item_id: &QString) -> bool {
        let id = item_id.to_string();
        let name = ap::local_theme_name(&id, "");
        rexon_core::de_apply::conky_autostart(
            &rexon_core::animations::SystemIo,
            &rexon_core::de_apply::Ctx::live(),
            &name,
        )
    }

    fn set_conky_autostart(mut self: Pin<&mut Self>, item_id: &QString, on: bool) -> bool {
        let id = item_id.to_string();
        let name = ap::local_theme_name(&id, "");
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        if !on {
            let (ok, msg) = rexon_core::de_apply::set_conky_autostart(&io, &ctx, &name, false);
            self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
            return rexon_core::de_apply::conky_autostart(&io, &ctx, &name);
        }
        // При входе виджет запустится без Rexon — проверить его сейчас тем же
        // путём, что «▶️ Запустить»: разрешение на команды (вопрос — в потоке
        // окна), затем программы «всё или ничего» (установка — в фоне, без
        // вопроса). Не готов — автозапуск не включается, сказано почему;
        // переключатель перечитывается по `actionDone`.
        if ask_conky_safety(&name, None) == Some(false) {
            self.as_mut()
                .action_done(QString::from(&rexon_core::conky_safety::declined_text(&name)), false);
            return rexon_core::de_apply::conky_autostart(&io, &ctx, &name);
        }
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        let widget = name.clone();
        std::thread::spawn(move || {
            use rexon_core::conky_safety as cs;
            let name = widget;
            let note = match cs::live_prepare_start(std::slice::from_ref(&name), cs::Retry::Autostart).pop() {
                Some(Err(text)) => {
                    finish_action(&state, &handle, text, false);
                    return;
                }
                Some(Ok(note)) => note,
                None => None,
            };
            let io = rexon_core::animations::SystemIo;
            let ctx = rexon_core::de_apply::Ctx::live();
            let (ok, mut msg) = rexon_core::de_apply::set_conky_autostart(&io, &ctx, &name, true);
            if let Some(n) = note.filter(|_| ok) {
                msg = format!("{msg} {n}");
            }
            finish_action(&state, &handle, ap::action_result_status(ok, &msg), ok);
        });
        rexon_core::de_apply::conky_autostart(&io, &ctx, &name)
    }

    fn conky_autostart_options(&self, item_id: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let v = rexon_core::conky_autostart::widget_view(
            &rexon_core::animations::SystemIo,
            &rexon_core::de_apply::Ctx::live(),
            &name,
        );
        QString::from(&serde_json::to_string(&v).unwrap_or_default())
    }

    fn set_conky_autostart_options(mut self: Pin<&mut Self>, item_id: &QString, delay: i32, only_here: bool) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        let (ok, msg) = rexon_core::conky_autostart::set_widget(&io, &ctx, &name, delay.max(0) as u32, only_here);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        self.conky_autostart_options(item_id)
    }

    fn conky_parts(&self, item_id: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let parts = rexon_core::conky_procs::parts(
            &rexon_core::animations::SystemIo,
            &rexon_core::de_apply::Ctx::live(),
            &name,
        );
        QString::from(&serde_json::to_string(&parts).unwrap_or_else(|_| "[]".into()))
    }

    fn press_conky_part(mut self: Pin<&mut Self>, item_id: &QString, conf: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        if ask_conky_safety(&name, Some(&conf.to_string())) == Some(false) {
            let msg = rexon_core::conky_safety::declined_text(&name);
            self.as_mut().action_done(QString::from(&ap::action_result_status(false, &msg)), false);
            return self.conky_parts(item_id);
        }
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        let conf = conf.to_string();
        // «Всё или ничего», как у плитки: часть без нужных программ не
        // запускается. Пакеты отсюда не ставятся (кнопка части работает в
        // потоке интерфейса) — их поставит «Запустить» плитки.
        let starting = !rexon_core::conky_procs::running_confs(&io, &ctx).iter().any(|c| *c == conf);
        let (family, installable) = rexon_core::conky_safety::live_family();
        let h = rexon_core::conky_safety::health(&io, &ctx, &name, &family);
        if starting {
            if let Some(text) = rexon_core::conky_safety::part_refusal(&name, &h, installable) {
                self.as_mut().action_done(QString::from(&ap::action_result_status(false, &text)), false);
                return self.conky_parts(item_id);
            }
        }
        let (ok, mut msg) = rexon_core::conky_procs::press_part(&io, &ctx, &name, &conf);
        if ok && starting {
            if let Some(n) = rexon_core::conky_safety::files_note(&h) {
                msg = format!("{msg} {n}");
            }
        }
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        self.conky_parts(item_id)
    }

    fn conky_log(&self, item_id: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        QString::from(&rexon_core::conky_procs::log_tail(
            &rexon_core::animations::SystemIo,
            &rexon_core::de_apply::Ctx::live(),
            &name,
        ))
    }

    fn conky_settings(&self, item_id: &QString, conf: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let view = rexon_core::conky_overlay::live_settings_view(&name, &conf.to_string());
        QString::from(&serde_json::to_string(&view).unwrap_or_default())
    }

    fn set_conky_setting(
        mut self: Pin<&mut Self>,
        item_id: &QString,
        conf: &QString,
        key: &QString,
        value: &QString,
    ) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let value: Value = serde_json::from_str(&value.to_string()).unwrap_or(Value::Null);
        let (ok, msg) = rexon_core::conky_overlay::live_set(&name, &conf.to_string(), &key.to_string(), &value);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        self.conky_settings(item_id, conf)
    }

    fn reset_conky_settings(mut self: Pin<&mut Self>, item_id: &QString, conf: &QString) -> QString {
        let name = ap::local_theme_name(&item_id.to_string(), "");
        let (ok, msg) = rexon_core::conky_overlay::live_reset(&name, &conf.to_string());
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        self.conky_settings(item_id, conf)
    }

    fn conky_texts(&self) -> QString {
        QString::from(&serde_json::to_string(&rexon_core::conky_procs::section_texts()).unwrap_or_default())
    }

    fn conky_ribbon(&self) -> QString {
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        let extra = rexon_core::conky_scan::extra_dirs_for(&ctx);
        let r = rexon_core::conky_procs::ribbon(&io, &ctx, &extra);
        QString::from(&serde_json::to_string(&r).unwrap_or_else(|_| "[]".into()))
    }

    fn stop_conky_ribbon_item(mut self: Pin<&mut Self>, item_id: &QString) -> bool {
        use rexon_core::conky_procs;
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        let extra = rexon_core::conky_scan::extra_dirs_for(&ctx);
        let id = item_id.to_string();
        let Some(it) = conky_procs::ribbon(&io, &ctx, &extra).into_iter().find(|i| i.item_id == id) else {
            return false;
        };
        let (ok, msg) = conky_procs::stop_confs(&io, &ctx, &it.title, &it.confs);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        ok
    }

    fn conky_sets(&self) -> QString {
        let v = serde_json::json!({
            "texts": rexon_core::conky_sets::texts(),
            "rows": rexon_core::conky_sets::live_rows(),
        });
        QString::from(&v.to_string())
    }

    fn save_conky_set(mut self: Pin<&mut Self>, name: &QString) -> bool {
        let (ok, msg) = rexon_core::conky_sets::live_save(&name.to_string());
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        ok
    }

    fn apply_conky_set(mut self: Pin<&mut Self>, name: &QString) {
        use rexon_core::conky_sets as sets;
        let (plan, question) = match sets::live_plan(&name.to_string()) {
            Ok(x) => x,
            Err(why) => {
                self.as_mut().action_done(QString::from(&ap::action_result_status(false, &why)), false);
                return;
            }
        };
        // Вопрос о командах — в потоке окна, ДО фоновой работы; «Нет» —
        // набор не применяется вовсе (половина набора хуже, чем никакой).
        let trusted = match question {
            Some((title, text)) => {
                if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                    return;
                }
                true
            }
            None => false,
        };
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let (ok, msg) = sets::live_apply(&plan, trusted);
            finish_action(&state, &handle, msg, ok);
        });
    }

    fn remove_conky_set(mut self: Pin<&mut Self>, name: &QString) -> bool {
        let name = name.to_string();
        let (title, text) = rexon_core::conky_sets::remove_question(&name);
        if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
            return false;
        }
        let (ok, msg) = rexon_core::conky_sets::live_remove(&name);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        ok
    }

    fn stop_all_conky(mut self: Pin<&mut Self>) -> bool {
        use rexon_core::conky_procs;
        let io = rexon_core::animations::SystemIo;
        let ctx = rexon_core::de_apply::Ctx::live();
        let extra = rexon_core::conky_scan::extra_dirs_for(&ctx);
        let procs = conky_procs::all(&io);
        // Вопрос — со списком того, что будет остановлено; гасятся ровно
        // показанные номера (ядро ещё раз сверит, что это conky).
        let Some((title, text)) = conky_procs::stop_all_question(&ctx, &extra, &procs) else {
            let (ok, msg) = conky_procs::stop_all(&io, &[]);
            self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
            return false;
        };
        if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
            return false;
        }
        let pids: Vec<String> = procs.into_iter().map(|p| p.pid).collect();
        let (ok, msg) = conky_procs::stop_all(&io, &pids);
        self.as_mut().action_done(QString::from(&ap::action_result_status(ok, &msg)), ok);
        ok
    }

    fn watch_conky_disk(mut self: Pin<&mut Self>) {
        let state = self.as_ref().get_ref().rust().state.clone();
        if state.conky_watch.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let handle = self.as_mut().qt_thread();
        // Снимки виджетов для плиток — в фоне, на невидимом экране; готово —
        // сигнал разделу.
        rexon_core::conky_preview::enable();
        let preview_handle = handle.clone();
        rexon_core::conky_preview::watch(Box::new(move || {
            !matches!(
                preview_handle.queue(|mut o| o.as_mut().conky_previews_ready()),
                Err(cxx_qt::ThreadingQueueError::ObjectDestroyed)
            )
        }));
        let ctx = rexon_core::de_apply::Ctx::live();
        let targets = rexon_core::conky_scan::watch_targets(&ctx, &rexon_core::conky_scan::extra_dirs_for(&ctx));
        std::thread::spawn(move || {
            let Some(mut watch) = rexon_core::conky_scan::Watch::new(targets) else {
                rexon_core::log_warn!("appearance", "слежка за виджетами Conky не запущена: inotify недоступен");
                return;
            };
            loop {
                if !watch.wait(-1) {
                    continue;
                }
                // Распаковка темы — сотни записей подряд: сказать один раз,
                // после секунды тишины.
                while watch.wait(1000) {}
                if handle.queue(|mut o| o.as_mut().conky_disk_changed()).is_err() {
                    return; // страницы больше нет
                }
            }
        });
    }

    fn set_conky_preview_screen(&self, width: i32, height: i32, dpi: i32) {
        rexon_core::conky_preview::set_screen(width.max(0) as u32, height.max(0) as u32, dpi.max(0) as u32);
    }

    fn open_url(&self, url: &QString) -> bool {
        if url.to_string().is_empty() {
            return false;
        }
        qobject::open_url_external(url)
    }

    fn demo_splash(mut self: Pin<&mut Self>, theme: &QString) -> QString {
        use std::sync::atomic::{AtomicBool, Ordering};
        use rexon_core::appearance_installer as ainst;
        // Плитка темы SDDM: её кнопка — превью экрана входа.
        if theme.to_string().starts_with(ap::LOGIN_PREVIEW_PREFIX) {
            return self.preview_login_screen(theme);
        }
        // Один показ за раз, как у модуля KDE (кнопка ждёт конца показа).
        static RUNNING: AtomicBool = AtomicBool::new(false);
        let Some(argv) = ainst::splash_demo_argv(&theme.to_string()) else {
            return QString::default();
        };
        if RUNNING.swap(true, Ordering::SeqCst) {
            return QString::default();
        }
        let child = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn();
        let child = match child {
            Ok(c) => c,
            Err(e) => {
                RUNNING.store(false, Ordering::SeqCst);
                rexon_core::log_warn!("theme", "показ заставки: {e}");
                return QString::from(&if e.kind() == std::io::ErrorKind::NotFound {
                    ainst::splash_demo_missing()
                } else {
                    ainst::splash_demo_failed(&e.to_string())
                });
            }
        };
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let out = child.wait_with_output();
            RUNNING.store(false, Ordering::SeqCst);
            let failed = match out {
                Ok(o) if o.status.success() => None,
                Ok(o) => Some(String::from_utf8_lossy(&o.stderr).trim().to_string()),
                Err(e) => Some(e.to_string()),
            };
            if let Some(detail) = failed {
                rexon_core::log_warn!("theme", "показ заставки не удался: {detail}");
                let text = QString::from(&ainst::splash_demo_failed(&detail));
                let _ = handle.queue(move |mut o| o.as_mut().status_message(text));
            }
        });
        QString::default()
    }

    fn cancel_previews(&self) {
        let Ok(mut jobs) = self.rust().state.preview_cancels.lock() else {
            return;
        };
        for (_, token) in jobs.drain() {
            token.cancel();
        }
    }
}

// ============================================================================
// Долгая работа: каталог
// ============================================================================

impl qobject::AppearanceActions {
    fn load_catalog(mut self: Pin<&mut Self>, request: &QString) {
        let Ok(req) = serde_json::from_str::<Value>(&request.to_string()) else {
            return;
        };
        let key = s(&req, "key").to_string();
        let kind = s(&req, "kind").to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        // Запросы прежнего раздела больше не нужны — рвём их сразу, не
        // дожидаясь ответа: канал освобождается для того, что открыли.
        abort_catalog(&state, &key);

        let handle = self.as_mut().qt_thread();
        // Отменяемый транспорт заводится ДО потока: страница может сменить
        // раздел раньше, чем поток доберётся до первого запроса.
        let token = CancelToken::new();
        if let Ok(mut cancels) = state.catalog_cancels.lock() {
            cancels.insert(key.clone(), token.clone());
        }
        std::thread::spawn(move || {
            let answer = match kind.as_str() {
                "combined" => fetch_combined(&state, &req, &token),
                other => fetch_kind(&state, &req, &token, other),
            };
            if let Ok(mut cancels) = state.catalog_cancels.lock() {
                cancels.remove(&key);
            }
            post(&handle, move |mut object| {
                object.as_mut().catalog_ready(QString::from(&answer));
            });
        });
    }

    fn load_prefetch(mut self: Pin<&mut Self>, request: &QString) {
        let Ok(req) = serde_json::from_str::<Value>(&request.to_string()) else {
            return;
        };
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let mut answer = req.clone();
            let mut items: Vec<Value> = Vec::new();
            let io = LiveOcsIo::new(&home());
            let preview_dir = io.preview_dir();
            let mut client = OcsClient::new(io, &preview_dir);
            // Прогрев греет ту же страницу с добором, что откроет «Вперёд»:
            // все серверные страницы, из которых она набирается, лягут в кеш.
            let found = store_page(&mut client, &req).items;
            for c in &found {
                items.push(serde_json::to_value(&c.item).unwrap_or(Value::Null));
            }
            if let Some(map) = answer.as_object_mut() {
                map.insert("items".to_string(), Value::Array(items));
            }
            let payload = answer.to_string();
            post(&handle, move |mut object| {
                object.as_mut().prefetch_ready(QString::from(&payload));
            });
        });
    }
}

// ============================================================================
// Долгая работа: картинки
// ============================================================================

impl qobject::AppearanceActions {
    fn load_preview(
        mut self: Pin<&mut Self>,
        job: &QString,
        _item_id: &QString,
        url: &QString,
        low: bool,
    ) {
        let job = job.to_string();
        let url = url.to_string();
        if url.is_empty() {
            self.as_mut()
                .preview_failed(QString::from(&job), QString::default());
            return;
        }
        // Ядро спрашивает только о том, чего у него нет, но картинка могла
        // доехать по соседней заявке того же адреса.
        let dest = ap::preview_cache_path(&home(), &url);
        if std::path::Path::new(&dest).is_file() {
            self.as_mut()
                .preview_done(QString::from(&job), QString::from(&dest));
            return;
        }

        let state = self.as_ref().get_ref().rust().state.clone();
        let token = CancelToken::new();
        if let Ok(mut jobs) = state.preview_cancels.lock() {
            jobs.insert(job.clone(), token.clone());
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // ⚠️ Низкий приоритет фоновой заявки (`low`) здесь НЕ выражен, и
            // это осознанно: у Python его задавал
            // `QNetworkRequest::LowPriority` — свойство очереди одного
            // `QNetworkAccessManager`, которого в Rust-пути нет. Заявка
            // остаётся честной: сколько картинок лететь разом и в каком
            // порядке, всё равно решает очередь ядра (`PREVIEW_MAX_INFLIGHT`
            // и `PREVIEW_MAX_INFLIGHT_LOW` — два разных потолка), а
            // приоритет внутри уже отобранной пачки её решения не меняет.
            let _ = low;
            let net = UreqNet::new().with_cancel(token.clone());
            let io = LiveOcsIo::with_net(net, &home());
            let preview_dir = io.preview_dir();
            let mut client = OcsClient::new(io, &preview_dir);
            let outcome = client
                .get_preview_image(&url)
                .filter(|p| usable_preview(p, &preview_dir));
            if let Ok(mut jobs) = state.preview_cancels.lock() {
                jobs.remove(&job);
            }
            let cancelled = token.is_cancelled();
            post(&handle, move |mut object| {
                match outcome {
                    Some(path) => object.as_mut().preview_done(
                        QString::from(&job),
                        QString::from(&path),
                    ),
                    // Отмену очередь ядра НЕ повторяет: раздел сменили мы
                    // сами, и повтор потянул бы картинки прежнего раздела.
                    None if cancelled => object
                        .as_mut()
                        .preview_failed(QString::from(&job), QString::from("canceled")),
                    None => object
                        .as_mut()
                        .preview_failed(QString::from(&job), QString::default()),
                }
            });
        });
    }
}

// ============================================================================
// Долгая работа: панель деталей
// ============================================================================

impl qobject::AppearanceActions {
    fn load_detail(mut self: Pin<&mut Self>, item_id: &QString) {
        let id = item_id.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        let Some(item) = state.items.lock().ok().and_then(|m| m.get(&id).cloned()) else {
            return;
        };
        if let Ok(mut open) = state.detail_id.lock() {
            *open = id.clone();
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            fetch_detail(&state, &handle, item);
        });
    }

    fn load_gallery_frame(mut self: Pin<&mut Self>, index: &QString, url: &QString) {
        let index = index.to_string();
        let url = url.to_string();
        if url.is_empty() {
            self.as_mut().gallery_failed(QString::from(&index));
            return;
        }
        if index.parse::<i64>().is_err() {
            return;
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let io = LiveOcsIo::new(&home());
            let preview_dir = io.preview_dir();
            let mut client = OcsClient::new(io, &preview_dir);
            emit_frame(&handle, &mut client, &index, &url);
        });
    }

    /// Страница комментариев: сначала САМ СПИСОК, потом переводы вдогонку.
    ///
    /// ⭐ Порядок тот же, что у панели деталей: показ не ждёт ничего, что можно
    /// дослать. Python переводил комментарии ДО отправки списка наверх, по
    /// одному, и человек всё это время смотрел на пустую ветку.
    fn load_comments(mut self: Pin<&mut Self>, item_id: &QString, page: &QString) {
        let id = item_id.to_string();
        let page: i64 = page.to_string().parse().unwrap_or(0);
        let state = self.as_ref().get_ref().rust().state.clone();
        // Обход списка помечается ЗДЕСЬ, в потоке интерфейса, а не в рабочем:
        // иначе два запроса подряд («Показать ещё» дважды) могли бы
        // перенумеровать друг друга в обратном порядке.
        let view = mark_comments_view(&state, &id, page);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let io = LiveOcsIo::new(&home());
            let preview_dir = io.preview_dir();
            let mut client = OcsClient::new(io, &preview_dir);
            // `None` — раздела комментариев у товара нет вовсе; страница в
            // этом случае не должна показывать пустую ветку.
            let Some(comments) = client.comments(&id, page, ap::COMMENTS_PAGESIZE as i64) else {
                return;
            };
            let mut rows: Vec<Value> = Vec::with_capacity(comments.len());
            for c in &comments {
                rows.push(json!({
                    "author": c.author,
                    "date": c.date,
                    "subject": c.subject,
                    "text": c.text,
                    "score": c.score,
                    "depth": c.depth,
                    // Перевод ЗДЕСЬ пустой намеренно: пустая строка означает
                    // «показывай оригинал», и список от перевода не ждёт
                    // ничего. Переводы догоняют его вторым проходом
                    // (`translate_comment_page` ниже) и правят уже показанные
                    // строки.
                    "translated": "",
                }));
            }
            let mut answer = Map::new();
            answer.insert("itemId".to_string(), Value::from(id.as_str()));
            answer.insert("page".to_string(), Value::from(page));
            answer.insert("comments".to_string(), Value::Array(rows));
            let count = state
                .comment_counts
                .lock()
                .ok()
                .and_then(|m| m.get(&id).copied())
                .unwrap_or(0);
            if count > 0 {
                answer.insert("total".to_string(), Value::from(count));
            }
            let payload = Value::Object(answer).to_string();
            post(&handle, move |mut object| {
                object.as_mut().comments_ready(QString::from(&payload));
            });

            let texts: Vec<String> = comments.iter().map(|c| c.text.clone()).collect();
            translate_comment_page(&state, &handle, &id, page, view, &texts);
        });
    }

    fn load_suggestions(mut self: Pin<&mut Self>, request: &QString) {
        let Ok(req) = serde_json::from_str::<Value>(&request.to_string()) else {
            return;
        };
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let io = LiveOcsIo::new(&home());
            let preview_dir = io.preview_dir();
            let mut client = OcsClient::new(io, &preview_dir);
            let (found, _total) = client.search(
                s(&req, "categoryId"),
                num(&req, "page"),
                i64::from(ap::PAGE_SIZE),
                s(&req, "sort"),
                s(&req, "query"),
                true,
            );
            let items: Vec<ThemeItem> = found.iter().map(|c| c.item.clone()).collect();
            remember_items(&state, &items);
            let payload = json!({
                "generation": num(&req, "generation"),
                "items": items,
            })
            .to_string();
            post(&handle, move |mut object| {
                object.as_mut().suggest_ready(QString::from(&payload));
            });
        });
    }
}

// ============================================================================
// Долгая работа: установка, применение, удаление
// ============================================================================

impl qobject::AppearanceActions {
    fn install_by_id(mut self: Pin<&mut Self>, item_id: &QString, apply_after: bool, file_name: &QString) {
        let id = item_id.to_string();
        let file_name = file_name.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        let Some(item) = state.items.lock().ok().and_then(|m| m.get(&id).cloned()) else {
            self.as_mut().action_done(QString::default(), false);
            return;
        };
        // Слот заставки при входе один на всех — спор за него разбирается ДО
        // фоновой работы: диалог живёт в потоке интерфейса.
        let (apply_after, keep_video, note) = if apply_after {
            match splash_slot_answer(&item) {
                // Место остаётся за роликом. Установку при этом НЕ отменяем:
                // покупатель просил тему себе, отказали мы только в
                // применении — и сказали об этом словами.
                ss::SplashSlotPlan::Stop(text) => (false, None, text),
                ss::SplashSlotPlan::Apply { keep_video, note } => (true, keep_video, note),
            }
        } else {
            (false, None, String::new())
        };
        // Виджет Conky без самой программы: она ставится вместе с виджетом
        // без вопроса (защита — окно пароля). Не встала — виджет ставится,
        // но не запускается.
        let conky = apply_after && conky_needs_install(&item);
        // Файл товара — 7z, а распаковать нечем: bsdtar ставится без вопроса
        // (защита — окно пароля). Не встал — товар не ставится.
        let seven_zip = seven_zip_needs_install(&item, &file_name);
        rexon_core::log_info!(
            "theme",
            "установка «{}» (id {}, категория {}){}",
            item.name,
            item.id,
            item.category_id,
            if apply_after { " с применением" } else { "" }
        );
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            if seven_zip {
                if let Err(why) = rexon_core::appearance_installer::install_seven_zip_tool_live() {
                    rexon_core::log_warn!("theme", "bsdtar для «{}» не встал: {why}", item.name);
                    finish_action(&state, &handle, rexon_core::appearance_installer::seven_zip_install_failed(&why), false);
                    return;
                }
            }
            let mut apply_after = apply_after;
            let mut conky_note = None;
            if conky {
                if let Err(why) = de_apply::install_conky_live() {
                    apply_after = false;
                    conky_note = Some(de_apply::conky_install_failed(&why));
                }
            }
            let (mut msg, ok) = install_theme(&handle, item, apply_after, &file_name);
            finish_splash_slot(keep_video, &note, ok, &mut msg);
            if let Some(line) = conky_note {
                msg = format!("{msg}. ⚠️ {line}");
            }
            finish_action(&state, &handle, msg, ok);
        });
    }

    fn install_aur(mut self: Pin<&mut Self>, pkg_name: &QString) {
        let name = pkg_name.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        // Что ставить — решает ядро (`aur_install::plan_install`): пакет-форк
        // вроде AeroThemePlasma ставится комплектом и только после
        // предупреждения. Вопрос — синхронно, в потоке интерфейса (диалог
        // живёт здесь); список установленного нужен только комплекту.
        let plan = if aur_install::kit_for(&name).is_some() {
            let have = installed_packages();
            aur_install::plan_install(&name, &|p| have.contains(p))
        } else {
            aur_install::plan_install(&name, &|_| false)
        };
        let Some(helper) = aur_helper(&state) else {
            self.as_mut().action_done(QString::from(&ap::aur_no_builder()), false);
            return;
        };
        if let Some(question) = &plan.question {
            let yes = crate::extensions_actions::ask_confirm(
                &QString::from(&question.title),
                &QString::from(&question.text),
            );
            if !yes {
                return;
            }
        }
        let handle = self.as_mut().qt_thread();
        aur_subscribe(handle, state);
        let log_path = aur_install::log_path(&home(), &name);
        let queued = match aur_runner().queue.lock() {
            Ok(mut q) => q.enqueue(&name, plan, &helper, &log_path),
            Err(_) => return,
        };
        rexon_core::log_info!(
            "aur",
            "в очередь: «{name}» помощником {helper}{}",
            if queued.start_worker { ", очередь была пуста" } else { "" }
        );
        aur_broadcast();
        if queued.start_worker {
            std::thread::spawn(aur_worker);
        }
    }

    fn aur_jobs(mut self: Pin<&mut Self>) -> QString {
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        aur_subscribe(handle, state);
        QString::from(&aur_snapshot())
    }

    fn open_aur_log(&self, pkg_name: &QString) -> bool {
        let path = aur_install::log_path(&home(), &pkg_name.to_string());
        if !std::path::Path::new(&path).is_file() {
            return false;
        }
        qobject::open_url_external(&QString::from(&format!("file://{path}")))
    }

    /// Поставить (обновить) спайс Cinnamon: скачать с сайта, проверить,
    /// разложить — в потоке, с полосой хода.
    fn install_spice(mut self: Pin<&mut Self>, item: ThemeItem) {
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let progress_handle = handle.clone();
            let progress: Box<dyn FnMut(i64, i64) + Send> = Box::new(move |done, total| {
                if total > 0 {
                    let pct = (done * 100 / total) as i32;
                    post(&progress_handle, move |mut object| {
                        object.as_mut().install_progress(pct);
                    });
                }
            });
            rexon_core::log_info!("theme", "установка из Cinnamon Spices: {}", item.id);
            let (ok, msg) = cs::live_install(&item, log_sink(&handle), Some(progress));
            finish_action(&state, &handle, msg, ok);
        });
    }

    fn install_files(mut self: Pin<&mut Self>, paths_json: &QString, section: &QString) {
        let paths: Vec<String> = serde_json::from_str(&paths_json.to_string()).unwrap_or_default();
        let section = section.to_string();
        if paths.is_empty() {
            self.as_mut().action_done(QString::default(), false);
            return;
        }
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        rexon_core::log_info!("theme", "установка из файла в раздел «{section}»: {} шт.", paths.len());
        std::thread::spawn(move || {
            let outcome = rexon_core::local_install::live_install(&paths, &section, log_sink(&handle));
            let payload = serde_json::to_string(&outcome).unwrap_or_default();
            // Часть файлов встала, часть нет: итог «не удалось», но новые
            // темы на диске уже есть — кеш сканов всё равно сбросить.
            if outcome.installed && !outcome.ok {
                if let Ok(mut cache) = state.scan_cache.lock() {
                    cache.clear();
                }
            }
            finish_action(&state, &handle, outcome.message.clone(), outcome.ok);
            post(&handle, move |mut object| {
                object.as_mut().local_install_done(QString::from(&payload));
            });
        });
    }

    fn apply_local_by_id(mut self: Pin<&mut Self>, item_id: &QString) {
        let id = item_id.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        let Some(item) = state.items.lock().ok().and_then(|m| m.get(&id).cloned()) else {
            self.as_mut().action_done(QString::default(), false);
            return;
        };
        let name = ap::local_theme_name(&item.id, &item.name);
        self.apply_item(item, name);
    }

    fn global_apply_question(&self, item_id: &QString) -> QString {
        let id = item_id.to_string();
        let item = self.rust().state.items.lock().ok().and_then(|m| m.get(&id).cloned());
        let Some((item, name)) = item.and_then(|item| global_target(&item).map(|n| (item, n))) else {
            return QString::default();
        };
        let display = ap::card_name(&item.name);
        let display = if display.is_empty() { name.as_str() } else { display };
        let io = rexon_core::animations::SystemIo;
        // Вне Plasma окно говорит, какие части переносятся на этот стол.
        let de = LiveInstaller::new("", Box::new(|_: &str| {})).de_id().to_string();
        match rexon_core::themes::global_question_live(&io, &home(), &name, display, &de) {
            Some(q) => {
                let mut v = serde_json::to_value(q).unwrap_or_default();
                v["itemId"] = json!(id);
                QString::from(&v.to_string())
            }
            None => QString::default(),
        }
    }

    fn apply_global_by_id(mut self: Pin<&mut Self>, item_id: &QString, appearance: bool, layout: bool, parts: i32) {
        let id = item_id.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        let item = state.items.lock().ok().and_then(|m| m.get(&id).cloned());
        let Some((item, name)) = item.and_then(|item| global_target(&item).map(|n| (item, n))) else {
            self.as_mut().action_done(QString::default(), false);
            return;
        };
        let parts = u32::try_from(parts).ok();
        let choice = rexon_core::themes::GlobalChoice { appearance, layout, parts };
        self.apply_item_with(item, name, Some(choice));
    }

    fn act_on_store_item(mut self: Pin<&mut Self>, item_id: &QString) {
        let id = item_id.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        let Some(item) = state.items.lock().ok().and_then(|m| m.get(&id).cloned()) else {
            self.as_mut().action_done(QString::default(), false);
            return;
        };
        if item.id.starts_with(cs::ID_PREFIX) {
            // Тема Spices в разделе GTK или рамок окон: стоит и не устарела —
            // применить тем же путём, что тему с диска (тема GTK, рамки
            // xfwm4 или Marco — по разделу карточки).
            if let Some(name) = cs::live_part_apply_name(&item) {
                self.apply_item(item, name);
                return;
            }
            self.install_spice(item);
            return;
        }
        let home = home();
        let installed = ap::live_logged_theme_name(&home, &item.id);
        let can_apply = ap::store_item_can_apply(std::path::Path::new(&home), &item);
        match (ap::store_card_action(installed.is_some(), can_apply), installed) {
            (ap::StoreCardAction::ApplyInstalled, Some(name)) => self.apply_item(item, name),
            _ => self.install_by_id(item_id, false, &QString::default()),
        }
    }

    /// Применить тему, которая уже на диске (`name` — имя каталога/файла):
    /// местная плитка и поставленный товар магазина — один путь, с теми же
    /// вопросами (заставка, значки и курсор GTK-темы, программа Conky).
    fn apply_item(self: Pin<&mut Self>, item: ThemeItem, name: String) {
        self.apply_item_with(item, name, None);
    }

    /// [`Self::apply_item`] с ответом окна «Применить «X»?» глобальной темы
    /// (`None` — окна не было).
    fn apply_item_with(
        mut self: Pin<&mut Self>,
        item: ThemeItem,
        name: String,
        global: Option<rexon_core::themes::GlobalChoice>,
    ) {
        let state = self.as_ref().get_ref().rust().state.clone();
        // Отказ «тема старого формата» мгновенный и синхронный: пока он ходил
        // через фоновую работу, та успевала умереть раньше доставки, и
        // человек не видел причины вовсе.
        if let Some(name) = stale_global(&item, &name) {
            self.as_mut()
                .action_done(QString::from(&ap::stale_global_msg(&name)), false);
            return;
        }
        // Тот же спор за слот заставки, что и при установке из магазина, и по
        // той же причине синхронный. Здесь у «оставить ролик» второго исхода
        // нет: тема уже на диске, применение — единственное, о чём просили.
        let (keep_video, note) = match splash_slot_answer(&item) {
            ss::SplashSlotPlan::Stop(text) => {
                self.as_mut().action_done(QString::from(&text), false);
                return;
            }
            ss::SplashSlotPlan::Apply { keep_video, note } => (keep_video, note),
        };
        // Тема рассчитана на свои значки: спросить один раз, включать ли их
        // (решение 3а; ответ запоминается для темы). «Оставить мои» — тема
        // применяется целиком, кроме значков.
        let probe = LiveInstaller::new("", Box::new(|_: &str| {}));
        let icons_choice = probe.theme_icons_question(&item, &name).map(|q| {
            let yes = crate::extensions_actions::ask_confirm_labeled(&q.title, &q.text, &q.yes, &q.no);
            rexon_core::theme_watch::remember_icons_choice(&name, yes);
            yes
        });
        // Виджет Conky без самой программы: она ставится без вопроса (защита
        // — окно пароля). Не встала — виджет не запускается, и сказано почему.
        let conky = conky_needs_install(&item);
        // Виджет выполняет команды или сценарии: показать, что именно, и
        // запускать только с разрешения (`conky_safety`).
        if de_apply::is_conky_item(&item) && ask_conky_safety(&name, None) == Some(false) {
            self.as_mut()
                .action_done(QString::from(&rexon_core::conky_safety::declined_text(&name)), false);
            return;
        }
        // Чего виджету не хватает: программы с пакетом из репозиториев
        // ставятся без вопроса (защита — окно пароля polkit); чего не
        // поставить — виджет не запускается.
        let check_programs = de_apply::is_conky_item(&item) && conky_will_start(&name);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            if conky {
                if let Err(why) = de_apply::install_conky_live() {
                    finish_action(&state, &handle, de_apply::conky_install_failed(&why), false);
                    return;
                }
            }
            // «Всё или ничего»: недостающие программы из репозиториев ставятся
            // сами (защита — окно пароля), не встали или ставить их Rexon не
            // может — виджет не запускается, а в итоге сказано, что сделать.
            // Общий путь с «Применить» набора (`conky_safety::prepare_start`).
            let mut files_note = None;
            if check_programs {
                use rexon_core::conky_safety as cs;
                match cs::live_prepare_start(std::slice::from_ref(&name), cs::Retry::Tile).pop() {
                    Some(Err(text)) => {
                        finish_action(&state, &handle, text, false);
                        return;
                    }
                    Some(Ok(note)) => files_note = note,
                    None => {}
                }
            }
            let logger = log_sink(&handle);
            let mut installer = LiveInstaller::new("", logger);
            if icons_choice.is_some() {
                installer.set_icons_choice(icons_choice);
            }
            installer.set_global_choice(global);
            // GTK-тема идёт со своим курсором (и значками, если выбрано их
            // включить), а их нет: ставятся без вопроса (защита — окно
            // пароля). Не встали — тема не применяется, и сказано почему.
            if let Err(why) = installer.prepare_theme_deps(&item, &name) {
                rexon_core::log_warn!("theme", "наборы темы «{name}» не встали: {why}");
                finish_action(&state, &handle, why, false);
                return;
            }
            rexon_core::log_info!("theme", "применение «{name}» (id {}, вид {:?})", item.id, item.local_kind);
            let (ok, mut msg) = installer.apply(&item, &name);
            finish_splash_slot(keep_video, &note, ok, &mut msg);
            if ok {
                add_shell_palette_note(&item, &mut msg);
                if let Some(n) = files_note {
                    msg = format!("{msg} {n}");
                }
            }
            finish_action(&state, &handle, msg, ok);
        });
    }

    fn cursor_size(&self) -> i32 {
        let installer = LiveInstaller::new("", Box::new(|_: &str| {}));
        installer.cursor_size().map(|n| n as i32).unwrap_or(0)
    }

    fn apply_cursor_size(mut self: Pin<&mut Self>, size: i32) {
        let Ok(size) = u32::try_from(size) else {
            return;
        };
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let logger = log_sink(&handle);
            let mut installer = LiveInstaller::new("", logger);
            let (ok, msg) = installer.apply_cursor_size(size);
            finish_action(&state, &handle, msg, ok);
        });
    }

    fn uninstall(mut self: Pin<&mut Self>, item_json: &QString) {
        let Ok(raw) = serde_json::from_str::<Value>(&item_json.to_string()) else {
            return;
        };
        let id = s(&raw, "id").to_string();
        if id.is_empty() {
            return;
        }
        // Тема с диска в журнале установленных не числится: её «Удалить» —
        // по пути из скана.
        if id.starts_with("local:") {
            self.remove_local_by_id(&QString::from(&id));
            return;
        }
        // В папке покупки лежат отдельно поставленные части (рамка окон в
        // теме GTK) — удаление унесёт и их: спросить.
        if let Some((title, text)) = removal::live_uninstall_question(&id) {
            if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                return;
            }
        }
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        spawn_uninstall(state, handle, id, false);
    }

    fn remove_local_by_id(mut self: Pin<&mut Self>, item_id: &QString) {
        let id = item_id.to_string();
        let state = self.as_ref().get_ref().rust().state.clone();
        let Some(item) = state.items.lock().ok().and_then(|m| m.get(&id).cloned()) else {
            self.as_mut().action_done(QString::default(), false);
            return;
        };
        let live = LiveRemoval::new();
        match live.verdict(&item) {
            Verdict::Local { path } => {
                // Папка темы с отдельно поставленной рамкой внутри — спросить.
                if let Some((title, text)) = live.separate_parts_question(&path, &item.name) {
                    if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                        return;
                    }
                }
                rexon_core::log_info!("theme", "к удалению: {path}");
                let text = match state.removals.lock() {
                    Ok(mut queue) => queue.mark(&path, &item.name, &item.id),
                    Err(_) => return,
                };
                self.as_mut().removal_pending(QString::from(&text));
            }
            Verdict::Journal { content_id } => {
                let handle = self.as_mut().qt_thread();
                spawn_uninstall(state, handle, content_id, true);
            }
            Verdict::Loose { path } => {
                let handle = self.as_mut().qt_thread();
                std::thread::spawn(move || {
                    let logger = log_sink(&handle);
                    let mut installer = LiveInstaller::new("", logger);
                    rexon_core::log_info!("theme", "удаление системной темы без пакета: {path}");
                    let (ok, msg) = installer.remove_loose_system_theme(&path);
                    finish_action(&state, &handle, msg.clone(), ok);
                    post(&handle, move |mut object| object.as_mut().removal_done(QString::from(&msg)));
                });
            }
            Verdict::FoundConky { path } => {
                // Виджет, поставленный не через Rexon: спросить (у своей папки
                // человека — дважды) здесь же, в потоке интерфейса; снять — в
                // фоне (гаснут процессы, папка уходит в корзину).
                let questions = match rexon_core::conky_scan::live_found_removal(&path) {
                    Ok(Some(r)) => rexon_core::conky_scan::found_removal_questions(&r),
                    Ok(None) => return,
                    Err(reason) => {
                        self.as_mut().action_done(QString::from(&reason), false);
                        return;
                    }
                };
                for (title, text) in questions {
                    if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                        return;
                    }
                }
                let handle = self.as_mut().qt_thread();
                std::thread::spawn(move || {
                    rexon_core::log_info!("theme", "виджет Conky не из Rexon — в корзину: {path}");
                    let (ok, msg) = rexon_core::conky_scan::live_remove_found(&path);
                    if let Ok(mut cache) = state.scan_cache.lock() {
                        cache.clear();
                    }
                    finish_action(&state, &handle, msg.clone(), ok);
                    post(&handle, move |mut object| object.as_mut().removal_done(QString::from(&msg)));
                });
            }
            Verdict::RunningConky { path } => {
                // Свой виджет запущен: спросить «остановить и удалить?»;
                // «Нет» — ничего не трогать. Снять — в фоне.
                let (title, text) = rexon_core::conky_scan::running_removal_question(&path);
                if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                    return;
                }
                let handle = self.as_mut().qt_thread();
                let item_id = item.id.clone();
                std::thread::spawn(move || {
                    rexon_core::log_info!("theme", "запущенный виджет Conky — остановить и в корзину: {path}");
                    let (ok, msg) = removal::live_remove_running_conky(&path, &item_id);
                    if let Ok(mut cache) = state.scan_cache.lock() {
                        cache.clear();
                    }
                    finish_action(&state, &handle, msg.clone(), ok);
                    post(&handle, move |mut object| object.as_mut().removal_done(QString::from(&msg)));
                });
            }
            Verdict::Blocked { reason } => {
                self.as_mut().action_done(QString::from(&reason), false);
            }
            Verdict::Hidden => {}
        }
    }

    fn commit_removals(mut self: Pin<&mut Self>) {
        let state = self.as_ref().get_ref().rust().state.clone();
        let pending = state.removals.lock().map(|mut q| q.take_all()).unwrap_or_default();
        if pending.is_empty() {
            return;
        }
        let de = state.last_de.lock().map(|d| d.clone()).unwrap_or_default();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let (ok, msg) = removal::live_commit(pending, &de);
            // Снятое пропадает с диска, а неснятое (включили, пока висела
            // строка) обязано вернуться в сетку: кеш сканов — заново.
            if let Ok(mut cache) = state.scan_cache.lock() {
                cache.clear();
            }
            finish_action(&state, &handle, msg.clone(), ok);
            post(&handle, move |mut object| object.as_mut().removal_done(QString::from(&msg)));
        });
    }

    fn undo_removals(&self) -> i32 {
        let n = self.rust().state.removals.lock().map(|mut q| q.undo_all()).unwrap_or(0);
        i32::try_from(n).unwrap_or(i32::MAX)
    }

    fn probe_login_screen(mut self: Pin<&mut Self>) {
        let handle = self.as_mut().qt_thread();
        let state = self.rust().state.clone();
        std::thread::spawn(move || probe_login_screen_into(&state, &handle));
    }

    fn login_screen(&self) -> QString {
        let last = self
            .rust()
            .state
            .login_screen
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default();
        QString::from(&last)
    }

    fn make_default_login_manager(mut self: Pin<&mut Self>, unit: &QString, greeter: &QString) {
        use std::sync::atomic::Ordering;
        let handle = self.as_mut().qt_thread();
        let state = self.rust().state.clone();
        let (unit, greeter) = (unit.to_string(), greeter.to_string());
        if state.login_switching.swap(true, Ordering::SeqCst) {
            let msg = rexon_core::system_plan::login_manager_switch_failed(&rexon_core::i18n::tr(
                "the previous change isn't finished yet",
            ));
            post(&handle, move |mut object| {
                object.as_mut().login_manager_switched(false, QString::from(&msg));
            });
            return;
        }
        std::thread::spawn(move || {
            let greeter = (!greeter.trim().is_empty()).then_some(greeter.trim());
            let (ok, msg) = rexon_core::system_exec::live().switch_login_manager(&unit, greeter);
            let first = msg.lines().next().unwrap_or("");
            if ok {
                rexon_core::log_info!("login", "экран входа {unit}: сделан основным — {first}");
            } else {
                rexon_core::log_warn!("login", "экран входа {unit}: не вышло — {first}");
            }
            state.login_switching.store(false, Ordering::SeqCst);
            post(&handle, move |mut object| {
                object.as_mut().login_manager_switched(ok, QString::from(&msg));
            });
            // Раздел перечитывается: основной сменился (или нет — тогда
            // проба покажет прежнее, и это тоже правда).
            probe_login_screen_into(&state, &handle);
        });
    }

    fn make_wayland_default_session(mut self: Pin<&mut Self>, x11: &QString) {
        use std::sync::atomic::Ordering;
        let handle = self.as_mut().qt_thread();
        let state = self.rust().state.clone();
        let x11 = x11.to_string();
        let Some(session) = system_plan::X11_SESSION_SUCCESSORS.iter().find(|s| s.x11 == x11.trim()) else {
            return;
        };
        if state.login_switching.swap(true, Ordering::SeqCst) {
            let msg = system_plan::login_manager_switch_failed(&rexon_core::i18n::tr(
                "the previous change isn't finished yet",
            ));
            post(&handle, move |mut object| {
                object.as_mut().login_manager_switched(false, QString::from(&msg));
            });
            return;
        }
        std::thread::spawn(move || {
            let (ok, msg) = match system_plan::make_wayland_default_live(session) {
                Ok(text) => (true, text),
                Err(why) => (false, why),
            };
            if ok {
                rexon_core::log_info!("login", "сеанс по умолчанию: {} → {}", session.x11, session.wayland);
            } else {
                rexon_core::log_warn!("login", "сеанс по умолчанию не сменён ({}): {msg}", session.x11);
            }
            state.login_switching.store(false, Ordering::SeqCst);
            post(&handle, move |mut object| {
                object.as_mut().login_manager_switched(ok, QString::from(&msg));
            });
            probe_login_screen_into(&state, &handle);
        });
    }

    fn set_login_background(mut self: Pin<&mut Self>, path: &QString) {
        let path = path.to_string();
        let path = path.strip_prefix("file://").map(str::to_string).unwrap_or(path);
        self.as_mut().run_login_look(move || {
            let image = (!path.trim().is_empty()).then_some(path.trim());
            // Plasma Login Manager: пишет его собственный помощник (KAuth),
            // окно пароля — polkit.
            if system_plan::is_plasma_login(&ainst::active_login_manager()) {
                return rexon_core::plasma_login::background_live(image);
            }
            let Some(theme) = system_plan::sddm_current_theme_live() else {
                return Err(rexon_core::i18n::tr("Couldn't change the login screen background."));
            };
            system_plan::sddm_background_live(&theme, image)
        });
    }

    fn apply_login_look(mut self: Pin<&mut Self>, reset: bool) {
        let state = self.rust().state.clone();
        self.as_mut().run_login_look(move || {
            if system_plan::is_plasma_login(&ainst::active_login_manager()) {
                let de = (!reset).then(|| greeter_desktop(&state));
                return rexon_core::plasma_login::look_live(de.as_deref());
            }
            if reset {
                return system_plan::sddm_look_live(None);
            }
            let de = greeter_desktop(&state);
            let look = system_plan::desktop_greeter_look_live(&de);
            system_plan::sddm_look_live(Some(&look))
        });
    }

    fn set_login_clock(mut self: Pin<&mut Self>, show: bool) {
        self.as_mut().run_login_look(move || {
            if system_plan::is_plasma_login(&ainst::active_login_manager()) {
                return rexon_core::plasma_login::clock_live(show);
            }
            let Some(theme) = system_plan::sddm_current_theme_live() else {
                return Err(rexon_core::i18n::tr("Couldn't change the login screen clock."));
            };
            system_plan::sddm_clock_live(&theme, show)
        });
    }

    fn preview_login_screen(mut self: Pin<&mut Self>, token: &QString) -> QString {
        use std::sync::atomic::Ordering;
        // Одно превью за раз; щелчок, пока оно открыто, его закрывает
        // (кнопка блока в это время — «Закрыть предпросмотр»).
        if LOGIN_PREVIEW_RUNNING.load(Ordering::SeqCst) {
            LOGIN_PREVIEW_STOP.store(true, Ordering::SeqCst);
            return QString::default();
        }
        let argv = match system_plan::login_preview_argv_live(&token.to_string()) {
            Ok(a) => a,
            Err(e) => return QString::from(&e),
        };
        if LOGIN_PREVIEW_RUNNING.swap(true, Ordering::SeqCst) {
            return QString::default();
        }
        LOGIN_PREVIEW_STOP.store(false, Ordering::SeqCst);
        rexon_core::log_info!("login", "превью экрана входа: {}", argv.join(" "));
        self.as_mut().login_preview_changed(true, QString::default());
        let handle = self.as_mut().qt_thread();
        // Поток живёт до конца показа: с ним (PR_SET_PDEATHSIG) гаснет и
        // приветствие, если Rexon закроют посреди превью.
        std::thread::spawn(move || {
            let max = std::time::Duration::from_secs(system_plan::LOGIN_PREVIEW_MAX_SECS);
            let out = system_plan::run_greeter_preview(&argv, &LOGIN_PREVIEW_STOP, max);
            LOGIN_PREVIEW_RUNNING.store(false, Ordering::SeqCst);
            let msg = match out {
                Ok(()) => String::new(),
                Err(text) => {
                    rexon_core::log_warn!("login", "превью экрана входа не удалось: {text}");
                    text
                }
            };
            post(&handle, move |mut o| {
                if !msg.is_empty() {
                    o.as_mut().status_message(QString::from(&msg));
                }
                o.as_mut().login_preview_changed(false, QString::from(&msg));
            });
        });
        QString::default()
    }

    fn close_login_preview(&self) {
        LOGIN_PREVIEW_STOP.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn tile_demo_label(&self, token: &QString) -> QString {
        QString::from(&if token.to_string().starts_with(ap::LOGIN_PREVIEW_PREFIX) {
            system_plan::greeter_preview_view(false, None).label
        } else {
            ainst::splash_demo_label()
        })
    }

    fn tile_demo_tooltip(&self, token: &QString) -> QString {
        QString::from(&if token.to_string().starts_with(ap::LOGIN_PREVIEW_PREFIX) {
            system_plan::greeter_preview_view(false, None).tooltip
        } else {
            ainst::splash_demo_tooltip()
        })
    }

    fn open_login_screen_settings(&self) -> bool {
        // Первая команда, которая запустилась. Процесс живёт своей жизнью;
        // ждём его в отдельном потоке только затем, чтобы не оставить зомби.
        for argv in login_settings_commands() {
            let Some((bin, args)) = argv.split_first() else {
                continue;
            };
            if let Ok(mut child) = std::process::Command::new(bin)
                .args(args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return true;
            }
        }
        false
    }

    fn shell_palette(&self) -> QString {
        let banner = palette::live_banner();
        QString::from(&serde_json::to_string(&banner).unwrap_or_default())
    }

    fn set_palette_lead(&self, lead: &QString) -> bool {
        match palette::Lead::parse(&lead.to_string()) {
            Some(l) => palette::set_lead(&palette::PaletteHome::live(), l).is_ok(),
            None => false,
        }
    }

    fn revert_applied(&self, section: &QString) -> QString {
        let rexon = utils::rexon_config_dir();
        let (ok, text) = ap::revert_applied_section(&rexon, &palette::PaletteHome::live(), &section.to_string());
        if let Ok(mut cache) = self.rust().state.scan_cache.lock() {
            cache.clear();
        }
        QString::from(&json!({ "ok": ok, "text": text }).to_string())
    }

    fn push_shell_palette(mut self: Pin<&mut Self>) {
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let (ok, msg) = palette::push_flow(palette::live_detect(), &palette::PaletteHome::live());
            post_palette(&handle, msg, ok);
        });
    }

    fn pull_shell_palette(mut self: Pin<&mut Self>) {
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let logger = log_sink(&handle);
            let mut installer = LiveInstaller::new("", logger);
            let mut apply = |scheme: &str| apply_color_scheme(&mut installer, scheme);
            let (ok, msg) = palette::pull_flow(
                palette::live_detect(),
                &palette::PaletteHome::live(),
                &mut palette::nudge_noctalia_live,
                3000,
                &mut apply,
            );
            if ok {
                // Активная схема сменилась — бейдж «Активна» в разделе тоже.
                if let Ok(mut cache) = state.scan_cache.lock() {
                    cache.clear();
                }
            }
            post_palette(&handle, msg, ok);
        });
    }

    fn revert_shell_palette(mut self: Pin<&mut Self>) {
        let state = self.as_ref().get_ref().rust().state.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let logger = log_sink(&handle);
            let mut installer = LiveInstaller::new("", logger);
            let mut apply = |scheme: &str| apply_color_scheme(&mut installer, scheme);
            let (ok, msg) = palette::revert_flow(&palette::PaletteHome::live(), &mut apply);
            if let Ok(mut cache) = state.scan_cache.lock() {
                cache.clear();
            }
            post_palette(&handle, msg, ok);
        });
    }

    fn purge_translations(mut self: Pin<&mut Self>) {
        // Чистка протухших записей общего кеша переводов. Переводит Rust-вкладка
        // теперь всё сама — и описания (`translate_detail_description`), и
        // комментарии (`translate_comment_page`), — а база у неё с виджетной
        // вкладкой ОДНА: записи, переведённые той, никуда не деваются, и
        // чистильщик здесь тоже один на обе.
        std::thread::spawn(move || {
            let mut cache = rexon_core::appearance_cache::LiveCache::open(&home());
            cache.purge_stale_translations();
        });
        // `self` здесь не нужен, но подпись метода обязана быть той же, что у
        // остальных долгих действий: страница зовёт его таймером.
        let _ = self.as_mut();
    }
}

/// Чем открыть настройки экрана входа на этой машине — решение ядра
/// (`appearance_page::login_settings_commands`), здесь только поиск программ.
fn login_settings_commands() -> Vec<Vec<String>> {
    ap::login_settings_commands(&|bin| utils::which(bin, None).is_some())
}

// ============================================================================
// Состав раздела
// ============================================================================

/// Один источник каталога по виду запроса.
fn fetch_kind(state: &Arc<State>, req: &Value, token: &CancelToken, kind: &str) -> String {
    match kind {
        "local" => scan_local(state, req),
        "aur" => fetch_aur(state, req, token),
        "spices" => fetch_spices(state, req),
        _ => fetch_ocs(state, req, token),
    }
}

/// Раздел с установленным: сверху установленное (только на первой странице
/// магазина), под ним страница магазина раздела — одним ответом. Магазин
/// не ответил (нет сети) — его ошибка идёт в строку состояния, а
/// установленное всё равно видно.
fn fetch_combined(state: &Arc<State>, req: &Value, token: &CancelToken) -> String {
    let store: Value = serde_json::from_str(&fetch_kind(state, req, token, s(req, "storeKind")))
        .unwrap_or_else(|_| json!({}));
    let mut answer = match store {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    answer.insert("key".to_string(), Value::from(s(req, "key")));
    if num(req, "page") == 0 {
        let local: Value = serde_json::from_str(&scan_local(state, req)).unwrap_or_else(|_| json!({}));
        let mut items: Vec<Value> = local.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        if cs::type_for_section(s(req, "categoryKey")).is_some() {
            mark_spice_danger(&mut items);
        }
        if let Some(Value::Array(rest)) = answer.get("items") {
            items.extend(rest.iter().cloned());
        }
        answer.insert("items".to_string(), Value::Array(items));
    }
    Value::Object(answer).to_string()
}

/// Страница магазина (плюс AUR-хвост в режиме «Все») — порт `_fetch_ocs`.
fn fetch_ocs(state: &Arc<State>, req: &Value, token: &CancelToken) -> String {
    let mut answer = Map::new();
    answer.insert("key".to_string(), Value::from(s(req, "key")));

    let net = UreqNet::new().with_cancel(token.clone());
    let io = LiveOcsIo::with_net(net, &home());
    let preview_dir = io.preview_dir();
    let mut client = OcsClient::new(io, &preview_dir);
    let (page, failure) = store_page_checked(&mut client, req);
    let ap::StreamPage { items: found, total, has_more } = page;
    let mut items: Vec<ThemeItem> = found.iter().map(|c| c.item.clone()).collect();
    // Магазин всегда отвечает «не установлено»: «✓ Установлено»/«✓ Применено»
    // восстанавливаются по журналу установок (решение ядра).
    ap::stamp_installed_live(&home(), &mut items);
    remember_items(state, &items);
    let rows: Vec<Value> = items
        .iter()
        .map(|it| serde_json::to_value(it).unwrap_or(Value::Null))
        .collect();

    let first_page = num(req, "page") == 0;
    // Установленное сюда больше не клеится: у заставки, полных тем
    // блокировки и разделов KDE оно открывается своей сеткой, а магазин —
    // кнопкой «Загрузить новые…» (`CatalogView::installed_view`), и запрос
    // за ним идёт обычным сканом раздела (`scan_local`).
    // Магазин страницу не дал (отказ, сбой связи) — это не «ничего не
    // найдено»: строка состояния говорит, что магазин не отвечает.
    // Установленное раздел показывает по-прежнему (`fetch_combined`).
    if rows.is_empty() {
        if let Some(f) = &failure {
            answer.insert("error".to_string(), Value::from(ap::store_failure_text(f)));
        }
    }
    answer.insert("items".to_string(), Value::Array(rows));
    answer.insert("total".to_string(), Value::from(total));
    answer.insert("hasMore".to_string(), Value::from(has_more));

    // «Все»: AUR-карточки доклеиваются НИЖЕ магазинных и только на первой
    // странице — дальше листается только магазин.
    if s(req, "kind") == "both" && first_page {
        match aur_packages(state, req) {
            Ok(pkgs) => {
                let top: Vec<AurPackage> =
                    pkgs.into_iter().take(page_size(req) as usize).collect();
                remember_packages(state, &top);
                answer.insert(
                    "packages".to_string(),
                    serde_json::to_value(&top).unwrap_or(Value::Array(Vec::new())),
                );
            }
            // AUR не ломает магазин: витрина уже собрана, и отказ RPC не
            // повод показать человеку пустой раздел.
            Err(e) => rexon_core::log_warn!("aur", "режим «Все»: {e}"),
        }
    }
    Value::Object(answer).to_string()
}

/// Страница магазина с добором до полной (`ap::stream_page`): KDE Store
/// скрывает часть работ, и серверная страница приходит короче запрошенной.
/// Серверные страницы берутся через кеш `ocs_cache`, так что при листании
/// подряд в сеть уходят только новые.
fn store_page<IO: rexon_core::ocs::OcsIo>(
    client: &mut OcsClient<IO>,
    req: &Value,
) -> ap::StreamPage<rexon_core::ocs::OcsContent> {
    store_page_checked(client, req).0
}

/// [`store_page`] и первая причина, по которой магазин страницу не дал.
fn store_page_checked<IO: rexon_core::ocs::OcsIo>(
    client: &mut OcsClient<IO>,
    req: &Value,
) -> (ap::StreamPage<rexon_core::ocs::OcsContent>, Option<rexon_core::ocs::StoreFailure>) {
    let size = page_size(req).clamp(1, i64::from(u32::MAX)) as u32;
    let mut failure = None;
    // Серверная страница — всегда `PAGE_SIZE`: ключ кеша магазина размера
    // не знает, а страница Rexon (целые ряды сетки) режется из потока.
    let page = ap::stream_page(num(req, "page"), size, ap::PAGE_SIZE, |k| {
        let (mut chunk, total) = match client.search_checked(
            s(req, "categoryId"),
            k,
            i64::from(ap::PAGE_SIZE),
            s(req, "sort"),
            s(req, "query"),
            true,
        ) {
            Ok(found) => found,
            Err(e) => {
                failure.get_or_insert(e);
                (Vec::new(), 0)
            }
        };
        // Программы в разделе виджетов Conky (Conky Manager) — не виджеты;
        // страница добирается до полной следующей серверной.
        chunk.retain(|c| !de_apply::is_conky_program(&c.item));
        (chunk, total)
    });
    (page, failure)
}

/// Раздел из AUR: подборка по разделу либо поиск по всему AUR — порт
/// `_fetch_aur`.
fn fetch_aur(state: &Arc<State>, req: &Value, _token: &CancelToken) -> String {
    let mut answer = Map::new();
    answer.insert("key".to_string(), Value::from(s(req, "key")));
    answer.insert("items".to_string(), Value::Array(Vec::new()));
    answer.insert("total".to_string(), Value::from(0));

    let query = s(req, "query");
    if ap::aur_query_too_short(query) {
        answer.insert(
            "error".to_string(),
            Value::from(ap::aur_query_too_short_msg()),
        );
        return Value::Object(answer).to_string();
    }
    let pkgs = match aur_packages(state, req) {
        Ok(p) => p,
        Err(e) => {
            answer.insert(
                "error".to_string(),
                Value::from(ui_base::humanize_network_error(&e, "AUR").as_str()),
            );
            return Value::Object(answer).to_string();
        }
    };
    // Страниц у RPC нет — он отдаёт список целиком; режем сами.
    let size = page_size(req).max(1) as usize;
    let page = num(req, "page").max(0) as usize;
    let chunk: Vec<AurPackage> = pkgs.iter().skip(page * size).take(size).cloned().collect();
    remember_packages(state, &chunk);
    answer.insert(
        "packages".to_string(),
        serde_json::to_value(&chunk).unwrap_or(Value::Array(Vec::new())),
    );
    answer.insert("total".to_string(), Value::from(pkgs.len() as i64));
    Value::Object(answer).to_string()
}

/// Магазин Cinnamon Spices раздела: индекс сайта из кэша (сеть — раз в
/// сутки условным запросом или по «Обновить»), страница и поиск — по нему.
/// Установлено ли и есть ли обновление — по диску (`last-edited` в
/// `metadata.json`): так видно и поставленное окном настроек Cinnamon.
fn fetch_spices(state: &Arc<State>, req: &Value) -> String {
    let mut answer = Map::new();
    answer.insert("key".to_string(), Value::from(s(req, "key")));
    let section = s(req, "categoryKey");
    // Разделы Cinnamon — свой тип спайсов; «Темы GTK» и рамки окон — темы,
    // отобранные по составу (`cinnamon_spices::PART_SECTIONS`).
    let part = cs::is_part_section(section).then_some(section);
    let Some(t) = cs::type_for_section(section).or_else(|| part.and_then(|_| cs::type_for_key("theme"))) else {
        answer.insert("items".to_string(), Value::Array(Vec::new()));
        answer.insert("total".to_string(), Value::from(0));
        return Value::Object(answer).to_string();
    };
    let force = req.get("force").and_then(Value::as_bool).unwrap_or(false);
    let mut refresh = cs::live_refresh(t, force);
    let parts = part.map(|_| cs::live_refresh_parts(force));
    if let (Some(sec), Some(p)) = (part, parts.as_ref()) {
        refresh.spices = cs::themes_for_section(&refresh.spices, &p.parts, sec);
        if !p.note.is_empty() {
            refresh.note = if refresh.note.is_empty() { p.note.clone() } else { format!("{} {}", refresh.note, p.note) };
        }
    }
    let lang = rexon_core::i18n::current_language();
    let size = page_size(req).max(1) as usize;
    let page = num(req, "page").max(0) as usize;
    let (chunk, total) = cs::filter_sort_page(&refresh.spices, s(req, "query"), s(req, "sort"), page, size, &lang);
    let env = cs::SpiceEnv::live();
    let mut items: Vec<ThemeItem> = Vec::with_capacity(chunk.len());
    let mut rows: Vec<Value> = Vec::with_capacity(chunk.len());
    for sp in &chunk {
        let mut it = match (part, parts.as_ref()) {
            (Some(sec), Some(p)) => {
                let paths = p.parts.get(&sp.key).map(Vec::as_slice).unwrap_or(&[]);
                cs::section_theme_item(sp, &lang, sec, s(req, "de"), paths)
            }
            _ => cs::theme_item(t, sp, &lang),
        };
        let mark = cs::installed_state(&env, t, &sp.key);
        it.installed = mark.is_some();
        let danger = it.installed && cs::code_dir(&env, t, &sp.key).is_some_and(|d| cs::scan_for_danger(std::path::Path::new(&d)));
        let mut v = serde_json::to_value(&it).unwrap_or(Value::Null);
        if let Some(m) = v.as_object_mut() {
            m.insert("spicesScore".to_string(), Value::from(sp.score));
            m.insert("spicesEdited".to_string(), Value::from(sp.last_edited));
            m.insert("spicesUpdate".to_string(), Value::from(mark.is_some_and(|m| cs::has_update(m, sp.last_edited))));
            m.insert("spicesPage".to_string(), Value::from(cs::page_url(t, &sp.spices_id)));
            m.insert("spicesDanger".to_string(), Value::from(danger));
            // В разделе GTK или рамок поставленную тему включает сама
            // карточка — «Применить».
            m.insert("spicesApply".to_string(), Value::from(part.is_some()));
        }
        rows.push(v);
        items.push(it);
    }
    remember_items(state, &items);
    if rows.is_empty() && refresh.spices.is_empty() && !refresh.note.is_empty() {
        answer.insert("error".to_string(), Value::from(refresh.note.clone()));
    } else if !refresh.note.is_empty() {
        answer.insert("note".to_string(), Value::from(refresh.note.clone()));
    }
    answer.insert("items".to_string(), Value::Array(rows));
    answer.insert("total".to_string(), Value::from(total as i64));
    Value::Object(answer).to_string()
}

/// Плиткам установленного раздела Cinnamon — пометка `spicesDanger`, если
/// код спайса зовёт блокирующие вызовы (`cinnamon_spices::scan_for_danger`).
fn mark_spice_danger(items: &mut [Value]) {
    for v in items.iter_mut() {
        let path = v.get("install_path").and_then(Value::as_str).unwrap_or("").to_string();
        if path.is_empty() || !std::path::Path::new(&path).is_dir() {
            continue;
        }
        if cs::scan_for_danger(std::path::Path::new(&path)) {
            if let Some(m) = v.as_object_mut() {
                m.insert("spicesDanger".to_string(), Value::from(true));
            }
        }
    }
}

/// Список пакетов AUR под запрос, с кешем на сессию вкладки.
fn aur_packages(state: &Arc<State>, req: &Value) -> Result<Vec<AurPackage>, String> {
    let key = s(req, "aurKey").to_string();
    if let Some(cached) = state.aur_cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Ok(cached);
    }
    let mut net = UreqNet::new();
    let query = s(req, "query");
    // Без запроса — раздел целиком, и наверху его курируемые пакеты
    // (`aur::CURATED`): AeroThemePlasma в «Глобальных темах», её значки — в
    // «Значках», звуки — в «Звуках».
    let mut pkgs = if query.is_empty() {
        aur::fetch_category_with_curated(&mut net, s(req, "categoryKey"))
    } else {
        aur::search_aur(&mut net, query, None)
    }
    .map_err(|e| e.to_string())?;
    let installed = installed_packages();
    for p in &mut pkgs {
        p.installed = installed.contains(&p.name);
    }
    if let Ok(mut cache) = state.aur_cache.lock() {
        cache.insert(key, pkgs.clone());
    }
    Ok(pkgs)
}

/// Обзор диска: активные, загруженные, системные, избранное, стили — порт
/// `_scan_local`.
fn scan_local(state: &Arc<State>, req: &Value) -> String {
    let which = s(req, "which").to_string();
    // Рабочий стол, под который смотрит вкладка: от него зависят разделы
    // окружений в обзорах и то, какие плитки здесь вообще применимы.
    let de = s(req, "de").to_string();
    if let Ok(mut last) = state.last_de.lock() {
        *last = de.clone();
    }
    let cache_key = format!("{which}@{de}");
    let mut answer = Map::new();
    answer.insert("key".to_string(), Value::from(s(req, "key")));

    let mut items: Vec<ThemeItem> = if which == "favorites" {
        // Избранное — товары магазина из кеша: состояние установки у них то
        // же, что в сетке магазина, и берётся из того же журнала.
        let mut fav = favorite_items();
        ap::stamp_installed_live(&home(), &mut fav);
        fav
    } else {
        if req.get("fresh").and_then(Value::as_bool).unwrap_or(false) {
            if let Ok(mut cache) = state.scan_cache.lock() {
                cache.remove(&cache_key);
            }
        }
        // Тему положили или сняли мимо Rexon (в `~/.themes`, `~/.icons`…)
        // при открытом окне: прежние сканы не годятся ни для одного раздела.
        let stamp = scan::roots_stamp(&home(), "/usr");
        if let (Ok(mut last), Ok(mut cache)) = (state.scan_stamp.lock(), state.scan_cache.lock()) {
            if *last != stamp {
                cache.clear();
                *last = stamp;
            }
        }
        let cached = state
            .scan_cache
            .lock()
            .ok()
            .and_then(|c| c.get(&cache_key).cloned());
        match cached {
            Some(list) => list,
            None => match scan_disk(state, &which, &de) {
                Some(list) => {
                    let mut list = ap::local_items_for_de(list, std::path::Path::new(&home()), &de);
                    // Мелкие миниатюры подкаталогов (120×35) плитку целиком
                    // не покрывают: такой плитке кадр соберёт окно.
                    rexon_core::local_preview::drop_small_thumbnails(&mut list);
                    // GTK-тема без своих значков и курсора — сказать на плитке.
                    let deps = LiveInstaller::new("", Box::new(|_: &str| {}));
                    scan::stamp_missing_deps(&mut list, &|name| deps.theme_deps_plan(name));
                    if let Ok(mut cache) = state.scan_cache.lock() {
                        cache.insert(cache_key.clone(), list.clone());
                    }
                    list
                }
                None => {
                    // Системные темы не классифицированы достоверно
                    // (`pacman -Qm` упал): показать их нельзя — протекли бы
                    // AUR-темы. Кеш не пишем, при следующем заходе пробуем
                    // снова.
                    answer.insert("items".to_string(), Value::Array(Vec::new()));
                    answer.insert("total".to_string(), Value::from(0));
                    answer.insert(
                        "error".to_string(),
                        Value::from(ap::status_system_scan_unavailable()),
                    );
                    return Value::Object(answer).to_string();
                }
            },
        }
    };

    // Поиск по обзору диска — на месте, без сети. Мерило «остаётся ли тема в
    // поле зрения» СВОЁ здесь не считается: оно одно на все места, где
    // страница показывает темы с диска, и живёт в ядре.
    let query = s(req, "query");
    if !query.trim().is_empty() {
        items = ap::local_filter(&items, query);
    }
    // Помеченные к удалению не показываются, пока висит строка «Вернуть».
    if let Ok(queue) = state.removals.lock() {
        queue.retain_visible(&mut items);
    }
    remember_items(state, &items);
    let rows: Vec<Value> = local_rows(&items, &which);
    answer.insert("total".to_string(), Value::from(items.len() as i64));
    answer.insert("items".to_string(), Value::Array(rows));
    Value::Object(answer).to_string()
}

/// Один скан диска. `None` — источник пакетов не определён (только
/// «Системные»).
///
/// ⚠️ Таблица «вид скана → добытчик» оставлена ЗДЕСЬ осознанно: справа в ней
/// стоят живые обходы диска и опрос пакетного менеджера, то есть исполнение.
/// Знание О ФОРМАТЕ ключа при этом внизу — его собирает и разбирает ядро
/// (`local_scan_plan`).
fn scan_disk(state: &Arc<State>, which: &str, de: &str) -> Option<Vec<ThemeItem>> {
    let (kind, style_name) = ap::local_scan_plan(which);
    let live = live_scan().with_de(de);
    let active = live.active_map();
    let items = match kind.as_str() {
        "active" => live.active(&active),
        "downloaded" => live.downloaded(&active),
        "system" => match live.system(&active) {
            Ok(items) => items,
            Err(_) => return None,
        },
        // «Стиль приложений» одной сеткой: стили Qt (тот же живой список,
        // что снят в `environment`; не снят — запас ядра) и темы Kvantum.
        // Вторым элементом плана — фильтр (`ap::STYLE_FILTER_*`).
        "widgetstyle" => {
            let filter = style_name.as_str();
            let mut items = Vec::new();
            if filter != ap::STYLE_FILTER_KVANTUM {
                let known = state
                    .qt_style_keys
                    .lock()
                    .map(|k| k.clone())
                    .unwrap_or_default();
                let styles = ap::shown_qt_styles(&scan::available_widget_styles(&known));
                items.extend(live.widget_styles(&active, &styles, true));
            }
            if filter != ap::STYLE_FILTER_QT {
                items.extend(live.kvantum(&active));
            }
            items
        }
        "splash" | "lockscreen" => live.lnf(&kind, &active),
        // Раздел окружения: под-ключ — сам раздел (`local_scan_plan`).
        "section" => live.section(&style_name, &active),
        _ => Vec::new(),
    };
    Some(items)
}

/// Строки местных тем с решением «Удалить» (`theme_removal`): журнал и
/// каталоги читаются один раз на скан.
fn local_rows(items: &[ThemeItem], which: &str) -> Vec<Value> {
    let removal = LiveRemoval::new();
    items
        .iter()
        .map(|it| {
            let mut row = local_row(it, which);
            if it.id.starts_with("local:") {
                stamp_removal(&mut row, &removal.verdict(it));
            }
            row
        })
        .collect()
}

/// Поля «Удалить» в строке плитки: `removeVisible`, `removeEnabled`,
/// `removeTooltip` (их читает страница, `RemovalUi`).
fn stamp_removal(row: &mut Value, verdict: &Verdict) {
    if let Some(map) = row.as_object_mut() {
        map.insert("removeVisible".to_string(), Value::from(verdict.visible()));
        map.insert("removeEnabled".to_string(), Value::from(verdict.enabled()));
        map.insert("removeTooltip".to_string(), Value::from(verdict.tooltip()));
    }
}

/// Местная тема для страницы: у неё свой вид плитки и своя кнопка. Какой
/// именно вид — решает ядро (`scan_local_kind`).
fn local_row(item: &ThemeItem, which: &str) -> Value {
    let mut raw = serde_json::to_value(item).unwrap_or(Value::Null);
    let kind = item
        .local_kind
        .clone()
        .unwrap_or_else(|| ap::scan_local_kind(which, &item.author).to_string());
    if let Some(map) = raw.as_object_mut() {
        map.insert("localKind".to_string(), Value::from(kind));
        // Поле в файлы не пишется (`#[serde(skip)]`), но поиску на странице
        // нужно и оригинальное имя — везём его рядом, как вид плитки.
        if !item.source_name.is_empty() {
            map.insert("sourceName".to_string(), Value::from(item.source_name.clone()));
        }
    }
    raw
}

// ============================================================================
// Панель деталей
// ============================================================================

/// Кадры карусели, затем полные детали, затем остальные кадры, затем перевод
/// описания.
///
/// ⭐ Сначала КАРТИНКИ, потом текст: ссылки на скриншоты известны уже в момент
/// щелчка, а детальный ответ магазина шёл до 20 с — всё это время человек
/// смотрел на «Загрузка скриншота».
///
/// ⭐ Перевод — ПОСЛЕДНИМ и отдельным сигналом, а не полем `detail_ready`.
/// Причина та же, по которой картинки идут первыми: показ карточки не должен
/// ждать ничего, что можно дослать. Модель поднимается 0,3–1 с, описание из
/// двадцати предложений переводится около секунды — задержи на это первый
/// показ, и щелчок по плитке снова выглядел бы зависанием.
fn fetch_detail(state: &Arc<State>, handle: &Handle, item: ThemeItem) {
    let io = LiveOcsIo::new(&home());
    let preview_dir = io.preview_dir();
    let mut client = OcsClient::new(io, &preview_dir);
    let id = item.id.clone();

    let mut seen: HashMap<usize, String> = HashMap::new();
    for (i, url) in ap::gallery_urls(&item.preview_urls, &item.preview_url)
        .into_iter()
        .enumerate()
    {
        if !still_open(state, &id) {
            return; // товар закрыли — качать незачем
        }
        if emit_frame(handle, &mut client, &i.to_string(), &url) {
            seen.insert(i, url);
        }
    }

    // Свежие (до суток) полные детали — из кеша, без ожидания магазина; срок
    // один на обе версии вкладки (`ocs::DETAIL_TTL_SECS`). Установка этим
    // путём не ходит: ей нужны свежие ссылки на архив.
    // Спайсу Cinnamon магазин KDE не нужен: всё, что есть у сайта, уже в
    // карточке из индекса.
    let mut full = if id.starts_with(cs::ID_PREFIX) {
        item
    } else {
        client
            .get_item_cached(&id, rexon_core::ocs::DETAIL_TTL_SECS)
            .map(|c| c.item)
            .unwrap_or(item)
    };
    // Полные детали магазина тоже приходят «не установлено» — без сверки с
    // журналом они сбросили бы «✓ Применено» открытого товара.
    ap::stamp_installed_live(&home(), std::slice::from_mut(&mut full));
    remember_items(state, std::slice::from_ref(&full));
    if !still_open(state, &id) {
        return;
    }
    let mut payload = serde_json::to_value(&full).unwrap_or(Value::Null);
    // «Удалить» у поставленного товара: включённую сейчас тему снять нельзя —
    // та же проверка, что у плитки установленного (`theme_removal`).
    if full.installed && full.local_kind.is_none() {
        let de = state.last_de.lock().map(|d| d.clone()).unwrap_or_default();
        let verdict = match removal::live_store_blocked(&id, &de) {
            Some(reason) => Verdict::Blocked { reason },
            None => Verdict::Journal { content_id: id.clone() },
        };
        stamp_removal(&mut payload, &verdict);
    }
    let payload = payload.to_string();
    let handle_detail = handle.clone();
    post(&handle_detail, move |mut object| {
        object.as_mut().detail_ready(QString::from(&payload));
    });

    // Детальный ответ знает БОЛЬШЕ скриншотов, чем список: проходим полный
    // набор, пропуская уже скачанное по тому же адресу.
    let mut got = 0usize;
    for (i, url) in ap::gallery_urls(&full.preview_urls, &full.preview_url)
        .into_iter()
        .enumerate()
    {
        if !still_open(state, &id) {
            return;
        }
        if seen.get(&i).map(|u| u == &url).unwrap_or(false) {
            got += 1;
            continue;
        }
        if emit_frame(handle, &mut client, &i.to_string(), &url) {
            got += 1;
        }
    }
    let count = got.to_string();
    post(handle, move |mut object| {
        object.as_mut().gallery_finished(QString::from(&count));
    });

    translate_detail_description(state, handle, &full);
}

/// Второй проход: перевести описание открытого товара и дослать его на панель.
///
/// Работа идёт в ТОМ ЖЕ потоке, что и остальная долгая работа панели, — окно её
/// не ждёт. Наружу смотрит одна дверь ядра,
/// [`rexon_core::translator_engine::translate_live_guarded`]: она сама берёт целевой
/// язык из локали, отличает прозу от техники, ходит в кеш и поднимает движок.
///
/// Три тихих выхода, и все три — не ошибки:
///   * описание пустое — переводить нечего;
///   * товар уже закрыли (человек ушёл на другой) — перевод чужой карточке не
///     нужен, и `still_open` спрашивается ДВАЖДЫ: до долгой работы и после неё;
///   * `was_translated == false` — модели в поставке нет, текст уже на языке
///     покупателя либо это техника. Сигнала нет, жалобы нет, на панели остаётся
///     оригинал: перевод — удобство, а не условие работы вкладки.
fn translate_detail_description(state: &Arc<State>, handle: &Handle, item: &ThemeItem) {
    if item.description.trim().is_empty() || !still_open(state, &item.id) {
        return;
    }
    let mut cache = rexon_core::appearance_cache::LiveCache::open(&home());
    // Название темы и имя автора движку не отдаются: без этого «Sweet Mars»
    // становится «сладким Марсом», а «Nordic» — «Скандинавской». Ядро само
    // отбрасывает пустые и слишком короткие имена, поэтому сверять тут нечего.
    let done = te::translate_live_guarded(
        &item.description,
        &rexon_core::translator::interface_locale(),
        &te::LiveEngine::new(),
        &mut cache,
        &[item.name.as_str(), item.author.as_str()],
    );
    if !done.was_translated || !still_open(state, &item.id) {
        return;
    }
    let payload = json!({ "itemId": item.id, "translated": done.text }).to_string();
    post(handle, move |mut object| {
        object.as_mut().description_ready(QString::from(&payload));
    });
}

/// Пометить новый обход списка комментариев и вернуть его номер.
///
/// Номер меняется, только когда список строится ЗАНОВО: первая страница либо
/// другой товар. «Показать ещё» (страницы 1, 2, 3) обход не сбрасывает — те
/// комментарии остаются на экране, и перевод, который для них уже считается,
/// по-прежнему нужен.
fn mark_comments_view(state: &Arc<State>, id: &str, page: i64) -> u64 {
    let Ok(mut view) = state.comments_view.lock() else {
        return 0;
    };
    if page == 0 || view.0 != id {
        *view = (id.to_string(), view.1 + 1);
    }
    view.1
}

/// Те же комментарии всё ещё показываются: тот же товар открыт и список с тех
/// пор не строился заново.
fn comments_shown(state: &Arc<State>, id: &str, view: u64) -> bool {
    still_open(state, id)
        && state
            .comments_view
            .lock()
            .map(|now| now.0 == id && now.1 == view)
            .unwrap_or(false)
}

/// Второй проход страницы комментариев: перевести её ЦЕЛИКОМ одним походом к
/// движку и дослать переводы на уже показанный список.
///
/// ⭐ ОДНИМ пакетом, а не по комментарию:
/// [`rexon_core::translator_engine::translate_live_batch`] набирает блоки
/// предложений ПОПЕРЁК текстов. Комментарии короткие (одна-две фразы) и их
/// двадцать на страницу — по одному это были бы двадцать почти пустых блоков,
/// то есть пакетный путь движка вхолостую. Решение «переводить ли» при этом
/// остаётся у КАЖДОГО комментария своё, и русский среди английских к движку не
/// уезжает вовсе.
///
/// Тихие выходы — те же, что у описания, и ни один не ошибка:
///   * переводить нечего (все комментарии пустые);
///   * список уже сменился — сторож [`comments_shown`] спрашивается ДВАЖДЫ, до
///     долгой работы и после неё;
///   * ни одного перевода не случилось (нет модели, комментарии уже на языке
///     покупателя) — сигнала нет, жалобы нет, на экране остаются оригиналы.
///
/// В `rows` едут только те комментарии, у которых перевод действительно
/// случился: остальных страница не увидит, и ссылка «показать оригинал» над
/// ними не появится.
fn translate_comment_page(
    state: &Arc<State>,
    handle: &Handle,
    id: &str,
    page: i64,
    view: u64,
    comments: &[String],
) {
    // Только тексты: тот же второй проход у комментариев магазина и у
    // комментариев со страницы пакета AUR.
    let texts: Vec<&str> = comments.iter().map(String::as_str).collect();
    if texts.iter().all(|t| t.trim().is_empty()) || !comments_shown(state, id, view) {
        return;
    }
    let mut cache = rexon_core::appearance_cache::LiveCache::open(&home());
    // Люди в комментариях называют тему по имени («Sweet Mars looks great»),
    // и без охраны имя переводится так же, как в описании. Товар берём из
    // того же склада, что и остальные ветки вкладки; не нашли — просто
    // переводим без охраны, это не повод отказываться от перевода.
    let item = state.items.lock().ok().and_then(|m| m.get(id).cloned());
    let guarded: Vec<&str> = item
        .as_ref()
        .map(|it| vec![it.name.as_str(), it.author.as_str()])
        .unwrap_or_default();
    let done = te::translate_live_batch_guarded(
        &texts,
        &rexon_core::translator::interface_locale(),
        &te::LiveEngine::new(),
        &mut cache,
        &guarded,
    );
    if !comments_shown(state, id, view) {
        return;
    }
    // Пара «что было → что вышло», а не номер строки: список к этому времени
    // мог дорасти страницей, и место комментария в нём — знание страницы, не
    // наше. По тексту же она найдёт свои строки сама.
    let rows: Vec<Value> = comments
        .iter()
        .zip(done.iter())
        .filter(|(_, tr)| tr.was_translated)
        .map(|(c, tr)| json!({ "text": c, "translated": tr.text }))
        .collect();
    if rows.is_empty() {
        return;
    }
    let payload = json!({ "itemId": id, "page": page, "rows": rows }).to_string();
    post(handle, move |mut object| {
        object
            .as_mut()
            .comment_translations_ready(QString::from(&payload));
    });
}

/// Один кадр карусели: из кеша либо из сети, и сразу на панель.
fn emit_frame(
    handle: &Handle,
    client: &mut OcsClient<LiveOcsIo>,
    index: &str,
    url: &str,
) -> bool {
    let preview_dir = rexon_core::appearance_cache::preview_dir(&home());
    match client.get_preview_image(url).filter(|p| usable_preview(p, &preview_dir)) {
        Some(local) => {
            let (index, local) = (index.to_string(), local);
            post(handle, move |mut object| {
                object
                    .as_mut()
                    .gallery_ready(QString::from(&index), QString::from(&local));
            });
            true
        }
        None => {
            // Молчать нельзя: без отказа панель показывала бы «Загрузка
            // скриншота…» вечно — удачи она видела, а отказы нет.
            let index = index.to_string();
            post(handle, move |mut object| {
                object.as_mut().gallery_failed(QString::from(&index));
            });
            false
        }
    }
}

/// Нужно ли поставить bsdtar перед установкой товара, чей файл — архив 7z,
/// а распаковать его нечем (без вопроса — защита окно пароля polkit).
fn seven_zip_needs_install(item: &ThemeItem, file_name: &str) -> bool {
    use rexon_core::appearance_installer as ai;
    let idx = rexon_core::ocs::file_index_for(item, file_name);
    let file = rexon_core::ocs::file_name_at(item, idx).unwrap_or_else(|| item.download_name.clone());
    ai::seven_zip_install_package(&file, ai::have_seven_zip_tool(), ai::live_seven_zip_package()).is_some()
}

/// Нужно ли поставить Conky перед запуском виджета (без вопроса — защита
/// окно пароля polkit). `false` — не виджет Conky, Conky стоит или
/// поставить его здесь нельзя.
fn conky_needs_install(item: &ThemeItem) -> bool {
    de_apply::is_conky_item(item)
        && de_apply::conky_install_package(rexon_core::de_apply::have_conky(), de_apply::live_conky_package()).is_some()
}

/// Вопрос «разрешить виджету Conky выполнять команды?» перед запуском — в
/// потоке интерфейса, ДО фоновой работы. `part` — файл части виджета
/// (кнопка части), `None` — виджет целиком. `None` в ответе — спрашивать
/// не о чем: кнопка остановит работающий виджет, команд нет или виджет уже
/// разрешён в нынешнем виде; `Some(ответ)` — что сказал человек, «да»
/// запоминается сразу.
fn ask_conky_safety(name: &str, part: Option<&str>) -> Option<bool> {
    use rexon_core::conky_safety as cs;
    let io = rexon_core::animations::SystemIo;
    let ctx = de_apply::Ctx::live();
    let running = match part {
        Some(conf) => rexon_core::conky_procs::running_confs(&io, &ctx).iter().any(|c| c == conf),
        None => de_apply::conky_running_ids(&io, &ctx).iter().any(|i| i == name),
    };
    if running {
        return None;
    }
    // Ремонт путей правит файлы виджета — до отпечатка, а не после.
    de_apply::conky_prepare(&ctx, name);
    let (title, text) = cs::start_question(&io, &ctx, name)?;
    let yes = crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text));
    if yes && !cs::remember(&io, &ctx, name) {
        rexon_core::log_warn!("appearance", "разрешение виджета Conky «{name}» не записалось: {}", cs::trust_path(&ctx));
    }
    Some(yes)
}

/// Запустит ли кнопка виджет Conky (а не остановит работающий) — тогда
/// фоновая работа сначала проверяет программы виджета
/// (`conky_safety::live_prepare_start`). Смотрится в потоке интерфейса, до
/// нажатия-итога.
fn conky_will_start(name: &str) -> bool {
    let io = rexon_core::animations::SystemIo;
    let ctx = de_apply::Ctx::live();
    !de_apply::conky_running_ids(&io, &ctx).iter().any(|i| i == name)
}

// ============================================================================
// Установка
// ============================================================================

/// Порт `_install`: обновить детали, скачать архив, поставить (и применить).
/// `file_name` — файл, выбранный руками в панели деталей; пусто или такого
/// файла у товара нет — автоматический выбор (`ocs::file_index_for`).
fn install_theme(handle: &Handle, item: ThemeItem, apply_after: bool, file_name: &str) -> (String, bool) {
    let mut item = item;
    // Плитки сетки и записи кеша приезжают БЕЗ списка файлов, а старый кеш мог
    // сохранить внешнюю ссылку вместо архива: обновляем детали, чтобы выбрать
    // настоящий архив для Linux.
    if item.download_links.is_empty() {
        let io = LiveOcsIo::new(&home());
        let preview_dir = io.preview_dir();
        let mut client = OcsClient::new(io, &preview_dir);
        if let Some(fresh) = client.get_item(&item.id) {
            item = fresh.item;
        }
    }

    // Имя — у того файла, который скачается: по нему названа тема без своей
    // папки, а записанное в плитку `download_name` могло остаться от
    // прежнего выбора.
    let file_index = rexon_core::ocs::file_index_for(&item, file_name);
    let name = rexon_core::ocs::file_name_at(&item, file_index).unwrap_or_else(|| {
        if item.download_name.is_empty() {
            format!("{}.archive", item.id)
        } else {
            item.download_name.clone()
        }
    });
    let Some(work) = mkdtemp("rexon-dl-") else {
        return (ap::install_download_failed(), false);
    };
    let dest = format!("{work}/{name}");

    // Подписи шага и отказа — из ядра: они одни на обе версии вкладки, а
    // «Скачивание…» вдобавок одно на весь продукт.
    say_log(handle, &rexon_core::we_window::download_start_text(""));
    // У товара несколько файлов — в журнале видно, какой из них взят.
    if let Some(note) = rexon_core::ocs::file_note_at(&item, file_index) {
        say_log(handle, &note);
    }
    let progress_handle = handle.clone();
    let net = UreqNet::new().with_progress(move |done, total| {
        if total > 0 {
            let pct = (done * 100 / total) as i32;
            post(&progress_handle, move |mut object| {
                object.as_mut().install_progress(pct);
            });
        }
    });
    let io = LiveOcsIo::with_net(net, &home());
    let preview_dir = io.preview_dir();
    let mut client = OcsClient::new(io, &preview_dir);
    let downloaded = client.download(&item, &dest, file_index, true);
    if !downloaded {
        let _ = std::fs::remove_dir_all(&work);
        return (ap::install_download_failed(), false);
    }
    post(handle, |mut object| {
        object.as_mut().install_progress(100);
    });

    let logger = log_sink(handle);
    let mut installer = LiveInstaller::new("", logger);
    let outcome = if apply_after {
        let (ok, mut msg) = installer.install_and_apply(&item, &dest);
        if ok {
            add_shell_palette_note(&item, &mut msg);
        }
        (ok, msg)
    } else {
        let (ok, msg, _names) = installer.install(&item, &dest);
        (ok, msg)
    };
    // ⚠️ Починка при переносе: временный каталог скачанного убирается ВСЕГДА.
    // У Python он оставался в `/tmp` после каждой установки (`mkdtemp` без
    // уборки), и за сессию каталог набивался архивами тем по десятку мегабайт.
    let _ = std::fs::remove_dir_all(&work);
    (outcome.1, outcome.0)
}

// ============================================================================
// Очередь установок AUR — одна на всё приложение
// ============================================================================
//
// Решения (по одной, кто следующий, подписи стадий) — ядро
// (`aur_install::AurQueue`, `stage_of_line`). Здесь — один поток-исполнитель
// на процесс и рассылка состояния всем живым страницам «Оформления»: очередь
// переживает и смену раздела, и закрытие страницы (живой случай 05.10.2026 —
// три параллельных помощника, три окна пароля, гонка зависимостей).

struct AurRunner {
    queue: Mutex<aur_install::AurQueue>,
    /// Страницы, которым рассказывать о ходе (закрытые отваливаются сами —
    /// по ошибке доставки).
    listeners: Mutex<Vec<(Handle, Arc<State>)>>,
}

fn aur_runner() -> &'static AurRunner {
    static RUNNER: std::sync::OnceLock<AurRunner> = std::sync::OnceLock::new();
    RUNNER.get_or_init(|| AurRunner {
        queue: Mutex::new(aur_install::AurQueue::new()),
        listeners: Mutex::new(Vec::new()),
    })
}

fn aur_subscribe(handle: Handle, state: Arc<State>) {
    if let Ok(mut list) = aur_runner().listeners.lock() {
        if !list.iter().any(|(_, s)| Arc::ptr_eq(s, &state)) {
            list.push((handle, state));
        }
    }
}

fn aur_snapshot() -> String {
    aur_runner().queue.lock().map(|q| q.snapshot_json()).unwrap_or_default()
}

/// Разослать состояние очереди всем страницам.
fn aur_broadcast() {
    let payload = aur_snapshot();
    if let Ok(mut list) = aur_runner().listeners.lock() {
        list.retain(|(handle, _)| {
            let payload = payload.clone();
            handle
                .queue(move |mut object| {
                    object.as_mut().aur_jobs_changed(QString::from(&payload));
                })
                .is_ok()
        });
    }
}

/// Поток-исполнитель: берёт установки по одной, пока очередь не опустеет.
/// Флаг «исполнитель работает» снимает сама очередь (`take_next`) под своим
/// замком — гонки «встала в очередь, а забрать некому» нет.
fn aur_worker() {
    loop {
        let job = match aur_runner().queue.lock() {
            Ok(mut q) => q.take_next(),
            Err(_) => return,
        };
        let Some(job) = job else {
            aur_broadcast();
            return;
        };
        aur_broadcast();
        rexon_core::log_info!(
            "aur",
            "сборка «{}»: пакеты {} (помощник {}, журнал {})",
            job.name,
            job.plan.packages.join(" "),
            job.helper,
            job.log_path
        );
        let started = std::time::Instant::now();
        let (ok, message, short) = run_aur_job(&job);
        if ok {
            rexon_core::log_info!("aur", "«{}» установлен за {:.0} с", job.name, started.elapsed().as_secs_f64());
        } else {
            rexon_core::log_warn!(
                "aur",
                "«{}» не установлен за {:.0} с: {}",
                job.name,
                started.elapsed().as_secs_f64(),
                if short.is_empty() { message.lines().next().unwrap_or("") } else { short.as_str() }
            );
        }
        if ok {
            // Новый пакет принёс темы: кешированный скан показал бы прежнее.
            if let Ok(list) = aur_runner().listeners.lock() {
                for (_, state) in list.iter() {
                    if let Ok(mut cache) = state.scan_cache.lock() {
                        cache.clear();
                    }
                }
            }
        }
        if let Ok(mut q) = aur_runner().queue.lock() {
            q.finish(&job.name, ok, &message, &short);
        }
        aur_broadcast();
    }
}

/// Новая стадия идущей установки — в очередь и, если изменилась, страницам.
fn aur_stage(name: &str, stage: Option<aur_install::AurStage>, current: Option<&str>) {
    let changed = aur_runner()
        .queue
        .lock()
        .map(|mut q| q.set_stage(name, stage, current))
        .unwrap_or(false);
    if changed {
        aur_broadcast();
    }
}

/// Сборка пакета (или комплекта) помощником: `(удалось, итог, коротко)`.
///
/// Команда, разбор отказа и тексты — ядро (`aur_install`). Здесь — замок
/// базы, запуск с выводом в файл, стадии по строкам вывода и сохранение
/// журнала в `~/.cache/rexon/aur/<пакет>.log`.
fn run_aur_job(job: &aur_install::AurJob) -> (bool, String, String) {
    let name = job.name.as_str();
    let plan = &job.plan;
    let helper = job.helper.as_str();
    // Соседний pacman (или хук снимка) держит замок базы — ждём, как ждёт
    // общая установка пакетов. ⚠️ Ждать приходится по-настоящему: snap-pac
    // держит `db.lck` ещё около тридцати секунд ПОСЛЕ конца чужой транзакции,
    // и начатая сразу сборка падала бы «база занята».
    let family = system::DistroDetector::detect_live().family_live();
    // Замок pacman — из настоящего `DBPath` (`pacman-conf`), а не жёсткий путь.
    let paths = system_plan::lock_paths_live(family);
    let mut waiter = system_plan::LiveLockWait::new(|_stage: &str, _message: &str| {});
    let (free, why) = system_plan::wait_for_pkg_lock(&mut waiter, &paths, PKG_LOCK_TIMEOUT_SECS);
    if !free {
        return (false, why.clone(), why);
    }
    aur_stage(name, Some(aur_install::AurStage::Downloading), None);
    let argv = aur_install::install_argv(helper, &plan.packages, plan.replace_conflicts);
    let log_path = job.log_path.clone();
    if let Some(dir) = std::path::Path::new(&log_path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let job_name = name.to_string();
    let mut tail = system_plan::tail_log(&log_path, system_plan::LogTailKind::Raw, move |_s, line| {
        if let Some(info) = aur_install::stage_of_line(line) {
            aur_stage(&job_name, info.stage, info.package.as_deref());
        }
    });
    utils::run(
        &utils::RunSpec {
            argv,
            output: utils::RunOutput::File(log_path.clone()),
            timeout_secs: Some(3600),
            // Вывод разбирается ядром (`aur_install::failure_cause`).
            lang_c: true,
            env: Vec::new(),
            stderr_to_stdout: true,
        },
        None,
    );
    tail.stop();
    // Достоверная проверка — по факту наличия пакета: помощник порой
    // возвращает ненулевой код и при успехе.
    if installed_packages().contains(name) {
        let msg = match plan.kit {
            Some(kit) => aur_install::kit_done_message(kit),
            None => i18n::trf("“%1” is installed", &[&name]),
        };
        return (true, msg, String::new());
    }
    let log = std::fs::read_to_string(&log_path).unwrap_or_default();
    // «Not authorized» у pkexec — и закрытое окно, и окно, которое агент
    // даже не открыл; настоящую причину агент пишет только в журнал системы.
    let cause = aur_install::refine_with_agent_journal(
        aur_install::failure_cause(&log),
        &journal_since(started),
    );
    // Команда для терминала — только сам пакет: помощник сам доберёт
    // зависимости, а строка остаётся короткой.
    let msg = aur_install::failure_message_for(
        name,
        helper,
        &[name.to_string()],
        &log,
        &log_path,
        cause.as_ref(),
    );
    (false, msg, aur_install::failure_short(cause.as_ref()))
}

/// Журнал системы и сеанса с момента `since` (секунды эпохи), только текст
/// сообщений. Пусто — journalctl нет или читать нечего.
fn journal_since(since: u64) -> String {
    if since == 0 || utils::which("journalctl", None).is_none() {
        return String::new();
    }
    let mut out = String::new();
    for user in [false, true] {
        let mut argv = vec!["journalctl".to_string()];
        if user {
            argv.push("--user".to_string());
        }
        argv.extend([
            "--no-pager".to_string(),
            "-o".to_string(),
            "cat".to_string(),
            "-n".to_string(),
            "5000".to_string(),
            format!("--since=@{since}"),
        ]);
        let report = utils::run(
            &utils::RunSpec {
                argv,
                output: utils::RunOutput::Capture,
                timeout_secs: Some(15),
                lang_c: true,
                env: Vec::new(),
                stderr_to_stdout: false,
            },
            None,
        );
        out.push_str(&String::from_utf8_lossy(&report.stdout));
    }
    out
}

// ============================================================================
// Спор за слот заставки при входе
// ============================================================================
//
// Видеозаставка входа Rexon («гибрид KSplash») и заставка KDE — ОДНО И ТО ЖЕ
// место: при входе показывается что-то одно. Гибрид вдобавок лечит себя сам
// при каждом входе, поэтому применить чужую заставку молча значило бы
// пообещать покупателю то, чего не будет: следующий вход вернул бы нашу.
//
// Решение, условия и все тексты — в ядре (`session_splash::splash_slot_*`).
// Здесь только показ вопроса тем же диалогом, каким спрашивает «Расширения»
// (`rexon_ext_confirm` — общий шим крейта), и исполнение ответа. Вопрос
// задаётся СИНХРОННО, до фоновой работы: диалог живёт в потоке интерфейса, а
// показывать его из рабочего потока нельзя.

/// Спросить покупателя, если применение займёт слот заставки при входе.
///
/// Ветвление «Да/Нет → что делать» — в ядре (`Splash::resolve_splash_slot`),
/// тот же вызов делает и Python-хозяин живой вкладки. Здесь только диалог.
///
/// Зовётся ТОЛЬКО из потока интерфейса.
fn splash_slot_answer(item: &ThemeItem) -> ss::SplashSlotPlan {
    let home = home();
    let kind = categories::category_by_id(std::path::Path::new(&home), item.category_id)
        .map(|c| c.apply_kind)
        .unwrap_or_default();
    let theme = ap::local_theme_name(&item.id, &item.name);
    let mut io = LiveSplashIo::new(&home, rexon_core::installer::PYTHON_FALLBACK_EXE);
    Splash::new(&mut io, &home).resolve_splash_slot(&kind, &theme, |conflict| {
        crate::extensions_actions::ask_confirm(
            &QString::from(conflict.title),
            &QString::from(&conflict.question),
        )
    })
}

/// Довести план до конца после применения (ядро: `Splash::finish_splash_slot`).
fn finish_splash_slot(keep_video: Option<String>, note: &str, ok: bool, msg: &mut String) {
    let home = home();
    let mut io = LiveSplashIo::new(&home, rexon_core::installer::PYTHON_FALLBACK_EXE);
    *msg = Splash::new(&mut io, &home).finish_splash_slot(keep_video.as_deref(), note, ok, msg);
}

/// Имя глобальной темы Plasma, которую «Применить» этой плитки включит, —
/// тот же выбор, что у [`AppearanceActions::apply_local_by_id`] и
/// [`AppearanceActions::act_on_store_item`]. `None` — плитка не глобальной
/// темы, товар ещё не поставлен, или тема старого формата (её отказ
/// приходит прежним путём, без окна).
fn global_target(item: &ThemeItem) -> Option<String> {
    if item.id.starts_with(cs::ID_PREFIX) {
        return None;
    }
    let home = home();
    let path = std::path::Path::new(&home);
    let cat = categories::category_by_id(path, item.category_id)?;
    if ainst::apply_action_for(Some(&cat), "") != ainst::ApplyAction::KdeGlobal {
        return None;
    }
    let name = if item.id.starts_with("local:") {
        ap::local_theme_name(&item.id, &item.name)
    } else {
        let installed = ap::live_logged_theme_name(&home, &item.id);
        let can_apply = ap::store_item_can_apply(path, item);
        match (ap::store_card_action(installed.is_some(), can_apply), installed) {
            (ap::StoreCardAction::ApplyInstalled, Some(name)) => name,
            _ => return None,
        }
    };
    if stale_global(item, &name).is_some() {
        return None;
    }
    Some(name)
}

/// Полная тема старого формата Plasma 5 — её Plasma 6 не применит.
/// `Some(имя)` — не применится.
fn stale_global(item: &ThemeItem, name: &str) -> Option<String> {
    let home = home();
    let cat = categories::category_by_id(std::path::Path::new(&home), item.category_id)?;
    if cat.apply_kind != "global" {
        return None;
    }
    if ainst::live_global_theme_missing_metadata_json(&home, name) {
        Some(name.to_string())
    } else {
        None
    }
}

/// Итог действия полосы «Палитра оболочки».
fn post_palette(handle: &Handle, msg: String, ok: bool) {
    post(handle, move |mut object| {
        object.as_mut().shell_palette_done(QString::from(&msg), ok);
    });
}

/// Применить цветовую схему по имени ТЕМ ЖЕ путём, что раздел «Цветовые
/// схемы» (`LiveInstaller::apply` с категорией схем): своей второй дороги
/// у палитры шелла нет.
fn apply_color_scheme(installer: &mut LiveInstaller, scheme: &str) -> (bool, String) {
    let Some(cat) = categories::category_by_kind(std::path::Path::new(&home()), "colors") else {
        return (false, String::new());
    };
    let item = ThemeItem {
        id: scheme.to_string(),
        name: scheme.to_string(),
        category_id: cat.id,
        ..Default::default()
    };
    installer.apply(&item, scheme)
}

/// Применили цветовую схему при живом шелле — сказать, что будет с его
/// палитрой (а если палитру ведёт Rexon, сразу передать её шеллу).
fn add_shell_palette_note(item: &ThemeItem, msg: &mut String) {
    let is_colors = categories::category_by_id(std::path::Path::new(&home()), item.category_id)
        .is_some_and(|c| c.apply_kind == "colors");
    if !is_colors {
        return;
    }
    if let Some(note) = palette::after_scheme_applied(palette::live_detect(), &palette::PaletteHome::live()) {
        if !msg.is_empty() {
            msg.push_str("\n");
        }
        msg.push_str(&note);
    }
}

/// Итог долгого действия: сбросить кеш сканов и отчитаться странице.
/// Снять товар магазина `content_id` по журналу установленных (системную
/// тему — помощником от root). `reload` — после итога перечитать раздел
/// (удаляли с плитки темы с диска).
fn spawn_uninstall(state: Arc<State>, handle: Handle, content_id: String, reload: bool) {
    std::thread::spawn(move || {
        let logger = log_sink(&handle);
        let mut installer = LiveInstaller::new("", logger);
        rexon_core::log_info!("theme", "удаление: {content_id}");
        // Включённую сейчас тему не снимаем и здесь: кнопка деталей могла
        // устареть (тему включили после открытия карточки).
        if !content_id.starts_with("local:") {
            let de = state.last_de.lock().map(|d| d.clone()).unwrap_or_default();
            if let Some(reason) = removal::live_store_blocked(&content_id, &de) {
                finish_action(&state, &handle, reason, false);
                return;
            }
        }
        // Спайс Cinnamon снимается целиком своим планом: выключить, каталог,
        // переводы, настройки (`cinnamon_spices::plan_uninstall`).
        let (ok, msg) = if content_id.starts_with(cs::ID_PREFIX) {
            drop(installer);
            cs::live_uninstall(&content_id, log_sink(&handle))
        } else {
            installer.uninstall(&content_id)
        };
        finish_action(&state, &handle, msg.clone(), ok);
        if reload {
            post(&handle, move |mut object| object.as_mut().removal_done(QString::from(&msg)));
        }
    });
}

/// Проба экрана входа (рабочий поток): что рисует экран входа, SteamOS ли
/// это и раздел «Менеджер входа» — установленные, основной, работающий,
/// кнопки и тексты (`system_exec::login_screen_view`). Ответ запоминается
/// и уходит сигналом `loginScreenReady`.
fn probe_login_screen_into(state: &Arc<State>, handle: &Handle) {
    let unit = ainst::active_login_manager();
    let steamos = rexon_core::distro_compat::running_on_steamos();
    let view = rexon_core::system_exec::live().login_screen_view();
    let mut answer = json!({
        "plasmaLogin": system_plan::is_plasma_login(&unit),
        "loginManager": unit,
        "steamos": steamos,
        "canOpen": !login_settings_commands().is_empty(),
        // Экран входа сам открывает или предлагает уходящий X11-сеанс —
        // полоса с кнопкой «Сделать … сеансом по умолчанию» (на SteamOS
        // раздел закрыт целиком).
        "waylandDefault": (!steamos)
            .then(system_plan::x11_login_default_live)
            .flatten()
            .map(system_plan::wayland_default_offer),
    });
    if let (Some(obj), Ok(Value::Object(extra))) = (answer.as_object_mut(), serde_json::to_value(&view)) {
        // Кнопки на SteamOS заперты и в ядре: она неизменяемая
        // (`DistroDetector::is_immutable`), а раздел там закрыт целиком.
        obj.extend(extra);
    }
    // Блок «Фон и оформление экрана входа»: при SDDM — через сеанс прав
    // окна, при Plasma Login Manager — через его собственный помощник
    // (KAuth), как его «Параметры системы».
    if system_plan::is_plasma_login(&unit) {
        let de = greeter_desktop(state);
        let greeter = rexon_core::plasma_login::greeter_view_live(&de);
        if let (Some(obj), Ok(v)) = (answer.as_object_mut(), serde_json::to_value(&greeter)) {
            obj.insert("plmGreeter".into(), v);
        }
    }
    if unit == "sddm" {
        let de = greeter_desktop(state);
        let look = system_plan::desktop_greeter_look_live(&de);
        let greeter = system_plan::sddm_greeter_view_live(&de, &look);
        if let (Some(obj), Ok(v)) = (answer.as_object_mut(), serde_json::to_value(&greeter)) {
            obj.insert("sddmGreeter".into(), v);
        }
    }
    let answer = answer.to_string();
    if let Ok(mut last) = state.login_screen.lock() {
        *last = answer.clone();
    }
    post(handle, move |mut object| {
        object.as_mut().login_screen_ready(QString::from(&answer));
    });
}

/// Стол, с которого переносится оформление на экран входа: тот, под
/// который смотрит вкладка; до первого обзора — запущенный сейчас.
fn greeter_desktop(state: &Arc<State>) -> String {
    let de = state.last_de.lock().map(|d| d.clone()).unwrap_or_default();
    if !de.is_empty() {
        return de;
    }
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    rexon_core::system::detect_current_de(
        &env("XDG_CURRENT_DESKTOP"),
        &env("DESKTOP_SESSION"),
        !env("WAYLAND_DISPLAY").is_empty(),
    )
    .unwrap_or_default()
}

impl qobject::AppearanceActions {
    /// Действие блока «Фон и оформление экрана входа»: в рабочем потоке,
    /// по одному (тот же замок, что у «Сделать основным»), итог —
    /// `loginLookDone`, следом свежая проба экрана входа.
    fn run_login_look(self: Pin<&mut Self>, work: impl FnOnce() -> Result<String, String> + Send + 'static) {
        use std::sync::atomic::Ordering;
        let mut this = self;
        let handle = this.as_mut().qt_thread();
        let state = this.rust().state.clone();
        if state.login_switching.swap(true, Ordering::SeqCst) {
            let msg = rexon_core::i18n::tr("The previous change isn't finished yet.");
            post(&handle, move |mut object| object.as_mut().login_look_done(false, QString::from(&msg)));
            return;
        }
        std::thread::spawn(move || {
            let (ok, msg) = match work() {
                Ok(t) => (true, t),
                Err(t) => (false, t),
            };
            let first = msg.lines().next().unwrap_or("");
            if ok {
                rexon_core::log_info!("login", "фон и оформление экрана входа: {first}");
            } else {
                rexon_core::log_warn!("login", "фон и оформление экрана входа, не вышло: {first}");
            }
            state.login_switching.store(false, Ordering::SeqCst);
            post(&handle, move |mut object| object.as_mut().login_look_done(ok, QString::from(&msg)));
            probe_login_screen_into(&state, &handle);
        });
    }
}

fn finish_action(state: &Arc<State>, handle: &Handle, msg: String, ok: bool) {
    // Итог установки/применения/удаления темы — одной строкой в журнал.
    let first = msg.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if ok {
        rexon_core::log_info!("theme", "итог: удалось — {first}");
    } else {
        rexon_core::log_warn!("theme", "итог: не удалось — {first}");
    }
    if ok {
        // Папка темы новая, и рядом с ней новая картинка: кешированный скан
        // показал бы прежнее, без превью и с чужим бейджем.
        if let Ok(mut cache) = state.scan_cache.lock() {
            cache.clear();
        }
    }
    post(handle, move |mut object| {
        object
            .as_mut()
            .action_done(QString::from(&msg), ok);
    });
}

// ============================================================================
// Мелочи
// ============================================================================

/// Доставить работу в поток интерфейса.
///
/// Ни `unwrap()`, ни предварительной проверки «жив ли объект»: между проверкой
/// и постановкой страница успевает закрыться, и это не ошибка — просто
/// отчитываться уже некому.
/// Превью экрана входа открыто (одно на процесс).
static LOGIN_PREVIEW_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Просьба закрыть превью («Закрыть предпросмотр»).
static LOGIN_PREVIEW_STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn post(
    handle: &Handle,
    work: impl FnOnce(Pin<&mut qobject::AppearanceActions>) + Send + 'static,
) {
    if let Err(e) = handle.queue(work) {
        if !matches!(e, cxx_qt::ThreadingQueueError::ObjectDestroyed) {
            rexon_core::log_warn!("appearance", "ответ вкладки «Оформление» не доставлен: {e:?}");
        }
    }
}

/// Строка журнала установки в интерфейс.
fn say_log(handle: &Handle, text: &str) {
    let text = text.to_string();
    post(handle, move |mut object| {
        object.as_mut().install_log(QString::from(&text));
    });
}

/// Приёмник журнала для живого установщика — тот же сигнал `installLog`.
fn log_sink(handle: &Handle) -> Box<dyn FnMut(&str) + Send> {
    let handle = handle.clone();
    Box::new(move |text: &str| say_log(&handle, text))
}

/// Сколько ждать освобождения замка пакетной базы — то же число, с которым
/// ждёт общая установка пакетов (`system_plan::wait_lock_step`). Своего здесь
/// быть не должно: два разных терпения на одну и ту же очередь.
const PKG_LOCK_TIMEOUT_SECS: u64 = 300;

fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// Живой обзор диска с сырьём кеша магазина: превью установленных тем
/// достаются оттуда, когда своего файла у темы нет.
fn live_scan() -> LiveScan {
    let mut cache = rexon_core::appearance_cache::LiveCache::open(&home());
    let ocs_cache = cache.all_items_min();
    let previews: HashMap<String, String> = cache.all_previews_min().into_iter().collect();
    let sections = cache.all_preview_categories();
    LiveScan::new(ocs_cache, previews).with_store_records(sections)
}

/// Собрать пакет AUR ТОЙ ЖЕ общей очередью, что «Установить» у плиток AUR
/// (по одной сборке на приложение), и дождаться итога. Для мест, которым
/// нужен пакет до следующего шага (части раскладки панели MATE). Звать не
/// из потока интерфейса.
pub(crate) fn aur_install_wait(name: &str) -> Result<(), String> {
    let family = system::DistroDetector::detect_live().family_live();
    let Some(helper) = system_plan::pick_aur_helper(family, &|bin| utils::which(bin, None).is_some()) else {
        return Err(ap::aur_no_builder());
    };
    let plan = aur_install::plan_install(name, &|_| false);
    let log_path = aur_install::log_path(&home(), name);
    let queued = match aur_runner().queue.lock() {
        Ok(mut q) => q.enqueue(name, plan, &helper, &log_path),
        Err(_) => return Err(ap::aur_no_builder()),
    };
    aur_broadcast();
    if queued.start_worker {
        std::thread::spawn(aur_worker);
    }
    // Сборка идёт до часа (`run_aur_job`); ждём чуть дольше.
    for _ in 0..(2 * 3600) {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let done = aur_runner().queue.lock().ok().and_then(|q| {
            q.jobs()
                .iter()
                .rev()
                .find(|j| j.name == name)
                .filter(|j| !j.stage.active())
                .map(|j| (j.stage == aur_install::AurStage::Done, j.message.clone()))
        });
        match done {
            Some((true, _)) => return Ok(()),
            Some((false, msg)) => return Err(msg.lines().next().unwrap_or("").to_string()),
            None => {}
        }
    }
    Err(i18n::trf("“%1” wasn't built in time", &[&name]))
}

/// Помощник AUR, спрошенный ОДИН раз за сессию вкладки (`shutil.which` —
/// дорогая операция на медленном диске, а ответ за сессию не меняется).
fn aur_helper(state: &Arc<State>) -> Option<String> {
    if let Ok(known) = state.aur_helper.lock() {
        if let Some(value) = known.as_ref() {
            return value.clone();
        }
    }
    let found = system_plan::pick_aur_helper(
        system::DistroDetector::detect_live().family_live(),
        &|bin| utils::which(bin, None).is_some(),
    );
    if let Ok(mut known) = state.aur_helper.lock() {
        *known = Some(found.clone());
    }
    found
}

/// Имена установленных пакетов — порт `aur_client.installed_set`.
fn installed_packages() -> std::collections::HashSet<String> {
    let family = system::DistroDetector::detect_live().family_live();
    let Some(query) = system_plan::installed_packages_query(family) else {
        return std::collections::HashSet::new();
    };
    if utils::which(&query.binary, None).is_none() {
        return std::collections::HashSet::new();
    }
    let report = utils::run(
        &utils::RunSpec {
            argv: query.cmd.argv.clone(),
            output: utils::RunOutput::Capture,
            timeout_secs: query.cmd.timeout_secs,
            lang_c: query.cmd.lang_c,
            env: Vec::new(),
            stderr_to_stdout: false,
        },
        None,
    );
    let ok = report.returncode == Some(0);
    system_plan::parse_installed_packages(ok, &utils::decode_text(&report.stdout))
        .into_iter()
        .collect()
}

/// Избранное — из своего хранилища, без сети и без скана диска.
fn favorite_items() -> Vec<ThemeItem> {
    favorites::list_items(&favorites::load_raw(&favorites::default_path()))
}

/// Состав избранного строкой JSON (список номеров).
fn favorite_ids() -> String {
    let ids: Vec<String> = favorite_items().into_iter().map(|it| it.id).collect();
    serde_json::to_string(&ids).unwrap_or_else(|_| "[]".to_string())
}

/// Запомнить выбранный рабочий стол между запусками.
///
/// Вместе с выбором пишется и стол, ЗАПУЩЕННЫЙ в момент выбора: по нему ядро
/// потом отличает выбор человека от унаследованного (`initial_de`).
fn remember_de(de_id: &str, seen: &str) -> bool {
    let Ok(mut settings) = SettingsManager::new() else {
        return false;
    };
    let seen = if seen.is_empty() {
        system::detect_current_de(
            &env_var("XDG_CURRENT_DESKTOP"),
            &env_var("DESKTOP_SESSION"),
            !env_var("WAYLAND_DISPLAY").is_empty(),
        )
        .unwrap_or_default()
    } else {
        seen.to_string()
    };
    if settings
        .set("appearance_de", Value::from(de_id))
        .and_then(|()| settings.set("appearance_de_seen", Value::from(seen.as_str())))
        .is_err()
    {
        return false;
    }
    true
}

/// Запомнить товары, отданные наверх (для действий по номеру).
fn remember_items(state: &Arc<State>, items: &[ThemeItem]) {
    let Ok(mut known) = state.items.lock() else {
        return;
    };
    for it in items {
        if !it.id.is_empty() {
            known.insert(it.id.clone(), it.clone());
        }
    }
}

fn remember_packages(state: &Arc<State>, pkgs: &[AurPackage]) {
    let Ok(mut known) = state.packages.lock() else {
        return;
    };
    for p in pkgs {
        if !p.name.is_empty() {
            known.insert(p.name.clone(), p.clone());
        }
    }
}

/// Скачанное превью — правда картинка? Вместо картинки сервер иногда отдаёт
/// страницу ошибки; такой файл удаляется (кеш превью сверяет наличие файла и
/// второй раз его не отдаст), а плитка берёт следующую картинку товара.
///
/// Судим только файлы СВОЕГО кеша превью: чужой файл (картинка местной темы)
/// не проверяется и тем более не удаляется.
fn usable_preview(path: &str, cache_dir: &str) -> bool {
    use std::io::Read;
    let ours = !cache_dir.is_empty()
        && std::path::Path::new(path).starts_with(std::path::Path::new(cache_dir));
    if !ours {
        return true;
    }
    let mut head = [0u8; 64];
    let n = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .unwrap_or(0);
    if ap::looks_like_image(&head[..n]) {
        return true;
    }
    let _ = std::fs::remove_file(path);
    false
}

/// Оборвать летящие запросы каталога, кроме запроса за `keep`.
fn abort_catalog(state: &Arc<State>, keep: &str) {
    let Ok(mut cancels) = state.catalog_cancels.lock() else {
        return;
    };
    let stale: Vec<String> = cancels.keys().filter(|k| *k != keep).cloned().collect();
    for key in stale {
        if let Some(token) = cancels.remove(&key) {
            token.cancel();
        }
    }
}

/// Тот ли товар всё ещё открыт.
fn still_open(state: &Arc<State>, id: &str) -> bool {
    state
        .detail_id
        .lock()
        .map(|open| *open == id)
        .unwrap_or(false)
}

/// Временный каталог с префиксом — то, что Python звал `tempfile.mkdtemp`.
fn mkdtemp(prefix: &str) -> Option<String> {
    let base = std::env::temp_dir();
    for attempt in 0..64u32 {
        let name = format!(
            "{prefix}{}-{attempt}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let path = base.join(name);
        if std::fs::create_dir(&path).is_ok() {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
}

/// Строковое поле запроса (пусто — поля нет).
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Числовое поле запроса.
fn num(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

/// Размер страницы запроса; своего умолчания у объекта нет — оно у ядра.
fn page_size(v: &Value) -> i64 {
    v.get("pageSize")
        .and_then(Value::as_i64)
        .unwrap_or(i64::from(ap::PAGE_SIZE))
}

// ============================================================================
// Карточка пакета AUR
// ============================================================================
//
// Что показывать и как разбирать — ядро (`aur::fetch_info`,
// `aur::parse_package_page`, `aur::detail_image_sources`). Здесь — кеш на
// сессию, пауза между обращениями к страницам сайта и доставка ответов
// странице теми же сигналами, что у товара магазина (кадры, комментарии,
// перевод описания).

impl qobject::AppearanceActions {
    fn load_aur_detail(mut self: Pin<&mut Self>, pkg_name: &QString) {
        let name = pkg_name.to_string();
        if name.is_empty() {
            return;
        }
        let state = self.as_ref().get_ref().rust().state.clone();
        if let Ok(mut open) = state.detail_id.lock() {
            *open = name.clone();
        }
        let tile = state.packages.lock().ok().and_then(|m| m.get(&name).cloned());
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let (info, error) = aur_info_cached(&state, &name);
            if !still_open(&state, &name) {
                return;
            }
            let payload = match (&info, &error) {
                (Some(i), _) => json!({ "itemId": name, "info": i }),
                (None, Some(e)) => json!({ "itemId": name, "error": e }),
                (None, None) => json!({ "itemId": name, "missing": true }),
            }
            .to_string();
            post(&handle, move |mut object| {
                object.as_mut().aur_detail_ready(QString::from(&payload));
            });

            // Проверка источника — своим потоком: в ней обращения к сайту
            // AUR (с паузами) и к GitHub, картинки и перевод её не ждут.
            spawn_aur_trust(&state, &handle, &name, info.clone());

            // Картинки: адрес проекта из паспорта, без него — с плитки.
            let upstream = info
                .as_ref()
                .map(|i| i.url.clone())
                .or_else(|| tile.as_ref().map(|p| p.upstream_url.clone()))
                .unwrap_or_default();
            let sources = aur::detail_image_sources(&upstream);
            if !sources.is_empty() {
                let io = LiveOcsIo::new(&home());
                let preview_dir = io.preview_dir();
                let mut client = OcsClient::new(io, &preview_dir);
                let mut got = 0usize;
                for src in &sources {
                    if !still_open(&state, &name) {
                        return;
                    }
                    let Some(url) = aur_image_url(&state, src) else { continue };
                    if emit_frame(&handle, &mut client, &got.to_string(), &url) {
                        got += 1;
                    }
                }
                let count = got.to_string();
                post(&handle, move |mut object| {
                    object.as_mut().gallery_finished(QString::from(&count));
                });
            }

            // Перевод описания — тем же вторым проходом, что у товара
            // магазина; имя пакета и сопровождающего движку не отдаются.
            let base = info.map(|i| i.as_package()).or(tile);
            if let Some(pkg) = base {
                let item = ThemeItem {
                    id: name.clone(),
                    name: pkg.name.clone(),
                    author: pkg.maintainer.clone(),
                    description: pkg.description.clone(),
                    ..Default::default()
                };
                // Охрана имени при переводе комментариев берёт товар отсюда.
                remember_items(&state, std::slice::from_ref(&item));
                translate_detail_description(&state, &handle, &item);
            }
        });
    }

    fn load_aur_pkgbuild(mut self: Pin<&mut Self>, pkg_name: &QString, pkg_base: &QString) {
        let name = pkg_name.to_string();
        let base = pkg_base.to_string();
        let base = if base.is_empty() { name.clone() } else { base };
        if name.is_empty() {
            return;
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            static TEXTS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
            let cached = TEXTS.lock().ok().and_then(|m| m.as_ref()?.get(&base).cloned());
            let answer = match cached {
                Some(t) => Ok(t),
                None => {
                    aur_site_turn();
                    let mut net = UreqNet::new();
                    let got = rexon_core::aur_trust::fetch_pkgbuild(&mut net, &base);
                    if let (Ok(t), Ok(mut m)) = (&got, TEXTS.lock()) {
                        m.get_or_insert_with(HashMap::new).insert(base.clone(), t.clone());
                    }
                    got
                }
            };
            let payload = match answer {
                Ok(text) => json!({ "itemId": name, "text": text }),
                Err(e) => json!({ "itemId": name, "error": e }),
            }
            .to_string();
            post(&handle, move |mut object| {
                object.as_mut().aur_pkgbuild_ready(QString::from(&payload));
            });
        });
    }

    fn load_aur_comments(mut self: Pin<&mut Self>, pkg_name: &QString, page: &QString) {
        let name = pkg_name.to_string();
        let page: i64 = page.to_string().parse().unwrap_or(0);
        let state = self.as_ref().get_ref().rust().state.clone();
        let view = mark_comments_view(&state, &name, page);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let answer = aur_page_cached(&state, &name, page);
            if !comments_shown(&state, &name, view) {
                return;
            }
            let mut out = Map::new();
            out.insert("itemId".to_string(), Value::from(name.as_str()));
            out.insert("page".to_string(), Value::from(page));
            let texts: Vec<String> = match &answer {
                Some(p) => {
                    let rows: Vec<Value> = p
                        .comments
                        .iter()
                        // Закреплённые повторяются на каждой странице сайта —
                        // показываем их один раз, наверху первой.
                        .filter(|c| page == 0 || !c.pinned)
                        .map(|c| {
                            json!({
                                "author": c.author,
                                "date": c.date,
                                "text": c.text,
                                "depth": 0,
                                "pinned": c.pinned,
                                "translated": "",
                            })
                        })
                        .collect();
                    out.insert("comments".to_string(), Value::Array(rows));
                    out.insert("hasMore".to_string(), Value::from(p.has_more));
                    out.insert("comaintainers".to_string(), json!(p.comaintainers));
                    p.comments
                        .iter()
                        .filter(|c| page == 0 || !c.pinned)
                        .map(|c| c.text.clone())
                        .collect()
                }
                None => {
                    // Страницу не дали (защита от ботов, нет сети): честное
                    // «комментарии — на сайте», а не «комментариев нет».
                    out.insert("comments".to_string(), Value::Array(Vec::new()));
                    out.insert("blocked".to_string(), Value::from(true));
                    Vec::new()
                }
            };
            let payload = Value::Object(out).to_string();
            post(&handle, move |mut object| {
                object.as_mut().comments_ready(QString::from(&payload));
            });
            translate_comment_page(&state, &handle, &name, page, view, &texts);
        });
    }
}

/// Проверка источника пакета (`aur_trust`) в своём потоке: кеш на диске
/// сутки, иначе сайт AUR (через общую очередь страниц) и API GitHub без
/// входа. Без паспорта RPC проверять нечего — карточка узнаёт, что
/// проверка кончилась. Ответ про закрытую карточку не шлётся.
fn spawn_aur_trust(
    state: &Arc<State>,
    handle: &Handle,
    name: &str,
    info: Option<aur::AurInfo>,
) {
    let state = state.clone();
    let handle = handle.clone();
    let name = name.to_string();
    std::thread::spawn(move || {
        let entry = info.map(|info| {
            let mut aur_net = UreqNet::new();
            let mut gh_net = UreqNet::new().with_headers(rexon_core::aur_trust::github_api_headers());
            let mut pause = aur_site_turn;
            let mut f = rexon_core::aur_trust::Fetchers {
                aur: &mut aur_net,
                github: &mut gh_net,
                pause: &mut pause,
            };
            let path = rexon_core::aur_trust::live_cache_path();
            rexon_core::aur_trust::check_cached(&path, &mut f, &info, rexon_core::aur_trust::now_secs())
        });
        if !still_open(&state, &name) {
            return;
        }
        let payload = match entry {
            Some(e) => json!({ "itemId": name, "entry": e }),
            None => json!({ "itemId": name, "failed": true }),
        }
        .to_string();
        post(&handle, move |mut object| {
            object.as_mut().aur_trust_ready(QString::from(&payload));
        });
    });
}

/// Свежая запись кеша карточки AUR.
fn aur_fresh(at: std::time::Instant) -> bool {
    at.elapsed().as_secs() < aur::DETAIL_CACHE_SECS
}

/// Паспорт пакета: из кеша сессии либо запросом RPC. `(паспорт, ошибка)`:
/// оба пусты — AUR пакета не знает.
fn aur_info_cached(state: &Arc<State>, name: &str) -> (Option<aur::AurInfo>, Option<String>) {
    if let Some((at, info)) = state.aur_info.lock().ok().and_then(|m| m.get(name).cloned()) {
        if aur_fresh(at) {
            return (info, None);
        }
    }
    let mut net = UreqNet::new();
    match aur::fetch_info(&mut net, name) {
        Ok(info) => {
            if let Ok(mut m) = state.aur_info.lock() {
                m.insert(name.to_string(), (std::time::Instant::now(), info.clone()));
            }
            (info, None)
        }
        Err(e) => (None, Some(e.to_string())),
    }
}

/// Очередь к страницам сайта AUR (комментарии, журнал и `.SRCINFO` для
/// проверки источника, PKGBUILD): не чаще, чем раз в `aur::PAGE_PAUSE_MS`
/// на весь процесс. Замок держится и на время паузы: два запроса подряд
/// встают в очередь, а не уходят на сайт разом.
fn aur_site_turn() {
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let Ok(mut last) = LAST.lock() else { return };
    let since = last.map(|t| t.elapsed().as_millis() as u64);
    let wait = aur::page_pause_left_ms(since);
    if wait > 0 {
        std::thread::sleep(std::time::Duration::from_millis(wait));
    }
    *last = Some(std::time::Instant::now());
}

/// Страница пакета: из кеша сессии либо с сайта — не чаще, чем раз в
/// `aur::PAGE_PAUSE_MS` на весь процесс. `None` — страницы не дали.
fn aur_page_cached(state: &Arc<State>, name: &str, page: i64) -> Option<aur::AurPage> {
    let key = (name.to_string(), page);
    if let Some((at, p)) = state.aur_pages.lock().ok().and_then(|m| m.get(&key).cloned()) {
        if aur_fresh(at) {
            return Some(p);
        }
    }
    aur_site_turn();
    let mut net = UreqNet::new();
    match aur::fetch_package_page(&mut net, name, page) {
        Ok(p) => {
            if let Ok(mut m) = state.aur_pages.lock() {
                m.insert(key, (std::time::Instant::now(), p.clone()));
            }
            Some(p)
        }
        Err(e) => {
            rexon_core::log_warn!("aur", "страница пакета AUR «{name}» не получена: {e:?}");
            None
        }
    }
}

/// Прямой адрес картинки карточки: метки README и `og:image` разрешаются
/// (один раз за сессию), обычный адрес — как есть.
fn aur_image_url(state: &Arc<State>, src: &str) -> Option<String> {
    let is_marker =
        src.starts_with(aur::README_PREVIEW_SCHEME) || src.starts_with(aur::OG_IMAGE_SCHEME);
    if !is_marker {
        return Some(src.to_string());
    }
    if let Some(found) = state.aur_images.lock().ok().and_then(|m| m.get(src).cloned()) {
        return found;
    }
    let mut net = UreqNet::new();
    let found = if src.starts_with(aur::README_PREVIEW_SCHEME) {
        aur::resolve_readme_preview(&mut net, src)
    } else {
        aur::resolve_og_image(&mut net, src)
    };
    if let Ok(mut m) = state.aur_images.lock() {
        m.insert(src.to_string(), found.clone());
    }
    found
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Договор со страницей: у ответа каталога ОБЯЗАН быть ключ запроса.
    /// Ядро сверяет его с действующим (`requests.answer`) и ответ без ключа
    /// молча отбрасывает — витрина осталась бы пустой при живом хозяине.
    #[test]
    fn catalog_answer_always_carries_the_request_key() {
        let req = json!({
            "key": "splash:0:new::",
            "kind": "local",
            "which": "нет-такого-скана",
            "query": "",
        });
        let state = Arc::new(State::default());
        let answer: Value = serde_json::from_str(&scan_local(&state, &req)).unwrap();
        assert_eq!(answer["key"], Value::from("splash:0:new::"));
        assert!(answer["items"].is_array(), "витрина обязана быть списком");
        assert_eq!(answer["total"], Value::from(0));
    }

    /// Отказ AUR при коротком запросе называется СЛОВАМИ ЯДРА, а не своей
    /// копией текста: подпись одна на обе версии вкладки.
    #[test]
    fn short_aur_query_is_refused_with_core_words() {
        // Язык процесса общий, и соседние сверки крейта ставят исходный:
        // без закрепления на русской локали подпись менялась между двумя
        // вызовами `tr` и сверка плавала.
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let req = json!({"key": "aur:0", "kind": "aur", "query": "п", "aurKey": "x"});
        let state = Arc::new(State::default());
        let answer: Value = serde_json::from_str(&fetch_aur(&state, &req, &CancelToken::new()))
            .unwrap();
        assert_eq!(answer["error"], Value::from(ap::aur_query_too_short_msg()));
        // ⚠️ Сети тут быть не должно вовсе: короткий запрос отсекается ДО
        // похода в RPC, иначе каждая буква в строке поиска — запрос наружу.
        assert_eq!(answer["total"], Value::from(0));
    }

    /// Местная тема приезжает на страницу СО СВОИМ видом (`localKind`): по
    /// нему у неё другая кнопка и другой бейдж. Забудь его — и тема с диска
    /// показала бы кнопку «Установить», которой у неё нет.
    #[test]
    fn local_card_names_its_kind() {
        // Язык процесса общий, и соседние сверки крейта ставят исходный:
        // без закрепления на русской локали подпись менялась между двумя
        // вызовами `tr` и сверка плавала.
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let item = ThemeItem {
            id: "local:active:Breeze".to_string(),
            name: "Breeze".to_string(),
            author: ap::local_active_badge(),
            ..Default::default()
        };
        let row = local_row(&item, "splash");
        assert_eq!(row["localKind"], Value::from("active"));
        // Вид «Загруженные» у того же скана — когда бейджа в подписи нет.
        let plain = ThemeItem {
            id: "local:splash:Другая".to_string(),
            name: "Другая".to_string(),
            ..Default::default()
        };
        assert_eq!(local_row(&plain, "splash")["localKind"], Value::from("downloaded"));
        // Имя без перевода в файлы не пишется, но поиску страницы нужно —
        // едет отдельным ключом, и только когда оно есть.
        assert!(local_row(&plain, "splash").get("sourceName").is_none());
        let translated = ThemeItem {
            id: "local:gtk:HighContrast".to_string(),
            name: "Высококонтрастная".to_string(),
            source_name: "High Contrast".to_string(),
            ..Default::default()
        };
        assert_eq!(local_row(&translated, "system")["sourceName"], Value::from("High Contrast"));
    }

    /// Состав избранного — список СТРОК, и пустое хранилище даёт `[]`, а не
    /// пустую строку: страница разбирает ответ как JSON и на пустой строке
    /// потеряла бы все звёздочки разом.
    #[test]
    fn favorites_answer_is_always_a_json_list() {
        let raw = favorite_ids();
        let parsed: Value = serde_json::from_str(&raw).expect("избранное обязано быть JSON");
        assert!(parsed.is_array(), "избранное обязано быть списком: {raw}");
        for id in parsed.as_array().unwrap() {
            assert!(id.is_string(), "номер избранного обязан быть строкой");
        }
    }
}
