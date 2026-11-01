//! Порт `rexon/qt_wallpaper_engine/client.py` — клиент демона waywallen для
//! вкладки «Wallpaper Engine».
//!
//! Оригинал — два слоя поверх Qt: `DaemonControl` (discovery/автозапуск демона
//! по D-Bus) и `WaywallenClient` (control plane по WebSocket+protobuf). Здесь
//! живёт ВСЯ логика, свободная от Qt:
//!
//! 1. **Поиск бинаря демона** — [`daemon_binary_candidates`] /
//!    [`find_daemon_binary`]. Порядок проверок значим (фикс бага 2026-06-24) и
//!    закреплён отдельным тестом.
//! 2. **Автозапуск при входе** — содержимое systemd-юнита и `.desktop`, пути,
//!    argv `systemctl --user`, планы включения/выключения ([`enable_plan`],
//!    [`disable_plan`]). ⛔ Ядро НИЧЕГО не запускает и НИЧЕГО не пишет: план
//!    исполняет `rexon-gui::autostart_queries` (Python-исполнителя не
//!    осталось с 07.09.2026), там же он подключён к переключателю
//!    «Start at login» страницы настроек вкладки.
//! 3. **Решения `DaemonControl`** — [`probe`] поверх уже существующего трейта
//!    [`WaywallenIo`] (свойства `WsPort`/`Version`/`CurrentWallpaperId` НЕ
//!    дублируются — единый механизм, правило #12), argv detached-запуска,
//!    [`ensure_running_decision`], [`poll_decision`], разбор `ListTasks`
//!    (QtDBus-строки и фолбэк `busctl --json`).
//! 4. **Кодек ЗАПРОСОВ** — выдача `request_id` ([`RequestIds`]) и сборка байтов
//!    `Request` поверх prost-структур [`crate::proto`]: типизированные обёртки
//!    (`health`, `wallpaper_list`, …) и универсальный [`request_bytes`] для
//!    пути `call(field, msg)` из `qml_ui/bridge.py`.
//!
//! ⛔ Что СОЗНАТЕЛЬНО остаётся на Python (граница этапа 2):
//! * `QWebSocket` целиком (открытие/закрытие/состояние/`sendBinaryMessage`) —
//!   второй event loop (tokio/tungstenite) в процесс не тащим;
//! * `QtDBus`/`QProcess`/`QTimer` — исполнители решений ядра;
//! * карта `_pending` с владельцами-QObject и `shiboken6.isValid` при доставке
//!   (фикс краша `39617f2` по умершим QML-объектам) — владельцы это живые
//!   Qt-объекты, в Rust им делать нечего;
//! * **РАЗБОР ОТВЕТОВ** демона в QML-страницах: `bridge.py` читает поля
//!   `pb.Response` напрямую (разбор портированных запросов — в
//!   [`crate::we_query`]). Это этап 3. Исключение — ответы, нужные САМОМУ
//!   `client.py` в его синхронном пути: [`renderer_ids_from_frame`].
//!
//! ## Починенные при порте баги оригинала (правило авто-починки)
//!
//! 1. **`int(ws_port)` в `probe`** падал необработанным `ValueError`, если
//!    демон отдавал нечисловое `WsPort` (старая/битая версия интерфейса) —
//!    вкладка WE не открывалась вообще. Здесь неконвертируемое значение даёт
//!    `ws_port = 0`, что уводит `ensure_running` в ветку автозапуска. Тот же
//!    класс, что уже починен в [`crate::waywallen_client`].
//! 2. **Путь systemd-юнита игнорировал `XDG_CONFIG_HOME`.**
//!    `_autostart_unit_path` жёстко брал `~/.config/systemd/user`, а
//!    `_autostart_desktop_path` рядом — уважал `XDG_CONFIG_HOME`. При
//!    нестандартном `XDG_CONFIG_HOME` юнит писался туда, где systemd его НЕ
//!    ищет, и следом `systemctl --user enable` падал «Unit file does not
//!    exist» — автозапуск молча не включался. Здесь оба пути считаются от
//!    одного корня ([`config_home`]).
//! 3. **Юнит убивал демона сигналом BUS на КАЖДОЙ остановке.** В юните не был
//!    задан `KillMode`, а значение по умолчанию — `control-group`: systemd шлёт
//!    SIGTERM ВСЕМ процессам cgroup разом. Демон запускается из AppImage, и
//!    рядом с ним в той же cgroup живёт процесс AppImage-обёртки, держащий
//!    FUSE-монтирование squashfs (`/tmp/.mount_*`), откуда исполняется сам
//!    бинарь. Обёртка на SIGTERM размонтирует образ немедленно, а демон в это
//!    время ещё доигрывает штатное завершение — первая же страница кода,
//!    которой не оказалось в памяти, даёт SIGBUS/`BUS_ADRERR`. Итог: каждый
//!    `stop`/`restart`/выход из сессии (`PartOf=graphical-session.target`)
//!    оканчивался core dump, а `Restart=on-failure` считал это сбоем и поднимал
//!    демона обратно. Здесь юнит несёт [`KillMode=mixed`](AUTOSTART_UNIT_TMPL):
//!    SIGTERM получает ТОЛЬКО главный процесс, монтирование живёт до его
//!    выхода. Замер: SIGTERM одному главному PID — чистый выход, SIGTERM всей
//!    cgroup — `status=7/BUS` (воспроизведено 2026-08-22).

use serde::Serialize;
use prost::Message;

use crate::proto::control as pb;
use crate::waywallen_client::{DbusValue, WaywallenIo};

// ============================================================================
// Константы D-Bus и опроса
// ============================================================================

/// Имя сервиса демона (то же, что в [`crate::waywallen_client::BUS_NAME`] —
/// реэкспорт, чтобы вкладка WE не заводила вторую копию адреса).
pub use crate::waywallen_client::{
    BUS_NAME as DBUS_SERVICE, IFACE as DBUS_IFACE, OBJECT_PATH as DBUS_PATH,
};

/// Интервал поллинга D-Bus после автозапуска демона, мс.
pub const POLL_MS: u64 = 500;

/// Сколько раз опрашивать (≈12 секунд на холодный старт демона).
pub const POLL_LIMIT: i64 = 24;

/// Таймаут `Properties.Get`, мс.
pub const PROPERTY_TIMEOUT_MS: u64 = 1000;

/// Таймаут `ListTasks` по D-Bus, мс.
pub const LIST_TASKS_TIMEOUT_MS: u64 = 2000;

/// Таймаут фолбэка `busctl`, секунды.
pub const BUSCTL_TIMEOUT_SECS: u64 = 3;

/// Аргументы, с которыми демон стартует из Rexon: свой UI и трей не открывать,
/// обои демон восстанавливает сам по своему конфигу.
pub const DAEMON_ARGS: [&str; 2] = ["--no-ui", "--no-tray"];

// ============================================================================
// 1. Поиск бинаря демона
// ============================================================================

/// Запасные кандидаты (по приоритету), порт `_DAEMON_FALLBACKS`.
///
/// ⚠️ Заведомо устаревший извлечённый бинарь `owe-ref/install/bin/waywallen`
/// НЕ включён сознательно: он не знает миграции БД, переписанной новым
/// AppImage, → демон падает на старте (баг 2026-06-24).
///
/// ⚠️ Бывший второй кандидат `~/Projects/waywallen/waywallen.AppImage`
/// (dev-сборка) убран: файла больше нет, симлинк `~/.local/bin/waywallen`
/// ведёт на управляемый AppImage. Мёртвый путь только удлинял поиск.
pub const DAEMON_FALLBACKS: [&str; 1] = [
    // симлинк на управляемый AppImage
    "~/.local/bin/waywallen",
];

/// Имена вшитого AppImage в `rexon/vendor/` (в порядке проверки).
pub const BUNDLED_NAMES: [&str; 2] = ["waywallen.AppImage", "waywallen"];

/// Каталог движка, который ставит пакет Rexon (`packaging/aur/PKGBUILD`):
/// AppImage waywallen и рядом `plugins/<id>/` — плагин Open Wallpaper Engine.
///
/// Почему `/usr/lib`, а не `/usr/share`: там лежат исполняемые файлы под
/// архитектуру (AppImage, рендереры плагина, CEF). Почему у каталога ИМЕННО
/// такая раскладка: демон waywallen принимает флаг `--plugin <каталог>` и
/// ищет плагины в `<каталог>/plugins/` (waywallen v0.4.2,
/// `daemon/bootstrap.rs`), поэтому каталог движка передаётся демону как есть
/// — вторая раскладка не нужна.
pub const ENGINE_DIR: &str = "/usr/lib/rexon/engine";

/// Имя AppImage движка в [`ENGINE_DIR`].
pub const ENGINE_APPIMAGE_NAME: &str = "waywallen.AppImage";

/// Подкаталог плагинов внутри каталога, переданного демону флагом
/// [`PLUGIN_FLAG`] (договор демона, см. [`ENGINE_DIR`]).
pub const ENGINE_PLUGINS_SUBDIR: &str = "plugins";

/// Флаг демона «ещё один корень плагинов». Есть у waywallen с 0.3.x; у
/// 0.2.x его нет вовсе (незнакомый флаг — отказ старта), поэтому флаг
/// передаётся ТОЛЬКО когда каталог плагинов комплекта реально лежит на
/// диске, то есть рядом стоит движок того же комплекта или новее.
pub const PLUGIN_FLAG: &str = "--plugin";

/// AppImage движка из комплекта пакета.
pub fn engine_appimage_path(engine_dir: &str) -> String {
    format!("{}/{ENGINE_APPIMAGE_NAME}", engine_dir.trim_end_matches('/'))
}

/// Каталог плагинов комплекта (`<каталог движка>/plugins`).
pub fn engine_plugins_path(engine_dir: &str) -> String {
    format!("{}/{ENGINE_PLUGINS_SUBDIR}", engine_dir.trim_end_matches('/'))
}

// ── Движок из пакетов дистрибутива (с 30.10.2026) ───────────────────────────
//
// Пакет Rexon для дистрибутивов (`packaging/aur/PKGBUILD`) больше НЕ вшивает
// движок: он зависит от отдельных пакетов `waywallen` и
// `open-wallpaper-engine`. Чужой AppImage со статически вшитыми библиотеками
// и свой архив GPL-исходников внутри pacman-пакета — то, что сопровождающие
// дистрибутива на ревью не пропустят; обновлять движок — дело пакетного
// менеджера, а не Rexon. Вшитая раскладка ([`ENGINE_DIR`]) в ядре остаётся
// для поставок, где системного пакета движка нет; стоят оба — первым идёт
// системный ([`engine_layouts`]).
//
// Куда ставят файлы пакеты Arch/AUR (сверено по их PKGBUILD 30.10.2026):
//   * `waywallen` — `lito install --prefix /usr`: демон `/usr/bin/waywallen`,
//     встроенные плагины в `/usr/share/waywallen/plugins/`,
//     `libwaywallen-bridge.so` в `/usr/lib`; юнита systemd апстрим не ставит
//     (его пишет Rexon, [`AUTOSTART_UNIT`]);
//   * `waywallen-bin` — распакованный AppImage в `/opt/waywallen-bin`,
//     `/usr/bin/waywallen` — символьная ссылка на его `AppRun`;
//   * `open-wallpaper-engine` — плагин в
//     `/usr/share/waywallen/plugins/org.waywallen.open-wallpaper-engine/`
//     (`install.lua` тега v0.3.2), рисовальщики `waywallen-wescene-renderer` и
//     `waywallen-weweb-renderer` в `/usr/bin` (CEF — системный пакет `cef`).
//
// Демон, лежащий в `<префикс>/bin`, САМ ищет плагины в
// `<префикс>/share/waywallen/plugins` (`standard_plugin_roots`, waywallen
// v0.5.0) — по настоящему пути своего файла, после символьных ссылок. Значит,
// демону `waywallen` из пакета флаг `--plugin` не нужен, а `waywallen-bin` и
// управляемому AppImage человека — нужен: их файл живёт не под `/usr`.
// Передавать флаг «на всякий случай» нельзя: тот же каталог дважды демон
// сканирует дважды и держит две одинаковые записи каждого плагина.

/// Демон движка из пакета дистрибутива.
pub const SYSTEM_DAEMON: &str = "/usr/bin/waywallen";

/// Каталог данных движка из пакетов дистрибутива: в нём `plugins/`.
pub const SYSTEM_ENGINE_DIR: &str = "/usr/share/waywallen";

/// Чей движок: вшитый в пакет Rexon или из пакетов дистрибутива.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    /// Вшит в поставку Rexon ([`ENGINE_DIR`]): AppImage и плагины рядом.
    Bundled,
    /// Отдельные пакеты дистрибутива ([`SYSTEM_DAEMON`],
    /// [`SYSTEM_ENGINE_DIR`]): обновляет их пакетный менеджер.
    System,
}

/// Раскладка движка, который поставил не человек, а пакет: где демон и
/// где каталог с `plugins/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineLayout {
    pub kind: EngineKind,
    /// Бинарь демона.
    pub daemon: String,
    /// Каталог, в котором лежит `plugins/` (его же получает `--plugin`).
    pub dir: String,
}

impl EngineLayout {
    /// Вшитый комплект в каталоге `engine_dir`.
    pub fn bundled_in(engine_dir: &str) -> EngineLayout {
        EngineLayout {
            kind: EngineKind::Bundled,
            daemon: engine_appimage_path(engine_dir),
            dir: engine_dir.trim_end_matches('/').to_string(),
        }
    }

    /// Движок пакетов дистрибутива: демон `daemon`, данные в `dir`.
    pub fn system_in(daemon: &str, dir: &str) -> EngineLayout {
        EngineLayout {
            kind: EngineKind::System,
            daemon: daemon.to_string(),
            dir: dir.trim_end_matches('/').to_string(),
        }
    }

    /// `<dir>/plugins`.
    pub fn plugins_dir(&self) -> String {
        engine_plugins_path(&self.dir)
    }
}

/// Раскладки движка на машине покупателя, по приоритету: пакеты
/// дистрибутива, затем вшитый комплект. Одна точка правды для поиска демона,
/// флага `--plugin` и сторожа пакета.
///
/// Системный пакет первым: его обновляет пакетный менеджер вместе с
/// библиотеками системы, а вшитый комплект (поставка Steam, ручная
/// установка) — только запасной путь там, где пакета движка нет. Стоят оба —
/// запускается системный, плагины комплекта он получает флагом `--plugin`
/// ([`plugin_root_for`]), и из двух копий плагина демон берёт новую.
pub fn engine_layouts() -> Vec<EngineLayout> {
    vec![
        EngineLayout::system_in(SYSTEM_DAEMON, SYSTEM_ENGINE_DIR),
        EngineLayout::bundled_in(ENGINE_DIR),
    ]
}

/// Раскладка, чей демон — `binary` (путь из поиска демона, до ссылок).
pub fn layout_of<'a>(layouts: &'a [EngineLayout], binary: &str) -> Option<&'a EngineLayout> {
    layouts.iter().find(|l| l.daemon == binary)
}

/// Каталог данных, который демон видит сам: `<префикс>/share/waywallen` для
/// файла `<префикс>/bin/<имя>`. `resolved` — НАСТОЯЩИЙ путь файла (после
/// символьных ссылок): по нему демон и считает свой префикс.
pub fn daemon_own_engine_dir(resolved: &str) -> Option<String> {
    let p = std::path::Path::new(resolved);
    let prefix = p.parent()?.parent()?;
    Some(prefix.join("share/waywallen").to_string_lossy().into_owned())
}

