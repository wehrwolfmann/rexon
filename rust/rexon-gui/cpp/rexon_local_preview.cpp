#include "rexon_local_preview.h"

#include <QBrush>
#include <QColor>
#include <QFont>
#include <QFontMetricsF>
#include <QHash>
#include <QImage>
#include <QImageReader>
#include <QLinearGradient>
#include <QPainter>
#include <QPainterPath>
#include <QPen>
#include <QRectF>
#include <QString>
#include <QSvgRenderer>

#include <memory>

namespace {

// Холст живёт между вызовами: кадр рисуется по одной операции за раз.
// Рисуется QImage, а не QPixmap: QImage можно рисовать в любом потоке, а
// кадры плиток готовятся вне потока интерфейса.
struct Canvas {
    QImage image;
    QPainter painter;
    // Рамка из рисунка SVG — это девять отрисовок одного файла: разбирать
    // его каждый раз заново незачем.
    QHash<QString, std::shared_ptr<QSvgRenderer>> svgs;

    QSvgRenderer *svg(const char *path)
    {
        const QString key = QString::fromUtf8(path);
        auto found = svgs.find(key);
        if (found == svgs.end()) {
            found = svgs.insert(key, std::make_shared<QSvgRenderer>(key));
        }
        return found.value()->isValid() ? found.value().get() : nullptr;
    }

    // Сколько отсечений открыто: лишнее закрытие не должно снять состояние,
    // которого холст не сохранял.
    int clips = 0;

    Canvas(int width, int height, const char *base_hex)
        : image(width, height, QImage::Format_ARGB32_Premultiplied)
    {
        image.fill(QColor(QString::fromUtf8(base_hex)));
        painter.begin(&image);
        painter.setRenderHint(QPainter::Antialiasing, true);
        painter.setRenderHint(QPainter::TextAntialiasing, true);
    }
};

// Выравнивание текста — одно на все текстовые операции.
constexpr int kTextAlign = Qt::AlignHCenter | Qt::AlignVCenter;

} // namespace

void *rexon_lp_canvas_new(int32_t width, int32_t height, const char *base_hex)
{
    return new Canvas(width, height, base_hex);
}

void rexon_lp_fill(void *canvas, double x, double y, double w, double h,
                   uint8_t r, uint8_t g, uint8_t b, uint8_t a)
{
    auto *c = static_cast<Canvas *>(canvas);
    c->painter.fillRect(QRectF(x, y, w, h), QColor(r, g, b, a));
}

void rexon_lp_gradient(void *canvas, double x, double y, double w, double h,
                       const char *top_hex, const char *bottom_hex)
{
    auto *c = static_cast<Canvas *>(canvas);
    QLinearGradient grad(x, y, x, y + h);
    grad.setColorAt(0.0, QColor(QString::fromUtf8(top_hex)));
    grad.setColorAt(1.0, QColor(QString::fromUtf8(bottom_hex)));
    c->painter.fillRect(QRectF(x, y, w, h), grad);
}

void rexon_lp_text(void *canvas, double x, double y, double w, double h,
                   const char *text, int32_t point_size, uint8_t r, uint8_t g,
                   uint8_t b, uint8_t a)
{
    auto *c = static_cast<Canvas *>(canvas);
    QFont font;
    font.setPointSize(point_size);
    c->painter.setFont(font);
    c->painter.setPen(QColor(r, g, b, a));
    c->painter.drawText(QRectF(x, y, w, h), kTextAlign,
                        QString::fromUtf8(text));
}

bool rexon_lp_image_size(const char *path, int32_t *out_width,
                         int32_t *out_height)
{
    QImageReader reader(QString::fromUtf8(path));
    const QSize size = reader.size();
    if (!reader.canRead() || !size.isValid() || size.isEmpty()) {
        return false;
    }
    *out_width = size.width();
    *out_height = size.height();
    return true;
}

void rexon_lp_image(void *canvas, const char *path, double x, double y,
                    double w, double h, double cx, double cy, double cw,
                    double ch)
{
    auto *c = static_cast<Canvas *>(canvas);
    QImageReader reader(QString::fromUtf8(path));
    // Рисунок SVG читается сразу в нужном размере — иначе его растянуло бы
    // из родного, обычно крошечного.
    reader.setScaledSize(QSize(qRound(w), qRound(h)));
    const QImage image = reader.read();
    if (image.isNull()) {
        return;
    }
    c->painter.save();
    c->painter.setClipRect(QRectF(cx, cy, cw, ch));
    c->painter.setRenderHint(QPainter::SmoothPixmapTransform, true);
    c->painter.drawImage(QRectF(x, y, w, h), image);
    c->painter.restore();
}

