// Implementation of the generic bridge to gadget meta-reflection. Fully
// repeats the behavior of toGadgetVariant / readOnGadget / writeOnGadget from
// the C++ model filter_rule_model.cpp, but the metatype is looked up at runtime by name,
// without a compile-time dependency on filter.qpb.h.
#include "rexon_gadget_shim.h"

#include <QtCore/QJsonDocument>
#include <QtCore/QMetaObject>
#include <QtCore/QMetaProperty>
#include <QtCore/QMetaType>
#include <QtCore/QString>
#include <QtCore/QVariantMap>

namespace {

// An internal Qt-Protobuf pointer property (`stringFilter_p`,
// `intFilter_p`, `tagFilter_p` of a oneof branch). Its GETTER DOES NOT READ, IT
// WRITES: `WallpaperFilterRule::intFilter_p()` on a rule with the string branch
// set detaches the data and puts an empty IntFilter into the oneof
// (`QProtobufOneof::message<T>()` → `ensureRawValue`), i.e. it erases
// the real payload. After that `stringFilter()` reads a foreign type as
// its own — it came out as {value:"", condition:0}. That is how on 24.10.2026 any rule
// («Title / contains / zzqxw», library, size, tag…) went to the daemon
// empty and was silently discarded. These properties must be neither read nor written:
// QML does not see them (SCRIPTABLE false), their values are the same oneof branches
// that travel through their own value properties anyway.
bool isPointerProperty(const QMetaProperty &p)
{
    const QMetaType::TypeFlags flags = p.metaType().flags();
    return flags.testAnyFlags(QMetaType::IsPointer | QMetaType::PointerToGadget
                              | QMetaType::PointerToQObject);
}

// QVariantMap (camelCase keys) → a gadget QVariant of the given metatype.
// An existing gadget passes straight through; primitives are converted via QVariant.
// Recursive for nested subfilters (stringFilter/intFilter/tagFilter).
QVariant toGadgetVariant(QMetaType target, const QVariant &value)
{
    if (!target.isValid())
        return value;
    if (value.metaType() == target)
        return value;
    const QMetaObject *mo = target.metaObject();
    if (mo && value.metaType() == QMetaType::fromType<QVariantMap>()) {
        const QVariantMap map = value.toMap();
        QVariant out{target, nullptr}; // default gadget
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

// Gadget → QVariantMap via the metaobject. Keys are property names; of a oneof branch
// only the set one is included (by its `has<Field>`).
QVariantMap gadgetToMap(const QVariant &value);

// Property value: a gadget is unfolded recursively, a list element by element,
// the rest is passed as is.
QVariant plainValue(const QVariant &value)
{
    if (value.metaType().flags().testFlag(QMetaType::IsGadget))
        return gadgetToMap(value);
    // ⚠️ Enum fields of a gadget: the core reads them as a NUMBER (`get_i32`), while
    // QJsonDocument::fromVariant prints the constant NAME
    // ("WALLPAPER_SORT_KEY_NAME"). Parsing such a name as a number gives 0 —
    // the sort rule went to the daemon empty, i.e. the order chosen
    // by the customer was silently lost. Numbers come from the meta system, not
    // the serializer, so we unfold them here.
    if (value.metaType().flags().testFlag(QMetaType::IsEnumeration))
        return QVariant(value.toInt());
    // Qt-Protobuf integers (`QtProtobuf::int32`/`int64`/`fixed64`…) are
    // `TransparentWrapper` wrappers, the JSON serializer does not know them, and it printed
    // them as a string through the QString converter ("group": "1"). The core parses the
    // string, but a file size above 2^53 or the sign of unsigned values would not survive
    // that — we pass a number, as for the enums above.
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

// Field name from the has-flag name: hasStringFilter → stringFilter.
QString fieldOfHasFlag(const QString &name)
{
    if (! name.startsWith(QStringLiteral("has")) || name.size() < 4)
        return {};
    if (! name.at(3).isUpper())
        return {};
    return name.at(3).toLower() + name.mid(4);
}

// Read a value property; an inactive oneof branch (its `has<Field>` is
// false) yields a default-constructed message instead of calling the getter.
QVariant readValueProperty(const QMetaObject *mo, const QMetaProperty &p, const QVariant &gadget)
{
    const QString name = QString::fromLatin1(p.name());
    if (! name.isEmpty()) {
        const QString hasName = QStringLiteral("has") + name.at(0).toUpper() + name.mid(1);
        const int hasIdx = mo->indexOfProperty(hasName.toLatin1().constData());
        if (hasIdx >= 0 && ! mo->property(hasIdx).readOnGadget(gadget.constData()).toBool())
            return QVariant(p.metaType(), nullptr);
    }
    return p.readOnGadget(gadget.constData());
}

QVariantMap gadgetToMap(const QVariant &value)
{
    QVariantMap out;
    const QMetaObject *mo = value.metaType().metaObject();
    if (mo == nullptr)
        return out;

    // First pass: has flags. They tell which oneof branch is set.
    QHash<QString, bool> present;
    for (int i = 0; i < mo->propertyCount(); ++i) {
        const QMetaProperty p = mo->property(i);
        const QString field = fieldOfHasFlag(QString::fromLatin1(p.name()));
        if (! field.isEmpty())
            present.insert(field, p.readOnGadget(value.constData()).toBool());
    }

    // Second pass: values. The flags themselves are not passed out — the core tells
    // the branch by the PRESENCE of the key (see we_settings::map_to_filter_rule).
    for (int i = 0; i < mo->propertyCount(); ++i) {
        const QMetaProperty p = mo->property(i);
        // ⛔ Do not read pointer properties: their getter erases the oneof branch
        // (see isPointerProperty).
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
    const QMetaProperty p = mo->property(role_index);
    // waywallen 0.6.1 does the same in WallpaperFilterRuleModel::data():
    // non-scriptable (pointer) roles are not served, and an inactive oneof
    // branch is answered with an empty message without calling its getter,
    // because reading it can replace the stored payload.
    if (!p.isScriptable() || isPointerProperty(p))
        return QVariant();
    return readValueProperty(mo, p, gadget);
}

QVariant rexon_gadget_read_named(const QVariant &gadget, rust::Str prop)
{
    const QMetaObject *mo = gadget.metaType().metaObject();
    if (!mo)
        return QVariant();
    const QByteArray n(prop.data(), static_cast<int>(prop.size()));
    const int idx = mo->indexOfProperty(n.constData());
    if (idx < 0)
        return QVariant();
    const QMetaProperty p = mo->property(idx);
    if (isPointerProperty(p))
        return QVariant();
    return readValueProperty(mo, p, gadget);
}

::std::int64_t rexon_gadget_read_i64(const QVariant &gadget, rust::Str prop)
{
    const QVariant v = rexon_gadget_read_named(gadget, prop);
    return v.canConvert<qlonglong>() ? v.toLongLong() : 0;
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
    // SCRIPTABLE false = internal qt-protobuf pointer properties
    // (stringFilter_p and the like): toGadgetVariant for a QVariantMap creates
    // a null pointer, the setter dereferences it — SIGSEGV. QML writes
    // only value roles (stringFilter), pointer ones are left alone.
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
    v.convert(p.metaType()); // int → enum (LogicOp/type) if needed
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
        for (int i = 0; i < mo->propertyCount(); ++i) {
            // waywallen 0.6.1: non-scriptable (pointer) properties are not roles.
            if (! mo->property(i).isScriptable())
                continue;
            names.insert(256 + 1 + i, mo->property(i).name()); // Qt::UserRole + 1 + i
        }
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
