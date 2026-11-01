//! Системный слой — порт `rexon/system.py` (ТОЛЬКО чистая логика).
//!
//! Портировано:
//!   * `PkgFamily` — семейство пакетного менеджера (enum);
//!   * `DistroDetector.ID_MAP` + детекция дистрибутива: разбор os-release
//!     (текст подаётся параметром, файловый фолбэк /etc → /usr/lib — в
//!     `parse_os_release_files`; живое чтение — тонкая обёртка
//!     `DistroDetector::detect_live`), детекция по бинарникам вынесена за
//!     предикат `has_binary` (как `command_exists` в settings.rs);
//!   * `PackageDB` — вся встроенная база DE (`DE_DATABASE`, 15 DE, включая
//!     LXQt, Wayfire и тайловые niri/sway/river, × семейства пакетов) + мета для UI
//!     (`DE_META`) и функции выбора
//!     пакетов для (de, family);
//!   * чистые алгоритмы установщика: `expand_groups` (раскрытие групп
//!     pacman), `parse_qi_field`/`qi_field` (разбор `pacman -Qi`),
//!     `required_by_outside`, `filter_removable_transitive` (forbidden set +
//!     транзитивное замыкание по `Depends On`). Вывод pacman подаётся
//!     замыканием-фикстурой; live-вызовы `pacman -Qg`/`-Qi` (под LANG=C —
//!     русская локаль ломает разбор полей, см. тест `russian-locale-*`)
//!     остаются на стороне исполнителя.
//!
//! ГРАНИЦА. Всё, что ВЫПОЛНЯЕТ — запускает команды, ходит в pkexec, читает
//! живую файловую систему, — лежит в двух соседях, а не здесь:
//!   * решения и argv (тексты helper-скриптов, polkit-политики, планы
//!     установки, откат форков Plasma / Plymouth / SDDM, детект экрана входа)
//!     — [`crate::system_plan`];
//!   * сами походы наружу — [`crate::system_exec`]: `pkexec`-хелперы,
//!     `systemctl is-enabled`, опрос пакетного менеджера, каталоги
//!     `/etc/sddm.conf.d` и `/usr/share/plymouth/themes`, `logout_session`.
//!     Побочные вызовы там инъектируются трейтом `SystemIo`, поэтому
//!     исполнитель проверяется без живой машины.
//!
//! НА ПИТОНЕ ПОКА ОСТАЛОСЬ (следующий заход): установка, переключение и
//! удаление рабочих столов (`install_de`/`switch_de`/`remove_de`,
//! `PackageManager.install/install_batch/remove/update_db/run_post_install`,
//! `ensure_display_manager`, пересборка AeroShell) и живые проверки
//! установленности `SystemInfo`/`DesktopEnvironment`.
//!
//! Каждая портированная функция сверяется с Python-оракулом golden-тестами
//! `tests/system_parity.rs` (фикстуры `tests/golden/system_*.json` сняты с
//! живого `rexon.system`).

use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;

use serde::{Serialize, Serializer};
use serde_json::{Map, Value};

use crate::i18n::n_;

// ============================================================================
// PkgFamily
// ============================================================================

/// Семейство пакетного менеджера — порт `PkgFamily(str, Enum)`.
///
/// Значения совпадают со строковыми `PkgFamily.<X>.value`, чтобы сериализация
/// (`family.value`) и разбор os-release давали то же, что Python.
/// `distro_compat.rs` реэкспортирует этот тип (в Python enum живёт в
/// system.py, distro_compat.py его импортирует).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkgFamily {
    Arch,
    Debian,
    Fedora,
    Suse,
    Gentoo,
    Alpine,
    Void,
    Nix,
    Solus,
    Clear,
    Alt,
    Unknown,
}

impl PkgFamily {
    /// Строковое значение enum — как `PkgFamily.<X>.value` в Python.
    pub fn as_str(self) -> &'static str {
        match self {
            PkgFamily::Arch => "arch",
            PkgFamily::Debian => "debian",
            PkgFamily::Fedora => "fedora",
            PkgFamily::Suse => "suse",
            PkgFamily::Gentoo => "gentoo",
            PkgFamily::Alpine => "alpine",
            PkgFamily::Void => "void",
            PkgFamily::Nix => "nix",
            PkgFamily::Solus => "solus",
            PkgFamily::Clear => "clear",
            PkgFamily::Alt => "alt",
            PkgFamily::Unknown => "unknown",
        }
    }

    /// Разбор из строкового значения (обратно к `as_str`); неизвестное → None.
    /// Имя `from_value` — не `from_str` — чтобы не путать со `std::str::FromStr`
    /// (тот возвращает `Result`, здесь семантика Python `PkgFamily(value)`).
    pub fn from_value(s: &str) -> Option<Self> {
        Some(match s {
            "arch" => PkgFamily::Arch,
            "debian" => PkgFamily::Debian,
            "fedora" => PkgFamily::Fedora,
            "suse" => PkgFamily::Suse,
            "gentoo" => PkgFamily::Gentoo,
            "alpine" => PkgFamily::Alpine,
            "void" => PkgFamily::Void,
            "nix" => PkgFamily::Nix,
            "solus" => PkgFamily::Solus,
            "clear" => PkgFamily::Clear,
            "alt" => PkgFamily::Alt,
            "unknown" => PkgFamily::Unknown,
            _ => return None,
        })
    }
}

/// Сериализуется как строковое значение (аналог `PkgFamily.value` в дампах).
impl Serialize for PkgFamily {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}

// ============================================================================
// DistroDetector
// ============================================================================

/// Разбор одного текста os-release — порт `DistroDetector._parse_os_release`
/// (тело цикла): строки с `=` и не-комментарии; ключ/значение обрезаются,
/// значение чистится от обрамляющих `"` затем `'` (именно в этом порядке,
/// как `strip('"').strip("'")`). Повторный ключ перезаписывает значение,
/// сохраняя позицию (семантика dict).
pub fn parse_os_release(text: &str) -> Map<String, Value> {
    let mut result = Map::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.contains('=') && !line.starts_with('#') {
            let (k, v) = line.split_once('=').expect("contains('=') проверен");
            let v = v.trim().trim_matches('"').trim_matches('\'');
            result.insert(k.trim().to_string(), Value::from(v));
        }
    }
    result
}

/// Фолбэк по файлам — порт цикла `for path in ("/etc/os-release",
/// "/usr/lib/os-release")`: ПЕРВЫЙ существующий файл разбирается и
/// возвращается (даже если он пуст — второй уже не смотрим); нет обоих → {}.
/// Параметры — содержимое файлов (None = FileNotFoundError).
pub fn parse_os_release_files(
    etc: Option<&str>,
    usr_lib: Option<&str>,
) -> Map<String, Value> {
    if let Some(text) = etc {
        return parse_os_release(text);
    }
    if let Some(text) = usr_lib {
        return parse_os_release(text);
    }
    Map::new()
}

/// Есть ли исполняемый файл в PATH — аналог `shutil.which(name) is not None`.
/// Живая обёртка для предиката `has_binary`; тесты подают предикат-фикстуру.
pub fn binary_in_path(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&paths) {
        let p = dir.join(name);
        if let Ok(md) = std::fs::metadata(&p) {
            if md.is_file() && md.permissions().mode() & 0o111 != 0 {
                return true;
            }
        }
    }
    false
}

/// Поиск семейства по `ID_MAP` (линейный по const-таблице).
pub fn id_map_lookup(id: &str) -> Option<PkgFamily> {
    ID_MAP.iter().find(|(k, _)| *k == id).map(|(_, f)| *f)
}

/// Определяет дистрибутив и семейство пакетного менеджера —
/// порт класса `DistroDetector` (без чтения файлов в конструкторе:
/// `_info` строится из текста os-release, поданного параметром).
pub struct DistroDetector {
    /// Разобранный os-release — `self._info` Python.
    pub info: Map<String, Value>,
}

impl DistroDetector {
    /// Конструктор из содержимого os-release файлов (None = файла нет).
    pub fn from_os_release_files(etc: Option<&str>, usr_lib: Option<&str>) -> Self {
        Self { info: parse_os_release_files(etc, usr_lib) }
    }

    /// Живая детекция: читает /etc/os-release, затем /usr/lib/os-release —
    /// как `DistroDetector()` в Python (ошибка чтения = отсутствие файла).
    pub fn detect_live() -> Self {
        let etc = std::fs::read_to_string("/etc/os-release").ok();
        let usr_lib = std::fs::read_to_string("/usr/lib/os-release").ok();
        Self::from_os_release_files(etc.as_deref(), usr_lib.as_deref())
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.info.get(key).and_then(Value::as_str)
    }

    /// Имя дистрибутива / ОС: NAME → PRETTY_NAME → "Linux" (по НАЛИЧИЮ
    /// ключа, не по непустоте — пустой NAME="" возвращается как есть).
    pub fn name(&self) -> String {
        if let Some(v) = self.get("NAME") {
            return v.to_string();
        }
        if let Some(v) = self.get("PRETTY_NAME") {
            return v.to_string();
        }
        "Linux".to_string()
    }

    /// Красивое имя: PRETTY_NAME → name().
    pub fn pretty_name(&self) -> String {
        match self.get("PRETTY_NAME") {
            Some(v) => v.to_string(),
            None => self.name(),
        }
    }

    /// ID дистрибутива (из os-release), в нижнем регистре.
    pub fn distro_id(&self) -> String {
        self.get("ID").unwrap_or("").to_lowercase()
    }

    /// Родительские дистрибутивы (ID_LIKE), в нижнем регистре.
    pub fn id_like(&self) -> Vec<String> {
        self.get("ID_LIKE")
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_lowercase)
            .collect()
    }

    /// Версия дистрибутива (VERSION_ID) или "".
    pub fn version_id(&self) -> String {
        self.get("VERSION_ID").unwrap_or("").to_string()
    }

    /// Семейство пакетного менеджера — порт свойства `family`.
    ///
    /// Порядок: прямое совпадение ID → фолбэк на ID_LIKE → детекция по
    /// бинарникам через предикат `has_binary` (live — `binary_in_path`).
    /// Особый случай APT-RPM: на ALT есть apt-get + rpm, но НЕТ dpkg.
    pub fn family(&self, has_binary: &dyn Fn(&str) -> bool) -> PkgFamily {
        if let Some(f) = id_map_lookup(&self.distro_id()) {
            return f;
        }
        for like in self.id_like() {
            if let Some(f) = id_map_lookup(&like) {
                return f;
            }
        }
        if has_binary("apt-get") {
            if has_binary("dpkg") {
                return PkgFamily::Debian;
            }
            if has_binary("rpm") {
                return PkgFamily::Alt;
            }
            return PkgFamily::Debian; // предполагаем Debian
        }
        for (binary, family) in [
            ("pacman", PkgFamily::Arch),
            ("dnf", PkgFamily::Fedora),
            ("yum", PkgFamily::Fedora),
            ("zypper", PkgFamily::Suse),
            ("emerge", PkgFamily::Gentoo),
            ("apk", PkgFamily::Alpine),
            ("xbps-install", PkgFamily::Void),
            ("nix-env", PkgFamily::Nix),
            ("eopkg", PkgFamily::Solus),
            ("swupd", PkgFamily::Clear),
        ] {
            if has_binary(binary) {
                return family;
            }
        }
        PkgFamily::Unknown
    }

    /// Семейство с живой детекцией бинарников (PATH).
    pub fn family_live(&self) -> PkgFamily {
        self.family(&binary_in_path)
    }

    /// Это SteamOS?
    pub fn is_steamos(&self) -> bool {
        self.distro_id().contains("steamos") || self.name().to_lowercase().contains("steamos")
    }

    /// Иммутабельный дистрибутив?
    pub fn is_immutable(&self) -> bool {
        const IMMUTABLE_IDS: [&str; 7] = [
            "steamos", "nixos", "bazzite", "silverblue", "kinoite", "sericea", "onyx",
        ];
        let name = self.name().to_lowercase();
        IMMUTABLE_IDS.contains(&self.distro_id().as_str())
            || name.contains("atomic")
            || name.contains("silverblue")
            || self
                .get("VARIANT_ID")
                .unwrap_or("")
                .to_lowercase()
                .contains("ostree")
    }
}

