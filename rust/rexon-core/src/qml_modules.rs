//! Где лежат собранные QML-плагины и Rust-сборки вкладок — порт
//! `rexon/qt_wallpaper_engine/qml_ui/__init__.py`, части `qml_ui/bridge.py`
//! и `rexon/qt_main/__init__.py`.
//!
//! Вкладка «Wallpaper Engine» показывает вендоренный QML waywallen, а типы
//! этого QML печатают три отдельные сборки: `Qcm.Material` (C++, MIT),
//! `waywallen.control` (Qt-Protobuf value-типы) и наш крейт
//! `rexon-qt-filter` (типы `rexon-gui` вшиты в само окно статически,
//! отдельной `.so` у него нет). Четвёртая — необязательная: настоящий плагин
//! `QExtra` автора (C++); без него работает наша QML-подмена, выбор —
//! [`choose_qextra`]. Все они собираются вне колеса и раскладываются по
//! каталогам, которые зависят от того, откуда Rexon поставлен: дерево
//! разработчика, пользовательская сборка скриптами `tools/build_qml_*.sh`
//! или пакет дистрибутива. Вопрос «где именно» — единственное РЕШЕНИЕ,
//! которое оставалось в этом слое на Python.
//!
//! Копий этого решения было ТРИ, и они уже разъехались (правило #12):
//! `qml_ui.rexon_gui_candidates`, `bridge._rust_filter_model_candidates` и
//! `qt_main.rust_gui_candidates`. Теперь список один; Python остаётся
//! исполнителем — `os.path.isfile`, `glob` и `ctypes.CDLL`.
//!
//! ⚠️ Починки оригинала (правило #16), каждая помечена `Починка:` по месту:
//!   1. `qt_main.rust_gui_candidates` не знал ни `$XDG_DATA_DIRS`, ни
//!      `/usr/lib64`, ни мультиарх-каталога `/usr/lib/<триплет>` — то есть
//!      нёс ровно тот дефект поиска только в `/usr/lib`,
//!      который во второй копии уже был починен. На Fedora, openSUSE,
//!      Debian и Ubuntu пакетная `librexon_gui.so` не находилась, QML-модуль
//!      `org.rexon.gui` не поднимался — и на виджеты откатывались ВСЕ
//!      перенесённые вкладки, не только вкладка обоев;
//!   2. `.so` наших крейтов искались только в ОДНОМ каталоге модулей — том,
//!      где нашёлся `Qcm.Material`. Не собран `Qcm.Material` — каталог
//!      схлопывался в первый кандидат (`~/.local/share/…`), и лежащая в
//!      `/usr/lib/rexon/qml_modules/waywallen/control/librexon_gui.so`
//!      сборка не проверялась вовсе: транспорт демона был недостижим при
//!      живом файле на диске. Теперь каждый файл ищется по ВСЕМУ списку;
//!   3. пользовательский каталог модулей брался от `~/.local/share`
//!      литералом, мимо `$XDG_DATA_HOME` — тот же класс, что починенный
//!      `$XDG_CONFIG_HOME` у юнита автозапуска в `we_client`. Теперь
//!      уважается и он, а `~/.local/share` остаётся вторым кандидатом:
//!      `tools/build_qml_*.sh` кладут сборку именно туда.
//!
//! Сверка — `rust/rexon-core/tests/qml_modules_parity.rs`.

use crate::utils::data_home;

// ============================================================================
// Живые проверки ФС
// ============================================================================

/// Проверки файловой системы для поиска сборок. Ядро в ФС не лезет само —
/// тот же приём, что [`crate::we_client::DaemonFs`]: порядок внутри,
/// side-effect снаружи.
pub trait ModuleFs {
    /// `os.path.isfile(path)` — существующий обычный файл.
    fn is_file(&mut self, path: &str) -> bool;

    /// Под какую версию Qt (старшая, младшая) собран плагин по этому пути —
    /// разбор [`plugin_qt_version`] над содержимым файла. `None` — не
    /// прочитать или не QML-плагин: тогда проверка версии не мешает.
    fn plugin_qt_version(&mut self, _path: &str) -> Option<(u32, u32)> {
        None
    }

    /// Версия Qt (старшая, младшая), на которой процесс работает сейчас.
    /// `None` — неизвестна, проверка версии не мешает.
    fn running_qt_version(&mut self) -> Option<(u32, u32)> {
        None
    }

    /// Содержимое малого текстового файла (отметка сборки модуля,
    /// [`CONTROL_STAMP_REL`]). `None` — файла нет или он не прочитался.
    fn read_text(&mut self, _path: &str) -> Option<String> {
        None
    }
}

// ============================================================================
// Версия Qt, под которую собран плагин
// ============================================================================
//
// ⚠️ Зачем (поймано 09.10.2026). `Qcm.Material` пользуется ЧАСТНЫМ API Qt
// Quick (`QuickPrivate`), а частный API между младшими выпусками Qt не
// совместим. Сам Qt этого не проверяет: плагин, собранный под 6.11,
// спокойно грузится в Qt 6.12 — и портит дерево узлов сцены. Вкладка обоев
// падала SIGSEGV в `QSGNode::removeChildNode` ← `updateDirtyNode` при
// открытии «Настроек» (там строятся поля ввода `Qcm.Material`), примерно
// в половине запусков. Плагин той же ревизии, пересобранный под 6.12, не
// падал ни разу. Поэтому сборка `Qcm.Material` под другую младшую версию
// Qt считается ОТСУТСТВУЮЩЕЙ: вкладка покажет объяснение вместо падения,
// а годная сборка из другого каталога останется в ходу.

/// Метка, за которой в заметке `.note.qt.metadata` плагина Qt 6 идут байты
/// «версия формата, старшая, младшая» версии Qt, под которую он собран
/// (макрос `QT_PLUGIN_METADATA_SECTION` / `qt_plugin_query_metadata_v2`).
pub const QT_PLUGIN_NOTE_MARK: &[u8] = b"qt-project!\0";

/// Версия Qt (старшая, младшая), под которую собран плагин, по байтам его
/// файла. `None` — метки нет (не плагин Qt 6) или файл обрезан.
pub fn plugin_qt_version(bytes: &[u8]) -> Option<(u32, u32)> {
    let at = bytes
        .windows(QT_PLUGIN_NOTE_MARK.len())
        .position(|w| w == QT_PLUGIN_NOTE_MARK)?;
    let rest = &bytes[at + QT_PLUGIN_NOTE_MARK.len()..];
    // [версия формата метаданных, QT_VERSION_MAJOR, QT_VERSION_MINOR, …]
    match rest {
        [_, major, minor, ..] if *major >= 5 => Some((*major as u32, *minor as u32)),
        _ => None,
    }
}

