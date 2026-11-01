# Rexon Wallpaper Engine — GNOME Shell extension

Wrapper для запуска wallpaper-движков Rexon (`web_wallpaper`, `scene_engine`,
`app_wallpaper`) на GNOME Shell. Альтернатива `wlr-layer-shell`, который Mutter
не поддерживает.

**Поддержка:** GNOME Shell 45–49 (ESM-extension API). Wayland.

## Состояние

- ✅ `enable()` / `disable()` хуки
- ✅ Запуск демона обоев (бинарь `rexon-wallpaper-host`, строку запуска
  присылает сам Rexon в ключе `daemon-argv`) через `GLib.spawn_async` с pid-watch.
  ⚠️ **Без `SpawnFlags.SEARCH_PATH`:** запускается ровно тот файл, путь к
  которому прислал Rexon, — в `PATH` оболочки лежит не тот Rexon, что
  установлен, если он стоит в venv, переносном каталоге или Flatpak
- ✅ Отказ запуска объясняется человеку: путь проверяется до вызова оболочки
  (нет строки / имя вместо пути / файла нет / нет права на выполнение), текст
  уходит и в журнал, и всплывающим сообщением. Общий блок диагностики
  одинаков с расширением Cinnamon, сторож — `shell_extensions_*` в
  `rust/rexon-core/tests/wallpaper_mmap_parity.rs`
- ✅ `Clutter.Actor` под each monitor's background
- ✅ GSettings schema: `engine` / `source` / `fps`
- ✅ Сторона хоста: `REXON_WP_OUTPUT` режим — рендер в mmap-файл RGBA8888
  (`rexon-gui/cpp/rexon_wallpaper_host.cpp`) под `flock(LOCK_EX)`
- ✅ Frame-streaming в `_tick()`: `GLib.MappedFile` → `Clutter.Image.set_data` →
  `actor.set_content` → `queue_redraw` (30 FPS)
- ⏳ UI prefs (через `gnome-extensions prefs`) — TODO
- ⏳ Тест на реальной GNOME-машине (текущая dev-машина — KDE Plasma)

## Архитектура frame-streaming

```
┌─ rexon.parallax_engine (Python, headless) ────────┐
│  REXON_WP_OUTPUT=/run/user/$UID/...rgba        │
│  REXON_WP_WIDTH=1920, REXON_WP_HEIGHT=1080     │
│  ↓                                              │
│  SceneRenderer.update_and_render(painter, ...)  │
│  → QImage(mmap_buffer, Format_RGBA8888)         │
│  → flock(LOCK_EX) write кадра                   │
└────────────────────────────────────────────────┘
                       ↓
              [/run/user/$UID/rexon-wp-frame.rgba]
                       ↓
┌─ extension.js (gnome-shell process) ───────────┐
│  _tick() каждые 33ms:                          │
│    GLib.MappedFile.new(path, false)             │
│    Clutter.Image.set_data(bytes, RGBA_8888,...) │
│    actor.set_content(image); queue_redraw()     │
└────────────────────────────────────────────────┘
```

Хост (`rexon-wallpaper-host`, любой режим, на платформе Qt `offscreen`)
пишет кадр под `LOCK_EX`, но JS замок взять не может — у GJS нет `flock`.
`mmap` атомарного снимка НЕ даёт: замер на 1920×1080/30 кадрах — 12 рваных
кадров из 180 тиков. Поэтому рядом с кадром лежит счётчик поколения
`rexon-wp-frame.rgba.gen` (u64 LE, нечётный — кадр пишется): JS читает его до
и после копии кадра и пропускает тик, если счётчик нечётный или сменился
(`readWholeFrame` в общем блоке `[rexon-wp:frame-read]`). Раскладка самого
файла кадра прежняя — ровно width×height×4 байта без заголовка.

## Установка (для разработки)

```bash
# Скопировать в каталог пользователя
ext_dir="$HOME/.local/share/gnome-shell/extensions/rexon-wallpaper@rexon.app"
cp -r data/gnome-extension/rexon-wallpaper@rexon.app "$ext_dir"

# Скомпилировать gschema
glib-compile-schemas "$ext_dir/schemas/"

# Включить
gnome-extensions enable rexon-wallpaper@rexon.app

# Перезапустить gnome-shell на X11: Alt+F2 → r
# На Wayland: logout/login (Wayland не поддерживает горячий restart shell)
```

## Настройка (gsettings)

```bash
schema="org.gnome.shell.extensions.rexon-wallpaper"

# Параметры:
gsettings set $schema engine 'scene'
gsettings set $schema source '/path/to/scene.json'
gsettings set $schema fps 30
```

## Логи

```bash
journalctl --user -f -o cat /usr/bin/gnome-shell | grep -i rexon-wp
```

## Известные ограничения

- **GNOME Shell часто ломает extension API** (особенно с 40 → 41 → 42). При
  выходе нового GNOME Shell нужно проверить `metadata.json:shell-version` и
  обновить.
- **Mutter не предоставляет нативной wlr-layer-shell альтернативы** — мы
  «обёртываем» background actor extension'ом, что менее чисто чем layer-shell.
- **Производительность:** Cogl.Texture upload каждый кадр на 30 FPS = ~250
  МБ/сек на 1920×1080 RGBA8888. На слабых GPU может тормозить. Можно понизить
  до 15 FPS или использовать YUV/DMA-BUF (более продвинуто).

## Альтернативы

- **Перейти на KDE / sway / Hyprland** — нативный wlr-layer-shell без extension.
- **Использовать `gnome-shell-extension-blur-my-shell`** или подобные fork'и
  как пример работы с background-actor.
