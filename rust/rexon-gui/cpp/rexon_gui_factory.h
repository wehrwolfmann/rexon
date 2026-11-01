// Фабрика дочерних QObject'ов для cxx-qt-моста.
//
// Зачем шим: cxx-qt не умеет создавать экземпляр СВОЕГО же QObject из Rust —
// сгенерированный класс существует только в C++, а `new` там. Ровно этой
// операции требует архитектура вендорского QML: запрос владеет моделью и
// отдаёт её свойством (`Q_PROPERTY(Model* model READ model CONSTANT)`), как
// сделано у waywallen и в Python-мосте (RemoteSearchQuery.model).
//
// Шим обобщённый по смыслу, но не по типу: каждый новый владеемый объект
// добавляет сюда одну функцию — это дешевле, чем тащить шаблоны через cxx.
#pragma once

class QObject;

/// Создать RemoteListModel. parent может быть nullptr — тогда владение
/// остаётся у зовущего и объект обязан быть снят rexon_delete_object().
/// Нулевой родитель нужен стенду поздних колбэков: он специально удаляет
/// модель, пока ответ ещё в полёте.
QObject* rexon_new_remote_list_model(QObject* parent);

/// То же самое, что rexon_new_remote_list_model, но под другим именем — для
/// моста запросов Discover.
///
/// ⚠️ Дублирование вынужденное и НЕ подлежит «упрощению»: cxx печатает
/// шим-обёртку на каждое объявление `extern "C++"`, а имя шима собрано по
/// имени C++-функции. Одна и та же функция, объявленная в ДВУХ мостах
/// одного крейта, даёт `duplicate symbol` на линковке — ровно тот же класс
/// граблей, что с `#[qobject]`-базой QAbstractListModel.
QObject* rexon_new_search_result_model(QObject* parent);

/// Пропускать окну нажатия мыши ВНЕ `area`, которые съел бы модальный слой
/// Material (`Qcm.Material.Overlay`).
///
/// Зачем шим: слой Material один на окно и нажатия блокирует фильтром событий
/// самого окна — раньше, чем их увидит любой элемент сцены. Для waywallen
/// окно и есть весь интерфейс, а у нас в том же окне над вкладкой обоев
/// стоят разделы Rexon: нажатие на них при открытом окне поверх («Плагины»,
/// «Настройки», «О программе», любой модальный вопрос) закрывало его, а само
/// пропадало. Опередить чужой фильтр событий можно только своим фильтром —
/// из QML и из Rust этого не сделать. Решений здесь нет: «вне `area`» —
/// геометрия, остальное делает сам слой.
///
/// `area` — элемент сцены (`QQuickItem`), вне которого нажатия пропускаются;
/// повторный вызов безопасен (фильтр переставляется первым в очереди — он
/// обязан стоять ПОСЛЕ фильтра слоя, а тот заводится лениво).
void rexon_pass_outside_presses(QObject* area);

/// Значение QVariant в JSON — так, как его увидит QML.
///
/// Шим, а не разбор на Rust: `QVariant::value<T>()` в cxx-qt-lib проверяет
/// `canConvert`, а он у Qt разрешающий — строка «конвертируется» и в bool, и
/// в список посимвольно, поэтому определить НАСТОЯЩИЙ тип значения им
/// нельзя. QJsonDocument::fromVariant делает ровно то, что нужно, и делает
/// это правилами самого Qt.
class QByteArray;
class QVariant;
QByteArray rexon_variant_to_json(const QVariant& value);

/// Создать запросы страницы Discover (партия 3). parent может быть nullptr —
/// тогда объект снимается rexon_delete_object(); из QML их создаёт сам
/// движок, а фабрика нужна стенду и будущему шиму регистрации.
QObject* rexon_new_remote_availability_query(QObject* parent);
QObject* rexon_new_remote_search_query(QObject* parent);
QObject* rexon_new_remote_details_query(QObject* parent);
QObject* rexon_new_remote_download_query(QObject* parent);

/// Одноразовый таймер отложенной перезагрузки запроса (qextra query.cpp: 100
/// мс). По срабатыванию зовёт Q_INVOKABLE-метод `reload()` владельца, поэтому
/// подряд идущие смены свойств склеиваются в ОДИН запрос.
///
/// Шим, а не Rust: обёртки QTimer в cxx-qt-lib 0.9 нет вовсе (в core/ есть
/// QDate/QTime, но не таймер), а заводить ради дебаунса собственный
/// #[qobject]-таймер дороже, чем четыре строки на C++.
QObject* rexon_new_reload_debounce(QObject* owner, int interval_ms);

