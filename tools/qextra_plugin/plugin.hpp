#pragma once
#include <QtQml/qqmlextensionplugin.h>

class QExtraDynPlugin : public QQmlEngineExtensionPlugin {
    Q_OBJECT
    Q_PLUGIN_METADATA(IID QQmlEngineExtensionInterface_iid)

public:
    explicit QExtraDynPlugin(QObject* parent = nullptr);
};
