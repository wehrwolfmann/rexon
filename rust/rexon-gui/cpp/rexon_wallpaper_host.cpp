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
// Полупубличный с Qt 6.6 API видеокарты Qt (каталог с версией добавляет
// build.rs): текстура-цель QQuickRenderControl и чтение её обратно.
#include <rhi/qrhi.h>
#include <QTimer>
#include <QVariant>
#include <QVideoFrame>
#include <QVideoSink>
#include <QWindow>

#include <QGuiApplication>
#include <QScreen>
#include <QtGui/qguiapplication_platform.h>
// ⚠️ Приватный, но стабильный с Qt 5 интерфейс платформы: публичного способа
// взять wl_surface окна в Qt 6.11 нет (wl_display и xcb — публичные ниже).
// Каталоги с версией Qt добавляет build.rs.
#include <qpa/qplatformnativeinterface.h>

namespace
{

/// Сцена Qt Quick без окна: `QQuickRenderControl` рисует содержимое окна QML
/// в свою цель, кадр читается обратно.
///
/// Зачем (замер 15.09.2026, 1920×1080, 30 к/с). Прежде веб-обои в потоке
/// кадров снимались `grabWindow` с показанного окна на платформе `offscreen`:
/// там у Qt нет EGL, Qt Quick уходил в программный рендер, Chromium — без
/// видеокарты: CSS 17 к/с при 157 % CPU, WebGL нет вовсе. На настоящей
/// платформе сеанса (wayland/xcb) окно показывать нельзя — на Mutter оно
/// выходит поверх стола, — а QQuickRenderControl окна не создаёт вовсе (за
/// прогон ни одной `wl_surface`) и рисует на видеокарте: CSS 43 % CPU, WebGL
/// 25 % с живым кадром, страница идёт с частотой потока. Штатный путь Qt
/// «рисовать без экрана» (документация QQuickRenderControl, пример
/// rendercontrol_rhi).
struct SceneRender {
    QQuickRenderControl *control = nullptr;
    QQuickWindow *window = nullptr;
    QRhiTexture *texture = nullptr;
    QRhiRenderBuffer *depth = nullptr;
    QRhiTextureRenderTarget *target = nullptr;
    QRhiRenderPassDescriptor *pass = nullptr;
    /// Цель программного рендера — когда видеокарты у Qt нет (платформа
    /// `offscreen` без GL).
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

/// Состояние одного потока кадров. Живёт ровно столько, сколько окно:
/// таймер — потомок окна, а запись в карте снимается по `destroyed`.
struct Stream {
    int fd = -1;
    /// Счётчик поколения кадра (seqlock для читателя без замка); -1 — нет.
    int gen_fd = -1;
    std::uint64_t generation = 0;
    std::int32_t width = 0;
    std::int32_t height = 0;
    std::int64_t frames = 0;
    /// Центральный пиксель последнего записанного кадра, 0xRRGGBB; -1 — не было.
    std::int64_t center_rgb = -1;
    /// Доля не-чёрных пикселей последнего кадра, ‰ (выборка); -1 — не было.
    std::int64_t nonblack_permille = -1;
    /// Источник кадров — VideoOutput; пусто — сцена Qt Quick.
    QPointer<QObject> video;
    /// Сцена без окна (веб); пусто при видео и при снимке окна.
    std::shared_ptr<SceneRender> scene;
    /// Чем снимается кадр — для самопроверки и журнала.
    QString capture;
};

QHash<QObject *, Stream> &streams()
{
    static QHash<QObject *, Stream> map;
    return map;
}

/// Окно QtQuick: сам объект либо первый такой потомок. Кадры снимаются
/// только с него — `grabWindow` есть лишь у QQuickWindow.
QQuickWindow *quick_window(QObject *object)
{
    if (auto *w = qobject_cast<QQuickWindow *>(object)) {
        return w;
    }
    return object == nullptr ? nullptr : object->findChild<QQuickWindow *>();
}

/// Текущий кадр проигрывателя прямо из приёмника VideoOutput, заполняющий
/// кадр потока целиком с обрезкой краёв — как `VideoOutput.PreserveAspectCrop`
/// в WallpaperVideo.qml.
///
/// Зачем не `grabWindow`. В режиме потока кадров хост живёт на платформе
/// `offscreen` (иначе на GNOME/Cinnamon-Wayland его окно выходит поверх
/// рабочего стола), а там Qt Quick рисует только программным рендером, и
/// VideoOutput в нём не рисует ничего: замер — 186 кадров в файле, все
/// чёрные (0,0,0), при любых QSG_RHI_BACKEND/QT_QUICK_BACKEND. Приёмник же
/// получает кадры декодера независимо от рендера, и `QVideoFrame::toImage`
/// на той же платформе отдаёт настоящий цвет.
QImage video_image(QObject *video_output, std::int32_t width, std::int32_t height)
{
    auto *sink = qobject_cast<QVideoSink *>(
        video_output->property("videoSink").value<QObject *>());
    if (sink == nullptr) {
        return {};
    }
    const QVideoFrame frame = sink->videoFrame();
    if (!frame.isValid()) {
        return {}; // декодер ещё не отдал первый кадр — файл остаётся коротким
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

/// Записать счётчик поколения (u64 little-endian — раскладку задаёт ядро,
/// `wallpaper_mmap::frame_generation_bytes`).
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

/// Один кадр в файл. Пишется целиком и под исключительным замком — для
/// читателей, которые умеют брать разделяемый. Расширения оболочки не умеют
/// (у GJS нет `flock`), им тот же кадр огорожен счётчиком поколения:
/// нечётный — кадр пишется, чётный — записан целиком
/// (`wallpaper_mmap::frame_generation_path`).
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
    stream.generation += 1; // нечётное: кадр пишется
    write_generation(stream, stream.generation);
    ::lseek(stream.fd, 0, SEEK_SET);
    // Построчно, а не одним куском: у QImage строки выровнены, и
    // bytesPerLine может быть больше ширины кадра — запись целиком уехала бы
    // со сдвигом, и расширение показало бы «косую» картинку.
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
    stream.generation += 1; // чётное: кадр записан целиком
    write_generation(stream, stream.generation);
    ::flock(stream.fd, LOCK_UN);
    stream.frames += 1;
    const QRgb center = image.pixel(stream.width / 2, stream.height / 2);
    stream.center_rgb = (static_cast<std::int64_t>(qRed(center)) << 16)
        | (qGreen(center) << 8) | qBlue(center);
    // Центр бывает чёрным и у живой страницы, поэтому самопроверке нужна ещё
    // доля не-чёрного по всему кадру. Выборка сеткой 16×16: ~8 тыс. точек.
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

/// Перенести содержимое окна QML в сцену QQuickRenderControl и приготовить
/// цель. `false` — Qt не поднял сцену (содержимое возвращено на место).
bool start_scene(Stream &stream, QQuickWindow *source)
{
    auto scene = std::make_shared<SceneRender>();
    scene->control = new QQuickRenderControl;
    scene->window = new QQuickWindow(scene->control);
    scene->window->setColor(source->color());
    const QSize size(stream.width, stream.height);
    scene->window->resize(size);
    // ⚠️ `resize` окна без платформенного окна до `contentItem` не доходит (он
    // остаётся 0×0), а WebEngineView привязан к нему `anchors.fill`. Замер:
    // без этой строки Chromium весь прогон видел `innerWidth=0`, холст WebGL
    // вставал 0×0 и кадр был чёрным.
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
    // Кадр — ровно width×height пикселей, какой бы масштаб ни был у экрана.
    target.setDevicePixelRatio(1.0);
    scene->window->setRenderTarget(target);

    stream.scene = std::move(scene);
    return true;
}

/// Один кадр сцены без окна — на КАЖДОМ тике.
///
/// ⚠️ Не пропускать тик «сцена не менялась» (`sceneChanged`/`renderRequested`).
/// Замер: с таким пропуском за 30 с в файл ушёл ОДИН кадр, чёрный. Следующий
/// кадр Chromium WebEngineView отдаёт только после того, как сцену отрисовали
/// (`updatePaintNode` забирает кадр компоситора), а сигнал об изменении
/// сцены приходит уже после этого — пропуск ждал сигнала, сигнал ждал отрисовки.
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
    // OpenGL кладёт первую строку внизу — файл кадра ждёт её сверху.
    write_frame(stream, rhi->isYUpInFramebuffer() ? image.flipped(Qt::Vertical) : image);
}

/// Отцепить прежний файл и открыть свежий inode (см. пояснение в
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
        return true; // поток уже идёт — повторный запуск не нужен
    }

