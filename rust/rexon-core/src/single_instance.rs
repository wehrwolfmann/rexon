//! Один Rexon на сессию: замок второго запуска и слушатель «подними окно».
//!
//! ЗАЧЕМ ОТДЕЛЬНЫЙ МОДУЛЬ. До этой правки механизм был разорван на три
//! конца, и ни один не был в ядре целиком:
//!   * адреса замка и сокета лежали в `app.rs` (чистые функции);
//!   * сам `flock` — в Python-биндинге `rust/rexon-py/src/lib.rs`, то есть
//!     Rust-точке входа продукта он был недоступен в принципе: биндинг
//!     существует только внутри процесса Python;
//!   * слушатель «подними окно» (`QLocalServer`) и стук в него — в Python
//!     (`rexon/qt_main/app.py::_serve_activation`, `rexon/app.py::
//!     _activate_running_instance`).
//! Пока замок живёт в биндинге, Rust-бинарь не может стать точкой входа
//! продукта: он либо поднимет второе окно рядом с работающим Rexon, либо
//! обзаведётся ВТОРОЙ копией замка — а два замка на один продукт означают
//! два разных ответа на один вопрос «я первый?» (правило #12).
//!
//! Здесь весь механизм собран в одном месте: адреса, замок, слушатель и
//! стук. Биндинг и обе точки входа (Python и Rust) зовут ядро.
//!
//! ── ИСТОРИЯ ДЕФЕКТОВ, КОТОРЫЕ ЭТИМ ЗАКРЫТЫ ─────────────────────────────
//!
//! 2026-09-02, «пять кактусов в трее». У владельца в системном лотке KDE
//! висело пять-шесть одинаковых значков Rexon. `qdbus6` +
//! `GetConnectionUnixProcessID` + `/proc/<pid>/cmdline` показали пять ЖИВЫХ
//! процессов `rexon` (не мёртвых записей StatusNotifierWatcher), запущенных
//! в разное время за трое суток: часть ярлыком лаунчера, часть вручную из
//! каталога разработки. Ни один запуск не проверял, что он не первый.
//!
//! 2026-09-06, «программа работает, двери в неё нет». Замка ОДНОГО МАЛО.
//! Сам по себе он ведёт себя правильно и молча уходит — но покупателю от
//! этого ничего не видно: предупреждение уходит в консоль, которой у ярлыка
//! нет. Окно первого Rexon было спрятано в лоток крестиком (при живом
//! значке крестик прячет, а не гасит), человек дважды нажал ярлык, оба
//! запуска уткнулись в замок, и на экране не произошло РОВНО НИЧЕГО.
//! Правильный ответ единственного экземпляра на второй запуск — не «уйти»,
//! а ПОДНЯТЬ окно того, кто уже работает.
//!
//! 2026-09-15, «дверь не помещается в адрес». `sun_path` вмещает 107 байт,
//! и при длинном `XDG_RUNTIME_DIR` слушатель не вставал вовсе, а второй
//! запуск говорил «уже запущен — второй значок не создаю» вместо поднятого
//! окна. Теперь адрес решает одна функция [`socket_path`] ДО `bind` и
//! `connect` (запасной короткий каталог `/tmp/rexon-<uid>`), обе стороны
//! сверяют uid собеседника, а человеку при молчащем слушателе ядро говорит
//! правду ([`said_not_responding`]).

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::i18n::{tr, trf};

// ============================================================================
// Адреса: каталог, замок, сокет
// ============================================================================

/// Каталог замка и сокета: `$XDG_RUNTIME_DIR`, а при его отсутствии `/tmp`.
///
/// Порт выражения `os.environ.get("XDG_RUNTIME_DIR") or "/tmp"`, которое в
/// Python повторялось в двух концах механизма. Пустая строка в переменной
/// равна её отсутствию — ровно как `or` у Python, где `""` ложна.
pub fn runtime_dir(xdg_runtime_dir: Option<&str>) -> String {
    match xdg_runtime_dir {
        Some(dir) if !dir.is_empty() => dir.to_string(),
        _ => "/tmp".to_string(),
    }
}

/// То же по живому окружению — единственное место, где читается переменная.
pub fn runtime_dir_from_env() -> String {
    runtime_dir(std::env::var("XDG_RUNTIME_DIR").ok().as_deref())
}

/// flock-файл «один Rexon на сессию».
pub fn lock_path(runtime_dir: &str) -> String {
    format!("{runtime_dir}/rexon.lock")
}

/// Самый длинный адрес сокета, который примет ядро: `sockaddr_un.sun_path` —
/// 108 байт вместе с завершающим нулём.
pub const SUN_PATH_MAX: usize = 107;

/// Имя сокета в runtime-каталоге.
const SOCKET_FILE: &str = "rexon-activate.sock";

/// Корень запасного каталога. Именно `/tmp`, а не `std::env::temp_dir()`:
/// `TMPDIR` бывает сколь угодно длинным, и запасной адрес вылетел бы за
/// предел по той же причине, что и основной.
const FALLBACK_ROOT: &str = "/tmp";

/// Действующий uid этого процесса — владелец запасного каталога и
/// единственный, чей стук слушатель принимает.
pub fn current_uid() -> u32 {
    unsafe { libc::geteuid() }
}

/// Основной адрес сокета — рядом с flock-файлом, в том же каталоге.
pub fn preferred_socket_path(runtime_dir: &str) -> String {
    format!("{runtime_dir}/{SOCKET_FILE}")
}

/// Запасной каталог пользователя `uid`: `/tmp/rexon-<uid>` (права 0700).
pub fn fallback_socket_dir(uid: u32) -> String {
    format!("{FALLBACK_ROOT}/rexon-{uid}")
}

/// Запасной адрес для runtime-каталога: `/tmp/rexon-<uid>/activate-<хеш>.sock`.
///
/// Хеш каталога в имени обязателен: у одного пользователя бывает несколько
/// сеансов с разными (и оба длинными) `XDG_RUNTIME_DIR`, замки у них
/// разные, и без хеша второй сеанс стучался бы в окно первого.
///
/// ⚠️ Хеш — свой FNV-1a, а не `DefaultHasher`: его алгоритм std не обещает
/// сохранять между выпусками, а слушатель и стук бывают разными бинарями
/// (колесо Python и `rexon-gui`), собранными разными версиями Rust.
pub fn fallback_socket_path(runtime_dir: &str, uid: u32) -> String {
    let key = match runtime_dir.trim_end_matches('/') {
        "" => runtime_dir,
        trimmed => trimmed,
    };
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{}/activate-{hash:016x}.sock", fallback_socket_dir(uid))
}

/// Сокет «подними окно» для пользователя `uid` — см. [`socket_path`].
pub fn socket_path_for(runtime_dir: &str, uid: u32) -> String {
    let preferred = preferred_socket_path(runtime_dir);
    if preferred.len() <= SUN_PATH_MAX {
        preferred
    } else {
        fallback_socket_path(runtime_dir, uid)
    }
}

