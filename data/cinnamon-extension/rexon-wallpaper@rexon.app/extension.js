// Rexon Wallpaper Engine — Cinnamon extension wrapper.
//
// Wehrwolfmann: counterpart of the GNOME extension. Cinnamon uses Muffin (a fork
// of Mutter), which did not support wlr-layer-shell before 6.8. On Cinnamon-X11 the
// xprop fallback works (`rexon-gui::wallpaper_host::apply_x11_desktop_layer`); on
// Cinnamon-Wayland before 6.8 this extension is needed. Since 6.8 (layer-shell) the wallpaper
// is placed by Rexon as a desktop layer, and from here — only the screen effect.
//
// Architecture:
// 1. The wallpaper host renders each frame into /run/user/$UID/rexon-wp-frame.rgba
//    (RGBA8888, under flock LOCK_EX). The launch line is SENT by Rexon in the
//    `daemon-argv` settings key: the extension is shell code, it does not know where
//    the wallpaper host binary lives. There is no fallback: no line sent —
//    the extension says so in the log and does not start the wallpaper.
// 2. This extension places an actor on every monitor between the background and
//    the windows (where exactly and why — see `_installActors`) and every tick
//    (~30 FPS) puts the frame into them as a Clutter.Image.
// 3. The screen effect (scanlines, CRT and so on) is separate from the wallpaper:
//    the shader Rexon writes (key `effect-shader`) is applied as a
//    Cinnamon.GLSLEffect on Main.uiGroup — windows, panels, desktop. Cinnamon keeps
//    application pop-up menus above uiGroup, directly on the stage, the
//    effect does not touch them. Works on both X11 and Wayland.
//
// Cinnamon JS API: classic imports (NOT ESM as in GNOME 45+).

const GLib = imports.gi.GLib;
const Gio = imports.gi.Gio;
const GObject = imports.gi.GObject;
const Cinnamon = imports.gi.Cinnamon;
const Clutter = imports.gi.Clutter;
const Cogl = imports.gi.Cogl;
const Meta = imports.gi.Meta;
const Main = imports.ui.main;
const Lang = imports.lang;

const FRAME_PATH = GLib.get_user_runtime_dir() + '/rexon-wp-frame.rgba';
const DAEMON_TICK_MS = 33;  // ~30 FPS

// Module-level state (Cinnamon extension lifecycle: init/enable/disable
// are called as plain functions, not class methods).
let _settings = null;
let _daemon_pid = null;
let _daemon_stopping = false;
let _tick_source = 0;
let _actors = [];          // [{actor, metaWindow, sizeId}] — where the frame goes
let _fallback = null;      // Meta.BackgroundGroup, when the csd background is not found
let _bottomAddedId = 0;    // subscription to new windows of the bottom group
let _monitorsChangedId = 0;
let _fallbackIdle = 0;
let _frame_w = 0;
let _frame_h = 0;
let _meta = null;
let _gio_settings = null;
let _settings_changed_id = 0;
let _effect = null;        // ScreenEffect — screen effect on top of uiGroup

function init(metadata) {
    _meta = metadata;
}

function enable() {
    _effect = new ScreenEffect({
        target: Main.uiGroup,
        base: Cinnamon.GLSLEffect,
        hook: Cinnamon.SnippetHook.FRAGMENT,
        log: function(text) { global.log(text); },
        logError: function(text) { global.logError(text); },
        unredirect: function(allow) {
            if (allow)
                Meta.enable_unredirect_for_display(global.display);
            else
                Meta.disable_unredirect_for_display(global.display);
        },
    });
    // The wallpaper must not take the screen effect and the settings subscription down with it:
    // an error in them goes only to the log.
    try {
        _reload();
    } catch (e) {
        global.logError('[rexon-wp] обои не поднялись: ' + e);
    }
    // Rexon changes the wallpaper and the screen effect by writing the extension settings.
    // Without a subscription the new ones would appear only after logging into the session again.
    // The effect key does NOT restart the wallpaper: otherwise changing the effect would stop and
    // bring up the wallpaper host again.
    if (_gio_settings && !_settings_changed_id) {
        _settings_changed_id = _gio_settings.connect('changed', function(_s, key) {
            if (key === EFFECT_KEY) {
                if (_effect) _effect.queue(function() { return effectShaderPath(_gio_settings); });
            } else {
                try {
                    _reload();
                } catch (e) {
                    global.logError('[rexon-wp] обои не поднялись: ' + e);
                }
            }
        });
    }
    _effect.apply(effectShaderPath(_gio_settings));
    global.log('[rexon-wp] enabled, daemon pid=' + _daemon_pid);
}

