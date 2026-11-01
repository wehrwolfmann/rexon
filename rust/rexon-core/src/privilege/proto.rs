//! Протокол окна и root-помощника (пункт Б2 плана, см. [`super`]).
//!
//! # Транспорт
//!
//! `AF_UNIX` + `SOCK_SEQPACKET` из `socketpair` (окно отдаёт свой конец
//! помощнику на fd 0/1, адреса у канала нет — З3). Одна запись в сокет =
//! один кадр = одно сообщение: границы держит ядро, длину мы не кодируем.
//!
//! # Кадр
//!
//! JSON вида `{"v":1,"msg":{…}}`, не длиннее [`MAX_FRAME`] байт:
//! * `v` — версия протокола ([`PROTO_VERSION`]); чужая версия → ошибка;
//! * `msg` — сообщение ([`ClientMsg`] от окна, [`HelperMsg`] от помощника);
//! * лишние поля на любом уровне → ошибка (`deny_unknown_fields`).
//!
//! # Ошибка кадра = конец сеанса
//!
//! Порченый, слишком большой, с лишними полями или чужой версии кадр —
//! [`ProtoError`]. Восстанавливаться посреди потока нельзя: кадр мог прийти
//! не от того, с кем мы договорились. Поэтому закрывает **та сторона, что
//! кадр получила**:
//! * помощник: роняет свой конец сокета и ведёт себя как при EOF (З4) —
//!   идущую транзакцию доводит, новых не берёт, выходит. Ответ на порченый
//!   кадр не шлётся: собеседнику, который шлёт мусор, отвечать нечего;
//! * окно: роняет свой конец (помощник видит EOF и выходит), сеанс считается
//!   мёртвым, идущая операция — с неизвестным исходом. Сырой `pkexec` вместо
//!   неё **не** запускается (З5): человек видит сообщение.
//!
//! # Отказ ≠ сбой
//!
//! [`Done`](HelperMsg::Done) — операция исполнялась (успешно или нет, с кодом
//! выхода программы). [`Error`](HelperMsg::Error) — до исполнения дело не
//! дошло или сеанс прерван; его [`ErrorCode::class`] различает «отказ
//! проверки» и «сбой помощника». Ни один код этого протокола не разрешает
//! окну откатиться на сырой `pkexec` (З5): сырой путь — только когда
//! помощника нет вовсе, и это решает клиент ДО рукопожатия.
//!
//! # Операции
//!
//! Тип операции — параметр `Op` (появится в Б1 как `privilege::op::PrivOp`).
//! Протокол о содержимом операции ничего не знает и только переносит её.

use std::fmt;
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Версия протокола. Меняется при любой несовместимой правке сообщений.
pub const PROTO_VERSION: u32 = 1;

/// Предел одного кадра в байтах (JSON целиком, с конвертом).
pub const MAX_FRAME: usize = 64 * 1024;

/// Сколько дескрипторов (`SCM_RIGHTS`) может нести один кадр. Самой
/// «толстой» операции хватает одного файла; запас — на будущее, но не
/// больше: каждый лишний fd — лишнее, что root держит открытым.
pub const MAX_FDS: usize = 4;

/// Отпечаток сборки Rexon для рукопожатия: версия и коммит,
/// `0.5.0+1a2b3c4d5e`.
///
/// Коммит — `REXON_GIT_REV` из `build.rs` (тот же, что в строке запуска
/// журнала, [`crate::applog::version_line`]). Окно и помощник — бинари одной
/// сборки одного крейта, поэтому отпечаток у них совпадает, а помощник от
/// другой установки Rexon (пакет обновили при открытом окне, чужая копия) —
/// отвергается. Сборка без git (из архива) даёт только версию.
pub fn build_id() -> String {
    let v = crate::window_menu::version_string();
    match option_env!("REXON_GIT_REV") {
        Some(rev) => format!("{v}+{rev}"),
        None => v,
    }
}

// ============================================================================
// Сообщения
// ============================================================================

/// Первое сообщение окна. Пока помощник его не принял, он не берёт ничего.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    /// [`build_id`] окна.
    pub build_id: String,
}

/// Окно → помощник.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMsg<Op> {
    /// Рукопожатие; ровно один раз и первым.
    Hello(Hello),
    /// Выполнить операцию. `id` выдаёт окно, помощник повторяет его в ответах.
    Request { id: u64, op: Op },
    /// Отменить операцию `id`. Действует только до касания системы; после —
    /// помощник доводит операцию и отвечает обычным `Done`.
    Cancel { id: u64 },
}

/// Помощник → окно.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HelperMsg {
    /// Рукопожатие принято; `build_id` помощника — для журнала окна.
    Welcome { build_id: String },
    /// Строка хода операции (вместо журнала в `/tmp`, З2). Помощник режет
    /// длинные строки сам: кадр больше [`MAX_FRAME`] не уйдёт.
    Progress { id: u64, line: String },
    /// Операция исполнена. `ok == false` — программа отработала с ошибкой:
    /// `code` — её код выхода (нет кода — убита сигналом), `message_key` —
    /// ключ перевода пояснения, если помощник его знает.
    Done {
        id: u64,
        ok: bool,
        code: Option<i32>,
        message_key: Option<String>,
    },
    /// Уже идёт другая операция: помощник берёт по одной. `id` — отвергнутой.
    Busy { id: u64 },
    /// До исполнения не дошло или сеанс прерван. `id` — операции, если ошибка
    /// о ней (`None` — о сеансе: рукопожатие, отзыв прав).
    Error {
        id: Option<u64>,
        code: ErrorCode,
        message_key: String,
    },
}