/// Годится ли сборка `Qcm.Material` в этом каталоге для запущенного Qt:
/// файл есть, и младшая версия Qt сборки совпадает с запущенной (когда
/// обе известны).
pub fn qcm_usable<FS: ModuleFs>(fs: &mut FS, dir: &str) -> bool {
    let path = plugin_path(dir, QCM_MATERIAL_REL);
    if !fs.is_file(&path) {
        return false;
    }
    qt_versions_compatible(fs.plugin_qt_version(&path), fs.running_qt_version())
}

/// Совпадают ли старшая и младшая версии Qt. Неизвестная с любой стороны —
/// «совпадают»: проверка не должна отключать вкладку там, где ничего не
/// знает.
pub fn qt_versions_compatible(built: Option<(u32, u32)>, running: Option<(u32, u32)>) -> bool {
    match (built, running) {
        (Some(b), Some(r)) => b == r,
        _ => true,
    }
}

/// Сборка `Qcm.Material`, найденная, но собранная под другую младшую
/// версию Qt: `(путь плагина, версия сборки, версия запущенного Qt)`.
/// Берётся первая по порядку предпочтения — её и назовёт заглушка.
pub fn incompatible_qcm<FS: ModuleFs>(
    fs: &mut FS,
    dirs: &[String],
) -> Option<(String, (u32, u32), (u32, u32))> {
    let running = fs.running_qt_version()?;
    for d in dirs {
        let path = plugin_path(d, QCM_MATERIAL_REL);
        if !fs.is_file(&path) {
            continue;
        }
        if let Some(built) = fs.plugin_qt_version(&path) {
            if built != running {
                return Some((path, built, running));
            }
        }
    }
    None
}

/// Каталоги модулей без негодных сборок: без тех, где лежит `Qcm.Material`
/// под другую версию Qt, и без тех, чей `waywallen.control` собран из
/// другого протокола (см. [`stale_control_dirs`]).
///
/// Такой каталог выкидывается ЦЕЛИКОМ, а не только его негодный модуль:
/// отдай его движку ради соседнего модуля — и `import` найдёт в нём
/// негодную сборку раньше годной.
pub fn usable_dirs<FS: ModuleFs>(fs: &mut FS, dirs: &[String]) -> Vec<String> {
    let by_qt = qt_usable_dirs(fs, dirs);
    let stale: Vec<String> = stale_control_among(fs, &by_qt)
        .into_iter()
        .map(|(dir, _)| dir)
        .collect();
    if stale.is_empty() {
        return by_qt;
    }
    let mut out: Vec<String> = by_qt.iter().filter(|d| !stale.contains(d)).cloned().collect();
    // Устаревший каталог мог нести ЕДИНСТВЕННЫЙ годный `Qcm.Material`
    // (оформление собрано дома, value-типы приехали пакетом). Тогда он не
    // выбрасывается, а уходит в конец списка: свежий `waywallen.control`
    // стоит раньше и заслоняет устаревший, а оформление остаётся в ходу.
    if !out.iter().any(|d| fs.is_file(&plugin_path(d, QCM_MATERIAL_REL))) {
        out.extend(
            stale
                .iter()
                .filter(|d| fs.is_file(&plugin_path(d, QCM_MATERIAL_REL)))
                .cloned(),
        );
    }
    out
}

/// Каталоги модулей без тех, где лежит `Qcm.Material` под другую версию Qt.
fn qt_usable_dirs<FS: ModuleFs>(fs: &mut FS, dirs: &[String]) -> Vec<String> {
    dirs.iter()
        .filter(|d| {
            let path = plugin_path(d, QCM_MATERIAL_REL);
            !fs.is_file(&path) || qcm_usable(fs, d)
        })
        .cloned()
        .collect()
}

// ============================================================================
// Протокол, из которого собран `waywallen.control`
// ============================================================================
//
// ⚠️ Зачем (поймано 10.10.2026). `waywallen.control` — value-типы и
// перечисления Qt-Protobuf, порождённые из `control.proto` и `filter.proto`.
// Вендорные страницы пишутся под протокол ТЕКУЩЕГО выпуска, а сборка модуля
// в домашней папке (`tools/build_qml_control.sh`) стоит в списке каталогов
// первой и сама не обновляется. После приёма нового протокола она
// заслоняла свежую пакетную: в ней не было перечисления `Flip`, и «Мониторы»
// сыпали `TypeError: Cannot read property 'FLIP_NONE' of undefined`.
//
// Признак свежести — отметка рядом с модулем: SHA-256 тех самых двух
// `.proto`, из которых модуль порождён. Её пишет скрипт сборки модуля (он же
// собирает модуль пакета), а Rexon сравнивает со своей — суммой тех же
// файлов, вшитых в ядро при сборке. Почему так:
//   * сумма не знает ни про одно имя из протокола — сработает на любом
//     следующем обновлении, а не только на `Flip`;
//   * проверка — чтение файла в 65 байт: ни разбора `.so`, ни запуска
//     процессов;
//   * тег выпуска (`UPSTREAM_TAG`) не годится: поверх вендорного протокола
//     лежат наши добавления, и протокол меняется при том же теге;
//   * время файла не годится: пакет и домашняя сборка делаются на разных
//     машинах, а `cp -a` и распаковка время переносят.
// Цена — правка одного комментария в `.proto` тоже объявляет прежние сборки
// устаревшими. Это безвредно: устаревшая сборка отбрасывается, только когда
// рядом есть свежая.

/// Отметка сборки `waywallen.control` внутри каталога модулей: одна строка,
/// SHA-256 (hex) склейки `control.proto` + `filter.proto`, из которых модуль
/// порождён. Пишет `tools/build_qml_control.sh`.
pub const CONTROL_STAMP_REL: &str = "waywallen/control/proto.sha256";

const CONTROL_PROTO: &[u8] =
    include_bytes!("../../../data/qt_wallpaper_engine/proto/control.proto");
const FILTER_PROTO: &[u8] =
    include_bytes!("../../../data/qt_wallpaper_engine/proto/filter.proto");