/// Какой каталог передать демону флагом `--plugin`.
///
/// `present` — раскладки, у которых `plugins/` реально лежит на диске (по
/// приоритету [`engine_layouts`]); `resolved_binary` — настоящий путь
/// бинаря демона, который будет запущен (`None` — не известен). Берётся
/// первая раскладка, чей каталог демон не видит сам
/// ([`daemon_own_engine_dir`]); `None` — флаг не нужен.
pub fn plugin_root_for(present: &[EngineLayout], resolved_binary: Option<&str>) -> Option<String> {
    let own = resolved_binary.and_then(daemon_own_engine_dir);
    present
        .iter()
        .map(|l| l.dir.clone())
        .find(|d| own.as_deref() != Some(d.as_str()))
}

/// Аргументы запуска демона: [`DAEMON_ARGS`] и, если у комплекта есть
/// каталог плагинов, `--plugin <каталог движка>`.
///
/// `plugin_root` — каталог движка, если в нём лежит `plugins/` (наблюдение
/// снимает исполнитель), `None` — комплекта нет.
///
/// Какой плагин побеждает, если тот же `id` лежит и в комплекте, и у
/// человека в `~/.local/share/waywallen/plugins`, решает САМ демон, и
/// решение у него ровно то, какое нам нужно: новее по `version` из
/// `plugin.toml` (семантическое сравнение), при равенстве — копия человека
/// (`select_active_plugins`/`plugin_priority`, waywallen v0.4.2). Значит:
///   * пакет обновился, а у человека завалялась старая ручная копия — работает
///     новая из пакета, старая не мешает;
///   * человек сам поставил плагин новее комплекта — работает его копия, и
///     обновление пакета её не «откатит».
/// Дублировать этот выбор у себя мы не стали: второе решение рядом с
/// решением демона разъехалось бы с ним молча. Зеркало правила для
/// отчётов и проверок — [`active_plugin_source`].
pub fn engine_launch_args(plugin_root: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = DAEMON_ARGS.iter().map(|a| a.to_string()).collect();
    if let Some(root) = plugin_root {
        args.push(PLUGIN_FLAG.to_string());
        args.push(root.trim_end_matches('/').to_string());
    }
    args
}

/// Откуда демон возьмёт плагин, лежащий в двух местах.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSource {
    /// Копия из комплекта пакета ([`ENGINE_DIR`]).
    Bundled,
    /// Копия человека (`$XDG_DATA_HOME/waywallen/plugins`).
    User,
}

/// Зеркало выбора демона между копиями одного плагина (см.
/// [`engine_launch_args`]): новее по версии, при равенстве — копия
/// человека. Версия, не разобранная как `X.Y.Z`, сравнивается строкой —
/// как у демона.
pub fn active_plugin_source(bundled_version: &str, user_version: &str) -> PluginSource {
    let norm = |v: &str| v.trim().trim_start_matches(['v', 'V']).to_string();
    let (b, u) = (norm(bundled_version), norm(user_version));
    let semver = |v: &str| -> Option<Vec<u128>> {
        let core = v.split(['-', '+']).next().unwrap_or("");
        let parts: Vec<&str> = core.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        parts.iter().map(|p| p.parse::<u128>().ok()).collect()
    };
    let order = match (semver(&b), semver(&u)) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => b.cmp(&u),
    };
    if order.is_gt() {
        PluginSource::Bundled
    } else {
        PluginSource::User
    }
}

// ── Живой демон не взял плагин из пакета ────────────────────────────────────
//
// Дефект живой проверки 24.10.2026: в списке плагинов waywallen стояла копия
// человека 0.3.0 и предложение обновиться до 0.3.1, хотя пакет Rexon уже
// положил 0.3.1 в [`ENGINE_DIR`]. Сам выбор копии у демона правильный (новее
// по версии, при равенстве — копия человека, см. [`engine_launch_args`]), но
// делает он его ОДИН раз — когда сканирует корни плагинов при старте
// (`daemon/bootstrap.rs`, `scan_plugin_roots`) и после установки плагина
// через свой API. Команды «пересканировать» у демона нет. Демон, запущенный
// ДО обновления пакета (или кем-то без флага `--plugin`), так и держит
// старую копию, пока его не перезапустят.
//
// Поэтому лекарство — перезапуск демона, а не правка файлов человека:
//   * удалять его копию не нужно — после перезапуска демон сам её не
//     возьмёт, пока в пакете новее;
//   * «обновить» её штатной кнопкой waywallen — значит поставить самую
//     свежую версию с GitHub мимо пакета, и дальше она перекрывала бы уже
//     пакетную;
//   * выключить одну копию у демона нечем — такого запроса нет.

/// Копия плагина: из `plugin.toml` комплекта или активная у демона.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCopy {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// `id`, `name` и `version` из текста `plugin.toml` (секция `[plugin]`).
/// `None` — манифест битый или без `id`/`version`.
pub fn plugin_manifest_copy(toml_text: &str) -> Option<PluginCopy> {
    let value = toml_text.parse::<toml::Value>().ok()?;
    let plugin = value.get("plugin")?;
    let field = |key: &str| plugin.get(key).and_then(|v| v.as_str()).map(str::trim);
    let id = field("id").filter(|s| !s.is_empty())?;
    let version = field("version").filter(|s| !s.is_empty())?;
    Some(PluginCopy {
        id: id.to_string(),
        name: field("name").unwrap_or_default().to_string(),
        version: version.to_string(),
    })
}

/// Активные копии плагинов из ответа демона на `plugin_list`. `None` —
/// кадр не разобрался, это не ответ `plugin_list` или демон ответил ошибкой.
pub fn active_plugins_from_frame(frame_bytes: &[u8]) -> Option<Vec<PluginCopy>> {
    let frame = pb::ServerFrame::decode(frame_bytes).ok()?;
    let pb::server_frame::Kind::Response(rsp) = frame.kind? else {
        return None;
    };
    if rsp.status != pb::Status::Ok as i32 {
        return None;
    }
    let pb::response::Payload::PluginList(list) = rsp.payload? else {
        return None;
    };
    Some(
        list.plugins
            .into_iter()
            .map(|p| PluginCopy { id: p.id, name: p.name, version: p.version })
            .collect(),
    )
}

/// Плагин комплекта, которого живой демон не взял, хотя по своему же правилу
/// обязан был.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StalePlugin {
    pub id: String,
    pub name: String,
    pub bundled_version: String,
    /// Версия, которую держит демон; пусто — плагина у демона нет вовсе
    /// (его запустили без `--plugin`, а копии у человека нет).
    pub active_version: String,
}