// Reread the settings and restart the wallpaper accordingly.
// Actors are placed ONLY when the wallpaper is actually running: otherwise a black actor
// would cover the normal background and the desktop would turn black.
function _reload() {
    _stopTick();
    _removeActors();
    _killDaemon();

    _settings = _loadSettings();
    if (!_settings || !_settings.engine || !_settings.source) {
        global.log('[rexon-wp] обои не заданы — рабочий стол не трогаем');
        return;
    }

    // Cinnamon 6.8+: the background is drawn by the cinnamon-background-daemon service
    // as a wlr-layer-shell surface, Muffin speaks this protocol itself, and
    // Meta.BackgroundGroup was removed from the bindings. There Rexon places the wallpaper with its own
    // background layer window, without the extension: no actors and no host here. A host
    // brought up from here would run as a SECOND one next to Rexon's host (live
    // 15.11.2026 — from old keys, with a different video), so the session
    // indicator decides — the same one Rexon uses — and not whether there was somewhere to put the frame.
    if (_layerShellSession()) {
        global.log('[rexon-wp] Cinnamon с layer-shell — обои показывает Rexon слоем стола, расширение их не рисует и хост не запускает');
        return;
    }
    if (_csdBackgrounds().length === 0 && !_canBuildBackgroundGroup()) {
        global.log('[rexon-wp] своей группы фона в этой версии Cinnamon нет — обои показывает Rexon слоем стола, расширение их не рисует');
        return;
    }

    _spawnDaemon(_settings);
    if (!_daemon_pid) return;
    _installActors();
    _startTick();
}

function disable() {
    if (_gio_settings && _settings_changed_id) {
        _gio_settings.disconnect(_settings_changed_id);
        _settings_changed_id = 0;
    }
    _gio_settings = null;
    if (_effect) {
        _effect.destroy();
        _effect = null;
    }
    _stopTick();
    _removeActors();
    _killDaemon();
    _settings = null;
    global.log('[rexon-wp] disabled');
}

// ── Settings (gsettings, not the GSettings binding from cinnamon-settings) ────

function _loadSettings() {
    // Wehrwolfmann: we read directly via Gio.Settings — Cinnamon
    // can resolve the schema automatically from metadata.json.
    try {
        const schemaSrc = Gio.SettingsSchemaSource.new_from_directory(
            _meta.path + '/schemas',
            Gio.SettingsSchemaSource.get_default(),
            false,
        );
        const schema = schemaSrc.lookup(
            'org.cinnamon.extensions.rexon-wallpaper', false,
        );
        if (!schema) {
            global.logError('[rexon-wp] schema не найдена');
            return null;
        }
        // There is ONE settings object for the whole time the extension is enabled: the
        // 'changed' subscription hangs on it. Previously each _reload created a new one —
        // the subscription stayed on the first (which could be garbage collected and
        // go silent), and disable() unsubscribed its id from a different object.
        if (!_gio_settings)
            _gio_settings = new Gio.Settings({ settings_schema: schema });
        const s = _gio_settings;   // must stay alive for the 'changed' subscription
        // The key appeared later than the extension itself: one left over from a previous
        // version does not have it, and crashing because of that is not allowed — otherwise the wallpaper
        // would stop working entirely. Empty = "Rexon sent nothing".
        //
        // ⚠️ We ask THE SCHEMA rather than wrap the read in try/catch:
        // `get_strv` on an unknown key does not throw a catchable
        // exception, it prints "does not contain a key named …" and KILLS
        // THE PROCESS (verified — SIGABRT, exit code 134). The process here is
        // the shell itself, so the price would be not a wallpaper but the session.
        const daemon_argv = schema.has_key('daemon-argv')
            ? s.get_strv('daemon-argv')
            : [];
        return {
            engine: s.get_string('engine'),
            source: s.get_string('source'),
            fps: s.get_int('fps') || 30,
            daemon_argv: daemon_argv,
        };
    } catch (e) {
        global.logError('[rexon-wp] settings load: ' + e);
        return null;
    }
}

// ── Daemon control ──────────────────────────────────────────────────

// The wallpaper daemon launch line — ONLY the one Rexon sent.
//
// The fallback `python3 -m rexon.video_wallpaper` is no longer here:
// the Python wallpaper host was removed from the tree, there is nothing to
// substitute the binary with. An empty array in the settings = "Rexon sent
// nothing"; this case is handled not here but in the shared block below —
// together with the other unusable launch lines, so that the explanation
// to the person is the same for all cases.

// ─── SHARED LAUNCH DIAGNOSTICS BLOCK ───────────────────────────────
//
// Verbatim identical in the GNOME and Cinnamon extensions: the defect was the
// same in both files, so the fix is the same too (rule #12). The guard against
// divergence is the test `rust/rexon-core/tests/wallpaper_mmap_parity.rs`,
// `shell_extensions_share_launch_diagnostics`: it cuts this block out of
// both files and compares them character by character, so an edit here must
// go INTO BOTH FILES. Logging and showing the message are NOT part of it: on
// GNOME that is `console.*`, on Cinnamon `global.log*`, and that is the only
// difference the block tolerates.
// [rexon-wp:launch-diagnostics:begin]
const LAUNCH_FAIL_TITLE = 'Rexon: живые обои не запустились';