/// Отметка протокола, с которым собран этот Rexon, — то же, что
/// `cat control.proto filter.proto | sha256sum` в скрипте сборки модуля.
pub fn control_proto_stamp() -> &'static str {
    use sha2::{Digest, Sha256};
    static STAMP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    STAMP.get_or_init(|| {
        let mut hasher = Sha256::new();
        hasher.update(CONTROL_PROTO);
        hasher.update(FILTER_PROTO);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    })
}

/// Чем сборка `waywallen.control` в каталоге приходится текущему протоколу.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlBuild {
    /// Модуля в каталоге нет.
    Missing,
    /// Отметка совпала с протоколом этого Rexon.
    Fresh,
    /// Отметка есть, но от другого протокола.
    Stale,
    /// Отметки нет: сборка старше самой отметки (до 10.10.2026) либо
    /// сделана чем-то, кроме `tools/build_qml_control.sh`.
    Unmarked,
}

/// Состояние сборки `waywallen.control` в каталоге модулей.
pub fn control_build<FS: ModuleFs>(fs: &mut FS, dir: &str) -> ControlBuild {
    if !fs.is_file(&plugin_path(dir, WAYWALLEN_CONTROL_REL)) {
        return ControlBuild::Missing;
    }
    match fs.read_text(&plugin_path(dir, CONTROL_STAMP_REL)) {
        // Берётся первое слово: сырой вывод `sha256sum` («сумма  -») тоже
        // читается, регистр hex не важен.
        Some(text) => match text.split_whitespace().next() {
            Some(stamp) if stamp.eq_ignore_ascii_case(control_proto_stamp()) => {
                ControlBuild::Fresh
            }
            Some(_) => ControlBuild::Stale,
            None => ControlBuild::Unmarked,
        },
        None => ControlBuild::Unmarked,
    }
}

/// Каталоги, чей `waywallen.control` заслонял бы свежий, и причина
/// ([`ControlBuild::Stale`] либо [`ControlBuild::Unmarked`]) — для строки в
/// журнале. Считается над тем же списком, что видит движок: каталоги с
/// `Qcm.Material` под другую версию Qt уже отброшены и свежими не считаются.
///
/// ⭐ Устаревшей сборка считается, ТОЛЬКО когда в списке есть свежая. Нет
/// свежей — не отбрасывается ничего: старый модуль лучше, чем никакого
/// (большая часть страниц на нём работает). Отсюда же правило для каталога
/// без отметки: один он остаётся в ходу, рядом со свежим — уступает.
pub fn stale_control_dirs<FS: ModuleFs>(
    fs: &mut FS,
    dirs: &[String],
) -> Vec<(String, ControlBuild)> {
    let by_qt = qt_usable_dirs(fs, dirs);
    stale_control_among(fs, &by_qt)
}

fn stale_control_among<FS: ModuleFs>(
    fs: &mut FS,
    dirs: &[String],
) -> Vec<(String, ControlBuild)> {
    let states: Vec<(String, ControlBuild)> = dirs
        .iter()
        .map(|d| (d.clone(), control_build(fs, d)))
        .collect();
    if !states.iter().any(|(_, s)| *s == ControlBuild::Fresh) {
        return Vec::new();
    }
    states
        .into_iter()
        .filter(|(_, s)| matches!(s, ControlBuild::Stale | ControlBuild::Unmarked))
        .collect()
}

/// Каталог со свежим `waywallen.control`, первый по порядку предпочтения, —
/// его назовёт строка журнала рядом с отброшенным.
pub fn fresh_control_dir<FS: ModuleFs>(fs: &mut FS, dirs: &[String]) -> Option<String> {
    qt_usable_dirs(fs, dirs)
        .into_iter()
        .find(|d| control_build(fs, d) == ControlBuild::Fresh)
}

// ============================================================================
// QExtra: настоящий плагин автора или наша QML-подмена
// ============================================================================
//
// Вендорная разметка зовёт модуль `QExtra` (`QE.AnimatedImage`,
// `QE.ImageCache`). Его дают две сборки:
//   * настоящий плагин автора waywallen (C++, `tools/build_qml_qextra.sh`) —
//     лежит в каталоге модулей, `QExtra/libqextraplugin.so`;
//   * наша QML-подмена в каталоге вендорной разметки (`qml_ui/QExtra/`) —
//     запасная, работает всегда, но беднее (нет общего такта и потолка
//     памяти кадров).
// Каталог вендорной разметки стоит в путях импорта первым, поэтому сама по
// себе побеждает подмена. Настоящий плагин берётся, когда исполнитель
// ставит его каталог ВПЕРЕДИ вендорного, — и только если выполнено всё:
//   1. плагин есть в годном каталоге модулей;
//   2. рядом лежит отметка версии Qt, с которой он собран, и её старшая и
//      младшая совпадают с Qt процесса. Плагин зовёт частный API Qt
//      (`QRhi`), а тот между младшими выпусками не совместим — тот же
//      класс, что у `Qcm.Material` выше. В отличие от него, здесь неизвестная
//      версия — отказ: есть куда отступить;
//   3. граф сцены рисует через RHI. На программном графе (`offscreen`,
//      `QT_QUICK_BACKEND=software`) `QE.AnimatedImage` уходит в ошибку
//      «Image requires an RHI scene graph backend» и пишет её в журнал на
//      каждый кадр.

/// Настоящий плагин `QExtra` внутри каталога модулей; ставит
/// `tools/build_qml_qextra.sh`.
pub const QEXTRA_PLUGIN_REL: &str = "QExtra/libqextraplugin.so";

/// Отметка рядом с плагином `QExtra`: одна строка, версия Qt, с которой он
/// собран (`6.12.0`). Пишет `tools/build_qml_qextra.sh`.
pub const QEXTRA_QT_STAMP_REL: &str = "QExtra/QT_VERSION";

/// Платформы Qt, у которых графа сцены на RHI нет: Qt Quick на них сам
/// уходит на программную отрисовку.
pub const SOFTWARE_ONLY_PLATFORMS: [&str; 4] = ["offscreen", "minimal", "linuxfb", "vnc"];

/// Значения `QT_QUICK_BACKEND`, при которых граф сцены не на RHI.
pub const NON_RHI_QUICK_BACKENDS: [&str; 3] = ["software", "softwarecontext", "openvg"];