/// Плагины комплекта, которые демон держит в старой версии (или не видит).
///
/// Правило то же, что у демона ([`active_plugin_source`]): копия комплекта
/// обязана работать, если она СТРОГО новее активной. Активная новее или
/// равна — так и задумано (человек поставил свою), это не дефект.
pub fn stale_bundled_plugins(bundled: &[PluginCopy], active: &[PluginCopy]) -> Vec<StalePlugin> {
    let mut out: Vec<StalePlugin> = bundled
        .iter()
        .filter_map(|b| {
            let active_version = active
                .iter()
                .find(|a| a.id == b.id)
                .map(|a| a.version.clone())
                .unwrap_or_default();
            let behind = active_version.is_empty()
                || active_plugin_source(&b.version, &active_version) == PluginSource::Bundled;
            behind.then(|| StalePlugin {
                id: b.id.clone(),
                name: if b.name.is_empty() { b.id.clone() } else { b.name.clone() },
                bundled_version: b.version.clone(),
                active_version,
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Ключ попытки: какие плагины и до каких версий перезапуск должен поднять.
/// Тот же ключ второй раз — перезапуск уже не помог. Ключ печатается в
/// журнал, поэтому читается слева направо: «у демона → в пакете»
/// (`id=0.3.1→0.3.2`; плагина у демона нет — `id=нет→0.3.2`). Прежний вид
/// `id=0.3.2<0.3.1` читался как «0.3.2 меньше 0.3.1».
pub fn stale_plugins_key(stale: &[StalePlugin]) -> String {
    stale
        .iter()
        .map(|s| {
            let active = if s.active_version.is_empty() { "нет" } else { s.active_version.as_str() };
            format!("{}={active}→{}", s.id, s.bundled_version)
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Что делать после сверки плагинов живого демона с комплектом.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRefresh {
    /// Демон держит то, что должен.
    Fine,
    /// Перезапустить демона; `key` запомнить ([`stale_plugins_key`]).
    Restart { key: String, stale: Vec<StalePlugin> },
    /// Ради ровно этого уже перезапускали — не помогло. Второй раз не
    /// трогаем: перезапуск по кругу гасил бы обои каждые несколько секунд.
    GiveUp { stale: Vec<StalePlugin> },
}

/// Решение сверки. `tried_key` — ключ прошлой попытки в этом процессе.
pub fn plugin_refresh_decision(
    bundled: &[PluginCopy],
    active: &[PluginCopy],
    tried_key: Option<&str>,
) -> PluginRefresh {
    let stale = stale_bundled_plugins(bundled, active);
    if stale.is_empty() {
        return PluginRefresh::Fine;
    }
    let key = stale_plugins_key(&stale);
    if tried_key == Some(key.as_str()) {
        PluginRefresh::GiveUp { stale }
    } else {
        PluginRefresh::Restart { key, stale }
    }
}

/// Что делать, когда живой демон и движок комплекта на диске разошлись.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineRefresh {
    /// Демон — тот же движок, что на диске (или сверять не с чем).
    Fine,
    /// Перезапустить демона; `key` запомнить ([`engine_refresh_key`]).
    Restart { key: String },
    /// Ради ровно этой пары версий уже перезапускали — не помогло.
    GiveUp,
}

/// Ключ попытки: с какой версии на какую перезапуск должен перевести.
pub fn engine_refresh_key(live: &str, on_disk: &str) -> String {
    format!("engine={live}→{on_disk}")
}

/// Сверка живого демона с AppImage комплекта после замены файла пакетом.
///
/// `runs_bundled` — запуск демона идёт из комплекта (поиск бинаря выбрал
/// его, а не AppImage человека: тот обновляет сам человек, и перезапуск
/// ради чужого файла был бы самоуправством). `live` — свойство `Version`
/// живого демона, `on_disk` — `--version` файла на диске. Сравниваются
/// разобранные номера ([`crate::engine_update::parse_binary_version`]):
/// хвост сборки у одного из источников (`0.5.0-x86_64`) — не повод.
/// Версию не узнать с любой стороны — не трогаем: перезапуск вслепую гасил
/// бы обои без пользы.
pub fn engine_refresh_decision(runs_bundled: bool, live: &str, on_disk: &str, tried_key: Option<&str>) -> EngineRefresh {
    let live = crate::engine_update::parse_binary_version(live);
    let on_disk = crate::engine_update::parse_binary_version(on_disk);
    if !runs_bundled || live.is_empty() || on_disk.is_empty() || live == on_disk {
        return EngineRefresh::Fine;
    }
    let key = engine_refresh_key(&live, &on_disk);
    if tried_key == Some(key.as_str()) {
        EngineRefresh::GiveUp
    } else {
        EngineRefresh::Restart { key }
    }
}

/// Текст уведомления о перезапуске ради нового движка из пакета
/// (заголовок — общий, [`plugin_refresh_title`]).
pub fn engine_refresh_body(live: &str, on_disk: &str) -> String {
    engine_refresh_body_for(EngineKind::Bundled, live, on_disk)
}

/// [`engine_refresh_body`] для движка той раскладки, что его поставила:
/// вшитый приходит с обновлением Rexon, системный — с обновлением системы.
pub fn engine_refresh_body_for(kind: EngineKind, live: &str, on_disk: &str) -> String {
    let (live, on_disk) = (
        crate::engine_update::parse_binary_version(live),
        crate::engine_update::parse_binary_version(on_disk),
    );
    match kind {
        EngineKind::Bundled => crate::i18n::trf(
            // TRANSLATORS: Body of the "The wallpaper engine was restarted" notification. %1 is the old engine version, %2 the new one from the Rexon update.
            "It was still running version %1; the Rexon update brought version %2.",
            &[&live, &on_disk],
        ),
        EngineKind::System => crate::i18n::trf(
            // TRANSLATORS: Body of the "The wallpaper engine was restarted" notification. %1 is the old engine version, %2 the new one that a system update installed.
            "It was still running version %1; a system update installed version %2.",
            &[&live, &on_disk],
        ),
    }
}

/// Вид задачи демона «служба»: долгоживущий цикл (сокет вывода, звук, MPRIS,
/// сторож питания…). Такие задачи стоят в `running` всю жизнь демона.
pub const TASK_KIND_SERVICE: &str = "service";

/// Задачи, которые ЖДУТ события, а не работают: перезапуск им ничего не
/// обрывает. `boot/display-watcher` ждёт первого дисплея и стоит в
/// `running`, пока его нет, — на столе без вывода (MATE, XFCE на X11) это
/// навсегда.
pub const WAITING_TASK_NAMES: &[&str] = &["boot/display-watcher"];

/// Занят ли демон работой, которую перезапуск оборвал бы (скачивание,
/// сканирование, установка плагина, применение обоев): в `ListTasks` есть
/// такая задача в состоянии `running`. Тогда перезапуск откладывается до
/// следующего подключения.
///
/// ⛔ Дефект 30.10.2026 (MATE, демон 0.5.0): считалась ЛЮБАЯ задача в
/// `running`, а у демона 0.5.0 их всегда больше десятка — службы
/// ([`TASK_KIND_SERVICE`]) и ждущий `boot/display-watcher`
/// ([`WAITING_TASK_NAMES`]). Демон выходил «занятым» вечно, и перезапуск
/// ради плагина из пакета откладывался при каждой сверке.
pub fn daemon_busy(tasks: &[DaemonTask]) -> bool {
    tasks.iter().any(|t| {
        t.state == "running"
            && t.kind != TASK_KIND_SERVICE
            && !WAITING_TASK_NAMES.contains(&t.name.as_str())
    })
}

/// Заголовок уведомления о перезапуске ради плагина из пакета.
pub fn plugin_refresh_title() -> String {
    crate::i18n::tr("The wallpaper engine was restarted")
}

/// Текст уведомления: что демон теперь возьмёт из пакета и вместо чего.
pub fn plugin_refresh_body(stale: &[StalePlugin]) -> String {
    plugin_refresh_body_for(EngineKind::Bundled, stale)
}

/// [`plugin_refresh_body`] для плагинов той раскладки, что их поставила.
pub fn plugin_refresh_body_for(kind: EngineKind, stale: &[StalePlugin]) -> String {
    let mut lines = vec![match kind {
        EngineKind::Bundled => crate::i18n::tr(
            "It was still running an older plugin; Rexon has a newer one in its package:",
        ),
        EngineKind::System => crate::i18n::tr(
            "It was still running an older plugin; a system update installed a newer one:",
        ),
    }];
    for s in stale {
        lines.push(if s.active_version.is_empty() {
            crate::i18n::trf("%1 %2 (was not loaded)", &[&s.name, &s.bundled_version])
        } else {
            crate::i18n::trf("%1 %2 instead of %3", &[&s.name, &s.bundled_version, &s.active_version])
        });
    }
    lines.join("\n")
}

// ── Сторож пакета: сверка сразу после обновления ────────────────────────────
//
// Сверка выше идёт при подключении вкладки к демону. Но Rexon обычно живёт в
// лотке, вкладку никто не открывает, и после `pacman -Syu` демон держал бы
// старый плагин до следующего входа в сеанс. Поэтому лоток следит за
// каталогом движка (inotify, `fs_watch`) и после затишья делает ту же сверку.

/// Пауза тишины после последней записи пакетного менеджера, мс: pacman
/// пишет файлы пачкой, а смотреть плагины надо, когда она кончилась.
pub const ENGINE_WATCH_QUIET_MS: i32 = 8000;
/// Потолок ожидания конца пачки, мс: даже если запись не утихает (большое
/// обновление системы), сверить через две минуты от первой.
pub const ENGINE_WATCH_MAX_MS: u64 = 120_000;
/// Повторная сверка, пока демон занят задачами (скачивание, сканирование), мс.
pub const ENGINE_WATCH_BUSY_RETRY_MS: i32 = 60_000;
/// Сколько раз повторять сверку при занятом демоне (час при паузе в минуту);
/// дальше — до следующего изменения пакета или подключения вкладки.
pub const ENGINE_WATCH_BUSY_RETRIES: u32 = 60;
/// Срок разового запроса `plugin_list` сторожа, мс.
pub const ENGINE_WATCH_QUERY_MS: i32 = 5000;

/// За чем следить, чтобы заметить новый плагин в пакете.
///
/// * родитель каталога движка — имя самого каталога (пакет мог заменить его
///   целиком или он появился впервые);
/// * каталог движка — `plugins/` и сам AppImage движка: заменённый пакетом
///   движок живой демон не подхватит, пока его не перезапустят
///   ([`engine_refresh_decision`]);
/// * `plugins/` — любое имя: появилась, пропала или заменена папка плагина;
/// * каждая `plugins/<папка>/` — только `plugin.toml`: версию демон берёт
///   оттуда, а замена библиотек без смены версии сверку не меняет.
///
/// `plugin_dirs` — имена папок в `plugins/` на момент постановки слежки;
/// новые подхватываются, когда сторож переставит слежку после пачки.
///
/// Демон вне каталога движка (пакеты дистрибутива: `/usr/bin/waywallen`) —
/// отдельная цель: его каталог, только имя демона.
pub fn engine_watch_targets(layout: &EngineLayout, plugin_dirs: &[String]) -> Vec<crate::fs_watch::WatchTarget> {
    use crate::fs_watch::WatchTarget;
    let engine = layout.dir.trim_end_matches('/');
    let plugins = engine_plugins_path(engine);
    let mut out = Vec::new();
    if let Some((parent, name)) = engine.rsplit_once('/') {
        if !parent.is_empty() && !name.is_empty() {
            out.push(WatchTarget { dir: parent.to_string(), names: vec![name.to_string()] });
        }
    }
    let mut names = vec![ENGINE_PLUGINS_SUBDIR.to_string()];
    match layout.daemon.rsplit_once('/') {
        Some((dir, name)) if dir == engine => names.push(name.to_string()),
        Some((dir, name)) if !dir.is_empty() && !name.is_empty() => {
            out.push(WatchTarget { dir: dir.to_string(), names: vec![name.to_string()] });
        }
        _ => {}
    }
    out.push(WatchTarget { dir: engine.to_string(), names });
    out.push(WatchTarget { dir: plugins.clone(), names: Vec::new() });
    for d in plugin_dirs {
        if d.is_empty() || d.contains('/') {
            continue;
        }
        out.push(WatchTarget { dir: format!("{plugins}/{d}"), names: vec!["plugin.toml".to_string()] });
    }
    out
}

/// Один шаг поиска бинаря. Порядок списка = приоритет.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonCandidate {
    /// Годен, если это существующий исполняемый ФАЙЛ
    /// (`os.path.isfile && os.access(X_OK)`).
    File(String),
    /// `shutil.which(name)` — поиск исполняемого в `PATH`.
    PathLookup(String),
}

/// Живые проверки ФС для [`find_daemon_binary`]. Ядро само в ФС не лезет —
/// тот же приём, что `WaywallenIo`: транспорт снаружи, порядок внутри.
pub trait DaemonFs {
    /// `os.path.isfile(path) and os.access(path, os.X_OK)`.
    fn is_exec_file(&mut self, path: &str) -> bool;
    /// `shutil.which(name)` → полный путь или `None`.
    fn which(&mut self, name: &str) -> Option<String>;
    /// Версия движка по пути (`X.Y.Z`) либо `None`, если её не узнать.
    ///
    /// Спрашивается ТОЛЬКО когда на диске одновременно лежат движок комплекта
    /// и управляемый AppImage (см. [`find_daemon_binary_in`]) — исполнителю
    /// разрешено запускать `--version`. По умолчанию `None`: тогда остаётся
    /// порядок списка.
    fn binary_version(&mut self, _path: &str) -> Option<String> {
        None
    }
}

/// `~` в начале пути → `home` (порт `os.path.expanduser`; достаточно формы
/// `~/...`, других в модуле нет). Общий расширитель ядра, ветка
/// `~/.config` уважает `$XDG_CONFIG_HOME` — см. [`crate::utils::expand_user`].
pub fn expanduser(path: &str, home: &str) -> String {
    crate::utils::expand_user(path, home, None)
}

/// Полный упорядоченный список кандидатов на бинарь демона.
///
/// `rexon_pkg_dir` — каталог пакета `rexon/` (в Python вычисляется от
/// `__file__`), `home` — домашний каталог. Порядок НАМЕРЕННЫЙ (фикс бага
/// 2026-06-24, покрыт тестом `порядок кандидатов`):
///   1. вшитый в дерево AppImage (`rexon/vendor/`) — переопределение
///      разработчика: положил образ рядом с исходниками, и запускается он;
///   2. движок из пакета дистрибутива ([`SYSTEM_DAEMON`]);
///   3. движок, вшитый в поставку Rexon ([`ENGINE_DIR`]);
///   4. управляемый AppImage в `~/.local/share/waywallen/`;
///   5. `waywallen` в `PATH` (обычно симлинк на управляемый);
///   6. известные запасные пути ([`DAEMON_FALLBACKS`]).
///
/// ⚠️ Между 2–3 и 4 порядок не окончательный: см. [`find_daemon_binary_in`].
pub fn daemon_binary_candidates(
    rexon_pkg_dir: &str,
    home: &str,
) -> Vec<DaemonCandidate> {
    daemon_binary_candidates_in(rexon_pkg_dir, home, ENGINE_DIR)
}

/// [`daemon_binary_candidates`] с явным каталогом движка комплекта (для
/// проверок — настоящий `/usr/lib/rexon/engine` у стенда может и лежать).
pub fn daemon_binary_candidates_in(
    rexon_pkg_dir: &str,
    home: &str,
    engine_dir: &str,
) -> Vec<DaemonCandidate> {
    daemon_binary_candidates_for(rexon_pkg_dir, home, &layouts_with(engine_dir))
}

/// Раскладки [`engine_layouts`] с подменённым каталогом вшитого комплекта
/// (проверкам: настоящий `/usr/lib/rexon/engine` у стенда может и лежать).
fn layouts_with(engine_dir: &str) -> Vec<EngineLayout> {
    engine_layouts()
        .into_iter()
        .map(|l| match l.kind {
            EngineKind::Bundled => EngineLayout::bundled_in(engine_dir),
            EngineKind::System => l,
        })
        .collect()
}

/// [`daemon_binary_candidates`] с явным набором раскладок движка пакетов.
pub fn daemon_binary_candidates_for(
    rexon_pkg_dir: &str,
    home: &str,
    layouts: &[EngineLayout],
) -> Vec<DaemonCandidate> {
    let pkg = rexon_pkg_dir.trim_end_matches('/');
    let mut out: Vec<DaemonCandidate> = BUNDLED_NAMES
        .iter()
        .map(|name| DaemonCandidate::File(format!("{pkg}/vendor/{name}")))
        .collect();
    out.extend(layouts.iter().map(|l| DaemonCandidate::File(l.daemon.clone())));
    out.push(DaemonCandidate::File(managed_appimage_path(home)));
    out.push(DaemonCandidate::PathLookup("waywallen".to_string()));
    out.extend(
        DAEMON_FALLBACKS
            .iter()
            .map(|c| DaemonCandidate::File(expanduser(c, home))),
    );
    out
}

/// Управляемый AppImage в пользовательском data-dir — порт
/// `_managed_appimage`. Сюда dev-режим ставит новые версии движка, и сюда же
/// указывает симлинк `~/.local/bin/waywallen`; этот файл мигрирует БД движка
/// под себя → его надо предпочитать извлечённым устаревшим бинарям.
///
/// Путь НЕ дублируется: единственный источник — [`crate::engine_update`],
/// который этот же файл и устанавливает (правило полноты — одно место
/// правки при смене каталога).
pub fn managed_appimage_path(home: &str) -> String {
    crate::engine_update::managed_appimage(home)
}

/// Пути вшитого AppImage — порт `_bundled_appimage` (кандидаты, без проверок).
pub fn bundled_appimage_paths(rexon_pkg_dir: &str) -> Vec<String> {
    let pkg = rexon_pkg_dir.trim_end_matches('/');
    BUNDLED_NAMES
        .iter()
        .map(|name| format!("{pkg}/vendor/{name}"))
        .collect()
}

/// Порт `find_daemon_binary` — первый годный кандидат из
/// [`daemon_binary_candidates`].
pub fn find_daemon_binary<FS: DaemonFs>(
    fs: &mut FS,
    rexon_pkg_dir: &str,
    home: &str,
) -> Option<String> {
    find_daemon_binary_in(fs, rexon_pkg_dir, home, ENGINE_DIR)
}

/// Движок пакета (вшитый комплект или пакет дистрибутива) или управляемый
/// AppImage — что запускать, если на диске лежат оба.
///
/// Движок пакета стоит в списке выше: пакет ставит его ради того, чтобы обои
/// работали сразу и без сети. Но управляемый AppImage ставит сам человек
/// (кнопка обновления страницы «Разработчик»), и если он НОВЕЕ комплекта,
/// то он уже переписал базу движка под свою схему. Запуск более старого
/// комплекта после этого кончается отказом старта («migration … is
/// missing») — ровно дефект 2026-06-24. Поэтому новее — побеждает он.
///
/// Версию не узнать (старая сборка без `--version`, сбой запуска) — остаётся
/// порядок списка: комплект. Равные версии — тоже комплект: его файл не
/// подменят под живым демоном ни кнопка, ни пакетный менеджер человека.
pub fn prefer_managed_over_bundled(
    bundled_version: Option<&str>,
    managed_version: Option<&str>,
) -> bool {
    match (bundled_version, managed_version) {
        (Some(b), Some(m)) if !b.is_empty() && !m.is_empty() => {
            crate::engine_update::ver_tuple(m) > crate::engine_update::ver_tuple(b)
        }
        _ => false,
    }
}

/// [`find_daemon_binary`] с явным каталогом движка комплекта.
pub fn find_daemon_binary_in<FS: DaemonFs>(
    fs: &mut FS,
    rexon_pkg_dir: &str,
    home: &str,
    engine_dir: &str,
) -> Option<String> {
    find_daemon_binary_for(fs, rexon_pkg_dir, home, &layouts_with(engine_dir))
}

/// [`find_daemon_binary`] с явным набором раскладок движка пакетов.
///
/// Правило «управляемый AppImage побеждает, только если строго новее»
/// ([`prefer_managed_over_bundled`]) одно для обеих раскладок: базу движка
/// под свою схему переписывает любой более новый движок, кто бы его ни
/// поставил.
pub fn find_daemon_binary_for<FS: DaemonFs>(
    fs: &mut FS,
    rexon_pkg_dir: &str,
    home: &str,
    layouts: &[EngineLayout],
) -> Option<String> {
    let managed = managed_appimage_path(home);
    for cand in daemon_binary_candidates_for(rexon_pkg_dir, home, layouts) {
        match cand {
            DaemonCandidate::File(p) => {
                if fs.is_exec_file(&p) {
                    if layout_of(layouts, &p).is_some() && fs.is_exec_file(&managed) {
                        let b = fs.binary_version(&p);
                        let m = fs.binary_version(&managed);
                        if prefer_managed_over_bundled(b.as_deref(), m.as_deref()) {
                            return Some(managed);
                        }
                    }
                    return Some(p);
                }
            }
            DaemonCandidate::PathLookup(name) => {
                if let Some(p) = fs.which(&name) {
                    return Some(p);
                }
            }
        }
    }
    None
}

// ============================================================================
// 2. Автозапуск движка при входе в систему
// ============================================================================

/// Имя systemd user unit автозапуска.
pub const AUTOSTART_UNIT: &str = "rexon-wallpaper-engine.service";

/// Имя `.desktop` XDG-автозапуска (фолбэк для систем без systemd).
pub const AUTOSTART_DESKTOP: &str = "rexon-wallpaper-engine.desktop";

/// Шаблон systemd user unit. `{binary}` — абсолютный путь бинаря демона.
///
/// ⚠️ `KillMode=mixed` обязателен и снимать его нельзя (починка бага #3 шапки
/// модуля). Демон исполняется из AppImage, то есть из FUSE-монтирования,
/// которое держит соседний процесс в ТОЙ ЖЕ cgroup. При умолчательном
/// `control-group` SIGTERM прилетает обоим разом, образ отваливается под ещё
/// живым демоном, и штатное завершение оканчивается SIGBUS вместо кода 0.
///
/// `Description` — по-английски: systemd не умеет локализованных ключей, а
/// видна строка только в `systemctl`/журнале (см. `session_splash`).
const AUTOSTART_UNIT_TMPL: &str = "\
[Unit]
Description=Rexon: wallpaper engine (waywallen)
# Файл управляется Rexon (вкладка WE → «Start at login»); правки затрутся.
After=graphical-session.target
PartOf=graphical-session.target

[Service]
# Демон уже жив вне службы (поднят окном waywallen или вручную) — второй
# копии не будет: служба честно пропускается с причиной в журнале, а не
# выходит «успешно» со словами «already running».
ExecCondition=/bin/sh -c \"if busctl --user status {bus} >/dev/null 2>&1; then echo 'waywallen is already running outside this service; not starting a second copy' >&2; exit 1; fi\"
ExecStart=\"{binary}\" --no-ui --no-tray{plugin}
# Демон живёт в AppImage: FUSE-монтирование держит соседний процесс той же
# cgroup. SIGTERM только главному — иначе образ размонтируется под работающим
# демоном и завершение падает с SIGBUS.
KillMode=mixed
Restart=on-failure
RestartSec=3

[Install]
WantedBy=graphical-session.target
";

// TRANSLATORS: Name of Rexon's wallpaper engine autostart entry (shown in the desktop's autostart settings).
const AUTOSTART_NAME: &str = crate::i18n::n_("Rexon Wallpaper Engine");
// TRANSLATORS: Description of the wallpaper engine autostart entry. waywallen is a program name.
const AUTOSTART_COMMENT: &str = crate::i18n::n_("Start the wallpaper engine (waywallen) at login");

/// Шаблон `.desktop` XDG-автозапуска. `{name}` и `{comment}` — строки
/// `Key=` + `Key[<код>]=` на всех языках каталога
/// ([`crate::system_l10n::desktop_key`]): прежде `Comment` был только
/// русским, без ключа языка, а `Name[ru]` — парой в коде.
const AUTOSTART_DESKTOP_TMPL: &str = "\
[Desktop Entry]
Type=Application
{name}{comment}Exec=\"{binary}\" --no-ui --no-tray{plugin}
Terminal=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
";

/// Содержимое systemd user unit для данного бинаря (без плагинов комплекта).
pub fn autostart_unit_content(binary: &str) -> String {
    autostart_unit_content_with(binary, None)
}

/// Содержимое `.desktop` XDG-автозапуска для данного бинаря.
pub fn autostart_desktop_content(binary: &str) -> String {
    autostart_desktop_content_with(binary, None)
}

/// Хвост `ExecStart`/`Exec` с каталогом плагинов комплекта (см.
/// [`engine_launch_args`]); без комплекта — пусто, строка юнита та же, что
/// до появления комплекта.
fn plugin_tail(plugin_root: Option<&str>) -> String {
    match plugin_root {
        Some(root) => format!(" {PLUGIN_FLAG} \"{}\"", root.trim_end_matches('/')),
        None => String::new(),
    }
}

/// [`autostart_unit_content`] с каталогом плагинов комплекта: служба,
/// поднятая при входе, обязана видеть плагин Open Wallpaper Engine из пакета
/// так же, как демон, запущенный из окна.
pub fn autostart_unit_content_with(binary: &str, plugin_root: Option<&str>) -> String {
    AUTOSTART_UNIT_TMPL
        .replace("{bus}", DBUS_SERVICE)
        .replace("{binary}", binary)
        .replace("{plugin}", &plugin_tail(plugin_root))
}

/// [`autostart_desktop_content`] с каталогом плагинов комплекта.
pub fn autostart_desktop_content_with(binary: &str, plugin_root: Option<&str>) -> String {
    use crate::system_l10n::desktop_key as key;
    // Сначала строки языков (в них нет `{…}`), потом путь — путь с
    // фигурными скобками не должен задеть меток шаблона.
    AUTOSTART_DESKTOP_TMPL
        .replace("{name}", &key("Name", AUTOSTART_NAME))
        .replace("{comment}", &key("Comment", AUTOSTART_COMMENT))
        .replace("{binary}", binary)
        .replace("{plugin}", &plugin_tail(plugin_root))
}

/// Корень пользовательских конфигов: `$XDG_CONFIG_HOME` либо `~/.config`.
/// Реэкспорт единой точки ядра — [`crate::utils::config_home`].
pub fn config_home(xdg_config_home: Option<&str>, home: &str) -> String {
    crate::utils::config_home(xdg_config_home, home)
}

/// Путь systemd user unit.
///
/// ⚠️ Намеренное расхождение с Python (починка, см. шапку модуля):
/// `XDG_CONFIG_HOME` учитывается — иначе при нестандартном значении юнит
/// пишется туда, где systemd его не ищет, и `enable` падает.
pub fn autostart_unit_path(xdg_config_home: Option<&str>, home: &str) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/systemd/user/{AUTOSTART_UNIT}")
}

/// Путь `.desktop` XDG-автозапуска.
pub fn autostart_desktop_path(
    xdg_config_home: Option<&str>,
    home: &str,
) -> String {
    let cfg = config_home(xdg_config_home, home);
    format!("{cfg}/autostart/{AUTOSTART_DESKTOP}")
}

/// Каталог живого user-менеджера systemd — порт условия
/// `os.path.isdir(f"/run/user/{uid}/systemd")`.
pub fn systemd_runtime_dir(uid: u32) -> String {
    format!("/run/user/{uid}/systemd")
}

/// Есть ли живой user-менеджер systemd (иначе — XDG-фолбэк). Наблюдения:
/// `shutil.which("systemctl") is not None` и наличие [`systemd_runtime_dir`].
pub fn systemd_user_available(
    systemctl_present: bool,
    runtime_dir_exists: bool,
) -> bool {
    systemctl_present && runtime_dir_exists
}

/// argv `systemctl --user <args…>` — порт `_systemctl_user`.
pub fn systemctl_user_argv(args: &[&str]) -> Vec<String> {
    let mut argv = vec!["systemctl".to_string(), "--user".to_string()];
    argv.extend(args.iter().map(|a| a.to_string()));
    argv
}

/// Таймаут вызовов `systemctl --user`, секунды.
pub const SYSTEMCTL_TIMEOUT_SECS: u64 = 15;

/// argv проверки «включён ли юнит».
pub fn autostart_is_enabled_argv() -> Vec<String> {
    systemctl_user_argv(&["is-enabled", AUTOSTART_UNIT])
}

/// argv `daemon-reload`.
pub fn autostart_daemon_reload_argv() -> Vec<String> {
    systemctl_user_argv(&["daemon-reload"])
}

/// argv включения юнита.
pub fn autostart_enable_argv() -> Vec<String> {
    systemctl_user_argv(&["enable", AUTOSTART_UNIT])
}

/// Команда для конфигов композиторов, которые `graphical-session.target` не
/// поднимают (sway без sway-systemd, Hyprland без uwsm, river, labwc,
/// Wayfire): включённый юнит там при входе не стартует сам, и строка в
/// конфиге стартует его явно (`crate::compositor_autostart`). Повторный
/// `start` уже работающей службы ничего не делает — двух движков не будет.
/// Переменные сеанса передаются менеджеру прежде старта: без
/// `WAYLAND_DISPLAY` движку не к чему подключиться, а импорт у композиторов
/// идёт своей строкой, наперегонки с нашей.
pub fn autostart_unit_start_argv() -> Vec<String> {
    vec![
        "sh".to_string(),
        "-c".to_string(),
        format!(
            "systemctl --user import-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP 2>/dev/null; \
             exec systemctl --user start {AUTOSTART_UNIT}"
        ),
    ]
}

/// argv выключения юнита.
pub fn autostart_disable_argv() -> Vec<String> {
    systemctl_user_argv(&["disable", AUTOSTART_UNIT])
}

/// Порт `autostart_enabled`: вердикт по наблюдениям.
///
/// `is_enabled` — результат `systemctl --user is-enabled` (код возврата и
/// stdout) либо `None`, если systemd недоступен или вызов упал (в Python —
/// перехваченный `OSError`/`SubprocessError`). `desktop_exists` — наличие
/// `.desktop` XDG-автозапуска.
pub fn autostart_enabled(
    is_enabled: Option<(i32, &str)>,
    desktop_exists: bool,
) -> bool {
    if let Some((code, stdout)) = is_enabled {
        if code == 0 && stdout.trim() == "enabled" {
            return true;
        }
    }
    desktop_exists
}

/// План выключения автозапуска — порт ветки `not enabled` в
/// `_set_autostart_inner`. Снимаются ОБА механизма (после смены системы не
/// оставлять хвост).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisablePlan {
    /// argv `systemctl --user disable` — только если systemd жив И юнит есть.
    pub disable_argv: Option<Vec<String>>,
    /// Файл юнита к удалению (отсутствие — не ошибка).
    pub remove_unit: String,
    /// argv `daemon-reload` — выполнить, только если юнит РЕАЛЬНО удалён.
    pub daemon_reload_argv: Option<Vec<String>>,
    /// `.desktop` к удалению (отсутствие — не ошибка).
    pub remove_desktop: String,
    /// Итоговое сообщение (`ok = true`).
    pub message: String,
}

/// Построить [`DisablePlan`].
pub fn disable_plan(
    xdg_config_home: Option<&str>,
    home: &str,
    systemd_available: bool,
    unit_exists: bool,
) -> DisablePlan {
    DisablePlan {
        disable_argv: if systemd_available && unit_exists {
            Some(autostart_disable_argv())
        } else {
            None
        },
        remove_unit: autostart_unit_path(xdg_config_home, home),
        daemon_reload_argv: if systemd_available {
            Some(autostart_daemon_reload_argv())
        } else {
            None
        },
        remove_desktop: autostart_desktop_path(xdg_config_home, home),
        message: crate::i18n::tr("Autostart disabled"),
    }
}

/// План включения автозапуска — порт ветки `enabled` в `_set_autostart_inner`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnablePlan {
    /// Бинарь движка не найден — настраивать нечего.
    NoBinary { message: String },
    /// Основной путь: systemd user unit.
    Systemd {
        unit_path: String,
        unit_content: String,
        daemon_reload_argv: Vec<String>,
        enable_argv: Vec<String>,
        /// Второй механизм параллельно не держим.
        remove_desktop: String,
        message: String,
    },
    /// Фолбэк для систем без systemd.
    Xdg { desktop_path: String, desktop_content: String, message: String },
}

/// Построить [`EnablePlan`]. `binary` — результат [`find_daemon_binary`]
/// (АБСОЛЮТНЫЙ путь; повторное включение просто перезаписывает файл).
pub fn enable_plan(
    binary: Option<&str>,
    xdg_config_home: Option<&str>,
    home: &str,
    systemd_available: bool,
) -> EnablePlan {
    enable_plan_with(binary, xdg_config_home, home, systemd_available, None)
}

/// [`enable_plan`] с каталогом плагинов комплекта (см. [`engine_launch_args`]).
pub fn enable_plan_with(
    binary: Option<&str>,
    xdg_config_home: Option<&str>,
    home: &str,
    systemd_available: bool,
    plugin_root: Option<&str>,
) -> EnablePlan {
    let Some(binary) = binary else {
        return EnablePlan::NoBinary {
            message: crate::i18n::tr("The waywallen engine was not found — autostart is not set up."),
        };
    };
    if systemd_available {
        return EnablePlan::Systemd {
            unit_path: autostart_unit_path(xdg_config_home, home),
            unit_content: autostart_unit_content_with(binary, plugin_root),
            daemon_reload_argv: autostart_daemon_reload_argv(),
            enable_argv: autostart_enable_argv(),
            remove_desktop: autostart_desktop_path(xdg_config_home, home),
            message: crate::i18n::tr("Autostart enabled (systemd user unit)"),
        };
    }
    EnablePlan::Xdg {
        desktop_path: autostart_desktop_path(xdg_config_home, home),
        desktop_content: autostart_desktop_content_with(binary, plugin_root),
        message: crate::i18n::tr("Autostart enabled (XDG autostart)"),
    }
}

/// Сообщение о неудаче `systemctl enable` — `stderr`, а если он пуст, `stdout`
/// (оба обрезаются по краям, порт `r.stderr.strip() or r.stdout.strip()`).
pub fn enable_failed_message(stderr: &str, stdout: &str) -> String {
    let s = stderr.trim();
    let text = if s.is_empty() { stdout.trim() } else { s };
    format!("systemctl enable: {text}")
}

/// Сообщение внешнего перехвата `set_autostart`.
pub fn set_autostart_error_message(err: &str) -> String {
    crate::i18n::trf("Could not set up autostart: %1", &[&err])
}

/// Бинарь из строки `ExecStart="…" --no-ui --no-tray` уже записанного юнита.
///
/// Нужен освежению: путь к движку выбрал пользователь (или прежняя установка),
/// и переписывание юнита не смеет его подменять.
pub fn unit_binary(existing: &str) -> Option<String> {
    let line = existing
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("ExecStart="))?;
    let value = line["ExecStart=".len()..].trim();
    let path = match value.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => value.split_whitespace().next()?,
    };
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// Что делать с уже лежащим на диске юнитом автозапуска.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitRefresh {
    /// Юнита нет (автозапуск не включён) либо `ExecStart` не разобран —
    /// трогать нечего.
    Skip,
    /// Содержимое совпадает с актуальным шаблоном.
    UpToDate,
    /// Юнит устарел: переписать и перечитать конфигурацию systemd.
    Rewrite {
        unit_path: String,
        unit_content: String,
        daemon_reload_argv: Vec<String>,
    },
}

/// План освежения ранее записанного юнита автозапуска.
///
/// Зачем: юнит пишется ОДИН раз — в момент включения «Start at login», и потом
/// живёт у пользователя годами. Любая починка шаблона (например `KillMode=mixed`,
/// баг #3 шапки модуля) без этого механизма достаётся только тем, кто заново
/// щёлкнул переключатель. Здесь шаблон пересобирается на СТАРОМ пути бинаря
/// ([`unit_binary`]) — выбор движка сохраняется, чинится только обвязка.
///
/// `existing` — содержимое файла юнита (`None`, если файла нет).
pub fn refresh_unit_plan(
    existing: Option<&str>,
    xdg_config_home: Option<&str>,
    home: &str,
) -> UnitRefresh {
    refresh_unit_plan_with(existing, xdg_config_home, home, None)
}

/// [`refresh_unit_plan`] с каталогом плагинов комплекта.
///
/// Бинарь юнита по-прежнему не трогается (его выбрал человек или прежняя
/// установка), а хвост `--plugin` следует за комплектом: поставили пакет —
/// служба начинает видеть его плагин; сняли — флаг уходит, и демон не
/// ищет каталог, которого больше нет.
pub fn refresh_unit_plan_with(
    existing: Option<&str>,
    xdg_config_home: Option<&str>,
    home: &str,
    plugin_root: Option<&str>,
) -> UnitRefresh {
    let Some(binary) = existing.and_then(unit_binary) else {
        return UnitRefresh::Skip;
    };
    refresh_unit_plan_on(existing, &binary, xdg_config_home, home, plugin_root)
}

/// Бинарь, на котором пересобрать юнит: записанный в нём, пока он на диске;
/// пропал — найденный сейчас ([`find_daemon_binary`]); не нашлось ничего —
/// прежний (юнит без движка всё равно не поднимется, а чужой выбор не
/// затираем).
///
/// Зачем: 30.10.2026 пакет Rexon перестал вшивать движок
/// (`/usr/lib/rexon/engine/waywallen.AppImage` ушёл вместе с обновлением), и
/// юнит, записанный на этот путь, после обновления пакета смотрел бы в
/// пустоту — обои пропали бы со следующего входа.
pub fn unit_refresh_binary(written: &str, written_alive: bool, found: Option<&str>) -> String {
    match found {
        Some(f) if !written_alive && !f.is_empty() => f.to_string(),
        _ => written.to_string(),
    }
}

/// [`refresh_unit_plan_with`] на явно выбранном бинаре
/// ([`unit_refresh_binary`]); `existing` — содержимое юнита.
pub fn refresh_unit_plan_on(
    existing: Option<&str>,
    binary: &str,
    xdg_config_home: Option<&str>,
    home: &str,
    plugin_root: Option<&str>,
) -> UnitRefresh {
    let Some(existing) = existing else {
        return UnitRefresh::Skip;
    };
    let unit_content = autostart_unit_content_with(binary, plugin_root);
    if unit_content == existing {
        return UnitRefresh::UpToDate;
    }
    UnitRefresh::Rewrite {
        unit_path: autostart_unit_path(xdg_config_home, home),
        unit_content,
        daemon_reload_argv: autostart_daemon_reload_argv(),
    }
}

// ============================================================================
// 3. Решения DaemonControl
// ============================================================================

/// Статус демона — порт датакласса `DaemonStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub version: String,
    pub ws_port: i64,
    pub current_wallpaper_id: String,
}

