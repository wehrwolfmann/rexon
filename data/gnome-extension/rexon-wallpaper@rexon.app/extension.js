// Rexon Wallpaper Engine — GNOME Shell extension wrapper.
//
// Why this is needed at all: Mutter has no wlr-layer-shell, so wallpapers
// cannot be placed here as a window, as on KDE/sway/Hyprland. The waywallen
// engine is powerless here — the only way to show live wallpapers on
// GNOME-Wayland is a shell extension that reads ready-made frames.
//
// Architecture:
// 1. The wallpaper host is started as a background process via GLib.spawn_async
//    and draws the wallpaper into an invisible window. The launch line is SENT
//    by Rexon in the `daemon-argv` settings key: the extension is shell code,
//    it does not know where the wallpaper host binary lives. There is no
//    fallback: no line sent — the extension says so in the log and does not
//    start the wallpaper.
// 2. The daemon writes each frame into the shared mmap file /run/user/$UID/rexon-wp-frame.rgba
//    (RGBA8888, no header, under flock LOCK_EX).
// 3. This extension puts St.ImageContent on its own actors above the
//    GNOME Shell background (Main.layoutManager._bgManagers) and updates the
//    texture every tick. Clutter.Image was removed in mutter 48 — see makeFrameUploader.
// 4. The screen effect (scanlines, CRT and so on) is separate from the wallpaper:
//    the shader Rexon writes (key `effect-shader`) is applied as a
//    Shell.GLSLEffect on Main.uiGroup, i.e. on the whole screen. Wallpaper and
//    effect do not depend on each other: changing one does not touch the other.
//
// GNOME Shell 45+ requires ESM (import) syntax.

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';
import Cogl from 'gi://Cogl';
import * as Config from 'resource:///org/gnome/shell/misc/config.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';


const FRAME_PATH = GLib.get_user_runtime_dir() + '/rexon-wp-frame.rgba';
const DAEMON_TICK_MS = 33;  // ~30 FPS

// Adapter for shell versions: how to upload a frame into St.ImageContent.
//
// GNOME 45–47: St.ImageContent is a subclass of Clutter.Image, it has no
// set_data of its own, the inherited set_data(data, format, w, h, stride) works.
// GNOME 48+: Clutter.Image was removed from mutter (Clutter-51.gir has no
// Image class), St.ImageContent became standalone and requires a Cogl
// context as the first argument — set_data(cogl, data, format, w, h, stride).
// We get the context the same way the shell itself does (screenshot.js 48, qrCode.js 51):
// global.stage.context.get_backend().get_cogl_context().
// Previously there was `new Clutter.Image()` here: on 48+ it threw an exception
// every tick, and the actor's black backing covered the desktop.
function makeFrameUploader() {
    const major = parseInt(Config.PACKAGE_VERSION, 10);
    if (major >= 48) {
        const cogl = global.stage.context.get_backend().get_cogl_context();
        return (content, data, w, h) => content.set_data(
            cogl, data, Cogl.PixelFormat.RGBA_8888, w, h, w * 4);
    }
    return (content, data, w, h) => content.set_data(
        data, Cogl.PixelFormat.RGBA_8888, w, h, w * 4);
}

// The wallpaper daemon launch line — ONLY the one Rexon sent.
//
// The fallback `python3 -m rexon.video_wallpaper` is no longer here:
// the Python wallpaper host was removed from the tree, there is nothing to
// substitute the binary with. An empty array in the settings = "Rexon sent
// nothing"; this case is handled not here but in the shared block below —
// together with the other unusable launch lines, so that the explanation
// to the person is the same for all cases.
function _daemonArgv(settings) {
    // ⚠️ Whether the key exists is asked OF THE SCHEMA, not via try/catch around
    // the read. `get_strv` on a key that is not in the schema does not throw a
    // catchable exception: GLib prints
    // "does not contain a key named …" and KILLS THE PROCESS (verified —
    // SIGABRT, exit code 134). Here the process is the shell itself: instead of
    // "the wallpaper did not appear" the customer would get a session crash.
    const schema = settings.settings_schema;
    return (schema && schema.has_key('daemon-argv'))
        ? settings.get_strv('daemon-argv')
        : [];
}


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

export default class RexonWallpaperExtension extends Extension {