/// Рисует ли граф сцены Qt Quick через RHI — по тому, что известно до
/// создания первого окна: запрошенная платформа Qt
/// ([`requested_qpa_platform`]) и `QT_QUICK_BACKEND`.
///
/// `qpa_platform` — сырое значение, как его задают переменной или ключом:
/// список через `;` (Qt берёт первую платформу, которая поднялась), у каждой
/// после `:` свои параметры.
/// Судим по первой записи. Не задана — платформа сеанса (Wayland или X11),
/// у обеих RHI есть.
pub fn scene_graph_uses_rhi(qpa_platform: Option<&str>, quick_backend: Option<&str>) -> bool {
    let backend = quick_backend.unwrap_or("").trim().to_ascii_lowercase();
    if NON_RHI_QUICK_BACKENDS.contains(&backend.as_str()) {
        return false;
    }
    let platform = qpa_platform
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    !SOFTWARE_ONLY_PLATFORMS.contains(&platform.as_str())
}

/// Какую платформу Qt просили: значение для первого аргумента
/// [`scene_graph_uses_rhi`]. `args` — командная строка процесса вместе с
/// нулевым аргументом, `env_platform` — сырое `QT_QPA_PLATFORM`.
///
/// Qt читает переменную, а потом командную строку, и ключ `-platform ИМЯ`
/// (он же `--platform ИМЯ`) переменную перекрывает; из нескольких ключей
/// действует последний, ключ без значения не действует. Без этого запуск
/// `rexon-main-window -platform offscreen` считался бы запуском на платформе
/// сеанса и получал настоящий `QExtra` на программном графе.
pub fn requested_qpa_platform<S: AsRef<str>>(
    args: &[S],
    env_platform: Option<&str>,
) -> Option<String> {
    let mut platform = env_platform.map(str::to_owned);
    let mut rest = args.iter().skip(1).map(AsRef::as_ref);
    while let Some(arg) = rest.next() {
        if arg == "-platform" || arg == "--platform" {
            if let Some(value) = rest.next() {
                platform = Some(value.to_owned());
            }
        }
    }
    platform
}

/// Версия Qt (старшая, младшая) из отметки [`QEXTRA_QT_STAMP_REL`]: первое
/// слово вида `6.12` или `6.12.0`.
pub fn parse_qt_stamp(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.split_whitespace().next()?.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Почему вместо настоящего `QExtra` взята подмена.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QExtraFallback {
    /// Граф сцены не на RHI — настоящий там не рисует.
    NoRhi,
    /// Плагина нет ни в одном годном каталоге модулей.
    NoPlugin,
    /// Версия запущенного Qt неизвестна — сверять отметку не с чем.
    RunningQtUnknown { path: String },
    /// Плагин есть, отметки версии Qt рядом нет либо она не читается.
    NoStamp { path: String },
    /// Плагин собран под другую младшую версию Qt.
    WrongQt {
        path: String,
        built: (u32, u32),
        running: (u32, u32),
    },
}

/// Какой модуль `QExtra` получает движок QML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QExtraChoice {
    /// Настоящий плагин: `dir` — каталог модулей, который исполнитель ставит
    /// в пути импорта ВПЕРЕДИ каталога вендорной разметки.
    Real { dir: String, qt: (u32, u32) },
    /// QML-подмена из каталога вендорной разметки — путей менять не надо.
    Shim(QExtraFallback),
}

/// Выбрать модуль `QExtra`. `dirs` — полный список каталогов модулей
/// ([`module_dirs`]), `rhi` — ответ [`scene_graph_uses_rhi`].
///
/// Ищется по годным каталогам ([`usable_dirs`]) в их порядке; берётся первый
/// плагин, прошедший сверку. Негодный не закрывает годный в следующем
/// каталоге, а причина отказа называется по первому встреченному.
///
/// ⚠️ Каталог с устаревшим `waywallen.control` ([`stale_control_dirs`]) в
/// счёт не идёт, даже когда он оставлен в хвосте списка ради `Qcm.Material`:
/// выбранный каталог встаёт в пути импорта первым, и устаревший модуль в нём
/// снова заслонил бы свежий.
pub fn choose_qextra<FS: ModuleFs>(fs: &mut FS, dirs: &[String], rhi: bool) -> QExtraChoice {
    if !rhi {
        return QExtraChoice::Shim(QExtraFallback::NoRhi);
    }
    let stale: Vec<String> = stale_control_dirs(fs, dirs)
        .into_iter()
        .map(|(dir, _)| dir)
        .collect();
    let running = fs.running_qt_version();
    let mut first_reject: Option<QExtraFallback> = None;
    for dir in usable_dirs(fs, dirs) {
        if stale.contains(&dir) {
            continue;
        }
        let path = plugin_path(&dir, QEXTRA_PLUGIN_REL);
        if !fs.is_file(&path) {
            continue;
        }
        let Some(running) = running else {
            return QExtraChoice::Shim(QExtraFallback::RunningQtUnknown { path });
        };
        let stamp = fs
            .read_text(&plugin_path(&dir, QEXTRA_QT_STAMP_REL))
            .and_then(|text| parse_qt_stamp(&text));
        let reject = match stamp {
            None => QExtraFallback::NoStamp { path },
            Some(built) if built != running => QExtraFallback::WrongQt { path, built, running },
            // Отметка сошлась, но сам плагин называет другую версию Qt
            // (файл подменили, отметку оставили) — верим плагину.
            Some(_) => match fs.plugin_qt_version(&path) {
                Some(built) if built != running => {
                    QExtraFallback::WrongQt { path, built, running }
                }
                _ => return QExtraChoice::Real { dir, qt: running },
            },
        };
        first_reject.get_or_insert(reject);
    }
    QExtraChoice::Shim(first_reject.unwrap_or(QExtraFallback::NoPlugin))
}

