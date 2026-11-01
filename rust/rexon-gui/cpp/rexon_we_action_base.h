// База синглтона Action: один-единственный сигнал `toast` с ДЕФОЛТНЫМИ
// аргументами.
//
// Зачем отдельная база, а не сигнал в мосте cxx-qt. У оригинала
// (`ui/src/action.cppm`) сигнал объявлен так:
//
//     void toast(QString text, qint32 duration = 3000, qint32 flags = 0,
//                QObject* action = nullptr);
//
// и вендорный QML пользуется дефолтами вовсю — `W.Action.toast(текст)` с
// ОДНИМ аргументом встречается в девяти местах. Дефолтные аргументы
// печатает moc: он генерирует укороченные клоны метода, и QML выбирает клон
// по числу аргументов. cxx-qt дефолтов не умеет, а перегрузки того же имени
// ломают его же генератор: он соединяет сигнал через `&Action::toast`, и на
// четырёх одноимённых методах это «unresolved overloaded function type».
//
// Поэтому сигнал остаётся тем, чем был у оригинала, — обычным Qt-сигналом в
// moc'аемом заголовке, а мост cxx-qt наследует его через `#[base]`. Заголовки
// из `cpp_files` cxx-qt-build через moc ПРОГОНЯЕТ (документировано в
// `CxxQtBuilder::cpp_file`: «compile or run moc on»), .cpp — нет.
//
// ⚠️ Наш Python-мост этого сделать не мог (PySide6-Signal дефолтов не знает)
// и разошёлся с оригиналом: сигнал там назывался `toasted`, а `toast` был
// слотом. Из-за переименования вендорный обработчик `Window.qml:86
// function onToast(...)` не срабатывал никогда.
#pragma once

#include <QtCore/QObject>
#include <QtCore/QString>

class RexonWeActionBase : public QObject {
    Q_OBJECT

public:
    explicit RexonWeActionBase(QObject* parent = nullptr): QObject(parent) {}
    ~RexonWeActionBase() override = default;

Q_SIGNALS:
    /// Тост поверх окна. Сигнатура и дефолты — из оригинала.
    void toast(QString text, qint32 duration = 3000, qint32 flags = 0,
               QObject* action = nullptr);
};
