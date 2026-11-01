//! Экран входа Plasma Login Manager: фон и перенос оформления стола — тем же
//! путём, что у модуля «Параметров системы» `kcm_plasmalogin` (PLM 6.8).
//!
//! ЗАЧЕМ ТАК. Пишет в систему не Rexon, а собственный помощник PLM
//! (`kcmplasmalogin_authhelper`, KAuth, D-Bus-активация от root). Повторять
//! его запись самим нельзя: ровно на ней PLM получил CVE-2026-25710
//! (ссылки, гонки и смена владельца в доме `plasmalogin`), и следить за
//! миграциями прав PLM пришлось бы нам. Поэтому Rexon зовёт помощник так же,
//! как его зовёт KAuth из kcm (`DBusHelperProxy::executeAction`, KAuth 6.31):
//!
//! * системная шина, служба [`HELPER_ID`], объект `/`, интерфейс
//!   [`KAUTH_INTERFACE`], метод `performAction(s action, ay callerID,
//!   a{sv} details, ay arguments, a{sh} fdArguments) → ay`;
//! * `arguments` — `QVariantMap` в `QDataStream` (Qt 6, версия 24) без
//!   дескрипторов; дескрипторы — отдельно в `fdArguments` (KAuth сам
//!   возвращает их в карту под теми же ключами);
//! * `callerID` помощник не смотрит: он проверяет у polkit ОТПРАВИТЕЛЯ
//!   сообщения (`Polkit1Backend::isCallerAuthorized`, флаг
//!   `AllowUserInteraction`), поэтому окно пароля показывает сам polkit —
//!   агент сеанса пользователя, политика `auth_admin` PLM;
//! * ответ — `ActionReply::serialized()`: `QVariantMap data`, `qint32
//!   errorCode`, `quint32 type`, `QString errorDescription`.
//!
//! Что передаётся — ровно то, что передаёт kcm (`kcm.cpp` PLM 6.8.2):
//! * `save`: `config` — ВЕСЬ текст `/etc/plasmalogin.conf` (помощник
//!   перезаписывает файл целиком), `wallpapers` — имена картинок в
//!   `~plasmalogin/wallpapers`, `_fd_<имя>` — открытый на чтение файл
//!   картинки (каталог картинок помощник каждый раз очищает);
//! * `sync`: тексты файлов стола под их именами внутри `~/.config`;
//! * `reset`: без аргументов (помощник стирает `.config` и `.cache` дома
//!   `plasmalogin`).
//!
//! Намеренное отличие от kcm, как у SDDM: kcm собирает `config` из своих
//! полей и теряет чужие ключи файла; Rexon берёт текущий текст файла и
//! меняет в нём только ключи фона — остальное уходит помощнику как было.
//!
//! Логика — чистые функции на подставных фактах и подставной шине
//! ([`KauthBus`]); живые обёртки `*_live` — в конце файла.

use std::collections::HashMap;
use std::os::fd::OwnedFd;

use crate::i18n::tr;
use crate::system_plan::{
    greeter_clock_view, greeter_preview_view, login_image_filter, DesktopLook, SddmBackgroundView, SddmGreeterView,
    SddmLookView,
};

/// Служба помощника PLM на системной шине (она же `helperId` действий).
pub const HELPER_ID: &str = "org.kde.kcontrol.kcmplasmalogin";
/// Интерфейс помощника KAuth 6.
pub const KAUTH_INTERFACE: &str = "org.kde.kf6auth";
/// Действия помощника (`kcm_plasmalogin.actions`, политика `auth_admin`).
pub const ACTION_SAVE: &str = "org.kde.kcontrol.kcmplasmalogin.save";
pub const ACTION_SYNC: &str = "org.kde.kcontrol.kcmplasmalogin.sync";
pub const ACTION_RESET: &str = "org.kde.kcontrol.kcmplasmalogin.reset";
/// Файл активации помощника: нет его — звать некого.
pub const HELPER_SERVICE_FILE: &str = "/usr/share/dbus-1/system-services/org.kde.kcontrol.kcmplasmalogin.service";

/// Файл, который пишет `save` (`PLASMALOGIN_CONFIG_FILE`).
pub const CONFIG_FILE: &str = "/etc/plasmalogin.conf";
/// Каталоги и файл, которые PLM читает под ним (поздний перекрывает
/// ранний; сам [`CONFIG_FILE`] — сильнее всех): значения пакета, затем
/// администратора.
pub const SYSTEM_DEFAULTS_FILE: &str = "/usr/lib/plasmalogin/defaults.conf";
pub const SYSTEM_CONF_DIR: &str = "/usr/lib/plasmalogin/plasmalogin.conf.d";
pub const CONF_DIR: &str = "/etc/plasmalogin.conf.d";
/// Системная учётная запись приветствия PLM.
pub const PLM_USER: &str = "plasmalogin";
/// Предел текста `config` у помощника (больше — отказ).
pub const CONFIG_MAX_BYTES: usize = 1024 * 1024;
/// Самый большой файл стола, который уходит в `sync` (как у SDDM).
pub const LOOK_FILE_MAX_BYTES: u64 = 1024 * 1024;

/// Модуль обоев «Картинка» и группа его настроек в `[Greeter]`.
pub const IMAGE_PLUGIN: &str = "org.kde.image";
pub const GREETER_GROUP: &str = "Greeter";
pub const PLUGIN_KEY: &str = "WallpaperPluginId";
pub const IMAGE_GROUP: &str = "Greeter][Wallpaper][org.kde.image][General";
/// Имя своей картинки в `~plasmalogin/wallpapers` (+ `.png`/`.jpg`).
pub const BACKGROUND_STEM: &str = "rexon-background";

/// Файлы стола, которые kcm переносит в `~plasmalogin/.config` (ключи
/// `sync`). Других помощник не пишет.
pub const SYNC_FILES: &[&str] = &[
    "kdeglobals",
    "plasmarc",
    "plasma-localerc",
    "kcminputrc",
    "kwinoutputconfig.json",
    "fontconfig/fonts.conf",
    "kxkbrc",
];