// Where to send the person when the launch line is unusable. It is fixed in
// exactly one place: the line is written by Rexon itself when it hands the wallpaper to the extension.
const REINSTALL_HINT = 'Откройте в Rexon «Оформление», раздел расширений ' +
    'оболочки, и установите расширение обоев заново — строку запуска пишет туда сам Rexon.';

// What is wrong with the launch line, BEFORE calling the shell.
// `null` — everything is fine.
//
// Why check in advance. `GLib.spawn_async` on an unusable line answers with a
// bare "Failed to execute child process … (No such file or directory)",
// and from that text the customer will understand neither WHAT was not found nor WHERE it was looked for.
// So each case is named in its own words and the path itself is ALWAYS
// shown: it is the answer to "where was it looked for".
function launchProblem(argv) {
    if (!argv || argv.length === 0 || !argv[0]) {
        return 'Rexon не сказал, чем показывать обои: ключ настроек ' +
            '«daemon-argv» пуст. Так выглядит расширение, оставшееся от ' +
            'прежней версии Rexon. ' + REINSTALL_HINT;
    }
    const exe = argv[0];
    if (exe.charAt(0) !== '/') {
        return 'Rexon прислал имя программы, а не путь к ней: «' + exe +
            '». Расширение НЕ ищет программы в PATH, и это намеренно: в ' +
            'PATH оболочки лежит не тот Rexon, что установлен у вас, если ' +
            'он стоит в venv, в переносном каталоге или во Flatpak. Нужен ' +
            'полный путь от корня. ' + REINSTALL_HINT;
    }
    if (!GLib.file_test(exe, GLib.FileTest.EXISTS)) {
        return 'Программа показа обоев не найдена. Искали ровно один файл, ' +
            'путь к нему прислал сам Rexon: ' + exe + ' (в PATH расширение ' +
            'не смотрит). Похоже, Rexon переставили или перенесли в другой ' +
            'каталог. ' + REINSTALL_HINT;
    }
    if (!GLib.file_test(exe, GLib.FileTest.IS_EXECUTABLE)) {
        return 'Программа показа обоев на месте, но запускать её нечем: у ' +
            'файла ' + exe + ' нет права на выполнение. Так бывает после ' +
            'распаковки архива или копирования на диск, где прав Unix нет. ' +
            REINSTALL_HINT;
    }
    return null;
}

// A failure that happened already inside the shell: the line was usable, but the
// launch did not work. `detail` — the exception text, or empty if no reason was given.
function spawnFailureText(argv, detail) {
    return 'Оболочка не смогла запустить показ обоев: ' +
        (detail ? detail : 'причину она не назвала') + '. Запускали: ' +
        argv.join(' ');
}

// The host came up and died right away. Staying silent is not an option: the actors are
// already in place, and a black rectangle would remain on screen above the normal
// background — exactly the "it just does not work" that the bug was filed for.
function earlyExitText(argv, status) {
    return 'Показ обоев запустился и сразу завершился (код ' + status +
        '). Запускали: ' + argv.join(' ') + '. Причину называет сама ' +
        'программа показа — её видно в журнале сеанса.';
}

// The host ran longer than this and died — that is no longer a launch failure but a crash
// on the go (sleep, GPU reset, driver error). Rexon restarts these
// itself: its watchdog sees the host is gone by the heartbeat mark and restarts
// the wallpaper through the extension settings, and if crashes come in a row it sets
// a still frame and says so in ONE notification. A shell message
// here would be a second one, and for every crash.
const EARLY_EXIT_MS = 10000;

// Whether the host exit counts as a launch failure (show the message).
function isLaunchFailure(livedMs) {
    return livedMs < EARLY_EXIT_MS;
}
// [rexon-wp:launch-diagnostics:end]


// ─── SHARED FRAME READ BLOCK ───────────────────────────────────────
//
// Verbatim identical in the GNOME and Cinnamon extensions, the guard is
// `shell_extensions_share_frame_read` in
// `rust/rexon-core/tests/wallpaper_mmap_parity.rs`. Edits go INTO BOTH FILES.
// [rexon-wp:frame-read:begin]
// The frame generation counter lies next to the frame file (contract —
// `rexon-core::wallpaper_mmap::frame_generation_path`): u64 little-endian,
// odd — the host is writing a frame right now, even — the frame is written completely.
//
// Why. The host writes the frame under flock(LOCK_EX), and there is no way to
// take the lock from here: GJS has neither flock nor fcntl. Without the lock the copy landed
// in the middle of a write — measured at 1920×1080 and 30 fps: 12 torn frames out of 180 ticks
// (top from one frame, bottom from another).
const FRAME_GEN_SUFFIX = '.gen';