// ============================================================================
// PackageDB — ВСТРОЕННАЯ база пакетов (порт класса PackageDB)
// ============================================================================

/// Запись `_DE_DATABASE[de_id]`. Списки пакетов — в порядке вставки
/// Python-словаря (для лукапа порядок семейств не важен, но сохранён).
pub struct DeEntry {
    pub packages: &'static [(PkgFamily, &'static [&'static str])],
    pub aur: &'static [&'static str],
    /// Имя ПАКЕТА для вопроса пакетному менеджеру «стоит ли DE». Не бинарник:
    /// исполняемый файл зовётся иначе (`plasma-desktop` → `plasmashell`,
    /// `hyprland` → `Hyprland`). См. [`get_check_binary`].
    pub check_package: &'static str,
    pub config_paths: &'static [&'static str],
}

/// Нагрузка стола на процессор — ПРИЗНАК, а не подпись.
///
/// Раньше это была русская строка («Низкое»/«Среднее»/«Высокое»), и
/// «Сравнение» красило ячейку, сравнивая её текст со списком; с переводом
/// подпись у каждого языка своя, поэтому решение о цвете опирается на
/// вариант, а текст — только показ ([`CpuLoad::label`]). `Display` и
/// `Serialize` отдают готовую подпись на языке интерфейса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuLoad {
    Low,
    Medium,
    High,
}

impl CpuLoad {
    /// Подпись на языке интерфейса.
    pub fn label(self) -> String {
        match self {
            CpuLoad::Low => crate::i18n::trc("CPU load", "Low"),
            CpuLoad::Medium => crate::i18n::trc("CPU load", "Medium"),
            CpuLoad::High => crate::i18n::trc("CPU load", "High"),
        }
    }
}

impl std::fmt::Display for CpuLoad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

impl Serialize for CpuLoad {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.label())
    }
}

/// Сверки сравнивают готовую подпись ячейки с признаком стола.
impl PartialEq<CpuLoad> for String {
    fn eq(&self, other: &CpuLoad) -> bool {
        *self == other.label()
    }
}

/// Мета-информация DE для UI — элемент `_DE_META`. Порядок полей совпадает
/// с порядком ключей Python-словаря (важен для сверки дампов).
///
/// Тексты для человека (`description`, `compositor` у LXQt) — английские
/// `msgid` каталога перевода; показывать их через [`DeMeta::description_text`]
/// и [`DeMeta::compositor_text`], а не полем напрямую. Остальные строки —
/// имена программ и технологий, они не переводятся.
#[derive(Serialize)]
pub struct DeMeta {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub session_name: &'static str,
    pub ram_mb: u32,
    pub cpu_usage: CpuLoad,
    pub customizable: u8,
    pub gaming_friendly: u8,
    pub toolkit: &'static str,
    pub session_type: &'static str,
    pub compositor: &'static str,
    pub panel_type: &'static str,
    pub file_manager: &'static str,
    pub terminal: &'static str,
}

impl DeMeta {
    /// Описание стола на языке интерфейса.
    pub fn description_text(&self) -> String {
        crate::i18n::tr(self.description)
    }

    /// Композитор на языке интерфейса (у большинства — имя программы).
    pub fn compositor_text(&self) -> String {
        crate::i18n::tr(self.compositor)
    }
}

/// `DistroDetector.ID_MAP`: os-release ID → семейство, в порядке вставки.
pub const ID_MAP: &[(&str, PkgFamily)] = &[
    ("arch", PkgFamily::Arch),
    ("cachyos", PkgFamily::Arch),
    ("manjaro", PkgFamily::Arch),
    ("endeavouros", PkgFamily::Arch),
    ("garuda", PkgFamily::Arch),
    ("artix", PkgFamily::Arch),
    ("steamos", PkgFamily::Arch),
    ("archcraft", PkgFamily::Arch),
    ("blendos", PkgFamily::Arch),
    ("archbang", PkgFamily::Arch),
    ("crystal", PkgFamily::Arch),
    ("debian", PkgFamily::Debian),
    ("ubuntu", PkgFamily::Debian),
    ("linuxmint", PkgFamily::Debian),
    ("pop", PkgFamily::Debian),
    ("elementary", PkgFamily::Debian),
    ("zorin", PkgFamily::Debian),
    ("kali", PkgFamily::Debian),
    ("parrot", PkgFamily::Debian),
    ("mx", PkgFamily::Debian),
    ("mxlinux", PkgFamily::Debian),
    ("lmde", PkgFamily::Debian),
    ("raspbian", PkgFamily::Debian),
    ("peppermint", PkgFamily::Debian),
    ("tuxedo", PkgFamily::Debian),
    ("fedora", PkgFamily::Fedora),
    ("rhel", PkgFamily::Fedora),
    ("centos", PkgFamily::Fedora),
    ("almalinux", PkgFamily::Fedora),
    ("rocky", PkgFamily::Fedora),
    ("ol", PkgFamily::Fedora),
    ("nobara", PkgFamily::Fedora),
    ("ultramarine", PkgFamily::Fedora),
    ("bazzite", PkgFamily::Fedora),
    ("opensuse", PkgFamily::Suse),
    ("opensuse-leap", PkgFamily::Suse),
    ("opensuse-tumbleweed", PkgFamily::Suse),
    ("sle", PkgFamily::Suse),
    ("sles", PkgFamily::Suse),
    ("gentoo", PkgFamily::Gentoo),
    ("alpine", PkgFamily::Alpine),
    ("void", PkgFamily::Void),
    ("nixos", PkgFamily::Nix),
    ("solus", PkgFamily::Solus),
    ("clear-linux-os", PkgFamily::Clear),
    ("alt", PkgFamily::Alt),
    ("altlinux", PkgFamily::Alt),
    ("simply", PkgFamily::Alt),
    ("astra", PkgFamily::Debian),
    ("osnova", PkgFamily::Debian),
    ("boss", PkgFamily::Debian),
    ("redos", PkgFamily::Fedora),
    ("rosa", PkgFamily::Fedora),
    ("calculate", PkgFamily::Gentoo),
];

