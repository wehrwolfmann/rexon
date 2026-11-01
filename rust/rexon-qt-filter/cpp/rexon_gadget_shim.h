// Обобщённый мост к Qt-мета-рефлексии гаджетов (Qt Protobuf value-типы
// WallpaperFilterRule / FilterLogic). Ни одного протобаф-типа по имени тут
// НЕ линкуется: метатип берётся в рантайме через QMetaType::fromName, а
// чтение/запись свойств — через QMetaProperty::readOnGadget/writeOnGadget.
// Это единственная часть модели, которую Rust/cxx-qt выразить не могут
// (нет доступа к чужому staticMetaObject), поэтому она вынесена в C++.
#pragma once

#include <cstdint>

#include <QtCore/QByteArray>
#include <QtCore/QHash>
#include <QtCore/QVariant>

#include "rust/cxx.h"

// Метатип по имени (waywallen::control::v1::WallpaperFilterRule и т.п.).
// Тип обязан быть уже зарегистрирован (модуль waywallen.control загружен).
QVariant rexon_default_gadget(rust::Str type_name);
QVariant rexon_coerce_gadget(rust::Str type_name, const QVariant &value);

// Роль == индекс свойства метаобъекта гаджета (как в оригинале:
// Qt::UserRole + 1 + i). read/set по индексу свойства.
QVariant rexon_gadget_read_by_role(const QVariant &gadget, ::std::int32_t role_index);
QVariant rexon_gadget_set_by_role(const QVariant &gadget, ::std::int32_t role_index,
                                  const QVariant &value);

// Удобные обёртки для скалярных int-полей (group / op / groupA / groupB).
::std::int32_t rexon_gadget_read_int(const QVariant &gadget, rust::Str prop);
QVariant rexon_gadget_set_int(const QVariant &gadget, rust::Str prop, ::std::int32_t value);

bool rexon_variant_is_valid(const QVariant &v);

// Значение из QML без гаджетов: карты, списки и вложенные гаджеты
// разворачиваются рекурсивно в обычные QVariantMap/QVariantList, остальное
// отдаётся как есть.
//
// Гаджеты приходят двумя путями: их пишет Rust-модель редактора фильтров
// (правила и логика групп) и собирает страница «Обои» (правила сортировки
// в настройках). Обычные JS-объекты проходят насквозь — так же, как это
// делал питоновский `_rules_plain`.
//
// Дескриптор protobuf не нужен: имена ключей — это имена свойств
// метаобъекта гаджета, то есть тот же camelCase, что у json_name. Ветку
// oneof отличаем по свойствам `has<Field>` — у гаджета чтение неактивного
// члена отдаёт не undefined, а сообщение по умолчанию, и без has-флага
// пустая ветка уехала бы в запрос как настоящая.
//
// Дверь ОДНА на весь проект (правило #12): её зовёт общий конвертер
// `rexon_variant_to_json`, и потому гаджеты понимает КАЖДЫЙ путь
// «QML → ядро», а не только редактор фильтров, где они завелись первыми.
QVariant rexon_plain_variant(const QVariant &value);

// roleNames как в оригинале: имена всех свойств метаобъекта гаджета.
QHash<::std::int32_t, QByteArray> rexon_role_names(rust::Str type_name);
::std::int32_t rexon_prop_count(rust::Str type_name);