// The counter value; null — there is no counter (host of an older build).
function frameGeneration(framePath) {
    let data;
    try {
        data = GLib.file_get_contents(framePath + FRAME_GEN_SUFFIX)[1];
    } catch (e) {
        return null;
    }
    if (!data || data.length < 8) return null;
    return new DataView(data.buffer, data.byteOffset, 8).getBigUint64(0, true);
}

// A whole frame (Uint8Array no shorter than expected) or null if there is
// no frame yet, it is incomplete, or the host was writing during the copy — then the tick
// is skipped and the previous frame stays on screen. File mapping errors
// are passed to the caller: it has its own log.
function readWholeFrame(framePath, expected) {
    const before = frameGeneration(framePath);
    if (before !== null && (before & 1n) === 1n) return null;
    const bytes = GLib.MappedFile.new(framePath, false).get_bytes();
    if (bytes.get_size() < expected) return null;
    // get_data() COPIES (verified: after the file is overwritten the array is unchanged),
    // so checking the counter after the copy guards exactly this copy.
    const data = bytes.get_data();
    if (frameGeneration(framePath) !== before) return null;
    return data;
}
// [rexon-wp:frame-read:end]


// ─── SHARED SCREEN EFFECT BLOCK ──────────────────────────────────────
//
// Verbatim identical in the GNOME and Cinnamon extensions, the guard is
// `shell_extensions_share_screen_effect` in
// `rust/rexon-core/src/screen_effects.rs`. Edits go INTO BOTH FILES. The difference
// between the shells (effect class, snippet hook, log, fullscreen windows)
// comes from outside as the `host` object.
// [rexon-wp:screen-effect:begin]
// Contract with the core (`rexon-core::screen_effects`): the settings key is the path to
// the shader file, empty — the effect is off. File: Cogl snippet declarations,
// a marker line, the snippet code (FRAGMENT hook with replace). The file is written BEFORE
// the key changes, and the new shader has a different path, so the extension never
// reads an incomplete file.
const EFFECT_KEY = 'effect-shader';
const EFFECT_CODE_MARKER = '//@rexon-cogl-code';
const EFFECT_NAME = 'rexon-screen-effect';
// Clock of animated effects: ~30 frames per second.
const EFFECT_TICK_MS = 33;
// Time in the shader wraps around: at large numbers float loses precision,
// and the noise freezes into stripes.
const EFFECT_TIME_WRAP = 3600;
// Rexon writes the file and the key in separate commands — wait for things to settle.
const EFFECT_SETTLE_MS = 150;
// Reply to Rexon: what came of the last apply (`shell_status_path` of the core).
// Rexon says "applied" only on this reply — without it the window said
// "applied" when the extension did not work at all (15.11.2026).
const EFFECT_STATUS_PATH = GLib.build_filenamev([
    GLib.get_user_config_dir(), 'rexon', 'screen-effects', 'shell.status']);

// state: 'on' | 'off' | 'error'; path — the shader file the reply is about.
function writeEffectStatus(state, path, detail) {
    try {
        GLib.mkdir_with_parents(GLib.path_get_dirname(EFFECT_STATUS_PATH), 0o755);
        GLib.file_set_contents(EFFECT_STATUS_PATH,
            'state=' + state + '\npath=' + (path || '') +
            '\ndetail=' + String(detail || '').replace(/\n/g, ' ') + '\n');
    } catch (e) {
        // The reply is a hint to the Rexon window, not a reason to bring down the shell.
    }
}

// Path from the settings. Whether the key exists is asked OF THE SCHEMA: reading a key
// that is not in the schema kills the shell itself (see `daemon-argv`).
function effectShaderPath(settings) {
    const schema = settings ? settings.settings_schema : null;
    return (schema && schema.has_key(EFFECT_KEY)) ? settings.get_string(EFFECT_KEY) : '';
}

// Parse the shader file. Throws an exception with a clear reason.
function readEffectShader(path) {
    const bytes = GLib.file_get_contents(path)[1];
    const text = new TextDecoder().decode(bytes);
    const at = text.indexOf(EFFECT_CODE_MARKER);
    if (at < 0)
        throw new Error('в файле нет строки-метки кода: ' + path);
    const code = text.slice(at + EFFECT_CODE_MARKER.length);
    return {
        text: text,
        declarations: text.slice(0, at),
        code: code,
        animated: code.indexOf('rx_time') >= 0,
    };
}

// Effect class for a shader. The GLSLEffect pipeline is built ONCE per
// class (`build_pipeline` is called on the first instance, then copied), so
// a new shader needs a new class. GObject types cannot be unregistered, so
// classes are remembered by shader text and reused.
const effectClasses = new Map();
let effectClassSerial = 0;