/// Порт `DaemonControl.probe` — синхронный опрос состояния демона.
///
/// Свойства читаются через уже существующий трейт [`WaywallenIo`] (`WsPort` /
/// `Version` / `CurrentWallpaperId` не дублируются — правило #12). Отсутствие
/// `WsPort` ([`DbusValue::None`], в Python — `reply.type() != ReplyMessage`
/// или пустые аргументы) означает «демон не отвечает».
///
/// ⚠️ Намеренное расхождение: нечисловое `WsPort` даёт `0` (Python падал
/// необработанным `ValueError`).
pub fn probe<IO: WaywallenIo>(io: &mut IO) -> DaemonStatus {
    let ws_port = io.get_property("WsPort");
    if ws_port == DbusValue::None {
        return DaemonStatus::default();
    }
    DaemonStatus {
        running: true,
        ws_port: ws_port.py_int().unwrap_or(0),
        version: str_or_empty(io.get_property("Version")),
        current_wallpaper_id: str_or_empty(
            io.get_property("CurrentWallpaperId"),
        ),
    }
}

/// Порт `str(value or "")`.
fn str_or_empty(v: DbusValue) -> String {
    if v.is_truthy() { v.py_str() } else { String::new() }
}

/// argv detached-запуска демона: `(program, args)`. Демон должен пережить
/// закрытие Rexon — запуск отдельной сессией делает окно (`we_socket::spawn_engine`).
pub fn daemon_launch_argv(binary: &str) -> (String, Vec<String>) {
    daemon_launch_argv_with(binary, None)
}

/// [`daemon_launch_argv`] с каталогом плагинов комплекта.
pub fn daemon_launch_argv_with(binary: &str, plugin_root: Option<&str>) -> (String, Vec<String>) {
    (binary.to_string(), engine_launch_args(plugin_root))
}

/// Сообщение «бинарь не найден» из `ensure_running`.
pub fn daemon_not_found_message() -> String {
    format!(
        "Движок waywallen не найден. Ожидается вшитый AppImage (rexon/vendor/), \
         пакет дистрибутива ({SYSTEM_DAEMON}), движок пакета ({ENGINE_DIR}/{ENGINE_APPIMAGE_NAME}), \
         ~/.local/share/waywallen/waywallen.AppImage, в PATH или \
         ~/.local/bin/waywallen."
    )
}

