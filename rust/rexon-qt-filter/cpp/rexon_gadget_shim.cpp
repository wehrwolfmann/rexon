// Реализация обобщённого моста к мета-рефлексии гаджетов. Полностью
// повторяет поведение toGadgetVariant / readOnGadget / writeOnGadget из
// C++-модели filter_rule_model.cpp, но метатип берётся в рантайме по имени,
// без компиляционной зависимости от filter.qpb.h.
#include "rexon_gadget_shim.h"

#include <QtCore/QJsonDocument>
#include <QtCore/QMetaObject>
#include <QtCore/QMetaProperty>
#include <QtCore/QMetaType>
#include <QtCore/QString>
#include <QtCore/QVariantMap>

namespace {

// Служебное указательное свойство Qt-Protobuf (`stringFilter_p`,
// `intFilter_p`, `tagFilter_p` у ветки oneof). Его ГЕТТЕР НЕ ЧИТАЕТ, А
// ПИШЕТ: `WallpaperFilterRule::intFilter_p()` у правила с установленной
// строковой веткой отцепляет данные и ставит в oneof пустой IntFilter
// (`QProtobufOneof::message<T>()` → `ensureRawValue`), то есть стирает
// настоящую нагрузку. После этого `stringFilter()` читает чужой тип как
// свой — выходило {value:"", condition:0}. Так 24.10.2026 любое правило
// («Название / содержит / zzqxw», библиотека, размер, тег…) уезжало демону
// пустым и молча отбрасывалось. Ни читать, ни писать эти свойства нельзя:
// QML их не видит (SCRIPTABLE false), их значения — те же ветки oneof,
// которые и так ходят своими value-свойствами.
bool isPointerProperty(const QMetaProperty &p)
{
    const QMetaType::TypeFlags flags = p.metaType().flags();
    return flags.testAnyFlags(QMetaType::IsPointer | QMetaType::PointerToGadget
                              | QMetaType::PointerToQObject);
}

// QVariantMap (camelCase-ключи) → гаджет-QVariant заданного метатипа.
// Уже-гаджет проходит насквозь; примитивы конвертятся через QVariant.
// Рекурсивно для вложенных субфильтров (stringFilter/intFilter/tagFilter).
QVariant toGadgetVariant(QMetaType target, const QVariant &value)
{
    if (!target.isValid())
        return value;
    if (value.metaType() == target)
        return value;
    const QMetaObject *mo = target.metaObject();
    if (mo && value.metaType() == QMetaType::fromType<QVariantMap>()) {
        const QVariantMap map = value.toMap();
        QVariant out{target, nullptr}; // гаджет по умолчанию
        for (int i = 0; i < mo->propertyCount(); ++i) {
            const QMetaProperty p = mo->property(i);
            if (!p.isWritable() || isPointerProperty(p))
                continue;
            const auto it = map.constFind(QString::fromLatin1(p.name()));
            if (it == map.constEnd())
                continue;
            p.writeOnGadget(out.data(), toGadgetVariant(p.metaType(), *it));
        }
        return out;
    }
    QVariant converted = value;
    converted.convert(target);
    return converted;
}

// Гаджет → QVariantMap по метаобъекту. Ключи — имена свойств; ветка oneof
// кладётся только установленная (по её `has<Field>`).
QVariantMap gadgetToMap(const QVariant &value);

// Значение свойства: гаджет разворачиваем рекурсивно, список — поэлементно,
// остальное отдаём как есть.
QVariant plainValue(const QVariant &value)
{
    if (value.metaType().flags().testFlag(QMetaType::IsGadget))
        return gadgetToMap(value);
    // ⚠️ Поля-перечисления гаджета: ядро читает их ЧИСЛОМ (`get_i32`), а
    // QJsonDocument::fromVariant печатает ИМЯ константы
    // («WALLPAPER_SORT_KEY_NAME»). Разбор такого имени числом даёт 0 —
    // правило сортировки уезжало демону пустым, то есть выбранный
    // покупателем порядок молча терялся. Числа отдаёт мета-система, а не
    // сериализатор, поэтому разворачиваем здесь.
    if (value.metaType().flags().testFlag(QMetaType::IsEnumeration))
        return QVariant(value.toInt());
    // Целые Qt-Protobuf (`QtProtobuf::int32`/`int64`/`fixed64`…) — обёртки
    // `TransparentWrapper`, сериализатору JSON они незнакомы, и он печатал
    // их строкой через конвертер в QString («group»: "1"). Ядро строку
    // разбирает, но размер файла больше 2^53 или знак у беззнаковых так
    // не переживут — отдаём число, как у перечислений выше.
    if (QByteArrayView(value.metaType().name()).startsWith("QtProtobuf::")) {
        const QByteArrayView name(value.metaType().name());
        const bool isUnsigned = name.contains("uint") || name.endsWith("::fixed32")
            || name.endsWith("::fixed64");
        if (isUnsigned && value.canConvert<qulonglong>())
            return QVariant(value.toULongLong());
        if (value.canConvert<qlonglong>())
            return QVariant(value.toLongLong());
    }
    if (value.metaType() == QMetaType::fromType<QVariantList>()) {
        QVariantList out;
        const QVariantList in = value.toList();
        out.reserve(in.size());
        for (const QVariant &el : in) out.append(plainValue(el));
        return out;
    }
    if (value.metaType() == QMetaType::fromType<QVariantMap>()) {
        QVariantMap out;
        const QVariantMap in = value.toMap();
        for (auto it = in.constBegin(); it != in.constEnd(); ++it)
            out.insert(it.key(), plainValue(it.value()));
        return out;
    }
    return value;
}

// Имя поля по имени has-флага: hasStringFilter → stringFilter.
QString fieldOfHasFlag(const QString &name)
{
    if (! name.startsWith(QStringLiteral("has")) || name.size() < 4)
        return {};
    if (! name.at(3).isUpper())
        return {};
    return name.at(3).toLower() + name.mid(4);
}

QVariantMap gadgetToMap(const QVariant &value)
{
    QVariantMap out;
    const QMetaObject *mo = value.metaType().metaObject();
    if (mo == nullptr)
        return out;

    // Проход первый: has-флаги. Они говорят, какая ветка oneof установлена.
    QHash<QString, bool> present;
    for (int i = 0; i < mo->propertyCount(); ++i) {
        const QMetaProperty p = mo->property(i);
        const QString field = fieldOfHasFlag(QString::fromLatin1(p.name()));
        if (! field.isEmpty())
            present.insert(field, p.readOnGadget(value.constData()).toBool());
    }

    // Проход второй: значения. Сами флаги наружу не отдаём — ядро отличает
    // ветку по НАЛИЧИЮ ключа (see we_settings::map_to_filter_rule).
    for (int i = 0; i < mo->propertyCount(); ++i) {
        const QMetaProperty p = mo->property(i);
        // ⛔ Указательные свойства не читать: их геттер стирает ветку oneof
        // (см. isPointerProperty).
        if (isPointerProperty(p))
            continue;
        const QString name = QString::fromLatin1(p.name());
        if (present.contains(fieldOfHasFlag(name)))
            continue;
        const auto it = present.constFind(name);
        if (it != present.constEnd() && ! it.value())
            continue;
        out.insert(name, plainValue(p.readOnGadget(value.constData())));
    }
    return out;
}

QMetaType typeByName(rust::Str name)
{
    const QByteArray n(name.data(), static_cast<int>(name.size()));
    return QMetaType::fromName(n.constData());
}

} // namespace