/// `_DE_DATABASE`: id DE → пакеты по семействам / AUR / проверка /
/// конфиги. Транскрипция сгенерирована из живого Python (см. заголовок
/// `tests/system_parity.rs`) и сверяется golden-тестом целиком.
pub const DE_DATABASE: &[(&str, DeEntry)] = &[
    // cinnamon-mint
    ("cinnamon-mint", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "cinnamon", "cinnamon-translations", "nemo", "nemo-fileroller",
                "gnome-terminal", "lightdm", "lightdm-slick-greeter",
            ]),
            (PkgFamily::Debian, &[
                "cinnamon-desktop-environment", "lightdm",
                "lightdm-gtk-greeter", "nemo",
            ]),
            (PkgFamily::Fedora, &[
                "@cinnamon-desktop-environment", "cinnamon-themes",
                "mint-y-theme", "mint-y-icons", "mint-x-icons", "lightdm",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Suse, &[
                "patterns-cinnamon-cinnamon", "lightdm", "lightdm-gtk-greeter",
                "nemo",
            ]),
            (PkgFamily::Gentoo, &[
                "gnome-extra/cinnamon", "x11-misc/lightdm", "gnome-extra/nemo",
            ]),
            (PkgFamily::Alpine, &[
                "cinnamon", "nemo", "gnome-terminal", "lightdm",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Solus, &["cinnamon", "nemo", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Void, &["cinnamon-all", "lightdm", "lightdm-gtk3-greeter"]),
            (PkgFamily::Alt, &[
                "cinnamon", "nemo", "gnome-terminal", "lightdm",
                "lightdm-gtk-greeter",
            ]),
        ],
        // ⚠️ `lightdm-slick-greeter` отсюда убран: он лежит в extra и уже
        // стоит в Arch-списке выше. Помеченный AUR-ом пакет из официального
        // репозитория — не косметика: на машине БЕЗ yay/paru весь этот
        // список молча выбрасывается (`aur_packages=… if has_aur_helper`),
        // а карточка стола врёт покупателю «часть пакетов из AUR».
        //
        // ⛔ `mint-artwork` сюда НЕ возвращать: он объявляет
        // `conflicts=(lightdm-gtk-greeter lightdm-gtk-greeter-settings)` и сам
        // кладёт `/etc/lightdm/lightdm-gtk-greeter.conf`, а экран входа GTK
        // Rexon ставит с XFCE и MATE — на такой машине pacman отказал бы, а
        // без отказа снёс бы соседям вход в систему. Вид Mint дают темы,
        // значки и обои ниже; без него теряются только файлы /etc/skel и
        // курсор Bibata (AUR RPC, 19.10.2026).
        //
        // На CachyOS темы и значки Mint лежат в репозитории `cachyos`: что
        // из этого списка есть в подключённых репозиториях, установка берёт
        // оттуда по факту (`pacman -Si`), см. `system_plan::split_repo_available`.
        aur: &[
            "mint-themes", "mint-x-icons", "mint-y-icons", "mint-backgrounds",
            "lightdm-settings",
        ],
        check_package: "cinnamon",
        config_paths: &[
            "~/.config/cinnamon", "~/.cinnamon", "~/.cinnamon/configs",
            "~/.local/share/cinnamon", "~/.config/cinnamon-session",
            "~/.config/dconf/user",
        ],
    }),
    // xfce
    ("xfce", DeEntry {
        packages: &[
            (PkgFamily::Arch, &["xfce4", "xfce4-goodies", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Debian, &["xfce4", "xfce4-goodies", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Fedora, &["@xfce-desktop-environment", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Suse, &["patterns-xfce-xfce", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Gentoo, &[
                "xfce-base/xfce4-meta", "x11-terms/xfce4-terminal",
                "app-editors/mousepad", "x11-misc/lightdm",
            ]),
            (PkgFamily::Alpine, &[
                "xfce4", "xfce4-terminal", "xfce4-screensaver", "lightdm",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Void, &["xfce4", "lightdm", "lightdm-gtk3-greeter"]),
            (PkgFamily::Solus, &["xfce4-desktop", "lightdm"]),
            (PkgFamily::Clear, &["desktop-xfce4"]),
            (PkgFamily::Alt, &["xfce4", "xfce4-goodies", "lightdm", "lightdm-gtk-greeter"]),
        ],
        aur: &[],
        check_package: "xfce4-session",
        config_paths: &["~/.config/xfce4", "~/.config/Thunar"],
    }),
    // mate
    ("mate", DeEntry {
        packages: &[
            // `appmenu-gtk-module` — сторона GTK-программ для глобального
            // меню (`vala-panel-appmenu-mate` из списка AUR), лежит в extra.
            (PkgFamily::Arch, &[
                "mate", "mate-extra", "plank", "lightdm",
                "lightdm-gtk-greeter", "appmenu-gtk-module",
            ]),
            (PkgFamily::Debian, &[
                "mate-desktop-environment", "mate-desktop-environment-extras",
                "lightdm", "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Fedora, &["@mate-desktop-environment", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Suse, &["patterns-mate-mate", "lightdm", "lightdm-gtk-greeter"]),
            (PkgFamily::Gentoo, &["mate-base/mate", "x11-misc/lightdm"]),
            (PkgFamily::Alpine, &[
                "mate-desktop", "mate-session-manager", "mate-panel", "caja",
                "marco", "mate-terminal", "mate-utils", "lightdm",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Void, &["mate", "lightdm", "lightdm-gtk3-greeter"]),
            (PkgFamily::Alt, &[
                "mate", "mate-terminal", "plank", "lightdm",
                "lightdm-gtk-greeter",
            ]),
        ],
        aur: &[
            "mate-tweak", "brisk-menu", "mate-indicator-applet",
            "ayatana-indicator-application", "ayatana-indicator-session",
            "ayatana-indicator-datetime", "ayatana-indicator-sound",
            "ayatana-indicator-notifications",
            // ⛔ Не общий `vala-panel-appmenu`: он зависит от `-budgie` (а тот —
            // от всего budgie-desktop) и от `-valapanel` (vala-panel). MATE
            // нужны только своя часть и реестр меню (AUR RPC, 19.10.2026).
            "vala-panel-appmenu-mate", "vala-panel-appmenu-registrar",
        ],
        check_package: "mate-session-manager",
        config_paths: &[
            "~/.config/mate", "~/.config/caja", "~/.config/mate-session",
            "~/.config/dconf/user",
        ],
    }),
    // gnome
    ("gnome", DeEntry {
        packages: &[
            (PkgFamily::Arch, &["gnome", "gnome-tweaks", "gdm"]),
            (PkgFamily::Debian, &["gnome", "gnome-tweaks", "gdm3"]),
            (PkgFamily::Fedora, &["@gnome-desktop", "gnome-tweaks", "gdm"]),
            (PkgFamily::Suse, &["patterns-gnome-gnome", "gdm"]),
            (PkgFamily::Gentoo, &["gnome-base/gnome", "gnome-base/gdm"]),
            (PkgFamily::Alpine, &[
                "gnome-desktop", "gnome-shell", "gnome-control-center",
                "nautilus", "gdm",
            ]),
            (PkgFamily::Void, &["gnome", "gdm"]),
            (PkgFamily::Solus, &[
                "gnome-shell", "gnome-desktop-branding",
                "gnome-control-center", "gdm",
            ]),
            (PkgFamily::Clear, &["desktop-gnome-apps"]),
            (PkgFamily::Alt, &[
                "gnome-shell", "gnome-control-center", "gnome-tweaks",
                "nautilus", "gdm",
            ]),
        ],
        aur: &[],
        check_package: "gnome-shell",
        config_paths: &[
            "~/.config/gnome-session", "~/.config/dconf/user",
            "~/.local/share/gnome-shell/extensions",
            "~/.local/share/backgrounds",
        ],
    }),
    // kde
    ("kde", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "plasma-meta", "konsole", "dolphin", "kate", "gwenview", "ark",
                "spectacle", "plasma-login-manager",
            ]),
            (PkgFamily::Debian, &["kde-plasma-desktop", "konsole", "dolphin", "sddm"]),
            (PkgFamily::Fedora, &["@kde-desktop-environment", "sddm"]),
            (PkgFamily::Suse, &["patterns-kde-kde", "sddm"]),
            (PkgFamily::Gentoo, &["kde-plasma/plasma-meta", "x11-misc/sddm"]),
            (PkgFamily::Alpine, &["plasma-desktop-meta", "konsole", "dolphin", "sddm"]),
            (PkgFamily::Void, &["kde-plasma", "konsole", "dolphin", "sddm"]),
            (PkgFamily::Solus, &["plasma-desktop", "sddm"]),
            (PkgFamily::Clear, &["desktop-kde-apps"]),
            (PkgFamily::Alt, &["kde5", "plasma5-workspace", "konsole", "dolphin", "sddm"]),
        ],
        aur: &[],
        check_package: "plasma-desktop",
        config_paths: &[
            "~/.config/plasma-org.kde.plasma.desktop-appletsrc",
            "~/.config/plasmashellrc", "~/.config/kdeglobals",
            "~/.config/kwinrc", "~/.config/kglobalshortcutsrc",
            "~/.config/kcminputrc", "~/.config/plasmarc",
            "~/.config/plasmanotifyrc", "~/.local/share/plasma",
            "~/.local/share/color-schemes",
        ],
    }),
    // budgie
    ("budgie", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "budgie-desktop", "budgie-desktop-view", "budgie-extras",
                "network-manager-applet", "nautilus", "lightdm",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Debian, &[
                "budgie-desktop", "budgie-extras", "lightdm",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Fedora, &["@budgie-desktop-environment", "lightdm"]),
            (PkgFamily::Suse, &["patterns-budgie-budgie", "lightdm"]),
            (PkgFamily::Gentoo, &[
                "budgie-base/budgie-desktop",
                "budgie-base/budgie-desktop-view", "x11-misc/lightdm",
            ]),
            (PkgFamily::Void, &["budgie-desktop", "lightdm", "lightdm-gtk3-greeter"]),
            (PkgFamily::Solus, &["budgie-desktop", "lightdm"]),
            (PkgFamily::Alt, &[
                "budgie-desktop", "nautilus", "lightdm", "lightdm-gtk-greeter",
            ]),
        ],
        // `budgie-extras` лежит в extra — ставится pacman'ом вместе с
        // остальными, помощник AUR для него не нужен (и на машине без
        // помощника он больше не пропадает).
        aur: &[],
        check_package: "budgie-desktop",
        config_paths: &[
            "~/.config/budgie-desktop", "~/.config/budgie-extras",
            "~/.config/dconf/user",
        ],
    }),
    // pantheon — рабочий стол elementary OS (Gala на Mutter, Wingpanel,
    // Plank, Switchboard). Пакетов нет нигде, кроме Arch (группа `pantheon`
    // в extra, 02.10.2026) и NixOS (модуль `services.desktopManager.pantheon`):
    // в Debian/Ubuntu его нет вовсе (только сама elementary OS), в Fedora
    // отдельные части без сеанса — стола из них не собрать.
    ("pantheon", DeEntry {
        packages: &[
            // Группа тянет сеанс (`pantheon-session` → gala, wingpanel,
            // plank, gnome-session), настройки по умолчанию, тему,
            // значки и приложения elementary. ⚠️ `lightdm-gtk-greeter` —
            // защита от дурака: lightdm в Arch собран с приветствием
            // lightdm-gtk-greeter по умолчанию, а приветствие Pantheon из
            // группы без правки `greeter-session` в /etc/lightdm/lightdm.conf
            // не включается — без gtk-приветствия экран входа не поднимется.
            (PkgFamily::Arch, &["pantheon", "lightdm", "lightdm-gtk-greeter"]),
        ],
        aur: &[],
        check_package: "pantheon-session",
        config_paths: &[
            "~/.config/dconf/user", "~/.config/plank", "~/.local/share/plank",
        ],
    }),
    // deepin — рабочий стол Deepin (DDE v25: deepin-session, dde-shell) ТОЛЬКО
    // на Treeland, Wayland-композиторе Deepin (решение Wehrwolfmann
    // 26.10.2026): сеанс X11 с deepin-kwin Rexon не ставит никогда. Набор ниже
    // — сырьё: исполнитель ставит из группы всё, кроме deepin-session (его
    // заменяет своя сборка без deepin-kwin) — `crate::deepin_treeland`, и
    // помощнику от root эта запись не отдаётся. Проверено только Arch
    // (группа `deepin` в extra, 02.10.2026). ⚠️ openSUSE в 2025 исключил
    // Deepin из своих репозиториев: служебный пакет включал D-Bus-службы и
    // правила polkit в обход проверки безопасности дистрибутива. Поэтому —
    // ТОЛЬКО штатная группа дистрибутива и никаких своих служб, правил и
    // «включателей»; где пакетов нет, стол не предлагается вовсе.
    ("deepin", DeEntry {
        packages: &[
            // Группа: сеанс, настройки, тему и приложения (её
            // `deepin-session` тянет deepin-kwin — потому группа целиком и не
            // ставится, см. выше). Экран входа — lightdm с
            // приветствием Deepin: его ставит deepin-session-shell, а
            // `/usr/share/lightdm/lightdm.conf.d/60-deepin.conf` из
            // deepin-daemon сам задаёт `greeter-session=lightdm-deepin-greeter`.
            // lightdm назван явно, чтобы защита «нет включённого экрана входа»
            // знала, какой включать (ddm из группы — ранний и без X11).
            (PkgFamily::Arch, &["deepin", "lightdm"]),
        ],
        aur: &[],
        check_package: "deepin-session",
        config_paths: &[
            "~/.config/deepin", "~/.config/dsg", "~/.config/dconf/user",
        ],
    }),
    // cosmic
    ("cosmic", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "cosmic-session", "cosmic-comp", "cosmic-panel",
                "cosmic-applets", "cosmic-app-library", "cosmic-bg",
                "cosmic-greeter", "cosmic-idle", "cosmic-launcher",
                "cosmic-notifications", "cosmic-osd", "cosmic-randr",
                "cosmic-screenshot", "cosmic-settings",
                "cosmic-settings-daemon", "cosmic-workspaces",
                "cosmic-initial-setup", "xdg-desktop-portal-cosmic",
                "cosmic-files", "cosmic-terminal", "cosmic-text-editor",
                "cosmic-store", "cosmic-player", "cosmic-wallpapers",
                "cosmic-icon-theme", "gnome-keyring", "power-profiles-daemon",
                "packagekit", "flatpak",
            ]),
            (PkgFamily::Fedora, &["cosmic-desktop"]),
            (PkgFamily::Suse, &["patterns-cosmic-cosmic"]),
            (PkgFamily::Gentoo, &["cosmic-base/cosmic-session"]),
            (PkgFamily::Alpine, &[
                "cosmic-session", "cosmic-comp", "cosmic-panel",
                "cosmic-applets", "cosmic-app-library", "cosmic-bg",
                "cosmic-greeter", "cosmic-idle", "cosmic-launcher",
                "cosmic-notifications", "cosmic-osd", "cosmic-randr",
                "cosmic-screenshot", "cosmic-settings",
                "cosmic-settings-daemon", "cosmic-workspaces",
                "xdg-desktop-portal-cosmic", "cosmic-files", "cosmic-terminal",
                "cosmic-text-editor", "cosmic-store", "cosmic-wallpapers",
                "cosmic-icons",
            ]),
            (PkgFamily::Void, &["cosmic-desktop"]),
        ],
        aur: &[],
        check_package: "cosmic-session",
        config_paths: &["~/.config/cosmic", "~/.local/state/cosmic"],
    }),
    // hyprland
    // SDDM убран: экран входа общесистемный, его гарантирует install_de →
    // ensure_display_manager (Python-путь), а не per-WM пакет-лист.
    ("hyprland", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "hyprland", "hyprpaper", "hyprlock", "hypridle",
                "xdg-desktop-portal-hyprland", "xdg-desktop-portal-gtk",
                "waybar", "wofi", "kitty",
                "thunar", "grim", "slurp", "wl-clipboard", "polkit-gnome",
                "otf-font-awesome",
            ]),
            (PkgFamily::Debian, &[
                "hyprland", "xdg-desktop-portal-hyprland", "waybar", "wofi",
                "kitty", "grim", "slurp", "wl-clipboard",
            ]),
            (PkgFamily::Fedora, &[
                "hyprland", "xdg-desktop-portal-hyprland", "waybar", "wofi",
                "kitty", "grim", "slurp", "wl-clipboard",
            ]),
            (PkgFamily::Suse, &[
                "hyprland", "xdg-desktop-portal-hyprland", "waybar", "wofi",
                "kitty", "grim", "slurp", "wl-clipboard",
            ]),
            (PkgFamily::Gentoo, &[
                "gui-wm/hyprland", "gui-apps/waybar", "gui-apps/wofi",
                "x11-terms/kitty", "gui-apps/grim", "gui-apps/slurp",
            ]),
            (PkgFamily::Void, &[
                "hyprland", "waybar", "wofi", "kitty", "grim", "slurp",
                "wl-clipboard",
            ]),
            (PkgFamily::Alpine, &[
                "hyprland", "waybar", "wofi", "kitty", "grim", "slurp",
                "wl-clipboard",
            ]),
            (PkgFamily::Alt, &[
                "hyprland", "waybar", "wofi", "kitty", "grim", "slurp",
                "wl-clipboard",
            ]),
        ],
        // Все три лежат в extra и уже перечислены в Arch-списке выше —
        // дублировать их помощником AUR значило собирать из исходников уже
        // готовое, а на машине без помощника терять их вовсе.
        aur: &[],
        check_package: "hyprland",
        config_paths: &[
            "~/.config/hypr", "~/.config/waybar", "~/.config/wofi",
            "~/.config/kitty",
        ],
    }),
    // lxqt
    ("lxqt", DeEntry {
        packages: &[
            // `openbox` назван ЯВНО, а не взят «как-нибудь из группы»:
            // post_install у lxqt-session печатает «An X11 window manager is
            // also needed» безусловным cat'ом, и полагаться на то, что WM
            // окажется в системе сам, для чистой машины покупателя нельзя.
            // `breeze-icons` — тема значков (она же жёсткая зависимость
            // lxqt-session, но здесь стоит намеренно: список обязан отвечать
            // за обе просьбы сам).
            (PkgFamily::Arch, &["lxqt", "openbox", "breeze-icons", "sddm"]),
            (PkgFamily::Debian, &["lxqt", "sddm"]),
            (PkgFamily::Fedora, &["@lxqt-desktop-environment", "sddm"]),
            (PkgFamily::Suse, &["patterns-lxqt-lxqt", "sddm"]),
            (PkgFamily::Gentoo, &["lxqt-base/lxqt-meta", "x11-misc/sddm"]),
            (PkgFamily::Alpine, &[
                "lxqt-desktop", "lximage-qt", "pavucontrol-qt", "font-dejavu",
                "sddm",
            ]),
            (PkgFamily::Void, &["lxqt", "sddm"]),
            (PkgFamily::Alt, &[
                "lxqt-session", "lxqt-panel", "lxqt-config", "lxqt-policykit",
                "lxqt-runner", "lxqt-notificationd", "pcmanfm-qt", "qterminal",
                "openbox", "sddm",
            ]),
        ],
        aur: &[],
        check_package: "lxqt-session",
        config_paths: &[
            "~/.config/lxqt", "~/.config/pcmanfm-qt",
            "~/.config/qterminal.org", "~/.config/openbox",
        ],
    }),
    // niri
    // Шрифты-иконки для waybar (иначе «тофу» вместо глифов): Font Awesome —
    // дефолтный конфиг waybar; Nerd-symbols (symbols-only) — из офиц. репо там,
    // где есть (Arch/openSUSE/Gentoo/Void/Alpine; в Debian/Fedora нет). SDDM убран:
    // экран входа общесистемный, его гарантирует install_de →
    // ensure_display_manager (Python-путь), а не per-WM пакет-лист.
    ("niri", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "niri", "swaybg", "swayidle", "swaylock", "waybar", "fuzzel",
                "alacritty", "mako", "thunar", "grim", "slurp", "wl-clipboard",
                "polkit-gnome", "xdg-desktop-portal-gnome",
                "xwayland-satellite",
                "otf-font-awesome", "ttf-nerd-fonts-symbols",
            ]),
            (PkgFamily::Fedora, &[
                "niri", "swaybg", "swayidle", "swaylock", "waybar", "fuzzel",
                "alacritty", "mako", "thunar", "grim", "slurp", "wl-clipboard",
                "xdg-desktop-portal-gnome", "fontawesome-fonts",
            ]),
            (PkgFamily::Suse, &[
                "niri", "swaybg", "swayidle", "swaylock", "waybar", "fuzzel",
                "alacritty", "mako", "grim", "slurp", "wl-clipboard",
                "xdg-desktop-portal-gnome", "fontawesome-fonts",
                "symbols-only-nerd-fonts",
            ]),
            (PkgFamily::Alpine, &[
                "niri", "swaybg", "swayidle", "swaylock", "waybar", "fuzzel",
                "alacritty", "mako", "grim", "slurp", "wl-clipboard",
                "xdg-desktop-portal-gnome", "font-awesome",
                "font-nerd-fonts-symbols",
            ]),
        ],
        aur: &[],
        check_package: "niri",
        config_paths: &["~/.config/niri", "~/.config/waybar", "~/.config/fuzzel"],
    }),
    // sway
    // Шрифты-иконки для waybar (иначе «тофу» вместо глифов): Font Awesome —
    // дефолтный конфиг waybar; Nerd-symbols (symbols-only) — из офиц. репо там,
    // где есть (Arch/openSUSE/Gentoo/Void/Alpine; в Debian/Fedora нет). SDDM убран:
    // экран входа общесистемный, его гарантирует install_de →
    // ensure_display_manager (Python-путь), а не per-WM пакет-лист.
    ("sway", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "wmenu", "foot", "mako", "thunar", "grim", "slurp",
                "wl-clipboard", "polkit-gnome", "xdg-desktop-portal-wlr",
                "xdg-desktop-portal-gtk", "xorg-xwayland",
                "otf-font-awesome", "ttf-nerd-fonts-symbols",
            ]),
            // Уведомления в Debian/Ubuntu — `mako-notifier`: пакет с именем
            // `mako` там — исходник шаблонизатора на Python, и `apt-get
            // install mako` ронял ВСЮ установку sway.
            (PkgFamily::Debian, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "foot", "mako-notifier", "grim", "slurp", "wl-clipboard",
                "xdg-desktop-portal-wlr", "fonts-font-awesome",
            ]),
            (PkgFamily::Fedora, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "foot", "mako", "grim", "slurp", "wl-clipboard",
                "xdg-desktop-portal-wlr", "fontawesome-fonts",
            ]),
            (PkgFamily::Suse, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "foot", "mako", "grim", "slurp", "wl-clipboard",
                "xdg-desktop-portal-wlr", "fontawesome-fonts",
                "symbols-only-nerd-fonts",
            ]),
            (PkgFamily::Gentoo, &[
                "gui-wm/sway", "gui-apps/swaybg", "gui-apps/waybar",
                "gui-apps/wofi", "gui-apps/foot", "gui-apps/grim",
                "gui-apps/slurp", "media-fonts/fontawesome",
                "media-fonts/symbols-nerd-font",
            ]),
            (PkgFamily::Void, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "foot", "mako", "grim", "slurp", "wl-clipboard", "font-awesome",
                "nerd-fonts-symbols-ttf",
            ]),
            (PkgFamily::Alpine, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "foot", "mako", "grim", "slurp", "wl-clipboard", "font-awesome",
                "font-nerd-fonts-symbols",
            ]),
            (PkgFamily::Alt, &[
                "sway", "swaybg", "swayidle", "swaylock", "waybar", "wofi",
                "foot", "mako", "grim", "slurp", "wl-clipboard",
                "fonts-font-awesome",
            ]),
        ],
        aur: &[],
        check_package: "sway",
        config_paths: &[
            "~/.config/sway", "~/.config/waybar", "~/.config/wofi",
            "~/.config/foot",
        ],
    }),
    // river
    // ⚠️ Ставится river-classic (ветка 0.3: свой тайлинг `rivertile` и
    // `riverctl`), а не river 0.4+. Новый river — «немонолитный»: окнами в
    // нём управляет ОТДЕЛЬНАЯ программа по протоколу
    // river-window-management-v1, и без неё river сам по себе бесполезен
    // (river(1): «river is not useful without a compatible window manager»).
    // Ни одного такого менеджера в официальных репозиториях нет, а голый
    // river покупатель увидел бы чёрным экраном без единой клавиши (правило
    // #13). Имена пакета по семействам разные: Arch, Fedora (с 43) и Alpine
    // (с 3.23) зовут ветку 0.3 `river-classic`, у Void она пока `river`.
    // Debian, openSUSE, Gentoo и ALT river в официальных репозиториях не
    // держат — там стол честно «недоступен на этом дистрибутиве».
    // Без `~/.config/river/init` river-classic тоже стартует без клавиш —
    // стартовый init кладёт пост-установка (`plan::river_starter_init`).
    // Панель — waybar (её запускает стартовый init), запуск программ — fuzzel,
    // терминал — foot (его зовёт и пример init из самого river-classic).
    ("river", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "river-classic", "waybar", "fuzzel", "foot", "mako", "swaybg",
                "thunar", "grim", "slurp", "wl-clipboard", "polkit-gnome",
                "xdg-desktop-portal-wlr", "xdg-desktop-portal-gtk",
                "otf-font-awesome", "ttf-nerd-fonts-symbols",
            ]),
            (PkgFamily::Fedora, &[
                "river-classic", "waybar", "fuzzel", "foot", "mako", "swaybg",
                "grim", "slurp", "wl-clipboard", "xdg-desktop-portal-wlr",
                "fontawesome-fonts",
            ]),
            (PkgFamily::Void, &[
                "river", "waybar", "fuzzel", "foot", "mako", "swaybg", "grim",
                "slurp", "wl-clipboard", "xdg-desktop-portal-wlr",
                "font-awesome", "nerd-fonts-symbols-ttf",
            ]),
            (PkgFamily::Alpine, &[
                "river-classic", "waybar", "fuzzel", "foot", "mako", "swaybg",
                "grim", "slurp", "wl-clipboard", "xdg-desktop-portal-wlr",
                "font-awesome", "font-nerd-fonts-symbols",
            ]),
        ],
        aur: &[],
        check_package: "river-classic",
        config_paths: &["~/.config/river", "~/.config/waybar", "~/.config/fuzzel"],
    }),
    // wayfire
    // wf-shell (панель wf-panel с меню программ и фон wf-background) входит
    // в комплект обязательно: модуль autostart Wayfire сам поднимает их при
    // входе (`autostart_wf_shell`, по умолчанию true), а без них сеанс —
    // серый экран без меню и без единой клавиши запуска программ (в
    // умолчаниях модуля command привязок нет), правило #13. wcm — окно
    // настроек Wayfire, чтобы не править wayfire.ini руками. Alpine держит
    // Wayfire только в testing, ALT — только в Sisyphus: там стол
    // «недоступен».
    ("wayfire", DeEntry {
        packages: &[
            (PkgFamily::Arch, &[
                "wayfire", "wf-shell", "wcm", "foot", "mako", "thunar", "grim",
                "slurp", "wl-clipboard", "polkit-gnome",
                "xdg-desktop-portal-wlr", "xdg-desktop-portal-gtk",
                "xorg-xwayland",
            ]),
            (PkgFamily::Debian, &[
                "wayfire", "wf-shell", "wcm", "foot", "mako-notifier", "grim",
                "slurp", "wl-clipboard", "xdg-desktop-portal-wlr",
            ]),
            (PkgFamily::Fedora, &[
                "wayfire", "wf-shell", "wayfire-config-manager", "foot", "mako",
                "grim", "slurp", "wl-clipboard", "xdg-desktop-portal-wlr",
            ]),
            (PkgFamily::Suse, &[
                "wayfire", "wf-shell", "wcm", "foot", "mako", "grim", "slurp",
                "wl-clipboard", "xdg-desktop-portal-wlr",
            ]),
            (PkgFamily::Gentoo, &[
                "gui-wm/wayfire", "gui-apps/wf-shell", "gui-apps/wcm",
                "gui-apps/foot", "gui-apps/grim", "gui-apps/slurp",
            ]),
            (PkgFamily::Void, &[
                "wayfire", "wf-shell", "wcm", "foot", "mako", "grim", "slurp",
                "wl-clipboard", "xdg-desktop-portal-wlr",
            ]),
        ],
        aur: &[],
        check_package: "wayfire",
        config_paths: &[
            "~/.config/wayfire.ini", "~/.config/wayfire", "~/.config/wf-shell.ini",
        ],
    }),
];

