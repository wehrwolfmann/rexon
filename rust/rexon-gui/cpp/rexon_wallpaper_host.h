// Shim of the video wallpaper host: two things that Rust cannot reach.
//
// The first is the X11 window identifier (`QWindow::winId`); without it
// there is nothing to set EWMH properties on. The second is the window's
// frame stream into a shared mmap file: it relies on
// `QQuickWindow::grabWindow`, which returns a `QImage`, and cxx-qt-lib 0.9
// has no QImage at all.
//
// The boundaries are the same as for the layer shim
// (cpp/rexon_layer_shell.*): there are no decisions here. The frame layout,
// the file permissions, the tick interval and the EWMH plan for X11 arrive
// from the core (`rexon-core::wallpaper_mmap`) already computed — the shim
// only executes.
#pragma once

#include <cstddef>
#include <cstdint>

#include <QString>

class QObject;

/// Window identifier for X11 (`QWindow::winId`). 0 means the object is not
/// a QWindow.
///
/// If the platform window does not exist yet, winId creates it (without
/// showing) — on X11 this is used to set the window type BEFORE showing. On
/// Wayland do not call it before showing: there it is just another source
/// of surprises.
std::int64_t rexon_wp_window_id(QObject *window);

/// Start the window's frame stream into an mmap file.
///
/// Frames are captured from a `QQuickWindow` (the window itself or its
/// first such descendant), converted to RGBA8888 and to the given size and
/// written to the file in full under `flock(LOCK_EX)` — the same contract
/// the GNOME and Cinnamon shell extensions read: raw RGBA8888, exactly
/// width×height×4 bytes, no header.
///
/// The frame file is created ANEW when the stream starts (the previous one
/// is unlinked) and until the first real frame it is shorter than
/// width×height×4 — that is how the shell extensions tell "no frame yet"
/// from a finished frame. Without this, on a wallpaper change they managed
/// to show the last frame of the PREVIOUS wallpaper.
///
/// Frame source: `video_output` (a VideoOutput from QML) — the current
/// frame of its sink; `nullptr` — the window's scene is rendered by
/// `QQuickRenderControl` without a window on screen (the window's content
/// is moved into it) and read back. Video is taken from the sink because
/// VideoOutput does not render on the `offscreen` platform. If the
/// windowless scene did not come up, the window is shown and captured with
/// `grabWindow` — ONLY with `may_show_window` (the core's decision: the
/// `offscreen` platform), otherwise a refusal.
///
/// Next to the frame a generation counter is kept (`generation_path`, u64
/// LE): odd while a frame is being written, even afterwards. The shell
/// extensions use it — they cannot take an `flock` lock from GJS. An empty
/// path means no counter.
///
/// The timer and the descriptor live on the window object and die together
/// with it. `false` means the window was not found or the file did not
/// open.
bool rexon_wp_mmap_stream_start(QObject *window,
                                QObject *video_output,
                                const QString &path,
                                const QString &generation_path,
                                std::int32_t width,
                                std::int32_t height,
                                std::int32_t interval_ms,
                                std::uint32_t file_mode,
                                bool may_show_window);

/// What captures the stream's frames: `rendercontrol-gpu <API> <device>`,
/// `rendercontrol-software`, `videosink`, `grabwindow`; empty means there
/// is no stream.
QString rexon_wp_mmap_capture(QObject *window);

/// Share of non-black pixels of the last frame, ‰ (sampled); -1 means there
/// were no frames.
std::int64_t rexon_wp_mmap_nonblack_permille(QObject *window);

/// How many frames went into the file since the stream started. For the
/// self-test: "the stream started" and "the frames arrived" are different
/// statements.
std::int64_t rexon_wp_mmap_frames(QObject *window);

/// Centre pixel of the last written frame (0xRRGGBB), -1 means there were no
/// frames. For the self-test: "the frames arrived" and "the frames contain a
/// picture" are also different statements (black frames used to count as
/// success).
std::int64_t rexon_wp_mmap_center_rgb(QObject *window);

// ── Flash mode (SWF): a bare window that wgpu draws into ─────────────────
// cxx-qt-lib 0.9 has no QWindow, QScreen or platform native interface
// types — hence the shim. There are no decisions here: placement, size and
// pace are decided by Rust.

/// How many screens the session has right now. 0 means there are no
/// monitors at all (this can happen on the fly: the last cable was pulled),
/// and then the wallpaper is put nowhere.
std::int32_t rexon_wp_screen_count();

/// Whether to quit the application together with the last window.
///
/// Warning: wallpapers need `false`, and this is not decoration. The host
/// has as many windows as monitors; when a monitor is disconnected its
/// window is destroyed, and with the Qt default (`true`) the departure of
/// the last monitor would kill the whole wallpaper process — a monitor
/// coming back a second later would stay empty forever.
void rexon_wp_set_quit_on_last_window_closed(bool enabled);

/// A new window without QML and without a Qt buffer, on screen
/// `screen_index` (< 0 — the primary). `width`/`height` ≤ 0 means the size
/// of that screen, and the window is placed in its corner: on X11 otherwise
/// all wallpaper windows would land on one monitor. The layer (LayerShellQt)
/// is set BEFORE showing.
QObject *rexon_wp_swf_window_new(std::int32_t width,
                                 std::int32_t height,
                                 std::int32_t screen_index);
void rexon_wp_swf_window_show(QObject *window);
/// Remove the window entirely: the GPU refused to draw into it, and an
/// empty shutter left behind would be a black rectangle over the whole
/// monitor instead of a wallpaper.
void rexon_wp_swf_window_close(QObject *window);
bool rexon_wp_swf_window_exposed(QObject *window);
/// Size in physical pixels (taking devicePixelRatio into account).
std::int32_t rexon_wp_swf_window_width(QObject *window);
std::int32_t rexon_wp_swf_window_height(QObject *window);
/// Spin the Qt event queue (Flash mode has its own render loop).
void rexon_wp_process_events();
/// Native handles for wgpu; 0 means the wrong platform or no window.
std::size_t rexon_wp_wl_display();
std::size_t rexon_wp_wl_surface(QObject *window);
std::size_t rexon_wp_xcb_connection();