/// Сокет «подними окно» — ЕДИНСТВЕННАЯ функция адреса: на нём первый Rexon
/// слушает, второй стучится (оба входа, Rust и Python через мост).
///
/// ⚠️ Обычно — рядом с замком, в том же каталоге: сокет живёт и умирает
/// вместе с сеансом. Но `sun_path` вмещает 107 байт, и при длинном
/// `XDG_RUNTIME_DIR` (замер 15.09: слушатель Rust-окна не вставал с
/// «path must be shorter than SUN_LEN», второй запуск врал «уже запущен»
/// вместо поднятого окна) адрес уходит в короткий запасной каталог
/// [`fallback_socket_path`]. Решение принимается ЗДЕСЬ, до `bind` и
/// `connect`, поэтому обе стороны приходят к одному адресу.
///
/// Почему запасной адрес — файл в `/tmp/rexon-<uid>/`, а не абстрактное имя
/// Linux: слушатель классического окна — `QLocalServer`, он получает
/// адрес строкой через мост и абстрактного имени без правки Python не
/// понимает. Абстрактный адрес у Rust и файловый у Python — это два
/// адреса на один механизм, и Rust-запуск не достучался бы до
/// Python-хозяина. Чужого пользователя отсекают права каталога (0700,
/// владелец проверяется, [`ensure_private_dir`]) и сверка uid собеседника
/// на ОБОИХ концах соединения (`SO_PEERCRED`).
pub fn socket_path(runtime_dir: &str) -> String {
    socket_path_for(runtime_dir, current_uid())
}

/// Строка в журнал, если адрес ушёл в запасной каталог; `None` — основной.
pub fn socket_address_note(runtime_dir: &str) -> Option<String> {
    let preferred = preferred_socket_path(runtime_dir);
    if preferred.len() <= SUN_PATH_MAX {
        return None;
    }
    Some(format!(
        "адрес «подними окно» {preferred} — {} байт, а в sun_path помещается {SUN_PATH_MAX}; \
         взят запасной {}",
        preferred.len(),
        fallback_socket_path(runtime_dir, current_uid())
    ))
}

/// Подготовить личный каталог сокета: создать с правами 0700 либо
/// убедиться, что существующий — настоящий каталог (не ссылка) этого
/// пользователя. Свой каталог с лишними правами поправляется до 0700.
///
/// ⚠️ `/tmp` общий на всю машину: чужой пользователь может заранее создать
/// `/tmp/rexon-<uid>` у себя и подложить туда свой сокет. Такой каталог НЕ
/// используется — вставать там нельзя, и причина уходит в журнал.
pub fn ensure_private_dir(dir: &str, uid: u32) -> Result<(), BindError> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let refuse = |reason: String| BindError::UnsafeDir {
        dir: dir.to_string(),
        reason,
    };
    let meta = match std::fs::symlink_metadata(dir) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::DirBuilder::new().mode(0o700).create(dir) {
                // Гонка с соседним запуском, создавшим каталог первым, —
                // не ошибка: ниже его всё равно проверяем.
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(refuse(format!("не создаётся: {e}"))),
            }
            std::fs::symlink_metadata(dir).map_err(|e| refuse(format!("не читается: {e}")))?
        }
        Err(e) => return Err(refuse(format!("не читается: {e}"))),
    };
    if meta.file_type().is_symlink() {
        return Err(refuse("это символическая ссылка, а не каталог".to_string()));
    }
    if !meta.is_dir() {
        return Err(refuse("это не каталог".to_string()));
    }
    if meta.uid() != uid {
        return Err(refuse(format!(
            "владелец — uid {}, а не {uid}",
            meta.uid()
        )));
    }
    if meta.mode() & 0o077 != 0 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| refuse(format!("права {:o} не исправляются: {e}", meta.mode() & 0o777)))?;
    }
    Ok(())
}

/// uid процесса на том конце соединения (`SO_PEERCRED`); `None` — ядро не
/// ответило.
pub fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (rc == 0 && len as usize == std::mem::size_of::<libc::ucred>()).then_some(cred.uid)
}

// ============================================================================
// Протокол стука
// ============================================================================

/// Просьба второго запуска: «подними своё окно».
pub const SHOW_REQUEST: &[u8] = b"show\n";
/// Ответ «поднял».
pub const REPLY_RAISED: &[u8] = b"ok\n";
/// Ответ «не смог поднять» — второй запуск обязан сказать человеку правду
/// про лоток, а не сделать вид, что окно на экране.
pub const REPLY_FAILED: &[u8] = b"no\n";

/// Сколько ждать ответа. Секунды хватает с запасом: на том конце живой цикл
/// событий, а не работа. Ждать дольше — держать человека перед пустым
/// экраном вместо того, чтобы сказать ему про лоток.
pub const KNOCK_TIMEOUT: Duration = Duration::from_secs(2);

/// Больше принимать нечего: просьба — «show» с разделом и подразделом,
/// ответ — короткое слово. 16 байт прежнего предела обрезали бы уже
/// «show desktops compare».
const WIRE_BUF: usize = 128;

/// Первое слово просьбы «подними окно».
const SHOW_WORD: &str = "show";

/// Обрезка перевода строки и пробелов по краям — порт `bytes.strip()`.
fn trimmed(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i + 1);
    &bytes[start..end]
}

/// Просьба ли это «подними окно» — голая или с разделом.
pub fn is_show_request(bytes: &[u8]) -> bool {
    parse_show_request(bytes).is_some()
}

/// Разобранная просьба «подними окно».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowRequest {
    /// (раздел, подраздел), который открыть; `None` — открыть как есть.
    pub section: Option<(String, String)>,
}

/// Проверенная цель: раздел из [`crate::qt_main::SECTIONS`] и подраздел,
/// который у этого раздела действительно бывает (иначе подраздел пуст).
///
/// Своего списка здесь нет: подразделы берутся из тех же двух таблиц,
/// которыми раздел выбирается при старте (`app::PAGE_TARGETS` и
/// `cli::SINGLE_TAB_SECTIONS`). Незнакомый раздел — `None`: чужое или
/// мусорное имя окно поднимает, но никуда не переключает.
pub fn valid_target(section: &str, subpage: &str) -> Option<(String, String)> {
    if !crate::qt_main::SECTIONS.contains(&section) {
        return None;
    }
    let known_subpage = !subpage.is_empty()
        && crate::app::PAGE_TARGETS
            .iter()
            .map(|(_, s, p)| (*s, *p))
            .chain(crate::cli::SINGLE_TAB_SECTIONS.iter().map(|(_, s, p)| (*s, *p)))
            .any(|(s, p)| s == section && p == subpage);
    let subpage = if known_subpage { subpage } else { "" };
    Some((section.to_string(), subpage.to_string()))
}

/// Разобрать просьбу: `show`, `show <раздел>` или `show <раздел> <подраздел>`.
///
/// `None` — это не «show» вовсе (окно не трогать). Голое `show` — прежний
/// протокол, работает как раньше. Раздел проверяется [`valid_target`];
/// незнакомый раздел или лишние слова дают просьбу БЕЗ раздела: окно
/// поднимается, но на мусор не переключается.
pub fn parse_show_request(bytes: &[u8]) -> Option<ShowRequest> {
    let text = std::str::from_utf8(trimmed(bytes)).ok()?;
    let mut words = text.split_ascii_whitespace();
    if words.next()? != SHOW_WORD {
        return None;
    }
    let rest: Vec<&str> = words.collect();
    let section = match rest.as_slice() {
        [section] => valid_target(section, ""),
        [section, subpage] => valid_target(section, subpage),
        _ => None,
    };
    Some(ShowRequest { section })
}

/// Слово стука для цели: `show\n` либо `show <раздел>[ <подраздел>]\n`.
/// Непроверенная цель в провод не уходит — уходит голое `show`.
pub fn show_request(target: Option<(&str, &str)>) -> Vec<u8> {
    match target.and_then(|(s, p)| valid_target(s, p)) {
        Some((section, subpage)) if subpage.is_empty() => {
            format!("{SHOW_WORD} {section}\n").into_bytes()
        }
        Some((section, subpage)) => format!("{SHOW_WORD} {section} {subpage}\n").into_bytes(),
        None => SHOW_REQUEST.to_vec(),
    }
}

