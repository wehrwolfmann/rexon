#include "rexon_wallpaper_host.h"

#include <fcntl.h>
#include <sys/file.h>
#include <unistd.h>

#include <memory>

#include <QHash>
#include <QImage>
#include <QObject>
#include <QPointer>
#include <QQuickItem>
#include <QQuickRenderControl>
#include <QQuickRenderTarget>
#include <QQuickWindow>
// Qt's GPU API, semi-public since Qt 6.6 (the versioned directory is added
// by build.rs): the QQuickRenderControl render target texture and reading
// it back.
#include <rhi/qrhi.h>
#include <QTimer>
#include <QVariant>
#include <QVideoFrame>
#include <QVideoSink>
#include <QWindow>

#include <QGuiApplication>
#include <QScreen>
#include <QtGui/qguiapplication_platform.h>
// Warning: a private but stable since Qt 5 platform interface: there is no
// public way to get a window's wl_surface in Qt 6.11 (wl_display and xcb
// are public, below). The directories with the Qt version are added by
// build.rs.
#include <qpa/qplatformnativeinterface.h>

namespace
{

/// A Qt Quick scene without a window: `QQuickRenderControl` renders the QML
/// window's content into its own target, and the frame is read back.
///
/// Why (measurement 15.09.2026, 1920×1080, 30 fps). Previously web
/// wallpapers in the frame stream were captured with `grabWindow` from a
/// shown window on the `offscreen` platform: Qt has no EGL there, Qt Quick
/// fell back to software rendering, Chromium ran without a GPU: CSS at 17
/// fps with 157 % CPU, no WebGL at all. On the session's real platform
/// (wayland/xcb) the window must not be shown — on Mutter it appears on top
/// of the desktop — while QQuickRenderControl creates no window at all (not
/// a single `wl_surface` over the run) and renders on the GPU: CSS at 43 %
/// CPU, WebGL at 25 % with a live frame, the page runs at the stream's
/// rate. This is Qt's standard "render without a screen" path
/// (QQuickRenderControl documentation, the rendercontrol_rhi example).
struct SceneRender {
    QQuickRenderControl *control = nullptr;
    QQuickWindow *window = nullptr;
    QRhiTexture *texture = nullptr;
    QRhiRenderBuffer *depth = nullptr;
    QRhiTextureRenderTarget *target = nullptr;
    QRhiRenderPassDescriptor *pass = nullptr;
    /// Software render target — when Qt has no GPU (the `offscreen` platform
    /// without GL).
    QImage software;