function effectClassFor(base, hook, shader) {
    let klass = effectClasses.get(shader.text);
    if (klass)
        return klass;
    effectClassSerial += 1;
    // The type name is unique even after the extension is disabled and enabled again: the module
    // is reloaded, but the types of the previous classes remain in the process.
    const typeName = 'RexonScreenEffect_' + GLib.get_monotonic_time() + '_' + effectClassSerial;
    klass = GObject.registerClass({ GTypeName: typeName },
        class RexonScreenEffect extends base {
            vfunc_build_pipeline() {
                this.add_glsl_snippet(hook, shader.declarations, shader.code, true);
            }
        });
    effectClasses.set(shader.text, klass);
    return klass;
}

// Effect on the whole-screen actor.
// host: { target, base, hook, log(text), logError(text), unredirect(bool) }.
class ScreenEffect {
    constructor(host) {
        this._host = host;
        this._effect = null;
        this._text = '';
        this._resLoc = -1;
        this._timeLoc = -1;
        this._tickId = 0;
        this._settleId = 0;
        this._sizeIds = [];
        this._unredirectBlocked = false;
        this._start = 0;
    }

    // Settings changed: apply once Rexon has finished writing everything.
    queue(readPath) {
        if (this._settleId)
            GLib.source_remove(this._settleId);
        this._settleId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, EFFECT_SETTLE_MS, () => {
            this._settleId = 0;
            this.apply(readPath());
            return GLib.SOURCE_REMOVE;
        });
    }

    apply(path) {
        if (!path) {
            if (this._effect)
                this._host.log('[rexon-fx] эффект экрана выключен');
            this._remove();
            writeEffectStatus('off', '', '');
            return;
        }
        let shader;
        try {
            shader = readEffectShader(path);
        } catch (e) {
            // Without a shader the screen stays as it is — nothing is applied.
            this._host.logError('[rexon-fx] шейдер не прочитан: ' + e.message);
            this._remove();
            writeEffectStatus('error', path, e.message);
            return;
        }
        if (this._effect && shader.text === this._text) {
            writeEffectStatus('on', path, '');
            return;
        }
        this._remove();
        try {
            const Klass = effectClassFor(this._host.base, this._host.hook, shader);
            const effect = new Klass();
            this._resLoc = effect.get_uniform_location('rx_res');
            this._timeLoc = shader.animated ? effect.get_uniform_location('rx_time') : -1;
            this._effect = effect;
            this._text = shader.text;
            this._start = GLib.get_monotonic_time();
            this._setUniforms();
            this._host.target.add_effect_with_name(EFFECT_NAME, effect);
            // The shell sends a fullscreen window (game, video) to the screen
            // bypassing itself — the effect would disappear there. While the effect is on,
            // we do not do that.
            this._host.unredirect(false);
            this._unredirectBlocked = true;
            const target = this._host.target;
            for (const prop of ['notify::width', 'notify::height'])
                this._sizeIds.push(target.connect(prop, () => this._setUniforms()));
            if (this._timeLoc >= 0) {
                this._tickId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, EFFECT_TICK_MS, () => {
                    this._setUniforms();
                    if (this._effect)
                        this._effect.queue_repaint();
                    return GLib.SOURCE_CONTINUE;
                });
            }
            this._host.log('[rexon-fx] эффект экрана включён: ' + path +
                (this._tickId ? ' (движущийся)' : ''));
            writeEffectStatus('on', path, '');
        } catch (e) {
            this._host.logError('[rexon-fx] эффект не наложен: ' + e.message);
            this._remove();
            writeEffectStatus('error', path, e.message);
        }
    }

    _setUniforms() {
        const effect = this._effect;
        if (!effect)
            return;
        const target = this._host.target;
        let w = target.width;
        let h = target.height;
        if (!w || !h) {
            w = global.stage.width;
            h = global.stage.height;
        }
        if (this._resLoc >= 0)
            effect.set_uniform_float(this._resLoc, 2, [w, h]);
        if (this._timeLoc >= 0) {
            const t = ((GLib.get_monotonic_time() - this._start) / 1e6) % EFFECT_TIME_WRAP;
            effect.set_uniform_float(this._timeLoc, 1, [t]);
        }
    }

    // Remove the effect: the screen is exactly as it was before it.
    _remove() {
        if (this._tickId) {
            GLib.source_remove(this._tickId);
            this._tickId = 0;
        }
        const target = this._host.target;
        for (const id of this._sizeIds)
            target.disconnect(id);
        this._sizeIds = [];
        if (this._effect) {
            target.remove_effect(this._effect);
            this._effect = null;
        }
        if (this._unredirectBlocked) {
            this._host.unredirect(true);
            this._unredirectBlocked = false;
        }
        this._text = '';
        this._resLoc = -1;
        this._timeLoc = -1;
    }

    destroy() {
        if (this._settleId) {
            GLib.source_remove(this._settleId);
            this._settleId = 0;
        }
        this._remove();
    }
}
// [rexon-wp:screen-effect:end]

