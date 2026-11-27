// Generic bridge to Qt meta-reflection of gadgets (Qt Protobuf value types
// WallpaperFilterRule / FilterLogic). Not a single protobuf type is linked here
// by name: the metatype is looked up at runtime via QMetaType::fromName, and
// property reads/writes go through QMetaProperty::readOnGadget/writeOnGadget.
// This is the only part of the model that Rust/cxx-qt cannot express
// (no access to a foreign staticMetaObject), so it is moved into C++.
#pragma once

#include <cstdint>

#include <QtCore/QByteArray>
#include <QtCore/QHash>
#include <QtCore/QVariant>

#include "rust/cxx.h"

// Metatype by name (waywallen::control::v1::WallpaperFilterRule and the like).
// The type must already be registered (the waywallen.control module is loaded).
QVariant rexon_default_gadget(rust::Str type_name);
QVariant rexon_coerce_gadget(rust::Str type_name, const QVariant &value);

// Role == property index of the gadget's metaobject (as in the original:
// Qt::UserRole + 1 + i). read/set by property index.
QVariant rexon_gadget_read_by_role(const QVariant &gadget, ::std::int32_t role_index);
QVariant rexon_gadget_set_by_role(const QVariant &gadget, ::std::int32_t role_index,
                                  const QVariant &value);

// Convenience wrappers for scalar int fields (group / op / groupA / groupB).
::std::int32_t rexon_gadget_read_int(const QVariant &gadget, rust::Str prop);
::std::int64_t rexon_gadget_read_i64(const QVariant &gadget, rust::Str prop);
// A named value property (never a pointer one), for nested gadgets.
QVariant rexon_gadget_read_named(const QVariant &gadget, rust::Str prop);
QVariant rexon_gadget_set_int(const QVariant &gadget, rust::Str prop, ::std::int32_t value);

bool rexon_variant_is_valid(const QVariant &v);

// A value from QML without gadgets: maps, lists and nested gadgets
// are unfolded recursively into plain QVariantMap/QVariantList, the rest
// is passed as is.
//
// Gadgets arrive by two paths: they are written by the Rust filter editor model
// (rules and group logic) and assembled by the «Wallpapers» page (sort rules
// in the settings). Plain JS objects pass straight through — the same way the
// Python `_rules_plain` did.
//
// The protobuf descriptor is not needed: key names are the property names of the
// gadget's metaobject, i.e. the same camelCase as json_name. The oneof
// branch is told apart by the `has<Field>` properties — on a gadget, reading an inactive
// member returns not undefined but a default message, and without the has flag
// an empty branch would go into the request as a real one.
//
// There is ONE door for the whole project (rule #12): it is called by the shared converter
// `rexon_variant_to_json`, and so EVERY "QML → core" path understands gadgets,
// not only the filter editor where they first appeared.
QVariant rexon_plain_variant(const QVariant &value);

// roleNames as in the original: the names of all properties of the gadget's metaobject.
QHash<::std::int32_t, QByteArray> rexon_role_names(rust::Str type_name);
::std::int32_t rexon_prop_count(rust::Str type_name);
