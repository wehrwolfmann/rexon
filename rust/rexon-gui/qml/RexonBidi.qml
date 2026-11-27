// Substitutions into translated QML strings — ONE place for all of our QML.
//
// The core substitutes `%1` via `i18n::fill`, and with a right-to-left
// language every substitution goes into a U+2068…U+2069 isolate (FSI/PDI):
// a file name, a path, a number or an English title does not scramble the
// word order of the Arabic or Hebrew sentence around it. Strings that QML
// builds itself (`qsTranslate(…).arg(…)`) bypass the core — `String.arg`
// has no isolates — so they come here:
//
//     RexonBidi { id: bidi }
//     text: bidi.arg(qsTranslate("rexon", "Saved to %1"), dir)
//
// The isolate follows the application direction
// (`Qt.application.layoutDirection` — the same thing the window root uses
// to turn on mirroring), not the location: even in a physical zone the
// string is Arabic, and a substitution inside it needs a boundary.
// Left to right, the output is byte-for-byte unchanged — not a single
// invisible character.
//
// The guard is `rust/rexon-core/tests/rtl_parity.rs`: `.arg(` in our QML
// is written only in this file.

import QtQml

QtObject {
    id: bidi

    /// The interface is written right to left.
    readonly property bool rtl: Qt.application.layoutDirection === Qt.RightToLeft

    /// Wrap a substitution in an isolate when writing right to left.
    function isolate(value) {
        const s = String(value);
        return bidi.rtl && s !== "" ? "\u2068" + s + "\u2069" : s;
    }

    /// `template` with `%1`…`%9` replaced by `values` by number — like a chain
    /// of `.arg()` over the translation, but in a single pass (a substitution
    /// that itself contains "%2" does not swallow the next one) and with
    /// isolates for right to left. A foreign number and a "%" without a digit
    /// stay as they are.
    function arg(template) {
        const values = Array.prototype.slice.call(arguments, 1);
        return String(template).replace(/%([1-9])/g, (m, d) =>
            d - 1 < values.length ? bidi.isolate(values[d - 1]) : m);
    }
}
