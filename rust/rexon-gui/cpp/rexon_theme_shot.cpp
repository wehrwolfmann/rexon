#include "rexon_theme_shot.h"

#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QGridLayout>
#include <QHBoxLayout>
#include <QImage>
#include <QLineEdit>
#include <QMetaEnum>
#include <QPainter>
#include <QPalette>
#include <QProgressBar>
#include <QPushButton>
#include <QRadioButton>
#include <QScrollBar>
#include <QSlider>
#include <QSpinBox>
#include <QStyle>
#include <QStyleFactory>
#include <QTabWidget>
#include <QVBoxLayout>

#include <cstring>
#include <memory>

namespace {

// Стиль и палитра — каждому виджету формы: виджет без своего стиля взял бы
// стиль процесса, а не образца.
void applyRecursively(QWidget *widget, QStyle *style, const QPalette &palette)
{
    widget->setPalette(palette);
    widget->setStyle(style);
    for (QObject *child : widget->children()) {
        if (child->isWidgetType()) {
            applyRecursively(static_cast<QWidget *>(child), style, palette);
        }
    }
}

QString text(const char *const *texts, int count, int at)
{
    return at < count ? QString::fromUtf8(texts[at]) : QString();
}

} // namespace

extern "C" int rexon_qt_style_shot(const char *style, const char *const *roles,
                                   const char *const *colors, int32_t count,
                                   const char *const *texts, int32_t text_count,
                                   int32_t form_w, int32_t form_h,
                                   int32_t out_w, int32_t out_h,
                                   int32_t scale, const char *out)
{
    static int argc = 1;
    static char arg0[] = "rexon-theme-shot";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);

    std::unique_ptr<QStyle> st(QStyleFactory::create(QString::fromUtf8(style)));
    if (!st) {
        return REXON_SHOT_NO_STYLE;
    }

    QPalette palette = st->standardPalette();
    const QMetaEnum roleEnum = QMetaEnum::fromType<QPalette::ColorRole>();
    for (int i = 0; i < count; ++i) {
        QByteArray name(roles[i]);
        const bool disabled = name.startsWith("Disabled:");
        if (disabled) {
            name = name.mid(int(std::strlen("Disabled:")));
        }
        bool ok = false;
        const int role = roleEnum.keyToValue(name.constData(), &ok);
        const QColor color(QString::fromUtf8(colors[i]));
        if (!ok || !color.isValid()) {
            continue;
        }
        if (disabled) {
            palette.setColor(QPalette::Disabled, QPalette::ColorRole(role), color);
        } else {
            palette.setColor(QPalette::ColorRole(role), color);
        }
    }
    // Тему (Kvantum) стиль кладёт в палитру сам.
    st->polish(palette);
    // Форма без родительского окна считается неактивной и тусклой: цвета
    // неактивной группы — как у активной.
    for (int i = 0; i < QPalette::NColorRoles; ++i) {
        const auto role = QPalette::ColorRole(i);
        palette.setColor(QPalette::Inactive, role, palette.color(QPalette::Active, role));
    }

    QWidget root;
    root.setAttribute(Qt::WA_DontShowOnScreen);
    root.setAutoFillBackground(true);
    auto *outer = new QHBoxLayout(&root);
    auto *tabs = new QTabWidget;
    auto *page = new QWidget;
    tabs->addTab(page, text(texts, text_count, 0));
    tabs->addTab(new QWidget, text(texts, text_count, 1));
    auto *grid = new QGridLayout(page);

    auto *apply = new QPushButton(text(texts, text_count, 2));
    apply->setDefault(true);
    auto *combo = new QComboBox;
    combo->addItem(text(texts, text_count, 6));
    auto *check = new QCheckBox(text(texts, text_count, 4));
    check->setChecked(true);
    auto *radio = new QRadioButton(text(texts, text_count, 5));
    radio->setChecked(true);
    auto *edit = new QLineEdit(text(texts, text_count, 7));
    auto *spin = new QSpinBox;
    spin->setValue(42);
    auto *slider = new QSlider(Qt::Horizontal);
    slider->setValue(60);
    auto *progress = new QProgressBar;
    progress->setValue(60);
    progress->setTextVisible(false);
    auto *cancel = new QPushButton(text(texts, text_count, 3));
    cancel->setEnabled(false);

    grid->addWidget(apply, 0, 0);
    grid->addWidget(combo, 0, 1);
    grid->addWidget(check, 1, 0);
    grid->addWidget(radio, 1, 1);
    grid->addWidget(edit, 2, 0);
    grid->addWidget(spin, 2, 1);
    grid->addWidget(slider, 3, 0);
    grid->addWidget(progress, 3, 1);
    grid->addWidget(cancel, 4, 0);
    grid->setRowStretch(5, 1);

    auto *scroll = new QScrollBar(Qt::Vertical);
    scroll->setRange(0, 100);
    scroll->setPageStep(40);
    outer->addWidget(tabs, 1);
    outer->addWidget(scroll);

    applyRecursively(&root, st.get(), palette);
    root.ensurePolished();
    root.resize(form_w, form_h);
    root.show();
    QApplication::processEvents();

    // Форма рисуется в `scale` раз крупнее (как на экране с таким
    // масштабом) и потом только сжимается: растянутый кадр выходил мыльным.
    const int k = scale < 1 ? 1 : scale;
    QImage shot(form_w * k, form_h * k, QImage::Format_ARGB32_Premultiplied);
    shot.setDevicePixelRatio(k);
    shot.fill(Qt::transparent);
    root.render(&shot);
    QImage frame(out_w, out_h, QImage::Format_ARGB32_Premultiplied);
    frame.fill(Qt::transparent);
    {
        QImage scaled = shot.scaled(out_w, out_h, Qt::KeepAspectRatio, Qt::SmoothTransformation);
        // Сжатый кадр — в точках кадра, а не формы.
        scaled.setDevicePixelRatio(1);
        QPainter p(&frame);
        p.drawImage((out_w - scaled.width()) / 2, (out_h - scaled.height()) / 2, scaled);
    }
    root.hide();
    const bool saved = frame.save(QString::fromUtf8(out), "PNG");
    // Стиль закрыть до QApplication: часть сторонних стилей при разрушении
    // фабрики стилей на выходе обращается к шине и зависает.
    applyRecursively(&root, QApplication::style(), palette);
    st.reset();
    return saved ? REXON_SHOT_OK : REXON_SHOT_SAVE_FAILED;
}