/// `_DE_META`: мета-информация DE для UI, в исходном порядке.
pub const DE_META: &[DeMeta] = &[
    DeMeta {
        id: "cinnamon-mint",
        display_name: "Cinnamon",
        description: n_("🍃 The Cinnamon desktop with the full set of Linux Mint components"),
        session_name: "cinnamon",
        ram_mb: 650,
        cpu_usage: CpuLoad::Medium,
        customizable: 5,
        gaming_friendly: 4,
        toolkit: "GTK3",
        session_type: "X11",
        compositor: "Muffin",
        panel_type: "Cinnamon Panel",
        file_manager: "Nemo",
        terminal: "gnome-terminal",
    },
    DeMeta {
        id: "xfce",
        display_name: "XFCE",
        description: n_("A light and fast desktop that uses few resources"),
        session_name: "xfce",
        ram_mb: 300,
        cpu_usage: CpuLoad::Low,
        customizable: 4,
        gaming_friendly: 5,
        toolkit: "GTK3",
        session_type: "X11",
        compositor: "Xfwm4",
        panel_type: "Xfce Panel",
        file_manager: "Thunar",
        terminal: "xfce4-terminal",
    },
    DeMeta {
        id: "mate",
        display_name: "MATE",
        description: n_("A traditional desktop that carries on GNOME 2"),
        session_name: "mate",
        ram_mb: 400,
        cpu_usage: CpuLoad::Low,
        customizable: 3,
        gaming_friendly: 4,
        toolkit: "GTK3",
        session_type: "X11",
        compositor: "Marco",
        panel_type: "MATE Panel",
        file_manager: "Caja",
        terminal: "mate-terminal",
    },
    DeMeta {
        id: "gnome",
        display_name: "GNOME",
        description: n_("A modern desktop with a minimalist design"),
        session_name: "gnome",
        ram_mb: 800,
        cpu_usage: CpuLoad::High,
        customizable: 3,
        gaming_friendly: 3,
        toolkit: "GTK4",
        session_type: "Wayland + X11",
        compositor: "Mutter",
        panel_type: "GNOME Shell",
        file_manager: "Nautilus",
        terminal: "GNOME Console",
    },
    DeMeta {
        id: "kde",
        display_name: "KDE Plasma",
        description: n_("A powerful, customizable desktop with beautiful effects"),
        session_name: "plasma",
        ram_mb: 700,
        cpu_usage: CpuLoad::Medium,
        customizable: 5,
        gaming_friendly: 5,
        toolkit: "Qt6",
        session_type: "Wayland + X11",
        compositor: "KWin",
        panel_type: "Plasma Panel",
        file_manager: "Dolphin",
        terminal: "Konsole",
    },
    DeMeta {
        id: "budgie",
        display_name: "Budgie",
        description: n_("An elegant desktop from Buddies of Budgie"),
        session_name: "budgie-desktop",
        ram_mb: 550,
        cpu_usage: CpuLoad::Medium,
        customizable: 3,
        gaming_friendly: 3,
        toolkit: "GTK3 → GTK4",
        session_type: "X11 (Wayland WIP)",
        compositor: "Budgie WM / Magpie",
        panel_type: "Budgie Panel",
        file_manager: "Nautilus",
        terminal: "GNOME Terminal",
    },
    DeMeta {
        id: "pantheon",
        display_name: "Pantheon",
        description: n_("The calm, macOS-like desktop of elementary OS"),
        session_name: "pantheon",
        ram_mb: 600,
        cpu_usage: CpuLoad::Medium,
        customizable: 2,
        gaming_friendly: 3,
        toolkit: "GTK3 + GTK4 (Granite)",
        session_type: "X11 + Wayland",
        compositor: "Gala (Mutter)",
        panel_type: "Wingpanel + Plank",
        file_manager: "Pantheon Files",
        terminal: "Pantheon Terminal",
    },
    DeMeta {
        id: "deepin",
        display_name: "Deepin",
        description: n_("The polished desktop of deepin with rounded, translucent windows, on its own Wayland compositor Treeland (experimental)"),
        session_name: "treeland",
        ram_mb: 900,
        cpu_usage: CpuLoad::High,
        customizable: 3,
        gaming_friendly: 3,
        toolkit: "Qt6 (DTK)",
        session_type: "Wayland",
        compositor: "Treeland",
        panel_type: "dde-shell",
        file_manager: "Deepin File Manager",
        terminal: "Deepin Terminal",
    },
    DeMeta {
        id: "cosmic",
        display_name: "COSMIC",
        description: n_("A new Wayland-native desktop from System76, written in Rust"),
        session_name: "cosmic",
        ram_mb: 500,
        cpu_usage: CpuLoad::Medium,
        customizable: 4,
        gaming_friendly: 4,
        toolkit: "iced (Rust)",
        session_type: "Wayland",
        compositor: "cosmic-comp",
        panel_type: "COSMIC Panel",
        file_manager: "COSMIC Files",
        terminal: "COSMIC Term",
    },
    DeMeta {
        id: "lxqt",
        display_name: "LXQt",
        description: n_("🪶 A lightweight Qt desktop, the successor to LXDE — the Qt counterpart of XFCE"),
        session_name: "lxqt",
        ram_mb: 350,
        cpu_usage: CpuLoad::Low,
        customizable: 4,
        gaming_friendly: 4,
        toolkit: "Qt6",
        session_type: "X11 (Wayland WIP)",
        compositor: n_("none (Openbox WM; labwc optional)"),
        panel_type: "LXQt Panel",
        file_manager: "PCManFM-Qt",
        terminal: "QTerminal",
    },
    // Wayfire — не тайловый: окна плавающие, как в Compiz. Но и не полный
    // стол: своих программ и общих настроек системы нет, только панель
    // wf-shell и настройки самого композитора (WCM), — поэтому в списке у
    // него своя секция «Композиторы с эффектами» (`qt_desktops::EFFECTS_IDS`).
    DeMeta {
        id: "wayfire",
        display_name: "Wayfire",
        description: n_("🔥 A Wayland window manager with Compiz-style effects (desktop cube, wobbly windows) and a simple wf-shell panel. It isn't a full desktop: it has no programs or system settings of its own — only settings for Wayfire itself."),
        session_name: "wayfire",
        ram_mb: 250,
        cpu_usage: CpuLoad::Low,
        customizable: 4,
        gaming_friendly: 3,
        toolkit: "GTK4 (wf-shell)",
        session_type: "Wayland",
        compositor: "Wayfire (wlroots)",
        panel_type: "wf-panel",
        file_manager: "Thunar",
        terminal: "foot",
    },
    // Тайловые оконные менеджеры — после полных DE.
    DeMeta {
        id: "hyprland",
        display_name: "Hyprland",
        description: n_("🪟 A tiling Wayland compositor with animations and high performance"),
        session_name: "hyprland",
        ram_mb: 250,
        cpu_usage: CpuLoad::Low,
        customizable: 5,
        gaming_friendly: 5,
        toolkit: "—",
        session_type: "Wayland",
        compositor: "Hyprland",
        panel_type: "Waybar / Eww",
        file_manager: "Thunar / Dolphin",
        terminal: "kitty / foot",
    },
    DeMeta {
        id: "sway",
        display_name: "Sway",
        description: n_("🧩 A tiling Wayland compositor — a drop-in replacement for i3 that reads its config"),
        session_name: "sway",
        ram_mb: 150,
        cpu_usage: CpuLoad::Low,
        customizable: 5,
        gaming_friendly: 4,
        toolkit: "—",
        session_type: "Wayland",
        compositor: "Sway (wlroots)",
        panel_type: "Waybar",
        file_manager: "Thunar",
        terminal: "foot",
    },
    DeMeta {
        id: "niri",
        display_name: "niri",
        description: n_("🎞️ A scrollable tiling Wayland compositor — windows sit on an endless horizontal strip"),
        session_name: "niri",
        ram_mb: 200,
        cpu_usage: CpuLoad::Low,
        customizable: 4,
        gaming_friendly: 4,
        toolkit: "—",
        session_type: "Wayland",
        compositor: "niri (Smithay)",
        panel_type: "Waybar",
        file_manager: "Thunar",
        terminal: "Alacritty / foot",
    },
    DeMeta {
        id: "river",
        display_name: "River",
        description: n_("🏞️ A dynamic tiling Wayland compositor: windows are grouped by tags instead of workspaces"),
        session_name: "river",
        ram_mb: 120,
        cpu_usage: CpuLoad::Low,
        customizable: 4,
        gaming_friendly: 4,
        toolkit: "—",
        session_type: "Wayland",
        compositor: "River (wlroots)",
        panel_type: "Waybar",
        file_manager: "Thunar",
        terminal: "foot",
    },
];

