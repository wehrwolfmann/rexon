pragma Singleton
import QtQml
import waywallen.ui

QtObject {
    readonly property var revision: PluginTranslations.revision

    function tr(value) {
        const dependency = revision;
        return PluginTranslations.translate(value);
    }

    function optionLabel(options, value, fallback) {
        const raw = value === undefined || value === null ? "" : String(value);
        const option = (options ?? []).find(option => String(option.value) === raw);
        const label = tr(option?.labelText ?? option?.label ?? fallback ?? raw);
        return label.length > 0 ? label : raw;
    }

    // Library rows carry no options of their own: the value stored in the
    // DB is the key, and `labels` only maps it to what a source called it.
    function valueLabel(labels, value) {
        const raw = value === undefined || value === null ? "" : String(value);
        return optionLabel(null, raw, labels?.[raw]);
    }

    // Binary (1024-based) size, with the units and decimal separator of the
    // UI language.
    function formatBytes(bytes) {
        const locale = Qt.locale(Qt.uiLanguage);
        const units = [qsTr("B"), qsTr("KiB"), qsTr("MiB"), qsTr("GiB"), qsTr("TiB")];
        let value = Math.max(0, Number(bytes ?? 0));
        let index = 0;
        while (value >= 1024 && index < units.length - 1) {
            value /= 1024;
            ++index;
        }
        return value.toFixed(index === 0 ? 0 : 1).replace(".", locale.decimalPoint) + " " + units[index];
    }

    // Size reported by a source: a byte count, or a string such as "12.3 MB"
    // (read as 1024-based, like the byte count). Empty when unknown.
    function formatSize(size) {
        const text = String(size ?? "").trim();
        if (text.length === 0)
            return "";
        if (/^\d+$/.test(text))
            return Number(text) > 0 ? formatBytes(Number(text)) : "";
        const match = text.match(/^([\d.,]+)\s*([KMGT]?B)$/i);
        if (!match)
            return text;
        const value = parseFloat(match[1].replace(/,/g, ""));
        if (isNaN(value))
            return text;
        const power = "BKMGT".indexOf(match[2].toUpperCase()[0]);
        return formatBytes(value * Math.pow(1024, power));
    }
}
