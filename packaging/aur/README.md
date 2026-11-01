# Rexon — AUR-пакет (rexon-git)

Пакетные артефакты для Arch Linux / CachyOS / производных.
Автор: Wehrwolfmann.

## Состав

| Файл | Назначение |
|---|---|
| `PKGBUILD` | -git вариант: `pkgname=rexon-git`, `provides/conflicts=rexon` |
| `.SRCINFO` | сгенерирован `makepkg --printsrcinfo` (регенерировать при каждой правке PKGBUILD!) |

Статичных `.desktop` у пакета нет: `rexon.desktop` и `rexon-recover.desktop`
печатает при сборке генератор ядра (`rust/rexon-core/src/bin/desktop-entry.rs`,
`desktop-entry --package`) — вход на Rust-окно `/usr/lib/rexon/rexon-main-window`.
Сверка `ярлыки_пакета_печатает_генератор_ядра` в
`rust/rexon-core/tests/app_parity.rs` краснеет, если статичная копия вернётся
или путь установки окна разойдётся с ядром.

⚠️ **Python в пакете нет вовсе** (16.09.2026). `/usr/bin/rexon` — `sh`-лаунчер
на Rust-окно; интерпретатора, PySide6 и колеса `rexon_py` пакет не везёт.
Помощники polkit и их политики печатает при сборке второй бинарь ядра —
`package-polkit` (`rust/rexon-core/src/bin/package-polkit.rs`).

## Что ставит пакет (отличия от встроенного установщика)

Встроенный установщик Rexon (ядро: `rust/rexon-core/src/installer.rs` —
решения, `installer_exec.rs` — исполнение под pkexec) кладёт в
`/usr/local/lib/rexon`; пакет — в
**пакетные** пути, без какой-либо pkexec-логики (root даёт pacman):

- `/usr/lib/rexon/rexon/` — ДАННЫЕ продукта (не код): вендорный QML вкладки обоев
  с каталогом перевода, `qt_wallpaper_engine/proto/UPSTREAM_TAG`,
  `kpackage/org.waywallen.kde` (официальный архив выпуска waywallen-display
  без изменений + наш `vendor.toml`), `themes.json`, `assets/fonts`;
  рядом `/usr/lib/rexon/rexon_icon.png` — значок лотка (`tray::icon_candidates`);
- `/usr/lib/rexon/rexon-main-window` — Rust-окно, основной вход ярлыка (QML запечён в бинарь);
- `/usr/bin/{rexon-wallpaper-host,rexon-splash-overlay}` — хост обоев (режимы `--mode video|web`) и заставка входа (ядро ищет их по PATH);
- `/usr/lib/rexon/qml_modules/` — каталог QML-модулей (ядро ищет его как `qml_modules::SYSTEM_MODULE_DIRS`):
  - `Qcm/Material/libqml_materialplugin.so` + `libqml_material*.so` и `assets/` — оформление вендорных страниц (QmlMaterial, MIT);
  - `waywallen/control/libwaywallen-controlplugin.so` + `libwaywallen-control.so` — value-типы Qt-Protobuf;
  - `waywallen/control/librexon_qt_filter.so` — модель редактора фильтров (вкладки на QML вшиты в `rexon-main-window`);
- `/usr/bin/rexon` — `sh`-лаунчер: `REXON_PACKAGED=1` + `exec /usr/lib/rexon/rexon-main-window "$@"`
  (Rust-окно понимает все флаги продукта одной дверью ядра `cli::run_headless`);
- `/usr/lib/rexon-{de,pkg,stage,system}-helper` + `/usr/share/polkit-1/actions/org.rexon.*.policy` —
  привилегированные помощники и их политики (пароль реже: `auth_admin_keep` на 5 минут,
  подтверждённое `org.rexon.pkg` покрывает остальные три действия);
- `/usr/lib/rexon-priv-helper` (бинарь ядра, root 755) + `/usr/share/polkit-1/actions/org.rexon.session.policy` —
  помощник сеанса прав «один пароль на запуск»: окно поднимает его одним `pkexec`
  (`auth_admin` без keep, пароль держит сам помощник, пока открыто окно); действие
  не входит ни в правило «без пароля», ни в чей `imply`. Файл берётся из
  каталога сборки по `package-polkit --list-binaries` / `--path session`;