// ── QDataStream (Qt 6) ───────────────────────────────────────────────────

/// Значение `QVariant` — те типы, что ходят через KAuth у PLM, и те, что
/// может вернуть `ActionReply::data`.
#[derive(Debug, Clone, PartialEq)]
pub enum QVariant {
    Invalid,
    Bool(bool),
    Int(i32),
    UInt(u32),
    LongLong(i64),
    ULongLong(u64),
    Double(f64),
    /// `None` — нулевая `QString`.
    String(Option<String>),
    StringList(Vec<String>),
    ByteArray(Option<Vec<u8>>),
    Map(Vec<(String, QVariant)>),
    List(Vec<QVariant>),
}

// Номера типов `QMetaType` (Qt 6).
const QMT_INVALID: u32 = 0;
const QMT_BOOL: u32 = 1;
const QMT_INT: u32 = 2;
const QMT_UINT: u32 = 3;
const QMT_LONGLONG: u32 = 4;
const QMT_ULONGLONG: u32 = 5;
const QMT_DOUBLE: u32 = 6;
const QMT_MAP: u32 = 8;
const QMT_LIST: u32 = 9;
const QMT_STRING: u32 = 10;
const QMT_STRINGLIST: u32 = 11;
const QMT_BYTEARRAY: u32 = 12;

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

/// `QString`: длина в байтах UTF-16 (`0xFFFFFFFF` — нулевая), затем UTF-16BE.
pub fn put_qstring(out: &mut Vec<u8>, s: Option<&str>) {
    let Some(s) = s else {
        put_u32(out, u32::MAX);
        return;
    };
    let units: Vec<u16> = s.encode_utf16().collect();
    put_u32(out, (units.len() * 2) as u32);
    for u in units {
        out.extend_from_slice(&u.to_be_bytes());
    }
}

fn put_qvariant(out: &mut Vec<u8>, v: &QVariant) {
    let (id, null) = match v {
        QVariant::Invalid => (QMT_INVALID, true),
        QVariant::Bool(_) => (QMT_BOOL, false),
        QVariant::Int(_) => (QMT_INT, false),
        QVariant::UInt(_) => (QMT_UINT, false),
        QVariant::LongLong(_) => (QMT_LONGLONG, false),
        QVariant::ULongLong(_) => (QMT_ULONGLONG, false),
        QVariant::Double(_) => (QMT_DOUBLE, false),
        QVariant::String(_) => (QMT_STRING, false),
        QVariant::StringList(_) => (QMT_STRINGLIST, false),
        QVariant::ByteArray(_) => (QMT_BYTEARRAY, false),
        QVariant::Map(_) => (QMT_MAP, false),
        QVariant::List(_) => (QMT_LIST, false),
    };
    put_u32(out, id);
    out.push(null as u8);
    match v {
        QVariant::Invalid => {}
        QVariant::Bool(b) => out.push(*b as u8),
        QVariant::Int(i) => out.extend_from_slice(&i.to_be_bytes()),
        QVariant::UInt(i) => out.extend_from_slice(&i.to_be_bytes()),
        QVariant::LongLong(i) => out.extend_from_slice(&i.to_be_bytes()),
        QVariant::ULongLong(i) => out.extend_from_slice(&i.to_be_bytes()),
        QVariant::Double(d) => out.extend_from_slice(&d.to_be_bytes()),
        QVariant::String(s) => put_qstring(out, s.as_deref()),
        QVariant::StringList(list) => {
            put_u32(out, list.len() as u32);
            for s in list {
                put_qstring(out, Some(s));
            }
        }
        QVariant::ByteArray(b) => match b {
            None => put_u32(out, u32::MAX),
            Some(b) => {
                put_u32(out, b.len() as u32);
                out.extend_from_slice(b);
            }
        },
        QVariant::Map(m) => put_qvariant_map(out, m),
        QVariant::List(list) => {
            put_u32(out, list.len() as u32);
            for v in list {
                put_qvariant(out, v);
            }
        }
    }
}

/// `QVariantMap` так, как его пишет `QDataStream << QMap`: число пар, затем
/// пары по возрастанию ключа (порядок `QString` — по единицам UTF-16).
/// Повтор ключа — остаётся последнее значение, как у `QMap::insert`.
pub fn put_qvariant_map(out: &mut Vec<u8>, map: &[(String, QVariant)]) {
    let mut entries: Vec<(&String, &QVariant)> = Vec::with_capacity(map.len());
    for (k, v) in map {
        match entries.iter_mut().find(|(ek, _)| *ek == k) {
            Some(e) => e.1 = v,
            None => entries.push((k, v)),
        }
    }
    entries.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
    put_u32(out, entries.len() as u32);
    for (k, v) in entries {
        put_qstring(out, Some(k));
        put_qvariant(out, v);
    }
}

/// Байты `QDataStream << QVariantMap`.
pub fn encode_qvariant_map(map: &[(String, QVariant)]) -> Vec<u8> {
    let mut out = Vec::new();
    put_qvariant_map(&mut out, map);
    out
}

