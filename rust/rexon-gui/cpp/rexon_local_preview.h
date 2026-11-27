// Qt primitives for the local theme tile picture.
//
// The decisions (what to draw, colours, all geometry, which theme files to
// take) are made by rexon-core::local_preview and handed over as a list of
// operations; they are executed by rust/rexon-gui/src/local_preview_render.rs.
// Here is only what cxx-qt-lib 0.9 lacks entirely: a QPainter over a
// QImage (the binding has no begin()/device), drawText with a rectangle
// and alignment, a QLinearGradient as a fill brush, reading images
// (QImageReader) and SVG drawings in parts (QSvgRenderer).
//
// Everything here can be called from any thread: the canvas is a QImage,
// not a QPixmap.
#pragma once

#include <cstdint>

extern "C" {

// Canvas: a QImage of the given size filled with `base_hex` (a string like
// "#1e2d3d"), with a QPainter open on it with antialiasing for shapes and
// text.
void *rexon_lp_canvas_new(int32_t width, int32_t height, const char *base_hex);

// p.scale(k, k): the next operations are given in points, the canvas is k
// times larger (a full-screen picture).
void rexon_lp_scale(void *canvas, double k);

// p.fillRect(QRectF(x, y, w, h), QColor(r, g, b, a))
void rexon_lp_fill(void *canvas, double x, double y, double w, double h,
                   uint8_t r, uint8_t g, uint8_t b, uint8_t a);

// p.fillRect(QRectF(..), QLinearGradient(x, y, x, y + h)) with the stops
// `top_hex` at 0.0 and `bottom_hex` at 1.0.
void rexon_lp_gradient(void *canvas, double x, double y, double w, double h,
                       const char *top_hex, const char *bottom_hex);

// p.drawText(QRectF(..), AlignHCenter|AlignVCenter, text) in a font of size
// `point_size` and colour (r, g, b, a).
void rexon_lp_text(void *canvas, double x, double y, double w, double h,
                   const char *text, int32_t point_size, uint8_t r, uint8_t g,
                   uint8_t b, uint8_t a);

// Image size from a file (PNG, JPEG, SVG — anything QImageReader reads).
// false means the file cannot be read as an image.
bool rexon_lp_image_size(const char *path, int32_t *out_width,
                         int32_t *out_height);

// An image from a file stretched over the rectangle (x, y, w, h) and
// clipped by the rectangle (cx, cy, cw, ch). Where and at what size is
// decided by the caller; here there is only reading the file and drawing.
void rexon_lp_image(void *canvas, const char *path, double x, double y,
                    double w, double h, double cx, double cy, double cw,
                    double ch);

// An image from a file at its own size, tiled over the rectangle
// (x, y, w, h) from its top left corner; the excess is clipped.
void rexon_lp_image_tiled(void *canvas, const char *path, double x, double y,
                          double w, double h);

// Size of part `element` of an SVG drawing (an empty name — the whole
// drawing). false means there is no such file or part.
bool rexon_lp_svg_size(void *canvas, const char *path, const char *element,
                       double *out_width, double *out_height);

// Part `element` of an SVG drawing (an empty name — the whole drawing),
// stretched over the rectangle (x, y, w, h).
void rexon_lp_svg(void *canvas, const char *path, const char *element,
                  double x, double y, double w, double h);

// RGBA8888 pixels (width * height * 4 bytes, not premultiplied by alpha)
// with their top left corner at (x, y).
void rexon_lp_pixels(void *canvas, const uint8_t *rgba, int32_t width,
                     int32_t height, double x, double y);

// Open a clip with the rectangle (x, y, w, h) with rounded corners (radii
// per corner; all zeros — a plain rectangle). Acts together with the
// already open ones, until the paired rexon_lp_clip_pop.
void rexon_lp_clip_push(void *canvas, double x, double y, double w, double h,
                        double top_left, double top_right, double bottom_left,
                        double bottom_right);

// Close the last open clip; an extra call does nothing.
void rexon_lp_clip_pop(void *canvas);

// A segment of thickness `width` between the centres of the points
// (x1, y1) and (x2, y2).
void rexon_lp_line(void *canvas, double x1, double y1, double x2, double y2,
                   double width, uint8_t r, uint8_t g, uint8_t b, uint8_t a);

// An arc of the ellipse inscribed in (x, y, w, h): from angle `start` for
// `extent` degrees clockwise, zero is up. filled — a filled sector.
void rexon_lp_arc(void *canvas, double x, double y, double w, double h,
                  double start, double extent, bool filled, uint8_t r,
                  uint8_t g, uint8_t b, uint8_t a);

// A gradient through `count` colours (four RGBA bytes each) in equal
// shares: direction 0 — left to right, 1 — top to bottom, 2 — from the top
// left corner to the bottom right.
void rexon_lp_shade(void *canvas, double x, double y, double w, double h,
                    int32_t direction, const uint8_t *rgba, int32_t count);

// An image from a file stretched over (x, y, w, h) without preserving the
// aspect ratio or, with tile, repeated at its own size; alpha is the
// opacity 0–1. With colorize the brightness of each pixel leads from black
// through (r, g, b) to white.
void rexon_lp_picture(void *canvas, const char *path, double x, double y,
                      double w, double h, bool tile, double alpha,
                      bool colorize, uint8_t r, uint8_t g, uint8_t b);

// A line centred in (x, y, w, h) in a font pixel_size screen pixels high.
void rexon_lp_label(void *canvas, double x, double y, double w, double h,
                    const char *text, int32_t pixel_size, bool bold, uint8_t r,
                    uint8_t g, uint8_t b, uint8_t a);

// Close the QPainter and write the finished frame as a PNG file at `path`.
// The canvas is destroyed in the process. true means the file is written.
// A QML tile needs exactly a FILE: Image takes its source by address.
bool rexon_lp_finish_png(void *canvas, const char *path);

} // extern "C"
