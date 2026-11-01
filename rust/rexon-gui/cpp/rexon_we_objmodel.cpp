#include "rexon_we_objmodel.h"

#include <QtCore/QCoreApplication>
#include <QtCore/QObject>
#include <QtCore/QVariant>

#include "rexon-gui/src/we_objmodel.cxxqt.h"

namespace {
template <typename T>
QObject* make_child(QObject* parent)
{
    auto* child = new T();
    // setParent, а не аргумент конструктора: сгенерированный cxx-qt класс
    // конструктора с родителем не имеет.
    if (parent != nullptr)
        child->setParent(parent);
    return child;
}
}  // namespace

QObject* rexon_new_renderer(QObject* parent)
{
    return make_child<Renderer>(parent);
}

QObject* rexon_new_display(QObject* parent)
{
    return make_child<Display>(parent);
}

QObject* rexon_new_canvas(QObject* parent)
{
    return make_child<Canvas>(parent);
}

QObject* rexon_new_gpu(QObject* parent)
{
    return make_child<Gpu>(parent);
}

QObject* rexon_new_renderer_manager(QObject* parent)
{
    return make_child<RendererManager>(parent);
}

QObject* rexon_new_display_manager(QObject* parent)
{
    return make_child<DisplayManager>(parent);
}

QObject* rexon_new_library_manager(QObject* parent)
{
    return make_child<LibraryManager>(parent);
}

QObject* rexon_new_gpu_manager(QObject* parent)
{
    return make_child<GpuManager>(parent);
}

QObject* rexon_new_presentation_manager(QObject* parent)
{
    return make_child<PresentationManager>(parent);
}

QObject* rexon_new_now_playing_model(QObject* parent)
{
    return make_child<NowPlayingModel>(parent);
}

QString rexon_we_vendor_tr(const QString& context, const QString& source)
{
    return QCoreApplication::translate(context.toUtf8().constData(),
                                       source.toUtf8().constData());
}

void rexon_delete_child_later(QObject* object)
{
    if (object != nullptr)
        object->deleteLater();
}

QVariant rexon_object_to_variant(QObject* object)
{
    return QVariant::fromValue(object);
}