    // Файл кадра НЕ переиспользуется между потоками — прежний отцепляется, и
    // новый поток пишет в свежий inode.
    //
    // Зачем (замер на подставном пути, кадр 64x48 = 12288 байт): прежде
    // файл жил весь сеанс, и при смене обоев новый хост открывал тот же
    // файл с готовым кадром ПРЕЖНИХ обоев полного размера. Расширение
    // оболочки годность кадра определяет по размеру файла, поэтому в
    // промежутке между стартом потока и первым своим кадром (34 мс на
    // стенде, а на живой машине ещё и весь запуск процесса хоста) оно
    // честно рисовало чужую картинку. Хуже того: пока прежний хост
    // доживает свои миллисекунды после `kill`, он продолжает писать в тот
    // же файл — на замере два хоста чередовали кадры в файле секундами.
    //
    // Отцепление лечит оба случая разом: прежний хост дописывает в
    // отцепленный inode, до экрана это уже не доходит. Счётчик поколения
    // отцепляется так же и по той же причине.
    const int fd = open_fresh(path, file_mode);
    if (fd < 0) {
        return false;
    }
    // Файл намеренно остаётся НУЛЕВОЙ длины: расширения GNOME и Cinnamon
    // пропускают тик, пока в файле меньше width*height*4 байт, и держат свой
    // чёрный актёр. Раздуть файл заранее (прежний `ftruncate` на полный
    // кадр) значило бы объявить готовым кадр, которого ещё нет: нули в
    // RGBA8888 — это ПРОЗРАЧНЫЙ кадр во весь экран. Полного размера файл
    // достигает ровно тогда, когда `write_frame` допишет последнюю строку
    // первого настоящего кадра, — то есть «кадр готов» и «файл дорос»
    // становятся одним событием, и отдельного признака готовности не нужно.