void rexon_lp_image_tiled(void *canvas, const char *path, double x, double y,
                          double w, double h)
{
    auto *c = static_cast<Canvas *>(canvas);
    const QImage image = QImageReader(QString::fromUtf8(path)).read();
    if (image.isNull()) {
        return;
    }
    const QRectF rect(x, y, w, h);
    c->painter.save();
    c->painter.setClipRect(rect);
    // Кисть-картинка кладётся от начала координат холста: сдвигаем её
    // начало в угол прямоугольника, иначе плитка шла бы «с середины».
    c->painter.setBrushOrigin(rect.topLeft());
    c->painter.fillRect(rect, QBrush(image));
    c->painter.restore();
}

bool rexon_lp_svg_size(void *canvas, const char *path, const char *element,
                       double *out_width, double *out_height)
{
    auto *c = static_cast<Canvas *>(canvas);
    QSvgRenderer *svg = c->svg(path);
    if (svg == nullptr) {
        return false;
    }
    const QString id = QString::fromUtf8(element);
    QSizeF size;
    if (id.isEmpty()) {
        size = svg->defaultSize();
    } else if (svg->elementExists(id)) {
        size = svg->transformForElement(id)
                   .mapRect(svg->boundsOnElement(id))
                   .size();
    } else {
        return false;
    }
    *out_width = size.width();
    *out_height = size.height();
    return true;
}

void rexon_lp_svg(void *canvas, const char *path, const char *element,
                  double x, double y, double w, double h)
{
    auto *c = static_cast<Canvas *>(canvas);
    QSvgRenderer *svg = c->svg(path);
    if (svg == nullptr) {
        return;
    }
    const QString id = QString::fromUtf8(element);
    if (id.isEmpty()) {
        svg->render(&c->painter, QRectF(x, y, w, h));
    } else {
        svg->render(&c->painter, id, QRectF(x, y, w, h));
    }
}

void rexon_lp_pixels(void *canvas, const uint8_t *rgba, int32_t width,
                     int32_t height, double x, double y)
{
    auto *c = static_cast<Canvas *>(canvas);
    const QImage image(rgba, width, height, width * 4,
                       QImage::Format_RGBA8888);
    c->painter.drawImage(QPointF(x, y), image);
}

void rexon_lp_clip_push(void *canvas, double x, double y, double w, double h,
                        double top_left, double top_right, double bottom_left,
                        double bottom_right)
{
    auto *c = static_cast<Canvas *>(canvas);
    const QRectF rect(x, y, w, h);
    c->painter.save();
    c->clips += 1;
    if (top_left <= 0.0 && top_right <= 0.0 && bottom_left <= 0.0
        && bottom_right <= 0.0) {
        c->painter.setClipRect(rect, Qt::IntersectClip);
        return;
    }
    // Очертание со своим радиусом у каждого угла: по часовой стрелке от
    // левого верхнего.
    QPainterPath path;
    path.moveTo(rect.left() + top_left, rect.top());
    path.lineTo(rect.right() - top_right, rect.top());
    path.arcTo(rect.right() - 2 * top_right, rect.top(), 2 * top_right,
               2 * top_right, 90, -90);
    path.lineTo(rect.right(), rect.bottom() - bottom_right);
    path.arcTo(rect.right() - 2 * bottom_right,
               rect.bottom() - 2 * bottom_right, 2 * bottom_right,
               2 * bottom_right, 0, -90);
    path.lineTo(rect.left() + bottom_left, rect.bottom());
    path.arcTo(rect.left(), rect.bottom() - 2 * bottom_left, 2 * bottom_left,
               2 * bottom_left, 270, -90);
    path.lineTo(rect.left(), rect.top() + top_left);
    path.arcTo(rect.left(), rect.top(), 2 * top_left, 2 * top_left, 180, -90);
    path.closeSubpath();
    c->painter.setClipPath(path, Qt::IntersectClip);
}

void rexon_lp_clip_pop(void *canvas)
{
    auto *c = static_cast<Canvas *>(canvas);
    if (c->clips > 0) {
        c->clips -= 1;
        c->painter.restore();
    }
}

void rexon_lp_line(void *canvas, double x1, double y1, double x2, double y2,
                   double width, uint8_t r, uint8_t g, uint8_t b, uint8_t a)
{
    auto *c = static_cast<Canvas *>(canvas);
    c->painter.save();
    c->painter.setPen(QPen(QColor(r, g, b, a), width, Qt::SolidLine,
                           Qt::FlatCap));
    // Координаты называют точки, линия идёт через их середины.
    c->painter.drawLine(QPointF(x1 + 0.5, y1 + 0.5),
                        QPointF(x2 + 0.5, y2 + 0.5));
    c->painter.restore();
}