    ~SceneRender()
    {
        delete pass;
        delete target;
        delete depth;
        delete texture;
        delete window;
        delete control;
    }
};

/// State of one frame stream. It lives exactly as long as the window: the
/// timer is a child of the window, and the map entry is removed on
/// `destroyed`.
struct Stream {
    int fd = -1;
    /// Frame generation counter (a seqlock for a lock-free reader); -1 — none.
    int gen_fd = -1;
    std::uint64_t generation = 0;
    std::int32_t width = 0;
    std::int32_t height = 0;
    std::int64_t frames = 0;
    /// Centre pixel of the last written frame, 0xRRGGBB; -1 — there was none.
    std::int64_t center_rgb = -1;
    /// Share of non-black pixels of the last frame, ‰ (sampled); -1 — there was
    /// none.
    std::int64_t nonblack_permille = -1;
    /// Frame source — a VideoOutput; empty means the Qt Quick scene.
    QPointer<QObject> video;
    /// Windowless scene (web); empty for video and for a window snapshot.
    std::shared_ptr<SceneRender> scene;
    /// What captures the frame — for the self-test and the log.
    QString capture;
};

QHash<QObject *, Stream> &streams()
{
    static QHash<QObject *, Stream> map;
    return map;
}

/// The QtQuick window: the object itself or its first such descendant.
/// Frames are captured only from it — only QQuickWindow has `grabWindow`.
QQuickWindow *quick_window(QObject *object)
{
    if (auto *w = qobject_cast<QQuickWindow *>(object)) {
        return w;
    }
    return object == nullptr ? nullptr : object->findChild<QQuickWindow *>();
}

/// The player's current frame straight from the VideoOutput sink, filling
/// the stream frame entirely with the edges cropped — like
/// `VideoOutput.PreserveAspectCrop` in WallpaperVideo.qml.
///
/// Why not `grabWindow`. In frame stream mode the host lives on the
/// `offscreen` platform (otherwise on GNOME/Cinnamon Wayland its window
/// appears on top of the desktop), and there Qt Quick renders only in
/// software, and VideoOutput draws nothing in it: measurement — 186 frames
/// in the file, all black (0,0,0), with any QSG_RHI_BACKEND/QT_QUICK_BACKEND.
/// The sink, however, gets the decoder's frames regardless of the renderer,
/// and `QVideoFrame::toImage` on the same platform gives real colour.
QImage video_image(QObject *video_output, std::int32_t width, std::int32_t height)
{
    auto *sink = qobject_cast<QVideoSink *>(
        video_output->property("videoSink").value<QObject *>());
    if (sink == nullptr) {
        return {};
    }
    const QVideoFrame frame = sink->videoFrame();
    if (!frame.isValid()) {
        return {}; // the decoder has not given the first frame yet — the file stays short
    }
    const QImage image = frame.toImage();
    if (image.isNull() || (image.width() == width && image.height() == height)) {
        return image;
    }
    const QImage scaled = image.scaled(width, height, Qt::KeepAspectRatioByExpanding,
                                       Qt::SmoothTransformation);
    return scaled.copy((scaled.width() - width) / 2, (scaled.height() - height) / 2,
                       width, height);
}

/// Write the generation counter (u64 little-endian — the layout is set by
/// the core, `wallpaper_mmap::frame_generation_bytes`).
void write_generation(const Stream &stream, std::uint64_t value)
{
    if (stream.gen_fd < 0) {
        return;
    }
    unsigned char bytes[8];
    for (int i = 0; i < 8; ++i) {
        bytes[i] = static_cast<unsigned char>(value >> (8 * i));
    }
    (void)::pwrite(stream.gen_fd, bytes, sizeof bytes, 0);
}

/// One frame into the file. It is written in full and under an exclusive
/// lock — for readers that can take a shared one. The shell extensions
/// cannot (GJS has no `flock`), so for them the same frame is fenced by the
/// generation counter: odd — the frame is being written, even — it is
/// written in full (`wallpaper_mmap::frame_generation_path`).
void write_frame(Stream &stream, const QImage &source)
{
    QImage image = source;
    if (image.isNull()) {
        return;
    }
    if (image.width() != stream.width || image.height() != stream.height) {
        image = image.scaled(stream.width, stream.height);
    }
    if (image.format() != QImage::Format_RGBA8888) {
        image = image.convertToFormat(QImage::Format_RGBA8888);
    }
    if (image.isNull()) {
        return;
    }

    const qsizetype row_bytes = static_cast<qsizetype>(stream.width) * 4;
    if (::flock(stream.fd, LOCK_EX) != 0) {
        return;
    }
    stream.generation += 1; // odd: the frame is being written
    write_generation(stream, stream.generation);
    ::lseek(stream.fd, 0, SEEK_SET);
    // Row by row, not in one chunk: QImage rows are aligned, and bytesPerLine
    // may be larger than the frame width — writing it in one piece would
    // shift, and the extension would show a "skewed" picture.
    for (std::int32_t y = 0; y < stream.height; ++y) {
        const uchar *line = image.constScanLine(y);
        qsizetype written = 0;
        while (written < row_bytes) {
            const ssize_t n = ::write(stream.fd, line + written,
                                      static_cast<size_t>(row_bytes - written));
            if (n <= 0) {
                break;
            }
            written += n;
        }
    }
    stream.generation += 1; // even: the frame is written in full
    write_generation(stream, stream.generation);
    ::flock(stream.fd, LOCK_UN);
    stream.frames += 1;
    const QRgb center = image.pixel(stream.width / 2, stream.height / 2);
    stream.center_rgb = (static_cast<std::int64_t>(qRed(center)) << 16)
        | (qGreen(center) << 8) | qBlue(center);
    // The centre can be black on a live page too, so the self-test also needs
    // the share of non-black over the whole frame. Sampling on a 16×16 grid:
    // ~8 thousand points.
    std::int64_t lit = 0;
    std::int64_t total = 0;
    for (std::int32_t y = 0; y < stream.height; y += 16) {
        const uchar *line = image.constScanLine(y);
        for (std::int32_t x = 0; x < stream.width; x += 16) {
            const uchar *px = line + static_cast<qsizetype>(x) * 4;
            total += 1;
            if (px[0] > 16 || px[1] > 16 || px[2] > 16) {
                lit += 1;
            }
        }
    }
    stream.nonblack_permille = total == 0 ? 0 : lit * 1000 / total;
}

/// Move the QML window's content into the QQuickRenderControl scene and
/// prepare the target. `false` means Qt did not bring up the scene (the
/// content is put back).
bool start_scene(Stream &stream, QQuickWindow *source)
{
    auto scene = std::make_shared<SceneRender>();
    scene->control = new QQuickRenderControl;
    scene->window = new QQuickWindow(scene->control);
    scene->window->setColor(source->color());
    const QSize size(stream.width, stream.height);
    scene->window->resize(size);
    // Warning: `resize` of a window without a platform window does not reach
    // `contentItem` (it stays 0×0), and the WebEngineView is attached to it
    // with `anchors.fill`. Measurement: without this line Chromium saw
    // `innerWidth=0` for the whole run, the WebGL canvas came up 0×0 and the
    // frame was black.
    scene->window->contentItem()->setSize(QSizeF(size));

    const QList<QQuickItem *> items = source->contentItem()->childItems();
    for (QQuickItem *item : items) {
        item->setParentItem(scene->window->contentItem());
    }
    if (!scene->control->initialize()) {
        for (QQuickItem *item : items) {
            item->setParentItem(source->contentItem());
        }
        return false;
    }

    QQuickRenderTarget target;
    if (QRhi *rhi = scene->control->rhi()) {
        scene->texture = rhi->newTexture(QRhiTexture::RGBA8, size, 1,
                                         QRhiTexture::RenderTarget
                                             | QRhiTexture::UsedAsTransferSource);
        scene->depth = rhi->newRenderBuffer(QRhiRenderBuffer::DepthStencil, size, 1);
        if (!scene->texture->create() || !scene->depth->create()) {
            for (QQuickItem *item : items) {
                item->setParentItem(source->contentItem());
            }
            return false;
        }
        scene->target = rhi->newTextureRenderTarget(
            QRhiTextureRenderTargetDescription(QRhiColorAttachment(scene->texture),
                                               scene->depth));
        scene->pass = scene->target->newCompatibleRenderPassDescriptor();
        scene->target->setRenderPassDescriptor(scene->pass);
        if (!scene->target->create()) {
            for (QQuickItem *item : items) {
                item->setParentItem(source->contentItem());
            }
            return false;
        }
        target = QQuickRenderTarget::fromRhiRenderTarget(scene->target);
        stream.capture = QStringLiteral("rendercontrol-gpu %1 %2")
                             .arg(QString::fromLatin1(rhi->backendName()),
                                  QString::fromUtf8(rhi->driverInfo().deviceName));
    } else {
        scene->software = QImage(size, QImage::Format_RGBA8888_Premultiplied);
        scene->software.fill(Qt::black);
        target = QQuickRenderTarget::fromPaintDevice(&scene->software);
        stream.capture = QStringLiteral("rendercontrol-software");
    }
    // A frame is exactly width×height pixels, whatever the screen scale.
    target.setDevicePixelRatio(1.0);
    scene->window->setRenderTarget(target);

    stream.scene = std::move(scene);
    return true;
}

/// One frame of the windowless scene — on EVERY tick.
///
/// Warning: do not skip a "scene did not change" tick
/// (`sceneChanged`/`renderRequested`). Measurement: with such skipping only
/// ONE frame, black, went into the file in 30 s. The Chromium WebEngineView
/// hands over its next frame only after the scene has been rendered
/// (`updatePaintNode` takes the compositor frame), while the scene change
/// signal arrives after that — the skip waited for the signal, and the
/// signal waited for rendering.
void render_scene(Stream &stream)
{
    SceneRender &scene = *stream.scene;
    scene.control->polishItems();
    scene.control->beginFrame();
    scene.control->sync();
    scene.control->render();
    QRhi *rhi = scene.control->rhi();
    QRhiReadbackResult result;
    if (rhi != nullptr) {
        QRhiResourceUpdateBatch *batch = rhi->nextResourceUpdateBatch();
        batch->readBackTexture(scene.texture, &result);
        scene.control->commandBuffer()->resourceUpdate(batch);
    }
    scene.control->endFrame();
    if (rhi == nullptr) {
        write_frame(stream, scene.software);
        return;
    }
    const QImage image(reinterpret_cast<const uchar *>(result.data.constData()),
                       result.pixelSize.width(), result.pixelSize.height(),
                       QImage::Format_RGBA8888);
    // OpenGL puts the first row at the bottom — the frame file expects it at
    // the top.
    write_frame(stream, rhi->isYUpInFramebuffer() ? image.flipped(Qt::Vertical) : image);
}

/// Unlink the previous file and open a fresh inode (see the explanation in
/// `rexon_wp_mmap_stream_start`).
int open_fresh(const QString &path, std::uint32_t file_mode)
{
    const QByteArray native = path.toLocal8Bit();
    ::unlink(native.constData());
    return ::open(native.constData(), O_RDWR | O_CREAT | O_TRUNC,
                  static_cast<mode_t>(file_mode));
}

} // namespace