    Stream stream;
    stream.fd = fd;
    // Без счётчика поток всё равно идёт: читатель без счётчика ведёт себя
    // как расширение прежней сборки — изредка рваный кадр, но не пустой экран.
    stream.gen_fd = generation_path.isEmpty() ? -1 : open_fresh(generation_path, file_mode);
    stream.width = width;
    stream.height = height;
    stream.video = video_output;
    if (video_output != nullptr) {
        stream.capture = QStringLiteral("videosink");
    } else if (!start_scene(stream, qwin)) {
        // Сцена без окна не поднялась. Снимок показанного окна — только там,
        // где окну некуда выйти (решение ядра `stream_window_may_show`): на
        // настоящей платформе окно на Mutter легло бы поверх стола.
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

    // Дескриптор закрывается по смерти окна, а не «когда-нибудь при выходе»:
    // за сеанс окно обоев пересоздаётся (смена монитора, перезапуск обоев), и
    // без этого дескрипторы копились бы.
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

// ── Flash-режим (SWF) ─────────────────────────────────────────────────────
// Qt только создаёт окно и отдаёт native-хэндлы; буферы в поверхность кладёт
// wgpu (Ruffle, крейт rexon-swf). RasterSurface без QBackingStore: Qt в
// поверхность не рисует — стенд на KWin (слой) и Xwayland конфликта буферов
// не показал.

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
    // ⚠️ ДО показа: экран поверхности слоя LayerShellQt читает из окна, а
    // на X11 позиция решает, какому монитору достанутся обои.
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
        // deleteLater, а не delete: окно могло попасть в очередь событий, а
        // цикл Flash-режима крутит её сам (rexon_wp_process_events).
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