/// Те же две функции под другими именами — для моста страницы «Обои».
///
/// ⚠️ Дублирование вынужденное и НЕ подлежит «упрощению»: cxx печатает
/// шим-обёртку на каждое объявление `extern "C++"`, а имя шима собрано по
/// имени C++-функции. Одна и та же функция, объявленная в ДВУХ мостах
/// одного крейта, даёт `duplicate symbol` на линковке — тот же класс
/// граблей, что с фабрикой модели выдачи.
QObject* rexon_new_wallpaper_debounce(QObject* owner, int interval_ms);
void rexon_restart_wallpaper_debounce(QObject* timer);

/// Третья пара имён — для моста запросов-действий страницы «Обои».
QObject* rexon_new_action_debounce(QObject* owner, int interval_ms);
void rexon_restart_action_debounce(QObject* timer);

/// Четвёртая пара — для моста плейлистов (`PlaylistDetailQuery`, 0.3.7).
QObject* rexon_new_playlist_debounce(QObject* owner, int interval_ms);
void rexon_restart_playlist_debounce(QObject* timer);

/// Перезапустить таймер дебаунса (start() у одноразового QTimer сбрасывает
/// отсчёт — именно это и нужно: считаем от ПОСЛЕДНЕЙ смены свойства).
void rexon_restart_debounce(QObject* timer);

/// Модель библиотеки обоев или nullptr, если объект другой.
///
/// Проверенное приведение (qobject_cast), а не голый каст указателя:
/// свойство `model` хранилища выбора пишет QML, и подсунуть туда можно
/// любой QObject. Оригинал поступает так же (qextra select_storage.cpp:16).
QObject* rexon_as_wallpaper_list_model(QObject* object);

/// Создать WallpaperListModel как ребёнка запроса-владельца.
QObject* rexon_new_wallpaper_list_model(QObject* parent);

/// Те же две функции под другими именами — для моста плейлистов
/// (`PlaylistDetailQuery` владеет своей моделью состава) и для моста
/// хранилища выбора состава плейлиста. Причина дублирования — та же, что
/// у пар выше: имя шима cxx собрано по имени C++-функции, и одно и то же
/// объявление в двух мостах даёт `duplicate symbol` на линковке.
QObject* rexon_new_playlist_wallpaper_list_model(QObject* parent);
QObject* rexon_as_playlist_wallpaper_list_model(QObject* object);

/// Восемь объектов страницы «Обои» для стенда голдена
/// (src/wallpaper_page_selftest.rs). В живом Rexon их создаёт QML — там
/// фабрика не нужна, а стенду нужны те же самые классы без окна и без QML.
QObject* rexon_new_wallpaper_list_query(QObject* parent);
QObject* rexon_new_wallpaper_get_query(QObject* parent);
QObject* rexon_new_wallpaper_apply_query(QObject* parent);
QObject* rexon_new_wallpaper_remove_query(QObject* parent);
QObject* rexon_new_wallpaper_scan_query(QObject* parent);
QObject* rexon_new_wallpaper_property_set_query(QObject* parent);
QObject* rexon_new_wallpaper_layout_set_query(QObject* parent);
QObject* rexon_new_wallpaper_apply_via_portal_query(QObject* parent);
/// waywallen 0.4.4: «скрыть обои» — проверка v0.4.4 того же стенда.
QObject* rexon_new_wallpaper_hide_query(QObject* parent);
/// waywallen 0.5.0: запросы плейлистов — проверка v0.5.0 того же стенда
/// (`saveSmart`, поколения ответов, молчание без номера).
QObject* rexon_new_playlist_list_query(QObject* parent);
QObject* rexon_new_playlist_detail_query(QObject* parent);
QObject* rexon_new_playlist_mutation_query(QObject* parent);

/// Удалить QObject, созданный без родителя. Через delete, а не
/// deleteLater(): стенду нужна ИМЕННО немедленная смерть объекта, чтобы
/// поздний колбэк пришёл на заведомо мёртвый адрес.
void rexon_delete_object(QObject* object);

/// Прокрутить цикл событий не дольше ms миллисекунд. cxx-qt-lib не даёт
/// обёртки QCoreApplication::processEvents, а стенду поздних колбэков нужна
/// именно она: очередь CxxQtThread разбирается циклом событий.
void rexon_process_events(int ms);