/// Сообщение о неудачном detached-запуске.
pub fn daemon_launch_failed_message(binary: &str) -> String {
    format!("Не удалось запустить {binary}")
}

/// Сообщение таймаута поллинга.
pub fn daemon_poll_timeout_message() -> String {
    "Демон waywallen не поднялся за 12 секунд.".to_string()
}

/// Сообщение «транспортный слой не собран» из `ensure_running`.
///
/// Текст жил литералом в `client.py` — единственный из сообщений клиента,
/// который не спрашивали у ядра. Одна подпись — одно место правки
/// (правило #12), иначе следующее переименование скрипта сборки разъедется
/// с остальными такими же строками вкладки.
pub fn transport_missing_message() -> String {
    crate::i18n::tr(
        "The wallpaper engine module is not built: build it with tools/build_qml_control.sh",
    )
}

/// Сообщение о неразобранном кадре сервера (несовпадение версий протокола).
pub fn frame_parse_error_message(err: &str) -> String {
    crate::i18n::trf("Could not parse a server frame: %1", &[&err])
}

/// Показывать ли тостом очередную ошибку транспорта (сокет, битый кадр).
///
/// Пока демона нет, транспорт пробует переподключиться снова и снова, и
/// каждая попытка даёт ту же ошибку сокета. Тост на каждую — это стена
/// одинаковых сообщений, которые человек не может ни прочесть, ни закрыть
/// (правило #13). Поэтому один тост на одно «пропало соединение»; следующий —
/// только когда соединение восстановилось и пропало снова, либо когда
/// ошибка стала ДРУГОЙ (новая причина — новое знание для человека).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TransportErrorToast {
    /// Текст последнего показанного тоста в текущем обрыве; `None` — с
    /// последнего подключения тостов не было.
    shown: Option<String>,
}

impl TransportErrorToast {
    /// Пришла ошибка. `true` — показать её тостом.
    pub fn on_error(&mut self, text: &str) -> bool {
        if self.shown.as_deref() == Some(text) {
            return false;
        }
        self.shown = Some(text.to_owned());
        true
    }

    /// Соединение установлено: следующая ошибка — уже новый обрыв.
    pub fn on_connected(&mut self) {
        self.shown = None;
    }
}

/// Решение `ensure_running`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnsureAction {
    /// Демон жив → `ready(status)` сразу.
    Ready { status: DaemonStatus },
    /// Бинарь не найден → `failed(message)`.
    NoBinary { message: String },
    /// Автозапуск + поллинг D-Bus.
    Launch {
        program: String,
        args: Vec<String>,
        polls_left: i64,
        poll_interval_ms: u64,
    },
}

impl EnsureAction {
    /// Нужно ли перед действием тихо поставить+выбрать Plasma-плагин
    /// `org.waywallen.kde` (best-effort, KDE-only).
    ///
    /// ТОЛЬКО при живом/запускаемом движке: без него containment-плагин
    /// подменит обои пользователя чёрным экраном.
    pub fn ensure_kde_plugin(&self) -> bool {
        !matches!(self, EnsureAction::NoBinary { .. })
    }
}

/// Порт `DaemonControl.ensure_running` (без побочных эффектов).
///
/// Живым демон считается при `running && ws_port != 0` — как `st.running and
/// st.ws_port` в Python.
pub fn ensure_running_decision(
    status: &DaemonStatus,
    binary: Option<&str>,
) -> EnsureAction {
    ensure_running_decision_with(status, binary, None)
}

/// [`ensure_running_decision`] с каталогом плагинов комплекта.
pub fn ensure_running_decision_with(
    status: &DaemonStatus,
    binary: Option<&str>,
    plugin_root: Option<&str>,
) -> EnsureAction {
    if status.running && status.ws_port != 0 {
        return EnsureAction::Ready { status: status.clone() };
    }
    match binary {
        None => EnsureAction::NoBinary { message: daemon_not_found_message() },
        Some(bin) => {
            let (program, args) = daemon_launch_argv_with(bin, plugin_root);
            EnsureAction::Launch {
                program,
                args,
                polls_left: POLL_LIMIT,
                poll_interval_ms: POLL_MS,
            }
        }
    }
}

/// [`ensure_running_decision_with`], когда известно, есть ли служба
/// автозапуска (`service` — юнит [`AUTOSTART_UNIT`] записан и systemd
/// пользователя жив).
///
/// Есть служба — демон поднимается ЧЕРЕЗ неё (`systemctl --user start`): так
/// он один и под присмотром службы, с её `--plugin` и `KillMode`. Прежде
/// вкладка запускала второй, отдельный процесс мимо службы, и та потом при
/// старте натыкалась на чужого демона (19.10.2026). Уже работающий демон —
/// чей угодно — не трогается.
pub fn ensure_running_decision_service(
    status: &DaemonStatus,
    binary: Option<&str>,
    plugin_root: Option<&str>,
    service: bool,
) -> EnsureAction {
    let action = ensure_running_decision_with(status, binary, plugin_root);
    match action {
        // Переменные сеанса — менеджеру прежде старта (как строка
        // автозапуска композиторов): без `WAYLAND_DISPLAY` демону службы не
        // к чему подключиться, если сеанс их не передал сам.
        EnsureAction::Launch { polls_left, poll_interval_ms, .. } if service => {
            let mut argv = autostart_unit_start_argv();
            let program = argv.remove(0);
            EnsureAction::Launch { program, args: argv, polls_left, poll_interval_ms }
        }
        other => other,
    }
}

/// Решение одного тика поллинга.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PollAction {
    /// Демон поднялся → остановить таймер, `ready(status)`.
    Ready { status: DaemonStatus },
    /// Ждём дальше; сколько попыток осталось.
    Continue { polls_left: i64 },
    /// Попытки кончились → остановить таймер, `failed(message)`.
    Timeout { message: String },
}

/// Порт `DaemonControl._poll_once`.
pub fn poll_decision(status: &DaemonStatus, polls_left: i64) -> PollAction {
    if status.running && status.ws_port != 0 {
        return PollAction::Ready { status: status.clone() };
    }
    let left = polls_left - 1;
    if left <= 0 {
        return PollAction::Timeout { message: daemon_poll_timeout_message() };
    }
    PollAction::Continue { polls_left: left }
}

// ============================================================================
// Сторож запуска движка: ждать, пока процесс жив и работает
// ============================================================================
//
// ⛔ Дефект 30.10.2026 (ВМ, пакет r1895): первое открытие вкладки сразу после
// установки пакета кончалось экраном «движок запускается слишком долго» —
// [`poll_decision`] сдавался ровно через 24 × 500 мс, хотя движок честно
// стартовал: первый запуск AppImage на медленной машине (монтирование и
// распаковка образа, миграции базы) длится дольше 12 с. «Повторить запуск»
// тут же всё открывал. Покупатель видел ошибку там, где ошибки не было
// (правило #13).
//
// Теперь срок не один, а три, и судит сторож по ПРОЦЕССУ, а не по часам:
//   * первые [`START_QUIET_MS`] — просто ждём шину, как прежде;
//   * дальше ждём, пока процесс жив и подаёт признаки жизни (растёт время
//     процессора или счётчики ввода-вывода у него или у его потомков); на
//     экране — «запускается…» с пояснением, что первый запуск бывает долгим;
//   * отказ — только если процесс умер, затих на [`START_STALL_MS`] или
//     истёк предельный срок [`START_HARD_LIMIT_MS`] (зациклился).
// Умер процесс — отказ сразу, а не по истечении 12 с: ждать нечего.

/// Тихое окно запуска, мс: столько ждём шину, ни о чём не спрашивая процесс.
/// Это прежний жёсткий срок ([`POLL_LIMIT`] × [`POLL_MS`]): обычный старт
/// укладывается в 3–5 с (замер в шапке `we_window::CONNECT_TIMEOUT_MS`).
pub const START_QUIET_MS: u64 = POLL_LIMIT as u64 * POLL_MS;

/// Сколько процесс может не подавать признаков жизни после тихого окна, мс,
/// прежде чем счесть его зависшим. Признаки жизни не видны вовсе (нет
/// `/proc`, служба не отвечает) — тот же срок отсчитывается от запуска.
pub const START_STALL_MS: u64 = 30_000;

/// Предельный срок запуска, мс: дольше не ждём даже работающий процесс —
/// так ловится зацикливание, при котором процессор занят, а шины нет.
pub const START_HARD_LIMIT_MS: u64 = 180_000;

/// Снимок признаков жизни запущенного движка.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineLife {
    /// Процесс жив (для службы — юнит запускается или работает).
    pub alive: bool,
    /// Счётчик работы дерева процессов: время процессора в тиках плюс байты
    /// ввода-вывода. Важно не значение, а то, меняется ли оно.
    pub activity: u64,
    /// Сколько раз служба перезапускала движок (`NRestarts`); у процесса
    /// вне службы — 0. Рост за время запуска значит: движок упал.
    pub restarts: u32,
}

/// Решение одного тика сторожа запуска.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartStep {
    /// Движок на шине и слушает порт — запуск окончен.
    Ready { status: DaemonStatus },
    /// Ждём дальше. `slow` — тихое окно позади: пора сказать человеку, что
    /// первый запуск бывает долгим.
    Wait { slow: bool },
    /// Запуск не удался; `message` — СЫРАЯ причина для журнала (подпись для
    /// человека складывает `we_window::no_daemon_text_in`).
    Failed { message: String },
}

/// Сторож запуска движка. Тикает с шагом поллинга шины.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartWatch {
    elapsed_ms: u64,
    idle_ms: u64,
    last_activity: Option<u64>,
    seen_alive: bool,
    first_restarts: Option<u32>,
}

impl StartWatch {
    /// `started` — процесс точно был запущен нами (известен его PID).
    /// Служба (`systemctl --user start`) поднимает движок не сразу, и пока
    /// юнит ещё не перешёл в «запускается», «мёртв» не значит «умер».
    pub fn new(started: bool) -> Self {
        Self { seen_alive: started, ..Self::default() }
    }

    /// Сколько уже ждём, мс.
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }

    /// Очередной тик: `status` — шина, `life` — признаки жизни процесса
    /// (`None` — узнать не удалось), `step_ms` — шаг с прошлого тика.
    pub fn tick(&mut self, status: &DaemonStatus, life: Option<EngineLife>, step_ms: u64) -> StartStep {
        if status.running && status.ws_port != 0 {
            return StartStep::Ready { status: status.clone() };
        }
        self.elapsed_ms = self.elapsed_ms.saturating_add(step_ms);
        match life {
            Some(life) => {
                let first = *self.first_restarts.get_or_insert(life.restarts);
                let restarted = life.restarts > first;
                if life.alive && !restarted {
                    self.seen_alive = true;
                    if self.last_activity == Some(life.activity) {
                        self.idle_ms = self.idle_ms.saturating_add(step_ms);
                    } else {
                        self.last_activity = Some(life.activity);
                        self.idle_ms = 0;
                    }
                } else {
                    if self.seen_alive || restarted || self.elapsed_ms >= START_QUIET_MS {
                        return StartStep::Failed { message: daemon_exited_message() };
                    }
                    self.idle_ms = self.idle_ms.saturating_add(step_ms);
                }
            }
            None => self.idle_ms = self.idle_ms.saturating_add(step_ms),
        }
        let secs = self.elapsed_ms / 1000;
        if self.elapsed_ms >= START_HARD_LIMIT_MS {
            return StartStep::Failed { message: daemon_start_timeout_message(secs, false) };
        }
        if self.elapsed_ms >= START_QUIET_MS && self.idle_ms >= START_STALL_MS {
            return StartStep::Failed { message: daemon_start_timeout_message(secs, true) };
        }
        StartStep::Wait { slow: self.elapsed_ms >= START_QUIET_MS }
    }
}

/// Сырая причина: процесс движка завершился, не выйдя на связь.
pub fn daemon_exited_message() -> String {
    "Процесс движка waywallen завершился, не выйдя на связь.".to_string()
}

/// Начало сырой причины «не вышел на связь за срок» — по нему отказ
/// опознаёт `we_window::daemon_failure`.
pub const START_TIMEOUT_HEAD: &str = "Демон waywallen не вышел на связь за";

/// Сырая причина таймаута сторожа запуска. `stalled` — процесс затих;
/// иначе истёк предельный срок.
pub fn daemon_start_timeout_message(secs: u64, stalled: bool) -> String {
    let why = if stalled {
        "процесс перестал подавать признаки жизни"
    } else {
        "истёк предельный срок запуска"
    };
    format!("{START_TIMEOUT_HEAD} {secs} с: {why}.")
}

// ── Что рассказать журналу, когда движок умер на запуске ────────────────────
//
// ⏳ Случай 30.10.2026 (ВМ, чистый профиль, движок из пакета системы): первый
// запуск кончился «процесс завершился, не выйдя на связь», повторные шли.
// Журналов не осталось, а в нашем было бы нечего читать: демон запускался
// `QProcess::startDetached` — код выхода у чужого потомка не узнать, а его
// stderr молча уходил в rexon.log вперемешку с нашими строками (окно уводит
// туда свой дескриптор 2, демон его наследовал).
//
// Что известно по исходнику демона v0.5.0 (`daemon/bootstrap.rs`,
// `system/dbus.rs`): имя шины он захватывает ДО базы, сканирования плагинов
// и публикации объекта; второй экземпляр при занятом имени и `--no-ui`
// печатает «already running, no UI to launch» и выходит с кодом 0, ещё не
// открыв порт. Проба шины (`WsPort`) всё это время отвечает «не запущен».
// Остальные ранние выходы — код 1 и строка `Error: …` в stderr (сокет
// вывода, база, переводы плагинов). Отсюда три сведения, которые отличают
// причины друг от друга: код выхода, хвост stderr демона и кто держит имя
// шины в момент отказа.

/// Файл вывода демона, запущенного Rexon (stdout и stderr), — рядом с
/// rexon.log. Перезаписывается при каждом запуске: история демона и так
/// лежит в его собственном журнале (`~/.local/state/waywallen/logs`).
pub const DAEMON_OUTPUT_LOG: &str = "waywallen-daemon.log";

/// Сколько последних строк вывода демона класть в журнал при отказе.
pub const DAEMON_OUTPUT_TAIL_LINES: usize = 15;

/// Путь файла вывода демона в каталоге журнала `log_dir`.
pub fn daemon_output_log_path(log_dir: &str) -> String {
    format!("{}/{DAEMON_OUTPUT_LOG}", log_dir.trim_end_matches('/'))
}

/// Последние `max` непустых строк текста (без хвостовых пробелов).
pub fn text_tail(text: &str, max: usize) -> Vec<String> {
    let lines: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect();
    let from = lines.len().saturating_sub(max);
    lines[from..].iter().map(|l| l.to_string()).collect()
}

/// argv «чей процесс держит имя шины демона» (`GetConnectionUnixProcessID`).
pub fn daemon_name_owner_pid_argv() -> Vec<String> {
    [
        "busctl",
        "--user",
        "call",
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetConnectionUnixProcessID",
        "s",
        crate::waywallen_client::BUS_NAME,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Ответ `busctl call … GetConnectionUnixProcessID`: `u 1234` → `1234`.
pub fn parse_name_owner_pid(stdout: &str) -> Option<u32> {
    let mut it = stdout.split_whitespace();
    (it.next()? == "u").then_some(())?;
    it.next()?.parse().ok().filter(|p| *p != 0)
}

/// Как завершился процесс движка: код или сигнал.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineExit {
    /// Ещё жив (отказ по сроку) или узнать не удалось.
    Unknown,
    Code(i32),
    Signal(i32),
}

/// Владелец имени шины демона в момент отказа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusNameOwner {
    /// Имя свободно.
    Free,
    /// Держит процесс `pid` с командной строкой `cmdline` (пусто — не прочлась).
    Process { pid: u32, cmdline: String },
}

