// Подопытное окно стенда слоя обоев: то же самое окно, что поднимает
// действующий Python-путь (rexon/qml/WebWallpaper.qml), но слой ему ставит
// Rust — через WallpaperLayer из org.rexon.gui, без Python в процессе.
//
// Окно намеренно почти прозрачное: стенд запускается на ЖИВОМ рабочем столе,
// и заслонять обои Wehrwolfmann'а ему незачем. Виден только маленький ярлык
// в центре, да и тот полторы секунды и в ФОНОВОМ слое, то есть под всеми
// окнами. Обои пользователя стенд не трогает вообще — он лишь показывает
// поверх них своё окно и закрывает его.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Window
import org.rexon.gui 1.0

Window {
    id: root

    // ⚠️ Порядок: окно создаётся НЕВИДИМЫМ, слой ставится в
    // Component.onCompleted, и только потом visible = true. Наоборот нельзя —
    // показанное окно уже стало обычным xdg_toplevel и в слой не переселится.
    visible: false
    color: "transparent"
    flags: Qt.FramelessWindowHint
    width: Screen.width
    height: Screen.height
    title: ""

    // Настройщик слоя на Rust. Значения по умолчанию у него — уже слой обоев;
    // applyWallpaperLayer() ставит канонический набор, не глядя на свойства.
    WallpaperLayer {
        id: layerShell
        scope: "rexon-layer-shell-selftest"
    }

    property bool applied: false
    property bool reported: false

    // ⚠️ Проверка на укус: стенд, который зеленеет всегда, ничего не
    // доказывает. Здесь задаётся ЗАВЕДОМАЯ поломка слоя, и родительская
    // половина стенда требует, чтобы прогон покраснел. Значение приходит
    // из Rust через setInitialProperties (переменная REXON_LS_BITE), по
    // умолчанию пусто — обычный прогон.
    //
    // Мутации не выдуманные: каждая — реальный способ испортить обои.
    //   layer    — слой Top: обои встанут ПОВЕРХ окон;
    //   anchors  — один якорь: обои не растянутся на экран;
    //   zone     — зона 0: панели начнут отодвигать обои;
    //   keyboard — обои перехватывают клавиатуру;
    //   size     — якорей нет вовсе, окно 640×360 вместо экрана;
    //   late     — слой ставится ПОСЛЕ показа окна: оно уже стало обычным
    //              xdg_toplevel и в слой не переселится (грабля порядка).
    property string bite: ""

    Component.onCompleted: {
        if (root.bite === "size") {
            // Заведомо неверный размер: без якорей поверхность слоя берёт
            // размер из set_size, и проверка «во весь экран» обязана пасть.
            root.width = 640;
            root.height = 360;
        }
        if (root.bite === "late") {
            root.visible = true;
            root.applied = layerShell.applyWallpaperLayer(root);
        } else {
            root.applied = root.applyLayer();
            root.visible = true;
        }
        settle.interval = root.bite === "" ? 1500 : 800;
        settle.start();
    }

    // Ставит слой: канонический набор либо испорченный укусом.
    function applyLayer() {
        switch (root.bite) {
        case "":
        case "late":
        case "size":
            break;
        case "layer":
            layerShell.layer = 2;            // LayerTop
            break;
        case "anchors":
            layerShell.anchors = 1;          // только верх
            break;
        case "zone":
            layerShell.exclusionZone = 0;
            break;
        case "keyboard":
            layerShell.keyboardInteractivity = 1;  // Exclusive
            break;
        default:
            console.log("REXON-LS-BITE-UNKNOWN " + root.bite);
            break;
        }
        if (root.bite === "size")
            layerShell.anchors = 0;          // AnchorNone: размер решает окно
        // Канонический слой ставится ТОЛЬКО в чистом прогоне: apply() берёт
        // свойства объекта, то есть донесёт до композитора укус.
        return root.bite === ""
            ? layerShell.applyWallpaperLayer(root)
            : layerShell.apply(root);
    }

    // Пауза на круг протокола: между показом окна и ответом композитора
    // (configure) проходит несколько кадров, и без неё геометрия читалась бы
    // до того, как KWin её выдал.
    Timer {
        id: settle
        // Значение ставится в Component.onCompleted: прогону с укусом
        // столько ждать незачем, а окно висит на ЖИВОМ рабочем столе.
        interval: 1500
        repeat: false
        onTriggered: root.report()
    }

    function report() {
        if (root.reported)
            return;
        root.reported = true;

        console.log("REXON-LS-APPLY applied=" + root.applied
                    + " available=" + layerShell.available());
        // Обратное чтение из LayerShellQt: доказывает лишь, что сеттеры не
        // проигнорированы. Настоящее доказательство — протокол, его разбирает
        // родительский процесс стенда.
        console.log("REXON-LS-READBACK layer=" + layerShell.readLayer(root)
                    + " anchors=" + layerShell.readAnchors(root)
                    + " exclusionZone=" + layerShell.readExclusionZone(root)
                    + " keyboard=" + layerShell.readKeyboardInteractivity(root));
        console.log("REXON-LS-GEOMETRY windowW=" + root.width
                    + " windowH=" + root.height
                    + " screenW=" + Screen.width
                    + " screenH=" + Screen.height);

        // Окно убирается ДО выхода, а не «когда-нибудь при завершении
        // процесса»: на живом столе лишний кадр чужого окна — это уже дефект.
        root.visible = false;
        Qt.exit(0);
    }

    // Ярлык — чтобы прогон был виден человеку, а не только в журнале.
    Rectangle {
        anchors.centerIn: parent
        width: label.implicitWidth + 32
        height: label.implicitHeight + 20
        radius: 6
        // Base/Text из скина steam_dark (правило #17).
        color: "#16202d"
        opacity: 0.85

        Text {
            id: label
            anchors.centerIn: parent
            text: "Rexon · проверка слоя обоев"
            color: "#66c0f4"
            font.pixelSize: 16
        }
    }
}