fn de_entry(de_id: &str) -> Option<&'static DeEntry> {
    DE_DATABASE.iter().find(|(k, _)| *k == de_id).map(|(_, e)| e)
}

/// Список основных пакетов для DE и дистрибутива — порт
/// `PackageDB.get_packages`. Возвращает [] если DE недоступен на этом
/// дистрибутиве (fallback на Debian НЕ делается — имена несовместимы).
pub fn get_packages(de_id: &str, family: PkgFamily) -> Vec<String> {
    let Some(entry) = de_entry(de_id) else {
        return Vec::new();
    };
    entry
        .packages
        .iter()
        .find(|(f, _)| *f == family)
        .map(|(_, pkgs)| pkgs.iter().map(|p| p.to_string()).collect())
        .unwrap_or_default()
}

/// Доступен ли DE на данном дистрибутиве — порт `PackageDB.is_available`.
pub fn is_available(de_id: &str, family: PkgFamily) -> bool {
    de_entry(de_id)
        .map(|e| e.packages.iter().any(|(f, _)| *f == family))
        .unwrap_or(false)
}

/// AUR-пакеты (возвращает [] для не-Arch) — порт `PackageDB.get_aur_packages`.
pub fn get_aur_packages(de_id: &str, family: PkgFamily) -> Vec<String> {
    if family != PkgFamily::Arch {
        return Vec::new();
    }
    de_entry(de_id)
        .map(|e| e.aur.iter().map(|p| p.to_string()).collect())
        .unwrap_or_default()
}

/// Имя ПАКЕТА, по которому спрашивают пакетный менеджер, стоит ли DE — порт
/// `PackageDB.get_check_binary` (None для неизвестного DE).
///
/// ⚠️ Имя функции обманывает и остаётся таким только ради паритета с Python:
/// возвращается `check_package`, то есть имя ПАКЕТА (`plasma-desktop`,
/// `mate-session-manager`), а не исполняемого файла. Исполняемых файлов с
/// такими именами не существует — `which("plasma-desktop")` не найдёт ничего.
/// Карта «DE → исполняемый файл» (`plasmashell`, `mate-session`, `Hyprland`)
/// живёт рядом — `de_installed_binaries`.
pub fn get_check_binary(de_id: &str) -> Option<&'static str> {
    de_entry(de_id).map(|e| e.check_package)
}

/// Исполняемые файлы, по которым видно, что рабочий стол РЕАЛЬНО установлен.
///
/// `check_package` из базы — имя ПАКЕТА (`plasma-desktop`,
/// `mate-session-manager`); искать такой файл в PATH бессмысленно, его там
/// нет. Поэтому у каждого стола свой исполняемый файл, а у Cinnamon их два:
/// `cinnamon-session` тянется зависимостями и без самого стола, поэтому
/// подтверждаем ещё и файловым менеджером `nemo`.
///
/// Забыть новый стол здесь нельзя: неизвестный `de_id` отдаёт пустой
/// список, а не «имя пакета вместо файла», — установленный стол больше не
/// показывается вечно отсутствующим (так вело себя прежнее
/// `_CHECK_BINARIES.get(name) or check_package`).
pub fn de_installed_binaries(de_id: &str) -> &'static [&'static str] {
    match de_id {
        "cinnamon-mint" => &["cinnamon", "nemo"],
        "xfce" => &["xfce4-session"],
        "mate" => &["mate-session"],
        "gnome" => &["gnome-shell"],
        "kde" => &["plasmashell"],
        "budgie" => &["budgie-desktop"],
        "pantheon" => &["gala", "io.elementary.wingpanel"],
        "deepin" => &["dde-session"],
        "cosmic" => &["cosmic-session"],
        "hyprland" => &["Hyprland"],
        "lxqt" => &["lxqt-session"],
        "niri" => &["niri"],
        "sway" => &["sway"],
        // `rivertile` есть только у river-classic: голый river 0.4+ (тоже
        // `/usr/bin/river`, но без менеджера окон) не должен выглядеть
        // установленным столом — войти в него покупатель не сможет.
        "river" => &["river", "rivertile"],
        "wayfire" => &["wayfire"],
        _ => &[],
    }
}

/// Пути конфигурации DE — порт `PackageDB.get_config_paths`.
pub fn get_config_paths(de_id: &str) -> Vec<String> {
    de_entry(de_id)
        .map(|e| e.config_paths.iter().map(|p| p.to_string()).collect())
        .unwrap_or_default()
}