/// Строка журнала о выборе модуля `QExtra`: что взято и почему.
pub fn qextra_choice_line(choice: &QExtraChoice) -> String {
    const SHIM: &str = "QExtra: взята QML-подмена";
    match choice {
        QExtraChoice::Real { dir, qt } => format!(
            "QExtra: взят настоящий плагин {} — собран под Qt {}.{}, как запущенный, граф сцены на RHI",
            plugin_path(dir, QEXTRA_PLUGIN_REL),
            qt.0,
            qt.1
        ),
        QExtraChoice::Shim(QExtraFallback::NoRhi) => {
            format!("{SHIM} — граф сцены не на RHI, настоящий плагин на нём не рисует")
        }
        QExtraChoice::Shim(QExtraFallback::NoPlugin) => {
            format!("{SHIM} — настоящего плагина нет в каталогах модулей")
        }
        QExtraChoice::Shim(QExtraFallback::RunningQtUnknown { path }) => {
            format!("{SHIM} — версия запущенного Qt неизвестна, плагин не сверить: {path}")
        }
        QExtraChoice::Shim(QExtraFallback::NoStamp { path }) => format!(
            "{SHIM} — рядом с плагином нет отметки версии Qt ({QEXTRA_QT_STAMP_REL}): {path}"
        ),
        QExtraChoice::Shim(QExtraFallback::WrongQt { path, built, running }) => format!(
            "{SHIM} — плагин собран под Qt {}.{}, а запущен Qt {}.{}: {path}",
            built.0, built.1, running.0, running.1
        ),
    }
}

// ============================================================================
// Каталоги, где лежат собранные QML-плагины
// ============================================================================

/// Шаблон мультиарх-каталога Debian и Ubuntu (`/usr/lib/x86_64-linux-gnu/…`).
///
/// Разворачивает его вызывающий (`glob.glob`) и отдаёт результат
/// [`module_dirs`] уже отсортированным: обход каталогов — side-effect.
pub const MULTIARCH_GLOB: &str = "/usr/lib/*/rexon/qml_modules";

/// Хвост пути каталога модулей внутри корня данных.
pub const MODULES_REL: &str = "rexon/qml_modules";

/// Системные каталоги модулей фиксированной раскладки: Arch кладёт плагины
/// в `/usr/lib`, Fedora и openSUSE — в `/usr/lib64`.
///
/// Третий — установка в систему (`/usr/local`): её раскладка та же, что у
/// пакета (`installer_exec::InstallLayout`). ⭐ Починка 15.09.2026: прежде
/// каталог ручной установки не искался вовсе, и Rust-окно после неё не
/// находило плагины вкладки обоев.
pub const SYSTEM_MODULE_DIRS: [&str; 3] = [
    "/usr/lib/rexon/qml_modules",
    "/usr/lib64/rexon/qml_modules",
    "/usr/local/lib/rexon/qml_modules",
];

/// Все каталоги, где могут лежать собранные QML-плагины, по убыванию
/// предпочтения и без повторов.
///
/// Порядок намеренный: пользовательская сборка (её делает разработчик и
/// именно её он ждёт увидеть) → каталоги данных из `$XDG_DATA_DIRS` →
/// системные фиксированные → мультиарх-каталоги. Это порядок РАЗРАБОТКИ:
/// у пакета и установки его переставляет [`order_module_dirs`].
///
/// `xdg_data_home` — `$XDG_DATA_HOME` (Починка 3), `xdg_data_dirs` — сырое
/// значение `$XDG_DATA_DIRS` через двоеточие, `multiarch` — уже развёрнутый
/// и отсортированный [`MULTIARCH_GLOB`].
pub fn module_dirs(
    xdg_data_home: Option<&str>,
    home: &str,
    xdg_data_dirs: &str,
    multiarch: &[String],
) -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();
    // Починка 3: `$XDG_DATA_HOME` уважается, но `~/.local/share` остаётся
    // кандидатом — `tools/build_qml_*.sh` пишут туда безусловно, и при
    // нестандартном XDG иначе потерялась бы уже собранная сборка.
    dirs.push(format!("{}/{MODULES_REL}", data_home(xdg_data_home, home)));
    dirs.push(format!("{}/{MODULES_REL}", data_home(None, home)));
    for base in xdg_data_dirs.split(':') {
        let base = base.trim();
        if !base.is_empty() {
            dirs.push(format!("{}/{MODULES_REL}", base.trim_end_matches('/')));
        }
    }
    dirs.extend(SYSTEM_MODULE_DIRS.iter().map(|d| d.to_string()));
    // ⭐ Починка 15.09.2026: установка в домашнюю папку — та же раскладка
    // (`installer_exec::InstallLayout`), и её каталог модулей не искался.
    // Место — сразу за установкой в систему, как у бинаря окна
    // (`installer::main_window_candidates`).
    if let Some(lib) = crate::installer::user_install_lib_dir(home) {
        dirs.push(format!("{lib}/{}", crate::installer_exec::QML_MODULES_DIR_NAME));
    }
    dirs.extend(multiarch.iter().cloned());

    let mut seen: Vec<String> = Vec::new();
    dirs.retain(|d| {
        if seen.iter().any(|s| s == d) {
            false
        } else {
            seen.push(d.clone());
            true
        }
    });
    dirs
}

// ============================================================================
// Пользовательская сборка против пакетной
// ============================================================================
//
// ⚠️ Зачем (поймано 02.11.2026). [`module_dirs`] ставит пользовательский
// каталог (`~/.local/share/rexon/qml_modules`, туда пишут
// `tools/build_qml_*.sh`) ПЕРВЫМ — это порядок разработчика. Но тот же
// порядок действовал и у пакета дистрибутива: на машине, где когда-то
// собирали модули руками, установленный Rexon брал `QExtra`, `Qcm.Material` и
// `waywallen.control` из старой ручной сборки, а не из своего пакета. Пока
// ревизии совпадают, это незаметно; после обновления пакета в ход пошла бы
// устаревшая копия, которую пакетный менеджер не обновляет никогда.
//
// Правило: пользовательский каталог главнее только при запуске из дерева
// исходников (разработка). У пакета и у установки главнее своя копия, а
// пользовательская берётся, лишь когда пакетной копии модуля нет. Файлы
// пользователя не удаляются — это его ручная сборка; журнал говорит, что
// она пропущена и чем отличается.

/// Отметка ревизии `Qcm.Material` рядом с модулем; пишет
/// `tools/build_qml_material.sh` (и сборка пакета им же).
pub const QCM_MATERIAL_PIN_REL: &str = "Qcm/QML_MATERIAL_PIN";

/// Отметка ревизии `QExtra` рядом с модулем; пишет `tools/build_qml_qextra.sh`.
pub const QEXTRA_PIN_REL: &str = "QExtra/QEXTRA_PIN";

/// Что сверяется у одного модуля: имя для журнала, файл плагина и отметки
/// рядом с ним (подпись, путь в каталоге модулей).
struct ComparedModule {
    name: &'static str,
    plugin_rel: &'static str,
    stamps: &'static [(&'static str, &'static str)],
}

