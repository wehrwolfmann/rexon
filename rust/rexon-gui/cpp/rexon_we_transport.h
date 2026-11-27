// The return address of the Wallpaper Engine tab's queries: whom to give
// the daemon's answer to.
//
// What it does. The transport is entirely in Rust (src/we_socket.rs — the
// socket to the daemon, src/we_transport.rs — sending frames). A query (any
// of dozens of cxx-qt classes: Discover, wallpapers, playlists, plugins…)
// leaves its number and a pointer to itself here before sending a frame.
// When the socket receives an answer with this number, it calls
// rexon_we_do_deliver_response — and the shim calls the query's
// `deliverResponse(int64, QByteArray)` invokable. On a disconnection
// rexon_we_do_fail_pending hands one error frame to everyone waiting, so
// that none stays an eternal spinner.
//
// Why C++ and not Rust (rule #16). There are no decisions here — only two
// Qt mechanisms that cxx-qt lacks:
//   * QPointer — a weak reference to a QObject that its destructor nulls.
//     An answer to a closed tab (the query is already deleted) therefore
//     turns into a quiet 0 rather than an access to freed memory. cxx-qt-lib
//     0.9 has no QPointer, and manual lifetime accounting from Rust would
//     give exactly the "answer arrived after deletion" race;
//   * calling a method by NAME through the metaobject
//     (QMetaObject::invokeMethod). The queries are different types, they
//     have no common Rust trait; they are united only by an identically
//     named invokable, and only moc can find it.
// Ordinary functions rather than a QObject with signals: the crate's
// cpp_files do not go through moc (cxx-qt-build builds them as is).
#pragma once

#include <QtCore/QtGlobal>

class QObject;

extern "C" {

/// Remember whom the number belongs to. Called when a frame is sent.
void rexon_we_remember(qint64 token, QObject* query);

/// Give the daemon's answer to the query that asked for it.
///
/// Returns 1 if the answer was delivered, and 0 if the query has already
/// died (the tab was closed, the view was recreated): the QPointer was
/// nulled, there is nobody to deliver to.
int rexon_we_do_deliver_response(qint64 token, const char* data, int len);

/// How many queries are waiting for an answer right now (for the socket
/// check and the log).
int rexon_we_do_pending_count();

/// Give ONE and the same frame to everyone waiting and clear the map.
///
/// A disconnection: each gets a `Response` frame with a non-OK status, and
/// the query goes into Error by its ordinary path — no new invokables are
/// needed.
///
/// Returns the number of queries the frame reached.
int rexon_we_do_fail_pending(const char* data, int len);
}