/// Список всех известных DE — порт `PackageDB.list_de_ids`.
pub fn list_de_ids() -> Vec<&'static str> {
    DE_DATABASE.iter().map(|(k, _)| *k).collect()
}

// ============================================================================
// Какое окружение запущено
// ============================================================================
//
// ⚠️ ОДНА реализация на весь продукт. Их было ТРИ, и все три отличались:
//   * `compliance` знал `plasma`, но не знал sway, niri и LXQt — тайловым
//     проверки соответствия молча не доставались (тест прямо фиксировал
//     `detect_current_de("sway") == None`);
//   * `appdefaults` знал sway и niri, но сравнивал токены с учётом регистра
//     и не знал `plasma`;
//   * `SystemInfo.detect_current_de` в Python искал подстроки и тоже не знал
//     ни sway, ни niri — а от его ответа зависит запрет удалять ЗАПУЩЕННЫЙ
//     рабочий стол (правило #13): в сеансе sway ответ был «unknown», и
//     защита не срабатывала.
// Разъезд трёх таблиц — ровно то, что запрещает правило #12, поэтому обе
// прежние функции теперь зовут эту. 30.09 к ней сведён и
// `wallpaper_engine::detect_de` (четвёртая таблица: подстроки, Cinnamon как
// `cinnamon`, `plasma` в имени сеанса не узнавался). Отдельно живёт только
// `we_util::detect_desktop` — порт `Util::desktop` демона waywallen, у него
// свой перечень ответов.

/// Токен `XDG_CURRENT_DESKTOP` (в нижнем регистре) → идентификатор DE Rexon.
/// Порядок важен: `Budgie:GNOME` обязан дать budgie, поэтому перебираются
/// сначала токены строки, и только внутри — эта таблица. Ей же помощник
/// пакетов от root узнаёт стол графического сеанса по `Desktop=` logind
/// (`system_plan::pkg_helper_script`).
pub const XDG_TOKEN_TO_DE: &[(&str, &str)] = &[
    ("kde", "kde"),
    ("plasma", "kde"),
    ("gnome", "gnome"),
    ("x-cinnamon", "cinnamon-mint"),
    ("cinnamon", "cinnamon-mint"),
    ("xfce", "xfce"),
    ("mate", "mate"),
    ("budgie", "budgie"),
    // Сеанс Pantheon: `DesktopNames=Pantheon` в обоих session-файлах
    // (elementary/session-settings: xsessions/pantheon.desktop.in и
    // wayland-sessions/pantheon-wayland.desktop.in).
    ("pantheon", "pantheon"),
    // Deepin: DDE v23/v25 ставит `XDG_CURRENT_DESKTOP=DDE` сам
    // (dde-session `environmentsmanager.cpp`), deepin 20 — `Deepin`
    // (startdde `session.go`); сеанс Wayland объявляет `DesktopNames=Treeland`.
    // Имена сеансов — `deepin` (X11) и `treeland` (Wayland).
    ("dde", "deepin"),
    ("deepin", "deepin"),
    ("treeland", "deepin"),
    ("lxqt", "lxqt"),
    ("cosmic", "cosmic"),
    ("hyprland", "hyprland"),
    ("sway", "sway"),
    ("niri", "niri"),
    // river и Wayfire Rexon с 24.10 ставит сам. river-classic
    // `XDG_CURRENT_DESKTOP` не выставляет (его выставляет стартовый init
    // Rexon, а сеанс из экрана входа опознаётся по `DESKTOP_SESSION=river`);
    // Wayfire ставит `wayfire` сам.
    ("river", "river"),
    ("wayfire", "wayfire"),
    // labwc Rexon не ставит, но узнаёт: фон на нём кладёт внешний демон
    // (`wallpaper_engine::WLROOTS_DE`). Раньше такие композиторы знала только
    // отдельная таблица движка обоев, а здесь они проходили лишь на Wayland
    // «первым токеном как есть» — и по имени сеанса не опознавались вовсе.
    ("labwc", "labwc"),
];

/// Какое окружение запущено СЕЙЧАС: по `XDG_CURRENT_DESKTOP`, при неудаче —
/// по `DESKTOP_SESSION`, при неудаче — первый токен как есть, если сеанс
/// вообще Wayland (неизвестный композитор лучше «ничего»: обои и шрифты ему
/// всё равно ставятся).
///
/// Значения окружения передаются параметрами: функция чистая и проверяется
/// без подмены `os.environ`.
pub fn detect_current_de(
    xdg_current_desktop: &str,
    desktop_session: &str,
    has_wayland: bool,
) -> Option<String> {
    let xdg = xdg_current_desktop.to_lowercase();
    for part in xdg.split(':') {
        let part = part.trim();
        if let Some((_, de)) = XDG_TOKEN_TO_DE.iter().find(|(k, _)| *k == part) {
            return Some((*de).to_string());
        }
    }
    // Запасной путь — имя сеанса. Ищем подстроку: имена сеансов пишут как
    // попало (`gnome-xorg`, `plasmawayland`, `cinnamon2d`).
    let session = desktop_session.to_lowercase();
    if !session.is_empty() {
        if let Some((_, de)) = XDG_TOKEN_TO_DE.iter().find(|(k, _)| session.contains(*k)) {
            return Some((*de).to_string());
        }
    }
    if has_wayland {
        let first = xdg.split(':').next().unwrap_or("").trim();
        if !first.is_empty() {
            return Some(first.to_string());
        }
    }
    None
}

// ============================================================================
// Связка «композитор + шелл»: кто ещё живёт в этом сеансе
// ============================================================================
//
// ⚠️ Модель «одно окружение = одна строка» неполна. Под niri можно сидеть
// голым, а можно — с чужим шеллом (Noctalia, DankMaterialShell), и это две
// РАЗНЫЕ машины: во втором случае фон рисует не наш движок, а шелл — своей
// полноэкранной поверхностью wlr-layer-shell на слое Background, на каждый
// выход (у Noctalia она с namespace `noctalia-wallpaper`, у DMS namespace не
// задан вовсе). Наша поверхность обоев ляжет под неё, и покупатель увидит
// чужие обои при зелёном рапорте. `detect_current_de` в обоих случаях вернёт
// «niri», поэтому связку опознаём отдельным ответом.
//
// Ядро НИЧЕГО не спрашивает у машины: наблюдения приходят параметрами
// ([`ShellFacts`]) ровно как у `detect_current_de`, добывает их вызывающий.
// Иначе связку нельзя было бы прогнать сверкой.
//
// ⚠️ Граница ответственности. Файл в каталоге сеанса доказывает, что шелл
// ТУТ БЫЛ: свой сокет Noctalia снимает только в деструкторе, после падения
// файл остаётся. Поэтому «живость» — работа вызывающего (flock на
// `noctalia-*.lock`, ответ `noctalia msg status`, `systemctl --user
// is-active`), а ядро принимает уже добытый факт. Имя на шине этим пороком
// не страдает: протухшим оно быть не может, и это самый надёжный признак.

/// Чужой шелл рабочего стола, который сам рисует фон.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopShell {
    /// Noctalia (нативный C++/GLES, MIT). Живёт не только на wlroots, но и
    /// на KDE Plasma, поэтому опознавать его по композитору нельзя.
    ///
    /// ⚠️ Plasma у Noctalia — НЕ полноценная интеграция: их собственная
    /// документация (`docs/user/compositor-settings/kde.mdx`) прямо говорит
    /// «not a first-class compositor integration», окна там идут через
    /// внедряемый по D-Bus скрипт KWin (сами называют его «more fragile»).
    /// И ещё одна ловушка именно Plasma: при каждой смене СВОИХ обоев
    /// Noctalia зовёт `plasma-apply-wallpaperimage`, то есть переписывает
    /// обои самой Plasma, — и делает это независимо от `[wallpaper] enabled`.
    Noctalia,
    /// DankMaterialShell (на Quickshell, MIT).
    DankMaterialShell,
}

impl DesktopShell {
    /// Идентификатор для настроек и журнала.
    pub fn as_str(self) -> &'static str {
        match self {
            DesktopShell::Noctalia => "noctalia",
            DesktopShell::DankMaterialShell => "dms",
        }
    }

    /// Имя для покупателя (в тексте на экране идентификатору не место,
    /// правило #13).
    pub fn label(self) -> &'static str {
        match self {
            DesktopShell::Noctalia => "Noctalia",
            DesktopShell::DankMaterialShell => "DankMaterialShell",
        }
    }

    /// Все известные шеллы — чтобы перебор нигде не разъехался с таблицей.
    pub fn all() -> &'static [DesktopShell] {
        &[DesktopShell::Noctalia, DesktopShell::DankMaterialShell]
    }
}

impl Serialize for DesktopShell {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

/// Наблюдения за сеансом, по которым узнаётся шелл. Всё — данные: имена
/// файлов, имена юнитов, имена на шине, имена процессов, переменные
/// окружения.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShellFacts<'a> {
    /// Значение `$WAYLAND_DISPLAY` (пусто — не Wayland либо неизвестно).
    pub wayland_display: &'a str,
    /// Имена файлов (без пути) в `$XDG_RUNTIME_DIR`.
    pub runtime_files: &'a [String],
    /// Активные пользовательские юниты systemd — с суффиксом, как их
    /// печатает `systemctl --user`.
    pub active_user_units: &'a [String],
    /// Занятые имена на сессионной шине D-Bus.
    pub dbus_names: &'a [String],
    /// Имена работающих процессов пользователя (без пути и аргументов).
    pub process_names: &'a [String],
    /// Переменные окружения парами имя→значение (нам достаточно тех, что
    /// перечислены в [`SHELL_ENV_MARKERS`]; живой сбор —
    /// [`SHELL_ENV_NAMES`]).
    pub env: &'a [(&'a str, &'a str)],
}

/// Опознанный шелл вместе с тем, ЧЕМ он опознан.
///
/// Список подтверждений — не украшение: при разборе жалобы «обои не
/// применились» он единственный отвечает на вопрос, почему ядро решило, что
/// фон чужой.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShellDetection {
    /// Кто именно.
    pub shell: DesktopShell,
    /// Чем подтверждено, человеческим языком.
    pub evidence: Vec<String>,
}

/// Точное имя управляющего сокета Noctalia в `$XDG_RUNTIME_DIR`.
///
/// ⚠️ При пустом `WAYLAND_DISPLAY` сам Noctalia подставляет `wayland-0`, а
/// при пустом `XDG_RUNTIME_DIR` уходит в `/tmp` — гоняться за этими
/// запасными путями ядру незачем, каталог и имя добывает вызывающий.
pub fn noctalia_socket_name(wayland_display: &str) -> String {
    format!("noctalia-{}.sock", noctalia_display(wayland_display))
}

/// Точное имя файла блокировки Noctalia (по нему шелл держит flock от
/// второго запуска) — та же формула, что у сокета.
pub fn noctalia_lock_name(wayland_display: &str) -> String {
    format!("noctalia-{}.lock", noctalia_display(wayland_display))
}

/// Дисплей в именах файлов Noctalia: пустой шелл заменяет на `wayland-0`.
fn noctalia_display(wayland_display: &str) -> &str {
    if wayland_display.is_empty() {
        "wayland-0"
    } else {
        wayland_display
    }
}

/// Имена на сессионной шине, которые шелл занимает БЕЗУСЛОВНО.
///
/// У Noctalia своего имени вида `dev.noctalia.Noctalia` нет (это только
/// app_id окна), но два служебных он берёт всегда. Условные — не годятся:
/// `dev.noctalia.KWinActiveWindow` бывает только на KDE,
/// `org.kde.StatusNotifierWatcher` — только если имя свободно, а
/// `org.freedesktop.Notifications` не принадлежит никому конкретно (его
/// берут и Noctalia, и DMS, и штатные оболочки).
const SHELL_DBUS_NAMES: &[(DesktopShell, &[&str])] = &[
    (DesktopShell::Noctalia, &["dev.noctalia.Debug", "dev.noctalia.Mpris"]),
    (DesktopShell::DankMaterialShell, &[]),
];