    enable() {
        this._settings = this.getSettings();
        this._daemon_pid = null;
        this._daemon_stopping = false;
        this._tick_source = 0;
        this._actors = [];
        this._content = null;

        this._effect = new ScreenEffect({
            target: Main.uiGroup,
            base: Shell.GLSLEffect,
            // GNOME 45–47: Shell.SnippetHook; 48+ accepts Cogl.SnippetHook.
            hook: Shell.SnippetHook ? Shell.SnippetHook.FRAGMENT : Cogl.SnippetHook.FRAGMENT,
            log: text => console.log(text),
            logError: text => console.error(text),
            unredirect: allow => {
                // mutter 44+: a compositor method; before that — a Meta function.
                const compositor = global.compositor;
                if (compositor && compositor.disable_unredirect) {
                    if (allow)
                        compositor.enable_unredirect();
                    else
                        compositor.disable_unredirect();
                } else if (allow) {
                    Meta.enable_unredirect_for_display(global.display);
                } else {
                    Meta.disable_unredirect_for_display(global.display);
                }
            },
        });

        // Rexon changes the wallpaper and the screen effect by writing the
        // extension settings. Without a subscription the new ones would appear only after
        // logging into the session again. The effect key does NOT restart the wallpaper: otherwise
        // changing the effect would stop and bring up the wallpaper host again.
        this._settings_changed_id = this._settings.connect(
            'changed', (_settings, key) => {
                if (key === EFFECT_KEY)
                    this._effect.queue(() => effectShaderPath(this._settings));
                else
                    this._reload();
            });

        this._reload();
        this._effect.apply(effectShaderPath(this._settings));
        console.log('[rexon-wp] enabled, daemon pid=' + this._daemon_pid);
    }

    disable() {
        if (this._settings_changed_id) {
            this._settings.disconnect(this._settings_changed_id);
            this._settings_changed_id = 0;
        }
        if (this._effect) {
            this._effect.destroy();
            this._effect = null;
        }
        this._stopTick();
        this._removeActors();
        this._killDaemon();
        this._settings = null;
        console.log('[rexon-wp] disabled');
    }

    // Reread the settings and restart the wallpaper accordingly.
    // Actors are placed ONLY when the wallpaper is actually running: otherwise a black
    // actor would cover the normal background and the desktop would turn black.
    _reload() {
        this._stopTick();
        this._removeActors();
        this._killDaemon();

        const config = this._loadConfig();
        if (!config) {
            console.log('[rexon-wp] обои не заданы — рабочий стол не трогаем');
            return;
        }

        this._spawnDaemon(config);
        if (!this._daemon_pid) return;
        this._installActors();
        this._startTick();
    }

    // ── Daemon control ─────────────────────────────────────────────

    _loadConfig() {
        // gsettings: org.gnome.shell.extensions.rexon-wallpaper
        // keys: 'engine' (string: video), 'source' (string: path),
        //         'fps' (int)
        const engine = this._settings.get_string('engine');
        const source = this._settings.get_string('source');
        if (!engine || !source) return null;
        return {
            engine: engine,
            source: source,
            fps: this._settings.get_int('fps') || 30,
        };
    }

    // Report the failure BOTH ways: in the log (details for analysis there) and
    // as a shell pop-up message — otherwise the customer only sees that
    // "the wallpaper does not work", and has nowhere to learn the reason.
    _reportFailure(text, cfg) {
        console.error('[rexon-wp] ' + text + ' [обои: движок «' +
            (cfg ? cfg.engine : '?') + '», источник «' +
            (cfg ? cfg.source : '?') + '»]');
        try {
            Main.notifyError(LAUNCH_FAIL_TITLE, text);
        } catch (e) {
            console.error('[rexon-wp] показать сообщение не вышло: ' + e.message);
        }
    }