/// Чтение `QDataStream`: позиция в байтах.
struct QReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl QReader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.pos.checked_add(n).filter(|e| *e <= self.data.len()).ok_or("обрыв данных")?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn qstring(&mut self) -> Result<Option<String>, String> {
        let len = self.u32()?;
        if len == u32::MAX {
            return Ok(None);
        }
        if len % 2 != 0 {
            return Err("нечётная длина QString".into());
        }
        let raw = self.take(len as usize)?;
        let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        Ok(Some(String::from_utf16_lossy(&units)))
    }
    fn count(&mut self) -> Result<usize, String> {
        let n = self.u32()? as usize;
        // Каждый элемент — не меньше 4 байт: защита от безумной длины.
        if n > (self.data.len() - self.pos) / 4 + 1 {
            return Err("слишком длинный список".into());
        }
        Ok(n)
    }
    fn qvariant(&mut self, depth: u32) -> Result<QVariant, String> {
        if depth > 16 {
            return Err("слишком глубокая вложенность".into());
        }
        let id = self.u32()?;
        let _null = self.u8()?;
        Ok(match id {
            QMT_INVALID => QVariant::Invalid,
            QMT_BOOL => QVariant::Bool(self.u8()? != 0),
            QMT_INT => QVariant::Int(self.u32()? as i32),
            QMT_UINT => QVariant::UInt(self.u32()?),
            QMT_LONGLONG => QVariant::LongLong(self.u64()? as i64),
            QMT_ULONGLONG => QVariant::ULongLong(self.u64()?),
            QMT_DOUBLE => QVariant::Double(f64::from_bits(self.u64()?)),
            QMT_STRING => QVariant::String(self.qstring()?),
            QMT_STRINGLIST => {
                let n = self.count()?;
                let mut list = Vec::with_capacity(n);
                for _ in 0..n {
                    list.push(self.qstring()?.unwrap_or_default());
                }
                QVariant::StringList(list)
            }
            QMT_BYTEARRAY => {
                let len = self.u32()?;
                if len == u32::MAX {
                    QVariant::ByteArray(None)
                } else {
                    QVariant::ByteArray(Some(self.take(len as usize)?.to_vec()))
                }
            }
            QMT_MAP => QVariant::Map(self.map(depth + 1)?),
            QMT_LIST => {
                let n = self.count()?;
                let mut list = Vec::with_capacity(n);
                for _ in 0..n {
                    list.push(self.qvariant(depth + 1)?);
                }
                QVariant::List(list)
            }
            other => return Err(format!("тип QVariant {other} не поддержан")),
        })
    }
    fn map(&mut self, depth: u32) -> Result<Vec<(String, QVariant)>, String> {
        let n = self.count()?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let k = self.qstring()?.unwrap_or_default();
            let v = self.qvariant(depth)?;
            out.push((k, v));
        }
        Ok(out)
    }
}

/// Разбор `QDataStream >> QVariantMap` (весь буфер).
pub fn decode_qvariant_map(data: &[u8]) -> Result<Vec<(String, QVariant)>, String> {
    let mut r = QReader { data, pos: 0 };
    let m = r.map(0)?;
    if r.pos != data.len() {
        return Err("лишние байты после QVariantMap".into());
    }
    Ok(m)
}

// ── Ответ помощника (KAuth::ActionReply) ─────────────────────────────────

/// Вид ответа (`ActionReply::Type`).
pub const REPLY_KAUTH_ERROR: u32 = 0;
pub const REPLY_HELPER_ERROR: u32 = 1;
pub const REPLY_SUCCESS: u32 = 2;
/// Коды ошибок KAuth (`ActionReply::Error`).
pub const ERR_NO_RESPONDER: i32 = 1;
pub const ERR_NO_SUCH_ACTION: i32 = 2;
pub const ERR_INVALID_ACTION: i32 = 3;
pub const ERR_AUTHORIZATION_DENIED: i32 = 4;
pub const ERR_USER_CANCELLED: i32 = 5;
pub const ERR_HELPER_BUSY: i32 = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct KauthReply {
    pub data: Vec<(String, QVariant)>,
    pub code: i32,
    pub kind: u32,
    pub description: String,
}

/// Разбор `ActionReply::serialized()`.
pub fn decode_reply(bytes: &[u8]) -> Result<KauthReply, String> {
    let mut r = QReader { data: bytes, pos: 0 };
    let data = r.map(0)?;
    let code = r.u32()? as i32;
    let kind = r.u32()?;
    let description = r.qstring()?.unwrap_or_default();
    if r.pos != bytes.len() {
        return Err("лишние байты после ActionReply".into());
    }
    Ok(KauthReply { data, code, kind, description })
}

/// Байты `ActionReply::serialized()` (для подставной шины и эталонов).
pub fn encode_reply(reply: &KauthReply) -> Vec<u8> {
    let mut out = encode_qvariant_map(&reply.data);
    out.extend_from_slice(&reply.code.to_be_bytes());
    put_u32(&mut out, reply.kind);
    put_qstring(&mut out, (!reply.description.is_empty()).then_some(reply.description.as_str()));
    out
}

/// Итог ответа для человека: `Ok(())` — помощник сделал; иначе готовый текст.
pub fn reply_outcome(reply: &KauthReply) -> Result<(), String> {
    if reply.kind == REPLY_SUCCESS {
        return Ok(());
    }
    let why = reply.description.trim();
    if reply.kind == REPLY_HELPER_ERROR {
        let mut text = tr("Plasma Login Manager's helper couldn't make the change.");
        if !why.is_empty() {
            text.push('\n');
            text.push_str(why);
        }
        return Err(text);
    }
    Err(match reply.code {
        ERR_AUTHORIZATION_DENIED | ERR_USER_CANCELLED => {
            tr("The password wasn't entered, so the login screen stays as it was.")
        }
        ERR_HELPER_BUSY => tr("Plasma Login Manager's helper is busy with another change. Try again in a moment."),
        ERR_NO_RESPONDER | ERR_NO_SUCH_ACTION | ERR_INVALID_ACTION => {
            tr("This version of Plasma Login Manager doesn't support this change. Use its own settings instead.")
        }
        _ => {
            let mut text = tr("Plasma Login Manager's helper refused the change.");
            if !why.is_empty() {
                text.push('\n');
                text.push_str(why);
            }
            text
        }
    })
}

// ── Шина ─────────────────────────────────────────────────────────────────

/// Один вызов `performAction`: действие, `QVariantMap` без дескрипторов
/// (уже в байтах `QDataStream`) и дескрипторы под своими ключами.
#[derive(Debug)]
pub struct KauthCall {
    pub action: String,
    pub arguments: Vec<u8>,
    pub fds: Vec<(String, OwnedFd)>,
}

/// Транспорт до помощника. Живой — [`SystemKauth`]; в сверках — подставной,
/// который записывает пришедшее и отвечает готовыми байтами.
pub trait KauthBus {
    /// Байты ответа помощника или текст ошибки шины.
    fn perform(&mut self, call: KauthCall) -> Result<Vec<u8>, String>;
}