void rexon_lp_arc(void *canvas, double x, double y, double w, double h,
                  double start, double extent, bool filled, uint8_t r,
                  uint8_t g, uint8_t b, uint8_t a)
{
    auto *c = static_cast<Canvas *>(canvas);
    // У Qt углы идут против часовой стрелки от «трёх часов» в 1/16 градуса;
    // сюда приходят по часовой от «двенадцати».
    const int qt_start = qRound((90.0 - start) * 16.0);
    const int qt_span = qRound(-extent * 16.0);
    c->painter.save();
    if (filled) {
        c->painter.setPen(Qt::NoPen);
        c->painter.setBrush(QColor(r, g, b, a));
        c->painter.drawPie(QRectF(x, y, w, h), qt_start, qt_span);
    } else {
        c->painter.setPen(QPen(QColor(r, g, b, a), 1.0));
        c->painter.setBrush(Qt::NoBrush);
        c->painter.drawArc(QRectF(x + 0.5, y + 0.5, w, h), qt_start, qt_span);
    }
    c->painter.restore();
}

void rexon_lp_shade(void *canvas, double x, double y, double w, double h,
                    int32_t direction, const uint8_t *rgba, int32_t count)
{
    auto *c = static_cast<Canvas *>(canvas);
    if (count < 2) {
        return;
    }
    // 0 — слева направо, 1 — сверху вниз, 2 — из угла в угол.
    const QPointF from(x, y);
    const QPointF to(direction == 1 ? x : x + w, direction == 0 ? y : y + h);
    QLinearGradient grad(from, to);
    for (int32_t i = 0; i < count; ++i) {
        const uint8_t *p = rgba + i * 4;
        grad.setColorAt(double(i) / double(count - 1),
                        QColor(p[0], p[1], p[2], p[3]));
    }
    c->painter.fillRect(QRectF(x, y, w, h), grad);
}

void rexon_lp_picture(void *canvas, const char *path, double x, double y,
                      double w, double h, bool tile, double alpha,
                      bool colorize, uint8_t r, uint8_t g, uint8_t b)
{
    auto *c = static_cast<Canvas *>(canvas);
    QImageReader reader(QString::fromUtf8(path));
    if (!tile) {
        // Рисунок SVG читается сразу в нужном размере.
        reader.setScaledSize(QSize(qMax(1, qRound(w)), qMax(1, qRound(h))));
    }
    QImage image = reader.read();
    if (image.isNull()) {
        return;
    }
    if (colorize) {
        // Яркость точки ведёт от чёрного через названный цвет к белому;
        // прозрачность точки остаётся своей.
        image = image.convertToFormat(QImage::Format_ARGB32);
        const int target[3] = {r, g, b};
        for (int row = 0; row < image.height(); ++row) {
            QRgb *line = reinterpret_cast<QRgb *>(image.scanLine(row));
            for (int col = 0; col < image.width(); ++col) {
                const QRgb px = line[col];
                const double level = (0.30 * qRed(px) + 0.59 * qGreen(px)
                                      + 0.11 * qBlue(px)) / 255.0;
                int out[3];
                for (int i = 0; i < 3; ++i) {
                    const double v = level <= 0.5
                        ? target[i] * level * 2.0
                        : target[i] + (255.0 - target[i]) * (level - 0.5) * 2.0;
                    out[i] = qBound(0, qRound(v), 255);
                }
                line[col] = qRgba(out[0], out[1], out[2], qAlpha(px));
            }
        }
    }
    const QRectF rect(x, y, w, h);
    c->painter.save();
    c->painter.setOpacity(alpha);
    if (tile) {
        c->painter.setClipRect(rect, Qt::IntersectClip);
        c->painter.setBrushOrigin(rect.topLeft());
        c->painter.fillRect(rect, QBrush(image));
    } else {
        c->painter.setRenderHint(QPainter::SmoothPixmapTransform, true);
        c->painter.drawImage(rect, image);
    }
    c->painter.restore();
}

void rexon_lp_label(void *canvas, double x, double y, double w, double h,
                    const char *text, int32_t pixel_size, bool bold, uint8_t r,
                    uint8_t g, uint8_t b, uint8_t a)
{
    auto *c = static_cast<Canvas *>(canvas);
    QFont font;
    font.setPixelSize(qMax(1, pixel_size));
    font.setBold(bold);
    const QString line = QString::fromUtf8(text);
    // Место под строку рассчитано без настоящего шрифта: если строка вышла
    // шире, шрифт уменьшается так, чтобы она в него вошла.
    const double natural = QFontMetricsF(font).horizontalAdvance(line);
    if (w > 0.0 && natural > w) {
        font.setPixelSize(qMax(6, int(pixel_size * w / natural)));
    }
    c->painter.save();
    c->painter.setFont(font);
    c->painter.setPen(QColor(r, g, b, a));
    // Строка может оказаться шире расчётного места — не обрезаем её им.
    c->painter.drawText(QRectF(x, y, w, h),
                        kTextAlign | Qt::TextDontClip | Qt::TextSingleLine,
                        line);
    c->painter.restore();
}

bool rexon_lp_finish_png(void *canvas, const char *path)
{
    auto *c = static_cast<Canvas *>(canvas);
    c->painter.end();
    const bool ok = c->image.save(QString::fromUtf8(path), "PNG");
    delete c;
    return ok;
}