/// Коды [`HelperMsg::Error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Проверка аргументов не пройдена (имя, белый список, жизненно важный
    /// пакет, текущий стол — З1, З2, З6, З7, З8).
    Refused,
    /// Права отозваны: простой, блокировка, смена сеанса, «Запретить сейчас».
    /// Сеанс закрывается; новый — только с новым паролем.
    Revoked,
    /// Рукопожатие: другая сборка Rexon. Помощник после ответа выходит.
    BuildMismatch,
    /// Запрос до `Hello`, повторный `Hello`, `Cancel` чужого `id`.
    Sequence,
    /// `Cancel` принят до касания системы: операция не выполнялась.
    Cancelled,
    /// Сбой самого помощника (не запустилась программа, ошибка ввода-вывода).
    Internal,
}

/// Чем по сути является [`ErrorCode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Помощник сознательно не стал делать. Обойти сырым `pkexec` нельзя (З5).
    Refusal,
    /// Операцию отменил сам человек.
    Cancelled,
    /// Помощник не смог. Сырой `pkexec` тоже не запускается (З5): помощник
    /// был, и раз он не справился, второй путь в обход его — та же дыра.
    Failure,
}

impl ErrorCode {
    pub fn class(self) -> ErrorClass {
        match self {
            Self::Refused | Self::Revoked | Self::BuildMismatch | Self::Sequence => {
                ErrorClass::Refusal
            }
            Self::Cancelled => ErrorClass::Cancelled,
            Self::Internal => ErrorClass::Failure,
        }
    }
}

/// Проверка рукопожатия на стороне помощника: `Ok` — продолжаем,
/// `Err` — ответить [`HelperMsg::Error`] с этим кодом и выйти.
pub fn check_hello(hello: &Hello, own_build_id: &str) -> Result<(), ErrorCode> {
    if hello.build_id == own_build_id {
        Ok(())
    } else {
        Err(ErrorCode::BuildMismatch)
    }
}

// ============================================================================
// Кодек
// ============================================================================

/// Ошибка кадра. Любая из них — конец сеанса (см. описание модуля).
#[derive(Debug)]
pub enum ProtoError {
    /// Ошибка сокета.
    Io(io::Error),
    /// Кадр длиннее [`MAX_FRAME`] (длина — настоящая, а не обрезанная).
    TooLarge(usize),
    /// Не JSON, обрыв, лишние или недостающие поля, неизвестное сообщение.
    Malformed(String),
    /// Чужая версия протокола (`None` — поля `v` нет или оно не число).
    Version(Option<u64>),
    /// Дескрипторы кадра: больше [`MAX_FDS`], обрезаны ядром (`MSG_CTRUNC`),
    /// чужой вид служебных данных. Все полученные с кадром fd уже закрыты.
    Fds(String),
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "сокет сеанса прав: {e}"),
            Self::TooLarge(n) => write!(f, "кадр {n} байт больше предела {MAX_FRAME}"),
            Self::Malformed(e) => write!(f, "порченый кадр: {e}"),
            Self::Version(Some(v)) => {
                write!(f, "версия протокола {v}, ожидалась {PROTO_VERSION}")
            }
            Self::Version(None) => write!(f, "в кадре нет версии протокола"),
            Self::Fds(e) => write!(f, "дескрипторы кадра: {e}"),
        }
    }
}

impl std::error::Error for ProtoError {}

impl From<io::Error> for ProtoError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Serialize)]
struct EnvelopeRef<'a, M> {
    v: u32,
    msg: &'a M,
}

/// Сообщение → байты кадра. Больше [`MAX_FRAME`] — [`ProtoError::TooLarge`].
pub fn encode<M: Serialize>(msg: &M) -> Result<Vec<u8>, ProtoError> {
    let bytes = serde_json::to_vec(&EnvelopeRef {
        v: PROTO_VERSION,
        msg,
    })
    .map_err(|e| ProtoError::Malformed(e.to_string()))?;
    if bytes.len() > MAX_FRAME {
        return Err(ProtoError::TooLarge(bytes.len()));
    }
    Ok(bytes)
}

/// Байты кадра → сообщение.
///
/// Разбор в два шага: сначала конверт (ровно поля `v` и `msg`, версия), потом
/// само сообщение. Так кадр новой версии даёт внятное
/// [`ProtoError::Version`], а не «неизвестный вариант» из глубины `msg`.
pub fn decode<M: DeserializeOwned>(bytes: &[u8]) -> Result<M, ProtoError> {
    if bytes.len() > MAX_FRAME {
        return Err(ProtoError::TooLarge(bytes.len()));
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| ProtoError::Malformed(e.to_string()))?;
    let serde_json::Value::Object(mut obj) = value else {
        return Err(ProtoError::Malformed("кадр не объект".into()));
    };
    let version = obj.get("v").and_then(serde_json::Value::as_u64);
    if version != Some(u64::from(PROTO_VERSION)) {
        return Err(ProtoError::Version(version));
    }
    if let Some(extra) = obj.keys().find(|k| *k != "v" && *k != "msg") {
        return Err(ProtoError::Malformed(format!(
            "лишнее поле конверта `{extra}`"
        )));
    }
    let msg = obj
        .remove("msg")
        .ok_or_else(|| ProtoError::Malformed("нет поля `msg`".into()))?;
    serde_json::from_value(msg).map_err(|e| ProtoError::Malformed(e.to_string()))
}

