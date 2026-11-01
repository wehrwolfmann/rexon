# Как проверять Rexon

Сборка, тесты, стенды и отладка: что запускать и на что смотреть.

## ⚠️ Сборка Rust: голый `cargo build --release` НЕ собирает вкладки

Первое, что нужно знать перед любым замером UI.

В `rust/Cargo.toml` стоит `default-members = ["rexon-core"]` — намеренно,
чтобы голый `cargo test` ядра не тянул Qt6 (rexon-gui, rexon-qt-filter на
cxx-qt) и интерпретатор Python (rexon-py). Обратная сторона: **голый
`cargo build --release` собирает одно ядро**, рапортует «Finished» за
секунды и оставляет окно `rexon-main-window` с вкладками на QML старым.
Меряешь позавчерашний день, будучи уверен, что собрал сегодняшний. На это наступили 2026-08-29: сборка «прошла успешно»,
.so была двухдневной давности.

Замер после правки исходника вкладки: `cargo build --release` — «Finished
in 0.05s», сборка вкладок не тронута; `cargo build-app` — 1 мин 45 с, свежая.

Собирать — псевдонимами из `rust/.cargo/config.toml` (`cargo --list`
печатает их вместе с разворотом):

```bash
cd rust
cargo build-app          # ядро + rexon-main-window + librexon_qt_filter.so — ЭТО «собрать проект»
cargo build-app-debug    # то же в debug (release при этом остаётся старой!)
cargo build-splash       # бинарь заставки входа, отдельной командой (почему — ниже)
cargo test-core          # тесты ядра: без Qt и без Python
cd rexon-py && maturin build --release   # мост в Python: его собирает maturin, не cargo
```

Проверять глазами, а не по слову «Finished»:

```bash
ls -la --time-style=+%H:%M rust/target/release/rexon-main-window
```

⚠️ Колесо моста: `maturin failed … libssl-566862b9.so.3 could not be located`.
Хеш в имени библиотеки — примета УЖЕ починенного колесом файла: maturin
переиспользует `rust/target/maturin/librexon_py.so`, оставшийся от прошлой
сборки, и чинит починенное. Лечится удалением промежуточного каталога, а не
разбирательством с openssl:

```bash
rm -rf rust/target/maturin && (cd rust/rexon-py && maturin build --release)
```

Установка собранного колеса (PEP 668 иначе откажет):

```bash
pip install --user --force-reinstall --no-deps --break-system-packages \
    rust/target/wheels/rexon_py-*-manylinux*.whl
```

Почему заставка отдельной командой. `rexon-splash-overlay` тянет свой стек
(wayland/EGL). Когда он попадает в ОДИН прогон cargo вместе с `rexon-gui`,
объединение фич общих зависимостей (libc, rustix, bitflags, log…) меняется
и `rexon-gui` пересобирается целиком: чередование «всё одним прогоном» и
`build-app` жгло по 1 мин 40 с на ровном месте. Разными прогонами — 0,1 с.

⚠️ Псевдонимы читаются по текущему каталогу, а не по `--manifest-path`:
работают из `rust/` или изнутри крейта. Из корня репозитория
(`cargo build --manifest-path rust/Cargo.toml`) cargo этого файла не видит
— там набор пакетов пишем через `-p` руками.

Полная установка вкладок на машину (QML-модуль `waywallen.control` +
обе .so туда, где их ищет приложение) — `tools/build_qml_control.sh`;
он сам зовёт cargo для `rexon-gui` и `rexon-qt-filter`.

## Тестирование