/// Строки журнала об отказе запуска: код выхода, владелец имени шины и
/// хвост вывода демона. Только журнал — человеку это не показывается.
///
/// Код 0 при чужом владельце имени — почти наверняка второй экземпляр:
/// движок поднимал кто-то ещё (служба, другое окно, плагин рабочего стола),
/// и наш процесс уступил ему имя.
pub fn daemon_exit_report(
    pid: u32,
    exit: EngineExit,
    owner: Option<&BusNameOwner>,
    output_path: &str,
    tail: &[String],
) -> Vec<String> {
    let mut out = Vec::new();
    let how = match exit {
        EngineExit::Unknown => "процесс жив или код выхода неизвестен".to_string(),
        EngineExit::Code(c) => format!("код выхода {c}"),
        EngineExit::Signal(s) => format!("убит сигналом {s}"),
    };
    out.push(format!("движок pid {pid}: {how}"));
    match owner {
        None => out.push("имя шины демона: узнать не удалось".to_string()),
        Some(BusNameOwner::Free) => out.push("имя шины демона свободно".to_string()),
        Some(BusNameOwner::Process { pid: owner_pid, cmdline }) => {
            let who = if cmdline.is_empty() { "?" } else { cmdline.as_str() };
            let note = if *owner_pid != pid && exit == EngineExit::Code(0) {
                " — похоже, движок уже поднимал другой процесс, а наш уступил ему имя"
            } else {
                ""
            };
            out.push(format!("имя шины демона держит pid {owner_pid} ({who}){note}"));
        }
    }
    if tail.is_empty() {
        out.push(format!("вывод демона пуст ({output_path})"));
    } else {
        out.push(format!("хвост вывода демона ({output_path}):"));
        out.extend(tail.iter().map(|l| format!("  | {l}")));
    }
    out
}

/// Одна строка `/proc/<pid>/stat`: `(pid, ppid, состояние, utime+stime)`.
///
/// Имя процесса в скобках может содержать пробелы и скобки — режем по
/// последней `)`. Дальше поля с третьего: состояние (3), ppid (4) … utime
/// (14), stime (15).
pub fn parse_proc_stat(text: &str) -> Option<(u32, u32, char, u64)> {
    let open = text.find('(')?;
    let pid = text[..open].trim().parse().ok()?;
    let rest = &text[text.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    let state = f.first()?.chars().next()?;
    let ppid = f.get(1)?.parse().ok()?;
    let utime: u64 = f.get(11)?.parse().ok()?;
    let stime: u64 = f.get(12)?.parse().ok()?;
    Some((pid, ppid, state, utime.saturating_add(stime)))
}

/// Байты ввода-вывода из `/proc/<pid>/io` (`rchar` + `wchar`). Читаются они
/// и при чтении из монтирования образа — то есть растут, пока AppImage
/// распаковывается, даже если процессор ждёт диска.
pub fn parse_proc_io(text: &str) -> u64 {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            matches!(key.trim(), "rchar" | "wchar").then(|| value.trim().parse::<u64>().ok())?
        })
        .fold(0u64, u64::saturating_add)
}

/// Процесс из `/proc` для [`tree_life`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcEntry {
    pub pid: u32,
    pub ppid: u32,
    /// Состояние из `stat` (`R`, `S`, `Z` …).
    pub state: char,
    /// Время процессора + байты ввода-вывода.
    pub activity: u64,
}

/// Признаки жизни дерева процессов с корнем `root`: жив ли корень (зомби —
/// уже нет) и сумма работы корня и всех его потомков. Потомки нужны
/// потому, что у AppImage образ обслуживает отдельный процесс.
pub fn tree_life(root: u32, entries: &[ProcEntry]) -> EngineLife {
    let alive = entries
        .iter()
        .any(|e| e.pid == root && e.state != 'Z' && e.state != 'X');
    let mut tree = vec![root];
    let mut activity = 0u64;
    let mut i = 0;
    while i < tree.len() {
        let pid = tree[i];
        for e in entries {
            if e.pid == pid {
                activity = activity.saturating_add(e.activity);
            }
            if e.ppid == pid && e.pid != pid && !tree.contains(&e.pid) {
                tree.push(e.pid);
            }
        }
        i += 1;
    }
    EngineLife { alive, activity, restarts: 0 }
}

/// Снять процессы пользователя из `/proc` (свои `io` читаются без прав).
pub fn read_proc_entries() -> Vec<ProcEntry> {
    let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
    dir.flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let pid: u32 = name.to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let (pid, ppid, state, cpu) = parse_proc_stat(&stat)?;
            let io = std::fs::read_to_string(format!("/proc/{pid}/io"))
                .map(|t| parse_proc_io(&t))
                .unwrap_or(0);
            Some(ProcEntry { pid, ppid, state, activity: cpu.saturating_add(io) })
        })
        .collect()
}

/// Признаки жизни процесса `pid`, запущенного вне службы. `None` — `/proc`
/// недоступен вовсе, и судить не по чему.
pub fn read_process_life(pid: u32) -> Option<EngineLife> {
    let entries = read_proc_entries();
    if entries.is_empty() || pid == 0 {
        return None;
    }
    Some(tree_life(pid, &entries))
}

/// argv опроса службы движка: состояние, главный PID и число перезапусков.
pub fn engine_unit_show_argv() -> Vec<String> {
    systemctl_user_argv(&[
        "show",
        "--property=ActiveState,MainPID,NRestarts",
        AUTOSTART_UNIT,
    ])
}

/// Состояние службы из вывода [`engine_unit_show_argv`]: `(жива, главный
/// PID, перезапуски)`. «Жива» — юнит запускается, работает или ждёт
/// перезапуска (`activating`), но не `failed`/`inactive`.
pub fn parse_engine_unit_show(stdout: &str) -> Option<(bool, u32, u32)> {
    let mut state = None;
    let mut pid = 0;
    let mut restarts = 0;
    for line in stdout.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        match key.trim() {
            "ActiveState" => state = Some(value.to_string()),
            "MainPID" => pid = value.parse().unwrap_or(0),
            "NRestarts" => restarts = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    let state = state?;
    let alive = matches!(state.as_str(), "activating" | "active" | "reloading");
    Some((alive, pid, restarts))
}

/// Признаки жизни движка под службой: состояние юнита плюс работа дерева
/// его главного процесса. `unit_show` — вывод [`engine_unit_show_argv`].
pub fn service_life(unit_show: &str, entries: &[ProcEntry]) -> Option<EngineLife> {
    let (alive, pid, restarts) = parse_engine_unit_show(unit_show)?;
    let activity = if pid != 0 { tree_life(pid, entries).activity } else { 0 };
    Some(EngineLife { alive, activity, restarts })
}

/// Порт `dbus_quick`: `str(args[0]) if args else ""`.
pub fn dbus_quick_result(arguments: &[DbusValue]) -> String {
    match arguments.first() {
        Some(v) => v.py_str(),
        None => String::new(),
    }
}

/// Запись `ListTasks` — порт кортежа
/// `(id, kind, name, started_at_ms, state)`; схема D-Bus `a(tssxs)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonTask {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub started_at_ms: i64,
    pub state: String,
}

/// Порт разбора демаршалённого QtDBus-ответа `ListTasks`.
///
/// `None` — строка не разобралась (не 5 элементов / нечисловой id), в Python
/// это `TypeError`/`ValueError` → фолбэк на `busctl` ([`busctl_tasks_argv`] +
/// [`parse_busctl_tasks`]).
pub fn tasks_from_dbus_rows(rows: &[Vec<DbusValue>]) -> Option<Vec<DaemonTask>> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if row.len() != 5 {
            return None;
        }
        out.push(DaemonTask {
            id: row[0].py_int()?,
            kind: row[1].py_str(),
            name: row[2].py_str(),
            started_at_ms: row[3].py_int()?,
            state: row[4].py_str(),
        });
    }
    Some(out)
}

/// argv фолбэка `busctl --json=short` (systemd есть на всех целевых Arch).
pub fn busctl_tasks_argv() -> Vec<String> {
    [
        "busctl",
        "--user",
        "--json=short",
        "call",
        DBUS_SERVICE,
        DBUS_PATH,
        DBUS_IFACE,
        "ListTasks",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Порт `_list_tasks_busctl`: разбор `json.loads(stdout)["data"][0]`.
/// Любая ошибка формата → пустой список (оригинал глушит всё через
/// `except Exception: return []`).
pub fn parse_busctl_tasks(stdout: &str) -> Vec<DaemonTask> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return Vec::new();
    };
    let Some(rows) = v.get("data").and_then(|d| d.get(0)).and_then(|d| d.as_array())
    else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(r) = r.as_array() else { return Vec::new() };
        if r.len() < 5 {
            return Vec::new();
        }
        let (Some(id), Some(started)) = (json_int(&r[0]), json_int(&r[3]))
        else {
            return Vec::new();
        };
        out.push(DaemonTask {
            id,
            kind: json_str(&r[1]),
            name: json_str(&r[2]),
            started_at_ms: started,
            state: json_str(&r[4]),
        });
    }
    out
}

/// `int(x)` над JSON-значением: число или числовая строка.
fn json_int(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        serde_json::Value::Bool(b) => Some(i64::from(*b)),
        serde_json::Value::String(s) => {
            DbusValue::Str(s.clone()).py_int()
        }
        _ => None,
    }
}

/// `str(x)` над JSON-значением (в ответе busctl это всегда строка).
fn json_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "None".to_string(),
        serde_json::Value::Bool(true) => "True".to_string(),
        serde_json::Value::Bool(false) => "False".to_string(),
        other => other.to_string(),
    }
}

// ── синхронная остановка активных WE-обоев (для установщика статичных) ──────

/// Итог `stop_active_wallpapers`: сняли `n` активных рендереров.
pub const STOP_STOPPED: &str = "stopped";
/// Демон waywallen не запущен → WE-обои нет, делать нечего.
pub const STOP_NOT_RUNNING: &str = "not_running";
/// Нет `QApplication` (headless/CLI) → синхронно снять нельзя.
pub const STOP_NO_QT: &str = "no_qt";
/// Демон есть, но снять не удалось (таймаут/обрыв WS).
pub const STOP_ERROR: &str = "error";

/// Таймаут всей операции по умолчанию, мс.
pub const STOP_TIMEOUT_MS: u64 = 4000;

/// Порт ранних веток `stop_active_wallpapers`: можно ли вообще работать.
/// `None` — можно, идём в WS-часть; `Some((код, 0))` — сразу этот результат.
pub fn stop_active_gate(
    status: &DaemonStatus,
    qt_available: bool,
) -> Option<(&'static str, i64)> {
    if !status.running || status.ws_port == 0 {
        return Some((STOP_NOT_RUNNING, 0));
    }
    if !qt_available {
        return Some((STOP_NO_QT, 0));
    }
    None
}

/// Порт финальной ветки `stop_active_wallpapers`.
pub fn stop_active_result(ok: bool, killed: i64) -> (&'static str, i64) {
    if ok { (STOP_STOPPED, killed) } else { (STOP_ERROR, 0) }
}

// ============================================================================
// 4. Кодек ЗАПРОСОВ
// ============================================================================

/// Выдача `request_id` — порт поля `_next_id` (нумерация с 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestIds {
    next: u64,
}

impl Default for RequestIds {
    fn default() -> Self {
        RequestIds { next: 1 }
    }
}

impl RequestIds {
    pub fn new() -> Self {
        Self::default()
    }

    /// Следующий `request_id` (и сдвиг счётчика).
    pub fn alloc(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }

    /// Текущее значение счётчика (не сдвигая) — для сверки с шимом.
    pub fn peek(&self) -> u64 {
        self.next
    }
}

// Таблица номеров полей `oneof payload` сообщения `Request` — нужна
// универсальному [`request_bytes`] (путь `call(field, msg)` из
// `qml_ui/bridge.py`, где payload приходит уже сериализованным).
//
// ⚠️ Файл ГЕНЕРИТСЯ build.rs из того же `control.proto`, что и структуры
// [`crate::proto`]. Рукописный список тут уже отставал от вендора: апстрим
// добавил поля 69–74 (`plugin_action`, `subscription_status`,
// `subscription_set`, `qr_login_cancel`, `remote_settings_patch`,
// `wallpaper_unsubscribe`), а таблица про них не знала — эти запросы молча
// уходили на Python-фолбэк. Теперь новое поле в `.proto` подхватывается само.
include!(concat!(env!("OUT_DIR"), "/request_payload_fields.rs"));

/// Номер поля `oneof payload` по его имени.
pub fn request_payload_field_number(field: &str) -> Option<u32> {
    REQUEST_PAYLOAD_FIELDS
        .iter()
        .find(|(name, _)| *name == field)
        .map(|(_, num)| *num)
}

/// Сборка `Request{request_id, <field>: msg}` из УЖЕ сериализованного payload
/// — универсальный путь `WaywallenClient.call(field, msg)`.
///
/// Байты собираются напрямую: `request_id` (поле 1, varint, при 0 опускается
/// как proto3-дефолт), затем length-delimited payload с номером поля из
/// [`REQUEST_PAYLOAD_FIELDS`]. Порядок восходящий — тот же, что у
/// `google.protobuf`.
pub fn request_bytes(
    request_id: u64,
    field: &str,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let num = request_payload_field_number(field)
        .ok_or_else(|| format!("неизвестное поле Request: {field}"))?;
    let mut out = Vec::with_capacity(payload.len() + 16);
    if request_id != 0 {
        out.push(0x08); // поле 1, varint
        put_varint(&mut out, request_id);
    }
    put_varint(&mut out, (u64::from(num) << 3) | 2);
    put_varint(&mut out, payload.len() as u64);
    out.extend_from_slice(payload);
    Ok(out)
}

/// Запись varint по протоколу protobuf.
fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Собрать `Request` из готовой полезной нагрузки prost-типа.
fn encode(request_id: u64, payload: pb::request::Payload) -> Vec<u8> {
    pb::Request { request_id, payload: Some(payload) }.encode_to_vec()
}

// ── обёртки, используемые страницами вкладки ────────────────────────────────

/// `health`.
pub fn health(request_id: u64) -> Vec<u8> {
    encode(request_id, pb::request::Payload::Health(pb::HealthRequest {}))
}

/// Параметры `wallpaper_list` — быстрые фильтры диалога Filters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallpaperListParams {
    pub search_text: String,
    /// `filter.proto`: `WallpaperSortKey`; по умолчанию
    /// `WALLPAPER_SORT_KEY_NAME` (1).
    pub sort_key: i32,
    pub sort_desc: bool,
    /// Исключить типы.
    pub skip_types: Vec<String>,
    /// Только обои с любым из тегов (пусто = все).
    pub filter_tags: Vec<String>,
    /// Исключить рейтинги.
    pub skip_content_ratings: Vec<String>,
}

impl Default for WallpaperListParams {
    fn default() -> Self {
        WallpaperListParams {
            search_text: String::new(),
            sort_key: pb::WallpaperSortKey::Name as i32,
            sort_desc: false,
            skip_types: Vec::new(),
            filter_tags: Vec::new(),
            skip_content_ratings: Vec::new(),
        }
    }
}

/// `wallpaper_list`. `page_size = 0` — все записи, без пагинации.
pub fn wallpaper_list(request_id: u64, p: &WallpaperListParams) -> Vec<u8> {
    let direction = if p.sort_desc {
        pb::SortDirection::Desc
    } else {
        pb::SortDirection::Asc
    } as i32;
    let req = pb::WallpaperListRequest {
        page_size: 0,
        search_text: p.search_text.clone(),
        skip_types: p.skip_types.clone(),
        filter_tags: p.filter_tags.clone(),
        skip_content_ratings: p.skip_content_ratings.clone(),
        sorts: vec![pb::WallpaperSortRule { key: p.sort_key, direction }],
        ..Default::default()
    };
    encode(request_id, pb::request::Payload::WallpaperList(req))
}

/// `display_list`.
pub fn display_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::DisplayList(pb::DisplayListRequest {}),
    )
}