    _spawnDaemon(cfg) {
        const argv = _daemonArgv(this._settings);
        const problem = launchProblem(argv);
        if (problem) {
            this._reportFailure(problem, cfg);
            return;
        }

        const monitor = Main.layoutManager.primaryMonitor;
        const w = monitor ? monitor.width : 1920;
        const h = monitor ? monitor.height : 1080;
        this._frame_w = w;
        this._frame_h = h;

        // ENV: REXON_WP_OUTPUT — path to the mmap frame file (parsed by
        // rexon-core::wallpaper_mmap),
        // REXON_WP_WIDTH/HEIGHT — frame size (equal to the primary monitor size),
        // REXON_WP_FPS — refresh rate.
        const env = GLib.get_environ();
        env.push('REXON_WP_OUTPUT=' + FRAME_PATH);
        env.push('REXON_WP_WIDTH=' + w);
        env.push('REXON_WP_HEIGHT=' + h);
        env.push('REXON_WP_FPS=' + cfg.fps);

        // A new launch is a new life: the "we are stopping it ourselves" mark could remain
        // from last time if that daemon never reported its exit.
        this._daemon_stopping = false;

        // ⚠️ WITHOUT `SpawnFlags.SEARCH_PATH`, and this is the main part of the fix: with it
        // the shell would silently take the FIRST matching program from its PATH,
        // and for a customer with a venv, a portable directory or Flatpak that is not the
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
                this._reportFailure(spawnFailureText(argv, null), cfg);
                return;
            }
            this._daemon_pid = pid;
            const startedUs = GLib.get_monotonic_time();
            GLib.child_watch_add(GLib.PRIORITY_DEFAULT, pid, (_pid, status) => {
                console.log('[rexon-wp] daemon exited, status=' + status);
                GLib.spawn_close_pid(_pid);
                // Exit of the PREVIOUS host (stopped by us or a restart): the current one
                // is already a different one — its actors and number are left alone. Previously such a
                // late exit removed the actors of the host that had just come up.
                if (this._daemon_pid !== _pid) return;
                this._daemon_pid = null;
                // We stopped the daemon ourselves (wallpaper change, shutdown) — this is not a failure.
                const stopping = this._daemon_stopping;
                this._daemon_stopping = false;
                if (stopping || status === 0) return;
                // The daemon died on its own: remove the black actors, otherwise the desktop
                // would stay a black rectangle without wallpaper.
                this._stopTick();
                this._removeActors();
                const livedMs = (GLib.get_monotonic_time() - startedUs) / 1000;
                if (isLaunchFailure(livedMs))
                    this._reportFailure(earlyExitText(argv, status), cfg);
                else
                    console.warn('[rexon-wp] показ обоев завершился на ходу (код ' +
                        status + ') — поднимет сторож Rexon');
            });
        } catch (e) {
            this._reportFailure(spawnFailureText(argv, e.message), cfg);
        }
    }

    _killDaemon() {
        if (this._daemon_pid) {
            // Mark for child-watch: the next daemon exit is our own
            // doing, no need to complain about it.
            this._daemon_stopping = true;
            try {
                GLib.spawn_command_line_async(`kill ${this._daemon_pid}`);
            } catch (e) {
                console.error('[rexon-wp] kill failed: ' + e.message);
            }
            this._daemon_pid = null;
        }
    }

    // ── Background actors ──────────────────────────────────────────

    _installActors() {
        // GNOME Shell has one bg actor per monitor:
        // Main.layoutManager._bgManagers[i].backgroundActor.
        // We add our actor (St.Widget) above the default wallpaper, below the windows.
        // The screen size is passed in ENV to the wallpaper host so the frame matches.
        const bgManagers = Main.layoutManager._bgManagers || [];
        for (const bgm of bgManagers) {
            const monitor = bgm._monitorIndex !== undefined
                ? Main.layoutManager.monitors[bgm._monitorIndex]
                : Main.layoutManager.primaryMonitor;
            if (!monitor) continue;

            const actor = new St.Widget({
                width: monitor.width,
                height: monitor.height,
                x: monitor.x,
                y: monitor.y,
            });
            actor._monitor_w = monitor.width;
            actor._monitor_h = monitor.height;
            const parent = bgm.backgroundActor.get_parent();
            if (parent) {
                parent.insert_child_above(actor, bgm.backgroundActor);
            }
            this._actors.push(actor);
        }
    }

    _removeActors() {
        for (const a of this._actors) {
            const parent = a.get_parent();
            if (parent) parent.remove_child(a);
            a.destroy();
        }
        this._actors = [];
        // The content is tied to the frame size — on a wallpaper restart the
        // size may have changed, so it is created anew on the first frame.
        this._content = null;
    }

    // ── Frame tick (stub) ──────────────────────────────────────

    _startTick() {
        this._tick_source = GLib.timeout_add(
            GLib.PRIORITY_DEFAULT, DAEMON_TICK_MS,
            () => this._tick(),
        );
    }

    _stopTick() {
        if (this._tick_source) {
            GLib.source_remove(this._tick_source);
            this._tick_source = 0;
        }
    }

    _tick() {
        // Read the mmap file → St.ImageContent → actor.content on every monitor.
        // GLib.MappedFile takes a zero-copy view of the file; on the next call
        // create a new mapping to get a fresh snapshot (the wallpaper host writes under
        // flock, mmap reflects the current state of the file).
        if (!this._frame_w || !this._frame_h) {
            return GLib.SOURCE_CONTINUE;
        }
        if (!GLib.file_test(FRAME_PATH, GLib.FileTest.EXISTS)) {
            return GLib.SOURCE_CONTINUE;  // the daemon has not started rendering yet
        }

        let data;
        try {
            data = readWholeFrame(FRAME_PATH, this._frame_w * this._frame_h * 4);
        } catch (e) {
            console.warn('[rexon-wp] mmap read failed: ' + e.message);
            return GLib.SOURCE_CONTINUE;
        }
        if (!data) {
            return GLib.SOURCE_CONTINUE;  // no frame, incomplete or being written — wait
        }

        // One copy for all monitors: previously each actor copied the file
        // anew, and monitors could get different frames. The content is also
        // one — actors of different monitors show the same frame.
        try {
            if (!this._content) {
                this._content = St.ImageContent.new_with_preferred_size(
                    this._frame_w, this._frame_h);
                this._upload = makeFrameUploader();
            }
            this._upload(this._content, data, this._frame_w, this._frame_h);
        } catch (e) {
            console.warn('[rexon-wp] St.ImageContent set failed: ' + e.message);
            return GLib.SOURCE_CONTINUE;
        }
        for (const actor of this._actors) {
            if (actor.get_content() !== this._content)
                actor.set_content(this._content);
        }

        return GLib.SOURCE_CONTINUE;
    }
}