// ── Точка входа: то, чего в cxx-qt-lib 0.9 нет вовсе ───────────────────
//
// Обе функции с сишной линковкой намеренно: их зовёт бинарь главного окна
// напрямую (`extern "C"` в Rust), без моста cxx. Мост здесь был бы лишним
// звеном — ни QObject'ов, ни типов Qt через границу не ходит, только
// строки и числа.
extern "C" {

/// Каталог переводов САМОЙ Qt (`QLibraryInfo::TranslationsPath`) в UTF-8.
///
/// Пишет в `buf` не больше `len - 1` байт и нуль; → полная длина пути
/// (больше `len - 1` — буфер мал, позвать ещё раз с нужным размером).
///
/// Шим, а не Rust: обёртки QLibraryInfo в cxx-qt-lib 0.9 нет. Какие файлы
/// из этого каталога грузить, решает ядро (`rexon_core::cli::
/// qt_translation_files` по языку Rexon), а не QLocale::system().
int rexon_qt_translations_dir(char* buf, int len);

/// Включить каталоги переводов САМОЙ Qt — кнопки, которые рисует Qt
/// («OK», «Cancel», «Show Details…», меню правки в полях ввода).
///
/// `paths` — полные пути `.qm`, которые назвало ядро, в порядке загрузки;
/// `count == 0` — английский: только снять прежние. Прежний набор снимается
/// всегда (смена языка). Ни языка, ни отката здесь не выбирается: это
/// решение ядра, сюда приезжают готовые пути. → сколько загрузилось
/// (-1 — приложения ещё нет).
///
/// ⛔ Переводчик обязан быть объектом Qt, а не питоновским: питоновский
/// зовётся из потока загрузки QML и намертво сцепляет потоки через GIL
/// (канон PySide6). Здесь его и нет — только C++.
int rexon_install_qt_translations(const char* const* paths, int count);

/// Приёмник строки перевода: `target` — то, что передал переводчик, `utf8`
/// и `len` — готовый перевод (не нуль-терминирован).
typedef void (*RexonTextSink)(void* target, const char* utf8, int len);

/// Поиск строки QML в каталоге ядра (`rexon_core::i18n::qt_translate`):
/// `disambiguation` — уточнение (`nullptr` — нет), `n` — число (`< 0` —
/// нет). Нашлась — отдать её в `sink(target, …)` и вернуть true; false —
/// перевода нет (Qt покажет исходник).
typedef bool (*RexonAppTranslate)(const char* disambiguation, const char* source, int n,
                                  RexonTextSink sink, void* target);

/// Переводчик САМОГО Rexon — для `qsTranslate("rexon", …)` в QML.
///
/// Своего каталога (`.qm`) у него НЕТ: каждую строку контекста `context`
/// он спрашивает у ядра через `lookup`, а ядро ищет её в том же `.mo`, что
/// и для строк на Rust. Перевод хранится в одном месте и разойтись не может.
/// Чужие контексты (вендорный waywallen, сама Qt) он пропускает дальше, не
/// заходя в Rust.
///
/// Переводчик ровно один: повторный вызов снимает прежний (вместе с его
/// кэшем) и ставит новый — смена языка. `lookup == nullptr` — только снять:
/// у английского (исходного) каталога нет. → false, если приложения нет.
///
/// Зовётся Qt из любого потока (QQmlThread тоже): `lookup` обязан быть
/// потокобезопасным, кэш строк здесь — под замком.
bool rexon_install_app_translator(const char* context, RexonAppTranslate lookup);

/// Осушить ОБЩИЙ пул потоков Qt, не дольше ms миллисекунд.
/// → сколько задач НЕ дождались (0 — пул пуст).
///
/// Рабочие потоки QThreadPool заводит C++, и своим счётчиком фоновых задач
/// (`rexon_core::exit_guard`) их не видно в принципе — отсюда отдельный
/// досчёт. В пуле сидят задачи Qt Quick (асинхронные загрузчики картинок и
/// делегатов): в кордампе 2292278 восемь его потоков стояли в
/// QWaitCondition::wait.
///
/// ⚠️ Исход ОБЯЗАН попасть в ответ. Ждать пул ради побочного действия, а
/// отвечать только числом своих задач — значит сказать «фон пуст» при
/// работающем фоне (замер 22.08.2026 на Python-части): сторож выхода тогда
/// не обрывает процесс, и выход подвисает до конца задачи.
int rexon_drain_thread_pool(int ms);

/// Остановить и разгрузить ВСЕ проигрыватели (`QMediaPlayer`, в QML —
/// `MediaPlayer`) во всех окнах процесса. → сколько нашлось.
///
/// Зовётся из потока GUI после выхода из цикла событий и ДО роспуска движка
/// QML. Разгрузка (`setSource(QUrl())`) синхронна: Qt сносит движок
/// воспроизведения и дожидается его потоков декодирования — к возврату
/// отсюда ни один поток FFmpeg больше не работает. Почему это нельзя
/// оставить деструкторам — в `main_window.rs` у вызова.
int rexon_stop_media_players();

}  // extern "C"