/// Цели применения из номеров дисплеев — `PresentationTarget` v0.3.5.
///
/// С v0.3.5 поле 5 `WallpaperApplyRequest` (и поля 2/1 у активации и
/// деактивации плейлиста) сменило тип: было `repeated uint64 display_ids`,
/// стало `repeated PresentationTarget targets`. НОМЕР ПОЛЯ ТОТ ЖЕ, а тип на
/// проводе другой (упакованные varint'ы против вложенных сообщений), поэтому
/// кадры двух версий несовместимы в обе стороны и демон роняет чужой кадр
/// молча. Отсюда единственная точка сборки целей — здесь, чтобы номера
/// дисплеев не превращались в кадр где-то ещё по старой форме.
///
/// Нули отбрасываются: у вендора id дисплея — ненулевой ключ, а `0` приходит
/// от QML как «значения нет» (`presentationTargetsFromVariant`).
pub fn display_targets(display_ids: &[u64]) -> Vec<pb::PresentationTarget> {
    display_ids
        .iter()
        .filter(|id| **id != 0)
        .map(|id| pb::PresentationTarget {
            target: Some(pb::presentation_target::Target::DisplayId(*id)),
        })
        .collect()
}

/// Цель-холст (`PresentationTarget.canvas_id`). Пустой id — не цель.
pub fn canvas_target(canvas_id: &str) -> Option<pb::PresentationTarget> {
    if canvas_id.is_empty() {
        return None;
    }
    Some(pb::PresentationTarget {
        target: Some(pb::presentation_target::Target::CanvasId(
            canvas_id.to_string(),
        )),
    })
}

/// `wallpaper_apply`.
pub fn wallpaper_apply(
    request_id: u64,
    wallpaper_id: &str,
    targets: &[pb::PresentationTarget],
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::WallpaperApply(pb::WallpaperApplyRequest {
            wallpaper_id: wallpaper_id.to_string(),
            targets: targets.to_vec(),
            ..Default::default()
        }),
    )
}

/// `wallpaper_scan`.
pub fn wallpaper_scan(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::WallpaperScan(pb::WallpaperScanRequest {}),
    )
}

/// `wallpaper_get` — свежий снимок одной обоины (только здесь приходит
/// `user_properties_schema` + `user_property_overrides`).
pub fn wallpaper_get(request_id: u64, wallpaper_id: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::WallpaperGet(pb::WallpaperGetRequest {
            wallpaper_id: wallpaper_id.to_string(),
        }),
    )
}

/// `wallpaper_property_set` — одно user-property override. Пустое `value` =
/// сброс к дефолту `project.json` (у демона это валидное значение oneof).
pub fn wallpaper_property_set(
    request_id: u64,
    wallpaper_id: &str,
    key: &str,
    value: &str,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::WallpaperPropertySet(
            pb::WallpaperPropertySetRequest {
                wallpaper_id: wallpaper_id.to_string(),
                key: key.to_string(),
                operation: Some(
                    pb::wallpaper_property_set_request::Operation::Value(
                        value.to_string(),
                    ),
                ),
            },
        ),
    )
}

// ── плейлисты ───────────────────────────────────────────────────────────────

/// `playlist_list`.
pub fn playlist_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistList(pb::PlaylistListRequest {}),
    )
}

/// `playlist_status`.
pub fn playlist_status(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistStatus(pb::PlaylistStatusRequest {}),
    )
}

/// `playlist_create`.
pub fn playlist_create(
    request_id: u64,
    name: &str,
    mode: i32,
    interval_secs: u32,
    entry_ids: &[String],
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistCreate(pb::PlaylistCreateRequest {
            name: name.to_string(),
            mode,
            interval_secs,
            entry_ids: entry_ids.to_vec(),
            // Поле появилось в протоколе 0.3.7 (общий выбор обоев у
            // плейлиста). `false` — ровно нынешнее поведение: в proto3
            // нулевой bool на провод не пишется, кадр остаётся тем же
            // байт-в-байт. Значение придёт из формы, когда на неё
            // переедет вендорный QML 0.3.7.
            synchronized_selection: false,
            // waywallen 0.5.0: источник плейлиста (курируемый список или
            // умная выборка). `None` демон читает как курируемый из
            // `entry_ids` — ровно прежнее поведение; умные плейлисты Rexon
            // пока не создаёт.
            source: None,
        }),
    )
}

/// `playlist_delete`.
pub fn playlist_delete(request_id: u64, playlist_id: i64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistDelete(pb::PlaylistDeleteRequest {
            id: playlist_id,
        }),
    )
}

/// `playlist_rename`.
pub fn playlist_rename(request_id: u64, playlist_id: i64, name: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistRename(pb::PlaylistRenameRequest {
            id: playlist_id,
            name: name.to_string(),
        }),
    )
}

/// `playlist_set_items`.
pub fn playlist_set_items(
    request_id: u64,
    playlist_id: i64,
    entry_ids: &[String],
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistSetItems(pb::PlaylistSetItemsRequest {
            id: playlist_id,
            entry_ids: entry_ids.to_vec(),
            // Сторож гонки правок из протокола 0.3.7. Ноль = «не сверять»,
            // то есть нынешнее поведение; на проводе нулевое поле не
            // появляется вовсе.
            expected_revision: 0,
        }),
    )
}

/// `playlist_set_mode`.
pub fn playlist_set_mode(request_id: u64, playlist_id: i64, mode: i32) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistSetMode(pb::PlaylistSetModeRequest {
            id: playlist_id,
            mode,
        }),
    )
}

/// `playlist_set_interval`.
pub fn playlist_set_interval(
    request_id: u64,
    playlist_id: i64,
    interval_secs: u32,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistSetInterval(
            pb::PlaylistSetIntervalRequest { id: playlist_id, interval_secs },
        ),
    )
}

/// `playlist_activate`.
pub fn playlist_activate(
    request_id: u64,
    playlist_id: i64,
    targets: &[pb::PresentationTarget],
    auto_attach: bool,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistActivate(pb::PlaylistActivateRequest {
            id: playlist_id,
            targets: targets.to_vec(),
            auto_attach,
        }),
    )
}

/// `playlist_deactivate`.
pub fn playlist_deactivate(
    request_id: u64,
    targets: &[pb::PresentationTarget],
    clear_auto_attach: i64,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistDeactivate(
            pb::PlaylistDeactivateRequest {
                targets: targets.to_vec(),
                clear_auto_attach,
            },
        ),
    )
}

/// `playlist_jump_to`.
pub fn playlist_jump_to(
    request_id: u64,
    playlist_id: i64,
    entry_id: &str,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PlaylistJumpTo(pb::PlaylistJumpToRequest {
            id: playlist_id,
            entry_id: entry_id.to_string(),
        }),
    )
}

// ⚠️ playlist_export / playlist_import УДАЛЕНЫ демоном в v0.2.3 (поля 55/56
// Request — reserved). Обёрток нет намеренно.

// ── дисплеи ─────────────────────────────────────────────────────────────────

/// `display_rename`.
///
/// ⚠️ `display_id` обязателен ровно по той же причине, что и у
/// `display_layout_set` ниже: с v0.2.4 демон резолвит цель по номеру, а
/// `name` держит только для старых клиентов. Прежняя обёртка номер не
/// слала вовсе (`..Default::default()`), и переименование ушло бы в
/// никуда — починено при переносе запроса на Rust (правило #16).
pub fn display_rename(
    request_id: u64,
    name: &str,
    display_id: u64,
    alias: &str,
    clear: bool,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::DisplayRename(pb::DisplayRenameRequest {
            name: name.to_string(),
            display_id,
            alias: alias.to_string(),
            clear,
        }),
    )
}

/// Sparse-override per-display layout — параметры `display_layout_set`.
///
/// С v0.2.4 таргет резолвится по `display_id` («name is retained for older
/// clients but is not used for target resolution», фикс #79) — передавать
/// ОБЯЗАТЕЛЬНО; `name` шлём тоже, для старых демонов.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DisplayLayoutParams {
    pub name: String,
    pub display_id: u64,
    pub fillmode: Option<i32>,
    pub align: Option<i32>,
    pub rotation: Option<i32>,
    /// `(x, y)` в процентах 0..100, 50 = центр.
    pub location: Option<(u32, u32)>,
    pub clear_fillmode: bool,
    pub clear_align: bool,
    pub clear_rotation: bool,
    pub clear_location: bool,
}

/// `display_layout_set`. Подсообщение `override` появляется в кадре, только
/// если задано хоть одно из `fillmode`/`align`/`rotation`/`location` (в Python
/// оно создаётся самим обращением `req.override.*`).
pub fn display_layout_set(request_id: u64, p: &DisplayLayoutParams) -> Vec<u8> {
    let touched = p.fillmode.is_some()
        || p.align.is_some()
        || p.rotation.is_some()
        || p.location.is_some();
    let over = touched.then(|| {
        let mut o = pb::LayoutOverride::default();
        if let Some(v) = p.fillmode {
            o.fillmode_set = true;
            o.fillmode = v;
        }
        if let Some(v) = p.align {
            o.align_set = true;
            o.align = v;
        }
        if let Some(v) = p.rotation {
            o.rotation_set = true;
            o.rotation = v;
        }
        if let Some((x, y)) = p.location {
            o.location_set = true;
            o.location_x = x;
            o.location_y = y;
        }
        o
    });
    encode(
        request_id,
        pb::request::Payload::DisplayLayoutSet(pb::DisplayLayoutSetRequest {
            name: p.name.clone(),
            r#override: over,
            clear_fillmode: p.clear_fillmode,
            clear_align: p.clear_align,
            clear_rotation: p.clear_rotation,
            display_id: p.display_id,
            clear_location: p.clear_location,
            // waywallen 0.4.4: отражение (`flip`). Органа управления у нас
            // нет: `flip_set` в оверрайде ложен, сброса не просим — демон
            // оставляет то отражение, что уже стоит у дисплея.
            clear_flip: false,
        }),
    )
}

// ── статус ──────────────────────────────────────────────────────────────────

/// `renderer_list`.
pub fn renderer_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RendererList(pb::RendererListRequest {}),
    )
}

/// `renderer_plugin_list`.
pub fn renderer_plugin_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RendererPluginList(
            pb::RendererPluginListRequest {},
        ),
    )
}

/// `renderer_kill`.
pub fn renderer_kill(request_id: u64, renderer_id: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RendererKill(pb::RendererKillRequest {
            renderer_id: renderer_id.to_string(),
        }),
    )
}

/// `gpu_list`.
pub fn gpu_list(request_id: u64) -> Vec<u8> {
    encode(request_id, pb::request::Payload::GpuList(pb::GpuListRequest {}))
}

/// `wallpaper_lookup` — обоины по известным идентификаторам (карточка
/// «Текущие обои»). Демон отвечает из своей базы, источники не опрашивает.
pub fn wallpaper_lookup(request_id: u64, wallpaper_ids: &[String]) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::WallpaperLookup(pb::WallpaperLookupRequest {
            wallpaper_ids: wallpaper_ids.to_vec(),
        }),
    )
}

// ── фильтры (справочники тегов/рейтингов) ───────────────────────────────────

/// `tag_list`.
pub fn tag_list(request_id: u64) -> Vec<u8> {
    encode(request_id, pb::request::Payload::TagList(pb::TagListRequest {}))
}

/// `content_rating_list`.
pub fn content_rating_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::ContentRatingList(pb::ContentRatingListRequest {}),
    )
}

// ── источники/библиотеки ────────────────────────────────────────────────────

/// `source_list`.
pub fn source_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::SourceList(pb::SourceListRequest {}),
    )
}

/// `library_list`.
pub fn library_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::LibraryList(pb::LibraryListRequest {}),
    )
}

/// Имя библиотеки так, как его шлёт вендорный интерфейс waywallen 0.5.0:
/// пробелы по краям срезаны, пустое не шлётся вовсе (`None` — имя по
/// умолчанию выбирает демон). Одно правило на добавление и переименование.
fn library_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Тело `library_add` — или `None`, если запрос не должен уйти: без пути
/// или без плагина оригинал молча выходит. Имя — по [`library_name`]
/// (waywallen 0.5.0, необязательное поле `LibraryAddRequest.name`).
pub fn library_add_request(
    path: &str,
    plugin_name: &str,
    name: &str,
) -> Option<pb::LibraryAddRequest> {
    if path.is_empty() || plugin_name.is_empty() {
        return None;
    }
    Some(pb::LibraryAddRequest {
        path: path.to_string(),
        plugin_name: plugin_name.to_string(),
        name: library_name(name),
    })
}

/// `library_add`. Пустое `name` — имя по умолчанию от демона, как до
/// появления поля в 0.5.0.
pub fn library_add(request_id: u64, path: &str, plugin_name: &str, name: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::LibraryAdd(pb::LibraryAddRequest {
            path: path.to_string(),
            plugin_name: plugin_name.to_string(),
            name: library_name(name),
        }),
    )
}

/// Тело `library_rename` (waywallen 0.5.0) — или `None`, если запрос не
/// должен уйти: номер библиотеки не положительный или имя после среза
/// пробелов пусто. Так же молча выходит оригинал; ответ демона — `Empty`.
pub fn library_rename_request(library_id: i64, name: &str) -> Option<pb::LibraryRenameRequest> {
    if library_id <= 0 {
        return None;
    }
    Some(pb::LibraryRenameRequest {
        id: library_id,
        name: library_name(name)?,
    })
}

/// `library_rename` — новое имя библиотеки (пробелы по краям срезаны).
pub fn library_rename(request_id: u64, library_id: i64, name: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::LibraryRename(pb::LibraryRenameRequest {
            id: library_id,
            name: name.trim().to_string(),
        }),
    )
}

/// `library_remove`.
pub fn library_remove(request_id: u64, library_id: i64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::LibraryRemove(pb::LibraryRemoveRequest {
            id: library_id,
        }),
    )
}

/// `library_auto_detect` — источник-плагины сами ищут библиотеки в типовых
/// местах (Pictures, Steam workshop) и регистрируют найденное.
pub fn library_auto_detect(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::LibraryAutoDetect(pb::LibraryAutoDetectRequest {}),
    )
}

// ── настройки демона ────────────────────────────────────────────────────────

/// `settings_get`.
pub fn settings_get(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::SettingsGet(pb::SettingsGetRequest {}),
    )
}

/// `settings_set`.
///
/// ⚠️ Full-replace: демон перезаписывает `global` И карту плагинов тем, что
/// пришло — плагины, не попавшие в запрос, УДАЛЯЮТСЯ. Всегда передавать
/// `plugins` из последнего `settings_get`.
///
/// Подсообщения приходят уже сериализованными (их формируют страницы
/// настроек на Python) и здесь декодируются в prost-типы.
///
/// ⚠️ Байтовая сверка с Python для этого запроса НЕ применима: порядок
/// `map`-записей на проводе не определён ни в одной из реализаций.
pub fn settings_set(
    request_id: u64,
    global_settings: &[u8],
    plugins: &[(String, Vec<u8>)],
) -> Result<Vec<u8>, String> {
    let global = pb::GlobalSettings::decode(global_settings)
        .map_err(|e| format!("settings_set: global не разобран: {e}"))?;
    let mut map = std::collections::HashMap::with_capacity(plugins.len());
    for (name, raw) in plugins {
        let ps = pb::PluginSettings::decode(&raw[..]).map_err(|e| {
            format!("settings_set: плагин {name} не разобран: {e}")
        })?;
        map.insert(name.clone(), ps);
    }
    Ok(encode(
        request_id,
        pb::request::Payload::SettingsSet(pb::SettingsSetRequest {
            global: Some(global),
            plugins: map,
        }),
    ))
}

// ── installable-плагины ─────────────────────────────────────────────────────

/// `plugin_list`.
pub fn plugin_list(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PluginList(pb::PluginListRequest {}),
    )
}