/// Вызов действия и ответ для человека.
pub fn run_action(bus: &mut dyn KauthBus, call: KauthCall) -> Result<(), String> {
    let action = call.action.clone();
    let bytes = bus.perform(call).map_err(|e| {
        let mut text = tr("Rexon couldn't reach Plasma Login Manager's helper. Nothing was changed.");
        if !e.trim().is_empty() {
            text.push('\n');
            text.push_str(e.trim());
        }
        text
    })?;
    let reply = decode_reply(&bytes).map_err(|e| {
        crate::log_warn!("login", "ответ помощника PLM на {action} не разобран: {e}");
        tr("Plasma Login Manager's helper sent an answer Rexon doesn't understand. Check the login screen in its own settings.")
    })?;
    reply_outcome(&reply)
}

/// Живая системная шина (zbus, синхронный вызов). Ожидание ответа не
/// ограничено: внутри вызова помощник ждёт, пока человек введёт пароль в
/// окне polkit.
pub struct SystemKauth {
    /// Адрес шины; `None` — системная (иначе — стенд с поддельной службой).
    pub address: Option<String>,
}

impl SystemKauth {
    pub fn system() -> Self {
        SystemKauth { address: None }
    }
}

impl KauthBus for SystemKauth {
    fn perform(&mut self, call: KauthCall) -> Result<Vec<u8>, String> {
        use zbus::zvariant;
        let conn = match &self.address {
            None => zbus::blocking::Connection::system(),
            Some(a) => zbus::blocking::connection::Builder::address(a.as_str()).and_then(|b| b.build()),
        }
        .map_err(|e| e.to_string())?;
        // KAuth кладёт сюда имя своего соединения; помощник его не смотрит.
        let caller: Vec<u8> = conn.unique_name().map(|n| n.as_str().as_bytes().to_vec()).unwrap_or_default();
        let details: HashMap<String, zvariant::Value<'_>> = HashMap::new();
        let fds: HashMap<String, zvariant::OwnedFd> = call.fds.into_iter().map(|(k, fd)| (k, zvariant::OwnedFd::from(fd))).collect();
        let reply = conn
            .call_method(
                Some(HELPER_ID),
                "/",
                Some(KAUTH_INTERFACE),
                "performAction",
                &(call.action.as_str(), caller, details, call.arguments, fds),
            )
            .map_err(|e| e.to_string())?;
        reply.body().deserialize::<Vec<u8>>().map_err(|e| e.to_string())
    }
}

// ── Факты о машине ───────────────────────────────────────────────────────

/// Что известно о PLM на машине.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlmFacts {
    /// Текст [`CONFIG_FILE`] (`None` — файла нет).
    pub config: Option<String>,
    /// Тексты слоёв под ним в порядке чтения (поздний перекрывает).
    pub layers: Vec<String>,
    /// Дом учётной записи [`PLM_USER`] (`None` — её нет).
    pub home: Option<String>,
    /// Помощник установлен ([`HELPER_SERVICE_FILE`]).
    pub helper: bool,
    /// Приветствие (Qt 6) покажет WebP: у Qt есть модуль WebP
    /// ([`crate::system_plan::qt_has_webp`]).
    pub webp: bool,
    /// Своя копия Rexon картинки фона
    /// ([`crate::system_plan::kept_login_background`]).
    pub kept: Option<String>,
    /// Картинка из настроек (`Image`) читается этим пользователем.
    pub image_readable: bool,
}

impl PlmFacts {
    /// Значение ключа так, как его прочтёт PLM.
    pub fn value(&self, group: &str, key: &str) -> Option<String> {
        let mut texts = self.layers.clone();
        texts.extend(self.config.clone());
        crate::login_rescue::ini_value(&texts, group, key)
    }

    /// Модуль обоев экрана входа.
    pub fn plugin(&self) -> String {
        self.value(GREETER_GROUP, PLUGIN_KEY).filter(|v| !v.is_empty()).unwrap_or_else(|| IMAGE_PLUGIN.to_string())
    }

    fn own_value(&self, group: &str, key: &str) -> Option<String> {
        let texts: Vec<String> = self.config.iter().cloned().collect();
        crate::login_rescue::ini_value(&texts, group, key)
    }
}

/// Путь картинки из значения `Image` (`file:///…` или путь); пусто — нет.
pub fn image_path(value: &str) -> Option<String> {
    let v = value.trim();
    let v = v.strip_prefix("file://").unwrap_or(v);
    let v = v.split('#').next().unwrap_or("");
    v.starts_with('/').then(|| v.to_string())
}

/// Почему менять нельзя (пусто — можно).
pub fn blocked_reason(facts: &PlmFacts, steamos_block: &str) -> String {
    if !steamos_block.is_empty() {
        return steamos_block.to_string();
    }
    if !facts.helper {
        return tr(
            "Plasma Login Manager's settings helper isn't installed, so Rexon can't change the login screen. \
             Reinstall the plasma-login-manager package.",
        );
    }
    if facts.home.is_none() {
        // TRANSLATORS: "plasmalogin" is the name of a system account; keep it as is.
        return tr(
            "Rexon couldn't find the login screen's system account “plasmalogin”. Check that Plasma Login \
             Manager is installed properly.",
        );
    }
    String::new()
}

// ── Фон ──────────────────────────────────────────────────────────────────