```bash
# Проверка синтаксиса всех модулей
find rexon -name "*.py" | xargs -I{} python3 -m py_compile {}

# Запуск приложения
rexon                       # Единое Qt-окно (default)
rexon --tray                # сразу в системный лоток, без окна (запуск при входе)
rexon --desktops-qt         # одиночная Qt-вкладка «Рабочие столы»
rexon --compare-qt          # «Сравнение DE»
rexon --appearance-qt       # «Оформление»
rexon --extensions-qt       # «Расширения»
rexon --settings-qt         # «Настройки»
rexon --recover-kde         # аварийный откат KDE-темы

# Главное окно, у которого ХОЗЯИН — Rust (отдельный вход для разработчика).
# Виджетов и Python в процессе нет: QGuiApplication + ApplicationWindow на
# QML. ⚠️ Продукт запускается по-прежнему `rexon` и этого бинаря не знает.
rust/target/release/rexon-main-window                    # окно целиком
rust/target/release/rexon-main-window --section=steam    # сразу нужный раздел
QT_QPA_PLATFORM=offscreen rust/target/release/rexon-main-window --selftest
#   0 — проверки прошли, 1 — провал (печатает строки REXON-MAINWINDOW-FAIL),
#   2 — окно не отчиталось за 30 с. Проверка стенда на укус: тот же прогон с
#   `--section=steam` ОБЯЗАН покраснеть — у того раздела страницы ещё нет.

# Wallpaper-хост (standalone; прочие типы рисует движок waywallen).
# Python-хоста больше нет: обои показывает бинарь крейта rexon-gui.
# Хост один на видео и веб: режим — `--mode video|web` (без ключа — видео).
rust/target/release/rexon-wallpaper-host --mode video /path/to/video.mp4 [--windowed]
rust/target/release/rexon-wallpaper-host --mode video /path/to/wallpaper_dir  # WE-обои с project.json
rust/target/release/rexon-wallpaper-host --mode video /path/to/video.mp4 --selftest
rust/target/release/rexon-wallpaper-host --mode web /path/to/index.html --selftest  # код 5 — страница не загрузилась

# Forced X11 mode (для отладки fallback на Wayland-машине)
env -u WAYLAND_DISPLAY -u XDG_SESSION_TYPE QT_QPA_PLATFORM=xcb \
    rust/target/release/rexon-wallpaper-host --mode video /path/video.mp4 --windowed -v

# Проверка импортов
python3 -c "from rexon.extensions import Extension, ExtensionManager, VALID_TYPES; print('OK')"
python3 -c "from rexon.qt_main.window import RexonMainWindow; print('Qt OK')"
python3 -c "import rexon_py; print('helpers OK', rexon_py.wpm_is_wayland_session('wayland',''))"

# Установка в систему (потребует pkexec пароль)
python3 -c "from rexon.installer import RexonInstaller; ok,m=RexonInstaller().install_to_system(); print(ok,m)"

# Дамп всех потоков при зависании
kill -USR1 $(pgrep -f "python3 /usr/local/bin/rexon")

# Подсчёт строк
find rexon -name "*.py" | xargs wc -l | tail -1
```

---


## Agent tools

Установлены для усиления agent workflow (2026-05-22):

- **`ast-grep`** (`~/.local/bin/ast-grep`, v0.42.3) — AST-based grep вместо regex. Точно матчит синтаксические конструкции, игнорируя комментарии/строки. Пример:
  ```bash
  # Правильно (только подклассы):
  ast-grep --pattern 'class $NAME(BaseWallpaper)' --lang python rexon/
  # Неправильно (ловит и определение, и комментарий):
  grep -rn 'class.*BaseWallpaper' rexon/ --include='*.py'
  ```
  Использовать для рефакторинга, поиска определений, anti-pattern audit.

- **`imagehash` + `Pillow`** — perceptual hash для wallpaper regression detection. Установлены в venv `~/.local/share/rexon-tools-venv/` (`--system-site-packages`, Pillow 12.2.0 из системы, imagehash 4.3.2 в venv). Helper: `scripts/image-diff actual.png reference.png [threshold]` — exit 0 = match (≤ threshold phash distance), 1 = regression, 2 = error. Default threshold = 5.

- **`qmllint` Qt 6.11.1** (`/usr/lib/qt6/bin/qmllint`, через qt6-declarative) — pre-flight QML syntax/semantics check. Wrapper: `scripts/qmllint-check file.qml [...]` — exit 0 если только warnings, 1 если errors. Использовать ПОСЛЕ любого edit `.qml` файла — ДО kpackage rebuild и `plasmashell --replace`. Спасает от ситуации «правил весь день, узнал об ошибке после plasmashell restart».