/// `plugin_install` — установка плагина из ЛОКАЛЬНОГО `.zip`
/// (`$XDG_DATA_HOME/waywallen/plugins/`, демон ничего не качает).
pub fn plugin_install(request_id: u64, zip_path: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::PluginInstall(pb::PluginInstallRequest {
            zip_path: zip_path.to_string(),
        }),
    )
}

// ── Workshop (страница «Discover») ──────────────────────────────────────────

/// `remote_availability`.
pub fn remote_availability(request_id: u64) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RemoteAvailability(pb::RemoteAvailabilityRequest {}),
    )
}

/// `remote_search`. v0.2.3+: источник задаётся `source_id` (из
/// `RemoteAvailability.sources`), `sort_key` — строковый ключ из
/// `RemoteSourceInfo.sorts`.
pub fn remote_search(
    request_id: u64,
    query: &str,
    sort_key: &str,
    page: u32,
    source_id: &str,
    tags: &[String],
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RemoteSearch(pb::RemoteSearchRequest {
            source_id: source_id.to_string(),
            query: query.to_string(),
            sort_key: sort_key.to_string(),
            page,
            required_tags: tags.to_vec(),
        }),
    )
}

/// `remote_details`.
pub fn remote_details(request_id: u64, item_id: &str, source_id: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RemoteDetails(pb::RemoteDetailsRequest {
            source_id: source_id.to_string(),
            id: item_id.to_string(),
        }),
    )
}

/// `remote_download`.
pub fn remote_download(request_id: u64, item_id: &str, source_id: &str) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RemoteDownload(pb::RemoteDownloadRequest {
            source_id: source_id.to_string(),
            id: item_id.to_string(),
        }),
    )
}

/// `remote_uninstall`.
pub fn remote_uninstall(
    request_id: u64,
    item_id: &str,
    source_id: &str,
) -> Vec<u8> {
    encode(
        request_id,
        pb::request::Payload::RemoteUninstall(pb::RemoteUninstallRequest {
            source_id: source_id.to_string(),
            id: item_id.to_string(),
        }),
    )
}

// ============================================================================
// Разбор ОТВЕТОВ, нужных самому модулю
// ============================================================================
//
// Разбор ответов вкладки живёт в [`crate::we_query`] (там же — состояние
// `Query` и сигналы Qt). Здесь только то, что читает СИНХРОННЫЙ путь
// `client.py`, у которого никакого `Query` нет: `stop_active_wallpapers`
// снимает активные рендереры перед установкой статичных обоев.

/// Идентификаторы активных рендереров из кадра ответа `renderer_list`.
///
/// Вход — СЫРЫЕ байты `ServerFrame` (как их отдаёт WebSocket). `None` —
/// кадр не разобрался или это не ответ `renderer_list`: вызывающий тогда
/// считает, что рендереров нет (порт `except → ids = []` оригинала, где
/// «не смогли прочитать» и «нечего снимать» ведут себя одинаково).
///
/// Пустые `renderer_id` отбрасываются: убивать рендерер без адреса нечем, а
/// в счётчик снятых он бы попал и соврал бы пользователю про результат.
pub fn renderer_ids_from_frame(frame_bytes: &[u8]) -> Option<Vec<String>> {
    let frame = pb::ServerFrame::decode(frame_bytes).ok()?;
    let pb::server_frame::Kind::Response(rsp) = frame.kind? else {
        return None;
    };
    let pb::response::Payload::RendererList(rl) = rsp.payload? else {
        return None;
    };
    Some(
        rl.instances
            .into_iter()
            .map(|i| i.renderer_id)
            .filter(|id| !id.is_empty())
            .collect(),
    )
}

// ============================================================================
// Кадр сервера для колбэков на Python
// ============================================================================
//
// Питоновские колбэки вкладки читали поля `pb.Response` САМИ — ради этого в
// поставке покупателя лежал сгенерированный `control_pb2.py` и зависимость
// `python-protobuf`. Разбор у всех трёх колбэков один и тот же, поэтому он
// здесь ОДИН (правило #12), а не по копии на каждый.
//
// ⚠️ Нагрузки, которые Python НЕ читает, сюда не расписаны намеренно: их
// разбирает ядро на своей стороне (`crate::we_query`), и вторая копия разбора
// рядом старела бы молча. Появился новый потребитель на Python — добавьте ему
// ветку в `payload_fields`, а не свой разбор по месту.

/// Имя нагрузки, для которой разбора здесь нет.
///
/// Не пустая строка и не «ok»: колбэк, увидевший это имя, обязан понять, что
/// поля ему никто не разобрал, а не принять пустоту за ответ демона.
pub const REPLY_PAYLOAD_UNPARSED: &str = "unparsed";

/// Кадр `ServerFrame`, разобранный для кода на Python.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ReplyFields {
    /// `"response"`, `"event"` либо `""` — кадр без содержимого.
    pub kind: String,
    pub request_id: u64,
    pub status: i32,
    pub error_code: i32,
    pub message: String,
    /// Имя поля `oneof payload` — `""` (нагрузки нет) либо
    /// [`REPLY_PAYLOAD_UNPARSED`], если разбора для неё здесь не заведено.
    pub payload: String,
    /// Строковые поля нагрузки.
    pub text: Vec<(String, String)>,
    /// Логические поля нагрузки.
    pub flags: Vec<(String, bool)>,
}

/// Поля нагрузки ответа, которые читают колбэки на Python.
fn payload_fields(payload: Option<pb::response::Payload>) -> (String, Vec<(String, String)>, Vec<(String, bool)>) {
    match payload {
        None => (String::new(), Vec::new(), Vec::new()),
        // Окно вкладки: здоров ли движок после подключения сокета.
        Some(pb::response::Payload::Health(h)) => (
            "health".to_owned(),
            vec![
                ("service".to_owned(), h.service),
                ("state".to_owned(), h.state),
                ("os_name".to_owned(), h.os_name),
            ],
            Vec::new(),
        ),
        // Страница «Разработчик»: горячая установка плагина движка.
        Some(pb::response::Payload::PluginInstall(p)) => (
            "plugin_install".to_owned(),
            vec![("plugin_id".to_owned(), p.plugin_id)],
            vec![("needs_restart".to_owned(), p.needs_restart)],
        ),
        Some(_) => (REPLY_PAYLOAD_UNPARSED.to_owned(), Vec::new(), Vec::new()),
    }
}

/// Разобрать СЫРЫЕ байты `ServerFrame` для колбэка на Python.
///
/// Ошибка разбора возвращается вызывающему: битый кадр обязан дать понятную
/// ошибку и потеряться, а не быть принятым за пустой ответ (демон может быть
/// новее нас).
pub fn reply_fields(frame_bytes: &[u8]) -> Result<ReplyFields, prost::DecodeError> {
    let frame = pb::ServerFrame::decode(frame_bytes)?;
    match frame.kind {
        Some(pb::server_frame::Kind::Response(rsp)) => {
            let (payload, text, flags) = payload_fields(rsp.payload);
            Ok(ReplyFields {
                kind: "response".to_owned(),
                request_id: rsp.request_id,
                status: rsp.status,
                error_code: rsp.error_code,
                message: rsp.message,
                payload,
                text,
                flags,
            })
        }
        Some(pb::server_frame::Kind::Event(_)) => Ok(ReplyFields {
            kind: "event".to_owned(),
            ..Default::default()
        }),
        None => Ok(ReplyFields::default()),
    }
}

#[cfg(test)]
mod start_watch_tests {
    use super::*;

    #[test]
    fn exit_report_tells_causes_apart() {
        assert_eq!(daemon_output_log_path("/s/rexon/"), "/s/rexon/waywallen-daemon.log");
        assert_eq!(text_tail("a\n\nb  \nc\n", 2), ["b", "c"]);
        assert_eq!(text_tail("", 5), Vec::<String>::new());
        assert_eq!(parse_name_owner_pid("u 4242\n"), Some(4242));
        assert_eq!(parse_name_owner_pid("u 0"), None);
        assert_eq!(parse_name_owner_pid("Call failed: name has no owner"), None);
        assert_eq!(daemon_name_owner_pid_argv().last().map(String::as_str), Some(crate::waywallen_client::BUS_NAME));

        // Второй экземпляр: код 0, имя держит другой процесс.
        let owner = BusNameOwner::Process { pid: 77, cmdline: "/usr/bin/waywallen --no-ui".into() };
        let tail = vec!["waywallen: already running, no UI to launch".to_string()];
        let r = daemon_exit_report(55, EngineExit::Code(0), Some(&owner), "/s/w.log", &tail);
        assert_eq!(r[0], "движок pid 55: код выхода 0");
        assert!(r[1].contains("pid 77") && r[1].contains("уступил"), "{r:?}");
        assert_eq!(r[3], "  | waywallen: already running, no UI to launch");

        // Свой отказ: код 1, имя свободно, ошибка в хвосте.
        let r = daemon_exit_report(55, EngineExit::Code(1), Some(&BusNameOwner::Free), "/s/w.log", &[]);
        assert_eq!(r, ["движок pid 55: код выхода 1", "имя шины демона свободно", "вывод демона пуст (/s/w.log)"]);
        let r = daemon_exit_report(55, EngineExit::Signal(9), None, "/s/w.log", &[]);
        assert!(r[0].ends_with("убит сигналом 9") && r[1].contains("не удалось"));
        // Своё имя при отказе по сроку — без догадки о втором экземпляре.
        let me = BusNameOwner::Process { pid: 55, cmdline: String::new() };
        let r = daemon_exit_report(55, EngineExit::Unknown, Some(&me), "/s/w.log", &[]);
        assert_eq!(r[1], "имя шины демона держит pid 55 (?)");
    }

    fn dead_bus() -> DaemonStatus {
        DaemonStatus::default()
    }

    fn life(alive: bool, activity: u64) -> Option<EngineLife> {
        Some(EngineLife { alive, activity, restarts: 0 })
    }

    /// Дефект 30.10 (r1895): живой, работающий процесс через 12 с — не
    /// отказ, а «ждём, первый запуск бывает долгим».
    #[test]
    fn working_process_is_waited_past_the_old_12_seconds() {
        let mut w = StartWatch::new(true);
        let mut activity = 0;
        let mut last = StartStep::Wait { slow: false };
        for _ in 0..(60_000 / POLL_MS) {
            activity += 7;
            last = w.tick(&dead_bus(), life(true, activity), POLL_MS);
            assert!(!matches!(last, StartStep::Failed { .. }), "живой процесс сдан: {last:?}");
        }
        assert_eq!(last, StartStep::Wait { slow: true });
        let live = DaemonStatus { running: true, ws_port: 5, ..Default::default() };
        assert_eq!(w.tick(&live, life(true, activity), POLL_MS), StartStep::Ready { status: live });
    }

    #[test]
    fn first_ticks_are_quiet_not_slow() {
        let mut w = StartWatch::new(true);
        assert_eq!(w.tick(&dead_bus(), life(true, 1), POLL_MS), StartStep::Wait { slow: false });
    }

    /// Умер — отказ сразу, ждать 12 с нечего.
    #[test]
    fn dead_process_fails_at_once() {
        let mut w = StartWatch::new(true);
        assert_eq!(
            w.tick(&dead_bus(), life(false, 0), POLL_MS),
            StartStep::Failed { message: daemon_exited_message() }
        );
    }

    /// Служба ещё не перешла в «запускается» — это не смерть, пока не
    /// истекло тихое окно.
    #[test]
    fn service_not_yet_started_is_not_dead_until_quiet_window_ends() {
        let mut w = StartWatch::new(false);
        let mut n = 0;
        loop {
            n += 1;
            match w.tick(&dead_bus(), life(false, 0), POLL_MS) {
                StartStep::Wait { .. } => {}
                StartStep::Failed { message } => {
                    assert_eq!(message, daemon_exited_message());
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(n as u64 * POLL_MS, START_QUIET_MS);
    }

    #[test]
    fn service_restart_means_crash() {
        let mut w = StartWatch::new(false);
        let at = |r| Some(EngineLife { alive: true, activity: r as u64, restarts: r });
        assert!(matches!(w.tick(&dead_bus(), at(0), POLL_MS), StartStep::Wait { .. }));
        assert_eq!(
            w.tick(&dead_bus(), at(1), POLL_MS),
            StartStep::Failed { message: daemon_exited_message() }
        );
    }

    /// Живой, но затихший процесс — завис: отказ после тихого окна и срока
    /// затишья, не раньше.
    #[test]
    fn silent_process_fails_after_stall() {
        let mut w = StartWatch::new(true);
        let mut t = 0;
        let message = loop {
            t += POLL_MS;
            if let StartStep::Failed { message } = w.tick(&dead_bus(), life(true, 42), POLL_MS) {
                break message;
            }
        };
        assert!(t >= START_QUIET_MS && t >= START_STALL_MS, "сдался на {t} мс");
        assert!(t < START_QUIET_MS + START_STALL_MS);
        assert!(message.starts_with(START_TIMEOUT_HEAD), "{message}");
    }

    #[test]
    fn unknown_life_fails_after_stall_from_start() {
        let mut w = StartWatch::new(true);
        let mut t = 0;
        while !matches!(w.tick(&dead_bus(), None, POLL_MS), StartStep::Failed { .. }) {
            t += POLL_MS;
        }
        assert_eq!(t + POLL_MS, START_STALL_MS.max(START_QUIET_MS));
    }

    /// Работает без конца, а шины нет — предельный срок.
    #[test]
    fn busy_forever_hits_hard_limit() {
        let mut w = StartWatch::new(true);
        let mut activity = 0;
        let mut t = 0;
        let message = loop {
            activity += 1;
            t += POLL_MS;
            if let StartStep::Failed { message } = w.tick(&dead_bus(), life(true, activity), POLL_MS) {
                break message;
            }
        };
        assert_eq!(t, START_HARD_LIMIT_MS);
        assert!(message.starts_with(START_TIMEOUT_HEAD));
    }

    #[test]
    fn proc_stat_parsing_survives_odd_names() {
        let line = "1234 (way wal) (len)) S 77 1 1 0 -1 4194560 100 0 0 0 150 25 0 0 20 0 4 0 999 1 2";
        assert_eq!(parse_proc_stat(line), Some((1234, 77, 'S', 175)));
        assert_eq!(parse_proc_stat("garbage"), None);
    }

    #[test]
    fn proc_io_sums_read_and_write_chars() {
        let io = "rchar: 1000\nwchar: 24\nsyscr: 5\nread_bytes: 4096\n";
        assert_eq!(parse_proc_io(io), 1024);
        assert_eq!(parse_proc_io(""), 0);
    }

    #[test]
    fn tree_life_counts_descendants_and_ignores_zombies() {
        let e = |pid, ppid, state, activity| ProcEntry { pid, ppid, state, activity };
        let entries = [e(10, 1, 'S', 5), e(11, 10, 'R', 7), e(12, 11, 'S', 1), e(20, 1, 'R', 100)];
        assert_eq!(tree_life(10, &entries), EngineLife { alive: true, activity: 13, restarts: 0 });
        assert!(!tree_life(30, &entries).alive);
        assert!(!tree_life(10, &[e(10, 1, 'Z', 5)]).alive);
    }

    #[test]
    fn unit_show_parsing() {
        let out = "ActiveState=activating\nMainPID=4321\nNRestarts=2\n";
        assert_eq!(parse_engine_unit_show(out), Some((true, 4321, 2)));
        assert_eq!(parse_engine_unit_show("ActiveState=failed\nMainPID=0\nNRestarts=5"), Some((false, 0, 5)));
        assert_eq!(parse_engine_unit_show(""), None);
        let entries = [ProcEntry { pid: 4321, ppid: 1, state: 'S', activity: 9 }];
        assert_eq!(
            service_life(out, &entries),
            Some(EngineLife { alive: true, activity: 9, restarts: 2 })
        );
        assert_eq!(engine_unit_show_argv().last().map(String::as_str), Some(AUTOSTART_UNIT));
    }
}