/// Текст [`CONFIG_FILE`] после смены фона: `image` — имя своей картинки в
/// `<home>/wallpapers` (`None` — вернуть стандартный фон). Меняются только
/// ключи фона, всё прочее остаётся как было. Как kcm: модуль обоев —
/// «Картинка», если сейчас другой; `Image` — адрес `file://` копии,
/// временный `PreviewImage` снимается.
pub fn background_config(facts: &PlmFacts, image: Option<&str>) -> String {
    use crate::system_plan::{drop_ini_keys, ini_put, SessionKey};
    let mut text = facts.config.clone().unwrap_or_default();
    let preview = [SessionKey { group: IMAGE_GROUP, key: "PreviewImage" }];
    if let Some(t) = drop_ini_keys(&text, &preview) {
        text = t;
    }
    match image {
        Some(name) => {
            let home = facts.home.as_deref().unwrap_or("/var/lib/plasmalogin").trim_end_matches('/');
            if facts.plugin() != IMAGE_PLUGIN {
                text = ini_put(Some(&text), GREETER_GROUP, PLUGIN_KEY, IMAGE_PLUGIN);
            }
            text = ini_put(Some(&text), IMAGE_GROUP, "Image", &format!("file://{home}/wallpapers/{name}"));
        }
        None => {
            let keys = [SessionKey { group: IMAGE_GROUP, key: "Image" }, SessionKey { group: GREETER_GROUP, key: PLUGIN_KEY }];
            if let Some(t) = drop_ini_keys(&text, &keys) {
                text = t;
            }
        }
    }
    text
}

/// Свой ли сейчас фон и можно ли его вернуть: в [`CONFIG_FILE`] задана
/// картинка или другой модуль обоев.
pub fn background_custom(facts: &PlmFacts) -> bool {
    facts.own_value(IMAGE_GROUP, "Image").is_some_and(|v| !v.is_empty())
        || facts.own_value(GREETER_GROUP, PLUGIN_KEY).is_some_and(|v| !v.is_empty())
}

/// Сменить фон (`image` — путь картинки, `None` — стандартный фон). До
/// пароля — проверка картинки по первым байтам (`head`); файл открывается
/// на чтение здесь, помощник получает дескриптор, как от kcm.
pub fn background_with(
    facts: &PlmFacts,
    image: Option<&str>,
    head: &dyn Fn(&str) -> Option<Vec<u8>>,
    open: &dyn Fn(&str) -> Option<OwnedFd>,
    bus: &mut dyn KauthBus,
) -> Result<String, String> {
    let blocked = blocked_reason(facts, "");
    if !blocked.is_empty() {
        return Err(blocked);
    }
    let mut args: Vec<(String, QVariant)> = Vec::new();
    let mut fds = Vec::new();
    let name = match image {
        Some(path) => {
            let bytes = head(path).unwrap_or_default();
            let Some(ext) = crate::system_plan::login_image_ext_with(&bytes, facts.webp) else {
                return Err(crate::system_plan::login_image_refusal(&bytes, facts.webp, 6));
            };
            let Some(fd) = open(path) else {
                return Err(tr("Rexon couldn't read this picture. Nothing was changed."));
            };
            let name = format!("{BACKGROUND_STEM}.{ext}");
            fds.push((format!("_fd_{name}"), fd));
            args.push(("wallpapers".into(), QVariant::StringList(vec![name.clone()])));
            Some(name)
        }
        None => None,
    };
    let config = background_config(facts, name.as_deref());
    if config.len() > CONFIG_MAX_BYTES {
        return Err(tr("The login screen settings file is too large for Plasma Login Manager's helper. Nothing was changed."));
    }
    args.push(("config".into(), QVariant::String(Some(config))));
    run_action(bus, KauthCall { action: ACTION_SAVE.into(), arguments: encode_qvariant_map(&args), fds })?;
    Ok(if image.is_some() {
        tr("Done. The new background will show the next time the login screen appears.")
    } else {
        tr("Done. The login screen shows the standard background again.")
    })
}

// ── Часы ─────────────────────────────────────────────────────────────────

/// Ключ «Показывать часы» в `[Greeter]` (kcm: `ShowClock`, по умолчанию да).
pub const CLOCK_KEY: &str = "ShowClock";

/// Значение «Показывать часы» так, как его прочтёт PLM (нет — да).
pub fn show_clock(facts: &PlmFacts) -> bool {
    facts.value(GREETER_GROUP, CLOCK_KEY).and_then(|v| crate::system_plan::kconfig_bool(&v)).unwrap_or(true)
}

/// Текст [`CONFIG_FILE`] с «Показывать часы» = `show`. Как KConfigSkeleton
/// модуля: совпадает с умолчанием (слои под файлом, иначе «да») — ключ
/// снимается, иначе пишется; прочее остаётся как было.
pub fn clock_config(facts: &PlmFacts, show: bool) -> String {
    use crate::system_plan::{drop_ini_keys, ini_put, SessionKey};
    let text = facts.config.clone().unwrap_or_default();
    let default = crate::login_rescue::ini_value(&facts.layers, GREETER_GROUP, CLOCK_KEY)
        .and_then(|v| crate::system_plan::kconfig_bool(&v))
        .unwrap_or(true);
    if show == default {
        drop_ini_keys(&text, &[SessionKey { group: GREETER_GROUP, key: CLOCK_KEY }]).unwrap_or(text)
    } else {
        ini_put(Some(&text), GREETER_GROUP, CLOCK_KEY, if show { "true" } else { "false" })
    }
}