std::int64_t rexon_wp_window_id(QObject *window)
{
    auto *w = qobject_cast<QWindow *>(window);
    return w == nullptr ? 0 : static_cast<std::int64_t>(w->winId());
}

bool rexon_wp_mmap_stream_start(QObject *window,
                                QObject *video_output,
                                const QString &path,
                                const QString &generation_path,
                                std::int32_t width,
                                std::int32_t height,
                                std::int32_t interval_ms,
                                std::uint32_t file_mode,
                                bool may_show_window)
{
    QQuickWindow *qwin = quick_window(window);
    if (qwin == nullptr || width <= 0 || height <= 0) {
        return false;
    }
    if (streams().contains(qwin)) {
        return true; // the stream is already running — a repeated start is not needed
    }

    // The frame file is NOT reused between streams — the previous one is
    // unlinked, and the new stream writes into a fresh inode.
    //
    // Why (measured on a fake path, a 64x48 frame = 12288 bytes): previously
    // the file lived for the whole session, and on a wallpaper change the new
    // host opened the same file with a full-size finished frame of the
    // PREVIOUS wallpaper. The shell extension judges a frame's validity by the
    // file size, so in the gap between the stream start and its own first
    // frame (34 ms on the bench, and on a live machine also the whole host
    // process start) it honestly drew someone else's picture. Worse: while the
    // previous host lives out its milliseconds after `kill`, it keeps writing
    // into the same file — in the measurement two hosts alternated frames in
    // the file for seconds.
    //
    // Unlinking cures both cases at once: the previous host keeps writing into
    // an unlinked inode, which no longer reaches the screen. The generation
    // counter is unlinked in the same way for the same reason.
    const int fd = open_fresh(path, file_mode);
    if (fd < 0) {
        return false;
    }
    // The file deliberately stays at ZERO length: the GNOME and Cinnamon
    // extensions skip the tick while the file has fewer than width*height*4
    // bytes and keep their black actor. Growing the file in advance (the former
    // `ftruncate` to a full frame) would declare ready a frame that does not
    // exist yet: zeros in RGBA8888 are a full-screen TRANSPARENT frame. The
    // file reaches full size exactly when `write_frame` finishes writing the
    // last row of the first real frame — so "the frame is ready" and "the file
    // has grown" become one event, and no separate readiness flag is needed.

    Stream stream;
    stream.fd = fd;
    // Without a counter the stream still runs: a reader without the counter
    // behaves like an extension of the earlier build — an occasional torn
    // frame, but not an empty screen.
    stream.gen_fd = generation_path.isEmpty() ? -1 : open_fresh(generation_path, file_mode);
    stream.width = width;
    stream.height = height;
    stream.video = video_output;
    if (video_output != nullptr) {
        stream.capture = QStringLiteral("videosink");
    } else if (!start_scene(stream, qwin)) {
        // The windowless scene did not come up. A snapshot of a shown window —
        // only where the window has nowhere to appear (the core's decision
        // `stream_window_may_show`): on a real platform the window on Mutter would
        // lie on top of the desktop.
        if (!may_show_window) {
            ::close(fd);
            if (stream.gen_fd >= 0) {
                ::close(stream.gen_fd);
            }
            return false;
        }
        qwin->show();
        stream.capture = QStringLiteral("grabwindow");
    }
    write_generation(stream, 0);
    streams().insert(qwin, stream);

    // The descriptor is closed when the window dies, not "sometime at exit":
    // over a session the wallpaper window is recreated (a monitor change, a
    // wallpaper restart), and without this descriptors would pile up.
    QObject::connect(qwin, &QObject::destroyed, qwin, [qwin]() {
        const auto it = streams().find(qwin);
        if (it != streams().end()) {
            if (it->fd >= 0) {
                ::close(it->fd);
            }
            if (it->gen_fd >= 0) {
                ::close(it->gen_fd);
            }
            streams().erase(it);
        }
    });

    auto *timer = new QTimer(qwin);
    timer->setInterval(interval_ms);
    QObject::connect(timer, &QTimer::timeout, qwin, [qwin]() {
        const auto it = streams().find(qwin);
        if (it == streams().end()) {
            return;
        }
        if (!it->video.isNull()) {
            write_frame(*it, video_image(it->video.data(), it->width, it->height));
        } else if (it->scene) {
            render_scene(*it);
        } else {
            write_frame(*it, qwin->grabWindow());
        }
    });
    timer->start();
    return true;
}