QVariant rexon_default_gadget(rust::Str type_name)
{
    const QMetaType t = typeByName(type_name);
    if (!t.isValid())
        return QVariant();
    return QVariant(t, nullptr);
}

QVariant rexon_coerce_gadget(rust::Str type_name, const QVariant &value)
{
    return toGadgetVariant(typeByName(type_name), value);
}

QVariant rexon_gadget_read_by_role(const QVariant &gadget, ::std::int32_t role_index)
{
    const QMetaObject *mo = gadget.metaType().metaObject();
    if (!mo || role_index < 0 || role_index >= mo->propertyCount())
        return QVariant();
    return mo->property(role_index).readOnGadget(gadget.constData());
}

QVariant rexon_gadget_set_by_role(const QVariant &gadget, ::std::int32_t role_index,
                                  const QVariant &value)
{
    const QMetaObject *mo = gadget.metaType().metaObject();
    if (!mo || role_index < 0 || role_index >= mo->propertyCount())
        return QVariant();
    const QMetaProperty p = mo->property(role_index);
    if (!p.isWritable())
        return QVariant();
    // SCRIPTABLE false = служебные указательные свойства qt-protobuf
    // (stringFilter_p и т.п.): toGadgetVariant для QVariantMap создаёт
    // нулевой указатель, сеттер его разыменовывает — SIGSEGV. QML пишет
    // только value-роли (stringFilter), указательные не трогаем.
    if (!p.isScriptable())
        return QVariant();
    QVariant copy = gadget; // detach
    if (!p.writeOnGadget(copy.data(), toGadgetVariant(p.metaType(), value)))
        return QVariant();
    return copy;
}

::std::int32_t rexon_gadget_read_int(const QVariant &gadget, rust::Str prop)
{
    const QMetaObject *mo = gadget.metaType().metaObject();
    if (!mo)
        return 0;
    const QByteArray n(prop.data(), static_cast<int>(prop.size()));
    const int idx = mo->indexOfProperty(n.constData());
    if (idx < 0)
        return 0;
    return mo->property(idx).readOnGadget(gadget.constData()).toInt();
}

QVariant rexon_gadget_set_int(const QVariant &gadget, rust::Str prop, ::std::int32_t value)
{
    const QMetaObject *mo = gadget.metaType().metaObject();
    if (!mo)
        return QVariant();
    const QByteArray n(prop.data(), static_cast<int>(prop.size()));
    const int idx = mo->indexOfProperty(n.constData());
    if (idx < 0)
        return QVariant();
    const QMetaProperty p = mo->property(idx);
    QVariant copy = gadget;
    QVariant v(value);
    v.convert(p.metaType()); // int → enum (LogicOp/тип) при необходимости
    if (!p.writeOnGadget(copy.data(), v))
        return QVariant();
    return copy;
}

bool rexon_variant_is_valid(const QVariant &v)
{
    return v.isValid();
}

QHash<::std::int32_t, QByteArray> rexon_role_names(rust::Str type_name)
{
    QHash<::std::int32_t, QByteArray> names;
    const QMetaType t = typeByName(type_name);
    const QMetaObject *mo = t.isValid() ? t.metaObject() : nullptr;
    if (mo)
        for (int i = 0; i < mo->propertyCount(); ++i)
            names.insert(256 + 1 + i, mo->property(i).name()); // Qt::UserRole + 1 + i
    return names;
}

::std::int32_t rexon_prop_count(rust::Str type_name)
{
    const QMetaType t = typeByName(type_name);
    const QMetaObject *mo = t.isValid() ? t.metaObject() : nullptr;
    return mo ? mo->propertyCount() : 0;
}

QVariant rexon_plain_variant(const QVariant &value)
{
    return plainValue(value);
}