/// «Показывать часы» через помощника PLM (то же действие `save`, что у фона).
/// `save` у помощника стирает `<home>/wallpapers` и кладёт туда только
/// переданные картинки, поэтому своя картинка фона пересылается заново
/// дескриптором (как у kcm: `Image` указывает на её копию); `open` —
/// открыть обычный файл на чтение. Не открылась — отказ: иначе фон молча
/// пропал бы.
pub fn clock_with(
    facts: &PlmFacts,
    show: bool,
    open: &dyn Fn(&str) -> Option<OwnedFd>,
    bus: &mut dyn KauthBus,
) -> Result<String, String> {
    use crate::system_plan::ini_put;
    let blocked = blocked_reason(facts, "");
    if !blocked.is_empty() {
        return Err(blocked);
    }
    let mut config = clock_config(facts, show);
    let mut args: Vec<(String, QVariant)> = Vec::new();
    let mut fds = Vec::new();
    let own_image = (facts.plugin() == IMAGE_PLUGIN)
        .then(|| facts.own_value(IMAGE_GROUP, "Image"))
        .flatten()
        .and_then(|v| image_path(&v));
    if let Some(path) = own_image {
        let name = path.rsplit('/').next().unwrap_or("").to_string();
        let keep = || tr("Rexon couldn't read the current login screen background, so it would be lost. Nothing was changed.");
        if name.is_empty() || name.contains("..") {
            return Err(keep());
        }
        let Some(fd) = open(&path) else {
            return Err(keep());
        };
        let home = facts.home.as_deref().unwrap_or("/var/lib/plasmalogin").trim_end_matches('/');
        config = ini_put(Some(&config), IMAGE_GROUP, "Image", &format!("file://{home}/wallpapers/{name}"));
        fds.push((format!("_fd_{name}"), fd));
        args.push(("wallpapers".into(), QVariant::StringList(vec![name])));
    }
    if config.len() > CONFIG_MAX_BYTES {
        return Err(tr("The login screen settings file is too large for Plasma Login Manager's helper. Nothing was changed."));
    }
    args.push(("config".into(), QVariant::String(Some(config))));
    run_action(bus, KauthCall { action: ACTION_SAVE.into(), arguments: encode_qvariant_map(&args), fds })?;
    Ok(if show {
        tr("Done. The login screen will show the clock.")
    } else {
        tr("Done. The login screen won't show the clock.")
    })
}

// ── Живое превью ─────────────────────────────────────────────────────────
//
// Сверено с исходниками plasma-login-manager 6.8.2
// (`src/frontend/greeter/main.cpp`, `mockbackend/MockGreeterProxy.cpp`):
// приветствие `plasma-login-greeter` (каталог libexec) с флагом `--test`
// работает без демона, вход идёт в подставной бэкенд. Окна — во весь экран
// на каждом мониторе, на Wayland — слоем поверх всего с захватом клавиатуры;
// закрывается приветствие только само: успешный подставной вход любым
// пользователем с паролем «mypassword». Поэтому превью держит и гасит Rexon
// ([`crate::system_plan::run_greeter_preview`]), подсказка называет пароль.
// Кнопки питания там настоящие (`SessionManagement` без подмены) — об этом
// подсказка тоже говорит. Обои в тестовом режиме не рисуются (серый фон).

/// Где приветствие PLM лежит в разных раскладках libexec.
pub const GREETER_PATHS: &[&str] = &[
    "/usr/lib/plasma-login-greeter",
    "/usr/libexec/plasma-login-greeter",
    "/usr/lib64/libexec/plasma-login-greeter",
    "/usr/lib/x86_64-linux-gnu/libexec/plasma-login-greeter",
];

/// Команда превью экрана входа PLM. `Err` — почему показать нельзя.
pub fn preview_argv(executable: &dyn Fn(&str) -> bool) -> Result<Vec<String>, String> {
    GREETER_PATHS
        .iter()
        .find(|p| executable(p))
        .map(|p| vec![p.to_string(), "--test".to_string()])
        .ok_or_else(|| tr("Can't show the preview: Plasma Login Manager's greeter isn't installed."))
}

// ── Перенос оформления ───────────────────────────────────────────────────

fn item_locale() -> String {
    tr("Language and locale settings")
}
fn item_keyboard() -> String {
    tr("Keyboard layouts")
}
fn item_screens() -> String {
    tr("Screen arrangement, scale, and orientation settings")
}

/// Что уйдёт на экран входа PLM с этого стола: файлы (имя из
/// [`SYNC_FILES`] → путь у пользователя), вне Plasma — курсор и шрифт
/// значениями, и подписи для человека.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlmLook {
    pub plasma: bool,
    pub files: Vec<(String, String)>,
    pub cursor_theme: Option<String>,
    pub cursor_size: Option<u32>,
    pub font: Option<String>,
    pub items: Vec<String>,
    pub missing: Vec<String>,
}

/// Оформление стола для PLM по [`DesktopLook`] (значения курсора, шрифта и
/// NumLock стол уже прочёл). В Plasma — файлы, как у kcm; вне Plasma —
/// цвета Qt и `fontconfig`, курсор и шрифт значениями (их помощник получит
/// файлами `kcminputrc` и `kdeglobals`).
pub fn plm_look(plasma: bool, config_home: &str, desktop: &DesktopLook, exists: &dyn Fn(&str) -> bool) -> PlmLook {
    let cfg = config_home.trim_end_matches('/');
    let mut out = PlmLook { plasma, ..PlmLook::default() };
    let wanted: &[&str] = if plasma { SYNC_FILES } else { &["kdeglobals", "fontconfig/fonts.conf"] };
    for name in wanted {
        let path = format!("{cfg}/{name}");
        if exists(&path) {
            out.files.push((name.to_string(), path));
        }
    }
    if !plasma {
        out.cursor_theme = desktop.look.cursor_theme.clone();
        out.cursor_size = desktop.look.cursor_size;
        out.font = desktop.look.font.clone();
    }
    let has = |n: &str| out.files.iter().any(|(f, _)| f == n);
    let font = if plasma { desktop.look.font.is_some() } else { out.font.is_some() };
    let cursor = if plasma { desktop.look.cursor_theme.is_some() } else { out.cursor_theme.is_some() };
    let numlock = plasma && desktop.look.numlock.is_some();
    // Порядок — как в окне kcm «Применить параметры Plasma».
    let rows: Vec<(String, bool)> = vec![
        (item_locale(), plasma && has("plasma-localerc")),
        (item_keyboard(), plasma && has("kxkbrc")),
        (item_screens(), plasma && has("kwinoutputconfig.json")),
        (tr("NumLock preference"), numlock),
        (tr("Font"), font),
        (tr("Font rendering"), has("fontconfig/fonts.conf")),
        (tr("Color scheme"), has("kdeglobals")),
        (tr("Cursor theme and size"), cursor),
        (tr("Plasma theme"), plasma && has("plasmarc")),
    ];
    for (label, present) in rows {
        if present {
            out.items.push(label);
        } else {
            out.missing.push(label);
        }
    }
    out
}

