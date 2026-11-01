// Qt-примитивы моста «не гасить экран» (org.freedesktop.ScreenSaver).
//
// Здесь НЕТ ни одного решения: нужен ли мост в этом окружении, во что
// пробрасывать просьбу, какие куки выдавать и когда снимать удержание — знает
// ядро (rexon-core::screensaver_bridge), исполняет
// rust/rexon-gui/src/screensaver_bridge.rs. Этот файл только держит объект на
// шине, передаёт каждый пришедший вызов в Rust как есть и по команде Rust
// отвечает, зовёт чужие службы и следит за исчезновением клиентов.
//
// Почему шим, а не чистый Rust: служба на шине — это наследник
// QDBusVirtualObject (тип, а не свободная функция), и вызовы к ней Qt
// доставляет через виртуальный метод. Биндингов к QtDBus в cxx-qt-lib нет, а
// тащить второй клиент шины (zbus) ради одного объекта — лишнее: подключение
// к шине у процесса уже есть, и это ВАЖНО по существу — удержание у сеанса
// числится за тем подключением, через которое его взяли, и снимается само,
// когда подключение закрывается. Потому все вызовы идут через одно
// долгоживущее подключение процесса.
//
// Значения аргументов ходят через границу строками по сигнатуре D-Bus из
// простых типов: `s` — как есть, `u`/`i` — десятичным числом, `b` —
// «true»/«false». Другого мосту не нужно.
#pragma once

#include <cstdint>

extern "C" {

// ── обратные вызовы в Rust (все — в потоке GUI) ───────────────────────────

// Пришёл вызов метода на один из наших путей. `token` — квитанция: на неё
// Rust обязан ответить РОВНО один раз (`rexon_ssb_reply*`), сразу или позже.
typedef void (*RexonSsbMessage)(uint64_t token, const char *sender, const char *path,
                                const char *iface, const char *member,
                                const char *signature, const char *const *values,
                                int32_t count);
// Ответ на `rexon_ssb_call`. `ok` — ответ пришёл не ошибкой; `value` —
// первое возвращённое значение строкой («» — значений нет), при ошибке —
// имя ошибки и текст через пробел.
typedef void (*RexonSsbCallDone)(uint64_t ctx, bool ok, const char *value);
// Имя на шине, за которым следили (`rexon_ssb_watch`), осталось без хозяина.
typedef void (*RexonSsbVanished)(const char *name);
// Наше имя ушло другому хозяину (он попросил его с заменой).
typedef void (*RexonSsbLost)();

void rexon_ssb_set_hooks(RexonSsbMessage on_message, RexonSsbCallDone on_call_done,
                         RexonSsbVanished on_vanished, RexonSsbLost on_lost);

// ── шина ──────────────────────────────────────────────────────────────────

// Есть ли у имени хозяин: 1 — да, 0 — нет, -1 — шины нет. Из любого потока.
int32_t rexon_ssb_name_has_owner(const char *name);

// Занять имя и поставить объект на пути `paths` (`count` штук) с описанием
// интерфейса `introspection_xml`. Из любого потока: работа уходит в поток GUI.
// 0 — встали; 1 — имя уже занято (ничего не поставлено); 2 — шины нет;
// 3 — пути не зарегистрировались.
int32_t rexon_ssb_start(const char *service, const char *const *paths, int32_t count,
                        const char *introspection_xml);

// Ответить на квитанцию значениями по сигнатуре («» — пустой ответ).
void rexon_ssb_reply(uint64_t token, const char *signature, const char *const *values,
                     int32_t count);
// Ответить на квитанцию ошибкой D-Bus.
void rexon_ssb_reply_error(uint64_t token, const char *error_name, const char *text);

// Позвать чужую службу, не ожидая ответа в этом потоке. Ответ придёт в
// `on_call_done` с тем же `ctx`. `timeout_ms` решает ядро (-1 — срок шины).
void rexon_ssb_call(uint64_t ctx, const char *service, const char *path, const char *iface,
                    const char *method, const char *signature, const char *const *values,
                    int32_t count, int32_t timeout_ms);

// Следить за именем / перестать следить.
void rexon_ssb_watch(const char *name);
void rexon_ssb_unwatch(const char *name);

// Снять объект с путей и отдать имя (если оно ещё наше). Поток GUI.
void rexon_ssb_stop(const char *service);

}