/// Все три модуля вкладки обоев, которые кладут и `tools/build_qml_*.sh`, и
/// пакет. Новый модуль добавляется строкой сюда (правило #12).
const COMPARED_MODULES: [ComparedModule; 3] = [
    ComparedModule {
        name: "Qcm.Material",
        plugin_rel: QCM_MATERIAL_REL,
        stamps: &[("ревизия", QCM_MATERIAL_PIN_REL)],
    },
    ComparedModule {
        name: "waywallen.control",
        plugin_rel: WAYWALLEN_CONTROL_REL,
        stamps: &[("протокол", CONTROL_STAMP_REL)],
    },
    ComparedModule {
        name: "QExtra",
        plugin_rel: QEXTRA_PLUGIN_REL,
        stamps: &[("ревизия", QEXTRA_PIN_REL), ("отметка Qt", QEXTRA_QT_STAMP_REL)],
    },
];

/// Каталоги пользовательской сборки — те же первые кандидаты, что у
/// [`module_dirs`]: `$XDG_DATA_HOME` и `~/.local/share`, без повторов.
pub fn user_module_dirs(xdg_data_home: Option<&str>, home: &str) -> Vec<String> {
    let mut out = vec![format!("{}/{MODULES_REL}", data_home(xdg_data_home, home))];
    let plain = format!("{}/{MODULES_REL}", data_home(None, home));
    if !out.contains(&plain) {
        out.push(plain);
    }
    out
}

/// Главнее ли пользовательская сборка модулей: только у запуска из дерева
/// исходников, и только если лаунчер пакета не сказал «пакет»
/// (`installer::PACKAGED_ENV`).
pub fn user_modules_first(packaged: bool, dev_tree: bool) -> bool {
    dev_tree && !packaged
}

/// Чем модуль пользовательской сборки отличается от пакетного.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDiff {
    pub module: &'static str,
    /// Что отличается: «ревизия», «протокол», «отметка Qt», «Qt сборки».
    pub what: &'static str,
    /// Значение у пользователя; «нет», когда отметки нет.
    pub user: String,
    pub package: String,
}

/// Что сделано с пользовательским каталогом модулей.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserCopyUse {
    /// Запуск из дерева исходников — каталог остаётся первым.
    DevTree,
    /// Пакетной копии нет ни у одного его модуля — берётся он.
    NoPackageCopy,
    /// Часть модулей есть только у него — оставлен ПОЗАДИ пакетных каталогов:
    /// общие модули идут из пакета.
    OnlyHere(Vec<&'static str>),
    /// Всё, что в нём есть, даёт пакет — каталог не используется.
    Skipped,
}

/// Решение по одному пользовательскому каталогу — для строки журнала.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserCopyNote {
    pub dir: String,
    /// Первый не пользовательский каталог, где нашлась копия его модулей.
    pub package_dir: Option<String>,
    pub diffs: Vec<ModuleDiff>,
    pub used: UserCopyUse,
}

/// Порядок каталогов модулей и решения по пользовательским каталогам.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleOrder {
    pub dirs: Vec<String>,
    pub notes: Vec<UserCopyNote>,
}

/// Значение отметки: первое слово файла.
fn stamp_value<FS: ModuleFs>(fs: &mut FS, dir: &str, rel: &str) -> Option<String> {
    fs.read_text(&plugin_path(dir, rel))
        .and_then(|t| t.split_whitespace().next().map(str::to_ascii_lowercase))
}

/// Чем модуль `m` в каталоге `user` отличается от него же в `package`.
/// Отметки и версия Qt плагина сверяются, только когда у пакета они есть:
/// сравнивать не с чем — не отличие.
fn module_diffs<FS: ModuleFs>(
    fs: &mut FS,
    m: &ComparedModule,
    user: &str,
    package: &str,
) -> Vec<ModuleDiff> {
    let mut out = Vec::new();
    for (what, rel) in m.stamps {
        let Some(pkg) = stamp_value(fs, package, rel) else { continue };
        let usr = stamp_value(fs, user, rel);
        if usr.as_deref() != Some(pkg.as_str()) {
            out.push(ModuleDiff {
                module: m.name,
                what,
                user: usr.unwrap_or_else(|| "нет".to_string()),
                package: pkg,
            });
        }
    }
    let fmt = |v: (u32, u32)| format!("{}.{}", v.0, v.1);
    if let Some(pkg) = fs.plugin_qt_version(&plugin_path(package, m.plugin_rel)) {
        let usr = fs.plugin_qt_version(&plugin_path(user, m.plugin_rel));
        if usr != Some(pkg) {
            out.push(ModuleDiff {
                module: m.name,
                what: "Qt сборки",
                user: usr.map(fmt).unwrap_or_else(|| "нет".to_string()),
                package: fmt(pkg),
            });
        }
    }
    out
}

/// Расставить каталоги модулей ([`module_dirs`]) по тому, откуда запущен
/// Rexon. `user_dirs` — [`user_module_dirs`], `user_first` —
/// [`user_modules_first`].
///
/// * `user_first` — порядок не меняется (разработка: своя сборка главнее);
///   отличия от пакетной копии всё равно попадают в заметки.
/// * иначе пакетные каталоги идут первыми. Пользовательский каталог
///   выбрасывается, если всё, что в нём лежит, есть в пакете (совпадает или
///   устарело — неважно, главнее пакет); оставляется В КОНЦЕ списка, если
///   хотя бы одного его модуля в пакете нет.
pub fn order_module_dirs<FS: ModuleFs>(
    fs: &mut FS,
    dirs: &[String],
    user_dirs: &[String],
    user_first: bool,
) -> ModuleOrder {
    let package: Vec<String> = dirs.iter().filter(|d| !user_dirs.contains(d)).cloned().collect();
    let mut kept_users: Vec<String> = Vec::new();
    let mut notes = Vec::new();
    for user in dirs.iter().filter(|d| user_dirs.contains(d)) {
        let mut present = 0usize;
        let mut only_here = Vec::new();
        let mut diffs = Vec::new();
        let mut package_dir: Option<String> = None;
        for m in &COMPARED_MODULES {
            if !fs.is_file(&plugin_path(user, m.plugin_rel)) {
                continue;
            }
            present += 1;
            let pkg = package
                .iter()
                .find(|p| fs.is_file(&plugin_path(p, m.plugin_rel)))
                .cloned();
            match pkg {
                Some(pkg) => {
                    diffs.extend(module_diffs(fs, m, user, &pkg));
                    package_dir.get_or_insert(pkg);
                }
                None => only_here.push(m.name),
            }
        }
        if present == 0 {
            // Пустой каталог ничему не мешает и ничего не даёт.
            if user_first {
                kept_users.push(user.clone());
            }
            continue;
        }
        let used = if user_first {
            UserCopyUse::DevTree
        } else if only_here.len() == present {
            UserCopyUse::NoPackageCopy
        } else if !only_here.is_empty() {
            UserCopyUse::OnlyHere(only_here)
        } else {
            UserCopyUse::Skipped
        };
        if used != UserCopyUse::Skipped {
            kept_users.push(user.clone());
        }
        notes.push(UserCopyNote { dir: user.clone(), package_dir, diffs, used });
    }
    let dirs = if user_first {
        dirs.iter()
            .filter(|d| !user_dirs.contains(d) || kept_users.contains(d))
            .cloned()
            .collect()
    } else {
        package.into_iter().chain(kept_users).collect()
    };
    ModuleOrder { dirs, notes }
}