/// Какой раздел нести в стуке по командной строке второго запуска.
///
/// Тот же перевод, каким раздел выбирается при старте
/// (`cli::start_section`: `--section=`, флаг одиночной вкладки, `--page=`),
/// плюс проверка [`valid_target`]. Иначе быстрое действие ярлыка
/// («Управление резервными копиями») при работающем Rexon поднимало окно
/// на том разделе, где его оставили, — раздел терялся в стуке.
pub fn knock_target(args: &[String]) -> Option<(String, String)> {
    let (section, subpage) = crate::cli::start_section(args)?;
    valid_target(&section, &subpage)
}

/// Ответ слушателя по факту: поднялось окно или нет.
pub fn reply_for(raised: bool) -> &'static [u8] {
    if raised {
        REPLY_RAISED
    } else {
        REPLY_FAILED
    }
}

/// Принял ли первый экземпляр просьбу и поднял ли окно.
///
/// Порт `reply.strip() == b"ok"`: любой другой ответ (включая пустой и
/// мусорный) считается отказом — соврать покупателю «поднял» хуже, чем
/// честно отправить его в лоток.
pub fn knock_accepted(reply: &[u8]) -> bool {
    trimmed(reply) == trimmed(REPLY_RAISED)
}

// ============================================================================
// Замок
// ============================================================================

/// Дескриптор замка живёт до конца ЭТОГО процесса вместе с `flock`.
///
/// Порт модульной переменной `_LOCK_FD` из биндинга. Стухшего замка не
/// бывает: ядро освобождает `flock` само при закрытии дескриптора или
/// смерти держателя, поэтому оставшийся с прошлой сессии файл замка второй
/// запуск не обманывает.
///
/// Рядом с дескриптором — путь замка: по нему [`holds_lock`] отвечает, взял
/// ли замок уже ЭТОТ процесс.
static LOCK_FD: Mutex<Option<(String, File)>> = Mutex::new(None);

/// `flock(LOCK_EX|LOCK_NB)` на `path`; `false` — замок уже держит живой
/// процесс, и вызывающий обязан не поднимать второе окно, а постучаться в
/// первое ([`activate_running_instance`]).
pub fn acquire_lock(path: &str) -> bool {
    let Ok(file) = File::create(path) else {
        return false;
    };
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return false;
    }
    if let Ok(mut slot) = LOCK_FD.lock() {
        *slot = Some((path.to_string(), file));
    }
    true
}

/// Держит ли ЭТОТ процесс замок на `path`.
///
/// Нужна точке входа, до которой можно дойти и после взятия замка, и в
/// обход него: `rexon/qt_main/app.py::main` зовётся из `rexon/app.py`, где
/// замок уже взят, но и напрямую (`python3 -m rexon.qt_main.app`), где его
/// не брал никто. Повторный `flock` из того же процесса на новом
/// дескрипторе блокирует сам себя — без этого вопроса первый экземпляр
/// принял бы себя за второй.
pub fn holds_lock(path: &str) -> bool {
    LOCK_FD
        .lock()
        .map(|slot| matches!(slot.as_ref(), Some((held, _)) if held == path))
        .unwrap_or(false)
}

/// Флаг классического окна (Python-вход): пункт ярлыка «Rexon (классическое
/// окно)». При занятом замке такой запуск просит держателя уступить место.
pub const CLASSIC_FLAG: &str = "--classic";

/// Что делать запуску, упёршемуся в занятый замок.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnBusyLock {
    /// Постучаться в первый экземпляр и поднять его окно.
    Knock,
    /// Уйти молча с кодом 0, ничего не трогая: окна никто не просил.
    ExitQuietly,
    /// Классическое окно (`--classic`): попросить держателя уступить замок
    /// ([`request_handover`]). Иначе при хозяине-Rust ярлык «классическое
    /// окно» лишь поднимал бы Rust-окно — открыть классическое было нельзя.
    RequestHandover,
}

/// Решение для запуска, упёршегося в занятый замок (`args` — без имени
/// программы). Одно на обе точки входа: Python (`rexon/app.py`) и Rust
/// (`rexon-gui/src/bin/main_window.rs`).
///
/// ⚠️ Скрытый запуск (`--tray`, им Rexon стартует при входе в систему) НЕ
/// стучится. Раньше стучался — и вытаскивал уже открытое окно на экран:
/// запуск, которому окно не нужно по определению, показывал его человеку
/// поверх рабочего стола. Если Rexon уже работает, скрытому запуску делать
/// нечего вовсе: значок в лотке уже есть.
pub fn on_busy_lock(args: &[String]) -> OnBusyLock {
    if crate::tray::starts_hidden(args) {
        OnBusyLock::ExitQuietly
    } else if args.iter().any(|a| a == CLASSIC_FLAG) {
        OnBusyLock::RequestHandover
    } else {
        OnBusyLock::Knock
    }
}

// ============================================================================
// Стук: «подними своё окно»
// ============================================================================

/// Попросить уже работающий Rexon поднять окно и ДОЖДАТЬСЯ ответа.
///
/// `false` — слушателя нет (Rexon старее этой правки), сокет мёртв либо
/// окно поднять не удалось. Без ответа мы не знаем, поднялось ли окно, а
/// соврать покупателю «поднял» хуже, чем честно отправить его в лоток.
pub fn activate_running_instance(socket_path: &str) -> bool {
    activate_running_instance_to(socket_path, None)
}

/// То же, но с разделом, который открыть в поднятом окне ([`show_request`]).
pub fn activate_running_instance_to(socket_path: &str, target: Option<(&str, &str)>) -> bool {
    knock(socket_path, target) == KnockOutcome::Raised
}

/// Чем кончился стук — от этого зависит, что сказать человеку.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnockOutcome {
    /// Слушатель поднял окно.
    Raised,
    /// Слушатель ответил, но окно не поднялось.
    Refused,
    /// Ответа не было: слушатель не встал, сокет мёртв, чужой владелец либо
    /// истёк срок. Строка — настоящая причина для журнала.
    NoAnswer(String),
}

/// Постучаться и узнать исход подробно (для текста второго запуска).
pub fn knock(socket_path: &str, target: Option<(&str, &str)>) -> KnockOutcome {
    knock_expecting_owner(socket_path, target, current_uid())
}

/// То же, но слушатель обязан принадлежать пользователю `owner_uid`.
pub fn knock_expecting_owner(
    socket_path: &str,
    target: Option<(&str, &str)>,
    owner_uid: u32,
) -> KnockOutcome {
    match exchange(socket_path, &show_request(target), owner_uid) {
        Ok(reply) if knock_accepted(&reply) => KnockOutcome::Raised,
        Ok(reply) if trimmed(&reply).is_empty() => {
            KnockOutcome::NoAnswer(format!("слушатель на {socket_path} закрыл соединение без ответа"))
        }
        Ok(_) => KnockOutcome::Refused,
        Err(reason) => KnockOutcome::NoAnswer(reason),
    }
}