- `/usr/share/applications/{rexon,rexon-recover}.desktop` — печатает генератор ядра при сборке;
- `/usr/share/icons/hicolor/{16…512}/apps/rexon.png` (вектора у нового значка нет) + `/usr/share/pixmaps/rexon.png`;
- `/usr/share/{gnome-shell,cinnamon}/extensions/rexon-wallpaper@rexon.app/` (+ скомпилированные gschemas);
- `/usr/share/licenses/rexon-git/{LICENSE,NOTICE,LICENSE.QmlMaterial,LICENSE.NotoSans-OFL-1.1}`
  плюс движок автоперевода: `LICENSE.fxtranslate-MPL-2.0`,
  `LICENSE.gemmology-MIT`, `LICENSE.xsimd-BSD-3-Clause` и
  `fxtranslate-0.4.2-rexon.patch` — наши правки крейта одним файлом; именно
  его установкой закрывается пофайловый копилефт MPL-2.0 (разбор —
  `rust/vendor/fxtranslate/ATTRIBUTION.md`).

Кеши иконок / desktop-database / sycoca обновляют **pacman-хуки** — руками ничего не нужно.

### Пакетный режим (REXON_PACKAGED)

Мастер первого запуска не должен всплывать у человека с пакетом. Признаков
ДВА, и оба нужны:

1. `REXON_PACKAGED=1` от лаунчера `/usr/bin/rexon` — его читают
   `installer::detect_default_mode` (режим `portable`) и `first_run_offer`
   (`FirstRunSkip::PackagedEnv`);
2. **место своего файла** — ярлык зовёт `/usr/lib/rexon/rexon-main-window`
   напрямую, минуя лаунчер, и переменной там нет. `installer::packaged_location`
   узнаёт пакет по префиксу `/usr/` (кроме `/usr/local/`), `/nix/store/` и
   `/app/` → `FirstRunSkip::PackagedLocation`.

Выбор режима решает ядро; `prepare()` исходники Rexon не правит.

### Помощники polkit: почему `/usr/lib`, а не `/usr/local/lib`

Ручная установка кладёт помощников в `/usr/local/lib` — пакету туда нельзя:
нормы Arch запрещают пакетам писать в `/usr/local`, и `pacman -R` такие
файлы не снял бы. Путь знает ядро (`system_plan::HELPER_DIRS`: пакет —
`/usr/lib`, ручная установка — `/usr/local/lib`), выбор при запуске — по
факту наличия файла (`de_helper_path` / `pkg_helper_path` /
`stage_helper_path`). Политика обязана `annotate`'ить ровно тот путь, по
которому помощник лежит, — иначе pkexec наше действие не выберет и пароль
будет спрашиваться на КАЖДУЮ операцию; поэтому XML печатается под путь
(`*_policy_xml_for`). Passwordless-правило `/etc/polkit-1/rules.d/49-rexon-nopasswd.rules`
пакет НЕ ставит: это отдельный выбор человека.

## Лицензия

Rexon — **Apache License 2.0** (`LICENSE` и `NOTICE` в корне репозитория, © 2026 Wehrwolfmann).
Поэтому:

- `license=('Apache-2.0' …)` — вместе с лицензиями того, что едет в пакете (MIT, MPL-2.0,
  BSD-3-Clause, GPL-2.0-only/-or-later — см. `NOTICE`);
- LICENSE + NOTICE обязательно ставятся в `/usr/share/licenses/$pkgname/`.

Открытая лицензия снимает прежний вопрос AUR о легальном распространении исходника.
Формат `.rxn` — тоже Apache-2.0 (как и весь Rexon). GPL-компоненты (плагин Open Wallpaper Engine, плагин обоев
KDE) — отдельные программы со своими лицензиями и исходниками в поставке.

## Локальная сборка / проверка

