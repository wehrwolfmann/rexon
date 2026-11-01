pragma ComponentBehavior: Bound
import QtCore
import QtQml
import waywallen.ui as W

QtObject {
    id: root

    required property var page
    required property var playlistListQuery
    required property var playlistMutation
    required property var playlistCreateMutation
    required property var playlistPlaybackMutation

    readonly property bool playbackQuerying: playlistPlaybackMutation.querying

    readonly property Settings settings: Settings {
        id: playlistSheetSettings
        category: "PlaylistListSheet"
        property alias allTargets: playTargets.allTargets
        property bool shareAllDisplays: false
        property bool targetScopeMigrated: false
    }

    readonly property W.PresentationTargetState targetState: W.PresentationTargetState {
        id: playTargets
        allTargets: false
        fallbackToFirst: true
    }

    readonly property bool listLoading: page.playlistListLoading
    readonly property bool mutationQuerying: playlistMutation.querying || playlistCreateMutation.querying
    readonly property bool createQuerying: playlistCreateMutation.querying
    readonly property var playlists: playlistListQuery.playlists || []
    // ПРАВКА REXON (починка дефекта, не доработка): на KDE дисплей
    // появляется, только когда плагин обоев уже назначен, а назначает его
    // Rexon при запуске плейлиста — без этой ветки запуск на свежей системе
    // был недоступен навсегда. Пока дисплеев нет, цель — «все рабочие
    // столы»; кадр придерживает транспорт Rexon до регистрации дисплея.
    // Правка живёт в patches/0011-playlist-without-displays-on-kde.patch.
    readonly property bool playWithoutDisplays: !targetState.hasTargets && W.App.playWithoutDisplays
    readonly property bool hasPlayTarget: targetState.hasSelection || playWithoutDisplays

    Component.onCompleted: {
        if (!playlistSheetSettings.targetScopeMigrated) {
            if (playlistSheetSettings.shareAllDisplays)
                targetState.allTargets = true;
            playlistSheetSettings.targetScopeMigrated = true;
        }
        targetState.reconcileSelection();
    }

    function playlistIsPlayingOnSelectedTargets(playlist) {
        if (!playlist || !root.hasPlayTarget)
            return false;
        // ПРАВКА REXON: без дисплеев ничего не играет — `every()` по
        // пустому набору ответил бы «уже играет» и превратил запуск в паузу.
        if (targetState.selectedDisplayIds.length === 0)
            return false;
        const playlistId = String(playlist.id);
        return targetState.selectedDisplayIds.every(displayId => {
            const display = W.App.displayManager.get(displayId);
            return display && String(display.activePlaylistId) === playlistId;
        });
    }

    function playlistDisplayLabels(playlist) {
        if (!playlist)
            return [];
        const playlistId = String(playlist.id);
        const out = [];
        for (const target of targetState.targets) {
            const active = (target.displayIds || []).some(displayId => {
                const display = W.App.displayManager.get(displayId);
                return display && String(display.activePlaylistId) === playlistId;
            });
            if (active)
                out.push(target.label);
        }
        return out;
    }

    function playlistRuntimeText(playlist) {
        let waiting = false;
        for (const target of targetState.targets) {
            for (const id of target.displayIds || []) {
                const display = W.App.displayManager.get(id);
                if (!display || String(display.activePlaylistId) !== String(playlist.id))
                    continue;
                const status = display.playlistStatus;
                if (status.error)
                    return status.error;
                waiting = waiting || status.waiting;
            }
        }
        return waiting ? qsTranslate("PlaylistEditPage", "Waiting for matching wallpapers") : "";
    }

    function togglePlayback(playlist) {
        if (!playlist || root.playbackQuerying || !root.hasPlayTarget)
            return;
        if (root.playlistIsPlayingOnSelectedTargets(playlist)) {
            playlistPlaybackMutation.deactivate(targetState.wireTargets, targetState.allTargets ? playlist.id : 0);
            return;
        }
        if (root.playWithoutDisplays) {
            // ПРАВКА REXON: «все рабочие столы» — как вендорное «все»
            // (пустые цели и подключение новых дисплеев).
            targetState.selectAll();
        }
        playlistPlaybackMutation.activate(playlist.id, targetState.wireTargets, targetState.allTargets);
    }

    function editPlaylist(playlist) {
        page.openPlaylistEditor(playlist);
    }

    function editPlaylistItems(playlist) {
        if (!playlist || playlist.sourceKind === "smart")
            return;
        page.beginPlaylistItemSelection(playlist.id, playlist.revision, playlist.entryIds || []);
    }

    function deletePlaylist(playlist) {
        page.deletePlaylist(playlist);
    }

    function createPlaylist() {
        page.createEmptyPlaylist();
    }

    function createSmartPlaylist() {
        page.openPlaylistEditor(0, true);
    }
}
