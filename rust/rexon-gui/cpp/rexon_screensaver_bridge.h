// Qt primitives of the "do not blank the screen" bridge
// (org.freedesktop.ScreenSaver).
//
// There is NOT A SINGLE decision here: whether the bridge is needed in
// this environment, where to forward the request, which cookies to hand
// out and when to release the inhibition is known to the core
// (rexon-core::screensaver_bridge) and executed by
// rust/rexon-gui/src/screensaver_bridge.rs. This file only keeps the
// object on the bus, passes every incoming call to Rust as is and, on
// Rust's command, answers, calls other services and watches clients
// disappear.
//
// Why a shim and not pure Rust: a bus service is a subclass of
// QDBusVirtualObject (a type, not a free function), and Qt delivers calls
// to it through a virtual method. cxx-qt-lib has no QtDBus bindings, and
// pulling in a second bus client (zbus) for one object is excessive: the
// process already has a bus connection, and this MATTERS in substance —
// the session counts an inhibition against the connection it was taken
// through, and releases it by itself when the connection closes. So all
// calls go through one long-lived process connection.
//
// Argument values cross the boundary as strings by a D-Bus signature of
// simple types: `s` — as is, `u`/`i` — as a decimal number, `b` —
// "true"/"false". The bridge needs nothing else.
#pragma once

#include <cstdint>

extern "C" {

// ── callbacks into Rust (all in the GUI thread) ───────────────────────────

// A method call arrived on one of our paths. `token` is a receipt: Rust
// must answer it EXACTLY once (`rexon_ssb_reply*`), immediately or later.
typedef void (*RexonSsbMessage)(uint64_t token, const char *sender, const char *path,
                                const char *iface, const char *member,
                                const char *signature, const char *const *values,
                                int32_t count);
// Answer to `rexon_ssb_call`. `ok` means the answer was not an error;
// `value` is the first returned value as a string ("" — no values), on an
// error — the error name and the text separated by a space.
typedef void (*RexonSsbCallDone)(uint64_t ctx, bool ok, const char *value);
// A bus name being watched (`rexon_ssb_watch`) was left without an owner.
typedef void (*RexonSsbVanished)(const char *name);
// Our name went to another owner (they asked for it with replacement).
typedef void (*RexonSsbLost)();

void rexon_ssb_set_hooks(RexonSsbMessage on_message, RexonSsbCallDone on_call_done,
                         RexonSsbVanished on_vanished, RexonSsbLost on_lost);

// ── bus ───────────────────────────────────────────────────────────────────

// Whether the name has an owner: 1 — yes, 0 — no, -1 — no bus. From any
// thread.
int32_t rexon_ssb_name_has_owner(const char *name);

// Take the name and put the object on the paths `paths` (`count` of them)
// with the interface description `introspection_xml`. From any thread: the
// work goes to the GUI thread. 0 — in place; 1 — the name is already taken
// (nothing was put); 2 — no bus; 3 — the paths did not register.
int32_t rexon_ssb_start(const char *service, const char *const *paths, int32_t count,
                        const char *introspection_xml);

// Answer a receipt with values by signature ("" — an empty answer).
void rexon_ssb_reply(uint64_t token, const char *signature, const char *const *values,
                     int32_t count);
// Answer a receipt with a D-Bus error.
void rexon_ssb_reply_error(uint64_t token, const char *error_name, const char *text);

// Call another service without waiting for the answer in this thread. The
// answer arrives in `on_call_done` with the same `ctx`. `timeout_ms` is
// decided by the core (-1 — the bus timeout).
void rexon_ssb_call(uint64_t ctx, const char *service, const char *path, const char *iface,
                    const char *method, const char *signature, const char *const *values,
                    int32_t count, int32_t timeout_ms);

// Watch a name / stop watching.
void rexon_ssb_watch(const char *name);
void rexon_ssb_unwatch(const char *name);

// Remove the object from the paths and give up the name (if it is still
// ours). GUI thread.
void rexon_ssb_stop(const char *service);

}