/// Сказать слово слушателю и дождаться ответа; `Err` — настоящая причина,
/// почему ответа нет (слушателя нет, сокет мёртв, чужой владелец, срок).
fn exchange(socket_path: &str, word: &[u8], owner_uid: u32) -> Result<Vec<u8>, String> {
    if socket_path.len() > SUN_PATH_MAX {
        return Err(too_long(socket_path));
    }
    let mut sock = UnixStream::connect(socket_path)
        .map_err(|e| format!("к {socket_path} не подключиться: {e}"))?;
    // Слово уходит только своему: на чужом конце может стоять процесс
    // другого пользователя, подложивший сокет по нашему адресу.
    match peer_uid(&sock) {
        Some(uid) if uid == owner_uid => {}
        other => {
            return Err(format!(
                "на {socket_path} слушает не наш процесс (uid {}, ожидался {owner_uid}) — \
                 стук не отправлен",
                other.map_or_else(|| "не известен".to_string(), |u| u.to_string())
            ));
        }
    }
    // Оба таймаута обязательны: без них мёртвый на другом конце процесс
    // (сокет цел, цикл событий стоит) подвесил бы запуск навсегда.
    let io = |e: std::io::Error| format!("обмен с {socket_path} оборвался: {e}");
    sock.set_read_timeout(Some(KNOCK_TIMEOUT)).map_err(io)?;
    sock.set_write_timeout(Some(KNOCK_TIMEOUT)).map_err(io)?;
    sock.write_all(word).map_err(io)?;
    sock.flush().map_err(io)?;
    let mut reply = [0u8; WIRE_BUF];
    let n = sock.read(&mut reply).map_err(io)?;
    Ok(reply[..n].to_vec())
}

/// Причина «адрес не помещается» — с путём, длиной и пределом.
fn too_long(path: &str) -> String {
    format!(
        "адрес {path} — {} байт, а в sun_path помещается {SUN_PATH_MAX}",
        path.len()
    )
}

// ============================================================================
// Передача хозяина: классическое окно просит держателя уступить замок
// ============================================================================
//
// Хозяином сеанса может быть Rust-окно (с треем и автозапуском так будет
// всегда), а пункт ярлыка «Rexon (классическое окно)» запускает Python-вход.
// Обычный стук поднял бы Rust-окно — классическое открыть было бы нельзя.
// Поэтому классический запуск говорит особое слово, и держатель решает по
// себе ([`listener_action`]): Rust-держатель закрывает окно и значок и
// уходит (замок освобождается вместе с процессом), Python-держатель просто
// поднимает окно — оно и есть классическое, передавать нечего.

/// Просьба классического окна «уступи замок» — одно слово протокола.
pub const HANDOVER_REQUEST: &[u8] = b"handover\n";
/// Ответ держателя «ухожу, замок сейчас освободится».
pub const REPLY_LEAVING: &[u8] = b"bye\n";
/// Сколько новый процесс ждёт освобождения замка после «bye»: держателю
/// нужно закрыть окно и значок и выйти.
pub const HANDOVER_WAIT: Duration = Duration::from_secs(5);

/// Разобранное слово на сокете.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// «Подними окно» (с разделом или без).
    Show(ShowRequest),
    /// «Уступи замок» от классического окна.
    Handover,
}

/// Разобрать слово: `show …` ([`parse_show_request`]) либо `handover`.
pub fn parse_request(bytes: &[u8]) -> Option<Request> {
    if let Some(show) = parse_show_request(bytes) {
        return Some(Request::Show(show));
    }
    let word = std::str::from_utf8(trimmed(bytes)).ok()?;
    (word == trimmed_str(HANDOVER_REQUEST)).then_some(Request::Handover)
}

fn trimmed_str(bytes: &'static [u8]) -> &'static str {
    std::str::from_utf8(trimmed(bytes)).unwrap_or("")
}

/// Кто держит замок — от этого зависит ответ на `handover`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// Классическое окно на Python.
    Python,
    /// Окно на Rust.
    Rust,
}

/// Что делать слушателю на слово.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenerAction {
    /// Поднять окно и, если задан, открыть раздел.
    Raise(Option<(String, String)>),
    /// Уступить замок: закрыть окно и значок, выйти.
    HandOver,
}

/// Решение слушателя держателя `holder` на слово; `None` — чужое слово,
/// окно не трогать. Одно на оба слушателя.
pub fn listener_action(holder: Holder, bytes: &[u8]) -> Option<ListenerAction> {
    Some(match parse_request(bytes)? {
        Request::Show(show) => ListenerAction::Raise(show.section),
        // Держатель — само классическое окно: передавать нечего.
        Request::Handover if holder == Holder::Python => ListenerAction::Raise(None),
        Request::Handover => ListenerAction::HandOver,
    })
}

/// Взять замок, подождав его освобождения не дольше `limit`.
///
/// Нужна после «bye»: держатель уходит не мгновенно (закрывает окно и
/// значок), и гадать с таймингами снаружи значило бы то промахиваться, то
/// ждать зря.
pub fn wait_for_lock(path: &str, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if acquire_lock(path) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ============================================================================
// Перезапуск: новый процесс ждёт, пока уйдёт прежний
// ============================================================================
//
// Живой случай 08.10.2026: «Перезапустить» после смены языка — и Rexon не
// вернулся. Прежняя схема ждала выхода старого процесса снаружи (`sh` с
// `kill -0` до 30 с) и запускала новый «вслепую»: не ушёл старый вовремя —
// новый утыкался в замок, стучался в уходящее окно и тихо выходил, а
// следом уходил и старый. Теперь новый процесс знает, что он перезапуск
// ([`RESTART_OF_ENV`] = pid прежнего), и сам ждёт освобождения замка
// ([`wait_for_previous`]); каждый шаг — строкой в журнал.

/// Переменная «я перезапуск процесса <pid>» — ставит перезапускающий.
pub const RESTART_OF_ENV: &str = "REXON_RESTART_OF";

/// Сколько новый процесс ждёт ухода прежнего. Выход прежнего ограничен
/// сторожем выхода (`main_window`: `EXIT_WATCHDOG`), так что ждать дольше
/// него незачем, а меньше — значит снова уйти раньше старого.
pub const RESTART_LOCK_WAIT: Duration = Duration::from_secs(45);

/// Жив ли процесс `pid` по-настоящему (зомби — уже не жив: замки он
/// отпустил, а `kill -0` на нём отвечает «есть»).
pub fn process_alive(pid: u32) -> bool {
    if pid == 0 || unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 {
        // EPERM — процесс есть, но чужой; для нашего случая он «жив».
        return std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    }
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        // Состояние — первое поле после закрывающей скобки имени.
        Ok(stat) => stat
            .rsplit_once(") ")
            .and_then(|(_, rest)| rest.chars().next())
            .is_none_or(|state| state != 'Z' && state != 'X'),
        Err(_) => false,
    }
}

/// Кто держит `flock` на файле `path` — по `/proc/locks`. Для журнала:
/// «замок занят» без имени держателя ничего не объясняет.
pub fn lock_holder_pid(path: &str) -> Option<u32> {
    let meta = std::fs::metadata(path).ok()?;
    let dev = meta.dev();
    let (major, minor) = (libc::major(dev), libc::minor(dev));
    let ino = meta.ino();
    let locks = std::fs::read_to_string("/proc/locks").ok()?;
    lock_holder_in(&locks, major, minor, ino)
}

/// Разбор строк `/proc/locks`: `1: FLOCK ADVISORY WRITE <pid> <maj>:<min>:<ino> 0 EOF`.
pub fn lock_holder_in(locks: &str, major: u32, minor: u32, ino: u64) -> Option<u32> {
    locks.lines().find_map(|line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        // Строки ожидающих (` -> FLOCK …`) сдвинуты на одно поле — пропускаем.
        if f.len() < 6 || f[1] != "FLOCK" {
            return None;
        }
        let mut id = f[5].split(':');
        let maj = u32::from_str_radix(id.next()?, 16).ok()?;
        let min = u32::from_str_radix(id.next()?, 16).ok()?;
        let i: u64 = id.next()?.parse().ok()?;
        (maj == major && min == minor && i == ino).then(|| f[4].parse().ok()).flatten()
    })
}

/// Чем кончилось ожидание прежнего процесса.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviousWait {
    /// Замок наш — прежний ушёл.
    Acquired(Duration),
    /// Прежний так и не ушёл за отведённое время.
    PreviousStillRunning,
    /// Прежний ушёл, а замок держит кто-то ещё (pid, если известен):
    /// например, Rexon запустили вручную, пока шёл перезапуск.
    OtherHolder(Option<u32>),
}