/// Аргументы `sync`: имя файла → текст. Файлы больше
/// [`LOOK_FILE_MAX_BYTES`] не уходят (`size`). Вне Plasma шрифт ложится в
/// `[General] font` копии `kdeglobals`, курсор — в `[Mouse]` нового
/// `kcminputrc`.
pub fn sync_args(
    look: &PlmLook,
    read: &dyn Fn(&str) -> Option<String>,
    size: &dyn Fn(&str) -> Option<u64>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (name, path) in &look.files {
        if !size(path).is_some_and(|s| s <= LOOK_FILE_MAX_BYTES) {
            continue;
        }
        if let Some(text) = read(path) {
            out.push((name.clone(), text));
        }
    }
    if look.plasma {
        return out;
    }
    if let Some(font) = &look.font {
        let base = out.iter().position(|(n, _)| n == "kdeglobals");
        let text = crate::system_plan::ini_put(base.map(|i| out[i].1.as_str()), "General", "font", font);
        match base {
            Some(i) => out[i].1 = text,
            None => out.push(("kdeglobals".into(), text)),
        }
    }
    if let Some(theme) = &look.cursor_theme {
        let mut text = format!("[Mouse]\ncursorTheme={theme}\n");
        if let Some(size) = look.cursor_size {
            text.push_str(&format!("cursorSize={size}\n"));
        }
        out.push(("kcminputrc".into(), text));
    }
    out
}

/// Перенести оформление стола (`Some`) или «Сбросить» (`None`).
pub fn look_with(
    facts: &PlmFacts,
    look: Option<&PlmLook>,
    read: &dyn Fn(&str) -> Option<String>,
    size: &dyn Fn(&str) -> Option<u64>,
    bus: &mut dyn KauthBus,
) -> Result<String, String> {
    let blocked = blocked_reason(facts, "");
    if !blocked.is_empty() {
        return Err(blocked);
    }
    match look {
        Some(look) => {
            let args: Vec<(String, QVariant)> =
                sync_args(look, read, size).into_iter().map(|(k, v)| (k, QVariant::String(Some(v)))).collect();
            run_action(bus, KauthCall { action: ACTION_SYNC.into(), arguments: encode_qvariant_map(&args), fds: Vec::new() })?;
            Ok(tr("Done. The login screen will use these settings the next time it appears."))
        }
        None => {
            run_action(bus, KauthCall { action: ACTION_RESET.into(), arguments: encode_qvariant_map(&[]), fds: Vec::new() })?;
            Ok(tr("Done. The login screen settings are back to default."))
        }
    }
}

// ── Вид для окна ─────────────────────────────────────────────────────────

/// Раздел «Фон и оформление экрана входа» при PLM — в том же виде, что у
/// SDDM ([`SddmGreeterView`]); `standard` — картинка стандартного фона
/// Plasma для превью (пусто — нет).
pub fn greeter_view(facts: &PlmFacts, look: &PlmLook, standard: &str, steamos_block: &str) -> SddmGreeterView {
    let blocked = blocked_reason(facts, steamos_block);
    let plugin = facts.plugin();
    let own_image = (plugin == IMAGE_PLUGIN)
        .then(|| facts.value(IMAGE_GROUP, "Image"))
        .flatten()
        .and_then(|v| image_path(&v));
    let (image, state) = if plugin != IMAGE_PLUGIN {
        (String::new(), tr("Background: not a picture — set in the login screen's own settings"))
    } else if let Some(p) = own_image {
        let ours = is_own_background(p.rsplit('/').next().unwrap_or(""));
        crate::system_plan::custom_background_preview(ours, &p, facts.kept.as_deref(), &|_| facts.image_readable)
    } else {
        (standard.to_string(), tr("Background: the standard one"))
    };
    let list = |items: &[String]| items.iter().map(|i| format!("• {i}")).collect::<Vec<_>>().join("\n");
    let mut confirm = if look.plasma {
        tr("The login screen will reflect your customizations to the following Plasma settings:")
    } else {
        tr("The login screen will reflect these settings of your desktop:")
    };
    confirm.push('\n');
    confirm.push_str(&list(&look.items));
    if !look.missing.is_empty() {
        confirm.push_str("\n\n");
        confirm.push_str(&if look.plasma {
            tr("Not set on this desktop, so the login screen keeps its own:")
        } else {
            tr("Not taken from this desktop — the login screen keeps its own:")
        });
        confirm.push('\n');
        confirm.push_str(&list(&look.missing));
    }
    confirm.push_str("\n\n");
    confirm.push_str(&tr("Themes and fonts show on the login screen only if they are installed for all users."));
    SddmGreeterView {
        visible: true,
        title: tr("Login screen background and look"),
        theme: String::new(),
        can_change: blocked.is_empty(),
        blocked,
        background: SddmBackgroundView {
            supported: true,
            image,
            state,
            note: String::new(),
            choose: tr("Change Background…"),
            reset: tr("Use the Standard Background"),
            can_reset: background_custom(facts),
            filter: login_image_filter(facts.webp),
            dialog_title: tr("Choose a Login Screen Background"),
        },
        clock: greeter_clock_view(true, show_clock(facts)),
        preview: greeter_preview_view(true, None),
        look: SddmLookView {
            apply: if look.plasma { tr("Apply Plasma Settings…") } else { tr("Apply Desktop Settings…") },
            reset: tr("Reset to Default Settings…"),
            confirm_title: if look.plasma { tr("Apply Plasma Settings") } else { tr("Apply Desktop Settings") },
            confirm,
            reset_confirm: tr(
                "The login screen will go back to its own cursor, font, colors, keyboard layouts, language and \
                 screen settings. Its background and other settings stay as they are.",
            ),
        },
    }
}

// ── Живая машина ─────────────────────────────────────────────────────────