// ============================================================================
// Сокет
// ============================================================================

/// Пара `SOCK_SEQPACKET` с `SOCK_CLOEXEC`: ни один из концов не переживёт
/// `exec` у потомков окна (З3). Конец помощника клиент сам ставит на fd 0/1
/// дочки через `dup2` — у копии CLOEXEC снят, у оригинала остаётся.
pub fn seqpacket_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: `fds` — массив из двух int, ровно то, что ждёт socketpair.
    let rc = unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            0,
            fds.as_mut_ptr(),
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: socketpair вернул два новых дескриптора, владеем ими только мы.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Записать один кадр. `MSG_NOSIGNAL`: закрытый собеседник — `EPIPE`
/// ошибкой, а не SIGPIPE, роняющий процесс.
pub fn send_frame(fd: BorrowedFd<'_>, frame: &[u8]) -> Result<(), ProtoError> {
    if frame.len() > MAX_FRAME {
        return Err(ProtoError::TooLarge(frame.len()));
    }
    loop {
        // SAFETY: указатель и длина — от живого среза.
        let n = unsafe {
            libc::send(
                fd.as_raw_fd(),
                frame.as_ptr().cast(),
                frame.len(),
                libc::MSG_NOSIGNAL,
            )
        };
        if n >= 0 {
            // SEQPACKET пишет кадр целиком или не пишет вовсе.
            return if n as usize == frame.len() {
                Ok(())
            } else {
                Err(ProtoError::Io(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "кадр записан не целиком",
                )))
            };
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err.into());
        }
    }
}

/// Прочитать один кадр. `Ok(None)` — собеседник закрыл свой конец (EOF).
///
/// `MSG_TRUNC` заставляет ядро вернуть настоящую длину кадра, даже если он
/// не влез в буфер: слишком большой кадр опознаётся, а не режется молча до
/// «правдоподобного» JSON. Пустых кадров протокол не шлёт, поэтому длина 0
/// всегда читается как EOF.
pub fn recv_frame(fd: BorrowedFd<'_>) -> Result<Option<Vec<u8>>, ProtoError> {
    let mut buf = vec![0u8; MAX_FRAME];
    loop {
        // SAFETY: указатель и длина — от живого буфера.
        let n = unsafe {
            libc::recv(
                fd.as_raw_fd(),
                buf.as_mut_ptr().cast(),
                buf.len(),
                libc::MSG_TRUNC,
            )
        };
        if n >= 0 {
            let n = n as usize;
            if n == 0 {
                return Ok(None);
            }
            if n > MAX_FRAME {
                return Err(ProtoError::TooLarge(n));
            }
            buf.truncate(n);
            return Ok(Some(buf));
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err.into());
        }
    }
}

/// [`encode`] + [`send_frame`].
pub fn send_msg<M: Serialize>(fd: BorrowedFd<'_>, msg: &M) -> Result<(), ProtoError> {
    send_frame(fd, &encode(msg)?)
}

/// [`recv_frame`] + [`decode`]. `Ok(None)` — EOF.
pub fn recv_msg<M: DeserializeOwned>(fd: BorrowedFd<'_>) -> Result<Option<M>, ProtoError> {
    match recv_frame(fd)? {
        Some(frame) => decode(&frame).map(Some),
        None => Ok(None),
    }
}

// ============================================================================
// Кадр с дескрипторами (SCM_RIGHTS)
// ============================================================================
//
// Файлы пользователя помощник получает ТОЛЬКО открытыми дескрипторами (З2):
// окно открывает файл само, от своего имени, и передаёт fd вместе с кадром
// операции; путь помощник не открывает никогда. Номер fd в операции —
// индекс в списке этого кадра (`op::LocalFile::slot`).

/// Место под служебные данные на `n` дескрипторов (`CMSG_SPACE`), в словах
/// `u64`: буфер cmsg обязан быть выровнен как `cmsghdr`.
fn cmsg_space_words(n: usize) -> usize {
    // SAFETY: CMSG_SPACE — чистая арифметика над длиной.
    let bytes = unsafe { libc::CMSG_SPACE((n * std::mem::size_of::<libc::c_int>()) as u32) };
    (bytes as usize).div_ceil(std::mem::size_of::<u64>())
}