/// Ждать освобождения замка `path`, пока жив прежний процесс `previous`,
/// но не дольше `limit`.
pub fn wait_for_previous(path: &str, previous: u32, limit: Duration) -> PreviousWait {
    let started = Instant::now();
    let deadline = started + limit;
    // После смерти прежнего замок освобождается сразу; полсекунды запаса —
    // на случай, если ядро ещё не дочистило дескрипторы.
    let mut gone_since: Option<Instant> = None;
    loop {
        if acquire_lock(path) {
            return PreviousWait::Acquired(started.elapsed());
        }
        let now = Instant::now();
        if !process_alive(previous) {
            let since = *gone_since.get_or_insert(now);
            if now.duration_since(since) > Duration::from_millis(500) {
                return PreviousWait::OtherHolder(lock_holder_pid(path));
            }
        }
        if now >= deadline {
            return PreviousWait::PreviousStillRunning;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Чем кончилась просьба уступить замок.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoverOutcome {
    /// Держатель — классическое окно: поднял себя, этому запуску уходить.
    Raised,
    /// Держатель ушёл, замок теперь наш: запуск продолжается первым.
    Acquired,
    /// Не вышло: слушателя нет, он отказал либо замок не освободился в срок.
    Failed,
}

/// Попросить держателя замка `lock_path` уступить место классическому окну.
pub fn request_handover(socket_path: &str, lock_path: &str, wait: Duration) -> HandoverOutcome {
    let Ok(reply) = exchange(socket_path, HANDOVER_REQUEST, current_uid()) else {
        return HandoverOutcome::Failed;
    };
    if knock_accepted(&reply) {
        return HandoverOutcome::Raised;
    }
    if trimmed(&reply) == trimmed(REPLY_LEAVING) && wait_for_lock(lock_path, wait) {
        return HandoverOutcome::Acquired;
    }
    HandoverOutcome::Failed
}

/// Жив ли слушатель на адресе — БЕЗ просьбы поднять окно.
///
/// Отдельно от стука намеренно: проверка живости нужна тому, кто СОБИРАЕТСЯ
/// слушать, и она не вправе дёргать окно чужого экземпляра как побочный
/// эффект своего запуска.
pub fn socket_alive(path: &str) -> bool {
    UnixStream::connect(path).is_ok()
}

// ============================================================================
// Слушатель
// ============================================================================

/// Почему не удалось встать слушателем.
#[derive(Debug)]
pub enum BindError {
    /// На адресе уже кто-то слушает: мы не первый.
    Busy,
    /// Адрес занят НЕ сокетом — чужой файл сносить нельзя.
    NotASocket,
    /// Ошибка файловой системы или ядра.
    Io(std::io::Error),
    /// Адрес не помещается в `sun_path` (путь, длина — в тексте).
    TooLong(String),
    /// Запасной каталог непригоден: чужой владелец, ссылка, не каталог.
    UnsafeDir { dir: String, reason: String },
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::Busy => write!(f, "на адресе уже слушает живой Rexon"),
            BindError::NotASocket => write!(f, "адрес занят не сокетом"),
            BindError::Io(e) => write!(f, "{e}"),
            BindError::TooLong(path) => write!(f, "{}", too_long(path)),
            BindError::UnsafeDir { dir, reason } => {
                write!(f, "запасной каталог {dir} непригоден: {reason}")
            }
        }
    }
}

/// Освободить адрес под слушателя: снести файл, только если это МЁРТВЫЙ
/// сокет. `Ok(())` — адрес свободен (файла не было либо мёртвый снесён).
///
/// Одна на оба слушателя: Rust ([`ActivationListener::bind`]) и Python
/// (`QLocalServer` в `rexon/qt_main/app.py`). Python-слушатель раньше звал
/// `QLocalServer::removeServer`, который сносит файл БЕЗУСЛОВНО, — и прямой
/// запуск модуля в обход замка отнимал живой общий сокет у работающего
/// Rexon, оставляя его без двери для вторых запусков.
///
/// Она же готовит запасной каталог ([`socket_path`] при длинном
/// `XDG_RUNTIME_DIR`): Python-слушатель зовёт её перед `QLocalServer.listen`,
/// и каталог 0700 появляется у обоих слушателей одним кодом.
pub fn clear_stale_socket(path: &str) -> Result<(), BindError> {
    if path.len() > SUN_PATH_MAX {
        return Err(BindError::TooLong(path.to_string()));
    }
    let uid = current_uid();
    let fallback_dir = fallback_socket_dir(uid);
    if std::path::Path::new(path).parent() == Some(std::path::Path::new(&fallback_dir)) {
        ensure_private_dir(&fallback_dir, uid)?;
    }
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(());
    };
    if socket_alive(path) {
        return Err(BindError::Busy);
    }
    if !meta.file_type().is_socket() {
        return Err(BindError::NotASocket);
    }
    std::fs::remove_file(path).map_err(BindError::Io)
}

/// Слушатель просьб «подними окно» от вторых запусков.
///
/// Порт `rexon/qt_main/app.py::_serve_activation` без Qt: `QLocalServer` на
/// Unix — это тот же `AF_UNIX`-сокет, и ядру для него Qt не нужен.
pub struct ActivationListener {
    listener: UnixListener,
    path: String,
    /// inode сокета на момент захвата — см. [`Drop`].
    inode: u64,
    /// Чей стук принимается: по умолчанию — свой uid ([`current_uid`]).
    owner_uid: u32,
}

impl ActivationListener {
    /// Встать слушателем на `path`.
    ///
    /// ⚠️⚠️ ПОЧЕМУ НЕ `QLocalServer::removeServer`, КАК В PYTHON. Тот сносит
    /// файл по адресу БЕЗУСЛОВНО — и это ровно та ловушка, которая уже
    /// стоила проекту сеанса: `$XDG_RUNTIME_DIR` общий на весь сеанс, и
    /// прогон, снёсший чужой сокет, отобрал у живой программы её адрес.
    /// Поэтому здесь порядок обратный: сперва спрашиваем, не слушает ли по
    /// адресу ЖИВОЙ Rexon (`socket_alive`), и если да — уходим с
    /// [`BindError::Busy`], ничего не снося. Сносим только мёртвый файл, и
    /// только если он действительно сокет.
    pub fn bind(path: &str) -> Result<ActivationListener, BindError> {
        clear_stale_socket(path)?;
        let listener = UnixListener::bind(path).map_err(BindError::Io)?;
        let inode = std::fs::symlink_metadata(path)
            .map(|m| m.ino())
            .map_err(BindError::Io)?;
        Ok(ActivationListener {
            listener,
            path: path.to_string(),
            inode,
            owner_uid: current_uid(),
        })
    }

    /// Принимать стук только от пользователя `uid` (по умолчанию — свой).
    ///
    /// ⚠️ Стук чужого пользователя — отказ без чтения слова: иначе процесс
    /// другого пользователя мог бы дёргать окно или, хуже, словом
    /// `handover` закрыть Rexon и забрать место.
    pub fn with_owner_uid(mut self, uid: u32) -> ActivationListener {
        self.owner_uid = uid;
        self
    }

    /// Адрес, на котором стоим.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Обслужить ОДНУ просьбу: принять соединение, прочитать слово, поднять
    /// окно и ответить по факту.
    ///
    /// `Some(поднялось)` — просьба обслужена; `None` — соединение не
    /// состоялось либо на нём пришло не «show» (тогда мы не трогаем окно и
    /// отвечаем отказом: чужой стук в наш адрес не повод дёргать экран).
    pub fn serve_one<F: FnMut() -> bool>(&self, raise: &mut F) -> Option<bool> {
        self.serve_one_to(&mut |_section| raise())
    }