// Report the failure BOTH ways: in the log (details for analysis there) and
// as a shell pop-up message — otherwise the customer only sees that
// "the wallpaper does not work", and has nowhere to learn the reason.
function _reportFailure(text, cfg) {
    global.logError('[rexon-wp] ' + text + ' [обои: движок «' +
        (cfg ? cfg.engine : '?') + '», источник «' +
        (cfg ? cfg.source : '?') + '»]');
    try {
        Main.notifyError(LAUNCH_FAIL_TITLE, text);
    } catch (e) {
        global.logError('[rexon-wp] показать сообщение не вышло: ' + e);
    }
}

function _spawnDaemon(cfg) {
    const argv = cfg.daemon_argv;
    const problem = launchProblem(argv);
    if (problem) {
        _reportFailure(problem, cfg);
        return;
    }

    // Size of the first monitor (Cinnamon usually has one primary)
    const monitor = Main.layoutManager.primaryMonitor;
    const w = monitor ? monitor.width : 1920;
    const h = monitor ? monitor.height : 1080;
    _frame_w = w;
    _frame_h = h;

    const env = GLib.get_environ();
    env.push('REXON_WP_OUTPUT=' + FRAME_PATH);
    env.push('REXON_WP_WIDTH=' + w);
    env.push('REXON_WP_HEIGHT=' + h);
    env.push('REXON_WP_FPS=' + cfg.fps);

    // A new launch is a new life: the "we are stopping it ourselves" mark could remain from
    // last time if that daemon never reported its exit.
    _daemon_stopping = false;

    // ⚠️ WITHOUT `SpawnFlags.SEARCH_PATH`, and this is the main part of the fix: with it
    // the shell would silently take the FIRST matching program from its PATH, and
    // for a customer with a venv, a portable directory or Flatpak that is not the
    // Rexon that is installed. We launch exactly the file whose path
    // Rexon itself sent — and the path was checked for usability above.
    try {
        const [success, pid] = GLib.spawn_async(
            null, argv, env,
            GLib.SpawnFlags.DO_NOT_REAP_CHILD,
            null,
        );
        if (!success) {
            // The "returned false but did not throw" branch: previously nothing at all
            // happened here — not a single line in the log.
            _reportFailure(spawnFailureText(argv, null), cfg);
            return;
        }
        _daemon_pid = pid;
        const startedUs = GLib.get_monotonic_time();
        GLib.child_watch_add(GLib.PRIORITY_DEFAULT, pid, function(_p, status) {
            global.log('[rexon-wp] daemon exited, status=' + status);
            GLib.spawn_close_pid(_p);
            // Exit of the PREVIOUS host (stopped by us or a restart): the current one is already
            // a different one — its actors and number are left alone. Previously such a
            // late exit removed the actors of the host that had just come up.
            if (_daemon_pid !== _p) return;
            _daemon_pid = null;
            // We stopped the daemon ourselves (wallpaper change, shutdown) — this is not a failure.
            const stopping = _daemon_stopping;
            _daemon_stopping = false;
            if (stopping || status === 0) return;
            // The daemon died on its own: remove the black actors, otherwise the desktop would
            // stay a black rectangle without wallpaper.
            _stopTick();
            _removeActors();
            const livedMs = (GLib.get_monotonic_time() - startedUs) / 1000;
            if (isLaunchFailure(livedMs))
                _reportFailure(earlyExitText(argv, status), cfg);
            else
                global.log('[rexon-wp] показ обоев завершился на ходу (код ' +
                    status + ') — поднимет сторож Rexon');
        });
    } catch (e) {
        _reportFailure(spawnFailureText(argv, e), cfg);
    }
}

function _killDaemon() {
    if (_daemon_pid) {
        // Mark for child-watch: the next daemon exit is our own doing,
        // no need to complain about it.
        _daemon_stopping = true;
        try {
            GLib.spawn_command_line_async('kill ' + _daemon_pid);
        } catch (e) {
            global.logError('[rexon-wp] kill failed: ' + e);
        }
        _daemon_pid = null;
    }
}