```bash
# НЕ в /tmp: там tmpfs в ОЗУ, а target/ Rust-сборки — гигабайты
mkdir -p ~/rexon-pkgtest && cp PKGBUILD ~/rexon-pkgtest/
cd ~/rexon-pkgtest
makepkg --noarchive        # только сборка, без .pkg.tar.zst
makepkg -f                 # полный пакет → pacman -U (на свой страх)
namcap PKGBUILD; namcap *.pkg.tar.zst   # линт (пакет namcap)
```

`source` указывает на публичный репозиторий. Собрать из своего клона:
`REXON_GITURL="git+file://<путь к клону>" makepkg …` — собирается **только
committed**-состояние, незакоммиченные правки в пакет не попадают.

### Вариант с вшитым движком обоев (`REXON_BUNDLED_ENGINE=1`)

По умолчанию движок обоев — отдельные системные пакеты `waywallen` и
`open-wallpaper-engine` (так пакет уходит в AUR и CachyOS). Для поставки
Steam, ручной установки и машины, где этих пакетов нет и ставить их не
хочется, тот же PKGBUILD собирает пакет с движком внутри:

```bash
REXON_BUNDLED_ENGINE=1 makepkg -f
```

Что меняется при `1` (при `0` или без переменной — ничего):

- `depends`: вместо `waywallen>=…` и `open-wallpaper-engine>=…` —
  `fuse3` (рантайм AppImage) и `at-spi2-core` (CEF внутри плагина);
- источники: AppImage `waywallen-${_ww_ver}`, MIT waywallen, выпуск плагина
  OWE `${_owe_ver}`, git-тег OWE с зависимостями lito и архив CEF (из него
  берутся только исходники обёртки) — с прежними sha256;
- `/usr/lib/rexon/engine/waywallen.AppImage` и
  `plugins/org.waywallen.open-wallpaper-engine/` — там их ищет ядро
  (`we_client::ENGINE_DIR`);
- GPL-2.0-only §3(a): полный исходник OWE того же тега собирается в
  `build()` и ставится в `/usr/share/rexon/sources/`; лицензии waywallen
  (MIT), OWE (GPL-2.0) и CEF (BSD-3-Clause) — в `/usr/share/licenses/rexon-git/`,
  в `license=()` добавляется `GPL-2.0-only`. Уведомление — `NOTICE`.

С пакетами `waywallen`/`open-wallpaper-engine` такой пакет не конфликтует:
файлы комплекта лежат в своих каталогах. Стоят оба — ядро запускает
системный движок, комплект остаётся запасным. Другое значение переменной,
кроме `0` и `1`, `prepare()` отвергает. Сторож тега протокола один на оба
варианта: `_ww_min` и (во вшитом) `_ww_ver` обязаны совпасть с
`proto/UPSTREAM_TAG`, `_owe_ver` — не младше `_owe_min`.

`.SRCINFO` описывает только вариант по умолчанию (`makepkg --printsrcinfo`
без переменной): AUR и помощники вроде paru видят системные зависимости.

Почему переменная окружения, а не правка строки в начале файла: так же
подаётся `REXON_GITURL`; файл в репозитории и `.SRCINFO` остаются
вариантом по умолчанию, и правку «на один раз» нельзя случайно закоммитить.

## Публикация в AUR (когда появится публичный репозиторий)

1. Сверить `_giturl` в `PKGBUILD` с точным публичным адресом
   (сейчас — заглушка `git+https://github.com/wehrwolfmann/rexon.git`, TODO),
   добавить `url=`.
2. (Желательно) поставить git-тег `v5.0.0` в репозитории и упростить
   `pkgver()` до варианта с `git describe --long --tags`
   (см. wiki: VCS package guidelines). Сейчас тегов нет — версия
   собирается как `<version из rust/Cargo.toml>.r<rev-count>.g<hash>`.
3. Завести AUR-аккаунт, добавить SSH-ключ: https://aur.archlinux.org →
   My Account → SSH Public Key.
4. ```bash
   git clone ssh://aur@aur.archlinux.org/rexon-git.git aur-rexon-git
   cd aur-rexon-git
   cp /path/to/packaging/aur/PKGBUILD .
   makepkg --printsrcinfo > .SRCINFO     # ОБЯЗАТЕЛЬНО при каждом пуше
   git add PKGBUILD .SRCINFO
   git commit -m "rexon-git 5.0.0.r37"
   git push
   ```