/// Пользовательские юниты, которыми поднимают шелл.
///
/// ⚠️ У Noctalia штатного юнита НЕТ (пакет ставит только бинарь и `.desktop`
/// с `Exec=noctalia --daemon`); `noctalia.service` заводит лишь модуль
/// NixOS. Поэтому отсутствие юнита ничего не доказывает — доказывает только
/// его наличие.
///
/// ⚠️ `dms.service` пакет ставит с `WantedBy=graphical-session.target`. Если
/// человек сделал `systemctl --user enable dms` (а не `add-wants` к своему
/// композитору, как советуют сами авторы), юнит поднимется в ЛЮБОМ сеансе,
/// в том числе в Plasma и GNOME. Поэтому DMS «под Plasma» встретиться может,
/// хотя официально Plasma он не поддерживает, — признак верен и там.
const SHELL_UNITS: &[(DesktopShell, &[&str])] = &[
    (DesktopShell::Noctalia, &["noctalia.service"]),
    (DesktopShell::DankMaterialShell, &["dms.service"]),
];

/// Метка в окружении: имя переменной и, если важно, её ТОЧНОЕ значение.
///
/// Значение нужно там, где саму переменную ставит не только наш шелл:
/// `QS_APP_ID` выставляет общий модуль запуска `dankgo`, а им пользуется не
/// один DMS (у каждой программы на нём свой идентификатор).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvMarker {
    /// Имя переменной.
    pub name: &'static str,
    /// Требуемое значение; `None` — годится любое непустое.
    pub value: Option<&'static str>,
}

/// Переменные окружения, которые шелл ставит своему процессу интерфейса и
/// которые наследуют запущенные ИЗ шелла программы — то есть мы.
///
/// DankMaterialShell: процессу `qs` бэкенд ставит `DMS_SOCKET` и
/// `QS_APP_ID=com.danklinux.dms` (модуль `dankgo`, `shellapp/env.go`), а
/// `DMS_EXECUTABLE` — сам `dms` (`core/cmd/dms/shellapp.go`). Чистки
/// окружения перед запуском программ в шелле нет (запуск идёт через
/// `systemd-run --user --scope`, он окружение наследует), так что все три
/// доходят и до нас. ⚠️ `QS_APP_ID` — именно ПЕРЕМЕННАЯ окружения; прежняя
/// запись здесь («прагма AppId в QML, в окружении её нет») была неверна.
///
/// У Noctalia своих переменных нет: `setenv` в её коде встречается только
/// у хуков и снимков экрана.
pub const SHELL_ENV_MARKERS: &[(DesktopShell, &[EnvMarker])] = &[
    (DesktopShell::Noctalia, &[]),
    (
        DesktopShell::DankMaterialShell,
        &[
            EnvMarker { name: "DMS_SOCKET", value: None },
            EnvMarker { name: "DMS_EXECUTABLE", value: None },
            EnvMarker { name: "QS_APP_ID", value: Some("com.danklinux.dms") },
        ],
    ),
];

/// Имена всех переменных из [`SHELL_ENV_MARKERS`] — что собирать живому
/// вызывающему. Совпадение с таблицей сторожит сверка
/// `tests/desktop_shell.rs`, иначе сбор и таблица разъедутся молча.
pub const SHELL_ENV_NAMES: &[&str] = &["DMS_SOCKET", "DMS_EXECUTABLE", "QS_APP_ID"];

/// Суффиксы служебных файлов DankMaterialShell.
///
/// Имена подтверждены исходниками модуля запуска `dankgo` (тот коммит, что
/// прибит в `core/go.mod` DMS 1.6.2): сокет `danklinux-<pid бэкенда>.sock`
/// (`paths/paths.go`, имя = `ID` приложения + `-<pid>.sock`, `ID` у DMS —
/// `danklinux`), рядом `danklinux-<pid>.pid` (в нём PID процесса `qs`) и
/// `danklinux-<pid>.session` (в нём `WAYLAND_DISPLAY`) —
/// `shellapp/pidfile.go`. При штатном выходе оба файла удаляются.
const DMS_RUNTIME_SUFFIXES: &[&str] = &[".sock", ".pid", ".session"];

/// Значение из [`ShellFacts::env`] по имени переменной.
fn env_value<'a>(facts: &ShellFacts<'a>, name: &str) -> &'a str {
    facts
        .env
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .unwrap_or("")
}

/// Имена шелла из таблицы «шелл → список имён».
fn names_for(table: &'static [(DesktopShell, &'static [&'static str])], shell: DesktopShell)
    -> &'static [&'static str]
{
    table
        .iter()
        .find(|(s, _)| *s == shell)
        .map(|(_, n)| *n)
        .unwrap_or(&[])
}

/// Подтверждения присутствия одного шелла.
///
/// Возвращает (число ДОКАЗАТЕЛЬСТВ, список подтверждений). Разница между
/// доказательством и подтверждением принципиальная: имя процесса
/// доказательством НЕ является, потому что то же имя носит командная
/// строка самого шелла (`noctalia msg …`, `dms ipc call …`) — она живёт
/// миллисекунды и шелла в сеансе не означает. Такие признаки попадают в
/// список, но решения не принимают.
fn shell_signs(shell: DesktopShell, facts: &ShellFacts) -> (usize, Vec<String>) {
    let mut proofs = 0usize;
    let mut evidence: Vec<String> = Vec::new();

    // 1. Служебный файл в каталоге сеанса — его создаёт САМ живой шелл.
    let file = match shell {
        // Управляющий сокет `noctalia-$WAYLAND_DISPLAY.sock` и файл
        // блокировки `noctalia-$WAYLAND_DISPLAY.lock` (по нему шелл держит
        // flock от второго запуска). Сначала — ТОЧНЫЕ имена для нашего
        // дисплея: они и есть самое точное подтверждение. Запасом — любая
        // форма `noctalia-*.sock|.lock`: слепнуть из-за пустого или чужого
        // `WAYLAND_DISPLAY` незачем. Под запас попадает и второй сокет
        // демона, `noctalia-dmenu-$WAYLAND_DISPLAY.sock`; ложного «шелл
        // есть» это не даёт — его тоже создаёт сам демон.
        DesktopShell::Noctalia => {
            let exact = [
                noctalia_socket_name(facts.wayland_display),
                noctalia_lock_name(facts.wayland_display),
            ];
            exact
                .iter()
                .find_map(|e| facts.runtime_files.iter().find(|f| *f == e))
                .or_else(|| {
                    facts.runtime_files.iter().find(|f| {
                        f.starts_with("noctalia-")
                            && (f.ends_with(".sock") || f.ends_with(".lock"))
                    })
                })
        }
        // `danklinux-<pid>.sock` / `.pid` / `.session`. Имя БЕЗ номера
        // процесса (`danklinux.sock`) сервер не создаёт никогда: это лишь
        // запасной путь, по которому клиент `dms` ищет сокет
        // (`core/cmd/dms/server_client.go`). В признаки его не берём.
        DesktopShell::DankMaterialShell => facts.runtime_files.iter().find(|f| {
            let stem_ok = f.starts_with("danklinux-");
            stem_ok && DMS_RUNTIME_SUFFIXES.iter().any(|s| f.ends_with(s))
        }),
    };
    if let Some(name) = file {
        proofs += 1;
        evidence.push(format!("файл {name} в каталоге сеанса"));
    }

    // 2. Занятое имя на сессионной шине. Протухшим не бывает — самый
    //    надёжный признак из всех.
    let bus = names_for(SHELL_DBUS_NAMES, shell);
    if let Some(name) = facts.dbus_names.iter().find(|n| bus.contains(&n.as_str())) {
        proofs += 1;
        evidence.push(format!("имя {name} на шине"));
    }

    // 3. Поднятый пользовательский юнит.
    let units = names_for(SHELL_UNITS, shell);
    if let Some(unit) = facts
        .active_user_units
        .iter()
        .find(|u| units.contains(&u.as_str()))
    {
        proofs += 1;
        evidence.push(format!("запущен юнит {unit}"));
    }

    // 4. Метка в нашем окружении: значит, нас запустили ИЗ шелла.
    let markers = SHELL_ENV_MARKERS
        .iter()
        .find(|(s, _)| *s == shell)
        .map(|(_, m)| *m)
        .unwrap_or(&[]);
    for marker in markers {
        let value = env_value(facts, marker.name);
        let hit = match marker.value {
            Some(want) => value == want,
            None => !value.is_empty(),
        };
        if hit {
            proofs += 1;
            evidence.push(match marker.value {
                Some(want) => format!("переменная {}={want} в окружении", marker.name),
                None => format!("переменная {} в окружении", marker.name),
            });
            break;
        }
    }

    // 5. Имя процесса — только подтверждение (см. докстринг). Имена
    //    процесса интерфейса (`qs`, `quickshell`) не берём вовсе: под ними
    //    ходит любой шелл на Quickshell, и запустить его можно руками.
    let procs: &[&str] = match shell {
        DesktopShell::Noctalia => &["noctalia"],
        DesktopShell::DankMaterialShell => &["dms"],
    };
    for p in procs {
        if facts.process_names.iter().any(|n| n == p) {
            evidence.push(format!("процесс {p}"));
        }
    }

    (proofs, evidence)
}

/// Какой чужой шелл рисует поверх этого композитора. `None` — шелла нет
/// (или он ничем себя не выдал: тогда фон наш, и это верный ответ —
/// рисовать поверх нас некому).
///
/// Каждое доказательство самодостаточно: хватает ОДНОГО — служебного файла в
/// каталоге сеанса, имени на шине, поднятого юнита или метки в окружении.
/// Так и должно быть: шелл поднимают и юнитом, и `.desktop`-автозапуском, и
/// строкой в конфиге композитора, а слой обоев он занимает одинаково.
pub fn detect_shell(facts: &ShellFacts) -> Option<ShellDetection> {
    let mut best: Option<(usize, ShellDetection)> = None;
    for shell in DesktopShell::all() {
        let (proofs, evidence) = shell_signs(*shell, facts);
        if proofs == 0 {
            continue;
        }
        // Двух шеллов разом не бывает, но если признаки нашлись у обоих
        // (остались чужие файлы от прошлого сеанса), берём того, за кого
        // доказательств больше; при равенстве — порядок таблицы. Для
        // главного вопроса — «наш ли фон» — ответ в любом случае один.
        let better = best.as_ref().map_or(true, |(p, _)| proofs > *p);
        if better {
            best = Some((proofs, ShellDetection { shell: *shell, evidence }));
        }
    }
    best.map(|(_, d)| d)
}

/// Итерация по базе для генераторов helper-таблиц (`system_plan.rs`):
/// de_id → пакеты по семействам в порядке вставки.
pub fn de_database_packages(
) -> impl Iterator<Item = (&'static str, &'static [(PkgFamily, &'static [&'static str])])> {
    DE_DATABASE.iter().map(|(k, e)| (*k, e.packages))
}

/// Пакеты пары (de_id, family) по строковому значению семейства — как
/// итерация `db["packages"].items()` в `_de_helper_supports`: None, если
/// DE или семейство отсутствуют.
pub fn de_entry_packages_for(
    de_id: &str,
    family_value: &str,
) -> Option<&'static [&'static str]> {
    de_entry(de_id).and_then(|e| {
        e.packages
            .iter()
            .find(|(f, _)| f.as_str() == family_value)
            .map(|(_, pkgs)| *pkgs)
    })
}

// ============================================================================
// polkit: без pkexec не работает НИ ОДНА привилегированная операция
// ============================================================================
//
// ⚠️ Проверка наличия `pkexec` на весь проект была ровно одна (в установщике
// тем), и она ничего не говорила человеку. Между тем на openSUSE `pkexec` —
// отдельный подпакет, который тянется лишь через `Recommends`: при установке
// системы с `--no-recommends` его нет, и тогда молча не работают установка
// рабочих столов, системных тем, экранов входа — ВСЁ привилегированное.
// Поэтому проверка одна, ранняя и с человеческим объяснением (правило #13).