/// Записать кадр вместе с дескрипторами `fds` (не больше [`MAX_FDS`]).
/// Пустой `fds` — то же, что [`send_frame`]. Свои копии fd отправитель
/// держит и закрывает сам: ядро передаёт собеседнику дубликаты.
pub fn send_frame_with_fds(
    fd: BorrowedFd<'_>,
    frame: &[u8],
    fds: &[BorrowedFd<'_>],
) -> Result<(), ProtoError> {
    if fds.is_empty() {
        return send_frame(fd, frame);
    }
    if fds.len() > MAX_FDS {
        return Err(ProtoError::Fds(format!("{} дескрипторов больше предела {MAX_FDS}", fds.len())));
    }
    if frame.is_empty() {
        // Пустой кадр собеседник прочтёт как EOF, а fd с ним потеряются.
        return Err(ProtoError::Malformed("пустой кадр с дескрипторами".into()));
    }
    if frame.len() > MAX_FRAME {
        return Err(ProtoError::TooLarge(frame.len()));
    }
    let raw: Vec<libc::c_int> = fds.iter().map(|f| f.as_raw_fd()).collect();
    let mut control = vec![0u64; cmsg_space_words(raw.len())];
    let mut iov = libc::iovec {
        iov_base: frame.as_ptr() as *mut libc::c_void,
        iov_len: frame.len(),
    };
    // SAFETY: msghdr — простая C-структура, нули в ней допустимы.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    // SAFETY: CMSG_SPACE — арифметика.
    msg.msg_controllen =
        unsafe { libc::CMSG_SPACE(std::mem::size_of_val(raw.as_slice()) as u32) } as _;
    // SAFETY: буфер control выделен под CMSG_SPACE(n) и выровнен; первый
    // заголовок существует, данные пишутся ровно в его границах.
    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(raw.as_slice()) as u32) as _;
        std::ptr::copy_nonoverlapping(
            raw.as_ptr().cast::<u8>(),
            libc::CMSG_DATA(cmsg),
            std::mem::size_of_val(raw.as_slice()),
        );
    }
    loop {
        // SAFETY: msg указывает на живые iov и control.
        let n = unsafe { libc::sendmsg(fd.as_raw_fd(), &msg, libc::MSG_NOSIGNAL) };
        if n >= 0 {
            return if n as usize == frame.len() {
                Ok(())
            } else {
                Err(ProtoError::Io(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "кадр записан не целиком",
                )))
            };
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err.into());
        }
    }
}

/// Прочитать кадр вместе с дескрипторами. `Ok(None)` — EOF.
///
/// * `MSG_CMSG_CLOEXEC`: полученные fd не переживут `exec` программ,
///   которые помощник запускает (pacman, cp) — ни одна из них не должна
///   получить файл окна в наследство;
/// * каждый полученный fd СРАЗУ становится [`OwnedFd`] — до любой проверки,
///   поэтому на любой ошибке ниже (обрезка, лишние, чужой вид cmsg, кадр
///   больше предела, EOF) все они закрываются сами;
/// * `MSG_CTRUNC` (служебные данные не влезли: собеседник прислал больше
///   [`MAX_FDS`]) — ошибка: часть fd ядро уже выбросило, кадр неполон;
/// * служебные данные не `SCM_RIGHTS` (например, `SCM_CREDENTIALS`) — ошибка.
pub fn recv_frame_with_fds(
    fd: BorrowedFd<'_>,
) -> Result<Option<(Vec<u8>, Vec<OwnedFd>)>, ProtoError> {
    let mut buf = vec![0u8; MAX_FRAME];
    let mut control = vec![0u64; cmsg_space_words(MAX_FDS)];
    loop {
        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        };
        // SAFETY: msghdr — простая C-структура, нули в ней допустимы.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = control.as_mut_ptr().cast();
        msg.msg_controllen = std::mem::size_of_val(control.as_slice()) as _;
        // SAFETY: msg указывает на живые buf и control.
        let n = unsafe {
            libc::recvmsg(
                fd.as_raw_fd(),
                &mut msg,
                libc::MSG_TRUNC | libc::MSG_CMSG_CLOEXEC,
            )
        };
        if n < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err.into());
        }
        // Сначала забрать во владение всё, что пришло, — потом проверять.
        let mut owned: Vec<OwnedFd> = Vec::new();
        let mut foreign = false;
        // SAFETY: обход заголовков макросами ядра в границах msg_controllen,
        // который recvmsg выставил по факту; данные SCM_RIGHTS — массив int.
        unsafe {
            let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
            while !cmsg.is_null() {
                if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                    let data = libc::CMSG_DATA(cmsg);
                    let len = (*cmsg).cmsg_len as usize - (data as usize - cmsg as usize);
                    let count = len / std::mem::size_of::<libc::c_int>();
                    for i in 0..count {
                        let raw = std::ptr::read_unaligned(
                            data.cast::<libc::c_int>().add(i),
                        );
                        if raw >= 0 {
                            owned.push(OwnedFd::from_raw_fd(raw));
                        }
                    }
                } else {
                    foreign = true;
                }
                cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
            }
        }
        if msg.msg_flags & libc::MSG_CTRUNC != 0 {
            return Err(ProtoError::Fds("служебные данные обрезаны (MSG_CTRUNC)".into()));
        }
        if foreign {
            return Err(ProtoError::Fds("служебные данные не SCM_RIGHTS".into()));
        }
        if owned.len() > MAX_FDS {
            return Err(ProtoError::Fds(format!(
                "{} дескрипторов больше предела {MAX_FDS}",
                owned.len()
            )));
        }
        let n = n as usize;
        if n == 0 {
            return Ok(None);
        }
        if n > MAX_FRAME {
            return Err(ProtoError::TooLarge(n));
        }
        buf.truncate(n);
        return Ok(Some((buf, owned)));
    }
}