QString rexon_wp_mmap_capture(QObject *window)
{
    QQuickWindow *qwin = quick_window(window);
    if (qwin == nullptr) {
        return {};
    }
    const auto it = streams().constFind(qwin);
    return it == streams().constEnd() ? QString() : it->capture;
}

std::int64_t rexon_wp_mmap_nonblack_permille(QObject *window)
{
    QQuickWindow *qwin = quick_window(window);
    if (qwin == nullptr) {
        return -1;
    }
    const auto it = streams().constFind(qwin);
    return it == streams().constEnd() ? -1 : it->nonblack_permille;
}

std::int64_t rexon_wp_mmap_frames(QObject *window)
{
    QQuickWindow *qwin = quick_window(window);
    if (qwin == nullptr) {
        return -1;
    }
    const auto it = streams().constFind(qwin);
    return it == streams().constEnd() ? -1 : it->frames;
}

std::int64_t rexon_wp_mmap_center_rgb(QObject *window)
{
    QQuickWindow *qwin = quick_window(window);
    if (qwin == nullptr) {
        return -1;
    }
    const auto it = streams().constFind(qwin);
    return it == streams().constEnd() ? -1 : it->center_rgb;
}

// ── Flash mode (SWF) ──────────────────────────────────────────────────────
// Qt only creates the window and hands out the native handles; buffers are
// put into the surface by wgpu (Ruffle, the rexon-swf crate). A
// RasterSurface without a QBackingStore: Qt does not draw into the surface
// — the bench on KWin (layer) and Xwayland showed no buffer conflict.