/// Что показать/записать, если `pkexec` в системе нет. `hint` — «Установите
/// пакет …» из `utils::tool_package_hint("pkexec", family)`; пусто — если
/// имени пакета для семейства не нашлось.
pub fn polkit_missing_message(hint: &str) -> String {
    if hint.is_empty() {
        // TRANSLATORS: Shown when the pkexec program (polkit) is missing. pkexec is a program name.
        crate::i18n::tr(
            "pkexec was not found: Rexon won't be able to do anything that needs \
             administrator rights — installing desktop environments, system \
             themes or login screens.",
        )
    } else {
        // TRANSLATORS: Same as the previous message; %1 is a hint like "Install the polkit package."
        crate::i18n::trf(
            "pkexec was not found: Rexon won't be able to do anything that needs \
             administrator rights — installing desktop environments, system \
             themes or login screens. %1",
            &[&hint],
        )
    }
}

// ============================================================================
// Чистые алгоритмы установщика (PackageManager, только разбор/фильтры)
// ============================================================================
//
// Вывод pacman подаётся замыканием: `Fn(&str) -> Option<String>` — stdout
// команды для пакета при returncode == 0, None при ошибке. Живой вызов
// (`pacman -Qg/-Qi` под LANG=C, LC_ALL=C — русская локаль переводит имена
// полей и ломает разбор) — исполнение при подключении к живому Python.

/// Члены группы пакетов из вывода [`crate::system_plan::package_group_query`].
///
/// `ok` — команда завершилась нулём (не запустилась вовсе → false).
/// Единственный разбор этого вывода на весь проект.
///
/// `None` значит «это не группа»: либо спросить не вышло, либо такой группы
/// нет. `pacman -Qg` на не-группе печатает "error: group … was not found" в
/// stderr, отдаёт код 1 и ПУСТОЙ stdout (проверено на живой машине на
/// `bash`, на несуществующем имени и на `base-devel`, которого в базе Arch
/// давно нет). Пустой вывод при нулевом коде трактуется так же — имя
/// остаётся как есть, и это верно: обычный пакет группой не становится.
///
/// Форма вывода одна — "группа член" в строке; строки короче двух слов
/// выбрасываются, чем отсеивается и любой мусор.
pub fn parse_group_members(ok: bool, stdout: &str) -> Option<Vec<String>> {
    if !ok || stdout.trim().is_empty() {
        return None;
    }
    Some(
        stdout
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let (_group, member) = (parts.next()?, parts.next()?);
                Some(member.to_string())
            })
            .collect(),
    )
}

/// Раскрыть имена групп пакетов в реальные пакеты группы — порт
/// `PackageManager._expand_groups`.
///
/// В `DE_DATABASE` для MATE есть "mate" и "mate-extra" — это группы:
/// `pacman -Q mate` падает (group != package), а вопрос движка о составе
/// группы даёт строки "группа член". `qg_output(pkg)` — stdout этого
/// вопроса при нулевом коде либо None. Успешный, но пустой вывод оставляет
/// пакет как есть; результат дедуплицируется с сохранением порядка.
///
/// Семейства, у которых понятия «группа пакетов» нет, движок отсекает сам
/// ([`crate::system_plan::package_group_query`] отдаёт им None), но ранний
/// выход по Arch оставлен: без него не-Arch платил бы вызовом колбэка на
/// каждое имя.
pub fn expand_groups(
    family: PkgFamily,
    packages: &[String],
    qg_output: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    if family != PkgFamily::Arch {
        return packages.to_vec();
    }
    let mut expanded: Vec<String> = Vec::new();
    for p in packages {
        let out = qg_output(p);
        match parse_group_members(out.is_some(), out.as_deref().unwrap_or_default()) {
            Some(members) => expanded.extend(members),
            None => expanded.push(p.clone()),
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    expanded.into_iter().filter(|p| seen.insert(p.clone())).collect()
}

/// Прочитать одно поле из stdout `pacman -Qi <pkg>` — чистая часть
/// `PackageManager._qi_field` (LANG=C: поля "Required By" | "Depends On" |
/// "Provides" | …). Возвращает [] если поле пустое ("None") или не найдено.
///
/// Чистка версионных констрейнтов режет имя по САМОМУ ЛЕВОМУ из разделителей
/// ("=", ">", "<"). Так "gtk3>=3.24.0" даёт "gtk3".
///
/// Починен баг оригинала: перебор `for sep in "=><"` брал первый сепаратор,
/// который вообще присутствует в строке (для "gtk3>=…" это "=" на поз.5),
/// а не самый левый ("&gt;" на поз.4), из-за чего получалось "gtk3>".
/// Из-за этого транзитивная защита удаления DE не распознавала пакеты с
/// версионной зависимостью (`foo>=1.2` — gtk3, qt6-base, lightdm), и
/// предохранитель против удаления активного DE для них не срабатывал.
pub fn parse_qi_field(stdout: &str, field: &str) -> Vec<String> {
    for line in stdout.lines() {
        if !line.starts_with(field) {
            continue;
        }
        let Some((_, rest)) = line.split_once(':') else {
            continue;
        };
        let items: Vec<&str> = rest.split_whitespace().collect();
        if items.is_empty() || items == ["None"] {
            return Vec::new();
        }
        let mut cleaned: Vec<String> = Vec::new();
        for item in items {
            let it = match item.find(['=', '>', '<']) {
                Some(pos) => &item[..pos],
                None => item,
            };
            cleaned.push(it.to_string());
        }
        return cleaned;
    }
    Vec::new()
}

/// Порт `PackageManager._qi_field` целиком: не-Arch → [], ошибка команды →
/// []. `qi_output(pkg)` — stdout `pacman -Qi pkg` или None.
pub fn qi_field(
    family: PkgFamily,
    package: &str,
    field: &str,
    qi_output: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    if family != PkgFamily::Arch {
        return Vec::new();
    }
    match qi_output(package) {
        Some(out) => parse_qi_field(&out, field),
        None => Vec::new(),
    }
}

/// Какие установленные пакеты ВНЕ `candidate_set` требуют `package` — порт
/// `PackageManager._required_by_outside`.
pub fn required_by_outside(
    family: PkgFamily,
    package: &str,
    candidate_set: &HashSet<String>,
    qi_output: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    qi_field(family, package, "Required By", qi_output)
        .into_iter()
        .filter(|d| !candidate_set.contains(d))
        .collect()
}

/// Пакеты стола по таблице ядра: основные и AUR — тот же набор, что
/// удаляет `remove_de` (`system_plan::remove_de_decision`).
pub fn desktop_packages(de_id: &str, family: PkgFamily) -> Vec<String> {
    let mut all = get_packages(de_id, family);
    for p in get_aur_packages(de_id, family) {
        if !all.contains(&p) {
            all.push(p);
        }
    }
    all
}

/// Пакет экрана входа по имени его юнита: обычно совпадают; исключения —
/// таблицы ядра (`plasmalogin` → `plasma-login-manager`, на Debian `gdm` →
/// `gdm3`).
pub fn dm_package_for_unit(family: PkgFamily, unit: &str) -> String {
    let table = if family == PkgFamily::Debian {
        crate::system_plan::DM_DEBIAN_PKG_FOR_UNIT
    } else {
        crate::system_plan::DM_PKG_FOR_UNIT
    };
    table
        .iter()
        .find(|(u, _)| *u == unit)
        .map(|(_, p)| p.to_string())
        .unwrap_or_else(|| unit.to_string())
}

/// Приветствие LightDM (`lightdm-gtk-greeter`, `lightdm-slick-greeter`, …):
/// сам `lightdm` ни от одного не зависит, и удаление стола, принёсшего
/// выбранное приветствие, оставило бы экран входа без лица — служба не
/// стартует. Пока активный экран входа — LightDM, ни одно приветствие не
/// снимается.
pub fn is_lightdm_greeter(package: &str) -> bool {
    package.starts_with("lightdm-") && package.contains("greeter")
}

/// Что удаление ДРУГОГО стола не трогает никогда: пакеты текущего стола по
/// таблице ядра (основные и AUR) и активный экран входа системы — его юнит,
/// пакет ([`dm_package_for_unit`]) и, у LightDM, приветствия
/// ([`is_lightdm_greeter`]; их сверяет [`without_current_desktop`]).
///
/// ⛔ Дыра, которую это закрыло (28.10.2026): защита `remove_de` сравнивала
/// только идентификаторы столов. Сидя в Xfce, удаление Cinnamon снимало
/// `lightdm` — общий экран входа обоих, — и после перезагрузки входа в
/// графику не было. Замыкание зависимостей его не держало: `lightdm` никому
/// не нужен по `Required By`.
///
/// `current_de` неизвестен (`""`, `unknown`) — таблицы нет, остаётся экран
/// входа; `active_dm_unit` — ответ `active_login_manager_opt` (`None` —
/// определить нечем).
pub fn current_desktop_keep(
    family: PkgFamily,
    current_de: &str,
    active_dm_unit: Option<&str>,
) -> Vec<String> {
    let mut keep = desktop_packages(current_de.trim(), family);
    if let Some(unit) = active_dm_unit.map(str::trim).filter(|u| !u.is_empty()) {
        for p in [unit.to_string(), dm_package_for_unit(family, unit)] {
            if !keep.contains(&p) {
                keep.push(p);
            }
        }
    }
    keep
}

/// Набор удаления стола без того, что держит текущий стол: группы Arch
/// раскрываются с ОБЕИХ сторон (`qg` — вывод `pacman -Qg`, как у
/// [`expand_groups`]), и член общей группы тоже под защитой; приветствия
/// LightDM остаются, пока в `keep` есть `lightdm`. Порядок — как в `all`.
pub fn without_current_desktop(
    family: PkgFamily,
    all: &[String],
    keep: &[String],
    qg_output: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    // Сверка по имени без категории Gentoo: юнит `lightdm` держит и
    // `x11-misc/lightdm` из таблицы.
    let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
    let mut held: HashSet<String> = keep.iter().map(|p| base(p)).collect();
    held.extend(expand_groups(family, keep, qg_output).iter().map(|p| base(p)));
    let greeters = held.contains("lightdm");
    expand_groups(family, all, qg_output)
        .into_iter()
        .filter(|p| {
            let b = base(p);
            !held.contains(&b) && !(greeters && is_lightdm_greeter(&b))
        })
        .collect()
}

/// Двухпроходный фильтр для безопасного удаления (Arch only) — порт
/// `PackageManager._filter_removable_transitive`. Возвращает (safe, skipped)
/// с сохранением исходного порядка `packages`.
///
/// 1. Прямые external deps: пакет требуется кем-то ВНЕ списка → forbidden.
/// 2. Транзитивно: если forbidden пакет X зависит от Y (Y в списке) — Y тоже
///    forbidden, иначе pacman упадёт «could not satisfy». Замыкание —
///    до неподвижной точки; цикл в зависимостях не зацикливает (проверено
///    golden-кейсом `dependency-cycle`).
pub fn filter_removable_transitive(
    family: PkgFamily,
    packages: &[String],
    qi_output: &dyn Fn(&str) -> Option<String>,
) -> (Vec<String>, Vec<String>) {
    if family != PkgFamily::Arch {
        return (packages.to_vec(), Vec::new());
    }
    let candidate: HashSet<String> = packages.iter().cloned().collect();
    let mut forbidden: HashSet<String> = HashSet::new();
    for p in packages {
        if !required_by_outside(family, p, &candidate, qi_output).is_empty() {
            forbidden.insert(p.clone());
        }
    }
    // Транзитивное замыкание: deps forbidden-пакетов, попавшие в candidate
    let mut changed = true;
    while changed {
        changed = false;
        for p in forbidden.clone() {
            let deps: HashSet<String> =
                qi_field(family, &p, "Depends On", qi_output).into_iter().collect();
            for d in deps.intersection(&candidate) {
                if !forbidden.contains(d) {
                    forbidden.insert(d.clone());
                    changed = true;
                }
            }
        }
    }
    let safe = packages.iter().filter(|p| !forbidden.contains(*p)).cloned().collect();
    let skipped = packages.iter().filter(|p| forbidden.contains(*p)).cloned().collect();
    (safe, skipped)
}