    /// То же, но `raise` получает раздел из просьбы (`None` — без раздела).
    /// Хозяина такой слушатель не передаёт: на `handover` — отказ «no».
    pub fn serve_one_to<F: FnMut(Option<(String, String)>) -> bool>(
        &self,
        raise: &mut F,
    ) -> Option<bool> {
        self.serve_one_with(None, &mut |action| match action {
            ListenerAction::Raise(section) => raise(section),
            ListenerAction::HandOver => false,
        })
    }

    /// Слушатель держателя `holder`, умеющий передать хозяина
    /// ([`listener_action`]).
    ///
    /// ⚠️ На [`ListenerAction::HandOver`] обработчик обязан НЕ выходить сам,
    /// а запланировать выход (в потоке GUI: закрыть окно и значок, выйти —
    /// замок уйдёт вместе с процессом) и вернуть `true`: ответ «bye» уходит
    /// ПОСЛЕ его возврата. `false` — остаюсь, запуск получит отказ.
    pub fn serve_one_for<F: FnMut(ListenerAction) -> bool>(
        &self,
        holder: Holder,
        handler: &mut F,
    ) -> Option<bool> {
        self.serve_one_with(Some(holder), handler)
    }

    fn serve_one_with<F: FnMut(ListenerAction) -> bool>(
        &self,
        holder: Option<Holder>,
        handler: &mut F,
    ) -> Option<bool> {
        let (mut conn, _) = self.listener.accept().ok()?;
        if peer_uid(&conn) != Some(self.owner_uid) {
            let _ = conn.set_write_timeout(Some(KNOCK_TIMEOUT));
            let _ = conn.write_all(REPLY_FAILED);
            let _ = conn.shutdown(std::net::Shutdown::Both);
            return None;
        }
        let _ = conn.set_read_timeout(Some(KNOCK_TIMEOUT));
        let _ = conn.set_write_timeout(Some(KNOCK_TIMEOUT));
        let mut buf = [0u8; WIRE_BUF];
        let read = conn.read(&mut buf).unwrap_or(0);
        let word = &buf[..read];
        let action = match holder {
            Some(holder) => listener_action(holder, word),
            // Слушатель без передачи хозяина понимает только «show».
            None => parse_show_request(word).map(|show| ListenerAction::Raise(show.section)),
        };
        let Some(action) = action else {
            let _ = conn.write_all(REPLY_FAILED);
            let _ = conn.flush();
            return None;
        };
        let leaving = action == ListenerAction::HandOver;
        let raised = handler(action);
        let reply = match (leaving, raised) {
            (true, true) => REPLY_LEAVING,
            (true, false) => REPLY_FAILED,
            (false, raised) => reply_for(raised),
        };
        let _ = conn.write_all(reply);
        let _ = conn.flush();
        // Закрытие — сразу после ответа: второй запуск ждёт одно слово и
        // уходит, держать соединение открытым нечем и незачем.
        let _ = conn.shutdown(std::net::Shutdown::Both);
        Some(raised)
    }

    /// Обслуживать просьбы до конца процесса.
    pub fn serve_forever<F: FnMut() -> bool>(&self, mut raise: F) {
        self.serve_forever_to(move |_section| raise())
    }

    /// То же, но `raise` получает раздел из просьбы.
    pub fn serve_forever_to<F: FnMut(Option<(String, String)>) -> bool>(&self, mut raise: F) {
        loop {
            // Сорванное соединение — не причина перестать слушать: иначе
            // один кривой стук лишил бы покупателя двери в программу до
            // перезапуска.
            let _ = self.serve_one_to(&mut raise);
        }
    }

    /// Встать слушателем и обслуживать просьбы в своём потоке.
    ///
    /// ⚠️ `raise` зовётся ИЗ ЭТОГО ПОТОКА. Вызывающий отвечает за то, чтобы
    /// внутри `raise` работа с окном попадала в поток GUI (у Qt поднимать
    /// окно из чужого потока нельзя).
    pub fn spawn<F>(path: &str, raise: F) -> Result<(), BindError>
    where
        F: FnMut() -> bool + Send + 'static,
    {
        let mut raise = raise;
        ActivationListener::spawn_to(path, move |_section| raise())
    }

    /// То же, но `raise` получает раздел из просьбы (быстрое действие ярлыка
    /// при работающем Rexon). ⚠️ Зовётся из потока слушателя — как и `spawn`.
    pub fn spawn_to<F>(path: &str, raise: F) -> Result<(), BindError>
    where
        F: FnMut(Option<(String, String)>) -> bool + Send + 'static,
    {
        let server = ActivationListener::bind(path)?;
        std::thread::Builder::new()
            .name("rexon-activate".to_string())
            .spawn(move || server.serve_forever_to(raise))
            .map_err(BindError::Io)?;
        Ok(())
    }

    /// Обслуживать просьбы держателя `holder` до конца процесса.
    pub fn serve_forever_for<F: FnMut(ListenerAction) -> bool>(&self, holder: Holder, mut handler: F) {
        loop {
            let _ = self.serve_one_for(holder, &mut handler);
        }
    }

    /// Встать слушателем держателя `holder` в своём потоке — с передачей
    /// хозяина ([`Self::serve_one_for`]). ⚠️ `handler` зовётся из потока
    /// слушателя, как и у `spawn`.
    pub fn spawn_for<F>(path: &str, holder: Holder, handler: F) -> Result<(), BindError>
    where
        F: FnMut(ListenerAction) -> bool + Send + 'static,
    {
        let server = ActivationListener::bind(path)?;
        std::thread::Builder::new()
            .name("rexon-activate".to_string())
            .spawn(move || server.serve_forever_for(holder, handler))
            .map_err(BindError::Io)?;
        Ok(())
    }
}

impl Drop for ActivationListener {
    fn drop(&mut self) {
        // Убираем ТОЛЬКО свой сокет: сверяем inode с тем, что захватили.
        // Иначе слушатель, встающий на наш адрес после нас, лишился бы его
        // от нашего же завершения — та самая кража общего сокета, только
        // задним ходом.
        let ours = std::fs::symlink_metadata(&self.path)
            .map(|m| m.file_type().is_socket() && m.ino() == self.inode)
            .unwrap_or(false);
        if ours {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

// ============================================================================
// Rexon обновился, а работает прежняя копия
// ============================================================================
//
// Пакетный менеджер не трогает работающий процесс: он кладёт новый файл
// рядом и переименованием ставит его на место старого. Окно и лоток живут
// дальше прежней копией — до выхода из сеанса, иногда неделями, — и человек
// не понимает, почему «обновлённый» Rexon ведёт себя по-старому. Признаки
// замены видны прямо из процесса: ядро дописывает « (deleted)» к ссылке
// `/proc/self/exe`, если файл, из которого процесс запущен, снят с диска, а
// по прежнему пути лежит файл с другим inode/временем изменения.
// Решение — здесь (чистые функции под тестами на подставном пути), опрос и
// уведомление с кнопкой «Перезапустить» — у лотка (`rexon-gui::tray`).

/// Хвост, который ядро дописывает к ссылке на снятый с диска файл.
pub const DELETED_EXE_SUFFIX: &str = " (deleted)";

/// Ссылка процесса на свой исполняемый файл.
pub const PROC_SELF_EXE: &str = "/proc/self/exe";

/// Как часто лоток сверяет свой бинарь с диском, с. Опрос, а не inotify:
/// это `readlink` и два `stat` раз в полминуты, а слежка за каталогом
/// пакета будила бы лоток на каждом файле любого обновления системы.
pub const SELF_UPDATE_POLL_SECS: u64 = 30;

/// Сколько ждать нажатия кнопки уведомления, с: сутки — человек мог отойти.
pub const SELF_UPDATE_NOTICE_WAIT_SECS: u64 = 24 * 3600;

/// Отпечаток файла: устройство, inode и время изменения. Сменился любой из
/// трёх — на диске уже другой файл (или тот же, переписанный на месте).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileStamp {
    pub dev: u64,
    pub ino: u64,
    pub mtime_sec: i64,
    pub mtime_nsec: i64,
}

impl FileStamp {
    /// Отпечаток файла по пути (ссылки разворачиваются). `None` — файла нет.
    pub fn of(path: &std::path::Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self { dev: meta.dev(), ino: meta.ino(), mtime_sec: meta.mtime(), mtime_nsec: meta.mtime_nsec() })
    }
}

/// Что стало с исполняемым файлом работающего процесса.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExeChange {
    /// Работает то, что лежит на диске.
    Same,
    /// По прежнему пути лежит другой файл — новая версия. `stamp` — её
    /// отпечаток: по нему уведомление показывается один раз на обновление.
    Replaced { path: String, stamp: FileStamp },
    /// Файла по прежнему пути нет вовсе (пакет удалили или замена ещё
    /// идёт) — перезапускаться не во что, предлагать нечего.
    Gone,
}