5. Стабильный (не -git) пакет `rexon` — позже: source = tarball релиза с
   фиксированной sha256, без `pkgver()`.

## Известные ограничения

- Пакет конфликтует логически с установкой встроенным установщиком в
  `/usr/local` (pacman её не видит) — перед `pacman -U/-S` удалить её через
  Rexon «Удалить» либо вручную.
- Автозапуск пакет не ставит — его пишет трей по выбору человека.
- Редакция `Qcm.Material` записана в PKGBUILD дважды — `_qm_pin` и суммы
  шрифтов. `prepare()` падает, если `_qm_pin` отстал от вендорного
  `QML_MATERIAL_PIN`; при смене пина пересчитать sha256 пяти шрифтов
  (`makepkg -g`) и обновить `.SRCINFO`.
- Движок обоев — отдельные пакеты (с 30.10.2026): `depends` на
  `waywallen>=${_ww_min}` и `open-wallpaper-engine>=${_owe_min}`; AppImage,
  плагин OWE и его исходники пакет Rexon по умолчанию не несёт (вшитый
  вариант — `REXON_BUNDLED_ENGINE=1`, см. «Локальная сборка»). Оба пакета сейчас в
  AUR (ZhangHua): `makepkg -s` их не возьмёт — поставить заранее (`paru -S
  waywallen open-wallpaper-engine`). `waywallen-bin` версионную зависимость
  `open-wallpaper-engine` не закрывает (его `provides=waywallen` без версии).
  `_ww_min` обязан совпасть с `proto/UPSTREAM_TAG` — `prepare()` проверяет;
  при бумпе вендора поднять его, сверить договор плагинов
  (`SUPPORTED_ENTRY_VERSIONS`, `SPAWN_VERSION`) и `_owe_min`, обновить
  `.SRCINFO`; вшитому варианту — `_ww_ver`, `_owe_ver`, их sha256 (=
  `digest` выпуска GitHub), `_owe_git`/`_owe_reg` по `lito.lock` тега OWE и
  строки в `NOTICE`. `_qm_pin` — по `QML_MATERIAL_PIN` после
  `scripts/rexon-vendor-update`.
- `xdg-terminal-exec` и `yay` в optdepends — AUR-пакеты.
- `lito` в makedepends — тоже AUR-пакет (в официальных репозиториях его нет):
  им собирается QML-плагин QExtra. Перед сборкой поставить `lito` либо
  `lito-bin` (provides=lito) версии не ниже 0.8.7 — `makepkg -s` сам его из
  AUR не возьмёт; годный бинарь из другого места подаётся переменной `LITO`
  (и `makepkg -d`, раз зависимость не установлена пакетом). Каталог сборки
  (`BUILDDIR`) обязан быть путём из латиницы — `prepare()` иначе отказывает.
  Источники QExtra (`_qe_git`, `_qe_reg`) обязаны совпадать с
  `tools/qextra_plugin/lito.lock`; его выписывает `scripts/rexon-vendor-update`.
- Флаги `makepkg.conf` годятся любые (`-O2` Arch, `-O3` CachyOS, с `-g` и
  без): QExtra собирается профилем `release` lito, а он сам держит
  оптимизацию, отладочную информацию, LTO и strip — эти ключи и
  `_FORTIFY_SOURCE` `tools/build_qml_qextra.sh` из переданных lito флагов
  срезает, защитные (`-fstack-clash-protection`, `-fcf-protection`,
  `-D_GLIBCXX_ASSERTIONS`…) оставляет.
- Память. Сборка параллельная, и на машине с 8 ГБ ОЗУ её дважды убивал OOM:
  `rustc` на линковке и `cc1plus` на `control.qpb.cpp`. Число потоков
  ограничивают `CARGO_BUILD_JOBS` (Rust) и `-jN` в `MAKEFLAGS`
  (`makepkg.conf`): из него `build()` берёт `CMAKE_BUILD_PARALLEL_LEVEL`
  для `cmake --build` и число потоков lito, если они не заданы явно.
  На 8 ГБ проходит так:
  `CARGO_BUILD_JOBS=2 MAKEFLAGS=-j2 makepkg -f`.