// ── Background actors ────────────────────────────────────────────────
//
// Where to put the picture. Worked out from the Cinnamon 6.6 / Muffin 6.6 sources:
//
// * Cinnamon has NO `Main.layoutManager._bgManagers` — that is a GNOME Shell field.
//   The old code looped over an empty array, and the wallpaper never appeared at all.
// * The Muffin stack (`compositor.c`, `sync_actor_stacking`): inside
//   `global.window_group` from bottom to top — the X11 background (`global.background_actor`,
//   X11 only), `global.bottom_window_group` with DESKTOP-type windows,
//   the desklet container, normal windows. Any "foreign" actor in the window group
//   floats ABOVE the windows on restacking; only
//   Meta.BackgroundActor / Meta.BackgroundGroup stay in place — Muffin lowers them to the very
//   bottom.
// * On Wayland the background is drawn by csd-background: one XWayland window per monitor
//   of type DESKTOP with keep-below (`monitor-background.c`). Muffin puts it into
//   `bottom_window_group` and lowers it below the Nemo desktop window. The solid background window
//   covers everything below it, so an actor under it is not visible, and one above it in the same
//   group floats above Nemo and hides the icons.
//
// Hence two places:
// 1. A csd background window exists — the actor becomes a CHILD actor of its
//    MetaWindowActor: it is drawn on top of the csd picture and moves in the stack together with
//    the window, i.e. always under Nemo and under the windows. The monitor is the same as the
//    window's: csd keeps one window per monitor.
// 2. There is no csd background window (X11, or the csd background is off) — a Meta.BackgroundGroup
//    at the bottom of the window group (above the X11 background, if there is one) with an actor per
//    monitor. Muffin treats the group as background and keeps it under all windows.
const CSD_BACKGROUND_CLASS = 'csd-background';

function _isCsdBackground(windowActor) {
    if (!(windowActor instanceof Meta.WindowActor)) return false;
    const mw = windowActor.get_meta_window();
    if (!mw || mw.get_window_type() !== Meta.WindowType.DESKTOP) return false;
    const names = [mw.get_wm_class_instance(), mw.get_wm_class()];
    for (let i = 0; i < names.length; i++) {
        if (names[i] && names[i].toLowerCase() === CSD_BACKGROUND_CLASS) return true;
    }
    return false;
}

function _trackActor(actor, metaWindow, sizeId) {
    const entry = { actor: actor, metaWindow: metaWindow, sizeId: sizeId };
    _actors.push(entry);
    // The background window goes away (monitor change — csd recreates the windows) and takes
    // the child actor with it: we forget it, and if no background windows are left —
    // the fallback group takes over.
    actor.connect('destroy', function() {
        const i = _actors.indexOf(entry);
        if (i >= 0) _actors.splice(i, 1);
        _disconnectSize(entry);
        if (metaWindow) _queueFallbackCheck();
    });
}

function _disconnectSize(entry) {
    if (entry.metaWindow && entry.sizeId) {
        try {
            entry.metaWindow.disconnect(entry.sizeId);
        } catch (e) {
            // the window is already torn down — nothing to unsubscribe from
        }
    }
    entry.sizeId = 0;
}

// The frame is exactly the window frame: the child actor's coordinates are counted from
// the window buffer, and it may be wider than the frame.
function _fitToWindow(actor, mw) {
    const buf = mw.get_buffer_rect();
    const frame = mw.get_frame_rect();
    actor.set_position(frame.x - buf.x, frame.y - buf.y);
    actor.set_size(frame.width, frame.height);
}

function _attachToCsd(windowActor) {
    for (let i = 0; i < _actors.length; i++) {
        if (_actors[i].actor.get_parent() === windowActor) return;
    }
    const mw = windowActor.get_meta_window();
    // No background color: until the first frame the normal csd background is visible, not a black
    // rectangle.
    const actor = new Clutter.Actor({ reactive: false });
    _fitToWindow(actor, mw);
    windowActor.add_child(actor);
    const sizeId = mw.connect('size-changed', function() {
        _fitToWindow(actor, mw);
    });
    _trackActor(actor, mw, sizeId);
    _removeFallback();
}

function _csdBackgrounds() {
    const found = [];
    const group = global.bottom_window_group;
    if (!group) return found;
    const children = group.get_children();
    for (let i = 0; i < children.length; i++) {
        if (_isCsdBackground(children[i])) found.push(children[i]);
    }
    return found;
}

// Cinnamon with layer-shell: Wayland and the Cinnamon background service (its D-Bus
// activation file in the data directories: on Wayland it lives only on top of
// layer-shell). The same indicator Rexon uses when there is no way to ask the compositor
// (`rexon-core::wallpaper_backend::cinnamon_layer_shell`,
// `cinnamon_bg_service_files`): the shell has no way of its own to query protocols.
const CINNAMON_BG_SERVICE = 'org.Cinnamon.Background';

function _layerShellSession() {
    if (!Meta.is_wayland_compositor()) return false;
    const dirs = [GLib.get_user_data_dir()].concat(GLib.get_system_data_dirs());
    for (let i = 0; i < dirs.length; i++) {
        const file = dirs[i] + '/dbus-1/services/' + CINNAMON_BG_SERVICE + '.service';
        if (GLib.file_test(file, GLib.FileTest.EXISTS)) return true;
    }
    return false;
}