std::int32_t rexon_wp_screen_count()
{
    return QGuiApplication::instance() == nullptr
        ? 0
        : static_cast<std::int32_t>(QGuiApplication::screens().size());
}

void rexon_wp_set_quit_on_last_window_closed(bool enabled)
{
    QGuiApplication::setQuitOnLastWindowClosed(enabled);
}

QObject *rexon_wp_swf_window_new(std::int32_t width,
                                 std::int32_t height,
                                 std::int32_t screen_index)
{
    const QList<QScreen *> screens = QGuiApplication::screens();
    QScreen *screen = screen_index >= 0 && screen_index < screens.size()
        ? screens.at(screen_index)
        : QGuiApplication::primaryScreen();

    auto *window = new QWindow();
    window->setSurfaceType(QSurface::RasterSurface);
    window->setFlags(Qt::FramelessWindowHint);
    // Warning: BEFORE showing: LayerShellQt reads the layer surface's screen
    // from the window, and on X11 the position decides which monitor gets the
    // wallpaper.
    if (screen != nullptr) {
        window->setScreen(screen);
    }
    if (width <= 0 || height <= 0) {
        const QRect geometry = screen ? screen->geometry() : QRect(0, 0, 1920, 1080);
        window->setGeometry(geometry);
    } else {
        window->resize(QSize(width, height));
    }
    return window;
}

