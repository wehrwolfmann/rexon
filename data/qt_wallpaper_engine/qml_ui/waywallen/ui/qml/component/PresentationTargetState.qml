pragma ComponentBehavior: Bound
import QtQml
import Qcm.Material as MD
import waywallen.ui as W

QtObject {
    id: root

    property bool allTargets: true
    property var selectedKeys: []
    property bool fallbackToFirst: true

    readonly property var targets: {
        const out = [];
        for (const canvas of W.App.displayManager.canvases || []) {
            if (!canvas?.hasLiveDisplays)
                continue;
            const displayIds = [];
            const seen = {};
            for (const member of canvas.members || []) {
                for (const displayId of member.displayIds || []) {
                    const key = String(displayId);
                    if (seen[key] === true)
                        continue;
                    seen[key] = true;
                    displayIds.push(displayId);
                }
            }
            if (displayIds.length === 0)
                continue;
            out.push({
                key: root.canvasKey(canvas.id),
                kind: "canvas",
                label: canvas.name || qsTr("Unnamed canvas"),
                iconName: MD.Token.icon.dashboard,
                wireTarget: {
                    canvasId: canvas.id
                },
                displayIds: displayIds,
                maximumWidth: 240,
                toolTip: qsTr("%1 of %2 members online").arg(canvas.onlineCount || 0).arg(canvas.memberCount || 0)
            });
        }
        for (const display of W.App.displayManager.displays || []) {
            if (!display?.selectableTarget)
                continue;
            out.push({
                key: root.displayKey(display.id),
                kind: "display",
                label: root.displayLabel(display),
                iconName: MD.Token.icon.monitor,
                wireTarget: {
                    displayId: display.id
                },
                displayIds: [display.id],
                maximumWidth: 220,
                toolTip: ""
            });
        }
        return out;
    }
    readonly property bool hasTargets: targets.length > 0
    readonly property var wireTargets: {
        if (allTargets)
            return [];
        const out = [];
        const selected = root.selectedKeySet();
        for (const target of targets) {
            if (selected[target.key] === true)
                out.push(target.wireTarget);
        }
        return out;
    }
    readonly property var selectedDisplayIds: {
        const out = [];
        const seen = {};
        const selected = root.selectedKeySet();
        for (const target of targets) {
            if (!allTargets && selected[target.key] !== true)
                continue;
            for (const displayId of target.displayIds || []) {
                const key = String(displayId);
                if (seen[key] === true)
                    continue;
                seen[key] = true;
                out.push(displayId);
            }
        }
        return out;
    }
    readonly property bool hasSelection: hasTargets && (allTargets || selectedDisplayIds.length > 0)

    onTargetsChanged: Qt.callLater(root.reconcileSelection)

    Component.onCompleted: reconcileSelection()

    function displayKey(id) {
        return "display:" + id;
    }

    function canvasKey(id) {
        return "canvas:" + id;
    }

    function displayLabel(display) {
        const alias = display?.alias || "";
        const name = (display?.name || "").replace(/^waywallen-[a-z]+-[a-z]+-/, "");
        const base = alias.length > 0 ? alias : name;
        if (!base.length)
            return qsTr("Display #%1").arg(display?.id);
        return base + " (#" + display.id + ")";
    }

    function selectedKeySet() {
        const out = {};
        for (const key of selectedKeys || [])
            out[String(key)] = true;
        return out;
    }

    function isSelected(key) {
        return !allTargets && selectedKeySet()[String(key)] === true;
    }

    function selectAll() {
        allTargets = true;
        selectedKeys = [];
    }

    function toggleTarget(key) {
        const normalized = String(key);
        if (!targets.some(target => target.key === normalized))
            return;
        if (allTargets) {
            allTargets = false;
            selectedKeys = [normalized];
            return;
        }
        const next = (selectedKeys || []).map(value => String(value));
        const index = next.indexOf(normalized);
        if (index >= 0) {
            next.splice(index, 1);
        } else if (targets.length === 2) {
            selectedKeys = [normalized];
            return;
        } else {
            next.push(normalized);
        }
        if (next.length === 0)
            selectAll();
        else
            selectedKeys = next;
    }

    function reconcileSelection() {
        if (allTargets) {
            if ((selectedKeys || []).length > 0)
                selectedKeys = [];
            return;
        }
        const valid = {};
        for (const target of targets)
            valid[target.key] = true;
        const next = [];
        const seen = {};
        for (const value of selectedKeys || []) {
            const key = String(value);
            if (valid[key] !== true || seen[key] === true)
                continue;
            seen[key] = true;
            next.push(key);
        }
        if (next.length > 0) {
            if (JSON.stringify(next) !== JSON.stringify(selectedKeys || []))
                selectedKeys = next;
            return;
        }
        if (fallbackToFirst && targets.length > 0)
            selectedKeys = [targets[0].key];
        else
            selectedKeys = [];
    }
}