/// Строка журнала о пользовательском каталоге модулей и уровень: `true` —
/// предупреждение (копия отличается от пакетной), `false` — сведение.
/// `None` — сказать нечего (разработка без отличий).
pub fn user_copy_note_line(note: &UserCopyNote) -> Option<(bool, String)> {
    let diffs = note
        .diffs
        .iter()
        .map(|d| format!("{} — {}: у пользователя {}, в пакете {}", d.module, d.what, d.user, d.package))
        .collect::<Vec<_>>()
        .join("; ");
    let pkg = note.package_dir.as_deref().unwrap_or("?");
    let warn = !note.diffs.is_empty();
    let line = match &note.used {
        UserCopyUse::DevTree if warn => format!(
            "пользовательская сборка модулей {} отличается от пакетной ({pkg}): {diffs}; \
             запуск из дерева исходников — берётся пользовательская",
            note.dir
        ),
        UserCopyUse::DevTree => return None,
        UserCopyUse::NoPackageCopy => format!(
            "пакетной копии QML-модулей нет — берётся пользовательская сборка {}",
            note.dir
        ),
        UserCopyUse::OnlyHere(names) => format!(
            "пользовательская сборка модулей {} оставлена позади пакетной ({pkg}): только в ней есть {}{}",
            note.dir,
            names.join(", "),
            if warn { format!("; общие модули отличаются и берутся из пакета: {diffs}") } else { String::new() }
        ),
        UserCopyUse::Skipped if warn => format!(
            "пользовательская сборка модулей {} устарела относительно пакетной ({pkg}) и не используется: \
             {diffs}. Файлы не тронуты; если сборка не нужна, каталог можно удалить вручную",
            note.dir
        ),
        UserCopyUse::Skipped => format!(
            "пользовательская сборка модулей {} не используется: Rexon поставлен пакетом, \
             берётся пакетная копия {pkg}",
            note.dir
        ),
    };
    Some((warn, line))
}

// ============================================================================
// Что именно ищем в этих каталогах
// ============================================================================

/// `Qcm.Material` — скомпилированный QML-плагин (MIT, QmlMaterial hypengw),
/// ставит `tools/build_qml_material.sh`.
pub const QCM_MATERIAL_REL: &str = "Qcm/Material/libqml_materialplugin.so";

/// `waywallen.control` — Qt-Protobuf value-типы, которые PySide6
/// предоставить не может; ставит `tools/build_qml_control.sh`.
pub const WAYWALLEN_CONTROL_REL: &str =
    "waywallen/control/libwaywallen-controlplugin.so";

/// Библиотека самих value-типов `waywallen.control` (плагин выше — её
/// тонкая QML-обёртка). Типы регистрируются при загрузке библиотеки, так
/// что стенду без движка QML её достаточно подгрузить.
pub const WAYWALLEN_CONTROL_TYPES_REL: &str = "waywallen/control/libwaywallen-control.so";

/// Куда `tools/build_qml_control.sh` кладёт наши Rust-сборки рядом с
/// C++-плагином: у ПАКЕТА дерева `rust/target` нет вовсе.
pub const RUST_LIB_SUBDIR: &str = "waywallen/control";

/// Крейт `rexon-qt-filter` (cxx-qt): модель редактора фильтров обоев.
pub const FILTER_LIB_NAME: &str = "librexon_qt_filter.so";

/// Профили сборки в дереве разработчика, по убыванию свежести.
pub const TARGET_PROFILES: [&str; 2] = ["release", "debug"];

/// Полный путь плагина в каталоге модулей.
pub fn plugin_path(modules_dir: &str, rel: &str) -> String {
    format!("{}/{rel}", modules_dir.trim_end_matches('/'))
}

/// Первый каталог из списка, где лежит нужный плагин.
pub fn find_plugin<FS: ModuleFs>(
    fs: &mut FS,
    dirs: &[String],
    rel: &str,
) -> Option<String> {
    dirs.iter()
        .map(|d| plugin_path(d, rel))
        .find(|p| fs.is_file(p))
}

/// Каталог, который отдаётся движку QML как `addImportPath` первым — тот,
/// где нашёлся `Qcm.Material`; ни одного нет — первый кандидат (чтобы у
/// сообщения об ошибке был осмысленный путь).
///
/// Порт `_resolve_qcm_dir` вместе с его запасной веткой.
pub fn resolve_import_dir<FS: ModuleFs>(fs: &mut FS, dirs: &[String]) -> String {
    for d in dirs {
        if qcm_usable(fs, d) {
            return d.clone();
        }
    }
    dirs.first().cloned().unwrap_or_default()
}

/// Все каталоги модулей, в которых что-то из наших плагинов реально лежит —
/// их движку QML стоит отдать в `addImportPath`.
///
/// Починка 2 с другой стороны: пока путь импорта был ОДИН, раздельная
/// установка (`Qcm.Material` собран пользователем, `waywallen.control`
/// приехал пакетом) не работала — половина типов не импортировалась. Список
/// сохраняет прежний порядок предпочтения, поэтому для обычной установки в
/// один каталог ответ тот же, что был.
pub fn import_dirs<FS: ModuleFs>(fs: &mut FS, dirs: &[String]) -> Vec<String> {
    usable_dirs(fs, dirs)
        .iter()
        .filter(|d| {
            fs.is_file(&plugin_path(d, QCM_MATERIAL_REL))
                || fs.is_file(&plugin_path(d, WAYWALLEN_CONTROL_REL))
        })
        .cloned()
        .collect()
}