// ============================================================================
// Сверки
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsFd;

    /// Заглушка операции до Б1 (`privilege::op`).
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case", deny_unknown_fields)]
    enum TestOp {
        Install { packages: Vec<String> },
        Reboot,
    }

    type Client = ClientMsg<TestOp>;

    fn all_client() -> Vec<Client> {
        vec![
            ClientMsg::Hello(Hello {
                build_id: build_id(),
            }),
            ClientMsg::Request {
                id: 1,
                op: TestOp::Install {
                    packages: vec!["plasma-desktop".into()],
                },
            },
            ClientMsg::Request {
                id: 2,
                op: TestOp::Reboot,
            },
            ClientMsg::Cancel { id: 1 },
        ]
    }

    fn all_helper() -> Vec<HelperMsg> {
        let mut v = vec![
            HelperMsg::Welcome {
                build_id: build_id(),
            },
            HelperMsg::Progress {
                id: 1,
                line: "(1/3) установка «пакета»…".into(),
            },
            HelperMsg::Done {
                id: 1,
                ok: true,
                code: Some(0),
                message_key: None,
            },
            HelperMsg::Done {
                id: 2,
                ok: false,
                code: None,
                message_key: Some("priv.killed".into()),
            },
            HelperMsg::Busy { id: 3 },
        ];
        for code in [
            ErrorCode::Refused,
            ErrorCode::Revoked,
            ErrorCode::BuildMismatch,
            ErrorCode::Sequence,
            ErrorCode::Cancelled,
            ErrorCode::Internal,
        ] {
            v.push(HelperMsg::Error {
                id: Some(4),
                code,
                message_key: "k".into(),
            });
            v.push(HelperMsg::Error {
                id: None,
                code,
                message_key: "k".into(),
            });
        }
        v
    }

    #[test]
    fn round_trip_all_messages() {
        for m in all_client() {
            let back: Client = decode(&encode(&m).unwrap()).unwrap();
            assert_eq!(back, m);
        }
        for m in all_helper() {
            let back: HelperMsg = decode(&encode(&m).unwrap()).unwrap();
            assert_eq!(back, m);
        }
    }

    #[test]
    fn frame_shape_is_stable() {
        let bytes = encode(&Client::Cancel { id: 7 }).unwrap();
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            r#"{"v":1,"msg":{"cancel":{"id":7}}}"#
        );
        let bytes = encode(&HelperMsg::Error {
            id: None,
            code: ErrorCode::BuildMismatch,
            message_key: "k".into(),
        })
        .unwrap();
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            r#"{"v":1,"msg":{"error":{"id":null,"code":"build_mismatch","message_key":"k"}}}"#
        );
    }

    #[test]
    fn too_large_rejected_both_ways() {
        let big = HelperMsg::Progress {
            id: 1,
            line: "x".repeat(MAX_FRAME),
        };
        assert!(matches!(encode(&big), Err(ProtoError::TooLarge(_))));
        let raw = vec![b' '; MAX_FRAME + 1];
        assert!(matches!(
            decode::<HelperMsg>(&raw),
            Err(ProtoError::TooLarge(_))
        ));
        assert!(matches!(
            send_frame_dummy(&raw),
            Err(ProtoError::TooLarge(_))
        ));
    }

    fn send_frame_dummy(frame: &[u8]) -> Result<(), ProtoError> {
        let (a, _b) = seqpacket_pair().unwrap();
        send_frame(a.as_fd(), frame)
    }

    #[test]
    fn unknown_fields_rejected() {
        let cases = [
            // в конверте
            r#"{"v":1,"msg":{"cancel":{"id":1}},"x":1}"#,
            // в сообщении
            r#"{"v":1,"msg":{"cancel":{"id":1,"argv":["sh"]}}}"#,
            // в Hello (отдельная структура)
            r#"{"v":1,"msg":{"hello":{"build_id":"a","uid":0}}}"#,
            // в операции
            r#"{"v":1,"msg":{"request":{"id":1,"op":{"install":{"packages":[],"flags":"-oX"}}}}}"#,
            // в запросе рядом с операцией
            r#"{"v":1,"msg":{"request":{"id":1,"op":"reboot","shell":"x"}}}"#,
        ];
        for c in cases {
            let r = decode::<Client>(c.as_bytes());
            assert!(matches!(r, Err(ProtoError::Malformed(_))), "{c}: {r:?}");
        }
        let r = decode::<HelperMsg>(br#"{"v":1,"msg":{"busy":{"id":1,"pid":5}}}"#);
        assert!(matches!(r, Err(ProtoError::Malformed(_))));
    }

    #[test]
    fn missing_fields_and_unknown_variants_rejected() {
        let cases = [
            r#"{"v":1}"#,
            r#"{"v":1,"msg":{"cancel":{}}}"#,
            r#"{"v":1,"msg":{"exec":{"argv":["id"]}}}"#,
            r#"{"v":1,"msg":{"request":{"id":1,"op":"rm_rf"}}}"#,
            r#"{"v":1,"msg":{"request":{"id":-1,"op":"reboot"}}}"#,
        ];
        for c in cases {
            let r = decode::<Client>(c.as_bytes());
            assert!(matches!(r, Err(ProtoError::Malformed(_))), "{c}: {r:?}");
        }
        let r = decode::<HelperMsg>(
            br#"{"v":1,"msg":{"error":{"id":null,"code":"oops","message_key":"k"}}}"#,
        );
        assert!(matches!(r, Err(ProtoError::Malformed(_))));
    }

    #[test]
    fn garbage_and_cut_frames_rejected() {
        let good = encode(&Client::Request {
            id: 9,
            op: TestOp::Reboot,
        })
        .unwrap();
        for cut in 1..good.len() {
            let r = decode::<Client>(&good[..cut]);
            assert!(r.is_err(), "обрыв на {cut} байте принят: {r:?}");
        }
        for junk in [
            &b""[..],
            b"\0\0\0",
            b"\xff\xfe{",
            b"[1,2]",
            b"\"hello\"",
            b"null",
        ] {
            assert!(decode::<Client>(junk).is_err(), "{junk:?}");
        }
    }

    #[test]
    fn version_checked() {
        for (c, want) in [
            (r#"{"v":2,"msg":{"cancel":{"id":1}}}"#, Some(2)),
            (r#"{"v":0,"msg":{"cancel":{"id":1}}}"#, Some(0)),
            (r#"{"msg":{"cancel":{"id":1}}}"#, None),
            (r#"{"v":"1","msg":{"cancel":{"id":1}}}"#, None),
            // новая версия с незнакомым сообщением — всё равно «версия»
            (r#"{"v":2,"msg":{"something_new":{}}}"#, Some(2)),
        ] {
            match decode::<Client>(c.as_bytes()) {
                Err(ProtoError::Version(v)) => assert_eq!(v, want, "{c}"),
                other => panic!("{c}: {other:?}"),
            }
        }
    }

    #[test]
    fn hello_check_and_error_classes() {
        let own = build_id();
        assert!(!own.is_empty());
        assert_eq!(
            check_hello(
                &Hello {
                    build_id: own.clone()
                },
                &own
            ),
            Ok(())
        );
        assert_eq!(
            check_hello(
                &Hello {
                    build_id: "0.0.1+deadbeef".into()
                },
                &own
            ),
            Err(ErrorCode::BuildMismatch)
        );
        // З5: отказ проверки и сбой — разные классы, отмена — своя.
        assert_eq!(ErrorCode::Refused.class(), ErrorClass::Refusal);
        assert_eq!(ErrorCode::Revoked.class(), ErrorClass::Refusal);
        assert_eq!(ErrorCode::BuildMismatch.class(), ErrorClass::Refusal);
        assert_eq!(ErrorCode::Sequence.class(), ErrorClass::Refusal);
        assert_eq!(ErrorCode::Cancelled.class(), ErrorClass::Cancelled);
        assert_eq!(ErrorCode::Internal.class(), ErrorClass::Failure);
    }

    #[test]
    fn socketpair_is_cloexec_seqpacket() {
        let (a, b) = seqpacket_pair().unwrap();
        for fd in [&a, &b] {
            // SAFETY: fcntl/getsockopt на живых дескрипторах.
            let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
            assert!(flags & libc::FD_CLOEXEC != 0);
            let mut ty: libc::c_int = 0;
            let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
            let rc = unsafe {
                libc::getsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_TYPE,
                    (&mut ty as *mut libc::c_int).cast(),
                    &mut len,
                )
            };
            assert_eq!(rc, 0);
            assert_eq!(ty, libc::SOCK_SEQPACKET);
        }
    }

    /// Живой обмен по socketpair в двух потоках: рукопожатие, запрос, ход,
    /// итог, затем EOF у помощника, когда окно роняет свой конец.
    #[test]
    fn socketpair_dialogue_two_threads() {
        let (win, helper) = seqpacket_pair().unwrap();
        let h = std::thread::spawn(move || {
            let fd = helper.as_fd();
            let Some(Client::Hello(hello)) = recv_msg::<Client>(fd).unwrap() else {
                panic!("первым должен прийти Hello");
            };
            check_hello(&hello, &build_id()).unwrap();
            send_msg(
                fd,
                &HelperMsg::Welcome {
                    build_id: build_id(),
                },
            )
            .unwrap();
            let mut seen = Vec::new();
            while let Some(m) = recv_msg::<Client>(fd).unwrap() {
                if let Client::Request { id, .. } = &m {
                    send_msg(
                        fd,
                        &HelperMsg::Progress {
                            id: *id,
                            line: "шаг".into(),
                        },
                    )
                    .unwrap();
                    send_msg(
                        fd,
                        &HelperMsg::Done {
                            id: *id,
                            ok: true,
                            code: Some(0),
                            message_key: None,
                        },
                    )
                    .unwrap();
                }
                seen.push(m);
            }
            seen // EOF: окно закрыло свой конец
        });
        let fd = win.as_fd();
        send_msg(
            fd,
            &Client::Hello(Hello {
                build_id: build_id(),
            }),
        )
        .unwrap();
        assert_eq!(
            recv_msg::<HelperMsg>(fd).unwrap(),
            Some(HelperMsg::Welcome {
                build_id: build_id()
            })
        );
        let req = Client::Request {
            id: 5,
            op: TestOp::Install {
                packages: vec!["a".into()],
            },
        };
        send_msg(fd, &req).unwrap();
        assert_eq!(
            recv_msg::<HelperMsg>(fd).unwrap(),
            Some(HelperMsg::Progress {
                id: 5,
                line: "шаг".into()
            })
        );
        assert_eq!(
            recv_msg::<HelperMsg>(fd).unwrap(),
            Some(HelperMsg::Done {
                id: 5,
                ok: true,
                code: Some(0),
                message_key: None
            })
        );
        drop(win);
        assert_eq!(h.join().unwrap(), vec![req]);
    }

    /// Слишком большой и порченый кадры, пришедшие по настоящему сокету:
    /// длина опознаётся по `MSG_TRUNC`, а не по обрезку; после кадра в
    /// 64 КБ + 1 следующий нормальный кадр читается отдельно (границы целы).
    #[test]
    fn socketpair_oversize_and_garbage() {
        let (a, b) = seqpacket_pair().unwrap();
        let writer = std::thread::spawn(move || {
            let fd = a.as_fd();
            let huge = vec![b'{'; MAX_FRAME + 1];
            // SAFETY: в обход send_frame — имитируем чужого писателя.
            let n = unsafe {
                libc::send(
                    fd.as_raw_fd(),
                    huge.as_ptr().cast(),
                    huge.len(),
                    libc::MSG_NOSIGNAL,
                )
            };
            assert_eq!(n as usize, huge.len());
            send_frame(fd, b"{\"v\":1,\"msg\":{\"cancel\":{\"id\":1}").unwrap(); // обрыв
            send_frame(fd, br#"{"v":1,"msg":{"cancel":{"id":1}},"evil":true}"#).unwrap();
            send_msg(fd, &Client::Cancel { id: 2 }).unwrap();
            a // держим конец открытым до конца чтения
        });
        let fd = b.as_fd();
        match recv_msg::<Client>(fd) {
            Err(ProtoError::TooLarge(n)) => assert_eq!(n, MAX_FRAME + 1),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            recv_msg::<Client>(fd),
            Err(ProtoError::Malformed(_))
        ));
        assert!(matches!(
            recv_msg::<Client>(fd),
            Err(ProtoError::Malformed(_))
        ));
        assert_eq!(
            recv_msg::<Client>(fd).unwrap(),
            Some(Client::Cancel { id: 2 })
        );
        let a = writer.join().unwrap();
        drop(a);
        assert_eq!(recv_msg::<Client>(fd).unwrap(), None);
    }

    /// Запись в сокет, чей собеседник ушёл, — ошибка, а не SIGPIPE.
    #[test]
    fn send_to_closed_peer_is_error() {
        let (a, b) = seqpacket_pair().unwrap();
        drop(b);
        assert!(matches!(
            send_msg(a.as_fd(), &Client::Cancel { id: 1 }),
            Err(ProtoError::Io(_))
        ));
    }

    // ---- кадр с дескрипторами ----

    fn pipe_cloexec() -> (OwnedFd, OwnedFd) {
        let mut fds = [0; 2];
        // SAFETY: массив из двух int под pipe2.
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) }, 0);
        // SAFETY: pipe2 вернул два новых fd.
        unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) }
    }

    fn is_cloexec(fd: BorrowedFd<'_>) -> bool {
        // SAFETY: F_GETFD на живом fd.
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
        flags >= 0 && flags & libc::FD_CLOEXEC != 0
    }

    /// Ждать EOF на читающем конце канала: он наступает, только когда
    /// закрыты ВСЕ копии пишущего конца — так проверяется, что помощник
    /// не оставил у себя ни одной.
    fn pipe_sees_eof(r: BorrowedFd<'_>) -> bool {
        let mut pfd = libc::pollfd { fd: r.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: один pollfd.
        let rc = unsafe { libc::poll(&mut pfd, 1, 5000) };
        if rc <= 0 {
            return false;
        }
        let mut b = [0u8; 1];
        // SAFETY: буфер из одного байта.
        unsafe { libc::read(r.as_raw_fd(), b.as_mut_ptr().cast(), 1) == 0 }
    }

    /// Сырой sendmsg с любым числом fd — чтобы прислать больше MAX_FDS в
    /// обход проверки отправителя (как сделал бы чужой собеседник).
    fn raw_send_fds(sock: BorrowedFd<'_>, frame: &[u8], fds: &[libc::c_int]) {
        let mut control = vec![0u64; cmsg_space_words(fds.len())];
        let mut iov = libc::iovec { iov_base: frame.as_ptr() as *mut _, iov_len: frame.len() };
        // SAFETY: как в send_frame_with_fds.
        unsafe {
            let mut msg: libc::msghdr = std::mem::zeroed();
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = control.as_mut_ptr().cast();
            msg.msg_controllen = libc::CMSG_SPACE(std::mem::size_of_val(fds) as u32) as _;
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(fds) as u32) as _;
            std::ptr::copy_nonoverlapping(fds.as_ptr().cast::<u8>(), libc::CMSG_DATA(cmsg), std::mem::size_of_val(fds));
            assert_eq!(libc::sendmsg(sock.as_raw_fd(), &msg, libc::MSG_NOSIGNAL), frame.len() as isize);
        }
    }

    /// Окно в одном потоке шлёт кадр с файлом и концом канала, помощник в
    /// другом получает: кадр цел, fd рабочие и с CLOEXEC, порядок сохранён.
    #[test]
    fn fds_two_threads_cloexec_and_order() {
        let (a, b) = seqpacket_pair().unwrap();
        let (pr, pw) = pipe_cloexec();
        let dir = std::env::temp_dir().join(format!("rexon-proto-fds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f");
        std::fs::write(&path, b"theme-bytes").unwrap();
        let file = std::fs::File::open(&path).unwrap();
        let frame = encode(&Client::Cancel { id: 7 }).unwrap();
        let writer = std::thread::spawn(move || {
            send_frame_with_fds(a.as_fd(), &frame, &[file.as_fd(), pw.as_fd()]).unwrap();
            // Свои копии отправитель закрывает — у помощника остаются его.
            drop(file);
            drop(pw);
            a
        });
        let (got, fds) = recv_frame_with_fds(b.as_fd()).unwrap().unwrap();
        assert_eq!(decode::<Client>(&got).unwrap(), Client::Cancel { id: 7 });
        assert_eq!(fds.len(), 2);
        assert!(fds.iter().all(|f| is_cloexec(f.as_fd())), "полученные fd обязаны быть CLOEXEC");
        let mut f = std::fs::File::from(fds.into_iter().next().unwrap());
        let mut s = String::new();
        std::io::Read::read_to_string(&mut f, &mut s).unwrap();
        assert_eq!(s, "theme-bytes");
        let _a = writer.join().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        // Второй полученный fd (pw) уже закрыт вместе с вектором — EOF.
        assert!(pipe_sees_eof(pr.as_fd()));
    }

    /// Старые и новые функции понимают друг друга: кадр без fd читается
    /// recv_frame_with_fds с пустым списком, пустой список — обычный send.
    #[test]
    fn fds_compatible_with_plain_frames() {
        let (a, b) = seqpacket_pair().unwrap();
        send_msg(a.as_fd(), &Client::Cancel { id: 1 }).unwrap();
        let (frame, fds) = recv_frame_with_fds(b.as_fd()).unwrap().unwrap();
        assert!(fds.is_empty());
        assert_eq!(decode::<Client>(&frame).unwrap(), Client::Cancel { id: 1 });
        send_frame_with_fds(a.as_fd(), &encode(&Client::Cancel { id: 2 }).unwrap(), &[]).unwrap();
        assert_eq!(recv_msg::<Client>(b.as_fd()).unwrap(), Some(Client::Cancel { id: 2 }));
        drop(a);
        assert!(recv_frame_with_fds(b.as_fd()).unwrap().is_none());
    }

    /// Отправитель не шлёт больше MAX_FDS и пустой кадр с fd.
    #[test]
    fn fds_sender_limits() {
        let (a, _b) = seqpacket_pair().unwrap();
        let (_r, w) = pipe_cloexec();
        let many: Vec<BorrowedFd<'_>> = (0..=MAX_FDS).map(|_| w.as_fd()).collect();
        assert!(matches!(send_frame_with_fds(a.as_fd(), b"{}", &many), Err(ProtoError::Fds(_))));
        assert!(matches!(send_frame_with_fds(a.as_fd(), b"", &[w.as_fd()]), Err(ProtoError::Malformed(_))));
        let big = vec![b' '; MAX_FRAME + 1];
        assert!(matches!(send_frame_with_fds(a.as_fd(), &big, &[w.as_fd()]), Err(ProtoError::TooLarge(_))));
    }

    /// Чужой собеседник шлёт MAX_FDS+2 дескриптора: ядро режет служебные
    /// данные (MSG_CTRUNC), приём — ошибка, и НИ ОДИН fd у получателя не
    /// остаётся (канал видит EOF, когда отправитель закрыл свои копии).
    #[test]
    fn fds_ctrunc_rejected_and_all_closed() {
        let (a, b) = seqpacket_pair().unwrap();
        let (pr, pw) = pipe_cloexec();
        let writer = std::thread::spawn(move || {
            let raws = vec![pw.as_raw_fd(); MAX_FDS + 2];
            raw_send_fds(a.as_fd(), b"{\"v\":1}", &raws);
            drop(pw);
            a
        });
        let r = recv_frame_with_fds(b.as_fd());
        assert!(matches!(r, Err(ProtoError::Fds(ref e)) if e.contains("MSG_CTRUNC")), "{r:?}");
        let _a = writer.join().unwrap();
        assert!(pipe_sees_eof(pr.as_fd()), "полученные fd обязаны закрыться");
    }

    /// Кадр больше предела вместе с fd — ошибка, fd закрыты.
    #[test]
    fn fds_with_oversize_frame_closed() {
        let (a, b) = seqpacket_pair().unwrap();
        // Буфер отправки по умолчанию может быть меньше; расширим.
        let sz: libc::c_int = 4 * MAX_FRAME as libc::c_int;
        // SAFETY: setsockopt с int.
        unsafe {
            libc::setsockopt(a.as_raw_fd(), libc::SOL_SOCKET, libc::SO_SNDBUF, (&sz as *const libc::c_int).cast(), 4);
        }
        let (pr, pw) = pipe_cloexec();
        let big = vec![b' '; MAX_FRAME + 10];
        raw_send_fds(a.as_fd(), &big, &[pw.as_raw_fd()]);
        drop(pw);
        assert!(matches!(recv_frame_with_fds(b.as_fd()), Err(ProtoError::TooLarge(n)) if n == MAX_FRAME + 10));
        assert!(pipe_sees_eof(pr.as_fd()));
    }
}
