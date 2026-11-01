// Qt-примитивы для картинки плитки местной темы.
//
// Решения (что нарисовать, цвета, вся геометрия, какие файлы темы взять)
// принимает rexon-core::local_preview и отдаёт списком операций; исполняет
// их rust/rexon-gui/src/local_preview_render.rs. Здесь — только то, чего в
// cxx-qt-lib 0.9 нет вовсе: QPainter поверх QImage (у биндинга нет
// begin()/устройства), drawText с прямоугольником и выравниванием,
// QLinearGradient как кисть заливки, чтение картинок (QImageReader) и
// рисунков SVG по частям (QSvgRenderer).
//
// Всё здесь можно звать из любого потока: холст — QImage, не QPixmap.
#pragma once

#include <cstdint>

extern "C" {

// Холст: QImage заданного размера, залитый `base_hex` (строка вида
// "#1e2d3d"), и открытый на нём QPainter со сглаживанием фигур и текста.
void *rexon_lp_canvas_new(int32_t width, int32_t height, const char *base_hex);

// p.fillRect(QRectF(x, y, w, h), QColor(r, g, b, a))
void rexon_lp_fill(void *canvas, double x, double y, double w, double h,
                   uint8_t r, uint8_t g, uint8_t b, uint8_t a);

// p.fillRect(QRectF(..), QLinearGradient(x, y, x, y + h)) со стопами
// `top_hex` на 0.0 и `bottom_hex` на 1.0.
void rexon_lp_gradient(void *canvas, double x, double y, double w, double h,
                       const char *top_hex, const char *bottom_hex);

// p.drawText(QRectF(..), AlignHCenter|AlignVCenter, text) шрифтом кегля
// `point_size` цветом (r, g, b, a).
void rexon_lp_text(void *canvas, double x, double y, double w, double h,
                   const char *text, int32_t point_size, uint8_t r, uint8_t g,
                   uint8_t b, uint8_t a);

// Размер картинки из файла (PNG, JPEG, SVG — всё, что читает QImageReader).
// false — файл не читается как картинка.
bool rexon_lp_image_size(const char *path, int32_t *out_width,
                         int32_t *out_height);

// Картинка из файла, растянутая на прямоугольник (x, y, w, h) и обрезанная
// прямоугольником (cx, cy, cw, ch). Куда и в каком размере — решает
// вызывающая сторона; здесь только чтение файла и отрисовка.
void rexon_lp_image(void *canvas, const char *path, double x, double y,
                    double w, double h, double cx, double cy, double cw,
                    double ch);

// Картинка из файла в своём размере, повторённая плиткой по прямоугольнику
// (x, y, w, h) от его левого верхнего угла; лишнее обрезается.
void rexon_lp_image_tiled(void *canvas, const char *path, double x, double y,
                          double w, double h);

// Размер части `element` рисунка SVG (пустое имя — рисунок целиком).
// false — файла или части нет.
bool rexon_lp_svg_size(void *canvas, const char *path, const char *element,
                       double *out_width, double *out_height);

// Часть `element` рисунка SVG (пустое имя — рисунок целиком), растянутая на
// прямоугольник (x, y, w, h).
void rexon_lp_svg(void *canvas, const char *path, const char *element,
                  double x, double y, double w, double h);

// Точки RGBA8888 (width * height * 4 байта, не умноженные на прозрачность)
// левым верхним углом в (x, y).
void rexon_lp_pixels(void *canvas, const uint8_t *rgba, int32_t width,
                     int32_t height, double x, double y);

// Открыть отсечение прямоугольником (x, y, w, h) со скруглёнными углами
// (радиусы по углам; все нули — простой прямоугольник). Действует вместе с
// уже открытыми, до парного rexon_lp_clip_pop.
void rexon_lp_clip_push(void *canvas, double x, double y, double w, double h,
                        double top_left, double top_right, double bottom_left,
                        double bottom_right);

// Закрыть последнее открытое отсечение; лишний вызов ничего не делает.
void rexon_lp_clip_pop(void *canvas);

// Отрезок толщиной `width` между серединами точек (x1, y1) и (x2, y2).
void rexon_lp_line(void *canvas, double x1, double y1, double x2, double y2,
                   double width, uint8_t r, uint8_t g, uint8_t b, uint8_t a);

// Дуга эллипса, вписанного в (x, y, w, h): от угла `start` на `extent`
// градусов по часовой стрелке, ноль — вверх. filled — закрашенный сектор.
void rexon_lp_arc(void *canvas, double x, double y, double w, double h,
                  double start, double extent, bool filled, uint8_t r,
                  uint8_t g, uint8_t b, uint8_t a);

// Переход через `count` цветов (по четыре байта RGBA) равными долями:
// direction 0 — слева направо, 1 — сверху вниз, 2 — из левого верхнего угла
// в правый нижний.
void rexon_lp_shade(void *canvas, double x, double y, double w, double h,
                    int32_t direction, const uint8_t *rgba, int32_t count);

// Картинка из файла, растянутая на (x, y, w, h) без сохранения сторон либо,
// при tile, повторённая в своём размере; alpha — непрозрачность 0–1. При
// colorize яркость каждой точки ведёт от чёрного через (r, g, b) к белому.
void rexon_lp_picture(void *canvas, const char *path, double x, double y,
                      double w, double h, bool tile, double alpha,
                      bool colorize, uint8_t r, uint8_t g, uint8_t b);

// Строка по центру (x, y, w, h) шрифтом высотой pixel_size точек экрана.
void rexon_lp_label(void *canvas, double x, double y, double w, double h,
                    const char *text, int32_t pixel_size, bool bold, uint8_t r,
                    uint8_t g, uint8_t b, uint8_t a);

// Закрыть QPainter и записать готовый кадр файлом PNG по пути `path`.
// Холст при этом уничтожается. true — файл записан. Плитке на QML нужен
// именно ФАЙЛ: Image берёт источник по адресу.
bool rexon_lp_finish_png(void *canvas, const char *path);

} // extern "C"