/// Есть ли ОБА обязательных QML-плагина (иначе QML-интерфейс вкладки
/// недоступен) — порт `qcm_material_available`.
///
/// Нужны именно два: `Qcm.Material` даёт оформление, `waywallen.control` —
/// value-типы Qt-Protobuf, без которых не строится страница «Обои».
///
/// Каталоги с `Qcm.Material` под другую версию Qt и с `waywallen.control`
/// из другого протокола (при живом свежем) в счёт не идут — см.
/// [`usable_dirs`].
pub fn qml_plugins_ready<FS: ModuleFs>(fs: &mut FS, dirs: &[String]) -> bool {
    let dirs = usable_dirs(fs, dirs);
    find_plugin(fs, &dirs, QCM_MATERIAL_REL).is_some()
        && find_plugin(fs, &dirs, WAYWALLEN_CONTROL_REL).is_some()
}

/// Кандидаты на Rust-сборку по убыванию свежести: `release` → `debug` в
/// дереве разработчика → каталоги QML-модулей.
///
/// Починка 1 и 2: каталоги модулей идут ВСЕ и в том же порядке, что у
/// [`module_dirs`], а не один выбранный по `Qcm.Material`.
pub fn rust_lib_candidates(
    repo_root: &str,
    dirs: &[String],
    lib_name: &str,
) -> Vec<String> {
    let target = format!("{}/rust/target", repo_root.trim_end_matches('/'));
    let mut out: Vec<String> = TARGET_PROFILES
        .iter()
        .map(|p| format!("{target}/{p}/{lib_name}"))
        .collect();
    out.extend(
        dirs.iter()
            .map(|d| plugin_path(d, &format!("{RUST_LIB_SUBDIR}/{lib_name}"))),
    );
    out
}

/// Кандидаты на сборку крейта `rexon-qt-filter` — порт
/// `bridge._rust_filter_model_candidates`.
pub fn filter_lib_candidates(repo_root: &str, dirs: &[String]) -> Vec<String> {
    rust_lib_candidates(repo_root, dirs, FILTER_LIB_NAME)
}

/// Первый существующий кандидат либо `None`.
pub fn pick_existing<FS: ModuleFs>(fs: &mut FS, candidates: &[String]) -> Option<String> {
    candidates.iter().find(|p| fs.is_file(p)).cloned()
}

// ============================================================================
// Пути вендоренного QML и дискового кэша миниатюр
// ============================================================================

/// Корневой QML-хост вкладки: `Loader`, который держит показываемую
/// страницу, чтобы движок QML на все страницы оставался ОДИН.
pub const PAGE_HOST_QML: &str = "PageHost.qml";

/// Прогревочная сцена: регистрация Qt-Protobuf модуля `waywallen.control`
/// обязана пройти на ГЛАВНОМ потоке до первого `QQuickWidget`.
pub const PREWARM_QML: &str = "_prewarm_control.qml";

/// Файл с тегом вендоренного апстрима внутри каталога QML вкладки.
pub const UPSTREAM_TAG_REL: &str = "waywallen/ui/UPSTREAM_TAG";

/// Каталог страниц вендоренного QML внутри каталога вкладки.
pub const VENDOR_PAGE_DIR: &str = "waywallen/ui/qml/page";

/// Ответ [`upstream_tag`], когда файла нет либо он не прочитался.
pub const UPSTREAM_TAG_UNKNOWN: &str = "?";

/// Путь файла внутри каталога QML вкладки (`qml_ui/`).
pub fn qml_path(qml_dir: &str, rel: &str) -> String {
    format!("{}/{rel}", qml_dir.trim_end_matches('/'))
}

/// Путь вендоренной страницы по её имени (`"DiscoverPage.qml"`);
/// пустое имя — пустой ответ, хост показывает пустой `Loader`.
pub fn vendor_page_path(qml_dir: &str, page_qml: &str) -> String {
    if page_qml.is_empty() {
        return String::new();
    }
    qml_path(qml_dir, &format!("{VENDOR_PAGE_DIR}/{page_qml}"))
}

/// Тег вендоренного апстрима из прочитанного содержимого файла.
pub fn upstream_tag(raw: Option<&str>) -> String {
    match raw {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        // Пустой файл раньше отдавался как пустая строка и попадал прямо в
        // заголовок окна («waywallen »). Тот же ответ, что и у отсутствующего
        // файла, честнее: тега мы не знаем (правило #13).
        _ => UPSTREAM_TAG_UNKNOWN.to_string(),
    }
}

/// Версия вендорного интерфейса по содержимому `UPSTREAM_TAG`:
/// `v0.5.0 (564eb02)` → `0.5.0`.
///
/// Это то же число, что у апстрима `Qt.application.version` (версия пакета
/// `waywallen-ui`), и того же вида, что версия демона по D-Bus, — страница
/// «О программе» сравнивает их и показывает вторую строку «Daemon version»
/// только при расхождении (наш патч 0002). Тега нет — тот же ответ, что у
/// [`upstream_tag`]: честное «?», а не пустая строка.
pub fn ui_version(raw: Option<&str>) -> String {
    let tag = upstream_tag(raw);
    let first = tag.split_whitespace().next().unwrap_or(UPSTREAM_TAG_UNKNOWN);
    let version = first.strip_prefix('v').unwrap_or(first);
    if version.is_empty() {
        UPSTREAM_TAG_UNKNOWN.to_string()
    } else {
        version.to_string()
    }
}

/// Хвост каталога дискового кэша сетевых картинок QML (миниатюры Discover).
pub const NET_CACHE_REL: &str = "rexon/qml_net";

/// Предел дискового кэша миниатюр, байт.
pub const NET_CACHE_MAX_BYTES: i64 = 200 * 1024 * 1024;

/// Каталог дискового кэша миниатюр. `$XDG_CACHE_HOME` уважается — это тот
/// же класс, что Починка 3 (в оригинале стоял литерал `~/.cache`).
pub fn net_cache_dir(xdg_cache_home: Option<&str>, home: &str) -> String {
    let base = match xdg_cache_home {
        Some(x) if !x.is_empty() => x.trim_end_matches('/').to_string(),
        _ => format!("{}/.cache", home.trim_end_matches('/')),
    };
    format!("{base}/{NET_CACHE_REL}")
}