/// Решение по наблюдениям: `link_target` — содержимое ссылки
/// `/proc/self/exe`, `running` — отпечаток работающего файла, `on_disk` —
/// отпечаток файла по пути из ссылки (без хвоста « (deleted)»).
pub fn exe_change(link_target: &str, running: Option<FileStamp>, on_disk: Option<FileStamp>) -> ExeChange {
    let (path, deleted) = match link_target.strip_suffix(DELETED_EXE_SUFFIX) {
        Some(p) => (p, true),
        None => (link_target, false),
    };
    let Some(stamp) = on_disk else {
        return if deleted { ExeChange::Gone } else { ExeChange::Same };
    };
    let replaced = deleted || running.is_some_and(|r| r != stamp);
    if replaced {
        ExeChange::Replaced { path: path.to_string(), stamp }
    } else {
        ExeChange::Same
    }
}

/// Живая сверка по ссылке `link` (в продукте — [`PROC_SELF_EXE`], в тестах
/// — подставная ссылка). `running` — отпечаток работающего файла; у
/// `/proc/self/exe` это `FileStamp::of(link)`: ядро отдаёт по ней сам
/// работающий файл, даже снятый с диска.
pub fn exe_change_at(link: &std::path::Path, running: Option<FileStamp>) -> ExeChange {
    let Ok(target) = std::fs::read_link(link) else {
        return ExeChange::Same;
    };
    let target = target.to_string_lossy().into_owned();
    let path = target.strip_suffix(DELETED_EXE_SUFFIX).unwrap_or(&target);
    exe_change(&target, running, FileStamp::of(std::path::Path::new(path)))
}

/// Память уведомления «Rexon обновлён»: показать ОДИН раз на обновление и
/// не раньше, чем замена устоялась.
///
/// Устоялась — значит один и тот же новый файл виден на двух опросах
/// подряд: посреди транзакции пакетного менеджера бинарь уже новый, а
/// библиотеки рядом ещё старые, и перезапуск в эту секунду только упал бы.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelfUpdateNotice {
    /// Отпечаток, о котором уже сказали.
    told: Option<FileStamp>,
    /// Отпечаток, замеченный прошлым опросом и ещё не подтверждённый.
    pending: Option<FileStamp>,
}

impl SelfUpdateNotice {
    /// Шаг опроса. `true` — показать уведомление сейчас.
    pub fn step(&mut self, change: &ExeChange) -> bool {
        let ExeChange::Replaced { stamp, .. } = change else {
            self.pending = None;
            return false;
        };
        if self.told == Some(*stamp) {
            return false;
        }
        if self.pending == Some(*stamp) {
            self.pending = None;
            self.told = Some(*stamp);
            return true;
        }
        self.pending = Some(*stamp);
        false
    }

    /// Шаг опроса по толчку ([`nudge_self_update_watch`]): обновление уже
    /// подтвердил помощник сеанса прав другой сборки, второй опрос
    /// «устоялось ли» не нужен. `true` — показать уведомление сейчас (о том
    /// же файле второй раз не говорим).
    pub fn confirm(&mut self, change: &ExeChange) -> bool {
        let ExeChange::Replaced { stamp, .. } = change else {
            return false;
        };
        self.pending = None;
        if self.told == Some(*stamp) {
            return false;
        }
        self.told = Some(*stamp);
        true
    }
}

/// Толчок сторожу «Rexon обновлён» и его ожидание.
static SELF_UPDATE_NUDGE: (Mutex<bool>, std::sync::Condvar) = (Mutex::new(false), std::sync::Condvar::new());

/// Разбудить сторожа «Rexon обновлён» сейчас, не дожидаясь опроса: сеанс
/// прав ответил «помощник другой сборки» (пакет обновили при открытом окне,
/// Б13). Уведомление то же самое и одно — второго окна про обновление нет.
pub fn nudge_self_update_watch() {
    let (flag, cv) = &SELF_UPDATE_NUDGE;
    *flag.lock().unwrap_or_else(|e| e.into_inner()) = true;
    cv.notify_all();
}

/// Ждать следующего опроса сторожа: `timeout` или толчок. `true` — толкнули.
pub fn wait_self_update_poll(timeout: Duration) -> bool {
    let (flag, cv) = &SELF_UPDATE_NUDGE;
    let guard = flag.lock().unwrap_or_else(|e| e.into_inner());
    let (mut guard, _) = cv
        .wait_timeout_while(guard, timeout, |nudged| !*nudged)
        .unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *guard)
}

/// Аргументы новой копии при перезапуске ради обновления: те же, что у
/// прежней, но без скрытого старта, самопроверки и раздела ярлыка.
/// Человек сам нажал «Перезапустить» — окно новой копии и есть ответ, что
/// перезапуск случился; молча вернуться в лоток значило бы оставить его
/// гадать.
pub fn update_restart_args(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|a| {
            *a != crate::tray::START_HIDDEN_FLAG
                && *a != "--selftest"
                && !a.starts_with(crate::cli::SECTION_FLAG)
                && !a.starts_with(crate::app::PAGE_FLAG)
        })
        .cloned()
        .collect()
}

/// Ключ кнопки «Перезапустить» в уведомлении.
pub const SELF_UPDATE_RESTART_ACTION: &str = "restart";

pub fn self_update_title() -> String {
    // TRANSLATORS: Title of a desktop notification: the Rexon package was updated while Rexon kept running.
    tr("Rexon was updated")
}

pub fn self_update_body() -> String {
    // TRANSLATORS: Body of the "Rexon was updated" notification; the button below it says "Restart".
    tr("A new version of Rexon is installed, but the old one is still running. Restart Rexon to start using the new version.")
}

/// Кнопки уведомления.
pub fn self_update_actions() -> Vec<(String, String)> {
    // TRANSLATORS: Button in the "Rexon was updated" notification.
    vec![(SELF_UPDATE_RESTART_ACTION.to_string(), tr("Restart Rexon"))]
}

/// Заголовок уведомления, если перезапуск не удался (текст — причина).
pub fn self_update_restart_failed_title() -> String {
    // TRANSLATORS: Title of a desktop notification when Rexon failed to come back after a restart.
    tr("Rexon didn't restart")
}