// Whether the Muffin bindings have a background group (before Cinnamon 6.8 — yes).
function _canBuildBackgroundGroup() {
    return typeof Meta.BackgroundGroup === 'function';
}

function _installFallback() {
    _removeFallback();
    if (!_canBuildBackgroundGroup()) return;
    const group = new Meta.BackgroundGroup();
    const monitors = Main.layoutManager.monitors;
    for (let i = 0; i < monitors.length; i++) {
        const m = monitors[i];
        const actor = new Clutter.Actor({
            x: m.x, y: m.y, width: m.width, height: m.height, reactive: false,
        });
        group.add_child(actor);
        _trackActor(actor, null, 0);
    }
    const parent = global.window_group;
    const x11bg = global.background_actor;
    if (x11bg && x11bg.get_parent() === parent) {
        parent.insert_child_above(group, x11bg);
    } else {
        parent.insert_child_at_index(group, 0);
    }
    _fallback = group;
}

function _removeFallback() {
    if (!_fallback) return;
    const group = _fallback;
    _fallback = null;
    group.destroy();   // child actors leave _actors on their own, on 'destroy'
}

function _queueFallbackCheck() {
    if (_fallbackIdle || !_bottomAddedId) return;
    _fallbackIdle = GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, function() {
        _fallbackIdle = 0;
        if (_bottomAddedId && !_fallback && _csdBackgrounds().length === 0) {
            _installFallback();
        }
        return GLib.SOURCE_REMOVE;
    });
}

function _installActors() {
    const csd = _csdBackgrounds();
    for (let i = 0; i < csd.length; i++) _attachToCsd(csd[i]);
    if (csd.length === 0) _installFallback();

    // csd often brings up the background windows later than the extension and recreates them on
    // monitor changes — we attach to each new one.
    if (global.bottom_window_group && !_bottomAddedId) {
        _bottomAddedId = global.bottom_window_group.connect('actor-added',
            function(_group, child) {
                if (_isCsdBackground(child)) _attachToCsd(child);
            });
    }
    if (!_monitorsChangedId) {
        _monitorsChangedId = Main.layoutManager.connect('monitors-changed',
            function() {
                if (_fallback) _installFallback();
            });
    }
    global.log('[rexon-wp] актёры обоев: ' + _actors.length + ' (' +
        (csd.length ? 'поверх фона csd-background' : 'своя группа фона') + ')');
}

function _removeActors() {
    if (_bottomAddedId) {
        global.bottom_window_group.disconnect(_bottomAddedId);
        _bottomAddedId = 0;
    }
    if (_monitorsChangedId) {
        Main.layoutManager.disconnect(_monitorsChangedId);
        _monitorsChangedId = 0;
    }
    if (_fallbackIdle) {
        GLib.source_remove(_fallbackIdle);
        _fallbackIdle = 0;
    }
    _removeFallback();
    const left = _actors.slice();
    _actors = [];
    for (let i = 0; i < left.length; i++) {
        _disconnectSize(left[i]);
        left[i].actor.destroy();
    }
}

// ── Frame tick ───────────────────────────────────────────────────────

function _startTick() {
    _tick_source = GLib.timeout_add(
        GLib.PRIORITY_DEFAULT, DAEMON_TICK_MS, _tick,
    );
}

function _stopTick() {
    if (_tick_source) {
        GLib.source_remove(_tick_source);
        _tick_source = 0;
    }
}

function _tick() {
    if (!_frame_w || !_frame_h) return GLib.SOURCE_CONTINUE;
    if (!GLib.file_test(FRAME_PATH, GLib.FileTest.EXISTS)) {
        return GLib.SOURCE_CONTINUE;  // the daemon has not started yet
    }

    let data;
    try {
        data = readWholeFrame(FRAME_PATH, _frame_w * _frame_h * 4);
    } catch (e) {
        global.logError('[rexon-wp] mmap read failed: ' + e);
        return GLib.SOURCE_CONTINUE;
    }
    if (!data) {
        return GLib.SOURCE_CONTINUE;  // no frame, incomplete or being written — wait
    }

    // One copy and one picture for all monitors: previously each actor
    // copied the file anew, and monitors could get different frames. The frame
    // is the size of the primary monitor and is stretched on the others.
    if (_actors.length === 0) return GLib.SOURCE_CONTINUE;
    let image;
    try {
        image = new Clutter.Image();
        image.set_data(
            data,
            Cogl.PixelFormat.RGBA_8888,
            _frame_w, _frame_h,
            _frame_w * 4,
        );
    } catch (e) {
        global.logError('[rexon-wp] Clutter.Image set failed: ' + e);
        return GLib.SOURCE_CONTINUE;
    }
    for (let i = 0; i < _actors.length; i++) {
        _actors[i].actor.set_content(image);
    }

    return GLib.SOURCE_CONTINUE;
}