/// Дом учётной записи `user` (NSS, как `KUser`).
pub fn user_home_live(user: &str) -> Option<String> {
    let name = std::ffi::CString::new(user).ok()?;
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0 as libc::c_char; 16 * 1024];
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: буферы живут до конца вызова; результат читается только при успехе.
    let rc = unsafe { libc::getpwnam_r(name.as_ptr(), &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result) };
    if rc != 0 || result.is_null() || pwd.pw_dir.is_null() {
        return None;
    }
    // SAFETY: pw_dir указывает в `buf`, строка с нулём на конце.
    let dir = unsafe { std::ffi::CStr::from_ptr(pwd.pw_dir) }.to_string_lossy().into_owned();
    (!dir.is_empty()).then_some(dir)
}

fn conf_dir_texts(dir: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.iter().filter_map(|n| std::fs::read_to_string(format!("{dir}/{n}")).ok()).collect()
}

/// [`PlmFacts`] на живой машине.
pub fn facts_live() -> PlmFacts {
    let mut layers: Vec<String> = std::fs::read_to_string(SYSTEM_DEFAULTS_FILE).ok().into_iter().collect();
    layers.extend(conf_dir_texts(SYSTEM_CONF_DIR));
    layers.extend(conf_dir_texts(CONF_DIR));
    PlmFacts {
        config: std::fs::read_to_string(CONFIG_FILE).ok(),
        layers,
        home: user_home_live(PLM_USER),
        helper: std::path::Path::new(HELPER_SERVICE_FILE).is_file(),
        webp: crate::system_plan::qt_has_webp_live(6),
        kept: crate::system_plan::kept_login_background_live(crate::system_plan::LOGIN_BG_PLM),
        image_readable: false,
    }
    .with_image_readable_live()
}

impl PlmFacts {
    /// Отметить, читается ли картинка из настроек этим пользователем.
    fn with_image_readable_live(mut self) -> Self {
        self.image_readable = self.value(IMAGE_GROUP, "Image")
            .and_then(|v| image_path(&v))
            .is_some_and(|p| std::fs::File::open(p).is_ok());
        self
    }
}

/// Картинка стандартного фона Plasma (`Next`) для превью; пусто — нет.
pub fn standard_background_live() -> String {
    let dir = "/usr/share/wallpapers/Next/contents/images";
    let preferred = format!("{dir}/1920x1080.png");
    if std::path::Path::new(&preferred).is_file() {
        return preferred;
    }
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names.into_iter().find(|n| n.ends_with(".png") || n.ends_with(".jpg")).map(|n| format!("{dir}/{n}")).unwrap_or_default()
}

/// [`plm_look`] на живой машине для стола `de` (`kde` — Plasma).
pub fn plm_look_live(de: &str) -> PlmLook {
    let desktop = crate::system_plan::desktop_greeter_look_live(de);
    plm_look(de == "kde", &crate::utils::config_home_env(), &desktop, &|p| std::path::Path::new(p).is_file())
}

/// [`greeter_view`] на живой машине.
pub fn greeter_view_live(de: &str) -> SddmGreeterView {
    let steamos = if crate::distro_compat::running_on_steamos() {
        crate::distro_compat::SteamOsBlock::LoginScreen.reason()
    } else {
        String::new()
    };
    let mut view = greeter_view(&facts_live(), &plm_look_live(de), &standard_background_live(), &steamos);
    view.preview = greeter_preview_view(true, preview_argv(&crate::system_plan::is_executable_live).err());
    view
}

/// [`clock_with`] на живой машине (окно пароля — polkit).
pub fn clock_live(show: bool) -> Result<String, String> {
    // Копия в доме `plasmalogin` обычно не читается (дом закрыт) — тогда
    // своя копия Rexon ([`crate::system_plan::kept_login_background`]).
    clock_with(
        &facts_live(),
        show,
        &|p| {
            open_regular(p).or_else(|| {
                let name = p.rsplit('/').next().unwrap_or("");
                is_own_background(name)
                    .then(|| crate::system_plan::kept_login_background_live(crate::system_plan::LOGIN_BG_PLM))
                    .flatten()
                    .and_then(|k| open_regular(&k))
            })
        },
        &mut SystemKauth::system(),
    )
}

/// [`background_with`] на живой машине (окно пароля — polkit).
pub fn background_live(image: Option<&str>) -> Result<String, String> {
    use std::io::Read;
    let head = |p: &str| {
        let mut buf = [0u8; 16];
        let n = std::fs::File::open(p).and_then(|mut f| f.read(&mut buf)).ok()?;
        Some(buf[..n].to_vec())
    };
    let facts = facts_live();
    let done = background_with(&facts, image, &head, &open_regular, &mut SystemKauth::system())?;
    // Своя копия картинки: дом `plasmalogin` закрыт для чтения, а превью и
    // `save` помощника (часы, [`clock_with`]) берут её отсюда.
    crate::system_plan::sync_kept_login_background_live(crate::system_plan::LOGIN_BG_PLM, image, facts.webp);
    Ok(done)
}

/// Обычный файл на чтение (не каталог, не устройство).
fn open_regular(p: &str) -> Option<OwnedFd> {
    std::fs::File::open(p).ok().filter(|f| f.metadata().is_ok_and(|m| m.is_file())).map(OwnedFd::from)
}

/// Имя файла — картинка фона, положенная Rexon (`rexon-background.<ext>`).
pub fn is_own_background(name: &str) -> bool {
    name.strip_prefix(BACKGROUND_STEM)
        .and_then(|r| r.strip_prefix('.'))
        .is_some_and(|e| matches!(e, "png" | "jpg" | "webp"))
}

/// [`look_with`] на живой машине: `de` — стол, с которого переносится
/// оформление (`None` — «Сбросить»).
pub fn look_live(de: Option<&str>) -> Result<String, String> {
    let look = de.map(plm_look_live);
    look_with(
        &facts_live(),
        look.as_ref(),
        &|p| std::fs::read(p).ok().map(|b| String::from_utf8_lossy(&b).into_owned()),
        &|p| std::fs::metadata(p).ok().filter(|m| m.is_file()).map(|m| m.len()),
        &mut SystemKauth::system(),
    )
}