/// Что сделать самому, если служба уведомлений кнопок не умеет.
pub fn self_update_no_buttons() -> String {
    // TRANSLATORS: Added to the "Rexon was updated" notification when the system can't show buttons.
    tr("To restart, choose “Quit” in the menu of the Rexon tray icon and open Rexon again.")
}

// ============================================================================
// Что говорить человеку
// ============================================================================
//
// Ни одна точка входа не вправе держать свою копию этих строк: их видит
// покупатель, а значит расхождение между Python- и Rust-входом было бы
// расхождением в продукте.

/// Второй запуск поднял окно первого.
pub fn said_raised() -> String {
    tr("↑ Rexon is already running — brought its window to the front.")
}
/// Второй запуск уткнулся в замок и поднять окно не смог.
pub fn said_already_running() -> String {
    tr("⚠️  Rexon is already running — not adding a second tray icon.")
}
/// Строка с адресом замка (для второго запуска, ушедшего ни с чем).
pub fn said_lock_path(lock_path: &str) -> String {
    format!("    {}", trf("Lock: %1", &[&lock_path]))
}
/// Куда смотреть, когда окно поднять не удалось.
pub fn said_look_in_tray() -> String {
    format!(
        "    {}",
        tr("Couldn't bring up the window — look for the Rexon icon in the system tray.")
    )
}
/// Rexon работает (замок занят), но на просьбу показать окно не ответил:
/// слушатель не встал, адрес непригоден либо программа не отвечает. Прежние
/// строки («второй значок не создаю» и путь замка) здесь вводили в
/// заблуждение: человек просил окно, а не значок.
pub fn said_not_responding() -> String {
    tr("⚠️  Rexon is already open but doesn't respond to the request to show its window. \
        Open it from the icon in the system tray or restart Rexon.")
}

/// Что сказать запуску, упёршемуся в занятый замок: `knock` — исход стука,
/// `None` — стука не было (скрытый запуск). Одно решение на все входы.
pub fn said_on_busy_lock(lock_path: &str, knock: Option<&KnockOutcome>) -> Vec<String> {
    let already = || vec![said_already_running(), said_lock_path(lock_path)];
    match knock {
        Some(KnockOutcome::Raised) => vec![said_raised()],
        Some(KnockOutcome::NoAnswer(_)) => vec![said_not_responding()],
        Some(KnockOutcome::Refused) => {
            let mut lines = already();
            lines.push(said_look_in_tray());
            lines
        }
        None => already(),
    }
}

// ============================================================================
// Уведомление о запуске (X11, freedesktop startup-notification)
// ============================================================================
//
// 2026-11-01, «окно открыто, а курсор думает». MATE на X11: Rexon уже
// работает, человек жмёт пункт меню — второй запуск упирается в замок,
// стучит первому (окно поднимается) и выходит. Панель же запускала его с
// `DESKTOP_STARTUP_ID` и ждёт ответа «remove: ID=…» в `_NET_STARTUP_INFO`:
// до него курсор «думает», а на панели висит «Запускается Rexon». Ответ
// обычно шлёт сама Qt (`QXcbScreen::windowShown` при показе первого
// верхнего окна), но второй запуск до `QApplication` не доходит — и не
// отвечал никто, пока панель не сдавалась по таймауту. Журнал владельца:
// каждый такой запуск из меню — «замок занят» → «стук: Raised» → выход.
//
// Теперь запуск, уходящий без своего окна, завершает уведомление сам.

/// Переменная, в которой запускающий (панель, меню) передаёт ID запуска.
pub const STARTUP_ID_ENV: &str = "DESKTOP_STARTUP_ID";

/// ID запуска из окружения; пустое значение — запуска с уведомлением не было.
pub fn startup_id_from(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// Сообщение «запуск окончен» по спецификации startup-notification:
/// значение в кавычках, если в нём пробел, кавычка или обратная черта.
pub fn startup_remove_message(id: &str) -> String {
    let needs_quotes = id.chars().any(|c| c == ' ' || c == '"' || c == '\\');
    if needs_quotes {
        let escaped: String = id
            .chars()
            .flat_map(|c| match c {
                '"' | '\\' => vec!['\\', c],
                _ => vec![c],
            })
            .collect();
        format!("remove: ID=\"{escaped}\"")
    } else {
        format!("remove: ID={id}")
    }
}

/// Нарезка сообщения на клиентские сообщения X по 20 байт, с завершающим
/// нулём (как в libstartup-notification и в Qt): первый кусок идёт с типом
/// `_NET_STARTUP_INFO_BEGIN`, остальные — `_NET_STARTUP_INFO`.
pub fn startup_message_chunks(message: &str) -> Vec<[u8; 20]> {
    let mut bytes = message.as_bytes().to_vec();
    bytes.push(0);
    bytes
        .chunks(20)
        .map(|c| {
            let mut out = [0u8; 20];
            out[..c.len()].copy_from_slice(c);
            out
        })
        .collect()
}

/// Послать «remove: ID=…» на корневое окно дисплея `display` (`None` —
/// `DISPLAY` процесса). libX11 грузится в момент вызова: без неё (чистый
/// Wayland) — честная ошибка, а не падение.
pub fn send_startup_remove(display: Option<&str>, id: &str) -> Result<(), String> {
    use std::ffi::CString;
    use x11_dl::xlib;

    let lib = xlib::Xlib::open().map_err(|e| format!("libX11 не загрузилась: {e}"))?;
    let name = display
        .map(|d| CString::new(d).map_err(|_| "имя дисплея с нулевым байтом".to_string()))
        .transpose()?;
    let chunks = startup_message_chunks(&startup_remove_message(id));
    // SAFETY: свой дисплей, открыт и закрыт здесь же; событие заполнено
    // целиком (обнулено), указатели живут до конца блока.
    unsafe {
        let dpy = (lib.XOpenDisplay)(name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()));
        if dpy.is_null() {
            return Err(format!("дисплей {} не открылся", display.unwrap_or("$DISPLAY")));
        }
        let root = (lib.XDefaultRootWindow)(dpy);
        let begin = (lib.XInternAtom)(dpy, c"_NET_STARTUP_INFO_BEGIN".as_ptr(), xlib::False);
        let more = (lib.XInternAtom)(dpy, c"_NET_STARTUP_INFO".as_ptr(), xlib::False);
        for (i, chunk) in chunks.iter().enumerate() {
            let mut ev: xlib::XClientMessageEvent = std::mem::zeroed();
            ev.type_ = xlib::ClientMessage;
            ev.display = dpy;
            ev.window = root;
            ev.message_type = if i == 0 { begin } else { more };
            ev.format = 8;
            for (k, b) in chunk.iter().enumerate() {
                ev.data.as_bytes_mut()[k] = *b as std::os::raw::c_char;
            }
            let mut xev = xlib::XEvent::from(ev);
            (lib.XSendEvent)(dpy, root, xlib::False, xlib::PropertyChangeMask, &mut xev);
        }
        (lib.XSync)(dpy, xlib::False);
        (lib.XCloseDisplay)(dpy);
    }
    Ok(())
}

/// Запуск уходит без своего окна (замок занят): завершить уведомление о
/// запуске, если оно было. `None` — уведомления не было; иначе — ID и исход
/// отправки. Переменную не снимаем: повторный «remove» того же ID (если её
/// унаследует ребёнок на Qt) безвреден, а снятие при живых потоках — нет.
pub fn complete_startup_notification() -> Option<(String, Result<(), String>)> {
    let id = startup_id_from(std::env::var(STARTUP_ID_ENV).ok().as_deref())?;
    let has_x = std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty());
    let sent = if has_x {
        send_startup_remove(None, &id)
    } else {
        Err("нет DISPLAY — уведомление X11 некому завершать".to_string())
    };
    Some((id, sent))
}