void rexon_wp_swf_window_show(QObject *object)
{
    if (auto *window = qobject_cast<QWindow *>(object)) {
        window->show();
    }
}

void rexon_wp_swf_window_close(QObject *object)
{
    if (auto *window = qobject_cast<QWindow *>(object)) {
        window->destroy();
        // deleteLater, not delete: the window may have got into the event queue,
        // and the Flash mode loop spins it by itself (rexon_wp_process_events).
        window->deleteLater();
    }
}

bool rexon_wp_swf_window_exposed(QObject *object)
{
    auto *window = qobject_cast<QWindow *>(object);
    return window != nullptr && window->isExposed();
}

std::int32_t rexon_wp_swf_window_width(QObject *object)
{
    auto *window = qobject_cast<QWindow *>(object);
    return window ? qRound(window->width() * window->devicePixelRatio()) : 0;
}

std::int32_t rexon_wp_swf_window_height(QObject *object)
{
    auto *window = qobject_cast<QWindow *>(object);
    return window ? qRound(window->height() * window->devicePixelRatio()) : 0;
}

void rexon_wp_process_events()
{
    QCoreApplication::processEvents(QEventLoop::AllEvents, 2);
}

std::size_t rexon_wp_wl_display()
{
#if QT_CONFIG(wayland)
    if (qGuiApp != nullptr) {
        if (auto *wayland = qGuiApp->nativeInterface<QNativeInterface::QWaylandApplication>()) {
            return reinterpret_cast<std::size_t>(wayland->display());
        }
    }
#endif
    return 0;
}

std::size_t rexon_wp_wl_surface(QObject *object)
{
    auto *window = qobject_cast<QWindow *>(object);
    QPlatformNativeInterface *native = QGuiApplication::platformNativeInterface();
    if (window == nullptr || native == nullptr) {
        return 0;
    }
    return reinterpret_cast<std::size_t>(native->nativeResourceForWindow("surface", window));
}

std::size_t rexon_wp_xcb_connection()
{
#if QT_CONFIG(xcb)
    if (qGuiApp != nullptr) {
        if (auto *x11 = qGuiApp->nativeInterface<QNativeInterface::QX11Application>()) {
            return reinterpret_cast<std::size_t>(x11->connection());
        }
    }
#endif
    return 0;
}