## Эталоны: один вход на всё

Эталонов в дереве больше двух сотен, и обновлялись они четырьмя разными
способами. Теперь вход один — `scripts/rexon-golden-update`:

```
scripts/rexon-golden-update --check   # ничего не пишет, говорит, что разошлось (код ≠ 0)
scripts/rexon-golden-update           # обновляет всё и печатает дельту по каждому файлу
scripts/rexon-golden-update --list    # таблица «эталон → чем обновляется»
```

Прогон честно делит эталоны на три рода: снимаемые с кода, требующие ЖИВОГО
демона waywallen (или `lrelease`) и не поддающиеся пересъёмке вовсе. Второй род
без предпосылок обязан сказать «НЕ ПРОВЕРЕНО» и назвать причину — тихого
пропуска здесь нет. Полная карта с причинами — `docs/golden_reference_map.md`.

## Обновление вендорного QML waywallen — одна команда

Каталог `rexon/qt_wallpaper_engine/qml_ui/waywallen/ui` руками не правят:
его целиком пересобирает `scripts/sync-waywallen-qml.py`.

```
scripts/sync-waywallen-qml.py --ref v0.3.9   # забрать новый выпуск апстрима
scripts/sync-waywallen-qml.py --check        # 0 — дерево сошлось, 3 — разошлось
scripts/sync-waywallen-qml.py --git https://github.com/hypengw/waywallen --ref v0.3.9
```

За один прогон он: кладёт `ui/qml`, `ui/i18n` и вендоримые `ui/assets`
побайтово с тега; ГЕНЕРИРУЕТ `ui/qmldir` из их `ui/build.lua` (рукописных
строк там нет); накладывает нашу серию
`waywallen/patches/NNNN-*.patch`; выписывает `ui/QML_MATERIAL_PIN` —
ревизию `Qcm.Material` из `lito.lock` того же выпуска, её берёт
`tools/build_qml_material.sh`; печатает имена типов `W.*`, которые
вендорный QML зовёт, а наш модуль не регистрирует (код возврата 4).

Не лёг патч — это настоящий конфликт, дерево остаётся прежним, код 1.
Свою правку заводят так: поправить файл в `ui/`, снять по нему `git diff`
и положить в `patches/` следующим номером.

⚠️ После синхронизации пересобрать `Qcm.Material` — пин мог поехать:
`tools/build_qml_material.sh`.

## После обновления вендорного QML waywallen

Подписи вендорных страниц переводят ВЕНДОРНЫЕ каталоги
`data/qt_wallpaper_engine/qml_ui/waywallen/ui/i18n/waywallen_{ru,de}.ts`
(немецкий — с waywallen 0.5.0) — они вендорятся вместе с `ui/qml`, с того же
коммита, что указан в `ui/UPSTREAM_TAG`. Своего словаря подписей у нас нет и заводить его не нужно:
перевод чужой программы правится У НЕЁ (иначе он не достаётся никому, кроме
нас, и расходится с оригиналом).

Обновили вендора — обновить **обе** папки, `ui/qml` и `ui/i18n`, и пересобрать
каталог:

```
scripts/rexon-golden-update waywallen_qm   # каждый .ts → свой waywallen_<язык>.qm
cargo test -p rexon-core --test remote_l10n_parity
```

Тест краснеет, если каталог и `ui/qml` разошлись, и говорит, в какую сторону:
вендорные строки без перевода (покупатель увидит английский) или записи
каталога, которых в `ui/qml` уже нет. Строка у вендора новая и непереведённая —
перевод подаётся в waywallen, чтобы достался всем.

## Стенд помощника сеанса прав («один пароль на запуск»)

Привилегированная часть (`rust/rexon-core/src/privilege/`, бинарь
`rexon-priv-helper`) проверяется без root, без `pkexec`, без настоящего
`pacman` и без GUI — в три уровня:

1. **Юнит-тесты** модулей `privilege/*` — в обычном `cargo test-core`.
2. **Помощник обычным процессом** — `tests/priv_stand.rs` + бинарь
   `rexon-priv-stand`. Окно — настоящий `privilege::client::Client`; «шим
   pkexec» теста запускает помощника с сокетом на fd 0/1 и `PKEXEC_UID`, как
   pkexec после пароля. Помощник — тот же `helper::run_session`, но с «сухим»
   `RootIo`: программы не запускаются, их argv пишется в журнал стенда, файлы
   ложатся под корень-префикс `<стенд>/root`. logind, часы и счётчик сна —
   файлы стенда (`session`, `clock_ms`, `slept_ms`), их читают настоящие
   `LogindWatch` и `Revoker`. Сценарии: несколько операций одним паролем;
   закрытие окна (EOF) — выход; EOF и отзыв посреди операции — она
   доводится; блокировка, 15 минут простоя, сон, неактивный сеанс — новые
   команды старым правом не идут (нужен новый пароль); отказ на вечное
   (`linux-cachyos`, `mesa`, `glibc`, `systemd`) и на пакеты текущего стола;
   чужая сборка до пароля и в рукопожатии (`BuildMismatch`, липкий); чужой
   сеанс logind; чужой собеседник на сокете и наследование сокета дочками окна.
3. **Пространство имён пользователя** — `tests/priv_userns.rs`: НАСТОЯЩИЙ
   `rexon-priv-helper` (живой `LiveRootIo`) от uid 0 в `unshare -r --mount`.
   `packaging/test/priv-ns-enter.sh` bind-монтирует подменные `/usr/bin`
   (скрипты `pacman`, `loginctl`, `systemctl`), `/etc`, `/usr/share`,
   `/var/tmp` — их видит только помощник, на хосте не меняется ничего.
   Проверяются настоящие файлы: права темы SDDM (0644/0755), ссылка на месте
   приёмника заменяется, а не пишется насквозь, архивы со ссылкой, `..` и
   абсолютным путём отвергаются (`openat2`), рабочие каталоги убраны;
   окружение и дескрипторы программ помощника (чистое, без сокета окна); EOF
   посреди транзакции; `PR_SET_PDEATHSIG` (умер поток окна, сокет открыт).
   Нет `unshare` или ядро не даёт пространство имён — тест пишет
   «ПРОПУЩЕНО (стенд уровня 3): <причина>» и не падает.

```
scripts/priv-stand                  # уровни 2 и 3 (через build-queue)
scripts/priv-stand --release-check  # + в релизном rexon-priv-helper нет строк стенда
scripts/priv-stand -- screen_lock   # только тесты с этим словом в имени
# то же руками:
BUILD_QUEUE_SLOTS=1 scripts/build-queue test -p rexon-core --features stand --test priv_stand --test priv_userns
```

⛔ Feature `stand` — только для стенда: бинарь `rexon-priv-stand` и тест
`priv_stand` без неё не собираются (`required-features`), пакет и
`cargo build-app` её не включают. Код стенда живёт ТОЛЬКО в
`src/bin/rexon-priv-stand.rs`: библиотека и настоящий помощник от feature не
меняются. Сторож — `priv_userns::stand_code_stays_out_of_release` (идёт и в
обычном `cargo test-core`).

4. **ВМ** (настоящие pkexec, polkitd, logind, PLM) и итоговая приёмка пакетом —
   по плану, вне этого стенда.

## Pre-flight checks (обязательно после edit)

После каждого edit:
- `.py` → `python3 -m py_compile <file>` (+ если установлен — `ruff check <file>`)
- `.qml` → `scripts/qmllint-check <file>`
- `.rs` → `cd rust && cargo build-app` (голый `cargo build` вкладки не пересоберёт — см. раздел о ловушке выше)
- `.cpp/.hpp` → diagnostics в IDE (clang LSP); полный CMake build — вручную
- screenshot-based regression → `scripts/image-diff new.png reference.png`

---

